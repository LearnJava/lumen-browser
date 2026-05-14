//! Push API subscriptions — VAPID-based push endpoint persistence.
//!
//! Spec: <https://w3c.github.io/push-api/>. Push subscription = handle на
//! browser-side endpoint, через который push-service может доставлять
//! сообщения. Subscription создаётся ServiceWorkerRegistration.pushManager
//! и привязана к (origin + scope) + endpoint от push-service.
//!
//! Phase 0: storage layer. Реальный push runtime (long-poll к push-service,
//! получение / decrypt сообщений, доставка в SW `push`-event) — отдельные
//! задачи Phase 3+. VAPID-keys (p256dh / auth) хранятся как base64-строки;
//! при доставке push-message они нужны для расшифровки.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushSubscription {
    pub id: i64,
    pub origin: String,
    pub scope: String,
    /// URL push-service endpoint-а (от FCM / Mozilla autopush / etc.).
    pub endpoint: String,
    /// Base64 p256dh public-key для расшифровки push-messages.
    pub p256dh: String,
    /// Base64 auth-secret для расшифровки.
    pub auth: String,
    /// `true` если пользователь видит уведомление о каждом push (Push API §5
    /// userVisibleOnly). Phase 0: только этот режим (silent push не поддержан).
    pub user_visible_only: bool,
    pub created_at: i64,
}

pub struct PushSubscriptions {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for PushSubscriptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushSubscriptions").finish()
    }
}

