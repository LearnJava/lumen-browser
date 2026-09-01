//! Geometry decisions derived from the window surface: the CSS-px layout
//! viewport left for the page under the browser chrome, and the poll tick that
//! reconciles an asynchronous fullscreen resize (BUG-167).
//!
//! Both are pure functions of sizes, which is what lets the fullscreen
//! reconciliation be unit-tested without a real window.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// Outcome of a single fullscreen-resize poll tick (BUG-167).
///
/// Pure decision extracted from [`Lumen::poll_fullscreen_resize`] so the
/// async-resize reconciliation can be unit-tested without a real window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FullscreenPoll {
    /// The OS applied a new size: resize the renderer to `(w, h)` physical px
    /// and relayout. Clears the pending state.
    Apply(u32, u32),
    /// The size has not been applied yet: keep waiting with `(prev_w, prev_h,
    /// attempts_left)` (one attempt spent this tick).
    Wait(u32, u32, u8),
    /// Give up — the attempt budget is exhausted. Clears the pending state.
    Done,
}

/// Decide what to do on one fullscreen-resize poll tick (BUG-167).
///
/// `prev` is the window's physical inner size captured before `set_fullscreen`;
/// `cur` is the size read this tick; `attempts` is the remaining poll budget.
/// A zero-sized `cur` (minimized / not yet mapped) counts as "not applied yet".
pub(crate) fn decide_fullscreen_poll(prev: (u32, u32), cur: (u32, u32), attempts: u8) -> FullscreenPoll {
    let (prev_w, prev_h) = prev;
    let (cur_w, cur_h) = cur;
    if cur_w != 0 && cur_h != 0 && (cur_w != prev_w || cur_h != prev_h) {
        return FullscreenPoll::Apply(cur_w, cur_h);
    }
    match attempts.checked_sub(1) {
        Some(0) | None => FullscreenPoll::Done,
        Some(left) => FullscreenPoll::Wait(prev_w, prev_h, left),
    }
}

/// CSS-px layout viewport (width, height) for the page content region, derived
/// from the full renderer surface size `surface` (already in CSS px).
///
/// In an interactive window the page is composited *below* the browser chrome:
/// the tab strip + toolbar (`toolbar::CHROME_H`) always, plus the workspace
/// switcher (`SWITCHER_HEIGHT`) when visible. The page content is shifted down by that
/// chrome via `PushTransform`, and scroll clamping uses the same reduced height
/// (`viewport_height_css`). The layout pass must therefore see the *content*
/// height — not the full window — so that `vh`/`%`-heights/`@media (height)`
/// resolve against the actually-visible page region. Width is unaffected (the
/// chrome only occupies vertical space).
///
/// Headless surfaces (`--screenshot` / `--dump-*` / `--ipc-server`, i.e.
/// `has_window == false`) have no chrome: the full surface is the viewport,
/// which keeps those paths deterministic at 1024×720.
pub(crate) fn content_layout_viewport(surface: Size, has_window: bool, workspace_visible: bool) -> (f32, f32) {
    if !has_window {
        return (surface.width, surface.height);
    }
    let chrome_h = toolbar::CHROME_H
        + if workspace_visible {
            panels::workspace_panel::SWITCHER_HEIGHT
        } else {
            0.0
        };
    (surface.width, (surface.height - chrome_h).max(0.0))
}
