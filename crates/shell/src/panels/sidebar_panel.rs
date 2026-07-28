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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_H: f32 = 720.0;
    const TAB_H: f32 = 36.0;

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
}
