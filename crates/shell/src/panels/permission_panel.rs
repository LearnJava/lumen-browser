//! Per-site permission popover (7C.2): floating panel anchored below the tab
//! bar on the left side of the window (where a lock icon would sit).
//!
//! Shows the allow/deny/ask state for four browser permissions —
//! Camera, Microphone, Notifications, Clipboard — for the current page origin.
//! Each row has a toggle button that cycles the state.  The panel does not
//! persist state across sessions (in-memory only); a `StorageBackend` hook-up
//! is a future task.
//!
//! Toggled with `Ctrl+Shift+P`.

use std::collections::HashMap;

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the floating permission panel in CSS px.
pub const PANEL_W: f32 = 240.0;
/// Height of the floating permission panel in CSS px.
pub const PANEL_H: f32 = 164.0;
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

const BG: Color = Color { r: 20, g: 20, b: 28, a: 245 };
const BORDER: Color = Color { r: 50, g: 50, b: 65, a: 255 };
const TEXT_MAIN: Color = Color { r: 220, g: 220, b: 228, a: 255 };
const TEXT_DIM: Color = Color { r: 130, g: 130, b: 145, a: 255 };
const ALLOW_FG: Color = Color { r: 60, g: 200, b: 120, a: 255 };
const DENY_FG: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const ASK_FG: Color = Color { r: 160, g: 140, b: 60, a: 255 };
const ALLOW_BG: Color = Color { r: 25, g: 70, b: 45, a: 255 };
const DENY_BG: Color = Color { r: 80, g: 30, b: 30, a: 255 };
const ASK_BG: Color = Color { r: 65, g: 58, b: 22, a: 255 };
const CLOSE_FG: Color = Color { r: 140, g: 80, b: 80, a: 255 };

const FONT_SZ: f32 = 11.0;
const FONT_SZ_SM: f32 = 10.0;
const PANEL_RADIUS: f32 = 6.0;
const BTN_RADIUS: f32 = 4.0;
const BTN_W: f32 = 54.0;
const BTN_H: f32 = 18.0;

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

    /// Short display name for the permission row label.
    pub fn label(self) -> &'static str {
        match self {
            PermissionKind::Camera => "Camera",
            PermissionKind::Microphone => "Microphone",
            PermissionKind::Notifications => "Notifications",
            PermissionKind::Clipboard => "Clipboard",
        }
    }

    /// Emoji icon shown to the left of the label.
    pub fn icon(self) -> &'static str {
        match self {
            PermissionKind::Camera => "📷",
            PermissionKind::Microphone => "🎤",
            PermissionKind::Notifications => "🔔",
            PermissionKind::Clipboard => "📋",
        }
    }
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
    /// Label shown on the toggle button.
    pub fn label(self) -> &'static str {
        match self {
            PermissionState::Allow => "Allow",
            PermissionState::Deny => "Deny",
            PermissionState::Ask => "Ask",
        }
    }

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

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the permission floating panel.
///
/// The panel is anchored at the top-left of the window, offset by
/// `tab_bar_h` from the top.
pub fn build_panel(panel: &PermissionPanel, tab_bar_h: f32) -> DisplayList {
    let (px, py) = panel_origin(tab_bar_h);
    let mut out = DisplayList::with_capacity(30);
    let radii = uniform_radii(PANEL_RADIUS);

    // Background + border.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        radii,
        color: BORDER,
    });
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + 1.0, PANEL_W - 2.0, PANEL_H - 2.0),
        radii: uniform_radii(PANEL_RADIUS - 1.0),
        color: BG,
    });

    // Header: lock icon + origin label + close "×".
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + PAD_X, py + 6.0, 16.0, 16.0),
        text: "🔒".to_owned(),
        font_size: 13.0,
        color: TEXT_DIM,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });
    let origin_label = panel
        .current_origin
        .as_deref()
        .unwrap_or("(no origin)");
    let origin_text = truncate_label(origin_label, 24);
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + PAD_X + 18.0, py + 8.0, PANEL_W - 54.0, FONT_SZ * 1.3),
        text: origin_text,
        font_size: FONT_SZ,
        color: TEXT_MAIN,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // Close "×" button.
    let close_x = px + PANEL_W - 18.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(close_x, py + 6.0, 14.0, FONT_SZ * 1.2),
        text: "×".to_owned(),
        font_size: FONT_SZ,
        color: CLOSE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // Divider line between header and rows.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 1.0, py + HEADER_H - 1.0, PANEL_W - 2.0, 1.0),
        color: BORDER,
    });

    // Permission rows.
    for (i, &kind) in PermissionKind::ALL.iter().enumerate() {
        let row_top = py + HEADER_H + i as f32 * ROW_H;
        let state = panel.state_for(kind);

        // Icon.
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(px + PAD_X, row_top + 7.0, 16.0, 16.0),
            text: kind.icon().to_owned(),
            font_size: 12.0,
            color: TEXT_MAIN,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        // Label.
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(px + PAD_X + 18.0, row_top + 9.0, 90.0, FONT_SZ_SM * 1.3),
            text: kind.label().to_owned(),
            font_size: FONT_SZ_SM,
            color: TEXT_MAIN,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        // Toggle button: coloured badge on the right.
        let (btn_fg, btn_bg) = state_colors(state);
        let btn_x = px + PANEL_W - PAD_X - BTN_W;
        let btn_y = row_top + (ROW_H - BTN_H) / 2.0;
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(btn_x, btn_y, BTN_W, BTN_H),
            radii: uniform_radii(BTN_RADIUS),
            color: btn_bg,
        });
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(btn_x + 6.0, btn_y + 3.0, BTN_W - 12.0, FONT_SZ_SM * 1.2),
            text: state.label().to_owned(),
            font_size: FONT_SZ_SM,
            color: btn_fg,
            font_family: Vec::new(),
            font_weight: FontWeight::BOLD,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Top-left corner of the permission panel in CSS px.
