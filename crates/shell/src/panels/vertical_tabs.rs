//! Vertical tabs panel (7A.1): sidebar tab list docked to the left edge.
//!
//! Shows all open tabs as a vertical list with title, favicon placeholder,
//! lifecycle badge, and close button. Toggle with Ctrl+B.
//!
//! Panel occupies `x = 0..PANEL_WIDTH`, `y = tab_bar_height..window_h`.
//! When visible, `viewport_width_css()` subtracts `PANEL_WIDTH` and the page
//! display list is shifted right by `PANEL_WIDTH` before rendering.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::tab_lifecycle::state::TabState;
use crate::tabs::strip::TabStrip;

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the vertical tab panel in CSS px.
pub const PANEL_WIDTH: f32 = 200.0;

/// Height of each tab row in CSS px.
pub const ROW_H: f32 = 36.0;

const PANEL_BG: Color = Color { r: 18, g: 18, b: 22, a: 255 };
const ROW_INACTIVE_BG: Color = Color { r: 24, g: 24, b: 29, a: 255 };
const ROW_ACTIVE_BG: Color = Color { r: 32, g: 32, b: 40, a: 255 };
const ACCENT: Color = Color { r: 100, g: 160, b: 255, a: 255 };
const TEXT_ACTIVE: Color = Color { r: 218, g: 218, b: 228, a: 255 };
const TEXT_DIM: Color = Color { r: 140, g: 140, b: 148, a: 255 };
const CLOSE_FG: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const DIVIDER: Color = Color { r: 35, g: 36, b: 42, a: 255 };
const ICON_BG: Color = Color { r: 60, g: 60, b: 70, a: 255 };
const BADGE_OLD: Color = Color { r: 255, g: 168, b: 0, a: 210 };
const BADGE_HIB: Color = Color { r: 110, g: 110, b: 120, a: 210 };

const FONT_SZ: f32 = 12.0;
/// Left margin for the favicon icon square.
const ICON_LEFT: f32 = 12.0;
/// Favicon square width/height.
const ICON_SZ: f32 = 16.0;
/// Left edge of tab title text.
const TEXT_LEFT: f32 = ICON_LEFT + ICON_SZ + 8.0;
/// Width of the close button glyph area.
const CLOSE_W: f32 = 16.0;
/// Right margin from panel edge to close button.
const CLOSE_RIGHT_MARGIN: f32 = 8.0;
/// Lifecycle badge dot diameter.
const BADGE_SZ: f32 = 5.0;

// ── Panel state ───────────────────────────────────────────────────────────────

/// Vertical tabs panel: list of open tabs rendered as a left-docked sidebar.
pub struct VerticalTabsPanel {
    /// `true` while the panel is visible. Toggled via Ctrl+B.
    pub visible: bool,
}

impl VerticalTabsPanel {
    /// Create a new (hidden) panel.
    pub fn new() -> Self {
        Self { visible: false }
    }

    /// Flip visibility. Caller must trigger relayout + redraw.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
}

