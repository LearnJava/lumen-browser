//! Relayout pipeline of the shell: page reflow, the rAF turn, the off-thread
//! layout job (ADR-016) and ownership of the page's JS context handle.
//!
//! SPLIT-SH1 (2026-08-26): moved verbatim out of `main.rs`. Behaviour, order of
//! operations and method bodies are unchanged; only module path and visibility
//! (`fn` -> `pub(crate) fn`, required for a caller in the parent module) differ.

use crate::*;

impl Lumen {
    /// Заменяет display list страницы, бампая его версию (BUG-405 срез 39).
    ///
    /// Единственный способ присвоить [`Self::display_list`]: рендерер решает по
    /// версии, можно ли переиспользовать свёртку кадровых хэшей, поэтому запись
    /// мимо этого метода показала бы устаревшие пиксели.
    pub(crate) fn set_display_list(&mut self, mut dl: DisplayList) {
        // BUG-480 срез 14: содержимое под-документов фреймов вклеивается на
        // КАЖДОЙ записи списка, а не один раз на загрузку — список страницы
        // пересобирается из layout при любом relayout и о фреймах не знает.
        // Метрики (`content_height_of`/`content_width_of`) считаются
        // вызывающей стороной ДО этого места и остаются метриками СТРАНИЦЫ:
        // они складываются по плоскому списку прямоугольников, без клипов, а
        // содержимое фрейма выше его бокса прокручивать страницу не должно.
        crate::frames::splice_frame_content(&mut dl, &self.frames);
        self.display_list = dl;
        self.bump_display_list_epoch();
    }

    /// Бампает версию [`Self::display_list`] (BUG-405 срез 39).
    ///
    /// Отдельно от [`Self::set_display_list`] для трёх мест, где заимствования
    /// не дают взять `&mut self` целиком: правка списка на месте и два места,
    /// где `self.layout_source`/`self.layout_box` уже заняты — там поле пишется
    /// напрямую, а версия бампается этим вызовом рядом.
    pub(crate) fn bump_display_list_epoch(&mut self) {
        self.display_list_epoch = next_dl_epoch(self.display_list_epoch);
    }

    /// BUG-743: пересобрать каскад, если набор инлайновых `<style>` изменился
    /// с последней сборки. Возвращает `true`, если лист заменён.
    ///
    /// Таблица стилей страницы собирается один раз за навигацию — на этапе
    /// разбора, сразу после выполнения синхронных скриптов. Всё, что вставляет
    /// `<style>` позже (обработчик `load`, `setTimeout`, rAF, промис — то есть
    /// любой CSS-in-JS), до этого оставалось вне каскада навсегда. Здесь
    /// дешёвый отпечаток ([`inline_style_fingerprint`]) сверяется на каждом
    /// релейауте, а полная пересборка (склейка из [`DynamicCssBase`] + парс)
    /// происходит только когда блоки действительно изменились.
    ///
    /// Сеть не трогается: `@import` внутри *нового* листа останется
    /// неразрешённым, `@font-face` из него не подгрузится — релейаут не место
    /// для загрузок. Обычный CSS-in-JS ни того, ни другого не использует.
    pub(crate) fn refresh_dynamic_css(&mut self) -> bool {
        let Some(src) = self.layout_source.as_mut() else {
            return false;
        };
        // Раздельные заимствования полей: `document` читается, пока `stylesheet`
        // и `dynamic_css` держатся на запись.
        let LayoutSource { document, stylesheet, dynamic_css, .. } = src;
        let Some(base) = dynamic_css.as_mut() else {
            return false;
        };
        let Ok(doc) = document.lock() else {
            return false;
        };
        let fp = inline_style_fingerprint(&doc);
        if fp == base.inline_fp {
            return false;
        }
        let inline = extract_style_blocks(&doc);
        drop(doc);
        let mut css =
            String::with_capacity(base.imports_prefix.len() + inline.len() + base.linked.len());
        css.push_str(&base.imports_prefix);
        css.push_str(&inline);
        css.push_str(&base.linked);
        let sheet = lumen_css_parser::parse(&css);
        eprintln!(
            "CSS пересобран после правки <style>: {} правил",
            sheet.rules.len()
        );
        *stylesheet = Arc::new(sheet);
        base.inline_fp = fp;
        // Инкрементальный рестайл (BUG-341 S7) переиспользует стили прошлого
        // прохода — против нового листа они недействительны.
        self.page_prev_cascade_styles = None;
        true
    }

