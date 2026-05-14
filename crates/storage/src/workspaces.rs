//! Tab workspaces — группы вкладок с собственным контекстом (cookies
//! опционально, своя позиция в UI, цветовая маркировка).
//!
//! §8.3 плана: «Workspaces — наборы вкладок, переключение Ctrl+1..9.
//! Каждый — со своим контекстом cookies (опционально).»
//!
//! Phase 0: storage layer для workspace metadata. Cookie-isolation
//! интеграция и UI — отдельные задачи (UI работает с `tab-session-export`
//! веткой другой сессии). Здесь только базовая schema.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    /// Цвет для UI-маркировки (CSS-color строкой: `#RRGGBB` или name).
    pub color: String,
    /// Иконка (emoji, font-icon-name, или URL — UI решает).
    pub icon: String,
    /// Опциональный isolated cookie/storage namespace. None = shared с default.
    pub cookie_partition: Option<String>,
    pub created_at: i64,
    /// Порядок отображения (для UI). Меньше = раньше в списке.
    pub position: i64,
}

pub struct Workspaces {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Workspaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspaces").finish()
    }
}

impl Workspaces {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("workspaces open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("workspaces open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS workspaces (
                id              INTEGER PRIMARY KEY,
                name            TEXT NOT NULL UNIQUE,
                color           TEXT NOT NULL DEFAULT '',
                icon            TEXT NOT NULL DEFAULT '',
                cookie_partition TEXT,
                created_at      INTEGER NOT NULL,
                position        INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS workspaces_position_idx
                ON workspaces(position ASC);
            "#,
        )
        .map_err(|e| Error::Storage(format!("workspaces init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Создать workspace. Position автоматически = MAX(existing)+1.
    /// Возвращает id.
    pub fn create(
        &self,
        name: &str,
        color: &str,
        icon: &str,
        cookie_partition: Option<&str>,
        created_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        let next_pos: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM workspaces",
                [],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("workspaces next_pos: {e}")))?;
        conn.execute(
            "INSERT INTO workspaces (name, color, icon, cookie_partition, created_at, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![name, color, icon, cookie_partition, created_at, next_pos],
        )
        .map_err(|e| Error::Storage(format!("workspaces create: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Option<Workspace>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, name, color, icon, cookie_partition, created_at, position
             FROM workspaces WHERE id = ?1",
            params![id],
            row_to_workspace,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("workspaces get: {e}")))
    }

    pub fn get_by_name(&self, name: &str) -> Result<Option<Workspace>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, name, color, icon, cookie_partition, created_at, position
             FROM workspaces WHERE name = ?1",
            params![name],
            row_to_workspace,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("workspaces get_by_name: {e}")))
    }

