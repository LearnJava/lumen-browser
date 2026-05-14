//! Search history — recent omnibox queries for autocomplete suggestions.
//!
//! Каждый запрос имеет:
//! - query: исходная строка (case-preserved, trim);
//! - normalized: lowercase для dedup и поиска (одна запись на normalized);
//! - frequency: счётчик использований;
//! - last_used: Unix timestamp последнего поиска.
//!
//! API оптимизировано под omnibox autocomplete: `recent(limit)` для
//! "недавние", `popular(limit)` для "часто используемые",
//! `prefix_match(prefix, limit)` для предикативного поиска.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub id: i64,
    /// Исходная строка пользователя (case-preserved).
    pub query: String,
    /// Lowercase-нормализованная (для dedup и prefix-match).
    pub normalized: String,
    pub frequency: i64,
    pub last_used: i64,
    pub first_used: i64,
}

pub struct SearchHistory {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SearchHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchHistory").finish()
    }
}

impl SearchHistory {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("search_history open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("search_history open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS search_queries (
                id          INTEGER PRIMARY KEY,
                query       TEXT NOT NULL,
                normalized  TEXT NOT NULL UNIQUE,
                frequency   INTEGER NOT NULL DEFAULT 1,
                last_used   INTEGER NOT NULL,
                first_used  INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS sq_last_used_idx ON search_queries(last_used DESC);
            CREATE INDEX IF NOT EXISTS sq_frequency_idx ON search_queries(frequency DESC);
            CREATE INDEX IF NOT EXISTS sq_normalized_prefix_idx ON search_queries(normalized);
            "#,
        )
        .map_err(|e| Error::Storage(format!("search_history init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Зафиксировать запрос. Если normalized уже в БД — инкрементит
    /// frequency и обновляет last_used; иначе вставляет новую строку.
    pub fn record(&self, query: &str, now_unix: i64) -> Result<()> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let normalized = trimmed.to_lowercase();
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO search_queries (query, normalized, last_used, first_used)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT (normalized) DO UPDATE SET
                 frequency = frequency + 1,
                 last_used = MAX(last_used, excluded.last_used),
                 query = excluded.query",
            params![trimmed, normalized, now_unix],
        )
        .map_err(|e| Error::Storage(format!("search_history record: {e}")))?;
        Ok(())
    }

    /// Последние N запросов по last_used DESC.
    pub fn recent(&self, limit: i64) -> Result<Vec<SearchQuery>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, query, normalized, frequency, last_used, first_used
                 FROM search_queries ORDER BY last_used DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("search_history recent prepare: {e}")))?;
        let rows = stmt
            .query_map(params![limit], row_to_query)
            .map_err(|e| Error::Storage(format!("search_history recent query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("search_history row: {e}")))?);
        }
        Ok(out)
    }

    /// Самые частые запросы (DESC by frequency, tie-break — last_used DESC).
    pub fn popular(&self, limit: i64) -> Result<Vec<SearchQuery>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, query, normalized, frequency, last_used, first_used
                 FROM search_queries ORDER BY frequency DESC, last_used DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("search_history popular prepare: {e}")))?;
        let rows = stmt
            .query_map(params![limit], row_to_query)
            .map_err(|e| Error::Storage(format!("search_history popular query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("search_history row: {e}")))?);
        }
        Ok(out)
    }

    /// Запросы, начинающиеся с `prefix` (case-insensitive). Сортировка
    /// по frequency DESC + last_used DESC.
    pub fn prefix_match(&self, prefix: &str, limit: i64) -> Result<Vec<SearchQuery>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        let prefix_lc = prefix.trim().to_lowercase();
        let pattern = format!("{}%", escape_sql_like(&prefix_lc));
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, query, normalized, frequency, last_used, first_used
                 FROM search_queries WHERE normalized LIKE ?1 ESCAPE '\\'
                 ORDER BY frequency DESC, last_used DESC LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("search_history prefix prepare: {e}")))?;
        let rows = stmt
            .query_map(params![pattern, limit], row_to_query)
            .map_err(|e| Error::Storage(format!("search_history prefix query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("search_history row: {e}")))?);
        }
        Ok(out)
    }

    pub fn delete_query(&self, normalized: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM search_queries WHERE normalized = ?1",
            params![normalized],
        )
        .map_err(|e| Error::Storage(format!("search_history delete: {e}")))?;
        Ok(())
    }

    pub fn delete_older_than(&self, before: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM search_queries WHERE last_used < ?1",
                params![before],
            )
            .map_err(|e| Error::Storage(format!("search_history delete_older: {e}")))?;
        Ok(n)
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        conn.execute("DELETE FROM search_queries", [])
            .map_err(|e| Error::Storage(format!("search_history clear: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_history mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM search_queries", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("search_history count: {e}")))?;
        Ok(n)
    }
}

