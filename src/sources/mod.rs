pub mod confluence;
pub mod github;
pub mod local;

use anyhow::{Context, Result};
use async_trait::async_trait;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::{cache, config::Config};

/// Result of a search across sources
pub struct SearchResult {
    pub alias: String,
    pub snippet: String,
    pub score: i64,
}

/// Trait implemented by all doc sources
#[async_trait]
pub trait Source: Send + Sync {
    async fn fetch_content(&self, path: &str) -> Result<String>;
}

/// Fetch a doc by alias, using cache if enabled and fresh.
pub async fn fetch(cfg: &Config, alias: &str, use_cache: bool) -> Result<String> {
    let doc = cfg
        .find_doc(alias)
        .ok_or_else(|| anyhow::anyhow!("Unknown alias '{alias}'. Run `tome list` to see available docs."))?;

    // Try cache first
    if use_cache && cfg.cache.enabled {
        if let Some(cached) = cache::get(alias, cfg.cache.ttl_seconds) {
            tracing::debug!("Cache hit for '{alias}'");
            return Ok(cached);
        }
    }

    // Fetch live
    let content = fetch_live(cfg, doc).await?;

    // Store in cache
    if cfg.cache.enabled {
        if let Err(e) = cache::set(alias, &content, cfg.cache.ttl_seconds) {
            tracing::warn!("Failed to cache '{alias}': {e}");
        }
    }

    Ok(content)
}

/// Fetch doc content from the configured source, bypassing cache.
async fn fetch_live(cfg: &Config, doc: &crate::config::DocConfig) -> Result<String> {
    let source_name = &doc.source;

    // Inline docs saved by `tome add` / `tome_add` MCP tool
    if source_name == "inline" {
        let path = crate::config::inline_path(&doc.alias);
        return std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read inline doc '{}' at {}", doc.alias, path.display()));
    }

    // Special case: inline local path (source = "local" with no named source config)
    if source_name == "local" && cfg.find_source("local").is_none() {
        let path = doc
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Local doc '{}' has no path configured", doc.alias))?;
        let src = local::LocalSource::new(path.into());
        return src.fetch_content("").await;
    }

    let source_cfg = cfg
        .find_source(source_name)
        .ok_or_else(|| anyhow::anyhow!("Source '{source_name}' not found in config"))?;

    match source_cfg.kind {
        crate::config::SourceKind::Local => {
            let root = source_cfg
                .root
                .as_deref()
                .unwrap_or(".");
            let path = doc.path.as_deref().unwrap_or("");
            let src = local::LocalSource::new(root.into());
            src.fetch_content(path).await
        }
        crate::config::SourceKind::Github => {
            let repo = source_cfg
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("GitHub source '{source_name}' missing 'repo'"))?;
            let git_ref = source_cfg.git_ref.as_deref().unwrap_or("main");
            let src = github::GitHubSource::new(repo, git_ref)?;
            let path = doc.path.as_deref().unwrap_or("");
            src.fetch_content(path).await
        }
        crate::config::SourceKind::Confluence => {
            let base_url = source_cfg
                .base_url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Confluence source '{source_name}' missing 'base_url'"))?;
            let src = confluence::ConfluenceSource::new(base_url)?;
            let path = doc
                .page_id
                .as_deref()
                .or(doc.path.as_deref())
                .ok_or_else(|| anyhow::anyhow!("Confluence doc '{}' needs 'page_id' or 'path'", doc.alias))?;
            src.fetch_content(path).await
        }
        // Inline is handled above before source lookup; this arm is unreachable
        crate::config::SourceKind::Inline => {
            anyhow::bail!("Inline doc '{}' should not reach source dispatch", doc.alias)
        }
    }
}

/// Fuzzy search across all docs by alias and tags.
/// For a deeper content search, docs are fetched (cached preferred).
pub async fn search(cfg: &Config, query: &str) -> Result<Vec<SearchResult>> {
    let matcher = SkimMatcherV2::default();
    let mut results: Vec<SearchResult> = Vec::new();

    for doc in &cfg.docs {
        // Match against alias + tags
        let haystack = format!("{} {}", doc.alias, doc.tags.join(" "));
        if let Some(score) = matcher.fuzzy_match(&haystack, query) {
            results.push(SearchResult {
                alias: doc.alias.clone(),
                snippet: format!("[{}] {}", doc.source, doc.tags.join(", ")),
                score,
            });
        }
    }

    results.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(results)
}
