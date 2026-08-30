//! Per-thread cascade-index cache: the [`CascadeIndex`] type (a
//! [`crate::rule_index::RuleIndex`] per top-level/`@layer`/`@media`/`@supports`
//! block plus node-independent sheet-wide predicates), its BUG-341 diagnostics
//! (`CascadeIndexStats`/`PseudoCascadeStats`), and the shadow-tree stylesheet
//! table `:host`/`::slotted()` matching reads.
//!
//! Перенесено батчем SPLIT-ST18 из `crates/engine/layout/src/style.rs`
//! (анкер `struct CascadeIndex`) без правок тел.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::rule_index::RuleIndex;

use lumen_core::geom::Size;
use lumen_css_parser::{
    ComplexSelector, MediaContext, PseudoElementKind, Rule, SimpleSelector, Stylesheet,
    StylesheetRevision, SUPPORTED_PROPERTIES,
};
use lumen_dom::NodeId;

use crate::style::env::media_context_from_viewport;
use crate::style::{forced_colors_active, print_media_active, pseudo_element_name};

/// BUG-284: cascade-wide rule index — the top-level [`RuleIndex`] plus one
/// per-block index for every `@layer`/`@media`/`@supports` block, in the same
/// order as `Stylesheet.layers`/`media_rules`/`supports_rules`.
///
/// Before this, only the top-level `rules` were indexed; the `@layer`/`@media`/
/// `@supports` loops in `compute_style` brute-force scanned every rule in
/// every block for every node. Real-world stylesheets often put the bulk of
/// their rules inside `@media` breakpoints, which made that brute-force scan
/// the dominant cascade cost (observed: ~1.1ms/node on a page with ~1100
/// styled nodes and ~3000 rules, most inside `@media`).
pub(in crate::style) struct CascadeIndex {
    pub(in crate::style) rules: RuleIndex,
    pub(in crate::style) layers: Vec<RuleIndex>,
    pub(in crate::style) media: Vec<RuleIndex>,
    pub(in crate::style) supports: Vec<RuleIndex>,
    /// Perf (docs/tasks/p3-cascade-perf.md Задача 1): whether each
    /// `sheet.media_rules[i]`/`sheet.supports_rules[i]` block is currently
    /// active, precomputed once per (sheet, viewport, dark_mode) instead of
    /// re-evaluating `media.query.matches(..)`/`supports.condition.evaluate(..)`
    /// for every block on every node. This loop is node-independent — the
    /// per-node re-evaluation (×2 call sites: `compute_style` and
    /// `compute_pseudo_element_style`, the latter running twice per element
    /// for `::before`/`::after`) was the dominant cascade cost on stylesheets
    /// that put most rules inside `@media` breakpoints (profiled: ~60% of
    /// `compute_style`'s matching phase on a 3000-rule real-world sheet).
    pub(in crate::style) active_media: Vec<bool>,
    pub(in crate::style) active_supports: Vec<bool>,
    /// BUG-341 S10 — whether the sheet contains *any* rule whose subject is a
    /// `::-webkit-scrollbar*` pseudo-element. `compute_style` translates those
    /// onto `scrollbar-width`/`scrollbar-color` (CC-CSS-1) by running three
    /// extra pseudo-element cascades on **every** element; on a sheet with no
    /// such rule — every page that is not Lumen's own chrome — all three were
    /// pure waste. Node-independent, so it is decided once per sheet here.
    pub(in crate::style) has_webkit_scrollbar_rules: bool,
    /// BUG-341 S10 — whether any declaration in the sheet mentions `quote`
    /// (`content: open-quote`, `quotes: …`). `counters::walk` probes
    /// `::before`/`::after` on every node solely to keep the CSS Generated
    /// Content L3 §3.2 quote-nesting counter continuous; with no quote
    /// anywhere in the sheet that probe cannot produce a depth, so it is
    /// skipped. Deliberately a substring test over raw declaration values: it
    /// over-approximates (a `--quote-color` custom property arms it) and must,
    /// because a `var()` can smuggle `open-quote` in from anywhere. `attr()`
    /// arms it too: that value comes from the DOM, which a sheet-level
    /// predicate cannot see.
    has_quote_content: bool,
    /// BUG-341 S23 — every pseudo-element name the sheet uses as the **subject**
    /// of a selector, lowercased and deduplicated.
    ///
    /// `matches_complex_for_pseudo` only ever looks at the subject compound, so
    /// a name absent from this list cannot match anywhere in the sheet and
    /// `compute_pseudo_element_style` for it is guaranteed to return `None`.
    /// That guarantee is what lets the callers skip whole traversals rather than
    /// individual cascades: `apply_first_line_pseudo_styles` walks the laid-out
    /// tree and probes `::first-line` on every block box — 123 probes with zero
    /// hits per cycle on `chrome.html`, which has no `::first-line` rule at all,
    /// and the largest single item of an interaction frame's pseudo stage.
    ///
    /// A `Vec` rather than a `HashSet`: real sheets use a handful of distinct
    /// pseudo-elements, and a linear `eq_ignore_ascii_case` scan over ≤10 short
    /// names beats hashing a `&str` (and needs no allocation at the call site).
    pseudo_subjects: Vec<Box<str>>,
}

