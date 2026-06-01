//! Tree-style tabs panel (7A.2): vertical sidebar showing parent-child tab tree.
//!
//! Extends the flat vertical-tabs view (7A.1) with:
//! - Children indented 8 px per depth level.
//! - Collapse/expand triangles (▶/▼) for tabs that have children.
//! - `TreeTabsPanel` state tracking which subtrees are collapsed.
//!
//! The panel shares the same visual constants (size, colours) as
//! `vertical_tabs`: `PANEL_WIDTH = 200`, `ROW_H = 36`.
//!
//! # Layout
//! ```text
//! x=0                      x=PANEL_WIDTH=200
//! ┌──────────────────────────┐
//! │▼ Parent tab title      × │  depth=0
//! │  ▼ Child tab title     × │  depth=1 → indent 8px
//! │      Grandchild        × │  depth=2 → indent 16px
//! │▶ Collapsed tab         × │  depth=0, children hidden
//! └──────────────────────────┘
//! ```

use std::collections::HashSet;

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::tab_lifecycle::state::TabState;
use crate::tabs::strip::TabStrip;
use crate::tabs::tree::visible_order;

// ── Visual constants (shared with vertical_tabs) ──────────────────────────────

/// Width of the tree-tabs panel in CSS px.
pub const PANEL_WIDTH: f32 = 200.0;

/// Height of each tab row in CSS px.
pub const ROW_H: f32 = 36.0;

/// Indent per depth level in CSS px.
const INDENT: f32 = 8.0;

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
const ARROW_COLOR: Color = Color { r: 160, g: 160, b: 170, a: 255 };

const FONT_SZ: f32 = 12.0;
/// Left margin before the collapse/expand arrow.
const ARROW_LEFT_BASE: f32 = 4.0;
/// Width reserved for the arrow glyph.
const ARROW_W: f32 = 10.0;
/// Gap between arrow right edge and favicon.
const ARROW_GAP: f32 = 2.0;
/// Favicon left edge relative to row start (after arrow area + indent).
const ICON_LEFT_BASE: f32 = ARROW_LEFT_BASE + ARROW_W + ARROW_GAP;
/// Favicon square size.
const ICON_SZ: f32 = 16.0;
/// Left edge of title text (relative to row start, before indent).
const TEXT_LEFT_BASE: f32 = ICON_LEFT_BASE + ICON_SZ + 8.0;
/// Width of the close button glyph.
const CLOSE_W: f32 = 16.0;
/// Right margin from panel edge to close button.
const CLOSE_RIGHT_MARGIN: f32 = 8.0;
/// Lifecycle badge dot size.
const BADGE_SZ: f32 = 5.0;

// ── Panel state ───────────────────────────────────────────────────────────────

/// Tree-style tabs panel state.
///
/// Tracks which tab subtrees are currently collapsed (children hidden).
/// The panel is hidden by default; toggled via the same `Ctrl+B` as the flat
/// vertical tabs panel — callers decide which one to show.
pub struct TreeTabsPanel {
    /// `true` while the panel is visible.
    pub visible: bool,
    /// Set of tab IDs whose direct children are hidden.
    pub collapsed: HashSet<usize>,
}

impl TreeTabsPanel {
    /// Create a new hidden panel with no collapsed subtrees.
    pub fn new() -> Self {
        Self { visible: false, collapsed: HashSet::new() }
    }

    /// Flip visibility. Caller must trigger relayout + redraw.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Toggle the collapsed state of the subtree rooted at `tab_id`.
    ///
    /// If the subtree is currently expanded, it becomes collapsed (children
    /// hidden). If collapsed, it becomes expanded. No-op if the tab has no
    /// children — caller should check `VisibleRow::has_children`.
    pub fn toggle_collapsed(&mut self, tab_id: usize) {
        if self.collapsed.contains(&tab_id) {
            self.collapsed.remove(&tab_id);
        } else {
            self.collapsed.insert(tab_id);
        }
    }
}

