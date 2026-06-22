//! Persisted docked-panel layout (F2-6: drag-to-resize + layout persistence).
//!
//! The shell's docked sidebars (vertical tabs, tree tabs, AI assistant, web
//! sidebar) historically used hard-coded `PANEL_WIDTH` constants and were never
//! remembered across restarts. This module replaces the constants with
//! per-panel *runtime* widths the user can drag-resize, and persists them to a
//! small text file in the portable browser data directory so the layout
//! survives a restart.
//!
//! Persistence format — a flat `key = value` text file (no new dependency,
//! mirroring [`crate::config`]). One line per panel:
//!
//! ```text
//! # Lumen panel layout
//! vertical-tabs = 200
//! sidebar = 320
//! ```
//!
//! Unknown keys are ignored; malformed values fall back to the panel's compiled
//! default. The file lives at `<exe_dir>/data/ui/panel_layout.txt`
//! ([browser-folder storage policy], not `%APPDATA%`/XDG).
//!
//! [browser-folder storage policy]: crate::adblock::browser_data_dir

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::adblock::browser_data_dir;

/// Stable persistence key + resize id for the left vertical-tabs sidebar.
pub const ID_VERTICAL_TABS: &str = "vertical-tabs";
/// Stable persistence key + resize id for the left tree-tabs sidebar.
pub const ID_TREE_TABS: &str = "tree-tabs";
/// Stable persistence key + resize id for the right AI assistant sidebar.
pub const ID_AI: &str = "ai";
/// Stable persistence key + resize id for the right web sidebar.
pub const ID_SIDEBAR: &str = "sidebar";

/// Which window edge a docked sidebar hugs.
///
/// Left-docked panels (vertical/tree tabs) grow rightward from `x = 0`; their
/// resize handle sits at `x = width`. Right-docked panels (AI, web sidebar)
/// grow leftward from the window's right edge; their handle sits at
/// `x = window_w − width`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dock {
    /// Hugs the left window edge.
    Left,
    /// Hugs the right window edge.
    Right,
}

impl Dock {
    /// Resolve the dragged cursor x-position into a panel width for this dock,
    /// given the window width. Both in CSS px. The result is *not* clamped here
    /// — [`PanelLayout::set_width`] applies the `[MIN_WIDTH, MAX_WIDTH]` clamp.
    #[must_use]
    pub fn width_from_cursor(self, cursor_x: f32, window_w: f32) -> f32 {
        match self {
            Dock::Left => cursor_x,
            Dock::Right => window_w - cursor_x,
        }
    }

    /// The opposite window edge (used by cross-dock "move to other side").
    #[must_use]
    pub fn opposite(self) -> Dock {
        match self {
            Dock::Left => Dock::Right,
            Dock::Right => Dock::Left,
        }
    }

    /// Lowercase token used in the persisted layout file (`left` / `right`).
    #[must_use]
    pub fn as_token(self) -> &'static str {
        match self {
            Dock::Left => "left",
            Dock::Right => "right",
        }
    }

    /// Parse a persisted token; `None` for anything but `left` / `right`.
    #[must_use]
    pub fn from_token(s: &str) -> Option<Dock> {
        match s {
            "left" => Some(Dock::Left),
            "right" => Some(Dock::Right),
            _ => None,
        }
    }
}

/// Compiled default dock side for a panel id.
///
/// The tab sidebars hug the left edge; the AI assistant and web sidebar hug the
/// right. Unknown ids default to the left edge. A fresh profile (no persisted
/// override) therefore reproduces the historical fixed-side layout exactly.
#[must_use]
pub fn default_dock(id: &str) -> Dock {
    match id {
        ID_AI | ID_SIDEBAR => Dock::Right,
        _ => Dock::Left,
    }
}

/// Half-width (CSS px) of the invisible resize hit-zone straddling a panel's
/// inner edge. A cursor within `±RESIZE_GRAB` of the edge starts a resize drag.
pub const RESIZE_GRAB: f32 = 4.0;

/// Minimum width a docked panel can be resized to, in CSS px.
///
/// Below this a panel becomes unusable (header/close button clip), so resize
/// drags clamp here rather than allowing a sliver.
pub const MIN_WIDTH: f32 = 120.0;

/// Maximum width a docked panel can be resized to, in CSS px.
///
/// Keeps at least a usable page viewport on typical windows; resize drags clamp
/// here.
pub const MAX_WIDTH: f32 = 600.0;

/// Runtime, persisted widths of the docked panels, keyed by panel id.
///
/// Widths are stored in CSS px and always read back clamped to
/// `[MIN_WIDTH, MAX_WIDTH]`. A missing key means "use the panel's compiled
/// default", so a fresh profile renders exactly as before this feature.
#[derive(Debug, Clone, Default)]
pub struct PanelLayout {
    /// panel id → user-chosen width (CSS px). Absent ⇒ compiled default.
    widths: BTreeMap<String, f32>,
    /// panel id → user-chosen dock side (cross-dock override). Absent ⇒
    /// [`default_dock`] for that id.
    sides: BTreeMap<String, Dock>,
}

