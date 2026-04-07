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
        Command::Refresh { alias } => {
            if !db.alias_exists(&alias) {
                anyhow::bail!("No doc with alias '{}' found.", alias);
            }
            cache::invalidate(&alias)?;
            let content = sources::fetch(&cfg, &db, &alias, false).await?;
            println!("Refreshed '{alias}' ({} bytes).", content.len());
        }
        Command::Open { alias } => {
            let doc = db.find_doc(&alias)?
                .ok_or_else(|| anyhow::anyhow!("No doc with alias '{}' found.", alias))?;
            let url = source_url(&cfg, &doc)?;
            open_in_browser(&url)?;
            println!("Opening {url}");
        }
        Command::Export { format } => {
            let docs = db.list_docs(None)?;
            match format.as_str() {
                "json" => {
                    let mut records = Vec::new();
                    for info in &docs {
                        if let Ok(Some(doc)) = db.find_doc(&info.alias) {
                            records.push(serde_json::json!({
                                "alias": doc.alias,
                                "source": doc.source,
                                "page_id": doc.page_id,
                                "path": doc.path,
                                "tags": doc.tags,
                                "content": doc.content,
                            }));
                        }
                    }
                    println!("{}", serde_json::to_string_pretty(&records)?);
                }
                "markdown" => {
                    for info in &docs {
                        if let Ok(Some(doc)) = db.find_doc(&info.alias) {
                            println!("# {}", doc.alias);
                            println!("<!-- source: {} | tags: {} -->", doc.source, doc.tags.join(", "));
                            println!();
                            if let Some(content) = &doc.content {
                                println!("{}", content);
                            } else {
                                println!("_(remote doc — run `tome get {}` to fetch content)_", doc.alias);
                            }
                            println!("\n---\n");
                        }
                    }
                }
                other => {
                    anyhow::bail!("Unknown format '{other}'. Use 'json' or 'markdown'.");
                }
            }
        }
        Command::Tag { alias, tag } => {
            let doc = db.find_doc(&alias)?
                .ok_or_else(|| anyhow::anyhow!("No doc with alias '{}' found.", alias))?;
            let mut tags = doc.tags.clone();
            let tag = tag.trim().to_string();
            if tags.contains(&tag) {
                eprintln!("Tag '{}' already present on '{}'.", tag, alias);
            } else {
                tags.push(tag.clone());
                db.update_tags(&alias, &tags)?;
                println!("Added tag '{}' to '{}'.", tag, alias);
            }
        }
        Command::Untag { alias, tag } => {
            let doc = db.find_doc(&alias)?
                .ok_or_else(|| anyhow::anyhow!("No doc with alias '{}' found.", alias))?;
            let original_len = doc.tags.len();
            let tags: Vec<String> = doc.tags.into_iter().filter(|t| t != &tag).collect();
            if tags.len() == original_len {
                anyhow::bail!("Tag '{}' not found on '{}'.", tag, alias);
            }
            db.update_tags(&alias, &tags)?;
            println!("Removed tag '{}' from '{}'.", tag, alias);
        }
        Command::History { alias } => {
            let versions = db.list_versions(&alias)?;
            if versions.is_empty() {
                eprintln!("No history for '{alias}'. Fetch the doc at least once with `tome get` or `tome refresh`.");
            } else {
                println!("{:<4} {:<28} {:<10} ALIAS", "#", "FETCHED AT", "HASH");
                println!("{}", "-".repeat(60));
                for v in &versions {
                    println!("{:<4} {:<28} {:<10} {}", v.version, v.fetched_at, v.content_hash, v.alias);
                }
            }
        }
        Command::Diff { alias, v1, v2 } => {
            let versions = db.list_versions(&alias)?;
            if versions.len() < 2 {
                anyhow::bail!("Need at least 2 versions to diff. Run `tome refresh {alias}` to fetch a new version.");
            }
            // 0 means "use default": v1 = second-to-last, v2 = last
            let idx1 = if v1 == 0 { versions.len() - 2 } else { v1 - 1 };
            let idx2 = if v2 == 0 { versions.len() - 1 } else { v2 - 1 };
            let a = versions.get(idx1).ok_or_else(|| anyhow::anyhow!("Version {} not found", idx1 + 1))?;
            let b = versions.get(idx2).ok_or_else(|| anyhow::anyhow!("Version {} not found", idx2 + 1))?;
            println!("--- {} v{} ({})", alias, a.version, a.fetched_at);
            println!("+++ {} v{} ({})", alias, b.version, b.fetched_at);
            println!();
            print_unified_diff(&a.content, &b.content);
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

/// Derive the canonical source URL for a doc record.
fn source_url(cfg: &config::Config, doc: &db::DocRecord) -> anyhow::Result<String> {
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
fn open_in_browser(url: &str) -> anyhow::Result<()> {
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
    html_to_markdown(html)
}

/// Convert HTML to readable markdown.
/// Handles: headings, paragraphs, lists, code blocks, inline code, links, bold/italic, nav/script/style removal.
fn html_to_markdown(html: &str) -> String {
    let mut out = String::new();
    let mut chars = html.chars().peekable();
    let mut in_skip = false;       // inside <script>, <style>, <nav>, <header>, <footer>
    let mut in_pre = false;        // inside <pre>
    let mut list_depth: usize = 0;
    let mut ordered_counters: Vec<usize> = Vec::new();
    let mut pending_nl = 0usize;   // deferred newlines (avoids leading blanks)

    let flush_nl = |out: &mut String, n: usize| {
        for _ in 0..n {
            out.push('\n');
        }
    };

    while let Some(ch) = chars.next() {
        if ch != '<' {
            if !in_skip {
                if pending_nl > 0 {
                    flush_nl(&mut out, pending_nl);
                    pending_nl = 0;
                }
                if in_pre {
                    out.push(ch);
                } else {
                    // Collapse whitespace in normal text
                    if ch == '\n' || ch == '\r' {
                        if !out.ends_with(' ') && !out.ends_with('\n') {
                            out.push(' ');
                        }
                    } else {
                        out.push(ch);
                    }
                }
            }
            continue;
        }

        // Inside a tag — collect tag name and attributes
        let mut tag = String::new();
        let mut closing = false;
        if chars.peek() == Some(&'/') {
            closing = true;
            chars.next();
        }
        // Read tag name
        while let Some(&c) = chars.peek() {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ':' {
                tag.push(c.to_ascii_lowercase());
                chars.next();
            } else {
                break;
            }
        }
        // Drain rest of tag to '>'
        let mut attrs = String::new();
        let mut depth = 0i32;
        for c in chars.by_ref() {
            if c == '<' { depth += 1; }
            if c == '>' {
                if depth == 0 { break; }
                depth -= 1;
            }
            attrs.push(c);
        }

        let self_closing = attrs.trim_end().ends_with('/');

        // Skip tags
        let skip_tags = ["script", "style", "nav", "header", "footer", "aside",
                         "form", "button", "input", "svg", "iframe", "noscript"];
        if skip_tags.contains(&tag.as_str()) {
            if !closing && !self_closing { in_skip = true; }
            if closing { in_skip = false; }
            continue;
        }

        if in_skip { continue; }

        match (tag.as_str(), closing) {
            ("h1", false) => { pending_nl = pending_nl.max(2); if pending_nl > 0 && !out.is_empty() { flush_nl(&mut out, pending_nl); pending_nl = 0; } out.push_str("# "); }
            ("h2", false) => { pending_nl = pending_nl.max(2); if !out.is_empty() { flush_nl(&mut out, pending_nl); pending_nl = 0; } out.push_str("## "); }
            ("h3", false) => { pending_nl = pending_nl.max(2); if !out.is_empty() { flush_nl(&mut out, pending_nl); pending_nl = 0; } out.push_str("### "); }
            ("h4", false) => { pending_nl = pending_nl.max(2); if !out.is_empty() { flush_nl(&mut out, pending_nl); pending_nl = 0; } out.push_str("#### "); }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => { pending_nl = 2; }
            ("p", false) => { if !out.is_empty() { pending_nl = pending_nl.max(2); } }
            ("p", true) => { pending_nl = pending_nl.max(2); }
            ("br", _) => { out.push('\n'); }
            ("hr", _) => { out.push_str("\n\n---\n\n"); }
            ("pre", false) => { in_pre = true; pending_nl = pending_nl.max(2); if !out.is_empty() { flush_nl(&mut out, pending_nl); pending_nl = 0; } out.push_str("```\n"); }
            ("pre", true) => { in_pre = false; out.push_str("\n```"); pending_nl = 2; }
            ("code", false) if !in_pre => { out.push('`'); }
            ("code", true) if !in_pre => { out.push('`'); }
            ("strong" | "b", false) => { out.push_str("**"); }
            ("strong" | "b", true) => { out.push_str("**"); }
            ("em" | "i", false) => { out.push('_'); }
            ("em" | "i", true) => { out.push('_'); }
            ("ul", false) => { list_depth += 1; ordered_counters.push(0); pending_nl = pending_nl.max(1); }
            ("ul", true) => { list_depth = list_depth.saturating_sub(1); ordered_counters.pop(); pending_nl = pending_nl.max(1); }
            ("ol", false) => { list_depth += 1; ordered_counters.push(0); pending_nl = pending_nl.max(1); }
            ("ol", true) => { list_depth = list_depth.saturating_sub(1); ordered_counters.pop(); pending_nl = pending_nl.max(1); }
            ("li", false) => {
                flush_nl(&mut out, pending_nl.max(1));
                pending_nl = 0;
                let indent = "  ".repeat(list_depth.saturating_sub(1));
                if let Some(counter) = ordered_counters.last_mut() {
                    *counter += 1;
                    out.push_str(&format!("{}{}. ", indent, *counter));
                } else {
                    out.push_str(&format!("{}- ", indent));
                }
            }
            ("li", true) => { pending_nl = 1; }
            ("a", false) => {
                // Extract href from attrs
                if let Some(href) = extract_attr(&attrs, "href") {
                    out.push('[');
                    // We'll close the link on </a>; stash href somewhere
                    // Simple approach: push a placeholder and track
                    // For simplicity, just emit inline text (href appended after)
                    // We use a sentinel; real text follows, then </a> closes bracket
                    out.push_str("\x00HREF=");
                    out.push_str(&href);
                    out.push('\x00');
                }
            }
            ("a", true) => {
                // Close link bracket if we opened one
                // Look back for sentinel and restructure: [text](href)
                if let Some(start) = out.rfind("\x00HREF=") {
                    let sentinel_end = out[start..].find('\x00').map(|i| start + i + 1).unwrap_or(out.len());
                    let href: String = out[start + 6..sentinel_end - 1].to_string();
                    let text: String = out[sentinel_end..].to_string();
                    out.truncate(start);
                    if text.trim().is_empty() {
                        out.push_str(&href);
                    } else {
                        out.push('[');
                        out.push_str(text.trim());
                        out.push_str("](");
                        out.push_str(&href);
                        out.push(')');
                    }
                }
            }
            ("div" | "section" | "article" | "main", false) => { pending_nl = pending_nl.max(1); }
            ("div" | "section" | "article" | "main", true) => { pending_nl = pending_nl.max(1); }
            ("blockquote", false) => { pending_nl = pending_nl.max(2); }
            ("blockquote", true) => { pending_nl = pending_nl.max(2); }
            _ => {}
        }
    }

    // Decode HTML entities in the output
    decode_entities(out.trim())
}

/// Extract an attribute value from tag attribute string.
fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!("{}=", name);
    let pos = attrs.to_ascii_lowercase().find(&pattern)?;
    let after = &attrs[pos + pattern.len()..];
    if let Some(stripped) = after.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else if let Some(stripped) = after.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(stripped[..end].to_string())
    } else {
        let end = after.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

/// Decode common HTML entities.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&#39;", "'")
     .replace("&apos;", "'")
     .replace("&nbsp;", " ")
     .replace("&ndash;", "–")
     .replace("&mdash;", "—")
     .replace("&hellip;", "…")
}

/// Print a simple unified-style diff of two text blobs.
/// Uses an LCS-based approach: lines unique to `a` are shown as `-`, new in `b` as `+`.
/// Context lines (unchanged) shown as ` `. Only changed hunks + 3 context lines printed.
fn print_unified_diff(a: &str, b: &str) {
    let a_lines: Vec<&str> = a.lines().collect();
    let b_lines: Vec<&str> = b.lines().collect();

    // Build edit sequence with a simple O(n*m) LCS DP — fine for docs (< 10k lines).
    let m = a_lines.len();
    let n = b_lines.len();

    // dp[i][j] = length of LCS of a[..i] and b[..j]
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

    // Backtrack to produce edit operations: ('=', line), ('-', line), ('+', line)
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a_lines[i - 1] == b_lines[j - 1] {
            ops.push(('=', a_lines[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(('+', b_lines[j - 1]));
            j -= 1;
        } else {
            ops.push(('-', a_lines[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();

    // Print with context (3 lines around changes)
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

    let mut last_printed: Option<usize> = None;
    for (k, (op, line)) in ops.iter().enumerate() {
        if !printed[k] {
            continue;
        }
        if let Some(last) = last_printed {
            if k > last + 1 {
                println!("@@ ... @@");
            }
        }
        let prefix = match op {
            '-' => "-",
            '+' => "+",
            _ => " ",
        };
        println!("{}{}", prefix, line);
        last_printed = Some(k);
    }

    if last_printed.is_none() {
        println!("(no differences)");
    }
}
