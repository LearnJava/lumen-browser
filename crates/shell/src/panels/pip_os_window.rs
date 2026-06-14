//! Real OS-level picture-in-picture window (CC-7).
//!
//! Where [`pip_window`](super::pip_window) draws an *in-window overlay* card,
//! this module backs the genuine `Surface::OsWindow` from ADR-009: a separate,
//! always-on-top `winit::Window` that floats above every other application and
//! keeps a tab's `<video>` element visible after the user switches away from the
//! Lumen window entirely.
//!
//! ## Pieces
//!
//! * [`PipController`] — the enter/exit state machine driven by the JS bindings
//!   `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)` (mirrors the Fullscreen API
//!   request drain). Pure data; holds no OS handles, so it is fully unit-tested.
//! * [`PipOsConfig`] + [`pip_window_attributes`] — the winit window description.
//!   On Windows winit maps [`WindowLevel::AlwaysOnTop`] to the `WS_EX_TOPMOST`
//!   extended style and `with_decorations(false)` to a borderless `WS_POPUP`-style
//!   frame, matching the spec's request for a topmost popup.
//! * [`build_pip_content`] — the forwarded `<video>` display-list subtree, scaled
//!   (object-fit: contain, letterboxed) into the floating window's client area.
//!
//! The shell owns the live `winit::Window` + [`RenderBackend`] pair (see
//! `PipOsWindow` in `main.rs`); this module owns everything that can be reasoned
//! about — and tested — without a GPU surface.
//!
//! [`RenderBackend`]: lumen_paint::RenderBackend
//! [`WindowLevel::AlwaysOnTop`]: winit::window::WindowLevel::AlwaysOnTop

use lumen_core::geom::Rect;
use lumen_layout::{Color, ImageRendering, ObjectFit, ObjectPosition};
use lumen_paint::{DisplayCommand, DisplayList};
use winit::window::{WindowAttributes, WindowLevel};

// ── Win32 styles (informational; winit applies the equivalents) ────────────────

/// `WS_EX_TOPMOST` — keeps the PiP window above all non-topmost windows.
///
/// winit applies this automatically for [`WindowLevel::AlwaysOnTop`]; the bare
/// constant is kept so the mapping is explicit and testable. (Documentation
/// constant — the shell is a binary, so it is referenced only by tests.)
#[allow(dead_code)]
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;

/// `WS_POPUP` — a borderless popup frame (no title bar / system menu).
///
/// winit produces an equivalent frame via `with_decorations(false)`.
#[allow(dead_code)]
pub const WS_POPUP: u32 = 0x8000_0000;

// ── Window configuration ───────────────────────────────────────────────────────

/// Geometry for the floating PiP window, in logical (CSS) pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PipOsConfig {
    /// Initial client width.
    pub width: f32,
    /// Initial client height.
    pub height: f32,
    /// Minimum client width the user may shrink the window to.
    pub min_width: f32,
    /// Minimum client height the user may shrink the window to.
    pub min_height: f32,
}

impl PipOsConfig {
    /// Default 400×225 (16:9) card with a 192×108 floor.
    pub const DEFAULT: Self = Self {
        width: 400.0,
        height: 225.0,
        min_width: 192.0,
        min_height: 108.0,
    };
}

impl Default for PipOsConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Build the winit attributes for the floating PiP window.
///
/// * always-on-top ([`WindowLevel::AlwaysOnTop`] → `WS_EX_TOPMOST`),
/// * borderless ([`with_decorations(false)`] → `WS_POPUP`-style frame),
/// * resizable down to [`PipOsConfig::min_width`] × [`PipOsConfig::min_height`].
///
/// [`with_decorations(false)`]: winit::window::WindowAttributes::with_decorations
pub fn pip_window_attributes(title: &str, cfg: PipOsConfig) -> WindowAttributes {
    use winit::dpi::LogicalSize;
    WindowAttributes::default()
        .with_title(title.to_owned())
        .with_inner_size(LogicalSize::new(cfg.width, cfg.height))
        .with_min_inner_size(LogicalSize::new(cfg.min_width, cfg.min_height))
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_decorations(false)
        .with_resizable(true)
}

// ── Forwarded `<video>` content ────────────────────────────────────────────────

