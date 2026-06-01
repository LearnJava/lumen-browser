//! SQLite-backed store for the *last session* — every open tab at the moment
//! the browser window was closed (§10I).
//!
//! Distinct from two neighbouring stores:
//! * [`crate::tab_snapshot::TabSnapshotStore`] keeps DOM blobs for tabs that the
//!   *running* browser hibernated (T3) — keyed by the volatile in-session
//!   `tab_id`, cleared on restore.
//! * [`crate::session_export`] is a portable, SQLite-free JSON file for manual
//!   backup / sharing between machines.
//!
//! This store, by contrast, is the engine's own cross-restart memory: the shell
//! overwrites it wholesale on window close and reads it back on the next launch
//! to reopen the exact set of tabs (URL + title + scroll + serialised DOM).
//! Tabs are addressed by left-to-right ordinal so restore preserves tab order.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

/// One persisted tab in the saved session.
///
/// The DOM blob is produced by `Document::to_bytes()` so a background tab can be
/// reconstructed via `Document::from_bytes()` without a network round-trip; it
/// may be empty for tabs that never finished loading (those are restored by a
/// fresh navigation to `url`).
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedTab {
    /// Page URL (or `file://`-style path string). Never empty for a stored tab.
    pub url: String,
    /// Tab title at save time. May be empty.
    pub title: String,
    /// Horizontal scroll offset in CSS px.
    pub scroll_x: f32,
    /// Vertical scroll offset in CSS px.
    pub scroll_y: f32,
    /// Whether this was the focused tab when the session was saved.
    pub is_active: bool,
    /// Bincode-serialised `Document` (`Document::to_bytes()`); empty if unknown.
    pub dom_blob: Vec<u8>,
}

/// SQLite-backed store holding exactly one session — the tabs open at last close.
///
/// Phase 0: open an on-disk file (e.g. `last_session.db`) for cross-restart
/// restore, or an in-memory database in tests.
pub struct SessionStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore").finish()
    }
}