    /// Повторный layout+paint при изменении размера viewport.
    /// Использует сохранённый `LayoutSource`; парсинг не повторяется.
    pub(crate) fn relayout(&mut self) {
        self.refresh_dynamic_css();
        let Some(viewport) = self.relayout_viewport() else { return };
        // ADR-016 M2.2: a synchronous relayout is authoritative — advance the
        // applied generation to `job_generation` so any off-thread commit still
        // in flight (older generation) is dropped by `poll_engine_commit`'s
        // guard, and no poll-wakeup is armed for a job that no longer matters.
        self.engine_job_generation = self.engine_job_generation.wrapping_add(1);
        self.engine_applied_generation = self.engine_job_generation;
        // ADR-016 M2.0: time the whole UI-thread relayout (style + layout +
        // display-list build + JS-observer delivery) — the work M2 moves to an
        // engine thread. Only under `LUMEN_FRAME_LOG`, so a normal run pays
        // nothing. Recorded after `apply_relayout_result` so the state it reports
        // (display list / styled nodes) is the freshly-applied one.
        let engine_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
        let Some(src) = self.layout_source.as_ref() else { return };
        // Set interactive hover/focus/active state for this layout pass so that
        // :hover / :focus / :active / :focus-within CSS rules evaluate correctly.
        lumen_layout::set_interactive_state(self.hovered_nid, self.focused_node, self.active_nid);
        // Forced Colors Mode (CSS Color Adjust L1 §3) — a11y preference drives
        // the forced system palette and the `(forced-colors: active)` media
        // feature for this layout pass.
        lumen_layout::set_forced_colors(self.a11y_store.forced_colors());
        // content-visibility: auto (BB-4) — relevance-проверка против текущего
        // scroll-положения + ratchet-набора. Сброс к дефолтам после прохода,
        // чтобы layout других документов (sidebar, фоновый парс) не унаследовал
        // чужой scroll/relevant.
        lumen_layout::set_cv_scroll(self.scroll_x, self.scroll_y);
        lumen_layout::set_cv_relevant(self.cv_relevant.clone());
        let (new_dl, lb) = relayout_page(src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        lumen_layout::clear_interactive_state();
        lumen_layout::set_cv_scroll(0.0, 0.0);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        self.apply_relayout_result(new_dl, lb, viewport);
        if let Some(t0) = engine_t0 {
            let engine_ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.engine_stats.record(engine_ms);
            eprintln!(
                "[engine] relayout {engine_ms:.2}ms dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
    }

    /// ADR-016 M2.2b: route an **async-safe chrome-inset relayout** off the UI
    /// thread when the engine thread is enabled, falling back to the synchronous
    /// [`Self::relayout`] otherwise (the default, so behavior is byte-identical
    /// unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// "Async-safe" means the caller changed only *chrome* geometry — a docked
    /// panel's side/width, the workspace bar, vertical/tree tabs, sidebar
    /// visibility, the AI / accessibility side panels (M2.2b-3), or a mouse-click
    /// *close* of the AI / sidebar / accessibility panels (M2.2b-6) — or triggered a
    /// whole-page *restyle* with no geometry read of its own (an OS/settings theme
    /// flip, M2.2b-4; an interactive `:hover`/`:active` pseudo-class flip, M2.2b-5,
    /// including the `:hover` clear on cursor-leave, M2.2b-8; a `:focus`/`:focus-within`
    /// change from a JS focus request or a click, M2.2b-7; a web-font FOUT→FOIT swap,
    /// M2.2b-8) — or opened the web sidebar's error-placeholder panel (M2.2b-8) —
    /// and is in either case **not** followed by a synchronous read
    /// of page layout geometry. The reflowed content may
    /// therefore land a few frames later via [`Self::poll_engine_commit`], the
    /// same contract as the debounced zoom (M2.2a). The chrome itself is drawn
    /// from its own state, so it updates on the immediately-requested redraw; only
    /// the page reflow underneath it is deferred.
    pub(crate) fn relayout_chrome(&mut self) {
        if !self.submit_relayout_job() {
            self.relayout();
        }
    }

    /// ADR-016 M2.2c-3: route an **async-safe form-control DOM-mutation relayout**
    /// off the UI thread when the engine thread is enabled, falling back to the
    /// synchronous [`Self::relayout`] otherwise (the default, so behavior is
    /// byte-identical unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// "Async-safe" here means the caller already mutated the shared layout
    /// `Document` (a checkbox/radio `checked` flip, a `<details>` open toggle, a
    /// range-slider value change, …) directly on the UI thread and is **not**
    /// followed by a synchronous read of page layout geometry. The mutation is
    /// therefore visible in the immutable `Arc<Mutex<Document>>` snapshot the
    /// off-thread job captures, and the reflowed content lands a few frames later
    /// via [`Self::poll_engine_commit`] — the same contract as the debounced zoom
    /// (M2.2a) and the chrome-inset toggles ([`Self::relayout_chrome`], M2.2b).
    ///
    /// Sites that read geometry synchronously right after the mutation (caret
    /// placement, `scrollIntoView`, hit-test) cannot use this — they belong to the
    /// blocking-readback path (`EngineThread::readback`, M2.2c-1) instead.
    pub(crate) fn relayout_form(&mut self) {
        if !self.submit_relayout_job() {
            self.relayout();
        }
    }

    /// ADR-016 M4: incremental re-layout for rAF JS DOM mutations.
    ///
    /// Runs [`layout_mutation_incremental`] (full cascade + `graft_geometry` +
    /// incremental geometry pass + post-layout passes) reusing the retained
    /// `self.layout_box` as `prev`. Returns `true` on success and calls
    /// [`Self::apply_relayout_result`] (updates `self.display_list` /
    /// `self.layout_box` / scroll clamps). Returns `false` when no previous
    /// layout is available (first load) or when `layout_source` / viewport are
    /// not ready — the caller falls back to [`Self::relayout`].
    ///
    /// BUG-341 S7: when [`Self::page_prev_cascade_styles`] is `Some` (the last
    /// cycle to touch `self.layout_box` was this same restyle path) *and* the
    /// page-side JS DOM-mutation tracker ([`PersistentJs::take_dom_touched`])
    /// reports an attributed summary, this takes the incremental-cascade path
    /// ([`lumen_layout::box_tree::layout_mutation_incremental_restyle`])
    /// instead of the plain graft-only one — mirroring
    /// `Lumen::relayout_chrome_host`'s BUG-341 S6 wiring. `dirty_roots` unions
    /// the interactive-state delta (hover/focus/active, vs.
    /// `self.page_prev_interactive`) with the DOM-mutation delta
    /// (`touched.nodes`); `content_dirty` is `Nothing` only when `touched.nodes`
    /// is empty (a pure interactive-state cycle) and `Untracked` otherwise, the
    /// same precondition `RestyleDelta::content_dirty` documents. An `unattributed` summary
    /// (untracked mutation primitive — Shadow DOM attach, `execCommand`, …) or a
    /// missing/invalidated cache falls back to today's `layout_mutation_incremental`
    /// (full cascade, still correct, just without the cascade-skip win).
    ///
    /// `self.layout_box` is **moved out** (not cloned) to avoid copying the
    /// potentially large tree; `apply_relayout_result` moves the fresh tree
    /// back in, so field is always `Some` after a successful call.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn try_relayout_raf_incremental(&mut self) -> bool {
        let Some(viewport) = self.relayout_viewport() else {
            return false;
        };
        // BUG-743: смена таблицы стилей может задеть любой узел дерева —
        // геометрию прошлого прохода переиспользовать нельзя, пусть вызывающий
        // сделает полный [`Self::relayout`].
        if self.refresh_dynamic_css() {
            return false;
        }
        let Some(prev_lb) = self.layout_box.take() else {
            return false;
        };
        let Some(src) = self.layout_source.as_ref() else {
            self.layout_box = Some(prev_lb);
            return false;
        };
        self.engine_job_generation = self.engine_job_generation.wrapping_add(1);
        self.engine_applied_generation = self.engine_job_generation;
        lumen_layout::set_interactive_state(self.hovered_nid, self.focused_node, self.active_nid);
        lumen_layout::set_forced_colors(self.a11y_store.forced_colors());
        lumen_layout::set_cv_scroll(self.scroll_x, self.scroll_y);
        lumen_layout::set_cv_relevant(self.cv_relevant.clone());
        let new_interactive = (self.hovered_nid, self.focused_node, self.active_nid);
        let touched = self.js_ctx.as_ref().map(|js| js.take_dom_touched()).unwrap_or_default();
        // BUG-341 S19: the two paths are one `if`/`else` rather than an
        // `Option` plus a `match` because the restyle path now *consumes*
        // `prev_lb` (it moves the reusable subtrees straight into the fresh
        // tree instead of copying them), and only this shape lets the compiler
        // see that the fallback below runs exactly when the move did not.
        let (new_dl, new_lb, fresh_cascade_styles) = if !touched.unattributed
            && let Some(prev_styles) = self.page_prev_cascade_styles.take()
        {
            let (prev_hover, prev_focus, prev_active) = self.page_prev_interactive;
            let doc = src.document.lock().unwrap();
            // BUG-341 S7: computed once per pass, reused across all three axes.
            let state_index = lumen_layout::style::restyle_state_index(&doc, &src.stylesheet);
            let mut dirty_roots = std::collections::HashSet::new();
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                &doc, prev_hover, new_interactive.0, &state_index,
            ));
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                &doc, prev_focus, new_interactive.1, &state_index,
            ));
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                &doc, prev_active, new_interactive.2, &state_index,
            ));
            // BUG-341 S17: `DomTouched` records node ids without attribute
            // names, so every page-side mutation stays `Unattributed` — the
            // pre-S17 widen-to-parent behaviour, unchanged.
            let node_index = lumen_layout::style::restyle_node_index(&doc, &src.stylesheet);
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                &doc,
                touched.nodes.iter().map(|&n| (n, lumen_layout::style::NodeChange::Unattributed)),
                &node_index,
            ));
            drop(doc);
            // BUG-341 S16: the page-side tracker reports *selector-relevant*
            // nodes only (`DomTouched` deliberately says nothing about text
            // writes) and has an `unattributed` escape hatch, so it cannot
            // claim a complete per-node content record the way
            // `bind_model_tracked` can. Anything but "nothing touched at all"
            // must therefore stay `Untracked` — this is exactly S4's
            // `dom_content_stable` semantics, unchanged. Giving the page path a
            // real content set means completing `DomTouched` for content first.
            let content_dirty = if touched.nodes.is_empty() {
                lumen_layout::counters::ContentDirty::Nothing
            } else {
                lumen_layout::counters::ContentDirty::Untracked
            };
            let delta = lumen_layout::counters::RestyleDelta { prev_styles, dirty_roots, content_dirty };
            lumen_layout::counters::set_incremental_restyle(true);
            // BUG-341 S15 — see the twin call in `relayout_chrome_host`: the
            // box-build reuse rides on the same content precondition computed
            // just above.
            lumen_layout::box_tree::set_incremental_box_build(true);
            let (dl, lb, counters) = relayout_page_incremental_restyle(
                src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts, prev_lb, delta,
            );
            lumen_layout::box_tree::set_incremental_box_build(false);
            lumen_layout::counters::set_incremental_restyle(false);
            (dl, lb, Some(counters.into_styles()))
        } else {
            let (dl, lb) = relayout_page_incremental(
                src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts, &prev_lb,
            );
            (dl, lb, None)
        };
        lumen_layout::clear_interactive_state();
        lumen_layout::set_cv_scroll(0.0, 0.0);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        self.apply_relayout_result(new_dl, new_lb, viewport);
        // `apply_relayout_result` unconditionally clears the cache — restore it
        // here, after `lb` has already landed in `self.layout_box`, only when
        // this cycle actually produced a matching one.
        if let Some(styles) = fresh_cascade_styles {
            self.page_prev_cascade_styles = Some(styles);
            self.page_prev_interactive = new_interactive;
        }
        true
    }

    /// ADR-016 M2.2c-3: route the **async-safe rAF DOM-dirty flush** off the UI
    /// thread when the engine thread is enabled, falling back to the synchronous
    /// [`Self::relayout`] otherwise (the default, so behavior is byte-identical
    /// unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// This is the `about_to_wait` rAF pump: a `requestAnimationFrame` callback ran
    /// (engine-side under the flag) and mutated the DOM, so the shared
    /// `Arc<Mutex<Document>>` already carries the mutation the off-thread job's
    /// snapshot will observe (invariant 1). The caller only requests a redraw
    /// afterwards — it does **not** read page geometry synchronously — so the
    /// reflow may land a few frames later via [`Self::poll_engine_commit`], the same
    /// async contract as the debounced zoom (M2.2a) and the form-input toggles
    /// ([`Self::relayout_form`]). The `RedrawRequested` counterpart *does* read a
    /// layout product synchronously (Step 5 PerformancePaintTiming) and therefore
    /// uses the blocking [`Self::readback_relayout_job`] path instead.
    ///
    /// ADR-016 M4: when the engine thread is present (default since ADR-023),
    /// [`Self::submit_relayout_job`] (full, off-thread) wins and the incremental
    /// path below is never reached (BUG-935). In the single-thread fallback path,
    /// tries the incremental layout ([`Self::try_relayout_raf_incremental`])
    /// before the full [`Self::relayout`].
    pub(crate) fn relayout_raf_dirty(&mut self) {
        if !self.submit_relayout_job() && !self.try_relayout_raf_incremental() {
            self.relayout();
        }
    }

    /// ADR-016 M2.3: `true` while a `run_animation_frame` batch dispatched to the
    /// engine thread has not yet completed (engine thread present + inflight flag
    /// set). While inflight the UI thread must not enqueue new blocking JS work —
    /// it would serialize the winit thread behind the (possibly 200 ms) turn,
    /// freezing scroll. Always `false` off the flag (no engine thread).
    pub(crate) fn raf_turn_inflight(&self) -> bool {
        self.engine_thread.is_some()
            && self
                .raf_task_inflight
                .load(std::sync::atomic::Ordering::Acquire)
    }

    /// ADR-016 M2.3: consume (clear + return) the rAF-pending flag lock-free via
    /// the cached UI-side atomic. `false` when no flag is cached (JS-less tab /
    /// off the flag). No engine `query`, so it never blocks behind an in-flight
    /// turn — unlike [`route_query_js`]`(… take_raf_pending)`.
    pub(crate) fn take_raf_pending_lockfree(&self) -> bool {
        self.raf_pending_flag
            .as_ref()
            .is_some_and(|f| f.swap(false, std::sync::atomic::Ordering::Relaxed))
    }

    /// ADR-016 M2.3: value-returning JS drain that is **deferred** (returns
    /// `None`) while a rAF turn is in flight on the engine thread. The parked
    /// `about_to_wait` loop issues several blocking `route_query_js` drains each
    /// pass (canvas bitmaps, history/pushState, traversals, navigation updates);
    /// under the flag every one of them would otherwise FIFO-serialize behind the
    /// in-flight (up to ~200 ms) `run_animation_frame` task and freeze the loop —
    /// exactly the stall M2.3 removes. Skipping a drain merely defers it to the
    /// next pass after the turn finishes (the short rAF wakeup keeps the loop
    /// warm). Off the flag `raf_turn_inflight()` is always `false`, so this is
    /// byte-identical to calling [`route_query_js`] directly.
    pub(crate) fn drain_query_js<R: Send + 'static>(
        &self,
        read: impl FnOnce(&Arc<dyn PersistentJs>) -> R + Send + 'static,
    ) -> Option<R> {
        if self.raf_turn_inflight() {
            return None;
        }
        route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), read)
    }

    /// ADR-016 M2.3: non-consuming peek at the rAF-pending flag lock-free (the
    /// [`Self::take_raf_pending_lockfree`] counterpart of `has_raf_pending`).
    /// Used to decide the next parked-loop wakeup without clearing the signal.
    pub(crate) fn raf_pending_lockfree(&self) -> bool {
        self.raf_pending_flag
            .as_ref()
            .is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// ADR-016 M2.3: consume (clear + return) the DOM-dirty flag lock-free via the
    /// cached UI-side atomic (companion to [`Self::take_raf_pending_lockfree`]).
    pub(crate) fn take_dom_dirty_lockfree(&self) -> bool {
        self.dom_dirty_flag
            .as_ref()
            .is_some_and(|f| f.swap(false, std::sync::atomic::Ordering::Relaxed))
    }

    /// ADR-016 M2.3: dispatch one `run_animation_frame(raf_ts)` batch to the
    /// engine thread as a **non-blocking** `task`, marking `raf_task_inflight`
    /// for its whole duration so the scroll/redraw path presents the retained
    /// display list (and skips the JS pump) until it finishes. The caller must
    /// have already consumed the pending flag and updated `last_raf_batch_ms`.
    /// Only reached under `LUMEN_ENGINE_THREAD=1` (engine thread present).
    pub(crate) fn fire_raf_turn_async(&self, raf_ts: f64) {
        let Some(engine) = self.engine_thread.as_ref() else {
            return;
        };
        let inflight = Arc::clone(&self.raf_task_inflight);
        inflight.store(true, std::sync::atomic::Ordering::Release);
        engine.task(move |state| {
            if let Some(js) = &state.js {
                js.run_animation_frame(raf_ts);
            }
            inflight.store(false, std::sync::atomic::Ordering::Release);
        });
    }

    /// ADR-016 M2.3: engine-thread rAF pump step shared by `RedrawRequested`
    /// Step 3.1/4 and the `about_to_wait` parked pump. Runs **only** under the
    /// flag (`self.engine_thread.is_some()`); the single-thread path keeps its
    /// original synchronous sequence for byte-identical behavior.
    ///
    /// Non-blocking by construction: (1) when a rAF batch is due and none is
    /// already running, consume the pending flag lock-free and fire the turn
    /// async ([`Self::fire_raf_turn_async`]); (2) when no turn is running,
    /// consume the DOM-dirty flag lock-free and, if a completed turn mutated the
    /// DOM, submit an **async** relayout ([`Self::relayout_raf_dirty`]) whose
    /// result lands via [`Self::poll_engine_commit`]. Neither step issues a
    /// blocking engine `query`, so the winit thread never stalls behind the JS
    /// turn and scroll stays smooth. Returns `true` if a relayout was submitted
    /// (caller requests a redraw).
    pub(crate) fn pump_raf_engine_thread(&mut self, raf_due: bool, timestamp_ms: f64) -> bool {
        // A turn still running hasn't finished its DOM mutations and holds the
        // engine FIFO — leave both the dirty check and the next fire to a later
        // pass (the flag is not cleared, so the pending signal survives).
        if self.raf_turn_inflight() {
            return false;
        }
        // Consume a completed turn's DOM mutations first (before any re-fire) so a
        // continuous rAF-DOM loop still relayouts each cycle.
        let mut submitted = false;
        if self.take_dom_dirty_lockfree() {
            self.relayout_raf_dirty();
            submitted = true;
        }
        // Drain gate: the first non-inflight pass after a turn completes is
        // reserved for the deferred `drain_query_js` queues (which run this pass,
        // engine now free) — hold off firing the next turn until the following
        // pass so a continuous rAF loop can't starve notifications/popups/console.
        if self.raf_drain_gate {
            self.raf_drain_gate = false;
            return submitted;
        }
        if raf_due && self.take_raf_pending_lockfree() {
            self.last_raf_batch_ms = timestamp_ms;
            let raf_ts = if self.deterministic.enabled { 0.0 } else { -1.0 };
            self.fire_raf_turn_async(raf_ts);
            self.raf_drain_gate = true;
        }
        submitted
    }

    /// ADR-016 M2.2c-3: route the **rAF DOM-dirty flush that is followed by a
    /// synchronous read of a layout product** off the UI thread via the blocking
    /// request/reply [`engine_thread::EngineThread::readback`], falling back to the
    /// synchronous [`Self::relayout`] otherwise (the default, so behavior is
    /// byte-identical unless `LUMEN_ENGINE_THREAD=1`).
    ///
    /// This is the `RedrawRequested` Step 4 site: a `requestAnimationFrame` callback
    /// mutated the DOM and the very next Step 5 reads `self.display_list.is_empty()`
    /// to latch PerformancePaintTiming (W3C Paint Timing §2). That read must see the
    /// freshly-reflowed display list, so — unlike the async [`Self::relayout_form`] /
    /// [`Self::relayout_raf_dirty`] — the relayout cannot be deferred to a later
    /// commit. [`Self::readback_relayout_job`] computes the layout **on the engine
    /// thread** (which owns the mutable `Document` + `js_ctx` under the flag) and
    /// blocks for exactly that one commit, applying it synchronously so Step 5 sees
    /// the current display list.
    ///
    /// ADR-016 M4: in the single-thread fallback path, tries the incremental layout
    /// ([`Self::try_relayout_raf_incremental`]) before the full [`Self::relayout`].
    pub(crate) fn relayout_raf_dirty_readback(&mut self) {
        if !self.readback_relayout_job() && !self.try_relayout_raf_incremental() {
            self.relayout();
        }
    }

    /// Derive the CSS layout viewport for a relayout (shared by the synchronous
    /// [`Self::relayout`] and the off-thread [`Self::submit_relayout_job`]).
    ///
    /// Returns `None` — skip relayout — when there is no `LayoutSource`/renderer
    /// yet or the content region is degenerate (minimized window). Applies the
    /// live chrome inset (RP-2), `<meta viewport initial-scale>` and the user
    /// zoom, matching scroll clamping and the content `PushTransform`.
    pub(crate) fn relayout_viewport(&self) -> Option<Size> {
        let src = self.layout_source.as_ref()?;
        let r = self.renderer.as_ref()?;
        let vp_size = r.viewport_size();
        // RP-2: lay out against the live page content region, not the full
        // window. In an interactive window the page sits below the tab strip
        // (+ workspace switcher), so the layout viewport must exclude that
        // chrome to match scroll clamping (`viewport_height_css`) and the
        // PushTransform that shifts content down. Headless surfaces have no
        // chrome and use the full surface. Tracks live `inner_size` because
        // `viewport_size()` reflects the last `r.resize()` on `Resized`.
        let (vp_w, vp_h) =
            content_layout_viewport(vp_size, self.window.is_some(), self.workspace_panel.visible);
        // Guard against degenerate viewport (renderer not yet configured or minimized).
        if vp_w <= 0.0 || vp_h <= 0.0 {
            return None;
        }
        // Apply <meta viewport initial-scale> + user zoom to derive the CSS layout viewport.
        let meta_scale = meta_initial_scale(src);
        let (css_w, css_h) = zoom::effective_viewport(vp_w, vp_h, meta_scale, self.zoom_factor);
        Some(Size::new(css_w, css_h))
    }

    /// ADR-016 M2.2: post-layout UI-thread work shared by the synchronous
    /// [`Self::relayout`] and the off-thread commit path
    /// ([`Self::poll_engine_commit`]). Takes an already-computed
    /// `(DisplayList, LayoutBox)` (built either inline or on the engine thread)
    /// and applies everything that touches `&mut self`: caches, transitions /
    /// `@starting-style` sync, `will-change` layer promotion, zoom-preview reset,
    /// scroll clamping and JS-observer delivery. Kept identical for both callers
    /// so an off-thread relayout is byte-for-byte equivalent to a synchronous one.
    pub(crate) fn apply_relayout_result(&mut self, mut new_dl: DisplayList, lb: lumen_layout::LayoutBox, viewport: Size) {
        // BUG-480 срез 13: контентный вьюпорт под-документов следует за
        // размером их host-бокса — значит за каждым relayout (ресайз, зум,
        // любое движение вёрстки над фреймом). Проход сам гейтится на
        // «размер не менялся» и на пустом списке фреймов стоит ноль. ДО
        // заимствования `layout_source`: там берётся `&self` на всю функцию.
        let frame_state = self.frame_interactive();
        crate::frames::sync_frame_viewports(&mut self.frames, &lb, frame_state);
        // FRAME-5 срез 2: fetch+register whatever lazy `<img>` just entered a
        // frame's own proximity margin — a mere relayout has no page-commit
        // step to piggy-back on, unlike the initial load (`page_pipeline.rs`).
        self.register_frame_lazy_images();
        let Some(src) = self.layout_source.as_ref() else { return };
        self.content_height = content_height_of(&new_dl);
        self.content_width = content_width_of(&new_dl);
        // BUG-480 срез 14: вклейка ПОСЛЕ метрик (содержимое фрейма не должно
        // растягивать прокрутку страницы — обе функции складывают плоский
        // список прямоугольников и клипов не видят) и ДО diff/кэша, чтобы обе
        // стороны сравнения были одинаково склеенными.
        crate::frames::splice_frame_content(&mut new_dl, &self.frames);
        self.tile_grid.update_from_diff(&self.display_list, &new_dl);
        // Cache display list directly (avoid &mut self while layout_source is borrowed).
        let _dl_hash = lumen_paint::hash_commands(&new_dl);
        self.display_list_cache.insert(lb.node.index() as u32, new_dl.clone(), _dl_hash, None);
        // Поля пишутся напрямую (не через `set_display_list`): `layout_source`
        // здесь заимствован, `&mut self` целиком взять нельзя.
        self.display_list = new_dl;
        self.display_list_epoch = next_dl_epoch(self.display_list_epoch);
        // Sync transitions: compare prev styles with new layout before replacing.
        let now_s = self.epoch.elapsed().as_secs_f32();
        let mut new_styles = HashMap::new();
        collect_box_styles(&lb, &mut new_styles);
        for (node, new_style) in &new_styles {
            if let Some(old_style) = self.prev_styles.get(node) {
                self.transition_scheduler.sync(*node, old_style, new_style, now_s);
            }
        }
        // @starting-style (CSS Transitions L2 §3.4): newly visible nodes (not in
        // prev_styles) use @starting-style rules as the before-change style so that
        // entry transitions start from the declared starting values.
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
                        self.transition_scheduler.sync(
                            *node,
                            starting_style,
                            new_style,
                            now_s,
                        );
                    }
                }
            }
        }
        self.prev_styles = new_styles;
        // BUG-341 S7: invalidate the restyle-cascade cache by default — every
        // producer routes through here, but only `try_relayout_raf_incremental`'s
        // restyle sub-path knows how to recompute a cache that actually matches
        // `lb`, and re-validates it right after this call returns. Every other
        // producer (full `relayout()`, `readback_relayout_job`,
        // `poll_engine_commit`) leaves it `None`, forcing the next incremental
        // attempt onto the safe full-cascade-plus-graft fallback for one cycle.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(lb);
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
        self.animation_scheduler.clear();
        // Do NOT reset transition_scheduler here: active transitions must survive
        // relayout (viewport resize, DOM mutations) so that in-flight animations
        // continue smoothly. reset happens only on page load (apply_loaded_page).
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
            if self.js_present
                && let Some(lb_ref) = self.layout_box.as_ref()
            {
                let rects = collect_layout_rects(lb_ref);
                let hit_test_tree = Arc::new(lb_ref.clone());
                let styles = collect_computed_styles(lb_ref);
                let customs = collect_custom_properties(lb_ref);
                let (vw, vh) = (viewport.width, viewport.height);
                let dark_mode = self.dark_mode;
                let reduced_motion = self.a11y_store.reduced_motion();
                // Keep JS scroll-state cache in sync so scrollTop/scrollLeft reads
                // immediately after relayout return the correct clamped values.
                let scroll_states: HashMap<u32, [f32; 4]> = collect_scroll_containers(lb_ref)
                    .iter()
                    .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                    .collect();
                lazy_reqs = route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                    js.update_layout_rects(rects);
                    js.update_hit_test_tree(hit_test_tree);
                    js.update_computed_styles(styles);
                    js.update_custom_properties(customs);
                    js.update_viewport_size(vw, vh);
                    js.deliver_layout_observers();
                    // CSS MQ L4 §4.2: re-evaluate matchMedia() lists against the new
                    // viewport. `dark_mode` mirrors the OS `prefers-color-scheme`,
                    // read from winit at window creation / refreshed on ThemeChanged.
                    js.deliver_media_query_changes(vw, vh, dark_mode, reduced_motion);
                    // After fresh rects are in JS: fire lazy-load proximity check.
                    // Images that entered the viewport+margin are queued by JS via
                    // _lumen_request_lazy_image_load; we drain and fetch them below.
                    js.deliver_lazy_images();
                    let reqs = js.take_lazy_image_requests();
                    js.update_scroll_states(scroll_states);
                    reqs
                })
                .unwrap_or_default();
            }
            if !lazy_reqs.is_empty() {
                self.fetch_and_register_lazy_images(lazy_reqs);
            }
        }
        // BUG-730: images the page added after load land here — this is the one
        // post-layout point every relayout producer routes through, so a
        // script-appended `<img>` is picked up whichever path relaid it out.
        self.spawn_dynamic_image_loads(viewport);
        // BUG-735: и по той же причине — свежеперестроенное поддерево могло
        // принести НОВЫЙ `<img>` с уже декодированным `src` (React перерисовал
        // блок: узел другой, картинка та же). Второго `ImageDecoded` для него не
        // будет — запрос дедуплицирован по URL, — поэтому размеры ему раздаёт
        // проход `apply_stream_intrinsic_sizes`, и здесь мы его заказываем.
        // Пустой карте заказывать нечего; сам проход no-op, если дописывать
        // нечего, так что «релейаут → проход → релейаут» не зацикливается.
        self.stream_image_sizes_dirty |= !self.stream_image_sizes.is_empty();
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
        let (cv_x, cv_y) = (self.scroll_x, self.scroll_y);
        let cv_relevant = self.cv_relevant.clone();
        let job = move || {
            let t0 = std::time::Instant::now();
            // Interactive state is thread-local — set it on THIS (engine) thread.
            lumen_layout::set_interactive_state(hovered, focused, active);
            lumen_layout::set_forced_colors(forced_colors);
            lumen_layout::set_cv_scroll(cv_x, cv_y);
            lumen_layout::set_cv_relevant(cv_relevant);
            let (content, layout_box) =
                compute_layout(&document, &stylesheet, viewport, &*hp, dark_mode, &web_fonts);
            lumen_layout::clear_interactive_state();
            lumen_layout::set_cv_scroll(0.0, 0.0);
            lumen_layout::set_cv_relevant(std::collections::HashSet::new());
            EngineCommit {
                content,
                layout_box,
                viewport,
                generation,
                compute_ms: t0.elapsed().as_secs_f32() * 1000.0,
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
        let EngineCommit { content, layout_box, viewport, compute_ms, .. } = commit;
        self.apply_relayout_result(content, layout_box, viewport);
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
        let EngineCommit { content, layout_box, viewport, compute_ms, .. } = commit;
        self.apply_relayout_result(content, layout_box, viewport);
        // ADR-016 M2.0/M2.2: record the off-thread compute cost. Unlike the
        // synchronous path this excludes the UI-thread apply (observers etc.),
        // and is tagged `(off-thread)` so the summary reflects the work moved off
        // the UI thread.
        self.engine_stats.record(compute_ms);
        if lumen_paint::frame_log_enabled() {
            eprintln!(
                "[engine] relayout {compute_ms:.2}ms (off-thread) dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
    }

    /// ADR-016 M2.2c-2d (21): назначить JS-хэндл активной вкладки, держа
    /// [`Self::js_present`] в связке с фактическим владельцем `Arc`.
    ///
    /// **Это единственная точка владения хэндлом.** Куда садится `Arc` зависит от
    /// того, поднят ли движковый поток:
    /// - поток есть (`LUMEN_ENGINE_THREAD=1`) → `Arc` **переезжает на движковый
    ///   поток** в [`EngineJsState::js`] через [`engine_thread::EngineThread::task`],
    ///   а UI-сторонний [`Self::js_ctx`] остаётся `None`. Маршрутизаторы
    ///   ([`route_task_js`]/[`route_query_js`]/[`route_eval_js`]) под флагом и так
    ///   игнорируют переданный UI-клон и читают `state.js`, поэтому все call-site'ы
    ///   остаются корректны, а сам рантайм всё равно живёт на своём `lumen-js`-потоке
    ///   (ADR-014) — это перенос владения хэндлом, а не разделение мутабельного
    ///   состояния (инвариант 1);
    /// - потока нет (флаг выключен, по умолчанию, либо spawn не удался) → `Arc`
    ///   хранится в UI-стороннем [`Self::js_ctx`] как прежде — **байт-идентично**.
    ///
    /// [`Self::js_present`] отделяет решение «есть ли JS?» от того, какая сторона
    /// держит `Arc`: все гейты (`if self.js_present`) читают его, поэтому остаются
    /// верны в обоих режимах флага.
    pub(crate) fn set_js_ctx(&mut self, handle: Option<Arc<dyn PersistentJs>>) {
        // BUG-839: the document is committed at this point, so per-step
        // Resource Timing delivery may resume — whatever is still queued, and
        // everything that arrives from here on, belongs to this runtime. The
        // *clear* is deliberately not here (it runs where the load starts): by
        // the time this is reached, `source.load` has already fetched the
        // page's stylesheets, scripts and images.
        resource_timing::resume();
        self.js_present = handle.is_some();
        match self.engine_thread.as_ref() {
            // Flag on: the handle lives engine-side; deposit it into
            // `EngineJsState.js` and leave the UI field empty.
            Some(engine) => {
                // ADR-016 M2.3: before the handle moves engine-side, cache
                // lock-free clones of its rAF-pending / DOM-dirty flags so the
                // UI thread can schedule + consume rAF turns without a blocking
                // engine `query`. `None` clears them (blank/JS-less tab).
                self.raf_pending_flag = handle.as_ref().and_then(|h| h.raf_pending_flag());
                self.dom_dirty_flag = handle.as_ref().and_then(|h| h.dom_dirty_flag());
                self.js_ctx = None;
                engine.task(move |state| state.js = handle);
            }
            // Flag off (default): the UI thread owns the handle, exactly as before.
            None => self.js_ctx = handle,
        }
    }

    /// ADR-016 M2.2c-2b: зеркалит разделяемый `Document` активной вкладки в
    /// персистентное состояние [`EngineJsState`] движкового потока.
    ///
    /// No-op, когда движкового потока нет (`LUMEN_ENGINE_THREAD` выключен, по
    /// умолчанию) — тогда поведение shell байт-идентично. Вызывается при каждой
    /// смене страницы (после [`Self::set_js_ctx`] + установки `layout_source`),
    /// чтобы `task`/`query`-вызовы видели актуальный DOM. `Arc`-клон дёшев.
    ///
    /// Владение JS-хэндлом сюда больше не входит — с M2.2c-2d (21) его переносит
    /// сам [`Self::set_js_ctx`]; здесь остаётся только зеркало `document`
    /// («сиденье» будущего владения DOM движковым потоком, M2.2c-3).
    pub(crate) fn sync_engine_js_state(&self) {
        let Some(engine) = self.engine_thread.as_ref() else { return };
        let document = self.layout_source.as_ref().map(|ls| Arc::clone(&ls.document));
        engine.task(move |state| state.document = document);
    }

    /// ADR-016 M2.2c-2d (21): извлечь JS-хэндл активной вкладки для снапшота
    /// (`save_page_snapshot`).
    ///
    /// Под флагом (`LUMEN_ENGINE_THREAD=1`) `Arc` живёт в [`EngineJsState::js`] на
    /// движковом потоке, поэтому его вынимает блокирующий `query`, `take`-ающий его
    /// из состояния (встаёт в очередь после уже отправленных `task`, так что видит
    /// последний зеркалированный хэндл); без флага (по умолчанию) — `take` прямо из
    /// UI-стороннего [`Self::js_ctx`], **байт-идентично** прежнему `self.js_ctx.take()`.
    /// Возвращённый `Arc` кладётся в [`PageSnapshot::js_ctx`] и остаётся реальным
    /// хэндлом даже под флагом (bg-tab GC и restore читают его напрямую).
    pub(crate) fn take_js_ctx(&mut self) -> Option<Arc<dyn PersistentJs>> {
        match self.engine_thread.as_ref() {
            Some(engine) => engine.query(|state| state.js.take()).flatten(),
            None => self.js_ctx.take(),
        }
    }

    /// BUG-480 срез 19: КЛОН JS-хэндла страницы, где бы он ни лежал.
    ///
    /// [`Self::js_ctx`] в живом окне почти всегда `None`: с ADR-023 движковый
    /// поток включён ПО УМОЛЧАНИЮ, и [`Self::set_js_ctx`] кладёт хэндл в его
    /// состояние, оставляя UI-поле пустым. Код, который берёт `self.js_ctx`
    /// напрямую вместо `route_*`, поэтому молча ничего не делает — навигация
    /// фрейма так не зарегистрировала под-документ у родителя и не отправила
    /// `load` на хосте, пока проба не измерила это на живом окне.
    ///
    /// Нужен именно хэндл, а не `route_task_js`: под-документ грузится
    /// синхронно и зовёт у родителя четыре разных метода, а маршрутизатор
    /// умеет только «выполнить одно замыкание и забыть». Вызов самого хэндла с
    /// UI-потока безопасен — каждый `V8JsRuntime` владеет своим потоком и
    /// изолятом и сам переправляет работу туда (это и есть путь без
    /// движкового потока).
    pub(crate) fn clone_js_ctx(&self) -> Option<Arc<dyn PersistentJs>> {
        match self.engine_thread.as_ref() {
            Some(engine) => engine.query(|state| state.js.clone()).flatten(),
            None => self.js_ctx.clone(),
        }
    }
}

/// Повторный layout+paint по сохранённому `LayoutSource` с новым viewport.
/// Возвращает `(DisplayList, LayoutBox)` — LayoutBox нужен для animation scheduler.
/// `dark_mode` is forwarded to `layout_measured_hyp` so `@media (prefers-color-scheme: dark)`
/// rules take effect on relayout (e.g. after OS theme change or window resize).
pub(crate) fn relayout_page(
    src: &LayoutSource,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
) -> (DisplayList, lumen_layout::LayoutBox) {
    compute_layout(&src.document, &src.stylesheet, viewport, hp, dark_mode, web_fonts)
}

/// Процесс-глобальные метрики системных шрифтов для измерителя: CSS
/// generic-семейства + конкретные системные семейства по имени (BUG-128).
///
/// Строится один раз поверх общего системного индекса
/// ([`lumen_font::shared_system_index`]) и переиспользуется всеми
/// пересборками измерителя: сам скан директорий шрифтов страница делает в
/// любом случае (рендер резолвит face-ы через тот же индекс), а чтение и
/// парсинг выбранных файлов не должно повторяться на каждый relayout.
/// Ленивый кэш конкретных семейств живёт здесь же, поэтому `font-family:
/// Arial` читается с диска один раз на процесс, а не на каждый релэйаут.
pub(crate) fn system_font_faces() -> Arc<lumen_paint::SystemFaceSet> {
    static SHARED: std::sync::OnceLock<Arc<lumen_paint::SystemFaceSet>> =
        std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| {
            Arc::new(lumen_paint::SystemFaceSet::from_provider(
                lumen_font::shared_system_index().clone(),
            ))
        })
        .clone()
}

/// Измеритель для страницы: bundled Inter + @font-face-семьи + системные
/// face-ы (generic-семейства и конкретные семейства по имени).
///
/// Единая точка сборки для всех layout-путей (полный / инкрементальный /
/// restyle) — иначе системные семейства меряются по-разному в зависимости от
/// того, есть ли на странице web-шрифты.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn page_measurer(
    font: &lumen_font::Font<'static>,
    web_fonts: &[LoadedWebFont],
) -> lumen_paint::MultiFontMeasurer {
    let mut measurer = lumen_paint::MultiFontMeasurer::new(font)
        .expect("MultiFontMeasurer из bundled Inter");
    for wf in web_fonts {
        measurer.register_family_with_ranges(
            &wf.family,
            wf.bytes.clone(),
            wf.unicode_range.clone(),
        );
    }
    measurer.set_system_faces(system_font_faces());
    measurer
}

