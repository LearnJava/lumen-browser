//! Browser settings panel (D-7).
//!
//! A full-page centred overlay opened by `Ctrl+,`, the settings gear button in
//! the tab strip, or by navigating to `about:settings`. Seven tabbed sections:
//!
//! - **General** — homepage URL, default search engine ID.
//! - **Privacy** — shields on/off, fingerprint mode, DoH on/off, Tor status (read-only).
//! - **Appearance** — base font size (px), UI theme, tab-strip layout.
//! - **Downloads** — default download directory path, panel-layout reset.
//! - **Network** — HTTP/3 (QUIC) toggle (persisted to `fingerprint.toml`).
//! - **Adblock** — enable/disable each filter-list subscription, manual refresh.
//! - **Language** — read-only spellcheck dictionary status.
//!
//! State is split between [`SettingsPanel`] (UI/UX) and
//! `lumen_storage::BrowserSettings` (persistence). The panel holds a
//! [`lumen_storage::BrowserSettingsSnapshot`] as a working draft. On close the
//! caller persists it via `BrowserSettings::apply_snapshot`. The HTTP/3 toggle
//! and the ad-block subscription list live in separate stores (`fingerprint.toml`
//! and `AdblockStore` respectively) and are threaded through dedicated setters
//! (see [`SettingsPanel::set_http3`], [`SettingsPanel::set_adblock_subs`]).
//!
//! Hit testing: [`hit_test`]. The legacy display-list renderer and its hover
//! tooltips were removed in CC-15-4 - under the engine chrome the panel is
//! `#view-settings`.

use lumen_storage::adblock::Subscription;
use lumen_storage::BrowserSettingsSnapshot;

// ── Panel state ───────────────────────────────────────────────────────────────

/// Which text input currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingInput {
    /// The homepage URL field.
    #[allow(dead_code, reason = "BUG-421: элементы управления настройками ещё не перенесены в движковый #view-settings")]
    Homepage,
    /// The download directory path field.
    #[allow(dead_code, reason = "BUG-421: элементы управления настройками ещё не перенесены в движковый #view-settings")]
    DownloadPath,
}

/// Settings panel UI state.
#[derive(Debug)]
pub struct SettingsPanel {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Pending edits not yet written to `BrowserSettings`.
    pub draft: BrowserSettingsSnapshot,
    /// Focused text input field, if any.
    pub focused_input: Option<SettingInput>,
    /// Vertical scroll offset within the content area.
    pub scroll_y: f32,
    /// HTTP/3 (QUIC) draft toggle. Persisted separately, to `fingerprint.toml`
    /// — it is not part of [`BrowserSettingsSnapshot`], which lives in a
    /// different store. Populated on open via [`Self::set_http3`]; applied by
    /// the caller (`crate::config::set_http3`) on panel close.
    pub http3_draft: bool,
    /// Whether Tor mode is active for this session (read-only status — Tor is
    /// wired only via the `--tor` CLI flag at startup, not toggleable here).
    pub tor_active: bool,
    /// Ad-block filter-list subscriptions shown in the Adblock section.
    /// Refreshed from the `AdblockStore` on panel open and after every toggle
    /// via [`Self::set_adblock_subs`].
    pub adblock_subs: Vec<Subscription>,
    /// Locale string of the loaded spellcheck dictionaries (e.g.
    /// `"en_US+ru_RU"`), or `None` while the background loader hasn't
    /// finished yet.
    pub spell_locale: Option<String>,
}

impl SettingsPanel {
    /// Create a new, hidden panel.
    pub fn new() -> Self {
        Self {
            visible: false,
            draft: BrowserSettingsSnapshot::default(),
            focused_input: None,
            scroll_y: 0.0,
            http3_draft: false,
            tor_active: false,
            adblock_subs: Vec::new(),
            spell_locale: None,
        }
    }

    /// Open the panel, loading a fresh snapshot as the working draft.
    ///
    /// Only covers `BrowserSettingsSnapshot`-backed fields — callers must
    /// follow up with [`Self::set_http3`], [`Self::set_tor_active`],
    /// [`Self::set_adblock_subs`], and [`Self::set_spell_locale`] to populate
    /// the sections backed by other stores.
    pub fn open(&mut self, snap: BrowserSettingsSnapshot) {
        self.visible = true;
        self.draft = snap;
        self.focused_input = None;
        self.scroll_y = 0.0;
    }

