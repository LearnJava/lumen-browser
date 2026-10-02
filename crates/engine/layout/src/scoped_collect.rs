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

use crate::box_tree::BoxRole;
use crate::resolved_geometry::{self, GeomCtx};
use crate::style::ContainerType;
use crate::{
    box_published_rect, collect_boxed_node_ids, collect_client_rects_box, collect_client_rects_rec,
    collect_computed_styles_box, collect_computed_styles_parts, collect_layout_rects_box,
    collect_layout_rects_rec, BoxKind, CounterMap, LayoutBox, Position,
};
use lumen_dom::NodeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type StyleMaps = HashMap<u32, HashMap<String, String>>;

/// BUG-935 срез 59 — which nodes of an incremental flush really changed, as far
/// as a computed-style entry can tell.
///
/// A dirty root is re-cascaded as a whole, but an element the cascade handed back
/// an equal style for has nothing new to say in the computed-style map: that map
/// costs ~175 µs a box, and on a page whose forced-reflow loop touches one
/// `span` the dirty root is the whole document. [`ScopedCollection::plan`] uses
/// this to leave such entries published.
pub struct ChangedNodes {
    /// Elements the cascade recomputed to a *different* style.
    own: HashSet<NodeId>,
    /// The subset of `own` whose change can alter what its *descendants'* entries
    /// are resolved in — see [`context_style_eq`]. A new background colour is in
    /// `own` and not here.
    context: HashSet<NodeId>,
    /// `own`, the nodes whose content changed, and every ancestor of either: a node
    /// outside it has an unchanged subtree, style and content alike.
    closure: HashSet<NodeId>,
}

impl ChangedNodes {
    /// `content` is the document's own record of the nodes whose content changed
    /// since the previous flush (see `ContentDirty::Nodes`); a flush that does not
    /// have a complete one must not build this at all.
    ///
    /// `None` for a document with an author shadow root — the closure walks DOM
    /// parents, which is not the composed tree there.
    pub fn new(doc: &lumen_dom::Document, counters: &CounterMap, content: &HashSet<NodeId>) -> Option<Self> {
        if doc.has_author_shadow_roots() {
            return None;
        }
        let styles = counters.styles();
        let mut own = HashSet::new();
        let mut context = HashSet::new();
        for (&id, old) in counters.replaced_styles() {
            match styles.get(&id) {
                Some(new) if Arc::ptr_eq(old, new) || **old == **new => {}
                Some(new) => {
                    own.insert(id);
                    if !context_style_eq(old, new) {
                        context.insert(id);
                    }
                }
                None => {
                    own.insert(id);
                    context.insert(id);
                }
            }
        }
        let mut closure = HashSet::with_capacity(own.len() + content.len());
        for &seed in own.iter().chain(content.iter()) {
            let mut cur = Some(seed).filter(|&id| doc.contains_id(id));
            while let Some(id) = cur.filter(|&id| closure.insert(id)) {
                cur = doc.get(id).parent;
            }
        }
        Some(Self { own, context, closure })
    }
}

/// Whether two styles of one element agree on everything the geometry context its
/// children are resolved in is made of ([`resolved_geometry::child_ctx`]): the
/// content box (padding, borders, `em` for either), whether it is positioned or
/// lays out block flow — plus the container fields, since a container query
/// restyles descendants after layout.
fn context_style_eq(a: &crate::style::ComputedStyle, b: &crate::style::ComputedStyle) -> bool {
    a.padding_top == b.padding_top
        && a.padding_right == b.padding_right
        && a.padding_bottom == b.padding_bottom
        && a.padding_left == b.padding_left
        && a.border_top_width == b.border_top_width
        && a.border_right_width == b.border_right_width
        && a.border_bottom_width == b.border_bottom_width
        && a.border_left_width == b.border_left_width
        && a.font_size == b.font_size
        && a.display == b.display
        && a.position == b.position
        && a.container_type == b.container_type
        && a.container_name == b.container_name
}

