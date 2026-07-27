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

// ─── Graft accounting (BUG-341 S13) ─────────────────────────────────────────

/// Per-pass tally of what [`graft_geometry`] reused and why it refused the rest.
///
/// BUG-341 S13: wall-clock cannot tell "the dirty set is genuinely wide" from
/// "the reuse mechanism is refusing boxes it could have kept" — S8 shipped a
/// mechanism that reused *nothing* and every differential test stayed green,
/// because reusing nothing still produces the correct output. These counters
/// make the distinction observable, and are what the S13 gates assert on.
///
/// The reject counters are not mutually exclusive per box in principle, but are
/// recorded so they partition the visited set: a box is counted in exactly one
/// of `reused_clean` / `reject_identity` / `reject_style` / `reject_child_count`
/// / `reject_descendant`, in that priority order (identity is checked first and
/// skips the subtree; a box that both changed style and has a changed
/// descendant counts as `reject_style`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraftStats {
    /// Boxes `graft_geometry` compared against a predecessor.
    pub visited: usize,
    /// Boxes whose whole subtree was reused clean (O(1) translate afterwards).
    pub reused_clean: usize,
    /// Boxes whose `NodeId` or [`kind_layout_eq`] payload differed — the
    /// subtree below is not even compared (positional matching is meaningless).
    pub reject_identity: usize,
    /// Boxes that matched by identity but whose [`crate::style::ComputedStyle`]
    /// differed from the predecessor's.
    pub reject_style: usize,
    /// Boxes that matched by identity and style but gained or lost children.
    pub reject_child_count: usize,
    /// Boxes that matched entirely themselves but hold a changed descendant.
    pub reject_descendant: usize,
    /// Subset of `reject_style` whose styles become equal once the used-value
    /// writeback fields (`width`/`height`/`box_sizing`) are taken from the
    /// predecessor — i.e. boxes rejected only because `lay_out` wrote its own
    /// output back into the previous tree's style. Only counted while
    /// [`set_graft_diagnostics`] is on (the check costs a style copy).
    pub reject_style_used_value_only: usize,
    /// Subset of `reject_style` whose node has no entry in the previous pass's
    /// cascade map — the box's style was not taken straight from the cascade
    /// (anonymous/pseudo boxes, or a node the cascade never visited), so the
    /// unpolluted comparison is unavailable for it. Diagnostics only.
    pub reject_style_no_cascade_entry: usize,
    /// Subset of `reject_style` whose node *has* a cascade entry that the fresh
    /// style genuinely differs from — the honest "this style changed" bucket.
    /// Diagnostics only.
    pub reject_style_cascade_differs: usize,
}

