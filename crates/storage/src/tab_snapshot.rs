//! SQLite-backed blob store for T3-hibernated tab DOM snapshots (ADR-008 §10J).
//!
//! When a tab transitions from T2 (BackgroundOld) to T3 (Hibernated), the shell
//! serialises its `Document` via `Document::to_bytes()` (bincode) and stores
//! the blob here alongside the inline CSS text and scroll position.  Only a
//! lightweight `TabMetadata` struct (~200 B) remains in RAM.
//!
//! On restore (T3 → T0 Active), the shell fetches the blob, calls
//! `Document::from_bytes()`, re-parses the CSS, and re-runs layout+paint.
//! Target SLO: ≤ 1 500 ms.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// All data stored on disk for a hibernated tab.
///
/// Produced by the shell just before evicting a background tab's `PageSnapshot`
/// from RAM and stored in [`TabSnapshotStore`].
pub struct HibernatedTabData {
    /// Bincode-serialised `Document` blob produced by `Document::to_bytes()`.
    ///
    /// Allows `Document::from_bytes()` to skip HTML reparsing on restore.
    pub dom_blob: Vec<u8>,
    /// Combined inline + external CSS text that was used for the last layout.
    ///
    /// Re-parsed into a `Stylesheet` on restore so the cascade is correct
    /// without another network round-trip.
    pub css_source: String,
    /// Page URL — used for display and as the base URL for relative resources
    /// when re-loading images on restore.
    pub url: String,
    /// Tab title at the time of hibernation.
    pub title: String,
    /// Horizontal scroll offset in CSS px at the time of hibernation.
    pub scroll_x: f32,
    /// Vertical scroll offset in CSS px at the time of hibernation.
    pub scroll_y: f32,
}

/// SQLite-backed store for hibernated tab snapshots.
///
/// Phase 0: uses an in-memory database (data lost on browser restart).
/// Phase 2: open a real file at the profile directory for cross-session restore.
pub struct TabSnapshotStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for TabSnapshotStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabSnapshotStore").finish()
    }
}

