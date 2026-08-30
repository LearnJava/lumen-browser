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

/// CSS Scoping L1 — collect each shadow tree's author stylesheet, keyed by its
/// shadow-host `NodeId`, from the `<style>` elements inside every shadow root.
///
/// These sheets are installed via [`crate::style::set_shadow_sheets`] at the start
/// of every layout pass so the cascade can apply `:host`/`:host()`/`::slotted()`
/// rules in their proper scope. The page's document `<style>` is collected
/// separately by the shell (`extract_style_blocks`), which does NOT descend into
/// shadow roots — so the two collections never overlap.
fn build_shadow_sheets(doc: &Document) -> std::collections::HashMap<NodeId, Stylesheet> {
    let mut map = std::collections::HashMap::new();
    if doc.is_empty() {
        return map;
    }
    for i in 0..doc.len() {
        let host = NodeId::from_index(i);
        if !doc.is_shadow_host(host) {
            continue;
        }
        let Some(sr) = doc.shadow_root_of(host) else { continue };
        let mut css = String::new();
        collect_shadow_style_css(doc, sr, &mut css);
        if !css.trim().is_empty() {
            map.insert(host, lumen_css_parser::parse(&css));
        }
    }
    map
}

/// Concatenate the text of all `<style>` elements within a shadow subtree.
/// Walks DOM children only; nested shadow roots are not DOM children, so a nested
/// host's own `<style>` stays in its own scope (collected by the outer loop).
fn collect_shadow_style_css(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
                out.push('\n');
            }
        }
        return;
    }
    for &child in &node.children {
        collect_shadow_style_css(doc, child, out);
    }
}

/// Lay out a document without a text measurer. For tests and headless dump modes.
/// Invalidates the rule-index cache before the cascade so stale hits are impossible.
pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    // Prevent stale RULE_IDX_CACHE hits when a new sheet lands at the same address as a freed one.
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    let counters = precompute_counters(doc, sheet, viewport, &flat, false);
    let registry = build_counter_style_registry(sheet);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, false, None);
    propagate_canvas_background(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let null_hp = NullHyphenationProvider;
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), None, viewport, init_pcb, &null_hp, false);
    apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, false);
    // CSS Container Queries L1: second pass applies @container rules + re-layout.
    apply_container_styles(&mut root, doc, sheet, viewport, None, &null_hp, false);
    // CSS Anchor Positioning L1: post-layout pass repositions anchored elements.
    apply_anchor_positions(&mut root, viewport);
    // CSS Pseudo-elements L4 §3.1: split first formatted lines into own boxes (BB-1).
    split_first_line_boxes(&mut root);
    #[cfg(debug_assertions)]
    crate::invariants::check_geometry(&root);
    root
}

/// Layout without a text measurer. For tests and headless modes; uses `layout_measured_hyp` with `dark_mode=false`.
pub fn layout_measured(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> LayoutBox {
    let null_hp = NullHyphenationProvider;
    layout_measured_hyp(doc, sheet, viewport, measurer, &null_hp, false)
}

/// Layout with a real hyphenation provider (for `hyphens: auto`).
/// `dark_mode` drives `@media (prefers-color-scheme: dark)` matching throughout
/// the cascade — shell reads the value from `Lumen.dark_mode` (OS preference via winit).
pub fn layout_measured_hyp(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) -> LayoutBox {
    layout_measured_hyp_with_counters(doc, sheet, viewport, measurer, hp, dark_mode).0
}

/// Like [`layout_measured_hyp`], but also returns the [`CounterMap`] the cascade
/// pass produced (BUG-341 S2).
///
/// The `CounterMap` carries the full per-node `ComputedStyle` cascade cache (its
/// `styles` field — see [`CounterMap::styles`]) that `build_box` reused. Persisting
/// it across interaction cycles is the foundation of the incremental cascade
/// (BUG-341 S3+): the incremental path must reproduce this exact map for the same
/// final state, and the `incr == full` differential tests assert that.
///
/// [`layout_measured_hyp`] is a thin wrapper that discards the map, so this
/// function carries the real body and there is no behavioural difference between
/// them.
pub fn layout_measured_hyp_with_counters(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) -> (LayoutBox, CounterMap) {
    let _prof = lumen_core::profile::scope("layout_measured_hyp");
    lumen_core::tracy_zone!("layout_measured_hyp");
    // Invalidate the rule-index cache before each layout pass to prevent
    // stale hits when a new stylesheet lands at the same pointer as a freed one.
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    let counters = {
        let _prof = lumen_core::profile::scope("precompute_counters");
        lumen_core::tracy_zone!("precompute_counters");
        precompute_counters(doc, sheet, viewport, &flat, dark_mode)
    };
    let registry = build_counter_style_registry(sheet);
    let mut root = {
        let _prof = lumen_core::profile::scope("build_box");
        lumen_core::tracy_zone!("build_box");
        build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, None)
    };
    propagate_canvas_background(doc, &mut root);
    // CSS Fonts L5 §4 — resolve `font-size-adjust` against the real font x-height
    // before measurement, so both line wrapping and paint use the scaled size.
    apply_font_size_adjust(&mut root, measurer);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    {
        let _prof = lumen_core::profile::scope("lay_out");
        lumen_core::tracy_zone!("lay_out");
        lay_out(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), Some(measurer), viewport, init_pcb, hp, false);
    }
    {
        let _prof = lumen_core::profile::scope("post_layout_passes");
        lumen_core::tracy_zone!("post_layout_passes");
        apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, dark_mode);
        apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp, dark_mode);
        // CSS Anchor Positioning L1: post-layout pass repositions anchored elements.
        apply_anchor_positions(&mut root, viewport);
        // CSS Pseudo-elements L4 §3.1: split first formatted lines into own boxes (BB-1).
        split_first_line_boxes(&mut root);
    }
    #[cfg(debug_assertions)]
    crate::invariants::check_geometry(&root);
    (root, counters)
}

/// Incremental re-layout pass: skips clean subtrees, re-lays out only dirty ones.
///
/// `root` must be a previously laid-out `LayoutBox` (from `layout_measured_hyp`).
/// Call [`crate::incremental::mark_dirty`] on changed nodes first.
///
/// Internally enables [`INCREMENTAL_LAYOUT_MODE`] so that `lay_out` returns early
/// (translating the subtree to its new position) for any node with
/// [`crate::incremental::DirtyBits::CLEAN`]. After this call all dirty bits are
/// cleared automatically via [`crate::incremental::clear_dirty`].
///
/// Parameters match `lay_out` / `layout_measured_hyp`. Phase 0 limitation:
/// container-query re-evaluation and anchor positioning are not re-run here
/// (they rely on a full layout pass); add a full `layout_measured_hyp` call when
/// those features are required.
#[allow(clippy::too_many_arguments)]
pub fn lay_out_incremental(
    root: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    INCREMENTAL_LAYOUT_MODE.with(|m| m.set(true));
    lay_out(root, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp, false);
    INCREMENTAL_LAYOUT_MODE.with(|m| m.set(false));
    crate::incremental::clear_dirty(root);
    #[cfg(debug_assertions)]
    crate::invariants::check_geometry(root);
}

/// Streaming incremental layout (PH1-2b).
///
/// Builds a fresh box tree from `doc` + `sheet` — which, during a streaming load,
/// grows by nodes appended at the end each tick — then reuses laid-out geometry
/// from `prev` (the previous tick's result) for every subtree whose node id, box
/// kind payload and computed style are unchanged. Only new or changed subtrees
/// are re-laid-out; unchanged prefix siblings are repositioned in O(1) by the
/// `lay_out` incremental fast path (a zero-delta translate when content is merely
/// appended below them).
///
/// `prev` must be a tree produced by an earlier `layout_streaming_incremental`
/// or `layout_measured*` call on an ancestor DOM of `doc` (same, stable node ids
/// — the incremental tree builder only appends new ids). When the stylesheet
/// changed since `prev` was built, the per-box style comparison naturally marks
/// the affected boxes dirty and re-lays them out.
///
/// Post-layout passes (container queries, anchor positioning, first-line split)
/// are NOT re-run here — same Phase 0 limitation as [`lay_out_incremental`]. The
/// final `LoadDone` pipeline applies them via a full `layout_measured_hyp`.
#[allow(clippy::too_many_arguments)]
pub fn layout_streaming_incremental(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    prev: &LayoutBox,
) -> LayoutBox {
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    let counters = precompute_counters(doc, sheet, viewport, &flat, dark_mode);
    let registry = build_counter_style_registry(sheet);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, None);
    propagate_canvas_background(doc, &mut root);
    apply_font_size_adjust(&mut root, measurer);
    // Every freshly-built box needs layout; graft clears the bit on reusable
    // subtrees so the incremental pass only re-lays-out new/changed content.
    crate::incremental::mark_subtree_dirty(&mut root);
    crate::incremental::graft_geometry(&mut root, prev);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    lay_out_incremental(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), Some(measurer), viewport, init_pcb, hp);
    root
}

/// Incremental re-layout for JS DOM mutations (ADR-016 M4).
///
/// Functionally equivalent to [`layout_measured_hyp`] but avoids re-computing
/// geometry for subtrees whose [`crate::style::ComputedStyle`] did not change:
/// the cascade runs in full (same as [`layout_measured_hyp`]), then
/// [`crate::incremental::graft_geometry`] copies laid-out rects from `prev` for
/// unchanged subtrees (marking them [`crate::incremental::DirtyBits::CLEAN`]),
/// and only dirty subtrees are re-laid-out by [`lay_out_incremental`]. All
/// post-layout passes (container queries, anchor positioning, `::first-line`
/// split) run afterwards, matching [`layout_measured_hyp`] semantics exactly.
///
/// Typical speedup: ~10× on a single-node class toggle on a large page (the
/// unchanged siblings are translated in O(k), not re-laid-out). For mutations
/// where every node's style changes (e.g. a viewport-wide media query flip) the
/// overhead of `graft_geometry` is small compared to the full geometry pass.
///
/// `prev` must be a tree produced by an earlier [`layout_measured_hyp`] or
/// `layout_mutation_incremental` call on a compatible DOM (same stable node ids).
/// When `prev` is unavailable (first load) call [`layout_measured_hyp`] instead.
#[allow(clippy::too_many_arguments)]
pub fn layout_mutation_incremental(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    prev: &LayoutBox,
) -> LayoutBox {
    // Full cascade + graft: unchanged-style subtrees become CLEAN.
    let mut root = layout_streaming_incremental(doc, sheet, viewport, measurer, hp, dark_mode, prev);
    // Post-layout passes — same set as layout_measured_hyp, same order.
    apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, dark_mode);
    apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp, dark_mode);
    apply_anchor_positions(&mut root, viewport);
    split_first_line_boxes(&mut root);
    root
}

/// BUG-341 S5: incremental re-layout for a pure interactive-state transition
/// (`:hover`/`:focus`/`:active`), combining the S3 incremental cascade with
/// [`layout_mutation_incremental`]'s existing geometry-graft reuse.
///
/// Like [`layout_mutation_incremental`], but the cascade itself only
/// re-derives `delta.dirty_roots` and their subtrees
/// ([`crate::counters::incremental_precompute_counters`]) instead of every
/// node — the dominant cost S1's profiling found (brief §1,
/// `precompute_counters` at 53% of the cycle). BUG-341 S15: box-build is
/// skipped too when [`set_incremental_box_build`] is on — whole `LayoutBox`
/// subtrees are cloned from `prev` wherever [`CounterMap::clean_subtrees`]
/// licenses it ([`incremental_build_box`]) — and [`crate::incremental::
/// graft_geometry`] then reuses the layout geometry the same way it already
/// did. With the flag off this builds fresh boxes with the plain `build_box`,
/// exactly like [`layout_streaming_incremental`] does.
///
/// `delta.prev_styles` must be the [`CounterMap::styles`] this same document
/// produced on the previous cycle (this function's own returned `CounterMap`,
/// or [`layout_measured_hyp_with_counters`] for the first cycle). The caller
/// is responsible for only using this entry point when nothing besides
/// interactive state changed since `prev` — `delta.dom_content_stable` must be
/// `true` (a DOM/attribute mutation can change content `build_box` reads,
/// e.g. text or attribute values, in ways a style-only comparison does not
/// catch) and `delta.dirty_roots` should come from
/// [`crate::style::restyle_root_set_for_state_change`]. See
/// [`crate::counters::RestyleDelta`]'s own doc comment for the full
/// correctness precondition.
///
/// BUG-341 S19: `prev` is taken **by value** — this pass moves the reusable
/// subtrees out of it into the tree it returns rather than copying them, so the
/// previous tree does not survive the call. Callers persist the *returned* tree
/// as the next cycle's `prev`; one that also needs the old tree afterwards must
/// clone it before handing it over.
///
/// Returns the fresh `CounterMap` alongside the tree so the caller can carry
/// its `styles()` forward as the next cycle's `prev_styles`.
#[allow(clippy::too_many_arguments)]
pub fn layout_mutation_incremental_restyle(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    mut prev: LayoutBox,
    delta: crate::counters::RestyleDelta<'_>,
) -> (LayoutBox, CounterMap) {
    // Stage scopes deliberately reuse `layout_measured_hyp_with_counters`'
    // names so `LUMEN_PROFILE_TREE=1` yields a directly comparable split of
    // the *incremental* path — BUG-341 §1's profile only ever described the
    // full pass, and every slice since S3 has been reasoning from it.
    let _prof = lumen_core::profile::scope("layout_mutation_incremental_restyle");
    crate::content_visibility::reset_cv_skipped();
    let root_style = ComputedStyle::root();
    let flat = {
        let _prof = lumen_core::profile::scope("build_flat_tree");
        build_flat_tree(doc)
    };
    {
        // BUG-341 S26: scoped because it is per-pass whole-document work that
        // the delta cannot shrink — it walks every node asking `is_shadow_host`,
        // with no `shadow_roots.is_empty()` fast path of its own (unlike
        // `build_flat_tree`). Unscoped, it was invisible to every stage profile
        // this track has taken.
        let _prof = lumen_core::profile::scope("build_shadow_sheets");
        crate::style::set_shadow_sheets(build_shadow_sheets(doc));
    }
    let counters = {
        let _prof = lumen_core::profile::scope("precompute_counters");
        crate::counters::incremental_precompute_counters(doc, sheet, viewport, &flat, dark_mode, delta)
    };
    let registry = {
        // BUG-341 S26: likewise per-pass, sheet-wide and delta-independent.
        let _prof = lumen_core::profile::scope("counter_style_registry");
        build_counter_style_registry(sheet)
    };
    let mut root = {
        let _prof = lumen_core::profile::scope("build_box");
        // BUG-341 S15: reuse whole `LayoutBox` subtrees from `prev` wherever
        // `CounterMap::clean_subtrees` licenses it (the S4 mechanism), instead
        // of rebuilding a tree that `graft_geometry` is about to graft straight
        // back onto the previous geometry. S4's own measurement rejected this
        // because `index_by_node`'s whole-prev-tree hash outweighed the ~8%
        // `build_box` share it saved; both halves of that trade have since
        // moved — `build_box` is now ~60% of the incremental cycle (S14's
        // profile) and, after S13/S14, the dirty set on a chrome interaction is
        // empty, so `clean_subtrees` licenses nearly the whole tree.
        // BUG-341 S19: this is where `prev` is consumed — the reusable subtrees
        // are moved into the tree being built, not copied out of it.
        if incremental_box_build_enabled() {
            incremental_build_box(
                doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, &mut prev,
            )
        } else {
            build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode, None)
        }
    };
    {
        // BUG-341 S26: two whole-tree walks between the build and graft stages,
        // both previously unscoped.
        let _prof = lumen_core::profile::scope("post_build_tree_walks");
        propagate_canvas_background(doc, &mut root);
        apply_font_size_adjust(&mut root, measurer);
    }
    {
        let _prof = lumen_core::profile::scope("graft_geometry");
        // Every freshly-built box needs layout; graft clears the bit on reusable
        // subtrees so the incremental pass only re-lays-out new/changed content.
        crate::incremental::mark_subtree_dirty(&mut root);
        // BUG-341 S13: `prev` is a laid-out tree, so its styles carry the used
        // values `lay_out` wrote back into them; `delta.prev_styles` is the
        // unpolluted cascade those boxes were built from, and lets the graft
        // tell "the author's style changed" from "layout wrote its own output
        // here last cycle". Without it, 81 of the chrome document's 318 boxes
        // were rejected on style every single hover flip — every one of them
        // differing only in those used-value fields — plus 41 ancestors
        // dragged along by the reject propagation.
        // BUG-341 S19: `prev` is a husked tree by now — the reusable subtrees
        // are in `root`, and every position they came from carries
        // `DirtyBits::MOVED_OUT`. The graft skips those on the S18 claim before
        // it ever looks at the husk, and rejects any it reaches without one.
        // BUG-341 S24: the delta's cache was moved into `counters` and rewritten
        // in place, so the "what was `prev` built from" view now comes from
        // there — live entries for everything this pass reused, displaced ones
        // for everything it recomputed.
        crate::incremental::graft_geometry_with_cascade(&mut root, &prev, Some(counters.prev_cascade()));
    }
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    {
        let _prof = lumen_core::profile::scope("lay_out");
        lay_out_incremental(
            &mut root, 0.0, 0.0, viewport.width, Some(viewport.height), Some(measurer), viewport, init_pcb, hp,
        );
    }
    {
        let _prof = lumen_core::profile::scope("post_layout_passes");
        // Post-layout passes — same set as layout_measured_hyp/layout_mutation_incremental, same order.
        apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, dark_mode);
        apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp, dark_mode);
        apply_anchor_positions(&mut root, viewport);
        split_first_line_boxes(&mut root);
    }
    (root, counters)
}

/// CSS Fonts L5 §4 — used `font-size` after applying `font-size-adjust`.
///
/// The aspect value of the rendered font is `x_height_px(size) / size`. To make
/// the text's x-height equal `adjust × size`, the size is scaled by
/// `adjust / aspect`. `None` (initial) and `Auto` (use the first available
/// font's own aspect — a no-op for a single font) leave the size unchanged.
fn font_size_adjust_used(style: &ComputedStyle, m: &dyn TextMeasurer) -> f32 {
    use crate::style::FontSizeAdjust;
    let size = style.font_size;
    match style.font_size_adjust {
        FontSizeAdjust::None | FontSizeAdjust::Auto => size,
        FontSizeAdjust::Value(z) => {
            let xh = m.x_height_px(size);
            if size > 0.0 && xh > 0.0 {
                let aspect = xh / size;
                size * z / aspect
            } else {
                size
            }
        }
    }
}

/// Apply `font-size-adjust` to a single style in place (CSS Fonts L5 §4).
///
/// Mutates `font_size` to the x-height-normalised used size. Because an absolute
/// `line-height` (`<length>`/`<percentage>`/`em`/`rem`) computes to a fixed line
/// box that must NOT rescale with the used font-size, the ratio-encoded
/// `line_height` is corrected inversely so the absolute line box stays constant
/// (CSS2 §10.8.1). Relative line-heights (`normal`/`<number>`) keep their ratio
/// and scale with the new size, as the spec requires.
fn apply_font_size_adjust_to_style(style: &mut ComputedStyle, m: &dyn TextMeasurer) {
    use crate::style::FontSizeAdjust;
    if matches!(style.font_size_adjust, FontSizeAdjust::None) {
        return;
    }
    let old_size = style.font_size;
    let new_size = font_size_adjust_used(style, m);
    style.font_size = new_size;
    if !style.line_height_is_relative && new_size > 0.0 {
        style.line_height = style.line_height * old_size / new_size;
    }
}

