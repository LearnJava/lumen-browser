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
use crate::style::{ContainerType, TransformFn};
use crate::{
    box_published_rect, collect_boxed_node_ids, collect_client_rects_box, collect_client_rects_rec,
    collect_computed_styles_parts, collect_layout_rects_box,
    collect_layout_rects_rec, BoxKind, CounterMap, LayoutBox, Position,
};
use lumen_dom::NodeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type StyleMaps = HashMap<u32, crate::StyleMap>;

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
    /// A float is among the nodes that changed (or contains one): its size, or its being
    /// there at all, moves the lines of the siblings after it without moving their boxes.
    floats: bool,
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
        let is_float = |s: &crate::style::ComputedStyle| s.float_side != crate::style::FloatSide::None;
        let floats = own.iter().any(|id| {
            counters.replaced_styles().get(id).is_some_and(|old| is_float(old)) || styles.get(id).is_some_and(|s| is_float(s))
        }) || closure.iter().any(|id| styles.get(id).is_some_and(|s| is_float(s)));
        Some(Self { own, context, closure, floats })
    }

    /// Whether the subtree under `node` is the one the previous flush saw: the cascade gave
    /// nothing in it a different style and no content in it changed.
    fn untouched(&self, node: NodeId) -> bool {
        !self.closure.contains(&node)
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
    reasons: Reasons,
}

/// What one [`ScopedCollection::collect_computed_styles`] call did, for the frame log
/// (BUG-935 срез 61): how much of the planned work was real and how much was skipped.
#[derive(Debug, Default, Clone, Copy)]
pub struct StyleCollectStats {
    /// Items whose own box only (an ancestor of a dirty root).
    pub partial_items: u32,
    /// Items collected whole, and the boxes under them.
    pub whole_items: u32,
    pub whole_boxes: u32,
    /// Boxes of whole items whose own entry was rebuilt / left published.
    pub own_built: u32,
    pub own_kept: u32,
    /// `InlineRun` boxes whose flattened segment entries were rebuilt.
    pub run_built: u32,
    /// Entries actually built (after `or_insert` dedup).
    pub built: u32,
    /// Why the first box of a node in a whole item was not skipped, as counted by
    /// [`plan_style_skips`]: see [`Reasons`].
    pub reasons: Reasons,
}

/// Why a principal element box's entry could not be left published.
#[derive(Debug, Default, Clone, Copy)]
pub struct Reasons {
    /// The geometry chain broke above the box.
    pub chain: u32,
    /// Where the chain broke, counted once per breaking box: see [`ChainBreaks`].
    pub breaks: ChainBreaks,
    /// The box has no published rect (new).
    pub unpublished: u32,
    /// The box publishes a rect that is not its own (transform / ruby base).
    pub transformed: u32,
    /// Published `x` differs.
    pub moved_x: u32,
    /// Published `width` differs.
    pub moved_w: u32,
    /// Published `height` differs.
    pub moved_h: u32,
    /// `position: relative`.
    pub relative: u32,
    /// `position: absolute`/`fixed`.
    pub absolute: u32,
    /// The cascade gave it a different style.
    pub restyled: u32,
    /// The box is not a principal element box (anonymous / pseudo) or a later box of a node.
    pub other: u32,
}

/// Which check of [`chain_through`] failed first at a box whose ancestors all passed it
/// (BUG-935 срез 62): the box that tears the chain, not the ones under it.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChainBreaks {
    /// Anonymous box narrower than / shifted from its parent's content box.
    pub anon: u32,
    /// The element box has no published rect.
    pub unpublished: u32,
    /// The element publishes a rect that is not its own (transform / ruby base).
    pub transformed: u32,
    /// Published `x` differs.
    pub moved_x: u32,
    /// Published `width` differs.
    pub moved_w: u32,
    /// `changed.context`: a style field the children resolve against changed.
    pub context: u32,
    /// `container-type: size` box whose height moved.
    pub container_h: u32,
}