/// Ядро style+layout+display-list по immutable-снапшоту документа и стилей.
///
/// Вынесено из [`relayout_page`], чтобы одну и ту же работу можно было вызвать и
/// на UI-потоке (синхронный `relayout()`), и на движковом потоке (ADR-016 M2.2,
/// [`Lumen::submit_relayout_job`]) — второму `LayoutSource` недоступен, у него на
/// руках только `Arc`-снимки `document`/`stylesheet`. Интерактивное состояние
/// (`:hover`/`:focus`/`forced-colors`/`content-visibility` scroll) — thread-local
/// (`lumen_layout::set_*`), поэтому вызывающая сторона обязана выставить его на
/// **том же** потоке до вызова и сбросить после.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn compute_layout(
    document: &Mutex<Document>,
    stylesheet: &lumen_css_parser::Stylesheet,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
) -> (DisplayList, lumen_layout::LayoutBox) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    // PH3-19: измеритель включает накопленные web-шрифты (FOUT relayout);
    // BUG-128: и системные face-ы.
    let measurer = page_measurer(&font, web_fonts);
    let doc = document.lock().unwrap();
    let layout = lumen_layout::layout_measured_hyp(&doc, stylesheet, viewport, &measurer, hp, dark_mode);
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout)
}

/// ADR-016 M4: incremental variant of [`relayout_page`] — uses
/// [`lumen_layout::layout_mutation_incremental`] to skip geometry re-computation
/// for subtrees whose [`lumen_layout::ComputedStyle`] is unchanged, while
/// preserving full cascade and post-layout passes. `prev` is the previously
/// laid-out tree stored in `Lumen::layout_box`.
pub(crate) fn relayout_page_incremental(
    src: &LayoutSource,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    prev: &lumen_layout::LayoutBox,
) -> (DisplayList, lumen_layout::LayoutBox) {
    compute_layout_incremental(&src.document, &src.stylesheet, viewport, hp, dark_mode, web_fonts, prev)
}