/// CSS Fonts L5 §4 — post-build pass rewriting `font_size` wherever
/// `font-size-adjust` is a number, using the measurer's real x-height.
///
/// Runs after `build_box` and before `lay_out`: mutating `style.font_size` here
/// makes both inline measurement and the display list (which reads
/// `frag.style.font_size`) pick up the scaled size from a single source. Inline
/// text segments carry their own cloned style, so they are adjusted too.
fn apply_font_size_adjust(b: &mut LayoutBox, m: &dyn TextMeasurer) {
    // BUG-341 S12: the `None` test lives here rather than only inside
    // `apply_font_size_adjust_to_style`, because reaching for `Arc::make_mut`
    // on a style shared with the cascade cache would deep-copy it — on every
    // box of the document, for a property almost no box sets.
    if !matches!(b.style.font_size_adjust, crate::style::FontSizeAdjust::None) {
        apply_font_size_adjust_to_style(Arc::make_mut(&mut b.style), m);
    }
    if let BoxKind::InlineRun { segments, .. } = &mut b.kind {
        for seg in segments.iter_mut() {
            apply_font_size_adjust_to_style(&mut seg.style, m);
        }
    }
    for child in &mut b.children {
        apply_font_size_adjust(child, m);
    }
}

/// Parse inline HTML from an `<iframe srcdoc="...">` attribute (HTML spec §4.8.5).
///
/// Returns the parsed `Document` ready for sub-document layout. The document
/// has no base URL — relative resource references inside `srcdoc` HTML are
/// interpreted as `about:blank`-relative (effectively unresolvable until
/// Phase 1 navigation wiring).
pub fn build_iframe_document(srcdoc: &str) -> Document {
    lumen_html_parser::parse(srcdoc)
}

/// CSS Backgrounds L3 §2.11.2 — «The Canvas Background and the Root Element»:
/// если у root-элемента (`<html>`) нет собственного фона
/// (`background-color: transparent` И `background-image: none`), фон
/// `<body>` пропагируется на root box, а у `<body>` обнуляется. Это
/// покрывает legacy-страницы `body { background: red }`, где иначе фон
/// рисуется только в пределах body box-а и не достигает viewport-а
/// сверху / снизу.
///
/// Phase 0: переносим только два longhand-а — `background-color` и
/// `background-image`. Остальные `background-*` longhand-ы у body без
/// image не имеют визуального эффекта и сейчас не propagated; при
/// добавлении реального paint pattern fill-а их тоже нужно будет
/// перенести.
///
/// Structure: `doc.root()` — Document-узел; его ребёнок — `<html>`
/// element. Body — прямой ребёнок `<html>`. SVG / MathML root-ы пока не
/// учитываются (spec упоминает их отдельно).
fn propagate_canvas_background(doc: &Document, root: &mut LayoutBox) {
    let html_idx = root
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "html"));
    let Some(html_idx) = html_idx else {
        return;
    };

    let html_box = &mut root.children[html_idx];
    let html_has_bg = html_box.style.background_color.is_some()
        || !html_box.style.background_layers.is_empty();
    if html_has_bg {
        return;
    }

    let body_idx = html_box
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "body"));
    let Some(body_idx) = body_idx else {
        return;
    };

    let body = &mut html_box.children[body_idx];
    let body_has_bg = body.style.background_color.is_some()
        || !body.style.background_layers.is_empty();
    if !body_has_bg {
        return;
    }

    let body_style = Arc::make_mut(&mut body.style);
    let bg_color = body_style.background_color.take();
    let bg_layers = std::mem::take(&mut body_style.background_layers);
    let html_style = Arc::make_mut(&mut html_box.style);
    html_style.background_color = bg_color;
    html_style.background_layers = bg_layers;
}

/// CSS Backgrounds §3.11.1 — the canvas background color.
///
/// Returns the opaque background color of the root element box (the color
/// `propagate_canvas_background` moved onto `<html>`, originally the root's or
/// `<body>`'s background). The renderer clears the **entire** surface to this
/// color so the page background covers the whole viewport even when the root
/// element's box is shorter or narrower than the window — e.g. a fixed 1024×720
/// page in a maximized window, where painting only the root box's rect would
/// leave the rest of the canvas the UA-default white (and the root's own
/// `background-color` shows only as a band the size of the box, not the canvas).
///
/// Returns `None` (→ UA-default white clear) when the root element has no
/// background color or the color is not fully opaque: a translucent root
/// background must composite over the UA canvas, which the root box's own
/// background `FillRect` already handles within its rect.
pub fn canvas_background_color(root: &LayoutBox) -> Option<crate::style::Color> {
    let html = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block | BoxKind::FlowRoot))?;
    let color = html.style.background_color?.to_color_opt()?;
    (color.a == 255).then_some(color)
}

fn is_html_element_named(doc: &Document, id: NodeId, want: &str) -> bool {
    matches!(
        doc.get(id).element_name(),
        Some(q) if q.local.eq_ignore_ascii_case(want)
    )
}

/// Является ли DOM-узел inline-контентом (non-whitespace текст или inline-элемент).
///
/// True for Unicode control characters (Cc: C0, DEL, C1) that browsers render as
/// invisible zero-advance — EXCEPT tab/LF/CR, which carry white-space semantics
/// (CSS Text L3 §4.1). Such characters are stripped at the inline-item level so a
/// stray control byte never produces a visible line box (BUG-120: Edge renders
/// U+0001 invisible, Lumen drew a 19.2px text line shifting content below).
fn is_invisible_control(c: char) -> bool {
    c.is_control() && c != '\t' && c != '\n' && c != '\r'
}

/// Removes invisible control characters (see [`is_invisible_control`]) from `s`.
/// Borrows the input unchanged when no such characters are present (common case).
fn strip_invisible_controls(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains(is_invisible_control) {
        std::borrow::Cow::Owned(s.chars().filter(|&c| !is_invisible_control(c)).collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// The computed `display` of `id`, as the box-build stage will see it.
///
/// BUG-341 S25. Every caller of this asks the same question — *which formatting
/// context does this child join* — about a node whose style
/// [`precompute_counters`] has already cascaded against this very `inherited`,
/// and which `build_box_inner` will build out of that cached entry regardless
/// of what a probe says. Re-running `compute_style` here therefore did not just
/// cost a second cascade per element child (14 of them on a chrome keystroke,
/// 0.21-0.25 ms of a 0.63 ms cycle): it let the probe and the box disagree.
/// Reading the cache makes the two answers the same one by construction.
///
/// The `compute_style` fallback stays for the genuine misses — a full pass over
/// a node the cascade did not visit, and any caller holding a `CounterMap` that
/// predates the node. It is not a performance path: on chrome it never fires
/// for an element.
fn probe_display(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> Display {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.display_probes += 1;
        s.set(v);
    });
    match counters.style_arc(id) {
        Some(s) => s.display,
        None => {
            note_display_probe(|| compute_style(doc, id, sheet, inherited, viewport, dark_mode))
                .display
        }
    }
}

/// `display` плюс признак «бокс выведен из inline-потока»: CSS 2.1 §9.7 делает
/// плавающий и абсолютно позиционированный бокс блочным независимо от
/// объявленного `display`.
///
/// Отдельная функция, а не второй вызов [`probe_display`]: оба поля читаются из
/// одного и того же `ComputedStyle`, и повторный проход по каскаду на промахе
/// кэша стоил бы ровно столько же, сколько первый.
#[allow(clippy::too_many_arguments)]
fn probe_display_and_flow(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> (Display, bool) {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.display_probes += 1;
        s.set(v);
    });
    let read = |s: &ComputedStyle| {
        (
            s.display,
            s.float_side != FloatSide::None
                || matches!(s.position, Position::Absolute | Position::Fixed),
        )
    };
    match counters.style_arc(id) {
        Some(s) => read(&s),
        None => read(&note_display_probe(|| {
            compute_style(doc, id, sheet, inherited, viewport, dark_mode)
        })),
    }
}

/// Порождает ли элемент содержимое, которое можно уплощить в `InlineSegment`-ы.
///
/// `<img>` и form controls исключены: это replaced-элементы, у них есть
/// собственная высота, которой у сегмента нет — как сегмент такой элемент
/// схлопывается в высоту строки ([BUG-728]). Они получают собственный бокс
/// (`BoxKind::Image` / `BoxKind::FormControl`), как и всё блочно-уровневое.
///
/// `inline-flex` / `inline-grid` тоже исключены ([BUG-739]): по CSS Display L3
/// §2.1 это **atomic inline-level** боксы — снаружи inline, внутри собственный
/// flex/grid formatting context. Сегмент такого контекста не несёт, поэтому
/// уплощение стоило элементу бокса целиком: ни фона, ни рамки, ни размеров,
/// flex/grid-алгоритм не запускался. Их место — рядом с `inline-block`, в
/// [`is_atomic_inline_level`].
///
/// `display` передаётся отдельно, чтобы вызывающий не считал стиль дважды:
/// [`collect_inline_segments`] к этому месту уже имеет вычисленный
/// `ComputedStyle` узла, а [`is_inline_content`] берёт `display` из кэша.
fn produces_inline_segments(doc: &Document, id: NodeId, display: Display) -> bool {
    if is_image_element(doc, id) || is_form_control_element(doc, id) {
        return false;
    }
    display == Display::Inline
}

/// То же для потомка inline-элемента: `display: contents` дополнительно
/// прозрачен — бокса он не порождает вовсе (CSS Display L3 §3.1), его дети
/// участвуют в inline-контексте родителя напрямую, поэтому уплощать надо
/// сквозь него. Собственный бокс достанется уже его не-inline потомкам.
///
/// На уровне сиблингов блочного контейнера `contents` этой поблажки не имеет
/// ([`is_inline_content`]) — там он и до [BUG-728] получал отдельный бокс.
fn produces_inline_segments_nested(doc: &Document, id: NodeId, display: Display) -> bool {
    if display == Display::Contents {
        // На replaced-элементе `contents` вычисляется в `inline` (§3.1), то
        // есть бокс у него остаётся — и высота, ради которой всё это.
        return !is_image_element(doc, id) && !is_form_control_element(doc, id);
    }
    produces_inline_segments(doc, id, display)
}

/// `<img>` — не inline-**контент**, хотя и inline-уровневый: он порождает
/// собственный `BoxKind::Image` вместо того, чтобы влиться в `InlineRun`
/// сегментом (у сегмента нет своей высоты — BUG-728). В строку он попадает
/// через [`is_atomic_inline_level`], как `inline-block` и form controls.
#[allow(clippy::too_many_arguments)]
fn is_inline_content(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> bool {
    match &doc.get(id).data {
        // Control-only text (after BUG-120 stripping) is no more inline content
        // than whitespace-only text: it must not open an inline run / line box.
        NodeData::Text(s) => !s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)),
        NodeData::Element { .. } => {
            if is_image_element(doc, id) || is_form_control_element(doc, id) {
                return false;
            }
            produces_inline_segments(
                doc,
                id,
                probe_display(doc, sheet, id, inherited, viewport, dark_mode, counters),
            )
        }
        _ => false,
    }
}

/// Является ли DOM-узел **atomic inline-level** элементом — таким, который
/// снаружи участвует в inline-контексте одним неделимым боксом, а внутри
/// заводит собственный formatting context (CSS Display L3 §2.1):
/// `inline-block`, `inline-flex`, `inline-grid`.
///
/// Все три собираются в `InlineBlockRow` и текут горизонтально рядом с текстом;
/// различает их только внутренний лэйаут, который выбирает `lay_out` по
/// `style.display` (ветки `Display::Flex | Display::InlineFlex` и
/// `Display::Grid | Display::InlineGrid`). До [BUG-739] `inline-flex`/
/// `inline-grid` не попадали сюда и уплощались в сегменты родителя, то есть
/// не получали бокса вовсе.
///
/// Form controls (`<input>`/`<select>`/`<button>`/…) участвуют как inline-block,
/// когда их computed `display` == InlineBlock (UA-дефолт из `default_display`):
/// их replaced/виджет-бокс (`BoxKind::FormControl`) собирается в
/// `InlineBlockRow` и течёт горизонтально рядом с текстом и соседними
/// контролами. Author `display:block` поверх → обычный block-бокс (эта функция
/// вернёт false).
///
/// `<img>` (IFC-2) — четвёртый случай: у него UA-дефолт `display: inline`, но
/// как replaced-элемент он неделим, поэтому inline-level он именно **atomic**
/// (CSS Display L3 §2.1), а не источник сегментов ([`produces_inline_segments`]
/// возвращает для него false). Поэтому у картинки принимается и `Inline`.
///
/// Плавающая или абсолютно позиционированная картинка сюда НЕ попадает: CSS 2.1
/// §9.7 выводит такой бокс из inline-потока и делает блочным независимо от
/// `display`, а обтекание умеет только блочная ветка `lay_out`. До IFC-2
/// `<img>` был блочным всегда, поэтому обтекание у него работало — сузить его
/// молча значило бы разменять одну раскладку на другую. Тот же случай у
/// плавающего `inline-block` разбирается по-старому (он и до IFC-2 собирался в
/// ряд, теряя float) — это отдельный дефект, здесь не трогается.
#[allow(clippy::too_many_arguments)]
fn is_atomic_inline_level(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> bool {
    if !matches!(&doc.get(id).data, NodeData::Element { .. }) {
        return false;
    }
    if is_image_element(doc, id) {
        let (display, out_of_flow) =
            probe_display_and_flow(doc, sheet, id, inherited, viewport, dark_mode, counters);
        return !out_of_flow
            && matches!(
                display,
                Display::Inline
                    | Display::InlineBlock
                    | Display::InlineFlex
                    | Display::InlineGrid
            );
    }
    matches!(
        probe_display(doc, sheet, id, inherited, viewport, dark_mode, counters),
        Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
    )
}

/// Обнуляет box-model spacing анонимного контейнера (InlineRun / InlineBlockRow).
// Anonymous boxes inherit only inheritable properties from their parent; every
// non-inherited property takes its initial value (CSS 2.1 §9.2.2.1). Cloning the
// parent style and resetting the non-inherited longhands below approximates that.
// `float`, `clear` and `position` are non-inherited (CSS 2.1 §9.5.1/§9.5.2, CSS
// Positioned Layout L3 §3): an anonymous box must NOT float or be positioned
// (BUG-152 — an anonymous InlineRun cloning a floated parent's `float_side`
// re-entered the float branch of its own parent's layout loop, so `child_y` never
// advanced and the run overlapped the following block siblings).
fn anon_style(parent: &ComputedStyle) -> ComputedStyle {
    let mut s = parent.clone();
    s.float_side = FloatSide::None;
    s.clear = ClearSide::None;
    s.position = Position::Static;
    s.margin_top = LengthOrAuto::ZERO;
    s.margin_right = LengthOrAuto::ZERO;
    s.margin_bottom = LengthOrAuto::ZERO;
    s.margin_left = LengthOrAuto::ZERO;
    s.padding_top = Length::Px(0.0);
    s.padding_right = Length::Px(0.0);
    s.padding_bottom = Length::Px(0.0);
    s.padding_left = Length::Px(0.0);
    s.background_color = None;
    s.width = None;
    s.height = None;
    s.min_width = None;
    s.max_width = None;
    s.min_height = None;
    s.max_height = None;
    s.border_top_width = 0.0;
    s.border_right_width = 0.0;
    s.border_bottom_width = 0.0;
    s.border_left_width = 0.0;
    s.box_sizing = BoxSizing::ContentBox;
    s
}

/// `role` disambiguates the many different reasons callers wrap segments in an
/// anonymous inline run — a blockified flex/grid text item, a whitespace-flush
/// gap, or `::before`/`::after` generated content — per ADR-025 §1.
fn anon_inline_run(
    node: NodeId,
    parent: &ComputedStyle,
    segs: Vec<InlineSegment>,
    role: BoxRole,
) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: Arc::new(anon_style(parent)),
        kind: BoxKind::InlineRun { segments: segs, lines: vec![], first_line_style: None },
        children: vec![],
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(node), role },
    }
}

/// CSS Flexbox §4 / Grid §6: a contiguous run of text directly inside a flex or
/// grid container is wrapped in an anonymous (blockified) item. Returns `None`
/// for a whitespace/control-only run — such runs do not generate an item.
///
/// The item is an anonymous `Block` container (so its inline content formats into
/// line boxes like any block) holding a single `InlineRun` with the text. Without
/// this, the text node's box is `Skip` and the text vanishes — BUG-194: white
/// digit labels inside `.item { display: flex }` were dropped entirely.
#[allow(clippy::too_many_arguments)]
fn build_anon_text_item(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    parent: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
) -> Option<LayoutBox> {
    let NodeData::Text(s) = &doc.get(id).data else {
        return None;
    };
    if s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)) {
        return None;
    }
    let mut segs = Vec::new();
    // Each anonymous text item is its own inline context — ::first-letter does not
    // apply to anonymous flex/grid items, so disable the candidate flag.
    let mut need_first_letter = false;
    // `id` — текстовый узел, у него нет потомков: escape-ов быть не может.
    let mut escapes = Vec::new();
    collect_inline_segments(
        doc, sheet, id, parent, viewport, &mut segs, &mut escapes, flat, counters, registry,
        &mut need_first_letter, dark_mode,
    );
    if segs.is_empty() {
        return None;
    }
    let run = anon_inline_run(id, parent, segs, BoxRole::AnonymousInlineRun);
    let mut item_style = anon_style(parent);
    // The anonymous item is blockified regardless of the container's own display.
    item_style.display = Display::Block;
    Some(LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style: Arc::new(item_style),
        kind: BoxKind::Block,
        children: vec![run],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin { node: Some(id), role: BoxRole::AnonymousBlock },
    })
}

/// CSS Pseudo-elements L4 §5.4: applies `::first-letter` style to the first grapheme of the
/// `FirstLetter`-marked segment. Splits the segment if it contains more than one character so
/// only the first grapheme gets the pseudo-element style; the remainder keeps the original style.
/// No-op when no `FirstLetter` segment exists or no matching `::first-letter` rule is found.
fn apply_first_letter_pseudo(
    segs: &mut Vec<InlineSegment>,
    doc: &lumen_dom::Document,
    node: lumen_dom::NodeId,
    sheet: &lumen_css_parser::Stylesheet,
    parent: &crate::style::ComputedStyle,
    viewport: lumen_core::geom::Size,
    dark_mode: bool,
) {
    let Some(pos) = segs.iter().position(|s| s.pseudo_kind == PseudoKind::FirstLetter) else {
        return;
    };
    let Some(fl_style) = crate::style::compute_pseudo_element_style(
        doc, node, "first-letter", sheet, parent, viewport, dark_mode,
    ) else {
        return;
    };
    // CSS Pseudo-elements L4 §5.1: leading punctuation + first letter. Char-level
    // boundary (full grapheme cluster support requires unicode-segmentation,
    // which is not yet a dependency).
    let first_char_end = first_letter_text_len(&segs[pos].text);
    if first_char_end == 0 {
        return;
    }
    if first_char_end >= segs[pos].text.len() {
        // Single-character segment: layer the pseudo style on in place.
        segs[pos].style = crate::style::merge_pseudo_inherited(&segs[pos].style, parent, &fl_style);
        return;
    }
    // Multi-character: split into [first_char | rest], each with its own style.
    let rest_text = segs[pos].text[first_char_end..].to_string();
    let original_style = segs[pos].style.clone();
    let source_node = segs[pos].source_node;
    let post_space = segs[pos].post_space;
    segs[pos].text.truncate(first_char_end);
    segs[pos].style = crate::style::merge_pseudo_inherited(&original_style, parent, &fl_style);
    segs[pos].post_space = 0.0;
    segs.insert(pos + 1, InlineSegment {
        text: rest_text,
        style: original_style,
        pre_space: 0.0,
        post_space,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node,
        source_char_offset: first_char_end as u32,
        bidi_level: 0,
    });
}

