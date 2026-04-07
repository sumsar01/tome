use anyhow::Result;
use clap::{Parser, Subcommand};
mod cache;
mod config;
mod db;
mod http;
mod mcp;
mod paths;
mod sources;
mod tui;

#[derive(Parser)]
#[command(name = "tome", version, about = "A docs reader for humans and AI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Browse all docs in the TUI (default when no command given)
    Browse,
    /// Open a specific doc in the TUI reader
    Read {
        /// Doc alias to open
        alias: String,
    },
    /// Print doc content to stdout
    Get {
        /// Doc alias to fetch
        alias: String,
        /// Skip local cache and fetch live
        #[arg(long)]
        no_cache: bool,
    },
    /// List all configured doc aliases
    List,
    /// Fuzzy search across all docs
    Search {
        /// Search query
        query: String,
    },
    /// Store authentication credentials securely
    Auth {
        #[command(subcommand)]
        provider: AuthProvider,
    },
    /// Clear the local doc cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Start the MCP stdio server for AI agent use
    Mcp,
    /// Remove a registered doc by alias
    Remove {
        /// Doc alias to remove
        alias: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Save a local markdown file or URL as an inline doc
    Add {
        /// Short unique alias (kebab-case, e.g. "fastify-plugins")
        #[arg(long)]
        alias: String,
        /// Path to a local markdown file to save
        #[arg(long, conflicts_with = "url")]
        file: Option<String>,
        /// URL to fetch and save as markdown
        #[arg(long, conflicts_with = "file")]
        url: Option<String>,
        /// Comma-separated tags (inferred from headings if omitted)
        #[arg(long)]
        tags: Option<String>,
    },
}

#[derive(Subcommand)]
enum AuthProvider {
    /// Store Confluence API token in OS keychain
    Confluence {
        /// Your Atlassian account email
        #[arg(long)]
        email: String,
    },
    /// Store GitHub token in OS keychain (or use existing gh CLI auth)
    Github,
}

#[derive(Subcommand)]
enum CacheAction {
    /// Clear all cached docs
    Clear,
    /// Show cache status and size
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::Config::load()?;

    // Only enable logging for non-TUI commands — tracing output to stderr
    // corrupts the alternate screen used by the TUI.
    let is_tui = matches!(
        cli.command,
        None | Some(Command::Browse) | Some(Command::Read { .. })
    );
    if !is_tui {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::WARN.into()),
            )
            .with_writer(std::io::stderr)
            .init();
    }

    // Open the database and migrate any legacy config.toml [[docs]] entries.
    let db = db::Db::open()?;
    cfg.migrate_docs_to_db(&db)?;

    match cli.command.unwrap_or(Command::Browse) {
        Command::Browse => tui::run(cfg, db).await?,
        Command::Read { alias } => tui::run_reader(cfg, db, &alias).await?,
        Command::Get { alias, no_cache } => {
            let content = sources::fetch(&cfg, &db, &alias, !no_cache).await?;
            print!("{content}");
        }
        Command::List => {
            let docs = db.list_docs(None)?;
            if docs.is_empty() {
                eprintln!("No docs configured. Use `tome add` to add docs.");
            } else {
                println!("{:<24} {:<16} TAGS", "ALIAS", "SOURCE");
                println!("{}", "-".repeat(60));
                for doc in docs {
                    println!(
                        "{:<24} {:<16} {}",
                        doc.alias,
                        doc.source,
                        doc.tags.join(", ")
                    );
                }
            }
        }
        Command::Search { query } => {
            let results = sources::search(&cfg, &db, &query).await?;
            if results.is_empty() {
                eprintln!("No results for '{query}'");
            } else {
                for r in results {
                    println!("{} — {}", r.alias, r.snippet);
                }
            }
        }
        Command::Auth { provider } => match provider {
            AuthProvider::Confluence { email } => {
                config::auth::store_confluence_token(&email)?;
            }
            AuthProvider::Github => {
                config::auth::store_github_token()?;
            }
        },
        Command::Cache { action } => match action {
            CacheAction::Clear => {
                cache::clear()?;
                println!("Cache cleared.");
            }
            CacheAction::Status => {
                cache::status()?;
            }
        },
        Command::Mcp => {
            mcp::serve(cfg, db).await?;
        }
        Command::Remove { alias, force } => {
            if !db.alias_exists(&alias) {
                anyhow::bail!("No doc with alias '{}' found.", alias);
            }
            if !force {
                eprint!("Remove '{alias}'? [y/N] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    return Ok(());
                }
            }
            db.remove_doc(&alias)?;
            println!("Removed '{alias}'.");
        }
        Command::Add { alias, file, url, tags } => {
            let tags_vec: Vec<String> = tags
                .unwrap_or_default()
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();

            let content = if let Some(path) = file {
                std::fs::read_to_string(&path)
                    .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?
            } else if let Some(ref u) = url {
                fetch_url(u).await?
            } else {
                anyhow::bail!("Provide either --file <path> or --url <url>");
            };

            let final_tags = if tags_vec.is_empty() {
                mcp::infer_tags(&content)
            } else {
                tags_vec
            };

            db.add_doc(&db::DocRecord {
                alias: alias.clone(),
                source: db::SOURCE_INLINE.to_string(),
                page_id: None,
                path: None,
                tags: final_tags.clone(),
                content: Some(content),
            })?;
            println!(
                "Saved '{}' with tags: {}",
                alias,
                if final_tags.is_empty() { "(none)".to_string() } else { final_tags.join(", ") }
            );
        }
    }

    Ok(())
}

/// Fetch a URL and return its content as markdown (best-effort).
async fn fetch_url(url: &str) -> anyhow::Result<String> {
    let client = http::build_http_client()?;
    let resp = client.get(url).send().await
        .map_err(|e| anyhow::anyhow!("Failed to fetch '{url}': {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching '{url}'", resp.status());
    }
    let text = resp.text().await?;
    // If it looks like HTML, strip tags naively; otherwise return as-is
    if text.trim_start().starts_with('<') {
        Ok(strip_html(&text))
    } else {
        Ok(text)
    }
}

/// Very light HTML stripper for `tome add --url` (not Confluence storage format).
fn strip_html(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Collapse whitespace runs
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
