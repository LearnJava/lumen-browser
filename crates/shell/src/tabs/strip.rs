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
use crate::tabs::containers::ContainerKind;

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

/// Badge colour for BackgroundOld tier — amber "z" sleep icon.
const BADGE_OLD_COLOR: Color = Color { r: 255, g: 168, b: 0, a: 210 };
/// Badge colour for Hibernated tier — grey "Z" sleep icon.
const BADGE_HIBERNATE_COLOR: Color = Color { r: 110, g: 110, b: 120, a: 210 };
/// Dimmed background for BackgroundOld (T2) tabs — signals reduced activity.
const TAB_T2_BG: Color = Color { r: 26, g: 27, b: 30, a: 255 };
/// Dimmed background for Hibernated (T3) tabs — signals deep sleep.
const TAB_T3_BG: Color = Color { r: 21, g: 21, b: 24, a: 255 };

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
/// Font size for the "Z"/"z" sleep-icon badge on T2/T3 tabs.
const BADGE_Z_SZ: f32 = 9.0;
/// Height of the container border-top strip in CSS px (7D.2). Drawn at the
/// very top of each tab button when its `container` is not `ContainerKind::None`.
const CONTAINER_STRIP_HEIGHT: f32 = 3.0;

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
    /// `BackgroundOld` — amber "z" badge + dimmed background (fade-opacity T2).
    /// `Hibernated` — grey "Z" badge + darker background (fade-opacity T3).
    /// Other tiers — no badge rendered.
    pub tab_state: TabState,
    /// ID of the tab that opened this one, or `None` for root (top-level) tabs.
    ///
    /// Forms the parent-child tree used by tree-style tabs (7A.2).
    /// Depth is computed by walking this chain upward. Cycles are impossible
    /// because `opener_id` is set once at creation and always points to an
    /// already-existing tab.
    pub opener_id: Option<usize>,
    /// Container assigned to this tab (7D.2). Drives the border-top strip
    /// rendered above the tab and the cookie/storage isolation key.
    ///
    /// Default `ContainerKind::None` — no container, shared state. New
    /// tabs inherit `None`; the user changes containers via the shell's
    /// `set_tab_container` API.
    pub container: ContainerKind,
    /// Session-elapsed milliseconds when this tab was last made active.
    ///
    /// Set to `now_ms` on tab creation and on every activation via
    /// `update_last_activated`. The auto-archive tick (7A.5) compares this
    /// against `ARCHIVE_AFTER_MS` to decide whether a background tab should
    /// be moved to [`crate::tabs::archive::TabArchive`].
    pub last_activated_ms: f64,
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
                container: ContainerKind::None,
                last_activated_ms: 0.0,
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
    ///
    /// `now_ms` — current session-elapsed milliseconds, stored as
    /// `last_activated_ms` so the auto-archive timer starts from creation time.
    pub fn push_blank(&mut self, now_ms: f64) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            title: "Новая вкладка".to_owned(),
            tab_state: TabState::Active,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: now_ms,
        });
        self.tabs.len() - 1
    }

    /// Append a new blank child tab opened by the tab with `opener_id`.
    ///
    /// Sets `TabEntry::opener_id` so tree-style tab rendering can indent and
    /// group this tab under its parent. Returns the new tab's strip index.
    ///
    /// `now_ms` — current session-elapsed milliseconds (same semantics as
    /// [`push_blank`]).
    pub fn push_with_opener(&mut self, opener_id: usize, now_ms: f64) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            title: "Новая вкладка".to_owned(),
            tab_state: TabState::Active,
            opener_id: Some(opener_id),
            container: ContainerKind::None,
            last_activated_ms: now_ms,
        });
        self.tabs.len() - 1
    }

    /// Record `now_ms` as the activation timestamp for the tab at `idx`.
    ///
    /// Call on every tab switch so the auto-archive timer resets for the
    /// newly-active tab and advances for all background tabs.
    pub fn update_last_activated(&mut self, idx: usize, now_ms: f64) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.last_activated_ms = now_ms;
        }
    }

    /// Assign `container` to the tab at `idx`. Out-of-bounds index is a no-op.
    ///
    /// Triggers a visual change on the next `build_tab_bar` call — the
    /// border-top strip swaps colour or appears/disappears. Cookie/storage
    /// isolation rewiring is the caller's responsibility (see
    /// `ContainerStore::get_or_create`).
    pub fn set_tab_container(&mut self, idx: usize, container: ContainerKind) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.container = container;
        }
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

        // Tab background: T2/T3 use darker backgrounds as fade-opacity signal.
        let bg = if is_active {
            TAB_ACTIVE_BG
        } else {
            match tab.tab_state {
                TabState::BackgroundOld => TAB_T2_BG,
                TabState::Hibernated => TAB_T3_BG,
                _ => TAB_INACTIVE_BG,
            }
        };
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

        // Container border-top strip (7D.2). 3 px tall coloured bar at the
        // very top edge of the tab. Skipped for ContainerKind::None.
        if let Some(color) = tab.container.border_color() {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(left, 0.0, right - left, CONTAINER_STRIP_HEIGHT),
                color,
            });
        }

        // Tab right divider (skip last tab).
        if i + 1 < n {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(right - 1.0, 4.0, 1.0, TAB_BAR_HEIGHT - 8.0),
                color: DIVIDER,
            });
        }

        // Lifecycle badge — "Z" glyph at top-right corner (sleep icon).
        // BackgroundOld → amber lowercase "z"; Hibernated → grey uppercase "Z".
        let badge_info: Option<(&str, Color)> = match tab.tab_state {
            TabState::BackgroundOld => Some(("z", BADGE_OLD_COLOR)),
            TabState::Hibernated => Some(("Z", BADGE_HIBERNATE_COLOR)),
            _ => None,
        };
        if let Some((glyph, color)) = badge_info {
            // Position: top-right of the tab, inset 3px from right edge, 3px from top.
            let bx = right - BADGE_Z_SZ - 3.0;
            let by = 3.0;
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(bx, by, BADGE_Z_SZ, BADGE_Z_SZ * 1.2),
                text: glyph.to_owned(),
                font_size: BADGE_Z_SZ,
                color,
                font_family: Vec::new(),
                font_weight: FontWeight::BOLD,
                font_style: FontStyle::Italic,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
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

/// Build a small tooltip overlay for a tab with a non-Active tier badge.
///
/// Returns `None` if the hovered tab has no tier badge (Active / BackgroundRecent).
/// Tooltip displays above the tab bar with context about the tab state.
pub fn build_tab_tooltip(
    tab: &TabEntry,
    tab_center_x: f32,
    tab_bar_bottom: f32,
) -> Option<DisplayList> {
    let msg = match tab.tab_state {
        TabState::BackgroundOld => "Вкладка фоновая — потребляет меньше памяти",
        TabState::Hibernated => "Вкладка спит — клик восстановит (~1 сек)",
        _ => return None,
    };

    const TT_W: f32 = 240.0;
    const TT_H: f32 = 28.0;
    const PAD: f32 = 8.0;
    const RADIUS: f32 = 4.0;
    const FONT_SZ: f32 = 11.0;

    let x = (tab_center_x - TT_W / 2.0).max(4.0);
    let y = tab_bar_bottom + 4.0;

    let bg = Color { r: 38, g: 38, b: 42, a: 235 };
    let text_color = Color { r: 255, g: 255, b: 255, a: 255 };

    Some(vec![
        DisplayCommand::FillRoundedRect {
            rect: Rect::new(x, y, TT_W, TT_H),
            radii: CornerRadii { tl: RADIUS, tl_y: RADIUS, tr: RADIUS, tr_y: RADIUS, br: RADIUS, br_y: RADIUS, bl: RADIUS, bl_y: RADIUS },
            color: bg,
        },
        DisplayCommand::DrawText {
            rect: Rect::new(x + PAD, y + TT_H / 2.0 - FONT_SZ * 0.4, TT_W - 2.0 * PAD, FONT_SZ * 1.2),
            text: msg.to_string(),
            font_size: FONT_SZ,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        },
    ])
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
        let idx = s.push_blank(0.0);
        assert_eq!(idx, 1);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn push_blank_starts_active_state() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        assert_eq!(s.tabs[1].tab_state, TabState::Active);
    }

    #[test]
    fn remove_tab_clamps_active() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.push_blank(0.0);
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
        s.push_blank(0.0);
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
        s.push_blank(0.0);
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
        // Active tab must not emit a sleep-icon badge (no "Z"/"z" glyph).
        let has_sleep_badge = dl.iter().any(|c| match c {
            DisplayCommand::DrawText { text, .. } => text == "Z" || text == "z",
            _ => false,
        });
        assert!(!has_sleep_badge, "Active tab must not render a sleep badge");
    }

    #[test]
    fn build_tab_bar_badge_for_background_old() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.set_tab_state(0, TabState::BackgroundOld);
        let dl = build_tab_bar(&s, 1024.0);
        // Amber "z" glyph badge for BackgroundOld tier.
        let has_z = dl.iter().any(|c| match c {
            DisplayCommand::DrawText { text, color, .. } => {
                text == "z" && color.r == BADGE_OLD_COLOR.r && color.g == BADGE_OLD_COLOR.g
            }
            _ => false,
        });
        assert!(has_z, "BackgroundOld tab must render amber 'z' badge");
    }

    #[test]
    fn build_tab_bar_badge_for_hibernated() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.set_tab_state(0, TabState::Hibernated);
        let dl = build_tab_bar(&s, 1024.0);
        // Grey "Z" glyph badge for Hibernated tier.
        let has_z = dl.iter().any(|c| match c {
            DisplayCommand::DrawText { text, color, .. } => {
                text == "Z" && color.r == BADGE_HIBERNATE_COLOR.r && color.g == BADGE_HIBERNATE_COLOR.g
            }
            _ => false,
        });
        assert!(has_z, "Hibernated tab must render grey 'Z' badge");
    }

    #[test]
    fn build_tab_bar_fade_bg_for_background_old() {
        let mut s = TabStrip::new();
        s.push_blank(0.0); // index 0 — active
        s.push_blank(0.0); // index 1 — inactive BackgroundOld
        s.set_tab_state(1, TabState::BackgroundOld);
        let dl = build_tab_bar(&s, 1024.0);
        // T2 background must be TAB_T2_BG, not TAB_INACTIVE_BG.
        let has_t2_bg = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { color, .. } => *color == TAB_T2_BG,
            _ => false,
        });
        assert!(has_t2_bg, "BackgroundOld inactive tab must use dimmed T2 background");
    }

    #[test]
    fn build_tab_bar_fade_bg_for_hibernated() {
        let mut s = TabStrip::new();
        s.push_blank(0.0); // index 0 — active
        s.push_blank(0.0); // index 1 — inactive Hibernated
        s.set_tab_state(1, TabState::Hibernated);
        let dl = build_tab_bar(&s, 1024.0);
        // T3 background must be TAB_T3_BG.
        let has_t3_bg = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { color, .. } => *color == TAB_T3_BG,
            _ => false,
        });
        assert!(has_t3_bg, "Hibernated inactive tab must use dimmed T3 background");
    }

    // ── Container strip tests (7D.2) ─────────────────────────────────────────

    #[test]
    fn new_tab_has_no_container() {
        let s = TabStrip::new();
        assert_eq!(s.tabs[0].container, ContainerKind::None);
    }

    #[test]
    fn push_blank_starts_without_container() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        assert_eq!(s.tabs[1].container, ContainerKind::None);
    }

    #[test]
    fn push_with_opener_starts_without_container() {
        let mut s = TabStrip::new();
        let opener_id = s.tabs[0].id;
        s.push_with_opener(opener_id, 0.0);
        assert_eq!(s.tabs[1].container, ContainerKind::None);
    }

    #[test]
    fn set_tab_container_updates_entry() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Work);
        assert_eq!(s.tabs[0].container, ContainerKind::Work);
    }

    #[test]
    fn set_tab_container_out_of_bounds_no_panic() {
        let mut s = TabStrip::new();
        s.set_tab_container(99, ContainerKind::Personal); // must not panic
        assert_eq!(s.tabs[0].container, ContainerKind::None);
    }

    /// Helper: count `FillRect` commands whose rect matches the container
    /// border-top strip — height equals `CONTAINER_STRIP_HEIGHT` and origin
    /// `y == 0.0`. Excludes the full-bar background rect (its height ==
    /// `TAB_BAR_HEIGHT`).
    fn count_container_strips(dl: &DisplayList, expected_color: Color) -> usize {
        dl.iter()
            .filter(|c| match c {
                DisplayCommand::FillRect { rect, color } => {
                    (rect.height - CONTAINER_STRIP_HEIGHT).abs() < f32::EPSILON
                        && rect.y.abs() < f32::EPSILON
                        && *color == expected_color
                }
                _ => false,
            })
            .count()
    }

    #[test]
    fn build_tab_bar_renders_strip_for_work() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Work);
        let dl = build_tab_bar(&s, 1024.0);
        let expected = ContainerKind::Work.border_color().expect("Work has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_personal() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Personal);
        let dl = build_tab_bar(&s, 1024.0);
        let expected = ContainerKind::Personal.border_color().expect("Personal has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_finance() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Finance);
        let dl = build_tab_bar(&s, 1024.0);
        let expected = ContainerKind::Finance.border_color().expect("Finance has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_shopping() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Shopping);
        let dl = build_tab_bar(&s, 1024.0);
        let expected = ContainerKind::Shopping.border_color().expect("Shopping has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_custom_rgb() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Custom(200, 50, 100));
        let dl = build_tab_bar(&s, 1024.0);
        let expected = Color { r: 200, g: 50, b: 100, a: 255 };
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_no_strip_for_none_container() {
        let s = TabStrip::new(); // single tab, ContainerKind::None
        let dl = build_tab_bar(&s, 1024.0);
        // No FillRect of CONTAINER_STRIP_HEIGHT may exist when container is None.
        let strips = dl
            .iter()
            .filter(|c| match c {
                DisplayCommand::FillRect { rect, .. } => {
                    (rect.height - CONTAINER_STRIP_HEIGHT).abs() < f32::EPSILON
                        && rect.y.abs() < f32::EPSILON
                }
                _ => false,
            })
            .count();
        assert_eq!(strips, 0, "ContainerKind::None must not render a strip");
    }

    #[test]
    fn build_tab_bar_strip_only_for_tabs_with_container() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.push_blank(0.0);
        s.set_tab_container(1, ContainerKind::Work);
        let dl = build_tab_bar(&s, 1024.0);
        let work_color = ContainerKind::Work.border_color().expect("Work has colour");
        // Exactly one Work-coloured strip (tab 1); tabs 0 and 2 have None.
        assert_eq!(count_container_strips(&dl, work_color), 1);
    }

    #[test]
    fn tooltip_none_for_active_tab() {
        let tab = TabEntry {
            id: 0,
            title: "Test".to_owned(),
            tab_state: TabState::Active,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: 0.0,
        };
        assert!(build_tab_tooltip(&tab, 100.0, 36.0).is_none());
    }

    #[test]
    fn tooltip_some_for_hibernated_tab() {
        let tab = TabEntry {
            id: 0,
            title: "Test".to_owned(),
            tab_state: TabState::Hibernated,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: 0.0,
        };
        let cmds = build_tab_tooltip(&tab, 100.0, 36.0);
        assert!(cmds.is_some());
        // Tooltip must have at least background + text.
        assert!(cmds.unwrap().len() >= 2);
    }

    #[test]
    fn tooltip_some_for_background_old() {
        let tab = TabEntry {
            id: 0,
            title: "Test".to_owned(),
            tab_state: TabState::BackgroundOld,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: 0.0,
        };
        assert!(build_tab_tooltip(&tab, 100.0, 36.0).is_some());
    }
}
