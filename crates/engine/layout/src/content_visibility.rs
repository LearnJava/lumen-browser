//! CSS Containment L3 §4.4 — `content-visibility: auto` skip rendering (BB-4).
//!
//! An element with `content-visibility: auto` that is not *relevant to the user*
//! (its border box does not intersect the viewport expanded by a slack band)
//! skips layout of its contents: the element keeps its own box (explicit
//! `width`/`height` still apply; auto height collapses, as per spec without
//! `contain-intrinsic-size`), but its children are dropped from the box tree
//! for this pass, so paint emits nothing for the subtree.
//!
//! Phase 0 scope:
//! * Only boxes whose flow position starts **below** the expanded viewport are
//!   skipped (above-viewport skipping needs the box height before layout, which
//!   would require a separate estimation pass).
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

use lumen_dom::NodeId;

/// Slack band as a fraction of viewport height added below the viewport when
/// deciding relevance. 0.5 ⇒ contents within half a screen of the bottom edge
/// are laid out eagerly so scrolling reveals them without a visible pop-in.
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
/// skipped — i.e. the node is not in the ratchet set and its flow top
/// (`start_y`, page coordinates) lies below the viewport bottom expanded by
/// [`CV_SLACK_FACTOR`]. Records the node in the skip list when skipping.
pub(crate) fn cv_should_skip(node: NodeId, start_y: f32, viewport_h: f32) -> bool {
    let relevant = CV_RELEVANT.with(|c| c.borrow().contains(&node));
    if relevant {
        return false;
    }
    let (_sx, sy) = CV_SCROLL.with(|c| *c.borrow());
    let bottom_bound = sy + viewport_h * (1.0 + CV_SLACK_FACTOR);
    if start_y > bottom_bound {
        CV_SKIPPED.with(|c| c.borrow_mut().push((node, start_y)));
        true
    } else {
        false
    }
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
        assert!(cv_should_skip(nid(1), 1200.0, 720.0));
        let skipped = take_cv_skipped();
        assert_eq!(skipped, vec![(nid(1), 1200.0)]);
    }

    #[test]
    fn no_skip_within_slack_band() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 0.0);
        set_cv_relevant(HashSet::new());
        // 1000 ≤ 1080 ⇒ laid out.
        assert!(!cv_should_skip(nid(2), 1000.0, 720.0));
        assert!(take_cv_skipped().is_empty());
    }

    #[test]
    fn scroll_offset_moves_the_bound() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 4000.0);
        set_cv_relevant(HashSet::new());
        // bound = 4000 + 1080 = 5080 ⇒ 4500 is laid out, 6000 is skipped.
        assert!(!cv_should_skip(nid(3), 4500.0, 720.0));
        assert!(cv_should_skip(nid(4), 6000.0, 720.0));
        set_cv_scroll(0.0, 0.0);
    }

    #[test]
    fn relevant_ratchet_prevents_skip() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 0.0);
        let mut rel = HashSet::new();
        rel.insert(nid(5));
        set_cv_relevant(rel);
        assert!(!cv_should_skip(nid(5), 9000.0, 720.0));
        set_cv_relevant(HashSet::new());
    }

    #[test]
    fn take_drains_and_dedups_keeping_last_position() {
        reset_cv_skipped();
        set_cv_scroll(0.0, 0.0);
        set_cv_relevant(HashSet::new());
        assert!(cv_should_skip(nid(6), 2000.0, 720.0));
        // Container-query second pass sees the same node at a shifted position.
        assert!(cv_should_skip(nid(6), 2100.0, 720.0));
        assert!(cv_should_skip(nid(7), 3000.0, 720.0));
        let skipped = take_cv_skipped();
        assert_eq!(skipped, vec![(nid(6), 2100.0), (nid(7), 3000.0)]);
        // Drained: second take is empty.
        assert!(take_cv_skipped().is_empty());
    }
}
