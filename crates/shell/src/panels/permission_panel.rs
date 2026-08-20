//! Per-site permission popover (7C.2): floating panel anchored below the tab
//! bar on the left side of the window (where a lock icon would sit).
//!
//! Models the allow/deny/ask state for four browser permissions — Camera,
//! Microphone, Notifications, Clipboard — for the current page origin. State is
//! in-memory only (no cross-session persistence); a `StorageBackend` hook-up is
//! a future task.
//!
//! The legacy display-list renderer was removed in CC-15-4 — under the engine
//! chrome the rows live in `#permPopover`. The frozen design gave it only two
//! (Camera, Microphone), which left Notifications/Clipboard unreachable from
//! the UI until `BUG-411` added the missing two rows to the reference; all four
//! of `PermissionKind::ALL` are now bound, in that order. `hit_test` is kept
//! (still called ungated, a `BUG-404` site).
//!
//! Toggled with `Ctrl+Shift+P`.

use std::collections::HashMap;

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the floating permission panel in CSS px.
pub const PANEL_W: f32 = 240.0;
/// Height of the floating permission panel in CSS px.
///
/// Includes the header, the four permission rows, and the reserved
/// [`FINE_PRINT_H`] block at the bottom for the session-only disclaimer.
pub const PANEL_H: f32 = HEADER_H + 4.0 * ROW_H + FINE_PRINT_H;
/// Top offset from the tab-bar bottom edge (CSS px).
const PANEL_TOP_OFFSET: f32 = 4.0;
/// Left margin from the window edge (CSS px).
const PANEL_LEFT_MARGIN: f32 = 8.0;
/// Height of the header row (origin + close button).
const HEADER_H: f32 = 28.0;
/// Height of each permission row.
const ROW_H: f32 = 30.0;
/// Horizontal padding inside the panel.
const PAD_X: f32 = 10.0;
/// Reserved height at the bottom of the panel for the session-only fine
/// print (divider + up to three wrapped text lines + padding). Sized for
/// the current disclaimer wording; grows automatically if the string is
/// ever edited, since [`wrap_text`] output is capped at 3 lines here.
const FINE_PRINT_H: f32 = 56.0;

const BTN_W: f32 = 54.0;

// ── Permission types ──────────────────────────────────────────────────────────

/// A single browser permission kind tracked by the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionKind {
    /// Camera / video capture.
    Camera,
    /// Microphone / audio capture.
    Microphone,
    /// Desktop notifications (`Notification.requestPermission()`).
    Notifications,
    /// Clipboard read/write access.
    Clipboard,
}

impl PermissionKind {
    /// All four permission kinds in display order.
    pub const ALL: [PermissionKind; 4] = [
        PermissionKind::Camera,
        PermissionKind::Microphone,
        PermissionKind::Notifications,
        PermissionKind::Clipboard,
    ];
}

/// Grant state for a single permission on a single origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionState {
    /// The page may use this capability without a user prompt.
    Allow,
    /// The capability is blocked; no prompt is shown.
    Deny,
    /// Default: the browser will prompt the user when the capability is first
    /// requested.
    #[default]
    Ask,
}

impl PermissionState {
    /// Cycle to the next state: Ask → Allow → Deny → Ask.
    pub fn cycle(self) -> Self {
        match self {
            PermissionState::Ask => PermissionState::Allow,
            PermissionState::Allow => PermissionState::Deny,
            PermissionState::Deny => PermissionState::Ask,
        }
    }
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Per-site permission popover state (7C.2).
pub struct PermissionPanel {
    /// `true` while the floating panel is visible.  Toggled via Ctrl+Shift+P.
    pub visible: bool,
    /// Origin of the currently loaded page (e.g. `"https://example.com"`).
    ///
    /// `None` while no page is loaded or for `file:` URLs.
    pub current_origin: Option<String>,
    /// Stored permission grants keyed by `(origin, kind)`.
    ///
    /// Defaults to [`PermissionState::Ask`] when the pair is absent.
    pub permissions: HashMap<(String, PermissionKind), PermissionState>,
}

impl PermissionPanel {
    /// Create a new hidden panel with no stored permissions.
    pub fn new() -> Self {
        Self {
            visible: false,
            current_origin: None,
            permissions: HashMap::new(),
        }
    }

