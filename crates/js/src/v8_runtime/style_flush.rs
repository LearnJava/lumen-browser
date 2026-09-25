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
        if !self.never_flushed.load(Ordering::Relaxed)
            && !self.flush_stale.load(Ordering::Relaxed)
            && !focus_changed
            && !self.sheet_sync.dirty.load(Ordering::Relaxed)
            && !computed_styles_pending
            && !pseudo_styles_pending
            && !custom_props_pending
        {
            return;
        }
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
        let Some(doc_guard) = lock_document_bounded(&self.doc) else {
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
        let (mut layout_root, counters) =
            lumen_layout::layout_measured_with_counters(&doc_guard, &sheet, viewport, &measurer);
        lumen_layout::clear_interactive_state();
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
        *self
            .layout_rects
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = lumen_layout::collect_layout_rects(&layout_root, &doc_guard);
        *self
            .client_rects
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = lumen_layout::collect_client_rects(&layout_root, &doc_guard);
        // BUG-935 S44: skip while the page has never read `computed_styles`
        // (via `getComputedStyle`/`computedStyleMap()`/`_lumen_request_scroll`)
        // — same rationale as the two collectors below. The setting native
        // calls `maybe_flush` right after, so a page's very first read still
        // forces a real collect here rather than serving a stale/empty map.
        if self.computed_styles_needed.load(Ordering::Relaxed) {
            *self
                .computed_styles
                .lock()
                .unwrap_or_else(|e| e.into_inner()) =
                lumen_layout::collect_computed_styles(&layout_root, &doc_guard, Some(&counters), viewport);
            self.computed_styles_collected.store(true, Ordering::Relaxed);
        }
        // BUG-935 S43: skip while the page has never read the corresponding
        // cache — see the fields' doc comments. Each of the two natives that
        // can set the flag calls `maybe_flush` right after, so a page's very
        // first read still forces a real collect here rather than serving a
        // stale/empty map.
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
        *self
            .scroll_states
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = lumen_layout::collect_scroll_containers_for_js_state(&layout_root)
            .iter()
            .map(|c| (c.node.raw(), [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
            .collect();
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
