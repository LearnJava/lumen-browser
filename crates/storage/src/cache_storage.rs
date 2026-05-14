//! Cache Storage API persistence — `caches.open(name)` ↔ запросы/ответы.
//!
//! Spec: <https://w3c.github.io/ServiceWorker/#cache-objects>. Каждая
//! origin держит набор именованных кэшей; внутри каждого — пары
//! (request_url, response). Поддерживаются: open/put/match/delete/keys
//! на уровне cache, и `keys()` для перечисления имён кэшей origin-а.
//!
//! Phase 0: SQLite-таблица + методы. Реальная интеграция с fetch event
//! (ServiceWorker `event.respondWith(caches.match(...))`) — задача
//! отдельно (Phase 3+ с SW runtime).

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedEntry {
    pub origin: String,
    pub cache_name: String,
    pub request_url: String,
    pub request_method: String,
    pub response_status: u16,
    pub response_headers: String,
    pub response_body: Vec<u8>,
    pub cached_at: i64,
}

pub struct CacheStorage {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for CacheStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheStorage").finish()
    }
}

impl CacheStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("cache_storage open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("cache_storage open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS cache_entries (
                origin           TEXT NOT NULL,
                cache_name       TEXT NOT NULL,
                request_url      TEXT NOT NULL,
                request_method   TEXT NOT NULL DEFAULT 'GET',
                response_status  INTEGER NOT NULL,
                response_headers TEXT NOT NULL DEFAULT '',
                response_body    BLOB NOT NULL,
                cached_at        INTEGER NOT NULL,
                PRIMARY KEY (origin, cache_name, request_url, request_method)
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS cache_origin_name_idx ON cache_entries(origin, cache_name);
            "#,
        )
        .map_err(|e| Error::Storage(format!("cache_storage init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// `cache.put(request, response)` — записать пару.
    #[allow(clippy::too_many_arguments)]
    pub fn put(
        &self,
        origin: &str,
        cache_name: &str,
        request_url: &str,
        request_method: &str,
        response_status: u16,
        response_headers: &str,
        response_body: &[u8],
        cached_at: i64,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO cache_entries
                (origin, cache_name, request_url, request_method, response_status,
                 response_headers, response_body, cached_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (origin, cache_name, request_url, request_method)
             DO UPDATE SET
                 response_status = excluded.response_status,
                 response_headers = excluded.response_headers,
                 response_body = excluded.response_body,
                 cached_at = excluded.cached_at",
            params![
                origin,
                cache_name,
                request_url,
                request_method,
                response_status,
                response_headers,
                response_body,
                cached_at
            ],
        )
        .map_err(|e| Error::Storage(format!("cache_storage put: {e}")))?;
        Ok(())
    }

    /// `cache.match(request)` — найти ответ. Метод по умолчанию `GET`.
    pub fn match_(
        &self,
        origin: &str,
        cache_name: &str,
        request_url: &str,
        request_method: &str,
    ) -> Result<Option<CachedEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        conn.query_row(
            "SELECT origin, cache_name, request_url, request_method, response_status,
                    response_headers, response_body, cached_at
             FROM cache_entries
             WHERE origin = ?1 AND cache_name = ?2 AND request_url = ?3 AND request_method = ?4",
            params![origin, cache_name, request_url, request_method],
            row_to_entry,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("cache_storage match: {e}")))
    }

    /// `cache.delete(request)` — удалить пару. Возвращает true если удалили.
    pub fn delete(
        &self,
        origin: &str,
        cache_name: &str,
        request_url: &str,
        request_method: &str,
    ) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM cache_entries
                 WHERE origin = ?1 AND cache_name = ?2 AND request_url = ?3 AND request_method = ?4",
                params![origin, cache_name, request_url, request_method],
            )
            .map_err(|e| Error::Storage(format!("cache_storage delete: {e}")))?;
        Ok(n > 0)
    }

    /// `cache.keys()` — все entries в одном именованном кэше.
    pub fn keys(&self, origin: &str, cache_name: &str) -> Result<Vec<CachedEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT origin, cache_name, request_url, request_method, response_status,
                        response_headers, response_body, cached_at
                 FROM cache_entries
                 WHERE origin = ?1 AND cache_name = ?2
                 ORDER BY cached_at ASC",
            )
            .map_err(|e| Error::Storage(format!("cache_storage keys prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin, cache_name], row_to_entry)
            .map_err(|e| Error::Storage(format!("cache_storage keys query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("cache_storage row: {e}")))?);
        }
        Ok(out)
    }

    /// `caches.keys()` — список имён всех кэшей origin-а (distinct).
    pub fn list_cache_names(&self, origin: &str) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT DISTINCT cache_name FROM cache_entries
                 WHERE origin = ?1 ORDER BY cache_name ASC",
            )
            .map_err(|e| Error::Storage(format!("cache_storage list_names prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin], |r| r.get::<_, String>(0))
            .map_err(|e| Error::Storage(format!("cache_storage list_names query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("cache_storage row: {e}")))?);
        }
        Ok(out)
    }

    /// `caches.delete(name)` — удалить весь кэш с именем `cache_name`.
    pub fn delete_cache(&self, origin: &str, cache_name: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM cache_entries WHERE origin = ?1 AND cache_name = ?2",
                params![origin, cache_name],
            )
            .map_err(|e| Error::Storage(format!("cache_storage delete_cache: {e}")))?;
        Ok(n)
    }

    /// Очистить все entries для origin-а (origin storage clear).
    pub fn clear_origin(&self, origin: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM cache_entries WHERE origin = ?1",
                params![origin],
            )
            .map_err(|e| Error::Storage(format!("cache_storage clear_origin: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cache_storage mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM cache_entries", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("cache_storage count: {e}")))?;
        Ok(n)
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedEntry> {
    Ok(CachedEntry {
        origin: row.get(0)?,
        cache_name: row.get(1)?,
        request_url: row.get(2)?,
        request_method: row.get(3)?,
        response_status: row.get::<_, i64>(4)? as u16,
        response_headers: row.get(5)?,
        response_body: row.get(6)?,
        cached_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> CacheStorage {
        CacheStorage::open_in_memory().unwrap()
    }

    #[test]
    fn put_and_match() {
        let c = make();
        c.put(
            "https://app/",
            "static-v1",
            "/main.css",
            "GET",
            200,
            "content-type: text/css\r\n",
            b"body { color: red; }",
            100,
        )
        .unwrap();
        let e = c.match_("https://app/", "static-v1", "/main.css", "GET").unwrap().unwrap();
        assert_eq!(e.response_status, 200);
        assert_eq!(e.response_body, b"body { color: red; }");
    }

    #[test]
    fn put_overwrites_existing() {
        let c = make();
        c.put("https://app/", "v1", "/x", "GET", 200, "", b"v1", 100).unwrap();
        c.put("https://app/", "v1", "/x", "GET", 200, "", b"v2", 200).unwrap();
        assert_eq!(c.match_("https://app/", "v1", "/x", "GET").unwrap().unwrap().response_body, b"v2");
    }

    #[test]
    fn delete_returns_true_when_exists() {
        let c = make();
        c.put("https://app/", "v1", "/x", "GET", 200, "", b"", 100).unwrap();
        assert!(c.delete("https://app/", "v1", "/x", "GET").unwrap());
        assert!(c.match_("https://app/", "v1", "/x", "GET").unwrap().is_none());
    }

    #[test]
    fn delete_returns_false_when_missing() {
        let c = make();
        assert!(!c.delete("https://app/", "v1", "/x", "GET").unwrap());
    }

    #[test]
    fn keys_lists_all_in_cache() {
        let c = make();
        c.put("https://app/", "v1", "/a", "GET", 200, "", b"", 100).unwrap();
        c.put("https://app/", "v1", "/b", "GET", 200, "", b"", 200).unwrap();
        c.put("https://app/", "v2", "/c", "GET", 200, "", b"", 300).unwrap();
        let list = c.keys("https://app/", "v1").unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn list_cache_names_distinct() {
        let c = make();
        c.put("https://app/", "v1", "/a", "GET", 200, "", b"", 100).unwrap();
        c.put("https://app/", "v1", "/b", "GET", 200, "", b"", 200).unwrap();
        c.put("https://app/", "v2", "/c", "GET", 200, "", b"", 300).unwrap();
        let names = c.list_cache_names("https://app/").unwrap();
        assert_eq!(names, vec!["v1".to_string(), "v2".to_string()]);
    }

    #[test]
    fn delete_cache_removes_all_entries() {
        let c = make();
        c.put("https://app/", "v1", "/a", "GET", 200, "", b"", 100).unwrap();
        c.put("https://app/", "v1", "/b", "GET", 200, "", b"", 200).unwrap();
        let n = c.delete_cache("https://app/", "v1").unwrap();
        assert_eq!(n, 2);
        assert!(c.list_cache_names("https://app/").unwrap().is_empty());
    }

    #[test]
    fn clear_origin_removes_all_caches() {
        let c = make();
        c.put("https://app/", "v1", "/a", "GET", 200, "", b"", 100).unwrap();
        c.put("https://app/", "v2", "/b", "GET", 200, "", b"", 200).unwrap();
        c.put("https://other/", "v1", "/c", "GET", 200, "", b"", 300).unwrap();
        let n = c.clear_origin("https://app/").unwrap();
        assert_eq!(n, 2);
        assert_eq!(c.count().unwrap(), 1);
    }

    #[test]
    fn different_methods_are_distinct_entries() {
        let c = make();
        c.put("https://app/", "v1", "/x", "GET", 200, "", b"get", 100).unwrap();
        c.put("https://app/", "v1", "/x", "POST", 200, "", b"post", 200).unwrap();
        assert_eq!(c.match_("https://app/", "v1", "/x", "GET").unwrap().unwrap().response_body, b"get");
        assert_eq!(c.match_("https://app/", "v1", "/x", "POST").unwrap().unwrap().response_body, b"post");
    }

    #[test]
    fn cyrillic_url_and_body() {
        let c = make();
        c.put("https://пример/", "кэш", "/путь", "GET", 200, "", "Привет".as_bytes(), 100).unwrap();
        let e = c.match_("https://пример/", "кэш", "/путь", "GET").unwrap().unwrap();
        assert_eq!(e.response_body, "Привет".as_bytes());
    }
}
