//! Layout invalidation subtree ratchet (EE-3).
//!
//! Tracks which [`LayoutBox`] subtrees need re-layout after DOM/style changes.
//! Call [`mark_dirty`] after a mutation, then use [`crate::box_tree::lay_out_incremental`]
//! to re-layout only the affected subtrees. Clean subtrees are translated to
//! their new positions without re-running layout (~10× speedup on class toggle).
//!
//! Typical flow:
//! ```ignore
//! // Step 1 — initial full layout:
//! let mut root = layout_measured_hyp(&doc, &sheet, vp, measurer, hp, false);
//! clear_dirty(&mut root);          // mark entire tree clean for incremental use
//!
//! // Step 2 — after a CSS class toggle on `changed_id`:
//! mark_dirty(&mut root, changed_id);
//!
//! // Step 3 — incremental re-layout (skips clean subtrees):
//! let pcb = Rect::new(0.0, 0.0, vp.width, vp.height);
//! lay_out_incremental(&mut root, 0.0, 0.0, vp.width, Some(vp.height),
//!                     Some(measurer), vp, pcb, &hp);
//! // dirty bits are cleared automatically by lay_out_incremental.
//! ```

use lumen_dom::NodeId;
use crate::box_tree::LayoutBox;

// ─── DirtyBits ───────────────────────────────────────────────────────────────

/// Bitflag tracking which aspects of a [`LayoutBox`] need recalculation.
///
/// Only checked when `lay_out_incremental` is active (the incremental layout
/// mode flag is set). Normal `lay_out` calls ignore dirty bits entirely.
///
/// Invariant: a node with [`DirtyBits::HAS_DIRTY_DESCENDANT`] but not
/// [`DirtyBits::SELF_SIZE`] always has at least one child with `SELF_SIZE`
/// (directly or transitively).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtyBits(pub(crate) u8);

impl DirtyBits {
    /// Box is clean — no recalculation needed.
    pub const CLEAN: Self = DirtyBits(0);
    /// This box's own style or size-affecting attributes changed.
    pub const SELF_SIZE: Self = DirtyBits(0b001);
    /// At least one descendant has `SELF_SIZE`; must recurse to reach it.
    pub const HAS_DIRTY_DESCENDANT: Self = DirtyBits(0b010);
    /// Entire subtree is dirty (e.g. viewport resize, font change).
    pub const SUBTREE: Self = DirtyBits(0b100);

    /// Returns `true` when no bits are set (layout is up-to-date).
    #[inline]
    pub fn is_clean(self) -> bool { self.0 == 0 }

    /// Returns `true` when any bit is set.
    #[inline]
    pub fn is_dirty(self) -> bool { self.0 != 0 }

    /// Returns `true` when all bits in `rhs` are also set in `self`.
    #[inline]
    pub fn contains(self, rhs: Self) -> bool { (self.0 & rhs.0) == rhs.0 }
}

impl std::ops::BitOr for DirtyBits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self { DirtyBits(self.0 | rhs.0) }
}

impl std::ops::BitOrAssign for DirtyBits {
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}

// ─── Core operations ─────────────────────────────────────────────────────────

/// Translate every rect in `b`'s subtree by `(dx, dy)` without re-running layout.
///
/// Used to reposition a clean subtree when a dirty sibling above it changed
/// height, keeping the block-flow y-cursor consistent across siblings.
/// Zero deltas are a no-op (early exit at the root level).
pub fn translate_subtree(b: &mut LayoutBox, dx: f32, dy: f32) {
    if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
        return;
    }
    b.rect.x += dx;
    b.rect.y += dy;
    for child in &mut b.children {
        translate_subtree(child, dx, dy);
    }
}

/// Mark `node_id` as needing full re-layout.
///
/// Walks `root` depth-first to locate the node, sets [`DirtyBits::SELF_SIZE`]
/// on it, and sets [`DirtyBits::HAS_DIRTY_DESCENDANT`] on every ancestor from
/// the target back up to `root`. Returns `true` if the node was found.
pub fn mark_dirty(root: &mut LayoutBox, node_id: NodeId) -> bool {
    mark_dirty_inner(root, node_id)
}

fn mark_dirty_inner(b: &mut LayoutBox, target: NodeId) -> bool {
    if b.node == target {
        b.dirty |= DirtyBits::SELF_SIZE;
        return true;
    }
    for child in &mut b.children {
        if mark_dirty_inner(child, target) {
            b.dirty |= DirtyBits::HAS_DIRTY_DESCENDANT;
            return true;
        }
    }
    false
}

/// Mark all nodes in `node_ids` as dirty (one tree walk per node).
///
/// Convenience wrapper over [`mark_dirty`] for batch mutations where multiple
/// nodes change style simultaneously (e.g. a CSS class affecting many elements).
pub fn mark_dirty_set(root: &mut LayoutBox, node_ids: &[NodeId]) {
    for &id in node_ids {
        mark_dirty(root, id);
    }
}

/// Recursively clear all dirty bits throughout `b`'s entire subtree.
///
/// Call after the initial `layout_measured_hyp` pass to transition the tree
/// into incremental mode, and after each `lay_out_incremental` call (though
/// `lay_out_incremental` clears bits automatically).
pub fn clear_dirty(b: &mut LayoutBox) {
    b.dirty = DirtyBits::CLEAN;
    for child in &mut b.children {
        clear_dirty(child);
    }
}

// ─── Incremental box-build index (BUG-341 S4) ───────────────────────────────

/// Index a previous [`LayoutBox`] tree by `NodeId` for `build_box_or_reuse`'s
/// whole-subtree lookup.
///
/// A single `NodeId` can appear on more than one box in the tree: anonymous
/// boxes (`InlineRun`, `InlineBlockRow`, `Marker`, `InlineSpace`, pseudo-
/// element boxes) are tagged with their *owning* element's id, not a unique id
/// of their own, and are always descendants of that owning element's own
/// top-level box. Pre-order `insert`-keep-first therefore always keeps the
/// outermost (real) box for a given id — the correct "whole subtree for this
/// node" — even though later, deeper traversal steps revisit the same id on
/// that element's own synthetic children.
pub(crate) fn index_by_node(root: &LayoutBox) -> std::collections::HashMap<NodeId, &LayoutBox> {
    let mut map = std::collections::HashMap::new();
    index_by_node_inner(root, &mut map);
    map
}

