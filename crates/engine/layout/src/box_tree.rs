//! Box tree: block-флоу + inline-флоу.
//!
//! Каждый DOM-элемент даёт один LayoutBox. Блочные элементы стэкаются
//! вертикально. Текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`,
//! `<strong>`, и т.д.) объединяются в `InlineRun` — анонимный бокс, в
//! котором слова переносятся как единый поток. Слова с одинаковым стилем
//! на одной строке объединяются в один фрагмент (→ один DrawText).
//!
//! Whitespace-only текст и комментарии пропускаются.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use lumen_core::geom::{Rect, Size};
use lumen_core::ext::{HyphenationProvider, NullHyphenationProvider};
use lumen_css_parser::Stylesheet;
use lumen_dom::{build_flat_tree, Document, FlatTree, NodeData, NodeId};
use lumen_html_parser::{
    PictureParams, SizesViewport, pick_img_source, pick_picture_source,
};

use crate::style::{
    apply_container_rules, clear_cq_context, compute_pseudo_element_style, compute_style,
    set_cq_context, AlignValue,
    BackgroundImage, BorderCollapse, BoxSizing, ClearSide, ContainFlags, ContainerContext, ContainerType, Content,
    ContentItem, ComputedStyle, Direction, Display, FlexBasis, FlexDirection, FlexWrap, FloatSide,
    FontVariantCaps,
    GridAutoFlow, GridLine, GridTrackSize, Hyphens, Length, LengthOrAuto, LineBreak,
    ListStylePosition,
    ListStyleType, Overflow, OverflowWrap, Position, ScrollbarGutter, ScrollbarWidth,
    TextAlign, TextAlignLast, TextOverflow,
    TextWrapMode, TextWrapStyle,
    VerticalAlign, WordBreak,
};
use crate::counters::{precompute_counters, CounterMap, CounterStyleRegistry, QuoteSlot,
                      build_counter_style_registry, format_counter_with_registry,
                      build_list_marker_text};
use crate::subgrid::{SubgridContext, SubgridContextGuard, SUBGRID_COL_CTX, SUBGRID_ROW_CTX};
use crate::anchor::{collect_anchors, InsetAreaKeyword};
use crate::field_sizing::field_sizing_content_intrinsic;
use crate::style::FieldSizing;
use crate::TextMeasurer;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

mod container_anchor;
pub use container_anchor::apply_container_styles;
use container_anchor::apply_anchor_positions;

mod inline_wrap;
pub use inline_wrap::{measure_text_w, measure_text_w_families, measure_text_w_varied};
pub(crate) use inline_wrap::strip_soft_hyphens;
use inline_wrap::{
    align_lines, apply_inline_vertical_align, apply_line_clamp, apply_text_overflow_ellipsis, balance_wrap,
    one_line_fallback, pretty_wrap, step_line_height, wrap_inline_run,
};
// Used only by `mod tests` (super::super::X) — never called from this file's own non-test code.
#[cfg(test)]
use inline_wrap::{caps_synthesis, char_break_offset, try_hyp_break, SMALL_CAPS_SCALE};

mod grid;
pub use grid::resolve_auto_fill_fit_count;
use grid::lay_out_grid;

mod flex;
use flex::{lay_out_flex, UsedSizeOverride};

mod multicol_abspos;
use multicol_abspos::{lay_out_abs_children, lay_out_multicol_children};

mod table;
use table::{lay_out_table, lay_out_table_row, table_intrinsic_content_width};

// EE-3: when true, `lay_out` checks `b.dirty.is_clean()` and skips clean subtrees.
thread_local! {
    static INCREMENTAL_LAYOUT_MODE: Cell<bool> = const { Cell::new(false) };
}

thread_local! {
    /// BUG-341 S4 — master on/off switch for incremental box-build
    /// (`build_box_or_reuse`'s whole-subtree clone path), mirroring
    /// `counters::INCREMENTAL_RESTYLE`'s pattern. Off by default; S15 turns it
    /// on around `layout_mutation_incremental_restyle` at the pipeline call
    /// sites, alongside `counters::set_incremental_restyle`.
    static INCREMENTAL_BOX_BUILD: Cell<bool> = const { Cell::new(false) };
}

/// BUG-341 S30 census: how often a hypothetical (node, incoming-constraints)
/// layout-result cache — the "Fix scope note" idea `lay_out_flex`'s doc comment
/// gestures at — would actually hit within one full (non-incremental) layout
/// pass, measured *before* building the cache mechanism itself
/// (`docs/perf-method.md` §1: "перепись перед первой правкой").
///
/// Thread-local, not a `Mutex` like `BOX_BUILD_STATS`'s S18/S20 siblings:
/// `lay_out`/`lay_out_inner` never fan out over rayon (confirmed by grep —
/// the only `rayon::` uses in this file are inside `build_box`'s flex/grid
/// dispatch, a different stage), so there is no S15-style trap here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutKeyCensus {
    /// Real (non-`Skip`, non-incremental-translate) `lay_out_inner` calls counted.
    pub calls: u32,
    /// Of those, calls whose `(NodeId, available_width, available_height)` key
    /// had already been seen earlier in the same pass — a call a memoization
    /// cache keyed this way could have served from cache instead of recomputing.
    pub repeat_key_calls: u32,
    /// BUG-341 S31 — of `repeat_key_calls`, the ones whose `b.style` `Arc` is
    /// also `ptr_eq` to the style used the *previous* time this exact key was
    /// recorded. A `(node, constraints)`-keyed cache is only safe to serve on a
    /// repeat when the style is unchanged too: `lay_out_flex`'s own placement
    /// pass overwrites `children[i].style`'s `width`/`height`/`box_sizing`
    /// in place (`Arc::make_mut`) between its Step-1 probe and the final
    /// placement call for the *same* item, so a naive key match there would
    /// serve a stale result. This field answers "of the calls S30 counted as
    /// a hit, how many would actually be safe to serve" before the cache is
    /// built, not after.
    pub repeat_key_same_style: u32,
    /// BUG-341 S35 — of `repeat_key_same_style`, the ones whose
    /// [`UsedSizeOverride`] (S34: `lay_out_flex`'s three re-layout call sites'
    /// resolved-size override, applied out-of-place instead of mutating
    /// `b.style`) also matches the override recorded the *previous* time this
    /// key was seen. S34 removed `SavedItemSizing`'s style mutation, which
    /// made `repeat_key_same_style` jump from S31's 23.5% to 77.7% — but
    /// `repeat_key_same_style` alone does not prove a cache could safely serve
    /// those repeats: a Step-1 probe call (no override) and a final-pass call
    /// (`UsedSizeOverride` present, carrying the item's *resolved* main/cross
    /// size) can now land on the exact same `(node, available_width,
    /// available_height)` key with the exact same style `Arc` — and still be
    /// two genuinely different calls, since the override is not part of that
    /// key. Naively caching the probe's result and serving it to the override
    /// call (or vice versa) would silently use the wrong width/height/
    /// box-sizing whenever they differ, which the raw S34 number alone cannot
    /// tell you. This field is the honest ceiling S34's own "first job" note
    /// asked for: the fraction of same-style repeats that are *also* safe by
    /// this additional check, before any cache mechanism is written.
    pub repeat_key_same_style_and_override: u32,
}

/// `(node, available_width bits, available_height bits)` — the census key a
/// hypothetical layout-result cache would use, per [`LayoutKeyCensus`].
type LayoutCensusKey = (NodeId, u32, Option<u32>);

/// BUG-341 S35 — a plain-data snapshot of [`UsedSizeOverride`] the census can
/// store and compare by value (`UsedSizeOverride` itself derives neither
/// `PartialEq` nor `Hash`, and does not need to for its one real caller,
/// `lay_out_inner`'s local `s` binding — adding them there would be dead
/// weight on the production struct for a comparison only this diagnostic
/// needs). `f32` fields are compared via `to_bits`, matching the census's
/// existing exact-bit-equality convention for `available_width`/`available_height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
struct UsedSizeOverrideBits {
    width_bits: Option<u32>,
    height_bits: Option<u32>,
    box_sizing: Option<BoxSizing>,
}

impl From<Option<&UsedSizeOverride>> for UsedSizeOverrideBits {
    fn from(o: Option<&UsedSizeOverride>) -> Self {
        match o {
            None => Self::default(),
            Some(o) => Self {
                width_bits: o.width.map(f32::to_bits),
                height_bits: o.height.map(f32::to_bits),
                box_sizing: o.box_sizing,
            },
        }
    }
}

/// One [`LAYOUT_KEY_SEEN`] entry: occurrence count, the style `Arc` from the
/// most recent occurrence, and that occurrence's `UsedSizeOverride` snapshot
/// (S30 only needed the count, S31 added the style compare, S35 adds the
/// override compare) — factored out per clippy's `type_complexity`.
type LayoutCensusSeenEntry = (u32, Arc<ComputedStyle>, UsedSizeOverrideBits);

thread_local! {
    static LAYOUT_KEY_CENSUS_ON: Cell<bool> = const { Cell::new(false) };
    static LAYOUT_KEY_CENSUS: Cell<LayoutKeyCensus> = const { Cell::new(LayoutKeyCensus { calls: 0, repeat_key_calls: 0, repeat_key_same_style: 0, repeat_key_same_style_and_override: 0 }) };
    static LAYOUT_KEY_SEEN: RefCell<HashMap<LayoutCensusKey, LayoutCensusSeenEntry>> = RefCell::new(HashMap::new());
}

/// Enables/disables the BUG-341 S30/S31/S35 layout-key census and clears its
/// state — call once before a full layout pass, then read with
/// [`take_layout_key_census`].
pub fn set_layout_key_census(on: bool) {
    LAYOUT_KEY_CENSUS_ON.with(|c| c.set(on));
    LAYOUT_KEY_SEEN.with(|m| m.borrow_mut().clear());
    LAYOUT_KEY_CENSUS.with(|c| c.set(LayoutKeyCensus::default()));
}

/// Returns the accumulated [`LayoutKeyCensus`] and resets the tally.
pub fn take_layout_key_census() -> LayoutKeyCensus {
    LAYOUT_KEY_SEEN.with(|m| m.borrow_mut().clear());
    LAYOUT_KEY_CENSUS.with(|c| c.replace(LayoutKeyCensus::default()))
}

/// Records one real `lay_out_inner` invocation for the S30/S31/S35 census —
/// see [`LayoutKeyCensus`]. Exact bit-equality on the constraints, not an
/// epsilon compare: a real cache keyed this way would only ever hit on an
/// exact re-derivation of the same numbers, so exact equality is the honest
/// measure of its ceiling, not an approximation of it. `style` is compared by
/// `Arc` identity (S31): the cascade hands out one `Arc<ComputedStyle>` per
/// node and only in-place `Arc::make_mut` callers ever diverge it, so
/// `ptr_eq` is exact, not approximate, for "has this box's style changed
/// since the last time this key was recorded". `used_size_override` (S35) is
/// compared by value via [`UsedSizeOverrideBits`], since it is never behind
/// an `Arc` and two calls legitimately construct equal-but-distinct override
/// values.
fn record_layout_key_occurrence(
    node: NodeId,
    available_width: f32,
    available_height: Option<f32>,
    style: &Arc<ComputedStyle>,
    used_size_override: Option<&UsedSizeOverride>,
) {
    if !LAYOUT_KEY_CENSUS_ON.with(|c| c.get()) {
        return;
    }
    let key = (node, available_width.to_bits(), available_height.map(f32::to_bits));
    let override_bits = UsedSizeOverrideBits::from(used_size_override);
    let (repeat, same_style, same_override) = LAYOUT_KEY_SEEN.with(|m| {
        let mut seen = m.borrow_mut();
        match seen.get_mut(&key) {
            Some((count, prev_style, prev_override)) => {
                *count += 1;
                let same_style = Arc::ptr_eq(prev_style, style);
                let same_override = *prev_override == override_bits;
                *prev_style = Arc::clone(style);
                *prev_override = override_bits;
                (true, same_style, same_override)
            }
            None => {
                seen.insert(key, (1, Arc::clone(style), override_bits));
                (false, false, false)
            }
        }
    });
    LAYOUT_KEY_CENSUS.with(|c| {
        let mut v = c.get();
        v.calls += 1;
        if repeat {
            v.repeat_key_calls += 1;
            if same_style {
                v.repeat_key_same_style += 1;
                if same_override {
                    v.repeat_key_same_style_and_override += 1;
                }
            }
        }
        c.set(v);
    });
}

// BUG-341 S32/S33 correctness guard: `content-visibility: auto`'s skip
// decision (`cv_should_skip`, `content_visibility.rs`) depends on the
// current scroll offset and a cross-frame "seen at least once" ratchet —
// state that lives outside the cascade entirely, so it is *not* reflected in
// a box's `style`. Set whenever `lay_out_inner` even *checks*
// `content_visibility == Auto` for any node in the subtree currently being
// computed (not just when it actually skips).
//
// Consulted by `lay_out_grid`'s Step-4/Step-5 probe reuse (S33): a probed
// subtree whose layout touched this must never be replayed verbatim at a
// later, real position — the skip decision could differ once actually
// positioned there. Callers save (`replace(false)`), run the wrapped
// `lay_out` call, then restore `outer || touched_here` so a nested probe
// deeper in the tree doesn't falsely poison an unrelated sibling's reuse,
// while still propagating "touched" up to whichever ancestor is deciding
// whether *it* may reuse its own probe.
//
// S32 built a general `(node, constraints)`-keyed cache around this flag
// (`LayoutResultKey`/`LayoutResultEntry`, since removed): every `lay_out`
// call checked a thread-local `HashMap`, cloning the matched subtree on each
// miss. Measured on the real chrome document it was 33-41% *slower* than no
// cache at every percentile — hit rate 8.3%, driven by `SavedItemSizing`
// (`lay_out_flex`'s final placement pass) minting a fresh `style` `Arc` on
// every visit (the cascade cache already holds a second reference, so
// `Arc::make_mut` never mutates in place), so flex items structurally could
// never `ptr_eq`-match a earlier occurrence regardless of value equality —
// only CSS Grid's own intrinsic-height probe (which never mutates style) hit
// at all, and even that case paid full-subtree-clone-on-every-miss overhead
// for the ~91% of insertions that were never read back. S33 replaced the
// general mechanism with the targeted, zero-overhead fix in `lay_out_grid`
// below (see its Step-4 doc comment) and confirmed the general cache's one
// real win was never reachable from BUG-341's actual target in the first
// place: `crates/chrome/` contains no `display: grid` anywhere, so neither
// the old general cache nor the new targeted fix moves CC-12's chrome-flex
// gate at all — that gate's redundancy is 100% `lay_out_flex`, and no
// cache-shaped mechanism keyed on style identity can reach it (five slices,
// S28-S33, converge on the same wall). S34 removed `SavedItemSizing` and its
// style-mutation dance entirely: `lay_out_flex`'s three re-layout call sites
// now pass an explicit `UsedSizeOverride` into `lay_out_with_used_size`,
// applied to a locally cloned `ComputedStyle` inside `lay_out_inner` — see
// `UsedSizeOverride`'s doc comment. `b.style`'s `Arc` pointer is no longer
// touched by flex item re-layout at all, restoring the `ptr_eq` precondition
// S31 found broken (this alone does not remove the double-`lay_out()` call
// itself — that is still Step-1 probe + final pass, unchanged — so it does
// not reopen the cache question S28-S33 closed for CC-12's specific gate; see
// BUG-341 S34 for the measured effect on style-identity stability). S35
// confirmed the precondition holds even accounting for `UsedSizeOverride`
// itself (99.8% of same-style repeats also match by override value). S36
// re-builds the general cache S32 removed, keyed this time on
// `UsedSizeOverrideBits` too (not just style `ptr_eq`) — see
// `LayoutResultKey`'s own doc comment for why, and BUG-341 S36 for the
// re-measured wall-clock result against this now-stable precondition.
thread_local! {
    static CV_AUTO_TOUCHED: Cell<bool> = const { Cell::new(false) };
}

// BUG-802 correctness guard for `lay_out_flex`'s column probe reuse, the same
// shape as `CV_AUTO_TOUCHED` above. A probe lays the item out with
// `available_height: None` (indefinite containing-block height); the final
// placement pass hands it a *definite* one. The two results are identical
// exactly when nothing in the subtree cared about that difference — and the
// only way `lay_out_inner` ever consults `available_height` is by resolving a
// block-axis length against it (`height`/`min-height`/`max-height`, plus the
// aspect-ratio height read), which is what `resolve_block_size` below funnels.
// Set when such a resolution returned `None` *because* the basis was
// indefinite: with a definite one the same site could have produced a value,
// so the probe's result may not be replayed. Also set unconditionally by the
// two dispatch paths that consume `available_height` inside another module
// (vertical writing modes, SVG roots) and are therefore not auditable here.
//
// Callers save (`replace(false)`), run the probe, then restore
// `outer || touched_here`, so a nested probe neither poisons a sibling's reuse
// nor hides the fact from an ancestor deciding about its own — the protocol
// `CV_AUTO_TOUCHED`'s doc comment describes.
thread_local! {
    static INDEFINITE_HEIGHT_CONSULTED: Cell<bool> = const { Cell::new(false) };
}

/// BUG-802, second half — the per-pass memo of what a column item's Step-1
/// probe measured: one `f32` per `(node, available width)`, never a subtree.
///
/// Replaying the probe (see `lay_out_flex`) only helps when the final pass
/// would compute the identical thing. It cannot when flex grow/shrink changed
/// the item's used main size — and a chain of definite-height containers whose
/// items overflow them does exactly that at every level, so the ×2-per-level
/// cost survived there: the item's probe is run once under its parent's own
/// probe and a second time under its parent's final placement pass.
///
/// Those two runs are the same call with the same arguments (both pass
/// `available_height: None`, and the parent's content width does not change
/// between them), so the second one only ever needs the number the first one
/// produced. Remembering the height alone — instead of the laid-out subtree —
/// is what keeps this from being the general layout-result cache BUG-341
/// S28-S33 measured as net-negative: nothing is cloned, and a miss costs one
/// map lookup. The item is still laid out for real by the final placement
/// pass, so its geometry (including anything positioned against a containing
/// block that *did* move) is always computed fresh; only the hypothetical main
/// size is served from here.
type FlexProbeKey = (NodeId, u32);

/// One [`FLEX_COLUMN_PROBE_HEIGHTS`] entry: the style the probe ran with (an
/// `Arc` identity check, same convention as the S31 census and the S36 cache)
/// and the border-box height it produced.
type FlexProbeEntry = (Arc<ComputedStyle>, f32);

