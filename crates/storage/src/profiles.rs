//! §9.3 Профили — структура и storage. Профиль — изолированный
//! набор пользовательских данных (история, закладки, заметки, cookies,
//! настройки). Один профиль = один отдельный SQLite-файл с собственной
//! cookie jar / history / bookmarks / etc. Этот модуль хранит metadata
//! профилей (имя, дата создания, путь к storage-файлу, default flag)
//! в **отдельной БД** (profiles.db) — она же выбирает, какой профиль
//! сейчас активен.
//!
//! Phase 0 покрывает: CRUD профилей, переключение active, имена
//! настроек (settings JSON-blob — текстовое поле без типизации; UI
//! десериализует по контексту).
//!
//! Phase 1 (PH2-3) добавляет: per-profile AES-256-GCM ключ шифрования
//! (`set_password` / `unlock` / `clear_password`). Ключ хранится в поле
//! `encrypted_key_blob` — формат описан в [`crate::profile_vault`].
//!
//! Отложено: capability tokens (§11.4) для plugin-permission boundary,
//! миграции схемы, telemetry-counters per profile.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::profile_vault;

/// Один профиль пользователя.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Уникальный идентификатор профиля.
    pub id: i64,
    /// Отображаемое имя (уникальное в рамках БД).
    pub name: String,
    /// Путь к storage-каталогу профиля (где живут history.db, cookies.db,
    /// и т.д.). Каждый профиль — отдельная папка.
    pub storage_path: String,
    /// UNIX timestamp создания профиля.
    pub created_at: i64,
    /// JSON-строка с настройками (UI десериализует по своему типу).
    /// Пустая строка `""` — значения по умолчанию.
    pub settings_json: String,
    /// `true` если этот профиль является активным (текущим).
    pub is_default: bool,
    /// `true` если профиль защищён паролем (encrypted_key_blob присутствует).
    pub is_encrypted: bool,
}

pub struct ProfileRegistry {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for ProfileRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileRegistry").finish()
    }
}

impl ProfileRegistry {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("profiles open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("profiles open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS profiles (
                id                 INTEGER PRIMARY KEY,
                name               TEXT NOT NULL UNIQUE,
                storage_path       TEXT NOT NULL,
                created_at         INTEGER NOT NULL,
                settings_json      TEXT NOT NULL DEFAULT '',
                encrypted_key_blob BLOB DEFAULT NULL
            );
            CREATE TABLE IF NOT EXISTS active_profile (
                lock INTEGER PRIMARY KEY CHECK (lock = 0),  -- singleton
                profile_id INTEGER,
                FOREIGN KEY (profile_id) REFERENCES profiles(id)
                    ON DELETE SET NULL
            );
            -- Singleton-строка active_profile (только id=0). Если её нет — null active.
            INSERT OR IGNORE INTO active_profile (lock, profile_id) VALUES (0, NULL);
            "#,
        )
        .map_err(|e| Error::Storage(format!("profiles init: {e}")))?;

        // Schema migration: add encrypted_key_blob if the table was created
        // by an older version that didn't have this column. ALTER TABLE ADD COLUMN
        // is idempotent-ish — we catch the "duplicate column" error and ignore it.
        let _ = conn.execute_batch(
            "ALTER TABLE profiles ADD COLUMN encrypted_key_blob BLOB DEFAULT NULL;",
        );

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Создать новый профиль. Имя должно быть уникальным.
    /// Возвращает id.
    pub fn create(
        &self,
        name: &str,
        storage_path: &str,
        settings_json: &str,
        created_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO profiles (name, storage_path, settings_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![name, storage_path, settings_json, created_at],
        )
        .map_err(|e| Error::Storage(format!("profiles create: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Получить профиль по id.
    pub fn get(&self, id: i64) -> Result<Option<Profile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let active_id = active_id_locked(&conn)?;
        let p = conn
            .query_row(
                "SELECT id, name, storage_path, created_at, settings_json, encrypted_key_blob
                 FROM profiles WHERE id = ?1",
                params![id],
                row_to_profile_partial,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("profiles get: {e}")))?;
        Ok(p.map(|mut p| {
            p.is_default = Some(p.id) == active_id;
            p
        }))
    }

    /// Получить профиль по имени.
    pub fn get_by_name(&self, name: &str) -> Result<Option<Profile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let active_id = active_id_locked(&conn)?;
        let p = conn
            .query_row(
                "SELECT id, name, storage_path, created_at, settings_json, encrypted_key_blob
                 FROM profiles WHERE name = ?1",
                params![name],
                row_to_profile_partial,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("profiles get_by_name: {e}")))?;
        Ok(p.map(|mut p| {
            p.is_default = Some(p.id) == active_id;
            p
        }))
    }

