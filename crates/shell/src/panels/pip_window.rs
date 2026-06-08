//! Picture-in-picture floating video window (task #21).
//!
//! A compact 320×180 (16:9) card that floats above the page and keeps a tab's
//! `<video>` element visible while the user scrolls the page or switches to
//! another tab.  Conceptually this is ADR-009's `Surface::OsWindow`, but the
//! `Surface` trait does not yet exist in code, so — like every other shell
//! panel (`focus_panel`, `command_palette`, …) — it is implemented ad-hoc:
//! state lives on `Lumen`, [`hit_test`] classifies clicks, and [`build_panel`]
//! returns a [`DisplayList`].  It is an **in-window overlay**, not a separate OS
//! window; a real second winit window awaits multi-window support in the shell
//! event loop (the genuine blocker on the spec's `Surface::OsWindow`).
//!
//! The card shows the video's poster image (the `<video>` placeholder is a grey
//! frame until real playback exists), a translucent title bar with a `×` close
//! button, and a centred play / pause button.  It can be dragged anywhere inside
//! the window by its title bar.  Toggled with `Ctrl+Shift+V`.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight, ImageRendering, ObjectFit, ObjectPosition};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the PiP card in CSS px.
pub const CARD_W: f32 = 320.0;

/// Height of the PiP card in CSS px (16:9 with `CARD_W`).
pub const CARD_H: f32 = 180.0;

/// Margin from the window edge to the card when first opened, in CSS px.
const MARGIN: f32 = 16.0;

/// Height of the translucent title bar overlaid on the top of the card.
const HEADER_H: f32 = 26.0;

/// Side length of the square `×` close hit-zone in the title bar's top-right.
const CLOSE_W: f32 = 26.0;

/// Radius of the centred play / pause button in CSS px.
const PLAY_R: f32 = 24.0;

/// Corner radius of the card in CSS px.
const CARD_RADIUS: f32 = 10.0;

const CARD_BORDER: Color = Color { r: 12, g: 12, b: 16, a: 255 };
const VIDEO_BG: Color = Color { r: 24, g: 24, b: 30, a: 255 };
const HEADER_BG: Color = Color { r: 10, g: 10, b: 14, a: 180 };
const TITLE_FG: Color = Color { r: 232, g: 232, b: 240, a: 255 };
const CLOSE_FG: Color = Color { r: 220, g: 150, b: 150, a: 255 };
const BTN_BG: Color = Color { r: 0, g: 0, b: 0, a: 110 };
const BTN_FG: Color = Color { r: 240, g: 240, b: 244, a: 235 };

// ── Panel state ───────────────────────────────────────────────────────────────

/// Picture-in-picture window state.
///
/// Holds the source video reference, the play / pause flag, and the card's
/// top-left position (in window CSS px) so it can be dragged.  Inactive by
/// default; [`open`](Self::open) populates it from a page `<video>` box.
pub struct PipWindow {
    /// `true` while the PiP card is shown.
    pub active: bool,
    /// `src` URL of the embedded `<video>` element (informational; not fetched).
    pub source_url: String,
    /// `poster` image URL drawn in the video frame; empty → grey placeholder.
    pub poster_url: String,
    /// Title shown in the card's title bar (usually the originating tab title).
    pub title: String,
    /// `true` while nominally playing; toggled by the centre button.  Real
    /// playback does not exist yet, so this only drives the button glyph.
    pub playing: bool,
    /// Card top-left corner `(x, y)` in window CSS px.
    pub pos: (f32, f32),
    /// While dragging the title bar: pointer offset `(dx, dy)` from `pos` to the
    /// grab point, so the card follows the cursor without jumping.  `None` when
    /// not dragging.
    drag: Option<(f32, f32)>,
}

impl PipWindow {
    /// Create an inactive PiP window positioned at the origin (re-anchored to the
    /// bottom-right corner on the first [`open`](Self::open)).
    pub fn new() -> Self {
        Self {
            active: false,
            source_url: String::new(),
            poster_url: String::new(),
            title: String::new(),
            playing: false,
            pos: (0.0, 0.0),
            drag: None,
        }
    }

    /// Open the PiP card for a `<video>` source, anchored to the bottom-right of
    /// a `win_w`×`win_h` (CSS px) window.  Begins in the playing state.
    pub fn open(
        &mut self,
        source_url: impl Into<String>,
        poster_url: impl Into<String>,
        title: impl Into<String>,
        win_w: f32,
        win_h: f32,
    ) {
        self.active = true;
        self.source_url = source_url.into();
        self.poster_url = poster_url.into();
        self.title = title.into();
        self.playing = true;
        self.drag = None;
        self.pos = Self::default_pos(win_w, win_h);
    }

