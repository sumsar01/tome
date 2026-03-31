use anyhow::Result;
use clap::{Parser, Subcommand};

mod cache;
mod config;
mod mcp;
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let cfg = config::Config::load()?;

    match cli.command.unwrap_or(Command::Browse) {
        Command::Browse => tui::run(cfg).await?,
        Command::Read { alias } => tui::run_reader(cfg, &alias).await?,
        Command::Get { alias, no_cache } => {
            let content = sources::fetch(&cfg, &alias, !no_cache).await?;
            print!("{content}");
        }
        Command::List => {
            let docs = cfg.list_docs();
            if docs.is_empty() {
                eprintln!("No docs configured. Add entries to ~/.config/tome/config.toml");
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
            let results = sources::search(&cfg, &query).await?;
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
            mcp::serve(cfg).await?;
        }
    }

    Ok(())
}