impl CascadeIndex {
    fn empty() -> Self {
        Self {
            rules: RuleIndex::empty(),
            layers: Vec::new(),
            media: Vec::new(),
            supports: Vec::new(),
            active_media: Vec::new(),
            active_supports: Vec::new(),
            has_webkit_scrollbar_rules: false,
            has_quote_content: false,
            pseudo_subjects: Vec::new(),
        }
    }

    /// Builds the index, timing each of its four phases into the returned
    /// [`CascadeIndexStats`] (BUG-341 S21 census — four clock reads per rebuild,
    /// against a rebuild that walks every rule in the sheet several times).
    fn build(sheet: &Stylesheet, media_ctx: &MediaContext) -> (Self, CascadeIndexStats) {
        let t = std::time::Instant::now();
        let rules = RuleIndex::build(sheet);
        let rules_ns = t.elapsed().as_nanos() as u64;

        let t = std::time::Instant::now();
        let layers: Vec<RuleIndex> =
            sheet.layers.iter().map(|l| RuleIndex::build_from_rules(&l.rules)).collect();
        let media: Vec<RuleIndex> =
            sheet.media_rules.iter().map(|m| RuleIndex::build_from_rules(&m.rules)).collect();
        let supports: Vec<RuleIndex> =
            sheet.supports_rules.iter().map(|s| RuleIndex::build_from_rules(&s.rules)).collect();
        let blocks_ns = t.elapsed().as_nanos() as u64;

        let t = std::time::Instant::now();
        let active_media: Vec<bool> =
            sheet.media_rules.iter().map(|m| m.query.matches(media_ctx)).collect();
        let active_supports: Vec<bool> = sheet
            .supports_rules
            .iter()
            .map(|s| s.condition.evaluate(SUPPORTED_PROPERTIES))
            .collect();
        let active_ns = t.elapsed().as_nanos() as u64;

        let t = std::time::Instant::now();
        let has_webkit_scrollbar_rules =
            all_rules(sheet).any(|r| r.selectors.iter().any(selector_targets_webkit_scrollbar));
        let has_quote_content = all_rules(sheet).any(|r| {
            r.declarations
                .iter()
                .any(|d| value_mentions_quote(&d.value) || d.value.contains("attr("))
        });
        let mut pseudo_subjects: Vec<Box<str>> = Vec::new();
        for rule in all_rules(sheet) {
            for selector in &rule.selectors {
                for name in selector_pseudo_subjects(selector) {
                    if !pseudo_subjects.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                        pseudo_subjects.push(name.to_ascii_lowercase().into_boxed_str());
                    }
                }
            }
        }
        let predicates_ns = t.elapsed().as_nanos() as u64;

        let idx = Self {
            rules,
            layers,
            media,
            supports,
            active_media,
            active_supports,
            has_webkit_scrollbar_rules,
            has_quote_content,
            pseudo_subjects,
        };
        let stats = CascadeIndexStats {
            builds: 1,
            build_ns: rules_ns + blocks_ns + active_ns + predicates_ns,
            rules_ns,
            blocks_ns,
            active_ns,
            predicates_ns,
        };
        (idx, stats)
    }
}

