//! Service Worker registrations — persistent state SW per (origin, scope).
//!
//! Spec: <https://w3c.github.io/ServiceWorker/>. Каждая регистрация:
//! - origin (для security boundary);
//! - scope (URL prefix, к которому применяется SW; e.g. `/app/`);
//! - script_url — JavaScript-файл воркера;
//! - update_via_cache — `imports`/`all`/`none`;
//! - registered_at + last_active.
//!
//! Phase 0: storage layer. Реальный SW runtime (lifecycle: install /
//! activate / fetch event), интеграция с lumen-network — отдельные
//! задачи (Phase 3+).

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UpdateViaCache {
    /// `imports` (default) — SW-script всегда из network, imports могут из cache.
    #[default]
    Imports,
    /// `all` — оба из cache.
    All,
    /// `none` — оба из network (обходит HTTP cache).
    None,
}

impl UpdateViaCache {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imports => "imports",
            Self::All => "all",
            Self::None => "none",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "imports" => Some(Self::Imports),
            "all" => Some(Self::All),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceWorkerRegistration {
    pub id: i64,
    pub origin: String,
    pub scope: String,
    pub script_url: String,
    pub update_via_cache: UpdateViaCache,
    pub registered_at: i64,
    pub last_active: Option<i64>,
}

pub struct ServiceWorkers {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for ServiceWorkers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceWorkers").finish()
    }
}

