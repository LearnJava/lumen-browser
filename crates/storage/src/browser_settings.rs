//! Persistent browser settings backed by SQLite.
//!
//! A thin key-value table `browser_settings` with one row per setting name.
//! Covers four sections exposed by the settings panel (D-7):
//!
//! - **General**: homepage URL, default search engine ID.
//! - **Privacy**: shields enabled, fingerprint resistance mode, DoH enabled.
//! - **Appearance**: base font size (px), UI theme name.
//! - **Downloads**: default download directory path.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

use crate::migrations::{run_migrations, set_common_pragmas, Migration};

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: "CREATE TABLE IF NOT EXISTS browser_settings (
                key   TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );",
}];

// ── Setting keys (keep in sync with SettingsSection in settings_panel) ─────

const KEY_HOMEPAGE: &str = "homepage";
const KEY_SEARCH_ENGINE_ID: &str = "search_engine_id";
const KEY_SHIELDS_ENABLED: &str = "shields_enabled";
const KEY_FINGERPRINT_MODE: &str = "fingerprint_mode";
const KEY_DOH_ENABLED: &str = "doh_enabled";
const KEY_FONT_SIZE: &str = "font_size";
const KEY_THEME: &str = "theme";
const KEY_DOWNLOAD_PATH: &str = "download_path";
const KEY_TAB_LAYOUT: &str = "tab_layout";
const KEY_PANEL_LAYOUT: &str = "panel_layout";
const KEY_TRANSLATE_LANGUAGE: &str = "translate_language";
const KEY_TRANSLATE_MODEL: &str = "translate_model";
const KEY_COMPAT_PACK_OFF: &str = "compat_pack_off";
const KEY_COMPAT_DISABLED: &str = "compat_disabled_ids";
const KEY_COMPAT_JOURNAL: &str = "compat_journal";

/// The compat-pack change journal keeps only the newest this-many lines.
pub const COMPAT_JOURNAL_MAX: usize = 50;

// ── Defaults ────────────────────────────────────────────────────────────────

const DEFAULT_HOMEPAGE: &str = "about:blank";
const DEFAULT_SEARCH_ENGINE_ID: i64 = 1;
// Off by default (user decision 2026-09-23): the ad-blocker changes what a
// page loads, so with it on Lumen is not comparable with a stock browser —
// the user opts in through "Блокировать рекламу" in settings.
const DEFAULT_SHIELDS_ENABLED: bool = false;
const DEFAULT_FINGERPRINT_MODE: &str = "standard";
const DEFAULT_DOH_ENABLED: bool = false;
const DEFAULT_FONT_SIZE: f64 = 16.0;
const DEFAULT_THEME: &str = "dark";
const DEFAULT_DOWNLOAD_PATH: &str = "";
const DEFAULT_TAB_LAYOUT: &str = "horizontal";
const DEFAULT_PANEL_LAYOUT: &str = "";
const DEFAULT_TRANSLATE_LANGUAGE: &str = "ru";
const DEFAULT_TRANSLATE_MODEL: &str = "";

/// All browser settings in a single value type for easy read/write.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserSettingsSnapshot {
    /// Start page / new-tab URL.
    pub homepage: String,
    /// ID of the default `SearchProviderEntry`; 0 = none configured.
    pub search_engine_id: i64,
    /// Whether the shields (tracker/ad blocker) are globally enabled.
    pub shields_enabled: bool,
    /// Fingerprint resistance mode: `"standard"`, `"strict"`, or `"off"`.
    pub fingerprint_mode: String,
    /// Whether DNS-over-HTTPS is enabled globally.
    pub doh_enabled: bool,
    /// Base font size in CSS pixels (e.g. 16.0).
    pub font_size: f64,
    /// UI theme name: `"dark"`, `"light"`, or `"system"`.
    pub theme: String,
    /// Absolute path to the default download directory. Empty = OS default.
    pub download_path: String,
    /// Tab layout mode: `"horizontal"` (default top strip) or `"vertical"` (left sidebar).
    ///
    /// Serialised as a string so future modes can be added without a schema migration.
    pub tab_layout: String,
    /// Serialised docked-panel layout (slot assignment + per-slot size +
    /// visibility), as produced by `SurfaceManager::serialize_layout`.
    ///
    /// Empty string means no saved layout — the shell uses built-in defaults.
    pub panel_layout: String,
}

