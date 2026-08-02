//! Paint-time structural invariants over provenance (ADR-025 §4, DEVX-8b).
//!
//! This module is the acceptance test for DEVX-7: a firing assertion here
//! means the [`ProvenanceIndex`] returned by
//! [`build_display_list_ordered_dpr`](crate::display_list::build_display_list_ordered_dpr)
//! is lying about which layout box produced which command. Same rule as
//! `lumen-layout::invariants` (DEVX-8a): a firing invariant is a bug to fix
//! in `BUGS.md`, never a condition to relax. `debug_assert!`-only via the
//! `assert!`s below, gated by `cfg(debug_assertions)` at the call site in
//! `display_list.rs` — zero cost in release builds.
//!
//! ADR-025 §4 lists five properties; four are checked here
//! (`check_coverage`, `check_clip_stack_balance`, `check_origins_resolve`,
//! `check_visible_boxes_have_spans`). The fifth — "paint order within a
//! stacking context is consistent with the stacking-context tree" — is
//! **not** re-checked at the span level: in `build_display_list_ordered_dpr`,
//! command order in the final list *is* `order.steps`' iteration order (each
//! bucket field is flushed exactly once, inside that single loop, in that
//! loop's order) — there is no separate assignment step that could disagree
//! with it, so a span-level check here would only re-verify that a `for`
//! loop iterates its own vector in order. The real content of that
//! property — that `order.steps` itself encodes correct z-index/stacking
//! semantics — is already covered by the existing
//! `ordered_negative_z_child_painted_before_root_content` /
//! `ordered_positive_z_child_painted_after_root_content` / `ordered_*`
//! command-sequence tests in `display_list.rs`.

use std::collections::HashSet;

use lumen_dom::NodeId;
use lumen_layout::{BoxKind, LayoutBox};

use crate::display_list::{
    background_clip_rect, background_color_clip, is_hidden_empty_cell,
    is_opacity_subtree_painted, is_paint_visible, DisplayCommand, ProvenanceIndex,
};

/// Runs every DEVX-8b check over one freshly built display list and its
/// provenance index. Called from `build_display_list_ordered_dpr` right
/// before it returns `(out, ProvenanceIndex)`.
pub(crate) fn check(out: &[DisplayCommand], index: &ProvenanceIndex, root: &LayoutBox) {
    check_coverage(out, index);
    check_clip_stack_balance(out);
    let known_nodes = collect_node_ids(root);
    check_origins_resolve(index, &known_nodes);
    check_visible_boxes_have_spans(root, index);
}

/// ADR-025 §4 property 1 — every command belongs to exactly one span.
/// `fill_buckets` builds spans as contiguous, non-overlapping runs of a
/// bucket field by construction (every append between a `record_span`'s
/// `start`/`end` belongs to that call's origin, and the next box's own
/// span starts exactly where the previous one's ends) — so a coverage
/// count other than 1 anywhere means a command escaped bookkeeping (a
/// gap, i.e. no origin claims it) or two origins claimed the same
/// command (an overlap).
fn check_coverage(out: &[DisplayCommand], index: &ProvenanceIndex) {
    let mut covers = vec![0u16; out.len()];
    for span in index.spans() {
        assert!(
            span.range.start < span.range.end,
            "ProvenanceSpan with an empty or inverted range: {:?}",
            span.range
        );
        assert!(
            span.range.end <= out.len(),
            "ProvenanceSpan {:?} out of bounds for a display list of {} commands",
            span.range,
            out.len()
        );
        for c in &mut covers[span.range.clone()] {
            *c += 1;
        }
    }
    for (i, c) in covers.iter().enumerate() {
        assert_eq!(
            *c, 1,
            "command #{i} ({:?}) is covered by {c} provenance spans, expected exactly 1",
            out[i]
        );
    }
}

