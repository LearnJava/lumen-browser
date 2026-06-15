//! Browser settings panel (D-7).
//!
//! A full-page centred overlay opened by `Ctrl+,` or by navigating to
//! `about:settings`. Four tabbed sections:
//!
//! - **General** — homepage URL, default search engine ID.
//! - **Privacy** — shields on/off, fingerprint mode, DoH on/off.
//! - **Appearance** — base font size (px), UI theme.
//! - **Downloads** — default download directory path.
//!
//! State is split between [`SettingsPanel`] (UI/UX) and
//! `lumen_storage::BrowserSettings` (persistence). The panel holds a
//! [`lumen_storage::BrowserSettingsSnapshot`] as a working draft. On close the
//! caller persists it via `BrowserSettings::apply_snapshot`.
//!
//! Hit testing: [`hit_test`]. Rendering: [`build_panel`].

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};
use lumen_storage::BrowserSettingsSnapshot;

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_W: f32 = 640.0;
/// Panel height in CSS px (exported for anchor calculation in main.rs).
pub const PANEL_H: f32 = 480.0;
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
/// Each of 4 equal-width tabs.
const TAB_W: f32 = PANEL_W / 4.0;

// ── Colours ──────────────────────────────────────────────────────────────────

const PANEL_BORDER_COL: Color = Color { r: 52, g: 52, b: 66, a: 255 };
const PANEL_BG: Color = Color { r: 18, g: 18, b: 26, a: 254 };
const HEADER_BG: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const HEADER_TEXT: Color = Color { r: 210, g: 210, b: 225, a: 255 };
const CLOSE_COL: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const TAB_BG: Color = Color { r: 22, g: 22, b: 32, a: 255 };
const TAB_ACTIVE_BG: Color = Color { r: 32, g: 40, b: 58, a: 255 };
const TAB_TEXT: Color = Color { r: 140, g: 140, b: 160, a: 255 };
const TAB_ACTIVE_TEXT: Color = Color { r: 220, g: 220, b: 235, a: 255 };
const TAB_ACCENT: Color = Color { r: 82, g: 128, b: 220, a: 255 };
const ROW_EVEN: Color = Color { r: 22, g: 22, b: 32, a: 255 };
const ROW_ODD: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const LABEL_COL: Color = Color { r: 200, g: 200, b: 218, a: 255 };
const VALUE_COL: Color = Color { r: 150, g: 150, b: 170, a: 255 };
const INPUT_BG: Color = Color { r: 14, g: 14, b: 22, a: 255 };
const INPUT_ACTIVE_BG: Color = Color { r: 18, g: 24, b: 38, a: 255 };
const INPUT_TEXT: Color = Color { r: 190, g: 190, b: 208, a: 255 };
const TOGGLE_ON: Color = Color { r: 60, g: 140, b: 80, a: 230 };
const TOGGLE_OFF: Color = Color { r: 70, g: 70, b: 90, a: 230 };
const TOGGLE_TEXT: Color = Color { r: 240, g: 240, b: 248, a: 255 };
const OPTION_BG: Color = Color { r: 36, g: 36, b: 52, a: 230 };
const OPTION_ACTIVE: Color = Color { r: 58, g: 90, b: 160, a: 230 };
const OPTION_TEXT: Color = Color { r: 190, g: 190, b: 210, a: 255 };
const SEPARATOR: Color = Color { r: 36, g: 36, b: 50, a: 255 };
const SECTION_HDR: Color = Color { r: 100, g: 120, b: 160, a: 255 };

// ── Section ───────────────────────────────────────────────────────────────────

/// The four top-level settings sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    /// Homepage URL + default search engine.
    General,
    /// Shields, fingerprint mode, DoH.
    Privacy,
    /// Font size + UI theme.
    Appearance,
    /// Default download directory.
    Downloads,
}

impl SettingsSection {
    /// All four sections in tab order.
    pub const ALL: [Self; 4] = [
        Self::General,
        Self::Privacy,
        Self::Appearance,
        Self::Downloads,
    ];

