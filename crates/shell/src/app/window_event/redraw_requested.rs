//! Ветка `WindowEvent::RedrawRequested` цикла событий (SPLIT-SH2).
//!
//! Тело ветки вынесено из `Lumen::window_event` (`main.rs`) как есть, с
//! дедентом на 8 пробелов и без единой правки логики; строки внутри
//! многострочных строковых литералов дедент не затронул.

use crate::*;

impl Lumen {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn on_redraw_requested(&mut self) {
        // HTML В§8.1.5.1 В«Update the renderingВ» вЂ” spec-correct order:
        //   1.   scroll              в†ђ advance_scroll_anim + advance_momentum
        //   1.5  scroll-driven anims в†ђ deliver_scroll_progress в†’ ScrollTimeline.currentTime
        //   2.   CSS Animations + Transitions tick  (spec: update animations before rAF)
        //   3.   rAF callbacks       в†ђ runtime.run_rendering_step + JS run_animation_frame
        //   4.   layout invalidation в†ђ relayout() if dom_dirty after rAF
        //        в†’ deliver_layout_observers() (ResizeObserver + IntersectionObserver)
        //   5.   paint timing        в†ђ PerformanceObserver 'paint' entries
        //   6.   paint               в†ђ r.render(...)
        //
        // Scroll before CSS/rAF so callbacks read current scroll position.
        // CSS animations/transitions before rAF: spec В§8.1.5.1 step В«update
        // animations and send eventsВ» precedes В«run animation frame callbacksВ».
        let timestamp_ms =
            self.epoch.elapsed().as_secs_f64() * 1000.0;

        // Р”РёР°РіРЅРѕСЃС‚РёРєР° РєР°РґСЂР° (LUMEN_FRAME_LOG=1): РїРѕР»РЅРѕРµ РІСЂРµРјСЏ
        // RedrawRequested (С€Р°РіРё 1вЂ“6). Paint-С„Р°Р·Р° РѕС‚РґРµР»СЊРЅРѕ Р»РѕРіРёСЂСѓРµС‚СЃСЏ
        // Р±СЌРєРµРЅРґРѕРј СЃС‚СЂРѕРєРѕР№ `[frame] paint вЂ¦`.
        let frame_log_t0 =
            lumen_paint::frame_log_enabled().then(std::time::Instant::now);

        // Warm-frame bench (LUMEN_BENCH): С‚Р°Р№РјРµСЂ РІСЃРµРіРѕ RedrawRequested,
        // РІРєР»СЋС‡Р°СЏ skip-РїСѓС‚СЊ вЂ” РЅР° РЅС‘Рј РєР°Рє СЂР°Р· Рё РјРµСЂСЏРµС‚СЃСЏ С†РµРЅР° СЂРµС€РµРЅРёСЏ
        // В«РЅРµ СЂРёСЃРѕРІР°С‚СЊВ» (СЃРј. crates/shell/src/bench_frames.rs).
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
        // BUG-821: this is also CSSOM-View В§14 В«run the scroll stepsВ» вЂ”
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
        // still driving the position вЂ” a smooth animation, touch
        // momentum, or a scrollbar thumb held under the cursor. An
        // instant scroll (`window.scrollTo`, find-in-page, a key jump
        // that lands immediately) is `moved && settled` and gets both
        // events in this one frame, which CSSOM-View В§14 allows; an
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
        // ADR-008 В§10E.4: after scroll, evict CPU-decoded images beyond gate zone.
        self.try_discard_offscreen_images();
        // Step 1.6: content-visibility: auto (BB-4) вЂ” РїСЂРѕРїСѓС‰РµРЅРЅС‹Р№ СѓР·РµР»
        // РІРѕС€С‘Р» РІ СЂР°СЃС€РёСЂРµРЅРЅС‹Р№ viewport в†’ ratchet relevant + relayout.
        self.maybe_expand_cv_relevant();
        // Step 1.65 (BUG-852): Рё Р·РґРµСЃСЊ Р¶Рµ вЂ” РµРґРёРЅСЃС‚РІРµРЅРЅР°СЏ С‚РѕС‡РєР° РІС‹РґР°С‡Рё
        // `contentvisibilityautostatechange`. CSS Contain L2 В§4.1
        // РѕРїСЂРµРґРµР»СЏРµС‚ СЂРµР»РµРІР°РЅС‚РЅРѕСЃС‚СЊ РІРЅСѓС‚СЂРё В«update the renderingВ» Рё
        // РїСЂРѕСЃРёС‚ РїРѕСЃС‚Р°РІРёС‚СЊ СЃРѕР±С‹С‚РёРµ Р·Р°РґР°С‡РµР№; РѕС‡РµСЂРµРґСЊ РЅР°РїРѕР»РЅСЏСЋС‚ С‡РµС‚С‹СЂРµ
        // РІС‹Р·РѕРІР° `refresh_cv_state` (Р·Р°РіСЂСѓР·РєР°, СЂРµР»РµР№Р°СѓС‚, РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёРµ,
        // ratchet РІС‹С€Рµ), Р° JS-РєРѕРЅС‚РµРєСЃС‚ Рє СЌС‚РѕРјСѓ С€Р°РіСѓ СѓР¶Рµ РµСЃС‚СЊ РЅР° РІСЃРµС…
        // РїСѓС‚СЏС… вЂ” РЅР° РґРІСѓС… РёР· РЅРёС… РІ РјРѕРјРµРЅС‚ СЃР°РјРѕРіРѕ `refresh_cv_state` РµС‰С‘
        // РЅРµС‚.
        #[cfg(feature = "v8")]
        self.deliver_cv_state_changes();
        // Step 1.7 (BUG-735): РєР°СЂС‚РёРЅРєРё, РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ streaming/
        // РґРёРЅР°РјРёС‡РµСЃРєРёРј РїСѓС‚С‘Рј СЃ РїСЂРѕС€Р»РѕРіРѕ РєР°РґСЂР°, РѕС‚РґР°СЋС‚ DOM-Сѓ СЃРІРѕРё
        // intrinsic-СЂР°Р·РјРµСЂС‹ (РєРѕР°Р»РµСЃС†РёСЂРѕРІР°РЅРЅРѕ: РѕРґРЅР° РїР°С‡РєР° вЂ” РѕРґРёРЅ СЂРµР»РµР№Р°СѓС‚).
        self.apply_stream_intrinsic_sizes();

