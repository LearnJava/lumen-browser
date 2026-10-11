//! Vertical tabs layout state (7A.1): the sidebar tab list is `#sbTabs` of the
//! engine chrome (`bind_model`, UX-CHROME-PANELS 14); this module keeps only
//! what the shell has to remember about it — whether the vertical layout is
//! on (`Ctrl+B`) and how far the list is scrolled.
//!
//! The chrome has no wheel-scrollable container, so the list is windowed the
//! way the other chrome panels are: `chrome_model_snapshot` skips
//! `scroll_rows` leading rows and the sidebar clips whatever does not fit.

/// Rows that stay on screen at the bottom of the scroll range. The sidebar's
/// real capacity depends on the window height and the profile/workspace
/// blocks above the list; six rows fit in any window the chrome supports, so
/// the last tabs can never be scrolled out of reach.
pub const MIN_VISIBLE_ROWS: usize = 6;

/// Vertical tabs layout state.
pub struct VerticalTabsPanel {
    /// `true` while the sidebar (vertical) tab layout is on. Toggled via
    /// Ctrl+B or the Appearance setting.
    pub visible: bool,
    /// Rows scrolled off the top of the sidebar tab list.
    pub scroll_rows: usize,
}

impl VerticalTabsPanel {
    /// Create a new (horizontal-layout) state.
    pub fn new() -> Self {
        Self { visible: false, scroll_rows: 0 }
    }

    /// Flip visibility. Caller must trigger relayout + redraw.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Largest useful offset for a list of `row_count` rows.
    pub fn max_scroll_rows(row_count: usize) -> usize {
        row_count.saturating_sub(MIN_VISIBLE_ROWS)
    }

    /// Scroll by `delta` rows (positive = down), clamped to the list.
    pub fn scroll_by(&mut self, delta: i32, row_count: usize) {
        let next = (self.scroll_rows as i64 + i64::from(delta))
            .clamp(0, Self::max_scroll_rows(row_count) as i64);
        self.scroll_rows = next as usize;
    }
}

impl Default for VerticalTabsPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_panel_is_hidden() {
        assert!(!VerticalTabsPanel::new().visible);
    }

    #[test]
    fn double_toggle_hides_again() {
        let mut p = VerticalTabsPanel::new();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn scroll_by_clamps_to_list() {
        let mut p = VerticalTabsPanel::new();
        p.scroll_by(999, 10);
        assert_eq!(p.scroll_rows, 10 - MIN_VISIBLE_ROWS);
        p.scroll_by(-999, 10);
        assert_eq!(p.scroll_rows, 0);
    }

    #[test]
    fn short_list_does_not_scroll() {
        let mut p = VerticalTabsPanel::new();
        p.scroll_by(3, MIN_VISIBLE_ROWS);
        assert_eq!(p.scroll_rows, 0);
    }
}
