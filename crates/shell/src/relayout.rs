//! Relayout pipeline of the shell: page reflow, the rAF turn, the off-thread
//! layout job (ADR-016) and ownership of the page's JS context handle.
//!
//! SPLIT-SH1 (2026-08-26): moved verbatim out of `main.rs`. Behaviour, order of
//! operations and method bodies are unchanged; only module path and visibility
//! (`fn` -> `pub(crate) fn`, required for a caller in the parent module) differ.

use crate::*;

/// BUG-935 S31: measurement-only override for `defer_js_push` at the two call
/// sites S27 converted (`relayout`/`poll_engine_commit`, `:188`/`:1267`
/// below). S28/S29/S30 each tried an interleaved A/B of `true` vs `false` by
/// editing the literal and doing a full rebuild between runs — every attempt
/// lost the comparison to machine noise that grew *during* the rebuild+link
/// gap (minutes), not during the measurement itself. Reading an env var once
/// (no rebuild, just a process restart) narrows that gap to seconds, so the
/// next census attempt does not have to fight the same clock. `None` (the
/// default, unset) leaves both sites at their shipped `true` — this must
/// never change measured behavior for anyone who has not set the var.
fn defer_js_push_override() -> Option<bool> {
    static OVERRIDE: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *OVERRIDE.get_or_init(|| match std::env::var("LUMEN_BUG935_DEFER_JS_PUSH").ok().as_deref() {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    })
}

