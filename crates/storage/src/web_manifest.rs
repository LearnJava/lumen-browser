//! Web App Manifest — PWA manifest persistence.
//!
//! Spec: <https://w3c.github.io/manifest/>. Manifest хранится per-origin
//! как JSON-string (whole document). Phase 0: storage; парсер JSON в
//! типизированную structure и реальный «Install» PWA UX — задачи shell.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebManifest {
    pub origin: String,
    /// Full manifest document как JSON-string.
    pub manifest_json: String,
    /// Path к manifest-файлу относительно origin (`/manifest.webmanifest`).
    pub manifest_url: String,
    /// Установлен ли как standalone-app пользователем.
    pub installed: bool,
    pub fetched_at: i64,
}

pub struct WebManifests {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for WebManifests {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebManifests").finish()
    }
}

impl WebManifests {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("web_manifest open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("web_manifest open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS web_manifests (
                origin        TEXT PRIMARY KEY,
                manifest_url  TEXT NOT NULL,
                manifest_json TEXT NOT NULL,
                installed     INTEGER NOT NULL DEFAULT 0,
                fetched_at    INTEGER NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS wm_installed_idx ON web_manifests(installed);
            "#,
        )
        .map_err(|e| Error::Storage(format!("web_manifest init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn store(
        &self,
        origin: &str,
        manifest_url: &str,
        manifest_json: &str,
        fetched_at: i64,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("web_manifest mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO web_manifests (origin, manifest_url, manifest_json, fetched_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (origin) DO UPDATE SET
                 manifest_url = excluded.manifest_url,
                 manifest_json = excluded.manifest_json,
                 fetched_at = excluded.fetched_at",
            params![origin, manifest_url, manifest_json, fetched_at],
        )
        .map_err(|e| Error::Storage(format!("web_manifest store: {e}")))?;
        Ok(())
    }

    pub fn set_installed(&self, origin: &str, installed: bool) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("web_manifest mutex poisoned".into()))?;
        conn.execute(
            "UPDATE web_manifests SET installed = ?1 WHERE origin = ?2",
            params![installed as i32, origin],
        )
        .map_err(|e| Error::Storage(format!("web_manifest set_installed: {e}")))?;
        Ok(())
    }

    pub fn get(&self, origin: &str) -> Result<Option<WebManifest>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("web_manifest mutex poisoned".into()))?;
        conn.query_row(
            "SELECT origin, manifest_url, manifest_json, installed, fetched_at
             FROM web_manifests WHERE origin = ?1",
            params![origin],
            |r| {
                Ok(WebManifest {
                    origin: r.get(0)?,
                    manifest_url: r.get(1)?,
                    manifest_json: r.get(2)?,
                    installed: r.get::<_, i32>(3)? != 0,
                    fetched_at: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| Error::Storage(format!("web_manifest get: {e}")))
    }

    /// Все установленные PWA (для UI «Installed apps»).
    pub fn list_installed(&self) -> Result<Vec<WebManifest>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("web_manifest mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT origin, manifest_url, manifest_json, installed, fetched_at
                 FROM web_manifests WHERE installed = 1 ORDER BY fetched_at DESC",
            )
            .map_err(|e| Error::Storage(format!("web_manifest list prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(WebManifest {
                    origin: r.get(0)?,
                    manifest_url: r.get(1)?,
                    manifest_json: r.get(2)?,
                    installed: r.get::<_, i32>(3)? != 0,
                    fetched_at: r.get(4)?,
                })
            })
            .map_err(|e| Error::Storage(format!("web_manifest list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("web_manifest row: {e}")))?);
        }
        Ok(out)
    }

    pub fn delete(&self, origin: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("web_manifest mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM web_manifests WHERE origin = ?1",
            params![origin],
        )
        .map_err(|e| Error::Storage(format!("web_manifest delete: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("web_manifest mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM web_manifests", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("web_manifest count: {e}")))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> WebManifests {
        WebManifests::open_in_memory().unwrap()
    }

    #[test]
    fn store_and_get() {
        let m = make();
        m.store(
            "https://app.example.com",
            "/manifest.webmanifest",
            r#"{"name":"Example","start_url":"/"}"#,
            100,
        )
        .unwrap();
        let got = m.get("https://app.example.com").unwrap().unwrap();
        assert_eq!(got.manifest_url, "/manifest.webmanifest");
        assert!(got.manifest_json.contains("Example"));
        assert!(!got.installed);
        assert_eq!(got.fetched_at, 100);
    }

    #[test]
    fn store_overwrites() {
        let m = make();
        m.store("https://x/", "/m.json", r#"{"v":1}"#, 100).unwrap();
        m.store("https://x/", "/m.json", r#"{"v":2}"#, 200).unwrap();
        let got = m.get("https://x/").unwrap().unwrap();
        assert!(got.manifest_json.contains("\"v\":2"));
        assert_eq!(got.fetched_at, 200);
    }

    #[test]
    fn set_installed_persists() {
        let m = make();
        m.store("https://x/", "/m", "{}", 100).unwrap();
        m.set_installed("https://x/", true).unwrap();
        assert!(m.get("https://x/").unwrap().unwrap().installed);
        m.set_installed("https://x/", false).unwrap();
        assert!(!m.get("https://x/").unwrap().unwrap().installed);
    }

    #[test]
    fn list_installed_filters() {
        let m = make();
        m.store("https://a/", "/m", "{}", 100).unwrap();
        m.store("https://b/", "/m", "{}", 200).unwrap();
        m.store("https://c/", "/m", "{}", 300).unwrap();
        m.set_installed("https://a/", true).unwrap();
        m.set_installed("https://c/", true).unwrap();
        let installed = m.list_installed().unwrap();
        assert_eq!(installed.len(), 2);
        // DESC by fetched_at: c, a.
        assert_eq!(installed[0].origin, "https://c/");
    }

    #[test]
    fn delete_removes() {
        let m = make();
        m.store("https://x/", "/m", "{}", 100).unwrap();
        m.delete("https://x/").unwrap();
        assert!(m.get("https://x/").unwrap().is_none());
    }

    #[test]
    fn cyrillic_manifest_content() {
        let m = make();
        m.store("https://пример.рф/", "/m", r#"{"name":"Пример"}"#, 100).unwrap();
        assert!(m.get("https://пример.рф/").unwrap().unwrap().manifest_json.contains("Пример"));
    }

    #[test]
    fn count_works() {
        let m = make();
        assert_eq!(m.count().unwrap(), 0);
        m.store("https://a/", "/m", "{}", 100).unwrap();
        m.store("https://b/", "/m", "{}", 200).unwrap();
        assert_eq!(m.count().unwrap(), 2);
    }
}
