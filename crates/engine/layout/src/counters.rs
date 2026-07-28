//! CSS Counters resolution — CSS Lists L3 §6.4 + CSS Content L3 §4 +
//! CSS Counter Styles L3 (custom `@counter-style` rules).
//!
//! Implements a pre-order DOM traversal that computes a per-element snapshot of
//! counter values (after the element's own counter-reset + counter-increment, before
//! children). These snapshots are used by `content_to_inline_segments` to resolve
//! `counter()` / `counters()` / `attr()` in `content:` for `::before` / `::after`.
//!
//! # Algorithm
//! For each element in pre-order:
//! 1. Apply `counter-reset` (push a new scope value onto the named stack).
//! 2. Apply `counter-increment` (add to the top of the named stack).
//! 3. Apply `counter-set` (set the top of the named stack to a value).
//! 4. Save a snapshot (`NodeId → stacks`) for `counter()` / `counters()` resolution.
//! 5. Recurse into children.
//! 6. Pop the scopes added in step 1.
//!
//! The reset → increment → set order is normative (CSS Lists L3 §4).
//!
//! This gives correct resolution for `::before` pseudo-elements (which read the state
//! after the element's own increment but before children). `::after` reads post-children
//! state; for Phase 0 we use the same pre-children snapshot (sufficient for the common
//! `counter-increment` on `li` + `::before { content: counter(x) }` pattern).
//!
//! # Custom counter styles
//! `CounterStyleRegistry` maps counter style names to `CounterStyleDef` — parsed
//! descriptors from `@counter-style` at-rules. Use `build_counter_style_registry`
//! to build from a `Stylesheet`, then pass to `format_counter_with_registry`.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lumen_dom::{Document, FlatTree, NodeData, NodeId};

use crate::style::{
    compute_pseudo_element_style, compute_style, Content, ComputedStyle, ContentItem, ListStyleType,
};
use lumen_css_parser::Stylesheet;
use lumen_core::Size;

/// Per-element counter stacks snapshot.
///
/// Maps counter name → ordered stack of current values (outermost scope first,
/// innermost last). The stack holds all nested scopes so `counters()` can join them.
pub type CounterSnapshot = HashMap<String, Vec<i32>>;

/// Generated-content slot of an element that can carry `open-quote` /
/// `close-quote` content (CSS Generated Content L3 §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuoteSlot {
    /// `::before` pseudo-element content (processed before children).
    Before,
    /// `::after` pseudo-element content (processed after children).
    After,
}

/// BUG-341 S24 — the per-node cascade cache, carried from one pass to the next.
///
/// Before this slice every pass built its own map: a hover flip reused all 828
/// of the previous pass's `Arc<ComputedStyle>`s and then inserted all 828 into a
/// fresh `HashMap`, which the pipeline cloned wholesale so it could serve as the
/// next cycle's `prev_styles`. Three full-size passes over the document to end
/// up with the map it started from. The map is now moved into the pass
/// ([`RestyleDelta::prev_styles`]) and moved back out inside its [`CounterMap`],
/// with only the recomputed entries rewritten — the S19 answer applied to the
/// cascade cache instead of the box tree.
///
/// # Why entries carry a pass ordinal
///
/// The pre-S24 map had one property everything downstream relied on without
/// naming it: **an entry exists iff the immediately preceding pass visited that
/// node**. Absence is what forces a recompute for a node that was detached and
/// re-attached, or moved to a new parent, since the style it would otherwise
/// reuse was computed under a different inherited chain. A map that simply
/// accumulates entries forever loses exactly that property (`lumen_dom` never
/// frees a node or reuses a [`NodeId`], so a *stale* entry is not a wrong-node
/// hazard — it is a wrong-*pass* one).
///
/// So each entry is stamped with the ordinal of the pass that wrote or confirmed
/// it, and [`Self::finish_pass`] drops the ones this pass did not touch whenever
/// the counts disagree — a sweep that costs what the old rebuild always cost,
/// on the rare pass where something was actually removed, and nothing at all on
/// the steady state. The invariant it maintains is the one above, stated
/// exactly: after a pass, `entries` holds precisely the elements that pass
/// walked, all stamped with its ordinal.
#[derive(Debug, Default, Clone)]
pub struct CascadeStyles {
    /// `NodeId` → (style, ordinal of the pass that last wrote or confirmed it).
    entries: HashMap<NodeId, (Arc<ComputedStyle>, u32)>,
    /// Ordinal of the pass currently writing into this map, or of the last one
    /// to have finished. Wraps; see [`Self::reuse`] for why that is harmless.
    pass: u32,
    /// Elements the current pass has visited so far — the reference
    /// [`Self::finish_pass`] compares `entries.len()` against.
    visited: usize,
    /// Whether the last [`Self::finish_pass`] had to sweep. Observable so a
    /// gate can assert the steady state never does: a sweep every pass would
    /// put the O(document) cost this slice removes straight back, while
    /// producing byte-identical output — the S8 failure mode exactly.
    swept: bool,
    /// BUG-341 S26 — whether the pass that filled this cache recorded any
    /// counter snapshot or quote depth ([`CounterMap::nodes`] /
    /// [`CounterMap::quotes`]).
    ///
    /// It rides here because this is the one thing an interaction cycle already
    /// carries from pass to pass, so the fact costs no plumbing through the
    /// shell and page pipelines. It is what licenses
    /// [`incremental_precompute_counters`]'s no-op path: those two collections
    /// are rebuilt by the walk rather than carried, so a pass that does not walk
    /// hands back empty ones. That is only equivalent when the previous pass had
    /// nothing in them — and a document that *does* use `counter-increment` or
    /// `open-quote` would otherwise silently lose its generated content, i.e.
    /// visible corruption rather than a slow frame.
    ///
    /// Deliberately not a predicate over the stylesheet (the S10/S23 shape):
    /// `counter-reset` and friends can be set from a `style` attribute, which no
    /// sheet-wide scan can see. Recording what the pass actually produced has no
    /// such hole.
    generated_content: bool,
}

impl CascadeStyles {
    /// An empty cache sized for a document of `elements` styled elements.
    fn with_capacity(elements: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(elements),
            pass: 0,
            visited: 0,
            swept: false,
            generated_content: false,
        }
    }

    /// Start writing a new pass into this carried cache.
    fn begin_pass(&mut self) {
        self.pass = self.pass.wrapping_add(1);
        self.visited = 0;
    }

    /// Take `id`'s entry from the previous pass, restamping it as visited.
    ///
    /// `None` when there is nothing to reuse — no entry at all, or one the
    /// *immediately* preceding pass did not write. Both mean the same thing to
    /// the caller ("recompute"), which is why the ordinal can wrap without
    /// consequence: a mismatch only ever costs a recompute, and
    /// [`Self::finish_pass`] keeps the map exact so a mismatch cannot happen in
    /// the first place.
    ///
    /// One hash lookup, where the pre-S24 shape needed three (`contains_key`
    /// for the decision, an index for the value, an insert into the fresh map).
    fn reuse(&mut self, id: NodeId) -> Option<Arc<ComputedStyle>> {
        let pass = self.pass;
        let entry = self.entries.get_mut(&id)?;
        if entry.1.wrapping_add(1) != pass {
            return None;
        }
        entry.1 = pass;
        self.visited += 1;
        Some(Arc::clone(&entry.0))
    }

    /// Record a freshly computed style for `id`, returning the entry it
    /// displaced (the previous pass's style for that node), if any.
    ///
    /// The displaced value is what [`CounterMap::replaced_styles`] keeps for the
    /// graft — see that field.
    fn write(&mut self, id: NodeId, style: Arc<ComputedStyle>) -> Option<Arc<ComputedStyle>> {
        self.visited += 1;
        self.entries.insert(id, (style, self.pass)).map(|(prev, _)| prev)
    }

    /// Close the pass: drop every entry it did not visit.
    ///
    /// Skipped entirely when the counts already agree, which is every pass that
    /// removed no node — the whole point of carrying the map. When they do
    /// disagree the sweep is O(entries), i.e. exactly what rebuilding the map
    /// used to cost unconditionally.
    fn finish_pass(&mut self) {
        self.swept = self.entries.len() != self.visited;
        if !self.swept {
            return;
        }
        let pass = self.pass;
        self.entries.retain(|_, (_, stamp)| *stamp == pass);
    }

    /// How many passes have written into this cache — 0 for one that has only
    /// ever been filled by a full cascade.
    ///
    /// The counter that tells a carried map from a rebuilt one. A pipeline that
    /// goes back to handing each pass a fresh map produces byte-identical
    /// styles, just at the cost this slice removed (the S8 lesson), so the gate
    /// on the carry has to be this and not the cascade output.
    pub fn passes_lived(&self) -> u32 {
        self.pass
    }

    /// Whether the pass that just finished had to evict — see the `swept` field.
    pub fn swept_last_pass(&self) -> bool {
        self.swept
    }

    /// Whether the pass that filled this cache produced any counter snapshot or
    /// quote depth (BUG-341 S26) — see the `generated_content` field.
    pub fn generated_content(&self) -> bool {
        self.generated_content
    }

    /// Record what the pass that just filled `map` produced (BUG-341 S26).
    ///
    /// Called by both cascade entry points once their walk is done, so the fact
    /// travels with the cache to the pass that may skip walking.
    fn note_generated_content(&mut self, any: bool) {
        self.generated_content = any;
    }

    /// The style this cache holds for `id`, if any.
    pub fn get(&self, id: &NodeId) -> Option<&Arc<ComputedStyle>> {
        self.entries.get(id).map(|(style, _)| style)
    }

    /// Whether this cache holds an entry for `id`.
    pub fn contains_key(&self, id: &NodeId) -> bool {
        self.entries.contains_key(id)
    }

    /// Number of nodes this cache holds a style for.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate the nodes this cache holds a style for, in arbitrary order.
    pub fn keys(&self) -> impl Iterator<Item = &NodeId> {
        self.entries.keys()
    }

    /// Iterate `(node, style)` pairs in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &Arc<ComputedStyle>)> {
        self.entries.iter().map(|(id, (style, _))| (id, style))
    }

    /// This cache's contents as a plain map, dropping the pass stamps.
    ///
    /// Only used where a *snapshot* rather than the carrier is wanted — the
    /// flag-off fallback in [`incremental_precompute_counters`]. Carrying the
    /// map is the whole point everywhere else.
    fn into_plain(self) -> HashMap<NodeId, Arc<ComputedStyle>> {
        self.entries.into_iter().map(|(id, (style, _))| (id, style)).collect()
    }

    /// A cache holding exactly `styles`, as though one pass had just written it.
    ///
    /// For callers that assemble a cascade by hand rather than by walking a
    /// document — the graft's unit gates, which care about one or two nodes.
    pub fn from_plain(styles: HashMap<NodeId, Arc<ComputedStyle>>) -> Self {
        Self {
            entries: styles.into_iter().map(|(id, style)| (id, (style, 0))).collect(),
            pass: 0,
            visited: 0,
            swept: false,
            // Conservative: a hand-assembled cascade cannot vouch for what the
            // document's generated content looks like, so it never licenses the
            // no-op path.
            generated_content: true,
        }
    }
}

impl std::ops::Index<&NodeId> for CascadeStyles {
    type Output = Arc<ComputedStyle>;

    fn index(&self, id: &NodeId) -> &Arc<ComputedStyle> {
        self.get(id).expect("no cascade entry for node")
    }
}

impl<'a> IntoIterator for &'a CascadeStyles {
    type Item = (&'a NodeId, &'a Arc<ComputedStyle>);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl PartialEq for CascadeStyles {
    /// Compares the cascade *result*, not the bookkeeping: the pass ordinals are
    /// an artefact of how many passes a map has lived through, and the
    /// `incr == full` differential tests compare a carried map against a
    /// freshly built one by construction.
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .all(|(id, (style, _))| other.get(id).is_some_and(|o| style == o))
    }
}

impl Eq for CascadeStyles {}

/// BUG-341 S24 — the cascade as it stood *before* the current pass, for
/// [`crate::incremental::graft_geometry_with_cascade`].
///
/// Since S24 the live map holds this pass's values, so "what did the box in
/// `prev` get built from" is answered by the displaced entries first and the
/// live map second: a node this pass recomputed is in `replaced`, and every node
/// it reused is unchanged in `current` by definition.
#[derive(Clone, Copy)]
pub struct PrevCascade<'a> {
    /// This pass's cache — correct for every node the pass did not recompute.
    current: &'a CascadeStyles,
    /// What the pass displaced — correct for every node it did recompute.
    /// `None` when `current` *is* the previous cascade, untouched.
    replaced: Option<&'a HashMap<NodeId, Arc<ComputedStyle>>>,
}

