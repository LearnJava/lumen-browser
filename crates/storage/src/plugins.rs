//! Plugins manifest — registry установленных WASM-плагинов (§11 плана).
//!
//! Каждый плагин имеет:
//! - id, name (UNIQUE), version;
//! - source — путь к WASM-файлу или URL источника;
//! - capabilities — JSON-список запрошенных capability-токенов
//!   (§11.4): `network`, `clipboard`, `storage:<key>`, etc.;
//! - enabled — флаг включения;
//! - installed_at / last_used_at — для UI.
//!
//! Phase 0: storage layer. Capability tokens — пока строки в JSON,
//! типизированный CapabilityToken-enum в `lumen-core::ext` уже описан
//! как future trait — здесь храним сырые имена. Runtime-проверка
//! capability при вызовах плагина — задача `lumen-shell` + wasmtime
//! sandbox.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub source: String,
    /// JSON-строка со списком capability-токенов.
    /// Пример: `["network", "clipboard", "storage:knowledge"]`.
    pub capabilities_json: String,
    pub enabled: bool,
    pub installed_at: i64,
    pub last_used_at: Option<i64>,
}

pub struct Plugins {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Plugins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugins").finish()
    }
}

impl Plugins {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("plugins open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("plugins open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS plugins (
                id                INTEGER PRIMARY KEY,
                name              TEXT NOT NULL UNIQUE,
                version           TEXT NOT NULL DEFAULT '0.0.0',
                source            TEXT NOT NULL,
                capabilities_json TEXT NOT NULL DEFAULT '[]',
                enabled           INTEGER NOT NULL DEFAULT 1,
                installed_at      INTEGER NOT NULL,
                last_used_at      INTEGER
            );
            CREATE INDEX IF NOT EXISTS plugins_enabled_idx ON plugins(enabled);
            "#,
        )
        .map_err(|e| Error::Storage(format!("plugins init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Установить плагин. Если name уже есть — Error (UNIQUE constraint).
    pub fn install(
        &self,
        name: &str,
        version: &str,
        source: &str,
        capabilities_json: &str,
        installed_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO plugins (name, version, source, capabilities_json, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, version, source, capabilities_json, installed_at],
        )
        .map_err(|e| Error::Storage(format!("plugins install: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Обновить версию + capabilities (например, после re-install с новой
    /// версией плагина). source может обновиться тоже.
    pub fn update_manifest(
        &self,
        id: i64,
        version: &str,
        source: &str,
        capabilities_json: &str,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        conn.execute(
            "UPDATE plugins SET version = ?1, source = ?2, capabilities_json = ?3
             WHERE id = ?4",
            params![version, source, capabilities_json, id],
        )
        .map_err(|e| Error::Storage(format!("plugins update_manifest: {e}")))?;
        Ok(())
    }

    pub fn set_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        conn.execute(
            "UPDATE plugins SET enabled = ?1 WHERE id = ?2",
            params![enabled as i32, id],
        )
        .map_err(|e| Error::Storage(format!("plugins set_enabled: {e}")))?;
        Ok(())
    }

    /// Обновить last_used_at (вызывается при каждом invocation плагина).
    pub fn touch(&self, id: i64, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        conn.execute(
            "UPDATE plugins SET last_used_at = ?1 WHERE id = ?2",
            params![now_unix, id],
        )
        .map_err(|e| Error::Storage(format!("plugins touch: {e}")))?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<PluginManifest>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, name, version, source, capabilities_json, enabled,
                    installed_at, last_used_at
             FROM plugins WHERE id = ?1",
            params![id],
            row_to_manifest,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("plugins get: {e}")))
    }

    pub fn get_by_name(&self, name: &str) -> Result<Option<PluginManifest>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, name, version, source, capabilities_json, enabled,
                    installed_at, last_used_at
             FROM plugins WHERE name = ?1",
            params![name],
            row_to_manifest,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("plugins get_by_name: {e}")))
    }

