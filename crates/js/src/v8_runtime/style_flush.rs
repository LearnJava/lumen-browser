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
//! This slice covers [`super::V8JsRuntime::update_stylesheet`] callers only —
//! `InProcessSession` (headless/WPT/driver path), pushed once per navigation
//! right after CSS parsing. The interactive shell never calls
//! `update_stylesheet`, so [`FlushHandles::stylesheet`] stays `None` there
//! and [`FlushHandles::maybe_flush`] is a no-op — exactly its pre-CSSOM-4
//! behaviour, zero regression risk for the live multithreaded pipeline
//! (ADR-016): routing a flush through the engine thread from inside a native
//! would risk a deadlock when the engine thread is itself mid-rAF-turn
//! waiting on the JS thread, so this deliberately never touches the engine
//! thread — extending coverage to the shell needs its own stylesheet-push
//! call site plus a decision on dark-mode/forced-colors/web-fonts
//! thread-locals, tracked as a follow-up slice, not attempted here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::named_access::lock_document_bounded;
use super::runtime::CustomPropertySnapshot;

/// Bundled embedder-pushed state a same-tick accessor native needs to force
/// a synchronous flush — one `Clone` (every field is an `Arc`) instead of
/// threading eight separate parameters through each `install_*` call site.
#[derive(Clone)]
pub(crate) struct FlushHandles {
    pub(crate) doc: Arc<Mutex<lumen_dom::Document>>,
    pub(crate) layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    pub(crate) computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    pub(crate) custom_properties: Arc<Mutex<CustomPropertySnapshot>>,
    pub(crate) viewport_size: Arc<Mutex<[f32; 2]>>,
    pub(crate) stylesheet: Arc<Mutex<Option<Arc<lumen_css_parser::Stylesheet>>>>,
    pub(crate) dom_dirty: Arc<AtomicBool>,
    pub(crate) never_flushed: Arc<AtomicBool>,
}

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
        if !self.never_flushed.load(Ordering::Relaxed) && !self.dom_dirty.load(Ordering::Relaxed) {
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
        let (layout_root, counters) =
            lumen_layout::layout_measured_with_counters(&doc_guard, &sheet, viewport, &measurer);
        *self
            .layout_rects
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = lumen_layout::collect_layout_rects(&layout_root, &doc_guard);
        *self
            .computed_styles
            .lock()
            .unwrap_or_else(|e| e.into_inner()) =
            lumen_layout::collect_computed_styles(&layout_root, &doc_guard, Some(&counters));
        *self
            .custom_properties
            .lock()
            .unwrap_or_else(|e| e.into_inner()) =
            lumen_layout::collect_custom_properties(&layout_root, viewport);
        self.never_flushed.store(false, Ordering::Relaxed);
    }
}