/// ADR-025 §4 property 3, the clip-stack half — `ProvenanceSpan::clip_depth`
/// (`annotate_clip_depth`) only means something if the clip / scroll-layer
/// push/pop stacks the display list encodes are themselves well-formed:
/// never popped past empty, and fully closed by the end of the list.
///
/// Span-level nesting checks stop here on purpose: "clip depth is constant
/// across one span's own range" is not a real invariant and was rejected
/// during design (`docs/tasks/p1-introspection-track.md`, DEVX-8b) — a
/// `root_bg` span legitimately includes the box's own `overflow-x/y`
/// clip-open command right after its background/border, so depth changes
/// partway through that very span.
fn check_clip_stack_balance(out: &[DisplayCommand]) {
    let mut clip_depth: i32 = 0;
    let mut scroll_depth: i32 = 0;
    for cmd in out {
        match cmd {
            DisplayCommand::PushClipRect { .. }
            | DisplayCommand::PushClipRoundedRect { .. }
            | DisplayCommand::PushClipPath { .. } => clip_depth += 1,
            DisplayCommand::PopClip => {
                clip_depth -= 1;
                assert!(clip_depth >= 0, "PopClip with no matching Push* still open");
            }
            DisplayCommand::PushScrollLayer { .. } => scroll_depth += 1,
            DisplayCommand::PopScrollLayer => {
                scroll_depth -= 1;
                assert!(
                    scroll_depth >= 0,
                    "PopScrollLayer with no matching PushScrollLayer still open"
                );
            }
            _ => {}
        }
    }
    assert_eq!(clip_depth, 0, "display list ends with {clip_depth} clip push(es) still open");
    assert_eq!(scroll_depth, 0, "display list ends with {scroll_depth} scroll layer(s) still open");
}

/// Collects every `LayoutBox::node` in the tree — the universe of node
/// identities a `ProvenanceSpan::origin` is allowed to reference.
fn collect_node_ids(root: &LayoutBox) -> HashSet<NodeId> {
    fn walk(b: &LayoutBox, out: &mut HashSet<NodeId>) {
        out.insert(b.node);
        for child in &b.children {
            walk(child, out);
        }
    }
    let mut out = HashSet::new();
    walk(root, &mut out);
    out
}

/// ADR-025 §4 property 2 — every span's origin resolves. `BoxOrigin::node`,
/// when `Some`, is always either a box's own `LayoutBox::node` or the
/// containing element's node id used by `box_tree.rs`'s anonymous-box and
/// pseudo-element constructors — in both cases a value that appears as
/// *some* box's own `node` field in the same tree. A span whose origin
/// references a `NodeId` absent from that set points at a dangling
/// identity — e.g. stale after an incremental graft, or copied from the
/// wrong node.
fn check_origins_resolve(index: &ProvenanceIndex, known_nodes: &HashSet<NodeId>) {
    for span in index.spans() {
        if let Some(n) = span.origin.node {
            assert!(
                known_nodes.contains(&n),
                "ProvenanceSpan origin references node {n:?}, absent from the layout \
                 tree that produced this display list"
            );
        }
    }
}

/// ADR-025 §4 property 4 — every box with a visible background or border
/// has at least one span. Scoped to the `Block`/`FlowRoot`/`TableRow`/
/// `Table`/`TableRowGroup` self-paint path (`emit_box_self`'s first match
/// arm) — the one whose suppression conditions (`opacity`, `visibility`,
/// `empty-cells: hide`, a zero-size overflow clip) are cheap to mirror
/// here. Other `BoxKind`s (inline runs, form controls, markers, SVG) paint
/// through their own emitters with their own visibility rules and are out
/// of scope for this pass — same narrowing spirit as DEVX-8a's geometry
/// invariant, which found real exceptions rather than weaken the check.
///
/// `BoxOrigin` identifies boxes by `(node, role)`, not by instance — two
/// sibling anonymous wrappers around the same element share an origin
/// (ADR-025 §1). This check can only prove "some box with this origin
/// painted something", not "this exact box instance did" — still enough
/// to catch the failure mode DEVX-7's gate exists for: an origin that
/// painted nothing at all.
fn check_visible_boxes_have_spans(root: &LayoutBox, index: &ProvenanceIndex) {
    if box_has_visible_self_paint(root) {
        assert!(
            index.spans_for(root.origin).next().is_some(),
            "box with visible background/border produced no provenance span: node={:?} role={:?}",
            root.origin.node,
            root.origin.role
        );
    }
    for child in &root.children {
        check_visible_boxes_have_spans(child, index);
    }
}

