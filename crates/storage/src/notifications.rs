//! Notifications store — Web Notifications API persistence.
//!
//! Хранит показанные/dismissed уведомления для:
//! 1. Истории "что показывали" — UI may surface как notification log;
//! 2. De-dup tag-based notifications (`Notification.tag`);
//! 3. Click-tracking (какой URL открылся при клике).
//!
//! Phase 0: storage layer. Реальный prompt-UI и Web Notifications API
//! binding — задачи shell.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: i64,
    pub origin: String,
    pub title: String,
    pub body: String,
    pub icon_url: Option<String>,
    /// `Notification.tag` — для replacement (same tag → replaces).
    /// Pустая строка = no tag.
    pub tag: String,
    /// URL, который откроется при клике (опционально).
    pub click_url: Option<String>,
    pub shown_at: i64,
    pub dismissed_at: Option<i64>,
    pub clicked_at: Option<i64>,
}

pub struct Notifications {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Notifications {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notifications").finish()
    }
}

impl Notifications {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("notifications open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("notifications open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS notifications (
                id           INTEGER PRIMARY KEY,
                origin       TEXT NOT NULL,
                title        TEXT NOT NULL,
                body         TEXT NOT NULL DEFAULT '',
                icon_url     TEXT,
                tag          TEXT NOT NULL DEFAULT '',
                click_url    TEXT,
                shown_at     INTEGER NOT NULL,
                dismissed_at INTEGER,
                clicked_at   INTEGER
            );
            CREATE INDEX IF NOT EXISTS notifications_origin_tag_idx
                ON notifications(origin, tag);
            CREATE INDEX IF NOT EXISTS notifications_shown_at_idx
                ON notifications(shown_at DESC);
            "#,
        )
        .map_err(|e| Error::Storage(format!("notifications init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Показать notification. Если `tag` непустая и для (origin, tag)
    /// уже есть активная notification (не dismissed/clicked) — обновляем
    /// её (заменяем). Иначе — insert.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &self,
        origin: &str,
        title: &str,
        body: &str,
        icon_url: Option<&str>,
        tag: &str,
        click_url: Option<&str>,
        shown_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        // Если tag непустой — ищем существующую активную.
        if !tag.is_empty() {
            let existing: Option<i64> = conn
                .query_row(
                    "SELECT id FROM notifications
                     WHERE origin = ?1 AND tag = ?2
                       AND dismissed_at IS NULL AND clicked_at IS NULL
                     ORDER BY shown_at DESC LIMIT 1",
                    params![origin, tag],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| Error::Storage(format!("notifications find-tag: {e}")))?;
            if let Some(id) = existing {
                // Replace: обновляем содержимое и shown_at.
                conn.execute(
                    "UPDATE notifications SET title = ?1, body = ?2,
                     icon_url = ?3, click_url = ?4, shown_at = ?5
                     WHERE id = ?6",
                    params![title, body, icon_url, click_url, shown_at, id],
                )
                .map_err(|e| Error::Storage(format!("notifications replace: {e}")))?;
                return Ok(id);
            }
        }
        // Insert новой.
        conn.execute(
            "INSERT INTO notifications (origin, title, body, icon_url, tag, click_url, shown_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![origin, title, body, icon_url, tag, click_url, shown_at],
        )
        .map_err(|e| Error::Storage(format!("notifications show: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn mark_dismissed(&self, id: i64, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        conn.execute(
            "UPDATE notifications SET dismissed_at = ?1 WHERE id = ?2 AND dismissed_at IS NULL",
            params![now_unix, id],
        )
        .map_err(|e| Error::Storage(format!("notifications mark_dismissed: {e}")))?;
        Ok(())
    }

    pub fn mark_clicked(&self, id: i64, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        conn.execute(
            "UPDATE notifications SET clicked_at = ?1 WHERE id = ?2 AND clicked_at IS NULL",
            params![now_unix, id],
        )
        .map_err(|e| Error::Storage(format!("notifications mark_clicked: {e}")))?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<Notification>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, origin, title, body, icon_url, tag, click_url,
                    shown_at, dismissed_at, clicked_at
             FROM notifications WHERE id = ?1",
            params![id],
            row_to_notification,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("notifications get: {e}")))
    }

    /// Активные (не dismissed и не clicked) notifications.
    pub fn active(&self, limit: i64) -> Result<Vec<Notification>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, origin, title, body, icon_url, tag, click_url,
                        shown_at, dismissed_at, clicked_at
                 FROM notifications
                 WHERE dismissed_at IS NULL AND clicked_at IS NULL
                 ORDER BY shown_at DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("notifications active prepare: {e}")))?;
        let rows = stmt
            .query_map(params![limit], row_to_notification)
            .map_err(|e| Error::Storage(format!("notifications active query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("notifications row: {e}")))?);
        }
        Ok(out)
    }

    /// История всех показанных notifications (включая закрытые).
    pub fn history(&self, limit: i64) -> Result<Vec<Notification>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, origin, title, body, icon_url, tag, click_url,
                        shown_at, dismissed_at, clicked_at
                 FROM notifications ORDER BY shown_at DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("notifications history prepare: {e}")))?;
        let rows = stmt
            .query_map(params![limit], row_to_notification)
            .map_err(|e| Error::Storage(format!("notifications history query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("notifications row: {e}")))?);
        }
        Ok(out)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        conn.execute("DELETE FROM notifications WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("notifications delete: {e}")))?;
        Ok(())
    }

    pub fn delete_older_than(&self, before: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM notifications WHERE shown_at < ?1",
                params![before],
            )
            .map_err(|e| Error::Storage(format!("notifications delete_older: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notifications mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM notifications", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("notifications count: {e}")))?;
        Ok(n)
    }
}

fn row_to_notification(row: &rusqlite::Row<'_>) -> rusqlite::Result<Notification> {
    Ok(Notification {
        id: row.get(0)?,
        origin: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        icon_url: row.get(4)?,
        tag: row.get(5)?,
        click_url: row.get(6)?,
        shown_at: row.get(7)?,
        dismissed_at: row.get(8)?,
        clicked_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Notifications {
        Notifications::open_in_memory().unwrap()
    }

    #[test]
    fn show_creates_active_notification() {
        let n = make();
        let id = n
            .show("https://x/", "Hello", "World", None, "", None, 100)
            .unwrap();
        let got = n.get(id).unwrap().unwrap();
        assert_eq!(got.title, "Hello");
        assert_eq!(got.body, "World");
        assert_eq!(got.dismissed_at, None);
        assert_eq!(got.clicked_at, None);
    }

    #[test]
    fn show_with_tag_replaces_active() {
        let n = make();
        let id1 = n
            .show("https://x/", "v1", "body1", None, "msg-1", None, 100)
            .unwrap();
        let id2 = n
            .show("https://x/", "v2", "body2", None, "msg-1", None, 200)
            .unwrap();
        // Same id — replaced in place.
        assert_eq!(id1, id2);
        let got = n.get(id1).unwrap().unwrap();
        assert_eq!(got.title, "v2");
        assert_eq!(got.body, "body2");
        assert_eq!(got.shown_at, 200);
    }

    #[test]
    fn show_dismissed_with_tag_creates_new() {
        let n = make();
        let id1 = n
            .show("https://x/", "v1", "body1", None, "msg-1", None, 100)
            .unwrap();
        n.mark_dismissed(id1, 150).unwrap();
        // После dismiss — новый show с тем же tag создаёт новую запись.
        let id2 = n
            .show("https://x/", "v2", "body2", None, "msg-1", None, 200)
            .unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn show_empty_tag_always_creates_new() {
        let n = make();
        let id1 = n.show("https://x/", "a", "", None, "", None, 100).unwrap();
        let id2 = n.show("https://x/", "b", "", None, "", None, 200).unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn mark_dismissed_sets_timestamp() {
        let n = make();
        let id = n.show("https://x/", "t", "", None, "", None, 100).unwrap();
        n.mark_dismissed(id, 200).unwrap();
        assert_eq!(n.get(id).unwrap().unwrap().dismissed_at, Some(200));
    }

    #[test]
    fn mark_clicked_sets_timestamp() {
        let n = make();
        let id = n.show("https://x/", "t", "", None, "", None, 100).unwrap();
        n.mark_clicked(id, 300).unwrap();
        assert_eq!(n.get(id).unwrap().unwrap().clicked_at, Some(300));
    }

    #[test]
    fn active_excludes_dismissed_and_clicked() {
        let n = make();
        let id1 = n.show("https://x/", "a", "", None, "", None, 100).unwrap();
        let id2 = n.show("https://x/", "b", "", None, "", None, 200).unwrap();
        n.show("https://x/", "c", "", None, "", None, 300).unwrap();
        n.mark_dismissed(id1, 150).unwrap();
        n.mark_clicked(id2, 250).unwrap();
        let active = n.active(10).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].title, "c");
    }

    #[test]
    fn history_includes_all() {
        let n = make();
        let id1 = n.show("https://x/", "a", "", None, "", None, 100).unwrap();
        n.show("https://x/", "b", "", None, "", None, 200).unwrap();
        n.mark_dismissed(id1, 150).unwrap();
        let hist = n.history(10).unwrap();
        assert_eq!(hist.len(), 2);
        // DESC by shown_at: b, a.
        assert_eq!(hist[0].title, "b");
    }

    #[test]
    fn click_url_preserved() {
        let n = make();
        let id = n
            .show(
                "https://x/",
                "title",
                "",
                None,
                "",
                Some("https://x/details"),
                100,
            )
            .unwrap();
        assert_eq!(
            n.get(id).unwrap().unwrap().click_url,
            Some("https://x/details".to_string())
        );
    }

    #[test]
    fn icon_url_preserved() {
        let n = make();
        let id = n
            .show(
                "https://x/",
                "title",
                "",
                Some("https://x/icon.png"),
                "",
                None,
                100,
            )
            .unwrap();
        assert_eq!(
            n.get(id).unwrap().unwrap().icon_url,
            Some("https://x/icon.png".to_string())
        );
    }

    #[test]
    fn delete_older_than_removes_old() {
        let n = make();
        n.show("https://x/", "old", "", None, "", None, 100).unwrap();
        n.show("https://x/", "new", "", None, "", None, 1000).unwrap();
        let removed = n.delete_older_than(500).unwrap();
        assert_eq!(removed, 1);
    }

    #[test]
    fn delete_individual() {
        let n = make();
        let id = n.show("https://x/", "t", "", None, "", None, 100).unwrap();
        n.delete(id).unwrap();
        assert!(n.get(id).unwrap().is_none());
    }

    #[test]
    fn cyrillic_content() {
        let n = make();
        let id = n
            .show(
                "https://пример.рф/",
                "Уведомление",
                "Тело сообщения",
                None,
                "",
                None,
                100,
            )
            .unwrap();
        let got = n.get(id).unwrap().unwrap();
        assert_eq!(got.title, "Уведомление");
        assert_eq!(got.body, "Тело сообщения");
    }

    #[test]
    fn count_works() {
        let n = make();
        assert_eq!(n.count().unwrap(), 0);
        n.show("https://x/", "a", "", None, "", None, 100).unwrap();
        n.show("https://x/", "b", "", None, "", None, 200).unwrap();
        assert_eq!(n.count().unwrap(), 2);
    }
}
