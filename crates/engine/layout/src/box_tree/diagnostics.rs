//! BUG-341 census/cache-statistics infrastructure for the box-build and
//! layout stages: `LayoutKeyCensus`/`LayoutResultCacheStats`/`BoxBuildStats`/
//! `BoxCopyStats` and their `take_*`/`set_*` accessors, plus the S32/S36
//! layout-result cache and the S20/S25 timing censuses that back them.
//!
//! Перенесено батчем SPLIT-BT19 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `struct LayoutKeyCensus` до `struct ViewBox`) без правок тел.

use super::*;

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
    /// BUG-341 S39 — of [`Self::repeat_key_same_style_and_override`], the ones
    /// whose `(start_x, start_y)` also matches the origin recorded the
    /// *previous* time this key was seen. This is the precondition S38's
    /// recommended redesign (ii) ("store an `Rc`/`Arc`-shared subtree a hit
    /// repositions instead of cloning out of") silently assumes: a
    /// `LayoutBox`'s rects are **absolute**, not parent-relative
    /// (`lay_out_inner` writes `b.rect.x = start_x + margin_left`), so serving
    /// a shared subtree at a *different* origin means rewriting every
    /// descendant's rect — `translate_subtree`, the same O(nodes) mutating
    /// walk the hit path already runs. Under sharing that mutation forces a
    /// copy-on-write of the whole subtree, i.e. exactly the deep clone S37
    /// priced at ≈1.7 μs/node and (ii) exists to avoid. Sharing is therefore
    /// only free for hits at an *identical* origin, and this field is how many
    /// of the otherwise-safe repeats those are — measured before the mechanism
    /// is written, per `docs/perf-method.md` §1.
    pub repeat_key_same_style_override_and_origin: u32,
    /// BUG-341 S38 — distinct keys seen exactly once this pass (no repeat at
    /// all, so a deferred-insertion cache would never clone or store them).
    pub keys_seen_once: u32,
    /// BUG-341 S38 — distinct keys seen exactly twice this pass. S33 found
    /// the CSS-Grid-specific hit case was always exactly this shape, which is
    /// why "clone only on the second sighting, serve on the third" would have
    /// zeroed that case's only hit. This field is the same question asked of
    /// the flex-item redundancy this census's fixture actually exercises,
    /// per `docs/perf-method.md`'s standing rule not to let a repeat-shape
    /// finding from one target transfer unchecked to another.
    pub keys_seen_twice: u32,
    /// BUG-341 S38 — distinct keys seen three or more times this pass. A
    /// "clone on second sighting, serve from the third onward" policy can
    /// only ever produce a hit for a key counted here, never one counted in
    /// [`Self::keys_seen_twice`].
    pub keys_seen_three_plus: u32,
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
pub(crate) struct UsedSizeOverrideBits {
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
/// most recent occurrence, that occurrence's `UsedSizeOverride` snapshot, and
/// its `(start_x, start_y)` origin in `to_bits` form (S30 only needed the
/// count, S31 added the style compare, S35 the override compare, S39 the
/// origin compare) — factored out per clippy's `type_complexity`.
type LayoutCensusSeenEntry = (u32, Arc<ComputedStyle>, UsedSizeOverrideBits, (u32, u32));

thread_local! {
    static LAYOUT_KEY_CENSUS_ON: Cell<bool> = const { Cell::new(false) };
    static LAYOUT_KEY_CENSUS: Cell<LayoutKeyCensus> = const { Cell::new(LayoutKeyCensus { calls: 0, repeat_key_calls: 0, repeat_key_same_style: 0, repeat_key_same_style_and_override: 0, repeat_key_same_style_override_and_origin: 0, keys_seen_once: 0, keys_seen_twice: 0, keys_seen_three_plus: 0 }) };
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

/// Returns the accumulated [`LayoutKeyCensus`] and resets the tally. The
/// per-key occurrence histogram (BUG-341 S38: `keys_seen_once`/`_twice`/
/// `_three_plus`) is derived from [`LAYOUT_KEY_SEEN`]'s final counts here,
/// the one point both still hold their pass-end state before either is
/// cleared.
pub fn take_layout_key_census() -> LayoutKeyCensus {
    let (once, twice, three_plus) = LAYOUT_KEY_SEEN.with(|m| {
        m.borrow().values().fold((0u32, 0u32, 0u32), |(once, twice, three_plus), (count, ..)| {
            match count {
                1 => (once + 1, twice, three_plus),
                2 => (once, twice + 1, three_plus),
                _ => (once, twice, three_plus + 1),
            }
        })
    });
    LAYOUT_KEY_SEEN.with(|m| m.borrow_mut().clear());
    let mut census = LAYOUT_KEY_CENSUS.with(|c| c.replace(LayoutKeyCensus::default()));
    census.keys_seen_once = once;
    census.keys_seen_twice = twice;
    census.keys_seen_three_plus = three_plus;
    census
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
/// values. `start_x`/`start_y` (S39) are compared by the same exact-bit rule as
/// the constraints — they are deliberately *not* part of the key (a cache
/// serves a repeat at a new position by translating it), only of the
/// per-occurrence record, so that
/// [`LayoutKeyCensus::repeat_key_same_style_override_and_origin`] can report
/// how many otherwise-safe repeats need no translation at all.
pub(crate) fn record_layout_key_occurrence(
    node: NodeId,
    start_x: f32,
    start_y: f32,
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
    let origin_bits = (start_x.to_bits(), start_y.to_bits());
    let (repeat, same_style, same_override, same_origin) = LAYOUT_KEY_SEEN.with(|m| {
        let mut seen = m.borrow_mut();
        match seen.get_mut(&key) {
            Some((count, prev_style, prev_override, prev_origin)) => {
                *count += 1;
                let same_style = Arc::ptr_eq(prev_style, style);
                let same_override = *prev_override == override_bits;
                let same_origin = *prev_origin == origin_bits;
                *prev_style = Arc::clone(style);
                *prev_override = override_bits;
                *prev_origin = origin_bits;
                (true, same_style, same_override, same_origin)
            }
            None => {
                seen.insert(key, (1, Arc::clone(style), override_bits, origin_bits));
                (false, false, false, false)
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
                    if same_origin {
                        v.repeat_key_same_style_override_and_origin += 1;
                    }
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
    pub(crate) static CV_AUTO_TOUCHED: Cell<bool> = const { Cell::new(false) };
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
    pub(crate) static INDEFINITE_HEIGHT_CONSULTED: Cell<bool> = const { Cell::new(false) };
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
pub(crate) type FlexProbeKey = (NodeId, u32);

/// One [`FLEX_COLUMN_PROBE_HEIGHTS`] entry: the style the probe ran with (an
/// `Arc` identity check, same convention as the S31 census and the S36 cache)
/// and the border-box height it produced.
pub(crate) type FlexProbeEntry = (Arc<ComputedStyle>, f32);

thread_local! {
    pub(crate) static FLEX_COLUMN_PROBE_HEIGHTS: RefCell<HashMap<FlexProbeKey, FlexProbeEntry>> =
        RefCell::new(HashMap::new());
    /// Recursion depth of [`lay_out_cache_checked`], so the memo above can be
    /// emptied at the start and end of every layout pass without every entry
    /// point having to remember to do it. A stale entry from an earlier pass
    /// would be served against a box whose *contents* changed while its style
    /// `Arc` stayed the same — the one thing the `ptr_eq` check cannot catch.
    pub(crate) static LAYOUT_PASS_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// RAII marker for one layout pass — see [`LAYOUT_PASS_DEPTH`]. Clears the
/// probe-height memo when the outermost call is entered and again when it
/// returns, so nothing survives into the next pass (or past a panic).
pub(crate) struct LayoutPassGuard;

impl LayoutPassGuard {
    pub(crate) fn enter() -> Self {
        let depth = LAYOUT_PASS_DEPTH.with(|c| {
            let d = c.get() + 1;
            c.set(d);
            d
        });
        if depth == 1 {
            FLEX_COLUMN_PROBE_HEIGHTS.with(|m| m.borrow_mut().clear());
            clear_layout_in_place_memo();
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
            clear_layout_in_place_memo();
        }
    }
}

/// Resolves a block-axis length against the containing block's content height,
/// recording in [`INDEFINITE_HEIGHT_CONSULTED`] when an indefinite basis is
/// what made the resolution fail. Every `lay_out_inner` read of its
/// `available_height` parameter goes through here — see that flag's comment.
pub(crate) fn resolve_block_size(
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
pub(crate) struct LayoutResultKey {
    pub(crate) node: NodeId,
    pub(crate) width_bits: u32,
    pub(crate) height_bits: Option<u32>,
    pub(crate) viewport_w_bits: u32,
    pub(crate) viewport_h_bits: u32,
    pub(crate) pcb_x_bits: u32,
    pub(crate) pcb_y_bits: u32,
    pub(crate) pcb_w_bits: u32,
    pub(crate) pcb_h_bits: u32,
    pub(crate) in_block_flow: bool,
    pub(crate) measurer_ptr: usize,
    pub(crate) hp_ptr: usize,
    pub(crate) used_size_override: UsedSizeOverrideBits,
}

/// One cached [`lay_out`]/[`lay_out_with_used_size`] result — same shape as
/// S32's own entry (style `Arc` for the `ptr_eq` correctness check, the
/// origin the subtree's rects are expressed in so a hit can
/// [`crate::incremental::translate_subtree`] to a different origin, and the
/// laid-out subtree itself).
pub(crate) struct LayoutResultEntry {
    pub(crate) style: Arc<ComputedStyle>,
    pub(crate) start_x: f32,
    pub(crate) start_y: f32,
    pub(crate) result: LayoutBox,
}

/// BUG-341 S38 — which insertion policy [`lay_out_cache_checked`] applies on
/// a cache miss. S37 measured the per-call fixed cost as ~99% one thing, the
/// full subtree clone paid on every miss (`b.clone()`, ~1.7μs/node) —
/// negligible next to key construction (5.1ns) or the `HashMap` lookup
/// (266.3ns) — and found ~91% of S32-era insertions were never read back,
/// i.e. that clone was pure waste on most misses. `Lazy` defers the clone to
/// a key's *confirmed* second sighting (same node/constraints/style as the
/// first) instead of paying it on every miss unconditionally like `Eager`
/// (S36's mechanism, kept for comparison) — but per
/// `docs/perf-method.md`'s standing rule, this policy is only correct if the
/// real repeat shape supports it: S33 rejected exactly this idea for the
/// CSS-Grid probe/final case because that case's repeats are always exactly
/// two occurrences (deferring to the second sighting would have zeroed its
/// only hit). BUG-341 S38's own census of this cache's actual flex-item
/// target found the opposite shape — of 271 distinct keys in one real
/// chrome-document pass, only 4.4% repeat exactly twice while 86.3% repeat
/// three or more times — so deferring the clone to the second sighting still
/// serves the third-and-later occurrences that dominate this fixture's
/// redundancy, while skipping the clone entirely on the 9.2% of keys that
/// are never seen again at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LayoutResultCacheMode {
    /// No cache — plain `lay_out_inner` on every call.
    #[default]
    Off,
    /// S36 — clone and store on every miss, unconditionally.
    Eager,
    /// S38 — clone and store only once a key's second sighting confirms the
    /// repeat is real; see this type's own doc comment.
    Lazy,
}

thread_local! {
    pub(crate) static LAYOUT_RESULT_CACHE_MODE: Cell<LayoutResultCacheMode> = const { Cell::new(LayoutResultCacheMode::Off) };
    pub(crate) static LAYOUT_RESULT_CACHE: RefCell<HashMap<LayoutResultKey, LayoutResultEntry>> = RefCell::new(HashMap::new());
    /// BUG-341 S38 — `Lazy` mode's marker for "this key was seen exactly
    /// once so far this pass": the style `Arc` from that first sighting
    /// (a refcount bump, not a subtree clone) so the second sighting can
    /// check the same `ptr_eq` correctness precondition `Eager` checks on
    /// every hit, before paying for the clone. Never holds a key that is
    /// also present in [`LAYOUT_RESULT_CACHE`] — a key graduates out of this
    /// map into that one the moment its second sighting confirms the style
    /// is unchanged.
    pub(crate) static LAYOUT_RESULT_CACHE_SEEN: RefCell<HashMap<LayoutResultKey, Arc<ComputedStyle>>> = RefCell::new(HashMap::new());
}

/// Enables/disables the BUG-341 S36 layout-result cache in `Eager` mode
/// (clone-and-store on every miss) and clears its state — call once before a
/// full (non-incremental) layout pass. See [`set_layout_result_cache_lazy`]
/// for the S38 alternative policy.
pub fn set_layout_result_cache(on: bool) {
    set_layout_result_cache_mode(if on { LayoutResultCacheMode::Eager } else { LayoutResultCacheMode::Off });
}

/// Enables/disables the BUG-341 S38 layout-result cache in `Lazy` mode
/// (clone-and-store only from a key's confirmed second sighting) and clears
/// its state — call once before a full (non-incremental) layout pass. See
/// [`LayoutResultCacheMode`]'s doc comment for why this policy exists
/// alongside [`set_layout_result_cache`]'s `Eager` one rather than replacing
/// it.
pub fn set_layout_result_cache_lazy(on: bool) {
    set_layout_result_cache_mode(if on { LayoutResultCacheMode::Lazy } else { LayoutResultCacheMode::Off });
}

fn set_layout_result_cache_mode(mode: LayoutResultCacheMode) {
    LAYOUT_RESULT_CACHE_MODE.with(|c| c.set(mode));
    LAYOUT_RESULT_CACHE.with(|m| m.borrow_mut().clear());
    LAYOUT_RESULT_CACHE_SEEN.with(|m| m.borrow_mut().clear());
    CV_AUTO_TOUCHED.with(|c| c.set(false));
    LAYOUT_RESULT_CACHE_STATS.with(|c| c.set(LayoutResultCacheStats::default()));
}

/// Whether the BUG-341 S36/S38 layout-result cache is currently enabled, in
/// either policy.
pub fn layout_result_cache_enabled() -> bool {
    LAYOUT_RESULT_CACHE_MODE.with(|c| c.get() != LayoutResultCacheMode::Off)
}

/// The BUG-341 S38 policy currently in effect — [`lay_out_cache_checked`]'s
/// miss branch reads this to decide whether to clone-and-store
/// unconditionally (`Eager`) or defer to a confirmed second sighting
/// (`Lazy`). Never called while [`layout_result_cache_enabled`] is `false`.
pub(crate) fn layout_result_cache_mode() -> LayoutResultCacheMode {
    LAYOUT_RESULT_CACHE_MODE.with(|c| c.get())
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
    /// BUG-341 S38 — `Lazy` mode only: misses that recorded a cheap
    /// first-sighting marker ([`LAYOUT_RESULT_CACHE_SEEN`], no subtree
    /// clone) instead of materializing into the real cache. Always `0` in
    /// `Eager` mode. `misses - deferred` is the count of misses that *did*
    /// pay the clone (confirmed second sighting or later, or `Eager`'s
    /// unconditional store).
    pub deferred: u32,
}

thread_local! {
    pub(crate) static LAYOUT_RESULT_CACHE_STATS: Cell<LayoutResultCacheStats> = const { Cell::new(LayoutResultCacheStats { hits: 0, misses: 0, poisoned: 0, deferred: 0 }) };
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
pub(crate) fn cacheable_for_layout_result_cache(b: &LayoutBox) -> bool {
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
    pub(crate) static BOX_BUILD_STATS: Cell<BoxBuildStats> = const {
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
pub(crate) static BOX_BUILD_TIME_LOG: std::sync::Mutex<Vec<(NodeId, u64)>> = std::sync::Mutex::new(Vec::new());

/// Gate for [`BOX_BUILD_TIME_LOG`] — deliberately **not** the S18/S19
/// [`BOX_BUILD_LOG_ON`] flag.
///
/// That flag also arms the copy census, whose `count_boxes` walks every reused
/// subtree (299 of chrome's 318 boxes on a keystroke) from inside
/// `build_box_or_reuse` — i.e. from inside the *parent's* `build_box` call. Run
/// together, the copy census would land squarely in the timing census's numbers
/// and make whichever box happens to own the largest reused subtree look like
/// the most expensive box to build. One census must not be measuring the other.
pub(crate) static BOX_TIME_LOG_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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

pub(crate) static BOX_CLONE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static BOX_CLONE_BOXES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
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
pub(crate) fn count_boxes(b: &LayoutBox) -> u64 {
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
pub(crate) fn note_display_probe<T>(f: impl FnOnce() -> T) -> T {
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
pub(crate) fn note_style_miss<T>(f: impl FnOnce() -> T) -> T {
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
pub(crate) fn note_box_built(id: NodeId) {
    if BOX_BUILD_LOG_ON.load(std::sync::atomic::Ordering::Relaxed)
        && let Ok(mut log) = BOX_BUILD_LOG.lock()
    {
        log.push(id);
    }
}

/// BUG-341 S40 — the inputs a `lay_out_inner` call is fully determined by,
/// *including* its origin. This is deliberately a stricter key than
/// [`LayoutResultKey`]: that one omits `(start_x, start_y)` because a cache
/// serves a repeat at a new position by translating the stored subtree into
/// place, and S32/S36/S38 all measured that translation-plus-clone as costing
/// more than it saves. S39's audit found why the clone is unavoidable there —
/// a `LayoutBox` stores absolute rects, so a result is only valid at the
/// origin it was computed for — and S39's census found that 75.2% of the
/// otherwise-safe repeats need no translation at all, because they arrive at
/// the *identical* origin.
///
/// That is the case this key isolates, and for it no storage mechanism is
/// needed at all: the box being asked to lay out again is the same box that
/// already holds the answer, still sitting in the tree where the earlier call
/// left it. The reuse is therefore a *skip*, not a copy — no subtree clone on
/// insert (S32's 91%-never-read-back waste), none on serve (S37's
/// ≈1.7 μs/node), and no deferred first hit (S38's recursion-short-circuit
/// cost). What is stored per entry is a fixed-size witness, never a subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LayoutInPlaceKey {
    pub(crate) node: NodeId,
    /// One DOM node can own several boxes (an element's principal box plus the
    /// anonymous wrappers around its inline children — ADR-025 §1), so `node`
    /// alone does not identify a box. Compared by variant only: `BoxRole`'s
    /// payload (`PseudoKind`) is already reflected in the box's `style` `Arc`,
    /// which the witness checks by pointer identity.
    pub(crate) role: std::mem::Discriminant<BoxRole>,
    pub(crate) start_x_bits: u32,
    pub(crate) start_y_bits: u32,
    pub(crate) width_bits: u32,
    pub(crate) height_bits: Option<u32>,
    pub(crate) viewport_w_bits: u32,
    pub(crate) viewport_h_bits: u32,
    pub(crate) pcb_x_bits: u32,
    pub(crate) pcb_y_bits: u32,
    pub(crate) pcb_w_bits: u32,
    pub(crate) pcb_h_bits: u32,
    pub(crate) in_block_flow: bool,
    pub(crate) measurer_ptr: usize,
    pub(crate) hp_ptr: usize,
    pub(crate) used_size_override: UsedSizeOverrideBits,
}

/// Proof that the box a [`LayoutInPlaceKey`] was recorded for is still the box
/// in front of us, and still holds the result that call produced. Fixed size —
/// this mechanism never stores a subtree.
///
/// All three checks are load-bearing, and none subsumes the others:
///
/// * `box_ptr` — the same allocation. Layout runs over `&mut LayoutBox`es
///   living in their parents' `children` vectors; `lay_out_grid`'s Step-5
///   probe reuse assigns a whole new box into such a slot (`children[i] =
///   reused`), so an address can outlive the box that was recorded at it.
/// * `style` — `Arc` pointer identity, the same check S31 established for the
///   cache. `Arc::make_mut` writers (`apply_font_size_adjust_to_style`,
///   `apply_container_rules`, `lay_out_table_row`'s width dance) diverge a
///   box's style in place; a diverged style means the recorded result is not
///   what this call would produce.
/// * `out_rect` — the geometry the recorded call left behind. S39's audit
///   found eight production sites that reposition an already-laid-out subtree
///   (`shift_tree`/`translate_subtree`, two of them inside `lay_out_flex`
///   itself) plus several that overwrite an item's `rect` directly for
///   alignment or row-height normalisation. Every one of those changes
///   `b.rect`, so comparing it against the recorded output is what makes those
///   mutations invalidate the entry instead of silently outdating it.
struct LayoutInPlaceWitness {
    style: Arc<ComputedStyle>,
    box_ptr: usize,
    out_rect: [u32; 4],
}

/// Per-pass tally of [`LayoutInPlaceKey`] outcomes — the counter the S40 gates
/// assert on, since reuse is invisible in output by construction (a pass that
/// reuses nothing produces exactly the same tree, only slower — the S8 lesson).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutInPlaceStats {
    /// Calls served by skipping `lay_out_inner` entirely — and with it, the
    /// whole recursive descent into the box's children.
    pub hits: u32,
    /// Calls that had to compute, and were recorded for a later repeat.
    pub recorded: u32,
    /// Calls that computed but were refused a recording because the subtree
    /// consulted `content-visibility: auto` (see `CV_AUTO_TOUCHED`).
    pub refused: u32,
}

thread_local! {
    static LAYOUT_IN_PLACE_ON: Cell<bool> = const { Cell::new(false) };
    static LAYOUT_IN_PLACE: RefCell<HashMap<LayoutInPlaceKey, LayoutInPlaceWitness>> =
        RefCell::new(HashMap::new());
    static LAYOUT_IN_PLACE_STATS: Cell<LayoutInPlaceStats> =
        const { Cell::new(LayoutInPlaceStats { hits: 0, recorded: 0, refused: 0 }) };
}

/// Enables/disables BUG-341 S40 in-place layout reuse on the current thread.
///
/// **Off by default, and measured that way deliberately.** The mechanism is
/// correct (six differential tests) and, unlike S32/S36/S38's cache, its hits
/// are free — but S40's A/B on the real chrome document found only a 3.1% hit
/// rate, because the repeats S39's census counted as same-origin are mostly
/// *different box instances* of the same node rather than the same box asked
/// twice. 531 recordings per pass buy 17 hits, and two interleaved runs
/// disagree on the sign of the wall-clock difference — i.e. neutral. It stays
/// wired and tested rather than deleted because it is the one reuse shape
/// whose hit costs nothing, so a future change that raises the hit rate (e.g.
/// removing the box-instance churn) can re-measure it by flipping this flag
/// instead of rebuilding the mechanism.
pub fn set_layout_in_place_reuse(on: bool) {
    LAYOUT_IN_PLACE_ON.with(|c| c.set(on));
    LAYOUT_IN_PLACE.with(|m| m.borrow_mut().clear());
}

/// Whether BUG-341 S40 in-place layout reuse is active on this thread.
pub fn layout_in_place_reuse_enabled() -> bool {
    LAYOUT_IN_PLACE_ON.with(|c| c.get())
}

/// Returns the accumulated [`LayoutInPlaceStats`] and resets the tally.
pub fn take_layout_in_place_stats() -> LayoutInPlaceStats {
    LAYOUT_IN_PLACE_STATS.with(|c| c.replace(LayoutInPlaceStats::default()))
}

/// Clears the per-pass reuse memo. Called from [`LayoutPassGuard`] at both
/// ends of an outermost pass, exactly like `FLEX_COLUMN_PROBE_HEIGHTS` — an
/// entry is only ever valid inside the pass that produced it, which is what
/// keeps a later pass (a changed cascade, a new frame, one of `entry.rs`'s
/// four post-layout passes) from ever seeing a stale one.
pub(crate) fn clear_layout_in_place_memo() {
    LAYOUT_IN_PLACE.with(|m| m.borrow_mut().clear());
}

/// `to_bits` snapshot of a rect, for the exact-equality witness compare.
fn rect_bits(r: &Rect) -> [u32; 4] {
    [r.x.to_bits(), r.y.to_bits(), r.width.to_bits(), r.height.to_bits()]
}

/// True when `b` already holds the result `key` describes — see
/// [`LayoutInPlaceWitness`] for why all three checks are needed. On `true` the
/// caller must return without calling `lay_out_inner`: the box, and the whole
/// subtree below it, already is the answer.
pub(crate) fn layout_in_place_hit(b: &LayoutBox, key: &LayoutInPlaceKey) -> bool {
    let hit = LAYOUT_IN_PLACE.with(|m| {
        m.borrow().get(key).is_some_and(|w| {
            w.box_ptr == b as *const LayoutBox as usize
                && Arc::ptr_eq(&w.style, &b.style)
                && w.out_rect == rect_bits(&b.rect)
        })
    });
    if hit {
        LAYOUT_IN_PLACE_STATS.with(|c| {
            let mut v = c.get();
            v.hits += 1;
            c.set(v);
        });
    }
    hit
}

/// Records `b`'s just-computed state against `key`. `cv_touched` refuses the
/// recording for a subtree that consulted `content-visibility: auto`: that
/// decision depends on the scroll offset and a cross-frame ratchet, neither of
/// which is in the key or in `style`, so the same inputs can legitimately
/// produce a different result — the same exclusion S32 established for the
/// cache, for the same reason.
pub(crate) fn record_layout_in_place(b: &LayoutBox, key: LayoutInPlaceKey, cv_touched: bool) {
    LAYOUT_IN_PLACE_STATS.with(|c| {
        let mut v = c.get();
        if cv_touched {
            v.refused += 1;
        } else {
            v.recorded += 1;
        }
        c.set(v);
    });
    if cv_touched {
        return;
    }
    LAYOUT_IN_PLACE.with(|m| {
        m.borrow_mut().insert(
            key,
            LayoutInPlaceWitness {
                style: Arc::clone(&b.style),
                box_ptr: b as *const LayoutBox as usize,
                out_rect: rect_bits(&b.rect),
            },
        );
    });
}

/// BUG-341 S41 census: what is left of `lay_out_flex`'s column-direction
/// double layout after BUG-802's probe replay, split by the reason the replay
/// was refused.
///
/// The bug's title names the double layout, and every slice from S32 on
/// attacked it by *reusing a result* — five mechanisms, all measured neutral or
/// negative (S40 closed that branch with a 3.1% ceiling). The remaining path
/// S40's "Not attempted" names is the opposite one: stop making the second
/// call. This census measures how much of that there is to stop, and which of
/// the three refusal reasons carries it — before any mechanism is built, per
/// `docs/perf-method.md` §1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlexColumnCensus {
    /// Column-direction flex items that needed a Step-1 base size at all.
    pub needed: u32,
    /// Of those, served from [`FLEX_COLUMN_PROBE_HEIGHTS`] — no layout ran.
    pub memo_served: u32,
    /// Of those, a real Step-1 `lay_out` call.
    pub probed: u32,
    /// Items whose final placement pass replayed the probe (`shift_tree`)
    /// instead of laying out again — BUG-802's win, one layout for the item.
    pub replayed: u32,
    /// **The residual double layout.** Items that ran a real Step-1 probe *and*
    /// a full final placement layout: two recursive layouts of one subtree.
    pub double: u32,
    /// Of [`Self::double`], refused because the probe itself was not replayable
    /// (`content-visibility: auto` or an indefinite-height consultation).
    pub double_dirty: u32,
    /// Of [`Self::double`], refused because flex grow/shrink resolved a main
    /// size differing from the probed one, cross size unchanged — the bucket a
    /// height-independent subtree could serve without laying out again.
    pub double_size: u32,
    /// Of [`Self::double`], refused because the item's used cross size is not
    /// the container's full content width (`align-self`, an auto cross margin).
    pub double_cross: u32,
    /// Of [`Self::double_size`], the ones where the resolved main size is
    /// *larger* than the probed content height (the item grew).
    pub double_size_grew: u32,
}

thread_local! {
    static FLEX_COLUMN_CENSUS_ON: Cell<bool> = const { Cell::new(false) };
    static FLEX_COLUMN_CENSUS: Cell<FlexColumnCensus> = const { Cell::new(FlexColumnCensus {
        needed: 0, memo_served: 0, probed: 0, replayed: 0, double: 0,
        double_dirty: 0, double_size: 0, double_cross: 0, double_size_grew: 0,
    }) };
}

/// Enables/disables the BUG-341 S41 column-probe census on the current thread.
pub fn set_flex_column_census(on: bool) {
    FLEX_COLUMN_CENSUS_ON.with(|c| c.set(on));
}

/// Whether the BUG-341 S41 census is recording on this thread. `lay_out_flex`
/// checks this before allocating the per-item bookkeeping only the census
/// reads, so an off census costs one `Cell` read per container.
pub(crate) fn flex_column_census_on() -> bool {
    FLEX_COLUMN_CENSUS_ON.with(|c| c.get())
}

/// Returns the accumulated [`FlexColumnCensus`] and resets the tally.
pub fn take_flex_column_census() -> FlexColumnCensus {
    FLEX_COLUMN_CENSUS.with(|c| c.replace(FlexColumnCensus::default()))
}

/// Applies `f` to the current thread's [`FlexColumnCensus`] tally when the
/// census is on. A no-op otherwise — the call sites in `lay_out_flex` sit on
/// the hottest path in the engine and must cost one `Cell` read when off.
pub(crate) fn note_flex_column(f: impl FnOnce(&mut FlexColumnCensus)) {
    if !flex_column_census_on() {
        return;
    }
    FLEX_COLUMN_CENSUS.with(|c| {
        let mut v = c.get();
        f(&mut v);
        c.set(v);
    });
}

/// Folds `d` into the current thread's [`BoxBuildStats`] tally.
pub(crate) fn add_box_build_stats(d: BoxBuildStats) {
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