    /// Flip panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Update the current origin on navigation (does not clear stored grants).
    pub fn set_origin(&mut self, origin: Option<String>) {
        self.current_origin = origin;
    }

    /// Return the stored state for `kind` at the current origin.
    ///
    /// Returns [`PermissionState::Ask`] when no grant has been recorded.
    pub fn state_for(&self, kind: PermissionKind) -> PermissionState {
        let Some(ref origin) = self.current_origin else {
            return PermissionState::Ask;
        };
        self.permissions
            .get(&(origin.clone(), kind))
            .copied()
            .unwrap_or_default()
    }

    /// Cycle the state for `kind` at the current origin to the next value.
    ///
    /// Does nothing if `current_origin` is `None`.
    pub fn cycle_permission(&mut self, kind: PermissionKind) {
        let Some(ref origin) = self.current_origin.clone() else {
            return;
        };
        let current = self
            .permissions
            .get(&(origin.clone(), kind))
            .copied()
            .unwrap_or_default();
        self.permissions.insert((origin.clone(), kind), current.cycle());
    }

    /// Set the state for `kind` at the current origin directly (CC-9's
    /// engine-rendered popover has two distinct allow/deny buttons — unlike
    /// the legacy panel's single [`Self::cycle_permission`] toggle button,
    /// there's no "ask" control to cycle back to, so this sets the state a
    /// click actually asked for instead of advancing a cycle).
    ///
    /// Does nothing if `current_origin` is `None`.
    pub fn set_permission(&mut self, kind: PermissionKind, state: PermissionState) {
        let Some(ref origin) = self.current_origin.clone() else {
            return;
        };
        self.permissions.insert((origin.clone(), kind), state);
    }
}

impl Default for PermissionPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the permission panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionHit {
    /// User clicked the toggle button for the given permission kind.
    Toggle(PermissionKind),
    /// User closed the panel (clicked the "×").
    Close,
    /// Clicked inside the panel but on a non-interactive area.
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the permission panel.
///
/// Returns `None` when the click is outside the panel.
/// `tab_bar_h` is the height of the tab bar (panel is anchored below it).
pub fn hit_test(
    _panel: &PermissionPanel,
    x: f32,
    y: f32,
    tab_bar_h: f32,
) -> Option<PermissionHit> {
    let (px, py) = panel_origin(tab_bar_h);
    if x < px || x >= px + PANEL_W || y < py || y >= py + PANEL_H {
        return None;
    }

    let rel_x = x - px;
    let rel_y = y - py;

    // Close button: top-right 20×20 area of the header.
    if rel_x >= PANEL_W - 20.0 && rel_y < HEADER_H {
        return Some(PermissionHit::Close);
    }

    // Permission rows — each is ROW_H tall starting at HEADER_H.
    for (i, kind) in PermissionKind::ALL.iter().enumerate() {
        let row_top = HEADER_H + i as f32 * ROW_H;
        let row_bot = row_top + ROW_H;
        if rel_y >= row_top && rel_y < row_bot {
            // Toggle button: right side of the row.
            let btn_x = PANEL_W - PAD_X - BTN_W;
            if rel_x >= btn_x && rel_x < btn_x + BTN_W {
                return Some(PermissionHit::Toggle(*kind));
            }
            return Some(PermissionHit::Empty);
        }
    }

    Some(PermissionHit::Empty)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Top-left corner of the permission panel in CSS px.
fn panel_origin(tab_bar_h: f32) -> (f32, f32) {
    (PANEL_LEFT_MARGIN, tab_bar_h + PANEL_TOP_OFFSET)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel(origin: Option<&str>) -> PermissionPanel {
        let mut p = PermissionPanel::new();
        p.visible = true;
        p.current_origin = origin.map(|s| s.to_owned());
        p
    }

    const TAB_H: f32 = 36.0;

    // ── PermissionState ──────────────────────────────────────────────────────

    #[test]
    fn state_cycle_ask_to_allow() {
        assert_eq!(PermissionState::Ask.cycle(), PermissionState::Allow);
    }

    #[test]
    fn state_cycle_allow_to_deny() {
        assert_eq!(PermissionState::Allow.cycle(), PermissionState::Deny);
    }

    #[test]
    fn state_cycle_deny_to_ask() {
        assert_eq!(PermissionState::Deny.cycle(), PermissionState::Ask);
    }

    // ── PermissionPanel ──────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden() {
        let p = PermissionPanel::new();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_shows_panel() {
        let mut p = PermissionPanel::new();
        p.toggle();
        assert!(p.visible);
    }

    #[test]
    fn double_toggle_hides() {
        let mut p = PermissionPanel::new();
        p.toggle();
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn default_state_is_ask() {
        let p = make_panel(Some("https://example.com"));
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Ask);
    }

    #[test]
    fn cycle_permission_advances_state() {
        let mut p = make_panel(Some("https://example.com"));
        p.cycle_permission(PermissionKind::Camera);
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Allow);
        p.cycle_permission(PermissionKind::Camera);
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Deny);
        p.cycle_permission(PermissionKind::Camera);
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Ask);
    }

