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
                // HTML LS §4.12.4: width/height content attributes are
                // non-negative integers; defaults are 300×150 CSS px.
                let cw = node
                    .get_attr("width")
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .unwrap_or(300);
                let ch = node
                    .get_attr("height")
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .unwrap_or(150);
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

/// Crate-internal shim so `vertical.rs` can recursively invoke the main
/// `lay_out` for children inside a vertical writing-mode container.
///
/// Same parameters and semantics as the private `lay_out`. Exists only
/// because Rust modules cannot reach a sibling module's private functions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_for_vertical(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    lay_out(b, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp, false);
}

/// CSS 2.1 §9.4.1 — does this box establish a new Block Formatting Context?
///
/// A BFC root does NOT collapse its margins with its in-flow children
/// (CSS 2.1 §8.3.1). Within the block-layout arm a box is always `Block` or
/// `FlowRoot`; the remaining BFC triggers detectable from the box alone are a
/// non-`visible` overflow, a float, and out-of-flow positioning. (Being a flex
/// / grid item also establishes an independent FC, but that depends on the
/// parent and is signalled separately via `lay_out`'s `in_block_flow` flag.)
fn establishes_bfc(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::FlowRoot)
        || b.style.overflow_x != Overflow::Visible
        || b.style.overflow_y != Overflow::Visible
        || b.style.float_side != FloatSide::None
        || matches!(b.style.position, Position::Absolute | Position::Fixed)
}

/// True if the box has any in-flow child that produces content (i.e. a child
/// that is not a float, out-of-flow box, `::marker`, or zero-height `Skip`).
///
/// CSS 2.1 §9.5: a block-level box beside a float keeps full containing-block
/// width while only its *line boxes* are shortened. Lumen cannot yet shorten
/// line boxes inside a child block (floats are not propagated into nested
/// layout), so it approximates the narrowing by clipping the box itself. That
/// clip is only geometrically faithful when the box has no in-flow content to
/// reflow — this predicate gates the full-width path to such boxes (e.g. an
/// empty `<div>` background sitting in the gap between two floats).
fn has_in_flow_content(b: &LayoutBox) -> bool {
    b.children.iter().any(|c| {
        !matches!(c.kind, BoxKind::Skip | BoxKind::Marker { .. })
            && c.style.float_side == FloatSide::None
            && !matches!(c.style.position, Position::Absolute | Position::Fixed)
    })
}

/// Returns the first in-flow `Block` child whose top margin collapses with the
/// owning box's top margin (CSS 2.1 §8.3.1). Out-of-flow children (floats,
/// absolutely positioned), `::marker`s and `Skip` boxes are transparent and
/// skipped. If the first remaining in-flow child is not a plain `Block` (e.g.
/// an inline run or a replaced element) the collapsing chain is broken and
/// `None` is returned. A child with clearance also breaks the chain.
fn first_collapsible_child(b: &LayoutBox) -> Option<&LayoutBox> {
    for child in &b.children {
        if matches!(child.kind, BoxKind::Marker { .. } | BoxKind::Skip) {
            continue;
        }
        if child.style.float_side != FloatSide::None
            || matches!(child.style.position, Position::Absolute | Position::Fixed)
        {
            continue;
        }
        if child.style.clear != ClearSide::None {
            return None;
        }
        return matches!(child.kind, BoxKind::Block).then_some(child);
    }
    None
}

/// CSS 2.1 §8.3.1 — the *collapsed* top margin of a block-level box (px).
///
/// The top margin of an in-flow block collapses with the top margin of its
/// first in-flow block-level child when nothing separates them: the box has no
/// top border, no top padding, establishes no BFC, and the first in-flow child
/// is itself a plain block with no clearance. The collapse recurses down the
/// chain of first children. `cb` is the containing-block width used to resolve
/// percentage margins. Only the common non-negative case is folded (parity with
/// sibling collapse); negative margins fall through as the box's own margin.
fn collapsed_top_margin(b: &LayoutBox, cb: f32, viewport: Size) -> f32 {
    let em = b.style.font_size;
    let own = b.style.margin_top.resolve_or_zero(em, cb, viewport);
    if !matches!(b.kind, BoxKind::Block) || establishes_bfc(b) {
        return own;
    }
    let pt = b.style.padding_top.resolve_or_zero(em, cb, viewport);
    if pt != 0.0 || b.style.border_top_width != 0.0 {
        return own;
    }
    match first_collapsible_child(b) {
        Some(child) => {
            // Child's containing-block width = this box's content width.
            let child_cb = (cb
                - b.style.padding_left.resolve_or_zero(em, cb, viewport)
                - b.style.padding_right.resolve_or_zero(em, cb, viewport)
                - b.style.border_left_width
                - b.style.border_right_width)
                .max(0.0);
            own.max(collapsed_top_margin(child, child_cb, viewport))
        }
        None => own,
    }
}

/// Returns the last in-flow `Block` child whose bottom margin collapses with the
/// owning box's bottom margin (CSS 2.1 §8.3.1). Mirror of `first_collapsible_child`
/// for the bottom edge: out-of-flow children (floats, absolutely positioned),
/// `::marker`s and zero-height `Skip` boxes are transparent and skipped. If the
/// last remaining in-flow child is not a plain `Block` (e.g. an inline run or a
/// replaced element) the collapsing chain is broken and `None` is returned. A
/// child with clearance also breaks the chain.
fn last_collapsible_child(b: &LayoutBox) -> Option<&LayoutBox> {
    for child in b.children.iter().rev() {
        if matches!(child.kind, BoxKind::Marker { .. } | BoxKind::Skip) {
            continue;
        }
        if child.style.float_side != FloatSide::None
            || matches!(child.style.position, Position::Absolute | Position::Fixed)
        {
            continue;
        }
        if child.style.clear != ClearSide::None {
            return None;
        }
        return matches!(child.kind, BoxKind::Block).then_some(child);
    }
    None
}

/// CSS 2.1 §8.3.1 — the *collapsed* bottom margin of a block-level box (px).
///
/// The bottom margin of an in-flow block collapses with the bottom margin of its
/// last in-flow block-level child when nothing separates them: the box has an
/// `auto` height, no bottom border, no bottom padding, establishes no BFC, and the
/// last in-flow child is itself a plain block with no clearance. The collapse
/// recurses down the chain of last children. `cb` is the containing-block width
/// used to resolve percentage margins. Only the common non-negative case is folded
/// (parity with `collapsed_top_margin`); negative margins fall through as the box's
/// own margin.
fn collapsed_bottom_margin(b: &LayoutBox, cb: f32, viewport: Size) -> f32 {
    let em = b.style.font_size;
    let own = b.style.margin_bottom.resolve_or_zero(em, cb, viewport);
    if !matches!(b.kind, BoxKind::Block) || establishes_bfc(b) {
        return own;
    }
    // A definite height blocks the last child's bottom margin from reaching the
    // box's bottom edge, so the through-collapse does not happen.
    if b.style.height.is_some() {
        return own;
    }
    let pb = b.style.padding_bottom.resolve_or_zero(em, cb, viewport);
    if pb != 0.0 || b.style.border_bottom_width != 0.0 {
        return own;
    }
    match last_collapsible_child(b) {
        Some(child) => {
            // Child's containing-block width = this box's content width.
            let child_cb = (cb
                - b.style.padding_left.resolve_or_zero(em, cb, viewport)
                - b.style.padding_right.resolve_or_zero(em, cb, viewport)
                - b.style.border_left_width
                - b.style.border_right_width)
                .max(0.0);
            own.max(collapsed_bottom_margin(child, child_cb, viewport))
        }
        None => own,
    }
}

/// CSS Box Sizing L4 §5 — content block-size contribution under size containment.
/// When `size_contained` is true the box ignores its children for auto sizing and
/// uses the resolved `contain-intrinsic-height` (content-box px, clamped ≥ 0), or
/// `0.0` when the value is `none`/unset. Otherwise returns the measured
/// `content_height` unchanged.
fn contained_content_height(
    size_contained: bool,
    style: &ComputedStyle,
    em: f32,
    viewport: Size,
    content_height: f32,
) -> f32 {
    if size_contained {
        style
            .contain_intrinsic_height
            .as_ref()
            .and_then(|l| l.resolve(em, None, viewport))
            .map_or(0.0, |v| v.max(0.0))
    } else {
        content_height
    }
}

/// `pcb` — rect positioned containing block (ближайший предок с position != static),
/// используется для layout абсолютно-позиционированных потомков.
///
/// `in_block_flow` — `true` only when this box is laid out as a normal in-flow
/// block child of a block container. It gates parent↔first-child margin
/// collapsing (CSS 2.1 §8.3.1): a box laid out as a flex/grid item, table cell,
/// or document root establishes an independent formatting context and must not
/// collapse its top margin into its first child, so those call sites pass
/// `false`.
#[allow(clippy::too_many_arguments)]
fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
) {
    // Thin wrapper: most call sites lay out boxes that establish an independent
    // formatting context (flex/grid items, table cells, the document root), so
    // they inherit no enclosing floats. The block-flow normal-child recursion in
    // `lay_out_inner` is the one site that propagates a parent `FloatContext`.
    lay_out_cache_checked(
        b, start_x, start_y, available_width, available_height,
        measurer, viewport, pcb, hp, in_block_flow, None,
    );
}

/// Same as [`lay_out`], but resolves `b`'s own used width/height/box-sizing
/// from `used_size_override` instead of from `b.style`'s declared values —
/// see [`UsedSizeOverride`] for why this replaces the old
/// capture-mutate-restore dance around `b.style` (BUG-341 S34).
#[allow(clippy::too_many_arguments)]
fn lay_out_with_used_size(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    used_size_override: UsedSizeOverride,
) {
    lay_out_cache_checked(
        b, start_x, start_y, available_width, available_height,
        measurer, viewport, pcb, hp, in_block_flow, Some(used_size_override),
    );
}

/// BUG-341 S36 — the layout-result cache's one choke point, shared by
/// [`lay_out`] (`used_size_override: None`) and [`lay_out_with_used_size`]
/// (`used_size_override: Some(..)`, `lay_out_flex`'s three re-layout call
/// sites). Both wrappers pass `outer_floats: None, parent_justify_items:
/// Auto` unconditionally into `lay_out_inner` — the block-flow normal-child
/// recursion is the one `lay_out_inner` call site that threads real
/// floats/justify-items and is therefore never intercepted here, same
/// exclusion S32 established.
#[allow(clippy::too_many_arguments)]
fn lay_out_cache_checked(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    used_size_override: Option<UsedSizeOverride>,
) {
    // BUG-802: this wrapper is the one entry point every layout pass starts
    // from, so it is where a pass is delimited for the probe-height memo.
    let _pass = LayoutPassGuard::enter();
    if layout_result_cache_enabled() && cacheable_for_layout_result_cache(b) {
        let key = LayoutResultKey {
            node: b.node,
            width_bits: available_width.to_bits(),
            height_bits: available_height.map(f32::to_bits),
            viewport_w_bits: viewport.width.to_bits(),
            viewport_h_bits: viewport.height.to_bits(),
            pcb_x_bits: pcb.x.to_bits(),
            pcb_y_bits: pcb.y.to_bits(),
            pcb_w_bits: pcb.width.to_bits(),
            pcb_h_bits: pcb.height.to_bits(),
            in_block_flow,
            measurer_ptr: measurer
                .map(|m| m as *const dyn TextMeasurer as *const () as usize)
                .unwrap_or(0),
            hp_ptr: hp as *const dyn HyphenationProvider as *const () as usize,
            used_size_override: UsedSizeOverrideBits::from(used_size_override.as_ref()),
        };
        let hit = LAYOUT_RESULT_CACHE.with(|c| {
            c.borrow().get(&key).and_then(|e| {
                if Arc::ptr_eq(&e.style, &b.style) && crate::incremental::kind_layout_eq(&e.result.kind, &b.kind) {
                    Some((e.result.clone(), e.start_x, e.start_y))
                } else {
                    None
                }
            })
        });
        if let Some((mut result, cached_x, cached_y)) = hit {
            crate::incremental::translate_subtree(&mut result, start_x - cached_x, start_y - cached_y);
            *b = result;
            LAYOUT_RESULT_CACHE_STATS.with(|c| {
                let mut v = c.get();
                v.hits += 1;
                c.set(v);
            });
            return;
        }

        // Cache miss: compute normally, tracking whether the computation
        // touched `content-visibility: auto` anywhere in this subtree (see
        // `CV_AUTO_TOUCHED`'s doc comment).
        let outer_touched = CV_AUTO_TOUCHED.with(|c| c.replace(false));
        lay_out_inner(
            b, start_x, start_y, available_width, available_height,
            measurer, viewport, pcb, hp, in_block_flow, None, AlignValue::Auto,
            used_size_override,
        );
        let touched_here = CV_AUTO_TOUCHED.with(|c| c.get());
        CV_AUTO_TOUCHED.with(|c| c.set(outer_touched || touched_here));
        if !touched_here {
            LAYOUT_RESULT_CACHE.with(|c| {
                c.borrow_mut().insert(
                    key,
                    LayoutResultEntry {
                        style: Arc::clone(&b.style),
                        start_x,
                        start_y,
                        result: b.clone(),
                    },
                );
            });
            LAYOUT_RESULT_CACHE_STATS.with(|c| {
                let mut v = c.get();
                v.misses += 1;
                c.set(v);
            });
        } else {
            LAYOUT_RESULT_CACHE_STATS.with(|c| {
                let mut v = c.get();
                v.poisoned += 1;
                c.set(v);
            });
        }
        return;
    }
    lay_out_inner(
        b, start_x, start_y, available_width, available_height,
        measurer, viewport, pcb, hp, in_block_flow, None, AlignValue::Auto,
        used_size_override,
    );
}