/// Собирает поток inline-контента блочного контейнера в элементы будущего ряда,
/// разрезая его на местах [`InlineEscape`] (CSS 2.1 §9.2.1.1, [BUG-728]).
///
/// Сегменты между двумя escape-ами становятся отдельным `InlineRun`, каждый
/// escape — собственным боксом ровно на своём месте потока. `::first-letter`
/// применяется к каждому куску отдельно: маркер `PseudoKind::FirstLetter` стоит
/// ровно на одном сегменте, поэтому для остальных кусков это no-op — так
/// индексы escape-ов не сбиваются вставкой сегмента-остатка.
#[allow(clippy::too_many_arguments)]
fn split_inline_pieces(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    style: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev_index: Option<&crate::incremental::ReuseIndex>,
    segs: Vec<InlineSegment>,
    escapes: Vec<InlineEscape>,
    out_items: &mut Vec<LayoutBox>,
) {
    let push_run = |chunk: Vec<InlineSegment>, out_items: &mut Vec<LayoutBox>| {
        if chunk.is_empty() {
            return;
        }
        let mut chunk = chunk;
        apply_first_letter_pseudo(&mut chunk, doc, id, sheet, style, viewport, dark_mode);
        out_items.push(anon_inline_run(id, style, chunk, BoxRole::AnonymousInlineRun));
    };
    let mut rest = segs;
    // Escape-ы приходят в порядке обхода, их `at` не убывает; идём с конца,
    // чтобы отрезать хвост `split_off`-ом без сдвигов уже отданных индексов.
    let mut tails: Vec<(Vec<InlineSegment>, NodeId, ComputedStyle)> = Vec::new();
    for esc in escapes.into_iter().rev() {
        let at = esc.at.min(rest.len());
        tails.push((rest.split_off(at), esc.node, esc.inherited));
    }
    push_run(std::mem::take(&mut rest), out_items);
    for (tail, node, inherited) in tails.into_iter().rev() {
        let child = build_box_or_reuse(
            doc, sheet, node, &inherited, viewport, flat, counters, registry, dark_mode, prev_index,
        );
        if !matches!(child.kind, BoxKind::Skip) {
            out_items.push(child);
        }
        push_run(tail, out_items);
    }
}

/// CSS Pseudo-elements L4 §5.3: `::first-line` относится к первой строке блока,
/// то есть к первому `InlineRun` его inline-контекста. Один сброс потока может
/// дать несколько прогонов (разрезы по [`InlineEscape`]), поэтому стиль ищет
/// первый подходящий бокс среди только что добавленных и взводит `assigned`,
/// чтобы следующие сбросы его не перетёрли.
fn assign_first_line_style(
    fresh: &mut [LayoutBox],
    first_line_style: &Option<Box<ComputedStyle>>,
    assigned: &mut bool,
) {
    if *assigned {
        return;
    }
    for item in fresh {
        if let BoxKind::InlineRun { first_line_style: ref mut fls, .. } = item.kind {
            *fls = first_line_style.clone();
            *assigned = true;
            return;
        }
    }
}

/// Ширина схлопнутого пробела, которым текстовый прогон граничит с соседом по
/// строке. Повторяет выбор шрифта из [`wrap_inline_run`]: кегль — контейнера,
/// семейство — первого сегмента, иначе прогон и зазор перед ним меряются
/// разными шрифтами (BUG-128).
fn inline_space_width(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> f32 {
    let (Some(m), BoxKind::InlineRun { segments, .. }) = (measurer, &b.kind) else {
        return 0.0;
    };
    let em = b.style.font_size;
    segments.first().map_or_else(
        || m.char_width(' ', em),
        |seg| m.char_width_with_families(' ', em, &seg.style.font_family),
    )
}

/// CSS Text L3 §4.1.1 — схлопнутый пробел, с которого текстовый прогон
/// начинается: `wrap_inline_run` срезает пробел в начале строки, поэтому зазор
/// между предшествующим atomic inline и текстом не записан больше нигде.
///
/// Считается по сегментам, а не по строкам: значение нужно ДО раскладки
/// прогона, чтобы знать, с какого x его класть.
fn inline_run_lead_space(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> f32 {
    let BoxKind::InlineRun { segments, .. } = &b.kind else {
        return 0.0;
    };
    if b.style.white_space.preserves_whitespace() {
        // Пробел сохранён в самом сегменте — он уже внутри фрагментов.
        return 0.0;
    }
    let starts_ws = segments
        .iter()
        .find(|seg| !seg.text.is_empty())
        .is_some_and(|seg| seg.text.starts_with(|c: char| c.is_whitespace()));
    if starts_ws { inline_space_width(b, measurer) } else { 0.0 }
}

/// Насколько текстовый прогон продвигает inline formatting context — ширина его
/// ПОСЛЕДНЕЙ строки плюс схлопнутый пробел, которым он заканчивается.
///
/// Бокс прогона широк ровно настолько, сколько ему предложили, а не настолько,
/// сколько занял текст, поэтому продвигаться по `rect.width` нельзя: следующий
/// atomic inline всегда оказывался бы за правым краем контейнера и переносился
/// на свою строку (IFC-1 — «Aa <span inline-block> Bb» раскладывался тремя
/// строками вместо одной). Важна только последняя строка: все предыдущие
/// закончились мягким переносом, и контент после прогона продолжает именно её.
fn inline_run_advance(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> f32 {
    let BoxKind::InlineRun { segments, lines, .. } = &b.kind else {
        return b.rect.width;
    };
    let Some(last) = lines.last() else {
        // Раскладки не было (нет measurer) — прежнее поведение.
        return b.rect.width;
    };
    let extent = last
        .iter()
        .map(|f| f.x + f.width)
        .fold(0.0_f32, f32::max);
    let trail = if b.style.white_space.preserves_whitespace() {
        0.0
    } else {
        let ends_ws = segments
            .iter()
            .rev()
            .find(|seg| !seg.text.is_empty())
            .is_some_and(|seg| seg.text.ends_with(|c: char| c.is_whitespace()));
        if ends_ws { inline_space_width(b, measurer) } else { 0.0 }
    };
    extent + trail
}

/// CSS 2.1 §10.8.1 — расстояние от верхней кромки border box до базовой линии,
/// которую бокс предлагает своему inline formatting context. `None` означает,
/// что такой линии нет и выравнивать бокс надо по нижней кромке margin box:
/// замещаемый элемент, пустой `inline-block` или `inline-block` с `overflow`,
/// отличным от `visible`.
fn inline_baseline(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    match &b.kind {
        BoxKind::InlineRun { .. } => {
            let m = measurer?;
            let em = b.style.font_size;
            let line_h = step_line_height(em * b.style.line_height, b.style.line_height_step);
            // Базовая линия — у ПОСЛЕДНЕЙ строки прогона: именно её продолжает
            // контент, стоящий за прогоном. Отсчитывается от высоты бокса, а не
            // как `(n-1) * line_h`, чтобы не разойтись с `::first-line` и
            // `line-height-step`, которые делают строки разновысокими.
            let half_leading = (line_h - (m.ascent_px(em) + m.descent_px(em))) / 2.0;
            Some(b.rect.height - line_h + half_leading + m.ascent_px(em))
        }
        // Базовая линия замещаемого элемента — нижняя кромка margin box.
        BoxKind::Image { .. }
        | BoxKind::Video { .. }
        | BoxKind::Canvas { .. }
        | BoxKind::Audio { .. }
        | BoxKind::Iframe { .. }
        | BoxKind::SvgRoot { .. } => None,
        _ => {
            if b.style.overflow_x != Overflow::Visible || b.style.overflow_y != Overflow::Visible {
                return None;
            }
            if let BoxKind::FormControl { kind } = &b.kind
                && !form_control_has_text_baseline(kind)
            {
                return None;
            }
            if let Some(bl) = last_in_flow_baseline(b, measurer) {
                return Some(bl);
            }
            // HTML §15.5 — у текстового контрола line box своего потока нет
            // (значение рисует виджет), но базовая линия у него есть, и браузеры
            // берут её по тексту, а не по нижней кромке. Строка ставится по
            // центру content box — ровно так, как её рисует
            // `emit_input_value_text`, иначе раскладка и отрисовка разъедутся.
            if matches!(b.kind, BoxKind::FormControl { .. }) {
                let m = measurer?;
                let s = &b.style;
                let em = s.font_size;
                let pt = s.padding_top.resolve_or_zero(em, 0.0, Size::ZERO);
                let pb = s.padding_bottom.resolve_or_zero(em, 0.0, Size::ZERO);
                let inner_h = (b.rect.height
                    - s.border_top_width
                    - s.border_bottom_width
                    - pt
                    - pb)
                    .max(0.0);
                let line_h = step_line_height(em * s.line_height, s.line_height_step);
                let half_leading = (line_h - (m.ascent_px(em) + m.descent_px(em))) / 2.0;
                return Some(
                    s.border_top_width
                        + pt
                        + ((inner_h - line_h) / 2.0).max(0.0)
                        + half_leading
                        + m.ascent_px(em),
                );
            }
            None
        }
    }
}

/// Несёт ли контрол текст, по которому браузер берёт его базовую линию.
///
/// `checkbox`/`radio`/`color`/`file`/`range`/`progress`/`meter` — замещаемые
/// виджеты без текста: их базовая линия — нижняя кромка margin box (CSS 2.1
/// §10.8.1), и синтезировать текстовую линию для них значит поднять контрол над
/// строкой. `<textarea>` тоже выравнивается по нижней кромке (проверено против
/// Edge на TEST-34: `<select>` рядом с ним садится НИЖЕ его нижнего края —
/// значит базовая линия строки идёт по textarea, а не по его первой строке).
fn form_control_has_text_baseline(kind: &FormControlKind) -> bool {
    match kind {
        FormControlKind::Button | FormControlKind::Select { .. } => true,
        FormControlKind::Input { input_type, .. } => matches!(
            input_type,
            lumen_dom::InputType::Text
                | lumen_dom::InputType::Password
                | lumen_dom::InputType::Email
                | lumen_dom::InputType::Tel
                | lumen_dom::InputType::Url
                | lumen_dom::InputType::Number
                | lumen_dom::InputType::Search
                | lumen_dom::InputType::Date
                | lumen_dom::InputType::DateTimeLocal
                | lumen_dom::InputType::Time
                | lumen_dom::InputType::Month
                | lumen_dom::InputType::Week
                | lumen_dom::InputType::Submit
                | lumen_dom::InputType::Reset
                | lumen_dom::InputType::Button
        ),
        FormControlKind::Textarea { .. }
        | FormControlKind::Range { .. }
        | FormControlKind::Progress { .. }
        | FormControlKind::Meter { .. } => false,
    }
}

/// Базовая линия последнего потомка `b`, находящегося в нормальном потоке
/// (CSS 2.1 §10.8.1 — «базовая линия последнего line box в нормальном потоке»),
/// в координатах border box самого `b`.
fn last_in_flow_baseline(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    for c in b.children.iter().rev() {
        if c.style.float_side != FloatSide::None
            || matches!(c.style.position, Position::Absolute | Position::Fixed)
        {
            continue;
        }
        if matches!(
            c.kind,
            BoxKind::Skip | BoxKind::InlineSpace | BoxKind::Marker { .. }
        ) {
            continue;
        }
        if let Some(bl) = inline_baseline(c, measurer) {
            return Some(c.rect.y - b.rect.y + bl);
        }
    }
    None
}

/// `vertical-align` бокса как участника inline-ряда. Анонимный прогон текста
/// всегда выравнивается по базовой линии: свойство не наследуется, а `anon_style`
/// клонирует стиль блока-родителя целиком.
fn inline_v_align(b: &LayoutBox) -> VerticalAlign {
    if matches!(b.kind, BoxKind::InlineRun { .. }) {
        VerticalAlign::Baseline
    } else {
        b.style.vertical_align
    }
}

/// Разрывает ли бокс анонимный inline-ряд: блочно-уровневый потомок, всплывший
/// из inline-элемента, не может делить line box с текстом (CSS 2.1 §9.2.1.1).
/// Анонимные прогоны и пробелы (`BoxRole::AnonymousInlineRun`) наследуют
/// `display` блока-родителя, поэтому по стилю их отличить нельзя — только по роли.
fn breaks_inline_row(b: &LayoutBox) -> bool {
    !matches!(b.origin.role, BoxRole::AnonymousInlineRun)
        && !matches!(
            b.style.display,
            Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        )
}

fn anon_inline_block_row(node: NodeId, parent: &ComputedStyle, items: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: Arc::new(anon_style(parent)),
        kind: BoxKind::InlineBlockRow,
        children: items,
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(node), role: BoxRole::AnonymousInlineRun },
    }
}

/// Inline-сегменты для текста, у которого нет DOM-узла-источника: значение
/// `<textarea>`, набранное пользователем или присвоенное скриптом (BUG-441).
///
/// Повторяет ветки [`collect_inline_segments`] для текстового узла, но берёт
/// строку не из DOM: при `white-space`, сохраняющем переводы строки (UA-стиль
/// `<textarea>` — `pre-wrap`), строка режется по `\n` на сегменты с
/// `forced_break` между ними; иначе отдаётся одним сегментом. `source_node` —
/// сам контрол: собственного текстового узла у значения нет.
fn control_value_segments(
    node: NodeId,
    value_text: &str,
    style: &ComputedStyle,
) -> Vec<InlineSegment> {
    let mut out = Vec::new();
    let mut push = |text: String, forced_break: bool, byte_offset: u32| {
        out.push(InlineSegment {
            text,
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            img_width: 0.0,
            forced_break,
            pseudo_kind: PseudoKind::None,
            source_node: node,
            source_char_offset: byte_offset,
            bidi_level: 0,
        });
    };
    if !style.white_space.preserves_newlines() {
        let text = style.text_transform.apply(&strip_invisible_controls(value_text));
        if !text.is_empty() {
            push(text, false, 0);
        }
        return out;
    }
    let mut byte_offset: u32 = 0;
    for (i, line) in value_text.split('\n').enumerate() {
        if i > 0 {
            push(String::new(), true, byte_offset);
            byte_offset += 1; // the \n character
        }
        // BUG-120: invisible controls must not occupy advance width.
        let text = style.text_transform.apply(&strip_invisible_controls(line));
        if !text.is_empty() {
            push(text, false, byte_offset);
        }
        byte_offset += line.len() as u32;
    }
    out
}

/// Потомок inline-элемента, который нельзя уплотнить в [`InlineSegment`].
///
/// CSS 2.1 §9.2.1.1: блочно-уровневый потомок разрезает окружающий inline-бокс,
/// а replaced-элемент (`<img>`, form control) обязан сохранить собственную
/// высоту. У сегмента высоты нет вовсе — до [BUG-728] такой потомок уплощался
/// вместе с текстом и схлопывался в высоту строки. Вместо этого
/// [`collect_inline_segments`] откладывает узел сюда, а строитель блочного
/// контейнера собирает ему настоящий бокс и вставляет на то же место потока.
#[derive(Debug, Clone)]
struct InlineEscape {
    /// Сколько сегментов уже собрано к моменту встречи узла: бокс встаёт
    /// ровно после них и перед всеми последующими.
    at: usize,
    /// DOM-узел, которому нужен собственный `LayoutBox`.
    node: NodeId,
    /// Стиль родительского inline-элемента — то, от чего узел наследует.
    /// Блочный контейнер строит бокс далеко от места находки, и его
    /// собственный стиль здесь не подходит: цвет/шрифт `<span>`-а между ними
    /// был бы потерян.
    inherited: ComputedStyle,
}

/// Рекурсивно собирает `InlineSegment`-ы из поддерева inline-контента.
///
/// `need_first_letter` — starts `true` for the first call on a block container; set to `false`
/// once the first non-whitespace text character is split into a `PseudoKind::FirstLetter` segment.
/// Callers must initialize to `true` and pass through all recursive calls within the same run.
/// After collection, `apply_first_letter_pseudo` overrides the `PseudoKind::FirstLetter`
/// segment's style via `compute_pseudo_element_style(node, "first-letter")`.
///
/// `escapes` собирает узлы, которым нужен собственный бокс — см. [`InlineEscape`].
#[allow(clippy::too_many_arguments)]
fn collect_inline_segments(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    out: &mut Vec<InlineSegment>,
    escapes: &mut Vec<InlineEscape>,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    need_first_letter: &mut bool,
    dark_mode: bool,
) {
    match &doc.get(id).data {
        NodeData::Text(s) if inherited.white_space.preserves_whitespace() => {
            // CSS Text L3 §4.1: white-space: pre/pre-wrap — preserve tabs and
            // newlines. Split on \n to produce forced-break segments.
            let style = inherited.clone();
            let mut byte_offset: u32 = 0;
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    out.push(InlineSegment {
                        text: String::new(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: true,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                    byte_offset += 1; // the \n character
                }
                // BUG-120: drop invisible controls (Cc except tab) — they must
                // not occupy advance width even in white-space: pre.
                let text = strip_invisible_controls(line);
                if !text.is_empty() {
                    out.push(InlineSegment {
                        text: text.into_owned(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: false,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                }
                byte_offset += line.len() as u32;
            }
        }
        NodeData::Text(s)
            if inherited.white_space.preserves_newlines() && s.contains('\n') =>
        {
            // CSS Text L4 §3.1 preserve-breaks (white-space: pre-line):
            // segment breaks сохраняются как forced line breaks, остальной
            // whitespace схлопывается как в normal (word-split в
            // wrap_inline_run). Сюда попадает только PreLine — режимы с
            // preserves_whitespace() перехвачены веткой выше.
            let style = inherited.clone();
            let mut byte_offset: u32 = 0;
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    out.push(InlineSegment {
                        text: String::new(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: true,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                    byte_offset += 1; // the \n character
                }
                let stripped = strip_invisible_controls(line);
                if !stripped.chars().all(|c| c.is_whitespace()) {
                    let text = inherited.text_transform.apply(&stripped);
                    let kind = if *need_first_letter && !text.trim().is_empty() {
                        *need_first_letter = false;
                        PseudoKind::FirstLetter
                    } else {
                        PseudoKind::None
                    };
                    out.push(InlineSegment {
                        text,
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: false,
                        pseudo_kind: kind,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                }
                byte_offset += line.len() as u32;
            }
        }
        NodeData::Text(s) if !s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)) => {
            // BUG-120: strip invisible controls before transform/measure — Edge
            // renders them zero-advance, they must not contribute glyphs.
            let s = strip_invisible_controls(s);
            // text-transform применяется здесь, до wrapping и paint —
            // measurer считает ширину уже после преобразования.
            let text = inherited.text_transform.apply(&s);
            // CSS Pseudo-elements L4 §5.1: the first text segment in this inline run
            // is the candidate for ::first-letter. Mark the whole first non-whitespace
            // segment; `apply_first_letter_pseudo` later looks up the ::first-letter rule
            // and splits at the character boundary, restyling only the first letter.
            let kind = if *need_first_letter && !text.trim().is_empty() {
                *need_first_letter = false;
                PseudoKind::FirstLetter
            } else {
                PseudoKind::None
            };
            out.push(InlineSegment {
                text,
                style: inherited.clone(),
                pre_space: 0.0,
                post_space: 0.0,
                is_element_box: false,
                img_src: None,
                img_is_lazy: false,
                img_width: 0.0,
                forced_break: false,
                pseudo_kind: kind,
                source_node: id,
                source_char_offset: 0,
                bidi_level: 0,
            });
        }
        NodeData::Text(_) => {
            // CSS Text L3 §4.1.1 — a collapsing whitespace-only text node between
            // inline-level boxes collapses to a single space. We don't emit a
            // segment for it (it would split to zero words); instead we record the
            // collapsible space on the preceding segment by giving its text a
            // trailing space, so `wrap_inline_run` inserts exactly one inter-word
            // gap at that boundary. Without this, adjacent segments would be joined
            // tightly even when source whitespace separated them. Leading
            // whitespace (no preceding segment) collapses away entirely.
            if let Some(last) = out.last_mut()
                && !last.forced_break
                && !last.style.white_space.preserves_whitespace()
                && !last.text.ends_with(|c: char| c.is_whitespace())
            {
                last.text.push(' ');
            }
        }
        NodeData::Element { .. } => {
            let s = compute_style(doc, id, sheet, inherited, viewport, dark_mode);
            if s.display == Display::None {
                return;
            }
            // BUG-728: всё, что не порождает сегментов — блочно-уровневый
            // потомок (CSS 2.1 §9.2.1.1 разрезает вокруг него inline-бокс),
            // `<img>`, form control — уходит вызывающему за собственным боксом.
            // Уплощение в сегмент стоило бы такому потомку высоты: у сегмента
            // её нет, вертикальный размер строки считается по метрикам шрифта,
            // и `<img width=50 height=50>` внутри `<a>` рисовался 50×16.8.
            if !produces_inline_segments_nested(doc, id, s.display) {
                escapes.push(InlineEscape { at: out.len(), node: id, inherited: inherited.clone() });
                return;
            }
            // Compute horizontal inline box model: margin + border + padding.
            // Use em=font_size, cb=0 (% padding on inline elements is uncommon).
            let em = s.font_size;
            let pre = s.margin_left.resolve_or_zero(em, 0.0, viewport)
                + s.border_left_width
                + s.padding_left.resolve_or_zero(em, 0.0, viewport);
            let post = s.padding_right.resolve_or_zero(em, 0.0, viewport)
                + s.border_right_width
                + s.margin_right.resolve_or_zero(em, 0.0, viewport);
            let start = out.len();
            // CSS Pseudo-elements L4 §4 — ::before in inline formatting context.
            // Block pseudo-elements inside inline context are skipped (Phase 0).
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "before", sheet, &s, viewport, dark_mode)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, doc, id, QuoteSlot::Before, viewport, counters, registry, out);
            }
            let children: Vec<NodeId> = flat.children_of(doc, id).to_vec();
            for child_id in children {
                collect_inline_segments(doc, sheet, child_id, &s, viewport, out, escapes, flat, counters, registry, need_first_letter, dark_mode);
            }
            // CSS Pseudo-elements L4 §4 — ::after in inline formatting context.
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "after", sheet, &s, viewport, dark_mode)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, doc, id, QuoteSlot::After, viewport, counters, registry, out);
            }
            let added = out.len() - start;
            // Mark all segments from this element (including pseudo-element content)
            // as element boxes so the painter draws their background/border.
            for seg in &mut out[start..start + added] {
                seg.is_element_box = true;
            }
            if added > 0 && (pre > 0.0 || post > 0.0) {
                out[start].pre_space += pre;
                out[start + added - 1].post_space += post;
            }
        }
        _ => {}
    }
}

