//! Tab strip: per-tab metadata and rendering.
//!
//! `TabStrip` holds the list of open tabs and the active index.
//! `build_tab_bar` produces a viewport-locked `DisplayList` for the strip area.
//! `hit_test` maps CSS-px (x, y) → `TabHit` for mouse dispatch.
//!
//! Visual constants follow a dark-chrome aesthetic consistent with
//! `address_bar.rs` and `find.rs`.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::tab_lifecycle::state::TabState;

// ── Visual constants ──────────────────────────────────────────────────────────

/// Height of the tab bar in CSS px. Subtracted from `viewport_height_css()`.
pub const TAB_BAR_HEIGHT: f32 = 36.0;

const BAR_BG: Color = Color { r: 22, g: 22, b: 26, a: 255 };
const TAB_INACTIVE_BG: Color = Color { r: 32, g: 33, b: 36, a: 255 };
const TAB_ACTIVE_BG: Color = Color { r: 18, g: 18, b: 22, a: 255 };
const TAB_ACTIVE_ACCENT: Color = Color { r: 100, g: 160, b: 255, a: 255 };
const TAB_TEXT: Color = Color { r: 218, g: 218, b: 228, a: 255 };
const TAB_TEXT_DIM: Color = Color { r: 140, g: 140, b: 148, a: 255 };
const CLOSE_FG: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const DIVIDER: Color = Color { r: 45, g: 46, b: 52, a: 255 };

/// Badge colour for BackgroundOld tier — amber moon indicator.
const BADGE_OLD_COLOR: Color = Color { r: 255, g: 168, b: 0, a: 210 };
/// Badge colour for Hibernated tier — grey disk indicator.
const BADGE_HIBERNATE_COLOR: Color = Color { r: 110, g: 110, b: 120, a: 210 };

const FONT_SZ: f32 = 12.0;
/// Minimum tab button width in CSS px.
const TAB_MIN_W: f32 = 80.0;
/// Maximum tab button width in CSS px.
const TAB_MAX_W: f32 = 200.0;
/// Horizontal padding inside a tab (text from left edge).
const TAB_PAD: f32 = 10.0;
/// Close-button glyph size.
const CLOSE_SZ: f32 = 14.0;
/// Gap between text area right edge and close-button left edge.
const CLOSE_MARGIN: f32 = 4.0;
/// Badge dot diameter in CSS px.
const BADGE_SIZE: f32 = 5.0;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Metadata for one browser tab.
pub struct TabEntry {
    /// Stable unique identifier, never reused within a session.
    pub id: usize,
    /// Display title shown in the tab button.
    pub title: String,
    /// Current lifecycle tier for this tab.
    ///
    /// `Active` — foreground tab, no badge rendered.
    /// `BackgroundOld` — amber badge (moon): JS heap off-loaded to disk.
    /// `Hibernated` — grey badge (disk): DOM snapshot on disk, minimal RAM.
    /// Other tiers — no badge rendered.
    pub tab_state: TabState,
    /// ID of the tab that opened this one, or `None` for root (top-level) tabs.
    ///
    /// Forms the parent-child tree used by tree-style tabs (7A.2).
    /// Depth is computed by walking this chain upward. Cycles are impossible
    /// because `opener_id` is set once at creation and always points to an
    /// already-existing tab.
    pub opener_id: Option<usize>,
}

/// State of the tab strip (tab list + active index).
pub struct TabStrip {
    /// Open tabs, in left-to-right order.
    pub tabs: Vec<TabEntry>,
    /// Index of the currently-visible tab.
    pub active: usize,
    /// Counter for generating fresh `TabEntry::id` values.
    pub(crate) next_id: usize,
}

impl TabStrip {
    /// Create the initial tab strip with one blank tab.
    pub fn new() -> Self {
        Self {
            tabs: vec![TabEntry {
                id: 0,
                title: "Новая вкладка".to_owned(),
                tab_state: TabState::Active,
                opener_id: None,
            }],
            active: 0,
            next_id: 1,
        }
    }

    /// Number of open tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Append a new blank tab and return its index.
    pub fn push_blank(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            title: "Новая вкладка".to_owned(),
            tab_state: TabState::Active,
            opener_id: None,
        });
        self.tabs.len() - 1
    }

    /// Append a new blank child tab opened by the tab with `opener_id`.
    ///
    /// Sets `TabEntry::opener_id` so tree-style tab rendering can indent and
    /// group this tab under its parent. Returns the new tab's strip index.
    pub fn push_with_opener(&mut self, opener_id: usize) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            title: "Новая вкладка".to_owned(),
            tab_state: TabState::Active,
            opener_id: Some(opener_id),
        });
        self.tabs.len() - 1
    }

    /// Remove the tab at `idx`. Returns the new active index (clamped to valid
    /// range). Caller must guard against removing the only tab (check `len() > 1`).
    pub fn remove(&mut self, idx: usize) -> usize {
        self.tabs.remove(idx);
        let new_active = if self.active >= self.tabs.len() {
            self.tabs.len().saturating_sub(1)
        } else {
            self.active
        };
        self.active = new_active;
        new_active
    }

    /// Update the title of the active tab.
    pub fn set_active_title(&mut self, title: impl Into<String>) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.title = title.into();
        }
    }

    /// Update the lifecycle state of the tab at `idx`.
    ///
    /// Called by the shell on tab switch (`Active` ↔ `BackgroundRecent`) and by
    /// the lifecycle manager on idle-timeout or memory-pressure transitions.
    pub fn set_tab_state(&mut self, idx: usize, state: TabState) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.tab_state = state;
        }
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of clicking inside the tab bar area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHit {
    /// Clicked the tab body (not close button) — `idx` = tab index.
    Tab(usize),
    /// Clicked the close ×  button — `idx` = tab index.
    Close(usize),
    /// Clicked empty area (right of all tabs).
    Empty,
}

