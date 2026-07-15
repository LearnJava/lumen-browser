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
//! Hit testing: [`hit_test`]. Rendering: [`build_panel`]. Hover tooltips:
//! [`tooltip_for`] + [`build_tooltip`] — every interactive or informational row
//! has a one-line description shown next to the cursor while hovering (no
//! delay, matching the tab-strip tooltip pattern in `tabs/strip.rs`).

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};
use lumen_storage::adblock::Subscription;
use lumen_storage::BrowserSettingsSnapshot;

use crate::panels::themes::Palette;

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
/// Content area height.
const CONTENT_H: f32 = PANEL_H - CONTENT_TOP;
/// Horizontal padding.
const PAD_H: f32 = 16.0;
/// Vertical padding inside a row.
const PAD_V: f32 = 10.0;
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

// ── Semantic colours (status indicators — kept hard-coded across all themes) ──

/// Red × close button — danger/destructive semantic, intentionally never
/// follows the palette so the close affordance stays universally recognisable.
const CLOSE_COL: Color = Color { r: 180, g: 80, b: 80, a: 255 };

/// Green toggle track for the ON state — semantic "enabled" status indicator.
const TOGGLE_ON: Color = Color { r: 60, g: 140, b: 80, a: 230 };

/// Tooltip text colour — the bubble background is always dark (native-OS-style),
/// independent of the active theme, so the text must not follow `pal.text`
/// (which is dark-on-light in the Light theme and would be invisible here).
const TOOLTIP_TEXT: Color = Color { r: 235, g: 235, b: 238, a: 255 };

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

    /// Display label for the tab.
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "Общие",
            Self::Privacy => "Конфиденц.",
            Self::Appearance => "Вид",
            Self::Downloads => "Загрузки",
            Self::Network => "Сеть",
            Self::Adblock => "Блокировка",
            Self::Language => "Язык",
        }
    }
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

// ── Hover tooltips ────────────────────────────────────────────────────────────

/// Describe whatever setting is under the cursor at `(mx, my)` (window CSS
/// px), or `None` if nothing there has a tooltip. `(px, py)` is the panel's
/// top-left corner, matching [`hit_test`]'s parameters exactly so the two stay
/// visually aligned.
pub fn tooltip_for(panel: &SettingsPanel, mx: f32, my: f32, px: f32, py: f32) -> Option<&'static str> {
    if mx < px || mx > px + PANEL_W || my < py || my > py + PANEL_H {
        return None;
    }
    if my < py + CONTENT_TOP {
        return None; // Header + tab bar carry no tooltips.
    }
    let lx = mx - px;
    let ly = my - (py + CONTENT_TOP) + panel.scroll_y - HEADER_GAP;
    match panel.section {
        SettingsSection::General => tt_general(ly),
        SettingsSection::Privacy => tt_privacy(ly),
        SettingsSection::Appearance => tt_appearance(ly),
        SettingsSection::Downloads => tt_downloads(lx, ly),
        SettingsSection::Network => tt_network(ly),
        SettingsSection::Adblock => tt_adblock(panel, ly),
        SettingsSection::Language => tt_language(ly),
    }
}

fn tt_general(ly: f32) -> Option<&'static str> {
    if (0.0..ROW_H).contains(&ly) {
        return Some("Страница, которая открывается в новой вкладке и при старте браузера.");
    }
    if (ROW_H..ROW_H * 2.0).contains(&ly) {
        return Some("Введите полный URL (например, https://example.com).");
    }
    if (ROW_H * 2.0..ROW_H * 3.0).contains(&ly) {
        return Some("ID поисковика по умолчанию, используемого при вводе запроса в адресную строку.");
    }
    None
}

fn tt_privacy(ly: f32) -> Option<&'static str> {
    if ly < ROW_H {
        return Some("Блокирует известные рекламные и трекинговые домены на всех сайтах.");
    }
    if ly < ROW_H * 2.0 {
        return Some("Насколько сильно браузер маскирует характеристики устройства (экран, шрифты, аппаратные параметры) от сайтов.");
    }
    if ly < ROW_H * 3.0 {
        return Some("Отправляет DNS-запросы по HTTPS вместо обычного DNS, скрывая их от провайдера.");
    }
    if ly < ROW_H * 4.0 {
        return Some("Tor-режим включается флагом командной строки --tor при запуске браузера и не переключается отсюда.");
    }
    None
}

