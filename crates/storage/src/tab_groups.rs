//! Tab groups persistence (CC-6) — SQLite-backed metadata for named,
//! colour-coded collections of tabs.
//!
//! Stores only the *group* presentation state (label, colour index, collapsed
//! flag, display position). Membership (which tab belongs to which group) is
//! session-scoped UI state owned by the shell tab strip and is not persisted
//! here — only the groups themselves survive a restart so their labels and
//! colours can be restored.
//!
//! Schema:
//! ```sql
//! CREATE TABLE tab_groups (
//!     id          INTEGER PRIMARY KEY,
//!     label       TEXT NOT NULL DEFAULT '',
//!     color       INTEGER NOT NULL DEFAULT 0,  -- GroupColor palette index 0..8
//!     collapsed   INTEGER NOT NULL DEFAULT 0,  -- 0 = expanded, 1 = collapsed
//!     position    INTEGER NOT NULL DEFAULT 0,  -- display order, ascending
//!     created_at  INTEGER NOT NULL             -- Unix timestamp
//! );
//! ```

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// One persisted tab group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedGroup {
    /// Database id (matches the in-memory group id when restored).
    pub id: i64,
    /// User-visible label (may be empty).
    pub label: String,
    /// Colour palette index (`0..8`); maps to `shell::tabs::groups::GroupColor`.
    pub color: u8,
    /// `true` if the group is collapsed.
    pub collapsed: bool,
    /// Display order; smaller sorts first.
    pub position: i64,
    /// Unix timestamp of creation.
    pub created_at: i64,
}

/// SQLite-backed store of tab-group metadata.
pub struct TabGroups {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for TabGroups {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabGroups").finish()
    }
}

impl TabGroups {
    /// Open (or create) the store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("tab_groups open: {e}")))?;
        Self::init(conn)
    }

    /// Open an ephemeral in-memory store (tests / private sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("tab_groups open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS tab_groups (
                id         INTEGER PRIMARY KEY,
                label      TEXT NOT NULL DEFAULT '',
                color      INTEGER NOT NULL DEFAULT 0,
                collapsed  INTEGER NOT NULL DEFAULT 0,
                position   INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS tab_groups_position_idx
                ON tab_groups(position ASC);
            "#,
        )
        .map_err(|e| Error::Storage(format!("tab_groups init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Create a group. `position` is auto-assigned as `MAX(existing) + 1`.
    /// Returns the new row id.
    pub fn create(&self, label: &str, color: u8, created_at: i64) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_groups mutex poisoned".into()))?;
        let next_pos: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM tab_groups",
                [],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("tab_groups next_pos: {e}")))?;
        conn.execute(
            "INSERT INTO tab_groups (label, color, collapsed, position, created_at)
             VALUES (?1, ?2, 0, ?3, ?4)",
            params![label, color as i64, next_pos, created_at],
        )
        .map_err(|e| Error::Storage(format!("tab_groups create: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Fetch a group by id. `None` if absent.
    pub fn get(&self, id: i64) -> Result<Option<PersistedGroup>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_groups mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, label, color, collapsed, position, created_at
             FROM tab_groups WHERE id = ?1",
            params![id],
            row_to_group,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("tab_groups get: {e}")))
    }

    /// All groups, ordered by `position` ascending.
    pub fn list_all(&self) -> Result<Vec<PersistedGroup>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_groups mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, label, color, collapsed, position, created_at
                 FROM tab_groups ORDER BY position ASC",
            )
            .map_err(|e| Error::Storage(format!("tab_groups list prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_group)
            .map_err(|e| Error::Storage(format!("tab_groups list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("tab_groups row: {e}")))?);
        }
        Ok(out)
    }

    /// Rename a group. Missing id is a no-op.
    pub fn rename(&self, id: i64, label: &str) -> Result<()> {
        self.update_col("label", id, |conn| {
            conn.execute(
                "UPDATE tab_groups SET label = ?1 WHERE id = ?2",
                params![label, id],
            )
        })
    }

    /// Change a group's colour palette index. Missing id is a no-op.
    pub fn set_color(&self, id: i64, color: u8) -> Result<()> {
        self.update_col("set_color", id, |conn| {
            conn.execute(
                "UPDATE tab_groups SET color = ?1 WHERE id = ?2",
                params![color as i64, id],
            )
        })
    }

    /// Set the collapsed flag. Missing id is a no-op.
    pub fn set_collapsed(&self, id: i64, collapsed: bool) -> Result<()> {
        self.update_col("set_collapsed", id, |conn| {
            conn.execute(
                "UPDATE tab_groups SET collapsed = ?1 WHERE id = ?2",
                params![i64::from(collapsed), id],
            )
        })
    }

    /// Set the display position. Missing id is a no-op.
    pub fn set_position(&self, id: i64, position: i64) -> Result<()> {
        self.update_col("set_position", id, |conn| {
            conn.execute(
                "UPDATE tab_groups SET position = ?1 WHERE id = ?2",
                params![position, id],
            )
        })
    }

    /// Delete a group. Missing id is a no-op.
    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_groups mutex poisoned".into()))?;
        conn.execute("DELETE FROM tab_groups WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("tab_groups delete: {e}")))?;
        Ok(())
    }

    /// Number of stored groups.
    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_groups mutex poisoned".into()))?;
        conn.query_row("SELECT COUNT(*) FROM tab_groups", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("tab_groups count: {e}")))
    }

    /// Shared helper for single-column `UPDATE`s.
    fn update_col(
        &self,
        what: &str,
        _id: i64,
        f: impl FnOnce(&Connection) -> rusqlite::Result<usize>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("tab_groups mutex poisoned".into()))?;
        f(&conn).map_err(|e| Error::Storage(format!("tab_groups {what}: {e}")))?;
        Ok(())
    }
}