/// Returns the `[left, right)` x-range of tab `idx` given `n_tabs` tabs and
/// a `window_w`-wide window.
fn tab_x_range(idx: usize, n_tabs: usize, window_w: f32) -> (f32, f32) {
    let tab_w = (window_w / n_tabs as f32).clamp(TAB_MIN_W, TAB_MAX_W);
    let left = idx as f32 * tab_w;
    (left, left + tab_w)
}

/// Hit-test a click at CSS-px `(x, y)` against the tab bar.
///
/// Returns `TabHit::Empty` if `y >= TAB_BAR_HEIGHT` (below the strip).
pub fn hit_test(strip: &TabStrip, x: f32, y: f32, window_w: f32) -> TabHit {
    if !(0.0..TAB_BAR_HEIGHT).contains(&y) {
        return TabHit::Empty;
    }
    let n = strip.tabs.len();
    for i in 0..n {
        let (left, right) = tab_x_range(i, n, window_w);
        if x >= left && x < right {
            // Close-button occupies the rightmost CLOSE_SZ + CLOSE_MARGIN px.
            let close_right = right - TAB_PAD;
            let close_left = close_right - CLOSE_SZ;
            if x >= close_left && x < close_right {
                return TabHit::Close(i);
            }
            return TabHit::Tab(i);
        }
    }
    TabHit::Empty
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build a viewport-locked display list for the tab bar.
///
/// Appended to the overlay buffer each frame; rendered on top of page content
/// at y = 0..`TAB_BAR_HEIGHT`.
///
/// Lifecycle badge rendering:
/// - `TabState::BackgroundOld` → amber dot at top-right corner of the tab button.
/// - `TabState::Hibernated`    → grey dot at top-right corner of the tab button.
/// - All other states          → no badge rendered.
pub fn build_tab_bar(strip: &TabStrip, window_w: f32) -> DisplayList {
    let n = strip.tabs.len();
    let mut out = DisplayList::with_capacity(4 + n * 6);

    // Background strip.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, window_w, TAB_BAR_HEIGHT),
        color: BAR_BG,
    });

    for (i, tab) in strip.tabs.iter().enumerate() {
        let (left, right) = tab_x_range(i, n, window_w);
        let is_active = i == strip.active;

        // Tab background.
        let bg = if is_active { TAB_ACTIVE_BG } else { TAB_INACTIVE_BG };
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(left, 0.0, right - left, TAB_BAR_HEIGHT),
            color: bg,
        });

        // Active tab accent bar at the bottom.
        if is_active {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(left, TAB_BAR_HEIGHT - 2.0, right - left, 2.0),
                color: TAB_ACTIVE_ACCENT,
            });
        }

        // Tab right divider (skip last tab).
        if i + 1 < n {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(right - 1.0, 4.0, 1.0, TAB_BAR_HEIGHT - 8.0),
                color: DIVIDER,
            });
        }

        // Lifecycle badge — small coloured circle at top-right corner.
        // BackgroundOld → amber (moon); Hibernated → grey (disk). Other states: no badge.
        let badge_color = match tab.tab_state {
            TabState::BackgroundOld => Some(BADGE_OLD_COLOR),
            TabState::Hibernated => Some(BADGE_HIBERNATE_COLOR),
            _ => None,
        };
        if let Some(color) = badge_color {
            // Position: top-right of the tab, inset 3px from right edge and 4px from top.
            let bx = right - BADGE_SIZE - 3.0;
            let by = 4.0;
            let r = BADGE_SIZE / 2.0;
            out.push(DisplayCommand::FillRoundedRect {
                rect: Rect::new(bx, by, BADGE_SIZE, BADGE_SIZE),
                radii: CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r },
                color,
            });
        }

        // Close button — ×
        let close_right = right - TAB_PAD;
        let close_left = close_right - CLOSE_SZ;
        let close_cy = (TAB_BAR_HEIGHT - CLOSE_SZ * 1.2) * 0.5;
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(close_left, close_cy, CLOSE_SZ, CLOSE_SZ * 1.2),
            text: "×".to_owned(),
            font_size: CLOSE_SZ,
            color: CLOSE_FG,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        // Tab title — truncated to fit between left edge and close button.
        let text_x = left + TAB_PAD;
        let text_w = (close_left - CLOSE_MARGIN - text_x).max(0.0);
        let text_y = (TAB_BAR_HEIGHT - FONT_SZ * 1.3) * 0.5;
        let text_color = if is_active { TAB_TEXT } else { TAB_TEXT_DIM };
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(text_x, text_y, text_w, FONT_SZ * 1.3),
            text: tab.title.clone(),
            font_size: FONT_SZ,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });
    }

    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_strip_has_one_tab() {
        let s = TabStrip::new();
        assert_eq!(s.len(), 1);
        assert_eq!(s.active, 0);
    }

    #[test]
    fn new_tab_starts_active() {
        let s = TabStrip::new();
        assert_eq!(s.tabs[0].tab_state, TabState::Active);
    }

    #[test]
    fn push_blank_increments_len() {
        let mut s = TabStrip::new();
        let idx = s.push_blank();
        assert_eq!(idx, 1);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn push_blank_starts_active_state() {
        let mut s = TabStrip::new();
        s.push_blank();
        assert_eq!(s.tabs[1].tab_state, TabState::Active);
    }

    #[test]
    fn remove_tab_clamps_active() {
        let mut s = TabStrip::new();
        s.push_blank();
        s.push_blank();
        s.active = 2;
        let new_active = s.remove(2);
        assert_eq!(s.len(), 2);
        assert_eq!(new_active, 1);
    }

    #[test]
    fn set_active_title_updates() {
        let mut s = TabStrip::new();
        s.set_active_title("Rust Lang");
        assert_eq!(s.tabs[0].title, "Rust Lang");
    }

    #[test]
    fn set_tab_state_updates_entry() {
        let mut s = TabStrip::new();
        s.push_blank();
        s.set_tab_state(0, TabState::BackgroundOld);
        assert_eq!(s.tabs[0].tab_state, TabState::BackgroundOld);
        assert_eq!(s.tabs[1].tab_state, TabState::Active);
    }

    #[test]
    fn set_tab_state_out_of_bounds_no_panic() {
        let mut s = TabStrip::new();
        s.set_tab_state(99, TabState::Hibernated); // must not panic
    }

    #[test]
    fn hit_test_tab_body() {
        let mut s = TabStrip::new();
        s.push_blank();
        // Two tabs, each 512px wide in a 1024px window.
        // Click in the middle of the first tab, away from close button.
        let hit = hit_test(&s, 100.0, 18.0, 1024.0);
        assert_eq!(hit, TabHit::Tab(0));
    }

    #[test]
    fn hit_test_close_button() {
        let s = TabStrip::new();
        // Single tab: tab_w = clamp(1024/1, 80, 200) = 200, so tab occupies [0, 200).
        // Close button: close_right = 200 - 10 = 190, close_left = 190 - 14 = 176.
        // → button at [176, 190); click at 182 should hit it.
        let hit = hit_test(&s, 182.0, 18.0, 1024.0);
        assert_eq!(hit, TabHit::Close(0));
    }

    #[test]
    fn hit_test_below_bar_returns_empty() {
        let s = TabStrip::new();
        let hit = hit_test(&s, 100.0, TAB_BAR_HEIGHT + 1.0, 1024.0);
        assert_eq!(hit, TabHit::Empty);
    }

    #[test]
    fn build_tab_bar_emits_commands() {
        let s = TabStrip::new();
        let dl = build_tab_bar(&s, 1024.0);
        assert!(!dl.is_empty());
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("вкладка"))
        });
        assert!(has_title);
    }

    #[test]
    fn build_tab_bar_no_badge_for_active() {
        let s = TabStrip::new(); // single Active tab
        let dl = build_tab_bar(&s, 1024.0);
        // Active tab must not emit any FillRoundedRect (all tab bar rects are FillRect).
        let has_rounded = dl.iter().any(|c| matches!(c, DisplayCommand::FillRoundedRect { .. }));
        assert!(!has_rounded, "Active tab must not render a lifecycle badge");
    }

    #[test]
    fn build_tab_bar_badge_for_background_old() {
        let mut s = TabStrip::new();
        s.push_blank();
        s.set_tab_state(0, TabState::BackgroundOld);
        let dl = build_tab_bar(&s, 1024.0);
        // Amber badge: r=255, g=168
        let has_amber = dl.iter().any(|c| match c {
            DisplayCommand::FillRoundedRect { color, .. } => {
                color.r == BADGE_OLD_COLOR.r && color.g == BADGE_OLD_COLOR.g
            }
            _ => false,
        });
        assert!(has_amber, "BackgroundOld tab must render amber badge");
    }

    #[test]
    fn build_tab_bar_badge_for_hibernated() {
        let mut s = TabStrip::new();
        s.push_blank();
        s.set_tab_state(0, TabState::Hibernated);
        let dl = build_tab_bar(&s, 1024.0);
        // Grey badge: r=110, g=110
        let has_grey = dl.iter().any(|c| match c {
            DisplayCommand::FillRoundedRect { color, .. } => {
                color.r == BADGE_HIBERNATE_COLOR.r && color.g == BADGE_HIBERNATE_COLOR.g
            }
            _ => false,
        });
        assert!(has_grey, "Hibernated tab must render grey badge");
    }

}
