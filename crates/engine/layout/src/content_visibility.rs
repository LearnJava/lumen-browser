//! CSS Containment L3 §4.4 — `content-visibility: auto` skip rendering (BB-4).
//!
//! An element with `content-visibility: auto` that is not *relevant to the user*
//! (its border box does not intersect the viewport expanded by a slack band)
//! skips layout of its contents: the element keeps its own box (explicit
//! `width`/`height` still apply; auto height collapses, as per spec without
//! `contain-intrinsic-size`), but its children are dropped from the box tree
//! for this pass, so paint emits nothing for the subtree.
//!
//! Scope:
//! * A box is skipped when it lies entirely outside the viewport expanded by
//!   the slack band: its flow top is **below** the band, or — when its
//!   block-size is known before layout ([`cv_bottom_estimate`]: an absolute
//!   `height` or a `contain-intrinsic-height` placeholder) — its estimated
//!   bottom is **above** the band. A box whose height cannot be known up front
//!   is never skipped from above: laying it out is the only way to learn
//!   where its bottom is.
//! * Relevance is a shell-side ratchet: once a node becomes relevant it stays
//!   laid out (`set_cv_relevant`), avoiding scroll-position oscillation.
//!
//! Shell protocol (one layout pass):
//! 1. `set_cv_scroll(x, y)` — root scroll offset so the relevance check uses the
//!    *current* viewport, not the scroll-0 viewport.
//! 2. `set_cv_relevant(set)` — nodes forced relevant (ratchet, persisted by shell).
//! 3. run layout (`layout_measured_hyp` / `layout`).
//! 4. `take_cv_skipped()` — drain `(node, collapsed_top_y)` of skipped subtrees;
//!    the shell diffs this against the previous pass and emits
//!    `ContentVisibilityChange` events, and on scroll checks whether a skipped
//!    top entered the expanded viewport → mark relevant + relayout.

use std::cell::RefCell;
use std::collections::HashSet;

use lumen_core::geom::Size;
use lumen_dom::NodeId;

use crate::style::{BoxSizing, ComputedStyle, Length};

/// Slack band as a fraction of viewport height added above and below the
/// viewport when deciding relevance. 0.5 ⇒ contents within half a screen of
/// either edge are laid out eagerly so scrolling reveals them without a
/// visible pop-in.
pub const CV_SLACK_FACTOR: f32 = 0.5;

thread_local! {
    /// Root scroll offset `(x, y)` in CSS px for the current layout pass.
    static CV_SCROLL: RefCell<(f32, f32)> = const { RefCell::new((0.0, 0.0)) };

    /// Nodes the shell has marked relevant (ratchet): never skipped.
    static CV_RELEVANT: RefCell<HashSet<NodeId>> = RefCell::new(HashSet::new());

    /// `(node, collapsed top y)` recorded for every subtree skipped this pass.
    static CV_SKIPPED: RefCell<Vec<(NodeId, f32)>> = const { RefCell::new(Vec::new()) };
}

/// Set the root scroll offset used by the relevance check for the next layout
/// pass. The shell calls this right before `layout_measured_hyp`.
pub fn set_cv_scroll(x: f32, y: f32) {
    CV_SCROLL.with(|c| *c.borrow_mut() = (x, y));
}

/// Install the set of nodes the shell considers relevant (ratchet set).
/// These are never skipped even when off-screen.
pub fn set_cv_relevant(nodes: HashSet<NodeId>) {
    CV_RELEVANT.with(|c| *c.borrow_mut() = nodes);
}

/// Clear per-pass skip records. Called at the start of every public layout
/// entry point so repeated passes (container queries, tests) don't accumulate.
pub(crate) fn reset_cv_skipped() {
    CV_SKIPPED.with(|c| c.borrow_mut().clear());
}

/// Drain the skip records of the last layout pass: `(node, collapsed_top_y)`,
/// deduplicated by node (container-query re-layout can visit a box twice; the
/// last recorded position wins). Top y is in page coordinates (scroll 0).
pub fn take_cv_skipped() -> Vec<(NodeId, f32)> {
    let raw = CV_SKIPPED.with(|c| std::mem::take(&mut *c.borrow_mut()));
    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut out: Vec<(NodeId, f32)> = Vec::with_capacity(raw.len());
    for &(node, top) in raw.iter().rev() {
        if seen.insert(node) {
            out.push((node, top));
        }
    }
    out.reverse();
    out
}