thread_local! {
    static FLEX_COLUMN_PROBE_HEIGHTS: RefCell<HashMap<FlexProbeKey, FlexProbeEntry>> =
        RefCell::new(HashMap::new());
    /// Recursion depth of [`lay_out_cache_checked`], so the memo above can be
    /// emptied at the start and end of every layout pass without every entry
    /// point having to remember to do it. A stale entry from an earlier pass
    /// would be served against a box whose *contents* changed while its style
    /// `Arc` stayed the same — the one thing the `ptr_eq` check cannot catch.
    static LAYOUT_PASS_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// RAII marker for one layout pass — see [`LAYOUT_PASS_DEPTH`]. Clears the
/// probe-height memo when the outermost call is entered and again when it
/// returns, so nothing survives into the next pass (or past a panic).
struct LayoutPassGuard;

impl LayoutPassGuard {
    fn enter() -> Self {
        let depth = LAYOUT_PASS_DEPTH.with(|c| {
            let d = c.get() + 1;
            c.set(d);
            d
        });
        if depth == 1 {
            FLEX_COLUMN_PROBE_HEIGHTS.with(|m| m.borrow_mut().clear());
        }
        Self
    }
}

impl Drop for LayoutPassGuard {
    fn drop(&mut self) {
        let depth = LAYOUT_PASS_DEPTH.with(|c| {
            let d = c.get().saturating_sub(1);
            c.set(d);
            d
        });
        if depth == 0 {
            FLEX_COLUMN_PROBE_HEIGHTS.with(|m| m.borrow_mut().clear());
        }
    }
}

/// Resolves a block-axis length against the containing block's content height,
/// recording in [`INDEFINITE_HEIGHT_CONSULTED`] when an indefinite basis is
/// what made the resolution fail. Every `lay_out_inner` read of its
/// `available_height` parameter goes through here — see that flag's comment.
fn resolve_block_size(
    l: &Length,
    em: f32,
    available_height: Option<f32>,
    viewport: Size,
) -> Option<f32> {
    let resolved = l.resolve(em, available_height, viewport);
    if resolved.is_none() && available_height.is_none() {
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(true));
    }
    resolved
}

/// BUG-341 S36 — the layout-result cache S32 built and S33 removed
/// (net-negative at the time, 8.3% hit rate), resurrected now that S34/S35
/// established the precondition it needs (a flex item's `style` `Arc` stays
/// `ptr_eq`-stable across its Step-1 probe and final placement pass in 77.5%
/// of repeat-key calls, not S31's original 23.5%). Same choke point as S32
/// (`lay_out`'s wrapper), extended to also cover [`lay_out_with_used_size`]'s
/// wrapper — a call site S32 predates (`UsedSizeOverride` did not exist yet)
/// but which S35's census proved matters: a Step-1 probe (no override) and a
/// final-pass call (override present) can land on the identical
/// `(node, width, height)` key with the identical style `Arc` while still
/// needing genuinely different results, so [`UsedSizeOverrideBits`] is part
/// of this key, not just an extra guard checked after a hit — two calls that
/// differ only by override must never collide into the same map slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct LayoutResultKey {
    node: NodeId,
    width_bits: u32,
    height_bits: Option<u32>,
    viewport_w_bits: u32,
    viewport_h_bits: u32,
    pcb_x_bits: u32,
    pcb_y_bits: u32,
    pcb_w_bits: u32,
    pcb_h_bits: u32,
    in_block_flow: bool,
    measurer_ptr: usize,
    hp_ptr: usize,
    used_size_override: UsedSizeOverrideBits,
}

/// One cached [`lay_out`]/[`lay_out_with_used_size`] result — same shape as
/// S32's own entry (style `Arc` for the `ptr_eq` correctness check, the
/// origin the subtree's rects are expressed in so a hit can
/// [`crate::incremental::translate_subtree`] to a different origin, and the
/// laid-out subtree itself).
struct LayoutResultEntry {
    style: Arc<ComputedStyle>,
    start_x: f32,
    start_y: f32,
    result: LayoutBox,
}

thread_local! {
    static LAYOUT_RESULT_CACHE_ON: Cell<bool> = const { Cell::new(false) };
    static LAYOUT_RESULT_CACHE: RefCell<HashMap<LayoutResultKey, LayoutResultEntry>> = RefCell::new(HashMap::new());
}

/// Enables/disables the BUG-341 S36 layout-result cache and clears its
/// state — call once before a full (non-incremental) layout pass.
pub fn set_layout_result_cache(on: bool) {
    LAYOUT_RESULT_CACHE_ON.with(|c| c.set(on));
    LAYOUT_RESULT_CACHE.with(|m| m.borrow_mut().clear());
    CV_AUTO_TOUCHED.with(|c| c.set(false));
    LAYOUT_RESULT_CACHE_STATS.with(|c| c.set(LayoutResultCacheStats::default()));
}

/// Whether the BUG-341 S36 layout-result cache is currently enabled.
pub fn layout_result_cache_enabled() -> bool {
    LAYOUT_RESULT_CACHE_ON.with(|c| c.get())
}

/// BUG-341 S36 — per-pass tally of what the cache-checked wrapper did,
/// mirroring S32's own `LayoutResultCacheStats`. `poisoned` counts misses
/// that were *not* stored afterward because [`CV_AUTO_TOUCHED`] fired for
/// that call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutResultCacheStats {
    /// Calls served from the cache instead of recomputed.
    pub hits: u32,
    /// Calls that missed and were computed normally, then cached.
    pub misses: u32,
    /// Calls that missed, were computed normally, and were *not* cached
    /// because the computation touched `content-visibility: auto`.
    pub poisoned: u32,
}

thread_local! {
    static LAYOUT_RESULT_CACHE_STATS: Cell<LayoutResultCacheStats> = const { Cell::new(LayoutResultCacheStats { hits: 0, misses: 0, poisoned: 0 }) };
}

/// Returns the accumulated [`LayoutResultCacheStats`] and resets the tally.
/// Call after a full layout pass with the cache enabled.
pub fn take_layout_result_cache_stats() -> LayoutResultCacheStats {
    LAYOUT_RESULT_CACHE_STATS.with(|c| c.replace(LayoutResultCacheStats::default()))
}

/// BUG-341 S36 — same pre-filters S32 used, checked before building a
/// [`LayoutResultKey`] (which requires reading `b.style`/`b.kind` that a
/// `Skip` box or an active subgrid dispatch shouldn't pay for or shouldn't
/// trust). See S32's original doc comment (git history) for the full
/// rationale — unchanged by S36.
fn cacheable_for_layout_result_cache(b: &LayoutBox) -> bool {
    if matches!(b.kind, BoxKind::Skip) {
        return false;
    }
    let subgrid_active = SUBGRID_COL_CTX.with(|c| c.borrow().is_some())
        || SUBGRID_ROW_CTX.with(|c| c.borrow().is_some());
    !subgrid_active
}

/// Enables/disables incremental box-build reuse for subsequent
/// [`incremental_build_box`] calls on the current thread.
pub fn set_incremental_box_build(enabled: bool) {
    INCREMENTAL_BOX_BUILD.with(|c| c.set(enabled));
}

/// Whether incremental box-build reuse is currently enabled on this thread.
pub fn incremental_box_build_enabled() -> bool {
    INCREMENTAL_BOX_BUILD.with(|c| c.get())
}

/// BUG-341 S4/S15 — per-pass tally of what the box-build stage rebuilt versus
/// reused wholesale from the previous tree.
///
/// The reuse mechanism is invisible in output (a build that reuses nothing
/// produces exactly the same tree, just slowly — the S8 lesson), and wall-clock
/// hides a total regression inside machine noise. These counters are what the
/// S15 gates assert on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoxBuildStats {
    /// `build_box` calls that really constructed a box.
    pub built: u32,
    /// Whole `LayoutBox` subtrees taken from the previous pass instead
    /// (`build_box_or_reuse`'s fast path).
    pub reused: u32,
    /// BUG-341 S19 — boxes of the previous tree that
    /// [`crate::incremental::extract_clean_subtrees`] had to walk to find those
    /// subtrees: the spine above them, not the tree.
    ///
    /// Recorded on the calling thread (extraction runs before `build_box` fans
    /// anything out), and always compiled — it is the counter that tells a
    /// reuse index carved out of the previous tree from S4's `index_by_node`,
    /// which hashed every box in it. A mechanism that regresses to the latter
    /// still produces the same output, just slowly.
    pub prev_index_visited: u32,
    /// BUG-341 S20 — flex/grid containers this pass dispatched onto rayon
    /// workers (the `RAYON_MIN_FLEX_CHILDREN` branch).
    ///
    /// The fan-out is invisible in output — it produces the identical tree —
    /// and its cost is thread-pool overhead, which machine noise hides. Only a
    /// counter can tell "the incremental path stopped dispatching workers for
    /// subtrees it was about to move in O(1)" from "it still does, and the
    /// timing run happened to be quiet".
    pub fanouts: u32,
    /// BUG-341 S25 — times the box-build stage asked what a child's `display`
    /// is (`is_inline_content` / `is_inline_block` and the `display:none`
    /// re-probe inside the inline-collect loop).
    ///
    /// Paired with [`Self::display_probe_cascades`] on purpose: the question has
    /// to keep being asked — it decides which formatting context each child
    /// joins — so a gate that only watched the expensive half could be passed by
    /// deleting the probes outright.
    pub display_probes: u32,
    /// BUG-341 S25 — of those probes, the ones that had to run a full
    /// `compute_style` because the cascade cache had no entry for the node.
    ///
    /// Pure re-derivation when it is not a genuine miss: `precompute_counters`
    /// already cascaded the same node against the same parent style, and
    /// `build_box_inner` builds the child's box out of *that* entry whatever the
    /// probe says. A probe that re-runs the cascade returns the same answer, so
    /// it is invisible in output and only a counter can hold it at zero.
    pub display_probe_cascades: u32,
    /// BUG-341 S25 — `CounterMap::style_arc` misses at the top of
    /// `build_box_inner`, each of which pays a full `compute_style`.
    ///
    /// The cascade records elements only, so every non-element box (whitespace
    /// text between pretty-printed tags, comments) misses by construction. Kept
    /// alongside [`Self::display_probes`] so the two can be told apart: they are
    /// the same cost with different causes, and only one of them is removable.
    pub style_misses: u32,
}

thread_local! {
    /// BUG-341 S4/S15 instrumentation: counts real `build_box` calls vs
    /// whole-subtree reuses via `build_box_or_reuse`, so gates can assert the
    /// incremental path actually skips work (not just that it matches a full
    /// build's output). Always compiled — two `Cell` bumps against a stage that
    /// costs microseconds per box — so a gate outside this crate can read it.
    static BOX_BUILD_STATS: Cell<BoxBuildStats> = const {
        Cell::new(BoxBuildStats {
            built: 0,
            reused: 0,
            prev_index_visited: 0,
            fanouts: 0,
            display_probes: 0,
            display_probe_cascades: 0,
            style_misses: 0,
        })
    };
}

/// Returns the accumulated [`BoxBuildStats`] and resets the tally.
///
/// Thread-local, like the profiler's own tree — `build_box` fans large
/// flex/grid containers out over rayon workers, and each of those drains its
/// own tally back into the thread running the parent container (see the
/// `RAYON_MIN_FLEX_CHILDREN` branch), so the count read on the layout thread is
/// the whole tree's.
pub fn take_box_build_stats() -> BoxBuildStats {
    BOX_BUILD_STATS.with(|s| s.replace(BoxBuildStats::default()))
}

/// BUG-341 S18 — census hook: when on, every real `build_box` call appends the
/// `NodeId` it built to a process-wide log, so a diagnostic can ask *which*
/// boxes a cycle rebuilt, not merely how many.
///
/// Process-wide (a `Mutex`, not a thread-local) on purpose: `build_box` fans
/// large flex/grid containers out over rayon workers, and a thread-local log
/// would silently report only the boxes that happened to be built on the
/// calling thread — the S15 trap. Off by default; the [`AtomicBool`] is checked
/// before the lock so the hot path costs one relaxed load.
static BOX_BUILD_LOG_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The log itself — see [`BOX_BUILD_LOG_ON`].
static BOX_BUILD_LOG: std::sync::Mutex<Vec<NodeId>> = std::sync::Mutex::new(Vec::new());

/// Enables/disables the BUG-341 S18 per-node build census, clearing the log.
///
/// Process-wide, so a test that turns it on must not run concurrently with
/// another layout pass in the same process — the census tests are `#[ignore]`d
/// manual diagnostics for exactly that reason.
pub fn set_box_build_diagnostics(on: bool) {
    if let Ok(mut log) = BOX_BUILD_LOG.lock() {
        log.clear();
    }
    BOX_BUILD_LOG_ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Drains the BUG-341 S18 build census — the `NodeId` of every box really built
/// since the last drain, in completion order (see [`set_box_build_diagnostics`]).
pub fn take_box_build_log() -> Vec<NodeId> {
    BOX_BUILD_LOG.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

/// BUG-341 S20 census: per-built-box **inclusive** wall-clock, paired with the
/// `NodeId` — S18's log answers *which* boxes a cycle rebuilt, this one answers
/// *what each of them cost*.
///
/// Inclusive, not self-time, because `build_box` recurses (and fans large
/// flex/grid containers out over rayon workers, where an "elapsed" reading also
/// covers the join wait). A census that wants self-time subtracts a node's
/// children itself — it has the document and can tell which log entries are
/// descendants; doing that subtraction here would need a parent link the hot
/// path does not carry. Same `Mutex`-not-thread-local reasoning as
/// [`BOX_BUILD_LOG`] (the S15 trap), and the same [`BOX_BUILD_LOG_ON`] gate, so
/// production pays one relaxed load.
static BOX_BUILD_TIME_LOG: std::sync::Mutex<Vec<(NodeId, u64)>> = std::sync::Mutex::new(Vec::new());

/// Gate for [`BOX_BUILD_TIME_LOG`] — deliberately **not** the S18/S19
/// [`BOX_BUILD_LOG_ON`] flag.
///
/// That flag also arms the copy census, whose `count_boxes` walks every reused
/// subtree (299 of chrome's 318 boxes on a keystroke) from inside
/// `build_box_or_reuse` — i.e. from inside the *parent's* `build_box` call. Run
/// together, the copy census would land squarely in the timing census's numbers
/// and make whichever box happens to own the largest reused subtree look like
/// the most expensive box to build. One census must not be measuring the other.
static BOX_TIME_LOG_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enables/disables the BUG-341 S20 per-box timing census, clearing the log.
///
/// Process-wide, same constraint as [`set_box_build_diagnostics`]: a test that
/// turns it on must not run concurrently with another layout pass.
pub fn set_box_time_diagnostics(on: bool) {
    if let Ok(mut log) = BOX_BUILD_TIME_LOG.lock() {
        log.clear();
    }
    BOX_TIME_LOG_ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Drains the BUG-341 S20 per-box timing census — see [`BOX_BUILD_TIME_LOG`].
pub fn take_box_build_time_log() -> Vec<(NodeId, u64)> {
    BOX_BUILD_TIME_LOG.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

/// BUG-341 S18/S19 census: what one incremental box-build pass spent on
/// *copying and indexing* the previous tree, as opposed to building boxes.
///
/// Only accumulated while [`set_box_build_diagnostics`] is on — both halves
/// need their own traversal of the previous tree, which must not run in
/// production. Drained by [`take_box_copy_stats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoxCopyStats {
    /// Nanoseconds spent taking whole reusable subtrees out of the previous
    /// tree inside `build_box_or_reuse` (a deep `clone` before S19, an O(1)
    /// move after it).
    pub reuse_ns: u64,
    /// Boxes those reuses carried over — the size of the reused region,
    /// whether it was copied or moved.
    pub reuse_boxes: u64,
    /// Nanoseconds spent in [`crate::incremental::extract_clean_subtrees`]
    /// building the id→subtree index the reuses draw from.
    pub index_ns: u64,
    /// Boxes that index walk visited. Before S19 this was the whole previous
    /// tree (`index_by_node` hashed every box); after it, only the spine above
    /// the reusable subtrees.
    pub index_boxes: u64,
}

static BOX_CLONE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static BOX_CLONE_BOXES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PREV_INDEX_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PREV_INDEX_BOXES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Drains the BUG-341 S18/S19 copy census — see [`BoxCopyStats`].
pub fn take_box_copy_stats() -> BoxCopyStats {
    use std::sync::atomic::Ordering::Relaxed;
    BoxCopyStats {
        reuse_ns: BOX_CLONE_NS.swap(0, Relaxed),
        reuse_boxes: BOX_CLONE_BOXES.swap(0, Relaxed),
        index_ns: PREV_INDEX_NS.swap(0, Relaxed),
        index_boxes: PREV_INDEX_BOXES.swap(0, Relaxed),
    }
}

/// Whether the S18/S19 copy census is on — see [`set_box_build_diagnostics`].
pub(crate) fn box_build_diagnostics_on() -> bool {
    BOX_BUILD_LOG_ON.load(std::sync::atomic::Ordering::Relaxed)
}

/// Records one index-build pass in the census (see [`BoxCopyStats`]).
pub(crate) fn note_prev_index(ns: u64, boxes: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    PREV_INDEX_NS.fetch_add(ns, Relaxed);
    PREV_INDEX_BOXES.fetch_add(boxes, Relaxed);
}

/// Number of boxes in `b`'s subtree, inclusive — census only.
fn count_boxes(b: &LayoutBox) -> u64 {
    1 + b.children.iter().map(count_boxes).sum::<u64>()
}

/// BUG-341 S25 census: nanoseconds the box-build stage spent inside the
/// `compute_style` calls tallied by [`BoxBuildStats::display_probes`] and
/// [`BoxBuildStats::style_misses`], respectively.
///
/// Counted only while the S20 timing census is armed ([`BOX_TIME_LOG_ON`]) —
/// the counts themselves are always compiled, because the S25 gate asserts on
/// them, but a timer per probe has no business on the production path.
/// Process-wide atomics rather than thread-locals: the probes run on rayon
/// workers too (the S15 trap).
static PROBE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static STYLE_MISS_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Drains the BUG-341 S25 probe timers — see [`PROBE_NS`] / [`STYLE_MISS_NS`].
pub fn take_box_probe_ns() -> (u64, u64) {
    use std::sync::atomic::Ordering::Relaxed;
    (PROBE_NS.swap(0, Relaxed), STYLE_MISS_NS.swap(0, Relaxed))
}

/// Runs `f` as the `compute_style` fallback of a `display` probe, tallying it
/// (see [`BoxBuildStats::display_probe_cascades`]) and, when the S20 timing
/// census is armed, timing it.
fn note_display_probe<T>(f: impl FnOnce() -> T) -> T {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.display_probe_cascades += 1;
        s.set(v);
    });
    if !BOX_TIME_LOG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return f();
    }
    let t = std::time::Instant::now();
    let out = f();
    PROBE_NS.fetch_add(t.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    out
}

/// Runs `f` as the `compute_style` fallback of a [`CounterMap::style_arc`] miss,
/// tallying it (see [`BoxBuildStats::style_misses`]) and timing it under the S20
/// census, exactly like [`note_display_probe`].
fn note_style_miss<T>(f: impl FnOnce() -> T) -> T {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.style_misses += 1;
        s.set(v);
    });
    if !BOX_TIME_LOG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return f();
    }
    let t = std::time::Instant::now();
    let out = f();
    STYLE_MISS_NS.fetch_add(t.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    out
}

/// Records `id` in the census log when [`set_box_build_diagnostics`] is on.
fn note_box_built(id: NodeId) {
    if BOX_BUILD_LOG_ON.load(std::sync::atomic::Ordering::Relaxed)
        && let Ok(mut log) = BOX_BUILD_LOG.lock()
    {
        log.push(id);
    }
}

/// Folds `d` into the current thread's [`BoxBuildStats`] tally.
fn add_box_build_stats(d: BoxBuildStats) {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.built += d.built;
        v.reused += d.reused;
        v.prev_index_visited += d.prev_index_visited;
        v.fanouts += d.fanouts;
        v.display_probes += d.display_probes;
        v.display_probe_cascades += d.display_probe_cascades;
        v.style_misses += d.style_misses;
        s.set(v);
    });
}

