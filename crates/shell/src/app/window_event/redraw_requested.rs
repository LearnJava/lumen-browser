//! Ветка `WindowEvent::RedrawRequested` цикла событий (SPLIT-SH2).
//!
//! Тело ветки вынесено из `Lumen::window_event` (`main.rs`) как есть, с
//! дедентом на 8 пробелов и без единой правки логики; строки внутри
//! многострочных строковых литералов дедент не затронул.

use crate::*;

impl Lumen {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn on_redraw_requested(&mut self) {
        // HTML §8.1.5.1 «Update the rendering» — spec-correct order:
        //   1.   scroll              ← advance_scroll_anim + advance_momentum
        //   1.5  scroll-driven anims ← deliver_scroll_progress → ScrollTimeline.currentTime
        //   2.   CSS Animations + Transitions tick  (spec: update animations before rAF)
        //   3.   rAF callbacks       ← runtime.run_rendering_step + JS run_animation_frame
        //   4.   layout invalidation ← relayout() if dom_dirty after rAF
        //        → deliver_layout_observers() (ResizeObserver + IntersectionObserver)
        //   5.   paint timing        ← PerformanceObserver 'paint' entries
        //   6.   paint               ← r.render(...)
        //
        // Scroll before CSS/rAF so callbacks read current scroll position.
        // CSS animations/transitions before rAF: spec §8.1.5.1 step «update
        // animations and send events» precedes «run animation frame callbacks».
        let timestamp_ms =
            self.epoch.elapsed().as_secs_f64() * 1000.0;

        // Диагностика кадра (LUMEN_FRAME_LOG=1): полное время
        // RedrawRequested (шаги 1–6). Paint-фаза отдельно логируется
        // бэкендом строкой `[frame] paint …`.
        let frame_log_t0 =
            lumen_paint::frame_log_enabled().then(std::time::Instant::now);

        // Warm-frame bench (LUMEN_BENCH): таймер всего RedrawRequested,
        // включая skip-путь — на нём как раз и меряется цена решения
        // «не рисовать» (см. crates/shell/src/bench_frames.rs).
        let bench_t0 = bench_frames::active().then(std::time::Instant::now);

