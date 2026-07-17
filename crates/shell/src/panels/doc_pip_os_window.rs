//! Real OS-level Document Picture-in-Picture window (slice 1 lifecycle +
//! slice 2 content + slice 3 author CSS).
//!
//! Mirrors [`pip_os_window`](super::pip_os_window) (CC-7, video PiP): a
//! separate, always-on-top, borderless `winit::Window` floating above every
//! other application, driven by the JS bindings
//! `_lumen_docpip_request_window(width, height)` / `_lumen_docpip_close()`
//! (`documentpip_bindings.rs`, mirroring `pip_bindings.rs`).
//!
//! Unlike video PiP, `documentPictureInPicture.requestWindow()` is meant to
//! host an arbitrary moved DOM subtree, not a single `<video>` frame.
//! [`build_docpip_content`] builds only the window's opaque background fill;
//! `Lumen::render_doc_pip_os` (`shell/src/main.rs`) lays out and paints the
//! moved subtree's serialized markup on top of it each frame (slice 2 — see
//! `document_pip.rs` module docs for the JS-side content bridge).
//!
//! [`PipOsConfig`], [`pip_window_attributes`] and [`physical_to_logical`] are
//! shared verbatim with video PiP (`super::pip_os_window`) — they carry no
//! video-specific state.

use lumen_core::geom::Rect;
use lumen_layout::Color;
use lumen_paint::{DisplayCommand, DisplayList};

/// Background fill for the floating window, painted under the moved subtree.
const DOC_PIP_BG: Color = Color { r: 24, g: 24, b: 30, a: 255 };

/// Build the opaque background fill for the floating Document PiP window.
///
/// `Lumen::render_doc_pip_os` paints the moved DOM subtree's own display list
/// on top of this when `pipWindow.document.body` has content.
pub fn build_docpip_content(win_w: f32, win_h: f32) -> DisplayList {
    let mut out = DisplayList::with_capacity(1);
    if win_w <= 0.0 || win_h <= 0.0 {
        return out;
    }
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, win_w, win_h),
        color: DOC_PIP_BG,
    });
    out
}

// ── Enter / exit state machine ─────────────────────────────────────────────────

/// What the shell should do after feeding a request into [`DocPipController`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocPipAction {
    /// Create the floating window at the requested logical size.
    Open {
        /// Requested initial client width.
        width: u32,
        /// Requested initial client height.
        height: u32,
    },
    /// Tear the floating window down.
    Close,
    /// Nothing changed (e.g. close while already closed).
    None,
}

/// Tracks whether the OS Document PiP window is currently open.
///
/// Per the W3C Document Picture-in-Picture spec, `requestWindow()` throws
/// (JS-side, `document_pip.rs`) while a window is already active, so unlike
/// video PiP's [`PipController`](super::pip_os_window::PipController) there is
/// no re-targeting case — the controller only tracks open/closed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DocPipController {
    open: bool,
}

impl DocPipController {
    /// Create an idle controller with no open window.
    pub fn new() -> Self {
        Self { open: false }
    }

    /// `true` while the OS Document PiP window should be shown.
    #[allow(dead_code)]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Handle `_lumen_docpip_request_window(width, height)`.
    pub fn on_open(&mut self, width: u32, height: u32) -> DocPipAction {
        self.open = true;
        DocPipAction::Open { width, height }
    }

    /// Handle `_lumen_docpip_close()` or an OS close button.
    ///
    /// Returns [`DocPipAction::None`] when nothing was open, so the shell can
    /// skip a redundant teardown / JS notification.
    pub fn on_close(&mut self) -> DocPipAction {
        if self.open {
            self.open = false;
            DocPipAction::Close
        } else {
            DocPipAction::None
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_fills_background() {
        let dl = build_docpip_content(400.0, 225.0);
        assert_eq!(dl.len(), 1);
        assert!(matches!(dl[0], DisplayCommand::FillRect { .. }));
    }

    #[test]
    fn content_zero_window_is_empty() {
        let dl = build_docpip_content(0.0, 0.0);
        assert!(dl.is_empty());
    }

    #[test]
    fn new_controller_is_closed() {
        let c = DocPipController::new();
        assert!(!c.is_open());
    }

    #[test]
    fn open_opens_and_records_size() {
        let mut c = DocPipController::new();
        assert_eq!(c.on_open(800, 450), DocPipAction::Open { width: 800, height: 450 });
        assert!(c.is_open());
    }

    #[test]
    fn close_closes_open_window() {
        let mut c = DocPipController::new();
        c.on_open(640, 360);
        assert_eq!(c.on_close(), DocPipAction::Close);
        assert!(!c.is_open());
    }

    #[test]
    fn close_while_idle_is_noop() {
        let mut c = DocPipController::new();
        assert_eq!(c.on_close(), DocPipAction::None);
    }

    #[test]
    fn reopen_while_open_still_reopens() {
        // Spec-level "already active" guard lives in JS (document_pip.rs);
        // the controller itself just tracks state, so a second `on_open` call
        // (should the shell ever receive one) is a no-op transition, not a panic.
        let mut c = DocPipController::new();
        c.on_open(640, 360);
        assert_eq!(c.on_open(800, 450), DocPipAction::Open { width: 800, height: 450 });
        assert!(c.is_open());
    }
}