impl SessionStore {
    /// Open an in-memory store (data lost when the process exits).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("session_store open_in_memory: {e}")))?;
        Self::init(conn)
    }

    /// Open a persistent on-disk store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("session_store open: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS session_tabs (
                ord       INTEGER PRIMARY KEY,
                url       TEXT    NOT NULL,
                title     TEXT    NOT NULL DEFAULT '',
                scroll_x  REAL    NOT NULL DEFAULT 0.0,
                scroll_y  REAL    NOT NULL DEFAULT 0.0,
                is_active INTEGER NOT NULL DEFAULT 0,
                dom_blob  BLOB    NOT NULL
            );
            "#,
        )
        .map_err(|e| Error::Storage(format!("session_store init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Replace the saved session with `tabs`, preserving their order.
    ///
    /// The whole table is cleared first inside a single transaction, so a saved
    /// session is always internally consistent (no leftover tabs from a longer
    /// previous session).
    pub fn save(&self, tabs: &[PersistedTab]) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| Error::Storage(format!("session_store tx: {e}")))?;
        tx.execute("DELETE FROM session_tabs", [])
            .map_err(|e| Error::Storage(format!("session_store clear: {e}")))?;
        for (ord, tab) in tabs.iter().enumerate() {
            tx.execute(
                "INSERT INTO session_tabs
                 (ord, url, title, scroll_x, scroll_y, is_active, dom_blob)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    ord as i64,
                    tab.url,
                    tab.title,
                    f64::from(tab.scroll_x),
                    f64::from(tab.scroll_y),
                    i64::from(tab.is_active),
                    tab.dom_blob,
                ],
            )
            .map_err(|e| Error::Storage(format!("session_store insert: {e}")))?;
        }
        tx.commit()
            .map_err(|e| Error::Storage(format!("session_store commit: {e}")))?;
        Ok(())
    }

    /// Load all saved tabs in their original left-to-right order.
    ///
    /// Returns an empty vector when no session has been saved.
    pub fn load(&self) -> Result<Vec<PersistedTab>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT url, title, scroll_x, scroll_y, is_active, dom_blob
                 FROM session_tabs ORDER BY ord ASC",
            )
            .map_err(|e| Error::Storage(format!("session_store prepare: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PersistedTab {
                    url: row.get(0)?,
                    title: row.get(1)?,
                    scroll_x: row.get::<_, f64>(2)? as f32,
                    scroll_y: row.get::<_, f64>(3)? as f32,
                    is_active: row.get::<_, i64>(4)? != 0,
                    dom_blob: row.get(5)?,
                })
            })
            .map_err(|e| Error::Storage(format!("session_store query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("session_store row: {e}")))?);
        }
        Ok(out)
    }

    /// Remove all saved tabs (e.g. user disabled session restore).
    pub fn clear(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM session_tabs", [])
            .map_err(|e| Error::Storage(format!("session_store clear: {e}")))?;
        Ok(())
    }

    /// Number of tabs in the saved session.
    pub fn len(&self) -> Result<usize> {
        let conn = self.lock()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_tabs", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("session_store count: {e}")))?;
        Ok(n as usize)
    }

    /// Returns `true` when no session has been saved.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Storage("session_store mutex poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> SessionStore {
        SessionStore::open_in_memory().unwrap()
    }

    fn tab(url: &str, active: bool) -> PersistedTab {
        PersistedTab {
            url: url.into(),
            title: format!("title of {url}"),
            scroll_x: 0.0,
            scroll_y: 0.0,
            is_active: active,
            dom_blob: vec![],
        }
    }

    #[test]
    fn empty_store_loads_nothing() {
        let s = make();
        assert!(s.is_empty().unwrap());
        assert_eq!(s.len().unwrap(), 0);
        assert!(s.load().unwrap().is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let s = make();
        let tabs = vec![
            PersistedTab {
                url: "https://example.com/".into(),
                title: "Example".into(),
                scroll_x: 12.0,
                scroll_y: 340.0,
                is_active: true,
                dom_blob: vec![1, 2, 3],
            },
            tab("https://rust-lang.org/", false),
        ];
        s.save(&tabs).unwrap();
        let loaded = s.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].url, "https://example.com/");
        assert_eq!(loaded[0].title, "Example");
        assert!((loaded[0].scroll_x - 12.0).abs() < 0.01);
        assert!((loaded[0].scroll_y - 340.0).abs() < 0.01);
        assert!(loaded[0].is_active);
        assert_eq!(loaded[0].dom_blob, vec![1, 2, 3]);
        assert!(!loaded[1].is_active);
    }

    #[test]
    fn load_preserves_order() {
        let s = make();
        let tabs: Vec<PersistedTab> = (0..5)
            .map(|i| tab(&format!("https://site{i}/"), i == 2))
            .collect();
        s.save(&tabs).unwrap();
        let loaded = s.load().unwrap();
        for (i, t) in loaded.iter().enumerate() {
            assert_eq!(t.url, format!("https://site{i}/"));
        }
        assert!(loaded[2].is_active);
    }

    #[test]
    fn save_replaces_previous_session() {
        let s = make();
        s.save(&[tab("https://a/", true), tab("https://b/", false)])
            .unwrap();
        s.save(&[tab("https://c/", true)]).unwrap();
        let loaded = s.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].url, "https://c/");
    }

    #[test]
    fn clear_empties_the_store() {
        let s = make();
        s.save(&[tab("https://a/", true)]).unwrap();
        assert!(!s.is_empty().unwrap());
        s.clear().unwrap();
        assert!(s.is_empty().unwrap());
    }

    #[test]
    fn save_empty_clears() {
        let s = make();
        s.save(&[tab("https://a/", true)]).unwrap();
        s.save(&[]).unwrap();
        assert!(s.is_empty().unwrap());
    }

    #[test]
    fn large_dom_blob_roundtrips() {
        let s = make();
        let blob = vec![7u8; 256 * 1024];
        let mut t = tab("https://big/", true);
        t.dom_blob = blob.clone();
        s.save(&[t]).unwrap();
        assert_eq!(s.load().unwrap()[0].dom_blob.len(), blob.len());
    }

    #[test]
    fn cyrillic_url_and_title() {
        let s = make();
        let mut t = tab("https://пример.рф/", true);
        t.title = "Главная".into();
        s.save(&[t]).unwrap();
        let loaded = s.load().unwrap();
        assert_eq!(loaded[0].url, "https://пример.рф/");
        assert_eq!(loaded[0].title, "Главная");
    }
}
