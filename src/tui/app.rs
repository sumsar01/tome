use anyhow::Result;
use std::collections::HashSet;
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
    History,
}

/// A row in the grouped browser list — either a category header or a doc entry.
#[derive(Debug, Clone, PartialEq)]
pub enum ListRow {
    Header(String),
    Doc(String),
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
    /// Currently selected index in browser list (counts header rows + doc rows)
    pub selected: usize,
    /// Current filter string (from `/` search)
    pub filter: String,
    /// Whether filter input is active
    pub filtering: bool,
    /// Set of category names that are currently collapsed
    pub collapsed_categories: HashSet<String>,

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

    // ── History/diff overlay ───────────────────────────────────────────────────
    /// Version history entries for history screen
    pub history_entries: Vec<crate::db::DocVersion>,
    /// Selected index in history list
    pub history_selected: usize,
    /// Diff text to display (None = show history list, Some = show diff)
    pub diff_content: Option<String>,
    /// Whether to show the metadata info overlay
    pub show_info: bool,
}

impl App {
    pub fn new(cfg: Config, db: Db) -> Self {
        let doc_aliases = db
            .list_docs(None, None)
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
            collapsed_categories: HashSet::new(),
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
            history_entries: Vec::new(),
            history_selected: 0,
            diff_content: None,
            show_info: false,
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

    /// Build the grouped list rows for the browser (header + doc rows, respecting collapse).
    /// When a filter is active, returns flat filtered doc rows only (no headers).
    pub fn build_list_rows(&self) -> Vec<ListRow> {
        if !self.filter.is_empty() {
            // Filter mode: flat list, no headers
            return self
                .filtered_aliases()
                .into_iter()
                .map(|a| ListRow::Doc(a.to_string()))
                .collect();
        }

        // Grouped mode: fetch full DocInfo to get categories
        let docs = self.db.list_docs(None, None).unwrap_or_default();

        // Collect unique categories in sorted order; "Uncategorized" last
        let mut categories: Vec<String> = {
            let mut seen = std::collections::BTreeSet::new();
            for d in &docs {
                if let Some(ref cat) = d.category {
                    seen.insert(cat.clone());
                }
            }
            seen.into_iter().collect()
        };
        // Add uncategorized group at end if any docs lack a category
        let has_uncategorized = docs.iter().any(|d| d.category.is_none());

        let mut rows: Vec<ListRow> = Vec::new();

        for cat in &categories {
            rows.push(ListRow::Header(cat.clone()));
            if !self.collapsed_categories.contains(cat) {
                for d in docs.iter().filter(|d| d.category.as_deref() == Some(cat.as_str())) {
                    rows.push(ListRow::Doc(d.alias.clone()));
                }
            }
        }

        if has_uncategorized {
            let label = "Uncategorized".to_string();
            rows.push(ListRow::Header(label.clone()));
            if !self.collapsed_categories.contains(&label) {
                for d in docs.iter().filter(|d| d.category.is_none()) {
                    rows.push(ListRow::Doc(d.alias.clone()));
                }
            }
        }

        rows
    }

    /// Toggle collapse state for a category header.
    pub fn toggle_category(&mut self, category: &str) {
        if self.collapsed_categories.contains(category) {
            self.collapsed_categories.remove(category);
        } else {
            self.collapsed_categories.insert(category.to_string());
        }
    }

    /// Return the alias at the current selected position, if the row is a Doc row.
    pub fn selected_alias(&self) -> Option<String> {
        let rows = self.build_list_rows();
        match rows.get(self.selected) {
            Some(ListRow::Doc(alias)) => Some(alias.clone()),
            _ => None,
        }
    }

    /// Return the category header at the current selected position, if any.
    pub fn selected_header(&self) -> Option<String> {
        let rows = self.build_list_rows();
        match rows.get(self.selected) {
            Some(ListRow::Header(cat)) => Some(cat.clone()),
            _ => None,
        }
    }

    /// Load the preview for the currently selected alias if it has changed.
    /// Should be called from handle_key after cursor movement.
    pub async fn load_preview(&mut self) -> Result<()> {
        let alias = match self.selected_alias() {
            Some(a) => a,
            None => {
                // Selected row is a header or list is empty — clear preview
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
        match self.screen {
            Screen::History => {
                self.screen = Screen::Reader;
                self.diff_content = None;
            }
            _ => {
                self.screen = Screen::Browser;
                self.reader_scroll = 0;
            }
        }
    }

    /// Open the history overlay for the current reader doc.
    pub fn open_history(&mut self) {
        match self.db.list_versions(&self.reader_title) {
            Ok(versions) => {
                self.history_entries = versions;
                self.history_selected = self.history_entries.len().saturating_sub(1);
                self.diff_content = None;
                self.screen = Screen::History;
            }
            Err(e) => {
                self.set_transient_status(format!("Error loading history: {e}"));
            }
        }
    }

    /// Show diff for the selected history entry vs the previous one.
    pub fn show_diff_for_selected(&mut self) {
        let n = self.history_entries.len();
        if n < 2 {
            self.set_transient_status("Need at least 2 versions to diff.".to_string());
            return;
        }
        let idx2 = self.history_selected;
        let idx1 = if idx2 == 0 { 1 } else { idx2 - 1 };
        let a = &self.history_entries[idx1];
        let b = &self.history_entries[idx2];
        self.diff_content = Some(crate::mcp::unified_diff_string_pub(&self.reader_title, a, b));
    }

    /// Return the filesystem path for the currently displayed doc if it is local,
    /// so the TUI event loop can set up a file watcher.
    pub fn current_local_path(&self) -> Option<String> {
        let alias = match self.screen {
            Screen::Reader | Screen::History => &self.reader_title,
            Screen::Browser => self.preview_alias.as_deref().unwrap_or(""),
        };
        if alias.is_empty() {
            return None;
        }
        let doc = self.db.find_doc(alias).ok()??;
        if doc.source == crate::db::SOURCE_INLINE {
            return None; // inline — not on disk as a separate file
        }
        // Local-sourced docs have a path field
        if doc.source == "local" {
            return doc.path;
        }
        // For local source with path configured via SourceConfig.root + doc.path
        if self.cfg.find_source(&doc.source).map(|s| s.kind == crate::config::SourceKind::Local).unwrap_or(false) {
            if let Some(root) = self.cfg.find_source(&doc.source).and_then(|s| s.root.as_deref()) {
                return Some(format!("{}/{}", root.trim_end_matches('/'), doc.path.unwrap_or_default()));
            }
        }
        None
    }

    /// Reload the current doc content from disk (called on file-system change event).
    pub async fn reload_current_local(&mut self) {
        match self.screen {
            Screen::Browser => {
                // Force preview re-fetch
                self.preview_alias = None;
                let _ = self.load_preview().await;
            }
            Screen::Reader | Screen::History => {
                let alias = self.reader_title.clone();
                if let Ok(content) = sources::fetch(&self.cfg, &self.db, &alias, false).await {
                    self.toc = super::markdown::extract_headings(&content);
                    self.reader_content = content;
                    self.set_transient_status("Reloaded.".to_string());
                }
            }
        }
    }

    /// Open the source URL for the current reader doc in the default browser.
    pub fn open_source_in_browser(&mut self) {
        let alias = &self.reader_title;
        match self.db.find_doc(alias) {
            Ok(Some(doc)) => {
                match crate::source_url(&self.cfg, &doc) {
                    Ok(url) => {
                        if let Err(e) = crate::open_in_browser(&url) {
                            self.set_transient_status(format!("{e}"));
                        } else {
                            self.set_transient_status(format!("Opened {}", url));
                        }
                    }
                    Err(e) => self.set_transient_status(format!("{e}")),
                }
            }
            _ => self.set_transient_status("Doc not found.".to_string()),
        }
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

    /// Return a help-bar `Line` that shows the status message if one is active,
    /// or the provided keybinding hints otherwise.
    ///
    /// This consolidates the identical `if !app.status.is_empty() { … } else { … }`
    /// pattern shared by browser, reader, and history draw functions.
    pub fn status_or_help<'a>(&self, theme: &'a super::theme::Theme, bindings: &[(&'a str, &'a str)]) -> ratatui::text::Line<'a> {
        if !self.status.is_empty() {
            ratatui::text::Line::from(vec![
                ratatui::text::Span::raw("  "),
                ratatui::text::Span::styled(self.status.clone(), theme.status_style()),
            ])
        } else {
            theme.help_bar(bindings)
        }
    }
}
