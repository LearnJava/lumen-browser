//! Per-origin permission grants. Storage layer для разрешений типа
//! camera / microphone / location / notifications / clipboard / midi.
//!
//! Структура: каждый (origin, permission_kind) имеет state
//! (Granted/Denied/Prompt), expires_at (для temporary grants),
//! last_used_at (для UI «недавно использовалось»).
//!
//! Phase 0: storage layer. Логика prompt-UI и проверки в JS-биндингах —
//! отдельные задачи.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Известные типы permissions. Произвольные строки тоже допустимы для
/// forward-compat (хранятся как `Other(String)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionKind {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    Clipboard,
    Midi,
    /// Persistent storage (Storage Standard).
    PersistentStorage,
    /// Forward-compat для не-описанных типов.
    Other(String),
}

impl PermissionKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Geolocation => "geolocation",
            Self::Notifications => "notifications",
            Self::Clipboard => "clipboard",
            Self::Midi => "midi",
            Self::PersistentStorage => "persistent-storage",
            Self::Other(s) => s.as_str(),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "camera" => Self::Camera,
            "microphone" => Self::Microphone,
            "geolocation" => Self::Geolocation,
            "notifications" => Self::Notifications,
            "clipboard" => Self::Clipboard,
            "midi" => Self::Midi,
            "persistent-storage" => Self::PersistentStorage,
            other => Self::Other(other.to_string()),
        }
    }
}

/// State permission grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionState {
    /// Нет записи — показать prompt пользователю.
    #[default]
    Prompt,
    /// Разрешено.
    Granted,
    /// Запрещено.
    Denied,
}

