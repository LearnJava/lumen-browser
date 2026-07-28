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

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_W: f32 = 760.0;
/// Panel height in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_H: f32 = 520.0;
/// Header bar height.
const HEADER_H: f32 = 38.0;
/// Tab bar height.
const TAB_BAR_H: f32 = 36.0;
/// Content area starts after header + tab bar.
const CONTENT_TOP: f32 = HEADER_H + TAB_BAR_H;
/// Horizontal padding.
const PAD_H: f32 = 16.0;
/// Height of one settings row.
const ROW_H: f32 = 44.0;
/// Width of the × close hit zone.
const CLOSE_W: f32 = 30.0;
/// Each tab's width — the tab bar is split evenly across all sections.
const TAB_W: f32 = PANEL_W / SettingsSection::ALL.len() as f32;
/// Vertical space reserved for a section's header label before its first row.
/// Shared by every `render_*`/`ht_*`/`tt_*` function so the three stay in sync
/// (content is rendered at `y + HEADER_GAP`, hit-test and tooltip zones must
/// subtract the same amount from the local `ly` before comparing row bounds).
const HEADER_GAP: f32 = 26.0;

// ── Section ───────────────────────────────────────────────────────────────────

/// The top-level settings sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    /// Homepage URL + default search engine.
    General,
    /// Shields, fingerprint mode, DoH, Tor status.
    Privacy,
    /// Font size, UI theme, tab-strip layout.
    Appearance,
    /// Default download directory + panel-layout reset.
    Downloads,
    /// HTTP/3 (QUIC) toggle.
    Network,
    /// Filter-list subscriptions (enable/disable, manual refresh).
    Adblock,
    /// Spellcheck dictionary status (read-only).
    Language,
}

impl SettingsSection {
    /// All sections in tab order.
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Privacy,
        Self::Appearance,
        Self::Downloads,
        Self::Network,
        Self::Adblock,
        Self::Language,
    ];

}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Which text input currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingInput {
    /// The homepage URL field.
    Homepage,
    /// The download directory path field.
    DownloadPath,
}

/// Settings panel UI state.
#[derive(Debug)]
pub struct SettingsPanel {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Active section tab.
    pub section: SettingsSection,
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
            section: SettingsSection::General,
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

/// Result of classifying a click inside the settings panel.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsHit {
    /// Click on the × close button.
    Close,
    /// Click on a section tab.
    TabSelect(SettingsSection),
    /// Click on the shields toggle.
    ToggleShields,
    /// Click on the DoH toggle.
    ToggleDoh,
    /// Click on a fingerprint mode option.
    SetFingerprintMode(String),
    /// Click on a base theme option (`"dark"`, `"light"`, `"system"`).
    SetTheme(String),
    /// Click on an accent-colour swatch.
    SetAccent(String),
    /// Click on the font-size decrease (−2 px) button.
    FontSizeDecrease,
    /// Click on the font-size increase (+2 px) button.
    FontSizeIncrease,
    /// Click on a tab-strip layout option (`"horizontal"` or `"vertical"`).
    SetTabLayout(String),
    /// Click on the homepage text field (focus it).
    FocusHomepage,
    /// Click on the download path text field (focus it).
    FocusDownloadPath,
    /// Click on the "reset panel layout" button.
    ResetPanelLayout,
    /// Click on the HTTP/3 toggle.
    ToggleHttp3,
    /// Click on a filter-list subscription's enable/disable toggle (by URL).
    ToggleSubscription(String),
    /// Click on the "refresh lists now" button.
    RefreshAdblockNow,
    /// Click inside the panel with no specific action.
    Inside,
    /// Click outside the panel.
    Outside,
}

/// Classify a click at `(mx, my)` in window CSS px. `(px, py)` is the panel
/// top-left corner.
pub fn hit_test(
    panel: &SettingsPanel,
    mx: f32,
    my: f32,
    px: f32,
    py: f32,
) -> SettingsHit {
    if mx < px || mx > px + PANEL_W || my < py || my > py + PANEL_H {
        return SettingsHit::Outside;
    }
    // Header zone.
    if my < py + HEADER_H {
        if mx >= px + PANEL_W - CLOSE_W {
            return SettingsHit::Close;
        }
        return SettingsHit::Inside;
    }
    // Tab bar zone.
    if my < py + HEADER_H + TAB_BAR_H {
        let idx = ((mx - px) / TAB_W).floor() as usize;
        if let Some(&sec) = SettingsSection::ALL.get(idx) {
            return SettingsHit::TabSelect(sec);
        }
        return SettingsHit::Inside;
    }
    // Content area — delegate by active section.
    let lx = mx - px;
    let ly = my - (py + CONTENT_TOP) + panel.scroll_y;
    match panel.section {
        SettingsSection::General => ht_general(lx, ly),
        SettingsSection::Privacy => ht_privacy(lx, ly),
        SettingsSection::Appearance => ht_appearance(lx, ly),
        SettingsSection::Downloads => ht_downloads(lx, ly),
        SettingsSection::Network => ht_network(lx, ly),
        SettingsSection::Adblock => ht_adblock(panel, lx, ly),
        SettingsSection::Language => SettingsHit::Inside,
    }
}

