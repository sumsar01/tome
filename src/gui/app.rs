use std::sync::mpsc;

use egui::Context;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::{
    config::{self, Config},
    db::Db,
    sources,
    tui::{markdown::extract_headings, theme::ThemeName},
};

use super::theme::GuiTheme;

/// Message sent back from a background fetch task.
pub struct FetchResult {
    pub alias: String,
    pub content: Result<String, String>,
}

/// Which panel is focused for keyboard routing.
#[derive(Clone, PartialEq, Default)]
pub enum Focus {
    #[default]
    Browser,
    Filter,
    Reader,
}

/// Central application state for the egui GUI.
pub struct GuiApp {
    // ── Core ──────────────────────────────────────────────────────────────────
    pub cfg: Config,
    pub db: Db,

    // ── Theme ─────────────────────────────────────────────────────────────────
    pub theme_name: ThemeName,
    pub gui_theme: GuiTheme,

    // ── Browser state ─────────────────────────────────────────────────────────
    pub doc_aliases: Vec<String>,
    pub selected: Option<usize>,
    pub filter: String,
    pub focus: Focus,

    // ── Reader state ──────────────────────────────────────────────────────────
    /// `None` = browser view, `Some(_)` = reader open.
    pub reader_content: Option<String>,
    pub reader_title: String,
    pub reader_loading: bool,
    /// Cached alias currently loaded in the reader (avoids re-fetching same doc).
    pub reader_alias: Option<String>,
    /// Pending scroll delta (px) to apply next frame; reset to 0 after applied.
    pub reader_scroll_delta: f32,
    /// Current absolute scroll offset, tracked so we can jump to top/bottom.
    pub reader_scroll_offset: f32,
    /// Set to true for one frame to jump to the very top.
    pub reader_scroll_to_top: bool,
    /// Set to true for one frame to jump to the very bottom.
    pub reader_scroll_to_bottom: bool,

    // ── Table of contents ─────────────────────────────────────────────────────
    pub toc: Vec<(u16, String)>,
    pub toc_visible: bool,
    /// Heading requested to scroll to (set by clicking ToC entry).
    pub toc_scroll_to: Option<usize>,

    // ── Status bar ────────────────────────────────────────────────────────────
    pub status: String,
    pub status_expires: Option<std::time::Instant>,

    // ── Background fetch channel ───────────────────────────────────────────────
    fetch_tx: mpsc::SyncSender<FetchResult>,
    pub fetch_rx: mpsc::Receiver<FetchResult>,

    // ── egui_commonmark cache ─────────────────────────────────────────────────
    pub cm_cache: egui_commonmark::CommonMarkCache,
}

impl GuiApp {
    pub fn new(cfg: Config, db: Db) -> Self {
        let doc_aliases: Vec<String> = db
            .list_docs(None)
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.alias)
            .collect();

        // Start with the first item selected so j/k navigation works immediately.
        let selected = if doc_aliases.is_empty() { None } else { Some(0) };

        let theme_name = ThemeName::from_str(&cfg.ui.theme).unwrap_or_default();
        let gui_theme = GuiTheme::from_name(&theme_name);

        let (fetch_tx, fetch_rx) = mpsc::sync_channel(32);

        Self {
            cfg,
            db,
            theme_name,
            gui_theme,
            doc_aliases,
            selected,
            filter: String::new(),
            focus: Focus::Browser,
            reader_content: None,
            reader_title: String::new(),
            reader_loading: false,
            reader_alias: None,
            reader_scroll_delta: 0.0,
            reader_scroll_offset: 0.0,
            reader_scroll_to_top: false,
            reader_scroll_to_bottom: false,
            toc: Vec::new(),
            toc_visible: true,
            toc_scroll_to: None,
            status: String::new(),
            status_expires: None,
            fetch_tx,
            fetch_rx,
            cm_cache: egui_commonmark::CommonMarkCache::default(),
        }
    }

    // ── Filtered alias list ────────────────────────────────────────────────────

    pub fn filtered_aliases(&self) -> Vec<&str> {
        if self.filter.is_empty() {
            return self.doc_aliases.iter().map(|s| s.as_str()).collect();
        }
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(&str, i64)> = self
            .doc_aliases
            .iter()
            .filter_map(|alias| {
                matcher
                    .fuzzy_match(alias, &self.filter)
                    .map(|score| (alias.as_str(), score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().map(|(a, _)| a).collect()
    }

    // ── Document loading ───────────────────────────────────────────────────────

    /// Trigger an async fetch of `alias`. Results arrive via `fetch_rx`.
    pub fn open_doc(&mut self, alias: &str, ctx: Context) {
        // Skip re-fetching if already loaded.
        if self.reader_alias.as_deref() == Some(alias) && self.reader_content.is_some() {
            return;
        }

        self.reader_title = alias.to_string();
        self.reader_loading = true;
        self.reader_content = None;
        self.reader_alias = Some(alias.to_string());
        self.toc = Vec::new();
        self.reader_scroll_to_top = true; // reset scroll when opening a new doc

        let alias = alias.to_string();
        let cfg = self.cfg.clone();
        let db = self.db.clone();
        let tx = self.fetch_tx.clone();

        tokio::spawn(async move {
            let result = sources::fetch(&cfg, &db, &alias, false)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(FetchResult { alias, content: result });
            ctx.request_repaint();
        });
    }

    /// Drain completed fetch results from the channel.
    pub fn poll_fetches(&mut self) {
        while let Ok(msg) = self.fetch_rx.try_recv() {
            if self.reader_alias.as_deref() == Some(&msg.alias) {
                match msg.content {
                    Ok(content) => {
                        self.toc = extract_headings(&content);
                        self.reader_content = Some(content);
                        self.reader_loading = false;
                    }
                    Err(e) => {
                        self.reader_loading = false;
                        self.set_status(format!("Error: {e}"));
                    }
                }
            }
        }
    }

    // ── Theme ─────────────────────────────────────────────────────────────────

    pub fn cycle_theme(&mut self, ctx: &Context) {
        self.theme_name = self.theme_name.next();
        self.gui_theme = GuiTheme::from_name(&self.theme_name);
        ctx.set_global_style(self.gui_theme.to_egui_style());
        self.set_status(format!("Theme: {}", self.theme_name.display()));
    }

    // ── Status bar ────────────────────────────────────────────────────────────

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_expires =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
    }

    pub fn tick_status(&mut self) {
        if let Some(exp) = self.status_expires {
            if std::time::Instant::now() >= exp {
                self.status.clear();
                self.status_expires = None;
            }
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    pub fn go_back(&mut self) {
        self.reader_content = None;
        self.reader_loading = false;
        self.reader_scroll_delta = 0.0;
        self.focus = Focus::Browser;
    }

    pub fn reader_is_open(&self) -> bool {
        self.reader_content.is_some() || self.reader_loading
    }

    /// Pre-warm credential caches so keychain prompts fire before the GUI window
    /// opens (same behaviour as the TUI). Errors are silently ignored — sources
    /// that don't need credentials are unaffected.
    pub fn prewarm_credentials() {
        let _ = config::auth::get_github_token();
        let _ = config::auth::get_confluence_credentials();
    }
}