impl<'a> PrevCascade<'a> {
    /// The style the previous pass cascaded for `id`, if it had one.
    ///
    /// Takes `self` by value (the type is [`Copy`]) so the borrow it hands back
    /// is tied to the maps, not to a copy of the two pointers.
    pub fn get(self, id: &NodeId) -> Option<&'a Arc<ComputedStyle>> {
        self.replaced.and_then(|r| r.get(id)).or_else(|| self.current.get(id))
    }

    /// Whether the previous pass cascaded a style for `id` at all.
    ///
    /// The distinction matters to the graft's last-resort probe for anonymous
    /// boxes, which are keyed by a text node the cascade never visits.
    pub fn contains_key(&self, id: &NodeId) -> bool {
        self.replaced.is_some_and(|r| r.contains_key(id)) || self.current.contains_key(id)
    }

    /// A view over a cascade the current pass has not displaced anything from.
    ///
    /// For callers holding the previous pass's cache directly rather than the
    /// [`CounterMap`] that rewrote it.
    pub fn unchanged(current: &'a CascadeStyles) -> Self {
        Self { current, replaced: None }
    }
}

/// Document-order snapshot of CSS generated-content state.
///
/// Holds both the per-element counter snapshots (after own reset/increment,
/// before children — for `counter()` / `counters()`) and the quote-nesting
/// depth index for each `open-quote` / `close-quote` occurrence in document
/// order (for the `quotes` property).
#[derive(Default)]
pub struct CounterMap {
    /// `NodeId` → counter snapshot used by `counter()` / `counters()`.
    nodes: HashMap<NodeId, CounterSnapshot>,
    /// `(NodeId, slot)` → ordered list of quote-nesting depth indices, one per
    /// `open-quote` / `close-quote` item in that slot's `content` (in order).
    /// `no-open-quote` / `no-close-quote` adjust depth but emit no entry.
    quotes: HashMap<(NodeId, QuoteSlot), Vec<usize>>,
    /// BUG-284: `NodeId` → the `ComputedStyle` this traversal already computed
    /// for it. `build_box` walks the same document in the same order with the
    /// same `inherited` chain (pure function of doc/sheet/viewport/dark_mode),
    /// so its own `compute_style(id, ...)` call would recompute an identical
    /// result — reusing this cache skips a second full cascade pass per node.
    ///
    /// BUG-341 S9: behind an [`Arc`] because this map has three consumers that
    /// each used to deep-copy a whole `ComputedStyle` per node — the incremental
    /// cascade's reuse path (`walk` below), the per-cycle snapshot the pipeline
    /// keeps for the next delta, and this map's own insert. All three are now
    /// refcount bumps. Nothing mutates a cached style in place, so the sharing
    /// is unobservable.
    ///
    /// BUG-341 S24: and the map itself is now *carried* from pass to pass
    /// ([`CascadeStyles`]) rather than rebuilt — see that type's doc comment.
    styles: CascadeStyles,
    /// BUG-341 S24 — for every entry this pass overwrote, the value it replaced.
    ///
    /// Carrying `styles` across passes means a recomputed node's *previous*
    /// style is no longer sitting in a separate map for
    /// [`crate::incremental::graft_geometry_with_cascade`] to compare against:
    /// the entry now holds this pass's value, and the freshly built box holds
    /// the very same allocation, so the graft's pointer test would report
    /// "unchanged" for exactly the nodes whose style *did* change — a box kept
    /// at last pass's geometry under a new style, i.e. visible corruption.
    ///
    /// So the pass records what it displaced instead of preserving a whole copy
    /// of what it did not (the S22 shape): one entry per recomputed node — one
    /// on a keystroke cycle, none on a hover flip — read through
    /// [`CounterMap::prev_cascade`]. Dropped with this `CounterMap`; only
    /// `styles` travels on.
    replaced_styles: HashMap<NodeId, Arc<ComputedStyle>>,
    /// BUG-341 S4: `NodeId`s whose own `ComputedStyle` AND entire descendant
    /// subtree are byte-identical to the previous pass (`walk`'s
    /// `must_recompute` was `false` for every node in the subtree), and
    /// (BUG-341 S16) which contain no content-mutated node per
    /// [`RestyleDelta::content_dirty`]. Populated only when that record is
    /// complete ([`ContentDirty::tracked`]). `build_box_or_reuse` (in
    /// `box_tree.rs`) treats membership as a licence to clone the whole
    /// `LayoutBox` subtree from the previous pass instead of rebuilding it —
    /// see that function's doc comment for why content dirtiness (not style
    /// equality alone) is the correctness precondition.
    clean_subtrees: HashSet<NodeId>,
}

impl CounterMap {
    /// An empty map sized for a document of `elements` styled elements.
    ///
    /// BUG-341 S23: `styles` and `clean_subtrees` take one entry per element
    /// and are built from scratch every pass, so a default-capacity map rehashes
    /// its way up to ~830 entries — moving roughly as many entries again as it
    /// finally holds. The two counter-related maps are left at zero: a document
    /// without counters or quotes never inserts into them at all.
    fn with_capacity(elements: usize) -> Self {
        Self {
            styles: CascadeStyles::with_capacity(elements),
            clean_subtrees: HashSet::with_capacity(elements),
            ..Self::default()
        }
    }

    /// BUG-341 S26 — the map a walk over an unchanged document would have
    /// produced, without walking it. See [`incremental_precompute_counters`].
    ///
    /// `clean_subtrees` holds the document root's **element** children and
    /// nothing else, which is not a shortcut but the exact outcome: a walk would
    /// put every one of the document's elements in the set, and the set's only
    /// consumer, [`crate::incremental::extract_clean_subtrees`], stops at the
    /// topmost member it meets and moves that whole subtree. On any document
    /// that is `<html>`. Non-elements are excluded for the same reason the walk
    /// excludes them: it records a node as clean only on its element branch, so
    /// a doctype or comment child of the root is not in the set a walk would
    /// build, and admitting one here would hand the reuse index a box the
    /// mechanism has never had to handle.
    ///
    /// `nodes`, `quotes` and `replaced_styles` stay empty: the first two because
    /// the licence for this path is that the previous pass produced none, the
    /// third because nothing was recomputed, so nothing was displaced.
    fn carried_unchanged(doc: &Document, flat: &FlatTree, styles: CascadeStyles) -> Self {
        let roots = flat.children_of(doc, doc.root());
        let mut clean_subtrees = HashSet::with_capacity(roots.len());
        for &child in roots {
            if matches!(doc.get(child).data, NodeData::Element { .. }) {
                clean_subtrees.insert(child);
            }
        }
        Self { styles, clean_subtrees, ..Self::default() }
    }

    /// BUG-341 S24 — a map that continues the carried cascade cache `styles`
    /// instead of starting a new one.
    fn continuing(mut styles: CascadeStyles, elements: usize) -> Self {
        styles.begin_pass();
        Self {
            styles,
            clean_subtrees: HashSet::with_capacity(elements),
            ..Self::default()
        }
    }

    /// Returns the counter snapshot for `id`, if any.
    pub fn counters(&self, id: NodeId) -> Option<&CounterSnapshot> {
        self.nodes.get(&id)
    }

    /// Returns the ordered quote-depth indices for the given `(id, slot)`'s
    /// generated content. Empty slice when the slot has no quote items.
    pub fn quote_depths(&self, id: NodeId, slot: QuoteSlot) -> &[usize] {
        self.quotes.get(&(id, slot)).map_or(&[][..], |v| v.as_slice())
    }

    /// Returns the `ComputedStyle` this map's traversal computed for `id`, if
    /// any (BUG-284 cascade-reuse cache — see the `styles` field).
    pub fn style_for(&self, id: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(&id).map(|s| &**s)
    }

    /// BUG-341 S24 — the cascade as the *previous* pass left it, for the graft.
    ///
    /// See [`Self::replaced_styles`] for why this is not simply [`Self::styles`]
    /// any more.
    pub fn prev_cascade(&self) -> PrevCascade<'_> {
        PrevCascade { current: &self.styles, replaced: Some(&self.replaced_styles) }
    }

    /// What this pass displaced from the carried cache (BUG-341 S24): for every
    /// node it recomputed, the style the previous pass had cascaded for it.
    ///
    /// The graft reads it through [`Self::prev_cascade`]; exposed on its own so
    /// a census can ask the question it is the exact answer to — "of the nodes
    /// that re-cascaded, how many really ended up with a different style".
    pub fn replaced_styles(&self) -> &HashMap<NodeId, Arc<ComputedStyle>> {
        &self.replaced_styles
    }

    /// BUG-341 S26 — stamp the carried cache with whether this pass produced any
    /// generated content, so a later pass knows whether it may skip its walk.
    ///
    /// Called by both cascade entry points, never by the no-op path itself: that
    /// path did not walk, so it has nothing new to say and must leave the fact
    /// exactly as the pass that did walk left it.
    fn record_generated_content(&mut self) {
        let any = !self.nodes.is_empty() || !self.quotes.is_empty();
        self.styles.note_generated_content(any);
    }

    /// Hand the carried cascade cache on to the next pass (BUG-341 S24).
    ///
    /// The pipeline persists this as the next cycle's
    /// [`RestyleDelta::prev_styles`]; it used to clone the whole map to do so.
    pub fn into_styles(self) -> CascadeStyles {
        self.styles
    }

    /// Like [`Self::style_for`], but hands back the shared allocation itself.
    ///
    /// BUG-341 S12: `build_box` stores the cascade result straight into
    /// `LayoutBox::style` (now an `Arc<ComputedStyle>`), so the cache entry can
    /// be shared rather than deep-copied per node — a 3.2 KB struct with ~30
    /// heap fields, built once per node and previously copied once more into
    /// every box. The few build-time rewrites (image intrinsic hints, marker
    /// styles) take their copy via `Arc::make_mut`.
    pub fn style_arc(&self, id: NodeId) -> Option<Arc<ComputedStyle>> {
        self.styles.get(&id).cloned()
    }

    /// Returns the full per-node `ComputedStyle` cascade cache (BUG-341 S2).
    ///
    /// The map the full cascade produced, keyed by `NodeId`. It is the reference
    /// the incremental cascade (BUG-341 S3+) must reproduce bit-for-bit; the
    /// `incr == full` differential tests compare two such maps directly.
    pub fn styles(&self) -> &CascadeStyles {
        &self.styles
    }

    /// How many per-node counter snapshots this map actually stores
    /// (BUG-341 S23 census hook).
    ///
    /// Not derivable from [`Self::styles`]'s size any more: a node whose
    /// counter stacks are empty stores nothing, so on a document with no
    /// counters this is zero while `styles` holds one entry per element. A
    /// census that assumes the two match would keep reporting the cost this
    /// slice removed.
    pub fn counter_snapshot_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the whole-subtree-unchanged node set (BUG-341 S4) — see the
    /// `clean_subtrees` field doc for the correctness precondition
    /// (`RestyleDelta::content_dirty`) that gates population.
    pub fn clean_subtrees(&self) -> &HashSet<NodeId> {
        &self.clean_subtrees
    }
}

/// Mutable state threaded through the pre-order DOM traversal.
#[derive(Default)]
struct CounterCtx {
    /// name → stack of scope values (innermost = last).
    stacks: HashMap<String, Vec<i32>>,
    /// Running quote-nesting depth in document order (CSS Content L3 §3.2).
    quote_depth: usize,
    /// BUG-341 S10 — whether the stylesheet can produce quote content at all
    /// (`style::sheet_has_quote_content`, decided once per pass). When false,
    /// `walk` skips its per-node `::before`/`::after` probe: that probe exists
    /// only to advance `quote_depth`, and without a quote anywhere in the sheet
    /// it can never record one. Lives here rather than as a `walk` parameter
    /// because it is per-pass state like the counter stacks themselves.
    quotes_possible: bool,
}

impl CounterCtx {
    /// Push new scope(s) from `counter-reset`. Returns the list of names that were reset
    /// so the caller can pop them later.
    fn apply_reset(&mut self, resets: &[(String, i32)]) {
        for (name, val) in resets {
            self.stacks.entry(name.clone()).or_default().push(*val);
        }
    }

    /// Increment top-of-stack for each entry in `counter-increment`. Auto-creates a
    /// counter with value 0 if it has never been reset.
    fn apply_increment(&mut self, increments: &[(String, i32)]) {
        for (name, val) in increments {
            let stack = self.stacks.entry(name.clone()).or_default();
            if stack.is_empty() {
                stack.push(0);
            }
            *stack.last_mut().unwrap() += val;
        }
    }

