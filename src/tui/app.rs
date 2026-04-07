use anyhow::Result;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::db::Db;
use crate::sources;
use super::theme::{Theme, ThemeName};

/// How long transient status messages (theme change, clipboard copy) remain visible.
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(PartialEq)]
pub enum Screen {
    Browser,
    Reader,
}

pub struct App {
    pub cfg: Config,
    pub db: Db,
    pub theme: Theme,
    pub theme_name: ThemeName,
    pub screen: Screen,

    // ── Browser state ──────────────────────────────────────────────────────────
    /// All doc aliases for the browser list
    pub doc_aliases: Vec<String>,
    /// Currently selected index in browser list
    pub selected: usize,
    /// Current filter string (from `/` search)
    pub filter: String,
    /// Whether filter input is active
    pub filtering: bool,

    // ── Preview state (browser right pane) ────────────────────────────────────
    /// Alias currently loaded in the preview pane (to avoid redundant reloads)
    pub preview_alias: Option<String>,
    /// Markdown content for the preview pane (None = not yet loaded / loading)
    pub preview_content: Option<String>,

    // ── Reader state ──────────────────────────────────────────────────────────
    /// Content being displayed in the reader
    pub reader_content: String,
    /// Title for the reader (alias)
    pub reader_title: String,
    /// Scroll offset in reader
    pub reader_scroll: u16,
    /// Total rendered lines (used for scrollbar percentage)
    pub reader_total_lines: u16,
    /// Table-of-contents entries: (heading_level 1-6, heading_text)
    pub toc: Vec<(u16, String)>,
    /// Whether the ToC sidebar is visible
    pub toc_visible: bool,

    // ── Global ─────────────────────────────────────────────────────────────────
    /// Status message shown at bottom (errors, notifications)
    pub status: String,
    /// When Some, the status message will be cleared after this instant.
    pub status_expires_at: Option<Instant>,
}

impl App {
    pub fn new(cfg: Config, db: Db) -> Self {
        let doc_aliases = db
            .list_docs(None)
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.alias)
            .collect();

        let theme_name = ThemeName::from_str(&cfg.ui.theme).unwrap_or_default();
        let theme = theme_name.to_theme();

        Self {
            cfg,
            db,
            theme,
            theme_name,
            screen: Screen::Browser,
            doc_aliases,
            selected: 0,
            filter: String::new(),
            filtering: false,
            preview_alias: None,
            preview_content: None,
            reader_content: String::new(),
            reader_title: String::new(),
            reader_scroll: 0,
            reader_total_lines: 0,
            toc: Vec::new(),
            toc_visible: true,
            status: String::new(),
            status_expires_at: None,
        }
    }

    /// Filtered view of aliases based on current filter string.
    pub fn filtered_aliases(&self) -> Vec<&str> {
        if self.filter.is_empty() {
            self.doc_aliases.iter().map(|s| s.as_str()).collect()
        } else {
            let q = self.filter.to_lowercase();
            self.doc_aliases
                .iter()
                .filter(|a| a.to_lowercase().contains(&q))
                .map(|s| s.as_str())
                .collect()
        }
    }

    /// Load the preview for the currently selected alias if it has changed.
    /// Should be called from handle_key after cursor movement.
    pub async fn load_preview(&mut self) -> Result<()> {
        let aliases = self.filtered_aliases();
        let alias = match aliases.get(self.selected.min(aliases.len().saturating_sub(1))) {
            Some(a) => a.to_string(),
            None => {
                self.preview_alias = None;
                self.preview_content = None;
                return Ok(());
            }
        };

        // Skip if already loaded for this alias
        if self.preview_alias.as_deref() == Some(&alias) {
            return Ok(());
        }

        self.preview_alias = Some(alias.clone());
        self.preview_content = None; // show "Loading…" state while fetching

        match sources::fetch(&self.cfg, &self.db, &alias, true).await {
            Ok(content) => {
                // Only apply if the selection hasn't changed in the meantime
                if self.preview_alias.as_deref() == Some(&alias) {
                    self.preview_content = Some(content);
                }
            }
            Err(e) => {
                self.preview_content = Some(format!("_Error loading preview: {}_", e));
            }
        }

        Ok(())
    }

    /// Open a doc by alias into the reader screen.
    pub async fn open_doc(&mut self, alias: &str) -> Result<()> {
        self.status = format!("Loading '{alias}'...");

        // Reuse preview content if already loaded for this alias (avoids duplicate fetch)
        let content = if self.preview_alias.as_deref() == Some(alias) {
            if let Some(c) = self.preview_content.clone() {
                Ok(c)
            } else {
                sources::fetch(&self.cfg, &self.db, alias, true).await
            }
        } else {
            sources::fetch(&self.cfg, &self.db, alias, true).await
        };

        match content {
            Ok(content) => {
                self.toc = super::markdown::extract_headings(&content);
                self.reader_content = content;
                self.reader_title = alias.to_string();
                self.reader_scroll = 0;
                self.reader_total_lines = 0;
                self.screen = Screen::Reader;
                self.status = String::new();
            }
            Err(e) => {
                self.status = format!("Error: {e}");
            }
        }
        Ok(())
    }

    pub fn go_back(&mut self) {
        self.screen = Screen::Browser;
        self.reader_scroll = 0;
    }

    /// Advance to the next theme in the rotation cycle.
    pub fn cycle_theme(&mut self) {
        self.theme_name = self.theme_name.next();
        self.theme = self.theme_name.to_theme();
        self.set_transient_status(format!("Theme: {}", self.theme_name.display()));
    }

    /// Set a status message that auto-clears after STATUS_TIMEOUT.
    pub fn set_transient_status(&mut self, msg: String) {
        self.status = msg;
        self.status_expires_at = Some(Instant::now() + STATUS_TIMEOUT);
    }

    /// Expire the status message if its timeout has elapsed.
    pub fn tick_status(&mut self) {
        if let Some(expires) = self.status_expires_at {
            if Instant::now() >= expires {
                self.status.clear();
                self.status_expires_at = None;
            }
        }
    }
}