    /// Display label for the tab.
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "Общие",
            Self::Privacy => "Конфиденц.",
            Self::Appearance => "Вид",
            Self::Downloads => "Загрузки",
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
        }
    }

    /// Open the panel, loading a fresh snapshot as the working draft.
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
    #[allow(dead_code)]
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
    /// Click on the homepage text field (focus it).
    FocusHomepage,
    /// Click on the download path text field (focus it).
    FocusDownloadPath,
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
        SettingsSection::Privacy => ht_privacy(panel, lx, ly),
        SettingsSection::Appearance => ht_appearance(lx, ly),
        SettingsSection::Downloads => ht_downloads(ly),
    }
}

fn ht_general(lx: f32, ly: f32) -> SettingsHit {
    let _ = lx;
    // Row 0 (y 0..ROW_H): label row; row 1 (ROW_H..2*ROW_H): homepage input.
    if (ROW_H..ROW_H * 2.0).contains(&ly) {
        return SettingsHit::FocusHomepage;
    }
    SettingsHit::Inside
}

fn ht_privacy(panel: &SettingsPanel, lx: f32, ly: f32) -> SettingsHit {
    let _ = panel;
    // Row 0: shields toggle
    if ly < ROW_H {
        let toggle_x = PANEL_W - PAD_H - 60.0;
        if lx >= toggle_x { return SettingsHit::ToggleShields; }
        return SettingsHit::Inside;
    }
    // Row 1: fingerprint mode options
    if ly < ROW_H * 2.0 {
        let options = ["standard", "strict", "off"];
        let right_start = PANEL_W / 2.0;
        let option_w = (PANEL_W / 2.0 - PAD_H) / options.len() as f32;
        for (i, &opt) in options.iter().enumerate() {
            let ox = right_start + i as f32 * option_w;
            if lx >= ox && lx < ox + option_w {
                return SettingsHit::SetFingerprintMode(opt.to_owned());
            }
        }
        return SettingsHit::Inside;
    }
    // Row 2: DoH toggle
    if ly < ROW_H * 3.0 {
        let toggle_x = PANEL_W - PAD_H - 60.0;
        if lx >= toggle_x { return SettingsHit::ToggleDoh; }
        return SettingsHit::Inside;
    }
    SettingsHit::Inside
}

fn ht_appearance(lx: f32, ly: f32) -> SettingsHit {
    // Row 0: font size: − / value / +
    if ly < ROW_H {
        let btn_w = 30.0;
        let val_w = 44.0;
        let right_end = PANEL_W - PAD_H;
        // + button: rightmost btn_w
        if lx >= right_end - btn_w { return SettingsHit::FontSizeIncrease; }
        // − button: after value display
        if lx >= right_end - btn_w - val_w - btn_w && lx < right_end - btn_w - val_w {
            return SettingsHit::FontSizeDecrease;
        }
        return SettingsHit::Inside;
    }
    // Row 1: base theme options (dark / light / system)
    if ly < ROW_H * 2.0 {
        let options = ["dark", "light", "system"];
        let right_start = PANEL_W / 2.0;
        let option_w = (PANEL_W / 2.0 - PAD_H) / options.len() as f32;
        for (i, &opt) in options.iter().enumerate() {
            let ox = right_start + i as f32 * option_w;
            if lx >= ox && lx < ox + option_w {
                return SettingsHit::SetTheme(opt.to_owned());
            }
        }
        return SettingsHit::Inside;
    }
    // Row 2: accent colour swatches (6 circles)
    if ly < ROW_H * 3.0 {
        use crate::panels::themes::AccentPreset;
        let swatch_sz = 22.0;
        let gap = 8.0;
        let total_w = AccentPreset::ALL.len() as f32 * (swatch_sz + gap) - gap;
        let start_x = PANEL_W / 2.0;
        let _ = total_w; // positioned from right-half start, left-to-right
        for (i, preset) in AccentPreset::ALL.iter().enumerate() {
            let sx = start_x + i as f32 * (swatch_sz + gap);
            if lx >= sx && lx < sx + swatch_sz {
                return SettingsHit::SetAccent(preset.key().to_owned());
            }
        }
    }
    SettingsHit::Inside
}