    /// Set top-of-stack to the given value for each entry in `counter-set`
    /// (CSS Lists L3 §4). Auto-creates the counter (with the set value as its
    /// only scope) if it has never been reset — `counter-set` on a non-existent
    /// counter acts as though it created the counter.
    fn apply_set(&mut self, sets: &[(String, i32)]) {
        for (name, val) in sets {
            let stack = self.stacks.entry(name.clone()).or_default();
            if stack.is_empty() {
                stack.push(*val);
            } else {
                *stack.last_mut().unwrap() = *val;
            }
        }
    }

    /// Snapshot the current stacks for this node.
    fn snapshot(&self) -> CounterSnapshot {
        self.stacks.clone()
    }

    /// Pop the scopes that were pushed by `apply_reset` for the given reset list.
    fn pop_reset(&mut self, resets: &[(String, i32)]) {
        for (name, _) in resets {
            if let Some(stack) = self.stacks.get_mut(name) {
                stack.pop();
                if stack.is_empty() {
                    self.stacks.remove(name);
                }
            }
        }
    }
}

/// Build a `CounterMap` by walking the DOM in pre-order.
///
/// Each element's snapshot captures counter state after its own `counter-reset`
/// and `counter-increment`, before any children are processed. This is the correct
/// state for resolving `counter()` in `::before` content.
/// Precomputes CSS counter values for the entire document tree.
/// `dark_mode` is forwarded to `@media (prefers-color-scheme: dark)` matching
/// during style computation so counter-related styles resolve correctly.
pub fn precompute_counters(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    flat: &FlatTree,
    dark_mode: bool,
) -> CounterMap {
    let root_style = ComputedStyle::root();
    let mut ctx = CounterCtx {
        quotes_possible: crate::style::sheet_has_quote_content(sheet, viewport, dark_mode),
        ..CounterCtx::default()
    };
    let mut map = CounterMap::with_capacity(doc.node_count());
    walk(doc, sheet, doc.root(), &root_style, viewport, flat, &mut ctx, &mut map, dark_mode, None, false);
    map.record_generated_content();
    map
}

/// BUG-341 S3 — incremental-cascade root-set + reuse cache (brief §3/§4).
///
/// Passed to [`incremental_precompute_counters`]. Every node in `dirty_roots`,
/// and every descendant reached from it via [`FlatTree::children_of`] (i.e.
/// its whole subtree), gets a fresh `compute_style` call; every other node
/// reuses its entry from `prev_styles` verbatim. A node absent from
/// `prev_styles` (freshly inserted since the snapshot was taken) always
/// recomputes even when not in `dirty_roots`, since there is nothing to reuse.
///
/// Callers derive `dirty_roots` conservatively via
/// [`crate::style::restyle_root_set_for_state_change`] (interactive-state
/// transitions) or [`crate::style::restyle_root_set_for_node_change`] (DOM
/// attribute/class/structural changes) — see brief §4 for the invalidation
/// model these implement.
pub struct RestyleDelta<'a> {
    /// The previous cascade's per-node style cache — [`CounterMap::styles`]
    /// from an earlier full or incremental pass over the same document.
    ///
    /// BUG-341 S24: **moved in**, not borrowed. The pass rewrites the entries it
    /// recomputes in place and hands the same map back inside its
    /// [`CounterMap`]; a caller that needs the previous cascade afterwards
    /// reads it through [`CounterMap::prev_cascade`], which also knows what this
    /// pass displaced.
    pub prev_styles: CascadeStyles,
    /// Root nodes whose entire subtree must be re-cascaded.
    pub dirty_roots: HashSet<NodeId>,
    /// BUG-341 S16: which nodes had their *content* — the things `build_box`
    /// reads that the cascade cannot see (text-node data, child lists,
    /// attributes) — mutated since `prev_styles` was taken. See
    /// [`ContentDirty`] for the three states and their contracts.
    ///
    /// Replaces S4's document-wide `dom_content_stable: bool`. That flag was
    /// all-or-nothing: one changed omnibox text node disabled box reuse for
    /// every one of chrome's 318 boxes, including the ~300 the mutation came
    /// nowhere near.
    ///
    /// One known residual gap, unchanged from S4: a CSS rule that conditions
    /// `counter-reset`/`counter-increment`/`counter-set` on a dynamic pseudo-
    /// class (e.g. `li:hover { counter-increment: item 2; }`) could change a
    /// *later, otherwise-clean* sibling's rendered counter text without
    /// changing that sibling's own `ComputedStyle`. This is not exercised
    /// anywhere in this codebase's CSS and is accepted as an unhandled exotic
    /// edge case for v1 (see BUG-341 "S4" notes) rather than blocking the
    /// common case on a full counter-snapshot equality check.
    pub content_dirty: ContentDirty<'a>,
}

impl RestyleDelta<'_> {
    /// BUG-341 S26 — whether this delta states that nothing at all changed.
    ///
    /// Not "nothing much": every one of the three inputs the cascade reads must
    /// positively say so. An empty root set means no node's selector match can
    /// have flipped; [`ContentDirty::nothing_changed`] means no node's text,
    /// children or attributes moved; and a non-empty carried cache means there
    /// is a previous cascade to keep at all (the first incremental pass after a
    /// full one gets its map from that full pass, so this is only false when
    /// there is genuinely nothing to reuse).
    ///
    /// When it holds, `walk` provably does nothing but restate its input: every
    /// element takes the reuse branch, every node reports itself clean, and the
    /// map it hands back has the same styles it was given. See
    /// [`incremental_precompute_counters`] for why that is *equivalent* to not
    /// walking rather than merely similar.
    fn is_noop(&self) -> bool {
        self.dirty_roots.is_empty()
            && self.content_dirty.nothing_changed()
            && !self.prev_styles.is_empty()
            && !self.prev_styles.generated_content()
    }
}

/// BUG-341 S16 — per-node DOM-content dirtiness for one incremental cycle.
///
/// The cascade decides whether a node's *style* is unchanged. It cannot decide
/// whether the node's *content* is unchanged: a text node's data, an element's
/// child list and its attributes are all invisible to `compute_style` reuse,
/// yet `build_box` reads all three. This enum is how a mutation source tells
/// [`incremental_precompute_counters`] what it knows about that second half,
/// and it is the sole licence for [`CounterMap::clean_subtrees`] — hence for
/// `build_box_or_reuse` cloning a whole `LayoutBox` subtree.
///
/// A missed mutation here is *visible corruption* (a stale box), not a slow
/// frame, so a source that cannot enumerate its own content mutations must say
/// [`ContentDirty::Untracked`] rather than guess.
#[derive(Debug, Clone, Copy)]
pub enum ContentDirty<'a> {
    /// No per-node content tracking for this cycle: any node's text, children
    /// or attributes may have changed. No subtree may be reused
    /// (`clean_subtrees` stays empty). This is S4's `dom_content_stable: false`
    /// and remains the correct answer for every mutation source that does not
    /// enumerate content mutations — today, page-side JS (`DomTouched` reports
    /// selector-relevant nodes only, plus an `unattributed` escape hatch).
    Untracked,
    /// Tracking is complete and nothing changed: a pure interactive-state
    /// (`:hover`/`:focus`/`:active`) transition, which by construction never
    /// touches the DOM. This is S4's `dom_content_stable: true`.
    Nothing,
    /// Tracking is complete and exactly these nodes were content-mutated. A
    /// subtree is reusable iff it contains none of them (and its styles were
    /// all reused). The set is *inclusive of the mutated node itself*: for a
    /// changed text node that is the text node's own id, for a container that
    /// gained or lost a child that is the container's id.
    ///
    /// Producing this variant obliges the caller to have instrumented **every**
    /// path by which it mutates the document — see
    /// `lumen_chrome::model`'s tracked-primitive wrappers and the
    /// `every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive`
    /// source-level gate that enforces it.
    Nodes(&'a HashSet<NodeId>),
}

impl ContentDirty<'_> {
    /// Whether this cycle has a complete per-node content-mutation record, and
    /// therefore may populate [`CounterMap::clean_subtrees`] at all.
    pub fn tracked(&self) -> bool {
        !matches!(self, ContentDirty::Untracked)
    }

    /// Whether `id`'s own content may have changed this cycle. `true` for
    /// every node under [`ContentDirty::Untracked`] — "unknown" must read as
    /// "dirty" everywhere the answer is used.
    pub fn contains(&self, id: NodeId) -> bool {
        match self {
            ContentDirty::Untracked => true,
            ContentDirty::Nothing => false,
            ContentDirty::Nodes(set) => set.contains(&id),
        }
    }

    /// BUG-341 S26 — whether this record positively states that *no* node's
    /// content changed. [`ContentDirty::Untracked`] is never that: "unknown"
    /// reads as "dirty" here as everywhere else.
    pub fn nothing_changed(&self) -> bool {
        match self {
            ContentDirty::Untracked => false,
            ContentDirty::Nothing => true,
            ContentDirty::Nodes(set) => set.is_empty(),
        }
    }
}

/// BUG-341 S3 — incremental cascade: like [`precompute_counters`], but reuses
/// `delta.prev_styles` for every node outside `delta.dirty_roots` (and their
/// subtrees) instead of recomputing `compute_style`.
///
/// Must reproduce [`precompute_counters`]'s output bit-for-bit for the same
/// final document state (brief §4 "Correctness gate") — the
/// `incr_cascade_matches_full_*` differential tests in `incremental.rs` guard
/// this. Any divergence means `dirty_roots` under-approximated the change,
/// not an acceptable trade-off.
pub fn incremental_precompute_counters(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    flat: &FlatTree,
    dark_mode: bool,
    delta: RestyleDelta<'_>,
) -> CounterMap {
    if !incremental_restyle_enabled() {
        // Flag off (the default): behave exactly like a full cascade so a
        // caller can wire this entry point in ahead of the flag ever being
        // flipped on (S5) without changing behaviour. BUG-341 S24: the carried
        // cache still has to reach the graft as "what `prev` was built from" —
        // a full pass displaces the whole of it, which is exactly what
        // `replaced_styles` means.
        let mut map = precompute_counters(doc, sheet, viewport, flat, dark_mode);
        map.replaced_styles = delta.prev_styles.into_plain();
        return map;
    }
    // BUG-341 S26 — the delta says nothing changed, so the walk would restate
    // its own input.
    //
    // This is not a heuristic and adds no trust the pass did not already place
    // in the delta. Feed `walk` an empty `dirty_roots` and a content record that
    // names nobody and every branch is forced: `force` starts false and can only
    // be set by `must_recompute`, which can only be set by a miss in the carried
    // cache — impossible for a node the previous pass wrote — so every element
    // takes the reuse branch, every node reports itself clean, and the map handed
    // back holds the same styles it was given, restamped. The one output that is
    // *not* a restatement is `nodes`/`quotes`, which the walk rebuilds rather
    // than carries; hence the `generated_content` half of the licence.
    //
    // Not walking also leaves the pass ordinal where it was, which is exactly
    // right: `CascadeStyles`' invariant is "an entry exists iff the immediately
    // preceding pass *visited* that node", and this pass visited nobody. The
    // next pass to walk therefore still sees its entries as one pass old.
    if delta.is_noop() {
        let _prof = lumen_core::profile::scope("cascade_noop");
        return CounterMap::carried_unchanged(doc, flat, delta.prev_styles);
    }
    let root_style = ComputedStyle::root();
    // BUG-341 S26: the stage's three parts are scoped separately because only
    // the middle one is proportional to the delta — the prologue is sheet-wide
    // and the epilogue is proportional to the *document*. A stage total cannot
    // tell "the cascade is expensive" from "the bookkeeping around it is".
    let (mut ctx, mut map, incr) = {
        let _prof = lumen_core::profile::scope("cascade_prologue");
        let ctx = CounterCtx {
            quotes_possible: crate::style::sheet_has_quote_content(sheet, viewport, dark_mode),
            ..CounterCtx::default()
        };
        // BUG-341 S23: the previous pass's cache is the best available estimate of
        // how many entries this one will hold — closer than `node_count`, which
        // counts text and comment nodes too. S24: and it *is* this pass's cache.
        let elements = delta.prev_styles.len();
        let RestyleDelta { prev_styles, dirty_roots, content_dirty } = delta;
        let map = CounterMap::continuing(prev_styles, elements);
        (ctx, map, IncrRestyle { dirty_roots, content_dirty })
    };
    {
        let _prof = lumen_core::profile::scope("cascade_walk");
        walk(doc, sheet, doc.root(), &root_style, viewport, flat, &mut ctx, &mut map, dark_mode, Some(&incr), false);
    }
    {
        let _prof = lumen_core::profile::scope("cascade_finish_pass");
        map.styles.finish_pass();
    }
    map.record_generated_content();
    map
}