/// Computed-style entries a [`ScopedCollection`] proved unchanged and leaves alone.
#[derive(Default)]
struct StyleSkips {
    /// Nodes whose own entry is still right: style, used geometry and the context
    /// it is resolved in are all what they were when it was published.
    own: HashSet<u32>,
    /// Owners of an `InlineRun` whose subtree is untouched, so the entries it
    /// flattens (text and plain inline elements) are still right.
    runs: HashSet<u32>,
}

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
    skips: StyleSkips,
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
    ///
    /// BUG-935 срез 59: with `changed`, a box inside a whole item whose computed
    /// style entry provably did not change is left out of the *computed-style*
    /// collection ([`Self::keeps_computed_style`]); the rect collectors still
    /// visit every box of the item, they are two orders of magnitude cheaper.
    pub fn plan(
        root: &'a LayoutBox,
        dirty_roots: &HashSet<NodeId>,
        clean_subtrees: &HashSet<NodeId>,
        layout_rects: &HashMap<u32, [f32; 4]>,
        viewport: lumen_core::geom::Size,
        changed: Option<&ChangedNodes>,
    ) -> Self {
        let mut items = Vec::new();
        let mut skips = StyleSkips::default();
        let mut seen = HashSet::new();
        // The third member: the parent's published `x`/`width` are where the map
        // says they are, i.e. its content box — this box's flow containing block —
        // did not move horizontally. The root's parent is the initial containing
        // block, which a viewport change would have sent down the full path.
        // The fourth: the same holds for every ancestor and none of them changed
        // what the context is made of, so the context `apply_used_geometry` resolves this box in is the
        // one its published entry was resolved in ([`chain_through`]).
        let mut stack = vec![(root, GeomCtx::root(viewport), true, true)];
        while let Some((b, ctx, parent_stable, chain)) = stack.pop() {
            if dirty_roots.contains(&b.node) {
                items.push(Item { b, ctx, whole: true, styles: true });
                if let Some(changed) = changed {
                    plan_style_skips(b, ctx, chain, layout_rects, viewport, changed, &mut seen, &mut skips);
                }
                continue;
            }
            let r = box_published_rect(b);
            let published = layout_rects.get(&(b.node.index() as u32));
            if clean_subtrees.contains(&b.node) {
                if published != Some(&[r.x, r.y, r.width, r.height]) {
                    let styles = !translation_keeps_styles(b, r, published, parent_stable);
                    items.push(Item { b, ctx, whole: true, styles });
                    if styles && let Some(changed) = changed {
                        plan_style_skips(b, ctx, chain, layout_rects, viewport, changed, &mut seen, &mut skips);
                    }
                }
                continue;
            }
            items.push(Item { b, ctx, whole: false, styles: true });
            seen.insert(b.node);
            let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
            let stable = published.is_some_and(|p| p[0] == r.x && p[2] == r.width);
            let child_chain = changed.is_some_and(|c| chain && chain_through(b, &ctx, published, c));
            stack.extend(b.children.iter().rev().map(|c| (c, child_ctx, stable, child_chain)));
        }
        Self { items, skips }
    }

    /// Whether the plan leaves `node`'s published computed-style entry alone, so the
    /// caller must not evict it before [`Self::collect_computed_styles`].
    pub fn keeps_computed_style(&self, node: u32) -> bool {
        self.skips.own.contains(&node)
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
    ///
    /// An entry the plan proved unchanged is collected anyway when `out` does not
    /// hold it, so a map that was incomplete stays no worse than before.
    pub fn collect_computed_styles(
        &self,
        doc: &lumen_dom::Document,
        viewport: lumen_core::geom::Size,
        out: &mut StyleMaps,
    ) {
        let mut fresh = HashMap::new();
        for it in self.items.iter().filter(|it| it.styles) {
            if !it.whole {
                collect_computed_styles_box(doc, it.b, &it.ctx, viewport, &mut fresh);
                continue;
            }
            let mut stack = vec![(it.b, it.ctx)];
            while let Some((b, ctx)) = stack.pop() {
                let idx = b.node.index() as u32;
                let own = !(self.skips.own.contains(&idx) && out.contains_key(&idx));
                let segments = !(self.skips.runs.contains(&idx)
                    && match &b.kind {
                        BoxKind::InlineRun { segments, .. } => segments
                            .iter()
                            .all(|s| s.source_node.index() == 0 || out.contains_key(&(s.source_node.index() as u32))),
                        _ => true,
                    });
                collect_computed_styles_parts(doc, b, &ctx, viewport, own, segments, &mut fresh);
                let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
                stack.extend(b.children.iter().rev().map(|c| (c, child_ctx)));
            }
        }
        out.extend(fresh);
    }
}