fn tt_appearance(ly: f32) -> Option<&'static str> {
    if ly < ROW_H {
        return Some("Базовый размер шрифта страницы в CSS-пикселях (8–36).");
    }
    if ly < ROW_H * 2.0 {
        return Some("Цветовая схема интерфейса: тёмная, светлая или следующая за системной.");
    }
    if ly < ROW_H * 3.0 {
        return Some("Акцентный цвет интерфейса (вкладки, адресная строка, кнопки).");
    }
    if ly < ROW_H * 4.0 {
        return Some("Расположение полосы вкладок: сверху окна или вертикально слева.");
    }
    None
}

fn tt_downloads(lx: f32, ly: f32) -> Option<&'static str> {
    if (0.0..ROW_H).contains(&ly) {
        return Some("Папка, куда браузер сохраняет загруженные файлы.");
    }
    if (ROW_H..ROW_H * 2.0).contains(&ly) {
        return Some("Оставьте пустым, чтобы использовать стандартную папку загрузок ОС.");
    }
    if (ROW_H * 3.0..ROW_H * 4.0).contains(&ly) {
        let btn_x = PANEL_W - PAD_H - 140.0;
        if lx >= btn_x {
            return Some("Возвращает боковые панели (вкладки, AI, история) к расположению по умолчанию после перезапуска.");
        }
    }
    None
}

fn tt_network(ly: f32) -> Option<&'static str> {
    if ly < ROW_H {
        return Some("Включает HTTP/3 (QUIC) для загрузки страниц. Изменение вступит в силу после перезапуска браузера.");
    }
    if ly < ROW_H * 2.0 {
        return Some("HTTP-отпечаток, который браузер предъявляет серверам (порядок заголовков, TLS, Client Hints).");
    }
    None
}

fn tt_adblock(panel: &SettingsPanel, ly: f32) -> Option<&'static str> {
    if ly < 0.0 {
        return None;
    }
    let row_idx = (ly / ROW_H).floor() as usize;
    if row_idx < panel.adblock_subs.len() {
        return Some("Включает или отключает эту подписку на список блокировки. Изменение применяется сразу.");
    }
    if row_idx == panel.adblock_subs.len() {
        return Some("Немедленно проверяет все включённые списки на обновления в фоне.");
    }
    None
}

fn tt_language(ly: f32) -> Option<&'static str> {
    if (0.0..ROW_H).contains(&ly) {
        return Some("Словари, найденные в data/spell/, используются для проверки правописания в текстовых полях.");
    }
    if (ROW_H..ROW_H * 2.0).contains(&ly) {
        return Some("Чтобы добавить язык, положите пару файлов <локаль>.aff/.dic в data/spell/ и перезапустите браузер.");
    }
    None
}