/// BUG-341 S24 — what `walk` still needs from a [`RestyleDelta`] once its
/// `prev_styles` have been moved into the map being written.
///
/// Splitting the delta this way is what lets the reuse path be a single
/// `get_mut` on the map: the "is there anything to reuse" question and the reuse
/// itself are now one lookup into one collection, instead of two lookups into a
/// borrowed map plus an insert into a fresh one.
struct IncrRestyle<'a> {
    /// See [`RestyleDelta::dirty_roots`].
    dirty_roots: HashSet<NodeId>,
    /// See [`RestyleDelta::content_dirty`].
    content_dirty: ContentDirty<'a>,
}

thread_local! {
    /// BUG-341 S3 — master on/off switch for the incremental cascade
    /// (`incremental_precompute_counters`), mirroring
    /// `box_tree::INCREMENTAL_LAYOUT_MODE`'s pattern. Off by default: no
    /// current pipeline entry point (`layout_measured_hyp`,
    /// `layout_mutation_incremental`) consults it yet — that wiring is S5.
    /// Exists now so `RestyleDelta`-based callers (differential tests today,
    /// pipeline code in S5) have a single well-known kill switch.
    static INCREMENTAL_RESTYLE: Cell<bool> = const { Cell::new(false) };
}

/// Enables/disables the incremental cascade for subsequent
/// [`incremental_precompute_counters`] calls on the current thread.
pub fn set_incremental_restyle(enabled: bool) {
    INCREMENTAL_RESTYLE.with(|c| c.set(enabled));
}

/// Whether the incremental cascade is currently enabled on this thread.
pub fn incremental_restyle_enabled() -> bool {
    INCREMENTAL_RESTYLE.with(|c| c.get())
}

/// BUG-341 S3/S17 — per-pass tally of what the cascade stage recomputed versus
/// reused from `RestyleDelta::prev_styles`.
///
/// Same rationale as [`crate::box_tree::BoxBuildStats`]: an incremental cascade
/// that reuses nothing produces exactly the full cascade's output, just slowly
/// (the S8 lesson), so only a counter can tell a working reuse mechanism from a
/// broken one. Public since S17 so gates outside this crate — where the real
/// chrome document lives — can assert on it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CascadeStats {
    /// Elements for which `walk` really called `compute_style`.
    pub recomputed: u32,
    /// Elements that took their `Arc<ComputedStyle>` from `prev_styles`.
    pub reused: u32,
    /// BUG-341 S26 — every node `walk` *entered*, elements and non-elements
    /// alike. `recomputed + reused` counts only elements, so a stage total that
    /// does not divide by them says nothing about the traversal itself: on the
    /// chrome document the walk enters roughly three times as many nodes as it
    /// cascades, and after S14/S17 it re-derives the style of one of them.
    pub visited: u32,
    /// BUG-341 S26 — inserts into [`CounterMap::clean_subtrees`]. One per clean
    /// element, i.e. nearly one per element in the steady state; the set is
    /// rebuilt from scratch every pass.
    pub clean_inserts: u32,
}

thread_local! {
    /// BUG-341 S3/S17 instrumentation, see [`CascadeStats`]. Always compiled —
    /// two `Cell` bumps against a stage that costs microseconds per node.
    ///
    /// `walk` is a plain recursion on the calling thread (unlike `build_box`,
    /// which fans out over rayon workers — S15's trap), so a thread-local tally
    /// here really does see the whole document.
    static CASCADE_STATS: Cell<CascadeStats> = const {
        Cell::new(CascadeStats { recomputed: 0, reused: 0, visited: 0, clean_inserts: 0 })
    };
}

/// Returns the accumulated [`CascadeStats`] and resets the tally.
pub fn take_cascade_stats() -> CascadeStats {
    CASCADE_STATS.with(|s| s.replace(CascadeStats::default()))
}

fn note_cascade(recomputed: bool) {
    CASCADE_STATS.with(|s| {
        let mut v = s.get();
        if recomputed {
            v.recomputed += 1;
        } else {
            v.reused += 1;
        }
        s.set(v);
    });
}

/// BUG-341 S26 — one node entered by `walk`, of any kind.
fn note_visit() {
    CASCADE_STATS.with(|s| {
        let mut v = s.get();
        v.visited += 1;
        s.set(v);
    });
}

/// BUG-341 S26 — one insert into `CounterMap::clean_subtrees`.
fn note_clean_insert() {
    CASCADE_STATS.with(|s| {
        let mut v = s.get();
        v.clean_inserts += 1;
        s.set(v);
    });
}

/// CSS Generated Content L3 §3.2 — record the quote-nesting depth for each
/// `open-quote` / `close-quote` item in `content`, advancing `depth` in document
/// order. `open-quote` uses the current depth then increments; `close-quote`
/// decrements (clamped) then uses the result; the `no-*` variants only adjust
/// depth. Returns the per-item depth indices (one per open/close-quote).
fn record_quote_depths(content: &Content, depth: &mut usize) -> Vec<usize> {
    let Content::Items(items) = content else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        match item {
            ContentItem::OpenQuote => {
                out.push(*depth);
                *depth += 1;
            }
            ContentItem::CloseQuote => {
                *depth = depth.saturating_sub(1);
                out.push(*depth);
            }
            ContentItem::NoOpenQuote => *depth += 1,
            ContentItem::NoCloseQuote => *depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn walk(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    ctx: &mut CounterCtx,
    map: &mut CounterMap,
    dark_mode: bool,
    // BUG-341 S3: `None` = full cascade (every node recomputes, today's
    // behaviour, byte-for-byte unchanged). `Some` = incremental cascade —
    // nodes outside `force`/`dirty_roots` reuse `prev_styles` instead of
    // calling `compute_style`. See `RestyleDelta`.
    incr: Option<&IncrRestyle<'_>>,
    // Set once an ancestor (or this node itself, via `dirty_roots`) forced a
    // recompute — propagates down the rest of the subtree unconditionally
    // (brief §4: "root-set and their style-descendants").
    force: bool,
    // BUG-341 S4: returns `true` when this node's own style AND its entire
    // descendant subtree are unchanged from `prev_styles` (vacuously `true`
    // for non-element nodes, which carry no style of their own). Aggregated
    // bottom-up into `map.clean_subtrees` — see that field's doc comment.
) -> bool {
    note_visit();
    match &doc.get(id).data {
        // Text / comment / doctype / fragment — no counter properties, no children.
        // BUG-341 S4/S16: a text node carries no style, so the cascade has
        // nothing to compare; its *content* can still change under a stable
        // style. Before S16 that was covered document-wide by
        // `dom_content_stable`; now the delta names the individual mutated text
        // nodes, and a named one reports itself dirty so its parent element
        // (whose box embeds this text) drops out of `clean_subtrees`.
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. }
        | NodeData::ShadowRoot { .. } | NodeData::DocumentFragment => {
            return !incr.is_some_and(|d| d.content_dirty.contains(id));
        }

        // Document node: has no style of its own; just recurse into children.
        NodeData::Document => {
            let mut all_clean = true;
            for &child_id in flat.children_of(doc, id) {
                all_clean &= walk(doc, sheet, child_id, inherited, viewport, flat, ctx, map, dark_mode, incr, force);
            }
            return all_clean;
        }

        NodeData::Element { .. } => {} // handled below
    }

    // BUG-341 S9: a refcount bump, not a `ComputedStyle` deep copy — this is
    // the reuse path, taken for every node outside the dirty root set.
    // BUG-341 S24: and it reads *and* restamps the carried cache in one lookup,
    // where S3-S23 asked the previous map twice and then inserted into a fresh
    // one. `None` here means "nothing to reuse" for either reason — no entry, or
    // one the immediately preceding pass did not write (see
    // `CascadeStyles::reuse`).
    let reused = match incr {
        None => None,
        Some(delta) if force || delta.dirty_roots.contains(&id) => None,
        Some(_) => map.styles.reuse(id),
    };
    let must_recompute = reused.is_none();
    note_cascade(must_recompute);
    let style: Arc<ComputedStyle> = match reused {
        Some(style) => style,
        None => {
            let style = Arc::new(compute_style(doc, id, sheet, inherited, viewport, dark_mode));
            // BUG-284: cache so `build_box`'s own `compute_style(id, ...)` call
            // (same doc/sheet/viewport/dark_mode, same `inherited` chain) can
            // reuse this result instead of recomputing an identical cascade.
            // BUG-341 S24: keep whatever this displaced — the graft still needs
            // to see the style `prev`'s box was built from (`replaced_styles`).
            if let Some(displaced) = map.styles.write(id, Arc::clone(&style)) {
                map.replaced_styles.insert(id, displaced);
            }
            style
        }
    };

    // CSS Lists L3 §4: counter-reset first, then counter-increment, then counter-set.
    ctx.apply_reset(&style.counter_reset);
    ctx.apply_increment(&style.counter_increment);
    ctx.apply_set(&style.counter_set);

    // BUG-341 S23: an empty snapshot is indistinguishable from no snapshot at
    // all — both consumers reach it through `snap.and_then(|s| s.get(name))`
    // (`content_to_inline_segments`, `content_items_to_string`) — so a document
    // with no counters in scope need not carry one entry per element. That is
    // every document that never uses `counter-reset`/`-increment`/`-set`,
    // including Lumen's own chrome: 828 inserts of an empty `HashMap` per
    // cycle. Exact rather than a sheet-wide predicate: the check is on the live
    // counter stacks, so a document that uses counters in one subtree still
    // skips the entries outside it.
    if !ctx.stacks.is_empty() {
        map.nodes.insert(id, ctx.snapshot());
    }

    // CSS Generated Content L3 §3.2 — ::before quotes are in document order
    // before the element's children; advance and snapshot the quote depth.
    // Always run (even when `style` was reused) — `quote_depth` is a running
    // document-order counter that must stay continuous regardless of which
    // nodes recomputed their style.
    // BUG-341 S10: `quotes_possible` is the sheet-wide "could any declaration
    // produce quote content" flag — when it is false this probe cannot record a
    // depth, and skipping it removes two pseudo-element cascades per node
    // (measured: 79% of `precompute_counters` on the CC12_KEY cycle, where only
    // a dozen nodes recompute their own style but all 828 were probed).
    if ctx.quotes_possible {
        let _prof = lumen_core::profile::scope_detail("quote_pseudos");
        if let Some(bps) =
            compute_pseudo_element_style(doc, id, "before", sheet, &style, viewport, dark_mode)
        {
            let depths = record_quote_depths(&bps.content, &mut ctx.quote_depth);
            if !depths.is_empty() {
                map.quotes.insert((id, QuoteSlot::Before), depths);
            }
        }
    }

    let child_force = force || must_recompute;
    let mut children_clean = true;
    for &child_id in flat.children_of(doc, id) {
        let child_clean = walk(doc, sheet, child_id, &style, viewport, flat, ctx, map, dark_mode, incr, child_force);
        children_clean &= child_clean;
    }

    // ::after quotes come after all children in document order.
    if ctx.quotes_possible {
        let _prof = lumen_core::profile::scope_detail("quote_pseudos");
        if let Some(aps) =
            compute_pseudo_element_style(doc, id, "after", sheet, &style, viewport, dark_mode)
        {
            let depths = record_quote_depths(&aps.content, &mut ctx.quote_depth);
            if !depths.is_empty() {
                map.quotes.insert((id, QuoteSlot::After), depths);
            }
        }
    }

    ctx.pop_reset(&style.counter_reset);

    // BUG-341 S4/S16: this node's own style was reused (not recomputed), its
    // own content was not mutated, AND every descendant is itself clean. Only
    // recorded when the delta has a complete content record at all — see
    // `ContentDirty`.
    let content_dirty = incr.is_some_and(|d| d.content_dirty.contains(id));
    let subtree_clean = !must_recompute && !content_dirty && children_clean;
    if subtree_clean && incr.is_some_and(|d| d.content_dirty.tracked()) {
        map.clean_subtrees.insert(id);
        note_clean_insert();
    }
    subtree_clean
}

// ─── Counter value formatting ────────────────────────────────────────────────

/// Format a counter integer value according to the given `list-style-type` keyword.
///
/// Supported styles: `decimal` (default), `lower-alpha` / `lower-latin`,
/// `upper-alpha` / `upper-latin`, `lower-roman`, `upper-roman`, `lower-greek`,
/// `disc`, `circle`, `square`, `none`. Unrecognised styles fall back to `decimal`.
pub fn format_counter(val: i32, style: &str) -> String {
    match style.trim() {
        "none" => String::new(),
        "lower-alpha" | "lower-latin" => alpha_counter(val, false),
        "upper-alpha" | "upper-latin" => alpha_counter(val, true),
        "lower-roman" => roman_counter(val, false),
        "upper-roman" => roman_counter(val, true),
        "lower-greek" => greek_counter(val),
        "disc" => "\u{2022}".to_string(),
        "circle" => "\u{25E6}".to_string(),
        "square" => "\u{25AA}".to_string(),
        // "decimal" and everything else:
        _ => val.to_string(),
    }
}

fn alpha_counter(n: i32, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let mut n = n as u32;
    let mut result = Vec::new();
    loop {
        n -= 1;
        let ch = (b'a' + (n % 26) as u8) as char;
        result.push(if upper { ch.to_ascii_uppercase() } else { ch });
        n /= 26;
        if n == 0 {
            break;
        }
    }
    result.iter().rev().collect()
}

fn roman_counter(n: i32, upper: bool) -> String {
    if n <= 0 || n > 3999 {
        return n.to_string();
    }
    const VALS: &[(u32, &str)] = &[
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"),
        (100,  "c"), (90,  "xc"), (50,  "l"), (40,  "xl"),
        (10,   "x"), (9,   "ix"), (5,   "v"), (4,   "iv"),
        (1,    "i"),
    ];
    let mut n = n as u32;
    let mut out = String::new();
    for &(v, s) in VALS {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    if upper { out.to_uppercase() } else { out }
}

fn greek_counter(n: i32) -> String {
    const GREEK: &[char] = &['α','β','γ','δ','ε','ζ','η','θ','ι','κ','λ','μ',
                              'ν','ξ','ο','π','ρ','σ','τ','υ','φ','χ','ψ','ω'];
    if n <= 0 { return n.to_string(); }
    let idx = ((n as usize) - 1) % GREEK.len();
    GREEK[idx].to_string()
}

// ─── Custom counter styles (CSS Counter Styles L3) ───────────────────────────

/// Numbering algorithm for a `@counter-style` rule — CSS Counter Styles L3 §4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterSystem {
    /// Cycles through symbols repeatedly (§4.2).
    Cyclic,
    /// Positional numeral system using symbols as digits (§4.3).
    Numeric,
    /// Bijective base-N (like spreadsheet columns: A…Z, AA…AZ, …) (§4.4).
    Alphabetic,
    /// Like cyclic but each symbol repeats more times per pass (§4.5).
    Symbolic,
    /// Weighted sum, like roman numerals (§4.6).
    Additive,
    /// Finite range starting at `first` value (§4.1).
    Fixed(i32),
    /// Inherits algorithm + symbols from another counter style (§4.7).
    Extends(String),
}

/// Counter range bound: `None` means ±infinite (CSS Counter Styles L3 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeBound {
    /// Inclusive lower bound, or `None` for −∞.
    pub min: Option<i32>,
    /// Inclusive upper bound, or `None` for +∞.
    pub max: Option<i32>,
}