impl Default for TreeTabsPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the tree tabs panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeTabHit {
    /// Clicked the collapse/expand arrow. Contains the tab ID (not strip index).
    Arrow(usize),
    /// Clicked the row body (not arrow, not close). Contains the strip index.
    Tab(usize),
    /// Clicked the close × button. Contains the strip index.
    Close(usize),
    /// Clicked panel background below all rows.
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the tree tabs panel.
///
/// Returns `None` if the point is outside the panel bounds.
/// `tab_bar_height` is the horizontal tab strip height (panel starts below it).
pub fn hit_test(
    strip: &TabStrip,
    panel: &TreeTabsPanel,
    x: f32,
    y: f32,
    tab_bar_height: f32,
    window_h: f32,
) -> Option<TreeTabHit> {
    if x >= PANEL_WIDTH || y < tab_bar_height || y >= window_h {
        return None;
    }
    let rows = visible_order(&strip.tabs, &panel.collapsed);
    let row_y = y - tab_bar_height;
    let row_pos = (row_y / ROW_H) as usize;
    if row_pos >= rows.len() {
        return Some(TreeTabHit::Empty);
    }
    let row = rows[row_pos];
    let indent = row.depth as f32 * INDENT;
    let arrow_left = ARROW_LEFT_BASE + indent;
    let arrow_right = arrow_left + ARROW_W;
    let close_left = PANEL_WIDTH - CLOSE_RIGHT_MARGIN - CLOSE_W;

    if x >= close_left {
        Some(TreeTabHit::Close(row.strip_idx))
    } else if row.has_children && x >= arrow_left && x < arrow_right {
        Some(TreeTabHit::Arrow(row.id))
    } else {
        Some(TreeTabHit::Tab(row.strip_idx))
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the tree-style tabs panel.
///
/// Panel occupies `x = 0..PANEL_WIDTH`, `y = tab_bar_height..window_h`.
/// Rows correspond to the visible tabs from [`visible_order`]; collapsed
/// subtrees are omitted. Each row is indented `depth × 8` px.
pub fn build_panel(
    strip: &TabStrip,
    panel: &TreeTabsPanel,
    tab_bar_height: f32,
    window_h: f32,
) -> DisplayList {
    let panel_h = (window_h - tab_bar_height).max(0.0);
    let rows = visible_order(&strip.tabs, &panel.collapsed);
    let mut out = DisplayList::with_capacity(4 + rows.len() * 9);

    // Panel background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, tab_bar_height, PANEL_WIDTH, panel_h),
        color: PANEL_BG,
    });

    // Right border divider.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(PANEL_WIDTH - 1.0, tab_bar_height, 1.0, panel_h),
        color: DIVIDER,
    });

    for (row_pos, row) in rows.iter().enumerate() {
        let row_top = tab_bar_height + row_pos as f32 * ROW_H;
        if row_top >= window_h {
            break;
        }
        let tab = &strip.tabs[row.strip_idx];
        let is_active = row.strip_idx == strip.active;
        let row_bg = if is_active { ROW_ACTIVE_BG } else { ROW_INACTIVE_BG };
        let indent = row.depth as f32 * INDENT;

        // Row background.
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

        // Collapse/expand arrow.
        if row.has_children {
            let arrow_left = ARROW_LEFT_BASE + indent;
            let arrow_top = row_top + (ROW_H - FONT_SZ * 1.2) * 0.5;
            let is_collapsed = panel.collapsed.contains(&row.id);
            let arrow_ch = if is_collapsed { "▶" } else { "▼" };
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(arrow_left, arrow_top, ARROW_W, FONT_SZ * 1.2),
                text: arrow_ch.to_owned(),
                font_size: FONT_SZ * 0.8,
                color: ARROW_COLOR,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
            });
        }

        // Favicon placeholder.
        let icon_left = ICON_LEFT_BASE + indent;
        let icon_top = row_top + (ROW_H - ICON_SZ) * 0.5;
        let icon_r = 2.0_f32;
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(icon_left, icon_top, ICON_SZ, ICON_SZ),
            radii: CornerRadii {
                tl: icon_r, tl_y: icon_r,
                tr: icon_r, tr_y: icon_r,
                br: icon_r, br_y: icon_r,
                bl: icon_r, bl_y: icon_r,
            },
            color: ICON_BG,
        });

        // Lifecycle badge.
        let badge_color = match tab.tab_state {
            TabState::BackgroundOld => Some(BADGE_OLD),
            TabState::Hibernated => Some(BADGE_HIB),
            _ => None,
        };
        if let Some(color) = badge_color {
            let bx = icon_left + ICON_SZ - BADGE_SZ * 0.5;
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
        });

        // Tab title.
        let text_left = TEXT_LEFT_BASE + indent;
        let text_right = close_left - 4.0;
        let text_w = (text_right - text_left).max(0.0);
        let text_top = row_top + (ROW_H - FONT_SZ * 1.3) * 0.5;
        let text_color = if is_active { TEXT_ACTIVE } else { TEXT_DIM };
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(text_left, text_top, text_w, FONT_SZ * 1.3),
            text: tab.title.clone(),
            font_size: FONT_SZ,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });
    }

    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::strip::TabStrip;

    const TAB_H: f32 = 36.0;
    const WIN_H: f32 = 720.0;

    fn strip_with_child() -> TabStrip {
        let mut s = TabStrip::new(); // tab id=0
        let root_id = s.tabs[0].id;
        s.push_with_opener(root_id); // tab id=1, child of 0
        s
    }

    fn strip_with_grandchild() -> TabStrip {
        let mut s = TabStrip::new(); // id=0
        let root_id = s.tabs[0].id;
        s.push_with_opener(root_id); // id=1
        let child_id = s.tabs[1].id;
        s.push_with_opener(child_id); // id=2
        s
    }

    // ── Panel state ──────────────────────────────────────────────────────────

    #[test]
    fn new_panel_is_hidden() {
        assert!(!TreeTabsPanel::new().visible);
    }

    #[test]
    fn toggle_panel_visibility() {
        let mut p = TreeTabsPanel::new();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_collapsed_adds_and_removes() {
        let mut p = TreeTabsPanel::new();
        p.toggle_collapsed(5);
        assert!(p.collapsed.contains(&5));
        p.toggle_collapsed(5);
        assert!(!p.collapsed.contains(&5));
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_returns_none() {
        let s = TabStrip::new();
        let p = TreeTabsPanel::new();
        assert_eq!(hit_test(&s, &p, PANEL_WIDTH + 1.0, 50.0, TAB_H, WIN_H), None);
    }

    #[test]
    fn hit_above_tab_bar_returns_none() {
        let s = TabStrip::new();
        let p = TreeTabsPanel::new();
        assert_eq!(hit_test(&s, &p, 10.0, TAB_H - 1.0, TAB_H, WIN_H), None);
    }

    #[test]
    fn hit_row_body_returns_tab() {
        let s = TabStrip::new();
        let p = TreeTabsPanel::new();
        let hit = hit_test(&s, &p, 80.0, TAB_H + ROW_H * 0.5, TAB_H, WIN_H);
        assert_eq!(hit, Some(TreeTabHit::Tab(0)));
    }

    #[test]
    fn hit_close_button() {
        let s = TabStrip::new();
        let p = TreeTabsPanel::new();
        let cx = PANEL_WIDTH - CLOSE_RIGHT_MARGIN - CLOSE_W + 2.0;
        let hit = hit_test(&s, &p, cx, TAB_H + ROW_H * 0.5, TAB_H, WIN_H);
        assert_eq!(hit, Some(TreeTabHit::Close(0)));
    }

    #[test]
    fn hit_arrow_on_parent_tab() {
        let s = strip_with_child();
        let p = TreeTabsPanel::new();
        // Arrow at depth=0: x in [ARROW_LEFT_BASE, ARROW_LEFT_BASE + ARROW_W)
        let ax = ARROW_LEFT_BASE + ARROW_W * 0.5;
        let hit = hit_test(&s, &p, ax, TAB_H + ROW_H * 0.5, TAB_H, WIN_H);
        assert_eq!(hit, Some(TreeTabHit::Arrow(0))); // id of root tab = 0
    }

    #[test]
    fn hit_arrow_does_not_apply_to_leaf() {
        let s = TabStrip::new(); // single tab, no children
        let p = TreeTabsPanel::new();
        let ax = ARROW_LEFT_BASE + ARROW_W * 0.5;
        let hit = hit_test(&s, &p, ax, TAB_H + ROW_H * 0.5, TAB_H, WIN_H);
        // No arrow on leaf — should return Tab, not Arrow
        assert_eq!(hit, Some(TreeTabHit::Tab(0)));
    }

    #[test]
    fn collapsed_subtree_children_not_in_hit_test() {
        let s = strip_with_child();
        let mut p = TreeTabsPanel::new();
        p.toggle_collapsed(0); // collapse root → child row disappears
        // Row 0 = root (visible), row 1 = should not exist
        let below_y = TAB_H + ROW_H + ROW_H * 0.5;
        let hit = hit_test(&s, &p, 50.0, below_y, TAB_H, WIN_H);
        assert_eq!(hit, Some(TreeTabHit::Empty));
    }

    // ── Rendering ────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_emits_commands() {
        let s = TabStrip::new();
        let p = TreeTabsPanel::new();
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        assert!(!dl.is_empty());
    }

    #[test]
    fn build_panel_has_title_text() {
        let s = TabStrip::new();
        let p = TreeTabsPanel::new();
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("вкладка"))
        });
        assert!(has_title);
    }

    #[test]
    fn build_panel_shows_arrow_for_parent() {
        let s = strip_with_child();
        let p = TreeTabsPanel::new();
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        let has_arrow = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "▼" || text == "▶")
        });
        assert!(has_arrow, "parent tab must render collapse/expand arrow");
    }

    #[test]
    fn build_panel_no_arrow_for_leaf() {
        let s = TabStrip::new();
        let p = TreeTabsPanel::new();
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        let has_arrow = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "▼" || text == "▶")
        });
        assert!(!has_arrow, "leaf tab must not render arrow");
    }

    #[test]
    fn build_panel_collapsed_shows_right_arrow() {
        let s = strip_with_child();
        let mut p = TreeTabsPanel::new();
        p.toggle_collapsed(0);
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        let has_right_arrow = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "▶")
        });
        assert!(has_right_arrow, "collapsed parent must show ▶ arrow");
    }

    #[test]
    fn build_panel_collapsed_hides_child_rows() {
        let s = strip_with_child();
        let mut p = TreeTabsPanel::new();
        p.toggle_collapsed(0);
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        // Only 1 tab visible; title count = 1.
        let title_count = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("вкладка"))
        }).count();
        assert_eq!(title_count, 1, "collapsed subtree must hide child tab titles");
    }

    #[test]
    fn build_panel_grandchild_has_deeper_indent() {
        // Grandchild row should appear after parent + child rows.
        // We verify by checking that we get 3 title rows.
        let s = strip_with_grandchild();
        let p = TreeTabsPanel::new();
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        let title_count = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("вкладка"))
        }).count();
        assert_eq!(title_count, 3, "parent + child + grandchild = 3 title rows");
    }

    #[test]
    fn build_panel_badge_for_background_old() {
        let mut s = TabStrip::new();
        s.push_blank();
        s.set_tab_state(0, TabState::BackgroundOld);
        let p = TreeTabsPanel::new();
        let dl = build_panel(&s, &p, TAB_H, WIN_H);
        let has_amber = dl.iter().any(|c| match c {
            DisplayCommand::FillRoundedRect { color, .. } => {
                color.r == BADGE_OLD.r && color.g == BADGE_OLD.g
            }
            _ => false,
        });
        assert!(has_amber, "BackgroundOld tab must render amber badge");
    }
}
