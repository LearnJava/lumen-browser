//! Tab session metadata — persistent state открытых вкладок для
//! восстановления после рестарта браузера. §12.7 плана.
//!
//! Phase 0 покрывает: schema + CRUD для одиночных вкладок и сессий
//! (snapshot = набор вкладок на момент времени). Form-values, scroll
//! и parent-tab — поля в схеме; реальная их синхронизация при изменениях
//! в UI и export/import в JSON/TOML — отдельные задачи. Эта ветка
//! НЕ пересекается с зарезервированной `tab-session-export` (другая
//! сессия): здесь — storage layer, там — export-format и shell-integration.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Одна вкладка в сохранённой сессии.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSession {
    pub id: i64,
    /// Сессия-владелец (например, последняя сессия перед закрытием).
    pub session_id: i64,
    pub url: String,
    pub title: String,
    /// Scroll-позиция в пикселях.
    pub scroll_y: i64,
    /// Form-values сериализованные (JSON-строка с {input_name: value}).
    pub form_values: String,
    /// Parent-tab (если вкладка открыта из другой через Ctrl+click и т.д.).
    pub parent_tab_id: Option<i64>,
    /// Workspace, к которому привязана вкладка.
    pub workspace_id: Option<i64>,
    /// Активная вкладка в своём workspace?
    pub is_active: bool,
    pub created_at: i64,
}

/// Снимок сессии — корневая запись для group of tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
}

pub struct TabSessions {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for TabSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabSessions").finish()
    }
}

