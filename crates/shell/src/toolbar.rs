//! Permanent toolbar strip below the tab bar.
//!
//! `build_toolbar` produces a viewport-locked `DisplayList` for the row.
//! `hit_test` maps CSS-px (x, y) → `ToolbarHit` for mouse dispatch. Mirrors
//! the `tabs::strip` pattern (build/hit_test pair, absolute-window-space
//! coordinates).
//!
//! Scope (DS-9, `docs/tasks/p1-design-v3.md`): navigation cluster (back /
//! forward / reload) on the left, a fixed action cluster on the right (find,
//! web sidebar, AI sidebar, downloads, DevTools, settings). The centre is
//! left empty — the inline omnibox lands in DS-10.

use lumen_core::geom::Rect;
use lumen_layout::{FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::panels::themes::Palette;
use crate::tabs::strip::TAB_BAR_HEIGHT;
use crate::theme_tokens::{radius, size};

/// Total CSS-px height of the tab bar + toolbar stack. This is the y-origin
/// of the page content region and of every chrome panel anchored "below the
/// bars" — see `docs/tasks/p1-design-v3.md` DS-9 step 2/3.
pub const CHROME_H: f32 = TAB_BAR_HEIGHT + size::TOOLBAR_H;

/// Side length of a toolbar button in CSS px (`.tb-btn` in the prototype).
const BTN_SZ: f32 = 26.0;

/// Gap between adjacent buttons within a cluster.
const BTN_GAP: f32 = 2.0;

/// Horizontal padding between the window edge and the outermost cluster.
const CLUSTER_PAD: f32 = 10.0;

/// Icon glyph size in CSS px, matching `tabs::strip`'s button icons.
const ICON_SZ: f32 = 12.0;

/// Number of buttons in the right-hand action cluster.
const RIGHT_BTN_COUNT: usize = 6;

/// A click target within the toolbar row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHit {
    /// No button under the cursor (still within the toolbar row).
    Empty,
    /// Navigate back one entry in session history.
    Back,
    /// Navigate forward one entry in session history.
    Forward,
    /// Reload the active tab.
    Reload,
    /// Toggle the find-in-page bar.
    Find,
    /// Toggle the web sidebar.
    WebSidebar,
    /// Toggle the AI sidebar.
    AiSidebar,
    /// Toggle the downloads panel.
    Downloads,
    /// Toggle the DevTools console.
    DevTools,
    /// Toggle the settings panel.
    Settings,
}

/// Which right-cluster buttons should render in their "open" (lit) state —
/// mirrors the corresponding panel's `visible` flag.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolbarActive {
    /// `self.find.is_open()`.
    pub find: bool,
    /// `self.sidebar.visible` (web sidebar).
    pub web_sidebar: bool,
    /// `self.ai_panel.visible`.
    pub ai_sidebar: bool,
    /// `self.downloads.visible`.
    pub downloads: bool,
    /// `self.devtools_console.visible`.
    pub devtools: bool,
    /// `self.settings_panel.visible`.
    pub settings: bool,
}

/// Left edge x-coordinate of each left-cluster button (back, forward, reload).
fn left_btn_x(idx: usize) -> f32 {
    CLUSTER_PAD + idx as f32 * (BTN_SZ + BTN_GAP)
}

/// Left edge x-coordinate of the `idx`-th right-cluster button (0 = find,
/// ..= 5 = settings), given the window width.
fn right_btn_x(window_w: f32, idx: usize) -> f32 {
    let cluster_w = RIGHT_BTN_COUNT as f32 * BTN_SZ + (RIGHT_BTN_COUNT - 1) as f32 * BTN_GAP;
    window_w - CLUSTER_PAD - cluster_w + idx as f32 * (BTN_SZ + BTN_GAP)
}

/// Hit-test a click at CSS-px `(x, y)` against the toolbar row.
///
/// Returns `ToolbarHit::Empty` if `y` falls outside `TAB_BAR_HEIGHT..CHROME_H`.
pub fn hit_test(x: f32, y: f32, window_w: f32) -> ToolbarHit {
    if !(TAB_BAR_HEIGHT..CHROME_H).contains(&y) {
        return ToolbarHit::Empty;
    }
    let nav = [ToolbarHit::Back, ToolbarHit::Forward, ToolbarHit::Reload];
    for (i, hit) in nav.into_iter().enumerate() {
        let bx = left_btn_x(i);
        if (bx..bx + BTN_SZ).contains(&x) {
            return hit;
        }
    }
    let right = [
        ToolbarHit::Find,
        ToolbarHit::WebSidebar,
        ToolbarHit::AiSidebar,
        ToolbarHit::Downloads,
        ToolbarHit::DevTools,
        ToolbarHit::Settings,
    ];
    for (i, hit) in right.into_iter().enumerate() {
        let bx = right_btn_x(window_w, i);
        if (bx..bx + BTN_SZ).contains(&x) {
            return hit;
        }
    }
    ToolbarHit::Empty
}