/// Layout-side gutter width for `scrollbar-width: auto` in CSS px.
///
/// Must match `SCROLLBAR_WIDTH` constant in `lumen_paint::display_list` so that
/// the space reserved in layout equals the painted scrollbar track width.
// CSS: scrollbar-width — P4: if the paint-side constant changes, update this too.
const SCROLLBAR_GUTTER_AUTO: f32 = 12.0;

/// Layout-side gutter width for `scrollbar-width: thin` in CSS px.
///
/// Must match `SCROLLBAR_WIDTH_THIN` in `lumen_paint::display_list`.
// CSS: scrollbar-width — P4: keep in sync with SCROLLBAR_WIDTH_THIN in display_list.rs.
const SCROLLBAR_GUTTER_THIN: f32 = 6.0;

/// CSS Scrollbars L1 §6.2 — inline-axis (horizontal) scrollbar gutter reservation.
///
/// Lumen renders overlay scrollbars, so by default (`scrollbar-gutter: auto`)
/// the scrollbar track overlaps content and no space is reserved in layout.
/// With `scrollbar-gutter: stable`, the UA must always reserve the gutter to
/// prevent layout shift when the scrollbar appears or disappears.
///
/// Returns the CSS px width to subtract from `content_width` before laying out children.
/// Only non-zero when `overflow-y` is `scroll` or `auto` AND `scrollbar-gutter` is `stable`
/// or `stable both-edges` AND `scrollbar-width` is not `none`.
///
// CSS: scrollbar-width, scrollbar-gutter — P4: verify SCROLLBAR_GUTTER_* match
// SCROLLBAR_WIDTH / SCROLLBAR_WIDTH_THIN in lumen_paint::display_list.
fn scrollbar_gutter_inline(s: &ComputedStyle) -> f32 {
    let can_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto);
    if !can_scroll_y {
        return 0.0;
    }
    let unit = match s.scrollbar_width {
        ScrollbarWidth::None => return 0.0,
        ScrollbarWidth::Auto => SCROLLBAR_GUTTER_AUTO,
        ScrollbarWidth::Thin => SCROLLBAR_GUTTER_THIN,
    };
    match s.scrollbar_gutter {
        ScrollbarGutter::Auto => 0.0,
        // `stable` reserves gutter on the end edge only.
        ScrollbarGutter::Stable => unit,
        // `stable both-edges` mirrors the gutter on the start edge as well
        // so the content remains centred even when the scrollbar appears.
        ScrollbarGutter::StableBothEdges => unit * 2.0,
    }
}

/// CSS Scrollbars L1 §6.2 — block-axis (vertical) scrollbar gutter reservation.
///
/// Returns the CSS px height to subtract from available content height when a
/// horizontal scrollbar's gutter must be reserved (`overflow-x: scroll/auto` +
/// `scrollbar-gutter: stable`). `both-edges` is not defined for the block axis
/// by the spec, so only one gutter unit is reserved regardless.
///
// CSS: scrollbar-width, scrollbar-gutter — the block-axis gutter reduces the
// content height handed to children (see `children_available_height`), mirroring
// the inline-axis `scrollbar_gutter_inline` reduction of `content_width`.
fn scrollbar_gutter_block(s: &ComputedStyle) -> f32 {
    let can_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto);
    if !can_scroll_x {
        return 0.0;
    }
    let unit = match s.scrollbar_width {
        ScrollbarWidth::None => return 0.0,
        ScrollbarWidth::Auto => SCROLLBAR_GUTTER_AUTO,
        ScrollbarWidth::Thin => SCROLLBAR_GUTTER_THIN,
    };
    match s.scrollbar_gutter {
        ScrollbarGutter::Auto => 0.0,
        ScrollbarGutter::Stable | ScrollbarGutter::StableBothEdges => unit,
    }
}

/// HTML-имя элемента `<img>` для распознавания replaced-боксов в layout.
/// Tag-name в DOM хранится lower-case (HTML5 tree-builder), поэтому
/// сравнение точное, без `eq_ignore_ascii_case`.
fn is_image_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "img"
    )
}

/// HTML-имя `<video>` для распознавания media replaced-боксов в layout.
fn is_video_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "video"
    )
}

/// HTML-имя `<canvas>` для распознавания replaced-боксов рисовалки в layout.
fn is_canvas_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "canvas"
    )
}

/// HTML-имя `<audio>` для распознавания media replaced-боксов в layout.
fn is_audio_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "audio"
    )
}

/// HTML-имя `<iframe>` для распознавания встроенных документов в layout.
fn is_iframe_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "iframe"
    )
}

/// HTML-имя `<picture>` — обёртка над `<source>`-кандидатами и одним
/// `<img>`-fallback-ом. Сам по себе пиктур ничего не рендерит, его
/// единственная роль — переадресовать source-selection на inner `<img>`.
fn is_picture_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "picture"
    )
}

/// SVG `viewBox="min-x min-y width height"` attribute. Maps SVG user-unit space
/// to the CSS pixel rect of the `<svg>` element. All four values are in SVG user units.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewBox {
    /// Left edge of the SVG viewport in user units.
    pub min_x: f32,
    /// Top edge of the SVG viewport in user units.
    pub min_y: f32,
    /// Width of the SVG viewport in user units (> 0).
    pub width: f32,
    /// Height of the SVG viewport in user units (> 0).
    pub height: f32,
}

/// SVG `preserveAspectRatio` attribute for aspect-ratio preservation.
/// Controls how viewBox scales to fit the SVG's CSS width/height.
/// Default is `xMidYMid` with uniform scaling.
#[derive(Debug, Clone, PartialEq)]
pub struct PreserveAspectRatio {
    /// Horizontal alignment: `xMin` (left), `xMid` (center), `xMax` (right).
    pub align_x: SvgAlignX,
    /// Vertical alignment: `YMin` (top), `YMid` (middle), `YMax` (bottom).
    pub align_y: SvgAlignY,
    /// Uniform scaling (`Uniform`) or stretch to fill (`NonUniform`).
    pub meet_or_slice: SvgMeetOrSlice,
}

/// SVG preserveAspectRatio horizontal alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgAlignX {
    /// `xMin` — align viewBox to left edge.
    Min,
    /// `xMid` — align viewBox to center (default).
    Mid,
    /// `xMax` — align viewBox to right edge.
    Max,
}

/// SVG preserveAspectRatio vertical alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgAlignY {
    /// `YMin` — align viewBox to top edge.
    Min,
    /// `YMid` — align viewBox to center (default).
    Mid,
    /// `YMax` — align viewBox to bottom edge.
    Max,
}

/// SVG preserveAspectRatio meet-or-slice mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgMeetOrSlice {
    /// `meet` (default) — uniform scale to fit inside, may have letterboxing.
    Meet,
    /// `slice` — uniform scale to cover, may clip.
    Slice,
}

/// SVG `text-anchor` attribute for text horizontal alignment.
/// Controls how text is anchored at the specified x position (SVG L1 §10.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvgTextAnchor {
    /// `start` (default) — text starts at the x position.
    #[default]
    Start,
    /// `middle` — text center is at the x position.
    Middle,
    /// `end` — text ends at the x position.
    End,
}

/// SVG `dominant-baseline` attribute for text vertical alignment.
/// Controls how text is anchored at the specified y position (SVG L1 §10.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvgDominantBaseline {
    /// `auto` (default) — dominant baseline is determined by the text.
    #[default]
    Auto,
    /// `baseline` — use the alphabetic baseline of the text.
    Baseline,
    /// `hanging` — use the hanging baseline (e.g., for Devanagari scripts).
    Hanging,
    /// `middle` — use the middle of the em-box.
    Middle,
    /// `central` — use the central baseline (midpoint between ascender and descender).
    Central,
    /// `text-before-edge` — use the top of the em-box.
    TextBeforeEdge,
    /// `text-after-edge` — use the bottom of the em-box.
    TextAfterEdge,
}

/// SVG 1.1 §10.9.2 / CSS Inline Layout L3 §5.2 — `baseline-shift`. Vertical shift
/// of the text baseline relative to the dominant baseline of the parent.
/// NOT inherited; initial `baseline` (no shift). Positive lengths/percentages
/// *raise* the text (shift up, toward smaller `y`); `sub` lowers and `super`
/// raises by an approximate sub/superscript offset.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SvgBaselineShift {
    /// `baseline` (initial) — no shift.
    #[default]
    Baseline,
    /// `sub` — lower the baseline to the subscript position.
    Sub,
    /// `super` — raise the baseline to the superscript position.
    Super,
    /// `<length>` in user units. Positive raises the text (shifts up).
    Length(f32),
    /// `<percentage>` as a fraction of the current font-size. Positive raises.
    Percentage(f32),
}

/// SVG transformation data from the `transform` presentation attribute.
/// Stores parsed transform functions in order of application.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SvgTransform {
    /// Transform matrix components: [a, b, c, d, e, f] representing the 2D transformation matrix.
    /// Default is identity matrix [1, 0, 0, 1, 0, 0].
    pub matrix: [f32; 6],
}

impl SvgTransform {
    /// Creates an identity transform (no transformation).
    pub fn identity() -> Self {
        SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] }
    }

    /// Creates a translation transform.
    pub fn translate(tx: f32, ty: f32) -> Self {
        SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, tx, ty] }
    }

    /// Multiplies this transform by another, composing them.
    pub fn compose(&mut self, other: &SvgTransform) {
        let [a, b, c, d, e, f] = self.matrix;
        let [a2, b2, c2, d2, e2, f2] = other.matrix;
        // Matrix multiplication: self × other
        self.matrix = [
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ];
    }

    /// Applies this transform to a point (x, y).
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.matrix;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

/// Geometric primitive for an SVG shape element in SVG user units (before viewBox scaling).
/// Coordinate origin: top-left of the SVG viewport.
#[derive(Debug, Clone, PartialEq)]
pub enum SvgShapeKind {
    /// `<rect x y width height rx ry>`. Corner radii `rx`/`ry` default to 0 (sharp corners).
    Rect { x: f32, y: f32, width: f32, height: f32, rx: f32, ry: f32 },
    /// `<circle cx cy r>`. Center at (cx, cy), radius r.
    Circle { cx: f32, cy: f32, r: f32 },
    /// `<ellipse cx cy rx ry>`. Center at (cx, cy), horizontal radius rx, vertical ry.
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    /// `<line x1 y1 x2 y2>`. Segment from (x1,y1) to (x2,y2).
    Line { x1: f32, y1: f32, x2: f32, y2: f32 },
    /// `<path d="...">`. SVG path data string; bounding box computed by paint.
    /// CSS: fill, stroke, stroke-width — P4 wires via ComputedStyle svg_fill/svg_stroke.
    Path { d: String },
}

/// Вид form control — используется в `BoxKind::FormControl` для paint-специализаций
/// (фокус-рамка, checkbox/radio indicator, placeholder, стрелка select и т.д.).
#[derive(Debug, Clone, PartialEq)]
pub enum FormControlKind {
    /// `<input>` — carries input type (from `type` attribute) and initial
    /// checked state (from presence of `checked` attribute in DOM). Paint uses
    /// this to draw checkbox/radio indicators without re-querying the DOM.
    /// `value_text` is the `value` attribute content, used by `field-sizing: content`.
    /// `placeholder` is the `placeholder` attribute content, painted in grey by
    /// text-like inputs when `value_text` is empty (HTML rendering §15.5.5).
    /// `placeholder_style` is the computed `::placeholder` override (CSS
    /// Pseudo-Elements L4 §4.10), if any author rule targets
    /// `input::placeholder` — `None` falls back to the UA default grey hint.
    Input {
        input_type: lumen_dom::InputType,
        checked: bool,
        value_text: String,
        placeholder: String,
        placeholder_style: Option<Box<ComputedStyle>>,
    },
    Button,
    /// `<select>` — `selected_text` is the label of the currently selected
    /// `<option>` (first option if none is explicitly selected). Paint uses this
    /// to draw the visible label without re-querying the DOM.
    Select { selected_text: String },
    /// `<textarea>` — `value_text` is the text content of all direct text children,
    /// used by `field-sizing: content` to compute intrinsic dimensions.
    Textarea { value_text: String },
    /// `<input type="range">` — carries current value and bounds so paint can
    /// draw track / fill / thumb without re-querying the DOM.
    Range {
        /// Current slider value clamped to [min, max].
        value: f32,
        /// Minimum bound (HTML `min` attribute; default 0).
        min: f32,
        /// Maximum bound (HTML `max` attribute; default 100).
        max: f32,
    },
    /// `<progress>` — determinate or indeterminate progress bar.
    ///
    /// `value` is `None` when the `value` attribute is absent (indeterminate).
    /// Paint draws a filled bar (blue) proportional to `value / max`, or a
    /// static partial fill for indeterminate.
    Progress {
        /// Current value clamped to [0, max]; `None` = indeterminate.
        value: Option<f32>,
        /// Maximum value (HTML `max` attribute; default 1.0).
        max: f32,
    },
    /// `<meter>` — gauge bar whose fill color reflects optimality (HTML5 §4.10.14).
    ///
    /// Color: green = optimal zone, yellow = sub-optimal, red = bad.
    Meter {
        /// Current value clamped to [min, max].
        value: f32,
        /// Minimum bound (HTML `min` attribute; default 0.0).
        min: f32,
        /// Maximum bound (HTML `max` attribute; default 1.0).
        max: f32,
        /// Low threshold: below `low` is the "low" segment (default = min).
        low: f32,
        /// High threshold: above `high` is the "high" segment (default = max).
        high: f32,
        /// Optimal value — determines which segment is colored green (default = midpoint).
        optimum: f32,
    },
}

/// Collect the text label of the currently selected `<option>` inside a
/// `<select>` element. Returns the text of the first `<option selected>` child,
/// falling back to the first `<option>` child, then an empty string.
fn collect_select_label(doc: &Document, select_id: NodeId) -> String {
    let children = doc.get(select_id).children.clone();
    let mut first_label: Option<String> = None;
    for child_id in children {
        let child = doc.get(child_id);
        let NodeData::Element { name, attrs, .. } = &child.data else { continue };
        if name.local.as_str() != "option" { continue }
        let label = option_text(doc, child_id);
        let is_selected = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("selected"));
        if is_selected {
            return label;
        }
        if first_label.is_none() {
            first_label = Some(label);
        }
    }
    first_label.unwrap_or_default()
}

/// Collect the selected `<option>` label from a `<selectlist>` element.
///
/// `<selectlist>` may contain `<option>` elements directly or nested inside a
/// `<listbox>` child (Customizable Select §3.1). Searches both levels.
/// Returns the first `<option selected>` text, falling back to the first
/// `<option>` text, or an empty string if no options are present.
///
/// Phase 0 layout stub — renders like a native `<select>` widget.
/// `// CSS: appearance: base-select` — P4 wires ::picker(select) styling.
pub fn collect_selectlist_label(doc: &Document, sl_id: NodeId) -> String {
    // Gather direct <option> children and <option> children inside <listbox>.
    let mut option_ids: Vec<NodeId> = Vec::new();
    for &child_id in &doc.get(sl_id).children.clone() {
        let child = doc.get(child_id);
        let NodeData::Element { name, .. } = &child.data else { continue };
        if name.local.as_str() == "option" {
            option_ids.push(child_id);
        } else if name.local.as_str() == "listbox" {
            for &gc_id in &child.children.clone() {
                let gc = doc.get(gc_id);
                let NodeData::Element { name: gcn, .. } = &gc.data else { continue };
                if gcn.local.as_str() == "option" {
                    option_ids.push(gc_id);
                }
            }
        }
    }
    let mut first_label: Option<String> = None;
    for opt_id in option_ids {
        let opt = doc.get(opt_id);
        let NodeData::Element { attrs, .. } = &opt.data else { continue };
        let label = option_text(doc, opt_id);
        let is_selected = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("selected"));
        if is_selected {
            return label;
        }
        if first_label.is_none() {
            first_label = Some(label);
        }
    }
    first_label.unwrap_or_default()
}

/// Returns `true` when `node` is a `<selectlist>` element (Customizable Select).
///
/// Used by layout to render `<selectlist>` as a form control widget (Phase 0
/// fallback — same appearance as `<select>`).
pub fn is_selectlist(doc: &Document, node: NodeId) -> bool {
    matches!(
        &doc.get(node).data,
        NodeData::Element { name, .. } if name.local.as_str() == "selectlist"
    )
}

/// Returns the display text for an `<option>` element: `label` attribute if
/// present, otherwise the concatenated text content of its child text nodes.
fn option_text(doc: &Document, option_id: NodeId) -> String {
    let node = doc.get(option_id);
    if let NodeData::Element { attrs, .. } = &node.data
        && let Some(label) = attrs.iter().find(|a| a.name.local.eq_ignore_ascii_case("label"))
    {
        return label.value.trim().to_owned();
    }
    node.children
        .iter()
        .filter_map(|&c| {
            if let NodeData::Text(t) = &doc.get(c).data { Some(t.as_str()) } else { None }
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_owned()
}

/// Является ли DOM-узел HTML form control-ом.
/// Tag-name хранится lower-case (HTML5 tree-builder).
fn is_form_control_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. }
            if matches!(name.local.as_str(), "input" | "button" | "select" | "selectlist" | "textarea" | "meter" | "progress")
    )
}

/// Финальный URL картинки + author-объявленные intrinsic dimensions.
/// Заполняется `resolve_image_source` ниже — это адаптер `PickedSource`
/// из `lumen-html-parser`, плюс legacy-fallback на голый `src`-атрибут
/// для битых страниц, у которых picker отказал.
struct ImageSource {
    url: String,
    intrinsic_width: Option<u32>,
    intrinsic_height: Option<u32>,
}

// ─── SVG helpers ─────────────────────────────────────────────────────────────

/// Returns `true` when `id` is an `<svg>` element.
/// Note: the HTML5 parser does not yet implement foreign-content mode, so all
/// elements (including SVG ones) are created with `Namespace::Html`. We detect
/// SVG elements by local name until the parser gains full foreign-content support.
fn is_svg_root(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("svg")
    )
}

/// Returns `true` when `id` is an SVG `<defs>` element (invisible container).
#[allow(dead_code)]
fn is_svg_defs(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("defs")
    )
}

/// Returns `true` when `id` is an SVG `<use>` element (reference to another element).
#[allow(dead_code)]
fn is_svg_use(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("use")
    )
}

/// Returns `true` when `id` is a `<details>` element.
fn is_details_element(doc: &Document, id: NodeId) -> bool {
    matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == "details")
}

/// Returns `true` when `id` is a `<summary>` element.
fn is_summary_element(doc: &Document, id: NodeId) -> bool {
    matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == "summary")
}

/// Returns `true` when `id` is a `<details>` element with the `open` attribute set.
///
/// HTML LS §4.11.1: when `open` is absent only `<summary>` is rendered; when present all
/// children are visible. External callers (paint, a11y) use this to query disclosure state.
pub fn is_open_details(doc: &Document, id: NodeId) -> bool {
    is_details_element(doc, id) && doc.get(id).get_attr("open").is_some()
}

/// Returns `true` when `id` has a `popover` attribute but is not open.
///
/// Elements with `popover` are hidden by default (UA: `[popover]{display:none}`);
/// JS calls `showPopover()` which sets `data-lumen-popover-open` to expose the element.
fn is_closed_popover(doc: &Document, id: NodeId) -> bool {
    let node = doc.get(id);
    node.get_attr("popover").is_some() && node.get_attr("data-lumen-popover-open").is_none()
}