    /// Все профили. Сортировка по created_at ASC (порядок создания).
    pub fn list_all(&self) -> Result<Vec<Profile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let active_id = active_id_locked(&conn)?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, name, storage_path, created_at, settings_json, encrypted_key_blob
                 FROM profiles ORDER BY created_at ASC",
            )
            .map_err(|e| Error::Storage(format!("profiles list prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_profile_partial)
            .map_err(|e| Error::Storage(format!("profiles list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            let mut p = r.map_err(|e| Error::Storage(format!("profiles row: {e}")))?;
            p.is_default = Some(p.id) == active_id;
            out.push(p);
        }
        Ok(out)
    }

    /// Переименовать. Имя уникально — конфликт → Error.
    pub fn rename(&self, id: i64, new_name: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        conn.execute(
            "UPDATE profiles SET name = ?1 WHERE id = ?2",
            params![new_name, id],
        )
        .map_err(|e| Error::Storage(format!("profiles rename: {e}")))?;
        Ok(())
    }

    /// Обновить settings_json.
    pub fn set_settings(&self, id: i64, settings_json: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        conn.execute(
            "UPDATE profiles SET settings_json = ?1 WHERE id = ?2",
            params![settings_json, id],
        )
        .map_err(|e| Error::Storage(format!("profiles set_settings: {e}")))?;
        Ok(())
    }

    /// Удалить профиль. Если он был активным — active становится NULL
    /// (через FK ON DELETE SET NULL).
    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        // Нужны foreign_keys ON для ON DELETE SET NULL.
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| Error::Storage(format!("profiles pragma: {e}")))?;
        conn.execute("DELETE FROM profiles WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("profiles delete: {e}")))?;
        Ok(())
    }

    /// Установить активный профиль. `None` → нет активного.
    pub fn set_active(&self, id: Option<i64>) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        if let Some(pid) = id {
            // Проверка существования.
            let exists: i64 = conn
                .query_row("SELECT COUNT(*) FROM profiles WHERE id = ?1", params![pid], |r| {
                    r.get(0)
                })
                .map_err(|e| Error::Storage(format!("profiles set_active check: {e}")))?;
            if exists == 0 {
                return Err(Error::NotFound(format!("profile id {pid}")));
            }
        }
        conn.execute(
            "UPDATE active_profile SET profile_id = ?1 WHERE lock = 0",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("profiles set_active: {e}")))?;
        Ok(())
    }

    /// Получить активный профиль.
    pub fn active(&self) -> Result<Option<Profile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let id = match active_id_locked(&conn)? {
            Some(i) => i,
            None => return Ok(None),
        };
        let p = conn
            .query_row(
                "SELECT id, name, storage_path, created_at, settings_json, encrypted_key_blob
                 FROM profiles WHERE id = ?1",
                params![id],
                row_to_profile_partial,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("profiles active: {e}")))?;
        Ok(p.map(|mut p| {
            p.is_default = true;
            p
        }))
    }

    /// Защитить профиль паролем.
    ///
    /// Генерирует новый случайный AES-256 ключ хранилища, оборачивает его
    /// через PBKDF2-HMAC-SHA256 + AES-256-GCM и сохраняет в `encrypted_key_blob`.
    /// Если профиль уже имеет пароль — он перезаписывается (ключ хранилища меняется).
    pub fn set_password(&self, id: i64, password: &str) -> Result<()> {
        let storage_key = profile_vault::generate_storage_key()?;
        let blob = profile_vault::seal(&storage_key, password.as_bytes())?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let n = conn
            .execute(
                "UPDATE profiles SET encrypted_key_blob = ?1 WHERE id = ?2",
                params![blob, id],
            )
            .map_err(|e| Error::Storage(format!("profiles set_password: {e}")))?;
        if n == 0 {
            return Err(Error::NotFound(format!("profile id {id}")));
        }
        Ok(())
    }

    /// Снять пароль с профиля.
    ///
    /// Требует правильный `current_password` для верификации перед снятием.
    /// После вызова профиль становится незашифрованным.
    pub fn clear_password(&self, id: i64, current_password: &str) -> Result<()> {
        // First unlock to verify password is correct.
        self.unlock(id, current_password)?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        conn.execute(
            "UPDATE profiles SET encrypted_key_blob = NULL WHERE id = ?1",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("profiles clear_password: {e}")))?;
        Ok(())
    }

    /// Разблокировать профиль и получить 32-байтовый ключ хранилища.
    ///
    /// Возвращает `Err` если профиль не защищён паролем, пароль неверный,
    /// или blob повреждён.
    pub fn unlock(&self, id: i64, password: &str) -> Result<[u8; profile_vault::KEY_LEN]> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT encrypted_key_blob FROM profiles WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("profiles unlock query: {e}")))?
            .flatten();

        let blob = blob.ok_or_else(|| {
            Error::Storage(format!("profile id {id} is not password-protected"))
        })?;
        drop(conn);
        profile_vault::open(&blob, password.as_bytes())
    }

    /// Проверить, защищён ли профиль паролем.
    pub fn is_encrypted(&self, id: i64) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let has_key: Option<Option<Vec<u8>>> = conn
            .query_row(
                "SELECT encrypted_key_blob FROM profiles WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("profiles is_encrypted: {e}")))?;
        match has_key {
            None => Err(Error::NotFound(format!("profile id {id}"))),
            Some(blob) => Ok(blob.is_some()),
        }
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("profiles mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("profiles count: {e}")))?;
        Ok(n)
    }
}

