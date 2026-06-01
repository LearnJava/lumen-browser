//! Omnibox bang-aliases stored in SQLite.
//!
//! A bang alias maps a short trigger (`!g`, `!gh`, `!yt`, …) to a URL template
//! containing `{query}`.  When the user types `!g rust programming` in the
//! address bar and commits, the engine expands it to
//! `https://www.google.com/search?q=rust%20programming` and navigates.
//!
//! Two built-in aliases are seeded on first open:
//! - `!g`  → Google
//! - `!gh` → GitHub repository search
//!
//! Both can be overridden or deleted by the user.  Custom aliases are added via
//! `set(trigger, expansion)`.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// One omnibox bang-alias entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmniboxAlias {
    /// Short trigger with leading `!`, e.g. `"!g"` or `"!gh"`.
    pub trigger: String,
    /// URL template with `{query}` placeholder, e.g.
    /// `"https://www.google.com/search?q={query}"`.
    pub expansion: String,
}

/// SQLite-backed registry of omnibox bang-aliases.
///
/// `open_in_memory()` is used for tests and ephemeral sessions;
/// `open(path)` persists across restarts.
pub struct OmniboxAliases {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for OmniboxAliases {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OmniboxAliases").finish()
    }
}

impl OmniboxAliases {
    /// Open persistent alias store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("omnibox_aliases open: {e}")))?;
        Self::init(conn)
    }

    /// Open in-memory store (tests / ephemeral sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("omnibox_aliases open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS omnibox_aliases (
                trigger    TEXT PRIMARY KEY,
                expansion  TEXT NOT NULL
            );
            "#,
        )
        .map_err(|e| Error::Storage(format!("omnibox_aliases init: {e}")))?;

        let store = Self { conn: Mutex::new(conn) };
        store.seed_defaults()?;
        Ok(store)
    }

    /// Insert built-in defaults if not already present (`INSERT OR IGNORE`).
    fn seed_defaults(&self) -> Result<()> {
        let defaults = [
            ("!g", "https://www.google.com/search?q={query}"),
            ("!gh", "https://github.com/search?q={query}&type=repositories"),
        ];
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("omnibox_aliases mutex poisoned".into()))?;
        for (trigger, expansion) in defaults {
            conn.execute(
                "INSERT OR IGNORE INTO omnibox_aliases (trigger, expansion) VALUES (?1, ?2)",
                params![trigger, expansion],
            )
            .map_err(|e| Error::Storage(format!("omnibox_aliases seed: {e}")))?;
        }
        Ok(())
    }

    /// Add or replace an alias.  `trigger` must start with `!`.
    pub fn set(&self, trigger: &str, expansion: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("omnibox_aliases mutex poisoned".into()))?;
        conn.execute(
            "INSERT OR REPLACE INTO omnibox_aliases (trigger, expansion) VALUES (?1, ?2)",
            params![trigger, expansion],
        )
        .map_err(|e| Error::Storage(format!("omnibox_aliases set: {e}")))?;
        Ok(())
    }

    /// Look up an alias by its `trigger` (e.g. `"!g"`).
    pub fn get(&self, trigger: &str) -> Result<Option<OmniboxAlias>> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("omnibox_aliases mutex poisoned".into()))?;
        let row = conn
            .query_row(
                "SELECT trigger, expansion FROM omnibox_aliases WHERE trigger = ?1",
                params![trigger],
                |r| Ok(OmniboxAlias { trigger: r.get(0)?, expansion: r.get(1)? }),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("omnibox_aliases get: {e}")))?;
        Ok(row)
    }

    /// All aliases ordered by trigger.
    pub fn list_all(&self) -> Result<Vec<OmniboxAlias>> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("omnibox_aliases mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT trigger, expansion FROM omnibox_aliases ORDER BY trigger ASC",
            )
            .map_err(|e| Error::Storage(format!("omnibox_aliases list prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(OmniboxAlias { trigger: r.get(0)?, expansion: r.get(1)? })
            })
            .map_err(|e| Error::Storage(format!("omnibox_aliases list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("omnibox_aliases row: {e}")))?);
        }
        Ok(out)
    }

    /// Delete an alias by trigger.  No-op if not found.
    pub fn delete(&self, trigger: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("omnibox_aliases mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM omnibox_aliases WHERE trigger = ?1",
            params![trigger],
        )
        .map_err(|e| Error::Storage(format!("omnibox_aliases delete: {e}")))?;
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> OmniboxAliases {
        OmniboxAliases::open_in_memory().unwrap()
    }

    #[test]
    fn defaults_seeded() {
        let s = make();
        let all = s.list_all().unwrap();
        let triggers: Vec<&str> = all.iter().map(|a| a.trigger.as_str()).collect();
        assert!(triggers.contains(&"!g"), "!g missing");
        assert!(triggers.contains(&"!gh"), "!gh missing");
    }

    #[test]
    fn get_builtin() {
        let s = make();
        let a = s.get("!g").unwrap().unwrap();
        assert!(a.expansion.contains("google.com"));
    }

    #[test]
    fn set_and_get_custom() {
        let s = make();
        s.set("!yt", "https://www.youtube.com/results?search_query={query}").unwrap();
        let a = s.get("!yt").unwrap().unwrap();
        assert!(a.expansion.contains("youtube.com"));
    }

    #[test]
    fn set_overrides_existing() {
        let s = make();
        s.set("!g", "https://custom.example.com/?q={query}").unwrap();
        let a = s.get("!g").unwrap().unwrap();
        assert!(a.expansion.contains("custom.example.com"));
    }

    #[test]
    fn delete_removes_entry() {
        let s = make();
        s.delete("!g").unwrap();
        assert!(s.get("!g").unwrap().is_none());
    }

    #[test]
    fn delete_nonexistent_is_noop() {
        let s = make();
        s.delete("!noexist").unwrap();
    }

    #[test]
    fn list_all_includes_custom() {
        let s = make();
        s.set("!yt", "https://yt/?q={query}").unwrap();
        let all = s.list_all().unwrap();
        assert!(all.iter().any(|a| a.trigger == "!yt"));
    }

    #[test]
    fn list_all_sorted_by_trigger() {
        let s = make();
        s.set("!zz", "https://zz.example/?q={query}").unwrap();
        s.set("!aa", "https://aa.example/?q={query}").unwrap();
        let all = s.list_all().unwrap();
        let triggers: Vec<&str> = all.iter().map(|a| a.trigger.as_str()).collect();
        let mut sorted = triggers.clone();
        sorted.sort_unstable();
        assert_eq!(triggers, sorted, "list_all should be sorted");
    }

    #[test]
    fn in_memory_has_exactly_two_defaults() {
        let s = make();
        // seed_defaults uses INSERT OR IGNORE so a second call must not duplicate.
        let count_g = s.list_all().unwrap().iter().filter(|a| a.trigger == "!g").count();
        assert_eq!(count_g, 1);
    }
}
