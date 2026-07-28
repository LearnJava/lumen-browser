//! Right-docked sidebar web panel (7D.3).
//!
//! Shows a secondary web viewport in a [`PANEL_WIDTH`]-wide slot at the right
//! edge of the window, below the tab bar.  Content is a frozen
//! [`DisplayList`] snapshot supplied by the shell (via [`SidebarPanel::set_page`])
//! after a page has been loaded at [`PANEL_WIDTH`]-wide viewport.
//!
//! When visible, `page_content_width_css()` subtracts [`PANEL_WIDTH`] so the
//! main page viewport shrinks accordingly.  `relayout()` is called on toggle.
//!
//! Layout (CSS px):
//! ```text
//! x=(window_w - PANEL_WIDTH)                       x=window_w
//! y=tab_bar_h  ┌──────────────────────────────────┐
//!              │ title                        [×]  │ ← HEADER_H = 32
//!              ├──────────────────────────────────┤
//!              │                                  │
//!              │  page display list               │
//!              │  (PushClipRect + PushTransform   │
//!              │   so scroll_y shifts content)    │
//!              │                                  │
//! y=window_h   └──────────────────────────────────┘
//! ```
//!
//! Opening: `shell::Lumen::open_sidebar(url)` — loads the page via the
//! existing `relayout_page` pipeline at sidebar-width viewport, stores DL here.
//! Keyboard toggle: `Ctrl+Shift+A` → `KeyCommand::ToggleSidebar`.

use lumen_paint::DisplayList;

// ── Visual constants ──────────────────────────────────────────────────────────

/// Width of the sidebar panel in CSS px.
pub const PANEL_WIDTH: f32 = 300.0;
/// Height of the sidebar title bar in CSS px.
const HEADER_H: f32 = 32.0;
/// Close button size in CSS px (square).
const CLOSE_SIZE: f32 = 18.0;
/// Right margin for the close button inside the header.
const CLOSE_RIGHT: f32 = 7.0;

// ── Data types ────────────────────────────────────────────────────────────────

/// Right-docked sidebar web panel state (7D.3).
///
/// When `visible` the right [`PANEL_WIDTH`] CSS px of the window are occupied
/// by the sidebar.  [`page_content_width_css`] in `main.rs` subtracts this
/// width and `relayout()` is called on every visibility change.
pub struct SidebarPanel {
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// URL of the page that was requested for the sidebar.  `None` means no
    /// page has been opened; `toggle()` is a no-op in that state.
    pub url: Option<String>,
    /// Frozen display list of the sidebar page (content coords, origin = 0,0).
    /// `None` = placeholder is rendered until the shell supplies the DL.
    pub page_dl: Option<DisplayList>,
    /// Title shown in the sidebar header bar (set from `<title>` after load).
    pub title: String,
    /// Vertical scroll offset in CSS px (0 = top of sidebar content).
    pub scroll_y: f32,
    /// Full content height of the sidebar page in CSS px (for scroll clamping).
    pub content_height: f32,
}

impl SidebarPanel {
    /// Create a new hidden sidebar panel with no page loaded.
    pub fn new() -> Self {
        Self {
            visible: false,
            url: None,
            page_dl: None,
            title: String::new(),
            scroll_y: 0.0,
            content_height: 0.0,
        }
    }

    /// Toggle panel visibility.  No-op when no URL has been set.
    #[allow(dead_code)]
    pub fn toggle(&mut self) {
        if self.url.is_some() {
            self.visible = !self.visible;
        }
    }

    /// Open the sidebar with `url`.  Clears content if the URL changed.
    ///
    /// Does not fetch or layout the page — the caller must call
    /// `open_sidebar_page` on the `Lumen` struct to supply the display list.
    pub fn open(&mut self, url: String) {
        let changed = self.url.as_deref() != Some(url.as_str());
        if changed {
            self.page_dl = None;
            self.scroll_y = 0.0;
            self.content_height = 0.0;
            self.title = url.clone();
        }
        self.url = Some(url);
        self.visible = true;
    }

    /// Close the sidebar (hide; URL and content are preserved for re-open).
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Store a freshly-rendered display list for the sidebar page.
    ///
    /// Called by `Lumen::open_sidebar_page` after the page pipeline completes.
    pub fn set_page(&mut self, dl: DisplayList, title: String, content_height: f32) {
        self.page_dl = Some(dl);
        self.title = title;
        self.content_height = content_height;
        self.scroll_y = 0.0;
    }