/// Every `Rule` in `sheet`, whichever container it sits in (top level,
/// `@media`, `@supports`, `@layer`, `@scope`). Used for the node-independent
/// sheet-wide predicates on [`CascadeIndex`]; a rule's container decides
/// *whether* it applies, which is irrelevant to "does this sheet mention X at
/// all" — over-approximating there only costs the fast path, never correctness.
fn all_rules(sheet: &Stylesheet) -> impl Iterator<Item = &Rule> {
    sheet
        .rules
        .iter()
        .chain(sheet.media_rules.iter().flat_map(|m| m.rules.iter()))
        .chain(sheet.supports_rules.iter().flat_map(|s| s.rules.iter()))
        .chain(sheet.layers.iter().flat_map(|l| l.rules.iter()))
        .chain(sheet.scope_rules.iter().flat_map(|s| s.rules.iter()))
}

/// Whether `selector`'s subject compound carries a `::-webkit-scrollbar*`
/// pseudo-element (`::-webkit-scrollbar`, `-thumb`, `-track`), i.e. whether
/// `apply_webkit_scrollbar_pseudos` could ever find something to apply.
fn selector_targets_webkit_scrollbar(selector: &ComplexSelector) -> bool {
    let subject = selector.tail.last().map_or(&selector.head, |(_, c)| c);
    subject.parts.iter().any(|p| match p {
        SimpleSelector::PseudoElement(PseudoElementKind::Unknown(name)) => {
            name.to_ascii_lowercase().starts_with("-webkit-scrollbar")
        }
        _ => false,
    })
}

/// Every pseudo-element name carried by `selector`'s **subject** compound.
///
/// The subject is the only compound `matches_complex_for_pseudo` inspects, so
/// this is exactly the set of `pseudo` arguments for which that function can
/// return `Some` on this selector. See [`CascadeIndex::pseudo_subjects`].
fn selector_pseudo_subjects(selector: &ComplexSelector) -> impl Iterator<Item = &str> {
    let subject = selector.tail.last().map_or(&selector.head, |(_, c)| c);
    subject.parts.iter().filter_map(|p| match p {
        SimpleSelector::PseudoElement(kind) => Some(pseudo_element_name(kind)),
        _ => None,
    })
}

/// Case-insensitive `value.contains("quote")` without allocating — see
/// [`CascadeIndex::has_quote_content`].
fn value_mentions_quote(value: &str) -> bool {
    value
        .as_bytes()
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"quote"))
}

/// Perf cache key fields beyond (sheet pointer, rules count) that affect
/// which `@media` blocks are active — a viewport resize or dark-mode/
/// print/forced-colors toggle must invalidate `active_media` just like a
/// stylesheet swap does. `f32` compared via `to_bits()` (no NaN in practice
/// for a viewport size, and bit-equality avoids float-equality pitfalls).
#[derive(Clone, Copy, PartialEq)]
struct CascadeMediaKey {
    width_bits: u32,
    height_bits: u32,
    dark_mode: bool,
    print_active: bool,
    forced_colors: bool,
}

impl CascadeMediaKey {
    fn current(viewport: Size, dark_mode: bool) -> Self {
        Self {
            width_bits: viewport.width.to_bits(),
            height_bits: viewport.height.to_bits(),
            dark_mode,
            print_active: print_media_active(),
            forced_colors: forced_colors_active(),
        }
    }
}

/// How many (sheet, media key) pairs the per-thread index cache keeps.
///
/// Two, because a browser frame lays out two documents on one thread — its own
/// chrome and the page — and a single slot would make them evict each other
/// every frame, which is exactly the per-pass rebuild BUG-341 S21 removed. A
/// third document per thread per frame does not exist, and each slot retains an
/// index for the process's life, so the count is deliberately not larger.
pub(in crate::style) const CASCADE_INDEX_SLOTS: usize = 2;

thread_local! {
    /// Per-thread rule-index cache, most recently used first.
    ///
    /// Keyed by ([`Stylesheet::revision`], media key): the revision changes
    /// whenever the sheet's rules do, and the media key covers a viewport
    /// resize or a dark-mode/print/forced-colors toggle, which decide which
    /// `@media` blocks are active.
    ///
    /// The key used to be the sheet's **address** plus its rule count, and an
    /// address is recycled the moment its sheet is freed — so the cache had to
    /// be invalidated at the top of every layout pass and on every rayon worker
    /// to avoid serving a freed sheet's index. That reduced it to a within-pass
    /// cache: a census (BUG-341 S21) measured one rebuild per incremental pass
    /// (0.14-0.22ms, 7-19% of the pass) and 33 per full pass across the worker
    /// pool. A revision is never recycled, so no invalidation is needed and the
    /// index now survives for as long as the sheet does.
    static RULE_IDX_CACHE: RefCell<Vec<(StylesheetRevision, CascadeMediaKey, CascadeIndex)>> =
        const { RefCell::new(Vec::new()) };
}

