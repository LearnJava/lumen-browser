//! LAYOUT-1 срез 3: regression guards for the tree-walkers converted from
//! recursion to an explicit stack — `serialize_layout_tree`/`write_box`
//! (`snapshot.rs`) and `collect_computed_styles`/`collect_layout_rects`/
//! `collect_client_rects` (`lib.rs`). Same shape as `box_tree::tests::
//! layout_box_drop`: a single-child chain deep enough that the old recursive
//! walk would have overflowed a normal thread stack (BUG-987) — built and
//! walked without going through `layout()` itself, since `lay_out_inner`'s
//! own block-flow recursion is a separate, still-open source of the same
//! overflow and would crash before these functions ever ran.
//!
//! LAYOUT-2 срез 2 extended this file with the same guard for six more
//! `lib.rs` walkers found by the same "no post-loop use of a child's result"
//! audit (`collect_sticky_boxes`/`collect_snap_containers`/
//! `collect_pseudo_computed_styles`/`collect_custom_properties`/
//! `collect_view_transition_names`/`collect_view_transition_groups`), plus
//! `collect_clickable_elements` on a real DOM-backed chain.
use super::*;
use lumen_core::geom::Rect;

/// `collect_layout_rects`/`collect_client_rects` do O(1) work per node, so
/// 200_000 — the same depth `box_tree::tests::layout_box_drop` uses — stays
/// fast. `write_box`'s `"  ".repeat(depth)` indent and
/// `collect_computed_styles`'s per-node 302-field `ComputedStyle` → map
/// conversion are each O(depth) work *per node*, i.e. O(depth²) total; at
/// 200_000 that is tens of GB and not what this test is about (it already
/// held at the old recursion depths the stack overflowed at long before
/// this cost mattered) — `SHALLOWER_DEPTH` keeps those two within a stack
/// depth the pre-fix recursive code could never have reached (ROADMAP:
/// the unbumped stack overflowed around 150-800 levels) while staying cheap.
const DEPTH: usize = 200_000;
const SHALLOWER_DEPTH: usize = 20_000;

/// Plain `Block` wrapping the given children — same shape as
/// `box_tree::tests::layout_box_drop::block`, duplicated here since that
/// helper is private to its own module.
fn block(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
    block_with_id(lumen_dom::NodeId::from_index(node_index), children)
}

/// Same shape as [`block`], keyed by a caller-supplied `NodeId` instead of a
/// synthetic index — needed for walkers (`collect_clickable_elements`) that
/// read the DOM node behind `LayoutBox::node`, which must actually exist in
/// the `Document` passed alongside.
fn block_with_id(node: lumen_dom::NodeId, children: Vec<LayoutBox>) -> LayoutBox {
    let style = ComputedStyle::root();
    LayoutBox {
        node,
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        used_line_height: style.font_size * style.line_height,
        style: std::sync::Arc::new(style),
        kind: BoxKind::Block,
        children,
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin::default(),
    }
}

/// Single-child chain, `depth` levels deep, root first — node index counts
/// down from `depth` at the root to `0` at the leaf so every box in the
/// chain carries a distinct `NodeId`.
fn deep_chain(depth: usize) -> LayoutBox {
    let mut node = block(0, vec![]);
    for i in 1..=depth {
        node = block(i, vec![node]);
    }
    node
}

#[test]
fn serialize_layout_tree_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(SHALLOWER_DEPTH);
    let text = serialize_layout_tree(&root);
    // One "Block rect=..." line per level; no per-child extra lines for a
    // plain Block with no InlineRun content.
    assert_eq!(text.lines().count(), SHALLOWER_DEPTH + 1);
    // Root is unindented; the leaf sits at `SHALLOWER_DEPTH` levels of
    // two-space indent.
    assert!(text.lines().next().unwrap().starts_with("Block "));
    let leaf_line = text.lines().last().unwrap();
    assert!(leaf_line.starts_with(&"  ".repeat(SHALLOWER_DEPTH)));
}