/// Mirrors the visibility suppressions `emit_box_self` applies to the
/// `Block`-family branch before drawing a background or border, without
/// duplicating the drawing itself.
fn box_has_visible_self_paint(b: &LayoutBox) -> bool {
    if !matches!(
        b.kind,
        BoxKind::Block
            | BoxKind::FlowRoot
            | BoxKind::TableRow
            | BoxKind::Table
            | BoxKind::TableRowGroup
    ) {
        return false;
    }
    if !is_opacity_subtree_painted(b) || !is_paint_visible(b) || is_hidden_empty_cell(b) {
        return false;
    }
    let s = &b.style;
    let has_border = s.border_top_style.is_visible()
        || s.border_right_style.is_visible()
        || s.border_bottom_style.is_visible()
        || s.border_left_style.is_visible();
    if has_border {
        return true;
    }
    let has_bg_color =
        s.background_color.and_then(|c| c.to_color_opt()).is_some_and(|c| c.a > 0);
    if !has_bg_color {
        return false;
    }
    let clip = background_clip_rect(b, background_color_clip(b));
    clip.width > 0.0 && clip.height > 0.0
}

/// Non-panicking DEVX-8b violation tally, consumed by DEVX-11's `explain_page`
/// (`crates/driver/src/explain.rs`). Same relationship to [`check`] as
/// `lumen_layout::invariants::GeometryViolationCounts` has to `check_geometry`:
/// a separate, defensive walk (never indexes with an out-of-bounds/inverted
/// span the way `check_coverage`'s `assert!`-guarded version can rely on
/// panicking first) that counts every occurrence instead of aborting on the
/// first one, so a batch introspection query over many pages reports a bug
/// instead of crashing on it. Deliberately not a refactor of the four
/// panicking checks below — those are DEVX-8b's already-gated code path,
/// validated against the full `graphic_tests`/`samples` corpus, and stay
/// untouched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintViolationCounts {
    /// Display-list commands not covered by exactly one provenance span
    /// (a gap, an overlap, or a malformed span range).
    pub coverage: usize,
    /// Clip/scroll-layer push/pop imbalance: pops with nothing open, plus
    /// pushes still open at the end of the list.
    pub clip_balance: usize,
    /// Provenance spans whose origin node doesn't resolve to any box in the
    /// layout tree that produced this display list.
    pub origin_resolution: usize,
    /// Boxes with a visible background/border but no provenance span of
    /// their own (same `Block`/`FlowRoot`/`TableRow`/`Table`/`TableRowGroup`
    /// scoping as [`check_visible_boxes_have_spans`]).
    pub visible_missing_span: usize,
}

/// Runs the DEVX-11 counting analogs of every DEVX-8b check over one
/// already-built display list and its provenance index — see
/// [`PaintViolationCounts`].
pub fn count_paint_violations(
    out: &[DisplayCommand],
    index: &ProvenanceIndex,
    root: &LayoutBox,
) -> PaintViolationCounts {
    let known_nodes = collect_node_ids(root);
    PaintViolationCounts {
        coverage: count_coverage_violations(out, index),
        clip_balance: count_clip_stack_violations(out),
        origin_resolution: count_origin_violations(index, &known_nodes),
        visible_missing_span: count_visible_missing_span(root, index),
    }
}

/// Counting analog of [`check_coverage`]. Unlike `check_coverage`, must not
/// panic on a malformed span (inverted or out-of-bounds range) — it tallies
/// that as a violation and moves on instead of relying on an `assert!` to
/// stop it from indexing out of bounds.
fn count_coverage_violations(out: &[DisplayCommand], index: &ProvenanceIndex) -> usize {
    let mut covers = vec![0u16; out.len()];
    let mut malformed = 0usize;
    for span in index.spans() {
        if span.range.start >= span.range.end || span.range.end > out.len() {
            malformed += 1;
            continue;
        }
        for c in &mut covers[span.range.clone()] {
            *c += 1;
        }
    }
    malformed + covers.iter().filter(|&&c| c != 1).count()
}

