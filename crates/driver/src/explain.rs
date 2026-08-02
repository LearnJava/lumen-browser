//! `explain_element(selector)` (DEVX-10) — a single-query causal chain:
//! DOM → style → layout → size → stacking context → paint → clip → layer.
//!
//! Session-agnostic: operates on an already-built `&LayoutBox` tree and
//! `&Document` directly, so any session backed by an in-process layout tree
//! (`InProcessSession`, `WinitSession`) can call it without duplicating the
//! walk. Reads [`ProvenanceIndex`](lumen_paint::ProvenanceIndex) (DEVX-7) for
//! the paint/clip/layer links — see `docs/decisions/ADR-025-identity-propagation.md`.

use std::time::Instant;

use lumen_core::geom::Rect;
use lumen_dom::{Document, NodeId};
use lumen_layout::{BoxKind, BoxRole, LayoutBox, Overflow, PaintOrder, StackingTree};

use crate::{ExplainElement, ExplainPage, InvariantViolationCounts};

/// Runs the DEVX-10 causal chain for the first DOM node matching `sel`.
///
/// `root` is the current page's layout tree; `doc` is the DOM it was built
/// from. Returns an all-`false`/`None` [`ExplainElement`] (only `in_dom` may
/// be `true`) when the selector matches nothing or nothing paints.
pub fn explain_element(root: &LayoutBox, doc: &Document, sel: &str) -> ExplainElement {
    let mut out = ExplainElement::default();

    let Some(node_id) = lumen_layout::find_first_dom_node_by_selector(doc, sel) else {
        return out;
    };
    out.in_dom = true;

    let mut path: Vec<&LayoutBox> = Vec::new();
    let Some(lb) = find_principal_box(root, node_id, &mut path) else {
        // The node exists in the DOM but the box tree never materialized a
        // box for it at all — e.g. a descendant of a `display:none`/`Skip`
        // ancestor, whose subtree the box tree never recurses into. Narrower
        // than "styles didn't apply": the cascade would still resolve a
        // style for it in principle, this engine just doesn't materialize
        // one here. Documented gap, not a silent wrong answer.
        return out;
    };
    out.style_applied = true;
    out.in_layout = !matches!(lb.kind, BoxKind::Skip);
    if out.in_layout {
        out.size = Some((lb.rect.width, lb.rect.height));
        out.creates_stacking_context = lumen_layout::creates_stacking_context(&lb.style);
    }

    let stacking_tree = StackingTree::build(root);
    let order = PaintOrder::from_tree(&stacking_tree);
    let (_commands, provenance) = lumen_paint::build_display_list_ordered(root, &stacking_tree, &order);
    let spans: Vec<_> = provenance.spans_for(lb.origin).collect();
    out.commands_emitted = spans.iter().map(|s| s.range.end - s.range.start).sum();
    out.clip_depth = spans.iter().map(|s| s.clip_depth).max();
    if out.in_layout {
        // BasicLayerTree::single_layer — the in-process compositor has
        // exactly one layer today, so this is the only possible answer.
        out.layer = Some(0);
    }

    if out.in_layout && out.commands_emitted == 0 {
        out.heuristic = heuristic_hint(&path, lb);
    }

    out
}

/// Runs the DEVX-11 page-level aggregate: invariant-firing counts by category
/// plus telemetry (box counts, overflow, commands, clip depth). Every counter
/// here is derived from the layout tree alone — no DOM lookup needed, unlike
/// [`explain_element`]. `relayout_count` is left at `None`: it needs
/// session-level state this session-agnostic function doesn't have, so
/// callers ([`crate::session::InProcessSession`] etc.) fill it in afterward.
pub fn explain_page(root: &LayoutBox) -> ExplainPage {
    let mut out = ExplainPage::default();

    let t0 = Instant::now();
    let mut geometry_boxes = 0usize;
    let mut anonymous_boxes = 0usize;
    let mut overflow_elements = 0usize;
    walk_telemetry(root, &mut geometry_boxes, &mut anonymous_boxes, &mut overflow_elements);
    out.box_count = geometry_boxes;
    out.anonymous_box_count = anonymous_boxes;
    out.overflow_element_count = overflow_elements;
    let geometry_violations = lumen_layout::count_geometry_violations(root);
    out.phase_ns.tree_walk_ns = t0.elapsed().as_nanos() as u64;

    let t1 = Instant::now();
    let stacking_tree = StackingTree::build(root);
    let order = PaintOrder::from_tree(&stacking_tree);
    let (commands, provenance) = lumen_paint::build_display_list_ordered(root, &stacking_tree, &order);
    let paint_violations = lumen_paint::count_paint_violations(&commands, &provenance, root);
    out.command_count = commands.len();
    out.max_clip_depth = provenance.spans().iter().map(|s| s.clip_depth).max().unwrap_or(0);
    out.phase_ns.display_list_build_ns = t1.elapsed().as_nanos() as u64;

    out.invariant_violations = InvariantViolationCounts {
        geometry_non_finite: geometry_violations.non_finite,
        geometry_containment: geometry_violations.containment,
        paint_coverage: paint_violations.coverage,
        paint_clip_balance: paint_violations.clip_balance,
        paint_origin_resolution: paint_violations.origin_resolution,
        paint_visible_missing_span: paint_violations.visible_missing_span,
    };

    out
}