impl PushSubscriptions {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("push_subscriptions open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("push_subscriptions open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS push_subscriptions (
                id                 INTEGER PRIMARY KEY,
                origin             TEXT NOT NULL,
                scope              TEXT NOT NULL,
                endpoint           TEXT NOT NULL,
                p256dh             TEXT NOT NULL,
                auth               TEXT NOT NULL,
                user_visible_only  INTEGER NOT NULL DEFAULT 1,
                created_at         INTEGER NOT NULL,
                UNIQUE (origin, scope)
            );
            CREATE INDEX IF NOT EXISTS push_origin_idx ON push_subscriptions(origin);
            "#,
        )
        .map_err(|e| Error::Storage(format!("push_subscriptions init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn subscribe(
        &self,
        origin: &str,
        scope: &str,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        user_visible_only: bool,
        created_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO push_subscriptions (origin, scope, endpoint, p256dh, auth, user_visible_only, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (origin, scope) DO UPDATE SET
                 endpoint = excluded.endpoint,
                 p256dh = excluded.p256dh,
                 auth = excluded.auth,
                 user_visible_only = excluded.user_visible_only,
                 created_at = excluded.created_at",
            params![
                origin,
                scope,
                endpoint,
                p256dh,
                auth,
                user_visible_only as i32,
                created_at
            ],
        )
        .map_err(|e| Error::Storage(format!("push_subscriptions subscribe: {e}")))?;
        let id: i64 = conn
            .query_row(
                "SELECT id FROM push_subscriptions WHERE origin = ?1 AND scope = ?2",
                params![origin, scope],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("push_subscriptions subscribe-lookup: {e}")))?;
        Ok(id)
    }

    pub fn get(&self, id: i64) -> Result<Option<PushSubscription>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, origin, scope, endpoint, p256dh, auth, user_visible_only, created_at
             FROM push_subscriptions WHERE id = ?1",
            params![id],
            row_to_sub,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("push_subscriptions get: {e}")))
    }

    pub fn get_by_scope(&self, origin: &str, scope: &str) -> Result<Option<PushSubscription>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, origin, scope, endpoint, p256dh, auth, user_visible_only, created_at
             FROM push_subscriptions WHERE origin = ?1 AND scope = ?2",
            params![origin, scope],
            row_to_sub,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("push_subscriptions get_by_scope: {e}")))
    }

    pub fn list_for_origin(&self, origin: &str) -> Result<Vec<PushSubscription>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, origin, scope, endpoint, p256dh, auth, user_visible_only, created_at
                 FROM push_subscriptions WHERE origin = ?1 ORDER BY scope ASC",
            )
            .map_err(|e| Error::Storage(format!("push_subscriptions list prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin], row_to_sub)
            .map_err(|e| Error::Storage(format!("push_subscriptions list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("push_subscriptions row: {e}")))?);
        }
        Ok(out)
    }

    pub fn list_all(&self) -> Result<Vec<PushSubscription>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, origin, scope, endpoint, p256dh, auth, user_visible_only, created_at
                 FROM push_subscriptions ORDER BY created_at ASC",
            )
            .map_err(|e| Error::Storage(format!("push_subscriptions list_all prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_sub)
            .map_err(|e| Error::Storage(format!("push_subscriptions list_all query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("push_subscriptions row: {e}")))?);
        }
        Ok(out)
    }

    pub fn unsubscribe(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM push_subscriptions WHERE id = ?1",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("push_subscriptions unsubscribe: {e}")))?;
        Ok(())
    }

    pub fn unsubscribe_origin(&self, origin: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM push_subscriptions WHERE origin = ?1",
                params![origin],
            )
            .map_err(|e| Error::Storage(format!("push_subscriptions unsubscribe_origin: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("push_subscriptions mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM push_subscriptions", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("push_subscriptions count: {e}")))?;
        Ok(n)
    }
}

fn row_to_sub(row: &rusqlite::Row<'_>) -> rusqlite::Result<PushSubscription> {
    Ok(PushSubscription {
        id: row.get(0)?,
        origin: row.get(1)?,
        scope: row.get(2)?,
        endpoint: row.get(3)?,
        p256dh: row.get(4)?,
        auth: row.get(5)?,
        user_visible_only: row.get::<_, i32>(6)? != 0,
        created_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> PushSubscriptions {
        PushSubscriptions::open_in_memory().unwrap()
    }

    #[test]
    fn subscribe_and_get() {
        let s = make();
        let id = s
            .subscribe(
                "https://app.com",
                "/",
                "https://push.mozilla.com/abc",
                "pubkey-base64",
                "auth-secret-base64",
                true,
                100,
            )
            .unwrap();
        let sub = s.get(id).unwrap().unwrap();
        assert_eq!(sub.endpoint, "https://push.mozilla.com/abc");
        assert_eq!(sub.p256dh, "pubkey-base64");
        assert_eq!(sub.auth, "auth-secret-base64");
        assert!(sub.user_visible_only);
    }

    #[test]
    fn subscribe_same_scope_updates() {
        let s = make();
        let id1 = s.subscribe("https://x/", "/", "ep1", "k1", "a1", true, 100).unwrap();
        let id2 = s.subscribe("https://x/", "/", "ep2", "k2", "a2", true, 200).unwrap();
        assert_eq!(id1, id2);
        let sub = s.get(id1).unwrap().unwrap();
        assert_eq!(sub.endpoint, "ep2");
        assert_eq!(sub.p256dh, "k2");
    }

    #[test]
    fn get_by_scope() {
        let s = make();
        s.subscribe("https://x/", "/", "ep", "k", "a", true, 100).unwrap();
        s.subscribe("https://x/", "/app/", "ep2", "k2", "a2", true, 200).unwrap();
        let sub = s.get_by_scope("https://x/", "/app/").unwrap().unwrap();
        assert_eq!(sub.endpoint, "ep2");
    }

    #[test]
    fn list_for_origin() {
        let s = make();
        s.subscribe("https://x/", "/", "ep1", "k", "a", true, 100).unwrap();
        s.subscribe("https://x/", "/app/", "ep2", "k", "a", true, 200).unwrap();
        s.subscribe("https://y/", "/", "ep3", "k", "a", true, 300).unwrap();
        let list = s.list_for_origin("https://x/").unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn unsubscribe_works() {
        let s = make();
        let id = s.subscribe("https://x/", "/", "ep", "k", "a", true, 100).unwrap();
        s.unsubscribe(id).unwrap();
        assert!(s.get(id).unwrap().is_none());
    }

    #[test]
    fn unsubscribe_origin_removes_all_scopes() {
        let s = make();
        s.subscribe("https://x/", "/", "ep1", "k", "a", true, 100).unwrap();
        s.subscribe("https://x/", "/app/", "ep2", "k", "a", true, 200).unwrap();
        s.subscribe("https://y/", "/", "ep3", "k", "a", true, 300).unwrap();
        let n = s.unsubscribe_origin("https://x/").unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.count().unwrap(), 1);
    }

    #[test]
    fn silent_push_user_visible_only_false() {
        let s = make();
        let id = s.subscribe("https://x/", "/", "ep", "k", "a", false, 100).unwrap();
        assert!(!s.get(id).unwrap().unwrap().user_visible_only);
    }

    #[test]
    fn list_all_ordered_by_creation() {
        let s = make();
        s.subscribe("https://c/", "/", "ep3", "k", "a", true, 300).unwrap();
        s.subscribe("https://a/", "/", "ep1", "k", "a", true, 100).unwrap();
        s.subscribe("https://b/", "/", "ep2", "k", "a", true, 200).unwrap();
        let list = s.list_all().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].endpoint, "ep1");
        assert_eq!(list[2].endpoint, "ep3");
    }

    #[test]
    fn count_works() {
        let s = make();
        assert_eq!(s.count().unwrap(), 0);
        s.subscribe("https://a/", "/", "ep", "k", "a", true, 100).unwrap();
        s.subscribe("https://b/", "/", "ep", "k", "a", true, 200).unwrap();
        assert_eq!(s.count().unwrap(), 2);
    }
}
