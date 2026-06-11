//! Print preferences backed by SQLite (W-2b).
//!
//! Stores print dialog settings persisted across sessions:
//!
//! - **scale** — document zoom level (50–200%), default 100%.
//! - **paper_size** — A4 / Letter / Legal.
//! - **orientation** — Portrait / Landscape.
//! - **margins** — Normal / Narrow / Wide.
//! - **color_mode** — Color / Grayscale.
//! - **page_range** — free-form string: "all" | "1-3,5" | etc.
//! - **output_path** — destination file path.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

// ── Setting keys ────────────────────────────────────────────────────────────

const KEY_SCALE: &str = "scale";
const KEY_PAPER_SIZE: &str = "paper_size";
const KEY_ORIENTATION: &str = "orientation";
const KEY_MARGINS: &str = "margins";
const KEY_COLOR_MODE: &str = "color_mode";
const KEY_PAGE_RANGE: &str = "page_range";
const KEY_OUTPUT_PATH: &str = "output_path";

// ── Defaults ────────────────────────────────────────────────────────────────

const DEFAULT_SCALE: i32 = 100; // percent
const DEFAULT_PAPER_SIZE: &str = "A4";
const DEFAULT_ORIENTATION: &str = "portrait";
const DEFAULT_MARGINS: &str = "normal";
const DEFAULT_COLOR_MODE: &str = "color";
const DEFAULT_PAGE_RANGE: &str = "all";
const DEFAULT_OUTPUT_PATH: &str = "output.pdf";

// ── Snapshot ─────────────────────────────────────────────────────────────────

/// All print preferences as a copyable value type.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintPrefsSnapshot {
    /// Document zoom level in percent (50–200%).
    pub scale: i32,
    /// Paper size: "A4" | "Letter" | "Legal".
    pub paper_size: String,
    /// Page orientation: "portrait" | "landscape".
    pub orientation: String,
    /// Margin preset: "normal" | "narrow" | "wide".
    pub margins: String,
    /// Color mode: "color" | "grayscale".
    pub color_mode: String,
    /// Page range string: "all" or explicit range like "1-3,5".
    pub page_range: String,
    /// Output file path.
    pub output_path: String,
}

impl Default for PrintPrefsSnapshot {
    fn default() -> Self {
        Self {
            scale: DEFAULT_SCALE,
            paper_size: DEFAULT_PAPER_SIZE.to_owned(),
            orientation: DEFAULT_ORIENTATION.to_owned(),
            margins: DEFAULT_MARGINS.to_owned(),
            color_mode: DEFAULT_COLOR_MODE.to_owned(),
            page_range: DEFAULT_PAGE_RANGE.to_owned(),
            output_path: DEFAULT_OUTPUT_PATH.to_owned(),
        }
    }
}

// ── PrintPrefs ─────────────────────────────────────────────────────────────

/// Print preferences backed by SQLite.
///
/// Thread-safe handle wrapping a connection + mutex. Load preferences
/// once at startup via `open()`, mutate them as the print dialog changes,
/// then call `save_snapshot()` to persist.
pub struct PrintPrefs {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for PrintPrefs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrintPrefs").finish_non_exhaustive()
    }
}

impl PrintPrefs {
    /// Open (or create) the SQLite store for print preferences.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(e.to_string()))?;

        // Create table if it doesn't exist.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS print_prefs (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Load the current snapshot of all print preferences.
    pub fn load_snapshot(&self) -> Result<PrintPrefsSnapshot> {
        let conn = self.conn.lock().unwrap();

        let scale = self
            .get_string(&conn, KEY_SCALE)?
            .parse::<i32>()
            .unwrap_or(DEFAULT_SCALE)
            .clamp(50, 200);

        Ok(PrintPrefsSnapshot {
            scale,
            paper_size: self.get_string(&conn, KEY_PAPER_SIZE)?,
            orientation: self.get_string(&conn, KEY_ORIENTATION)?,
            margins: self.get_string(&conn, KEY_MARGINS)?,
            color_mode: self.get_string(&conn, KEY_COLOR_MODE)?,
            page_range: self.get_string(&conn, KEY_PAGE_RANGE)?,
            output_path: self.get_string(&conn, KEY_OUTPUT_PATH)?,
        })
    }

