//! BUG-935 S41/S51: JS geometry gathered after a relayout before it is pushed
//! to JS — collected inline on the UI thread or precomputed on the engine
//! thread and carried back in `EngineCommit`.
//!
//! Moved verbatim out of `relayout.rs` (file-size gate, docs/lint-policy.md
//! §5.1); only module path and visibility differ.

use crate::*;
#[cfg(feature = "v8")]
use crate::layout_shift::{LayoutShiftSource, compute_layout_shift_score};

/// BUG-935 S41: everything [`Lumen::apply_relayout_result`]'s JS-geometry
/// block gathers before pushing to JS (S37-S40 found this the dominant cost
/// of `apply_relayout_result`, 60-105ms/tick, already paid on the UI thread
/// today via `poll_engine_commit`). Computed once by [`collect_js_data`],
/// reused by whichever producer supplies it: inline on the UI thread (the
/// fully synchronous [`Lumen::relayout`], which has no engine-thread job to
/// piggy-back on) or precomputed on the engine thread inside
/// [`Lumen::make_relayout_job`]'s closure, carried back in [`EngineCommit`].
#[cfg(feature = "v8")]
pub(crate) struct PrecollectedJsData {
    pub(crate) rects: std::collections::HashMap<u32, [f32; 4]>,
    pub(crate) layout_shift_score: f64,
    pub(crate) layout_shift_sources: Vec<LayoutShiftSource>,
    pub(crate) had_input: bool,
    pub(crate) client_rects: std::collections::HashMap<u32, Vec<[f32; 4]>>,
    pub(crate) hit_test_tree: Arc<lumen_layout::LayoutBox>,
    /// BUG-935 S44: `None` when the matching `*_needed` flag gated this
    /// collector off THIS tick — the caller must then skip the matching
    /// `update_*` push entirely rather than pass an empty map. `update_*`
    /// replaces the JS runtime's cache wholesale (no merge), so pushing an
    /// empty map on a tick where the flag simply hadn't been set yet would
    /// wipe out a valid snapshot a same-tick CSSOM-4 flush (`style_flush.rs`)
    /// had already populated via a different, non-gated write path.
    pub(crate) styles: Option<std::collections::HashMap<u32, lumen_layout::StyleMap>>,
    pub(crate) pseudo_styles:
        Option<std::collections::HashMap<(u32, String), std::collections::HashMap<String, String>>>,
    pub(crate) customs:
        Option<std::collections::HashMap<u32, Arc<std::collections::HashMap<String, String>>>>,
    pub(crate) scroll_states: std::collections::HashMap<u32, [f32; 4]>,
    /// The layout-shift baseline this commit installs into
    /// [`Lumen::prev_layout_shift_rects`] — only when this data is actually
    /// applied. A superseded engine-thread commit is dropped by the caller
    /// before ever reaching that assignment, so a discarded job's baseline
    /// never leaks into a later one (same generation-guard `poll_engine_commit`
    /// already relies on for the rest of the commit).
    pub(crate) next_layout_shift_baseline: std::collections::HashMap<u32, [f32; 4]>,
}

/// BUG-935 S51: the cheap, layout-shift half of [`collect_js_data`] — kept
/// on whichever thread applies the commit even when the expensive half
/// ([`collect_js_tree_data`]) moves into a deferred engine-thread task,
/// because it reads and advances [`Lumen::prev_layout_shift_rects`]: running
/// it later, on another thread, would need a second hand-off just to bring
/// the next baseline back. Measured sub-millisecond on lenta.ru
/// (`collect_layout_shift_rects` ~0.4ms, `layout_shift_score` ~0.04ms).
#[cfg(feature = "v8")]
pub(crate) struct JsShiftData {
    pub(crate) score: f64,
    pub(crate) sources: Vec<LayoutShiftSource>,
    pub(crate) had_input: bool,
    pub(crate) next_baseline: std::collections::HashMap<u32, [f32; 4]>,
}