/// ADR-016 M4: incremental variant of [`compute_layout`] — runs the full
/// cascade but reuses geometry from `prev` for unchanged subtrees.
///
/// Same caller contract as [`compute_layout`]: thread-local interactive state
/// must be set before the call and cleared afterwards.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn compute_layout_incremental(
    document: &Mutex<Document>,
    stylesheet: &lumen_css_parser::Stylesheet,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    prev: &lumen_layout::LayoutBox,
) -> (DisplayList, lumen_layout::LayoutBox) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = page_measurer(&font, web_fonts);
    let doc = document.lock().unwrap();
    let layout = lumen_layout::layout_mutation_incremental(
        &doc, stylesheet, viewport, &measurer, hp, dark_mode, prev,
    );
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout)
}

/// BUG-341 S7: restyle-aware variant of [`relayout_page_incremental`] — uses
/// [`lumen_layout::box_tree::layout_mutation_incremental_restyle`] instead of
/// [`lumen_layout::layout_mutation_incremental`], skipping cascade work (not
/// just geometry) for subtrees `delta.dirty_roots` proves untouched. Only
/// safe when `delta.prev_styles` is the exact `CounterMap::styles()` the
/// previous cycle over this same document produced — see
/// `layout_mutation_incremental_restyle`'s own doc comment for the full
/// precondition; [`Lumen::page_prev_cascade_styles`] being `Some` is the
/// caller-side half of that contract. Returns the fresh `CounterMap` so the
/// caller can persist its `styles()` as the next cycle's `delta.prev_styles`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn relayout_page_incremental_restyle(
    src: &LayoutSource,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    // BUG-341 S19: consumed — the reusable subtrees are moved out of it into
    // the tree returned. See `layout_mutation_incremental_restyle`.
    prev: lumen_layout::LayoutBox,
    delta: lumen_layout::counters::RestyleDelta<'_>,
) -> (DisplayList, lumen_layout::LayoutBox, lumen_layout::CounterMap) {
    compute_layout_incremental_restyle(
        &src.document, &src.stylesheet, viewport, hp, dark_mode, web_fonts, prev, delta,
    )
}

