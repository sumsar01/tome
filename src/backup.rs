//! Auto-backup: export all docs to a JSON file and optionally git-commit after writes.
//!
//! Enabled by setting `[backup] path = "/path/to/file.json"` in config.toml.
//! Silently skips if backup is not configured.
//!
//! # Security
//!
//! The backup JSON contains the full content of every inline doc stored in the
//! database.  **Only push to a PRIVATE repository.**  When `git_push = true`,
//! tome checks whether the upstream GitHub remote is public and refuses to push
//! if it is, to prevent accidental leakage of sensitive internal documentation.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{config::BackupConfig, db};

/// Run a backup after a mutating operation.
///
/// - Exports all docs to `cfg.path` as pretty-printed JSON.
/// - If `cfg.git` is true, runs `git add <file> && git commit -m "tome: backup"`.
/// - If `cfg.git_push` is true, also runs `git push` — but only after verifying
///   that the upstream GitHub remote is **private**.
/// - All failures are logged to stderr but never propagate — a backup failure
///   must not prevent the user's actual command from completing.
pub fn run(cfg: &BackupConfig, db: &db::Db, trigger: &str) {
    if cfg.path.is_empty() {
        return;
    }
    if let Err(e) = try_run(cfg, db, trigger) {
        eprintln!("tome: backup warning: {e}");
    }
}

fn try_run(cfg: &BackupConfig, db: &db::Db, trigger: &str) -> Result<()> {
    let path = Path::new(&cfg.path);

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create backup directory: {}", parent.display()))?;
    }

    // Export all docs to JSON.
    let json = export_json(db)?;
    std::fs::write(path, &json)
        .with_context(|| format!("Cannot write backup file: {}", path.display()))?;

    if cfg.git {
        git_commit(path, trigger, cfg.git_push)?;
    }

    Ok(())
}

fn export_json(db: &db::Db) -> Result<String> {
    let docs = db.list_docs(None, None)?;
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
    Ok(serde_json::to_string_pretty(&records)?)
}

fn git_commit(file: &Path, trigger: &str, push: bool) -> Result<()> {
    // Determine the repo root containing the backup file.
    let dir = file.parent().unwrap_or(Path::new("."));

    let run = |args: &[&str]| -> Result<()> {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .with_context(|| format!("Failed to run: git {}", args.join(" ")))?;
        if !status.success() {
            anyhow::bail!("git {} exited with {}", args.join(" "), status);
        }
        Ok(())
    };

    // Use the absolute path for `git add` so it works regardless of cwd.
    let abs = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf());
    let abs_str = abs.to_string_lossy();

    run(&["add", &abs_str])?;

    // Only commit if there's actually something staged (avoids "nothing to commit" errors).
    let diff_status = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(dir)
        .status()
        .context("Failed to check git staging area")?;

    if diff_status.success() {
        // Exit 0 means no staged changes — nothing to commit.
        return Ok(());
    }

    let msg = format!("tome: backup after {trigger}");
    run(&["commit", "-m", &msg])?;

    if push {
        // Safety check: refuse to push to a public GitHub repository.
        // The backup contains full doc content which may include internal/sensitive
        // documentation fetched from Confluence, private GitHub repos, or inline notes.
        guard_against_public_remote(dir)?;
        run(&["push"])?;
    }

    Ok(())
}

/// Inspect the git remote URL for the backup repo.  If it points at a public
/// GitHub repository, return an error rather than pushing sensitive content.
///
/// Non-GitHub remotes (GitLab, Bitbucket, self-hosted) cannot be checked via a
/// public API, so we print a warning and allow the push — the operator is
/// responsible for ensuring those repos are private.
fn guard_against_public_remote(repo_dir: &Path) -> Result<()> {
    // Fetch the remote URL from git.
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_dir)
        .output()
        .context("Failed to query git remote URL")?;

    if !output.status.success() {
        // No remote configured — local-only push would fail anyway; let git handle it.
        return Ok(());
    }

    let remote_url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Parse owner/repo from GitHub HTTPS or SSH URLs.
    // HTTPS: https://github.com/owner/repo[.git]
    // SSH:   git@github.com:owner/repo[.git]
    let github_slug = parse_github_slug(&remote_url);

    match github_slug {
        None => {
            // Non-GitHub remote — warn but do not block.
            eprintln!(
                "tome: backup: cannot verify that remote '{}' is private — \
                 ensure your backup repository is not publicly accessible.",
                remote_url
            );
            Ok(())
        }
        Some((owner, repo)) => {
            match is_github_repo_private(&owner, &repo) {
                Ok(true) => Ok(()), // private — safe to push
                Ok(false) => {
                    anyhow::bail!(
                        "backup aborted: '{owner}/{repo}' is a PUBLIC GitHub repository.\n\
                         The backup file contains your full doc content and must not be pushed publicly.\n\
                         Fix: set backup.git_push = false, or move the backup to a private repository."
                    )
                }
                Err(e) => {
                    // Cannot verify — fail safe: do not push.
                    anyhow::bail!(
                        "backup aborted: could not verify whether '{owner}/{repo}' is private ({e}).\n\
                         To allow pushing without the visibility check, use a non-GitHub remote or \
                         ensure network access to api.github.com."
                    )
                }
            }
        }
    }
}

/// Extract `(owner, repo)` from a GitHub remote URL, or return `None` for
/// non-GitHub remotes.
fn parse_github_slug(url: &str) -> Option<(String, String)> {
    // Normalise: strip trailing .git
    let url = url.trim_end_matches(".git");

    // HTTPS: https://github.com/owner/repo
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // SSH: git@github.com:owner/repo
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    None
}

/// Call the GitHub REST API to check whether a repository is private.
/// Returns `Ok(true)` if private, `Ok(false)` if public, `Err` on failure.
///
/// Uses a synchronous HTTP call (ureq-style via std + TcpStream is too involved;
/// instead we shell out to curl which is universally available on macOS/Linux).
/// This avoids adding a blocking reqwest call inside an async context and keeps
/// the backup module dependency-free beyond std.
fn is_github_repo_private(owner: &str, repo: &str) -> Result<bool> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}");

    let output = std::process::Command::new("curl")
        .args([
            "--silent",
            "--max-time", "10",
            "--user-agent", "tome-backup/1",
            "--header", "Accept: application/vnd.github+json",
            &url,
        ])
        .output()
        .context("Failed to run curl to check GitHub repo visibility")?;

    if !output.status.success() {
        anyhow::bail!("curl exited with status {}", output.status);
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&body)
        .context("GitHub API returned invalid JSON")?;

    // If the repo doesn't exist or is truly private, the API returns either
    // 404 (with a "message" field) or the full object with "private": true.
    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
        if msg.contains("Not Found") {
            // Repo not found — could be private (unauthenticated can't see it)
            // or genuinely missing.  Treat as private to avoid blocking legit use.
            return Ok(true);
        }
        anyhow::bail!("GitHub API error: {msg}");
    }

    let private = json
        .get("private")
        .and_then(|v| v.as_bool())
        .context("GitHub API response missing 'private' field")?;

    Ok(private)
}