    /// Persist a snapshot of print preferences to the database.
    pub fn save_snapshot(&self, snap: &PrintPrefsSnapshot) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        self.set_string(&conn, KEY_SCALE, &snap.scale.to_string())?;
        self.set_string(&conn, KEY_PAPER_SIZE, &snap.paper_size)?;
        self.set_string(&conn, KEY_ORIENTATION, &snap.orientation)?;
        self.set_string(&conn, KEY_MARGINS, &snap.margins)?;
        self.set_string(&conn, KEY_COLOR_MODE, &snap.color_mode)?;
        self.set_string(&conn, KEY_PAGE_RANGE, &snap.page_range)?;
        self.set_string(&conn, KEY_OUTPUT_PATH, &snap.output_path)?;

        Ok(())
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn get_string(&self, conn: &Connection, key: &str) -> Result<String> {
        match conn.query_row(
            "SELECT value FROM print_prefs WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(val),
            Err(_) => Ok(Self::default_value(key).to_owned()),
        }
    }

    fn set_string(&self, conn: &Connection, key: &str, value: &str) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO print_prefs (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    fn default_value(key: &str) -> &'static str {
        match key {
            KEY_SCALE => "100",
            KEY_PAPER_SIZE => DEFAULT_PAPER_SIZE,
            KEY_ORIENTATION => DEFAULT_ORIENTATION,
            KEY_MARGINS => DEFAULT_MARGINS,
            KEY_COLOR_MODE => DEFAULT_COLOR_MODE,
            KEY_PAGE_RANGE => DEFAULT_PAGE_RANGE,
            KEY_OUTPUT_PATH => DEFAULT_OUTPUT_PATH,
            _ => "unknown",
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_db() -> (String, PrintPrefs) {
        let tmpdir = std::env::temp_dir();
        let path = tmpdir.join(format!("test-print-prefs-{}-{}.db", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
        let _ = fs::remove_file(&path);
        let prefs = PrintPrefs::open(&path).unwrap();
        (path.to_string_lossy().to_string(), prefs)
    }

    #[test]
    fn default_snapshot() {
        let snap = PrintPrefsSnapshot::default();
        assert_eq!(snap.scale, 100);
        assert_eq!(snap.paper_size, "A4");
        assert_eq!(snap.orientation, "portrait");
        assert_eq!(snap.page_range, "all");
    }

    #[test]
    fn loads_with_defaults_when_empty() {
        let (_path, db) = tmp_db();
        let snap = db.load_snapshot().unwrap();
        assert_eq!(snap.scale, 100);
        assert_eq!(snap.paper_size, "A4");
    }

    #[test]
    fn saves_and_loads_scale() {
        let (_path, db) = tmp_db();
        let snap = PrintPrefsSnapshot {
            scale: 150,
            ..Default::default()
        };
        db.save_snapshot(&snap).unwrap();

        let loaded = db.load_snapshot().unwrap();
        assert_eq!(loaded.scale, 150);
    }

    #[test]
    fn saves_and_loads_all_fields() {
        let (_path, db) = tmp_db();
        let snap = PrintPrefsSnapshot {
            scale: 120,
            paper_size: "Letter".to_owned(),
            orientation: "landscape".to_owned(),
            page_range: "1-5".to_owned(),
            ..Default::default()
        };
        db.save_snapshot(&snap).unwrap();

        let loaded = db.load_snapshot().unwrap();
        assert_eq!(loaded.scale, 120);
        assert_eq!(loaded.paper_size, "Letter");
        assert_eq!(loaded.orientation, "landscape");
        assert_eq!(loaded.page_range, "1-5");
    }

    #[test]
    fn clamps_scale_to_50_200() {
        let (_path, db) = tmp_db();
        let snap = PrintPrefsSnapshot {
            scale: 250,
            ..Default::default()
        };
        db.save_snapshot(&snap).unwrap();

        let loaded = db.load_snapshot().unwrap();
        assert_eq!(loaded.scale, 200); // Clamped to max
    }
}
