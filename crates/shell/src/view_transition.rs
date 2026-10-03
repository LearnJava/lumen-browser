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
    /// Total cross-fade duration in milliseconds. [`DEFAULT_DURATION_MS`]
    /// unless the page restyles the pseudo-tree (see [`author_params`]).
    pub(crate) duration_ms: f64,
    /// Easing applied to the fade progress; `linear` unless the page sets
    /// `animation-timing-function` on the pseudo-tree.
    pub(crate) easing: lumen_layout::TimingFunction,
}

/// Cross-fade length when the page does not override it — the shell's
/// long-standing default, kept (rather than the spec's 250 ms) so pages that
/// never style `::view-transition-*` look exactly as before.
pub(crate) const DEFAULT_DURATION_MS: f64 = 300.0;

/// Duration and easing the page's `::view-transition-old(root)` /
/// `::view-transition-group(root)` rules ask for (CSS View Transitions L1
/// §6, §7.1): the author's `animation-duration` / `animation-timing-function`
/// on the whole-page capture. The fade is the `old` image's animation, so a
/// rule on `-old(root)` wins over one on `-group(root)`; a `*` argument
/// applies to both. `None` fields mean "no author value — keep the default".
///
/// Only the whole-page (`root`) capture is consulted: the shell cross-fades
/// the entire old display list as one layer and does not yet morph named
/// groups individually, so a per-name duration has nothing to drive.
pub(crate) fn author_params(
    doc: &lumen_dom::Document,
    sheet: &lumen_css_parser::Stylesheet,
    viewport: lumen_core::geom::Size,
    dark_mode: bool,
) -> (Option<f64>, Option<lumen_layout::TimingFunction>) {
    use lumen_layout::{compute_view_transition_pseudo_style as style_of, ViewTransitionPart};
    let mut duration = None;
    let mut easing = None;
    // Lowest priority first, so a later (more specific) part overwrites.
    for part in [ViewTransitionPart::Group, ViewTransitionPart::Old] {
        let Some(s) = style_of(doc, sheet, part, "root", viewport, dark_mode) else { continue };
        if let Some(d) = s.animation_durations.first() {
            duration = Some(f64::from(d.max(0.0)) * 1000.0);
        }
        if let Some(t) = s.animation_timing_functions.first() {
            easing = Some(t.clone());
        }
    }
    (duration, easing)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn params(css: &str) -> (Option<f64>, Option<lumen_layout::TimingFunction>) {
        let doc = lumen_html_parser::parse("<p>x</p>");
        let sheet = lumen_css_parser::parse(css);
        author_params(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0), false)
    }

    #[test]
    fn no_pseudo_rules_keeps_defaults() {
        assert_eq!(params("p { color: red; }"), (None, None));
    }

    #[test]
    fn old_root_duration_and_easing_are_picked_up() {
        let (d, e) = params(
            "::view-transition-old(root) { animation-duration: 1.5s; animation-timing-function: ease-in; }",
        );
        assert_eq!(d, Some(1500.0));
        assert_eq!(e, Some(lumen_layout::TimingFunction::parse("ease-in").unwrap()));
    }

    #[test]
    fn old_wins_over_group_and_other_names_are_ignored() {
        let (d, _) = params(
            "::view-transition-group(root) { animation-duration: 2s; }             ::view-transition-old(root) { animation-duration: 500ms; }             ::view-transition-old(hero) { animation-duration: 9s; }",
        );
        assert_eq!(d, Some(500.0));
        let (d, _) = params("::view-transition-group(root) { animation-duration: 2s; }");
        assert_eq!(d, Some(2000.0));
    }

    #[test]
    fn zero_duration_is_authored_zero() {
        assert_eq!(params("::view-transition-old(*) { animation-duration: 0s; }").0, Some(0.0));
    }
}