/// Makes the [`CascadeIndex`] for `sheet` under the current media conditions the
/// front slot of [`RULE_IDX_CACHE`], building it if this thread has not got it.
///
/// Every lookup site calls this first and then reads slot 0, so the borrow is
/// released between the two — a `candidates` call must not run while the cache
/// is mutably borrowed.
pub(in crate::style) fn ensure_cascade_index(sheet: &Stylesheet, viewport: Size, dark_mode: bool) {
    let key = (sheet.revision(), CascadeMediaKey::current(viewport, dark_mode));
    let hit = RULE_IDX_CACHE.with(|cell| {
        let mut slots = cell.borrow_mut();
        match slots.iter().position(|s| (s.0, s.1) == key) {
            Some(0) => true,
            Some(i) => {
                slots.swap(0, i);
                true
            }
            None => false,
        }
    });
    if hit {
        return;
    }
    // Built outside the borrow: `CascadeIndex::build` is long, and holding a
    // `RefCell` across it would panic on any re-entrant cache read.
    let media_ctx = media_context_from_viewport(viewport, dark_mode);
    let idx = build_cascade_index_timed(sheet, &media_ctx);
    RULE_IDX_CACHE.with(|cell| {
        let mut slots = cell.borrow_mut();
        slots.truncate(CASCADE_INDEX_SLOTS - 1);
        slots.insert(0, (key.0, key.1, idx));
    });
}

/// Hands the front slot's index to `f`. Call [`ensure_cascade_index`] first —
/// with an empty cache this falls back to a scratch empty index, which matches
/// nothing rather than matching wrongly.
pub(in crate::style) fn with_front_cascade_index<R>(f: impl FnOnce(&CascadeIndex) -> R) -> R {
    RULE_IDX_CACHE.with(|cell| match cell.borrow().first() {
        Some((_, _, idx)) => f(idx),
        None => f(&CascadeIndex::empty()),
    })
}

#[cfg(test)]
thread_local! {
    /// BUG-341 S10 test instrumentation — counts elements for which
    /// `apply_webkit_scrollbar_pseudos` actually ran its three pseudo-element
    /// cascades (i.e. took neither the "sheet has no such rule" nor the
    /// "same inheritance base as the parent" fast path).
    ///
    /// Gated by count rather than by output on purpose: the values these
    /// cascades produce are identical either way — that is the whole point of
    /// the fast path — so a regression that silently reinstates the per-element
    /// work is invisible to every differential test and shows up only as
    /// wall-clock, where machine noise hides it (BUG-341 "S8").
    pub(in crate::style) static SCROLLBAR_PSEUDO_CASCADES: Cell<u32> = const { Cell::new(0) };

    /// BUG-341 S10 test instrumentation — counts [`pseudo_inherited_style`]
    /// calls, i.e. how often a pseudo-element's 302-field starting style was
    /// actually built. Same reasoning as `SCROLLBAR_PSEUDO_CASCADES`: building
    /// it and then discarding it produces exactly the same output.
    pub(in crate::style) static PSEUDO_BASE_BUILDS: Cell<u32> = const { Cell::new(0) };
}

/// Resets the BUG-341 S10 pseudo-base counter for the current thread.
#[cfg(test)]
pub(crate) fn reset_pseudo_base_builds() {
    PSEUDO_BASE_BUILDS.with(|c| c.set(0));
}

/// Pseudo-element starting styles built since [`reset_pseudo_base_builds`].
#[cfg(test)]
pub(crate) fn pseudo_base_builds() -> u32 {
    PSEUDO_BASE_BUILDS.with(|c| c.get())
}

/// Resets the BUG-341 S10 scrollbar-cascade counter for the current thread.
#[cfg(test)]
pub(in crate::style) fn reset_scrollbar_pseudo_cascades() {
    SCROLLBAR_PSEUDO_CASCADES.with(|c| c.set(0));
}

/// Elements that ran the full `::-webkit-scrollbar*` cascade since the last
/// [`reset_scrollbar_pseudo_cascades`].
#[cfg(test)]
pub(in crate::style) fn scrollbar_pseudo_cascades() -> u32 {
    SCROLLBAR_PSEUDO_CASCADES.with(|c| c.get())
}

