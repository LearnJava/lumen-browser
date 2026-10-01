//! BUG-1238 — post-layout collectors for an *incremental* same-tick flush.
//!
//! A flush that restyled only `dirty_roots` still has to refresh every JS-visible
//! cache entry whose value changed, and that set is wider than the dirty
//! subtrees: a box after a resized sibling is translated wholesale, and every
//! ancestor of a dirty root may have grown. [`ScopedCollection::plan`] finds
//! exactly those boxes by walking the fresh tree top-down and pruning at the
//! subtrees the cascade left alone, **provided their geometry is where the
//! published map already says it is**; the `collect_*` methods then rebuild the
//! planned boxes' entries and overwrite them in the caches.

use crate::resolved_geometry::{self, GeomCtx};
use crate::{
    box_published_rect, collect_boxed_node_ids, collect_client_rects_box, collect_client_rects_rec,
    collect_computed_styles_box, collect_computed_styles_rec, collect_layout_rects_box,
    collect_layout_rects_rec, BoxKind, LayoutBox, Position,
};
use lumen_dom::NodeId;
use std::collections::{HashMap, HashSet};

type StyleMaps = HashMap<u32, HashMap<String, String>>;

/// One box the collectors must visit.
struct Item<'a> {
    b: &'a LayoutBox,
    /// The geometry context `b` itself is resolved in (its parent's child context).
    ctx: GeomCtx,
    /// `true` — the whole subtree is re-collected (a dirty root, or a clean subtree
    /// that moved); `false` — only `b`'s own entries (an ancestor of a dirty root,
    /// whose own size or position may have changed while its children's did not).
    whole: bool,
    /// Whether the computed-style map needs this item. `false` for a clean
    /// subtree that was merely translated vertically: see [`translation_keeps_styles`].
    styles: bool,
}

/// The boxes of an incrementally laid-out tree whose collector entries may differ
/// from the ones already published. Pre-order, so "first box in tree order wins"
/// keeps the meaning it has in the whole-tree collectors.
pub struct ScopedCollection<'a> {
    items: Vec<Item<'a>>,
}

impl<'a> ScopedCollection<'a> {
    /// Walks `root` (the tree the incremental layout just produced) top-down:
    ///
    /// * a box of a `dirty_roots` node — collected whole;
    /// * a box of a `clean_subtrees` node (nothing inside it was restyled or
    ///   mutated) — skipped if its published rect in `layout_rects` equals the one
    ///   it has now, since the layout moves such a subtree as one piece and an
    ///   unmoved root therefore means unmoved descendants; collected whole
    ///   otherwise (it was shifted, or is new);
    /// * anything else — an ancestor of a dirty root, or a box the cascade could
    ///   not vouch for: its own entries are rebuilt and the walk descends.
    ///
    /// Cost is the dirty and shifted subtrees plus the spine and its children,
    /// not the document — a flush that changes nothing around it compares a few
    /// hundred rects. When `clean_subtrees` is empty (the content record was
    /// unavailable) nothing can be pruned and every box is visited.
    pub fn plan(
        root: &'a LayoutBox,
        dirty_roots: &HashSet<NodeId>,
        clean_subtrees: &HashSet<NodeId>,
        layout_rects: &HashMap<u32, [f32; 4]>,
        viewport: lumen_core::geom::Size,
    ) -> Self {
        let mut items = Vec::new();
        // The third member: the parent's published `x`/`width` are where the map
        // says they are, i.e. its content box — this box's flow containing block —
        // did not move horizontally. The root's parent is the initial containing
        // block, which a viewport change would have sent down the full path.
        let mut stack = vec![(root, GeomCtx::root(viewport), true)];
        while let Some((b, ctx, parent_stable)) = stack.pop() {
            if dirty_roots.contains(&b.node) {
                items.push(Item { b, ctx, whole: true, styles: true });
                continue;
            }
            let r = box_published_rect(b);
            let published = layout_rects.get(&(b.node.index() as u32));
            if clean_subtrees.contains(&b.node) {
                if published != Some(&[r.x, r.y, r.width, r.height]) {
                    let styles = !translation_keeps_styles(b, r, published, parent_stable);
                    items.push(Item { b, ctx, whole: true, styles });
                }
                continue;
            }
            items.push(Item { b, ctx, whole: false, styles: true });
            let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
            let stable = published.is_some_and(|p| p[0] == r.x && p[2] == r.width);
            stack.extend(b.children.iter().rev().map(|c| (c, child_ctx, stable)));
        }
        Self { items }
    }

    /// Rebuilds the planned boxes' `getBoundingClientRect` entries and overwrites
    /// them in `out`. Entries for nodes the plan does not reach are left alone.
    pub fn collect_layout_rects(&self, doc: &lumen_dom::Document, out: &mut HashMap<u32, [f32; 4]>) {
        let mut fresh = HashMap::new();
        for it in &self.items {
            if it.whole {
                collect_layout_rects_rec(doc, it.b, &mut fresh);
            } else {
                collect_layout_rects_box(doc, it.b, &mut fresh);
            }
        }
        out.extend(fresh);
    }

    /// [`Self::collect_layout_rects`] for `getClientRects`. The `boxed` set a
    /// single `InlineRun` needs is its own subtree's — an inline-block nested in
    /// a line is a descendant of the run's box.
    pub fn collect_client_rects(&self, doc: &lumen_dom::Document, out: &mut HashMap<u32, Vec<[f32; 4]>>) {
        let mut fresh = HashMap::new();
        for it in &self.items {
            let needs_boxed = it.whole || matches!(it.b.kind, BoxKind::InlineRun { .. });
            let mut boxed = HashSet::new();
            if needs_boxed {
                collect_boxed_node_ids(it.b, &mut boxed);
            }
            if it.whole {
                collect_client_rects_rec(doc, it.b, &boxed, &mut fresh);
            } else {
                collect_client_rects_box(doc, it.b, &boxed, &mut fresh);
            }
        }
        out.extend(fresh);
    }

