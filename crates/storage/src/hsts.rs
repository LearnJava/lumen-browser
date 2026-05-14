//! HSTS (HTTP Strict-Transport-Security) parser + per-host store.
//!
//! Spec: <https://datatracker.ietf.org/doc/html/rfc6797>. Сервер сообщает
//! `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload`,
//! и клиент в течение `max-age` секунд обязан обращаться к этому хосту
//! только по HTTPS (HTTP-запросы переадресуются на HTTPS).
//!
//! Phase 0: storage layer + парсер. Реальный upgrade-to-HTTPS в network —
//! отдельная задача (hook в `HttpClient::parse_url` для `Url::scheme`).

use std::path::Path;
use std::sync::Mutex;

use lumen_core::ext::HstsEnforcement;
use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HstsEntry {
    pub host: String,
    pub max_age_seconds: u64,
    pub include_subdomains: bool,
    pub preload: bool,
    /// Unix timestamp когда entry истечёт (`registered_at + max_age`).
    pub expires_at: i64,
}

/// Парсит Strict-Transport-Security header.
/// Возвращает (max_age, include_subdomains, preload). Невалидный header
/// (`max-age` отсутствует или не число) → None.
pub fn parse_sts_header(text: &str) -> Option<(u64, bool, bool)> {
    let mut max_age: Option<u64> = None;
    let mut include_subdomains = false;
    let mut preload = false;
    for piece in text.split(';') {
        let p = piece.trim();
        if p.is_empty() {
            continue;
        }
        if let Some(rest) = p.strip_prefix("max-age") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let v = rest.trim();
                // RFC 6797 §6.1.1: max-age может быть в кавычках. Снять их.
                let v = v.trim_matches('"');
                if let Ok(n) = v.parse::<u64>() {
                    max_age = Some(n);
                }
            }
        } else if p.eq_ignore_ascii_case("includeSubDomains") {
            include_subdomains = true;
        } else if p.eq_ignore_ascii_case("preload") {
            preload = true;
        }
    }
    max_age.map(|m| (m, include_subdomains, preload))
}

pub struct HstsStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for HstsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HstsStore").finish()
    }
}

