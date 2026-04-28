use anyhow::Result;
use clap::{Parser, Subcommand};
mod cache;
mod cmd;
mod config;
mod db;
mod http;
mod mcp;
mod paths;
mod sources;
mod tui;
mod util;

#[derive(Parser)]
#[command(name = "tome", version, about = "A docs reader for humans and AI")]
struct Cli {
    /// Path to a config file (overrides default and TOME_PROFILE)
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<String>,
    /// Named config profile to use (reads config.<profile>.toml; overridden by --config)
    /// Can also be set via the TOME_PROFILE environment variable.
    #[arg(long, global = true, value_name = "PROFILE")]
    profile: Option<String>,
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
    List {
        /// Filter by tag (substring match)
        #[arg(long)]
        tag: Option<String>,
        /// Filter by namespace (exact match, e.g. "whiteaway")
        #[arg(long)]
        namespace: Option<String>,
    },
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
    /// Remove a registered doc by alias, or bulk-remove all docs in a namespace
    Remove {
        /// Doc alias to remove (omit if using --namespace)
        alias: Option<String>,
        /// Remove all docs with this namespace instead of a single alias
        #[arg(long)]
        namespace: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Re-fetch a doc and refresh its cache
    Refresh {
        /// Doc alias to refresh
        alias: String,
    },
    /// Open the source URL for a doc in the default browser
    Open {
        /// Doc alias
        alias: String,
    },
    /// Export all registered docs to stdout as JSON
    Export {
        /// Output format: json | markdown
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Add a tag to an existing doc
    Tag {
        /// Doc alias
        alias: String,
        /// Tag to add
        tag: String,
    },
    /// Remove a tag from an existing doc
    Untag {
        /// Doc alias
        alias: String,
        /// Tag to remove
        tag: String,
    },
    /// Show fetch history for a doc
    History {
        /// Doc alias
        alias: String,
    },
    /// Show a diff between two versions of a doc
    Diff {
        /// Doc alias
        alias: String,
        /// First version index (default: second-to-last)
        #[arg(long, default_value = "0")]
        v1: usize,
        /// Second version index (default: last)
        #[arg(long, default_value = "0")]
        v2: usize,
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
        /// Workplace or context label (e.g. "whiteaway", "personal")
        #[arg(long)]
        namespace: Option<String>,
    },
    /// Assign or clear the namespace on an existing doc
    SetNamespace {
        /// Doc alias
        alias: String,
        /// Namespace to assign (e.g. "whiteaway"). Omit to clear.
        namespace: Option<String>,
        /// Clear the namespace (same as omitting <namespace>)
        #[arg(long)]
        clear: bool,
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

    // Resolve config path: --config > --profile > TOME_PROFILE env > default
    let cfg = if let Some(ref path) = cli.config {
        config::Config::load_from(&std::path::PathBuf::from(path))?
    } else if let Some(ref profile) = cli.profile {
        config::Config::load_from(&config::profile_config_path(profile))?
    } else {
        config::Config::load()?
    };

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
        Command::Get { alias, no_cache } => cmd::get(&cfg, &db, &alias, no_cache).await?,
        Command::List { tag, namespace } => cmd::list(&db, tag.as_deref(), namespace.as_deref())?,
        Command::Search { query } => cmd::search(&cfg, &db, &query).await?,
        Command::Auth { provider } => match provider {
            AuthProvider::Confluence { email } => config::auth::store_confluence_token(&email)?,
            AuthProvider::Github => config::auth::store_github_token()?,
        },
        Command::Cache { action } => match action {
            CacheAction::Clear => { cache::clear()?; println!("Cache cleared."); }
            CacheAction::Status => cache::status()?,
        },
        Command::Mcp => mcp::serve(cfg, db).await?,
        Command::Remove { alias, namespace, force } => cmd::remove(&db, alias.as_deref(), namespace.as_deref(), force)?,
        Command::Refresh { alias } => cmd::refresh(&cfg, &db, &alias).await?,
        Command::Open { alias } => cmd::open(&cfg, &db, &alias)?,
        Command::Export { format } => cmd::export(&db, &format)?,
        Command::Tag { alias, tag } => cmd::tag(&db, &alias, &tag)?,
        Command::Untag { alias, tag } => cmd::untag(&db, &alias, &tag)?,
        Command::History { alias } => cmd::history(&db, &alias)?,
        Command::Diff { alias, v1, v2 } => cmd::diff(&db, &alias, v1, v2)?,
        Command::Add { alias, file, url, tags, namespace } => cmd::add(&db, &alias, file, url, tags, namespace).await?,
        Command::SetNamespace { alias, namespace, clear } => {
            let ns = if clear { None } else { namespace.as_deref() };
            cmd::set_namespace(&db, &alias, ns)?;
        }
    }

    Ok(())
}

/// Derive the canonical source URL for a doc record.
pub(crate) fn source_url(cfg: &config::Config, doc: &db::DocRecord) -> anyhow::Result<String> {
    match doc.source.as_str() {
        db::SOURCE_INLINE => {
            anyhow::bail!("'{}' is an inline doc — it has no source URL.", doc.alias);
        }
        "local" => {
            anyhow::bail!("'{}' is a local file — it has no URL.", doc.alias);
        }
        source_name => {
            let scfg = cfg.find_source(source_name)
                .ok_or_else(|| anyhow::anyhow!("Source '{}' not found in config", source_name))?;
            match scfg.kind {
                config::SourceKind::Github => {
                    let repo = scfg.repo.as_deref().unwrap_or("");
                    let git_ref = scfg.git_ref.as_deref().unwrap_or("main");
                    let path = doc.path.as_deref().unwrap_or("");
                    Ok(format!("https://github.com/{}/blob/{}/{}", repo, git_ref, path))
                }
                config::SourceKind::Confluence => {
                    let base = scfg.base_url.as_deref().unwrap_or("").trim_end_matches('/');
                    if let Some(page_id) = &doc.page_id {
                        Ok(format!("{}/wiki/spaces/_/pages/{}", base, page_id))
                    } else if let Some(path) = &doc.path {
                        Ok(format!("{}/{}", base, path.trim_start_matches('/')))
                    } else {
                        anyhow::bail!("Confluence doc '{}' has no page_id or path.", doc.alias);
                    }
                }
                config::SourceKind::Local => {
                    anyhow::bail!("'{}' is a local source — it has no URL.", doc.alias);
                }
                config::SourceKind::Inline => {
                    anyhow::bail!("'{}' is an inline doc — it has no source URL.", doc.alias);
                }
            }
        }
    }
}

/// Open a URL in the system's default browser.
pub(crate) fn open_in_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()
        .map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()
        .map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd").args(["/c", "start", url]).spawn()
        .map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;
    Ok(())
}