impl TabSessions {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("tab_sessions open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("tab_sessions open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS session_snapshots (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL,
                created_at  INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tab_sessions (
                id            INTEGER PRIMARY KEY,
                session_id    INTEGER NOT NULL,
                url           TEXT NOT NULL,
                title         TEXT NOT NULL DEFAULT '',
                scroll_y      INTEGER NOT NULL DEFAULT 0,
                form_values   TEXT NOT NULL DEFAULT '{}',
                parent_tab_id INTEGER,
                workspace_id  INTEGER,
                is_active     INTEGER NOT NULL DEFAULT 0,
                created_at    INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES session_snapshots(id)
                    ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS tab_sessions_session_idx
                ON tab_sessions(session_id);
            CREATE INDEX IF NOT EXISTS tab_sessions_workspace_idx
                ON tab_sessions(workspace_id);
            "#,
        )
        .map_err(|e| Error::Storage(format!("tab_sessions init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Создать новый snapshot сессии. Возвращает session_id.
    pub fn create_snapshot(&self, name: &str, created_at: i64) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO session_snapshots (name, created_at) VALUES (?1, ?2)",
            params![name, created_at],
        )
        .map_err(|e| Error::Storage(format!("tab_sessions create_snapshot: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Добавить вкладку в указанный snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn add_tab(
        &self,
        session_id: i64,
        url: &str,
        title: &str,
        scroll_y: i64,
        form_values: &str,
        parent_tab_id: Option<i64>,
        workspace_id: Option<i64>,
        is_active: bool,
        created_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO tab_sessions
             (session_id, url, title, scroll_y, form_values, parent_tab_id,
              workspace_id, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id,
                url,
                title,
                scroll_y,
                form_values,
                parent_tab_id,
                workspace_id,
                is_active as i32,
                created_at,
            ],
        )
        .map_err(|e| Error::Storage(format!("tab_sessions add_tab: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Обновить scroll-позицию (часто меняется).
    pub fn update_scroll(&self, tab_id: i64, scroll_y: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        conn.execute(
            "UPDATE tab_sessions SET scroll_y = ?1 WHERE id = ?2",
            params![scroll_y, tab_id],
        )
        .map_err(|e| Error::Storage(format!("tab_sessions update_scroll: {e}")))?;
        Ok(())
    }

    /// Обновить form-values (JSON-строка).
    pub fn update_form_values(&self, tab_id: i64, form_values: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        conn.execute(
            "UPDATE tab_sessions SET form_values = ?1 WHERE id = ?2",
            params![form_values, tab_id],
        )
        .map_err(|e| Error::Storage(format!("tab_sessions update_form_values: {e}")))?;
        Ok(())
    }

    pub fn get_snapshot(&self, session_id: i64) -> Result<Option<SessionSnapshot>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, name, created_at FROM session_snapshots WHERE id = ?1",
            params![session_id],
            |r| {
                Ok(SessionSnapshot {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created_at: r.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| Error::Storage(format!("tab_sessions get_snapshot: {e}")))
    }

    /// Все snapshot-ы сессий в порядке created_at DESC (последний — первый).
    pub fn list_snapshots(&self) -> Result<Vec<SessionSnapshot>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, name, created_at FROM session_snapshots
                 ORDER BY created_at DESC",
            )
            .map_err(|e| Error::Storage(format!("tab_sessions list_snapshots prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SessionSnapshot {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created_at: r.get(2)?,
                })
            })
            .map_err(|e| Error::Storage(format!("tab_sessions list_snapshots query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("tab_sessions row: {e}")))?);
        }
        Ok(out)
    }

    /// Все вкладки в snapshot-е.
    pub fn list_tabs(&self, session_id: i64) -> Result<Vec<TabSession>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, session_id, url, title, scroll_y, form_values,
                        parent_tab_id, workspace_id, is_active, created_at
                 FROM tab_sessions WHERE session_id = ?1
                 ORDER BY id ASC",
            )
            .map_err(|e| Error::Storage(format!("tab_sessions list_tabs prepare: {e}")))?;
        let rows = stmt
            .query_map(params![session_id], row_to_tab)
            .map_err(|e| Error::Storage(format!("tab_sessions list_tabs query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("tab_sessions row: {e}")))?);
        }
        Ok(out)
    }

    /// Удалить snapshot (cascade удаляет все его вкладки через FK).
    pub fn delete_snapshot(&self, session_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM session_snapshots WHERE id = ?1",
            params![session_id],
        )
        .map_err(|e| Error::Storage(format!("tab_sessions delete_snapshot: {e}")))?;
        Ok(())
    }

    /// Удалить одну вкладку.
    pub fn delete_tab(&self, tab_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        conn.execute("DELETE FROM tab_sessions WHERE id = ?1", params![tab_id])
            .map_err(|e| Error::Storage(format!("tab_sessions delete_tab: {e}")))?;
        Ok(())
    }

    /// Число snapshot-ов.
    pub fn snapshot_count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_sessions mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_snapshots", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("tab_sessions snapshot_count: {e}")))?;
        Ok(n)
    }
}

fn row_to_tab(row: &rusqlite::Row<'_>) -> rusqlite::Result<TabSession> {
    Ok(TabSession {
        id: row.get(0)?,
        session_id: row.get(1)?,
        url: row.get(2)?,
        title: row.get(3)?,
        scroll_y: row.get(4)?,
        form_values: row.get(5)?,
        parent_tab_id: row.get(6)?,
        workspace_id: row.get(7)?,
        is_active: row.get::<_, i32>(8)? != 0,
        created_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> TabSessions {
        TabSessions::open_in_memory().unwrap()
    }

    #[test]
    fn create_snapshot_and_add_tab() {
        let t = make();
        let sid = t.create_snapshot("last-session", 100).unwrap();
        let tid = t
            .add_tab(
                sid,
                "https://example.com/",
                "Example",
                500,
                r#"{"q":"rust"}"#,
                None,
                None,
                true,
                100,
            )
            .unwrap();
        let tabs = t.list_tabs(sid).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].id, tid);
        assert_eq!(tabs[0].url, "https://example.com/");
        assert_eq!(tabs[0].scroll_y, 500);
        assert_eq!(tabs[0].form_values, r#"{"q":"rust"}"#);
        assert!(tabs[0].is_active);
    }

    #[test]
    fn list_snapshots_desc_by_created_at() {
        let t = make();
        t.create_snapshot("old", 100).unwrap();
        t.create_snapshot("new", 300).unwrap();
        t.create_snapshot("mid", 200).unwrap();
        let snaps = t.list_snapshots().unwrap();
        let names: Vec<&str> = snaps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["new", "mid", "old"]);
    }

    #[test]
    fn update_scroll_persists() {
        let t = make();
        let sid = t.create_snapshot("x", 100).unwrap();
        let tid = t.add_tab(sid, "https://x/", "x", 0, "{}", None, None, false, 100).unwrap();
        t.update_scroll(tid, 1200).unwrap();
        assert_eq!(t.list_tabs(sid).unwrap()[0].scroll_y, 1200);
    }

    #[test]
    fn update_form_values_persists() {
        let t = make();
        let sid = t.create_snapshot("x", 100).unwrap();
        let tid = t.add_tab(sid, "https://x/", "x", 0, "{}", None, None, false, 100).unwrap();
        t.update_form_values(tid, r#"{"email":"a@b.com"}"#).unwrap();
        assert_eq!(t.list_tabs(sid).unwrap()[0].form_values, r#"{"email":"a@b.com"}"#);
    }

    #[test]
    fn delete_snapshot_cascades_tabs() {
        let t = make();
        let sid = t.create_snapshot("x", 100).unwrap();
        t.add_tab(sid, "https://a/", "a", 0, "{}", None, None, false, 100).unwrap();
        t.add_tab(sid, "https://b/", "b", 0, "{}", None, None, false, 100).unwrap();
        t.delete_snapshot(sid).unwrap();
        // Tabs тоже удалились через CASCADE.
        assert!(t.list_tabs(sid).unwrap().is_empty());
        assert!(t.get_snapshot(sid).unwrap().is_none());
    }

    #[test]
    fn parent_tab_relationship() {
        let t = make();
        let sid = t.create_snapshot("x", 100).unwrap();
        let parent_tid = t.add_tab(sid, "https://p/", "p", 0, "{}", None, None, true, 100).unwrap();
        let child_tid = t.add_tab(sid, "https://c/", "c", 0, "{}", Some(parent_tid), None, false, 100).unwrap();
        let tabs = t.list_tabs(sid).unwrap();
        let child = tabs.iter().find(|x| x.id == child_tid).unwrap();
        assert_eq!(child.parent_tab_id, Some(parent_tid));
    }

    #[test]
    fn workspace_assignment() {
        let t = make();
        let sid = t.create_snapshot("x", 100).unwrap();
        let tid = t.add_tab(sid, "https://x/", "x", 0, "{}", None, Some(42), false, 100).unwrap();
        assert_eq!(t.list_tabs(sid).unwrap()[0].workspace_id, Some(42));
        let _ = tid;
    }

    #[test]
    fn delete_individual_tab() {
        let t = make();
        let sid = t.create_snapshot("x", 100).unwrap();
        let tid = t.add_tab(sid, "https://x/", "x", 0, "{}", None, None, false, 100).unwrap();
        t.delete_tab(tid).unwrap();
        assert!(t.list_tabs(sid).unwrap().is_empty());
    }

    #[test]
    fn cyrillic_url_and_title() {
        let t = make();
        let sid = t.create_snapshot("сессия", 100).unwrap();
        t.add_tab(
            sid,
            "https://пример.рф/",
            "Главная страница",
            0,
            "{}",
            None,
            None,
            true,
            100,
        )
        .unwrap();
        let tabs = t.list_tabs(sid).unwrap();
        assert_eq!(tabs[0].url, "https://пример.рф/");
        assert_eq!(tabs[0].title, "Главная страница");
    }

    #[test]
    fn snapshot_count_works() {
        let t = make();
        assert_eq!(t.snapshot_count().unwrap(), 0);
        t.create_snapshot("a", 100).unwrap();
        t.create_snapshot("b", 200).unwrap();
        assert_eq!(t.snapshot_count().unwrap(), 2);
    }

    #[test]
    fn list_tabs_empty_for_unknown_session() {
        let t = make();
        assert!(t.list_tabs(999).unwrap().is_empty());
    }

    #[test]
    fn list_tabs_preserves_order_by_id() {
        let t = make();
        let sid = t.create_snapshot("x", 100).unwrap();
        t.add_tab(sid, "https://1/", "1", 0, "{}", None, None, false, 100).unwrap();
        t.add_tab(sid, "https://2/", "2", 0, "{}", None, None, false, 200).unwrap();
        t.add_tab(sid, "https://3/", "3", 0, "{}", None, None, false, 300).unwrap();
        let tabs = t.list_tabs(sid).unwrap();
        let titles: Vec<&str> = tabs.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles, vec!["1", "2", "3"]);
    }
}
