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

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants ──────────────────────────────────────────────────────────

/// Width of the sidebar panel in CSS px.
pub const PANEL_WIDTH: f32 = 300.0;
/// Height of the sidebar title bar in CSS px.
const HEADER_H: f32 = 32.0;
/// Close button size in CSS px (square).
const CLOSE_SIZE: f32 = 18.0;
/// Right margin for the close button inside the header.
const CLOSE_RIGHT: f32 = 7.0;

const BG: Color = Color { r: 26, g: 28, b: 36, a: 255 };
const HEADER_BG: Color = Color { r: 36, g: 39, b: 50, a: 255 };
const BORDER: Color = Color { r: 55, g: 58, b: 74, a: 255 };
const TEXT_MAIN: Color = Color { r: 215, g: 215, b: 226, a: 255 };
const TEXT_DIM: Color = Color { r: 120, g: 124, b: 142, a: 255 };
const CLOSE_BG: Color = Color { r: 60, g: 63, b: 80, a: 200 };
const CLOSE_FG: Color = Color { r: 160, g: 160, b: 172, a: 255 };
const PLACEHOLDER_BG: Color = Color { r: 31, g: 34, b: 44, a: 255 };

const FONT_SZ: f32 = 11.0;

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
    window_w: f32,
    tab_bar_h: f32,
    window_h: f32,
) -> Option<SidebarHit> {
    if !panel.visible {
        return None;
    }
    let px = window_w - PANEL_WIDTH;
    if x < px || x >= window_w || y < tab_bar_h || y >= window_h {
        return None;
    }
    let rel_y = y - tab_bar_h;

    if rel_y < HEADER_H {
        // Close button: right side of header.
        let close_x = px + PANEL_WIDTH - CLOSE_RIGHT - CLOSE_SIZE;
        let close_y = tab_bar_h + (HEADER_H - CLOSE_SIZE) / 2.0;
        if x >= close_x && x < close_x + CLOSE_SIZE && y >= close_y && y < close_y + CLOSE_SIZE {
            return Some(SidebarHit::Close);
        }
        return Some(SidebarHit::Header);
    }

    Some(SidebarHit::Content)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the right-docked sidebar panel.
///
/// Renders from `x = (window_w − PANEL_WIDTH)` to `x = window_w` and from
/// `y = tab_bar_h` to `y = window_h`.  Scroll offset is baked into a
/// `PushTransform` over the content area.
pub fn build_panel(
    panel: &SidebarPanel,
    window_w: f32,
    tab_bar_h: f32,
    window_h: f32,
) -> DisplayList {
    if !panel.visible {
        return DisplayList::new();
    }

    let px = window_w - PANEL_WIDTH;
    let panel_h = window_h - tab_bar_h;
    let content_y = tab_bar_h + HEADER_H;
    let content_h = (panel_h - HEADER_H).max(0.0);

    let mut out = DisplayList::with_capacity(28);

    // ── Panel background ──────────────────────────────────────────────────────
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_bar_h, PANEL_WIDTH, panel_h),
        color: BG,
    });

    // Left border (1 px divider between main page and sidebar).
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_bar_h, 1.0, panel_h),
        color: BORDER,
    });

    // ── Header bar ────────────────────────────────────────────────────────────
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 1.0, tab_bar_h, PANEL_WIDTH - 1.0, HEADER_H),
        color: HEADER_BG,
    });
    // Header bottom border.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 1.0, tab_bar_h + HEADER_H - 1.0, PANEL_WIDTH - 1.0, 1.0),
        color: BORDER,
    });

    // Title text (truncated to avoid overlapping the close button).
    let title_text = if !panel.title.is_empty() {
        truncate_label(&panel.title, 28)
    } else if let Some(ref u) = panel.url {
        truncate_label(u, 28)
    } else {
        "Sidebar".to_owned()
    };
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 10.0, tab_bar_h + 9.0, PANEL_WIDTH - CLOSE_SIZE - CLOSE_RIGHT * 2.0 - 14.0, FONT_SZ * 1.4),
        text: title_text,
        font_size: FONT_SZ,
        color: TEXT_MAIN,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Close button background.
    let close_x = px + PANEL_WIDTH - CLOSE_RIGHT - CLOSE_SIZE;
    let close_y = tab_bar_h + (HEADER_H - CLOSE_SIZE) / 2.0;
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(close_x, close_y, CLOSE_SIZE, CLOSE_SIZE),
        radii: CornerRadii { tl: 3.0, tl_y: 3.0, tr: 3.0, tr_y: 3.0, br: 3.0, br_y: 3.0, bl: 3.0, bl_y: 3.0 },
        color: CLOSE_BG,
    });
    // Close button "×" glyph.
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(close_x + 3.0, close_y + 1.0, CLOSE_SIZE - 6.0, CLOSE_SIZE - 2.0),
        text: "×".to_owned(),
        font_size: 13.0,
        color: CLOSE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // ── Content area ──────────────────────────────────────────────────────────
    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(px + 1.0, content_y, PANEL_WIDTH - 1.0, content_h),
    });

    if let Some(ref dl) = panel.page_dl {
        // Translate sidebar page DL: x offset → panel left edge,
        // y offset → content_y with scroll baked in.
        out.push(DisplayCommand::PushTransform {
            matrix: lumen_layout::Mat4::translation_2d(px + 1.0, content_y - panel.scroll_y),
        });
        out.extend_from_slice(dl);
        out.push(DisplayCommand::PopTransform);
    } else {
        // Placeholder: show URL and "Loading…" hint until the DL is ready.
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(px + 1.0, content_y, PANEL_WIDTH - 1.0, content_h),
            color: PLACEHOLDER_BG,
        });
        let url_str = panel.url.as_deref().unwrap_or("No page");
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(px + 10.0, content_y + 18.0, PANEL_WIDTH - 20.0, FONT_SZ * 1.4),
            text: truncate_label(url_str, 34),
            font_size: FONT_SZ,
            color: TEXT_DIM,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(px + 10.0, content_y + 38.0, PANEL_WIDTH - 20.0, FONT_SZ * 1.4),
            text: "Loading…".to_owned(),
            font_size: FONT_SZ,
            color: TEXT_DIM,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
    }

    out.push(DisplayCommand::PopClip);
    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Truncate a label to at most `max_chars` characters, appending "…" if cut.