fn ht_general(lx: f32, ly: f32) -> SettingsHit {
    let _ = lx;
    let ly = ly - HEADER_GAP;
    // Row 0: label (not clickable); row 1: homepage input; row 2: search engine (not clickable).
    if (ROW_H..ROW_H * 2.0).contains(&ly) {
        return SettingsHit::FocusHomepage;
    }
    SettingsHit::Inside
}

fn ht_privacy(lx: f32, ly: f32) -> SettingsHit {
    let ly = ly - HEADER_GAP;
    // Row 0: shields toggle
    if ly < ROW_H {
        let toggle_x = PANEL_W - PAD_H - 60.0;
        if lx >= toggle_x { return SettingsHit::ToggleShields; }
        return SettingsHit::Inside;
    }
    // Row 1: fingerprint mode options
    if ly < ROW_H * 2.0 {
        if let Some(opt) = option_hit(lx, &["standard", "strict", "off"]) {
            return SettingsHit::SetFingerprintMode(opt);
        }
        return SettingsHit::Inside;
    }
    // Row 2: DoH toggle
    if ly < ROW_H * 3.0 {
        let toggle_x = PANEL_W - PAD_H - 60.0;
        if lx >= toggle_x { return SettingsHit::ToggleDoh; }
        return SettingsHit::Inside;
    }
    // Row 3: Tor status — read-only, no action.
    SettingsHit::Inside
}

fn ht_appearance(lx: f32, ly: f32) -> SettingsHit {
    let ly = ly - HEADER_GAP;
    // Row 0: font size: − / value / +
    if ly < ROW_H {
        let btn_w = 30.0;
        let val_w = 44.0;
        let right_end = PANEL_W - PAD_H;
        if lx >= right_end - btn_w { return SettingsHit::FontSizeIncrease; }
        if lx >= right_end - btn_w - val_w - btn_w && lx < right_end - btn_w - val_w {
            return SettingsHit::FontSizeDecrease;
        }
        return SettingsHit::Inside;
    }
    // Row 1: base theme options (dark / light / system)
    if ly < ROW_H * 2.0 {
        if let Some(opt) = option_hit(lx, &["dark", "light", "system"]) {
            return SettingsHit::SetTheme(opt);
        }
        return SettingsHit::Inside;
    }
    // Row 2: accent colour swatches (6 circles)
    if ly < ROW_H * 3.0 {
        use crate::panels::themes::AccentPreset;
        let swatch_sz = 22.0;
        let gap = 8.0;
        let start_x = PANEL_W / 2.0;
        for (i, preset) in AccentPreset::ALL.iter().enumerate() {
            let sx = start_x + i as f32 * (swatch_sz + gap);
            if lx >= sx && lx < sx + swatch_sz {
                return SettingsHit::SetAccent(preset.key().to_owned());
            }
        }
        return SettingsHit::Inside;
    }
    // Row 3: tab-strip layout options (horizontal / vertical)
    if ly < ROW_H * 4.0
        && let Some(opt) = option_hit(lx, &["horizontal", "vertical"])
    {
        return SettingsHit::SetTabLayout(opt);
    }
    SettingsHit::Inside
}

fn ht_downloads(lx: f32, ly: f32) -> SettingsHit {
    let ly = ly - HEADER_GAP;
    // Row 0: label; row 1: path input.
    if (ROW_H..ROW_H * 2.0).contains(&ly) {
        return SettingsHit::FocusDownloadPath;
    }
    // Row 2: hint text (not clickable). Row 3: reset-panel-layout button.
    if (ROW_H * 3.0..ROW_H * 4.0).contains(&ly) {
        let btn_x = PANEL_W - PAD_H - 140.0;
        if lx >= btn_x {
            return SettingsHit::ResetPanelLayout;
        }
    }
    SettingsHit::Inside
}

fn ht_network(lx: f32, ly: f32) -> SettingsHit {
    let ly = ly - HEADER_GAP;
    if ly < ROW_H {
        let toggle_x = PANEL_W - PAD_H - 60.0;
        if lx >= toggle_x { return SettingsHit::ToggleHttp3; }
    }
    SettingsHit::Inside
}

fn ht_adblock(panel: &SettingsPanel, lx: f32, ly: f32) -> SettingsHit {
    let ly = ly - HEADER_GAP;
    if ly < 0.0 {
        return SettingsHit::Inside;
    }
    let row_idx = (ly / ROW_H).floor() as usize;
    if row_idx < panel.adblock_subs.len() {
        let toggle_x = PANEL_W - PAD_H - 60.0;
        if lx >= toggle_x {
            return SettingsHit::ToggleSubscription(panel.adblock_subs[row_idx].url.clone());
        }
        return SettingsHit::Inside;
    }
    if row_idx == panel.adblock_subs.len() {
        let btn_x = PANEL_W / 2.0;
        if lx >= btn_x {
            return SettingsHit::RefreshAdblockNow;
        }
    }
    SettingsHit::Inside
}

