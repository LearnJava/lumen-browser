//! Persistent keyboard shortcut overrides backed by SQLite (D-4).
//!
//! Stores user-defined keybinding overrides in a table
//! `keyboard_shortcuts(command, modifier, key)`.  The shell loads all rows at
//! startup and merges them with the compile-time defaults in `keybinding_for`.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

/// A single keybinding: a command name paired with its modifier + key strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardShortcutEntry {
    /// Shell command name, matching the `KeyCommand` variant name (e.g. `"Reload"`).
    pub command: String,
    /// Modifier string: `"ctrl"`, `"ctrl+shift"`, `"ctrl+alt"`, `"alt"`,
    /// `"shift"`, or `""` for no modifier.
    pub modifier: String,
    /// Key name as produced by `winit::keyboard::KeyCode` display (e.g. `"R"`,
    /// `"F5"`, `"Escape"`, `"Comma"`).
    pub key: String,
}

/// Persistent store for keyboard shortcut overrides.
pub struct KeyboardShortcuts {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for KeyboardShortcuts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyboardShortcuts").finish_non_exhaustive()
    }
}

impl KeyboardShortcuts {
    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS keyboard_shortcuts (
                command  TEXT PRIMARY KEY NOT NULL,
                modifier TEXT NOT NULL,
                key      TEXT NOT NULL
            );",
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Open (or create) an on-disk shortcuts database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| Error::Storage(e.to_string()))?;
        Self::init(conn)
    }

    /// Create an in-memory shortcuts database (for tests / ephemeral sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| Error::Storage(e.to_string()))?;
        Self::init(conn)
    }

    /// Return all stored overrides.
    pub fn all(&self) -> Vec<KeyboardShortcutEntry> {
        let conn = self.conn.lock().expect("shortcuts lock");
        let mut stmt = match conn
            .prepare("SELECT command, modifier, key FROM keyboard_shortcuts ORDER BY command")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| {
            Ok(KeyboardShortcutEntry {
                command: row.get(0)?,
                modifier: row.get(1)?,
                key: row.get(2)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Return the stored override for `command`, or `None` if using default.
    pub fn get(&self, command: &str) -> Option<KeyboardShortcutEntry> {
        let conn = self.conn.lock().expect("shortcuts lock");
        conn.query_row(
            "SELECT command, modifier, key FROM keyboard_shortcuts WHERE command = ?1",
            params![command],
            |row| {
                Ok(KeyboardShortcutEntry {
                    command: row.get(0)?,
                    modifier: row.get(1)?,
                    key: row.get(2)?,
                })
            },
        )
        .ok()
    }

    /// Save (or overwrite) a binding override for `command`.
    pub fn set(&self, command: &str, modifier: &str, key: &str) -> Result<()> {
        let conn = self.conn.lock().expect("shortcuts lock");
        conn.execute(
            "INSERT INTO keyboard_shortcuts (command, modifier, key) VALUES (?1, ?2, ?3)
             ON CONFLICT(command) DO UPDATE SET modifier = excluded.modifier,
                                                key      = excluded.key",
            params![command, modifier, key],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    /// Remove the override for `command` (reverts to compile-time default).
    pub fn remove(&self, command: &str) -> Result<()> {
        let conn = self.conn.lock().expect("shortcuts lock");
        conn.execute(
            "DELETE FROM keyboard_shortcuts WHERE command = ?1",
            params![command],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> KeyboardShortcuts {
        KeyboardShortcuts::open_in_memory().unwrap()
    }

    #[test]
    fn open_in_memory_succeeds() {
        db();
    }

    #[test]
    fn all_empty_initially() {
        assert!(db().all().is_empty());
    }

    #[test]
    fn set_and_get_binding() {
        let db = db();
        db.set("Reload", "ctrl", "R").unwrap();
        let e = db.get("Reload").unwrap();
        assert_eq!(e.command, "Reload");
        assert_eq!(e.modifier, "ctrl");
        assert_eq!(e.key, "R");
    }

    #[test]
    fn set_overwrites_existing() {
        let db = db();
        db.set("Reload", "ctrl", "R").unwrap();
        db.set("Reload", "", "F5").unwrap();
        let e = db.get("Reload").unwrap();
        assert_eq!(e.modifier, "");
        assert_eq!(e.key, "F5");
    }

    #[test]
    fn all_returns_all_entries() {
        let db = db();
        db.set("Reload", "ctrl", "R").unwrap();
        db.set("NewTab", "ctrl", "T").unwrap();
        let all = db.all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn remove_deletes_binding() {
        let db = db();
        db.set("Reload", "ctrl", "R").unwrap();
        db.remove("Reload").unwrap();
        assert!(db.get("Reload").is_none());
        assert!(db.all().is_empty());
    }

    #[test]
    fn remove_nonexistent_is_ok() {
        let db = db();
        assert!(db.remove("NonExistent").is_ok());
    }

    #[test]
    fn multiple_commands_independent() {
        let db = db();
        db.set("ZoomIn", "ctrl", "Equal").unwrap();
        db.set("ZoomOut", "ctrl", "Minus").unwrap();
        db.set("ZoomReset", "ctrl", "Digit0").unwrap();
        assert_eq!(db.all().len(), 3);
        db.remove("ZoomIn").unwrap();
        assert_eq!(db.all().len(), 2);
        assert!(db.get("ZoomIn").is_none());
        assert!(db.get("ZoomOut").is_some());
    }

    #[test]
    fn get_nonexistent_returns_none() {
        assert!(db().get("NoSuchCommand").is_none());
    }

    #[test]
    fn modifier_can_be_empty_string() {
        let db = db();
        db.set("ScrollPageDown", "", "Space").unwrap();
        let e = db.get("ScrollPageDown").unwrap();
        assert_eq!(e.modifier, "");
    }
}
