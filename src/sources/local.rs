use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use walkdir::WalkDir;

use super::Source;

pub struct LocalSource {
    root: PathBuf,
}

impl LocalSource {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl Source for LocalSource {
    async fn fetch_content(&self, path: &str) -> Result<String> {
        let full_path = if path.is_empty() {
            self.root.clone()
        } else {
            self.root.join(path)
        };

        if full_path.is_dir() {
            // Return a directory listing with file contents concatenated
            list_directory(&full_path)
        } else {
            std::fs::read_to_string(&full_path)
                .with_context(|| format!("Failed to read file: {}", full_path.display()))
        }
    }
}

/// Walk a directory and return a markdown-formatted listing.
fn list_directory(dir: &std::path::Path) -> Result<String> {
    let mut output = format!("# {}\n\n", dir.display());

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "md" | "txt" | "rst" | "markdown") {
            continue;
        }

        let rel = path.strip_prefix(dir).unwrap_or(path);
        output.push_str(&format!("## {}\n\n", rel.display()));

        match std::fs::read_to_string(path) {
            Ok(content) => output.push_str(&content),
            Err(e) => output.push_str(&format!("*Error reading file: {e}*\n")),
        }
        output.push_str("\n\n---\n\n");
    }

    Ok(output)
}

