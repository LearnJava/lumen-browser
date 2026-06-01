//! Workspace switcher panel (7A.3): bottom-docked horizontal bar for switching
//! between named workspaces.
//!
//! A workspace is a named group of tabs with optional cookie isolation and a
//! colour label.  The panel renders as a slim bar at the bottom of the window:
//!
//! ```text
//! [ ◉ Default ]  [ 💼 Work ]  [ 🏠 Home ]  [ + ]
//! ```
//!
//! Toggled with Ctrl+Shift+W.  When visible, `viewport_height_css()` subtracts
//! `SWITCHER_HEIGHT` so the page layout does not extend behind the bar.
//!
//! Each chip has a "×" delete button on the right edge.  The "+" button creates
//! a new workspace with an auto-generated name ("Workspace N").  Changes are
//! immediately persisted to the `Workspaces` SQLite store.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants ─────────────────────────────────────────────────────────

/// Height of the workspace switcher bar in CSS px.
pub const SWITCHER_HEIGHT: f32 = 32.0;

/// Horizontal gap between chips in CSS px.
const CHIP_GAP: f32 = 6.0;

/// Minimum chip width (can grow with longer names) in CSS px.
const CHIP_MIN_W: f32 = 80.0;

/// Maximum chip width in CSS px.
const CHIP_MAX_W: f32 = 160.0;

/// Chip height in CSS px.
const CHIP_H: f32 = 22.0;

/// Width of the "×" delete zone on the right side of each chip.
const DELETE_W: f32 = 18.0;

/// Left padding before chip text.
const CHIP_TEXT_PAD: f32 = 10.0;

/// Width of the "+" add button chip.
const ADD_BTN_W: f32 = 30.0;

/// Right margin from panel edge to the "+" button.
const ADD_BTN_RIGHT: f32 = 8.0;

const BAR_BG: Color = Color { r: 14, g: 14, b: 18, a: 255 };
const BAR_TOP_BORDER: Color = Color { r: 38, g: 38, b: 46, a: 255 };
const CHIP_INACTIVE_BG: Color = Color { r: 28, g: 28, b: 34, a: 255 };
const CHIP_ACTIVE_BG: Color = Color { r: 44, g: 44, b: 56, a: 255 };
const TEXT_ACTIVE: Color = Color { r: 218, g: 218, b: 228, a: 255 };
const TEXT_DIM: Color = Color { r: 140, g: 140, b: 148, a: 255 };
const DELETE_FG: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const ADD_FG: Color = Color { r: 120, g: 180, b: 120, a: 255 };
const CHIP_RADIUS: f32 = 5.0;
const FONT_SZ: f32 = 11.0;

// ── Data types ────────────────────────────────────────────────────────────────

/// Lightweight workspace entry used for panel rendering (loaded from storage on
/// each panel refresh).
#[derive(Debug, Clone)]
pub struct WsEntry {
    /// Workspace database id.
    pub id: i64,
    /// Display name.
    pub name: String,
    /// Accent colour parsed from the stored CSS colour string.  Falls back to a
    /// neutral grey when the stored value is malformed.
    pub accent: Color,
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Workspace switcher panel state.
pub struct WorkspacePanel {
    /// `true` while the bar is visible.  Toggled via Ctrl+Shift+W.
    pub visible: bool,
    /// Cached workspace list — refreshed after every create / delete / switch.
    pub workspaces: Vec<WsEntry>,
    /// Id of the currently active workspace.  `None` = default (no workspace
    /// selected), which is the state before any workspace is created.
    pub active_id: Option<i64>,
}

impl WorkspacePanel {
    /// Create a new (hidden) panel with an empty workspace list.
    pub fn new() -> Self {
        Self {
            visible: false,
            workspaces: Vec::new(),
            active_id: None,
        }
    }

    /// Flip visibility.  Caller must trigger redraw (and relayout if changing
    /// the visible viewport height matters for the current page).
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Replace the cached workspace list (call after any storage mutation).
    pub fn set_workspaces(&mut self, entries: Vec<WsEntry>) {
        self.workspaces = entries;
    }