/// Whether the geometry context of `b`'s children is still the one published
/// entries were resolved in, given that `b`'s own is (the caller's `chain`).
///
/// What `apply_used_geometry` reads from the context is the parent's content box
/// (`x`, `width`, resolved padding percentages) and whether it lays out block
/// flow; `x`/`width` of every ancestor staying put and no ancestor changing the
/// style fields of [`context_style_eq`] fixes all three. Height is not in the chain — only a positioned box reads it,
/// and those are never skipped — except for a `container-type: size` box, whose
/// height can restyle its descendants after layout.
fn chain_through(b: &LayoutBox, ctx: &GeomCtx, published: Option<&[f32; 4]>, changed: &ChangedNodes) -> bool {
    if b.origin.role != BoxRole::Element {
        // An anonymous box is its parent's content box — or narrower, and then
        // what it hands its children is not what the chain vouched for.
        return b.rect.x == ctx.flow_cb.x && b.rect.width == ctx.flow_cb.width;
    }
    let Some(p) = published else { return false };
    let r = box_published_rect(b);
    r == b.rect
        && p[0] == r.x
        && p[2] == r.width
        && !changed.context.contains(&b.node)
        && (b.style.container_type != ContainerType::Size || p[3] == r.height)
}

/// Fills `skips` for the subtree rooted at `root`, a whole item whose computed-style
/// entries the plan would otherwise rebuild in full.
#[allow(clippy::too_many_arguments)]
fn plan_style_skips(
    root: &LayoutBox,
    root_ctx: GeomCtx,
    root_chain: bool,
    layout_rects: &HashMap<u32, [f32; 4]>,
    viewport: lumen_core::geom::Size,
    changed: &ChangedNodes,
    seen: &mut HashSet<NodeId>,
    skips: &mut StyleSkips,
) {
    let mut stack = vec![(root, root_ctx, root_chain)];
    while let Some((b, ctx, chain)) = stack.pop() {
        let idx = b.node.index() as u32;
        let published = layout_rects.get(&idx);
        if matches!(b.kind, BoxKind::InlineRun { .. }) && !changed.closure.contains(&b.node) {
            skips.runs.insert(idx);
        }
        // The entry comes from the first box of the node in tree order, and only a
        // principal element box is something `published` describes.
        if seen.insert(b.node) && b.origin.role == BoxRole::Element && chain && entry_unchanged(b, published, changed) {
            skips.own.insert(idx);
        }
        let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
        let child_chain = chain && chain_through(b, &ctx, published, changed);
        stack.extend(b.children.iter().rev().map(|c| (c, child_ctx, child_chain)));
    }
}