    /// Replace the page display list after a width reflow (F2-6 drag-resize).
    ///
    /// Unlike [`set_page`], the title is kept and `scroll_y` is preserved
    /// (clamped to the new content height) so a resize does not jump the user
    /// back to the top of the page.
    pub fn update_page(&mut self, dl: DisplayList, content_height: f32) {
        self.page_dl = Some(dl);
        self.content_height = content_height;
        self.scroll_y = self.scroll_y.clamp(0.0, self.content_height.max(0.0));
    }

    /// Maximum valid `scroll_y` (0 if content fits in viewport).
    #[allow(dead_code)]
    pub fn max_scroll(&self, viewport_h: f32) -> f32 {
        let usable = (viewport_h - HEADER_H).max(0.0);
        (self.content_height - usable).max(0.0)
    }
}

impl Default for SidebarPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the sidebar panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarHit {
    /// Clicked the "×" close button in the header.
    Close,
    /// Clicked in the page content area.
    Content,
    /// Clicked in the header (not on the close button).
    Header,
}

/// Hit-test `(x, y)` in CSS px against the sidebar panel.
///
/// Returns `None` when the click is outside the panel or the panel is hidden.
/// `tab_bar_h` is the height of the tab strip above the panel.
pub fn hit_test(
    panel: &SidebarPanel,
    x: f32,
    y: f32,
    origin_x: f32,
    tab_bar_h: f32,
    window_h: f32,
    width: f32,
) -> Option<SidebarHit> {
    if !panel.visible {
        return None;
    }
    let px = origin_x;
    if x < px || x >= px + width || y < tab_bar_h || y >= window_h {
        return None;
    }
    let rel_y = y - tab_bar_h;

    if rel_y < HEADER_H {
        // Close button: right side of header.
        let close_x = px + width - CLOSE_RIGHT - CLOSE_SIZE;
        let close_y = tab_bar_h + (HEADER_H - CLOSE_SIZE) / 2.0;
        if x >= close_x && x < close_x + CLOSE_SIZE && y >= close_y && y < close_y + CLOSE_SIZE {
            return Some(SidebarHit::Close);
        }
        return Some(SidebarHit::Header);
    }

    Some(SidebarHit::Content)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_W: f32 = 1024.0;
    const WIN_H: f32 = 720.0;
    const TAB_H: f32 = 36.0;
    /// Left origin of the panel at its default right dock.
    const PX: f32 = WIN_W - PANEL_WIDTH;

    fn hidden() -> SidebarPanel {
        SidebarPanel::new()
    }

    fn visible_no_page() -> SidebarPanel {
        let mut p = SidebarPanel::new();
        p.open("https://example.com".into());
        p
    }

    fn visible_with_page() -> SidebarPanel {
        let mut p = visible_no_page();
        p.set_page(vec![], "Example".into(), 800.0);
        p
    }

    // ── toggle / open / close ─────────────────────────────────────────────────

    #[test]
    fn toggle_no_url_is_noop() {
        let mut p = hidden();
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_with_url_shows_and_hides() {
        let mut p = hidden();
        p.open("https://example.com".into());
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
        p.toggle();
        assert!(p.visible);
    }

    #[test]
    fn open_same_url_keeps_content() {
        let mut p = visible_no_page();
        p.set_page(vec![], "Title".into(), 400.0);
        p.open("https://example.com".into());
        assert!(p.page_dl.is_some(), "same-url open should keep existing DL");
        assert_eq!(p.title, "Title");
    }

    #[test]
    fn open_different_url_clears_content() {
        let mut p = visible_no_page();
        p.set_page(vec![], "Title".into(), 400.0);
        p.open("https://other.com".into());
        assert!(p.page_dl.is_none(), "new URL should clear old DL");
        assert_eq!(p.url.as_deref(), Some("https://other.com"));
    }

    // ── update_page (F2-6 reflow on drag-resize) ──────────────────────────────

    #[test]
    fn update_page_keeps_title_and_scroll() {
        let mut p = visible_with_page();
        p.scroll_y = 120.0;
        p.update_page(vec![], 900.0);
        assert_eq!(p.title, "Example", "reflow must keep the page title");
        assert_eq!(p.content_height, 900.0);
        assert_eq!(p.scroll_y, 120.0, "scroll preserved when still in range");
    }

    #[test]
    fn update_page_clamps_scroll_to_shrunk_content() {
        let mut p = visible_with_page();
        p.scroll_y = 700.0;
        // Reflow to a much shorter page (e.g. a wider sidebar fits more per line).
        p.update_page(vec![], 300.0);
        assert_eq!(p.scroll_y, 300.0, "scroll clamped to new content height");
    }

    #[test]
    fn close_hides_preserves_url() {
        let mut p = visible_no_page();
        p.close();
        assert!(!p.visible);
        assert!(p.url.is_some());
    }

    // ── max_scroll ────────────────────────────────────────────────────────────

    #[test]
    fn max_scroll_fits_in_viewport() {
        let mut p = visible_no_page();
        p.content_height = 200.0;
        let usable = WIN_H - TAB_H - HEADER_H;
        assert_eq!(p.max_scroll(WIN_H - TAB_H), (200.0 - usable).max(0.0));
    }

    #[test]
    fn max_scroll_zero_when_content_fits() {
        let mut p = visible_no_page();
        p.content_height = 10.0;
        assert_eq!(p.max_scroll(WIN_H - TAB_H), 0.0);
    }

    // ── hit_test ──────────────────────────────────────────────────────────────

    #[test]
    fn hit_test_hidden_returns_none() {
        let p = hidden();
        assert!(hit_test(&p, WIN_W - 10.0, 100.0, PX, TAB_H, WIN_H, PANEL_WIDTH).is_none());
    }

    #[test]
    fn hit_test_outside_panel_returns_none() {
        let p = visible_no_page();
        // Click in main page area
        assert!(hit_test(&p, WIN_W - PANEL_WIDTH - 1.0, 100.0, PX, TAB_H, WIN_H, PANEL_WIDTH).is_none());
    }

    #[test]
    fn hit_test_in_tab_bar_area_returns_none() {
        let p = visible_no_page();
        assert!(hit_test(&p, WIN_W - 10.0, TAB_H - 1.0, PX, TAB_H, WIN_H, PANEL_WIDTH).is_none());
    }

    #[test]
    fn hit_test_header_no_close() {
        let p = visible_no_page();
        let hit = hit_test(&p, WIN_W - PANEL_WIDTH + 50.0, TAB_H + 5.0, PX, TAB_H, WIN_H, PANEL_WIDTH);
        assert_eq!(hit, Some(SidebarHit::Header));
    }

    #[test]
    fn hit_test_close_button() {
        let p = visible_no_page();
        let close_x = WIN_W - CLOSE_RIGHT - CLOSE_SIZE + 2.0;
        let close_y = TAB_H + (HEADER_H - CLOSE_SIZE) / 2.0 + 2.0;
        let hit = hit_test(&p, close_x, close_y, PX, TAB_H, WIN_H, PANEL_WIDTH);
        assert_eq!(hit, Some(SidebarHit::Close));
    }

    #[test]
    fn hit_test_content_area() {
        let p = visible_no_page();
        let content_y = TAB_H + HEADER_H + 10.0;
        let hit = hit_test(&p, WIN_W - PANEL_WIDTH + 10.0, content_y, PX, TAB_H, WIN_H, PANEL_WIDTH);
        assert_eq!(hit, Some(SidebarHit::Content));
    }

    // ── cross-dock (origin_x at the left edge) ──────────────────────────────────

    #[test]
    fn hit_test_left_dock_inside_and_outside() {
        let p = visible_no_page();
        // origin_x = 0 → panel hugs the left edge, spanning [0, PANEL_WIDTH).
        assert!(hit_test(&p, 10.0, TAB_H + HEADER_H + 10.0, 0.0, TAB_H, WIN_H, PANEL_WIDTH).is_some());
        assert!(hit_test(&p, PANEL_WIDTH + 1.0, TAB_H + HEADER_H + 10.0, 0.0, TAB_H, WIN_H, PANEL_WIDTH).is_none());
    }

    #[test]
    fn hit_test_left_dock_close_button() {
        let p = visible_no_page();
        let close_x = PANEL_WIDTH - CLOSE_RIGHT - CLOSE_SIZE + 2.0;
        let close_y = TAB_H + (HEADER_H - CLOSE_SIZE) / 2.0 + 2.0;
        assert_eq!(
            hit_test(&p, close_x, close_y, 0.0, TAB_H, WIN_H, PANEL_WIDTH),
            Some(SidebarHit::Close)
        );
    }
}
