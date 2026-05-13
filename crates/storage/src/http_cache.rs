//! HTTP resource cache поверх SQLite — кеш загруженных страниц,
//! шрифтов, картинок, CSS, JS.
//!
//! Phase 0 покрывает базовую часть RFC 9111:
//! - Хранение тела ответа + status code + content-type + сохранённые
//!   важные headers (ETag, Last-Modified);
//! - max-age (Cache-Control) — TTL;
//! - parse Cache-Control: no-store / no-cache / max-age=N / public /
//!   private (по RFC 9111 §5.2).
//!
//! Сложные части (revalidation через If-None-Match / If-Modified-Since,
//! Vary header, partial responses, stale-while-revalidate, immutable)
//! — отложены до интеграции с `lumen-network`.
//!
//! Cache key — URL + опц. top_level_site (для total cache protection —
//! партиционирование как у cookies).

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Распарсенные директивы Cache-Control. Из RFC 9111 §5.2 берём только
/// то, что нужно для базового storage-кеша; revalidation directives
/// (must-revalidate, no-transform) пока игнорируем.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheControl {
    pub no_store: bool,
    pub no_cache: bool,
    pub max_age: Option<i64>,
    pub public: bool,
    pub private: bool,
    pub immutable: bool,
}

impl CacheControl {
    /// Распарсить значение Cache-Control HTTP-заголовка.
    ///
    /// Формат: comma-separated directives, каждая `key` или `key=value`.
    /// Имена case-insensitive, value (если нужен) — числовая часть
    /// `max-age=NNN`. Невалидные / неизвестные директивы — игнор.
    pub fn parse(header: &str) -> Self {
        let mut out = Self::default();
        for raw in header.split(',') {
            let part = raw.trim();
            if part.is_empty() {
                continue;
            }
            let (key, val) = match part.find('=') {
                Some(i) => (
                    part[..i].trim().to_ascii_lowercase(),
                    part[i + 1..].trim().trim_matches('"'),
                ),
                None => (part.trim().to_ascii_lowercase(), ""),
            };
            match key.as_str() {
                "no-store" => out.no_store = true,
                "no-cache" => out.no_cache = true,
                "public" => out.public = true,
                "private" => out.private = true,
                "immutable" => out.immutable = true,
                "max-age" => {
                    if let Ok(n) = val.parse::<i64>() {
                        out.max_age = Some(n);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Можно ли вообще хранить ответ в кеше?
    pub fn is_cacheable(&self) -> bool {
        !self.no_store
    }
}

/// Кешированная HTTP-запись.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedResponse {
    pub url: String,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Unix timestamp истечения (`stored_at + max-age`). None — никогда
    /// не expired (immutable-style), session-кеш.
    pub expires_at: Option<i64>,
    /// Unix timestamp, когда запись положена в кеш.
    pub stored_at: i64,
}

impl CachedResponse {
    pub fn is_fresh(&self, now_unix: i64) -> bool {
        match self.expires_at {
            None => true,
            Some(exp) => now_unix < exp,
        }
    }
}

pub struct HttpCache {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for HttpCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpCache").finish()
    }
}

impl HttpCache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("http_cache open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("http_cache open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS http_cache (
                top_level_site TEXT NOT NULL DEFAULT '',
                url            TEXT NOT NULL,
                status         INTEGER NOT NULL,
                content_type   TEXT NOT NULL DEFAULT '',
                body           BLOB NOT NULL,
                etag           TEXT,
                last_modified  TEXT,
                expires_at     INTEGER,
                stored_at      INTEGER NOT NULL,
                PRIMARY KEY (top_level_site, url)
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS http_cache_expires_idx
                ON http_cache(expires_at);
            "#,
        )
        .map_err(|e| Error::Storage(format!("http_cache init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Положить ответ в кеш. Перезаписывает существующую запись с
    /// тем же (top_level_site, url).
    pub fn put(
        &self,
        url: &str,
        top_level_site: Option<&str>,
        response: &CachedResponse,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("http_cache mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO http_cache (top_level_site, url, status, content_type, body,
                                     etag, last_modified, expires_at, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (top_level_site, url) DO UPDATE SET
                 status = excluded.status,
                 content_type = excluded.content_type,
                 body = excluded.body,
                 etag = excluded.etag,
                 last_modified = excluded.last_modified,
                 expires_at = excluded.expires_at,
                 stored_at = excluded.stored_at",
            params![
                top_level_site.unwrap_or(""),
                url,
                response.status as i64,
                response.content_type,
                response.body,
                response.etag,
                response.last_modified,
                response.expires_at,
                response.stored_at,
            ],
        )
        .map_err(|e| Error::Storage(format!("http_cache put: {e}")))?;
        Ok(())
    }

    /// Получить ответ по URL. Возвращает `Some` даже если запись
    /// «протухла» — caller сам решает, использовать stale entry для
    /// revalidation (с ETag / Last-Modified) или сходить в сеть.
    pub fn get(&self, url: &str, top_level_site: Option<&str>) -> Result<Option<CachedResponse>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("http_cache mutex poisoned".into()))?;
        let r = conn
            .query_row(
                "SELECT url, status, content_type, body, etag, last_modified, expires_at, stored_at
                 FROM http_cache WHERE top_level_site = ?1 AND url = ?2",
                params![top_level_site.unwrap_or(""), url],
                |row| {
                    Ok(CachedResponse {
                        url: row.get(0)?,
                        status: row.get::<_, i64>(1)? as u16,
                        content_type: row.get(2)?,
                        body: row.get(3)?,
                        etag: row.get(4)?,
                        last_modified: row.get(5)?,
                        expires_at: row.get(6)?,
                        stored_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Storage(format!("http_cache get: {e}")))?;
        Ok(r)
    }

    /// Получить ответ, но только если он свежий (`now < expires_at`).
    /// Удобный helper для скейс «не надо в сеть».
    pub fn get_fresh(
        &self,
        url: &str,
        top_level_site: Option<&str>,
        now_unix: i64,
    ) -> Result<Option<CachedResponse>> {
        let entry = self.get(url, top_level_site)?;
        Ok(entry.filter(|e| e.is_fresh(now_unix)))
    }

    /// Удалить запись.
    pub fn delete(&self, url: &str, top_level_site: Option<&str>) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("http_cache mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM http_cache WHERE top_level_site = ?1 AND url = ?2",
            params![top_level_site.unwrap_or(""), url],
        )
        .map_err(|e| Error::Storage(format!("http_cache delete: {e}")))?;
        Ok(())
    }

    /// Удалить expired записи. Возвращает число удалённых строк.
    pub fn clear_expired(&self, now_unix: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("http_cache mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM http_cache WHERE expires_at IS NOT NULL AND expires_at < ?1",
                params![now_unix],
            )
            .map_err(|e| Error::Storage(format!("http_cache clear_expired: {e}")))?;
        Ok(n)
    }

    /// Полная очистка кеша.
    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("http_cache mutex poisoned".into()))?;
        conn.execute("DELETE FROM http_cache", [])
            .map_err(|e| Error::Storage(format!("http_cache clear: {e}")))?;
        Ok(())
    }

    /// Общее число записей.
    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("http_cache mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM http_cache", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("http_cache count: {e}")))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(url: &str, now: i64, max_age: Option<i64>) -> CachedResponse {
        CachedResponse {
            url: url.to_string(),
            status: 200,
            content_type: "text/html".into(),
            body: b"<html>...</html>".to_vec(),
            etag: Some("\"abc123\"".into()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
            expires_at: max_age.map(|m| now + m),
            stored_at: now,
        }
    }

    // ── CacheControl parsing ──

    #[test]
    fn parse_cache_control_basic() {
        let cc = CacheControl::parse("max-age=3600");
        assert_eq!(cc.max_age, Some(3600));
        assert!(!cc.no_store);
        assert!(cc.is_cacheable());
    }

    #[test]
    fn parse_cache_control_no_store() {
        let cc = CacheControl::parse("no-store");
        assert!(cc.no_store);
        assert!(!cc.is_cacheable());
    }

    #[test]
    fn parse_cache_control_multiple() {
        let cc = CacheControl::parse("public, max-age=7200, immutable");
        assert!(cc.public);
        assert_eq!(cc.max_age, Some(7200));
        assert!(cc.immutable);
    }

    #[test]
    fn parse_cache_control_case_insensitive() {
        let cc = CacheControl::parse("Max-Age=100, NO-STORE");
        assert_eq!(cc.max_age, Some(100));
        assert!(cc.no_store);
    }

    #[test]
    fn parse_cache_control_quoted_value() {
        // Некоторые серверы экранируют value: max-age="3600".
        let cc = CacheControl::parse("max-age=\"3600\"");
        assert_eq!(cc.max_age, Some(3600));
    }

    #[test]
    fn parse_cache_control_unknown_directives_ignored() {
        let cc = CacheControl::parse("must-revalidate, no-transform, max-age=10");
        assert_eq!(cc.max_age, Some(10));
        assert!(!cc.no_store);
    }

    #[test]
    fn parse_cache_control_empty_string() {
        let cc = CacheControl::parse("");
        assert!(!cc.no_store);
        assert!(!cc.no_cache);
        assert!(cc.max_age.is_none());
    }

    // ── HttpCache CRUD ──

    #[test]
    fn put_then_get() {
        let c = HttpCache::open_in_memory().unwrap();
        let r = sample("https://example.com/", 100, Some(3600));
        c.put("https://example.com/", None, &r).unwrap();
        let got = c.get("https://example.com/", None).unwrap().unwrap();
        assert_eq!(got, r);
    }

    #[test]
    fn get_missing_returns_none() {
        let c = HttpCache::open_in_memory().unwrap();
        assert!(c.get("https://nope/", None).unwrap().is_none());
    }

    #[test]
    fn put_overwrites_existing() {
        let c = HttpCache::open_in_memory().unwrap();
        let mut r = sample("https://x/", 100, Some(60));
        c.put("https://x/", None, &r).unwrap();
        r.body = b"updated".to_vec();
        c.put("https://x/", None, &r).unwrap();
        let got = c.get("https://x/", None).unwrap().unwrap();
        assert_eq!(got.body, b"updated".to_vec());
        assert_eq!(c.count().unwrap(), 1);
    }

    #[test]
    fn delete_removes_entry() {
        let c = HttpCache::open_in_memory().unwrap();
        let r = sample("https://x/", 100, Some(60));
        c.put("https://x/", None, &r).unwrap();
        c.delete("https://x/", None).unwrap();
        assert!(c.get("https://x/", None).unwrap().is_none());
    }

    #[test]
    fn is_fresh_within_max_age() {
        let r = sample("https://x/", 1000, Some(3600));
        // stored_at = 1000, expires_at = 4600.
        assert!(r.is_fresh(1000));
        assert!(r.is_fresh(4599));
        assert!(!r.is_fresh(4600));
        assert!(!r.is_fresh(5000));
    }

    #[test]
    fn is_fresh_no_expires_always_true() {
        let mut r = sample("https://x/", 1000, None);
        r.expires_at = None;
        assert!(r.is_fresh(0));
        assert!(r.is_fresh(i64::MAX));
    }

    #[test]
    fn get_fresh_skips_stale() {
        let c = HttpCache::open_in_memory().unwrap();
        let r = sample("https://x/", 100, Some(60));  // expires at 160
        c.put("https://x/", None, &r).unwrap();
        assert!(c.get_fresh("https://x/", None, 150).unwrap().is_some());
        assert!(c.get_fresh("https://x/", None, 200).unwrap().is_none());
        // get() остаётся доступен даже после expire — для revalidation.
        assert!(c.get("https://x/", None).unwrap().is_some());
    }

    #[test]
    fn clear_expired_removes_only_past() {
        let c = HttpCache::open_in_memory().unwrap();
        c.put(
            "https://expired/",
            None,
            &sample("https://expired/", 100, Some(60)),  // exp 160
        )
        .unwrap();
        c.put(
            "https://future/",
            None,
            &sample("https://future/", 100, Some(10000)),  // exp 10100
        )
        .unwrap();
        let n = c.clear_expired(500).unwrap();
        assert_eq!(n, 1);
        assert!(c.get("https://expired/", None).unwrap().is_none());
        assert!(c.get("https://future/", None).unwrap().is_some());
    }

    #[test]
    fn top_level_site_partitions_cache() {
        let c = HttpCache::open_in_memory().unwrap();
        let r_a = sample("https://shared/", 100, Some(60));
        let mut r_b = r_a.clone();
        r_b.body = b"variant_b".to_vec();
        c.put("https://shared/", Some("https://a.com"), &r_a).unwrap();
        c.put("https://shared/", Some("https://b.com"), &r_b).unwrap();

        let got_a = c
            .get("https://shared/", Some("https://a.com"))
            .unwrap()
            .unwrap();
        let got_b = c
            .get("https://shared/", Some("https://b.com"))
            .unwrap()
            .unwrap();
        assert_eq!(got_a.body, b"<html>...</html>".to_vec());
        assert_eq!(got_b.body, b"variant_b".to_vec());
    }

    #[test]
    fn clear_wipes_all() {
        let c = HttpCache::open_in_memory().unwrap();
        c.put("https://a/", None, &sample("https://a/", 100, Some(60)))
            .unwrap();
        c.put("https://b/", None, &sample("https://b/", 100, Some(60)))
            .unwrap();
        c.clear().unwrap();
        assert_eq!(c.count().unwrap(), 0);
    }

    #[test]
    fn binary_body_preserved() {
        let c = HttpCache::open_in_memory().unwrap();
        let mut r = sample("https://x/", 100, Some(60));
        r.body = (0..=255u8).collect();
        c.put("https://x/", None, &r).unwrap();
        let got = c.get("https://x/", None).unwrap().unwrap();
        assert_eq!(got.body, r.body);
    }

    #[test]
    fn etag_and_last_modified_round_trip() {
        let c = HttpCache::open_in_memory().unwrap();
        let r = sample("https://x/", 100, None);
        c.put("https://x/", None, &r).unwrap();
        let got = c.get("https://x/", None).unwrap().unwrap();
        assert_eq!(got.etag, Some("\"abc123\"".into()));
        assert_eq!(
            got.last_modified,
            Some("Wed, 21 Oct 2015 07:28:00 GMT".into())
        );
    }
}
