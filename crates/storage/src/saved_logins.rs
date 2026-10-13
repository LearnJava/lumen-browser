//! Сохранённые логины и пароли (UX-PASSWORDS, срез 2).
//!
//! Пароль лежит в SQLite только в виде AES-256-GCM шифртекста. Ключ — 32 байта,
//! его выдаёт вызывающая сторона (см. [`crate::profile_vault`]): модуль не знает,
//! откуда ключ взялся, и не хранит его. К шифртексту привязаны `origin` и логин
//! (AAD), поэтому строку нельзя перенести под другой сайт или аккаунт, подменив
//! значения в таблице. Логин и origin остаются открытыми — по ним ищут запись.
//!
//! Список «никогда не сохранять для этого сайта» живёт в том же файле.

use std::path::Path;
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

use crate::migrations::{run_migrations, set_common_pragmas, Migration};
use crate::profile_vault::KEY_LEN;

const NONCE_LEN: usize = 12;

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
    CREATE TABLE IF NOT EXISTS saved_logins (
        origin     TEXT NOT NULL,
        username   TEXT NOT NULL,
        nonce      BLOB NOT NULL,
        ciphertext BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        last_used  INTEGER NOT NULL,
        PRIMARY KEY (origin, username)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS login_never_save (
        origin TEXT NOT NULL PRIMARY KEY
    ) WITHOUT ROWID;
    "#,
}];

/// Расшифрованная запись.
#[derive(Clone, PartialEq, Eq)]
pub struct SavedLogin {
    /// `scheme://host[:port]`.
    pub origin: String,
    /// Логин (может быть пустым — форма только с паролем).
    pub username: String,
    /// Пароль в открытом виде; живёт только пока запись в памяти.
    pub password: String,
    /// Unix-секунды первого сохранения.
    pub created_at: i64,
    /// Unix-секунды последнего сохранения или подстановки.
    pub last_used: i64,
}

impl std::fmt::Debug for SavedLogin {
    // Пароль не попадает ни в логи, ни в вывод паники теста.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedLogin")
            .field("origin", &self.origin)
            .field("username", &self.username)
            .field("password", &"<скрыт>")
            .finish()
    }
}

/// Что сделал [`SavedLogins::save`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Записи для `(origin, username)` не было.
    Added,
    /// Запись была, пароль заменён.
    Updated,
    /// Запись была с тем же паролем — менять нечего.
    Unchanged,
}

/// Хранилище логинов, зашифрованное ключом профиля.
pub struct SavedLogins {
    conn: Mutex<Connection>,
    key: [u8; KEY_LEN],
}

impl std::fmt::Debug for SavedLogins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedLogins").finish_non_exhaustive()
    }
}