    /// Mark `id` as the active workspace.
    pub fn set_active(&mut self, id: Option<i64>) {
        self.active_id = id;
    }
}

impl Default for WorkspacePanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the workspace switcher bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHit {
    /// Switch to workspace with the given id.
    SwitchTo(i64),
    /// Delete workspace with the given id (clicked the "×" on a chip).
    DeleteWorkspace(i64),
    /// Create a new workspace (clicked the "+" button).
    NewWorkspace,
    /// Clicked on the bar background (no actionable target).
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the workspace switcher bar.
///
/// Returns `None` when the click is outside the bar region.
/// `window_h` is the full window height in CSS px (including the tab bar).
pub fn hit_test(
    panel: &WorkspacePanel,
    x: f32,
    y: f32,
    window_w: f32,
    window_h: f32,
) -> Option<WorkspaceHit> {
    let bar_top = window_h - SWITCHER_HEIGHT;
    if y < bar_top || y >= window_h || x < 0.0 || x >= window_w {
        return None;
    }

    // "+" button — right-aligned.
    let add_left = window_w - ADD_BTN_RIGHT - ADD_BTN_W;
    if x >= add_left && x < add_left + ADD_BTN_W {
        return Some(WorkspaceHit::NewWorkspace);
    }

    // Workspace chips — left-aligned with CHIP_GAP spacing.
    let mut cursor_x = CHIP_GAP;
    let chip_top = bar_top + (SWITCHER_HEIGHT - CHIP_H) * 0.5;
    let chip_bottom = chip_top + CHIP_H;

    if y < chip_top || y > chip_bottom {
        return Some(WorkspaceHit::Empty);
    }

    for entry in &panel.workspaces {
        let chip_w = chip_width(&entry.name);
        let chip_right = cursor_x + chip_w;

        if x >= cursor_x && x < chip_right {
            // Inside this chip — check for delete zone.
            let delete_left = chip_right - DELETE_W;
            if x >= delete_left {
                return Some(WorkspaceHit::DeleteWorkspace(entry.id));
            }
            return Some(WorkspaceHit::SwitchTo(entry.id));
        }

        cursor_x = chip_right + CHIP_GAP;
        // Stop scanning if we've passed the add button area.
        if cursor_x >= add_left {
            break;
        }
    }

    Some(WorkspaceHit::Empty)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the workspace switcher bar.
///
/// The bar occupies `y = window_h - SWITCHER_HEIGHT .. window_h`, full window
/// width.  It is rendered as an overlay on top of the page content.
pub fn build_panel(panel: &WorkspacePanel, window_w: f32, window_h: f32) -> DisplayList {
    let bar_top = window_h - SWITCHER_HEIGHT;
    let mut out = DisplayList::with_capacity(8 + panel.workspaces.len() * 6);

    // Bar background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, bar_top, window_w, SWITCHER_HEIGHT),
        color: BAR_BG,
    });