/// Whether `b`'s own computed-style entry is what was published, given that the
/// context it is resolved in is: same cascaded style, same size, same `x`, not
/// positioned.
fn entry_unchanged(b: &LayoutBox, published: Option<&[f32; 4]>, changed: &ChangedNodes) -> bool {
    let Some(p) = published else { return false };
    let r = box_published_rect(b);
    r == b.rect
        && p[0] == r.x
        && p[2] == r.width
        && p[3] == r.height
        && matches!(b.style.position, Position::Static | Position::Sticky)
        && !changed.own.contains(&b.node)
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
        let plan = ScopedCollection::plan(&after, &dirty, &clean, &published(&before), VIEWPORT, None);
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
        let plan = ScopedCollection::plan(&after, &dirty, &clean, &published(&before), VIEWPORT, None);
        assert!(planned(&plan, 50.0).is_empty(), "the unmoved clean sibling is skipped");
        assert_eq!(planned(&plan, 20.0), [(true, true)]);
    }

    struct Fixed;
    impl crate::TextMeasurer for Fixed {
        fn char_width(&self, _: char, size: f32) -> f32 {
            size * 0.5
        }
    }

    /// BUG-935 срез 59: a dirty root the cascade re-cascaded as a whole, where only one
    /// descendant ended up with a different style — the others keep their entries.
    #[test]
    fn an_equal_restyle_leaves_the_unchanged_entries_published() {
        use crate::box_tree::{layout_measured_hyp_with_counters, layout_mutation_incremental_restyle};
        use crate::counters::{set_incremental_restyle, ContentDirty, RestyleDelta};
        use lumen_core::ext::NullHyphenationProvider;

        let html = "<body style=\"margin:0\"><div id=\"p\"><div id=\"a\" class=\"k\"></div>                    <div id=\"b\" class=\"k\"></div><div id=\"c\" class=\"k\"></div></div></body>";
        let css = ".k { height: 20px; color: black; } .hot { background-color: red; }";
        let mut doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let hp = NullHyphenationProvider;
        let (prev, prev_counters) = layout_measured_hyp_with_counters(&doc, &sheet, VIEWPORT, &Fixed, &hp, false);
        let published = crate::collect_layout_rects(&prev, &doc);
        let mut styles = crate::collect_computed_styles(&prev, &doc, None, VIEWPORT);

        let (p, a, b, c) = (
            doc.find_by_id("p").unwrap(),
            doc.find_by_id("a").unwrap(),
            doc.find_by_id("b").unwrap(),
            doc.find_by_id("c").unwrap(),
        );
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(b).data {
            for attr in attrs.iter_mut().filter(|a| a.name.local == "class") {
                attr.value = "k hot".to_string();
            }
        }
        // The widening a child-list change gets: the parent is the root, everything below re-cascades.
        let content = HashSet::from([p]);
        let delta = RestyleDelta {
            prev_styles: prev_counters.into_styles(),
            dirty_roots: HashSet::from([p]),
            content_dirty: ContentDirty::Nodes(&content),
        };
        set_incremental_restyle(true);
        let (after, counters) =
            layout_mutation_incremental_restyle(&doc, &sheet, VIEWPORT, &Fixed, &hp, false, prev, delta);
        set_incremental_restyle(false);

        let changed = ChangedNodes::new(&doc, &counters, &content).expect("no shadow root");
        let plan = ScopedCollection::plan(
            &after, &HashSet::from([p]), counters.clean_subtrees(), &published, VIEWPORT, Some(&changed),
        );
        let kept = |n: lumen_dom::NodeId| plan.keeps_computed_style(n.index() as u32);
        assert!(kept(a) && kept(c), "siblings re-cascaded to the styles they had keep their entries");
        assert!(kept(p), "the dirty root's own style, size and position did not change either");
        assert!(!kept(b), "the element whose style changed is rebuilt");

        // Without the change record nothing is left published.
        let plain = ScopedCollection::plan(
            &after, &HashSet::from([p]), counters.clean_subtrees(), &published, VIEWPORT, None,
        );
        assert!(![a, b, c, p].iter().any(|n| plain.keeps_computed_style(n.index() as u32)));

        // What the collector then publishes: the rebuilt entry changes, the kept ones are untouched.
        let before_b = styles[&(b.index() as u32)].clone();
        styles.get_mut(&(a.index() as u32)).unwrap().insert("sentinel".into(), "kept".into());
        plan.collect_computed_styles(&doc, VIEWPORT, &mut styles);
        assert_eq!(styles[&(a.index() as u32)].get("sentinel").map(String::as_str), Some("kept"));
        assert_ne!(styles[&(b.index() as u32)], before_b, "b's entry was rebuilt with its new background");
    }

    #[test]
    fn nothing_is_pruned_without_a_clean_record() {
        let after = laid(50);
        let plan =
            ScopedCollection::plan(&after, &HashSet::new(), &HashSet::new(), &published(&after), VIEWPORT, None);
        let mut boxes = 0;
        let mut stack = vec![&after];
        while let Some(b) = stack.pop() {
            boxes += 1;
            stack.extend(b.children.iter());
        }
        assert_eq!(plan.items.len(), boxes);
    }
}