/// Range descriptor value (CSS Counter Styles L3 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterRange {
    /// System-dependent default range.
    Auto,
    /// Explicit list of inclusive ranges.
    Explicit(Vec<RangeBound>),
}

/// Parsed `@counter-style` rule — CSS Counter Styles L3 §2.
#[derive(Debug, Clone, PartialEq)]
pub struct CounterStyleDef {
    /// Numbering algorithm.
    pub system: CounterSystem,
    /// Symbol list for cyclic/alphabetic/symbolic/numeric/fixed systems.
    pub symbols: Vec<String>,
    /// `(weight, symbol)` pairs for additive system, sorted descending by weight.
    pub additive_symbols: Vec<(i32, String)>,
    /// Prepended to every counter representation.
    pub prefix: String,
    /// Appended to every counter representation (default `"."`).
    pub suffix: String,
    /// Range descriptor controlling when fallback is used.
    pub range: CounterRange,
    /// `(min_length, pad_symbol)` — pad representation to min length.
    pub pad: Option<(i32, String)>,
    /// `(negative_prefix, negative_suffix)` applied when value < 0.
    pub negative: (String, String),
    /// Fallback counter style name (default `"decimal"`).
    pub fallback: String,
}

impl Default for CounterStyleDef {
    fn default() -> Self {
        Self {
            system: CounterSystem::Symbolic,
            symbols: Vec::new(),
            additive_symbols: Vec::new(),
            prefix: String::new(),
            suffix: ".".to_string(),
            range: CounterRange::Auto,
            pad: None,
            negative: ("-".to_string(), String::new()),
            fallback: "decimal".to_string(),
        }
    }
}

/// Maps counter style names to their parsed `CounterStyleDef`.
pub type CounterStyleRegistry = HashMap<String, CounterStyleDef>;

/// Build a `CounterStyleRegistry` from all `@counter-style` rules in a stylesheet.
pub fn build_counter_style_registry(sheet: &Stylesheet) -> CounterStyleRegistry {
    sheet
        .counter_styles
        .iter()
        .map(|rule| (rule.name.clone(), parse_counter_style_descriptors(&rule.declarations)))
        .collect()
}

fn parse_counter_style_descriptors(
    declarations: &[lumen_css_parser::Declaration],
) -> CounterStyleDef {
    let mut def = CounterStyleDef::default();
    for decl in declarations {
        match decl.property.trim().to_ascii_lowercase().as_str() {
            "system" => def.system = parse_counter_system(&decl.value),
            "symbols" => def.symbols = parse_symbols_list(&decl.value),
            "additive-symbols" => def.additive_symbols = parse_additive_symbols(&decl.value),
            "prefix" => def.prefix = parse_single_symbol(&decl.value),
            "suffix" => def.suffix = parse_single_symbol(&decl.value),
            "range" => def.range = parse_counter_range(&decl.value),
            "pad" => def.pad = parse_pad_descriptor(&decl.value),
            "negative" => def.negative = parse_negative_descriptor(&decl.value),
            "fallback" => def.fallback = decl.value.trim().to_string(),
            _ => {}
        }
    }
    def
}

fn parse_counter_system(value: &str) -> CounterSystem {
    let s = value.trim().to_ascii_lowercase();
    if s == "cyclic" {
        CounterSystem::Cyclic
    } else if s == "numeric" {
        CounterSystem::Numeric
    } else if s == "alphabetic" {
        CounterSystem::Alphabetic
    } else if s == "symbolic" {
        CounterSystem::Symbolic
    } else if s == "additive" {
        CounterSystem::Additive
    } else if let Some(rest) = s.strip_prefix("fixed") {
        let start = rest.trim().parse::<i32>().unwrap_or(1);
        CounterSystem::Fixed(start)
    } else if let Some(rest) = s.strip_prefix("extends") {
        CounterSystem::Extends(rest.trim().to_string())
    } else {
        CounterSystem::Symbolic
    }
}

/// Parse a CSS symbol list (space-separated quoted strings or idents).
fn parse_symbols_list(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut symbols = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            i += 1;
            let (s, end) = parse_css_string_from(&chars, i, quote);
            i = end;
            symbols.push(s);
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            if !tok.is_empty() {
                symbols.push(tok);
            }
        }
    }
    symbols
}

/// Parse a single CSS symbol (quoted string or ident).
fn parse_single_symbol(value: &str) -> String {
    let s = value.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        let inner: Vec<char> = s[1..s.len() - 1].chars().collect();
        let (result, _) = parse_css_string_from(&inner, 0, s.chars().next().unwrap());
        result
    } else {
        s.to_string()
    }
}

/// Parse `additive-symbols: <integer> <symbol> (, <integer> <symbol>)*`.
fn parse_additive_symbols(value: &str) -> Vec<(i32, String)> {
    let mut result = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let chars: Vec<char> = part.chars().collect();
        let mut i = 0;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        // Parse leading integer
        let neg = i < chars.len() && chars[i] == '-';
        if neg {
            i += 1;
        }
        let num_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == num_start {
            continue;
        }
        let num_str: String = chars[if neg { num_start - 1 } else { num_start }..i]
            .iter()
            .collect();
        let Ok(num) = num_str.parse::<i32>() else {
            continue;
        };
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let sym_str: String = chars[i..].iter().collect();
        let sym = parse_single_symbol(sym_str.trim());
        result.push((num, sym));
    }
    // Spec requires descending order for the additive algorithm.
    result.sort_unstable_by_key(|&(w, _): &(i32, _)| std::cmp::Reverse(w));
    result
}

/// Parse `range: auto | [ [ <integer> | infinite ]{2} ]#`.
fn parse_counter_range(value: &str) -> CounterRange {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" {
        return CounterRange::Auto;
    }
    let mut bounds = Vec::new();
    for pair in v.split(',') {
        let parts: Vec<&str> = pair.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let min = if parts[0] == "infinite" {
            None
        } else {
            parts[0].parse::<i32>().ok()
        };
        let max = if parts[1] == "infinite" {
            None
        } else {
            parts[1].parse::<i32>().ok()
        };
        bounds.push(RangeBound { min, max });
    }
    if bounds.is_empty() {
        CounterRange::Auto
    } else {
        CounterRange::Explicit(bounds)
    }
}

/// Parse `pad: <integer> <symbol>`.
fn parse_pad_descriptor(value: &str) -> Option<(i32, String)> {
    let chars: Vec<char> = value.trim().chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let num_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == num_start {
        return None;
    }
    let num_str: String = chars[num_start..i].iter().collect();
    let Ok(len) = num_str.parse::<i32>() else {
        return None;
    };
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let sym_str: String = chars[i..].iter().collect();
    let sym = parse_single_symbol(sym_str.trim());
    Some((len, sym))
}

/// Parse `negative: <symbol> <symbol>?` — prefix and optional suffix.
fn parse_negative_descriptor(value: &str) -> (String, String) {
    let v = value.trim();
    // May be one or two space-separated symbols (each can be a quoted string).
    let syms = parse_symbols_list(v);
    let prefix = syms.first().cloned().unwrap_or_else(|| "-".to_string());
    let suffix = syms.get(1).cloned().unwrap_or_default();
    (prefix, suffix)
}

/// Parse a quoted CSS string body (char array starting after the opening quote).
/// Returns (unescaped_string, index_after_closing_quote).
fn parse_css_string_from(chars: &[char], start: usize, quote: char) -> (String, usize) {
    let mut result = String::new();
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        if c == quote {
            i += 1; // skip closing quote
            break;
        }
        if c == '\\' {
            i += 1;
            // Collect up to 6 hex digits
            let mut hex = String::new();
            while i < chars.len() && chars[i].is_ascii_hexdigit() && hex.len() < 6 {
                hex.push(chars[i]);
                i += 1;
            }
            if !hex.is_empty() {
                // Skip optional single whitespace after hex escape (CSS §4.1.3)
                if i < chars.len()
                    && (chars[i] == ' ' || chars[i] == '\t' || chars[i] == '\n'
                        || chars[i] == '\r' || chars[i] == '\x0C')
                {
                    i += 1;
                }
                if let Ok(code) = u32::from_str_radix(&hex, 16)
                    && let Some(ch) = char::from_u32(code)
                {
                    result.push(ch);
                }
            } else if i < chars.len() {
                // Non-hex escape: include character literally
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(c);
            i += 1;
        }
    }
    (result, i)
}

// ─── Counter formatting with registry ────────────────────────────────────────

impl CounterStyleDef {
    /// Check whether `val` is within the effective range for this style.
    fn in_range(&self, val: i32) -> bool {
        match &self.range {
            CounterRange::Auto => self.auto_in_range(val),
            CounterRange::Explicit(bounds) => bounds.iter().any(|b| {
                b.min.is_none_or(|m| val >= m) && b.max.is_none_or(|m| val <= m)
            }),
        }
    }

    fn auto_in_range(&self, val: i32) -> bool {
        match &self.system {
            CounterSystem::Cyclic | CounterSystem::Numeric => true,
            CounterSystem::Alphabetic | CounterSystem::Symbolic => val >= 1,
            CounterSystem::Additive => val >= 0,
            CounterSystem::Fixed(start) => {
                val >= *start
                    && (val as i64 - *start as i64) < self.symbols.len() as i64
            }
            CounterSystem::Extends(_) => true,
        }
    }
}

/// Format a counter value using the registry (custom `@counter-style`) first,
/// then fall back to built-in `format_counter` for standard style names.
pub fn format_counter_with_registry(
    val: i32,
    style_name: &str,
    registry: &CounterStyleRegistry,
) -> String {
    if let Some(def) = registry.get(style_name) {
        format_with_def(val, def, registry, 8)
    } else {
        format_counter(val, style_name)
    }
}