/// Background fill for the floating window when the poster is absent or
/// letterboxed (matches the overlay card's `VIDEO_BG`).
const VIDEO_BG: Color = Color { r: 24, g: 24, b: 30, a: 255 };

/// Build the display list shown in the floating PiP window for a `<video>`.
///
/// The source `<video>` border-box is `video_rect` (page coordinates); only its
/// aspect ratio is used. The poster (or grey placeholder) is scaled into the
/// `win_w`×`win_h` client area with `object-fit: contain` semantics — preserving
/// aspect ratio and letterboxing the remainder — so the floating window never
/// distorts the frame regardless of how the user resizes it.
///
/// Coordinates are window-local: the returned commands start at `(0, 0)`, ready
/// to hand straight to the PiP window's own [`RenderBackend::render`] overlay.
///
/// [`RenderBackend::render`]: lumen_paint::RenderBackend::render
pub fn build_pip_content(video_rect: Rect, poster_url: &str, win_w: f32, win_h: f32) -> DisplayList {
    let mut out = DisplayList::with_capacity(2);
    if win_w <= 0.0 || win_h <= 0.0 {
        return out;
    }

    // Opaque background fills the whole client area (also the letterbox bars).
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, win_w, win_h),
        color: VIDEO_BG,
    });

    if poster_url.is_empty() {
        return out;
    }

    let dest = contain_rect(video_rect.width, video_rect.height, win_w, win_h);
    out.push(DisplayCommand::DrawImage {
        rect: dest,
        src: poster_url.to_owned(),
        alt: String::new(),
        object_fit: ObjectFit::Contain,
        object_position: ObjectPosition::default(),
        image_rendering: ImageRendering::Auto,
    });
    out
}

/// `object-fit: contain` destination rect: scale `src_w`×`src_h` to fit inside
/// `win_w`×`win_h` preserving aspect ratio, centred (letterboxed / pillarboxed).
///
/// Falls back to filling the window when the source has no positive dimensions.
fn contain_rect(src_w: f32, src_h: f32, win_w: f32, win_h: f32) -> Rect {
    if src_w <= 0.0 || src_h <= 0.0 {
        return Rect::new(0.0, 0.0, win_w, win_h);
    }
    let scale = (win_w / src_w).min(win_h / src_h);
    let dw = src_w * scale;
    let dh = src_h * scale;
    let dx = (win_w - dw) * 0.5;
    let dy = (win_h - dh) * 0.5;
    Rect::new(dx, dy, dw, dh)
}

// ── Enter / exit state machine ─────────────────────────────────────────────────

/// What the shell should do after feeding a request into [`PipController`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipAction {
    /// Create (or re-target) the floating window for the `<video>` node `nid`.
    Open(u32),
    /// Tear the floating window down — no `<video>` is in PiP anymore.
    Close,
    /// Nothing changed (e.g. exit while already closed).
    None,
}

/// Tracks which `<video>` (by node id) currently owns the OS PiP window.
///
/// Per the W3C Picture-in-Picture spec only one element may be in PiP at a time,
/// so entering for a new node while one is active re-targets the same floating
/// window rather than opening a second one.
#[derive(Debug, Default, Clone)]
pub struct PipController {
    /// Node id of the `<video>` currently in OS PiP, or `None` when closed.
    active: Option<u32>,
}

impl PipController {
    /// Create an idle controller with no active PiP window.
    pub fn new() -> Self {
        Self { active: None }
    }

    /// Node id of the element currently in OS PiP, or `None`.
    ///
    /// (Accessor exercised by tests; the binary drives state through
    /// [`on_enter`](Self::on_enter) / [`on_exit`](Self::on_exit).)
    #[allow(dead_code)]
    pub fn active(&self) -> Option<u32> {
        self.active
    }

    /// `true` while an OS PiP window should be shown.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Handle `_lumen_pip_enter(nid)`: open or re-target the floating window.
    pub fn on_enter(&mut self, nid: u32) -> PipAction {
        self.active = Some(nid);
        PipAction::Open(nid)
    }