/// BUG-935 S46: measurement-only override for the M4-routing order in
/// [`Lumen::relayout_raf_dirty`] — S12/S14/S18/S27 each tried the same swap
/// (incremental-first) via a literal edit + full rebuild between runs, and
/// each lost the comparison to noise introduced by the rebuild+link gap
/// itself (see [`defer_js_push_override`]'s doc for the same lesson). This
/// mirrors that fix: an env var read once per process, no rebuild needed to
/// flip it. `None` (unset, the default) leaves the shipped order (full
/// off-thread first) unchanged for anyone who has not set the var.
fn m4_swap_override() -> bool {
    static OVERRIDE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OVERRIDE.get_or_init(|| std::env::var("LUMEN_BUG935_M4_SWAP").ok().as_deref() == Some("1"))
}

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
    /// с последней сборки, или CSSOM-5 срез 2 (BUG-897) — если изменился
    /// `document.adoptedStyleSheets`, или BUG-493 — если страница правила лист
    /// через CSSOM (`insertRule`/`deleteRule`/`.style`). Возвращает `true`,
    /// если лист заменён.
    ///
    /// Таблица стилей страницы собирается один раз за навигацию — на этапе
    /// разбора, сразу после выполнения синхронных скриптов. Всё, что вставляет
    /// `<style>` позже (обработчик `load`, `setTimeout`, rAF, промис — то есть
    /// любой CSS-in-JS), до этого оставалось вне каскада навсегда. Здесь
    /// дешёвый отпечаток ([`inline_style_fingerprint`]) сверяется на каждом
    /// релейауте, а полная пересборка (склейка из [`DynamicCssBase`] + парс)
    /// происходит только когда блоки действительно изменились. Тот же принцип
    /// у `document.adoptedStyleSheets`: `PersistentJs::document_adopted_fingerprint`
    /// — дешёвая проверка на каждом релейауте, сам мёрдж — только при различии.
    ///
    /// Сеть не трогается: `@import` внутри *нового* листа останется
    /// неразрешённым, `@font-face` из него не подгрузится — релейаут не место
    /// для загрузок. Обычный CSS-in-JS ни того, ни другого не использует.
    pub(crate) fn refresh_dynamic_css(&mut self) -> bool {
        // Читается ДО заимствования `self.layout_source` — разные поля
        // `self`, но так проще: ручка нужна дважды (тут и ниже, для самого
        // мёрджа), а `layout_source` — только внутри этого вызова.
        // BUG-493: через `cascade_feed`, не `js_ctx` — под движковым потоком
        // (по умолчанию) `js_ctx` на UI-потоке пуст, и правки CSSOM с
        // `adoptedStyleSheets` до каскада отрисовки не доходили вовсе.
        let feed = self.cascade_feed.clone();
        let (adopted_fp, cssom_epoch) = feed
            .as_ref()
            .map(|f| (f.document_adopted_fingerprint(), f.cssom_epoch()))
            .unwrap_or((0, 0));
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
        let text_changed = fp != base.inline_fp || adopted_fp != base.adopted_fp;
        if !text_changed && cssom_epoch == base.cssom_epoch {
            return false;
        }
        let pristine = if text_changed {
            // GAP-CSPENF срез 21: та же политика, что `build_page_cascade` уже
            // считает для первичной сборки — поздно вставленный `<style>` (тот,
            // ради которого существует этот путь, BUG-743) обязан пройти тот же
            // гейт, иначе CSS-in-JS обходил бы style-src, вставляя стиль после
            // навигации вместо разметки.
            let root = doc.root();
            let csp_policy = crate::csp_enforce::document_csp_policy(&doc, root);
            let (inline, blocked) =
                extract_style_blocks(&doc, csp_policy.as_ref().map(|(p, _)| p.as_slice()));
            drop(doc);
            if let Some(js) = self.js_ctx.as_ref() {
                // GAP-CSPENF срез 57: each dispatch now carries the text of the
                // policy actually violated by that block, not the document's
                // combined text — same switch as `page_pipeline.rs`'s inline
                // `<style>` dispatch.
                for text in &blocked {
                    js.fire_csp_violation("style-src", "inline", text);
                }
            }
            let mut css =
                String::with_capacity(base.imports_prefix.len() + inline.len() + base.linked.len());
            css.push_str(&base.imports_prefix);
            css.push_str(&inline);
            css.push_str(&base.linked);
            let mut sheet = lumen_css_parser::parse(&css);
            if let Some(adopted) = feed.as_ref().and_then(|f| f.document_adopted_stylesheet()) {
                sheet.merge_from(adopted);
            }
            eprintln!(
                "CSS пересобран после правки <style>: {} правил",
                sheet.rules.len()
            );
            Arc::new(sheet)
        } else {
            // Документ нужен только для отпечатка; `patch_cascade` ниже сам
            // берёт его на сверку реестра — отпускаем до вызова.
            drop(doc);
            base.pristine.clone().unwrap_or_else(|| Arc::clone(stylesheet))
        };
        // BUG-493: CSSOM-правки страницы (`insertRule` в `<style>` от
        // CSS-in-JS в speedy-режиме, `deleteRule`, запись `.style`)
        // накладываются на каскад, по которому шелл рисует, а не только на
        // лист синхронного флаша — иначе правило, вставленное через CSSOM,
        // влияло бы на `getComputedStyle`, но не на экран.
        let patched = feed.as_ref().and_then(|f| f.patch_cascade(&pristine));
        match patched {
            Some(sheet) => {
                *stylesheet = Arc::new(sheet);
                base.pristine = Some(pristine);
            }
            None => {
                *stylesheet = pristine;
                base.pristine = None;
            }
        }
        base.inline_fp = fp;
        base.adopted_fp = adopted_fp;
        base.cssom_epoch = cssom_epoch;
        // Инкрементальный рестайл (BUG-341 S7) переиспользует стили прошлого
        // прохода — против нового листа они недействительны.
        self.page_prev_cascade_styles = None;
        true
    }

    /// GAP-CSSANIM срез 9: this frame's `height` transition/`@keyframes`
    /// overrides (if any), keyed by node — installed before a layout pass so
    /// `layout_measured_hyp_with_counters` sizes the animated box off the
    /// live interpolated value, the one way `getBoundingClientRect()`/
    /// `getClientRects()` can see it mid-animation (height cannot be
    /// compositor-offloaded the way `opacity`/`transform` are).
    pub(crate) fn animated_heights_snapshot(
        &self,
    ) -> HashMap<lumen_dom::NodeId, lumen_layout::style::Length> {
        self.anim_frame
            .as_ref()
            .map(|f| {
                f.overrides
                    .iter()
                    .filter_map(|(node, o)| o.height.clone().map(|h| (*node, h)))
                    .collect()
            })
            .unwrap_or_default()
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
        // LONGTASK-1: always-on frame timer for the Long Animation Frames API
        // — unlike `engine_t0` above, not gated by `LUMEN_FRAME_LOG` (that
        // flag only controls the debug stats/eprintln below), since a page's
        // `PerformanceObserver` must see slow frames in ordinary live
        // browsing, not only when profiling is turned on.
        let frame_start = std::time::Instant::now();
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
        // GAP-CSSANIM срез 9: see `animated_heights_snapshot` doc comment.
        lumen_layout::set_animated_heights(self.animated_heights_snapshot());
        let (new_dl, lb) = relayout_page(src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        lumen_layout::clear_interactive_state();
        lumen_layout::clear_animated_heights();
        lumen_layout::set_cv_scroll(0.0, 0.0);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        self.apply_relayout_result(
            new_dl,
            lb,
            viewport,
            defer_js_push_override().unwrap_or(true),
            #[cfg(feature = "v8")]
            None,
        );
        if let Some(t0) = engine_t0 {
            let engine_ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.engine_stats.record(engine_ms);
            eprintln!(
                "[engine] relayout {engine_ms:.2}ms dl={} styled={}",
                self.display_list.len(),
                self.prev_styles.len(),
            );
        }
        let frame_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        if frame_ms >= crate::persistent_js::LONGTASK_THRESHOLD_MS
            && let Some(js) = self.js_ctx.as_ref()
        {
            js.deliver_long_animation_frame(frame_ms);
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
        // BUG-935 S12: time this UI-thread path the same way `relayout()` times
        // the full sync path (`engine_t0` above) — before this section it ran
        // silently, and a census comparing it against `submit_relayout_job`'s
        // off-thread cost (S11) had nothing on this side to read.
        let incr_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
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
        // BUG-935 S13: both branches now hand back a `CascadeStyles` — the
        // non-restyle one used to return `None` here, which meant
        // `self.page_prev_cascade_styles` below stayed `None` after every cycle
        // that took it, so the restyle branch's precondition
        // (`page_prev_cascade_styles` being `Some`) was never met by a
        // continuous rAF+DOM-mutation loop and the cheap branch was
        // structurally unreachable (BUG-935 bug file, S12). `used_restyle`
        // keeps the two apart for the frame-log line below — it is no longer
        // derivable from "is there a cache to seed", since there always is now.
        // BUG-935 S15: S14 found the restyle branch's cost concentrated in the
        // <1ms-profiled prefix of `relayout_page_incremental_restyle` — a
        // 7047/6752ms outlier with the internal layout stages summing to
        // <1ms each. `document.lock()` right below is the one blocking call
        // in that prefix (the other candidate, `refresh_dynamic_css`, has
        // already run and returned above `incr_t0`), so time the wait on it
        // specifically instead of guessing from the aggregate.
        let mut lock_wait_ms: Option<f32> = None;
        let mut dirty_roots_ms: Option<f32> = None;
        let (new_dl, new_lb, fresh_cascade_styles, used_restyle) = if !touched.unattributed
            && let Some(prev_styles) = self.page_prev_cascade_styles.take()
        {
            let (prev_hover, prev_focus, prev_active) = self.page_prev_interactive;
            let lock_wait_t0 = incr_t0.is_some().then(std::time::Instant::now);
            let doc = src.document.lock().unwrap();
            lock_wait_ms = lock_wait_t0.map(|t| t.elapsed().as_secs_f32() * 1000.0);
            let dirty_roots_t0 = incr_t0.is_some().then(std::time::Instant::now);
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
            dirty_roots_ms = dirty_roots_t0.map(|t| t.elapsed().as_secs_f32() * 1000.0);
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
            (dl, lb, counters.into_styles(), true)
        } else {
            let (dl, lb, counters) = relayout_page_incremental(
                src, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts, &prev_lb,
            );
            (dl, lb, counters.into_styles(), false)
        };
        // BUG-935 S39: cheap read of the BUG-341 cascade-reuse tally
        // (`walk`'s `note_cascade`, already compiled in unconditionally) right
        // after whichever branch above ran — S38's premise ("the cascade skip
        // path hands back the same `Arc<ComputedStyle>` for untouched nodes")
        // was never directly measured, only inferred from a serialize-cache
        // hit rate. `LUMEN_FRAME_LOG`-gated, same zero-cost-when-off pattern as
        // every other `incr_t0`-conditioned line here.
        let cascade_stats = incr_t0.is_some().then(lumen_layout::counters::take_cascade_stats);
        lumen_layout::clear_interactive_state();
        lumen_layout::set_cv_scroll(0.0, 0.0);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        // BUG-935 S15: restyle-wrapper's own `document.lock()` (relayout.rs
        // ~1414) only accounted for 225-244ms of outlier ticks whose total
        // `incr_ms` ran 5.3-5.9s — most of the outlier is still unlocated;
        // `apply_relayout_result` is the next unprofiled candidate (it takes
        // a third, conditional `document.lock()` of its own for
        // @starting-style, plus tile-grid diff/hash over the whole DL).
        let apply_t0 = incr_t0.is_some().then(std::time::Instant::now);
        self.apply_relayout_result(
            new_dl,
            new_lb,
            viewport,
            true,
            #[cfg(feature = "v8")]
            None,
        );
        let apply_ms = apply_t0.map(|t| t.elapsed().as_secs_f32() * 1000.0);
        // `apply_relayout_result` unconditionally clears the cache — restore it
        // here, after `lb` has already landed in `self.layout_box`. BUG-935 S13:
        // both branches now produce a matching `CascadeStyles`, so this is no
        // longer conditional on which branch ran.
        self.page_prev_cascade_styles = Some(fresh_cascade_styles);
        self.page_prev_interactive = new_interactive;
        if let Some(t0) = incr_t0 {
            let incr_ms = t0.elapsed().as_secs_f32() * 1000.0;
            let fmt_ms = |ms: Option<f32>| ms.map(|v| format!("{v:.2}")).unwrap_or_else(|| "n/a".to_string());
            let (cascade_reused, cascade_recomputed) =
                cascade_stats.map(|s| (s.reused, s.recomputed)).unwrap_or_default();
            eprintln!(
                "[engine] relayout {incr_ms:.2}ms (incremental, on-thread) dl={} styled={} restyle={} lock_wait_ms={} dirty_roots_ms={} apply_ms={} cascade_reused={cascade_reused} cascade_recomputed={cascade_recomputed}",
                self.display_list.len(),
                self.prev_styles.len(),
                used_restyle as u8,
                fmt_ms(lock_wait_ms),
                fmt_ms(dirty_roots_ms),
                fmt_ms(apply_ms),
            );
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
    ///
    /// BUG-935 S12 tried swapping this order (incremental first, unconditionally)
    /// and measured a confirmed regression on `lenta.ru`, root-caused to the
    /// cascade-skip fast path (`page_prev_cascade_styles`) being structurally
    /// unreachable. BUG-935 S13 fixed that gap. BUG-935 S14 re-tried the swap
    /// under the S13 fix — `restyle=1` was reached this time (the fix works),
    /// but the census still failed to complete: two ticks stalled 6.7s/7.0s on
    /// the UI thread even though the restyle branch's own profiled substages
    /// summed to <1ms, and S14 guessed `src.document.lock()` contention with the
    /// engine thread's concurrent `run_animation_frame` turn as the cause.
    /// **BUG-935 S15 instrumented and refuted that guess** (`lock_wait_ms=0.00`
    /// on every measured tick, including the outlier): the real cost is
    /// [`Self::apply_relayout_result`]'s `route_query_js` JS-observer push
    /// (rects/styles/`deliver_layout_observers`/lazy-images) — a blocking
    /// round-trip into the engine thread's ordered FIFO, called from *every*
    /// producer, including the default off-thread one via
    /// [`Self::poll_engine_commit`]. The difference is *when* it is issued:
    /// [`Self::poll_engine_commit`] only calls it once [`EngineThread::take_committed`]
    /// reports a finished job, by which point the FIFO's earlier long item
    /// (the rAF JS turn / a synchronous `fetch`) has already drained, so the
    /// query lands on an idle queue. This on-thread path calls it synchronously
    /// right after the rAF JS turn that dirtied the DOM, landing *behind* that
    /// same turn if it is still running (e.g. mid-`fetch`) — hence the
    /// multi-second stalls. Reverted again — do not re-attempt this swap
    /// without first decoupling that JS-push from the synchronous layout apply
    /// (see the bug file's S15 section, "Не сделано / следующий срез").
    ///
    /// BUG-935 S18 re-tried the swap under the S17 fix: `apply_ms` stayed low
    /// (150-850ms) as expected, yet the census still stalled — the last
    /// logged tick showed a `[frame]` `js`-step of ~7.0s while
    /// `try_relayout_raf_incremental`'s own internal timers (`apply_ms` +
    /// profiled layout stages) summed to only ~1.07s, an almost-order-of-
    /// magnitude gap with no timer covering it. S18 could not tell whether
    /// that gap sits *inside* the function (an uninstrumented section its own
    /// `incr_ms` should already include, which would mean the two log lines
    /// were matched to the wrong tick) or *outside* it (something in this
    /// caller). The timer below wraps the exact call site and logs right next
    /// to it, so a future swap experiment can compare this number against the
    /// function's own `incr_ms` from a single well-defined tick instead of
    /// pattern-matching two independently-timed `eprintln!` lines after the
    /// fact.
    pub(crate) fn relayout_raf_dirty(&mut self) {
        if m4_swap_override() {
            let outer_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
            let handled = self.try_relayout_raf_incremental();
            if let Some(t0) = outer_t0 {
                eprintln!(
                    "[engine] relayout_raf_dirty outer_ms={:.2} (try_relayout_raf_incremental call, handled={handled})",
                    t0.elapsed().as_secs_f32() * 1000.0,
                );
            }
            if !handled && !self.submit_relayout_job() {
                self.relayout();
            }
            return;
        }
        if self.submit_relayout_job() {
            return;
        }
        let outer_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
        let handled = self.try_relayout_raf_incremental();
        if let Some(t0) = outer_t0 {
            eprintln!(
                "[engine] relayout_raf_dirty outer_ms={:.2} (try_relayout_raf_incremental call, handled={handled})",
                t0.elapsed().as_secs_f32() * 1000.0,
            );
        }
        if !handled {
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

    /// ADR-016 THREAD-2: pure decision behind [`Self::drain_query_js`]'s defer
    /// guard, split out so it is unit-testable without an engine thread. `true`
    /// means "do not call `query` this pass" — either a rAF turn is still
    /// running (`raf_turn_inflight`) or a submitted relayout `Run` job has not
    /// been applied yet (`job_generation != applied_generation`, the same
    /// mismatch [`Lumen`]'s `about_to_wait` already polls every 4 ms until it
    /// clears). Both are `Task`/`Run` messages queued on the same
    /// `EngineThread` channel as `query`'s own request (`engine_thread.rs`'s
    /// `run_batch` drains and executes a batch in FIFO order), so either one
    /// executing when `query` would be sent means `query` blocks for its full
    /// remaining duration, not just the short rAF-turn case `raf_turn_inflight`
    /// alone used to catch.
    fn should_defer_query(
        raf_turn_inflight: bool,
        job_generation: u64,
        applied_generation: u64,
    ) -> bool {
        raf_turn_inflight || job_generation != applied_generation
    }

    /// ADR-016 M2.3 + THREAD-2: value-returning JS drain that is **deferred**
    /// (returns `None`) while a rAF turn or a submitted relayout job is in
    /// flight on the engine thread (see [`Self::should_defer_query`]). The
    /// parked `about_to_wait` loop issues several blocking `route_query_js`
    /// drains each pass (canvas bitmaps, history/pushState, traversals,
    /// navigation updates); under the flag every one of them would otherwise
    /// FIFO-serialize behind whichever `Run`/`Task` the engine thread is
    /// currently executing — a rAF turn (up to ~200 ms) or a full off-thread
    /// relayout — and freeze the loop, exactly the stall M2.3/THREAD-2 remove.
    /// Skipping a drain merely defers it to the next pass once the in-flight
    /// job finishes: the rAF-turn case keeps the loop warm via its own short
    /// wakeup, the relayout case via the existing 4 ms
    /// `engine_job_generation != engine_applied_generation` poll in
    /// `about_to_wait`. Off the flag both conditions are always `false`, so
    /// this is byte-identical to calling [`route_query_js`] directly.
    pub(crate) fn drain_query_js<R: Send + 'static>(
        &self,
        read: impl FnOnce(&Arc<dyn PersistentJs>) -> R + Send + 'static,
    ) -> Option<R> {
        if Self::should_defer_query(
            self.raf_turn_inflight(),
            self.engine_job_generation,
            self.engine_applied_generation,
        ) {
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
            // FRAME-8: под движковым потоком (default, ADR-023) это —
            // единственное место, где страничный `dom_dirty` потребляется:
            // синхронные ветки `about_to_wait`/`RedrawRequested` для того же
            // сигнала — байт-идентичный fallback, живой только при
            // `LUMEN_NO_ENGINE_THREAD=1`. Довесок к уже добытому флагу, не
            // отдельный опрос — см. doc-comment `frame_dynamic.rs`.
            self.poll_dynamic_frames();
            self.relayout_raf_dirty();
            submitted = true;
        } else if self.engine_job_generation == self.engine_applied_generation
            && self
                .anim_frame
                .as_ref()
                .is_some_and(|f| f.overrides.values().any(|o| o.height.is_some()))
        {
            // GAP-CSSANIM срез 9: a `height` transition/`@keyframes` animation
            // is running with no DOM mutation this pass — the branch above
            // never fires for it, so `getBoundingClientRect()`/
            // `getClientRects()` would otherwise keep reading the box tree
            // from before the animation started (this is the default,
            // ADR-023 engine-thread path — see `animated_heights_snapshot`
            // and its installation in `make_relayout_job`). Same
            // "animating height forces a real reflow" tradeoff real browsers
            // make every such pass, mirroring the synchronous-fallback
            // branch in `RedrawRequested` Step 4.
            //
            // Single-flight guard (`engine_job_generation ==
            // engine_applied_generation`, i.e. no relayout already in
            // flight): `pump_raf_engine_thread` runs far more often than
            // once per rendered frame (every `about_to_wait` wakeup), and
            // this condition stays true for the whole animation duration —
            // without the guard each pass submits a fresh job that bumps
            // `engine_job_generation` and immediately supersedes the
            // previous one (`poll_engine_commit`'s generation check drops
            // it), so the animated height was computed correctly off-thread
            // but its commit never survived to land.
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
    /// BUG-935 S12/S14 measured swapping this order (S14 under the S13 cache-seed
    /// fix) and reverted it both times — see [`Self::relayout_raf_dirty`]'s doc
    /// comment for the confirmed-regression finding, which applies identically
    /// here.
    ///
    /// BUG-935 S19: same outer timer as [`Self::relayout_raf_dirty`], wrapping
    /// the same call site here — see that function's doc comment for why.
    pub(crate) fn relayout_raf_dirty_readback(&mut self) {
        if self.readback_relayout_job() {
            return;
        }
        let outer_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
        let handled = self.try_relayout_raf_incremental();
        if let Some(t0) = outer_t0 {
            eprintln!(
                "[engine] relayout_raf_dirty_readback outer_ms={:.2} (try_relayout_raf_incremental call, handled={handled})",
                t0.elapsed().as_secs_f32() * 1000.0,
            );
        }
        if !handled {
            self.relayout();
        }
    }

    /// Derive the CSS layout viewport for a relayout (shared by the synchronous
    /// [`Self::relayout`] and the off-thread [`Self::submit_relayout_job`]).
    ///
    /// Returns `None` — skip relayout — when there is no `LayoutSource`/renderer
    /// yet or the content region is degenerate (minimized window). Applies the
    /// live chrome inset (RP-2) and the user zoom, matching scroll clamping and
    /// the content `PushTransform`. GAP-VVPORT срез 3: `<meta viewport
    /// initial-scale>` no longer scales this — per CSSOM View, `initial-scale`
    /// sets the ratio between the layout viewport and the *visual* viewport
    /// (`window.visualViewport`, see `meta_viewport_scale` pushed alongside
    /// `zoom_factor` in `apply_relayout_result` below), not the layout viewport
    /// itself. Only real page zoom (Ctrl+=/Ctrl+-/Ctrl+0) reflows the box tree.
    pub(crate) fn relayout_viewport(&self) -> Option<Size> {
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
        let (css_w, css_h) = zoom::effective_viewport(vp_w, vp_h, self.zoom_factor);
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
        self.content_height = content_height_of(&new_dl);
        self.content_width = content_width_of(&new_dl);
        // BUG-480 срез 14: вклейка ПОСЛЕ метрик (содержимое фрейма не должно
        // растягивать прокрутку страницы — обе функции складывают плоский
        // список прямоугольников и клипов не видят) и ДО diff/кэша, чтобы обе
        // стороны сравнения были одинаково склеенными.
        apply_step!("dl_splice_diff_cache", {
            crate::frames::splice_frame_content(&mut new_dl, &self.frames);
            self.tile_grid.update_from_diff(&self.display_list, &new_dl);
            // Cache display list directly (avoid &mut self while layout_source is borrowed).
            let _dl_hash = lumen_paint::hash_commands(&new_dl);
            self.display_list_cache.insert(lb.node.index() as u32, new_dl.clone(), _dl_hash, None);
        });
        // Поля пишутся напрямую (не через `set_display_list`): `layout_source`
        // здесь заимствован, `&mut self` целиком взять нельзя.
        self.display_list = new_dl;
        self.display_list_epoch = next_dl_epoch(self.display_list_epoch);
        // Sync transitions: compare prev styles with new layout before replacing.
        let now_s = self.epoch.elapsed().as_secs_f32();
        let mut new_styles = HashMap::new();
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
            let js_data = if self.js_present {
                match precollected {
                    Some(data) => Some(data),
                    None => (|| {
                        let lb_ref = self.layout_box.as_ref()?;
                        let doc_guard =
                            self.layout_source.as_ref()?.document.lock().ok()?;
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
                        Some(apply_step!(
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
                        ))
                    })(),
                }
            } else {
                None
            };
            if let Some(PrecollectedJsData {
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
                next_layout_shift_baseline,
            }) = js_data
            {
                self.prev_layout_shift_rects = next_layout_shift_baseline;
                let (vw, vh) = (viewport.width, viewport.height);
                let zoom_factor = self.zoom_factor;
                let dark_mode = self.dark_mode;
                let reduced_motion = self.a11y_store.reduced_motion();
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
                            js.deliver_media_query_changes(vw, vh, dark_mode, reduced_motion)
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
                } else {
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
                        js.deliver_media_query_changes(vw, vh, dark_mode, reduced_motion);
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

    /// BUG-935 S26: independent drain for [`Self::pending_lazy_image_reqs`],
    /// called once per `about_to_wait` pass regardless of whether a relayout
    /// runs this tick. S25 found the queue's only consumer was the `if let`
    /// inside [`Self::apply_relayout_result`] (`:1054`) — safe as long as
    /// *some* later relayout is guaranteed to happen, which is true for
    /// [`Self::try_relayout_raf_incremental`] (the only `defer_js_push=true`
    /// producer, called from a continuous rAF+DOM-mutation loop) but not for
    /// a page's *last* relayout ever, after which the queue would sit
    /// forever unfetched. Parking this here — the per-tick pump every
    /// producer already routes through — closes that gap independently of
    /// which relayout path fires next or whether one fires again at all.
    #[cfg(feature = "v8")]
    pub(crate) fn drain_pending_lazy_image_reqs(&mut self) {
        let reqs = take_pending_lazy_image_reqs(&self.pending_lazy_image_reqs);
        if !reqs.is_empty() {
            self.fetch_and_register_lazy_images(reqs);
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
            lumen_layout::set_cv_scroll(cv_x, cv_y);
            lumen_layout::set_cv_relevant(cv_relevant);
            lumen_layout::set_animated_heights(animated_heights);
            let (content, layout_box) =
                compute_layout(&document, &stylesheet, viewport, &*hp, dark_mode, &web_fonts);
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
        let EngineCommit { content, layout_box, viewport, compute_ms, .. } = commit;
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
        // BUG-935 S43: lock-free clones of the "has JS ever read
        // pseudo-styles/custom-properties" flags, cached in both engine-thread
        // modes (unlike `raf_pending_flag`/`dom_dirty_flag` below, which the
        // no-engine-thread branch skips because it can read `self.js_ctx`
        // directly) — `make_relayout_job` builds its engine-thread closure on
        // this (UI) thread regardless of mode, so it always needs a
        // ready-to-move snapshot rather than a live handle to query from. `None`
        // clears them (blank/JS-less tab).
        self.pseudo_styles_needed_flag = handle.as_ref().and_then(|h| h.pseudo_styles_needed_flag());
        self.custom_props_needed_flag = handle.as_ref().and_then(|h| h.custom_props_needed_flag());
        self.computed_styles_needed_flag = handle.as_ref().and_then(|h| h.computed_styles_needed_flag());
        self.cascade_feed = handle.as_ref().and_then(|h| h.cascade_feed());
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
        measurer.register_family_with_overrides(
            &wf.family,
            wf.bytes.clone(),
            wf.unicode_range.clone(),
            wf.ascent_override,
            wf.descent_override,
            wf.size_adjust,
            wf.line_gap_override,
        );
    }
    measurer.set_system_faces(system_font_faces());
    measurer
}

/// Измеритель для документа хрома (BUG-625): тот же резолв семейств, что у
/// рендера хрома — зарезервированные bundled-имена (`Golos Text`,
/// `Golos Text Medium`, `JetBrains Mono`, DS-4) плюс системный набор для
/// остального стека (`--font-ui` без установленного Inter уходит в Segoe UI).
///
/// До BUG-625 хром мерился голым [`lumen_paint::FontMeasurer`], который
/// выбрасывает `font-family` и всё меряет bundled Inter-ом, тогда как рисуется
/// хром по объявленному стеку. Веб-шрифтов у хрома нет, поэтому измеритель
/// процесс-глобальный: хром перекладывается на каждый hover, а парсить три
/// bundled-файла на каждый проход незачем. `None` — bundled Inter не
/// разобрался (недостижимо для `include_bytes!`-ассета); вызывающий пропускает
/// проход, как при любом другом отсутствии входа.
pub(crate) fn chrome_measurer() -> Option<&'static lumen_paint::MultiFontMeasurer> {
    static SHARED: std::sync::OnceLock<Option<lumen_paint::MultiFontMeasurer>> =
        std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| {
            let font = lumen_font::Font::parse(INTER_FONT).ok()?;
            let mut measurer = lumen_paint::MultiFontMeasurer::new(&font).ok()?;
            measurer.register_chrome_bundled_families();
            measurer.set_system_faces(system_font_faces());
            Some(measurer)
        })
        .as_ref()
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
/// [`lumen_layout::layout_mutation_incremental_with_counters`] to skip geometry
/// re-computation for subtrees whose [`lumen_layout::ComputedStyle`] is
/// unchanged, while preserving full cascade and post-layout passes. `prev` is
/// the previously laid-out tree stored in `Lumen::layout_box`.
///
/// BUG-935 S13: also returns the [`lumen_layout::CounterMap`] the full cascade
/// this path always runs produced — previously discarded, which is why
/// [`Lumen::try_relayout_raf_incremental`]'s non-restyle branch had nothing to
/// seed [`Lumen::page_prev_cascade_styles`] with for the next cycle. The
/// caller persists `counters.into_styles()` there.
pub(crate) fn relayout_page_incremental(
    src: &LayoutSource,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    web_fonts: &[LoadedWebFont],
    prev: &lumen_layout::LayoutBox,
) -> (DisplayList, lumen_layout::LayoutBox, lumen_layout::CounterMap) {
    compute_layout_incremental(&src.document, &src.stylesheet, viewport, hp, dark_mode, web_fonts, prev)
}

/// ADR-016 M4: incremental variant of [`compute_layout`] — runs the full
/// cascade but reuses geometry from `prev` for unchanged subtrees.
///
/// Same caller contract as [`compute_layout`]: thread-local interactive state
/// must be set before the call and cleared afterwards. See
/// [`relayout_page_incremental`] for why the returned [`lumen_layout::CounterMap`]
/// matters (BUG-935 S13).
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
) -> (DisplayList, lumen_layout::LayoutBox, lumen_layout::CounterMap) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = page_measurer(&font, web_fonts);
    let doc = document.lock().unwrap();
    let (layout, counters) = lumen_layout::layout_mutation_incremental_with_counters(
        &doc, stylesheet, viewport, &measurer, hp, dark_mode, prev,
    );
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout, counters)
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
    // BUG-935 S15: S14's <1ms-profiled `layout_mutation_incremental_restyle`
    // stages sit far below the multi-hundred-ms/multi-second `[engine]
    // relayout` total the caller reports — the gap must be in this wrapper,
    // which the profile tree does not cover. Two unprofiled candidates live
    // here: `Font::parse` re-parses the bundled font on *every* incremental
    // call (fixed per-call tax, not contention), and `document.lock()` below
    // is a *second*, independent lock acquisition from the one the caller
    // (`try_relayout_raf_incremental`) already dropped before calling in —
    // it can block on the same engine-thread contention S14 suspected, just
    // one level deeper than S14 measured.
    let log_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let font_parse_ms = log_t0.map(|t| t.elapsed().as_secs_f32() * 1000.0);
    let measurer = page_measurer(&font, web_fonts);
    let lock2_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
    let doc = document.lock().unwrap();
    let lock2_wait_ms = lock2_t0.map(|t| t.elapsed().as_secs_f32() * 1000.0);
    let (layout, counters) = lumen_layout::box_tree::layout_mutation_incremental_restyle(
        &doc, stylesheet, viewport, &measurer, hp, dark_mode, prev, delta,
    );
    drop(doc);
    let paint_t0 = lumen_paint::frame_log_enabled().then(std::time::Instant::now);
    let dl = paint_ordered(&layout);
    if let Some(t0) = log_t0 {
        let paint_ms = paint_t0.map(|t| t.elapsed().as_secs_f32() * 1000.0).unwrap_or(0.0);
        eprintln!(
            "[engine] restyle-wrapper {:.2}ms total: font_parse={:.2}ms lock2_wait={:.2}ms paint_ordered={paint_ms:.2}ms",
            t0.elapsed().as_secs_f32() * 1000.0,
            font_parse_ms.unwrap_or(0.0),
            lock2_wait_ms.unwrap_or(0.0),
        );
    }
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

/// GAP-LAYOUTSHIFT (BUG-809): the visible-area of one rect clipped to the
/// `[0, 0, vw, vh]` viewport — the building block "impact region" is made
/// of (Layout Instability L1 §3.1).
fn clip_area_to_viewport(rect: [f32; 4], vw: f32, vh: f32) -> f32 {
    let x0 = rect[0].max(0.0);
    let y0 = rect[1].max(0.0);
    let x1 = (rect[0] + rect[2]).min(vw);
    let y1 = (rect[1] + rect[3]).min(vh);
    (x1 - x0).max(0.0) * (y1 - y0).max(0.0)
}

/// GAP-LAYOUTSHIFT (BUG-809): score one relayout's rect changes the way
/// Layout Instability L1 §3.1 scores a frame — `impact_fraction ×
/// distance_fraction` — from a `prev`/`next` snapshot pair of
/// [`lumen_layout::collect_layout_rects`] output.
///
/// A node present in only one snapshot (entered/left the tree, e.g. through
/// `display: none` or a DOM mutation) is not a shift and is skipped — the
/// spec scores *moved* elements only. A node whose rect moved less than
/// half a CSS px is treated as unchanged (sub-pixel layout jitter, not a
/// visible shift).
///
/// **Approximation, not the spec's exact geometry**: real §3.1 unions the
/// old+new rects of every unstable element into one non-overlapping impact
/// region before dividing by the viewport area; this sums each element's own
/// clipped area instead, so a page whose several elements shift within
/// overlapping regions double-counts that overlap (`impact_fraction` is
/// clamped to 1.0, so it caps rather than diverges). Good enough to turn
/// "no delivery at all" into a real score for the common one-or-few-elements
/// shift case (`simple-block-movement.html`, `cls-shift` probe variants).
/// `sources` ranks the shifted nodes by their own clipped impact area,
/// largest first, capped at [`LAYOUT_SHIFT_MAX_SOURCES`] (§4.2's "at most
/// five largest") — an approximation of the spec's attribution list, not an
/// exact match, since the spec ranks by the *unioned* region a node
/// contributes rather than each node's standalone area.
pub(crate) fn compute_layout_shift_score(
    prev: &std::collections::HashMap<u32, [f32; 4]>,
    next: &std::collections::HashMap<u32, [f32; 4]>,
    viewport_w: f32,
    viewport_h: f32,
) -> LayoutShiftResult {
    if viewport_w <= 0.0 || viewport_h <= 0.0 {
        return LayoutShiftResult::default();
    }
    let mut impact_area = 0.0f64;
    let mut max_distance_frac = 0.0f64;
    let mut shifted: Vec<(LayoutShiftSource, f64)> = Vec::new();
    for (node, new_rect) in next {
        let Some(old_rect) = prev.get(node) else { continue };
        let dx = new_rect[0] - old_rect[0];
        let dy = new_rect[1] - old_rect[1];
        if dx.abs() <= 0.5 && dy.abs() <= 0.5 {
            continue;
        }
        let old_area = clip_area_to_viewport(*old_rect, viewport_w, viewport_h);
        let new_area = clip_area_to_viewport(*new_rect, viewport_w, viewport_h);
        let node_area = old_area.max(new_area) as f64;
        impact_area += node_area;
        shifted.push((
            LayoutShiftSource {
                node: *node,
                previous_rect: *old_rect,
                current_rect: *new_rect,
            },
            node_area,
        ));
        let dist_frac = dx.abs().max(dy.abs()) as f64 / viewport_w.max(viewport_h) as f64;
        if dist_frac > max_distance_frac {
            max_distance_frac = dist_frac;
        }
    }
    if impact_area <= 0.0 {
        return LayoutShiftResult::default();
    }
    let impact_fraction = (impact_area / (viewport_w as f64 * viewport_h as f64)).min(1.0);
    shifted.sort_by(|a, b| b.1.total_cmp(&a.1));
    shifted.truncate(LAYOUT_SHIFT_MAX_SOURCES);
    LayoutShiftResult {
        score: impact_fraction * max_distance_frac,
        sources: shifted.into_iter().map(|(source, _)| source).collect(),
    }
}

/// Layout Instability §4.2: "at most five" largest sources per entry.
const LAYOUT_SHIFT_MAX_SOURCES: usize = 5;

/// Result of [`compute_layout_shift_score`] — a CLS score plus the node ids
/// behind it, for `entry.sources[]` attribution.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LayoutShiftResult {
    pub(crate) score: f64,
    pub(crate) sources: Vec<LayoutShiftSource>,
}

/// GAP-LAYOUTSHIFT срез 3 (BUG-809): one `entry.sources[]` attribution
/// entry — the node plus its pre-/post-shift border-box geometry
/// (`LayoutShiftAttribution.previousRect`/`currentRect`, L1 §4.2), both in
/// `collect_layout_rects`'s `[x, y, width, height]` layout-space form (not
/// yet clipped to the viewport — the JS side builds the `DOMRectReadOnly`).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct LayoutShiftSource {
    pub(crate) node: u32,
    pub(crate) previous_rect: [f32; 4],
    pub(crate) current_rect: [f32; 4],
}

/// BUG-935 S26: the pure half of [`Lumen::drain_pending_lazy_image_reqs`] —
/// swaps the queued `<img loading=lazy>` requests out of the shared slot,
/// leaving it empty, regardless of whether the lock is currently held by a
/// poisoned prior panic (`Err` case: the slot's contents are unrecoverable
/// either way, so this returns empty rather than propagating the poison).
/// Split out so the drain decision is testable without a live `Lumen`/network
/// stack — same rationale as [`Lumen::should_defer_query`] below.
pub(crate) fn take_pending_lazy_image_reqs(
    slot: &std::sync::Arc<std::sync::Mutex<Vec<(u32, String)>>>,
) -> Vec<(u32, String)> {
    match slot.lock() {
        Ok(mut guard) => std::mem::take(&mut *guard),
        Err(_) => Vec::new(),
    }
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
    pub(crate) styles: Option<std::collections::HashMap<u32, std::collections::HashMap<String, String>>>,
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

/// BUG-935 S41: the pure collection step factored out of
/// [`Lumen::apply_relayout_result`] so [`Lumen::make_relayout_job`] can run
/// it on the engine thread instead — S40 found this already the dominant
/// UI-thread cost of every commit, off-thread or not. Depends only on the
/// freshly computed layout tree, the (locked) document and a layout-shift
/// baseline snapshot — no `Lumen` field, so it needs neither `&self` nor the
/// renderer/frame state.
#[cfg(feature = "v8")]
#[allow(clippy::too_many_arguments)]
fn collect_js_data(
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
    let rects = step!("collect_layout_rects", collect_layout_rects(lb_ref, &doc_guard));
    let shift_rects = step!(
        "collect_layout_shift_rects",
        lumen_layout::collect_layout_shift_rects(lb_ref)
    );
    let layout_shift = step!(
        "layout_shift_score",
        compute_layout_shift_score(prev_layout_shift_rects, &shift_rects, viewport.width, viewport.height)
    );
    let had_input = now_s - last_input_epoch_s < 0.5;
    let client_rects = step!("collect_client_rects", collect_client_rects(lb_ref, &doc_guard));
    let hit_test_tree = step!("clone_hit_test_tree", Arc::new(lb_ref.clone()));
    let styles = if computed_styles_needed {
        Some(step!("collect_computed_styles", collect_computed_styles(lb_ref, &doc_guard, None)))
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
        layout_shift_score: layout_shift.score,
        layout_shift_sources: layout_shift.sources,
        had_input,
        client_rects,
        hit_test_tree,
        styles,
        pseudo_styles,
        customs,
        scroll_states,
        next_layout_shift_baseline: shift_rects,
    }
}

#[cfg(test)]
mod thread2_defer_query_tests {
    use super::Lumen;

    // ADR-016 THREAD-2: `should_defer_query` is the pure decision behind
    // `Lumen::drain_query_js`'s defer guard — exercised directly here because
    // `drain_query_js` itself needs a live `Lumen`/engine thread to call.

    #[test]
    fn does_not_defer_when_idle() {
        assert!(!Lumen::should_defer_query(false, 3, 3));
    }

    #[test]
    fn defers_while_raf_turn_inflight() {
        assert!(Lumen::should_defer_query(true, 3, 3));
    }

    #[test]
    fn defers_while_relayout_job_unapplied() {
        // job_generation (4) ahead of applied_generation (3): a submitted
        // `Run` has not landed yet — the case `raf_turn_inflight` alone missed.
        assert!(Lumen::should_defer_query(false, 4, 3));
    }

    #[test]
    fn resumes_once_relayout_job_applied() {
        // `poll_engine_commit` advanced `applied_generation` to match — the
        // engine thread is free again, `query` is safe to call.
        assert!(!Lumen::should_defer_query(false, 4, 4));
    }

    #[test]
    fn defers_when_both_conditions_hold() {
        assert!(Lumen::should_defer_query(true, 4, 3));
    }
}

#[cfg(test)]
mod bug935_s26_pending_lazy_image_drain_tests {
    use super::take_pending_lazy_image_reqs;
    use std::sync::{Arc, Mutex};

    #[test]
    fn empty_slot_drains_to_empty() {
        let slot = Arc::new(Mutex::new(Vec::new()));
        assert!(take_pending_lazy_image_reqs(&slot).is_empty());
    }

    #[test]
    fn drain_takes_everything_and_leaves_slot_empty() {
        let slot = Arc::new(Mutex::new(vec![
            (1u32, "a.png".to_string()),
            (2u32, "b.png".to_string()),
        ]));
        let reqs = take_pending_lazy_image_reqs(&slot);
        assert_eq!(reqs, vec![(1, "a.png".to_string()), (2, "b.png".to_string())]);
        assert!(
            slot.lock().expect("лок").is_empty(),
            "S25's independent-drain gap: a stale entry left behind here would \
             sit forever if no later relayout happens to pick it up"
        );
    }

    #[test]
    fn second_drain_after_first_is_empty() {
        let slot = Arc::new(Mutex::new(vec![(1u32, "a.png".to_string())]));
        let _first = take_pending_lazy_image_reqs(&slot);
        assert!(take_pending_lazy_image_reqs(&slot).is_empty());
    }
}
