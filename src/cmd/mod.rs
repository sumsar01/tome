use anyhow::Result;

use crate::{config, db, sources};

/// Width of the separator line used in tabular CLI output.
const TABLE_SEPARATOR_WIDTH: usize = 72;

/// `tome list` — print all registered doc aliases.
pub fn list(db: &db::Db, tag_filter: Option<&str>, namespace_filter: Option<&str>) -> Result<()> {
    let docs = db.list_docs(tag_filter, namespace_filter)?;
    if docs.is_empty() {
        eprintln!("No docs configured. Use `tome add` to add docs.");
    } else {
        println!("{:<24} {:<16} {:<16} TAGS", "ALIAS", "SOURCE", "NAMESPACE");
        println!("{}", "-".repeat(TABLE_SEPARATOR_WIDTH));
        for doc in docs {
            println!(
                "{:<24} {:<16} {:<16} {}",
                doc.alias,
                doc.source,
                doc.namespace.as_deref().unwrap_or("-"),
                doc.tags.join(", ")
            );
        }
    }
    Ok(())
}

/// `tome get` — print doc content to stdout.
pub async fn get(cfg: &config::Config, db: &db::Db, alias: &str, no_cache: bool) -> Result<()> {
    let content = sources::fetch(cfg, db, alias, !no_cache).await?;
    print!("{content}");
    Ok(())
}

/// `tome search` — fuzzy search across all docs.
pub async fn search(cfg: &config::Config, db: &db::Db, query: &str) -> Result<()> {
    let results = sources::search(cfg, db, query).await?;
    if results.is_empty() {
        eprintln!("No results for '{query}'");
    } else {
        for r in results {
            println!("{} — {}", r.alias, r.snippet);
        }
    }
    Ok(())
}

