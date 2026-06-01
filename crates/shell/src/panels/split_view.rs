//! Two-pane horizontal split view (ADR-009 §7A.4).
//!
//! The left pane is always the live active-tab state stored directly in
//! `Lumen`. The right pane is a frozen `SplitPane` snapshot: last rendered
//! display list + independent scroll offsets. Only one pane is focused at a
//! time (receives keyboard / scroll input). Clicking anywhere in a pane
//! transfers focus to it.
//!
//! Rendering: `build_combined_dl` merges both display lists into a single
//! `DisplayList` that the existing `Renderer::render` call can consume with
//! `scroll_y = 0.0, scroll_x = 0.0` (scroll is baked into per-pane
//! `PushTransform` commands).

use lumen_core::geom::Rect;
use lumen_layout::{Color, Mat4};
use lumen_paint::{DisplayCommand, DisplayList};

/// Which pane receives keyboard and scroll input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitFocus {
    /// Left pane (the active `Lumen` tab state) is focused.
    #[default]
    Left,
    /// Right pane (the frozen `SplitPane` snapshot) is focused.
    Right,
}

/// Frozen rendering state for the right pane in a split view.
///
/// Content coordinates in `display_list` start at (0, 0) — the same origin
/// as a normal (non-split) page display list. `build_combined_dl` translates
/// them to the correct window position.
#[derive(Clone)]
pub struct SplitPane {
    /// Tab id this pane belongs to (available for future tab-header rendering).
    #[allow(dead_code)]
    pub tab_id: usize,
    /// Last rendered display list for this pane (content coords, origin = 0,0).
    pub display_list: DisplayList,
    /// Vertical scroll offset in CSS px (0 = top of content).
    pub scroll_y: f32,
    /// Horizontal scroll offset in CSS px (0 = left edge of content).
    pub scroll_x: f32,
    /// Full document height in CSS px (used for scrollbar sizing).
    pub content_height: f32,
    /// Full document width in CSS px (used for scrollbar sizing).
    pub content_width: f32,
}

/// Active split-view state: two side-by-side `ContentViewport` slots.
///
/// The left pane is the live `Lumen` active-tab state. The right pane is
/// frozen in [`SplitPane`]. `build_combined_dl` merges them for rendering.
pub struct SplitView {
    /// Right-side frozen pane.
    pub right: SplitPane,
    /// Which pane currently has keyboard / scroll focus.
    pub focused: SplitFocus,
}

impl SplitView {
    /// Open split view: right pane shows the given tab's last rendered state.
    pub fn new(
        tab_id: usize,
        display_list: DisplayList,
        scroll_y: f32,
        scroll_x: f32,
        content_height: f32,
        content_width: f32,
    ) -> Self {
        Self {
            right: SplitPane {
                tab_id,
                display_list,
                scroll_y,
                scroll_x,
                content_height,
                content_width,
            },
            focused: SplitFocus::Left,
        }
    }