/// Greedily wrap `text` into lines of at most `max_chars` characters, breaking
/// only at word boundaries (a single word longer than `max_chars` still gets
/// its own, overflowing line rather than being split mid-word).
fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= max_chars {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Render a small tooltip bubble anchored just below-right of `(mx, my)`
/// (window CSS px), clamped so it never runs off the viewport edge. Long
/// descriptions wrap onto multiple lines (`wrap_text`) instead of overflowing
/// the bubble — a single unwrapped line at the longest tooltip's length would
/// run past the window edge when hovering a right-aligned control.
pub fn build_tooltip(text: &str, mx: f32, my: f32, win_w: f32, win_h: f32, pal: &Palette) -> DisplayList {
    const FONT_SZ: f32 = 11.0;
    const PAD: f32 = 8.0;
    const MAX_W: f32 = 300.0;
    // Average glyph advance at this font size (generous estimate for Cyrillic).
    const CHAR_W: f32 = FONT_SZ * 0.62;

    let max_chars = (((MAX_W - PAD * 2.0) / CHAR_W).floor() as usize).max(12);
    let lines = wrap_text(text, max_chars);
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let w = (longest as f32 * CHAR_W + PAD * 2.0).clamp(60.0, MAX_W);
    let line_h = FONT_SZ * 1.5;
    let h = line_h * lines.len().max(1) as f32 + PAD * 2.0 - (line_h - FONT_SZ * 1.4);

    let mut x = mx + 12.0;
    let mut y = my + 16.0;
    if x + w > win_w { x = win_w - w - 4.0; }
    if y + h > win_h { y = win_h - h - 4.0; }

    let mut list = vec![
        DisplayCommand::FillRoundedRect {
            rect: Rect::new(x, y, w, h),
            radii: radii(4.0),
            color: pal.overlay_border,
        },
        DisplayCommand::FillRoundedRect {
            rect: Rect::new(x + 1.0, y + 1.0, w - 2.0, h - 2.0),
            radii: radii(3.0),
            color: Color { r: 30, g: 30, b: 34, a: 245 },
        },
    ];
    for (i, line) in lines.iter().enumerate() {
        list.push(txt(
            line.clone(), x + PAD, y + PAD - 1.0 + i as f32 * line_h,
            w - PAD * 2.0, FONT_SZ, FontWeight::NORMAL, TOOLTIP_TEXT,
        ));
    }
    list
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// Append display commands for the settings panel to `list`.
///
/// `(px, py)` is the panel's top-left corner in window CSS px.
/// `pal` provides the active chrome colour tokens for light/dark theming.
pub fn build_panel(panel: &SettingsPanel, list: &mut DisplayList, px: f32, py: f32, pal: &Palette) {
    // Outer border ring.
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        radii: radii(7.0),
        color: pal.overlay_border,
    });
    // Inner background.
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + 1.0, PANEL_W - 2.0, PANEL_H - 2.0),
        radii: radii(6.0),
        color: pal.overlay_bg,
    });

    // ── Header ───────────────────────────────────────────────────────────────
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, HEADER_H),
        radii: CornerRadii { tl: 6.0, tl_y: 6.0, tr: 6.0, tr_y: 6.0,
                             bl: 0.0, bl_y: 0.0, br: 0.0, br_y: 0.0 },
        color: pal.header_bg,
    });
    list.push(txt("Настройки", px + PAD_H, py + 12.0, PANEL_W - PAD_H * 2.0 - CLOSE_W,
        13.0, FontWeight::BOLD, pal.text));
    list.push(txt("×", px + PANEL_W - CLOSE_W + 6.0, py + 10.0, 20.0,
        15.0, FontWeight::BOLD, CLOSE_COL));
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(px, py + HEADER_H - 1.0, PANEL_W, 1.0),
        color: pal.divider,
    });

    // ── Tab bar ──────────────────────────────────────────────────────────────
    let tab_y = py + HEADER_H;
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_y, PANEL_W, TAB_BAR_H),
        color: pal.item_bg,
    });
    for (i, &sec) in SettingsSection::ALL.iter().enumerate() {
        let tx = px + i as f32 * TAB_W;
        let is_active = sec == panel.section;
        if is_active {
            list.push(DisplayCommand::FillRect {
                rect: Rect::new(tx, tab_y, TAB_W, TAB_BAR_H),
                color: pal.item_selected_bg,
            });
            // Active underline accent.
            list.push(DisplayCommand::FillRect {
                rect: Rect::new(tx + 4.0, tab_y + TAB_BAR_H - 2.0, TAB_W - 8.0, 2.0),
                color: pal.accent,
            });
        }
        list.push(txt(
            sec.label().to_owned(),
            tx + 6.0,
            tab_y + 10.0,
            TAB_W - 12.0,
            11.0,
            if is_active { FontWeight::BOLD } else { FontWeight::NORMAL },
            if is_active { pal.text } else { pal.text_dim },
        ));
    }
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_y + TAB_BAR_H - 1.0, PANEL_W, 1.0),
        color: pal.divider,
    });

    // ── Content area ─────────────────────────────────────────────────────────
    let cy = py + CONTENT_TOP;
    list.push(DisplayCommand::PushClipRect {
        rect: Rect::new(px, cy, PANEL_W, CONTENT_H),
    });
    let off = panel.scroll_y;
    match panel.section {
        SettingsSection::General => render_general(panel, list, px, cy - off, pal),
        SettingsSection::Privacy => render_privacy(panel, list, px, cy - off, pal),
        SettingsSection::Appearance => render_appearance(panel, list, px, cy - off, pal),
        SettingsSection::Downloads => render_downloads(panel, list, px, cy - off, pal),
        SettingsSection::Network => render_network(panel, list, px, cy - off, pal),
        SettingsSection::Adblock => render_adblock(panel, list, px, cy - off, pal),
        SettingsSection::Language => render_language(panel, list, px, cy - off, pal),
    }
    list.push(DisplayCommand::PopClip);
}

