pub mod auth;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::db::SOURCE_INLINE;
use crate::paths;

/// Backup configuration — auto-export docs to a file after every write.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackupConfig {
    /// Path to write the JSON export (e.g. "/path/to/git-repo/tome-backup.json").
    /// If empty or not set, backups are disabled.
    #[serde(default)]
    pub path: String,
    /// Automatically run `git add <path> && git commit` after each export.
    #[serde(default = "default_true")]
    pub git: bool,
    /// Also run `git push` after committing (default: false).
    #[serde(default)]
    pub git_push: bool,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            git: true,
            git_push: false,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Top-level config loaded from the platform config file.
/// Contains only sources and cache settings.
/// Doc registrations live in the SQLite database (`tome.db`).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub backup: BackupConfig,
    /// Legacy [[docs]] entries — read during migration only, then removed from disk.
    #[serde(default, skip_serializing)]
    pub docs: Vec<DocConfig>,
    /// Path from which this config was loaded. Used when rewriting the file.
    /// Not serialised — set programmatically by `load_from`.
    #[serde(skip)]
    pub loaded_from: PathBuf,
}

/// UI / appearance settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    /// Name of the built-in theme to use on startup.
    /// Valid values: dark | light | catppuccin | gruvbox | nord | solarized-dark
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Keybinding overrides. Any omitted action keeps its default key.
    #[serde(default)]
    pub keys: KeysConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            keys: KeysConfig::default(),
        }
    }
}

fn default_theme() -> String {
    "dark".to_string()
}

/// Keybinding configuration for TUI actions.
///
/// Each field is a single character string (e.g. `"j"`) or a special key name
/// (`"up"`, `"down"`, `"pageup"`, `"pagedown"`, `"enter"`, `"esc"`, `"backspace"`).
/// Invalid values produce a clear error at startup.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KeysConfig {
    // Reader keys
    pub scroll_down: String,
    pub scroll_up: String,
    pub toggle_toc: String,
    pub copy: String,
    pub history: String,
    pub open_url: String,
    pub info: String,
    pub cycle_theme: String,
    // Browser keys
    pub filter: String,
    pub navigate_down: String,
    pub navigate_up: String,
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            scroll_down: "j".to_string(),
            scroll_up: "k".to_string(),
            toggle_toc: "t".to_string(),
            copy: "y".to_string(),
            history: "h".to_string(),
            open_url: "o".to_string(),
            info: "i".to_string(),
            cycle_theme: "T".to_string(),
            filter: "/".to_string(),
            navigate_down: "j".to_string(),
            navigate_up: "k".to_string(),
        }
    }
}

impl KeysConfig {
    /// Parse a key string into a crossterm `KeyCode`.
    /// Returns an error with a clear message if the string is unrecognised.
    pub fn parse_key(s: &str) -> anyhow::Result<crossterm::event::KeyCode> {
        use crossterm::event::KeyCode;
        match s {
            "up" => Ok(KeyCode::Up),
            "down" => Ok(KeyCode::Down),
            "pageup" | "pgup" => Ok(KeyCode::PageUp),
            "pagedown" | "pgdn" => Ok(KeyCode::PageDown),
            "enter" => Ok(KeyCode::Enter),
            "esc" | "escape" => Ok(KeyCode::Esc),
            "backspace" => Ok(KeyCode::Backspace),
            "tab" => Ok(KeyCode::Tab),
            "home" => Ok(KeyCode::Home),
            "end" => Ok(KeyCode::End),
            "delete" | "del" => Ok(KeyCode::Delete),
            s if s.chars().count() == 1 => Ok(KeyCode::Char(s.chars().next().unwrap())),
            other => anyhow::bail!(
                "Unknown key '{}' in [ui.keys] config. Use a single character or one of: \
                 up, down, pageup, pagedown, enter, esc, backspace, tab, home, end, delete",
                other
            ),
        }
    }

    /// Validate all key strings in this config, returning the first error found.
    pub fn validate(&self) -> anyhow::Result<()> {
        let fields = [
            ("scroll_down", &self.scroll_down),
            ("scroll_up", &self.scroll_up),
            ("toggle_toc", &self.toggle_toc),
            ("copy", &self.copy),
            ("history", &self.history),
            ("open_url", &self.open_url),
            ("info", &self.info),
            ("cycle_theme", &self.cycle_theme),
            ("filter", &self.filter),
            ("navigate_down", &self.navigate_down),
            ("navigate_up", &self.navigate_up),
        ];
        for (name, val) in &fields {
            Self::parse_key(val).map_err(|e| anyhow::anyhow!("[ui.keys].{}: {}", name, e))?;
        }
        Ok(())
    }
}

