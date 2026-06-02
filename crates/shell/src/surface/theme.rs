//! Design tokens for the Panel/Surface system (ADR-009).
//!
//! Every panel reads colours and sizes from a [`Theme`] rather than hard-coding
//! them, so switching the whole shell's look is one value swap.  This is a
//! pragmatic subset of the full token set in [`docs/shell-ui-architecture.md`];
//! tokens are added as panels migrate onto the system.

use lumen_layout::Color;

/// Convenience: build an opaque `Color` from 8-bit RGB.
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

/// All design tokens for one shell appearance.
///
/// Colours follow a chrome / paper / ink / accent grouping: *chrome* is shell
/// furniture (bars, sidebars), *paper* is content background, *ink* is text,
/// *accent* highlights interactive and active elements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Human-readable theme name, e.g. `"sand-indigo"`.
    pub name: &'static str,

    // ── Chrome / surfaces ────────────────────────────────────────────────
    /// Background of sidebars and bars.
    pub chrome_bg: Color,
    /// Deeper chrome background (title bar, nested panels).
    pub chrome_deep: Color,
    /// Separators and panel borders.
    pub chrome_edge: Color,
    /// Content-area background.
    pub paper: Color,
    /// Alternate row / selection background.
    pub paper_2: Color,

    // ── Text ─────────────────────────────────────────────────────────────
    /// Primary text colour.
    pub ink: Color,
    /// Secondary text colour.
    pub ink_soft: Color,
    /// Muted / inactive text.
    pub ink_mute: Color,

    // ── Accent ───────────────────────────────────────────────────────────
    /// Primary accent (active elements, primary buttons).
    pub accent: Color,
    /// Soft accent fill (selected row background).
    pub accent_soft: Color,

    // ── States ───────────────────────────────────────────────────────────
    /// Hover background.
    pub state_hover: Color,
    /// Selected-element background (distinct from active).
    pub state_selected: Color,

    // ── Sizes (CSS px) ───────────────────────────────────────────────────
    /// Default sidebar width.
    pub sidebar_w: f32,
    /// Top toolbar height.
    pub topbar_h: f32,
    /// Title bar height.
    pub titlebar_h: f32,
    /// Tab row height.
    pub tab_row_h: f32,
    /// Right-hand panel width.
    pub right_panel_w: f32,

    // ── Typography (CSS px) ──────────────────────────────────────────────
    /// Base UI font size.
    pub size_base: f32,
    /// Small / secondary font size.
    pub size_small: f32,
    /// Uppercase label font size.
    pub size_label: f32,
    /// Title font size.
    pub size_title: f32,

    // ── Shape (CSS px) ───────────────────────────────────────────────────
    /// Small corner radius.
    pub radius_sm: f32,
    /// Medium corner radius.
    pub radius_md: f32,
    /// Large corner radius (windows, popovers).
    pub radius_lg: f32,
}

impl Theme {
    /// V1 / default: warm sand + indigo (light).
    pub fn sand_indigo() -> Self {
        Self {
            name: "sand-indigo",
            chrome_bg: rgb(0xEE, 0xE7, 0xDA),
            chrome_deep: rgb(0xE2, 0xD9, 0xC8),
            chrome_edge: rgb(0xCB, 0xBF, 0xAA),
            paper: rgb(0xFB, 0xF8, 0xF2),
            paper_2: rgb(0xF1, 0xEC, 0xE1),
            ink: rgb(0x2A, 0x26, 0x1F),
            ink_soft: rgb(0x5C, 0x54, 0x46),
            ink_mute: rgb(0x91, 0x88, 0x76),
            accent: rgb(0x4F, 0x46, 0xC4),
            accent_soft: rgb(0xDD, 0xDA, 0xF4),
            state_hover: rgb(0xE6, 0xDF, 0xD0),
            state_selected: rgb(0xD6, 0xD2, 0xEC),
            sidebar_w: 280.0,
            topbar_h: 40.0,
            titlebar_h: 32.0,
            tab_row_h: 28.0,
            right_panel_w: 320.0,
            size_base: 13.0,
            size_small: 11.0,
            size_label: 10.0,
            size_title: 15.0,
            radius_sm: 3.0,
            radius_md: 6.0,
            radius_lg: 10.0,
        }
    }

    /// V2 / dark: graphite + amber.
    pub fn graphite_amber() -> Self {
        Self {
            name: "graphite-amber",
            chrome_bg: rgb(0x1C, 0x1C, 0x22),
            chrome_deep: rgb(0x14, 0x14, 0x19),
            chrome_edge: rgb(0x33, 0x33, 0x40),
            paper: rgb(0x24, 0x24, 0x2C),
            paper_2: rgb(0x2C, 0x2C, 0x36),
            ink: rgb(0xE8, 0xE4, 0xDA),
            ink_soft: rgb(0xB2, 0xAC, 0x9E),
            ink_mute: rgb(0x77, 0x73, 0x68),
            accent: rgb(0xF0, 0xA8, 0x30),
            accent_soft: rgb(0x4A, 0x3A, 0x1C),
            state_hover: rgb(0x2E, 0x2E, 0x38),
            state_selected: rgb(0x3A, 0x32, 0x22),
            sidebar_w: 280.0,
            topbar_h: 40.0,
            titlebar_h: 32.0,
            tab_row_h: 28.0,
            right_panel_w: 320.0,
            size_base: 13.0,
            size_small: 11.0,
            size_label: 10.0,
            size_title: 15.0,
            radius_sm: 3.0,
            radius_md: 6.0,
            radius_lg: 10.0,
        }
    }

    /// Pick a built-in theme by OS dark-mode preference.
    pub fn for_dark_mode(dark: bool) -> Self {
        if dark {
            Self::graphite_amber()
        } else {
            Self::sand_indigo()
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::sand_indigo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_have_distinct_names() {
        assert_eq!(Theme::sand_indigo().name, "sand-indigo");
        assert_eq!(Theme::graphite_amber().name, "graphite-amber");
        assert_ne!(Theme::sand_indigo(), Theme::graphite_amber());
    }

    #[test]
    fn for_dark_mode_selects_dark() {
        assert_eq!(Theme::for_dark_mode(true).name, "graphite-amber");
        assert_eq!(Theme::for_dark_mode(false).name, "sand-indigo");
    }

    #[test]
    fn default_is_light() {
        assert_eq!(Theme::default().name, "sand-indigo");
    }

    #[test]
    fn opaque_colors() {
        assert_eq!(Theme::sand_indigo().ink.a, 255);
        assert_eq!(Theme::graphite_amber().paper.a, 255);
    }
}