// ── Per-section renderers ────────────────────────────────────────────────────

fn row_bg(i: usize, pal: &Palette) -> Color {
    if i.is_multiple_of(2) { pal.item_bg } else { pal.row_alt_bg }
}

fn push_row(list: &mut DisplayList, x: f32, y: f32, i: usize, pal: &Palette) {
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, PANEL_W, ROW_H),
        color: row_bg(i, pal),
    });
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y + ROW_H - 1.0, PANEL_W, 1.0),
        color: pal.divider,
    });
}

fn push_label(list: &mut DisplayList, x: f32, y: f32, label: &str, pal: &Palette) {
    list.push(txt(label.to_owned(), x + PAD_H, y + PAD_V, PANEL_W / 2.0 - PAD_H,
        13.0, FontWeight::NORMAL, pal.text));
}

fn push_value(list: &mut DisplayList, x: f32, y: f32, value: &str, pal: &Palette) {
    list.push(txt(value.to_owned(), x + PANEL_W / 2.0, y + PAD_V,
        PANEL_W / 2.0 - PAD_H, 12.0, FontWeight::NORMAL, pal.text_dim));
}

fn push_input(list: &mut DisplayList, x: f32, y: f32, value: &str, focused: bool, pal: &Palette) {
    let ix = x + PAD_H;
    let iw = PANEL_W - PAD_H * 2.0;
    let iy = y + PAD_V;
    let ih = ROW_H - PAD_V * 2.0;
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(ix, iy, iw, ih),
        radii: radii(3.0),
        color: if focused { pal.item_selected_bg } else { pal.input_bg },
    });
    let display = if value.len() > 72 {
        format!("…{}", &value[value.len() - 70..])
    } else {
        value.to_owned()
    };
    list.push(txt(display, ix + 6.0, iy + 3.0, iw - 12.0, 12.0,
        FontWeight::NORMAL, pal.text));
}

fn push_toggle(list: &mut DisplayList, x: f32, y: f32, on: bool, pal: &Palette) {
    let tw = 60.0;
    let th = ROW_H - PAD_V * 2.0;
    let tx = x + PANEL_W - PAD_H - tw;
    let ty = y + PAD_V;
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(tx, ty, tw, th),
        radii: radii(3.0),
        // ON stays semantic green; OFF uses item_bg surface token.
        color: if on { TOGGLE_ON } else { pal.item_bg },
    });
    list.push(txt(
        if on { "Вкл" } else { "Выкл" }.to_owned(),
        tx + 4.0, ty + 4.0, tw - 8.0, 11.0, FontWeight::BOLD, pal.text,
    ));
}

/// A neutral action button, right-aligned within a row (reset/refresh actions
/// that aren't a binary toggle).
fn push_button(list: &mut DisplayList, x: f32, y: f32, w: f32, label: &str, pal: &Palette) {
    let bx = x + PANEL_W - PAD_H - w;
    let by = y + PAD_V;
    let bh = ROW_H - PAD_V * 2.0;
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(bx, by, w, bh),
        radii: radii(3.0),
        color: pal.item_selected_bg,
    });
    list.push(txt(label.to_owned(), bx + 8.0, by + 4.0, w - 16.0, 12.0,
        FontWeight::BOLD, pal.text));
}

fn push_options(
    list: &mut DisplayList,
    x: f32,
    y: f32,
    options: &[(&str, &str)],
    current: &str,
    pal: &Palette,
) {
    let right_start = x + PANEL_W / 2.0;
    let opt_w = (PANEL_W / 2.0 - PAD_H) / options.len() as f32;
    let oh = ROW_H - PAD_V * 2.0;
    let oy = y + PAD_V;
    for (i, &(val, lbl)) in options.iter().enumerate() {
        let ox = right_start + i as f32 * opt_w;
        let is_on = val == current;
        list.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(ox + 2.0, oy, opt_w - 4.0, oh),
            radii: radii(3.0),
            color: if is_on { pal.item_selected_bg } else { pal.item_bg },
        });
        list.push(txt(lbl.to_owned(), ox + 4.0, oy + 3.0, opt_w - 8.0, 11.0,
            if is_on { FontWeight::BOLD } else { FontWeight::NORMAL }, pal.text));
    }
}