/// Parses a float attribute from the given element; returns 0.0 if absent or non-numeric.
fn svg_attr_f32(doc: &Document, id: NodeId, attr: &str) -> f32 {
    doc.get(id)
        .get_attr(attr)
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// Parses the SVG `viewBox="min-x min-y width height"` attribute.
/// Returns `None` if the attribute is absent or malformed.
fn parse_view_box(doc: &Document, id: NodeId) -> Option<ViewBox> {
    let s = doc.get(id).get_attr("viewBox")?;
    let vals: Vec<f32> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<f32>().ok())
        .collect();
    if vals.len() < 4 || vals[2] <= 0.0 || vals[3] <= 0.0 {
        return None;
    }
    Some(ViewBox { min_x: vals[0], min_y: vals[1], width: vals[2], height: vals[3] })
}

/// Parses an SVG `points="x1,y1 x2,y2 ..."` list (commas and/or whitespace as
/// separators, SVG 1.1 §9.7) into a flat coordinate list, then groups it into
/// `(x, y)` pairs. A trailing lone coordinate is dropped.
fn parse_svg_points(s: &str) -> Vec<(f32, f32)> {
    let nums: Vec<f32> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<f32>().ok())
        .collect();
    nums.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// Builds an SVG path `d` string from a `points` list. `<polygon>` closes the
/// contour with `Z`; `<polyline>` leaves it open. Returns `None` when fewer than
/// two points are present (nothing renderable). Reusing the `<path>` pipeline
/// keeps polygon/polyline fill, stroke and joins consistent with `<path>`.
fn points_to_path_d(points: &[(f32, f32)], close: bool) -> Option<String> {
    if points.len() < 2 {
        return None;
    }
    let mut d = String::with_capacity(points.len() * 12);
    for (i, (x, y)) in points.iter().enumerate() {
        if i == 0 {
            d.push_str(&format!("M {x} {y}"));
        } else {
            d.push_str(&format!(" L {x} {y}"));
        }
    }
    if close {
        d.push_str(" Z");
    }
    Some(d)
}

/// Parses the SVG `preserveAspectRatio` attribute.
/// Format: `[defer] <align> [meet|slice]`
/// Default is `xMidYMid meet` (center, uniform scale, fit inside).
fn parse_preserve_aspect_ratio(doc: &Document, id: NodeId) -> PreserveAspectRatio {
    let s = match doc.get(id).get_attr("preserveAspectRatio") {
        Some(s) => s.trim(),
        None => "xMidYMid meet",
    };

    // Skip optional "defer" keyword at start.
    let s = s.strip_prefix("defer ").unwrap_or(s);

    // Parse align and meet-or-slice.
    let parts: Vec<&str> = s.split_whitespace().collect();
    let align_str = parts.first().copied().unwrap_or("xMidYMid");
    let meet_or_slice_str = parts.get(1).copied().unwrap_or("meet");

    // Parse alignment (e.g. "xMidYMid", "xMinYMin", etc.).
    let (align_x, align_y) = if align_str == "none" {
        // "none" means non-uniform scaling — not implemented yet, fall back to uniform.
        (SvgAlignX::Mid, SvgAlignY::Mid)
    } else {
        // Extract x-align from prefix: xMin|xMid|xMax.
        let align_x = if align_str.starts_with("xMin") {
            SvgAlignX::Min
        } else if align_str.starts_with("xMax") {
            SvgAlignX::Max
        } else {
            SvgAlignX::Mid
        };
        // Extract y-align from suffix: YMin|YMid|YMax.
        let align_y = if align_str.contains("YMin") {
            SvgAlignY::Min
        } else if align_str.contains("YMax") {
            SvgAlignY::Max
        } else {
            SvgAlignY::Mid
        };
        (align_x, align_y)
    };

    let meet_or_slice = if meet_or_slice_str == "slice" {
        SvgMeetOrSlice::Slice
    } else {
        SvgMeetOrSlice::Meet
    };

    PreserveAspectRatio { align_x, align_y, meet_or_slice }
}

/// Parses the SVG `transform` presentation attribute and returns a composed transform matrix.
/// Syntax: `<transform-function> [ <transform-function> ]* | none`
/// Supported functions: translate, scale, rotate, skewX, skewY, matrix.
fn parse_svg_transform(attr: Option<&str>) -> SvgTransform {
    let attr = match attr {
        Some(s) => s.trim(),
        None => return SvgTransform::identity(),
    };

    if attr.eq_ignore_ascii_case("none") {
        return SvgTransform::identity();
    }

    let mut result = SvgTransform::identity();

    // Simple regex-free parser: extract function names and their arguments.
    let mut pos = 0;
    let attr_bytes = attr.as_bytes();

    while pos < attr_bytes.len() {
        // Skip whitespace and commas. BUG-803: `&&` binds tighter than `||`,
        // so an unparenthesized condition reads as
        // `(pos < len && ws) || attr_bytes[pos] == b','` — once `pos == len`
        // the first disjunct is false but the second still indexes past the
        // end of the slice. Both checks must be gated by the length check.
        while pos < attr_bytes.len()
            && ((attr_bytes[pos] as char).is_whitespace() || attr_bytes[pos] == b',')
        {
            pos += 1;
        }

        if pos >= attr_bytes.len() {
            break;
        }

        // Extract function name.
        let start = pos;
        while pos < attr_bytes.len() && (attr_bytes[pos] as char).is_alphabetic() {
            pos += 1;
        }

        let func_name = &attr[start..pos];

        // Skip whitespace and opening paren.
        while pos < attr_bytes.len() && (attr_bytes[pos] as char).is_whitespace() {
            pos += 1;
        }

        if pos >= attr_bytes.len() || attr_bytes[pos] != b'(' {
            // BUG-803: a byte that is neither a letter, whitespace, a comma
            // nor `(` (an underscore, a digit, `;`, `|`, ...) leaves both the
            // name loop above and this branch without moving `pos` — `continue`
            // then re-enters this exact position forever. Force one byte of
            // progress whenever the name loop itself made none, so a name
            // that already advanced (e.g. `translate` before the `3` of
            // `translate3d`) still gets a second chance next iteration
            // instead of being force-skipped mid-token.
            if pos == start {
                pos += 1;
            }
            continue;
        }

        pos += 1; // skip '('

        // Extract arguments until closing paren.
        let args_start = pos;
        let mut depth = 1;
        while pos < attr_bytes.len() && depth > 0 {
            if attr_bytes[pos] == b'(' {
                depth += 1;
            } else if attr_bytes[pos] == b')' {
                depth -= 1;
            }
            if depth > 0 {
                pos += 1;
            }
        }

        let args_str = attr[args_start..pos].trim();
        let args: Vec<f32> = args_str
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();

        // Apply the transform function.
        let fn_transform = match func_name.to_lowercase().as_str() {
            "translate" => {
                let tx = args.first().copied().unwrap_or(0.0);
                let ty = args.get(1).copied().unwrap_or(0.0);
                SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, tx, ty] }
            }
            "scale" => {
                let sx = args.first().copied().unwrap_or(1.0);
                let sy = args.get(1).copied().unwrap_or(sx);
                SvgTransform { matrix: [sx, 0.0, 0.0, sy, 0.0, 0.0] }
            }
            "rotate" => {
                let angle = args.first().copied().unwrap_or(0.0); // in degrees
                let rad = angle.to_radians();
                let cos = rad.cos();
                let sin = rad.sin();
                // Optional cx, cy for rotation center.
                let cx = args.get(1).copied().unwrap_or(0.0);
                let cy = args.get(2).copied().unwrap_or(0.0);
                if cx.abs() < 0.001 && cy.abs() < 0.001 {
                    SvgTransform { matrix: [cos, sin, -sin, cos, 0.0, 0.0] }
                } else {
                    // rotate(a cx cy) = translate(cx cy) · rotate(a) · translate(-cx -cy).
                    // `compose` is `self × other` (other is applied first to a point),
                    // so the list must be accumulated left-to-right starting from the
                    // outermost translate. The previous code started from `R` and
                    // post-composed both translates, which cancel out
                    // (R · T(cx,cy) · T(-cx,-cy) = R) — silently dropping the rotation
                    // centre (BUG-244).
                    let mut m = SvgTransform::translate(cx, cy);
                    m.compose(&SvgTransform { matrix: [cos, sin, -sin, cos, 0.0, 0.0] });
                    m.compose(&SvgTransform::translate(-cx, -cy));
                    m
                }
            }
            "skewx" => {
                let angle = args.first().copied().unwrap_or(0.0);
                let tan = angle.to_radians().tan();
                SvgTransform { matrix: [1.0, 0.0, tan, 1.0, 0.0, 0.0] }
            }
            "skewy" => {
                let angle = args.first().copied().unwrap_or(0.0);
                let tan = angle.to_radians().tan();
                SvgTransform { matrix: [1.0, tan, 0.0, 1.0, 0.0, 0.0] }
            }
            "matrix" => {
                if let [a, b, c, d, e, f, ..] = args.as_slice() {
                    SvgTransform { matrix: [*a, *b, *c, *d, *e, *f] }
                } else {
                    SvgTransform::identity()
                }
            }
            _ => SvgTransform::identity(),
        };

        result.compose(&fn_transform);

        if pos < attr_bytes.len() && attr_bytes[pos] == b')' {
            pos += 1;
        }
    }

    result
}

/// Calculates the intrinsic aspect ratio from SVG viewBox.
/// Returns `Some(width / height)` if viewBox is present and both dimensions > 0.
#[allow(dead_code)]
fn svg_intrinsic_ratio(view_box: &Option<ViewBox>) -> Option<f32> {
    view_box.as_ref().and_then(|vb| {
        if vb.width > 0.0 && vb.height > 0.0 {
            Some(vb.width / vb.height)
        } else {
            None
        }
    })
}

/// Collects text content from an SVG text element and its descendants.
/// Recursively walks the DOM tree, concatenating text nodes and content of nested `<tspan>` elements.
fn collect_text_content(doc: &Document, node_id: NodeId) -> String {
    let mut text = String::new();
    let node = doc.get(node_id);

    // Walk through immediate children and concatenate text.
    for child_id in node.children.iter() {
        let child = doc.get(*child_id);
        match &child.data {
            NodeData::Text(s) => {
                // Text node: add content.
                text.push_str(s);
            }
            NodeData::Element { name, .. }
                if name.local.as_str() == "tspan" || name.local.as_str() == "textPath" =>
            {
                // For element nodes like <tspan>, recursively collect their text.
                text.push_str(&collect_text_content(doc, *child_id));
            }
            _ => {}
        }
    }

    text
}

/// Collects the text content of a `<textarea>` from its direct text-node children.
///
/// Used by `field-sizing: content` to determine the intrinsic size of the control.
/// Newlines are preserved (each `\n` becomes a line for height computation).
fn collect_textarea_content(doc: &Document, node_id: NodeId) -> String {
    // BUG-441: what a textarea *shows* is its current value — the child text is
    // only the default, replaced as soon as the user types or a script assigns
    // `el.value` (HTML LS §4.10.11).
    if let Some(dirty) = doc.dirty_value(node_id) {
        return dirty.to_owned();
    }
    let mut text = String::new();
    let node = doc.get(node_id);
    for child_id in node.children.iter() {
        if let NodeData::Text(s) = &doc.get(*child_id).data {
            text.push_str(s);
        }
    }
    text
}


/// Maps an SVG `viewBox` into the SVG viewport using the `preserveAspectRatio`
/// attribute (SVG 1.1 §7.8). Inline `<svg>` ignores CSS `object-fit`/`object-position`
/// (those govern replaced content only); browsers fit the viewBox per this attribute.
/// Returns `(scale_x, scale_y, origin_dx, origin_dy)` where `origin_d*` is the
/// document-space offset of the viewBox origin from the viewport's top-left corner —
/// same shape as [`compute_object_fit_transform`] so the caller is unchanged. BUG-198.
fn compute_preserve_aspect_ratio_transform(
    view_box: &ViewBox,
    box_w: f32,
    box_h: f32,
    par: &PreserveAspectRatio,
) -> (f32, f32, f32, f32) {
    let vb_w = view_box.width.max(0.001);
    let vb_h = view_box.height.max(0.001);
    let raw_sx = box_w / vb_w;
    let raw_sy = box_h / vb_h;

    // `meet` → uniform scale fitting inside (contain); `slice` → uniform scale
    // covering (cover). Lumen has no `preserveAspectRatio="none"` variant, so
    // non-uniform fill never occurs here.
    let (sx, sy) = match par.meet_or_slice {
        SvgMeetOrSlice::Meet  => { let s = raw_sx.min(raw_sy); (s, s) }
        SvgMeetOrSlice::Slice => { let s = raw_sx.max(raw_sy); (s, s) }
    };

    // Align the scaled viewBox within the free space (may be negative for `slice`).
    let free_x = box_w - vb_w * sx;
    let free_y = box_h - vb_h * sy;
    let ox = match par.align_x {
        SvgAlignX::Min => 0.0,
        SvgAlignX::Mid => free_x * 0.5,
        SvgAlignX::Max => free_x,
    };
    let oy = match par.align_y {
        SvgAlignY::Min => 0.0,
        SvgAlignY::Mid => free_y * 0.5,
        SvgAlignY::Max => free_y,
    };

    (sx, sy, ox - view_box.min_x * sx, oy - view_box.min_y * sy)
}

/// Best-effort CSS-px size of an `<svg>` root's own viewport, computed at box-tree-build
/// time (before layout runs, so percentage width/height cannot resolve against a containing
/// block yet — only the `None` percent-basis case). Mirrors the intrinsic-size fallback chain
/// `lay_out_svg_root` uses later for the box's own rect (CSS width/height → viewBox dims → SVG
/// default 300×150). BUG-334: this is the "current viewport" a descendant `<use>`/`<symbol>`
/// without explicit width/height should size itself against (SVG 2 §5.7/§7.10 — the used value
/// is 100% of the current viewport), not the target's own viewBox dimensions.
fn svg_root_own_size(style: &ComputedStyle, view_box: Option<&ViewBox>, viewport: Size) -> Size {
    let em = style.font_size;
    let width = style.width.as_ref()
        .and_then(|l| l.resolve(em, None, viewport))
        .or_else(|| view_box.map(|vb| vb.width))
        .unwrap_or(300.0)
        .max(0.0);
    let height = style.height.as_ref()
        .and_then(|l| l.resolve(em, None, viewport))
        .or_else(|| view_box.map(|vb| vb.height))
        .unwrap_or(150.0)
        .max(0.0);
    Size { width, height }
}

/// Builds `SvgShape` and `Block` (for `<g>`) layout boxes for the SVG subtree rooted at
/// `parent_id`. Because the HTML5 parser does not implement SVG foreign-content mode, self-
/// closing SVG tags like `<rect/>` are treated as open tags and subsequent siblings become
/// DOM children. This function performs a depth-first recursive scan, collecting SVG shape
/// elements wherever they appear in the subtree.
#[allow(clippy::too_many_arguments)]
fn build_svg_children(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    dark_mode: bool,
) -> Vec<LayoutBox> {
    let mut out = Vec::new();
    collect_svg_shapes(doc, sheet, parent_id, inherited, viewport, own_svg_size, flat, &mut out, dark_mode);
    out
}

/// Recursively collects SVG shape and group boxes from the DOM subtree of `parent_id`.
/// `use_stack` tracks NodeIds currently being expanded via `<use>` for cycle detection.
/// Handles the HTML5 parser's incorrect nesting of self-closing SVG tags: when a `<rect/>`
/// is parsed as an open element, its DOM children (intended siblings) are also scanned.
#[allow(clippy::too_many_arguments)]
fn collect_svg_shapes(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    out: &mut Vec<LayoutBox>,
    dark_mode: bool,
) {
    collect_svg_shapes_impl(doc, sheet, parent_id, inherited, viewport, own_svg_size, flat, out, dark_mode, &[]);
}