fn truncate_label(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{collected}…")
    } else {
        collected
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_W: f32 = 1024.0;
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
        assert!(hit_test(&p, WIN_W - 10.0, 100.0, WIN_W, TAB_H, WIN_H).is_none());
    }

    #[test]
    fn hit_test_outside_panel_returns_none() {
        let p = visible_no_page();
        // Click in main page area
        assert!(hit_test(&p, WIN_W - PANEL_WIDTH - 1.0, 100.0, WIN_W, TAB_H, WIN_H).is_none());
    }

    #[test]
    fn hit_test_in_tab_bar_area_returns_none() {
        let p = visible_no_page();
        assert!(hit_test(&p, WIN_W - 10.0, TAB_H - 1.0, WIN_W, TAB_H, WIN_H).is_none());
    }

    #[test]
    fn hit_test_header_no_close() {
        let p = visible_no_page();
        let hit = hit_test(&p, WIN_W - PANEL_WIDTH + 50.0, TAB_H + 5.0, WIN_W, TAB_H, WIN_H);
        assert_eq!(hit, Some(SidebarHit::Header));
    }

    #[test]
    fn hit_test_close_button() {
        let p = visible_no_page();
        let close_x = WIN_W - CLOSE_RIGHT - CLOSE_SIZE + 2.0;
        let close_y = TAB_H + (HEADER_H - CLOSE_SIZE) / 2.0 + 2.0;
        let hit = hit_test(&p, close_x, close_y, WIN_W, TAB_H, WIN_H);
        assert_eq!(hit, Some(SidebarHit::Close));
    }

    #[test]
    fn hit_test_content_area() {
        let p = visible_no_page();
        let content_y = TAB_H + HEADER_H + 10.0;
        let hit = hit_test(&p, WIN_W - PANEL_WIDTH + 10.0, content_y, WIN_W, TAB_H, WIN_H);
        assert_eq!(hit, Some(SidebarHit::Content));
    }

    // ── build_panel ───────────────────────────────────────────────────────────

    #[test]
    fn build_panel_hidden_is_empty() {
        let p = hidden();
        let dl = build_panel(&p, WIN_W, TAB_H, WIN_H);
        assert!(dl.is_empty());
    }

    #[test]
    fn build_panel_visible_no_page_has_placeholder() {
        let p = visible_no_page();
        let dl = build_panel(&p, WIN_W, TAB_H, WIN_H);
        assert!(!dl.is_empty());
        let has_loading = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Loading"))
        });
        assert!(has_loading, "placeholder should contain loading hint");
    }

    #[test]
    fn build_panel_with_page_no_loading_text() {
        let p = visible_with_page();
        let dl = build_panel(&p, WIN_W, TAB_H, WIN_H);
        let has_loading = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Loading"))
        });
        assert!(!has_loading, "page DL should not show loading placeholder");
    }

    #[test]
    fn build_panel_has_close_x_text() {
        let p = visible_no_page();
        let dl = build_panel(&p, WIN_W, TAB_H, WIN_H);
        let has_close = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "×")
        });
        assert!(has_close);
    }

    #[test]
    fn build_panel_has_clip_and_pop() {
        let p = visible_no_page();
        let dl = build_panel(&p, WIN_W, TAB_H, WIN_H);
        let clips = dl.iter().filter(|c| matches!(c, DisplayCommand::PushClipRect { .. })).count();
        let pops = dl.iter().filter(|c| matches!(c, DisplayCommand::PopClip)).count();
        assert_eq!(clips, 1);
        assert_eq!(pops, 1);
    }

    // ── truncate_label ────────────────────────────────────────────────────────

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate_label("hi", 10), "hi");
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        let s = truncate_label("hello world", 5);
        assert!(s.ends_with('…'));
        assert!(s.len() <= 8); // 5 chars + 3 bytes for '…'
    }
}