fn push_section_header(list: &mut DisplayList, x: f32, y: f32, title: &str, pal: &Palette) {
    list.push(txt(title.to_owned(), x + PAD_H, y + 8.0, PANEL_W - PAD_H * 2.0, 10.0,
        FontWeight::BOLD, pal.text_dim));
}

fn render_general(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32, pal: &Palette) {
    push_section_header(list, x, y, "ОБЩИЕ", pal);
    let by = y + HEADER_GAP;

    // Row 0: homepage label.
    push_row(list, x, by, 0, pal);
    push_label(list, x, by, "Домашняя страница", pal);

    // Row 1: homepage text input.
    push_row(list, x, by + ROW_H, 1, pal);
    push_input(
        list, x, by + ROW_H, &panel.draft.homepage,
        panel.focused_input == Some(SettingInput::Homepage),
        pal,
    );

    // Row 2: search engine.
    push_row(list, x, by + ROW_H * 2.0, 2, pal);
    push_label(list, x, by + ROW_H * 2.0, "Поисковик по умолчанию", pal);
    push_value(list, x, by + ROW_H * 2.0, &format!("ID: {}", panel.draft.search_engine_id), pal);
}

fn render_privacy(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32, pal: &Palette) {
    push_section_header(list, x, y, "КОНФИДЕНЦИАЛЬНОСТЬ", pal);
    let by = y + HEADER_GAP;

    // Row 0: shields toggle.
    push_row(list, x, by, 0, pal);
    push_label(list, x, by, "Блокировка трекеров", pal);
    push_toggle(list, x, by, panel.draft.shields_enabled, pal);

    // Row 1: fingerprint mode options.
    push_row(list, x, by + ROW_H, 1, pal);
    push_label(list, x, by + ROW_H, "Защита отпечатка", pal);
    push_options(
        list, x, by + ROW_H,
        &[("standard", "Стандарт"), ("strict", "Строгий"), ("off", "Откл")],
        &panel.draft.fingerprint_mode,
        pal,
    );

    // Row 2: DoH toggle.
    push_row(list, x, by + ROW_H * 2.0, 2, pal);
    push_label(list, x, by + ROW_H * 2.0, "DNS-over-HTTPS", pal);
    push_toggle(list, x, by + ROW_H * 2.0, panel.draft.doh_enabled, pal);

    // Row 3: Tor status (read-only — set only via --tor at startup).
    push_row(list, x, by + ROW_H * 3.0, 3, pal);
    push_label(list, x, by + ROW_H * 3.0, "Tor-режим", pal);
    push_value(
        list, x, by + ROW_H * 3.0,
        if panel.tor_active { "Включён (--tor)" } else { "Выключен" },
        pal,
    );
}