    #[test]
    fn cycle_without_origin_is_noop() {
        let mut p = make_panel(None);
        p.cycle_permission(PermissionKind::Microphone);
        // Still Ask (no origin → no entry stored).
        assert_eq!(p.state_for(PermissionKind::Microphone), PermissionState::Ask);
    }

    #[test]
    fn permissions_are_per_kind() {
        let mut p = make_panel(Some("https://example.com"));
        p.cycle_permission(PermissionKind::Camera);
        // Microphone should still be Ask.
        assert_eq!(p.state_for(PermissionKind::Microphone), PermissionState::Ask);
    }

    #[test]
    fn set_origin_does_not_clear_stored_grants() {
        let mut p = make_panel(Some("https://example.com"));
        p.cycle_permission(PermissionKind::Notifications);
        p.set_origin(Some("https://other.com".to_owned()));
        p.set_origin(Some("https://example.com".to_owned()));
        assert_eq!(
            p.state_for(PermissionKind::Notifications),
            PermissionState::Allow
        );
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_panel_returns_none() {
        let p = make_panel(Some("https://example.com"));
        // Far top-left outside panel.
        assert_eq!(hit_test(&p, 500.0, TAB_H + 2.0, TAB_H), None);
    }

    #[test]
    fn hit_close_button() {
        let p = make_panel(Some("https://example.com"));
        let (px, py) = panel_origin(TAB_H);
        let hit = hit_test(&p, px + PANEL_W - 5.0, py + 5.0, TAB_H);
        assert_eq!(hit, Some(PermissionHit::Close));
    }

    #[test]
    fn hit_first_toggle_button() {
        let p = make_panel(Some("https://example.com"));
        let (px, py) = panel_origin(TAB_H);
        let btn_x = px + PANEL_W - PAD_X - BTN_W + BTN_W / 2.0;
        let btn_y = py + HEADER_H + ROW_H / 2.0;
        let hit = hit_test(&p, btn_x, btn_y, TAB_H);
        assert_eq!(hit, Some(PermissionHit::Toggle(PermissionKind::Camera)));
    }

    #[test]
    fn hit_second_toggle_button() {
        let p = make_panel(Some("https://example.com"));
        let (px, py) = panel_origin(TAB_H);
        let btn_x = px + PANEL_W - PAD_X - BTN_W + BTN_W / 2.0;
        let btn_y = py + HEADER_H + ROW_H + ROW_H / 2.0;
        let hit = hit_test(&p, btn_x, btn_y, TAB_H);
        assert_eq!(hit, Some(PermissionHit::Toggle(PermissionKind::Microphone)));
    }

    #[test]
    fn hit_row_label_returns_empty() {
        let p = make_panel(Some("https://example.com"));
        let (px, py) = panel_origin(TAB_H);
        // Click the label area (left side of row), not the button.
        let hit = hit_test(&p, px + 30.0, py + HEADER_H + 15.0, TAB_H);
        assert_eq!(hit, Some(PermissionHit::Empty));
    }

}