/// Counting analog of [`check_clip_stack_balance`]. Same push/pop state
/// machine, but a bad pop resets the running depth to `0` and tallies a
/// violation instead of asserting — so one bad pop doesn't cascade into a
/// wall of spurious "still open" violations at the end of the list.
fn count_clip_stack_violations(out: &[DisplayCommand]) -> usize {
    let mut clip_depth: i32 = 0;
    let mut scroll_depth: i32 = 0;
    let mut violations = 0usize;
    for cmd in out {
        match cmd {
            DisplayCommand::PushClipRect { .. }
            | DisplayCommand::PushClipRoundedRect { .. }
            | DisplayCommand::PushClipPath { .. } => clip_depth += 1,
            DisplayCommand::PopClip => {
                clip_depth -= 1;
                if clip_depth < 0 {
                    violations += 1;
                    clip_depth = 0;
                }
            }
            DisplayCommand::PushScrollLayer { .. } => scroll_depth += 1,
            DisplayCommand::PopScrollLayer => {
                scroll_depth -= 1;
                if scroll_depth < 0 {
                    violations += 1;
                    scroll_depth = 0;
                }
            }
            _ => {}
        }
    }
    violations + clip_depth as usize + scroll_depth as usize
}

/// Counting analog of [`check_origins_resolve`].
fn count_origin_violations(index: &ProvenanceIndex, known_nodes: &HashSet<NodeId>) -> usize {
    index
        .spans()
        .iter()
        .filter(|span| span.origin.node.is_some_and(|n| !known_nodes.contains(&n)))
        .count()
}

