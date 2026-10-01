//! CSSOM-4/BUG-493 — synchronous style+layout flush on a same-tick accessor
//! read (`getComputedStyle`, `offsetWidth`/`clientWidth`, `getBoundingClientRect`).
//!
//! Lumen otherwise answers these reads from `computed_styles`/`layout_rects`,
//! snapshots the embedder pushes only after a whole script/turn finishes
//! (`InProcessSession::relayout`, the shell's `apply_relayout_result`) — a
//! script that mutates the DOM/style and reads a computed value back in the
//! SAME synchronous turn saw whatever the snapshot held before the mutation,
//! or nothing at all for a freshly created node. See `bugs/BUG-493-OPEN.md`
//! for the full symptom catalogue this closes.
//!
//! This slice originally covered [`super::V8JsRuntime::update_stylesheet`]
//! callers in `InProcessSession` (headless/WPT/driver path) only — the
//! interactive shell never called `update_stylesheet`, so
//! [`FlushHandles::stylesheet`] stayed `None` there and
//! [`FlushHandles::maybe_flush`] was a permanent no-op (see
//! `bugs/BUG-977-OPEN.md`, closed by CSSOM-7). The shell now pushes its live
//! cascade `Arc<lumen_css_parser::Stylesheet>` alongside the rest of this
//! same snapshot, from every site that already pushes `computed_styles`/
//! `layout_rects`: the two parse-time pushes in `crates/shell/src/page_pipeline.rs`
//! (before the page's own scripts run, and again right after if they touched
//! `<style>`/`<link>`/the DOM — the two windows a fully synchronous
//! parse-time `<script>` can read style/scroll in, before any relayout ever
//! happens) plus the steady-state pushes in `crates/shell/src/relayout.rs`'s
//! `apply_relayout_result` and `crates/shell/src/page_load.rs`'s two
//! post-load producers. Each is a plain `Arc::clone`/`Stylesheet::clone` +
//! `Mutex` write, same shape as [`super::V8JsRuntime::update_viewport_size`],
//! so none of them ever touch the engine thread: routing a flush through the
//! engine thread from inside a native would risk a deadlock when the engine
//! thread is itself mid-rAF-turn waiting on the JS thread (ADR-016), and
//! `maybe_flush` below still deliberately never does that.
//!
//! Known remaining approximation, shared with the headless path: this flush
//! recomputes layout without touching the `:hover`/`:focus`/`:active`,
//! forced-colors or dark-mode thread-locals (`crates/engine/layout/src/style/env.rs`)
//! that a real relayout sets on the engine thread right before laying out —
//! on the JS thread those stay at their default (no interactive state, no
//! forced colors, light mode), so a same-tick flush can disagree with the
//! next full relayout for a script that reads computed style/geometry while
//! also depending on one of those states. Not attempted here — see
//! `bugs/BUG-977-OPEN.md`'s residual for the follow-up.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::named_access::lock_document_bounded;
use super::runtime::{CustomPropertySnapshot, PseudoComputedStyles};

/// [`FlushHandles::try_incremental_flush`]'s success payload: the fresh
/// layout tree + counters from the incremental cascade+layout path, the
/// `dirty_roots` set it restyled (BUG-1211 post-collectors then scope their
/// walk to just those subtrees instead of the whole document), and two
/// eviction sets computed against the tree *before* this restyle — by
/// `NodeId::index()` for the `.index()`-keyed caches (`layout_rects`,
/// `client_rects`, `computed_styles`) and by `NodeId::raw()` for the
/// `.raw()`-keyed `scroll_states` — so a node the mutation removed from the
/// DOM doesn't linger in any of the four caches with stale data forever.
type IncrFlushResult = (
    lumen_layout::LayoutBox,
    lumen_layout::CounterMap,
    std::collections::HashSet<lumen_dom::NodeId>,
    std::collections::HashSet<u32>,
    std::collections::HashSet<u32>,
);