    /// Close the card (state is retained but no longer drawn).
    pub fn close(&mut self) {
        self.active = false;
        self.drag = None;
    }

    /// Flip the play / pause flag.
    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    /// Default bottom-right anchored top-left corner for a `win_w`×`win_h` window.
    pub fn default_pos(win_w: f32, win_h: f32) -> (f32, f32) {
        (
            (win_w - MARGIN - CARD_W).max(MARGIN),
            (win_h - MARGIN - CARD_H).max(MARGIN),
        )
    }

    /// Clamp the card so it stays fully inside a `win_w`×`win_h` window, leaving
    /// at least a sliver on screen even for tiny windows.
    pub fn clamp_to_window(&mut self, win_w: f32, win_h: f32) {
        let max_x = (win_w - CARD_W).max(0.0);
        let max_y = (win_h - CARD_H).max(0.0);
        self.pos.0 = self.pos.0.clamp(0.0, max_x);
        self.pos.1 = self.pos.1.clamp(0.0, max_y);
    }

    /// Begin dragging the card: record the pointer offset from the card origin.
    pub fn begin_drag(&mut self, x: f32, y: f32) {
        self.drag = Some((x - self.pos.0, y - self.pos.1));
    }

    /// `true` while a title-bar drag is in progress.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Update the card position from the pointer during a drag, clamped to the
    /// window.  No-op when not dragging.
    pub fn drag_to(&mut self, x: f32, y: f32, win_w: f32, win_h: f32) {
        if let Some((dx, dy)) = self.drag {
            self.pos = (x - dx, y - dy);
            self.clamp_to_window(win_w, win_h);
        }
    }

    /// End an in-progress drag.
    pub fn end_drag(&mut self) {
        self.drag = None;
    }
}

impl Default for PipWindow {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the PiP card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipHit {
    /// Clicked the `×` button — close the window.
    Close,
    /// Clicked the centre button — toggle play / pause.
    PlayPause,
    /// Pressed the title bar — start dragging the card.
    Header,
    /// Clicked elsewhere inside the card — swallow the click (do not fall through
    /// to the page beneath).
    Body,
}

/// Hit-test a click at window CSS-px `(x, y)` against the PiP card.
///
/// Returns `None` when the card is inactive or the point is outside it, so the
/// click falls through to the page.
pub fn hit_test(pip: &PipWindow, x: f32, y: f32) -> Option<PipHit> {
    if !pip.active {
        return None;
    }
    let (cx0, cy0) = pip.pos;
    if x < cx0 || x >= cx0 + CARD_W || y < cy0 || y >= cy0 + CARD_H {
        return None;
    }

    // `×` close zone — top-right corner of the title bar.
    if x >= cx0 + CARD_W - CLOSE_W && y < cy0 + HEADER_H {
        return Some(PipHit::Close);
    }

    // Centre play / pause button (circular).
    let bcx = cx0 + CARD_W * 0.5;
    let bcy = cy0 + CARD_H * 0.5;
    if (x - bcx) * (x - bcx) + (y - bcy) * (y - bcy) <= PLAY_R * PLAY_R {
        return Some(PipHit::PlayPause);
    }

    // Title bar (excluding the close zone handled above) — drag handle.
    if y < cy0 + HEADER_H {
        return Some(PipHit::Header);
    }

    Some(PipHit::Body)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the PiP overlay.  Empty when inactive.
pub fn build_panel(pip: &PipWindow) -> DisplayList {
    let mut out = DisplayList::with_capacity(10);
    if !pip.active {
        return out;
    }

    let (cx0, cy0) = pip.pos;
    let radii = CornerRadii {
        tl: CARD_RADIUS, tl_y: CARD_RADIUS,
        tr: CARD_RADIUS, tr_y: CARD_RADIUS,
        br: CARD_RADIUS, br_y: CARD_RADIUS,
        bl: CARD_RADIUS, bl_y: CARD_RADIUS,
    };

    // 1px border (slightly larger rounded rect behind the card).
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(cx0 - 1.0, cy0 - 1.0, CARD_W + 2.0, CARD_H + 2.0),
        radii,
        color: CARD_BORDER,
    });

