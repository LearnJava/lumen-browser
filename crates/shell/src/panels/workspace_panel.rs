//! Workspace switcher panel (7A.3): bottom-docked horizontal bar for switching
//! between named workspaces.
//!
//! A workspace is a named group of tabs with optional cookie isolation and a
//! colour label.  The panel renders as a slim bar at the bottom of the window
//! (`#workspaceBar` in the engine chrome, `bind_workspace_bar`,
//! UX-CHROME-PANELS):
//!
//! ```text
//! [ Default × ]  [ Work × ]  [ Home × ]  [ + ]
//! ```
//!
//! Toggled with Ctrl+Shift+W.  When visible, `viewport_height_css()` subtracts
//! `SWITCHER_HEIGHT` so the page layout does not extend behind the bar.
//!
//! Each chip has a "×" delete button on the right edge (`delete-workspace`).
//! The "+" button (`add-workspace`) creates a new workspace with an
//! auto-generated name ("Workspace N").  Changes are immediately persisted to
//! the `Workspaces` SQLite store.

use lumen_layout::Color;

// ── Visual constants ─────────────────────────────────────────────────────────

/// Height of the workspace switcher bar in CSS px (`#workspaceBar` height).
pub const SWITCHER_HEIGHT: f32 = 32.0;

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

    /// Accent colour of the currently active workspace (DS-12, level-1
    /// hierarchy marker). `None` when no workspace is selected or the active
    /// id no longer matches a cached entry — callers fall back to
    /// `Palette::accent`.
    #[must_use]
    pub fn active_accent(&self) -> Option<Color> {
        let active_id = self.active_id?;
        self.workspaces.iter().find(|w| w.id == active_id).map(|w| w.accent)
    }
}

impl Default for WorkspacePanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Default accent colours assigned to newly created workspaces, cycled by
/// creation order (DS-12; reference design `docs/design/lumen-v3_3.html`
/// lines 851–859: Личное/Проект Х/Чтение).
const DEFAULT_WORKSPACE_COLORS: [&str; 3] = ["#0066FF", "#8B5CF6", "#1F9D55"];

/// Pick a default colour for the `n`-th workspace created (0-indexed),
/// cycling through [`DEFAULT_WORKSPACE_COLORS`].
#[must_use]
pub fn default_color_for_index(n: usize) -> &'static str {
    DEFAULT_WORKSPACE_COLORS[n % DEFAULT_WORKSPACE_COLORS.len()]
}

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn active_accent_none_when_no_workspace_selected() {
        let p = make_panel(&["Work"], None);
        assert_eq!(p.active_accent(), None);
    }

    #[test]
    fn active_accent_matches_active_workspace() {
        let mut p = make_panel(&["Personal", "Work"], Some(2));
        p.workspaces[1].accent = Color { r: 139, g: 92, b: 246, a: 255 }; // #8B5CF6
        assert_eq!(p.active_accent(), Some(Color { r: 139, g: 92, b: 246, a: 255 }));
    }

    #[test]
    fn active_accent_none_when_id_not_cached() {
        let p = make_panel(&["Work"], Some(99));
        assert_eq!(p.active_accent(), None);
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

    #[test]
    fn default_color_for_index_cycles_reference_palette() {
        assert_eq!(default_color_for_index(0), "#0066FF");
        assert_eq!(default_color_for_index(1), "#8B5CF6");
        assert_eq!(default_color_for_index(2), "#1F9D55");
        assert_eq!(default_color_for_index(3), "#0066FF");
    }
}