/// A named source (GitHub repo, Confluence space, local directory)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceConfig {
    /// Unique name used in [[docs]] entries
    pub name: String,
    /// Source type: "github" | "confluence" | "local"
    #[serde(rename = "type")]
    pub kind: SourceKind,
    /// GitHub: "owner/repo"
    pub repo: Option<String>,
    /// GitHub: subdirectory within repo (optional)
    pub path: Option<String>,
    /// GitHub: branch/tag/sha (default: "main")
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Confluence: base URL e.g. "https://yourco.atlassian.net"
    pub base_url: Option<String>,
    /// Confluence: space key e.g. "ENG"
    pub space: Option<String>,
    /// Local: root directory path
    pub root: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Github,
    Confluence,
    Local,
    /// Legacy — kept for migration compatibility; no longer written to config.
    Inline,
}

/// Legacy doc config entry — only used during migration from config.toml → DB.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DocConfig {
    pub alias: String,
    pub source: String,
    pub path: Option<String>,
    pub page_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    #[serde(default = "default_cache_enabled")]
    pub enabled: bool,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_seconds: default_ttl(),
        }
    }
}

fn default_cache_enabled() -> bool {
    true
}

/// Default cache TTL: 1 hour.
const DEFAULT_TTL_SECONDS: u64 = 3600;

fn default_ttl() -> u64 {
    DEFAULT_TTL_SECONDS
}

impl Config {
    /// Load config from the platform config path, creating it if missing.
    ///
    /// If legacy `[[docs]]` entries are found and no `tome.db` exists yet,
    /// they are migrated automatically and removed from config.toml.
    pub fn load() -> Result<Self> {
        // Honour TOME_PROFILE env var for profile selection.
        let profile = std::env::var("TOME_PROFILE").ok();
        let path = profile
            .as_deref()
            .map(profile_config_path)
            .unwrap_or_else(config_path);
        Self::load_from(&path)
    }

    /// Load config from an explicit path (used by `--config` and `--profile` flags).
    pub fn load_from(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create config dir: {}", parent.display())
                })?;
            }
            std::fs::write(path, DEFAULT_CONFIG)
                .with_context(|| format!("Failed to write default config: {}", path.display()))?;
            eprintln!("Created default config at {}", path.display());
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;

        let cfg: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", path.display()))?;

        // Validate keybindings at load time so users get a clear error immediately.
        cfg.ui
            .keys
            .validate()
            .with_context(|| format!("Invalid keybinding in {}", path.display()))?;

        let mut cfg = cfg;
        cfg.loaded_from = path.clone();
        Ok(cfg)
    }

    /// Find a source config by name.
    pub fn find_source(&self, name: &str) -> Option<&SourceConfig> {
        self.sources.iter().find(|s| s.name == name)
    }

    /// Migrate legacy [[docs]] entries from config.toml into the DB.
    ///
    /// Called once by `main` after opening the DB. No-op if `docs` is empty.
    /// After migration, rewrites config.toml without the [[docs]] blocks.
    pub fn migrate_docs_to_db(&self, db: &crate::db::Db) -> Result<()> {
        if self.docs.is_empty() {
            return Ok(());
        }

        let db_path = crate::db::db_path();
        // Only auto-migrate if the DB is fresh (no existing docs table data).
        // This prevents re-migrating on every load if the user manually added
        // [[docs]] entries back to config.toml.
        let existing_count: usize = db.list_docs(None, None)?.len();
        if existing_count > 0 {
            // DB already has data — skip migration to avoid duplicates.
            return Ok(());
        }

        eprintln!(
            "tome: migrating {} doc entries from config.toml → {}",
            self.docs.len(),
            db_path.display()
        );

        for doc in &self.docs {
            let content = if doc.source == SOURCE_INLINE {
                // Read content from the old .md file location
                let md_path = legacy_inline_path(&doc.alias);
                match std::fs::read_to_string(&md_path) {
                    Ok(c) => Some(c),
                    Err(_) => {
                        eprintln!(
                            "tome: warning: inline doc '{}' has no content file at {} — migrating metadata only",
                            doc.alias,
                            md_path.display()
                        );
                        None
                    }
                }
            } else {
                None
            };

            let record = crate::db::DocRecord {
                alias: doc.alias.clone(),
                source: doc.source.clone(),
                page_id: doc.page_id.clone(),
                path: doc.path.clone(),
                tags: doc.tags.clone(),
                content,
                namespace: None,
            };

            match db.add_doc(&record) {
                Ok(()) => {}
                Err(_) if db.alias_exists(&doc.alias) => {
                    // Already in DB — skip silently
                }
                Err(e) => {
                    eprintln!("tome: warning: failed to migrate '{}': {e}", doc.alias);
                }
            }
        }

        // Rewrite config.toml without [[docs]] blocks
        self.rewrite_config_without_docs()
            .context("Failed to rewrite config.toml after migration")?;

        // Clean up old .md files for inline docs
        for doc in &self.docs {
            if doc.source == SOURCE_INLINE {
                let md_path = legacy_inline_path(&doc.alias);
                let _ = std::fs::remove_file(&md_path); // best-effort
            }
        }

        eprintln!("tome: migration complete. config.toml cleaned up.");
        Ok(())
    }

    /// Rewrite config.toml, serialising only sources and cache (no docs).
    fn rewrite_config_without_docs(&self) -> Result<()> {
        // Build a clean serialisable version without docs
        #[derive(Serialize)]
        struct CleanConfig<'a> {
            cache: &'a CacheConfig,
            ui: &'a UiConfig,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            sources: &'a Vec<SourceConfig>,
            #[serde(skip_serializing_if = "backup_is_empty")]
            backup: &'a BackupConfig,
        }

        fn backup_is_empty(b: &BackupConfig) -> bool {
            b.path.is_empty()
        }

        let clean = CleanConfig {
            cache: &self.cache,
            ui: &self.ui,
            sources: &self.sources,
            backup: &self.backup,
        };

        let header = "# tome configuration\n# Docs registry has moved to tome.db\n# Sources and cache settings only.\n\n";
        let body = toml::to_string_pretty(&clean).context("Failed to serialise config")?;
        // Use the path from which this config was loaded (respects --config and --profile flags).
        let path = if self.loaded_from.as_os_str().is_empty() {
            config_path()
        } else {
            self.loaded_from.clone()
        };
        std::fs::write(&path, format!("{header}{body}"))
            .with_context(|| format!("Failed to write config: {}", path.display()))?;
        Ok(())
    }
}

