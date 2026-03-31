use anyhow::Result;

use crate::config::Config;
use crate::sources;

pub enum Screen {
    Browser,
    Reader,
}

pub struct App {
    pub cfg: Config,
    pub screen: Screen,
    /// All doc aliases for the browser list
    pub doc_aliases: Vec<String>,
    /// Currently selected index in browser list
    pub selected: usize,
    /// Current filter string (from `/` search)
    pub filter: String,
    /// Whether filter input is active
    pub filtering: bool,
    /// Content being displayed in the reader
    pub reader_content: String,
    /// Title for the reader (alias)
    pub reader_title: String,
    /// Scroll offset in reader
    pub reader_scroll: u16,
    /// Status message shown at bottom
    pub status: String,
}

impl App {
    pub fn new(cfg: Config) -> Self {
        let doc_aliases = cfg.docs.iter().map(|d| d.alias.clone()).collect();
        Self {
            cfg,
            screen: Screen::Browser,
            doc_aliases,
            selected: 0,
            filter: String::new(),
            filtering: false,
            reader_content: String::new(),
            reader_title: String::new(),
            reader_scroll: 0,
            status: String::new(),
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

    /// Open a doc by alias into the reader screen.
    pub async fn open_doc(&mut self, alias: &str) -> Result<()> {
        self.status = format!("Loading '{alias}'...");
        match sources::fetch(&self.cfg, alias, true).await {
            Ok(content) => {
                self.reader_content = content;
                self.reader_title = alias.to_string();
                self.reader_scroll = 0;
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
}
