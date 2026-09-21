//! Pending decrypted Push API messages, queued per subscription until a
//! `push`-event dispatch consumes them (Ph3 push-api срез 5).
//!
//! Срез 4 (docs/tasks/ph3-push-api.md) stops at "receive + decrypt": a
//! delivered message lands here via [`PushMessages::enqueue`] and waits for
//! [`PushMessages::take_oldest`] to hand it to the Service Worker `push`
//! event in a later slice. In-memory only (one process session) — unlike
//! subscriptions, an undelivered push message has no cross-restart value.

#![allow(missing_docs)]

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::migrations::{run_migrations, set_common_pragmas, Migration};

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
    CREATE TABLE IF NOT EXISTS push_messages (
        id               INTEGER PRIMARY KEY,
        subscription_id  INTEGER NOT NULL,
        plaintext        BLOB NOT NULL,
        received_at      INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS push_messages_sub_idx ON push_messages(subscription_id, id);
    "#,
}];

pub struct PushMessages {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for PushMessages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushMessages").finish()
    }
}

impl PushMessages {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn =
            Connection::open(path).map_err(|e| Error::Storage(format!("push_messages open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("push_messages open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        set_common_pragmas(&conn).map_err(|e| Error::Storage(format!("push_messages pragmas: {e}")))?;
        run_migrations(&mut conn, MIGRATIONS)
            .map_err(|e| Error::Storage(format!("push_messages init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Queue a decrypted push message for `subscription_id`.
    pub fn enqueue(&self, subscription_id: i64, plaintext: &[u8], received_at: i64) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_messages mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO push_messages (subscription_id, plaintext, received_at) VALUES (?1, ?2, ?3)",
            params![subscription_id, plaintext, received_at],
        )
        .map_err(|e| Error::Storage(format!("push_messages enqueue: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Remove and return the oldest queued message for `subscription_id`, if any.
    pub fn take_oldest(&self, subscription_id: i64) -> Result<Option<Vec<u8>>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_messages mutex poisoned".into()))?;
        let row: Option<(i64, Vec<u8>)> = conn
            .query_row(
                "SELECT id, plaintext FROM push_messages WHERE subscription_id = ?1 ORDER BY id ASC LIMIT 1",
                params![subscription_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("push_messages take_oldest select: {e}")))?;
        let Some((id, plaintext)) = row else {
            return Ok(None);
        };
        conn.execute("DELETE FROM push_messages WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("push_messages take_oldest delete: {e}")))?;
        Ok(Some(plaintext))
    }

    /// Number of queued (undelivered) messages for `subscription_id`.
    pub fn count_pending(&self, subscription_id: i64) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_messages mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM push_messages WHERE subscription_id = ?1",
                params![subscription_id],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("push_messages count_pending: {e}")))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> PushMessages {
        PushMessages::open_in_memory().unwrap()
    }

    #[test]
    fn enqueue_then_take_oldest_returns_plaintext() {
        let m = make();
        m.enqueue(1, b"hello", 100).unwrap();
        assert_eq!(m.take_oldest(1).unwrap(), Some(b"hello".to_vec()));
    }

    #[test]
    fn take_oldest_is_fifo() {
        let m = make();
        m.enqueue(1, b"first", 100).unwrap();
        m.enqueue(1, b"second", 200).unwrap();
        assert_eq!(m.take_oldest(1).unwrap(), Some(b"first".to_vec()));
        assert_eq!(m.take_oldest(1).unwrap(), Some(b"second".to_vec()));
        assert_eq!(m.take_oldest(1).unwrap(), None);
    }

    #[test]
    fn take_oldest_removes_the_message() {
        let m = make();
        m.enqueue(1, b"once", 100).unwrap();
        assert!(m.take_oldest(1).unwrap().is_some());
        assert!(m.take_oldest(1).unwrap().is_none());
    }

    #[test]
    fn messages_are_isolated_per_subscription() {
        let m = make();
        m.enqueue(1, b"for-one", 100).unwrap();
        m.enqueue(2, b"for-two", 100).unwrap();
        assert_eq!(m.take_oldest(1).unwrap(), Some(b"for-one".to_vec()));
        assert_eq!(m.take_oldest(2).unwrap(), Some(b"for-two".to_vec()));
    }

    #[test]
    fn take_oldest_on_empty_queue_is_none() {
        let m = make();
        assert_eq!(m.take_oldest(1).unwrap(), None);
    }

    #[test]
    fn count_pending_reflects_queue_size() {
        let m = make();
        assert_eq!(m.count_pending(1).unwrap(), 0);
        m.enqueue(1, b"a", 100).unwrap();
        m.enqueue(1, b"b", 100).unwrap();
        assert_eq!(m.count_pending(1).unwrap(), 2);
        m.take_oldest(1).unwrap();
        assert_eq!(m.count_pending(1).unwrap(), 1);
    }
}
