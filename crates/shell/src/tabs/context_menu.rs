//! Tab context menu: right-click menu over the tab strip.
//!
//! `TabContextMenu` holds the open/closed state, the index of the tab the menu
//! acts on, and the raw cursor anchor in CSS px. `build_overlay` produces a
//! viewport-locked `DisplayList`; `item_at` maps a CSS-px `(x, y)` to the menu
//! row under it (used both for click dispatch and hover highlight).
//!
//! Menu items (8): Duplicate / Pin·Unpin / Move to new window / Add to new
//! group / Collapse·Expand group / Remove from group / Close others / Close
//! tabs to the right. The actual mutations are performed by the shell — this
//! module only describes geometry, rendering, and hit-testing.
//!
//! Visual constants follow the dark-chrome aesthetic of `strip.rs`.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants ────────────────────────────────────────────────────────

/// Menu width in CSS px.
pub const MENU_W: f32 = 210.0;
/// Height of one menu row in CSS px.
const ROW_H: f32 = 28.0;
/// Vertical padding above the first / below the last row.
const PAD_Y: f32 = 5.0;
/// Left text inset inside a row.
const TEXT_PAD_X: f32 = 14.0;
/// Row text font size.
const FONT_SZ: f32 = 13.0;
/// Corner radius of the menu background.
const RADIUS: f32 = 6.0;

const MENU_BG: Color = Color { r: 38, g: 39, b: 44, a: 250 };
const MENU_BORDER: Color = Color { r: 70, g: 71, b: 78, a: 255 };
const ROW_HOVER_BG: Color = Color { r: 58, g: 78, b: 120, a: 255 };
const ITEM_TEXT: Color = Color { r: 222, g: 222, b: 230, a: 255 };
const DIVIDER: Color = Color { r: 60, g: 61, b: 68, a: 255 };

/// Total menu height in CSS px (background box).
pub fn menu_height() -> f32 {
    PAD_Y * 2.0 + ROW_H * ITEMS.len() as f32
}

// ── Types ─────────────────────────────────────────────────────────────────────

/// An action the user can pick from the tab context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Duplicate the target tab (open a copy of its page right after it).
    Duplicate,
    /// Toggle the pinned flag of the target tab.
    TogglePin,
    /// Move the target tab into a new OS window (Phase 0: new process + close).
    MoveToNewWindow,
    /// Put the target tab into a new tab group (CC-6).
    AddToNewGroup,
    /// Toggle the collapsed state of the target tab's group (CC-6). No-op when
    /// the target is ungrouped.
    ToggleGroupCollapse,
    /// Remove the target tab from its group (CC-6). No-op when ungrouped.
    RemoveFromGroup,
    /// Close every other tab, keeping only the target (pinned tabs survive).
    CloseOthers,
    /// Close all tabs positioned to the right of the target (pinned tabs survive).
    CloseRight,
}

/// Fixed top-to-bottom order of menu rows.
const ITEMS: [MenuAction; 8] = [
    MenuAction::Duplicate,
    MenuAction::TogglePin,
    MenuAction::MoveToNewWindow,
    MenuAction::AddToNewGroup,
    MenuAction::ToggleGroupCollapse,
    MenuAction::RemoveFromGroup,
    MenuAction::CloseOthers,
    MenuAction::CloseRight,
];

/// Russian label for a row. `target_pinned` toggles the Pin/Unpin wording;
/// `target_collapsed` toggles the Collapse/Expand wording.
fn label(action: MenuAction, target_pinned: bool, target_collapsed: bool) -> &'static str {
    match action {
        MenuAction::Duplicate => "Дублировать",
        MenuAction::TogglePin => {
            if target_pinned {
                "Открепить"
            } else {
                "Закрепить"
            }
        }
        MenuAction::MoveToNewWindow => "В новое окно",
        MenuAction::AddToNewGroup => "В новую группу",
        MenuAction::ToggleGroupCollapse => {
            if target_collapsed {
                "Развернуть группу"
            } else {
                "Свернуть группу"
            }
        }
        MenuAction::RemoveFromGroup => "Убрать из группы",
        MenuAction::CloseOthers => "Закрыть другие",
        MenuAction::CloseRight => "Закрыть справа",
    }
}

/// State of the right-click tab context menu.
///
/// A single menu is open at a time. `open == false` means it is hidden and
/// `build_overlay` / `item_at` return nothing.
pub struct TabContextMenu {
    /// Whether the menu is currently visible.
    pub open: bool,
    /// Strip index of the tab the menu acts on.
    pub target_idx: usize,
    /// Whether the target tab is pinned (drives the Pin/Unpin label).
    pub target_pinned: bool,
    /// Whether the target tab belongs to a group (CC-6; drives whether the
    /// group rows are actionable).
    pub target_grouped: bool,
    /// Whether the target tab's group is collapsed (CC-6; drives the
    /// Collapse/Expand label).
    pub target_collapsed: bool,
    /// Raw cursor X where the menu was summoned, CSS px (pre-clamp).
    pub anchor_x: f32,
    /// Raw cursor Y where the menu was summoned, CSS px (pre-clamp).
    pub anchor_y: f32,
    /// Row index currently under the cursor, for hover highlight.
    pub hovered: Option<usize>,
}

