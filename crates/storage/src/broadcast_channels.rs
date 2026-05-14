//! BroadcastChannel registry — origin-keyed channel-имена для cross-tab
//! messaging.
//!
//! Spec: <https://html.spec.whatwg.org/multipage/web-messaging.html#broadcasting-to-other-browsing-contexts>.
//! BroadcastChannel позволяет вкладкам одного origin-а обмениваться
//! сообщениями через общее имя канала. Сами сообщения — ephemeral
//! (исчезают, когда нет слушателей), но регистрация активных каналов
//! полезна для:
//! - debug / UI «какие channels слушают эта origin»;
//! - persistence открытых channels для восстановления при reload;
//! - tracking активности per-origin.
//!
//! Phase 0: storage-layer plumbing. Реальный runtime (MessageEvent
//! dispatching между tab-ами одного origin-а) — отдельная задача
//! (нужен IPC между tab-процессами или event loop в shell).

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRegistration {
    pub id: i64,
    pub origin: String,
    pub channel_name: String,
    /// ID контекста, который подписан (вкладка / worker). Может быть
    /// пустой строкой для legacy-регистраций без context-tracking.
    pub context_id: String,
    pub registered_at: i64,
}

pub struct BroadcastChannels {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for BroadcastChannels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastChannels").finish()
    }
}

impl BroadcastChannels {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("broadcast_channels open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("broadcast_channels open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS broadcast_channels (
                id             INTEGER PRIMARY KEY,
                origin         TEXT NOT NULL,
                channel_name   TEXT NOT NULL,
                context_id     TEXT NOT NULL DEFAULT '',
                registered_at  INTEGER NOT NULL,
                UNIQUE (origin, channel_name, context_id)
            );
            CREATE INDEX IF NOT EXISTS bc_origin_idx ON broadcast_channels(origin);
            CREATE INDEX IF NOT EXISTS bc_origin_name_idx ON broadcast_channels(origin, channel_name);
            "#,
        )
        .map_err(|e| Error::Storage(format!("broadcast_channels init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// `new BroadcastChannel(name)` — зарегистрировать. Если уже была
    /// регистрация с тем же (origin, channel_name, context_id) —
    /// обновляет registered_at.
    pub fn register(
        &self,
        origin: &str,
        channel_name: &str,
        context_id: &str,
        registered_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("broadcast_channels mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO broadcast_channels (origin, channel_name, context_id, registered_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (origin, channel_name, context_id) DO UPDATE SET
                 registered_at = excluded.registered_at",
            params![origin, channel_name, context_id, registered_at],
        )
        .map_err(|e| Error::Storage(format!("broadcast_channels register: {e}")))?;
        let id: i64 = conn
            .query_row(
                "SELECT id FROM broadcast_channels
                 WHERE origin = ?1 AND channel_name = ?2 AND context_id = ?3",
                params![origin, channel_name, context_id],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("broadcast_channels register-lookup: {e}")))?;
        Ok(id)
    }

    pub fn get(&self, id: i64) -> Result<Option<ChannelRegistration>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("broadcast_channels mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, origin, channel_name, context_id, registered_at
             FROM broadcast_channels WHERE id = ?1",
            params![id],
            row_to_reg,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("broadcast_channels get: {e}")))
    }