impl ChainBreaks {
    fn count(&mut self, why: Break) {
        let slot = match why {
            Break::Anon => &mut self.anon,
            Break::Unpublished => &mut self.unpublished,
            Break::Transformed => &mut self.transformed,
            Break::MovedX => &mut self.moved_x,
            Break::MovedW => &mut self.moved_w,
            Break::Context => &mut self.context,
            Break::ContainerHeight => &mut self.container_h,
        };
        *slot += 1;
    }
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
    /// BUG-935 срез 70: the node ids (`NodeId::index` / `NodeId::raw`) of the boxes in the
    /// subtrees inside a dirty root that the plan proved unchanged and leaves alone — the ids a
    /// caller evicts for the whole dirty root must not include these.
    pruned_ids: HashSet<u32>,
    pruned_raw_ids: HashSet<u32>,
    /// The boxes of subtrees inside a dirty root that moved but whose computed-style entries did
    /// not ([`translation_keeps_styles`]): their rects are collected, their styles stay.
    style_kept_ids: HashSet<u32>,
}

/// What [`ScopedCollection::plan_with`] may do beyond the plain BUG-1238 walk.
#[derive(Clone, Copy)]
pub struct PlanOptions {
    /// The caller collects computed styles, so the plan works out which entries stay (срез 59).
    pub styles: bool,
    /// Walk a dirty root like any other box and leave its unchanged subtrees alone (срез 70)
    /// instead of collecting it whole. Needs the change record.
    pub prune: bool,
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
        let opts = PlanOptions { styles: true, prune: false };
        Self::plan_with(root, dirty_roots, clean_subtrees, layout_rects, viewport, changed, opts)
    }

    /// [`Self::plan`] with the options of [`PlanOptions`].
    ///
    /// BUG-935 срез 70: with `opts.prune` (and a change record) a dirty root is no longer
    /// collected whole. On `lenta.ru` the root is `body` — a child list changed — and the whole
    /// document under it was walked by four collectors to rebuild one `span`. The root is
    /// handled like any other box: if the cascade and the content record say nothing in
    /// its subtree changed, and the box is where the published map has it, the subtree is
    /// left alone ([`Self::keeps_published`]); a box that changed is collected for its own
    /// entries and the walk goes on to its children; a box that moved, or is new, is collected
    /// whole as it was before.
    #[allow(clippy::too_many_arguments)]
    pub fn plan_with(
        root: &'a LayoutBox,
        dirty_roots: &HashSet<NodeId>,
        clean_subtrees: &HashSet<NodeId>,
        layout_rects: &HashMap<u32, [f32; 4]>,
        viewport: lumen_core::geom::Size,
        changed: Option<&ChangedNodes>,
        opts: PlanOptions,
    ) -> Self {
        let prune_changed = changed.filter(|_| opts.prune);
        let style_changed = changed.filter(|_| opts.styles);
        let mut items = Vec::new();
        let mut pruned = Vec::new();
        let mut translated = Vec::new();
        let mut skips = StyleSkips::default();
        let mut seen = HashSet::new();
        // The third member: the parent's published `x`/`width` are where the map
        // says they are, i.e. its content box — this box's flow containing block —
        // did not move horizontally. The root's parent is the initial containing
        // block, which a viewport change would have sent down the full path.
        // The fourth: the same holds for every ancestor and none of them changed
        // what the context is made of, so the context `apply_used_geometry` resolves this box in is the
        // one its published entry was resolved in ([`chain_through`]).
        // The fifth: the box is inside a dirty root (срез 70).
        let mut stack = vec![(root, GeomCtx::root(viewport), true, true, true, false)];
        while let Some((b, ctx, parent_stable, chain, full, in_root)) = stack.pop() {
            let is_root = dirty_roots.contains(&b.node);
            let in_root = in_root || is_root;
            if is_root && prune_changed.is_none() {
                items.push(Item { b, ctx, whole: true, styles: true });
                if let Some(changed) = style_changed {
                    plan_style_skips(b, ctx, (chain, full), layout_rects, viewport, changed, &mut seen, &mut skips);
                }
                continue;
            }
            let r = box_published_rect(b);
            let published = layout_rects.get(&(b.node.index() as u32));
            // Inside a dirty root a subtree is left alone only when the box builder carried it over
            // from the previous tree (`clean_subtrees`) *and* the change record agrees nothing in it
            // changed. The record alone is not enough: a subtree the builder laid out again can come
            // out different with no DOM change inside it (an image whose size just became known), and
            // its entries must be rebuilt. Only the principal box of an element can say "unchanged";
            // an anonymous box carries its owner's node but not the rect published for it.
            let untouched = match prune_changed {
                Some(changed) if in_root => {
                    b.origin.role == BoxRole::Element && clean_subtrees.contains(&b.node) && changed.untouched(b.node)
                }
                _ => clean_subtrees.contains(&b.node),
            };
            if untouched {
                let unmoved = published == Some(&[r.x, r.y, r.width, r.height]);
                let safe = !in_root
                    || prune_changed.is_some_and(|c| {
                        // Nothing outside the subtree that its entries are made of moved: the
                        // ancestors are where they were (`chain`), no float changed that could
                        // re-wrap its lines, and a positioned box in it resolves against
                        // ancestors whose whole border box is where it was (`full`).
                        chain && !c.floats && (full || !has_positioned_box(b))
                    });
                if unmoved && safe {
                    if in_root {
                        pruned.push(b);
                    }
                } else if unmoved {
                    items.push(Item { b, ctx, whole: true, styles: true });
                    if let Some(changed) = style_changed {
                        plan_style_skips(b, ctx, (chain, full), layout_rects, viewport, changed, &mut seen, &mut skips);
                    }
                } else {
                    let styles = !translation_keeps_styles(b, r, published, parent_stable);
                    items.push(Item { b, ctx, whole: true, styles });
                    if !styles && in_root {
                        translated.push(b);
                    }
                    if styles && let Some(changed) = style_changed {
                        plan_style_skips(b, ctx, (chain, full), layout_rects, viewport, changed, &mut seen, &mut skips);
                    }
                }
                continue;
            }
            items.push(Item { b, ctx, whole: false, styles: true });
            let first = seen.insert(b.node);
            let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
            let stable = published.is_some_and(|p| p[0] == r.x && p[2] == r.width);
            let child_chain = match changed {
                Some(c) if chain && !b.children.is_empty() => match chain_through(b, &ctx, published, c) {
                    Ok(()) => true,
                    Err(why) => {
                        skips.reasons.breaks.count(why);
                        false
                    }
                },
                _ => false,
            };
            let child_full = changed.is_some_and(|c| full && full_through(b, &ctx, published, c));
            // BUG-935 срез 70: a box on the spine whose style, size and position are what they were
            // keeps its computed-style entry too. Under a dirty root that is most of the spine — the
            // root and its re-cascaded children — and each rebuilt entry costs ~170 µs because the
            // re-cascade handed it a fresh `Arc` the serialisation memo has not seen.
            if let Some(c) = style_changed
                && first
                && chain
                && b.origin.role == BoxRole::Element
                && entry_unchanged(b, published, c, full).is_ok()
            {
                skips.own.insert(b.node.index() as u32);
            }
            stack.extend(b.children.iter().rev().map(|c| (c, child_ctx, stable, child_chain, child_full, in_root)));
        }
        let mut pruned_ids = HashSet::new();
        let mut pruned_raw_ids = HashSet::new();
        for &top in &pruned {
            let mut walk = vec![top];
            while let Some(b) = walk.pop() {
                pruned_ids.insert(b.node.index() as u32);
                pruned_raw_ids.insert(b.node.raw());
                walk.extend(b.children.iter());
            }
        }
        let mut style_kept_ids = HashSet::new();
        for &top in &translated {
            let mut walk = vec![top];
            while let Some(b) = walk.pop() {
                style_kept_ids.insert(b.node.index() as u32);
                walk.extend(b.children.iter());
            }
        }
        Self { items, skips, pruned_ids, pruned_raw_ids, style_kept_ids }
    }

    /// Whether the plan leaves `node`'s published computed-style entry alone, so the
    /// caller must not evict it before [`Self::collect_computed_styles`].
    pub fn keeps_computed_style(&self, node: u32) -> bool {
        self.skips.own.contains(&node) || self.pruned_ids.contains(&node) || self.style_kept_ids.contains(&node)
    }

    /// How many boxes lie in the subtrees the plan left alone, and how many planned items there are.
    pub fn census(&self) -> (usize, usize) {
        (self.pruned_ids.len(), self.items.len())
    }

    /// The ids ([`Self::keeps_published`]) and raw ids ([`Self::keeps_scroll_state`]) of the
    /// boxes in the subtrees left alone — for the `LUMEN_VERIFY_SCOPE_PRUNE` self-check.
    pub fn pruned_node_ids(&self) -> (&HashSet<u32>, &HashSet<u32>) {
        (&self.pruned_ids, &self.pruned_raw_ids)
    }

    /// Whether `node` (a `NodeId::index`) owns a box in a subtree the plan left alone, so its
    /// rect entries are still right and must not be evicted with the rest of the dirty root.
    pub fn keeps_published(&self, node: u32) -> bool {
        self.pruned_ids.contains(&node)
    }

    /// [`Self::keeps_published`] for the scroll-state cache, which is keyed by `NodeId::raw`.
    pub fn keeps_scroll_state(&self, raw: u32) -> bool {
        self.pruned_raw_ids.contains(&raw)
    }

    /// The boxes whose scroll containers the scroll-state cache has to re-read: a collected
    /// whole item brings its subtree, a spine box only itself.
    pub fn scroll_containers(&self) -> Vec<crate::ScrollContainer> {
        let mut out = Vec::new();
        for it in &self.items {
            if it.whole {
                crate::collect_scroll_containers_for_js_state_scoped(&[it.b]).into_iter().for_each(|c| out.push(c));
            } else {
                out.extend(crate::scroll_container_of(it.b));
            }
        }
        out
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
    ) -> StyleCollectStats {
        let mut stats = StyleCollectStats::default();
        let mut fresh = HashMap::new();
        for it in self.items.iter().filter(|it| it.styles) {
            if !it.whole {
                stats.partial_items += 1;
                let idx = it.b.node.index() as u32;
                let own = !(self.skips.own.contains(&idx) && out.contains_key(&idx));
                collect_computed_styles_parts(doc, it.b, &it.ctx, viewport, own, true, &mut fresh);
                continue;
            }
            stats.whole_items += 1;
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
                stats.whole_boxes += 1;
                if own {
                    stats.own_built += 1;
                } else {
                    stats.own_kept += 1;
                }
                if !segments {
                    // A run whose subtree is untouched: counted with the kept boxes.
                } else if matches!(b.kind, BoxKind::InlineRun { .. }) {
                    stats.run_built += 1;
                }
                collect_computed_styles_parts(doc, b, &ctx, viewport, own, segments, &mut fresh);
                let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
                stack.extend(b.children.iter().rev().map(|c| (c, child_ctx)));
            }
        }
        let fresh_len = fresh.len() as u32;
        out.extend(fresh);
        stats.built = fresh_len;
        stats.reasons = self.skips.reasons;
        stats
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
fn chain_through(
    b: &LayoutBox,
    ctx: &GeomCtx,
    published: Option<&[f32; 4]>,
    changed: &ChangedNodes,
) -> Result<(), Break> {
    if b.origin.role != BoxRole::Element {
        // An anonymous box is its parent's content box — or narrower, and then
        // what it hands its children is not what the chain vouched for.
        return if b.rect.x == ctx.flow_cb.x && b.rect.width == ctx.flow_cb.width { Ok(()) } else { Err(Break::Anon) };
    }
    let Some(p) = published else { return Err(Break::Unpublished) };
    let r = box_published_rect(b);
    if r != b.rect && !translated_in_place(b, r, p, changed) {
        return Err(Break::Transformed);
    }
    if p[0] != r.x {
        return Err(Break::MovedX);
    }
    if p[2] != r.width {
        return Err(Break::MovedW);
    }
    if changed.context.contains(&b.node) {
        return Err(Break::Context);
    }
    if b.style.container_type == ContainerType::Size && p[3] != r.height {
        return Err(Break::ContainerHeight);
    }
    Ok(())
}