/// Recursively format using a `CounterStyleDef` (depth-limited to avoid cycles).
fn format_with_def(
    val: i32,
    def: &CounterStyleDef,
    registry: &CounterStyleRegistry,
    depth: u32,
) -> String {
    if depth == 0 {
        return val.to_string();
    }

    // Check range; use fallback if out of range.
    if !def.in_range(val) {
        return format_fallback(val, &def.fallback, registry, depth - 1);
    }

    // Step 1: Generate initial token using system algorithm.
    let abs = val.unsigned_abs() as i32;
    let token_opt = match &def.system {
        CounterSystem::Cyclic => {
            format_cyclic(val, &def.symbols)
        }
        CounterSystem::Numeric => {
            format_numeric(abs, &def.symbols)
        }
        CounterSystem::Alphabetic => {
            if val < 1 {
                None
            } else {
                format_alphabetic(val, &def.symbols)
            }
        }
        CounterSystem::Symbolic => {
            if val < 1 {
                None
            } else {
                format_symbolic(val, &def.symbols)
            }
        }
        CounterSystem::Additive => {
            format_additive(val, &def.additive_symbols)
        }
        CounterSystem::Fixed(first) => {
            let idx = val - first;
            if idx < 0 || (idx as usize) >= def.symbols.len() {
                None
            } else {
                Some(def.symbols[idx as usize].clone())
            }
        }
        CounterSystem::Extends(name) => {
            // Delegate entirely to the named base style.
            if let Some(base) = registry.get(name) {
                let s = format_with_def(val, base, registry, depth - 1);
                return s;
            }
            return format_counter(val, name);
        }
    };

    let Some(mut token) = token_opt else {
        return format_fallback(val, &def.fallback, registry, depth - 1);
    };

    // Step 2: Apply negative descriptor for negative values.
    if val < 0 {
        token = format!("{}{}{}", def.negative.0, token, def.negative.1);
    }

    // Step 3: Apply pad descriptor.
    if let Some((min_len, pad_sym)) = &def.pad {
        let cur_len = token.chars().count() as i32;
        if cur_len < *min_len {
            let needed = (*min_len - cur_len) as usize;
            let padding = pad_sym.repeat(needed);
            token = format!("{padding}{token}");
        }
    }

    // Step 4: Prepend prefix + append suffix.
    format!("{}{}{}", def.prefix, token, def.suffix)
}

fn format_fallback(
    val: i32,
    fallback: &str,
    registry: &CounterStyleRegistry,
    depth: u32,
) -> String {
    if let Some(def) = registry.get(fallback) {
        format_with_def(val, def, registry, depth)
    } else {
        format_counter(val, fallback)
    }
}

fn format_cyclic(val: i32, symbols: &[String]) -> Option<String> {
    if symbols.is_empty() {
        return None;
    }
    let len = symbols.len() as i32;
    // CSS spec: index = (val - 1) mod S, using mathematical (Euclidean) modulo.
    let idx = (val - 1).rem_euclid(len) as usize;
    Some(symbols[idx].clone())
}

fn format_numeric(abs: i32, symbols: &[String]) -> Option<String> {
    let len = symbols.len();
    if len < 2 {
        return None;
    }
    if abs == 0 {
        return Some(symbols[0].clone());
    }
    let mut digits: Vec<&str> = Vec::new();
    let mut n = abs as usize;
    while n > 0 {
        digits.push(&symbols[n % len]);
        n /= len;
    }
    digits.reverse();
    Some(digits.join(""))
}

fn format_alphabetic(val: i32, symbols: &[String]) -> Option<String> {
    if symbols.is_empty() || val < 1 {
        return None;
    }
    let len = symbols.len();
    let mut n = val as usize;
    let mut chars: Vec<&str> = Vec::new();
    loop {
        n -= 1;
        chars.push(&symbols[n % len]);
        n /= len;
        if n == 0 {
            break;
        }
    }
    chars.reverse();
    Some(chars.join(""))
}

fn format_symbolic(val: i32, symbols: &[String]) -> Option<String> {
    if symbols.is_empty() || val < 1 {
        return None;
    }
    let len = symbols.len();
    let idx = (val as usize - 1) % len;
    let repeat = (val as usize - 1) / len + 1;
    Some(symbols[idx].repeat(repeat))
}

/// CSS Counter Styles L3 §2 — format counter `n` using a resolved `CounterStyleDef`.
///
/// Public entry point for single-definition formatting. Used in list-marker
/// building and `content:` counter resolution when the definition is already
/// resolved from the registry.
pub fn resolve_counter_value(def: &CounterStyleDef, n: i32, registry: &CounterStyleRegistry) -> String {
    format_with_def(n, def, registry, 8)
}

/// CSS Lists L3 §2.1 — canonical wiring point for `list-style-type` + `@counter-style`.
///
/// Builds the full marker text string for a list item, consulting `registry` for
/// custom `@counter-style` definitions first. Built-in types use the standard
/// formatting with ". " suffix. Bullet types (Disc/Circle/Square) return `""` —
/// they are rendered as geometric shapes by the display-list emitter.
///
/// CSS: list-style-type (custom counter-style) — P4 adds `ListStyleType::Custom(name)`
/// and routes it through `format_counter_with_registry(ordinal, name, registry)`.
pub fn build_list_marker_text(
    lst: ListStyleType,
    ordinal: u32,
    registry: &CounterStyleRegistry,
) -> String {
    match lst {
        // Bullets rendered geometrically; no text needed.
        ListStyleType::None | ListStyleType::Disc | ListStyleType::Circle | ListStyleType::Square => String::new(),
        // Built-in counter styles: format ordinal then append ". ".
        ListStyleType::Decimal => format!("{}. ", format_counter_with_registry(ordinal as i32, "decimal", registry)),
        ListStyleType::DecimalLeadingZero => format!("{:02}. ", ordinal),
        ListStyleType::LowerRoman => format!("{}. ", format_counter_with_registry(ordinal as i32, "lower-roman", registry)),
        ListStyleType::UpperRoman => format!("{}. ", format_counter_with_registry(ordinal as i32, "upper-roman", registry)),
        ListStyleType::LowerAlpha => format!("{}. ", format_counter_with_registry(ordinal as i32, "lower-alpha", registry)),
        ListStyleType::UpperAlpha => format!("{}. ", format_counter_with_registry(ordinal as i32, "upper-alpha", registry)),
        ListStyleType::LowerGreek => format!("{}. ", format_counter_with_registry(ordinal as i32, "lower-greek", registry)),
        ListStyleType::Custom(ref name) => format_counter_with_registry(ordinal as i32, name, registry),
    }
}