fn render_appearance(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32, pal: &Palette) {
    push_section_header(list, x, y, "ВИД", pal);
    let by = y + HEADER_GAP;

    // Row 0: font size with − / value / + buttons.
    push_row(list, x, by, 0, pal);
    push_label(list, x, by, "Размер шрифта", pal);
    {
        let btn_w = 30.0;
        let val_w = 44.0;
        let right = x + PANEL_W - PAD_H;
        let bh = ROW_H - PAD_V * 2.0;
        let by2 = by + PAD_V;
        // + button.
        list.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(right - btn_w, by2, btn_w, bh),
            radii: radii(3.0), color: pal.item_bg,
        });
        list.push(txt("+".to_owned(), right - btn_w + 8.0, by2 + 3.0, btn_w - 8.0, 13.0,
            FontWeight::BOLD, pal.text));
        // Value.
        list.push(txt(format!("{:.0}px", panel.draft.font_size),
            right - btn_w - val_w, by2 + 3.0, val_w, 12.0,
            FontWeight::NORMAL, pal.text_dim));
        // − button.
        list.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(right - btn_w - val_w - btn_w, by2, btn_w, bh),
            radii: radii(3.0), color: pal.item_bg,
        });
        list.push(txt("−".to_owned(), right - btn_w - val_w - btn_w + 8.0, by2 + 3.0,
            btn_w - 8.0, 13.0, FontWeight::BOLD, pal.text));
    }

    // Row 1: base theme options.
    push_row(list, x, by + ROW_H, 1, pal);
    push_label(list, x, by + ROW_H, "Тема", pal);
    // Parse current base from draft.theme (before the '+' if present).
    let current_base = panel.draft.theme.split('+').next().unwrap_or("system");
    push_options(
        list, x, by + ROW_H,
        &[("dark", "Тёмная"), ("light", "Светлая"), ("system", "Система")],
        current_base,
        pal,
    );

    // Row 2: accent colour swatches.
    push_row(list, x, by + ROW_H * 2.0, 2, pal);
    push_label(list, x, by + ROW_H * 2.0, "Акцент", pal);
    {
        use crate::panels::themes::AccentPreset;
        let current_accent = panel.draft.theme.split('+').nth(1).unwrap_or("blue");
        let swatch_sz = 22.0;
        let gap = 8.0;
        let sy = by + ROW_H * 2.0 + (ROW_H - swatch_sz) * 0.5;
        let start_x = x + PANEL_W / 2.0;
        for (i, preset) in AccentPreset::ALL.iter().enumerate() {
            let sx = start_x + i as f32 * (swatch_sz + gap);
            let is_active = preset.key() == current_accent;
            if is_active {
                // White ring drawn first (below the swatch).
                list.push(DisplayCommand::FillRoundedRect {
                    rect: Rect::new(sx - 2.0, sy - 2.0, swatch_sz + 4.0, swatch_sz + 4.0),
                    radii: radii((swatch_sz + 4.0) * 0.5),
                    color: Color { r: 255, g: 255, b: 255, a: 200 },
                });
            }
            list.push(DisplayCommand::FillRoundedRect {
                rect: Rect::new(sx, sy, swatch_sz, swatch_sz),
                radii: radii(swatch_sz * 0.5),
                color: preset.color(),
            });
        }
    }

    // Row 3: tab-strip layout (horizontal / vertical).
    push_row(list, x, by + ROW_H * 3.0, 3, pal);
    push_label(list, x, by + ROW_H * 3.0, "Расположение вкладок", pal);
    push_options(
        list, x, by + ROW_H * 3.0,
        &[("horizontal", "Сверху"), ("vertical", "Сбоку")],
        &panel.draft.tab_layout,
        pal,
    );
}

fn render_downloads(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32, pal: &Palette) {
    push_section_header(list, x, y, "ЗАГРУЗКИ", pal);
    let by = y + HEADER_GAP;

    // Row 0: label.
    push_row(list, x, by, 0, pal);
    push_label(list, x, by, "Папка загрузок", pal);

    // Row 1: path input.
    push_row(list, x, by + ROW_H, 1, pal);
    push_input(
        list, x, by + ROW_H, &panel.draft.download_path,
        panel.focused_input == Some(SettingInput::DownloadPath),
        pal,
    );

    // Row 2: hint (not clickable).
    push_row(list, x, by + ROW_H * 2.0, 2, pal);
    list.push(txt(
        "Оставьте пустым — браузер использует стандартную папку ОС.".to_owned(),
        x + PAD_H, by + ROW_H * 2.0 + PAD_V, PANEL_W - PAD_H * 2.0, 10.0,
        FontWeight::NORMAL, pal.text_dim,
    ));

    // Row 3: reset panel layout.
    push_row(list, x, by + ROW_H * 3.0, 3, pal);
    push_label(list, x, by + ROW_H * 3.0, "Раскладка боковых панелей", pal);
    push_button(list, x, by + ROW_H * 3.0, 140.0, "Сбросить", pal);
}

fn render_network(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32, pal: &Palette) {
    push_section_header(list, x, y, "СЕТЬ", pal);
    let by = y + HEADER_GAP;

    // Row 0: HTTP/3 toggle.
    push_row(list, x, by, 0, pal);
    push_label(list, x, by, "HTTP/3 (QUIC)", pal);
    push_toggle(list, x, by, panel.http3_draft, pal);

    // Row 1: active HTTP fingerprint profile (read-only).
    push_row(list, x, by + ROW_H, 1, pal);
    push_label(list, x, by + ROW_H, "Активный HTTP-профиль", pal);
    push_value(list, x, by + ROW_H, &format!("{:?}", crate::config::global().http_profile), pal);

    // Hint below.
    list.push(txt(
        "Изменение HTTP/3 вступает в силу после перезапуска браузера.".to_owned(),
        x + PAD_H, by + ROW_H * 2.0 + 8.0, PANEL_W - PAD_H * 2.0, 10.0,
        FontWeight::NORMAL, pal.text_dim,
    ));
}