    /// [`Self::collect_layout_rects`] for `getComputedStyle`'s used geometry.
    ///
    /// The `Display::Contents` backfill [`crate::collect_computed_styles`] runs
    /// after its walk is intentionally not replicated: it re-derives entries for
    /// nodes that own no box from the whole-document cascade, and the caller
    /// maintains an already-complete map.
    pub fn collect_computed_styles(
        &self,
        doc: &lumen_dom::Document,
        viewport: lumen_core::geom::Size,
        out: &mut StyleMaps,
    ) {
        let mut fresh = HashMap::new();
        for it in self.items.iter().filter(|it| it.styles) {
            if it.whole {
                collect_computed_styles_rec(doc, it.b, it.ctx, viewport, &mut fresh);
            } else {
                collect_computed_styles_box(doc, it.b, &it.ctx, viewport, &mut fresh);
            }
        }
        out.extend(fresh);
    }
}

/// Whether the computed-style entries of a clean subtree rooted at `b` survive
/// its move from the `published` rect to `now`.
///
/// Collecting a computed-style map costs ~100 µs a box (a hundred times the rect
/// collectors), and a shifted subtree is usually shifted *vertically* because a
/// sibling above it changed height — nothing in the map moved. The used values
/// that do depend on where a box sits are: `margin-left`/`-right` of an in-flow
/// block (`x` against its containing block), and the insets of a positioned box
/// (against its containing block's height or position). So the entries are kept
/// when the root kept its `x` and size, its parent's content box kept its `x`
/// and width (`parent_stable`), and nothing in the subtree is positioned — a
/// relative `top: 10%` resolves against a height that may have changed, an
/// absolute inset against a containing block that may sit outside the subtree.
fn translation_keeps_styles(
    b: &LayoutBox,
    now: lumen_core::geom::Rect,
    published: Option<&[f32; 4]>,
    parent_stable: bool,
) -> bool {
    let Some(p) = published else { return false };
    parent_stable
        && p[0] == now.x
        && p[2] == now.width
        && p[3] == now.height
        && !has_positioned_box(b)
}

fn has_positioned_box(root: &LayoutBox) -> bool {
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        if !matches!(b.style.position, Position::Static | Position::Sticky) {
            return true;
        }
        stack.extend(b.children.iter());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;

    const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };

    fn laid(a_height: u32) -> LayoutBox {
        let doc = lumen_html_parser::parse(&format!(
            "<body style=\"margin:0\"><div style=\"height:{a_height}px\"></div><div style=\"height:20px\"></div></body>"
        ));
        crate::layout(&doc, &lumen_css_parser::parse(""), VIEWPORT)
    }

    fn block_with_height(root: &LayoutBox, h: f32) -> &LayoutBox {
        let mut stack = vec![root];
        while let Some(b) = stack.pop() {
            if matches!(b.kind, BoxKind::Block) && b.rect.height == h && b.children.is_empty() {
                return b;
            }
            stack.extend(b.children.iter());
        }
        panic!("no childless block of height {h}");
    }

    fn published(root: &LayoutBox) -> HashMap<u32, [f32; 4]> {
        // `collect_layout_rects` needs the document only for inline runs, of which there are none.
        crate::collect_layout_rects(root, &lumen_html_parser::parse(""))
    }

    /// `(whole, needs_styles)` of every planned item whose box has the given height.
    fn planned(plan: &ScopedCollection<'_>, h: f32) -> Vec<(bool, bool)> {
        plan.items.iter().filter(|it| it.b.rect.height == h).map(|it| (it.whole, it.styles)).collect()
    }

    #[test]
    fn a_sibling_moved_by_a_resize_is_planned_but_keeps_its_computed_styles() {
        let before = laid(50);
        let after = laid(100);
        let (a, b) = (block_with_height(&after, 100.0), block_with_height(&after, 20.0));
        let dirty = HashSet::from([a.node]);
        let clean = HashSet::from([b.node]);
        let plan = ScopedCollection::plan(&after, &dirty, &clean, &published(&before), VIEWPORT);
        assert_eq!(planned(&plan, 100.0), [(true, true)], "the dirty root is collected whole");
        // Moved 50 → 100 px down with the same size and `x`: rect entries refresh, the
        // computed-style map (a hundred times dearer) does not.
        assert_eq!(planned(&plan, 20.0), [(true, false)]);
    }

    #[test]
    fn a_clean_subtree_that_did_not_move_is_pruned() {
        let before = laid(50);
        let after = laid(50);
        let (a, b) = (block_with_height(&after, 50.0), block_with_height(&after, 20.0));
        let dirty = HashSet::from([b.node]);
        let clean = HashSet::from([a.node]);
        let plan = ScopedCollection::plan(&after, &dirty, &clean, &published(&before), VIEWPORT);
        assert!(planned(&plan, 50.0).is_empty(), "the unmoved clean sibling is skipped");
        assert_eq!(planned(&plan, 20.0), [(true, true)]);
    }

    #[test]
    fn nothing_is_pruned_without_a_clean_record() {
        let after = laid(50);
        let plan = ScopedCollection::plan(&after, &HashSet::new(), &HashSet::new(), &published(&after), VIEWPORT);
        let mut boxes = 0;
        let mut stack = vec![&after];
        while let Some(b) = stack.pop() {
            boxes += 1;
            stack.extend(b.children.iter());
        }
        assert_eq!(plan.items.len(), boxes);
    }
}