impl PermissionState {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Granted => "granted",
            Self::Denied => "denied",
        }
    }
    fn from_db_str(s: &str) -> Self {
        match s {
            "granted" => Self::Granted,
            "denied" => Self::Denied,
            _ => Self::Prompt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionEntry {
    pub origin: String,
    pub kind: PermissionKind,
    pub state: PermissionState,
    /// Unix timestamp истечения — `None` = permanent grant.
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

pub struct Permissions {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Permissions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Permissions").finish()
    }
}

impl Permissions {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("permissions open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("permissions open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS permissions (
                origin       TEXT NOT NULL,
                kind         TEXT NOT NULL,
                state        TEXT NOT NULL DEFAULT 'prompt',
                expires_at   INTEGER,
                last_used_at INTEGER,
                PRIMARY KEY (origin, kind)
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS permissions_origin_idx ON permissions(origin);
            "#,
        )
        .map_err(|e| Error::Storage(format!("permissions init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Поставить state для (origin, kind). Перезаписывает существующий.
    pub fn set(
        &self,
        origin: &str,
        kind: &PermissionKind,
        state: PermissionState,
        expires_at: Option<i64>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO permissions (origin, kind, state, expires_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (origin, kind) DO UPDATE SET
                 state = excluded.state,
                 expires_at = excluded.expires_at",
            params![origin, kind.as_str(), state.as_db_str(), expires_at],
        )
        .map_err(|e| Error::Storage(format!("permissions set: {e}")))?;
        Ok(())
    }

    /// Получить текущий state. Если запись есть, но `expires_at < now` —
    /// возвращается `Prompt` (expired permanent grant).
    pub fn query(&self, origin: &str, kind: &PermissionKind, now_unix: i64) -> Result<PermissionState> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        let row: Option<(String, Option<i64>)> = conn
            .query_row(
                "SELECT state, expires_at FROM permissions WHERE origin = ?1 AND kind = ?2",
                params![origin, kind.as_str()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("permissions query: {e}")))?;
        match row {
            None => Ok(PermissionState::Prompt),
            Some((state_str, expires)) => {
                if let Some(exp) = expires
                    && now_unix >= exp
                {
                    // Expired — фактически больше нет grant-а.
                    return Ok(PermissionState::Prompt);
                }
                Ok(PermissionState::from_db_str(&state_str))
            }
        }
    }

    /// Обновить last_used_at — вызывается при фактическом использовании
    /// разрешённого ресурса.
    pub fn touch(&self, origin: &str, kind: &PermissionKind, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        conn.execute(
            "UPDATE permissions SET last_used_at = ?1 WHERE origin = ?2 AND kind = ?3",
            params![now_unix, origin, kind.as_str()],
        )
        .map_err(|e| Error::Storage(format!("permissions touch: {e}")))?;
        Ok(())
    }

    /// Удалить grant (revoke).
    pub fn revoke(&self, origin: &str, kind: &PermissionKind) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM permissions WHERE origin = ?1 AND kind = ?2",
            params![origin, kind.as_str()],
        )
        .map_err(|e| Error::Storage(format!("permissions revoke: {e}")))?;
        Ok(())
    }

    /// Все permissions для одного origin.
    pub fn list_for_origin(&self, origin: &str) -> Result<Vec<PermissionEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT origin, kind, state, expires_at, last_used_at
                 FROM permissions WHERE origin = ?1 ORDER BY kind ASC",
            )
            .map_err(|e| Error::Storage(format!("permissions list_for_origin prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin], row_to_entry)
            .map_err(|e| Error::Storage(format!("permissions list_for_origin query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("permissions row: {e}")))?);
        }
        Ok(out)
    }

    /// Все записи в БД (для UI permissions-manager).
    pub fn list_all(&self) -> Result<Vec<PermissionEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT origin, kind, state, expires_at, last_used_at
                 FROM permissions ORDER BY origin ASC, kind ASC",
            )
            .map_err(|e| Error::Storage(format!("permissions list_all prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_entry)
            .map_err(|e| Error::Storage(format!("permissions list_all query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("permissions row: {e}")))?);
        }
        Ok(out)
    }

    /// Удалить все expired grants. Возвращает число удалённых.
    pub fn clear_expired(&self, now_unix: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM permissions WHERE expires_at IS NOT NULL AND expires_at < ?1",
                params![now_unix],
            )
            .map_err(|e| Error::Storage(format!("permissions clear_expired: {e}")))?;
        Ok(n)
    }

    /// Удалить все permissions для origin (clear site data).
    pub fn clear_origin(&self, origin: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions mutex poisoned".into()))?;
        let n = conn
            .execute("DELETE FROM permissions WHERE origin = ?1", params![origin])
            .map_err(|e| Error::Storage(format!("permissions clear_origin: {e}")))?;
        Ok(n)
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<PermissionEntry> {
    Ok(PermissionEntry {
        origin: row.get(0)?,
        kind: PermissionKind::parse(&row.get::<_, String>(1)?),
        state: PermissionState::from_db_str(&row.get::<_, String>(2)?),
        expires_at: row.get(3)?,
        last_used_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Permissions {
        Permissions::open_in_memory().unwrap()
    }

    #[test]
    fn query_unknown_returns_prompt() {
        let p = make();
        let st = p.query("https://x/", &PermissionKind::Camera, 0).unwrap();
        assert_eq!(st, PermissionState::Prompt);
    }

    #[test]
    fn set_granted_then_query() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, None)
            .unwrap();
        let st = p.query("https://x/", &PermissionKind::Camera, 0).unwrap();
        assert_eq!(st, PermissionState::Granted);
    }

    #[test]
    fn set_denied_persists() {
        let p = make();
        p.set("https://x/", &PermissionKind::Microphone, PermissionState::Denied, None)
            .unwrap();
        let st = p.query("https://x/", &PermissionKind::Microphone, 0).unwrap();
        assert_eq!(st, PermissionState::Denied);
    }

    #[test]
    fn set_overwrites_existing() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, None)
            .unwrap();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Denied, None)
            .unwrap();
        let st = p.query("https://x/", &PermissionKind::Camera, 0).unwrap();
        assert_eq!(st, PermissionState::Denied);
    }

    #[test]
    fn expires_returns_prompt_after_expiry() {
        let p = make();
        p.set(
            "https://x/",
            &PermissionKind::Camera,
            PermissionState::Granted,
            Some(100),
        )
        .unwrap();
        // До expiry — granted.
        assert_eq!(
            p.query("https://x/", &PermissionKind::Camera, 50).unwrap(),
            PermissionState::Granted
        );
        // После expiry — prompt.
        assert_eq!(
            p.query("https://x/", &PermissionKind::Camera, 200).unwrap(),
            PermissionState::Prompt
        );
    }

    #[test]
    fn touch_updates_last_used() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, None)
            .unwrap();
        p.touch("https://x/", &PermissionKind::Camera, 500).unwrap();
        let entries = p.list_for_origin("https://x/").unwrap();
        assert_eq!(entries[0].last_used_at, Some(500));
    }

    #[test]
    fn revoke_removes_grant() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, None)
            .unwrap();
        p.revoke("https://x/", &PermissionKind::Camera).unwrap();
        assert_eq!(
            p.query("https://x/", &PermissionKind::Camera, 0).unwrap(),
            PermissionState::Prompt
        );
    }

    #[test]
    fn different_kinds_isolated_per_origin() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, None).unwrap();
        p.set("https://x/", &PermissionKind::Microphone, PermissionState::Denied, None).unwrap();
        assert_eq!(
            p.query("https://x/", &PermissionKind::Camera, 0).unwrap(),
            PermissionState::Granted
        );
        assert_eq!(
            p.query("https://x/", &PermissionKind::Microphone, 0).unwrap(),
            PermissionState::Denied
        );
    }

    #[test]
    fn different_origins_isolated() {
        let p = make();
        p.set("https://a/", &PermissionKind::Camera, PermissionState::Granted, None).unwrap();
        assert_eq!(
            p.query("https://a/", &PermissionKind::Camera, 0).unwrap(),
            PermissionState::Granted
        );
        assert_eq!(
            p.query("https://b/", &PermissionKind::Camera, 0).unwrap(),
            PermissionState::Prompt
        );
    }

    #[test]
    fn list_for_origin_returns_all_kinds() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, None).unwrap();
        p.set("https://x/", &PermissionKind::Microphone, PermissionState::Denied, None).unwrap();
        p.set("https://y/", &PermissionKind::Camera, PermissionState::Granted, None).unwrap();
        let entries = p.list_for_origin("https://x/").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn clear_origin_removes_all_kinds() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, None).unwrap();
        p.set("https://x/", &PermissionKind::Microphone, PermissionState::Granted, None).unwrap();
        let removed = p.clear_origin("https://x/").unwrap();
        assert_eq!(removed, 2);
        assert!(p.list_for_origin("https://x/").unwrap().is_empty());
    }

    #[test]
    fn clear_expired_removes_only_past() {
        let p = make();
        p.set("https://x/", &PermissionKind::Camera, PermissionState::Granted, Some(100)).unwrap();
        p.set("https://y/", &PermissionKind::Camera, PermissionState::Granted, Some(1000)).unwrap();
        p.set("https://z/", &PermissionKind::Camera, PermissionState::Granted, None).unwrap();
        let removed = p.clear_expired(500).unwrap();
        assert_eq!(removed, 1);
        // y и z остались.
        assert!(p.list_for_origin("https://y/").unwrap().len() == 1);
        assert!(p.list_for_origin("https://z/").unwrap().len() == 1);
        assert!(p.list_for_origin("https://x/").unwrap().is_empty());
    }

    #[test]
    fn permission_kind_other_forward_compat() {
        let p = make();
        let kind = PermissionKind::Other("future-feature".into());
        p.set("https://x/", &kind, PermissionState::Granted, None).unwrap();
        let st = p.query("https://x/", &kind, 0).unwrap();
        assert_eq!(st, PermissionState::Granted);
        let entries = p.list_for_origin("https://x/").unwrap();
        assert_eq!(entries[0].kind, kind);
    }

    #[test]
    fn permission_kind_round_trip() {
        for k in [
            PermissionKind::Camera,
            PermissionKind::Microphone,
            PermissionKind::Geolocation,
            PermissionKind::Notifications,
            PermissionKind::Clipboard,
            PermissionKind::Midi,
            PermissionKind::PersistentStorage,
            PermissionKind::Other("custom".into()),
        ] {
            assert_eq!(PermissionKind::parse(k.as_str()), k);
        }
    }

    #[test]
    fn cyrillic_origin() {
        let p = make();
        p.set("https://пример.рф/", &PermissionKind::Camera, PermissionState::Granted, None)
            .unwrap();
        assert_eq!(
            p.query("https://пример.рф/", &PermissionKind::Camera, 0).unwrap(),
            PermissionState::Granted
        );
    }
}
