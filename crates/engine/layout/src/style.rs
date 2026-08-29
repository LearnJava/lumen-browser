//! Style cascade с поддержкой compound и complex selectors, attribute и
//! pseudo-class matching, specificity по CSS Selectors Level 3.
//!
//! Алгоритм каскада: для каждого правила в stylesheet проверяем, матчит ли оно
//! целевой элемент. Если матчит — для каждой декларации записываем «применять с
//! приоритетом (specificity, source_order)». В конце сортируем все
//! применимые декларации по этому ключу (по возрастанию) и применяем — так
//! правило с большей specificity перекрывает меньшую, а при равенстве выигрывает
//! более позднее.
//!
//! Matching complex selector-а — справа налево, жадно: для каждого combinator-а
//! берём первого подходящего предка/sibling-а без back-tracking. Для большинства
//! реальных страниц этого достаточно; патологические случаи `a b c` с
//! вложенными `a`-предками могут промахнуться — это известное упрощение, до
//! фазы со «честным» Selectors-движком.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use crate::font_palette::{resolve_font_palette_overrides, ResolvedFontPalette};
use crate::mathml::MathStyle;
use crate::rule_index::RuleIndex;
use crate::ruby::{RubyAlign, RubyMerge, RubyPosition};
use crate::scroll_timeline::ScrollAxis;

use lumen_core::geom::Size;
use lumen_core::ColorSpace;
use lumen_css_parser::{
    parse_inline_style, AttrOp, AttrSelector, Combinator, ComplexSelector, CompoundSelector,
    Declaration, DirArg, MediaContext, PropertyRule, PseudoClass,
    PseudoElementKind, Rule, SimpleSelector, Specificity, Stylesheet, StylesheetRevision,
    SUPPORTED_PROPERTIES,
};
use lumen_dom::{Attribute, Document, DocumentMode, NodeData, NodeId};

// Разбор CSS-значений, применение значений и табличные справочники, вынесенные
// из этого файла батчами SPLIT-ST3…ST10 (docs/tasks/p1-monolith-split-queue.md §4).
mod apply;
mod calc;
mod env;
mod logical;
mod parse;
mod restyle;
mod shorthand;
mod substitute;
mod values;

// SPLIT-ST8: сама `apply_declaration` вместе со своим `match prop` уехала в
// `style::apply`; вызыватели в этом файле остались на прежнем имени.
use apply::apply_declaration;
// Реэкспорт со старого пути: `resolve_logical_property` — публичный API крейта
// (`pub mod style` в `lib.rs`), вызывателей внутри `style.rs` у неё нет, поэтому
// без реэкспорта путь `lumen_layout::style::resolve_logical_property` пропал бы.
pub use logical::resolve_logical_property;
pub use parse::color::{parse_color, system_color};
// Реэкспорт со старого пути: этих троих зовут `lib.rs` и `animation.rs`, то есть
// потребитель вне `crate::style` (правило §2.1 очереди SPLIT).
pub use parse::image::{parse_background_gradient, parse_gradient_stops};
pub use parse::transform::parse_transform_list;
use parse::color::{encode_srgb, named_color, parse_color_legacy};
use parse::counters::is_css_ident;
// SPLIT-ST9. `CalcNode`/`MathFn`/`RoundStrategy`/`Length`/`LengthOrAuto`/
// `parse_length` — публичная поверхность крейта (`pub mod style` в `lib.rs`,
// обращения `lumen_layout::style::<Имя>` из шести крейтов), поэтому реэкспорт
// обязателен даже там, где вызывателя внутри `style.rs` уже нет (правило §2.1).
pub use calc::{CalcNode, MathFn, RoundStrategy};
pub use values::length::{parse_length, Length, LengthOrAuto};
// `expand_vars_and_env` — `pub(crate)`, её зовёт `lib.rs`; остальные видны только
// внутри `style` и его потомков, поэтому и реэкспорт сужен до `crate::style`.
pub(crate) use substitute::expand_vars_and_env;
pub(in crate::style) use substitute::{expand_attr_val, expand_custom_functions, expand_vars};
pub(in crate::style) use values::length::{parse_length_q, parse_sizing_length};
use parse::font_size::{FontSizeBasis, apply_font_size};

// SPLIT-ST10. Окружение прохода уехало в `style::env`, фан-аут рестайла — в
// `style::restyle`. `pub`-функции обоих — публичная поверхность крейта
// (`pub mod style` в `lib.rs`; `box_tree.rs`/`incremental.rs`/`lib.rs` зовут их
// по старому пути), поэтому реэкспорт обязателен даже там, где вызывателя
// внутри `style.rs` нет (правило §2.1).
pub use env::{
    clear_cq_context, clear_interactive_state, cq_context_active, forced_colors_active,
    pop_ch_ex_context, print_media_active, push_ch_ex_context, set_cq_context, set_forced_colors,
    set_interactive_state, set_print_media, StyleEnvSnapshot,
};
pub use restyle::{
    restyle_node_index, restyle_root_set_for_node_change, restyle_root_set_for_state_change,
    restyle_state_index, NodeChange, NodeRestyleIndex, StateRestyleIndex,
};
// `CONTAINER_CQ`/`FONT_CH_EX` читает `style::values::length` по старому пути
// `crate::style::…` (SPLIT-ST9): это реэкспорт, а не импорт, — своих вызывателей
// в `style.rs` у обеих нет.
pub(in crate::style) use env::{CONTAINER_CQ, FONT_CH_EX};
use env::{media_context_from_viewport, node_in_scope, ACTIVE_NID, FOCUS_NID, HOVER_NID};

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
struct CascadeIndex {
    rules: RuleIndex,
    layers: Vec<RuleIndex>,
    media: Vec<RuleIndex>,
    supports: Vec<RuleIndex>,
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
    active_media: Vec<bool>,
    active_supports: Vec<bool>,
    /// BUG-341 S10 — whether the sheet contains *any* rule whose subject is a
    /// `::-webkit-scrollbar*` pseudo-element. `compute_style` translates those
    /// onto `scrollbar-width`/`scrollbar-color` (CC-CSS-1) by running three
    /// extra pseudo-element cascades on **every** element; on a sheet with no
    /// such rule — every page that is not Lumen's own chrome — all three were
    /// pure waste. Node-independent, so it is decided once per sheet here.
    has_webkit_scrollbar_rules: bool,
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
const CASCADE_INDEX_SLOTS: usize = 2;

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
fn ensure_cascade_index(sheet: &Stylesheet, viewport: Size, dark_mode: bool) {
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
fn with_front_cascade_index<R>(f: impl FnOnce(&CascadeIndex) -> R) -> R {
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
    static SCROLLBAR_PSEUDO_CASCADES: Cell<u32> = const { Cell::new(0) };

    /// BUG-341 S10 test instrumentation — counts [`pseudo_inherited_style`]
    /// calls, i.e. how often a pseudo-element's 302-field starting style was
    /// actually built. Same reasoning as `SCROLLBAR_PSEUDO_CASCADES`: building
    /// it and then discarding it produces exactly the same output.
    static PSEUDO_BASE_BUILDS: Cell<u32> = const { Cell::new(0) };
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
fn reset_scrollbar_pseudo_cascades() {
    SCROLLBAR_PSEUDO_CASCADES.with(|c| c.set(0));
}

/// Elements that ran the full `::-webkit-scrollbar*` cascade since the last
/// [`reset_scrollbar_pseudo_cascades`].
#[cfg(test)]
fn scrollbar_pseudo_cascades() -> u32 {
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
static PSEUDO_STATS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
fn note_pseudo_cascade(pseudo: &str, ns: u64, hit: bool) {
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
fn with_cascade_index<R>(
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
    static SHADOW_SHEETS: RefCell<std::collections::HashMap<NodeId, Stylesheet>> =
        RefCell::new(std::collections::HashMap::new());

    /// Index of the shadow host whose own shadow-tree stylesheet is currently
    /// being matched, or `u32::MAX` when matching in document scope. The `:host`
    /// pseudo-class matches only when this equals the candidate node's index —
    /// this is what makes document-scope `:host` a no-op while shadow-scope
    /// `:host` matches its host.
    static SHADOW_HOST_SCOPE: Cell<u32> = const { Cell::new(u32::MAX) };
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
    /// CSS Flexbox L1 §3 — `display: flex`. Phase 0: парсится и хранится,
    /// но в layout трактуется как `Block` (нет flex-алгоритма). Реальный
    /// flex-pass — отдельная задача.
    Flex,
    /// `display: inline-flex` — аналогично, парсится но трактуется как Inline.
    InlineFlex,
    /// CSS Grid L1 — `display: grid`. Парсится, трактуется как Block.
    Grid,
    /// `display: inline-grid`.
    InlineGrid,
    /// CSS 2.1 §9.2.4 — `display: inline-block`. Внешне ведёт себя как
    /// inline (участвует в inline-потоке родителя), внутри — block
    /// formatting context (имеет собственные width/height/padding/border).
    /// В layout собирается в `BoxKind::InlineBlockRow`.
    InlineBlock,
    /// CSS Display L3 — `display: flow-root`. Creates a BFC; treated as Block in layout.
    FlowRoot,
    /// CSS Display L3 — `display: contents`. Box itself generates no box;
    /// children participate in parent formatting context. Treated as Block (deferred).
    Contents,
    /// CSS 2.1 table display types — parsed/stored; table layout deferred.
    Table,
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableColumnGroup,
    TableColumn,
    TableCell,
    TableCaption,
    /// CSS 2.1 — `display: list-item`. Generates principal block + marker box.
    ListItem,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// CSS `start`: left in LTR context, right in RTL context.
    /// This is the CSS-spec initial value (resolves at layout time via `direction`).
    #[default]
    Start,
    /// CSS `end`: right in LTR context, left in RTL context.
    End,
    Left,
    Center,
    Right,
}

/// CSS Text L3 §7.2 — `text-align-last`. NOT inherited. Initial: `Auto`.
/// Выравнивание последней (или единственной) строки блока.
/// Phase 0: parse + store; применение при line layout — деferred.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlignLast {
    #[default]
    Auto,
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

/// CSS Writing Modes L3 §2.1 — `direction: ltr | rtl`. Inherited.
///
/// Базовое направление потока inline-контента: задаёт paragraph embedding
/// level для Unicode Bidirectional Algorithm (`ltr` → 0, `rtl` → 1) и
/// разрешает логические значения `text-align: start|end` в физические
/// left/right. Реальный bidi-порядок фрагментов считает [`crate::bidi`],
/// применяет `box_tree::wrap_inline_run` → `align_lines`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// CSS Writing Modes L4 §2.2 — `unicode-bidi`. НЕ наследуется.
///
/// Управляет тем, как содержимое inline-бокса участвует в Unicode
/// Bidirectional Algorithm (UAX #9). Каждое значение эквивалентно обёртке
/// текста бокса в явные bidi-control-символы, которые [`crate::bidi`]
/// вставляет в текст параграфа перед прогоном UBA:
///
/// | Значение           | Обёртка (для `direction: ltr` / `rtl`)     |
/// |--------------------|--------------------------------------------|
/// | `normal`           | нет — содержимое сливается с окружением     |
/// | `embed`            | `LRE`/`RLE` … `PDF`                        |
/// | `isolate`          | `LRI`/`RLI` … `PDI`                        |
/// | `bidi-override`    | `LRO`/`RLO` … `PDF`                        |
/// | `isolate-override` | `FSI` `LRO`/`RLO` … `PDF` `PDI`            |
/// | `plaintext`        | `FSI` … `PDI` (направление — first-strong)  |
///
/// `plaintext` игнорирует `direction` бокса: базовое направление берётся
/// правилом P2/P3 из самого содержимого, что и делает `FSI`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UnicodeBidi {
    /// Содержимое участвует в UBA наравне с соседями — bidi-control не вставляется.
    #[default]
    Normal,
    /// Дополнительный уровень вложенности (`LRE`/`RLE` … `PDF`).
    Embed,
    /// Изолированная последовательность (`LRI`/`RLI` … `PDI`).
    Isolate,
    /// Принудительное направление всех символов (`LRO`/`RLO` … `PDF`).
    BidiOverride,
    /// Изоляция + принудительное направление (`FSI` `LRO`/`RLO` … `PDF` `PDI`).
    IsolateOverride,
    /// Изоляция с first-strong базовым направлением (`FSI` … `PDI`).
    Plaintext,
}

/// Разбирает значение `unicode-bidi` (CSS Writing Modes L4 §2.2).
///
/// Ключевые слова CSS ASCII-case-insensitive (CSS Values L4 §2.4).
/// Legacy-префиксы `-webkit-`/`-moz-` у трёх изолирующих значений принимаются
/// как алиасы — так их до сих пор пишут в CSS локализованных страниц.
/// `None` — значение не распознано, объявление игнорируется.
fn match_unicode_bidi(val: &str) -> Option<UnicodeBidi> {
    let v = val.trim().to_ascii_lowercase();
    let v = v.strip_prefix("-webkit-").or_else(|| v.strip_prefix("-moz-")).unwrap_or(&v);
    match v {
        "normal" => Some(UnicodeBidi::Normal),
        "embed" => Some(UnicodeBidi::Embed),
        "isolate" => Some(UnicodeBidi::Isolate),
        "bidi-override" => Some(UnicodeBidi::BidiOverride),
        "isolate-override" => Some(UnicodeBidi::IsolateOverride),
        "plaintext" => Some(UnicodeBidi::Plaintext),
        _ => None,
    }
}

/// CSS Backgrounds L3 §4.6 — спецификация одной тени бокса.
///
/// `inset` тени рисуются внутри коробки (имитация vignetting), не-inset —
/// снаружи (drop-shadow). Color None = currentColor по spec. Blur и spread
/// — длины в пикселях; spread увеличивает / уменьшает форму перед blur-ом.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Option<Color>,
    pub inset: bool,
}

/// CSS Text Decoration L3 §4 — спецификация одной тени текста.
///
/// Отличается от BoxShadow: нет `inset`, нет `spread`. Color None =
/// currentColor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub color: Option<Color>,
}

/// CSS UI L4 §8.1 — `cursor`. Inherited.
///
/// Хранится как enum 17 стандартных keyword-ов. URL-fallback (`cursor:
/// url(custom.png), pointer`) отложен. `Auto` — пусть UA решает (для
/// большинства это `Default`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Auto,
    Default,
    None,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    AllScroll,
    ColResize,
    RowResize,
    NResize,
    EResize,
    SResize,
    WResize,
    NeResize,
    NwResize,
    SeResize,
    SwResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ZoomIn,
    ZoomOut,
}

/// CSS UI L4 §10.1 — `text-overflow`. Не наследуется.
///
/// Применяется к содержимому, которое не помещается в коробку — то есть
/// требует overflow != Visible (обычно `hidden`/`clip`) И отсутствие
/// переноса (white-space: nowrap или overflow на oneline). Без этих
/// условий не имеет эффекта.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// CSS Overflow L3 — `overflow`. Не наследуется.
///
/// `Visible` — содержимое выходит за пределы коробки и видно. `Hidden` —
/// клипуется (без скроллбара). `Clip` — то же, но без формирования
/// scroll container и без поддержки `overflow-anchor`. `Scroll` — всегда
/// показать scrollbar, `Auto` — показать только если контент не влезает.
/// Phase 0 layout только хранит — реальный clipping / scroll в paint
/// pipeline ещё нет.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

/// CSS Display L3 §4 — `visibility`. Inherited.
///
/// В отличие от `display: none`, элемент с `visibility: hidden` участвует
/// в layout (занимает место), но не рисуется. `Collapse` для table-row
/// эквивалентен `display: none` (CSS spec); вне таблиц ведёт себя как
/// `Hidden`. Inheritance — ключевое отличие от display, поэтому дочерний
/// элемент может явно вернуть себя через `visibility: visible`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

/// CSS Text Module L3 §3.1 / L4 §2.1 — `white-space`. Inherited.
///
/// Управляет collapse-ом whitespace и переносами строк. В CSS Text L4 это
/// shorthand над `white-space-collapse` + `text-wrap-mode`; здесь хранится
/// «эффективное» комбинированное значение, которым пользуется layout, а
/// longhand-компоненты лежат в [`ComputedStyle::white_space_collapse`] и
/// `text_wrap_mode` и пересчитывают это поле через
/// [`WhiteSpace::combine`] при каждом применении.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
    /// Preserves all whitespace including tabs and newlines; no line wrapping.
    Pre,
    /// Preserves whitespace; wraps at available width.
    PreWrap,
    /// Collapses spaces but preserves newlines; wraps at available width.
    PreLine,
    /// CSS Text L3 §3.1 `break-spaces` — like `pre-wrap`, but any sequence of
    /// preserved spaces takes up space and provides wrap opportunities.
    /// Phase 0: layout behaves as `pre-wrap` (trailing-space hang nuance
    /// deferred until the line-breaker distinguishes hanging spaces).
    BreakSpaces,
}

impl WhiteSpace {
    /// True when whitespace (tabs, newlines) is preserved rather than collapsed.
    pub fn preserves_whitespace(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::BreakSpaces)
    }

    /// True when line wrapping is disabled (lines only break at forced breaks).
    pub fn is_nowrap(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::Nowrap)
    }

    /// True when segment breaks (`\n`) in the source are preserved as forced
    /// line breaks (CSS Text L4 §3.1: `preserve` / `preserve-breaks` /
    /// `break-spaces` collapse modes).
    pub fn preserves_newlines(self) -> bool {
        self.preserves_whitespace() || self == WhiteSpace::PreLine
    }

    /// CSS Text L4 §2.1 — recombine the two longhand components into the
    /// effective legacy value used by layout.
    ///
    /// `preserve-breaks + nowrap` and the `preserve-spaces` mode have no
    /// legacy equivalent; they map to the closest legacy value (`pre-line`
    /// and `pre-wrap`/`pre` respectively) — documented approximation.
    pub fn combine(collapse: WhiteSpaceCollapse, wrap: TextWrapMode) -> Self {
        let wraps = wrap == TextWrapMode::Wrap;
        match collapse {
            WhiteSpaceCollapse::Collapse => {
                if wraps { WhiteSpace::Normal } else { WhiteSpace::Nowrap }
            }
            WhiteSpaceCollapse::Preserve => {
                if wraps { WhiteSpace::PreWrap } else { WhiteSpace::Pre }
            }
            WhiteSpaceCollapse::PreserveBreaks => WhiteSpace::PreLine,
            WhiteSpaceCollapse::PreserveSpaces => {
                if wraps { WhiteSpace::PreWrap } else { WhiteSpace::Pre }
            }
            WhiteSpaceCollapse::BreakSpaces => {
                if wraps { WhiteSpace::BreakSpaces } else { WhiteSpace::Pre }
            }
        }
    }

    /// Decompose the legacy `white-space` value into its L4 collapse component
    /// (CSS Text L4 §2.1 shorthand expansion).
    pub fn collapse_component(self) -> WhiteSpaceCollapse {
        match self {
            WhiteSpace::Normal | WhiteSpace::Nowrap => WhiteSpaceCollapse::Collapse,
            WhiteSpace::Pre | WhiteSpace::PreWrap => WhiteSpaceCollapse::Preserve,
            WhiteSpace::PreLine => WhiteSpaceCollapse::PreserveBreaks,
            WhiteSpace::BreakSpaces => WhiteSpaceCollapse::BreakSpaces,
        }
    }

    /// Decompose the legacy `white-space` value into its L4 wrap component
    /// (CSS Text L4 §2.1 shorthand expansion).
    pub fn wrap_component(self) -> TextWrapMode {
        if self.is_nowrap() { TextWrapMode::Nowrap } else { TextWrapMode::Wrap }
    }
}

/// CSS Text Module L4 §3.1 — `white-space-collapse`. Inherited.
///
/// Longhand-компонента shorthand-а `white-space`, управляющая collapse-ом
/// пробелов и segment break-ов. Применение пересчитывает эффективное
/// [`ComputedStyle::white_space`] через [`WhiteSpace::combine`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WhiteSpaceCollapse {
    /// `collapse` (initial) — последовательности whitespace схлопываются.
    #[default]
    Collapse,
    /// `preserve` — пробелы и segment break-и сохраняются.
    Preserve,
    /// `preserve-breaks` — segment break-и сохраняются, пробелы схлопываются.
    PreserveBreaks,
    /// `preserve-spaces` — пробелы сохраняются, segment break-и и табы
    /// превращаются в пробелы. Phase 0: аппроксимируется как `preserve`.
    PreserveSpaces,
    /// `break-spaces` — как `preserve`, но preserved-пробелы занимают место
    /// и дают wrap opportunities.
    BreakSpaces,
}

impl WhiteSpaceCollapse {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "collapse" => Some(Self::Collapse),
            "preserve" => Some(Self::Preserve),
            "preserve-breaks" => Some(Self::PreserveBreaks),
            "preserve-spaces" => Some(Self::PreserveSpaces),
            "break-spaces" => Some(Self::BreakSpaces),
            _ => None,
        }
    }
}

/// CSS Text Module L3 §3.4 — `text-transform`. Inherited.
///
/// Применяется к текстовому содержимому при сборке inline-сегментов, до
/// word-wrapping и measurer-а. Cyrillic case-folding делается через
/// `char::to_uppercase` / `to_lowercase` стандартной библиотеки, что даёт
/// правильную обработку русских букв (А↔а, Я↔я и т.д.) без сюрпризов
/// типа турецкого `i`/`I`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    /// `capitalize`: первая буква каждого «слова» (по spec — character с
    /// Unicode property Letter) в верхний регистр. Phase 0: упрощённо —
    /// первая буква каждого whitespace-разделённого токена.
    Capitalize,
}

impl TextTransform {
    /// Применяет преобразование к строке. Не аллоцирует, если transform = None.
    pub fn apply(self, s: &str) -> String {
        match self {
            TextTransform::None => s.to_string(),
            TextTransform::Uppercase => s.to_uppercase(),
            TextTransform::Lowercase => s.to_lowercase(),
            TextTransform::Capitalize => {
                let mut out = String::with_capacity(s.len());
                let mut at_word_start = true;
                for ch in s.chars() {
                    if ch.is_whitespace() {
                        out.push(ch);
                        at_word_start = true;
                    } else if at_word_start {
                        out.extend(ch.to_uppercase());
                        at_word_start = false;
                    } else {
                        out.push(ch);
                    }
                }
                out
            }
        }
    }
}

/// CSS Fonts Module L4: `font-style: normal | italic | oblique`. Inherited.
///
/// Phase 0: layout различает свойство, рендерер пока использует один
/// шрифтовой файл (Inter Regular) и не отрисовывает italic-вариант. Поле
/// нужно, чтобы `text_rendering_eq` правильно разделял inline-фрагменты
/// — это корректно подготавливает структуру под подключение Italic-fontfile
/// или affine-skew transform позже.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// CSS Fonts L4 §6.2 — `font-variant-caps`. Inherited.
///
/// Полный набор значений спецификации. `font-variant` — shorthand, из
/// которого сюда попадает только caps-компонента (остальные longhand-ы
/// — `-ligatures`, `-numeric`, `-east-asian`, `-position`, `-alternates`
/// — ещё не реализованы).
///
/// Рендеринг: пять значений синтезируются в layout-е (`caps_synthesis` в
/// `box_tree.rs` — заглавные буквы, уменьшенные до `SMALL_CAPS_SCALE`),
/// потому что bundled-шрифт (Inter) не содержит ни `smcp`, ни `c2sc`, ни
/// `pcap`. `TitlingCaps` синтезировать нечем — оно уходит в шейпер
/// OpenType-фичей `titl` (см. [`text_font_features`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontVariantCaps {
    /// `normal` (initial) — обычные глифы, никаких caps-подстановок.
    #[default]
    Normal,
    /// `small-caps` — строчные буквы показываются капителью (OpenType `smcp`).
    SmallCaps,
    /// `all-small-caps` — капителью показываются И строчные, И заглавные
    /// (OpenType `c2sc` + `smcp`).
    AllSmallCaps,
    /// `petite-caps` — как `small-caps`, но капитель ниже (OpenType `pcap`).
    /// Синтезируется идентично `small-caps` (Phase 0, как в Gecko).
    PetiteCaps,
    /// `all-petite-caps` — `c2pc` + `pcap`; синтезируется как `all-small-caps`.
    AllPetiteCaps,
    /// `unicase` — заглавные показываются капителью, строчные остаются
    /// строчными (OpenType `unic`).
    Unicase,
    /// `titling-caps` — заглавные заменяются на титульные формы (OpenType
    /// `titl`). Синтезу не поддаётся: без глифов шрифта это no-op.
    TitlingCaps,
}

impl FontVariantCaps {
    /// Разбирает keyword `font-variant-caps` (CSS Fonts L4 §6.2).
    /// `None` — токен не относится к caps-компоненте.
    pub fn from_keyword(kw: &str) -> Option<Self> {
        match kw {
            "normal" => Some(Self::Normal),
            "small-caps" => Some(Self::SmallCaps),
            "all-small-caps" => Some(Self::AllSmallCaps),
            "petite-caps" => Some(Self::PetiteCaps),
            "all-petite-caps" => Some(Self::AllPetiteCaps),
            "unicase" => Some(Self::Unicase),
            "titling-caps" => Some(Self::TitlingCaps),
            _ => None,
        }
    }

    /// OpenType-фичи, которые это значение включает в шейпере.
    ///
    /// Пусто для всех значений, кроме `titling-caps`: остальные
    /// синтезируются в layout-е (`caps_synthesis`), и включать вдобавок
    /// `smcp`/`c2sc` нельзя — по уже поднятому в верхний регистр тексту
    /// `c2sc` отработал бы второй раз и капитель уменьшилась бы дважды.
    pub fn feature_tags(self) -> &'static [[u8; 4]] {
        const TITL: [[u8; 4]; 1] = [*b"titl"];
        match self {
            Self::TitlingCaps => &TITL,
            _ => &[],
        }
    }

    /// CSS-сериализация значения (для `getComputedStyle` и layout-дампов).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::SmallCaps => "small-caps",
            Self::AllSmallCaps => "all-small-caps",
            Self::PetiteCaps => "petite-caps",
            Self::AllPetiteCaps => "all-petite-caps",
            Self::Unicase => "unicase",
            Self::TitlingCaps => "titling-caps",
        }
    }
}

/// CSS Fonts L4 §6.6 — `font-variant-emoji`.
///
/// Задаёт, какой вариант презентации выбирается для символа с эмодзи-формой:
/// текстовый (монохромный) или эмодзи (цветной), — не трогая сам символ.
/// Наследуется.
///
/// **Ограничение Lumen:** значение парсится, наследуется и публикуется в
/// `getComputedStyle`, но на выбор глифа пока не влияет — presentation
/// selection (variation selectors VS15/VS16, curated emoji-fallback в
/// `femtovg_backend`) свойство не читает. Реализовано ради
/// [CSS Color Adjust L1 §3.1](https://drafts.csswg.org/css-color-adjust-1/),
/// который требует форсировать вычисленное значение в forced-colors mode
/// (BUG-388).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontVariantEmoji {
    /// `normal` (initial) — презентацию выбирает UA по своим правилам.
    #[default]
    Normal,
    /// `text` — текстовая (монохромная) презентация.
    Text,
    /// `emoji` — эмодзи-презентация (цветная).
    Emoji,
    /// `unicode` — презентация строго по правилам Unicode (только явные
    /// variation selectors в тексте).
    Unicode,
}

impl FontVariantEmoji {
    /// Разбирает keyword `font-variant-emoji`. `None` — не наш токен.
    pub fn from_keyword(kw: &str) -> Option<Self> {
        match kw {
            "normal" => Some(Self::Normal),
            "text" => Some(Self::Text),
            "emoji" => Some(Self::Emoji),
            "unicode" => Some(Self::Unicode),
            _ => None,
        }
    }

    /// CSS-сериализация значения (для `getComputedStyle` и layout-дампов).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Text => "text",
            Self::Emoji => "emoji",
            Self::Unicode => "unicode",
        }
    }
}

/// Собирает набор OpenType-фич для `DrawText.font_features`.
///
/// CSS Fonts L4 §6.4 (Font Feature Resolution) задаёт порядок: сперва фичи
/// от `font-variant-*`, последними — `font-feature-settings`. Шейпер
/// (`otlayout::apply_feature_overrides`) применяет пары слева направо, так
/// что более поздняя запись перекрывает раннюю — то есть автор может
/// выключить фичу капители через `font-feature-settings`.
pub fn text_font_features(style: &ComputedStyle) -> Vec<([u8; 4], u32)> {
    let caps = style.font_variant_caps.feature_tags();
    let mut out = Vec::with_capacity(caps.len() + style.font_feature_settings.len());
    out.extend(caps.iter().map(|tag| (*tag, 1)));
    out.extend(style.font_feature_settings.iter().map(|f| (f.tag, f.value)));
    out
}

/// CSS Fonts L4 §7.12 — `font-optical-sizing`. Inherited.
///
/// `auto` (initial): UA automatically sets the `opsz` variation axis equal to
/// the computed `font-size` in px. `none`: opsz axis is not touched by the UA.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontOpticalSizing {
    /// UA injects `opsz = font_size` into variation axes automatically.
    #[default]
    Auto,
    /// No automatic optical sizing; `font-variation-settings` controls opsz directly.
    None,
}

/// CSS Fonts Module L4 §2.5 — `font-stretch`. Inherited.
///
/// Хранится в десятых долях процента (u16): `normal` = 1000 (100%),
/// `condensed` = 750 (75%), `expanded` = 1250 (125%). Десятые нужны
/// из-за дробных keyword-ов: `semi-condensed` = 87.5% → 875,
/// `semi-expanded` = 112.5% → 1125. Численные проценты парсятся в
/// том же масштабе и клампятся в [50%, 200%] — Phase 0 не нужны
/// экстремальные значения, и это удерживает значение в u16 без
/// переполнения.
///
/// Значение доезжает до рендера двумя независимыми путями, которые
/// складываются: variable-шрифты получают ось `wdth`
/// (`DrawText::font_variation_axes`), а статические семейства с отдельными
/// condensed/expanded-файлами подбираются matcher-ом по `usWidthClass` из
/// OS/2 (`DrawText::font_stretch` → `FontProvider::pick_face`, CSS Fonts L4
/// §5.2). `text_rendering_eq` учитывает stretch, чтобы фрагменты с разным
/// stretch не сливались.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontStretch(pub u16);

impl FontStretch {
    /// 100% — нормальная ширина.
    pub const NORMAL: Self = Self(1000);

    /// Значение в CSS-процентах, округлённое до целого (50..200) — единицы
    /// [`lumen_core::FaceRecord::stretch`] и `usWidthClass`. Дробные
    /// keyword-ы (`semi-expanded` = 112.5%) округляются: шкала
    /// `usWidthClass` целочисленная и дробных ступеней не имеет.
    pub fn as_percent(self) -> u16 {
        (self.0 + 5) / 10
    }

    /// `<font-stretch-css3>`: keyword или `<percentage>` (CSS Fonts L4 §2.5).
    /// Берёт первый токен — L4 допускает диапазон из двух значений (это
    /// синтаксис дескриптора `@font-face`, для свойства второе значение
    /// игнорируется). `None` — значение не распознано.
    pub fn parse(val: &str) -> Option<Self> {
        let token = val.split_whitespace().next()?;
        if let Some(fs) = Self::from_keyword(token) {
            return Some(fs);
        }
        let pct = token.strip_suffix('%')?;
        let n = pct.trim().parse::<f32>().ok()?;
        // CSS Fonts L4 §2.5: percentage >= 0%. Out-of-range значения
        // формально валидны, но бесполезны для рендеринга и могут
        // переполнить u16 (max ≈ 6553%). Клампим в привычные [50%, 200%].
        let clamped = n.clamp(50.0, 200.0);
        Some(Self((clamped * 10.0).round() as u16))
    }

    fn from_keyword(kw: &str) -> Option<Self> {
        Some(match kw {
            "ultra-condensed" => Self(500),
            "extra-condensed" => Self(625),
            "condensed" => Self(750),
            "semi-condensed" => Self(875),
            "normal" => Self(1000),
            "semi-expanded" => Self(1125),
            "expanded" => Self(1250),
            "extra-expanded" => Self(1500),
            "ultra-expanded" => Self(2000),
            _ => return None,
        })
    }
}

impl Default for FontStretch {
    fn default() -> Self { Self::NORMAL }
}

/// CSS Fonts Module L4 §2.4 — `font-weight`. Inherited.
///
/// Хранится численно (1..1000), как в spec: `normal` = 400, `bold` = 700.
/// Ключевые слова `lighter` / `bolder` относительные — их разрешение
/// (по правилам §2.4.3) делается при парсинге: смотрим на родительский weight
/// и сдвигаем по таблице. `lighter` от 400 = 100; `bolder` от 400 = 700.
///
/// Phase 0: layout различает свойство, рендерер пока всегда Inter Regular —
/// real bold-варианта файлов нет. text_rendering_eq учитывает weight, чтобы
/// bold-фрагменты не сливались с обычными.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const NORMAL: Self = Self(400);
    pub const BOLD: Self = Self(700);

    pub fn is_bold(self) -> bool {
        self.0 >= 600
    }
}

impl Default for FontWeight {
    fn default() -> Self { Self::NORMAL }
}

/// CSS Fonts L4 §7 — одна запись `font-variation-settings`.
///
/// `tag` — четырёхбайтный OpenType axis tag (например `b"wght"`, `b"wdth"`).
/// `value` — user-space значение из CSS (до нормализации fvar/avar).
/// Нормализация выполняется в renderer-е, который имеет доступ к таблицам
/// шрифта. `normal` → пустой Vec; renderer применяет default-instance.
#[derive(Debug, Clone, PartialEq)]
pub struct FontVariationSetting {
    pub tag: [u8; 4],
    pub value: f32,
}

/// CSS Fonts L3 §6 — одна запись `font-feature-settings`.
///
/// `tag` — четырёхбайтный OpenType feature tag (например `b"liga"`,
/// `b"smcp"`). `value` — целое значение фичи: `0` = выключена, `1`
/// (или `on`, или опущено) = включена, >1 = выбор альтернативы
/// (например `"salt" 2`). `normal` → пустой Vec; шейпер применяет свой
/// default-набор фич (`liga`/`clig`/`calt`/`rlig`/`ccmp` + `kern`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFeatureSetting {
    /// Четырёхбайтный OpenType feature tag (ASCII U+20–U+7E).
    pub tag: [u8; 4],
    /// Значение фичи: 0 = off, 1 = on, >1 = номер альтернативы.
    pub value: u32,
}

/// Набор активных линий `text-decoration` для элемента.
///
/// CSS3 разделяет shorthand `text-decoration` на `-line`, `-style`, `-color`;
/// Phase 0 умеет только line (без двойных линий и кастомных цветов). Спецификация
/// CSS3 не наследует text-decoration-line, но визуально декорация всё равно
/// распространяется на потомков. Мы делаем явное наследование — это эквивалентно
/// поведению, ожидаемому от `a { text-decoration: underline }`, и при этом
/// позволяет дочернему элементу явно сбросить декорацию через
/// `text-decoration: none` (CSS3 для этого требует пересоздать stacking context,
/// но в нашей упрощённой модели достаточно перезаписать поле).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextDecorationLine {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
}

impl TextDecorationLine {
    pub const fn is_empty(self) -> bool {
        !self.underline && !self.overline && !self.line_through
    }
}

/// CSS Text Decoration L3 §2.2 — `text-decoration-style`. Стиль штриха
/// для всех активных линий (`underline` / `overline` / `line-through`).
///
/// Spec inherited: no — но в Phase 0 наследуем визуально, по той же причине
/// что [`TextDecorationLine`] (см. doc-комментарий выше).
///
/// Initial: `Solid`. Phase 0 рендерер рисует все стили как Solid одиночной
/// линией; реальное визуальное отличие (`Double` — две параллельные,
/// `Dotted` / `Dashed` — pattern, `Wavy` — синусоида) — задача P2.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextDecorationStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

impl TextDecorationStyle {
    /// Парсит одиночный keyword. Возвращает `None` для невалидных и для
    /// keyword-ов, имеющих другой смысл в context-е shorthand (например,
    /// `none` — это `<line>`, не `<style>`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "solid" => Some(Self::Solid),
            "double" => Some(Self::Double),
            "dotted" => Some(Self::Dotted),
            "dashed" => Some(Self::Dashed),
            "wavy" => Some(Self::Wavy),
            _ => None,
        }
    }
}

/// CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Толщина
/// штриха для линий декорации.
///
/// - `Auto` — UA выбирает (наш default; в Phase 0 рендерер использует 1px).
/// - `FromFont` — берётся из шрифтового `underlinePosition` / `underlineThickness`
///   (post-таблица), если шрифт их экспортирует; иначе как `Auto`.
/// - `Length(px)` — явная resolved-px толщина (после `<length>` resolution).
/// - `Percentage(frac)` — доля от **1em parent font-size** (spec явно
///   ссылается на parent, не на свой font-size). Храним как fraction
///   `0.05` для `5%`; resolved-px вычисляется в renderer-е, где известен
///   parent.font_size.
///
/// Spec inherited: no — но в Phase 0 наследуем визуально, по той же причине
/// что [`TextDecorationLine`].
///
/// Phase 0 рендерер игнорирует это значение (всегда 1px); реальное
/// использование — задача P2.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum TextDecorationThickness {
    #[default]
    Auto,
    FromFont,
    Length(f32),
    Percentage(f32),
}

/// CSS Text Decoration L4 §3.5 — `text-decoration-skip-ink`. Controls whether
/// underlines and overlines skip over glyph ink (descenders).
///
/// Spec inherited: yes. Initial: `Auto`.
///
/// - `Auto` — UA may skip underlines/overlines where they cross glyph ink.
///   Only characters with known ink below baseline (g, j, p, q, y, Q, J)
///   receive gaps. Applies to underlines; overlines are unaffected (they sit
///   above the cap height in normal text).
/// - `All` — UA must skip over all glyphs, including those wholly above/below
///   the decoration line (more aggressive than Auto).
/// - `None` — Never skip; decoration is always a continuous line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextDecorationSkipInk {
    /// Skip where decoration crosses glyph descenders (default).
    #[default]
    Auto,
    /// Skip over all glyphs, even those above/below the line.
    All,
    /// Never skip; draw a continuous line.
    None,
}

/// CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Форма emphasis-marks
/// (точечный набор над/под глифами).
///
/// Spec inherited: yes.
///
/// Grammar: `none | [ [ filled | open ] || [ dot | circle | double-circle |
/// triangle | sesame ] ] | <string>`. Если задан только fill keyword без
/// shape — UA fallback shape = `circle` для horizontal writing mode
/// (Phase 0 единственный supported); для vertical было бы `sesame`.
/// Если задан только shape без fill — fallback fill = `filled`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TextEmphasisStyle {
    #[default]
    None,
    /// Один из 5 предустановленных shape-ов, заполненный или контурный.
    Symbol {
        filled: bool,
        shape: TextEmphasisShape,
    },
    /// Произвольная строка-mark (по spec — первый grapheme cluster; в
    /// Phase 0 храним всю строку как есть, рендерер сам возьмёт первый
    /// graphem).
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEmphasisShape {
    Dot,
    #[default]
    Circle,
    DoubleCircle,
    Triangle,
    Sesame,
}

/// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Сторона
/// относительно текстовой строки, на которой рисуются marks.
///
/// Grammar: `[ over | under ] && [ right | left ]?`. Initial `over right`
/// для horizontal writing mode (наш default; для vertical было бы `over
/// right` тоже, но right имеет другой геометрический смысл — Phase 0 без
/// writing-mode не различает).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEmphasisPosition {
    #[default]
    OverRight,
    OverLeft,
    UnderRight,
    UnderLeft,
}

impl TextEmphasisPosition {
    pub fn is_over(self) -> bool {
        matches!(self, Self::OverRight | Self::OverLeft)
    }
}

/// CSS Text Decoration L3 §6.1 / L4 §5.1 — `text-underline-position`.
/// Управляет вертикальным положением underline относительно baseline.
/// Inherited. Initial: `Auto`.
/// Phase 0: parse + store; real offset calculation при underline paint — P2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextUnderlinePosition {
    /// UA выбирает оптимальное положение (обычно под baseline).
    #[default]
    Auto,
    /// Underline выровнен по шрифтовым метрикам (underline-position из OS/2).
    FromFont,
    /// Underline рисуется строго под текстом (под всеми нижними выносными
    /// символов, alphabetic baseline).
    Under,
    /// Для vertical writing-mode: underline рисуется с левой стороны.
    Left,
    /// Для vertical writing-mode: underline рисуется с правой стороны.
    Right,
}

/// CSS Color Adjustment L1 §4 — `forced-color-adjust`. NOT inherited. Initial: `Auto`.
/// Позволяет автору отказаться от принудительной цветовой настройки UA (Forced Colors Mode).
/// Phase 0: parse + store; применение при принудительных цветах — P2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForcedColorAdjust {
    /// `auto` — UA может применять принудительные цвета.
    #[default]
    Auto,
    /// `none` — элемент сохраняет авторские цвета.
    None,
    /// `preserve-parent-color` — унаследовать у родителя.
    PreserveParentColor,
}

/// CSS Color Adjustment L1 §3 — `color-scheme`. Inherited. Initial: `Normal`.
/// Подсказывает UA, какую цветовую тему поддерживает элемент.
/// Используется через [`ColorScheme::used_dark`] для определения «used
/// color scheme» (§2.3) и через [`system_color`] для резолва системных
/// цветовых ключевых слов (`Canvas`, `ButtonFace` и т.д.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// `normal` — элемент не заявляет предпочтений; UA выбирает самостоятельно.
    #[default]
    Normal,
    /// `light` — элемент поддерживает светлую тему.
    Light,
    /// `dark` — элемент поддерживает тёмную тему.
    Dark,
    /// `light dark` — оба; предпочтение light.
    LightDark,
    /// `dark light` — оба; предпочтение dark.
    DarkLight,
    /// `only light` — только светлая тема, без авто-инверсии UA.
    OnlyLight,
    /// `only dark` — только тёмная тема.
    OnlyDark,
}

impl ColorScheme {
    /// CSS Color Adjustment L1 §2.3 — резолвит «used color scheme» элемента
    /// в булев флаг «тёмная тема».
    ///
    /// `prefer_dark` — предпочтение пользователя / ОС (`@media
    /// (prefers-color-scheme: dark)`, в shell — `Lumen.dark_mode`).
    ///
    /// Алгоритм:
    /// - `light` / `only light` → всегда светлая (форсирует тему, игнорируя ОС);
    /// - `dark` / `only dark` → всегда тёмная;
    /// - `normal` / `light dark` / `dark light` → следуют предпочтению ОС.
    ///   `normal` рендерится в дефолтной теме UA, которая у Lumen совпадает
    ///   с предпочтением ОС (страница без `color-scheme` темнеет в dark-mode).
    ///
    /// Возвращает `true`, если элемент должен рендериться в тёмной теме.
    #[must_use]
    pub fn used_dark(self, prefer_dark: bool) -> bool {
        match self {
            ColorScheme::Light | ColorScheme::OnlyLight => false,
            ColorScheme::Dark | ColorScheme::OnlyDark => true,
            ColorScheme::Normal | ColorScheme::LightDark | ColorScheme::DarkLight => prefer_dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}

/// CSS Color L4 §10 — цветовое пространство для wide-gamut значений.
/// Wide-gamut цвет с float-каналами [0..1 для in-gamut, за пределами — out-of-gamut].
/// Используется для `color(display-p3 …)`, `color(rec2020 …)`, `color(srgb …)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorFloat {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    pub space: ColorSpace,
}

impl ColorFloat {
    /// Конвертирует в sRGB u8, применяя матрицу цветового пространства и гамму.
    /// Out-of-gamut значения клипируются в [0, 255].
    pub fn to_srgb_color(self) -> Color {
        let (lr, lg, lb) = match self.space {
            // Lab is a PCS encoding, not an RGB `ColorFloat` channel space, so it
            // never reaches this RGB→sRGB path; decode as sRGB to stay panic-free.
            ColorSpace::Srgb | ColorSpace::Lab => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                (lr, lg, lb)
            }
            ColorSpace::DisplayP3 => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                p3_linear_to_srgb_linear(lr, lg, lb)
            }
            ColorSpace::Rec2020 => {
                let lr = rec2020_gamma_decode(self.r);
                let lg = rec2020_gamma_decode(self.g);
                let lb = rec2020_gamma_decode(self.b);
                rec2020_linear_to_srgb_linear(lr, lg, lb)
            }
        };
        Color {
            r: encode_srgb(lr),
            g: encode_srgb(lg),
            b: encode_srgb(lb),
            a: (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        }
    }

    /// Линейные sRGB-каналы [0..1] для прямой передачи в GPU без квантизации.
    pub fn to_linear_srgb(self) -> [f32; 4] {
        let (lr, lg, lb) = match self.space {
            // Lab is a PCS encoding, not an RGB `ColorFloat` channel space, so it
            // never reaches this RGB→sRGB path; decode as sRGB to stay panic-free.
            ColorSpace::Srgb | ColorSpace::Lab => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                (lr, lg, lb)
            }
            ColorSpace::DisplayP3 => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                p3_linear_to_srgb_linear(lr, lg, lb)
            }
            ColorSpace::Rec2020 => {
                let lr = rec2020_gamma_decode(self.r);
                let lg = rec2020_gamma_decode(self.g);
                let lb = rec2020_gamma_decode(self.b);
                rec2020_linear_to_srgb_linear(lr, lg, lb)
            }
        };
        [lr, lg, lb, self.a.clamp(0.0, 1.0)]
    }

    /// Конвертирует `ColorFloat` в линейные каналы заданного `target` цветового
    /// пространства.
    ///
    /// `target == self.space` → identity: только декодируется гамма и
    /// возвращаются линейные каналы исходного пространства.
    /// `target == Srgb` → существующий `to_linear_srgb()` (никак не регрессит).
    /// Остальные комбинации пока маппятся через linear sRGB (Step 2 baseline).
    pub fn to_display(self, target: crate::ColorSpace) -> [f32; 4] {
        if target == self.space {
            return [
                self.decode(self.r),
                self.decode(self.g),
                self.decode(self.b),
                self.a.clamp(0.0, 1.0),
            ];
        }
        if target == crate::ColorSpace::Srgb {
            return self.to_linear_srgb();
        }
        // Baseline: route through linear sRGB for all other combos.
        // Step 2 acceptance criteria only require identity-preserve and sRGB
        // regression; P3↔Rec2020 direct mapping is deferred.
        let [r, g, b, a] = self.to_linear_srgb();
        let cf = ColorFloat {
            r,
            g,
            b,
            a,
            space: crate::ColorSpace::Srgb,
        };
        cf.to_display(target)
    }

    fn decode(self, c: f32) -> f32 {
        match self.space {
            crate::ColorSpace::Srgb | crate::ColorSpace::DisplayP3 => srgb_gamma_decode(c),
            crate::ColorSpace::Rec2020 => rec2020_gamma_decode(c),
            crate::ColorSpace::Lab => c,
        }
    }
}

/// CSS Color L4 §17 — XYZ (D65) → linear sRGB (sRGB primary matrix, CIE 1931).
/// Constants match the D65→linear-sRGB block already used in `lab_to_srgb`.
fn xyz_d65_to_srgb_linear(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let lr = 3.240_625_5 * x - 1.537_208 * y - 0.498_628_6 * z;
    let lg = -0.968_930_7 * x + 1.875_756_1 * y + 0.041_517_5 * z;
    let lb = 0.055_710_1 * x - 0.204_021_1 * y + 1.056_995_9 * z;
    (lr, lg, lb)
}

/// CSS Color L4 §11 — Bradford D50 → D65 chromatic adaptation of XYZ.
/// Constants match the D50→D65 block already used in `lab_to_srgb`.
fn xyz_d50_to_d65(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let xn = 0.955_576_6 * x - 0.023_039_3 * y + 0.063_163_6 * z;
    let yn = -0.028_289_5 * x + 1.009_941_6 * y + 0.021_007_7 * z;
    let zn = 0.012_298_2 * x - 0.020_483_0 * y + 1.329_909_8 * z;
    (xn, yn, zn)
}

/// CSS Color L4 §10 — convert a non-displayable predefined `color()` space to
/// linear sRGB. `c1`/`c2`/`c3` are the raw channel values. Returns `None` for
/// an unknown space token (caller treats the whole `color()` as invalid).
///
/// Displayable spaces (`srgb`/`display-p3`/`rec2020`) are *not* handled here —
/// they are stored verbatim as `ColorFloat` to preserve linear precision for
/// GPU paint. The spaces below have no sRGB-displayable representation, so they
/// are gamut-mapped to sRGB at parse time.
fn predefined_to_srgb_linear(space: &str, c1: f32, c2: f32, c3: f32) -> Option<(f32, f32, f32)> {
    Some(match space {
        // Linear-light sRGB primaries — channels are already linear sRGB.
        "srgb-linear" => (c1, c2, c3),
        // Adobe RGB (1998): gamma 563/256, then A98 linear → XYZ(D65) → sRGB.
        "a98-rgb" => {
            let dec = |c: f32| c.signum() * c.abs().powf(563.0 / 256.0);
            let (r, g, b) = (dec(c1), dec(c2), dec(c3));
            let x = 0.576_669 * r + 0.185_558 * g + 0.188_229 * b;
            let y = 0.297_345 * r + 0.627_364 * g + 0.075_291 * b;
            let z = 0.027_031 * r + 0.070_689 * g + 0.991_338 * b;
            xyz_d65_to_srgb_linear(x, y, z)
        }
        // ProPhoto RGB: gamma 1.8 (linear toe below 16·Et), linear → XYZ(D50)
        // → D65 → sRGB.
        "prophoto-rgb" => {
            let dec = |c: f32| {
                if c.abs() <= 16.0 / 512.0 {
                    c / 16.0
                } else {
                    c.signum() * c.abs().powf(1.8)
                }
            };
            let (r, g, b) = (dec(c1), dec(c2), dec(c3));
            let x = 0.797_761 * r + 0.135_186 * g + 0.031_349 * b;
            let y = 0.288_071 * r + 0.711_843 * g + 0.000_086 * b;
            let z = 0.825_105 * b;
            let (x65, y65, z65) = xyz_d50_to_d65(x, y, z);
            xyz_d65_to_srgb_linear(x65, y65, z65)
        }
        // CIE XYZ with a D65 white point (`xyz` is an alias for `xyz-d65`).
        "xyz" | "xyz-d65" => xyz_d65_to_srgb_linear(c1, c2, c3),
        // CIE XYZ with a D50 white point — adapt to D65 first.
        "xyz-d50" => {
            let (x65, y65, z65) = xyz_d50_to_d65(c1, c2, c3);
            xyz_d65_to_srgb_linear(x65, y65, z65)
        }
        _ => return None,
    })
}

/// Linear sRGB → gamma sRGB float in [0,1] (IEC 61966-2-1). Float twin of
/// [`encode_srgb`], used to store gamut-mapped wide-gamut colours back into a
/// `ColorFloat` with `space = Srgb`.
fn encode_srgb_f32(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Display P3 linear → sRGB linear (ICC/CSS Color L4 §10.9 matrix).
fn p3_linear_to_srgb_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let sr =  1.224_94 * r - 0.224_94 * g;
    let sg = -0.042_076 * r + 1.042_076 * g;
    let sb = -0.019_692 * r - 0.078_654 * g + 1.098_346 * b;
    (sr, sg, sb)
}

/// Rec2020 linear → sRGB linear (CSS Color L4 §10.9 matrix).
fn rec2020_linear_to_srgb_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let sr =  1.660_491 * r - 0.587_641 * g - 0.072_85 * b;
    let sg = -0.124_551 * r + 1.132_9 * g - 0.008_35 * b;
    let sb = -0.018_151 * r - 0.100_578 * g + 1.118_73 * b;
    (sr, sg, sb)
}

/// Декодирование sRGB / Display P3 гаммы → линейный свет.
fn srgb_gamma_decode(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Декодирование Rec2020 гаммы (BT.2020 OETF) → линейный свет.
fn rec2020_gamma_decode(c: f32) -> f32 {
    const ALPHA: f32 = 1.099_296_8;
    const BETA: f32 = 0.018_053_97;
    if c < 4.5 * BETA {
        c / 4.5
    } else {
        ((c + (ALPHA - 1.0)) / ALPHA).powf(1.0 / 0.45)
    }
}

/// CSS Color Level 4 §6.2 — system color keywords. Stored as a `Copy` enum to
/// avoid heap allocation in `CssColor`. Resolved to a concrete RGB at cascade
/// used-value time via `system_color()`, not at parse time, so the element's
/// used color scheme (`light`/`dark`) is taken into account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemColor {
    /// `Canvas` / `Window`
    Canvas,
    /// `CanvasText` / `WindowText` / `FieldText`
    CanvasText,
    /// `Field` (input/textarea backgrounds)
    Field,
    /// `ButtonFace`
    ButtonFace,
    /// `ButtonText`
    ButtonText,
    /// `ButtonBorder` / `ThreeDFace`
    ButtonBorder,
    /// `LinkText`
    LinkText,
    /// `VisitedText`
    VisitedText,
    /// `ActiveText`
    ActiveText,
    /// `Highlight` / `SelectedItem`
    Highlight,
    /// `HighlightText` / `SelectedItemText`
    HighlightText,
    /// `GrayText` / `GreyText`
    GrayText,
    /// `Mark`
    Mark,
    /// `MarkText`
    MarkText,
    /// `AccentColor`
    AccentColor,
    /// `AccentColorText`
    AccentColorText,
    /// `ThreeDHighlight`
    ThreeDHighlight,
    /// `ThreeDShadow`
    ThreeDShadow,
    /// `ThreeDLightShadow`
    ThreeDLightShadow,
    /// `ThreeDDarkShadow`
    ThreeDDarkShadow,
    /// `Scrollbar`
    Scrollbar,
    /// `ScrollbarTrack`
    ScrollbarTrack,
    /// `ScrollbarThumb`
    ScrollbarThumb,
}

impl SystemColor {
    /// Parse a CSS system color keyword (case-insensitive). Returns `None` for
    /// non-system-color strings; aliases are normalised to their canonical variant.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "canvas" | "window" => Some(Self::Canvas),
            "canvastext" | "windowtext" | "fieldtext" => Some(Self::CanvasText),
            "field" => Some(Self::Field),
            "buttonface" => Some(Self::ButtonFace),
            "buttontext" => Some(Self::ButtonText),
            "buttonborder" | "threedface" => Some(Self::ButtonBorder),
            "linktext" => Some(Self::LinkText),
            "visitedtext" => Some(Self::VisitedText),
            "activetext" => Some(Self::ActiveText),
            "highlight" | "selecteditem" => Some(Self::Highlight),
            "highlighttext" | "selecteditemtext" => Some(Self::HighlightText),
            "graytext" | "greytext" => Some(Self::GrayText),
            "mark" => Some(Self::Mark),
            "marktext" => Some(Self::MarkText),
            "accentcolor" => Some(Self::AccentColor),
            "accentcolortext" => Some(Self::AccentColorText),
            "threedhighlight" => Some(Self::ThreeDHighlight),
            "threedshadow" => Some(Self::ThreeDShadow),
            "threedlightshadow" => Some(Self::ThreeDLightShadow),
            "threeddarkshadow" => Some(Self::ThreeDDarkShadow),
            "scrollbar" => Some(Self::Scrollbar),
            "scrollbartrack" => Some(Self::ScrollbarTrack),
            "scrollbarthumb" => Some(Self::ScrollbarThumb),
            _ => None,
        }
    }

    /// Returns the canonical lowercase CSS keyword name for this variant.
    fn css_name(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::CanvasText => "canvastext",
            Self::Field => "field",
            Self::ButtonFace => "buttonface",
            Self::ButtonText => "buttontext",
            Self::ButtonBorder => "buttonborder",
            Self::LinkText => "linktext",
            Self::VisitedText => "visitedtext",
            Self::ActiveText => "activetext",
            Self::Highlight => "highlight",
            Self::HighlightText => "highlighttext",
            Self::GrayText => "graytext",
            Self::Mark => "mark",
            Self::MarkText => "marktext",
            Self::AccentColor => "accentcolor",
            Self::AccentColorText => "accentcolortext",
            Self::ThreeDHighlight => "threedhighlight",
            Self::ThreeDShadow => "threedshadow",
            Self::ThreeDLightShadow => "threedlightshadow",
            Self::ThreeDDarkShadow => "threeddarkshadow",
            Self::Scrollbar => "scrollbar",
            Self::ScrollbarTrack => "scrollbartrack",
            Self::ScrollbarThumb => "scrollbarthumb",
        }
    }

    /// Resolve to a concrete sRGB `Color` for the given used color scheme.
    /// `dark` — result of `ColorScheme::used_dark(prefer_dark)` for this element.
    pub fn resolve_color(self, dark: bool) -> Color {
        system_color(self.css_name(), dark)
            .unwrap_or(Color { r: 0, g: 0, b: 0, a: 255 })
    }
}

/// CSS Color L4 §4.2 — типизированное цветовое значение каскада.
///
/// `Rgba` — разрешённый конкретный цвет; `CurrentColor` — keyword `currentcolor`,
/// который разрешается в вычисленное значение `color` элемента при рендеринге.
/// `Wide` — wide-gamut цвет из `color()` функции (Display P3, Rec2020, sRGB float).
/// `System` — CSS Color 4 §6.2 system color keyword; resolved to Rgba at cascade
/// used-value time by `resolve_system_colors` at the end of `compute_style`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CssColor {
    Rgba(Color),
    CurrentColor,
    Wide(ColorFloat),
    /// System color keyword (e.g. `Canvas`, `ButtonFace`). Resolved to `Rgba`
    /// at the end of `compute_style` via `resolve_system_colors_in_style`.
    System(SystemColor),
}

impl CssColor {
    /// Разрешает значение в sRGB u8 Color. `Wide` конвертируется через матрицу.
    /// `System` — fallback light-mode resolution (post-pass should have resolved).
    pub fn resolve(self, current_color: Color) -> Color {
        match self {
            CssColor::Rgba(c) => c,
            CssColor::CurrentColor => current_color,
            CssColor::Wide(f) => f.to_srgb_color(),
            CssColor::System(sc) => sc.resolve_color(false),
        }
    }

    /// Конвертирует в `Color`, минуя `current_color`. `CurrentColor` → `None`.
    /// Wide-gamut значения конвертируются через матрицу в sRGB u8.
    pub fn to_color_opt(self) -> Option<Color> {
        match self {
            CssColor::Rgba(c) => Some(c),
            CssColor::Wide(f) => Some(f.to_srgb_color()),
            CssColor::CurrentColor => None,
            CssColor::System(sc) => Some(sc.resolve_color(false)),
        }
    }

    /// Линейные sRGB-каналы для прямой передачи в GPU.
    pub fn resolve_linear(self, current_color: Color) -> [f32; 4] {
        match self {
            CssColor::Rgba(c) => [
                srgb_gamma_decode(c.r as f32 / 255.0),
                srgb_gamma_decode(c.g as f32 / 255.0),
                srgb_gamma_decode(c.b as f32 / 255.0),
                c.a as f32 / 255.0,
            ],
            CssColor::CurrentColor => {
                let c = current_color;
                [
                    srgb_gamma_decode(c.r as f32 / 255.0),
                    srgb_gamma_decode(c.g as f32 / 255.0),
                    srgb_gamma_decode(c.b as f32 / 255.0),
                    c.a as f32 / 255.0,
                ]
            }
            CssColor::Wide(f) => f.to_linear_srgb(),
            CssColor::System(sc) => {
                let c = sc.resolve_color(false);
                [
                    srgb_gamma_decode(c.r as f32 / 255.0),
                    srgb_gamma_decode(c.g as f32 / 255.0),
                    srgb_gamma_decode(c.b as f32 / 255.0),
                    c.a as f32 / 255.0,
                ]
            }
        }
    }
}

/// SVG Presentation §11.2 — `fill` / `stroke` paint value (`<paint>` type).
/// Used by SVG shape elements. Inherited by descendants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgPaint {
    /// `none` — shape not painted (fully transparent).
    None,
    /// `currentColor` — resolves to the element's computed CSS `color`.
    CurrentColor,
    /// Explicit sRGB color value.
    Color(Color),
}

impl Default for SvgPaint {
    /// SVG §11.2 default fill is black; stroke default is none.
    /// For fill fields use `SvgPaint::Color(Color::BLACK)`; for stroke use `SvgPaint::None`.
    fn default() -> Self {
        SvgPaint::None
    }
}

impl SvgPaint {
    /// Resolves the paint value to a concrete `Color`. Returns `None` if paint is `none`.
    pub fn resolve(self, current_color: Color) -> Option<Color> {
        match self {
            SvgPaint::None => None,
            SvgPaint::CurrentColor => Some(current_color),
            SvgPaint::Color(c) => Some(c),
        }
    }
}

/// CSS Tables L2 §17.6 — `border-collapse`. Inherited. Initial: `Separate`.
/// Controls whether adjacent cell borders are merged (`collapse`) or kept separate (`separate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderCollapse {
    /// Each cell has its own borders separated by `border-spacing`.
    #[default]
    Separate,
    /// Adjacent borders are merged into a single shared border (no `border-spacing`).
    Collapse,
}

impl BorderCollapse {
    /// Parse CSS keyword; returns `None` for unrecognised values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "separate" => Some(Self::Separate),
            "collapse" => Some(Self::Collapse),
            _ => None,
        }
    }
}

/// CSS Tables L2 §17.6.1.1 — `empty-cells`. Inherited. Initial: `Show`.
/// In the separated-borders model, controls whether borders and backgrounds
/// are drawn around table cells that have no in-flow content. Has no effect
/// when `border-collapse: collapse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmptyCells {
    /// Empty cells are painted normally (borders + background drawn).
    #[default]
    Show,
    /// Empty cells suppress their borders and background.
    Hide,
}

impl EmptyCells {
    /// Parse CSS keyword; returns `None` for unrecognised values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "show" => Some(Self::Show),
            "hide" => Some(Self::Hide),
            _ => None,
        }
    }
}

/// SVG §11.3 — `fill-rule`. Inherited. Initial: `NonZero`.
/// Controls how the interior of a shape is determined for overlapping contours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillRule {
    /// Nonzero winding rule: count crossings, fill if winding number ≠ 0.
    #[default]
    NonZero,
    /// Even-odd rule: count crossings, fill if count is odd.
    EvenOdd,
}

/// SVG §11.4 — `stroke-linecap`. Inherited. Initial: `Butt`.
/// Shape of the cap at the end of open sub-paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeLinecap {
    /// Flat cap exactly at the endpoint (default).
    #[default]
    Butt,
    /// Semicircular cap extending `stroke-width/2` past the endpoint.
    Round,
    /// Rectangular cap extending `stroke-width/2` past the endpoint.
    Square,
}

/// SVG §11.4 — `stroke-linejoin`. Inherited. Initial: `Miter`.
/// Shape of join between connected path segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeLinejoin {
    /// Pointed join, bounded by `stroke-miterlimit` (default).
    #[default]
    Miter,
    /// Circular join.
    Round,
    /// Flat bevel cut at the join.
    Bevel,
}

/// CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — one component of `paint-order`.
/// Identifies which of fill, stroke or markers occupies a given paint slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintOrderSlot {
    /// The element's fill.
    Fill,
    /// The element's stroke.
    Stroke,
    /// SVG markers. Lumen does not yet render markers; the slot is preserved so
    /// that fill/stroke ordering around it stays spec-correct.
    Markers,
}

/// CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — `paint-order`. Inherited.
/// Resolved order in which the three components are painted, first slot drawn
/// first (so the last slot ends up on top). Initial value `normal` resolves to
/// `[Fill, Stroke, Markers]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvgPaintOrder(pub [PaintOrderSlot; 3]);

impl Default for SvgPaintOrder {
    fn default() -> Self {
        Self([PaintOrderSlot::Fill, PaintOrderSlot::Stroke, PaintOrderSlot::Markers])
    }
}

impl SvgPaintOrder {
    /// Parses `normal | [ fill || stroke || markers ]` (CSS Fill & Stroke L3 §6).
    /// Returns `None` for an unknown token or a repeated component. Components
    /// omitted from an otherwise-valid list are appended in the canonical
    /// `fill, stroke, markers` order, as the spec requires.
    pub fn parse(value: &str) -> Option<Self> {
        use PaintOrderSlot::{Fill, Markers, Stroke};
        let v = value.trim();
        if v.eq_ignore_ascii_case("normal") {
            return Some(Self::default());
        }
        let mut order: Vec<PaintOrderSlot> = Vec::with_capacity(3);
        for tok in v.split_whitespace() {
            let slot = if tok.eq_ignore_ascii_case("fill") {
                Fill
            } else if tok.eq_ignore_ascii_case("stroke") {
                Stroke
            } else if tok.eq_ignore_ascii_case("markers") {
                Markers
            } else {
                return None;
            };
            if order.contains(&slot) {
                return None; // repeated component — invalid per grammar
            }
            order.push(slot);
        }
        if order.is_empty() {
            return None;
        }
        for slot in [Fill, Stroke, Markers] {
            if !order.contains(&slot) {
                order.push(slot);
            }
        }
        Some(Self([order[0], order[1], order[2]]))
    }

    /// True when fill is painted before stroke (so the stroke is drawn on top).
    /// Markers are ignored — Lumen does not render them. Default `normal`
    /// (fill, stroke, markers) returns `true`; `paint-order: stroke` → `false`.
    pub fn fill_before_stroke(&self) -> bool {
        let fill_idx = self.0.iter().position(|s| *s == PaintOrderSlot::Fill);
        let stroke_idx = self.0.iter().position(|s| *s == PaintOrderSlot::Stroke);
        match (fill_idx, stroke_idx) {
            (Some(f), Some(s)) => f <= s,
            _ => true,
        }
    }
}

/// Стиль линии CSS border. None = рамка не отображается (как `display: none`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, BorderStyle::None)
    }
}

/// CSS Basic UI L4 §5.3 — `outline-style`. Включает все `<border-style>`
/// keyword-ы плюс `auto` (UA-defined focus indicator).
///
/// Phase 0: `Auto` рендерится как Solid с currentColor; отдельный variant
/// сохраняется, чтобы позже отличить «явный solid от автора» от «default
/// UA focus ring» — нужно для accessibility (нельзя глушить focus ring
/// через `outline-style: none` при `:focus-visible` в стиле UA).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutlineStyle {
    #[default]
    None,
    Auto,
    Solid,
    Dashed,
    Dotted,
}

impl OutlineStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, OutlineStyle::None)
    }
}

/// CSS Basic UI L4 §5.4 — `outline-color`. Помимо явного цвета поддерживает
/// `auto` (UA-defined контрастный цвет) и `currentColor` (вычисленный `color`
/// элемента).
///
/// Phase 0: `Auto` и `CurrentColor` оба резолвятся в `style.color` при
/// рендеринге — настоящий UA contrast требует знания фона за outline и
/// откладывается.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutlineColor {
    #[default]
    Auto,
    CurrentColor,
    Color(Color),
}

/// CSS Fragmentation L3 §3.1 — break-before / break-after / break-inside.
/// Phase 0: parse+store; реальный break enforcement требует pagination /
/// multi-column layout pipeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BreakValue {
    #[default]
    Auto,
    /// `avoid` / `avoid-page` / `avoid-column` / `avoid-region` — все
    /// нормализуются в `Avoid`. Phase 0 не различает page vs column vs region.
    Avoid,
    /// `always` / `page` (для break-before/after).
    Always,
    /// `column` — принудительный column break.
    Column,
    /// `page` — принудительный page break.
    Page,
    /// `region` — принудительный region break.
    Region,
}

/// CSS `box-sizing`. Определяет, что именно задаёт `width` / `height`:
///   - `ContentBox` (CSS default): размер контента; padding и border прибавляются сверху.
///   - `BorderBox`: размер вместе с padding и border; контент сжимается, чтобы влезть.
///
/// Свойство НЕ наследуется (CSS Basic UI 3 §4.1) — сбрасывается на default в каждом
/// `compute_style`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

/// CSS Positioned Layout L3 §3 — `position`. Не наследуется.
/// `Static` — нормальный поток (default). Остальные создают
/// containing-block-альтернативу и (для `Fixed` / `Sticky`, а также
/// `Relative` / `Absolute` с явным `z-index`) могут создавать
/// stacking context (§9.10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

impl Position {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "static" => Some(Self::Static),
            "relative" => Some(Self::Relative),
            "absolute" => Some(Self::Absolute),
            "fixed" => Some(Self::Fixed),
            "sticky" => Some(Self::Sticky),
            _ => None,
        }
    }
}

/// CSS 2.1 §9.5.1 — `float`. Не наследуется. `Left`/`Right` выводят
/// элемент из нормального потока и размещают его у соответствующего
/// края контейнера; следующий контент обтекает float сбоку.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FloatSide {
    #[default]
    None,
    Left,
    Right,
}

impl FloatSide {
    /// Parses `float` keyword value.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "inline-start" => Some(Self::Left),
            "inline-end" => Some(Self::Right),
            _ => None,
        }
    }

    /// Returns `true` for `float: none`.
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

/// CSS 2.1 §9.5.2 — `clear`. Не наследуется. Указывает, мимо
/// каких float-ов следующий блок должен «пройти» перед размещением.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClearSide {
    #[default]
    None,
    Left,
    Right,
    Both,
}

impl ClearSide {
    /// Parses `clear` keyword value.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "left" | "inline-start" => Some(Self::Left),
            "right" | "inline-end" => Some(Self::Right),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

/// CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется.
/// `Isolate` принудительно создаёт stacking context, обеспечивая
/// изоляцию blend / backdrop-filter эффектов потомков от внешних
/// слоёв.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Isolation {
    #[default]
    Auto,
    Isolate,
}

impl Isolation {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "isolate" => Some(Self::Isolate),
            _ => None,
        }
    }
}

/// CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется.
/// Любое значение, отличное от `Normal`, создаёт stacking context
/// (§9.10). Phase 0 layout только хранит — реальный compositor pipeline
/// для blend-effects появится у P2 (§16 трек, п.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MixBlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    PlusLighter,
}

impl MixBlendMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "multiply" => Some(Self::Multiply),
            "screen" => Some(Self::Screen),
            "overlay" => Some(Self::Overlay),
            "darken" => Some(Self::Darken),
            "lighten" => Some(Self::Lighten),
            "color-dodge" => Some(Self::ColorDodge),
            "color-burn" => Some(Self::ColorBurn),
            "hard-light" => Some(Self::HardLight),
            "soft-light" => Some(Self::SoftLight),
            "difference" => Some(Self::Difference),
            "exclusion" => Some(Self::Exclusion),
            "hue" => Some(Self::Hue),
            "saturation" => Some(Self::Saturation),
            "color" => Some(Self::Color),
            "luminosity" => Some(Self::Luminosity),
            "plus-lighter" => Some(Self::PlusLighter),
            _ => None,
        }
    }
}

/// CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется.
/// Default `Baseline`.
///
/// Keyword-варианты (`Baseline`, `Sub`, `Super`, `Top`, `TextTop`, `Middle`,
/// `Bottom`, `TextBottom`) — fixed enum values. `Length(px)` — resolved
/// сдвиг по вертикали от baseline (positive = up по CSS, как у всех
/// vertical-shift свойств). `Percent(p)` — процент от `line-height` текущего
/// элемента; разрешается во время layout-а, поскольку требует line-box
/// геометрии.
///
/// Phase 0: parsing + storage. Реальное применение к inline-flow требует
/// поля `y_offset` в `InlineFrag` и совместной правки `lumen-paint`
/// (DrawText.y-offset) — отдельная задача с согласованием P2.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Sub,
    Super,
    Top,
    TextTop,
    Middle,
    Bottom,
    TextBottom,
    /// Resolved px. Положительное — выше baseline, отрицательное — ниже
    /// (как `<length>` в CSS 2.1 §10.8.1).
    Length(f32),
    /// Процент от `line-height` элемента (CSS 2.1 §10.8.1). Резолвится
    /// в layout-pass — здесь хранится как есть.
    Percent(f32),
}

impl VerticalAlign {
    /// Парсит keyword-формы vertical-align. Не покрывает `<length>` /
    /// `<percentage>` — те идут через [`parse_length`] (см. apply_declaration).
    pub fn parse_keyword(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "baseline" => Some(Self::Baseline),
            "sub" => Some(Self::Sub),
            "super" => Some(Self::Super),
            "top" => Some(Self::Top),
            "text-top" => Some(Self::TextTop),
            "middle" => Some(Self::Middle),
            "bottom" => Some(Self::Bottom),
            "text-bottom" => Some(Self::TextBottom),
            _ => None,
        }
    }
}


/// CSS Easing L1 §2 — easing function для CSS Transitions и CSS Animations.
/// Не наследуется (используется как per-list-entry значение в
/// transition/animation longhand-ах). Default по spec — `ease`, что
/// эквивалентно `cubic-bezier(0.25, 0.1, 0.25, 1.0)`.
///
/// P2 п.3B compositor offload и P1 п.3A Web Animations interpolation —
/// потребители этого AST: оба применяют функцию `progress(t) → [0, 1]`
/// к линейному времени `t ∈ [0, 1]` для получения eased progress.
#[derive(Debug, Clone, PartialEq)]
pub enum TimingFunction {
    /// `linear` ≡ `cubic-bezier(0, 0, 1, 1)`. progress(t) = t.
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`. Также покрывает keyword-shortcuts:
    /// `ease` ≡ (0.25, 0.1, 0.25, 1.0);
    /// `ease-in` ≡ (0.42, 0, 1, 1);
    /// `ease-out` ≡ (0, 0, 0.58, 1);
    /// `ease-in-out` ≡ (0.42, 0, 0.58, 1).
    /// x1, x2 ∈ [0, 1] (spec); y1, y2 — unbounded.
    CubicBezier(f32, f32, f32, f32),
    /// `steps(n, <step-position>)`. `step-start` ≡ `steps(1, jump-start)`,
    /// `step-end` ≡ `steps(1, jump-end)`. `n` — положительное целое;
    /// для `jump-none` ещё и ≥ 2.
    Steps(u32, StepPosition),
    /// `linear(<linear-stop-list>)` (CSS Easing L2 §2.4) — кусочно-линейная
    /// функция easing-а, задаваемая 2+ control-точками. Каждая точка:
    /// output (unitless number, может выходить за `[0, 1]`) и input
    /// (∈ `[0, 1]`, монотонно неубывает). Inputs нормализованы по правилам
    /// §2.5.1: пропущенные значения распределяются между соседними
    /// заданными; первая точка получает `0`, последняя — `1`.
    ///
    /// Discontinuity-кейсы (две точки с одинаковым input → вертикальный
    /// прыжок) допустимы и формируются из stop-а с двумя percentage-ами:
    /// `linear(0 0% 50%, 1 50% 100%)` ≡ step-функция со скачком на 0.5.
    ///
    /// `linear(0, 1)` поведенчески эквивалентно `Linear`; парсер хранит
    /// этот случай как `LinearStops`, без коллапса в `Linear`, чтобы
    /// сохранять round-trip.
    LinearStops(Vec<LinearEasingPoint>),
}

/// CSS Easing L2 §2.4 — одна control-точка функции `linear(...)`.
///
/// `output` — значение easing-а в этой точке (unitless, может выходить за
/// `[0, 1]` — overshoot допустим). `input` — соответствующая позиция на
/// time-axis в `[0, 1]`. После канонизации (§2.5.1) inputs всех точек
/// одного `LinearStops` монотонно неубывают.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearEasingPoint {
    /// Output progress в этой точке. Unitless. May exceed `[0, 1]`.
    pub output: f32,
    /// Input progress ∈ `[0, 1]` (доля времени анимации).
    pub input: f32,
}

impl Default for TimingFunction {
    fn default() -> Self {
        // CSS Transitions/Animations L1 — initial value = `ease`.
        TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
    }
}

impl TimingFunction {
    /// Парсит keyword (`linear` / `ease` / `ease-in` / `ease-out` /
    /// `ease-in-out` / `step-start` / `step-end`) или функцию
    /// (`cubic-bezier(...)` / `steps(...)`). Возвращает `None` для
    /// невалидного значения (out-of-range x, n=0, неизвестный keyword).
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        match t.as_str() {
            "linear" => return Some(Self::Linear),
            "ease" => return Some(Self::CubicBezier(0.25, 0.1, 0.25, 1.0)),
            "ease-in" => return Some(Self::CubicBezier(0.42, 0.0, 1.0, 1.0)),
            "ease-out" => return Some(Self::CubicBezier(0.0, 0.0, 0.58, 1.0)),
            "ease-in-out" => return Some(Self::CubicBezier(0.42, 0.0, 0.58, 1.0)),
            "step-start" => return Some(Self::Steps(1, StepPosition::JumpStart)),
            "step-end" => return Some(Self::Steps(1, StepPosition::JumpEnd)),
            _ => {}
        }
        if let Some(args) = t
            .strip_prefix("cubic-bezier(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            if parts.len() != 4 {
                return None;
            }
            let x1 = parts[0].parse::<f32>().ok()?;
            let y1 = parts[1].parse::<f32>().ok()?;
            let x2 = parts[2].parse::<f32>().ok()?;
            let y2 = parts[3].parse::<f32>().ok()?;
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                return None;
            }
            return Some(Self::CubicBezier(x1, y1, x2, y2));
        }
        if let Some(args) = t
            .strip_prefix("steps(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            if parts.is_empty() || parts.len() > 2 {
                return None;
            }
            let n = parts[0].parse::<u32>().ok()?;
            if n == 0 {
                return None;
            }
            let pos = match parts.get(1).copied() {
                None => StepPosition::JumpEnd,
                Some("start") | Some("jump-start") => StepPosition::JumpStart,
                Some("end") | Some("jump-end") => StepPosition::JumpEnd,
                Some("jump-none") => {
                    if n < 2 {
                        return None;
                    }
                    StepPosition::JumpNone
                }
                Some("jump-both") => StepPosition::JumpBoth,
                _ => return None,
            };
            return Some(Self::Steps(n, pos));
        }
        if let Some(args) = t
            .strip_prefix("linear(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            return parse_linear_easing_stops(args).map(Self::LinearStops);
        }
        None
    }

    /// CSS Transitions/Animations L1 — comma-list of timing functions.
    /// Пустые / невалидные entry — пропускаются (best-effort lenient).
    pub fn parse_list(s: &str) -> Vec<TimingFunction> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(TimingFunction::parse)
            .collect()
    }

    /// CSS Easing L1 §2 — компьютация eased progress.
    ///
    /// Принимает линейный input ratio `t ∈ [0, 1]` (input progress по spec)
    /// и возвращает output progress в [0, 1] для `Linear` и `Steps`. Для
    /// `CubicBezier` выход может выходить за `[0, 1]` (overshoot — клиент
    /// либо clamp-ает при применении к Length/Color, либо использует напрямую
    /// — например для `transform`).
    ///
    /// Вне `[0, 1]` входное `t` clamp-ается, как требует §2: «If input
    /// progress is less than 0, return 0. If input progress is greater
    /// than 1, return 1.» (реальные `fill-mode` / `direction` обрабатываются
    /// в animation engine ДО вызова progress().)
    pub fn progress(&self, t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        match self {
            TimingFunction::Linear => x,
            TimingFunction::CubicBezier(x1, y1, x2, y2) => cubic_bezier_progress(*x1, *y1, *x2, *y2, x),
            TimingFunction::Steps(n, position) => steps_progress(*n, *position, x),
            TimingFunction::LinearStops(points) => linear_stops_progress(points, x),
        }
    }
}

/// CSS Easing L1 §2.3 — cubic bezier easing. Кривая определена двумя
/// контрольными точками `(x1, y1)`, `(x2, y2)` с эндпоинтами `(0, 0)`,
/// `(1, 1)`. По заданному `x` (== input progress) находим параметр `u`,
/// такой что `bezier_axis(u, x1, x2) = x`, и возвращаем
/// `bezier_axis(u, y1, y2)` — eased output.
///
/// Алгоритм: Newton-Raphson (быстрая сходимость в большинстве кейсов) с
/// bisection fallback на случай, когда производная около нуля или Newton
/// расходится. Стандартный подход в Blink/WebKit/Gecko.
fn cubic_bezier_progress(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let u = solve_bezier_x(x1, x2, x);
    bezier_axis(u, y1, y2)
}

/// `B(u) = 3(1-u)²·u·c1 + 3(1-u)·u²·c2 + u³` для P0=(0,0), P3=(1,1).
fn bezier_axis(u: f32, c1: f32, c2: f32) -> f32 {
    let omu = 1.0 - u;
    3.0 * omu * omu * u * c1 + 3.0 * omu * u * u * c2 + u * u * u
}

/// `B'(u) = 3(1-u)²·c1 + 6(1-u)·u·(c2-c1) + 3u²·(1-c2)`.
fn bezier_axis_derivative(u: f32, c1: f32, c2: f32) -> f32 {
    let omu = 1.0 - u;
    3.0 * omu * omu * c1 + 6.0 * omu * u * (c2 - c1) + 3.0 * u * u * (1.0 - c2)
}

/// Solve `bezier_axis(u, x1, x2) = x` for `u ∈ [0, 1]`.
fn solve_bezier_x(x1: f32, x2: f32, x: f32) -> f32 {
    const EPS: f32 = 1e-6;
    let mut u = x;
    for _ in 0..8 {
        let xu = bezier_axis(u, x1, x2);
        let err = xu - x;
        if err.abs() < EPS {
            return u.clamp(0.0, 1.0);
        }
        let d = bezier_axis_derivative(u, x1, x2);
        if d.abs() < EPS {
            break;
        }
        u -= err / d;
        if !u.is_finite() {
            break;
        }
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..64 {
        let mid = (lo + hi) * 0.5;
        let xu = bezier_axis(mid, x1, x2);
        if (xu - x).abs() < EPS || (hi - lo) < EPS {
            return mid;
        }
        if xu < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

/// CSS Easing L1 §3.2 — `steps(n, <step-position>)` easing.
///
/// step-position определяет, сколько output-уровней и где «прыжки»:
/// - `jump-start` / `start`: n уровней `1/n, 2/n, ..., n/n`. Прыжок при t=0.
/// - `jump-end` / `end` (default): n+1 уровень `0/n, 1/n, ..., n/n`. Прыжок при t=1.
/// - `jump-none`: n уровней `0/(n-1), ..., (n-1)/(n-1) = 1`. Прыжков на границах нет.
/// - `jump-both`: n+2 уровня `1/(n+1), 2/(n+1), ..., (n+1)/(n+1) = 1`. Прыжки на обеих границах.
///
/// Для `t = 0` и `t = 1` корректно clamp-ается до границы output-диапазона.
fn steps_progress(n: u32, position: StepPosition, t: f32) -> f32 {
    let n_f = n as f32;
    let (raw_index, divisor, max_step) = match position {
        StepPosition::JumpStart => ((t * n_f).floor() + 1.0, n_f, n_f),
        StepPosition::JumpEnd => ((t * n_f).floor(), n_f, n_f),
        StepPosition::JumpNone => ((t * n_f).floor(), n_f - 1.0, n_f - 1.0),
        StepPosition::JumpBoth => ((t * n_f).floor() + 1.0, n_f + 1.0, n_f + 1.0),
    };
    let step = raw_index.max(0.0).min(max_step);
    (step / divisor).clamp(0.0, 1.0)
}

/// CSS Easing L2 §2.5.1 — канонизация stop-листа `linear(...)`.
///
/// Принимает содержимое скобок (без `linear(` / `)`); ожидает 2+ stop-а,
/// разделённых запятыми. Каждый stop = `<number>` + 0..2 `<percentage>`.
/// Возвращает `None` при синтаксической ошибке или < 2 stop-ов.
///
/// Алгоритм (§2.5.1):
/// 1. Парсим raw stops, преобразуем percentages → доли в `[0, 1]`.
/// 2. Расширяем stops с двумя lengths в две точки с одинаковым output.
/// 3. Первый stop без length получает input = 0, последний — max(1, largest).
/// 4. Каждый явный input clamp-ается до текущего `largest_input` (монотонность).
/// 5. Пропуски (точки без input) распределяются равномерно между соседними
///    известными inputs.
fn parse_linear_easing_stops(args: &str) -> Option<Vec<LinearEasingPoint>> {
    let parts = split_top_level_commas(args);
    if parts.len() < 2 {
        return None;
    }

    // Raw: (output, optional percentages already normalised to [0, 1]).
    let mut raw: Vec<(f32, Vec<f32>)> = Vec::with_capacity(parts.len());
    for stop in &parts {
        let stop = stop.trim();
        if stop.is_empty() {
            return None;
        }
        let tokens: Vec<&str> = stop.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > 3 {
            return None;
        }
        let output = tokens[0].parse::<f32>().ok()?;
        if !output.is_finite() {
            return None;
        }
        let mut lengths: Vec<f32> = Vec::new();
        for tok in &tokens[1..] {
            let stripped = tok.strip_suffix('%')?;
            let pct = stripped.parse::<f32>().ok()?;
            if !pct.is_finite() {
                return None;
            }
            lengths.push(pct / 100.0);
        }
        raw.push((output, lengths));
    }

    // Step 1 + 3 + 4: build points list with optional inputs and clamp by
    // largest_input для монотонности (spec: «whichever is greater»).
    let last_idx = raw.len() - 1;
    let mut points: Vec<(f32, Option<f32>)> = Vec::new();
    let mut largest = f32::NEG_INFINITY;
    for (i, (output, lengths)) in raw.iter().enumerate() {
        if lengths.is_empty() {
            if i == 0 {
                points.push((*output, Some(0.0)));
                largest = 0.0;
            } else if i == last_idx {
                let v = 1.0_f32.max(largest);
                points.push((*output, Some(v)));
                largest = v;
            } else {
                points.push((*output, None));
            }
        } else {
            let first_len = lengths[0].max(largest);
            points.push((*output, Some(first_len)));
            largest = first_len;
            if lengths.len() == 2 {
                let second_len = lengths[1].max(largest);
                points.push((*output, Some(second_len)));
                largest = second_len;
            }
        }
    }

    // Step 5: distribute `None` runs evenly between surrounding known inputs.
    // По §2.5.1 первая и последняя точки гарантированно получают input
    // в шагах 3-4, поэтому None-run всегда окружён двумя Some-границами.
    let mut i = 0;
    while i < points.len() {
        if points[i].1.is_some() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < points.len() && points[j].1.is_none() {
            j += 1;
        }
        // i..j — диапазон None; prev и next — соседние Some.
        let prev = points[i - 1].1?;
        let next = points[j].1?;
        let span = next - prev;
        let count = (j - i) as f32 + 1.0;
        for (k, idx) in (i..j).enumerate() {
            let frac = (k as f32 + 1.0) / count;
            points[idx].1 = Some(prev + frac * span);
        }
        i = j;
    }

    Some(
        points
            .into_iter()
            .map(|(output, input)| LinearEasingPoint {
                output,
                input: input.unwrap_or(0.0),
            })
            .collect(),
    )
}

/// CSS Easing L2 §2.5.2 — вычисление output функции `linear(...)`.
///
/// `points` — канонизованный список из `parse_linear_easing_stops` (inputs
/// монотонно неубывают). `t ∈ [0, 1]` — input progress (уже clamp-нутый
/// вызывающим `progress()`). Алгоритм:
///
/// - Меньше первого input — возвращаем output первой точки.
/// - Больше-или-равно последнему input — output последней (включая
///   `t == 1.0` ровно).
/// - Иначе ищем первую пару соседних точек `[A, B]` такую, что
///   `A.input ≤ t < B.input`, и линейно интерполируем. Discontinuity
///   (одинаковые inputs у соседних точек) обрабатывается возвратом
///   output левой точки — пара выбирается по first-match, поэтому
///   при `t == A.input` мы попадём на левую сторону скачка.
fn linear_stops_progress(points: &[LinearEasingPoint], t: f32) -> f32 {
    match points.len() {
        0 => t,
        1 => points[0].output,
        _ => {
            let first = points[0];
            let last = points[points.len() - 1];
            if t < first.input {
                return first.output;
            }
            if t >= last.input {
                return last.output;
            }
            for w in points.windows(2) {
                let a = w[0];
                let b = w[1];
                if a.input <= t && t < b.input {
                    let span = b.input - a.input;
                    if span <= f32::EPSILON {
                        return a.output;
                    }
                    let local = (t - a.input) / span;
                    return a.output + local * (b.output - a.output);
                }
            }
            last.output
        }
    }
}

/// CSS Easing L1 §3 — позиция шага в `steps()`. Default по spec — `jump-end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepPosition {
    /// `jump-start` (alias `start`) — первый прыжок на t=0,
    /// последний шаг достигает 1 - 1/n.
    JumpStart,
    /// `jump-end` (alias `end`) — первый шаг на t > 0, последний прыжок
    /// на t=1. Default.
    #[default]
    JumpEnd,
    /// `jump-none` — `n` шагов, ни один на границе. Требует n ≥ 2.
    JumpNone,
    /// `jump-both` — n+1 шагов, оба на границах t=0 и t=1.
    JumpBoth,
}

/// CSS Animations L1 §3.5 — `animation-iteration-count`. Либо число
/// (может быть дробным; отрицательные значения трактуются как невалидные),
/// либо ключевое слово `infinite`. Default = `Finite(1.0)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IterationCount {
    Finite(f32),
    Infinite,
}

impl Default for IterationCount {
    fn default() -> Self {
        IterationCount::Finite(1.0)
    }
}

impl IterationCount {
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("infinite") {
            return Some(Self::Infinite);
        }
        let n = t.parse::<f32>().ok()?;
        if n.is_finite() && n >= 0.0 {
            Some(Self::Finite(n))
        } else {
            None
        }
    }

    pub fn parse_list(s: &str) -> Vec<IterationCount> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(IterationCount::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.6 — `animation-direction`. Default = `Normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationDirection {
    /// Прямое воспроизведение каждой итерации (0 → 100%).
    #[default]
    Normal,
    /// Обратное воспроизведение (100% → 0).
    Reverse,
    /// Чётные итерации normal, нечётные reverse.
    Alternate,
    /// Чётные reverse, нечётные normal.
    AlternateReverse,
}

impl AnimationDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "reverse" => Some(Self::Reverse),
            "alternate" => Some(Self::Alternate),
            "alternate-reverse" => Some(Self::AlternateReverse),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationDirection> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationDirection::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.7 — `animation-fill-mode`. Default = `None`.
/// Определяет, применяются ли значения keyframes до начала и/или после
/// окончания анимации.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationFillMode {
    /// До начала и после конца — используется computed-style без keyframes.
    #[default]
    None,
    /// После окончания — последняя keyframe сохраняется.
    Forwards,
    /// До начала — первая keyframe применяется.
    Backwards,
    /// Both `forwards` и `backwards` одновременно.
    Both,
}

impl AnimationFillMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "forwards" => Some(Self::Forwards),
            "backwards" => Some(Self::Backwards),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationFillMode> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationFillMode::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.8 — `animation-play-state`. Default = `Running`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationPlayState {
    /// Анимация идёт. Default.
    #[default]
    Running,
    /// Пауза — текущее значение фиксируется.
    Paused,
}

impl AnimationPlayState {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationPlayState> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationPlayState::parse)
            .collect()
    }
}

/// CSS Scroll-Driven Animations L1 §3.3 — `animation-timeline` CSS value.
///
/// Parsed from `animation-timeline: auto | scroll([axis] [scroller]) | view([axis]) | <custom-ident>`.
/// Stored per-animation parallel to `animation_names`. Resolution to a concrete
/// `ScrollTimeline` / `ViewTimeline` happens at runtime in the animation scheduler.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum AnimationTimeline {
    /// Default: time-driven animation (normal `@keyframes` clock).
    #[default]
    Auto,
    /// `scroll([<axis>] [nearest | root | self])` — scroll container progress.
    /// `nearest: true` = nearest scroll ancestor (default); `false` = root viewport.
    Scroll { axis: ScrollAxis, nearest: bool },
    /// `view([<axis>])` — element visibility in scroll container (cover range).
    View { axis: ScrollAxis },
    /// `<custom-ident>` — matched against `scroll-timeline-name` / `view-timeline-name`
    /// at runtime.
    Named(String),
}

/// CSS-wide keywords (CSS Cascade L4 §7) — применимы к любому свойству.
/// - `Inherit` — взять computed value родителя.
/// - `Initial` — взять initial value свойства из спецификации.
/// - `Unset` — для inherited-свойств = `Inherit`, для non-inherited = `Initial`.
/// - `Revert` — откатиться к значению, которое было бы у свойства без
///   author/user-правил, то есть к UA-стилю для этого элемента (User origin
///   в Lumen не выделен отдельно от UA). Источник — снэпшот `ComputedStyle`,
///   снятый в `compute_style` сразу после `ua_*`/`apply_ua_*`/presentational-hint
///   пассов и до применения matched-деклараций (`ua_baseline`). Если у
///   свойства нет UA-хинта, снэпшот совпадает с обычным inherited/initial —
///   тогда `Revert` ведёт себя как `Unset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssWideKeyword {
    Inherit,
    Initial,
    Unset,
    Revert,
}

/// ASCII case-insensitive проверка значения декларации на CSS-wide keyword.
/// Любое из четырёх ключевых слов в любом регистре, с trim-ом whitespace,
/// возвращает соответствующий `Some(...)`. Иначе — `None`.
pub fn parse_css_wide_keyword(value: &str) -> Option<CssWideKeyword> {
    let t = value.trim();
    if t.eq_ignore_ascii_case("inherit") {
        Some(CssWideKeyword::Inherit)
    } else if t.eq_ignore_ascii_case("initial") {
        Some(CssWideKeyword::Initial)
    } else if t.eq_ignore_ascii_case("unset") {
        Some(CssWideKeyword::Unset)
    } else if t.eq_ignore_ascii_case("revert") {
        Some(CssWideKeyword::Revert)
    } else {
        None
    }
}

/// Copy-on-write map of a node's CSS custom properties (`--name` → raw source
/// text), as carried by [`ComputedStyle::custom_props`].
///
/// **Why a dedicated type instead of a plain `HashMap`** (BUG-341 S9): CSS
/// Variables L1 makes *every* custom property inherited, so `compute_style`
/// copies the parent's whole map into each child. With the 30 custom properties
/// `assets/chrome/chrome.html` declares, that copy alone measured 3.7–4.7 µs per
/// node against 0.31–0.46 µs for a node with an empty map — i.e. the map, not
/// the 302 other `ComputedStyle` fields, dominated the cascade. Behind an
/// [`Arc`] the inherit step is a refcount bump, and only the handful of nodes
/// that actually declare a `--name` pay a real copy, through
/// [`make_mut`](Self::make_mut).
///
/// The same sharing makes [`PartialEq`] cheap: two styles that inherited their
/// properties from a common ancestor compare in one pointer comparison, which is
/// what `graft_geometry`'s per-box style comparison relies on. The fast path is
/// spelled out here rather than left to `Arc`'s own (unspecified) pointer
/// short-circuit, so the cost is a property of this type and not of a standard
/// library implementation detail.
///
/// Reads go through [`Deref`] to `HashMap`, so `.get`/`.contains_key`/`.values`
/// and `&props` where a `&HashMap` is expected all work unchanged.
#[derive(Debug, Clone)]
pub struct CustomProps(Arc<HashMap<String, String>>);

impl CustomProps {
    /// Returns a mutable reference to the underlying map, cloning it first if
    /// (and only if) another `ComputedStyle` still shares it — the copy-on-write
    /// half of this type. Call sites that only read must not use this: an
    /// unconditional `make_mut` on every node would reintroduce exactly the
    /// per-node clone this type exists to remove.
    pub fn make_mut(&mut self) -> &mut HashMap<String, String> {
        Arc::make_mut(&mut self.0)
    }

    /// True when both sides are the very same allocation, i.e. one was cloned
    /// from the other with no intervening write. Equal-but-unshared maps return
    /// `false` — this is an identity check, not equality.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Address of the shared map, for callers that memoise per unique
    /// allocation rather than per node — see `collect_custom_properties`,
    /// which resolves each distinct map's `var()` chains exactly once and
    /// hands every node inheriting it the same `Arc`. Never dereferenced:
    /// the pointer is a map-identity key, nothing more.
    pub fn as_ptr(&self) -> *const HashMap<String, String> {
        Arc::as_ptr(&self.0)
    }

    /// The shared map itself, cloned as an `Arc` (a refcount bump, not a copy).
    /// Lets an embedder publish one allocation for every node that inherits it.
    pub fn shared(&self) -> Arc<HashMap<String, String>> {
        Arc::clone(&self.0)
    }
}

impl Default for CustomProps {
    /// The empty map is a process-wide singleton, so every node in a document
    /// that declares no custom property at all shares one allocation and
    /// compares by pointer.
    fn default() -> Self {
        static EMPTY: OnceLock<Arc<HashMap<String, String>>> = OnceLock::new();
        Self(Arc::clone(EMPTY.get_or_init(|| Arc::new(HashMap::new()))))
    }
}

impl Deref for CustomProps {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &HashMap<String, String> {
        &self.0
    }
}

impl PartialEq for CustomProps {
    /// Pointer identity first (the overwhelmingly common case for inherited
    /// maps), full map comparison only for independently built maps.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Eq for CustomProps {}

impl From<HashMap<String, String>> for CustomProps {
    fn from(map: HashMap<String, String>) -> Self {
        Self(Arc::new(map))
    }
}

impl FromIterator<(String, String)> for CustomProps {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self(Arc::new(iter.into_iter().collect()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub text_align: TextAlign,
    /// CSS Text L3 §7.2 — `text-align-last`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; применение при line layout — deferred.
    pub text_align_last: TextAlignLast,
    /// CSS Writing Modes L3 §2.1 — направление inline-потока. Inherited.
    /// Задаёт paragraph embedding level для UBA и разрешает логические
    /// `text-align: start|end`. См. [`Direction`] для подробностей.
    pub direction: Direction,
    /// CSS Writing Modes L4 §2.2 — участие бокса в UBA. NOT inherited.
    /// Initial: `Normal`. См. [`UnicodeBidi`].
    pub unicode_bidi: UnicodeBidi,
    pub color: Color,
    /// Цветовое пространство, в котором объявлен `color` (CSS Color L4 §10).
    /// Используется renderer-ом для точной передачи wide-gamut цветов в GPU.
    pub color_space: ColorSpace,
    pub background_color: Option<CssColor>,
    pub font_size: f32,
    /// CSS Viewport L1 §5 — the element's *effective* `zoom`: its own declared
    /// `zoom` multiplied by every ancestor's, since `zoom` compounds down the
    /// tree. Initial (and the value for a page that never declares `zoom`):
    /// `1.0`, which makes every scaling site below a no-op.
    ///
    /// Unlike `transform: scale()`, `zoom` affects *layout*: it multiplies the
    /// computed value of the element's absolute lengths and font-size, so the
    /// box genuinely occupies less space rather than merely being painted
    /// smaller. Applied by [`apply_zoom_to_lengths`] after the main cascade
    /// pass; relative units need no separate handling because they resolve
    /// against bases (`font_size`, the containing block) that are themselves
    /// already zoomed.
    pub effective_zoom: f32,
    pub line_height: f32,
    /// CSS2 §10.8.1 / CSS Fonts L5 §4 — whether `line-height` was specified as a
    /// relative value (`normal` or a unitless `<number>`) that scales with the
    /// used font-size, vs. an absolute `<length>`/`<percentage>` whose computed
    /// line box is frozen. `line_height` is always stored as a ratio (×font-size);
    /// this flag records which kind it was so `font-size-adjust` (which mutates the
    /// used font-size post-cascade) can keep an absolute line box constant instead
    /// of re-scaling it. Initial: `true` (`normal` is relative). Inherited with
    /// `line_height`.
    pub line_height_is_relative: bool,
    /// CSS Rhythmic Sizing L1 §2 — `line-height-step` step unit in px.
    /// When `> 0`, each line box's used height is rounded up to the closest
    /// multiple of this value and the extra space is distributed as half-leading.
    /// `0.0` (initial) disables stepping. Resolved to absolute px at parse time
    /// (em/rem relative to the element's own font-size). Inherited.
    pub line_height_step: f32,
    pub font_style: FontStyle,
    pub font_weight: FontWeight,
    /// CSS Fonts L4 §6.2 — font-variant-caps (весь набор значений). Inherited.
    pub font_variant_caps: FontVariantCaps,
    /// CSS Fonts L4 §6.6 — font-variant-emoji. Inherited. На выбор глифа пока
    /// не влияет — см. [`FontVariantEmoji`].
    pub font_variant_emoji: FontVariantEmoji,
    /// CSS Fonts L4 §2.5 — font-stretch (десятые доли процента; normal = 1000).
    /// Inherited.
    pub font_stretch: FontStretch,
    /// CSS Fonts L4 §3.1 — font-family как приоритизированный список имён.
    /// Inherited. Phase 0: рендерер пока всегда использует Inter, но layout
    /// уже хранит и распространяет список — задел под будущий font matcher.
    /// Generic-family имена (`serif`, `sans-serif`, `monospace`, `cursive`,
    /// `fantasy`, `system-ui`) сохраняются в этом же списке как обычные строки.
    /// Пустой Vec = inherited / default.
    pub font_family: Vec<String>,
    /// CSS Fonts L4 §7 — `font-variation-settings`. Inherited.
    /// Initial: пустой Vec (эквивалентно `normal`). Renderer нормализует
    /// через fvar + avar при растеризации глифов.
    pub font_variation_settings: Vec<FontVariationSetting>,
    /// CSS Fonts L3 §6 — `font-feature-settings`. Inherited.
    /// Initial: пустой Vec (эквивалентно `normal`). Шейпер (lumen-font)
    /// накладывает записи поверх default-набора OpenType-фич: value 0
    /// выключает фичу, ≥1 включает.
    pub font_feature_settings: Vec<FontFeatureSetting>,
    /// CSS Fonts L4 §11.3 — `font-palette`. Inherited. Initial: `Normal`
    /// (default CPAL palette). Selects the color palette for COLR color
    /// glyphs; no effect on monochrome text.
    pub font_palette: FontPalette,
    /// Resolved `@font-palette-values` data when [`Self::font_palette`] is
    /// `Custom` — resolved at the end of `compute_style` against the
    /// stylesheet (paint builds the display list from `ComputedStyle` alone
    /// and has no stylesheet access). `None` for keyword values and unknown
    /// palette names (spec: behaves as `normal`).
    pub font_palette_resolved: Option<ResolvedFontPalette>,
    /// CSS Fonts L4 §7.12 — `font-optical-sizing: auto | none`. Inherited.
    /// `auto` (initial): renderer injects `opsz = font_size` variation axis.
    pub font_optical_sizing: FontOpticalSizing,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    /// CSS Text L4 §3.1 — `white-space-collapse`. Inherited. Longhand-компонента
    /// `white-space`; хранится для каскада/наследования, layout читает
    /// эффективное `white_space` (пересчитывается через [`WhiteSpace::combine`]).
    pub white_space_collapse: WhiteSpaceCollapse,
    /// CSS Text L3 §7.1: отступ перед первой строкой inline-content.
    /// Inherited. Typed `Length`; `%` = % cb_width, резолвится при layout.
    pub text_indent: Length,
    /// CSS Text L3 §11.2: дополнительное расстояние между каждой парой
    /// символов и между словами (resolved px). Inherited. Может быть
    /// отрицательным (сжимает текст). Применяется в wrap_inline_run при
    /// расчёте ширин.
    pub letter_spacing: f32,
    /// CSS Text L3 §11.3: дополнительное расстояние **между словами**
    /// (resolved px). Inherited. В отличие от `letter-spacing`, добавляется
    /// только на word-boundary, не между всеми символами. Может быть
    /// отрицательным.
    pub word_spacing: f32,
    pub text_decoration_line: TextDecorationLine,
    /// CSS Text Decoration L3 §3 — `text-decoration-color`. `CurrentColor`
    /// означает «использовать style.color при рендеринге» (initial value по
    /// spec). Inherited через каскад (как и `text-decoration-line` в Phase 0
    /// — см. decisions log).
    pub text_decoration_color: CssColor,
    /// CSS Text Decoration L3 §2.2 — `text-decoration-style`. Initial: Solid.
    /// Inherited через каскад (Phase 0; см. doc на [`TextDecorationStyle`]).
    pub text_decoration_style: TextDecorationStyle,
    /// CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Initial: Auto.
    /// Inherited через каскад (Phase 0; см. doc на [`TextDecorationThickness`]).
    pub text_decoration_thickness: TextDecorationThickness,
    /// CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Inherited.
    /// Initial: `None` (нет emphasis marks). Phase 0 layout: parse+store;
    /// real rendering поверх каждого глифа — задача P2.
    pub text_emphasis_style: TextEmphasisStyle,
    /// CSS Text Decoration L4 §5.4 — `text-emphasis-color`. Inherited.
    /// Initial: `CurrentColor` — разрешается в `style.color` при рендере.
    pub text_emphasis_color: CssColor,
    /// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Inherited.
    /// Initial: `OverRight` (horizontal writing-mode).
    pub text_emphasis_position: TextEmphasisPosition,
    /// CSS Text Decoration L3 §6.1 / L4 §5.1 — `text-underline-position`.
    /// Inherited. Initial: `Auto`. Controls whether underline is placed below
    /// the alphabetic baseline (`Under`) or uses font metrics (`FromFont`).
    pub text_underline_position: TextUnderlinePosition,
    /// CSS Text Decoration L4 §5.3 — `text-underline-offset`. Inherited.
    /// Initial: `None` (auto ≡ 0). `Some(px)` = additional offset added to
    /// the intrinsic underline position. Positive shifts down (away from text).
    pub text_underline_offset: Option<f32>,
    /// CSS Text Decoration L4 §3.5 — `text-decoration-skip-ink`. Inherited.
    /// Initial: `Auto`. Controls whether underlines skip over glyph descenders.
    pub text_decoration_skip_ink: TextDecorationSkipInk,
    /// Явная ширина (CSS `width`). `None` = auto. Typed `Length`; `%`
    /// резолвится при layout с known cb_width.
    pub width: Option<Length>,
    /// Явная высота (CSS `height`). `None` = auto.
    pub height: Option<Length>,
    /// CSS 2.1 §10.4 — `min-width`. `None` = initial/auto (≡ 0).
    pub min_width: Option<Length>,
    /// CSS 2.1 §10.4 — `max-width`. `None` = `none` (без ограничения).
    pub max_width: Option<Length>,
    /// CSS 2.1 §10.4 — `min-height`. `None` = initial/auto (≡ 0).
    pub min_height: Option<Length>,
    /// CSS 2.1 §10.4 — `max-height`. `None` = `none`.
    pub max_height: Option<Length>,
    /// CSS 2.1 §8.3 — внешние отступы. `Auto` для `margin: auto` (centering).
    /// `%` = % cb_width, резолвится при layout. Initial = `Length(Px(0.0))`.
    pub margin_top: LengthOrAuto,
    pub margin_right: LengthOrAuto,
    pub margin_bottom: LengthOrAuto,
    pub margin_left: LengthOrAuto,
    /// CSS 2.1 §8.4 — внутренние отступы. `%` = % cb_width, резолв при
    /// layout. Initial = `Px(0.0)`.
    pub padding_top: Length,
    pub padding_right: Length,
    pub padding_bottom: Length,
    pub padding_left: Length,
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,
    /// Initial = `CurrentColor` (spec: initial value of border-color IS currentColor).
    pub border_top_color: CssColor,
    pub border_right_color: CssColor,
    pub border_bottom_color: CssColor,
    pub border_left_color: CssColor,
    pub box_sizing: BoxSizing,
    /// CSS Positioned Layout L3 §3 — `position`. Не наследуется.
    /// Default `Static`. Используется для stacking context (§9.10) и layout.
    pub position: Position,
    /// CSS Positioned Layout L3 §4 — inset properties. Не наследуются.
    /// `auto` = не задано (участвует в shrink-to-fit или оставляет edge на месте).
    pub top: LengthOrAuto,
    pub right: LengthOrAuto,
    pub bottom: LengthOrAuto,
    pub left: LengthOrAuto,
    /// CSS Positioned Layout L3 §9.3 — `z-index: auto | <integer>`. Не
    /// наследуется. `None` = `auto` (stacking context создаётся только если
    /// другие триггеры в §9.10 совпали). `Some(n)` = явный integer; для
    /// positioned- и flex/grid-item элементов это запускает создание
    /// stacking context.
    pub z_index: Option<i32>,
    /// CSS 2.1 §9.5.1 — `float`. Не наследуется. `Left`/`Right` выводят
    /// элемент из нормального потока. `None` — нормальный поток.
    pub float_side: FloatSide,
    /// CSS 2.1 §9.5.2 — `clear`. Не наследуется. Определяет, мимо каких
    /// float-ов блок обязан «пройти» перед размещением в потоке.
    pub clear: ClearSide,
    /// CSS Inline Layout L3 §5 — `initial-letter` высота буквицы в строках.
    /// Не наследуется. `1.0` = `normal` (без эффекта); `> 1.0` активирует
    /// буквицу высотой в это число строк (значение может быть дробным).
    pub initial_letter_size: f32,
    /// CSS Inline Layout L3 §5 — `initial-letter` «утопление» (sink): сколько
    /// строк нормального потока занимает буквица сбоку. Не наследуется.
    /// `0` = `auto`, выводится как `floor(initial_letter_size)`.
    pub initial_letter_sink: u32,
    /// CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется.
    /// `Isolate` создаёт stacking context.
    pub isolation: Isolation,
    /// CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется.
    /// Любое значение, отличное от `Normal`, создаёт stacking context.
    pub mix_blend_mode: MixBlendMode,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. Stored as `Length::Px` for
    /// absolute/em/rem values (resolved at cascade time) or `Length::Percent` for `%`
    /// (resolved at paint time against border-box width). Not inherited.
    pub border_top_left_radius: Length,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. See `border_top_left_radius`.
    pub border_top_right_radius: Length,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. See `border_top_left_radius`.
    pub border_bottom_right_radius: Length,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. See `border_top_left_radius`.
    pub border_bottom_left_radius: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. Equals x-radius for circular corners
    /// (`border-radius: 10px`); differs for elliptical (`border-radius: 10px / 20px`). `%` is
    /// resolved against border-box **height** at paint time. Not inherited.
    pub border_top_left_radius_y: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. See `border_top_left_radius_y`.
    pub border_top_right_radius_y: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. See `border_top_left_radius_y`.
    pub border_bottom_right_radius_y: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. See `border_top_left_radius_y`.
    pub border_bottom_left_radius_y: Length,
    /// CSS Display L3 §4 — visibility. Inherited.
    pub visibility: Visibility,
    /// CSS UI L4 §8.1 — cursor. Inherited.
    pub cursor: Cursor,
    /// CSS Backgrounds L3 §4.6 — список теней. Не наследуется. Пустой Vec
    /// = `none`.
    pub box_shadow: Vec<BoxShadow>,
    /// CSS Text Decoration L3 §4 — список теней текста. Inherited
    /// (отличается от box-shadow!). Пустой Vec = `none`.
    pub text_shadow: Vec<TextShadow>,
    /// CSS Overflow L3 — отдельные поля для X и Y. Не наследуются.
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    /// CSS Overflow L3 §overflow-clip-margin — расширяет clip region при overflow:clip.
    /// Default: 0px. Не наследуется.
    pub overflow_clip_margin: Option<Length>,
    /// CSS UI L4 §10.1 — text-overflow. Не наследуется.
    pub text_overflow: TextOverflow,
    /// CSS Color L3 §3.2 — opacity (0.0..=1.0). Не наследуется. Работает
    /// как alpha всего слоя (включая фон, бордер, текст и потомков). В
    /// Phase 0 layout только хранит — paint пока не применяет alpha
    /// blending этого уровня; индивидуальные альфы в `color`/`background`
    /// продолжают работать.
    pub opacity: f32,
    /// CSS Basic UI L4 §5: outline. В отличие от border не сдвигает соседей
    /// и не учитывается в width/height (рисуется поверх / снаружи коробки).
    /// Не наследуется.
    ///
    /// Initial computed `outline-width` = `medium` (3 px по UA convention);
    /// **used** value становится 0 при `outline-style: none` (CSS 2.1
    /// §17.6.1 / Basic UI L4 §5.2) — см. `outline_used_width()`. Поэтому
    /// «outline по умолчанию невидим» обеспечивается style=None, а не
    /// width=0.
    pub outline_width: f32,
    pub outline_style: OutlineStyle,
    pub outline_color: OutlineColor,
    /// CSS Basic UI L4 §5.5 — outline-offset. Положительное — дальше от
    /// бокса, отрицательное — внутрь. Typed `Length`; резолвится при paint.
    pub outline_offset: Length,
    /// CSS UI L4 §6.1 — accent-color. Цвет встроенных form widgets
    /// (checkbox, radio, range, progress). `None` = `auto` (UA default).
    /// Inherited. В Phase 0 layout только хранит — real применение появится
    /// вместе с form-widget рендерингом.
    pub accent_color: Option<Color>,
    /// CSS Color Adjustment L1 §3 — `color-scheme`. Inherited. Initial: `Normal`.
    /// Phase 0: parse + store; реальное переключение SystemColor / UA-тем — P2.
    pub color_scheme: ColorScheme,
    /// CSS Color Adjustment L1 §4 — `forced-color-adjust`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; применение при Forced Colors Mode — P2.
    pub forced_color_adjust: ForcedColorAdjust,
    /// CSS Variables L1 — custom properties (`--name`). Все custom properties
    /// inherited (спека: `all custom properties are inherited by default`).
    /// Ключ — полное имя с ведущими `--`, значение — сырой текст из source.
    /// Substitution `var(--name [, fallback])` делается lazy при применении
    /// обычных деклараций (см. `apply_declaration`).
    ///
    /// Shared copy-on-write (BUG-341 S9) — see [`CustomProps`] for why.
    pub custom_props: CustomProps,
    /// CSS Lists L3 §3 — `counter-reset: name [N]?`. Каждый element задаёт
    /// (имя-счётчика, начальное-значение). Не наследуется. Пустой `Vec`
    /// при отсутствии декларации или `counter-reset: none`. Реальное
    /// разрешение counter() в `content` pseudo-elements — отдельная задача
    /// (требует layout-time counter scoping walker).
    pub counter_reset: Vec<(String, i32)>,
    /// CSS Lists L3 §3 — `counter-increment: name [N]?`. Каждый element
    /// инкрементирует названный counter на N (default +1). Не наследуется.
    pub counter_increment: Vec<(String, i32)>,
    /// CSS Lists L3 §4 — `counter-set: name [N]?`. Каждый element
    /// устанавливает названный counter в N (default 0). Не наследуется.
    /// Применяется ПОСЛЕ counter-reset и counter-increment (порядок по spec).
    /// Если счётчика с таким именем нет в области видимости — создаёт его.
    pub counter_set: Vec<(String, i32)>,
    /// CSS Masking L1 §3 — `clip-path: <basic-shape> | none`. Не
    /// наследуется. Phase 0: parsing only — real geometric clipping
    /// в paint pipeline отложен.
    pub clip_path: Option<ClipPath>,
    /// CSS Transforms L1 §2 — `transform: <transform-list> | none`.
    /// Список функций — каждая `TransformFn` хранит параметры. Не
    /// наследуется.
    pub transform: Vec<TransformFn>,
    /// CSS Transforms L2 §2 — `translate: none | <tx> [<ty>]`.
    /// Individual translate in px; composed BEFORE `transform` in the final matrix.
    /// `None` = `none` (identity). Not inherited.
    pub translate: Option<(f32, f32)>,
    /// CSS Transforms L2 §2 — `rotate: none | <angle>`.
    /// Individual 2D rotation in radians; composed BEFORE `transform`.
    /// `None` = `none` (identity). Not inherited.
    pub rotate: Option<f32>,
    /// CSS Transforms L2 §2 — `scale: none | <sx> [<sy>]`.
    /// Individual scale factors; composed BEFORE `transform`.
    /// `None` = `none` (identity). Not inherited.
    pub scale: Option<(f32, f32)>,
    /// CSS Filter Effects L1 §3 — `filter: <filter-function-list> | none`.
    /// Список функций — blur/brightness/contrast/grayscale/etc. Не
    /// наследуется. Phase 0: parsing only.
    pub filter: Vec<FilterFn>,
    /// CSS Box Alignment L3 §8 — `row-gap` / `column-gap` для
    /// flex/grid container-ов. В пикселях (resolved). Default 0.
    /// Не наследуется. Phase 0: parsing only — real flex/grid algorithm
    /// не реализован, гарантированно gap не применяется.
    /// CSS Box Alignment L3 §8 — `row-gap` / `column-gap` для flex/grid.
    /// Typed `Length`; `%` = % cb_size. Default `Px(0)`. Phase 0: parse only.
    pub row_gap: Length,
    pub column_gap: Length,
    /// CSS Multi-column L1 §3.2 — `column-count: <integer> | auto`. `None`
    /// = `auto`. Phase 0: parsing only.
    pub column_count: Option<u32>,
    /// CSS Multi-column L1 §3.3 — `column-width: <length> | auto`. Typed.
    /// `None` = `auto`. Phase 0: parsing only.
    pub column_width: Option<Length>,
    /// CSS Multi-column L1 §4.1 — `column-rule-width` (px). Default 0.
    pub column_rule_width: f32,
    /// CSS Multi-column L1 §4.2 — `column-rule-style`. Default `None`
    /// (без линии — линия рисуется только если style != None и width > 0).
    pub column_rule_style: BorderStyle,
    /// CSS Multi-column L1 §4.3 — `column-rule-color`. Initial = `CurrentColor`.
    pub column_rule_color: CssColor,
    /// CSS Gap Decorations L1 — `gap-rule-width` (px). Default 0. Non-inherited.
    /// Thickness of the visual rule drawn in flex/grid/multicol gaps.
    pub gap_rule_width: f32,
    /// CSS Gap Decorations L1 — `gap-rule-style`. Default `None`. Non-inherited.
    /// Rule is only visible when style != None and width > 0.
    pub gap_rule_style: BorderStyle,
    /// CSS Gap Decorations L1 — `gap-rule-color`. Default `CurrentColor`. Non-inherited.
    pub gap_rule_color: CssColor,
    /// CSS Tables L2 §17.6 — `border-collapse`. Inherited. Default `Separate`.
    /// When `Collapse`, `border-spacing` has no effect and adjacent cell borders merge.
    pub border_collapse: BorderCollapse,
    /// CSS Tables L2 §17.6.1.1 — `empty-cells`. Inherited. Default `Show`.
    /// When `Hide`, a table cell with no in-flow content draws neither borders nor
    /// background. No effect under `border-collapse: collapse`.
    pub empty_cells: EmptyCells,
    /// CSS 2.1 §17.6 — `border-spacing: <length> [<length>]?`. Inherited. Default 0.
    /// Horizontal gap (px) between adjacent table cells in separate-border mode.
    /// Only applies when `border-collapse: separate` (CSS 2.1 default).
    pub border_spacing_h: f32,
    /// CSS 2.1 §17.6 — `border-spacing` vertical component (px). Inherited.
    /// Vertical gap between adjacent table rows in separate-border mode.
    pub border_spacing_v: f32,
    /// CSS Multi-column L1 §6.1 — `column-span: none | all`. По умолчанию
    /// `None` (False), `Some(true)` = `all` (элемент растягивается через
    /// все колонки). Не наследуется. Phase 0: parse+store.
    pub column_span_all: bool,
    /// CSS Multi-column L1 §6.2 — `column-fill: balance | auto`. `true`
    /// = balance (spec default — распределяет содержимое поровну между
    /// колонками), `false` = auto (заполняет колонки последовательно до
    /// высоты контейнера). Не наследуется.
    pub column_fill_balance: bool,
    /// CSS Fragmentation L3 §3.1 — `break-before`. Phase 0 — enum со
    /// значениями auto/avoid/always/page/column/region. Не наследуется.
    pub break_before: BreakValue,
    pub break_after: BreakValue,
    pub break_inside: BreakValue,
    /// CSS Sizing L4 §6.1 — `aspect-ratio: auto | <ratio> | auto <ratio>`.
    /// `None` = `auto` (UA выбирает). `Some((w, h))` = явное отношение
    /// W:H (например, 16:9 → (16.0, 9.0)). Не наследуется.
    /// Phase 0: parsing — real intrinsic-aspect-ratio enforcement
    /// требует layout-time pass.
    pub aspect_ratio: Option<(f32, f32)>,
    /// CSS Box Alignment L3 — alignment свойства для flex/grid items.
    /// Все не наследуются. Phase 0: parsing only.
    pub align_items: AlignValue,
    pub align_self: AlignValue,
    pub align_content: AlignValue,
    pub justify_items: AlignValue,
    pub justify_self: AlignValue,
    pub justify_content: AlignValue,
    /// CSS Backgrounds L3 §3 — стек фоновых слоёв. Первый элемент = верхний (рендерится поверх).
    /// Пустой Vec соответствует `background-image: none` без слоёв. `background-color` отдельно.
    pub background_layers: Vec<BackgroundLayer>,
    /// CSS Will Change L1. Список имён свойств для optimization hint.
    /// Пустой Vec = `auto` (default). Не наследуется.
    pub will_change: Vec<String>,
    /// CSS Pointer Events L1. Default `auto`. Не наследуется.
    pub pointer_events: PointerEvents,
    /// CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; обработка touch-жестов — P3 task.
    pub touch_action: TouchAction,
    /// CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; form-widget styling — P2/P3 task.
    pub appearance: Appearance,
    /// CSS Basic UI L4 §4.4 — `field-sizing`. NOT inherited. Initial: `Fixed`.
    /// When `Content`, UA-default dimensions are suppressed and `lay_out_box` calls
    /// `field_sizing_content_intrinsic()` to size the control from its text.
    pub field_sizing: FieldSizing,
    /// CSS UI L4 §6.2 — `user-select`. Inherited (по спеке).
    pub user_select: UserSelect,
    /// CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`.
    /// Drag-resize UI (grip hit-test, apply): `crates/shell/src/main.rs` (CC-CSS-4).
    pub resize: Resize,
    /// CSS Overflow L3 — `scroll-behavior`. Inherited.
    pub scroll_behavior: ScrollBehavior,
    /// CSS Scroll Snap L1 §3.1 — `scroll-snap-type`. Не наследуется.
    pub scroll_snap_type: ScrollSnapType,
    /// CSS Scroll Snap L1 §6.1 — `scroll-snap-align`. Не наследуется.
    pub scroll_snap_align: ScrollSnapAlign,
    /// CSS Scroll Snap L1 §6.2 — `scroll-snap-stop`. Не наследуется.
    pub scroll_snap_stop: ScrollSnapStop,
    /// CSS Scroll Snap L1 §4 — `scroll-margin-*` (resolved px).
    pub scroll_margin_top: f32,
    pub scroll_margin_right: f32,
    pub scroll_margin_bottom: f32,
    pub scroll_margin_left: f32,
    /// CSS Scroll Snap L1 §4 — `scroll-padding-*` (resolved px).
    pub scroll_padding_top: f32,
    pub scroll_padding_right: f32,
    pub scroll_padding_bottom: f32,
    pub scroll_padding_left: f32,
    /// CSS Overscroll Behavior L1 §2 — `overscroll-behavior-x`. Не наследуется.
    pub overscroll_behavior_x: OverscrollBehavior,
    pub overscroll_behavior_y: OverscrollBehavior,
    /// CSS Text L3 §10.1 — `tab-size: <integer> | <length>`. Inherited.
    /// В пикселях если length; для integer хранится как число × 8 (default
    /// 8 spaces — стандартный default). Default 8 spaces = 64px при 8px-space.
    pub tab_size: f32,
    /// CSS UI L4 §6.3 — `caret-color: auto | <color>`. Inherited.
    /// `None` = auto (UA выбирает). `Some(color)` — явный цвет.
    pub caret_color: Option<Color>,
    /// CSS Text L3 §5.2 — `overflow-wrap: normal | break-word | anywhere`.
    /// Inherited. Default `Normal`.
    pub overflow_wrap: OverflowWrap,
    /// CSS Text L3 §5.1 — `word-break: normal | keep-all | break-all |
    /// break-word`. Inherited. Default `Normal`.
    pub word_break: WordBreak,
    /// CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`.
    /// Phase 0: parse + store; CJK line-breaking — отдельная задача.
    pub line_break: LineBreak,
    /// CSS Text L3 §6 — `hyphens: none | manual | auto`. Inherited.
    /// Default `Manual`.
    pub hyphens: Hyphens,
    /// CSS Transforms L1 §6 — `transform-origin: <x> <y> <z>?`.
    /// Default `50% 50% 0` — центр бокса. Percentages resolved at display-list
    /// time against border-box width/height (box dimensions known only after layout).
    pub transform_origin: (PositionComponent, PositionComponent, f32),
    /// CSS Transforms L2 §4 — `perspective: <length> | none`.
    /// `None` = no perspective; `Some(px)` = distance to camera.
    pub perspective: Option<f32>,
    /// CSS Transforms L2 §4 — `perspective-origin: <x> <y>`.
    /// Default `50% 50%`. Resolved at display-list time against border-box.
    pub perspective_origin: (PositionComponent, PositionComponent),
    /// CSS Transforms L2 §6 — `transform-style: flat | preserve-3d`.
    /// `Preserve3d` makes children participate in 3D rendering context.
    pub transform_style: TransformStyle,
    /// CSS Transforms L2 §5.1 — `backface-visibility: visible | hidden`.
    /// Stored on ComputedStyle; actual back-face culling deferred until
    /// a 3D rendering context exists (3D projection ⬜).
    pub backface_visibility: BackfaceVisibility,
    /// CSS Lists L3 §2.1 — `list-style-type`.
    pub list_style_type: ListStyleType,
    /// CSS Lists L3 §2.3 — `list-style-position`.
    pub list_style_position: ListStylePosition,
    /// CSS Lists L3 §2.2 — `list-style-image: url(...) | none`.
    pub list_style_image: Option<String>,
    /// CSS Transitions L1 §3 — `transition-property: none | all | <ident>+`.
    pub transition_properties: Vec<String>,
    /// CSS Transitions L1 §3 — `transition-duration: <time>+` в секундах.
    pub transition_durations: Vec<f32>,
    /// CSS Transitions L1 §3 — `transition-delay: <time>+` в секундах.
    pub transition_delays: Vec<f32>,
    /// CSS Transitions L1 §3 — `transition-timing-function: <easing-function>+`.
    /// Per-property list; если длина короче `transition_properties`, при
    /// resolve-time spec велит cyclically reuse последний элемент.
    pub transition_timing_functions: Vec<TimingFunction>,
    /// CSS Transitions L2 §3 — `transition-fill-mode: <single-animation-fill-mode>#`.
    /// Parallels animation-fill-mode; используется для сохранения значений
    /// в delay-периоде (backwards) и после завершения (forwards).
    pub transition_fill_modes: Vec<AnimationFillMode>,
    /// CSS Animations L1 §3.1 — `animation-name: none | <keyframes-name>#`.
    /// `none` хранится как пустой `Vec` (нет анимаций); иначе список имён.
    /// Имя соответствует `@keyframes name { ... }` в [`Stylesheet`].
    pub animation_names: Vec<String>,
    /// CSS Animations L1 §3.2 — `animation-duration: <time>#`. Секунды.
    /// Параллельный список к `animation_names`; cyclically reuse при
    /// несовпадении длины (resolve в P1 п.3A scheduler).
    pub animation_durations: Vec<f32>,
    /// CSS Animations L1 §3.3 — `animation-timing-function: <easing-function>#`.
    pub animation_timing_functions: Vec<TimingFunction>,
    /// CSS Animations L1 §3.4 — `animation-delay: <time>#`. Секунды.
    /// Отрицательные значения допустимы и означают «анимация началась
    /// в прошлом» (используется для phase-offset нескольких анимаций).
    pub animation_delays: Vec<f32>,
    /// CSS Animations L1 §3.5 — `animation-iteration-count: <single-iteration-count>#`.
    pub animation_iteration_counts: Vec<IterationCount>,
    /// CSS Animations L1 §3.6 — `animation-direction: <single-animation-direction>#`.
    pub animation_directions: Vec<AnimationDirection>,
    /// CSS Animations L1 §3.7 — `animation-fill-mode: <single-animation-fill-mode>#`.
    pub animation_fill_modes: Vec<AnimationFillMode>,
    /// CSS Animations L1 §3.8 — `animation-play-state: <single-animation-play-state>#`.
    pub animation_play_states: Vec<AnimationPlayState>,
    /// CSS Scroll-Driven Animations L1 §3.3 — `animation-timeline: auto | scroll() | view() | <ident>#`.
    /// Non-inherited. Parallel list to `animation_names`. Default empty = all `Auto`.
    pub animation_timelines: Vec<AnimationTimeline>,
    /// CSS Scroll-Driven Animations L1 §3.1 — `scroll-timeline-name: none | <custom-ident>`.
    /// Non-inherited. Names this element as a scroll container for a named scroll timeline.
    pub scroll_timeline_name: Option<String>,
    /// CSS Scroll-Driven Animations L1 §3.2 — `scroll-timeline-axis: block | inline | x | y`.
    /// Non-inherited. Which axis drives the named scroll timeline. Default `Block`.
    pub scroll_timeline_axis: ScrollAxis,
    /// CSS Scroll-Driven Animations L1 §3.3 — `view-timeline-name: none | <custom-ident>`.
    /// Non-inherited. Names this element as a view-timeline subject.
    pub view_timeline_name: Option<String>,
    /// CSS Scroll-Driven Animations L1 §3.4 — `view-timeline-axis: block | inline | x | y`.
    /// Non-inherited. Which axis drives the named view timeline. Default `Block`.
    pub view_timeline_axis: ScrollAxis,
    /// CSS Masking L1 §4.9 — список слоёв маски (`mask-image` и все
    /// сопутствующие per-layer longhand-ы). Первый элемент = верхний слой,
    /// последний = нижний (тот же порядок, что у [`Self::background_layers`]).
    /// Пустой Vec = `mask: none`, маска не применяется. Не наследуется.
    pub mask_layers: Vec<MaskLayer>,
    /// CSS Scrollbars 1 — `scrollbar-width: auto | thin | none`.
    pub scrollbar_width: ScrollbarWidth,
    /// CSS Scrollbars 1 — `scrollbar-color: auto | <color> <color>`
    /// (thumb-color + track-color).
    pub scrollbar_color: Option<(Color, Color)>,
    /// CSS Overflow L3 — `scrollbar-gutter: auto | stable | stable both-edges`.
    pub scrollbar_gutter: ScrollbarGutter,
    /// CSS Content L3 §2.1 — `content`. Используется в pseudo-elements
    /// (`::before` / `::after`) и для counter()-разрешения. Phase 0:
    /// parsing + storage; реальные pseudo-elements в layout — отдельная
    /// большая задача. Default `Normal` (use element's box-tree).
    pub content: Content,
    /// CSS Images L3 §5.5 — `object-fit`. Применяется только к replaced
    /// elements (`<img>` и пр.). Не наследуется. Default `Fill`.
    pub object_fit: ObjectFit,
    /// CSS Images L3 §5.5 — `object-position`. Не наследуется. Default
    /// `50% 50%` (центр коробки).
    pub object_position: ObjectPosition,
    /// CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется.
    /// Default `Baseline`. Phase 0: parsing + storage; реальное применение
    /// (y_offset фрагмента в inline-flow и DrawText в paint) — отдельная
    /// задача с согласованием P2 (см. doc-comment на [`VerticalAlign`]).
    pub vertical_align: VerticalAlign,
    /// CSS Images L3 §6.1 — `image-rendering`. Inherited. Default `Auto`.
    /// Phase 0: parsing + storage; реальное переключение GPU sampler filter
    /// в `lumen-paint` (linear vs nearest-neighbour для `<img>` и background)
    /// — отдельная задача с согласованием P2.
    pub image_rendering: ImageRendering,
    /// CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited. Default `Row`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_direction: FlexDirection,
    /// CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited. Default `Nowrap`.
    /// Phase 0: parsing + storage; реальный multi-line flex — задача 4B.5.
    pub flex_wrap: FlexWrap,
    /// CSS Flexbox L1 §7.1 — `flex-grow`. Non-inherited. Default `0`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_grow: f32,
    /// CSS Flexbox L1 §7.2 — `flex-shrink`. Non-inherited. Default `1`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_shrink: f32,
    /// CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited. Default `Auto`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_basis: FlexBasis,
    /// CSS Flexbox L1 §5.4 — `order`. Non-inherited. Initial: `0`.
    /// Управляет порядком отображения flex-элементов внутри контейнера.
    pub order: i32,
    /// CSS Grid Layout L1 §7.2 — `grid-template-columns`. Non-inherited.
    /// Default `[]` (no explicit tracks). Parsed track-list.
    pub grid_template_columns: Vec<GridTrackSize>,
    /// CSS Grid Layout L1 §7.2 — `grid-template-rows`. Non-inherited.
    /// Default `[]` (no explicit tracks). Parsed track-list.
    pub grid_template_rows: Vec<GridTrackSize>,
    /// CSS Grid Layout L1 §7.2 — auto-fill/auto-fit repeat metadata for columns.
    /// `Some` when `grid-template-columns` contains `repeat(auto-fill|auto-fit, ...)`.
    /// Resolved at layout time via `resolve_auto_fill_fit_count`. Non-inherited. Default `None`.
    pub grid_template_col_auto_repeat: Option<GridRepeat>,
    /// CSS Grid Layout L1 §7.2 — auto-fill/auto-fit repeat metadata for rows.
    /// `Some` when `grid-template-rows` contains `repeat(auto-fill|auto-fit, ...)`.
    /// Resolved at layout time via `resolve_auto_fill_fit_count`. Non-inherited. Default `None`.
    pub grid_template_row_auto_repeat: Option<GridRepeat>,
    /// CSS Grid Layout L1 §7.3 — `grid-template-areas`. Non-inherited.
    /// Default `[]` (none). Outer vec = rows (top-to-bottom), inner vec = columns
    /// (left-to-right). Each string is a cell name; `"."` means unnamed cell.
    pub grid_template_areas: Vec<Vec<String>>,
    /// CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited. Default `Row`.
    pub grid_auto_flow: GridAutoFlow,
    /// CSS Masonry Layout §9 — `masonry-auto-flow`. Controls placement order in
    /// masonry containers. Non-inherited. Default `DefiniteFirst`.
    pub masonry_auto_flow: MasonryAutoFlow,
    /// CSS Grid Layout L1 §8.6 — `grid-auto-columns`. Non-inherited. Default `Auto`.
    pub grid_auto_columns: GridTrackSize,
    /// CSS Grid Layout L1 §8.6 — `grid-auto-rows`. Non-inherited. Default `Auto`.
    pub grid_auto_rows: GridTrackSize,
    /// CSS Grid Layout L1 §8.3 — `grid-column-start`. Non-inherited. Default `Auto`.
    pub grid_column_start: GridLine,
    /// CSS Grid Layout L1 §8.3 — `grid-column-end`. Non-inherited. Default `Auto`.
    pub grid_column_end: GridLine,
    /// CSS Grid Layout L1 §8.3 — `grid-row-start`. Non-inherited. Default `Auto`.
    pub grid_row_start: GridLine,
    /// CSS Grid Layout L1 §8.3 — `grid-row-end`. Non-inherited. Default `Auto`.
    pub grid_row_end: GridLine,
    /// CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited.
    /// Default `Wrap`. Phase 0: parsing + storage; реальная связка с
    /// inline-flow line-breaker-ом (когда `Nowrap` подавляет soft wraps
    /// и эмитит overflowing line, эквивалентно legacy `white-space: nowrap`)
    /// — отдельная задача рядом с типизацией white-space (P1 1B).
    pub text_wrap_mode: TextWrapMode,
    /// CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited.
    /// Default `Auto`. Phase 0: parsing + storage; реальная интерпретация
    /// `balance` / `pretty` / `stable` требует Knuth–Plass-style breaker-а
    /// и Unicode line-break tables — отложено до интеграции `UnicodeProvider`
    /// (provisional `icu4x`, P1 п.5).
    pub text_wrap_style: TextWrapStyle,
    /// CSS Overflow L4 / compat `-webkit-line-clamp` — максимальное число
    /// строк до обрезки текста. `None` = `none` (нет ограничения). Не
    /// наследуется. Phase 0: parsing + storage; реальное применение (truncate
    /// inline-flow после N-й строки и добавить ellipsis) — отдельная задача.
    pub line_clamp: Option<u32>,
    /// CSS Fragmentation L3 §3.3 — `orphans`: минимальное число строк в конце
    /// фрагмента перед page/column break (сколько строк должно остаться «внизу»
    /// после разрыва). Inherited. Initial: 2. Phase 0: parsing + storage;
    /// реальная фрагментация — отдельная задача.
    pub orphans: u32,
    /// CSS Fragmentation L3 §3.3 — `widows`: минимальное число строк в начале
    /// фрагмента после page/column break (сколько строк должно перенестись
    /// «наверх» нового фрагмента). Inherited. Initial: 2. Phase 0: parsing + storage.
    pub widows: u32,
    /// CSS Containment L3 §3 — `contain`. NOT inherited. Initial: `NONE`.
    /// Phase 0: parse + store; containment enforcement in layout/paint — deferred.
    pub contain: ContainFlags,
    /// CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`.
    /// Phase 0: parse + store; skip-content optimization — deferred.
    pub content_visibility: ContentVisibility,
    /// CSS Box Sizing L4 §5 — `contain-intrinsic-width`. NOT inherited. Initial: `None`.
    /// Placeholder inline-size used as the box's intrinsic width when the element
    /// is subject to size containment (`contain: size`, `content-visibility: hidden`,
    /// or `content-visibility: auto` while skipped off-screen). `None` = the CSS
    /// keyword `none` (no placeholder; content-based width collapses). The optional
    /// `auto` keyword (last-remembered size) is parsed but treated as the length —
    /// its presence is kept in [`Self::contain_intrinsic_width_auto`] for
    /// serialization only, since the computed value the CSSOM must report is the
    /// specified `auto? [none | <length>]`, not just the length.
    /// Stored as a content-box `Length`, resolved against the font-size at layout.
    pub contain_intrinsic_width: Option<Length>,
    /// CSS Box Sizing L4 §5 — the `auto` keyword of `contain-intrinsic-width`.
    /// Behaviourally ignored (no last-remembered size); kept so
    /// `getComputedStyle` can serialise `auto 1px` rather than `1px`
    /// (BUG-852).
    pub contain_intrinsic_width_auto: bool,
    /// CSS Box Sizing L4 §5 — `contain-intrinsic-height`. NOT inherited. Initial: `None`.
    /// Placeholder block-size under size containment. See `contain_intrinsic_width`.
    pub contain_intrinsic_height: Option<Length>,
    /// CSS Box Sizing L4 §5 — the `auto` keyword of `contain-intrinsic-height`.
    /// See [`Self::contain_intrinsic_width_auto`].
    pub contain_intrinsic_height_auto: bool,
    /// CSS Sizing L4 §4.5 — `interpolate-size`. **Inherited.** Initial: `NumericOnly`.
    /// Controls whether keyword sizes (`auto`, `min-content`, …) participate in
    /// transitions/animations. Read by `TransitionScheduler::sync()` to gate
    /// `height: auto` interpolation.
    pub interpolate_size: InterpolateSizeMode,
    /// CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`.
    /// Phase 0: parse + store; @container query matching — deferred.
    pub container_type: ContainerType,
    /// CSS Container Queries L1 §3.2 — `container-name`. NOT inherited. Initial: empty.
    /// `none` = empty vec. Each name is a `<custom-ident>`.
    pub container_name: Vec<String>,
    /// CSS Filter Effects L2 §2 — `backdrop-filter`. NOT inherited. Initial: empty Vec (none).
    /// Same filter functions as `filter`. Phase 0: parse + store; backdrop blur in paint — deferred.
    pub backdrop_filter: Vec<FilterFn>,
    /// CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`.
    /// Phase 0: parse + store; print rendering path — deferred.
    pub print_color_adjust: PrintColorAdjust,
    /// CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`.
    /// Wired: `box_tree::apply_font_size_adjust` rescales the used `font_size`
    /// to `size · adjust / aspect` (aspect = font x-height / em) before layout.
    pub font_size_adjust: FontSizeAdjust,
    /// CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`.
    /// Phase 0: parse + store; vertical layout — deferred.
    pub writing_mode: WritingMode,
    /// CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`.
    /// Phase 0: parse + store; glyph rotation — deferred.
    pub text_orientation: TextOrientation,
    /// CSS Ruby L1 §4 — `ruby-position`. Inherited. Initial: `Over`.
    /// Drives `lay_out_ruby`; `<ruby>` box-tree integration — deferred.
    pub ruby_position: RubyPosition,
    /// CSS Ruby L1 §4 — `ruby-align`. Inherited. Initial: `SpaceAround`.
    pub ruby_align: RubyAlign,
    /// CSS Ruby L1 §4 — `ruby-merge`. Inherited. Initial: `Separate`.
    pub ruby_merge: RubyMerge,
    /// MathML Core §2.1.1 — `math-style`. Inherited. Initial: `Normal`.
    /// Drives `lay_out_mathml` (compact scales mfrac children); `<math>` box-tree integration — deferred.
    pub math_style: MathStyle,
    /// MathML Core §2.1.2 — `math-depth`. Inherited. Computed value: integer. Initial: `0`.
    /// `auto-add` / `add(<integer>)` resolve against the inherited depth at compute time.
    pub math_depth: i32,
    /// CSS Shapes L1 §3 — `shape-outside`. NOT inherited. Initial: `None`.
    /// Phase 0: parse + store; float-wrap shape application — deferred.
    pub shape_outside: ShapeOutside,
    /// CSS Shapes L1 §4 — `shape-margin: <length-percentage>`. NOT inherited. Initial: 0.
    pub shape_margin: Length,
    /// CSS Shapes L1 §5 — `shape-image-threshold: <number>`. NOT inherited. Initial: 0.0.
    pub shape_image_threshold: f32,
    /// CSS Motion Path L1 §3 — `offset-path`. NOT inherited. Initial: `None` (no motion path).
    /// Phase 0: parse + store; path-based motion animation — deferred.
    pub offset_path: Option<String>,
    /// CSS Motion Path L1 §3 — `offset-distance: <length-percentage>`. NOT inherited. Initial: `0`.
    pub offset_distance: Length,
    /// CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`.
    pub offset_rotate: OffsetRotate,
    /// CSS Motion Path L1 §3 — `offset-anchor`: auto | `<position>`. NOT inherited. Initial: `Auto` (None).
    pub offset_anchor: Option<ObjectPosition>,
    /// SVG §11.2 — `fill`. Inherited. Initial: `Color(Color::BLACK)` per SVG spec.
    /// Overrides the SVG default presentation fill for shape elements.
    pub svg_fill: SvgPaint,
    /// SVG §11.3 — `fill-opacity`. Inherited. Range 0.0–1.0. Initial: 1.0.
    pub svg_fill_opacity: f32,
    /// SVG §11.2 — `stroke`. Inherited. Initial: `None` (no stroke per SVG spec).
    pub svg_stroke: SvgPaint,
    /// SVG §11.3 — `stroke-opacity`. Inherited. Range 0.0–1.0. Initial: 1.0.
    pub svg_stroke_opacity: f32,
    /// SVG §11.4 — `stroke-width`. Inherited. In resolved px. Initial: 1.0.
    pub svg_stroke_width: f32,
    /// SVG §11.3 — `fill-rule`. Inherited. Initial: `NonZero`.
    pub svg_fill_rule: FillRule,
    /// SVG §14.3.4 — `clip-rule`. Inherited. Initial: `NonZero`.
    /// Fill rule used for the interior of a `<clipPath>` child shape (reuses
    /// [`FillRule`]). Parsed and cascaded; consumed once SVG `clip-path:
    /// url(#id)` references land.
    pub svg_clip_rule: FillRule,
    /// SVG §11.4 — `stroke-linecap`. Inherited. Initial: `Butt`.
    pub svg_stroke_linecap: StrokeLinecap,
    /// SVG §11.4 — `stroke-linejoin`. Inherited. Initial: `Miter`.
    pub svg_stroke_linejoin: StrokeLinejoin,
    /// SVG §11.4 — `stroke-miterlimit`. Inherited. Range ≥ 1.0. Initial: 4.0.
    pub svg_stroke_miterlimit: f32,
    /// SVG §11.4 — `stroke-dasharray`. Inherited. Empty = solid line (none).
    /// Resolved dash/gap lengths in px, repeated cyclically.
    pub svg_stroke_dasharray: Vec<f32>,
    /// SVG §11.4 — `stroke-dashoffset`. Inherited. In resolved px. Initial: 0.0.
    pub svg_stroke_dashoffset: f32,
    /// CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — `paint-order`. Inherited.
    /// Order fill/stroke/markers are painted; initial `normal` = fill, stroke,
    /// markers (fill drawn first, markers on top).
    pub paint_order: SvgPaintOrder,
    /// SVG 2 §11.6 / CSS — `text-anchor`. Inherited. `None` = not set by CSS
    /// (the SVG presentation attribute, then the `start` initial, applies at box
    /// build time). `Some(_)` = an author CSS rule won the cascade and overrides
    /// any presentation attribute (presentation attributes have specificity 0).
    pub text_anchor: Option<crate::box_tree::SvgTextAnchor>,
    /// SVG 2 §11.10.2 / CSS — `dominant-baseline`. Inherited. `None` = not set by
    /// CSS (the presentation attribute, then the `auto` initial, applies at box
    /// build time). `Some(_)` = author CSS rule overrides the attribute.
    pub dominant_baseline: Option<crate::box_tree::SvgDominantBaseline>,
    /// SVG 1.1 §10.9.2 / CSS Inline L3 §5.2 — `baseline-shift`. NOT inherited.
    /// Initial `baseline` (no shift). Positive lengths/percentages raise the text.
    pub baseline_shift: crate::box_tree::SvgBaselineShift,
    // CSS Logical Properties L1 §2 — temporary storage for logical properties.
    // These are resolved to physical properties in resolve_logical_properties().
    /// CSS Logical Properties L1 — `inline-size`. `None` = auto.
    pub inline_size: Option<Length>,
    /// CSS Logical Properties L1 — `block-size`. `None` = auto.
    pub block_size: Option<Length>,
    /// CSS Logical Properties L1 — `inset-inline-start`.
    pub inset_inline_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `inset-inline-end`.
    pub inset_inline_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `inset-block-start`.
    pub inset_block_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `inset-block-end`.
    pub inset_block_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-inline-start`.
    pub margin_inline_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-inline-end`.
    pub margin_inline_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-block-start`.
    pub margin_block_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-block-end`.
    pub margin_block_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `padding-inline-start`.
    pub padding_inline_start: Length,
    /// CSS Logical Properties L1 — `padding-inline-end`.
    pub padding_inline_end: Length,
    /// CSS Logical Properties L1 — `padding-block-start`.
    pub padding_block_start: Length,
    /// CSS Logical Properties L1 — `padding-block-end`.
    pub padding_block_end: Length,
    /// CSS Logical Properties L1 — `border-inline-start-width`.
    pub border_inline_start_width: f32,
    /// CSS Logical Properties L1 — `border-inline-end-width`.
    pub border_inline_end_width: f32,
    /// CSS Logical Properties L1 — `border-block-start-width`.
    pub border_block_start_width: f32,
    /// CSS Logical Properties L1 — `border-block-end-width`.
    pub border_block_end_width: f32,
    /// CSS Anchor Positioning L1 §2 — `anchor-name`. Custom-ident with `--` prefix.
    /// Non-inherited. When set, this element is registered as an anchor for positioned elements.
    pub anchor_name: Option<Box<str>>,
    /// CSS Anchor Positioning L1 §3 — `position-anchor`. Custom-ident with `--` prefix.
    /// Non-inherited. Names the default anchor element for `inset-area` and `anchor()` resolution.
    pub position_anchor: Option<Box<str>>,
    /// CSS Anchor Positioning L1 §5 — `inset-area` row (vertical axis) keyword. Non-inherited.
    pub inset_area_row: crate::anchor::InsetAreaKeyword,
    /// CSS Anchor Positioning L1 §5 — `inset-area` column (horizontal axis) keyword. Non-inherited.
    pub inset_area_col: crate::anchor::InsetAreaKeyword,
    /// CSS Anchor Positioning L1 §2.1 — `anchor-scope`. Non-inherited. Initial: `None`.
    /// Restricts which named anchors from this element's subtree are visible outside.
    pub anchor_scope: crate::anchor::AnchorScope,
    /// CSS Anchor Positioning L1 §4 — `anchor-size()` in `width`. Non-inherited. Initial: `None`.
    /// When set, the element's used width is the referenced anchor's dimension.
    pub anchor_size_w: Option<crate::anchor::AnchorSizeFunc>,
    /// CSS Anchor Positioning L1 §4 — `anchor-size()` in `height`. Non-inherited. Initial: `None`.
    /// When set, the element's used height is the referenced anchor's dimension.
    pub anchor_size_h: Option<crate::anchor::AnchorSizeFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `top`. Non-inherited. Initial: `None`.
    pub anchor_top: Option<crate::anchor::AnchorFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `right`. Non-inherited. Initial: `None`.
    pub anchor_right: Option<crate::anchor::AnchorFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `bottom`. Non-inherited. Initial: `None`.
    pub anchor_bottom: Option<crate::anchor::AnchorFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `left`. Non-inherited. Initial: `None`.
    pub anchor_left: Option<crate::anchor::AnchorFunc>,

    /// CSS View Transitions L1 §10 — `view-transition-name`. Non-inherited. Initial: `None` (none).
    /// Custom-ident that marks this element as a named view-transition capture target.
    /// During a `document.startViewTransition()` call the shell matches old/new snapshots
    /// by name and cross-fades them. `None` means the property is `none`.
    pub view_transition_name: Option<Box<str>>,
    /// CSS Generated Content L3 §3.2 — `quotes`. Inherited. Initial `auto`.
    /// Supplies the glyph pairs for `content: open-quote` / `close-quote`.
    pub quotes: Quotes,
}

/// CSS Content L3 — value свойства `content`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Content {
    /// `normal` (default) — поведение по умолчанию для каждого element.
    #[default]
    Normal,
    /// `none` — pseudo-element не генерируется.
    None,
    /// Список фрагментов: строки, counter()/counters(), attr(), url().
    /// Phase 0 хранит список typed-фрагментов; конкатенация для render —
    /// задача paint pipeline.
    Items(Vec<ContentItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentItem {
    /// Литеральная строка из CSS-string-literal (без кавычек).
    String(String),
    /// `attr(name)` — значение HTML-атрибута текущего element.
    Attr(String),
    /// `url("path")` — изображение / external resource.
    Url(String),
    /// `counter(name [, style])` — значение counter-а. `style` — пока
    /// сырая строка (Phase 0 разрешит только `decimal` etc.).
    Counter {
        name: String,
        style: Option<String>,
    },
    /// `counters(name, separator [, style])` — вложенные counters
    /// (`1.2.3` через `.`).
    Counters {
        name: String,
        separator: String,
        style: Option<String>,
    },
    /// `open-quote` / `close-quote` — quotation marks per `quotes` property.
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
}

/// CSS Generated Content L3 §3.2 — `quotes`. Inherited. Initial: `auto`.
///
/// Controls the quotation marks produced by `content: open-quote` /
/// `close-quote`. The nesting depth (which pair is used) is tracked in
/// document order by the counters pre-pass; this value only supplies the
/// glyph pairs to choose from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Quotes {
    /// `auto` — UA language-appropriate quotation marks. Lumen uses English
    /// curly quotes: primary “ ”, secondary ‘ ’.
    #[default]
    Auto,
    /// `none` — `open-quote` / `close-quote` produce no marks (depth still
    /// advances).
    None,
    /// Explicit `[<string> <string>]+` pairs — outermost (depth 0) first.
    /// Each tuple is `(open, close)`.
    Pairs(Vec<(String, String)>),
}

impl Quotes {
    /// Returns the `(open, close)` glyph strings for the given nesting `depth`.
    ///
    /// `Auto` uses the built-in English pairs; `Pairs` clamps `depth` to the
    /// last available pair (CSS Content L3 §3.2). Returns `None` for `quotes:
    /// none` or an empty explicit list — the caller emits nothing in that case.
    pub fn pair_for_depth(&self, depth: usize) -> Option<(&str, &str)> {
        const AUTO: &[(&str, &str)] = &[("\u{201C}", "\u{201D}"), ("\u{2018}", "\u{2019}")];
        match self {
            Quotes::None => None,
            Quotes::Auto => {
                let idx = depth.min(AUTO.len() - 1);
                Some(AUTO[idx])
            }
            Quotes::Pairs(pairs) => {
                if pairs.is_empty() {
                    return None;
                }
                let idx = depth.min(pairs.len() - 1);
                let (o, c) = &pairs[idx];
                Some((o.as_str(), c.as_str()))
            }
        }
    }
}

/// CSS Scrollbars 1 — `scrollbar-width`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    /// `thin` — тонкий scrollbar.
    Thin,
    /// `none` — без visible scrollbar (контент всё ещё скроллится через
    /// keyboard / touch / programmatic).
    None,
}

impl ScrollbarWidth {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "thin" => Some(Self::Thin),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scrollbar-gutter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarGutter {
    /// `auto` (default) — gutter появляется когда overflow:scroll.
    #[default]
    Auto,
    /// `stable` — gutter всегда зарезервирован (не двигает контент при scroll).
    Stable,
    /// `stable both-edges` — gutter на обоих краях для симметрии.
    StableBothEdges,
}

impl ScrollbarGutter {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        if lc == "auto" {
            return Some(Self::Auto);
        }
        if lc == "stable" {
            return Some(Self::Stable);
        }
        // `stable both-edges` — двухтокеновая форма.
        let tokens: Vec<&str> = lc.split_whitespace().collect();
        if tokens == ["stable", "both-edges"] {
            return Some(Self::StableBothEdges);
        }
        None
    }
}

/// CSS Lists L3 §2.1 — markers для list items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ListStyleType {
    /// `none` — без marker.
    None,
    /// `disc` — закрашенный кружок (default для ul).
    #[default]
    Disc,
    /// `circle` — пустой кружок.
    Circle,
    /// `square` — квадратик.
    Square,
    /// `decimal` — 1, 2, 3, ... (default для ol).
    Decimal,
    /// `decimal-leading-zero` — 01, 02, ..., 09, 10, ...
    DecimalLeadingZero,
    /// `lower-roman` — i, ii, iii, ...
    LowerRoman,
    /// `upper-roman` — I, II, III, ...
    UpperRoman,
    /// `lower-alpha` / `lower-latin` — a, b, c, ...
    LowerAlpha,
    /// `upper-alpha` / `upper-latin` — A, B, C, ...
    UpperAlpha,
    /// `lower-greek` — α, β, γ, ...
    LowerGreek,
    /// `<custom-ident>` — ссылка на именованный `@counter-style`.
    Custom(Box<str>),
}

impl ListStyleType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "disc" => Some(Self::Disc),
            "circle" => Some(Self::Circle),
            "square" => Some(Self::Square),
            "decimal" => Some(Self::Decimal),
            "decimal-leading-zero" => Some(Self::DecimalLeadingZero),
            "lower-roman" => Some(Self::LowerRoman),
            "upper-roman" => Some(Self::UpperRoman),
            "lower-alpha" | "lower-latin" => Some(Self::LowerAlpha),
            "upper-alpha" | "upper-latin" => Some(Self::UpperAlpha),
            "lower-greek" => Some(Self::LowerGreek),
            // Any unrecognised ident is a reference to a named @counter-style.
            s if !s.is_empty() => Some(Self::Custom(s.into())),
            _ => None,
        }
    }
}

/// CSS Lists L3 §2.3 — `list-style-position`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListStylePosition {
    /// `outside` (default) — marker вне content-area.
    #[default]
    Outside,
    /// `inside` — marker внутри content-area.
    Inside,
}

impl ListStylePosition {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "outside" => Some(Self::Outside),
            "inside" => Some(Self::Inside),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.2 — `overflow-wrap`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverflowWrap {
    #[default]
    Normal,
    /// `break-word` — разрешает перенос любого слова, чтобы не было overflow.
    BreakWord,
    /// `anywhere` — как `break-word`, но также влияет на intrinsic-width
    /// computation (CSS Text L3).
    Anywhere,
}

impl OverflowWrap {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "break-word" => Some(Self::BreakWord),
            "anywhere" => Some(Self::Anywhere),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`.
/// Управляет строгостью правил переноса CJK-текста по пробелам.
/// Phase 0: parse + store; реальный CJK-wrap — отдельная задача.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineBreak {
    #[default]
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

/// CSS Text L3 §5.1 — `word-break`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WordBreak {
    #[default]
    Normal,
    /// `keep-all` — CJK не разбивается.
    KeepAll,
    /// `break-all` — разрыв в любом месте, кроме whitespace.
    BreakAll,
    /// `break-word` — legacy для `overflow-wrap: break-word`.
    BreakWord,
}

impl WordBreak {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "keep-all" => Some(Self::KeepAll),
            "break-all" => Some(Self::BreakAll),
            "break-word" => Some(Self::BreakWord),
            _ => None,
        }
    }
}

/// CSS Text L3 §6 — `hyphens`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hyphens {
    /// `none` — переносы запрещены.
    None,
    /// `manual` (default) — переносы только при явных hyphenation-точках
    /// (`&shy;` / U+00AD).
    #[default]
    Manual,
    /// `auto` — UA расставляет переносы по алгоритму (требует hyphenation
    /// dictionary).
    Auto,
}

impl Hyphens {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`.
/// Указывает, какими жестами UA управляет самостоятельно (pan/zoom).
/// Phase 0: parse + store; реальная обработка touch-жестов — P3 task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TouchAction {
    #[default]
    Auto,
    None,
    PanX,
    PanLeft,
    PanRight,
    PanY,
    PanUp,
    PanDown,
    PinchZoom,
    Manipulation,
}

/// CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`.
/// Контролирует отображение элемента согласно UA-теме (форм-виджеты).
/// Phase 0: parse + store; реальная стилизация форм-виджетов — P2/P3 task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Auto,
    None,
    /// `menulist-button` / `searchfield` / `textfield` / `button` и прочие
    /// platform-специфичные значения — хранятся как Compat.
    Compat,
    /// `base-select` (HTML/CSS «Customizable Select») — `<select>` рендерится
    /// как author-стилизуемое дерево (кнопка-триггер + `<selectedcontent>` +
    /// `::picker(select)` со списком опций) вместо непрозрачного нативного
    /// контрола. См. `box_tree.rs` (построение дерева) и `forms.rs` (поповер).
    BaseSelect,
}

/// CSS Basic UI L4 §4.4 — `field-sizing`. NOT inherited. Initial: `Fixed`.
/// `Fixed` — UA-specified dimensions apply (default browser behaviour).
/// `Content` — intrinsic size comes from the control's text content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FieldSizing {
    /// UA default dimensions (e.g. `<input>` is 174×21 px).
    #[default]
    Fixed,
    /// Size the control to fit its text content (CSS Basic UI L4 §4.4).
    Content,
}

/// CSS Pointer Events L1. Default `auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PointerEvents {
    #[default]
    Auto,
    None,
    Visible,
    /// `painted` / `fill` / `stroke` / `all` — для SVG. В non-SVG
    /// контексте трактуются как `auto`.
    Painted,
    Fill,
    Stroke,
    All,
}

impl PointerEvents {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            "visible" | "visiblepainted" | "visiblefill" | "visiblestroke" => {
                Some(Self::Visible)
            }
            "painted" => Some(Self::Painted),
            "fill" => Some(Self::Fill),
            "stroke" => Some(Self::Stroke),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`.
/// Позволяет пользователю изменять размер элемента мышью.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Resize {
    /// `none` — resize запрещён.
    #[default]
    None,
    /// `both` — resize по обеим физическим осям.
    Both,
    /// `horizontal` — resize только по физической ширине.
    Horizontal,
    /// `vertical` — resize только по физической высоте.
    Vertical,
    /// `block` — resize вдоль block-оси (логическая, зависит от `writing-mode`).
    Block,
    /// `inline` — resize вдоль inline-оси (логическая, зависит от `writing-mode`).
    Inline,
}

impl Resize {
    /// Разрешает логическую ось `resize` (`Block`/`Inline`) в физическую пару
    /// `(разрешена ширина, разрешена высота)` с учётом `writing-mode`.
    ///
    /// В `horizontal-tb` block-ось — вертикальная, inline-ось — горизонтальная;
    /// в вертикальных режимах (`vertical-rl`/`vertical-lr`/`sideways-rl`) — наоборот.
    /// Используется драг-хендлером grip-а (`crates/shell/src/main.rs`), чтобы
    /// вложенный корректно гейтить, какую из осей (`width`/`height`) двигать.
    pub fn allowed_axes(self, writing_mode: WritingMode) -> (bool, bool) {
        let vertical_wm = matches!(
            writing_mode,
            WritingMode::VerticalRl
                | WritingMode::VerticalLr
                | WritingMode::SidewaysRl
                | WritingMode::SidewaysLr
        );
        match self {
            Resize::None => (false, false),
            Resize::Both => (true, true),
            Resize::Horizontal => (true, false),
            Resize::Vertical => (false, true),
            Resize::Block => (vertical_wm, !vertical_wm),
            Resize::Inline => (!vertical_wm, vertical_wm),
        }
    }
}

/// CSS Containment L3 §3 — `contain` property.
/// Bitflags: bit0=size, bit1=inline-size, bit2=layout, bit3=style, bit4=paint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContainFlags(pub u8);

impl ContainFlags {
    pub const NONE: Self = Self(0);
    pub const SIZE: Self = Self(1 << 0);
    pub const INLINE_SIZE: Self = Self(1 << 1);
    pub const LAYOUT: Self = Self(1 << 2);
    pub const STYLE: Self = Self(1 << 3);
    pub const PAINT: Self = Self(1 << 4);
    /// `strict` = size + layout + style + paint
    pub const STRICT: Self = Self(1 | (1 << 2) | (1 << 3) | (1 << 4));
    /// `content` = layout + style + paint
    pub const CONTENT: Self = Self((1 << 2) | (1 << 3) | (1 << 4));
}

/// CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentVisibility {
    #[default]
    Visible,
    Auto,
    Hidden,
}

/// CSS Sizing L4 §4.5 — `interpolate-size` property value.
///
/// Controls whether keyword sizes (`auto`, `min-content`, `max-content`,
/// `fit-content`) can participate in CSS transitions and animations.
/// When `AllowKeywords` is active, the layout engine resolves keyword sizes
/// to their px equivalent at transition start, enabling smooth
/// `height: 0 → height: auto` transitions.
///
/// # CSS: interpolate-size
/// P4 wires this enum via `apply_declaration("interpolate-size", ...)` and
/// stores the result in `ComputedStyle::interpolate_size`. The engine reads
/// it in `TransitionScheduler::sync()` to decide whether to allow keyword
/// size interpolation for a given element.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InterpolateSizeMode {
    /// CSS Sizing L4 §4.5.1 initial value — keyword sizes are discrete.
    /// Transitions that start or end at a keyword size snap at `t = 0.5`.
    #[default]
    NumericOnly,
    /// CSS Sizing L4 §4.5 `allow-keywords` value — keyword sizes resolve
    /// to their px value at transition start, enabling smooth animations.
    AllowKeywords,
}

/// CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContainerType {
    #[default]
    Normal,
    Size,
    InlineSize,
}

/// Resolved container dimensions, passed during style re-computation for container queries.
/// CSS Container Queries L1 §3: size features (width, height) evaluated against this context.
#[derive(Debug, Clone)]
pub struct ContainerContext {
    /// Content width of the container element in pixels.
    pub width: f32,
    /// Content height if definite (None when auto/unknown).
    pub height: Option<f32>,
    /// The container's `container-name` values (for named queries).
    pub names: Vec<String>,
    /// Custom properties (`--*`) контейнера — для style() queries (CSS Containment L3 §4).
    /// Shares the container's own [`CustomProps`] allocation, so building a
    /// `ContainerContext` costs no map copy.
    pub custom_props: CustomProps,
    /// Container's own computed style, serialized the same way as
    /// `window.getComputedStyle()` (`selector_query::computed_style_to_map`) —
    /// used to resolve `style()` queries against standard (non-custom) properties.
    pub style_props: HashMap<String, String>,
    /// Container's own font-size — the `em` basis when resolving relative
    /// units in a `style()` query's declared value.
    pub font_size: f32,
    /// Viewport size — the `vw`/`vh`/`vmin`/`vmax` basis when resolving
    /// relative units in a `style()` query's declared value.
    pub viewport: Size,
    /// Height of the *container's own* containing block (its immediate
    /// parent's content box) — the CSS2.1 §10.5 basis for resolving `%` in
    /// the container's own `height`/`top`/`bottom`/`min-height`/
    /// `max-height` during a `style()` query. Distinct from `height` above,
    /// which is the container's own (already-resolved) content height
    /// exposed to descendants for `(min-height: …)`-style size queries.
    /// Always a concrete pixel value: by the time `ContainerContext` is
    /// built, the whole tree has already been laid out to definite rects,
    /// so this doesn't distinguish an explicitly-sized parent from one whose
    /// own height was itself content-derived (CSS2.1 §10.5's "if the height
    /// of the containing block is not specified explicitly… the percentage
    /// value computes to auto" is not modeled here).
    pub own_containing_block_height: f32,
}

/// Evaluates a raw @container condition string against a `ContainerContext`.
///
/// Phase 0: handles `(min-width: Npx)`, `(max-width: Npx)`, `(min-height: Npx)`,
/// `(max-height: Npx)`, `(width: Npx)`, `(height: Npx)`, and `and`/`or`/`not` operators.
/// Also supports `style(--prop: value)` and boolean `style(--prop)` forms
/// (CSS Containment L3 §4). Custom-property style queries compare the container's
/// value against the query value as *normalized* token streams — internal runs of
/// whitespace collapse to a single space and whitespace around commas is removed,
/// so `style(--gap: 1px 2px)` matches a container declaring `--gap: 1px  2px` or
/// `--gap:1px 2px` (CSS Custom Properties L1 §2 «computed value is the specified
/// value with whitespace trimmed»). The container's declared value is `var()`-expanded
/// against its own `custom_props` map before comparison — e.g. a container with
/// `--base: 8px; --gap: var(--base);` matches `style(--gap: 8px)` — mirroring how
/// `var()` is substituted when a custom property is consumed elsewhere in the cascade.
/// Standard (non-custom) properties are compared against the container's own
/// computed style (`ctx.style_props`, same serialization as `getComputedStyle()`):
/// `style(display: flex)` matches a container computed to `display: flex`. The
/// comparison is case-insensitive after the same whitespace/comma normalization
/// used for custom properties, so it works for keyword and length values whose
/// author-written form matches the serialized form (`style(width: 100px)` against
/// a computed `100px`); if that normalized comparison fails, both sides are also
/// tried as CSS colors and as lengths (`style_query_value_matches`), so
/// `style(color: red)` matches a computed `rgb(255, 0, 0)`, `style(border-width:
/// 2pt)` matches a computed `2.6667px`, and relative lengths (`em`, `%`,
/// viewport units) resolve against the container's own `font_size`/`viewport`
/// (`style(width: 1em)` matches a computed `16px` on a container whose
/// font-size is `16px`) — the same `em`/viewport basis `cq*` units use,
/// since a `style()` query's declared value is evaluated as if specified on
/// the container element itself (CSS Containment L3 §4). The `%` basis is
/// picked per queried property by `style_query_percent_basis` — the
/// container's width by default, but its own font-size for `line-height` and
/// its own containing block's height for `height`/`top`/`bottom`/
/// `min-height`/`max-height`.
/// Boolean form (`style(--prop)` / `style(prop)` without a value) is true when the
/// container has any value for that property — for custom properties this checks
/// `custom_props`, for standard properties `style_props` (a standard property never
/// computes to the custom-property-only guaranteed-invalid value, so in practice
/// this is true whenever the container's computed style was resolved for it).
/// A single `style()` call may itself combine multiple property queries with
/// `and`/`or`/`not`, each wrapped in its own parentheses — e.g.
/// `style((--a: 1) and (--b: 2))` or `style(not (display: none))` — per the
/// formal grammar (`<style-query> = <style-condition> | <style-feature>`,
/// CSS Containment L3 §5.2); see `evaluate_style_query`.
/// Phase 0 limitations:
/// - `state()` container queries: not a Lumen gap — the CSS Containment L3
///   spec itself removed/deferred state query features, so there is nothing
///   to implement against.
/// - Vertical box-model properties (`margin-top`/`margin-bottom`/
///   `padding-top`/`padding-bottom`) resolve `%` against the container's
///   width per CSS2.1 §8.3/§10.3 (correct — the containing block width is
///   the basis for *all four* margin/padding sides).
/// - `height`/`top`/`bottom`/`min-height`/`max-height` resolve `%` against
///   `ContainerContext::own_containing_block_height` — the container's own
///   immediate parent's content height, correctly distinct from the
///   container's own size or width (see that field's doc). The one
///   remaining approximation: this value is always treated as definite,
///   since Lumen's post-layout box tree no longer distinguishes a parent
///   whose height was explicitly specified from one whose height was itself
///   content-derived (CSS2.1 §10.5 would compute the `%` as `auto` in the
///   latter case).
///
/// Unknown features → false (safe fallback).
pub fn evaluate_container_condition(condition: &str, ctx: &ContainerContext) -> bool {
    let s = condition.trim();
    // Handle `not (...)` and `not style(...)`.
    if let Some(rest) = s.strip_prefix("not") {
        let rest = rest.trim();
        if rest.starts_with('(') || rest.to_ascii_lowercase().starts_with("style(") {
            return !evaluate_container_condition(rest, ctx);
        }
    }
    // Split on top-level `and` / `or`.
    if let Some((lhs, rhs)) = split_top_level_logical(s, " and ") {
        return evaluate_container_condition(lhs, ctx) && evaluate_container_condition(rhs, ctx);
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " or ") {
        return evaluate_container_condition(lhs, ctx) || evaluate_container_condition(rhs, ctx);
    }
    // Handle `style(...)` queries.
    let s_lower = s.to_ascii_lowercase();
    if s_lower.starts_with("style(") && s.ends_with(')') {
        // Extract content between `style(` and the final `)`.
        let inner = s[6..s.len() - 1].trim();
        return evaluate_style_query(inner, ctx);
    }
    // Feature: `(feature: value)`.
    let inner = s.strip_prefix('(').and_then(|x| x.strip_suffix(')'));
    let inner = match inner {
        Some(i) => i.trim(),
        None => return false,
    };
    // Parse `feature: value`.
    let colon = inner.find(':');
    let (feature, value) = if let Some(pos) = colon {
        (inner[..pos].trim(), inner[pos + 1..].trim())
    } else {
        // Boolean feature (e.g. `(color)`) — unsupported in Phase 0.
        return false;
    };
    let px = parse_css_length_to_px(value);
    match (feature, px) {
        ("min-width", Some(v))  => ctx.width >= v,
        ("max-width", Some(v))  => ctx.width <= v,
        ("width", Some(v))      => (ctx.width - v).abs() < 0.5,
        ("min-height", Some(v)) => ctx.height.is_some_and(|h| h >= v),
        ("max-height", Some(v)) => ctx.height.is_none_or(|h| h <= v),
        ("height", Some(v))     => ctx.height.is_some_and(|h| (h - v).abs() < 0.5),
        _ => false,
    }
}

/// Evaluates the content of a `style()` container query — CSS Containment L3
/// §5.2. Per the formal grammar (`<style-query> = <style-condition> |
/// <style-feature>`, `<style-condition> = not <style-in-parens> |
/// <style-in-parens> [and <style-in-parens>]* | [or <style-in-parens>]*`,
/// `<style-in-parens> = (<style-condition>) | (<style-feature>)`), a single
/// `style()` call may combine multiple property queries with `and`/`or`/`not`,
/// each wrapped in its own parentheses (e.g. `style((--a: 1) and (--b: 2))`,
/// `style(not (display: none))`). `<style-feature>` itself always queries
/// exactly one property — the grammar has no comma-separated multi-declaration
/// form — handled by `evaluate_style_feature`.
fn evaluate_style_query(s: &str, ctx: &ContainerContext) -> bool {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("not") {
        let rest = rest.trim();
        if rest.starts_with('(') {
            return !evaluate_style_query(rest, ctx);
        }
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " and ") {
        return evaluate_style_query(lhs, ctx) && evaluate_style_query(rhs, ctx);
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " or ") {
        return evaluate_style_query(lhs, ctx) || evaluate_style_query(rhs, ctx);
    }
    // `<style-in-parens>` grouping: strip one layer and recurse.
    if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return evaluate_style_query(inner, ctx);
    }
    // Leaf: a bare `<style-feature>` (boolean `prop` or declaration `prop: value`).
    evaluate_style_feature(s, ctx)
}

/// Evaluates a single `<style-feature>` (CSS Containment L3 §5.2): the boolean
/// form `prop` (true iff the container has any value for it) or the
/// declaration form `prop: value` (true iff the container's own value matches,
/// with the same custom-property/var()-expansion and standard-property
/// canonicalization as `evaluate_container_condition`'s `style()` handling).
fn evaluate_style_feature(feature: &str, ctx: &ContainerContext) -> bool {
    let inner = feature.trim();
    // Boolean form: `--prop` or `prop`.
    if !inner.contains(':') {
        let name = inner.trim();
        if name.starts_with("--") {
            return resolve_container_custom_prop(ctx, name).is_some_and(|v| !v.trim().is_empty());
        }
        // Standard property: true if the container's computed style has
        // any value for it (a standard property never computes to the
        // custom-property-only guaranteed-invalid value).
        return ctx
            .style_props
            .get(&name.to_ascii_lowercase())
            .is_some_and(|v| !v.trim().is_empty());
    }
    // Declaration form: `--prop: value` or `prop: value`.
    if let Some((name, value)) = inner.split_once(':') {
        let name = name.trim();
        let want = normalize_style_value(value);
        if name.starts_with("--") {
            return resolve_container_custom_prop(ctx, name).map(|v| normalize_style_value(&v))
                == Some(want);
        }
        // Standard property: compare against the container's own computed
        // style (case-insensitive — CSS keywords are ASCII case-insensitive).
        let name_lower = name.to_ascii_lowercase();
        return ctx
            .style_props
            .get(&name_lower)
            .is_some_and(|v| style_query_value_matches(v, &want, &name_lower, ctx));
    }
    false
}

/// Resolves a container's custom property for a `style()` query: looks up `name`
/// in `ctx.custom_props` and expands any `var()` references against that same map
/// (CSS Variables L1 §3), so a chain like `--base: 8px; --gap: var(--base);`
/// resolves `--gap` to `8px` before comparison. Returns `None` if the property is
/// absent or its `var()` chain fails to resolve (unknown reference, no fallback,
/// or recursion past `VAR_EXPAND_MAX_DEPTH`).
fn resolve_container_custom_prop(ctx: &ContainerContext, name: &str) -> Option<String> {
    let raw = ctx.custom_props.get(name)?;
    expand_vars(raw, &ctx.custom_props, 0)
}

/// Normalizes a custom-property value for `style()` query comparison.
///
/// Collapses each run of ASCII whitespace to a single space, trims the ends, and
/// removes whitespace immediately around commas. This mirrors how a custom
/// property's computed value drops insignificant whitespace between tokens
/// (CSS Custom Properties L1 §2), so equivalent declarations compare equal
/// regardless of the author's spacing (`1px 2px` == `1px  2px`, `a,b` == `a, b`).
fn normalize_style_value(s: &str) -> String {
    // First collapse internal whitespace runs to single spaces.
    let collapsed: String = {
        let mut out = String::with_capacity(s.len());
        let mut prev_ws = false;
        for ch in s.trim().chars() {
            if ch.is_ascii_whitespace() {
                if !prev_ws {
                    out.push(' ');
                }
                prev_ws = true;
            } else {
                out.push(ch);
                prev_ws = false;
            }
        }
        out
    };
    // Then strip the spaces that sit directly around commas.
    let mut out = String::with_capacity(collapsed.len());
    let bytes = collapsed.as_bytes();
    for (i, ch) in collapsed.char_indices() {
        if ch == ' ' {
            let next_is_comma = bytes.get(i + 1) == Some(&b',');
            let prev_is_comma = i > 0 && bytes[i - 1] == b',';
            if next_is_comma || prev_is_comma {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Compares a container's serialized computed-style value against a `style()`
/// query's declared value for a standard (non-custom) property.
///
/// First tries the normalized token comparison (`normalize_style_value`,
/// case-insensitive). If that fails, falls back to two context-free
/// canonicalizations, tried in order:
/// 1. CSS colors: both sides parsed and compared by resolved RGBA channels —
///    so `style(color: red)` matches a container computed to
///    `color: rgb(255, 0, 0)` (CSS Color L4 §4, equivalent notations denote
///    the same color).
/// 2. CSS lengths: both sides parsed via `parse_length`, then resolved to px
///    using `ctx` as the basis — `ctx.font_size` for `em`, `ctx.viewport` for
///    `vw`/`vh`/`vmin`/`vmax` (CSS Values L3 §5.2/§6.1; absolute units like
///    `pt` resolve independent of any basis) — so `style(border-width: 2pt)`
///    matches a computed `2.6667px`, and `style(width: 1em)` matches a
///    computed `16px` on a container whose font-size is `16px`. The `%`
///    basis is picked per `prop_name` by `style_query_percent_basis` — e.g.
///    `line-height`'s is the container's own font-size, not its width.
///    Values that need layout context beyond `ctx` (`min-content`, unresolved
///    `cq*` outside a re-layout pass) don't resolve and fall through to the
///    textual comparison's `false`.
///
/// `want` must already be normalized by the caller. `prop_name` must already
/// be lowercased by the caller.
fn style_query_value_matches(computed: &str, want: &str, prop_name: &str, ctx: &ContainerContext) -> bool {
    if normalize_style_value(computed).eq_ignore_ascii_case(want) {
        return true;
    }
    if let (Some(a), Some(b)) = (parse_color(computed), parse_color(want)) {
        return a == b;
    }
    if let (Some(a), Some(b)) = (parse_length(computed), parse_length(want)) {
        let basis = Some(style_query_percent_basis(prop_name, ctx));
        if let (Some(pa), Some(pb)) = (
            a.resolve(ctx.font_size, basis, ctx.viewport),
            b.resolve(ctx.font_size, basis, ctx.viewport),
        ) {
            return (pa - pb).abs() < 0.01;
        }
    }
    false
}

/// Picks the `%` reference basis (in px) for a `style()` query's declared
/// value, based on which standard property is being queried — CSS Values L3
/// §5.2's «the percentage is calculated with respect to X» is per-property,
/// not a single container size. Mirrors the handful of properties whose
/// basis differs from the common "containing block width" default:
/// - `line-height`: the element's own font-size (CSS Inline L3 §4.6.2),
///   which for a `style()` query is the container's own `font_size`.
/// - Vertical box-model properties (`height`, `top`/`bottom`, vertical
///   `min-`/`max-height`): the *container's own* containing block's height
///   (CSS2.1 §10.5) — `ctx.own_containing_block_height`, i.e. the height of
///   the container's parent content box, not the container's own height
///   (`ctx.height` is a different quantity: the container's own resolved
///   size, exposed to descendants for `(min-height: …)`-style size queries).
///
/// Every other property (including `margin-top`/`margin-bottom`/
/// `padding-top`/`padding-bottom`, which CSS2.1 §8.3/§10.3 defines against
/// the containing block *width* despite being vertical) falls back to the
/// container's width, unchanged from before this function existed.
fn style_query_percent_basis(prop_name: &str, ctx: &ContainerContext) -> f32 {
    match prop_name {
        "line-height" => ctx.font_size,
        "height" | "min-height" | "max-height" | "top" | "bottom" => {
            ctx.own_containing_block_height
        }
        _ => ctx.width,
    }
}

/// Parses a CSS length value to pixels (px / em not supported — just px for Phase 0).
fn parse_css_length_to_px(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix("px") {
        n.trim().parse::<f32>().ok()
    } else if let Some(n) = s.strip_suffix("em") {
        // Phase 0: treat em as px (approximate).
        n.trim().parse::<f32>().ok()
    } else {
        s.parse::<f32>().ok()
    }
}

/// Splits `s` on the first occurrence of `sep` that is not inside parentheses.
fn split_top_level_logical<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let sep_bytes = sep.as_bytes();
    let s_bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i + sep.len() <= s.len() {
        match s_bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && s_bytes[i..].starts_with(sep_bytes) {
            return Some((&s[..i], &s[i + sep.len()..]));
        }
        i += 1;
    }
    None
}

/// Applies matching `@container` rules from `sheet` to `style`.
/// Called during the second layout pass for descendants of container elements.
/// `ctx` — resolved size of the nearest container ancestor.
pub fn apply_container_rules(
    style: &mut ComputedStyle,
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
    dark_mode: bool,
) {
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for container_rule in &sheet.container_rules {
        // Name filter: if the rule has a name, the context must include that name.
        if container_rule.name.as_ref().is_some_and(|rule_name| {
            !ctx.names.iter().any(|n| n == rule_name)
        }) {
            continue;
        }
        if !evaluate_container_condition(&container_rule.condition, ctx) {
            continue;
        }
        // Apply declarations from matching rules.
        for rule in &container_rule.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if best.is_some() {
                let em = style.font_size;
                let pw = style.font_weight;
                let inherited = style.clone();
                for decl in &rule.declarations {
                    let attr_buf;
                    let effective_decl: &Declaration = if decl.value.contains("attr(") {
                        let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
                        attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
                        &attr_buf
                    } else {
                        decl
                    };
                    apply_declaration(style, effective_decl, em, viewport, pw, &inherited, &inherited, is_quirks, dark_mode);
                }
            }
        }
    }
}

/// CSS Shapes L1 §3 — `shape-outside` value. NOT inherited. Initial: `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ShapeOutside {
    #[default]
    None,
    /// `<basic-shape>` or `<url>` or `<box-value>` — stored as raw string for Phase 0.
    Value(String),
}

/// CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum OffsetRotate {
    #[default]
    Auto,
    /// `auto <angle>` — auto direction plus a fixed rotation offset.
    AutoAngle(f32),
    Reverse,
    Angle(f32),
}

/// CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrintColorAdjust {
    #[default]
    Economy,
    Exact,
}

/// CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum FontSizeAdjust {
    #[default]
    None,
    Auto,
    Value(f32),
}

/// CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WritingMode {
    /// `horizontal-tb` — left-to-right horizontal, top-to-bottom block.
    #[default]
    HorizontalTb,
    /// `vertical-rl` — top-to-bottom vertical, right-to-left block.
    VerticalRl,
    /// `vertical-lr` — top-to-bottom vertical, left-to-right block.
    VerticalLr,
    /// `sideways-rl` — same as vertical-rl but glyphs rotated 90° CW.
    SidewaysRl,
    /// `sideways-lr` — same as vertical-lr but glyphs rotated 90° CCW.
    SidewaysLr,
}

/// CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`.
/// Only meaningful in vertical writing modes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOrientation {
    /// `mixed` — rotate CJK upright, rotate others 90° CW.
    #[default]
    Mixed,
    /// `upright` — all glyphs upright; implies `direction: ltr`.
    Upright,
    /// `sideways` — all glyphs rotated 90° CW (like vertical-rl inline).
    Sideways,
}

/// CSS UI L4 §6.2 — `user-select`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UserSelect {
    #[default]
    Auto,
    Text,
    None,
    Contain,
    All,
}

impl UserSelect {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "text" => Some(Self::Text),
            "none" => Some(Self::None),
            "contain" => Some(Self::Contain),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scroll-behavior`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollBehavior {
    #[default]
    Auto,
    Smooth,
}

/// CSS Scroll Snap L1 §3.1 — `scroll-snap-type: none | <axis> [mandatory | proximity]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapType {
    pub axis: ScrollSnapAxis,
    pub strictness: ScrollSnapStrictness,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAxis {
    #[default]
    None,
    X,
    Y,
    Block,
    Inline,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStrictness {
    #[default]
    Proximity,
    Mandatory,
}

/// CSS Scroll Snap L1 §6.1 — `scroll-snap-align: none | <axis-keyword>{1,2}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapAlign {
    pub block: ScrollSnapAlignKeyword,
    pub inline: ScrollSnapAlignKeyword,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAlignKeyword {
    #[default]
    None,
    Start,
    End,
    Center,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStop {
    #[default]
    Normal,
    Always,
}

/// CSS Overscroll Behavior L1 §2 — `overscroll-behavior: auto | contain | none`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverscrollBehavior {
    #[default]
    Auto,
    Contain,
    None,
}

impl ScrollBehavior {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            _ => None,
        }
    }
}

/// CSS Images L3/L4 §3.3/§3.7 — parsed linear / radial / conic gradient.
///
/// Stored instead of the raw CSS string once `parse_background_gradient`
/// has tokenised the gradient function. `Unknown` is kept as fallback for
/// future / malformed variants so they round-trip without information loss.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedGradient {
    /// `linear-gradient(angle, stop, ...)` — angle in CSS degrees measured
    /// clockwise from "to top" (0° = top, 90° = right, 180° = bottom).
    Linear {
        /// Gradient line angle in CSS degrees (0° = to top, 90° = to right).
        /// For a `to <corner>` keyword this is only the square-box (45/135/
        /// 225/315°) placeholder — `corner` carries the keyword so a paint-time
        /// consumer that knows the actual box size can resolve the true,
        /// aspect-ratio-dependent angle via [`GradientCorner::angle_deg`].
        angle_deg: f32,
        /// `Some` when the direction was written as `to <corner>` (CSS Images
        /// L3 §3.1) rather than an explicit `<angle>` — the true gradient-line
        /// angle for a corner keyword depends on the gradient box's aspect
        /// ratio, which is not known at style-parse time.
        corner: Option<GradientCorner>,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-linear-gradient`.
        repeating: bool,
    },
    /// `radial-gradient(...)` — radial gradient centred at `(cx, cy)`.
    Radial {
        /// Centre as fraction of box width/height ([0, 1] = [left/top, right/bottom]).
        center_x_pct: f32,
        center_y_pct: f32,
        /// Ending shape — `circle` or `ellipse` (CSS Images L3 §3.5). The radii
        /// are resolved against the box at paint time via [`radial_gradient_radii`].
        shape: RadialShape,
        /// Sizing keyword for the ending shape (default `farthest-corner`).
        size: RadialSize,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-radial-gradient`.
        repeating: bool,
    },
    /// CSS Images L4 §3.7 — `conic-gradient([from <angle>]? [at <pos>]?, <stops>)`.
    /// Angular gradient revolving around `(center_x_pct, center_y_pct)` (fraction of
    /// box width/height). `from_angle_deg` is the starting angle in CSS degrees
    /// (0° = top, 90° = right), clockwise. Stops' positions are stored as
    /// `Length::Percent` where 100% corresponds to a full revolution
    /// (angle units `<angle>` are pre-converted to percent on parse).
    Conic {
        center_x_pct: f32,
        center_y_pct: f32,
        /// Starting angle in CSS degrees (0° = top, 90° = right, clockwise).
        from_angle_deg: f32,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-conic-gradient`.
        repeating: bool,
    },
    /// Fallback for any future gradient variant not yet rendered.
    Unknown(String),
}

/// CSS Images L3 §3.1 — `to <corner>` keyword of a `linear-gradient`'s
/// direction. Unlike the four side keywords (`to top`/`to right`/…), a corner
/// keyword's true gradient-line angle depends on the gradient box's aspect
/// ratio: the line is defined to pass exactly through the two opposite
/// corners, so on a non-square box it tilts away from the naive 45°
/// diagonal toward whichever side the box is longer along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientCorner {
    /// `to top right` / `to right top`.
    TopRight,
    /// `to bottom right` / `to right bottom`.
    BottomRight,
    /// `to bottom left` / `to left bottom`.
    BottomLeft,
    /// `to top left` / `to left top`.
    TopLeft,
}

impl GradientCorner {
    /// Resolves the keyword to a true gradient-line angle (CSS degrees,
    /// 0° = to top, clockwise) for a box of the given size.
    ///
    /// Per CSS Images L3 §3.1 the gradient line is defined to be
    /// *perpendicular* to the diagonal connecting the two corners the
    /// keyword does *not* name — e.g. for `to bottom right` that diagonal
    /// runs between the top-right and bottom-left corners, direction
    /// `(-width, height)`, so the gradient line itself runs along
    /// `(height, width)`. That makes the base angle `atan2(height, width)`
    /// (note: height first), not `atan2(width, height)` — on a box much
    /// wider than it is tall this angle is *small* (the line tilts toward
    /// vertical, "to bottom"/"to top"), which is the opposite of the naive
    /// "tilts toward the long axis" guess. Verified against a real Edge
    /// render of a 960×160 box: predicted 170.5°, measured ~170.5°.
    /// Reduces to the familiar 45/135/225/315° only when `width == height`.
    pub fn angle_deg(self, width: f32, height: f32) -> f32 {
        let base = height.max(0.0).atan2(width.max(0.0)).to_degrees();
        match self {
            GradientCorner::TopRight => base,
            GradientCorner::BottomRight => 180.0 - base,
            GradientCorner::BottomLeft => 180.0 + base,
            GradientCorner::TopLeft => 360.0 - base,
        }
    }
}

/// CSS Images L3 §3.5 — ending-shape of a `radial-gradient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialShape {
    /// `circle` — isotropic, a single radius along every direction.
    Circle,
    /// `ellipse` (also the default when no shape keyword is given) — independent
    /// horizontal and vertical radii.
    Ellipse,
}

/// CSS Images L3 §3.5 — sizing keyword controlling the radii of a
/// `radial-gradient`'s ending shape. Explicit `<length>` radii are not yet
/// modelled; they fall back to [`RadialSize::FarthestCorner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialSize {
    /// Ending shape meets the side(s) nearest the centre.
    ClosestSide,
    /// Ending shape passes through the corner nearest the centre.
    ClosestCorner,
    /// Ending shape meets the side(s) farthest from the centre.
    FarthestSide,
    /// Ending shape passes through the corner farthest from the centre (default).
    FarthestCorner,
}

/// CSS Images L3 §3.5.1 — resolves a `radial-gradient` ending shape to concrete
/// `(radius_x, radius_y)` in CSS px for a box `w×h` with centre at
/// `(cx_pct·w, cy_pct·h)`. For [`RadialShape::Circle`] both radii are equal.
/// Corner sizes use the aspect ratio of the matching side size and scale the
/// ellipse to pass through the chosen corner (CSS Images L3 §3.5.1, last list
/// item). Radii are clamped to ≥ 1 px to avoid a degenerate gradient.
#[must_use]
pub fn radial_gradient_radii(
    shape: RadialShape, size: RadialSize, cx_pct: f32, cy_pct: f32, w: f32, h: f32,
) -> (f32, f32) {
    let cx = cx_pct * w;
    let cy = cy_pct * h;
    let near_x = cx.abs().min((w - cx).abs());
    let far_x = cx.abs().max((w - cx).abs());
    let near_y = cy.abs().min((h - cy).abs());
    let far_y = cy.abs().max((h - cy).abs());
    // Ellipse with aspect ratio `sx:sy` scaled to pass through corner (cdx, cdy).
    let through_corner = |sx: f32, sy: f32, cdx: f32, cdy: f32| -> (f32, f32) {
        let a = (sx / sy.max(1e-6)).max(1e-6); // rx / ry
        let ry = ((cdx / a).powi(2) + cdy * cdy).sqrt().max(1.0);
        ((a * ry).max(1.0), ry)
    };
    match shape {
        RadialShape::Circle => {
            let r = match size {
                RadialSize::ClosestSide => near_x.min(near_y),
                RadialSize::FarthestSide => far_x.max(far_y),
                RadialSize::ClosestCorner => near_x.hypot(near_y),
                RadialSize::FarthestCorner => far_x.hypot(far_y),
            }
            .max(1.0);
            (r, r)
        }
        RadialShape::Ellipse => match size {
            RadialSize::ClosestSide => (near_x.max(1.0), near_y.max(1.0)),
            RadialSize::FarthestSide => (far_x.max(1.0), far_y.max(1.0)),
            RadialSize::ClosestCorner => through_corner(near_x, near_y, near_x, near_y),
            RadialSize::FarthestCorner => through_corner(far_x, far_y, far_x, far_y),
        },
    }
}

/// CSS Backgrounds L3 §3.1 / CSS Images L4 §4 — `background-image` value.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum BackgroundImage {
    #[default]
    None,
    /// `url("path")` or raw `image-set(…)` / `-webkit-image-set(…)` string.
    ///
    /// `image-set()` strings are stored verbatim — paint resolves them to the
    /// best URL for the current DPR via `select_image_set_url` (CSS Images L4 §5).
    Url(String),
    /// Parsed gradient. Phase 0 renders linear / radial / conic.
    Gradient(ParsedGradient),
    /// CSS Images L4 §4 — `cross-fade(<image-a>, <image-b>, <percentage>)`.
    ///
    /// `t` is the blend factor in `[0.0, 1.0]`: `0.0` = fully `a`, `1.0` = fully `b`.
    CrossFade {
        /// First image (`t = 0.0`).
        a: Box<BackgroundImage>,
        /// Second image (`t = 1.0`).
        b: Box<BackgroundImage>,
        /// Blend factor clamped to `[0.0, 1.0]`.
        t: f32,
    },
    /// CSS Paint API (Houdini) — `paint(name)` generates dynamic image via registered worklet.
    /// Phase 0: stored as placeholder grey `DrawImage`; Phase 1: calls worklet `paint()` callback.
    Paint(String),
}

/// CSS Backgrounds L3 §3.4 — `background-repeat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundRepeat {
    #[default]
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
    Round,
    Space,
}

impl BackgroundRepeat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "repeat" => Some(Self::Repeat),
            "no-repeat" => Some(Self::NoRepeat),
            "repeat-x" => Some(Self::RepeatX),
            "repeat-y" => Some(Self::RepeatY),
            "round" => Some(Self::Round),
            "space" => Some(Self::Space),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.5 — one axis of an explicit `background-size` value.
///
/// `Px`/`Percent` are resolved against the positioning area extent along this
/// axis at paint time; `Auto` derives the extent from the other axis (preserving
/// the image's intrinsic aspect ratio).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BgSizeAxis {
    /// Derive this axis from the other axis / the image's intrinsic ratio.
    Auto,
    /// Fixed length in CSS px.
    Px(f32),
    /// Percentage of the positioning area along this axis (fraction `0.0..`).
    Percent(f32),
}

impl BgSizeAxis {
    /// Resolve to a concrete px extent against `area` (the positioning-area
    /// size along this axis). Returns `None` for `Auto` (caller derives it from
    /// the other axis / intrinsic ratio).
    #[must_use]
    pub fn resolve(self, area: f32) -> Option<f32> {
        match self {
            BgSizeAxis::Auto => None,
            BgSizeAxis::Px(v) => Some(v),
            BgSizeAxis::Percent(p) => Some(p * area),
        }
    }
}

/// CSS Backgrounds L3 §3.5 — `background-size`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum BackgroundSize {
    #[default]
    Auto,
    Cover,
    Contain,
    /// Explicit width / height, each `auto` | `<length>` | `<percentage>`.
    /// Percentages resolve against the positioning area at paint time.
    Length(BgSizeAxis, BgSizeAxis),
}

/// CSS Backgrounds L3 §3.6 — `background-attachment`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundAttachment {
    #[default]
    Scroll,
    Fixed,
    Local,
}

impl BackgroundAttachment {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "scroll" => Some(Self::Scroll),
            "fixed" => Some(Self::Fixed),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.7 — `background-origin`. Non-inherited.
///
/// Определяет, к какому **краю box-а** привязана позиционная система
/// для `background-image` (initial = padding edge). На `background-color`
/// не влияет (тот всегда заливает border-edge независимо от origin).
///
/// **Phase 0 ограничение:** parsing + storage only. Реальное смещение
/// origin-у в paint pipeline (выбор `border_box` / `padding_box` /
/// `content_box` rect при расчёте начала tile-тиления) — отдельная
/// задача с согласованием P2 (crate-ownership matrix).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundOrigin {
    /// `border-box` — позиционная система начинается с border-edge.
    BorderBox,
    /// `padding-box` (initial) — с padding-edge (= внутренний край border-а).
    #[default]
    PaddingBox,
    /// `content-box` — с content-edge (= внутренний край padding-а).
    ContentBox,
}

impl BackgroundOrigin {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.8 — `background-clip`. Non-inherited.
///
/// Определяет, к какому **краю box-а** обрезается `background-color`
/// и `background-image` (initial = border edge, т.е. фон видно даже
/// сквозь полупрозрачную рамку).
///
/// Variant `Text` (CSS Backgrounds L4) клипает фон по форме глифов —
/// классический паттерн «gradient text» через `background-clip: text`
/// и `color: transparent`. Реализация в paint требует подмаски через
/// glyph-cache mask-image — отдельная задача с согласованием P2.
///
/// **Phase 0 ограничение:** parsing + storage only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundClip {
    /// `border-box` (initial) — фон под border-ом виден.
    #[default]
    BorderBox,
    /// `padding-box` — фон обрезается до внутреннего края border-а.
    PaddingBox,
    /// `content-box` — фон только в content-area.
    ContentBox,
    /// `text` (CSS Backgrounds L4) — фон клипается по форме текста
    /// внутри box-а. Phase 0 хранит как atom, реальный clip — P2.
    Text,
}

impl BackgroundClip {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// CSS Masking L1 §4.6 — `mask-clip: <coord-box> | no-clip`.
///
/// `<coord-box>` = `content-box | padding-box | border-box | fill-box |
/// stroke-box | view-box`. Unlike `background-clip`, `mask-clip` also accepts
/// the SVG reference boxes and the `no-clip` keyword. For elements laid out
/// with the CSS box model (non-SVG HTML boxes) the SVG-specific boxes fall
/// back to their box-model equivalents (CSS Box 4 §1 "Choosing the layout
/// box"): `fill-box` → content box, `stroke-box`/`view-box` → border box.
/// `no-clip` disables the mask painting-area clip entirely. Non-inherited,
/// initial `border-box`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaskClip {
    /// `border-box` (initial) — mask painting area is the border box.
    #[default]
    BorderBox,
    /// `padding-box` — clip to the inner border edge.
    PaddingBox,
    /// `content-box` — clip to the content area.
    ContentBox,
    /// `fill-box` — object bounding box; for CSS boxes equals the content box.
    FillBox,
    /// `stroke-box` — stroke bounding box; for CSS boxes equals the border box.
    StrokeBox,
    /// `view-box` — nearest SVG viewport; for CSS boxes equals the border box.
    ViewBox,
    /// `no-clip` — the mask painting area is not clipped.
    NoClip,
}

impl MaskClip {
    /// Parses a single `mask-clip` keyword (CSS Masking L1 §4.6).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            "fill-box" => Some(Self::FillBox),
            "stroke-box" => Some(Self::StrokeBox),
            "view-box" => Some(Self::ViewBox),
            "no-clip" => Some(Self::NoClip),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3 — один фоновый слой. Первый в Vec = верхний (рисуется последним).
///
/// Все поля — initial values из спецификации. `background_color` не входит
/// в слой — он всегда одиночный и хранится в `ComputedStyle.background_color`.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundLayer {
    /// `background-image` для этого слоя.
    pub image: BackgroundImage,
    /// `background-repeat` для этого слоя.
    pub repeat: BackgroundRepeat,
    /// `background-size` для этого слоя.
    pub size: BackgroundSize,
    /// `background-position` для этого слоя.
    pub position: ObjectPosition,
    /// `background-attachment` для этого слоя.
    pub attachment: BackgroundAttachment,
    /// `background-origin` для этого слоя.
    pub origin: BackgroundOrigin,
    /// `background-clip` для этого слоя.
    pub clip: BackgroundClip,
    /// CSS Compositing L1 §8.3 — `background-blend-mode` для этого слоя.
    /// Initial: normal. Не наследуется. Применяется при слиянии background
    /// layers между собой (не с контентом элемента).
    pub blend_mode: MixBlendMode,
}

impl Default for BackgroundLayer {
    fn default() -> Self {
        Self {
            image: BackgroundImage::None,
            repeat: BackgroundRepeat::Repeat,
            size: BackgroundSize::Auto,
            position: ObjectPosition::background_initial(),
            attachment: BackgroundAttachment::Scroll,
            origin: BackgroundOrigin::PaddingBox,
            clip: BackgroundClip::BorderBox,
            blend_mode: MixBlendMode::Normal,
        }
    }
}

/// CSS Images L3 §5.5 — `object-fit`. Применяется к replaced elements
/// (`<img>`, `<video>`, `<canvas>` и т.д.) и определяет, как «коробка»
/// заливается содержимым с учётом intrinsic-размеров. Не наследуется.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObjectFit {
    /// `fill` (default) — растянуть на размер коробки без сохранения
    /// aspect ratio. Картинка может быть искажена.
    #[default]
    Fill,
    /// `contain` — максимально большой размер с сохранением aspect ratio,
    /// при котором изображение **умещается** целиком (letterbox / pillarbox).
    Contain,
    /// `cover` — минимально большой размер с сохранением aspect ratio,
    /// при котором изображение **покрывает** коробку. Излишки клипятся
    /// по `object-position`.
    Cover,
    /// `none` — без масштабирования (intrinsic-размер 1:1). Излишки
    /// клипятся; недостаток заполняется по `object-position`.
    None,
    /// `scale-down` — `min(none, contain)`: если intrinsic-размер меньше
    /// коробки, ведёт себя как `none`; иначе как `contain`.
    ScaleDown,
}

impl ObjectFit {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fill" => Some(Self::Fill),
            "contain" => Some(Self::Contain),
            "cover" => Some(Self::Cover),
            "none" => Some(Self::None),
            "scale-down" => Some(Self::ScaleDown),
            _ => None,
        }
    }
}

/// CSS Images L3 §6.1 — `image-rendering`. Hint для движка о том, как
/// масштабировать растровое изображение (применимо к `<img>`, background-image,
/// canvas, и т.д.). Inherited.
///
/// Phase 0: parsing + storage. Реальное переключение GPU sampler filter
/// (`Linear` для `auto`/`smooth`/`high-quality`, `Nearest` для `pixelated`/
/// `crisp-edges`) в `lumen-paint` — отдельная задача с согласованием P2.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageRendering {
    /// `auto` (default) — UA выбирает алгоритм. Обычно — bilinear.
    #[default]
    Auto,
    /// `smooth` — high-quality scaling, оптимизирован для smooth gradient.
    /// На практике в современных движках = `auto`.
    Smooth,
    /// `high-quality` — высочайшее качество масштабирования (тяжелее `smooth`).
    /// Спецификация добавлена в CSS Images L4; считается переименованием
    /// `optimizeQuality` из L3 (которое теперь deprecated).
    HighQuality,
    /// `crisp-edges` — сохраняет контраст и резкость границ (pixel art /
    /// vector graphics). UA может использовать nearest-neighbour или
    /// edge-preserving алгоритм.
    CrispEdges,
    /// `pixelated` — nearest-neighbour. Полезно для масштабирования pixel art.
    Pixelated,
}

impl ImageRendering {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            "high-quality" => Some(Self::HighQuality),
            "crisp-edges" => Some(Self::CrispEdges),
            "pixelated" => Some(Self::Pixelated),
            _ => None,
        }
    }
}

/// CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited.
///
/// Управляет тем, переносятся ли строки внутри блока. `wrap` — нормальный
/// перенос по soft wrap opportunities (initial). `nowrap` — текст растягивается
/// в одну линию, до явного break-control (`<br>`, preserved newline).
///
/// Является non-shorthand-частью `text-wrap` (§6.4.3) и одновременно
/// частью legacy `white-space` shorthand (§2.1 — `white-space-collapse` ||
/// `text-wrap-mode` || `white-space-trim`). В этой кодовой базе `white-space`
/// исторически хранится отдельным [`WhiteSpace`] enum-ом — связка двух полей
/// уйдёт в типизацию декрараций (P1 1B).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapMode {
    /// `wrap` (initial) — обычный перенос строк.
    #[default]
    Wrap,
    /// `nowrap` — без переноса, текст в одну линию.
    Nowrap,
}

impl TextWrapMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wrap" => Some(Self::Wrap),
            "nowrap" => Some(Self::Nowrap),
            _ => None,
        }
    }
}

/// CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited.
///
/// Расширенные стратегии перевода строк. `auto` — UA выбирает по умолчанию
/// (обычно greedy first-fit). Остальные значения — типографические
/// улучшения, требующие реального line-breaker-а (Knuth–Plass / Latin
/// last-line orphan-prevention) — Phase 0 хранит как atom, применение
/// откладывается до интеграции с `UnicodeProvider` (provisional `icu4x`,
/// P1 п.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapStyle {
    /// `auto` (initial) — UA-default стратегия (обычно greedy).
    #[default]
    Auto,
    /// `balance` — балансировать длины строк короткого блока (≤ ~10 строк).
    Balance,
    /// `stable` — стабильные break-points при редактировании (для contenteditable).
    Stable,
    /// `pretty` — улучшенный last-line (без orphan / висячих слов).
    Pretty,
}

impl TextWrapStyle {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "balance" => Some(Self::Balance),
            "stable" => Some(Self::Stable),
            "pretty" => Some(Self::Pretty),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited.
///
/// Задаёт направление главной оси flex-контейнера. Phase 0: parsing + storage;
/// реальный flex-layout pass — задача 4B.3.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexDirection {
    /// `row` (initial) — горизонтально, слева направо.
    #[default]
    Row,
    /// `row-reverse` — горизонтально, справа налево.
    RowReverse,
    /// `column` — вертикально, сверху вниз.
    Column,
    /// `column-reverse` — вертикально, снизу вверх.
    ColumnReverse,
}

impl FlexDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row" => Some(Self::Row),
            "row-reverse" => Some(Self::RowReverse),
            "column" => Some(Self::Column),
            "column-reverse" => Some(Self::ColumnReverse),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited.
///
/// Разрешает или запрещает перенос flex-элементов на новые строки/столбцы.
/// Phase 0: parsing + storage; реальный multi-line flex — задача 4B.5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexWrap {
    /// `nowrap` (initial) — все элементы в одну строку.
    #[default]
    Nowrap,
    /// `wrap` — перенос вперёд (вниз или вправо).
    Wrap,
    /// `wrap-reverse` — перенос назад.
    WrapReverse,
}

impl FlexWrap {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "nowrap" => Some(Self::Nowrap),
            "wrap" => Some(Self::Wrap),
            "wrap-reverse" => Some(Self::WrapReverse),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited.
///
/// Размер flex-элемента вдоль главной оси до применения grow/shrink.
/// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum FlexBasis {
    /// `auto` (initial) — использовать width/height элемента.
    #[default]
    Auto,
    /// `content` — intrinsic content-size (CSS Flexbox L1 §7.3.2).
    Content,
    /// Explicit length/percentage.
    Length(Length),
}

impl FlexBasis {
    pub fn parse(s: &str, is_quirks: bool) -> Option<Self> {
        let trimmed = s.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "content" => Some(Self::Content),
            _ => parse_length_q(trimmed, is_quirks).map(Self::Length),
        }
    }
}

/// CSS Grid Layout L3 §9 — `repeat(auto-fill | auto-fit | <count>, <track-list>)`.
/// Stored in grid_template_columns/rows during Phase 0 to preserve repeat information
/// until resolution time (lay_out_grid). Expanded via `resolve_grid_template` before layout.
#[derive(Debug, Clone, PartialEq)]
pub struct GridRepeat {
    /// `Count::Fixed(N)` for `repeat(N, ...)`, `AutoFill` for auto-fill, `AutoFit` for auto-fit.
    pub count: RepeatCount,
    /// The track sizing functions inside the parentheses, e.g. `minmax(100px, 1fr)`.
    pub tracks: Vec<GridTrackSize>,
}

/// Count type for grid-template-columns/rows `repeat()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatCount {
    /// Fixed count: `repeat(3, ...)`.
    Fixed(usize),
    /// Auto-fill: `repeat(auto-fill, ...)` — fill available space, prefer empty tracks over overflow.
    AutoFill,
    /// Auto-fit: `repeat(auto-fit, ...)` — fill available space, collapse empty tracks.
    AutoFit,
}

/// CSS Grid Layout L1 §7.2 — sizing function for a grid track.
/// Non-inherited. Appears in `grid-template-columns` / `grid-template-rows`
/// and `grid-auto-columns` / `grid-auto-rows`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum GridTrackSize {
    /// `auto` — sized by content (min-content as min, max-content as max).
    #[default]
    Auto,
    /// Fixed length (px, em, rem, %).
    Length(Length),
    /// `<number>fr` — fractional unit of remaining free space.
    Fr(f32),
    /// `min-content` — minimum content size.
    MinContent,
    /// `max-content` — maximum content size.
    MaxContent,
    /// `minmax(min, max)` — track between min and max sizing functions.
    Minmax(Box<GridTrackSize>, Box<GridTrackSize>),
    /// `fit-content(N)` — track sized to fit content with max limit (CSS Grid L3 §9.1).
    /// Equivalent to `minmax(auto, max(auto, min(N, max-content)))`.
    FitContent(Box<GridTrackSize>),
    /// `subgrid` — inherit track sizes from the spanning tracks of the parent grid
    /// (CSS Grid Layout L2 §9). The grid item must itself be a grid container;
    /// its column/row tracks are replaced by the parent's resolved track sizes
    /// for the cells it spans. Stored as a sentinel `vec![GridTrackSize::Subgrid]`
    /// in `grid_template_columns` or `grid_template_rows`.
    Subgrid,
    /// `masonry` — CSS Grid L3 §14 waterfall layout axis sentinel.
    /// Stored as `vec![GridTrackSize::Masonry]` in `grid_template_columns` or
    /// `grid_template_rows` to signal that the axis uses masonry placement.
    /// The perpendicular axis defines track sizes; `masonry.rs` handles placement.
    /// P4 handoff: `masonry-auto-flow`, `align-tracks`, `justify-tracks` in ComputedStyle.
    Masonry,
}

impl GridTrackSize {
    /// Resolve to a concrete pixel size given container width, em, viewport.
    /// For `fr`, `auto`, `fit-content`, `subgrid`, and `masonry` returns `None` — caller handles those specially.
    pub fn resolve_fixed(&self, em: f32, cb: f32, viewport: Size) -> Option<f32> {
        match self {
            Self::Length(l) => l.resolve(em, Some(cb), viewport),
            Self::Fr(_) | Self::Auto | Self::MinContent | Self::MaxContent | Self::FitContent(_) | Self::Subgrid | Self::Masonry => None,
            Self::Minmax(min, _max) => min.resolve_fixed(em, cb, viewport),
        }
    }

    /// True for fractional tracks.
    pub fn is_fr(&self) -> bool {
        matches!(self, Self::Fr(_))
    }

    /// Extract fr value.
    pub fn fr(&self) -> Option<f32> {
        if let Self::Fr(v) = self { Some(*v) } else { None }
    }

    /// True when this track inherits its size from the parent grid (subgrid axis).
    pub fn is_subgrid(&self) -> bool {
        matches!(self, Self::Subgrid)
    }

    /// True when this axis uses masonry placement (CSS Grid L3 §14).
    pub fn is_masonry(&self) -> bool {
        matches!(self, Self::Masonry)
    }

    /// Parse a single track sizing keyword / value (no `repeat()`).
    fn parse_single(s: &str, is_quirks: bool) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "auto" => return Some(Self::Auto),
            "min-content" => return Some(Self::MinContent),
            "max-content" => return Some(Self::MaxContent),
            // `subgrid` / `masonry` as single tokens are handled in parse_track_list;
            // reaching here means they appeared inside a repeat() context — treat as auto.
            "subgrid" | "masonry" => return Some(Self::Auto),
            _ => {}
        }
        // `<number>fr`
        if let Some(n) = lc.strip_suffix("fr")
            && let Ok(v) = n.trim().parse::<f32>()
        {
            return Some(Self::Fr(v.max(0.0)));
        }
        // `minmax(min, max)`
        if lc.starts_with("minmax(") && lc.ends_with(')') {
            let inner = &s.trim()[7..s.trim().len() - 1];
            if let Some((a, b)) = split_paren_aware_comma(inner) {
                let min = Self::parse_single(a.trim(), is_quirks)?;
                let max = Self::parse_single(b.trim(), is_quirks)?;
                return Some(Self::Minmax(Box::new(min), Box::new(max)));
            }
        }
        // `fit-content(<length-percentage>)` (CSS Grid L3 §9.1)
        if lc.starts_with("fit-content(") && lc.ends_with(')') {
            let inner = &s.trim()[12..s.trim().len() - 1];
            if let Some(limit) = Self::parse_single(inner.trim(), is_quirks) {
                return Some(Self::FitContent(Box::new(limit)));
            }
        }
        // length / percentage
        parse_length_q(s.trim(), is_quirks).map(Self::Length)
    }

    /// Parse a track-list value string into a Vec of GridTrackSize.
    /// Handles `repeat(N, <track-list>)` by expanding.
    /// `subgrid` as the entire value returns `vec![Subgrid]` (sentinel for the whole axis).
    /// `masonry` as the entire value returns `vec![Masonry]` (CSS Grid L3 §14 sentinel).
    pub fn parse_track_list(s: &str, is_quirks: bool) -> Vec<Self> {
        let trimmed = s.trim();
        // CSS Grid L2 §9: `subgrid` replaces the entire track list for that axis.
        if trimmed.eq_ignore_ascii_case("subgrid") {
            return vec![Self::Subgrid];
        }
        // CSS Grid L3 §14: `masonry` replaces the entire track list — waterfall placement axis.
        if trimmed.eq_ignore_ascii_case("masonry") {
            return vec![Self::Masonry];
        }
        let mut result = Vec::new();
        for token in split_track_list_tokens(trimmed) {
            let t = token.trim();
            let lc = t.to_ascii_lowercase();
            if lc.starts_with("repeat(") && lc.ends_with(')') {
                let inner = &t[7..t.len() - 1];
                if let Some((count_s, rest)) = split_paren_aware_comma(inner) {
                    let count_s_trim = count_s.trim();
                    let count_lc = count_s_trim.to_ascii_lowercase();
                    let count = if count_lc == "auto-fill" {
                        RepeatCount::AutoFill
                    } else if count_lc == "auto-fit" {
                        RepeatCount::AutoFit
                    } else if let Ok(n) = count_s_trim.parse::<usize>() {
                        RepeatCount::Fixed(n)
                    } else {
                        continue; // Invalid repeat count, skip
                    };

                    let tracks = Self::parse_track_list(rest.trim(), is_quirks);
                    if count == RepeatCount::Fixed(0) {
                        // zero repeat, add nothing
                    } else if matches!(count, RepeatCount::Fixed(_)) {
                        // Expand fixed repeat immediately
                        let n = match count {
                            RepeatCount::Fixed(n) => n,
                            _ => unreachable!(),
                        };
                        for _ in 0..n {
                            result.extend(tracks.iter().cloned());
                        }
                    } else {
                        // For auto-fill / auto-fit, store GridRepeat sentinel for resolution at layout time
                        // Phase 1: Add first track as GridRepeat sentinel. Caller (lay_out_grid) resolves count.
                        // For now, expand as single "repeat" marker that resolver can recognize and expand.
                        if !tracks.is_empty() {
                            // Store info in a way resolver can find: mark with a sentinel or new enum variant.
                            // Currently: add the first track once, and store GridRepeat in ComputedStyle separately.
                            // Phase 1 simplified: treat as auto (no expansion) until resolver wire-up
                            result.extend(tracks.iter().cloned());
                        }
                    }
                }
            } else if let Some(ts) = Self::parse_single(t, is_quirks) {
                result.push(ts);
            }
        }
        result
    }
}

/// Extracts auto-fill/auto-fit repeat metadata from a track-list string.
/// Returns `Some(GridRepeat)` when the string is exactly `repeat(auto-fill|auto-fit, ...)`.
/// Used in Phase 2 of CSS Grid auto-repeat expansion (CSS Grid L1 §7.2.3.4).
pub(crate) fn parse_auto_repeat(s: &str) -> Option<GridRepeat> {
    let trimmed = s.trim();
    // Must start with "repeat(" (case-insensitive) and end with ")"
    let lc = trimmed.to_ascii_lowercase();
    let inner = lc.strip_prefix("repeat(")?.strip_suffix(')')?;
    let (count_s, rest) = split_paren_aware_comma(inner)?;
    let count = match count_s.trim() {
        "auto-fill" => RepeatCount::AutoFill,
        "auto-fit" => RepeatCount::AutoFit,
        _ => return None,
    };
    // Re-parse from original string to preserve case in track sizes
    let orig_inner = trimmed
        .get("repeat(".len()..trimmed.len() - 1)?;
    let (_, orig_rest) = split_paren_aware_comma(orig_inner)?;
    let tracks = GridTrackSize::parse_track_list(orig_rest.trim(), false);
    if tracks.is_empty() {
        return None;
    }
    let _ = rest; // suppress unused warning from lc version
    Some(GridRepeat { count, tracks })
}

/// Split a comma inside a track-list token that may contain nested parens.
fn split_paren_aware_comma(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

/// Tokenize a track-list string into individual track tokens,
/// respecting parentheses (so `minmax(...)` stays as one token).
fn split_track_list_tokens(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b' ' | b'\t' | b'\n' if depth == 0 => {
                let tok = s[start..i].trim();
                if !tok.is_empty() {
                    tokens.push(tok);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        tokens.push(last);
    }
    tokens
}

/// CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GridAutoFlow {
    /// `row` (initial) — fill rows, add new rows as needed.
    #[default]
    Row,
    /// `column` — fill columns, add new columns as needed.
    Column,
    /// `row dense` — row flow with dense packing.
    RowDense,
    /// `column dense` — column flow with dense packing.
    ColumnDense,
}

impl GridAutoFlow {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row" => Some(Self::Row),
            "column" => Some(Self::Column),
            "row dense" | "dense row" => Some(Self::RowDense),
            "column dense" | "dense column" => Some(Self::ColumnDense),
            _ => None,
        }
    }
}

/// CSS Masonry Layout §9 — `masonry-auto-flow`. Controls the placement order
/// of auto-placed items in a masonry container. Non-inherited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MasonryAutoFlow {
    /// `definite-first` (initial) — items with an explicit grid-axis position are
    /// placed first, then auto items in source order.
    #[default]
    DefiniteFirst,
    /// `next` — all items placed in source order, no definite-first prioritisation.
    Next,
    /// `ordered` — items sorted by their CSS `order` property before placement.
    Ordered,
}

impl MasonryAutoFlow {
    /// Parse a CSS `masonry-auto-flow` value string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "definite-first" => Some(Self::DefiniteFirst),
            "next" => Some(Self::Next),
            "ordered" => Some(Self::Ordered),
            _ => None,
        }
    }
}

/// CSS Grid Layout L1 §8.3 — a grid-line reference for grid-column-start,
/// grid-column-end, grid-row-start, grid-row-end. Non-inherited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GridLine {
    /// `auto` — automatic placement.
    #[default]
    Auto,
    /// Integer line number (1-based from start, negative from end).
    Line(i32),
    /// `span <integer>` — span N tracks.
    Span(u32),
    /// Named grid area reference (CSS Grid L1 §8.3). Resolved at layout time
    /// by looking up the name in the containing grid's `grid-template-areas`.
    Named(String),
}

impl GridLine {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        // `span N` or `span`
        if let Some(rest) = trimmed.to_ascii_lowercase().strip_prefix("span") {
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(Self::Span(1));
            }
            if let Ok(n) = rest.parse::<u32>() {
                return Some(Self::Span(n.max(1)));
            }
        }
        // integer line number
        if let Ok(n) = trimmed.parse::<i32>() && n != 0 {
            return Some(Self::Line(n));
        }
        // CSS custom-ident: named grid area or named line.
        // Only accept valid CSS idents (letters, digits, hyphens, underscores;
        // cannot start with a digit or two hyphens without a letter).
        if is_css_ident(trimmed) {
            return Some(Self::Named(trimmed.to_string()));
        }
        None
    }
}

/// Одна компонента `object-position`. Length-варианты резолвятся в px
/// относительно края коробки (positive = от left/top); percentage —
/// относительно **свободного места** `box_size - content_size` (может быть
/// отрицательным, тогда излишек уходит за противоположный край). См.
/// CSS Images L3 §5.5 «object-position».
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionComponent {
    /// Length в px (после resolve em/rem/vw/...).
    Px(f32),
    /// Percentage в долях 1.0 (`50%` → 0.5). Резолвится на paint-стадии
    /// против свободного места: `offset = free_space * percent`.
    Percent(f32),
}

impl PositionComponent {
    /// Резолв в финальный px-offset относительно левого/верхнего края
    /// коробки. `free_space = box_size - content_size`; может быть
    /// отрицательным (content > box) — тогда offset тоже отрицательный,
    /// и излишек уезжает за противоположный край.
    pub fn resolve(self, free_space: f32) -> f32 {
        match self {
            Self::Px(px) => px,
            Self::Percent(p) => free_space * p,
        }
    }
}

/// CSS Images L3 §5.5 — `object-position` (две компоненты, x + y).
/// Default — `50% 50%` (центр).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectPosition {
    pub x: PositionComponent,
    pub y: PositionComponent,
}

impl Default for ObjectPosition {
    fn default() -> Self {
        Self {
            x: PositionComponent::Percent(0.5),
            y: PositionComponent::Percent(0.5),
        }
    }
}

impl ObjectPosition {
    /// CSS Backgrounds L3 §3.5 — initial value `background-position: 0% 0%`
    /// (top-left). Отличается от Object Position default (`50% 50%`, центр)
    /// специально потому, что `background-image` обычно anchored к top-left
    /// при первой укладке (см. CSS 2.1 §14.2.1).
    pub const fn background_initial() -> Self {
        Self {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        }
    }
}

impl ObjectPosition {
    /// CSS Values L4 §9.4 — `<position>` для object-position. Phase 0
    /// поддерживает:
    ///   - keyword `center` (= 50%),
    ///   - axis-keywords `left|right|top|bottom`,
    ///   - один token (`50%`, `10px`, keyword) — второй = `center`,
    ///   - два token-а — первый x, второй y.
    ///
    /// Tri- и quad-форма (`<keyword> <length> <keyword> <length>` для
    /// сторон-якорей) — отложены: на современных страницах редкость.
    pub fn parse(s: &str, em_basis: f32, viewport: Size) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > 2 {
            return None;
        }
        // Single-token: применяется к horizontal оси; вертикальная = center.
        // Если token — vertical keyword (`top`/`bottom`), то horizontal = center.
        if tokens.len() == 1 {
            let t = tokens[0];
            if t.eq_ignore_ascii_case("top") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(0.0),
                });
            }
            if t.eq_ignore_ascii_case("bottom") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(1.0),
                });
            }
            let x = parse_position_component(t, em_basis, viewport, /*vertical*/ false)?;
            return Some(Self {
                x,
                y: PositionComponent::Percent(0.5),
            });
        }
        // Two-token form: <x> <y>. Swap, если порядок инвертирован
        // (`top left` ≡ `left top`).
        let (t0, t1) = (tokens[0], tokens[1]);
        let (xtok, ytok) = if is_vertical_keyword(t0) || is_horizontal_keyword(t1) {
            (t1, t0)
        } else {
            (t0, t1)
        };
        let x = parse_position_component(xtok, em_basis, viewport, false)?;
        let y = parse_position_component(ytok, em_basis, viewport, true)?;
        Some(Self { x, y })
    }
}

fn is_vertical_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("top") || t.eq_ignore_ascii_case("bottom")
}

fn is_horizontal_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("left") || t.eq_ignore_ascii_case("right")
}

fn parse_position_component(
    t: &str,
    em_basis: f32,
    viewport: Size,
    vertical: bool,
) -> Option<PositionComponent> {
    // Keyword-формы.
    if t.eq_ignore_ascii_case("center") {
        return Some(PositionComponent::Percent(0.5));
    }
    if !vertical {
        if t.eq_ignore_ascii_case("left") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("right") {
            return Some(PositionComponent::Percent(1.0));
        }
        // top/bottom в horizontal-позиции — недопустимо.
        if is_vertical_keyword(t) {
            return None;
        }
    } else {
        if t.eq_ignore_ascii_case("top") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("bottom") {
            return Some(PositionComponent::Percent(1.0));
        }
        if is_horizontal_keyword(t) {
            return None;
        }
    }
    // Length / percentage. Percent-форма `50%` сохраняется как доля 0..=1
    // (без clamp — отрицательные и >100% валидны по спеке и используются
    // художниками для художественных смещений).
    if let Some(pct) = t.strip_suffix('%')
        && let Ok(n) = pct.trim().parse::<f32>()
    {
        return Some(PositionComponent::Percent(n / 100.0));
    }
    let len = parse_length(t)?;
    let px = len.resolve(em_basis, None, viewport)?;
    Some(PositionComponent::Px(px))
}

/// CSS Box Alignment L3 §6.1 — значения для align-/justify- свойств.
/// Phase 0: основной набор keyword-ов. `Auto` — default (resolve в
/// `Normal` или specific behavior контекстом).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AlignValue {
    /// CSS keyword `auto` — default. Behavior зависит от контекста
    /// (parent layout type). Для absolute-positioned — `normal`.
    #[default]
    Auto,
    /// `normal` — default-behavior для conteneur'а (stretch for grid,
    /// start for flex).
    Normal,
    /// `stretch` — растянуть на доступное место (default для grid).
    Stretch,
    /// `start` / `flex-start` — выровнять к началу cross/main axis.
    Start,
    /// `end` / `flex-end` — выровнять к концу.
    End,
    /// `center` — выровнять по центру.
    Center,
    /// `baseline` — выровнять text-baseline (для align-items).
    Baseline,
    /// `space-between` — равные промежутки между items, по краям нет.
    SpaceBetween,
    /// `space-around` — промежутки между + половинные по краям.
    SpaceAround,
    /// `space-evenly` — все промежутки одинаковые, включая края.
    SpaceEvenly,
}

impl AlignValue {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "auto" => Some(Self::Auto),
            "normal" => Some(Self::Normal),
            "stretch" => Some(Self::Stretch),
            "start" | "flex-start" | "self-start" => Some(Self::Start),
            "end" | "flex-end" | "self-end" => Some(Self::End),
            "center" => Some(Self::Center),
            "baseline" | "first baseline" | "last baseline" => Some(Self::Baseline),
            "space-between" => Some(Self::SpaceBetween),
            "space-around" => Some(Self::SpaceAround),
            "space-evenly" => Some(Self::SpaceEvenly),
            _ => None,
        }
    }
}

/// CSS Masking L1 §3.5 — `<length-percentage>` значение координаты/размера
/// basic-shape для `clip-path`. Проценты резолвятся на этапе paint
/// относительно reference box (border-box элемента): горизонтальные — по
/// width, вертикальные — по height, радиус `circle()` — по
/// `sqrt(w²+h²)/√2` (CSS Shapes L1 §5.1, BUG-140).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeValue {
    /// Абсолютное значение в px (em/rem уже приведены к px при парсинге).
    Px(f32),
    /// Процент соответствующего базиса reference box (0–100).
    Pct(f32),
}

impl ShapeValue {
    /// Резолвит значение в px. `basis` — размер reference box по
    /// соответствующей оси (px); для `Px` игнорируется.
    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            Self::Px(v) => v,
            Self::Pct(p) => p / 100.0 * basis,
        }
    }
}

/// CSS Masking L1 §3.5 — basic-shapes для `clip-path`. Phase 0
/// поддерживает: `inset(...)`, `circle(...)`, `ellipse(...)`,
/// `polygon(...)`. URL / `path()` / `none` отложены.
/// Координаты — `ShapeValue` (px или %), проценты резолвятся по
/// border-box на этапе paint (BUG-140: `circle(40% at 50% 50%)` раньше
/// молча отбрасывался целиком).
#[derive(Debug, Clone, PartialEq)]
pub enum ClipPath {
    /// `inset(top right bottom left)` — 1..=4 length-percentage значения
    /// (top/bottom — % от height, left/right — % от width).
    Inset(Vec<ShapeValue>),
    /// `circle(radius at cx cy)` — radius и center (опц.).
    Circle {
        /// Радиус: % резолвится по `sqrt(w²+h²)/√2`.
        radius: ShapeValue,
        /// Центр (cx — % от width, cy — % от height); `None` = 50% 50%.
        center: Option<(ShapeValue, ShapeValue)>,
    },
    /// `ellipse(rx ry at cx cy)` — rx — % от width, ry — % от height.
    Ellipse {
        /// Горизонтальный радиус.
        rx: ShapeValue,
        /// Вертикальный радиус.
        ry: ShapeValue,
        /// Центр; `None` = 50% 50%.
        center: Option<(ShapeValue, ShapeValue)>,
    },
    /// `polygon([<fill-rule>,]? x1 y1, x2 y2, ...)` — список вершин (x — % от
    /// width, y — % от height) + правило заливки. `FillRule` (CSS Shapes L1
    /// §3) управляет самопересекающимися полигонами: `EvenOdd` оставляет
    /// «дырки» в местах перекрытия, `NonZero` (default) заливает их.
    Polygon(Vec<(ShapeValue, ShapeValue)>, FillRule),
    /// `path([<fill-rule>,]? "<svg-path>")` — CSS Shapes L1 §4. Хранит
    /// предварительно флэттенный полигон в px-координатах системы пути
    /// (origin = верхний левый угол reference box; проценты в `path()`
    /// недопустимы по спецификации). Кривые разбиты на отрезки на этапе
    /// парсинга через `motion_path::flatten_path_to_polygon`. Второе поле —
    /// `FillRule` (default `NonZero`); `EvenOdd` делает дырки в
    /// самопересекающихся путях (звёзды-пентаграммы и т. п.).
    Path(Vec<(f32, f32)>, FillRule),
}

/// CSS Transforms L1 §11 — функции `transform`. Phase 0 поддерживает
/// translate/translateX/translateY, rotate, scale/scaleX/scaleY,
/// CSS Transforms L2 §6 — `transform-style: flat | preserve-3d`.
/// `Flat` = children are flattened into the parent plane (default).
/// `Preserve3d` = children participate in the parent 3D rendering context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformStyle {
    #[default]
    Flat,
    Preserve3d,
}

/// CSS Transforms L2 §5.1 — `backface-visibility: visible | hidden`.
/// `Hidden` = element is invisible when its back face is oriented toward
/// the viewer (requires a 3D rendering context to have any effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackfaceVisibility {
    /// Back face is visible (initial value).
    #[default]
    Visible,
    /// Back face is hidden.
    Hidden,
}

/// CSS transform functions — translate/scale/rotate/skew/skewX/skewY/matrix
/// and all 3D variants (CSS Transforms L2).
#[derive(Debug, Clone, PartialEq)]
pub enum TransformFn {
    Translate(f32, f32),
    TranslateX(f32),
    TranslateY(f32),
    /// `translateZ(<length>)` — translate along Z axis in px.
    TranslateZ(f32),
    /// `translate3d(<tx>, <ty>, <tz>)` — all three axes in px.
    Translate3d(f32, f32, f32),
    /// Угол в радианах (нормализован парсером из deg/rad/turn/grad).
    Rotate(f32),
    /// `rotateX(<angle>)` — angle in radians.
    RotateX(f32),
    /// `rotateY(<angle>)` — angle in radians.
    RotateY(f32),
    /// `rotateZ(<angle>)` — alias for 2D rotate, angle in radians.
    RotateZ(f32),
    /// `rotate3d(<x>, <y>, <z>, <angle>)` — arbitrary axis rotation.
    Rotate3d(f32, f32, f32, f32),
    Scale(f32, f32),
    ScaleX(f32),
    ScaleY(f32),
    /// `scaleZ(<s>)` — scale along Z axis.
    ScaleZ(f32),
    /// `scale3d(<sx>, <sy>, <sz>)` — all three axes.
    Scale3d(f32, f32, f32),
    SkewX(f32),
    SkewY(f32),
    Matrix([f32; 6]),
    /// `matrix3d(<16 values>)` — column-major 4×4 matrix.
    Matrix3d([f32; 16]),
    /// `perspective(<length>)` — perspective distance in px (> 0).
    Perspective(f32),
}

/// CSS Filter Effects L1 §3 — функции `filter`. Phase 0 поддерживает
/// все 9 стандартных функций кроме `drop-shadow` (требует rendering
/// pass — отложено).
#[derive(Debug, Clone, PartialEq)]
pub enum FilterFn {
    /// `blur(<length>)` — радиус gaussian blur.
    Blur(f32),
    /// `brightness(<number-percentage>)`. 1.0 = unchanged.
    Brightness(f32),
    /// `contrast(<number-percentage>)`. 1.0 = unchanged.
    Contrast(f32),
    /// `grayscale(<number-percentage>)`. 0.0 = unchanged, 1.0 = full grayscale.
    Grayscale(f32),
    /// `hue-rotate(<angle>)` — угол в радианах.
    HueRotate(f32),
    /// `invert(<number-percentage>)`. 0.0 = unchanged, 1.0 = inverted.
    Invert(f32),
    /// `opacity(<number-percentage>)`. 1.0 = unchanged.
    Opacity(f32),
    /// `saturate(<number-percentage>)`. 1.0 = unchanged.
    Saturate(f32),
    /// `sepia(<number-percentage>)`. 0.0 = unchanged, 1.0 = full sepia.
    Sepia(f32),
}

/// CSS Images L3 §3.4 — единичный `<color-stop>` градиента.
///
/// `position == None` означает auto-распределение: при resolve до used-value
/// auto-stops равномерно разносятся между фиксированными соседями (spec §3.4.3
/// "Color stop processing"). Здесь типизация специфицированного значения —
/// auto хранится как `None`, без раскрытия.
///
/// Только цвет и позиция (length / percentage). Hint-stops (`<color-stop>,
/// <length-percentage>, <color-stop>`) — без позиции цвета, чисто
/// midpoint-маркер — пока не моделируем: они отрабатывают на интерполяции
/// между соседями и не имеют animation-смысла на уровне per-stop pair.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GradientStop {
    pub color: Color,
    /// Source color space of this stop. `Srgb` for legacy `<color>` values;
    /// `DisplayP3` / `Rec2020` when the stop was written as `color(display-p3 …)`
    /// / `color(rec2020 …)`. Carried through the display list so the renderer
    /// can apply the correct output transform (ph3-color-management Step 3).
    pub color_space: ColorSpace,
    pub position: Option<Length>,
}

/// CSS Masking L1 §6.4 — `mask-mode`. Selects which channel of the mask image
/// is used as the per-pixel mask value when compositing the masked element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskMode {
    /// Use the alpha channel of the mask image directly. Initial behaviour for
    /// `<image>` mask sources (`match-source` resolves here for gradients/URLs).
    #[default]
    Alpha,
    /// Use luminance (`0.2126·R + 0.7152·G + 0.0722·B`, sRGB) multiplied by the
    /// source alpha as the mask value. A dark mask pixel hides the element even
    /// when fully opaque.
    Luminance,
}

/// CSS Masking L1 §4.7 — `mask-composite`. Determines how a mask layer is
/// combined with the mask already assembled from the layers **below** it
/// (Porter-Duff on the mask channel: `add` = source-over, `subtract` =
/// source-out, `intersect` = source-in, `exclude` = xor).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaskComposite {
    /// `add` (initial) — Porter-Duff source-over on the mask channel.
    #[default]
    Add,
    /// `subtract` — Porter-Duff source-out: the layer below is removed where
    /// this layer paints.
    Subtract,
    /// `intersect` — Porter-Duff source-in: only the overlap survives.
    Intersect,
    /// `exclude` — Porter-Duff xor: the overlap is removed.
    Exclude,
}

impl MaskComposite {
    /// Parses a single `mask-composite` keyword (CSS Masking L1 §4.7).
    /// Case-insensitive; returns `None` on an unrecognised keyword.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "add" => Some(Self::Add),
            "subtract" => Some(Self::Subtract),
            "intersect" => Some(Self::Intersect),
            "exclude" => Some(Self::Exclude),
            _ => None,
        }
    }
}

/// CSS Masking L1 §4.9 — один слой маски.
///
/// `mask-image` задаёт количество слоёв; остальные longhand-ы циклически
/// повторяются по этому количеству («Layering Multiple Mask Layers»). Тот же
/// приём, что и у [`BackgroundLayer`], поэтому типы значений переиспользуются
/// из background (`mask-repeat` / `mask-size` / `mask-position` имеют ту же
/// грамматику, `mask-clip` — надмножество `background-clip`).
///
/// Порядок в [`ComputedStyle::mask_layers`]: первый = верхний слой. Слои
/// собираются в одну маску снизу вверх, каждый — оператором своего
/// [`MaskLayer::composite`].
#[derive(Debug, Clone, PartialEq)]
pub struct MaskLayer {
    /// `mask-image` этого слоя. `BackgroundImage` переиспользуется как тип
    /// (та же структура: None / Url / Gradient).
    pub image: BackgroundImage,
    /// `mask-repeat` этого слоя (§4.3). Initial `repeat`.
    pub repeat: BackgroundRepeat,
    /// `mask-size` этого слоя (§4.2). Initial `auto`.
    pub size: BackgroundSize,
    /// `mask-position` этого слоя (§4.4). Initial `center`.
    pub position: ObjectPosition,
    /// `mask-origin` этого слоя (§4.5). Initial `border-box`.
    pub origin: BackgroundOrigin,
    /// `mask-clip` этого слоя (§4.6). Initial `border-box`.
    pub clip: MaskClip,
    /// `mask-mode` этого слоя (§6.4). Initial `match-source`, который для
    /// поддерживаемых `<image>`-источников резолвится в `alpha`.
    pub mode: MaskMode,
    /// `mask-composite` этого слоя (§4.7) — оператор смешивания с уже
    /// собранными слоями ниже. Initial `add`.
    pub composite: MaskComposite,
}

impl Default for MaskLayer {
    fn default() -> Self {
        Self {
            image: BackgroundImage::None,
            repeat: BackgroundRepeat::Repeat,
            size: BackgroundSize::Auto,
            position: ObjectPosition::default(),
            origin: BackgroundOrigin::BorderBox,
            clip: MaskClip::BorderBox,
            mode: MaskMode::Alpha,
            composite: MaskComposite::Add,
        }
    }
}

impl ComputedStyle {
    /// CSS 2.1 §17.6.1 / Basic UI L4 §5.2 — **used** value `outline-width`
    /// равно 0, если `outline-style` равен `none` (это spec, не аппроксимация).
    /// Computed `outline_width` хранится как есть (medium = 3 по UA convention),
    /// чтобы `outline-style: solid` без явного width давал видимый outline.
    pub fn outline_used_width(&self) -> f32 {
        if matches!(self.outline_style, OutlineStyle::None) {
            0.0
        } else {
            self.outline_width
        }
    }

    /// Два стиля рендерят текст одинаково (цвет, размер, интерлиньяж, начертание,
    /// насыщенность, letter/word-spacing, декорация). Используется для слияния
    /// inline-фрагментов в wrap_inline_run.
    pub fn text_rendering_eq(&self, other: &Self) -> bool {
        self.color == other.color
            && (self.font_size - other.font_size).abs() < f32::EPSILON
            && (self.line_height - other.line_height).abs() < f32::EPSILON
            && self.font_style == other.font_style
            && self.font_weight == other.font_weight
            && self.font_variant_caps == other.font_variant_caps
            && self.font_stretch == other.font_stretch
            && self.font_feature_settings == other.font_feature_settings
            && (self.letter_spacing - other.letter_spacing).abs() < f32::EPSILON
            && (self.word_spacing - other.word_spacing).abs() < f32::EPSILON
            && self.text_decoration_line == other.text_decoration_line
            && self.text_decoration_color == other.text_decoration_color
            && self.text_decoration_style == other.text_decoration_style
            && self.text_decoration_thickness == other.text_decoration_thickness
    }

    /// Стартовые значения для корня документа.
    pub fn root() -> Self {
        Self {
            display: Display::Block,
            text_align: TextAlign::Start,
            text_align_last: TextAlignLast::Auto,
            direction: Direction::Ltr,
            unicode_bidi: UnicodeBidi::Normal,
            color: Color::BLACK,
            color_space: ColorSpace::Srgb,
            background_color: None,
            font_size: 16.0,
            // No ancestor and no declaration yet — `zoom` starts neutral.
            effective_zoom: 1.0,
            line_height: 1.2,
            line_height_is_relative: true,
            line_height_step: 0.0,
            font_style: FontStyle::Normal,
            font_weight: FontWeight::NORMAL,
            font_variant_caps: FontVariantCaps::Normal,
            font_variant_emoji: FontVariantEmoji::Normal,
            font_stretch: FontStretch::NORMAL,
            font_family: default_font_family(),
            font_variation_settings: Vec::new(),
            font_feature_settings: Vec::new(),
            font_palette: FontPalette::Normal,
            font_palette_resolved: None,
            font_optical_sizing: FontOpticalSizing::Auto,
            text_transform: TextTransform::None,
            white_space: WhiteSpace::Normal,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            text_indent: Length::Px(0.0),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_decoration_line: TextDecorationLine::default(),
            text_decoration_color: CssColor::CurrentColor,
            text_decoration_style: TextDecorationStyle::Solid,
            text_decoration_thickness: TextDecorationThickness::Auto,
            text_emphasis_style: TextEmphasisStyle::None,
            text_emphasis_color: CssColor::CurrentColor,
            text_emphasis_position: TextEmphasisPosition::OverRight,
            text_underline_position: TextUnderlinePosition::Auto,
            text_underline_offset: None,
            text_decoration_skip_ink: TextDecorationSkipInk::Auto,
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            margin_top: LengthOrAuto::ZERO,
            margin_right: LengthOrAuto::ZERO,
            margin_bottom: LengthOrAuto::ZERO,
            margin_left: LengthOrAuto::ZERO,
            padding_top: Length::Px(0.0),
            padding_right: Length::Px(0.0),
            padding_bottom: Length::Px(0.0),
            padding_left: Length::Px(0.0),
            border_top_width: 0.0,
            border_right_width: 0.0,
            border_bottom_width: 0.0,
            border_left_width: 0.0,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
            border_top_color: CssColor::CurrentColor,
            border_right_color: CssColor::CurrentColor,
            border_bottom_color: CssColor::CurrentColor,
            border_left_color: CssColor::CurrentColor,
            box_sizing: BoxSizing::ContentBox,
            position: Position::Static,
            top: LengthOrAuto::Auto,
            right: LengthOrAuto::Auto,
            bottom: LengthOrAuto::Auto,
            left: LengthOrAuto::Auto,
            z_index: None,
            float_side: FloatSide::None,
            clear: ClearSide::None,
            initial_letter_size: 1.0,
            initial_letter_sink: 0,
            isolation: Isolation::Auto,
            mix_blend_mode: MixBlendMode::Normal,
            border_top_left_radius: Length::Px(0.0),
            border_top_right_radius: Length::Px(0.0),
            border_bottom_right_radius: Length::Px(0.0),
            border_bottom_left_radius: Length::Px(0.0),
            border_top_left_radius_y: Length::Px(0.0),
            border_top_right_radius_y: Length::Px(0.0),
            border_bottom_right_radius_y: Length::Px(0.0),
            border_bottom_left_radius_y: Length::Px(0.0),
            visibility: Visibility::Visible,
            cursor: Cursor::Auto,
            box_shadow: Vec::new(),
            text_shadow: Vec::new(),
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            overflow_clip_margin: None,
            text_overflow: TextOverflow::Clip,
            opacity: 1.0,
            outline_width: 3.0,
            outline_style: OutlineStyle::None,
            outline_color: OutlineColor::Auto,
            outline_offset: Length::Px(0.0),
            accent_color: None,
            color_scheme: ColorScheme::Normal,
            forced_color_adjust: ForcedColorAdjust::Auto,
            custom_props: CustomProps::default(),
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            counter_set: Vec::new(),
            clip_path: None,
            transform: Vec::new(),
            translate: None,
            rotate: None,
            scale: None,
            filter: Vec::new(),
            row_gap: Length::Px(0.0),
            column_gap: Length::Px(0.0),
            column_count: None,
            column_width: None,
            column_rule_width: 0.0,
            column_rule_style: BorderStyle::None,
            column_rule_color: CssColor::CurrentColor,
            gap_rule_width: 0.0,
            gap_rule_style: BorderStyle::None,
            gap_rule_color: CssColor::CurrentColor,
            border_collapse: BorderCollapse::Separate,
            empty_cells: EmptyCells::Show,
            border_spacing_h: 0.0,
            border_spacing_v: 0.0,
            column_span_all: false,
            column_fill_balance: true,
            break_before: BreakValue::Auto,
            break_after: BreakValue::Auto,
            break_inside: BreakValue::Auto,
            aspect_ratio: None,
            align_items: AlignValue::Auto,
            align_self: AlignValue::Auto,
            align_content: AlignValue::Auto,
            justify_items: AlignValue::Auto,
            justify_self: AlignValue::Auto,
            justify_content: AlignValue::Auto,
            background_layers: Vec::new(),
            will_change: Vec::new(),
            pointer_events: PointerEvents::Auto,
            touch_action: TouchAction::Auto,
            appearance: Appearance::Auto,
            field_sizing: FieldSizing::Fixed,
            user_select: UserSelect::Auto,
            resize: Resize::None,
            scroll_behavior: ScrollBehavior::Auto,
            // CSS Scroll Snap / Overscroll defaults.
            scroll_snap_type: ScrollSnapType::default(),
            scroll_snap_align: ScrollSnapAlign::default(),
            scroll_snap_stop: ScrollSnapStop::default(),
            scroll_margin_top: 0.0,
            scroll_margin_right: 0.0,
            scroll_margin_bottom: 0.0,
            scroll_margin_left: 0.0,
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            overscroll_behavior_x: OverscrollBehavior::Auto,
            overscroll_behavior_y: OverscrollBehavior::Auto,
            // CSS Text typography defaults.
            tab_size: 64.0,  // 8 spaces × 8px-space-width default.
            caret_color: None,  // `auto`.
            overflow_wrap: OverflowWrap::Normal,
            word_break: WordBreak::Normal,
            line_break: LineBreak::Auto,
            hyphens: Hyphens::Manual,
            transform_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5), 0.0),
            perspective: None,
            perspective_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5)),
            transform_style: TransformStyle::Flat,
            backface_visibility: BackfaceVisibility::Visible,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            list_style_image: None,
            transition_properties: Vec::new(),
            transition_durations: Vec::new(),
            transition_delays: Vec::new(),
            transition_timing_functions: Vec::new(),
            transition_fill_modes: Vec::new(),
            animation_names: Vec::new(),
            animation_durations: Vec::new(),
            animation_timing_functions: Vec::new(),
            animation_delays: Vec::new(),
            animation_iteration_counts: Vec::new(),
            animation_directions: Vec::new(),
            animation_fill_modes: Vec::new(),
            animation_play_states: Vec::new(),
            animation_timelines: Vec::new(),
            scroll_timeline_name: None,
            scroll_timeline_axis: ScrollAxis::Block,
            view_timeline_name: None,
            view_timeline_axis: ScrollAxis::Block,
            mask_layers: Vec::new(),
            scrollbar_width: ScrollbarWidth::Auto,
            scrollbar_color: None,
            scrollbar_gutter: ScrollbarGutter::Auto,
            content: Content::Normal,
            object_fit: ObjectFit::Fill,
            object_position: ObjectPosition::default(),
            vertical_align: VerticalAlign::Baseline,
            image_rendering: ImageRendering::Auto,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Nowrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: FlexBasis::Auto,
            order: 0,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_template_col_auto_repeat: None,
            grid_template_row_auto_repeat: None,
            grid_template_areas: Vec::new(),
            grid_auto_flow: GridAutoFlow::Row,
            masonry_auto_flow: MasonryAutoFlow::DefiniteFirst,
            grid_auto_columns: GridTrackSize::Auto,
            grid_auto_rows: GridTrackSize::Auto,
            grid_column_start: GridLine::Auto,
            grid_column_end: GridLine::Auto,
            grid_row_start: GridLine::Auto,
            grid_row_end: GridLine::Auto,
            text_wrap_mode: TextWrapMode::Wrap,
            text_wrap_style: TextWrapStyle::Auto,
            line_clamp: None,
            orphans: 2,
            widows: 2,
            contain: ContainFlags::NONE,
            content_visibility: ContentVisibility::Visible,
            contain_intrinsic_width: None,
            contain_intrinsic_width_auto: false,
            contain_intrinsic_height: None,
            contain_intrinsic_height_auto: false,
            interpolate_size: InterpolateSizeMode::NumericOnly,
            container_type: ContainerType::Normal,
            container_name: Vec::new(),
            backdrop_filter: Vec::new(),
            print_color_adjust: PrintColorAdjust::Economy,
            font_size_adjust: FontSizeAdjust::None,
            writing_mode: WritingMode::HorizontalTb,
            text_orientation: TextOrientation::Mixed,
            ruby_position: RubyPosition::Over,
            ruby_align: RubyAlign::SpaceAround,
            ruby_merge: RubyMerge::Separate,
            math_style: MathStyle::Normal,
            math_depth: 0,
            shape_outside: ShapeOutside::None,
            shape_margin: Length::Px(0.0),
            shape_image_threshold: 0.0,
            offset_path: None,
            offset_distance: Length::Px(0.0),
            offset_rotate: OffsetRotate::Auto,
            offset_anchor: None,
            // SVG presentation attributes — inherited. Initial per SVG §11.2/11.3/11.4.
            svg_fill: SvgPaint::Color(Color::BLACK),
            svg_fill_opacity: 1.0,
            svg_stroke: SvgPaint::None,
            svg_stroke_opacity: 1.0,
            svg_stroke_width: 1.0,
            svg_fill_rule: FillRule::NonZero,
            svg_clip_rule: FillRule::NonZero,
            svg_stroke_linecap: StrokeLinecap::Butt,
            svg_stroke_linejoin: StrokeLinejoin::Miter,
            svg_stroke_miterlimit: 4.0,
            svg_stroke_dasharray: Vec::new(),
            svg_stroke_dashoffset: 0.0,
            paint_order: SvgPaintOrder::default(),
            text_anchor: None,
            dominant_baseline: None,
            baseline_shift: crate::box_tree::SvgBaselineShift::Baseline,
            // CSS Logical Properties L1 — initial values.
            inline_size: None,
            block_size: None,
            inset_inline_start: LengthOrAuto::Auto,
            inset_inline_end: LengthOrAuto::Auto,
            inset_block_start: LengthOrAuto::Auto,
            inset_block_end: LengthOrAuto::Auto,
            margin_inline_start: LengthOrAuto::ZERO,
            margin_inline_end: LengthOrAuto::ZERO,
            margin_block_start: LengthOrAuto::ZERO,
            margin_block_end: LengthOrAuto::ZERO,
            padding_inline_start: Length::Px(0.0),
            padding_inline_end: Length::Px(0.0),
            padding_block_start: Length::Px(0.0),
            padding_block_end: Length::Px(0.0),
            border_inline_start_width: 0.0,
            border_inline_end_width: 0.0,
            border_block_start_width: 0.0,
            border_block_end_width: 0.0,
            anchor_name: None,
            position_anchor: None,
            inset_area_row: crate::anchor::InsetAreaKeyword::None,
            inset_area_col: crate::anchor::InsetAreaKeyword::None,
            anchor_scope: crate::anchor::AnchorScope::None,
            anchor_size_w: None,
            anchor_size_h: None,
            anchor_top: None,
            anchor_right: None,
            anchor_bottom: None,
            anchor_left: None,
            view_transition_name: None,
            quotes: Quotes::Auto,
        }
    }
}

/// BUG-341 S18 — process-wide tally of full [`compute_style`] runs.
///
/// The cascade stage's own [`crate::counters::CascadeStats`] counts only the
/// calls `counters::walk` makes. It cannot see the ones the box-build stage
/// makes behind its back: `is_inline_content` / `is_inline_block` probe every
/// child of every rebuilt container with a fresh `compute_style` instead of the
/// `CounterMap` cache `build_box` itself uses, and non-element nodes have no
/// cache entry at all. Process-wide (an atomic, not a thread-local) because
/// `build_box` fans out over rayon workers — the S15 trap.
static COMPUTE_STYLE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Returns the number of [`compute_style`] runs since the last drain, and
/// resets the tally (see [`COMPUTE_STYLE_CALLS`]).
pub fn take_compute_style_calls() -> u64 {
    COMPUTE_STYLE_CALLS.swap(0, std::sync::atomic::Ordering::Relaxed)
}

/// Bumps the [`COMPUTE_STYLE_CALLS`] tally.
fn note_compute_style() {
    COMPUTE_STYLE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Computes the `ComputedStyle` for `node` by running the CSS cascade.
///
/// `dark_mode` is forwarded to `@media (prefers-color-scheme: dark)` matching.
/// CSS Viewport L1 §5 — parse the specified value of `zoom`.
///
/// Accepted: a non-negative `<number>` (`0.8`, `.8`, `1`), a `<percentage>`
/// (`80%`), and the keywords `normal` / `reset`, both of which mean "no scaling
/// of my own" and so yield `1.0`. (`reset`'s real WebKit semantics — ignore the
/// ancestors' zoom rather than merely contributing 1.0 — are not modelled;
/// nothing in the wild depends on it and it would need a separate flag.)
///
/// Returns `None` when the value does not parse, in which case the caller must
/// leave the previous value alone — an invalid declaration is ignored, per
/// CSS Syntax, not treated as `1.0`.
fn parse_zoom(value: &str) -> Option<f32> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("normal") || v.eq_ignore_ascii_case("reset") {
        return Some(1.0);
    }
    let factor = if let Some(pct) = v.strip_suffix('%') {
        pct.trim().parse::<f32>().ok()? / 100.0
    } else {
        v.parse::<f32>().ok()?
    };
    // A negative or non-finite zoom is invalid; a zero one would collapse the
    // subtree to nothing, which no page means and which would divide by zero
    // when un-zooming. Both are rejected so the declaration is simply dropped.
    if !factor.is_finite() || factor <= 0.0 {
        return None;
    }
    Some(factor)
}

/// Scale one already-computed absolute length by `z`. Only `Px` is touched:
/// every other unit resolves later against a basis (`font_size`, the containing
/// block, the viewport) that is itself already zoomed, so scaling here too
/// would apply the factor twice.
fn zoom_length(len: &mut Length, z: f32) {
    if let Length::Px(v) = len {
        *v *= z;
    }
}

/// Same for a `<length> | auto` field — `auto` carries no length to scale.
fn zoom_length_or_auto(len: &mut LengthOrAuto, z: f32) {
    if let LengthOrAuto::Length(l) = len {
        zoom_length(l, z);
    }
}

/// CSS Viewport L1 §5 — fold the element's effective `zoom` into its computed
/// box-model lengths.
///
/// Runs after the main cascade pass, so it sees the winning declarations. Every
/// property scaled here is **non-inherited**, which is what makes a blanket
/// multiply correct: the value is either specified on this element (and so has
/// not been scaled by anyone) or is the initial `0`/`auto`/`none` (where
/// scaling is a no-op). Inherited length properties are deliberately absent —
/// they arrive already carrying the ancestors' zoom, so touching them would
/// double-apply it.
///
/// `font_size` is handled by the caller rather than here, because it is the one
/// value whose correct factor depends on whether the element specified it (see
/// the call site).
fn apply_zoom_to_lengths(style: &mut ComputedStyle, z: f32) {
    if (z - 1.0).abs() < f32::EPSILON {
        return;
    }
    for len in [
        &mut style.width,
        &mut style.height,
        &mut style.min_width,
        &mut style.max_width,
        &mut style.min_height,
        &mut style.max_height,
    ] {
        if let Some(l) = len.as_mut() {
            zoom_length(l, z);
        }
    }
    for len in [
        &mut style.margin_top,
        &mut style.margin_right,
        &mut style.margin_bottom,
        &mut style.margin_left,
        &mut style.top,
        &mut style.right,
        &mut style.bottom,
        &mut style.left,
    ] {
        zoom_length_or_auto(len, z);
    }
    for len in [
        &mut style.padding_top,
        &mut style.padding_right,
        &mut style.padding_bottom,
        &mut style.padding_left,
        &mut style.row_gap,
        &mut style.column_gap,
    ] {
        zoom_length(len, z);
    }
    // Border widths are already resolved to px by the cascade.
    style.border_top_width *= z;
    style.border_right_width *= z;
    style.border_bottom_width *= z;
    style.border_left_width *= z;
}

pub fn compute_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> ComputedStyle {
    // BUG-341 S10: permanent per-phase instrumentation. Same-named sibling
    // scopes are merged by `lumen_core::profile`, so a `LUMEN_PROFILE_TREE=1`
    // run prints one aggregated line per phase with a `×N` call count instead
    // of one line per node. Costs a cached bool check per phase when disabled.
    let _prof = lumen_core::profile::scope_detail("compute_style");
    note_compute_style();
    let prof_init = lumen_core::profile::scope_detail("cs_init");
    let mut style = ComputedStyle {
        display: default_display(doc, node),
        // Наследуемые свойства (CSS inherited properties).
        color: inherited.color,
        color_space: inherited.color_space,
        text_align: inherited.text_align,
        direction: inherited.direction,
        // `unicode-bidi` не наследуется (CSS Writing Modes L4 §2.2).
        unicode_bidi: UnicodeBidi::Normal,
        font_size: inherited.font_size,
        // Seeded from the parent so the value compounds; the element's own
        // `zoom` declaration is folded in by the pre-pass below.
        effective_zoom: inherited.effective_zoom,
        line_height: inherited.line_height,
        line_height_is_relative: inherited.line_height_is_relative,
        line_height_step: inherited.line_height_step,
        font_style: inherited.font_style,
        font_weight: inherited.font_weight,
        font_variant_caps: inherited.font_variant_caps,
        font_variant_emoji: inherited.font_variant_emoji,
        font_stretch: inherited.font_stretch,
        font_family: inherited.font_family.clone(),
        font_variation_settings: inherited.font_variation_settings.clone(),
        font_feature_settings: inherited.font_feature_settings.clone(),
        font_palette: inherited.font_palette.clone(),
        font_palette_resolved: inherited.font_palette_resolved.clone(),
        font_optical_sizing: inherited.font_optical_sizing,
        text_transform: inherited.text_transform,
        white_space: ua_white_space(doc, node).unwrap_or(inherited.white_space),
        white_space_collapse: ua_white_space(doc, node)
            .map(WhiteSpace::collapse_component)
            .unwrap_or(inherited.white_space_collapse),
        text_indent: inherited.text_indent.clone(),
        letter_spacing: inherited.letter_spacing,
        word_spacing: inherited.word_spacing,
        text_decoration_line: inherited.text_decoration_line,
        text_decoration_color: inherited.text_decoration_color,
        text_decoration_style: inherited.text_decoration_style,
        text_decoration_thickness: inherited.text_decoration_thickness,
        text_emphasis_style: inherited.text_emphasis_style.clone(),
        text_emphasis_color: inherited.text_emphasis_color,
        text_emphasis_position: inherited.text_emphasis_position,
        text_underline_position: inherited.text_underline_position,
        text_underline_offset: inherited.text_underline_offset,
        text_decoration_skip_ink: inherited.text_decoration_skip_ink,
        accent_color: inherited.accent_color,
        color_scheme: inherited.color_scheme,
        // CSS Color Adjustment L1 §4: forced-color-adjust IS inherited.
        forced_color_adjust: inherited.forced_color_adjust,
        // CSS Variables L1: все custom properties inherited.
        custom_props: inherited.custom_props.clone(),
        // Ненаследуемые — сброс.
        background_color: None,
        width: None,
        height: None,
        min_width: None,
        max_width: None,
        min_height: None,
        max_height: None,
        margin_top: LengthOrAuto::ZERO,
        margin_right: LengthOrAuto::ZERO,
        margin_bottom: LengthOrAuto::ZERO,
        margin_left: LengthOrAuto::ZERO,
        padding_top: Length::Px(0.0),
        padding_right: Length::Px(0.0),
        padding_bottom: Length::Px(0.0),
        padding_left: Length::Px(0.0),
        border_top_width: 0.0,
        border_right_width: 0.0,
        border_bottom_width: 0.0,
        border_left_width: 0.0,
        border_top_style: BorderStyle::None,
        border_right_style: BorderStyle::None,
        border_bottom_style: BorderStyle::None,
        border_left_style: BorderStyle::None,
        border_top_color: CssColor::CurrentColor,
        border_right_color: CssColor::CurrentColor,
        border_bottom_color: CssColor::CurrentColor,
        border_left_color: CssColor::CurrentColor,
        box_sizing: BoxSizing::ContentBox,
        // CSS Positioned Layout L3 §3 / Compositing L1 — не наследуются.
        position: Position::Static,
        top: LengthOrAuto::Auto,
        right: LengthOrAuto::Auto,
        bottom: LengthOrAuto::Auto,
        left: LengthOrAuto::Auto,
        z_index: None,
        float_side: FloatSide::None,
        clear: ClearSide::None,
        initial_letter_size: 1.0,
        initial_letter_sink: 0,
        isolation: Isolation::Auto,
        mix_blend_mode: MixBlendMode::Normal,
        // border-radius не наследуется.
        border_top_left_radius: Length::Px(0.0),
        border_top_right_radius: Length::Px(0.0),
        border_bottom_right_radius: Length::Px(0.0),
        border_bottom_left_radius: Length::Px(0.0),
        border_top_left_radius_y: Length::Px(0.0),
        border_top_right_radius_y: Length::Px(0.0),
        border_bottom_right_radius_y: Length::Px(0.0),
        border_bottom_left_radius_y: Length::Px(0.0),
        // Inherited (CSS Display L3 §4).
        visibility: inherited.visibility,
        // Inherited (CSS UI L4 §8.1).
        cursor: inherited.cursor,
        // text-shadow inherited (CSS Text Decoration L3 §4).
        text_shadow: inherited.text_shadow.clone(),
        // Не наследуется.
        box_shadow: Vec::new(),
        overflow_x: Overflow::Visible,
        overflow_y: Overflow::Visible,
        overflow_clip_margin: None,
        text_overflow: TextOverflow::Clip,
        opacity: 1.0,
        outline_width: 3.0,
        outline_style: OutlineStyle::None,
        outline_color: OutlineColor::Auto,
        outline_offset: Length::Px(0.0),
        // CSS Lists L3 §3 — не наследуются.
        counter_reset: Vec::new(),
        counter_increment: Vec::new(),
        counter_set: Vec::new(),
        // CSS Masking / Transforms / Filter — не наследуются.
        clip_path: None,
        transform: Vec::new(),
        translate: None,
        rotate: None,
        scale: None,
        filter: Vec::new(),
        // Box Alignment gap / Sizing aspect-ratio — не наследуются.
        row_gap: Length::Px(0.0),
        column_gap: Length::Px(0.0),
        // CSS Multi-column — не наследуются.
        column_count: None,
        column_width: None,
        column_rule_width: 0.0,
        column_rule_style: BorderStyle::None,
        column_rule_color: CssColor::CurrentColor,
        gap_rule_width: 0.0,
        gap_rule_style: BorderStyle::None,
        gap_rule_color: CssColor::CurrentColor,
        column_span_all: false,
        column_fill_balance: true,
        break_before: BreakValue::Auto,
        break_after: BreakValue::Auto,
        break_inside: BreakValue::Auto,
        aspect_ratio: None,
        // Box Alignment — все не наследуются, default = Auto.
        align_items: AlignValue::Auto,
        align_self: AlignValue::Auto,
        align_content: AlignValue::Auto,
        justify_items: AlignValue::Auto,
        justify_self: AlignValue::Auto,
        justify_content: AlignValue::Auto,
        // Backgrounds — не наследуются, defaults.
        background_layers: Vec::new(),
        // Will Change / Pointer Events — не наследуются.
        will_change: Vec::new(),
        pointer_events: PointerEvents::Auto,
        touch_action: TouchAction::Auto,
        appearance: Appearance::Auto,
        field_sizing: FieldSizing::Fixed,
        text_align_last: TextAlignLast::Auto,
        // User Select / Scroll Behavior — наследуются.
        user_select: inherited.user_select,
        resize: Resize::None,
        scroll_behavior: inherited.scroll_behavior,
        // Scroll Snap / Overscroll — не наследуются, defaults.
        scroll_snap_type: ScrollSnapType::default(),
        scroll_snap_align: ScrollSnapAlign::default(),
        scroll_snap_stop: ScrollSnapStop::default(),
        scroll_margin_top: 0.0,
        scroll_margin_right: 0.0,
        scroll_margin_bottom: 0.0,
        scroll_margin_left: 0.0,
        scroll_padding_top: 0.0,
        scroll_padding_right: 0.0,
        scroll_padding_bottom: 0.0,
        scroll_padding_left: 0.0,
        overscroll_behavior_x: OverscrollBehavior::Auto,
        overscroll_behavior_y: OverscrollBehavior::Auto,
        // CSS Table — border-collapse and border-spacing are inherited (CSS Tables L2 §17.6).
        border_collapse: inherited.border_collapse,
        empty_cells: inherited.empty_cells,
        border_spacing_h: inherited.border_spacing_h,
        border_spacing_v: inherited.border_spacing_v,
        // CSS Text typography — все inherited.
        tab_size: inherited.tab_size,
        caret_color: inherited.caret_color,
        overflow_wrap: inherited.overflow_wrap,
        word_break: inherited.word_break,
        line_break: inherited.line_break,
        hyphens: inherited.hyphens,
        // CSS Transforms — не наследуются.
        transform_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5), 0.0),
        perspective: None,
        perspective_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5)),
        transform_style: TransformStyle::Flat,
        backface_visibility: BackfaceVisibility::Visible,
        // CSS Lists — list-style-* наследуются.
        list_style_type: inherited.list_style_type.clone(),
        list_style_position: inherited.list_style_position,
        list_style_image: inherited.list_style_image.clone(),
        // CSS Transitions / Animations — не наследуются. Initial = empty list.
        transition_properties: Vec::new(),
        transition_durations: Vec::new(),
        transition_delays: Vec::new(),
        transition_timing_functions: Vec::new(),
        transition_fill_modes: Vec::new(),
        animation_names: Vec::new(),
        animation_durations: Vec::new(),
        animation_timing_functions: Vec::new(),
        animation_delays: Vec::new(),
        animation_iteration_counts: Vec::new(),
        animation_directions: Vec::new(),
        animation_fill_modes: Vec::new(),
        animation_play_states: Vec::new(),
        animation_timelines: Vec::new(),
        scroll_timeline_name: None,
        scroll_timeline_axis: ScrollAxis::Block,
        view_timeline_name: None,
        view_timeline_axis: ScrollAxis::Block,
        // CSS Masking — не наследуется.
        mask_layers: Vec::new(),
        // CSS Scrollbars — scrollbar-width/-color inherited;
        // scrollbar-gutter не наследуется.
        scrollbar_width: inherited.scrollbar_width,
        scrollbar_color: inherited.scrollbar_color,
        scrollbar_gutter: ScrollbarGutter::Auto,
        content: Content::Normal,
        // CSS Images L3 §5.5 — object-fit / object-position не наследуются.
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        // CSS 2.1 §10.8.1 — vertical-align не наследуется. Initial = baseline.
        vertical_align: VerticalAlign::Baseline,
        // CSS Images L3 §6.1 — image-rendering inherited.
        image_rendering: inherited.image_rendering,
        // CSS Flexbox L1 §5 — flex-direction / flex-wrap не наследуются.
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Nowrap,
        // CSS Flexbox L1 §7 — flex-grow / flex-shrink / flex-basis не наследуются.
        flex_grow: 0.0,
        flex_shrink: 1.0,
        flex_basis: FlexBasis::Auto,
        order: 0,
        // CSS Grid Layout L1 — grid properties не наследуются.
        grid_template_columns: Vec::new(),
        grid_template_rows: Vec::new(),
        grid_template_col_auto_repeat: None,
        grid_template_row_auto_repeat: None,
        grid_template_areas: Vec::new(),
        grid_auto_flow: GridAutoFlow::Row,
        masonry_auto_flow: MasonryAutoFlow::DefiniteFirst,
        grid_auto_columns: GridTrackSize::Auto,
        grid_auto_rows: GridTrackSize::Auto,
        grid_column_start: GridLine::Auto,
        grid_column_end: GridLine::Auto,
        grid_row_start: GridLine::Auto,
        grid_row_end: GridLine::Auto,
        // CSS Text Module Level 4 §6.4 — text-wrap-mode / text-wrap-style inherited.
        text_wrap_mode: inherited.text_wrap_mode,
        text_wrap_style: inherited.text_wrap_style,
        // CSS Overflow L4 — line-clamp не наследуется. Initial = none.
        line_clamp: None,
        // CSS Fragmentation L3 §3.3 — orphans / widows наследуются. Initial = 2.
        orphans: inherited.orphans,
        widows: inherited.widows,
        // CSS Containment L3 — не наследуются. Initial values.
        contain: ContainFlags::NONE,
        content_visibility: ContentVisibility::Visible,
        // CSS Box Sizing L4 §5 — contain-intrinsic-* are NOT inherited.
        contain_intrinsic_width: None,
        contain_intrinsic_width_auto: false,
        contain_intrinsic_height: None,
        contain_intrinsic_height_auto: false,
        // CSS Sizing L4 §4.5 — interpolate-size is inherited.
        interpolate_size: inherited.interpolate_size,
        container_type: ContainerType::Normal,
        container_name: Vec::new(),
        // CSS Filter Effects L2 — backdrop-filter не наследуется.
        backdrop_filter: Vec::new(),
        // CSS Color Adjustment L1 §5 — print-color-adjust не наследуется.
        print_color_adjust: PrintColorAdjust::Economy,
        // CSS Fonts L5 §4 — font-size-adjust inherited.
        font_size_adjust: inherited.font_size_adjust,
        // CSS Writing Modes L3 — оба inherited.
        writing_mode: inherited.writing_mode,
        text_orientation: inherited.text_orientation,
        // CSS Ruby L1 §4 — все три inherited.
        ruby_position: inherited.ruby_position,
        ruby_align: inherited.ruby_align,
        ruby_merge: inherited.ruby_merge,
        // MathML Core §2.1 — оба inherited (math-depth уже как computed integer).
        math_style: inherited.math_style,
        math_depth: inherited.math_depth,
        // CSS Shapes L1 / Motion Path — не наследуются. Initial values.
        shape_outside: ShapeOutside::None,
        shape_margin: Length::Px(0.0),
        shape_image_threshold: 0.0,
        offset_path: None,
        offset_distance: Length::Px(0.0),
        offset_rotate: OffsetRotate::Auto,
        offset_anchor: None,
        // SVG presentation attributes — all inherited per SVG spec §11.
        svg_fill: inherited.svg_fill,
        svg_fill_opacity: inherited.svg_fill_opacity,
        svg_stroke: inherited.svg_stroke,
        svg_stroke_opacity: inherited.svg_stroke_opacity,
        svg_stroke_width: inherited.svg_stroke_width,
        svg_fill_rule: inherited.svg_fill_rule,
        svg_clip_rule: inherited.svg_clip_rule,
        svg_stroke_linecap: inherited.svg_stroke_linecap,
        svg_stroke_linejoin: inherited.svg_stroke_linejoin,
        svg_stroke_miterlimit: inherited.svg_stroke_miterlimit,
        svg_stroke_dasharray: inherited.svg_stroke_dasharray.clone(),
        svg_stroke_dashoffset: inherited.svg_stroke_dashoffset,
        paint_order: inherited.paint_order,
        text_anchor: inherited.text_anchor,
        dominant_baseline: inherited.dominant_baseline,
        // SVG baseline-shift is NOT inherited — reset to initial each element.
        baseline_shift: crate::box_tree::SvgBaselineShift::Baseline,
        // CSS Logical Properties L1 — not inherited. Initial values.
        inline_size: None,
        block_size: None,
        inset_inline_start: LengthOrAuto::Auto,
        inset_inline_end: LengthOrAuto::Auto,
        inset_block_start: LengthOrAuto::Auto,
        inset_block_end: LengthOrAuto::Auto,
        margin_inline_start: LengthOrAuto::ZERO,
        margin_inline_end: LengthOrAuto::ZERO,
        margin_block_start: LengthOrAuto::ZERO,
        margin_block_end: LengthOrAuto::ZERO,
        padding_inline_start: Length::Px(0.0),
        padding_inline_end: Length::Px(0.0),
        padding_block_start: Length::Px(0.0),
        padding_block_end: Length::Px(0.0),
        border_inline_start_width: 0.0,
        border_inline_end_width: 0.0,
        border_block_start_width: 0.0,
        border_block_end_width: 0.0,
        anchor_name: None,
        position_anchor: None,
        inset_area_row: crate::anchor::InsetAreaKeyword::None,
        inset_area_col: crate::anchor::InsetAreaKeyword::None,
        anchor_scope: crate::anchor::AnchorScope::None,
        anchor_size_w: None,
        anchor_size_h: None,
        anchor_top: None,
        anchor_right: None,
        anchor_bottom: None,
        anchor_left: None,
        view_transition_name: None,
        // CSS Generated Content L3 §3.2 — quotes inherited.
        quotes: inherited.quotes.clone(),
    };

    // CSS Properties and Values L1 §1.1 — registry зарегистрированных
    // custom-properties. Карта строится локально для каждого узла:
    // на типичной странице 0..5 @property-правил, накладные расходы мизерны
    // в сравнении со стоимостью каскада. При повторе имени (см. spec —
    // last wins) `insert` корректно сохраняет последнее объявление.
    let registry: HashMap<&str, &PropertyRule> = sheet
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();

    // Откатываем у себя унаследованные значения тех зарегистрированных
    // custom-properties, у которых `inherits: false` — для них потомок
    // должен видеть либо локальную декларацию, либо initial-value, а не
    // родительское значение.
    //
    // BUG-341 S9: `retain` needs a `make_mut`, which copies the inherited map —
    // so first check whether any key would actually be dropped. Pages that
    // register no `inherits: false` property (or declare none of the ones they
    // do register) keep sharing the parent's allocation.
    if !registry.is_empty()
        && style
            .custom_props
            .keys()
            .any(|key| registry.get(key.as_str()).is_some_and(|p| !p.inherits))
    {
        style.custom_props.make_mut().retain(|key, _| {
            registry.get(key.as_str()).is_none_or(|p| p.inherits)
        });
    }

    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        // Для не-элементов (Document, Text внутри anonymous-wrapping) тоже
        // применяем initial-value: var(--registered) в наследуемом стиле
        // должен резолвиться через initial-value, если декларации нет.
        apply_property_initial_values(&mut style.custom_props, &registry);
        return style;
    }
    drop(prof_init);
    let prof_ua = lumen_core::profile::scope_detail("cs_ua_hints");

    // UA stylesheet: семантические элементы получают italic / bold по
    // умолчанию, CSS-декларации ниже могут это переопределить.
    if let Some(fs) = ua_font_style(doc, node) {
        style.font_style = fs;
    }
    if let Some(fw) = ua_font_weight(doc, node) {
        style.font_weight = fw;
    }
    // UA stylesheet: <pre>/<code>/<kbd>/<samp>/<tt> → font-family: monospace.
    if let Some(fam) = ua_font_family(doc, node) {
        style.font_family = fam;
    }
    // UA stylesheet: text-decoration для <del>/<s> (line-through),
    // <ins>/<u>/<a href> (underline). HTML5 §15.3.7.
    apply_ua_text_decoration(doc, node, &mut style);
    // UA stylesheet: <a href> → color: #0000ee. HTML5 §15.3.3.
    if let Some(c) = ua_link_color(doc, node) {
        style.color = c;
    }
    // UA stylesheet: <small>/<sub>/<sup> → font-size: 0.83× parent.
    // HTML5 §15.3.3. Author font-size перекроет через pre-pass.
    if let Some(factor) = ua_font_size_factor(doc, node) {
        style.font_size = inherited.font_size * factor;
    }
    // UA stylesheet: <sub>/<sup> → vertical-align. HTML5 §15.3.3.
    if let Some(va) = ua_vertical_align(doc, node) {
        style.vertical_align = va;
    }
    // UA stylesheet: <h1>–<h6> → font-size + vertical margins. HTML Rendering §15.3.3.
    // Set font-size here (before the author font-size pre-pass) so author CSS overrides it.
    apply_ua_heading_style(doc, node, inherited, &mut style);
    apply_ua_hr_style(doc, node, &mut style);
    // UA stylesheet: <body> → margin: 8px. HTML Rendering §14.3.3. Author CSS перекроет.
    apply_ua_body_margin(doc, node, &mut style);
    // UA stylesheet: form controls — display, intrinsic dimensions, border,
    // background, and foreground color. HTML5 §15.5. Author CSS поверх перекроет.
    //
    // CSS Color Adjustment L1 §2.3: тема UA-виджета определяется «used color
    // scheme» элемента, а не сырым предпочтением ОС. `color-scheme` наследуется,
    // поэтому на этапе UA-фазы (до author-каскада) берём inherited-значение —
    // оно покрывает типовой паттерн `:root { color-scheme: dark }`, спускающийся
    // к контролам. Так `color-scheme: light` форсирует светлый виджет даже в
    // OS-dark, а `dark` — тёмный в OS-light.
    //
    // CSS: system-color — P4 wires `system_color()` into the color cascade
    // (a `CssColor::System(name)` variant resolved at used-value time against
    // the element's used color scheme) for `Canvas`/`CanvasText`/`ButtonFace`/…
    // keyword support. The resolution table already lives in `system_color()`.
    let widget_dark = inherited.color_scheme.used_dark(dark_mode);
    apply_ua_form_controls(doc, node, &mut style, widget_dark);
    // UA stylesheet: <dialog> without `open` → display:none. HTML5 §15.3.9.
    apply_ua_dialog_display(doc, node, &mut style);
    // UA stylesheet: <td>/<th> → padding: 1px (HTML Rendering §15.3.8); the
    // ancestor <table cellpadding=N> overrides it. Author `padding` wins.
    apply_ua_table_cell_padding(doc, node, &mut style);
    // UA stylesheet (HTML Rendering §15.4.2): `[inert] { pointer-events: none; }`.
    // Applied during the pre-cascade UA phase so author `pointer-events` wins.
    apply_ua_inert(doc, node, &mut style);

    // CSS Quirks Mode — Quirks-only UA-rule для `<table>`: сбрасывает
    // font / color / text-align / white-space к initial-values, чтобы
    // legacy table-layout страницы (где CSS на `<body>` задавал шрифт /
    // цвет) рендерились с дефолтным шрифтом таблицы, как в IE/Netscape.
    // В Standards / LimitedQuirks не применяется.
    apply_quirks_table_reset(doc, node, &mut style);
    // CSS Quirks Mode §3.2: replaced-элементы получают line-height: 1 как UA-правило.
    apply_quirks_line_height(doc, node, &mut style);
    // CSS Quirks Mode §3.5: <html> получает height: 100vh как UA-правило,
    // чтобы body { height: 100% } резолвилось против viewport.
    apply_quirks_html_height(doc, node, &mut style);

    // HTML presentational hints (HTML5 §10): для `<img>` атрибуты
    // `width`/`height` задают начальные значения соответствующих CSS-свойств.
    // Применяются ДО CSS-каскада, поэтому любое author-CSS правило
    // перекроет атрибут даже с specificity (0,0,1). Парсятся как unitless
    // целые пиксели — это HTML5 правило для `<img>`, единицы и проценты
    // в этих атрибутах игнорируются.
    apply_image_presentational_hints(doc, node, &mut style);

    // HTML5 §15 «Rendering»: `bgcolor` на `<body>` / `<table>` / `<thead>` /
    // `<tbody>` / `<tfoot>` / `<tr>` / `<td>` / `<th>` мапается на
    // `background-color` (presentational hint). Парсится по HTML5 §2.4.6
    // «rules for parsing a legacy color value» — более лояльный алгоритм,
    // чем CSS quirks hashless hex: принимает named colors, `#rgb` / `#rrggbb`,
    // hashless hex произвольной длины и любую строку, в которой можно
    // найти хотя бы какие-то hex-digits после padding-procedure.
    apply_bgcolor_presentational_hint(doc, node, &mut style);

    // HTML LS §15.3.8 «Tables»: `background`/`bordercolor`/`cellspacing`
    // presentational hints (BUG-603 point 2) — siblings of `bgcolor` above,
    // narrower in scope (table-tree elements only, `cellspacing` table-only).
    apply_background_image_presentational_hint(doc, node, &mut style);
    apply_bordercolor_presentational_hint(doc, node, &mut style);
    apply_cellspacing_presentational_hint(doc, node, &mut style);

    // HTML5 §15.3.6 «The page»: `text` атрибут на `<body>` и `<font color>`
    // на любом элементе мапаются на CSS `color` (presentational hint).
    // Парсятся тем же legacy-парсером, что и `bgcolor`. Author CSS поверх —
    // выигрывает. `<body link/vlink/alink>` отложены: `:link` единственный
    // матчится в Phase 0, `:visited`/`:active` без runtime — no-op.
    apply_text_color_presentational_hint(doc, node, &mut style);

    // HTML5 §15.3.2: `<font size>` → font-size; `<font face>` → font-family.
    apply_font_element_presentational_hints(doc, node, &mut style);

    // HTML5 §15.3.3: `align` на блочных элементах → text-align.
    apply_align_presentational_hint(doc, node, &mut style);

    // CSS Quirks Mode §4.1 + HTML5 §14.3.9: `width`/`height` attr на
    // `<td>`/`<th>`/`<table>`. В quirks-mode width ячейки → min-width.
    apply_table_cell_width_hint(doc, node, &mut style);

    // CSS Cascade L4 §6.4.3 — inline style: парсим HTML-атрибут `style=""`
    // и кладём его декларации в отдельный буфер. Они подключаются к каскаду
    // через дополнительный sort-bit `is_inline` (ниже): внутри одного origin
    // (нормального или !important) inline всегда побеждает любой селектор —
    // это «Element-Attached Styles» тир в Cascade L4 §8.1, идущий после
    // Layer/Specificity/Order, но до Importance-инверсии.
    drop(prof_ua);
    let prof_match = lumen_core::profile::scope_detail("cs_match");
    let inline_decls: Vec<Declaration> = doc
        .get(node)
        .get_attr("style")
        .filter(|s| !s.is_empty())
        .map(parse_inline_style)
        .unwrap_or_default();

    // Собираем все matched declarations с их sort key:
    // (important, is_inline, layer_priority, specificity, rule_order, decl_index).
    //
    // `important` идёт первым: !important побеждает normal (CSS Cascade L4 §8.1).
    // `is_inline` — вторым: inline-style атрибут побеждает стилевой лист
    // (CSS Cascade L4 §6.4.3).
    // `layer_priority` — CSS Cascade L5 §6.4.5 @layer ordering:
    //   - normal: unlayered = N (highest), layer[i] = i (earlier layer = lower priority)
    //   - !important: unlayered = -N (lowest), layer[i] = -i (earlier layer = highest)
    //   Ascending sort, last applied wins → correct per spec.
    // `specificity`, `rule_idx`, `decl_idx` — обычный каскад внутри одного layer.
    let layer_n = sheet.layer_order.len() as i32;
    // Compute layer priority sign correctly for normal vs !important declarations.
    // For normal (imp=false): higher = wins → unlayered = N > layer[N-1] > ... > layer[0]
    // For !important (imp=true): lower layer_idx wins → layer[0] = 0 > layer[1] = -1 > ... > unlayered = -N
    let layer_pri = |imp: bool, layer_idx: i32| -> i32 {
        if imp { -layer_idx } else { layer_idx }
    };
    let mut matched: Vec<(bool, bool, i32, Specificity, usize, usize, &Declaration)> = Vec::new();


    // Build or reuse a per-stylesheet rule index (thread-local, keyed by
    // pointer+length). Amortised O(1): rebuilt only when the sheet changes.
    let node_data = doc.get(node);
    let node_tag = node_data.element_name().map_or("", |q| q.local.as_str());
    let node_id = node_data.get_attr("id");
    let class_attr = node_data.get_attr("class").unwrap_or("");
    let node_classes: Vec<&str> = class_attr.split_whitespace().collect();

    ensure_cascade_index(sheet, viewport, dark_mode);
    let cands = with_front_cascade_index(|idx| {
        idx.rules.candidates(node_tag, node_id, &node_classes)
    });

    for &rule_idx in &cands {
        let rule = &sheet.rules[rule_idx];
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if matches_complex(complex, doc, node) {
                let spec = complex.specificity();
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                let lp = layer_pri(decl.important, layer_n);
                matched.push((decl.important, false, lp, spec, rule_idx, decl_idx, decl));
            }
        }
    }

    // CSS Cascade L5 §6.4.5 — @layer rules: каждый LayerRule добавляет
    // свои декларации в каскад с layer_priority < unlayered. Layer с меньшим
    // индексом в `layer_order` имеет меньший приоритет для normal (earlier
    // declared → overridden by later), и больший для !important (CSS Cascade
    // L5 §6.4.5 inversion: earlier layer !important wins).
    let layer_rule_base = sheet.rules.len()
        + sheet.media_rules.iter().map(|m| m.rules.len()).sum::<usize>();
    let mut layer_rule_offset = 0usize;
    for (layer_i, layer_rule) in sheet.layers.iter().enumerate() {
        let layer_idx = sheet.layer_order.iter()
            .position(|n| n == &layer_rule.name)
            .unwrap_or(0) as i32;
        // BUG-284: candidate pre-filter (was a brute-force scan of every rule
        // in the layer for every node — dominant cascade cost on stylesheets
        // that put most rules inside layers/media/supports blocks).
        let layer_cands = with_front_cascade_index(|idx| {
            idx.layers[layer_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in layer_cands {
            let rule = &layer_rule.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = layer_rule_base + layer_rule_offset + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_idx);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        layer_rule_offset += layer_rule.rules.len();
    }

    // CSS Media Queries L4: rules внутри `@media`-блока, чей query
    // совпадает с текущим MediaContext, добавляются в каскад. В Phase 0
    // упрощённый MediaContext: media_type="screen", width/height из
    // viewport. Source-order между обычными и
    // @media-rules не сохраняется идеально (все @media идут после
    // обычных) — это известное ограничение.
    //
    // Perf: "active" per block precomputed once per (sheet, viewport,
    // dark_mode) in `CascadeIndex::active_media` — see its doc comment.
    // `media.query.matches(..)` used to run here on every node. Fetched once
    // per node (not once per block) to avoid N thread-local accesses when
    // the stylesheet has many `@media` blocks.
    let active_media = with_front_cascade_index(|idx| idx.active_media.clone());
    let mut next_rule_idx = sheet.rules.len();
    for (media_i, media) in sheet.media_rules.iter().enumerate() {
        if !active_media[media_i] {
            next_rule_idx += media.rules.len();
            continue;
        }
        // BUG-284: candidate pre-filter (see @layer above) — real-world
        // stylesheets often put the bulk of their rules inside @media blocks.
        let media_cands = with_front_cascade_index(|idx| {
            idx.media[media_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in media_cands {
            let rule = &media.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += media.rules.len();
    }
    // CSS Conditional Rules L3 §2 — `@supports`: evaluate condition against
    // Lumen's supported-properties list; include contained rules only when
    // condition is true (same ordering semantics as @media).
    //
    // Perf: "active" precomputed once per sheet in `CascadeIndex::active_supports`
    // (see doc comment) — `supports.condition.evaluate(..)` used to run per node.
    let active_supports = with_front_cascade_index(|idx| idx.active_supports.clone());
    for (supports_i, supports) in sheet.supports_rules.iter().enumerate() {
        if !active_supports[supports_i] {
            next_rule_idx += supports.rules.len();
            continue;
        }
        let supports_cands = with_front_cascade_index(|idx| {
            idx.supports[supports_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in supports_cands {
            let rule = &supports.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += supports.rules.len();
    }
    // CSS Cascade L6 §5 — @scope rules: apply only when node is in scope.
    for scope_rule in &sheet.scope_rules {
        // Donut scoping (§3): `node` is in scope when it is an inclusive
        // descendant of the scope root but *not* of a scope limit that lies
        // within that same root subtree. `node_in_scope` resolves root and
        // limit together (nearest boundary wins) so a limit-matching element
        // *above* the root no longer removes the node from scope.
        if !node_in_scope(doc, node, &scope_rule.root, scope_rule.limit.as_deref()) {
            next_rule_idx += scope_rule.rules.len();
            continue;
        }
        for rule in &scope_rule.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, next_rule_idx, decl_idx, decl));
                }
            }
            next_rule_idx += 1;
        }
    }
    // CSS Scoping L1 §6.1-6.2 — shadow-tree-scoped style rules. `:host`/`:host()`
    // and `::slotted()` only have effect when written *inside* a shadow tree's own
    // stylesheet (collected per-host in `SHADOW_SHEETS`); the same selectors in the
    // page's document `<style>` are no-ops. Two scopes touch this node:
    //   (a) the node is itself a shadow host → its OWN shadow sheet's `:host` rules
    //       cascade onto it (the host lives in the light tree, so only `:host`-bearing
    //       rules from its shadow reach it);
    //   (b) the node is a slotted light child → its host's shadow sheet's `::slotted()`
    //       rules cascade onto it.
    // These declarations join the same cascade as document author rules; we give them
    // `rule_idx` values past the document range so source order stays stable (shadow
    // markup follows the head `<style>` in document order).
    // Clone the relevant shadow sheets out of the thread-local into locals that
    // live for the rest of this function, so the `&Declaration` references pushed
    // into `matched` outlive the (closure-scoped) thread-local borrow.
    let any_shadow = SHADOW_SHEETS.with(|c| !c.borrow().is_empty());
    let own_shadow: Option<Stylesheet> = if any_shadow && doc.is_shadow_host(node) {
        SHADOW_SHEETS.with(|c| c.borrow().get(&node).cloned())
    } else {
        None
    };
    let host_shadow: Option<Stylesheet> = if any_shadow {
        doc.get(node)
            .parent
            .filter(|&p| doc.is_shadow_host(p))
            .and_then(|host| SHADOW_SHEETS.with(|c| c.borrow().get(&host).cloned()))
    } else {
        None
    };
    // (a) `:host` / `:host(sel)` from the node's own shadow tree apply to the host.
    if let Some(ref shadow) = own_shadow {
        SHADOW_HOST_SCOPE.with(|c| c.set(node.index() as u32));
        for (i, rule) in shadow.rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if complex_has_host(complex) && matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let gidx = next_rule_idx + i;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, gidx, decl_idx, decl));
                }
            }
        }
        SHADOW_HOST_SCOPE.with(|c| c.set(u32::MAX));
    }
    // (b) `::slotted(sel)` from this node's host's shadow tree apply to the slotted child.
    if let Some(ref shadow) = host_shadow {
        let base = next_rule_idx + shadow.rules.len();
        for (i, rule) in shadow.rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_slotted_complex(complex, doc, node) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let gidx = base + i;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, gidx, decl_idx, decl));
                }
            }
        }
    }

    // Inline-style declarations подключаются с `is_inline = true` и
    // synthetic specificity = default (Cascade L4 §6.4.3 — реальная
    // specificity inline-стиля игнорируется в сортировке: за порядок
    // отвечает is_inline-бит, а внутри inline — источниковый порядок
    // декларации в атрибуте). Inline-стиль всегда unlayered.
    for (decl_idx, decl) in inline_decls.iter().enumerate() {
        matched.push((
            decl.important,
            true,
            layer_pri(decl.important, layer_n),
            Specificity::default(),
            next_rule_idx,
            decl_idx,
            decl,
        ));
    }
    matched.sort_by_key(|&(imp, inline, lp, spec, rule_idx, decl_idx, _)| {
        (imp, inline, lp, spec, rule_idx, decl_idx)
    });
    drop(prof_match);
    let prof_revert = lumen_core::profile::scope_detail("cs_revert_prepass");

    // CSS Cascade L5 §6.4.6 — `revert-layer`: a declaration whose value is
    // `revert-layer` rolls the cascaded value back to what it would be if all
    // declarations of that property in the *current* cascade layer (same
    // importance) were removed. We resolve it as a pre-pass over the already
    // cascade-sorted `matched` set: for every property whose winning
    // declaration (the last occurrence in sort order) is `revert-layer`, drop
    // every declaration of that property belonging to the winning layer, then
    // repeat — a lower layer may itself contain `revert-layer`. The normal
    // last-wins apply loop below then yields the reverted value automatically;
    // when nothing remains the property keeps its inherited/initial value.
    //
    // `revert-layer` is intentionally NOT a `CssWideKeyword`: it depends on the
    // declaration's own layer, so it cannot be applied per-declaration like
    // `inherit`/`initial`. Shorthand↔longhand reverts across layers are a known
    // limitation (grouping is by exact property name).
    //
    // BUG-341 S10: the loop below allocates a lowercased `String` key per
    // matched declaration plus a `HashMap` just to discover, on essentially
    // every element of every real page, that nothing declares `revert-layer`.
    // One allocation-free scan first (measured: 1.4 ms per chrome layout pass,
    // ~7% of the cascade stage).
    while matched
        .iter()
        .any(|&(_, _, _, _, _, _, decl)| decl.value.trim().eq_ignore_ascii_case("revert-layer"))
    {
        use std::collections::HashMap;
        // Winner per property = last occurrence in the cascade-sorted vec.
        // (lp, important, is_revert_layer)
        let mut winners: HashMap<String, (i32, bool, bool)> = HashMap::new();
        for &(imp, _inline, lp, _, _, _, decl) in &matched {
            let key = decl.property.to_ascii_lowercase();
            let is_revert = decl.value.trim().eq_ignore_ascii_case("revert-layer");
            winners.insert(key, (lp, imp, is_revert));
        }
        let targets: Vec<(String, i32, bool)> = winners
            .into_iter()
            .filter(|&(_, (_, _, is_revert))| is_revert)
            .map(|(k, (lp, imp, _))| (k, lp, imp))
            .collect();
        if targets.is_empty() {
            break;
        }
        matched.retain(|&(imp, _inline, lp, _, _, _, decl)| {
            let key = decl.property.to_ascii_lowercase();
            !targets
                .iter()
                .any(|(tk, tlp, timp)| *tk == key && *tlp == lp && *timp == imp)
        });
    }

    // CSS Cascade L4 §7.4 — `revert` откатывается к значению «как если бы
    // author/user-правил не было». `style` прямо здесь уже содержит ровно
    // это: наследуемые поля скопированы из `inherited`, а все `ua_*`/
    // `apply_ua_*`/presentational-hint пассы выше (§ «UA stylesheet» /
    // «HTML presentational hints») отработали, но ни одна matched-декларация
    // ещё не применена. Снэпшот уходит в `apply_declaration` → `apply_css_wide_keyword`.
    //
    // Perf (docs/tasks/p3-cascade-perf.md Задача 1): безусловный
    // `ComputedStyle::clone()` здесь был вторым по весу вкладом в build_box
    // на тяжёлых страницах — на каждый узел клонируются десятки Vec/String/
    // HashMap-полей ради свойства, которое почти никогда не встречается в
    // реальном CSS. Клонируем, только если среди matched-деклараций реально
    // есть `revert` — прямой (`prop: revert`) или через цепочку custom
    // properties (`--x: revert; prop: var(--x);`, в т.ч. унаследованную от
    // предка — все такие декларации остаются raw-строками в
    // `custom_props`/`inherited.custom_props`, поэтому проверка ловит любую
    // глубину вложенности). Когда клон не нужен, `ua_baseline_ref` указывает
    // на `inherited` как безопасную заглушку: `apply_declaration` читает этот
    // параметр только внутри ветки `kw == Revert`, которая в этом случае
    // гарантированно не сработает ни для одной декларации.
    let ua_baseline_font_size = style.font_size;
    let needs_ua_baseline = matched.iter().any(|&(_, _, _, _, _, _, decl)| {
        decl.value.trim().eq_ignore_ascii_case("revert")
    }) || (
        matched.iter().any(|&(_, _, _, _, _, _, decl)| decl.value.contains("var("))
            && inherited.custom_props.values().any(|v| v.trim().eq_ignore_ascii_case("revert"))
    );
    let ua_baseline_storage: Option<ComputedStyle> = needs_ua_baseline.then(|| style.clone());
    let ua_baseline_ref: &ComputedStyle = ua_baseline_storage.as_ref().unwrap_or(inherited);
    drop(prof_revert);
    let prof_apply = lumen_core::profile::scope_detail("cs_apply");

    // Custom-properties pass: все `--name: value` декларации применяются
    // отдельно и ДО остальных пассов, чтобы любая обычная декларация могла
    // видеть финальное значение custom property независимо от порядка
    // объявления в source. Каскад уже соблюдён через sort `matched`:
    // последующая запись с тем же ключом перебивает раннюю.
    //
    // BUG-731: пасс стоит ПЕРЕД font-size-pre-pass, а не после него. Иначе
    // `font-size: var(--x)` / `font: var(--x)` видели бы только унаследованную
    // карту, а собственное объявление элемента (`.card { --fs: 20px;
    // font-size: var(--fs) }`) — нет. Пасс ни от чего в pre-pass-ах не зависит:
    // он читает только `matched` + `registry`, а `validate_against_syntax`
    // работает по тексту значения, не по computed font-size.
    //
    // CSS Properties and Values L1 §1.1 «invalid at computed value time»:
    // для зарегистрированных custom properties value валидируется против
    // `syntax`-дескриптора. Невалидное значение игнорируется — старое
    // значение (родительское inherited или initial-value) остаётся.
    // value, содержащее `var(`, пропускается без валидации — резолв
    // происходит позже, и итоговая строка может быть валидной.
    for (_, _, _, _, _, _, decl) in &matched {
        if let Some(name) = decl.property.strip_prefix("--") {
            let key = format!("--{name}");
            if let Some(prop_rule) = registry.get(key.as_str())
                && !decl.value.contains("var(")
                && !validate_against_syntax(&decl.value, &prop_rule.syntax)
            {
                // Invalid at computed value time — skip declaration.
                continue;
            }
            style.custom_props.make_mut().insert(key, decl.value.clone());
        }
    }

    // CSS Properties and Values L1 §1.1: для каждого зарегистрированного
    // имени, у которого после custom-pass нет значения (ни унаследованного,
    // ни локально объявленного), подставить `initial-value`. Делается до
    // остальных пассов, чтобы `var(--registered)` в обычных декларациях
    // видел initial-value-fallback.
    apply_property_initial_values(&mut style.custom_props, &registry);

    // Pre-pass: применяем font-size раньше, потому что em/% других свойств
    // считаются относительно computed font-size этого же элемента, а em для
    // самого font-size — относительно inherited (родительского) font-size.
    // Pre-pass: `zoom` (CSS Viewport L1 §5) must be known before font-size and
    // before any other length is resolved, because it multiplies all of them.
    // `matched` is cascade-sorted, so the last parseable declaration wins.
    let mut own_zoom = 1.0f32;
    for (_, _, _, _, _, _, decl) in &matched {
        if decl.property.eq_ignore_ascii_case("zoom")
            && let Some(z) = parse_zoom(&decl.value)
        {
            own_zoom = z;
        }
    }
    style.effective_zoom = inherited.effective_zoom * own_zoom;

    let parent_fs = inherited.font_size;
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    // Which basis the winning font-size resolved against decides the zoom factor
    // below. No declaration applies → the value is the inherited (or UA-hinted
    // `em`) one, i.e. parent-relative.
    let mut fs_basis = FontSizeBasis::ParentRelative;
    for (_, _, _, _, _, _, decl) in &matched {
        if let Some(basis) =
            apply_font_size(&mut style, decl, parent_fs, ua_baseline_font_size, viewport, is_quirks)
        {
            fs_basis = basis;
        }
    }

    // A font-size resolved from a zoom-independent basis (`16px`, `rem`, …) has
    // not been scaled by anyone, so it takes the full compounded factor. One
    // resolved against the parent's size (`em`, `%`, or plain inheritance)
    // already carries every ancestor's zoom and needs only this element's own
    // contribution — applying `effective_zoom` to it would re-apply the
    // ancestors', once per level of nesting.
    style.font_size *= match fs_basis {
        FontSizeBasis::Absolute => style.effective_zoom,
        FontSizeBasis::ParentRelative => own_zoom,
    };

    // Pre-pass: применяем color-scheme раньше main-pass, чтобы системные
    // цвета (Canvas, ButtonFace, …) резолвились против правильной темы
    // ещё в ходе main-pass (для поля `color: Color`; CssColor-поля
    // резолвятся отдельным post-pass в конце compute_style).
    for (_, _, _, _, _, _, decl) in &matched {
        if decl.property.eq_ignore_ascii_case("color-scheme") {
            apply_declaration(&mut style, decl, parent_fs, viewport, FontWeight::NORMAL, inherited, ua_baseline_ref, is_quirks, dark_mode);
        }
    }

    // Main-pass: остальные декларации; em-basis теперь = current font_size.
    // Inherited font_weight нужен для разрешения `lighter`/`bolder`;
    // `inherited` целиком — для CSS-wide keywords (CSS Cascade L4 §7).
    let em_basis = style.font_size;
    let parent_weight = inherited.font_weight;

    // SVG 2 §6.4: presentation attributes act as author rules of the lowest
    // priority. Apply them before the matched-declaration loop so any CSS rule
    // (stylesheet or inline) overrides them.
    apply_svg_presentational_hints(
        doc, node, &mut style, em_basis, viewport, parent_weight, inherited, is_quirks,
    );

    // CSS Basic UI L4 §5 — pre-scan the cascade-winning `appearance` value
    // (matched is cascade-sorted; later = higher priority, inline included) so
    // that `appearance: none` strips UA-default border/background/padding
    // *before* the author cascade. Stripping after the cascade clobbered
    // author-specified border/background/padding (BUG-211).
    let mut appearance_none = false;
    for (_, _, _, _, _, _, decl) in &matched {
        match decl.property.as_str() {
            "appearance" | "-webkit-appearance" | "-moz-appearance" => {
                appearance_none = decl.value.trim().eq_ignore_ascii_case("none");
            }
            _ => {}
        }
    }
    if appearance_none {
        strip_ua_appearance_box_styling(doc, node, &mut style);
    }

    for (_, _, _, _, _, _, decl) in &matched {
        // CSS Cascade L5 §6.4.6: a `revert-layer` declaration that survived the
        // pre-pass was overridden by a higher layer for the same property, so it
        // has no effect — skip it instead of letting it fail property parsing.
        if decl.value.trim().eq_ignore_ascii_case("revert-layer") {
            continue;
        }
        // CSS Values L4 §7.7: expand attr() typed references before applying.
        let attr_buf;
        let effective_decl: &Declaration = if decl.value.contains("attr(") {
            let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
            attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
            &attr_buf
        } else {
            decl
        };
        // CSS Functions and Mixins L1: expand `--name(<args>)` custom function
        // calls before applying. `var(` is resolved first (against the same
        // `style.custom_props` `apply_declaration` would use) so a call reached
        // indirectly through a custom property (`--gap: --double(5px); width:
        // var(--gap);`) is visible to the call-site scanner, not just direct
        // calls (`width: --double(5px);`). Gated on `function_rules` being
        // non-empty — pages without `@function` pay nothing extra here, and
        // `apply_declaration`'s own `var()` pass below is then a no-op.
        let func_buf;
        let effective_decl: &Declaration = if !sheet.function_rules.is_empty()
            && effective_decl.value.contains("--")
        {
            let pre = if effective_decl.value.contains("var(") {
                match expand_vars(&effective_decl.value, &style.custom_props, 0) {
                    Some(v) => v,
                    None => continue,
                }
            } else {
                effective_decl.value.clone()
            };
            match expand_custom_functions(&pre, &sheet.function_rules, &style.custom_props, 0) {
                Some(v) => {
                    func_buf = Declaration {
                        property: effective_decl.property.clone(),
                        value: v,
                        important: effective_decl.important,
                    };
                    &func_buf
                }
                None => continue,
            }
        } else {
            effective_decl
        };
        apply_declaration(&mut style, effective_decl, em_basis, viewport, parent_weight, inherited, ua_baseline_ref, is_quirks, dark_mode);
    }

    // CSS Color 4 §6.2 — post-pass: resolve any CssColor::System variants in
    // CssColor-typed fields (border-color, background-color, etc.) now that
    // style.color_scheme is final. The `color` field (Color, not CssColor) was
    // already resolved inline in the `"color"` branch of apply_declaration.
    drop(prof_apply);
    let _prof_post = lumen_core::profile::scope_detail("cs_post");
    resolve_system_colors_in_style(&mut style, dark_mode);

    // CSS Color Adjustment L1 §3 — Forced Colors Mode: when the user preference
    // is active, override author colors with the forced system palette
    // (respecting `forced-color-adjust`). Runs after system-color resolution so
    // it sees final Rgba values and after the full cascade so it sees the final
    // `forced-color-adjust` value.
    if forced_colors_active() {
        apply_forced_colors_mode(doc, node, &mut style, dark_mode);
    }

    // CSS Overflow L3 §2.1: if one axis is `visible` and the other is not,
    // the `visible` axis becomes `auto` (both axes must agree on visibility).
    (style.overflow_x, style.overflow_y) = coerce_overflow_axes(style.overflow_x, style.overflow_y);

    // CSS Logical Properties L1 — resolve logical properties to physical.
    resolve_logical_properties(&mut style);

    // CSS Basic UI L4 §4.4 — field-sizing: content post-pass.
    // apply_ua_form_controls ran before the cascade and may have set explicit UA
    // dimensions. Now that field_sizing is final, clear width/height for text-entry
    // controls so lay_out picks up field_sizing_content_intrinsic dimensions instead.
    if style.field_sizing == FieldSizing::Content {
        apply_ua_form_controls_field_sizing_clear(doc, node, &mut style);
    }

    // CSS Fonts L4 §13 — resolve `font-palette: <dashed-ident>` against the
    // stylesheet's `@font-palette-values` rules now: paint builds the display
    // list from ComputedStyle alone and has no stylesheet access. Runs after
    // the full cascade so it sees the final `font-palette` and `font-family`.
    style.font_palette_resolved = match &style.font_palette {
        FontPalette::Custom(name) => resolve_font_palette_overrides(
            &sheet.font_palette_values,
            name,
            style.font_family.first().map(String::as_str).unwrap_or(""),
        ),
        _ => None,
    };

    apply_webkit_scrollbar_pseudos(doc, node, sheet, &mut style, viewport, dark_mode);

    // Last, so every earlier pass has already written its box-model lengths and
    // each is scaled exactly once. `font_size` was handled next to the cascade's
    // font-size pre-pass and is deliberately not re-scaled here.
    let z = style.effective_zoom;
    apply_zoom_to_lengths(&mut style, z);

    style
}

/// Whether a scrollbar can ever be shown for `node`, i.e. whether translating
/// `::-webkit-scrollbar*` onto its `scrollbar-width`/`scrollbar-color` can have
/// any effect (BUG-341 S11).
///
/// The condition mirrors paint's own: `lumen_paint::display_list` emits a
/// scrollbar only for a box whose `overflow-x`/`overflow-y` is `scroll` or
/// `auto` (`overflow: hidden` scrolls programmatically but draws no bar), and
/// `box_tree::scrollbar_gutter_{inline,block}` reserve gutter under the same
/// condition. The root element and `<body>` are included regardless: they are
/// the conventional target for styling the *page* scrollbar, and it costs two
/// elements per document to keep that idiom working if the viewport scrollbar
/// ever starts reading its style from them.
///
/// **This is a deliberate behaviour change** (user decision, 2026-07-27),
/// not a pure optimization. Before it, the translation ran on *every* element,
/// so `::-webkit-scrollbar` rules matching a non-scrollable element wrote
/// `scrollbar-width`/`scrollbar-color` there and — both being inherited
/// properties — leaked down to scrollable descendants that matched no rule of
/// their own. WebKit has no such inheritance: `::-webkit-scrollbar` styles the
/// scrollbar of the element it matches. Lumen's leak was an artifact of
/// translating a pseudo-element onto standard inherited properties, so
/// narrowing is also a fidelity fix. The standard `scrollbar-width` /
/// `scrollbar-color` properties are untouched and keep inheriting normally.
fn element_can_have_scrollbar(doc: &Document, node: NodeId, style: &ComputedStyle) -> bool {
    if matches!(style.overflow_x, Overflow::Scroll | Overflow::Auto)
        || matches!(style.overflow_y, Overflow::Scroll | Overflow::Auto)
    {
        return true;
    }
    doc.get(node)
        .element_name()
        .is_some_and(|q| matches!(q.local.as_ref(), "html" | "body"))
}

/// CC-CSS-1: legacy WebKit scrollbar pseudo-elements (`::-webkit-scrollbar`,
/// `::-webkit-scrollbar-thumb`, `::-webkit-scrollbar-track`) are not part of the
/// standard cascade — `PseudoElementKind::Unknown` already parses and matches them
/// (see `pseudo_element_matches`), so this translates their declarations onto the
/// standard `scrollbar-width`/`scrollbar-color` fields, letting pages/chrome that
/// only style scrollbars through the WebKit-only idiom still get a styled result.
/// `-webkit-font-smoothing` needs no handling here: it falls through the ordinary
/// `apply_declaration` catch-all (parsed, then silently ignored) like any other
/// unrecognized property.
///
/// **Runs only for elements that can actually have a scrollbar** — see
/// [`element_can_have_scrollbar`] and BUG-341 "S11" for the behaviour change
/// this implies.
fn apply_webkit_scrollbar_pseudos(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    style: &mut ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) {
    // BUG-341 S10: three full pseudo-element cascades per element — 55% of
    // `compute_style` on Lumen's own chrome, and on every other sheet not one of
    // them can match. Node-independent, so it is decided once per sheet.
    if !with_cascade_index(sheet, viewport, dark_mode, |idx| idx.has_webkit_scrollbar_rules) {
        return;
    }
    // BUG-341 S11: and of the sheets that do declare them, only scroll
    // containers can show the result.
    if !element_can_have_scrollbar(doc, node, style) {
        return;
    }
    #[cfg(test)]
    SCROLLBAR_PSEUDO_CASCADES.with(|c| c.set(c.get() + 1));
    let _prof = lumen_core::profile::scope_detail("cs_scrollbar_pseudos");
    // `scrollbar-width` (CSS Scrollbars 1 §2) has no numeric keyword — bucket the
    // pixel width into the closest of the two sized keywords. 9px is the midpoint
    // between `thin`'s 6px and `auto`'s 12px used-width (`display_list.rs`), and
    // matches Lumen's own chrome reference (`docs/design/lumen-v3_3.html`).
    if let Some(bar) =
        compute_pseudo_element_style(doc, node, "-webkit-scrollbar", sheet, style, viewport, dark_mode)
        && let Some(w) = bar.width.as_ref().and_then(|l| l.resolve(bar.font_size, None, viewport))
    {
        style.scrollbar_width = if w <= 0.0 {
            ScrollbarWidth::None
        } else if w <= 9.0 {
            ScrollbarWidth::Thin
        } else {
            ScrollbarWidth::Auto
        };
    }
    let webkit_thumb = compute_pseudo_element_style(
        doc, node, "-webkit-scrollbar-thumb", sheet, style, viewport, dark_mode,
    )
    .and_then(|s| s.background_color)
    .map(|c| c.resolve(style.color));
    let webkit_track = compute_pseudo_element_style(
        doc, node, "-webkit-scrollbar-track", sheet, style, viewport, dark_mode,
    )
    .and_then(|s| s.background_color)
    .map(|c| c.resolve(style.color));
    // Both sides required: `scrollbar-color`'s used value is a (thumb, track) pair,
    // and there is no honest per-side "unset" fallback to reach for here — the UA
    // defaults live one layer down, in `paint::display_list`.
    if let (Some(thumb), Some(track)) = (webkit_thumb, webkit_track) {
        style.scrollbar_color = Some((thumb, track));
    }
}

/// CSS Color 4 §6.2 — resolve `CssColor::System` variants in all CssColor-typed
/// fields of `style` to `CssColor::Rgba` using the element's final used color
/// scheme. Called once at the end of `compute_style`, after all declarations
/// have been applied so `style.color_scheme` is final.
fn resolve_system_colors_in_style(style: &mut ComputedStyle, dark_mode: bool) {
    let dark = style.color_scheme.used_dark(dark_mode);

    macro_rules! resolve_opt {
        ($field:expr) => {
            if let Some(CssColor::System(sc)) = $field {
                *$field = Some(CssColor::Rgba(sc.resolve_color(dark)));
            }
        };
    }
    macro_rules! resolve {
        ($field:expr) => {
            if let CssColor::System(sc) = $field {
                *$field = CssColor::Rgba(sc.resolve_color(dark));
            }
        };
    }

    resolve_opt!(&mut style.background_color);
    resolve!(&mut style.text_decoration_color);
    resolve!(&mut style.text_emphasis_color);
    resolve!(&mut style.border_top_color);
    resolve!(&mut style.border_right_color);
    resolve!(&mut style.border_bottom_color);
    resolve!(&mut style.border_left_color);
    resolve!(&mut style.column_rule_color);
    resolve!(&mut style.gap_rule_color);
}

/// CSS Color Adjustment L1 §3.1 — forces the element's colors to the system
/// palette when Forced Colors Mode is active.
///
/// `forced-color-adjust` is honored: `none` leaves the element untouched;
/// `preserve-parent-color` forces everything except `color`, which keeps its
/// computed (typically inherited, already-forced) value.
///
/// Forced values follow element semantics (§3.1 + HTML UA guidance):
/// links (`a[href]`/`area[href]`) → `LinkText`, disabled controls → `GrayText`,
/// buttons → `ButtonText`/`ButtonFace`/`ButtonBorder`, text fields →
/// `CanvasText`/`Field`; everything else → `CanvasText`/`Canvas`.
/// `box-shadow`/`text-shadow` are forced to none; non-`url()` background
/// images (gradients, cross-fades, `paint()`) are dropped — `url()` images
/// are kept per spec. `background-color` keeps the author's full transparency:
/// an unset or `transparent` background stays transparent.
fn apply_forced_colors_mode(doc: &Document, node: NodeId, style: &mut ComputedStyle, dark_mode: bool) {
    if style.forced_color_adjust == ForcedColorAdjust::None {
        return;
    }
    let dark = style.color_scheme.used_dark(dark_mode);

    // Element semantics for system-color pair selection.
    let mut is_link = false;
    let mut is_button = false;
    let mut is_field = false;
    let mut is_disabled = false;
    if let NodeData::Element { name, .. } = &doc.get(node).data {
        let tag = name.local.as_str();
        is_link = matches!(tag, "a" | "area") && doc.get(node).get_attr("href").is_some();
        let input_type = if tag == "input" {
            doc.get(node)
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string())
        } else {
            String::new()
        };
        is_button = tag == "button" || matches!(input_type.as_str(), "button" | "submit" | "reset");
        is_field = matches!(tag, "textarea" | "select") || (tag == "input" && !is_button);
        is_disabled = matches!(tag, "input" | "textarea" | "select" | "button")
            && doc.get(node).get_attr("disabled").is_some();
    }

    let fg_kw = if is_disabled {
        SystemColor::GrayText
    } else if is_link {
        SystemColor::LinkText
    } else if is_button {
        SystemColor::ButtonText
    } else {
        SystemColor::CanvasText
    };
    let fg = fg_kw.resolve_color(dark);
    let border = if is_button { SystemColor::ButtonBorder } else { SystemColor::CanvasText }
        .resolve_color(dark);
    let bg = if is_button {
        SystemColor::ButtonFace
    } else if is_field {
        SystemColor::Field
    } else {
        SystemColor::Canvas
    }
    .resolve_color(dark);

    // §3.2 `preserve-parent-color`: only the `color` property escapes forcing.
    if style.forced_color_adjust != ForcedColorAdjust::PreserveParentColor {
        style.color = fg;
    }

    // background-color: forced to the backdrop system color, but the author's
    // full transparency is preserved (unset / alpha 0 stays transparent).
    let bg_visible = match &style.background_color {
        Some(CssColor::Rgba(c)) => c.a > 0,
        Some(CssColor::Wide(w)) => w.a > 0.0,
        // System already resolved to Rgba by resolve_system_colors_in_style;
        // CurrentColor follows the (forced, opaque) `color`.
        Some(CssColor::CurrentColor) | Some(CssColor::System(_)) => true,
        None => false,
    };
    if bg_visible {
        style.background_color = Some(CssColor::Rgba(bg));
    }

    style.border_top_color = CssColor::Rgba(border);
    style.border_right_color = CssColor::Rgba(border);
    style.border_bottom_color = CssColor::Rgba(border);
    style.border_left_color = CssColor::Rgba(border);
    style.column_rule_color = CssColor::Rgba(border);
    style.gap_rule_color = CssColor::Rgba(border);
    if !matches!(style.outline_color, OutlineColor::Auto) {
        style.outline_color = OutlineColor::Color(fg);
    }
    style.text_decoration_color = CssColor::Rgba(fg);
    style.text_emphasis_color = CssColor::Rgba(fg);
    if style.caret_color.is_some() {
        // `auto` (None) already follows the forced `color`.
        style.caret_color = Some(fg);
    }

    // SVG geometry is painted from `fill`/`stroke` (§3.1 lists both).
    if !matches!(style.svg_fill, SvgPaint::None) {
        style.svg_fill = SvgPaint::Color(fg);
    }
    if !matches!(style.svg_stroke, SvgPaint::None) {
        style.svg_stroke = SvgPaint::Color(fg);
    }

    // Shadows are forced to `none`.
    style.box_shadow.clear();
    style.text_shadow.clear();

    // `scrollbar-color` computes to `auto` (§3.1): the system palette owns the
    // scrollbar, an author thumb/track pair would punch a hole in it. `None`
    // *is* the `auto` representation of the field (BUG-388).
    style.scrollbar_color = None;

    // `font-variant-emoji`: «If font-variant-emoji computes to normal or
    // unicode, UAs should force any emoji on the page to its monochrome
    // variant … by forcing the computed value … to text» (§3.1). An explicit
    // `emoji` is the author asking for colour on purpose and survives, as does
    // `text` (already monochrome) — so only the two neutral values move.
    if matches!(style.font_variant_emoji, FontVariantEmoji::Normal | FontVariantEmoji::Unicode) {
        style.font_variant_emoji = FontVariantEmoji::Text;
    }

    // background-image: gradients / cross-fades / paint() are dropped;
    // `url()` images are kept (spec: forced to none unless a url()).
    for layer in &mut style.background_layers {
        if !matches!(layer.image, BackgroundImage::None | BackgroundImage::Url(_)) {
            layer.image = BackgroundImage::None;
        }
    }
}

/// CSS Overflow L3 §2.1: coerce mismatched overflow axes.
/// If one axis is `visible` and the other is not, `visible` becomes `auto`.
fn coerce_overflow_axes(ox: Overflow, oy: Overflow) -> (Overflow, Overflow) {
    let new_ox = if ox == Overflow::Visible && oy != Overflow::Visible { Overflow::Auto } else { ox };
    let new_oy = if oy == Overflow::Visible && ox != Overflow::Visible { Overflow::Auto } else { oy };
    (new_ox, new_oy)
}

/// CSS Logical Properties L1 — resolve logical properties to physical.
/// Depends on writing-mode to determine which physical properties correspond to inline/block axis.
/// Phase 0: horizontal-tb only (inline-start=left, inline-end=right, block-start=top, block-end=bottom).
fn resolve_logical_properties(style: &mut ComputedStyle) {
    // In horizontal-tb writing mode (default, Phase 0):
    // inline-start = left, inline-end = right, block-start = top, block-end = bottom.
    // For other writing modes, mapping differs; Phase 1+ will implement full support.

    // CSS Logical Properties L1 §2 — inline-size / block-size → width / height.
    if style.inline_size.is_some() && style.width.is_none() {
        style.width = style.inline_size.clone();
    }
    if style.block_size.is_some() && style.height.is_none() {
        style.height = style.block_size.clone();
    }

    // CSS Logical Properties L1 §4 — inset-inline-* / inset-block-* → top/right/bottom/left.
    // Phase 0: horizontal-tb (inline-start=left, inline-end=right).
    if style.inset_inline_start != LengthOrAuto::Auto && style.left == LengthOrAuto::Auto {
        style.left = style.inset_inline_start.clone();
    }
    if style.inset_inline_end != LengthOrAuto::Auto && style.right == LengthOrAuto::Auto {
        style.right = style.inset_inline_end.clone();
    }
    if style.inset_block_start != LengthOrAuto::Auto && style.top == LengthOrAuto::Auto {
        style.top = style.inset_block_start.clone();
    }
    if style.inset_block_end != LengthOrAuto::Auto && style.bottom == LengthOrAuto::Auto {
        style.bottom = style.inset_block_end.clone();
    }

    // CSS Logical Properties L1 §5 — margin-inline-* / margin-block-* → margin-left/right/top/bottom.
    if style.margin_inline_start != LengthOrAuto::ZERO && style.margin_left == LengthOrAuto::ZERO {
        style.margin_left = style.margin_inline_start.clone();
    }
    if style.margin_inline_end != LengthOrAuto::ZERO && style.margin_right == LengthOrAuto::ZERO {
        style.margin_right = style.margin_inline_end.clone();
    }
    if style.margin_block_start != LengthOrAuto::ZERO && style.margin_top == LengthOrAuto::ZERO {
        style.margin_top = style.margin_block_start.clone();
    }
    if style.margin_block_end != LengthOrAuto::ZERO && style.margin_bottom == LengthOrAuto::ZERO {
        style.margin_bottom = style.margin_block_end.clone();
    }

    // CSS Logical Properties L1 §6 — padding-inline-* / padding-block-* → padding-left/right/top/bottom.
    if style.padding_inline_start != Length::Px(0.0) && style.padding_left == Length::Px(0.0) {
        style.padding_left = style.padding_inline_start.clone();
    }
    if style.padding_inline_end != Length::Px(0.0) && style.padding_right == Length::Px(0.0) {
        style.padding_right = style.padding_inline_end.clone();
    }
    if style.padding_block_start != Length::Px(0.0) && style.padding_top == Length::Px(0.0) {
        style.padding_top = style.padding_block_start.clone();
    }
    if style.padding_block_end != Length::Px(0.0) && style.padding_bottom == Length::Px(0.0) {
        style.padding_bottom = style.padding_block_end.clone();
    }

    // CSS Logical Properties L1 §7 — border-inline-*-width / border-block-*-width.
    if style.border_inline_start_width > 0.0 && style.border_left_width == 0.0 {
        style.border_left_width = style.border_inline_start_width;
    }
    if style.border_inline_end_width > 0.0 && style.border_right_width == 0.0 {
        style.border_right_width = style.border_inline_end_width;
    }
    if style.border_block_start_width > 0.0 && style.border_top_width == 0.0 {
        style.border_top_width = style.border_block_start_width;
    }
    if style.border_block_end_width > 0.0 && style.border_bottom_width == 0.0 {
        style.border_bottom_width = style.border_block_end_width;
    }
}

// ── Pseudo-element style matching ───────────────────────────────────────────

/// Проверяет, является ли `complex` правилом для псевдоэлемента `pseudo`
/// (например "before" для `::before`) на элементе `node`.
/// Если да — возвращает specificity исходного (полного) селектора.
/// Алгоритм: последний compound должен содержать `PseudoElement(pseudo)`;
/// остаток селектора (после удаления этой части) проверяется через
/// существующий `matches_complex`.
/// The name `kind` is written with, without the leading `::`.
///
/// BUG-341 S23: the single source of truth for the kind↔name correspondence.
/// [`pseudo_element_matches`] and [`CascadeIndex::pseudo_subjects`] both go
/// through it, so the sheet-level "does this pseudo appear at all" predicate
/// cannot drift from the matcher it is meant to short-circuit — a drift that
/// would silently drop a pseudo-element's styling, not slow a frame down.
/// Parameterized kinds report the bare name they are spelled with: the
/// argument (`::slotted(sel)`, `::highlight(name)`, `::picker(sel)`) is checked
/// by the matcher, not by the name.
fn pseudo_element_name(kind: &PseudoElementKind) -> &str {
    match kind {
        PseudoElementKind::Before => "before",
        PseudoElementKind::After => "after",
        PseudoElementKind::FirstLine => "first-line",
        PseudoElementKind::FirstLetter => "first-letter",
        PseudoElementKind::Slotted(_) => "slotted",
        PseudoElementKind::Marker => "marker",
        PseudoElementKind::Selection => "selection",
        PseudoElementKind::Placeholder => "placeholder",
        PseudoElementKind::Highlight(_) => "highlight",
        PseudoElementKind::Picker(_) => "picker",
        PseudoElementKind::Checkmark => "checkmark",
        PseudoElementKind::PickerIcon => "picker-icon",
        PseudoElementKind::Unknown(s) => s.as_str(),
    }
}

/// Helper: check if a pseudo-element name matches a PseudoElementKind.
fn pseudo_element_matches(kind: &PseudoElementKind, name: &str) -> bool {
    pseudo_element_name(kind).eq_ignore_ascii_case(name)
}

#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn matches_complex_for_pseudo(
    complex: &ComplexSelector,
    pseudo: &str,
    doc: &Document,
    node: NodeId,
) -> Option<Specificity> {
    let last = complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head);
    if !last.parts.iter().any(|p| {
        matches!(p, SimpleSelector::PseudoElement(n) if pseudo_element_matches(n, pseudo))
    }) {
        return None;
    }
    // Строим модифицированный последний compound без PseudoElement.
    let stripped = CompoundSelector {
        parts: last.parts.iter()
            .filter(|p| !matches!(p, SimpleSelector::PseudoElement(_)))
            .cloned()
            .collect(),
    };
    // Собираем модифицированный ComplexSelector.
    let modified = if complex.tail.is_empty() {
        ComplexSelector { head: stripped, tail: vec![] }
    } else {
        let mut tail = complex.tail.clone();
        tail.last_mut().unwrap().1 = stripped;
        ComplexSelector { head: complex.head.clone(), tail }
    };
    if matches_complex(&modified, doc, node) {
        Some(complex.specificity())
    } else {
        None
    }
}

/// Build a `ComputedStyle` from a flat list of declarations with neutral context.
///
/// Used by `@starting-style` cascade wiring: converts the declarations returned by
/// `resolve_starting_style()` into a `ComputedStyle` that serves as the
/// *before-change* style for CSS entry transitions (CSS Transitions L2 §3.4).
///
/// Context defaults: em_basis = 16 px, inherited = default, non-quirks mode.
/// Suitable for transition value extraction (`opacity`, `color`, `background-color`,
/// `transform`).
pub fn compute_style_from_declarations(decls: &[Declaration], viewport: Size) -> ComputedStyle {
    let inherited = ComputedStyle::root();
    let mut style = inherited.clone();
    for decl in decls {
        apply_declaration(&mut style, decl, 16.0, viewport, FontWeight::NORMAL, &inherited, &inherited, false, false);
    }
    style
}

/// CSS Pseudo-Elements L4 §5.5 — true when property `prop` is one of the limited
/// set that applies to the `::marker` pseudo-element: all font properties, the
/// `white-space` property, `color`, the `direction` / `unicode-bidi` /
/// `text-combine-upright` writing-mode properties, `content`, and all animation
/// and transition properties. Custom properties (`--*`) are kept so `var()` inside
/// `content` still resolves. Any other declaration on `::marker` is ignored.
fn marker_property_applies(prop: &str) -> bool {
    let p = prop.trim().to_ascii_lowercase();
    // Custom properties stay available for `var()` substitution inside `content`.
    if p.starts_with("--") {
        return true;
    }
    // `font`, `font-*`, `animation*` and `transition*` families are allowed wholesale.
    p.starts_with("font")
        || p.starts_with("animation")
        || p.starts_with("transition")
        || matches!(
            p.as_str(),
            "color"
                | "content"
                | "white-space"
                | "white-space-collapse"
                | "direction"
                | "unicode-bidi"
                | "text-combine-upright"
        )
}

/// Builds the starting `ComputedStyle` for a pseudo-element of `parent`: every
/// field at its initial value (CSS Pseudo-elements L4 §4 makes `display`
/// `inline`), then every inherited property copied down from the originating
/// element.
///
/// BUG-341 S10: extracted from [`compute_pseudo_element_style`] so it can run
/// *after* the cascade match rather than before it. It costs a 302-field
/// literal plus ~50 field clones, and the overwhelmingly common outcome — no
/// rule matches this element for this pseudo-element — threw all of it away.
/// The profile that found it: `precompute_counters` probes `::before`/`::after`
/// on *every* node to keep `quotes` nesting continuous (1656 calls per chrome
/// layout pass), and `apply_webkit_scrollbar_pseudos` adds three more per
/// element.
fn pseudo_inherited_style(parent: &ComputedStyle) -> ComputedStyle {
    #[cfg(test)]
    PSEUDO_BASE_BUILDS.with(|c| c.set(c.get() + 1));
    // Pseudo-elements inherit from their originating element.
    // Start from root() (all fields at initial values) then override inherited properties.
    // CSS Pseudo-elements L4 §4: default display = inline.
    let mut style = ComputedStyle::root();
    style.display = Display::Inline;
    style.content = Content::Normal;
    // Inherited properties — copy from parent.
    style.color = parent.color;
    style.color_space = parent.color_space;
    style.text_align = parent.text_align;
    style.direction = parent.direction;
    style.font_size = parent.font_size;
    style.line_height = parent.line_height;
    style.line_height_is_relative = parent.line_height_is_relative;
    style.line_height_step = parent.line_height_step;
    style.font_style = parent.font_style;
    style.font_weight = parent.font_weight;
    style.font_variant_caps = parent.font_variant_caps;
    style.font_variant_emoji = parent.font_variant_emoji;
    style.font_stretch = parent.font_stretch;
    style.font_family = parent.font_family.clone();
    style.font_variation_settings = parent.font_variation_settings.clone();
    style.font_feature_settings = parent.font_feature_settings.clone();
    style.font_palette = parent.font_palette.clone();
    style.font_palette_resolved = parent.font_palette_resolved.clone();
    style.text_transform = parent.text_transform;
    style.white_space = parent.white_space;
    style.white_space_collapse = parent.white_space_collapse;
    style.text_indent = parent.text_indent.clone();
    style.letter_spacing = parent.letter_spacing;
    style.word_spacing = parent.word_spacing;
    style.text_decoration_line = parent.text_decoration_line;
    style.text_decoration_color = parent.text_decoration_color;
    style.text_decoration_style = parent.text_decoration_style;
    style.text_decoration_thickness = parent.text_decoration_thickness;
    style.text_emphasis_style = parent.text_emphasis_style.clone();
    style.text_emphasis_color = parent.text_emphasis_color;
    style.text_emphasis_position = parent.text_emphasis_position;
    style.text_underline_position = parent.text_underline_position;
    style.text_underline_offset = parent.text_underline_offset;
    style.text_decoration_skip_ink = parent.text_decoration_skip_ink;
    style.accent_color = parent.accent_color;
    style.color_scheme = parent.color_scheme;
    style.custom_props = parent.custom_props.clone();
    style.visibility = parent.visibility;
    style.cursor = parent.cursor;
    style.text_shadow = parent.text_shadow.clone();
    style.user_select = parent.user_select;
    style.scroll_behavior = parent.scroll_behavior;
    style.tab_size = parent.tab_size;
    style.caret_color = parent.caret_color;
    style.overflow_wrap = parent.overflow_wrap;
    style.word_break = parent.word_break;
    style.line_break = parent.line_break;
    style.hyphens = parent.hyphens;
    style.list_style_type = parent.list_style_type.clone();
    style.list_style_position = parent.list_style_position;
    style.list_style_image = parent.list_style_image.clone();
    style.orphans = parent.orphans;
    style.widows = parent.widows;
    style.scrollbar_width = parent.scrollbar_width;
    style.scrollbar_color = parent.scrollbar_color;
    style.image_rendering = parent.image_rendering;
    style.writing_mode = parent.writing_mode;
    style.text_orientation = parent.text_orientation;
    style.ruby_position = parent.ruby_position;
    style.ruby_align = parent.ruby_align;
    style.ruby_merge = parent.ruby_merge;
    style.math_style = parent.math_style;
    style.math_depth = parent.math_depth;
    style.font_size_adjust = parent.font_size_adjust;
    style.text_wrap_mode = parent.text_wrap_mode;
    style.text_wrap_style = parent.text_wrap_style;
    style.interpolate_size = parent.interpolate_size;
    style.quotes = parent.quotes.clone();
    style
}

/// CSS Pseudo-elements L4 §3.4 — inheritance through the `::first-line` /
/// `::first-letter` fictional tag sequence.
///
/// The pseudo-element is the *parent* of the affected content, not a blanket
/// override of it: a descendant that specifies a property itself (`<b>`'s
/// `font-weight`, `<em>`'s `font-style`, an inline `style="color:…"`) keeps its
/// own value; only what it merely inherited comes from the pseudo-element.
/// Replacing the whole style instead silently drops those inner declarations.
///
/// - `own` — the fragment's/segment's computed style (the descendant);
/// - `base` — the originating element's style, which `own` inherited from;
/// - `pseudo` — the `::first-line` / `::first-letter` style.
///
/// A property is taken from `pseudo` only when `own` still equals `base` for it,
/// i.e. nothing in the inline chain specified it. Only the properties that apply
/// to these pseudo-elements (§3.2 / §4.4) and are meaningful for a text run are
/// merged — box-level ones (background, margins) are painted from the
/// pseudo-element's own box, not from the fragment.
///
/// Approximation: a descendant that *re-declares* the originating element's own
/// value (`color: blue` inside a `color: blue` block) is indistinguishable from
/// plain inheritance here and loses to the pseudo-element.
pub fn merge_pseudo_inherited(
    own: &ComputedStyle,
    base: &ComputedStyle,
    pseudo: &ComputedStyle,
) -> ComputedStyle {
    let mut out = own.clone();
    // `own == base` for a property ⇒ it was inherited ⇒ the pseudo-element
    // supplies it. Split by `Copy`-ness: `clone()` on a `Copy` field would trip
    // `clippy::clone_on_copy`.
    macro_rules! take_copy {
        ($($f:ident),+ $(,)?) => { $(if out.$f == base.$f { out.$f = pseudo.$f; })+ };
    }
    macro_rules! take_clone {
        ($($f:ident),+ $(,)?) => { $(if out.$f == base.$f { out.$f = pseudo.$f.clone(); })+ };
    }
    take_copy!(
        color,
        color_space,
        font_size,
        line_height,
        line_height_is_relative,
        font_style,
        font_weight,
        font_variant_caps,
        font_variant_emoji,
        font_stretch,
        font_optical_sizing,
        font_size_adjust,
        text_transform,
        letter_spacing,
        word_spacing,
        text_decoration_line,
        text_decoration_style,
        text_decoration_thickness,
        text_decoration_skip_ink,
        text_emphasis_position,
        vertical_align,
    );
    take_clone!(
        font_family,
        font_variation_settings,
        font_feature_settings,
        font_palette,
        font_palette_resolved,
        text_decoration_color,
        text_emphasis_style,
        text_emphasis_color,
        text_shadow,
    );
    out
}

/// Вычисляет стиль для псевдоэлемента `::before` или `::after` элемента `node`.
///
/// `pseudo` — "before" или "after" (без "::"). `dark_mode` forwarded to
/// `@media (prefers-color-scheme: dark)` matching.
///
/// Возвращает `None` если:
/// - нет CSS-правил для данного псевдоэлемента на этом узле, или
/// - вычисленный `content` равен `none` / `normal`.
pub fn compute_pseudo_element_style(
    doc: &Document,
    node: NodeId,
    pseudo: &str,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        return None;
    }
    // BUG-341 S20 census hook — see `PseudoCascadeStats`.
    if !PSEUDO_STATS_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return compute_pseudo_element_style_inner(doc, node, pseudo, sheet, parent, viewport, dark_mode);
    }
    let t_pseudo = std::time::Instant::now();
    let out = compute_pseudo_element_style_inner(doc, node, pseudo, sheet, parent, viewport, dark_mode);
    note_pseudo_cascade(pseudo, t_pseudo.elapsed().as_nanos() as u64, out.is_some());
    out
}

/// The body of [`compute_pseudo_element_style`] — split out so the census hook
/// above covers every exit path.
#[allow(clippy::too_many_arguments)]
fn compute_pseudo_element_style_inner(
    doc: &Document,
    node: NodeId,
    pseudo: &str,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    let _prof = lumen_core::profile::scope_detail("pseudo_style");

    // BUG-341 S23: if the sheet never uses this pseudo-element as a selector
    // subject, `matches_complex_for_pseudo` below cannot match anything and the
    // whole cascade is a no-op. `::marker` is the one exception — it
    // synthesizes a style out of `list-style-type` with no rule at all (CSS
    // Lists L3 §2.1, the `matched.is_empty()` branch), so it must never be
    // short-circuited here.
    if !pseudo.eq_ignore_ascii_case("marker")
        && !sheet_targets_pseudo(sheet, viewport, dark_mode, pseudo)
    {
        return None;
    }

    // Собираем matching declarations из всех правил.
    //
    // BUG-284: candidate pre-filter via the same thread-local `CascadeIndex` as
    // `compute_style` (subject-key bucketing is agnostic to `::before`/`::after`
    // being appended to the subject compound, so the same index is valid here).
    // This function runs for *every* element for both "before" and "after" —
    // unlike `compute_style`, it was never indexed at all, making it one of the
    // largest un-indexed cascade costs on stylesheets with many `@media` rules.
    //
    // BUG-341 S10: matching runs *before* `pseudo_inherited_style` — see that
    // function's doc comment. Nothing here reads the pseudo-element's own style.
    let prof_match = lumen_core::profile::scope_detail("ps_match");
    let mut matched: Vec<(bool, Specificity, usize, usize, &Declaration)> = Vec::new();
    let node_data = doc.get(node);
    let node_tag = node_data.element_name().map_or("", |q| q.local.as_str());
    let node_id = node_data.get_attr("id");
    let class_attr = node_data.get_attr("class").unwrap_or("");
    let node_classes: Vec<&str> = class_attr.split_whitespace().collect();
    ensure_cascade_index(sheet, viewport, dark_mode);
    let cands = with_front_cascade_index(|idx| {
        idx.rules.candidates(node_tag, node_id, &node_classes)
    });
    for rule_idx in cands {
        let rule = &sheet.rules[rule_idx];
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if let Some(spec) = matches_complex_for_pseudo(complex, pseudo, doc, node) {
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                matched.push((decl.important, spec, rule_idx, decl_idx, decl));
            }
        }
    }

    // Perf: see the analogous @media/@supports comments in `compute_style` —
    // "active" precomputed once per (sheet, viewport, dark_mode) rather than
    // re-evaluated on every element (this function runs twice per element,
    // for `::before` and `::after`).
    let active_media = with_front_cascade_index(|idx| idx.active_media.clone());
    let mut next_rule_idx = sheet.rules.len();
    for (media_i, media) in sheet.media_rules.iter().enumerate() {
        if !active_media[media_i] {
            next_rule_idx += media.rules.len();
            continue;
        }
        let media_cands = with_front_cascade_index(|idx| {
            idx.media[media_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in media_cands {
            let rule = &media.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_complex_for_pseudo(complex, pseudo, doc, node) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    matched.push((decl.important, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += media.rules.len();
    }
    // CSS Conditional Rules L3 §2 — @supports in pseudo-element context.
    let active_supports = with_front_cascade_index(|idx| idx.active_supports.clone());
    for (supports_i, supports) in sheet.supports_rules.iter().enumerate() {
        if !active_supports[supports_i] {
            next_rule_idx += supports.rules.len();
            continue;
        }
        let supports_cands = with_front_cascade_index(|idx| {
            idx.supports[supports_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in supports_cands {
            let rule = &supports.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_complex_for_pseudo(complex, pseudo, doc, node) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    matched.push((decl.important, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += supports.rules.len();
    }

    if matched.is_empty() {
        // CSS Lists L3 §2.1: ::marker always generates a marker box from list-style-type
        // without any explicit CSS rule. Other pseudo-elements require a matching declaration.
        if pseudo.eq_ignore_ascii_case("marker") {
            let _prof_init = lumen_core::profile::scope_detail("ps_init");
            return Some(pseudo_inherited_style(parent));
        }
        return None;
    }
    drop(prof_match);
    let mut style = {
        let _prof_init = lumen_core::profile::scope_detail("ps_init");
        pseudo_inherited_style(parent)
    };
    let _prof_apply = lumen_core::profile::scope_detail("ps_apply");

    matched.sort_by_key(|&(imp, spec, rule_idx, decl_idx, _)| (imp, spec, rule_idx, decl_idx));

    let parent_fs = parent.font_size;
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for (_, _, _, _, decl) in &matched {
        // Pseudo-element style: the basis is irrelevant here — `zoom` is folded
        // into the originating element's style, which this one inherits from.
        let _ = apply_font_size(&mut style, decl, parent_fs, parent_fs, viewport, is_quirks);
    }
    let em_basis = style.font_size;
    let parent_weight = parent.font_weight;
    // CSS Pseudo-Elements L4 §5.5 — only a restricted set of properties applies to
    // `::marker`. Declarations outside that set (e.g. `line-height`, `margin`,
    // `background`) are dropped so a `::marker` rule cannot perturb marker layout
    // or paint beyond the spec-permitted font/color/text-flow styling.
    let is_marker = pseudo.eq_ignore_ascii_case("marker");
    for (_, _, _, _, decl) in &matched {
        if is_marker && !marker_property_applies(&decl.property) {
            continue;
        }
        let attr_buf;
        let effective_decl: &Declaration = if decl.value.contains("attr(") {
            let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
            attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
            &attr_buf
        } else {
            decl
        };
        apply_declaration(&mut style, effective_decl, em_basis, viewport, parent_weight, parent, parent, is_quirks, dark_mode);
    }

    // ::before/::after require content: to render; ::first-letter/::first-line do not.
    // ::marker renders by default (content comes from list-style-type); content:none suppresses it.
    // ::selection applies to active text selection — no content required (CSS Pseudo-elements L4 §5.6).
    // ::placeholder styles the UA-generated placeholder hint text — no content required
    // (CSS Pseudo-elements L4 §4.10).
    // CC-CSS-1: `::-webkit-scrollbar`/`-thumb`/`-track` are legacy scrollbar-styling
    // pseudo-elements (translated onto `scrollbar-width`/`scrollbar-color` by
    // `apply_webkit_scrollbar_pseudos`) — no `content:` required either.
    if pseudo.eq_ignore_ascii_case("first-letter")
        || pseudo.eq_ignore_ascii_case("first-line")
        || pseudo.eq_ignore_ascii_case("selection")
        || pseudo.eq_ignore_ascii_case("placeholder")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar-thumb")
        || pseudo.eq_ignore_ascii_case("-webkit-scrollbar-track")
    {
        Some(style)
    } else if pseudo.eq_ignore_ascii_case("marker") {
        match &style.content {
            Content::None => None,
            _ => Some(style),
        }
    } else {
        match &style.content {
            Content::Items(_) => Some(style),
            _ => None,
        }
    }
}

/// Computes the `::selection` override style for a DOM element.
///
/// Collects all CSS rules targeting `element::selection`, applies declarations
/// in specificity order, and returns the computed style. Returns `None` when
/// no `::selection` rules match `node` (callers should fall back to the OS
/// default selection highlight colour in that case).
///
/// Only a limited subset of properties are honoured by `::selection` per
/// CSS Pseudo-elements L4 §5.6: `color`, `background-color`,
/// `text-decoration-*`, `text-shadow`. Other declared properties are parsed
/// and stored but should be ignored by the paint layer.
pub fn compute_selection_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    parent: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> Option<ComputedStyle> {
    compute_pseudo_element_style(doc, node, "selection", sheet, parent, viewport, dark_mode)
}

/// CSS Properties and Values L1 §1.1: для каждого зарегистрированного
/// custom property, у которого нет значения в `custom_props`, подставляет
/// `initial-value` (если он указан). Невызов для `inherits: true` имени
/// с унаследованным значением — потому что `contains_key` уже возвращает
/// true. Для `inherits: false` имени родительское значение было выпилено
/// в `compute_style` через `retain`.
/// BUG-341 S9: takes [`CustomProps`] rather than the bare map so the
/// copy-on-write copy happens only if a value is really substituted — the common
/// case (no `@property` rules at all, or all of them already resolved) leaves the
/// node sharing its parent's allocation.
fn apply_property_initial_values(
    custom_props: &mut CustomProps,
    registry: &HashMap<&str, &PropertyRule>,
) {
    for (name, p) in registry {
        if custom_props.contains_key(*name) {
            continue;
        }
        if let Some(iv) = &p.initial_value {
            // CSS Properties and Values L1 §1.1: initial-value валидируется
            // против syntax. Per spec — невалидный initial делает @property
            // невалидным целиком; Phase 0 более снисходителен и просто
            // не подставляет неподходящий initial (потомок без декларации
            // получит inherited или ничего).
            if validate_against_syntax(iv, &p.syntax) {
                custom_props.make_mut().insert((*name).to_string(), iv.clone());
            }
        }
    }
}

/// CSS Properties and Values L1 §2 — упрощённая валидация значения
/// custom property против `syntax`-дескриптора.
///
/// Поддерживаются:
/// - `*` — универсал (любое значение проходит);
/// - `<length>` — px, em, rem, vh, vw, vmin, vmax (но не `%`);
/// - `<percentage>` — число с суффиксом `%`;
/// - `<length-percentage>` — union;
/// - `<color>` — любая форма, которую парсит `parse_color`;
/// - `<integer>` — целое со знаком;
/// - `<number>` — число с плавающей точкой;
/// - `<angle>` — `deg` / `rad` / `turn` / `grad`;
/// - `<time>` — `s` / `ms` (CSS Values L4 §8);
/// - `<resolution>` — `dpi` / `dpcm` / `dppx` / `x` (CSS Values L4 §9.1);
/// - `<custom-ident>` — идентификатор, не совпадающий с CSS-wide keyword.
///
/// Union через `|` — match если хоть одна альтернатива принимает. Прочие
/// типы (`<image>`, `<url>`, `<transform-function>`, и т.д.) и multipliers
/// (`+`, `#`) в Phase 0 трактуются как universal — возвращают `true`,
/// чтобы не отбраковывать корректные value у потребителей этих типов.
pub fn validate_against_syntax(value: &str, syntax: &str) -> bool {
    let syntax = syntax.trim();
    if syntax == "*" {
        return true;
    }
    let value = value.trim();
    // Union по `|`.
    for alt in syntax.split('|') {
        let alt = alt.trim();
        let matched = match alt {
            "<length>" => matches_syntax_length(value),
            "<percentage>" => matches_syntax_percentage(value),
            "<length-percentage>" => {
                matches_syntax_length(value) || matches_syntax_percentage(value)
            }
            "<color>" => parse_color(value).is_some(),
            "<integer>" => matches_syntax_integer(value),
            "<number>" => matches_syntax_number(value),
            "<angle>" => matches_syntax_angle(value),
            "<time>" => matches_syntax_time(value),
            "<resolution>" => matches_syntax_resolution(value),
            "<custom-ident>" => matches_syntax_custom_ident(value),
            // Неизвестный тип — permissive, чтобы не блокировать корректные
            // declarations с пока-неподдержанными syntax-формами.
            _ => true,
        };
        if matched {
            return true;
        }
    }
    false
}

fn matches_syntax_length(value: &str) -> bool {
    // <length> = px/em/rem/vh/vw/vmin/vmax/calc(...) — без `%`.
    match parse_length(value) {
        Some(Length::Percent(_)) => false,
        Some(_) => true,
        None => false,
    }
}

fn matches_syntax_percentage(value: &str) -> bool {
    matches!(parse_length(value), Some(Length::Percent(_)))
}

fn matches_syntax_integer(value: &str) -> bool {
    value.parse::<i64>().is_ok()
}

fn matches_syntax_number(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

fn matches_syntax_angle(value: &str) -> bool {
    // Number + один из суффиксов: deg, rad, turn, grad.
    for suffix in ["deg", "rad", "turn", "grad"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_time(value: &str) -> bool {
    // CSS Values L4 §8 — <time> с суффиксами `s` или `ms`.
    // Порядок важен: `ms` проверяем раньше `s`, иначе `200ms` распарсится
    // как 200m + остаток `s` (а `200m` не валидный number → false).
    for suffix in ["ms", "s"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_resolution(value: &str) -> bool {
    // CSS Values L4 §9.1 — <resolution> с суффиксами `dppx`/`dpcm`/`dpi`/`x`.
    // `dppx` проверяем раньше `dpi`/`dpcm` (длинный суффикс), `x` — последним
    // (резервный alias dppx; HTML5 media queries).
    for suffix in ["dppx", "dpcm", "dpi", "x"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_custom_ident(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    // CSS-wide keywords нельзя использовать как custom-ident.
    if parse_css_wide_keyword(value).is_some() {
        return false;
    }
    // Также запрещены `default` (CSS spec) и `none` в большинстве контекстов.
    // Простая проверка: ident начинается с letter / `_` / `-`, дальше —
    // alphanumeric / `-` / `_`. ASCII-only для простоты.
    let mut chars = value.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ──────────────── selector matching ────────────────

pub(crate) fn matches_complex(complex: &ComplexSelector, doc: &Document, node: NodeId) -> bool {
    // Справа налево с back-tracking. Алгоритм:
    //   1. Складываем (compounds, combinators) в массивы.
    //   2. Рекурсивно: матчим последний compound на текущем `node`; если ОК
    //      и осталось > 0 compound-ов левее, для combinator-а перед ним
    //      перебираем ВСЕ возможные кандидаты (предки для descendant /
    //      earlier-siblings для later-sibling) и рекурсивно матчим суффикс
    //      в каждом. child / next-sibling имеют ровно одного кандидата.
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    compounds.push(&complex.head);
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    matches_chain(&compounds, &combinators, doc, node)
}

/// Рекурсивный matcher с back-tracking. `compounds[last]` матчится на `node`;
/// для левее идущих compound-ов перебираем кандидатов согласно combinator-у.
fn matches_chain(
    compounds: &[&CompoundSelector],
    combinators: &[Combinator],
    doc: &Document,
    node: NodeId,
) -> bool {
    let n = compounds.len();
    debug_assert_eq!(combinators.len(), n - 1);

    if !matches_compound(compounds[n - 1], doc, node) {
        return false;
    }
    if n == 1 {
        return true;
    }

    let comb = combinators[n - 2];
    let prev_compounds = &compounds[..n - 1];
    let prev_combinators = &combinators[..n - 2];

    match comb {
        Combinator::Descendant => {
            // Перебираем всех предков как кандидатов.
            let mut cur = doc.get(node).parent;
            while let Some(p) = cur {
                if is_element(doc, p)
                    && matches_chain(prev_compounds, prev_combinators, doc, p)
                {
                    return true;
                }
                cur = doc.get(p).parent;
            }
            false
        }
        Combinator::Child => {
            // Один кандидат: parent.
            let Some(parent) = doc.get(node).parent else { return false; };
            if !is_element(doc, parent) {
                return false;
            }
            matches_chain(prev_compounds, prev_combinators, doc, parent)
        }
        Combinator::NextSibling => {
            // Один кандидат: предыдущий element-sibling.
            let Some(prev) = previous_element_sibling(doc, node) else { return false; };
            matches_chain(prev_compounds, prev_combinators, doc, prev)
        }
        Combinator::LaterSibling => {
            // Перебираем все earlier-siblings как кандидатов.
            let mut sib = previous_element_sibling(doc, node);
            while let Some(s) = sib {
                if matches_chain(prev_compounds, prev_combinators, doc, s) {
                    return true;
                }
                sib = previous_element_sibling(doc, s);
            }
            false
        }
    }
}

/// CSS Scoping L1 §6.2: true if `node` is a direct light-tree child of a shadow host,
/// meaning it is eligible to be slotted via a `<slot>` in the shadow tree.
fn is_slotted_element(doc: &Document, node: NodeId) -> bool {
    doc.get(node).parent
        .map(|p| doc.is_shadow_host(p))
        .unwrap_or(false)
}

/// CSS Scoping L1 §6.1 — true if the subject (last) compound of `complex`
/// contains a `:host` / `:host(sel)` pseudo-class. Used to select, from a shadow
/// tree's stylesheet, the rules that target the host element (as opposed to rules
/// scoped to shadow descendants).
fn complex_has_host(complex: &ComplexSelector) -> bool {
    let last = complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head);
    last.parts.iter().any(|p| matches!(p, SimpleSelector::PseudoClass(PseudoClass::Host(_))))
}

/// CSS Scoping L1 §6.2 — attempts to match a complex selector containing
/// `::slotted(inner_sel)` against `node`.
///
/// Returns `Some(specificity)` when all conditions hold:
/// 1. The last compound of `complex` contains `::slotted(inner_sel)`.
/// 2. `node` is a slotted element (DOM parent is a shadow host).
/// 3. `node` matches every selector in `inner_sel`.
/// 4. The outer context (compound minus `::slotted`) matches the shadow host (node's parent).
///    If the outer context is empty, no ancestor check is needed.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn matches_slotted_complex(
    complex: &ComplexSelector,
    doc: &Document,
    node: NodeId,
) -> Option<Specificity> {
    // Locate the last compound, which must contain ::slotted.
    let last = complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head);
    let slotted_inner: Option<&Vec<ComplexSelector>> = last.parts.iter().find_map(|p| {
        if let SimpleSelector::PseudoElement(PseudoElementKind::Slotted(inner)) = p {
            Some(inner.as_ref()?)
        } else {
            None
        }
    });
    // Rule must contain ::slotted.
    let inner_selectors = slotted_inner?;

    // Node must be a slotted element.
    if !is_slotted_element(doc, node) {
        return None;
    }

    // Node must match the inner selector list.
    if !inner_selectors.iter().any(|s| matches_complex(s, doc, node)) {
        return None;
    }

    // Build the outer complex selector (strip ::slotted from the last compound).
    let stripped_last = CompoundSelector {
        parts: last.parts.iter()
            .filter(|p| !matches!(p, SimpleSelector::PseudoElement(PseudoElementKind::Slotted(_))))
            .cloned()
            .collect(),
    };

    // If there is no outer context at all, the rule matches.
    if complex.tail.is_empty() && stripped_last.parts.is_empty() {
        return Some(complex.specificity());
    }

    // Outer context: match against the shadow host (node's DOM parent).
    let host = doc.get(node).parent.expect("is_slotted_element ensures parent");
    let outer = if complex.tail.is_empty() {
        ComplexSelector { head: stripped_last, tail: vec![] }
    } else {
        let mut tail = complex.tail.clone();
        tail.last_mut().expect("non-empty tail").1 = stripped_last;
        ComplexSelector { head: complex.head.clone(), tail }
    };

    if matches_complex(&outer, doc, host) {
        Some(complex.specificity())
    } else {
        None
    }
}

fn matches_compound(compound: &CompoundSelector, doc: &Document, node: NodeId) -> bool {
    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return false;
    };
    for part in &compound.parts {
        if !matches_simple(part, doc, node, &name.local, attrs) {
            return false;
        }
    }
    true
}

fn matches_simple(
    sel: &SimpleSelector,
    doc: &Document,
    node: NodeId,
    tag: &str,
    attrs: &[Attribute],
) -> bool {
    match sel {
        SimpleSelector::Type(t) => t == tag,
        SimpleSelector::Class(c) => attrs
            .iter()
            .find(|a| a.name.local == "class")
            .map(|a| a.value.split_whitespace().any(|w| w == c))
            .unwrap_or(false),
        SimpleSelector::Id(i) => attrs
            .iter()
            .find(|a| a.name.local == "id")
            .map(|a| a.value == *i)
            .unwrap_or(false),
        SimpleSelector::Universal => true,
        SimpleSelector::Attribute(a) => matches_attribute(a, attrs),
        SimpleSelector::PseudoClass(p) => matches_pseudo_class(p, doc, node),
        SimpleSelector::PseudoElement(_) => false,
    }
}

fn matches_attribute(sel: &AttrSelector, attrs: &[Attribute]) -> bool {
    let Some(attr) = attrs.iter().find(|a| a.name.local == sel.name) else {
        return false;
    };
    let ci = sel.case_insensitive;
    match (sel.op, sel.value.as_deref()) {
        (None, _) => true,
        (Some(AttrOp::Equals), Some(v)) => str_eq(&attr.value, v, ci),
        (Some(AttrOp::Includes), Some(v)) => {
            !v.is_empty() && attr.value.split_whitespace().any(|w| str_eq(w, v, ci))
        }
        (Some(AttrOp::DashMatch), Some(v)) => {
            // Точное совпадение или префикс с разделителем `-`. `i` применяется
            // к обеим частям сравнения (CSS L4 §6.3.6).
            str_eq(&attr.value, v, ci) || str_starts_with(&attr.value, &format!("{v}-"), ci)
        }
        (Some(AttrOp::Prefix), Some(v)) => !v.is_empty() && str_starts_with(&attr.value, v, ci),
        (Some(AttrOp::Suffix), Some(v)) => !v.is_empty() && str_ends_with(&attr.value, v, ci),
        (Some(AttrOp::Substring), Some(v)) => !v.is_empty() && str_contains(&attr.value, v, ci),
        _ => false,
    }
}

/// ASCII case-insensitive (если `ci`) сравнение, иначе побайтовое. Cyrillic и
/// другой не-ASCII всегда сравнивается побайтово (`eq_ignore_ascii_case` не
/// трогает байты со старшим битом). Работа через `as_bytes()` нужна, чтобы
/// `starts_with`/`ends_with`/`contains` не упирались в char-boundary в
/// многобайтовых UTF-8 строках.
fn str_eq(a: &str, b: &str, ci: bool) -> bool {
    if ci { a.eq_ignore_ascii_case(b) } else { a == b }
}

fn str_starts_with(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.starts_with(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.len() >= n.len() && h[..n.len()].eq_ignore_ascii_case(n)
}

fn str_ends_with(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.ends_with(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.len() >= n.len() && h[h.len() - n.len()..].eq_ignore_ascii_case(n)
}

fn str_contains(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.contains(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    if h.len() < n.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

fn matches_pseudo_class(p: &PseudoClass, doc: &Document, node: NodeId) -> bool {
    match p {
        PseudoClass::FirstChild => is_first_element_child(doc, node),
        PseudoClass::LastChild => is_last_element_child(doc, node),
        PseudoClass::OnlyChild => {
            is_first_element_child(doc, node) && is_last_element_child(doc, node)
        }
        PseudoClass::Empty => is_empty_element(doc, node),
        PseudoClass::Root => is_root_element(doc, node),
        PseudoClass::FirstOfType => is_first_of_type(doc, node),
        PseudoClass::LastOfType => is_last_of_type(doc, node),
        PseudoClass::OnlyOfType => is_first_of_type(doc, node) && is_last_of_type(doc, node),
        PseudoClass::NthChild(spec, of) => {
            match element_index_filtered(doc, node, false, of.as_deref()) {
                Some(i) => spec.matches(i),
                None => false,
            }
        }
        PseudoClass::NthLastChild(spec, of) => {
            match element_index_filtered(doc, node, true, of.as_deref()) {
                Some(i) => spec.matches(i),
                None => false,
            }
        }
        PseudoClass::NthOfType(spec) => match element_index_of_type(doc, node, false) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthLastOfType(spec) => match element_index_of_type(doc, node, true) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::Not(list) => {
            // CSS Selectors L4 §5.4: матчит, если ни один селектор из списка
            // элементу не подходит. Внутри допустимы complex-селекторы и
            // nested `:not` — рекурсия идёт через `matches_complex`.
            !list.iter().any(|s| matches_complex(s, doc, node))
        }
        PseudoClass::Is(list) | PseudoClass::Where(list) => {
            // CSS4 §17: матчит, если матчит хоть один селектор из списка.
            // `:where(...)` отличается только тем, что contributes 0 specificity —
            // matching identical с `:is`.
            list.iter().any(|s| matches_complex(s, doc, node))
        }
        PseudoClass::Has(list) => {
            // CSS Selectors L4 §17.2: матчит элемент E, если хоть один из
            // relative selectors удовлетворён каким-то элементом в его
            // поддереве (для combinator None или Child) или sibling-цепочке
            // (для NextSibling / LaterSibling). Внутри matches_complex —
            // тот же recursive matcher с back-tracking, относительно
            // кандидата (а не E); кандидаты ищутся согласно combinator-у.
            list.iter().any(|rs| matches_relative(rs, doc, node))
        }
        PseudoClass::PlaceholderShown => matches_placeholder_shown(doc, node),
        PseudoClass::Required => matches_required(doc, node, true),
        PseudoClass::Optional => matches_required(doc, node, false),
        PseudoClass::ReadOnly => matches_read_only(doc, node),
        PseudoClass::ReadWrite => matches_read_write(doc, node),
        PseudoClass::Disabled => matches_disabled(doc, node, true),
        PseudoClass::Enabled => matches_disabled(doc, node, false),
        PseudoClass::Checked => matches_checked(doc, node),
        PseudoClass::Indeterminate => matches_indeterminate(doc, node),
        PseudoClass::Default => matches_default(doc, node),
        PseudoClass::Lang(tags) => matches_lang(doc, node, tags),
        PseudoClass::Dir(arg) => matches_dir(doc, node, *arg),
        PseudoClass::Link => matches_any_link(doc, node),
        // CSS Selectors L4 §6.2.3: `:visited` требует history-runtime
        // (`lumen-storage::History` + safe-history-API с privacy-ограничениями).
        // Phase 0 без runtime — всегда false; никакая ссылка не считается
        // посещённой. Это безопасный default (соответствует privacy-by-default
        // принципу проекта №1: ничего не утекает через стилизацию).
        PseudoClass::Visited => false,
        PseudoClass::AnyLink => matches_any_link(doc, node),
        // CSS Selectors L4 §4.2: `:scope` matches the document's root element
        // в author-CSS context (без runtime querySelector). Эквивалент `:root`.
        // Реальная разница появится при integration с DOM querySelector API
        // (P3 + JS-runtime) — пока что в layout-cascade оба ведут себя
        // одинаково.
        PseudoClass::Scope => is_root_element(doc, node),
        // CSS Selectors L4 §9.6: `:target` matches element с id равным
        // URL fragment-у (case-sensitive — HTML LS §3.2.6 делает `id`
        // case-sensitive, поэтому matcher не lowercase'ит). Без fragment-а
        // (`Document::target() == None`) — никакой element не матчит.
        // Phase 0: значение target_id выставляет shell-интеграция (P3) при
        // навигации; до её появления matcher всегда возвращает false.
        PseudoClass::Target => matches_target(doc, node),
        // CSS Selectors L4 §9.7: `:target-within` — element сам :target или
        // у него в поддереве есть :target-element. Short-circuit при
        // `Document::target() == None` — на странице без fragment-а никто
        // не матчит, walk поддерева не нужен.
        PseudoClass::TargetWithin => matches_target_within(doc, node),
        // CSS Selectors L4 §6.4.1, HTML LS §4.13.5 — `:defined` матчит
        // built-in HTML/SVG/MathML элементы и зарегистрированные custom
        // elements. Custom-element-имена по HTML LS §4.13.2 обязаны иметь
        // ASCII `-`; без registry в Phase 0 matcher использует это правило
        // как аппроксимацию: имя без `-` → built-in (defined); имя с `-` →
        // un-registered custom element (undefined). Когда P3 поднимет
        // registry, проверка станет `built-in || registry.has(name)`.
        PseudoClass::Defined => matches_defined(doc, node),
        // Fullscreen API §4.2 `:fullscreen` — runtime-only: top-layer
        // элементов, поднятых через `Element.requestFullscreen()`. JS API
        // реализован (p1-fullscreen-api); sentinel — `data-lumen-fullscreen`.
        // CSS: :fullscreen — P4: check doc.get_attr(node.id,"data-lumen-fullscreen").is_some()
        PseudoClass::Fullscreen => doc.get(node).get_attr("data-lumen-fullscreen").is_some(),
        // CSS Selectors L4 §16.5.2 `:modal` — `<dialog>` opened via
        // `showModal()`. JS sets `data-lumen-modal` sentinel; `show()` / author
        // attribute do not set it, so non-modal dialogs stay unmatched.
        PseudoClass::Modal => doc.get(node).get_attr("data-lumen-modal").is_some(),
        // HTML LS §6.12.2 `:popover-open` — popover в открытом состоянии
        // после `element.showPopover()` / клика по `popovertarget`.
        // Runtime-only: атрибут `popover` декларирует тип, но не открытое
        // состояние. Phase 0 без Popover API runtime — всегда `false`.
        PseudoClass::PopoverOpen => doc.get(node).get_attr("data-lumen-popover-open").is_some(),
        // CSS Selectors L4 §17.4 `:state(name)` — WHATWG HTML §4.13.2
        // `ElementInternals.states` (`CustomStateSet`). Runtime-only, same
        // sentinel-attribute pattern as `:fullscreen`/`:modal`: the JS shim
        // (`CustomStateSet.add`/`delete`/`clear`) reflects each active state
        // into a `data-lumen-state-<name>` attribute on the host element via
        // `_lumen_set_attr`/`_lumen_remove_attr` — layout never calls into
        // the JS engine during matching.
        PseudoClass::State(name) => doc
            .get(node)
            .get_attr(&format!("data-lumen-state-{name}"))
            .is_some(),
        // CSS Selectors L4 §11.4 time-dimensional pseudo-classes —
        // `:current` / `:past` / `:future` matches на active / elapsed /
        // upcoming моменты в timed-text потоке (WebVTT cue rendering при
        // воспроизведении видео/аудио). Runtime-only: нужна синхронизация с
        // media timeline и cue lifecycle. Phase 0 без timed-text runtime
        // все три всегда `false`.
        PseudoClass::Current => false,
        PseudoClass::Past => false,
        PseudoClass::Future => false,
        PseudoClass::InRange => matches_in_range(doc, node) == Some(true),
        PseudoClass::OutOfRange => matches_in_range(doc, node) == Some(false),
        PseudoClass::Valid => form_validity(doc, node) == Some(true),
        PseudoClass::Invalid => form_validity(doc, node) == Some(false),
        // Phase 0: без интерактивного состояния пользователя — всегда false.
        PseudoClass::UserValid | PseudoClass::UserInvalid => false,
        // ── Interactive pseudo-classes ────────────────────────────────────────────
        // State is set thread-locally by `set_interactive_state` before layout.
        // `:hover` — element under pointer, or its ancestors (CSS Selectors L4 §4.3).
        PseudoClass::Hover => {
            let hid = HOVER_NID.with(Cell::get);
            if hid == u32::MAX { return false; }
            is_self_or_ancestor(doc, node, NodeId::from_index(hid as usize))
        }
        // `:focus` — exact keyboard-focused element (no ancestor propagation).
        PseudoClass::Focus => {
            FOCUS_NID.with(Cell::get) == node.index() as u32
        }
        // `:active` — mouse-pressed element and its ancestors (CSS Selectors L4 §4.5).
        PseudoClass::Active => {
            let aid = ACTIVE_NID.with(Cell::get);
            if aid == u32::MAX { return false; }
            is_self_or_ancestor(doc, node, NodeId::from_index(aid as usize))
        }
        // `:focus-within` — element or any descendant has focus (CSS Selectors L4 §4.4.2).
        PseudoClass::FocusWithin => {
            let fid = FOCUS_NID.with(Cell::get);
            if fid == u32::MAX { return false; }
            is_self_or_ancestor(doc, node, NodeId::from_index(fid as usize))
        }
        // `:focus-visible` — Phase 0: identical to `:focus` (no keyboard-vs-mouse distinction yet).
        PseudoClass::FocusVisible => {
            FOCUS_NID.with(Cell::get) == node.index() as u32
        }
        PseudoClass::Unsupported(_) => false,
        // CSS Scoping L1 §6.1: `:host` matches the shadow host element, but ONLY
        // from within that host's own shadow-tree stylesheet. We model scope with
        // the `SHADOW_HOST_SCOPE` thread-local: it equals the host index only while
        // the shadow sheet of `node` is being matched. In document scope (MAX) or
        // when matching a *different* host's shadow sheet, `:host` never matches —
        // so a `:host` rule in the page's own `<style>` is a no-op (spec-correct).
        PseudoClass::Host(opt_list) => {
            if SHADOW_HOST_SCOPE.with(Cell::get) != node.index() as u32 {
                return false;
            }
            if !doc.is_shadow_host(node) {
                return false;
            }
            match opt_list {
                None => true,
                Some(list) => list.iter().any(|s| matches_complex(s, doc, node)),
            }
        }
    }
}

/// `:defined` matcher per CSS Selectors L4 §6.4.1 / HTML LS §4.13.5.
///
/// Текстовые / комментарные ноды псевдо-классам не подвергаются вообще
/// (Selector L4 §3.1 «selectors only apply to elements»), но selector
/// engine приходит сюда только для элементов — на всякий случай делаем
/// fast-fail на не-элемент.
fn matches_defined(doc: &Document, node: NodeId) -> bool {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return false;
    };
    // HTML LS §4.13.2 «Valid custom element name»: имя custom-element-а
    // обязано содержать дефис. Это единственная синтаксическая разница
    // между «built-in» и «custom». В Phase 0 без CustomElementRegistry
    // считаем все built-in defined, все custom-имена — undefined.
    !name.local.as_str().contains('-')
}

/// Default-значение `<input type>` — `text` (HTML5 §4.10.5.1.2). Возвращает
/// lower-case значение `type`-атрибута; пустая строка трактуется как `text`.
fn input_type_lower(doc: &Document, node: NodeId) -> String {
    let node_ref = doc.get(node);
    node_ref
        .get_attr("type")
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "text".to_string())
}

/// `<input>`-типы, к которым применимы `:read-only` / `:read-write` per HTML5
/// §4.16.4 «mutable input» — text-like (введение текста).
fn input_is_text_like(input_type: &str) -> bool {
    matches!(
        input_type,
        "text"
            | "search"
            | "url"
            | "tel"
            | "email"
            | "password"
            | "number"
            | "date"
            | "month"
            | "week"
            | "time"
            | "datetime-local"
    )
}

/// `<input>`-типы, к которым применим `required` per HTML5 §4.10.3 — text-like
/// + `checkbox` / `radio` / `file`.
fn input_supports_required(input_type: &str) -> bool {
    input_is_text_like(input_type)
        || matches!(input_type, "checkbox" | "radio" | "file")
}

/// CSS Selectors L4 §15.4 / HTML5 §4.10.3 `:required` / `:optional`.
/// `want_required = true` → `:required`, иначе `:optional`. Возвращает true
/// только для form control-ов, к которым применим атрибут `required`.
///
/// Применимо: `<select>`, `<textarea>`, и `<input>` text-like / checkbox /
/// radio / file. Прочие элементы (`<input type=hidden>`, `<button>`, `<div>`)
/// не матчатся ни одним из двух.
fn matches_required(doc: &Document, node: NodeId, want_required: bool) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    let applies = match tag {
        "select" | "textarea" => true,
        "input" => input_supports_required(&input_type_lower(doc, node)),
        _ => false,
    };
    if !applies {
        return false;
    }
    let has_required = node_ref.get_attr("required").is_some();
    has_required == want_required
}

/// CSS Selectors L4 §15.5 / HTML5 §4.16.4 `:read-write` — «mutable» form
/// control или `contenteditable`-элемент.
///
/// True для:
///   - `<input>` text-like type БЕЗ `readonly` и БЕЗ `disabled`;
///   - `<textarea>` БЕЗ `readonly` и БЕЗ `disabled`;
///   - любого элемента с эффективным `contenteditable="true"` (включая
///     наследование от ancestor — `contenteditable=""` тоже считается true).
///
/// Прочие элементы — false (и матчат `:read-only`).
fn matches_read_write(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    let is_form_mutable = match tag {
        "input" => {
            input_is_text_like(&input_type_lower(doc, node))
                && node_ref.get_attr("readonly").is_none()
                && node_ref.get_attr("disabled").is_none()
        }
        "textarea" => {
            node_ref.get_attr("readonly").is_none()
                && node_ref.get_attr("disabled").is_none()
        }
        _ => false,
    };
    if is_form_mutable {
        return true;
    }
    is_effectively_contenteditable(doc, node)
}

/// CSS Selectors L4 §15.5 / HTML5 §4.16.4 `:read-only` — «not mutable».
///
/// Per spec: «matches all other HTML elements» — то есть все Element-ы, не
/// попадающие под `:read-write`. Не Element-ы (Text / Comment / Document) не
/// матчатся ничем.
fn matches_read_only(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    if !matches!(node_ref.data, NodeData::Element { .. }) {
        return false;
    }
    !matches_read_write(doc, node)
}

/// Эффективное значение `contenteditable` с наследованием от ancestor-ов.
/// `contenteditable="true"` или `contenteditable=""` (пустая строка) → true;
/// `contenteditable="false"` → false (и обрывает наследование); отсутствие
/// атрибута на узле — смотрим выше.
fn is_effectively_contenteditable(doc: &Document, node: NodeId) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        let node_ref = doc.get(n);
        if let NodeData::Element { .. } = node_ref.data
            && let Some(v) = node_ref.get_attr("contenteditable")
        {
            let lower = v.trim().to_ascii_lowercase();
            if lower.is_empty() || lower == "true" {
                return true;
            }
            if lower == "false" {
                return false;
            }
        }
        cur = node_ref.parent;
    }
    false
}

/// HTML5 §4.10.19.2 «can be disabled»-элементы — `<button>`, `<input>`,
/// `<select>`, `<textarea>`, `<optgroup>`, `<option>`, `<fieldset>`.
fn is_disableable_form_control(tag: &str) -> bool {
    matches!(
        tag,
        "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
    )
}

/// CSS Selectors L4 §14.2 / HTML5 §4.10.19.2 `:disabled` / `:enabled`.
/// `want_disabled = true` → `:disabled`, иначе `:enabled`.
///
/// Элемент считается disabled, если:
///   - применим к `:disabled` per `is_disableable_form_control` И;
///   - либо у него самого есть атрибут `disabled`;
///   - либо у `<option>` ancestor-`<optgroup>` имеет `disabled` (HTML5 §4.10.10);
///   - либо элемент находится внутри `<fieldset disabled>` И НЕ внутри
///     первого `<legend>`-ребёнка этого fieldset (HTML5 §4.10.16).
///     `<fieldset>` сам disabled только по собственному атрибуту, не от
///     ancestor-fieldset.
///
/// Прочие элементы (`<div>`, `<p>`, и т.д.) — не матчат ни `:disabled`, ни
/// `:enabled`.
fn matches_disabled(doc: &Document, node: NodeId, want_disabled: bool) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if !is_disableable_form_control(tag) {
        return false;
    }
    let actually_disabled = is_actually_disabled(doc, node, tag);
    actually_disabled == want_disabled
}

fn is_actually_disabled(doc: &Document, node: NodeId, tag: &str) -> bool {
    let node_ref = doc.get(node);
    if node_ref.get_attr("disabled").is_some() {
        return true;
    }
    // `<option>` наследует disabled от непосредственного `<optgroup>`-родителя
    // (HTML5 §4.10.10): «An option element is disabled if its disabled attribute
    // is set or if it is a child of an optgroup element whose disabled attribute
    // is set».
    if tag == "option"
        && let Some(p) = node_ref.parent
    {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "optgroup"
            && p_ref.get_attr("disabled").is_some()
        {
            return true;
        }
    }
    // `<fieldset>` сам disabled только по собственному атрибуту; ancestor-walk
    // для него не нужен.
    if tag == "fieldset" {
        return false;
    }
    // Form control внутри `<fieldset disabled>` — disabled, кроме случая, когда
    // он лежит в первом `<legend>`-ребёнке этого fieldset (HTML5 §4.10.16).
    let mut child = node;
    let mut cur = node_ref.parent;
    while let Some(p) = cur {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "fieldset"
            && p_ref.get_attr("disabled").is_some()
            && !is_descendant_of_first_legend_child(doc, p, child)
        {
            return true;
        }
        child = p;
        cur = p_ref.parent;
    }
    false
}

/// True, если `descendant_chain_start` — это сам first-`<legend>`-ребёнок
/// `fieldset` или лежит в его поддереве. Для проверки достаточно посмотреть на
/// `child` — тот узел, через которого мы дошли до fieldset; если он же —
/// первый element-child `<legend>`, то вся ветка живёт под legend.
fn is_descendant_of_first_legend_child(
    doc: &Document,
    fieldset: NodeId,
    child_on_path: NodeId,
) -> bool {
    let first_legend = doc
        .get(fieldset)
        .children
        .iter()
        .copied()
        .find(|&c| is_element(doc, c))
        .filter(|&c| {
            let c_ref = doc.get(c);
            matches!(&c_ref.data, NodeData::Element { name, .. } if name.local.as_str() == "legend")
        });
    matches!(first_legend, Some(l) if l == child_on_path)
}

/// CSS Selectors L4 §15.1 `:placeholder-shown` — true для form-control,
/// у которого есть непустой `placeholder`-атрибут И пустое текущее значение.
///
/// Текущее значение берётся из [`Document::control_value`]: набранный текст и
/// присвоенный скриптом `el.value` прячут placeholder ровно так же, как
/// author-объявленный `value`-атрибут (BUG-441).
fn matches_placeholder_shown(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if tag != "input" && tag != "textarea" {
        return false;
    }
    let Some(placeholder) = node_ref.get_attr("placeholder") else {
        return false;
    };
    if placeholder.trim().is_empty() {
        return false;
    }
    // Непустое текущее значение → контент уже задан, placeholder скрыт.
    // Набранное/присвоенное значение перекрывает дефолт целиком: пустой dirty
    // value возвращает placeholder даже у `<textarea>` с author-текстом.
    if let Some(dirty) = doc.dirty_value(node) {
        return dirty.is_empty();
    }
    // Дефолтная ветка — прежнее правило: у `<input>` это `value`-атрибут, у
    // `<textarea>` — текстовые дети (whitespace-only контентом не считается).
    if tag == "textarea" {
        return !has_non_whitespace_text(doc, node);
    }
    node_ref.get_attr("value").unwrap_or("").is_empty()
}

/// `:checked` (CSS Selectors L4 §10.1). Pure attribute-based matcher без
/// runtime form-state:
/// - `<input type=checkbox|radio>` с атрибутом `checked` (значение атрибута
///   не имеет значения — спецификация трактует наличие как true);
/// - `<option>` с атрибутом `selected`.
///
/// Динамически переключённый через клик/JS checkbox не отражается в
/// DOM-атрибутах и здесь не учитывается — Phase 0 без form-state runtime.
fn matches_checked(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    match name.local.as_str() {
        "input" => {
            let t = input_type_lower(doc, node);
            if t != "checkbox" && t != "radio" {
                return false;
            }
            node_ref.get_attr("checked").is_some()
        }
        "option" => node_ref.get_attr("selected").is_some(),
        _ => false,
    }
}

/// `:indeterminate` (CSS Selectors L4 §10.2, HTML5 §4.16.3 + §4.10.18.4).
/// Применяется к:
/// - `<input type=checkbox>` с DOM-флагом indeterminate (Phase 0: всегда
///   `false` — флаг существует только через JS `.indeterminate = true`,
///   которого пока нет);
/// - `<input type=radio>` в группе (одинаковый `name` внутри ближайшей
///   form-owner-области) без ни одного checked-радио. Если радио без `name`,
///   группа = только сам элемент — тогда indeterminate ≡ нет `checked`;
/// - `<progress>` без атрибута `value` (indeterminate progress per HTML5).
fn matches_indeterminate(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    match name.local.as_str() {
        "input" => {
            let t = input_type_lower(doc, node);
            if t == "radio" {
                // Найти ближайший <form>-предок; если нет — корень документа.
                let scope = nearest_form_or_root(doc, node);
                let radio_name = node_ref.get_attr("name").map(|s| s.to_string());
                !any_descendant(doc, scope, |n| {
                    if !is_element(doc, n) {
                        return false;
                    }
                    let other = doc.get(n);
                    let NodeData::Element { name: n2, .. } = &other.data else {
                        return false;
                    };
                    if n2.local.as_str() != "input" {
                        return false;
                    }
                    let t2 = input_type_lower(doc, n);
                    if t2 != "radio" {
                        return false;
                    }
                    // Радио считается членом той же группы если name совпадает
                    // (или оба отсутствуют — узкая группа из одного элемента).
                    let n2_name = other.get_attr("name").map(|s| s.to_string());
                    if n2_name != radio_name {
                        return false;
                    }
                    other.get_attr("checked").is_some()
                })
            } else {
                // Phase 0: checkbox indeterminate выставляется только через
                // JS — DOM не выражает этого. Всегда false.
                false
            }
        }
        "progress" => node_ref.get_attr("value").is_none(),
        _ => false,
    }
}

/// `:default` (CSS Selectors L4 §10.4, HTML5 §4.16.3) — «по-умолчанию
/// активный» form control:
/// - `<option>` с атрибутом `selected`;
/// - checkbox/radio с атрибутом `checked`;
/// - default submit-button формы — первая в DOM-порядке формы
///   `<button type=submit>` / `<input type=submit|image>`. `type=submit` —
///   default для `<button>` (HTML5 §4.10.8) и для `<input>` без `type` это
///   `text`, поэтому submit-button обязан иметь `type=submit`.
fn matches_default(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    match tag {
        "option" => node_ref.get_attr("selected").is_some(),
        "input" => {
            let t = input_type_lower(doc, node);
            if (t == "checkbox" || t == "radio") && node_ref.get_attr("checked").is_some() {
                return true;
            }
            if t == "submit" || t == "image" {
                return is_default_submit_button(doc, node);
            }
            false
        }
        "button" => {
            // default-type для <button> = submit (HTML5 §4.10.8).
            let t = node_ref
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            if t != "submit" {
                return false;
            }
            is_default_submit_button(doc, node)
        }
        _ => false,
    }
}

/// Default submit-button формы — первая submit-кнопка в DOM-порядке внутри
/// ближайшего `<form>`-предка (HTML5 §4.10.22.3 «implicit submission»).
/// Если предка `<form>` нет, кнопка не form-owner-связана и не считается
/// default.
fn is_default_submit_button(doc: &Document, node: NodeId) -> bool {
    let Some(form) = nearest_form(doc, node) else {
        return false;
    };
    let mut found: Option<NodeId> = None;
    walk_first_submit(doc, form, &mut found);
    found == Some(node)
}

/// Pre-order обход поддерева form в поиске первой submit-кнопки. Сохраняет
/// результат в `found` и останавливается раньше через короткое замыкание
/// `is_some()` на ранних уровнях.
fn walk_first_submit(doc: &Document, scope: NodeId, found: &mut Option<NodeId>) {
    if found.is_some() {
        return;
    }
    for &child in &doc.get(scope).children {
        if found.is_some() {
            return;
        }
        if !is_element(doc, child) {
            continue;
        }
        let NodeData::Element { name, .. } = &doc.get(child).data else {
            continue;
        };
        let tag = name.local.as_str();
        if tag == "input" {
            let t = input_type_lower(doc, child);
            if t == "submit" || t == "image" {
                *found = Some(child);
                return;
            }
        } else if tag == "button" {
            let t = doc
                .get(child)
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            if t == "submit" {
                *found = Some(child);
                return;
            }
        }
        walk_first_submit(doc, child, found);
    }
}

/// Ближайший `<form>`-предок (или сам node, если он `<form>`). None — нет.
fn nearest_form(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { name, .. } = &doc.get(n).data
            && name.local.as_str() == "form"
        {
            return Some(n);
        }
        cur = doc.get(n).parent;
    }
    None
}

/// Ближайший `<form>`-предок или корень документа — scope-для-обхода
/// radio-группы. Возвращает корень документа если предка `<form>` нет.
fn nearest_form_or_root(doc: &Document, node: NodeId) -> NodeId {
    nearest_form(doc, node).unwrap_or_else(|| doc.root())
}

/// `:lang(<tag>#)` (CSS Selectors L4 §11). Элемент матчит, если его
/// content-language matches хотя бы один из tag-ов в списке по RFC 4647
/// §3.3.1 «basic filtering»: range matches tag, если range — exact equal
/// или range — proper prefix tag с границей по `-`. То есть `:lang(en)`
/// matches `lang="en"`, `lang="en-US"`, `lang="en-Latn-GB"`, но не
/// `lang="english"` и не `lang="fr-en"` (последний — `fr` + `en` — `en`
/// здесь регион/вариант, не language).
///
/// Content-language определяется через ближайший `lang` или `xml:lang`
/// атрибут вверх по дереву (HTML5 §3.2.6 «inheritance»; xml:lang —
/// исторически из XHTML, до сих пор используется в реальных страницах).
/// Если ни один ancestor не имеет `lang`, элемент не имеет языка и не
/// матчит ни один tag — кроме пустого `*` (Selectors L4 расширение пока
/// не поддерживается).
fn matches_lang(doc: &Document, node: NodeId, tags: &[String]) -> bool {
    let Some(content_lang) = element_lang(doc, node) else {
        return false;
    };
    let content_lc = content_lang.to_ascii_lowercase();
    tags.iter().any(|range| lang_range_matches(range, &content_lc))
}

/// Определяет content-language элемента, walking up ancestors. Сначала
/// `lang`, потом `xml:lang` на том же узле; затем родитель, и так далее.
/// Возвращает None если ни у кого нет атрибута либо найденное значение —
/// пустая строка (HTML5: `lang=""` — «явно неизвестен», не наследует от
/// предков — Phase 0 трактует как «нет языка»).
fn element_lang(doc: &Document, node: NodeId) -> Option<String> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { .. } = &doc.get(n).data {
            let nr = doc.get(n);
            if let Some(v) = nr.get_attr("lang") {
                return if v.is_empty() { None } else { Some(v.to_string()) };
            }
            if let Some(v) = nr.get_attr("xml:lang") {
                return if v.is_empty() { None } else { Some(v.to_string()) };
            }
        }
        cur = doc.get(n).parent;
    }
    None
}

/// RFC 4647 §3.3.1 «basic filtering»: language range matches language tag,
/// если range — case-insensitive prefix tag с границей по `-` или концом
/// строки. Обе стороны уже ожидаются в lowercase.
fn lang_range_matches(range_lc: &str, tag_lc: &str) -> bool {
    if range_lc == tag_lc {
        return true;
    }
    if let Some(rest) = tag_lc.strip_prefix(range_lc) {
        return rest.starts_with('-');
    }
    false
}

/// `:any-link` / `:link` (CSS Selectors L4 §6.2.1 / §6.2.2, HTML5 §4.6).
/// Hyperlinks в HTML: `<a>`, `<area>`, `<link>` элементы с **непустым**
/// `href`-атрибутом (HTML5 §4.6.1 — hyperlink требует non-empty href; пустой
/// href трактуется как ссылка на текущий документ и формально валиден, но
/// все mainstream браузеры считают такой элемент hyperlink-ом — мы тоже).
/// Spec различает hyperlink (`href` присутствует) от non-hyperlink (no href),
/// последний не матчит ни `:link`, ни `:visited`, ни `:any-link`.
fn matches_any_link(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if !matches!(tag, "a" | "area" | "link") {
        return false;
    }
    node_ref.get_attr("href").is_some()
}

/// `:target` matcher (CSS Selectors L4 §9.6). Возвращает true, если у элемента
/// есть `id`-атрибут, равный текущему `Document::target()` (URL fragment без
/// `:in-range` / `:out-of-range` (CSS Selectors L4 §14.5, HTML5 §4.10.21.4).
///
/// Возвращает `Some(true)` если value в [min, max], `Some(false)` если вне,
/// `None` если у элемента нет range-limitations или нет displayed value.
/// Phase 0: поддерживаются только `type=number` и `type=range`.
fn matches_in_range(doc: &Document, node: NodeId) -> Option<bool> {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return None;
    };
    if name.local.as_str() != "input" {
        return None;
    }
    let t = input_type_lower(doc, node);
    let supports_numeric = matches!(t.as_str(), "number" | "range");
    if !supports_numeric {
        return None;
    }

    let min_attr = node_ref.get_attr("min").and_then(parse_html_number);
    let max_attr = node_ref.get_attr("max").and_then(parse_html_number);

    let (min, max) = match t.as_str() {
        "range" => (min_attr.unwrap_or(0.0), max_attr.unwrap_or(100.0)),
        _ => {
            if min_attr.is_none() && max_attr.is_none() {
                return None;
            }
            (min_attr.unwrap_or(f64::NEG_INFINITY), max_attr.unwrap_or(f64::INFINITY))
        }
    };

    // Текущее значение контрола, а не `value`-атрибут: набранное/присвоенное
    // число решает, попадает ли поле в диапазон (BUG-441).
    let value = match parse_html_number(doc.control_value(node).as_ref()) {
        Some(v) => v,
        None => {
            if t == "range" {
                // Spec §4.10.5.1.13: default value = min + (max-min)/2, clamped.
                let mid = min + (max - min) / 2.0;
                mid.clamp(min, max)
            } else {
                return None;
            }
        }
    };

    Some(value >= min && value <= max)
}

/// `:valid` / `:invalid` (CSS Selectors L4 §14.1, HTML5 §4.10.21).
///
/// Делегирует в `lumen_dom::element_validity` — единый источник истины для
/// constraint validation. `None` — элемент не является кандидатом.
fn form_validity(doc: &Document, node: NodeId) -> Option<bool> {
    lumen_dom::element_validity(doc, node).map(|vs| vs.valid())
}

/// Парсит HTML5 «valid floating-point number» (§2.5.5).
/// Отбрасывает leading `+`, NaN и ±∞ (не допускаются spec-ом).
fn parse_html_number(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.starts_with('+') {
        return None;
    }
    let v: f64 = trimmed.parse().ok()?;
    if v.is_finite() { Some(v) } else { None }
}

/// `#`). Comparison case-sensitive — HTML id case-sensitive per HTML LS §3.2.6.
/// Текстовые узлы и не-element-узлы не матчат.
fn matches_target(doc: &Document, node: NodeId) -> bool {
    let Some(target) = doc.target() else {
        return false;
    };
    let node_ref = doc.get(node);
    if !matches!(&node_ref.data, NodeData::Element { .. }) {
        return false;
    }
    node_ref.get_attr("id") == Some(target)
}

/// `:target-within` matcher (CSS Selectors L4 §9.7). Element matches if it
/// itself is `:target`, OR has any descendant element matching `:target`.
/// Short-circuits на `Document::target() == None` (нет fragment-а — никто не
/// матчит, сэкономим обход поддерева).
fn matches_target_within(doc: &Document, node: NodeId) -> bool {
    let Some(target) = doc.target() else {
        return false;
    };
    if !is_element(doc, node) {
        return false;
    }
    if doc.get(node).get_attr("id") == Some(target) {
        return true;
    }
    any_descendant(doc, node, |n| doc.get(n).get_attr("id") == Some(target))
}

/// `:dir(ltr|rtl)` (CSS Selectors L4 §13.2). Матчит элемент с
/// соответствующей directionality, определяемой через `dir`-атрибут
/// (с inherited fallback от ближайшего ancestor-а). При отсутствии
/// `dir` нигде в цепочке — default `ltr` (HTML5 §3.2.6.1).
fn matches_dir(doc: &Document, node: NodeId, want: DirArg) -> bool {
    element_directionality(doc, node) == want
}

/// Computes content-directionality элемента по HTML5 §3.2.6.1
/// «directionality»: значение `dir`-атрибута самого элемента, либо
/// унаследовано от ближайшего ancestor с `dir`-атрибутом. Default `ltr`.
///
/// Phase 0 не реализует real auto-direction (UAX #9 first-strong scan по
/// текстовому содержимому для `<bdi>` и `dir="auto"`) — оба трактуются
/// как `ltr`, что соответствует поведению типичных страниц на латинице.
/// Real bidi откладывается до layout-bidi движка (см. lumen-layout `Отложено`).
fn element_directionality(doc: &Document, node: NodeId) -> DirArg {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { .. } = &doc.get(n).data
            && let Some(v) = doc.get(n).get_attr("dir")
        {
            return match v.trim().to_ascii_lowercase().as_str() {
                "ltr" => DirArg::Ltr,
                "rtl" => DirArg::Rtl,
                // `auto` и любое другое значение — Phase 0 fallback to ltr;
                // продолжаем walking up НЕ нужно: spec говорит, что
                // `dir` атрибут на самом элементе финализирует
                // directionality (`auto` тоже считается «явным»).
                _ => DirArg::Ltr,
            };
        }
        cur = doc.get(n).parent;
    }
    DirArg::Ltr
}

/// Проверка: у узла есть хоть один text-ребёнок с непустым содержимым
/// (после whitespace-trim). Нужно для `<textarea>` чьё «значение» — это
/// его текстовый контент в DOM (HTML5 §4.10.11), а не `value`-атрибут.
fn has_non_whitespace_text(doc: &Document, node: NodeId) -> bool {
    for &child in &doc.get(node).children {
        if let NodeData::Text(t) = &doc.get(child).data
            && !t.trim().is_empty()
        {
            return true;
        }
    }
    false
}

/// Проверяет, что хоть один кандидат относительно `scope` (в зависимости от
/// combinator-а) удовлетворяет внутреннему selector-у.
fn matches_relative(rs: &lumen_css_parser::RelativeSelector, doc: &Document, scope: NodeId) -> bool {
    match rs.combinator {
        // Implicit descendant — обходим всё поддерево scope.
        None => any_descendant(doc, scope, |n| matches_complex(&rs.selector, doc, n)),
        Some(Combinator::Child) => {
            // Прямые element-children scope.
            doc.get(scope).children.iter().any(|&c| {
                is_element(doc, c) && matches_complex(&rs.selector, doc, c)
            })
        }
        Some(Combinator::NextSibling) => {
            // Прямой следующий element-sibling.
            next_element_sibling(doc, scope)
                .map(|n| matches_complex(&rs.selector, doc, n))
                .unwrap_or(false)
        }
        Some(Combinator::LaterSibling) => {
            // Любой последующий element-sibling.
            let mut cur = next_element_sibling(doc, scope);
            while let Some(n) = cur {
                if matches_complex(&rs.selector, doc, n) {
                    return true;
                }
                cur = next_element_sibling(doc, n);
            }
            false
        }
        // Descendant как explicit combinator — то же что None.
        Some(Combinator::Descendant) => {
            any_descendant(doc, scope, |n| matches_complex(&rs.selector, doc, n))
        }
    }
}

/// True если хоть один element-descendant `root` удовлетворяет `pred`. Сам
/// `root` не проверяется — только потомки (по spec :has() ищет среди
/// descendants, не включая E).
fn any_descendant<F: Fn(NodeId) -> bool>(doc: &Document, root: NodeId, pred: F) -> bool {
    fn walk<F: Fn(NodeId) -> bool>(doc: &Document, n: NodeId, pred: &F) -> bool {
        for &c in &doc.get(n).children {
            if is_element(doc, c) && pred(c) {
                return true;
            }
            if walk(doc, c, pred) {
                return true;
            }
        }
        false
    }
    walk(doc, root, &pred)
}

fn next_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let idx = siblings.iter().position(|&id| id == node)?;
    siblings[idx + 1..].iter().copied().find(|&id| is_element(doc, id))
}

/// 1-based индекс элемента среди element-sibling-ов. Если `from_end` —
/// считаем с конца. None — если узел не элемент или нет родителя.
fn element_index(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    if !is_element(doc, node) {
        return None;
    }
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        if !is_element(doc, id) {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

/// 1-based индекс элемента среди sibling-ов, удовлетворяющих опциональному
/// `of <selector-list>` фильтру (CSS Selectors L4 §6.6.5.1). При `of=None`
/// эквивалент `element_index` (все element-sibling-ы). При `of=Some(list)`:
/// сначала проверяем, что сам узел матчит хотя бы один из селекторов
/// списка — иначе `:nth-child(... of S)` не применим, возвращаем None;
/// затем считаем index среди siblings, удовлетворяющих тому же list-у.
fn element_index_filtered(
    doc: &Document,
    node: NodeId,
    from_end: bool,
    of: Option<&[ComplexSelector]>,
) -> Option<i32> {
    let Some(list) = of else {
        return element_index(doc, node, from_end);
    };
    if !is_element(doc, node) {
        return None;
    }
    // Сам элемент должен матчить хотя бы один селектор list-а — иначе
    // `:nth-child(an+b of S)` к нему вообще не применяется.
    if !list.iter().any(|s| matches_complex(s, doc, node)) {
        return None;
    }
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        if !is_element(doc, id) {
            continue;
        }
        if !list.iter().any(|s| matches_complex(s, doc, id)) {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

/// 1-based индекс элемента среди sibling-ов **того же тега**.
fn element_index_of_type(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    let self_name = match &doc.get(node).data {
        NodeData::Element { name, .. } => name,
        _ => return None,
    };
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        let same_type = matches!(
            &doc.get(id).data,
            NodeData::Element { name, .. } if name == self_name
        );
        if !same_type {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

fn is_first_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, false) == Some(1)
}

fn is_last_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, true) == Some(1)
}

// ──────────────── DOM-traversal хелперы ────────────────

fn is_element(doc: &Document, node: NodeId) -> bool {
    matches!(doc.get(node).data, NodeData::Element { .. })
}

fn previous_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let idx = siblings.iter().position(|&id| id == node)?;
    siblings[..idx]
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
}

fn is_first_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_last_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_empty_element(doc: &Document, node: NodeId) -> bool {
    // `:empty` — нет ни элементов-детей, ни текстовых узлов с непустым контентом.
    doc.get(node).children.iter().all(|&cid| {
        matches!(
            doc.get(cid).data,
            NodeData::Comment(_) | NodeData::Doctype { .. }
        ) || matches!(&doc.get(cid).data, NodeData::Text(t) if t.is_empty())
    })
}

fn is_root_element(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    matches!(doc.get(parent).data, NodeData::Document)
}

// ──────────────── default display / declarations ────────────────

fn default_display(doc: &Document, node: NodeId) -> Display {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return Display::Block;
    };
    match name.local.as_str() {
        // <head> и его метаданные никогда не рендерятся как видимый контент.
        // `<source>` и `<track>` — child-кандидаты `<picture>` / `<video>` /
        // `<audio>`; реальное визуальное представление даёт inner `<img>`
        // (резолвится `pick_picture_source`) или сам media-элемент. Сами
        // эти теги в DOM есть, но layout-бокса не порождают.
        "head" | "title" | "style" | "script" | "meta" | "link" | "base" | "noscript"
        | "source" | "track" => Display::None,
        // Inline-уровневые элементы. Phase 0: пока трактуем как block — текст
        // внутри `<a>`/`<span>` будет на своей строке. Это известное ограничение.
        "a" | "span" | "b" | "i" | "em" | "strong" | "code" | "small" | "sub" | "sup"
        | "label" | "abbr" | "cite" | "q" | "mark" | "u"
        // HTML §15.3.7: <del>, <ins>, <s> — flow content, UA display = inline.
        | "del" | "ins" | "s" => Display::Inline,
        // HTML rendering §15.3.1 — `<img>` is inline-level replaced content, so
        // it shares the line box with the text around it (icon in a button, logo
        // next to a title, avatar in a comment). It never becomes an `InlineRun`
        // segment — a segment has no height of its own (BUG-728) — but
        // `is_atomic_inline_level` picks it up as an atomic inline-level box and
        // it flows inside `InlineBlockRow` beside text and `inline-block`
        // siblings (IFC-2). Author `display:` overrides win through the cascade.
        "img" => Display::Inline,
        // CSS 2.1 table model — UA default display values per HTML spec.
        "table" => Display::Table,
        "caption" => Display::TableCaption,
        "colgroup" => Display::TableColumnGroup,
        "col" => Display::TableColumn,
        "thead" => Display::TableHeaderGroup,
        "tbody" => Display::TableRowGroup,
        "tfoot" => Display::TableFooterGroup,
        "tr" => Display::TableRow,
        "td" | "th" => Display::TableCell,
        // CSS 2.1 — list-item UA default.
        "li" => Display::ListItem,
        // HTML rendering §15.3.1 / §15.5 — form controls are `inline-block`
        // by default, so they flow horizontally with surrounding inline content
        // (text labels, sibling controls) instead of each taking its own line.
        // Author `display:` overrides win through the normal cascade.
        "input" | "button" | "select" | "selectlist" | "textarea" | "meter" | "progress" => {
            Display::InlineBlock
        }
        // HTML rendering §15.5.3 — `<option>` is not rendered in the document
        // flow of a closed `<select>`; the selected label is read straight from
        // the DOM (`collect_select_label`) and painted by the select widget.
        // `display:none` suppresses the painted option text (which otherwise
        // leaks below/over the control) while still generating a (non-painted)
        // box, so `:disabled`/`:checked` selector matching on options keeps
        // working. `<optgroup>` stays in flow (it has no rendered text of its
        // own — only an attribute label — and must recurse so descendant option
        // styles are still computed).
        "option" => Display::None,
        _ => Display::Block,
    }
}

/// HTML5 §14.3.3 — UA white-space for specific elements.
/// Returns `Some` only for elements that override the inherited value.
fn ua_white_space(doc: &Document, node: NodeId) -> Option<WhiteSpace> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        // HTML5 UA stylesheet: pre, listing, xmp, plaintext — white-space: pre
        "pre" | "listing" | "xmp" | "plaintext" => Some(WhiteSpace::Pre),
        // textarea — white-space: pre-wrap (per HTML5 rendering spec)
        "textarea" => Some(WhiteSpace::PreWrap),
        _ => None,
    }
}

/// Разбивает строку на куски по запятым, не пересекая `(...)` (для
/// shadow-list, где цвет может быть `rgba(0, 0, 0, 0.5)` с запятыми).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Парсит одну box-shadow спецификацию. Формат:
/// `[inset]? <length>{2,4} <color>?` — токены произвольно перемешаны.
fn parse_box_shadow_one(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<BoxShadow> {
    // Сложность: цветовые функции (`rgba(...)`) содержат пробелы — наивный
    // split_whitespace их разорвёт. Восстанавливаем токены, балансируя `()`.
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => { depth += 1; buf.push(c); }
            ')' => { depth -= 1; buf.push(c); }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }

    let mut inset = false;
    let mut color: Option<Color> = None;
    let mut lengths: Vec<f32> = Vec::new();

    for tok in tokens {
        if tok.eq_ignore_ascii_case("inset") {
            inset = true;
        } else if let Some(c) = parse_color_legacy(&tok, is_quirks) {
            color = Some(c);
        } else if let Some(len) = parse_length(&tok)
            && let Some(px) = match len {
                Length::Percent(_) => None,
                other => other.resolve(em_basis, None, viewport),
            }
        {
            lengths.push(px);
        }
    }

    // Должно быть 2-4 длины (offset-x, offset-y, blur?, spread?).
    let (offset_x, offset_y, blur, spread) = match lengths.as_slice() {
        [x, y] => (*x, *y, 0.0, 0.0),
        [x, y, b] => (*x, *y, *b, 0.0),
        [x, y, b, sp] => (*x, *y, *b, *sp),
        _ => return None,
    };

    Some(BoxShadow { offset_x, offset_y, blur, spread, color, inset })
}

/// Парсит одну text-shadow спецификацию. Формат:
/// `<length>{2,3} <color>?` (без inset, без spread).
fn parse_text_shadow_one(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<TextShadow> {
    // Тот же tokenization-трюк, что у box-shadow — балансируем `()`,
    // чтобы цветовые функции не разрывались.
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => { depth += 1; buf.push(c); }
            ')' => { depth -= 1; buf.push(c); }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }

    let mut color: Option<Color> = None;
    let mut lengths: Vec<f32> = Vec::new();

    for tok in tokens {
        if let Some(c) = parse_color_legacy(&tok, is_quirks) {
            color = Some(c);
        } else if let Some(len) = parse_length(&tok)
            && let Some(px) = match len {
                Length::Percent(_) => None,
                other => other.resolve(em_basis, None, viewport),
            }
        {
            lengths.push(px);
        }
    }

    let (offset_x, offset_y, blur) = match lengths.as_slice() {
        [x, y] => (*x, *y, 0.0),
        [x, y, b] => (*x, *y, *b),
        _ => return None,
    };

    Some(TextShadow { offset_x, offset_y, blur, color })
}

/// CSS UI L4 §8.1: парсит keyword в `Cursor`. None = неизвестное.
fn parse_cursor_kw(s: &str) -> Option<Cursor> {
    Some(match s {
        "auto" => Cursor::Auto,
        "default" => Cursor::Default,
        "none" => Cursor::None,
        "context-menu" => Cursor::ContextMenu,
        "help" => Cursor::Help,
        "pointer" => Cursor::Pointer,
        "progress" => Cursor::Progress,
        "wait" => Cursor::Wait,
        "cell" => Cursor::Cell,
        "crosshair" => Cursor::Crosshair,
        "text" => Cursor::Text,
        "vertical-text" => Cursor::VerticalText,
        "alias" => Cursor::Alias,
        "copy" => Cursor::Copy,
        "move" => Cursor::Move,
        "no-drop" => Cursor::NoDrop,
        "not-allowed" => Cursor::NotAllowed,
        "grab" => Cursor::Grab,
        "grabbing" => Cursor::Grabbing,
        "all-scroll" => Cursor::AllScroll,
        "col-resize" => Cursor::ColResize,
        "row-resize" => Cursor::RowResize,
        "n-resize" => Cursor::NResize,
        "e-resize" => Cursor::EResize,
        "s-resize" => Cursor::SResize,
        "w-resize" => Cursor::WResize,
        "ne-resize" => Cursor::NeResize,
        "nw-resize" => Cursor::NwResize,
        "se-resize" => Cursor::SeResize,
        "sw-resize" => Cursor::SwResize,
        "ew-resize" => Cursor::EwResize,
        "ns-resize" => Cursor::NsResize,
        "nesw-resize" => Cursor::NeswResize,
        "nwse-resize" => Cursor::NwseResize,
        "zoom-in" => Cursor::ZoomIn,
        "zoom-out" => Cursor::ZoomOut,
        _ => return None,
    })
}

/// CSS Overflow L3: парсит keyword в `Overflow`. None = неизвестное.
fn parse_overflow_kw(s: &str) -> Option<Overflow> {
    match s {
        "visible" => Some(Overflow::Visible),
        "hidden" => Some(Overflow::Hidden),
        "clip" => Some(Overflow::Clip),
        "scroll" => Some(Overflow::Scroll),
        "auto" => Some(Overflow::Auto),
        _ => None,
    }
}

/// Эмулирует UA stylesheet для font-style: HTML §15.3.3 рекомендует italic
/// для `<em>` / `<i>` / `<cite>` / `<dfn>` / `<address>` / `<var>`. Возвращает
/// `Some(Italic)` для них, `None` для остальных (= наследовать как обычно).
fn ua_font_style(doc: &Document, node: NodeId) -> Option<FontStyle> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "em" | "i" | "cite" | "dfn" | "address" | "var" => Some(FontStyle::Italic),
        _ => None,
    }
}

/// Эмулирует UA stylesheet для `font-family`: HTML §15.3.2 задаёт
/// `font-family: monospace` для `<pre>` / `<code>` / `<kbd>` / `<samp>` /
/// `<tt>` (плюс исторические `<listing>` / `<xmp>` / `<plaintext>`,
/// которые уже получают `white-space: pre` рядом).
///
/// Возвращает `Some(["monospace"])` для них, `None` для остальных
/// (= наследовать как обычно). Generic-имя резолвится в конкретный системный
/// шрифт на этапе рендера/измерения (BUG-128); до этого моноширинные элементы
/// рисовались пропорциональным Inter-ом.
fn ua_font_family(doc: &Document, node: NodeId) -> Option<Vec<String>> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "pre" | "code" | "kbd" | "samp" | "tt" | "listing" | "xmp" | "plaintext" => {
            Some(vec!["monospace".to_string()])
        }
        _ => None,
    }
}

/// UA stylesheet: `text-decoration` для семантических HTML-элементов
/// (HTML5 §15.3.7 + §15.3.3).
///
/// - `<del>`, `<s>` → `line-through`
/// - `<ins>`, `<u>` → `underline`
/// - `<a>` (с атрибутом `href`) → `underline`
///
/// Устанавливается ДО CSS-каскада, поэтому любое author-правило перекроет.
/// `<u>` уже в списке inline-элементов — эта функция добавляет ему decoration.
fn apply_ua_text_decoration(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    match name.local.as_str() {
        "del" | "s" => {
            style.text_decoration_line.line_through = true;
        }
        "ins" | "u" => {
            style.text_decoration_line.underline = true;
        }
        "a" if doc.get(node).get_attr("href").is_some() => {
            style.text_decoration_line.underline = true;
        }
        _ => {}
    }
}

/// UA stylesheet: `color` для `<a href="…">`.
/// HTML5 §15.3.3: unvisited links → `color: -webkit-link` (обычно #0000ee).
/// Phase 0 не поддерживает `:visited` — все `<a>` получают link-color.
/// Возвращает `Some(color)` только если у элемента есть `href` атрибут
/// (якорные `<a>` без `href` не являются гиперссылками).
fn ua_link_color(doc: &Document, node: NodeId) -> Option<Color> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    if name.local.as_str() == "a" && doc.get(node).get_attr("href").is_some() {
        Some(Color { r: 0, g: 0, b: 238, a: 255 }) // #0000ee
    } else {
        None
    }
}

/// UA stylesheet: масштаб font-size для `<small>`, `<sub>`, `<sup>`.
/// HTML5 §15.3.3: font-size: smaller (≈ 0.83× родительского).
/// Возвращает `Some(factor)` — multiplier к `parent_font_size`.
fn ua_font_size_factor(doc: &Document, node: NodeId) -> Option<f32> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "small" | "sub" | "sup" => Some(0.83),
        _ => None,
    }
}

/// UA stylesheet: `vertical-align` для `<sub>` и `<sup>`.
/// HTML5 §15.3.3: sub → Sub, sup → Super.
fn ua_vertical_align(doc: &Document, node: NodeId) -> Option<VerticalAlign> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "sub" => Some(VerticalAlign::Sub),
        "sup" => Some(VerticalAlign::Super),
        _ => None,
    }
}

/// Применяет HTML presentational hints для `<img>`, `<video>`, `<iframe>`:
/// `width`/`height`, `hspace`/`vspace` (→ margin), `border` для `<img>`.
/// HTML5 §15.3.9. Author CSS поверх — выигрывает.
fn apply_image_presentational_hints(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let is_img = name.local == "img";
    let is_video = name.local == "video";
    let is_iframe = name.local == "iframe";
    if !is_img && !is_video && !is_iframe {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(w) = node_ref.get_attr("width").and_then(parse_html_dimension) {
        style.width = Some(Length::Px(w));
    }
    if let Some(h) = node_ref.get_attr("height").and_then(parse_html_dimension) {
        style.height = Some(Length::Px(h));
    }
    // hspace/vspace/border are <img>-only presentational attributes (HTML5 §15.3.9).
    if is_img {
        if let Some(h) = node_ref.get_attr("hspace").and_then(parse_html_dimension) {
            style.margin_left = LengthOrAuto::Length(Length::Px(h));
            style.margin_right = LengthOrAuto::Length(Length::Px(h));
        }
        if let Some(v) = node_ref.get_attr("vspace").and_then(parse_html_dimension) {
            style.margin_top = LengthOrAuto::Length(Length::Px(v));
            style.margin_bottom = LengthOrAuto::Length(Length::Px(v));
        }
        if let Some(b) = node_ref.get_attr("border").and_then(parse_html_dimension) {
            style.border_top_width = b;
            style.border_right_width = b;
            style.border_bottom_width = b;
            style.border_left_width = b;
            if b > 0.0 {
                style.border_top_style = BorderStyle::Solid;
                style.border_right_style = BorderStyle::Solid;
                style.border_bottom_style = BorderStyle::Solid;
                style.border_left_style = BorderStyle::Solid;
            }
        }
    }
}

/// SVG 2 §6.4 — SVG presentation attributes. Geometry and paint properties on
/// SVG elements may be given as plain XML attributes (e.g. `<path fill="none"
/// stroke="#e94560" stroke-width="8">`) instead of CSS. Each maps onto the
/// corresponding CSS property, but with the **lowest author-origin priority**:
/// any matching CSS rule (stylesheet selector or inline `style=""`) overrides it.
///
/// We therefore apply them *before* the matched-declaration cascade loop, reusing
/// `apply_declaration` for parsing so the attribute and the CSS form share one
/// code path. Gated by SVG tag name so HTML attributes coincidentally named
/// `fill`/`stroke`/`color` on non-SVG elements are not reinterpreted as paint.
#[allow(clippy::too_many_arguments)]
fn apply_svg_presentational_hints(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
    em_basis: f32,
    viewport: Size,
    parent_weight: FontWeight,
    inherited: &ComputedStyle,
    is_quirks: bool,
) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if !is_svg_presentational_element(name.local.as_ref()) {
        return;
    }
    // Presentation attributes recognised by `apply_declaration`
    // (SVG §11 paint + §13 stroke geometry, plus `color` for `currentColor`).
    const ATTRS: &[&str] = &[
        "fill",
        "fill-opacity",
        "fill-rule",
        "clip-rule",
        "stroke",
        "stroke-opacity",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
        "stroke-miterlimit",
        "stroke-dasharray",
        "stroke-dashoffset",
        "text-anchor",
        "dominant-baseline",
        "baseline-shift",
        "color",
        "opacity",
    ];
    let node_ref = doc.get(node);
    for &attr in ATTRS {
        let Some(val) = node_ref.get_attr(attr) else { continue };
        if val.trim().is_empty() {
            continue;
        }
        let decl = Declaration {
            property: attr.to_string(),
            value: val.to_string(),
            important: false,
        };
        apply_declaration(style, &decl, em_basis, viewport, parent_weight, inherited, inherited, is_quirks, false);
    }
}

/// True for SVG element local names that accept SVG presentation attributes.
/// Covers the shapes/containers Lumen lays out plus text elements.
fn is_svg_presentational_element(local: &str) -> bool {
    matches!(
        local,
        "svg" | "g" | "rect" | "circle" | "ellipse" | "line" | "path"
            | "polygon" | "polyline" | "text" | "tspan" | "textPath" | "use"
    )
}

/// HTML5 §15: `bgcolor` атрибут на `<body>` / table-related элементах
/// мапается на `background-color` (presentational hint). Парсится через
/// HTML5 §2.4.6 «rules for parsing a legacy color value». Любое author-CSS
/// правило в каскаде ниже перекроет hint — так и устроена presentational
/// hint конструкция.
///
/// Список тегов взят из HTML5 §15.3.6 (`<body>`) и §15.3.8 (table-tree).
/// Phase 0 ещё не делает табличный layout — но bgcolor попадает в
/// `style.background_color` всё равно, чтобы при появлении table-layout
/// рендеринг сразу работал.
fn apply_bgcolor_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    if !matches!(
        tag,
        "body" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("bgcolor")
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        style.background_color = Some(CssColor::Rgba(c));
    }
}

/// HTML LS §15.3.8 «Tables»: `background` атрибут на `<body>` / table-tree
/// элементах мапается на `background-image` (BUG-603 point 2). Тот же tag-set,
/// что и у [`apply_bgcolor_presentational_hint`]. В отличие от `bgcolor`,
/// значение не резолвится в абсолютный URL здесь — как и обычный CSS
/// `background: url(...)`, сырая строка хранится в `BackgroundImage::Url` и
/// резолвится относительно document base URL на paint/fetch стороне (см.
/// использование `BackgroundImage::Url` в CSS-парсинге фона выше — там тоже
/// хранится сырой текст, не резолвленный путь).
fn apply_background_image_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    if !matches!(
        tag,
        "body" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("background") {
        let val = val.trim();
        if !val.is_empty() {
            if style.background_layers.is_empty() {
                style.background_layers.push(BackgroundLayer::default());
            }
            style.background_layers[0].image = BackgroundImage::Url(val.to_string());
        }
    }
}

/// HTML LS §15.3.8 «Tables»: `bordercolor` атрибут на table-tree элементах
/// мапается на все четыре `border-*-color` (BUG-603 point 2). Парсится тем же
/// legacy-парсером, что и `bgcolor`/`text`/`font color`. Не включает `<body>`
/// (spec ограничивает `bordercolor` собственно табличными элементами).
fn apply_bordercolor_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    if !matches!(
        tag,
        "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("bordercolor")
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        let color = CssColor::Rgba(c);
        style.border_top_color = color;
        style.border_right_color = color;
        style.border_bottom_color = color;
        style.border_left_color = color;
    }
}

/// HTML LS §15.3.8 «Tables»: `cellspacing` атрибут на `<table>` мапается на
/// `border-spacing` (BUG-603 point 2) — один legacy-атрибут задаёт оба
/// компонента (horizontal и vertical) одинаково, симметрично `cellpadding`→
/// `padding` в [`apply_ua_table_cell_padding`]. Unlike `cellpadding`, this
/// applies directly to the `<table>` element itself (`border-spacing` is not
/// something a `<td>`/`<tr>` reads), not via an ancestor walk.
fn apply_cellspacing_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "table" {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("cellspacing")
        && let Ok(n) = val.trim().parse::<f32>()
        && n >= 0.0
    {
        style.border_spacing_h = n;
        style.border_spacing_v = n;
    }
}

/// HTML5 §15.3.6 «The page» (для `<body text>`) + §15.3.2 «Phrasing
/// content» (для `<font color>`): мапает legacy-атрибуты на CSS `color`.
///
/// - `<body text="…">` → `body.color`. Через CSS-наследование цвет
///   распространяется на всех потомков, у которых нет явного `color`.
/// - `<font color="…">` → элементный `color`. Атрибут применим к любому
///   элементу с именем `font`, в т.ч. внутри других элементов.
///
/// `<body link/vlink/alink>` отложены: hyperlink coloring требует UA
/// stylesheet с descendant-селектором (`body :link { color: … }`), а в
/// Phase 0 без visited/active runtime два из трёх атрибутов всё равно
/// были бы no-op.
///
/// Парсинг — `parse_legacy_color_html_attr` (HTML5 §2.4.6). Hint
/// применяется ДО CSS-каскада, поэтому любое author-CSS правило
/// перекроет атрибут.
fn apply_text_color_presentational_hint(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    let node_ref = doc.get(node);
    let attr_name = match tag {
        "body" => "text",
        "font" => "color",
        _ => return,
    };
    if let Some(val) = node_ref.get_attr(attr_name)
        && let Some(c) = parse_legacy_color_html_attr(val)
    {
        style.color = c;
    }
}

/// HTML5 §15.3.2: `<font size="N">` → абсолютный font-size; `<font face="…">` → font-family.
///
/// Значения `size` 1–7 отображаются на CSS absolute-size keywords (medium = 16px):
/// 1→10px 2→13px 3→16px 4→18px 5→24px 6→32px 7→48px.
/// Относительные (`+2`, `-1`) прибавляются к базе 3, затем клэмпируются в [1,7].
/// Hint применяется ДО CSS-каскада, поэтому author font-size/font-family перекроет.
fn apply_font_element_presentational_hints(
    doc: &Document,
    node: NodeId,
    style: &mut ComputedStyle,
) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "font" {
        return;
    }
    let node_ref = doc.get(node);

    // `size` attribute → font-size.
    if let Some(val) = node_ref.get_attr("size") {
        let val = val.trim();
        let size_num: Option<i32> = if let Some(rel) = val.strip_prefix('+') {
            rel.parse::<i32>().ok().map(|d| 3 + d)
        } else if let Some(rel) = val.strip_prefix('-') {
            rel.parse::<i32>().ok().map(|d| 3 - d)
        } else {
            val.parse::<i32>().ok()
        };
        if let Some(n) = size_num {
            // Clamp to [1, 7] then map to absolute px per HTML5 §15.3.2.
            let px: f32 = match n.clamp(1, 7) {
                1 => 10.0,
                2 => 13.0,
                3 => 16.0,
                4 => 18.0,
                5 => 24.0,
                6 => 32.0,
                _ => 48.0, // 7
            };
            style.font_size = px;
        }
    }

    // `face` attribute → font-family.
    if let Some(val) = node_ref.get_attr("face") {
        let families = parse_font_family(val);
        if !families.is_empty() {
            style.font_family = families;
        }
    }
}

/// HTML5 §15.3.3: атрибут `align` на блочных элементах → CSS `text-align`.
///
/// Применяется к: div, p, h1–h6, blockquote, address, dt, dd, caption.
/// Значения: left→Left, right→Right, center/middle→Center, justify→Justify.
/// Hint применяется ДО CSS-каскада, author text-align перекроет.
fn apply_align_presentational_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if !matches!(
        name.local.as_str(),
        "div" | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "address"
            | "dt"
            | "dd"
            | "caption"
    ) {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(val) = node_ref.get_attr("align") {
        let ta = match val.trim().to_ascii_lowercase().as_str() {
            "left" => TextAlign::Left,
            "right" => TextAlign::Right,
            "center" | "middle" => TextAlign::Center,
            _ => return,
        };
        style.text_align = ta;
    }
}

/// CSS Quirks Mode §4.1 + HTML5 §14.3.9: `width`/`height` presentational
/// hints для ячеек таблицы (`<td>`, `<th>`) и самого `<table>`.
///
/// `<table width="N">` → `width: Npx` (оба режима).
/// `<td width="N">` / `<th width="N">`:
///   - Standards mode → `width: Npx`
///   - Quirks mode → `min-width: Npx` (CSS Quirks §4.1: ячейка не
///     может быть *уже* указанного, но расширяться разрешено — table
///     layout не перегрузит ячейку по ширине)
///
/// `<td height="N">` / `<th height="N">` / `<table height="N">` → `height: Npx`
/// без quirks-вариации (HTML5 §14.3.9.1).
///
/// Процентные значения (`"50%"`) поддерживаются через `Length::Percent`.
fn apply_table_cell_width_hint(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let tag = name.local.as_str();
    let is_cell = matches!(tag, "td" | "th");
    let is_table = tag == "table";
    if !is_cell && !is_table {
        return;
    }
    let node_ref = doc.get(node);
    if let Some(len) = node_ref.get_attr("width").and_then(parse_html_length_attr) {
        if is_cell && doc.mode() == DocumentMode::Quirks {
            // CSS Quirks §4.1: width attr на ячейке → min-width, не width.
            style.min_width = Some(len);
        } else {
            style.width = Some(len);
        }
    }
    if let Some(len) = node_ref.get_attr("height").and_then(parse_html_length_attr) {
        style.height = Some(len);
    }
}

/// Парсит HTML dimension-атрибут как `Length`.
///
/// `"200"` → `Length::Px(200.0)`, `"50%"` → `Length::Percent(50.0)`.
/// Мусор после цифр игнорируется (HTML5 §2.4.4.5).
fn parse_html_length_attr(s: &str) -> Option<Length> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let digits: String = pct.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u32>().ok().map(|n| Length::Percent(n as f32))
    } else {
        parse_html_dimension(s).map(Length::Px)
    }
}

/// HTML5 §2.4.6 «rules for parsing a legacy color value».
///
/// Используется для presentational hint-атрибутов вроде `<body bgcolor>`,
/// `<td bgcolor>`, `<body text>`, `<font color>`. Алгоритм значительно
/// лояльнее CSS-парсера: принимает named colors, `#rgb` / `#rrggbb`,
/// hashless hex произвольной длины, и через padding/truncate process
/// выдаёт цвет из почти любой непустой строки, отличной от
/// «transparent».
///
/// Отказы (Spec: «error»):
/// - пустая строка / только whitespace;
/// - ASCII case-insensitive match «transparent».
///
/// Все остальные строки возвращают непустой цвет — это нужно для
/// совместимости с legacy-разметкой, где атрибуты часто содержат мусор.
///
/// Реализация работает в `Vec<char>` (Unicode code points), как требует
/// spec — не в байтах. Не-BMP code-point (> U+FFFF) заменяется на две
/// ASCII-«0» (spec step 6).
fn parse_legacy_color_html_attr(input: &str) -> Option<Color> {
    // Step 1-2: empty → error.
    if input.is_empty() {
        return None;
    }
    // Step 3: strip leading/trailing ASCII whitespace.
    let trimmed = input.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' '));
    if trimmed.is_empty() {
        return None;
    }
    // Step 4: case-insensitive «transparent» → error.
    if trimmed.eq_ignore_ascii_case("transparent") {
        return None;
    }
    // Step 5: named X11 / CSS3 color.
    let lc = trimmed.to_ascii_lowercase();
    // `named_color` принимает уже-lc имя и для «transparent» вернул бы
    // TRANSPARENT-константу — но мы уже отказали выше, так что попадание
    // невозможно.
    if let Some(c) = named_color(&lc) {
        return Some(c);
    }
    // Step 6: special-case 4-char `#xyz` short hex.
    let bytes = trimmed.as_bytes();
    if trimmed.len() == 4
        && bytes[0] == b'#'
        && bytes[1].is_ascii_hexdigit()
        && bytes[2].is_ascii_hexdigit()
        && bytes[3].is_ascii_hexdigit()
    {
        let r = hex_digit_value(bytes[1]) * 17;
        let g = hex_digit_value(bytes[2]) * 17;
        let b = hex_digit_value(bytes[3]) * 17;
        return Some(Color { r, g, b, a: 255 });
    }
    // Step 7: replace non-BMP code-points с двумя «0»; затем truncate до 128.
    let mut chars: Vec<char> = Vec::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        if (c as u32) > 0xFFFF {
            chars.push('0');
            chars.push('0');
        } else {
            chars.push(c);
        }
    }
    if chars.len() > 128 {
        chars.truncate(128);
    }
    // Step 8: leading `#` удаляется.
    if !chars.is_empty() && chars[0] == '#' {
        chars.remove(0);
    }
    // Step 9: не-hex-digits заменяются на «0».
    for c in &mut chars {
        if !c.is_ascii_hexdigit() {
            *c = '0';
        }
    }
    // Step 10: padding нулями до длины > 0 и multiple of 3.
    while chars.is_empty() || !chars.len().is_multiple_of(3) {
        chars.push('0');
    }
    // Step 11: split на три равных компонента.
    let mut length = chars.len() / 3;
    let mut red: Vec<char> = chars[0..length].to_vec();
    let mut green: Vec<char> = chars[length..length * 2].to_vec();
    let mut blue: Vec<char> = chars[length * 2..length * 3].to_vec();
    // Step 12: если length > 8, оставляем только последние 8 (срезаем leading).
    if length > 8 {
        let skip = length - 8;
        red.drain(0..skip);
        green.drain(0..skip);
        blue.drain(0..skip);
        length = 8;
    }
    // Step 13: пока length > 2 и у всех трёх компонентов лидирующий «0» —
    // удаляем по «0» из каждого. Это «strip common leading zeros».
    while length > 2 && red[0] == '0' && green[0] == '0' && blue[0] == '0' {
        red.remove(0);
        green.remove(0);
        blue.remove(0);
        length -= 1;
    }
    // Step 14: если length всё ещё > 2, оставляем только первые 2.
    if length > 2 {
        red.truncate(2);
        green.truncate(2);
        blue.truncate(2);
    }
    // Step 15-19: parse hex.
    let r = u8::from_str_radix(&red.iter().collect::<String>(), 16).ok()?;
    let g = u8::from_str_radix(&green.iter().collect::<String>(), 16).ok()?;
    let b = u8::from_str_radix(&blue.iter().collect::<String>(), 16).ok()?;
    Some(Color { r, g, b, a: 255 })
}

/// Значение ASCII hex-digit как 0..=15. Caller гарантирует
/// `is_ascii_hexdigit()` — иначе возвращает 0.
fn hex_digit_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// HTML5 «rules for parsing dimension values»: unitless целое число
/// пикселей, опциональный trailing `%` (Phase 0 пропускаем процентный
/// случай — нужен containing-block-width). Отрицательные значения
/// невалидны.
fn parse_html_dimension(s: &str) -> Option<f32> {
    let s = s.trim();
    // Процентные размеры пока не поддерживаем — требуют containing block.
    if s.ends_with('%') {
        return None;
    }
    // Берём префикс из цифр (HTML5 принимает мусор после), парсим как u32.
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok().map(|n| n as f32)
}

/// CSS Quirks Mode — UA-rule только для Quirks-mode: элемент `<table>`
/// сбрасывает font / color / text-align / white-space-related свойства
/// к initial-values, не наследует от родителя. Эквивалент UA-stylesheet
/// правила (как в Chromium / Firefox / WebKit):
///
/// ```css
/// table {
///     font-size: medium;
///     font-weight: normal;
///     font-style: normal;
///     font-variant: normal;
///     line-height: normal;
///     color: -webkit-text;
///     text-align: -webkit-auto;
///     white-space: normal;
///     font-family: -webkit-default;
/// }
/// ```
///
/// Эффект: classics 90-х/2000-х с `<body style="font: 20px serif; color:
/// blue">` + table-layout не «протекают» в таблицу — таблица отрисовывается
/// дефолтным шрифтом / цветом. В Standards / LimitedQuirks таблица
/// наследует обычно. Author CSS поверх Quirks-reset выигрывает: spec
/// §UA-stylesheet — это самый низкий cascade origin.
fn apply_quirks_table_reset(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "table" {
        return;
    }
    style.font_size = ROOT_FONT_SIZE;
    style.line_height = 1.2;
    style.font_family = default_font_family();
    style.font_style = FontStyle::Normal;
    style.font_variant_caps = FontVariantCaps::Normal;
    style.font_weight = FontWeight::NORMAL;
    style.font_stretch = FontStretch::NORMAL;
    style.color = Color::BLACK;
    style.text_align = TextAlign::Start;
    style.white_space = WhiteSpace::Normal;
}

/// CSS Quirks Mode §3.2: в quirks-mode replaced-элементы получают UA-правило
/// `line-height: 1`, которое блокирует наследование «normal» и убирает зазор
/// под `<img>` в inline-контексте (так делал IE7). Author CSS поверх — выигрывает.
fn apply_quirks_line_height(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if matches!(
        name.local.as_str(),
        "img" | "video" | "canvas" | "embed" | "object"
            | "iframe" | "input" | "textarea" | "select" | "audio"
    ) {
        style.line_height = 1.0;
    }
}

/// CSS Quirks Mode §3.5 — viewport height as percentage basis for `<html>`.
///
/// In quirks mode the `<html>` element acts as if it has a definite height
/// equal to the viewport height, so that descendant elements can resolve
/// percentage heights against it (e.g. `body { height: 100% }`).
///
/// Implemented as a UA rule `html { height: 100vh }` applied before the CSS
/// cascade.  `Vh` resolves against the viewport directly and therefore does
/// not need a definite `available_height` from the parent (Document) box,
/// which currently propagates `None`.  Author CSS (`height: 200px`,
/// `height: auto`) overrides this UA rule through normal cascade ordering.
fn apply_quirks_html_height(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() == "html" {
        style.height = Some(Length::Vh(100.0));
    }
}

/// UA stylesheet для font-weight: `<b>`, `<strong>`, `<th>`, `<h1>`–`<h6>`
/// получают bold по умолчанию (HTML §15.3.3).
fn ua_font_weight(doc: &Document, node: NodeId) -> Option<FontWeight> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "b" | "strong" | "th" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            Some(FontWeight::BOLD)
        }
        _ => None,
    }
}

/// UA stylesheet для `<hr>` (HTML5 §15.3.7 / Rendering §14.6).
///
/// Браузеры рендерят `<hr>` как 1px-линию через border-top с авто-маргинами.
/// Author CSS может перекрыть любое из этих значений.
fn apply_ua_hr_style(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "hr" {
        return;
    }
    style.border_top_width = 1.0;
    style.border_top_style = BorderStyle::Solid;
    style.border_top_color = CssColor::Rgba(Color { r: 118, g: 118, b: 118, a: 255 });
    style.margin_top = LengthOrAuto::Length(Length::Em(0.5));
    style.margin_bottom = LengthOrAuto::Length(Length::Em(0.5));
    style.margin_left = LengthOrAuto::Auto;
    style.margin_right = LengthOrAuto::Auto;
}

/// UA stylesheet для `<body>` (HTML Rendering §14.3.3): `body { margin: 8px }`.
///
/// Без этого правила `<body>` прижимается вплотную к краю viewport, и весь
/// контент в нормальном потоке сдвинут на 8px относительно настоящих браузеров.
/// Применяется ДО CSS-каскада, поэтому author `body { margin: 0 }` или
/// `* { margin: 0 }` перекрывает его (как в большинстве graphic-тестов с reset).
///
/// BUG-204: страницы anchor-positioning (тесты 85–89) без CSS-reset расходились
/// с Edge на ~2% — Edge сдвигал `.__f`-рамку на 8px (body margin), Lumen рисовал
/// её вплотную к краю.
fn apply_ua_body_margin(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "body" {
        return;
    }
    style.margin_top = LengthOrAuto::Length(Length::Px(8.0));
    style.margin_right = LengthOrAuto::Length(Length::Px(8.0));
    style.margin_bottom = LengthOrAuto::Length(Length::Px(8.0));
    style.margin_left = LengthOrAuto::Length(Length::Px(8.0));
}

/// UA stylesheet для `<h1>`–`<h6>` (HTML Rendering §15.3.3 «Sections and headings»).
///
/// Браузеры задают заголовкам увеличенный `font-size` (em относительно
/// родителя) и вертикальные `margin` (em относительно собственного
/// computed font-size). `font-weight: bold` уже выставляется `ua_font_weight`.
///
/// `font_size` пишется как computed px (`inherited.font_size * factor`) — так же,
/// как `ua_font_size_factor` для `<small>`/`<sub>`/`<sup>`; author `font-size`
/// перекроет его в font-size pre-pass. Маргины задаются как `Em`, поэтому
/// резолвятся против финального font-size заголовка на этапе layout; author CSS
/// перекроет их в main-pass каскада.
///
/// Значения (font-size factor, vertical margin em):
/// h1 2.0/0.67, h2 1.5/0.83, h3 1.17/1.0, h4 1.0/1.33, h5 0.83/1.67, h6 0.67/2.33.
fn apply_ua_heading_style(doc: &Document, node: NodeId, inherited: &ComputedStyle, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    let (size_factor, margin_em) = match name.local.as_str() {
        "h1" => (2.0, 0.67),
        "h2" => (1.5, 0.83),
        "h3" => (1.17, 1.0),
        "h4" => (1.0, 1.33),
        "h5" => (0.83, 1.67),
        "h6" => (0.67, 2.33),
        _ => return,
    };
    style.font_size = inherited.font_size * size_factor;
    style.margin_top = LengthOrAuto::Length(Length::Em(margin_em));
    style.margin_bottom = LengthOrAuto::Length(Length::Em(margin_em));
}

/// UA stylesheet для HTML form controls (HTML5 §15.5 «Rendering»).
///
/// Returns UA colors `(border, background, foreground)` for form controls.
///
/// Approximates CSS Color 4 system-color keywords (`ButtonFace`, `Field`,
/// `ButtonText`, `FieldText`) without a full system-color implementation.
/// Used by `apply_ua_form_controls` to theme controls for light/dark mode.
///
/// - Light: border #767676 / bg white (inputs) or #efefef (button) / fg black
/// - Dark:  border #616161 / bg #1e1e1e (inputs) or #3a3a3c (button) / fg white
///
/// `// CSS: color-scheme` — P4 wires this to `ComputedStyle.color_scheme`
/// for full system-color keyword support.
pub fn ua_form_element_colors(tag: &str, dark_mode: bool) -> (CssColor, CssColor, Color) {
    if dark_mode {
        let border = CssColor::Rgba(Color { r: 97, g: 97, b: 97, a: 255 });
        let fg = Color { r: 255, g: 255, b: 255, a: 255 };
        let bg = if tag == "button" {
            CssColor::Rgba(Color { r: 58, g: 58, b: 60, a: 255 })
        } else {
            CssColor::Rgba(Color { r: 30, g: 30, b: 30, a: 255 })
        };
        (border, bg, fg)
    } else {
        let border = CssColor::Rgba(Color { r: 118, g: 118, b: 118, a: 255 });
        let fg = Color { r: 0, g: 0, b: 0, a: 255 };
        let bg = if tag == "button" {
            CssColor::Rgba(Color { r: 239, g: 239, b: 239, a: 255 })
        } else {
            CssColor::Rgba(Color { r: 255, g: 255, b: 255, a: 255 })
        };
        (border, bg, fg)
    }
}

/// Применяется ДО CSS-каскада — любой author-rule перекрывает.
/// - `<input type=hidden>` → `display: none`
/// - `<input type=checkbox|radio>` → 13×13 px
/// - `<input>` (остальные) → 174×21 px
/// - `<button>` → height 21 px
/// - `<textarea>` → 200×48 px
/// - `<select>` → height 21 px
/// - `<progress>` → 300×16 px
/// - `<meter>` → 300×16 px
/// - Все кроме hidden → border, background, color по `ua_form_element_colors`
fn apply_ua_form_controls(doc: &Document, node: NodeId, style: &mut ComputedStyle, dark_mode: bool) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    let tag = name.local.as_str();
    match tag {
        "input" => {
            let ty = doc
                .get(node)
                .get_attr("type")
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            match ty.trim() {
                "hidden" => {
                    style.display = Display::None;
                    return;
                }
                "checkbox" | "radio" => {
                    style.width = Some(Length::Px(13.0));
                    style.height = Some(Length::Px(13.0));
                }
                "range" => {
                    style.width = Some(Length::Px(129.0));
                    style.height = Some(Length::Px(20.0));
                    // Range input has no visible border — track/thumb are drawn by paint.
                    return;
                }
                _ => {
                    style.width = Some(Length::Px(174.0));
                    style.height = Some(Length::Px(21.0));
                }
            }
        }
        "button" => {
            style.height = Some(Length::Px(21.0));
        }
        "textarea" => {
            style.width = Some(Length::Px(200.0));
            style.height = Some(Length::Px(48.0));
        }
        "select" => {
            style.height = Some(Length::Px(21.0));
        }
        "progress" | "meter" => {
            style.width = Some(Length::Px(300.0));
            style.height = Some(Length::Px(16.0));
        }
        _ => return,
    }
    let (border, bg, fg) = ua_form_element_colors(tag, dark_mode);
    style.border_top_width = 1.0;
    style.border_right_width = 1.0;
    style.border_bottom_width = 1.0;
    style.border_left_width = 1.0;
    style.border_top_style = BorderStyle::Solid;
    style.border_right_style = BorderStyle::Solid;
    style.border_bottom_style = BorderStyle::Solid;
    style.border_left_style = BorderStyle::Solid;
    style.border_top_color = border;
    style.border_right_color = border;
    style.border_bottom_color = border;
    style.border_left_color = border;
    style.background_color = Some(bg);
    style.color = fg;
}

/// CSS Basic UI L4 §4.4 — post-cascade pass: when `field-sizing: content` was set
/// by the author stylesheet, clears any UA-supplied `width`/`height` on text-entry
/// controls so that `lay_out` will call `field_sizing_content_intrinsic` instead.
///
/// Must run AFTER the CSS cascade so that author `field-sizing: content` is final.
fn apply_ua_form_controls_field_sizing_clear(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    match name.local.as_str() {
        "input" => {
            let ty = doc
                .get(node)
                .get_attr("type")
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            // Only text-entry types are eligible; checkbox/radio/range use fixed sizes.
            match ty.trim() {
                "checkbox" | "radio" | "range" | "hidden" => {}
                _ => {
                    style.width = None;
                    style.height = None;
                }
            }
        }
        "textarea" => {
            style.width = None;
            style.height = None;
        }
        _ => {}
    }
}

/// CSS Basic UI L4 §5 — strips UA-default styling (border, padding, background)
/// from a form control under `appearance: none`.
///
/// Called *before* the author cascade (gated on the pre-scanned cascade-winning
/// `appearance` value, see `compute_style`) so author-specified
/// border/background/padding declarations apply on top of the cleared UA
/// defaults. Running this *after* the cascade (the pre-BUG-211 behaviour)
/// clobbered author values, leaving content-sized fields with width-0 borders
/// and a transparent background.
///
/// Applies to: <input>, <button>, <select>, <textarea>, <progress>, <meter>.
fn strip_ua_appearance_box_styling(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    match name.local.as_str() {
        "input" | "button" | "select" | "textarea" | "progress" | "meter" => {
            // Remove UA border
            style.border_top_width = 0.0;
            style.border_right_width = 0.0;
            style.border_bottom_width = 0.0;
            style.border_left_width = 0.0;
            // Remove UA padding
            style.padding_top = Length::Px(0.0);
            style.padding_right = Length::Px(0.0);
            style.padding_bottom = Length::Px(0.0);
            style.padding_left = Length::Px(0.0);
            // Remove UA background (fully transparent)
            style.background_color = Some(CssColor::Rgba(Color { r: 0, g: 0, b: 0, a: 0 }));
        }
        _ => {}
    }
}

/// UA stylesheet: `<dialog>` without the `open` attribute → `display: none`.
/// HTML5 §15.3.9: "dialog:not([open]) { display: none; }"
fn apply_ua_dialog_display(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let NodeData::Element { name, .. } = &doc.get(node).data else { return; };
    if name.local.as_str() == "dialog" && doc.get(node).get_attr("open").is_none() {
        style.display = Display::None;
    }
}

/// UA stylesheet (HTML Rendering §15.3.8): `td, th { padding: 1px }`.
///
/// Table cells get a default 1px padding on all four sides. The legacy
/// `cellpadding` attribute on the nearest ancestor `<table>` overrides this for
/// every cell (HTML §14.3.9.1): a non-negative numeric value sets the padding,
/// so `cellpadding="0"` (ubiquitous in legacy layout tables) restores zero.
/// Applied during the pre-cascade UA phase so author `padding` declarations win.
fn apply_ua_table_cell_padding(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else { return; };
    if !matches!(name.local.as_str(), "td" | "th") {
        return;
    }
    // Default 1px; an ancestor <table cellpadding=N> overrides it.
    let mut pad = 1.0_f32;
    let mut cur = node_ref.parent;
    while let Some(p) = cur {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "table"
        {
            if let Some(v) = p_ref.get_attr("cellpadding")
                && let Ok(n) = v.trim().parse::<f32>()
                && n >= 0.0
            {
                pad = n;
            }
            break;
        }
        cur = p_ref.parent;
    }
    style.padding_top = Length::Px(pad);
    style.padding_right = Length::Px(pad);
    style.padding_bottom = Length::Px(pad);
    style.padding_left = Length::Px(pad);
}

/// UA stylesheet (HTML Rendering §15.4.2): `[inert] { pointer-events: none; }`.
///
/// An element carrying the `inert` boolean attribute — and, because inertness is
/// inherited down the DOM tree, every descendant of such an element — is made
/// non-interactive. The UA origin sets `pointer-events: none` so that
/// `ComputedStyle.pointer_events` reflects inertness (e.g. for `getComputedStyle`
/// and cursor resolution), complementing the layout-level hit-test filter in
/// `collect_clickable_elements` (lumen-layout `lib.rs`, see `// CSS: inert`).
///
/// Applied during the pre-cascade UA phase, so an author `pointer-events`
/// declaration overrides it (UA origin has the lowest cascade priority).
/// [`inert::is_inert`] walks the ancestor chain, so a node nested inside an
/// inert subtree is matched even when it carries no `inert` attribute itself.
fn apply_ua_inert(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if crate::inert::is_inert(doc, node) {
        style.pointer_events = PointerEvents::None;
    }
}

/// Парсит `font-family: a, "b c", d` в Vec<String>. Запятые разделяют
/// семейства; кавычки (одинарные или двойные) обрамляют имя с пробелами.
/// Имена без кавычек: один или несколько whitespace-разделённых
/// идентификаторов сливаются в одну строку с одним пробелом
/// (`Times New Roman` → `"Times New Roman"`). Пустые имена пропускаются.
pub fn parse_font_family(val: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = val.chars().peekable();
    while chars.peek().is_some() {
        // Пропускаем ведущий whitespace и запятые.
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else {
                break;
            }
        }
        let Some(&first) = chars.peek() else { break };
        let name = if first == '"' || first == '\'' {
            chars.next();
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == first { break; }
                s.push(c);
            }
            // Пропускаем до следующей запятой / EOF.
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                chars.next();
            }
            s
        } else {
            // Unquoted: собираем до запятой, схлопывая whitespace в один пробел.
            let mut s = String::new();
            let mut prev_space = false;
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                chars.next();
                if c.is_whitespace() {
                    if !s.is_empty() && !prev_space {
                        s.push(' ');
                        prev_space = true;
                    }
                } else {
                    s.push(c);
                    prev_space = false;
                }
            }
            // Trim trailing space.
            while s.ends_with(' ') {
                s.pop();
            }
            s
        };
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
}

/// Парсит CSS `font-variation-settings` (CSS Fonts L4 §7).
///
/// Синтаксис: `normal | [<string> <number>]#`
/// Пример: `"wght" 600, "wdth" 80`
///
/// Возвращает `None` при синтаксической ошибке (CSS cascading игнорирует
/// невалидные объявления). `normal` → `Some(Vec::new())`.
pub fn parse_font_variation_settings(val: &str) -> Option<Vec<FontVariationSetting>> {
    let val = val.trim();
    if val.eq_ignore_ascii_case("normal") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for token_pair in val.split(',') {
        let pair = token_pair.trim();
        if pair.is_empty() {
            continue;
        }
        // Первый токен — quoted 4-char tag
        let (tag_str, rest) = if let Some(stripped) = pair.strip_prefix('"') {
            let end = stripped.find('"')?;
            (&stripped[..end], stripped[end + 1..].trim())
        } else {
            let stripped = pair.strip_prefix('\'')?;
            let end = stripped.find('\'')?;
            (&stripped[..end], stripped[end + 1..].trim())
        };
        // Tag должен быть ровно 4 ASCII символа
        if tag_str.len() != 4 || !tag_str.is_ascii() {
            return None;
        }
        let tag_bytes = tag_str.as_bytes();
        let tag: [u8; 4] = [tag_bytes[0], tag_bytes[1], tag_bytes[2], tag_bytes[3]];
        // Следующий токен — число
        let value: f32 = rest.parse().ok()?;
        out.push(FontVariationSetting { tag, value });
    }
    Some(out)
}

/// Парсит CSS `font-feature-settings` (CSS Fonts L3 §6).
///
/// Синтаксис: `normal | <feature-tag-value>#`, где
/// `<feature-tag-value> = <string> [ <integer> | on | off ]?`.
/// Пример: `"liga" 0, "smcp", "salt" 2, "kern" off`.
///
/// Тег — ровно 4 символа ASCII U+20–U+7E; значение опущено → 1,
/// `on` → 1, `off` → 0, целое должно быть ≥ 0. Возвращает `None` при
/// синтаксической ошибке (cascade игнорирует невалидные объявления).
/// `normal` → `Some(Vec::new())`.
pub fn parse_font_feature_settings(val: &str) -> Option<Vec<FontFeatureSetting>> {
    let val = val.trim();
    if val.eq_ignore_ascii_case("normal") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for token_pair in val.split(',') {
        let pair = token_pair.trim();
        if pair.is_empty() {
            continue;
        }
        // Первый токен — quoted 4-char tag.
        let (tag_str, rest) = if let Some(stripped) = pair.strip_prefix('"') {
            let end = stripped.find('"')?;
            (&stripped[..end], stripped[end + 1..].trim())
        } else {
            let stripped = pair.strip_prefix('\'')?;
            let end = stripped.find('\'')?;
            (&stripped[..end], stripped[end + 1..].trim())
        };
        // Тег — ровно 4 печатных ASCII-символа (U+20–U+7E).
        if tag_str.len() != 4 || !tag_str.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return None;
        }
        let tag_bytes = tag_str.as_bytes();
        let tag: [u8; 4] = [tag_bytes[0], tag_bytes[1], tag_bytes[2], tag_bytes[3]];
        // Второй токен опционален: <integer ≥ 0> | on | off; по умолчанию 1.
        let value: u32 = if rest.is_empty() || rest.eq_ignore_ascii_case("on") {
            1
        } else if rest.eq_ignore_ascii_case("off") {
            0
        } else {
            rest.parse().ok()?
        };
        out.push(FontFeatureSetting { tag, value });
    }
    Some(out)
}

/// CSS Fonts L4 §11.3 — computed value of `font-palette`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FontPalette {
    /// Default CPAL palette (index 0). Initial value.
    #[default]
    Normal,
    /// First CPAL palette flagged «usable with light background».
    Light,
    /// First CPAL palette flagged «usable with dark background».
    Dark,
    /// `<dashed-ident>` naming a `@font-palette-values` rule (case-sensitive,
    /// stored with the leading `--`).
    Custom(String),
}

/// Парсит CSS `font-palette`: `normal | light | dark | <dashed-ident>`
/// (CSS Fonts L4 §11.3). Ключевые слова case-insensitive, dashed-ident
/// case-sensitive. Возвращает `None` при невалидном значении (cascade
/// игнорирует объявление).
pub fn parse_font_palette(val: &str) -> Option<FontPalette> {
    let v = val.trim();
    if v.eq_ignore_ascii_case("normal") {
        return Some(FontPalette::Normal);
    }
    if v.eq_ignore_ascii_case("light") {
        return Some(FontPalette::Light);
    }
    if v.eq_ignore_ascii_case("dark") {
        return Some(FontPalette::Dark);
    }
    if v.len() > 2 && v.starts_with("--") && !v.contains(char::is_whitespace) {
        return Some(FontPalette::Custom(v.to_string()));
    }
    None
}

/// Парсит CSS `font-weight`. Поддерживает:
///   - `normal` → 400, `bold` → 700;
///   - численные `100`..`900` (или любое число 1..1000 — Variable Fonts);
///   - относительные `lighter` / `bolder` — резолвятся относительно `parent`
///     по таблице из CSS Fonts L4 §2.4.3.
fn parse_font_weight(val: &str, parent: FontWeight) -> Option<FontWeight> {
    match val.trim() {
        "normal" => Some(FontWeight::NORMAL),
        "bold" => Some(FontWeight::BOLD),
        "lighter" => Some(relative_lighter(parent)),
        "bolder" => Some(relative_bolder(parent)),
        s => s.parse::<u16>().ok().filter(|&n| (1..=1000).contains(&n)).map(FontWeight),
    }
}

/// CSS Fonts L4 §2.4.3 таблица для `lighter`. Сужаем weight в сторону normal.
fn relative_lighter(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        100..=349 => 100,
        350..=549 => 100,
        550..=749 => 400,
        _ => 700, // 750..=1000
    })
}

/// CSS Fonts L4 §2.4.3 таблица для `bolder`.
fn relative_bolder(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        0..=349 => 400,
        350..=549 => 700,
        550..=749 => 900,
        _ => 900,
    })
}

/// Корневой font-size в CSS — 16px на момент Phase 0 (без `<html>`-стилей и
/// настроек пользователя). Используется как базис для `rem`.
pub const ROOT_FONT_SIZE: f32 = 16.0;

/// Дефолтное `font-family` документа (UA stylesheet, BUG-128).
///
/// HTML не задаёт конкретного значения — это «default font» настройки
/// браузера, и у Edge / Chrome / Firefox она равна `serif` (на Windows —
/// Times New Roman). Раньше корневой стиль нёс ПУСТОЙ список, а пустой
/// список в рендере (`Renderer::resolve_face_id`) зарезервирован за chrome
/// UI (bundled Golos Text, DS-4) — то есть страница без объявленного
/// `font-family` рисовалась шрифтом браузерного интерфейса.
///
/// Generic-имя резолвится в системный face на этапе рендера и измерения
/// (`FontProvider::pick_generic_face`, `GenericFaceSet`), поэтому здесь
/// хранится именно CSS-generic, а не конкретное имя семейства: платформенная
/// таблица кандидатов живёт в `lumen_core::ext::generic_family_candidates`.
///
/// Инвариант, на который опирается рендер: у контента `font_family` НИКОГДА
/// не пуст, пустой список бывает только у chrome-овых `DrawText`.
pub const DEFAULT_FONT_FAMILY: &str = "serif";

/// Дефолтный список `font-family` документа — см. [`DEFAULT_FONT_FAMILY`].
#[must_use]
pub fn default_font_family() -> Vec<String> {
    vec![DEFAULT_FONT_FAMILY.to_string()]
}

/// Returns `true` if `ancestor` is `node` itself, or a proper ancestor of `node` in the tree.
fn is_self_or_ancestor(doc: &Document, ancestor: NodeId, node: NodeId) -> bool {
    if ancestor == node { return true; }
    let mut cur = doc.get(node).parent;
    while let Some(parent_id) = cur {
        if parent_id == ancestor { return true; }
        if parent_id == doc.root() { break; }
        cur = doc.get(parent_id).parent;
    }
    false
}

fn expand_grouped_transition_property(prop: &str) -> Vec<String> {
    let lower = prop.to_ascii_lowercase();
    match lower.as_str() {
        "margin" => vec!["margin-top", "margin-right", "margin-bottom", "margin-left"]
            .into_iter().map(|s| s.to_string()).collect(),
        "padding" => vec!["padding-top", "padding-right", "padding-bottom", "padding-left"]
            .into_iter().map(|s| s.to_string()).collect(),
        "border" => vec!["border-top", "border-right", "border-bottom", "border-left"]
            .into_iter().map(|s| s.to_string()).collect(),
        "border-radius" => vec!["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"]
            .into_iter().map(|s| s.to_string()).collect(),
        _ => vec![prop.to_string()],
    }
}

#[allow(clippy::too_many_arguments)]
/// CSS Inline Layout L3 §5 — parse `initial-letter: normal | <number> <integer>?`.
///
/// Returns `(size, sink)`: `size` is the cap height in lines (≥ 1, may be
/// fractional) and `sink` is the number of in-flow lines the letter occupies
/// (`0` = `auto`, later resolved to `floor(size)`). `normal` → `(1.0, 0)`.
/// Returns `None` on malformed input, so the declaration is ignored and the
/// previous value is kept (CSS error recovery).
fn parse_initial_letter(val: &str) -> Option<(f32, u32)> {
    let v = val.trim();
    if v.eq_ignore_ascii_case("normal") {
        return Some((1.0, 0));
    }
    let mut it = v.split_whitespace();
    let size: f32 = it.next()?.parse().ok()?;
    if !size.is_finite() || size < 1.0 {
        return None;
    }
    let sink = match it.next() {
        Some(tok) => {
            let n: i64 = tok.parse().ok()?;
            if n < 1 {
                return None;
            }
            u32::try_from(n).ok()?
        }
        None => 0,
    };
    if it.next().is_some() {
        return None;
    }
    Some((size, sink))
}


/// CSS Grid L1 §7.3 — parse `grid-template-areas` value.
///
/// Input: a CSS string value like `'"header header" "sidebar main"'`.
/// Each quoted string defines one row; tokens within the string are cell
/// names. `"."` (dot) is the null cell token (unnamed). Returns a 2D grid:
/// outer vec = rows top-to-bottom, inner vec = column names left-to-right.
///
/// Malformed rows (different column count) are silently dropped to keep the
/// grid rectangular.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn parse_grid_template_areas(val: &str) -> Vec<Vec<String>> {
    // Extract all quoted strings in order.
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut s = val.trim();
    while !s.is_empty() {
        s = s.trim_start();
        if s.starts_with('"') || s.starts_with('\'') {
            let quote = s.chars().next().unwrap();
            s = &s[1..];
            let end = s.find(quote).unwrap_or(s.len());
            let row_str = &s[..end];
            s = if end < s.len() { &s[end + 1..] } else { "" };
            let cells: Vec<String> = row_str
                .split_whitespace()
                .map(|t| t.to_string())
                .collect();
            if !cells.is_empty() {
                rows.push(cells);
            }
        } else {
            // Skip unexpected token.
            let next = s.find(|c: char| c.is_whitespace() || c == '"' || c == '\'').unwrap_or(s.len());
            s = &s[next..];
        }
    }
    // Ensure all rows have the same column count (take minimum; drop trailing).
    if rows.is_empty() {
        return rows;
    }
    let cols = rows.iter().map(Vec::len).min().unwrap_or(0);
    rows.retain(|r| r.len() == cols);
    rows
}

/// CSS Sizing L4 §6.1 — парсит `<ratio>`: либо одно положительное
/// число (трактуется как W:1), либо `W / H` пара. Phase 0 не
/// поддерживает `auto <ratio>` форму (она бы хранилась как fallback,
/// но требует расширения структуры).
fn parse_aspect_ratio_value(s: &str) -> Option<(f32, f32)> {
    let s = s.trim();
    if let Some((w_str, h_str)) = s.split_once('/') {
        let w = w_str.trim().parse::<f32>().ok()?;
        let h = h_str.trim().parse::<f32>().ok()?;
        if w > 0.0 && h > 0.0 {
            return Some((w, h));
        }
        return None;
    }
    // Single number — W:1.
    let v = s.parse::<f32>().ok()?;
    if v > 0.0 {
        Some((v, 1.0))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
