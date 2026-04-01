pub mod confluence;
pub mod github;
pub mod local;

use anyhow::Result;
use async_trait::async_trait;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::{cache, config::Config, db::{Db, SOURCE_INLINE}};

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
pub async fn fetch(cfg: &Config, db: &Db, alias: &str, use_cache: bool) -> Result<String> {
    let doc = db
        .find_doc(alias)?
        .ok_or_else(|| anyhow::anyhow!("Unknown alias '{alias}'. Run `tome list` to see available docs."))?;

    // Inline docs: content stored directly in DB
    if doc.source == SOURCE_INLINE {
        if let Some(content) = &doc.content {
            return Ok(content.clone());
        }
        anyhow::bail!("Inline doc '{alias}' has no content stored.");
    }

    // Try cache first
    if use_cache && cfg.cache.enabled {
        if let Some(cached) = cache::get(alias, cfg.cache.ttl_seconds) {
            tracing::debug!("Cache hit for '{alias}'");
            return Ok(cached);
        }
    }

    // Fetch live
    let content = fetch_live(cfg, &doc).await?;

    // Store in cache
    if cfg.cache.enabled {
        if let Err(e) = cache::set(alias, &content, cfg.cache.ttl_seconds) {
            tracing::warn!("Failed to cache '{alias}': {e}");
        }
    }

    Ok(content)
}

/// Fetch doc content from the configured source, bypassing cache.
async fn fetch_live(cfg: &Config, doc: &crate::db::DocRecord) -> Result<String> {
    let source_name = &doc.source;

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
            let root = source_cfg.root.as_deref().unwrap_or(".");
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
        // Inline is handled above; this arm should be unreachable
        crate::config::SourceKind::Inline => {
            anyhow::bail!("Inline doc '{}' should not reach source dispatch", doc.alias)
        }
    }
}

/// Fuzzy search across all docs by alias and tags.
pub async fn search(cfg: &Config, db: &Db, query: &str) -> Result<Vec<SearchResult>> {
    let _ = cfg; // cfg kept for future use (cache settings etc.)
    let matcher = SkimMatcherV2::default();
    let mut results: Vec<SearchResult> = Vec::new();

    for doc in db.list_docs(None)? {
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