impl Default for TabContextMenu {
    fn default() -> Self {
        Self {
            open: false,
            target_idx: 0,
            target_pinned: false,
            target_grouped: false,
            target_collapsed: false,
            anchor_x: 0.0,
            anchor_y: 0.0,
            hovered: None,
        }
    }
}

impl TabContextMenu {
    /// Open the menu for tab `idx` at cursor `(x, y)`. `pinned` is the target
    /// tab's current pinned state (Pin/Unpin label); `grouped`/`collapsed`
    /// describe its tab-group state (CC-6; Collapse/Expand label).
    pub fn open_for(
        &mut self,
        idx: usize,
        pinned: bool,
        grouped: bool,
        collapsed: bool,
        x: f32,
        y: f32,
    ) {
        self.open = true;
        self.target_idx = idx;
        self.target_pinned = pinned;
        self.target_grouped = grouped;
        self.target_collapsed = collapsed;
        self.anchor_x = x;
        self.anchor_y = y;
        self.hovered = None;
    }

    /// Hide the menu.
    pub fn close(&mut self) {
        self.open = false;
        self.hovered = None;
    }

    /// `true` while the menu is visible.
    pub fn is_open(&self) -> bool {
        self.open
    }
}

// ── Geometry ──────────────────────────────────────────────────────────────────

/// Compute the clamped top-left anchor so the menu stays inside the window.
fn anchor(menu: &TabContextMenu, window_w: f32, window_h: f32) -> (f32, f32) {
    let h = menu_height();
    let x = menu.anchor_x.min((window_w - MENU_W).max(0.0)).max(0.0);
    let y = menu.anchor_y.min((window_h - h).max(0.0)).max(0.0);
    (x, y)
}

/// Map a CSS-px `(x, y)` to the menu row index under it, or `None` if the
/// point is outside the menu box. Window dimensions are needed to reproduce
/// the same clamping used by `build_overlay`.
pub fn item_at(menu: &TabContextMenu, x: f32, y: f32, window_w: f32, window_h: f32) -> Option<usize> {
    if !menu.open {
        return None;
    }
    let (x0, y0) = anchor(menu, window_w, window_h);
    let h = menu_height();
    if x < x0 || x >= x0 + MENU_W || y < y0 || y >= y0 + h {
        return None;
    }
    let row_top = y0 + PAD_Y;
    if y < row_top {
        return None;
    }
    let idx = ((y - row_top) / ROW_H) as usize;
    if idx < ITEMS.len() { Some(idx) } else { None }
}