/// Injects a pseudo-element box (::before or ::after) into the children list.
///
/// `is_before = true` → prepend; `false` → append.
/// Inline pseudo-elements are merged into the adjacent InlineRun when possible.
/// Block pseudo-elements are inserted as separate Block boxes.
///
/// `blockify = true` forces every pseudo-element into its own block-level box,
/// regardless of its computed `display`. Used for flex/grid containers: CSS
/// Flexbox §4 / Grid §6 blockify all in-flow children (including generated
/// `::before`/`::after`) into individual items, so they must not be merged into
/// an adjacent InlineRun.
#[allow(clippy::too_many_arguments)]
fn inject_pseudo(
    parent_id: NodeId,
    children: &mut Vec<LayoutBox>,
    ps: Option<ComputedStyle>,
    is_before: bool,
    doc: &Document,
    viewport: Size,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    blockify: bool,
) {
    let Some(ps) = ps else { return };
    let slot = if is_before { QuoteSlot::Before } else { QuoteSlot::After };
    match ps.display {
        Display::Inline
        | Display::InlineFlex
        | Display::InlineGrid
        | Display::InlineBlock
            if !blockify =>
        {
            let segs = content_to_inline_segments(&ps, doc, parent_id, slot, viewport, counters, registry);
            if segs.is_empty() {
                return;
            }
            if is_before {
                match children.first_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        let mut new_segs = segs;
                        new_segs.extend(std::mem::take(segments));
                        *segments = new_segs;
                    }
                    _ => children.insert(
                        0,
                        anon_inline_run(parent_id, &ps, segs, BoxRole::Pseudo(PseudoKind::Before)),
                    ),
                }
            } else {
                match children.last_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        segments.extend(segs);
                    }
                    _ => children.push(anon_inline_run(
                        parent_id,
                        &ps,
                        segs,
                        BoxRole::Pseudo(PseudoKind::After),
                    )),
                }
            }
        }
        _ => {
            // Block-level pseudo-element.
            let pseudo_kind = if is_before { PseudoKind::Before } else { PseudoKind::After };
            let inner_segs = content_to_inline_segments(&ps, doc, parent_id, slot, viewport, counters, registry);
            let inner = if inner_segs.is_empty() {
                vec![]
            } else {
                vec![anon_inline_run(parent_id, &ps, inner_segs, BoxRole::Pseudo(pseudo_kind))]
            };
            let b = LayoutBox {
                node: parent_id,
                rect: Rect::ZERO,
                style: Arc::new(ps),
                kind: BoxKind::Block,
                children: inner,
                col_span: 1,
                row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(parent_id), role: BoxRole::Pseudo(pseudo_kind) },
            };
            if is_before {
                children.insert(0, b);
            } else {
                children.push(b);
            }
        }
    }
}

/// Extracts text from `Content::Items` and returns it as a single `InlineSegment`.
///
/// Resolves `ContentItem::String`, `ContentItem::Counter`, `ContentItem::Counters`,
/// `ContentItem::Attr` and `open-quote`/`close-quote` using the per-element
/// `CounterMap` snapshot and DOM lookup. `owner_id` is the element whose
/// `::before`/`::after` pseudo-element we're generating; `slot` selects which
/// precomputed quote-depth list to consume (CSS Generated Content L3 §3.2).
/// Custom `@counter-style` names are resolved via `registry`.
fn content_to_inline_segments(
    style: &ComputedStyle,
    doc: &Document,
    owner_id: NodeId,
    slot: QuoteSlot,
    viewport: Size,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) -> Vec<InlineSegment> {
    let Content::Items(items) = &style.content else {
        return vec![];
    };
    let snap = counters.counters(owner_id);
    let qdepths = counters.quote_depths(owner_id, slot);
    let mut qi = 0usize;
    let mut out: Vec<InlineSegment> = Vec::new();
    // Text-producing items concatenate into a single run; a `url()` item flushes
    // the pending run and emits its own inline-replaced image segment.
    let mut text = String::new();

    for item in items {
        // CSS Generated Content L3 §2.1 — a `url()` value is an inline-replaced
        // image. It interrupts the surrounding text run and becomes its own image
        // segment (mirrors the inline-`<img>` path in `collect_inline_segments`).
        if let ContentItem::Url(url) = item {
            if !text.is_empty() {
                out.push(make_content_text_segment(style, owner_id, std::mem::take(&mut text)));
            }
            if !url.is_empty() {
                let em = style.font_size;
                // No intrinsic size is known before the image is fetched, so honour
                // an explicit `width` and otherwise fall back to `2em` — the same
                // placeholder the inline-`<img>` path uses for undecoded images.
                let w = style
                    .width
                    .as_ref()
                    .and_then(|l| l.resolve(em, None, viewport))
                    .unwrap_or(em * 2.0);
                out.push(make_content_image_segment(style, url.clone(), w));
            }
            continue;
        }
        let piece = match item {
            ContentItem::String(s) => Some(s.clone()),
            ContentItem::Counter { name, style: list_style } => {
                let val = snap
                    .and_then(|s| s.get(name))
                    .and_then(|v| v.last())
                    .copied()
                    .unwrap_or(0);
                let sname = list_style.as_deref().unwrap_or("decimal");
                Some(format_counter_with_registry(val, sname, registry))
            }
            ContentItem::Counters { name, separator, style: list_style } => {
                let vals = snap
                    .and_then(|s| s.get(name))
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let sname = list_style.as_deref().unwrap_or("decimal");
                let formatted: Vec<String> = vals
                    .iter()
                    .map(|&v| format_counter_with_registry(v, sname, registry))
                    .collect();
                Some(formatted.join(separator.as_str()))
            }
            ContentItem::Attr(attr) => {
                doc.get(owner_id).get_attr(attr).map(|s| s.to_string())
            }
            // CSS Generated Content L3 §3.2 — open-quote / close-quote pick a
            // (open, close) pair from `quotes` at the precomputed nesting depth.
            ContentItem::OpenQuote => {
                let depth = qdepths.get(qi).copied().unwrap_or(0);
                qi += 1;
                style.quotes.pair_for_depth(depth).map(|(o, _)| o.to_string())
            }
            ContentItem::CloseQuote => {
                let depth = qdepths.get(qi).copied().unwrap_or(0);
                qi += 1;
                style.quotes.pair_for_depth(depth).map(|(_, c)| c.to_string())
            }
            // url() is handled above; no-open-quote / no-close-quote only advance
            // depth (handled in the precompute pass) and emit nothing.
            _ => None,
        };
        if let Some(piece) = piece {
            text.push_str(&piece);
        }
    }
    if !text.is_empty() {
        out.push(make_content_text_segment(style, owner_id, text));
    }
    out
}

/// Builds a plain-text `InlineSegment` for generated (`content`) text.
/// `source_node` is the owning element so Selection/Range can map back to it.
fn make_content_text_segment(
    style: &ComputedStyle,
    owner_id: NodeId,
    text: String,
) -> InlineSegment {
    InlineSegment {
        text,
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: owner_id,
        source_char_offset: 0,
        bidi_level: 0,
    }
}

/// Builds an inline-replaced image `InlineSegment` for a `content: url(...)` item.
/// `source_node` is `NodeId::from_index(0)` ("no DOM origin"): a generated image is
/// not a selectable text node, and `collect_background_image_requests` keys on this
/// sentinel to recognise generated-content images that still need fetching +
/// registering (real inline `<img>` frags carry their element's own `NodeId`).
fn make_content_image_segment(
    style: &ComputedStyle,
    url: String,
    width: f32,
) -> InlineSegment {
    InlineSegment {
        text: String::new(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: true,
        img_src: Some(url),
        img_is_lazy: false,
        img_width: width,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    }
}

/// Builds inline segments for a pseudo-element and applies its own box model
/// spacing (margin + border + padding) as `pre_space` / `post_space`.
/// Used by `collect_inline_segments` to inject `::before` / `::after` content.
#[allow(clippy::too_many_arguments)]
fn push_pseudo_inline_segs(
    ps: &ComputedStyle,
    doc: &Document,
    owner_id: NodeId,
    slot: QuoteSlot,
    viewport: Size,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    out: &mut Vec<InlineSegment>,
) {
    let mut segs = content_to_inline_segments(ps, doc, owner_id, slot, viewport, counters, registry);
    if segs.is_empty() {
        return;
    }
    let em = ps.font_size;
    let pre = ps.margin_left.resolve_or_zero(em, 0.0, viewport)
        + ps.border_left_width
        + ps.padding_left.resolve_or_zero(em, 0.0, viewport);
    let post = ps.padding_right.resolve_or_zero(em, 0.0, viewport)
        + ps.border_right_width
        + ps.margin_right.resolve_or_zero(em, 0.0, viewport);
    if pre > 0.0 {
        segs[0].pre_space += pre;
    }
    if post > 0.0 {
        let last = segs.len() - 1;
        segs[last].post_space += post;
    }
    out.extend(segs);
}

/// CSS Lists L3 §2.1 — ordinal of a `<li>` among its element siblings (1-based).
fn li_ordinal(doc: &Document, id: NodeId) -> u32 {
    let Some(parent_id) = doc.get(id).parent else { return 1 };
    let mut n = 0u32;
    for &sib in &doc.get(parent_id).children.clone() {
        if matches!(&doc.get(sib).data, NodeData::Element { name, .. } if name.local.as_str() == "li") {
            n += 1;
            if sib == id {
                return n;
            }
        }
    }
    1
}

/// CSS Lists L3 §2.1 — creates `BoxKind::Marker` and prepends to children.
/// Calls `compute_pseudo_element_style("marker")` so CSS `::marker` rules (color,
/// font, content) override the defaults. `content: none` on `::marker` suppresses
/// the marker entirely; `content: <string>` / `counter()` replaces the default text.
#[allow(clippy::too_many_arguments)]
fn inject_marker(
    parent_id: NodeId,
    children: &mut Vec<LayoutBox>,
    style: &ComputedStyle,
    ordinal: u32,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) {
    // CSS Lists L3 §2.3: an explicit `list-style-image` shows even when
    // `list-style-type: none` — the image takes precedence over the type, so a
    // marker is still generated. Only suppress when both are absent.
    if matches!(style.list_style_type, ListStyleType::None) && style.list_style_image.is_none() {
        return;
    }
    // CSS Pseudo-elements L4 §14.2 — compute ::marker style.
    // Returns None only when `content: none` is set, which suppresses the marker.
    let Some(mut ms) = compute_pseudo_element_style(
        doc, parent_id, "marker", sheet, style, viewport, dark_mode,
    ) else {
        return;
    };
    // CSS: list-style-image — P4 wires image markers.
    let text = match &ms.content {
        Content::Items(items) => marker_content_text(items, doc, parent_id, counters, registry),
        // CSS: list-style-type (custom counter-style) — build_list_marker_text consults registry.
        _ => build_list_marker_text(style.list_style_type.clone(), ordinal, registry),
    };
    ms.display = Display::Inline;
    children.insert(0, LayoutBox {
        node:     parent_id,
        rect:     Rect::ZERO,
        style:    Arc::new(ms),
        kind:     BoxKind::Marker {
            text,
            position:        style.list_style_position,
            list_style_type: style.list_style_type.clone(),
            image:           style.list_style_image.clone(),
        },
        children: vec![],
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(parent_id), role: BoxRole::ListMarker },
    });
}

/// Extracts a plain-text string from `::marker { content: <items> }`.
/// Supports String literals, `attr()`, `counter()`, `counters()`.
fn marker_content_text(
    items: &[ContentItem],
    doc: &Document,
    owner_id: NodeId,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) -> String {
    let snap = counters.counters(owner_id);
    items.iter().filter_map(|item| match item {
        ContentItem::String(s) => Some(s.clone()),
        ContentItem::Counter { name, style: list_style } => {
            let val = snap
                .and_then(|s| s.get(name))
                .and_then(|v| v.last())
                .copied()
                .unwrap_or(0);
            let sname = list_style.as_deref().unwrap_or("decimal");
            Some(format_counter_with_registry(val, sname, registry))
        }
        ContentItem::Counters { name, separator, style: list_style } => {
            let vals = snap
                .and_then(|s| s.get(name))
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let sname = list_style.as_deref().unwrap_or("decimal");
            let parts: Vec<String> = vals.iter()
                .map(|&v| format_counter_with_registry(v, sname, registry))
                .collect();
            Some(parts.join(separator.as_str()))
        }
        ContentItem::Attr(attr) => {
            doc.get(owner_id).get_attr(attr).map(str::to_string)
        }
        _ => None,
    }).collect()
}

/// CSS Display L3 §7.2 — replaces each `BoxKind::Contents` child with its own
/// children in-place. Grandchildren are already flattened (recursive `build_box`
/// calls run `flatten_contents` on inner levels first).
fn flatten_contents(children: &mut Vec<LayoutBox>) {
    let mut i = 0;
    while i < children.len() {
        if matches!(children[i].kind, BoxKind::Contents) {
            let grandchildren = std::mem::take(&mut children[i].children);
            let gc_len = grandchildren.len();
            children.remove(i);
            for (j, gc) in grandchildren.into_iter().enumerate() {
                children.insert(i + j, gc);
            }
            // Don't advance i — a grandchild might itself be Contents (edge case
            // if the inner build_box somehow produced an un-flattened Contents).
            // Advancing by gc_len skips them all safely since they were already
            // flattened at their own build level.
            i += gc_len;
        } else {
            i += 1;
        }
    }
}

/// True when `node` is a `<select>`/`<selectlist>` host that opts into the
/// HTML/CSS «Customizable Select» rendering (`appearance: base-select`).
fn is_base_select_host(doc: &Document, node: NodeId) -> bool {
    matches!(
        &doc.get(node).data,
        NodeData::Element { name, .. }
            if matches!(name.local.as_str(), "select" | "selectlist")
    )
}

/// Build the author-styleable box subtree for a `<select>`/`<selectlist>` with
/// `appearance: base-select` (HTML/CSS «Customizable Select»).
///
/// Structure (Phase 0 — closed state):
/// ```text
/// FlowRoot (the <select> box, styled by author rules on `select`)
/// └── Block  trigger button — holds the `<selectedcontent>` label text
/// ```
/// Unlike the opaque native `FormControlKind::Select`, this is a real box tree,
/// so author CSS on the `<select>` (and, later, on `option`/`::picker(select)`)
/// cascades into it. The pop-up option list (`::picker(select)`) is revealed by
/// the shell as a popover on click — see `forms.rs`.
#[allow(clippy::too_many_arguments)]
fn build_base_select_box(
    doc: &Document,
    style: &ComputedStyle,
    id: NodeId,
) -> LayoutBox {
    // The trigger button shows the currently-selected option's label, mirroring
    // the `<selectedcontent>` element of the Customizable Select spec.
    let label = if is_selectlist(doc, id) {
        collect_selectlist_label(doc, id)
    } else {
        collect_select_label(doc, id)
    };

    let mut trigger_children = Vec::new();
    if !label.is_empty() {
        let seg = InlineSegment {
            text: label,
            style: anon_style(style),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            img_width: 0.0,
            forced_break: false,
            pseudo_kind: PseudoKind::None,
            source_node: id,
            source_char_offset: 0,
            bidi_level: 0,
        };
        trigger_children.push(anon_inline_run(id, style, vec![seg], BoxRole::AnonymousInlineRun));
    }

    let mut trigger_style = anon_style(style);
    trigger_style.display = Display::Block;
    let trigger = LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style: Arc::new(trigger_style),
        kind: BoxKind::Block,
        children: trigger_children,
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        // Not GeneratedContent: this mirrors the `<select>`'s own selected-option
        // label, not `content:` — closest is the anonymous UA-scaffolding wrapper.
        origin: BoxOrigin { node: Some(id), role: BoxRole::AnonymousBlock },
    };

    LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style: Arc::new(style.clone()),
        // FlowRoot: establishes a BFC and lays out the trigger as a block child,
        // regardless of the select's own (inline-block) UA display.
        kind: BoxKind::FlowRoot,
        children: vec![trigger],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin { node: Some(id), role: BoxRole::Element },
    }
}

