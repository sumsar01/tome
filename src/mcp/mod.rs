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
use crate::sources;

pub struct TomeServer {
    cfg: Arc<Config>,
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

#[tool_router]
impl TomeServer {
    #[allow(clippy::new_without_default)]
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg: Arc::new(cfg),
            tool_router: Self::tool_router(),
        }
    }

    /// List all available documentation aliases registered in tome.
    #[tool(description = "List all available documentation aliases registered in tome. Returns alias, source, and tags for each doc.")]
    async fn tome_list(&self, Parameters(params): Parameters<ListParams>) -> String {
        let docs = self.cfg.list_docs();
        let filtered: Vec<_> = match &params.tag {
            Some(t) => docs
                .iter()
                .filter(|d| d.tags.iter().any(|dt| dt.contains(t.as_str())))
                .collect(),
            None => docs.iter().collect(),
        };

        if filtered.is_empty() {
            return "No docs found.".to_string();
        }

        let mut out = String::from("Available docs:\n\n");
        for doc in filtered {
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
        match sources::fetch(&self.cfg, &params.alias, true).await {
            Ok(content) => content,
            Err(e) => format!("Error fetching '{}': {e}", params.alias),
        }
    }

    /// Search for documentation by keyword across aliases and tags.
    #[tool(description = "Search for documentation by keyword. Searches across doc aliases and tags, returns ranked results.")]
    async fn tome_search(&self, Parameters(params): Parameters<SearchParams>) -> String {
        match sources::search(&self.cfg, &params.query).await {
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
}

#[tool_handler]
impl ServerHandler for TomeServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "tome gives you access to internal documentation. \
                 Use tome_list to see available docs, tome_get to fetch a doc by alias, \
                 and tome_search to find relevant docs.",
            )
    }
}

/// Start the MCP stdio server.
pub async fn serve(cfg: Config) -> Result<()> {
    let server = TomeServer::new(cfg);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
