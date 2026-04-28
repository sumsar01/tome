//! SQLite-backed doc registry with FTS5 full-text search.
//!
//! Database location: `~/Library/Application Support/tome/tome.db`
//!
//! Schema:
//! - `docs`      — registry of all doc aliases (both inline and remote)
//! - `docs_fts`  — FTS5 virtual table for full-text search of inline content

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::paths;

/// Source name used for docs whose content is stored directly in the DB.
pub const SOURCE_INLINE: &str = "inline";

/// Maximum number of FTS5 search results to return.
const MAX_SEARCH_RESULTS: usize = 20;

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 14695981039346656037;

/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 1099511628211;

/// A registered doc entry.
#[derive(Debug, Clone)]
pub struct DocRecord {
    pub alias: String,
    /// Source name: `SOURCE_INLINE`, or a named source from config (e.g. "whiteaway", a GitHub source name).
    pub source: String,
    /// Confluence page ID (remote docs)
    pub page_id: Option<String>,
    /// File path within a GitHub/local source
    pub path: Option<String>,
    /// Tags for filtering and search
    pub tags: Vec<String>,
    /// Full markdown content — populated for inline docs, None for remote
    pub content: Option<String>,
    /// Workplace / context label (e.g. "whiteaway", "personal"). NULL means unset.
    pub namespace: Option<String>,
}

/// Flat view used for listing (no content)
#[derive(Debug, Clone)]
pub struct DocInfo {
    pub alias: String,
    pub source: String,
    pub tags: Vec<String>,
    /// Workplace / context label
    pub namespace: Option<String>,
}

/// A search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub alias: String,
    pub snippet: String,
    pub score: f64,
}

/// A version entry in the doc history.
#[derive(Debug, Clone)]
pub struct DocVersion {
    /// 1-based index within this alias (oldest = 1)
    pub version: usize,
    pub alias: String,
    pub fetched_at: String,   // ISO 8601 UTC
    pub content_hash: String, // first 8 hex chars of SHA-256
    pub content: String,
}