impl TabSnapshotStore {
    /// Open an in-memory store (data is lost when the process exits).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("tab_snapshot open_in_memory: {e}")))?;
        Self::init(conn)
    }

    /// Open a persistent on-disk store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("tab_snapshot open: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS hibernated_tabs (
                tab_id      INTEGER PRIMARY KEY,
                dom_blob    BLOB    NOT NULL,
                css_source  TEXT    NOT NULL DEFAULT '',
                url         TEXT    NOT NULL DEFAULT '',
                title       TEXT    NOT NULL DEFAULT '',
                scroll_x    REAL    NOT NULL DEFAULT 0.0,
                scroll_y    REAL    NOT NULL DEFAULT 0.0
            );
            "#,
        )
        .map_err(|e| Error::Storage(format!("tab_snapshot init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Persist a hibernated tab snapshot.  Overwrites any previous entry for
    /// the same `tab_id` (upsert).
    pub fn store(&self, tab_id: i64, data: &HibernatedTabData) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO hibernated_tabs
             (tab_id, dom_blob, css_source, url, title, scroll_x, scroll_y)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                tab_id,
                data.dom_blob,
                data.css_source,
                data.url,
                data.title,
                data.scroll_x as f64,
                data.scroll_y as f64,
            ],
        )
        .map_err(|e| Error::Storage(format!("tab_snapshot store: {e}")))?;
        Ok(())
    }

    /// Load the hibernated snapshot for `tab_id`.
    ///
    /// Returns `Ok(None)` if no snapshot exists for that tab.
    pub fn fetch(&self, tab_id: i64) -> Result<Option<HibernatedTabData>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT dom_blob, css_source, url, title, scroll_x, scroll_y
             FROM hibernated_tabs WHERE tab_id = ?1",
            params![tab_id],
            |row| {
                Ok(HibernatedTabData {
                    dom_blob: row.get(0)?,
                    css_source: row.get(1)?,
                    url: row.get(2)?,
                    title: row.get(3)?,
                    scroll_x: row.get::<_, f64>(4)? as f32,
                    scroll_y: row.get::<_, f64>(5)? as f32,
                })
            },
        )
        .optional()
        .map_err(|e| Error::Storage(format!("tab_snapshot fetch: {e}")))
    }

    /// Remove the snapshot for `tab_id` (called after successful restore).
    pub fn delete(&self, tab_id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM hibernated_tabs WHERE tab_id = ?1",
            params![tab_id],
        )
        .map_err(|e| Error::Storage(format!("tab_snapshot delete: {e}")))?;
        Ok(())
    }

    /// Returns `true` if a snapshot exists for `tab_id`.
    pub fn exists(&self, tab_id: i64) -> Result<bool> {
        let conn = self.lock()?;
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM hibernated_tabs WHERE tab_id = ?1",
                params![tab_id],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("tab_snapshot exists: {e}")))?;
        Ok(n > 0)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Storage("tab_snapshot mutex poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> TabSnapshotStore {
        TabSnapshotStore::open_in_memory().unwrap()
    }

    fn sample_data() -> HibernatedTabData {
        HibernatedTabData {
            dom_blob: vec![1, 2, 3, 4, 5],
            css_source: "body { color: red; }".into(),
            url: "https://example.com/".into(),
            title: "Example".into(),
            scroll_x: 0.0,
            scroll_y: 320.5,
        }
    }

    #[test]
    fn store_and_fetch() {
        let s = make();
        s.store(1, &sample_data()).unwrap();
        let data = s.fetch(1).unwrap().unwrap();
        assert_eq!(data.dom_blob, vec![1, 2, 3, 4, 5]);
        assert_eq!(data.css_source, "body { color: red; }");
        assert_eq!(data.url, "https://example.com/");
        assert_eq!(data.title, "Example");
        assert!((data.scroll_y - 320.5).abs() < 0.01);
    }

    #[test]
    fn fetch_missing_returns_none() {
        let s = make();
        assert!(s.fetch(999).unwrap().is_none());
    }

    #[test]
    fn exists_true_after_store() {
        let s = make();
        assert!(!s.exists(1).unwrap());
        s.store(1, &sample_data()).unwrap();
        assert!(s.exists(1).unwrap());
    }

    #[test]
    fn delete_removes_entry() {
        let s = make();
        s.store(1, &sample_data()).unwrap();
        s.delete(1).unwrap();
        assert!(s.fetch(1).unwrap().is_none());
        assert!(!s.exists(1).unwrap());
    }

    #[test]
    fn store_overwrites_same_tab_id() {
        let s = make();
        s.store(1, &sample_data()).unwrap();
        let updated = HibernatedTabData {
            title: "Updated".into(),
            scroll_y: 100.0,
            ..sample_data()
        };
        s.store(1, &updated).unwrap();
        let data = s.fetch(1).unwrap().unwrap();
        assert_eq!(data.title, "Updated");
        assert!((data.scroll_y - 100.0).abs() < 0.01);
    }

    #[test]
    fn store_large_blob() {
        let s = make();
        let large_blob = vec![0u8; 512 * 1024]; // 512 KB
        let data = HibernatedTabData { dom_blob: large_blob.clone(), ..sample_data() };
        s.store(42, &data).unwrap();
        let fetched = s.fetch(42).unwrap().unwrap();
        assert_eq!(fetched.dom_blob.len(), 512 * 1024);
    }

    #[test]
    fn multiple_tabs_independent() {
        let s = make();
        let d1 = HibernatedTabData { title: "Tab 1".into(), ..sample_data() };
        let d2 = HibernatedTabData { title: "Tab 2".into(), scroll_y: 50.0, ..sample_data() };
        s.store(1, &d1).unwrap();
        s.store(2, &d2).unwrap();
        assert_eq!(s.fetch(1).unwrap().unwrap().title, "Tab 1");
        assert_eq!(s.fetch(2).unwrap().unwrap().title, "Tab 2");
        s.delete(1).unwrap();
        assert!(s.fetch(1).unwrap().is_none());
        assert!(s.fetch(2).unwrap().is_some());
    }

    #[test]
    fn cyrillic_title_and_url() {
        let s = make();
        let data = HibernatedTabData {
            url: "https://пример.рф/".into(),
            title: "Главная страница".into(),
            ..sample_data()
        };
        s.store(7, &data).unwrap();
        let fetched = s.fetch(7).unwrap().unwrap();
        assert_eq!(fetched.url, "https://пример.рф/");
        assert_eq!(fetched.title, "Главная страница");
    }
}