fn ht_downloads(ly: f32) -> SettingsHit {
    // Row 0 (y 0..ROW_H): label; row 1 (ROW_H..2*ROW_H): path input.
    if (ROW_H..ROW_H * 2.0).contains(&ly) {
        return SettingsHit::FocusDownloadPath;
    }
    SettingsHit::Inside
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// Append display commands for the settings panel to `list`.
///
/// `(px, py)` is the panel's top-left corner in window CSS px.
pub fn build_panel(panel: &SettingsPanel, list: &mut DisplayList, px: f32, py: f32) {
    // Outer border ring.
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        radii: radii(7.0),
        color: PANEL_BORDER_COL,
    });
    // Inner background.
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + 1.0, PANEL_W - 2.0, PANEL_H - 2.0),
        radii: radii(6.0),
        color: PANEL_BG,
    });

    // ── Header ───────────────────────────────────────────────────────────────
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, HEADER_H),
        radii: CornerRadii { tl: 6.0, tl_y: 6.0, tr: 6.0, tr_y: 6.0,
                             bl: 0.0, bl_y: 0.0, br: 0.0, br_y: 0.0 },
        color: HEADER_BG,
    });
    list.push(txt("Настройки", px + PAD_H, py + 12.0, PANEL_W - PAD_H * 2.0 - CLOSE_W,
        13.0, FontWeight::BOLD, HEADER_TEXT));
    list.push(txt("×", px + PANEL_W - CLOSE_W + 6.0, py + 10.0, 20.0,
        15.0, FontWeight::BOLD, CLOSE_COL));
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(px, py + HEADER_H - 1.0, PANEL_W, 1.0),
        color: SEPARATOR,
    });

    // ── Tab bar ──────────────────────────────────────────────────────────────
    let tab_y = py + HEADER_H;
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_y, PANEL_W, TAB_BAR_H),
        color: TAB_BG,
    });
    for (i, &sec) in SettingsSection::ALL.iter().enumerate() {
        let tx = px + i as f32 * TAB_W;
        let is_active = sec == panel.section;
        if is_active {
            list.push(DisplayCommand::FillRect {
                rect: Rect::new(tx, tab_y, TAB_W, TAB_BAR_H),
                color: TAB_ACTIVE_BG,
            });
            // Active underline accent.
            list.push(DisplayCommand::FillRect {
                rect: Rect::new(tx + 4.0, tab_y + TAB_BAR_H - 2.0, TAB_W - 8.0, 2.0),
                color: TAB_ACCENT,
            });
        }
        list.push(txt(
            sec.label().to_owned(),
            tx + 6.0,
            tab_y + 10.0,
            TAB_W - 12.0,
            12.0,
            if is_active { FontWeight::BOLD } else { FontWeight::NORMAL },
            if is_active { TAB_ACTIVE_TEXT } else { TAB_TEXT },
        ));
    }
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_y + TAB_BAR_H - 1.0, PANEL_W, 1.0),
        color: SEPARATOR,
    });

    // ── Content area ─────────────────────────────────────────────────────────
    let cy = py + CONTENT_TOP;
    list.push(DisplayCommand::PushClipRect {
        rect: Rect::new(px, cy, PANEL_W, CONTENT_H),
    });
    let off = panel.scroll_y;
    match panel.section {
        SettingsSection::General => render_general(panel, list, px, cy - off),
        SettingsSection::Privacy => render_privacy(panel, list, px, cy - off),
        SettingsSection::Appearance => render_appearance(panel, list, px, cy - off),
        SettingsSection::Downloads => render_downloads(panel, list, px, cy - off),
    }
    list.push(DisplayCommand::PopClip);
}

// ── Per-section renderers ────────────────────────────────────────────────────

fn row_bg(i: usize) -> Color { if i.is_multiple_of(2) { ROW_EVEN } else { ROW_ODD } }