/// The failed check of [`chain_through`], for [`ChainBreaks`].
#[derive(Clone, Copy)]
enum Break {
    Anon,
    Unpublished,
    Transformed,
    MovedX,
    MovedW,
    Context,
    ContainerHeight,
}

/// [`chain_through`] for a *positioned* box: its entry is read against the vertical
/// extent of its containing blocks too (`top: 10%`, `bottom` of an absolute box,
/// the auto offsets recovered from where layout put it), and against their
/// position. So the whole border box of every ancestor — not just `x`/`width` —
/// must be where the published map says, and none may have changed what the
/// context is made of.
///
/// An anonymous box has no entry of its own to compare with; it passes only when
/// it is exactly its parent's content box.
fn full_through(b: &LayoutBox, ctx: &GeomCtx, published: Option<&[f32; 4]>, changed: &ChangedNodes) -> bool {
    if b.origin.role != BoxRole::Element {
        return b.rect == ctx.flow_cb;
    }
    let Some(p) = published else { return false };
    let r = box_published_rect(b);
    (r == b.rect && *p == [r.x, r.y, r.width, r.height] || translated_in_place(b, r, p, changed))
        && !changed.context.contains(&b.node)
}

/// Whether `b`, which publishes the rect `r` of its own `transform` instead of its
/// border box, still has the border box it had when `published` was taken.
///
/// A box whose whole transform is a translation (`translateX(-310px)` on an
/// off-canvas menu) publishes its border box shifted by a constant the style
/// fixes, so with the style untouched (`changed.own`) `published == r` leaves
/// exactly one border box that could have been there. Anything with a rotation
/// or a scale publishes a bounding box that several border boxes share, and is
/// not vouched for. The chain would otherwise break at such a box and every
/// entry below it be rebuilt.
fn translated_in_place(b: &LayoutBox, r: lumen_core::geom::Rect, published: &[f32; 4], changed: &ChangedNodes) -> bool {
    *published == [r.x, r.y, r.width, r.height]
        && !changed.own.contains(&b.node)
        && !matches!(b.kind, BoxKind::Ruby { .. })
        && b.style.rotate.is_none()
        && b.style.scale.is_none()
        && b.style.offset_path.is_none()
        && b.style
            .transform
            .iter()
            .all(|f| matches!(f, TransformFn::Translate(..) | TransformFn::TranslateX(_) | TransformFn::TranslateY(_)))
}