    // 1px top border.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, bar_top, window_w, 1.0),
        color: BAR_TOP_BORDER,
    });

    let chip_top = bar_top + (SWITCHER_HEIGHT - CHIP_H) * 0.5;
    let add_left = window_w - ADD_BTN_RIGHT - ADD_BTN_W;
    let radii = CornerRadii {
        tl: CHIP_RADIUS, tl_y: CHIP_RADIUS,
        tr: CHIP_RADIUS, tr_y: CHIP_RADIUS,
        br: CHIP_RADIUS, br_y: CHIP_RADIUS,
        bl: CHIP_RADIUS, bl_y: CHIP_RADIUS,
    };

    // Workspace chips.
    let mut cursor_x = CHIP_GAP;
    for entry in &panel.workspaces {
        let chip_w = chip_width(&entry.name);
        let chip_right = cursor_x + chip_w;

        // Stop if chip would overlap the add button.
        if cursor_x + chip_w > add_left - CHIP_GAP {
            break;
        }

        let is_active = panel.active_id == Some(entry.id);
        let bg = if is_active { CHIP_ACTIVE_BG } else { CHIP_INACTIVE_BG };

        // Chip background.
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(cursor_x, chip_top, chip_w, CHIP_H),
            radii,
            color: bg,
        });

        // Active accent: 2px bottom bar with workspace colour.
        if is_active {
            let accent = entry.accent;
            out.push(DisplayCommand::FillRoundedRect {
                rect: Rect::new(cursor_x + 2.0, chip_top + CHIP_H - 3.0, chip_w - 4.0, 2.0),
                radii: CornerRadii {
                    tl: 1.0, tl_y: 1.0,
                    tr: 1.0, tr_y: 1.0,
                    br: 1.0, br_y: 1.0,
                    bl: 1.0, bl_y: 1.0,
                },
                color: accent,
            });
        }

        // Delete "×" label (right side).
        let del_left = chip_right - DELETE_W;
        let del_text_top = chip_top + (CHIP_H - FONT_SZ * 1.2) * 0.5;
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(del_left, del_text_top, DELETE_W, FONT_SZ * 1.2),
            text: "×".to_owned(),
            font_size: FONT_SZ,
            color: DELETE_FG,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        // Workspace name (truncated to leave room for "×").
        let name_w = (del_left - cursor_x - CHIP_TEXT_PAD - 2.0).max(0.0);
        let name_top = chip_top + (CHIP_H - FONT_SZ * 1.3) * 0.5;
        let text_color = if is_active { TEXT_ACTIVE } else { TEXT_DIM };
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(cursor_x + CHIP_TEXT_PAD, name_top, name_w, FONT_SZ * 1.3),
            text: entry.name.clone(),
            font_size: FONT_SZ,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        cursor_x = chip_right + CHIP_GAP;
    }

    // "+" add button (right-aligned).
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(add_left, chip_top, ADD_BTN_W, CHIP_H),
        radii,
        color: CHIP_INACTIVE_BG,
    });
    let add_text_top = chip_top + (CHIP_H - FONT_SZ * 1.2) * 0.5;
    let add_text_x = add_left + (ADD_BTN_W - FONT_SZ) * 0.5;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(add_text_x, add_text_top, FONT_SZ * 1.2, FONT_SZ * 1.2),
        text: "+".to_owned(),
        font_size: FONT_SZ * 1.1,
        color: ADD_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a stored CSS colour string (`#RRGGBB`, `#RGB`, or named colour
/// `red`/`green`/`blue`/`purple`/…) into a render `Color`.  Falls back to a
/// neutral blue-grey when the value cannot be parsed.
pub fn parse_ws_color(s: &str) -> Color {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(100);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(130);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
                return Color { r, g, b, a: 255 };
            }
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).unwrap_or(6);
                let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(8);
                let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(15);
                return Color { r: r * 17, g: g * 17, b: b * 17, a: 255 };
            }
            _ => {}
        }
    }
    // Simple named colours.
    match s {
        "red"    => Color { r: 220, g:  80, b:  80, a: 255 },
        "green"  => Color { r:  80, g: 200, b: 100, a: 255 },
        "blue"   => Color { r:  80, g: 130, b: 255, a: 255 },
        "yellow" => Color { r: 255, g: 210, b:  60, a: 255 },
        "purple" => Color { r: 160, g:  80, b: 220, a: 255 },
        "orange" => Color { r: 255, g: 140, b:  40, a: 255 },
        "pink"   => Color { r: 255, g: 130, b: 180, a: 255 },
        "cyan"   => Color { r:  60, g: 200, b: 220, a: 255 },
        _        => Color { r: 100, g: 130, b: 220, a: 255 },
    }
}