    /// Handle `_lumen_pip_exit(_)` or an OS close button: tear the window down.
    ///
    /// Returns [`PipAction::None`] when nothing was open, so the shell can skip a
    /// redundant teardown / JS notification.
    pub fn on_exit(&mut self) -> PipAction {
        if self.active.take().is_some() {
            PipAction::Close
        } else {
            PipAction::None
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config / attributes ──────────────────────────────────────────────────────

    #[test]
    fn default_config_is_16_9() {
        let c = PipOsConfig::DEFAULT;
        assert!((c.width / c.height - 16.0 / 9.0).abs() < 1e-3);
        assert!(c.min_width <= c.width && c.min_height <= c.height);
    }

    #[test]
    fn attributes_are_topmost_borderless() {
        let attrs = pip_window_attributes("PiP", PipOsConfig::DEFAULT);
        assert_eq!(attrs.window_level, WindowLevel::AlwaysOnTop);
        assert!(!attrs.decorations, "PiP window must be borderless (WS_POPUP-style)");
        assert!(attrs.resizable);
        assert_eq!(attrs.title, "PiP");
    }

    #[test]
    fn win32_style_constants() {
        // Sanity: the documented Win32 equivalents have their canonical values.
        assert_eq!(WS_EX_TOPMOST, 0x0000_0008);
        assert_eq!(WS_POPUP, 0x8000_0000);
    }

    // ── Forwarded content ────────────────────────────────────────────────────────

    #[test]
    fn content_without_poster_is_background_only() {
        let dl = build_pip_content(Rect::new(0.0, 0.0, 640.0, 360.0), "", 400.0, 225.0);
        assert_eq!(dl.len(), 1);
        assert!(matches!(dl[0], DisplayCommand::FillRect { .. }));
    }

    #[test]
    fn content_with_poster_draws_letterboxed_image() {
        // 16:9 source into a 1:1 window → pillarbox: image narrower than window,
        // full height, horizontally centred.
        let dl = build_pip_content(Rect::new(0.0, 0.0, 640.0, 360.0), "poster.jpg", 200.0, 200.0);
        let img = dl.iter().find_map(|c| match c {
            DisplayCommand::DrawImage { rect, src, .. } if src == "poster.jpg" => Some(*rect),
            _ => None,
        });
        let r = img.expect("poster image must be drawn");
        // contain scale = min(200/640, 200/360) = 200/640 → height 112.5, width 200.
        assert!((r.width - 200.0).abs() < 0.1, "width fills the window");
        assert!((r.height - 112.5).abs() < 0.1, "height letterboxed by aspect ratio");
        assert!(r.x.abs() < 0.1, "no horizontal padding (width fills)");
        assert!((r.y - (200.0 - 112.5) / 2.0).abs() < 0.1, "vertically centred");
    }

    #[test]
    fn content_zero_window_is_empty() {
        let dl = build_pip_content(Rect::new(0.0, 0.0, 640.0, 360.0), "p.jpg", 0.0, 0.0);
        assert!(dl.is_empty());
    }

    #[test]
    fn contain_rect_centres_pillarbox() {
        // Square source into wide window → pillarbox (bars left/right).
        let r = contain_rect(100.0, 100.0, 400.0, 200.0);
        assert!((r.height - 200.0).abs() < 0.1);
        assert!((r.width - 200.0).abs() < 0.1);
        assert!((r.x - 100.0).abs() < 0.1);
        assert!(r.y.abs() < 0.1);
    }

    // ── Controller state machine ─────────────────────────────────────────────────

    #[test]
    fn new_controller_is_idle() {
        let c = PipController::new();
        assert!(!c.is_active());
        assert_eq!(c.active(), None);
    }

    #[test]
    fn enter_opens_and_records_node() {
        let mut c = PipController::new();
        assert_eq!(c.on_enter(7), PipAction::Open(7));
        assert!(c.is_active());
        assert_eq!(c.active(), Some(7));
    }

    #[test]
    fn enter_while_active_retargets_same_window() {
        let mut c = PipController::new();
        c.on_enter(7);
        assert_eq!(c.on_enter(9), PipAction::Open(9), "re-target, not a second window");
        assert_eq!(c.active(), Some(9));
    }

    #[test]
    fn exit_closes_active_window() {
        let mut c = PipController::new();
        c.on_enter(7);
        assert_eq!(c.on_exit(), PipAction::Close);
        assert!(!c.is_active());
    }

    #[test]
    fn exit_while_idle_is_noop() {
        let mut c = PipController::new();
        assert_eq!(c.on_exit(), PipAction::None);
    }
}