/// BUG-341 S7: restyle-aware variant of [`compute_layout_incremental`] — see
/// [`relayout_page_incremental_restyle`].
#[allow(clippy::too_many_arguments)]
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn compute_layout_incremental_restyle(
    document: &Mutex<Document>,
    stylesheet: &lumen_css_parser::Stylesheet,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    // BUG-341 S19: consumed — the reusable subtrees are moved out of it into
    // the tree returned. See `layout_mutation_incremental_restyle`.
    prev: lumen_layout::LayoutBox,
    delta: lumen_layout::counters::RestyleDelta<'_>,
) -> (DisplayList, lumen_layout::LayoutBox, lumen_layout::CounterMap) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = page_measurer(&font, web_fonts);
    let doc = document.lock().unwrap();
    let (layout, counters) = lumen_layout::box_tree::layout_mutation_incremental_restyle(
        &doc, stylesheet, viewport, &measurer, hp, dark_mode, prev, delta,
    );
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout, counters)
}

/// CSS Containment L3 §4.4 (BB-4) — shell-событие: элемент с
/// `content-visibility: auto` сменил skipped-состояние между layout-проходами.
/// `skipped == true` — поддерево выпало из расширенного viewport и пропущено;
/// `false` — узел стал relevant и его содержимое снова выложено.
/// Phase 2: P3 доставляет как `contentvisibilityautostatechange` в JS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContentVisibilityChange {
    /// DOM-узел элемента с `content-visibility: auto`.
    pub(crate) node: NodeId,
    /// Новое состояние: `true` — содержимое пропущено, `false` — выложено.
    pub(crate) skipped: bool,
}