fn index_by_node_inner<'a>(
    b: &'a LayoutBox,
    map: &mut std::collections::HashMap<NodeId, &'a LayoutBox>,
) {
    map.entry(b.node).or_insert(b);
    for child in &b.children {
        index_by_node_inner(child, map);
    }
}

// ─── Streaming graft (PH1-2b) ──────────────────────────────────────────────────

/// Mark every box in `b`'s subtree as [`DirtyBits::SELF_SIZE`].
///
/// Used by streaming incremental layout: a freshly-built box tree has valid
/// styles but no geometry, so in incremental mode every node must be re-laid-out
/// *unless* its geometry can be reused from the previous tick. The grafting pass
/// ([`graft_geometry`]) then clears the bits on subtrees it can reuse. Without
/// this, a fresh box defaults to [`DirtyBits::CLEAN`] and `lay_out` would skip it
/// (translating its zero-sized rect) instead of laying it out.
pub fn mark_subtree_dirty(b: &mut LayoutBox) {
    b.dirty = DirtyBits::SELF_SIZE;
    for child in &mut b.children {
        mark_subtree_dirty(child);
    }
}

/// Reuse laid-out geometry from `prev` for unchanged subtrees of the fresh tree
/// `new`, marking them [`DirtyBits::CLEAN`] (PH1-2b streaming incremental layout).
///
/// `new` is a freshly-built box tree (all nodes [`DirtyBits::SELF_SIZE`] after
/// [`mark_subtree_dirty`]) produced from a DOM that is a superset of the one that
/// produced `prev` (the previous tick's laid-out tree). For every subtree whose
/// node id, box kind payload and computed style are identical and whose structure
/// matches recursively, the entire `prev` subtree (including its laid-out
/// fragments) is cloned into `new` and marked clean. Such subtrees are then
/// repositioned in O(1) by `lay_out`'s incremental fast path instead of being
/// re-laid-out. New or changed subtrees keep their `SELF_SIZE` bit and are laid
/// out fresh.
///
/// Matching is by index: streaming appends nodes at the end, so the unchanged
/// prefix of each child list matches and the changed/new tail is re-laid-out.
/// Returns `true` when `new`'s whole subtree was reused clean from `prev`.
pub fn graft_geometry(new: &mut LayoutBox, prev: &LayoutBox) -> bool {
    if new.node != prev.node
        || !kind_layout_eq(&new.kind, &prev.kind)
        || new.style != prev.style
    {
        // Node identity, box kind payload or style differ → cannot reuse this
        // box. Leave the whole subtree dirty (marked by `mark_subtree_dirty`).
        return false;
    }

    let common = new.children.len().min(prev.children.len());
    let mut all_clean = new.children.len() == prev.children.len();
    for i in 0..common {
        let child_clean = graft_geometry(&mut new.children[i], &prev.children[i]);
        all_clean &= child_clean;
    }

    if all_clean {
        // Entire subtree (this node + all descendants) is unchanged. Children
        // were already grafted in place by the recursive calls above, so only
        // this node's own scalar fields need to come from `prev` — `new.children`
        // must NOT be replaced wholesale here. (BUG-341: an earlier version did
        // `*new = prev.clone()`, deep-cloning the whole subtree again at *every*
        // ancestor level on the way back up, i.e. O(depth) redundant clones of
        // the same already-grafted descendants for a linear chain of clean
        // ancestors.) `kind` still needs cloning (not just `==` — carries
        // post-layout payload e.g. InlineRun's laid-out `lines`, absent on the
        // freshly-built `new` side).
        new.rect = prev.rect;
        new.kind = prev.kind.clone();
        new.scroll_x = prev.scroll_x;
        new.scroll_y = prev.scroll_y;
        new.col_span = prev.col_span;
        new.row_span = prev.row_span;
        new.svg_group_transform = prev.svg_group_transform.clone();
        new.dirty = DirtyBits::CLEAN;
        return true;
    }

    // This node matches but a descendant changed or a child was appended/removed:
    // it must be re-laid-out (clean children grafted above are translated cheaply,
    // dirty/new children laid out fresh).
    new.dirty = DirtyBits::SELF_SIZE | DirtyBits::HAS_DIRTY_DESCENDANT;
    false
}

/// Compare the layout-affecting payload of two [`crate::box_tree::BoxKind`]s.
///
/// Container kinds (Block, FlowRoot, …) carry no size-affecting payload of their
/// own — their geometry comes from children + style, so the discriminant alone
/// decides equality. Content kinds carry data that affects size or paint
/// (inline text segments, image/iframe URLs, canvas dimensions, …); those are
/// compared field-by-field. `InlineRun` compares its `segments` (the pre-layout
/// inline content) so that text accumulating into an open element during
/// streaming is detected as changed. Differing discriminants are never equal.
fn kind_layout_eq(a: &crate::box_tree::BoxKind, b: &crate::box_tree::BoxKind) -> bool {
    use crate::box_tree::BoxKind::{
        Audio, Block, Canvas, FlowRoot, FormControl, Iframe, Image, InlineBlockRow, InlineRun,
        InlineSpace, Marker, Skip, TableRow, Video,
    };
    match (a, b) {
        (Block, Block)
        | (InlineBlockRow, InlineBlockRow)
        | (TableRow, TableRow)
        | (InlineSpace, InlineSpace)
        | (Skip, Skip)
        | (FlowRoot, FlowRoot) => true,
        (InlineRun { segments: sa, .. }, InlineRun { segments: sb, .. }) => segments_eq(sa, sb),
        (
            Image { src: s1, alt: a1, is_lazy: l1 },
            Image { src: s2, alt: a2, is_lazy: l2 },
        ) => s1 == s2 && a1 == a2 && l1 == l2,
        (Video { src: s1, poster: p1 }, Video { src: s2, poster: p2 }) => s1 == s2 && p1 == p2,
        (Canvas { width: w1, height: h1 }, Canvas { width: w2, height: h2 }) => {
            w1 == w2 && h1 == h2
        }
        (Audio { src: s1, controls: c1 }, Audio { src: s2, controls: c2 }) => {
            s1 == s2 && c1 == c2
        }
        (Iframe { src: s1, srcdoc: d1 }, Iframe { src: s2, srcdoc: d2 }) => s1 == s2 && d1 == d2,
        (FormControl { kind: k1 }, FormControl { kind: k2 }) => k1 == k2,
        (
            Marker { text: t1, position: p1, list_style_type: ls1, image: i1 },
            Marker { text: t2, position: p2, list_style_type: ls2, image: i2 },
        ) => t1 == t2 && p1 == p2 && ls1 == ls2 && i1 == i2,
        _ => false,
    }
}