/// Thread-safe handle to the SQLite database.
#[derive(Clone)]
pub struct Db {
    inner: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open (or create) the database at the standard platform path.
    pub fn open() -> Result<Self> {
        let path = db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create db dir: {}", parent.display()))?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open database: {}", path.display()))?;
        let db = Self {
            inner: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    /// Open an in-memory database (used in tests).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            inner: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    /// Create schema if not already present. Safe to call multiple times.
    fn init_schema(&self) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;

            CREATE TABLE IF NOT EXISTS docs (
                alias     TEXT PRIMARY KEY,
                source    TEXT NOT NULL,
                page_id   TEXT,
                path      TEXT,
                tags      TEXT NOT NULL DEFAULT '[]',
                content   TEXT,
                namespace TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5(
                alias,
                content,
                content='docs',
                content_rowid='rowid',
                tokenize='unicode61'
            );

            -- Keep FTS in sync on insert
            CREATE TRIGGER IF NOT EXISTS docs_ai
            AFTER INSERT ON docs BEGIN
                INSERT INTO docs_fts(rowid, alias, content)
                VALUES (new.rowid, new.alias, COALESCE(new.content, ''));
            END;

            -- Keep FTS in sync on update
            CREATE TRIGGER IF NOT EXISTS docs_au
            AFTER UPDATE ON docs BEGIN
                INSERT INTO docs_fts(docs_fts, rowid, alias, content)
                VALUES ('delete', old.rowid, old.alias, COALESCE(old.content, ''));
                INSERT INTO docs_fts(rowid, alias, content)
                VALUES (new.rowid, new.alias, COALESCE(new.content, ''));
            END;

            -- Keep FTS in sync on delete
            CREATE TRIGGER IF NOT EXISTS docs_ad
            AFTER DELETE ON docs BEGIN
                INSERT INTO docs_fts(docs_fts, rowid, alias, content)
                VALUES ('delete', old.rowid, old.alias, COALESCE(old.content, ''));
            END;

            CREATE TABLE IF NOT EXISTS doc_versions (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                alias        TEXT NOT NULL,
                fetched_at   TEXT NOT NULL,  -- ISO 8601 UTC
                content_hash TEXT NOT NULL,  -- first 8 hex chars of SHA-256
                content      TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS doc_versions_alias ON doc_versions(alias, id ASC);
            ",
        )
        .context("Failed to initialise database schema")?;

        // Migrate existing databases: add namespace column if absent.
        // ALTER TABLE … ADD COLUMN errors if the column exists; we ignore that.
        let _ = conn.execute_batch("ALTER TABLE docs ADD COLUMN namespace TEXT;");

        Ok(())
    }

    // ── Write operations ─────────────────────────────────────────────────────

    /// Add a new doc. Returns an error if the alias already exists.
    pub fn add_doc(&self, doc: &DocRecord) -> Result<()> {
        let tags_json = serde_json::to_string(&doc.tags).context("Failed to serialise tags")?;
        let conn = self.inner.lock().unwrap();
        let existing: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM docs WHERE alias = ?1",
                params![doc.alias],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if existing > 0 {
            anyhow::bail!(
                "alias '{}' already exists in tome. \
                 Use a different alias or remove the existing entry first.",
                doc.alias
            );
        }
        conn.execute(
            "INSERT INTO docs (alias, source, page_id, path, tags, content, namespace)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                doc.alias,
                doc.source,
                doc.page_id,
                doc.path,
                tags_json,
                doc.content,
                doc.namespace,
            ],
        )
        .with_context(|| format!("Failed to insert doc '{}'", doc.alias))?;
        Ok(())
    }

    /// Remove a doc by alias. Returns true if a row was deleted.
    pub fn remove_doc(&self, alias: &str) -> Result<bool> {
        let conn = self.inner.lock().unwrap();
        let n = conn
            .execute("DELETE FROM docs WHERE alias = ?1", params![alias])
            .with_context(|| format!("Failed to delete doc '{alias}'"))?;
        Ok(n > 0)
    }

    // ── Read operations ───────────────────────────────────────────────────────

    /// Find a doc by alias.
    pub fn find_doc(&self, alias: &str) -> Result<Option<DocRecord>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT alias, source, page_id, path, tags, content, namespace FROM docs WHERE alias = ?1",
        )?;
        let mut rows = stmt.query(params![alias])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_doc(row)?))
        } else {
            Ok(None)
        }
    }

    /// Check whether an alias exists.
    pub fn alias_exists(&self, alias: &str) -> bool {
        let conn = self.inner.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM docs WHERE alias = ?1",
            params![alias],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
            > 0
    }

    /// List all docs, optionally filtered by tag and/or namespace.
    pub fn list_docs(&self, tag_filter: Option<&str>, namespace_filter: Option<&str>) -> Result<Vec<DocInfo>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare("SELECT alias, source, tags, namespace FROM docs ORDER BY alias ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;

        let mut docs = Vec::new();
        for row in rows {
            let (alias, source, tags_json, namespace) = row?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            if let Some(filter) = tag_filter {
                if !tags.iter().any(|t| t.contains(filter)) {
                    continue;
                }
            }
            if let Some(ns) = namespace_filter {
                if namespace.as_deref() != Some(ns) {
                    continue;
                }
            }
            docs.push(DocInfo {
                alias,
                source,
                tags,
                namespace,
            });
        }
        Ok(docs)
    }

    /// Update the tags for a doc.
    pub fn update_tags(&self, alias: &str, tags: &[String]) -> Result<bool> {
        let tags_json = serde_json::to_string(tags).context("Failed to serialise tags")?;
        let conn = self.inner.lock().unwrap();
        let n = conn.execute(
            "UPDATE docs SET tags = ?1 WHERE alias = ?2",
            params![tags_json, alias],
        )?;
        Ok(n > 0)
    }

    /// Update (or clear) the namespace for a doc.
    /// Pass `None` to clear the namespace.
    pub fn update_namespace(&self, alias: &str, namespace: Option<&str>) -> Result<bool> {
        let conn = self.inner.lock().unwrap();
        let n = conn.execute(
            "UPDATE docs SET namespace = ?1 WHERE alias = ?2",
            params![namespace, alias],
        )?;
        Ok(n > 0)
    }

    // ── Versioning ────────────────────────────────────────────────────────────
    /// Record a new version of a doc. Skips insert if content hash is unchanged
    /// (i.e. the doc has not changed since the last fetch).
    pub fn record_version(&self, alias: &str, content: &str) -> Result<()> {
        let hash = content_hash(content);
        let conn = self.inner.lock().unwrap();

        // Check if the last stored hash matches
        let last_hash: Option<String> = conn
            .query_row(
                "SELECT content_hash FROM doc_versions WHERE alias = ?1 ORDER BY id DESC LIMIT 1",
                params![alias],
                |row| row.get(0),
            )
            .ok();

        if last_hash.as_deref() == Some(&hash) {
            return Ok(()); // unchanged — don't store a duplicate
        }

        let fetched_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO doc_versions (alias, fetched_at, content_hash, content)
             VALUES (?1, ?2, ?3, ?4)",
            params![alias, fetched_at, hash, content],
        )
        .with_context(|| format!("Failed to record version for '{alias}'"))?;
        Ok(())
    }

    /// List all recorded versions of a doc (oldest first).
    pub fn list_versions(&self, alias: &str) -> Result<Vec<DocVersion>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT alias, fetched_at, content_hash, content
             FROM doc_versions WHERE alias = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![alias], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut versions = Vec::new();
        for (i, row) in rows.enumerate() {
            let (alias, fetched_at, content_hash, content) = row?;
            versions.push(DocVersion {
                version: i + 1,
                alias,
                fetched_at,
                content_hash,
                content,
            });
        }
        Ok(versions)
    }

    // ── Search ────────────────────────────────────────────────────────────────

    /// Full-text search over inline doc content using FTS5 + BM25 ranking.
    /// Returns results ordered by relevance (best first).
    pub fn search_fts(&self, query: &str) -> Result<Vec<SearchResult>> {
        let conn = self.inner.lock().unwrap();
        // FTS5 bm25() returns negative values; ORDER BY ascending = best first.
        let mut stmt = conn.prepare(
            "SELECT alias, snippet(docs_fts, 1, '**', '**', '...', 20), bm25(docs_fts)
             FROM docs_fts
             WHERE docs_fts MATCH ?1
             ORDER BY bm25(docs_fts) ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, MAX_SEARCH_RESULTS as i64], |row| {
            Ok(SearchResult {
                alias: row.get(0)?,
                snippet: row.get(1)?,
                score: row.get::<_, f64>(2)?.abs(),
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn row_to_doc(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocRecord> {
    let tags_json: String = row.get(4)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(DocRecord {
        alias: row.get(0)?,
        source: row.get(1)?,
        page_id: row.get(2)?,
        path: row.get(3)?,
        tags,
        content: row.get(5)?,
        namespace: row.get(6)?,
    })
}

/// Compute the first 8 hex chars of a FNV-1a hash of `content` — fast and
/// collision-resistant enough to detect content changes between fetches.
pub fn content_hash(content: &str) -> String {
    let mut h: u64 = FNV_OFFSET_BASIS;
    for byte in content.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", h)[..8].to_string()
}

/// Path to the SQLite database file.
pub fn db_path() -> PathBuf {
    paths::app_data_dir().join("tome.db")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_doc(alias: &str, source: &str, content: Option<&str>) -> DocRecord {
        DocRecord {
            alias: alias.to_string(),
            source: source.to_string(),
            page_id: None,
            path: None,
            tags: vec!["test".to_string()],
            content: content.map(str::to_string),
            namespace: None,
        }
    }

    #[test]
    fn add_and_find() {
        let db = Db::open_in_memory().unwrap();
        db.add_doc(&make_doc("foo", SOURCE_INLINE, Some("hello world")))
            .unwrap();
        let doc = db.find_doc("foo").unwrap().unwrap();
        assert_eq!(doc.alias, "foo");
        assert_eq!(doc.content.as_deref(), Some("hello world"));
    }

    #[test]
    fn duplicate_alias_errors() {
        let db = Db::open_in_memory().unwrap();
        db.add_doc(&make_doc("bar", SOURCE_INLINE, Some("a")))
            .unwrap();
        assert!(db
            .add_doc(&make_doc("bar", SOURCE_INLINE, Some("b")))
            .is_err());
    }

    #[test]
    fn list_with_tag_filter() {
        let db = Db::open_in_memory().unwrap();
        let mut doc = make_doc("a", SOURCE_INLINE, None);
        doc.tags = vec!["rust".to_string()];
        db.add_doc(&doc).unwrap();
        let mut doc2 = make_doc("b", SOURCE_INLINE, None);
        doc2.tags = vec!["python".to_string()];
        db.add_doc(&doc2).unwrap();

        let all = db.list_docs(None, None).unwrap();
        assert_eq!(all.len(), 2);
        let filtered = db.list_docs(Some("rust"), None).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].alias, "a");
    }

    #[test]
    fn fts_search() {
        let db = Db::open_in_memory().unwrap();
        db.add_doc(&make_doc(
            "guide",
            SOURCE_INLINE,
            Some("This is a guide about tokio async runtime"),
        ))
        .unwrap();
        db.add_doc(&make_doc(
            "other",
            SOURCE_INLINE,
            Some("Completely unrelated content about cooking"),
        ))
        .unwrap();
        let results = db.search_fts("tokio").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].alias, "guide");
    }

    #[test]
    fn remove_doc() {
        let db = Db::open_in_memory().unwrap();
        db.add_doc(&make_doc("del", SOURCE_INLINE, Some("bye")))
            .unwrap();
        assert!(db.alias_exists("del"));
        db.remove_doc("del").unwrap();
        assert!(!db.alias_exists("del"));
    }
}