/// CSS 2.1 §9.5 — same as [`lay_out`] but threads `outer_floats`: the float
/// context of an *enclosing* block formatting context, present only when `b` is
/// an in-flow non-BFC block child laid out beside the parent's floats. When set,
/// `b`'s own float context inherits those floats so its (and its descendants')
/// line boxes are shortened by them, instead of the box itself being clipped.
///
/// `parent_justify_items` carries the enclosing block container's `justify-items`
/// value (CSS Box Alignment L3 §6.3), threaded only from the in-flow block-child
/// recursion. When `b`'s own `justify-self` is `auto`, it resolves to this value
/// (the container default); every independent-formatting-context call site passes
/// `AlignValue::Auto`, so those boxes fall back to the inline-start behaviour.
///
/// `used_size_override` — see [`UsedSizeOverride`]; `None` for every call site
/// except `lay_out_with_used_size`'s wrapper (`lay_out_flex`'s re-layout passes).
#[allow(clippy::too_many_arguments)]
fn lay_out_inner(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    // CSS 2.1 §10.5: definite content height of the containing block, or None if auto.
    // None means percentage heights on children compute to 'auto'.
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
    outer_floats: Option<&FloatContext>,
    parent_justify_items: AlignValue,
    used_size_override: Option<UsedSizeOverride>,
) {
    // DEVX-8a: `pcb` is the positioned containing block, threaded as a mandatory
    // parameter through every `lay_out`/`lay_out_inner` call — this is the choke
    // point proving "every box resolves a containing block" without a second
    // tree walk. A non-finite `pcb` means a caller propagated a bad rect (e.g.
    // through an unresolved percentage or a NaN from an earlier pass).
    debug_assert!(
        pcb.x.is_finite() && pcb.y.is_finite() && pcb.width.is_finite() && pcb.height.is_finite(),
        "DEVX-8a: non-finite containing block for node={:?}: pcb={:?}",
        b.node,
        pcb
    );
    if matches!(b.kind, BoxKind::Skip) {
        b.rect = Rect::new(start_x, start_y, 0.0, 0.0);
        return;
    }

    // EE-3: incremental layout — skip clean subtrees entirely.
    // When INCREMENTAL_LAYOUT_MODE is on and the box has no dirty bits, translate
    // the existing rect to the new (start_x, start_y) without re-running layout.
    // The block-children loop in the parent already advanced child_y using the
    // existing height, so the position is consistent across siblings.
    if INCREMENTAL_LAYOUT_MODE.with(|m| m.get()) && b.dirty.is_clean() {
        let _prof = lumen_core::profile::scope_detail("lo_translate");
        crate::incremental::translate_subtree(b, start_x - b.rect.x, start_y - b.rect.y);
        return;
    }

    record_layout_key_occurrence(b.node, available_width, available_height, &b.style, used_size_override.as_ref());

    // CSS Values L4 §5.1.1 — publish this box's real `ch`/`ex` metrics (advance of
    // the "0" glyph and the x-height at the used font-size) so `Length::{Ch,Ex}`
    // resolve against the actual font for this box and its descendants. The guard
    // restores the parent's value on every return path, keeping the thread-local
    // balanced across the recursive layout walk. Without a measurer the context is
    // cleared, so ch/ex fall back to the spec `0.5em` assumption.
    struct ChExGuard(Option<(f32, f32)>);
    impl Drop for ChExGuard {
        fn drop(&mut self) {
            crate::style::pop_ch_ex_context(self.0);
        }
    }
    let _ch_ex_guard = {
        let _prof = lumen_core::profile::scope_detail("lo_chex");
        let ch_ex = measurer.map(|m| {
            let fs = b.style.font_size.max(0.0);
            (
                m.char_width_with_families('0', fs, &b.style.font_family),
                m.x_height_px(fs),
            )
        });
        ChExGuard(crate::style::push_ch_ex_context(ch_ex))
    };

    // CSS Containment L3 §4.4 — content-visibility: auto (BB-4). When the box
    // flow position starts below the expanded viewport and the shell hasn't
    // ratcheted the node relevant, drop the children for this pass: the element
    // keeps its own box and paint emits nothing for the subtree. While skipped,
    // the element is size-contained, so its auto block-size collapses to the
    // `contain-intrinsic-height` placeholder (see `size_contained` below). The
    // shell drains `take_cv_skipped()` after layout and emits
    // ContentVisibilityChange events / triggers relayout on scroll.
    // CSS: content-visibility — parsing + ComputedStyle field already wired.
    // BUG-341 S32: `content-visibility: auto`'s skip decision below depends on
    // scroll position/a cross-frame ratchet, neither of which lives in
    // `b.style` — mark this subtree's computation as poisoned for the
    // layout-result cache regardless of which way `cv_should_skip` resolves
    // this time, since a *different* call at a different scroll offset could
    // resolve it the other way from the exact same `(node, constraints,
    // style)` key. See `CV_AUTO_TOUCHED`'s doc comment.
    if b.style.content_visibility == crate::style::ContentVisibility::Auto {
        CV_AUTO_TOUCHED.with(|c| c.set(true));
    }
    let cv_auto_skipped = b.style.content_visibility == crate::style::ContentVisibility::Auto
        && !b.children.is_empty()
        && crate::content_visibility::cv_should_skip(b.node, start_y, viewport.height);
    if cv_auto_skipped {
        b.children.clear();
    }

    // SVG root dispatches to its own layout algorithm: replaced-element sizing
    // from CSS width/height (or viewBox fallback), then SVG-coordinate shape positioning.
    if matches!(b.kind, BoxKind::SvgRoot { .. } | BoxKind::SvgShape { .. } | BoxKind::SvgText { .. }) {
        let _prof = lumen_core::profile::scope_detail("lo_svg");
        // BUG-802: this path reads `available_height` in another function, so
        // the flag cannot be maintained per resolution site here.
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(true));
        lay_out_svg_root(b, start_x, start_y, available_width, available_height, viewport);
        return;
    }

    // CSS Writing Modes L3 §3: vertical writing modes swap the block/inline axes.
    // Vertical block stacking and InlineRun flow (below, `lay_out_vertical_inline_run`)
    // are both implemented in the `vertical` module. FormControl and other box
    // kinds inside a vertical context still fall through to horizontal layout.
    // Glyph rotation is a paint concern — CPU rasterizer and wgpu renderer (live
    // default backend, ADR-017) both honor it, including the per-glyph `mixed`
    // CJK-upright/Latin-rotated split; femtovg (fallback backend) does not.
    if !matches!(b.style.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot)
    {
        // BUG-802: `available_height` is consumed inside `crate::vertical`,
        // out of reach of `resolve_block_size`'s per-site bookkeeping.
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(true));
        crate::vertical::lay_out_vertical_block(
            b,
            start_x,
            start_y,
            available_width,
            available_height,
            measurer,
            viewport,
            pcb,
            hp,
        );
        return;
    }

    // BUG-341 S12: an `Arc` bump, not a 3.2 KB deep copy, on the (overwhelming
    // majority) no-override path. The scope stays because its call count is the
    // honest "boxes fully laid out this pass" counter (`lo_translate`'s count is
    // the ones reused from `prev`), and because a future edit that reintroduces
    // an owned clone here would show up as this line growing from ~0.1 ms back
    // towards the 2.3 ms it was.
    //
    // BUG-341 S34: `used_size_override`, when present, is applied to a locally
    // cloned `ComputedStyle` here instead of being burned into `b.style` — see
    // [`UsedSizeOverride`]. `b.style`'s own `Arc` is never touched by this
    // function, so its pointer identity survives this call unconditionally.
    let s = {
        let _prof = lumen_core::profile::scope_detail("lo_style_ref");
        match used_size_override {
            Some(ov) => {
                let mut owned = (*b.style).clone();
                if let Some(bs) = ov.box_sizing {
                    owned.box_sizing = bs;
                }
                if let Some(w) = ov.width {
                    owned.width = Some(Length::Px(w));
                }
                if let Some(h) = ov.height {
                    owned.height = Some(Length::Px(h));
                }
                Arc::new(owned)
            }
            None => Arc::clone(&b.style),
        }
    };
    let em = s.font_size;
    let cb = available_width;

    // CSS Box Sizing L4 §5 — the box is subject to size containment (its size is
    // computed as if it had no contents) when `contain: size` is set, when
    // `content-visibility: hidden` (always skips/contains its subtree), or when
    // `content-visibility: auto` skipped the subtree this pass. Under size
    // containment, auto width/height come from `contain-intrinsic-*` (or 0 when
    // the value is `none`) instead of the content.
    let size_contained = s.contain.0 & crate::style::ContainFlags::SIZE.0 != 0
        || s.content_visibility == crate::style::ContentVisibility::Hidden
        || cv_auto_skipped;

    // Резолвим typed Length-поля с known containing block.
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_right = s.margin_right.resolve_or_zero(em, cb, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb, viewport);

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;
    // Block: auto-ширина = весь доступный inline-размер контейнера.
    // Replaced element (Image): auto-ширина = intrinsic (0 в Phase 0, без
    // декодированных пикселей). Это CSS 2.1 §10.3.2 — replaced-боксы
    // НЕ растягиваются на весь контейнер при отсутствии width.
    // CSS Display L3 §2.4: FormControl (`<button>`, `<select>`) is only
    // "replaced" for sizing while its used `display` keeps the UA-default
    // box type. An author `display: flex`/`grid` blockifies it into a real
    // flex/grid *container* with ordinary box-tree children (e.g. an icon +
    // text `<span>` inside `<button>`) — those children must get auto-width =
    // available space like any other block, not intrinsic-0. Leaving this
    // unconditional made `.ws-add`-style buttons (icon + label, no explicit
    // `width`, in a `flex-direction: column` sidebar) collapse to width 0 and
    // wrap their label onto two lines (BUG-425 item 3) — real browsers don't,
    // because `display: flex` overrides the replaced-sizing default.
    let is_replaced = matches!(b.kind, BoxKind::Image { .. } | BoxKind::Video { .. } | BoxKind::Canvas { .. } | BoxKind::Iframe { .. })
        || (matches!(b.kind, BoxKind::FormControl { .. })
            && !matches!(s.display, Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid));
    // CSS Basic UI L4 §4.4 — field-sizing: content.
    // Pre-compute intrinsic (padding-box width, padding-box height) from text content.
    // Only applies to text-entry FormControls when UA did not supply explicit dimensions.
    let field_intrinsic: Option<(f32, f32)> = if s.field_sizing == FieldSizing::Content
        && is_replaced
        && s.width.is_none()
    {
        if let (BoxKind::FormControl { kind }, Some(m)) = (&b.kind, measurer) {
            let lh = s.font_size * s.line_height;
            match kind {
                FormControlKind::Input { value_text, .. } => {
                    Some(field_sizing_content_intrinsic("input", value_text, s.font_size, lh, m))
                }
                FormControlKind::Textarea { value_text } => {
                    Some(field_sizing_content_intrinsic("textarea", value_text, s.font_size, lh, m))
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    b.rect.width = if is_replaced {
        if let Some((pw, _)) = field_intrinsic {
            pw + s.border_left_width + s.border_right_width
        } else if let Some((aw, ah)) = s.aspect_ratio
            && aw > 0.0
            && ah > 0.0
            && s.width.is_none()
            && let Some(h_len) = &s.height
            && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
        {
            // BUG-734 / CSS 2.1 §10.6.2: `width: auto` + definite height +
            // известное соотношение → ширина выводится из высоты. Симметрично
            // ratio-ветке высоты ниже, поэтому считается в border-box
            // пространстве (у картинок padding/border почти всегда нулевые).
            let h_bb = match s.box_sizing {
                BoxSizing::ContentBox => {
                    h + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                }
                BoxSizing::BorderBox => h,
            };
            (h_bb * aw / ah).max(0.0)
        } else {
            0.0
        }
    } else {
        (available_width - margin_left - margin_right).max(0.0)
    };
    // Явная ширина (CSS width: Npx) перекрывает auto-ширину.
    // box-sizing определяет, к какой части бокса относится `width`:
    //   - content-box: width — это размер контента, padding+border прибавляются;
    //   - border-box: width — общий размер вместе с padding+border.
    if let Some(w_len) = &s.width {
        if w_len.is_intrinsic() {
            // CSS Intrinsic Sizing L3 §4 — min-content / max-content / fit-content.
            // max_content_outer_width / min_content_outer_width already include
            // the box's own padding+border (border-box width), so we assign directly.
            let avail_bb = (available_width - margin_left - margin_right).max(0.0);
            b.rect.width = match w_len {
                Length::MaxContent => max_content_outer_width(b, measurer, viewport),
                Length::MinContent => min_content_outer_width(b, measurer, viewport),
                Length::FitContent(max_arg) => {
                    let max_c = max_content_outer_width(b, measurer, viewport);
                    if let Some(arg) = max_arg {
                        // fit-content(<length>) = min(avail, max(min-content, arg))
                        let min_c = min_content_outer_width(b, measurer, viewport);
                        let arg_px = arg.resolve(em, Some(cb), viewport).unwrap_or(avail_bb);
                        // arg_px is a content-box length; convert to border-box:
                        let arg_bb = match s.box_sizing {
                            BoxSizing::ContentBox => arg_px + padding_left + padding_right
                                + s.border_left_width + s.border_right_width,
                            BoxSizing::BorderBox => arg_px,
                        };
                        max_c.min(min_c.max(arg_bb)).min(avail_bb)
                    } else {
                        // fit-content = min(available, max-content)
                        max_c.min(avail_bb)
                    }
                }
                _ => unreachable!(),
            };
        } else if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
            b.rect.width = match s.box_sizing {
                BoxSizing::ContentBox => (w + padding_left + padding_right
                    + s.border_left_width + s.border_right_width).max(0.0),
                BoxSizing::BorderBox => w.max(padding_left + padding_right + s.border_left_width + s.border_right_width),
            };
        }
    }
    // CSS 2.1 §10.4: tentative width → clamp в [min-width, max-width].
    // Intrinsic keywords in min-/max- also resolve to intrinsic values here.
    // Порядок «max сначала, потом min» автоматически даёт правило
    // «при min > max побеждает min». min-/max- интерпретируются в той же
    // box-sizing модели, что и width: content-box добавляет padding+border,
    // border-box оставляет как есть.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + padding_left + padding_right
            + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_len) = &s.max_width {
        let max_bb = if max_len.is_intrinsic() {
            Some(max_content_outer_width(b, measurer, viewport))
        } else {
            max_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v).max(0.0))
        };
        if let Some(max_w) = max_bb {
            b.rect.width = b.rect.width.min(max_w);
        }
    }
    if let Some(min_len) = &s.min_width {
        let min_bb = if min_len.is_intrinsic() {
            Some(min_content_outer_width(b, measurer, viewport))
        } else {
            min_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v.max(0.0)))
        };
        if let Some(min_w) = min_bb {
            b.rect.width = b.rect.width.max(min_w);
        }
    }
    // Phase 0 shrink-to-fit для atomic inline-level бокса без явной CSS width.
    // Полный алгоритм (CSS 2.1 §10.3.9) требует двух проходов; здесь —
    // упрощение: ищем максимальную explicit-width среди потомков.
    // CSS Box Sizing L4 §5: a size-contained inline-block ignores its content
    // for auto inline-size and uses contain-intrinsic-width (content-box → +pad/
    // border), or 0 when `none`/unset — exactly as if it had no contents.
    //
    // BUG-739: `inline-flex`/`inline-grid` — тот же класс боксов (CSS Display L3
    // §2.1), их auto-ширина тоже shrink-to-fit, а не «весь доступный inline-
    // размер». Без этой ветки inline-flex-кнопка растягивалась бы на всю строку.
    if s.width.is_none()
        && matches!(
            s.display,
            Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        )
    {
        if size_contained {
            let cw = s
                .contain_intrinsic_width
                .as_ref()
                .and_then(|l| l.resolve(em, None, viewport))
                .map_or(0.0, |v| v.max(0.0));
            b.rect.width = (cw + padding_left + padding_right
                + s.border_left_width + s.border_right_width)
                .min(b.rect.width);
        } else if let Some(pref_w) = preferred_inline_block_width(b, measurer, viewport) {
            b.rect.width = pref_w.min(b.rect.width);
        }
    }

    // CSS 2.1 §10.3.3 — auto horizontal-margin centering for block-level
    // non-replaced elements in normal flow with an explicit CSS width.
    // Remaining inline space distributes to auto margins: both auto → equal
    // halves (centered block); only left auto → left takes all remaining;
    // only right auto → no x shift (right margin absorbs remainder silently).
    // Does not apply to: replaced, inline-block, flex/grid containers, floats,
    // or absolute/fixed positioned elements.
    let ml_is_auto = s.margin_left.is_auto();
    let mr_is_auto = s.margin_right.is_auto();
    if (ml_is_auto || mr_is_auto)
        && s.width.is_some()
        && !is_replaced
        && !matches!(
            s.display,
            Display::InlineBlock
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
        )
        && !matches!(s.float_side, FloatSide::Left | FloatSide::Right)
        && !matches!(s.position, Position::Absolute | Position::Fixed)
    {
        let ml_fixed = if ml_is_auto { 0.0 } else { margin_left };
        let mr_fixed = if mr_is_auto { 0.0 } else { margin_right };
        let remaining = (available_width - b.rect.width - ml_fixed - mr_fixed).max(0.0);
        let ml_computed = if ml_is_auto && mr_is_auto {
            remaining / 2.0
        } else if ml_is_auto {
            remaining
        } else {
            ml_fixed
        };
        b.rect.x = start_x + ml_computed;
    }

    // CSS Box Alignment L3 §5.2 — `justify-self` for block-level boxes in normal
    // flow with a definite inline size and no auto inline margins. Distributes the
    // free inline space (containing block − box margin box) within the containing
    // block: `center` centres, `end` flushes to the inline-end. `start` (and
    // `stretch`/`normal`, whose block-level behaviour is inline-start) leave the box
    // at the inline-start (current behaviour), so pages that don't align are
    // unaffected. Auto margins take precedence (handled above), matching the spec's
    // alignment/margin ordering. Same box class as auto-margin centring:
    // non-replaced block-level in flow.
    //
    // §6.3: `justify-self: auto` resolves to the parent's `justify-items`
    // (`parent_justify_items`, threaded from the in-flow block-child recursion).
    // Independent-formatting-context call sites pass `AlignValue::Auto`, so their
    // boxes keep the inline-start default.
    let effective_justify = if matches!(s.justify_self, AlignValue::Auto) {
        parent_justify_items
    } else {
        s.justify_self
    };
    if !ml_is_auto
        && !mr_is_auto
        && s.width.is_some()
        && matches!(effective_justify, AlignValue::Center | AlignValue::End)
        && !is_replaced
        && !matches!(
            s.display,
            Display::InlineBlock
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
        )
        && !matches!(s.float_side, FloatSide::Left | FloatSide::Right)
        && !matches!(s.position, Position::Absolute | Position::Fixed)
    {
        let remaining = (available_width - b.rect.width - margin_left - margin_right).max(0.0);
        let shift = match effective_justify {
            AlignValue::Center => remaining / 2.0,
            AlignValue::End => remaining,
            _ => 0.0,
        };
        b.rect.x = start_x + margin_left + shift;
    }

    let content_x = b.rect.x + padding_left + s.border_left_width;
    let content_y = b.rect.y + padding_top + s.border_top_width;
    let mut content_width = (b.rect.width
        - padding_left - padding_right
        - s.border_left_width - s.border_right_width).max(0.0);
    // CSS Scrollbars L1 §6.2: `scrollbar-gutter: stable` reserves gutter space in
    // layout so content shifts don't occur when the scrollbar track appears.
    content_width = (content_width - scrollbar_gutter_inline(&s)).max(0.0);

    // pcb для потомков: если текущий элемент positioned — он сам CB для абсолютных детей.
    // CSS Containment L3: contain:layout и contain:paint тоже устанавливают containing block.
    // Высота ещё неизвестна, используем 0 — корректируем after layout.
    let is_positioned = !matches!(s.position, Position::Static);
    let contain_establishes_cb = s.contain.0
        & (ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0 | ContainFlags::STRICT.0) != 0;
    let children_pcb = if is_positioned || contain_establishes_cb {
        // CSS Position L3 §2.2: CB for absolute descendants = padding edge of the element.
        Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            0.0,
        )
    } else {
        pcb
    };

    // Vertical InlineRun layout (Phase 2): text flows top→bottom with
    // glyph rotation handled in paint. Dispatches before the horizontal
    // InlineRun branch so vertical text gets axis-swapped wrapping.
    if !matches!(s.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(b.kind, BoxKind::InlineRun { .. })
    {
        // BUG-802 — see the sibling `lay_out_vertical_block` dispatch above.
        INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(true));
        crate::vertical::lay_out_vertical_inline_run(
            b,
            start_x,
            start_y,
            available_width,
            available_height,
            measurer,
            viewport,
            pcb,
            hp,
        );
        return;
    }

    // InlineRun обрабатывается до основного match.
    if let BoxKind::InlineRun { segments, lines, first_line_style } = &mut b.kind {
        if let Some(m) = measurer {
            // white-space: nowrap / text-wrap-mode: nowrap → infinite max_width so
            // the line-breaker never wraps; word-spacing/letter-spacing logic unchanged.
            let wrap_width = if s.white_space.is_nowrap() || s.text_wrap_mode == TextWrapMode::Nowrap {
                f32::INFINITY
            } else {
                content_width
            };
            let text_indent_px = s.text_indent.resolve_or_zero(em, cb, viewport);
            // UAX #9 P2–I2 once per paragraph, before any wrapping trial: the
            // result splits segments at embedding-level boundaries, and every
            // re-wrap (::first-line pass B, text-wrap: balance/pretty) must see
            // the same segment list the frags will be mapped back onto.
            // `b.kind` keeps the logical, unsplit segments — resolution is a
            // pure function of them, so a relayout reproduces it exactly.
            let resolved;
            let segments: &[InlineSegment] =
                if crate::bidi::needs_resolution(segments, s.direction) {
                    resolved = crate::bidi::resolve(segments, s.direction);
                    &resolved
                } else {
                    segments
                };
            *lines = if let Some(fls) = first_line_style.as_deref() {
                // CSS Pseudo-elements L4 §3.1 — ::first-line layout split (BB-1).
                // Pass A: wrap ALL segments under the ::first-line style to find the
                // true extent of the first formatted line (a larger ::first-line font
                // fits fewer words). Hyphenation is off for this pass: a first line
                // ending mid-word would make the word-level remainder split ambiguous,
                // so the first formatted line never auto-hyphenates (UA freedom).
                let fl_segments: Vec<InlineSegment> = segments
                    .iter()
                    .map(|seg| {
                        let mut fl_seg = seg.clone();
                        if fl_seg.img_src.is_none() {
                            // §3.4: the pseudo-element only supplies what the
                            // segment inherited — an inner `<b>`/`<em>` keeps its
                            // own metrics, so pass A measures the real glyphs.
                            fl_seg.style =
                                crate::style::merge_pseudo_inherited(&seg.style, &s, fls);
                        }
                        fl_seg
                    })
                    .collect();
                let mut lines_a = wrap_inline_run(
                    &fl_segments, wrap_width, fls.font_size, text_indent_px, viewport,
                    m, Hyphens::None, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                );
                if lines_a.len() <= 1 {
                    // Everything fits the first formatted line; ::first-line covers it all.
                    lines_a
                } else {
                    // Pass B: re-wrap the content NOT consumed by line 0 under the base
                    // style (its own font metrics, no text-indent — indent is first-line only).
                    let line0 = lines_a.remove(0);
                    let (_, rest_segs) = split_segments_at_first_line(
                        segments, &line0, s.white_space.preserves_whitespace(),
                    );
                    let raw_rest = wrap_inline_run(
                        &rest_segs, wrap_width, s.font_size, 0.0, viewport,
                        m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                    );
                    let rest = if wrap_width.is_finite() {
                        match s.text_wrap_style {
                            TextWrapStyle::Balance => balance_wrap(
                                &rest_segs, wrap_width, raw_rest, s.font_size, 0.0,
                                viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                            ),
                            TextWrapStyle::Pretty => pretty_wrap(
                                &rest_segs, wrap_width, raw_rest, s.font_size, 0.0,
                                viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                            ),
                            TextWrapStyle::Auto | TextWrapStyle::Stable => raw_rest,
                        }
                    } else {
                        raw_rest
                    };
                    let mut all = Vec::with_capacity(1 + rest.len());
                    all.push(line0);
                    all.extend(rest);
                    all
                }
            } else {
                let raw_lines = wrap_inline_run(segments, wrap_width, s.font_size, text_indent_px, viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break);
                // CSS Text L4 §6.4.2: apply text-wrap-style post-processing only when
                // wrapping is active (wrap_width is finite) and text actually wraps.
                if wrap_width.is_finite() {
                    match s.text_wrap_style {
                        TextWrapStyle::Balance => balance_wrap(
                            segments, wrap_width, raw_lines, s.font_size, text_indent_px,
                            viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                        ),
                        TextWrapStyle::Pretty => pretty_wrap(
                            segments, wrap_width, raw_lines, s.font_size, text_indent_px,
                            viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap, s.line_break,
                        ),
                        // Auto / Stable: greedy result unchanged.
                        // Stable stability is about incremental editing; for static layout it's identical to auto.
                        TextWrapStyle::Auto | TextWrapStyle::Stable => raw_lines,
                    }
                } else {
                    raw_lines
                }
            };
            align_lines(lines, content_width, s.text_align, s.text_align_last, s.direction);
            // CSS Rhythmic Sizing L1 §2 — round each line box up to a multiple of line-height-step.
            let line_h = step_line_height(s.font_size * s.line_height, s.line_height_step);
            apply_inline_vertical_align(lines, line_h);
            // CSS Overflow L4 §3.2: -webkit-line-clamp / line-clamp — multi-line truncation.
            // Takes priority over text-overflow:ellipsis (both cannot apply simultaneously).
            if let Some(n) = s.line_clamp.filter(|&n| n > 0) {
                apply_line_clamp(lines, n, content_width, s.font_size, m);
            } else if s.text_overflow == TextOverflow::Ellipsis
                && (s.overflow_x != Overflow::Visible || s.overflow_y != Overflow::Visible)
            {
                // CSS UI L4 §10.1: text-overflow: ellipsis требует overflow != visible.
                apply_text_overflow_ellipsis(lines, content_width, s.font_size, m);
            }
        } else {
            *lines = one_line_fallback(segments);
        }
        // CSS Pseudo-elements L4 §3.1: ::first-line applies to the first formatted line.
        // Mark frags on lines[0] and apply pre-computed ::first-line style override.
        if let Some(first_line) = lines.first_mut() {
            for frag in first_line.iter_mut() {
                frag.is_first_line = true;
                // §3.4: ::first-line is the *parent* of the first line's content,
                // so it only supplies properties the fragment inherited; an inner
                // `<b>`/`<em>`/`style="color:…"` keeps its own declarations.
                if let Some(fls) = first_line_style {
                    frag.style = crate::style::merge_pseudo_inherited(&frag.style, &s, fls);
                }
            }
        }
        let line_count = lines.len().max(1);
        // CSS Pseudo-elements L4 §3.1: the first formatted line uses the ::first-line
        // style's own font metrics for its line box height (BB-1).
        // CSS Rhythmic Sizing L1 §2 — line-height-step rounds every line box (incl. ::first-line).
        let step = s.line_height_step;
        b.rect.height = match first_line_style.as_deref() {
            Some(fls) if !lines.is_empty() => {
                step_line_height(fls.font_size * fls.line_height, step)
                    + (line_count - 1) as f32 * step_line_height(s.font_size * s.line_height, step)
            }
            _ => line_count as f32 * step_line_height(s.font_size * s.line_height, step),
        };
        return;
    }

    // Абсолютно-позиционированные дети: (index, static_x, static_y).
    // Заполняется внутри Block-flow и обрабатывается после match.
    let mut abs_deferred: Vec<(usize, f32, f32)> = Vec::new();

    match &mut b.kind {
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::Image { .. } | BoxKind::Video { .. } | BoxKind::Canvas { .. } | BoxKind::Audio { .. } | BoxKind::Iframe { .. } | BoxKind::FormControl { .. } => {
            // Flex containers dispatch to lay_out_flex before block-flow.
            if matches!(s.display, Display::Flex | Display::InlineFlex) {
                // For row flex, align-content needs the explicit container height (cross axis).
                let flex_explicit_cross = if !matches!(
                    s.flex_direction,
                    FlexDirection::Column | FlexDirection::ColumnReverse
                ) {
                    s.height.as_ref()
                        .and_then(|h| resolve_block_size(h, em, available_height, viewport))
                        .map(|h| match s.box_sizing {
                            BoxSizing::ContentBox => h,
                            BoxSizing::BorderBox => (h - padding_top - padding_bottom
                                - s.border_top_width - s.border_bottom_width)
                                .max(0.0),
                        })
                } else {
                    None
                };
                // CSS Flexbox §9.7: for a column flex container with a definite
                // main (block) size, free space is distributed to flex-grow items.
                // Compute that definite content-box height here so `lay_out_flex`
                // can grow children instead of collapsing them to flex-basis
                // (BUG-104 — `.right-col` children with `flex:1` were height 0).
                let flex_explicit_main = if matches!(
                    s.flex_direction,
                    FlexDirection::Column | FlexDirection::ColumnReverse
                ) {
                    s.height.as_ref()
                        .and_then(|h| resolve_block_size(h, em, available_height, viewport))
                        .map(|h| match s.box_sizing {
                            BoxSizing::ContentBox => h,
                            BoxSizing::BorderBox => (h - padding_top - padding_bottom
                                - s.border_top_width - s.border_bottom_width)
                                .max(0.0),
                        })
                } else {
                    None
                };
                let content_height = lay_out_flex(
                    &mut b.children, &s, content_x, content_y, content_width,
                    flex_explicit_cross, flex_explicit_main, measurer, viewport, children_pcb, hp,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else if let Some((aw, ah)) = s.aspect_ratio
                    && aw > 0.0 && ah > 0.0
                {
                    (b.rect.width * ah / aw).max(0.0)
                } else {
                    let ch = contained_content_height(size_contained, &s, em, viewport, content_height);
                    ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                };
                // CSS Flexbox L1 §4.1: absolutely-positioned children were excluded
                // from flex layout above. Position them now against this container's
                // content box (its padding edge when positioned), using the content
                // origin as their static position.
                let flex_abs: Vec<(usize, f32, f32)> = b.children.iter().enumerate()
                    .filter(|(_, c)| matches!(c.style.position, Position::Absolute | Position::Fixed))
                    .map(|(i, _)| (i, content_x, content_y))
                    .collect();
                if !flex_abs.is_empty() {
                    let my_pcb = if is_positioned {
                        Rect::new(
                            b.rect.x + s.border_left_width,
                            b.rect.y + s.border_top_width,
                            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                            (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
                        )
                    } else {
                        pcb
                    };
                    lay_out_abs_children(b, &flex_abs, measurer, viewport, my_pcb, hp);
                }
                return;
            }
            // Grid containers dispatch to lay_out_grid before block-flow.
            if matches!(s.display, Display::Grid | Display::InlineGrid) {
                // CSS Box Alignment L3 §5: `align-content` distributes the block-axis
                // free space of the grid container, so the row-axis pass needs the
                // container's *definite* content-box height (None when the height is
                // content-derived — there is no free space to distribute then).
                let grid_definite_height = s.height.as_ref()
                    .and_then(|h| resolve_block_size(h, em, available_height, viewport))
                    .map(|h| match s.box_sizing {
                        BoxSizing::ContentBox => h,
                        BoxSizing::BorderBox => (h - padding_top - padding_bottom
                            - s.border_top_width - s.border_bottom_width)
                            .max(0.0),
                    });
                let content_height = lay_out_grid(
                    &mut b.children, &s, content_x, content_y, content_width, grid_definite_height,
                    measurer, viewport, children_pcb, hp,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else if let Some((aw, ah)) = s.aspect_ratio
                    && aw > 0.0 && ah > 0.0
                {
                    (b.rect.width * ah / aw).max(0.0)
                } else {
                    let ch = contained_content_height(size_contained, &s, em, viewport, content_height);
                    ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                };
                return;
            }
            // Image не имеет flow-детей, поэтому child-цикл просто пуст —
            // объединяем с Block, чтобы общий код width/height/min-max/borders
            // не дублировался. content_height = 0 для Image без явной высоты
            // даёт коробку только из padding+border (что для пустой картинки
            // визуально корректно).
            // CSS 2.1 §10.5: definite content height for children's height percentage resolution.
            // Only available when this element itself has an explicit height.
            let children_available_height: Option<f32> = if let Some(h_len) = &s.height
                && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
            {
                let content_h = match s.box_sizing {
                    BoxSizing::ContentBox => h,
                    BoxSizing::BorderBox => (h - padding_top - padding_bottom
                        - s.border_top_width - s.border_bottom_width).max(0.0),
                };
                // CSS Scrollbars L1 §6.2: reserve the block-axis gutter (space for a
                // horizontal scrollbar at the block-end edge) so `%`-height children
                // don't shift when the scrollbar appears. Symmetric to the inline
                // `content_width -= scrollbar_gutter_inline(&s)` reduction above; the
                // box's own border-box height is unchanged, only the content area seen
                // by children shrinks.
                Some((content_h - scrollbar_gutter_block(&s)).max(0.0))
            } else {
                None
            };
            let content_height = if (s.column_count.is_some() || s.column_width.is_some())
                && !b.children.is_empty()
            {
                lay_out_multicol_children(
                    &mut b.children,
                    content_x, content_y, content_width,
                    &s, em, measurer, viewport, children_pcb, hp,
                    children_available_height,
                )
            } else {
                // CSS 2.1 §9.5 — float context for this block formatting context.
                // A non-BFC block laid out beside an enclosing context's floats
                // inherits them so its line boxes are shortened (it does not own
                // them). A BFC root starts fresh — it never overlaps outer floats.
                let mut fc = match outer_floats {
                    Some(p) if !establishes_bfc(b) => FloatContext::inheriting(p),
                    _ => FloatContext::new(),
                };
                let container_right = content_x + content_width;

                let mut child_y = content_y;
                // CSS 2.1 §8.3.1: resolved bottom margin of the previous block-level child.
                // Adjacent Block/FlowRoot siblings collapse their margins (gap = max, not sum).
                // Inline runs, replaced elements, and floats break the collapsing chain.
                let mut prev_block_mb: f32 = 0.0;
                // CSS 2.1 §8.3.1: this block's top margin collapses with the top margin of
                // its first in-flow block child when nothing separates them — no top border,
                // no top padding, no BFC, and the box is itself a normal in-flow block (not a
                // flex/grid item or document root). In that case the first child's top margin
                // has already been folded into this box's position by the parent loop (via
                // `collapsed_top_margin`), so the child is placed flush at the content top.
                let b_collapses_top = in_block_flow
                    && matches!(b.kind, BoxKind::Block)
                    && !establishes_bfc(b)
                    && padding_top == 0.0
                    && s.border_top_width == 0.0;
                // CSS 2.1 §8.3.1: symmetric to `b_collapses_top` — this block's
                // bottom margin collapses with the bottom margin of its last in-flow
                // block child when nothing separates them: auto height, no bottom
                // padding, no bottom border, no BFC, and the box is a normal in-flow
                // block. In that case the last child's bottom margin escapes out of
                // this box (folded into its own bottom margin by the parent loop via
                // `collapsed_bottom_margin`) instead of inflating the content height.
                let b_collapses_bottom = in_block_flow
                    && matches!(b.kind, BoxKind::Block)
                    && !establishes_bfc(b)
                    && padding_bottom == 0.0
                    && s.border_bottom_width == 0.0
                    && s.height.is_none();
                // Tracks whether the first in-flow child has been positioned yet.
                let mut seen_inflow_child = false;
                // CSS Lists L3 §2.4: pending indent from an inside ::marker (em units).
                // Consumed by the first normal-flow content child after the marker.
                let mut inside_marker_w: f32 = 0.0;
                for (i, child) in b.children.iter_mut().enumerate() {
                    if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                        abs_deferred.push((i, content_x, child_y));
                        continue;
                    }
                    // CSS Lists L3 §2.4 — position ::marker outside or inside principal block.
                    if matches!(&child.kind, BoxKind::Marker { .. }) {
                        let (position, em, lh, marker_text) =
                            if let BoxKind::Marker { position, text, .. } = &child.kind {
                                (*position, child.style.font_size, child.style.line_height, text.clone())
                            } else { unreachable!() };
                        let line_h = em * lh;
                        // CSS Lists L3 §2.4 — the outside marker occupies the area to the
                        // left of the principal box. The default box is `em * 1.5`; a text
                        // marker (counter glyph or `::marker { content }`) wider than that —
                        // e.g. a custom `@counter-style` with a long prefix/suffix like
                        // "#1: " — must grow the box leftward so its string right-aligns at
                        // the content edge instead of overflowing into the first word
                        // ("#1:One" instead of "#1: One" — BUG-185).
                        let default_w = em * 1.5;
                        let text_w = if marker_text.is_empty() {
                            0.0
                        } else {
                            measurer.map_or(0.0, |m| {
                                let fams = &child.style.font_family;
                                let ts = child.style.tab_size
                                    * m.char_width_with_families(' ', em, fams);
                                measure_text_w_families(
                                    &marker_text, em, child.style.letter_spacing, ts, fams, m,
                                )
                            })
                        };
                        let marker_w = default_w.max(text_w); // CSS: list-style-type determines exact width
                        match position {
                            ListStylePosition::Outside => {
                                // Out of flow: does not advance child_y.
                                // Snap to integer CSS pixels — em*1.5 is often fractional (BUG-083).
                                child.rect = Rect::new(
                                    (content_x - marker_w).round(),
                                    child_y.round(),
                                    marker_w.round(),
                                    line_h.round(),
                                );
                            }
                            ListStylePosition::Inside => {
                                // CSS Lists L3 §2.4: inside marker shares the first line with
                                // content. Place at content_x; record indent for the next child.
                                child.rect = Rect::new(
                                    content_x.round(),
                                    child_y.round(),
                                    marker_w.round(),
                                    line_h.round(),
                                );
                                inside_marker_w = marker_w.round();
                                // Do NOT advance child_y — marker is inline with content.
                            }
                        }
                        continue;
                    }

                    // CSS 2.1 §9.5.2: clear — advance child_y past relevant floats.
                    // Clearance is inserted between the top margin and the top border, so the
                    // final border edge ends up at max(natural-flow border, float bottom): the
                    // top margin is *absorbed* by clearance, not stacked on top of the float
                    // bottom. `clearance_pre` remembers the pre-clear flow position so the
                    // start_y computation below can place the border at that maximum (fixes the
                    // double-count where a cleared block dropped to float_bottom + margin_top).
                    let clearance_pre = if !fc.is_empty() && child.style.clear != ClearSide::None {
                        let pre = child_y;
                        child_y = fc.clear_y(child_y, child.style.clear);
                        Some(pre)
                    } else {
                        None
                    };

                    // CSS 2.1 §9.5.1: float box — placed out of normal flow.
                    if child.style.float_side != FloatSide::None {
                        let cem = child.style.font_size;
                        // Shrink-to-fit width (CSS 2.1 §10.3.5): explicit CSS width wins;
                        // otherwise preferred content width, falling back to max-content
                        // measurement for text-only floats (e.g. the ::first-letter
                        // drop-cap box, BB-2), clamped to available space. `probe_w` decides
                        // the float's box at the *current* line; the outer width is then used
                        // to test whether the float fits or must drop (rule 8 below).
                        let probe_avail = {
                            let l = fc.left_edge_at(child_y, content_x);
                            let r = fc.right_edge_at(child_y, container_right);
                            (r - l).max(0.0)
                        };
                        let probe_w = if child.style.width.is_some() {
                            probe_avail
                        } else {
                            preferred_inline_block_width(child, measurer, viewport)
                                .or_else(|| {
                                    let w = max_content_outer_width(child, measurer, viewport);
                                    (w > 0.0).then_some(w)
                                })
                                .map(|pw| pw.min(probe_avail))
                                .unwrap_or(probe_avail)
                        };
                        lay_out(child, fc.left_edge_at(child_y, content_x), child_y, probe_w,
                                children_available_height, measurer, viewport, children_pcb, hp, false);

                        // CSS 2.1 §9.5.1 rule 8: if the float's outer margin box does not fit
                        // in the space beside existing floats, drop it below them until it fits
                        // (or no float remains to clear). This wraps a row of left floats onto a
                        // new line in a narrow container instead of overflowing past the edge.
                        let probe_ml = child.style.margin_left.resolve_or_zero(cem, probe_avail, viewport);
                        let probe_mr = child.style.margin_right.resolve_or_zero(cem, probe_avail, viewport);
                        let outer_w = probe_ml + child.rect.width + probe_mr;
                        let mut float_y = child_y;
                        while !fc.is_empty() {
                            let l = fc.left_edge_at(float_y, content_x);
                            let r = fc.right_edge_at(float_y, container_right);
                            if outer_w <= (r - l).max(0.0) {
                                break;
                            }
                            match fc.next_float_bottom(float_y) {
                                Some(ny) => float_y = ny,
                                None => break,
                            }
                        }
                        let dropped = (float_y - child_y).abs() > f32::EPSILON;
                        // Shadow child_y at the (possibly dropped) line for the placement below.
                        let child_y = float_y;
                        let avail_left  = fc.left_edge_at(child_y, content_x);
                        let avail_right = fc.right_edge_at(child_y, container_right);
                        let avail_w = (avail_right - avail_left).max(0.0);
                        // Re-lay-out at the dropped line: an auto-width float may grow into the
                        // wider line, and the box's origin changed.
                        if dropped {
                            let w = if child.style.width.is_some() {
                                avail_w
                            } else {
                                preferred_inline_block_width(child, measurer, viewport)
                                    .or_else(|| {
                                        let w = max_content_outer_width(child, measurer, viewport);
                                        (w > 0.0).then_some(w)
                                    })
                                    .map(|pw| pw.min(avail_w))
                                    .unwrap_or(avail_w)
                            };
                            lay_out(child, avail_left, child_y, w,
                                    children_available_height, measurer, viewport, children_pcb, hp, false);
                        }

                        let fml = child.style.margin_left.resolve_or_zero(cem, avail_w, viewport);
                        let fmr = child.style.margin_right.resolve_or_zero(cem, avail_w, viewport);
                        let fmt = child.style.margin_top.resolve_or_zero(cem, avail_w, viewport);
                        let fmb = child.style.margin_bottom.resolve_or_zero(cem, avail_w, viewport);
                        let fw  = child.rect.width;
                        let fh  = child.rect.height;

                        match child.style.float_side {
                            FloatSide::Left => {
                                let lx = fc.left_edge_at(child_y, content_x);
                                child.rect.x = lx + fml;
                                child.rect.y = child_y + fmt;
                                let top_y  = child_y + fmt;
                                let bot_y  = top_y + fh + fmb;
                                let right_edge = lx + fml + fw + fmr;
                                fc.add_left(bot_y, right_edge);
                                // CSS Shapes L1 — wire shape-outside for left float.
                                // Margin-box origin: (lx, child_y). Points are float-local.
                                if let crate::style::ShapeOutside::Value(ref sv) = child.style.shape_outside {
                                    if let Some(r) = parse_circle_px(sv) {
                                        let cx = child.rect.x + fw / 2.0;
                                        let cy = top_y + fh / 2.0;
                                        fc.shape_circles.push((top_y, bot_y, true, cx, cy, r));
                                    } else if let Some(local_pts) = parse_shape_path_px(sv)
                                        .or_else(|| parse_shape_polygon_px(sv))
                                    {
                                        let pts = local_pts.into_iter()
                                            .map(|(px, py)| (px + lx, py + child_y))
                                            .collect();
                                        fc.shape_polygons.push(ShapePolygon {
                                            top_y, bottom_y: bot_y, is_left: true, points: pts,
                                        });
                                    } else if let Some((rx, ry, ecx, ecy)) = parse_shape_ellipse_px(sv) {
                                        fc.shape_ellipses.push(ShapeEllipse {
                                            top_y, bottom_y: bot_y, is_left: true,
                                            cx: ecx + lx, cy: ecy + child_y, rx, ry,
                                        });
                                    } else if let Some((it, ir, ib, il, irad)) = parse_shape_inset_px(sv) {
                                        // Reference box = margin box: origin (lx, child_y),
                                        // width fml+fw+fmr, bottom bot_y.
                                        let shape_top = (child_y + it).min(bot_y);
                                        let shape_bot = (bot_y - ib).max(shape_top);
                                        fc.shape_insets.push(ShapeInset {
                                            top_y: shape_top, bottom_y: shape_bot, is_left: true,
                                            left_x: lx + il,
                                            right_x: lx + fml + fw + fmr - ir,
                                            radius: irad,
                                        });
                                    }
                                }
                            }
                            FloatSide::Right => {
                                let rx = fc.right_edge_at(child_y, container_right);
                                child.rect.x = rx - fmr - fw;
                                child.rect.y = child_y + fmt;
                                let top_y  = child_y + fmt;
                                let bot_y  = top_y + fh + fmb;
                                let left_edge = rx - fmr - fw - fml;
                                fc.add_right(bot_y, left_edge);
                                // CSS Shapes L1 — wire shape-outside for right float.
                                // Margin-box origin: (left_edge, child_y). Points are float-local.
                                if let crate::style::ShapeOutside::Value(ref sv) = child.style.shape_outside {
                                    if let Some(r) = parse_circle_px(sv) {
                                        let cx = child.rect.x + fw / 2.0;
                                        let cy = top_y + fh / 2.0;
                                        fc.shape_circles.push((top_y, bot_y, false, cx, cy, r));
                                    } else if let Some(local_pts) = parse_shape_path_px(sv)
                                        .or_else(|| parse_shape_polygon_px(sv))
                                    {
                                        let pts = local_pts.into_iter()
                                            .map(|(px, py)| (px + left_edge, py + child_y))
                                            .collect();
                                        fc.shape_polygons.push(ShapePolygon {
                                            top_y, bottom_y: bot_y, is_left: false, points: pts,
                                        });
                                    } else if let Some((rx_e, ry_e, ecx, ecy)) = parse_shape_ellipse_px(sv) {
                                        fc.shape_ellipses.push(ShapeEllipse {
                                            top_y, bottom_y: bot_y, is_left: false,
                                            cx: ecx + left_edge, cy: ecy + child_y, rx: rx_e, ry: ry_e,
                                        });
                                    } else if let Some((it, ir, ib, il, irad)) = parse_shape_inset_px(sv) {
                                        // Reference box = margin box: origin (left_edge, child_y),
                                        // right edge rx, bottom bot_y.
                                        let shape_top = (child_y + it).min(bot_y);
                                        let shape_bot = (bot_y - ib).max(shape_top);
                                        fc.shape_insets.push(ShapeInset {
                                            top_y: shape_top, bottom_y: shape_bot, is_left: false,
                                            left_x: left_edge + il,
                                            right_x: rx - ir,
                                            radius: irad,
                                        });
                                    }
                                }
                            }
                            FloatSide::None => unreachable!(),
                        }
                        // Float does not advance child_y in normal flow.
                        continue;
                    }

                    // Normal flow: narrow x/width for active floats.
                    let flow_left  = fc.left_edge_at(child_y, content_x);
                    let flow_right = fc.right_edge_at(child_y, container_right);
                    // Apply inside-marker indent to the first normal-flow content child.
                    let (mut eff_left, mut eff_w) = if inside_marker_w > 0.0 {
                        let l = flow_left + inside_marker_w;
                        inside_marker_w = 0.0;
                        (l, (flow_right - l).max(0.0))
                    } else {
                        (flow_left, (flow_right - flow_left).max(0.0))
                    };
                    // CSS 2.1 §9.5: a block-level box in normal flow is NOT narrowed by
                    // floats — its width and margins resolve against the full containing
                    // block and only its line boxes are shortened.
                    //
                    // `outer_for_child` carries this block's float context down into an
                    // in-flow non-BFC child so its (and its descendants') line boxes are
                    // shortened by the active floats — instead of the box itself being
                    // narrowed/clipped (the legacy approximation).
                    let mut outer_for_child: Option<&FloatContext> = None;
                    if (flow_left > content_x || flow_right < container_right)
                        && child.style.width.is_none()
                        && matches!(child.kind, BoxKind::Block)
                        && !establishes_bfc(child)
                    {
                        if has_in_flow_content(child) {
                            // Auto-width non-BFC block with content beside a float: keep the
                            // full containing-block width and propagate the float context so
                            // the child's line boxes recede past the float (CSS 2.1 §9.5).
                            eff_left = content_x;
                            eff_w = content_width;
                            outer_for_child = Some(&fc);
                        } else {
                            // *Empty* auto-width block (no in-flow content to reflow): resolve
                            // geometry against the full content width, then clip the result to
                            // the non-float band. This keeps the visual identical when the box
                            // would overlap a float (Lumen paints floats in source order, so the
                            // clip stands in for float-over-block painting), while restoring a
                            // margin'd box that fits in the gap between two floats — which the
                            // naive narrowing collapsed to zero width.
                            let cem = child.style.font_size;
                            let ml = child.style.margin_left.resolve_or_zero(cem, content_width, viewport);
                            let mr = child.style.margin_right.resolve_or_zero(cem, content_width, viewport);
                            let bw = (content_width - ml - mr).max(0.0);
                            let nat_x = content_x + ml;
                            let vx = nat_x.max(flow_left);
                            let vw = ((nat_x + bw).min(flow_right) - vx).max(0.0);
                            // Reproduce the clipped border-box through lay_out's margin re-add:
                            // it places x at eff_left + ml and width at eff_w − ml − mr.
                            eff_left = vx - ml;
                            eff_w = vw + ml + mr;
                        }
                    }

                    // CSS 2.1 §8.3.1: collapse adjacent sibling block margins.
                    // Block/FlowRoot/Table participate; other kinds break the chain. A `Table`
                    // box is block-level and its (wrapper) margins collapse with adjacent
                    // sibling margins like a normal block, even though it establishes a BFC for
                    // its own contents (so `collapsed_top_margin`/`collapsed_bottom_margin`
                    // return its own margin without folding into its rows — see those fns).
                    // `own_mt` is the child's own resolved top margin (what lay_out re-adds
                    // internally); `collapsed_mt` additionally folds the child's own first-child
                    // chain (§8.3.1). The base formula offsets start_y by (collapsed_mt − own_mt)
                    // so that lay_out's internal "+own_mt" lands the child at its collapsed flow
                    // position child_y + max(prev_block_mb, collapsed_mt).
                    let is_block = matches!(&child.kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Table);
                    let is_first_inflow = !seen_inflow_child;
                    let own_mt = child.style.margin_top
                        .resolve_or_zero(child.style.font_size, eff_w, viewport);
                    // CSS 2.1 §8.3.1: the margins of the root element's box do not collapse.
                    // When this container is the document box (`NodeId` index 0), its first
                    // in-flow block child IS the root element, so the parent↔first-child collapse
                    // chain must terminate there: a descendant's escaping top margin must not
                    // shift the root element (and the propagated canvas background it backs) off
                    // the viewport origin. Laying it out with `in_block_flow == false` also stops
                    // it from flush-collapsing its own first child, so that child's collapsed
                    // margin stays inside the root box (BUG-153 — restores the 1px magenta frame
                    // top edge that BUG-151's collapse-through regressed).
                    let child_is_root_element =
                        b.node.index() == 0 && is_first_inflow && is_block;
                    let collapsed_mt = if child_is_root_element {
                        own_mt
                    } else {
                        collapsed_top_margin(child, eff_w, viewport)
                    };
                    let start_y = if let Some(pre_clear_y) = clearance_pre {
                        // CSS 2.1 §9.5.2: a cleared block's border edge sits at the larger of
                        // its natural flow position (margin included) and the cleared float
                        // bottom (`child_y`, advanced by clear_y above). Clearance fills any
                        // gap; the margin is not added a second time on top of the float
                        // bottom. `natural_border` is the pre-clearance border-top.
                        let natural_border = pre_clear_y
                            - prev_block_mb.min(collapsed_mt.max(0.0)) + collapsed_mt;
                        natural_border.max(child_y) - own_mt
                    } else if is_block {
                        if is_first_inflow
                            && b_collapses_top
                            && matches!(child.kind, BoxKind::Block)
                            && child.style.clear == ClearSide::None
                        {
                            // Parent↔first-child collapse: the margin escaped up into this box's
                            // own (already-applied) top margin. Place the child flush at the
                            // content top; lay_out re-adds own_mt, so pre-subtract it.
                            content_y - own_mt
                        } else {
                            child_y - prev_block_mb.min(collapsed_mt.max(0.0)) + collapsed_mt - own_mt
                        }
                    } else {
                        child_y
                    };

                    lay_out_inner(child, eff_left, start_y, eff_w,
                            children_available_height, measurer, viewport, children_pcb, hp,
                            !child_is_root_element, outer_for_child, s.justify_items, None);
                    if matches!(child.kind, BoxKind::Skip) {
                        // Zero-height; does not break the collapsing chain.
                        continue;
                    }
                    seen_inflow_child = true;
                    // CSS 2.1 §8.3.1: the child's effective bottom margin is its own
                    // bottom margin folded with any bottom margin escaping from its
                    // last-child chain (collapse-through), mirroring `collapsed_mt` on
                    // the top edge. For non-block kinds this is just the own margin.
                    let child_mb = collapsed_bottom_margin(child, content_width, viewport);
                    child_y = child.rect.y + child.rect.height + child_mb;
                    // CSS 2.1 §10.8 — inline-image line-box descent (the classic
                    // "image bottom gap"). `<video>`/`<canvas>`/`<iframe>` are
                    // inline-level replaced media that Lumen still lays out as
                    // block-flow children (`default_display` maps them to Block),
                    // so the sub-baseline space of their line box would be dropped
                    // and every media-wrapping block would come out ~descent px too
                    // short; in a grid that shortfall accumulates as an upward row
                    // drift versus a browser (BUG-180, TEST-18). Add the strut
                    // descent of *this block's* font after a baseline-aligned such
                    // child. Restricted to the default `vertical-align: baseline`;
                    // top/middle/bottom anchor the replaced box against the line box
                    // differently and get no sub-baseline gap.
                    //
                    // `BoxKind::Image` is deliberately NOT in this list since IFC-2:
                    // an `<img>` is inline-level for real now and gets its descent
                    // from the `InlineBlockRow` strut. Reaching block flow at all
                    // means the author blockified it (`display: block`, a float,
                    // absolute positioning) — and a blockified box has no line box
                    // and therefore no gap under it.
                    let child_is_replaced_media = matches!(
                        child.kind,
                        BoxKind::Video { .. } | BoxKind::Canvas { .. } | BoxKind::Iframe { .. }
                    );
                    if child_is_replaced_media
                        && matches!(child.style.vertical_align, VerticalAlign::Baseline)
                    {
                        child_y += measurer.map_or(0.0, |m| m.descent_px(b.style.font_size));
                    }
                    prev_block_mb = if is_block { child_mb.max(0.0) } else { 0.0 };
                }
                // CSS 2.1 §8.3.1: parent↔last-child bottom margin collapse. When this
                // box collapses its bottom margin (auto height, no bottom padding/border,
                // no BFC) and the last in-flow child is a collapsible block, that child's
                // (collapsed) bottom margin escapes out of this box rather than enlarging
                // its content height — it becomes part of this box's own bottom margin
                // (reported to the parent loop via `collapsed_bottom_margin`). Only fold
                // it out when no float extends past the last child's flow bottom.
                let escaped_bottom = if b_collapses_bottom {
                    last_collapsible_child(b)
                        .map(|c| collapsed_bottom_margin(c, content_width, viewport))
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                // CSS 2.1 §9.5: the container height must also enclose all floats.
                let float_bottom = fc.left.iter().chain(fc.right.iter())
                    .map(|(bot, _)| *bot)
                    .fold(child_y, f32::max);
                let base = (float_bottom - content_y).max(0.0);
                if escaped_bottom > 0.0 && (float_bottom - child_y).abs() < 0.01 {
                    (base - escaped_bottom).max(0.0)
                } else {
                    base
                }
            };
            // Явная высота (CSS height: Npx) перекрывает auto-высоту по содержимому.
            // box-sizing работает симметрично width: content-box прибавляет
            // padding+border, border-box оставляет h как итоговую высоту.
            b.rect.height = if let Some(h_len) = &s.height {
                if let Some(h) = resolve_block_size(h_len, em, available_height, viewport) {
                    let specified = match s.box_sizing {
                        BoxSizing::ContentBox => h
                            + padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    };
                    // CSS 2.1 §17.5.3: the `height` of a table cell is a minimum — the cell
                    // grows to fit content taller than the specified height (unlike a regular
                    // block, where overflow just spills). Without this the cell clamps to the
                    // specified border-box height and content overflows into the inter-row
                    // border-spacing gap, so row pitch is short by the overflow amount and the
                    // error accumulates down the table (BUG-177).
                    if s.display == Display::TableCell {
                        let content_box = content_height
                            + padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width;
                        specified.max(content_box)
                    } else {
                        specified
                    }
                } else {
                    content_height + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width
                }
            } else if let Some((aw, ah)) = s.aspect_ratio
                && aw > 0.0 && ah > 0.0
            {
                // CSS Sizing L4 §6.1: height auto + aspect-ratio → derive from width.
                // Phase 0: ratio applied in border-box space.
                (b.rect.width * ah / aw).max(0.0)
            } else {
                // CSS Containment L3 §3.3 / CSS Box Sizing L4 §5: size containment
                // suppresses children's contribution to auto height — the box uses
                // contain-intrinsic-height (or 0 when `none`/unset) instead.
                let ch = contained_content_height(size_contained, &s, em, viewport, content_height);
                ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
            };
            // CSS Basic UI L4 §4.4 — field-sizing: content height override.
            // When s.height was not set by UA (field_intrinsic is Some), replace the
            // zero content_height with the padding-box height from the measurement.
            if let Some((_, ph)) = field_intrinsic
                && s.height.is_none()
            {
                b.rect.height = ph + s.border_top_width + s.border_bottom_width;
            }
            // CSS 2.1 §10.4: clamp [min-height, max-height]. Симметрия с
            // width: max сначала, потом min → «min побеждает max». Content
            // оверфлоу-ит коробку если min режет ниже — это правильное
            // поведение CSS.
            let outer_vert = |v: f32| match s.box_sizing {
                BoxSizing::ContentBox => v + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width,
                BoxSizing::BorderBox => v,
            };
            if let Some(max_len) = &s.max_height
                && let Some(max_h) = resolve_block_size(max_len, em, available_height, viewport)
            {
                b.rect.height = b.rect.height.min(outer_vert(max_h).max(0.0));
            }
            if let Some(min_len) = &s.min_height
                && let Some(min_h) = resolve_block_size(min_len, em, available_height, viewport)
            {
                b.rect.height = b.rect.height.max(outer_vert(min_h.max(0.0)));
            }
        }
        BoxKind::InlineBlockRow => {
            // Двухфазный горизонтальный layout с переносом строк и
            // vertical-align (CSS 2.1 §9.4.3 + §10.8).
            //
            // Фаза 1: расставляем детей по X, группируем в строки.
            // Фаза 2: применяем вертикальное выравнивание внутри каждой строки.
            //
            // IFC strut (CSS §10.8 / верифицировано pixel-diff TEST-11/TEST-12):
            // strut участвует в высоте строки только если в ней есть хотя бы один
            // элемент с vertical-align: baseline (явный или InlineRun). Для строк,
            // где все элементы используют top/bottom/middle, strut не нужен —
            // baseline вообще не задействован (Edge/Blink подтверждено).
            // Strut — content area шрифта ряда БЕЗ half-leading, и это осознанное
            // расхождение со спекой, а не упрощение. `line-height: normal` в этом
            // движке — 1.2em, тогда как у настоящего шрифта это ascent + descent +
            // lineGap, то есть почти ровно content area; добавив half-leading от
            // 1.2em, строка из одних atomic inline становится на ~1.3px выше, чем
            // в Edge, и TEST-02/04/21/56 (ряды пустых inline-block) уходят в FAIL
            // на 0.68 % при пороге 0.5 %. Измерено A/B, IFC-1. Строки с текстом
            // это не задевает: у прогона своё half-leading, и оно всегда больше.
            let strut_descent = measurer.map_or(0.0, |m| m.descent_px(b.style.font_size));
            let strut_ascent = measurer.map_or(0.0, |m| m.ascent_px(b.style.font_size));
            // Half x-height of the row's font: locates `vertical-align: middle`
            // relative to the baseline (CSS 2.1 §10.8.1).
            let x_half = measurer.map_or(0.0, |m| m.x_height_px(b.style.font_size)) / 2.0;
            // Метрики каждого ребёнка как участника строки: ascent — от верхней
            // кромки margin box до его базовой линии, descent — остаток margin box
            // под ней. Считаются сразу после раскладки ребёнка, потому что фазе 1
            // нужна итоговая высота строки, чтобы сдвинуть cur_y (CSS 2.1 §10.8).
            let mut metrics: Vec<(f32, f32)> = vec![(0.0, 0.0); b.children.len()];
            // rows: (row_y, above, below, Vec<child_index>)
            let mut rows: Vec<(f32, f32, f32, Vec<usize>)> = Vec::new();
            let mut cur_x = content_x;
            let mut cur_y = content_y;
            let mut row_y = cur_y;
            let mut cur_row: Vec<usize> = Vec::new();
            let mut row_has_baseline = false;
            let mut total_h: f32 = 0.0;

            // CSS 2.1 §10.8 — размер line box: базовая линия ставится так, чтобы
            // вместить всех выровненных по ней участников, после чего top/bottom
            // раздвигают строку в противоположную сторону.
            let line_metrics = |children: &[LayoutBox],
                                metrics: &[(f32, f32)],
                                idxs: &[usize],
                                has_baseline: bool| -> (f32, f32) {
                let (mut above, mut below) = if has_baseline {
                    (strut_ascent, strut_descent)
                } else {
                    (0.0, 0.0)
                };
                for &idx in idxs {
                    let (a, d) = metrics[idx];
                    match inline_v_align(&children[idx]) {
                        VerticalAlign::Baseline => {
                            above = above.max(a);
                            below = below.max(d);
                        }
                        // `middle` совмещает центр бокса с (базовая линия − x/2),
                        // а не с центром line box: высокий top/bottom-участник
                        // уводит базовую линию от середины (BUG-182, TEST-24 row1).
                        VerticalAlign::Middle => {
                            above = above.max((a + d) / 2.0 + x_half);
                            below = below.max((a + d) / 2.0 - x_half);
                        }
                        _ => {}
                    }
                }
                for &idx in idxs {
                    let (a, d) = metrics[idx];
                    let fh = a + d;
                    match inline_v_align(&children[idx]) {
                        VerticalAlign::Top | VerticalAlign::TextTop => below = below.max(fh - above),
                        VerticalAlign::Bottom | VerticalAlign::TextBottom => {
                            above = above.max(fh - below)
                        }
                        _ => {}
                    }
                }
                (above, below)
            };

            for i in 0..b.children.len() {
                // InlineSpace: collapsed whitespace gap — advance cur_x only.
                if matches!(b.children[i].kind, BoxKind::InlineSpace) {
                    let space_w = measurer.map_or(0.0, |m| m.char_width(' ', b.style.font_size));
                    cur_x += space_w;
                    continue;
                }
                let is_run = matches!(b.children[i].kind, BoxKind::InlineRun { .. });
                // Схлопнутый пробел в начале текста существует только пока текст
                // не первый на строке: `wrap_inline_run` срезает его как пробел в
                // начале строки, поэтому зазор после atomic inline даёт этот сдвиг.
                let lead = if is_run && cur_x > content_x {
                    inline_run_lead_space(&b.children[i], measurer)
                } else {
                    0.0
                };
                // Snap inline-block x to integer CSS pixels (Chrome/Edge behaviour at DPR=1).
                // InlineSpace uses float advance (font metrics); accumulated sub-pixel error
                // would shift all subsequent elements by up to 1px relative to Edge.
                let place_x = if is_run { cur_x + lead } else { cur_x.floor() };
                let child_avail = if is_run {
                    (content_width - (place_x - content_x)).max(0.0)
                } else {
                    content_width
                };
                lay_out(&mut b.children[i], place_x, cur_y, child_avail, None, measurer, viewport, children_pcb, hp, false);
                if matches!(b.children[i].kind, BoxKind::Skip) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let child_mr = b.children[i].style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                let child_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let child_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                // Продвижение по строке: у текста — по последней строке прогона,
                // у остальных — по border box (см. `inline_run_advance`).
                let mut advance = if is_run {
                    inline_run_advance(&b.children[i], measurer)
                } else {
                    b.children[i].rect.width
                };
                let child_right = b.children[i].rect.x + advance + child_mr;

                if !is_run && child_right > content_x + content_width && cur_x > content_x {
                    let (above, below) =
                        line_metrics(&b.children, &metrics, &cur_row, row_has_baseline);
                    rows.push((row_y, above, below, std::mem::take(&mut cur_row)));
                    // Snap to integer CSS pixels (Chrome/Edge DPR=1 behaviour): fractional
                    // IFC strut from font metrics (descent_px) would otherwise drift row
                    // y-positions by sub-pixel amounts relative to a browser with a different
                    // default font.
                    let new_y = (cur_y + above + below).round();
                    let actual_spacing = new_y - cur_y;
                    total_h += actual_spacing;
                    cur_y = new_y;
                    row_y = cur_y;
                    cur_x = content_x;
                    row_has_baseline = false;
                    lay_out(&mut b.children[i], cur_x, cur_y, content_width, None, measurer, viewport, children_pcb, hp, false);
                    advance = b.children[i].rect.width;
                }
                cur_row.push(i);
                if matches!(inline_v_align(&b.children[i]), VerticalAlign::Baseline) {
                    row_has_baseline = true;
                }
                let fh = child_mt + b.children[i].rect.height + child_mb;
                // Нет собственной базовой линии — выравнивание по нижней кромке
                // margin box (CSS 2.1 §10.8.1).
                let asc = match inline_baseline(&b.children[i], measurer) {
                    Some(bl) => child_mt + bl,
                    None => fh,
                };
                metrics[i] = (asc, fh - asc);
                cur_x = b.children[i].rect.x + advance + child_mr;
            }
            let (last_above, last_below) =
                line_metrics(&b.children, &metrics, &cur_row, row_has_baseline);
            if !cur_row.is_empty() {
                rows.push((row_y, last_above, last_below, cur_row));
                b.rect.height = total_h + last_above + last_below;
            } else {
                b.rect.height = total_h;
            }

            // Фаза 2: vertical-align (CSS 2.1 §10.8.1). Дети сейчас стоят border
            // box'ом на верхней кромке строки; сдвигаем каждого туда, куда его
            // ставит его собственное выравнивание.
            let mut adjustments: Vec<(usize, f32)> = Vec::new();
            for (row_top, above, below, child_idxs) in &rows {
                for &idx in child_idxs {
                    let (a, d) = metrics[idx];
                    let fh = a + d;
                    let c_em = b.children[idx].style.font_size;
                    let mt = b.children[idx]
                        .style
                        .margin_top
                        .resolve_or_zero(c_em, content_width, viewport);
                    // Верхняя кромка margin box относительно верха строки.
                    let margin_top_y = match inline_v_align(&b.children[idx]) {
                        VerticalAlign::Baseline => above - a,
                        VerticalAlign::Bottom | VerticalAlign::TextBottom => above + below - fh,
                        VerticalAlign::Top | VerticalAlign::TextTop => 0.0,
                        VerticalAlign::Middle => above - x_half - fh / 2.0,
                        _ => 0.0,
                    };
                    let dy = row_top + margin_top_y + mt - b.children[idx].rect.y;
                    if dy.abs() > 0.001 {
                        adjustments.push((idx, dy));
                    }
                }
            }
            for (idx, dy) in adjustments {
                // Round dy to integer CSS pixels so vertical-aligned children land on
                // whole-pixel boundaries, matching the .round() applied to IFC row y-positions
                // above. Fractional dy causes 0.99% deviation vs Edge (BUG-081).
                shift_y_box(&mut b.children[idx], dy.round());
            }
        }
        BoxKind::TableRow => {
            // CSS 2.1 §17.5 — table row: ячейки раскладываются горизонтально.
            // col_widths=None → per-row auto-distribution (standalone <tr> outside <table>).
            let row_h = lay_out_table_row(
                b, content_x, content_y, content_width, None, None, 0.0, None, measurer, viewport, children_pcb, hp,
            );
            b.rect.height = if let Some(h_len) = &s.height
                && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
            {
                match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                }
            } else {
                row_h + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width
            };
        }
        BoxKind::Table => {
            // CSS 2.1 §17 / §17.5.2 — table container: compute global column widths, lay out rows.
            // When no explicit CSS width is given, tables use shrink-to-fit: the table box is
            // only as wide as its columns require (total column widths + border-spacing gaps).
            // This differs from block elements which fill the available inline size.
            if s.width.is_none() {
                let intrinsic = table_intrinsic_content_width(b, viewport);
                if intrinsic > 0.0 && intrinsic < content_width {
                    b.rect.width = (intrinsic + padding_left + padding_right
                        + s.border_left_width + s.border_right_width).max(0.0);
                    content_width = intrinsic;
                }
            }
            let content_height = lay_out_table(
                b, content_x, content_y, content_width, measurer, viewport, children_pcb, hp,
            );
            if let Some(h_len) = &s.height
                && let Some(h) = resolve_block_size(h_len, em, available_height, viewport)
            {
                b.rect.height = match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                };
            } else if !matches!(s.border_collapse, BorderCollapse::Collapse) {
                // Collapse mode sets b.rect.height directly in lay_out_table (the table border-box
                // coincides with the outer cells' collapsed borders).
                b.rect.height = content_height + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width;
            }
        }
        BoxKind::TableRowGroup => {
            // CSS 2.1 §17 — row group standalone (outside a <table>): block-flow of rows.
            // When inside a Table, rows are handled directly by lay_out_table.
            let mut cur_y = content_y;
            for i in 0..b.children.len() {
                if !matches!(b.children[i].kind, BoxKind::TableRow) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                lay_out(&mut b.children[i], content_x, cur_y + c_mt, content_width, None, measurer, viewport, children_pcb, hp, false);
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb;
            }
            b.rect.height = (cur_y - content_y) + padding_top + padding_bottom
                + s.border_top_width + s.border_bottom_width;
        }
        BoxKind::InlineRun { .. } => unreachable!(),
        BoxKind::InlineSpace => unreachable!(),
        BoxKind::Skip => unreachable!(),
        BoxKind::Contents => unreachable!("display:contents boxes must be flattened before lay_out"),
        BoxKind::Marker { .. } => {
            // Rect is set by the parent's block-flow loop; nothing to do here.
        }
        // SvgRoot, SvgShape, and SvgText are dispatched before this match (early return above).
        BoxKind::SvgRoot { .. } | BoxKind::SvgShape { .. } | BoxKind::SvgText { .. } => unreachable!(),
    }

    // CSS Positioned Layout L3 §4 — абсолютное / фиксированное позиционирование.
    // Деферированные дети (abs_deferred) собраны в Block-ветке выше.
    // Обрабатываем после finalize b.rect.height, чтобы знать высоту containing block.
    if !abs_deferred.is_empty() {
        let my_pcb = if is_positioned {
            // CSS Position L3 §2.2: CB for absolute descendants = padding edge.
            Rect::new(
                b.rect.x + s.border_left_width,
                b.rect.y + s.border_top_width,
                (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
            )
        } else {
            pcb
        };
        lay_out_abs_children(b, &abs_deferred, measurer, viewport, my_pcb, hp);
    }

    // CSS Positioned Layout L3 §9.4.3 — position: relative — смещение после normal flow.
    if matches!(s.position, Position::Relative) {
        let off_x = match &s.left {
            LengthOrAuto::Length(l) => l.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.right {
                LengthOrAuto::Length(r) => -(r.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        let off_y = match &s.top {
            LengthOrAuto::Length(t) => t.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.bottom {
                LengthOrAuto::Length(bot) => -(bot.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        if off_x != 0.0 || off_y != 0.0 {
            shift_tree(b, off_x, off_y);
        }
    }
    // CSS: position: sticky — treated as normal flow here; inset values (top/right/
    // bottom/left) are resolved from ComputedStyle in lib.rs::collect_sticky_rec()
    // after this pass. P3 calls collect_sticky_boxes() + compute_sticky_offset() to
    // apply scroll-driven paint transforms at render time.
}

/// CSS 2.1 §17.5 — table row layout with colspan/rowspan support.
///
/// Algorithm:
/// 1. Map each cell to its starting column (skipping rowspan-occupied columns).
/// 2. Determine cell width: sum of spanned `col_widths` columns, or explicit CSS width.
/// 3. Place cells horizontally; use column-position x when `col_widths` is present.
/// 4. Normalise heights: non-rowspan cells all get the max row height.
///    Rowspan cells keep their laid-out height; `lay_out_table` fixes them after all rows.
/// 5. Register new rowspan occupancy in `rowspan_map` (caller decrements after the row).
///
/// Returns content height (without the row's own padding/border).
#[allow(clippy::too_many_arguments)]
fn lay_out_table_row(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    col_widths: Option<&[f32]>,
    // None for standalone <tr> outside <table>; caller must call decrement_rowspan_map after return.
    rowspan_map: Option<&mut Vec<u32>>,
    // Horizontal gap between adjacent cells (from table's border-spacing-h). 0.0 for standalone rows.
    h_spacing: f32,
    // CSS 2.1 §17.6.2 collapsed border model: absolute x of each column's cell border-box left
    // edge (length = n_cols). When present, cells are positioned here so adjacent borders overlap
    // by the collapsed grid-line width instead of being spaced by `h_spacing`.
    collapse_col_x: Option<&[f32]>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let cell_idxs: Vec<usize> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    let n = cell_idxs.len();
    if n == 0 {
        return 0.0;
    }

    // Step 1 + 2: map cells to (col_start, cell_width).
    // `cell_cols[j]` = (starting column index, border-box width to allocate).
    let cell_cols: Vec<(usize, f32)> = if let Some(cw) = col_widths {
        // Pre-computed table-wide column widths are authoritative.
        // Skip columns occupied by rowspan cells from prior rows.
        let empty: Vec<u32> = Vec::new();
        let rsmap: &[u32] = rowspan_map
            .as_deref()
            .map(|v: &Vec<u32>| v.as_slice())
            .unwrap_or(empty.as_slice());
        let mut col_pos = 0usize;
        let mut result = Vec::with_capacity(n);
        for &i in &cell_idxs {
            while col_pos < rsmap.len() && rsmap[col_pos] > 0 {
                col_pos += 1;
            }
            let span = b.children[i].col_span.max(1) as usize;
            let w: f32 = (col_pos..col_pos + span)
                .map(|c| cw.get(c).copied().unwrap_or(0.0))
                .sum();
            result.push((col_pos, w));
            col_pos += span;
        }
        result
    } else {
        // No pre-computed widths: derive from each cell's explicit CSS width.
        let mut explicit_w: Vec<Option<f32>> = Vec::with_capacity(n);
        let mut total_explicit = 0.0_f32;
        let mut auto_count: usize = 0;
        for &i in &cell_idxs {
            let c = &b.children[i];
            let em = c.style.font_size;
            if let Some(w_len) = &c.style.width
                && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
            {
                let border_w = match c.style.box_sizing {
                    BoxSizing::ContentBox => {
                        let pl = c.style.padding_left.resolve_or_zero(em, content_width, viewport);
                        let pr = c.style.padding_right.resolve_or_zero(em, content_width, viewport);
                        w + pl + pr + c.style.border_left_width + c.style.border_right_width
                    }
                    BoxSizing::BorderBox => w,
                };
                explicit_w.push(Some(border_w));
                total_explicit += border_w;
                continue;
            }
            explicit_w.push(None);
            auto_count += 1;
        }
        let auto_share = if auto_count > 0 {
            ((content_width - total_explicit) / auto_count as f32).max(0.0)
        } else {
            0.0
        };
        // Standalone row: sequential column assignment (cell j → column j).
        (0..n)
            .map(|j| (j, explicit_w[j].unwrap_or(auto_share)))
            .collect()
    };

    // Step 3: lay out each cell at its column x position.
    // When col_widths is present, the column width is authoritative — clear the cell's CSS
    // `width` temporarily so lay_out uses `avail` as the final width.
    let use_global = col_widths.is_some();
    for (j, &i) in cell_idxs.iter().enumerate() {
        let (col_start, avail) = cell_cols[j];
        let cell_x = if let Some(cx) = collapse_col_x {
            // Collapsed border model: each column has a precomputed absolute x at which its
            // cells' border-box starts; adjacent cells overlap by the shared grid-line border.
            cx.get(col_start).copied().unwrap_or(content_x)
        } else if use_global {
            // Exact x from column positions, accounting for h_spacing slots.
            // Cell at col_start k: content_x + (k+1)*h_spacing + sum(col_widths[0..k]).
            content_x
                + (col_start + 1) as f32 * h_spacing
                + (0..col_start)
                    .map(|c| col_widths.and_then(|cw| cw.get(c)).copied().unwrap_or(0.0))
                    .sum::<f32>()
        } else {
            // Standalone row: use prior cell's right edge.
            if j == 0 {
                content_x
            } else {
                let prev_i = cell_idxs[j - 1];
                let c = &b.children[prev_i];
                let c_em = c.style.font_size;
                let mr = c.style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                c.rect.x + c.rect.width + mr
            }
        };
        let saved_width =
            if use_global { Arc::make_mut(&mut b.children[i].style).width.take() } else { None };
        lay_out(
            &mut b.children[i],
            cell_x,
            content_y,
            avail,
            None,
            measurer,
            viewport,
            pcb,
            hp,
            false,
        );
        if use_global {
            Arc::make_mut(&mut b.children[i].style).width = saved_width;
        }
    }

    // Register rowspan occupancy. Value = row_span (not row_span-1) because the caller
    // calls decrement_rowspan_map after this row, leaving row_span-1 remaining rows occupied.
    if let Some(rsmap) = rowspan_map {
        for (j, &i) in cell_idxs.iter().enumerate() {
            if b.children[i].row_span > 1 {
                let (col_start, _) = cell_cols[j];
                let span = b.children[i].col_span.max(1) as usize;
                let end_col = col_start + span;
                if end_col > rsmap.len() {
                    rsmap.resize(end_col, 0);
                }
                let rs = b.children[i].row_span;
                for v in rsmap.iter_mut().skip(col_start).take(span) {
                    if *v < rs {
                        *v = rs;
                    }
                }
            }
        }
    }

    // Step 4: normalise heights — non-rowspan cells all become the max row height.
    // Rowspan > 1 cells keep their own height; lay_out_table patches them later.
    let row_h = cell_idxs
        .iter()
        .filter(|&&i| b.children[i].row_span == 1)
        .map(|&i| b.children[i].rect.height)
        .fold(0.0_f32, f32::max);
    for &i in &cell_idxs {
        if b.children[i].row_span == 1 {
            b.children[i].rect.height = row_h;
        }
    }

    row_h
}

/// CSS 2.1 §17.6.2 — collapsed vertical border width at each column grid line for a table.
///
/// Returns a `Vec<f32>` of length `n_cols + 1`. Index `k` (1..n_cols) is the shared border
/// width between column `k-1` and column `k`: the maximum, over every row, of the right border
/// of the cell in column `k-1` and the left border of the cell in column `k`. Indices `0` and
/// `n_cols` (the outer edges) are left at `0.0` — outer cells are snapped onto the table border
/// by the caller, so their grid-line width is handled there. Cells are mapped to columns by
/// sequential order (colspan/rowspan are not accounted for; collapse overlap is exact only for
/// simple uniform grids, which is the common case).
fn collapse_v_edges(b: &LayoutBox, n_cols: usize) -> Vec<f32> {
    let mut edges = vec![0.0_f32; n_cols + 1];
    let mut visit = |row: &LayoutBox| {
        let cells: Vec<&LayoutBox> = row
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        for col in 1..cells.len().min(n_cols) {
            let edge = cells[col].style.border_left_width.max(cells[col - 1].style.border_right_width);
            edges[col] = edges[col].max(edge);
        }
    };
    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => visit(child),
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        visit(row);
                    }
                }
            }
            _ => {}
        }
    }
    edges
}

/// CSS 2.1 §17.6.2 — representative collapsed horizontal (row-to-row) border width.
///
/// Returns the maximum top/bottom border width across all cells in the table. Used as a uniform
/// row overlap in collapse mode: consecutive rows are pulled together by this amount so their
/// shared horizontal grid line renders as one border instead of two stacked ones. Uniform (rather
/// than per-row-pair) is exact when row borders are consistent — the common case.
fn collapse_max_cross_border(b: &LayoutBox) -> f32 {
    let mut max_b = 0.0_f32;
    let mut visit = |row: &LayoutBox| {
        for cell in row.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)) {
            max_b = max_b.max(cell.style.border_top_width).max(cell.style.border_bottom_width);
        }
    };
    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => visit(child),
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        visit(row);
                    }
                }
            }
            _ => {}
        }
    }
    max_b
}