/// SQL LIKE-pattern escape: `%`, `_`, и сам `\` → `\\?`.
fn escape_sql_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn row_to_query(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchQuery> {
    Ok(SearchQuery {
        id: row.get(0)?,
        query: row.get(1)?,
        normalized: row.get(2)?,
        frequency: row.get(3)?,
        last_used: row.get(4)?,
        first_used: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> SearchHistory {
        SearchHistory::open_in_memory().unwrap()
    }

    #[test]
    fn record_inserts_new_query() {
        let s = make();
        s.record("rust async", 100).unwrap();
        let r = s.recent(10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].query, "rust async");
        assert_eq!(r[0].normalized, "rust async");
        assert_eq!(r[0].frequency, 1);
    }

    #[test]
    fn record_increments_existing() {
        let s = make();
        s.record("Rust", 100).unwrap();
        s.record("rust", 200).unwrap();  // case-insensitive dedup
        s.record("RUST", 300).unwrap();
        let r = s.recent(10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].frequency, 3);
        assert_eq!(r[0].last_used, 300);
        // query сохраняется последний (UPDATE excluded.query).
        assert_eq!(r[0].query, "RUST");
    }

    #[test]
    fn record_trims_whitespace() {
        let s = make();
        s.record("  hello world  ", 100).unwrap();
        assert_eq!(s.recent(1).unwrap()[0].query, "hello world");
    }

    #[test]
    fn record_empty_skipped() {
        let s = make();
        s.record("", 100).unwrap();
        s.record("   ", 200).unwrap();
        assert_eq!(s.count().unwrap(), 0);
    }

    #[test]
    fn recent_desc_by_last_used() {
        let s = make();
        s.record("a", 100).unwrap();
        s.record("c", 300).unwrap();
        s.record("b", 200).unwrap();
        let r = s.recent(10).unwrap();
        let qs: Vec<&str> = r.iter().map(|q| q.query.as_str()).collect();
        assert_eq!(qs, vec!["c", "b", "a"]);
    }

    #[test]
    fn popular_desc_by_frequency() {
        let s = make();
        s.record("rare", 100).unwrap();
        for i in 200..210 {
            s.record("frequent", i).unwrap();
        }
        s.record("medium", 150).unwrap();
        s.record("medium", 160).unwrap();
        let p = s.popular(10).unwrap();
        // frequent (10), medium (2), rare (1).
        assert_eq!(p[0].query, "frequent");
        assert_eq!(p[0].frequency, 10);
        assert_eq!(p[1].query, "medium");
        assert_eq!(p[1].frequency, 2);
        assert_eq!(p[2].query, "rare");
    }

    #[test]
    fn prefix_match_filters() {
        let s = make();
        s.record("rust async", 100).unwrap();
        s.record("rust borrow checker", 200).unwrap();
        s.record("python decorators", 300).unwrap();
        let r = s.prefix_match("rust", 10).unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn prefix_match_case_insensitive() {
        let s = make();
        s.record("Hello World", 100).unwrap();
        let r = s.prefix_match("hello", 10).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn prefix_match_escapes_sql_wildcards() {
        let s = make();
        s.record("50%off", 100).unwrap();
        s.record("50anything", 200).unwrap();
        // `%` в prefix не должен матчить произвольное.
        let r = s.prefix_match("50%", 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].query, "50%off");
    }

    #[test]
    fn delete_query_by_normalized() {
        let s = make();
        s.record("Rust", 100).unwrap();
        s.delete_query("rust").unwrap();
        assert_eq!(s.count().unwrap(), 0);
    }

    #[test]
    fn delete_older_than_removes_old() {
        let s = make();
        s.record("old", 100).unwrap();
        s.record("mid", 200).unwrap();
        s.record("new", 1000).unwrap();
        let removed = s.delete_older_than(500).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(s.count().unwrap(), 1);
        assert_eq!(s.recent(1).unwrap()[0].query, "new");
    }

    #[test]
    fn clear_wipes_all() {
        let s = make();
        s.record("a", 100).unwrap();
        s.record("b", 200).unwrap();
        s.clear().unwrap();
        assert_eq!(s.count().unwrap(), 0);
    }

    #[test]
    fn cyrillic_queries() {
        let s = make();
        s.record("рустовый async", 100).unwrap();
        let r = s.prefix_match("руст", 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].query, "рустовый async");
    }

    #[test]
    fn first_used_preserved() {
        let s = make();
        s.record("x", 100).unwrap();
        s.record("x", 500).unwrap();
        let r = s.recent(1).unwrap();
        assert_eq!(r[0].first_used, 100);
        assert_eq!(r[0].last_used, 500);
    }

    #[test]
    fn popular_tie_break_by_last_used() {
        let s = make();
        s.record("a", 100).unwrap();
        s.record("b", 200).unwrap();
        // Обе с frequency=1; tie-break по last_used DESC → b раньше a.
        let p = s.popular(10).unwrap();
        assert_eq!(p[0].query, "b");
        assert_eq!(p[1].query, "a");
    }
}