/// BUG-341 S20 — tally of [`CascadeIndex`] rebuilds.
///
/// Building the index walks every rule in the sheet four times over
/// ([`RuleIndex::build`], the `@layer`/`@media`/`@supports` blocks, the two
/// sheet-wide predicates). The counter exists because that rebuild is invisible
/// in output and lands *inside* `precompute_counters`' profile scope, where it
/// reads as cascade cost — it is what showed the index being rebuilt once per
/// pass and 33 times per full pass before S21 keyed the cache by revision.
/// It is also the gate: an index that is silently rebuilt every frame produces
/// exactly the same styles, just slower.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CascadeIndexStats {
    /// [`CascadeIndex::build`] calls since the last drain.
    pub builds: u32,
    /// Nanoseconds those calls took (the sum of the four fields below).
    pub build_ns: u64,
    /// Of `build_ns`: the top-level [`RuleIndex::build`].
    pub rules_ns: u64,
    /// Of `build_ns`: one [`RuleIndex`] per `@layer`/`@media`/`@supports` block.
    pub blocks_ns: u64,
    /// Of `build_ns`: evaluating which `@media`/`@supports` blocks are active.
    pub active_ns: u64,
    /// Of `build_ns`: the two sheet-wide predicate scans
    /// (`has_webkit_scrollbar_rules`, `has_quote_content`).
    pub predicates_ns: u64,
}

impl CascadeIndexStats {
    /// Folds `other` into `self` field by field.
    pub fn add(&mut self, other: CascadeIndexStats) {
        self.builds += other.builds;
        self.build_ns += other.build_ns;
        self.rules_ns += other.rules_ns;
        self.blocks_ns += other.blocks_ns;
        self.active_ns += other.active_ns;
        self.predicates_ns += other.predicates_ns;
    }
}

thread_local! {
    /// BUG-341 S20 instrumentation, see [`CascadeIndexStats`]. Always compiled:
    /// the timer runs once per rebuild, not per node, and a rebuild already
    /// costs orders of magnitude more than reading the clock.
    static CASCADE_INDEX_STATS: Cell<CascadeIndexStats> = const {
        Cell::new(CascadeIndexStats {
            builds: 0,
            build_ns: 0,
            rules_ns: 0,
            blocks_ns: 0,
            active_ns: 0,
            predicates_ns: 0,
        })
    };
}

/// Returns the accumulated [`CascadeIndexStats`] and resets the tally.
///
/// Thread-local, like [`crate::box_tree::take_box_build_stats`] and for the
/// same two reasons. It has to *see* the rayon workers, because the cache it
/// counts is per-thread and `build_box` fans flex/grid containers out (the S15
/// trap — a naively thread-local tally reported one rebuild per pass where a
/// full pass makes 33). And it must not see *other* tests, because a gate that
/// asserts "this pass rebuilt nothing" against a process-wide counter fails
/// whenever a concurrent test builds an index. Both hold because the fan-out
/// closure drains each worker's tally back into the parent thread through
/// [`add_cascade_index_stats`], exactly as the box-build tally does.
pub fn take_cascade_index_stats() -> CascadeIndexStats {
    CASCADE_INDEX_STATS.with(|s| s.replace(CascadeIndexStats::default()))
}

/// Folds a rayon worker's drained [`CascadeIndexStats`] into this thread's
/// tally — see [`take_cascade_index_stats`].
pub fn add_cascade_index_stats(other: CascadeIndexStats) {
    CASCADE_INDEX_STATS.with(|s| {
        let mut v = s.get();
        v.add(other);
        s.set(v);
    });
}

/// BUG-341 S20 — per-pass tally of [`compute_pseudo_element_style`] calls.
///
/// A pseudo-element cascade is a full candidate match plus, on a hit, a
/// 302-field starting style. S10 found three of them per element at the
/// *cascade* stage and removed them; the same shape survives at the *box-build*
/// stage, where every flex/grid container unconditionally asks for its own
/// `::before` and `::after`. Counted because the answer is almost always `None`
/// and therefore invisible in the produced tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PseudoCascadeStats {
    /// [`compute_pseudo_element_style`] calls that got as far as matching (i.e.
    /// the node was an element).
    pub calls: u32,
    /// Of those, the ones that produced a style rather than `None`.
    pub hits: u32,
    /// Nanoseconds those calls took.
    pub ns: u64,
}