    /// Все установленные плагины (включая disabled). ORDER BY installed_at ASC.
    pub fn list_all(&self) -> Result<Vec<PluginManifest>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, name, version, source, capabilities_json, enabled,
                        installed_at, last_used_at
                 FROM plugins ORDER BY installed_at ASC",
            )
            .map_err(|e| Error::Storage(format!("plugins list_all prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_manifest)
            .map_err(|e| Error::Storage(format!("plugins list_all query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("plugins row: {e}")))?);
        }
        Ok(out)
    }

    /// Только enabled-плагины — для runtime-loading.
    pub fn list_enabled(&self) -> Result<Vec<PluginManifest>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, name, version, source, capabilities_json, enabled,
                        installed_at, last_used_at
                 FROM plugins WHERE enabled = 1 ORDER BY installed_at ASC",
            )
            .map_err(|e| Error::Storage(format!("plugins list_enabled prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_manifest)
            .map_err(|e| Error::Storage(format!("plugins list_enabled query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("plugins row: {e}")))?);
        }
        Ok(out)
    }

    pub fn uninstall(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        conn.execute("DELETE FROM plugins WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("plugins uninstall: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("plugins mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM plugins", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("plugins count: {e}")))?;
        Ok(n)
    }
}

fn row_to_manifest(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginManifest> {
    Ok(PluginManifest {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        source: row.get(3)?,
        capabilities_json: row.get(4)?,
        enabled: row.get::<_, i32>(5)? != 0,
        installed_at: row.get(6)?,
        last_used_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Plugins {
        Plugins::open_in_memory().unwrap()
    }

    #[test]
    fn install_and_get() {
        let p = make();
        let id = p
            .install("ad-blocker", "1.0.0", "/plugins/ad.wasm", r#"["network"]"#, 100)
            .unwrap();
        let m = p.get(id).unwrap().unwrap();
        assert_eq!(m.name, "ad-blocker");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.source, "/plugins/ad.wasm");
        assert_eq!(m.capabilities_json, r#"["network"]"#);
        assert!(m.enabled);  // default
        assert_eq!(m.installed_at, 100);
        assert_eq!(m.last_used_at, None);
    }

    #[test]
    fn install_duplicate_name_fails() {
        let p = make();
        p.install("x", "1.0.0", "/x.wasm", "[]", 100).unwrap();
        assert!(p.install("x", "2.0.0", "/y.wasm", "[]", 200).is_err());
    }

    #[test]
    fn update_manifest_works() {
        let p = make();
        let id = p.install("x", "1.0.0", "/x.wasm", "[]", 100).unwrap();
        p.update_manifest(id, "2.0.0", "/x-v2.wasm", r#"["network","clipboard"]"#)
            .unwrap();
        let m = p.get(id).unwrap().unwrap();
        assert_eq!(m.version, "2.0.0");
        assert_eq!(m.source, "/x-v2.wasm");
        assert_eq!(m.capabilities_json, r#"["network","clipboard"]"#);
        // installed_at сохраняется.
        assert_eq!(m.installed_at, 100);
    }

    #[test]
    fn set_enabled_toggles() {
        let p = make();
        let id = p.install("x", "1.0.0", "/x.wasm", "[]", 100).unwrap();
        assert!(p.get(id).unwrap().unwrap().enabled);
        p.set_enabled(id, false).unwrap();
        assert!(!p.get(id).unwrap().unwrap().enabled);
        p.set_enabled(id, true).unwrap();
        assert!(p.get(id).unwrap().unwrap().enabled);
    }

    #[test]
    fn touch_updates_last_used() {
        let p = make();
        let id = p.install("x", "1.0.0", "/x.wasm", "[]", 100).unwrap();
        p.touch(id, 500).unwrap();
        assert_eq!(p.get(id).unwrap().unwrap().last_used_at, Some(500));
    }

    #[test]
    fn list_all_includes_disabled() {
        let p = make();
        let id1 = p.install("a", "1", "/a.wasm", "[]", 100).unwrap();
        p.install("b", "1", "/b.wasm", "[]", 200).unwrap();
        p.set_enabled(id1, false).unwrap();
        let all = p.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn list_enabled_filters() {
        let p = make();
        let id1 = p.install("a", "1", "/a.wasm", "[]", 100).unwrap();
        p.install("b", "1", "/b.wasm", "[]", 200).unwrap();
        p.set_enabled(id1, false).unwrap();
        let enabled = p.list_enabled().unwrap();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "b");
    }

    #[test]
    fn list_all_ordered_by_installed_at() {
        let p = make();
        p.install("new", "1", "/n.wasm", "[]", 300).unwrap();
        p.install("old", "1", "/o.wasm", "[]", 100).unwrap();
        p.install("mid", "1", "/m.wasm", "[]", 200).unwrap();
        let all = p.list_all().unwrap();
        let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["old", "mid", "new"]);
    }

    #[test]
    fn uninstall_removes_plugin() {
        let p = make();
        let id = p.install("x", "1", "/x.wasm", "[]", 100).unwrap();
        p.uninstall(id).unwrap();
        assert!(p.get(id).unwrap().is_none());
    }

    #[test]
    fn get_by_name_works() {
        let p = make();
        p.install("rust-helper", "1", "/r.wasm", "[]", 100).unwrap();
        let m = p.get_by_name("rust-helper").unwrap().unwrap();
        assert_eq!(m.name, "rust-helper");
    }

    #[test]
    fn count_works() {
        let p = make();
        assert_eq!(p.count().unwrap(), 0);
        p.install("a", "1", "/a.wasm", "[]", 100).unwrap();
        p.install("b", "1", "/b.wasm", "[]", 200).unwrap();
        assert_eq!(p.count().unwrap(), 2);
    }

    #[test]
    fn cyrillic_plugin_name() {
        let p = make();
        let id = p
            .install("блокировщик-рекламы", "1.0.0", "/x.wasm", r#"["network"]"#, 100)
            .unwrap();
        let m = p.get(id).unwrap().unwrap();
        assert_eq!(m.name, "блокировщик-рекламы");
    }

    #[test]
    fn capabilities_json_round_trip() {
        let p = make();
        let caps = r#"["network","clipboard","storage:knowledge","local-ai"]"#;
        let id = p.install("x", "1", "/x.wasm", caps, 100).unwrap();
        assert_eq!(p.get(id).unwrap().unwrap().capabilities_json, caps);
    }

    #[test]
    fn get_missing_returns_none() {
        let p = make();
        assert!(p.get(999).unwrap().is_none());
        assert!(p.get_by_name("nope").unwrap().is_none());
    }
}