/// CSS 2.1 §17 — table layout with colspan/rowspan support.
///
/// Pass 1: compute column widths (span-aware), lay out rows top-to-bottom while tracking
/// rowspan occupancy and collecting spanning cells.
/// Pass 2: fix spanning cell heights — each rowspan cell's height is extended to cover
/// the bottom edge of its last spanned row.
///
/// Returns content height.
#[allow(clippy::too_many_arguments)]
fn lay_out_table(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    // CSS Tables L2 §17.6: collapse mode zeroes out border-spacing.
    let collapse = matches!(b.style.border_collapse, BorderCollapse::Collapse);
    let (h_spacing, v_spacing) = match b.style.border_collapse {
        BorderCollapse::Collapse => (0.0, 0.0),
        BorderCollapse::Separate => (b.style.border_spacing_h, b.style.border_spacing_v),
    };

    let col_widths = compute_table_col_widths(b, content_width, viewport, measurer);

    // CSS 2.1 §17.6.2 — collapsing border model. Adjacent cell borders (and the table's own
    // border with the outer cells) share a single grid line whose width is the larger of the
    // two meeting borders. We model this by positioning columns so neighbouring cells overlap
    // by that collapsed width, and by snapping the outer cells onto the table border (so a 2px
    // table border + 2px cell border render as one 2px line, not 4px). Width/colour conflict
    // resolution is approximated by max-width (sufficient for same-style same-colour grids).
    let n_cols = col_widths.len();
    let (collapse_col_x, collapse_v_overlap, collapse_width) = if collapse && n_cols > 0 {
        let v_edges = collapse_v_edges(b, n_cols);
        // base_x = table border-box left edge: outer cell borders coincide with the table border.
        let base_x = b.rect.x;
        let mut col_x = Vec::with_capacity(n_cols);
        col_x.push(base_x);
        for k in 1..n_cols {
            let prev = col_x[k - 1] + col_widths[k - 1] - v_edges[k];
            col_x.push(prev);
        }
        let total_w = (col_x[n_cols - 1] + col_widths[n_cols - 1] - base_x).max(0.0);
        (Some(col_x), collapse_max_cross_border(b), total_w)
    } else {
        (None, 0.0, 0.0)
    };
    let collapse_col_x_ref = collapse_col_x.as_deref();

    // First row starts after the top outer v_spacing slot; in collapse mode the first row's top
    // border coincides with the table's top border (start at the table border-box top edge).
    let mut cur_y = if collapse { b.rect.y } else { content_y + v_spacing };
    let mut rowspan_map: Vec<u32> = Vec::new();

    // flat_row_rects[k] = (y, height) for the k-th row in DOM order (across all groups).
    let mut flat_row_rects: Vec<(f32, f32)> = Vec::new();

    // Spanning cells that need height post-fix:
    // (group: Option<usize>, row_in_group: usize, child_idx: usize, start_flat: usize, span: u32)
    let mut span_fixes: Vec<(Option<usize>, usize, usize, usize, u32)> = Vec::new();

    let n = b.children.len();
    for i in 0..n {
        match b.children[i].kind {
            BoxKind::TableRow => {
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let row_y = cur_y + c_mt;
                b.children[i].rect.x = content_x;
                b.children[i].rect.y = row_y;
                b.children[i].rect.width = content_width;
                let flat_idx = flat_row_rects.len();
                let row_h = lay_out_table_row(
                    &mut b.children[i],
                    content_x, row_y, content_width,
                    Some(&col_widths),
                    Some(&mut rowspan_map),
                    h_spacing,
                    collapse_col_x_ref,
                    measurer, viewport, pcb, hp,
                );
                let row_style_h = {
                    let s = &b.children[i].style;
                    if let Some(h_len) = &s.height
                        && let Some(h) = h_len.resolve(s.font_size, None, viewport)
                    {
                        let pt = s.padding_top.resolve_or_zero(s.font_size, content_width, viewport);
                        let pb = s.padding_bottom.resolve_or_zero(s.font_size, content_width, viewport);
                        match s.box_sizing {
                            BoxSizing::ContentBox => (h + pt + pb + s.border_top_width + s.border_bottom_width).max(0.0),
                            BoxSizing::BorderBox => h.max(pt + pb + s.border_top_width + s.border_bottom_width),
                        }
                    } else {
                        let pt = b.children[i].style.padding_top.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        let pb = b.children[i].style.padding_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        row_h + pt + pb + b.children[i].style.border_top_width + b.children[i].style.border_bottom_width
                    }
                };
                b.children[i].rect.height = row_style_h;
                flat_row_rects.push((b.children[i].rect.y, row_style_h));
                // Collect spanning cells for post-fix.
                for (ci, child) in b.children[i].children.iter().enumerate() {
                    if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                        span_fixes.push((None, i, ci, flat_idx, child.row_span));
                    }
                }
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                // Add v_spacing gap after each row (outer bottom slot included); in collapse mode
                // pull the next row up by the shared horizontal grid-line border instead.
                // CSS: border-spacing
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb + v_spacing - collapse_v_overlap;
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                let group_em = b.children[i].style.font_size;
                let g_mt = b.children[i].style.margin_top.resolve_or_zero(group_em, content_width, viewport);
                let group_y = cur_y + g_mt;
                b.children[i].rect.x = content_x;
                b.children[i].rect.y = group_y;
                b.children[i].rect.width = content_width;
                let mut row_y = group_y;
                let n_rows = b.children[i].children.len();
                for r in 0..n_rows {
                    if !matches!(b.children[i].children[r].kind, BoxKind::TableRow) {
                        continue;
                    }
                    let flat_idx = flat_row_rects.len();
                    let r_em = b.children[i].children[r].style.font_size;
                    let r_mt = b.children[i].children[r].style.margin_top.resolve_or_zero(r_em, content_width, viewport);
                    b.children[i].children[r].rect.x = content_x;
                    b.children[i].children[r].rect.y = row_y + r_mt;
                    b.children[i].children[r].rect.width = content_width;
                    let row_h = lay_out_table_row(
                        &mut b.children[i].children[r],
                        content_x, row_y + r_mt, content_width,
                        Some(&col_widths),
                        Some(&mut rowspan_map),
                        h_spacing,
                        collapse_col_x_ref,
                        measurer, viewport, pcb, hp,
                    );
                    let r_pt = b.children[i].children[r].style.padding_top.resolve_or_zero(r_em, content_width, viewport);
                    let r_pb = b.children[i].children[r].style.padding_bottom.resolve_or_zero(r_em, content_width, viewport);
                    let r_bor = b.children[i].children[r].style.border_top_width + b.children[i].children[r].style.border_bottom_width;
                    let row_style_h = row_h + r_pt + r_pb + r_bor;
                    b.children[i].children[r].rect.height = row_style_h;
                    flat_row_rects.push((b.children[i].children[r].rect.y, row_style_h));
                    // Collect spanning cells for post-fix.
                    for (ci, child) in b.children[i].children[r].children.iter().enumerate() {
                        if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                            span_fixes.push((Some(i), r, ci, flat_idx, child.row_span));
                        }
                    }
                    let r_mb = b.children[i].children[r].style.margin_bottom.resolve_or_zero(r_em, content_width, viewport);
                    // CSS: border-spacing — collapse mode pulls rows together by the shared border.
                    row_y = b.children[i].children[r].rect.y + b.children[i].children[r].rect.height + r_mb + v_spacing - collapse_v_overlap;
                    decrement_rowspan_map(&mut rowspan_map);
                }
                let g_pt = b.children[i].style.padding_top.resolve_or_zero(group_em, content_width, viewport);
                let g_pb = b.children[i].style.padding_bottom.resolve_or_zero(group_em, content_width, viewport);
                let g_bor = b.children[i].style.border_top_width + b.children[i].style.border_bottom_width;
                b.children[i].rect.height = (row_y - group_y) + g_pt + g_pb + g_bor;
                let g_mb = b.children[i].style.margin_bottom.resolve_or_zero(group_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + g_mb;
            }
            _ => {}
        }
    }

    // Pass 2: fix rowspan cell heights.
    // Each spanning cell's height is extended to reach the bottom of its last spanned row.
    for (group, row, child_idx, start_flat, span) in span_fixes {
        let end_flat = (start_flat + span as usize).min(flat_row_rects.len());
        if end_flat == 0 {
            continue;
        }
        let (last_y, last_h) = flat_row_rects[end_flat - 1];
        let target_bottom = last_y + last_h;
        let cell = match group {
            None => &mut b.children[row].children[child_idx],
            Some(g) => &mut b.children[g].children[row].children[child_idx],
        };
        let new_h = (target_bottom - cell.rect.y).max(cell.rect.height);
        cell.rect.height = new_h;
    }

    // CSS 2.1 §17.6.2 — collapsing model: the table border-box coincides with the outer cells'
    // shared borders. Snap the table width to the overlapped grid and the height to the bottom
    // edge of the last row (which already includes the collapsed top/bottom borders). The caller
    // skips its own height computation in collapse mode, so set it here for every collapse table
    // (an empty table with no rows collapses to a zero-height border-box).
    if collapse {
        if b.style.width.is_none() && n_cols > 0 {
            b.rect.width = collapse_width;
        }
        if b.style.height.is_none() {
            b.rect.height = flat_row_rects
                .last()
                .map(|&(last_y, last_h)| (last_y + last_h - b.rect.y).max(0.0))
                .unwrap_or(0.0);
        }
    }

    (cur_y - content_y).max(0.0)
}