/// Map a CSS-px `(x, y)` to the [`MenuAction`] under it, or `None`.
pub fn action_at(menu: &TabContextMenu, x: f32, y: f32, window_w: f32, window_h: f32) -> Option<MenuAction> {
    item_at(menu, x, y, window_w, window_h).map(|i| ITEMS[i])
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build a viewport-locked display list for the open menu.
///
/// Returns an empty list when the menu is closed. The menu is clamped to stay
/// fully inside `window_w` × `window_h`.
pub fn build_overlay(menu: &TabContextMenu, window_w: f32, window_h: f32) -> DisplayList {
    if !menu.open {
        return DisplayList::new();
    }
    let (x0, y0) = anchor(menu, window_w, window_h);
    let h = menu_height();
    let mut out = DisplayList::with_capacity(ITEMS.len() * 2 + 2);

    // Border (drawn 1 px larger behind the background fill).
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(x0 - 1.0, y0 - 1.0, MENU_W + 2.0, h + 2.0),
        radii: corners(RADIUS),
        color: MENU_BORDER,
    });
    // Background.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(x0, y0, MENU_W, h),
        radii: corners(RADIUS),
        color: MENU_BG,
    });

    for (i, &action) in ITEMS.iter().enumerate() {
        let row_y = y0 + PAD_Y + i as f32 * ROW_H;

        // Hover highlight.
        if menu.hovered == Some(i) {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x0 + 2.0, row_y, MENU_W - 4.0, ROW_H),
                color: ROW_HOVER_BG,
            });
        }

        // Dividers: above the group rows and above the destructive "close" rows.
        if action == MenuAction::AddToNewGroup || action == MenuAction::CloseOthers {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x0 + 8.0, row_y - 1.0, MENU_W - 16.0, 1.0),
                color: DIVIDER,
            });
        }

        out.push(DisplayCommand::DrawText {
            rect: Rect::new(
                x0 + TEXT_PAD_X,
                row_y + (ROW_H - FONT_SZ * 1.3) * 0.5,
                MENU_W - TEXT_PAD_X * 2.0,
                FONT_SZ * 1.3,
            ),
            text: label(action, menu.target_pinned, menu.target_collapsed).to_owned(),
            font_size: FONT_SZ,
            color: ITEM_TEXT,
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

/// Uniform corner radii helper.
fn corners(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn open_menu() -> TabContextMenu {
        let mut m = TabContextMenu::default();
        m.open_for(2, false, false, false, 100.0, 50.0);
        m
    }

    #[test]
    fn default_is_closed() {
        let m = TabContextMenu::default();
        assert!(!m.is_open());
        assert!(build_overlay(&m, 1024.0, 720.0).is_empty());
    }

    #[test]
    fn open_for_sets_target_and_opens() {
        let m = open_menu();
        assert!(m.is_open());
        assert_eq!(m.target_idx, 2);
        assert!(!m.target_pinned);
    }

    #[test]
    fn close_hides_menu() {
        let mut m = open_menu();
        m.close();
        assert!(!m.is_open());
        assert_eq!(m.hovered, None);
    }

    #[test]
    fn build_overlay_emits_all_rows() {
        let m = open_menu();
        let dl = build_overlay(&m, 1024.0, 720.0);
        let text_rows = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .count();
        assert_eq!(text_rows, ITEMS.len());
    }

    #[test]
    fn pin_label_flips_with_state() {
        assert_eq!(label(MenuAction::TogglePin, false, false), "Закрепить");
        assert_eq!(label(MenuAction::TogglePin, true, false), "Открепить");
    }

    #[test]
    fn group_collapse_label_flips_with_state() {
        assert_eq!(label(MenuAction::ToggleGroupCollapse, false, false), "Свернуть группу");
        assert_eq!(label(MenuAction::ToggleGroupCollapse, false, true), "Развернуть группу");
    }

    #[test]
    fn group_rows_present_in_overlay() {
        let m = open_menu();
        let dl = build_overlay(&m, 1024.0, 720.0);
        let has = |needle: &str| {
            dl.iter().any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == needle))
        };
        assert!(has("В новую группу"));
        assert!(has("Свернуть группу"));
        assert!(has("Убрать из группы"));
    }

    #[test]
    fn build_overlay_uses_unpin_when_pinned() {
        let mut m = TabContextMenu::default();
        m.open_for(0, true, false, false, 10.0, 10.0);
        let dl = build_overlay(&m, 1024.0, 720.0);
        let has_unpin = dl.iter().any(|c| match c {
            DisplayCommand::DrawText { text, .. } => text == "Открепить",
            _ => false,
        });
        assert!(has_unpin);
    }

    #[test]
    fn item_at_first_row() {
        let m = open_menu();
        // First row starts at anchor_y + PAD_Y; sample just inside it.
        let idx = item_at(&m, 110.0, 50.0 + PAD_Y + 2.0, 1024.0, 720.0);
        assert_eq!(idx, Some(0));
    }

    #[test]
    fn item_at_last_row() {
        let m = open_menu();
        let last = ITEMS.len() - 1;
        let y = 50.0 + PAD_Y + last as f32 * ROW_H + 2.0;
        assert_eq!(item_at(&m, 110.0, y, 1024.0, 720.0), Some(last));
    }

    #[test]
    fn item_at_outside_returns_none() {
        let m = open_menu();
        // Far below the menu.
        assert_eq!(item_at(&m, 110.0, 50.0 + menu_height() + 20.0, 1024.0, 720.0), None);
        // Left of the menu.
        assert_eq!(item_at(&m, 10.0, 60.0, 1024.0, 720.0), None);
    }

    #[test]
    fn action_at_maps_rows_to_actions() {
        let m = open_menu();
        let y0 = 50.0 + PAD_Y;
        assert_eq!(action_at(&m, 110.0, y0 + 2.0, 1024.0, 720.0), Some(MenuAction::Duplicate));
        assert_eq!(
            action_at(&m, 110.0, y0 + ROW_H + 2.0, 1024.0, 720.0),
            Some(MenuAction::TogglePin)
        );
        assert_eq!(
            action_at(&m, 110.0, y0 + 2.0 * ROW_H + 2.0, 1024.0, 720.0),
            Some(MenuAction::MoveToNewWindow)
        );
    }

    #[test]
    fn menu_clamps_to_window_right_edge() {
        let mut m = TabContextMenu::default();
        // Anchor near the right edge — menu must shift left to stay visible.
        m.open_for(0, false, false, false, 1000.0, 10.0);
        let (x0, _) = anchor(&m, 1024.0, 720.0);
        assert!(x0 + MENU_W <= 1024.0, "menu overflows right edge: x0={x0}");
    }

    #[test]
    fn menu_clamps_to_window_bottom_edge() {
        let mut m = TabContextMenu::default();
        m.open_for(0, false, false, false, 10.0, 715.0);
        let (_, y0) = anchor(&m, 1024.0, 720.0);
        assert!(y0 + menu_height() <= 720.0, "menu overflows bottom edge: y0={y0}");
    }

    #[test]
    fn closed_menu_item_at_is_none() {
        let m = TabContextMenu::default();
        assert_eq!(item_at(&m, 100.0, 50.0, 1024.0, 720.0), None);
    }
}
