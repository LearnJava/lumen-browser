//! Accessibility settings panel (E-2).
//!
//! A centred 300×260 px overlay opened by `Ctrl+Shift+Q`. It exposes four
//! preferences from [`lumen_storage::A11yPrefs`]:
//!
//! - **Font size multiplier** — five pill-buttons: 0.8 / 1.0 / 1.25 / 1.5 / 2.0.
//! - **Prefers-reduced-motion** — toggle; wired to `_lumen_deliver_media_changes`.
//! - **Forced colors** — toggle; stored in [`lumen_storage::A11yPrefs`].
//! - **Cursor size** — three pill-buttons: Normal / Large / Extra.
//!
//! The panel holds a [`lumen_storage::A11yPrefsSnapshot`] as a working draft.
//! On close the caller persists it via [`lumen_storage::A11yPrefs::apply_snapshot`]
//! and re-delivers media changes to the JS engine.
//!
//! Hit testing: [`hit_test`]. Rendering: [`build_a11y_panel`].

use lumen_core::geom::Rect;
use lumen_layout::{BorderStyle, Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};
use lumen_storage::{A11yPrefsSnapshot, CursorSize};

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px (exported for hit-test callers in main.rs).
pub const PANEL_W: f32 = 300.0;
/// Panel height in CSS px (exported for hit-test callers in main.rs).
pub const PANEL_H: f32 = 260.0;
/// Header bar height.
const HEADER_H: f32 = 36.0;
/// Height of a content row (label + control).
const ROW_H: f32 = 40.0;
/// Pill button height.
const PILL_H: f32 = 26.0;
/// Horizontal padding inside the panel.
const PAD_H: f32 = 14.0;
/// Vertical padding between rows.
const PAD_V: f32 = 6.0;
/// Close `×` hit zone width.
const CLOSE_W: f32 = 28.0;
/// Font size for labels.
const FONT_SIZE: f32 = 12.0;
/// Font size for the header title.
const HEADER_FONT: f32 = 13.0;

// ── Colours ───────────────────────────────────────────────────────────────��──

const PANEL_BG: Color = Color { r: 18, g: 18, b: 26, a: 254 };
const PANEL_BORDER: Color = Color { r: 52, g: 52, b: 66, a: 255 };
const HEADER_BG: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const HEADER_TEXT: Color = Color { r: 210, g: 210, b: 225, a: 255 };
const CLOSE_COL: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const LABEL_COL: Color = Color { r: 190, g: 190, b: 210, a: 255 };
const PILL_BG: Color = Color { r: 36, g: 36, b: 52, a: 230 };
const PILL_ACTIVE: Color = Color { r: 58, g: 90, b: 160, a: 230 };
const PILL_TEXT: Color = Color { r: 190, g: 190, b: 210, a: 255 };
const TOGGLE_ON: Color = Color { r: 60, g: 140, b: 80, a: 230 };
const TOGGLE_OFF: Color = Color { r: 70, g: 70, b: 90, a: 230 };
const TOGGLE_TEXT: Color = Color { r: 240, g: 240, b: 248, a: 255 };
const ROW_EVEN: Color = Color { r: 22, g: 22, b: 32, a: 255 };
const ROW_ODD: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const SEPARATOR: Color = Color { r: 36, g: 36, b: 50, a: 255 };

// ── Panel state ───────────────────────────────────────────────────────────────

/// Accessibility settings panel state.
///
/// `visible` gates rendering and hit-testing. `draft` holds in-progress edits
/// that are persisted only when the user closes the panel via the `×` button
/// or the `Ctrl+Shift+Q` toggle.
pub struct A11yPanel {
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// Working copy of the accessibility preferences; persisted on close.
    pub draft: A11yPrefsSnapshot,
}

impl A11yPanel {
    /// Create a new hidden panel with default preferences.
    pub fn new() -> Self {
        Self {
            visible: false,
            draft: A11yPrefsSnapshot::default(),
        }
    }

    /// Toggle panel visibility.
    ///
    /// Note: prefer using the main.rs key handler directly since opening the
    /// panel should also call `load_draft` first.
    #[allow(dead_code)]
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Load current preferences into the draft so edits start from persisted values.
    pub fn load_draft(&mut self, snap: A11yPrefsSnapshot) {
        self.draft = snap;
    }
}

impl Default for A11yPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit testing ───────────────────────────────────────────────────────────────