    /// Все workspace-ы в порядке position ASC.
    pub fn list_all(&self) -> Result<Vec<Workspace>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, name, color, icon, cookie_partition, created_at, position
                 FROM workspaces ORDER BY position ASC",
            )
            .map_err(|e| Error::Storage(format!("workspaces list prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_workspace)
            .map_err(|e| Error::Storage(format!("workspaces list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("workspaces row: {e}")))?);
        }
        Ok(out)
    }

    pub fn rename(&self, id: i64, new_name: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        conn.execute(
            "UPDATE workspaces SET name = ?1 WHERE id = ?2",
            params![new_name, id],
        )
        .map_err(|e| Error::Storage(format!("workspaces rename: {e}")))?;
        Ok(())
    }

    pub fn set_color(&self, id: i64, color: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        conn.execute(
            "UPDATE workspaces SET color = ?1 WHERE id = ?2",
            params![color, id],
        )
        .map_err(|e| Error::Storage(format!("workspaces set_color: {e}")))?;
        Ok(())
    }

    pub fn set_icon(&self, id: i64, icon: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        conn.execute(
            "UPDATE workspaces SET icon = ?1 WHERE id = ?2",
            params![icon, id],
        )
        .map_err(|e| Error::Storage(format!("workspaces set_icon: {e}")))?;
        Ok(())
    }

    pub fn set_position(&self, id: i64, position: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        conn.execute(
            "UPDATE workspaces SET position = ?1 WHERE id = ?2",
            params![position, id],
        )
        .map_err(|e| Error::Storage(format!("workspaces set_position: {e}")))?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("workspaces delete: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("workspaces mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("workspaces count: {e}")))?;
        Ok(n)
    }
}

fn row_to_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        icon: row.get(3)?,
        cookie_partition: row.get(4)?,
        created_at: row.get(5)?,
        position: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Workspaces {
        Workspaces::open_in_memory().unwrap()
    }

    #[test]
    fn create_basic_workspace() {
        let w = make();
        let id = w.create("Work", "#0066CC", "💼", None, 100).unwrap();
        let ws = w.get(id).unwrap().unwrap();
        assert_eq!(ws.name, "Work");
        assert_eq!(ws.color, "#0066CC");
        assert_eq!(ws.icon, "💼");
        assert_eq!(ws.cookie_partition, None);
        assert_eq!(ws.position, 0);
    }

    #[test]
    fn create_with_cookie_partition() {
        let w = make();
        let id = w
            .create("Private", "red", "🔒", Some("private-ns"), 100)
            .unwrap();
        let ws = w.get(id).unwrap().unwrap();
        assert_eq!(ws.cookie_partition, Some("private-ns".into()));
    }

    #[test]
    fn duplicate_name_fails() {
        let w = make();
        w.create("X", "", "", None, 100).unwrap();
        assert!(w.create("X", "", "", None, 200).is_err());
    }

    #[test]
    fn position_auto_increments() {
        let w = make();
        let id1 = w.create("A", "", "", None, 100).unwrap();
        let id2 = w.create("B", "", "", None, 200).unwrap();
        let id3 = w.create("C", "", "", None, 300).unwrap();
        assert_eq!(w.get(id1).unwrap().unwrap().position, 0);
        assert_eq!(w.get(id2).unwrap().unwrap().position, 1);
        assert_eq!(w.get(id3).unwrap().unwrap().position, 2);
    }

    #[test]
    fn list_all_ordered_by_position() {
        let w = make();
        w.create("Last", "", "", None, 300).unwrap();   // pos 0
        w.create("Mid", "", "", None, 100).unwrap();    // pos 1
        let id_first = w.create("First", "", "", None, 50).unwrap(); // pos 2
        // Переставим First на pos -1 (раньше всех).
        w.set_position(id_first, -1).unwrap();
        let all = w.list_all().unwrap();
        let names: Vec<&str> = all.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["First", "Last", "Mid"]);
    }

    #[test]
    fn rename_works() {
        let w = make();
        let id = w.create("Old", "", "", None, 100).unwrap();
        w.rename(id, "New").unwrap();
        assert!(w.get_by_name("New").unwrap().is_some());
        assert!(w.get_by_name("Old").unwrap().is_none());
    }

    #[test]
    fn set_color_and_icon() {
        let w = make();
        let id = w.create("X", "", "", None, 100).unwrap();
        w.set_color(id, "#FF0000").unwrap();
        w.set_icon(id, "🔥").unwrap();
        let ws = w.get(id).unwrap().unwrap();
        assert_eq!(ws.color, "#FF0000");
        assert_eq!(ws.icon, "🔥");
    }

    #[test]
    fn delete_removes() {
        let w = make();
        let id = w.create("X", "", "", None, 100).unwrap();
        w.delete(id).unwrap();
        assert!(w.get(id).unwrap().is_none());
    }

    #[test]
    fn cyrillic_workspace_name() {
        let w = make();
        let id = w.create("Личное", "blue", "🏠", None, 100).unwrap();
        let ws = w.get(id).unwrap().unwrap();
        assert_eq!(ws.name, "Личное");
    }

    #[test]
    fn count_workspaces() {
        let w = make();
        assert_eq!(w.count().unwrap(), 0);
        w.create("A", "", "", None, 100).unwrap();
        w.create("B", "", "", None, 200).unwrap();
        assert_eq!(w.count().unwrap(), 2);
    }

    #[test]
    fn get_missing_returns_none() {
        let w = make();
        assert!(w.get(999).unwrap().is_none());
        assert!(w.get_by_name("nope").unwrap().is_none());
    }
}