    // Video frame background (grey placeholder).
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(cx0, cy0, CARD_W, CARD_H),
        radii,
        color: VIDEO_BG,
    });

    // Poster image, if any — covers the whole frame.
    if !pip.poster_url.is_empty() {
        out.push(DisplayCommand::DrawImage {
            rect: Rect::new(cx0, cy0, CARD_W, CARD_H),
            src: pip.poster_url.clone(),
            alt: String::new(),
            object_fit: ObjectFit::Cover,
            object_position: ObjectPosition::default(),
            image_rendering: ImageRendering::Auto,
        });
    }

    // Translucent title bar across the top.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(cx0, cy0, CARD_W, HEADER_H),
        color: HEADER_BG,
    });

    // Title text (truncated to fit before the close button).
    let title = if pip.title.is_empty() {
        "Picture-in-Picture".to_owned()
    } else {
        truncate(&pip.title, 40)
    };
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(cx0 + 10.0, cy0 + 6.0, CARD_W - CLOSE_W - 14.0, 16.0),
        text: title,
        font_size: 12.0,
        color: TITLE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // `×` close glyph.
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(cx0 + CARD_W - CLOSE_W, cy0 + 5.0, CLOSE_W, 16.0),
        text: "×".to_owned(),
        font_size: 15.0,
        color: CLOSE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Centre play / pause button: circular scrim + glyph.
    let bcx = cx0 + CARD_W * 0.5;
    let bcy = cy0 + CARD_H * 0.5;
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(bcx - PLAY_R, bcy - PLAY_R, PLAY_R * 2.0, PLAY_R * 2.0),
        radii: CornerRadii {
            tl: PLAY_R, tl_y: PLAY_R,
            tr: PLAY_R, tr_y: PLAY_R,
            br: PLAY_R, br_y: PLAY_R,
            bl: PLAY_R, bl_y: PLAY_R,
        },
        color: BTN_BG,
    });
    if pip.playing {
        // Pause: two vertical bars.
        let bar_w = 5.0;
        let bar_h = 22.0;
        let gap = 5.0;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(bcx - gap - bar_w, bcy - bar_h * 0.5, bar_w, bar_h),
            color: BTN_FG,
        });
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(bcx + gap, bcy - bar_h * 0.5, bar_w, bar_h),
            color: BTN_FG,
        });
    } else {
        // Play: right-pointing triangle, optically centred.
        let r = 12.0;
        out.push(DisplayCommand::DrawSvgPath {
            vertices: vec![
                [bcx - r * 0.55, bcy - r],
                [bcx - r * 0.55, bcy + r],
                [bcx + r * 0.9, bcy],
            ],
            color: BTN_FG,
        });
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Truncate `s` to at most `max` characters, appending `…` when shortened.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_W: f32 = 1024.0;
    const WIN_H: f32 = 720.0;

    fn opened() -> PipWindow {
        let mut p = PipWindow::new();
        p.open("video.mp4", "poster.jpg", "My Tab", WIN_W, WIN_H);
        p
    }

    // ── State ────────────────────────────────────────────────────────────────────

    #[test]
    fn new_is_inactive() {
        let p = PipWindow::new();
        assert!(!p.active);
        assert!(!p.dragging());
    }

    #[test]
    fn open_activates_and_anchors_bottom_right() {
        let p = opened();
        assert!(p.active);
        assert!(p.playing);
        assert_eq!(p.source_url, "video.mp4");
        assert_eq!(p.poster_url, "poster.jpg");
        assert_eq!(p.title, "My Tab");
        assert_eq!(p.pos, PipWindow::default_pos(WIN_W, WIN_H));
        // Bottom-right: card fits within the window.
        assert!(p.pos.0 + CARD_W <= WIN_W);
        assert!(p.pos.1 + CARD_H <= WIN_H);
    }

    #[test]
    fn close_deactivates() {
        let mut p = opened();
        p.close();
        assert!(!p.active);
    }

    #[test]
    fn toggle_play_flips() {
        let mut p = opened();
        assert!(p.playing);
        p.toggle_play();
        assert!(!p.playing);
        p.toggle_play();
        assert!(p.playing);
    }

    #[test]
    fn default_pos_never_negative_for_small_window() {
        let (x, y) = PipWindow::default_pos(100.0, 80.0);
        assert!(x >= MARGIN);
        assert!(y >= MARGIN);
    }

    // ── Dragging ─────────────────────────────────────────────────────────────────

    #[test]
    fn drag_moves_card_by_pointer_delta() {
        let mut p = opened();
        let (x0, y0) = p.pos;
        // Grab near the title bar, then move 30px left / 20px up.
        p.begin_drag(x0 + 50.0, y0 + 10.0);
        assert!(p.dragging());
        p.drag_to(x0 + 50.0 - 30.0, y0 + 10.0 - 20.0, WIN_W, WIN_H);
        assert!((p.pos.0 - (x0 - 30.0)).abs() < 1e-3);
        assert!((p.pos.1 - (y0 - 20.0)).abs() < 1e-3);
        p.end_drag();
        assert!(!p.dragging());
    }

    #[test]
    fn drag_clamps_to_window() {
        let mut p = opened();
        let (x0, y0) = p.pos;
        p.begin_drag(x0 + 5.0, y0 + 5.0);
        // Drag far past the top-left corner — should clamp to (0, 0).
        p.drag_to(-500.0, -500.0, WIN_W, WIN_H);
        assert_eq!(p.pos, (0.0, 0.0));
    }

    #[test]
    fn drag_to_without_begin_is_noop() {
        let mut p = opened();
        let before = p.pos;
        p.drag_to(0.0, 0.0, WIN_W, WIN_H);
        assert_eq!(p.pos, before);
    }

    #[test]
    fn clamp_keeps_card_in_bounds() {
        let mut p = opened();
        p.pos = (5000.0, 5000.0);
        p.clamp_to_window(WIN_W, WIN_H);
        assert!(p.pos.0 + CARD_W <= WIN_W);
        assert!(p.pos.1 + CARD_H <= WIN_H);
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────────

    #[test]
    fn hit_inactive_is_none() {
        let p = PipWindow::new();
        assert_eq!(hit_test(&p, 10.0, 10.0), None);
    }

    #[test]
    fn hit_outside_card_is_none() {
        let p = opened();
        assert_eq!(hit_test(&p, p.pos.0 - 5.0, p.pos.1 - 5.0), None);
    }

    #[test]
    fn hit_close_zone() {
        let p = opened();
        let x = p.pos.0 + CARD_W - CLOSE_W * 0.5;
        let y = p.pos.1 + HEADER_H * 0.5;
        assert_eq!(hit_test(&p, x, y), Some(PipHit::Close));
    }

    #[test]
    fn hit_play_pause_centre() {
        let p = opened();
        let x = p.pos.0 + CARD_W * 0.5;
        let y = p.pos.1 + CARD_H * 0.5;
        assert_eq!(hit_test(&p, x, y), Some(PipHit::PlayPause));
    }

    #[test]
    fn hit_header_is_drag_handle() {
        let p = opened();
        // Left part of the title bar, away from the close button.
        let x = p.pos.0 + 12.0;
        let y = p.pos.1 + HEADER_H * 0.5;
        assert_eq!(hit_test(&p, x, y), Some(PipHit::Header));
    }

    #[test]
    fn hit_body_swallows() {
        let p = opened();
        // Lower-left corner: inside the card, below the header, off the button.
        let x = p.pos.0 + 8.0;
        let y = p.pos.1 + CARD_H - 8.0;
        assert_eq!(hit_test(&p, x, y), Some(PipHit::Body));
    }

    // ── Rendering ────────────────────────────────────────────────────────────────

    #[test]
    fn build_inactive_is_empty() {
        let p = PipWindow::new();
        assert!(build_panel(&p).is_empty());
    }

    #[test]
    fn build_active_emits_commands() {
        let p = opened();
        assert!(!build_panel(&p).is_empty());
    }

    #[test]
    fn build_draws_poster_image() {
        let p = opened();
        let dl = build_panel(&p);
        let has_poster = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawImage { src, .. } if src == "poster.jpg")
        });
        assert!(has_poster, "PiP must draw the poster image when present");
    }

    #[test]
    fn build_without_poster_has_no_image() {
        let mut p = PipWindow::new();
        p.open("video.mp4", "", "Tab", WIN_W, WIN_H);
        let dl = build_panel(&p);
        let has_image = dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::DrawImage { .. }));
        assert!(!has_image, "no poster → grey placeholder only, no DrawImage");
    }

    #[test]
    fn build_playing_draws_pause_bars() {
        let p = opened(); // playing
        let dl = build_panel(&p);
        // Two FillRect pause bars in addition to header bar = at least 3 FillRect.
        let fills = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .count();
        assert!(fills >= 3, "playing state draws two pause bars");
    }

    #[test]
    fn build_paused_draws_play_triangle() {
        let mut p = opened();
        p.toggle_play(); // now paused
        let dl = build_panel(&p);
        let has_triangle = dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::DrawSvgPath { .. }));
        assert!(has_triangle, "paused state draws a play triangle");
    }

    #[test]
    fn build_draws_title() {
        let p = opened();
        let dl = build_panel(&p);
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "My Tab")
        });
        assert!(has_title);
    }

    #[test]
    fn build_empty_title_uses_fallback() {
        let mut p = PipWindow::new();
        p.open("v.mp4", "", "", WIN_W, WIN_H);
        let dl = build_panel(&p);
        let has_fallback = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Picture-in-Picture")
        });
        assert!(has_fallback);
    }

    // ── Helpers ──────────────────────────────────────────────────────────────────

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hi", 40), "hi");
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        let s = "a".repeat(60);
        let t = truncate(&s, 10);
        assert_eq!(t.chars().count(), 10);
        assert!(t.ends_with('…'));
    }
}