impl PanelLayout {
    /// `<exe_dir>/data/ui` — directory holding shell UI layout state.
    fn dir() -> PathBuf {
        browser_data_dir().join("ui")
    }

    /// Path to the persisted layout file.
    fn path() -> PathBuf {
        Self::dir().join("panel_layout.txt")
    }

    /// Load the persisted layout, or an empty (all-default) layout if the file
    /// is missing or unreadable.
    ///
    /// Never fails: a corrupt or absent file yields the default layout so the
    /// shell always starts.
    #[must_use]
    pub fn load() -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }

    /// Parse the flat `key = value` text format. Malformed lines are skipped.
    ///
    /// Keys are namespaced by suffix: `<id>.dock` carries a side token, plain
    /// `<id>` carries a width. Backward-compatible — files written before
    /// cross-dock (widths only) parse unchanged, and an unknown suffix is
    /// ignored rather than treated as a width.
    fn parse(text: &str) -> Self {
        let mut widths = BTreeMap::new();
        let mut sides = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim();
            if key.is_empty() {
                continue;
            }
            if let Some(id) = key.strip_suffix(".dock") {
                if !id.is_empty()
                    && let Some(d) = Dock::from_token(val)
                {
                    sides.insert(id.to_string(), d);
                }
            } else if let Ok(w) = val.parse::<f32>()
                && w.is_finite()
            {
                widths.insert(key.to_string(), w.clamp(MIN_WIDTH, MAX_WIDTH));
            }
        }
        Self { widths, sides }
    }

    /// Serialise the layout to the flat text format (deterministic key order:
    /// all widths, then all dock-side overrides).
    fn serialize(&self) -> String {
        let mut out = String::from("# Lumen panel layout\n");
        for (id, w) in &self.widths {
            out.push_str(id);
            out.push_str(" = ");
            // Trim a trailing ".0" for tidy integer widths.
            if (w.fract()).abs() < f32::EPSILON {
                out.push_str(&format!("{}", *w as i64));
            } else {
                out.push_str(&format!("{w}"));
            }
            out.push('\n');
        }
        for (id, d) in &self.sides {
            out.push_str(id);
            out.push_str(".dock = ");
            out.push_str(d.as_token());
            out.push('\n');
        }
        out
    }

    /// Width to use for the panel `id`, falling back to `default` when the user
    /// has never resized it. Always clamped to `[MIN_WIDTH, MAX_WIDTH]`.
    #[must_use]
    pub fn width_for(&self, id: &str, default: f32) -> f32 {
        self.widths
            .get(id)
            .copied()
            .unwrap_or(default)
            .clamp(MIN_WIDTH, MAX_WIDTH)
    }

    /// Record a new width for panel `id` (clamped). Returns `true` if the stored
    /// value changed, so the caller can decide whether a relayout/save is due.
    pub fn set_width(&mut self, id: &str, width: f32) -> bool {
        if !width.is_finite() {
            return false;
        }
        let w = width.clamp(MIN_WIDTH, MAX_WIDTH);
        match self.widths.get(id) {
            Some(prev) if (prev - w).abs() < f32::EPSILON => false,
            _ => {
                self.widths.insert(id.to_string(), w);
                true
            }
        }
    }

    /// Effective dock side for panel `id`: the user's cross-dock override, or
    /// `default` (typically [`default_dock`]) when never moved.
    #[must_use]
    pub fn dock_for(&self, id: &str, default: Dock) -> Dock {
        self.sides.get(id).copied().unwrap_or(default)
    }

    /// Record a dock side for panel `id`. Returns `true` if the stored value
    /// changed, so the caller can decide whether a relayout/save is due.
    pub fn set_dock(&mut self, id: &str, dock: Dock) -> bool {
        match self.sides.get(id) {
            Some(prev) if *prev == dock => false,
            _ => {
                self.sides.insert(id.to_string(), dock);
                true
            }
        }
    }

    /// Persist the layout to disk (best-effort).
    ///
    /// Creates `<data>/ui/` if missing. Write failures are swallowed: losing a
    /// remembered width is preferable to interrupting the user.
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(Self::dir());
        let _ = std::fs::write(Self::path(), self.serialize());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_falls_back_to_compiled() {
        let layout = PanelLayout::default();
        assert_eq!(layout.width_for("sidebar", 300.0), 300.0);
        assert_eq!(layout.width_for("ai", 200.0), 200.0);
    }

    #[test]
    fn set_then_read_roundtrips() {
        let mut layout = PanelLayout::default();
        assert!(layout.set_width("sidebar", 360.0));
        assert_eq!(layout.width_for("sidebar", 300.0), 360.0);
    }

    #[test]
    fn set_clamps_to_bounds() {
        let mut layout = PanelLayout::default();
        layout.set_width("sidebar", 5000.0);
        assert_eq!(layout.width_for("sidebar", 300.0), MAX_WIDTH);
        layout.set_width("ai", 10.0);
        assert_eq!(layout.width_for("ai", 200.0), MIN_WIDTH);
    }

    #[test]
    fn set_reports_change() {
        let mut layout = PanelLayout::default();
        assert!(layout.set_width("sidebar", 250.0));
        assert!(!layout.set_width("sidebar", 250.0)); // unchanged
        assert!(layout.set_width("sidebar", 251.0)); // changed
    }

    #[test]
    fn set_rejects_non_finite() {
        let mut layout = PanelLayout::default();
        assert!(!layout.set_width("sidebar", f32::NAN));
        assert!(!layout.set_width("sidebar", f32::INFINITY));
        assert_eq!(layout.width_for("sidebar", 300.0), 300.0);
    }

    #[test]
    fn parse_skips_malformed_and_clamps() {
        let text = "\
# comment
vertical-tabs = 240
broken line without equals
sidebar = 5000
ai =
tree-tabs = abc
empty =
 = 100
";
        let layout = PanelLayout::parse(text);
        assert_eq!(layout.width_for("vertical-tabs", 200.0), 240.0);
        assert_eq!(layout.width_for("sidebar", 300.0), MAX_WIDTH); // clamped
        assert_eq!(layout.width_for("ai", 200.0), 200.0); // empty value → default
        assert_eq!(layout.width_for("tree-tabs", 200.0), 200.0); // non-numeric → default
    }

    #[test]
    fn serialize_parse_roundtrip() {
        let mut layout = PanelLayout::default();
        layout.set_width("sidebar", 320.0);
        layout.set_width("vertical-tabs", 180.5);
        let text = layout.serialize();
        let back = PanelLayout::parse(&text);
        assert_eq!(back.width_for("sidebar", 300.0), 320.0);
        assert_eq!(back.width_for("vertical-tabs", 200.0), 180.5);
    }

    #[test]
    fn dock_width_from_cursor() {
        assert_eq!(Dock::Left.width_from_cursor(180.0, 1000.0), 180.0);
        assert_eq!(Dock::Right.width_from_cursor(700.0, 1000.0), 300.0);
    }

    #[test]
    fn serialize_trims_integer_fraction() {
        let mut layout = PanelLayout::default();
        layout.set_width("sidebar", 300.0);
        let text = layout.serialize();
        assert!(text.contains("sidebar = 300\n"), "got: {text}");
    }

    // ── Cross-dock side persistence ───────────────────────────────────────────

    #[test]
    fn default_dock_matches_compiled_sides() {
        assert_eq!(default_dock(ID_VERTICAL_TABS), Dock::Left);
        assert_eq!(default_dock(ID_TREE_TABS), Dock::Left);
        assert_eq!(default_dock(ID_AI), Dock::Right);
        assert_eq!(default_dock(ID_SIDEBAR), Dock::Right);
        assert_eq!(default_dock("unknown"), Dock::Left);
    }

    #[test]
    fn dock_for_falls_back_to_default() {
        let layout = PanelLayout::default();
        assert_eq!(layout.dock_for(ID_VERTICAL_TABS, Dock::Left), Dock::Left);
        assert_eq!(layout.dock_for(ID_AI, Dock::Right), Dock::Right);
    }

    #[test]
    fn set_dock_overrides_and_reports_change() {
        let mut layout = PanelLayout::default();
        assert!(layout.set_dock(ID_VERTICAL_TABS, Dock::Right));
        assert_eq!(layout.dock_for(ID_VERTICAL_TABS, Dock::Left), Dock::Right);
        assert!(!layout.set_dock(ID_VERTICAL_TABS, Dock::Right)); // unchanged
        assert!(layout.set_dock(ID_VERTICAL_TABS, Dock::Left)); // changed back
    }

    #[test]
    fn dock_opposite_and_tokens() {
        assert_eq!(Dock::Left.opposite(), Dock::Right);
        assert_eq!(Dock::Right.opposite(), Dock::Left);
        assert_eq!(Dock::Left.as_token(), "left");
        assert_eq!(Dock::from_token("right"), Some(Dock::Right));
        assert_eq!(Dock::from_token("middle"), None);
    }

    #[test]
    fn side_survives_serialize_parse_roundtrip() {
        let mut layout = PanelLayout::default();
        layout.set_width(ID_AI, 280.0);
        layout.set_dock(ID_VERTICAL_TABS, Dock::Right);
        layout.set_dock(ID_AI, Dock::Left);
        let text = layout.serialize();
        let back = PanelLayout::parse(&text);
        assert_eq!(back.dock_for(ID_VERTICAL_TABS, Dock::Left), Dock::Right);
        assert_eq!(back.dock_for(ID_AI, Dock::Right), Dock::Left);
        assert_eq!(back.width_for(ID_AI, 200.0), 280.0); // widths still intact
    }

    #[test]
    fn parse_ignores_bad_dock_token_and_width_keys_unaffected() {
        let text = "\
# comment
vertical-tabs = 240
vertical-tabs.dock = right
ai.dock = sideways
.dock = left
";
        let layout = PanelLayout::parse(text);
        assert_eq!(layout.width_for("vertical-tabs", 200.0), 240.0);
        assert_eq!(layout.dock_for("vertical-tabs", Dock::Left), Dock::Right);
        // bad token → default; empty id → ignored
        assert_eq!(layout.dock_for("ai", Dock::Right), Dock::Right);
    }
}