fn push_row(list: &mut DisplayList, x: f32, y: f32, i: usize) {
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, PANEL_W, ROW_H),
        color: row_bg(i),
    });
    list.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y + ROW_H - 1.0, PANEL_W, 1.0),
        color: SEPARATOR,
    });
}

fn push_label(list: &mut DisplayList, x: f32, y: f32, label: &str) {
    list.push(txt(label.to_owned(), x + PAD_H, y + PAD_V, PANEL_W / 2.0 - PAD_H,
        13.0, FontWeight::NORMAL, LABEL_COL));
}

fn push_input(list: &mut DisplayList, x: f32, y: f32, value: &str, focused: bool) {
    let ix = x + PAD_H;
    let iw = PANEL_W - PAD_H * 2.0;
    let iy = y + PAD_V;
    let ih = ROW_H - PAD_V * 2.0;
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(ix, iy, iw, ih),
        radii: radii(3.0),
        color: if focused { INPUT_ACTIVE_BG } else { INPUT_BG },
    });
    let display = if value.len() > 72 {
        format!("…{}", &value[value.len() - 70..])
    } else {
        value.to_owned()
    };
    list.push(txt(display, ix + 6.0, iy + 3.0, iw - 12.0, 12.0,
        FontWeight::NORMAL, INPUT_TEXT));
}

fn push_toggle(list: &mut DisplayList, x: f32, y: f32, on: bool) {
    let tw = 60.0;
    let th = ROW_H - PAD_V * 2.0;
    let tx = x + PANEL_W - PAD_H - tw;
    let ty = y + PAD_V;
    list.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(tx, ty, tw, th),
        radii: radii(3.0),
        color: if on { TOGGLE_ON } else { TOGGLE_OFF },
    });
    list.push(txt(
        if on { "Вкл" } else { "Выкл" }.to_owned(),
        tx + 4.0, ty + 4.0, tw - 8.0, 11.0, FontWeight::BOLD, TOGGLE_TEXT,
    ));
}

fn push_options(
    list: &mut DisplayList,
    x: f32,
    y: f32,
    options: &[(&str, &str)],
    current: &str,
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
            color: if is_on { OPTION_ACTIVE } else { OPTION_BG },
        });
        list.push(txt(lbl.to_owned(), ox + 4.0, oy + 3.0, opt_w - 8.0, 11.0,
            if is_on { FontWeight::BOLD } else { FontWeight::NORMAL }, OPTION_TEXT));
    }
}

fn push_section_header(list: &mut DisplayList, x: f32, y: f32, title: &str) {
    list.push(txt(title.to_owned(), x + PAD_H, y + 8.0, PANEL_W - PAD_H * 2.0, 10.0,
        FontWeight::BOLD, SECTION_HDR));
}

fn render_general(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32) {
    push_section_header(list, x, y, "ОБЩИЕ");
    let by = y + 26.0;

    // Row 0: homepage label.
    push_row(list, x, by, 0);
    push_label(list, x, by, "Домашняя страница");

    // Row 1: homepage text input.
    push_row(list, x, by + ROW_H, 1);
    push_input(
        list, x, by + ROW_H, &panel.draft.homepage,
        panel.focused_input == Some(SettingInput::Homepage),
    );

    // Row 2: search engine.
    push_row(list, x, by + ROW_H * 2.0, 2);
    push_label(list, x, by + ROW_H * 2.0, "Поисковик по умолчанию");
    list.push(txt(
        format!("ID: {}", panel.draft.search_engine_id),
        x + PANEL_W / 2.0, by + ROW_H * 2.0 + PAD_V,
        PANEL_W / 2.0 - PAD_H, 12.0, FontWeight::NORMAL, VALUE_COL,
    ));
}

