//! Accessibility preferences backed by SQLite.
//!
//! Stores four settings exposed by the accessibility settings panel (E-2):
//!
//! - **font_size_multiplier** — scale factor applied on top of the base font
//!   size: 0.8 / 1.0 / 1.25 / 1.5 / 2.0.
//! - **reduced_motion** — mirrors `prefers-reduced-motion: reduce`; delivered
//!   to JS via `_lumen_deliver_media_changes`.
//! - **forced_colors** — mirrors `prefers-forced-colors: active`; stored for
//!   layout-time CSS media query matching.
//! - **cursor_size** — OS-style cursor magnification: Normal / Large /
//!   ExtraLarge.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

// ── Setting keys ────────────────────────────────────────────────────────────

const KEY_FONT_MULTIPLIER: &str = "font_size_multiplier";
const KEY_REDUCED_MOTION: &str = "reduced_motion";
const KEY_FORCED_COLORS: &str = "forced_colors";
const KEY_CURSOR_SIZE: &str = "cursor_size";

// ── Defaults ────────────────────────────────────────────────────────────────

const DEFAULT_FONT_MULTIPLIER: f64 = 1.0;
const DEFAULT_REDUCED_MOTION: bool = false;
const DEFAULT_FORCED_COLORS: bool = false;
const DEFAULT_CURSOR_SIZE: &str = "normal";

// ── CursorSize ───────────────────────────────────────────────────────────────

/// Accessibility cursor magnification level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorSize {
    /// System-default cursor size.
    #[default]
    Normal,
    /// 1.5× cursor magnification.
    Large,
    /// 2× cursor magnification.
    ExtraLarge,
}

impl CursorSize {
    /// Serialize to the storage string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
            Self::ExtraLarge => "extra_large",
        }
    }

    /// Parse from the storage string representation; unknown values → `Normal`.
    pub fn parse(s: &str) -> Self {
        match s {
            "large" => Self::Large,
            "extra_large" => Self::ExtraLarge,
            _ => Self::Normal,
        }
    }
}

// ── Snapshot ─────────────────────────────────────────────────────────────────

/// All accessibility preferences as a copyable value type.
#[derive(Debug, Clone, PartialEq)]
pub struct A11yPrefsSnapshot {
    /// Font-size scale factor applied on top of the base browser font size.
    ///
    /// Valid values: 0.8, 1.0, 1.25, 1.5, 2.0. Stored as-is; the shell
    /// multiplies the base CSS `font-size` by this factor before layout.
    pub font_size_multiplier: f64,
    /// Whether `prefers-reduced-motion: reduce` is active.
    ///
    /// When `true` the shell delivers `reducedMotion=true` to JS via
    /// `_lumen_deliver_media_changes`.
    pub reduced_motion: bool,
    /// Whether `prefers-forced-colors: active` is set.
    ///
    /// When `true` CSS media `(forced-colors: active)` matches in layout.
    pub forced_colors: bool,
    /// Accessibility cursor magnification level.
    pub cursor_size: CursorSize,
}

impl Default for A11yPrefsSnapshot {
    fn default() -> Self {
        Self {
            font_size_multiplier: DEFAULT_FONT_MULTIPLIER,
            reduced_motion: DEFAULT_REDUCED_MOTION,
            forced_colors: DEFAULT_FORCED_COLORS,
            cursor_size: CursorSize::default(),
        }
    }
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// Persistent accessibility preferences store.
pub struct A11yPrefs {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for A11yPrefs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("A11yPrefs").finish_non_exhaustive()
    }
}

