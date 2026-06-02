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
//!
//! The `dom_blob` is transparently deflate-compressed on the way in and
//! inflated on the way out (ADR-008 §10J.1), so the in-RAM/on-disk footprint of
//! hibernated tabs stays small. `HibernatedTabData::dom_blob` is always the raw
//! bincode bytes — compression is an internal storage detail.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Magic prefix tagging a deflate-compressed DOM blob (ADR-008 §10J.1).
///
/// `store()` prepends these 4 bytes before the zlib stream so `fetch()` can tell
/// a compressed blob from a legacy raw-bincode one and pick the right path.
/// "LZD1" = **L**umen **Z**lib **D**eflate, format version **1**.
const BLOB_MAGIC: [u8; 4] = *b"LZD1";

/// Compress a raw bincode `Document` blob for on-disk storage.
///
/// Returns [`BLOB_MAGIC`] followed by a zlib (deflate) stream of `raw`. DOM blobs
/// are string-heavy (repeated tag/attribute names) and typically shrink 3-5×,
/// directly serving the ADR-008 RAM/disk goal for hibernated tabs. On the
/// (effectively impossible) encoder failure the raw bytes are stored unchanged
/// so the snapshot is never lost.
fn compress_blob(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() / 3 + BLOB_MAGIC.len());
    out.extend_from_slice(&BLOB_MAGIC);
    let mut encoder = ZlibEncoder::new(out, Compression::default());
    if encoder.write_all(raw).is_ok()
        && let Ok(buf) = encoder.finish()
    {
        return buf;
    }
    // Fallback: never drop a snapshot — store raw (no magic ⇒ read as legacy).
    raw.to_vec()
}

/// Inverse of [`compress_blob`].
///
/// If `stored` begins with [`BLOB_MAGIC`] the trailing zlib stream is inflated;
/// otherwise the bytes are returned verbatim (legacy uncompressed blobs written
/// before 10J.1, or the raw-fallback path above).
fn decompress_blob(stored: Vec<u8>) -> Result<Vec<u8>> {
    if stored.len() < BLOB_MAGIC.len() || stored[..BLOB_MAGIC.len()] != BLOB_MAGIC {
        return Ok(stored);
    }
    let mut decoder = ZlibDecoder::new(&stored[BLOB_MAGIC.len()..]);
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|e| Error::Storage(format!("tab_snapshot decompress: {e}")))?;
    Ok(raw)
}

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
        let blob = compress_blob(&data.dom_blob);
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO hibernated_tabs
             (tab_id, dom_blob, css_source, url, title, scroll_x, scroll_y)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                tab_id,
                blob,
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
        let row = conn
            .query_row(
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
            .map_err(|e| Error::Storage(format!("tab_snapshot fetch: {e}")))?;
        // Inflate the stored blob back to raw bincode for the caller.
        match row {
            Some(mut data) => {
                data.dom_blob = decompress_blob(data.dom_blob)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
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
    fn compress_decompress_roundtrip() {
        for raw in [
            Vec::new(),
            vec![1u8, 2, 3, 4, 5],
            b"the quick brown fox".to_vec(),
            vec![0u8; 4096],
        ] {
            let stored = compress_blob(&raw);
            let back = decompress_blob(stored).unwrap();
            assert_eq!(back, raw);
        }
    }

    #[test]
    fn compress_tags_with_magic_and_shrinks_repetitive() {
        // bincode DOM blobs are string-heavy; emulate with a repetitive payload.
        let raw: Vec<u8> = b"<div class=\"row\">".iter().cloned().cycle().take(64 * 1024).collect();
        let stored = compress_blob(&raw);
        assert_eq!(&stored[..BLOB_MAGIC.len()], &BLOB_MAGIC);
        assert!(stored.len() < raw.len() / 4, "expected >4x shrink, got {} → {}", raw.len(), stored.len());
        assert_eq!(decompress_blob(stored).unwrap(), raw);
    }

    #[test]
    fn decompress_passes_through_legacy_raw_blob() {
        // A pre-10J.1 uncompressed bincode blob has no magic prefix.
        let legacy = vec![9u8, 8, 7, 6, 5, 4, 3, 2, 1, 0];
        assert_eq!(decompress_blob(legacy.clone()).unwrap(), legacy);
    }

    #[test]
    fn decompress_short_blob_passes_through() {
        let short = vec![1u8, 2];
        assert_eq!(decompress_blob(short.clone()).unwrap(), short);
    }

    #[test]
    fn store_compresses_blob_on_disk() {
        let s = make();
        let raw: Vec<u8> = b"<span>text</span>".iter().cloned().cycle().take(128 * 1024).collect();
        let data = HibernatedTabData { dom_blob: raw.clone(), ..sample_data() };
        s.store(5, &data).unwrap();
        // The on-disk column is the compressed form, much smaller than the input.
        let on_disk_len: i64 = s
            .lock()
            .unwrap()
            .query_row(
                "SELECT length(dom_blob) FROM hibernated_tabs WHERE tab_id = 5",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!((on_disk_len as usize) < raw.len() / 4);
        // fetch transparently inflates back to the original bytes.
        assert_eq!(s.fetch(5).unwrap().unwrap().dom_blob, raw);
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