/// Result of a click on (or near) the accessibility panel.
#[derive(Debug, Clone, PartialEq)]
pub enum A11yHit {
    /// The close `×` button — persist draft and hide.
    Close,
    /// A font-size multiplier pill was clicked; contains the chosen value.
    FontMultiplier(f32),
    /// The prefers-reduced-motion toggle was clicked.
    ReducedMotion,
    /// The forced-colors toggle was clicked.
    ForcedColors,
    /// A cursor-size pill was clicked; contains the chosen size.
    CursorSizeOption(CursorSize),
    /// Click inside the panel body (non-interactive area) — swallow it.
    Inside,
    /// Click outside the panel — pass it through.
    Outside,
}

/// Top-left corner of the centred panel given the window dimensions.
fn panel_origin(win_w: f32, win_h: f32) -> (f32, f32) {
    (
        ((win_w - PANEL_W) * 0.5).max(0.0),
        ((win_h - PANEL_H) * 0.5).max(0.0),
    )
}

/// Classify a click at `(x, y)` CSS px.
pub fn hit_test(panel: &A11yPanel, x: f32, y: f32, win_w: f32, win_h: f32) -> A11yHit {
    if !panel.visible {
        return A11yHit::Outside;
    }
    let (px, py) = panel_origin(win_w, win_h);
    if x < px || x > px + PANEL_W || y < py || y > py + PANEL_H {
        return A11yHit::Outside;
    }

    // Close button: top-right of header.
    if y < py + HEADER_H && x > px + PANEL_W - CLOSE_W {
        return A11yHit::Close;
    }

    // Row areas below the header.
    let content_y = py + HEADER_H;
    // Row 0: font-size multiplier pills.
    let row0_y = content_y + PAD_V;
    let row0_bottom = row0_y + ROW_H;
    if y >= row0_y && y < row0_bottom {
        return font_multiplier_hit(panel, x, y, px, row0_y);
    }
    // Row 1: reduced-motion toggle.
    let row1_y = row0_bottom;
    let row1_bottom = row1_y + ROW_H;
    if y >= row1_y && y < row1_bottom {
        let toggle_x = px + PANEL_W - PAD_H - 52.0;
        if x >= toggle_x {
            return A11yHit::ReducedMotion;
        }
    }
    // Row 2: forced-colors toggle.
    let row2_y = row1_bottom;
    let row2_bottom = row2_y + ROW_H;
    if y >= row2_y && y < row2_bottom {
        let toggle_x = px + PANEL_W - PAD_H - 52.0;
        if x >= toggle_x {
            return A11yHit::ForcedColors;
        }
    }
    // Row 3: cursor-size pills.
    let row3_y = row2_bottom;
    let row3_bottom = row3_y + ROW_H;
    if y >= row3_y && y < row3_bottom {
        return cursor_size_hit(panel, x, y, px, row3_y);
    }
    if y < row3_bottom {
        return A11yHit::Inside;
    }

    A11yHit::Inside
}

/// Determine which font-multiplier pill, if any, was hit.
fn font_multiplier_hit(_panel: &A11yPanel, x: f32, _y: f32, px: f32, _row_y: f32) -> A11yHit {
    let pills_x = px + PAD_H + 70.0;
    let pill_w = (PANEL_W - PAD_H * 2.0 - 70.0) / 5.0;
    for (i, &val) in FONT_MULTIPLIERS.iter().enumerate() {
        let bx = pills_x + i as f32 * pill_w;
        if x >= bx && x < bx + pill_w - 2.0 {
            return A11yHit::FontMultiplier(val);
        }
    }
    A11yHit::Inside
}