#[cfg(feature = "v8")]
pub(crate) fn collect_js_shift_data(
    lb_ref: &lumen_layout::LayoutBox,
    viewport: Size,
    prev_layout_shift_rects: &std::collections::HashMap<u32, [f32; 4]>,
    last_input_epoch_s: f32,
    now_s: f32,
) -> JsShiftData {
    let step_log = lumen_paint::frame_log_enabled();
    macro_rules! step {
        ($label:literal, $expr:expr) => {{
            let t0 = step_log.then(std::time::Instant::now);
            let result = $expr;
            if let Some(t0) = t0 {
                let ms = t0.elapsed().as_secs_f32() * 1000.0;
                eprintln!("[engine] apply-step {ms:.2}ms ({})", $label);
            }
            result
        }};
    }
    let shift_rects = step!(
        "collect_layout_shift_rects",
        lumen_layout::collect_layout_shift_rects(lb_ref)
    );
    let layout_shift = step!(
        "layout_shift_score",
        compute_layout_shift_score(prev_layout_shift_rects, &shift_rects, viewport.width, viewport.height)
    );
    JsShiftData {
        score: layout_shift.score,
        sources: layout_shift.sources,
        had_input: now_s - last_input_epoch_s < 0.5,
        next_baseline: shift_rects,
    }
}

/// BUG-935 S41: the pure collection step factored out of
/// [`Lumen::apply_relayout_result`] so [`Lumen::make_relayout_job`] can run
/// it on the engine thread instead — S40 found this already the dominant
/// UI-thread cost of every commit, off-thread or not. Depends only on the
/// freshly computed layout tree, the (locked) document and a layout-shift
/// baseline snapshot — no `Lumen` field, so it needs neither `&self` nor the
/// renderer/frame state. BUG-935 S51: now the composition of
/// [`collect_js_shift_data`] and [`collect_js_tree_data`].
#[cfg(feature = "v8")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_js_data(
    lb_ref: &lumen_layout::LayoutBox,
    doc_guard: std::sync::MutexGuard<'_, Document>,
    viewport: Size,
    prev_layout_shift_rects: &std::collections::HashMap<u32, [f32; 4]>,
    last_input_epoch_s: f32,
    now_s: f32,
    // BUG-935 S43/S44: skip the collectors below while the page has never
    // read the corresponding cache. S42 found `_lumen_request_scroll` reading
    // `computed_styles` outside the `getComputedStyle`-family signal; S44
    // made that native set `computed_styles_needed` too, so the same gate is
    // now safe for all three caches.
    pseudo_styles_needed: bool,
    custom_props_needed: bool,
    computed_styles_needed: bool,
) -> PrecollectedJsData {
    let shift = collect_js_shift_data(lb_ref, viewport, prev_layout_shift_rects, last_input_epoch_s, now_s);
    let step_log = lumen_paint::frame_log_enabled();
    let t0 = step_log.then(std::time::Instant::now);
    let hit_test_tree = Arc::new(lb_ref.clone());
    if let Some(t0) = t0 {
        eprintln!("[engine] apply-step {:.2}ms (clone_hit_test_tree)", t0.elapsed().as_secs_f32() * 1000.0);
    }
    collect_js_tree_data(
        hit_test_tree,
        doc_guard,
        viewport,
        shift,
        pseudo_styles_needed,
        custom_props_needed,
        computed_styles_needed,
    )
}

/// BUG-935 S51: the expensive half of [`collect_js_data`] — every tree walk
/// (`collect_layout_rects`/`collect_client_rects`/`collect_computed_styles`
/// and the gated pseudo/custom-property/scroll collectors), 160-700ms per
/// tick on lenta.ru. Takes the layout tree as the `Arc` that is also pushed
/// to JS as `hit_test_tree`, so a caller that defers this into an
/// engine-thread task ([`DeferredJsCollect`]) pays for exactly one clone of
/// the tree on its own thread — the same clone the inline path already made.
#[cfg(feature = "v8")]
#[allow(clippy::too_many_arguments)]
fn collect_js_tree_data(
    hit_test_tree: Arc<lumen_layout::LayoutBox>,
    doc_guard: std::sync::MutexGuard<'_, Document>,
    viewport: Size,
    shift: JsShiftData,
    pseudo_styles_needed: bool,
    custom_props_needed: bool,
    computed_styles_needed: bool,
) -> PrecollectedJsData {
    let step_log = lumen_paint::frame_log_enabled();
    macro_rules! step {
        ($label:literal, $expr:expr) => {{
            let t0 = step_log.then(std::time::Instant::now);
            let result = $expr;
            if let Some(t0) = t0 {
                let ms = t0.elapsed().as_secs_f32() * 1000.0;
                eprintln!("[engine] apply-step {ms:.2}ms ({})", $label);
            }
            result
        }};
    }
    let lb_ref: &lumen_layout::LayoutBox = &hit_test_tree;
    let rects = step!("collect_layout_rects", collect_layout_rects(lb_ref, &doc_guard));
    let client_rects = step!("collect_client_rects", collect_client_rects(lb_ref, &doc_guard));
    let styles = if computed_styles_needed {
        Some(step!("collect_computed_styles", collect_computed_styles(lb_ref, &doc_guard, None, viewport)))
    } else {
        None
    };
    let pseudo_styles = if pseudo_styles_needed {
        Some(step!(
            "collect_pseudo_computed_styles",
            collect_pseudo_computed_styles(lb_ref)
        ))
    } else {
        None
    };
    // Drop the document lock before the remaining collectors, which read
    // only `lb_ref`/`viewport` — matches the lock-hold window of the inline
    // path this replaced (BUG-935 S37's original block dropped it here too).
    drop(doc_guard);
    let customs = if custom_props_needed {
        Some(step!(
            "collect_custom_properties",
            collect_custom_properties(lb_ref, viewport)
        ))
    } else {
        None
    };
    let scroll_states: std::collections::HashMap<u32, [f32; 4]> = step!(
        "collect_scroll_containers",
        collect_scroll_containers_for_js_state(lb_ref)
            .iter()
            .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
            .collect()
    );
    PrecollectedJsData {
        rects,
        layout_shift_score: shift.score,
        layout_shift_sources: shift.sources,
        had_input: shift.had_input,
        client_rects,
        hit_test_tree,
        styles,
        pseudo_styles,
        customs,
        scroll_states,
        next_layout_shift_baseline: shift.next_baseline,
    }
}