    /// Все listeners на конкретном канале origin-а.
    pub fn listeners(&self, origin: &str, channel_name: &str) -> Result<Vec<ChannelRegistration>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("broadcast_channels mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, origin, channel_name, context_id, registered_at
                 FROM broadcast_channels WHERE origin = ?1 AND channel_name = ?2
                 ORDER BY registered_at ASC",
            )
            .map_err(|e| Error::Storage(format!("broadcast_channels listeners prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin, channel_name], row_to_reg)
            .map_err(|e| Error::Storage(format!("broadcast_channels listeners query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("broadcast_channels row: {e}")))?);
        }
        Ok(out)
    }

    /// Все channel-имена, на которые подписан origin (distinct).
    pub fn channels_for_origin(&self, origin: &str) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("broadcast_channels mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT DISTINCT channel_name FROM broadcast_channels
                 WHERE origin = ?1 ORDER BY channel_name ASC",
            )
            .map_err(|e| Error::Storage(format!("broadcast_channels channels prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin], |r| r.get::<_, String>(0))
            .map_err(|e| Error::Storage(format!("broadcast_channels channels query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("broadcast_channels row: {e}")))?);
        }
        Ok(out)
    }

    /// `channel.close()` — снять регистрацию.
    pub fn unregister(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("broadcast_channels mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM broadcast_channels WHERE id = ?1",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("broadcast_channels unregister: {e}")))?;
        Ok(())
    }

    /// При закрытии вкладки — снять все регистрации этого context-а.
    pub fn unregister_context(&self, context_id: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("broadcast_channels mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM broadcast_channels WHERE context_id = ?1",
                params![context_id],
            )
            .map_err(|e| Error::Storage(format!("broadcast_channels unregister_context: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("broadcast_channels mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM broadcast_channels", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("broadcast_channels count: {e}")))?;
        Ok(n)
    }
}

fn row_to_reg(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChannelRegistration> {
    Ok(ChannelRegistration {
        id: row.get(0)?,
        origin: row.get(1)?,
        channel_name: row.get(2)?,
        context_id: row.get(3)?,
        registered_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> BroadcastChannels {
        BroadcastChannels::open_in_memory().unwrap()
    }

    #[test]
    fn register_and_get() {
        let bc = make();
        let id = bc.register("https://app/", "main", "tab-1", 100).unwrap();
        let r = bc.get(id).unwrap().unwrap();
        assert_eq!(r.origin, "https://app/");
        assert_eq!(r.channel_name, "main");
        assert_eq!(r.context_id, "tab-1");
    }

    #[test]
    fn duplicate_registration_updates_timestamp() {
        let bc = make();
        let id1 = bc.register("https://app/", "main", "tab-1", 100).unwrap();
        let id2 = bc.register("https://app/", "main", "tab-1", 200).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(bc.get(id1).unwrap().unwrap().registered_at, 200);
    }

    #[test]
    fn listeners_returns_all_for_channel() {
        let bc = make();
        bc.register("https://app/", "main", "tab-1", 100).unwrap();
        bc.register("https://app/", "main", "tab-2", 200).unwrap();
        bc.register("https://app/", "other", "tab-3", 300).unwrap();
        let list = bc.listeners("https://app/", "main").unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn channels_for_origin_distinct() {
        let bc = make();
        bc.register("https://app/", "main", "tab-1", 100).unwrap();
        bc.register("https://app/", "main", "tab-2", 200).unwrap();
        bc.register("https://app/", "logs", "tab-1", 300).unwrap();
        let names = bc.channels_for_origin("https://app/").unwrap();
        assert_eq!(names, vec!["logs".to_string(), "main".to_string()]);
    }

    #[test]
    fn unregister_removes_only_this_listener() {
        let bc = make();
        let id1 = bc.register("https://app/", "main", "tab-1", 100).unwrap();
        bc.register("https://app/", "main", "tab-2", 200).unwrap();
        bc.unregister(id1).unwrap();
        let list = bc.listeners("https://app/", "main").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].context_id, "tab-2");
    }

    #[test]
    fn unregister_context_removes_all_listeners_of_tab() {
        let bc = make();
        bc.register("https://app/", "main", "tab-1", 100).unwrap();
        bc.register("https://app/", "logs", "tab-1", 200).unwrap();
        bc.register("https://app/", "main", "tab-2", 300).unwrap();
        let n = bc.unregister_context("tab-1").unwrap();
        assert_eq!(n, 2);
        assert_eq!(bc.count().unwrap(), 1);
    }

    #[test]
    fn cross_origin_isolated() {
        let bc = make();
        bc.register("https://a/", "main", "tab-1", 100).unwrap();
        bc.register("https://b/", "main", "tab-1", 200).unwrap();
        // Один origin не видит каналов другого.
        assert_eq!(bc.listeners("https://a/", "main").unwrap().len(), 1);
        assert_eq!(bc.listeners("https://b/", "main").unwrap().len(), 1);
    }

    #[test]
    fn count_works() {
        let bc = make();
        assert_eq!(bc.count().unwrap(), 0);
        bc.register("https://a/", "x", "t1", 100).unwrap();
        bc.register("https://b/", "y", "t1", 200).unwrap();
        assert_eq!(bc.count().unwrap(), 2);
    }
}