/// Determine which cursor-size pill, if any, was hit.
fn cursor_size_hit(_panel: &A11yPanel, x: f32, _y: f32, px: f32, _row_y: f32) -> A11yHit {
    let pills_x = px + PAD_H + 70.0;
    let pill_w = (PANEL_W - PAD_H * 2.0 - 70.0) / 3.0;
    const SIZES: [CursorSize; 3] = [CursorSize::Normal, CursorSize::Large, CursorSize::ExtraLarge];
    for (i, size) in SIZES.iter().enumerate() {
        let bx = pills_x + i as f32 * pill_w;
        if x >= bx && x < bx + pill_w - 2.0 {
            return A11yHit::CursorSizeOption(*size);
        }
    }
    A11yHit::Inside
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Font-size multiplier options shown as pill buttons.
const FONT_MULTIPLIERS: [f32; 5] = [0.8, 1.0, 1.25, 1.5, 2.0];

/// Build the centred accessibility settings panel overlay.
///
/// Returns an empty `DisplayList` when `panel.visible` is `false`.
/// `(win_w, win_h)` are the window dimensions in CSS px.
pub fn build_a11y_panel(panel: &A11yPanel, (win_w, win_h): (u32, u32)) -> DisplayList {
    if !panel.visible {
        return Vec::new();
    }
    let win_w = win_w as f32;
    let win_h = win_h as f32;
    let (px, py) = panel_origin(win_w, win_h);

    // Dim backdrop.
    let mut out: DisplayList = vec![DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, win_w, win_h),
        color: Color { r: 0, g: 0, b: 0, a: 100 },
    }];

    // Panel background + border.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        color: PANEL_BG,
        radii: uniform_radii(6.0),
    });
    out.push(DisplayCommand::DrawBorder {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        widths: [1.0; 4],
        colors: [PANEL_BORDER; 4],
        styles: [BorderStyle::Solid; 4],
        radii: uniform_radii(6.0),
    });

    // Header.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, HEADER_H),
        color: HEADER_BG,
        radii: CornerRadii { tl: 6.0, tl_y: 6.0, tr: 6.0, tr_y: 6.0, br: 0.0, br_y: 0.0, bl: 0.0, bl_y: 0.0 },
    });
    out.push(make_text(
        "Accessibility".to_string(),
        px + PAD_H,
        py + (HEADER_H - HEADER_FONT) / 2.0,
        PANEL_W - PAD_H * 2.0,
        HEADER_FONT,
        FontWeight::BOLD,
        HEADER_TEXT,
    ));
    // Close button.
    out.push(make_text(
        "×".to_string(),
        px + PANEL_W - CLOSE_W + 4.0,
        py + (HEADER_H - HEADER_FONT) / 2.0 - 1.0,
        CLOSE_W,
        HEADER_FONT + 3.0,
        FontWeight::NORMAL,
        CLOSE_COL,
    ));

    // Content rows.
    let content_y = py + HEADER_H;

    // Row 0: Font size multiplier.
    let row0_y = content_y + PAD_V;
    emit_row_bg(&mut out, px, row0_y, 0);
    out.push(make_text(
        "Font size".to_string(),
        px + PAD_H,
        row0_y + (ROW_H - FONT_SIZE) / 2.0,
        68.0,
        FONT_SIZE,
        FontWeight::NORMAL,
        LABEL_COL,
    ));
    emit_font_multiplier_pills(
        &mut out,
        px,
        row0_y,
        panel.draft.font_size_multiplier as f32,
    );

    // Row 1: Reduced motion.
    let row1_y = row0_y + ROW_H;
    emit_row_bg(&mut out, px, row1_y, 1);
    out.push(make_text(
        "Reduced motion".to_string(),
        px + PAD_H,
        row1_y + (ROW_H - FONT_SIZE) / 2.0,
        160.0,
        FONT_SIZE,
        FontWeight::NORMAL,
        LABEL_COL,
    ));
    emit_toggle(&mut out, px + PANEL_W - PAD_H - 52.0, row1_y + (ROW_H - 20.0) / 2.0, panel.draft.reduced_motion);

    // Row 2: Forced colors.
    let row2_y = row1_y + ROW_H;
    emit_row_bg(&mut out, px, row2_y, 0);
    out.push(make_text(
        "Forced colors".to_string(),
        px + PAD_H,
        row2_y + (ROW_H - FONT_SIZE) / 2.0,
        160.0,
        FONT_SIZE,
        FontWeight::NORMAL,
        LABEL_COL,
    ));
    emit_toggle(&mut out, px + PANEL_W - PAD_H - 52.0, row2_y + (ROW_H - 20.0) / 2.0, panel.draft.forced_colors);

    // Row 3: Cursor size.
    let row3_y = row2_y + ROW_H;
    emit_row_bg(&mut out, px, row3_y, 1);
    out.push(make_text(
        "Cursor".to_string(),
        px + PAD_H,
        row3_y + (ROW_H - FONT_SIZE) / 2.0,
        68.0,
        FONT_SIZE,
        FontWeight::NORMAL,
        LABEL_COL,
    ));
    emit_cursor_size_pills(&mut out, px, row3_y, panel.draft.cursor_size);

    // Bottom separator.
    let sep_y = row3_y + ROW_H;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + PAD_H, sep_y + 4.0, PANEL_W - PAD_H * 2.0, 1.0),
        color: SEPARATOR,
    });

    out
}

