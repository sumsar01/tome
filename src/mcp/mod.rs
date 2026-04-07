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

#[tool_handler]
impl ServerHandler for TomeServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "tome gives you access to internal documentation. \
                 Use tome_list to see available docs, tome_get to fetch a doc by alias, \
                 tome_search to find relevant docs, tome_add to save new docs, \
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
    let mut out = format!(
        "--- {} v{} ({})\n+++ {} v{} ({})\n\n",
        alias, a.version, a.fetched_at,
        alias, b.version, b.fetched_at
    );

    let a_lines: Vec<&str> = a.content.lines().collect();
    let b_lines: Vec<&str> = b.content.lines().collect();
    let m = a_lines.len();
    let n = b_lines.len();

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a_lines[i - 1] == b_lines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a_lines[i - 1] == b_lines[j - 1] {
            ops.push(('=', a_lines[i - 1]));
            i -= 1; j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(('+', b_lines[j - 1]));
            j -= 1;
        } else {
            ops.push(('-', a_lines[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();

    const CTX: usize = 3;
    let changed: Vec<bool> = ops.iter().map(|(op, _)| *op != '=').collect();
    let mut printed = vec![false; ops.len()];
    for (k, changed_k) in changed.iter().enumerate() {
        if *changed_k {
            let start = k.saturating_sub(CTX);
            let end = (k + CTX + 1).min(ops.len());
            for p in printed.iter_mut().take(end).skip(start) { *p = true; }
        }
    }

    let mut last: Option<usize> = None;
    for (k, (op, line)) in ops.iter().enumerate() {
        if !printed[k] { continue; }
        if let Some(l) = last { if k > l + 1 { out.push_str("@@ ... @@\n"); } }
        let prefix = match op { '-' => "-", '+' => "+", _ => " " };
        out.push_str(&format!("{}{}\n", prefix, line));
        last = Some(k);
    }

    if last.is_none() { out.push_str("(no differences)\n"); }
    out
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