/// Scans `row`'s cells and updates `col_explicit` with per-column explicit border-box
/// widths. Colspan cells distribute their width evenly across spanned columns.
/// Rowspan cells register occupancy in `rowspan_map` for subsequent rows.
/// Caller must call `decrement_rowspan_map` after processing each row.
fn scan_row_explicit_widths(
    row: &LayoutBox,
    col_explicit: &mut Vec<Option<f32>>,
    rowspan_map: &mut Vec<u32>,
    content_width: f32,
    viewport: Size,
) {
    let cells: Vec<_> = row
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();

    let mut col_pos = 0usize;
    for cell in &cells {
        // Skip columns occupied by rowspan cells from prior rows.
        while col_pos < rowspan_map.len() && rowspan_map[col_pos] > 0 {
            col_pos += 1;
        }

        let span = cell.col_span.max(1) as usize;
        let em = cell.style.font_size;
        let w_border = if let Some(w_len) = &cell.style.width
            && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
        {
            let bw = match cell.style.box_sizing {
                BoxSizing::ContentBox => {
                    let pl = cell.style.padding_left.resolve_or_zero(em, content_width, viewport);
                    let pr = cell.style.padding_right.resolve_or_zero(em, content_width, viewport);
                    w + pl + pr + cell.style.border_left_width + cell.style.border_right_width
                }
                BoxSizing::BorderBox => w,
            };
            Some(bw)
        } else {
            None
        };

        let end_col = col_pos + span;
        if end_col > col_explicit.len() {
            col_explicit.resize(end_col, None);
        }
        // Distribute the cell's explicit width evenly across its spanned columns.
        if let Some(total_w) = w_border {
            let per_col = total_w / span as f32;
            for slot in col_explicit.iter_mut().skip(col_pos).take(span) {
                *slot = Some(match *slot {
                    Some(existing) => existing.max(per_col),
                    None => per_col,
                });
            }
        }

        // Register rowspan occupancy. Value = row_span (decrement_rowspan_map brings it to
        // row_span-1 after this row, meaning row_span-1 subsequent rows remain occupied).
        if cell.row_span > 1 {
            if end_col > rowspan_map.len() {
                rowspan_map.resize(end_col, 0);
            }
            let rs = cell.row_span;
            for v in rowspan_map.iter_mut().skip(col_pos).take(span) {
                if *v < rs {
                    *v = rs;
                }
            }
        }

        col_pos = end_col;
    }
}

/// Decrements each entry in `rowspan_map` by 1 (clamped to 0). Call after each row.
fn decrement_rowspan_map(map: &mut [u32]) {
    for v in map.iter_mut() {
        *v = v.saturating_sub(1);
    }
}