// ── Row helpers ───────────────────────────────────────────────────────────────

fn emit_row_bg(out: &mut DisplayList, px: f32, row_y: f32, parity: u8) {
    let color = if parity == 0 { ROW_EVEN } else { ROW_ODD };
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, row_y, PANEL_W, ROW_H),
        color,
    });
}

fn emit_font_multiplier_pills(out: &mut DisplayList, px: f32, row_y: f32, current: f32) {
    let pills_x = px + PAD_H + 70.0;
    let available = PANEL_W - PAD_H * 2.0 - 70.0;
    let pill_w = available / 5.0;
    let pill_y = row_y + (ROW_H - PILL_H) / 2.0;

    for (i, &val) in FONT_MULTIPLIERS.iter().enumerate() {
        let bx = pills_x + i as f32 * pill_w;
        let active = (current - val).abs() < 0.01;
        let bg = if active { PILL_ACTIVE } else { PILL_BG };
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(bx, pill_y, pill_w - 3.0, PILL_H),
            color: bg,
            radii: uniform_radii(4.0),
        });
        let label = format_multiplier(val);
        out.push(make_text(
            label,
            bx + 2.0,
            pill_y + (PILL_H - FONT_SIZE) / 2.0,
            pill_w - 4.0,
            FONT_SIZE - 1.0,
            if active { FontWeight::BOLD } else { FontWeight::NORMAL },
            PILL_TEXT,
        ));
    }
}

fn emit_cursor_size_pills(out: &mut DisplayList, px: f32, row_y: f32, current: CursorSize) {
    let pills_x = px + PAD_H + 70.0;
    let available = PANEL_W - PAD_H * 2.0 - 70.0;
    let pill_w = available / 3.0;
    let pill_y = row_y + (ROW_H - PILL_H) / 2.0;

    const SIZES: [(CursorSize, &str); 3] = [
        (CursorSize::Normal, "Normal"),
        (CursorSize::Large, "Large"),
        (CursorSize::ExtraLarge, "Extra"),
    ];
    for (i, (size, label)) in SIZES.iter().enumerate() {
        let bx = pills_x + i as f32 * pill_w;
        let active = current == *size;
        let bg = if active { PILL_ACTIVE } else { PILL_BG };
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(bx, pill_y, pill_w - 3.0, PILL_H),
            color: bg,
            radii: uniform_radii(4.0),
        });
        out.push(make_text(
            (*label).to_string(),
            bx + 2.0,
            pill_y + (PILL_H - FONT_SIZE) / 2.0,
            pill_w - 4.0,
            FONT_SIZE - 1.0,
            if active { FontWeight::BOLD } else { FontWeight::NORMAL },
            PILL_TEXT,
        ));
    }
}

fn emit_toggle(out: &mut DisplayList, x: f32, y: f32, on: bool) {
    let bg = if on { TOGGLE_ON } else { TOGGLE_OFF };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(x, y, 44.0, 20.0),
        color: bg,
        radii: uniform_radii(10.0),
    });
    // Thumb.
    let thumb_x = if on { x + 26.0 } else { x + 2.0 };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(thumb_x, y + 2.0, 16.0, 16.0),
        color: TOGGLE_TEXT,
        radii: uniform_radii(8.0),
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Format a font-size multiplier as a compact label: `"1.0"`, `"1.25"`, etc.
fn format_multiplier(val: f32) -> String {
    if (val - val.floor()).abs() < 0.001 {
        format!("{}", val as u32)
    } else {
        format!("{val:.2}")
            .trim_end_matches('0')
            .to_owned()
    }
}

fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