/// BUG-341 S4 — per-node reuse decision point for incremental box-build.
///
/// Called wherever `build_box` recurses into a DOM child. When incremental
/// box-build is enabled and `prev_index` still holds `id`'s subtree, **takes**
/// that previous [`LayoutBox`] subtree instead of rebuilding it. Otherwise
/// falls through to a normal `build_box` call (which itself threads
/// `prev_index` down, so a dirty ancestor's clean descendants still get reused
/// at their own level).
///
/// BUG-341 S19: membership in `prev_index` *is* the reuse licence — the index
/// is built by [`crate::incremental::extract_clean_subtrees`] from exactly
/// [`CounterMap::clean_subtrees`], so the separate `clean_subtrees` test S4-S18
/// did here would be asking the same question twice. Each entry can be taken
/// only once, which is also what keeps the previous tree's boxes from ending up
/// in two places at once.
#[allow(clippy::too_many_arguments)]
fn build_box_or_reuse(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev_index: Option<&crate::incremental::ReuseIndex>,
) -> LayoutBox {
    // BUG-341 S15: the gate is `prev_index.is_some()`, NOT the
    // `INCREMENTAL_BOX_BUILD` thread-local — `build_box` fans flex/grid
    // containers out over rayon workers, whose thread-locals start at their
    // defaults (the same trap `StyleEnvSnapshot` exists for), so a thread-local
    // check here silently disabled reuse for every child of a container with 8+
    // items. Chrome is built out of exactly such containers. The flag is
    // consulted once, at `incremental_build_box`, which is what decides whether
    // an index exists at all.
    if let Some(idx) = prev_index
        && let Some(cell) = idx.get(&id)
    {
        let taken = if box_build_diagnostics_on() {
            use std::sync::atomic::Ordering::Relaxed;
            let t = std::time::Instant::now();
            let taken = cell.lock().ok().and_then(|mut slot| slot.take());
            BOX_CLONE_NS.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
            if let Some(b) = taken.as_ref() {
                BOX_CLONE_BOXES.fetch_add(count_boxes(b), Relaxed);
            }
            taken
        } else {
            cell.lock().ok().and_then(|mut slot| slot.take())
        };
        if let Some(mut subtree) = taken {
            BOX_BUILD_STATS.with(|s| {
                let mut v = s.get();
                v.reused += 1;
                s.set(v);
            });
            // BUG-341 S18: tell the two stages that follow — `mark_subtree_dirty`
            // and `graft_geometry` — that this subtree came out of `prev` itself.
            // Both of them exist to answer "may this subtree keep the previous
            // pass's geometry", and here the answer is known by construction, so
            // both can honour it at the root instead of walking the copy against
            // its own original. Only the root carries the flag: the move stops the
            // recursion, so nothing inside it can hold a claim of its own.
            subtree.dirty = crate::incremental::DirtyBits::REUSED_SUBTREE;
            return subtree;
        }
    }
    build_box(doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, prev_index)
}

/// BUG-341 S4 — incremental box-build entry point.
///
/// Builds the `LayoutBox` tree rooted at `id`, **moving** whole subtrees out of
/// `prev` wherever [`CounterMap::clean_subtrees`] says it is safe to (see
/// [`build_box_or_reuse`]) instead of calling `build_box` for them. Must
/// reproduce a full `build_box` pass bit-for-bit for the same final state —
/// the `incr == full` differential tests in `incremental.rs` guard this.
///
/// BUG-341 S19: `prev` is taken by unique reference and is **gutted** by the
/// call — every reusable subtree ends up in the returned tree, and its old
/// position holds a husk (see [`crate::incremental::DirtyBits::MOVED_OUT`]).
/// The only thing a caller may still do with `prev` afterwards is hand it to
/// [`crate::incremental::graft_geometry_with_cascade`], which recognises the
/// husks; anything else must clone `prev` first.
///
/// Gated behind [`set_incremental_box_build`]: flag off (the default) makes
/// this behave exactly like `build_box(..., None)` and leaves `prev` untouched.
///
/// BUG-341 S15 wired this into [`layout_mutation_incremental_restyle`], the
/// chrome and page incremental pipelines' entry point. The full-layout entry
/// points (`layout_measured_hyp_with_counters`, `layout_streaming_incremental`)
/// still call `build_box` directly — they have no `RestyleDelta`, hence no
/// `clean_subtrees`, hence nothing to reuse.
#[allow(clippy::too_many_arguments)]
pub fn incremental_build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev: &mut LayoutBox,
) -> LayoutBox {
    if !incremental_box_build_enabled() {
        return build_box(doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, None);
    }
    let t = std::time::Instant::now();
    let (prev_index, visited) = crate::incremental::extract_clean_subtrees(prev, counters.clean_subtrees());
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.prev_index_visited += visited as u32;
        s.set(v);
    });
    if box_build_diagnostics_on() {
        note_prev_index(t.elapsed().as_nanos() as u64, visited);
    }
    build_box_or_reuse(doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, Some(&prev_index))
}

