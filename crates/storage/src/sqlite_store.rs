//! Persistent KV-хранилище поверх SQLite (exception #5 в §5 политики
//! зависимостей).
//!
//! Реализует `lumen_core::ext::StorageBackend` тем же контрактом, что
//! `InMemoryStorage`, но кладёт данные на диск (или в `:memory:` базу
//! для тестов). Одна таблица `kv` с составным первичным ключом
//! `(origin, top_level_site, key)` — origin-партиционирование такое же,
//! как у in-memory варианта; `None` параметры маппятся в пустую строку.
//!
//! WAL + synchronous=NORMAL — стандартный компромисс для долгоживущего
//! single-writer storage: durable до crash-а, но без `fsync` на каждый
//! commit. Для cookies / history достаточно; для критичных данных
//! (например, password DB в будущем) — `synchronous=FULL` отдельно.
//!
//! Connection не-Sync (rusqlite ограничение), поэтому держим `Mutex<Connection>`
//! внутри — это даёт `Send + Sync` для самой `SqliteStorage`. Локов в
//! hot-path немного: каждый `get` / `put` берёт mutex на время одной
//! SQL-команды.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::ext::StorageBackend;
use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Persistent KV-хранилище на SQLite. Создаёт таблицу `kv` при инициализации
/// (idempotent через `IF NOT EXISTS`); WAL-режим + synchronous=NORMAL.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SqliteStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStorage").finish()
    }
}

impl SqliteStorage {
    /// Открыть БД по пути (файл создаётся при отсутствии).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("sqlite open: {e}")))?;
        Self::init(conn)
    }

    /// Открыть in-memory БД (для тестов и ephemeral session-state).
    /// Каждый вызов создаёт новый изолированный экземпляр.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("sqlite open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // PRAGMA-ы выставляем до создания таблиц. WAL — постоянное свойство
        // БД-файла (включается один раз), synchronous=NORMAL — per-connection.
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS kv (
                origin         TEXT NOT NULL DEFAULT '',
                top_level_site TEXT NOT NULL DEFAULT '',
                key            TEXT NOT NULL,
                value          BLOB NOT NULL,
                PRIMARY KEY (origin, top_level_site, key)
            ) WITHOUT ROWID;
            "#,
        )
        .map_err(|e| Error::Storage(format!("sqlite init: {e}")))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

fn part(s: Option<&str>) -> &str {
    s.unwrap_or("")
}