fn row_to_group(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedGroup> {
    let color: i64 = row.get(2)?;
    let collapsed: i64 = row.get(3)?;
    Ok(PersistedGroup {
        id: row.get(0)?,
        label: row.get(1)?,
        color: color.clamp(0, 255) as u8,
        collapsed: collapsed != 0,
        position: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> TabGroups {
        TabGroups::open_in_memory().unwrap()
    }

    #[test]
    fn create_basic_group() {
        let g = make();
        let id = g.create("Research", 6, 100).unwrap();
        let row = g.get(id).unwrap().unwrap();
        assert_eq!(row.label, "Research");
        assert_eq!(row.color, 6);
        assert!(!row.collapsed);
        assert_eq!(row.position, 0);
        assert_eq!(row.created_at, 100);
    }

    #[test]
    fn position_auto_increments() {
        let g = make();
        let a = g.create("A", 0, 100).unwrap();
        let b = g.create("B", 1, 200).unwrap();
        let c = g.create("C", 2, 300).unwrap();
        assert_eq!(g.get(a).unwrap().unwrap().position, 0);
        assert_eq!(g.get(b).unwrap().unwrap().position, 1);
        assert_eq!(g.get(c).unwrap().unwrap().position, 2);
    }

    #[test]
    fn rename_changes_label() {
        let g = make();
        let id = g.create("Old", 0, 100).unwrap();
        g.rename(id, "New").unwrap();
        assert_eq!(g.get(id).unwrap().unwrap().label, "New");
    }

    #[test]
    fn set_color_persists() {
        let g = make();
        let id = g.create("X", 0, 100).unwrap();
        g.set_color(id, 4).unwrap();
        assert_eq!(g.get(id).unwrap().unwrap().color, 4);
    }

    #[test]
    fn set_collapsed_round_trips() {
        let g = make();
        let id = g.create("X", 0, 100).unwrap();
        g.set_collapsed(id, true).unwrap();
        assert!(g.get(id).unwrap().unwrap().collapsed);
        g.set_collapsed(id, false).unwrap();
        assert!(!g.get(id).unwrap().unwrap().collapsed);
    }

    #[test]
    fn list_all_ordered_by_position() {
        let g = make();
        let last = g.create("Last", 0, 100).unwrap();
        g.create("Mid", 0, 200).unwrap();
        let first = g.create("First", 0, 300).unwrap();
        g.set_position(first, -1).unwrap();
        g.set_position(last, 5).unwrap();
        let all = g.list_all().unwrap();
        let labels: Vec<&str> = all.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, vec!["First", "Mid", "Last"]);
    }

    #[test]
    fn delete_removes_group() {
        let g = make();
        let id = g.create("X", 0, 100).unwrap();
        g.delete(id).unwrap();
        assert!(g.get(id).unwrap().is_none());
    }

    #[test]
    fn count_reflects_inserts() {
        let g = make();
        assert_eq!(g.count().unwrap(), 0);
        g.create("A", 0, 100).unwrap();
        g.create("B", 0, 200).unwrap();
        assert_eq!(g.count().unwrap(), 2);
    }

    #[test]
    fn cyrillic_label_round_trips() {
        let g = make();
        let id = g.create("Исследование", 5, 100).unwrap();
        assert_eq!(g.get(id).unwrap().unwrap().label, "Исследование");
    }

    #[test]
    fn missing_id_updates_are_noops() {
        let g = make();
        g.rename(999, "x").unwrap();
        g.set_color(999, 3).unwrap();
        g.set_collapsed(999, true).unwrap();
        g.set_position(999, 1).unwrap();
        g.delete(999).unwrap();
        assert_eq!(g.count().unwrap(), 0);
    }

    #[test]
    fn get_missing_returns_none() {
        let g = make();
        assert!(g.get(42).unwrap().is_none());
    }
}