/// BUG-341 S20 — timing shim around [`build_box_inner`].
///
/// Off the census path (the overwhelmingly common case) this is one relaxed
/// atomic load and a direct call. With [`set_box_build_diagnostics`] on it
/// records the call's inclusive wall-clock into [`BOX_BUILD_TIME_LOG`]; the
/// timer lives here rather than inside the body so it covers every one of the
/// body's exit paths (`build_base_select_box`'s early return among them).
#[allow(clippy::too_many_arguments)]
fn build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev_index: Option<&crate::incremental::ReuseIndex>,
) -> LayoutBox {
    if !BOX_TIME_LOG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return build_box_inner(
            doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, prev_index,
        );
    }
    let t = std::time::Instant::now();
    let out = build_box_inner(
        doc, sheet, id, inherited, viewport, flat, counters, registry, dark_mode, prev_index,
    );
    let ns = t.elapsed().as_nanos() as u64;
    if let Ok(mut log) = BOX_BUILD_TIME_LOG.lock() {
        log.push((id, ns));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn build_box_inner(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    // BUG-341 S4/S19: `Some` when an incremental box-build pass is in progress —
    // an id→owned-subtree index carved out of the previous pass's tree,
    // consulted by `build_box_or_reuse` at every recursive call site below.
    // `None` for the full/legacy build path (all current pipeline entry points).
    prev_index: Option<&crate::incremental::ReuseIndex>,
) -> LayoutBox {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.built += 1;
        s.set(v);
    });
    note_box_built(id);
    // BUG-284: `precompute_counters` already ran a full document-order cascade
    // pass over this exact tree (same `inherited` chain, same sheet/viewport/
    // dark_mode) to resolve counter-reset/increment/set — reuse its cached
    // result instead of paying for an identical `compute_style` call again.
    let mut style = counters.style_arc(id).unwrap_or_else(|| {
        note_style_miss(|| {
            Arc::new(compute_style(doc, id, sheet, inherited, viewport, dark_mode))
        })
    });

    // HTML/CSS «Customizable Select»: a `<select appearance:base-select>` renders
    // as an author-styleable widget tree instead of the opaque native control.
    if style.appearance == crate::style::Appearance::BaseSelect
        && style.display != Display::None
        && is_base_select_host(doc, id)
    {
        return build_base_select_box(doc, &style, id);
    }

    let kind = match &doc.get(id).data {
        // Shadow root nodes are infrastructure — never rendered directly.
        // The flat tree already maps host children to shadow root's children.
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. } | NodeData::ShadowRoot { .. } | NodeData::DocumentFragment => BoxKind::Skip,
        NodeData::Document | NodeData::Element { .. } => {
            if style.display == Display::None || is_closed_popover(doc, id) || is_svg_defs(doc, id) {
                BoxKind::Skip
            } else if is_image_element(doc, id) {
                let src = resolve_image_source(doc, id, viewport);
                let alt = doc.get(id).get_attr("alt").unwrap_or("").to_string();
                // Intrinsic dimensions у выбранного `<source>` (а для голого
                // `<img>` — его собственные `width`/`height` атрибуты, куда
                // shell кладёт размер декодированной картинки) действуют как
                // presentational hint: заполняют только пустые слоты, не
                // перекрывают ни CSS-каскад, ни собственные `<img width|
                // height>` атрибуты (последние уже легли в style через
                // `apply_image_presentational_hints`). HTML5 §10 «mapped
                // attributes»: hint = UA-rule с specificity 0.
                //
                // BUG-734: когда известны ОБЕ стороны, это не два независимых
                // hint-а, а intrinsic **соотношение** — CSS Sizing L4 §4.1
                // («`aspect-ratio: auto` на замещаемом элементе = intrinsic
                // ratio») и CSS 2.1 §10.6.2. Подставлять сырое значение в
                // пустой слот нельзя: `width: 100px; height: auto` дало бы
                // 100×<intrinsic h> вместо 100×(100/ratio), а самый частый в
                // вебе `max-width: 100%` не пересчитывал бы высоту после
                // клампа ширины. Поэтому ratio уезжает в `style.aspect_ratio`
                // (если author его не задал), а сырой размер подставляется
                // ровно в одном случае «обе стороны auto» — и только в
                // ширину: высоту из неё выведет ratio-ветка, и она же
                // отработает после `min-`/`max-width`.
                let intrinsic_ratio = match (src.intrinsic_width, src.intrinsic_height) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as f32, h as f32)),
                    _ => None,
                };
                if let Some((iw, ih)) = intrinsic_ratio {
                    let st = Arc::make_mut(&mut style);
                    if st.aspect_ratio.is_none() {
                        st.aspect_ratio = Some((iw, ih));
                    }
                    if st.width.is_none() && st.height.is_none() {
                        st.width = Some(Length::Px(iw));
                    }
                } else {
                    // Известна одна сторона — ratio не построить, поведение
                    // прежнее: hint заполняет пустой слот.
                    if style.width.is_none()
                        && let Some(w) = src.intrinsic_width
                    {
                        Arc::make_mut(&mut style).width = Some(Length::Px(w as f32));
                    }
                    if style.height.is_none()
                        && let Some(h) = src.intrinsic_height
                    {
                        Arc::make_mut(&mut style).height = Some(Length::Px(h as f32));
                    }
                }
                let is_lazy = doc.get(id).get_attr("loading")
                    .is_some_and(|v| v.eq_ignore_ascii_case("lazy"));
                BoxKind::Image { src: src.url, alt, is_lazy }
            } else if is_video_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let poster = node.get_attr("poster").unwrap_or("").to_string();
                // HTML spec §14.1: UA default intrinsic size is 300×150 CSS px.
                // Explicit width/height attrs applied earlier as presentational hints;
                // fill only if still unset.
                if style.width.is_none() {
                    Arc::make_mut(&mut style).width = Some(Length::Px(300.0));
                }
                if style.height.is_none() {
                    Arc::make_mut(&mut style).height = Some(Length::Px(150.0));
                }
                BoxKind::Video { src, poster }
            } else if is_canvas_element(doc, id) {
                let node = doc.get(id);
                // HTML LS §4.12.4: width/height content attributes reflect as
                // `unsigned long`; defaults are 300×150 CSS px.
                //
                // BUG-452: this was `v.trim().parse::<u32>()`, whose rules are
                // neither the spec's nor `parseInt`'s — it rejected `"100.999"`,
                // `"100em"` and `"0x100"` (§2.4.4.1 gives 100/100/**0**), so the
                // box was laid out at the 300×150 default while `canvas.width`
                // from script answered 100 off the JS mirror of the same rule.
                let cw = lumen_dom::attr_int::reflect_unsigned_long(node.get_attr("width"), 300);
                let ch = lumen_dom::attr_int::reflect_unsigned_long(node.get_attr("height"), 150);
                // The bitmap dimensions act as intrinsic size; explicit CSS
                // width/height (or presentational hints) win if already set.
                //
                // BUG-099: unlike `<img>`/`<video>`, HTML Rendering §15.4.1 does
                // NOT map the `<canvas>` dimension attributes to the `width`/
                // `height` properties — they are the element's *intrinsic* size,
                // i.e. a content-box size. Feeding them through `style.width`
                // makes `box-sizing: border-box` subtract borders and padding
                // from the bitmap, shrinking the element (TEST-57 c3: 180×150
                // instead of Edge's 186×156 border box). Add the border+padding
                // back so that the resulting *content* box stays the bitmap size.
                // % padding resolves against the containing block, unknown here —
                // it degrades to 0, same limitation as the `<img>` hint above.
                let (fill_extra_w, fill_extra_h) = match style.box_sizing {
                    BoxSizing::ContentBox => (0.0, 0.0),
                    BoxSizing::BorderBox => {
                        let em = style.font_size;
                        (
                            style.border_left_width
                                + style.border_right_width
                                + style.padding_left.resolve_or_zero(em, 0.0, viewport)
                                + style.padding_right.resolve_or_zero(em, 0.0, viewport),
                            style.border_top_width
                                + style.border_bottom_width
                                + style.padding_top.resolve_or_zero(em, 0.0, viewport)
                                + style.padding_bottom.resolve_or_zero(em, 0.0, viewport),
                        )
                    }
                };
                if style.width.is_none() {
                    Arc::make_mut(&mut style).width = Some(Length::Px(cw as f32 + fill_extra_w));
                }
                if style.height.is_none() {
                    Arc::make_mut(&mut style).height = Some(Length::Px(ch as f32 + fill_extra_h));
                }
                BoxKind::Canvas { width: cw, height: ch }
            } else if is_audio_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let controls = node.get_attr("controls").is_some();
                // HTML spec §4.8.10: without controls, <audio> has no box (0×0).
                // With controls, UA must render a control interface; we use 40px height.
                if controls {
                    if style.height.is_none() {
                        Arc::make_mut(&mut style).height = Some(Length::Px(40.0));
                    }
                } else {
                    Arc::make_mut(&mut style).width = Some(Length::Px(0.0));
                    Arc::make_mut(&mut style).height = Some(Length::Px(0.0));
                }
                BoxKind::Audio { src, controls }
            } else if is_iframe_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let srcdoc = node.get_attr("srcdoc").filter(|s| !s.is_empty()).map(str::to_owned);
                // HTML spec §4.8.5: UA default intrinsic size is 300×150 CSS px.
                // Explicit width/height attrs applied earlier as presentational hints;
                // fill only if still unset.
                if style.width.is_none() {
                    Arc::make_mut(&mut style).width = Some(Length::Px(300.0));
                }
                if style.height.is_none() {
                    Arc::make_mut(&mut style).height = Some(Length::Px(150.0));
                }
                BoxKind::Iframe { src, srcdoc }
            } else if is_form_control_element(doc, id) {
                let kind = {
                    let node = doc.get(id);
                    let tag = node.element_name()
                        .map(|q| q.local.as_str())
                        .unwrap_or("")
                        .to_owned();
                    match tag.as_str() {
                        "button"   => FormControlKind::Button,
                        "select"   => {
                            let selected_text = collect_select_label(doc, id);
                            FormControlKind::Select { selected_text }
                        }
                        // <selectlist> (Customizable Select, Phase 0) renders as a
                        // native-select widget. P4 wires ::picker(select) appearance.
                        // CSS: appearance: base-select
                        "selectlist" => {
                            let selected_text = collect_selectlist_label(doc, id);
                            FormControlKind::Select { selected_text }
                        }
                        "textarea" => {
                            let value_text = collect_textarea_content(doc, id);
                            FormControlKind::Textarea { value_text }
                        }
                        "progress" => {
                            let max = node.get_attr("max")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(1.0)
                                .max(f32::EPSILON);
                            let value = node.get_attr("value")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .map(|v| v.clamp(0.0, max));
                            FormControlKind::Progress { value, max }
                        }
                        "meter" => {
                            let raw_min = node.get_attr("min")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(0.0);
                            let raw_max = node.get_attr("max")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(1.0);
                            // Spec §4.10.14: if min ≥ max, reset to defaults 0/1.
                            let (min, max) = if raw_min < raw_max {
                                (raw_min, raw_max)
                            } else {
                                (0.0, 1.0)
                            };
                            let low = node.get_attr("low")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(min)
                                .clamp(min, max);
                            let high = node.get_attr("high")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(max)
                                .clamp(min, max);
                            let optimum = node.get_attr("optimum")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or((min + max) / 2.0)
                                .clamp(min, max);
                            let value = node.get_attr("value")
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(0.0)
                                .clamp(min, max);
                            FormControlKind::Meter { value, min, max, low, high, optimum }
                        }
                        _ => {
                            let input_type = node.input_type()
                                .unwrap_or(lumen_dom::InputType::Text);
                            if input_type == lumen_dom::InputType::Range {
                                let min = node.get_attr("min")
                                    .and_then(|v| v.trim().parse::<f32>().ok())
                                    .unwrap_or(0.0);
                                let max = node.get_attr("max")
                                    .and_then(|v| v.trim().parse::<f32>().ok())
                                    .unwrap_or(100.0);
                                let default_val = (min + max) / 2.0;
                                let value = doc.control_value(id)
                                    .trim()
                                    .parse::<f32>()
                                    .ok()
                                    .unwrap_or(default_val)
                                    .clamp(min, max);
                                FormControlKind::Range { value, min, max }
                            } else {
                                // BUG-444: the painted mark is the control's
                                // current checkedness; `checked=` is only its
                                // default.
                                let checked = doc.control_checked(id);
                                // BUG-441: the painted text is the control's
                                // current value; `value=` is only its default.
                                let value_text = doc.control_value(id).into_owned();
                                let placeholder = node.get_attr("placeholder")
                                    .unwrap_or("")
                                    .to_owned();
                                let placeholder_style = compute_pseudo_element_style(
                                    doc, id, "placeholder", sheet, &style, viewport, dark_mode,
                                ).map(Box::new);
                                FormControlKind::Input { input_type, checked, value_text, placeholder, placeholder_style }
                            }
                        }
                    }
                };
                BoxKind::FormControl { kind }
            } else if matches!(style.display, Display::TableRow) {
                BoxKind::TableRow
            } else if matches!(style.display, Display::Table | Display::InlineTable) {
                BoxKind::Table
            } else if matches!(
                style.display,
                Display::TableRowGroup
                    | Display::TableHeaderGroup
                    | Display::TableFooterGroup
            ) {
                BoxKind::TableRowGroup
            } else if matches!(style.display, Display::FlowRoot) {
                BoxKind::FlowRoot
            } else if matches!(style.display, Display::Contents) {
                BoxKind::Contents
            } else if is_svg_root(doc, id) {
                // SVG root: apply width/height attributes as presentational hints.
                // CSS: width, height — if author CSS is absent, attribute values are used.
                // CSS: object-fit, object-position — P4 can override viewBox scaling (Phase 2)
                // CSS: intrinsic aspect-ratio from viewBox for replaced element sizing
                if style.width.is_none()
                    && let Some(w) = doc.get(id).get_attr("width").and_then(|v| v.trim().parse::<f32>().ok())
                {
                    Arc::make_mut(&mut style).width = Some(crate::style::Length::Px(w));
                }
                if style.height.is_none()
                    && let Some(h) = doc.get(id).get_attr("height").and_then(|v| v.trim().parse::<f32>().ok())
                {
                    Arc::make_mut(&mut style).height = Some(crate::style::Length::Px(h));
                }
                BoxKind::SvgRoot {
                    view_box: parse_view_box(doc, id),
                    preserve_aspect_ratio: parse_preserve_aspect_ratio(doc, id),
                }
            } else {
                BoxKind::Block
            }
        }
    };

    // CSS Containment L3 §4 — content-visibility: hidden suppresses the subtree.
    // Phase 1: element keeps its own box but contributes 0×0 (no contain-intrinsic-size yet).
    // content-visibility: auto (off-viewport skip) is deferred to Phase 2.
    if style.content_visibility == crate::style::ContentVisibility::Hidden {
        return LayoutBox {
            node: id,
            rect: Rect::ZERO,
            style,
            kind,
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: Default::default(),
            origin: BoxOrigin { node: Some(id), role: BoxRole::Element },
        };
    }

    let mut children = Vec::new();
    // BUG-441: a `<textarea>` renders its *current* value. Unlike `<input>`,
    // whose text the form-control painter draws from `FormControlKind`, a
    // textarea's text is ordinary inline content laid out from its DOM
    // children — and those children are only its *default* value (HTML LS
    // §4.10.11). Once the control has a runtime value, that value is laid out
    // in their place, so typing and `el.value = …` reach the screen without
    // rewriting the markup they came from.
    let textarea_runtime_value: Option<String> = match &kind {
        BoxKind::FormControl { kind: FormControlKind::Textarea { value_text } }
            if doc.dirty_value(id).is_some() =>
        {
            Some(value_text.clone())
        }
        _ => None,
    };
    if let Some(value_text) = &textarea_runtime_value {
        children.push(anon_inline_run(
            id,
            &style,
            control_value_segments(id, value_text, &style),
            BoxRole::AnonymousInlineRun,
        ));
    }
    if matches!(kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Contents | BoxKind::FormControl { .. } | BoxKind::TableRow | BoxKind::Table | BoxKind::TableRowGroup | BoxKind::SvgRoot { .. }) {
        // CSS: :host, ::slotted — P4 wires shadow-scoped styles here
        // HTML5 §4.11.1 — <details>: when `open` attribute absent, only <summary> is rendered.
        // P3 wires: clicking <summary> should toggle `open` attribute + relayout.
        let dom_children: Vec<NodeId> = if textarea_runtime_value.is_some() {
            // The default value's text nodes are replaced by the run above.
            Vec::new()
        } else if is_details_element(doc, id)
            && doc.get(id).get_attr("open").is_none()
        {
            flat.children_of(doc, id)
                .iter()
                .copied()
                .filter(|&cid| is_summary_element(doc, cid))
                .collect()
        } else {
            flat.children_of(doc, id).to_vec()
        };
        // CSS Grid L1 §6: all direct children of a grid/flex container are
        // "blockified" — they participate as individual items, not wrapped in
        // InlineRun. Skip the inline-collection logic for these containers.
        let is_item_container = matches!(
            style.display,
            Display::Grid | Display::InlineGrid | Display::Flex | Display::InlineFlex
                | Display::TableRow
                | Display::Table | Display::InlineTable
                | Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup
        );
        if is_item_container {
            // CSS Flexbox §4 / Grid §6: text runs directly inside a flex/grid
            // container become anonymous items. Tables keep their own
            // anonymous-box rules (text → anonymous cell), so wrap only for
            // flex/grid here.
            let wrap_text_items = matches!(
                style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
            );

            // ADR-016 M4.1 — parallel selector matching for large item containers.
            // Siblings in a flex/grid/table container share only the immutable parent
            // style; their `compute_style` calls are fully independent. rayon worker
            // threads start with default thread-locals (no interactive state, no shadow
            // sheets), so we capture a `StyleEnvSnapshot` on the layout thread and
            // install it at the top of each closure before any style work runs.
            // The parallel path produces identical results to the sequential path;
            // item order is preserved by rayon's par_iter + collect guarantee.
            //
            // Threshold: only parallelize when the item count justifies the rayon
            // spawn overhead (~1–2 µs per closure on a warm thread pool).
            const RAYON_MIN_FLEX_CHILDREN: usize = 8;
            // BUG-341 S20: the threshold counts the children this pass will
            // really **build**, not the ones the container happens to have.
            //
            // M4.1 sized the threshold against a full pass, where every child
            // costs a cascade plus a box build and eight of them comfortably
            // outweigh the fan-out. On the incremental path since S15/S19 a
            // child that is in `prev_index` costs a `Mutex` lock and a move —
            // and on a chrome interaction nearly all of them are. Counting DOM
            // children there dispatched a worker per reused subtree to do
            // nothing: measured at ~1 ms of a 2.5 ms keystroke cycle, spread
            // over `body`/`.main-col`/`.omnibox-wrap`, each of whose *own* work
            // is ~4 µs (BUG-341 "S20" census). Same shape as S18 — the stage
            // was deciding for itself something the reuse mechanism had already
            // established.
            //
            // Non-element children are excluded from the estimate for the same
            // reason: whitespace between pretty-printed markup never enters the
            // reuse index (it holds elements only), yet it costs a `Skip` box or
            // one small anonymous item — never the cascade the threshold was
            // sized against. Counting it kept `body` above the threshold on a
            // cycle where every one of its element children was a move.
            //
            // `None` (every full-layout entry point) leaves the decision at
            // `dom_children.len()`, i.e. M4.1's behaviour byte for byte.
            let children_to_build = match prev_index {
                None => dom_children.len(),
                Some(idx) => dom_children
                    .iter()
                    .filter(|&&c| {
                        !idx.contains_key(&c)
                            && matches!(doc.get(c).data, NodeData::Element { .. })
                    })
                    .count(),
            };
            if children_to_build >= RAYON_MIN_FLEX_CHILDREN {
                use rayon::prelude::*;
                let snap = crate::style::StyleEnvSnapshot::capture();
                // BUG-341 S15: each closure drains the tally of whatever thread
                // ran it into this shared counter, which is folded back into the
                // parent's thread below. Draining is exact even when rayon
                // work-steals a closure onto the calling thread — whatever it
                // takes from that thread's tally comes straight back in the
                // fold. Without it every box built under a container with 8+
                // items was invisible to the reuse gates.
                let par_built = std::sync::atomic::AtomicU32::new(0);
                let par_reused = std::sync::atomic::AtomicU32::new(0);
                // Nested containers a worker fans out again are folded back
                // through the same drain as `built`/`reused` below.
                let par_fanouts = std::sync::atomic::AtomicU32::new(0);
                // BUG-341 S25: same drain for the display-probe / style-miss
                // tallies, for the same reason as `built`/`reused` above.
                let par_probes = std::sync::atomic::AtomicU32::new(0);
                let par_probe_cascades = std::sync::atomic::AtomicU32::new(0);
                let par_misses = std::sync::atomic::AtomicU32::new(0);
                // BUG-341 S21: the cascade's rule index is per-thread too, so a
                // worker that has not seen this sheet builds its own. Drained
                // through the same fold as the box tallies, for the same reason
                // — otherwise the gate that asserts a pass rebuilds no index
                // would be blind to every rebuild a worker made.
                let par_index_stats = std::sync::Mutex::new(crate::style::CascadeIndexStats::default());
                // BUG-341 S23: same drain for the pseudo-element cascade census.
                // Without it the census undercounts exactly the containers this
                // branch exists for — every flex/grid container with 8+ items.
                let par_pseudo_stats =
                    std::sync::Mutex::new(crate::style::PseudoCascadeStats::default());
                let par_pseudo_sites: std::sync::Mutex<
                    std::collections::HashMap<String, crate::style::PseudoCascadeStats>,
                > = std::sync::Mutex::new(std::collections::HashMap::new());
                children = dom_children.par_iter().filter_map(|&child_id| {
                    snap.install();
                    let out = if wrap_text_items && matches!(doc.get(child_id).data, NodeData::Text(_)) {
                        build_anon_text_item(
                            doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode,
                        )
                    } else {
                        let b = build_box_or_reuse(
                            doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode, prev_index,
                        );
                        if matches!(b.kind, BoxKind::Skip) { None } else { Some(b) }
                    };
                    let d = take_box_build_stats();
                    use std::sync::atomic::Ordering::Relaxed;
                    par_built.fetch_add(d.built, Relaxed);
                    par_reused.fetch_add(d.reused, Relaxed);
                    par_fanouts.fetch_add(d.fanouts, Relaxed);
                    par_probes.fetch_add(d.display_probes, Relaxed);
                    par_probe_cascades.fetch_add(d.display_probe_cascades, Relaxed);
                    par_misses.fetch_add(d.style_misses, Relaxed);
                    let idx_stats = crate::style::take_cascade_index_stats();
                    par_index_stats
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .add(idx_stats);
                    let ps_stats = crate::style::take_pseudo_cascade_stats();
                    par_pseudo_stats
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .add(ps_stats);
                    let ps_sites = crate::style::take_pseudo_cascade_sites();
                    if !ps_sites.is_empty() {
                        let mut acc = par_pseudo_sites.lock().unwrap_or_else(|e| e.into_inner());
                        for (k, v) in ps_sites {
                            acc.entry(k).or_default().add(v);
                        }
                    }
                    out
                }).collect();
                crate::style::add_cascade_index_stats(
                    *par_index_stats.lock().unwrap_or_else(|e| e.into_inner()),
                );
                crate::style::add_pseudo_cascade_stats(
                    *par_pseudo_stats.lock().unwrap_or_else(|e| e.into_inner()),
                );
                crate::style::add_pseudo_cascade_sites(std::mem::take(
                    &mut *par_pseudo_sites.lock().unwrap_or_else(|e| e.into_inner()),
                ));
                {
                    use std::sync::atomic::Ordering::Relaxed;
                    add_box_build_stats(BoxBuildStats {
                        built: par_built.load(Relaxed),
                        reused: par_reused.load(Relaxed),
                        // Extraction runs once, on the thread that owns `prev`
                        // — a worker never adds to this.
                        prev_index_visited: 0,
                        // This container's own dispatch, plus any a worker made.
                        fanouts: par_fanouts.load(Relaxed) + 1,
                        display_probes: par_probes.load(Relaxed),
                        display_probe_cascades: par_probe_cascades.load(Relaxed),
                        style_misses: par_misses.load(Relaxed),
                    });
                }
            } else {
                for child_id in dom_children {
                    if wrap_text_items
                        && matches!(doc.get(child_id).data, NodeData::Text(_))
                    {
                        if let Some(item) = build_anon_text_item(
                            doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode,
                        ) {
                            children.push(item);
                        }
                        continue;
                    }
                    let child_box = build_box_or_reuse(
                        doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode, prev_index,
                    );
                    if !matches!(child_box.kind, BoxKind::Skip) {
                        children.push(child_box);
                    }
                }
            }
            // CSS Flexbox §4 / Grid §6 — ::before / ::after on a flex or grid
            // container generate blockified flex/grid items (first and last,
            // respectively). Tables have their own anonymous-box rules, so they
            // are excluded here.
            if matches!(
                style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
            ) {
                let before_ps = compute_pseudo_element_style(
                    doc, id, "before", sheet, &style, viewport, dark_mode,
                );
                let after_ps = compute_pseudo_element_style(
                    doc, id, "after", sheet, &style, viewport, dark_mode,
                );
                inject_pseudo(id, &mut children, before_ps, true, doc, viewport, counters, registry, true);
                inject_pseudo(id, &mut children, after_ps, false, doc, viewport, counters, registry, true);
            }
        } else {
        let mut i = 0;
        while i < dom_children.len() {
            let child_id = dom_children[i];
            let is_inl =
                is_inline_content(doc, sheet, child_id, &style, viewport, dark_mode, counters);
            let is_ib = !is_inl
                && is_atomic_inline_level(doc, sheet, child_id, &style, viewport, dark_mode, counters);

            if is_inl || is_ib {
                // Унифицированный сбор inline-уровневого контента: inline-элементы
                // и atomic inline-level (`inline-block`/`inline-flex`/
                // `inline-grid`) участвуют в ОДНОМ inline-контексте.
                // Межэлементный whitespace не прерывает поток.
                // Результат: InlineRun (чистый текст) или InlineBlockRow (смешанный).
                let mut row_items: Vec<LayoutBox> = Vec::new();
                let mut pending: Vec<InlineSegment> = Vec::new();
                // BUG-728: потомки inline-элементов, которым нужен собственный
                // бокс. Индексы `at` считаются по общему `pending`, поэтому
                // вектор один на весь цикл, как и `pending`.
                let mut pending_escapes: Vec<InlineEscape> = Vec::new();
                // CSS §4.1.2 white-space collapsing: whitespace between
                // inline-level siblings collapses to a single space.
                let mut had_ws = false;
                // CSS Pseudo-elements L4 §5.1: first letter of this inline run hasn't been
                // split out yet. Passed through all collect_inline_segments calls in this loop.
                let mut need_first_letter = true;
                // CSS Pseudo-elements L4 §5.3: pre-compute ::first-line style once for this block.
                // BUG-341 S23: skipped outright on a sheet that never uses
                // `::first-line` as a selector subject — the cascade could only
                // return `None` there, and this runs per inline-content block.
                let first_line_style = if crate::style::sheet_targets_pseudo(sheet, viewport, dark_mode, "first-line") {
                    crate::style::compute_pseudo_element_style(doc, id, "first-line", sheet, &style, viewport, dark_mode)
                        .map(Box::new)
                } else {
                    None
                };
                // Track whether first_line_style has been assigned to the first InlineRun.
                let mut first_line_assigned = false;

                loop {
                    if i >= dom_children.len() {
                        break;
                    }
                    let cid = dom_children[i];
                    match &doc.get(cid).data {
                        // BUG-120: control-only text is skipped like whitespace-only,
                        // but contributes an inter-segment space only if it actually
                        // contains whitespace (a bare U+0001 is zero-advance in Edge).
                        NodeData::Text(s)
                            if s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)) =>
                        {
                            had_ws |= s.chars().any(char::is_whitespace);
                            i += 1;
                            continue;
                        }
                        NodeData::Comment(_) | NodeData::Doctype { .. } => {
                            i += 1;
                            continue;
                        }
                        _ => {}
                    }
                    if is_inline_content(doc, sheet, cid, &style, viewport, dark_mode, counters) {
                        // CSS §4.1.1 — collapsed whitespace between inline-level
                        // siblings becomes a single inter-word gap. Record it as a
                        // trailing space on the previous segment so wrap_inline_run
                        // inserts exactly one space at the boundary; without it,
                        // `<span>a</span> <span>b</span>` would join tightly.
                        if had_ws
                            && let Some(last) = pending.last_mut()
                            && !last.forced_break
                            && !last.style.white_space.preserves_whitespace()
                            && !last.text.ends_with(|c: char| c.is_whitespace())
                        {
                            last.text.push(' ');
                        }
                        collect_inline_segments(doc, sheet, cid, &style, viewport, &mut pending, &mut pending_escapes, flat, counters, registry, &mut need_first_letter, dark_mode);
                        had_ws = false;
                        i += 1;
                    } else if is_atomic_inline_level(doc, sheet, cid, &style, viewport, dark_mode, counters)
                    {
                        if !pending.is_empty() || !pending_escapes.is_empty() {
                            let from = row_items.len();
                            split_inline_pieces(
                                doc, sheet, id, &style, viewport, flat, counters, registry,
                                dark_mode, prev_index,
                                std::mem::take(&mut pending),
                                std::mem::take(&mut pending_escapes),
                                &mut row_items,
                            );
                            assign_first_line_style(
                                &mut row_items[from..], &first_line_style, &mut first_line_assigned,
                            );
                        }
                        // Whitespace between inline-blocks → collapsed space gap.
                        if had_ws && !row_items.is_empty() {
                            row_items.push(LayoutBox {
                                node: id,
                                rect: Rect::ZERO,
                                style: Arc::new(anon_style(&style)),
                                kind: BoxKind::InlineSpace,
                                children: vec![],
                                col_span: 1,
                                row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                                origin: BoxOrigin { node: Some(id), role: BoxRole::AnonymousInlineRun },
                            });
                        }
                        row_items.push(build_box_or_reuse(doc, sheet, cid, &style, viewport, flat, counters, registry, dark_mode, prev_index));
                        had_ws = false;
                        i += 1;
                    } else if matches!(doc.get(cid).data, NodeData::Element { .. })
                        && probe_display(doc, sheet, cid, &style, viewport, dark_mode, counters)
                            == Display::None
                    {
                        // display:none не прерывает inline-контекст — CSS §9.2.4.
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !pending.is_empty() || !pending_escapes.is_empty() {
                    let from = row_items.len();
                    split_inline_pieces(
                        doc, sheet, id, &style, viewport, flat, counters, registry,
                        dark_mode, prev_index,
                        std::mem::take(&mut pending),
                        std::mem::take(&mut pending_escapes),
                        &mut row_items,
                    );
                    assign_first_line_style(
                        &mut row_items[from..], &first_line_style, &mut first_line_assigned,
                    );
                }

                // CSS Pseudo-elements L4 §5.1 — apply ::first-letter style.
                // collect_inline_segments marks the first non-whitespace text segment
                // with PseudoKind::FirstLetter; split it here so wrap_inline_run uses
                // the override font metrics for both the letter and the remainder.
                let fl_pseudo = compute_pseudo_element_style(
                    doc, id, "first-letter", sheet, &style, viewport, dark_mode,
                );
                // CSS Inline Layout L3 §5 — `initial-letter`. Effective value:
                // a ::first-letter pseudo `initial-letter` wins over the element's
                // own; `size > 1` activates the drop cap and supersedes the legacy
                // float-::first-letter path.
                let initial_letter = fl_pseudo
                    .as_ref()
                    .map(|p| (p.initial_letter_size, p.initial_letter_sink))
                    .filter(|(s, _)| *s > 1.0)
                    .or_else(|| {
                        (style.initial_letter_size > 1.0)
                            .then_some((style.initial_letter_size, style.initial_letter_sink))
                    });
                // ::first-letter / initial-letter target the block's first formatted
                // line, so only the inline group that opens the block qualifies.
                let first_group = children
                    .iter()
                    .all(|c| matches!(c.kind, BoxKind::Marker { .. }));
                if let Some((size, sink)) = initial_letter {
                    if first_group
                        && let Some(letter) = extract_initial_letter(
                            &mut row_items, &style, fl_pseudo.as_ref(), size, sink,
                        )
                    {
                        children.push(letter);
                    }
                } else if let Some(fl_style) = fl_pseudo {
                    // CSS Pseudo-elements L4 §5.2 — float ::first-letter → drop cap
                    // (BB-2): promote the letter to a block-level float sibling placed
                    // before the run.
                    if fl_style.float_side != FloatSide::None && first_group {
                        if let Some(letter) = extract_first_letter_float(&mut row_items, &fl_style) {
                            children.push(letter);
                        }
                    } else {
                        apply_first_letter_style(&mut row_items, fl_style, &style);
                    }
                }

                // BUG-728 / CSS 2.1 §9.2.1.1: блочно-уровневый бокс, всплывший
                // из inline-элемента, разрывает inline-контекст — контент до
                // него и после него образуют РАЗНЫЕ анонимные группы, а сам он
                // становится блочным сиблингом. Без escape-ов цикл вырождается
                // в прежнюю одну группу на весь ряд.
                let mut group: Vec<LayoutBox> = Vec::new();
                let flush_group = |group: &mut Vec<LayoutBox>, children: &mut Vec<LayoutBox>| {
                    match group.len() {
                        0 => {}
                        // Единственный чисто-текстовый run — без лишней обёртки.
                        1 if matches!(group[0].kind, BoxKind::InlineRun { .. }) => {
                            children.push(group.remove(0));
                        }
                        // Несколько элементов или inline-block → InlineBlockRow.
                        _ => {
                            children.push(anon_inline_block_row(id, &style, std::mem::take(group)));
                        }
                    }
                };
                for item in row_items.drain(..) {
                    if breaks_inline_row(&item) {
                        flush_group(&mut group, &mut children);
                        children.push(item);
                    } else {
                        group.push(item);
                    }
                }
                flush_group(&mut group, &mut children);
            } else {
                children.push(build_box_or_reuse(doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode, prev_index));
                i += 1;
            }
        }
        // CSS Pseudo-elements L4 §4 — inject ::before / ::after for block-flow.
        // Only for Block / FlowRoot (not FormControl, not flex/grid item containers).
        if matches!(kind, BoxKind::Block | BoxKind::FlowRoot) {
            let before_ps =
                compute_pseudo_element_style(doc, id, "before", sheet, &style, viewport, dark_mode);
            let after_ps =
                compute_pseudo_element_style(doc, id, "after", sheet, &style, viewport, dark_mode);
            inject_pseudo(id, &mut children, before_ps, true, doc, viewport, counters, registry, false);
            inject_pseudo(id, &mut children, after_ps, false, doc, viewport, counters, registry, false);
            // CSS Lists L3 §2.1 — inject ::marker for list items.
            // ::marker comes before ::before in document order.
            if style.display == Display::ListItem {
                let ordinal = li_ordinal(doc, id);
                inject_marker(id, &mut children, &style, ordinal,
                              doc, sheet, viewport, dark_mode, counters, registry);
            }
        }
        } // end else (non-item-container)
        // CSS Display L3 §7.2 — flatten display:contents boxes into this context.
        // Must run for ALL child-building paths (item-container and non-item-container)
        // because flex/grid/table children may include display:contents elements whose
        // Contents boxes must be unpacked before lay_out sees them.
        flatten_contents(&mut children);
    }

    // SVG root: build SVG shape children (separate from HTML box-tree flow).
    if let BoxKind::SvgRoot { view_box, .. } = &kind {
        let own_svg_size = svg_root_own_size(&style, view_box.as_ref(), viewport);
        children = build_svg_children(doc, sheet, id, &style, viewport, own_svg_size, flat, dark_mode);
    }

    // Read HTML colspan/rowspan attributes for table-cell elements.
    let (col_span, row_span) = if style.display == Display::TableCell {
        let cs = doc
            .get(id)
            .get_attr("colspan")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);
        let rs = doc
            .get(id)
            .get_attr("rowspan")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);
        (cs, rs)
    } else {
        (1, 1)
    };

    LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style,
        kind,
        children,
        col_span,
        row_span,
        svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(id), role: BoxRole::Element },
    }
}