/// Bundled embedder-pushed state a same-tick accessor native needs to force
/// a synchronous flush — one `Clone` (every field is an `Arc`) instead of
/// threading eight separate parameters through each `install_*` call site.
#[derive(Clone)]
pub(crate) struct FlushHandles {
    pub(crate) doc: Arc<Mutex<lumen_dom::Document>>,
    pub(crate) layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// BUG-1007: per-fragment client rects, refreshed alongside `layout_rects`
    /// so a same-tick `getClientRects()` after a DOM/style mutation never
    /// disagrees with `getBoundingClientRect()` over the same flush.
    pub(crate) client_rects: Arc<Mutex<HashMap<u32, Vec<[f32; 4]>>>>,
    pub(crate) computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    /// CSSOM-6 (BUG-490): sibling of `computed_styles` for pseudo-elements,
    /// keyed by `(node, pseudo name)` — see `V8JsRuntime::pseudo_computed_styles`.
    pub(crate) pseudo_computed_styles: Arc<Mutex<PseudoComputedStyles>>,
    pub(crate) custom_properties: Arc<Mutex<CustomPropertySnapshot>>,
    pub(crate) viewport_size: Arc<Mutex<[f32; 2]>>,
    pub(crate) stylesheet: Arc<Mutex<Option<Arc<lumen_css_parser::Stylesheet>>>>,
    /// BUG-935 S34: sibling of `V8JsRuntime::dom_dirty` (see its doc comment),
    /// set at every same DOM-mutating call site but gated/cleared only here
    /// by [`Self::maybe_flush`] — the scheduler's own `dom_dirty` is a
    /// separate `Arc<AtomicBool>` this struct no longer holds, so a same-tick
    /// flush can no longer consume the scheduler's "DOM mutated" signal.
    pub(crate) flush_stale: Arc<AtomicBool>,
    pub(crate) never_flushed: Arc<AtomicBool>,
    /// BUG-504 part 10: `scrollLeft`/`scrollTop`/`scrollWidth`/`scrollHeight`
    /// JS-visible cache, keyed like `layout_rects`. Reapplied onto the fresh
    /// flush tree from its own pre-flush contents (see `maybe_flush`) rather
    /// than left at the freshly-built tree's `scroll_x`/`scroll_y == 0.0`
    /// default, so a same-tick read after a scroll-affecting style mutation
    /// (e.g. `overflow` flipping to `clip`) sees the correctly reclamped
    /// value instead of silently losing a prior scroll position.
    pub(crate) scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// BUG-560: the engine thread's same-tick focus mirror (see
    /// [`super::runtime::V8JsRuntime::focused_nid`]) — read-only here, used to
    /// install the correct `:focus`/`:focus-within` state before laying out.
    pub(crate) focused_nid: Arc<Mutex<Option<u32>>>,
    /// BUG-560: the focus target baked into the current `computed_styles`/
    /// `layout_rects` snapshot, so [`Self::maybe_flush`] can tell a same-tick
    /// `.focus()` apart from "nothing changed" even when it left `dom_dirty`
    /// untouched. Updated at the end of every successful flush.
    pub(crate) last_flushed_focus: Arc<Mutex<Option<u32>>>,
    /// CSSOM-8 вариант C / BUG-493: the per-`<style>`/`<link>` registry, the
    /// CSSOM edit log and its "grew since the last flush" flag, bundled with
    /// what reconciles the registry with the DOM — see
    /// [`super::sheet_sync::SheetSync`]. A CSSOM write touches neither the DOM
    /// nor the focus, so without `sheet_sync.dirty` the gate below would serve
    /// the pre-mutation snapshot to a same-tick `getComputedStyle()`.
    pub(crate) sheet_sync: super::sheet_sync::SheetSync,
    /// BUG-935 S43: mirrors [`super::runtime::V8JsRuntime::pseudo_styles_needed`] —
    /// `true` once the page has read `getComputedStyle(el, pseudoElt)`/
    /// `computedStyleMap()`'s pseudo-element path. Gates the matching
    /// collector in [`Self::maybe_flush`] the same way the async
    /// `collect_js_data` (`crates/shell/src/relayout.rs`) gates its own copy
    /// of the same collector — S42's consumer audit found no reader of
    /// `pseudo_computed_styles` outside the `maybe_flush`-gated natives, so
    /// this cannot serve a stale answer.
    pub(crate) pseudo_styles_needed: Arc<AtomicBool>,
    /// BUG-935 S45: backs [`Self::maybe_flush`]'s `pseudo_styles_pending`
    /// bypass, mirroring [`Self::computed_styles_collected`] (S44) for this
    /// cache — closes the same latent same-tick-ordering race S44 found and
    /// fixed for `computed_styles`, flagged there as dormant for this field.
    pub(crate) pseudo_styles_collected: Arc<AtomicBool>,
    /// BUG-935 S43: sibling of [`Self::pseudo_styles_needed`] for
    /// [`Self::custom_properties`].
    pub(crate) custom_props_needed: Arc<AtomicBool>,
    /// BUG-935 S45: sibling of [`Self::pseudo_styles_collected`] for
    /// [`Self::custom_props_needed`].
    pub(crate) custom_props_collected: Arc<AtomicBool>,
    /// BUG-935 S44: sibling of [`Self::pseudo_styles_needed`] for
    /// [`Self::computed_styles`] itself — S37's measured dominant cost.
    /// Unlike the other two, this cache also has a non-`getComputedStyle`-family
    /// reader (`_lumen_request_scroll`'s overflow-clip check, BUG-975), which
    /// sets this same flag, so gating the collector on it here cannot serve a
    /// stale answer to that reader either.
    pub(crate) computed_styles_needed: Arc<AtomicBool>,
    /// BUG-935 S44: backs [`Self::maybe_flush`]'s `computed_styles_pending`
    /// bypass — `true` once a collect has actually run while
    /// [`Self::computed_styles_needed`] was set, mirroring
    /// [`Self::never_flushed`] but scoped to this one cache.
    pub(crate) computed_styles_collected: Arc<AtomicBool>,
    /// GAP-HLHITTEST: `text node index → laid-out fragments` with their UTF-16
    /// offset spans ([`lumen_layout::collect_text_frag_rects`]), the geometry
    /// `CSS.highlights.highlightsFromPoint()` hit-tests against. Collected
    /// only while [`Self::text_frags_needed`] is set — most pages never call
    /// that API, and the walk touches every inline fragment.
    pub(crate) text_frag_rects: Arc<Mutex<HashMap<u32, Vec<lumen_layout::TextFragRect>>>>,
    /// GAP-HLHITTEST: set by `_lumen_text_at_point` before it flushes.
    pub(crate) text_frags_needed: Arc<AtomicBool>,
    /// GAP-HLHITTEST: same bypass as [`Self::computed_styles_collected`];
    /// additionally cleared by `update_client_rects`, because the table is
    /// never pushed by the embedder alongside its fresh geometry.
    pub(crate) text_frags_collected: Arc<AtomicBool>,
    /// BUG-1211: page-side DOM-mutation tracker (BUG-341 S7), read-only here
    /// — [`Self::maybe_flush`] only *peeks* at [`super::runtime::DomTouched`],
    /// it never drains it. Draining is [`super::runtime::V8JsRuntime::
    /// take_dom_touched`]'s job (the shell's rAF/relayout pipeline), and a
    /// same-tick flush that also drained it would make the page pipeline's
    /// own next `try_relayout_raf_incremental` blind to mutations this flush
    /// already saw, forcing an unnecessary full cascade there instead.
    pub(crate) dom_touched: Arc<Mutex<super::runtime::DomTouched>>,
    /// BUG-1211: incremental same-tick flush basis — the previous flush's
    /// laid-out tree, cascade cache and the inputs it was built against
    /// (viewport, sheet revision, focus). `None` until the first successful
    /// flush (mirrors [`Self::never_flushed`]) or whenever [`Self::
    /// maybe_flush`] falls back to a full recompute for a reason the
    /// incremental path cannot handle (sheet/viewport change, `unattributed`
    /// mutation, `:hover`/`:active` change — this flush never tracks those,
    /// see the module doc comment's \"Known remaining approximation\").
    pub(crate) incr_basis: Arc<Mutex<Option<IncrFlushBasis>>>,
}