impl Default for VerticalTabsPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the vertical tab panel area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VTabHit {
    /// Clicked the tab row body (not the close button). Index = tab index.
    Tab(usize),
    /// Clicked the close × button. Index = tab index.
    Close(usize),
    /// Clicked the panel background (below all tab rows).
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the vertical tabs panel.
///
/// Returns `None` if the coordinates are outside the panel bounds.
/// `tab_bar_height` is the horizontal tab strip height (panel starts below it).
/// `window_h` is the full window height in CSS px.
pub fn hit_test(
    strip: &TabStrip,
    x: f32,
    y: f32,
    tab_bar_height: f32,
    window_h: f32,
) -> Option<VTabHit> {
    if x >= PANEL_WIDTH || y < tab_bar_height || y >= window_h {
        return None;
    }
    let row_y = y - tab_bar_height;
    let idx = (row_y / ROW_H) as usize;
    if idx >= strip.tabs.len() {
        return Some(VTabHit::Empty);
    }
    let close_left = PANEL_WIDTH - CLOSE_RIGHT_MARGIN - CLOSE_W;
    if x >= close_left {
        Some(VTabHit::Close(idx))
    } else {
        Some(VTabHit::Tab(idx))
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the vertical tabs panel.
///
/// Panel occupies `x = 0..PANEL_WIDTH`, `y = tab_bar_height..window_h`.
/// Rows are clipped to the visible panel area; rows scrolled off the bottom
/// are not emitted.
pub fn build_panel(strip: &TabStrip, tab_bar_height: f32, window_h: f32) -> DisplayList {
    let panel_h = (window_h - tab_bar_height).max(0.0);
    let mut out = DisplayList::with_capacity(4 + strip.tabs.len() * 8);

    // Panel background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, tab_bar_height, PANEL_WIDTH, panel_h),
        color: PANEL_BG,
    });

    // Right border divider (1 px, full panel height).
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(PANEL_WIDTH - 1.0, tab_bar_height, 1.0, panel_h),
        color: DIVIDER,
    });

    for (i, tab) in strip.tabs.iter().enumerate() {
        let row_top = tab_bar_height + i as f32 * ROW_H;
        if row_top >= window_h {
            break;
        }
        let is_active = i == strip.active;
        let row_bg = if is_active { ROW_ACTIVE_BG } else { ROW_INACTIVE_BG };

        // Row background (excludes right border pixel).
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(0.0, row_top, PANEL_WIDTH - 1.0, ROW_H),
            color: row_bg,
        });

        // Active-tab left accent bar (2 px).
        if is_active {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(0.0, row_top, 2.0, ROW_H),
                color: ACCENT,
            });
        }

        // Row bottom divider.
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(0.0, row_top + ROW_H - 1.0, PANEL_WIDTH - 1.0, 1.0),
            color: DIVIDER,
        });

        // Favicon placeholder square.
        let icon_top = row_top + (ROW_H - ICON_SZ) * 0.5;
        let icon_r = 2.0_f32;
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(ICON_LEFT, icon_top, ICON_SZ, ICON_SZ),
            radii: CornerRadii {
                tl: icon_r, tl_y: icon_r,
                tr: icon_r, tr_y: icon_r,
                br: icon_r, br_y: icon_r,
                bl: icon_r, bl_y: icon_r,
            },
            color: ICON_BG,
        });

        // Lifecycle badge — small circle at top-right of the favicon.
        let badge_color = match tab.tab_state {
            TabState::BackgroundOld => Some(BADGE_OLD),
            TabState::Hibernated => Some(BADGE_HIB),
            _ => None,
        };
        if let Some(color) = badge_color {
            let bx = ICON_LEFT + ICON_SZ - BADGE_SZ * 0.5;
            let by = icon_top - BADGE_SZ * 0.5;
            let r = BADGE_SZ / 2.0;
            out.push(DisplayCommand::FillRoundedRect {
                rect: Rect::new(bx, by, BADGE_SZ, BADGE_SZ),
                radii: CornerRadii {
                    tl: r, tl_y: r,
                    tr: r, tr_y: r,
                    br: r, br_y: r,
                    bl: r, bl_y: r,
                },
                color,
            });
        }

        // Close button ×.
        let close_left = PANEL_WIDTH - CLOSE_RIGHT_MARGIN - CLOSE_W;
        let close_top = row_top + (ROW_H - FONT_SZ * 1.2) * 0.5;
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(close_left, close_top, CLOSE_W, FONT_SZ * 1.2),
            text: "×".to_owned(),
            font_size: FONT_SZ,
            color: CLOSE_FG,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });

        // Tab title, truncated between icon and close button.
        let text_right = close_left - 4.0;
        let text_w = (text_right - TEXT_LEFT).max(0.0);
        let text_top = row_top + (ROW_H - FONT_SZ * 1.3) * 0.5;
        let text_color = if is_active { TEXT_ACTIVE } else { TEXT_DIM };
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(TEXT_LEFT, text_top, text_w, FONT_SZ * 1.3),
            text: tab.title.clone(),
            font_size: FONT_SZ,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
    }

    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::strip::TabStrip;

    const TAB_H: f32 = 36.0; // reuse TAB_BAR_HEIGHT value
    const WIN_H: f32 = 720.0;

    fn strip2() -> TabStrip {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s
    }

    // ── Panel state ──────────────────────────────────────────────────────────

    #[test]
    fn new_panel_is_hidden() {
        let p = VerticalTabsPanel::new();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_makes_visible() {
        let mut p = VerticalTabsPanel::new();
        p.toggle();
        assert!(p.visible);
    }

    #[test]
    fn double_toggle_hides_again() {
        let mut p = VerticalTabsPanel::new();
        p.toggle();
        p.toggle();
        assert!(!p.visible);
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_panel_returns_none() {
        let s = TabStrip::new();
        // x = PANEL_WIDTH is outside
        assert_eq!(hit_test(&s, PANEL_WIDTH, 50.0, TAB_H, WIN_H), None);
    }

    #[test]
    fn hit_inside_tab_bar_returns_none() {
        let s = TabStrip::new();
        // y < TAB_H is the horizontal tab bar area
        assert_eq!(hit_test(&s, 10.0, TAB_H - 1.0, TAB_H, WIN_H), None);
    }

    #[test]
    fn hit_first_row_body() {
        let s = TabStrip::new();
        // First row: y = TAB_H..TAB_H+ROW_H; click in the middle of the row body
        let hit = hit_test(&s, 50.0, TAB_H + ROW_H * 0.5, TAB_H, WIN_H);
        assert_eq!(hit, Some(VTabHit::Tab(0)));
    }

    #[test]
    fn hit_close_button() {
        let s = TabStrip::new();
        // Close button starts at PANEL_WIDTH - CLOSE_RIGHT_MARGIN - CLOSE_W
        let close_x = PANEL_WIDTH - CLOSE_RIGHT_MARGIN - CLOSE_W + 2.0;
        let hit = hit_test(&s, close_x, TAB_H + ROW_H * 0.5, TAB_H, WIN_H);
        assert_eq!(hit, Some(VTabHit::Close(0)));
    }

    #[test]
    fn hit_second_row() {
        let s = strip2();
        let row2_y = TAB_H + ROW_H + ROW_H * 0.5;
        let hit = hit_test(&s, 50.0, row2_y, TAB_H, WIN_H);
        assert_eq!(hit, Some(VTabHit::Tab(1)));
    }

    #[test]
    fn hit_below_all_rows_returns_empty() {
        let s = TabStrip::new(); // 1 tab → rows end at TAB_H + ROW_H
        let below_y = TAB_H + ROW_H + 1.0;
        let hit = hit_test(&s, 50.0, below_y, TAB_H, WIN_H);
        assert_eq!(hit, Some(VTabHit::Empty));
    }

    // ── Rendering ────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_emits_commands() {
        let s = TabStrip::new();
        let dl = build_panel(&s, TAB_H, WIN_H);
        assert!(!dl.is_empty());
    }

    #[test]
    fn build_panel_has_title_text() {
        let s = TabStrip::new();
        let dl = build_panel(&s, TAB_H, WIN_H);
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("вкладка"))
        });
        assert!(has_title, "panel must draw tab title");
    }

    #[test]
    fn build_panel_no_badge_for_active() {
        let s = TabStrip::new(); // single Active tab
        let dl = build_panel(&s, TAB_H, WIN_H);
        // Active tab has no badge (only FillRect background + accent bar, no badge radii for lifecycle)
        // Panel uses FillRoundedRect for favicon + possibly badge. Badge colors are BADGE_OLD/BADGE_HIB.
        let has_lifecycle_badge = dl.iter().any(|c| match c {
            DisplayCommand::FillRoundedRect { color, .. } => {
                (color.r == BADGE_OLD.r && color.g == BADGE_OLD.g)
                    || (color.r == BADGE_HIB.r && color.g == BADGE_HIB.g)
            }
            _ => false,
        });
        assert!(!has_lifecycle_badge, "Active tab must not render lifecycle badge");
    }

    #[test]
    fn build_panel_badge_for_background_old() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.set_tab_state(0, TabState::BackgroundOld);
        let dl = build_panel(&s, TAB_H, WIN_H);
        let has_amber = dl.iter().any(|c| match c {
            DisplayCommand::FillRoundedRect { color, .. } => {
                color.r == BADGE_OLD.r && color.g == BADGE_OLD.g
            }
            _ => false,
        });
        assert!(has_amber, "BackgroundOld tab must render amber badge");
    }

    #[test]
    fn build_panel_badge_for_hibernated() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.set_tab_state(0, TabState::Hibernated);
        let dl = build_panel(&s, TAB_H, WIN_H);
        let has_grey = dl.iter().any(|c| match c {
            DisplayCommand::FillRoundedRect { color, .. } => {
                color.r == BADGE_HIB.r && color.g == BADGE_HIB.g
            }
            _ => false,
        });
        assert!(has_grey, "Hibernated tab must render grey badge");
    }

    #[test]
    fn build_panel_clips_rows_to_window_height() {
        let mut s = TabStrip::new();
        // Add many tabs so they would overflow.
        for _ in 0..30 {
            s.push_blank(0.0);
        }
        // window_h only fits a few rows.
        let small_h = TAB_H + 3.0 * ROW_H;
        let dl = build_panel(&s, TAB_H, small_h);
        // Count DrawText commands with tab titles — must be <= 3 (rows that fit).
        let title_count = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("вкладка"))
        }).count();
        assert!(title_count <= 3, "must not render rows beyond window height");
    }
}
