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
    pub(crate) dom_dirty: Arc<AtomicBool>,
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
    /// CSSOM-8 вариант C: the per-`<style>`/`<link>` registry, read here to
    /// map a recorded CSSOM edit onto the cascade sheet — an entry's own
    /// `Stylesheet::source` is the needle
    /// [`lumen_css_parser::Stylesheet::locate_embedded_source`] looks for.
    pub(crate) stylesheet_nodes: Arc<Mutex<Vec<lumen_css_parser::StylesheetNodeEntry>>>,
    /// CSSOM-8 вариант C: every CSSOM write against an owned sheet, in the
    /// order it happened, tagged with its owner node — see [`CssomDeltaLog`].
    pub(crate) cssom_deltas: CssomDeltaLog,
    /// CSSOM-8 вариант C: set when [`Self::cssom_deltas`] grows, cleared at
    /// the end of a successful flush. A CSSOM write touches neither the DOM
    /// nor the focus, so without this the gate below would serve the
    /// pre-mutation snapshot to a same-tick `getComputedStyle()` — exactly
    /// the half `.style`/`insertRule` was missing before this slice.
    pub(crate) cssom_dirty: Arc<AtomicBool>,
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
        // without touching the DOM, so it never sets `dom_dirty` — without this
        // check a same-tick `getComputedStyle()` right after `.focus()` would
        // keep serving the pre-focus snapshot even though the flush below would
        // otherwise happily recompute it. Compare against the focus baked into
        // the last flush rather than trusting `dom_dirty`/`never_flushed` alone.
        let current_focus = *self.focused_nid.lock().unwrap_or_else(|e| e.into_inner());
        let focus_changed =
            *self.last_flushed_focus.lock().unwrap_or_else(|e| e.into_inner()) != current_focus;
        if !self.never_flushed.load(Ordering::Relaxed)
            && !self.dom_dirty.load(Ordering::Relaxed)
            && !focus_changed
            && !self.cssom_dirty.load(Ordering::Relaxed)
        {
            return;
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
        let focus_node = current_focus.map(|n| lumen_dom::NodeId::from_index(n as usize));
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
                lumen_dom::NodeId::from_index(nid as usize),
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
        *self
            .computed_styles
            .lock()
            .unwrap_or_else(|e| e.into_inner()) =
            lumen_layout::collect_computed_styles(&layout_root, &doc_guard, Some(&counters));
        *self
            .pseudo_computed_styles
            .lock()
            .unwrap_or_else(|e| e.into_inner()) =
            lumen_layout::collect_pseudo_computed_styles(&layout_root);
        *self
            .custom_properties
            .lock()
            .unwrap_or_else(|e| e.into_inner()) =
            lumen_layout::collect_custom_properties(&layout_root, viewport);
        *self
            .scroll_states
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = lumen_layout::collect_scroll_containers_for_js_state(&layout_root)
            .iter()
            .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
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
        self.cssom_dirty.store(false, Ordering::Relaxed);
    }

    /// CSSOM-8 вариант C: replay every recorded CSSOM write
    /// ([`Self::cssom_deltas`]) onto a throwaway clone of `sheet`, address-
    /// translated through each owning node's own parsed source text. `None`
    /// when there is nothing recorded (the common case — most pages never
    /// call `CSSStyleSheet.insertRule`/`.deleteRule`/write `.style`), so the
    /// caller can skip the clone and lay out against the pristine `sheet`
    /// directly.
    ///
    /// Processes nodes in descending `cssRules`-base order so that an
    /// earlier-in-page node's `insertRule`/`deleteRule` — which shifts every
    /// `cssRules` index after it in the working clone — never invalidates a
    /// `base` already computed for a node further down the page: every
    /// `base` is computed once, up front, against the pristine `sheet`
    /// (never against the working clone once it starts mutating), and a
    /// later-in-page node's own edits can only ever shift indices *above*
    /// its own `base`, never below it — see
    /// [`lumen_css_parser::Stylesheet::cssom_range_for_source_span`]'s doc
    /// comment for why the translation itself is only valid pristine.
    fn cssom_patched_sheet(
        &self,
        sheet: &Arc<lumen_css_parser::Stylesheet>,
    ) -> Option<Arc<lumen_css_parser::Stylesheet>> {
        let deltas = self.cssom_deltas.lock().unwrap_or_else(|e| e.into_inner());
        if deltas.is_empty() {
            return None;
        }
        let nodes = self
            .stylesheet_nodes
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // `base` for every node carrying at least one recorded op, resolved
        // against the pristine `sheet` while walking `nodes` in document
        // order — the same cursor discipline `locate_embedded_source`'s doc
        // comment describes, so two nodes with byte-identical bodies each
        // resolve to their own occurrence rather than both to the first.
        let mut bases: HashMap<u32, usize> = HashMap::new();
        let mut cursor = 0usize;
        for entry in nodes.iter() {
            let Some(needle) = entry.sheet.source() else {
                continue;
            };
            let Some((start, end)) = sheet.locate_embedded_source(needle, cursor) else {
                continue;
            };
            cursor = end;
            if deltas.iter().any(|(n, _)| *n == entry.node) {
                let (base, _count) = sheet.cssom_range_for_source_span(start, end);
                bases.insert(entry.node, base);
            }
        }
        drop(nodes);
        if bases.is_empty() {
            // Every op's owner node has since disappeared from the registry
            // (page navigated away from under it, or removed the `<style>`)
            // or never resolved a byte range — nothing to replay.
            return None;
        }
        let mut order: Vec<u32> = bases.keys().copied().collect();
        order.sort_unstable_by_key(|n| std::cmp::Reverse(bases[n]));
        let mut patched = (**sheet).clone();
        for node in order {
            let base = bases[&node];
            let ops: Vec<lumen_css_parser::CssomOp> = deltas
                .iter()
                .filter(|(n, _)| *n == node)
                .map(|(_, op)| op.clone())
                .collect();
            // Best-effort: an op that fails to apply (e.g. its recorded index
            // is now out of range because a later native call already
            // deleted the rule) leaves the rest of this node's ops applied —
            // no error channel back to the JS call site that recorded it.
            let _ = patched.replay_cssom_ops(base, &ops);
        }
        Some(Arc::new(patched))
    }
}