/// See [`FlushHandles::incr_basis`].
pub(crate) struct IncrFlushBasis {
    pub(crate) layout: lumen_layout::LayoutBox,
    pub(crate) cascade: lumen_layout::CascadeStyles,
    pub(crate) viewport: [f32; 2],
    pub(crate) sheet_revision: lumen_css_parser::StylesheetRevision,
    /// Focus baked into `layout`/`cascade` — mirrors [`FlushHandles::
    /// last_flushed_focus`] at the moment this basis was produced, kept
    /// alongside it so a focus-only transition since then can still be
    /// expressed as a `RestyleDelta` instead of forcing a full recompute.
    pub(crate) focus: Option<u32>,
    /// BUG-1211: [`super::runtime::DomTouched::epoch`] at the moment this
    /// basis was produced — the watermark [`FlushHandles::
    /// try_incremental_flush`] diffs `touch_gen` against, since `touched.
    /// nodes` itself is never drained (see [`FlushHandles::dom_touched`]'s
    /// doc comment) and would otherwise re-widen `dirty_roots` to every
    /// node touched since the page loaded on every single flush.
    pub(crate) touch_epoch: u64,
}

/// Recorded CSSOM writes awaiting replay onto the cascade sheet, each paired
/// with the `<style>`/`<link>` node whose own sheet it was applied to
/// (CSSOM-8 вариант C).
///
/// A flat log rather than a map keyed by node because replay order matters
/// within a node and the list is short (one entry per CSSOM write the page has
/// ever made); grouping happens at replay time.
pub(crate) type CssomDeltaLog = Arc<Mutex<Vec<(u32, lumen_css_parser::CssomOp)>>>;

/// Bundled font for the flush's own measurer — the same file every other
/// bundled-Inter call site in this crate uses (`crates/js/src/canvas2d.rs`),
/// duplicated rather than shared because there is no common asset crate to
/// put it in yet. Ignoring the page's `@font-face`/web fonts here is a known
/// Phase-0 approximation mirroring `InProcessSession::layout_and_commit`,
/// which makes the exact same trade-off for the headless path this flush
/// serves — a page whose layout genuinely depends on a custom font metric
/// may see a slightly different value from a same-tick flush than from the
/// next full relayout.
const FLUSH_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

/// Test-only switch (BUG-935 S55): while set, [`FlushHandles::try_incremental_flush`]
/// ignores the document's content journal and reports `ContentDirty::Untracked`,
/// i.e. the behaviour before the journal. Lets a test run one script both ways and
/// demand the same geometry. Process-global, so only a test that runs its two
/// halves back to back (and tolerates a concurrent flush taking the old path,
/// which is correct too) may flip it.
#[cfg(test)]
pub(crate) static CONTENT_JOURNAL_DISABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[inline]
fn content_journal_disabled() -> bool {
    #[cfg(test)]
    {
        CONTENT_JOURNAL_DISABLED.load(Ordering::Relaxed)
    }
    #[cfg(not(test))]
    {
        false
    }
}