/// CSS 2.1 §17.5.2 — minimum (shrink-to-fit) content width for a table box.
///
/// Returns `sum(explicit_column_widths) + (n_cols + 1) * border_spacing_h`.
/// Cells without an explicit CSS width contribute 0 (effectively auto/min-content).
/// Used to shrink `display:table` boxes that have no explicit CSS `width`.
fn table_intrinsic_content_width(b: &LayoutBox, viewport: Size) -> f32 {
    let h_spacing = b.style.border_spacing_h;
    let mut col_explicit: Vec<Option<f32>> = Vec::new();
    let mut rowspan_map: Vec<u32> = Vec::new();
    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => {
                scan_row_explicit_widths(child, &mut col_explicit, &mut rowspan_map, 0.0, viewport);
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        scan_row_explicit_widths(row, &mut col_explicit, &mut rowspan_map, 0.0, viewport);
                        decrement_rowspan_map(&mut rowspan_map);
                    }
                }
            }
            _ => {}
        }
    }
    let n_cols = col_explicit.len();
    if n_cols == 0 {
        return 0.0;
    }
    let total_explicit: f32 = col_explicit.iter().filter_map(|w| *w).sum();
    total_explicit + (n_cols + 1) as f32 * h_spacing
}

/// CSS 2.1 §17.5.2 — min-content and max-content widths for a slice of boxes.
///
/// Traverses block containers recursively. Block-level items stack vertically —
/// the container's min/max is the max of its children's widths. `InlineRun`
/// items accumulate segments left-to-right for max-content and take the widest
/// whitespace-separated token for min-content.
///
/// Returns `(min_content_width, max_content_width)` as content-box widths
/// (the caller must add the container's own padding + border).
fn box_min_max_content_w(boxes: &[LayoutBox], m: &dyn TextMeasurer, vp: Size) -> (f32, f32) {
    let mut min_w = 0.0f32;
    let mut max_w = 0.0f32;
    for b in boxes {
        let (bmin, bmax) = match &b.kind {
            BoxKind::InlineRun { segments, .. } => {
                let mut line_w = 0.0f32;
                let mut run_max = 0.0f32;
                let mut run_min = 0.0f32;
                for seg in segments {
                    if seg.forced_break {
                        run_max = run_max.max(line_w);
                        line_w = 0.0;
                        continue;
                    }
                    let fs = seg.style.font_size;
                    let ls = seg.style.letter_spacing;
                    if seg.img_src.is_some() {
                        let w = seg.pre_space + seg.img_width + seg.post_space;
                        line_w += w;
                        run_min = run_min.max(w);
                    } else {
                        let fams = &seg.style.font_family;
                        line_w += seg.pre_space
                            + measure_text_w_families(&seg.text, fs, ls, 0.0, fams, m)
                            + seg.post_space;
                        for word in seg.text.split_ascii_whitespace() {
                            run_min = run_min.max(
                                seg.pre_space
                                    + measure_text_w_families(word, fs, ls, 0.0, fams, m)
                                    + seg.post_space,
                            );
                        }
                    }
                }
                run_max = run_max.max(line_w);
                (run_min, run_max)
            }
            BoxKind::Block | BoxKind::FlowRoot | BoxKind::InlineBlockRow => {
                let em = b.style.font_size;
                let pl = b.style.padding_left.resolve_or_zero(em, 0.0, vp);
                let pr = b.style.padding_right.resolve_or_zero(em, 0.0, vp);
                let bw = b.style.border_left_width + b.style.border_right_width;
                let (cmin, cmax) = box_min_max_content_w(&b.children, m, vp);
                (cmin + pl + pr + bw, cmax + pl + pr + bw)
            }
            BoxKind::Skip
            | BoxKind::TableRow
            | BoxKind::TableRowGroup
            | BoxKind::InlineSpace
            | BoxKind::Marker { .. }
            | BoxKind::Contents => (0.0, 0.0),
            // Replaced elements (Image, FormControl, Video, …): use explicit width if set.
            _ => {
                let em = b.style.font_size;
                if let Some(wl) = &b.style.width
                    && let Some(w) = wl.resolve(em, None, vp)
                    && w > 0.0
                {
                    (w, w)
                } else {
                    (0.0, 0.0)
                }
            }
        };
        min_w = min_w.max(bmin);
        max_w = max_w.max(bmax);
    }
    (min_w, max_w)
}

/// Returns `(min_content_border_box, max_content_border_box)` for a single table cell,
/// including the cell's own horizontal padding and border.
fn cell_min_max_border_box_w(cell: &LayoutBox, m: &dyn TextMeasurer, vp: Size) -> (f32, f32) {
    let em = cell.style.font_size;
    let pl = cell.style.padding_left.resolve_or_zero(em, 0.0, vp);
    let pr = cell.style.padding_right.resolve_or_zero(em, 0.0, vp);
    let bw = cell.style.border_left_width + cell.style.border_right_width;
    let horiz = pl + pr + bw;
    let (cmin, cmax) = box_min_max_content_w(&cell.children, m, vp);
    (cmin + horiz, cmax + horiz)
}

/// Scans `row`'s cells and updates `col_min`/`col_max` with per-column content-based widths.
/// Colspan cells distribute their content width evenly across the spanned columns.
/// Rowspan occupancy is tracked in `rowspan_map` (same semantics as `scan_row_explicit_widths`).
fn scan_row_content_widths(
    row: &LayoutBox,
    col_min: &mut Vec<f32>,
    col_max: &mut Vec<f32>,
    rowspan_map: &mut Vec<u32>,
    m: &dyn TextMeasurer,
    vp: Size,
) {
    let mut col_pos = 0usize;
    for cell in row.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)) {
        while col_pos < rowspan_map.len() && rowspan_map[col_pos] > 0 {
            col_pos += 1;
        }
        let span = cell.col_span.max(1) as usize;
        let end_col = col_pos + span;
        if end_col > col_min.len() {
            col_min.resize(end_col, 0.0);
            col_max.resize(end_col, 0.0);
        }
        if end_col > rowspan_map.len() {
            rowspan_map.resize(end_col, 0);
        }
        let (cmin, cmax) = cell_min_max_border_box_w(cell, m, vp);
        let per_min = cmin / span as f32;
        let per_max = cmax / span as f32;
        for i in col_pos..end_col {
            col_min[i] = col_min[i].max(per_min);
            col_max[i] = col_max[i].max(per_max);
        }
        if cell.row_span > 1 {
            let rs = cell.row_span;
            for v in rowspan_map.iter_mut().skip(col_pos).take(span) {
                if *v < rs {
                    *v = rs;
                }
            }
        }
        col_pos = end_col;
    }
}

/// Computes per-column widths for a `BoxKind::Table` element by scanning all rows
/// (direct and inside `TableRowGroup` children). Colspan/rowspan-aware: cells with
/// `colspan > 1` distribute their width across columns; `rowspan > 1` cells block
/// subsequent rows from reusing those columns. Returns a `Vec<f32>` of border-box
/// widths, one per column.
///
/// When `measurer` is provided, uses CSS 2.1 §17.5.2 content-based auto sizing:
/// each auto column gets at least its min-content width, with the remaining space
/// distributed proportionally to max-content widths. Without a measurer, falls back
/// to equal distribution among auto columns.
///
/// In Separate border mode, `(n_cols + 1) * h_spacing` is reserved for inter-cell and
/// outer gaps before distributing the remaining width among auto-width columns.
/// CSS: border-spacing — P4 wires h_spacing from ComputedStyle.border_spacing_h
fn compute_table_col_widths(
    b: &LayoutBox,
    content_width: f32,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
) -> Vec<f32> {
    let h_spacing = match b.style.border_collapse {
        BorderCollapse::Collapse => 0.0,
        BorderCollapse::Separate => b.style.border_spacing_h,
    };

    let mut col_explicit: Vec<Option<f32>> = Vec::new();
    let mut rowspan_map: Vec<u32> = Vec::new();

    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => {
                scan_row_explicit_widths(child, &mut col_explicit, &mut rowspan_map, content_width, viewport);
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        scan_row_explicit_widths(row, &mut col_explicit, &mut rowspan_map, content_width, viewport);
                        decrement_rowspan_map(&mut rowspan_map);
                    }
                }
            }
            _ => {}
        }
    }

    let n_cols = col_explicit.len();
    if n_cols == 0 {
        return Vec::new();
    }

    // Subtract spacing slots from available width before distributing to auto columns.
    let total_h_spacing = (n_cols + 1) as f32 * h_spacing;
    let total_explicit: f32 = col_explicit.iter().filter_map(|w| *w).sum();
    let available = (content_width - total_h_spacing - total_explicit).max(0.0);
    let auto_count = col_explicit.iter().filter(|w| w.is_none()).count();

    if auto_count == 0 {
        return col_explicit.iter().map(|w| w.unwrap_or(0.0)).collect();
    }

    // CSS 2.1 §17.5.2: content-based auto column sizing when a text measurer is available.
    if let Some(m) = measurer {
        let mut col_min = vec![0.0f32; n_cols];
        let mut col_max = vec![0.0f32; n_cols];
        let mut rs_map: Vec<u32> = Vec::new();
        for child in &b.children {
            match &child.kind {
                BoxKind::TableRow => {
                    scan_row_content_widths(child, &mut col_min, &mut col_max, &mut rs_map, m, viewport);
                    decrement_rowspan_map(&mut rs_map);
                }
                BoxKind::TableRowGroup => {
                    for row in &child.children {
                        if matches!(row.kind, BoxKind::TableRow) {
                            scan_row_content_widths(row, &mut col_min, &mut col_max, &mut rs_map, m, viewport);
                            decrement_rowspan_map(&mut rs_map);
                        }
                    }
                }
                _ => {}
            }
        }

        let auto_min_total: f32 = (0..n_cols)
            .filter(|&i| col_explicit[i].is_none())
            .map(|i| col_min[i])
            .sum();
        // Use col_max as the proportional weight; clamp at col_min so weight is always ≥ min.
        let total_weight: f32 = (0..n_cols)
            .filter(|&i| col_explicit[i].is_none())
            .map(|i| col_max[i].max(col_min[i]))
            .sum();

        return (0..n_cols)
            .map(|i| {
                col_explicit[i].unwrap_or_else(|| {
                    if auto_min_total >= available {
                        // Not enough space for min-content: distribute proportionally to min.
                        if auto_min_total > 0.0 {
                            (available * col_min[i] / auto_min_total).max(0.0)
                        } else {
                            available / auto_count as f32
                        }
                    } else {
                        // Enough for min; distribute extra proportionally to max-content weight.
                        let extra = available - auto_min_total;
                        let weight = col_max[i].max(col_min[i]);
                        col_min[i]
                            + if total_weight > 0.0 {
                                extra * weight / total_weight
                            } else {
                                extra / auto_count as f32
                            }
                    }
                })
            })
            .collect();
    }

    // Fallback without measurer: equal distribution.
    let auto_share = (available / auto_count as f32).max(0.0);
    col_explicit.iter().map(|w| w.unwrap_or(auto_share)).collect()
}

/// CSS Multi-column Layout L1 — lays out `children` into N columns.
/// Returns content height (max column height, without padding/border).
///
/// `container_h` is the resolved content-box height of the multi-column container, used
/// by `column-fill: auto` to fill columns sequentially up to that height instead of
/// balancing content equally across all columns.
/// CSS Multi-column L1 §3.4 — true column fragmentation breaks block content
/// across columns. Lumen approximates this by geometrically slicing a box into
/// per-column pieces (see `lay_out_multicol_children`). That is only visually
/// faithful for a "simple" box: a leaf block whose paint is a flat fill
/// (background-color) — no children or text that a slice would duplicate, no
/// border whose cut edge would show. Anything else keeps the atomic
/// one-box-per-column placement.
fn box_is_column_sliceable(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block)
        && b.children.is_empty()
        && b.style.border_top_width == 0.0
        && b.style.border_bottom_width == 0.0
        && b.style.border_left_width == 0.0
        && b.style.border_right_width == 0.0
}

/// CSS Multicol §7.1 — balanced column height for atomic (unsliceable) boxes.
///
/// Returns the smallest column height `H` such that greedily packing `outer_hs`
/// (each box's margin-box height, in source order, opening a new column whenever
/// the running height would exceed `H`) fits within `n_cols` columns. This is the
/// target browsers minimise when `column-fill: balance` and items cannot be split
/// across columns — e.g. 9 cards of varying height fill 3 columns as 3/3/3 rather
/// than packing the first column to the container height.
fn balanced_column_height(outer_hs: &[f32], n_cols: usize) -> f32 {
    let total: f32 = outer_hs.iter().sum();
    if n_cols <= 1 || outer_hs.is_empty() {
        return total.max(1.0);
    }
    let max_item = outer_hs.iter().cloned().fold(0.0_f32, f32::max);
    // Any feasible height is at least the tallest single item and at least the
    // perfectly even split; the sum is always feasible (one column holds all).
    let mut lo = max_item.max(total / n_cols as f32);
    let mut hi = total.max(lo);
    let fits = |h: f32| -> bool {
        let mut cols = 1usize;
        let mut cur = 0.0f32;
        for &x in outer_hs {
            if cur > 0.0 && cur + x > h {
                cols += 1;
                if cols > n_cols {
                    return false;
                }
                cur = x;
            } else {
                cur += x;
            }
        }
        true
    };
    // Binary search for the minimal feasible height (~0.25 px precision).
    for _ in 0..40 {
        if hi - lo <= 0.25 {
            break;
        }
        let mid = (lo + hi) * 0.5;
        if fits(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi.ceil().max(1.0)
}

#[allow(clippy::too_many_arguments)]
fn lay_out_multicol_children(
    children: &mut Vec<LayoutBox>,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    s: &ComputedStyle,
    em: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    container_h: Option<f32>,
) -> f32 {
    let cb = content_width;
    let col_gap = s.column_gap.resolve_or_zero(em, cb, viewport).max(0.0);

    // Compute column count from column-count / column-width.
    let n_cols: u32 = match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
                let n_from_w = ((content_width + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport)
                && w > 0.0
            {
                ((content_width + col_gap) / (w + col_gap)).floor() as u32
            } else {
                1
            }
        }
        (None, None) => 1,
    }.max(1);

    let col_w = ((content_width - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);

    // column-fill: balance distributes content equally; auto fills columns to container height.
    // When no container height is known, auto behaves like balance.
    let balance = s.column_fill_balance || container_h.is_none();

    // Move children out so the slice path can replace whole boxes with multiple
    // per-column fragment clones (the box count changes), then rebuild `children`.
    let mut work = std::mem::take(children);

    // Collect flow (non-abs, non-skip) child indices.
    let flow_idxs: Vec<usize> = work
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    if flow_idxs.is_empty() {
        *children = work;
        return 0.0;
    }

    // Split flow children into segments separated by column-span:all elements.
    // Each entry is (regular_children, Option<span_all_child_idx>).
    let mut segments: Vec<(Vec<usize>, Option<usize>)> = Vec::new();
    let mut seg: Vec<usize> = Vec::new();
    for &i in &flow_idxs {
        if work[i].style.column_span_all {
            segments.push((std::mem::take(&mut seg), Some(i)));
        } else {
            seg.push(i);
        }
    }
    segments.push((seg, None));

    let mut cur_y = content_y;
    // Boxes placed into the rebuilt child list. Fragment clones (slice path) and
    // positioned originals (atomic / span path) are pushed here in turn; any box
    // not consumed (absolute / Skip placeholders) is appended unchanged at the end.
    let mut out: Vec<LayoutBox> = Vec::with_capacity(work.len());
    let mut consumed = vec![false; work.len()];

    for (seg_idxs, span_idx) in &segments {
        if !seg_idxs.is_empty() {
            // First pass at (0, 0) to measure intrinsic heights.
            for &i in seg_idxs {
                lay_out(&mut work[i], 0.0, 0.0, col_w, None, measurer, viewport, pcb, hp, false);
            }

            // Outer height of each segment child = margin_top + rect.height + margin_bottom.
            let outer_hs: Vec<f32> = seg_idxs.iter().map(|&i| {
                let c = &work[i];
                let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
                let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
                mt + c.rect.height + mb
            }).collect();

            let total_h: f32 = outer_hs.iter().sum();

            // CSS Multicol §3.4: when every box can be safely sliced, fragment the
            // segment's content across all columns by height (this is what browsers
            // do — a tall empty block spills from one column into the next). The
            // balanced column height is total/n_cols; column-fill:auto fills each
            // column to the container height instead.
            let all_sliceable =
                n_cols > 1 && seg_idxs.iter().all(|&i| box_is_column_sliceable(&work[i]));

            if all_sliceable {
                let col_h = if balance {
                    (total_h / n_cols as f32).ceil().max(1.0)
                } else {
                    container_h.unwrap_or_else(|| (total_h / n_cols as f32).ceil()).max(1.0)
                };

                // Virtual single-column stack: each box's border-box occupies
                // [virtual_top, virtual_top + height), with margins as gaps.
                let mut stack: Vec<(usize, f32, f32)> = Vec::with_capacity(seg_idxs.len());
                let mut v = 0.0f32;
                for (&i, &oh) in seg_idxs.iter().zip(outer_hs.iter()) {
                    let mt = work[i].style.margin_top
                        .resolve_or_zero(work[i].style.font_size, col_w, viewport);
                    stack.push((i, v + mt, work[i].rect.height));
                    v += oh;
                }

                // Emit one clipped fragment per (column, box) overlap.
                let mut seg_extent = 0.0f32;
                for c in 0..n_cols as usize {
                    let col_lo = c as f32 * col_h;
                    let col_hi = col_lo + col_h;
                    let col_x = content_x + c as f32 * (col_w + col_gap);
                    for &(i, bt, bh) in &stack {
                        let bb = bt + bh;
                        let ov_lo = bt.max(col_lo);
                        let ov_hi = bb.min(col_hi);
                        if ov_hi > ov_lo {
                            let mut frag = work[i].clone();
                            frag.rect.x = col_x;
                            frag.rect.y = cur_y + (ov_lo - col_lo);
                            frag.rect.width = col_w;
                            frag.rect.height = ov_hi - ov_lo;
                            seg_extent = seg_extent.max(ov_hi - col_lo);
                            out.push(frag);
                        }
                    }
                }
                for &i in seg_idxs {
                    consumed[i] = true;
                }
                cur_y += seg_extent.max(0.0);
            } else {
                // Atomic fallback: place each whole box into a column (greedy by height).
                // In balance mode the target is the optimal balanced column height
                // (smallest H that packs all boxes into n_cols columns) — matches how
                // browsers distribute unsliceable items (e.g. 9 cards → 3×3, not 5/4/0).
                // column-fill:auto fills each column to the container height instead.
                let target_h = if balance {
                    balanced_column_height(&outer_hs, n_cols as usize)
                } else {
                    container_h.unwrap_or_else(|| (total_h / n_cols as f32).ceil()).max(1.0)
                };

                let mut col_assignment = vec![0usize; seg_idxs.len()];
                let mut col_fill = vec![0.0f32; n_cols as usize];
                let mut cur_col = 0usize;
                for (j, &oh) in outer_hs.iter().enumerate() {
                    let height_overflow = col_fill[cur_col] + oh > target_h && oh > 0.0;
                    // Never advance past an empty column: a column must hold at least one item
                    // before overflowing to the next, otherwise an item taller than target_h
                    // would skip column 0 and leave it blank (CSS Multicol §3.4 — every column
                    // box is filled in order, starting from the first).
                    let col_nonempty = col_fill[cur_col] > 0.0;
                    if cur_col + 1 < n_cols as usize && col_nonempty && height_overflow {
                        cur_col += 1;
                    }
                    col_assignment[j] = cur_col;
                    col_fill[cur_col] += oh;
                }

                // Final positioning.
                let mut col_y = vec![cur_y; n_cols as usize];
                for (j, &i) in seg_idxs.iter().enumerate() {
                    let col = col_assignment[j];
                    let col_x = content_x + col as f32 * (col_w + col_gap);
                    lay_out(&mut work[i], col_x, col_y[col], col_w, None, measurer, viewport, pcb, hp, false);
                    let mb = work[i].style.margin_bottom
                        .resolve_or_zero(work[i].style.font_size, col_w, viewport);
                    col_y[col] = work[i].rect.y + work[i].rect.height + mb;
                    out.push(work[i].clone());
                    consumed[i] = true;
                }

                cur_y = col_y.into_iter().fold(cur_y, f32::max);
            }
        }

        // column-span: all — element spans the full column container width.
        if let Some(span_i) = *span_idx {
            lay_out(&mut work[span_i], content_x, cur_y, content_width, None, measurer, viewport, pcb, hp, false);
            let mb = work[span_i].style.margin_bottom
                .resolve_or_zero(work[span_i].style.font_size, content_width, viewport);
            cur_y = work[span_i].rect.y + work[span_i].rect.height + mb;
            out.push(work[span_i].clone());
            consumed[span_i] = true;
        }
    }

    // Preserve any non-flow boxes (absolute/fixed, Skip placeholders) unchanged.
    for (i, b) in work.into_iter().enumerate() {
        if !consumed[i] {
            out.push(b);
        }
    }
    *children = out;

    cur_y - content_y
}

/// CSS 2.1 §10.3.7 — does an absolutely positioned box resolve its `auto`
/// width by shrink-to-fit (BUG-745), or does it keep the legacy
/// "stretch to the containing block" behaviour?
///
/// Shrink-to-fit is the spec rule for *non-replaced* boxes, so the replaced
/// kinds (`<img>`, `<video>`, `<canvas>`, `<iframe>`, form controls) are
/// excluded: §10.3.8 sizes them from their intrinsic dimensions instead, and
/// their content is invisible to [`max_content_outer_width`] (an image's
/// intrinsic width lives in `BoxKind::Image`, not in child boxes), so measuring
/// them here would collapse them to their padding+border.
///
/// Two more kinds opt out because the intrinsic-width machinery does not model
/// them:
/// * `BoxKind::Table` already shrink-to-fits itself in `lay_out_inner` from
///   `table_intrinsic_content_width` (column widths + border-spacing), which the
///   block "widest child" rule of [`max_content_outer_width`] cannot reproduce;
/// * `display: grid`/`inline-grid` — a grid's max-content width is the sum of
///   its column max-contents plus gaps (the analogue of `flex_row_intrinsic_sum`
///   for the row axis), and no such rule exists yet, so the block rule would
///   under-measure a multi-column grid into one column's width. Stretching is
///   the safer failure mode until that rule lands.
fn abs_box_shrinks_to_fit(b: &LayoutBox) -> bool {
    !matches!(
        b.kind,
        BoxKind::Skip
            | BoxKind::Image { .. }
            | BoxKind::Video { .. }
            | BoxKind::Canvas { .. }
            | BoxKind::Iframe { .. }
            | BoxKind::FormControl { .. }
            | BoxKind::Table
    ) && !matches!(b.style.display, Display::Grid | Display::InlineGrid)
}

/// Positions absolutely/fixed-positioned deferred children of `parent`.
/// Called after parent's height is finalized so `my_pcb` is complete.
fn lay_out_abs_children(
    parent: &mut LayoutBox,
    deferred: &[(usize, f32, f32)],
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    my_pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    // CSS Anchor Positioning L1: collect all elements with `anchor-name` in the tree.
    // This registry is used to resolve `position-anchor` and `anchor()` function calls below.
    // CSS: anchor-name, position-anchor, anchor()
    let anchors = crate::anchor::collect_anchors(parent);

    for &(idx, static_x, static_y) in deferred {
        let cs = parent.children[idx].style.clone();
        let c_em = cs.font_size;

        let cb = if matches!(cs.position, Position::Fixed) {
            Rect::new(0.0, 0.0, viewport.width, viewport.height)
        } else {
            my_pcb
        };

        // CSS Anchor Positioning L1 §3.1 — intercept `anchor()` in top/right/bottom/left
        // before falling back to the plain length/auto value.
        // CSS: anchor(), position-anchor
        let default_anchor = cs.position_anchor.as_deref();
        let left = crate::anchor::resolve_inset(
            &anchors, &cs.left, cs.anchor_left.as_ref(), default_anchor, true, false, cb.x, cb.x + cb.width,
            c_em, cb.width, viewport,
        );
        let right = crate::anchor::resolve_inset(
            &anchors, &cs.right, cs.anchor_right.as_ref(), default_anchor, true, true, cb.x, cb.x + cb.width,
            c_em, cb.width, viewport,
        );
        let top = crate::anchor::resolve_inset(
            &anchors, &cs.top, cs.anchor_top.as_ref(), default_anchor, false, false, cb.y, cb.y + cb.height,
            c_em, cb.height, viewport,
        );
        let bottom = crate::anchor::resolve_inset(
            &anchors, &cs.bottom, cs.anchor_bottom.as_ref(), default_anchor, false, true, cb.y, cb.y + cb.height,
            c_em, cb.height, viewport,
        );

        let c_ml = cs.margin_left.resolve_or_zero(c_em, cb.width, viewport);
        let c_mr = cs.margin_right.resolve_or_zero(c_em, cb.width, viewport);
        let c_mt = cs.margin_top.resolve_or_zero(c_em, cb.height, viewport);
        let c_mb = cs.margin_bottom.resolve_or_zero(c_em, cb.height, viewport);

        // Доступная ширина для layout абсолютного child.
        let avail_w = if left.is_some() && right.is_some() && cs.width.is_none() {
            // Обе инсеты заданы, ширина `auto` → ширина выводится из зазора
            // между ними (CSS Position L3 §6), shrink-to-fit не применяется.
            (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0)).max(0.0)
        } else if cs.width.is_none() && abs_box_shrinks_to_fit(&parent.children[idx]) {
            // CSS 2.1 §10.3.7 (BUG-745): у абсолютного не-replaced бокса с
            // `width: auto` и хотя бы одной `auto`-инсетой используемая ширина —
            // shrink-to-fit = min(max(min-content, available), max-content), а не
            // ширина содержащего блока. Разница видна не только в самой ширине:
            // ветка `right` ниже отсчитывает x от правого края содержащего блока
            // назад на `child.rect.width`, поэтому растянутый бокс с
            // `right: 16px` уезжал за левый край (`x = -16, w = 1024` вместо
            // карточки в углу) — форма «тост/тултип/cookie-баннер, приклеенный
            // к углу», пункт 4 BUG-733 на `tbank.ru`.
            //
            // `available` — свободное место содержащего блока за вычетом
            // заданных инсет и margin'ов; max/min-content уже включают
            // padding+border самого бокса (border-box), поэтому margin'ы
            // возвращаются обратно: `lay_out` трактует свой `available_width`
            // как margin-box.
            let child = &parent.children[idx];
            let free =
                (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - c_ml - c_mr).max(0.0);
            let max_c = max_content_outer_width(child, measurer, viewport);
            let min_c = min_content_outer_width(child, measurer, viewport);
            max_c.min(min_c.max(free)) + c_ml + c_mr
        } else {
            cb.width
        };

        lay_out(&mut parent.children[idx], 0.0, 0.0, avail_w, None, measurer, viewport, my_pcb, hp, false);

        // CSS Position L3 §6: an abs-pos box with both `top` and `bottom` non-auto
        // and `height: auto` resolves its used height to fill the inset gap. Mirror of
        // the `avail_w` width-from-insets path above. Applied post-layout because the
        // gap height is a containing-block used value, not a content-driven size.
        if top.is_some() && bottom.is_some() && cs.height.is_none() {
            let resolved_h =
                (cb.height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - c_mt - c_mb).max(0.0);
            parent.children[idx].rect.height = resolved_h;
        }

        let child = &mut parent.children[idx];

        // CSS Anchor Positioning L1 §4 — apply `anchor-size()` overrides for width/height.
        // Done before resolving `inset-area` so the element's used size (used to
        // align it within its position-area band) reflects the anchor-size result.
        let mut w_fixed = cs.width.is_some();
        let mut h_fixed = cs.height.is_some();
        if let Some(w) = cs.anchor_size_w.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size(&anchors, f, cs.position_anchor.as_deref())
        }) {
            child.rect.width = w;
            w_fixed = true;
        }
        if let Some(h) = cs.anchor_size_h.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size(&anchors, f, cs.position_anchor.as_deref())
        }) {
            child.rect.height = h;
            h_fixed = true;
        }

        // CSS Anchor Positioning L1 §5 — resolve `position-area` / `inset-area`.
        // A definite-size axis keeps its size and is aligned toward the anchor;
        // an `auto` axis stretches to fill its position-area band.
        // CSS: position-anchor, inset-area, position-area
        let elem_w = if w_fixed {
            crate::anchor::AxisSize::Fixed(child.rect.width)
        } else {
            crate::anchor::AxisSize::Auto
        };
        let elem_h = if h_fixed {
            crate::anchor::AxisSize::Fixed(child.rect.height)
        } else {
            crate::anchor::AxisSize::Auto
        };
        let anchored_pos = cs.position_anchor.as_deref().and_then(|anchor_name| {
            crate::anchor::resolve_inset_area(
                &anchors,
                anchor_name,
                cs.inset_area_row,
                cs.inset_area_col,
                cb,
                elem_w,
                elem_h,
            )
        });

        let (new_x, new_y) = if let Some(ref pos) = anchored_pos {
            // Anchor-positioned: override width/height only for auto (stretched) axes.
            if let Some(w) = pos.width {
                child.rect.width = w;
            }
            if let Some(h) = pos.height {
                child.rect.height = h;
            }
            (cb.x + pos.left, cb.y + pos.top)
        } else {
            // Normal abs-pos: resolve from left/right/top/bottom insets.
            let nx = match (left, right) {
                (Some(l), _)    => cb.x + l + c_ml,
                (None, Some(r)) => cb.x + cb.width - r - c_mr - child.rect.width,
                (None, None)    => static_x + c_ml,
            };
            let ny = match (top, bottom) {
                (Some(t), _)     => cb.y + t + c_mt,
                (None, Some(bv)) => cb.y + cb.height - bv - c_mb - child.rect.height,
                (None, None)     => static_y + c_mt,
            };
            (nx, ny)
        };

        let dx = new_x - child.rect.x;
        let dy = new_y - child.rect.y;
        shift_tree(child, dx, dy);
    }
}