fn active_id_locked(conn: &Connection) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT profile_id FROM active_profile WHERE lock = 0",
        [],
        |r| r.get::<_, Option<i64>>(0),
    )
    .map_err(|e| Error::Storage(format!("profiles active_id: {e}")))
}

fn row_to_profile_partial(row: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let blob: Option<Vec<u8>> = row.get(5)?;
    Ok(Profile {
        id: row.get(0)?,
        name: row.get(1)?,
        storage_path: row.get(2)?,
        created_at: row.get(3)?,
        settings_json: row.get(4)?,
        is_default: false,      // заполняется caller-ом
        is_encrypted: blob.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> ProfileRegistry {
        ProfileRegistry::open_in_memory().unwrap()
    }

    #[test]
    fn create_and_get_basic() {
        let r = make();
        let id = r.create("Personal", "/profiles/personal/", "", 100).unwrap();
        let p = r.get(id).unwrap().unwrap();
        assert_eq!(p.name, "Personal");
        assert_eq!(p.storage_path, "/profiles/personal/");
        assert_eq!(p.created_at, 100);
        assert!(!p.is_default);
    }

    #[test]
    fn get_by_name_works() {
        let r = make();
        r.create("Work", "/profiles/work/", "", 100).unwrap();
        let p = r.get_by_name("Work").unwrap().unwrap();
        assert_eq!(p.name, "Work");
    }

    #[test]
    fn create_duplicate_name_fails() {
        let r = make();
        r.create("X", "/a/", "", 100).unwrap();
        assert!(r.create("X", "/b/", "", 200).is_err());
    }

    #[test]
    fn list_all_orders_by_created_at() {
        let r = make();
        r.create("Mid", "/m/", "", 200).unwrap();
        r.create("New", "/n/", "", 300).unwrap();
        r.create("Old", "/o/", "", 100).unwrap();
        let all = r.list_all().unwrap();
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Old", "Mid", "New"]);
    }

    #[test]
    fn rename_changes_name() {
        let r = make();
        let id = r.create("OldName", "/x/", "", 100).unwrap();
        r.rename(id, "NewName").unwrap();
        assert!(r.get_by_name("NewName").unwrap().is_some());
        assert!(r.get_by_name("OldName").unwrap().is_none());
    }

    #[test]
    fn set_settings_persists() {
        let r = make();
        let id = r.create("X", "/x/", "", 100).unwrap();
        r.set_settings(id, r#"{"theme":"dark"}"#).unwrap();
        let p = r.get(id).unwrap().unwrap();
        assert_eq!(p.settings_json, r#"{"theme":"dark"}"#);
    }

    #[test]
    fn delete_removes() {
        let r = make();
        let id = r.create("X", "/x/", "", 100).unwrap();
        r.delete(id).unwrap();
        assert!(r.get(id).unwrap().is_none());
    }

    #[test]
    fn set_active_marks_profile() {
        let r = make();
        let id1 = r.create("A", "/a/", "", 100).unwrap();
        let id2 = r.create("B", "/b/", "", 200).unwrap();
        r.set_active(Some(id1)).unwrap();
        let p1 = r.get(id1).unwrap().unwrap();
        let p2 = r.get(id2).unwrap().unwrap();
        assert!(p1.is_default);
        assert!(!p2.is_default);
        assert_eq!(r.active().unwrap().unwrap().id, id1);
    }

    #[test]
    fn set_active_switches() {
        let r = make();
        let id1 = r.create("A", "/a/", "", 100).unwrap();
        let id2 = r.create("B", "/b/", "", 200).unwrap();
        r.set_active(Some(id1)).unwrap();
        r.set_active(Some(id2)).unwrap();
        assert_eq!(r.active().unwrap().unwrap().id, id2);
        // id1 больше не default.
        assert!(!r.get(id1).unwrap().unwrap().is_default);
    }

    #[test]
    fn set_active_none_clears() {
        let r = make();
        let id = r.create("A", "/a/", "", 100).unwrap();
        r.set_active(Some(id)).unwrap();
        r.set_active(None).unwrap();
        assert!(r.active().unwrap().is_none());
    }

    #[test]
    fn set_active_nonexistent_returns_error() {
        let r = make();
        assert!(r.set_active(Some(999)).is_err());
    }

    #[test]
    fn delete_active_sets_active_to_null() {
        let r = make();
        let id = r.create("A", "/a/", "", 100).unwrap();
        r.set_active(Some(id)).unwrap();
        r.delete(id).unwrap();
        // FK ON DELETE SET NULL — active становится None.
        assert!(r.active().unwrap().is_none());
    }

    #[test]
    fn cyrillic_name_and_settings() {
        let r = make();
        let id = r
            .create(
                "Личный",
                "/профиль/личный/",
                r#"{"тема":"тёмная"}"#,
                100,
            )
            .unwrap();
        let p = r.get(id).unwrap().unwrap();
        assert_eq!(p.name, "Личный");
        assert_eq!(p.storage_path, "/профиль/личный/");
        assert_eq!(p.settings_json, r#"{"тема":"тёмная"}"#);
    }

    #[test]
    fn count_total() {
        let r = make();
        assert_eq!(r.count().unwrap(), 0);
        r.create("A", "/a/", "", 100).unwrap();
        r.create("B", "/b/", "", 200).unwrap();
        assert_eq!(r.count().unwrap(), 2);
    }

    #[test]
    fn no_active_returns_none() {
        let r = make();
        assert!(r.active().unwrap().is_none());
        r.create("A", "/a/", "", 100).unwrap();
        // Не set_active — всё ещё None.
        assert!(r.active().unwrap().is_none());
    }

    #[test]
    fn get_missing_returns_none() {
        let r = make();
        assert!(r.get(999).unwrap().is_none());
        assert!(r.get_by_name("nope").unwrap().is_none());
    }
}