impl PseudoCascadeStats {
    /// Folds another tally into this one.
    pub fn add(&mut self, other: PseudoCascadeStats) {
        self.calls += other.calls;
        self.hits += other.hits;
        self.ns += other.ns;
    }
}

thread_local! {
    /// BUG-341 S20 instrumentation, see [`PseudoCascadeStats`]. Always compiled:
    /// one clock read against a call that walks the sheet's candidate buckets.
    ///
    /// Thread-local, and `build_box` fans flex/grid containers out over rayon
    /// workers (the S15 trap). **S23**: the fan-out closure now drains each
    /// worker's tally back into the parent through [`add_pseudo_cascade_stats`],
    /// exactly as the box-build and cascade-index tallies do — before that the
    /// census saw only the containers built on the layout thread, which is what
    /// made S20's reading move from 139 to 160 for no visible reason.
    static PSEUDO_CASCADE_STATS: Cell<PseudoCascadeStats> =
        const { Cell::new(PseudoCascadeStats { calls: 0, hits: 0, ns: 0 }) };
}

/// Gate for [`PseudoCascadeStats`]. Off by default.
///
/// `compute_pseudo_element_style` runs twice per element on a full pass, so an
/// unconditional pair of clock reads here would be a per-element cost added to
/// the very path this track exists to make cheaper — a census must not be one
/// of the things it measures. Off, the hook costs one relaxed load.
pub(in crate::style) static PSEUDO_STATS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enables/disables the BUG-341 S20 pseudo-cascade census — see
/// [`PseudoCascadeStats`]. Process-wide, like the box-build censuses.
pub fn set_pseudo_cascade_diagnostics(on: bool) {
    PSEUDO_STATS_ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Returns the accumulated [`PseudoCascadeStats`] and resets the tally.
pub fn take_pseudo_cascade_stats() -> PseudoCascadeStats {
    PSEUDO_CASCADE_STATS.with(|s| s.replace(PseudoCascadeStats::default()))
}

/// Folds a rayon worker's drained [`PseudoCascadeStats`] into this thread's
/// tally — see [`take_pseudo_cascade_stats`].
pub fn add_pseudo_cascade_stats(other: PseudoCascadeStats) {
    PSEUDO_CASCADE_STATS.with(|s| {
        let mut v = s.get();
        v.add(other);
        s.set(v);
    });
}

thread_local! {
    /// BUG-341 S23 — the same tally, split by pseudo-element name.
    ///
    /// "160 calls" does not say which of the ~14 call sites made them, and the
    /// fix for each is different. Keyed by the `pseudo` argument, which is the
    /// one thing every call site already carries. Only filled while
    /// [`set_pseudo_cascade_diagnostics`] is on, so the `String` keys never cost
    /// a production pass anything.
    static PSEUDO_CASCADE_SITES: RefCell<HashMap<String, PseudoCascadeStats>> =
        RefCell::new(HashMap::new());
}

/// Returns the per-pseudo split of [`PseudoCascadeStats`] and resets it.
pub fn take_pseudo_cascade_sites() -> HashMap<String, PseudoCascadeStats> {
    PSEUDO_CASCADE_SITES.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

/// Folds a rayon worker's drained per-pseudo split into this thread's map.
pub fn add_pseudo_cascade_sites(other: HashMap<String, PseudoCascadeStats>) {
    PSEUDO_CASCADE_SITES.with(|m| {
        let mut m = m.borrow_mut();
        for (k, v) in other {
            m.entry(k).or_default().add(v);
        }
    });
}

/// Folds one [`compute_pseudo_element_style`] call into the tally.
pub(in crate::style) fn note_pseudo_cascade(pseudo: &str, ns: u64, hit: bool) {
    PSEUDO_CASCADE_STATS.with(|s| {
        let mut v = s.get();
        v.calls += 1;
        v.hits += u32::from(hit);
        v.ns += ns;
        s.set(v);
    });
    PSEUDO_CASCADE_SITES.with(|m| {
        let mut m = m.borrow_mut();
        let e = m.entry(pseudo.to_string()).or_default();
        e.calls += 1;
        e.hits += u32::from(hit);
        e.ns += ns;
    });
}

/// [`CascadeIndex::build`], tallied into [`CascadeIndexStats`]. Every cache
/// refill goes through here, so the counter cannot drift from reality.
fn build_cascade_index_timed(sheet: &Stylesheet, media_ctx: &MediaContext) -> CascadeIndex {
    let (idx, stats) = CascadeIndex::build(sheet, media_ctx);
    add_cascade_index_stats(stats);
    idx
}

/// Drops every cached [`CascadeIndex`] on the current thread.
///
/// Not needed for correctness — the cache is keyed by
/// [`Stylesheet::revision`], which is never recycled — and deliberately not
/// called on the layout path. Tests use it to measure a cold build.
pub fn clear_rule_idx_cache() {
    RULE_IDX_CACHE.with(|cell| cell.borrow_mut().clear());
}

/// Refreshes the thread-local [`CascadeIndex`] for `sheet` if this thread has
/// not got it, then hands it to `f`.
///
/// The ensure-then-borrow dance is spelled out inline at every candidate lookup
/// in `compute_style`; new call sites should use this instead. Do not call
/// anything that touches `RULE_IDX_CACHE` from inside `f` — the borrow is live
/// for its duration.
pub(in crate::style) fn with_cascade_index<R>(
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    f: impl FnOnce(&CascadeIndex) -> R,
) -> R {
    ensure_cascade_index(sheet, viewport, dark_mode);
    with_front_cascade_index(f)
}

/// CSS Generated Content L3 §3.2 — whether `sheet` can produce quote content
/// at all, i.e. whether the `::before`/`::after` probe in `counters::walk` can
/// yield a quote depth. See [`CascadeIndex::has_quote_content`].
pub fn sheet_has_quote_content(sheet: &Stylesheet, viewport: Size, dark_mode: bool) -> bool {
    with_cascade_index(sheet, viewport, dark_mode, |idx| idx.has_quote_content)
}

/// BUG-341 S23 — whether `sheet` uses `pseudo` (name without the `::`) as the
/// subject of any selector, i.e. whether
/// [`compute_pseudo_element_style`] for it can return anything but `None`.
///
/// Lets a caller skip a whole traversal instead of a single cascade: see
/// [`CascadeIndex::pseudo_subjects`]. **`::marker` is exempt** — CSS Lists L3
/// §2.1 synthesizes a marker style with no rule at all, so a `false` here says
/// nothing about it; callers must not gate `marker` on this predicate.
pub fn sheet_targets_pseudo(
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    pseudo: &str,
) -> bool {
    with_cascade_index(sheet, viewport, dark_mode, |idx| {
        idx.pseudo_subjects.iter().any(|s| s.eq_ignore_ascii_case(pseudo))
    })
}

thread_local! {
    /// CSS Scoping L1 — per-shadow-tree author stylesheets, keyed by shadow-host
    /// `NodeId`. Built once per layout pass from the `<style>` elements inside each
    /// shadow root (`set_shadow_sheets`). Their `:host`/`:host()`/`::slotted()`
    /// rules participate in the cascade only for the host (and its slotted light
    /// children) of the tree they belong to. Document-scope `:host`/`::slotted`
    /// rules are no-ops (CSS Scoping L1 §6.1-6.2).
    pub(in crate::style) static SHADOW_SHEETS: RefCell<std::collections::HashMap<NodeId, Stylesheet>> =
        RefCell::new(std::collections::HashMap::new());

    /// Index of the shadow host whose own shadow-tree stylesheet is currently
    /// being matched, or `u32::MAX` when matching in document scope. The `:host`
    /// pseudo-class matches only when this equals the candidate node's index —
    /// this is what makes document-scope `:host` a no-op while shadow-scope
    /// `:host` matches its host.
    pub(in crate::style) static SHADOW_HOST_SCOPE: Cell<u32> = const { Cell::new(u32::MAX) };
}

/// Install the per-shadow-host author stylesheets for the current layout pass.
///
/// Called once at the start of each layout entry point (after `build_flat_tree`),
/// replacing any sheets from the previous pass. Keyed by shadow-host `NodeId`.
pub fn set_shadow_sheets(map: std::collections::HashMap<NodeId, Stylesheet>) {
    SHADOW_SHEETS.with(|cell| *cell.borrow_mut() = map);
}

/// Drop all installed shadow-tree stylesheets (used by tests to avoid leaking
/// per-host sheets between cases that reuse `NodeId` indices).
pub fn clear_shadow_sheets() {
    SHADOW_SHEETS.with(|cell| cell.borrow_mut().clear());
}