/// An explicit override for a box's own used width/height/box-sizing,
/// threaded through `lay_out_inner` instead of being burned into `b.style`
/// via `Arc::make_mut` and undone afterward — the role `SavedItemSizing`
/// (removed, BUG-341 S34) used to play for `lay_out_flex`'s item re-layout.
///
/// `lay_out_inner` applies the override to a *locally cloned* `ComputedStyle`
/// used only for the duration of that one call (see its `s` binding);
/// `b.style`'s `Arc` is never mutated. This matters beyond avoiding a
/// save/restore dance: BUG-341 S31 found that `SavedItemSizing`'s double
/// `Arc::make_mut` (Step-1 probe never touched it, but the final placement
/// pass did) meant a flex item's `b.style` pointer was *never* stable across
/// two layout passes of the same item — the exact precondition a
/// style-identity-keyed cache would need. With the override applied
/// out-of-place, `b.style` keeps the same `Arc` across both passes whenever
/// nothing else about the item's style changed, restoring that precondition.
///
/// `None` fields leave the corresponding style declaration exactly as
/// authored — only fields the caller explicitly resolved are overridden.
#[derive(Clone, Copy, Default)]
struct UsedSizeOverride {
    /// Resolved width in px (interpreted per `box_sizing`), or `None` to leave
    /// `style.width` as declared.
    width: Option<f32>,
    /// Resolved height in px (interpreted per `box_sizing`), or `None` to leave
    /// `style.height` as declared.
    height: Option<f32>,
    /// Forces `style.box_sizing`, or `None` to leave it as declared. Flex's
    /// column/cross-stretch re-layout passes force `border-box` so the
    /// resolved size (already border-box, per the flexbox algorithm) is used
    /// verbatim instead of having padding+border added on top of it
    /// (BUG-333/BUG-343); its row-direction pass does not, matching what
    /// `SavedItemSizing`'s three call sites each did before this refactor.
    box_sizing: Option<BoxSizing>,
}

/// CSS Flexbox L1 §9 — multi-line flex layout.
///
/// Алгоритм:
/// 1. Для каждого flex-item вычисляем hypothetical main size из flex-basis.
/// 2. Распределяем free space через flex-grow / flex-shrink.
/// 3. Раскладываем items с учётом justify-content и align-items.
/// 4. При flex-wrap: apply align-content across flex lines.
///
/// `explicit_cross` — явная высота контейнера (content box) для row flex;
/// используется в align-content для вычисления свободного пространства по cross axis.
///
/// `explicit_main` — определённый main-размер (content box) для column flex
/// (явная `height` или растяжение родителем). `None` = main размер неопределён,
/// тогда контейнер сжимается по содержимому и flex-grow не действует.
///
/// Возвращает `content_height` (вертикальный размер контентной зоны контейнера).
#[allow(clippy::too_many_arguments)]
fn lay_out_flex(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    explicit_cross: Option<f32>,
    explicit_main: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let is_column = matches!(s.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse);
    let is_reverse = matches!(
        s.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let is_wrap = matches!(s.flex_wrap, FlexWrap::Wrap | FlexWrap::WrapReverse);
    let is_wrap_reverse = matches!(s.flex_wrap, FlexWrap::WrapReverse);

    // Indices of non-Skip children (actual flex items).
    // CSS Flexbox L1 §4.1: an absolutely-positioned child of a flex container does
    // not participate in flex layout — it must not become a flex item nor advance
    // the main-axis cursor. Such children are positioned afterward against the
    // container's content box (see the flex dispatch branch in `lay_out`).
    let mut item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip)
            && !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| i)
        .collect();
    // CSS Flexbox L1 §4 — stable sort by `order` (same-order items keep source order).
    item_idxs.sort_by_key(|&i| children[i].style.order);

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Container main size. For row it is always the definite content width. For
    // column it is the definite content height when known (explicit `height` or a
    // parent-imposed stretch — `explicit_main`), otherwise indefinite (auto):
    // the container then sizes to its items and flex-grow has no free space to
    // distribute (CSS Flexbox §9.7).
    let main_definite = if is_column { explicit_main } else { Some(content_width) };
    let container_main = main_definite.unwrap_or(0.0);

    // CSS Box Alignment §8: gap is fixed space between items, subtracted before flex-grow/shrink.
    let em = s.font_size;
    // item_gap: gap between items along the main axis.
    // cross_gap: gap between flex lines along the cross axis (wrap only).
    let item_gap = if is_column {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };
    let cross_gap = if is_column {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };

    // Step 1 — preliminary layout for intrinsic sizes.
    //
    // Only run for items whose `all_hyp` computation below actually reads
    // `item.rect` back: column-direction items always need the item's real
    // content height, and row-direction `auto`/`content` items with no
    // explicit width need `item.rect.width`. Every other combination
    // resolves its main size from the style directly (`FlexBasis::Length`)
    // or from the existing cheap `flex_auto_base_main_width` probe (row,
    // `auto`/`content`, no explicit width) — for those, `item.rect` is never
    // read before the final placement pass below re-lays the item out anyway
    // with its resolved main size. Skipping the unneeded call avoids a full
    // recursive re-layout of the item's whole subtree that nothing reads
    // (BUG-341: every flex item paid for two full recursive layouts instead
    // of one, compounding multiplicatively with flex-nesting depth).
    //
    // BUG-802: skipping the call was only half the story. In a *column*
    // container `flex-basis: auto` — the default — makes the condition above a
    // constant `true`, so every item still paid two full recursive layouts
    // (this probe plus the final placement pass below), and those two multiply
    // down the tree: a chain of nested `flex-direction: column` boxes cost
    // ×2 per level (measured 0.27 s at depth 16, 1.21 s at 18, 4.91 s at 20 —
    // a page with 22-24 levels never finishes). The probe's result is now
    // stashed and replayed by the final pass whenever the two calls would
    // compute the same thing, which collapses the exponent to one layout per
    // level; see `column_probe` below for the three conditions.
    let cb = content_width;
    // BUG-802 — per item (indexed like `item_idxs`): the border-box height the
    // Step-1 probe produced, present only when that probe is replayable. `None`
    // means "lay the item out again in the final pass", which is what every
    // item did unconditionally before this.
    let mut column_probe: Vec<Option<f32>> = vec![None; item_idxs.len()];
    // BUG-802 — the height Step-1 measured for this item, whether it was probed
    // now or served from [`FLEX_COLUMN_PROBE_HEIGHTS`]. `None` for items that
    // were not probed at all (the row direction's usual case), where the
    // hypothetical size comes from the item's style or from
    // `flex_auto_base_main_width` instead.
    let mut probed_main: Vec<Option<f32>> = vec![None; item_idxs.len()];
    // The memo remembers a measurement across calls, so it must stand down
    // wherever an identical call can legitimately measure differently: while a
    // subgrid track context or a container-query basis is installed, neither of
    // which is part of any box's style (the same exclusion
    // `cacheable_for_layout_result_cache` makes for the subgrid half).
    let memo_usable = is_column && !crate::style::cq_context_active();
    for (k, &i) in item_idxs.iter().enumerate() {
        let needs_prelayout = {
            let is = &children[i].style;
            if is_column {
                match &is.flex_basis {
                    FlexBasis::Auto | FlexBasis::Content => true,
                    FlexBasis::Length(_) => {
                        is.min_height.is_none() && is.overflow_y == Overflow::Visible
                    }
                }
            } else {
                match &is.flex_basis {
                    FlexBasis::Auto | FlexBasis::Content => is.width.is_some(),
                    FlexBasis::Length(_) => false,
                }
            }
        };
        if needs_prelayout {
            let memoized = if memo_usable && cacheable_for_layout_result_cache(&children[i]) {
                let key: FlexProbeKey = (children[i].node, content_width.to_bits());
                FLEX_COLUMN_PROBE_HEIGHTS.with(|m| {
                    m.borrow().get(&key).and_then(|(style, h)| {
                        Arc::ptr_eq(style, &children[i].style).then_some(*h)
                    })
                })
            } else {
                None
            };
            if let Some(h) = memoized {
                // Nothing else between here and the final placement pass reads
                // the probed *subtree* — `max_content_outer_width`,
                // `min_content_outer_width` and `flex_item_max_main_outer` are
                // all intrinsic (style plus contents, never `rect`) — so the
                // remembered height is the whole of what this probe was for.
                probed_main[k] = Some(h);
            } else if is_column {
                // The two flags are the correctness guard the replay needs: the
                // probe runs with an indefinite containing-block height and at a
                // temporary main-axis position, so a subtree that consulted
                // either (a percentage block size, `content-visibility: auto`'s
                // position-dependent skip) must not be replayed — see
                // `INDEFINITE_HEIGHT_CONSULTED` / `CV_AUTO_TOUCHED`.
                let outer_cv = CV_AUTO_TOUCHED.with(|c| c.replace(false));
                let outer_ih = INDEFINITE_HEIGHT_CONSULTED.with(|c| c.replace(false));
                lay_out(&mut children[i], content_x, content_y, content_width, None, measurer, viewport, pcb, hp, false);
                let cv_here = CV_AUTO_TOUCHED.with(|c| c.get());
                let ih_here = INDEFINITE_HEIGHT_CONSULTED.with(|c| c.get());
                CV_AUTO_TOUCHED.with(|c| c.set(outer_cv || cv_here));
                INDEFINITE_HEIGHT_CONSULTED.with(|c| c.set(outer_ih || ih_here));
                probed_main[k] = Some(children[i].rect.height);
                if !cv_here && !ih_here {
                    column_probe[k] = Some(children[i].rect.height);
                }
                // `content-visibility: auto` decides whether to skip a subtree
                // from the scroll offset and a cross-frame ratchet, so its
                // measured height is not a property of the box alone and must
                // not be remembered. The indefinite-height flag is *not* a
                // reason to refuse here, unlike for the replay: both the stored
                // probe and the one being served pass `available_height: None`,
                // so whatever a percentage block size resolved to is the same
                // for each.
                if !cv_here && memo_usable && cacheable_for_layout_result_cache(&children[i]) {
                    let key: FlexProbeKey = (children[i].node, content_width.to_bits());
                    let entry = (Arc::clone(&children[i].style), children[i].rect.height);
                    FLEX_COLUMN_PROBE_HEIGHTS.with(|m| {
                        m.borrow_mut().insert(key, entry);
                    });
                }
            } else {
                lay_out(&mut children[i], content_x, content_y, content_width, None, measurer, viewport, pcb, hp, false);
            }
        }
    }

    // Compute hypothetical main sizes for all items (outer = including margins).
    let all_hyp: Vec<f32> = item_idxs
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let item = &children[i];
            // BUG-802: the height Step-1 measured — from the probe just run, or
            // remembered from the identical probe of an earlier pass over this
            // same item. `unwrap_or` covers the items Step-1 never probed.
            let probed_height = probed_main[k].unwrap_or(item.rect.height);
            let is = &item.style;
            let iem = is.font_size;
            let m_l = is.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = is.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
            match &is.flex_basis {
                FlexBasis::Auto | FlexBasis::Content => {
                    if is_column {
                        probed_height + m_t + m_b
                    } else {
                        // CSS Flexbox §9.2/§9.7: for `auto`/`content` flex-basis with no
                        // explicit width, the flex base size is the item's max-content
                        // width, clamped by its own min-width / max-width. Using the
                        // preliminary-pass `item.rect.width` was wrong: a block item
                        // stretches to the full container width there, so a label that
                        // sets only `min-width` and holds short text reported the whole
                        // container width as its base size and was then shrunk down to an
                        // equal share of the row instead of staying at its min-width
                        // (BUG-179, TEST-46 — second column drifted ~160px right).
                        let w = if is.width.is_none() {
                            flex_auto_base_main_width(item, cb, measurer, viewport)
                        } else {
                            item.rect.width
                        };
                        w + m_l + m_r
                    }
                }
                FlexBasis::Length(l) => {
                    let base = l.resolve(iem, Some(cb), viewport).unwrap_or(0.0).max(0.0);
                    if is_column {
                        // CSS Flexbox §4.5: a flex item's automatic minimum size. When
                        // its main-axis `min-height` is `auto` and the block-axis
                        // overflow is `visible`, the item cannot shrink below its
                        // content size suggestion. Without this floor, `flex: 1`
                        // (which sets `flex-basis: 0`) collapses a content-sized item
                        // to height 0 in an indefinite-height column container, so
                        // following siblings paint on top of it (BUG-158, lenta.ru
                        // news cards). `item.rect.height` from the preliminary pass is
                        // the floor: it is the content height, already clamped by any
                        // real explicit `height` (the spec's "specified size suggestion"
                        // cap). We deliberately do NOT skip this when `style.height` is
                        // Some, because flex layout itself writes a resolved px height
                        // back into the item's style (see the `is_column` branch below);
                        // on a re-layout pass that stale value must not disable the
                        // floor and re-collapse the item.
                        let auto_min = if is.min_height.is_none()
                            && is.overflow_y == Overflow::Visible
                        {
                            probed_height
                        } else {
                            0.0
                        };
                        base.max(auto_min) + m_t + m_b
                    } else {
                        base + m_l + m_r
                    }
                }
            }
        })
        .collect();

    // Step 2 — break items into flex lines.
    // Wrap only applies to row direction (column wrapping requires known container height, Phase 0: skip).
    let lines: Vec<Vec<usize>> = if is_wrap && !is_column && container_main > 0.0 {
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut cur_line: Vec<usize> = Vec::new();
        let mut cur_main = 0.0_f32;
        for (k, &item_main) in all_hyp.iter().enumerate() {
            let gap = if cur_line.is_empty() { 0.0 } else { item_gap };
            if !cur_line.is_empty() && cur_main + gap + item_main > container_main {
                lines.push(cur_line);
                cur_line = vec![k];
                cur_main = item_main;
            } else {
                cur_line.push(k);
                cur_main += gap + item_main;
            }
        }
        if !cur_line.is_empty() {
            lines.push(cur_line);
        }
        lines
    } else {
        vec![(0..item_idxs.len()).collect()]
    };

    // Step 3–5: process each line (grow/shrink, justify, position, align).
    // cross_cursor tracks the current cross-axis offset across lines.
    let mut cross_cursor = 0.0_f32;

    let n_lines = lines.len();
    let ordered_line_idxs: Vec<usize> = if is_wrap_reverse {
        (0..n_lines).rev().collect()
    } else {
        (0..n_lines).collect()
    };
    // Track line cross-sizes for align-content.
    let mut line_cross_sizes: Vec<f32> = Vec::with_capacity(n_lines);


    for li in &ordered_line_idxs {
        let line_keys = &lines[*li]; // keys into item_idxs
        let n = line_keys.len();

        // Per-line hyp mains (mutable for grow/shrink).
        let mut hyp_mains: Vec<f32> = line_keys.iter().map(|&k| all_hyp[k]).collect();

        // Free space after gaps.
        let line_gap_total = if n > 1 { item_gap * (n - 1) as f32 } else { 0.0 };
        let total_hyp: f32 = hyp_mains.iter().sum();
        let free_space = if main_definite.is_some() {
            container_main - total_hyp - line_gap_total
        } else {
            0.0
        };

        if free_space > 0.0 {
            let total_grow: f32 = line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_grow).sum();
            if total_grow > 0.0 {
                // CSS Flexbox §9.7 шаг 4 «fix min/max violations» — тот же цикл
                // заморозки, что при сжатии ниже, только потолок здесь
                // `max-width`/`max-height` элемента. Без него растущий элемент
                // проезжал свой максимум: раскладка выдавала ему всю ширину
                // строки, свободного места не оставалось (и `justify-content`,
                // и auto-поля получали ноль), а видимая ширина всё равно
                // упиралась в `max-width` при собственной раскладке элемента —
                // отсюда «карточка во всю строку, но нарисована слева».
                let grows: Vec<f32> =
                    line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_grow).collect();
                let maxes: Vec<f32> = line_keys
                    .iter()
                    .map(|&k| {
                        flex_item_max_main_outer(&children[item_idxs[k]], cb, viewport, is_column)
                    })
                    .collect();
                let base: Vec<f32> = hyp_mains.clone();
                let mut frozen: Vec<bool> = grows.iter().map(|&g| g <= 0.0).collect();
                // Каждый проход замораживает хотя бы один элемент, поэтому `n`
                // проходов заведомо хватает.
                for _ in 0..n {
                    let unfrozen: Vec<usize> = (0..n).filter(|&j| !frozen[j]).collect();
                    if unfrozen.is_empty() {
                        break;
                    }
                    let frozen_sum: f32 = (0..n).filter(|&j| frozen[j]).map(|j| hyp_mains[j]).sum();
                    let unfrozen_base: f32 = unfrozen.iter().map(|&j| base[j]).sum();
                    let remaining = container_main - line_gap_total - frozen_sum - unfrozen_base;
                    let total_weight: f32 = unfrozen.iter().map(|&j| grows[j]).sum();
                    if remaining <= 0.0 || total_weight <= 0.0 {
                        for &j in &unfrozen {
                            hyp_mains[j] = base[j].min(maxes[j]);
                        }
                        break;
                    }
                    let mut violated = false;
                    for &j in &unfrozen {
                        let target = base[j] + remaining * (grows[j] / total_weight);
                        let clamped = target.min(maxes[j]);
                        hyp_mains[j] = clamped;
                        if clamped < target - 0.01 {
                            frozen[j] = true;
                            violated = true;
                        }
                    }
                    if !violated {
                        break;
                    }
                }
            }
        } else if free_space < 0.0 {
            // CSS Flexbox L1 §9.7 step 4 — «fix min/max violations». Shrinking is not
            // a single proportional pass: every item has a main-axis minimum
            // (§4.5 automatic minimum size for the initial `min-width: auto`), and an
            // item that would be pushed below it is frozen at that minimum while the
            // *remaining* deficit is redistributed over the still-flexible items. The
            // loop is what makes a row of fixed-width items overflow its container
            // instead of collapsing to an equal share of it (BUG-433).
            //
            // Only the row axis gets the floor: the column axis folds its content-size
            // floor into the base size above (see the `is_column` arm of `all_hyp`).
            let mins: Vec<f32> = line_keys
                .iter()
                .map(|&k| {
                    let item = &children[item_idxs[k]];
                    if is_column {
                        return 0.0;
                    }
                    let is = &item.style;
                    let iem = is.font_size;
                    let m_l = is.margin_left.resolve_or_zero(iem, cb, viewport);
                    let m_r = is.margin_right.resolve_or_zero(iem, cb, viewport);
                    // `mins` is compared against the *outer* (margin-box) sizes in
                    // `hyp_mains`, so the margins ride along with the floor.
                    flex_item_min_main_width(item, cb, measurer, viewport) + m_l + m_r
                })
                .collect();
            let shrink: Vec<f32> = line_keys
                .iter()
                .map(|&k| children[item_idxs[k]].style.flex_shrink)
                .collect();
            let base: Vec<f32> = hyp_mains.clone();
            // An item with `flex-shrink: 0` never shrinks — it starts out frozen at
            // its base size (still clamped by its own minimum, per step 4).
            let mut frozen: Vec<bool> = shrink.iter().map(|&f| f <= 0.0).collect();
            for j in 0..n {
                if frozen[j] {
                    hyp_mains[j] = base[j].max(mins[j]);
                }
            }
            // Each iteration freezes at least one item, so `n` passes always suffice.
            for _ in 0..n {
                let unfrozen: Vec<usize> = (0..n).filter(|&j| !frozen[j]).collect();
                if unfrozen.is_empty() {
                    break;
                }
                let frozen_sum: f32 = (0..n).filter(|&j| frozen[j]).map(|j| hyp_mains[j]).sum();
                let unfrozen_base: f32 = unfrozen.iter().map(|&j| base[j]).sum();
                let remaining = container_main - line_gap_total - frozen_sum - unfrozen_base;
                let total_weight: f32 = unfrozen.iter().map(|&j| shrink[j] * base[j]).sum();
                if remaining >= 0.0 || total_weight <= 0.0 {
                    // Deficit already absorbed by the frozen items (or nothing left that
                    // can absorb it) — the rest keep their base size.
                    for &j in &unfrozen {
                        hyp_mains[j] = base[j].max(mins[j]);
                    }
                    break;
                }
                let mut violated = false;
                for &j in &unfrozen {
                    let target = base[j] + remaining * (shrink[j] * base[j] / total_weight);
                    let clamped = target.max(mins[j]).max(0.0);
                    hyp_mains[j] = clamped;
                    if clamped > target + 0.01 {
                        frozen[j] = true;
                        violated = true;
                    }
                }
                if !violated {
                    break;
                }
            }
        }

        // Justify-content within the line.
        let resolved_main: f32 = hyp_mains.iter().sum();
        let remaining = if main_definite.is_some() {
            (container_main - resolved_main - line_gap_total).max(0.0)
        } else {
            0.0
        };
        // CSS Flexbox §8.1: `margin: auto` на ГЛАВНОЙ оси съедает всё
        // положительное свободное место ДО того, как спрашивают
        // `justify-content` — поэтому у элемента с `margin-left/right: auto`
        // в строчном контейнере ничего не остаётся на распределение, и он
        // встаёт по центру независимо от `justify-content`. Пока auto здесь
        // резолвился в ноль, такой элемент прижимался к началу строки: живой
        // пример — карточка формы входа `tbank.ru/login/` (`<main>` с
        // `margin: auto` внутри `_PageWrapper` с `space-between`), которая
        // стояла слева вместо центра.
        let auto_main: Vec<(bool, bool)> = (0..n)
            .map(|j| {
                let is = &children[item_idxs[line_keys[j]]].style;
                if is_column {
                    (
                        matches!(is.margin_top, LengthOrAuto::Auto),
                        matches!(is.margin_bottom, LengthOrAuto::Auto),
                    )
                } else {
                    (
                        matches!(is.margin_left, LengthOrAuto::Auto),
                        matches!(is.margin_right, LengthOrAuto::Auto),
                    )
                }
            })
            .collect();
        let auto_main_count =
            auto_main.iter().map(|(a, b)| usize::from(*a) + usize::from(*b)).sum::<usize>();
        let auto_main_share = if auto_main_count > 0 && remaining > 0.0 {
            remaining / auto_main_count as f32
        } else {
            0.0
        };

        let (jc_start, jc_gap) = if auto_main_share > 0.0 {
            // Свободного места уже нет — распределять `justify-content` нечего.
            (0.0, 0.0)
        } else {
            match s.justify_content {
                AlignValue::End => (remaining, 0.0),
                AlignValue::Center => (remaining / 2.0, 0.0),
                AlignValue::SpaceBetween => {
                    if n <= 1 { (0.0, 0.0) } else { (0.0, remaining / (n - 1) as f32) }
                }
                AlignValue::SpaceAround => {
                    let per = remaining / n as f32;
                    (per / 2.0, per)
                }
                AlignValue::SpaceEvenly => {
                    let per = remaining / (n + 1) as f32;
                    (per, per)
                }
                _ => (0.0, 0.0),
            }
        };

        // Final layout: position items along main axis.
        let ordered_keys: Vec<usize> = if is_reverse { (0..n).rev().collect() } else { (0..n).collect() };
        let mut main_cursor = jc_start;

        for &j in &ordered_keys {
            let k = line_keys[j];
            let i = item_idxs[k];
            let outer_main = hyp_mains[j];
            let item_s = children[i].style.clone();
            let iem = item_s.font_size;
            let m_l = item_s.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = item_s.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = item_s.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = item_s.margin_bottom.resolve_or_zero(iem, cb, viewport);
            // Доля auto-полей главной оси: перед элементом — та, что лежит со
            // стороны начала обхода (у reverse-направления это поле конца).
            let (auto_before, auto_after) = if is_reverse {
                (auto_main[j].1, auto_main[j].0)
            } else {
                (auto_main[j].0, auto_main[j].1)
            };
            if auto_before {
                main_cursor += auto_main_share;
            }

            if is_column {
                let inner_main = (outer_main - m_t - m_b).max(0.0);
                // Поперечная ось колоночного контейнера — ГОРИЗОНТАЛЬ. До
                // 2026-08-17 её не было вовсе: элемент всегда растягивался на
                // всю ширину контейнера, поэтому ни `align-items: center`, ни
                // `margin-left/right: auto` не двигали его с левого края
                // (живой случай — карточка формы входа `tbank.ru/login/`
                // внутри колоночной обёртки страницы).
                let avail_cross = (content_width - m_l - m_r).max(0.0);
                let auto_cross_l = matches!(item_s.margin_left, LengthOrAuto::Auto);
                let auto_cross_r = matches!(item_s.margin_right, LengthOrAuto::Auto);
                let cross_align = if matches!(item_s.align_self, AlignValue::Auto) {
                    s.align_items
                } else {
                    item_s.align_self
                };
                let aligned_cross = matches!(
                    cross_align,
                    AlignValue::Start | AlignValue::End | AlignValue::Center
                );
                // Выровненный (не растянутый) элемент занимает по поперечной
                // оси свой fit-content, а не всю ширину — иначе двигать нечего.
                let used_cross = if auto_cross_l || auto_cross_r || aligned_cross {
                    let max_c = max_content_outer_width(&children[i], measurer, viewport);
                    let min_c = min_content_outer_width(&children[i], measurer, viewport);
                    max_c.min(avail_cross).max(min_c).min(avail_cross).max(0.0)
                } else {
                    avail_cross
                };
                // `inner_main` is the item's resolved *border-box* main size (it is
                // derived from the preliminary border-box height and the flex
                // grow/shrink result). Force border-box before re-layout so the value
                // is used verbatim instead of having border+padding added on top of it
                // for a content-box item (which double-counts the border). Mirrors the
                // cross-axis stretch path below.
                // BUG-802: the Step-1 probe above already laid this exact subtree
                // out — at `content_y` instead of `content_y + main_cursor`, with
                // an indefinite height instead of the resolved `inner_main`, and
                // with `content_width` instead of `used_cross`. When the last two
                // differences are *no* difference (the item neither grew nor
                // shrank, and its cross size is the full content width — no auto
                // margin, no `align-self` narrowing it to fit-content), and the
                // probe was clean of the two position/height-sensitive markers,
                // the final pass would recompute the identical subtree. Replay it
                // and move it into place instead: this is what turns the ×2 per
                // nesting level into ×1. Exact bit equality, not an epsilon — an
                // approximate match would replay geometry that differs from what
                // the second layout would have produced.
                let replayable = column_probe[k].is_some_and(|probed| {
                    probed.to_bits() == inner_main.to_bits()
                        && used_cross.to_bits() == content_width.to_bits()
                });
                if replayable {
                    // The shift is the difference between the two calls' *box*
                    // origins, not the bare `main_cursor`: `lay_out_inner` lands
                    // the box at `start_y + margin_top` (BUG-294), so subtracting
                    // the probe's own origin from the final one reproduces its
                    // arithmetic exactly instead of re-associating the sum. The
                    // difference matters: adding `main_cursor` to an already
                    // rounded `content_y + m_t` moved a box at y≈17000 by 0.01 px
                    // against what the second layout would have produced
                    // (`samples/heavy.html`, the one page of the whole
                    // graphic-test corpus where an A/B of the dumps caught it).
                    let dy = ((content_y + main_cursor) + m_t) - (content_y + m_t);
                    shift_tree(&mut children[i], 0.0, dy);
                } else {
                    // BUG-294: pass the item's *margin-box* start (no margin pre-added).
                    // `lay_out_inner` unconditionally adds the box's own `margin_left`/
                    // `margin_top` to the `start_x`/`start_y` it receives, so pre-adding
                    // `m_l`/`m_t` here double-counts the margin. Every other call site in
                    // this file passes the bare margin-box origin and lets `lay_out_inner`
                    // apply the margin once.
                    lay_out_with_used_size(
                        &mut children[i],
                        content_x,
                        content_y + main_cursor,
                        used_cross,
                        Some(inner_main),
                        measurer,
                        viewport,
                        pcb,
                        hp,
                        false,
                        UsedSizeOverride {
                            height: Some(inner_main),
                            box_sizing: Some(BoxSizing::BorderBox),
                            ..Default::default()
                        },
                    );
                }
                // Свободное место поперечной оси достаётся auto-полям, а если
                // их нет — выравниванию (CSS Flexbox §8.1: auto старше
                // `align-self`).
                let free_cross = (avail_cross - children[i].rect.width).max(0.0);
                let cross_shift = if auto_cross_l && auto_cross_r {
                    free_cross / 2.0
                } else if auto_cross_l {
                    free_cross
                } else if auto_cross_r {
                    0.0
                } else {
                    match cross_align {
                        AlignValue::Center => free_cross / 2.0,
                        AlignValue::End => free_cross,
                        _ => 0.0,
                    }
                };
                if cross_shift != 0.0 {
                    shift_tree(&mut children[i], cross_shift, 0.0);
                }
                main_cursor += outer_main + item_gap + jc_gap;
                if auto_after {
                    main_cursor += auto_main_share;
                }
            } else {
                let inner_main = (outer_main - m_l - m_r).max(0.0);
                // BUG-427: `inner_main` is a *border-box* main size — the flex base
                // size comes from `max_content_outer_width`, which already includes
                // the item's own padding+border. Handing it to a content-box item as
                // its used `width` made the re-layout add that padding+border a
                // second time: the item's rect came out `padding_x + border_x` too
                // wide while the main-axis cursor kept advancing by the correct
                // border-box size, so every pair of adjacent padded row items
                // overlapped by exactly that amount (dzen.ru topic tabs, 24 px of
                // padding → chips drawn on top of each other; items with an explicit
                // `width` escaped it because their base size came from style).
                // Converted here rather than by forcing `box_sizing: BorderBox` the
                // way the column arm does — that switch also reinterprets the item's
                // own `height`, which is a *cross*-axis size in this arm and must
                // keep its declared box-sizing (TEST-30's `.box`: 120px + 3px border
                // is 126 tall, not 120).
                let used_main = {
                    let is = &children[i].style;
                    match is.box_sizing {
                        BoxSizing::BorderBox => inner_main,
                        BoxSizing::ContentBox => {
                            let iem = is.font_size;
                            let pl = is.padding_left.resolve_or_zero(iem, cb, viewport);
                            let pr = is.padding_right.resolve_or_zero(iem, cb, viewport);
                            (inner_main - pl - pr
                                - is.border_left_width
                                - is.border_right_width)
                                .max(0.0)
                        }
                    }
                };
                // CSS Flexbox §9.8: percentage cross sizes (e.g. height:100%) resolve
                // against the flex container's definite cross size.
                // BUG-294: margin-box start — `lay_out_inner` adds `m_l`/`m_t` itself
                // (see the column arm above), so pre-adding them here double-counts.
                lay_out_with_used_size(
                    &mut children[i],
                    content_x + main_cursor,
                    content_y + cross_cursor,
                    inner_main,
                    explicit_cross,
                    measurer,
                    viewport,
                    pcb,
                    hp,
                    false,
                    UsedSizeOverride {
                        width: Some(used_main),
                        ..Default::default()
                    },
                );
                main_cursor += outer_main + item_gap + jc_gap;
                if auto_after {
                    main_cursor += auto_main_share;
                }
            }
        }

        // Align-items on cross axis for this line.
        let line_cross: f32 = if is_column {
            0.0 // column cross axis (width) not handled in wrap Phase 0
        } else {
            line_keys.iter().map(|&k| children[item_idxs[k]].rect.height).fold(0.0_f32, f32::max)
        };
        line_cross_sizes.push(line_cross);

        if !is_column {
            // CSS Flexbox §9.5: for a single-line (non-wrapping) flex container the line
            // cross size equals the container's inner cross size (if definite). This lets
            // align-items: center/end position items relative to the full container height
            // rather than just the tallest item in the line.
            let effective_cross = if !is_wrap {
                explicit_cross.unwrap_or(line_cross)
            } else {
                line_cross
            };
            for &k in line_keys {
                let i = item_idxs[k];
                let item = &mut children[i];
                let is = &item.style;
                let iem = is.font_size;
                let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
                let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
                let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
                // CSS Flexbox §8.1: auto-поле ПОПЕРЕЧНОЙ оси съедает свободное
                // место раньше `align-self`/`align-items` (и отменяет stretch):
                // два auto — по центру, одно — прижать к противоположному краю.
                let auto_cross_start = matches!(is.margin_top, LengthOrAuto::Auto);
                let auto_cross_end = matches!(is.margin_bottom, LengthOrAuto::Auto);
                let outer_cross = item.rect.height + m_t + m_b;
                if auto_cross_start || auto_cross_end {
                    let free = (effective_cross - outer_cross).max(0.0);
                    let shift = if auto_cross_start && auto_cross_end {
                        free / 2.0
                    } else if auto_cross_start {
                        free
                    } else {
                        0.0
                    };
                    let new_y = content_y + cross_cursor + m_t + shift;
                    shift_y_box(item, new_y - item.rect.y);
                    continue;
                }
                // The item was laid out at the line's cross-start (`content_y +
                // cross_cursor + m_t`). Cross alignment must move the *whole*
                // subtree, not just `rect.y`: the item's descendants were already
                // positioned in absolute coordinates during the main-axis pass, so
                // shifting only `rect.y` leaves nested content (e.g. an anonymous
                // text item's InlineRun) at the cross-start — BUG-194 (centered
                // digit labels stuck at the box top). Same rationale as BUG-165.
                match align {
                    AlignValue::End => {
                        let new_y = content_y + cross_cursor + effective_cross - outer_cross + m_t;
                        shift_y_box(item, new_y - item.rect.y);
                    }
                    AlignValue::Center => {
                        let new_y = content_y + cross_cursor + m_t + (effective_cross - outer_cross) / 2.0;
                        shift_y_box(item, new_y - item.rect.y);
                    }
                    AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                        // CSS Flexbox §9.5: stretch applies only when the item's cross size
                        // is auto (no explicit height). Items with explicit heights are not
                        // grown beyond their declared size.
                        let stretch_h = if is.height.is_none() {
                            (effective_cross - m_t - m_b).max(0.0)
                        } else {
                            item.rect.height
                        };
                        // BUG-104: a stretched item with no explicit height gains a
                        // definite block size it lacked during its own layout. If the
                        // item is itself a column flex container, its `flex-grow`
                        // children were collapsed to flex-basis against an indefinite
                        // main size — they must be re-laid-out against the stretched
                        // height so they fill it.
                        //
                        // BUG-209: gate the re-layout on a *definite* container cross
                        // size. When `explicit_cross` is None the effective cross size
                        // falls back to `line_cross` (the line's own tallest item), so
                        // the "stretch" is a no-op against the item's current height.
                        // Re-laying-out anyway writes a resolved px `style.height` back
                        // onto the item (below), which permanently clobbers its
                        // `height: auto` state. A later pass that *does* have a definite
                        // cross size then sees `is.height.is_some()` and skips the real
                        // stretch — collapsing nested flex cells to content height
                        // (TEST-90: cell-items stuck at ~40px instead of filling the row).
                        let relayout_column_flex = is.height.is_none()
                            && explicit_cross.is_some()
                            && stretch_h > 0.0
                            && matches!(is.display, Display::Flex | Display::InlineFlex)
                            && matches!(
                                is.flex_direction,
                                FlexDirection::Column | FlexDirection::ColumnReverse
                            );
                        if item.rect.height < stretch_h {
                            item.rect.height = stretch_h;
                        }
                        item.rect.y = content_y + cross_cursor + m_t;
                        if relayout_column_flex {
                            // Force border-box + explicit height so the definite main
                            // size is honoured regardless of the item's own box-sizing,
                            // then re-lay-out in place (origin/width already resolved).
                            let rx = item.rect.x;
                            let ry = item.rect.y;
                            let rw = item.rect.width;
                            lay_out_with_used_size(
                                item, rx, ry, rw, Some(stretch_h), measurer, viewport, pcb, hp, false,
                                UsedSizeOverride {
                                    height: Some(stretch_h),
                                    box_sizing: Some(BoxSizing::BorderBox),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    _ => {
                        item.rect.y = content_y + cross_cursor + m_t;
                    }
                }
            }
        }

        cross_cursor += line_cross + cross_gap;
    }

    // Remove the trailing cross gap accumulated by the loop. Each processed line
    // appends `line_cross + cross_gap` (5225), so after the loop there is always
    // exactly one surplus `cross_gap` — including single-line containers, where the
    // row-gap (from `gap`/`row-gap`) must NOT leak into the container's cross size
    // (nothing to separate). Subtract whenever at least one line was laid out.
    let mut total_cross = if n_lines > 0 {
        (cross_cursor - cross_gap).max(0.0)
    } else {
        cross_cursor
    };

    // Apply align-content to distribute remaining space between flex lines (row wrap only).
    // CSS Box Alignment L3: align-content applies to single-line wrapped containers too
    // (Chrome/Edge 103+ behavior). Removed `n_lines > 1` guard to match browsers.
    if !is_column && is_wrap {
        let line_gap_total = cross_gap * (n_lines.saturating_sub(1)) as f32;
        let used_cross: f32 = line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        let free_cross = explicit_cross.map_or(0.0, |h| (h - used_cross).max(0.0));

        if free_cross > 0.0 {
            let mut line_offsets: Vec<f32> = vec![0.0; n_lines];

            // CSS Box Alignment L3 §5.4: `normal`/`auto` align-content behaves as
            // `stretch` for flex containers. The default (`Auto`) therefore
            // distributes free cross-space by growing each flex line.
            let effective = match s.align_content {
                AlignValue::Auto | AlignValue::Normal => AlignValue::Stretch,
                other => other,
            };

            match effective {
                AlignValue::End => {
                    line_offsets.fill(free_cross);
                }
                AlignValue::Center => {
                    line_offsets.fill(free_cross / 2.0);
                }
                AlignValue::SpaceBetween if n_lines > 1 => {
                    let gap_per = free_cross / (n_lines - 1) as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate().skip(1) {
                        *offset = gap_per * i as f32;
                    }
                }
                AlignValue::SpaceAround => {
                    let per = free_cross / n_lines as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per / 2.0 + (per * i as f32);
                    }
                }
                AlignValue::SpaceEvenly => {
                    let per = free_cross / (n_lines + 1) as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per * (i as f32 + 1.0);
                    }
                }
                AlignValue::Stretch => {
                    // CSS Flexbox §8.3: positive free space is split EQUALLY between
                    // all flex lines, increasing each line's cross size. Items on a
                    // later line shift toward the cross-end by the cumulative growth
                    // of all preceding lines (each grown line pushes the next down).
                    let per = free_cross / n_lines as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per * i as f32;
                    }
                    for size in line_cross_sizes.iter_mut() {
                        *size += per;
                    }
                }
                _ => {
                }
            }

            for li in 0..n_lines {
                let line_keys = &lines[li];
                let offset = line_offsets[li];

                if !is_column && offset > 0.0 {
                    for &k in line_keys {
                        let i = item_idxs[k];
                        // Shift the whole item subtree, not just its own box: the
                        // item's descendants were already positioned in absolute
                        // coordinates during the flex layout pass, so an
                        // align-content offset must move them in lockstep. Bumping
                        // only `rect.y` would leave the item's content (and any
                        // nested flex lines) behind by `offset` — BUG-165.
                        shift_y_box(&mut children[i], offset);
                    }
                }
            }

            total_cross = line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        }
    }

    if is_column {
        // Column: return main-axis height (main_cursor from last line).
        // Re-compute from stored item positions.
        item_idxs
            .iter()
            .map(|&i| children[i].rect.y + children[i].rect.height - content_y)
            .fold(0.0_f32, f32::max)
    } else {
        total_cross
    }
}