fn render_privacy(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32) {
    push_section_header(list, x, y, "КОНФИДЕНЦИАЛЬНОСТЬ");
    let by = y + 26.0;

    // Row 0: shields toggle.
    push_row(list, x, by, 0);
    push_label(list, x, by, "Блокировка трекеров");
    push_toggle(list, x, by, panel.draft.shields_enabled);

    // Row 1: fingerprint mode options.
    push_row(list, x, by + ROW_H, 1);
    push_label(list, x, by + ROW_H, "Защита отпечатка");
    push_options(
        list, x, by + ROW_H,
        &[("standard", "Стандарт"), ("strict", "Строгий"), ("off", "Откл")],
        &panel.draft.fingerprint_mode,
    );

    // Row 2: DoH toggle.
    push_row(list, x, by + ROW_H * 2.0, 2);
    push_label(list, x, by + ROW_H * 2.0, "DNS-over-HTTPS");
    push_toggle(list, x, by + ROW_H * 2.0, panel.draft.doh_enabled);
}

fn render_appearance(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32) {
    push_section_header(list, x, y, "ВИД");
    let by = y + 26.0;

    // Row 0: font size with − / value / + buttons.
    push_row(list, x, by, 0);
    push_label(list, x, by, "Размер шрифта");
    {
        let btn_w = 30.0;
        let val_w = 44.0;
        let right = x + PANEL_W - PAD_H;
        let bh = ROW_H - PAD_V * 2.0;
        let by2 = by + PAD_V;
        // + button.
        list.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(right - btn_w, by2, btn_w, bh),
            radii: radii(3.0), color: OPTION_BG,
        });
        list.push(txt("+".to_owned(), right - btn_w + 8.0, by2 + 3.0, btn_w - 8.0, 13.0,
            FontWeight::BOLD, TOGGLE_TEXT));
        // Value.
        list.push(txt(format!("{:.0}px", panel.draft.font_size),
            right - btn_w - val_w, by2 + 3.0, val_w, 12.0,
            FontWeight::NORMAL, VALUE_COL));
        // − button.
        list.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(right - btn_w - val_w - btn_w, by2, btn_w, bh),
            radii: radii(3.0), color: OPTION_BG,
        });
        list.push(txt("−".to_owned(), right - btn_w - val_w - btn_w + 8.0, by2 + 3.0,
            btn_w - 8.0, 13.0, FontWeight::BOLD, TOGGLE_TEXT));
    }

    // Row 1: base theme options.
    push_row(list, x, by + ROW_H, 1);
    push_label(list, x, by + ROW_H, "Тема");
    // Parse current base from draft.theme (before the '+' if present).
    let current_base = panel.draft.theme.split('+').next().unwrap_or("system");
    push_options(
        list, x, by + ROW_H,
        &[("dark", "Тёмная"), ("light", "Светлая"), ("system", "Система")],
        current_base,
    );

    // Row 2: accent colour swatches.
    push_row(list, x, by + ROW_H * 2.0, 2);
    push_label(list, x, by + ROW_H * 2.0, "Акцент");
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
}

fn render_downloads(panel: &SettingsPanel, list: &mut DisplayList, x: f32, y: f32) {
    push_section_header(list, x, y, "ЗАГРУЗКИ");
    let by = y + 26.0;

    // Row 0: label.
    push_row(list, x, by, 0);
    push_label(list, x, by, "Папка загрузок");

    // Row 1: path input.
    push_row(list, x, by + ROW_H, 1);
    push_input(
        list, x, by + ROW_H, &panel.draft.download_path,
        panel.focused_input == Some(SettingInput::DownloadPath),
    );

    // Hint below the input.
    list.push(txt(
        "Оставьте пустым — браузер использует стандартную папку ОС.".to_owned(),
        x + PAD_H, by + ROW_H * 2.0 + 8.0, PANEL_W - PAD_H * 2.0, 10.0,
        FontWeight::NORMAL, SECTION_HDR,
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
        tab_size: 0.0,
        highlight_name: None,
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
        build_panel(&p, &mut dl, 10.0, 10.0);
        assert!(!dl.is_empty());
    }

    #[test]
    fn section_labels_non_empty() {
        for &sec in &SettingsSection::ALL {
            assert!(!sec.label().is_empty());
        }
    }
}
