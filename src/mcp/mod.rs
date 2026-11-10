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

mod format;

pub struct TomeServer {
    cfg: Arc<Config>,
    db: Arc<Db>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    /// Filter by tag
    pub tag: Option<String>,
    /// Filter by namespace
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetParams {
    pub alias: String,
    /// Return only this heading's section (case-insensitive; exact title, else substring)
    pub section: Option<String>,
    /// Max chars to return (default 16000, 0 = no limit)
    pub max_chars: Option<usize>,
    /// Char offset to continue from (see truncation hint)
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    pub query: String,
    /// Max results (default 8)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddParams {
    /// Unique kebab-case alias
    pub alias: String,
    /// Markdown content
    pub content: String,
    /// Omit to infer from headings
    pub tags: Option<Vec<String>>,
    /// Employer name for work docs (e.g. "whiteaway"); omit for general knowledge
    pub namespace: Option<String>,
    /// Always set, e.g. "Notes", "How-To", "API Docs", "Runbooks", "Architecture"
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateParams {
    pub alias: String,
    /// "" clears; omit to keep
    pub category: Option<String>,
    /// "" clears; omit to keep
    pub namespace: Option<String>,
    /// Replaces all tags; [] clears; omit to keep
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HistoryParams {
    pub alias: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DiffParams {
    pub alias: String,
    /// 1-based version (default: second-to-last)
    pub v1: Option<usize>,
    /// 1-based version (default: last)
    pub v2: Option<usize>,
}

#[tool_router]
impl TomeServer {
    #[allow(clippy::new_without_default)]
    pub fn new(cfg: Config, db: Db) -> Self {
        let mut tool_router = Self::tool_router();
        for route in tool_router.map.values_mut() {
            let mut schema = (*route.attr.input_schema).clone();
            schema.remove("$schema");
            route.attr.input_schema = Arc::new(schema);
        }
        Self {
            cfg: Arc::new(cfg),
            db: Arc::new(db),
            tool_router,
        }
    }

    #[tool(description = "List all docs (alias, category, namespace, tags). Prefer tome_search.")]
    async fn tome_list(&self, Parameters(params): Parameters<ListParams>) -> String {
        let docs = match self.db.list_docs(params.tag.as_deref(), params.namespace.as_deref()) {
            Ok(d) => d,
            Err(e) => return format!("Error listing docs: {e}"),
        };

        if docs.is_empty() {
            return "No docs found.".to_string();
        }

        let mut out = format!("{} docs (alias | category | namespace | tags):\n", docs.len());
        for doc in &docs {
            out.push_str(&format!(
                "{} | {} | {} | {}\n",
                doc.alias,
                doc.category.as_deref().unwrap_or("-"),
                doc.namespace.as_deref().unwrap_or("-"),
                doc.tags.join(",")
            ));
        }
        out
    }

    #[tool(description = "Get a doc's markdown by alias. Long docs are truncated with a hint listing sections and the offset to continue.")]
    async fn tome_get(&self, Parameters(params): Parameters<GetParams>) -> String {
        match sources::fetch(&self.cfg, &self.db, &params.alias, true).await {
            Ok(content) => format::get_view(
                &params.alias,
                &content,
                params.section.as_deref(),
                params.max_chars,
                params.offset,
            ),
            Err(e) => format!("Error fetching '{}': {e}", params.alias),
        }
    }

    #[tool(description = "Search docs by keyword (content, alias, tags). Returns ranked aliases with title, category and a short snippet; fetch with tome_get.")]
    async fn tome_search(&self, Parameters(params): Parameters<SearchParams>) -> String {
        match sources::search(&self.cfg, &self.db, &params.query).await {
            Ok(results) => format::search_results(&self.db, &params.query, &results, params.limit),
            Err(e) => format!("Search error: {e}"),
        }
    }

    #[tool(description = "Save markdown as a new doc. Use for useful reference docs from URLs or reusable knowledge you produce. \
        Always set category; set namespace for work docs. Errors if alias exists (then use tome_update).")]
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
            namespace: params.namespace.clone(),
            category: params.category.clone(),
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

    #[tool(description = "Change category, namespace or tags of an existing doc.")]
    async fn tome_update(&self, Parameters(params): Parameters<UpdateParams>) -> String {
        if !self.db.alias_exists(&params.alias) {
            return format!("Error: no doc with alias '{}' found.", params.alias);
        }

        let mut changed: Vec<String> = Vec::new();

        if let Some(ref cat) = params.category {
            let value = if cat.is_empty() { None } else { Some(cat.as_str()) };
            match self.db.update_category(&params.alias, value) {
                Ok(_) => {
                    if cat.is_empty() {
                        changed.push("category cleared".to_string());
                    } else {
                        changed.push(format!("category set to '{cat}'"));
                    }
                }
                Err(e) => return format!("Error updating category: {e}"),
            }
        }

        if let Some(ref ns) = params.namespace {
            let value = if ns.is_empty() { None } else { Some(ns.as_str()) };
            match self.db.update_namespace(&params.alias, value) {
                Ok(_) => {
                    if ns.is_empty() {
                        changed.push("namespace cleared".to_string());
                    } else {
                        changed.push(format!("namespace set to '{ns}'"));
                    }
                }
                Err(e) => return format!("Error updating namespace: {e}"),
            }
        }

        if let Some(ref tags) = params.tags {
            match self.db.update_tags(&params.alias, tags) {
                Ok(_) => {
                    if tags.is_empty() {
                        changed.push("tags cleared".to_string());
                    } else {
                        changed.push(format!("tags set to: {}", tags.join(", ")));
                    }
                }
                Err(e) => return format!("Error updating tags: {e}"),
            }
        }

        if changed.is_empty() {
            format!("'{}': nothing to update (no fields provided).", params.alias)
        } else {
            format!("Updated '{}': {}.", params.alias, changed.join("; "))
        }
    }

    #[tool(description = "List recorded versions of a remote doc (index, time, hash).")]
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

    #[tool(description = "Unified diff between two versions from tome_history (default: last two).")]
    async fn tome_diff(&self, Parameters(params): Parameters<DiffParams>) -> String {
        let versions = match self.db.list_versions(&params.alias) {
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
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TomeServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Personal docs library. tome_search, then tome_get the best alias (use section/offset for long docs). \
                 Save reusable docs with tome_add; fix metadata with tome_update.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schemas_omit_meta_schema_url() {
        let server = TomeServer::new(Config::default(), Db::open_in_memory().unwrap());
        let tools = server.tool_router.list_all();
        assert_eq!(tools.len(), 7);
        for t in tools {
            assert!(!t.input_schema.contains_key("$schema"), "{}", t.name);
        }
    }
}