/// Shared option-row hit test: returns the matching option value string when
/// `lx` falls within one of `options`' evenly divided cells on the right half
/// of the row, else `None`. Mirrors [`push_options`]' geometry exactly.
fn option_hit(lx: f32, options: &[&str]) -> Option<String> {
    let right_start = PANEL_W / 2.0;
    let opt_w = (PANEL_W / 2.0 - PAD_H) / options.len() as f32;
    for (i, &opt) in options.iter().enumerate() {
        let ox = right_start + i as f32 * opt_w;
        if lx >= ox && lx < ox + opt_w {
            return Some(opt.to_owned());
        }
    }
    None
}

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
        assert_eq!(p.section, SettingsSection::General);
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
    fn hit_outside_returns_outside() {
        let p = panel_at_origin();
        assert_eq!(hit_test(&p, 0.0, 0.0, 100.0, 100.0), SettingsHit::Outside);
    }

    #[test]
    fn hit_close_button() {
        let p = panel_at_origin();
        let px = 50.0;
        let py = 50.0;
        let mx = px + PANEL_W - 5.0;
        let my = py + HEADER_H / 2.0;
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::Close);
    }

    #[test]
    fn hit_tab_selects_section() {
        let p = panel_at_origin();
        let px = 0.0;
        let py = 0.0;
        let my = HEADER_H + TAB_BAR_H / 2.0;
        // Privacy tab is index 1.
        let mx = TAB_W * 1.5;
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::TabSelect(SettingsSection::Privacy));
    }

    #[test]
    fn hit_last_tab_selects_language() {
        let p = panel_at_origin();
        let my = HEADER_H + TAB_BAR_H / 2.0;
        let mx = TAB_W * 6.5;
        assert_eq!(hit_test(&p, mx, my, 0.0, 0.0), SettingsHit::TabSelect(SettingsSection::Language));
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

    // ── Content-row hit tests (regression: rows must line up with render) ───

    fn content_point(section: SettingsSection, local_ly: f32, local_lx: f32) -> (f32, f32, f32, f32) {
        // px=py=0 for simplicity; content starts at CONTENT_TOP.
        let _ = section;
        (local_lx, CONTENT_TOP + local_ly, 0.0, 0.0)
    }

    #[test]
    fn hit_general_homepage_input_matches_render_offset() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::General;
        // Homepage input row is HEADER_GAP + ROW_H .. HEADER_GAP + 2*ROW_H.
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + ROW_H + 5.0, 10.0);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::FocusHomepage);
    }

    #[test]
    fn hit_general_label_row_is_inside_not_focus() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::General;
        // Label row (row 0) must NOT trigger FocusHomepage.
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + 5.0, 10.0);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::Inside);
    }

    #[test]
    fn hit_privacy_shields_toggle_matches_render_offset() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Privacy;
        let toggle_x = PANEL_W - PAD_H - 30.0;
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + 30.0, toggle_x);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::ToggleShields);
    }

    #[test]
    fn hit_privacy_doh_toggle_bottom_of_row_matches() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Privacy;
        let toggle_x = PANEL_W - PAD_H - 30.0;
        // Bottom half of row 2 (previously fell through to the wrong branch).
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + ROW_H * 2.0 + 40.0, toggle_x);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::ToggleDoh);
    }

    #[test]
    fn hit_appearance_tab_layout_options() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Appearance;
        let vertical_x = PANEL_W - PAD_H - 10.0; // rightmost option ("vertical")
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + ROW_H * 3.0 + 20.0, vertical_x);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::SetTabLayout("vertical".to_owned()));
    }

    #[test]
    fn hit_downloads_reset_button() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Downloads;
        let btn_x = PANEL_W - PAD_H - 60.0;
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + ROW_H * 3.0 + 20.0, btn_x);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::ResetPanelLayout);
    }

    #[test]
    fn hit_network_http3_toggle() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Network;
        let toggle_x = PANEL_W - PAD_H - 30.0;
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + 20.0, toggle_x);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::ToggleHttp3);
    }

    #[test]
    fn hit_adblock_subscription_toggle_and_refresh_button() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Adblock;
        p.set_adblock_subs(vec![
            Subscription { url: "https://a/list.txt".into(), title: "A".into(), enabled: true },
            Subscription { url: "https://b/list.txt".into(), title: "B".into(), enabled: false },
        ]);
        let toggle_x = PANEL_W - PAD_H - 30.0;
        // Second subscription's toggle (row index 1).
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + ROW_H + 20.0, toggle_x);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::ToggleSubscription("https://b/list.txt".to_owned()));
        // Refresh button row (row index == subs.len()).
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + ROW_H * 2.0 + 20.0, PANEL_W - PAD_H - 10.0);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::RefreshAdblockNow);
    }

    #[test]
    fn hit_language_section_has_no_actions() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Language;
        let (mx, my, px, py) = content_point(p.section, HEADER_GAP + 10.0, PANEL_W - 10.0);
        assert_eq!(hit_test(&p, mx, my, px, py), SettingsHit::Inside);
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
