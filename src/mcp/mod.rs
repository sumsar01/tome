use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{
        ServerCapabilities, ServerInfo,
        InitializeResult,
    },
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::config::Config;
use crate::db::{Db, DocRecord, SOURCE_INLINE};
use crate::sources;

pub struct TomeServer {
    cfg: Arc<Config>,
    db: Arc<Db>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    /// Optional tag to filter docs by (e.g. "migration", "infra")
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetParams {
    /// The doc alias to fetch, e.g. "kubernetes" or "migration-guide"
    pub alias: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// Search query, e.g. "migration kubernetes"
    pub query: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddParams {
    /// Short unique alias for the doc, e.g. "fastify-plugins" (kebab-case, no spaces)
    pub alias: String,
    /// Full markdown content of the document
    pub content: String,
    /// Tags for filtering and search. If omitted, tags are inferred from headings.
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HistoryParams {
    /// The doc alias to show history for
    pub alias: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DiffParams {
    /// The doc alias to diff
    pub alias: String,
    /// First version index (1-based, default: second-to-last)
    pub v1: Option<usize>,
    /// Second version index (1-based, default: last)
    pub v2: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteParams {
    /// The doc alias to delete
    pub alias: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RefreshParams {
    /// The doc alias to re-fetch
    pub alias: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RenameParams {
    /// The current doc alias
    pub old_alias: String,
    /// The new doc alias (kebab-case, no spaces)
    pub new_alias: String,
}

#[tool_router]
impl TomeServer {
    #[allow(clippy::new_without_default)]
    pub fn new(cfg: Config, db: Db) -> Self {
        Self {
            cfg: Arc::new(cfg),
            db: Arc::new(db),
            tool_router: Self::tool_router(),
        }
    }

    /// List all available documentation aliases registered in tome.
    #[tool(description = "List all available documentation aliases registered in tome. Returns alias, source, and tags for each doc.")]
    async fn tome_list(&self, Parameters(params): Parameters<ListParams>) -> String {
        let docs = match self.db.list_docs(params.tag.as_deref()) {
            Ok(d) => d,
            Err(e) => return format!("Error listing docs: {e}"),
        };

        if docs.is_empty() {
            return "No docs found.".to_string();
        }

        let mut out = String::from("Available docs:\n\n");
        for doc in &docs {
            out.push_str(&format!(
                "- **{}** (source: {}) tags: {}\n",
                doc.alias,
                doc.source,
                doc.tags.join(", ")
            ));
        }
        out
    }

    /// Fetch the full content of a documentation page by its alias.
    #[tool(description = "Fetch the full content of a documentation page by its alias. Returns markdown content.")]
    async fn tome_get(&self, Parameters(params): Parameters<GetParams>) -> String {
        match sources::fetch(&self.cfg, &self.db, &params.alias, true).await {
            Ok(content) => content,
            Err(e) => format!("Error fetching '{}': {e}", params.alias),
        }
    }

    /// Search for documentation by keyword across aliases and tags.
    #[tool(description = "Search for documentation by keyword. Searches across doc aliases and tags, returns ranked results.")]
    async fn tome_search(&self, Parameters(params): Parameters<SearchParams>) -> String {
        match sources::search(&self.cfg, &self.db, &params.query).await {
            Ok(results) if results.is_empty() => format!("No results for '{}'", params.query),
            Ok(results) => {
                let mut out = format!("Search results for '{}':\n\n", params.query);
                for r in results {
                    out.push_str(&format!("- **{}** — {}\n", r.alias, r.snippet));
                }
                out
            }
            Err(e) => format!("Search error: {e}"),
        }
    }

    /// Save a markdown document to tome with an alias and tags.
    /// Use this when the user shares a URL with useful reference documentation.
    /// Tags are inferred from headings if not provided.
    /// Returns an error if the alias already exists.
    #[tool(description = "Save a markdown document to tome so it can be retrieved later by alias. \
        Use when the user shares a URL containing useful reference docs (API docs, guides, runbooks, specs). \
        Tags are inferred from headings if omitted. Errors if alias already exists.")]
    async fn tome_add(&self, Parameters(params): Parameters<AddParams>) -> String {
        let alias = params.alias.trim().to_string();

        // Validate alias: kebab-case, no spaces
        if alias.is_empty() || alias.contains(' ') {
            return "Error: alias must be non-empty and contain no spaces (use kebab-case, e.g. 'fastify-plugins')".to_string();
        }

        let tags = match params.tags {
            Some(t) if !t.is_empty() => t,
            _ => infer_tags(&params.content),
        };

        let record = DocRecord {
            alias: alias.clone(),
            source: SOURCE_INLINE.to_string(),
            page_id: None,
            path: None,
            tags: tags.clone(),
            content: Some(params.content),
        };

        match self.db.add_doc(&record) {
            Ok(()) => format!(
                "Saved '{}' to tome with tags: {}",
                alias,
                if tags.is_empty() { "(none)".to_string() } else { tags.join(", ") }
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// List the fetch history for a doc alias.
    #[tool(description = "List the fetch history for a doc alias. Returns version index, timestamp, and content hash for each recorded fetch.")]
    async fn tome_history(&self, Parameters(params): Parameters<HistoryParams>) -> String {
        match self.db.list_versions(&params.alias) {
            Ok(versions) if versions.is_empty() => {
                format!("No history for '{}'. Fetch it first with tome_get.", params.alias)
            }
            Ok(versions) => {
                let mut out = format!("History for '{}':\n\n", params.alias);
                for v in &versions {
                    out.push_str(&format!(
                        "- v{}: {} (hash: {})\n",
                        v.version, v.fetched_at, v.content_hash
                    ));
                }
                out
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Show a unified diff between two versions of a doc.
    #[tool(description = "Show a unified diff between two versions of a doc. v1 and v2 are 1-based version indices from tome_history. Defaults to diffing the last two versions.")]
    async fn tome_diff(&self, Parameters(params): Parameters<DiffParams>) -> String {        let versions = match self.db.list_versions(&params.alias) {
            Ok(v) => v,
            Err(e) => return format!("Error: {e}"),
        };
        if versions.len() < 2 {
            return format!(
                "Need at least 2 versions to diff '{}'. Current count: {}",
                params.alias,
                versions.len()
            );
        }
        let idx1 = params.v1.map(|v| v.saturating_sub(1)).unwrap_or(versions.len() - 2);
        let idx2 = params.v2.map(|v| v.saturating_sub(1)).unwrap_or(versions.len() - 1);
        let a = match versions.get(idx1) {
            Some(v) => v,
            None => return format!("Version {} not found", idx1 + 1),
        };
        let b = match versions.get(idx2) {
            Some(v) => v,
            None => return format!("Version {} not found", idx2 + 1),
        };
        unified_diff_string(&params.alias, a, b)
    }

    /// Delete a doc from tome by alias.
    #[tool(description = "Delete a doc from tome by alias. Returns a confirmation message or an error if the alias does not exist.")]
    async fn tome_delete(&self, Parameters(params): Parameters<DeleteParams>) -> String {
        match self.db.remove_doc(&params.alias) {
            Ok(true) => format!("Deleted '{}' from tome.", params.alias),
            Ok(false) => format!("Error: no doc with alias '{}' found.", params.alias),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Re-fetch a doc and refresh its cached content.
    #[tool(description = "Re-fetch a doc and refresh its cached content. Use when a doc may be stale and you want the latest version. Returns a confirmation with byte count or an error if the alias does not exist.")]
    async fn tome_refresh(&self, Parameters(params): Parameters<RefreshParams>) -> String {
        if !self.db.alias_exists(&params.alias) {
            return format!("Error: no doc with alias '{}' found.", params.alias);
        }
        if let Err(e) = crate::cache::invalidate(&params.alias) {
            return format!("Error invalidating cache: {e}");
        }
        match sources::fetch(&self.cfg, &self.db, &params.alias, false).await {
            Ok(content) => format!("Refreshed '{}' ({} bytes).", params.alias, content.len()),
            Err(e) => format!("Error refreshing '{}': {e}", params.alias),
        }
    }

    /// Rename a doc alias in tome.
    #[tool(description = "Rename a doc alias in tome. Updates both the doc registry and its version history atomically. Errors if old_alias does not exist or new_alias already exists.")]
    async fn tome_rename(&self, Parameters(params): Parameters<RenameParams>) -> String {
        let new_alias = params.new_alias.trim().to_string();
        if new_alias.is_empty() || new_alias.contains(' ') {
            return "Error: new_alias must be non-empty and contain no spaces (use kebab-case, e.g. 'fastify-plugins')".to_string();
        }
        match self.db.rename_doc(&params.old_alias, &new_alias) {
            Ok(()) => format!("Renamed '{}' → '{}'.", params.old_alias, new_alias),
            Err(e) => format!("Error: {e}"),
        }
    }
}

#[tool_handler]
impl ServerHandler for TomeServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "tome gives you access to internal documentation. \
                 Use tome_list to see available docs, tome_get to fetch a doc by alias, \
                 tome_search to find relevant docs, tome_add to save new docs, \
                 tome_refresh to re-fetch a stale doc, tome_delete to remove docs, \
                 tome_rename to rename a doc alias, \
                 tome_history to see fetch history, and tome_diff to compare versions.",
            )
    }
}

/// Start the MCP stdio server.
pub async fn serve(cfg: Config, db: Db) -> Result<()> {
    let server = TomeServer::new(cfg, db);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Build a unified diff string between two DocVersions (public for TUI use).
pub fn unified_diff_string_pub(alias: &str, a: &crate::db::DocVersion, b: &crate::db::DocVersion) -> String {
    unified_diff_string(alias, a, b)
}

/// Build a unified diff string between two DocVersions.
fn unified_diff_string(alias: &str, a: &crate::db::DocVersion, b: &crate::db::DocVersion) -> String {
    let header = format!(
        "--- {} v{} ({})\n+++ {} v{} ({})\n\n",
        alias, a.version, a.fetched_at,
        alias, b.version, b.fetched_at
    );
    header + &crate::util::diff::unified_diff(&a.content, &b.content)
}

/// Infer tags from markdown headings (H1/H2), slugified and capped at 5.
pub(crate) fn infer_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        let heading = if let Some(h) = line.strip_prefix("## ") {
            h
        } else if let Some(h) = line.strip_prefix("# ") {
            h
        } else {
            continue;
        };
        let slug = slugify(heading);
        if !slug.is_empty() && !tags.contains(&slug) {
            tags.push(slug);
        }
        if tags.len() >= 5 {
            break;
        }
    }
    tags
}

/// Convert a heading string to a kebab-case tag slug.
fn slugify(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-')
        .collect::<String>()
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| !w.is_empty() && w.len() > 1)  // drop single-char words
        .collect::<Vec<_>>()
        .join("-")
}
