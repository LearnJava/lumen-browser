//! DNS cache — кэш hostname → IP-адреса с TTL. Шаг к будущему §9.1
//! DoH/DoT resolver: пока storage-слой, который DnsResolver мог бы
//! использовать.
//!
//! Один origin может resolve-иться в множество IP (DNS round-robin),
//! поэтому `addresses` — список. TTL хранится как абсолютный
//! `expires_at`. Истёкшие записи не удаляются автоматически — `get`
//! фильтрует их (так же, как HttpCache); `clear_expired` зачищает.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsEntry {
    pub hostname: String,
    /// IP-адреса (IPv4 или IPv6 в string-form: `1.2.3.4` / `2001:db8::1`).
    pub addresses: Vec<String>,
    pub cached_at: i64,
    pub expires_at: i64,
}

impl DnsEntry {
    pub fn is_fresh(&self, now_unix: i64) -> bool {
        now_unix < self.expires_at
    }
}

pub struct DnsCache {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for DnsCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DnsCache").finish()
    }
}

impl DnsCache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("dns_cache open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("dns_cache open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // addresses хранится как `,`-separated string. Альтернатива — JSON,
        // но для простого list-IPv4/IPv6 это лишний overhead.
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS dns_cache (
                hostname    TEXT PRIMARY KEY,
                addresses   TEXT NOT NULL DEFAULT '',
                cached_at   INTEGER NOT NULL,
                expires_at  INTEGER NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS dns_expires_idx ON dns_cache(expires_at);
            "#,
        )
        .map_err(|e| Error::Storage(format!("dns_cache init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Сохранить DNS-resolve в кэше. Перезаписывает существующую запись
    /// для того же hostname.
    pub fn put(
        &self,
        hostname: &str,
        addresses: &[String],
        cached_at: i64,
        ttl_seconds: i64,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("dns_cache mutex poisoned".into()))?;
        let joined = addresses.join(",");
        conn.execute(
            "INSERT INTO dns_cache (hostname, addresses, cached_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (hostname) DO UPDATE SET
                 addresses = excluded.addresses,
                 cached_at = excluded.cached_at,
                 expires_at = excluded.expires_at",
            params![hostname, joined, cached_at, cached_at + ttl_seconds.max(0)],
        )
        .map_err(|e| Error::Storage(format!("dns_cache put: {e}")))?;
        Ok(())
    }

    /// Получить fresh-запись. Если истекла — `None` (caller идёт в DNS-resolver).
    pub fn get(&self, hostname: &str, now_unix: i64) -> Result<Option<DnsEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("dns_cache mutex poisoned".into()))?;
        let row: Option<DnsEntry> = conn
            .query_row(
                "SELECT hostname, addresses, cached_at, expires_at FROM dns_cache
                 WHERE hostname = ?1",
                params![hostname],
                |r| {
                    let addrs_str: String = r.get(1)?;
                    let addresses = if addrs_str.is_empty() {
                        Vec::new()
                    } else {
                        addrs_str.split(',').map(str::to_string).collect()
                    };
                    Ok(DnsEntry {
                        hostname: r.get(0)?,
                        addresses,
                        cached_at: r.get(2)?,
                        expires_at: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Storage(format!("dns_cache get: {e}")))?;
        Ok(row.filter(|e| e.is_fresh(now_unix)))
    }

    pub fn delete(&self, hostname: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("dns_cache mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM dns_cache WHERE hostname = ?1",
            params![hostname],
        )
        .map_err(|e| Error::Storage(format!("dns_cache delete: {e}")))?;
        Ok(())
    }

    pub fn clear_expired(&self, now_unix: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("dns_cache mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM dns_cache WHERE expires_at < ?1",
                params![now_unix],
            )
            .map_err(|e| Error::Storage(format!("dns_cache clear_expired: {e}")))?;
        Ok(n)
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("dns_cache mutex poisoned".into()))?;
        conn.execute("DELETE FROM dns_cache", [])
            .map_err(|e| Error::Storage(format!("dns_cache clear: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("dns_cache mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM dns_cache", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("dns_cache count: {e}")))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> DnsCache {
        DnsCache::open_in_memory().unwrap()
    }

    #[test]
    fn put_then_get_fresh() {
        let c = make();
        c.put(
            "example.com",
            &["93.184.216.34".to_string()],
            100,
            300,
        )
        .unwrap();
        let e = c.get("example.com", 200).unwrap().unwrap();
        assert_eq!(e.hostname, "example.com");
        assert_eq!(e.addresses, vec!["93.184.216.34".to_string()]);
        assert_eq!(e.cached_at, 100);
        assert_eq!(e.expires_at, 400);
    }

    #[test]
    fn get_expired_returns_none() {
        let c = make();
        c.put("example.com", &["1.2.3.4".to_string()], 100, 60).unwrap();
        // now = 200, expires_at = 160 → expired.
        assert!(c.get("example.com", 200).unwrap().is_none());
    }

    #[test]
    fn put_multiple_addresses() {
        let c = make();
        let addrs = vec![
            "1.1.1.1".to_string(),
            "1.0.0.1".to_string(),
            "2606:4700:4700::1111".to_string(),
        ];
        c.put("one.one.one.one", &addrs, 100, 300).unwrap();
        assert_eq!(c.get("one.one.one.one", 100).unwrap().unwrap().addresses, addrs);
    }

    #[test]
    fn put_overwrites_existing() {
        let c = make();
        c.put("x.com", &["1.1.1.1".to_string()], 100, 60).unwrap();
        c.put("x.com", &["2.2.2.2".to_string()], 200, 60).unwrap();
        let e = c.get("x.com", 250).unwrap().unwrap();
        assert_eq!(e.addresses, vec!["2.2.2.2".to_string()]);
        assert_eq!(e.cached_at, 200);
    }

    #[test]
    fn delete_removes_entry() {
        let c = make();
        c.put("x.com", &["1.1.1.1".to_string()], 100, 300).unwrap();
        c.delete("x.com").unwrap();
        assert!(c.get("x.com", 200).unwrap().is_none());
    }

    #[test]
    fn clear_expired_removes_only_past() {
        let c = make();
        c.put("old.com", &["1.1.1.1".to_string()], 100, 60).unwrap();
        c.put("new.com", &["2.2.2.2".to_string()], 100, 1000).unwrap();
        let removed = c.clear_expired(200).unwrap();
        assert_eq!(removed, 1);
        // new.com осталась.
        assert!(c.get("new.com", 200).unwrap().is_some());
    }

    #[test]
    fn idn_hostname_punycode_form() {
        let c = make();
        c.put("xn--e1afmkfd.xn--p1ai", &["217.16.16.20".to_string()], 100, 300).unwrap();
        let e = c.get("xn--e1afmkfd.xn--p1ai", 200).unwrap().unwrap();
        assert_eq!(e.hostname, "xn--e1afmkfd.xn--p1ai");
    }

    #[test]
    fn count_works() {
        let c = make();
        assert_eq!(c.count().unwrap(), 0);
        c.put("a", &["1.1.1.1".to_string()], 100, 300).unwrap();
        c.put("b", &["2.2.2.2".to_string()], 100, 300).unwrap();
        assert_eq!(c.count().unwrap(), 2);
    }

    #[test]
    fn empty_addresses_stored() {
        // NXDOMAIN-like negative cache.
        let c = make();
        c.put("nonexistent.tld", &[], 100, 60).unwrap();
        let e = c.get("nonexistent.tld", 150).unwrap().unwrap();
        assert!(e.addresses.is_empty());
    }

    #[test]
    fn clear_wipes_all() {
        let c = make();
        c.put("a", &["1.1.1.1".to_string()], 100, 300).unwrap();
        c.put("b", &["2.2.2.2".to_string()], 100, 300).unwrap();
        c.clear().unwrap();
        assert_eq!(c.count().unwrap(), 0);
    }

    #[test]
    fn is_fresh_boundary() {
        let e = DnsEntry {
            hostname: "x".into(),
            addresses: vec![],
            cached_at: 100,
            expires_at: 200,
        };
        assert!(e.is_fresh(199));
        assert!(!e.is_fresh(200));
        assert!(!e.is_fresh(201));
    }
}