/// `tome remove` — remove a registered doc (by alias) or all docs in a namespace.
pub fn remove(db: &db::Db, alias: Option<&str>, namespace: Option<&str>, force: bool) -> Result<()> {
    if let Some(ns) = namespace {
        // Bulk delete by namespace
        let docs = db.list_docs(None, Some(ns))?;
        if docs.is_empty() {
            eprintln!("No docs with namespace '{ns}' found.");
            return Ok(());
        }
        if !force {
            eprint!(
                "Remove {} doc(s) with namespace '{ns}'? [y/N] ",
                docs.len()
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                eprintln!("Aborted.");
                return Ok(());
            }
        }
        let mut removed = 0usize;
        for doc in &docs {
            db.remove_doc(&doc.alias)?;
            removed += 1;
        }
        println!("Removed {removed} doc(s) with namespace '{ns}'.");
        return Ok(());
    }

    let alias = alias.ok_or_else(|| anyhow::anyhow!("Provide either an alias or --namespace <ns>"))?;
    if !db.alias_exists(alias) {
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
    db.remove_doc(alias)?;
    println!("Removed '{alias}'.");
    Ok(())
}

/// `tome set-namespace` — assign or clear the namespace on a doc.
pub fn set_namespace(db: &db::Db, alias: &str, namespace: Option<&str>) -> Result<()> {
    if !db.alias_exists(alias) {
        anyhow::bail!("No doc with alias '{}' found.", alias);
    }
    let updated = db.update_namespace(alias, namespace)?;
    if !updated {
        anyhow::bail!("Failed to update namespace for '{alias}'.");
    }
    match namespace {
        Some(ns) => println!("Set namespace '{ns}' on '{alias}'."),
        None => println!("Cleared namespace on '{alias}'."),
    }
    Ok(())
}

/// `tome update` — update metadata fields on an existing doc.
pub fn update(
    db: &db::Db,
    alias: &str,
    category: Option<&str>,
    clear_category: bool,
    namespace: Option<&str>,
    clear_namespace: bool,
    tags: Option<&str>,
    add_tag: Option<&str>,
    remove_tag: Option<&str>,
) -> Result<()> {
    if !db.alias_exists(alias) {
        anyhow::bail!("No doc with alias '{}' found.", alias);
    }

    let mut changed: Vec<String> = Vec::new();

    // category
    if clear_category {
        db.update_category(alias, None)?;
        changed.push("category cleared".to_string());
    } else if let Some(cat) = category {
        db.update_category(alias, Some(cat))?;
        changed.push(format!("category set to '{cat}'"));
    }

    // namespace
    if clear_namespace {
        db.update_namespace(alias, None)?;
        changed.push("namespace cleared".to_string());
    } else if let Some(ns) = namespace {
        db.update_namespace(alias, Some(ns))?;
        changed.push(format!("namespace set to '{ns}'"));
    }

    // full tag replacement
    if let Some(tags_str) = tags {
        let new_tags: Vec<String> = tags_str
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        db.update_tags(alias, &new_tags)?;
        if new_tags.is_empty() {
            changed.push("tags cleared".to_string());
        } else {
            changed.push(format!("tags set to: {}", new_tags.join(", ")));
        }
    } else {
        // incremental tag operations
        if let Some(tag) = add_tag {
            let doc = db.find_doc(alias)?.ok_or_else(|| anyhow::anyhow!("Doc not found"))?;
            let mut new_tags = doc.tags.clone();
            let tag = tag.trim().to_string();
            if new_tags.contains(&tag) {
                eprintln!("Tag '{}' already present on '{}'.", tag, alias);
            } else {
                new_tags.push(tag.clone());
                db.update_tags(alias, &new_tags)?;
                changed.push(format!("added tag '{tag}'"));
            }
        }
        if let Some(tag) = remove_tag {
            let doc = db.find_doc(alias)?.ok_or_else(|| anyhow::anyhow!("Doc not found"))?;
            let original_len = doc.tags.len();
            let new_tags: Vec<String> = doc.tags.into_iter().filter(|t| t != tag).collect();
            if new_tags.len() == original_len {
                anyhow::bail!("Tag '{}' not found on '{}'.", tag, alias);
            }
            db.update_tags(alias, &new_tags)?;
            changed.push(format!("removed tag '{tag}'"));
        }
    }

    if changed.is_empty() {
        eprintln!("Nothing to update. Provide at least one field to change.");
    } else {
        println!("Updated '{}': {}.", alias, changed.join("; "));
    }
    Ok(())
}

/// `tome categorize` — assign a category to a doc.
pub fn categorize(db: &db::Db, alias: &str, category: &str) -> Result<()> {
    if !db.alias_exists(alias) {
        anyhow::bail!("No doc with alias '{}' found.", alias);
    }
    db.update_category(alias, Some(category))?;
    println!("Set category '{}' on '{}'.", category, alias);
    Ok(())
}

/// `tome uncategorize` — clear the category from a doc.
pub fn uncategorize(db: &db::Db, alias: &str) -> Result<()> {
    if !db.alias_exists(alias) {
        anyhow::bail!("No doc with alias '{}' found.", alias);
    }
    db.update_category(alias, None)?;
    println!("Cleared category from '{}'.", alias);
    Ok(())
}

/// `tome rename` — rename a doc alias.
pub fn rename(db: &db::Db, old_alias: &str, new_alias: &str) -> Result<()> {
    db.rename_doc(old_alias, new_alias)?;
    println!("Renamed '{}' → '{}'.", old_alias, new_alias);
    Ok(())
}

/// `tome refresh` — re-fetch a doc and refresh its cache.
pub async fn refresh(cfg: &config::Config, db: &db::Db, alias: &str) -> Result<()> {
    if !db.alias_exists(alias) {
        anyhow::bail!("No doc with alias '{}' found.", alias);
    }
    crate::cache::invalidate(alias)?;
    let content = sources::fetch(cfg, db, alias, false).await?;
    println!("Refreshed '{alias}' ({} bytes).", content.len());
    Ok(())
}

/// `tome open` — open the source URL for a doc in the default browser.
pub fn open(cfg: &config::Config, db: &db::Db, alias: &str) -> Result<()> {
    let doc = db.find_doc(alias)?
        .ok_or_else(|| anyhow::anyhow!("No doc with alias '{}' found.", alias))?;
    let url = crate::source_url(cfg, &doc)?;
    crate::open_in_browser(&url)?;
    println!("Opening {url}");
    Ok(())
}

/// `tome export` — export all docs to stdout.
pub fn export(db: &db::Db, format: &str) -> Result<()> {
    let docs = db.list_docs(None, None)?;
    match format {
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
                        "namespace": doc.namespace,
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
    Ok(())
}

/// `tome tag` — add a tag to an existing doc.
pub fn tag(db: &db::Db, alias: &str, tag: &str) -> Result<()> {
    let doc = db.find_doc(alias)?
        .ok_or_else(|| anyhow::anyhow!("No doc with alias '{}' found.", alias))?;
    let mut tags = doc.tags.clone();
    let tag = tag.trim().to_string();
    if tags.contains(&tag) {
        eprintln!("Tag '{}' already present on '{}'.", tag, alias);
    } else {
        tags.push(tag.clone());
        db.update_tags(alias, &tags)?;
        println!("Added tag '{}' to '{}'.", tag, alias);
    }
    Ok(())
}

/// `tome untag` — remove a tag from an existing doc.
pub fn untag(db: &db::Db, alias: &str, tag: &str) -> Result<()> {
    let doc = db.find_doc(alias)?
        .ok_or_else(|| anyhow::anyhow!("No doc with alias '{}' found.", alias))?;
    let original_len = doc.tags.len();
    let tags: Vec<String> = doc.tags.into_iter().filter(|t| t != tag).collect();
    if tags.len() == original_len {
        anyhow::bail!("Tag '{}' not found on '{}'.", tag, alias);
    }
    db.update_tags(alias, &tags)?;
    println!("Removed tag '{}' from '{}'.", tag, alias);
    Ok(())
}

/// `tome history` — show fetch history for a doc.
pub fn history(db: &db::Db, alias: &str) -> Result<()> {
    let versions = db.list_versions(alias)?;
    if versions.is_empty() {
        eprintln!("No history for '{alias}'. Fetch the doc at least once with `tome get` or `tome refresh`.");
    } else {
        println!("{:<4} {:<28} {:<10} ALIAS", "#", "FETCHED AT", "HASH");
        println!("{}", "-".repeat(TABLE_SEPARATOR_WIDTH));
        for v in &versions {
            println!("{:<4} {:<28} {:<10} {}", v.version, v.fetched_at, v.content_hash, v.alias);
        }
    }
    Ok(())
}

/// `tome diff` — show a diff between two versions of a doc.
pub fn diff(db: &db::Db, alias: &str, v1: usize, v2: usize) -> Result<()> {
    let versions = db.list_versions(alias)?;
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
    print!("{}", crate::util::diff::unified_diff(&a.content, &b.content));
    Ok(())
}

/// `tome add` — save a local markdown file or URL as an inline doc.
pub async fn add(
    db: &db::Db,
    alias: &str,
    file: Option<String>,
    url: Option<String>,
    tags: Option<String>,
    namespace: Option<String>,
    category: Option<String>,
) -> Result<()> {
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

    db.add_doc(&db::DocRecord {
        alias: alias.to_string(),
        source: db::SOURCE_INLINE.to_string(),
        page_id: None,
        path: None,
        tags: tags_vec.clone(),
        content: Some(content),
        namespace,
        category: category.clone(),
    })?;
    println!(
        "Saved '{}' (category: {}, tags: {})",
        alias,
        category.as_deref().unwrap_or("(none)"),
        if tags_vec.is_empty() { "(none)".to_string() } else { tags_vec.join(", ") }
    );
    Ok(())
}

/// Fetch a URL and return its content as markdown (best-effort).
async fn fetch_url(url: &str) -> Result<String> {
    let client = crate::http::build_http_client()?;
    let resp = client.get(url).send().await
        .map_err(|e| anyhow::anyhow!("Failed to fetch '{url}': {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching '{url}'", resp.status());
    }
    let text = resp.text().await?;
    // If it looks like HTML, convert to markdown; otherwise return as-is
    if text.trim_start().starts_with('<') {
        Ok(crate::util::html::html_to_markdown(&text))
    } else {
        Ok(text)
    }
}