impl StorageBackend for SqliteStorage {
    fn get(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<Option<Vec<u8>>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("sqlite mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT value FROM kv WHERE origin = ?1 AND top_level_site = ?2 AND key = ?3",
            )
            .map_err(|e| Error::Storage(format!("sqlite prepare get: {e}")))?;
        let value: Option<Vec<u8>> = stmt
            .query_row(params![part(origin), part(top_level_site), key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| Error::Storage(format!("sqlite query get: {e}")))?;
        Ok(value)
    }

    fn put(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
        value: &[u8],
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("sqlite mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO kv (origin, top_level_site, key, value) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (origin, top_level_site, key) DO UPDATE SET value = excluded.value",
            params![part(origin), part(top_level_site), key, value],
        )
        .map_err(|e| Error::Storage(format!("sqlite put: {e}")))?;
        Ok(())
    }

    fn delete(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("sqlite mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM kv WHERE origin = ?1 AND top_level_site = ?2 AND key = ?3",
            params![part(origin), part(top_level_site), key],
        )
        .map_err(|e| Error::Storage(format!("sqlite delete: {e}")))?;
        Ok(())
    }

    fn list_keys(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
    ) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("sqlite mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT key FROM kv WHERE origin = ?1 AND top_level_site = ?2 ORDER BY key",
            )
            .map_err(|e| Error::Storage(format!("sqlite prepare list_keys: {e}")))?;
        let rows = stmt
            .query_map(params![part(origin), part(top_level_site)], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| Error::Storage(format!("sqlite query list_keys: {e}")))?;
        let mut keys = Vec::new();
        for r in rows {
            keys.push(r.map_err(|e| Error::Storage(format!("sqlite row list_keys: {e}")))?);
        }
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> SqliteStorage {
        SqliteStorage::open_in_memory().unwrap()
    }

    #[test]
    fn put_then_get_roundtrip() {
        let mut s = make();
        s.put(None, None, "foo", b"bar").unwrap();
        assert_eq!(s.get(None, None, "foo").unwrap(), Some(b"bar".to_vec()));
    }

    #[test]
    fn get_missing_returns_none() {
        let s = make();
        assert_eq!(s.get(None, None, "absent").unwrap(), None);
    }

    #[test]
    fn put_overwrites_existing() {
        let mut s = make();
        s.put(None, None, "k", b"v1").unwrap();
        s.put(None, None, "k", b"v2").unwrap();
        assert_eq!(s.get(None, None, "k").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn delete_removes_entry() {
        let mut s = make();
        s.put(None, None, "k", b"v").unwrap();
        s.delete(None, None, "k").unwrap();
        assert_eq!(s.get(None, None, "k").unwrap(), None);
    }

    #[test]
    fn delete_missing_is_noop() {
        let mut s = make();
        s.delete(None, None, "absent").unwrap();
    }

    #[test]
    fn origin_partitioning_isolates_data() {
        // Один key, разные origin → разные значения.
        let mut s = make();
        s.put(Some("https://a.com"), None, "tok", b"alpha").unwrap();
        s.put(Some("https://b.com"), None, "tok", b"beta").unwrap();
        assert_eq!(
            s.get(Some("https://a.com"), None, "tok").unwrap(),
            Some(b"alpha".to_vec())
        );
        assert_eq!(
            s.get(Some("https://b.com"), None, "tok").unwrap(),
            Some(b"beta".to_vec())
        );
    }

    #[test]
    fn top_level_site_partitioning_isolates_data() {
        // Total cookie protection: один третьесторонний origin под разными
        // top-level контекстами хранит разные данные.
        let mut s = make();
        s.put(Some("https://ads.com"), Some("https://news.com"), "id", b"news_id")
            .unwrap();
        s.put(Some("https://ads.com"), Some("https://blog.com"), "id", b"blog_id")
            .unwrap();
        assert_eq!(
            s.get(Some("https://ads.com"), Some("https://news.com"), "id")
                .unwrap(),
            Some(b"news_id".to_vec())
        );
        assert_eq!(
            s.get(Some("https://ads.com"), Some("https://blog.com"), "id")
                .unwrap(),
            Some(b"blog_id".to_vec())
        );
    }

    #[test]
    fn none_and_empty_origin_are_same_namespace() {
        // None и Some("") — один namespace по контракту трейта.
        let mut s = make();
        s.put(None, None, "k", b"v").unwrap();
        assert_eq!(s.get(Some(""), Some(""), "k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn list_keys_partitioned() {
        let mut s = make();
        s.put(None, None, "a", b"1").unwrap();
        s.put(None, None, "b", b"2").unwrap();
        s.put(Some("https://x.com"), None, "c", b"3").unwrap();

        let keys_global = s.list_keys(None, None).unwrap();
        assert_eq!(keys_global, vec!["a".to_string(), "b".to_string()]);

        let keys_x = s.list_keys(Some("https://x.com"), None).unwrap();
        assert_eq!(keys_x, vec!["c".to_string()]);
    }

    #[test]
    fn list_keys_empty_returns_empty() {
        let s = make();
        assert!(s.list_keys(None, None).unwrap().is_empty());
    }

    #[test]
    fn binary_values_preserved() {
        // SQLite BLOB должен хранить любые байты без потерь, включая 0x00.
        let mut s = make();
        let blob: Vec<u8> = (0..=255u8).collect();
        s.put(None, None, "bin", &blob).unwrap();
        assert_eq!(s.get(None, None, "bin").unwrap(), Some(blob));
    }

    #[test]
    fn cyrillic_keys_and_values() {
        let mut s = make();
        s.put(None, None, "ключ", "значение".as_bytes()).unwrap();
        assert_eq!(
            s.get(None, None, "ключ").unwrap(),
            Some("значение".as_bytes().to_vec())
        );
    }

    #[test]
    fn persists_across_open() {
        // Открыть → записать → закрыть → переоткрыть → данные сохранились.
        let tmpdir = std::env::temp_dir().join(format!(
            "lumen-sqlite-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmpdir);

        {
            let mut s = SqliteStorage::open(&tmpdir).unwrap();
            s.put(None, None, "persistent", b"yes").unwrap();
        }
        {
            let s = SqliteStorage::open(&tmpdir).unwrap();
            assert_eq!(
                s.get(None, None, "persistent").unwrap(),
                Some(b"yes".to_vec())
            );
        }

        let _ = std::fs::remove_file(&tmpdir);
        // WAL/shm файлы — clean-up на случай leak.
        let _ = std::fs::remove_file(format!("{}-wal", tmpdir.display()));
        let _ = std::fs::remove_file(format!("{}-shm", tmpdir.display()));
    }
}
