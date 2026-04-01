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

/// A registered doc entry.
#[derive(Debug, Clone)]
pub struct DocRecord {
    pub alias: String,
    /// Source name: "inline", "whiteaway", a GitHub source name, etc.
    pub source: String,
    /// Confluence page ID (remote docs)
    pub page_id: Option<String>,
    /// File path within a GitHub/local source
    pub path: Option<String>,
    /// Tags for filtering and search
    pub tags: Vec<String>,
    /// Full markdown content — populated for inline docs, None for remote
    pub content: Option<String>,
}

/// Flat view used for listing (no content)
#[derive(Debug, Clone)]
pub struct DocInfo {
    pub alias: String,
    pub source: String,
    pub tags: Vec<String>,
}

/// A search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub alias: String,
    pub snippet: String,
    pub score: f64,
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
                alias    TEXT PRIMARY KEY,
                source   TEXT NOT NULL,
                page_id  TEXT,
                path     TEXT,
                tags     TEXT NOT NULL DEFAULT '[]',
                content  TEXT
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
            ",
        )
        .context("Failed to initialise database schema")?;
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
            "INSERT INTO docs (alias, source, page_id, path, tags, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                doc.alias,
                doc.source,
                doc.page_id,
                doc.path,
                tags_json,
                doc.content,
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
            "SELECT alias, source, page_id, path, tags, content FROM docs WHERE alias = ?1",
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

    /// List all docs, optionally filtered by tag.
    pub fn list_docs(&self, tag_filter: Option<&str>) -> Result<Vec<DocInfo>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare("SELECT alias, source, tags FROM docs ORDER BY alias ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut docs = Vec::new();
        for row in rows {
            let (alias, source, tags_json) = row?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            if let Some(filter) = tag_filter {
                if !tags.iter().any(|t| t.contains(filter)) {
                    continue;
                }
            }
            docs.push(DocInfo {
                alias,
                source,
                tags,
            });
        }
        Ok(docs)
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
             LIMIT 20",
        )?;
        let rows = stmt.query_map(params![query], |row| {
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
    })
}

/// Path to the SQLite database file.
pub fn db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tome")
        .join("tome.db")
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
        }
    }

    #[test]
    fn add_and_find() {
        let db = Db::open_in_memory().unwrap();
        db.add_doc(&make_doc("foo", "inline", Some("hello world")))
            .unwrap();
        let doc = db.find_doc("foo").unwrap().unwrap();
        assert_eq!(doc.alias, "foo");
        assert_eq!(doc.content.as_deref(), Some("hello world"));
    }

    #[test]
    fn duplicate_alias_errors() {
        let db = Db::open_in_memory().unwrap();
        db.add_doc(&make_doc("bar", "inline", Some("a"))).unwrap();
        assert!(db.add_doc(&make_doc("bar", "inline", Some("b"))).is_err());
    }

    #[test]
    fn list_with_tag_filter() {
        let db = Db::open_in_memory().unwrap();
        let mut doc = make_doc("a", "inline", None);
        doc.tags = vec!["rust".to_string()];
        db.add_doc(&doc).unwrap();
        let mut doc2 = make_doc("b", "inline", None);
        doc2.tags = vec!["python".to_string()];
        db.add_doc(&doc2).unwrap();

        let all = db.list_docs(None).unwrap();
        assert_eq!(all.len(), 2);
        let filtered = db.list_docs(Some("rust")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].alias, "a");
    }

    #[test]
    fn fts_search() {
        let db = Db::open_in_memory().unwrap();
        db.add_doc(&make_doc(
            "guide",
            "inline",
            Some("This is a guide about tokio async runtime"),
        ))
        .unwrap();
        db.add_doc(&make_doc(
            "other",
            "inline",
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
        db.add_doc(&make_doc("del", "inline", Some("bye"))).unwrap();
        assert!(db.alias_exists("del"));
        db.remove_doc("del").unwrap();
        assert!(!db.alias_exists("del"));
    }
}