/// CSS Intrinsic Sizing L3 §4.1 / CSS 2.1 §10.3.7 — does `c` contribute to its
/// parent's intrinsic (max-content / min-content / shrink-to-fit) width?
///
/// Two kinds of children do not:
/// * `display: none` (`BoxKind::Skip`) — no box is generated at all, so not even
///   the element's own padding/border may be counted;
/// * out-of-flow boxes (`position: absolute`/`fixed`) — they are sized against a
///   containing block, not against their parent's content, and are laid out
///   after it. A nav item holding a hidden 1104px-wide mega-menu dropdown must
///   still be as wide as its label (BUG-738, `tbank.ru` top navigation).
fn contributes_to_intrinsic_width(c: &LayoutBox) -> bool {
    !matches!(c.kind, BoxKind::Skip)
        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
}

/// Is `b` a **row-direction** flex container (`display: flex`/`inline-flex`
/// with `flex-direction: row`/`row-reverse`)?
///
/// Only the row axis matters for intrinsic *width*: a column flex container
/// stacks its items vertically, exactly like a block container, so the existing
/// "widest child" rule is already right for it.
fn is_row_flex_container(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Flex | Display::InlineFlex)
        && !matches!(
            b.style.flex_direction,
            FlexDirection::Column | FlexDirection::ColumnReverse
        )
}

/// CSS Flexbox L1 §9.9 — intrinsic width contribution of a **row-direction**
/// flex container: its items sit side by side on the main axis, so the
/// container's intrinsic width is the *sum* of the items' outer (margin-box)
/// intrinsic widths plus the `column-gap` between them — not the maximum, which
/// is what the block-container rule (children stack vertically) yields.
///
/// `per_item` supplies the caller's own notion of an item's border-box
/// intrinsic width (max-content, min-content or shrink-to-fit preferred);
/// margins and gaps are added here so every caller agrees on them.
///
/// Same class of defect as [BUG-178] for floats: a formatting context whose
/// children are laid out horizontally was being measured with the vertical rule.
///
/// Item selection mirrors `lay_out_flex`: `Skip` boxes and absolutely-positioned
/// children are not flex items (§4.1) and contribute nothing.
///
/// Percentage `column-gap` resolves against the container's own content box,
/// which is exactly what intrinsic sizing does not know yet — it resolves to
/// zero here, consistent with every other percentage in these functions.
fn flex_row_intrinsic_sum(
    b: &LayoutBox,
    viewport: Size,
    per_item: &dyn Fn(&LayoutBox) -> f32,
) -> f32 {
    let gap = b
        .style
        .column_gap
        .resolve(b.style.font_size, Some(0.0), viewport)
        .unwrap_or(0.0)
        .max(0.0);
    let mut sum = 0.0_f32;
    let mut n_items = 0_usize;
    for c in &b.children {
        if !contributes_to_intrinsic_width(c) {
            continue;
        }
        let cem = c.style.font_size;
        let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
        let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
        sum += per_item(c) + ml + mr;
        n_items += 1;
    }
    sum + gap * n_items.saturating_sub(1) as f32
}

/// Phase 0 shrink-to-fit: возвращает «предпочтительную» ширину inline-block-бокса
/// (включая padding+border самого бокса). Алгоритм: если у бокса явная CSS `width` —
/// берём её; иначе рекурсивно ищем максимальную preferred_width среди потомков
/// и добавляем padding+border текущего бокса. Возвращает `None` если явных размеров
/// нет ни у бокса, ни у его потомков.
///
/// Для typed-Length полей используем em = font_size, cb_width = 0 как
/// аппроксимацию (shrink-to-fit не знает cb_width заранее).
fn preferred_inline_block_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> Option<f32> {
    let s = &b.style;
    let em = s.font_size;
    // % ширины на этом этапе не разрешима — трактуем как отсутствие.
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // CSS Sizing L3 §5.2.1 (BUG-742): процентная `width` в intrinsic-контексте
    // неразрешима и ведёт себя как `auto` — вклад считается по содержимому.
    // `percent_basis: None` (а не `Some(0.0)`) — единственное отличие от
    // остальных длин: иначе `width: 100%` давала бы 0 и целиком стирала вклад
    // поддерева, оставляя от бокса только его собственные padding + border.
    if let Some(w_len) = &s.width
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr
                + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return Some(outer.max(0.0));
    }
    // InlineRun — чисто-текстовый анонимный run: preferred = max-content ширина
    // текста (все сегменты на одной строке, без переноса). Без этой ветки
    // text-only inline-block (`<span style="display:inline-block">текст</span>`)
    // получал content_w = 0 (текст лежит в `segments`, а не в `children`) → None
    // → shrink-to-fit не применялся → бокс растягивался на всю доступную ширину
    // вместо обтягивания текста (BUG-202).
    if let BoxKind::InlineRun { segments, .. } = &b.kind {
        let text_w = measurer.map_or(0.0, |m| {
            segments
                .iter()
                .map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let ts = seg.style.tab_size
                        * m.char_width_with_families(' ', seg.style.font_size, fams);
                    measure_text_w_families(&seg.text, seg.style.font_size, ls, ts, fams, m)
                })
                .sum()
        });
        return if text_w > 0.0 { Some(text_w) } else { None };
    }
    // InlineBlockRow — горизонтальный поток: суммируем ширины детей + их margins.
    // InlineSpace — collapsed whitespace gap; его ширина = char_width(' ').
    // Остальные боксы (Block, Image и т.д.) — вертикальный поток: берём max.
    let content_w = if is_row_flex_container(b) {
        // Row flex container: items are laid side by side (see
        // `flex_row_intrinsic_sum`). A child with no preference of its own
        // contributes 0, matching the `unwrap_or(0.0)` used for the other
        // horizontal flow below.
        flex_row_intrinsic_sum(b, viewport, &|c| {
            preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0)
        })
    } else if matches!(b.kind, BoxKind::InlineBlockRow) {
        let sum: f32 = b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
            if matches!(c.kind, BoxKind::InlineSpace) {
                // Учитываем ширину collapsed space, чтобы при shrink-to-fit
                // не занижать ширину контейнера и не вызывать перенос соседних
                // inline-block элементов на следующую строку.
                return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
            }
            let cw = preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0);
            let cem = c.style.font_size;
            let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
            let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
            cw + ml + mr
        }).sum();
        sum
    } else {
        // Vertical (block) flow: in-flow children stack, so the container is as
        // wide as its widest child. Floated children, however, are placed side
        // by side on the same line (CSS 2.1 §9.5.1) — their margin-box widths
        // sum. The shrink-to-fit width is the larger of the two contributions.
        let mut inflow_max = 0.0_f32;
        let mut float_sum = 0.0_f32;
        for c in &b.children {
            if !contributes_to_intrinsic_width(c) {
                continue;
            }
            let Some(cw) = preferred_inline_block_width(c, measurer, viewport) else {
                continue;
            };
            if c.style.float_side != FloatSide::None {
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                float_sum += cw + ml.max(0.0) + mr.max(0.0);
            } else {
                inflow_max = inflow_max.max(cw);
            }
        }
        inflow_max.max(float_sum)
    };
    if content_w > 0.0 {
        Some(
            (content_w + pl + pr
                + s.border_left_width + s.border_right_width)
                .max(0.0),
        )
    } else {
        None
    }
}

/// CSS Intrinsic Sizing L3 §4 — max-content border-box width of `b`.
///
/// The max-content width is the width a box would use if line breaking were
/// suppressed: all content on one line. For block containers this is the
/// maximum over children's max-content widths. For `InlineRun` boxes it is
/// the sum of all segment text widths (no wrapping). Includes the box's own
/// padding + border in the returned value (border-box width).
///
/// Phase-0 approximation: only `char_width` per-character measurement is
/// available; inter-word spacing is included, but features like ligatures or
/// kerning are not. Word-break is not applied — text is treated as one run.
fn max_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Explicit non-intrinsic CSS width takes precedence (same logic as
    // preferred_inline_block_width). A percentage width is *not* explicit here:
    // it is unresolvable in an intrinsic context and behaves as `auto`
    // (CSS Sizing L3 §5.2.1, BUG-742) — hence `percent_basis: None`.
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // max-content = all segments on one line (no wrapping).
            measurer.map_or(0.0, |m| {
                segments.iter().map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let ts = seg.style.tab_size
                        * m.char_width_with_families(' ', seg.style.font_size, fams);
                    measure_text_w_families(&seg.text, seg.style.font_size, ls, ts, fams, m)
                }).sum()
            })
        }
        BoxKind::InlineBlockRow => {
            b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
                }
                let cw = max_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).sum()
        }
        // Row flex container: items sit side by side, so max-content is their
        // sum + gaps (CSS Flexbox §9.9). This holds for `flex-wrap: wrap` too —
        // max-content suppresses line breaking, so every item stays on one line.
        _ if is_row_flex_container(b) => {
            flex_row_intrinsic_sum(b, viewport, &|c| {
                max_content_outer_width(c, measurer, viewport)
            })
        }
        _ => {
            // Block container: in-flow children stack vertically → take the
            // widest. Floated children are laid side by side on one line
            // (CSS 2.1 §9.5.1), so their margin-box widths sum. The max-content
            // width is the larger of the in-flow maximum and the float run sum.
            let mut inflow_max = 0.0_f32;
            let mut float_sum = 0.0_f32;
            for c in &b.children {
                if !contributes_to_intrinsic_width(c) {
                    continue;
                }
                let cw = max_content_outer_width(c, measurer, viewport);
                if c.style.float_side != FloatSide::None {
                    let cem = c.style.font_size;
                    let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                    let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                    float_sum += cw + ml.max(0.0) + mr.max(0.0);
                } else {
                    inflow_max = inflow_max.max(cw);
                }
            }
            inflow_max.max(float_sum)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Intrinsic Sizing L3 §4 — min-content border-box width of `b`.
///
/// The min-content width is the narrowest a box can be without overflowing:
/// the width of the longest unbreakable content unit (word, image, etc.).
///
/// Phase-0 approximation: computes the max word width per `InlineRun` by
/// splitting on ASCII whitespace. This gives correct results for Latin text
/// but may overestimate for languages without whitespace-based word breaks.
fn min_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Percentage width behaves as `auto` here — see [`max_content_outer_width`]
    // (CSS Sizing L3 §5.2.1, BUG-742).
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    min_content_outer_width_of_contents(b, measurer, viewport)
}

/// Same as [`min_content_outer_width`] but ignoring `b`'s own definite `width`:
/// the min-content width the box would have if it were sized by its contents.
///
/// This is the CSS Flexbox §4.5 *content size suggestion*, which is deliberately
/// intrinsic — a flex item with `width: 300px` whose contents can collapse to
/// nothing still has a content size suggestion of 0, and so may be shrunk below
/// its preferred width. Descendants keep their own explicit widths; only the
/// box's own preferred size is bypassed.
fn min_content_outer_width_of_contents(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // min-content = widest unbreakable stretch of text.
            //
            // A space is a soft-wrap opportunity only where the segment's own
            // `white-space`/`text-wrap-mode` permits wrapping. Under `nowrap`
            // (and `pre`) there are none, so the stretch runs to the end of the
            // segment — and on to the next segment, since nothing between two
            // adjacent non-wrapping segments can break either. Splitting such
            // text on spaces anyway reported the widest *word* as the whole
            // run's minimum, which is what let a row of `white-space: nowrap`
            // flex items shrink far below their text and paint over each other
            // (BUG-427, dzen.ru topic tabs: "Москва — город будущего" claimed
            // the width of "будущего").
            //
            // `pre` still breaks at preserved newlines, so its stretches are the
            // segment's `\n`-separated lines rather than the whole segment.
            measurer.map_or(0.0, |m| {
                let mut best = 0.0_f32;
                let mut run = 0.0_f32;
                for seg in segments {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let fs = seg.style.font_size;
                    let ts = seg.style.tab_size * m.char_width_with_families(' ', fs, fams);
                    let piece =
                        |t: &str| measure_text_w_families(t, fs, ls, ts, fams, m);
                    let no_wrap = seg.style.white_space.is_nowrap()
                        || seg.style.text_wrap_mode == TextWrapMode::Nowrap;
                    if no_wrap {
                        let mut lines = seg.text.split('\n');
                        // The first line continues the stretch built so far.
                        if let Some(first) = lines.next() {
                            run += piece(first);
                            best = best.max(run);
                        }
                        for line in lines {
                            run = piece(line);
                            best = best.max(run);
                        }
                    } else {
                        // Wrappable: every space is a break opportunity, so the
                        // longest word bounds the minimum (a leading word could
                        // extend the previous stretch — deliberately not modelled,
                        // as before).
                        run = 0.0;
                        for word in seg.text.split_whitespace() {
                            best = best.max(piece(word));
                        }
                    }
                }
                best
            })
        }
        BoxKind::InlineBlockRow => {
            // For inline-block row, min-content is the max over children.
            b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return 0.0; // spaces are breakable
                }
                let cw = min_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).fold(0.0_f32, f32::max)
        }
        // Row flex container with `flex-wrap: nowrap`: the items cannot be
        // pushed onto separate lines, so the narrowest the container can get is
        // the sum of its items' min-content widths + gaps (CSS Flexbox §9.9).
        // With `wrap` the items *can* break onto their own lines, so the
        // min-content width is the widest single item — the block rule below.
        _ if is_row_flex_container(b) && matches!(b.style.flex_wrap, FlexWrap::Nowrap) => {
            flex_row_intrinsic_sum(b, viewport, &|c| {
                min_content_outer_width(c, measurer, viewport)
            })
        }
        _ => {
            b.children.iter()
                .filter(|c| contributes_to_intrinsic_width(c))
                .map(|c| min_content_outer_width(c, measurer, viewport))
                .fold(0.0_f32, f32::max)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Flexbox L1 §9.2/§9.7 — flex base size (main-axis, **border-box**) of a
/// row-direction flex item whose `flex-basis` is `auto`/`content` and which has
/// no explicit `width`. This is the item's max-content width clamped by its own
/// `min-width` / `max-width`. Margins are excluded (the caller adds them).
/// `cb` is the flex container's inner main size, used to resolve percentage
/// min/max-width. Replaces the old approximation that fell back to the
/// preliminary-pass stretched `item.rect.width` for text-only items (BUG-179).
/// Потолок главной оси флекс-элемента во ВНЕШНИХ величинах (граничная рамка
/// плюс поля) — `f32::INFINITY`, если максимум не задан или не разрешается в
/// длину.
///
/// Нужен шагу «fix min/max violations» (CSS Flexbox §9.7 шаг 4): растущий
/// элемент обязан замереть на своём `max-width`/`max-height`, а не забирать
/// всё свободное место строки. Величина внешняя, потому что гипотетические
/// главные размеры в `lay_out_flex` тоже внешние.
fn flex_item_max_main_outer(item: &LayoutBox, cb: f32, viewport: Size, is_column: bool) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let max_len = if is_column { s.max_height.as_ref() } else { s.max_width.as_ref() };
    let Some(max_len) = max_len else {
        return f32::INFINITY;
    };
    // Внутренние ключевые слова (`max-content` и родня) здесь не ограничивают:
    // их разрешение требует измерения содержимого, а промах в бо́льшую сторону
    // безопаснее, чем ложная заморозка элемента.
    if max_len.is_intrinsic() {
        return f32::INFINITY;
    }
    let Some(v) = max_len.resolve(em, Some(cb), viewport) else {
        return f32::INFINITY;
    };
    let (p_start, p_end, b_start, b_end) = if is_column {
        (
            s.padding_top.resolve_or_zero(em, cb, viewport),
            s.padding_bottom.resolve_or_zero(em, cb, viewport),
            s.border_top_width,
            s.border_bottom_width,
        )
    } else {
        (
            s.padding_left.resolve_or_zero(em, cb, viewport),
            s.padding_right.resolve_or_zero(em, cb, viewport),
            s.border_left_width,
            s.border_right_width,
        )
    };
    let border_box = match s.box_sizing {
        BoxSizing::ContentBox => v + p_start + p_end + b_start + b_end,
        BoxSizing::BorderBox => v,
    };
    let (m_start, m_end) = if is_column {
        (
            s.margin_top.resolve_or_zero(em, cb, viewport),
            s.margin_bottom.resolve_or_zero(em, cb, viewport),
        )
    } else {
        (
            s.margin_left.resolve_or_zero(em, cb, viewport),
            s.margin_right.resolve_or_zero(em, cb, viewport),
        )
    };
    (border_box + m_start + m_end).max(0.0)
}

fn flex_auto_base_main_width(
    item: &LayoutBox,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, cb, viewport);
    let pr = s.padding_right.resolve_or_zero(em, cb, viewport);
    // content-box → border-box conversion for a resolved min/max length.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + pl + pr + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    let mut base = max_content_outer_width(item, measurer, viewport);
    if let Some(max_len) = &s.max_width {
        let max_bb = if max_len.is_intrinsic() {
            Some(max_content_outer_width(item, measurer, viewport))
        } else {
            max_len
                .resolve(em, Some(cb), viewport)
                .map(|v| outer_horiz(v).max(0.0))
        };
        if let Some(m) = max_bb {
            base = base.min(m);
        }
    }
    if let Some(min_len) = &s.min_width {
        let min_bb = if min_len.is_intrinsic() {
            Some(min_content_outer_width(item, measurer, viewport))
        } else {
            min_len
                .resolve(em, Some(cb), viewport)
                .map(|v| outer_horiz(v.max(0.0)))
        };
        if let Some(m) = min_bb {
            base = base.max(m);
        }
    }
    base.max(0.0)
}

/// CSS Flexbox L1 §4.5 — automatic minimum size (main axis, **border-box**) of a
/// row-direction flex item. This is the floor below which the item may not be
/// shrunk by `flex-shrink` (§9.7 step 4). Margins are excluded (the caller adds
/// them).
///
/// * An explicit `min-width` always wins — it is simply resolved (an intrinsic
///   keyword resolves against the item's own min-content width).
/// * `min-width: auto` (the initial value, stored as `None`) means the
///   *content-based minimum size*: the smaller of the item's *content size
///   suggestion* (the min-content width of its **contents** — see
///   [`min_content_outer_width_of_contents`]) and its *specified size
///   suggestion* (its own definite `width`, when it has one), capped by a
///   definite `max-width`. Taking the smaller of the two is what keeps an item
///   whose contents can collapse — e.g. one holding only a `width: 100%` child —
///   shrinkable below its own preferred width.
///   It applies only while the main-axis overflow is `visible`; a scroll
///   container has no content-based minimum and may shrink to zero.
///
/// `cb` is the flex container's inner main size, used to resolve percentages.
fn flex_item_min_main_width(
    item: &LayoutBox,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, cb, viewport);
    let pr = s.padding_right.resolve_or_zero(em, cb, viewport);
    // content-box → border-box conversion for a resolved min/max length.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + pl + pr + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(min_len) = &s.min_width {
        let v = if min_len.is_intrinsic() {
            min_content_outer_width(item, measurer, viewport)
        } else {
            min_len
                .resolve(em, Some(cb), viewport)
                .map_or(0.0, |v| outer_horiz(v.max(0.0)))
        };
        return v.max(0.0);
    }
    if s.overflow_x != Overflow::Visible {
        return 0.0;
    }
    let mut floor = min_content_outer_width_of_contents(item, measurer, viewport);
    // Specified size suggestion — the item's own definite preferred main size.
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, Some(cb), viewport)
    {
        floor = floor.min(outer_horiz(w).max(0.0));
    }
    if let Some(max_len) = &s.max_width
        && !max_len.is_intrinsic()
        && let Some(v) = max_len.resolve(em, Some(cb), viewport)
    {
        floor = floor.min(outer_horiz(v).max(0.0));
    }
    floor.max(0.0)
}

