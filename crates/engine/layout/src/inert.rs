//! `inert` attribute layout algorithm — HTML Living Standard §6.7.
//!
//! The `inert` boolean attribute makes a subtree non-interactive:
//! - Elements in an inert subtree do not receive pointer events (click, mouseover, …).
//! - Elements in an inert subtree cannot receive focus.
//! - Elements in an inert subtree are excluded from the accessibility tree.
//! - `find-in-page` skips inert text.
//!
//! Inertness is *inherited down the DOM tree*: if an ancestor carries `inert`,
//! every descendant is also inert regardless of its own attributes.
//!
//! ## P1/P4 split
//!
//! **P1 (this module)** — DOM traversal and hit-test filtering:
//! - [`is_inert`] — checks a single node and all its ancestors for `inert`.
//! - [`collect_inert_regions`] — walks the layout tree and returns rects of
//!   inert subtrees so the shell can exclude them from pointer hit-testing and
//!   focus traversal without re-walking the DOM on every event.
//!
//! **P4 (CSS: inert)** — UA stylesheet entry:
//! - Add `[inert] { pointer-events: none; }` to the UA stylesheet so that
//!   `ComputedStyle.pointer_events` is already `none` for inert nodes.
//!   Use the comment `// CSS: inert` in `style.rs` as the wiring point.
//!
//! **P3 (shell wiring)** — event routing:
//! - Call `collect_inert_regions(root, doc)` after each layout pass and store
//!   the result. In `try_click` / focus traversal, check the hit point against
//!   `InertRegion::rect` before dispatching. See `// CSS: inert` comment in
//!   `collect_clickable_elements` (lumen-layout lib.rs).

use lumen_core::geom::Rect;
use lumen_dom::{Document, NodeId};

use crate::box_tree::LayoutBox;

// ─── is_inert ────────────────────────────────────────────────────────────────

/// Returns `true` if `node` or any of its ancestors carries the `inert`
/// boolean attribute (HTML LS §6.7).
///
/// Walks the parent chain upward from `node` to the root. A node is inert when
/// it or any ancestor has `get_attr("inert").is_some()`.
///
/// Returns `false` for non-element nodes (text, comment, document) — they have
/// no attributes, so inertness must come from an element ancestor.
pub fn is_inert(doc: &Document, mut node: NodeId) -> bool {
    loop {
        if doc.get(node).get_attr("inert").is_some() {
            return true;
        }
        match doc.get(node).parent {
            Some(p) => node = p,
            None => return false,
        }
    }
}

// ─── InertRegion ─────────────────────────────────────────────────────────────

/// A rectangular region in the layout tree that belongs to an inert subtree.
///
/// Collected by [`collect_inert_regions`] for use by the shell's hit-test and
/// focus-traversal logic. The shell skips pointer events that land inside any
/// `InertRegion::rect`.
#[derive(Debug, Clone, PartialEq)]
pub struct InertRegion {
    /// DOM node that carries the `inert` attribute (the root of the inert subtree).
    pub node_id: NodeId,
    /// Border-box rectangle in CSS px (document-relative, before scroll).
    /// Matches the bounding box of the inert element's layout box.
    pub rect: Rect,
}

// ─── collect_inert_regions ────────────────────────────────────────────────────

/// Walk the layout tree and return every inert root box as an [`InertRegion`].
///
/// Only the *root* of each inert subtree is returned — once a box is found to
/// be inert, its descendants are skipped because they are transitively inert.
/// This avoids O(depth) redundant lookups for deeply nested inert subtrees.
///
/// # CSS: inert
/// P4 should add `[inert] { pointer-events: none; }` to the UA stylesheet so
/// that `ComputedStyle.pointer_events` reflects inertness during cascade. This
/// function provides the complementary layout-level information (bounding boxes)
/// that the shell needs for hit-test filtering.
pub fn collect_inert_regions(root: &LayoutBox, doc: &Document) -> Vec<InertRegion> {
    let mut out = Vec::new();
    collect_inert_rec(root, doc, false, &mut out);
    out
}