/// Relevance check for one box: returns `true` when the subtree must be
/// skipped — i.e. the node is not in the ratchet set and the box lies outside
/// the viewport expanded by [`CV_SLACK_FACTOR`] (see [`cv_is_skipped`]).
/// `bottom` is [`cv_bottom_estimate`] of the box. Records the node in the skip
/// list when skipping.
pub(crate) fn cv_should_skip(node: NodeId, start_y: f32, bottom: f32, viewport_h: f32) -> bool {
    let relevant = CV_RELEVANT.with(|c| c.borrow().contains(&node));
    let (_sx, sy) = CV_SCROLL.with(|c| *c.borrow());
    if cv_is_skipped(relevant, start_y, bottom, sy, viewport_h) {
        CV_SKIPPED.with(|c| c.borrow_mut().push((node, start_y)));
        true
    } else {
        false
    }
}

/// Estimated bottom edge (page coordinates) of a `content-visibility: auto`
/// box whose flow top is `top`, known **before** its contents are laid out:
/// `top` + border-box block-size taken from an absolute `height`, else from the
/// `contain-intrinsic-height` placeholder (the box is size-contained while
/// skipped, so that placeholder *is* its height). `f32::INFINITY` when the
/// height cannot be known up front — `min-height`, `aspect-ratio`, a
/// percentage/`calc()` size or no placeholder at all — which makes the box
/// ineligible for the above-viewport skip.
pub fn cv_bottom_estimate(style: &ComputedStyle, top: f32, viewport: Size) -> f32 {
    if style.min_height.is_some() || style.aspect_ratio.is_some() {
        return f32::INFINITY;
    }
    let em = style.font_size;
    let (specified, is_content_box) =
        match style.height.as_ref().and_then(|l| l.resolve(em, None, viewport)) {
            Some(h) => (h, style.box_sizing == BoxSizing::ContentBox),
            None => match style
                .contain_intrinsic_height
                .as_ref()
                .and_then(|l| l.resolve(em, None, viewport))
            {
                // `contain-intrinsic-height` is a content-box size (CSS Sizing L4 §5).
                Some(h) => (h, true),
                None => return f32::INFINITY,
            },
        };
    let mut h = specified.max(0.0);
    if is_content_box {
        let pad = |l: &Length| l.resolve(em, None, viewport).unwrap_or(0.0);
        h += pad(&style.padding_top)
            + pad(&style.padding_bottom)
            + style.border_top_width
            + style.border_bottom_width;
    }
    top + h
}