impl ServiceWorkers {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("service_workers open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("service_workers open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS service_workers (
                id               INTEGER PRIMARY KEY,
                origin           TEXT NOT NULL,
                scope            TEXT NOT NULL,
                script_url       TEXT NOT NULL,
                update_via_cache TEXT NOT NULL DEFAULT 'imports',
                registered_at    INTEGER NOT NULL,
                last_active      INTEGER,
                UNIQUE (origin, scope)
            );
            CREATE INDEX IF NOT EXISTS sw_origin_idx ON service_workers(origin);
            "#,
        )
        .map_err(|e| Error::Storage(format!("service_workers init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn register(
        &self,
        origin: &str,
        scope: &str,
        script_url: &str,
        update_via_cache: UpdateViaCache,
        registered_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO service_workers (origin, scope, script_url, update_via_cache, registered_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (origin, scope) DO UPDATE SET
                 script_url = excluded.script_url,
                 update_via_cache = excluded.update_via_cache,
                 registered_at = excluded.registered_at",
            params![origin, scope, script_url, update_via_cache.as_str(), registered_at],
        )
        .map_err(|e| Error::Storage(format!("service_workers register: {e}")))?;
        let id: i64 = conn
            .query_row(
                "SELECT id FROM service_workers WHERE origin = ?1 AND scope = ?2",
                params![origin, scope],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("service_workers register-lookup: {e}")))?;
        Ok(id)
    }

    pub fn touch(&self, id: i64, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        conn.execute(
            "UPDATE service_workers SET last_active = ?1 WHERE id = ?2",
            params![now_unix, id],
        )
        .map_err(|e| Error::Storage(format!("service_workers touch: {e}")))?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<ServiceWorkerRegistration>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, origin, scope, script_url, update_via_cache, registered_at, last_active
             FROM service_workers WHERE id = ?1",
            params![id],
            row_to_reg,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("service_workers get: {e}")))
    }

    /// Найти SW для конкретного URL: scope с самым длинным prefix-match.
    /// Соответствует SW algorithm для service-worker selection.
    pub fn find_for_url(&self, origin: &str, url: &str) -> Result<Option<ServiceWorkerRegistration>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, origin, scope, script_url, update_via_cache, registered_at, last_active
                 FROM service_workers WHERE origin = ?1
                 ORDER BY length(scope) DESC",
            )
            .map_err(|e| Error::Storage(format!("service_workers find prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin], row_to_reg)
            .map_err(|e| Error::Storage(format!("service_workers find query: {e}")))?;
        for r in rows {
            let reg = r.map_err(|e| Error::Storage(format!("service_workers row: {e}")))?;
            if url.starts_with(&reg.scope) {
                return Ok(Some(reg));
            }
        }
        Ok(None)
    }

    pub fn list_for_origin(&self, origin: &str) -> Result<Vec<ServiceWorkerRegistration>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, origin, scope, script_url, update_via_cache, registered_at, last_active
                 FROM service_workers WHERE origin = ?1 ORDER BY scope ASC",
            )
            .map_err(|e| Error::Storage(format!("service_workers list prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin], row_to_reg)
            .map_err(|e| Error::Storage(format!("service_workers list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("service_workers row: {e}")))?);
        }
        Ok(out)
    }

    pub fn unregister(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM service_workers WHERE id = ?1",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("service_workers unregister: {e}")))?;
        Ok(())
    }

    pub fn unregister_origin(&self, origin: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM service_workers WHERE origin = ?1",
                params![origin],
            )
            .map_err(|e| Error::Storage(format!("service_workers unregister_origin: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("service_workers mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM service_workers", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("service_workers count: {e}")))?;
        Ok(n)
    }
}

fn row_to_reg(row: &rusqlite::Row<'_>) -> rusqlite::Result<ServiceWorkerRegistration> {
    let uvc: String = row.get(4)?;
    Ok(ServiceWorkerRegistration {
        id: row.get(0)?,
        origin: row.get(1)?,
        scope: row.get(2)?,
        script_url: row.get(3)?,
        update_via_cache: UpdateViaCache::parse(&uvc).unwrap_or_default(),
        registered_at: row.get(5)?,
        last_active: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> ServiceWorkers {
        ServiceWorkers::open_in_memory().unwrap()
    }

    #[test]
    fn register_and_get() {
        let s = make();
        let id = s
            .register(
                "https://app.com",
                "/",
                "/sw.js",
                UpdateViaCache::Imports,
                100,
            )
            .unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.origin, "https://app.com");
        assert_eq!(r.scope, "/");
        assert_eq!(r.script_url, "/sw.js");
        assert_eq!(r.update_via_cache, UpdateViaCache::Imports);
        assert_eq!(r.last_active, None);
    }

    #[test]
    fn register_same_scope_updates() {
        let s = make();
        let id1 = s.register("https://x/", "/", "/sw-v1.js", UpdateViaCache::Imports, 100).unwrap();
        let id2 = s.register("https://x/", "/", "/sw-v2.js", UpdateViaCache::None, 200).unwrap();
        assert_eq!(id1, id2);
        let r = s.get(id1).unwrap().unwrap();
        assert_eq!(r.script_url, "/sw-v2.js");
        assert_eq!(r.update_via_cache, UpdateViaCache::None);
    }

    #[test]
    fn touch_sets_last_active() {
        let s = make();
        let id = s.register("https://x/", "/", "/sw.js", UpdateViaCache::Imports, 100).unwrap();
        s.touch(id, 500).unwrap();
        assert_eq!(s.get(id).unwrap().unwrap().last_active, Some(500));
    }

    #[test]
    fn find_for_url_longest_prefix_match() {
        let s = make();
        s.register("https://x/", "/", "/sw-root.js", UpdateViaCache::Imports, 100).unwrap();
        s.register("https://x/", "/app/", "/sw-app.js", UpdateViaCache::Imports, 200).unwrap();
        s.register("https://x/", "/app/admin/", "/sw-admin.js", UpdateViaCache::Imports, 300).unwrap();
        // /app/admin/profile → longest match `/app/admin/` → sw-admin.
        let r = s.find_for_url("https://x/", "/app/admin/profile").unwrap().unwrap();
        assert_eq!(r.script_url, "/sw-admin.js");
        // /app/feed → longest = `/app/` → sw-app.
        let r2 = s.find_for_url("https://x/", "/app/feed").unwrap().unwrap();
        assert_eq!(r2.script_url, "/sw-app.js");
        // /other → root.
        let r3 = s.find_for_url("https://x/", "/other").unwrap().unwrap();
        assert_eq!(r3.script_url, "/sw-root.js");
    }

    #[test]
    fn find_for_url_none_when_no_match() {
        let s = make();
        s.register("https://x/", "/app/", "/sw.js", UpdateViaCache::Imports, 100).unwrap();
        let r = s.find_for_url("https://x/", "/other").unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn list_for_origin_returns_all_scopes() {
        let s = make();
        s.register("https://x/", "/", "/r.js", UpdateViaCache::Imports, 100).unwrap();
        s.register("https://x/", "/a/", "/a.js", UpdateViaCache::Imports, 200).unwrap();
        s.register("https://y/", "/", "/y.js", UpdateViaCache::Imports, 300).unwrap();
        let list = s.list_for_origin("https://x/").unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn unregister_works() {
        let s = make();
        let id = s.register("https://x/", "/", "/sw.js", UpdateViaCache::Imports, 100).unwrap();
        s.unregister(id).unwrap();
        assert!(s.get(id).unwrap().is_none());
    }

    #[test]
    fn unregister_origin_removes_all_scopes() {
        let s = make();
        s.register("https://x/", "/", "/sw.js", UpdateViaCache::Imports, 100).unwrap();
        s.register("https://x/", "/a/", "/a.js", UpdateViaCache::Imports, 200).unwrap();
        s.register("https://y/", "/", "/y.js", UpdateViaCache::Imports, 300).unwrap();
        let removed = s.unregister_origin("https://x/").unwrap();
        assert_eq!(removed, 2);
        assert_eq!(s.count().unwrap(), 1);
    }

    #[test]
    fn update_via_cache_round_trip() {
        for v in [UpdateViaCache::Imports, UpdateViaCache::All, UpdateViaCache::None] {
            assert_eq!(UpdateViaCache::parse(v.as_str()), Some(v));
        }
    }

    #[test]
    fn count_works() {
        let s = make();
        assert_eq!(s.count().unwrap(), 0);
        s.register("https://a/", "/", "/sw.js", UpdateViaCache::Imports, 100).unwrap();
        s.register("https://b/", "/", "/sw.js", UpdateViaCache::Imports, 200).unwrap();
        assert_eq!(s.count().unwrap(), 2);
    }
}
