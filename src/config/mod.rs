pub mod auth;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level config loaded from ~/.config/tome/config.toml
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    #[serde(default)]
    pub docs: Vec<DocConfig>,
    #[serde(default)]
    pub cache: CacheConfig,
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
    Inline,
}

/// A single registered doc with an alias
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DocConfig {
    /// Short name used on the CLI: `tome get kubernetes`
    pub alias: String,
    /// Matches a [[sources]] name, or "local" for inline local paths
    pub source: String,
    /// Confluence page ID, GitHub file path, or local file path
    pub path: Option<String>,
    /// Confluence page ID (alternative to path for confluence sources)
    pub page_id: Option<String>,
    /// Optional tags for filtering/search
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

/// Flat view of a doc used for listing
pub struct DocInfo {
    pub alias: String,
    pub source: String,
    pub tags: Vec<String>,
}

impl Config {
    /// Load config from ~/.config/tome/config.toml, creating it if missing.
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

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", path.display()))
    }

    /// Find a doc config by alias.
    pub fn find_doc(&self, alias: &str) -> Option<&DocConfig> {
        self.docs.iter().find(|d| d.alias == alias)
    }

    /// Find a source config by name.
    pub fn find_source(&self, name: &str) -> Option<&SourceConfig> {
        self.sources.iter().find(|s| s.name == name)
    }

    /// Check whether an alias already exists in the config.
    pub fn alias_exists(&self, alias: &str) -> bool {
        self.docs.iter().any(|d| d.alias == alias)
    }

    /// Save an inline doc: write content to disk and append to config.toml.
    /// Returns an error if the alias already exists.
    pub fn add_inline_doc(alias: &str, content: &str, tags: Vec<String>) -> Result<()> {
        // Load current config to check for duplicates
        let cfg = Self::load()?;
        if cfg.alias_exists(alias) {
            anyhow::bail!(
                "alias '{alias}' already exists in tome. \
                 Use a different alias or remove the existing entry from config.toml first."
            );
        }

        // Write content file
        let dir = inline_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create inline dir: {}", dir.display()))?;
        let file_path = inline_path(alias);
        std::fs::write(&file_path, content)
            .with_context(|| format!("Failed to write inline doc: {}", file_path.display()))?;

        // Append [[docs]] block to config.toml
        let toml_block = format!(
            "\n[[docs]]\nalias = \"{alias}\"\nsource = \"inline\"\ntags = [{}]\n",
            tags.iter()
                .map(|t| format!("\"{t}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let config_file = config_path();
        let mut current = std::fs::read_to_string(&config_file)
            .with_context(|| format!("Failed to read config: {}", config_file.display()))?;
        current.push_str(&toml_block);
        std::fs::write(&config_file, &current)
            .with_context(|| format!("Failed to write config: {}", config_file.display()))?;

        Ok(())
    }

    /// Flat list of all docs for the `list` command.
    pub fn list_docs(&self) -> Vec<DocInfo> {
        self.docs
            .iter()
            .map(|d| DocInfo {
                alias: d.alias.clone(),
                source: d.source.clone(),
                tags: d.tags.clone(),
            })
            .collect()
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tome")
        .join("config.toml")
}

/// Directory where inline (agent-saved) docs are stored.
pub fn inline_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tome")
        .join("local")
}

/// Path to the inline doc file for a given alias.
pub fn inline_path(alias: &str) -> PathBuf {
    inline_dir().join(format!("{alias}.md"))
}

const DEFAULT_CONFIG: &str = r#"# tome configuration
# Full docs: https://github.com/sumsar01/tome

# Cache settings
[cache]
enabled = true
ttl_seconds = 3600  # 1 hour

# --- Sources ---
# Define named sources to reference in [[docs]] entries.
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

# --- Docs ---
# Register individual docs with short aliases.

# Example GitHub doc:
# [[docs]]
# alias = "migration-guide"
# source = "infra-docs"
# path = "docs/migration.md"
# tags = ["migration", "infra"]

# Example Confluence doc:
# [[docs]]
# alias = "kubernetes"
# source = "confluence"
# page_id = "123456"
# tags = ["infra", "k8s"]

# Example local doc:
# [[docs]]
# alias = "runbook"
# source = "my-notes"
# path = "runbook.md"
# tags = ["ops"]
"#;