/// BUG-935 S51: everything [`collect_js_tree_data`] needs, captured on the UI
/// thread by [`Lumen::apply_relayout_result`] and run later inside the
/// deferred JS-push engine task — the M4 incremental path
/// (`try_relayout_raf_incremental`) and the synchronous `relayout()` have no
/// off-thread layout job to precollect in (S41 only covered
/// `poll_engine_commit`), so before this slice they paid the whole
/// collection on the UI thread: S13 of BUG-1112 measured it at 173-700ms per
/// rAF tick on lenta.ru under `LUMEN_BUG935_M4_SWAP=1`, the actual cause of
/// the S47 freeze. The document is snapshotted on the engine thread, between
/// JS turns — the same window `make_relayout_job`'s own collection already
/// runs in, so a script that mutated the DOM after this layout sees exactly
/// the snapshot semantics the off-thread path has had since S41.
#[cfg(feature = "v8")]
pub(crate) struct DeferredJsCollect {
    pub(crate) hit_test_tree: Arc<lumen_layout::LayoutBox>,
    pub(crate) document: Arc<std::sync::Mutex<Document>>,
    pub(crate) viewport: Size,
    pub(crate) shift: JsShiftData,
    pub(crate) pseudo_styles_needed: bool,
    pub(crate) custom_props_needed: bool,
    pub(crate) computed_styles_needed: bool,
}

#[cfg(feature = "v8")]
impl DeferredJsCollect {
    /// `None` only when the document lock is poisoned — the inline path
    /// skipped the whole push on the same condition.
    pub(crate) fn run(self) -> Option<PrecollectedJsData> {
        // Hold the document lock only for a snapshot clone, not for the whole
        // 100-250ms collection: the UI thread re-locks the document right
        // after spawning this task (`spawn_dynamic_image_loads`, hit tests,
        // event dispatch), and the first live A/B of S51 showed it simply
        // blocking on this mutex for the full collection instead of running
        // it — the freeze moved, it did not go away.
        let step_log = lumen_paint::frame_log_enabled();
        let t0 = step_log.then(std::time::Instant::now);
        let snapshot = std::sync::Mutex::new(self.document.lock().ok()?.clone());
        if let Some(t0) = t0 {
            eprintln!("[engine] task-step {:.2}ms (js_collect_doc_snapshot)", t0.elapsed().as_secs_f32() * 1000.0);
        }
        let doc_guard = snapshot.lock().ok()?;
        Some(collect_js_tree_data(
            self.hit_test_tree,
            doc_guard,
            self.viewport,
            self.shift,
            self.pseudo_styles_needed,
            self.custom_props_needed,
            self.computed_styles_needed,
        ))
    }
}

/// BUG-935 S51: JS geometry for one commit — already collected, or captured
/// for collection inside the deferred push task (see [`DeferredJsCollect`]).
#[cfg(feature = "v8")]
pub(crate) enum PendingJsData {
    Ready(Box<PrecollectedJsData>),
    Deferred(DeferredJsCollect),
}

#[cfg(feature = "v8")]
impl PendingJsData {
    pub(crate) fn resolve(self) -> Option<PrecollectedJsData> {
        match self {
            Self::Ready(data) => Some(*data),
            Self::Deferred(d) => d.run(),
        }
    }
}