/// CSS Box Alignment L3 §5 — content distribution along one axis of a grid container.
///
/// Returns `(start_offset, extra_gap)`: how far the first track is pushed away from
/// the content-box start edge, and how much spacing to insert between every pair of
/// adjacent tracks on top of the `gap` property.
///
/// # Arguments
/// * `align` — the used `align-content` / `justify-content` value.
/// * `free` — leftover space after all tracks and their gaps.
/// * `n` — number of tracks on the axis.
///
/// With non-positive free space the axis overflows, and §5.3 replaces the
/// distribution with its fallback alignment — `space-between` → `start`,
/// `space-around` / `space-evenly` → `center` — after which the alignment is
/// resolved *unsafely*: `center` / `end` still shift the tracks back past the
/// content-box start edge (a negative offset), matching Edge. `safe` / `unsafe`
/// are not parsed, so the unsafe behaviour is unconditional.
///
/// `normal` / `stretch` always return `(0, 0)` — that pair is handled by the track
/// sizing pass, which hands the free space to the auto-sized tracks instead.
fn grid_content_distribution(align: AlignValue, free: f32, n: usize) -> (f32, f32) {
    if n == 0 {
        return (0.0, 0.0);
    }
    if free <= 0.0 {
        return match align {
            AlignValue::End => (free, 0.0),
            // `center` directly, plus the two distributions that fall back to it.
            AlignValue::Center | AlignValue::SpaceAround | AlignValue::SpaceEvenly => {
                (free / 2.0, 0.0)
            }
            // `start`, `space-between` (falls back to `start`), `normal`, `stretch`.
            _ => (0.0, 0.0),
        };
    }
    match align {
        AlignValue::End => (free, 0.0),
        AlignValue::Center => (free / 2.0, 0.0),
        AlignValue::SpaceBetween => {
            // A single track has no in-between gap — the spec falls back to `start`.
            if n <= 1 { (0.0, 0.0) } else { (0.0, free / (n - 1) as f32) }
        }
        AlignValue::SpaceAround => {
            let per = free / n as f32;
            (per / 2.0, per)
        }
        AlignValue::SpaceEvenly => {
            let per = free / (n + 1) as f32;
            (per, per)
        }
        _ => (0.0, 0.0),
    }
}

/// Size of the cell spanning tracks `t0..t1` (0-based, end-exclusive), measured from
/// the resolved track offsets.
///
/// Deriving the span from offsets rather than summing sizes + `gap` keeps spanning
/// items correct when `align-content` / `justify-content` injected extra spacing
/// between tracks (`space-between` and friends).
fn grid_track_span(offsets: &[f32], sizes: &[f32], t0: usize, t1: usize) -> f32 {
    let last = t1.max(t0 + 1) - 1;
    match (offsets.get(t0), offsets.get(last), sizes.get(last)) {
        (Some(&o0), Some(&o_last), Some(&s_last)) => (o_last + s_last - o0).max(0.0),
        _ => sizes.get(t0).copied().unwrap_or(0.0),
    }
}

