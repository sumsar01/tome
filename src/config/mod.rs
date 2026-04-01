pub mod auth;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level config loaded from the platform config file.
/// Contains only sources and cache settings.
/// Doc registrations live in the SQLite database (`tome.db`).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    #[serde(default)]
    pub cache: CacheConfig,
    /// Legacy [[docs]] entries — read during migration only, then removed from disk.
    #[serde(default, skip_serializing)]
    pub docs: Vec<DocConfig>,
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

fn default_ttl() -> u64 {
    3600
}

impl Config {
    /// Load config from the platform config path, creating it if missing.
    ///
    /// If legacy `[[docs]]` entries are found and no `tome.db` exists yet,
    /// they are migrated automatically and removed from config.toml.
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create config dir: {}", parent.display())
                })?;
            }
            std::fs::write(&path, DEFAULT_CONFIG)
                .with_context(|| format!("Failed to write default config: {}", path.display()))?;
            eprintln!("Created default config at {}", path.display());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;

        let cfg: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", path.display()))?;

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
        let existing_count: usize = db.list_docs(None)?.len();
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
            let content = if doc.source == "inline" {
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
            };

            match db.add_doc(&record) {
                Ok(()) => {}
                Err(e) if e.to_string().contains("already exists") => {
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
            if doc.source == "inline" {
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
            #[serde(skip_serializing_if = "Vec::is_empty")]
            sources: &'a Vec<SourceConfig>,
        }

        let clean = CleanConfig {
            cache: &self.cache,
            sources: &self.sources,
        };

        let header = "# tome configuration\n# Docs registry has moved to tome.db\n# Sources and cache settings only.\n\n";
        let body = toml::to_string_pretty(&clean).context("Failed to serialise config")?;
        let path = config_path();
        std::fs::write(&path, format!("{header}{body}"))
            .with_context(|| format!("Failed to write config: {}", path.display()))?;
        Ok(())
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tome")
        .join("config.toml")
}

/// Legacy path for inline .md files — used only during migration.
pub fn legacy_inline_path(alias: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tome")
        .join("local")
        .join(format!("{alias}.md"))
}

const DEFAULT_CONFIG: &str = r#"# tome configuration
# Full docs: https://github.com/sumsar01/tome

# Cache settings
[cache]
enabled = true
ttl_seconds = 3600  # 1 hour

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
"#;