/// Fills `skips` for the subtree rooted at `root`, a whole item whose computed-style
/// entries the plan would otherwise rebuild in full.
#[allow(clippy::too_many_arguments)]
fn plan_style_skips(
    root: &LayoutBox,
    root_ctx: GeomCtx,
    root_chain: (bool, bool),
    layout_rects: &HashMap<u32, [f32; 4]>,
    viewport: lumen_core::geom::Size,
    changed: &ChangedNodes,
    seen: &mut HashSet<NodeId>,
    skips: &mut StyleSkips,
) {
    let mut stack = vec![(root, root_ctx, root_chain.0, root_chain.1)];
    while let Some((b, ctx, chain, full)) = stack.pop() {
        let idx = b.node.index() as u32;
        let published = layout_rects.get(&idx);
        if matches!(b.kind, BoxKind::InlineRun { .. }) && !changed.closure.contains(&b.node) {
            skips.runs.insert(idx);
        }
        // The entry comes from the first box of the node in tree order, and only a
        // principal element box is something `published` describes.
        if seen.insert(b.node) && b.origin.role == BoxRole::Element {
            if !chain {
                skips.reasons.chain += 1;
            } else {
                match entry_unchanged(b, published, changed, full) {
                    Ok(()) => {
                        skips.own.insert(idx);
                    }
                    Err(Why::Unpublished) => skips.reasons.unpublished += 1,
                    Err(Why::Transformed) => skips.reasons.transformed += 1,
                    Err(Why::Geometry { x, w, h }) => {
                        skips.reasons.moved_x += u32::from(x);
                        skips.reasons.moved_w += u32::from(w);
                        skips.reasons.moved_h += u32::from(h);
                    }
                    Err(Why::Positioned) => {
                        if b.style.position == Position::Relative {
                            skips.reasons.relative += 1;
                        } else {
                            skips.reasons.absolute += 1;
                        }
                    }
                    Err(Why::Restyled) => skips.reasons.restyled += 1,
                }
            }
        } else {
            skips.reasons.other += 1;
        }
        let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
        // A leaf hands nothing to anyone: its break is not one.
        let child_chain = chain
            && !b.children.is_empty()
            && match chain_through(b, &ctx, published, changed) {
                Ok(()) => true,
                Err(why) => {
                    skips.reasons.breaks.count(why);
                    false
                }
            };
        let child_full = full && full_through(b, &ctx, published, changed);
        stack.extend(b.children.iter().rev().map(|c| (c, child_ctx, child_chain, child_full)));
    }
}

