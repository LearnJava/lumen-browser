//! `explain_element(selector)` (DEVX-10) — a single-query causal chain:
//! DOM → style → layout → size → stacking context → paint → clip → layer.
//!
//! Session-agnostic: operates on an already-built `&LayoutBox` tree and
//! `&Document` directly, so any session backed by an in-process layout tree
//! (`InProcessSession`, `WinitSession`) can call it without duplicating the
//! walk. Reads [`ProvenanceIndex`](lumen_paint::ProvenanceIndex) (DEVX-7) for
//! the paint/clip/layer links — see `docs/decisions/ADR-025-identity-propagation.md`.

use lumen_core::geom::Rect;
use lumen_dom::{Document, NodeId};
use lumen_layout::{BoxKind, BoxRole, LayoutBox, Overflow, PaintOrder, StackingTree};

use crate::ExplainElement;

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