/// Counting analog of [`check_visible_boxes_have_spans`]. Reuses
/// [`box_has_visible_self_paint`] directly — that predicate is already a
/// standalone pure function shared with the panicking check, so there is no
/// condition logic to keep in sync here, only the walk shape.
fn count_visible_missing_span(root: &LayoutBox, index: &ProvenanceIndex) -> usize {
    let mut n = if box_has_visible_self_paint(root) && index.spans_for(root.origin).next().is_none() {
        1
    } else {
        0
    };
    for child in &root.children {
        n += count_visible_missing_span(child, index);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;
    use lumen_layout::BoxRole;

    use crate::display_list::ProvenanceSpan;

    fn origin(n: u32) -> lumen_layout::BoxOrigin {
        lumen_layout::BoxOrigin { node: Some(NodeId::from_index(n as usize)), role: BoxRole::Element }
    }

    fn span(range: std::ops::Range<usize>, n: u32) -> ProvenanceSpan {
        ProvenanceSpan { range, origin: origin(n), fragment: 0, clip_depth: 0 }
    }

    #[test]
    fn coverage_accepts_a_full_exact_partition() {
        let out = vec![DisplayCommand::PopClip; 4];
        let index = ProvenanceIndex { spans: vec![span(0..2, 1), span(2..4, 2)] };
        check_coverage(&out, &index); // must not panic
    }

    #[test]
    #[should_panic(expected = "expected exactly 1")]
    fn coverage_rejects_a_gap() {
        let out = vec![DisplayCommand::PopClip; 4];
        // Command #3 is claimed by no span.
        let index = ProvenanceIndex { spans: vec![span(0..2, 1), span(2..3, 2)] };
        check_coverage(&out, &index);
    }

    #[test]
    #[should_panic(expected = "expected exactly 1")]
    fn coverage_rejects_an_overlap() {
        let out = vec![DisplayCommand::PopClip; 4];
        // Commands #1..2 are claimed by both spans.
        let index = ProvenanceIndex { spans: vec![span(0..3, 1), span(1..4, 2)] };
        check_coverage(&out, &index);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn coverage_rejects_a_span_past_the_end() {
        let out = vec![DisplayCommand::PopClip; 2];
        let index = ProvenanceIndex { spans: vec![span(0..3, 1)] };
        check_coverage(&out, &index);
    }

    #[test]
    fn clip_stack_balance_accepts_properly_nested_pushes() {
        let out = vec![
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0) },
            DisplayCommand::PushScrollLayer {
                clip_rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                scroll_x: 0.0,
                scroll_y: 0.0,
            },
            DisplayCommand::PopScrollLayer,
            DisplayCommand::PopClip,
        ];
        check_clip_stack_balance(&out); // must not panic
    }

    #[test]
    #[should_panic(expected = "still open")]
    fn clip_stack_balance_rejects_an_unclosed_push() {
        let out = vec![DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0) }];
        check_clip_stack_balance(&out);
    }

    #[test]
    #[should_panic(expected = "no matching Push")]
    fn clip_stack_balance_rejects_a_pop_with_nothing_open() {
        let out = vec![DisplayCommand::PopClip];
        check_clip_stack_balance(&out);
    }

    #[test]
    fn origins_resolve_accepts_a_known_node() {
        let known: HashSet<NodeId> = [NodeId::from_index(1)].into_iter().collect();
        let index = ProvenanceIndex { spans: vec![span(0..1, 1)] };
        check_origins_resolve(&index, &known); // must not panic
    }

    #[test]
    #[should_panic(expected = "absent from the layout tree")]
    fn origins_resolve_rejects_a_dangling_node() {
        let known: HashSet<NodeId> = [NodeId::from_index(1)].into_iter().collect();
        // Origin references node 2, which no box in the tree owns.
        let index = ProvenanceIndex { spans: vec![span(0..1, 2)] };
        check_origins_resolve(&index, &known);
    }

    #[test]
    fn count_coverage_violations_accepts_a_full_exact_partition() {
        let out = vec![DisplayCommand::PopClip; 4];
        let index = ProvenanceIndex { spans: vec![span(0..2, 1), span(2..4, 2)] };
        assert_eq!(count_coverage_violations(&out, &index), 0);
    }

    #[test]
    fn count_coverage_violations_counts_a_gap_and_an_overlap() {
        let out = vec![DisplayCommand::PopClip; 4];
        // Command #3 is claimed by no span (gap); commands #0..2 are double-claimed (overlap).
        let index = ProvenanceIndex { spans: vec![span(0..3, 1), span(0..2, 2)] };
        assert_eq!(count_coverage_violations(&out, &index), 3);
    }

    #[test]
    fn count_coverage_violations_counts_a_malformed_span_without_panicking() {
        let out = vec![DisplayCommand::PopClip; 2];
        let index = ProvenanceIndex { spans: vec![span(0..3, 1)] };
        // 1 malformed span + 2 commands left with zero valid coverage (the
        // malformed span is skipped rather than applied) — the malformed
        // span doesn't get to also hide the coverage gap it caused.
        assert_eq!(count_coverage_violations(&out, &index), 3);
    }

    #[test]
    fn count_clip_stack_violations_accepts_a_balanced_stack() {
        let out = vec![
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0) },
            DisplayCommand::PopClip,
        ];
        assert_eq!(count_clip_stack_violations(&out), 0);
    }

    #[test]
    fn count_clip_stack_violations_counts_an_unclosed_push_and_a_bad_pop() {
        let out = vec![
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0) },
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 10.0, 10.0) },
            DisplayCommand::PopClip,
            DisplayCommand::PopClip,
            DisplayCommand::PopClip, // no matching push
        ];
        assert_eq!(count_clip_stack_violations(&out), 1);
    }

    #[test]
    fn count_origin_violations_counts_only_dangling_nodes() {
        let known: HashSet<NodeId> = [NodeId::from_index(1)].into_iter().collect();
        let index = ProvenanceIndex { spans: vec![span(0..1, 1), span(1..2, 2)] };
        assert_eq!(count_origin_violations(&index, &known), 1);
    }
}
