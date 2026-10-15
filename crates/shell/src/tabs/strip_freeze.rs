//! UX-TAB-CLOSE-FREEZE: width freeze of the horizontal tab strip.
//!
//! The strip lays its tabs out with equal widths, so closing one with the
//! mouse makes every remaining tab wider and moves the next × away from the
//! cursor. While the user keeps closing tabs in the same spot the width is
//! pinned to what the closed tab had; the pin lasts until the cursor leaves
//! the strip's zone, a tab is added, or the window is resized.

/// Extra margin around the strip inside which the cursor keeps the freeze
/// alive, CSS px (vertical / horizontal).
const ZONE_PAD_Y: f32 = 40.0;
const ZONE_PAD_X: f32 = 60.0;

/// An active width freeze.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HbarFreeze {
    /// Pinned width of every tab, CSS px.
    pub width: f32,
    /// Smallest tab count seen since the freeze began; a larger count means
    /// a tab was opened.
    tab_count: usize,
    /// Viewport width the freeze was taken at; a different width is a resize.
    viewport_w: f32,
}

impl HbarFreeze {
    /// Starts a freeze at `width` px for a strip of `tab_count` tabs (counted
    /// *before* the close) in a `viewport_w`-wide window.
    pub fn new(width: f32, tab_count: usize, viewport_w: f32) -> Self {
        Self { width, tab_count, viewport_w }
    }

    /// Records the current tab count and reports whether the freeze still
    /// holds: `false` after a tab was opened or the window was resized.
    pub fn keep(&mut self, tab_count: usize, viewport_w: f32) -> bool {
        if tab_count > self.tab_count || (viewport_w - self.viewport_w).abs() > 0.5 {
            return false;
        }
        self.tab_count = tab_count;
        true
    }
}

/// `true` when `(x, y)` lies inside the strip rect `(sx, sy, sw, sh)` grown by
/// 60 px horizontally and 40 px vertically.
pub fn in_freeze_zone(strip: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    let (sx, sy, sw, sh) = strip;
    x >= sx - ZONE_PAD_X && x <= sx + sw + ZONE_PAD_X && y >= sy - ZONE_PAD_Y && y <= sy + sh + ZONE_PAD_Y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_tabs_keeps_the_freeze() {
        let mut f = HbarFreeze::new(120.0, 5, 1000.0);
        assert!(f.keep(4, 1000.0));
        assert!(f.keep(3, 1000.0));
    }

    #[test]
    fn opening_a_tab_after_a_close_drops_the_freeze() {
        let mut f = HbarFreeze::new(120.0, 5, 1000.0);
        assert!(f.keep(4, 1000.0));
        assert!(!f.keep(5, 1000.0));
    }

    #[test]
    fn resize_drops_the_freeze() {
        let mut f = HbarFreeze::new(120.0, 5, 1000.0);
        assert!(!f.keep(5, 900.0));
    }

    #[test]
    fn zone_is_the_strip_grown_by_the_margins() {
        let strip = (100.0, 10.0, 500.0, 30.0);
        assert!(in_freeze_zone(strip, 40.0, 80.0));
        assert!(in_freeze_zone(strip, 660.0, -30.0));
        assert!(!in_freeze_zone(strip, 39.0, 20.0));
        assert!(!in_freeze_zone(strip, 661.0, 20.0));
        assert!(!in_freeze_zone(strip, 300.0, 81.0));
        assert!(!in_freeze_zone(strip, 300.0, -31.0));
    }
}
