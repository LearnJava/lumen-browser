//! Picture-in-picture floating video window (task #21).
//!
//! A compact 320×180 (16:9) card that floats above the page and keeps a tab's
//! `<video>` element visible while the user scrolls the page or switches to
//! another tab.  Conceptually this is ADR-009's `Surface::OsWindow`, but the
//! `Surface` trait does not yet exist in code, so — like every other shell
//! panel (`focus_panel`, `command_palette`, …) — it is implemented ad-hoc:
//! state lives on `Lumen`, the card itself is `#pipCard` in the engine chrome
//! ([`PipWindow::chrome_model`], `bind_pip`) and clicks are classified by
//! [`PipHit::from_attr`] off its `data-pip` attributes.  It is an **in-window
//! overlay**, not a separate OS window; a real second winit window awaits multi-window support in the shell
//! event loop (the genuine blocker on the spec's `Surface::OsWindow`).
//!
//! The card shows the video's poster image (the `<video>` placeholder is a grey
//! frame until real playback exists), a translucent title bar with a `×` close
//! button, and a centred play / pause button.  It can be dragged anywhere inside
//! the window by its title bar.  Toggled with `Ctrl+Shift+V`.

use lumen_chrome::ChromePipModel;

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the PiP card in CSS px.
pub const CARD_W: f32 = 320.0;

/// Height of the PiP card in CSS px (16:9 with `CARD_W`).
pub const CARD_H: f32 = 180.0;

/// Margin from the window edge to the card when first opened, in CSS px.
const MARGIN: f32 = 16.0;

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

// ── Chrome binding ────────────────────────────────────────────────────────────

impl PipWindow {
    /// Snapshot for `#pipCard`.
    pub fn chrome_model(&self) -> ChromePipModel {
        ChromePipModel {
            open: self.active,
            x: self.pos.0,
            y: self.pos.1,
            playing: self.playing,
            title: self.title.clone(),
            poster: self.poster_url.clone(),
        }
    }
}

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

impl PipHit {
    /// Parse a `data-pip` attribute value of `#pipCard` or one of its parts.
    pub fn from_attr(v: &str) -> Option<Self> {
        match v {
            "close" => Some(Self::Close),
            "play" => Some(Self::PlayPause),
            "header" => Some(Self::Header),
            "body" => Some(Self::Body),
            _ => None,
        }
    }
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

    // ── Chrome binding ───────────────────────────────────────────────────────────

    #[test]
    fn hit_from_attr_maps_all_parts() {
        assert_eq!(PipHit::from_attr("close"), Some(PipHit::Close));
        assert_eq!(PipHit::from_attr("play"), Some(PipHit::PlayPause));
        assert_eq!(PipHit::from_attr("header"), Some(PipHit::Header));
        assert_eq!(PipHit::from_attr("body"), Some(PipHit::Body));
        assert_eq!(PipHit::from_attr("x"), None);
    }

    #[test]
    fn chrome_model_mirrors_state() {
        assert!(!PipWindow::new().chrome_model().open);
        let m = opened().chrome_model();
        assert!(m.open && m.playing);
        assert_eq!((m.title.as_str(), m.poster.as_str()), ("My Tab", "poster.jpg"));
        assert_eq!((m.x, m.y), PipWindow::default_pos(WIN_W, WIN_H));
    }
}