/// Compare two `InlineRun` segment lists for layout equality.
///
/// Compares the size-affecting scalar fields of each [`crate::box_tree::InlineSegment`]
/// (text, inline-margin spaces, image source/width, forced break, element-box
/// flag, pseudo role). Per-segment `style` is intentionally not compared: during
/// streaming the stylesheet is stable for an unchanged text run, so identical
/// text implies identical style. A different count is always unequal.
fn segments_eq(a: &[crate::box_tree::InlineSegment], b: &[crate::box_tree::InlineSegment]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.text == y.text
            && x.img_src == y.img_src
            && x.forced_break == y.forced_break
            && x.is_element_box == y.is_element_box
            && x.pseudo_kind == y.pseudo_kind
            && (x.pre_space - y.pre_space).abs() < f32::EPSILON
            && (x.post_space - y.post_space).abs() < f32::EPSILON
            && (x.img_width - y.img_width).abs() < f32::EPSILON
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::{Rect, Size};
    use lumen_dom::NodeId;
    use crate::box_tree::{BoxKind, LayoutBox};
    use crate::style::ComputedStyle;

    fn leaf(id: u32, rect: Rect) -> LayoutBox {
        LayoutBox {
            node: NodeId::from_index(id as usize),
            rect,
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: DirtyBits::CLEAN,
        }
    }

    fn block_with_children(id: u32, rect: Rect, children: Vec<LayoutBox>) -> LayoutBox {
        LayoutBox {
            node: NodeId::from_index(id as usize),
            rect,
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children,
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: DirtyBits::CLEAN,
        }
    }

    // ── DirtyBits bit operations ──────────────────────────────────────────

    #[test]
    fn dirty_bits_default_is_clean() {
        let d = DirtyBits::default();
        assert!(d.is_clean());
        assert!(!d.is_dirty());
    }

    #[test]
    fn dirty_bits_self_size_is_dirty() {
        let d = DirtyBits::SELF_SIZE;
        assert!(d.is_dirty());
        assert!(!d.is_clean());
        assert!(d.contains(DirtyBits::SELF_SIZE));
        assert!(!d.contains(DirtyBits::HAS_DIRTY_DESCENDANT));
    }

    #[test]
    fn dirty_bits_bitor_combines() {
        let d = DirtyBits::SELF_SIZE | DirtyBits::HAS_DIRTY_DESCENDANT;
        assert!(d.contains(DirtyBits::SELF_SIZE));
        assert!(d.contains(DirtyBits::HAS_DIRTY_DESCENDANT));
        assert!(!d.contains(DirtyBits::SUBTREE));
    }

    #[test]
    fn dirty_bits_bitor_assign() {
        let mut d = DirtyBits::CLEAN;
        d |= DirtyBits::SELF_SIZE;
        assert!(d.contains(DirtyBits::SELF_SIZE));
    }

    // ── mark_dirty ────────────────────────────────────────────────────────

    #[test]
    fn mark_dirty_finds_leaf_node() {
        let child = leaf(2, Rect::new(0.0, 0.0, 100.0, 50.0));
        let mut root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 600.0), vec![child]);

        let found = mark_dirty(&mut root, NodeId::from_index(2));
        assert!(found);
        assert!(root.dirty.contains(DirtyBits::HAS_DIRTY_DESCENDANT));
        assert!(root.children[0].dirty.contains(DirtyBits::SELF_SIZE));
    }

    #[test]
    fn mark_dirty_returns_false_when_not_found() {
        let mut root = leaf(1, Rect::ZERO);
        let found = mark_dirty(&mut root, NodeId::from_index(99));
        assert!(!found);
        assert!(root.dirty.is_clean());
    }

    #[test]
    fn mark_dirty_propagates_to_all_ancestors() {
        // root (1) → mid (2) → leaf (3)
        let leaf_box = leaf(3, Rect::new(0.0, 10.0, 100.0, 20.0));
        let mid = block_with_children(2, Rect::new(0.0, 5.0, 100.0, 30.0), vec![leaf_box]);
        let mut root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 600.0), vec![mid]);

        mark_dirty(&mut root, NodeId::from_index(3));

        assert!(root.dirty.contains(DirtyBits::HAS_DIRTY_DESCENDANT));
        assert!(root.children[0].dirty.contains(DirtyBits::HAS_DIRTY_DESCENDANT));
        assert!(root.children[0].children[0].dirty.contains(DirtyBits::SELF_SIZE));
    }

    #[test]
    fn mark_dirty_set_marks_multiple_nodes() {
        let c1 = leaf(2, Rect::ZERO);
        let c2 = leaf(3, Rect::ZERO);
        let mut root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 600.0), vec![c1, c2]);

        mark_dirty_set(&mut root, &[NodeId::from_index(2), NodeId::from_index(3)]);

        assert!(root.dirty.contains(DirtyBits::HAS_DIRTY_DESCENDANT));
        assert!(root.children[0].dirty.contains(DirtyBits::SELF_SIZE));
        assert!(root.children[1].dirty.contains(DirtyBits::SELF_SIZE));
    }

    // ── clear_dirty ───────────────────────────────────────────────────────

    #[test]
    fn clear_dirty_clears_entire_subtree() {
        let mut root = leaf(1, Rect::ZERO);
        root.dirty = DirtyBits::SELF_SIZE | DirtyBits::HAS_DIRTY_DESCENDANT;
        let mut child = leaf(2, Rect::ZERO);
        child.dirty = DirtyBits::SELF_SIZE;
        root.children.push(child);

        clear_dirty(&mut root);

        assert!(root.dirty.is_clean());
        assert!(root.children[0].dirty.is_clean());
    }

    // ── translate_subtree ─────────────────────────────────────────────────

    #[test]
    fn translate_subtree_moves_all_rects() {
        let child = leaf(2, Rect::new(10.0, 20.0, 50.0, 30.0));
        let mut root = block_with_children(1, Rect::new(0.0, 0.0, 200.0, 100.0), vec![child]);

        translate_subtree(&mut root, 5.0, 10.0);

        assert!((root.rect.x - 5.0).abs() < f32::EPSILON);
        assert!((root.rect.y - 10.0).abs() < f32::EPSILON);
        assert!((root.children[0].rect.x - 15.0).abs() < f32::EPSILON);
        assert!((root.children[0].rect.y - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn translate_subtree_zero_is_noop() {
        let mut root = leaf(1, Rect::new(10.0, 20.0, 50.0, 30.0));
        translate_subtree(&mut root, 0.0, 0.0);
        assert!((root.rect.x - 10.0).abs() < f32::EPSILON);
        assert!((root.rect.y - 20.0).abs() < f32::EPSILON);
    }

    // ── incremental layout integration ────────────────────────────────────

    #[test]
    fn incremental_clean_root_is_noop() {
        // A fully clean tree passed through lay_out_incremental should stay at the
        // same position — nothing moves because dirty == CLEAN everywhere.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::{layout_measured_hyp, lay_out_incremental};
        use lumen_core::ext::NullHyphenationProvider;

        struct ZeroMeasurer;
        impl crate::TextMeasurer for ZeroMeasurer {
            fn char_width(&self, _: char, _: f32) -> f32 { 0.0 }
        }

        let html = r#"<div style="height:100px"></div><div style="height:50px"></div>"#;
        let doc = parse_html(html);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);
        let hp = NullHyphenationProvider;
        let m = ZeroMeasurer;

        let mut root = layout_measured_hyp(&doc, &sheet, vp, &m, &hp, false);
        // After clear_dirty the entire tree is clean.
        clear_dirty(&mut root);

        let orig_root_rect = root.rect;
        let pcb = Rect::new(0.0, 0.0, vp.width, vp.height);
        // Root is clean → lay_out_incremental translates it (by 0) and returns.
        lay_out_incremental(&mut root, 0.0, 0.0, vp.width, Some(vp.height), None, vp, pcb, &hp);

        assert!((root.rect.x - orig_root_rect.x).abs() < 0.5,
            "clean root x must not change: was {} got {}", orig_root_rect.x, root.rect.x);
        assert!((root.rect.y - orig_root_rect.y).abs() < 0.5,
            "clean root y must not change: was {} got {}", orig_root_rect.y, root.rect.y);
    }

    #[test]
    fn incremental_dirty_root_relays_out() {
        // A root marked SELF_SIZE must go through lay_out and update its rect.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::{layout_measured_hyp, lay_out_incremental};
        use lumen_core::ext::NullHyphenationProvider;

        struct ZeroMeasurer;
        impl crate::TextMeasurer for ZeroMeasurer {
            fn char_width(&self, _: char, _: f32) -> f32 { 0.0 }
        }

        let html = r#"<div style="height:80px"></div>"#;
        let doc = parse_html(html);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);
        let hp = NullHyphenationProvider;
        let m = ZeroMeasurer;

        let mut root = layout_measured_hyp(&doc, &sheet, vp, &m, &hp, false);
        clear_dirty(&mut root);

        // Mark root dirty so lay_out_incremental re-lays it out.
        root.dirty |= DirtyBits::SELF_SIZE;

        let pcb = Rect::new(0.0, 0.0, vp.width, vp.height);
        lay_out_incremental(&mut root, 0.0, 0.0, vp.width, Some(vp.height), None, vp, pcb, &hp);

        // After incremental, dirty bits must be cleared.
        assert!(root.dirty.is_clean(), "dirty bits must be cleared after lay_out_incremental");
    }

    // ── streaming graft (PH1-2b) ──────────────────────────────────────────

    struct FixedMeasurer;
    impl crate::TextMeasurer for FixedMeasurer {
        fn char_width(&self, _: char, size: f32) -> f32 { size * 0.5 }
    }

    /// Collect (node, rect) pairs in pre-order for geometry comparison.
    fn collect_rects(b: &LayoutBox, out: &mut Vec<(NodeId, Rect)>) {
        out.push((b.node, b.rect));
        for c in &b.children {
            collect_rects(c, out);
        }
    }

    fn full_layout(html: &str) -> LayoutBox {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_measured_hyp;
        use lumen_core::ext::NullHyphenationProvider;
        let doc = parse_html(html);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);
        layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false)
    }

    #[test]
    fn streaming_incremental_matches_full_layout() {
        // The geometry produced incrementally (reusing the prefix from a smaller
        // DOM) must match a full layout of the grown DOM exactly.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_streaming_incremental;
        use lumen_core::ext::NullHyphenationProvider;

        let prev = full_layout(
            r#"<div style="height:40px"></div><div style="height:60px"></div>"#,
        );

        // Grown DOM: same two divs + a third appended at the end.
        let grown = r#"<div style="height:40px"></div><div style="height:60px"></div><div style="height:30px"></div>"#;
        let doc = parse_html(grown);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);
        let incr = layout_streaming_incremental(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev,
        );

        let full = full_layout(grown);

        let mut a = Vec::new();
        let mut b = Vec::new();
        collect_rects(&incr, &mut a);
        collect_rects(&full, &mut b);
        assert_eq!(a.len(), b.len(), "box count must match full layout");
        for ((na, ra), (nb, rb)) in a.iter().zip(b.iter()) {
            assert_eq!(na, nb, "node order must match");
            assert!((ra.x - rb.x).abs() < 0.5 && (ra.y - rb.y).abs() < 0.5
                && (ra.width - rb.width).abs() < 0.5 && (ra.height - rb.height).abs() < 0.5,
                "rect mismatch for {na:?}: incr {ra:?} vs full {rb:?}");
        }
    }

    #[test]
    fn streaming_incremental_text_reflow_matches_full() {
        // Appending text to an existing paragraph must reflow that paragraph to
        // match a full layout (the InlineRun is detected as changed via segments).
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_streaming_incremental;
        use lumen_core::ext::NullHyphenationProvider;

        let prev = full_layout(r#"<p style="width:100px">hello</p>"#);
        let grown = r#"<p style="width:100px">hello world this is a longer run of text that wraps</p>"#;
        let doc = parse_html(grown);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);
        let incr = layout_streaming_incremental(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev,
        );
        let full = full_layout(grown);

        let mut a = Vec::new();
        let mut b = Vec::new();
        collect_rects(&incr, &mut a);
        collect_rects(&full, &mut b);
        assert_eq!(a.len(), b.len());
        for ((_, ra), (_, rb)) in a.iter().zip(b.iter()) {
            assert!((ra.height - rb.height).abs() < 0.5,
                "reflowed height must match: incr {} vs full {}", ra.height, rb.height);
        }
    }

    #[test]
    fn graft_identical_tree_is_all_clean() {
        let prev = leaf(2, Rect::new(0.0, 10.0, 100.0, 50.0));
        let mut prev_root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 60.0), vec![prev]);

        // Build a "fresh" copy with no geometry and all-dirty.
        let mut fresh = block_with_children(1, Rect::ZERO,
            vec![leaf(2, Rect::ZERO)]);
        mark_subtree_dirty(&mut fresh);

        let clean = graft_geometry(&mut fresh, &prev_root);
        assert!(clean, "identical tree must be fully clean");
        assert!(fresh.dirty.is_clean());
        assert!(fresh.children[0].dirty.is_clean());
        // Geometry was cloned from prev.
        assert!((fresh.children[0].rect.y - 10.0).abs() < f32::EPSILON);

        // Mutating prev_root afterwards must not affect fresh (deep clone).
        prev_root.children[0].rect.y = 999.0;
        assert!((fresh.children[0].rect.y - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn graft_appended_child_keeps_prefix_clean_parent_dirty() {
        let prev_root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 50.0),
            vec![leaf(2, Rect::new(0.0, 0.0, 100.0, 50.0))]);

        // Fresh tree: same child 2 + a new child 3 appended.
        let mut fresh = block_with_children(1, Rect::ZERO,
            vec![leaf(2, Rect::ZERO), leaf(3, Rect::ZERO)]);
        mark_subtree_dirty(&mut fresh);

        let clean = graft_geometry(&mut fresh, &prev_root);
        assert!(!clean, "parent with appended child cannot be fully clean");
        assert!(fresh.dirty.is_dirty(), "parent must stay dirty");
        assert!(fresh.children[0].dirty.is_clean(), "unchanged prefix child must be clean");
        assert!(fresh.children[1].dirty.is_dirty(), "appended child must be dirty");
    }

    #[test]
    fn graft_changed_style_marks_dirty() {
        let mut prev_root = leaf(1, Rect::new(0.0, 0.0, 100.0, 50.0));
        prev_root.style.font_size = 16.0;

        let mut fresh = leaf(1, Rect::ZERO);
        fresh.style.font_size = 24.0; // style changed
        mark_subtree_dirty(&mut fresh);

        let clean = graft_geometry(&mut fresh, &prev_root);
        assert!(!clean);
        assert!(fresh.dirty.is_dirty(), "changed style must keep box dirty");
    }

    #[test]
    fn incremental_preserves_clean_height() {
        // A clean leaf's height must be preserved after an incremental pass
        // that only translates it.
        let child = leaf(2, Rect::new(0.0, 100.0, 200.0, 50.0));
        let mut root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 600.0), vec![child]);

        // Translate root by dy=10 to simulate child_y shift
        translate_subtree(&mut root.children[0], 0.0, 10.0);

        assert!((root.children[0].rect.height - 50.0).abs() < f32::EPSILON,
            "clean child height must be preserved: got {}", root.children[0].rect.height);
        assert!((root.children[0].rect.y - 110.0).abs() < f32::EPSILON,
            "clean child y must be translated: got {}", root.children[0].rect.y);
    }

    // ── layout_mutation_incremental (M4) ──────────────────────────────────

    #[test]
    fn mutation_incremental_style_change_matches_full() {
        // A height change on one div must produce the same geometry as a full
        // layout: the changed div re-lays out, the unchanged one above stays in
        // place, and siblings below are translated to the new position.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_mutation_incremental;
        use lumen_core::ext::NullHyphenationProvider;

        let html_before = r#"<div style="height:40px"></div><div style="height:60px"></div>"#;
        let prev = full_layout(html_before);

        let html_after = r#"<div style="height:40px"></div><div style="height:80px"></div>"#;
        let doc = parse_html(html_after);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);
        let incr = layout_mutation_incremental(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev,
        );
        let full = full_layout(html_after);

        let mut a = Vec::new();
        let mut b = Vec::new();
        collect_rects(&incr, &mut a);
        collect_rects(&full, &mut b);
        assert_eq!(a.len(), b.len(), "box count must match full layout");
        for ((na, ra), (nb, rb)) in a.iter().zip(b.iter()) {
            assert_eq!(na, nb, "node order must match");
            assert!((ra.x - rb.x).abs() < 0.5 && (ra.y - rb.y).abs() < 0.5
                && (ra.width - rb.width).abs() < 0.5 && (ra.height - rb.height).abs() < 0.5,
                "rect mismatch for {na:?}: incr {ra:?} vs full {rb:?}");
        }
    }

    #[test]
    fn mutation_incremental_unchanged_dom_matches_full() {
        // When the DOM is identical to prev, mutation incremental must still
        // produce geometry equal to a full layout (all-clean fast path).
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_mutation_incremental;
        use lumen_core::ext::NullHyphenationProvider;

        let html = r#"<div style="height:50px"></div><div style="height:30px"></div>"#;
        let prev = full_layout(html);
        let doc = parse_html(html);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);
        let incr = layout_mutation_incremental(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev,
        );
        let full = full_layout(html);

        let mut a = Vec::new();
        let mut b = Vec::new();
        collect_rects(&incr, &mut a);
        collect_rects(&full, &mut b);
        assert_eq!(a.len(), b.len(), "box count must match");
        for ((na, ra), (nb, rb)) in a.iter().zip(b.iter()) {
            assert_eq!(na, nb);
            assert!((ra.x - rb.x).abs() < 0.5 && (ra.y - rb.y).abs() < 0.5
                && (ra.width - rb.width).abs() < 0.5 && (ra.height - rb.height).abs() < 0.5,
                "rect mismatch for {na:?}: incr {ra:?} vs full {rb:?}");
        }
    }

    // ── BUG-341 S5: layout_mutation_incremental_restyle ───────────────────

    #[test]
    fn mutation_incremental_restyle_hover_transition_matches_full() {
        // S5 wires the S3 incremental cascade into a real pipeline entry
        // point, combined with the existing graft-geometry reuse
        // (`layout_mutation_incremental`'s own mechanism) — this must
        // reproduce a full `layout_measured_hyp` bit-for-bit for the same
        // final state (brief §4 correctness gate), for a real hover
        // transition that also changes box geometry (`:hover { height }`),
        // not just a cascade-only style change.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::{
            layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental_restyle,
        };
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::{clear_interactive_state, restyle_root_set_for_state_change, set_interactive_state};
        use lumen_core::ext::NullHyphenationProvider;

        let html = r#"<ul id="menu">
            <li id="a" class="item">a</li>
            <li id="b" class="item">b</li>
            <li id="c" class="item">c</li>
        </ul>
        <div id="unrelated"><p>x</p><p>y</p><p>z</p></div>"#;
        let css = r#"
            .item { color: black; padding: 4px; height: 20px; }
            .item:hover { color: blue; height: 30px; }
            .item:hover + .item { color: green; }
        "#;
        let doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);

        let a = doc.find_by_id("a").expect("#a must exist");
        let b = doc.find_by_id("b").expect("#b must exist");

        // Baseline pass (`prev` for the incremental call): #a hovered.
        set_interactive_state(Some(a), None, None);
        let (prev, prev_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );

        // Incremental: hover moves from #a to #b.
        set_interactive_state(Some(b), None, None);
        let dirty_roots = restyle_root_set_for_state_change(&doc, Some(a), Some(b));
        let delta = RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, dom_content_stable: true };
        set_incremental_restyle(true);
        let (incr, _incr_counters) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        set_incremental_restyle(false);

        // Reference: full layout of the post-transition state.
        set_interactive_state(Some(b), None, None);
        let full = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);
        clear_interactive_state();

        let mut ia = Vec::new();
        let mut fb = Vec::new();
        collect_rects(&incr, &mut ia);
        collect_rects(&full, &mut fb);
        assert_eq!(ia.len(), fb.len(), "box count must match full layout");
        for ((na, ra), (nb, rb)) in ia.iter().zip(fb.iter()) {
            assert_eq!(na, nb, "node order must match");
            assert!((ra.x - rb.x).abs() < 0.5 && (ra.y - rb.y).abs() < 0.5
                && (ra.width - rb.width).abs() < 0.5 && (ra.height - rb.height).abs() < 0.5,
                "rect mismatch for {na:?}: incr {ra:?} vs full {rb:?}");
        }
    }

    #[test]
    fn mutation_incremental_restyle_unchanged_state_matches_full() {
        // A no-op transition (`dirty_roots` empty) must still produce
        // geometry equal to a full layout — the all-clean fast path through
        // both the incremental cascade and graft_geometry.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::{
            layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental_restyle,
        };
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::{clear_interactive_state, restyle_root_set_for_state_change};
        use lumen_core::ext::NullHyphenationProvider;

        let html = r#"<div style="height:50px"></div><div style="height:30px"></div>"#;
        let doc = parse_html(html);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);

        let (prev, prev_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );

        let dirty_roots = restyle_root_set_for_state_change(&doc, None, None);
        assert!(dirty_roots.is_empty(), "no-op transition must yield an empty root-set");
        let delta = RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, dom_content_stable: true };
        set_incremental_restyle(true);
        let (incr, _incr_counters) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        set_incremental_restyle(false);
        clear_interactive_state();

        let full = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);

        let mut ia = Vec::new();
        let mut fb = Vec::new();
        collect_rects(&incr, &mut ia);
        collect_rects(&full, &mut fb);
        assert_eq!(ia.len(), fb.len(), "box count must match");
        for ((na, ra), (nb, rb)) in ia.iter().zip(fb.iter()) {
            assert_eq!(na, nb);
            assert!((ra.x - rb.x).abs() < 0.5 && (ra.y - rb.y).abs() < 0.5
                && (ra.width - rb.width).abs() < 0.5 && (ra.height - rb.height).abs() < 0.5,
                "rect mismatch for {na:?}: incr {ra:?} vs full {rb:?}");
        }
    }

    // ── ADR-016 M4.1: parallel selector matching ──────────────────────────────

    #[test]
    fn parallel_grid_layout_matches_sequential() {
        // A grid container with 10 items triggers the rayon parallel path
        // (RAYON_MIN_FLEX_CHILDREN = 8). The parallel build_box must produce
        // the same box tree as a sequential build on the same HTML.
        //
        // We run layout_measured_hyp twice on identical HTML and compare all
        // box rects. Since both calls take the same code paths (one is
        // parallel via rayon, the other gets the same rayon path because
        // parallelism is transparent), we primarily verify: (a) the child
        // count is correct and (b) no out-of-order items appear.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_measured_hyp;
        use lumen_core::ext::NullHyphenationProvider;

        // 10 grid items → above the RAYON_MIN_FLEX_CHILDREN=8 threshold.
        let html = r#"<div style="display:grid;grid-template-columns:repeat(5,1fr)">
            <div style="height:20px"></div>
            <div style="height:30px"></div>
            <div style="height:25px"></div>
            <div style="height:10px"></div>
            <div style="height:40px"></div>
            <div style="height:15px"></div>
            <div style="height:35px"></div>
            <div style="height:20px"></div>
            <div style="height:45px"></div>
            <div style="height:22px"></div>
        </div>"#;
        let doc = parse_html(html);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);

        let a = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);
        let b = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);

        let mut ra = Vec::new();
        let mut rb = Vec::new();
        collect_rects(&a, &mut ra);
        collect_rects(&b, &mut rb);

        assert_eq!(ra.len(), rb.len(), "both layouts must produce the same box count");
        for ((na, a_rect), (nb, b_rect)) in ra.iter().zip(rb.iter()) {
            assert_eq!(na, nb, "box order must be identical");
            assert!(
                (a_rect.x - b_rect.x).abs() < 0.5
                    && (a_rect.y - b_rect.y).abs() < 0.5
                    && (a_rect.width - b_rect.width).abs() < 0.5
                    && (a_rect.height - b_rect.height).abs() < 0.5,
                "rect mismatch at {na:?}: first={a_rect:?} second={b_rect:?}",
            );
        }
    }

    #[test]
    fn parallel_flex_item_order_is_preserved() {
        // 8 flex items with explicitly distinct heights — the parallel path
        // must deliver children in DOM order, not rayon scheduler order.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_measured_hyp;
        use lumen_core::ext::NullHyphenationProvider;

        // Heights are all different so any reordering would be visible.
        let html = r#"<div style="display:flex;flex-direction:column">
            <div style="height:10px"></div>
            <div style="height:20px"></div>
            <div style="height:30px"></div>
            <div style="height:40px"></div>
            <div style="height:50px"></div>
            <div style="height:60px"></div>
            <div style="height:70px"></div>
            <div style="height:80px"></div>
        </div>"#;
        let doc = parse_html(html);
        let sheet = parse_css("");
        let vp = Size::new(800.0, 600.0);

        let root = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);

        // The flex container is the first child of the root (html > body > div).
        // Walk to the flex container's children and verify their heights in order.
        let mut items: Vec<f32> = Vec::new();
        fn collect_flex_items(b: &crate::box_tree::LayoutBox, depth: usize, items: &mut Vec<f32>) {
            use crate::style::Display;
            if depth == 0 {
                for c in &b.children { collect_flex_items(c, depth + 1, items); }
            } else if matches!(b.style.display, Display::Flex) {
                for c in &b.children { items.push(c.rect.height); }
            } else {
                for c in &b.children { collect_flex_items(c, depth + 1, items); }
            }
        }
        collect_flex_items(&root, 0, &mut items);

        // Items must appear in document order (ascending heights 10..80).
        assert_eq!(items.len(), 8, "must have 8 flex items");
        for (i, &h) in items.iter().enumerate() {
            let expected = ((i + 1) * 10) as f32;
            assert!(
                (h - expected).abs() < 0.5,
                "item {i} must have height {expected} not {h}",
            );
        }
    }

    // ── BUG-341 S3: incremental-cascade differential tests ─────────────────────
    //
    // The incremental cascade must reproduce the full cascade's per-node
    // `ComputedStyle` map bit-for-bit for the same final state (brief §4
    // "Correctness gate"). `incremental_cascade` below now calls the real
    // entry point (`incremental_precompute_counters`); the trivial/interactive
    // tests exercise the reuse path across a *steady state* (nothing changed),
    // and the transition tests further down exercise a genuine root-set
    // derivation (`restyle_root_set_for_state_change` /
    // `restyle_root_set_for_node_change`) and assert it recomputes a strict
    // subset of the document.

    /// Full-cascade reference: the `styles` map a complete `layout_measured_hyp`
    /// pass produces for `html` under the `css` stylesheet.
    fn full_cascade(html: &str, css: &str) -> crate::counters::CounterMap {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_measured_hyp_with_counters;
        use lumen_core::ext::NullHyphenationProvider;
        let doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);
        layout_measured_hyp_with_counters(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false).1
    }

    /// Incremental-cascade result via the real incremental entry point
    /// (BUG-341 S3): builds a baseline cascade over the same `html`/`css`, then
    /// re-cascades through [`crate::counters::incremental_precompute_counters`]
    /// with an *empty* dirty root-set — the steady-state case where nothing
    /// changed since the baseline. Every node must reuse its cached
    /// `ComputedStyle` (none are in the root-set), so this exercises the reuse
    /// path for the whole document while still having to match a fresh full
    /// recompute exactly.
    fn incremental_cascade(html: &str, css: &str) -> crate::counters::CounterMap {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use lumen_dom::build_flat_tree;
        use crate::counters::{incremental_precompute_counters, precompute_counters, set_incremental_restyle, RestyleDelta};
        let doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);
        let flat = build_flat_tree(&doc);

        crate::style::invalidate_rule_idx_cache();
        let prev = precompute_counters(&doc, &sheet, vp, &flat, false);

        set_incremental_restyle(true);
        crate::style::invalidate_rule_idx_cache();
        let delta = RestyleDelta {
            prev_styles: prev.styles(),
            dirty_roots: Default::default(),
            dom_content_stable: true,
        };
        let result = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, &delta);
        set_incremental_restyle(false);
        result
    }

    /// Assert two cascade maps are identical: same node set, same `ComputedStyle`
    /// per node. Any divergence is a too-narrow invalidation set (brief §4), not
    /// an acceptable trade-off.
    fn assert_cascades_eq(full: &crate::counters::CounterMap, incr: &crate::counters::CounterMap) {
        assert_eq!(
            incr.styles(),
            full.styles(),
            "incremental cascade must reproduce the full cascade exactly",
        );
    }

    #[test]
    fn incr_cascade_matches_full_trivial() {
        // Steady state: the whole document is reused from `prev_styles`
        // (empty dirty root-set). Must still match a fresh full recompute —
        // this is the reuse path's correctness baseline.
        let html = r#"<div class="a"><p>one</p><p>two</p></div><span>three</span>"#;
        let css = ".a { color: red; } p { font-weight: bold; }";
        let full = full_cascade(html, css);
        let incr = incremental_cascade(html, css);
        assert_cascades_eq(&full, &incr);
    }

    #[test]
    fn incr_cascade_matches_full_interactive_rules() {
        // Same steady-state reuse check, but against a doc with
        // :hover/:focus-dependent rules — the hardest correctness case for the
        // cascade itself (though this test doesn't change any interactive
        // state; see `incr_cascade_hover_transition_*` below for that).
        let html = r#"<ul id="menu">
            <li class="item">a</li>
            <li class="item">b</li>
            <li class="item">c</li>
        </ul>"#;
        let css = r#"
            .item { color: black; padding: 4px; }
            .item:hover { color: blue; }
            #menu:hover .item { background: gray; }
            .item:focus + .item { color: green; }
        "#;
        let full = full_cascade(html, css);
        let incr = incremental_cascade(html, css);
        assert_cascades_eq(&full, &incr);
    }

    #[test]
    fn incr_cascade_hover_transition_matches_full_and_recomputes_subset() {
        // BUG-341 S3: a real interactive-state change — hover moves from one
        // `<li>` to its sibling — must produce a cascade bit-identical to a
        // fresh full recompute of the post-transition state, while recomputing
        // strictly fewer nodes than a full cascade (proving
        // `restyle_root_set_for_state_change` is doing real, not degenerate,
        // work: the unrelated `#unrelated` subtree must be untouched).
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use lumen_dom::build_flat_tree;
        use crate::counters::{
            incremental_precompute_counters, precompute_counters, recompute_count,
            reset_recompute_count, set_incremental_restyle, RestyleDelta,
        };
        use crate::style::{clear_interactive_state, restyle_root_set_for_state_change, set_interactive_state};

        let html = r#"<ul id="menu">
            <li id="a" class="item">a</li>
            <li id="b" class="item">b</li>
            <li id="c" class="item">c</li>
        </ul>
        <div id="unrelated"><p>x</p><p>y</p><p>z</p></div>"#;
        let css = r#"
            .item { color: black; padding: 4px; }
            .item:hover { color: blue; }
            .item:hover + .item { color: green; }
        "#;
        let doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);
        let flat = build_flat_tree(&doc);

        let a = doc.find_by_id("a").expect("#a must exist");
        let b = doc.find_by_id("b").expect("#b must exist");

        // Baseline: #a hovered.
        set_interactive_state(Some(a), None, None);
        crate::style::invalidate_rule_idx_cache();
        let baseline = precompute_counters(&doc, &sheet, vp, &flat, false);
        let total_nodes = baseline.styles().len();

        // Reference: full recompute with hover moved to #b.
        set_interactive_state(Some(b), None, None);
        crate::style::invalidate_rule_idx_cache();
        let full_after = precompute_counters(&doc, &sheet, vp, &flat, false);

        // Incremental: same transition, conservative root-set derived from it.
        let dirty_roots = restyle_root_set_for_state_change(&doc, Some(a), Some(b));
        let delta = RestyleDelta { prev_styles: baseline.styles(), dirty_roots, dom_content_stable: true };
        set_incremental_restyle(true);
        reset_recompute_count();
        crate::style::invalidate_rule_idx_cache();
        let incr_after = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, &delta);
        let recomputed = recompute_count() as usize;
        set_incremental_restyle(false);
        clear_interactive_state();

        assert_eq!(
            incr_after.styles(), full_after.styles(),
            "incremental hover transition must reproduce the full cascade exactly",
        );
        assert!(
            recomputed < total_nodes,
            "incremental cascade recomputed {recomputed}/{total_nodes} nodes — a \
             hover move between #a/#b siblings should not force-recompute the \
             unrelated #unrelated subtree",
        );
    }

    #[test]
    fn incr_cascade_class_change_matches_full_and_recomputes_subset() {
        // BUG-341 S3: a DOM class mutation on a single node must match a full
        // recompute of the new state while only recomputing that node's parent
        // subtree (brief §4 — no `:has()` in this engine, so unlike interactive
        // state, no ancestor invalidation is needed for attribute/class changes).
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use lumen_dom::{build_flat_tree, NodeData};
        use crate::counters::{
            incremental_precompute_counters, precompute_counters, recompute_count,
            reset_recompute_count, set_incremental_restyle, RestyleDelta,
        };
        use crate::style::restyle_root_set_for_node_change;

        let css = r#"
            .item { color: black; }
            .item.active { color: blue; }
            .item.active + .item { color: green; }
        "#;
        let html = r#"<ul id="menu">
            <li id="a" class="item">a</li>
            <li id="b" class="item">b</li>
            <li id="c" class="item">c</li>
        </ul>
        <div id="unrelated"><p>x</p><p>y</p><p>z</p></div>"#;
        let mut doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);
        let flat = build_flat_tree(&doc);

        crate::style::invalidate_rule_idx_cache();
        let baseline = precompute_counters(&doc, &sheet, vp, &flat, false);
        let total_nodes = baseline.styles().len();

        let a = doc.find_by_id("a").expect("#a must exist");
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(a).data {
            for attr in attrs.iter_mut() {
                if attr.name.local == "class" {
                    attr.value = "item active".to_string();
                }
            }
        }

        crate::style::invalidate_rule_idx_cache();
        let full_after = precompute_counters(&doc, &sheet, vp, &flat, false);

        let dirty_roots = restyle_root_set_for_node_change(&doc, [a]);
        // BUG-341 S4: a DOM class mutation is NOT `dom_content_stable` — box-build
        // reuse must not trust style-equality alone here (see `RestyleDelta` doc).
        let delta = RestyleDelta { prev_styles: baseline.styles(), dirty_roots, dom_content_stable: false };
        set_incremental_restyle(true);
        reset_recompute_count();
        crate::style::invalidate_rule_idx_cache();
        let incr_after = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, &delta);
        let recomputed = recompute_count() as usize;
        set_incremental_restyle(false);

        assert_eq!(
            incr_after.styles(), full_after.styles(),
            "incremental class-change cascade must reproduce the full cascade exactly",
        );
        assert!(
            recomputed < total_nodes,
            "incremental cascade recomputed {recomputed}/{total_nodes} nodes — a \
             class change on #a should not force-recompute the unrelated \
             #unrelated subtree",
        );
    }
}
