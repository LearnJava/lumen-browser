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
        // Entire subtree (this node + all descendants) is unchanged: clone the
        // previous laid-out box wholesale so paint-side fragments (InlineRun
        // `lines`, etc.) and rects are reused verbatim, then mark clean.
        *new = prev.clone();
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
}