fn collect_inert_rec(b: &LayoutBox, doc: &Document, already_inert: bool, out: &mut Vec<InertRegion>) {
    use crate::box_tree::BoxKind;
    if matches!(b.kind, BoxKind::Skip) {
        return;
    }

    if !already_inert && doc.get(b.node).get_attr("inert").is_some() {
        // This box is the root of a new inert subtree — record it and stop descending.
        out.push(InertRegion { node_id: b.node, rect: b.rect });
        return;
    }

    for child in &b.children {
        collect_inert_rec(child, doc, already_inert, out);
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_inert ──

    #[test]
    fn not_inert_when_no_attribute() {
        let doc = lumen_html_parser::parse("<div id='a'><p>text</p></div>");
        // Find <div> — it's at depth 3 from document root (doc > html > body > div)
        let root = doc.root();
        let div = find_element_by_tag(&doc, root, "div").unwrap();
        assert!(!is_inert(&doc, div));
    }

    #[test]
    fn inert_on_self() {
        let doc = lumen_html_parser::parse("<div inert><p>text</p></div>");
        let root = doc.root();
        let div = find_element_by_tag(&doc, root, "div").unwrap();
        assert!(is_inert(&doc, div));
    }

    #[test]
    fn inert_inherited_from_ancestor() {
        let doc = lumen_html_parser::parse("<div inert><p id='child'>text</p></div>");
        let root = doc.root();
        let p = find_element_by_tag(&doc, root, "p").unwrap();
        // <p> itself has no `inert`, but its parent <div> does
        assert!(is_inert(&doc, p));
    }

    #[test]
    fn inert_not_inherited_from_sibling() {
        let doc = lumen_html_parser::parse("<div inert></div><section></section>");
        let root = doc.root();
        let section = find_element_by_tag(&doc, root, "section").unwrap();
        assert!(!is_inert(&doc, section));
    }

    #[test]
    fn inert_empty_value_treated_as_present() {
        // HTML boolean attributes: `inert=""` and `inert` are equivalent
        let doc = lumen_html_parser::parse(r#"<button inert="">click</button>"#);
        let root = doc.root();
        let btn = find_element_by_tag(&doc, root, "button").unwrap();
        assert!(is_inert(&doc, btn));
    }

    #[test]
    fn is_inert_root_node_no_panic() {
        let doc = lumen_html_parser::parse("<p>text</p>");
        // Document root itself: no parent, no inert — must return false cleanly
        assert!(!is_inert(&doc, doc.root()));
    }

    // ── collect_inert_regions ──

    fn lay(html: &str) -> (lumen_dom::Document, LayoutBox) {
        use lumen_core::geom::Size;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = crate::box_tree::layout(&doc, &sheet, Size::new(800.0, 600.0));
        (doc, root)
    }

    #[test]
    fn no_inert_nodes_yields_empty_result() {
        let (doc, tree) = lay("<div><p>text</p></div>");
        let regions = collect_inert_regions(&tree, &doc);
        assert!(regions.is_empty());
    }

    #[test]
    fn inert_root_collected_once() {
        let (doc, tree) = lay("<div inert><p>text</p></div>");
        let root_node = doc.root();
        let div = find_element_by_tag(&doc, root_node, "div").unwrap();

        let regions = collect_inert_regions(&tree, &doc);
        // Only the <div inert> should appear — <p> is a descendant and skipped
        assert_eq!(regions.len(), 1, "expected exactly one inert region (the <div inert>)");
        assert_eq!(regions[0].node_id, div);
    }

    #[test]
    fn two_sibling_inert_subtrees_both_collected() {
        let (doc, tree) = lay("<div inert></div><section inert></section>");
        let root_node = doc.root();
        let div = find_element_by_tag(&doc, root_node, "div").unwrap();
        let section = find_element_by_tag(&doc, root_node, "section").unwrap();

        let regions = collect_inert_regions(&tree, &doc);
        assert_eq!(regions.len(), 2, "expected two inert regions");
        let node_ids: Vec<_> = regions.iter().map(|r| r.node_id).collect();
        assert!(node_ids.contains(&div));
        assert!(node_ids.contains(&section));
    }

    #[test]
    fn nested_inert_not_double_counted() {
        // Inner <p inert> inside outer <div inert> — only the outer is returned.
        let (doc, tree) = lay("<div inert><p inert>text</p></div>");
        let root_node = doc.root();
        let div = find_element_by_tag(&doc, root_node, "div").unwrap();

        let regions = collect_inert_regions(&tree, &doc);
        // Only the outer <div inert> — inner <p inert> is a descendant of an inert root
        assert_eq!(regions.len(), 1, "nested inert must not be double-counted");
        assert_eq!(regions[0].node_id, div);
    }

    // ── helpers ──

    fn find_element_by_tag(doc: &Document, start: NodeId, tag: &str) -> Option<NodeId> {
        use lumen_dom::NodeData;
        if let NodeData::Element { name, .. } = &doc.get(start).data
            && name.local == tag
        {
            return Some(start);
        }
        for &child in &doc.get(start).children {
            if let Some(found) = find_element_by_tag(doc, child, tag) {
                return Some(found);
            }
        }
        None
    }
}