        // Fast-scroll РґРµРіСЂР°РґР°С†РёСЏ (EXPERIMENT.md В§2 СЃСЂРµР· 2, РїСЂРёРЅС†РёРї
        // РїРѕР»СЊР·РѕРІР°С‚РµР»СЏ 2026-07-10: С‡РµРј Р±С‹СЃС‚СЂРµРµ СЃРєСЂРѕР»Р», С‚РµРј РјРµРЅСЊС€Рµ
        // РїРѕР»СЊР·РѕРІР°С‚РµР»СЋ РІР°Р¶РЅРѕ СЃРѕРґРµСЂР¶РёРјРѕРµ). РџСЂРё Р±С‹СЃС‚СЂРѕРј СЃРєСЂРѕР»Р»Рµ
        // Р·Р°РјРѕСЂР°Р¶РёРІР°СЋС‚СЃСЏ РРЎРўРћР§РќРРљР РёР·РјРµРЅРµРЅРёР№ РєРѕРЅС‚РµРЅС‚Р° вЂ” С‚РёРєРё
        // CSS-Р°РЅРёРјР°С†РёР№/transitions (Step 2), GIF (Step 2.5) Рё
        // video-GIF (Step 2.6). Display list СЃС‚Р°РЅРѕРІРёС‚СЃСЏ
        // scroll-СЃС‚Р°Р±РёР»СЊРЅС‹Рј, Рё РєР°РґСЂ СЃРєСЂРѕР»Р»Р° СѓС…РѕРґРёС‚ РІ page-compose HIT
        // (~2 РјСЃ) РІРјРµСЃС‚Рѕ РјРѕРЅРѕР»РёС‚РЅРѕР№ РїРµСЂРµСЂРёСЃРѕРІРєРё. РђРЅРёРјР°С†РёРё time-based:
        // РїСЂРё РІС‹С…РѕРґРµ РёР· СЂРµР¶РёРјР° РѕРЅРё СЃР°РјРё РґРѕРіРѕРЅСЏСЋС‚ С‚РµРєСѓС‰РµРµ РІСЂРµРјСЏ,
        // В«РїР°СѓР·Р°В» РІРёРґРЅР° С‚РѕР»СЊРєРѕ РІРѕ РІСЂРµРјСЏ Р±С‹СЃС‚СЂРѕР№ РїСЂРѕРєСЂСѓС‚РєРё.
        // Р“РёСЃС‚РµСЂРµР·РёСЃ РїРѕ EMA-СЃРєРѕСЂРѕСЃС‚Рё: РІС…РѕРґ в‰Ґ48 CSS px/РєР°РґСЂ (РїРѕР»РЅС‹Р№
        // wheel-notch Р·Р° РєР°РґСЂ), РІС‹С…РѕРґ <12. Р Р°Р·РѕРІР°СЏ РїСЂРѕРєСЂСѓС‚РєР° РєРѕР»С‘СЃРёРєРѕРј
        // РґР°С‘С‚ РѕРґРЅСѓ-РґРІРµ Р·Р°РјРѕСЂРѕР¶РµРЅРЅС‹С… РїР°СЂС‹ РєР°РґСЂРѕРІ, РїР»Р°РІРЅС‹Р№ С‚СЂРµРєРїР°Рґ РЅРµ
        // РІС…РѕРґРёС‚ РІ СЂРµР¶РёРј РІРѕРІСЃРµ. LUMEN_NO_FAST_SCROLL_DEGRADE=1 вЂ” РІС‹РєР».
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
            // Р—Р°РјРѕСЂРѕР¶РµРЅРЅС‹Рј РёСЃС‚РѕС‡РЅРёРєР°Рј РЅСѓР¶РµРЅ Р¶РёРІРѕР№ С†РёРєР» РєР°РґСЂРѕРІ: РЅР°
            // РєР°РґСЂРµ, РіРґРµ СЃРєРѕСЂРѕСЃС‚СЊ СѓРїР°РґС‘С‚ РЅРёР¶Рµ РїРѕСЂРѕРіР°, С‚РёРєРё
            // РІРѕР·РѕР±РЅРѕРІСЏС‚СЃСЏ Рё Р°РЅРёРјР°С†РёРё РїСЂРѕРґРѕР»Р¶Р°С‚СЃСЏ.
            self.request_redraw();
        }