impl Default for BrowserSettingsSnapshot {
    fn default() -> Self {
        Self {
            homepage: DEFAULT_HOMEPAGE.to_owned(),
            search_engine_id: DEFAULT_SEARCH_ENGINE_ID,
            shields_enabled: DEFAULT_SHIELDS_ENABLED,
            fingerprint_mode: DEFAULT_FINGERPRINT_MODE.to_owned(),
            doh_enabled: DEFAULT_DOH_ENABLED,
            font_size: DEFAULT_FONT_SIZE,
            theme: DEFAULT_THEME.to_owned(),
            download_path: DEFAULT_DOWNLOAD_PATH.to_owned(),
            tab_layout: DEFAULT_TAB_LAYOUT.to_owned(),
            panel_layout: DEFAULT_PANEL_LAYOUT.to_owned(),
        }
    }
}

/// Persistent settings store.
pub struct BrowserSettings {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for BrowserSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserSettings").finish_non_exhaustive()
    }
}

impl BrowserSettings {
    fn init(mut conn: Connection) -> Result<Self> {
        set_common_pragmas(&conn)
            .map_err(|e| Error::Storage(format!("browser_settings pragmas: {e}")))?;
        run_migrations(&mut conn, MIGRATIONS)
            .map_err(|e| Error::Storage(format!("browser_settings init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Open (or create) an on-disk settings database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| Error::Storage(e.to_string()))?;
        Self::init(conn)
    }

    /// Create an in-memory settings database (for tests / ephemeral sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| Error::Storage(e.to_string()))?;
        Self::init(conn)
    }

    // ── Low-level helpers ────────────────────────────────────────────────────

    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
    fn get_str(&self, key: &str, default: &str) -> String {
        let conn = self.conn.lock().expect("settings lock");
        conn.query_row(
            "SELECT value FROM browser_settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| default.to_owned())
    }

    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
    fn set_str(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().expect("settings lock");
        conn.execute(
            "INSERT INTO browser_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    fn get_bool(&self, key: &str, default: bool) -> bool {
        let s = self.get_str(key, if default { "1" } else { "0" });
        s == "1"
    }

    fn set_bool(&self, key: &str, value: bool) -> Result<()> {
        self.set_str(key, if value { "1" } else { "0" })
    }

    fn get_i64(&self, key: &str, default: i64) -> i64 {
        self.get_str(key, &default.to_string())
            .parse()
            .unwrap_or(default)
    }

    fn set_i64(&self, key: &str, value: i64) -> Result<()> {
        self.set_str(key, &value.to_string())
    }

    fn get_f64(&self, key: &str, default: f64) -> f64 {
        self.get_str(key, &default.to_string())
            .parse()
            .unwrap_or(default)
    }

    fn set_f64(&self, key: &str, value: f64) -> Result<()> {
        self.set_str(key, &value.to_string())
    }

    // ── Public typed accessors ───────────────────────────────────────────────

    /// Homepage / new-tab URL.
    pub fn homepage(&self) -> String {
        self.get_str(KEY_HOMEPAGE, DEFAULT_HOMEPAGE)
    }

    /// Set homepage URL.
    pub fn set_homepage(&self, url: &str) -> Result<()> {
        self.set_str(KEY_HOMEPAGE, url)
    }

    /// ID of the default search engine (`SearchProviderEntry::id`).
    pub fn search_engine_id(&self) -> i64 {
        self.get_i64(KEY_SEARCH_ENGINE_ID, DEFAULT_SEARCH_ENGINE_ID)
    }

    /// Set default search engine ID.
    pub fn set_search_engine_id(&self, id: i64) -> Result<()> {
        self.set_i64(KEY_SEARCH_ENGINE_ID, id)
    }

    /// Whether shields (tracker blocker) are globally enabled.
    pub fn shields_enabled(&self) -> bool {
        self.get_bool(KEY_SHIELDS_ENABLED, DEFAULT_SHIELDS_ENABLED)
    }

    /// Set shields on/off.
    pub fn set_shields_enabled(&self, enabled: bool) -> Result<()> {
        self.set_bool(KEY_SHIELDS_ENABLED, enabled)
    }

    /// Fingerprint resistance mode: `"standard"`, `"strict"`, or `"off"`.
    pub fn fingerprint_mode(&self) -> String {
        self.get_str(KEY_FINGERPRINT_MODE, DEFAULT_FINGERPRINT_MODE)
    }

    /// Set fingerprint resistance mode.
    pub fn set_fingerprint_mode(&self, mode: &str) -> Result<()> {
        self.set_str(KEY_FINGERPRINT_MODE, mode)
    }

    /// Whether DNS-over-HTTPS is enabled.
    pub fn doh_enabled(&self) -> bool {
        self.get_bool(KEY_DOH_ENABLED, DEFAULT_DOH_ENABLED)
    }

    /// Set DNS-over-HTTPS on/off.
    pub fn set_doh_enabled(&self, enabled: bool) -> Result<()> {
        self.set_bool(KEY_DOH_ENABLED, enabled)
    }

    /// Base font size in CSS px (e.g. 16.0).
    pub fn font_size(&self) -> f64 {
        self.get_f64(KEY_FONT_SIZE, DEFAULT_FONT_SIZE)
    }

    /// Set base font size.
    pub fn set_font_size(&self, px: f64) -> Result<()> {
        self.set_f64(KEY_FONT_SIZE, px)
    }

    /// UI theme: `"dark"`, `"light"`, or `"system"`.
    pub fn theme(&self) -> String {
        self.get_str(KEY_THEME, DEFAULT_THEME)
    }

    /// Set UI theme.
    pub fn set_theme(&self, theme: &str) -> Result<()> {
        self.set_str(KEY_THEME, theme)
    }

    /// Absolute path to the default download directory. Empty = OS default.
    pub fn download_path(&self) -> String {
        self.get_str(KEY_DOWNLOAD_PATH, DEFAULT_DOWNLOAD_PATH)
    }

    /// Set default download directory path.
    pub fn set_download_path(&self, path: &str) -> Result<()> {
        self.set_str(KEY_DOWNLOAD_PATH, path)
    }

    /// Tab layout mode: `"horizontal"` or `"vertical"` (GG-4).
    pub fn tab_layout(&self) -> String {
        self.get_str(KEY_TAB_LAYOUT, DEFAULT_TAB_LAYOUT)
    }

    /// Set tab layout mode.
    pub fn set_tab_layout(&self, mode: &str) -> Result<()> {
        self.set_str(KEY_TAB_LAYOUT, mode)
    }

    /// Serialised docked-panel layout string (F2-6c); empty = built-in defaults.
    pub fn panel_layout(&self) -> String {
        self.get_str(KEY_PANEL_LAYOUT, DEFAULT_PANEL_LAYOUT)
    }

    /// Persist the serialised docked-panel layout.
    pub fn set_panel_layout(&self, layout: &str) -> Result<()> {
        self.set_str(KEY_PANEL_LAYOUT, layout)
    }

    /// Target language code of page translation (BCP 47, e.g. `"ru"`).
    pub fn translate_language(&self) -> String {
        self.get_str(KEY_TRANSLATE_LANGUAGE, DEFAULT_TRANSLATE_LANGUAGE)
    }

    /// Set the page-translation target language.
    pub fn set_translate_language(&self, code: &str) -> Result<()> {
        self.set_str(KEY_TRANSLATE_LANGUAGE, code)
    }

    /// Local model name used for page translation. Empty = built-in default.
    pub fn translate_model(&self) -> String {
        self.get_str(KEY_TRANSLATE_MODEL, DEFAULT_TRANSLATE_MODEL)
    }

    /// Set the page-translation model name.
    pub fn set_translate_model(&self, model: &str) -> Result<()> {
        self.set_str(KEY_TRANSLATE_MODEL, model)
    }

    /// «Do not use the Lumen list» (`about:compat`, COMPAT-PACK-PAGE): when set, no compat-pack
    /// record applies on any host. Off by default.
    pub fn compat_pack_off(&self) -> bool {
        self.get_bool(KEY_COMPAT_PACK_OFF, false)
    }

    /// Set «do not use the Lumen list».
    pub fn set_compat_pack_off(&self, off: bool) -> Result<()> {
        self.set_bool(KEY_COMPAT_PACK_OFF, off)
    }

    /// Ids of compat-pack records the user switched off one by one (pack order is not kept).
    pub fn compat_disabled_ids(&self) -> Vec<String> {
        self.get_str(KEY_COMPAT_DISABLED, "")
            .split(',')
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Switch one compat-pack record off (`disabled`) or back on. Ids are `[a-z0-9_-]`
    /// (`CompatPack::validate`), so a comma-joined list is unambiguous; anything else is refused.
    pub fn set_compat_entry_disabled(&self, id: &str, disabled: bool) -> Result<()> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
            return Err(Error::Storage(format!("compat record id {id:?} is not [a-z0-9_-]+")));
        }
        let mut ids = self.compat_disabled_ids();
        ids.retain(|d| d != id);
        if disabled {
            ids.push(id.to_owned());
        }
        self.set_str(KEY_COMPAT_DISABLED, &ids.join(","))
    }

    /// Change journal of the compat pack, oldest first: `(unix seconds, text)`.
    pub fn compat_journal(&self) -> Vec<(i64, String)> {
        self.get_str(KEY_COMPAT_JOURNAL, "")
            .lines()
            .filter_map(|line| {
                let (at, text) = line.split_once('\t')?;
                Some((at.parse().ok()?, text.to_owned()))
            })
            .collect()
    }

    /// Append one line to the compat-pack journal, keeping the newest [`COMPAT_JOURNAL_MAX`].
    pub fn append_compat_journal(&self, unix: i64, text: &str) -> Result<()> {
        let mut lines = self.compat_journal();
        let clean: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        lines.push((unix, clean));
        let skip = lines.len().saturating_sub(COMPAT_JOURNAL_MAX);
        let joined: Vec<String> = lines.iter().skip(skip).map(|(at, t)| format!("{at}\t{t}")).collect();
        self.set_str(KEY_COMPAT_JOURNAL, &joined.join("\n"))
    }

    /// Read all settings into a snapshot value.
    pub fn snapshot(&self) -> BrowserSettingsSnapshot {
        BrowserSettingsSnapshot {
            homepage: self.homepage(),
            search_engine_id: self.search_engine_id(),
            shields_enabled: self.shields_enabled(),
            fingerprint_mode: self.fingerprint_mode(),
            doh_enabled: self.doh_enabled(),
            font_size: self.font_size(),
            theme: self.theme(),
            download_path: self.download_path(),
            tab_layout: self.tab_layout(),
            panel_layout: self.panel_layout(),
        }
    }

    /// Persist all fields from a snapshot in one call.
    pub fn apply_snapshot(&self, snap: &BrowserSettingsSnapshot) -> Result<()> {
        self.set_homepage(&snap.homepage)?;
        self.set_search_engine_id(snap.search_engine_id)?;
        self.set_shields_enabled(snap.shields_enabled)?;
        self.set_fingerprint_mode(&snap.fingerprint_mode)?;
        self.set_doh_enabled(snap.doh_enabled)?;
        self.set_font_size(snap.font_size)?;
        self.set_theme(&snap.theme)?;
        self.set_download_path(&snap.download_path)?;
        self.set_tab_layout(&snap.tab_layout)?;
        self.set_panel_layout(&snap.panel_layout)?;
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> BrowserSettings {
        BrowserSettings::open_in_memory().expect("in-memory settings")
    }

    #[test]
    fn compat_switches_persist_and_validate() {
        let s = store();
        assert!(!s.compat_pack_off());
        assert!(s.compat_disabled_ids().is_empty());
        s.set_compat_pack_off(true).unwrap();
        s.set_compat_entry_disabled("a-css", true).unwrap();
        s.set_compat_entry_disabled("b_flag", true).unwrap();
        s.set_compat_entry_disabled("a-css", true).unwrap();
        assert!(s.compat_pack_off());
        assert_eq!(s.compat_disabled_ids(), ["b_flag", "a-css"]);
        s.set_compat_entry_disabled("b_flag", false).unwrap();
        assert_eq!(s.compat_disabled_ids(), ["a-css"]);
        assert!(s.set_compat_entry_disabled("a,b", true).is_err());
        assert!(s.set_compat_entry_disabled("", true).is_err());
    }

    #[test]
    fn compat_journal_is_capped_and_single_line() {
        let s = store();
        for i in 0..(COMPAT_JOURNAL_MAX + 5) {
            s.append_compat_journal(i as i64, &format!("запись {i}\nвторая строка")).unwrap();
        }
        let j = s.compat_journal();
        assert_eq!(j.len(), COMPAT_JOURNAL_MAX);
        assert_eq!(j[0].0, 5);
        assert_eq!(j[0].1, "запись 5 вторая строка");
        assert_eq!(j.last().unwrap().0, (COMPAT_JOURNAL_MAX + 4) as i64);
    }

    #[test]
    fn defaults_match_spec() {
        let s = store();
        assert_eq!(s.homepage(), "about:blank");
        assert_eq!(s.search_engine_id(), 1);
        assert!(!s.shields_enabled());
        assert_eq!(s.fingerprint_mode(), "standard");
        assert!(!s.doh_enabled());
        assert!((s.font_size() - 16.0).abs() < f64::EPSILON);
        assert_eq!(s.theme(), "dark");
        assert_eq!(s.download_path(), "");
    }

    #[test]
    fn translate_settings_default_and_round_trip() {
        let s = store();
        assert_eq!(s.translate_language(), "ru");
        assert_eq!(s.translate_model(), "");
        s.set_translate_language("de").unwrap();
        s.set_translate_model("llama3").unwrap();
        assert_eq!(s.translate_language(), "de");
        assert_eq!(s.translate_model(), "llama3");
    }

    #[test]
    fn set_and_get_homepage() {
        let s = store();
        s.set_homepage("https://example.com").unwrap();
        assert_eq!(s.homepage(), "https://example.com");
    }

    #[test]
    fn set_and_get_search_engine_id() {
        let s = store();
        s.set_search_engine_id(42).unwrap();
        assert_eq!(s.search_engine_id(), 42);
    }

    #[test]
    fn shields_toggle() {
        let s = store();
        s.set_shields_enabled(false).unwrap();
        assert!(!s.shields_enabled());
        s.set_shields_enabled(true).unwrap();
        assert!(s.shields_enabled());
    }

    #[test]
    fn fingerprint_mode_round_trip() {
        let s = store();
        for mode in &["standard", "strict", "off"] {
            s.set_fingerprint_mode(mode).unwrap();
            assert_eq!(&s.fingerprint_mode(), mode);
        }
    }

    #[test]
    fn doh_toggle() {
        let s = store();
        s.set_doh_enabled(true).unwrap();
        assert!(s.doh_enabled());
    }

    #[test]
    fn font_size_round_trip() {
        let s = store();
        s.set_font_size(20.0).unwrap();
        assert!((s.font_size() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn theme_round_trip() {
        let s = store();
        s.set_theme("light").unwrap();
        assert_eq!(s.theme(), "light");
    }

    #[test]
    fn download_path_round_trip() {
        let s = store();
        s.set_download_path("/home/user/Downloads").unwrap();
        assert_eq!(s.download_path(), "/home/user/Downloads");
    }

    #[test]
    fn snapshot_apply_round_trip() {
        let s = store();
        let snap = BrowserSettingsSnapshot {
            homepage: "https://lumen.local".to_owned(),
            search_engine_id: 3,
            shields_enabled: false,
            fingerprint_mode: "strict".to_owned(),
            doh_enabled: true,
            font_size: 14.0,
            theme: "light".to_owned(),
            download_path: "/tmp/dl".to_owned(),
            tab_layout: "vertical".to_owned(),
            panel_layout: "1\npanel sidebar right 1\n".to_owned(),
        };
        s.apply_snapshot(&snap).unwrap();
        assert_eq!(s.snapshot(), snap);
    }

    #[test]
    fn tab_layout_default_is_horizontal() {
        let s = store();
        assert_eq!(s.tab_layout(), "horizontal");
    }

    #[test]
    fn tab_layout_round_trip() {
        let s = store();
        s.set_tab_layout("vertical").unwrap();
        assert_eq!(s.tab_layout(), "vertical");
        s.set_tab_layout("horizontal").unwrap();
        assert_eq!(s.tab_layout(), "horizontal");
    }

    #[test]
    fn panel_layout_default_is_empty() {
        assert_eq!(store().panel_layout(), "");
    }

    #[test]
    fn panel_layout_round_trip() {
        let s = store();
        s.set_panel_layout("1\npanel sidebar right 1\n").unwrap();
        assert_eq!(s.panel_layout(), "1\npanel sidebar right 1\n");
    }
}