/// Рекурсивно смещает rect.y всего поддерева на dy (для vertical-align).
///
/// BUG-424 (в): `svg_paint_matrix` (document-space CTM for rotated/skewed SVG
/// shapes, `lay_out_svg_element_position`) bakes in the viewport origin at the
/// time it was computed. When flex/grid cross-axis alignment (`AlignValue::
/// Center`/`End` in `lay_out_flex`) relocates an already-laid-out SVG subtree
/// by patching `rect.y` instead of re-running SVG layout, the matrix used to
/// silently keep the stale origin — `rect` (used by the axis-aligned fast
/// path) moved, the CTM (used only when `has_rot_skew`) did not, drifting the
/// two out of sync by exactly this shift. Translating the matrix in lockstep
/// keeps both representations of the same box consistent.
fn shift_y_box(b: &mut LayoutBox, dy: f32) {
    b.rect.y += dy;
    if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut b.kind {
        svg_paint_matrix.matrix[5] += dy;
    }
    for child in &mut b.children {
        shift_y_box(child, dy);
    }
}

/// Рекурсивно смещает rect всего поддерева на (dx, dy).
/// Используется при позиционировании абсолютных потомков.
///
/// BUG-424 (в): keeps `svg_paint_matrix` in sync with `rect` — see
/// `shift_y_box` for why this matters.
fn shift_tree(b: &mut LayoutBox, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    b.rect.x += dx;
    b.rect.y += dy;
    if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut b.kind {
        svg_paint_matrix.matrix[4] += dx;
        svg_paint_matrix.matrix[5] += dy;
    }
    for child in &mut b.children {
        shift_tree(child, dx, dy);
    }
}

// ─── CSS 2.1 §9.5 — Float context ────────────────────────────────────────────

/// CSS Shapes L1 §5.1 — parse `circle(<length-px>)` from a raw shape string.
/// Returns the radius in px. Only handles `circle(Npx)` without `at` clause.
/// Returns `None` for any unrecognised syntax (fallback to rectangular float).
pub(crate) fn parse_circle_px(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("circle(")?.strip_suffix(')')?;
    let token = inner.split_whitespace().next()?;
    // Accept "50px" or bare "50" (assume px).
    let digits = token.strip_suffix("px").unwrap_or(token);
    digits.parse::<f32>().ok().filter(|&r| r > 0.0)
}

/// CSS Shapes L1 §5.2 — parse `polygon([<fill-rule>,] x1 y1, x2 y2, ...)`.
/// Returns vertex list in float-local (margin-box-relative) px coordinates.
/// Accepts `Npx` or bare `N` (assumed px). Returns `None` for any unknown syntax.
pub(crate) fn parse_shape_polygon_px(s: &str) -> Option<Vec<(f32, f32)>> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("polygon(")?.strip_suffix(')')?;
    // Strip optional fill-rule keyword (nonzero | evenodd).
    let coords_str = if inner.trim_start().starts_with("nonzero")
        || inner.trim_start().starts_with("evenodd")
    {
        inner.split_once(',').map(|x| x.1).unwrap_or("")
    } else {
        inner
    };
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for pair in coords_str.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.split_whitespace();
        let xs = it.next()?;
        let ys = it.next()?;
        let x = xs.strip_suffix("px").unwrap_or(xs).parse::<f32>().ok()?;
        let y = ys.strip_suffix("px").unwrap_or(ys).parse::<f32>().ok()?;
        pts.push((x, y));
    }
    if pts.len() >= 3 { Some(pts) } else { None }
}

/// CSS Shapes L1 §5.2 — parse `ellipse(<rx> <ry> at <cx> <cy>)`.
/// Returns `(rx, ry, cx, cy)` in float-local (margin-box-relative) px coords.
/// Returns `None` for any unknown syntax or zero/negative radii.
pub(crate) fn parse_shape_ellipse_px(s: &str) -> Option<(f32, f32, f32, f32)> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("ellipse(")?.strip_suffix(')')?;
    // Expected: "rxpx rypx at cxpx cypx"
    let at_pos = inner.find(" at ")?;
    let radii_part = inner[..at_pos].trim();
    let center_part = inner[at_pos + 4..].trim();
    let mut ri = radii_part.split_whitespace();
    let mut ci = center_part.split_whitespace();
    let rxs = ri.next()?;
    let rys = ri.next()?;
    let cxs = ci.next()?;
    let cys = ci.next()?;
    let rx = rxs.strip_suffix("px").unwrap_or(rxs).parse::<f32>().ok()?;
    let ry = rys.strip_suffix("px").unwrap_or(rys).parse::<f32>().ok()?;
    let cx = cxs.strip_suffix("px").unwrap_or(cxs).parse::<f32>().ok()?;
    let cy = cys.strip_suffix("px").unwrap_or(cys).parse::<f32>().ok()?;
    if rx > 0.0 && ry > 0.0 { Some((rx, ry, cx, cy)) } else { None }
}

/// CSS Shapes L1 §5.1 — parse `inset(<top> <right> <bottom> <left> [round <r>])`.
/// Returns `(top, right, bottom, left, radius)` insets in px from the reference
/// box edges, plus a single uniform corner radius (`0` = sharp corners).
/// Lengths follow the margin-shorthand expansion (1–4 values). The optional
/// `round` clause keeps only the first radius value (elliptical radii collapse
/// to their horizontal component). Returns `None` for any unknown syntax.
pub(crate) fn parse_shape_inset_px(s: &str) -> Option<(f32, f32, f32, f32, f32)> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("inset(")?.strip_suffix(')')?;
    // Split off the optional `round <border-radius>` clause.
    let (lens_part, radius) = match inner.split_once(" round ") {
        Some((l, r)) => {
            let rstr = r.split_whitespace().next()?;
            let rad = rstr
                .strip_suffix("px")
                .unwrap_or(rstr)
                .parse::<f32>()
                .ok()
                .filter(|v| *v >= 0.0)?;
            (l, rad)
        }
        None => (inner, 0.0),
    };
    let mut vals: Vec<f32> = Vec::new();
    for tok in lens_part.split_whitespace() {
        let v = tok.strip_suffix("px").unwrap_or(tok).parse::<f32>().ok()?;
        vals.push(v);
    }
    let (t, r, b, l) = match vals.len() {
        1 => (vals[0], vals[0], vals[0], vals[0]),
        2 => (vals[0], vals[1], vals[0], vals[1]),
        3 => (vals[0], vals[1], vals[2], vals[1]),
        4 => (vals[0], vals[1], vals[2], vals[3]),
        _ => return None,
    };
    Some((t, r, b, l, radius))
}

/// CSS Shapes L1 §4 — parse `path([<fill-rule>,]? "<svg-path>")`.
/// Flattens the SVG path `d` string into a vertex list in float-local
/// (reference-box-relative) px coordinates via [`crate::motion_path::flatten_path_to_polygon`].
/// The optional `<fill-rule>` (nonzero | evenodd) is accepted but ignored — float
/// wrapping uses the filled outline regardless. The `d` string must be quoted
/// (`"…"` or `'…'`); its letter case is preserved (SVG commands are case-sensitive).
/// `path()` coordinates are always px (no percentages per spec). Returns `None`
/// for any unknown syntax or a degenerate (< 3 vertices) outline.
pub(crate) fn parse_shape_path_px(s: &str) -> Option<Vec<(f32, f32)>> {
    let s = s.trim();
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open {
        return None;
    }
    // Only the function name is case-folded; the inner `d` string keeps its case.
    if !s[..open].trim().eq_ignore_ascii_case("path") {
        return None;
    }
    let inner = s[open + 1..close].trim();
    // Strip an optional leading `<fill-rule>,` (ignored for wrapping geometry).
    let inner = match inner.split_once(',') {
        Some((head, rest))
            if head.trim().eq_ignore_ascii_case("nonzero")
                || head.trim().eq_ignore_ascii_case("evenodd") =>
        {
            rest.trim()
        }
        _ => inner,
    };
    let path_str = inner
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| inner.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))?;
    let pts = crate::motion_path::flatten_path_to_polygon(path_str);
    if pts.len() >= 3 { Some(pts) } else { None }
}

/// CSS Shapes L1 §5.2 — polygon shape for `shape-outside` on a float.
/// Points are stored in content-area coordinates (same as FloatContext).
#[derive(Clone)]
struct ShapePolygon {
    top_y: f32,
    bottom_y: f32,
    /// `true` = left float, `false` = right float.
    is_left: bool,
    /// Polygon vertices in content-area coordinates.
    points: Vec<(f32, f32)>,
}

/// CSS Shapes L1 §5.2 — ellipse shape for `shape-outside` on a float.
/// All coordinates are in content-area space (same as FloatContext).
#[derive(Clone)]
struct ShapeEllipse {
    top_y: f32,
    bottom_y: f32,
    /// `true` = left float, `false` = right float.
    is_left: bool,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
}

/// CSS Shapes L1 §5.1 — `inset()` rectangle shape for `shape-outside` on a float.
/// All coordinates are in content-area space (same as FloatContext). The rectangle
/// spans `[left_x, right_x] × [top_y, bottom_y]` with optional uniform corner
/// rounding of `radius` px.
#[derive(Clone)]
struct ShapeInset {
    top_y: f32,
    bottom_y: f32,
    /// `true` = left float, `false` = right float.
    is_left: bool,
    left_x: f32,
    right_x: f32,
    /// Uniform corner radius in px (`0` = sharp corners).
    radius: f32,
}

/// CSS Shapes L1 §5.1 — horizontal inward offset of a rounded `inset()` corner
/// at scanline `y`. Returns `0` outside the corner bands or for a `0` radius.
/// Within `radius` px of the top/bottom edge the boundary follows a quarter
/// circle, so the inline edge recedes by `radius − √(radius² − dy²)`.
fn inset_corner_inward(y: f32, top_y: f32, bottom_y: f32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return 0.0;
    }
    let top_band = top_y + radius;
    let bot_band = bottom_y - radius;
    let dy = if y < top_band {
        top_band - y
    } else if y > bot_band {
        y - bot_band
    } else {
        return 0.0;
    };
    let dy = dy.min(radius);
    radius - (radius * radius - dy * dy).max(0.0).sqrt()
}

/// CSS 2.1 §9.5 — tracks float placements within a single block formatting
/// context.  Simplified Phase-0 implementation: only axis-aligned rectangles,
/// no shape-outside wrapping.  All coordinates are in the same space as the
/// block container's content area (i.e. not relative to viewport).
#[derive(Clone)]
struct FloatContext {
    /// Left floats: `(bottom_y, right_edge)` — right edge of the float margin
    /// box in content-area coordinates.  Active while `bottom_y > query_y`.
    left: Vec<(f32, f32)>,
    /// Right floats: `(bottom_y, left_edge)` — left edge of the float margin
    /// box.  Active while `bottom_y > query_y`.
    right: Vec<(f32, f32)>,
    /// CSS Shapes L1 — `shape-outside: circle(r)` overrides.
    /// `(top_y, bottom_y, is_left, center_x, center_y, radius)`.
    /// `is_left=true` → left float, `false` → right float.
    shape_circles: Vec<(f32, f32, bool, f32, f32, f32)>,
    /// CSS Shapes L1 — `shape-outside: polygon(...)` overrides.
    shape_polygons: Vec<ShapePolygon>,
    /// CSS Shapes L1 — `shape-outside: ellipse(...)` overrides.
    shape_ellipses: Vec<ShapeEllipse>,
    /// CSS Shapes L1 — `shape-outside: inset(...)` overrides.
    shape_insets: Vec<ShapeInset>,
    /// CSS 2.1 §9.5 — floats belonging to an *enclosing* block formatting
    /// context, inherited by a non-BFC child so its line boxes are shortened by
    /// the parent's floats (the child does not own them: they are excluded from
    /// this context's height enclosure and float placement). Coordinates are
    /// absolute (same space as the owned floats). Chains through nesting levels.
    inherited: Option<Box<FloatContext>>,
}

impl FloatContext {
    fn new() -> Self {
        Self {
            left: Vec::new(),
            right: Vec::new(),
            shape_circles: Vec::new(),
            shape_polygons: Vec::new(),
            shape_ellipses: Vec::new(),
            shape_insets: Vec::new(),
            inherited: None,
        }
    }

    /// CSS 2.1 §9.5 — a fresh context for a non-BFC child that inherits all
    /// floats currently visible in `parent` (the parent's own floats *and* any
    /// the parent itself inherited). The child adds its own floats to the empty
    /// owned buckets; queries (`left_edge_at`/`clear_y`/…) see both via the
    /// `inherited` chain. Coordinates are absolute, so no translation is needed.
    fn inheriting(parent: &FloatContext) -> Self {
        let mut c = Self::new();
        c.inherited = Some(Box::new(parent.clone()));
        c
    }

    /// Left boundary of available inline space at `y` (= rightmost right-edge
    /// of all left floats whose `bottom_y > y`).  Falls back to `default_x`.
    fn left_edge_at(&self, y: f32, default_x: f32) -> f32 {
        let rect_edge = self.left
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, r)| *r)
            .fold(default_x, f32::max);
        // CSS Shapes L1: circle boundary.
        let after_circles = self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| *is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx + hw
            })
            .fold(rect_edge, f32::max);
        // CSS Shapes L1: polygon boundary (rightmost edge at y).
        let after_polygons = self.shape_polygons
            .iter()
            .filter(|p| p.is_left && p.top_y <= y && p.bottom_y > y)
            .filter_map(|p| polygon_right_edge_at_y(&p.points, y))
            .fold(after_circles, f32::max);
        // CSS Shapes L1: ellipse boundary (right edge at y).
        let after_ellipses = self.shape_ellipses
            .iter()
            .filter(|e| e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some(e.cx + e.rx * (1.0 - norm * norm).max(0.0).sqrt())
            })
            .fold(after_polygons, f32::max);
        // CSS Shapes L1: inset() boundary (right edge at y, minus rounded corner).
        let own = self.shape_insets
            .iter()
            .filter(|s| s.is_left && s.top_y <= y && s.bottom_y > y)
            .map(|s| s.right_x - inset_corner_inward(y, s.top_y, s.bottom_y, s.radius))
            .fold(after_ellipses, f32::max);
        // CSS 2.1 §9.5: enclosing-context floats also push the left edge right.
        match &self.inherited {
            Some(p) => p.left_edge_at(y, own),
            None => own,
        }
    }

    /// Right boundary of available inline space at `y` (= leftmost left-edge
    /// of all right floats whose `bottom_y > y`).  Falls back to `default_x`.
    fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        let rect_edge = self.right
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, l)| *l)
            .fold(default_x, f32::min);
        // CSS Shapes L1: circle boundary.
        let after_circles = self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| !is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx - hw
            })
            .fold(rect_edge, f32::min);
        // CSS Shapes L1: polygon boundary (leftmost edge at y).
        let after_polygons = self.shape_polygons
            .iter()
            .filter(|p| !p.is_left && p.top_y <= y && p.bottom_y > y)
            .filter_map(|p| polygon_left_edge_at_y(&p.points, y))
            .fold(after_circles, f32::min);
        // CSS Shapes L1: ellipse boundary (left edge at y).
        let after_ellipses = self.shape_ellipses
            .iter()
            .filter(|e| !e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some(e.cx - e.rx * (1.0 - norm * norm).max(0.0).sqrt())
            })
            .fold(after_polygons, f32::min);
        // CSS Shapes L1: inset() boundary (left edge at y, plus rounded corner).
        let own = self.shape_insets
            .iter()
            .filter(|s| !s.is_left && s.top_y <= y && s.bottom_y > y)
            .map(|s| s.left_x + inset_corner_inward(y, s.top_y, s.bottom_y, s.radius))
            .fold(after_ellipses, f32::min);
        // CSS 2.1 §9.5: enclosing-context floats also pull the right edge left.
        match &self.inherited {
            Some(p) => p.right_edge_at(y, own),
            None => own,
        }
    }

    /// Record a left float occupying `[y_top, bottom_y)` with right margin
    /// edge at `right_edge`.
    fn add_left(&mut self, bottom_y: f32, right_edge: f32) {
        self.left.push((bottom_y, right_edge));
    }

    /// Record a right float occupying `[y_top, bottom_y)` with left margin
    /// edge at `left_edge`.
    fn add_right(&mut self, bottom_y: f32, left_edge: f32) {
        self.right.push((bottom_y, left_edge));
    }

    /// CSS 2.1 §9.5.2 — advance `y` past all floats on the given side.
    fn clear_y(&self, y: f32, side: ClearSide) -> f32 {
        let mut result = y;
        let do_left  = matches!(side, ClearSide::Left  | ClearSide::Both);
        let do_right = matches!(side, ClearSide::Right | ClearSide::Both);
        if do_left  { for (bot, _) in &self.left  { result = result.max(*bot); } }
        if do_right { for (bot, _) in &self.right { result = result.max(*bot); } }
        // CSS 2.1 §9.5.2: `clear` on a nested block clears the enclosing
        // context's floats too (their bottoms are absolute, like ours).
        match &self.inherited {
            Some(p) => p.clear_y(result, side),
            None => result,
        }
    }

    /// True when there are no active floats at all (owned or inherited).
    fn is_empty(&self) -> bool {
        self.left.is_empty()
            && self.right.is_empty()
            && self.inherited.as_ref().is_none_or(|p| p.is_empty())
    }

    /// CSS 2.1 §9.5.1 rule 8 — the smallest float bottom strictly below `y`
    /// across both sides. A float that does not fit beside the current floats
    /// drops to the next such bottom, where the line widens. Returns `None`
    /// when no float ends below `y` (nothing left to clear).
    fn next_float_bottom(&self, y: f32) -> Option<f32> {
        let own = self.left.iter().chain(self.right.iter())
            .map(|(bot, _)| *bot)
            .filter(|bot| *bot > y + 0.01)
            .fold(None, |acc, bot| Some(acc.map_or(bot, |a: f32| a.min(bot))));
        // CSS 2.1 §9.5.1 rule 8: enclosing-context floats also widen the band.
        let inh = self.inherited.as_ref().and_then(|p| p.next_float_bottom(y));
        match (own, inh) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// CSS Shapes L1 §4 — rightmost x of polygon boundary at scanline `y`.
/// Scans all edges that cross `y`; returns `None` if no edge crosses.
fn polygon_right_edge_at_y(pts: &[(f32, f32)], y: f32) -> Option<f32> {
    polygon_edge_x_at_y(pts, y, true)
}

/// CSS Shapes L1 §4 — leftmost x of polygon boundary at scanline `y`.
fn polygon_left_edge_at_y(pts: &[(f32, f32)], y: f32) -> Option<f32> {
    polygon_edge_x_at_y(pts, y, false)
}

/// Shared kernel: iterate polygon edges, return rightmost (want_max=true) or
/// leftmost (want_max=false) x intersection with horizontal scanline at `y`.
fn polygon_edge_x_at_y(pts: &[(f32, f32)], y: f32, want_max: bool) -> Option<f32> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let mut best: Option<f32> = None;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        // Edge crosses y iff exactly one endpoint is strictly below y.
        // Use half-open interval [min, max) to avoid double-counting vertices.
        if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
            let x_at_y = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
            best = Some(match best {
                None => x_at_y,
                Some(prev) => if want_max { prev.max(x_at_y) } else { prev.min(x_at_y) },
            });
        }
    }
    best
}

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