        // BUG-405 СЃСЂРµР· 34 (РїСѓРЅРєС‚ 68 РѕСЃС‚Р°С‚РєР°): СЂР°Р·Р±РёРІРєР° РєР°РґСЂР° РїРѕ С€Р°РіР°Рј
        // `RedrawRequested`. `[frame] total` РјРµСЂСЏРµС‚ РІРµСЃСЊ handler, Р°
        // `[frame:wgpu] total` вЂ” С‚РѕР»СЊРєРѕ РїР°СЃСЃ РєРѕРјРїРѕР·РёС‚РѕСЂР°; РЅР° РєР°РґСЂРµ
        // РџРћРџРђР”РђРќРРЇ РїРѕР»РѕСЃС‹ РјРµР¶РґСѓ РЅРёРјРё 3.6 РёР· 4.3 РјСЃ, Рё Сѓ РЅРёС… РЅРµ Р±С‹Р»Рѕ
        // РЅРё РѕРґРЅРѕР№ СЃС‚Р°С‚СЊРё. РњРµС‚РєРё Р±РµСЂСѓС‚СЃСЏ С‚РѕР»СЊРєРѕ РїСЂРё `LUMEN_FRAME_LOG`
        // (С‚Р° Р¶Рµ `Option<Instant>`, С‡С‚Рѕ Рё Сѓ `total`), РїРѕСЌС‚РѕРјСѓ С€С‚Р°С‚РЅС‹Р№
        // РїСѓС‚СЊ РЅРµ РїР»Р°С‚РёС‚ Р·Р° РЅРёС… РЅРёС‡РµРіРѕ.
        let mut marks = [0.0_f64; 6];
        if let Some(t0) = frame_log_t0 {
            marks[0] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Step 1.5: CSS Scroll-Driven Animations вЂ” update ScrollTimeline.currentTime.
        // Spec В§8.1.5.1 step В«update scroll-linked animationsВ» precedes CSS animations.
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
        // РџСЂРё fast-scroll С‚РёРє РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ: anim_frame РѕСЃС‚Р°С‘С‚СЃСЏ СЃ РїСЂРѕС€Р»С‹РјРё
        // Р·РЅР°С‡РµРЅРёСЏРјРё в†’ РїРµСЂРµСЃРѕР±СЂР°РЅРЅС‹Р№ anim_dl РёРґРµРЅС‚РёС‡РµРЅ в†’ РєР»СЋС‡ РїРѕР»РѕСЃС‹ СЃС‚Р°Р±РёР»РµРЅ.
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
        // document's own Animations + Transitions tick вЂ” separate
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

        // Step 2.5: GIF animation вЂ” update GPU textures for frames that changed.
        // Uses the same `epoch` as rAF timestamps so GIF timing is consistent
        // with CSS animations and JS. Runs before rAF so JS can read correct img.
        // РџСЂРё fast-scroll РєР°РґСЂС‹ GIF РЅРµ РѕР±РЅРѕРІР»СЏСЋС‚СЃСЏ (register_image Р±Р°РјРїР°РµС‚
        // content_generation Рё СѓР±РёРІР°Р» Р±С‹ РєР»СЋС‡ РїРѕР»РѕСЃС‹ РєР°Р¶РґС‹Р№ С‚РёРє).
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
                    eprintln!("GIF РєР°РґСЂ {url}[{idx}]: РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅ: {e}");
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

        // Step 2.6: Video GIF animation вЂ” drain pending loads, advance frames.
        // Р—Р°РјРѕСЂРѕР¶РµРЅРѕ РїСЂРё fast-scroll РїРѕ С‚РѕР№ Р¶Рµ РїСЂРёС‡РёРЅРµ, С‡С‚Рѕ Рё Step 2.5.
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
        // Pass -1.0 в†’ JS captures performance.now() at batch start (DOMHighResTimeStamp).
        // Pass 0.0 in deterministic mode в†’ frozen timestamp per HTML В§8.1.5.1.
        // ADR-016 M2.2c-2d: СЃРЅРёРјР°РµРј РїСЂСЏРјС‹Рµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёСЏ rAF-Р±Р°С‚С‡Р°
        // (`has_raf_pending` read в†’ `route_query_js`, `run_animation_frame` void
        // в†’ `route_task_js`). РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) С‡С‚РµРЅРёСЏ вЂ”
        // Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query`, Р±Р°С‚С‡ вЂ” `task` РІ РѕС‡РµСЂРµРґСЊ РјРµР¶РґСѓ РЅРёРјРё (РїРѕСЂСЏРґРѕРє
        // has_raf_pending в†’ take_raf_pending в†’ run_animation_frame СЃРѕС…СЂР°РЅС‘РЅ, Р°
        // РїРѕСЃР»РµРґСѓСЋС‰РёР№ Step 4 `take_dom_dirty`-query РІСЃС‚Р°С‘С‚ РїРѕСЃР»Рµ Р±Р°С‚С‡-`task`);
        // Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
        let raf_due = timestamp_ms - self.last_raf_batch_ms >= RAF_MIN_INTERVAL_MS;
        if self.engine_thread.is_some() {
            // ADR-016 M2.3 (flag on): async rAF pump вЂ” never block the
            // redraw on the JS turn. `pump_raf_engine_thread` fires the
            // batch off-thread (guarded so at most one 200 ms turn is in
            // flight, regardless of scroll cadence) and, when a completed
            // turn left the DOM dirty, submits an **async** relayout whose
            // result lands via `poll_engine_commit`. Step 5's
            // `display_list.is_empty()` read may then latch paint-timing a
            // frame late (acceptable under the async contract) вЂ” the
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
            // вЂ” NOT by an unconditional `request_redraw` here. A pure rAF loop that
            // never mutates the DOM must not force a 60 fps repaint cycle; loops that
            // do mutate get a relayout + real paint from the pump.

            // Step 4: layout invalidation вЂ” РµСЃР»Рё rAF-callback РёР·РјРµРЅРёР» DOM
            // (setAttribute/textContent/appendChild/etc.), РґРµР»Р°РµРј relayout
            // РїСЂРµР¶РґРµ С‡РµРј РєСЂР°СЃРёС‚СЊ, С‡С‚РѕР±С‹ paint РѕС‚СЂР°Р¶Р°Р» Р°РєС‚СѓР°Р»СЊРЅС‹Р№ DOM.
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

        // Launch->first-frame metric (В§4 score table): fires on the
        // first frame that has page content, present happens at the
        // end of this same handler (В±1 frame accuracy is enough).
        bench_frames::log_first_frame_once(self.display_list.len());

        if let Some(t0) = frame_log_t0 {
            marks[3] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Step 5: PerformancePaintTiming (W3C Paint Timing В§2).
        // Delivered once per page load; subsequent frames skip this block.
        // first-paint = first frame with any painted pixel (non-default bg).
        // first-contentful-paint = first frame with text, image, canvas, etc.
        // Phase 0: both fire on the first non-empty display list since
        // a page load. A page load resets both flags in apply_loaded_page.
        // ADR-016 M2.2c-2d: the `is_some()` gate is preserved (the delivered
        // flags must only latch when a JS context exists вЂ” byte-identical to
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

        // BUG-405 СЃСЂРµР· 37: РїРѕРґСЃС‚Р°С‚СЊРё С€Р°РіР° 6. РЎС‚Р°С‚СЊСЏ `build` РєР°РґСЂР°
        // РџРћРџРђР”РђРќРРЇ (0.22вЂ“0.32 РјСЃ РїРѕ СЃСЂРµР·Сѓ 34) вЂ” РІС‚РѕСЂР°СЏ РїРѕ РІРµР»РёС‡РёРЅРµ
        // РїРѕСЃР»Рµ РєРѕРјРїРѕР·РёС‚РЅРѕРіРѕ РїР°СЃСЃР° Рё РІС…РѕРґРёР»Р° С‚СѓРґР° РѕРґРЅРёРј С‡РёСЃР»РѕРј.
        // `chrome` вЂ” СЂР°СЃРєР»Р°РґРєР° С…СЂРѕРјР° РїРѕ РєР»РёРї-РїРѕР»РѕСЃР°Рј РІРѕРєСЂСѓРі СЃС‚СЂР°РЅРёС†С‹,
        // `sbar` вЂ” РїРѕР»РѕСЃР° РїСЂРѕРєСЂСѓС‚РєРё, `panels` вЂ” РІСЃРµ РѕСЃС‚Р°Р»СЊРЅС‹Рµ
        // overlay-СЃС‚СЂРѕРёС‚РµР»Рё, РѕСЃС‚Р°С‚РѕРє РґРѕ `marks[4]` вЂ” С…РІРѕСЃС‚
        // (split-view, РёРЅСЃРїРµРєС‚РѕСЂ, canvas-bg).
        let mut bmarks = [0.0_f64; 3];
        // РЎРєРѕР»СЊРєРѕ РєРѕРјР°РЅРґ С…СЂРѕРј СЃС‚РѕРёС‚ С€РµР»Р»Сѓ: (РґР»РёРЅР° СЃРЅРёРјРєР° chrome_dl,
        // РЅРµРїСѓСЃС‚С‹С… РїРѕР»РѕСЃ, РёС‚РѕРі РїРѕСЃР»Рµ СЂР°СЃРєР»Р°РґРєРё). РҐСЂРѕРј РєРѕРїРёСЂСѓРµС‚СЃСЏ РІ
        // РљРђР–Р”РЈР® РїРѕР»РѕСЃСѓ С†РµР»РёРєРѕРј, РїРѕСЌС‚РѕРјСѓ РёС‚РѕРі РєСЂР°С‚РµРЅ РґР»РёРЅРµ СЃРЅРёРјРєР° вЂ” Рё
        // РёРјРµРЅРЅРѕ СЌС‚РѕС‚ РјРЅРѕР¶РёС‚РµР»СЊ РїР»Р°С‚РёС‚ РїРѕС‚РѕРј РїРµСЂРµРїРёСЃСЊ РїР°СЃСЃР° (СЃСЂРµР· 36:
        // 132 РєРѕРјР°РЅРґС‹ overlay, РёР· РЅРёС… РјРµРЅСЏРµС‚СЃСЏ РѕРґРЅР°).
        let mut chrome_mix = (0_usize, 0_usize, 0_usize);

        // Step 6 (paint): build display list buffers and call renderer.
        // Page-РїРѕР»РѕСЃР°: РёСЃС…РѕРґРЅС‹Р№ display list + highlight-FillRect-С‹
        // РїРµСЂРµРґ СЃРІРѕРёРјРё DrawText (РєРѕРіРґР° find РѕС‚РєСЂС‹С‚). РџСЂРѕРєСЂСѓС‡РёРІР°РµС‚СЃСЏ.
        // Overlay-РїРѕР»РѕСЃР°: find-bar + scrollbar вЂ” viewport-locked.
        // Р‘РµР· find вЂ” page = self.display_list, overlay = С‚РѕР»СЊРєРѕ scrollbar.
        // Resolved chrome palette for the active theme вЂ” passed to every
        // themed overlay panel so they follow the light/dark setting.
        // DS-14: the active profile's accent overrides the theme's own
        // accent preset вЂ” profile (level 0) outranks the Appearance
        // setting for this one field, matching "РїРµСЂРµРєР»СЋС‡РµРЅРёРµ РїСЂРѕС„РёР»РµР№
        // РјРµРЅСЏРµС‚ ... accent РІСЃРµРіРѕ С…СЂРѕРјР°" in the DS-14 brief.
        // DS-15: profile visual signatures (level-0 nested-frame rule).
        // Name cloned up front вЂ” the border draw below needs it after
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
                // `Self::relayout_chrome_host`) вЂ” the legacy overlay
                // builder was deleted with the rollback flag. The
                // highlighted-page overlay above is page content, not
                // chrome, and stays unconditional.
                (Some(page), Vec::new())
            } else {
                (None, Vec::new())
            };

        // CC-4 (docs/tasks/p1-css-chrome.md): the engine-drawn chrome
        // paints first вЂ” every legacy panel/scrollbar/find-bar/tab-bar/
        // toolbar built below still lands on top of it, painter's order
        // (brief: "РѕСЃС‚Р°Р»СЊРЅРѕРµ РїРѕРєР° legacy РїРѕРІРµСЂС…"). Painted through 4
        // clip "frame" strips around the page-host rect (top/bottom/
        // left/right), not one plain copy: `#contentArea`'s ancestors
        // (`body{background:var(--surface-1); height:100vh}`) still emit
        // a full-window background box even with `#contentArea` itself
        // pruned out of the layout tree (`relayout_chrome_host`) вЂ” an
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
        // above вЂ” same to_compositor_frame() mechanism the page uses
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
            let strips = [
                Rect { x: 0.0, y: 0.0, width: win_w, height: host.y },
                Rect {
                    x: 0.0,
                    y: host.y + host.height,
                    width: win_w,
                    height: (win_h - (host.y + host.height)).max(0.0),
                },
                Rect { x: 0.0, y: host.y, width: host.x, height: host.height },
                Rect {
                    x: host.x + host.width,
                    y: host.y,
                    width: (win_w - (host.x + host.width)).max(0.0),
                    height: host.height,
                },
            ];
            let mut framed = lumen_paint::DisplayList::new();
            let mut strips_used = 0_usize;
            for strip in strips {
                if strip.width <= 0.0 || strip.height <= 0.0 {
                    continue;
                }
                strips_used += 1;
                framed.push(lumen_paint::DisplayCommand::PushClipRect { rect: strip });
                framed.extend_from_slice(chrome_dl);
                framed.push(lumen_paint::DisplayCommand::PopClip);
            }
            chrome_mix = (chrome_dl.len(), strips_used, framed.len());
            // CC-7: `#omniInput` editing stays owned by the legacy
            // `address_bar` state machine вЂ” no native `<input>` caret
            // exists (`crates/chrome/src/model.rs` only binds the
            // *value*), so it's hand-painted here, on top of the
            // chrome document just painted above. Same simplified
            // "flush right of the field" placement `build_inline_field`
            // used for the old overlay caret (`address_bar.rs`) вЂ” not
            // per-glyph-measured, and it never needed to be while
            // `AddressBarState` only supports append/backspace at the
            // end of the string. Hidden while a dropdown suggestion is
            // selected, mirroring the same overlay behavior.
            if self.address_bar.is_open()
                && self.address_bar.selected_idx().is_none()
                && !self.address_bar.input().is_empty()
                && let Some(field) = self.chrome_omni_input_rect
            {
                framed.push(lumen_paint::DisplayCommand::FillRect {
                    rect: Rect::new(
                        field.x + field.width - 8.0,
                        field.y + 4.0,
                        2.0,
                        (field.height - 8.0).max(0.0),
                    ),
                    color: lumen_layout::Color { a: 220, ..pal.accent },
                });
            }
            framed.append(&mut overlay_buf);
            overlay_buf = framed;
        }
        if let Some(t0) = frame_log_t0 {
            bmarks[0] = t0.elapsed().as_secs_f64() * 1e3;
        }

        // Scrollbar РІСЃС‚Р°С‘С‚ РїРµСЂРµРґ find-bar РІ overlay-Р±СѓС„РµСЂРµ: СЂРёСЃСѓРµС‚СЃСЏ
        // РїРµСЂРІС‹Рј = РЅР°С…РѕРґРёС‚СЃСЏ РїРѕРґ find-bar-РѕРј РІ painter's order. РћРЅРё РЅРµ
        // РїРµСЂРµСЃРµРєР°СЋС‚СЃСЏ РїРѕ x (bar Р·Р°РЅРёРјР°РµС‚ Р»РµРІРµРµ `ww - 12`, scrollbar
        // СЃРїСЂР°РІР° РѕС‚ `ww - 8`), С‚Р°Рє С‡С‚Рѕ С„Р°РєС‚РёС‡РµСЃРєРѕРіРѕ overdraw РЅРµС‚.
        // --no-scrollbar РїРѕРґР°РІР»СЏРµС‚ РїРѕР»РѕСЃСѓ РґР»СЏ screenshot-РїР°Р№РїР»Р°Р№РЅР°.
        if !self.no_scrollbar {
            let scrollbar_cmds = scrollbar::build_scrollbar_overlay(
                self.scroll_y,
                self.content_height,
                self.viewport_width_css(),
                self.viewport_height_css(),
            );
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

        // <dialog> modal overlay (L-2) вЂ” ::backdrop + centered dialog above page.
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

        // Compositor offload: РµСЃР»Рё РµСЃС‚СЊ Р°РєС‚РёРІРЅС‹Рµ Р°РЅРёРјР°С†РёРё СЃ opacity/transform/
        // color/background-color вЂ” РїРµСЂРµСЃРѕР±РёСЂР°РµРј display list РёР· layout_box СЃ
        // overrides, РјРёРЅСѓСЏ relayout (BUG-231 СЂР°СЃРїСЂРѕСЃС‚СЂР°РЅРёР» offload РЅР° С†РІРµС‚Р°).
        // Static/animated split (EXPERIMENT.md В§2): РІРјРµСЃС‚Рµ СЃРѕ СЃРїРёСЃРєРѕРј СЃС‚СЂРѕСЏС‚СЃСЏ
        // РґРёР°РїР°Р·РѕРЅС‹ Р°РЅРёРјРёСЂСѓРµРјС‹С… СЃРµРіРјРµРЅС‚РѕРІ вЂ” СЃРєСЂРѕР»Р»-РєРѕРјРїРѕР·РёС‚РѕСЂ РєСЌС€РёСЂСѓРµС‚ РїРѕР»РѕСЃСѓ
        // РїРѕ СЃС‚Р°С‚РёРєРµ, СЃРµРіРјРµРЅС‚С‹ СЂРёСЃСѓРµС‚ РїРѕРІРµСЂС…. РџРѕР·РґРЅРµР№С€РёРµ append-С‹ РІ anim_dl
        // (cue, squiggles) РёРґСѓС‚ РІ РєРѕРЅРµС† СЃРїРёСЃРєР° Рё РґРёР°РїР°Р·РѕРЅС‹ РЅРµ СЃРґРІРёРіР°СЋС‚.
        let mut anim_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        let mut anim_dl: Option<lumen_paint::DisplayList> =
            if let (Some(frame), Some(lb)) = (&self.anim_frame, &self.layout_box) {
                let comp = frame.to_compositor_frame();
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

        // Hint overlay: viewport-locked Р±РµР№РґР¶Рё kbd-РЅР°РІРёРіР°С†РёРё.
        // Р”РѕР±Р°РІР»СЏСЋС‚СЃСЏ РїРѕСЃР»РµРґРЅРёРјРё в†’ СЂРёСЃСѓСЋС‚СЃСЏ РїРѕРІРµСЂС… scrollbar/tooltip.
        if self.hint.is_active() {
            let mut hint_cmds = hints::build_hints_overlay(&self.hint, scroll_x, scroll_y);
            overlay_buf.append(&mut hint_cmds);
        }

        // CC-10/CC-15-6: the legacy download-panel overlay lived here,
        // gated off the rollback flag вЂ” `#downloadsPanel` in the engine
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
        // the tab bar. Refresh blocked counts before rendering вЂ” kept
        // unconditional (CC-10) since `chrome_model_snapshot`'s
        // `#statTrackers` binding (CC-9) reads `blocked_total_count()`
        // and this is the only call site that refreshes it; only the
        // legacy *paint* below is gated.
        if self.shields.visible {
            self.shields.refresh();
        }

        // Note viewer overlay (В§12.2, GG-2): floating annotation panel.
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
            // Full window height including tab bar вЂ” bar is docked at bottom.
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

        // Keyboard shortcuts panel (В§D-4): centred floating overlay.
        if self.shortcuts_panel.visible {
            let win_w = self.viewport_width_css();
            let win_h = self.viewport_height_css();
            let kp_x = (win_w - panels::shortcuts_panel::PANEL_W) * 0.5;
            let kp_y = (win_h - panels::shortcuts_panel::PANEL_H) * 0.5;
            self.shortcuts_panel.build_panel(&mut overlay_buf, kp_x, kp_y, &pal);
        }

        // В§12.3 Read-later panel: right-docked overlay.
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
        // locked strip at y=0..TAB_BAR_HEIGHT) lived here вЂ” removed
        // along with `tabs::strip::build_tab_bar`/`build_tab_tooltip`/
        // `build_layout_toggle_btn`/`build_settings_btn` and
        // `toolbar::build_toolbar`. Under the engine-drawn chrome
        // (CC-4) it never ran.

        // Profile switcher dropdown (DS-14): BUG-403 вЂ” kept as a
        // legacy overlay always (CC-15-1, `docs/tasks/p1-css-chrome.md`
        // В§CC-15-1 decision), not migrated to `ChromeModel`/`bind_model`
        // like the CC-9/CC-10 panels. Its hit-test (below, in the
        // `MouseInput` handler) was already unconditional вЂ” this render
        // call must match, or a click toggles `profile_menu.visible`
        // with nothing ever drawn (the actual BUG-403 symptom) while
        // the invisible popover still eats clicks under it. Anchored
        // via `page_offset()` rather than the legacy-only
        // `toolbar::CHROME_H` constant so the dropdown lines up with
        // the engine-drawn toolbar's *measured* bottom edge, not an
        // assumed one вЂ” the same class of drift BUG-404 flags for
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

        // CC-4: tab context menu вЂ” drawn above the tab strip.
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

        // P3-spell СЃСЂРµР· 3: page spell suggestion menu вЂ” drawn above the
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

        // Picture-in-picture window (task #21) вЂ” drawn last so it floats
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

        // P3-webvtt СЃСЂРµР· 4: Р°РєС‚РёРІРЅС‹Рµ WebVTT-cue РїРѕРІРµСЂС… video-Р±РѕРєСЃРѕРІ.
        // РљРѕРјР°РЅРґС‹ РґРѕР±Р°РІР»СЏСЋС‚СЃСЏ РІ page-РїРѕР»РѕСЃСѓ (СЃРєСЂРѕР»Р»СЏС‚СЃСЏ СЃРѕ СЃС‚СЂР°РЅРёС†РµР№);
        // РїСЂРё Р°РєС‚РёРІРЅРѕРј compositor-offload вЂ” РІ anim_dl. Р’СЂРµРјСЏ
        // РІРѕСЃРїСЂРѕРёР·РІРµРґРµРЅРёСЏ Р±РµСЂС‘С‚СЃСЏ РёР· СЂРµР°Р»СЊРЅРѕРіРѕ playback-РєР»РѕРєР° РІРёРґРµРѕ
        // (`VideoGifStore`); РґР»СЏ РЅРµ-GIF/РЅРµ-Р·Р°РїСѓС‰РµРЅРЅС‹С… РІРёРґРµРѕ вЂ” С„РѕР»Р±СЌРє РЅР°
        // РІСЂРµРјСЏ РѕС‚ СЃС‚Р°СЂС‚Р° РЅР°РІРёРіР°С†РёРё.
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
                // Cue СЃРјРµРЅСЏСЋС‚СЃСЏ РІСЂРµРјРµРЅРµРј вЂ” РґРµСЂР¶РёРј С†РёРєР» РїРµСЂРµСЂРёСЃРѕРІРєРё,
                // РїРѕРєР° СЃС‚СЂР°РЅРёС†Р° СЃ СЃСѓР±С‚РёС‚СЂР°РјРё Р°РєС‚РёРІРЅР°.
                self.request_redraw();
            }
        }

        // P3-spell СЃСЂРµР· 2+3: РєСЂР°СЃРЅРѕРµ squiggly-РїРѕРґС‡С‘СЂРєРёРІР°РЅРёРµ РѕС€РёР±РѕС‡РЅС‹С…
        // СЃР»РѕРІ РІ С„РѕРєСѓСЃРЅРѕРј СЂРµРґР°РєС‚РёСЂСѓРµРјРѕРј РїРѕР»Рµ вЂ” <input>/<textarea> РёР»Рё
        // С…РѕСЃС‚ contenteditable. РџСЂРѕРІРµСЂСЏРµС‚СЃСЏ РєР°Р¶РґС‹Р№ DrawText РІРЅСѓС‚СЂРё
        // Р±РѕРєСЃР° РїРѕР»СЏ; placeholder РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ. РЎР»РѕРІР° РёР·
        // РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРѕРіРѕ СЃР»РѕРІР°СЂСЏ Рё В«РџСЂРѕРїСѓС‰РµРЅРЅС‹РµВ» РЅР° СЃРµСЃСЃРёСЋ вЂ”
        // СЃС‡РёС‚Р°СЋС‚СЃСЏ РІРµСЂРЅС‹РјРё.
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

        // DS-15: Anonymous profile draws a thin red inset outline
        // around the whole window (design ref: `box-shadow: inset 0
        // 0 0 2px var(--accent)` on `.app-frame`). Appended last of
        // all chrome overlays so it sits above every panel/modal вЂ”
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
        // "#page-host") вЂ” the same [`Self::page_offset`] every input path
        // reads, so a click lands on the element actually painted there.
        let (page_x_offset, page_y_offset) = self.page_offset();

        // CSS Backgrounds В§3.11.1: clear the whole surface to the canvas
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
        // Р”Р»РёРЅР° overlay-Р±СѓС„РµСЂР° РЅР° РІС…РѕРґРµ РІ СЂРµРЅРґРµСЂРµСЂ: РїРµСЂРµРїРёСЃСЊ РїР°СЃСЃР°
        // (СЃСЂРµР· 36) СЃС‡РёС‚Р°Р»Р° РµС‘ РёР·РЅСѓС‚СЂРё paint, Р·РґРµСЃСЊ РѕРЅР° РЅСѓР¶РЅР° СЂСЏРґРѕРј СЃ
        // СЂР°СЃРєР»Р°РґРєРѕР№ С…СЂРѕРјР°, С‡С‚РѕР±С‹ РІРёРґРµС‚СЊ РґРѕР»СЋ С…СЂРѕРјР° РІ overlay.
        let overlay_len = overlay_buf.len();
        // BUG-405 СЃСЂРµР· 34: СЃРЅРёРјРѕРє СЃС‡С‘С‚С‡РёРєР° РїРµС‡Р°С‚Рё РїРѕС„Р°Р·РЅРѕРіРѕ Р»РѕРіР° вЂ” РµРіРѕ
        // РґРµР»СЊС‚Р° Р·Р° РєР°РґСЂ Рё РµСЃС‚СЊ С†РµРЅР° РёРЅСЃС‚СЂСѓРјРµРЅС‚Р° РІРЅСѓС‚СЂРё `paint`.
        let log_nanos_at_paint = frame_log_nanos();
        // BUG-405 СЃСЂРµР· 37: С‚Рµ Р¶Рµ РґРµР»СЊС‚Р°-СЃРЅРёРјРєРё РґР»СЏ РїРѕРґСЃС‚Р°С‚РµР№ СЂРµРЅРґРµСЂРµСЂР°.
        let phase_at_paint = frame_phase_ms();
        // BUG-405 slice 44: same delta-snapshot for the pre-`ComposeMarks`
        // gap (`PRE_MARKS_NANOS`) — see `pre_marks_nanos` doc comment.
        let pre_marks_at_paint = pre_marks_nanos();
        // BUG-405 slice 44: same delta-snapshot for `POST_CACHE_NANOS` — see
        // `post_cache_nanos` doc comment.
        let post_cache_at_paint = post_cache_nanos();

        // BUG-405 СЃСЂРµР· 37: С†РµРЅР° РћР‘РЃР РўРљР СЃС‚СЂР°РЅРёС†С‹, РїР»Р°С‚РёРјР°СЏ РІРЅСѓС‚СЂРё РѕРєРЅР°
        // `paint`, РЅРѕ СЃРЅР°СЂСѓР¶Рё РІСЃРµС… СЃС‡С‘С‚С‡РёРєРѕРІ СЂРµРЅРґРµСЂРµСЂР°. Р¤Р°СЃС‚-РїР°СЃ
        // `supports_page_offset` СѓРјРµРµС‚ СЂРёСЃРѕРІР°С‚СЊ СЃРїРёСЃРѕРє РїРѕ СЃСЃС‹Р»РєРµ, РЅРѕ
        // РµРіРѕ РѕС‚РІРµС‡Р°РµС‚ `true` С‚РѕР»СЊРєРѕ femtovg вЂ” РЅР° С€С‚Р°С‚РЅРѕРј wgpu-Р±СЌРєРµРЅРґРµ
        // Р±РµСЂС‘С‚СЃСЏ РІРµС‚РєР° РЅРёР¶Рµ, Рё РѕРЅР° РєРѕРїРёСЂСѓРµС‚ РІРµСЃСЊ display list РєР°Р¶РґС‹Р№
        // РєР°РґСЂ. Р‘РµР· СЌС‚РѕР№ РѕС‚СЃРµС‡РєРё С†РµРЅР° РєРѕРїРёРё СЃРёРґРµР»Р° Р±С‹ РІ РЅРµРІСЏР·РєРµ.
        let mut wrap_ms = 0.0_f64;
        // BUG-405 slice 44: shell-side setup between `marks[4]` and the
        // actual `render`/`render_with_anim` call — overlay/counter
        // snapshots, `set_canvas_background`/`set_page_offset`/
        // `set_content_epoch`, branch selection. On the fast path this used
        // to be entirely unnamed (only the fallback branch's `wrap_ms`
        // covered a piece of it); set right before each of the three call
        // sites below, so it never includes the render call itself.
        let mut setup_ms = 0.0_f64;
        // BUG-405 СЃСЂРµР· 39: РІРµСЂСЃРёСЏ СЃРїРёСЃРєР° РґР»СЏ СЂРµРЅРґРµСЂРµСЂР°. РќРµРЅСѓР»РµРІР°СЏ СЂРѕРІРЅРѕ
        // С‚РѕРіРґР°, РєРѕРіРґР° РІ СЂРµРЅРґРµСЂРµСЂ СѓС…РѕРґРёС‚ retained-СЃРїРёСЃРѕРє СЃС‚СЂР°РЅРёС†С‹ вЂ” Сѓ
        // РїСЂРѕРёР·РІРѕРґРЅС‹С… СЃРїРёСЃРєРѕРІ (Р°РЅРёРјР°С†РёРѕРЅРЅР°СЏ РїР°С‚С‡-РєРѕРїРёСЏ `anim_dl`,
        // РїРѕРґСЃРІРµС‚РєР° РїРѕРёСЃРєР° `page_buf`, split-view, РѕР±С‘СЂРЅСѓС‚Р°СЏ РєРѕРїРёСЏ
        // С„РѕР»Р±СЌРєР°) РІРµСЂСЃРёРё РЅРµС‚, Рё РјРµРјРѕРёР·Р°С†РёСЏ СЃРІС‘СЂС‚РєРё РґР»СЏ РЅРёС… РІС‹РєР»СЋС‡РµРЅР°.
        let retained_epoch = if anim_dl.is_none() && page_buf.is_none() {
            self.display_list_epoch
        } else {
            0
        };
        if let Some(r) = self.renderer.as_mut() {
            r.set_canvas_background(canvas_bg);
            if let Some(combined) = split_combined {
                // Split-view mode: combined DL with baked scroll; renderer gets 0,0.
                r.set_page_offset(0.0, 0.0);
                r.set_content_epoch(0);
                if let Some(t0) = frame_log_t0 {
                    setup_ms = t0.elapsed().as_secs_f64() * 1e3 - marks[4];
                }
                if let Err(err) = r.render(&combined, &overlay_buf, 0.0, 0.0) {
                    eprintln!("РћС€РёР±РєР° СЂРµРЅРґРµСЂР° (split): {err:?}");
                }
            } else {
                // Normal single-pane mode: shift page below tab bar (and right of
                // vertical tabs panel when it is visible).
                let base: &[lumen_paint::DisplayCommand] = anim_dl
                    .as_deref()
                    .or(page_buf.as_deref())
                    .unwrap_or(&self.display_list);
                // ADR-016 M0.4 fast path: РєРѕРіРґР° РµРґРёРЅСЃС‚РІРµРЅРЅР°СЏ РѕР±С‘СЂС‚РєР° РІРѕРєСЂСѓРі
                // СЃС‚СЂР°РЅРёС†С‹ вЂ” С„РёРєСЃРёСЂРѕРІР°РЅРЅС‹Р№ page-offset (РЅРµС‚ inspector-РѕРІРµСЂР»РµСЏ,
                // РєРѕС‚РѕСЂС‹Р№ РѕР±СЏР·Р°РЅ РµС…Р°С‚СЊ Р’РќРЈРўР Р page-С‚СЂР°РЅСЃС„РѕСЂРјР°), Р° Р±СЌРєРµРЅРґ СѓРјРµРµС‚
                // РЅР°РєР»Р°РґС‹РІР°С‚СЊ СЃРјРµС‰РµРЅРёРµ СЃР°Рј, СЂРёСЃСѓРµРј display-list РџРћ РЎРЎР«Р›РљР•.
                // Р Р°РЅСЊС€Рµ РєР°Р¶РґС‹Р№ РєР°РґСЂ (РІ С‚.С‡. РЅР° РєР°Р¶РґРѕРј РєР°РґСЂРµ РёРЅРµСЂС†РёРѕРЅРЅРѕРіРѕ
                // СЃРєСЂРѕР»Р»Р°) СЃСЋРґР° РєРѕРїРёСЂРѕРІР°Р»СЃСЏ РІРµСЃСЊ СЃРїРёСЃРѕРє СЂР°РґРё РѕРґРЅРѕРіРѕ
                // `PushTransform` вЂ” O(n) РіР»СѓР±РѕРєРёР№ РєР»РѕРЅ РєРѕРјР°РЅРґ.
                // Anim-split РґРёР°РїР°Р·РѕРЅС‹ С„Р°СЃС‚-РїР°СЃСѓ РЅРµ РјРµС€Р°СЋС‚: femtovg РёС…
                // РёРіРЅРѕСЂРёСЂСѓРµС‚ Рё СЂРёСЃСѓРµС‚ РјРѕРЅРѕР»РёС‚РѕРј (РєРѕРЅС‚РµРЅС‚ СЃРїРёСЃРєР° С‚РѕС‚ Р¶Рµ), Р°
                // wgpu-СЂРµРЅРґРµСЂРµСЂ (BUG-405 СЃСЂРµР· 38 вЂ” РѕРЅ С‚РѕР¶Рµ РѕС‚РІРµС‡Р°РµС‚
                // supports_page_offset=true) Р±РµСЂС‘С‚ РёС… РєР°Рє РµСЃС‚СЊ: Р±РµР· РѕР±С‘СЂС‚РєРё
                // РёРЅРґРµРєСЃС‹ РєРѕРјР°РЅРґ РќР• СЃРґРІРёРЅСѓС‚С‹, РїРѕСЌС‚РѕРјСѓ РґРёР°РїР°Р·РѕРЅС‹ РёРґСѓС‚ РІ
                // СЂРµРЅРґРµСЂРµСЂ Р±РµР· В«+1В» С„РѕР»Р±СЌРєР° РЅРёР¶Рµ.
                if inspector_box_dl.is_empty()
                    && r.supports_page_offset()
                    && !page_offset_fast_disabled()
                {
                    r.set_page_offset(page_x_offset, page_y_offset);
                    // Р¤Р°СЃС‚-РїР°СЃ РѕС‚РґР°С‘С‚ `base` РїРѕ СЃСЃС‹Р»РєРµ вЂ” СЌС‚Рѕ Рё РµСЃС‚СЊ С‚РѕС‚
                    // СЃРїРёСЃРѕРє, Рє РєРѕС‚РѕСЂРѕРјСѓ РѕС‚РЅРѕСЃРёС‚СЃСЏ РІРµСЂСЃРёСЏ.
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
                        eprintln!("РћС€РёР±РєР° СЂРµРЅРґРµСЂР°: {err:?}");
                    }
                } else {
                    // Fallback: Р°РєС‚РёРІРµРЅ inspector-РѕРІРµСЂР»РµР№ РёР»Рё Р±СЌРєРµРЅРґ РЅРµ
                    // РїРѕРґРґРµСЂР¶РёРІР°РµС‚ page-offset вЂ” РѕР±РѕСЂР°С‡РёРІР°РµРј РєРѕРЅС‚РµРЅС‚ РІ
                    // `PushTransform`, РєР°Рє СЂР°РЅСЊС€Рµ. Anim-split РґРёР°РїР°Р·РѕРЅС‹
                    // (static/animated split СЃРєСЂРѕР»Р»-РєРѕРјРїРѕР·РёС‚РѕСЂР° wgpu-РїСѓС‚Рё)
                    // РїСЂРѕРєРёРґС‹РІР°СЋС‚СЃСЏ С‡РµСЂРµР· render_with_anim.
                    r.set_page_offset(0.0, 0.0);
                    // Р¤РѕР»Р±СЌРє СЃРѕР±РёСЂР°РµС‚ РќРћР’Р«Р™ СЃРїРёСЃРѕРє РєР°Р¶РґС‹Р№ РєР°РґСЂ вЂ”
                    // РІРµСЂСЃРёРё Сѓ РЅРµРіРѕ РЅРµС‚ (BUG-405 СЃСЂРµР· 39).
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
                    // Split-РґРёР°РїР°Р·РѕРЅС‹ РІР°Р»РёРґРЅС‹ С‚РѕР»СЊРєРѕ РєРѕРіРґР° base == anim_dl;
                    // +1 вЂ” СЃРґРІРёРі РЅР° prepended PushTransform СЃС‚СЂР°РЅРёС†С‹.
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
                        eprintln!("РћС€РёР±РєР° СЂРµРЅРґРµСЂР°: {err:?}");
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
            // BUG-405 СЃСЂРµР· 34: С€Р°РіРё handler-Р° РєР°Рє РРќРўР•Р Р’РђР›Р« РјРµР¶РґСѓ
            // РјРµС‚РєР°РјРё. `scroll` вЂ” С€Р°РіРё 1/1.6/1.7 РїР»СЋСЃ РїРѕСЂРѕРі
            // fast-scroll, `sda` вЂ” С€Р°Рі 1.5, `anim` вЂ” 2/2b/2.5/2.6,
            // `js` вЂ” 3/3.1/4/5 (rAF, СЂРµР»РµР№Р°СѓС‚ РїРѕ РіСЂСЏР·РЅРѕРјСѓ DOM,
            // paint-timing), `build` вЂ” СЃР±РѕСЂРєР° overlay/chrome/anim-DL
            // С€Р°РіР° 6 Р”Рћ РѕР±СЂР°С‰РµРЅРёСЏ Рє СЂРµРЅРґРµСЂРµСЂСѓ, `paint` вЂ” СЃР°Рј РІС‹Р·РѕРІ
            // СЂРµРЅРґРµСЂРµСЂР° (С‚Рѕ, С‡С‚Рѕ РёР·РЅСѓС‚СЂРё РїРµС‡Р°С‚Р°РµС‚ `[frame:wgpu]`).
            // `log` вЂ” СЃРєРѕР»СЊРєРѕ РёР· `paint` СЃСЉРµР»Р° РїРµС‡Р°С‚СЊ СЃР°РјРѕРіРѕ РїРѕС„Р°Р·РЅРѕРіРѕ
            // Р±Р»РѕРєР° СЂРµРЅРґРµСЂРµСЂР° (РЅР° РїРѕРїР°РґР°РЅРёРё РѕРЅР° РєСЂСѓРїРЅРµРµ РІСЃРµР№ СЂР°Р±РѕС‚С‹
            // РєР°РґСЂР°); С‡РµСЃС‚РЅР°СЏ С†РµРЅР° РєР°РґСЂР° = total в€’ log.
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
            // BUG-405 СЃСЂРµР· 37: РїРѕРґСЃС‚Р°С‚СЊРё `build` РїР»СЋСЃ РїСЂРёР·РЅР°Рє РєР°РґСЂР°.
            // `band` Р±РµСЂС‘С‚СЃСЏ СЃС‡С‘С‚С‡РёРєРѕРј, Р° РЅРµ СЃС‚СЂРѕРєРѕР№ `page-compose
            // HIT` (РѕРЅР° РїРµС‡Р°С‚Р°РµС‚СЃСЏ С‚РѕР»СЊРєРѕ РЅР° СѓСЂРѕРІРЅРµ 2, С‡СЊСЏ РЅР°РґР±Р°РІРєР°
            // РєСЂСѓРїРЅРµРµ СЃР°РјРѕРіРѕ РєР°РґСЂР° РїРѕРїР°РґР°РЅРёСЏ вЂ” РїСѓРЅРєС‚ 71), РїРѕСЌС‚РѕРјСѓ
            // СЂР°Р·Р±РёРІРєСѓ РјРѕР¶РЅРѕ СЃРЅРёРјР°С‚СЊ РЅР° СѓСЂРѕРІРЅРµ 1.
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
            // Р Р°Р·Р±РёРІРєР° СЃС‚Р°С‚СЊРё `paint`. РџРµС‡Р°С‚Р°РµС‚СЃСЏ РџРћРЎР›Р• С‚Р°Р№РјРµСЂР° РєР°РґСЂР°,
            // РїРѕСЌС‚РѕРјСѓ РІ РёР·РјРµСЂСЏРµРјРѕРµ РѕРєРЅРѕ РЅРµ РїРѕРїР°РґР°РµС‚ вЂ” РІ РѕС‚Р»РёС‡РёРµ РѕС‚
            // РїРѕС„Р°Р·РЅРѕРіРѕ Р±Р»РѕРєР° СѓСЂРѕРІРЅСЏ 2 (РїСѓРЅРєС‚ 71).
            let ph = frame_phase_ms();
            let d = |i: usize| ph[i] - phase_at_paint[i];
            // BUG-405 slice 44: gap before `ComposeMarks::new()` starts —
            // see `pre_marks_nanos` doc comment. Named separately from
            // `d(0)` (`prep`, a `ComposeMarks`-relative phase) because it
            // happens strictly BEFORE that timer starts.
            let pre_marks_ms = (pre_marks_nanos() - pre_marks_at_paint) as f64 / 1e6;
            let post_cache_ms = (post_cache_nanos() - post_cache_at_paint) as f64 / 1e6;
            // BUG-405 slice 44: `setup_ms` (marks[4] -> just before the
            // `render`/`render_with_anim` call) is a SUPERSET of `wrap_ms`
            // on the fallback branch (the `shifted`-list build is a
            // sub-interval of it) — `wrap_ms` stays out of `named` to avoid
            // double-counting that sub-interval; it is still printed on its
            // own for the `offset` A/B arm in `build_phase_census.py`, which
            // isolates exactly that sub-cost.
            let named =
                d(0) + d(1) + d(2) + d(3) + log_ms + pre_marks_ms + post_cache_ms + setup_ms;
            eprintln!(
                "[frame]   paint: prep {:.2} hash {:.2} band {:.2} пасс {:.2} \
                         лог {:.2} предметки {:.2} послекэша {:.2} предвызов {:.2} \
                         обёртка {:.2} | невязка {:.2}",
                d(0),
                d(1),
                d(2),
                d(3),
                log_ms,
                pre_marks_ms,
                post_cache_ms,
                setup_ms,
                wrap_ms,
                (marks[5] - marks[4] - named).max(0.0),
            );
            // ADR-016 M0.5: classify this frame against the previous one
            // via the split fingerprint (content hash вџ‚ scroll/page
            // offset). Split-view bakes scroll into the display list, so
            // the content/offset split does not apply there вЂ” skip it.
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
                // scroll-independent content hash. Measurement only вЂ” the
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