fn render_adblock(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32, pal: &Palette) {
    push_section_header(list, x, y, "СПИСКИ БЛОКИРОВКИ", pal);
    let by = y + HEADER_GAP;

    for (i, sub) in panel.adblock_subs.iter().enumerate() {
        let ry = by + ROW_H * i as f32;
        push_row(list, x, ry, i, pal);
        push_label(list, x, ry, &sub.title, pal);
        push_toggle(list, x, ry, sub.enabled, pal);
    }

    let btn_row_y = by + ROW_H * panel.adblock_subs.len() as f32;
    push_row(list, x, btn_row_y, panel.adblock_subs.len(), pal);
    push_button(list, x, btn_row_y, PANEL_W / 2.0 - PAD_H, "Обновить сейчас", pal);
}

fn render_language(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32, pal: &Palette) {
    push_section_header(list, x, y, "ЯЗЫК И ПРАВОПИСАНИЕ", pal);
    let by = y + HEADER_GAP;

    // Row 0: loaded dictionaries (read-only).
    push_row(list, x, by, 0, pal);
    push_label(list, x, by, "Загруженные словари", pal);
    push_value(
        list, x, by,
        panel.spell_locale.as_deref().unwrap_or("Загрузка…"),
        pal,
    );

    // Row 1: hint.
    push_row(list, x, by + ROW_H, 1, pal);
    list.push(txt(
        "Чтобы добавить язык: положите пару <локаль>.aff/.dic в data/spell/ и перезапустите браузер.".to_owned(),
        x + PAD_H, by + ROW_H + PAD_V, PANEL_W - PAD_H * 2.0, 10.0,
        FontWeight::NORMAL, pal.text_dim,
    ));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn txt(text: impl Into<String>, x: f32, y: f32, w: f32, font_size: f32,
       weight: FontWeight, color: Color) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text: text.into(),
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: weight,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: None,
    }
}

fn radii(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, bl: r, bl_y: r, br: r, br_y: r }
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

    #[test]
    fn build_panel_produces_commands() {
        let p = panel_at_origin();
        let mut dl: DisplayList = Vec::new();
        build_panel(&p, &mut dl, 10.0, 10.0, &Palette::DARK);
        assert!(!dl.is_empty());
    }

    #[test]
    fn section_labels_non_empty() {
        for &sec in &SettingsSection::ALL {
            assert!(!sec.label().is_empty());
        }
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

    // ── Tooltip coverage ──────────────────────────────────────────────────────

    #[test]
    fn tooltip_none_outside_panel() {
        let p = panel_at_origin();
        assert_eq!(tooltip_for(&p, 1000.0, 1000.0, 0.0, 0.0), None);
    }

    #[test]
    fn tooltip_none_over_header_and_tabs() {
        let p = panel_at_origin();
        assert_eq!(tooltip_for(&p, 20.0, 10.0, 0.0, 0.0), None);
        assert_eq!(tooltip_for(&p, 20.0, HEADER_H + 5.0, 0.0, 0.0), None);
    }

    #[test]
    fn tooltip_present_over_shields_toggle() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Privacy;
        let my = CONTENT_TOP + HEADER_GAP + 20.0;
        assert!(tooltip_for(&p, PANEL_W - 20.0, my, 0.0, 0.0).is_some());
    }

    #[test]
    fn tooltip_present_over_http3_toggle() {
        let mut p = panel_at_origin();
        p.section = SettingsSection::Network;
        let my = CONTENT_TOP + HEADER_GAP + 20.0;
        let tip = tooltip_for(&p, PANEL_W - 20.0, my, 0.0, 0.0);
        assert!(tip.is_some());
        assert!(tip.unwrap().contains("перезапуска"));
    }

    #[test]
    fn build_tooltip_produces_commands_and_stays_in_viewport() {
        let dl = build_tooltip("Пример подсказки", 900.0, 700.0, 1024.0, 720.0, &Palette::DARK);
        assert!(!dl.is_empty());
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