impl A11yPrefs {
    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS a11y_prefs (
                key   TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Open (or create) an on-disk accessibility preferences database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| Error::Storage(e.to_string()))?;
        Self::init(conn)
    }

    /// Create an in-memory accessibility preferences database (for tests / ephemeral sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| Error::Storage(e.to_string()))?;
        Self::init(conn)
    }

    // ── Low-level helpers ────────────────────────────────────────────────────

    fn get_string(&self, key: &str, default: &str) -> String {
        let conn = self.conn.lock().expect("a11y_prefs lock");
        conn.query_row(
            "SELECT value FROM a11y_prefs WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| default.to_owned())
    }

    fn set_str(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().expect("a11y_prefs lock");
        conn.execute(
            "INSERT INTO a11y_prefs (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    fn get_bool(&self, key: &str, default: bool) -> bool {
        let s = self.get_string(key, if default { "1" } else { "0" });
        s == "1"
    }

    fn set_bool(&self, key: &str, value: bool) -> Result<()> {
        self.set_str(key, if value { "1" } else { "0" })
    }

    fn get_f64(&self, key: &str, default: f64) -> f64 {
        self.get_string(key, &default.to_string())
            .parse()
            .unwrap_or(default)
    }

    fn set_f64(&self, key: &str, value: f64) -> Result<()> {
        self.set_str(key, &value.to_string())
    }

    // ── Public typed accessors ───────────────────────────────────────────────

    /// Font-size scale multiplier (e.g. 1.0, 1.25, 1.5).
    pub fn font_size_multiplier(&self) -> f64 {
        self.get_f64(KEY_FONT_MULTIPLIER, DEFAULT_FONT_MULTIPLIER)
    }

    /// Set font-size scale multiplier.
    pub fn set_font_size_multiplier(&self, v: f64) -> Result<()> {
        self.set_f64(KEY_FONT_MULTIPLIER, v)
    }

    /// Whether `prefers-reduced-motion` is active.
    pub fn reduced_motion(&self) -> bool {
        self.get_bool(KEY_REDUCED_MOTION, DEFAULT_REDUCED_MOTION)
    }

    /// Set prefers-reduced-motion.
    pub fn set_reduced_motion(&self, v: bool) -> Result<()> {
        self.set_bool(KEY_REDUCED_MOTION, v)
    }

    /// Whether `prefers-forced-colors` is active.
    pub fn forced_colors(&self) -> bool {
        self.get_bool(KEY_FORCED_COLORS, DEFAULT_FORCED_COLORS)
    }

    /// Set forced-colors preference.
    pub fn set_forced_colors(&self, v: bool) -> Result<()> {
        self.set_bool(KEY_FORCED_COLORS, v)
    }

    /// Cursor magnification level.
    pub fn cursor_size(&self) -> CursorSize {
        CursorSize::parse(&self.get_string(KEY_CURSOR_SIZE, DEFAULT_CURSOR_SIZE))
    }

    /// Set cursor magnification level.
    pub fn set_cursor_size(&self, size: CursorSize) -> Result<()> {
        self.set_str(KEY_CURSOR_SIZE, size.as_str())
    }

    /// Read all preferences into a snapshot value.
    pub fn snapshot(&self) -> A11yPrefsSnapshot {
        A11yPrefsSnapshot {
            font_size_multiplier: self.font_size_multiplier(),
            reduced_motion: self.reduced_motion(),
            forced_colors: self.forced_colors(),
            cursor_size: self.cursor_size(),
        }
    }

    /// Persist all fields from a snapshot in one call.
    pub fn apply_snapshot(&self, snap: &A11yPrefsSnapshot) -> Result<()> {
        self.set_font_size_multiplier(snap.font_size_multiplier)?;
        self.set_reduced_motion(snap.reduced_motion)?;
        self.set_forced_colors(snap.forced_colors)?;
        self.set_cursor_size(snap.cursor_size)?;
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> A11yPrefs {
        A11yPrefs::open_in_memory().expect("in-memory a11y_prefs")
    }

    #[test]
    fn defaults_are_correct() {
        let s = store();
        assert!((s.font_size_multiplier() - 1.0).abs() < f64::EPSILON);
        assert!(!s.reduced_motion());
        assert!(!s.forced_colors());
        assert_eq!(s.cursor_size(), CursorSize::Normal);
    }

    #[test]
    fn set_and_get_font_multiplier() {
        let s = store();
        s.set_font_size_multiplier(1.5).unwrap();
        assert!((s.font_size_multiplier() - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn reduced_motion_toggle() {
        let s = store();
        s.set_reduced_motion(true).unwrap();
        assert!(s.reduced_motion());
        s.set_reduced_motion(false).unwrap();
        assert!(!s.reduced_motion());
    }

    #[test]
    fn forced_colors_toggle() {
        let s = store();
        s.set_forced_colors(true).unwrap();
        assert!(s.forced_colors());
    }

    #[test]
    fn cursor_size_round_trip() {
        let s = store();
        for size in [CursorSize::Normal, CursorSize::Large, CursorSize::ExtraLarge] {
            s.set_cursor_size(size).unwrap();
            assert_eq!(s.cursor_size(), size);
        }
    }

    #[test]
    fn snapshot_apply_round_trip() {
        let s = store();
        let snap = A11yPrefsSnapshot {
            font_size_multiplier: 1.25,
            reduced_motion: true,
            forced_colors: false,
            cursor_size: CursorSize::Large,
        };
        s.apply_snapshot(&snap).unwrap();
        assert_eq!(s.snapshot(), snap);
    }
}