/// The relevance rule itself, with no thread-local state: `true` when a
/// `content-visibility: auto` box spanning `top..bottom` (page coordinates,
/// `bottom` from [`cv_bottom_estimate`], `INFINITY` when unknown) is *not*
/// relevant to the user and its contents are therefore skipped: it starts
/// below the viewport expanded by [`CV_SLACK_FACTOR`], or ends above it.
///
/// Split out of [`cv_should_skip`] so the shell can answer the same question
/// for a box layout never asked about (BUG-852): layout consults the rule only
/// for a box that *has* children — there is nothing to skip otherwise — while
/// the `contentvisibilityautostatechange` event is owed to every
/// `content-visibility: auto` element, empty ones included. Two copies of this
/// rule would drift; there is one.
pub fn cv_is_skipped(
    relevant: bool,
    top: f32,
    bottom: f32,
    scroll_y: f32,
    viewport_h: f32,
) -> bool {
    if relevant {
        return false;
    }
    top > scroll_y + viewport_h * (1.0 + CV_SLACK_FACTOR)
        || bottom < scroll_y - viewport_h * CV_SLACK_FACTOR
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nid(n: usize) -> NodeId {
        NodeId::from_index(n)
    }

    #[test]
    fn skip_below_expanded_viewport() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 0.0);
        set_cv_relevant(HashSet::new());
        // viewport 720 ⇒ bound = 1080; 1200 > 1080 ⇒ skip.
        assert!(cv_should_skip(nid(1), 1200.0, f32::INFINITY, 720.0));
        let skipped = take_cv_skipped();
        assert_eq!(skipped, vec![(nid(1), 1200.0)]);
    }

    #[test]
    fn no_skip_within_slack_band() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 0.0);
        set_cv_relevant(HashSet::new());
        // 1000 ≤ 1080 ⇒ laid out.
        assert!(!cv_should_skip(nid(2), 1000.0, f32::INFINITY, 720.0));
        assert!(take_cv_skipped().is_empty());
    }

    #[test]
    fn scroll_offset_moves_the_bound() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 4000.0);
        set_cv_relevant(HashSet::new());
        // bound = 4000 + 1080 = 5080 ⇒ 4500 is laid out, 6000 is skipped.
        assert!(!cv_should_skip(nid(3), 4500.0, f32::INFINITY, 720.0));
        assert!(cv_should_skip(nid(4), 6000.0, f32::INFINITY, 720.0));
        set_cv_scroll(0.0, 0.0);
    }

    #[test]
    fn relevant_ratchet_prevents_skip() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 0.0);
        let mut rel = HashSet::new();
        rel.insert(nid(5));
        set_cv_relevant(rel);
        assert!(!cv_should_skip(nid(5), 9000.0, f32::INFINITY, 720.0));
        set_cv_relevant(HashSet::new());
    }

    #[test]
    fn take_drains_and_dedups_keeping_last_position() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 0.0);
        set_cv_relevant(HashSet::new());
        assert!(cv_should_skip(nid(6), 2000.0, f32::INFINITY, 720.0));
        // Container-query second pass sees the same node at a shifted position.
        assert!(cv_should_skip(nid(6), 2100.0, f32::INFINITY, 720.0));
        assert!(cv_should_skip(nid(7), 3000.0, f32::INFINITY, 720.0));
        let skipped = take_cv_skipped();
        assert_eq!(skipped, vec![(nid(6), 2100.0), (nid(7), 3000.0)]);
        // Drained: second take is empty.
        assert!(take_cv_skipped().is_empty());
    }

    #[test]
    fn skip_above_expanded_viewport() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 4000.0);
        set_cv_relevant(HashSet::new());
        // viewport 720 ⇒ lower bound = 4000 − 360 = 3640.
        // Ends at 3600 < 3640 ⇒ skipped; ends at 3700 ⇒ inside the band.
        assert!(cv_should_skip(nid(8), 3000.0, 3600.0, 720.0));
        assert!(!cv_should_skip(nid(9), 3000.0, 3700.0, 720.0));
        assert_eq!(take_cv_skipped(), vec![(nid(8), 3000.0)]);
        set_cv_scroll(0.0, 0.0);
    }

    #[test]
    fn unknown_height_is_never_skipped_from_above() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 4000.0);
        set_cv_relevant(HashSet::new());
        assert!(!cv_should_skip(nid(10), 100.0, f32::INFINITY, 720.0));
        assert!(take_cv_skipped().is_empty());
        set_cv_scroll(0.0, 0.0);
    }

    #[test]
    fn relevant_ratchet_prevents_above_skip() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 4000.0);
        set_cv_relevant(HashSet::from([nid(11)]));
        assert!(!cv_should_skip(nid(11), 0.0, 100.0, 720.0));
        set_cv_relevant(HashSet::new());
        set_cv_scroll(0.0, 0.0);
    }

    fn vp() -> Size {
        Size::new(1000.0, 720.0)
    }

    #[test]
    fn bottom_estimate_from_intrinsic_height_adds_box_extras() {
        let mut s = ComputedStyle::root();
        s.contain_intrinsic_height = Some(Length::Px(200.0));
        s.padding_top = Length::Px(10.0);
        s.padding_bottom = Length::Px(10.0);
        s.border_top_width = 1.0;
        s.border_bottom_width = 1.0;
        assert_eq!(cv_bottom_estimate(&s, 500.0, vp()), 722.0);
    }

    #[test]
    fn bottom_estimate_from_absolute_height_respects_box_sizing() {
        let mut s = ComputedStyle::root();
        s.height = Some(Length::Px(100.0));
        s.padding_top = Length::Px(10.0);
        s.box_sizing = BoxSizing::BorderBox;
        assert_eq!(cv_bottom_estimate(&s, 50.0, vp()), 150.0);
        s.box_sizing = BoxSizing::ContentBox;
        assert_eq!(cv_bottom_estimate(&s, 50.0, vp()), 160.0);
    }

    #[test]
    fn bottom_estimate_is_unknown_without_a_known_height() {
        let mut s = ComputedStyle::root();
        assert_eq!(cv_bottom_estimate(&s, 0.0, vp()), f32::INFINITY);
        s.height = Some(Length::Px(100.0));
        s.min_height = Some(Length::Px(300.0));
        assert_eq!(cv_bottom_estimate(&s, 0.0, vp()), f32::INFINITY);
    }
}