/// Walks `b`'s subtree accumulating the three `explain_page` box counters in
/// one pass: total boxes, anonymous boxes (`AnonymousBlock`/`AnonymousInlineRun`
/// — `Pseudo` deliberately excluded, see [`ExplainPage::anonymous_box_count`]),
/// and elements whose `overflow-x`/`overflow-y` isn't `visible`.
fn walk_telemetry(b: &LayoutBox, boxes: &mut usize, anonymous: &mut usize, overflow: &mut usize) {
    *boxes += 1;
    if matches!(b.origin.role, BoxRole::AnonymousBlock | BoxRole::AnonymousInlineRun) {
        *anonymous += 1;
    }
    if b.style.overflow_x != Overflow::Visible || b.style.overflow_y != Overflow::Visible {
        *overflow += 1;
    }
    for child in &b.children {
        walk_telemetry(child, boxes, anonymous, overflow);
    }
}

/// Finds `target`'s own principal box (`BoxRole::Element`) by walking `b`
/// unconditionally — including `BoxKind::Skip` boxes, which
/// `lumen_layout::find_box_by_selector` deliberately excludes (ADR-025 §1:
/// `origin.role` still identifies the box as the element's own even when
/// `kind` says "excluded from layout"). Records the full root→target
/// ancestor path in `path` (consumed by [`heuristic_hint`]).
fn find_principal_box<'a>(
    b: &'a LayoutBox,
    target: NodeId,
    path: &mut Vec<&'a LayoutBox>,
) -> Option<&'a LayoutBox> {
    path.push(b);
    if b.node == target && b.origin.role == BoxRole::Element {
        return Some(b);
    }
    for child in &b.children {
        if let Some(found) = find_principal_box(child, target, path) {
            return Some(found);
        }
    }
    path.pop();
    None
}

/// Best-effort guess at why a box with no own paint commands is invisible:
/// zero geometry, or the nearest overflow-clipping ancestor's rect doesn't
/// intersect this box's rect. Approximate — ignores transforms, rounded
/// corners and `clip-path` — which is exactly why the caller surfaces it as
/// a labelled heuristic, not a fact (ADR-024).
fn heuristic_hint(path: &[&LayoutBox], target: &LayoutBox) -> Option<String> {
    if target.rect.width <= 0.0 || target.rect.height <= 0.0 {
        return Some(format!(
            "box has zero geometry ({:.1}x{:.1}) — nothing to paint regardless of clipping",
            target.rect.width, target.rect.height
        ));
    }
    // `path` ends with `target` itself — walk ancestors only, nearest first.
    for ancestor in path[..path.len().saturating_sub(1)].iter().rev() {
        let clips = !matches!(ancestor.style.overflow_x, Overflow::Visible)
            || !matches!(ancestor.style.overflow_y, Overflow::Visible);
        if clips && !rects_intersect(ancestor.rect, target.rect) {
            return Some(format!(
                "box rect ({:.1},{:.1} {:.1}x{:.1}) falls fully outside an overflow-clipping \
                 ancestor's rect ({:.1},{:.1} {:.1}x{:.1})",
                target.rect.x, target.rect.y, target.rect.width, target.rect.height,
                ancestor.rect.x, ancestor.rect.y, ancestor.rect.width, ancestor.rect.height,
            ));
        }
    }
    None
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.right() && a.right() > b.x && a.y < b.bottom() && a.bottom() > b.y
}
