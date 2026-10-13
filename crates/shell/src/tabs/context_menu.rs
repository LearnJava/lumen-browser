//! Tab context menu: right-click menu over the tab strip.
//!
//! `TabContextMenu` holds the open/closed state, the index of the tab the menu
//! acts on, and the raw cursor anchor in CSS px. The menu itself is drawn by
//! the engine chrome (`#tabCtxMenu`, UX-CHROME-PANELS); this module owns the
//! state, the clamped anchor and the row-attribute → [`MenuAction`] mapping.
//!
//! Menu items (13): Duplicate / Pin·Unpin / Move to new window / Add to new
//! group / Collapse·Expand group / Remove from group / Close others / Close
//! tabs to the right / Reopen closed tab / Open in another browser (four
//! reasons, COMPAT-HANDOFF). The actual mutations are performed by the shell.

use lumen_chrome::ChromeTabCtxModel;

use crate::external_browser::HandoffReason;

/// Menu width in CSS px (matches `.tab-ctx` in `chrome.html`).
const MENU_W: f32 = 210.0;
/// Row height in CSS px (`.tab-ctx-row`).
const ROW_H: f32 = 28.0;
/// Vertical padding above the first / below the last row, plus the four
/// 1-px separators and the 1-px border on each side.
const CHROME_H: f32 = 10.0 + 4.0 + 2.0;

/// Total menu height in CSS px (background box).
fn menu_height() -> f32 {
    ROW_H * 13.0 + CHROME_H
}

// ── Types ─────────────────────────────────────────────────────────────────────

/// An action the user can pick from the tab context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Reopen the most recently closed tab (Ctrl+Shift+T).
    ReopenClosed,
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
    /// Hand the target tab's page to another installed browser, picked by the
    /// reason (COMPAT-HANDOFF). One-shot: no site rule is created.
    OpenInOtherBrowser(HandoffReason),
}

impl MenuAction {
    /// Maps a `data-tab-ctx` attribute value of `#tabCtxMenu`'s rows.
    pub fn from_attr(value: &str) -> Option<Self> {
        Some(match value {
            "duplicate" => Self::Duplicate,
            "pin" => Self::TogglePin,
            "new-window" => Self::MoveToNewWindow,
            "new-group" => Self::AddToNewGroup,
            "toggle-group" => Self::ToggleGroupCollapse,
            "ungroup" => Self::RemoveFromGroup,
            "close-others" => Self::CloseOthers,
            "close-right" => Self::CloseRight,
            "reopen" => Self::ReopenClosed,
            "other-browser" => Self::OpenInOtherBrowser(HandoffReason::Other),
            "other-browser-crypto" => Self::OpenInOtherBrowser(HandoffReason::Crypto),
            "other-browser-drm" => Self::OpenInOtherBrowser(HandoffReason::Drm),
            "other-browser-calls" => Self::OpenInOtherBrowser(HandoffReason::Calls),
            _ => return None,
        })
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
    }

    /// Hide the menu.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// `true` while the menu is visible.
    pub fn is_open(&self) -> bool {
        self.open
    }
}

impl TabContextMenu {
    /// Snapshot for `#tabCtxMenu`: the anchor clamped so the menu stays
    /// inside a `window_w` × `window_h` window.
    pub fn chrome_model(&self, window_w: f32, window_h: f32) -> ChromeTabCtxModel {
        let x = self.anchor_x.min((window_w - MENU_W).max(0.0)).max(0.0);
        let y = self.anchor_y.min((window_h - menu_height()).max(0.0)).max(0.0);
        ChromeTabCtxModel {
            open: self.open,
            x,
            y,
            pinned: self.target_pinned,
            collapsed: self.target_collapsed,
        }
    }
}

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
        assert!(!m.chrome_model(1024.0, 720.0).open);
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
    }

    #[test]
    fn row_attrs_map_to_actions() {
        assert_eq!(MenuAction::from_attr("duplicate"), Some(MenuAction::Duplicate));
        assert_eq!(MenuAction::from_attr("pin"), Some(MenuAction::TogglePin));
        assert_eq!(MenuAction::from_attr("reopen"), Some(MenuAction::ReopenClosed));
        assert_eq!(MenuAction::from_attr("nope"), None);
    }

    #[test]
    fn other_browser_rows_carry_their_reason() {
        for (attr, reason) in [
            ("other-browser", HandoffReason::Other),
            ("other-browser-crypto", HandoffReason::Crypto),
            ("other-browser-drm", HandoffReason::Drm),
            ("other-browser-calls", HandoffReason::Calls),
        ] {
            assert_eq!(MenuAction::from_attr(attr), Some(MenuAction::OpenInOtherBrowser(reason)));
        }
    }

    #[test]
    fn model_carries_pin_and_collapse_state() {
        let mut m = TabContextMenu::default();
        m.open_for(0, true, true, true, 10.0, 10.0);
        let c = m.chrome_model(1024.0, 720.0);
        assert!(c.pinned && c.collapsed && c.open);
    }

    #[test]
    fn menu_clamps_to_window_right_edge() {
        let mut m = TabContextMenu::default();
        m.open_for(0, false, false, false, 1000.0, 10.0);
        let c = m.chrome_model(1024.0, 720.0);
        assert!(c.x + MENU_W <= 1024.0, "menu overflows right edge: x={}", c.x);
    }

    #[test]
    fn menu_clamps_to_window_bottom_edge() {
        let mut m = TabContextMenu::default();
        m.open_for(0, false, false, false, 10.0, 715.0);
        let c = m.chrome_model(1024.0, 720.0);
        assert!(c.y + menu_height() <= 720.0, "menu overflows bottom edge: y={}", c.y);
    }
}