impl HstsStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("hsts open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("hsts open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS hsts_hosts (
                host                TEXT PRIMARY KEY,
                max_age_seconds     INTEGER NOT NULL,
                include_subdomains  INTEGER NOT NULL DEFAULT 0,
                preload             INTEGER NOT NULL DEFAULT 0,
                expires_at          INTEGER NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS hsts_expires_idx ON hsts_hosts(expires_at);
            "#,
        )
        .map_err(|e| Error::Storage(format!("hsts init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Записать HSTS entry. `host` — lowercase ASCII hostname (без порта).
    /// `now_unix` — текущее время для вычисления `expires_at`.
    /// `max_age = 0` означает «снять HSTS» — удаляет entry.
    pub fn upsert(
        &self,
        host: &str,
        max_age: u64,
        include_subdomains: bool,
        preload: bool,
        now_unix: i64,
    ) -> Result<()> {
        if max_age == 0 {
            return self.delete(host);
        }
        let expires_at = now_unix.saturating_add(max_age as i64);
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO hsts_hosts (host, max_age_seconds, include_subdomains, preload, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (host) DO UPDATE SET
                 max_age_seconds = excluded.max_age_seconds,
                 include_subdomains = excluded.include_subdomains,
                 preload = excluded.preload,
                 expires_at = excluded.expires_at",
            params![
                host,
                max_age as i64,
                include_subdomains as i32,
                preload as i32,
                expires_at
            ],
        )
        .map_err(|e| Error::Storage(format!("hsts upsert: {e}")))?;
        Ok(())
    }

    /// Проверить, должен ли host обрабатываться как HTTPS-only.
    /// Учитывает `includeSubDomains` (если родительский домен помечен и
    /// `include_subdomains=true`, то и subdomain тоже HTTPS-only).
    /// `now_unix` нужен для отбрасывания просроченных entries.
    pub fn is_https_only(&self, host: &str, now_unix: i64) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        // Сначала точное совпадение.
        let exact = conn
            .query_row(
                "SELECT 1 FROM hsts_hosts WHERE host = ?1 AND expires_at > ?2",
                params![host, now_unix],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("hsts is_https_only-exact: {e}")))?
            .is_some();
        if exact {
            return Ok(true);
        }
        // Проверка subdomain: ищем родителей с include_subdomains=1.
        // Простой подход — итерируем по `host` отрезая ведущие labels.
        let mut h = host;
        while let Some(idx) = h.find('.') {
            h = &h[idx + 1..];
            if h.is_empty() {
                break;
            }
            let sub = conn
                .query_row(
                    "SELECT 1 FROM hsts_hosts
                     WHERE host = ?1 AND include_subdomains = 1 AND expires_at > ?2",
                    params![h, now_unix],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|e| Error::Storage(format!("hsts is_https_only-sub: {e}")))?
                .is_some();
            if sub {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn get(&self, host: &str) -> Result<Option<HstsEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        conn.query_row(
            "SELECT host, max_age_seconds, include_subdomains, preload, expires_at
             FROM hsts_hosts WHERE host = ?1",
            params![host],
            |r| {
                Ok(HstsEntry {
                    host: r.get(0)?,
                    max_age_seconds: r.get::<_, i64>(1)? as u64,
                    include_subdomains: r.get::<_, i32>(2)? != 0,
                    preload: r.get::<_, i32>(3)? != 0,
                    expires_at: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| Error::Storage(format!("hsts get: {e}")))
    }

    pub fn delete(&self, host: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        conn.execute("DELETE FROM hsts_hosts WHERE host = ?1", params![host])
            .map_err(|e| Error::Storage(format!("hsts delete: {e}")))?;
        Ok(())
    }

    /// Удалить все просроченные entries (для GC).
    pub fn purge_expired(&self, now_unix: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM hsts_hosts WHERE expires_at <= ?1",
                params![now_unix],
            )
            .map_err(|e| Error::Storage(format!("hsts purge_expired: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM hsts_hosts", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("hsts count: {e}")))?;
        Ok(n)
    }
}

/// Адаптер `HstsStore` к `lumen-core::ext::HstsEnforcement` — позволяет
/// `lumen-network::HttpClient` принимать `Arc<dyn HstsEnforcement>` без
/// прямой зависимости на lumen-storage.
///
/// Fail-open: ошибки persistence (диск умер, mutex отравлен) логируются в
/// stderr и трактуются как «нет HSTS» (`is_https_only → false`) или
/// silent drop (`record_sts`). Принципы — в doc-комментарии trait-а.
impl HstsEnforcement for HstsStore {
    fn is_https_only(&self, host: &str, now_unix: i64) -> bool {
        match HstsStore::is_https_only(self, host, now_unix) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("HstsStore::is_https_only error: {e}; treating as not-HSTS");
                false
            }
        }
    }

    fn record_sts(
        &self,
        host: &str,
        max_age: u64,
        include_subdomains: bool,
        preload: bool,
        now_unix: i64,
    ) {
        if let Err(e) =
            self.upsert(host, max_age, include_subdomains, preload, now_unix)
        {
            eprintln!("HstsStore::record_sts error: {e}; ignored");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sts_basic() {
        let r = parse_sts_header("max-age=31536000; includeSubDomains; preload").unwrap();
        assert_eq!(r.0, 31_536_000);
        assert!(r.1);
        assert!(r.2);
    }

    #[test]
    fn parse_sts_without_optionals() {
        let r = parse_sts_header("max-age=3600").unwrap();
        assert_eq!(r, (3600, false, false));
    }

    #[test]
    fn parse_sts_quoted_max_age() {
        let r = parse_sts_header(r#"max-age="3600""#).unwrap();
        assert_eq!(r.0, 3600);
    }

    #[test]
    fn parse_sts_case_insensitive_directives() {
        let r = parse_sts_header("max-age=600; INCLUDESUBDOMAINS; Preload").unwrap();
        assert!(r.1);
        assert!(r.2);
    }

    #[test]
    fn parse_sts_no_max_age_returns_none() {
        assert!(parse_sts_header("includeSubDomains").is_none());
        assert!(parse_sts_header("").is_none());
    }

    #[test]
    fn upsert_and_get() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, true, false, 100).unwrap();
        let e = s.get("example.com").unwrap().unwrap();
        assert_eq!(e.max_age_seconds, 3600);
        assert!(e.include_subdomains);
        assert!(!e.preload);
        assert_eq!(e.expires_at, 3700);
    }

    #[test]
    fn upsert_with_zero_max_age_deletes() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        s.upsert("example.com", 0, false, false, 200).unwrap();
        assert!(s.get("example.com").unwrap().is_none());
    }

    #[test]
    fn is_https_only_exact_match() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        assert!(s.is_https_only("example.com", 200).unwrap());
        assert!(!s.is_https_only("other.com", 200).unwrap());
    }

    #[test]
    fn is_https_only_subdomain_match_when_include_set() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, true, false, 100).unwrap();
        // sub.example.com — родитель example.com помечен includeSubDomains.
        assert!(s.is_https_only("sub.example.com", 200).unwrap());
        assert!(s.is_https_only("deep.sub.example.com", 200).unwrap());
    }

    #[test]
    fn is_https_only_no_subdomain_match_when_include_not_set() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        assert!(!s.is_https_only("sub.example.com", 200).unwrap());
    }

    #[test]
    fn expired_entry_not_https_only() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 100, false, false, 100).unwrap();
        // expires_at = 200; now=300 → expired.
        assert!(!s.is_https_only("example.com", 300).unwrap());
    }

    #[test]
    fn purge_expired_removes() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("a.com", 100, false, false, 100).unwrap(); // expires_at=200
        s.upsert("b.com", 1000, false, false, 100).unwrap(); // expires_at=1100
        let n = s.purge_expired(500).unwrap();
        assert_eq!(n, 1);
        assert!(s.get("a.com").unwrap().is_none());
        assert!(s.get("b.com").unwrap().is_some());
    }

    #[test]
    fn count_works() {
        let s = HstsStore::open_in_memory().unwrap();
        assert_eq!(s.count().unwrap(), 0);
        s.upsert("a.com", 3600, false, false, 100).unwrap();
        s.upsert("b.com", 3600, false, false, 100).unwrap();
        assert_eq!(s.count().unwrap(), 2);
    }
}