    /// Build a combined display list for split-view rendering.
    ///
    /// Merges left and right pane display lists separated by a 1-px divider.
    /// Scroll offsets are baked into `PushTransform` commands so the caller
    /// can pass `scroll_y = 0, scroll_x = 0` to `Renderer::render`.
    ///
    /// # Arguments
    /// * `left_dl` — current page display list (content coords, origin = 0,0).
    /// * `left_scroll_y/x` — left pane scroll offsets in CSS px.
    /// * `split_x` — x of the divider in CSS px (typically `viewport_width / 2`).
    /// * `tab_bar_height` — tab strip height in CSS px (content starts below it).
    /// * `viewport_height` — full window height in CSS px (including tab strip).
    pub fn build_combined_dl(
        &self,
        left_dl: &[DisplayCommand],
        left_scroll_y: f32,
        left_scroll_x: f32,
        split_x: f32,
        tab_bar_height: f32,
        viewport_height: f32,
    ) -> DisplayList {
        let content_h = viewport_height - tab_bar_height;
        let right_x = split_x + 1.0;
        let right_w = split_x - 1.0;

        let mut out: DisplayList =
            Vec::with_capacity(left_dl.len() + self.right.display_list.len() + 10);

        // ── Left pane ──────────────────────────────────────────────────────
        out.push(DisplayCommand::PushClipRect {
            rect: Rect { x: 0.0, y: tab_bar_height, width: split_x, height: content_h },
        });
        // Bake scroll: content shifts left by scroll_x, up by scroll_y,
        // and down by tab_bar_height (so y=0 in content-space = top of visible area).
        out.push(DisplayCommand::PushTransform {
            matrix: Mat4::translation_2d(-left_scroll_x, tab_bar_height - left_scroll_y),
        });
        out.extend_from_slice(left_dl);
        out.push(DisplayCommand::PopTransform);
        out.push(DisplayCommand::PopClip);

        // ── Divider ────────────────────────────────────────────────────────
        out.push(DisplayCommand::FillRect {
            rect: Rect { x: split_x, y: tab_bar_height, width: 1.0, height: content_h },
            color: Color { r: 55, g: 55, b: 65, a: 255 },
        });

        // ── Right pane ─────────────────────────────────────────────────────
        out.push(DisplayCommand::PushClipRect {
            rect: Rect { x: right_x, y: tab_bar_height, width: right_w, height: content_h },
        });
        // Origin for the right pane's content is at (right_x, tab_bar_height)
        // in window coords; scroll shifts it up.
        out.push(DisplayCommand::PushTransform {
            matrix: Mat4::translation_2d(
                right_x - self.right.scroll_x,
                tab_bar_height - self.right.scroll_y,
            ),
        });
        out.extend_from_slice(&self.right.display_list);
        out.push(DisplayCommand::PopTransform);
        out.push(DisplayCommand::PopClip);

        out
    }

    /// Return `true` if `window_x` (CSS px) falls inside the right pane.
    pub fn cursor_in_right(&self, window_x: f32, split_x: f32) -> bool {
        window_x > split_x + 1.0
    }

    /// Map a window-space x coord to right-pane content x (accounts for scroll).
    #[allow(dead_code)]
    pub fn right_content_x(&self, window_x: f32, split_x: f32) -> f32 {
        window_x - split_x - 1.0 + self.right.scroll_x
    }

    /// Map a window-space y coord to right-pane content y (accounts for scroll).
    #[allow(dead_code)]
    pub fn right_content_y(&self, window_y: f32, tab_bar_height: f32) -> f32 {
        window_y - tab_bar_height + self.right.scroll_y
    }

    /// Toggle keyboard/scroll focus between left and right pane.
    pub fn toggle_focus(&mut self) {
        self.focused = match self.focused {
            SplitFocus::Left => SplitFocus::Right,
            SplitFocus::Right => SplitFocus::Left,
        };
    }

    /// Transfer focus to whichever pane contains `window_x`.
    pub fn focus_at(&mut self, window_x: f32, split_x: f32) {
        self.focused = if self.cursor_in_right(window_x, split_x) {
            SplitFocus::Right
        } else {
            SplitFocus::Left
        };
    }