#[test]
fn collect_computed_styles_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(SHALLOWER_DEPTH);
    let doc = lumen_dom::Document::new();
    let styles = collect_computed_styles(&root, &doc, None);
    assert_eq!(styles.len(), SHALLOWER_DEPTH + 1);
    assert!(styles.contains_key(&0));
    assert!(styles.contains_key(&(SHALLOWER_DEPTH as u32)));
}

#[test]
fn collect_layout_rects_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    let doc = lumen_dom::Document::new();
    let rects = collect_layout_rects(&root, &doc);
    assert_eq!(rects.len(), DEPTH + 1);
    assert_eq!(rects[&0], [0.0, 0.0, 10.0, 10.0]);
}

#[test]
fn collect_client_rects_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    let doc = lumen_dom::Document::new();
    let rects = collect_client_rects(&root, &doc);
    assert_eq!(rects.len(), DEPTH + 1);
    assert_eq!(rects[&0], vec![[0.0, 0.0, 10.0, 10.0]]);
}

/// LAYOUT-2 срез 2: same regression shape as the four tests above, for the
/// other tree-walkers found while auditing `lib.rs` for the same "pre-order,
/// nothing after the loop over `children` reads a produced value" class
/// (`collect_sticky_boxes`, `collect_snap_containers`,
/// `collect_pseudo_computed_styles`, `collect_custom_properties`,
/// `collect_view_transition_names`, `collect_view_transition_groups`) — all
/// O(1) work per node, so `DEPTH` is affordable. `collect_clickable_elements`
/// is deliberately excluded from this group: `inert::is_inert` walks the DOM
/// parent chain to the root on *every* node, making it O(depth) per node like
/// `collect_computed_styles` — it gets its own test below at
/// `SHALLOWER_DEPTH`, built over a real DOM chain instead of `deep_chain`'s
/// synthetic (DOM-less) `NodeId`s, since `is_inert`/`Document::get` panics on
/// an id outside the document's arena.
#[test]
fn collect_sticky_boxes_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    let viewport = lumen_core::geom::Size::new(800.0, 600.0);
    assert!(collect_sticky_boxes(&root, viewport).is_empty());
}

#[test]
fn collect_snap_containers_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    assert!(collect_snap_containers(&root).is_empty());
}

#[test]
fn collect_pseudo_computed_styles_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    assert!(collect_pseudo_computed_styles(&root).is_empty());
}

#[test]
fn collect_custom_properties_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    let viewport = lumen_core::geom::Size::new(800.0, 600.0);
    assert!(collect_custom_properties(&root, viewport).is_empty());
}

#[test]
fn collect_view_transition_names_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    assert!(collect_view_transition_names(&root).is_empty());
}

#[test]
fn collect_view_transition_groups_deep_chain_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    assert!(collect_view_transition_groups(&root).is_empty());
}

#[test]
fn collect_clickable_elements_deep_chain_does_not_overflow_the_stack() {
    let mut doc = lumen_dom::Document::new();
    let mut dom_parent = doc.root();
    let mut node_ids = Vec::with_capacity(SHALLOWER_DEPTH);
    for _ in 0..SHALLOWER_DEPTH {
        let div = doc.create_element(lumen_dom::QualName::html("div"));
        doc.append_child(dom_parent, div);
        dom_parent = div;
        node_ids.push(div);
    }
    // Layout-box chain over the same ids, root first — mirrors `deep_chain`
    // but keyed by real DOM node ids so `inert::is_inert` never walks off
    // the document's arena.
    let mut node = block_with_id(node_ids[SHALLOWER_DEPTH - 1], vec![]);
    for &id in node_ids[..SHALLOWER_DEPTH - 1].iter().rev() {
        node = block_with_id(id, vec![node]);
    }
    assert!(collect_clickable_elements(&node, &doc).is_empty());
}