        // Step 1: scroll update.
        if self.advance_scroll_anim() {
            self.request_redraw();
        }
        if self.advance_momentum(timestamp_ms) {
            self.request_redraw();
        }
        // Sync window.scrollY to current scroll_y so JS reads are accurate.
        // ADR-016 M2.2c-2d: fire-and-forget push via route_task_js
        // (off-UI-thread under LUMEN_ENGINE_THREAD=1, byte-identical sync
        // call when off); scroll_y is read into a local before routing so
        // the closure does not re-borrow `self`.
        //
        // BUG-821: this is also CSSOM-View §14 «run the scroll steps» —
        // the one place that sees *every* page-scroll movement, whatever
        // started it (wheel, keys, scrollbar drag, touch momentum,
        // find-in-page, `window.scrollTo`). The `scroll` event is bound
        // to the position changing since the last rendering update, not
        // to an input device: before this, `fire_window_scroll` had a
        // single call site in the mouse-wheel branch, so a programmatic
        // scroll moved the page and told nobody.
        //
        // BUG-822: `scrollend` is the same step's second half. It is due
        // once the sequence has *stopped*, so it is gated on nothing
        // still driving the position — a smooth animation, touch
        // momentum, or a scrollbar thumb held under the cursor. An
        // instant scroll (`window.scrollTo`, find-in-page, a key jump
        // that lands immediately) is `moved && settled` and gets both
        // events in this one frame, which CSSOM-View §14 allows; an
        // animated one gets `scroll` per frame and a single `scrollend`
        // on the update that finished it, because `advance_scroll_anim`
        // /`advance_momentum` clear their animation on the very frame
        // they last move the page. Known imprecision: a touchpad gesture
        // still being dragged has no "gesture active" flag here, so a
        // pause mid-gesture can end one sequence and start another.
        #[cfg(feature = "v8")]
        {
            let scroll_y = self.scroll_y;
            let settled = self.scroll_anim.is_none()
                && self.momentum_anim.is_none()
                && self.scroll_drag.is_none();
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                let moved = j.set_page_scroll_y(scroll_y);
                if moved {
                    j.fire_window_scroll();
                }
                if j.page_scrollend_due(moved, settled) {
                    j.fire_window_scrollend();
                }
            });
        }
        // ADR-008 §10E.4: after scroll, evict CPU-decoded images beyond gate zone.
        self.try_discard_offscreen_images();
        // Step 1.6: content-visibility: auto (BB-4) — пропущенный узел
        // вошёл в расширенный viewport → ratchet relevant + relayout.
        self.maybe_expand_cv_relevant();
        // Step 1.65 (BUG-852): и здесь же — единственная точка выдачи
        // `contentvisibilityautostatechange`. CSS Contain L2 §4.1
        // определяет релевантность внутри «update the rendering» и
        // просит поставить событие задачей; очередь наполняют четыре
        // вызова `refresh_cv_state` (загрузка, релейаут, восстановление,
        // ratchet выше), а JS-контекст к этому шагу уже есть на всех
        // путях — на двух из них в момент самого `refresh_cv_state` ещё
        // нет.
        #[cfg(feature = "v8")]
        self.deliver_cv_state_changes();
        // Step 1.7 (BUG-735): картинки, декодированные streaming/
        // динамическим путём с прошлого кадра, отдают DOM-у свои
        // intrinsic-размеры (коалесцированно: одна пачка — один релейаут).
        self.apply_stream_intrinsic_sizes();

        // Fast-scroll деградация (EXPERIMENT.md §2 срез 2, принцип
        // пользователя 2026-07-10: чем быстрее скролл, тем меньше
        // пользователю важно содержимое). При быстром скролле
        // замораживаются ИСТОЧНИКИ изменений контента — тики
        // CSS-анимаций/transitions (Step 2), GIF (Step 2.5) и
        // video-GIF (Step 2.6). Display list становится
        // scroll-стабильным, и кадр скролла уходит в page-compose HIT
        // (~2 мс) вместо монолитной перерисовки. Анимации time-based:
        // при выходе из режима они сами догоняют текущее время,
        // «пауза» видна только во время быстрой прокрутки.
        // Гистерезис по EMA-скорости: вход ≥48 CSS px/кадр (полный
        // wheel-notch за кадр), выход <12. Разовая прокрутка колёсиком
        // даёт одну-две замороженных пары кадров, плавный трекпад не
        // входит в режим вовсе. LUMEN_NO_FAST_SCROLL_DEGRADE=1 — выкл.
        let scroll_step = (self.scroll_y - self.last_frame_scroll_y).abs();
        self.last_frame_scroll_y = self.scroll_y;
        self.scroll_velocity = 0.6 * self.scroll_velocity + 0.4 * scroll_step;
        self.fast_scroll = !fast_scroll_degrade_disabled()
            && if self.fast_scroll {
                self.scroll_velocity >= 12.0
            } else {
                self.scroll_velocity >= 48.0
            };
        let freeze_content_ticks = self.fast_scroll;
        if freeze_content_ticks
            && (self.anim_frame.is_some() || !self.animated_gifs.is_empty())
        {
            // Замороженным источникам нужен живой цикл кадров: на
            // кадре, где скорость упадёт ниже порога, тики
            // возобновятся и анимации продолжатся.
            self.request_redraw();
        }

        // BUG-405 срез 34 (пункт 68 остатка): разбивка кадра по шагам
        // `RedrawRequested`. `[frame] total` меряет весь handler, а
        // `[frame:wgpu] total` — только пасс композитора; на кадре
        // ПОПАДАНИЯ полосы между ними 3.6 из 4.3 мс, и у них не было
        // ни одной статьи. Метки берутся только при `LUMEN_FRAME_LOG`
        // (та же `Option<Instant>`, что и у `total`), поэтому штатный
        // путь не платит за них ничего.
        let mut marks = [0.0_f64; 6];
        if let Some(t0) = frame_log_t0 {
            marks[0] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Step 1.5: CSS Scroll-Driven Animations — update ScrollTimeline.currentTime.
        // Spec §8.1.5.1 step «update scroll-linked animations» precedes CSS animations.
        // Compute root-viewport block/inline progress and deliver to JS.
        {
            let (p_y, p_x) = if let Some(lb) = &self.layout_box {
                let vp = lumen_layout::Viewport {
                    width: self.viewport_width_css(),
                    height: self.viewport_height_css(),
                };
                let tl_y = lumen_layout::ScrollTimeline {
                    element: None,
                    axis: lumen_layout::ScrollAxis::Block,
                };
                let tl_x = lumen_layout::ScrollTimeline {
                    element: None,
                    axis: lumen_layout::ScrollAxis::Inline,
                };
                (
                    lumen_layout::resolve_scroll_progress(&tl_y, lb, self.scroll_x, self.scroll_y, vp),
                    lumen_layout::resolve_scroll_progress(&tl_x, lb, self.scroll_x, self.scroll_y, vp),
                )
            } else {
                (0.0_f32, 0.0_f32)
            };
            // ADR-016 M2.2c-2d: fire-and-forget scroll-progress delivery via
            // route_task_js (off-UI-thread under LUMEN_ENGINE_THREAD=1,
            // byte-identical sync call when off).
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                j.deliver_scroll_progress(p_y, p_x);
            });
        }

        if let Some(t0) = frame_log_t0 {
            marks[1] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Step 2: CSS Animations + Transitions tick (spec order: before rAF).
        // Both schedulers are ticked once per frame and merged into a single
        // AnimationFrame. Transition values override @keyframes when both apply.
        // При fast-scroll тик пропускается: anim_frame остаётся с прошлыми
        // значениями → пересобранный anim_dl идентичен → ключ полосы стабилен.
        if !freeze_content_ticks
            && let (Some(lb), Some(src)) = (&self.layout_box, &self.layout_source)
        {
            let vp = lumen_layout::Viewport {
                width: self.viewport_width_css(),
                height: self.viewport_height_css(),
            };
            let mut frame = self.animation_scheduler.tick(
                timestamp_ms,
                lb,
                &src.stylesheet,
                self.scroll_x,
                self.scroll_y,
                vp,
            );
            let now_s = (timestamp_ms / 1000.0) as f32;
            let trans_frame = self.transition_scheduler.tick(now_s);
            frame.merge_from(trans_frame);
            if frame.has_active {
                self.request_redraw();
            }
            self.anim_frame = if frame.overrides.is_empty() { None } else { Some(frame) };
        }

        // Step 2b (CC-11, docs/tasks/p1-css-chrome.md): the chrome
        // document's own Animations + Transitions tick — separate
        // schedulers from the page's (see
        // Self::chrome_animation_scheduler doc comment for why),
        // same merge-and-request-redraw pattern. Not gated on
        // freeze_content_ticks: chrome isn't affected by page
        // fast-scroll degradation (its own document doesn't scroll
        // with the page).
        if let (Some((c_lb, _)), Some((_, c_sheet))) = (&self.chrome_layout, &self.chrome_doc)
        {
            let vp = lumen_layout::Viewport {
                width: self.viewport_width_css(),
                height: self.viewport_height_css(),
            };
            let mut c_frame = self.chrome_animation_scheduler.tick(
                timestamp_ms,
                c_lb,
                c_sheet,
                0.0,
                0.0,
                vp,
            );
            let now_s = (timestamp_ms / 1000.0) as f32;
            let c_trans_frame = self.chrome_transition_scheduler.tick(now_s);
            c_frame.merge_from(c_trans_frame);
            if c_frame.has_active {
                self.request_redraw();
            }
            self.chrome_anim_frame =
                if c_frame.overrides.is_empty() { None } else { Some(c_frame) };
        }

        // Step 2.5: GIF animation — update GPU textures for frames that changed.
        // Uses the same `epoch` as rAF timestamps so GIF timing is consistent
        // with CSS animations and JS. Runs before rAF so JS can read correct img.
        // При fast-scroll кадры GIF не обновляются (register_image бампает
        // content_generation и убивал бы ключ полосы каждый тик).
        if !freeze_content_ticks && !self.animated_gifs.is_empty() {
            let elapsed_ms = self.epoch.elapsed().as_millis() as u64;

            // Collect (url, frame_idx, frame_image) for frames that changed.
            let updates: Vec<(String, usize, lumen_image::Image)> = {
                let gifs = &self.animated_gifs;
                let last = &self.gif_last_frame;
                gifs.iter()
                    .filter_map(|(url, gif)| {
                        let idx = gif.frame_index_at(elapsed_ms);
                        if last.get(url).copied().unwrap_or(usize::MAX) != idx {
                            gif.frame_image(idx).ok().map(|img| (url.clone(), idx, img))
                        } else {
                            None
                        }
                    })
                    .collect()
            };

            for (url, idx, image) in updates {
                if let Some(r) = self.renderer.as_mut()
                    && let Err(e) = r.register_image(url.clone(), Arc::new(image))
                {
                    eprintln!("GIF кадр {url}[{idx}]: не зарегистрирован: {e}");
                }
                self.gif_last_frame.insert(url, idx);
            }

            // Request next redraw if any GIF still has more frames to show.
            let gif_animating = {
                let gifs = &self.animated_gifs;
                gifs.values().any(|gif| match gif.loop_count {
                    lumen_image::GifLoopCount::Infinite => gif.frame_count() > 1,
                    lumen_image::GifLoopCount::Finite(n) => {
                        let total_ms: u64 = gif.total_cycle_ms();
                        elapsed_ms < total_ms.saturating_mul(u64::from(n))
                    }
                })
            };
            if gif_animating {
                self.request_redraw();
            }
        }

        // Step 2.6: Video GIF animation — drain pending loads, advance frames.
        // Заморожено при fast-scroll по той же причине, что и Step 2.5.
        if !freeze_content_ticks {
            let video_elapsed_ms = self.epoch.elapsed().as_millis() as u64;
            self.tick_video_gifs(video_elapsed_ms);
        }

        if let Some(t0) = frame_log_t0 {
            marks[2] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Step 3: rAF callbacks + microtask checkpoint.
        self.runtime.run_rendering_step(timestamp_ms);

        // Step 3.1: JS requestAnimationFrame callbacks.
        // Vsync gate (EE-5): fire rAF at most once per RAF_MIN_INTERVAL_MS (~16.67ms).
        // Multiple requestAnimationFrame() calls within one frame period are coalesced
        // into a single batch (snapshot-pattern in JS shim). When RedrawRequested fires
        // faster than vsync (e.g. from scroll), we defer the batch without losing the
        // "pending" signal so it fires on the next eligible frame.
        // Pass -1.0 → JS captures performance.now() at batch start (DOMHighResTimeStamp).
        // Pass 0.0 in deterministic mode → frozen timestamp per HTML §8.1.5.1.
        // ADR-016 M2.2c-2d: снимаем прямые `self.js_ctx`-обращения rAF-батча
        // (`has_raf_pending` read → `route_query_js`, `run_animation_frame` void
        // → `route_task_js`). Под флагом (`LUMEN_ENGINE_THREAD=1`) чтения —
        // блокирующий `query`, батч — `task` в очередь между ними (порядок
        // has_raf_pending → take_raf_pending → run_animation_frame сохранён, а
        // последующий Step 4 `take_dom_dirty`-query встаёт после батч-`task`);
        // без флага (по умолчанию) — прежние синхронные вызовы, байт-идентично.
        let raf_due = timestamp_ms - self.last_raf_batch_ms >= RAF_MIN_INTERVAL_MS;
        if self.engine_thread.is_some() {
            // ADR-016 M2.3 (flag on): async rAF pump — never block the
            // redraw on the JS turn. `pump_raf_engine_thread` fires the
            // batch off-thread (guarded so at most one 200 ms turn is in
            // flight, regardless of scroll cadence) and, when a completed
            // turn left the DOM dirty, submits an **async** relayout whose
            // result lands via `poll_engine_commit`. Step 5's
            // `display_list.is_empty()` read may then latch paint-timing a
            // frame late (acceptable under the async contract) — the
            // decisive win is that scroll no longer stalls behind the turn.
            if self.pump_raf_engine_thread(raf_due, timestamp_ms) {
                self.request_redraw();
            }
        } else {
            // Flag off (default): byte-identical synchronous path.
            if raf_due
                && route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.has_raf_pending()
                })
                .unwrap_or(false)
            {
                // Consume the flag and fire the batch (former direct call).
                route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.take_raf_pending()
                });
                self.last_raf_batch_ms = timestamp_ms;
                let raf_ts = if self.deterministic.enabled { 0.0 } else { -1.0 };
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.run_animation_frame(raf_ts);
                });
            }
            // BUG-271: callbacks that remain queued (animation loop or deferred
            // batch) are fired by the `about_to_wait` rAF pump on a WaitUntil timer
            // — NOT by an unconditional `request_redraw` here. A pure rAF loop that
            // never mutates the DOM must not force a 60 fps repaint cycle; loops that
            // do mutate get a relayout + real paint from the pump.

            // Step 4: layout invalidation — если rAF-callback изменил DOM
            // (setAttribute/textContent/appendChild/etc.), делаем relayout
            // прежде чем красить, чтобы paint отражал актуальный DOM.
            // relayout() also delivers ResizeObserver + IntersectionObserver.
            // Step 5 below reads `display_list.is_empty()` synchronously
            // (PerformancePaintTiming), so this reflow is the synchronous
            // `relayout` (no engine thread to defer to).
            if route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                j.take_dom_dirty()
            })
            .unwrap_or(false)
            {
                self.relayout_raf_dirty_readback();
            }
        }

        // Launch->first-frame metric (§4 score table): fires on the
        // first frame that has page content, present happens at the
        // end of this same handler (±1 frame accuracy is enough).
        bench_frames::log_first_frame_once(self.display_list.len());

        if let Some(t0) = frame_log_t0 {
            marks[3] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Step 5: PerformancePaintTiming (W3C Paint Timing §2).
        // Delivered once per page load; subsequent frames skip this block.
        // first-paint = first frame with any painted pixel (non-default bg).
        // first-contentful-paint = first frame with text, image, canvas, etc.
        // Phase 0: both fire on the first non-empty display list since
        // a page load. A page load resets both flags in apply_loaded_page.
        // ADR-016 M2.2c-2d: the `is_some()` gate is preserved (the delivered
        // flags must only latch when a JS context exists — byte-identical to
        // the former `if let Some(js)`); the actual paint-timing calls are
        // fire-and-forget void, routed off-UI-thread under the flag.
        #[cfg(feature = "v8")]
        if self.js_present {
            let has_content = !self.display_list.is_empty();
            if has_content && !self.first_paint_delivered {
                self.first_paint_delivered = true;
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.deliver_paint_timing("first-paint", timestamp_ms);
                });
            }
            if has_content && !self.first_contentful_paint_delivered {
                self.first_contentful_paint_delivered = true;
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.deliver_paint_timing("first-contentful-paint", timestamp_ms);
                });
            }
        }

        // BUG-405 срез 37: подстатьи шага 6. Статья `build` кадра
        // ПОПАДАНИЯ (0.22–0.32 мс по срезу 34) — вторая по величине
        // после композитного пасса и входила туда одним числом.
        // `chrome` — раскладка хрома по клип-полосам вокруг страницы,
        // `sbar` — полоса прокрутки, `panels` — все остальные
        // overlay-строители, остаток до `marks[4]` — хвост
        // (split-view, инспектор, canvas-bg).
        let mut bmarks = [0.0_f64; 3];
        // Сколько команд хром стоит шеллу: (длина снимка chrome_dl,
        // непустых полос, итог после раскладки). Хром копируется в
        // КАЖДУЮ полосу целиком, поэтому итог кратен длине снимка — и
        // именно этот множитель платит потом перепись пасса (срез 36:
        // 132 команды overlay, из них меняется одна).
        let mut chrome_mix = (0_usize, 0_usize, 0_usize);

        // Step 6 (paint): build display list buffers and call renderer.
        // Page-полоса: исходный display list + highlight-FillRect-ы
        // перед своими DrawText (когда find открыт). Прокручивается.
        // Overlay-полоса: find-bar + scrollbar — viewport-locked.
        // Без find — page = self.display_list, overlay = только scrollbar.
        // Resolved chrome palette for the active theme — passed to every
        // themed overlay panel so they follow the light/dark setting.
        // DS-14: the active profile's accent overrides the theme's own
        // accent preset — profile (level 0) outranks the Appearance
        // setting for this one field, matching "переключение профилей
        // меняет ... accent всего хрома" in the DS-14 brief.
        // DS-15: profile visual signatures (level-0 nested-frame rule).
        // Name cloned up front — the border draw below needs it after
        // several intervening `&mut self` overlay-builder calls, past
        // where a borrow of `self.profile_menu` would still be live.
        let active_profile_name: Option<String> =
            self.profile_menu.active_entry().map(|e| e.name.clone());
        let mut pal = self.shell_theme.palette(self.dark_mode);
        if active_profile_name.as_deref().is_some_and(panels::profile_menu::is_guest) {
            pal = pal.desaturated();
        }
        if let Some(entry) = self.profile_menu.active_entry() {
            pal.accent = entry.color;
        }

        // BUG-405 срез 57 (п.85): (длина, дайджесты) chrome-сегмента, если он
        // был спрайден в `overlay_buf` этим кадром — `Some` заполняется в
        // блоке хрома ниже. Используется только когда после него до самого
        // рендера в `overlay_buf` ничего не ДОПИСАНО (см. проверку рядом с
        // `overlay_len` ниже) — то есть когда хром провably остаётся хвостом
        // финального буфера, а не где-то в середине.
        let mut chrome_tail_digests: Option<(usize, Vec<u64>)> = None;
        let (mut page_buf, mut overlay_buf): (Option<lumen_paint::DisplayList>, lumen_paint::DisplayList) =
            if self.find.is_open() {
                let matches = self.current_matches();
                let page = find::build_page_with_highlights(
                    &self.display_list,
                    &self.find,
                    &matches,
                );
                // CC-9/CC-15-6: the find bar itself is drawn by the
                // engine chrome (`#findBar`, bound in
                // `Self::relayout_chrome_host`) — the legacy overlay
                // builder was deleted with the rollback flag. The
                // highlighted-page overlay above is page content, not
                // chrome, and stays unconditional.
                (Some(page), Vec::new())
            } else {
                (None, Vec::new())
            };

        // CC-4 (docs/tasks/p1-css-chrome.md): the engine-drawn chrome
        // paints first — every legacy panel/scrollbar/find-bar/tab-bar/
        // toolbar built below still lands on top of it, painter's order
        // (brief: "остальное пока legacy поверх"). Painted through 4
        // clip "frame" strips around the page-host rect (top/bottom/
        // left/right), not one plain copy: `#contentArea`'s ancestors
        // (`body{background:var(--surface-1); height:100vh}`) still emit
        // a full-window background box even with `#contentArea` itself
        // pruned out of the layout tree (`relayout_chrome_host`) — an
        // unclipped copy would paint that full-window background *over*
        // the real page, which renders separately (as `content`, so it
        // draws *under* `overlay_buf`) at exactly that rect. Clipping to
        // the 4 strips surrounding the rect lets every other chrome
        // pixel (sidebar, toolbar, any future popover CC-9+ adds outside
        // that rect) through unchanged while guaranteeing nothing paints
        // inside the live page's own rect. No-op off the flag
        // (`chrome_layout` stays `None`).
        // CC-11: patch chrome_dl with compositor-offloadable overrides
        // (opacity/transform/color/background-color) from the tick
        // above — same to_compositor_frame() mechanism the page uses
        // for anim_dl (Step 6 below), rebuilt here since chrome_dl
        // itself is a cached snapshot from the last
        // relayout_chrome_host pass and isn't otherwise touched by
        // ticks. `width` transitions (#sidebar, .dl-progress-fill)
        // aren't offloadable and stay unanimated (see
        // Self::chrome_anim_frame doc comment).
        let chrome_dl_anim: Option<lumen_paint::DisplayList> =
            self.chrome_anim_frame.as_ref().and_then(|frame| {
                let comp = frame.to_compositor_frame();
                if comp.is_empty() {
                    None
                } else {
                    self.chrome_layout.as_ref().map(|(lb, _)| {
                        let tree = StackingTree::build(lb);
                        let order = PaintOrder::from_tree(&tree);
                        build_display_list_ordered_with_anim_split(
                            lb, &tree, &order, Some(&comp),
                        )
                        .0
                    })
                }
            });
        if let (Some((_layout, chrome_dl)), Some(host)) =
            (self.chrome_layout.as_ref(), self.chrome_page_host_rect)
        {
            let chrome_dl = chrome_dl_anim.as_ref().unwrap_or(chrome_dl);
            let win_w = self.viewport_width_css();
            let win_h = self.window_height_css();
            // CC-7: `#omniInput` editing stays owned by the legacy
            // `address_bar` state machine — no native `<input>` caret
            // exists (`crates/chrome/src/model.rs` only binds the
            // *value*), so it's hand-painted here, on top of the
            // chrome document just painted above. Same simplified
            // "flush right of the field" placement `build_inline_field`
            // used for the old overlay caret (`address_bar.rs`) — not
            // per-glyph-measured, and it never needed to be while
            // `AddressBarState` only supports append/backspace at the
            // end of the string. Hidden while a dropdown suggestion is
            // selected, mirroring the same overlay behavior.
            //
            // BUG-405 срез 50: computed up front, not inside the build
            // branch below — it is part of `ChromeOverlayFrameCache`'s key
            // (a caret blink/selection change must miss the cache exactly
            // like a real `chrome_dl` change would).
            let caret_plan = if self.address_bar.is_open()
                && self.address_bar.selected_idx().is_none()
                && !self.address_bar.input().is_empty()
            {
                self.chrome_omni_input_rect.map(|field| {
                    (
                        Rect::new(
                            field.x + field.width - 8.0,
                            field.y + 4.0,
                            2.0,
                            (field.height - 8.0).max(0.0),
                        ),
                        lumen_layout::Color { a: 220, ..pal.accent },
                    )
                })
            } else {
                None
            };
            // BUG-405 срез 50 (п.85 "вариант (б)"): `chrome_dl` is provably
            // unchanged whenever `relayout_chrome_host` hasn't run since this
            // cache was built (`chrome_layout_generation` — bumped
            // unconditionally by every pass, a safe superset of "content
            // changed", see its own doc comment) — reuse the assembled
            // segment with one `Vec` clone instead of re-copying `chrome_dl`
            // into up to 4 clip strips (`chrome_mix`'s multiplier). Never a
            // candidate while a chrome CSS transition/animation is live
            // (`chrome_dl_anim.is_some()`): that content differs every tick
            // by construction. `LUMEN_NO_CHROME_OVERLAY_CACHE=1` disables
            // reuse for A/B (docs/perf-method.md).
            let (mut framed, strips_used, digests, new_cache) = chrome_overlay_segment(
                chrome_dl,
                host,
                win_w,
                win_h,
                caret_plan,
                self.chrome_layout_generation,
                chrome_dl_anim.is_none() && !chrome_overlay_cache_disabled(),
                self.chrome_overlay_frame_cache.as_ref(),
            );
            chrome_mix = (chrome_dl.len(), strips_used, framed.len());
            if let Some(c) = new_cache {
                self.chrome_overlay_frame_cache = Some(c);
            }
            // BUG-405 срез 57: `overlay_buf` пуст в этой точке (все ветки
            // Step 6 выше либо оставляют его `Vec::new()`, либо кладут
            // страницу в `page_buf`, а не в overlay) — `framed.len()`
            // именно поэтому равен длине сегмента внутри финального буфера,
            // не только внутри `framed` самого по себе.
            chrome_tail_digests =
                (!chrome_overlay_digest_reuse_disabled()).then_some((framed.len(), digests));
            framed.append(&mut overlay_buf);
            overlay_buf = framed;
        }
        if let Some(t0) = frame_log_t0 {
            bmarks[0] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Scrollbar встаёт перед find-bar в overlay-буфере: рисуется
        // первым = находится под find-bar-ом в painter's order. Они не
        // пересекаются по x (bar занимает левее `ww - 12`, scrollbar
        // справа от `ww - 8`), так что фактического overdraw нет.
        // --no-scrollbar подавляет полосу для screenshot-пайплайна.
        if !self.no_scrollbar {
            // FRAME-3 remainder: собственный scrollbar каждого видимого
            // фрейма — то же приём, добавлен в ту же полосу overlay-а рядом
            // со страничным.
            let mut scrollbar_cmds = scrollbar::build_scrollbar_overlay(
                self.scroll_y,
                self.content_height,
                self.viewport_width_css(),
                self.viewport_height_css(),
            );
            scrollbar_cmds.extend(frames::frame_scrollbar_overlay(
                &self.frames,
                self.scroll_x,
                self.scroll_y,
            ));
            if !scrollbar_cmds.is_empty() {
                let mut combined = scrollbar_cmds;
                combined.append(&mut overlay_buf);
                overlay_buf = combined;
            }
        }
        if let Some(t0) = frame_log_t0 {
            bmarks[1] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Forms: validation tooltip and color picker overlays.
        let vp_w = self.viewport_width_css();
        if let Some((anchor, msg)) = &self.validation_tooltip {
            let mut tt = forms::build_validation_tooltip(
                *anchor, msg, self.scroll_y, vp_w,
            );
            tt.append(&mut overlay_buf);
            overlay_buf = tt;
        }
        if let (Some(picker_node), Some(lb)) =
            (self.color_picker_node, &self.layout_box)
            && let Some(anchor) = forms::find_box_rect(lb, picker_node)
        {
            let mut picker = forms::build_color_picker(anchor, self.scroll_y, vp_w);
            picker.append(&mut overlay_buf);
            overlay_buf = picker;
        }
        if let (Some(dp_node), Some(lb)) =
            (self.date_picker_node, &self.layout_box)
            && let Some(anchor) = forms::find_box_rect(lb, dp_node)
        {
            let mut dp = forms::build_date_picker(anchor, self.scroll_y, vp_w, self.date_picker_year, self.date_picker_month);
            dp.append(&mut overlay_buf);
            overlay_buf = dp;
        }
        if let (Some(sel_node), Some(lb)) =
            (self.select_dropdown_node, &self.layout_box)
            && let Some(anchor) = forms::find_box_rect(lb, sel_node)
        {
            // `appearance: base-select` renders the picker from author CSS
            // on the `<option>`s; the native (Auto/Compat) path keeps the
            // fixed UA chrome. Row geometry is shared, so `hit_select_option`
            // is valid regardless of which builder produced the overlay.
            let base_select_style = forms::find_layout_box(lb, sel_node)
                .filter(|b| b.style.appearance == lumen_layout::Appearance::BaseSelect)
                .map(|b| b.style.clone());
            if let Some(src) = self.layout_source.as_ref() {
                let doc = src.document.lock().unwrap();
                let opts = forms::collect_select_options(&doc, sel_node);
                let vp_h = self.viewport_height_css();
                let mut dd = if let Some(sel_style) = &base_select_style {
                    forms::build_base_select_dropdown(
                        anchor, &doc, &src.stylesheet, sel_style, &opts,
                        self.scroll_y, vp_w, vp_h, self.dark_mode,
                    )
                } else {
                    forms::build_select_dropdown(anchor, &opts, self.scroll_y, vp_w, vp_h)
                };
                dd.append(&mut overlay_buf);
                overlay_buf = dd;
            }
        }

        // FRAME-6: same three overlays, anchored to a control inside a
        // frame's OWN sub-document rather than the page's. `frame_overlay_anchor`
        // (`frame_forms.rs`) does the layout lookup in the frame's own tree
        // (`NodeId` only resolves there) plus the `frame_page_origin`
        // translation into page coordinates the page's plain `find_box_rect`
        // above does not need. Mutually exclusive with the page-side overlays
        // above by construction — `handle_click_at_inner` (`click.rs`) closes
        // every one of the six fields on any click that does not land inside
        // its own popup.
        if let Some((fidx, picker_node)) = self.frame_color_picker
            && let Some(anchor) = self.frame_overlay_anchor(fidx, picker_node)
        {
            let mut picker = forms::build_color_picker(anchor, self.scroll_y, vp_w);
            picker.append(&mut overlay_buf);
            overlay_buf = picker;
        }
        if let Some((fidx, dp_node)) = self.frame_date_picker
            && let Some(anchor) = self.frame_overlay_anchor(fidx, dp_node)
        {
            let mut dp = forms::build_date_picker(
                anchor, self.scroll_y, vp_w,
                self.frame_date_picker_year, self.frame_date_picker_month,
            );
            dp.append(&mut overlay_buf);
            overlay_buf = dp;
        }
        if let Some((fidx, sel_node)) = self.frame_select_dropdown
            && let Some(anchor) = self.frame_overlay_anchor(fidx, sel_node)
            && let Some(handle) = self.frames.get(fidx)
            && let Some(lb) = handle.layout.as_ref()
        {
            let base_select_style = forms::find_layout_box(lb, sel_node)
                .filter(|b| b.style.appearance == lumen_layout::Appearance::BaseSelect)
                .map(|b| b.style.clone());
            if let Ok(doc) = handle.doc.lock() {
                let opts = forms::collect_select_options(&doc, sel_node);
                let vp_h = self.viewport_height_css();
                let mut dd = if let Some(sel_style) = &base_select_style {
                    forms::build_base_select_dropdown(
                        anchor, &doc, &handle.sheet, sel_style, &opts,
                        self.scroll_y, vp_w, vp_h, self.dark_mode,
                    )
                } else {
                    forms::build_select_dropdown(anchor, &opts, self.scroll_y, vp_w, vp_h)
                };
                dd.append(&mut overlay_buf);
                overlay_buf = dd;
            }
        }

        // <dialog> modal overlay (L-2) — ::backdrop + centered dialog above page.
        if let Some(lb) = &self.layout_box {
            let doc =
                self.layout_source.as_ref().map(|s| s.document.lock().unwrap());
            if let Some(doc) = doc {
                let modal_nids = forms::collect_modal_dialogs(&doc);
                if !modal_nids.is_empty() {
                    let vp_h = self.viewport_height_css();
                    for &dlg_nid in &modal_nids {
                        if let Some(dlg_lb) = forms::find_layout_box(lb, dlg_nid) {
                            let mut dlg_overlay = forms::build_dialog_overlay(
                                dlg_lb,
                                self.scroll_y,
                                vp_w,
                                vp_h,
                            );
                            dlg_overlay.append(&mut overlay_buf);
                            overlay_buf = dlg_overlay;
                        }
                    }
                }
            }
        }

        // Compositor offload: если есть активные анимации с opacity/transform/
        // color/background-color — пересобираем display list из layout_box с
        // overrides, минуя relayout (BUG-231 распространил offload на цвета).
        // Static/animated split (EXPERIMENT.md §2): вместе со списком строятся
        // диапазоны анимируемых сегментов — скролл-композитор кэширует полосу
        // по статике, сегменты рисует поверх. Позднейшие append-ы в anim_dl
        // (cue, squiggles) идут в конец списка и диапазоны не сдвигают.
        // FRAME-7: the focused `<input>`'s caret rides the same per-NodeId
        // override map as CSS-animation offload — computed up front (before
        // `frame`/`lb` are borrowed) since it needs `&self`, not the
        // `anim_frame`/`layout_box` fields specifically.
        let caret_override = self.focused_input_caret();
        // FRAME-7 remainder 2: the focused `<input>`'s selection range, same
        // override channel as the caret above.
        let selection_override = self.focused_input_selection();
        let mut anim_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        let mut anim_dl: Option<lumen_paint::DisplayList> =
            if let Some(lb) = &self.layout_box {
                let mut comp = self
                    .anim_frame
                    .as_ref()
                    .map(|f| f.to_compositor_frame())
                    .unwrap_or_default();
                if let Some((nid, cursor)) = caret_override {
                    comp.overrides.entry(nid).or_default().caret = Some(cursor);
                }
                if let Some((nid, start, end)) = selection_override {
                    comp.overrides.entry(nid).or_default().selection = Some((start, end));
                }
                if !comp.is_empty() {
                    let tree = StackingTree::build(lb);
                    let order = PaintOrder::from_tree(&tree);
                    let (dl, ranges) = build_display_list_ordered_with_anim_split(
                        lb,
                        &tree,
                        &order,
                        Some(&comp),
                    );
                    if std::env::var("LUMEN_FRAME_LOG").is_ok_and(|v| v != "0") {
                        eprintln!(
                            "[frame] anim_dl: {} cmds, {} ranges, {} overrides",
                            dl.len(),
                            ranges.len(),
                            comp.overrides.len(),
                        );
                    }
                    anim_ranges = ranges;
                    Some(dl)
                } else {
                    None
                }
            } else {
                None
            };

        let scroll_y = self.scroll_y;
        let scroll_x = self.scroll_x;

        // CSS View Transitions: fade old display list over new content.
        // Renders old_dl wrapped in PushOpacity(1-progress)/PopOpacity so it
        // fades out while the new display list (rendered underneath) fades in.
        // Runs at most `duration_ms`; after that, view_transition is cleared.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        if let Some(ref vt) = self.view_transition {
            let elapsed = now_ms - vt.start_ms;
            let progress = (elapsed / vt.duration_ms).clamp(0.0, 1.0) as f32;
            let alpha = 1.0 - progress;
            if alpha > 0.0 {
                let mut vt_cmds = Vec::with_capacity(vt.old_dl.len() + 2);
                vt_cmds.push(lumen_paint::DisplayCommand::PushOpacity { alpha, bounds: None });
                vt_cmds.extend_from_slice(&vt.old_dl);
                vt_cmds.push(lumen_paint::DisplayCommand::PopOpacity);
                // Prepend so old content renders before (under) UI panels.
                vt_cmds.append(&mut overlay_buf);
                overlay_buf = vt_cmds;
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
        }
        // Clear completed transition (separate borrow from the block above).
        let transition_done = self
            .view_transition
            .as_ref()
            .is_some_and(|vt| now_ms - vt.start_ms >= vt.duration_ms);
        if transition_done {
            self.view_transition = None;
        }

        // BUG-405 срез 57: снимок длины ПОСЛЕ последнего builder-а, что
        // ПРЕПЕНДИТ (`X.append(&mut overlay_buf); overlay_buf = X;` —
        // хром/scrollbar/tooltip/picker/date-picker/select-dropdown/dialog/
        // view-transition выше), ДО первого builder-а, что ДОПИСЫВАЕТ в
        // конец (`overlay_buf.append(&mut Y)` — hint/console/network/
        // privacy/inspector/… ниже). Срез 56 нашёл, что каждый препенд
        // сохраняет хром хвостом накопленного буфера независимо от того,
        // сколько их сработало — значит если длина не выросла к моменту
        // рендера, хром остаётся хвостом и там же, а не где-то в середине.
        let overlay_len_after_prepend_phase = overlay_buf.len();

        // Hint overlay: viewport-locked бейджи kbd-навигации.
        // Добавляются последними → рисуются поверх scrollbar/tooltip.
        if self.hint.is_active() {
            let mut hint_cmds = hints::build_hints_overlay(&self.hint, scroll_x, scroll_y);
            overlay_buf.append(&mut hint_cmds);
        }

        // CC-10/CC-15-6: the legacy download-panel overlay lived here,
        // gated off the rollback flag — `#downloadsPanel` in the engine
        // chrome (`bind_downloads`, CC-9) is the only renderer now.

        // DevTools JS console panel: bottom overlay, toggled by F12.
        if self.devtools_console.visible {
            let con_win_size = self.window.as_ref().map_or((1024, 720), |w| {
                let s = w.inner_size();
                (s.width, s.height)
            });
            let mut con_cmds = devtools::console_panel::build_console_panel(
                &self.devtools_console,
                con_win_size,
            );
            overlay_buf.append(&mut con_cmds);
        }

        // DevTools network panel: bottom overlay, toggled by Ctrl+Shift+E.
        if self.network_panel.visible {
            self.network_panel.refresh();
            let net_win_size = self.window.as_ref().map_or((1024, 720), |w| {
                let s = w.inner_size();
                (s.width, s.height)
            });
            let mut net_cmds = devtools::network_panel::build_network_panel(
                &self.network_panel,
                net_win_size,
            );
            overlay_buf.append(&mut net_cmds);
        }

        // Privacy network panel (V5): right-docked overlay, Ctrl+Shift+Y.
        if self.privacy.visible {
            self.privacy.refresh();
            let priv_win_size = self.window.as_ref().map_or((1024, 720), |w| {
                let s = w.inner_size();
                (s.width, s.height)
            });
            let mut priv_cmds = panels::privacy_panel::build_privacy_panel(
                &self.privacy,
                priv_win_size,
                toolbar::CHROME_H,
                &pal,
            );
            overlay_buf.append(&mut priv_cmds);
        }

        // DevTools DOM inspector: right-docked computed-style side panel.
        // Viewport-locked; the box-model overlay for the hovered node is
        // emitted into the scrollable page layer below.
        if self.dom_inspector.visible {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let (pw, ph) = self.window.as_ref().map_or((1024, 720), |w| {
                let s = w.inner_size();
                (s.width, s.height)
            });
            let win_css = (
                (pw as f32 / dpr) as u32,
                (ph as f32 / dpr) as u32,
            );
            // Feed the inspector's Network tab a fresh request snapshot
            // from the shared NetworkLog (CC-9).
            let net_entries = self.network_panel.entries_clone();
            self.dom_inspector.set_network_entries(net_entries);
            let mut insp_cmds = devtools::inspector::build_inspector_panel(
                &self.dom_inspector,
                win_css,
                toolbar::CHROME_H,
            );
            overlay_buf.append(&mut insp_cmds);
        }

        // Vertical tab panel: docked left sidebar, below the tab bar.
        // Rendered before the tab bar so tab bar draws on top.
        if self.vertical_tabs.visible {
            let win_h = self.viewport_height_css() + toolbar::CHROME_H;
            let vt_w = self.panel_layout.width_for(
                panel_layout::ID_VERTICAL_TABS,
                panels::vertical_tabs::PANEL_WIDTH,
            );
            let mut vt_cmds = panels::vertical_tabs::build_tab_bar_vertical(
                &self.tab_strip,
                toolbar::CHROME_H,
                win_h,
                self.vertical_tabs.scroll_y,
                &pal,
                vt_w,
                self.workspace_panel.active_accent(),
            );
            // Cross-dock: the panel paints left-relative; re-home it onto
            // the right edge when flipped there.
            let vt_side = self.sidebar_dock_side(panel_layout::ID_VERTICAL_TABS);
            Self::offset_overlay_x(&mut vt_cmds, self.dock_origin_x(vt_side, vt_w));
            overlay_buf.append(&mut vt_cmds);
        }

        // Tree-style tab panel (7A.2): same slot as vertical_tabs, but with
        // parent-child indentation and collapse/expand arrows.
        // Toggle via Ctrl+Shift+B; occupies the same PANEL_WIDTH as vertical_tabs.
        if self.tree_tabs.visible {
            let win_h = self.viewport_height_css() + toolbar::CHROME_H;
            let tt_w = self.panel_layout.width_for(
                panel_layout::ID_TREE_TABS,
                panels::tree_tabs::PANEL_WIDTH,
            );
            let mut tt_cmds = panels::tree_tabs::build_panel(
                &self.tab_strip,
                &self.tree_tabs,
                toolbar::CHROME_H,
                win_h,
                &pal,
                tt_w,
            );
            // Cross-dock: re-home the left-relative panel onto the right
            // edge when flipped there.
            let tt_side = self.sidebar_dock_side(panel_layout::ID_TREE_TABS);
            Self::offset_overlay_x(&mut tt_cmds, self.dock_origin_x(tt_side, tt_w));
            overlay_buf.append(&mut tt_cmds);
        }

        // Shields floating panel (7C.4): top-right overlay anchored below
        // the tab bar. Refresh blocked counts before rendering — kept
        // unconditional (CC-10) since `chrome_model_snapshot`'s
        // `#statTrackers` binding (CC-9) reads `blocked_total_count()`
        // and this is the only call site that refreshes it; only the
        // legacy *paint* below is gated.
        if self.shields.visible {
            self.shields.refresh();
        }

        // Note viewer overlay (§12.2, GG-2): floating annotation panel.
        if self.note_viewer.visible {
            let win_size = self.window.as_ref().map_or((1024, 720), |w| {
                let s = w.inner_size();
                (s.width, s.height)
            });
            let mut nv_cmds = panels::note_viewer::build_note_viewer(&self.note_viewer, win_size, &pal);
            overlay_buf.append(&mut nv_cmds);
        }

        // Workspace switcher bar (7A.3): bottom-docked horizontal strip.
        // Rendered before the tab bar so tab bar always draws on top.
        if self.workspace_panel.visible {
            let win_w = self.viewport_width_css();
            // Full window height including tab bar — bar is docked at bottom.
            let win_h = self.viewport_height_css()
                + toolbar::CHROME_H
                + panels::workspace_panel::SWITCHER_HEIGHT;
            let mut ws_cmds = panels::workspace_panel::build_panel(
                &self.workspace_panel,
                win_w,
                win_h,
                &pal,
            );
            overlay_buf.append(&mut ws_cmds);
        }

        // Accessibility settings panel (E-2): centred overlay, Ctrl+Shift+Q.
        if self.a11y_panel.visible {
            let win_w = self.viewport_width_css();
            let win_h = self.viewport_height_css();
            let win_size = (win_w as u32, win_h as u32);
            let mut a11y_cmds =
                panels::a11y_panel::build_a11y_panel(&self.a11y_panel, win_size, &pal);
            overlay_buf.append(&mut a11y_cmds);
        }

        // Keyboard shortcuts panel (§D-4): centred floating overlay.
        if self.shortcuts_panel.visible {
            let win_w = self.viewport_width_css();
            let win_h = self.viewport_height_css();
            let kp_x = (win_w - panels::shortcuts_panel::PANEL_W) * 0.5;
            let kp_y = (win_h - panels::shortcuts_panel::PANEL_H) * 0.5;
            self.shortcuts_panel.build_panel(&mut overlay_buf, kp_x, kp_y, &pal);
        }

        // §12.3 Read-later panel: right-docked overlay.
        if self.read_later_panel.visible {
            let win_w = self.viewport_width_css();
            let tab_h = toolbar::CHROME_H;
            let mut rl_cmds = panels::read_later_panel::build_panel(
                &self.read_later_panel,
                win_w,
                tab_h,
                &pal,
            );
            overlay_buf.append(&mut rl_cmds);
        }

        // CC-15-3: the legacy tab-bar/toolbar paint block (viewport-
        // locked strip at y=0..TAB_BAR_HEIGHT) lived here — removed
        // along with `tabs::strip::build_tab_bar`/`build_tab_tooltip`/
        // `build_layout_toggle_btn`/`build_settings_btn` and
        // `toolbar::build_toolbar`. Under the engine-drawn chrome
        // (CC-4) it never ran.

        // Profile switcher dropdown (DS-14): BUG-403 — kept as a
        // legacy overlay always (CC-15-1, `docs/tasks/p1-css-chrome.md`
        // §CC-15-1 decision), not migrated to `ChromeModel`/`bind_model`
        // like the CC-9/CC-10 panels. Its hit-test (below, in the
        // `MouseInput` handler) was already unconditional — this render
        // call must match, or a click toggles `profile_menu.visible`
        // with nothing ever drawn (the actual BUG-403 symptom) while
        // the invisible popover still eats clicks under it. Anchored
        // via `page_offset()` rather than the legacy-only
        // `toolbar::CHROME_H` constant so the dropdown lines up with
        // the engine-drawn toolbar's *measured* bottom edge, not an
        // assumed one — the same class of drift BUG-404 flags for
        // `flush_pointer_moves`.
        if !self.focus.active && self.profile_menu.visible {
            let (_, page_y_offset) = self.page_offset();
            let mut pm_cmds = panels::profile_menu::build_panel(
                &self.profile_menu,
                toolbar::avatar_x(),
                page_y_offset,
                &pal,
            );
            overlay_buf.append(&mut pm_cmds);
        }

        // CC-4: tab context menu — drawn above the tab strip.
        if self.tab_context_menu.is_open() {
            let win_w = self.viewport_width_css();
            let win_h = self.window_height_css();
            let mut menu_cmds = tabs::context_menu::build_overlay(
                &self.tab_context_menu,
                win_w,
                win_h,
            );
            overlay_buf.append(&mut menu_cmds);
        }

        // P3-spell срез 3: page spell suggestion menu — drawn above the
        // page and tab strip like the tab context menu.
        if self.page_context_menu.is_open() {
            let win_w = self.viewport_width_css();
            let win_h = self.window_height_css();
            let mut menu_cmds = self.page_context_menu.build_overlay(win_w, win_h);
            overlay_buf.append(&mut menu_cmds);
        }

        // Focus mode widget (task #25): floating Pomodoro card with an
        // arc progress ring, drawn on top of everything (including where
        // the now-hidden tab bar was).
        if self.focus.active {
            let win_w = self.viewport_width_css();
            let mut focus_cmds =
                panels::focus_panel::build_panel(&self.focus, win_w, &pal);
            overlay_buf.append(&mut focus_cmds);
        }

        // Picture-in-picture window (task #21) — drawn last so it floats
        // above all other chrome.
        if self.pip.active {
            let mut pip_cmds = panels::pip_window::build_panel(&self.pip, &pal);
            overlay_buf.append(&mut pip_cmds);
        }

        // Loading spinner for hibernated tab restore >200ms (10K.3).
        if let Some(start_ms) = self.restore_spinner_start_ms {
            let elapsed_ms = now_ms - start_ms;
            let win_w = self.viewport_width_css();
            let win_h =
                self.viewport_height_css() + toolbar::CHROME_H;
            if let Some(mut spinner) =
                panels::restore_spinner::build_spinner(elapsed_ms, win_w, win_h)
            {
                overlay_buf.append(&mut spinner);
                // Keep animating the spinner while it's visible.
                self.request_redraw();
            }
        }

        // Sleep hint for T2 SQLite restore >100 ms (10I).
        if let Some(start_ms) = self.t2_restore_start_ms {
            let elapsed_ms = now_ms - start_ms;
            let win_w = self.viewport_width_css();
            if let Some(mut hint) =
                panels::sleep_hint::build_sleep_hint(elapsed_ms, win_w)
            {
                overlay_buf.append(&mut hint);
                self.request_redraw();
            }
        }

        // P3-webvtt срез 4: активные WebVTT-cue поверх video-боксов.
        // Команды добавляются в page-полосу (скроллятся со страницей);
        // при активном compositor-offload — в anim_dl. Время
        // воспроизведения берётся из реального playback-клока видео
        // (`VideoGifStore`); для не-GIF/не-запущенных видео — фолбэк на
        // время от старта навигации.
        if !self.page_tracks.is_empty() {
            let mut video_rects = Vec::new();
            if let Some(lb) = &self.layout_box {
                tracks::collect_video_rects(lb, &mut video_rects);
            }
            if !video_rects.is_empty()
                && let Ok(font) = lumen_font::Font::parse(INTER_FONT)
                && let Ok(m) = lumen_paint::FontMeasurer::new(&font)
            {
                let nav_t = self
                    .nav_start
                    .map_or(0.0, |s| s.elapsed().as_secs_f64());
                let clock_ms = self.epoch.elapsed().as_millis() as u64;
                let playback = self.video_gif_store.playback.lock().unwrap();
                let time_for = |node: lumen_dom::NodeId| -> f64 {
                    let nid = node.index() as u32;
                    match playback.get(&nid) {
                        Some(st) => st.current_ms(clock_ms) as f64 / 1000.0,
                        None => nav_t,
                    }
                };
                let measure = |s: &str, fs: f32| -> f32 {
                    use lumen_layout::TextMeasurer;
                    s.chars().map(|c| m.char_width(c, fs)).sum()
                };
                let mut cue_cmds = tracks::build_cue_overlay(
                    &self.page_tracks,
                    &video_rects,
                    &time_for,
                    &measure,
                );
                if !cue_cmds.is_empty() {
                    if let Some(dl) = anim_dl.as_mut() {
                        dl.append(&mut cue_cmds);
                    } else {
                        let mut buf = page_buf
                            .take()
                            .unwrap_or_else(|| self.display_list.clone());
                        buf.append(&mut cue_cmds);
                        page_buf = Some(buf);
                    }
                }
                // Cue сменяются временем — держим цикл перерисовки,
                // пока страница с субтитрами активна.
                self.request_redraw();
            }
        }

        // P3-spell срез 2+3: красное squiggly-подчёркивание ошибочных
        // слов в фокусном редактируемом поле — <input>/<textarea> или
        // хост contenteditable. Проверяется каждый DrawText внутри
        // бокса поля; placeholder пропускается. Слова из
        // пользовательского словаря и «Пропущенные» на сессию —
        // считаются верными.
        if let (Some(nid), Some(dicts)) = (self.focused_node, SPELL_DICTS.get())
            && !dicts.is_empty()
            && let Some((target_nid, placeholder, _kind)) = self.spell_target(nid)
            && let Some(node_lb) = self
                .layout_box
                .as_ref()
                .and_then(|lb| forms::find_layout_box(lb, target_nid))
            && let Ok(font) = lumen_font::Font::parse(INTER_FONT)
            && let Ok(m) = lumen_paint::FontMeasurer::new(&font)
        {
            let node_rect = node_lb.rect;
            let allow = self.spell_allow_set();
            let mut squiggles: lumen_paint::DisplayList = Vec::new();
            for cmd in &self.display_list {
                let lumen_paint::DisplayCommand::DrawText {
                    rect, text, font_size, ..
                } = cmd
                else {
                    continue;
                };
                if rect.x < node_rect.x
                    || rect.y < node_rect.y
                    || rect.x >= node_rect.x + node_rect.width
                    || rect.y >= node_rect.y + node_rect.height
                    || (!placeholder.is_empty() && text == &placeholder)
                {
                    continue;
                }
                let ranges = spellcheck::misspelled_ranges_with(dicts, text, &allow);
                if ranges.is_empty() {
                    continue;
                }
                let fs = *font_size;
                let measure = |s: &str| -> f32 {
                    use lumen_layout::TextMeasurer;
                    s.chars().map(|c| m.char_width(c, fs)).sum()
                };
                squiggles.extend(spellcheck::build_spell_overlay(
                    text, rect.x, rect.y, fs, &ranges, &measure,
                ));
            }
            if !squiggles.is_empty() {
                if let Some(dl) = anim_dl.as_mut() {
                    dl.extend(squiggles);
                } else {
                    let mut buf = page_buf
                        .take()
                        .unwrap_or_else(|| self.display_list.clone());
                    buf.extend(squiggles);
                    page_buf = Some(buf);
                }
            }
        }

        // FRAME-7 (remainder item 1): caret bar for a focused `<textarea>` —
        // a shell-side overlay built straight from the (cached, still-valid)
        // layout box tree, like the squiggle overlay above, rather than the
        // `CompositorOverride` channel slice 1 used for a page `<input>`: see
        // `forms::textarea_caret_rect`'s doc comment for why a textarea needs
        // a different mechanism.
        // FRAME-7 remainder 2: selection highlight for a focused `<textarea>`
        // — same shell-side overlay mechanism as its caret bar below, painted
        // first so the caret (suppressed below while a selection is active,
        // the same convention `emit_input_selection` enforces for a page
        // `<input>`) never draws on top of it.
        if let Some((nid, start, end, value)) = self.focused_textarea_selection()
            && let Some(lb) = self.layout_box.as_ref()
            && let Some(field_lb) = forms::find_layout_box(lb, nid)
            && let Ok(font) = lumen_font::Font::parse(INTER_FONT)
            && let Ok(m) = lumen_paint::FontMeasurer::new(&font)
        {
            let fs = field_lb.style.font_size;
            let measure = |s: &str| -> f32 {
                use lumen_layout::TextMeasurer;
                s.chars().map(|c| m.char_width(c, fs)).sum()
            };
            let rects = forms::textarea_selection_rects(field_lb, &value, start, end, &measure);
            if !rects.is_empty() {
                let mut sel_cmd =
                    vec![lumen_paint::DisplayCommand::PushClipRect { rect: field_lb.rect }];
                for rect in rects {
                    sel_cmd.push(lumen_paint::DisplayCommand::FillRect {
                        rect,
                        color: forms::SELECTION_HIGHLIGHT_DEFAULT,
                    });
                }
                sel_cmd.push(lumen_paint::DisplayCommand::PopClip);
                if let Some(dl) = anim_dl.as_mut() {
                    dl.append(&mut sel_cmd);
                } else {
                    let mut buf = page_buf.take().unwrap_or_else(|| self.display_list.clone());
                    buf.append(&mut sel_cmd);
                    page_buf = Some(buf);
                }
            }
        }

        if let Some((nid, cursor, value)) = self.focused_textarea_caret()
            && self.focused_textarea_selection().is_none()
            && let Some(lb) = self.layout_box.as_ref()
            && let Some(field_lb) = forms::find_layout_box(lb, nid)
            && let Ok(font) = lumen_font::Font::parse(INTER_FONT)
            && let Ok(m) = lumen_paint::FontMeasurer::new(&font)
        {
            let fs = field_lb.style.font_size;
            let measure = |s: &str| -> f32 {
                use lumen_layout::TextMeasurer;
                s.chars().map(|c| m.char_width(c, fs)).sum()
            };
            let rect = forms::textarea_caret_rect(field_lb, &value, cursor, &measure);
            // CSS UI L4 §6.3 `caret-color: auto` follows the text color —
            // same resolution `emit_input_caret` applies for `<input>`.
            let color = field_lb.style.caret_color.unwrap_or(field_lb.style.color);
            let mut caret_cmd = vec![
                lumen_paint::DisplayCommand::PushClipRect { rect: field_lb.rect },
                lumen_paint::DisplayCommand::FillRect { rect, color },
                lumen_paint::DisplayCommand::PopClip,
            ];
            if let Some(dl) = anim_dl.as_mut() {
                dl.append(&mut caret_cmd);
            } else {
                let mut buf = page_buf
                    .take()
                    .unwrap_or_else(|| self.display_list.clone());
                buf.append(&mut caret_cmd);
                page_buf = Some(buf);
            }
        }

        // FRAME-7 остаток (1): caret bar for a focused `<input>`/`<textarea>`
        // INSIDE a frame — a shell-side overlay in PAGE coordinates
        // (`frames::frame_page_origin` translates the rect found in the
        // frame's OWN layout tree, the same offset `show_frame_validation_tooltip`
        // uses), not the `CompositorOverride` channel the page `<input>` caret
        // rides above: a frame's `content_dl` is rebuilt only on relayout
        // (`frames::rebuild_frame_display_lists`'s dirty gate), so wiring the
        // override channel through it is the much larger change FRAME-7's
        // ROADMAP "Остаток" note describes. Clipped to the frame's OWN
        // viewport (translated the same way) in addition to the field's own
        // box, so a field scrolled out of the frame's visible area — or a
        // frame scrolled out of the page's — does not leave a caret floating
        // over unrelated content.
        // FRAME-7 remainder 2: selection highlight for a focused frame
        // `<input>` — same translate-into-page-coordinates + double clip as
        // its caret below, painted first so the caret (gated off below while
        // a selection is active) never draws on top of it.
        if let Some(handle) = self.focused_frame.and_then(|(idx, _)| self.frames.get(idx))
            && let Some((fidx, nid, start, end, value)) = self.focused_frame_input_selection()
            && let Some(field_lb) = handle.layout.as_ref().and_then(|lb| forms::find_layout_box(lb, nid))
            && let Some((ox, oy)) = frames::frame_page_origin(&self.frames, fidx)
        {
            let rect = forms::input_selection_rect(field_lb, &value, start, end);
            let translate = |r: lumen_core::geom::Rect| lumen_core::geom::Rect {
                x: r.x + ox,
                y: r.y + oy,
                ..r
            };
            let viewport_rect = lumen_core::geom::Rect {
                x: ox,
                y: oy,
                width: handle.viewport.width,
                height: handle.viewport.height,
            };
            let mut sel_cmd = vec![
                lumen_paint::DisplayCommand::PushClipRect { rect: viewport_rect },
                lumen_paint::DisplayCommand::PushClipRect { rect: translate(field_lb.rect) },
                lumen_paint::DisplayCommand::FillRect {
                    rect: translate(rect),
                    color: forms::SELECTION_HIGHLIGHT_DEFAULT,
                },
                lumen_paint::DisplayCommand::PopClip,
                lumen_paint::DisplayCommand::PopClip,
            ];
            if let Some(dl) = anim_dl.as_mut() {
                dl.append(&mut sel_cmd);
            } else {
                let mut buf = page_buf.take().unwrap_or_else(|| self.display_list.clone());
                buf.append(&mut sel_cmd);
                page_buf = Some(buf);
            }
        }
        if let Some(handle) = self.focused_frame.and_then(|(idx, _)| self.frames.get(idx))
            && let Some((fidx, nid, cursor, value)) = self.focused_frame_input_caret()
            && self.focused_frame_input_selection().is_none()
            && let Some(field_lb) = handle.layout.as_ref().and_then(|lb| forms::find_layout_box(lb, nid))
            && let Some((ox, oy)) = frames::frame_page_origin(&self.frames, fidx)
        {
            let rect = forms::input_caret_rect(field_lb, &value, cursor);
            let color = field_lb.style.caret_color.unwrap_or(field_lb.style.color);
            let translate = |r: lumen_core::geom::Rect| lumen_core::geom::Rect {
                x: r.x + ox,
                y: r.y + oy,
                ..r
            };
            let viewport_rect = lumen_core::geom::Rect {
                x: ox,
                y: oy,
                width: handle.viewport.width,
                height: handle.viewport.height,
            };
            let mut caret_cmd = vec![
                lumen_paint::DisplayCommand::PushClipRect { rect: viewport_rect },
                lumen_paint::DisplayCommand::PushClipRect { rect: translate(field_lb.rect) },
                lumen_paint::DisplayCommand::FillRect { rect: translate(rect), color },
                lumen_paint::DisplayCommand::PopClip,
                lumen_paint::DisplayCommand::PopClip,
            ];
            if let Some(dl) = anim_dl.as_mut() {
                dl.append(&mut caret_cmd);
            } else {
                let mut buf = page_buf
                    .take()
                    .unwrap_or_else(|| self.display_list.clone());
                buf.append(&mut caret_cmd);
                page_buf = Some(buf);
            }
        }
        // FRAME-7 remainder 2: selection highlight for a focused frame
        // `<textarea>` — same shell-side overlay + translate as its caret
        // below, painted first for the same "caret never on top" reason.
        if let Some(handle) = self.focused_frame.and_then(|(idx, _)| self.frames.get(idx))
            && let Some((fidx, nid, start, end, value)) = self.focused_frame_textarea_selection()
            && let Some(field_lb) = handle.layout.as_ref().and_then(|lb| forms::find_layout_box(lb, nid))
            && let Some((ox, oy)) = frames::frame_page_origin(&self.frames, fidx)
            && let Ok(font) = lumen_font::Font::parse(INTER_FONT)
            && let Ok(m) = lumen_paint::FontMeasurer::new(&font)
        {
            let fs = field_lb.style.font_size;
            let measure = |s: &str| -> f32 {
                use lumen_layout::TextMeasurer;
                s.chars().map(|c| m.char_width(c, fs)).sum()
            };
            let rects = forms::textarea_selection_rects(field_lb, &value, start, end, &measure);
            if !rects.is_empty() {
                let translate = |r: lumen_core::geom::Rect| lumen_core::geom::Rect {
                    x: r.x + ox,
                    y: r.y + oy,
                    ..r
                };
                let viewport_rect = lumen_core::geom::Rect {
                    x: ox,
                    y: oy,
                    width: handle.viewport.width,
                    height: handle.viewport.height,
                };
                let mut sel_cmd = vec![
                    lumen_paint::DisplayCommand::PushClipRect { rect: viewport_rect },
                    lumen_paint::DisplayCommand::PushClipRect { rect: translate(field_lb.rect) },
                ];
                for rect in rects {
                    sel_cmd.push(lumen_paint::DisplayCommand::FillRect {
                        rect: translate(rect),
                        color: forms::SELECTION_HIGHLIGHT_DEFAULT,
                    });
                }
                sel_cmd.push(lumen_paint::DisplayCommand::PopClip);
                sel_cmd.push(lumen_paint::DisplayCommand::PopClip);
                if let Some(dl) = anim_dl.as_mut() {
                    dl.append(&mut sel_cmd);
                } else {
                    let mut buf = page_buf.take().unwrap_or_else(|| self.display_list.clone());
                    buf.append(&mut sel_cmd);
                    page_buf = Some(buf);
                }
            }
        }
        if let Some(handle) = self.focused_frame.and_then(|(idx, _)| self.frames.get(idx))
            && let Some((fidx, nid, cursor, value)) = self.focused_frame_textarea_caret()
            && self.focused_frame_textarea_selection().is_none()
            && let Some(field_lb) = handle.layout.as_ref().and_then(|lb| forms::find_layout_box(lb, nid))
            && let Some((ox, oy)) = frames::frame_page_origin(&self.frames, fidx)
            && let Ok(font) = lumen_font::Font::parse(INTER_FONT)
            && let Ok(m) = lumen_paint::FontMeasurer::new(&font)
        {
            let fs = field_lb.style.font_size;
            let measure = |s: &str| -> f32 {
                use lumen_layout::TextMeasurer;
                s.chars().map(|c| m.char_width(c, fs)).sum()
            };
            let rect = forms::textarea_caret_rect(field_lb, &value, cursor, &measure);
            let color = field_lb.style.caret_color.unwrap_or(field_lb.style.color);
            let translate = |r: lumen_core::geom::Rect| lumen_core::geom::Rect {
                x: r.x + ox,
                y: r.y + oy,
                ..r
            };
            let viewport_rect = lumen_core::geom::Rect {
                x: ox,
                y: oy,
                width: handle.viewport.width,
                height: handle.viewport.height,
            };
            let mut caret_cmd = vec![
                lumen_paint::DisplayCommand::PushClipRect { rect: viewport_rect },
                lumen_paint::DisplayCommand::PushClipRect { rect: translate(field_lb.rect) },
                lumen_paint::DisplayCommand::FillRect { rect: translate(rect), color },
                lumen_paint::DisplayCommand::PopClip,
                lumen_paint::DisplayCommand::PopClip,
            ];
            if let Some(dl) = anim_dl.as_mut() {
                dl.append(&mut caret_cmd);
            } else {
                let mut buf = page_buf
                    .take()
                    .unwrap_or_else(|| self.display_list.clone());
                buf.append(&mut caret_cmd);
                page_buf = Some(buf);
            }
        }

        // DS-15: Anonymous profile draws a thin red inset outline
        // around the whole window (design ref: `box-shadow: inset 0
        // 0 0 2px var(--accent)` on `.app-frame`). Appended last of
        // all chrome overlays so it sits above every panel/modal —
        // web content itself is never touched, only the chrome layer.
        if active_profile_name.as_deref().is_some_and(panels::profile_menu::is_anonymous) {
            let win_w = self.viewport_width_css();
            let win_h = self.window_height_css();
            overlay_buf.append(&mut panels::themes::anonymous_border(
                win_w,
                win_h,
                theme_tokens::profile::ANONYMOUS,
            ));
        }

        if let Some(t0) = frame_log_t0 {
            bmarks[2] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Build the split-view combined DL before borrowing renderer,
        // so the immutable borrow of self.split_view ends first.
        let split_combined: Option<lumen_paint::DisplayList> = {
            let base_ref: &[lumen_paint::DisplayCommand] = anim_dl
                .as_deref()
                .or(page_buf.as_deref())
                .unwrap_or(&self.display_list);
            if let Some(ref sv) = self.split_view {
                let vp_w = self.viewport_width_css();
                let tab_h = toolbar::CHROME_H;
                let vp_full_h = self.viewport_height_css() + tab_h;
                let split_x = (vp_w / 2.0).floor();
                Some(sv.build_combined_dl(
                    base_ref,
                    scroll_y,
                    scroll_x,
                    split_x,
                    tab_h,
                    vp_full_h,
                    &pal,
                ))
            } else {
                None
            }
        };

        // DevTools inspector box-model overlay, in page coordinates so it
        // rides the same scroll/tab-bar transform as the page content.
        // Built before borrowing the renderer to keep borrows disjoint.
        let inspector_box_dl: lumen_paint::DisplayList = if self.dom_inspector.visible {
            if let Some(lb) = self.layout_box.as_ref() {
                let vp = Size::new(
                    self.viewport_width_css(),
                    self.viewport_height_css(),
                );
                devtools::inspector::build_box_overlay(
                    &self.dom_inspector,
                    lb,
                    vp,
                    (0.0, 0.0),
                )
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Width of any left-docked sidebar, computed before the renderer
        // borrow so the page transform can shift right of it. Cross-dock
        // aware across all four sidebars (tabs / AI / web).
        //
        // CC-4 (docs/tasks/p1-css-chrome.md): both offsets come from the
        // engine-drawn chrome's `#contentArea` rect (the brief's
        // "#page-host") — the same [`Self::page_offset`] every input path
        // reads, so a click lands on the element actually painted there.
        let (page_x_offset, page_y_offset) = self.page_offset();

        // CSS Backgrounds §3.11.1: clear the whole surface to the canvas
        // background (the root element's propagated background color) so the
        // page background fills the viewport even when the page box is smaller
        // than the window. Computed before borrowing the renderer mutably.
        let canvas_bg = self
            .layout_box
            .as_ref()
            .and_then(lumen_layout::canvas_background_color);

        if let Some(t0) = frame_log_t0 {
            marks[4] = t0.elapsed().as_secs_f64() * 1e3;
        }
        // Длина overlay-буфера на входе в рендерер: перепись пасса
        // (срез 36) считала её изнутри paint, здесь она нужна рядом с
        // раскладкой хрома, чтобы видеть долю хрома в overlay.
        let overlay_len = overlay_buf.len();
        // BUG-405 срез 57 (п.85): хром остаётся хвостом `overlay_buf` ровно
        // когда после снимка `overlay_len_after_prepend_phase` в буфер
        // ничего не дописано — тогда его дайджесты, посчитанные вместе с
        // `ChromeOverlayFrameCache` (при HIT — БЕЗ пересчёта), валидны для
        // `overlay_buf[overlay_len - chrome_len..]` и рендереру не нужно
        // хэшировать их заново в `fold_overlay`.
        let overlay_digest_reuse = chrome_tail_digests.as_ref().and_then(|(chrome_len, digests)| {
            (overlay_len == overlay_len_after_prepend_phase && *chrome_len <= overlay_len)
                .then(|| (overlay_len - chrome_len, digests.clone()))
        });
        // BUG-405 срез 34: снимок счётчика печати пофазного лога — его
        // дельта за кадр и есть цена инструмента внутри `paint`.
        let log_nanos_at_paint = frame_log_nanos();
        // BUG-405 срез 37: те же дельта-снимки для подстатей рендерера.
        let phase_at_paint = frame_phase_ms();
        // BUG-405 slice 44: same delta-snapshot for the pre-`ComposeMarks`
        // gap (`PRE_MARKS_NANOS`) — see `pre_marks_nanos` doc comment.
        let pre_marks_at_paint = pre_marks_nanos();
        // BUG-405 slice 44: same delta-snapshot for `POST_CACHE_NANOS` — see
        // `post_cache_nanos` doc comment.
        let post_cache_at_paint = post_cache_nanos();
        // BUG-405 slice 45: same delta-snapshot for `TAIL_NANOS` — see
        // `tail_nanos` doc comment.
        let tail_at_paint = tail_nanos();

        // BUG-405 срез 37: цена ОБЁРТКИ страницы, платимая внутри окна
        // `paint`, но снаружи всех счётчиков рендерера. Фаст-пас
        // `supports_page_offset` умеет рисовать список по ссылке, но
        // его отвечает `true` только femtovg — на штатном wgpu-бэкенде
        // берётся ветка ниже, и она копирует весь display list каждый
        // кадр. Без этой отсечки цена копии сидела бы в невязке.
        let mut wrap_ms = 0.0_f64;
        // BUG-405 slice 44: shell-side setup between `marks[4]` and the
        // actual `render`/`render_with_anim` call — overlay/counter
        // snapshots, `set_canvas_background`/`set_page_offset`/
        // `set_content_epoch`, branch selection. On the fast path this used
        // to be entirely unnamed (only the fallback branch's `wrap_ms`
        // covered a piece of it); set right before each of the three call
        // sites below, so it never includes the render call itself.
        let mut setup_ms = 0.0_f64;
        // BUG-405 срез 39: версия списка для рендерера. Ненулевая ровно
        // тогда, когда в рендерер уходит retained-список страницы — у
        // производных списков (анимационная патч-копия `anim_dl`,
        // подсветка поиска `page_buf`, split-view, обёрнутая копия
        // фолбэка) версии нет, и мемоизация свёртки для них выключена.
        let retained_epoch = if anim_dl.is_none() && page_buf.is_none() {
            self.display_list_epoch
        } else {
            0
        };
        if let Some(r) = self.renderer.as_mut() {
            r.set_canvas_background(canvas_bg);
            r.set_overlay_digest_reuse(overlay_digest_reuse);
            if let Some(combined) = split_combined {
                // Split-view mode: combined DL with baked scroll; renderer gets 0,0.
                r.set_page_offset(0.0, 0.0);
                r.set_content_epoch(0);
                if let Some(t0) = frame_log_t0 {
                    setup_ms = t0.elapsed().as_secs_f64() * 1e3 - marks[4];
                }
                if let Err(err) = r.render(&combined, &overlay_buf, 0.0, 0.0) {
                    eprintln!("Ошибка рендера (split): {err:?}");
                }
            } else {
                // Normal single-pane mode: shift page below tab bar (and right of
                // vertical tabs panel when it is visible).
                let base: &[lumen_paint::DisplayCommand] = anim_dl
                    .as_deref()
                    .or(page_buf.as_deref())
                    .unwrap_or(&self.display_list);
                // ADR-016 M0.4 fast path: когда единственная обёртка вокруг
                // страницы — фиксированный page-offset (нет inspector-оверлея,
                // который обязан ехать ВНУТРИ page-трансформа), а бэкенд умеет
                // накладывать смещение сам, рисуем display-list ПО ССЫЛКЕ.
                // Раньше каждый кадр (в т.ч. на каждом кадре инерционного
                // скролла) сюда копировался весь список ради одного
                // `PushTransform` — O(n) глубокий клон команд.
                // Anim-split диапазоны фаст-пасу не мешают: femtovg их
                // игнорирует и рисует монолитом (контент списка тот же), а
                // wgpu-рендерер (BUG-405 срез 38 — он тоже отвечает
                // supports_page_offset=true) берёт их как есть: без обёртки
                // индексы команд НЕ сдвинуты, поэтому диапазоны идут в
                // рендерер без «+1» фолбэка ниже.
                if inspector_box_dl.is_empty()
                    && r.supports_page_offset()
                    && !page_offset_fast_disabled()
                {
                    r.set_page_offset(page_x_offset, page_y_offset);
                    // Фаст-пас отдаёт `base` по ссылке — это и есть тот
                    // список, к которому относится версия.
                    r.set_content_epoch(retained_epoch);
                    let ranges: &[std::ops::Range<usize>] =
                        if anim_dl.is_some() { &anim_ranges } else { &[] };
                    if let Some(t0) = frame_log_t0 {
                        setup_ms = t0.elapsed().as_secs_f64() * 1e3 - marks[4];
                    }
                    if let Err(err) = r.render_with_anim(
                        base,
                        &overlay_buf,
                        scroll_y,
                        scroll_x,
                        ranges,
                    ) {
                        eprintln!("Ошибка рендера: {err:?}");
                    }
                } else {
                    // Fallback: активен inspector-оверлей или бэкенд не
                    // поддерживает page-offset — оборачиваем контент в
                    // `PushTransform`, как раньше. Anim-split диапазоны
                    // (static/animated split скролл-композитора wgpu-пути)
                    // прокидываются через render_with_anim.
                    r.set_page_offset(0.0, 0.0);
                    // Фолбэк собирает НОВЫЙ список каждый кадр —
                    // версии у него нет (BUG-405 срез 39).
                    r.set_content_epoch(0);
                    let t_wrap = frame_log_t0.map(|t0| t0.elapsed());
                    let mut shifted: lumen_paint::DisplayList =
                        Vec::with_capacity(base.len() + 2);
                    shifted.push(lumen_paint::DisplayCommand::PushTransform {
                        matrix: Mat4::translation_2d(
                            page_x_offset,
                            page_y_offset,
                        ),
                    });
                    shifted.extend_from_slice(base);
                    // Inspector box-model overlay rides inside the page transform.
                    shifted.extend_from_slice(&inspector_box_dl);
                    shifted.push(lumen_paint::DisplayCommand::PopTransform);
                    // Split-диапазоны валидны только когда base == anim_dl;
                    // +1 — сдвиг на prepended PushTransform страницы.
                    let shifted_ranges: Vec<std::ops::Range<usize>> =
                        if anim_dl.is_some() {
                            anim_ranges
                                .iter()
                                .map(|rr| rr.start + 1..rr.end + 1)
                                .collect()
                        } else {
                            Vec::new()
                        };
                    if let (Some(t0), Some(before)) = (frame_log_t0, t_wrap) {
                        wrap_ms = (t0.elapsed() - before).as_secs_f64() * 1e3;
                    }
                    if let Some(t0) = frame_log_t0 {
                        setup_ms = t0.elapsed().as_secs_f64() * 1e3 - marks[4];
                    }
                    if let Err(err) = r.render_with_anim(
                        &shifted,
                        &overlay_buf,
                        scroll_y,
                        scroll_x,
                        &shifted_ranges,
                    ) {
                        eprintln!("Ошибка рендера: {err:?}");
                    }
                }
            }
        }

        if let Some(t0) = frame_log_t0 {
            let frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
            marks[5] = frame_ms;
            self.frame_stats.record(frame_ms as f32);
            eprintln!(
                "[frame] total {frame_ms:6.2}ms  (scroll_y {:.0}, dl {} cmds)",
                self.scroll_y,
                self.display_list.len(),
            );
            // BUG-405 срез 34: шаги handler-а как ИНТЕРВАЛЫ между
            // метками. `scroll` — шаги 1/1.6/1.7 плюс порог
            // fast-scroll, `sda` — шаг 1.5, `anim` — 2/2b/2.5/2.6,
            // `js` — 3/3.1/4/5 (rAF, релейаут по грязному DOM,
            // paint-timing), `build` — сборка overlay/chrome/anim-DL
            // шага 6 ДО обращения к рендереру, `paint` — сам вызов
            // рендерера (то, что изнутри печатает `[frame:wgpu]`).
            // `log` — сколько из `paint` съела печать самого пофазного
            // блока рендерера (на попадании она крупнее всей работы
            // кадра); честная цена кадра = total − log.
            let log_ms = (frame_log_nanos() - log_nanos_at_paint) as f64 / 1e6;
            eprintln!(
                "[frame]   top: scroll {:.2} sda {:.2} anim {:.2} js {:.2} \
                         build {:.2} paint {:.2} (лог {:.2})",
                marks[0],
                marks[1] - marks[0],
                marks[2] - marks[1],
                marks[3] - marks[2],
                marks[4] - marks[3],
                marks[5] - marks[4],
                log_ms,
            );
            // BUG-405 срез 37: подстатьи `build` плюс признак кадра.
            // `band` берётся счётчиком, а не строкой `page-compose
            // HIT` (она печатается только на уровне 2, чья надбавка
            // крупнее самого кадра попадания — пункт 71), поэтому
            // разбивку можно снимать на уровне 1.
            eprintln!(
                "[frame]   build: chrome {:.2} sbar {:.2} panels {:.2} \
                         tail {:.2} | chrome {}x{}={} cmds, overlay {} | band {}",
                bmarks[0] - marks[3],
                bmarks[1] - bmarks[0],
                bmarks[2] - bmarks[1],
                marks[4] - bmarks[2],
                chrome_mix.0,
                chrome_mix.1,
                chrome_mix.2,
                overlay_len,
                compose_outcome_label(),
            );
            // Разбивка статьи `paint`. Печатается ПОСЛЕ таймера кадра,
            // поэтому в измеряемое окно не попадает — в отличие от
            // пофазного блока уровня 2 (пункт 71).
            let ph = frame_phase_ms();
            let d = |i: usize| ph[i] - phase_at_paint[i];
            // BUG-405 slice 44: gap before `ComposeMarks::new()` starts —
            // see `pre_marks_nanos` doc comment. Named separately from
            // `d(0)` (`prep`, a `ComposeMarks`-relative phase) because it
            // happens strictly BEFORE that timer starts.
            let pre_marks_ms = (pre_marks_nanos() - pre_marks_at_paint) as f64 / 1e6;
            let post_cache_ms = (post_cache_nanos() - post_cache_at_paint) as f64 / 1e6;
            // BUG-405 slice 45: gap between the `FRAME_PHASE_NANOS[3]`
            // (`пасс`) snapshot and `render_impl`'s own return — see
            // `tail_nanos` doc comment. Third candidate for the п.84
            // residual, after slice 44 ruled out `предметки`/`послекэша`.
            let tail_ms = (tail_nanos() - tail_at_paint) as f64 / 1e6;
            // BUG-405 slice 44: `setup_ms` (marks[4] -> just before the
            // `render`/`render_with_anim` call) is a SUPERSET of `wrap_ms`
            // on the fallback branch (the `shifted`-list build is a
            // sub-interval of it) — `wrap_ms` stays out of `named` to avoid
            // double-counting that sub-interval; it is still printed on its
            // own for the `offset` A/B arm in `build_phase_census.py`, which
            // isolates exactly that sub-cost.
            let named = d(0) + d(1) + d(2) + d(3) + log_ms
                + pre_marks_ms + post_cache_ms + setup_ms + tail_ms;
            eprintln!(
                "[frame]   paint: prep {:.2} hash {:.2} band {:.2} пасс {:.2} \
                         лог {:.2} предметки {:.2} послекэша {:.2} предвызов {:.2} \
                         хвост {:.2} обёртка {:.2} | невязка {:.2}",
                d(0),
                d(1),
                d(2),
                d(3),
                log_ms,
                pre_marks_ms,
                post_cache_ms,
                setup_ms,
                tail_ms,
                wrap_ms,
                (marks[5] - marks[4] - named).max(0.0),
            );
            // ADR-016 M0.5: classify this frame against the previous one
            // via the split fingerprint (content hash ⟂ scroll/page
            // offset). Split-view bakes scroll into the display list, so
            // the content/offset split does not apply there — skip it.
            // Costs an O(n) content hash, but only under LUMEN_FRAME_LOG.
            if self.split_view.is_none() {
                let base: &[lumen_paint::DisplayCommand] = anim_dl
                    .as_deref()
                    .or(page_buf.as_deref())
                    .unwrap_or(&self.display_list);
                let (sw, sh) = self.window.as_ref().map_or((1024, 720), |w| {
                    let s = w.inner_size();
                    (s.width, s.height)
                });
                let fp = lumen_paint::FrameFingerprint::new(
                    base,
                    sw,
                    sh,
                    (self.scroll_x, self.scroll_y),
                    (page_x_offset, toolbar::CHROME_H),
                );
                match self.last_frame_fp.map(|p| fp.delta_from(&p)) {
                    Some(d) => eprintln!("[frame] delta {d:?}"),
                    None => eprintln!("[frame] delta first"),
                }
                // ADR-016 M3.2.0: classify the frame against the retained
                // overscan band (blit / blit+expose / repaint) using the
                // scroll-independent content hash. Measurement only — the
                // femtovg backend does not yet own the content surface, so
                // this just reports the band mix real scrolling would hit
                // before the GL blit path (M3.2.1) acts on it. Record the
                // non-blit plans so the band re-seats exactly as the future
                // backend will after `record_repaint`.
                let content_hash = fp.content_hash;
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0_f32, |w| w.scale_factor() as f32);
                let viewport = (sw as f32 / scale, sh as f32 / scale);
                let plan = self.scroll_cache.plan(
                    content_hash,
                    (self.scroll_x, self.scroll_y),
                    viewport,
                );
                eprintln!("[frame] band {}", plan.label());
                match plan {
                    lumen_paint::ScrollFramePlan::Repaint { origin, size }
                    | lumen_paint::ScrollFramePlan::BlitAndExpose {
                        origin,
                        size,
                        ..
                    } => self.scroll_cache.record_repaint(content_hash, origin, size),
                    lumen_paint::ScrollFramePlan::Blit { .. } => {}
                }
                self.last_frame_fp = Some(fp);
            }
        }
        if let Some(t0) = bench_t0 {
            bench_frames::record_frame(t0.elapsed().as_secs_f64() * 1e3);
        }
    }
}