    /// Scroll the focused pane by `dy` CSS px (clamped to content bounds).
    #[allow(dead_code)]
    pub fn scroll_focused_by(
        &mut self,
        dy: f32,
        left_scroll_y: &mut f32,
        left_content_height: f32,
        viewport_content_height: f32,
    ) {
        match self.focused {
            SplitFocus::Left => {
                let max = (left_content_height - viewport_content_height).max(0.0);
                *left_scroll_y = (*left_scroll_y + dy).clamp(0.0, max);
            }
            SplitFocus::Right => {
                let max =
                    (self.right.content_height - viewport_content_height).max(0.0);
                self.right.scroll_y = (self.right.scroll_y + dy).clamp(0.0, max);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_sv() -> SplitView {
        SplitView::new(42, vec![], 0.0, 0.0, 800.0, 1024.0)
    }

    #[test]
    fn default_focus_is_left() {
        assert_eq!(blank_sv().focused, SplitFocus::Left);
    }

    #[test]
    fn toggle_focus_left_right_left() {
        let mut sv = blank_sv();
        sv.toggle_focus();
        assert_eq!(sv.focused, SplitFocus::Right);
        sv.toggle_focus();
        assert_eq!(sv.focused, SplitFocus::Left);
    }

    #[test]
    fn focus_at_routes_by_x() {
        let mut sv = blank_sv();
        sv.focus_at(600.0, 512.0); // 600 > 513 → right
        assert_eq!(sv.focused, SplitFocus::Right);
        sv.focus_at(400.0, 512.0); // 400 < 513 → left
        assert_eq!(sv.focused, SplitFocus::Left);
    }

    #[test]
    fn cursor_in_right_boundary() {
        let sv = blank_sv();
        assert!(!sv.cursor_in_right(513.0, 512.0)); // exactly on divider → not right
        assert!(sv.cursor_in_right(514.0, 512.0));
    }

    #[test]
    fn right_content_x_accounts_for_scroll() {
        let mut sv = blank_sv();
        sv.right.scroll_x = 20.0;
        // window_x=633, split_x=512 → 633 - 512 - 1 + 20 = 140
        let got = sv.right_content_x(633.0, 512.0);
        assert!((got - 140.0).abs() < f32::EPSILON, "got {got}");
    }

    #[test]
    fn right_content_y_accounts_for_scroll() {
        let mut sv = blank_sv();
        sv.right.scroll_y = 50.0;
        // window_y=136, tab_bar=36 → 136 - 36 + 50 = 150
        let got = sv.right_content_y(136.0, 36.0);
        assert!((got - 150.0).abs() < f32::EPSILON, "got {got}");
    }

    #[test]
    fn build_combined_dl_structure() {
        let left_dl = vec![DisplayCommand::FillRect {
            rect: Rect { x: 0.0, y: 0.0, width: 50.0, height: 30.0 },
            color: Color { r: 255, g: 0, b: 0, a: 255 },
        }];
        let sv = SplitView::new(1, vec![], 0.0, 0.0, 720.0, 512.0);
        let combined = sv.build_combined_dl(&left_dl, 0.0, 0.0, 512.0, 36.0, 720.0);
        // Left: PushClipRect PushTransform FillRect PopTransform PopClip (5)
        // Divider: FillRect (1)
        // Right: PushClipRect PushTransform PopTransform PopClip (4, right_dl empty)
        assert_eq!(combined.len(), 10);
        assert!(matches!(combined[0], DisplayCommand::PushClipRect { .. }));
        assert!(matches!(combined[4], DisplayCommand::PopClip));
        assert!(matches!(combined[5], DisplayCommand::FillRect { .. })); // divider
        assert!(matches!(combined[6], DisplayCommand::PushClipRect { .. })); // right pane
    }

    #[test]
    fn scroll_focused_clamps_left() {
        let mut sv = blank_sv();
        sv.focused = SplitFocus::Left;
        let mut scroll_y = 0.0f32;
        sv.scroll_focused_by(999.0, &mut scroll_y, 600.0, 684.0); // viewport_content_h=684
        assert_eq!(scroll_y, 0.0); // 600 - 684 < 0 → max=0, clamp to 0
    }

    #[test]
    fn scroll_focused_right_pane() {
        let mut sv = SplitView::new(1, vec![], 0.0, 0.0, 1200.0, 512.0);
        sv.focused = SplitFocus::Right;
        let mut dummy_left = 0.0f32;
        sv.scroll_focused_by(200.0, &mut dummy_left, 1000.0, 684.0);
        // max = 1200 - 684 = 516; 0 + 200 = 200 ≤ 516
        assert!((sv.right.scroll_y - 200.0).abs() < f32::EPSILON);
    }
}