impl SavedLogins {
    /// Открыть (или создать) файл базы.
    pub fn open(path: impl AsRef<Path>, key: [u8; KEY_LEN]) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("saved_logins open: {e}")))?;
        Self::init(conn, key)
    }

    /// База в памяти (приватный сеанс, тесты).
    pub fn open_in_memory(key: [u8; KEY_LEN]) -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("saved_logins open_in_memory: {e}")))?;
        Self::init(conn, key)
    }

    fn init(mut conn: Connection, key: [u8; KEY_LEN]) -> Result<Self> {
        set_common_pragmas(&conn)
            .map_err(|e| Error::Storage(format!("saved_logins pragmas: {e}")))?;
        run_migrations(&mut conn, MIGRATIONS)
            .map_err(|e| Error::Storage(format!("saved_logins init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn), key })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Storage("saved_logins mutex poisoned".into()))
    }

    fn cipher(&self) -> Result<Aes256Gcm> {
        Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| Error::Storage(format!("saved_logins aes init: {e}")))
    }

    fn aad(origin: &str, username: &str) -> Vec<u8> {
        let mut aad = Vec::with_capacity(origin.len() + username.len() + 1);
        aad.extend_from_slice(origin.as_bytes());
        aad.push(0);
        aad.extend_from_slice(username.as_bytes());
        aad
    }

    fn decrypt(&self, origin: &str, username: &str, nonce: &[u8], ct: &[u8]) -> Result<String> {
        if nonce.len() != NONCE_LEN {
            return Err(Error::Storage("saved_logins: повреждённый nonce".into()));
        }
        let aad = Self::aad(origin, username);
        let plain = self
            .cipher()?
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: &aad })
            .map_err(|_| Error::Storage("saved_logins: неверный ключ или запись повреждена".into()))?;
        String::from_utf8(plain).map_err(|_| Error::Storage("saved_logins: пароль не UTF-8".into()))
    }

    /// Сохранить пароль для `(origin, username)`. Пустой пароль не пишется.
    pub fn save(
        &self,
        origin: &str,
        username: &str,
        password: &str,
        now_unix: i64,
    ) -> Result<SaveOutcome> {
        if password.is_empty() {
            return Err(Error::Storage("saved_logins: пустой пароль".into()));
        }
        let existing = self.get(origin, username)?;
        if existing.as_ref().is_some_and(|e| e.password == password) {
            return Ok(SaveOutcome::Unchanged);
        }
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::getrandom(&mut nonce)
            .map_err(|e| Error::Storage(format!("saved_logins getrandom: {e}")))?;
        let aad = Self::aad(origin, username);
        let ct = self
            .cipher()?
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: password.as_bytes(), aad: &aad })
            .map_err(|e| Error::Storage(format!("saved_logins encrypt: {e}")))?;
        self.lock()?
            .execute(
                "INSERT INTO saved_logins (origin, username, nonce, ciphertext, created_at, last_used)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT (origin, username) DO UPDATE SET
                     nonce = excluded.nonce,
                     ciphertext = excluded.ciphertext,
                     last_used = excluded.last_used",
                params![origin, username, nonce.as_slice(), ct, now_unix],
            )
            .map_err(|e| Error::Storage(format!("saved_logins save: {e}")))?;
        Ok(if existing.is_some() { SaveOutcome::Updated } else { SaveOutcome::Added })
    }

    /// Запись для `(origin, username)`.
    pub fn get(&self, origin: &str, username: &str) -> Result<Option<SavedLogin>> {
        let row: Option<(Vec<u8>, Vec<u8>, i64, i64)> = {
            let conn = self.lock()?;
            let mut stmt = conn
                .prepare_cached(
                    "SELECT nonce, ciphertext, created_at, last_used
                     FROM saved_logins WHERE origin = ?1 AND username = ?2",
                )
                .map_err(|e| Error::Storage(format!("saved_logins get prepare: {e}")))?;
            let mut rows = stmt
                .query(params![origin, username])
                .map_err(|e| Error::Storage(format!("saved_logins get: {e}")))?;
            let next = rows
                .next()
                .map_err(|e| Error::Storage(format!("saved_logins get row: {e}")))?;
            match next {
                Some(r) => {
                    let col = |e: rusqlite::Error| Error::Storage(format!("saved_logins col: {e}"));
                    Some((
                        r.get(0).map_err(col)?,
                        r.get(1).map_err(col)?,
                        r.get(2).map_err(col)?,
                        r.get(3).map_err(col)?,
                    ))
                }
                None => None,
            }
        };
        let Some((nonce, ct, created_at, last_used)) = row else { return Ok(None) };
        let password = self.decrypt(origin, username, &nonce, &ct)?;
        Ok(Some(SavedLogin {
            origin: origin.to_owned(),
            username: username.to_owned(),
            password,
            created_at,
            last_used,
        }))
    }

    /// Логины, сохранённые для origin, — без паролей, последний использованный первым.
    pub fn usernames_for(&self, origin: &str) -> Result<Vec<String>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT username FROM saved_logins WHERE origin = ?1
                 ORDER BY last_used DESC, username",
            )
            .map_err(|e| Error::Storage(format!("saved_logins usernames prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin], |r| r.get::<_, String>(0))
            .map_err(|e| Error::Storage(format!("saved_logins usernames: {e}")))?;
        rows.map(|r| r.map_err(|e| Error::Storage(format!("saved_logins row: {e}")))).collect()
    }

    /// Все пары `(origin, username)` без паролей — для страницы управления.
    pub fn list(&self) -> Result<Vec<(String, String)>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare_cached("SELECT origin, username FROM saved_logins ORDER BY origin, username")
            .map_err(|e| Error::Storage(format!("saved_logins list prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| Error::Storage(format!("saved_logins list: {e}")))?;
        rows.map(|r| r.map_err(|e| Error::Storage(format!("saved_logins row: {e}")))).collect()
    }

    /// Удалить запись; `true`, если она была.
    pub fn delete(&self, origin: &str, username: &str) -> Result<bool> {
        let n = self
            .lock()?
            .execute(
                "DELETE FROM saved_logins WHERE origin = ?1 AND username = ?2",
                params![origin, username],
            )
            .map_err(|e| Error::Storage(format!("saved_logins delete: {e}")))?;
        Ok(n > 0)
    }

    /// Больше не предлагать сохранять пароли для origin.
    pub fn never_save(&self, origin: &str) -> Result<()> {
        self.lock()?
            .execute("INSERT OR IGNORE INTO login_never_save (origin) VALUES (?1)", params![origin])
            .map_err(|e| Error::Storage(format!("saved_logins never_save: {e}")))?;
        Ok(())
    }

    /// Снять запрет, поставленный [`Self::never_save`].
    pub fn allow_save(&self, origin: &str) -> Result<()> {
        self.lock()?
            .execute("DELETE FROM login_never_save WHERE origin = ?1", params![origin])
            .map_err(|e| Error::Storage(format!("saved_logins allow_save: {e}")))?;
        Ok(())
    }

    /// `true`, если для origin сохранение отключено.
    pub fn is_never_save(&self, origin: &str) -> Result<bool> {
        let conn = self.lock()?;
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM login_never_save WHERE origin = ?1",
                params![origin],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("saved_logins is_never_save: {e}")))?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_LEN] = [7; KEY_LEN];
    const SITE: &str = "https://example.com";

    fn store() -> SavedLogins {
        SavedLogins::open_in_memory(KEY).unwrap()
    }

    #[test]
    fn round_trip() {
        let s = store();
        assert_eq!(s.save(SITE, "anna", "s3cret", 10).unwrap(), SaveOutcome::Added);
        let got = s.get(SITE, "anna").unwrap().unwrap();
        assert_eq!(got.password, "s3cret");
        assert_eq!((got.created_at, got.last_used), (10, 10));
        assert!(s.get(SITE, "bob").unwrap().is_none());
    }

    #[test]
    fn update_and_unchanged() {
        let s = store();
        s.save(SITE, "anna", "one", 10).unwrap();
        assert_eq!(s.save(SITE, "anna", "one", 20).unwrap(), SaveOutcome::Unchanged);
        assert_eq!(s.save(SITE, "anna", "two", 30).unwrap(), SaveOutcome::Updated);
        let got = s.get(SITE, "anna").unwrap().unwrap();
        assert_eq!(got.password, "two");
        assert_eq!((got.created_at, got.last_used), (10, 30));
    }

    #[test]
    fn empty_password_is_rejected() {
        assert!(store().save(SITE, "anna", "", 1).is_err());
    }

    #[test]
    fn password_is_not_stored_in_plain_text() {
        let s = store();
        s.save(SITE, "anna", "hunter2-hunter2", 1).unwrap();
        let conn = s.conn.lock().unwrap();
        let ct: Vec<u8> =
            conn.query_row("SELECT ciphertext FROM saved_logins", [], |r| r.get(0)).unwrap();
        assert!(!ct.windows(7).any(|w| w == b"hunter2"));
        assert_eq!(ct.len(), "hunter2-hunter2".len() + 16);
    }

    #[test]
    fn same_password_gets_different_ciphertext_per_row() {
        let s = store();
        s.save(SITE, "a", "same", 1).unwrap();
        s.save(SITE, "b", "same", 1).unwrap();
        let conn = s.conn.lock().unwrap();
        let cts: Vec<Vec<u8>> = conn
            .prepare("SELECT ciphertext FROM saved_logins ORDER BY username")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_ne!(cts[0], cts[1]);
    }

    #[test]
    fn wrong_key_cannot_read() {
        let path = std::env::temp_dir().join(format!("lumen-logins-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        SavedLogins::open(&path, KEY).unwrap().save(SITE, "anna", "pw", 1).unwrap();
        let other = SavedLogins::open(&path, [9; KEY_LEN]).unwrap();
        assert!(other.get(SITE, "anna").is_err());
        let same = SavedLogins::open(&path, KEY).unwrap();
        assert_eq!(same.get(SITE, "anna").unwrap().unwrap().password, "pw");
        drop((other, same));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn row_moved_to_another_origin_fails_authentication() {
        let s = store();
        s.save(SITE, "anna", "pw", 1).unwrap();
        s.lock()
            .unwrap()
            .execute("UPDATE saved_logins SET origin = 'https://evil.test'", [])
            .unwrap();
        assert!(s.get("https://evil.test", "anna").is_err());
    }

    #[test]
    fn usernames_most_recent_first_and_delete() {
        let s = store();
        s.save(SITE, "old", "p", 1).unwrap();
        s.save(SITE, "new", "p", 5).unwrap();
        s.save("https://other.test", "x", "p", 9).unwrap();
        assert_eq!(s.usernames_for(SITE).unwrap(), ["new", "old"]);
        assert_eq!(s.list().unwrap().len(), 3);
        assert!(s.delete(SITE, "new").unwrap());
        assert!(!s.delete(SITE, "new").unwrap());
        assert_eq!(s.usernames_for(SITE).unwrap(), ["old"]);
    }

    #[test]
    fn never_save_list() {
        let s = store();
        assert!(!s.is_never_save(SITE).unwrap());
        s.never_save(SITE).unwrap();
        s.never_save(SITE).unwrap();
        assert!(s.is_never_save(SITE).unwrap());
        s.allow_save(SITE).unwrap();
        assert!(!s.is_never_save(SITE).unwrap());
    }

    #[test]
    fn debug_hides_password() {
        let l = SavedLogin {
            origin: SITE.into(),
            username: "anna".into(),
            password: "topsecret".into(),
            created_at: 0,
            last_used: 0,
        };
        assert!(!format!("{l:?}").contains("topsecret"));
    }
}