fn panel_origin(tab_bar_h: f32) -> (f32, f32) {
    (PANEL_LEFT_MARGIN, tab_bar_h + PANEL_TOP_OFFSET)
}

fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii {
        tl: r, tl_y: r,
        tr: r, tr_y: r,
        br: r, br_y: r,
        bl: r, bl_y: r,
    }
}

/// Foreground and background colours for a permission state toggle button.
fn state_colors(state: PermissionState) -> (Color, Color) {
    match state {
        PermissionState::Allow => (ALLOW_FG, ALLOW_BG),
        PermissionState::Deny => (DENY_FG, DENY_BG),
        PermissionState::Ask => (ASK_FG, ASK_BG),
    }
}

/// Truncate a label to at most `max_chars` characters, appending "…" if needed.
fn truncate_label(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let truncated: String = s.chars().take(max_chars - 1).collect();
    format!("{truncated}…")
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

    // ── Rendering ────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_emits_commands() {
        let p = make_panel(Some("https://example.com"));
        let dl = build_panel(&p, TAB_H);
        assert!(!dl.is_empty());
    }

    #[test]
    fn build_panel_shows_origin() {
        let p = make_panel(Some("https://example.com"));
        let dl = build_panel(&p, TAB_H);
        let has_origin = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("example.com"))
        });
        assert!(has_origin, "panel must render the current origin");
    }

    #[test]
    fn build_panel_shows_all_kinds() {
        let p = make_panel(Some("https://example.com"));
        let dl = build_panel(&p, TAB_H);
        for kind in PermissionKind::ALL {
            let found = dl.iter().any(|c| {
                matches!(c, DisplayCommand::DrawText { text, .. } if text == kind.label())
            });
            assert!(found, "panel must show row for {:?}", kind);
        }
    }

    #[test]
    fn build_panel_shows_allow_label_after_grant() {
        let mut p = make_panel(Some("https://example.com"));
        p.cycle_permission(PermissionKind::Camera);
        let dl = build_panel(&p, TAB_H);
        let has_allow = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Allow")
        });
        assert!(has_allow, "panel must show Allow button after granting camera");
    }

    #[test]
    fn truncate_short_label() {
        assert_eq!(truncate_label("example.com", 24), "example.com");
    }

    #[test]
    fn truncate_long_label() {
        let long = "a".repeat(30);
        let t = truncate_label(&long, 24);
        assert!(t.chars().count() <= 24);
        assert!(t.ends_with('…'));
    }
}