    /// Toggle visibility. When opening, loads `snap` as the draft.
    #[allow(dead_code)]
    pub fn toggle(&mut self, snap: BrowserSettingsSnapshot) {
        if self.visible {
            self.visible = false;
        } else {
            self.open(snap);
        }
    }

    /// Clone the current draft for persistence.
    pub fn apply_draft(&self) -> BrowserSettingsSnapshot {
        self.draft.clone()
    }

    /// Populate the HTTP/3 draft toggle (Network section) from the currently
    /// active [`crate::config::FingerprintProfile`].
    pub fn set_http3(&mut self, enabled: bool) {
        self.http3_draft = enabled;
    }

    /// Populate the read-only Tor status line (Privacy section).
    pub fn set_tor_active(&mut self, active: bool) {
        self.tor_active = active;
    }

    /// Populate the Adblock section's subscription list.
    pub fn set_adblock_subs(&mut self, subs: Vec<Subscription>) {
        self.adblock_subs = subs;
    }

    /// Populate the read-only spellcheck locale line (Language section).
    pub fn set_spell_locale(&mut self, locale: Option<String>) {
        self.spell_locale = locale;
    }

    /// Append a printable character to the focused text field.
    pub fn append_char(&mut self, ch: char) {
        match self.focused_input {
            Some(SettingInput::Homepage) => self.draft.homepage.push(ch),
            Some(SettingInput::DownloadPath) => self.draft.download_path.push(ch),
            None => {}
        }
    }

    /// Remove the last character from the focused text field.
    pub fn backspace(&mut self) {
        match self.focused_input {
            Some(SettingInput::Homepage) => { self.draft.homepage.pop(); }
            Some(SettingInput::DownloadPath) => { self.draft.download_path.pop(); }
            None => {}
        }
    }

    /// Scroll the content area by `dy` CSS px (positive = down).
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_y = (self.scroll_y + dy).max(0.0);
    }
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit testing ───────────────────────────────────────────────────────────────

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn panel_at_origin() -> SettingsPanel {
        let mut p = SettingsPanel::new();
        p.open(BrowserSettingsSnapshot::default());
        p
    }

    #[test]
    fn default_panel_hidden() {
        let p = SettingsPanel::new();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_opens_and_closes() {
        let mut p = SettingsPanel::new();
        let snap = BrowserSettingsSnapshot::default();
        p.toggle(snap.clone());
        assert!(p.visible);
        p.toggle(snap);
        assert!(!p.visible);
    }

    #[test]
    fn append_char_updates_homepage() {
        let mut p = panel_at_origin();
        p.focused_input = Some(SettingInput::Homepage);
        p.draft.homepage = "http://".to_owned();
        p.append_char('a');
        assert_eq!(p.draft.homepage, "http://a");
    }

    #[test]
    fn backspace_removes_char() {
        let mut p = panel_at_origin();
        p.focused_input = Some(SettingInput::Homepage);
        p.draft.homepage = "https://x".to_owned();
        p.backspace();
        assert_eq!(p.draft.homepage, "https://");
    }

    #[test]
    fn apply_draft_returns_current_draft() {
        let mut p = panel_at_origin();
        p.draft.theme = "light".to_owned();
        let snap = p.apply_draft();
        assert_eq!(snap.theme, "light");
    }

    // ── New setters ───────────────────────────────────────────────────────────

    #[test]
    fn setters_populate_extra_fields() {
        let mut p = SettingsPanel::new();
        p.set_http3(true);
        p.set_tor_active(true);
        p.set_spell_locale(Some("en_US+ru_RU".to_owned()));
        p.set_adblock_subs(vec![Subscription { url: "u".into(), title: "T".into(), enabled: true }]);
        assert!(p.http3_draft);
        assert!(p.tor_active);
        assert_eq!(p.spell_locale.as_deref(), Some("en_US+ru_RU"));
        assert_eq!(p.adblock_subs.len(), 1);
    }

    #[test]
    fn scroll_by_clamps_at_zero() {
        let mut p = SettingsPanel::new();
        p.scroll_by(-50.0);
        assert!((p.scroll_y - 0.0).abs() < f32::EPSILON);
        p.scroll_by(30.0);
        assert!((p.scroll_y - 30.0).abs() < f32::EPSILON);
    }
}
