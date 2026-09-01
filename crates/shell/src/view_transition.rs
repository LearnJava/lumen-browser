//! The shell side of a CSS View Transition: the cross-fade the compositor is
//! running ([`ViewTransitionState`]) and the three moments the page's
//! `document.startViewTransition` reports ([`ViewTransitionEvent`]).
//!
//! The event enum mirrors `lumen_js::ViewTransitionEvent` rather than reusing
//! it: `crate::persistent_js` converts one into the other at the JS boundary,
//! which keeps the shell free of a `lumen-js` type in its own state.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`; only visibility
//! changed.

/// State for an in-progress CSS View Transition cross-fade (CSS View Transitions L1).
///
/// Holds the captured old display list and timing parameters.
pub(crate) struct ViewTransitionState {
    /// Display list captured before the JS callback mutated the DOM.
    pub(crate) old_dl: lumen_paint::DisplayList,
    /// Wall-clock epoch offset (ms) when the cross-fade animation started.
    pub(crate) start_ms: f64,
    /// Total cross-fade duration in milliseconds (currently 300 ms).
    pub(crate) duration_ms: f64,
}

/// CSS View Transitions L1 — event kind emitted by `document.startViewTransition`.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ViewTransitionEvent {
    /// Callback is about to run — shell should snapshot the current frame.
    Begin,
    /// Callback finished — shell should relayout and start the cross-fade animation.
    End,
    /// Transition was cancelled (nested startViewTransition or explicit abort).
    Cancel,
}