impl FlushHandles {
    /// Recompute style+layout and refresh `layout_rects`/`computed_styles`/
    /// `custom_properties` in place if anything might be stale.
    ///
    /// No-op (serves whatever is already in the maps) when: nothing changed
    /// since the last successful flush, no stylesheet has been pushed yet
    /// (worker/test contexts, or the shell's not-yet-covered path), the
    /// viewport is still unknown, or the document is locked elsewhere past
    /// [`lock_document_bounded`]'s wait budget — every one of these degrades
    /// to the pre-CSSOM-4 stale-snapshot behaviour rather than blocking or
    /// panicking.
    pub(crate) fn maybe_flush(&self) {
        lumen_core::profile::claim_tree();
        // BUG-560: `element.focus()` changes `:focus`/`:focus-within` matching
        // without touching the DOM, so it never sets `flush_stale` — without
        // this check a same-tick `getComputedStyle()` right after `.focus()`
        // would keep serving the pre-focus snapshot even though the flush
        // below would otherwise happily recompute it. Compare against the
        // focus baked into the last flush rather than trusting
        // `flush_stale`/`never_flushed` alone.
        let current_focus = *self.focused_nid.lock().unwrap_or_else(|e| e.into_inner());
        let focus_changed =
            *self.last_flushed_focus.lock().unwrap_or_else(|e| e.into_inner()) != current_focus;
        // BUG-935 S34: gate on `flush_stale`, a dedicated flag set at every
        // same DOM-mutating call site as the scheduler's own `dom_dirty`
        // (`V8JsRuntime::dom_dirty`, a separate `Arc<AtomicBool>` this struct
        // no longer holds) but consumed only here — so a same-tick flush no
        // longer eats the scheduler's "DOM mutated" signal before its own
        // `take_dom_dirty`/`take_dom_dirty_lockfree` gets to see it.
        // BUG-935 S44: a same-tick sequence can call a DIFFERENT
        // `maybe_flush`-triggering native (e.g. `.focus()`'s scroll-into-view
        // via `_lumen_get_bounding_rect`) BEFORE the native that first sets
        // `computed_styles_needed`, consuming this early-return gate's single
        // "real flush" pass while the flag was still `false` — the later
        // `getComputedStyle` call would then find every other condition
        // already settled and skip its own recompute despite needing one
        // (caught by `v8_bug560_sync_focus`'s focus+getComputedStyle tests).
        // Mirrors `never_flushed`, but scoped to this one cache: `true` until
        // the first collect that actually ran while the flag was set.
        let computed_styles_pending = self.computed_styles_needed.load(Ordering::Relaxed)
            && !self.computed_styles_collected.load(Ordering::Relaxed);
        // BUG-935 S45: same bypass as `computed_styles_pending` above, applied
        // to the two S43 caches — S44 flagged this class as dormant for them
        // (no existing regression test hits the exact interference), but it
        // is the same defect shape: a different same-tick native can consume
        // this early-return gate's single "real flush" pass before the
        // native that first sets `pseudo_styles_needed`/`custom_props_needed`.
        let pseudo_styles_pending = self.pseudo_styles_needed.load(Ordering::Relaxed)
            && !self.pseudo_styles_collected.load(Ordering::Relaxed);
        let custom_props_pending = self.custom_props_needed.load(Ordering::Relaxed)
            && !self.custom_props_collected.load(Ordering::Relaxed);
        let text_frags_pending = self.text_frags_needed.load(Ordering::Relaxed)
            && !self.text_frags_collected.load(Ordering::Relaxed);
        if !self.never_flushed.load(Ordering::Relaxed)
            && !self.flush_stale.load(Ordering::Relaxed)
            && !focus_changed
            && !self.sheet_sync.dirty.load(Ordering::Relaxed)
            && !computed_styles_pending
            && !pseudo_styles_pending
            && !custom_props_pending
            && !text_frags_pending
        {
            return;
        }
        let flush_t0 = std::time::Instant::now();
        // BUG-935 S33: diagnostic-only counter — confirms/refutes whether a
        // per-`_lumen_get_bounding_rect`-call same-tick flush (CSSOM-4) fires a
        // *real* (non-no-op) full `layout_measured_with_counters` more than
        // once per `deliver_layout_observers` sweep (S32 found the whole sweep
        // taking seconds while the JS callback inside it measured 5-9ms).
        if lumen_paint::frame_log_enabled() {
            eprintln!(
                "[engine] maybe_flush real (never_flushed={} flush_stale={} focus_changed={} cssom_dirty={})",
                self.never_flushed.load(Ordering::Relaxed),
                self.flush_stale.load(Ordering::Relaxed),
                focus_changed,
                self.sheet_sync.dirty.load(Ordering::Relaxed),
            );
        }
        let Some(sheet) = self
            .stylesheet
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        else {
            return;
        };
        // CSSOM-8 вариант C: lay out against the cascade sheet *plus* every
        // CSSOM edit the page has made, replayed onto a throwaway clone. The
        // pushed sheet itself stays pristine — it is re-derived from the DOM
        // text by the shell whenever `<style>` content changes, and the same
        // log is replayed onto each new one, so an edit survives any number of
        // cascade rebuilds without ever being written back into the page CSS.
        let sheet = self.cssom_patched_sheet(&sheet).unwrap_or(sheet);
        let [vw, vh] = *self
            .viewport_size
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if vw <= 0.0 || vh <= 0.0 {
            return;
        }
        let Some(mut doc_guard) = lock_document_bounded(&self.doc) else {
            return;
        };
        let Ok(font) = lumen_font::Font::parse(FLUSH_FONT) else {
            return;
        };
        let Ok(measurer) = lumen_paint::FontMeasurer::new(&font) else {
            return;
        };
        let viewport = lumen_core::geom::Size::new(vw, vh);
        // BUG-560: install the engine thread's same-tick focus mirror before
        // laying out, so `:focus`/`:focus-within`/`:focus-visible` resolve
        // against the target `.focus()` just requested instead of the
        // thread-local's untouched "nothing focused" default (a real relayout
        // does the equivalent via `set_interactive_state` on the shell thread —
        // this flush runs on the engine thread, which never gets that call).
        // `:hover`/`:active` stay unset — out of this bug's scope.
        let focus_node = current_focus.map(lumen_dom::NodeId::from_raw);
        lumen_layout::set_interactive_state(None, focus_node, None);
        // BUG-1211: try the incremental restyle path first — a full
        // `layout_measured_with_counters` recomputes the *entire* tree's
        // cascade+layout on every same-tick accessor read after a mutation,
        // which is quadratic over a script that reads-after-writes N times in
        // a loop on a large real-site DOM (cnn/udemy/dailymail — see
        // `bugs/BUG-1211-OPEN.md`). Falls back to the full path whenever the
        // incremental one's preconditions (see `try_incremental_flush`)
        // don't hold — same-tick correctness (BUG-493) is identical either
        // way, only the cost differs.
        let touched = self
            .dom_touched
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        // BUG-935 S55: what changed in the document since the previous flush's
        // basis, from the document itself rather than from the JS bindings —
        // drained under this guard, so nothing can mutate between here and the
        // layout it licenses. `None` on the very first flush (starts the
        // record) and for a replaced document; either way no reuse is licensed.
        let content_journal = doc_guard.take_content_journal();
        let incr = self.try_incremental_flush(
            &doc_guard, &sheet, viewport, &measurer, current_focus, &touched, content_journal.as_ref(),
        );
        // BUG-1211 (post-collectors): `incr_scope` is `Some((dirty_roots,
        // prev_node_ids))` only when the incremental cascade+layout path
        // above actually ran — `dirty_roots` is where the fresh tree changed
        // (used to find the subtrees the four post-layout collectors below
        // need to re-walk), `prev_node_ids` is every node id those same
        // subtrees owned in the *previous* flush's tree (used to evict their
        // stale cache entries first, including ones for nodes removed from
        // the DOM since — see `try_incremental_flush`'s doc comment). `None`
        // means the full path ran and every collector below must rebuild its
        // whole-document map from scratch, same as before this slice.
        let (mut layout_root, counters, incr_scope) = match incr {
            Some((lr, c, dirty_roots, prev_node_ids, prev_node_raw_ids)) => {
                (lr, c, Some((dirty_roots, prev_node_ids, prev_node_raw_ids)))
            }
            None => {
                let (lr, c) =
                    lumen_layout::layout_measured_with_counters(&doc_guard, &sheet, viewport, &measurer);
                (lr, c, None)
            }
        };
        lumen_layout::clear_interactive_state();
        // BUG-1211: publish this flush's tree/cascade as the next same-tick
        // flush's incremental basis. This can be published even when the
        // full path just ran (not only the incremental one) — the full
        // recompute produced a fresh cascade+layout too, and either is a
        // valid starting point for the next flush's incremental attempt.
        *self.incr_basis.lock().unwrap_or_else(|e| e.into_inner()) = Some(IncrFlushBasis {
            layout: layout_root.clone(),
            cascade: counters.styles().clone(),
            viewport: [vw, vh],
            sheet_revision: sheet.revision(),
            focus: current_focus,
            touch_epoch: touched.epoch,
        });
        // BUG-504 part 10: the fresh tree above starts every scroll container
        // at `scroll_x`/`scroll_y == 0.0` (box-tree construction default) —
        // unlike a real relayout, this one-off flush tree never goes through
        // `graft_geometry`'s clone-unchanged-subtrees step, which is what
        // normally carries a prior scroll offset forward. Reapply the
        // pre-flush cache's offsets through `set_scroll_position`, which
        // clamps to the fresh (possibly now-different) scrollable extent and
        // zeroes out containers whose `overflow` just became `clip` — the
        // exact same rule a same-tick `scrollLeft`/`scrollTop` read after
        // e.g. `el.style.overflow = 'clip'` must observe.
        let prev_scroll = self
            .scroll_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for (&nid, s) in &prev_scroll {
            lumen_layout::set_scroll_position(
                &mut layout_root,
                lumen_dom::NodeId::from_raw(nid),
                s[0],
                s[1],
            );
        }
        // BUG-1211 (post-collectors): when the incremental cascade+layout
        // path ran, re-walk only the subtrees `dirty_roots` names instead of
        // the whole document — the four collectors below were, until this
        // slice, always O(whole document) regardless of how small the
        // incremental delta was, which is what left BUG-1211 open even
        // after the cascade+layout part went incremental (see the bug
        // file's "Частичный фикс" entry: ~90-120ms per flush here, vs.
        // 2-12µs for cascade+layout, on N=1500). `find_dirty_root_boxes`
        // locates each root's *fresh* box (present in `layout_root` even
        // when reused wholesale by the graft — reused subtrees keep their
        // node ids); the collectors below then only touch those subtrees'
        // nodes, and `prev_node_ids` (computed against the tree *before*
        // this restyle, back in `try_incremental_flush`) is evicted first
        // so a node the mutation removed from the DOM does not linger in
        // the caches with stale geometry/style forever.
        if let Some((dirty_roots, prev_node_ids, _)) = &incr_scope {
            let scoped_roots = lumen_layout::find_dirty_root_boxes(&layout_root, dirty_roots);
            {
                let mut lr = self.layout_rects.lock().unwrap_or_else(|e| e.into_inner());
                for nid in prev_node_ids {
                    lr.remove(nid);
                }
                lumen_layout::collect_layout_rects_scoped(&doc_guard, &scoped_roots, &mut lr);
            }
            {
                let mut cr = self.client_rects.lock().unwrap_or_else(|e| e.into_inner());
                for nid in prev_node_ids {
                    cr.remove(nid);
                }
                lumen_layout::collect_client_rects_scoped(&doc_guard, &scoped_roots, &mut cr);
            }
            if self.computed_styles_needed.load(Ordering::Relaxed) {
                let mut cs = self.computed_styles.lock().unwrap_or_else(|e| e.into_inner());
                // BUG-1211 (post-collectors) regression: `computed_styles`
                // is collected lazily (first read only, guarded by
                // `computed_styles_collected`) — the flush that FIRST turns
                // `computed_styles_needed` on is not necessarily the same
                // flush that has a non-empty `dirty_roots` (BUG-935 S44's
                // "a different native consumed the real-flush pass first"
                // already means it usually isn't: e.g. `.focus()`'s
                // scroll-into-view read runs the real flush, THEN
                // `getComputedStyle` sets `computed_styles_needed` on the
                // NEXT flush, whose `dirty_roots` is empty because nothing
                // changed since). Scoping to `dirty_roots` in that case
                // would leave the entire map empty forever — fall back to
                // one full collect exactly when this is that first-ever
                // collect, same as the always-on `layout_rects`/
                // `client_rects` above got for free by never being gated.
                if self.computed_styles_collected.load(Ordering::Relaxed) {
                    for nid in prev_node_ids {
                        cs.remove(nid);
                    }
                    lumen_layout::collect_computed_styles_scoped(
                        &doc_guard, &layout_root, dirty_roots, viewport, &mut cs,
                    );
                } else {
                    *cs = lumen_layout::collect_computed_styles(
                        &layout_root, &doc_guard, Some(&counters), viewport,
                    );
                }
                self.computed_styles_collected.store(true, Ordering::Relaxed);
            }
        } else {
            *self.layout_rects.lock().unwrap_or_else(|e| e.into_inner()) =
                lumen_layout::collect_layout_rects(&layout_root, &doc_guard);
            *self.client_rects.lock().unwrap_or_else(|e| e.into_inner()) =
                lumen_layout::collect_client_rects(&layout_root, &doc_guard);
            // BUG-935 S44: skip while the page has never read `computed_styles`
            // (via `getComputedStyle`/`computedStyleMap()`/`_lumen_request_scroll`)
            // — same rationale as the two collectors below. The setting native
            // calls `maybe_flush` right after, so a page's very first read still
            // forces a real collect here rather than serving a stale/empty map.
            if self.computed_styles_needed.load(Ordering::Relaxed) {
                *self.computed_styles.lock().unwrap_or_else(|e| e.into_inner()) =
                    lumen_layout::collect_computed_styles(&layout_root, &doc_guard, Some(&counters), viewport);
                self.computed_styles_collected.store(true, Ordering::Relaxed);
            }
        }
        // BUG-935 S43: skip while the page has never read the corresponding
        // cache — see the fields' doc comments. Each of the two natives that
        // can set the flag calls `maybe_flush` right after, so a page's very
        // first read still forces a real collect here rather than serving a
        // stale/empty map.
        //
        // BUG-1211: pseudo styles/custom properties/text frags are NOT
        // scoped to `incr_scope` — they are far rarer reads (most pages
        // never touch `::before`/`::after` computed style, CSS custom
        // properties or `CSS.highlights`), so they stay full-document,
        // gated only by their existing `_needed` flags, matching the
        // pre-BUG-1211 behaviour exactly.
        if self.pseudo_styles_needed.load(Ordering::Relaxed) {
            *self
                .pseudo_computed_styles
                .lock()
                .unwrap_or_else(|e| e.into_inner()) =
                lumen_layout::collect_pseudo_computed_styles(&layout_root);
            self.pseudo_styles_collected.store(true, Ordering::Relaxed);
        }
        if self.custom_props_needed.load(Ordering::Relaxed) {
            *self
                .custom_properties
                .lock()
                .unwrap_or_else(|e| e.into_inner()) =
                lumen_layout::collect_custom_properties(&layout_root, viewport);
            self.custom_props_collected.store(true, Ordering::Relaxed);
        }
        if self.text_frags_needed.load(Ordering::Relaxed) {
            *self
                .text_frag_rects
                .lock()
                .unwrap_or_else(|e| e.into_inner()) =
                lumen_layout::collect_text_frag_rects(&layout_root, &doc_guard);
            self.text_frags_collected.store(true, Ordering::Relaxed);
        }
        // BUG-1211: scroll containers ARE scoped — cheap per box (a handful
        // of field reads, no ancestor context needed) but still an
        // O(document) walk pre-scoping, and every same-tick flush refreshes
        // this cache unconditionally (no `_needed` gate), so it was paying
        // the full-document cost on every single incremental flush.
        if let Some((dirty_roots, _, prev_node_raw_ids)) = &incr_scope {
            let scoped_roots = lumen_layout::find_dirty_root_boxes(&layout_root, dirty_roots);
            let mut ss = self.scroll_states.lock().unwrap_or_else(|e| e.into_inner());
            for nid in prev_node_raw_ids {
                ss.remove(nid);
            }
            for c in lumen_layout::collect_scroll_containers_for_js_state_scoped(&scoped_roots) {
                ss.insert(c.node.raw(), [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]);
            }
        } else {
            *self.scroll_states.lock().unwrap_or_else(|e| e.into_inner()) =
                lumen_layout::collect_scroll_containers_for_js_state(&layout_root)
                    .iter()
                    .map(|c| (c.node.raw(), [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                    .collect();
        }
        self.never_flushed.store(false, Ordering::Relaxed);
        *self
            .last_flushed_focus
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = current_focus;
        // CSSOM-8 вариант C: `cssom_deltas` itself is never cleared — it is
        // replayed onto every future cascade rebuild too (see the field's doc
        // comment) — only the "has something changed since the last flush"
        // gate resets.
        self.sheet_sync.dirty.store(false, Ordering::Relaxed);
        // BUG-935 S34: only `flush_stale` resets here — `dom_dirty` is the
        // scheduler's own signal and stays untouched by this flush.
        self.flush_stale.store(false, Ordering::Relaxed);
        // BUG-935 срез 53: the cost and path of every real flush, so a live
        // `[js-stall]` sample can be matched to its forced-reflow count.
        if lumen_paint::frame_log_enabled() {
            eprintln!(
                "[engine] maybe_flush done {:.1}ms path={} dirty_roots={} touched={}",
                flush_t0.elapsed().as_secs_f64() * 1000.0,
                if incr_scope.is_some() { "incremental" } else { "full" },
                incr_scope.as_ref().map_or(0, |(roots, _, _)| roots.len()),
                touched.nodes.len(),
            );
        }
    }

    /// BUG-1211: attempt the incremental cascade+layout path instead of a
    /// full [`lumen_layout::layout_measured_with_counters`] recompute.
    ///
    /// Mirrors `Lumen::try_relayout_raf_incremental` (`crates/shell/src/
    /// relayout.rs`) — the JS-mutation branch of that function is the exact
    /// precedent this reimplements for the engine thread's own same-tick
    /// flush, using the same `RestyleDelta`/`layout_mutation_incremental_restyle`
    /// primitives (BUG-341). Returns `None` whenever any precondition that
    /// path also requires doesn't hold, so the caller falls back to the full
    /// recompute — same-tick correctness (BUG-493) never depends on which
    /// branch ran, only the cost does:
    ///
    /// * no previous basis yet ([`FlushHandles::incr_basis`] is `None` — the
    ///   first flush after navigation/creation, or the last one fell back);
    /// * the viewport or the stylesheet's [`lumen_css_parser::
    ///   StylesheetRevision`] changed since the basis was taken — either can
    ///   affect any node's cascade, so nothing in the old tree is safely
    ///   reusable (mirrors BUG-743's rationale in `relayout.rs`);
    /// * [`super::runtime::DomTouched::unattributed`] is set — an untracked
    ///   mutation primitive (`execCommand`, contenteditable, Shadow DOM
    ///   attach) whose reach `dirty_roots` cannot express.
    ///
    /// `:hover`/`:active` are never tracked by this flush (the module doc
    /// comment's "Known remaining approximation") and stay unset in both
    /// paths, so unlike `relayout.rs` there is no third interactive-state
    /// axis to fold into `dirty_roots` here — only the focus transition.
    #[allow(clippy::too_many_arguments)]
    fn try_incremental_flush(
        &self,
        doc: &lumen_dom::Document,
        sheet: &Arc<lumen_css_parser::Stylesheet>,
        viewport: lumen_core::geom::Size,
        measurer: &lumen_paint::FontMeasurer<'_>,
        current_focus: Option<u32>,
        touched: &super::runtime::DomTouched,
        content_journal: Option<&std::collections::HashSet<lumen_dom::NodeId>>,
    ) -> Option<IncrFlushResult> {
        if touched.unattributed {
            return None;
        }
        let mut basis_guard = self.incr_basis.lock().unwrap_or_else(|e| e.into_inner());
        let basis = basis_guard.take()?;
        if basis.viewport != [viewport.width, viewport.height] || basis.sheet_revision != sheet.revision() {
            return None;
        }
        // BUG-1211: `touched.nodes`/`touched.touch_gen` are never drained by
        // this flush (see `FlushHandles::dom_touched`'s doc comment) — they
        // keep accruing every attributed mutation since the shell's own
        // tracker last drained them, which can include nodes already folded
        // into an earlier incremental basis. Diff against `basis.
        // touch_epoch` (the `DomTouched::epoch` watermark this basis was
        // taken at) rather than using the whole accrued set: a node whose
        // last touch predates the basis is already reflected in `basis.
        // cascade`/`basis.layout`, and recomputing its subtree again on
        // every subsequent flush is exactly the quadratic-over-reads cost
        // this incremental path exists to avoid. An earlier revision of
        // this function diffed against a per-flush snapshot of `touched.
        // nodes` itself (a plain `HashSet`, no per-node generation), which
        // is unsound: a node touched again *after* being folded into a
        // basis is indistinguishable from one touched only before it once
        // both are just "present in the set", so a second same-tick
        // mutation to an already-seen node silently dropped out of
        // `dirty_roots`.
        let new_touched: std::collections::HashSet<lumen_dom::NodeId> = touched
            .nodes
            .iter()
            .copied()
            .filter(|n| touched.touch_gen.get(n).copied().unwrap_or(0) > basis.touch_epoch)
            .collect();
        let tp0 = std::time::Instant::now();
        let node_index = lumen_layout::style::restyle_node_index(doc, sheet);
        let tp_index = tp0.elapsed();
        let mut dirty_roots = std::collections::HashSet::new();
        // BUG-1211: a node whose every touch since the basis was a plain
        // attribute write is reported by name, so the root-set can ask which
        // selectors could react to it (`el.style.width = …` rarely widens);
        // anything else (child list, text, dirty value) stays `Unattributed`
        // and widens to the parent as before. The `Attr(&str)` borrows live
        // in `touched`, which outlives this call.
        let mut changes: Vec<(lumen_dom::NodeId, lumen_layout::style::NodeChange<'_>)> = Vec::new();
        for &n in &new_touched {
            let structural = touched.structural_gen.get(&n).copied().unwrap_or(0) > basis.touch_epoch;
            let named: Vec<&str> = touched
                .attr_gen
                .get(&n)
                .into_iter()
                .flatten()
                .filter(|&(_, &g)| g > basis.touch_epoch)
                .map(|(name, _)| &**name)
                .collect();
            if structural || named.is_empty() {
                changes.push((n, lumen_layout::style::NodeChange::Unattributed));
            } else {
                changes.extend(named.into_iter().map(|a| (n, lumen_layout::style::NodeChange::Attr(a))));
            }
        }
        dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(doc, changes, &node_index));
        let focus_changed = basis.focus != current_focus;
        if focus_changed {
            let state_index = lumen_layout::style::restyle_state_index(doc, sheet);
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                doc, basis.focus.map(lumen_dom::NodeId::from_raw), current_focus.map(lumen_dom::NodeId::from_raw), &state_index,
            ));
        }
        let tp_roots = tp0.elapsed();
        // BUG-935 S55: a complete per-node content record licenses reuse of
        // every box subtree the cascade left alone (`clean_subtrees`) — before
        // this, every flush rebuilt and re-compared the whole box tree even when
        // one `style.width` had changed (26 + 25 ms of a 55 ms layout on the
        // 1500-div stand). The journal is the document's own record; the JS
        // tracker's nodes are added as a belt-and-braces union. Anything the
        // journal cannot vouch for — no baseline, or a shadow tree / `<slot>`
        // involved (see `journal_touches_shadow`) — stays `Untracked`.
        let content_nodes: std::collections::HashSet<lumen_dom::NodeId>;
        let content_dirty = match content_journal {
            Some(journal) if !content_journal_disabled() && !doc.journal_touches_shadow(journal) => {
                content_nodes = journal.iter().chain(new_touched.iter()).copied().collect();
                lumen_layout::counters::ContentDirty::Nodes(&content_nodes)
            }
            _ => lumen_layout::counters::ContentDirty::Untracked,
        };
        // BUG-1211 (post-collectors): snapshot which nodes the touched
        // subtrees owned in the *previous* (`basis.layout`) tree before it
        // is moved into `layout_mutation_incremental_restyle` below — the
        // caller needs this to evict stale cache entries (including ones
        // for a node removed from the DOM by this same mutation, which the
        // *fresh* tree's subtree scan below can never see) before
        // re-inserting from the fresh subtree. Taken against `dirty_roots`
        // before it moves into `delta`.
        let prev_node_ids: std::collections::HashSet<u32> =
            lumen_layout::find_dirty_root_boxes(&basis.layout, &dirty_roots)
                .into_iter()
                .flat_map(lumen_layout::collect_subtree_node_indices)
                .collect();
        let prev_node_raw_ids: std::collections::HashSet<u32> =
            lumen_layout::find_dirty_root_boxes(&basis.layout, &dirty_roots)
                .into_iter()
                .flat_map(lumen_layout::collect_subtree_node_raw_ids)
                .collect();
        let dirty_roots_for_return = dirty_roots.clone();
        let delta = lumen_layout::counters::RestyleDelta {
            prev_styles: basis.cascade,
            dirty_roots,
            content_dirty,
        };
        let null_hp = lumen_core::ext::NullHyphenationProvider;
        lumen_layout::counters::set_incremental_restyle(true);
        lumen_layout::box_tree::set_incremental_box_build(true);
        let tp_prev = tp0.elapsed();
        let result = lumen_layout::box_tree::layout_mutation_incremental_restyle(
            doc, sheet, viewport, measurer, &null_hp, false, basis.layout, delta,
        );
        lumen_layout::box_tree::set_incremental_box_build(false);
        lumen_layout::counters::set_incremental_restyle(false);
        if lumen_paint::frame_log_enabled() {
            eprintln!("[engine] incr stages: index={:.1} roots={:.1} prev={:.1} layout_done={:.1}", tp_index.as_secs_f64()*1e3, tp_roots.as_secs_f64()*1e3, tp_prev.as_secs_f64()*1e3, tp0.elapsed().as_secs_f64()*1e3);
        }
        Some((result.0, result.1, dirty_roots_for_return, prev_node_ids, prev_node_raw_ids))
    }

    /// CSSOM-8 вариант C: replay every recorded CSSOM write onto a throwaway
    /// clone of `sheet` — see [`super::sheet_sync::patched_cascade`] for the
    /// index translation and ordering. `None` when there is nothing recorded
    /// (the common case — most pages never call `CSSStyleSheet.insertRule`/
    /// `.deleteRule`/write `.style`), so the caller can skip the clone.
    ///
    /// BUG-493: the registry is reconciled with the DOM first, so a `<style>`
    /// the script inserted after the shell last pushed `sheet` still takes
    /// part (its text is merged whole — the shell's sheet predates it). And
    /// when `sheet` is itself the shell's already-patched cascade
    /// (`V8JsRuntime::patch_cascade`, recognised by its revision), the edits
    /// are replayed onto the pristine sheet it was made from instead, so they
    /// are never applied twice.
    fn cssom_patched_sheet(
        &self,
        sheet: &Arc<lumen_css_parser::Stylesheet>,
    ) -> Option<Arc<lumen_css_parser::Stylesheet>> {
        self.sheet_sync.sync();
        let base = match &*self
            .sheet_sync
            .pristine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            Some((rev, pristine)) if *rev == sheet.revision() => Arc::clone(pristine),
            _ => Arc::clone(sheet),
        };
        let nodes = self
            .sheet_sync
            .nodes
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let deltas = self
            .sheet_sync
            .deltas
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let shadow = self
            .sheet_sync
            .shadow_owned
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match super::sheet_sync::patched_cascade(&base, &nodes, &deltas, &shadow, true) {
            Some(patched) => Some(Arc::new(patched)),
            // Nothing to replay onto the pristine sheet — the shell's patched
            // one carries edits that were since dropped, so use the pristine.
            None if !Arc::ptr_eq(&base, sheet) => Some(base),
            None => None,
        }
    }
}