/// Inner recursive worker for `collect_svg_shapes`. Carries `use_stack` for cycle detection.
#[allow(clippy::too_many_arguments)]
fn collect_svg_shapes_impl(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    out: &mut Vec<LayoutBox>,
    dark_mode: bool,
    use_stack: &[NodeId],
) {
    for child_id in flat.children_of(doc, parent_id) {
        process_svg_node(doc, sheet, *child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
    }
}

/// Processes a single SVG element node, appending layout boxes to `out`.
/// Used by both the main `collect_svg_shapes_impl` loop and `<use>` clone expansion.
#[allow(clippy::too_many_arguments)]
fn process_svg_node(
    doc: &Document,
    sheet: &Stylesheet,
    child_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    out: &mut Vec<LayoutBox>,
    dark_mode: bool,
    use_stack: &[NodeId],
) {
    let Some(name) = doc.get(child_id).element_name() else {
        return; // text node / comment / etc.
    };
    let style = Arc::new(crate::style::compute_style(doc, child_id, sheet, inherited, viewport, dark_mode));
    if style.display == crate::style::Display::None {
        return;
    }
    let svg_transform = parse_svg_transform(doc.get(child_id).get_attr("transform"));

    match name.local.as_str() {
        "rect" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Rect {
                        x: svg_attr_f32(doc, child_id, "x"),
                        y: svg_attr_f32(doc, child_id, "y"),
                        width: svg_attr_f32(doc, child_id, "width"),
                        height: svg_attr_f32(doc, child_id, "height"),
                        rx: svg_attr_f32(doc, child_id, "rx"),
                        ry: svg_attr_f32(doc, child_id, "ry"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            // Recurse: incorrectly-nested siblings (HTML5 parser wraps them inside rect).
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "circle" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Circle {
                        cx: svg_attr_f32(doc, child_id, "cx"),
                        cy: svg_attr_f32(doc, child_id, "cy"),
                        r: svg_attr_f32(doc, child_id, "r"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "ellipse" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Ellipse {
                        cx: svg_attr_f32(doc, child_id, "cx"),
                        cy: svg_attr_f32(doc, child_id, "cy"),
                        rx: svg_attr_f32(doc, child_id, "rx"),
                        ry: svg_attr_f32(doc, child_id, "ry"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "line" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Line {
                        x1: svg_attr_f32(doc, child_id, "x1"),
                        y1: svg_attr_f32(doc, child_id, "y1"),
                        x2: svg_attr_f32(doc, child_id, "x2"),
                        y2: svg_attr_f32(doc, child_id, "y2"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "path" => {
            let d = doc.get(child_id).get_attr("d").unwrap_or("").to_string();
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape { shape: SvgShapeKind::Path { d }, svg_transform: svg_transform.clone(), svg_paint_matrix: SvgTransform::identity() },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "text" | "tspan" | "textPath" => {
            // SVG text element: collect text content from this element and descendants.
            let text = collect_text_content(doc, child_id);
            // SVG 2 §11.6 / §11.10.2 — `text-anchor` / `dominant-baseline` come from
            // the cascade (`apply_svg_presentational_hints` folds the presentation
            // attributes in as lowest-priority declarations, so author CSS overrides
            // them and they inherit from container elements). `None` = the `start` /
            // `auto` initial value.
            let text_anchor = style.text_anchor.unwrap_or_default();
            let dominant_baseline = style.dominant_baseline.unwrap_or_default();
            // SVG 1.1 §10.9.2 — `baseline-shift` is non-inherited; the presentation
            // attribute is folded into the cascade by `apply_svg_presentational_hints`.
            let baseline_shift = style.baseline_shift;
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgText {
                    text,
                    x: svg_attr_f32(doc, child_id, "x"),
                    y: svg_attr_f32(doc, child_id, "y"),
                    dx: svg_attr_f32(doc, child_id, "dx"),
                    dy: svg_attr_f32(doc, child_id, "dy"),
                    text_anchor,
                    dominant_baseline,
                    baseline_shift,
                    svg_transform: svg_transform.clone(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            // Recurse for potential nested text/tspan/textPath elements.
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "g" => {
            // Group: collect children shapes, then wrap in a Block box.
            let mut group_children: Vec<LayoutBox> = Vec::new();
            collect_svg_shapes_impl(doc, sheet, child_id, &style, viewport, own_svg_size, flat, &mut group_children, dark_mode, use_stack);
            let group_transform = parse_svg_transform(doc.get(child_id).get_attr("transform"));
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::Block,
                children: group_children, col_span: 1, row_span: 1, svg_group_transform: Some(group_transform), scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
        }
        "use" => {
            // SVG <use>: clone the referenced element at an optional (x, y) offset.
            // SVG 2 §5.6 — shadow tree clone with cycle detection via `use_stack`.
            let href_val = doc.get(child_id).get_attr("href")
                .or_else(|| doc.get(child_id).get_attr("xlink:href"))
                .unwrap_or_default();
            let target_ref = href_val.trim_start_matches('#');
            if target_ref.is_empty() {
                return;
            }
            let Some(target_id) = doc.find_by_id(target_ref) else { return; };

            // Cycle guard: skip if target is already on the use-expansion stack.
            if use_stack.contains(&target_id) {
                return;
            }

            // Build the combined transform: <use transform="..."> then translate(x, y).
            let use_x = svg_attr_f32(doc, child_id, "x");
            let use_y = svg_attr_f32(doc, child_id, "y");
            let mut combined = svg_transform.clone();
            if use_x != 0.0 || use_y != 0.0 {
                combined.compose(&SvgTransform::translate(use_x, use_y));
            }

            // Build new stack with target pushed for nested <use> detection.
            let mut new_stack: Vec<NodeId> = use_stack.to_vec();
            new_stack.push(target_id);

            // Collect the referenced subtree into a clone group.
            let mut use_children: Vec<LayoutBox> = Vec::new();
            let target_tag = doc.get(target_id).element_name()
                .map(|n| n.local.as_str().to_owned())
                .unwrap_or_default();

            // BUG-246: a `<use>` referencing a `<symbol>` (or `<svg>`) with a
            // `viewBox` establishes a new viewport (SVG 2 §5.7). The instance is
            // sized by the `<use>`'s `width`/`height` (overriding the symbol's),
            // and the symbol's `viewBox` is mapped into that viewport via
            // `preserveAspectRatio`. Without this, every instance renders at the
            // viewBox's intrinsic size regardless of width/height. Compose the
            // viewBox→viewport scale onto `combined` *after* the use's x/y
            // translate, so it operates in the symbol's local coordinate system.
            if matches!(target_tag.as_str(), "symbol" | "svg")
                && let Some(vb) = parse_view_box(doc, target_id)
            {
                // Viewport size: `<use>` width/height win; else the symbol's own
                // width/height; else BUG-334: fall back to the enclosing `<svg>`'s own
                // CSS-resolved viewport (SVG 2 §5.7/§7.10 "100% of current viewport"),
                // not the target's viewBox dims (that was the BUG-246-era identity bug).
                let attr_dim = |id: NodeId, attr: &str| -> Option<f32> {
                    doc.get(id).get_attr(attr)
                        .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok())
                        .filter(|d| *d > 0.0)
                };
                let vp_w = attr_dim(child_id, "width")
                    .or_else(|| attr_dim(target_id, "width"))
                    .unwrap_or(own_svg_size.width);
                let vp_h = attr_dim(child_id, "height")
                    .or_else(|| attr_dim(target_id, "height"))
                    .unwrap_or(own_svg_size.height);
                let par = parse_preserve_aspect_ratio(doc, target_id);
                let (sx, sy, tx, ty) =
                    compute_preserve_aspect_ratio_transform(&vb, vp_w, vp_h, &par);
                combined.compose(&SvgTransform { matrix: [sx, 0.0, 0.0, sy, tx, ty] });
            }

            if matches!(target_tag.as_str(), "g" | "symbol") {
                // Container: recursively collect its children as the clone content.
                collect_svg_shapes_impl(doc, sheet, target_id, &style, viewport, own_svg_size, flat, &mut use_children, dark_mode, &new_stack);
            } else {
                // Single shape or other element: process the node directly.
                process_svg_node(doc, sheet, target_id, &style, viewport, own_svg_size, flat, &mut use_children, dark_mode, &new_stack);
            }

            if !use_children.is_empty() {
                out.push(LayoutBox {
                    node: child_id, rect: Rect::ZERO, style,
                    kind: BoxKind::Block,
                    children: use_children, col_span: 1, row_span: 1,
                    svg_group_transform: Some(combined),
                    scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                    origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
                });
            }

            // The HTML5 parser does not honour `<use/>` self-closing (it is not a
            // void element), so sibling SVG elements written after a `<use>` are
            // mis-nested as its DOM children. Scan them into `out` as siblings —
            // mirror the rect/circle workaround. A `<use>`'s rendered content comes
            // from its target, never from its DOM children, so this is unambiguous.
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "polygon" | "polyline" => {
            // SVG 1.1 §9.6/§9.7: render via the `<path>` pipeline. A polygon
            // auto-closes its contour (`Z`); a polyline stays open.
            let close = name.local.eq_ignore_ascii_case("polygon");
            let points = parse_svg_points(doc.get(child_id).get_attr("points").unwrap_or(""));
            if let Some(d) = points_to_path_d(&points, close) {
                out.push(LayoutBox {
                    node: child_id, rect: Rect::ZERO, style,
                    kind: BoxKind::SvgShape { shape: SvgShapeKind::Path { d }, svg_transform: svg_transform.clone(), svg_paint_matrix: SvgTransform::identity() },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
                });
            }
            // Mis-nested siblings (HTML5 parser wraps them inside the self-closed shape).
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "defs" | "symbol" => {
            // `<defs>` (SVG 2 §5.5) and `<symbol>` (§5.7) are never rendered
            // directly when encountered as a direct child — their content is
            // painted only when instantiated through `<use>`. (The `<use>` arm
            // collects a symbol's children explicitly, so referencing still works.)
        }
        _ => {
            // Unknown SVG element: skip self, but scan children for shapes.
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
    }
}

// ─── SVG layout ──────────────────────────────────────────────────────────────

/// Lays out an `SvgRoot` box: computes its CSS rect, then positions SVG shape children
/// in document coordinates by applying the viewBox-to-CSS-pixel transform.
fn lay_out_svg_root(b: &mut LayoutBox, start_x: f32, start_y: f32, avail_w: f32, avail_h: Option<f32>, viewport: Size) {
    let s = &b.style;
    let em = s.font_size;
    let cb = avail_w;
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_top  = s.margin_top.resolve_or_zero(em, cb, viewport);
    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;

    let (view_box, preserve_aspect_ratio) = if let BoxKind::SvgRoot { view_box, preserve_aspect_ratio, .. } = &b.kind {
        (view_box.clone(), preserve_aspect_ratio.clone())
    } else {
        // SVG default per §7.8: xMidYMid meet (centered, uniform fit-inside).
        (None, PreserveAspectRatio {
            align_x: SvgAlignX::Mid,
            align_y: SvgAlignY::Mid,
            meet_or_slice: SvgMeetOrSlice::Meet,
        })
    };

    // SVG intrinsic size: CSS width/height wins, then viewBox dimensions, then SVG defaults.
    let svg_w = s.width.as_ref()
        .and_then(|l| l.resolve(em, Some(cb), viewport))
        .or_else(|| view_box.as_ref().map(|vb| vb.width))
        .unwrap_or(300.0)
        .max(0.0);
    let svg_h = s.height.as_ref()
        .and_then(|l| l.resolve(em, avail_h, viewport))
        .or_else(|| view_box.as_ref().map(|vb| vb.height))
        .unwrap_or(150.0)
        .max(0.0);
    b.rect.width  = svg_w;
    b.rect.height = svg_h;

    // viewBox → CSS-px transform via the SVG `preserveAspectRatio` attribute
    // (SVG 1.1 §7.8). An inline `<svg>` is NOT a CSS replaced element, so CSS
    // `object-fit`/`object-position` do NOT apply to it — Chrome/Edge fit the
    // viewBox purely by `preserveAspectRatio` (verified pixel-for-pixel against
    // the Edge TEST-70 reference: every box renders as `meet`/contain, the named
    // `object-fit` classes have no effect). The earlier BUG-110 wiring routed the
    // viewBox through object-fit, stretching/cropping the viewBox in ways Edge
    // never does (BUG-198). object-fit still applies to `<img>`-embedded SVG via
    // the DrawImage path.
    let (scale_x, scale_y, origin_x, origin_y) = match &view_box {
        Some(vb) if vb.width > 0.0 && vb.height > 0.0 => {
            let (sx, sy, ox_delta, oy_delta) =
                compute_preserve_aspect_ratio_transform(vb, svg_w, svg_h, &preserve_aspect_ratio);
            (sx, sy, b.rect.x + ox_delta, b.rect.y + oy_delta)
        }
        _ => (1.0, 1.0, b.rect.x, b.rect.y),
    };
    let root_transform = SvgTransform::identity();
    lay_out_svg_children_positions(&mut b.children, origin_x, origin_y, scale_x, scale_y, &root_transform);
}

/// Recursively positions SVG shape boxes (and `<g>` group children) using the
/// viewBox-to-document-coordinate transform `(origin_x, origin_y, scale_x, scale_y)`.
/// Composes element transforms hierarchically via `parent_transform`.
fn lay_out_svg_children_positions(children: &mut [LayoutBox], ox: f32, oy: f32, sx: f32, sy: f32, parent_transform: &SvgTransform) {
    for child in children.iter_mut() {
        lay_out_svg_element_position(child, ox, oy, sx, sy, parent_transform);
    }
}

#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn lay_out_svg_element_position(b: &mut LayoutBox, ox: f32, oy: f32, sx: f32, sy: f32, parent_transform: &SvgTransform) {
    // Phase 2: full nested transform composition.
    // Get element's own transform (stored during box creation).
    let element_transform = match &b.kind {
        BoxKind::SvgShape { svg_transform, .. } => svg_transform.clone(),
        BoxKind::Block if b.svg_group_transform.is_some() => b.svg_group_transform.as_ref().unwrap().clone(),
        _ => SvgTransform::identity(),
    };

    // Compose parent transform with element transform.
    let mut composed = parent_transform.clone();
    composed.compose(&element_transform);

    if let BoxKind::SvgShape { ref shape, .. } = b.kind {
        // Compute the shape bbox in user coordinates, apply element/group
        // transforms in user space, THEN map user→document via the viewport
        // (ox, oy, sx, sy). Order matters: a `scale`/`rotate` element transform
        // must operate in the SVG local coordinate system, NOT scale the
        // document-space viewport origin. Baking ox/oy in first (the old order)
        // made `scale(0.75)` on a `<use>` in a low SVG drift the clone upward by
        // 0.75·origin_y (BUG-201 row 3: scaled tiles jumped from y≈347 to y≈260).
        let mut bbox = svg_shape_bbox(shape, 0.0, 0.0, 1.0, 1.0); // User coords
        bbox = apply_transform_to_bbox(&bbox, &composed);
        bbox.x = ox + bbox.x * sx;
        bbox.y = oy + bbox.y * sy;
        bbox.width *= sx;
        bbox.height *= sy;
        b.rect = bbox;
        // BUG-174: `<path>` has a ZERO bbox here (its document-space bounds are
        // computed at paint time from the `d` data). `apply_transform_to_bbox`
        // collapses a zero-size bbox to `Rect::ZERO`, discarding the SVG viewport
        // origin (ox, oy). The painter shifts the raw `d` coordinates by
        // `b.rect.x/y`, so without an origin every in-flow SVG path renders at the
        // page-space raw coords instead of inside its own SVG box. Mirror the
        // SvgText branch: anchor the path box at the document-space mapping of the
        // viewport origin. (Absolute-positioned SVGs already get this via the
        // post-layout `shift_tree`; in-flow inline-block SVGs did not.)
        if matches!(shape, SvgShapeKind::Path { .. }) {
            let (px, py) = composed.transform_point(ox, oy);
            b.rect = Rect::new(px, py, 0.0, 0.0);
        }
    } else if let BoxKind::SvgText { x, y, dx, dy, .. } = b.kind {
        // SVG text element: position at specified coordinates with offsets.
        // x, y are in user units; dx, dy are additional offsets.
        // Apply viewBox scaling to user unit coordinates.
        let text_x = ox + (x + dx) * sx;
        let text_y = oy + (y + dy) * sy;
        // Apply only the translation of the composed transform to the text origin point.
        // Cannot use apply_transform_to_bbox: it returns ZERO for zero-size bboxes.
        // Phase 2: measure text width and compute proper bbox based on text-anchor and dominant-baseline.
        let (tx, ty) = composed.transform_point(text_x, text_y);
        b.rect = Rect::new(tx, ty, 0.0, 0.0);
    } else if matches!(b.kind, BoxKind::Block) {
        // <g> group: position its children with composed transform, then compute union bbox.
        lay_out_svg_children_positions(&mut b.children, ox, oy, sx, sy, &composed);
        b.rect = svg_children_union_bbox(&b.children);
    }

    // BUG-244: store the full document-space transform (viewport V ∘ composed) on
    // the shape so paint can apply rotation/skew as a canvas CTM. `b.rect` above
    // remains the axis-aligned bounds (used for clip/hit-test); the matrix carries
    // the off-diagonal (rotate/skew) components an AABB cannot represent. The
    // viewport maps user→document as `doc = (ox + sx·x, oy + sy·y)`, applied AFTER
    // `composed` — mirroring the `bbox.x = ox + bbox.x * sx` mapping above.
    // Stored in the dedicated `svg_paint_matrix` output field — NOT back into
    // `svg_transform` (BUG-262): an inline-block `<svg>` that wraps gets laid out
    // twice, and the first pass's matrix (carrying the viewport translation) would
    // be misread as the element transform on the second pass, drifting the shape
    // out of its clip. Pure translate/scale (b=c=0) leaves paint on its existing
    // axis-aligned `b.rect` fast path.
    if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut b.kind {
        let mut m_doc = SvgTransform { matrix: [sx, 0.0, 0.0, sy, ox, oy] };
        m_doc.compose(&composed);
        *svg_paint_matrix = m_doc;
    }
}

/// Applies an SVG transform matrix to a bounding box by transforming all 4 corners
/// and computing the new bounding box. Phase 2: nested transform composition.
fn apply_transform_to_bbox(bbox: &Rect, transform: &SvgTransform) -> Rect {
    if bbox.width == 0.0 && bbox.height == 0.0 {
        return Rect::ZERO;
    }
    let corners = [
        (bbox.x, bbox.y),
        (bbox.x + bbox.width, bbox.y),
        (bbox.x, bbox.y + bbox.height),
        (bbox.x + bbox.width, bbox.y + bbox.height),
    ];
    let transformed: Vec<(f32, f32)> = corners.iter()
        .map(|(x, y)| transform.transform_point(*x, *y))
        .collect();
    let min_x = transformed.iter().map(|(x, _)| *x).fold(f32::INFINITY, f32::min);
    let min_y = transformed.iter().map(|(_, y)| *y).fold(f32::INFINITY, f32::min);
    let max_x = transformed.iter().map(|(x, _)| *x).fold(f32::NEG_INFINITY, f32::max);
    let max_y = transformed.iter().map(|(_, y)| *y).fold(f32::NEG_INFINITY, f32::max);
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Bounding box of an SVG shape in document coordinates.
/// `ox`/`oy` — document-space origin of the SVG viewport (after viewBox min_x/min_y offset).
/// `sx`/`sy` — CSS-px / SVG-user-unit scale factors.
fn svg_shape_bbox(shape: &SvgShapeKind, ox: f32, oy: f32, sx: f32, sy: f32) -> Rect {
    match *shape {
        SvgShapeKind::Rect { x, y, width, height, .. } =>
            Rect::new(ox + x * sx, oy + y * sy, width * sx, height * sy),
        SvgShapeKind::Circle { cx, cy, r } =>
            Rect::new(ox + (cx - r) * sx, oy + (cy - r) * sy, 2.0 * r * sx, 2.0 * r * sy),
        SvgShapeKind::Ellipse { cx, cy, rx, ry } =>
            Rect::new(ox + (cx - rx) * sx, oy + (cy - ry) * sy, 2.0 * rx * sx, 2.0 * ry * sy),
        SvgShapeKind::Line { x1, y1, x2, y2 } => {
            // Bounding rect of the line segment; minimum 1 CSS px on each axis so the
            // painter can clip-test against it.
            let lx = x1.min(x2);
            let ly = y1.min(y2);
            let rw = (x2 - x1).abs().max(1.0 / sx);
            let rh = (y2 - y1).abs().max(1.0 / sy);
            Rect::new(ox + lx * sx, oy + ly * sy, rw * sx, rh * sy)
        }
        SvgShapeKind::Path { .. } =>
            // Path bounding box requires full path-data parsing — deferred to paint.
            // CSS: fill, stroke — P4 wires; P2 renders via GPU path commands.
            Rect::ZERO,
    }
}

/// Union bounding box of a slice of already-positioned layout boxes.
/// Returns `Rect::ZERO` when all children have zero-area rects.
fn svg_children_union_bbox(children: &[LayoutBox]) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for c in children {
        if c.rect.width > 0.0 || c.rect.height > 0.0 {
            min_x = min_x.min(c.rect.x);
            min_y = min_y.min(c.rect.y);
            max_x = max_x.max(c.rect.x + c.rect.width);
            max_y = max_y.max(c.rect.y + c.rect.height);
        }
    }
    if min_x == f32::INFINITY { Rect::ZERO } else { Rect::new(min_x, min_y, max_x - min_x, max_y - min_y) }
}

/// Запрос на предзагрузку изображения: URL после picking-а по
/// `<picture>`/`srcset`/`sizes` плюс признаки явного задания размеров
/// author-ом (нужны shell для `apply_intrinsic_size`).
pub struct ImageRequest {
    pub node_id: NodeId,
    pub url: String,
    pub has_explicit_width: bool,
    pub has_explicit_height: bool,
    /// `loading="lazy"` (HTML LS §2.6.6.9): defer fetch until element is near viewport.
    /// Shell skips eager fetch and instead registers the image for IntersectionObserver
    /// proximity check; loaded once the element scrolls within one viewport of the fold.
    pub is_lazy: bool,
    /// `fetchpriority` (HTML LS §2.5.7): нормализованное `"high"`/`"low"`;
    /// `auto`, мусор и отсутствие атрибута → `None`.
    pub fetch_priority: Option<String>,
}

/// Обходит DOM и возвращает запросы на загрузку для всех `<img>`-элементов.
/// URL выбирается через тот же picker, что layout использует при построении
/// `BoxKind::Image { src }` — гарантирует совпадение ключей в
/// `Renderer::register_image` и `DisplayCommand::DrawImage.src`.
pub fn collect_image_requests(doc: &Document, viewport: Size) -> Vec<ImageRequest> {
    let mut out = Vec::new();
    collect_requests_inner(doc, doc.root(), viewport, &mut out);
    out
}

/// Обходит готовое layout-дерево и возвращает уникальные URL-ы из
/// `background-image: url(...)` (CSS Backgrounds L3 §3.10) — те же ключи,
/// что эмиттер кладёт в `DisplayCommand::DrawBackgroundImage.src`.
///
/// Background-image не участвует в расчёте размеров, поэтому собирается
/// уже после layout — shell вызывает функцию между layout-ом и paint-ом,
/// дозагружает байты и регистрирует через `Renderer::register_image`.
///
/// Возвращает `Vec<String>` (а не `Vec<ImageRequest>`): для background-image
/// нет node-anchored intrinsic-size hint-ов (CSS Backgrounds L3 §3.9 говорит
/// о `background-size` в стилях, intrinsic-размер картинки в layout не
/// влияет). Дубликаты отфильтрованы — одна и та же картинка на разных
/// элементах загружается один раз.
///
/// `dpr` — device pixel ratio, по которому разрешается `image-set()`
/// (CSS Images L4 §5). Значение **обязано** совпадать с тем, что получит
/// `build_display_list_ordered_dpr`: эмиттер кладёт в `src` уже выбранного
/// кандидата, и ключ загрузки должен быть тем же. `1.0` — дефолт
/// `build_display_list_ordered`.
#[must_use]
pub fn collect_background_image_requests(root: &LayoutBox, dpr: f32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_bg_image_inner(root, dpr, &mut out);
    out
}

/// Кладёт в `out` URL-ы, под которыми эмиттер будет искать картинки слоя.
///
/// `image-set()` хранится в слое дословно, а в display list попадает уже
/// выбранный кандидат — поэтому здесь функция разворачивается, иначе shell
/// уходил бы качать текст `image-set(…)` как имя файла. `cross-fade()` рисуется
/// одной командой из **двух** источников, и обе стороны (каждая сама может быть
/// `image-set()`) должны быть загружены. Пустые и уже собранные URL-ы
/// пропускаются.
fn push_bg_image_urls(image: &BackgroundImage, dpr: f32, out: &mut Vec<String>) {
    match image {
        BackgroundImage::Url(src) => {
            let resolved = if crate::image_set::is_image_set(src) {
                crate::image_set::select_image_set_url(src, dpr)
            } else {
                src.clone()
            };
            if !resolved.is_empty() && !out.contains(&resolved) {
                out.push(resolved);
            }
        }
        // CSS Images L4 §4 — обе стороны попадают в `DrawCrossFade`.
        BackgroundImage::CrossFade { a, b, .. } => {
            push_bg_image_urls(a, dpr, out);
            push_bg_image_urls(b, dpr, out);
        }
        _ => {}
    }
}

fn collect_bg_image_inner(b: &LayoutBox, dpr: f32, out: &mut Vec<String>) {
    for layer in &b.style.background_layers {
        push_bg_image_urls(&layer.image, dpr, out);
    }
    // CSS Lists L3 §2.3: a `list-style-image` marker also needs its URL fetched
    // and registered, same as a background image.
    if let BoxKind::Marker { image: Some(src), .. } = &b.kind
        && !src.is_empty()
        && !out.iter().any(|u| u == src)
    {
        out.push(src.clone());
    }
    // CSS Generated Content L3 §2.1: `content: url(...)` produces an inline-replaced
    // image segment that the shell would otherwise never fetch — unlike `<img>`, it
    // has no DOM element for `collect_image_requests` to walk. Such segments are
    // tagged with `source_node == NodeId::from_index(0)` ("no DOM origin"), which
    // distinguishes them from real inline `<img>` frags (already fetched, and
    // possibly `loading="lazy"`). Piggy-back on the post-layout background pass.
    if let BoxKind::InlineRun { segments, .. } = &b.kind {
        for seg in segments {
            if let Some(src) = &seg.img_src
                && seg.source_node == NodeId::from_index(0)
                && !src.is_empty()
                && !out.iter().any(|u| u == src)
            {
                out.push(src.clone());
            }
        }
    }
    for child in &b.children {
        collect_bg_image_inner(child, dpr, out);
    }
}

/// Доставляет intrinsic-размеры декодированной картинки в layout, дописывая
/// `<img>` пустые презентационные атрибуты `width`/`height`.
///
/// Возвращает `true`, если атрибут действительно был дописан — то есть DOM
/// изменился и странице нужен релейаут. `false` (ничего не изменилось) — когда
/// автор задал оба размера сам или размеры уже дописаны прошлым вызовом; на нём
/// держится сходимость повторного прохода `Lumen::apply_stream_intrinsic_sizes`
/// (BUG-735), иначе «применили → релейаут → применили» зациклилось бы.
///
/// Живёт рядом с [`collect_image_requests`] (BUG-430): и шелл, и headless-драйвер
/// сначала берут URL у picker-а, потом сообщают размеры декодированной картинки
/// обратно в DOM — правило заполнения слотов обязано быть у обоих одно.
pub fn apply_intrinsic_size(doc: &mut Document, node_id: NodeId, width: u32, height: u32) -> bool {
    use lumen_dom::{Attribute, QualName};
    let NodeData::Element { attrs, .. } = &mut doc.get_mut(node_id).data else {
        return false;
    };
    // Presence of the author's width/height content attributes (any value —
    // including percentages — counts as "set" and must never be duplicated).
    let has_w = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width"));
    let has_h = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height"));
    // Author values parsed as non-negative integer px (HTML dimension-attr
    // grammar). `None` when absent OR non-integer (e.g. `width="50%"`).
    let attr_w = attrs
        .iter()
        .find(|a| a.name.local.eq_ignore_ascii_case("width"))
        .and_then(|a| a.value.trim().parse::<u32>().ok());
    let attr_h = attrs
        .iter()
        .find(|a| a.name.local.eq_ignore_ascii_case("height"))
        .and_then(|a| a.value.trim().parse::<u32>().ok());

    // BUG-269: fill the missing dimension from the intrinsic aspect ratio
    // (CSS 2.1 §10.6.2) rather than from the raw intrinsic value, so a
    // fixed-width `<img width="240">` (intrinsic 120×80) becomes 240×160, not
    // 240×80 — and, crucially, is not left with a collapsed `height: auto` = 0.
    // Push only into empty attribute slots (presentational hint, specificity 0
    // — authored CSS still wins). A pixel-parsed author dimension drives the
    // ratio; a non-integer one (percentage) falls back to the raw intrinsic
    // value for the other axis.
    let (new_w, new_h) = match (attr_w, attr_h) {
        (Some(w), None) => {
            let h = if width > 0 {
                ((w as u64 * height as u64 + width as u64 / 2) / width as u64) as u32
            } else {
                height
            };
            (None, Some(h))
        }
        (None, Some(h)) => {
            let w = if height > 0 {
                ((h as u64 * width as u64 + height as u64 / 2) / height as u64) as u32
            } else {
                width
            };
            (Some(w), None)
        }
        // Both integers set, or one/both present but non-integer: fill any
        // still-empty slot with the raw intrinsic value.
        _ => (
            (!has_w).then_some(width),
            (!has_h).then_some(height),
        ),
    };

    let mut changed = false;
    if !has_w && let Some(w) = new_w {
        attrs.push(Attribute {
            name: QualName::html("width"),
            value: w.to_string(),
        });
        changed = true;
    }
    if !has_h && let Some(h) = new_h {
        attrs.push(Attribute {
            name: QualName::html("height"),
            value: h.to_string(),
        });
        changed = true;
    }
    changed
}

fn collect_requests_inner(doc: &Document, id: NodeId, viewport: Size, out: &mut Vec<ImageRequest>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "img"
    {
        let has_explicit_width = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width"));
        let has_explicit_height =
            attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height"));
        let is_lazy = attrs.iter().any(|a| {
            a.name.local.eq_ignore_ascii_case("loading")
                && a.value.as_str().eq_ignore_ascii_case("lazy")
        });
        // HTML LS §2.5.7: нормализация fetchpriority — только "high"/"low".
        let fetch_priority = attrs
            .iter()
            .find(|a| a.name.local.eq_ignore_ascii_case("fetchpriority"))
            .map(|a| a.value.trim().to_ascii_lowercase())
            .filter(|v| v == "high" || v == "low");
        let source = resolve_image_source(doc, id, viewport);
        if !source.url.is_empty() {
            out.push(ImageRequest {
                node_id: id,
                url: source.url,
                has_explicit_width,
                has_explicit_height,
                is_lazy,
                fetch_priority,
            });
        }
        return; // void element — нет children
    }
    // BUG-848: three more elements carry an image URL the display list will
    // key on but never produced a request at all — `<video poster>`, an
    // `<input type=image>` control and an SVG `<image>` (href/xlink:href).
    // None of the three support srcset/loading/fetchpriority, so the request
    // just carries the "not set" defaults for those fields.
    if let Some(url) = image_subresource_url(node) {
        out.push(ImageRequest {
            node_id: id,
            url,
            has_explicit_width: false,
            has_explicit_height: false,
            is_lazy: false,
            fetch_priority: None,
        });
    }
    for &child in &node.children {
        collect_requests_inner(doc, child, viewport, out);
    }
}

/// URL for the three BUG-848 element kinds that carry an image but are not
/// `<img>`: `<video poster>`, `<input type=image src>`, SVG `<image
/// href|xlink:href>`. `None` for every other element, or when the relevant
/// attribute is absent/empty — same "nothing to fetch" rule `<img>` uses.
fn image_subresource_url(node: &lumen_dom::Node) -> Option<String> {
    let name = node.element_name()?;
    let url = match name.local.as_str() {
        "video" => node.get_attr("poster"),
        "input" if node.input_type() == Some(lumen_dom::InputType::Image) => node.get_attr("src"),
        // SVG `<image>`; legacy `xlink:href` (SVG 1.1) alongside the plain
        // `href` this parser keeps as one attribute, same fallback `<use>`
        // resolution already uses a few lines up.
        "image" => node.get_attr("href").or_else(|| node.get_attr("xlink:href")),
        _ => None,
    }?;
    (!url.is_empty()).then(|| url.to_string())
}

/// Выбрать источник для `<img>`-элемента с учётом окружающего контекста:
///  1. Если parent — `<picture>`, прогоняем picture-picker
///     (выбирает `<source>` или fallback на `<img>` по `media`/`type`/
///     `srcset`/`sizes`).
///  2. Иначе — `<img>`-picker, учитывающий собственный `srcset`/`sizes`/`src`.
///  3. Если оба picker-а вернули `None` (нет ни `srcset`, ни `src`) —
///     fallback на голый `src` атрибут как раньше: для битой разметки
///     лучше отрисовать пустую коробку, чем ничего.
///
/// Phase 0: DPR=1.0 (layout не знает про device pixel ratio renderer-а —
/// это интегрирует P3 при relayout-on-resize), `prefers_dark` = false.
/// `supported_types` заполняется из `lumen_image::supported_mime_types()`:
/// picker пропускает `<source type="image/webp">` и аналогичные пока
/// неподдерживаемые форматы вместо того чтобы выбирать их и показывать пустую коробку.
fn resolve_image_source(doc: &Document, img_id: NodeId, viewport: Size) -> ImageSource {
    let sizes_vp = SizesViewport {
        width_px: viewport.width,
        height_px: viewport.height,
        root_font_size_px: 16.0,
        prefers_dark: false,
    };
    let params = PictureParams {
        viewport: sizes_vp,
        dpr: 1.0,
        supported_types: Some(lumen_image::supported_mime_types()),
    };

    if let Some(parent_id) = doc.get(img_id).parent
        && is_picture_element(doc, parent_id)
        && let Some(picked) = pick_picture_source(doc, parent_id, &params)
    {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    if let Some(picked) = pick_img_source(doc, img_id, sizes_vp, params.dpr) {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    let raw_src = doc.get(img_id).get_attr("src").unwrap_or("").to_string();
    ImageSource { url: raw_src, intrinsic_width: None, intrinsic_height: None }
}

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node: NodeId,
    /// Border-box rectangle: (x, y) is the top-left corner after margin,
    /// (width, height) includes padding + border but NOT margin.
    pub rect: Rect,
    /// Computed style of this box, shared with the cascade cache
    /// (`CounterMap::styles`) and with every previous-frame tree that still
    /// holds it, behind copy-on-write.
    ///
    /// BUG-341 S12: this used to be an owned `ComputedStyle` — a 3.2 KB,
    /// 302-field struct with ~30 heap-allocated fields. Every box therefore
    /// paid a deep copy in `build_box` (from the cascade cache), a second one
    /// in `lay_out_inner` (which snapshots the style to dodge the borrow
    /// checker), and a third in every whole-tree `clone()` the incremental
    /// pipeline does per frame to persist `prev`. Measured on `CC12_HOVER`:
    /// 1.2 ms of `lay_out`'s 3.7 ms, plus 1.5 ms of per-cycle bookkeeping.
    /// Behind an `Arc` all three become refcount bumps, and the handful of
    /// passes that genuinely rewrite a used value (`font-size-adjust`, flex
    /// item stretch, container queries) take the copy via
    /// [`std::sync::Arc::make_mut`] on exactly the boxes they touch.
    ///
    /// Reads are unchanged: `Arc` derefs to `ComputedStyle`, so `b.style.field`
    /// and `&b.style` (coerced to `&ComputedStyle`) both still work.
    pub style: Arc<ComputedStyle>,
    pub kind: BoxKind,
    pub children: Vec<LayoutBox>,
    /// HTML `colspan` attribute (table cells only). Number of columns this cell spans.
    /// Always ≥ 1; defaults to 1 for non-table-cell boxes.
    pub col_span: u32,
    /// HTML `rowspan` attribute (table cells only). Number of rows this cell spans.
    /// Always ≥ 1; defaults to 1 for non-table-cell boxes.
    pub row_span: u32,
    /// SVG `transform` attribute for `<g>` groups (Phase 2: nested transforms).
    /// Only used for Block boxes that represent SVG groups; None for all other boxes.
    pub svg_group_transform: Option<SvgTransform>,
    /// Horizontal scroll offset in CSS px for `overflow: scroll` / `overflow: auto`
    /// containers. Updated by shell on wheel/touch events via `set_scroll_position()`.
    /// Zero for non-scrollable boxes.
    pub scroll_x: f32,
    /// Vertical scroll offset in CSS px. Same semantics as `scroll_x`.
    pub scroll_y: f32,
    /// Incremental-layout dirty flags (EE-3). Only consulted during
    /// `lay_out_incremental` passes — normal `lay_out` ignores this field.
    /// Set via `mark_dirty`; cleared via `clear_dirty` / `lay_out_incremental`.
    pub dirty: crate::incremental::DirtyBits,
    /// Provenance for introspection (ADR-025 §1): where this box came from,
    /// distinct from `node` above. `node` stays the hot-path "whose style
    /// applies here" answer and is never `None`; `origin` is what
    /// `explain_element`/`ProvenanceIndex` read and correctly says "no DOM
    /// origin" instead of aliasing the document root.
    pub origin: BoxOrigin,
}

/// Where a layout box came from — the identity of a box for all
/// introspection purposes (ADR-025 §1). Replaces the `NodeId::from_index(0)`
/// "no DOM origin" sentinel, which collided with the document root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxOrigin {
    /// The DOM node this box belongs to, or `None` for boxes with no DOM
    /// origin (anonymous boxes, generated content). Never a sentinel value —
    /// use `None`, not `NodeId::from_index(0)`.
    pub node: Option<NodeId>,
    /// Why this box exists — disambiguates the many boxes one node can
    /// produce (an element's principal box vs. an anonymous wrapper around
    /// its inline children, for example).
    pub role: BoxRole,
}

impl Default for BoxOrigin {
    /// `node: None` + `BoxRole::Element` — used only as a placeholder for
    /// construction sites that predate provenance tracking (test fixtures,
    /// benchmark scaffolding). Production box constructors always set both
    /// fields explicitly instead of relying on this default.
    fn default() -> Self {
        BoxOrigin { node: None, role: BoxRole::Element }
    }
}

/// Disambiguates the many boxes one DOM node — or no node at all — can
/// produce (ADR-025 §1). Paired with `BoxOrigin::node` as the identity of a
/// box; `role` alone or `node` alone is never enough (an anonymous wrapper
/// must never be reported as its parent element).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxRole {
    /// The principal box of an element.
    Element,
    /// Anonymous block-level wrapper (CSS 2.1 §9.2.1.1) or other
    /// block-level box synthesised with no element of its own — table
    /// fixup boxes, `appearance: base-select` scaffolding, drop-cap float
    /// wrappers. `node` is the *containing* element; this role is what makes
    /// the wrapper distinguishable from it.
    AnonymousBlock,
    /// Anonymous inline-level wrapper — `anon_inline_run`,
    /// `anon_inline_block_row`, collapsed inline-block whitespace gaps.
    /// `node` is the containing element or the source text node.
    AnonymousInlineRun,
    /// Pseudo-element box or segment (`::before`, `::after`,
    /// `::first-letter`, `::first-line`, `::marker`'s content run).
    Pseudo(PseudoKind),
    /// List marker box (`::marker`'s own box, CSS Lists L3 §3).
    ListMarker,
    /// `content:` generated content with no DOM text/image node of its own.
    GeneratedContent,
    /// Scaffolding box with no rendered-page meaning at all — pre-navigation
    /// placeholders and benchmark harnesses that need a `LayoutBox` value
    /// before any real layout has run.
    Placeholder,
}

/// Отрезок inline-контента с собственным стилем (до layout).
#[derive(Debug, Clone)]
pub struct InlineSegment {
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved px space before this segment's first word:
    /// margin_left + border_left_width + padding_left of the inline element.
    pub pre_space: f32,
    /// Resolved px space after this segment's last word:
    /// padding_right + border_right_width + margin_right of the inline element.
    pub post_space: f32,
    /// True when this segment comes from inside an inline element box
    /// (not anonymous text directly in a block container). Used by the painter
    /// to know whether to draw the element's own background/border.
    pub is_element_box: bool,
    /// Non-None when this segment is an inline-replaced `<img>`. Contains the
    /// resolved image URL. `text` holds the alt attribute.
    pub img_src: Option<String>,
    /// `loading="lazy"` on the inline `<img>` — emit `LazyImageSlot` instead of `DrawImage`.
    pub img_is_lazy: bool,
    /// Pre-computed pixel width for image segments (0.0 for text segments).
    pub img_width: f32,
    /// True when this segment represents a forced line break (CSS §4.1: newline
    /// in white-space: pre / pre-wrap text). `text` is empty in this case.
    pub forced_break: bool,
    /// CSS structural pseudo-element role of this segment.
    /// Split out by `collect_inline_segments` before wrapping.
    /// `apply_first_letter_pseudo` looks up the `::first-letter` rule and overrides
    /// the style of segments where `pseudo_kind == PseudoKind::FirstLetter`.
    pub pseudo_kind: PseudoKind,
    /// DOM text node that produced this segment, for Selection/Range mapping.
    /// `NodeId(0)` (document root) for generated content with no DOM origin.
    pub source_node: NodeId,
    /// UTF-8 byte offset of `text[0]` within the source text node's content.
    /// Always 0 for non-pre text (whole text node → one segment after whitespace
    /// collapsing); non-zero for pre/pre-wrap segments split at `\n`.
    pub source_char_offset: u32,
    /// UAX #9 embedding level of this segment's text (even = left-to-right,
    /// odd = right-to-left). Assigned by [`crate::bidi::resolve`], which splits
    /// a segment wherever the level changes, so the value is uniform across
    /// `text`. `0` until the bidi pass runs (and for paragraphs it skips).
    pub bidi_level: u8,
}

/// Marks an inline segment as the target of a CSS structural pseudo-element.
/// `apply_first_letter_pseudo` applies `::first-letter` styles from this marker
/// without touching layout geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PseudoKind {
    /// Regular content — no pseudo-element style override.
    #[default]
    None,
    /// CSS Pseudo-elements L4 §5.1 — typographic first letter of the block.
    /// Split from the first non-whitespace text node by `collect_inline_segments`.
    /// Applied by `apply_first_letter_pseudo` via
    /// `compute_pseudo_element_style(node, "first-letter")`, which overrides `seg.style`.
    FirstLetter,
    /// `::before` generated content (ADR-025 `BoxRole::Pseudo` tag only — not
    /// produced by `collect_inline_segments`, which has no notion of `::before`
    /// at the segment level).
    Before,
    /// `::after` generated content (`BoxRole::Pseudo` tag only, see `Before`).
    After,
    /// `::first-line` styled box (`BoxRole::Pseudo` tag only — applied by
    /// `split_first_line_boxes`, which works on whole boxes, not segments).
    /// Paint keys the pseudo-element's own background off this role.
    FirstLine,
    /// `::marker` list-marker content (`BoxRole::Pseudo` tag only — markers are
    /// `BoxKind::Marker` boxes, tagged `BoxRole::ListMarker` instead; this
    /// variant exists for the rare case a marker's content is itself an
    /// inline run needing a `PseudoKind`, e.g. future nested-marker content).
    Marker,
}

/// Позиционированный текстовый фрагмент в строке (после layout).
/// `x` — смещение от левого края inline-контейнера до начала ТЕКСТА
/// (после border+padding inline-элемента слева).
/// `width` — ширина текста фрагмента в пикселях.
/// `padding_left` / `padding_right` — разрешённые px padding-а inline-элемента
/// для этого фрагмента (ненулевые только для первого/последнего слова сегмента).
#[derive(Debug, Clone)]
pub struct InlineFrag {
    pub x: f32,
    pub width: f32,
    /// Vertical offset within the line box (CSS vertical-align). Positive = down.
    pub y_offset: f32,
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved padding_left of this frag's inline box start (0 if not a box start).
    pub padding_left: f32,
    /// Resolved padding_right of this frag's inline box end (0 if not a box end).
    pub padding_right: f32,
    /// True when this frag comes from an inline element box (not anonymous text).
    /// Used by the painter to draw element background/border.
    pub is_element_box: bool,
    /// Non-None when this frag represents an inline-replaced `<img>`.
    /// `text` holds the alt attribute; `width` is the rendered pixel width.
    pub img_src: Option<String>,
    /// `loading="lazy"` on the inline `<img>` — emit `LazyImageSlot` instead of `DrawImage`.
    pub img_is_lazy: bool,
    /// True when this fragment lies on the first formatted line of its block container.
    /// Set by `lay_out` after `wrap_inline_run` completes.
    /// `split_first_line_boxes` applies `compute_pseudo_element_style(node, "first-line")`
    /// to the box holding the first-line frags, overriding their style.
    pub is_first_line: bool,
    /// DOM text node that produced this fragment (for Selection/Range mapping).
    /// Matches the source `InlineSegment::source_node`. `NodeId(0)` for
    /// generated/anonymous content with no direct DOM text node.
    pub source_node: NodeId,
    /// UTF-8 byte offset of `text[0]` within the source text node's content.
    /// Computed in `wrap_inline_run` as words are taken from the segment.
    pub source_char_offset: u32,
    /// UAX #9 embedding level inherited from the source [`InlineSegment`].
    /// `align_lines` feeds it to [`crate::bidi::reorder_line`] for the L2 pass;
    /// paint feeds it to [`crate::bidi::visual_text`], which is what turns an
    /// odd level into right-to-left glyph order.
    pub bidi_level: u8,
}

#[derive(Debug, Clone)]
pub enum BoxKind {
    /// Block-уровневый бокс (элемент или корень документа).
    Block,
    /// Анонимный контейнер для потока inline-контента (текст + inline-элементы).
    /// `segments` — сырые отрезки до lay_out; `lines` — позиционированные строки
    /// после lay_out. Каждая строка — `Vec<InlineFrag>`.
    /// `first_line_style` — pre-computed `::first-line` pseudo-element style for the owning
    /// element. `None` if no rule matches. Applied by `lay_out()` to frags on `lines[0]`.
    InlineRun {
        segments: Vec<InlineSegment>,
        lines: Vec<Vec<InlineFrag>>,
        /// CSS Pseudo-elements L4 §5.3: computed ::first-line style. Set during build_box(),
        /// applied in lay_out() after wrap_inline_run() to first-line frags.
        first_line_style: Option<Box<crate::style::ComputedStyle>>,
    },
    /// Анонимный контейнер для горизонтального потока `display: inline-block`
    /// элементов. Сами дочерние боксы хранятся в `LayoutBox.children`. При
    /// layout дети раскладываются горизонтально слева направо; высота строки
    /// = высота самого высокого дочернего элемента.
    InlineBlockRow,
    /// Replaced element: изображение (`<img>`). Inline-уровневый atomic-бокс
    /// (UA-дефолт `display: inline`, IFC-2): собирается в `InlineBlockRow` и
    /// делит строку с текстом, а на базовую линию садится нижней кромкой margin
    /// box (CSS 2.1 §10.8.1 — `inline_baseline` возвращает для него `None`).
    /// `src` — путь / URL ресурса (декодирование откладывается на следующий
    /// шаг), `alt` — alternate-текст для отображения и AT, размеры берутся из
    /// `style.width`/`style.height` (которые могут происходить из CSS или
    /// HTML-атрибутов как presentational hints).
    Image {
        src: String,
        alt: String,
        /// `loading="lazy"` (HTML LS §lazy-loading): fetch deferred until proximity check.
        /// Display list emits `LazyImageSlot` instead of `DrawImage` when `true`.
        is_lazy: bool,
    },
    /// Replaced element: HTML `<video>` element (HTML spec §14).
    ///
    /// Phase 0: rendered as a grey `DrawImage` placeholder (the video src is
    /// not fetched or decoded). Intrinsic size comes from `width`/`height`
    /// HTML attributes; UA default is 300×150 CSS px (HTML spec §14.1).
    /// `poster` is the optional poster-image URL shown before playback starts.
    Video {
        /// Primary video source URL (`src` attribute).
        src: String,
        /// Poster image URL (`poster` attribute), may be empty.
        poster: String,
    },
    /// Replaced element: HTML `<canvas>` element — CPU-rasterized drawing surface
    /// (HTML Living Standard §4.12.4).
    ///
    /// Phase 0: the pixel buffer is produced by JS Canvas 2D drawing operations
    /// (`canvas.getContext('2d')`) and rendered via a `DrawImage` command keyed by
    /// `canvas:{node_id}`. Intrinsic size comes from the `width`/`height` content
    /// attributes; UA defaults are 300×150 CSS px (HTML LS §4.12.4).
    Canvas {
        /// Canvas bitmap width in CSS pixels (from `width` attribute, default 300).
        width: u32,
        /// Canvas bitmap height in CSS pixels (from `height` attribute, default 150).
        height: u32,
    },
    /// Replaced element: HTML `<audio>` element (HTML spec §4.8.10).
    ///
    /// Phase 0: no audio playback. Without `controls` attribute: 0×0 (invisible).
    /// With `controls` attribute: full-width × 40px grey bar (UA default per spec).
    /// `src` is the primary audio source URL.
    Audio {
        /// Primary audio source URL (`src` attribute), may be empty.
        src: String,
        /// Whether the `controls` attribute is present (shows a 40px control bar).
        controls: bool,
    },
    /// Replaced element: HTML `<iframe>` element (HTML spec §4.8.5).
    ///
    /// Phase 0: rendered as a grey `DrawImage` placeholder (no sub-document
    /// navigation). Intrinsic size comes from `width`/`height` HTML attributes;
    /// UA defaults are 300×150 CSS px (HTML spec §4.8.5). `src` is the URL
    /// to display in paint-side label and in JS `src` property. When `srcdoc`
    /// is `Some`, the inline HTML was parsed via [`build_iframe_document`] and
    /// is available for future Phase 1 sub-document rendering.
    Iframe {
        /// Primary document URL (`src` attribute), may be empty.
        src: String,
        /// Inline HTML content from `srcdoc` attribute (HTML spec §4.8.5).
        /// `None` if the element has no `srcdoc` attribute.
        srcdoc: Option<String>,
    },
    /// Replaced element: HTML form control (`<input>`, `<button>`, `<select>`,
    /// `<textarea>`). Phase 0: block-level replaced. Размеры берутся из
    /// `style.width`/`style.height` (UA defaults из `apply_ua_form_controls`).
    /// `kind` зарезервирован для paint-специализаций в следующих фазах.
    FormControl {
        kind: FormControlKind,
    },
    /// CSS 2.1 §17 — строка таблицы (`display: table-row`). Дочерние
    /// боксы — ячейки (`display: table-cell`), которые раскладываются
    /// горизонтально слева направо. Высота строки = max высота ячейки.
    TableRow,
    /// Схлопнутый межэлементный пробел в InlineBlockRow.
    /// Не рисуется; участвует только как горизонтальный gap между
    /// inline-block соседями (CSS white-space collapsing §4.1.2).
    InlineSpace,
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
    /// CSS Lists L3 §2.1 — `::marker` pseudo-element for `display: list-item`.
    /// `text` — marker string for counter types (1., a., i., …); empty for bullet
    /// types (disc/circle/square) which are rendered as geometric shapes.
    /// `position` — inside/outside flow. `list_style_type` — used by the display-list
    /// emitter to choose geometric (disc/circle/square) vs text rendering.
    /// For `outside` (default) positioned left of the principal block, out of flow.
    /// `image` — CSS Lists L3 §2.3 `list-style-image`: resolved URL when set. When
    /// present it replaces the `list_style_type`/`text` marker (the painter emits a
    /// `DrawImage` instead of a bullet/counter). Same URL key used by
    /// `collect_background_image_requests`, so the shell fetches and registers it.
    Marker {
        text: String,
        position: ListStylePosition,
        list_style_type: ListStyleType,
        image: Option<String>,
    },
    /// CSS Display L3 §8 — `display: flow-root`. Establishes a Block Formatting
    /// Context: contains floats, prevents margin escape. Laid out identically to
    /// Block in Phase 0; BFC float-containment wired when float layout is added.
    /// CSS: flow-root
    FlowRoot,
    /// CSS Display L3 §7.2 — `display: contents`. The element itself generates no
    /// box. Children are flattened into the parent's formatting context by
    /// `flatten_contents()` during `build_box`. Must never appear in the final
    /// layout tree that reaches `lay_out`.
    Contents,
    /// CSS 2.1 §17 — table container (`display: table` / `display: inline-table`).
    /// Direct children are `TableRowGroup` or `TableRow` boxes. Layout computes
    /// global column widths across all rows before positioning each row.
    Table,
    /// CSS 2.1 §17 — row group (`display: table-row-group`, `table-header-group`,
    /// `table-footer-group`). Rendered as a transparent wrapper; rows inside are
    /// collected by the parent `Table` box during column-width computation.
    TableRowGroup,
    /// SVG root element (`<svg>`). Acts as a replaced element in CSS flow:
    /// `rect` is its border-box in document coordinates (CSS width × height).
    /// `view_box` maps SVG user-unit space to this rect for shape coordinate transforms.
    /// Children are `SvgShape` and `Block` (for `<g>` groups) boxes.
    /// CSS: width, height (from attributes as presentational hints), fill, stroke — P4 wires.
    SvgRoot {
        /// Parsed `viewBox` attribute. `None` when attribute absent: shapes use 1:1 px mapping.
        view_box: Option<ViewBox>,
        /// Parsed `preserveAspectRatio` attribute for aspect-ratio preservation.
        preserve_aspect_ratio: PreserveAspectRatio,
    },
    /// Individual SVG shape (`<rect>`, `<circle>`, `<ellipse>`, `<line>`, `<path>`).
    /// `LayoutBox.rect` is the bounding box in *document coordinates* (post-viewBox scaling).
    /// `shape` carries the original SVG user-unit geometry for accurate paint-side rendering.
    /// CSS: fill, stroke, stroke-width, opacity — P4 wires via ComputedStyle SVG fields.
    SvgShape {
        /// Geometric primitive in SVG user units (before viewBox scaling).
        shape: SvgShapeKind,
        /// Parsed SVG `transform` presentation attribute (Phase 2: nested transforms).
        /// Composed with parent transforms during layout for accurate positioning.
        /// This is layout *input* — the element's own transform — and must never be
        /// mutated by layout (BUG-262: an inline-block `<svg>` that wraps to a new
        /// line is laid out twice; overwriting this field on the first pass poisoned
        /// the second pass, drifting the shape outside its clip).
        svg_transform: SvgTransform,
        /// Document-space paint matrix `viewport ∘ parent ∘ element`, computed by
        /// layout (`lay_out_svg_element_position`) and consumed by paint as the
        /// canvas CTM for rotate/skew (BUG-244). Layout *output* only; defaults to
        /// identity at construction. Kept separate from `svg_transform` so re-layout
        /// (inline-block wrap, incremental relayout) always recomposes from the
        /// pristine element transform rather than a previous pass's result.
        svg_paint_matrix: SvgTransform,
    },
    /// SVG text element (`<text>`, `<tspan>`, `<textPath>`).
    /// `LayoutBox.rect` is the text bounding box in *document coordinates*.
    /// Text content is measured via `TextMeasurer` and positioned according to SVG text attributes.
    /// CSS: fill, stroke, font-family, font-size — P4 wires via ComputedStyle SVG fields.
    /// // CSS: text-anchor, dominant-baseline, dx, dy
    SvgText {
        /// Text content (concatenated from text nodes within `<text>`, `<tspan>`, `<textPath>`).
        text: String,
        /// SVG `x` attribute in user units (baseline x position). 0.0 if absent.
        x: f32,
        /// SVG `y` attribute in user units (baseline y position). 0.0 if absent.
        y: f32,
        /// SVG `dx` attribute in user units (horizontal offset). 0.0 if absent.
        dx: f32,
        /// SVG `dy` attribute in user units (vertical offset). 0.0 if absent.
        dy: f32,
        /// Text anchor alignment: start/middle/end. Defaults to "start" per SVG spec.
        text_anchor: SvgTextAnchor,
        /// Dominant baseline alignment: auto/baseline/hanging/middle/etc. Defaults to "auto" per SVG spec.
        dominant_baseline: SvgDominantBaseline,
        /// Baseline shift (sub/super/length/percentage). Defaults to `baseline` (no shift) per SVG spec.
        baseline_shift: SvgBaselineShift,
        /// Parsed SVG `transform` presentation attribute.
        svg_transform: SvgTransform,
    },
}

/// CSS Pseudo-elements L4 §5.1 — split the `PseudoKind::FirstLetter` segment in
/// `row_items` into `[first_grapheme | rest]` and apply `fl_style` to the first part.
///
/// The segment was already marked by `collect_inline_segments`; this function
/// overrides its style and (when the text is longer than one char) splits it so
/// `wrap_inline_run` applies the correct font metrics to each part independently.
fn apply_first_letter_style(
    row_items: &mut [LayoutBox],
    fl_style: ComputedStyle,
    inherited: &ComputedStyle,
) {
    for item in row_items.iter_mut() {
        let BoxKind::InlineRun { segments, .. } = &mut item.kind else {
            continue;
        };
        for i in 0..segments.len() {
            if segments[i].pseudo_kind != PseudoKind::FirstLetter {
                continue;
            }
            let text = segments[i].text.clone();
            // CSS Pseudo-elements L4 §5.1: leading punctuation + first letter.
            let boundary = first_letter_text_len(&text);
            if boundary < text.len() {
                // Multi-char segment: split into first-letter + rest.
                let rest_text = text[boundary..].to_string();
                let first_text = text[..boundary].to_string();
                let source_node = segments[i].source_node;
                let forced_break = segments[i].forced_break;
                let is_element_box = segments[i].is_element_box;
                let img_src = segments[i].img_src.clone();
                let img_width = segments[i].img_width;
                // The tail keeps the segment's own style — it may sit inside an
                // inline (`<em>Bravo</em>`) whose declarations outlive the split.
                let own_style = segments[i].style.clone();
                segments[i].text = first_text;
                segments[i].style =
                    crate::style::merge_pseudo_inherited(&own_style, inherited, &fl_style);
                let rest = InlineSegment {
                    text: rest_text,
                    style: own_style,
                    pre_space: 0.0,
                    post_space: segments[i].post_space,
                    is_element_box,
                    img_src,
                    img_is_lazy: false,
                    img_width,
                    forced_break,
                    pseudo_kind: PseudoKind::None,
                    source_node,
                    source_char_offset: segments[i].source_char_offset + boundary as u32,
                    bidi_level: 0,
                };
                // Transfer post_space from first-letter to rest.
                segments[i].post_space = 0.0;
                segments.insert(i + 1, rest);
            } else {
                // Single-char or empty segment: just layer the pseudo style on.
                segments[i].style = crate::style::merge_pseudo_inherited(
                    &segments[i].style, inherited, &fl_style,
                );
            }
            return;
        }
    }
}

/// CSS Pseudo-elements L4 §5.1 — byte length of the `::first-letter` text unit
/// at the start of `text`: leading whitespace (raw segment text keeps source
/// newlines/indent until wrap-time collapsing) plus leading punctuation plus
/// the first letter itself.
///
/// Phase 0 approximation: char-level (no grapheme clustering), leading
/// punctuation only (the spec also includes punctuation immediately following
/// the letter); `white-space: pre` significance of the swallowed leading
/// whitespace is ignored. Returns `text.len()` when no letter is found.
fn first_letter_text_len(text: &str) -> usize {
    for (i, c) in text.char_indices() {
        if c.is_whitespace() || is_first_letter_punctuation(c) {
            continue;
        }
        return i + c.len_utf8();
    }
    text.len()
}

/// True for punctuation that joins the `::first-letter` text unit
/// (CSS Pseudo-elements L4 §5.1: Unicode Ps/Pe/Pi/Pf/Po classes; approximated
/// as ASCII punctuation + common typographic quotes — no Unicode tables yet).
fn is_first_letter_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(c, '«' | '»' | '“' | '”' | '‘' | '’' | '„' | '‚' | '‹' | '›')
}

/// CSS Pseudo-elements L4 §5.2 — `::first-letter` layout split, float variant
/// (drop cap, BB-2).
///
/// When the `::first-letter` rule contains `float: left|right`, the first-letter
/// segment (already split out and styled by `apply_first_letter_pseudo`) is
/// removed from its `InlineRun` and promoted to a block-level float `LayoutBox`;
/// the parent block's float machinery then places it and narrows the remaining
/// text lines around it. Returns `None` when no `FirstLetter` segment exists.
///
/// Box structure: the outer `Block` carries the full ::first-letter style
/// (float, margins, padding, border, background); the inner anonymous
/// `InlineRun` holds the single letter segment and supplies the line metrics
/// (::first-letter `font-size` × `line-height`). An `InlineRun` emptied by the
/// extraction is dropped from `row_items`.
// CSS: ::first-letter — P4 wires further drop-cap properties on top of this
// split (initial-letter, initial-letter-align).
fn extract_first_letter_float(
    row_items: &mut Vec<LayoutBox>,
    fl_style: &ComputedStyle,
) -> Option<LayoutBox> {
    for ri in 0..row_items.len() {
        let BoxKind::InlineRun { segments, .. } = &mut row_items[ri].kind else {
            continue;
        };
        let Some(pos) = segments.iter().position(|s| s.pseudo_kind == PseudoKind::FirstLetter)
        else {
            continue;
        };
        let mut seg = segments.remove(pos);
        seg.pre_space = 0.0;
        seg.post_space = 0.0;
        // Strip leading source whitespace (raw newlines/indent from pretty-printed
        // HTML): it would inflate the drop cap's max-content shrink-to-fit width.
        let ws_len = seg.text.len() - seg.text.trim_start().len();
        if ws_len > 0 {
            seg.text.drain(..ws_len);
            seg.source_char_offset += ws_len as u32;
        }
        let node = seg.source_node;
        if segments.is_empty() {
            row_items.remove(ri);
        }
        // Inner anonymous run: ::first-letter font metrics for the line box,
        // but it must not itself float, clear, or indent inside the drop cap.
        let mut inner_style = anon_style(fl_style);
        inner_style.float_side = FloatSide::None;
        inner_style.clear = ClearSide::None;
        inner_style.text_indent = Length::Px(0.0);
        let inner = LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(inner_style),
            kind: BoxKind::InlineRun { segments: vec![seg], lines: vec![], first_line_style: None },
            children: vec![],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        };
        let mut outer_style = fl_style.clone();
        outer_style.display = Display::Block;
        outer_style.text_indent = Length::Px(0.0);
        return Some(LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(outer_style),
            kind: BoxKind::Block,
            children: vec![inner],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        });
    }
    None
}

/// CSS Inline Layout L3 §5 — `initial-letter` drop cap (Phase 0).
///
/// Promotes the block's first-letter segment (already marked
/// `PseudoKind::FirstLetter` by `collect_inline_segments`) to an inline-start
/// float `Block` whose glyph spans `size` lines and which reserves `sink` text
/// lines beside it. Reuses the float wrap machinery (the surrounding text lines
/// narrow around the float automatically).
///
/// Phase 0 approximations: inline-start = left (LTR only); the precise
/// cap-height/baseline alignment of the spec is approximated by
/// `font-size = size × parent line-height`, and the glyph box is clipped to the
/// reserved `sink`-line height. `letter_style` carries the optional
/// `::first-letter` author style (color/font); falls back to an anonymous style
/// derived from `base`.
///
/// `base` — the parent block style (supplies the reference `line-height` in px).
/// `size` — cap height in lines (> 1). `sink` — in-flow lines (`0` = `floor(size)`).
/// Returns `None` when no first-letter segment is present (e.g. the block opens
/// with an image or is empty).
fn extract_initial_letter(
    row_items: &mut Vec<LayoutBox>,
    base: &ComputedStyle,
    letter_style: Option<&ComputedStyle>,
    size: f32,
    sink: u32,
) -> Option<LayoutBox> {
    for ri in 0..row_items.len() {
        let BoxKind::InlineRun { segments, .. } = &mut row_items[ri].kind else {
            continue;
        };
        let Some(pos) = segments.iter().position(|s| s.pseudo_kind == PseudoKind::FirstLetter)
        else {
            continue;
        };
        // Split off the first-letter unit. With a `::first-letter` rule present,
        // `apply_first_letter_pseudo` has already isolated the letter (rest is a
        // sibling segment); without one (initial-letter set on the element), the
        // whole opening text segment is still marked FirstLetter and must be
        // split here.
        let boundary = first_letter_text_len(&segments[pos].text);
        let mut seg = segments[pos].clone();
        let rest_text = seg.text.split_off(boundary);
        // Strip leading source whitespace (pretty-print newlines/indent): it
        // would inflate the cap's shrink-to-fit width.
        let ws_len = seg.text.len() - seg.text.trim_start().len();
        if ws_len > 0 {
            seg.text.drain(..ws_len);
            seg.source_char_offset += ws_len as u32;
        }
        if seg.text.is_empty() {
            // No actual letter (all whitespace/punctuation) — leave content as-is.
            return None;
        }
        seg.pre_space = 0.0;
        seg.post_space = 0.0;
        let node = seg.source_node;
        // Put the remainder back into the run (or drop the now-empty run).
        if rest_text.is_empty() {
            segments.remove(pos);
            if segments.is_empty() {
                row_items.remove(ri);
            }
        } else {
            let rest = &mut segments[pos];
            rest.source_char_offset += boundary as u32;
            rest.text = rest_text;
            rest.pseudo_kind = PseudoKind::None;
            rest.pre_space = 0.0;
        }

        // Used line-height in px: the engine stores `line_height` as a multiplier
        // of `font_size` (relative) or px/font_size (absolute), so the product is
        // the px line box height in both cases (mirrors `font_size * line_height`
        // used throughout layout/paint).
        let ref_line = (base.font_size * base.line_height).max(1.0);
        let cap_font = (size * ref_line).max(1.0);
        let sink_lines = if sink == 0 { size.floor().max(1.0) as u32 } else { sink };
        let sink_px = sink_lines as f32 * ref_line;

        // Inner anonymous run: enlarged glyph metrics, never floats/indents itself.
        let mut inner_style = letter_style.cloned().unwrap_or_else(|| anon_style(base));
        inner_style.font_size = cap_font;
        // Tight line box equal to the cap font size (ratio 1.0): `line_height` is a
        // multiplier of `font_size`, so 1.0 → line box height == cap_font.
        inner_style.line_height = 1.0;
        inner_style.line_height_is_relative = true;
        inner_style.float_side = FloatSide::None;
        inner_style.clear = ClearSide::None;
        inner_style.text_indent = Length::Px(0.0);
        inner_style.initial_letter_size = 1.0;
        inner_style.initial_letter_sink = 0;
        seg.style = inner_style.clone();

        let inner = LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(inner_style),
            kind: BoxKind::InlineRun { segments: vec![seg], lines: vec![], first_line_style: None },
            children: vec![],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        };

        // Outer block: inline-start float reserving exactly `sink` text lines.
        let mut outer_style = letter_style.cloned().unwrap_or_else(|| anon_style(base));
        outer_style.display = Display::Block;
        outer_style.float_side = FloatSide::Left;
        outer_style.clear = ClearSide::None;
        outer_style.text_indent = Length::Px(0.0);
        outer_style.initial_letter_size = 1.0;
        outer_style.initial_letter_sink = 0;
        outer_style.height = Some(Length::Px(sink_px));
        outer_style.overflow_x = crate::style::Overflow::Hidden;
        outer_style.overflow_y = crate::style::Overflow::Hidden;
        return Some(LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(outer_style),
            kind: BoxKind::Block,
            children: vec![inner],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        });
    }
    None
}

/// True for the synthesized drop-cap box produced by
/// [`extract_first_letter_float`]: a float `Block` whose only child is an
/// `InlineRun` with a single `PseudoKind::FirstLetter` segment. Used to keep
/// `::first-line` overrides off the drop cap (CSS Pseudo-elements L4 §5.2:
/// ::first-letter wins where the two pseudo-elements conflict).
fn is_first_letter_box(b: &LayoutBox) -> bool {
    b.style.float_side != FloatSide::None
        && b.children.len() == 1
        && matches!(
            &b.children[0].kind,
            BoxKind::InlineRun { segments, .. }
                if segments.len() == 1 && segments[0].pseudo_kind == PseudoKind::FirstLetter
        )
}

/// CSS Pseudo-elements L4 §3.1 — apply `::first-line` style overrides after layout.
///
/// Must be called after `lay_out` has populated `InlineRun.lines` with `InlineFrag`s.
/// Walks the box tree; for each block-level box that has a `::first-line` rule on
/// its DOM node, overrides the style of every frag on the first formatted line
/// (`is_first_line == true`).
///
/// BUG-341 S23: the walk is skipped outright when the sheet has no
/// `::first-line` rule. It probed every block box in the document — 123 probes
/// per interaction cycle on `chrome.html`, none of which could ever hit,
/// because that sheet has no such rule. The predicate is over the same `sheet`
/// this function would consult, so skipping is exactly behaviour-preserving.
pub(crate) fn apply_first_line_pseudo_styles(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) {
    if !crate::style::sheet_targets_pseudo(sheet, viewport, dark_mode, "first-line") {
        return;
    }
    apply_first_line_pseudo_styles_inner(b, doc, sheet, viewport, dark_mode);
}

/// The recursive body of [`apply_first_line_pseudo_styles`], split out so the
/// sheet-level predicate is evaluated once per pass instead of once per box.
fn apply_first_line_pseudo_styles_inner(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) {
    // CSS Pseudo-elements L4 §5.2 (BB-2): never apply ::first-line inside the
    // synthesized drop-cap box — ::first-letter wins where the two conflict.
    if is_first_letter_box(b) {
        return;
    }
    for child in &mut b.children {
        apply_first_line_pseudo_styles_inner(child, doc, sheet, viewport, dark_mode);
    }
    if !matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot) {
        return;
    }
    let Some(fl_style) = compute_pseudo_element_style(doc, b.node, "first-line", sheet, &b.style, viewport, dark_mode) else {
        return;
    };
    // Find the first InlineRun child (or inside InlineBlockRow) and apply.
    // §3.4: layer the pseudo style over what each frag inherited from `b` —
    // an inner `<b>`/`<em>`/`style="…"` keeps its own declarations.
    let base = b.style.clone();
    let restyle = |lines: &mut Vec<Vec<InlineFrag>>| {
        if let Some(first_line) = lines.first_mut() {
            for frag in first_line.iter_mut() {
                if frag.is_first_line {
                    frag.style =
                        crate::style::merge_pseudo_inherited(&frag.style, &base, &fl_style);
                }
            }
        }
    };
    let mut applied = false;
    'find: for child in &mut b.children {
        match &mut child.kind {
            BoxKind::InlineRun { lines, .. } => {
                restyle(lines);
                applied = true;
                break 'find;
            }
            BoxKind::InlineBlockRow => {
                for row_child in &mut child.children {
                    if let BoxKind::InlineRun { lines, .. } = &mut row_child.kind {
                        restyle(lines);
                        applied = true;
                        break 'find;
                    }
                }
            }
            _ => {}
        }
    }
    let _ = applied;
}

/// Byte offsets of each whitespace-separated word start in `text`
/// (same word boundaries as `str::split_whitespace`).
fn word_start_offsets(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut in_word = false;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            in_word = false;
        } else if !in_word {
            starts.push(i);
            in_word = true;
        }
    }
    starts
}

/// CSS Pseudo-elements L4 §3.1 — partition `segments` into
/// `(consumed by the first formatted line, remainder)`.
///
/// `line0` is the first line produced by the ::first-line wrap pass; its frags
/// appear in segment order and never span segments, so consumption is counted
/// word-by-word with the same boundaries as `str::split_whitespace` (matching
/// `wrap_inline_run`). A partially consumed segment is split at the word
/// boundary: the head keeps the segment's `pre_space` (its inline box opened on
/// line 0, `post_space` → 0), the tail keeps `post_space` (`pre_space` → 0,
/// `source_char_offset` advanced by the cut byte offset).
///
/// `preserves_ws` (white-space: pre / pre-wrap): each non-empty segment before
/// the first forced break produced exactly one frag — whole segments up to and
/// including that break are consumed.
fn split_segments_at_first_line(
    segments: &[InlineSegment],
    line0: &[InlineFrag],
    preserves_ws: bool,
) -> (Vec<InlineSegment>, Vec<InlineSegment>) {
    let mut consumed: Vec<InlineSegment> = Vec::new();
    let mut idx = 0usize;

    if preserves_ws {
        for _ in line0 {
            // Empty non-break text segments produce no frag — consume silently.
            while idx < segments.len()
                && segments[idx].text.is_empty()
                && !segments[idx].forced_break
                && segments[idx].img_src.is_none()
            {
                consumed.push(segments[idx].clone());
                idx += 1;
            }
            if idx < segments.len() {
                consumed.push(segments[idx].clone());
                idx += 1;
            }
        }
        // The forced break that terminated line 0 belongs to it.
        if idx < segments.len() && segments[idx].forced_break {
            consumed.push(segments[idx].clone());
            idx += 1;
        }
        return (consumed, segments[idx..].to_vec());
    }

    // Word-level consumption for collapsing white-space modes.
    let mut words_taken = 0usize; // words already consumed from segments[idx]
    for frag in line0 {
        if frag.img_src.is_some() {
            // Advance to the img segment, consuming exhausted text segments.
            while idx < segments.len() && segments[idx].img_src.is_none() {
                consumed.push(segments[idx].clone());
                idx += 1;
                words_taken = 0;
            }
            if idx < segments.len() {
                consumed.push(segments[idx].clone());
                idx += 1;
            }
            continue;
        }
        let mut need = frag.text.split_whitespace().count();
        while need > 0 && idx < segments.len() {
            let seg = &segments[idx];
            if seg.img_src.is_some() || seg.forced_break {
                consumed.push(seg.clone());
                idx += 1;
                words_taken = 0;
                continue;
            }
            let total = seg.text.split_whitespace().count();
            let avail = total.saturating_sub(words_taken);
            if avail <= need {
                need -= avail;
                consumed.push(seg.clone());
                idx += 1;
                words_taken = 0;
            } else {
                words_taken += need;
                need = 0;
            }
        }
    }

    let mut rest: Vec<InlineSegment> = Vec::new();
    if words_taken > 0 && idx < segments.len() {
        // Partially consumed segment: split at the word boundary.
        let seg = &segments[idx];
        let starts = word_start_offsets(&seg.text);
        if words_taken < starts.len() {
            let cut = starts[words_taken];
            let mut head = seg.clone();
            head.text = seg.text[..cut].trim_end().to_string();
            head.post_space = 0.0;
            consumed.push(head);
            let mut tail = seg.clone();
            tail.text = seg.text[cut..].to_string();
            tail.pre_space = 0.0;
            tail.source_char_offset = seg.source_char_offset + cut as u32;
            rest.push(tail);
        } else {
            consumed.push(seg.clone());
        }
        idx += 1;
    }
    rest.extend(segments[idx..].iter().cloned());
    (consumed, rest)
}

/// CSS Pseudo-elements L4 §3.1 — ::first-line layout split (BB-1).
///
/// Post-layout pass: walks the box tree and, for every `InlineRun` carrying a
/// `first_line_style`, splits the first formatted line into its own `InlineRun`
/// box styled with the ::first-line style; the remainder keeps the base style.
/// Paint computes line height as `style.font_size * style.line_height` per box,
/// so the split gives the first line its correct (possibly larger) line box
/// height with no paint-side changes. Single-line runs are restyled in place.
/// Idempotent: `first_line_style` is cleared on every produced box.
/// The box receives the full `::first-line` `ComputedStyle` and the
/// `BoxRole::Pseudo(PseudoKind::FirstLine)` role, so background,
/// text-decoration, color and font all take effect at paint time — the role is
/// what lets `emit_inline_run` tell this box from an anonymous inline run,
/// whose `anon_style` has no background of its own (BUG-432).
pub(crate) fn split_first_line_boxes(b: &mut LayoutBox) {
    for child in &mut b.children {
        split_first_line_boxes(child);
    }
    let mut i = 0;
    while i < b.children.len() {
        let child = &mut b.children[i];
        let BoxKind::InlineRun { segments, lines, first_line_style } = &mut child.kind else {
            i += 1;
            continue;
        };
        let Some(fls) = first_line_style.take() else {
            i += 1;
            continue;
        };
        if lines.len() < 2 {
            // The whole run is the first formatted line: restyle the box in place
            // so paint uses the ::first-line font metrics for its single line box.
            child.style = Arc::new(*fls);
            child.origin.role = BoxRole::Pseudo(PseudoKind::FirstLine);
            i += 1;
            continue;
        }
        let preserves = child.style.white_space.preserves_whitespace();
        let (consumed_segs, rest_segs) =
            split_segments_at_first_line(segments, &lines[0], preserves);
        let line0 = lines[0].clone();
        let rest_lines: Vec<Vec<InlineFrag>> = lines[1..].to_vec();
        let fl_h = fls.font_size * fls.line_height;
        let base_h = child.style.font_size * child.style.line_height;
        let rect = child.rect;
        let box2 = LayoutBox {
            node: child.node,
            rect: Rect::new(rect.x, rect.y + fl_h, rect.width, rest_lines.len() as f32 * base_h),
            style: child.style.clone(),
            kind: BoxKind::InlineRun {
                segments: rest_segs,
                lines: rest_lines,
                first_line_style: None,
            },
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: Default::default(),
            // Fragment of the same InlineRun, split after ::first-line — same
            // provenance as the box it was split from, not a new box role.
            origin: child.origin,
        };
        // Reuse the original box as the first-line box.
        child.style = Arc::new(*fls);
        child.rect.height = fl_h;
        child.kind = BoxKind::InlineRun {
            segments: consumed_segs,
            lines: vec![line0],
            first_line_style: None,
        };
        // BUG-432: tag the box so paint can tell it from an ordinary anonymous
        // inline run and draw the pseudo-element's own background. Every other
        // `InlineRun` is built through `anon_style`, which clears
        // `background_color`; this one carries the full ::first-line style.
        child.origin.role = BoxRole::Pseudo(PseudoKind::FirstLine);
        b.children.insert(i + 1, box2);
        i += 2;
    }
}

mod entry;
use entry::{is_invisible_control, strip_invisible_controls};
#[cfg(test)]
use entry::{apply_font_size_adjust, font_size_adjust_used};
pub use entry::{
    build_iframe_document, canvas_background_color, lay_out_incremental, layout, layout_measured,
    layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental,
    layout_mutation_incremental_restyle, layout_streaming_incremental,
};

mod inline_build;
use inline_build::{
    anon_inline_block_row, anon_inline_run, anon_style, breaks_inline_row, build_anon_text_item,
    collect_inline_segments, control_value_segments, flatten_contents, inject_marker, inject_pseudo,
    inline_baseline, inline_run_advance, inline_run_lead_space, inline_v_align, is_atomic_inline_level,
    is_inline_content, li_ordinal, probe_display, split_inline_pieces, assign_first_line_style,
    InlineEscape,
};

mod build;
use build::{build_box, build_box_or_reuse};
pub use build::incremental_build_box;

mod intrinsic;
use intrinsic::{
    flex_auto_base_main_width, flex_item_max_main_outer, flex_item_min_main_width,
    max_content_outer_width, min_content_outer_width, preferred_inline_block_width,
};

mod shapes_floats;
use shapes_floats::{
    parse_circle_px, parse_shape_ellipse_px, parse_shape_inset_px, parse_shape_path_px,
    parse_shape_polygon_px, shift_tree, shift_y_box, FloatContext, ShapeEllipse, ShapeInset,
    ShapePolygon,
};
// Used only by `mod tests` (super::super::X) — never called from this file's
// own non-test code.
#[cfg(test)]
use shapes_floats::{inset_corner_inward, polygon_left_edge_at_y, polygon_right_edge_at_y};

mod bfc;
mod layout_dispatch;

pub(crate) use bfc::lay_out_for_vertical;
use bfc::{
    collapsed_bottom_margin, collapsed_top_margin, contained_content_height, establishes_bfc,
    has_in_flow_content, last_collapsible_child,
};
use layout_dispatch::{lay_out, lay_out_with_used_size};

#[cfg(test)]
mod tests;