/// Собрать `(node, top_y)` **всех** `content-visibility: auto` боксов в порядке
/// дерева. top_y — страница-координаты бокса. Скан по дереву (а не thread-local)
/// — работает и для layout-а, выполненного в фоновом потоке загрузки страницы.
///
/// BUG-852: раньше эта функция собирала только боксы с пустым списком детей и
/// звала их «пропущенными». Совпадение неточное в обе стороны: пустой
/// `<div style="content-visibility:auto">` — а именно такой строит
/// `content-visibility-auto-state-changed-first-observation.html` — выглядел
/// пропущенным, где бы он ни стоял, а layout про него вообще не спрашивал
/// (`cv_should_skip` вызывается только при `!children.is_empty()`). Состояние
/// теперь считает [`Lumen::refresh_cv_state`] по самому правилу релевантности.
///
/// **Дедупликация по узлу обязательна, и её отсутствие — не мелочь.** Анонимный
/// бокс (`InlineRun` для inline-содержимого, `InlineBlockRow`, обёртки таблиц)
/// не имеет своего элемента и несёт стиль родителя, включая
/// `content-visibility: auto`, — то есть `<div style="content-visibility:auto">
/// <span>x</span></div>` даёт ДВА бокса с этим значением. Без дедупликации
/// `diff_cv_state` сравнил бы второй из них с ещё не обновлённым `prev` и
/// выдал бы страницу **два** события на одно изменение, ровно то, что
/// `content-visibility-auto-state-changed-first-observation.html` запрещает
/// («already observed»). Первый бокс в порядке дерева — сам элемент, анонимный
/// всегда его потомок. Layout решает ту же задачу тем же способом:
/// `CV_SKIPPED` дедуплицируется по узлу.
pub(crate) fn collect_cv_auto(b: &lumen_layout::LayoutBox, out: &mut Vec<(NodeId, f32)>) {
    fn walk(
        b: &lumen_layout::LayoutBox,
        seen: &mut std::collections::HashSet<NodeId>,
        out: &mut Vec<(NodeId, f32)>,
    ) {
        if b.style.content_visibility == lumen_layout::style::ContentVisibility::Auto
            && seen.insert(b.node)
        {
            out.push((b.node, b.rect.y));
        }
        for c in &b.children {
            walk(c, seen, out);
        }
    }
    walk(b, &mut std::collections::HashSet::new(), out);
}

