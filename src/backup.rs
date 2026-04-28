//! Auto-backup: export all docs to a JSON file and optionally git-commit after writes.
//!
//! Enabled by setting `[backup] path = "/path/to/file.json"` in config.toml.
//! Silently skips if backup is not configured.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{config::BackupConfig, db};

/// Run a backup after a mutating operation.
///
/// - Exports all docs to `cfg.path` as pretty-printed JSON.
/// - If `cfg.git` is true, runs `git add <file> && git commit -m "tome: backup"`.
/// - If `cfg.git_push` is true, also runs `git push`.
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
    let docs = db.list_docs(None)?;
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
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(dir)
        .status()
        .context("Failed to check git staging area")?;

    if output.success() {
        // Exit 0 means no staged changes — nothing to commit.
        return Ok(());
    }

    let msg = format!("tome: backup after {trigger}");
    run(&["commit", "-m", &msg])?;

    if push {
        run(&["push"])?;
    }

    Ok(())
}