fn format_additive(val: i32, tuples: &[(i32, String)]) -> Option<String> {
    if val < 0 {
        return None;
    }
    if val == 0 {
        return tuples
            .iter()
            .find(|(w, _)| *w == 0)
            .map(|(_, s)| s.clone());
    }
    let mut result = String::new();
    let mut remaining = val as u64;
    for (weight, symbol) in tuples {
        if *weight <= 0 {
            continue;
        }
        let w = *weight as u64;
        if remaining >= w {
            let count = (remaining / w) as usize;
            remaining -= w * count as u64;
            for _ in 0..count {
                result.push_str(symbol);
            }
        }
    }
    if remaining > 0 { None } else { Some(result) }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_decimal() {
        assert_eq!(format_counter(1, "decimal"), "1");
        assert_eq!(format_counter(42, "decimal"), "42");
        assert_eq!(format_counter(0, "decimal"), "0");
        assert_eq!(format_counter(-1, "decimal"), "-1");
    }

    #[test]
    fn format_lower_alpha() {
        assert_eq!(format_counter(1, "lower-alpha"), "a");
        assert_eq!(format_counter(26, "lower-alpha"), "z");
        assert_eq!(format_counter(27, "lower-alpha"), "aa");
        assert_eq!(format_counter(52, "lower-alpha"), "az");
        assert_eq!(format_counter(53, "lower-alpha"), "ba");
    }

    #[test]
    fn format_upper_alpha() {
        assert_eq!(format_counter(1, "upper-alpha"), "A");
        assert_eq!(format_counter(26, "upper-alpha"), "Z");
        assert_eq!(format_counter(27, "upper-alpha"), "AA");
    }

    #[test]
    fn format_lower_roman() {
        assert_eq!(format_counter(1, "lower-roman"), "i");
        assert_eq!(format_counter(4, "lower-roman"), "iv");
        assert_eq!(format_counter(9, "lower-roman"), "ix");
        assert_eq!(format_counter(14, "lower-roman"), "xiv");
        assert_eq!(format_counter(40, "lower-roman"), "xl");
        assert_eq!(format_counter(90, "lower-roman"), "xc");
        assert_eq!(format_counter(400, "lower-roman"), "cd");
        assert_eq!(format_counter(900, "lower-roman"), "cm");
        assert_eq!(format_counter(1994, "lower-roman"), "mcmxciv");
    }

    #[test]
    fn format_upper_roman() {
        assert_eq!(format_counter(1994, "upper-roman"), "MCMXCIV");
    }

    #[test]
    fn format_none() {
        assert_eq!(format_counter(5, "none"), "");
    }

    #[test]
    fn counter_ctx_reset_increment_pop() {
        let mut ctx = CounterCtx::default();
        ctx.apply_reset(&[("x".into(), 0)]);
        ctx.apply_increment(&[("x".into(), 1)]);
        assert_eq!(ctx.stacks["x"], vec![1]);

        // Nested scope
        ctx.apply_reset(&[("x".into(), 10)]);
        ctx.apply_increment(&[("x".into(), 1)]);
        assert_eq!(ctx.stacks["x"], vec![1, 11]);

        ctx.pop_reset(&[("x".into(), 10)]);
        assert_eq!(ctx.stacks["x"], vec![1]);

        ctx.pop_reset(&[("x".into(), 0)]);
        assert!(!ctx.stacks.contains_key("x"));
    }

    // ── quote nesting depth (CSS Generated Content L3 §3.2) ──────────────────

    #[test]
    fn quote_depths_nested_document_order() {
        // Document order for `<q><q></q></q>`: outer ::before open, inner
        // ::before open, inner ::after close, outer ::after close.
        let mut depth = 0;
        let ob = record_quote_depths(&Content::Items(vec![ContentItem::OpenQuote]), &mut depth);
        let ib = record_quote_depths(&Content::Items(vec![ContentItem::OpenQuote]), &mut depth);
        let ia = record_quote_depths(&Content::Items(vec![ContentItem::CloseQuote]), &mut depth);
        let oa = record_quote_depths(&Content::Items(vec![ContentItem::CloseQuote]), &mut depth);
        assert_eq!(ob, vec![0]);
        assert_eq!(ib, vec![1]);
        assert_eq!(ia, vec![1]);
        assert_eq!(oa, vec![0]);
        assert_eq!(depth, 0);
    }

    #[test]
    fn quote_depths_no_quote_adjusts_without_emit() {
        let mut depth = 0;
        let v = record_quote_depths(
            &Content::Items(vec![
                ContentItem::OpenQuote,
                ContentItem::NoOpenQuote,
                ContentItem::CloseQuote,
                ContentItem::NoCloseQuote,
            ]),
            &mut depth,
        );
        // open→emit 0 (depth 1); no-open→depth 2; close→depth 1, emit 1; no-close→depth 0.
        assert_eq!(v, vec![0, 1]);
        assert_eq!(depth, 0);
    }

    #[test]
    fn quote_depths_close_underflow_clamped() {
        let mut depth = 0;
        let v = record_quote_depths(&Content::Items(vec![ContentItem::CloseQuote]), &mut depth);
        // Unbalanced close at depth 0 stays at 0 (saturating).
        assert_eq!(v, vec![0]);
        assert_eq!(depth, 0);
    }

    #[test]
    fn quote_depths_non_items_content_empty() {
        let mut depth = 3;
        let v = record_quote_depths(&Content::Normal, &mut depth);
        assert!(v.is_empty());
        assert_eq!(depth, 3);
    }

    #[test]
    fn counter_ctx_auto_create_on_increment() {
        let mut ctx = CounterCtx::default();
        ctx.apply_increment(&[("y".into(), 1)]);
        assert_eq!(ctx.stacks["y"], vec![1]);
    }

    #[test]
    fn counter_ctx_set_overrides_increment() {
        // CSS Lists L3 §4: order is reset → increment → set, so set wins.
        let mut ctx = CounterCtx::default();
        ctx.apply_reset(&[("c".into(), 0)]);
        ctx.apply_increment(&[("c".into(), 1)]);
        ctx.apply_set(&[("c".into(), 5)]);
        assert_eq!(ctx.stacks["c"], vec![5]);
    }

    #[test]
    fn counter_ctx_set_auto_creates() {
        // counter-set on a counter that was never reset acts as if it created it.
        let mut ctx = CounterCtx::default();
        ctx.apply_set(&[("z".into(), 7)]);
        assert_eq!(ctx.stacks["z"], vec![7]);
    }

    #[test]
    fn counter_ctx_set_only_top_scope() {
        // counter-set targets the innermost scope, leaving outer scopes intact.
        let mut ctx = CounterCtx::default();
        ctx.apply_reset(&[("c".into(), 1)]);
        ctx.apply_reset(&[("c".into(), 10)]);
        ctx.apply_set(&[("c".into(), 99)]);
        assert_eq!(ctx.stacks["c"], vec![1, 99]);
    }

    #[test]
    fn counter_set_test97_sibling_rows() {
        // BUG-213 / TEST-97: regression for the full counter-set row sequence.
        // body { counter-reset: c 0 } scopes a single counter `c`; five sibling
        // rows mutate it in document order. Each row's snapshot (value after its
        // own reset → increment → set, before children) is what `::before
        // { content: counter(c) }` renders. Expected: 5, 6, 0, 1, 42 — matching
        // Edge pixel-for-pixel (CSS Lists L3 §4: set wins over a same-element
        // increment; counter-set on a never-reset-this-element counter sets the
        // existing scope).
        let mut ctx = CounterCtx::default();
        // body: counter-reset: c 0
        let body_reset = [("c".to_string(), 0)];
        ctx.apply_reset(&body_reset);

        let top = |ctx: &CounterCtx| *ctx.stacks["c"].last().unwrap();

        // .r1 { counter-set: c 5 } → 5
        ctx.apply_set(&[("c".into(), 5)]);
        assert_eq!(top(&ctx), 5, "r1: set 5 on reset-0 counter");

        // .r2 { counter-increment: c } → 6
        ctx.apply_increment(&[("c".into(), 1)]);
        assert_eq!(top(&ctx), 6, "r2: increment from 5");

        // .r3 { counter-increment: c; counter-set: c 0 } → 0 (set wins)
        ctx.apply_increment(&[("c".into(), 1)]);
        ctx.apply_set(&[("c".into(), 0)]);
        assert_eq!(top(&ctx), 0, "r3: increment to 7 then set 0, set wins");

        // .r4 { counter-increment: c } → 1
        ctx.apply_increment(&[("c".into(), 1)]);
        assert_eq!(top(&ctx), 1, "r4: increment from 0");

        // .r5 { counter-set: c 42 } → 42
        ctx.apply_set(&[("c".into(), 42)]);
        assert_eq!(top(&ctx), 42, "r5: set 42");
    }

    /// BUG-341 S23 gate — **by counter, on both arms**.
    ///
    /// `walk` used to store one snapshot per element unconditionally; in a
    /// document with no counters in scope every one of them was empty, and an
    /// empty snapshot is indistinguishable from an absent one at both consumers.
    /// The assertion is on the *number of stored snapshots*, which no rendering
    /// diff can see — an empty document renders identically either way.
    ///
    /// The second arm is what makes the first mean anything: storing nothing at
    /// all would pass arm 1 and silently render every `counter()` as `0`.
    #[test]
    fn bug341_s23_counter_snapshots_are_stored_only_where_a_counter_is_in_scope() {
        let vp = Size::new(800.0, 600.0);
        fn all_ids(doc: &Document, id: NodeId, out: &mut Vec<NodeId>) {
            out.push(id);
            for &c in &doc.get(id).children {
                all_ids(doc, c, out);
            }
        }
        let stored = |html: &str, css: &str| {
            let doc = lumen_html_parser::parse(html);
            let flat = lumen_dom::build_flat_tree(&doc);
            let map = precompute_counters(&doc, &lumen_css_parser::parse(css), vp, &flat, false);
            let mut ids = Vec::new();
            all_ids(&doc, doc.root(), &mut ids);
            ids.into_iter().filter(|&id| map.counters(id).is_some()).count()
        };

        // Arm 1 — no counter anywhere: not one snapshot for ~6 elements.
        assert_eq!(
            stored("<div><p>a</p><p>b</p></div>", "p { color: red; }"),
            0,
            "a document with no counters must store no counter snapshots"
        );

        // Arm 2 — a counter in scope: every element inside the scope keeps its
        // snapshot, and the values are the ones `counter()` would render.
        let doc = lumen_html_parser::parse("<ol><li>a</li><li>b</li></ol>");
        let flat = lumen_dom::build_flat_tree(&doc);
        let css = "ol { counter-reset: n 0; } li { counter-increment: n; }";
        let map = precompute_counters(&doc, &lumen_css_parser::parse(css), vp, &flat, false);
        let mut ids = Vec::new();
        all_ids(&doc, doc.root(), &mut ids);
        let lis: Vec<NodeId> = ids
            .into_iter()
            .filter(|&id| doc.get(id).element_name().is_some_and(|q| q.local.as_str() == "li"))
            .collect();
        assert_eq!(lis.len(), 2, "fixture must hold two <li>");
        let value = |id: NodeId| {
            map.counters(id)
                .and_then(|s| s.get("n"))
                .and_then(|v| v.last())
                .copied()
        };
        assert_eq!(value(lis[0]), Some(1), "first <li> must still see n=1");
        assert_eq!(value(lis[1]), Some(2), "second <li> must still see n=2");
    }

    /// BUG-341 S26 gate — **by counter, on both arms**, and the second arm is
    /// the one that guards against visible corruption.
    ///
    /// A cycle whose delta says nothing changed must not enter `walk` at all.
    /// That is unobservable in the output by construction — the walk's whole
    /// contribution on such a cycle is to restate its input — so only a counter
    /// can tell "did not walk" from "walked and reused everything", which is the
    /// S8 lesson applied to a stage instead of to a reuse mechanism.
    ///
    /// Arm two: the map's `nodes`/`quotes` collections are rebuilt by the walk,
    /// not carried, so a pass that skips the walk hands back empty ones. On a
    /// document that uses counters that would silently render every `counter()`
    /// as nothing — visible corruption, not a slow frame. The licence is the
    /// `generated_content` bit the previous pass stamped into the carried cache,
    /// and this arm is what proves the bit is actually consulted: without it,
    /// arm one passes and the counter text disappears.
    #[test]
    fn bug341_s26_an_unchanged_cycle_skips_the_walk_unless_it_generates_content() {
        let vp = Size::new(800.0, 600.0);

        // One cycle over `html`/`css` whose delta says nothing changed, run on
        // the cache a full pass produced. Returns (nodes walk entered, the map).
        let unchanged_cycle = |html: &str, css: &str| {
            let doc = lumen_html_parser::parse(html);
            let sheet = lumen_css_parser::parse(css);
            let flat = lumen_dom::build_flat_tree(&doc);
            let full = precompute_counters(&doc, &sheet, vp, &flat, false);
            let delta = RestyleDelta {
                prev_styles: full.into_styles(),
                dirty_roots: HashSet::new(),
                content_dirty: ContentDirty::Nothing,
            };
            set_incremental_restyle(true);
            let _ = take_cascade_stats();
            let map = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, delta);
            let stats = take_cascade_stats();
            set_incremental_restyle(false);
            (stats, map, doc)
        };

        // Arm 1 — no generated content: the walk is skipped outright.
        let (stats, map, doc) = unchanged_cycle("<div><p>a</p><p>b</p></div>", "p { color: red; }");
        assert_eq!(
            stats.visited, 0,
            "a cycle with an empty root set, no content mutation and a carried cache re-walked \
             {} node(s) to conclude what the delta already stated. Every element would take the \
             reuse branch and every node report itself clean, so the output is identical either \
             way — which is precisely why this has to be a counter.",
            stats.visited,
        );
        assert_eq!(stats.recomputed, 0, "and it must not have re-cascaded anything");
        // The set the skipped walk still has to produce: the document's element
        // children, which is what the reuse index stops at.
        let html_el = doc
            .get(doc.root())
            .children
            .iter()
            .copied()
            .find(|&c| matches!(doc.get(c).data, NodeData::Element { .. }))
            .expect("fixture must have a root element");
        assert!(
            map.clean_subtrees().contains(&html_el),
            "the skipped walk must still license reuse of the root element's subtree, or the box \
             stage rebuilds the whole tree it was about to move"
        );

        // Arm 2 — the same shape of cycle on a document that *does* generate
        // content must walk, and must still know its counter values.
        let (stats, map, doc) = unchanged_cycle(
            "<ol><li>a</li><li>b</li></ol>",
            "ol { counter-reset: n 0; } li { counter-increment: n; } li::before { content: counter(n); }",
        );
        assert!(
            stats.visited > 0,
            "a document whose previous pass recorded counter snapshots must keep walking — the \
             snapshots are rebuilt by the walk, not carried, so skipping it hands the box stage \
             an empty counter map and every `counter()` renders as nothing"
        );
        let mut ids = Vec::new();
        fn all_ids(doc: &Document, id: NodeId, out: &mut Vec<NodeId>) {
            out.push(id);
            for &c in &doc.get(id).children {
                all_ids(doc, c, out);
            }
        }
        all_ids(&doc, doc.root(), &mut ids);
        let lis: Vec<NodeId> = ids
            .into_iter()
            .filter(|&id| doc.get(id).element_name().is_some_and(|q| q.local.as_str() == "li"))
            .collect();
        assert_eq!(lis.len(), 2, "fixture must hold two <li>");
        let value = |id: NodeId| map.counters(id).and_then(|s| s.get("n")).and_then(|v| v.last()).copied();
        assert_eq!(value(lis[0]), Some(1), "first <li> must still see n=1 after the cycle");
        assert_eq!(value(lis[1]), Some(2), "second <li> must still see n=2 after the cycle");
    }

    /// BUG-341 S26 gate: and a cycle that *does* carry a change must still walk.
    ///
    /// Separate from the test above because it guards a different failure: the
    /// cheapest way to zero the visit counter is to stop cascading altogether,
    /// which arm one of the test above cannot see.
    #[test]
    fn bug341_s26_a_cycle_with_a_real_change_still_walks() {
        let vp = Size::new(800.0, 600.0);
        let doc = lumen_html_parser::parse("<div id=a><p>x</p></div>");
        let sheet = lumen_css_parser::parse("p { color: red; }");
        let flat = lumen_dom::build_flat_tree(&doc);
        let full = precompute_counters(&doc, &sheet, vp, &flat, false);
        let target = doc.find_by_id("a").expect("fixture must have #a");

        let mut dirty_roots = HashSet::new();
        dirty_roots.insert(target);
        let delta = RestyleDelta {
            prev_styles: full.into_styles(),
            dirty_roots,
            content_dirty: ContentDirty::Nothing,
        };
        set_incremental_restyle(true);
        let _ = take_cascade_stats();
        let _ = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, delta);
        let stats = take_cascade_stats();
        set_incremental_restyle(false);

        assert!(
            stats.visited > 0 && stats.recomputed > 0,
            "a delta naming a dirty root must still reach the cascade — visited={} recomputed={}",
            stats.visited,
            stats.recomputed,
        );
    }

    // ── Custom counter style tests ────────────────────────────────────────────

    fn s(v: &str) -> String { v.to_string() }
    fn sym(v: &[&str]) -> Vec<String> { v.iter().map(|x| s(x)).collect() }

    #[test]
    fn cyclic_basic() {
        let syms = sym(&["a", "b", "c"]);
        assert_eq!(format_cyclic(1, &syms), Some(s("a")));
        assert_eq!(format_cyclic(2, &syms), Some(s("b")));
        assert_eq!(format_cyclic(3, &syms), Some(s("c")));
        assert_eq!(format_cyclic(4, &syms), Some(s("a")));
        assert_eq!(format_cyclic(0, &syms), Some(s("c")));
        assert_eq!(format_cyclic(-1, &syms), Some(s("b")));
    }

    #[test]
    fn cyclic_empty_symbols() {
        assert_eq!(format_cyclic(1, &[]), None);
    }

    #[test]
    fn numeric_decimal_like() {
        let syms = sym(&["0","1","2","3","4","5","6","7","8","9"]);
        assert_eq!(format_numeric(0, &syms), Some(s("0")));
        assert_eq!(format_numeric(5, &syms), Some(s("5")));
        assert_eq!(format_numeric(10, &syms), Some(s("10")));
        assert_eq!(format_numeric(42, &syms), Some(s("42")));
        assert_eq!(format_numeric(100, &syms), Some(s("100")));
    }

    #[test]
    fn numeric_binary() {
        let syms = sym(&["0", "1"]);
        assert_eq!(format_numeric(0, &syms), Some(s("0")));
        assert_eq!(format_numeric(1, &syms), Some(s("1")));
        assert_eq!(format_numeric(2, &syms), Some(s("10")));
        assert_eq!(format_numeric(5, &syms), Some(s("101")));
    }

    #[test]
    fn alphabetic_basic() {
        let syms = sym(&["a","b","c","d","e","f","g","h","i","j",
                         "k","l","m","n","o","p","q","r","s","t",
                         "u","v","w","x","y","z"]);
        assert_eq!(format_alphabetic(1, &syms), Some(s("a")));
        assert_eq!(format_alphabetic(26, &syms), Some(s("z")));
        assert_eq!(format_alphabetic(27, &syms), Some(s("aa")));
        assert_eq!(format_alphabetic(52, &syms), Some(s("az")));
        assert_eq!(format_alphabetic(53, &syms), Some(s("ba")));
    }

    #[test]
    fn symbolic_basic() {
        let syms = sym(&["*", "†", "‡"]);
        assert_eq!(format_symbolic(1, &syms), Some(s("*")));
        assert_eq!(format_symbolic(3, &syms), Some(s("‡")));
        assert_eq!(format_symbolic(4, &syms), Some(s("**")));
        // val=7: idx=(7-1)%3=0, repeat=(7-1)/3+1=3 → "***"
        assert_eq!(format_symbolic(7, &syms), Some(s("***")));
    }

    #[test]
    fn symbolic_repeat_count() {
        let syms = sym(&["*"]);
        assert_eq!(format_symbolic(1, &syms), Some(s("*")));
        assert_eq!(format_symbolic(2, &syms), Some(s("**")));
        assert_eq!(format_symbolic(3, &syms), Some(s("***")));
    }

    #[test]
    fn additive_roman_numerals() {
        let tuples: Vec<(i32, String)> = vec![
            (1000, s("M")), (900, s("CM")), (500, s("D")), (400, s("CD")),
            (100, s("C")),  (90, s("XC")),  (50, s("L")),  (40, s("XL")),
            (10, s("X")),   (9, s("IX")),   (5, s("V")),   (4, s("IV")),
            (1, s("I")),
        ];
        assert_eq!(format_additive(1, &tuples), Some(s("I")));
        assert_eq!(format_additive(4, &tuples), Some(s("IV")));
        assert_eq!(format_additive(9, &tuples), Some(s("IX")));
        assert_eq!(format_additive(14, &tuples), Some(s("XIV")));
        assert_eq!(format_additive(1994, &tuples), Some(s("MCMXCIV")));
    }

    #[test]
    fn additive_cannot_represent_returns_none() {
        // Only has weight 2 — can't represent odd numbers
        let tuples: Vec<(i32, String)> = vec![(2, s("□"))];
        assert_eq!(format_additive(1, &tuples), None);
        assert_eq!(format_additive(2, &tuples), Some(s("□")));
        assert_eq!(format_additive(4, &tuples), Some(s("□□")));
    }

    #[test]
    fn parse_symbols_list_quoted() {
        let syms = parse_symbols_list("\"A\" \"B\" \"C\"");
        assert_eq!(syms, vec!["A", "B", "C"]);
    }

    #[test]
    fn parse_symbols_list_unquoted() {
        let syms = parse_symbols_list("A B C D");
        assert_eq!(syms, vec!["A", "B", "C", "D"]);
    }

    #[test]
    fn parse_symbols_list_unicode_escape() {
        let syms = parse_symbols_list("\"\\1F44D\" \"\\1F44E\"");
        assert_eq!(syms, vec!["👍", "👎"]);
    }

    #[test]
    fn parse_additive_symbols_basic() {
        let tuples = parse_additive_symbols("1000 \"M\", 100 \"C\", 1 \"I\"");
        assert_eq!(tuples[0], (1000, s("M")));
        assert_eq!(tuples[1], (100, s("C")));
        assert_eq!(tuples[2], (1, s("I")));
    }

    #[test]
    fn parse_counter_system_variants() {
        assert_eq!(parse_counter_system("cyclic"), CounterSystem::Cyclic);
        assert_eq!(parse_counter_system("numeric"), CounterSystem::Numeric);
        assert_eq!(parse_counter_system("alphabetic"), CounterSystem::Alphabetic);
        assert_eq!(parse_counter_system("symbolic"), CounterSystem::Symbolic);
        assert_eq!(parse_counter_system("additive"), CounterSystem::Additive);
        assert_eq!(parse_counter_system("fixed"), CounterSystem::Fixed(1));
        assert_eq!(parse_counter_system("fixed 3"), CounterSystem::Fixed(3));
        assert_eq!(parse_counter_system("extends my-style"),
                   CounterSystem::Extends(s("my-style")));
    }

    #[test]
    fn format_with_registry_cyclic_thumbs() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Cyclic,
            symbols: sym(&["👍", "👎"]),
            suffix: " ".to_string(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("thumbs"), def);
        assert_eq!(format_counter_with_registry(1, "thumbs", &registry), "👍 ");
        assert_eq!(format_counter_with_registry(2, "thumbs", &registry), "👎 ");
        assert_eq!(format_counter_with_registry(3, "thumbs", &registry), "👍 ");
    }

    #[test]
    fn format_with_registry_fallback_to_decimal() {
        let registry = CounterStyleRegistry::new();
        // Unknown style name falls back to built-in
        assert_eq!(format_counter_with_registry(5, "decimal", &registry), "5");
        assert_eq!(format_counter_with_registry(3, "lower-roman", &registry), "iii");
    }

    #[test]
    fn format_with_registry_negative_value() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Numeric,
            symbols: sym(&["0","1","2","3","4","5","6","7","8","9"]),
            negative: (s("-"), s("")),
            suffix: String::new(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("my-num"), def);
        assert_eq!(format_counter_with_registry(-5, "my-num", &registry), "-5");
    }

    #[test]
    fn format_with_registry_pad() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Numeric,
            symbols: sym(&["0","1","2","3","4","5","6","7","8","9"]),
            pad: Some((3, s("0"))),
            suffix: String::new(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("zero-padded"), def);
        assert_eq!(format_counter_with_registry(1, "zero-padded", &registry), "001");
        assert_eq!(format_counter_with_registry(12, "zero-padded", &registry), "012");
        assert_eq!(format_counter_with_registry(123, "zero-padded", &registry), "123");
        assert_eq!(format_counter_with_registry(1234, "zero-padded", &registry), "1234");
    }

    #[test]
    fn format_with_registry_fixed_range() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Fixed(1),
            symbols: sym(&["①","②","③"]),
            suffix: String::new(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("circled"), def);
        assert_eq!(format_counter_with_registry(1, "circled", &registry), "①");
        assert_eq!(format_counter_with_registry(3, "circled", &registry), "③");
        // Out of range → fallback to decimal
        assert_eq!(format_counter_with_registry(4, "circled", &registry), "4");
    }

    #[test]
    fn parse_counter_range_auto() {
        assert_eq!(parse_counter_range("auto"), CounterRange::Auto);
    }

    #[test]
    fn parse_counter_range_explicit() {
        let r = parse_counter_range("1 10, 20 infinite");
        match r {
            CounterRange::Explicit(bounds) => {
                assert_eq!(bounds.len(), 2);
                assert_eq!(bounds[0].min, Some(1));
                assert_eq!(bounds[0].max, Some(10));
                assert_eq!(bounds[1].min, Some(20));
                assert_eq!(bounds[1].max, None);
            }
            _ => panic!("expected Explicit"),
        }
    }

    // ── build_list_marker_text tests ─────────────────────────────────────────

    #[test]
    fn build_marker_decimal() {
        let reg = CounterStyleRegistry::new();
        assert_eq!(build_list_marker_text(ListStyleType::Decimal, 1, &reg), "1. ");
        assert_eq!(build_list_marker_text(ListStyleType::Decimal, 9, &reg), "9. ");
        assert_eq!(build_list_marker_text(ListStyleType::Decimal, 42, &reg), "42. ");
    }

    #[test]
    fn build_marker_lower_roman() {
        let reg = CounterStyleRegistry::new();
        assert_eq!(build_list_marker_text(ListStyleType::LowerRoman, 1, &reg), "i. ");
        assert_eq!(build_list_marker_text(ListStyleType::LowerRoman, 4, &reg), "iv. ");
        assert_eq!(build_list_marker_text(ListStyleType::LowerRoman, 9, &reg), "ix. ");
    }

    #[test]
    fn build_marker_upper_alpha() {
        let reg = CounterStyleRegistry::new();
        assert_eq!(build_list_marker_text(ListStyleType::UpperAlpha, 1, &reg), "A. ");
        assert_eq!(build_list_marker_text(ListStyleType::UpperAlpha, 26, &reg), "Z. ");
        assert_eq!(build_list_marker_text(ListStyleType::UpperAlpha, 27, &reg), "AA. ");
    }

    #[test]
    fn build_marker_none_empty() {
        let reg = CounterStyleRegistry::new();
        assert_eq!(build_list_marker_text(ListStyleType::None, 1, &reg), "");
    }

    #[test]
    fn build_marker_bullets_empty() {
        let reg = CounterStyleRegistry::new();
        assert_eq!(build_list_marker_text(ListStyleType::Disc, 1, &reg), "");
        assert_eq!(build_list_marker_text(ListStyleType::Circle, 2, &reg), "");
        assert_eq!(build_list_marker_text(ListStyleType::Square, 3, &reg), "");
    }

    #[test]
    fn build_marker_custom_registered() {
        let mut reg = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Cyclic,
            symbols: sym(&["★", "☆"]),
            suffix: " ".to_string(),
            ..CounterStyleDef::default()
        };
        reg.insert(s("stars"), def);
        assert_eq!(
            build_list_marker_text(ListStyleType::Custom("stars".into()), 1, &reg),
            "★ "
        );
        assert_eq!(
            build_list_marker_text(ListStyleType::Custom("stars".into()), 2, &reg),
            "☆ "
        );
    }

    #[test]
    fn build_marker_custom_unknown_falls_back_to_decimal() {
        let reg = CounterStyleRegistry::new();
        // Unknown custom name falls back to decimal representation.
        assert_eq!(
            build_list_marker_text(ListStyleType::Custom("unknown-style".into()), 5, &reg),
            "5"
        );
    }

    #[test]
    fn list_style_type_parse_custom() {
        assert_eq!(
            ListStyleType::parse("my-counter"),
            Some(ListStyleType::Custom("my-counter".into()))
        );
        assert_eq!(ListStyleType::parse("decimal"), Some(ListStyleType::Decimal));
        assert_eq!(ListStyleType::parse("none"), Some(ListStyleType::None));
    }

    // ── resolve_counter_value tests ──────────────────────────────────────────

    #[test]
    fn resolve_counter_value_cyclic() {
        let mut reg = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Cyclic,
            symbols: sym(&["★", "☆"]),
            suffix: s(" "),
            ..CounterStyleDef::default()
        };
        reg.insert(s("stars"), def.clone());
        assert_eq!(resolve_counter_value(&def, 1, &reg), "★ ");
        assert_eq!(resolve_counter_value(&def, 2, &reg), "☆ ");
        assert_eq!(resolve_counter_value(&def, 3, &reg), "★ ");
    }

    #[test]
    fn resolve_counter_value_numeric() {
        let reg = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Numeric,
            symbols: sym(&["0","1","2","3","4","5","6","7","8","9"]),
            suffix: s("."),
            ..CounterStyleDef::default()
        };
        assert_eq!(resolve_counter_value(&def, 1, &reg), "1.");
        assert_eq!(resolve_counter_value(&def, 10, &reg), "10.");
        assert_eq!(resolve_counter_value(&def, 42, &reg), "42.");
    }

    #[test]
    fn resolve_counter_value_extends() {
        // "child" extends "parent" (cyclic with ✓/✗ symbols)
        let mut reg = CounterStyleRegistry::new();
        let parent = CounterStyleDef {
            system: CounterSystem::Cyclic,
            symbols: sym(&["✓", "✗"]),
            suffix: s(" "),
            ..CounterStyleDef::default()
        };
        reg.insert(s("checks"), parent);
        let child = CounterStyleDef {
            system: CounterSystem::Extends(s("checks")),
            ..CounterStyleDef::default()
        };
        assert_eq!(resolve_counter_value(&child, 1, &reg), "✓ ");
        assert_eq!(resolve_counter_value(&child, 2, &reg), "✗ ");
        assert_eq!(resolve_counter_value(&child, 3, &reg), "✓ ");
    }
}