#[allow(clippy::too_many_arguments)]
fn make_text(
    text: String,
    x: f32,
    y: f32,
    w: f32,
    font_size: f32,
    weight: FontWeight,
    color: Color,
) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text,
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel() -> A11yPanel {
        A11yPanel::new()
    }

    // ── State ──────────────────────────────────────────────────────���───────────

    #[test]
    fn new_panel_hidden_and_default_prefs() {
        let p = make_panel();
        assert!(!p.visible);
        assert!((p.draft.font_size_multiplier - 1.0).abs() < f64::EPSILON);
        assert!(!p.draft.reduced_motion);
        assert!(!p.draft.forced_colors);
        assert_eq!(p.draft.cursor_size, CursorSize::Normal);
    }

    #[test]
    fn toggle_visibility() {
        let mut p = make_panel();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn load_draft_updates_fields() {
        let mut p = make_panel();
        p.load_draft(lumen_storage::A11yPrefsSnapshot {
            font_size_multiplier: 1.5,
            reduced_motion: true,
            forced_colors: false,
            cursor_size: CursorSize::Large,
        });
        assert!((p.draft.font_size_multiplier - 1.5).abs() < f64::EPSILON);
        assert!(p.draft.reduced_motion);
        assert_eq!(p.draft.cursor_size, CursorSize::Large);
    }

    // ── hit_test ───────────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_when_hidden() {
        let p = make_panel();
        assert_eq!(hit_test(&p, 400.0, 300.0, 800.0, 600.0), A11yHit::Outside);
    }

    #[test]
    fn hit_outside_when_visible_but_off_panel() {
        let mut p = make_panel();
        p.visible = true;
        // Panel centred at (250, 170) for 800×600 window.
        assert_eq!(hit_test(&p, 10.0, 10.0, 800.0, 600.0), A11yHit::Outside);
    }

    #[test]
    fn hit_close_on_top_right_of_header() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        // Top-right corner of the header.
        assert_eq!(
            hit_test(&p, px + PANEL_W - 5.0, py + HEADER_H / 2.0, 800.0, 600.0),
            A11yHit::Close
        );
    }

    #[test]
    fn hit_font_multiplier_pill() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let row0_y = py + PANEL_H / 2.0 - PANEL_H / 2.0 + HEADER_H + PAD_V;
        let pills_x = px + PAD_H + 70.0;
        let pill_w = (PANEL_W - PAD_H * 2.0 - 70.0) / 5.0;
        // Click centre of first pill (0.8×).
        let hit = hit_test(&p, pills_x + pill_w * 0.5, row0_y + ROW_H / 2.0, 800.0, 600.0);
        assert!(matches!(hit, A11yHit::FontMultiplier(v) if (v - 0.8).abs() < 0.01));
    }

    #[test]
    fn hit_reduced_motion_toggle() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let row1_y = py + HEADER_H + PAD_V + ROW_H;
        let toggle_x = px + PANEL_W - PAD_H - 30.0;
        assert_eq!(
            hit_test(&p, toggle_x, row1_y + ROW_H / 2.0, 800.0, 600.0),
            A11yHit::ReducedMotion
        );
    }

    #[test]
    fn hit_forced_colors_toggle() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let row2_y = py + HEADER_H + PAD_V + ROW_H * 2.0;
        let toggle_x = px + PANEL_W - PAD_H - 30.0;
        assert_eq!(
            hit_test(&p, toggle_x, row2_y + ROW_H / 2.0, 800.0, 600.0),
            A11yHit::ForcedColors
        );
    }

    #[test]
    fn hit_cursor_size_pill() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let row3_y = py + HEADER_H + PAD_V + ROW_H * 3.0;
        let pills_x = px + PAD_H + 70.0;
        let pill_w = (PANEL_W - PAD_H * 2.0 - 70.0) / 3.0;
        // Centre of second pill (Large).
        let hit = hit_test(&p, pills_x + pill_w * 1.5, row3_y + ROW_H / 2.0, 800.0, 600.0);
        assert_eq!(hit, A11yHit::CursorSizeOption(CursorSize::Large));
    }

    // ── Rendering ──────────────────────────────────────────────────────────────

    #[test]
    fn build_hidden_returns_empty() {
        let p = make_panel();
        assert!(build_a11y_panel(&p, (800, 600)).is_empty());
    }

    #[test]
    fn build_visible_has_header_text() {
        let mut p = make_panel();
        p.visible = true;
        let dl = build_a11y_panel(&p, (800, 600));
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Accessibility")
        });
        assert!(has_title, "panel must have 'Accessibility' header text");
    }

    #[test]
    fn build_visible_has_label_texts() {
        let mut p = make_panel();
        p.visible = true;
        let dl = build_a11y_panel(&p, (800, 600));
        let labels: Vec<&str> = dl.iter().filter_map(|c| {
            if let DisplayCommand::DrawText { text, .. } = c { Some(text.as_str()) } else { None }
        }).collect();
        assert!(labels.contains(&"Font size"), "must show 'Font size' label");
        assert!(labels.contains(&"Reduced motion"), "must show 'Reduced motion' label");
        assert!(labels.contains(&"Forced colors"), "must show 'Forced colors' label");
        assert!(labels.contains(&"Cursor"), "must show 'Cursor' label");
    }
}