pub fn config_path() -> PathBuf {
    paths::app_config_dir().join("config.toml")
}

/// Return the config path for a named profile.
/// Profile "work" → `<config_dir>/config.work.toml`
pub fn profile_config_path(profile: &str) -> PathBuf {
    paths::app_config_dir().join(format!("config.{}.toml", profile))
}

/// Legacy path for inline .md files — used only during migration.
pub fn legacy_inline_path(alias: &str) -> PathBuf {
    paths::app_data_dir()
        .join("local")
        .join(format!("{alias}.md"))
}

const DEFAULT_CONFIG: &str = r#"# tome configuration
# Full docs: https://github.com/sumsar01/tome

# Cache settings
[cache]
enabled = true
ttl_seconds = 3600  # 1 hour

# UI / appearance settings
[ui]
# Built-in themes: dark | light | catppuccin | gruvbox | nord | solarized-dark
# Press T in the TUI to cycle through themes at runtime.
theme = "dark"

# Keybinding overrides (optional). Use a single character or: up, down, pageup,
# pagedown, enter, esc, backspace, tab, home, end, delete.
# Defaults shown below — uncomment and change to remap.
#
# [ui.keys]
# scroll_down   = "j"
# scroll_up     = "k"
# toggle_toc    = "t"
# copy          = "y"
# history       = "h"
# open_url      = "o"
# info          = "i"
# cycle_theme   = "T"
# filter        = "/"
# navigate_down = "j"
# navigate_up   = "k"

# --- Sources ---
# Define named sources to reference when adding docs.
# Tokens are stored in your OS keychain via `tome auth`, never here.

# Example GitHub source:
# [[sources]]
# name = "infra-docs"
# type = "github"
# repo = "yourorg/infra-docs"
# path = "docs/"      # optional subdirectory
# ref = "main"

# Example Confluence source:
# [[sources]]
# name = "confluence"
# type = "confluence"
# base_url = "https://yourco.atlassian.net"
# space = "ENG"       # optional space key for browsing

# Example local source:
# [[sources]]
# name = "my-notes"
# type = "local"
# root = "/Users/you/docs/"

# --- Backup ---
# Auto-export all docs to a JSON file after every write (add, remove, rename, tag, etc.).
# Set `path` to a file inside a git repo to keep your notes safe.
#
# SECURITY WARNING: The backup file contains the full content of every doc stored
# in tome, including internal documentation fetched from Confluence, private GitHub
# repos, and inline notes. Only enable git_push = true with a PRIVATE repository.
# tome will automatically block pushes to public GitHub repositories.
# [backup]
# path = "/Users/you/notes-backup/tome-backup.json"
# git = true        # auto git add + commit (default: true)
# git_push = false  # also git push after committing (default: false)
#                   # WARNING: only use with a private repository!
"#;
