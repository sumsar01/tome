use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::paths;

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    content: String,
    fetched_at: DateTime<Utc>,
    ttl_seconds: u64,
}

impl CacheEntry {
    fn is_fresh(&self) -> bool {
        let age = Utc::now()
            .signed_duration_since(self.fetched_at)
            .num_seconds();
        age >= 0 && (age as u64) < self.ttl_seconds
    }
}

fn cache_dir() -> PathBuf {
    paths::app_cache_dir()
}

fn entry_path(alias: &str) -> PathBuf {
    // Sanitize alias to be filesystem-safe
    let safe = alias.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    cache_dir().join(format!("{safe}.json"))
}

/// Try to load a cached doc. Returns None if missing or stale.
pub fn get(alias: &str, ttl_seconds: u64) -> Option<String> {
    let path = entry_path(alias);
    let data = std::fs::read_to_string(&path).ok()?;
    let mut entry: CacheEntry = serde_json::from_str(&data).ok()?;
    entry.ttl_seconds = ttl_seconds;
    if entry.is_fresh() {
        Some(entry.content)
    } else {
        None
    }
}

/// Store doc content in the cache.
pub fn set(alias: &str, content: &str, ttl_seconds: u64) -> Result<()> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache dir: {}", dir.display()))?;

    let entry = CacheEntry {
        content: content.to_string(),
        fetched_at: Utc::now(),
        ttl_seconds,
    };
    let data = serde_json::to_string(&entry).context("Failed to serialize cache entry")?;
    std::fs::write(entry_path(alias), data).context("Failed to write cache entry")?;
    Ok(())
}

/// Clear all cached docs.
pub fn clear() -> Result<()> {
    let dir = cache_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("Failed to clear cache dir: {}", dir.display()))?;
    }
    Ok(())
}

/// Print cache status.
pub fn status() -> Result<()> {
    let dir = cache_dir();
    if !dir.exists() {
        println!("Cache directory does not exist: {}", dir.display());
        return Ok(());
    }

    let entries: Vec<_> = std::fs::read_dir(&dir)
        .context("Failed to read cache dir")?
        .filter_map(|e| e.ok())
        .collect();

    let total_size: u64 = entries
        .iter()
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();

    println!("Cache directory: {}", dir.display());
    println!("Entries: {}", entries.len());
    println!("Total size: {} KB", total_size / 1024);
    Ok(())
}