/// Compute chip width from workspace name length (capped at CHIP_MAX_W).
fn chip_width(name: &str) -> f32 {
    // Rough approximation: each character ≈ 7px at 11px font size.
    let text_w = name.chars().count() as f32 * 7.0;
    let needed = CHIP_TEXT_PAD + text_w + DELETE_W + 2.0;
    needed.clamp(CHIP_MIN_W, CHIP_MAX_W)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_W: f32 = 1024.0;
    const WIN_H: f32 = 720.0;

    fn make_panel(names: &[&str], active: Option<i64>) -> WorkspacePanel {
        let mut p = WorkspacePanel::new();
        p.visible = true;
        p.workspaces = names
            .iter()
            .enumerate()
            .map(|(i, n)| WsEntry {
                id: i as i64 + 1,
                name: (*n).to_owned(),
                accent: Color { r: 100, g: 160, b: 255, a: 255 },
            })
            .collect();
        p.active_id = active;
        p
    }

    // ── Panel state ──────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden() {
        let p = WorkspacePanel::new();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_shows_panel() {
        let mut p = WorkspacePanel::new();
        p.toggle();
        assert!(p.visible);
    }

    #[test]
    fn double_toggle_hides_panel() {
        let mut p = WorkspacePanel::new();
        p.toggle();
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn set_active_updates_id() {
        let mut p = WorkspacePanel::new();
        p.set_active(Some(3));
        assert_eq!(p.active_id, Some(3));
        p.set_active(None);
        assert_eq!(p.active_id, None);
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_bar_returns_none() {
        let p = make_panel(&["Work"], Some(1));
        // Click above the bar.
        assert_eq!(hit_test(&p, 100.0, WIN_H - SWITCHER_HEIGHT - 1.0, WIN_W, WIN_H), None);
    }

    #[test]
    fn hit_add_button() {
        let p = make_panel(&["Work"], None);
        let add_center_x = WIN_W - ADD_BTN_RIGHT - ADD_BTN_W * 0.5;
        let bar_center_y = WIN_H - SWITCHER_HEIGHT * 0.5;
        let hit = hit_test(&p, add_center_x, bar_center_y, WIN_W, WIN_H);
        assert_eq!(hit, Some(WorkspaceHit::NewWorkspace));
    }

    #[test]
    fn hit_first_chip_switch() {
        let p = make_panel(&["Work", "Home"], Some(2));
        // First chip starts at x=CHIP_GAP, body area (not delete zone).
        let chip_center_x = CHIP_GAP + CHIP_MIN_W * 0.4;
        let bar_center_y = WIN_H - SWITCHER_HEIGHT * 0.5;
        let hit = hit_test(&p, chip_center_x, bar_center_y, WIN_W, WIN_H);
        assert_eq!(hit, Some(WorkspaceHit::SwitchTo(1)));
    }

    #[test]
    fn hit_chip_delete_zone() {
        let p = make_panel(&["Work"], None);
        let chip_w = chip_width("Work");
        // Delete zone: right CHIP_GAP of chip.
        let del_x = CHIP_GAP + chip_w - DELETE_W * 0.5;
        let bar_center_y = WIN_H - SWITCHER_HEIGHT * 0.5;
        let hit = hit_test(&p, del_x, bar_center_y, WIN_W, WIN_H);
        assert_eq!(hit, Some(WorkspaceHit::DeleteWorkspace(1)));
    }

    #[test]
    fn hit_empty_bar_background() {
        let p = make_panel(&[], None);
        let bar_center_y = WIN_H - SWITCHER_HEIGHT * 0.5;
        let hit = hit_test(&p, WIN_W * 0.5, bar_center_y, WIN_W, WIN_H);
        assert_eq!(hit, Some(WorkspaceHit::Empty));
    }

    // ── Rendering ────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_emits_commands() {
        let p = make_panel(&["Work"], Some(1));
        let dl = build_panel(&p, WIN_W, WIN_H);
        assert!(!dl.is_empty(), "panel must emit at least a background rect");
    }

    #[test]
    fn build_panel_draws_chip_names() {
        let p = make_panel(&["Alpha", "Beta"], Some(1));
        let dl = build_panel(&p, WIN_W, WIN_H);
        let has_alpha = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Alpha")
        });
        let has_beta = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Beta")
        });
        assert!(has_alpha && has_beta, "panel must draw workspace names");
    }

    #[test]
    fn build_panel_draws_add_button() {
        let p = make_panel(&[], None);
        let dl = build_panel(&p, WIN_W, WIN_H);
        let has_plus = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "+")
        });
        assert!(has_plus, "panel must draw the '+' add button");
    }

    #[test]
    fn build_panel_no_chips_for_empty_list() {
        let p = make_panel(&[], None);
        let dl = build_panel(&p, WIN_W, WIN_H);
        let name_texts: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text != "+" && text != "×"))
            .collect();
        assert!(
            name_texts.is_empty(),
            "no name text when workspace list is empty"
        );
    }

    // ── Color parsing ────────────────────────────────────────────────────────

    #[test]
    fn parse_hex6_color() {
        let c = parse_ws_color("#ff8040");
        assert_eq!((c.r, c.g, c.b), (0xff, 0x80, 0x40));
    }

    #[test]
    fn parse_hex3_color() {
        let c = parse_ws_color("#f80");
        assert_eq!((c.r, c.g, c.b), (0xff, 0x88, 0x00));
    }

    #[test]
    fn parse_named_color_red() {
        let c = parse_ws_color("red");
        assert!(c.r > c.g && c.r > c.b, "red must have highest R component");
    }

    #[test]
    fn parse_unknown_color_fallback() {
        let c = parse_ws_color("not-a-color");
        // Fallback must produce a fully-opaque colour.
        assert_eq!(c.a, 255);
    }
}