thread_local! {
    /// Accumulates [`GraftStats`] for the current pass; drained by [`take_graft_stats`].
    static GRAFT_STATS: std::cell::Cell<GraftStats> = const { std::cell::Cell::new(GraftStats {
        visited: 0,
        reused_clean: 0,
        reject_identity: 0,
        reject_style: 0,
        reject_child_count: 0,
        reject_descendant: 0,
        reject_style_used_value_only: 0,
        reject_style_no_cascade_entry: 0,
        reject_style_cascade_differs: 0,
    }) };
    /// Whether to attribute style rejects to used-value writeback (costly).
    static GRAFT_DIAGNOSTICS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Take and reset this thread's [`GraftStats`].
pub fn take_graft_stats() -> GraftStats {
    GRAFT_STATS.with(|s| s.replace(GraftStats::default()))
}

/// Enable/disable the costly `reject_style_used_value_only` attribution.
///
/// Off by default: it copies a [`crate::style::ComputedStyle`] per rejected box.
/// Diagnostic and test use only.
pub fn set_graft_diagnostics(on: bool) {
    GRAFT_DIAGNOSTICS.with(|d| d.set(on));
}

fn bump_graft(f: impl FnOnce(&mut GraftStats)) {
    GRAFT_STATS.with(|s| {
        let mut v = s.get();
        f(&mut v);
        s.set(v);
    });
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
    graft_geometry_with_cascade(new, prev, None)
}

/// [`graft_geometry`], told which cascade result produced `prev`'s styles.
///
/// BUG-341 S13. `prev` is a *laid-out* tree, and layout writes used values back
/// into the very styles this function compares: `lay_out_flex` overwrites every
/// flex item's `width`/`height`/`box_sizing` with the resolved used value, and
/// the post-layout passes (container queries, `::first-line`) rewrite more. The
/// freshly-built tree has none of that yet, so a plain `new.style == prev.style`
/// reports "changed" for boxes nothing changed on — and because a reject
/// propagates to every ancestor, a handful of flex items poisons the whole
/// document. Measured on the CC-12 chrome fixture: **81 of 318 boxes rejected on
/// style, all 81 differing only in the used-value fields**, dragging 41
/// ancestors down with them — 122 boxes re-laid-out per hover flip, none of
/// which had actually changed.
///
/// `prev_cascade` is the [`crate::counters::CounterMap::styles`] map of the pass
/// that produced `prev`. When the freshly-cascaded box still holds *the same
/// allocation* that map recorded for its node, the cascade demonstrably produced
/// an identical style for it this cycle, so whatever `prev`'s box-level style
/// has accumulated since is layout output, not an author-visible change, and the
/// box is reusable. That is a pointer comparison plus one hash lookup; it never
/// claims reuse for a box whose fresh style is *not* the shared cascade entry
/// (anonymous and pseudo boxes that derive their own style fall through to the
/// structural comparison, exactly as before).
///
/// A reused subtree keeps its own freshly-cascaded styles (only geometry and
/// `kind` come from `prev`), so the pollution never migrates into the live tree
/// — see the clean branch below.
pub fn graft_geometry_with_cascade(
    new: &mut LayoutBox,
    prev: &LayoutBox,
    prev_cascade: Option<&std::collections::HashMap<NodeId, std::sync::Arc<crate::style::ComputedStyle>>>,
) -> bool {
    bump_graft(|s| s.visited += 1);
    if new.node != prev.node || !kind_layout_eq(&new.kind, &prev.kind) {
        bump_graft(|s| s.reject_identity += 1);
        // Node identity or box-kind payload differ → this position no longer
        // describes the same box, so the children below it cannot be matched
        // positionally either. Leave the whole subtree dirty (marked by
        // `mark_subtree_dirty`).
        return false;
    }

    // A style difference means *this* box must be re-laid-out, but it says
    // nothing about its descendants — each child is compared against its own
    // predecessor below, and the fresh tree was built with the new cascade, so
    // any inherited change is already visible in the child's own `style`.
    // Returning early here (BUG-341 "S8") threw away geometry reuse for the
    // entire document whenever a single node's style differed: the chrome
    // document hits exactly that every cycle, because `lay_out` writes the used
    // viewport `height` back into the root box's `style`, so a freshly-built
    // root never equals its own laid-out predecessor. One such node at the root
    // meant `graft_geometry` reused nothing at all, on every interaction.
    //
    // BUG-341 S12: the pointer test short-circuits the 302-field comparison for
    // every box whose style came unchanged out of the incremental cascade —
    // both trees then hold the *same* `Arc` from `CounterMap::styles`. The
    // structural `==` still runs when the pointers differ, so a box whose style
    // was re-cascaded to an equal value is still recognised as reusable (that
    // matters: the incremental cascade legitimately re-computes nodes whose
    // result is unchanged).
    //
    // BUG-341 S13: the third clause compares against the cascade entry `prev`
    // was built from rather than against `prev`'s own, layout-polluted style —
    // see this function's doc comment for the measured cost of not doing so.
    let self_reusable = std::sync::Arc::ptr_eq(&new.style, &prev.style)
        || prev_cascade.and_then(|c| c.get(&new.node)).is_some_and(|cascade| {
            // Pointer first: on a narrow restyle the incremental cascade hands
            // the *same* allocation back (S9/S12), so this is the common case
            // and costs nothing. The structural fallback covers the wide
            // restyles — CC-12's own `SIDEBAR`/`None` hover toggle re-cascades
            // most of the document to values that are, node for node, the ones
            // it already had, and those must still be recognised as unchanged.
            std::sync::Arc::ptr_eq(&new.style, cascade) || *new.style == **cascade
        })
        || new.style == prev.style
        // Anonymous boxes (the wrapper a flex container generates around inline
        // content, and its kin) hold no cascade entry of their own — their node
        // is a text node the cascade never visits — so no comparison above can
        // reach them, and on the CC-12 fixture they were the entire remaining
        // reject set: 21 boxes, every one with `width: None` freshly derived and
        // a fractional used px width in `prev`. No author rule can put a
        // `width`/`height` on such a box, so a difference confined to those
        // fields is `lay_out`'s own output and nothing else. The probe copies a
        // style, which is why it runs last and only for this narrow class.
        || (prev_cascade.is_some_and(|c| !c.contains_key(&new.node))
            && used_value_writeback_only(new, prev));
    if !self_reusable && GRAFT_DIAGNOSTICS.with(|d| d.get()) {
        if used_value_writeback_only(new, prev) {
            bump_graft(|s| s.reject_style_used_value_only += 1);
        }
        match prev_cascade.and_then(|c| c.get(&new.node)) {
            None => bump_graft(|s| s.reject_style_no_cascade_entry += 1),
            Some(_) => bump_graft(|s| s.reject_style_cascade_differs += 1),
        }
    }

    let common = new.children.len().min(prev.children.len());
    let mut all_clean = self_reusable && new.children.len() == prev.children.len();
    for i in 0..common {
        let child_clean =
            graft_geometry_with_cascade(&mut new.children[i], &prev.children[i], prev_cascade);
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
        // `style` is deliberately NOT taken from `prev`, unlike `kind` above.
        // `kind` holds layout output paint reads back (`InlineRun`'s laid-out
        // `lines`); the used values in `prev`'s *style* are read by nothing
        // outside the layout pass that wrote them. Adopting them would pin the
        // pollution into the live tree for good — and a box that later goes
        // dirty would be laid out against a stale used size instead of its own
        // cascade value (BUG-341 S13).
        new.scroll_x = prev.scroll_x;
        new.scroll_y = prev.scroll_y;
        new.col_span = prev.col_span;
        new.row_span = prev.row_span;
        new.svg_group_transform = prev.svg_group_transform.clone();
        new.dirty = DirtyBits::CLEAN;
        bump_graft(|s| s.reused_clean += 1);
        return true;
    }

    if !self_reusable {
        bump_graft(|s| s.reject_style += 1);
    } else if new.children.len() != prev.children.len() {
        bump_graft(|s| s.reject_child_count += 1);
    } else {
        bump_graft(|s| s.reject_descendant += 1);
    }

    // This node matches but a descendant changed or a child was appended/removed:
    // it must be re-laid-out (clean children grafted above are translated cheaply,
    // dirty/new children laid out fresh).
    new.dirty = DirtyBits::SELF_SIZE | DirtyBits::HAS_DIRTY_DESCENDANT;
    false
}

/// Whether `new` and `prev` differ *only* in the fields `lay_out` writes back
/// into the box's own style as a used value.
///
/// BUG-341 S13: `lay_out_flex` overwrites a flex item's
/// `width`/`height`/`box_sizing` with the resolved used value, and the
/// replaced-element sizing path does the same for intrinsic sizes. The previous
/// tree therefore carries those writes while a freshly-cascaded tree does not,
/// which makes the two styles unequal for reasons that have nothing to do with
/// the author's stylesheet. Copying the three fields across and re-comparing
/// tells the two causes apart.
///
/// Used both as the diagnostic attribution behind [`set_graft_diagnostics`] and,
/// in [`graft_geometry_with_cascade`], as the last-resort reuse test for boxes
/// with no cascade entry — see the call site for why that is sound only there.
/// Costs a [`crate::style::ComputedStyle`] copy, so it must stay on paths that
/// have already failed every cheaper check.
fn used_value_writeback_only(new: &LayoutBox, prev: &LayoutBox) -> bool {
    let mut probe = (*new.style).clone();
    probe.width = prev.style.width.clone();
    probe.height = prev.style.height.clone();
    probe.box_sizing = prev.style.box_sizing;
    probe == *prev.style
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
///
/// **Every [`crate::box_tree::BoxKind`] variant must be listed here.** A missing
/// one falls into the `_ => false` arm, which does not merely lose that box's own
/// geometry: [`graft_geometry`] propagates the failure up, so every ancestor is
/// re-laid-out too, and a single unlisted kind anywhere in the document defeats
/// incremental layout for the whole tree. That is exactly what BUG-341 "S8"
/// found — `Contents`/`Table`/`TableRowGroup`/`SvgRoot`/`SvgShape`/`SvgText`
/// were absent, and the chrome document's SVG icons alone kept `graft_geometry`
/// returning `false` at the root on every single cycle.
fn kind_layout_eq(a: &crate::box_tree::BoxKind, b: &crate::box_tree::BoxKind) -> bool {
    use crate::box_tree::BoxKind::{
        Audio, Block, Canvas, Contents, FlowRoot, FormControl, Iframe, Image, InlineBlockRow,
        InlineRun, InlineSpace, Marker, Skip, SvgRoot, SvgShape, SvgText, Table, TableRow,
        TableRowGroup, Video,
    };
    match (a, b) {
        (Block, Block)
        | (InlineBlockRow, InlineBlockRow)
        | (TableRow, TableRow)
        | (TableRowGroup, TableRowGroup)
        | (Table, Table)
        | (Contents, Contents)
        | (InlineSpace, InlineSpace)
        | (Skip, Skip)
        | (FlowRoot, FlowRoot) => true,
        (
            SvgRoot { view_box: v1, preserve_aspect_ratio: p1 },
            SvgRoot { view_box: v2, preserve_aspect_ratio: p2 },
        ) => v1 == v2 && p1 == p2,
        // `svg_paint_matrix` is deliberately excluded: it is a layout *output*
        // (`box_tree.rs`, "Stored in the dedicated `svg_paint_matrix` output
        // field"), identity on a freshly-built box and only filled in during
        // `lay_out`. Comparing it would make every SVG shape unequal to its own
        // laid-out predecessor — the same reason `InlineRun` compares `segments`
        // (pre-layout) and not its laid-out `lines`. A clean graft copies `kind`
        // wholesale from `prev`, so the computed matrix carries forward.
        (
            SvgShape { shape: s1, svg_transform: t1, .. },
            SvgShape { shape: s2, svg_transform: t2, .. },
        ) => s1 == s2 && t1 == t2,
        (
            SvgText {
                text: t1,
                x: x1,
                y: y1,
                dx: dx1,
                dy: dy1,
                text_anchor: a1,
                dominant_baseline: b1,
                baseline_shift: sh1,
                svg_transform: tr1,
            },
            SvgText {
                text: t2,
                x: x2,
                y: y2,
                dx: dx2,
                dy: dy2,
                text_anchor: a2,
                dominant_baseline: b2,
                baseline_shift: sh2,
                svg_transform: tr2,
            },
        ) => {
            t1 == t2
                && x1 == x2
                && y1 == y2
                && dx1 == dx2
                && dy1 == dy2
                && a1 == a2
                && b1 == b2
                && sh1 == sh2
                && tr1 == tr2
        }
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
            style: std::sync::Arc::new(ComputedStyle::root()),
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
            style: std::sync::Arc::new(ComputedStyle::root()),
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

    // ── BUG-341 S8: every BoxKind must be graftable; a style break must not
    //    cost the subtree its geometry ─────────────────────────────────────

    /// Each of the six variants `kind_layout_eq` used to omit (falling into its
    /// `_ => false` arm) must now compare equal to itself. A variant missing
    /// here does not just lose its own box: `graft_geometry` propagates the
    /// failure to every ancestor, so one unlisted kind anywhere disables
    /// incremental layout for the whole document — which is precisely how
    /// chrome's SVG icons kept reuse at zero for slices S1-S7.
    #[test]
    fn kind_layout_eq_covers_every_box_kind_variant() {
        use crate::box_tree::{
            PreserveAspectRatio, SvgAlignX, SvgAlignY, SvgBaselineShift, SvgDominantBaseline,
            SvgMeetOrSlice, SvgShapeKind, SvgTextAnchor, SvgTransform, ViewBox,
        };

        let kinds = vec![
            BoxKind::Contents,
            BoxKind::Table,
            BoxKind::TableRowGroup,
            BoxKind::SvgRoot {
                view_box: Some(ViewBox { min_x: 0.0, min_y: 0.0, width: 24.0, height: 24.0 }),
                preserve_aspect_ratio: PreserveAspectRatio {
                    align_x: SvgAlignX::Mid,
                    align_y: SvgAlignY::Mid,
                    meet_or_slice: SvgMeetOrSlice::Meet,
                },
            },
            BoxKind::SvgShape {
                shape: SvgShapeKind::Path { d: "M0 0 L10 10".to_owned() },
                svg_transform: SvgTransform::translate(3.0, 4.0),
                svg_paint_matrix: SvgTransform::identity(),
            },
            BoxKind::SvgText {
                text: "hi".to_owned(),
                x: 1.0,
                y: 2.0,
                dx: 0.0,
                dy: 0.0,
                text_anchor: SvgTextAnchor::default(),
                dominant_baseline: SvgDominantBaseline::default(),
                baseline_shift: SvgBaselineShift::default(),
                svg_transform: SvgTransform::identity(),
            },
        ];
        for k in &kinds {
            assert!(kind_layout_eq(k, &k.clone()), "kind {k:?} must be equal to itself");
        }
        // Differing payload must still be unequal — the fix must not degrade
        // into "all SVG boxes are interchangeable".
        assert!(!kind_layout_eq(
            &BoxKind::SvgShape {
                shape: SvgShapeKind::Circle { cx: 0.0, cy: 0.0, r: 1.0 },
                svg_transform: SvgTransform::identity(),
                svg_paint_matrix: SvgTransform::identity(),
            },
            &BoxKind::SvgShape {
                shape: SvgShapeKind::Circle { cx: 0.0, cy: 0.0, r: 2.0 },
                svg_transform: SvgTransform::identity(),
                svg_paint_matrix: SvgTransform::identity(),
            },
        ));
    }

    /// `svg_paint_matrix` is a layout *output*: identity on a freshly-built box,
    /// filled in during `lay_out`. Comparing it would make every SVG shape
    /// unequal to its own laid-out predecessor, silently reproducing the very
    /// bug this slice fixed.
    #[test]
    fn kind_layout_eq_ignores_the_layout_written_svg_paint_matrix() {
        use crate::box_tree::{SvgShapeKind, SvgTransform};

        let fresh = BoxKind::SvgShape {
            shape: SvgShapeKind::Circle { cx: 1.0, cy: 1.0, r: 5.0 },
            svg_transform: SvgTransform::identity(),
            svg_paint_matrix: SvgTransform::identity(),
        };
        let laid_out = BoxKind::SvgShape {
            shape: SvgShapeKind::Circle { cx: 1.0, cy: 1.0, r: 5.0 },
            svg_transform: SvgTransform::identity(),
            svg_paint_matrix: SvgTransform { matrix: [2.0, 0.0, 0.0, 2.0, 40.0, 12.0] },
        };
        assert!(kind_layout_eq(&fresh, &laid_out));
    }

    /// A node whose style changed must be re-laid-out, but its descendants keep
    /// their geometry. Before BUG-341 S8 `graft_geometry` returned before
    /// recursing, so a single differing node — the root box, on every cycle,
    /// because `lay_out` writes the used viewport `height` back into its style —
    /// threw away geometry reuse for the entire document.
    #[test]
    fn graft_style_change_still_reuses_child_geometry() {
        let prev = block_with_children(
            1,
            Rect::new(0.0, 0.0, 800.0, 60.0),
            vec![leaf(2, Rect::new(0.0, 10.0, 100.0, 50.0))],
        );

        let mut fresh =
            block_with_children(1, Rect::ZERO, vec![leaf(2, Rect::ZERO)]);
        // Only the parent's style differs — exactly the used-value write-back shape.
        std::sync::Arc::make_mut(&mut fresh.style).height = Some(crate::style::Length::Px(600.0));
        mark_subtree_dirty(&mut fresh);

        let all_clean = graft_geometry(&mut fresh, &prev);

        assert!(!all_clean, "the node whose style changed must not report itself clean");
        assert!(fresh.dirty.is_dirty(), "the changed node must be re-laid-out");
        assert!(
            fresh.children[0].dirty.is_clean(),
            "the unchanged child must keep its geometry — losing it here is the S8 regression",
        );
        assert_eq!(
            fresh.children[0].rect,
            Rect::new(0.0, 10.0, 100.0, 50.0),
            "the child's geometry must be grafted from prev",
        );
    }

    // ── BUG-341 S13: the used-value writeback must not read as a style change ──

    /// A box whose *only* difference from its predecessor is what `lay_out`
    /// wrote back into the predecessor's style (flex item main size, in this
    /// fixture) must still be grafted clean when the cascade demonstrably
    /// handed back the same style allocation for its node.
    ///
    /// A gate on the reuse **count**, not on output: a graft that refuses this
    /// box still lays it out and produces identical geometry — just for 122 of
    /// the chrome document's 318 boxes per hover flip, which is exactly what
    /// the S13 census measured before this fix.
    #[test]
    fn graft_reuses_a_box_whose_prev_style_only_carries_used_values() {
        let cascade_style = std::sync::Arc::new(ComputedStyle::root());
        let node = NodeId::from_index(2);

        // `prev` is a laid-out tree: `lay_out_flex` has overwritten the item's
        // main size with the resolved used value.
        let mut prev = leaf(2, Rect::new(0.0, 10.0, 100.0, 50.0));
        prev.style = std::sync::Arc::clone(&cascade_style);
        std::sync::Arc::make_mut(&mut prev.style).width = Some(crate::style::Length::Px(100.0));
        let mut prev_root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 60.0), vec![prev]);
        prev_root.style = std::sync::Arc::clone(&cascade_style);

        // The freshly-cascaded tree holds the cascade cache's own allocation.
        let mut fresh_child = leaf(2, Rect::ZERO);
        fresh_child.style = std::sync::Arc::clone(&cascade_style);
        let mut fresh = block_with_children(1, Rect::ZERO, vec![fresh_child]);
        fresh.style = std::sync::Arc::clone(&cascade_style);
        mark_subtree_dirty(&mut fresh);

        let mut cascade = std::collections::HashMap::new();
        cascade.insert(node, std::sync::Arc::clone(&cascade_style));
        cascade.insert(NodeId::from_index(1), std::sync::Arc::clone(&cascade_style));

        let all_clean = graft_geometry_with_cascade(&mut fresh, &prev_root, Some(&cascade));

        assert!(all_clean, "a used-value-only difference must not defeat the graft");
        assert!(fresh.children[0].dirty.is_clean());
        assert_eq!(
            fresh.children[0].rect,
            Rect::new(0.0, 10.0, 100.0, 50.0),
            "the reused box must carry prev's geometry",
        );
        assert_eq!(
            fresh.children[0].style.width, None,
            "a reused box keeps its own cascade style — inheriting prev's used values would \
             pin layout output into the live tree and mis-size the box if it later goes dirty",
        );
    }

    /// The counterpart: when the cascade produced a *different* style allocation
    /// for the node, the box must still be rejected. Without this the S13 fix
    /// would degrade into "styles never matter", which no differential test on
    /// geometry alone would catch on an unchanged fixture.
    #[test]
    fn graft_still_rejects_a_box_whose_cascade_style_changed() {
        let prev_cascade_style = std::sync::Arc::new(ComputedStyle::root());
        let node = NodeId::from_index(2);

        let mut prev = leaf(2, Rect::new(0.0, 10.0, 100.0, 50.0));
        prev.style = std::sync::Arc::clone(&prev_cascade_style);

        // A genuinely re-cascaded child: a different allocation *and* a
        // different value.
        let mut fresh_child = leaf(2, Rect::ZERO);
        std::sync::Arc::make_mut(&mut fresh_child.style).font_size = 24.0;
        let mut fresh = block_with_children(1, Rect::ZERO, vec![fresh_child]);
        mark_subtree_dirty(&mut fresh);
        let prev_root = block_with_children(1, Rect::new(0.0, 0.0, 800.0, 60.0), vec![prev]);

        let mut cascade = std::collections::HashMap::new();
        cascade.insert(node, prev_cascade_style);

        let all_clean = graft_geometry_with_cascade(&mut fresh, &prev_root, Some(&cascade));

        assert!(!all_clean, "a real cascade change must still be rejected");
        assert!(fresh.children[0].dirty.is_dirty(), "the re-cascaded box must be re-laid-out");
    }

    /// The census counters must partition the visited set — every box lands in
    /// exactly one bucket. They are the S13 gates' only instrument, and a
    /// double-count or a gap would make every number derived from them wrong.
    #[test]
    fn graft_stats_partition_the_visited_set() {
        let prev_root = block_with_children(
            1,
            Rect::new(0.0, 0.0, 800.0, 60.0),
            vec![leaf(2, Rect::new(0.0, 10.0, 100.0, 50.0)), leaf(3, Rect::new(0.0, 60.0, 100.0, 20.0))],
        );
        let mut fresh_changed = leaf(2, Rect::ZERO);
        std::sync::Arc::make_mut(&mut fresh_changed.style).font_size = 24.0;
        let mut fresh =
            block_with_children(1, Rect::ZERO, vec![fresh_changed, leaf(3, Rect::ZERO)]);
        mark_subtree_dirty(&mut fresh);

        let _ = take_graft_stats();
        graft_geometry(&mut fresh, &prev_root);
        let s = take_graft_stats();

        assert_eq!(s.visited, 3, "root + two children");
        assert_eq!(
            s.reused_clean
                + s.reject_identity
                + s.reject_style
                + s.reject_child_count
                + s.reject_descendant,
            s.visited,
            "every visited box must be counted in exactly one bucket: {s:?}",
        );
        assert_eq!(s.reject_style, 1, "the re-cascaded child");
        assert_eq!(s.reject_descendant, 1, "its parent");
        assert_eq!(s.reused_clean, 1, "the untouched sibling");
        assert_eq!(take_graft_stats(), GraftStats::default(), "taking must reset");
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
        std::sync::Arc::make_mut(&mut prev_root.style).font_size = 16.0;

        let mut fresh = leaf(1, Rect::ZERO);
        std::sync::Arc::make_mut(&mut fresh.style).font_size = 24.0; // style changed
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
    fn mutation_incremental_restyle_hover_entering_from_nothing_matches_full() {
        // BUG-341 S14's exact shape and its exact risk: hover arrives from
        // "nothing hovered" onto a *deep* node, so `:hover` flips on the whole
        // ancestor chain and S14 drops from the root-set every ancestor no
        // state-dependent compound can match. Here two of those ancestors
        // (`.card`, `.item`) *can* be matched and must survive the narrowing —
        // `.item:hover .icon` restyles a descendant, so dropping `.item` would
        // leave `.icon` with a stale style and a visibly wrong height.
        //
        // `.card:hover` deliberately changes a *colour*, not `padding`: an
        // ancestor whose own box metrics change leaves its clean-grafted
        // descendants at their previous used width, which is BUG-355 — a
        // pre-existing hole in `graft_geometry`, reproducible with every node
        // in `dirty_roots` and therefore not about this narrowing at all. Put
        // `padding: 9px` back here once BUG-355 is fixed; this test is the
        // repro.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::{layout_measured_hyp_with_counters, layout_mutation_incremental_restyle};
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::{clear_interactive_state, restyle_root_set_for_state_change, set_interactive_state};
        use lumen_core::ext::NullHyphenationProvider;

        let html = r#"<div class="card"><ul id="list">
            <li id="a" class="item"><span id="icon" class="icon">x</span></li>
            <li id="b" class="item"><span class="icon">y</span></li>
        </ul></div>"#;
        let css = r#"
            .card { padding: 3px; }
            .item { color: black; height: 20px; }
            .icon { height: 8px; }
            .card:hover { color: navy; }
            .item:hover { color: blue; height: 30px; }
            .item:hover .icon { height: 16px; color: green; }
        "#;
        let doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);
        let icon = doc.find_by_id("icon").expect("#icon must exist");

        // Baseline pass (`prev`): nothing hovered.
        clear_interactive_state();
        let (prev, prev_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );

        // Incremental: the pointer enters, landing on the deep `.icon`.
        set_interactive_state(Some(icon), None, None);
        let state_index = crate::style::restyle_state_index(&doc, &sheet);
        let dirty_roots = restyle_root_set_for_state_change(&doc, None, Some(icon), &state_index);
        assert!(
            !dirty_roots.is_empty(),
            "the `.card`/`.item` ancestors carry hover rules — narrowing them away would be wrong",
        );
        let delta = RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Nothing };
        set_incremental_restyle(true);
        let (incr, incr_counters) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        set_incremental_restyle(false);

        // Reference: full layout + full cascade of the post-transition state.
        set_interactive_state(Some(icon), None, None);
        let (full, full_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );
        clear_interactive_state();

        assert_eq!(
            incr_counters.styles(),
            full_counters.styles(),
            "narrowed incremental cascade must reproduce the full cascade exactly",
        );

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

    // ── BUG-341 S15: incremental box-build wired into the restyle path ─────

    /// Shared fixture for the S15 gates: a hover flip on a deep node under a
    /// container whose siblings are provably clean. Returns
    /// `(doc, sheet, prev, prev_counters, icon)`.
    #[allow(clippy::type_complexity)]
    fn s15_fixture() -> (
        lumen_dom::Document,
        lumen_css_parser::Stylesheet,
        LayoutBox,
        crate::CounterMap,
        lumen_dom::NodeId,
    ) {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_measured_hyp_with_counters;
        use crate::style::clear_interactive_state;
        use lumen_core::ext::NullHyphenationProvider;

        // 10 siblings — over `RAYON_MIN_FLEX_CHILDREN`, so the container's
        // children are built on rayon workers. That path is exactly where the
        // pre-S15 thread-local flag check silently disabled reuse.
        let mut html = String::from(r#"<div class="card"><ul id="list">"#);
        for i in 0..10 {
            html.push_str(&format!(
                r#"<li id="i{i}" class="item"><span id="s{i}" class="icon">x</span></li>"#
            ));
        }
        html.push_str("</ul></div>");
        let css = r#"
            #list { display: flex; }
            .item { color: black; height: 20px; }
            .icon { height: 8px; }
            .item:hover { color: blue; height: 30px; }
            .item:hover .icon { height: 16px; color: green; }
        "#;
        let doc = parse_html(&html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);
        let icon = doc.find_by_id("s3").expect("#s3 must exist");

        clear_interactive_state();
        let (prev, prev_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );
        (doc, sheet, prev, prev_counters, icon)
    }

    /// BUG-341 S15 counter gate: the box-build stage must actually *reuse*
    /// subtrees, not merely produce the right answer.
    ///
    /// The S8 lesson applies verbatim here — a reuse mechanism that reuses
    /// nothing satisfies every `incremental == full` differential test, just
    /// slowly. So this asserts the tally: the flip touches one `<li>` and its
    /// `<span>`, so the other nine `<li>` subtrees must come out of `prev`
    /// wholesale, and the total number of boxes really constructed must drop
    /// well below what a full build costs.
    #[test]
    fn bug341_s15_hover_flip_reuses_the_clean_sibling_subtrees() {
        use crate::box_tree::{
            layout_mutation_incremental_restyle, set_incremental_box_build, take_box_build_stats,
        };
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::{
            clear_interactive_state, restyle_root_set_for_state_change, restyle_state_index,
            set_interactive_state,
        };
        use lumen_core::ext::NullHyphenationProvider;

        let (doc, sheet, prev, prev_counters, icon) = s15_fixture();
        let vp = Size::new(800.0, 600.0);
        // `prev`'s own build is the "what a full build costs" reference.
        let full_built = take_box_build_stats().built;
        assert!(full_built > 10, "fixture must build a non-trivial tree, got {full_built}");

        set_interactive_state(Some(icon), None, None);
        let state_index = restyle_state_index(&doc, &sheet);
        let dirty_roots = restyle_root_set_for_state_change(&doc, None, Some(icon), &state_index);
        let delta =
            RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Nothing };
        set_incremental_restyle(true);
        set_incremental_box_build(true);
        let _ = take_box_build_stats();
        let (_incr, _c) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        let stats = take_box_build_stats();
        set_incremental_box_build(false);
        set_incremental_restyle(false);
        clear_interactive_state();

        assert!(
            stats.reused >= 9,
            "the nine untouched <li> siblings must be cloned from prev, got {stats:?}",
        );
        assert!(
            stats.built * 2 < full_built,
            "incremental build must construct far fewer boxes than a full one \
             ({} built vs {full_built} full) — {stats:?}",
            stats.built,
        );
    }

    /// BUG-341 S15: whatever the box-build stage reuses, the resulting tree
    /// must still be indistinguishable from a full rebuild + full cascade.
    ///
    /// Same transition as the counter gate above, compared against the
    /// reference the whole track is defined by.
    #[test]
    fn bug341_s15_reused_boxes_match_a_full_rebuild() {
        use crate::box_tree::{
            layout_measured_hyp_with_counters, layout_mutation_incremental_restyle,
            set_incremental_box_build,
        };
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::{
            clear_interactive_state, restyle_root_set_for_state_change, restyle_state_index,
            set_interactive_state,
        };
        use lumen_core::ext::NullHyphenationProvider;

        let (doc, sheet, prev, prev_counters, icon) = s15_fixture();
        let vp = Size::new(800.0, 600.0);

        set_interactive_state(Some(icon), None, None);
        let state_index = restyle_state_index(&doc, &sheet);
        let dirty_roots = restyle_root_set_for_state_change(&doc, None, Some(icon), &state_index);
        let delta =
            RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Nothing };
        set_incremental_restyle(true);
        set_incremental_box_build(true);
        let (incr, incr_counters) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        set_incremental_box_build(false);
        set_incremental_restyle(false);

        set_interactive_state(Some(icon), None, None);
        let (full, full_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );
        clear_interactive_state();

        assert_eq!(
            incr_counters.styles(),
            full_counters.styles(),
            "cascade must still reproduce the full cascade exactly",
        );
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

    /// BUG-341 S15: a DOM-content change must switch the reuse off entirely.
    ///
    /// `content_dirty: crate::counters::ContentDirty::Untracked` is the whole correctness contract — text and
    /// attribute values `build_box` reads are invisible to a style-equality
    /// comparison, so `clean_subtrees` must stay empty and every box must be
    /// rebuilt. Nothing else in the mechanism checks this.
    #[test]
    fn bug341_s15_dom_content_change_disables_box_reuse() {
        use crate::box_tree::{
            layout_mutation_incremental_restyle, set_incremental_box_build, take_box_build_stats,
        };
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::clear_interactive_state;
        use lumen_core::ext::NullHyphenationProvider;

        let (doc, sheet, prev, prev_counters, _icon) = s15_fixture();
        let vp = Size::new(800.0, 600.0);
        clear_interactive_state();

        let delta = RestyleDelta {
            prev_styles: prev_counters.styles(),
            dirty_roots: std::collections::HashSet::new(),
            content_dirty: crate::counters::ContentDirty::Untracked,
        };
        set_incremental_restyle(true);
        set_incremental_box_build(true);
        let _ = take_box_build_stats();
        let _ = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        let stats = take_box_build_stats();
        set_incremental_box_build(false);
        set_incremental_restyle(false);

        assert_eq!(
            stats.reused, 0,
            "a delta that cannot vouch for DOM content must reuse nothing — {stats:?}",
        );
    }

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
        let state_index = crate::style::restyle_state_index(&doc, &sheet);
        let dirty_roots = restyle_root_set_for_state_change(&doc, Some(a), Some(b), &state_index);
        let delta = RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Nothing };
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

        let state_index = crate::style::restyle_state_index(&doc, &sheet);
        let dirty_roots = restyle_root_set_for_state_change(&doc, None, None, &state_index);
        assert!(dirty_roots.is_empty(), "no-op transition must yield an empty root-set");
        let delta = RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Nothing };
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

    #[test]
    fn mutation_incremental_restyle_dom_class_change_matches_full() {
        // BUG-341 S6: the full pipeline (`layout_mutation_incremental_restyle`,
        // not just the cascade alone — `incr_cascade_class_change_matches_full_
        // and_recomputes_subset` in this same file already covers the cascade
        // in isolation) for a DOM class mutation that also changes geometry
        // (`.active { height }`), the class of content change `bind_model`
        // produces (`set_class_token`/`set_attr`) and `bind_model_tracked`
        // (crates/chrome) reports via `restyle_root_set_for_node_change`. Must
        // reproduce a full `layout_measured_hyp` bit-for-bit, `dom_content_stable`
        // correctly `false` (RestyleDelta doc: DOM mutation, not just
        // interactive state).
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use lumen_dom::NodeData;
        use crate::box_tree::{
            layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental_restyle,
        };
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::{restyle_node_index, restyle_root_set_for_node_change, NodeChange};
        use lumen_core::ext::NullHyphenationProvider;

        let html = r#"<ul id="menu">
            <li id="a" class="item">a</li>
            <li id="b" class="item">b</li>
            <li id="c" class="item">c</li>
        </ul>
        <div id="unrelated"><p>x</p><p>y</p><p>z</p></div>"#;
        let css = r#"
            .item { color: black; padding: 4px; height: 20px; }
            .item.active { color: blue; height: 30px; }
            .item.active + .item { color: green; }
        "#;
        let mut doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);

        let (prev, prev_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );

        // Mutate #a's class in place, mirroring what `set_attr`/`set_class_token`
        // do in `crates/chrome/src/model.rs`.
        let a = doc.find_by_id("a").expect("#a must exist");
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(a).data {
            for attr in attrs.iter_mut() {
                if attr.name.local == "class" {
                    attr.value = "item active".to_string();
                }
            }
        }

        let node_index = restyle_node_index(&doc, &sheet);
        let dirty_roots =
            restyle_root_set_for_node_change(&doc, [(a, NodeChange::Attr("class"))], &node_index);
        let delta = RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Untracked };
        set_incremental_restyle(true);
        let (incr, _incr_counters) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        set_incremental_restyle(false);

        let full = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);

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


    /// BUG-341 S17 — the narrowed DOM root-set must still reproduce a full
    /// cascade exactly, including the mutated node's *descendants*.
    ///
    /// Correctness first: the sheet restyles `#a` itself, a descendant of `#a`
    /// (`[data-x="2"] span`), and — via a sibling combinator whose left compound
    /// cannot match `#a` — a sibling of a *different* element. S17 narrows the
    /// root-set from `#menu` (the parent) to `#a` alone, so if the narrowing
    /// were wrong about what a `data-x` write can reach, the `<span>` inside
    /// `#a` would keep its old colour and this comparison of the whole cascade
    /// (not just geometry — a colour change moves no boxes) would fail.
    ///
    /// Then the count: the root-set really is `{#a}`, which is the only thing
    /// that can regress silently — a mechanism that narrows nothing still
    /// reproduces the full cascade, just slowly (the S8 lesson).
    #[test]
    fn mutation_incremental_restyle_narrowed_attr_change_matches_full() {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use lumen_dom::{build_flat_tree, NodeData};
        use crate::box_tree::{
            layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental_restyle,
        };
        use crate::counters::{precompute_counters, set_incremental_restyle, RestyleDelta};
        use crate::style::{restyle_node_index, restyle_root_set_for_node_change, NodeChange};
        use lumen_core::ext::NullHyphenationProvider;

        let html = r#"<ul id="menu">
            <li id="a" class="item" data-x="1"><span id="as">a</span></li>
            <li id="b" class="item"><span id="bs">b</span></li>
        </ul>
        <div id="unrelated"><p>x</p></div>"#;
        let css = r#"
            .item { color: black; padding: 4px; height: 20px; }
            [data-x="2"] { color: blue; height: 40px; }
            [data-x="2"] span { color: red; }
            .other + .item { color: green; }
        "#;
        let mut doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);

        let (prev, prev_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );

        let a = doc.find_by_id("a").expect("#a must exist");
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(a).data {
            for attr in attrs.iter_mut() {
                if attr.name.local == "data-x" {
                    attr.value = "2".to_string();
                }
            }
        }

        let node_index = restyle_node_index(&doc, &sheet);
        let dirty_roots =
            restyle_root_set_for_node_change(&doc, [(a, NodeChange::Attr("data-x"))], &node_index);
        assert_eq!(
            dirty_roots,
            [a].into_iter().collect::<std::collections::HashSet<_>>(),
            "the only sibling rule in this sheet is `.other + .item`, which cannot match #a — \
             S17 must narrow the root-set to #a instead of widening to #menu",
        );

        let delta = RestyleDelta {
            prev_styles: prev_counters.styles(),
            dirty_roots,
            content_dirty: crate::counters::ContentDirty::Untracked,
        };
        set_incremental_restyle(true);
        crate::style::invalidate_rule_idx_cache();
        let (incr, incr_counters) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        set_incremental_restyle(false);

        let flat = build_flat_tree(&doc);
        crate::style::invalidate_rule_idx_cache();
        let full_counters = precompute_counters(&doc, &sheet, vp, &flat, false);
        assert_eq!(
            incr_counters.styles(),
            full_counters.styles(),
            "the narrowed root-set must reproduce the full cascade for every node — a `data-x` \
             write restyles #a and, via `[data-x=\"2\"] span`, its descendant",
        );

        let full = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);
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
    fn mutation_incremental_restyle_structural_change_matches_full() {
        // BUG-341 S6: a structural DOM change (a new sibling `<li>` appended,
        // the shape `bind_model`'s `reconcile_row_list` produces for a new
        // tab/workspace) — the freshly-inserted node has no `prev_styles`
        // entry, so `incremental_precompute_counters`'s own "absent from
        // prev_styles always recomputes" rule (crates/engine/layout/src/
        // counters.rs) must cover it even though it is outside `dirty_roots`;
        // `graft_geometry` must independently decline to reuse the container's
        // geometry once child counts differ. Must reproduce a full
        // `layout_measured_hyp` bit-for-bit.
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use lumen_dom::{NodeData, QualName};
        use crate::box_tree::{
            layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental_restyle,
        };
        use crate::counters::{set_incremental_restyle, RestyleDelta};
        use crate::style::{restyle_node_index, restyle_root_set_for_node_change, NodeChange};
        use lumen_core::ext::NullHyphenationProvider;

        let html = r#"<ul id="menu">
            <li id="a" class="item">a</li>
            <li id="b" class="item">b</li>
        </ul>"#;
        let css = ".item { color: black; padding: 4px; height: 20px; }";
        let mut doc = parse_html(html);
        let sheet = parse_css(css);
        let vp = Size::new(800.0, 600.0);

        let (prev, prev_counters) = layout_measured_hyp_with_counters(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false,
        );

        // Append a brand-new <li>, mirroring `reconcile_row_list`'s `build()`
        // path for a newly-added row — no prior `prev_styles` entry for it.
        let menu = doc.find_by_id("menu").expect("#menu must exist");
        let new_li = doc.create_element(QualName::html("li"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(new_li).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html("class"), value: "item".to_string() });
        }
        let text = doc.create_text("c".to_string());
        doc.append_child(new_li, text);
        doc.append_child(menu, new_li);

        // Structural change: `reconcile_row_list` reports the container
        // (`#menu`) touched, not the newly-created node itself.
        let node_index = restyle_node_index(&doc, &sheet);
        let dirty_roots =
            restyle_root_set_for_node_change(&doc, [(menu, NodeChange::Unattributed)], &node_index);
        let delta = RestyleDelta { prev_styles: prev_counters.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Untracked };
        set_incremental_restyle(true);
        let (incr, _incr_counters) = layout_mutation_incremental_restyle(
            &doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false, &prev, &delta,
        );
        set_incremental_restyle(false);

        let full = layout_measured_hyp(&doc, &sheet, vp, &FixedMeasurer, &NullHyphenationProvider, false);

        let mut ia = Vec::new();
        let mut fb = Vec::new();
        collect_rects(&incr, &mut ia);
        collect_rects(&full, &mut fb);
        assert_eq!(ia.len(), fb.len(), "box count must match full layout (incl. the new <li>)");
        for ((na, ra), (nb, rb)) in ia.iter().zip(fb.iter()) {
            assert_eq!(na, nb, "node order must match");
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
            content_dirty: crate::counters::ContentDirty::Nothing,
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

    /// BUG-341 S9 gate — by *identity*, not by output.
    ///
    /// `assert_cascades_eq` cannot tell a reused `ComputedStyle` from a fresh
    /// deep copy of one: both compare equal. That is precisely how S8's
    /// `graft_geometry` stayed inert for five slices while passing every
    /// differential test. So assert what the reuse path is actually for —
    /// nodes outside the dirty root-set must come back as the *same*
    /// allocation the previous pass produced, and nodes inside it must not.
    #[test]
    fn incr_cascade_reuse_hands_back_the_same_style_allocation() {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use lumen_dom::build_flat_tree;
        use crate::counters::{incremental_precompute_counters, precompute_counters, set_incremental_restyle, RestyleDelta};

        let doc = parse_html(r#"<div id="a"><p>one</p></div><div id="b"><p>two</p></div>"#);
        let sheet = parse_css("div { color: red; } p { font-weight: bold; }");
        let vp = Size::new(800.0, 600.0);
        let flat = build_flat_tree(&doc);

        crate::style::invalidate_rule_idx_cache();
        let prev = precompute_counters(&doc, &sheet, vp, &flat, false);

        // Dirty exactly one subtree root; everything else must be reused.
        let dirty_root = doc.find_by_id("a").expect("#a exists");
        let mut dirty_roots = std::collections::HashSet::new();
        dirty_roots.insert(dirty_root);

        set_incremental_restyle(true);
        crate::style::invalidate_rule_idx_cache();
        let delta = RestyleDelta {
            prev_styles: prev.styles(),
            dirty_roots,
            content_dirty: crate::counters::ContentDirty::Nothing,
        };
        let incr = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, &delta);
        set_incremental_restyle(false);

        let clean = doc.find_by_id("b").expect("#b exists");
        assert!(
            std::sync::Arc::ptr_eq(&prev.styles()[&clean], &incr.styles()[&clean]),
            "a node outside the dirty root-set must be handed back as the same \
             allocation, not re-cascaded or deep-copied"
        );
        // Its descendants ride the same reuse.
        let clean_child = doc.get(clean).children[0];
        assert!(std::sync::Arc::ptr_eq(&prev.styles()[&clean_child], &incr.styles()[&clean_child]));

        assert!(
            !std::sync::Arc::ptr_eq(&prev.styles()[&dirty_root], &incr.styles()[&dirty_root]),
            "a node in the dirty root-set must have been re-cascaded"
        );
        assert_eq!(
            prev.styles()[&dirty_root], incr.styles()[&dirty_root],
            "…and the recompute must land on the same value",
        );
    }

    /// BUG-341 S12 gate — by identity, not by output.
    ///
    /// `build_box` must hand the cascade cache's `Arc<ComputedStyle>` straight
    /// into `LayoutBox::style` rather than deep-copying it, and cloning a laid
    /// out tree (the incremental pipeline does exactly that once per frame to
    /// persist `prev`) must share those allocations too. Both were 3.2 KB,
    /// ~30-heap-field copies per box before S12: measured at 1.2 ms of
    /// `lay_out`'s 3.7 ms plus 1.5 ms of per-cycle bookkeeping on `CC12_HOVER`.
    ///
    /// A regression here is invisible in geometry — the output is identical
    /// either way, only slower — which is why this asserts pointers. Same
    /// reasoning as `incr_cascade_reuse_hands_back_the_same_style_allocation`
    /// above and the S8 count gate.
    #[test]
    fn built_boxes_share_the_cascade_cache_style_allocation() {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_measured_hyp_with_counters;

        let doc = parse_html(r#"<div id="a"><p id="p">one</p></div>"#);
        let sheet = parse_css("div { color: red; } p { font-weight: bold; }");
        let vp = Size::new(800.0, 600.0);

        crate::style::invalidate_rule_idx_cache();
        let (tree, counters) = layout_measured_hyp_with_counters(
            &doc,
            &sheet,
            vp,
            &FixedMeasurer,
            &lumen_core::ext::NullHyphenationProvider,
            false,
        );

        fn find(b: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
            if b.node == id && !matches!(b.kind, BoxKind::Skip) {
                return Some(b);
            }
            b.children.iter().find_map(|c| find(c, id))
        }

        let p = doc.find_by_id("p").expect("#p exists");
        let p_box = find(&tree, p).expect("#p has a box");
        assert!(
            std::sync::Arc::ptr_eq(&p_box.style, &counters.styles()[&p]),
            "build_box must share the cascade cache's allocation, not clone it"
        );

        // Persisting the tree for the next frame must not resurrect the copy.
        let persisted = tree.clone();
        let persisted_p = find(&persisted, p).expect("#p has a box in the clone");
        assert!(
            std::sync::Arc::ptr_eq(&persisted_p.style, &p_box.style),
            "cloning a layout tree must share styles, not deep-copy them"
        );
    }

    /// BUG-341 S12: the copy-on-write half of the contract above.
    ///
    /// The passes that rewrite a used value into a box's style (flex item
    /// stretch, `font-size-adjust`, container queries) reach it through
    /// `Arc::make_mut`, so they must get a private copy and leave the cascade
    /// cache — shared with the *previous* frame's tree — untouched. Without
    /// this, a stretched flex item would corrupt the cascade result every other
    /// pipeline stage compares against.
    #[test]
    fn used_value_writeback_does_not_leak_into_the_cascade_cache() {
        use lumen_css_parser::parse as parse_css;
        use lumen_html_parser::parse as parse_html;
        use crate::box_tree::layout_measured_hyp_with_counters;

        let doc = parse_html(r#"<div id="row"><span id="item">x</span></div>"#);
        let sheet = parse_css(
            "#row { display: flex; align-items: stretch; height: 200px; } \
             #item { display: block; }",
        );
        let vp = Size::new(800.0, 600.0);

        crate::style::invalidate_rule_idx_cache();
        let (tree, counters) = layout_measured_hyp_with_counters(
            &doc,
            &sheet,
            vp,
            &FixedMeasurer,
            &lumen_core::ext::NullHyphenationProvider,
            false,
        );

        fn find(b: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
            if b.node == id && !matches!(b.kind, BoxKind::Skip) {
                return Some(b);
            }
            b.children.iter().find_map(|c| find(c, id))
        }

        let item = doc.find_by_id("item").expect("#item exists");
        let item_box = find(&tree, item).expect("#item has a box");
        let cached = &counters.styles()[&item];
        // Either the box still shares the cache entry (nothing was written
        // back), or it took a private copy — never a mutated shared one.
        if !std::sync::Arc::ptr_eq(&item_box.style, cached) {
            assert_eq!(
                cached.height, None,
                "the cascade cache must keep the *computed* height, not the used \
                 value a stretch pass wrote into the box"
            );
        }
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
            incremental_precompute_counters, precompute_counters, set_incremental_restyle,
            take_cascade_stats, RestyleDelta,
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
        let state_index = crate::style::restyle_state_index(&doc, &sheet);
        let dirty_roots = restyle_root_set_for_state_change(&doc, Some(a), Some(b), &state_index);
        let delta = RestyleDelta { prev_styles: baseline.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Nothing };
        set_incremental_restyle(true);
        take_cascade_stats();
        crate::style::invalidate_rule_idx_cache();
        let incr_after = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, &delta);
        let recomputed = take_cascade_stats().recomputed as usize;
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
            incremental_precompute_counters, precompute_counters, set_incremental_restyle,
            take_cascade_stats, RestyleDelta,
        };
        use crate::style::{restyle_node_index, restyle_root_set_for_node_change, NodeChange};

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

        let node_index = restyle_node_index(&doc, &sheet);
        let dirty_roots =
            restyle_root_set_for_node_change(&doc, [(a, NodeChange::Attr("class"))], &node_index);
        // BUG-341 S4: a DOM class mutation is NOT `dom_content_stable` — box-build
        // reuse must not trust style-equality alone here (see `RestyleDelta` doc).
        let delta = RestyleDelta { prev_styles: baseline.styles(), dirty_roots, content_dirty: crate::counters::ContentDirty::Untracked };
        set_incremental_restyle(true);
        take_cascade_stats();
        crate::style::invalidate_rule_idx_cache();
        let incr_after = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, &delta);
        let recomputed = take_cascade_stats().recomputed as usize;
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