/// CSS Grid Layout Level 1 — grid container layout.
///
/// Implements a Phase-0 subset of the grid layout algorithm (CSS Grid L1 §12):
///
/// - Explicit track lists (grid-template-columns / rows) with px, fr, auto.
/// - `repeat(N, size)` expansion.
/// - `minmax(min, max)` — min side used for sizing.
/// - Integer line numbers (positive only), `span N`, and `auto` placement.
/// - `grid-auto-flow: row | column` (no dense packing).
/// - `gap` / `column-gap` / `row-gap` between cells.
/// - `align-items` / `justify-items` within cells.
/// - `align-content` / `justify-content` (and the `place-content` shorthand)
///   distributing the container's free space between tracks — CSS Box Alignment
///   L3 §5 / CSS Grid L1 §12.3.
///
/// `definite_content_height` is the container's content-box block size when it is
/// definite (explicit `height`, box-sizing already applied), `None` when the height
/// is derived from the content. Only a definite height leaves block-axis free space
/// for `align-content` to distribute.
///
/// Returns the total content height of the grid.
#[allow(clippy::too_many_arguments)]
fn lay_out_grid(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    definite_content_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let em = s.font_size;

    // CSS Grid L2 §9: If this grid was set up as a subgrid by its parent, read
    // the inherited track contexts that the parent set in the thread-locals.
    // We clear them immediately so our own children don't accidentally inherit them.
    let inherited_cols: Option<SubgridContext> = SUBGRID_COL_CTX.with(|c| c.borrow_mut().take());
    let inherited_rows: Option<SubgridContext> = SUBGRID_ROW_CTX.with(|c| c.borrow_mut().take());

    // Indices of actual items (non-Skip).
    let mut item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();
    // CSS Grid §6: grid items are placed in "modified document order" — source order
    // reordered by the `order` property. A stable sort preserves source order among
    // items with equal `order`, so auto-placement honours `order` like Edge does.
    item_idxs.sort_by_key(|&i| children[i].style.order);

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Gap between tracks.  When the axis is subgridded we use the parent's gap
    // (already baked into the offsets in SubgridContext); fall back to our own style.
    let col_gap = inherited_cols.as_ref()
        .map(|ctx| ctx.gap)
        .unwrap_or_else(|| s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0));
    let row_gap = inherited_rows.as_ref()
        .map(|ctx| ctx.gap)
        .unwrap_or_else(|| s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0));

    // CSS Grid L1 §7.2.3.4 — Phase 2: expand repeat(auto-fill|auto-fit, ...) at layout time.
    // If the style carried auto-repeat metadata, resolve the track count and build an expanded list.
    let auto_fill_col_tracks: Vec<GridTrackSize> =
        if let Some(ref rep) = s.grid_template_col_auto_repeat {
            let n = resolve_auto_fill_fit_count(content_width, &rep.tracks, col_gap).max(1);
            let mut tracks = Vec::with_capacity(n * rep.tracks.len());
            for _ in 0..n {
                tracks.extend_from_slice(&rep.tracks);
            }
            tracks
        } else {
            Vec::new()
        };
    let eff_col_template: &[GridTrackSize] = if s.grid_template_col_auto_repeat.is_some() {
        &auto_fill_col_tracks
    } else {
        &s.grid_template_columns
    };

    // CSS Masonry Layout (CSS Grid L3 §14) is not shipped by any stable browser —
    // Edge/Chrome treat `masonry` as an invalid track value and drop it, so the axis
    // falls back to `none` (a regular auto-sized grid). We match that ground truth:
    // strip the `masonry` sentinel from the effective track list on whichever axis
    // carries it, then fall through to the normal grid placement algorithm below.
    let col_is_masonry = eff_col_template.first() == Some(&GridTrackSize::Masonry);
    let row_is_masonry = s.grid_template_rows.first() == Some(&GridTrackSize::Masonry);
    let eff_col_template: &[GridTrackSize] = if col_is_masonry { &[] } else { eff_col_template };
    let eff_row_template: &[GridTrackSize] = if row_is_masonry { &[] } else { &s.grid_template_rows };

    // Determine explicit track counts.
    // Subgrid sentinel `[Subgrid]` is a single-element vec meaning "inherit all parent tracks";
    // for placement purposes use the number of inherited tracks (or 1 for auto-placement).
    let n_explicit_cols = if eff_col_template.first() == Some(&GridTrackSize::Subgrid) {
        inherited_cols.as_ref().map(|ctx| ctx.sizes.len()).unwrap_or(1).max(1)
    } else {
        eff_col_template.len().max(1)
    };

    // --- Step 1: Resolve placements for every item ---
    // placement: (col_start, col_end, row_start, row_end) all 1-based inclusive/exclusive.
    let mut placements: Vec<(u32, u32, u32, u32)> = vec![(0, 0, 0, 0); item_idxs.len()];

    let row_flow = !matches!(s.grid_auto_flow, GridAutoFlow::Column | GridAutoFlow::ColumnDense);

    // Pass 1: items with fully explicit placements.
    for (k, &i) in item_idxs.iter().enumerate() {
        let is = &children[i].style;

        // Resolve named area references first (grid-area: <name> shorthand or
        // individual grid-{row,column}-{start,end}: <name> values).
        let (named_cs, named_ce, named_rs, named_re) = {
            let has_named = matches!(&is.grid_column_start, GridLine::Named(_))
                || matches!(&is.grid_column_end, GridLine::Named(_))
                || matches!(&is.grid_row_start, GridLine::Named(_))
                || matches!(&is.grid_row_end, GridLine::Named(_));
            if has_named && !s.grid_template_areas.is_empty() {
                resolve_named_lines(
                    &is.grid_column_start,
                    &is.grid_column_end,
                    &is.grid_row_start,
                    &is.grid_row_end,
                    &s.grid_template_areas,
                )
            } else {
                (0, 0, 0, 0)
            }
        };

        // For each axis: use resolved named value if non-zero, else fall back to
        // the normal numeric/span resolver.
        let cs = if named_cs != 0 { named_cs } else { resolve_grid_line(&is.grid_column_start, n_explicit_cols as u32) };
        let ce = if named_ce != 0 { named_ce } else { resolve_grid_line_end(&is.grid_column_end, cs, n_explicit_cols as u32) };
        let rs = if named_rs != 0 { named_rs } else { resolve_grid_line(&is.grid_row_start, 0) };
        let re = if named_re != 0 { named_re } else { resolve_grid_line_end(&is.grid_row_end, rs, 0) };

        // `grid-column: span N` → start=Span(N), end=Auto → cs=0, ce=0.
        // resolve_grid_line returns 0 for Span-on-start, losing the count.
        // Recover the span so Pass 2 can use it for placement sizing.
        let ce = if ce == 0 {
            match &is.grid_column_start { GridLine::Span(n) => *n, _ => 0 }
        } else { ce };
        let re = if re == 0 {
            match &is.grid_row_start { GridLine::Span(n) => *n, _ => 0 }
        } else { re };

        if cs != 0 && rs != 0 {
            // Fully explicit: both axes known.
            placements[k] = (cs, ce, rs, re);
        } else if cs != 0 {
            // Column position fixed, row auto; preserve row-span if declared.
            placements[k] = (cs, ce, 0, re);
        } else if rs != 0 {
            // Row position fixed, column auto; preserve col-span if declared.
            placements[k] = (0, ce, rs, re);
        } else if ce > 0 || re > 0 {
            // Both axes auto but at least one span is declared (e.g. grid-column:span 2).
            // Store so pass-2 can recover the span via `end - 0 = span`.
            placements[k] = (0, ce, 0, re);
        }
        // All-auto no spans: stays (0,0,0,0) → span=1 in pass 2.
    }

    // Pass 2: auto-place remaining items — CSS Grid L1 §8.5 auto-placement algorithm.
    //
    // Two packing modes:
    //   Sparse (grid-auto-flow: row | column): cursor only moves forward.
    //   Dense  (grid-auto-flow: row dense | column dense): each item scans from
    //          (1,1) so it can fill gaps left by larger items.
    //
    // Occupancy HashSet replaces the O(k²) overlap scan from Pass 1 with O(1)
    // per-cell lookups.
    let dense = matches!(s.grid_auto_flow, GridAutoFlow::RowDense | GridAutoFlow::ColumnDense);
    let mut occupied: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for &(cs, ce, rs, re) in &placements {
        if cs != 0 && rs != 0 {
            for r in rs..re {
                for c in cs..ce {
                    occupied.insert((c, r));
                }
            }
        }
    }

    let mut cursor_row: u32 = 1;
    let mut cursor_col: u32 = 1;

    for (k, _) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs != 0 && rs != 0 {
            continue; // explicitly placed
        }

        let col_span = if ce > cs { ce - cs } else { 1 };
        let row_span = if re > rs { re - rs } else { 1 };

        if row_flow {
            let fixed_cs = if cs != 0 { cs } else { 0 };
            let fixed_ce = if cs != 0 { ce } else { 0 };

            // Dense packing starts each scan from (1,1); sparse continues from cursor.
            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            // BUG-801: the column bound below must never be able to reject
            // EVERY scan position, or the loop has no exit. Two ways that
            // happened: an auto-placed item whose own `col_span` exceeds
            // `n_explicit_cols` (`grid-column: span 3` on a 2-column grid)
            // failed `fits` at every column, since the bound never grew past
            // the explicit track count — CSS Grid L1 §7.1 grows the implicit
            // grid to fit such an item rather than refusing it, so the bound
            // here does too. An item with an EXPLICIT column start beyond the
            // explicit grid (`grid-column: 9 / span 2` on 2 columns,
            // `fixed_cs != 0`) failed the same check at every row, since
            // `try_ce_val` is fixed and never changes — that placement is not
            // a search at all, so it is exempted from the bound entirely and
            // only occupancy still applies.
            let col_bound = (n_explicit_cols as u32).max(col_span);

            loop {
                let try_c   = if fixed_cs != 0 { fixed_cs } else { scan_c };
                let try_ce_val = if fixed_cs != 0 { fixed_ce } else { try_c + col_span };

                // Bounds: item must fit within the (possibly grid-grown) column count.
                let fits = fixed_cs != 0 || (try_ce_val - 1) <= col_bound;
                let cell_free = fits && (try_c..try_ce_val)
                    .all(|c| (scan_r..scan_r + row_span).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (try_c, try_ce_val, scan_r, scan_r + row_span);
                    for r in scan_r..scan_r + row_span {
                        for c in try_c..try_ce_val {
                            occupied.insert((c, r));
                        }
                    }
                    // Track highest placed row for grid-size calculation.
                    cursor_row = cursor_row.max(scan_r);
                    if !dense {
                        cursor_col = try_ce_val;
                        if cursor_col > n_explicit_cols as u32 {
                            cursor_col = 1;
                            cursor_row += 1;
                        }
                    }
                    break;
                }

                // Advance scan position.
                if fixed_cs != 0 {
                    scan_r += 1;
                    scan_c = 1;
                } else {
                    scan_c += 1;
                    if scan_c > n_explicit_cols as u32 {
                        scan_c = 1;
                        scan_r += 1;
                    }
                }
            }
        } else {
            // Column flow: fill top-to-bottom, wrap to next column.
            let n_explicit_rows = eff_row_template.len().max(1) as u32;
            let fixed_rs = if rs != 0 { rs } else { 0 };
            let fixed_re = if rs != 0 { re } else { 0 };

            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            // BUG-801, column-flow mirror of the row-flow fix above.
            let row_bound = n_explicit_rows.max(row_span);

            loop {
                let try_r      = if fixed_rs != 0 { fixed_rs } else { scan_r };
                let try_re_val = if fixed_rs != 0 { fixed_re } else { try_r + row_span };

                let fits = fixed_rs != 0 || (try_re_val - 1) <= row_bound;
                let cell_free = fits && (scan_c..scan_c + col_span)
                    .all(|c| (try_r..try_re_val).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (scan_c, scan_c + col_span, try_r, try_re_val);
                    for r in try_r..try_re_val {
                        for c in scan_c..scan_c + col_span {
                            occupied.insert((c, r));
                        }
                    }
                    cursor_col = cursor_col.max(scan_c);
                    if !dense {
                        cursor_row = try_re_val;
                        if cursor_row > n_explicit_rows {
                            cursor_row = 1;
                            cursor_col += 1;
                        }
                    }
                    break;
                }

                if fixed_rs != 0 {
                    scan_c += 1;
                    scan_r = 1;
                } else {
                    scan_r += 1;
                    if scan_r > n_explicit_rows {
                        scan_r = 1;
                        scan_c += 1;
                    }
                }
            }
        }
    }

    // --- Step 2: Determine total grid dimensions ---
    let n_cols = placements.iter().map(|&(_, ce, _, _)| ce.saturating_sub(1)).max().unwrap_or(1)
        .max(n_explicit_cols as u32);
    let n_rows = placements.iter().map(|&(_, _, _, re)| re.saturating_sub(1)).max().unwrap_or(1);

    // --- Step 3: Compute column widths ---
    // If the column axis is subgridded, use the inherited track sizes directly;
    // otherwise compute from the style as usual (CSS Grid L2 §9).
    let (col_widths, col_offsets) = if let Some(ref ctx) = inherited_cols {
        // Subgrid column axis: clip to n_cols (parent may span more tracks than
        // the explicit template; auto-place inside those tracks).
        let sizes: Vec<f32> = ctx.sizes.iter().take(n_cols as usize).cloned().collect();
        let offsets: Vec<f32> = ctx.offsets.iter().take(n_cols as usize).cloned().collect();
        (sizes, offsets)
    } else {
        // Normal grid: compute column widths from the style.
        let mut col_widths: Vec<f32> = (0..n_cols)
            .map(|c| {
                let ts = grid_track(c, eff_col_template, &s.grid_auto_columns);
                match ts {
                    GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                    GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                    // Subgrid sentinel without parent context — fall back to auto.
                    GridTrackSize::Subgrid => 0.0,
                    _ => 0.0, // fr / auto resolved later
                }
            })
            .collect();

        // Total gap between columns.
        let total_col_gap = if n_cols > 1 { col_gap * (n_cols - 1) as f32 } else { 0.0 };
        let fixed_col_total: f32 = col_widths.iter().sum::<f32>() + total_col_gap;
        let free_col = (content_width - fixed_col_total).max(0.0);

        // Distribute fr among column tracks.
        let total_fr: f32 = (0..n_cols)
            .map(|c| grid_track(c, eff_col_template, &s.grid_auto_columns).fr().unwrap_or(0.0))
            .sum();
        let auto_col_count = (0..n_cols)
            .filter(|&c| matches!(
                grid_track(c, eff_col_template, &s.grid_auto_columns),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent
            ))
            .count();

        // For auto columns, divide remaining free space equally (after fr).
        let fr_width = if total_fr > 0.0 { free_col / total_fr } else { 0.0 };
        let auto_col_width = if auto_col_count > 0 && total_fr == 0.0 {
            free_col / auto_col_count as f32
        } else {
            0.0
        };

        for c in 0..n_cols {
            match grid_track(c, eff_col_template, &s.grid_auto_columns) {
                GridTrackSize::Fr(f) => col_widths[c as usize] = (f * fr_width).max(0.0),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent => {
                    col_widths[c as usize] = auto_col_width;
                }
                _ => {}
            }
        }

        // CSS Box Alignment L3 §5 — `justify-content` distributes whatever inline-axis
        // space the tracks left over. `fr` / `auto` tracks already absorb it during
        // sizing above, so this only ever fires for a fixed-size track list.
        let used_col_total: f32 = col_widths.iter().sum::<f32>() + total_col_gap;
        let (jc_start, jc_extra) = grid_content_distribution(
            s.justify_content,
            content_width - used_col_total,
            n_cols as usize,
        );

        // Column start offsets.
        let mut col_offsets: Vec<f32> = Vec::with_capacity(n_cols as usize);
        let mut x_off = jc_start;
        for c in 0..n_cols {
            col_offsets.push(x_off);
            x_off += col_widths[c as usize]
                + if c < n_cols - 1 { col_gap + jc_extra } else { 0.0 };
        }

        (col_widths, col_offsets)
    };

    // --- Step 4: Layout items to measure row heights ---
    // If the row axis is subgridded, use inherited sizes; otherwise compute from style.
    let mut row_heights: Vec<f32> = if let Some(ref ctx) = inherited_rows {
        ctx.sizes.iter().take(n_rows as usize).cloned().collect()
    } else {
        (0..n_rows)
            .map(|r| {
                match grid_track(r, eff_row_template, &s.grid_auto_rows) {
                    GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                    GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                    GridTrackSize::Subgrid => 0.0,
                    _ => 0.0,
                }
            })
            .collect()
    };

    // Row offsets (computed from row_heights regardless of subgrid).
    // For subgrid row axis the offsets are inherited below in final pass.

    // BUG-341 S33: this probe pass and Step 5's final positioning pass below
    // always call `lay_out` with the exact same `(width, height=None)` for a
    // given non-subgrid item — `col_offsets`/`col_widths` are resolved once,
    // above this loop, and nothing between here and Step 5 touches them again,
    // so `cell_w` is bit-identical for both passes *by construction*, not just
    // "happens to match" the way S30-S32's general `(node, width, height)`
    // cache could only ever hope for. Stash each non-subgrid item's probe
    // result and reuse it directly in Step 5 instead of laying the subtree out
    // twice — the one real redundancy the S28-S32 general layout-result cache
    // slices ever found a case for, captured here with zero overhead on every
    // other box in the document (no thread-local `HashMap`, no per-call key,
    // nothing paid on a miss that never repeats — see `CV_AUTO_TOUCHED`'s doc
    // comment for why the general mechanism was removed instead of kept).
    //
    // Subgrid items are excluded: their own recursive `lay_out_grid` reads a
    // thread-local track context (`SubgridContextGuard`, set in both arms
    // below) that genuinely differs between this estimated-tracks probe and
    // Step 5's resolved-tracks final pass.
    let mut probe_reuse: Vec<Option<(f32, f32, LayoutBox)>> = vec![None; item_idxs.len()];

    // Layout each item in its cell to determine content height.
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs == 0 || rs == 0 {
            continue; // unplaced (should not happen after auto-placement)
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let cell_w: f32 = grid_track_span(&col_offsets, &col_widths, c0, c1);

        // For subgrid children: set the thread-local context before laying out.
        let child_col_subgrid = children[i].style.grid_template_columns.first()
            == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = children[i].style.grid_template_rows.first()
            == Some(&GridTrackSize::Subgrid);

        if child_col_subgrid || child_row_subgrid {
            // Build subgrid context slices from our resolved track sizes.
            let child_col_ctx = if child_col_subgrid && c1 > c0 {
                Some(SubgridContext::from_parent_tracks(&col_widths[c0..c1], col_gap))
            } else {
                None
            };
            let child_row_ctx = if child_row_subgrid {
                // Row heights not fully determined yet; pass current estimates.
                let r0 = (rs - 1).min(n_rows - 1) as usize;
                let re_eff = re.max(rs + 1);
                let r1 = (re_eff - 1).min(n_rows) as usize;
                if r1 > r0 {
                    Some(SubgridContext::from_parent_tracks(&row_heights[r0..r1], row_gap))
                } else {
                    None
                }
            } else {
                None
            };
            let _guard = SubgridContextGuard::set(child_col_ctx, child_row_ctx);
            lay_out(&mut children[i], content_x + col_offsets.get(c0).copied().unwrap_or(0.0), 0.0, cell_w, None, measurer, viewport, pcb, hp, false);
        } else {
            // Layout at temporary position (y=0) to get intrinsic height.
            let probe_x = content_x + col_offsets.get(c0).copied().unwrap_or(0.0);
            let probe_y = 0.0;
            let outer_cv_touched = CV_AUTO_TOUCHED.with(|c| c.replace(false));
            lay_out(&mut children[i], probe_x, probe_y, cell_w, None, measurer, viewport, pcb, hp, false);
            let touched_here = CV_AUTO_TOUCHED.with(|c| c.get());
            CV_AUTO_TOUCHED.with(|c| c.set(outer_cv_touched || touched_here));
            if !touched_here {
                probe_reuse[k] = Some((probe_x, probe_y, children[i].clone()));
            }
        }

        // Update auto row heights.
        let r0 = (rs - 1) as usize;
        if r0 < row_heights.len()
            && inherited_rows.is_none()
            && matches!(
                grid_track(r0 as u32, eff_row_template, &s.grid_auto_rows),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent | GridTrackSize::Fr(_)
            )
        {
            let item_h = children[i].rect.height;
            if item_h > row_heights[r0] {
                row_heights[r0] = item_h;
            }
        }
    }

    // Resolve fr row heights (skip when row axis is subgridded — sizes are fixed).
    let total_row_gap = if n_rows > 1 { row_gap * (n_rows - 1) as f32 } else { 0.0 };
    if inherited_rows.is_none() {
        // CSS Grid L1 §11.7 — the free space available to flexible (`fr`) tracks is
        // the container's content size minus the base sizes of the OTHER tracks
        // only. `row_heights[r]` for an `fr` track was seeded from its content's
        // probed intrinsic height above (the fallback used when the container's
        // block size is indefinite) — that probed value is a floor for the final
        // `.max()` below, not a "fixed" size to subtract here. Counting it against
        // `definite_content_height` double-dips: a two-row `1fr 1fr` grid with
        // ~29px-tall cell content and a 220px definite height wrongly landed each
        // row at (220 - 29*2) / 2 ≈ 83px instead of 220 / 2 = 110px, leaving a
        // ~58px unaccounted gap at the bottom (found via TEST-62 BUG-277 triage).
        let fixed_row_total: f32 = (0..n_rows)
            .map(|r| {
                if grid_track(r, eff_row_template, &s.grid_auto_rows).fr().is_some() {
                    0.0
                } else {
                    row_heights[r as usize]
                }
            })
            .sum::<f32>()
            + total_row_gap;
        // If container has explicit height, distribute fr rows from it.
        let free_row = definite_content_height.map(|h| (h - fixed_row_total).max(0.0)).unwrap_or(0.0);
        let total_row_fr: f32 = (0..n_rows)
            .map(|r| grid_track(r, eff_row_template, &s.grid_auto_rows).fr().unwrap_or(0.0))
            .sum();
        if total_row_fr > 0.0 && free_row > 0.0 {
            let fr_h = free_row / total_row_fr;
            for r in 0..n_rows {
                if let Some(f) = grid_track(r, eff_row_template, &s.grid_auto_rows).fr() {
                    row_heights[r as usize] = (f * fr_h).max(row_heights[r as usize]);
                }
            }
        }

        // CSS Grid L1 §12.3 — `align-content: normal` behaves as `stretch` for a grid
        // container: the leftover block-axis space is shared equally between the
        // `auto`-sized rows. Only an explicitly sized container has leftover space.
        // Deferred: `minmax(_, auto)` rows do not participate — the track-sizing pass
        // above resolves them from their min side, not as auto.
        if matches!(s.align_content, AlignValue::Auto | AlignValue::Normal | AlignValue::Stretch) {
            let auto_rows: Vec<u32> = (0..n_rows)
                .filter(|&r| matches!(grid_track(r, eff_row_template, &s.grid_auto_rows), GridTrackSize::Auto))
                .collect();
            let used: f32 = row_heights.iter().sum::<f32>() + total_row_gap;
            let free = definite_content_height.map(|h| h - used).unwrap_or(0.0);
            if free > 0.0 && !auto_rows.is_empty() {
                let per = free / auto_rows.len() as f32;
                for r in auto_rows {
                    row_heights[r as usize] += per;
                }
            }
        }
    }

    // Row top offsets: if row axis is subgridded, use inherited offsets; else compute.
    let (row_offsets, y_off) = if let Some(ref ctx) = inherited_rows {
        let offsets: Vec<f32> = ctx.offsets.iter().take(n_rows as usize).cloned().collect();
        let total = ctx.total_size();
        (offsets, total)
    } else {
        // CSS Box Alignment L3 §5 — `align-content` distributes the block-axis free
        // space left over by the tracks (only ever non-zero for a definite height).
        let used_row_total: f32 = row_heights.iter().sum::<f32>() + total_row_gap;
        let (ac_start, ac_extra) = grid_content_distribution(
            s.align_content,
            definite_content_height.map(|h| h - used_row_total).unwrap_or(0.0),
            n_rows as usize,
        );

        let mut row_offsets: Vec<f32> = Vec::with_capacity(n_rows as usize);
        let mut y_off = ac_start;
        for r in 0..n_rows {
            row_offsets.push(y_off);
            y_off += row_heights[r as usize]
                + if r < n_rows - 1 { row_gap + ac_extra } else { 0.0 };
        }
        (row_offsets, y_off)
    };
    let mut y_off = y_off;

    // --- Step 5: Final positioning pass ---
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs == 0 || rs == 0 {
            // Unplaced — stack below grid content.
            lay_out(&mut children[i], content_x, content_y + y_off, content_width, None, measurer, viewport, pcb, hp, false);
            y_off += children[i].rect.height;
            continue;
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let r0 = (rs - 1).min(n_rows - 1) as usize;
        let r1 = (re - 1).min(n_rows) as usize;

        let cell_x = content_x + col_offsets.get(c0).copied().unwrap_or(0.0);
        let cell_y = content_y + row_offsets.get(r0).copied().unwrap_or(0.0);
        let cell_w: f32 = grid_track_span(&col_offsets, &col_widths, c0, c1);
        let cell_h: f32 = grid_track_span(&row_offsets, &row_heights, r0, r1);

        // Re-layout with final cell width. For subgrid children, restore the context.
        let child_col_subgrid = children[i].style.grid_template_columns.first()
            == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = children[i].style.grid_template_rows.first()
            == Some(&GridTrackSize::Subgrid);
        if child_col_subgrid || child_row_subgrid {
            let final_col_ctx = if child_col_subgrid && c1 > c0 {
                Some(SubgridContext::from_parent_tracks(&col_widths[c0..c1], col_gap))
            } else {
                None
            };
            let final_row_ctx = if child_row_subgrid && r1 > r0 {
                Some(SubgridContext::from_parent_tracks(&row_heights[r0..r1], row_gap))
            } else {
                None
            };
            let _guard = SubgridContextGuard::set(final_col_ctx, final_row_ctx);
            lay_out(&mut children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp, false);
        } else if let Some((probe_x, probe_y, mut reused)) = probe_reuse[k].take() {
            // BUG-341 S33: `cell_w` above was derived from the same
            // `col_offsets`/`col_widths`/`(c0, c1)` as the probe pass's, so
            // the subtree reused here already has the correct final size —
            // only its position needs to catch up to the resolved row offset.
            crate::incremental::translate_subtree(&mut reused, cell_x - probe_x, cell_y - probe_y);
            children[i] = reused;
        } else {
            // No usable probe: an unplaced-at-probe-time item can't reach
            // here (handled by the early-continue above), so this is a
            // subtree whose probe touched `content-visibility: auto` and was
            // refused for reuse (see `CV_AUTO_TOUCHED`'s doc comment).
            lay_out(&mut children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp, false);
        }

        let item = &mut children[i];
        let is = &item.style;
        let iem = is.font_size;
        let m_t = is.margin_top.resolve_or_zero(iem, content_width, viewport);
        let m_b = is.margin_bottom.resolve_or_zero(iem, content_width, viewport);
        let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
        let m_r = is.margin_right.resolve_or_zero(iem, content_width, viewport);

        // align-items (cross / block axis within cell).
        let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
        let item_outer_h = item.rect.height + m_t + m_b;
        match align {
            AlignValue::End => {
                item.rect.y = cell_y + cell_h - item.rect.height - m_b;
            }
            AlignValue::Center => {
                item.rect.y = cell_y + (cell_h - item_outer_h) / 2.0 + m_t;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                // CSS Grid §11.2: `stretch` only grows items whose used block size is
                // `auto`; an explicit `height` is preserved (the item is top-aligned in
                // the cell, leaving free space below — like Edge).
                if is.height.is_none() && item.rect.height < cell_h - m_t - m_b {
                    item.rect.height = (cell_h - m_t - m_b).max(item.rect.height);
                }
                item.rect.y = cell_y + m_t;
            }
            _ => {
                item.rect.y = cell_y + m_t;
            }
        }

        // justify-items (inline axis within cell).
        let justify = if matches!(is.justify_self, AlignValue::Auto) { s.justify_items } else { is.justify_self };
        let item_outer_w = item.rect.width + m_l + m_r;
        match justify {
            AlignValue::End => {
                item.rect.x = cell_x + cell_w - item.rect.width - m_r;
            }
            AlignValue::Center => {
                item.rect.x = cell_x + (cell_w - item_outer_w) / 2.0 + m_l;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                item.rect.x = cell_x + m_l;
            }
            _ => {
                item.rect.x = cell_x + m_l;
            }
        }
    }

    y_off
}

/// CSS Grid Layout L3 §9 — Resolve `repeat(auto-fill|auto-fit, <track-list>)` count.
/// Returns the number of tracks to fill the available space when using auto-fill or auto-fit.
///
/// # Arguments
/// * `available_width` — CSS px width of the container content box.
/// * `track_sizes` — The track sizes inside the repeat(), e.g. `[minmax(100px, 1fr)]`.
/// * `gap` — Column gap in px.
/// * `auto_fit` — If true, resolve as auto-fit (collapse empty tracks); else auto-fill.
///
/// # Returns
/// The minimum number of tracks that fit in available space, with preference
/// for auto-fill (leave empty) over auto-fit (collapse).
pub fn resolve_auto_fill_fit_count(
    available_width: f32,
    track_sizes: &[GridTrackSize],
    gap: f32,
) -> usize {
    if track_sizes.is_empty() || available_width <= 0.0 {
        return 1; // At least one track
    }

    // Compute minimum track width: the min() sizing function of each track.
    // For minmax(min, max), use min. For auto/fr/max-content, use 0 as placeholder (content-sized).
    let mut track_min_width: f32 = 0.0;
    for track in track_sizes {
        let w = match track {
            GridTrackSize::Length(len) => {
                // Fixed length: use as-is (simplified: only px supported in this pass)
                len.resolve(1.0, Some(available_width), Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            GridTrackSize::Minmax(min, _max) => {
                // Use the min() part
                min.resolve_fixed(1.0, available_width, Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            GridTrackSize::FitContent(limit) => {
                // Use the limit as min sizing (simplified)
                limit.resolve_fixed(1.0, available_width, Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            // Auto, MinContent, MaxContent, Fr, Subgrid: no fixed minimum, use 0
            _ => 0.0,
        };
        track_min_width = track_min_width.max(w);
    }

    // Count tracks: (available_width + gap) / (track_min_width + gap), minimum 1.
    let gap_adjusted_available = available_width + gap;
    let track_plus_gap = track_min_width + gap;

    if track_plus_gap <= 0.0 {
        1
    } else {
        ((gap_adjusted_available / track_plus_gap).floor() as usize).max(1)
    }
}

/// Return the track size for track index `idx` (0-based) from a template list,
/// falling back to `auto_track` for implicit tracks beyond the template.
fn grid_track<'a>(idx: u32, template: &'a [GridTrackSize], auto_track: &'a GridTrackSize) -> &'a GridTrackSize {
    template.get(idx as usize).unwrap_or(auto_track)
}

/// Resolve a `GridLine` to a 1-based track number, or 0 if auto.
fn resolve_grid_line(line: &GridLine, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => 0,
        GridLine::Line(n) => {
            if *n > 0 {
                *n as u32
            } else if n_tracks > 0 {
                // Negative line numbers count from the end.
                (n_tracks as i32 + 1 + n).max(1) as u32
            } else {
                1
            }
        }
        GridLine::Span(_) => 0, // span on start — auto
    }
}

/// Resolve a grid-line end given start position and span.
fn resolve_grid_line_end(line: &GridLine, start: u32, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => {
            if start > 0 { start + 1 } else { 0 }
        }
        GridLine::Line(n) => {
            if *n > 0 {
                (*n as u32).max(start + 1)
            } else if n_tracks > 0 {
                let abs = (n_tracks as i32 + 1 + n).max(1) as u32;
                abs.max(start + 1)
            } else {
                start + 1
            }
        }
        GridLine::Span(n) => {
            // When start is known: end = start + span.
            // When start is auto (0): store span N directly so pass-2 placement
            // can use `re - rs = N - 0 = N` to recover the span count.
            if start > 0 { start + n } else { *n }
        }
    }
}

/// CSS Grid L1 §7.3 — locate a named area in `grid-template-areas`.
///
/// Returns `(row_start, row_end, col_start, col_end)` as 1-based exclusive
/// line numbers, or `None` if the name is not found. Handles rectangular
/// area shapes only (CSS Grid L1 requires areas to be rectangular).
fn find_named_area(areas: &[Vec<String>], name: &str) -> Option<(u32, u32, u32, u32)> {
    let mut row_start: Option<u32> = None;
    let mut row_end: Option<u32> = None;
    let mut col_start: Option<u32> = None;
    let mut col_end: Option<u32> = None;
    for (r, row) in areas.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell == name {
                let rs = (r + 1) as u32;
                let re = (r + 2) as u32;
                let cs = (c + 1) as u32;
                let ce = (c + 2) as u32;
                row_start = Some(row_start.map_or(rs, |v: u32| v.min(rs)));
                row_end   = Some(row_end.map_or(re,   |v: u32| v.max(re)));
                col_start = Some(col_start.map_or(cs, |v: u32| v.min(cs)));
                col_end   = Some(col_end.map_or(ce,   |v: u32| v.max(ce)));
            }
        }
    }
    Some((row_start?, row_end?, col_start?, col_end?))
}

/// Resolve named grid-line references for a single item against the
/// container's `grid-template-areas`. Returns `(col_start, col_end, row_start, row_end)`.
///
/// When all four placement properties are `Named(same_name)` (set by
/// `grid-area: <name>` shorthand), the area bounds are looked up once and
/// applied to all four axes. Mixed named/unnamed configurations fall back
/// to `Auto` (0) for any unresolved axis.
fn resolve_named_lines(
    col_start: &GridLine,
    col_end: &GridLine,
    row_start: &GridLine,
    row_end: &GridLine,
    areas: &[Vec<String>],
) -> (u32, u32, u32, u32) {
    // When grid-area: <name> sets all four to Named(name), resolve as one area.
    if let (
        GridLine::Named(n_cs),
        GridLine::Named(n_ce),
        GridLine::Named(n_rs),
        GridLine::Named(n_re),
    ) = (col_start, col_end, row_start, row_end)
        && n_cs == n_ce
        && n_ce == n_rs
        && n_rs == n_re
        && let Some((rs, re, cs, ce)) = find_named_area(areas, n_cs)
    {
        return (cs, ce, rs, re);
    }
    // Partial Named references: each axis resolved independently.
    let cs = if let GridLine::Named(n) = col_start {
        find_named_area(areas, n).map_or(0, |(_, _, cs, _)| cs)
    } else { 0 };
    let ce = if let GridLine::Named(n) = col_end {
        find_named_area(areas, n).map_or(0, |(_, _, _, ce)| ce)
    } else { 0 };
    let rs = if let GridLine::Named(n) = row_start {
        find_named_area(areas, n).map_or(0, |(rs, _, _, _)| rs)
    } else { 0 };
    let re = if let GridLine::Named(n) = row_end {
        find_named_area(areas, n).map_or(0, |(_, re, _, _)| re)
    } else { 0 };
    (cs, ce, rs, re)
}

#[cfg(test)]
mod tests;