/// Дифф skipped-состояния между двумя проходами → события
/// [`ContentVisibilityChange`].
///
/// CSS Contain L2 §4.1: событие должно приходить и на **первое** наблюдение
/// элемента, в обе стороны — `skipped: false` для элемента во вьюпорте не менее
/// обязателен, чем `skipped: true` для элемента под ним. Поэтому узел, которого
/// в `prev` нет вовсе, всегда порождает событие со своим текущим состоянием, а
/// узел, который из дерева исчез, — никакого: отсоединённый элемент молчит
/// (`content-visibility-auto-state-changed-removed.html`).
///
/// `next` — в порядке дерева, чтобы порядок событий не зависел от обхода хеша.
pub(crate) fn diff_cv_state(
    prev: &std::collections::HashMap<NodeId, bool>,
    next: &[(NodeId, bool)],
) -> Vec<ContentVisibilityChange> {
    let mut out = Vec::new();
    for &(node, skipped) in next {
        if prev.get(&node) != Some(&skipped) {
            out.push(ContentVisibilityChange { node, skipped });
        }
    }
    out
}

/// Extract `initial-scale` from the `<meta name=viewport>` of a page's document.
///
/// Returns `1.0` when the page has no viewport meta or omits `initial-scale`.
pub(crate) fn meta_initial_scale(src: &LayoutSource) -> f32 {
    src.document
        .lock()
        .ok()
        .and_then(|doc| doc.viewport_meta().map(|m| m.initial_scale))
        .unwrap_or(1.0)
}
