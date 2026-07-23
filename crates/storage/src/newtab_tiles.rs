//! Pinned `about:newtab` speed-dial tiles, stored in SQLite.
//!
//! A pinned tile is a user-chosen `(url, title)` pair shown at a fixed
//! position in the newtab grid, ahead of the automatic top-sites filler
//! (`crate::history::History::most_visited`, consumed by `lumen-shell`'s
//! `newtab` module). Pinning/unpinning is driven by special `about:newtab?…`
//! links rendered on the page itself — see `newtab::NewtabAction` in
//! `lumen-shell` for the link format.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Maximum number of tiles the newtab grid can hold (mirrors
/// `lumen_shell::newtab::MAX_TILES`; duplicated here because `lumen-storage`
/// does not depend on `lumen-shell`). [`NewtabTiles::pin`] refuses to add a
/// ninth tile once this many are already pinned.
pub const MAX_PINNED: i64 = 8;

/// One pinned newtab tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedTile {
    /// Display order in the grid (0-based, lower = earlier).
    pub position: i64,
    /// Absolute URL the tile navigates to.
    pub url: String,
    /// Display title shown under the tile icon.
    pub title: String,
}

/// SQLite-backed registry of pinned newtab tiles.
///
/// `open_in_memory()` is used for tests and ephemeral sessions;
/// `open(path)` persists across restarts.
pub struct NewtabTiles {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for NewtabTiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewtabTiles").finish()
    }
}

impl NewtabTiles {
    /// Open persistent tile store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("newtab_tiles open: {e}")))?;
        Self::init(conn)
    }

    /// Open in-memory store (tests / ephemeral sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("newtab_tiles open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS pinned_tiles (
                position   INTEGER PRIMARY KEY,
                url        TEXT NOT NULL UNIQUE,
                title      TEXT NOT NULL
            );
            "#,
        )
        .map_err(|e| Error::Storage(format!("newtab_tiles init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Pin `url` (titled `title`) as a new tile at the next free position.
    ///
    /// Returns `Ok(false)` without changing anything if `url` is already
    /// pinned, or if the grid already holds [`MAX_PINNED`] tiles.
    pub fn pin(&self, url: &str, title: &str) -> Result<bool> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("newtab_tiles mutex poisoned".into()))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pinned_tiles", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("newtab_tiles count: {e}")))?;
        if count >= MAX_PINNED {
            return Ok(false);
        }
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pinned_tiles WHERE url = ?1)",
                params![url],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("newtab_tiles exists: {e}")))?;
        if exists {
            return Ok(false);
        }
        let next_position: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM pinned_tiles",
                [],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("newtab_tiles next_position: {e}")))?;
        conn.execute(
            "INSERT INTO pinned_tiles (position, url, title) VALUES (?1, ?2, ?3)",
            params![next_position, url, title],
        )
        .map_err(|e| Error::Storage(format!("newtab_tiles pin: {e}")))?;
        Ok(true)
    }

    /// Unpin `url`. No-op if it isn't currently pinned.
    pub fn unpin(&self, url: &str) -> Result<()> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("newtab_tiles mutex poisoned".into()))?;
        conn.execute("DELETE FROM pinned_tiles WHERE url = ?1", params![url])
            .map_err(|e| Error::Storage(format!("newtab_tiles unpin: {e}")))?;
        Ok(())
    }

    /// All pinned tiles ordered by position.
    pub fn list_all(&self) -> Result<Vec<PinnedTile>> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("newtab_tiles mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached("SELECT position, url, title FROM pinned_tiles ORDER BY position ASC")
            .map_err(|e| Error::Storage(format!("newtab_tiles list prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PinnedTile { position: r.get(0)?, url: r.get(1)?, title: r.get(2)? })
            })
            .map_err(|e| Error::Storage(format!("newtab_tiles list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("newtab_tiles row: {e}")))?);
        }
        Ok(out)
    }

    /// `get(url).is_some()` shortcut, also useful as a `?row` existence check
    /// without allocating the full list.
    pub fn get(&self, url: &str) -> Result<Option<PinnedTile>> {
        let conn = self.conn.lock()
            .map_err(|_| Error::Storage("newtab_tiles mutex poisoned".into()))?;
        let row = conn
            .query_row(
                "SELECT position, url, title FROM pinned_tiles WHERE url = ?1",
                params![url],
                |r| Ok(PinnedTile { position: r.get(0)?, url: r.get(1)?, title: r.get(2)? }),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("newtab_tiles get: {e}")))?;
        Ok(row)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> NewtabTiles {
        NewtabTiles::open_in_memory().unwrap()
    }

    #[test]
    fn starts_empty() {
        let s = make();
        assert!(s.list_all().unwrap().is_empty());
    }

    #[test]
    fn pin_adds_at_position_zero() {
        let s = make();
        assert!(s.pin("https://a.test/", "A").unwrap());
        let all = s.list_all().unwrap();
        assert_eq!(all, vec![PinnedTile { position: 0, url: "https://a.test/".into(), title: "A".into() }]);
    }

    #[test]
    fn pin_appends_in_order() {
        let s = make();
        s.pin("https://a.test/", "A").unwrap();
        s.pin("https://b.test/", "B").unwrap();
        let all = s.list_all().unwrap();
        let urls: Vec<&str> = all.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, vec!["https://a.test/", "https://b.test/"]);
        assert_eq!(all[1].position, 1);
    }

    #[test]
    fn pin_duplicate_url_is_noop() {
        let s = make();
        assert!(s.pin("https://a.test/", "A").unwrap());
        assert!(!s.pin("https://a.test/", "A renamed").unwrap());
        assert_eq!(s.list_all().unwrap().len(), 1);
    }

    #[test]
    fn pin_rejects_ninth_tile() {
        let s = make();
        for i in 0..MAX_PINNED {
            assert!(s.pin(&format!("https://s{i}.test/"), "S").unwrap());
        }
        assert!(!s.pin("https://overflow.test/", "Overflow").unwrap());
        assert_eq!(s.list_all().unwrap().len(), MAX_PINNED as usize);
    }

    #[test]
    fn unpin_removes_entry() {
        let s = make();
        s.pin("https://a.test/", "A").unwrap();
        s.unpin("https://a.test/").unwrap();
        assert!(s.list_all().unwrap().is_empty());
    }

    #[test]
    fn unpin_nonexistent_is_noop() {
        let s = make();
        s.unpin("https://nope.test/").unwrap();
    }

    #[test]
    fn unpin_then_pin_frees_a_slot() {
        let s = make();
        for i in 0..MAX_PINNED {
            s.pin(&format!("https://s{i}.test/"), "S").unwrap();
        }
        s.unpin("https://s0.test/").unwrap();
        assert!(s.pin("https://new.test/", "New").unwrap());
        assert_eq!(s.list_all().unwrap().len(), MAX_PINNED as usize);
    }

    #[test]
    fn get_returns_pinned_entry() {
        let s = make();
        s.pin("https://a.test/", "A").unwrap();
        let t = s.get("https://a.test/").unwrap().unwrap();
        assert_eq!(t.title, "A");
        assert!(s.get("https://missing.test/").unwrap().is_none());
    }
}
