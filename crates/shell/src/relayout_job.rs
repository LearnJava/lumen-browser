//! Off-thread relayout job of the shell (ADR-016 M2.2): building the
//! immutable-snapshot job, routing it to the engine thread, reading the commit
//! back, and the post-layout UI-thread work shared by both paths
//! (`apply_relayout_result`).
//!
//! Moved verbatim out of `relayout.rs` (file-size gate, docs/lint-policy.md
//! §5.1). Method bodies are unchanged; only module path and visibility differ.

use crate::*;
#[cfg(feature = "v8")]
use crate::js_precollect::{
    DeferredJsCollect, PendingJsData, PrecollectedJsData, collect_js_data, collect_js_shift_data,
};
use crate::relayout::{compute_layout_with_counters, defer_js_push_override, m4_swap_override};

impl Lumen {
    /// ADR-016 M2.2: post-layout UI-thread work shared by the synchronous
    /// [`Self::relayout`] and the off-thread commit path
    /// ([`Self::poll_engine_commit`]). Takes an already-computed
    /// `(DisplayList, LayoutBox)` (built either inline or on the engine thread)
    /// and applies everything that touches `&mut self`: caches, transitions /
    /// `@starting-style` sync, `will-change` layer promotion, zoom-preview reset,
    /// scroll clamping and JS-observer delivery. Kept identical for both callers
    /// so an off-thread relayout is byte-for-byte equivalent to a synchronous one.
    ///
    /// `defer_js_push` (BUG-935 S17): when `true`, the JS-observer push below
    /// (rects/styles/`deliver_layout_observers`/lazy-images/scroll-states) is
    /// issued as a fire-and-forget engine-thread [`route_task_js`] instead of
    /// the blocking [`route_query_js`] — see the field doc on
    /// [`crate::lumen::state::Lumen::pending_lazy_image_reqs`] for why lazy-image
    /// requests still reach [`Self::fetch_and_register_lazy_images`] despite the
    /// caller not getting them back synchronously.
    /// [`Self::try_relayout_raf_incremental`] passes `true` since S17 — BUG-935
    /// S15 found that path calling the blocking form lands *behind* the
    /// still-running rAF JS turn that dirtied the DOM (same ordered
    /// engine-thread FIFO), stalling the UI thread for as long as that turn's
    /// synchronous network calls take. [`Self::relayout`] and
    /// [`Self::poll_engine_commit`] pass `true` since S27 for the same reason
    /// (S23/S24 measured the latter as the dominant contributor to the
    /// `QUERY_TIMEOUT` tax on every subsequent frame's `route_query_js`) — S25
    /// found neither reads the push result synchronously and S26 added an
    /// independent drain for `pending_lazy_image_reqs` so the "next producer
    /// picks it up" assumption no longer has to hold. [`Self::readback_relayout_job`]
    /// still passes `false`, but is dead in production (S25 finding A: its
    /// sole caller is gated on the engine thread being *off*, and the function
    /// itself returns early on that same condition) — not converted, since
    /// there is no live path to measure or regress.
    pub(crate) fn apply_relayout_result(
        &mut self,
        mut new_dl: DisplayList,
        lb: lumen_layout::LayoutBox,
        viewport: Size,
        defer_js_push: bool,
        #[cfg(feature = "v8")] precollected: Option<PrecollectedJsData>,
    ) {
        // BUG-1634: текст страницы мог измениться — отложенные поднаборы `unicode-range`
        // с появившимися символами уходят в загрузку.
        if !self.deferred_web_fonts.is_empty() {
            self.spawn_web_font_fetches(Vec::new());
        }
        // BUG-935 S37: per-phase timing across the whole function, gated by the
        // same `LUMEN_FRAME_LOG` flag as `apply_ms` (the caller-side aggregate
        // S15/S36 already log). S36 found `apply_ms` alone accounts for 65-93ms
        // per tick with nothing inside this function broken down further (the
        // only existing breakdown, S33's `task-step` macro, times the deferred
        // JS-push closure, not the synchronous prefix that runs before it) — this
        // macro narrows the search to which phase of THIS function dominates.
        let step_log = lumen_paint::frame_log_enabled();
        macro_rules! apply_step {
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
        // BUG-480 срез 13: контентный вьюпорт под-документов следует за
        // размером их host-бокса — значит за каждым relayout (ресайз, зум,
        // любое движение вёрстки над фреймом). Проход сам гейтится на
        // «размер не менялся» и на пустом списке фреймов стоит ноль. ДО
        // заимствования `layout_source`: там берётся `&self` на всю функцию.
        apply_step!("frame_sync", {
            let frame_state = self.frame_interactive();
            crate::frames::sync_frame_viewports(&mut self.frames, &lb, frame_state);
            // FRAME-5 срез 2: fetch+register whatever lazy `<img>` just entered a
            // frame's own proximity margin — a mere relayout has no page-commit
            // step to piggy-back on, unlike the initial load (`page_pipeline.rs`).
            self.register_frame_lazy_images();
        });
        let Some(src) = self.layout_source.as_ref() else { return };
        (self.content_height, self.content_width) = crate::display_list_metrics::page_content_extent(&new_dl, &lb, self.viewport_width_css());
        // BUG-480 срез 14: вклейка ПОСЛЕ метрик (содержимое фрейма не должно
        // растягивать прокрутку страницы — обе функции складывают плоский
        // список прямоугольников и клипов не видят) и ДО diff/кэша, чтобы обе
        // стороны сравнения были одинаково склеенными.
        apply_step!("dl_splice_diff_cache", {
            crate::frames::splice_frame_content(&mut new_dl, &self.frames);
            // THREAD-7: the per-command `tile_grid` diff and the full-DL clone into
            // `display_list_cache` used to run here (~18 ms per tick on ria.ru).
            // Neither has a reader — nothing calls `TileGrid::is_dirty`/`dirty_tiles`
            // or `DisplayListCache::get` — so both were pure UI-thread cost.
            // Only the (much rarer) scroll/form patch paths still feed them.
        });
        // Поля пишутся напрямую (не через `set_display_list`): `layout_source`
        // здесь заимствован, `&mut self` целиком взять нельзя.
        self.display_list = new_dl;
        self.display_list_epoch = next_dl_epoch(self.display_list_epoch);
        // Sync transitions: compare prev styles with new layout before replacing.
        let now_s = self.epoch.elapsed().as_secs_f32();
        let mut new_styles = crate::layout_walk::StyleMap::default();
        apply_step!("transitions_sync", {
            collect_box_styles(&lb, &mut new_styles);
            for (node, new_style) in &new_styles {
                if let Some(old_style) = self.prev_styles.get(node) {
                    self.transition_events.extend(
                        self.transition_scheduler.sync(*node, old_style, new_style, now_s),
                    );
                }
            }
        });
        // GAP-CSSANIM срез 7: a node absent from this pass's layout tree (removed
        // from the DOM, or stopped generating a box via `display: none`) never
        // gets another `sync()` call, so any transition still `active` on it would
        // otherwise leak forever and never fire `transitioncancel` (CSS Transitions
        // L1 §3). `new_styles` already reflects exactly the current tree.
        apply_step!("transitions_cancel_missing", {
            self.transition_events.extend(
                self.transition_scheduler.cancel_missing(&new_styles, now_s),
            );
        });
        // @starting-style (CSS Transitions L2 §3.4): newly visible nodes (not in
        // prev_styles) use @starting-style rules as the before-change style so that
        // entry transitions start from the declared starting values.
        apply_step!("starting_style", {
            if !src.stylesheet.starting_style_rules.is_empty() {
                let entering: Vec<NodeId> = new_styles
                    .keys()
                    .filter(|n| !self.prev_styles.contains_key(*n))
                    .copied()
                    .collect();
                if !entering.is_empty() {
                    let mut entry_styles: Vec<(NodeId, ComputedStyle)> = Vec::new();
                    if let Ok(doc) = src.document.lock() {
                        for node in &entering {
                            if let Some(decls) =
                                resolve_starting_style(*node, &doc, &src.stylesheet)
                            {
                                entry_styles.push((
                                    *node,
                                    compute_style_from_declarations(&decls, viewport),
                                ));
                            }
                        }
                    }
                    // MutexGuard dropped — apply entry transitions outside the lock.
                    for (node, starting_style) in &entry_styles {
                        if let Some(new_style) = new_styles.get(node) {
                            self.transition_events.extend(self.transition_scheduler.sync(
                                *node,
                                starting_style,
                                new_style,
                                now_s,
                            ));
                        }
                    }
                }
            }
        });
        self.prev_styles = new_styles;
        // CSSOM-7 (BUG-977): grab the owned `Arc` now, while `src` (an
        // immutable borrow of `self.layout_source`) is still alive — every
        // `&mut self` method call below (`refresh_cv_state` etc.) would
        // otherwise conflict with holding `src` until the JS-push closure
        // near the end of this function.
        #[cfg(feature = "v8")]
        let stylesheet_for_flush = Arc::clone(&src.stylesheet);
        // GAP-VVPORT срез 3: same reason as `stylesheet_for_flush` above — grab
        // this now, while `src` is still alive, so it can be pushed to JS
        // alongside `zoom_factor` further down without re-borrowing
        // `self.layout_source` past the `&mut self` calls in between.
        let meta_viewport_scale = meta_initial_scale(src);
        // BUG-341 S7: invalidate the restyle-cascade cache by default — every
        // producer routes through here, but only `try_relayout_raf_incremental`'s
        // restyle sub-path knows how to recompute a cache that actually matches
        // `lb`, and re-validates it right after this call returns. Every other
        // producer (full `relayout()`, `readback_relayout_job`,
        // `poll_engine_commit`) leaves it `None`, forcing the next incremental
        // attempt onto the safe full-cascade-plus-graft fallback for one cycle.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(lb);
        apply_step!("cv_snap_scroll_state", {
            self.refresh_cv_state();
            // Promote nodes with will-change: transform/opacity/filter to GPU layers so
            // animation ticks can update only the layer matrix, bypassing relayout.
            // CSS: will-change — P4 wires ComputedStyle.will_change to promote_layer calls here.
            if let (Some(lb_ref), Some(r)) = (self.layout_box.as_ref(), self.renderer.as_mut()) {
                promote_will_change_layers(lb_ref, r.as_mut());
            }
            // ADR-016 M0.3: the fresh display list is now laid out at the current
            // zoom, so any transform-first zoom preview is complete — clear the
            // debounce and reset the backend to 1:1. Done for every relayout
            // (resize, DOM mutation, tab switch), not just the debounced zoom one,
            // so a relayout from another source also lands the pending zoom.
            self.laid_out_zoom_factor = self.zoom_factor;
            self.pending_zoom_relayout = None;
            if let Some(r) = self.renderer.as_mut() {
                r.set_preview_scale(1.0);
            }
            self.update_snap_containers();
            self.update_scroll_containers();
        });
        // GAP-CSSANIM срез 9 (correction): do NOT clear `animation_scheduler`
        // here either, for the same reason the comment below already gives
        // `transition_scheduler` — a running `@keyframes` animation must
        // survive relayout (viewport resize, DOM mutations), not restart
        // from t=0. Before this slice no relayout was ever triggered *by* an
        // active `@keyframes` animation itself, so this unconditionally-run
        // `clear()` went unnoticed; srez 9's height-geometry sync relayout
        // (`pump_raf_engine_thread`/`RedrawRequested` Step 4) fires every
        // tick while a height animation runs, so the reset happened on
        // every single frame, permanently pinning the interpolated height
        // near 0 — `AnimationScheduler::tick`'s own per-tick "stale"
        // cleanup (nodes no longer visited get cancelled) already handles
        // removal when a node's `animation-name` actually changes or the
        // node leaves the tree; a blanket `clear()` on top of that was
        // redundant even before this slice, just never exercised. Reset on
        // navigation still happens via `apply_loaded_page`.
        self.anim_frame = None;
        self.scroll_y = clamp_scroll(self.scroll_y, self.max_scroll());
        self.scroll_x = clamp_scroll(self.scroll_x, self.max_scroll_x());
        // Notify JS observers about the new layout geometry (ResizeObserver /
        // IntersectionObserver / getBoundingClientRect).
        #[cfg(feature = "v8")]
        {
            // Lazy-load requests drained while `self` is borrowed immutably;
            // fetched after the borrow ends (fetch needs `&mut self`).
            let mut lazy_reqs: Vec<(u32, String)> = Vec::new();
            // ADR-016 M2.2c-2d: layout-geometry push (`update_layout_rects` и Co.)
            // is the last mixed read+write UI→JS site in the relayout path. The
            // whole ordered sequence — rects/styles/viewport push → observer &
            // matchMedia & lazy-image delivery → `take_lazy_image_requests` read →
            // scroll-state push — moves into ONE `route_query_js` closure returning
            // `lazy_reqs`, so under the flag it runs atomically **in order** on the
            // engine thread (the value read after the void pushes keeps its
            // read-after-write ordering) and blocks only for that one result. The
            // `self.js_present` gate mirrors the old `if let Some(js)` — the
            // (side-effect-free) geometry collection runs only when a JS context
            // exists, byte-identical with the flag off. All captured data is owned
            // (`HashMap`/`Vec`) → the closure is `Send + 'static`.
            // BUG-935 S41: `precollected` (from `make_relayout_job` on the
            // engine thread) supersedes computing this inline — S37-S40 found
            // this collection the dominant cost of `apply_relayout_result`,
            // already paid on the UI thread today regardless of routing. The
            // fully synchronous `relayout()` has no engine-thread job to
            // piggy-back on, so it still falls back to the inline path below,
            // byte-identical to before this slice.
            // BUG-935 S51: on a deferred push with nothing precollected (the
            // M4 incremental path, the synchronous `relayout()`), only the
            // cheap layout-shift half runs here; the tree walks move into the
            // deferred engine task below — see `DeferredJsCollect`. A blocking
            // push (`defer_js_push=false`) still collects inline: its caller
            // gets the result back synchronously anyway.
            let pending = if self.js_present {
                match precollected {
                    Some(mut data) => {
                        self.prev_layout_shift_rects = std::mem::take(&mut data.next_layout_shift_baseline);
                        Some(PendingJsData::Ready(Box::new(data)))
                    }
                    None => (|| {
                        let pseudo_styles_needed = self
                            .pseudo_styles_needed_flag
                            .as_ref()
                            .map(|f| f.load(std::sync::atomic::Ordering::Relaxed))
                            .unwrap_or(true);
                        let custom_props_needed = self
                            .custom_props_needed_flag
                            .as_ref()
                            .map(|f| f.load(std::sync::atomic::Ordering::Relaxed))
                            .unwrap_or(true);
                        let computed_styles_needed = self
                            .computed_styles_needed_flag
                            .as_ref()
                            .map(|f| f.load(std::sync::atomic::Ordering::Relaxed))
                            .unwrap_or(true);
                        let lb_ref = self.layout_box.as_ref()?;
                        let document = Arc::clone(&self.layout_source.as_ref()?.document);
                        if defer_js_push {
                            let mut shift = apply_step!(
                                "js_shift_collect",
                                collect_js_shift_data(
                                    lb_ref,
                                    viewport,
                                    &self.prev_layout_shift_rects,
                                    self.last_input_epoch_s,
                                    now_s,
                                )
                            );
                            let hit_test_tree = apply_step!("clone_hit_test_tree", Arc::new(lb_ref.clone()));
                            self.prev_layout_shift_rects = std::mem::take(&mut shift.next_baseline);
                            return Some(PendingJsData::Deferred(DeferredJsCollect {
                                hit_test_tree,
                                document,
                                viewport,
                                shift,
                                pseudo_styles_needed,
                                custom_props_needed,
                                computed_styles_needed,
                            }));
                        }
                        let doc_guard = document.lock().ok()?;
                        let mut data = apply_step!(
                            "js_geometry_collect",
                            collect_js_data(
                                lb_ref,
                                doc_guard,
                                viewport,
                                &self.prev_layout_shift_rects,
                                self.last_input_epoch_s,
                                now_s,
                                pseudo_styles_needed,
                                custom_props_needed,
                                computed_styles_needed,
                            )
                        );
                        self.prev_layout_shift_rects = std::mem::take(&mut data.next_layout_shift_baseline);
                        Some(PendingJsData::Ready(Box::new(data)))
                    })(),
                }
            } else {
                None
            };
            if let Some(pending) = pending {
                let (vw, vh) = (viewport.width, viewport.height);
                let zoom_factor = self.zoom_factor;
                let dark_mode = self.dark_mode;
                let reduced_motion = self.effective_reduced_motion();
                let forced_colors_pref = self.a11y_store.forced_colors();
                // CSSOM-7 (BUG-977): push the live cascade alongside the rest
                // of this snapshot, same `Arc` `apply_relayout_result` already
                // laid out against (`relayout_page`/`compute_layout` above) —
                // feeds `FlushHandles::stylesheet` on the JS thread so the
                // same-tick flush (CSSOM-4/BUG-493) stops being a permanent
                // no-op in the interactive shell.
                let stylesheet = stylesheet_for_flush;
                // BUG-935 S17: `defer_js_push` splits the two callers that used
                // to share this one blocking `route_query_js` call.
                // `try_relayout_raf_incremental` (the only `true` caller) fires
                // it as a non-blocking `route_task_js` instead — S15 measured
                // the blocking form costing multiple seconds when it lands
                // behind the still-running rAF JS turn that dirtied the DOM on
                // the same ordered engine-thread FIFO. `take_lazy_image_requests`'s
                // result is therefore not available synchronously here; the task
                // stashes it in `pending_lazy_image_reqs` for the drain below
                // (this call's or a later producer's).
                if defer_js_push {
                    let reqs_slot = Arc::clone(&self.pending_lazy_image_reqs);
                    // BUG-935 S33: per-step timing inside the `route_task_js`
                    // closure — S32 found the whole `[engine] task` (this
                    // closure) taking up to 6.9s while the JS-side callback
                    // it triggers (`busyWork` in `deliver_layout_observers`)
                    // measured only 5-9ms from inside, a 1000-6900x gap that
                    // must live in one of the steps below or in V8-isolate
                    // entry/exit surrounding them. Only gated by
                    // `LUMEN_FRAME_LOG` (same flag as `engine_t0` above), so
                    // an ordinary run pays nothing.
                    let inner_step_log = lumen_paint::frame_log_enabled();
                    apply_step!("js_push_spawn_nonblocking", {
                    route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                        macro_rules! timed_step {
                            ($label:literal, $expr:expr) => {{
                                let t0 = inner_step_log.then(std::time::Instant::now);
                                let result = $expr;
                                if let Some(t0) = t0 {
                                    let ms = t0.elapsed().as_secs_f32() * 1000.0;
                                    eprintln!("[engine] task-step {ms:.2}ms ({})", $label);
                                }
                                result
                            }};
                        }
                        // BUG-935 S51: the tree walks run here, on the engine
                        // thread, when the producer had nothing precollected.
                        let Some(PrecollectedJsData {
                            rects,
                            layout_shift_score,
                            layout_shift_sources,
                            had_input,
                            client_rects,
                            hit_test_tree,
                            styles,
                            pseudo_styles,
                            customs,
                            scroll_states,
                            next_layout_shift_baseline: _,
                        }) = timed_step!("js_geometry_collect_deferred", pending.resolve())
                        else {
                            return;
                        };
                        timed_step!("update_layout_rects", js.update_layout_rects(rects));
                        timed_step!("update_client_rects", js.update_client_rects(client_rects));
                        timed_step!("update_hit_test_tree", js.update_hit_test_tree(hit_test_tree));
                        // BUG-935 S44: `None` means the collector was gated
                        // off this tick — skip the push entirely rather than
                        // replace a possibly-fresher cache with an empty map
                        // (see `PrecollectedJsData::styles` doc comment).
                        if let Some(styles) = styles {
                            timed_step!("update_computed_styles", js.update_computed_styles(styles));
                        }
                        if let Some(pseudo_styles) = pseudo_styles {
                            timed_step!("update_pseudo_computed_styles", js.update_pseudo_computed_styles(pseudo_styles));
                        }
                        if let Some(customs) = customs {
                            timed_step!("update_custom_properties", js.update_custom_properties(customs));
                        }
                        timed_step!("update_stylesheet", js.update_stylesheet(stylesheet));
                        timed_step!("update_viewport_size", js.update_viewport_size(vw, vh));
                        timed_step!("update_zoom_factor", js.update_zoom_factor(zoom_factor));
                        timed_step!(
                            "update_meta_viewport_scale",
                            js.update_meta_viewport_scale(meta_viewport_scale)
                        );
                        timed_step!("deliver_layout_observers", js.deliver_layout_observers());
                        if layout_shift_score > 0.0 {
                            timed_step!(
                                "deliver_layout_shift",
                                js.deliver_layout_shift(layout_shift_score, &layout_shift_sources, had_input)
                            );
                        }
                        timed_step!(
                            "deliver_media_query_changes",
                            js.deliver_media_query_changes(vw, vh, dark_mode, reduced_motion, forced_colors_pref)
                        );
                        timed_step!("deliver_lazy_images", js.deliver_lazy_images());
                        let reqs = timed_step!("take_lazy_image_requests", js.take_lazy_image_requests());
                        timed_step!("update_scroll_states", js.update_scroll_states(scroll_states));
                        if !reqs.is_empty()
                            && let Ok(mut slot) = reqs_slot.lock()
                        {
                            slot.extend(reqs);
                        }
                    });
                    });
                } else if let Some(PrecollectedJsData {
                    rects,
                    layout_shift_score,
                    layout_shift_sources,
                    had_input,
                    client_rects,
                    hit_test_tree,
                    styles,
                    pseudo_styles,
                    customs,
                    scroll_states,
                    next_layout_shift_baseline: _,
                }) = pending.resolve()
                {
                    lazy_reqs = apply_step!("js_push_blocking", route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                        js.update_layout_rects(rects);
                        js.update_client_rects(client_rects);
                        js.update_hit_test_tree(hit_test_tree);
                        // BUG-935 S44: see the sibling `defer_js_push` branch
                        // above — skip rather than overwrite with an empty map.
                        if let Some(styles) = styles {
                            js.update_computed_styles(styles);
                        }
                        if let Some(pseudo_styles) = pseudo_styles {
                            js.update_pseudo_computed_styles(pseudo_styles);
                        }
                        if let Some(customs) = customs {
                            js.update_custom_properties(customs);
                        }
                        js.update_stylesheet(stylesheet);
                        js.update_viewport_size(vw, vh);
                        js.update_zoom_factor(zoom_factor);
                        js.update_meta_viewport_scale(meta_viewport_scale);
                        js.deliver_layout_observers();
                        if layout_shift_score > 0.0 {
                            js.deliver_layout_shift(layout_shift_score, &layout_shift_sources, had_input);
                        }
                        // CSS MQ L4 §4.2: re-evaluate matchMedia() lists against the new
                        // viewport. `dark_mode` mirrors the OS `prefers-color-scheme`,
                        // read from winit at window creation / refreshed on ThemeChanged.
                        js.deliver_media_query_changes(vw, vh, dark_mode, reduced_motion, forced_colors_pref);
                        // After fresh rects are in JS: fire lazy-load proximity check.
                        // Images that entered the viewport+margin are queued by JS via
                        // _lumen_request_lazy_image_load; we drain and fetch them below.
                        js.deliver_lazy_images();
                        let reqs = js.take_lazy_image_requests();
                        js.update_scroll_states(scroll_states);
                        reqs
                    })
                    .unwrap_or_default());
                }
            }
            // BUG-935 S17: pick up whatever a *previous* deferred push (this
            // call's own, if `defer_js_push` above, or an earlier one) already
            // stashed — every producer routes through here, so this always
            // catches up within one relayout regardless of which producer runs
            // next. A no-op (empty lock, immediately dropped) on every build
            // that never takes the `defer_js_push` branch.
            if let Ok(mut slot) = self.pending_lazy_image_reqs.lock() {
                lazy_reqs.append(&mut slot);
            }
            if !lazy_reqs.is_empty() {
                self.fetch_and_register_lazy_images(lazy_reqs);
            }
        }
        // BUG-730: images the page added after load land here — this is the one
        // post-layout point every relayout producer routes through, so a
        // script-appended `<img>` is picked up whichever path relaid it out.
        self.spawn_dynamic_image_loads(viewport);
        // BUG-939: same point for `background-image` set/changed from JS —
        // `fetch_and_decode_background_images` in the initial pipeline only
        // runs once and never sees a later cascade mutation.
        self.spawn_dynamic_background_image_loads();
        // BUG-735: и по той же причине — свежеперестроенное поддерево могло
        // принести НОВЫЙ `<img>` с уже декодированным `src` (React перерисовал
        // блок: узел другой, картинка та же). Второго `ImageDecoded` для него не
        // будет — запрос дедуплицирован по URL, — поэтому размеры ему раздаёт
        // проход `apply_stream_intrinsic_sizes`, и здесь мы его заказываем.
        // Пустой карте заказывать нечего; сам проход no-op, если дописывать
        // нечего, так что «релейаут → проход → релейаут» не зацикливается.
        // BUG-1048: тот же аргумент для decode-неудач — новый узел мог принять
        // URL, чей фетч уже провалился раньше, и без этого никогда не узнал бы.
        self.stream_image_sizes_dirty |=
            !self.stream_image_sizes.is_empty() || !self.stream_image_errors.is_empty();
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// ADR-016 M2.2: build the immutable-snapshot relayout job that the engine
    /// thread runs off the UI thread — shared by the fire-and-forget
    /// [`Self::submit_relayout_job`] (latest-wins) and the blocking
    /// [`Self::readback_relayout_job`] (request/reply), so both produce a
    /// byte-identical [`EngineCommit`] for the same DOM state.
    ///
    /// Returns `None` — nothing to lay out — when there is no `LayoutSource`/renderer
    /// or the viewport is degenerate. On success bumps `engine_job_generation` and
    /// returns `(generation, job)`; the caller decides whether to `submit` it
    /// (deferred, latest-wins) or `readback` it (blocking). Because the generation is
    /// bumped here, callers must gate on the engine thread being present **before**
    /// calling this (both wrappers do) so a flag-off run never advances the counter.
    ///
    /// The job captures immutable `Arc` snapshots of the document + stylesheet +
    /// web-fonts (invariant 1) and re-establishes the interactive/forced-colors/
    /// content-visibility thread-local state **on the engine thread** before
    /// computing layout.
    pub(crate) fn make_relayout_job(
        &mut self,
    ) -> Option<(u64, impl FnOnce() -> EngineCommit + Send + 'static)> {
        let viewport = self.relayout_viewport()?;
        // BUG-743: снимок листа для движкового потока берётся здесь, поэтому
        // поздний динамический `<style>` должен попасть в каскад до клонирования.
        self.refresh_dynamic_css();
        let src = self.layout_source.as_ref()?;
        self.engine_job_generation = self.engine_job_generation.wrapping_add(1);
        let generation = self.engine_job_generation;
        // Immutable snapshots captured by the job (ADR-016 invariant 1). The
        // stylesheet is now an `Arc` in `LayoutSource` (M2.2b), so the job clones
        // only the handle — no per-submit deep clone of the whole `Stylesheet`.
        let document = Arc::clone(&src.document);
        let stylesheet = Arc::clone(&src.stylesheet);
        let hp = Arc::clone(&self.hyp_provider);
        let web_fonts = self.web_fonts.clone();
        let dark_mode = self.dark_mode;
        let hovered = self.hovered_nid;
        let focused = self.focused_node;
        let active = self.active_nid;
        let forced_colors = self.a11y_store.forced_colors();
        let device_pixel_ratio = self.zoom_factor;
        let (cv_x, cv_y) = (self.scroll_x, self.scroll_y);
        let cv_relevant = self.cv_relevant.clone();
        // BUG-935 S80: стили каскада нужны только свопу — без него
        // `page_prev_cascade_styles` всё равно обнулялся бы после коммита.
        let want_cascade_styles = m4_swap_override();
        // GAP-CSSANIM срез 9: see `animated_heights_snapshot` doc comment —
        // same thread-local handoff as interactive state/forced-colors above,
        // captured on the UI thread and installed on the engine thread.
        let animated_heights = self.animated_heights_snapshot();
        // BUG-935 S41: snapshots for the JS-geometry collection this job now
        // also runs (see `precollected` below) — same rationale as the
        // interactive-state snapshots above, just for `collect_js_data`'s
        // inputs instead of layout's.
        #[cfg(feature = "v8")]
        let js_present = self.js_present;
        #[cfg(feature = "v8")]
        let prev_layout_shift_rects = self.prev_layout_shift_rects.clone();
        #[cfg(feature = "v8")]
        let last_input_epoch_s = self.last_input_epoch_s;
        #[cfg(feature = "v8")]
        let epoch = self.epoch;
        // BUG-935 S43: snapshot the two "cache actually needed" flags on this
        // (UI) thread, same rationale as `js_present`/`prev_layout_shift_rects`
        // above — the closure below runs on the engine thread and must not
        // touch `self`.
        #[cfg(feature = "v8")]
        let pseudo_styles_needed = self
            .pseudo_styles_needed_flag
            .as_ref()
            .map(|f| f.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(true);
        #[cfg(feature = "v8")]
        let custom_props_needed = self
            .custom_props_needed_flag
            .as_ref()
            .map(|f| f.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(true);
        #[cfg(feature = "v8")]
        let computed_styles_needed = self
            .computed_styles_needed_flag
            .as_ref()
            .map(|f| f.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(true);
        let job = move || {
            let t0 = std::time::Instant::now();
            // Interactive state is thread-local — set it on THIS (engine) thread.
            lumen_layout::set_interactive_state(hovered, focused, active);
            lumen_layout::set_forced_colors(forced_colors);
            lumen_layout::set_device_pixel_ratio(device_pixel_ratio);
            lumen_layout::set_cv_scroll(cv_x, cv_y);
            lumen_layout::set_cv_relevant(cv_relevant);
            lumen_layout::set_animated_heights(animated_heights);
            let (content, layout_box, counters) =
                compute_layout_with_counters(&document, &stylesheet, viewport, &*hp, dark_mode, &web_fonts);
            let cascade_styles = want_cascade_styles.then(|| counters.into_styles());
            lumen_layout::clear_interactive_state();
            lumen_layout::clear_animated_heights();
            lumen_layout::set_cv_scroll(0.0, 0.0);
            lumen_layout::set_cv_relevant(std::collections::HashSet::new());
            // `compute_ms` keeps meaning exactly what it did before this
            // slice — pure style+layout+DL cost — so it stays untouched by
            // the JS-geometry collection below; that work gets its own
            // `LUMEN_FRAME_LOG` step timers inside `collect_js_data`.
            let compute_ms = t0.elapsed().as_secs_f32() * 1000.0;
            // BUG-935 S41: run `apply_relayout_result`'s JS-geometry
            // collection here instead of leaving it for the UI thread —
            // S37-S40 found it the dominant UI-thread cost of every commit,
            // off-thread routing or not. `None` when there is no JS context
            // yet or the document lock is unavailable (poisoned); the UI
            // side then falls back to its own inline collection.
            #[cfg(feature = "v8")]
            let precollected = if js_present {
                document.lock().ok().map(|doc_guard| {
                    let now_s = epoch.elapsed().as_secs_f32();
                    collect_js_data(
                        &layout_box,
                        doc_guard,
                        viewport,
                        &prev_layout_shift_rects,
                        last_input_epoch_s,
                        now_s,
                        pseudo_styles_needed,
                        custom_props_needed,
                        computed_styles_needed,
                    )
                })
            } else {
                None
            };
            EngineCommit {
                content,
                layout_box,
                viewport,
                generation,
                compute_ms,
                cascade_styles,
                interactive: (hovered, focused, active),
                #[cfg(feature = "v8")]
                precollected,
            }
        };
        Some((generation, job))
    }

    /// ADR-016 M2.2: route a relayout to the persistent engine thread (off the
    /// UI thread). Returns `true` if a job was submitted; `false` when the engine
    /// thread is absent (`LUMEN_ENGINE_THREAD` off) or there is nothing to lay out
    /// — the caller then falls back to the synchronous [`Self::relayout`].
    ///
    /// Only for **async-safe** triggers: no caller may read layout geometry
    /// synchronously after this returns, because the commit lands a few frames
    /// later via [`Self::poll_engine_commit`]. Callers are the debounced
    /// transform-first zoom (M0.3), the chrome-inset toggles ([`Self::relayout_chrome`],
    /// M2.2b), the form-input toggles ([`Self::relayout_form`], M2.2c-3) and the
    /// `about_to_wait` rAF DOM-dirty flush ([`Self::relayout_raf_dirty`], M2.2c-3) —
    /// none reads geometry synchronously afterward.
    pub(crate) fn submit_relayout_job(&mut self) -> bool {
        if self.engine_thread.is_none() {
            return false;
        }
        let Some((generation, job)) = self.make_relayout_job() else { return false };
        let Some(engine) = self.engine_thread.as_ref() else { return false };
        engine.submit(generation, job);
        true
    }

    /// ADR-016 M2.2c-3: run a relayout **on the engine thread but block** for its
    /// commit (request/reply via [`engine_thread::EngineThread::readback`]), then
    /// apply it synchronously — for sites that read a layout product in the same
    /// tick. Returns `true` if the readback ran and was applied; `false` when the
    /// engine thread is absent (`LUMEN_ENGINE_THREAD` off), there is nothing to lay
    /// out, or the thread was shutting down (`readback` → `None`) — the caller then
    /// falls back to the synchronous [`Self::relayout`].
    ///
    /// The sole caller today is the `RedrawRequested` rAF DOM-dirty flush
    /// ([`Self::relayout_raf_dirty_readback`]), whose next step reads
    /// `self.display_list.is_empty()` for PerformancePaintTiming. Unlike
    /// [`Self::submit_relayout_job`] the commit is **not** deposited in the
    /// latest-wins slot; it comes straight back and is applied here, so like the
    /// synchronous [`Self::relayout`] this is authoritative — `engine_applied_generation`
    /// advances to the just-bumped `engine_job_generation`, dropping any older
    /// in-flight async commit in [`Self::poll_engine_commit`]'s guard.
    pub(crate) fn readback_relayout_job(&mut self) -> bool {
        if self.engine_thread.is_none() {
            return false;
        }
        let Some((_generation, job)) = self.make_relayout_job() else { return false };
        let Some(engine) = self.engine_thread.as_ref() else { return false };
        let Some(commit) = engine.readback(job) else { return false };
        // Authoritative like `relayout()`: mark the just-bumped generation applied so
        // a stale in-flight async commit is dropped by `poll_engine_commit`.
        self.engine_applied_generation = self.engine_job_generation;
        #[cfg(feature = "v8")]
        let precollected = commit.precollected;
        let EngineCommit { content, layout_box, viewport, compute_ms, .. } = commit;
        self.apply_relayout_result(
            content,
            layout_box,
            viewport,
            false,
            #[cfg(feature = "v8")]
            precollected,
        );
        if lumen_paint::frame_log_enabled() {
            self.engine_stats.record(compute_ms);
            eprintln!(
                "[engine] relayout {compute_ms:.2}ms (readback) dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
        true
    }

    /// ADR-016 M2.2: consume the newest off-thread layout result, if the engine
    /// thread produced one, and apply it on the UI thread. A no-op when the engine
    /// thread is off or nothing is ready. The commit is dropped when its
    /// `generation` no longer matches `engine_job_generation` — a newer job or a
    /// synchronous `relayout()` has superseded it (generation-guard, invariant 2).
    pub(crate) fn poll_engine_commit(&mut self) {
        // Take the commit and release the `engine_thread` borrow before the
        // `&mut self` apply below.
        let Some(commit) = self.engine_thread.as_ref().and_then(|e| e.take_committed()) else {
            return;
        };
        if commit.generation != self.engine_job_generation {
            return; // superseded — drop the stale result.
        }
        self.engine_applied_generation = commit.generation;
        #[cfg(feature = "v8")]
        let precollected = commit.precollected;
        let EngineCommit { content, layout_box, viewport, compute_ms, cascade_styles, interactive, .. } = commit;
        // BUG-935 S27: `defer_js_push=true` — S23/S24 measured this call site
        // (the `poll_engine_commit` off-thread commit path) as the dominant
        // contributor to the `QUERY_TIMEOUT` tax on the UI thread (S24's
        // `relayout.rs:1023`, 51.7s over just 2 calls in a 3-minute run). S25
        // confirmed no caller reads the JS-side push result synchronously
        // (finding B) and S26 added an independent drain for
        // `pending_lazy_image_reqs` so a push that never gets picked up by a
        // *next* `apply_relayout_result` still gets fetched (finding C).
        // BUG-935 S41: `precollected` now carries the same `js_geometry_collect`
        // work `make_relayout_job` already ran on the engine thread — this call
        // no longer redoes it on the UI thread.
        self.apply_relayout_result(
            content,
            layout_box,
            viewport,
            defer_js_push_override().unwrap_or(true),
            #[cfg(feature = "v8")]
            precollected,
        );
        // BUG-935 S80: `apply_relayout_result` обнулил кэш каскада; коммит принёс
        // стили того же прохода, по которому построено `layout_box` — возвращаем
        // их, и следующий инкрементальный тик идёт по дешёвой ветке (restyle=1).
        if let Some(styles) = cascade_styles {
            self.page_prev_cascade_styles = Some(styles);
            self.page_prev_interactive = interactive;
        }
        // ADR-016 M2.0/M2.2: record the off-thread compute cost. Unlike the
        // synchronous path this excludes the UI-thread apply (observers etc.),
        // and is tagged `(off-thread)` so the summary reflects the work moved off
        // the UI thread.
        self.engine_stats.record(compute_ms);
        // BUG-935 S86: off-thread задание — всегда полный проход; его стоимость
        // решает, можно ли следующий предсказанный полный тик взять on-thread.
        self.m4_full_cost_ms = Some(compute_ms);
        if lumen_paint::frame_log_enabled() {
            eprintln!(
                "[engine] relayout {compute_ms:.2}ms (off-thread) dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
    }
}