/// Whether `b`'s own computed-style entry is what was published, given that the
/// context it is resolved in is: same cascaded style, same size, same `x`, not
/// positioned.
fn entry_unchanged(
    b: &LayoutBox,
    published: Option<&[f32; 4]>,
    changed: &ChangedNodes,
    full: bool,
) -> Result<(), Why> {
    let Some(p) = published else { return Err(Why::Unpublished) };
    let r = box_published_rect(b);
    if r != b.rect {
        return Err(Why::Transformed);
    }
    if !(p[0] == r.x && p[2] == r.width && p[3] == r.height) {
        return Err(Why::Geometry { x: p[0] != r.x, w: p[2] != r.width, h: p[3] != r.height });
    }
    if !matches!(b.style.position, Position::Static | Position::Sticky) && !(full && p[1] == r.y) {
        return Err(Why::Positioned);
    }
    if changed.own.contains(&b.node) {
        return Err(Why::Restyled);
    }
    Ok(())
}

enum Why {
    Unpublished,
    Transformed,
    Geometry { x: bool, w: bool, h: bool },
    Positioned,
    Restyled,
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
            shallow_roots: Default::default(), point_roots: Default::default(),
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
        styles.get_mut(&(a.index() as u32)).unwrap().insert("sentinel", "kept".into());
        plan.collect_computed_styles(&doc, VIEWPORT, &mut styles);
        assert_eq!(styles[&(a.index() as u32)].get("sentinel").map(String::as_str), Some("kept"));
        assert_ne!(styles[&(b.index() as u32)], before_b, "b's entry was rebuilt with its new background");
    }

    /// Lays `html` out, applies `mutate` to the document, re-cascades `dirty` (an id)
    /// as a whole and returns which of `probe` (ids) keep their computed-style entry.
    fn kept_after(html: &str, css: &str, dirty: &str, mutate: impl FnOnce(&mut lumen_dom::Document), probe: &[&str]) -> Vec<bool> {
        use crate::box_tree::{layout_measured_hyp_with_counters, layout_mutation_incremental_restyle};
        use crate::counters::{set_incremental_restyle, ContentDirty, RestyleDelta};
        use lumen_core::ext::NullHyphenationProvider;

        let mut doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let hp = NullHyphenationProvider;
        let (prev, prev_counters) = layout_measured_hyp_with_counters(&doc, &sheet, VIEWPORT, &Fixed, &hp, false);
        let published = crate::collect_layout_rects(&prev, &doc);
        mutate(&mut doc);
        let root = doc.find_by_id(dirty).unwrap();
        let content = HashSet::from([root]);
        let delta = RestyleDelta {
            prev_styles: prev_counters.into_styles(),
            dirty_roots: HashSet::from([root]),
            content_dirty: ContentDirty::Nodes(&content),
            shallow_roots: Default::default(), point_roots: Default::default(),
        };
        set_incremental_restyle(true);
        let (after, counters) =
            layout_mutation_incremental_restyle(&doc, &sheet, VIEWPORT, &Fixed, &hp, false, prev, delta);
        set_incremental_restyle(false);
        let changed = ChangedNodes::new(&doc, &counters, &content).expect("no shadow root");
        let plan = ScopedCollection::plan(
            &after, &HashSet::from([root]), counters.clean_subtrees(), &published, VIEWPORT, Some(&changed),
        );
        probe.iter().map(|id| plan.keeps_computed_style(doc.find_by_id(id).unwrap().index() as u32)).collect()
    }

    /// BUG-935 срез 61: a positioned box's entry reads the extent and position of its
    /// containing blocks, so it is left published exactly while every ancestor is where it was.
    #[test]
    fn a_positioned_entry_is_kept_while_every_ancestor_is_in_place() {
        let html = "<body style=\"margin:0\"><div id=\"p\"><div id=\"cb\" class=\"cb\">                    <div id=\"ab\" class=\"ab\"></div><div id=\"rel\" class=\"rel\"></div></div>                    <div id=\"other\" class=\"k\"></div></div></body>";
        let css = ".cb { position: relative; height: 80px; } .ab { position: absolute; right: 5px; bottom: 10%; width: 9px; height: 9px; }
                   .rel { position: relative; top: 10%; height: 5px; } .k { height: 20px; } .hot { background-color: red; }
                   .big { height: 140px; }";
        let hot = |doc: &mut lumen_dom::Document| {
            let other = doc.find_by_id("other").unwrap();
            if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(other).data {
                for attr in attrs.iter_mut().filter(|a| a.name.local == "class") {
                    attr.value = "k hot".to_string();
                }
            }
        };
        // Something elsewhere changes: nothing around the positioned boxes moved.
        assert_eq!(kept_after(html, css, "p", hot, &["ab", "rel", "cb"]), [true, true, true]);
        // The containing block itself is resized: both insets resolve against a new height.
        let grow = |doc: &mut lumen_dom::Document| {
            let cb = doc.find_by_id("cb").unwrap();
            if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
                for attr in attrs.iter_mut().filter(|a| a.name.local == "class") {
                    attr.value = "cb big".to_string();
                }
            }
        };
        assert_eq!(kept_after(html, css, "p", grow, &["ab", "rel"]), [false, false]);
    }

    /// BUG-935 срез 62: a box whose transform is a translation still vouches for the
    /// entries below it; a rotation or a scale does not.
    #[test]
    fn a_translated_box_does_not_break_the_chain() {
        let html = "<body style=\"margin:0\"><div id=\"p\"><div id=\"t\" class=\"tr\"><div id=\"kid\" class=\"mid\"></div></div>                    <div id=\"r\" class=\"rot\"><div id=\"rkid\" class=\"mid\"></div></div><div id=\"other\" class=\"k\"></div></div></body>";
        let css = ".tr { transform: translateX(-30px); } .rot { transform: rotate(5deg); } .k { height: 20px; }
                   .mid { width: 50px; height: 10px; margin: 0 auto; } .hot { background-color: red; } .wide { width: 300px; }";
        let hot = |doc: &mut lumen_dom::Document| {
            let other = doc.find_by_id("other").unwrap();
            if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(other).data {
                for attr in attrs.iter_mut().filter(|a| a.name.local == "class") {
                    attr.value = "k hot".to_string();
                }
            }
        };
        assert_eq!(kept_after(html, css, "p", hot, &["kid", "rkid"]), [true, false]);
        // The translated box itself is resized: what its child resolves in changed.
        let widen = |doc: &mut lumen_dom::Document| {
            let t = doc.find_by_id("t").unwrap();
            if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(t).data {
                for attr in attrs.iter_mut().filter(|a| a.name.local == "class") {
                    attr.value = "tr wide".to_string();
                }
            }
        };
        assert_eq!(kept_after(html, css, "p", widen, &["kid"]), [false]);
    }

    /// BUG-935 срез 70: the `lenta.ru` font-probe loop in miniature — a child appended to a
    /// shallow root whose existing children are carried over untouched.
    fn append_to_shallow_root() -> (Vec<(&'static str, bool, bool)>, usize, usize) {
        use crate::box_tree::{layout_measured_hyp_with_counters, layout_mutation_incremental_restyle};
        use crate::counters::{set_incremental_restyle, ContentDirty, RestyleDelta};
        use lumen_core::ext::NullHyphenationProvider;

        let html = "<body style=\"margin:0\"><div id=\"p\"><div id=\"a\" class=\"k\"><div id=\"a1\" class=\"k\"></div></div>                    <div id=\"b\" class=\"k\"><div id=\"b1\" class=\"k\"></div></div></div></body>";
        let css = ".k { height: 20px; color: black; }";
        let mut doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let hp = NullHyphenationProvider;
        let (prev, prev_counters) = layout_measured_hyp_with_counters(&doc, &sheet, VIEWPORT, &Fixed, &hp, false);
        let published = crate::collect_layout_rects(&prev, &doc);
        let p = doc.find_by_id("p").unwrap();
        let n = doc.create_element(lumen_dom::QualName::html("span"));
        doc.append_child(p, n);
        let content = HashSet::from([p, n]);
        let delta = RestyleDelta {
            prev_styles: prev_counters.into_styles(),
            dirty_roots: Default::default(),
            content_dirty: ContentDirty::Nodes(&content),
            shallow_roots: HashSet::from([p]),
            point_roots: Default::default(),
        };
        set_incremental_restyle(true);
        let (after, counters) =
            layout_mutation_incremental_restyle(&doc, &sheet, VIEWPORT, &Fixed, &hp, false, prev, delta);
        set_incremental_restyle(false);
        let changed = ChangedNodes::new(&doc, &counters, &content).expect("no shadow root");
        let opts = PlanOptions { styles: true, prune: true };
        let plan = ScopedCollection::plan_with(
            &after, &HashSet::from([p]), counters.clean_subtrees(), &published, VIEWPORT, Some(&changed), opts,
        );
        let ids = ["p", "a", "a1", "b", "b1"];
        let rows = ids
            .iter()
            .map(|id| {
                let n = doc.find_by_id(id).unwrap().index() as u32;
                (*id, plan.keeps_published(n), plan.keeps_computed_style(n))
            })
            .collect();
        let whole_items = plan.items.iter().filter(|it| it.whole).count();
        (rows, plan.items.len(), whole_items)
    }

    #[test]
    fn a_shallow_root_keeps_the_children_it_carried_over() {
        let (rows, items, whole) = append_to_shallow_root();
        let get = |id: &str| rows.iter().find(|r| r.0 == id).copied().unwrap();
        // The root and its children are re-cascaded, so each is a spine item collected for its own
        // rect; what is under them was carried over and is left alone, rect maps and styles alike.
        assert!(get("a1").1 && get("b1").1, "carried-over grandchildren are left alone: {rows:?}");
        assert!(!get("p").1 && !get("a").1 && !get("b").1, "the spine is collected for its rects: {rows:?}");
        // The spine's computed-style entries stay too: nothing in them changed.
        assert!(get("p").2 && get("a").2 && get("b").2 && get("a1").2 && get("b1").2, "styles kept: {rows:?}");
        // The new span is in the change record, so it is a spine item too; nothing needs a whole walk.
        assert_eq!(whole, 0, "no subtree is collected whole ({items} items)");
    }

    #[test]
    fn nothing_is_left_alone_with_the_pruning_off() {
        use crate::box_tree::layout_measured_hyp_with_counters;
        use lumen_core::ext::NullHyphenationProvider;
        let doc = lumen_html_parser::parse("<body><div id=\"p\"><div id=\"a\"></div></div></body>");
        let sheet = lumen_css_parser::parse("");
        let (laid, counters) =
            layout_measured_hyp_with_counters(&doc, &sheet, VIEWPORT, &Fixed, &NullHyphenationProvider, false);
        let p = doc.find_by_id("p").unwrap();
        let published = crate::collect_layout_rects(&laid, &doc);
        let plan = ScopedCollection::plan(&laid, &HashSet::from([p]), counters.clean_subtrees(), &published, VIEWPORT, None);
        assert_eq!(plan.census().0, 0);
        assert!(!plan.keeps_published(doc.find_by_id("a").unwrap().index() as u32));
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