/// Uniform corner radii helper (mirrors the identically-named private helper
/// in other chrome modules, e.g. `page_context_menu.rs`).
fn corners(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

/// Push one button (rounded-rect background + centered glyph) into `out`.
fn push_btn(out: &mut DisplayList, btn_x: f32, glyph: &str, active: bool, pal: &Palette) {
    let bg = if active { pal.item_selected_bg } else { pal.toolbar_bg };
    let icon_color = if active { pal.accent } else { pal.text_dim };
    let btn_y = TAB_BAR_HEIGHT + (size::TOOLBAR_H - BTN_SZ) * 0.5;
    if active {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(btn_x, btn_y, BTN_SZ, BTN_SZ),
            radii: corners(radius::MD),
            color: bg,
        });
    }
    let icon_x = btn_x + (BTN_SZ - ICON_SZ) * 0.5;
    let icon_y = btn_y + (BTN_SZ - ICON_SZ * 1.2) * 0.5;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(icon_x, icon_y, ICON_SZ, ICON_SZ * 1.2),
        text: glyph.to_owned(),
        font_size: ICON_SZ,
        color: icon_color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: None,
    });
}

/// Build a viewport-locked display list for the toolbar row.
///
/// Renders the bar background + bottom divider, the left navigation cluster
/// (back/forward/reload — always enabled; DoD only requires the buttons call
/// the existing handlers, not disabled-state fidelity) and the right action
/// cluster. `active` lights the buttons whose panel is currently open.
pub fn build_toolbar(window_w: f32, pal: &Palette, active: ToolbarActive) -> DisplayList {
    let mut out = DisplayList::with_capacity(2 + 9 * 2);
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, TAB_BAR_HEIGHT, window_w, size::TOOLBAR_H),
        color: pal.toolbar_bg,
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, CHROME_H - 1.0, window_w, 1.0),
        color: pal.divider,
    });

    push_btn(&mut out, left_btn_x(0), "\u{2190}", false, pal); // ← back
    push_btn(&mut out, left_btn_x(1), "\u{2192}", false, pal); // → forward
    push_btn(&mut out, left_btn_x(2), "\u{21BB}", false, pal); // ↻ reload

    push_btn(&mut out, right_btn_x(window_w, 0), "\u{2315}", active.find, pal); // ⌕ find
    push_btn(&mut out, right_btn_x(window_w, 1), "\u{25EB}", active.web_sidebar, pal); // ◫ web sidebar
    push_btn(&mut out, right_btn_x(window_w, 2), "\u{2726}", active.ai_sidebar, pal); // ✦ AI sidebar
    push_btn(&mut out, right_btn_x(window_w, 3), "\u{2B07}", active.downloads, pal); // ⬇ downloads
    push_btn(&mut out, right_btn_x(window_w, 4), "\u{2692}", active.devtools, pal); // ⚒ DevTools
    push_btn(&mut out, right_btn_x(window_w, 5), "\u{2699}", active.settings, pal); // ⚙ settings

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dark() -> Palette {
        Palette::DARK
    }

    #[test]
    fn hit_test_outside_row_is_empty() {
        assert_eq!(hit_test(20.0, 0.0, 1024.0), ToolbarHit::Empty);
        assert_eq!(hit_test(20.0, CHROME_H + 1.0, 1024.0), ToolbarHit::Empty);
    }

    #[test]
    fn hit_test_left_cluster() {
        let y = TAB_BAR_HEIGHT + 1.0;
        assert_eq!(hit_test(left_btn_x(0) + 2.0, y, 1024.0), ToolbarHit::Back);
        assert_eq!(hit_test(left_btn_x(1) + 2.0, y, 1024.0), ToolbarHit::Forward);
        assert_eq!(hit_test(left_btn_x(2) + 2.0, y, 1024.0), ToolbarHit::Reload);
    }

    #[test]
    fn hit_test_right_cluster() {
        let y = TAB_BAR_HEIGHT + 1.0;
        let w = 1024.0;
        assert_eq!(hit_test(right_btn_x(w, 0) + 2.0, y, w), ToolbarHit::Find);
        assert_eq!(hit_test(right_btn_x(w, 1) + 2.0, y, w), ToolbarHit::WebSidebar);
        assert_eq!(hit_test(right_btn_x(w, 2) + 2.0, y, w), ToolbarHit::AiSidebar);
        assert_eq!(hit_test(right_btn_x(w, 3) + 2.0, y, w), ToolbarHit::Downloads);
        assert_eq!(hit_test(right_btn_x(w, 4) + 2.0, y, w), ToolbarHit::DevTools);
        assert_eq!(hit_test(right_btn_x(w, 5) + 2.0, y, w), ToolbarHit::Settings);
    }

    #[test]
    fn hit_test_gap_between_buttons_is_empty() {
        let y = TAB_BAR_HEIGHT + 1.0;
        // Just past the back button, inside the 2 px gap before forward.
        assert_eq!(hit_test(left_btn_x(0) + BTN_SZ + 1.0, y, 1024.0), ToolbarHit::Empty);
    }

    #[test]
    fn build_toolbar_emits_background_and_nine_buttons() {
        let cmds = build_toolbar(1024.0, &dark(), ToolbarActive::default());
        let fill_rects = cmds
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .count();
        let texts = cmds.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).count();
        // Background + divider = 2 FillRects; no button is "active" so no
        // FillRoundedRect highlights are emitted; 9 buttons → 9 glyphs.
        assert_eq!(fill_rects, 2);
        assert_eq!(texts, 9);
    }

    #[test]
    fn build_toolbar_active_button_gets_highlight() {
        let active = ToolbarActive { settings: true, ..Default::default() };
        let cmds = build_toolbar(1024.0, &dark(), active);
        let highlights =
            cmds.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).count();
        assert_eq!(highlights, 1);
    }

    #[test]
    fn chrome_h_is_tab_bar_plus_toolbar() {
        assert!((CHROME_H - (TAB_BAR_HEIGHT + size::TOOLBAR_H)).abs() < 1e-6);
    }
}
