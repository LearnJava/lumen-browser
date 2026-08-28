//! `ApplicationHandler::about_to_wait` — работа между кадрами: таймеры,
//! анимации, drain очередей автоматизации (SPLIT-SH2).
//!
//! Тело вынесено из `impl ApplicationHandler<LoadEvent> for Lumen`
//! (`super`) как есть, вместе с его `#[allow]`.

use crate::*;

/// Р‘СЋРґР¶РµС‚ idle-РѕРєРЅР° РґР»СЏ `requestIdleCallback`-РѕРІ, РїРµСЂРµРґР°РІР°РµРјС‹Р№ РІ
/// `EventLoop::run_idle_callbacks` РЅР° РєР°Р¶РґРѕРј `about_to_wait`. Phase 0 РЅРµ Р·РЅР°РµС‚
/// СЂРµР°Р»СЊРЅРѕРіРѕ РІСЂРµРјРµРЅРё РґРѕ СЃР»РµРґСѓСЋС‰РµРіРѕ vsync, РїРѕСЌС‚РѕРјСѓ РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ С„РёРєСЃРёСЂРѕРІР°РЅРЅС‹Р№
/// 10 ms вЂ” С‚РѕС‚ Р¶Рµ РґРµС„РѕР»С‚, С‡С‚Рѕ Сѓ Chromium РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІРёРё СЏРІРЅРѕРіРѕ measurement-Р°
/// idle-РѕРєРЅР°. Idle-callback-Рё С‚СЂР°РєС‚СѓСЋС‚ СЌС‚Рѕ РєР°Рє В«СѓСЃРїРµР№ Р·Р° ~10 msВ».
const IDLE_BUDGET_MS: f64 = 10.0;

impl Lumen {
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn on_about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Warm-frame bench (LUMEN_BENCH=hover:N | scroll:N). Drives redraws
        // itself instead of waiting for a human, then exits. Placed first so a
        // benched process never falls through into the idle paths below.
        //
        // Gated on `window.is_some()`: before `resumed()` there is nothing to
        // redraw, and the first frame must come from the normal load path.
        if let Some(cfg) = bench_frames::cfg()
            && self.window.is_some()
        {
            if bench_frames::done() {
                bench_frames::report();
                event_loop.exit();
                return;
            }
            bench_frames::log_geometry_once(
                self.content_height,
                self.viewport_height_css(),
                self.max_scroll(),
                self.display_list.len(),
            );
            if cfg.mode == bench_frames::BenchMode::Scroll {
                self.scroll_y = bench_frames::next_scroll(self.scroll_y, self.max_scroll());
            }
            self.request_redraw();
        }

        // TEMP BUG-272 diagnostics: dump known-store sizes every ~10 s.
        if std::env::var("LUMEN_MEM_REPORT").is_ok() {
            let now_s = self.epoch.elapsed().as_secs_f64();
            if now_s - self.last_mem_report_s >= 10.0 {
                self.last_mem_report_s = now_s;
                let wf_bytes: usize = self.web_fonts.iter().map(|f| f.bytes.len()).sum();
                // BUG-272 СЃСЂРµР· 19: lazy GIFs hold encoded bytes + ~one decoded frame,
                // not all N frames вЂ” `resident_bytes` reflects the real footprint.
                let gif_bytes: usize = self
                    .animated_gifs
                    .values()
                    .map(lumen_image::AnimatedGif::resident_bytes)
                    .sum();
                let (img_n, img_b) = image_cache::IMAGE_CACHE.debug_stats();
                let (pf_n, pf_b) = prefetch::PREFETCH_CACHE.debug_stats();
                // ADR-016 M2.2c-2d (20): direct `self.js_ctx` read в†’ `route_query_js`.
                // Flag-off (default) `js.map(...).unwrap_or((-1,-1))` = the old
                // `map_or((-1,-1), ...)`; under the flag it is a blocking `query`.
                let js_heap = self.drain_query_js(|j| j.debug_js_heap()).unwrap_or((-1, -1));
                eprintln!(
                    "MEM_REPORT dl_cmds={} prev_styles={} dl_cache={:.1}MB img_cache={}/{:.1}MB prefetch={}/{:.1}MB web_fonts={}/{:.1}MB gifs={}/{:.1}MB js_malloc={:.1}MB js_used={:.1}MB {}",
                    self.display_list.len(),
                    self.prev_styles.len(),
                    self.display_list_cache.used_bytes() as f64 / 1e6,
                    img_n, img_b as f64 / 1e6,
                    pf_n, pf_b as f64 / 1e6,
                    self.web_fonts.len(), wf_bytes as f64 / 1e6,
                    self.animated_gifs.len(), gif_bytes as f64 / 1e6,
                    js_heap.0 as f64 / 1e6, js_heap.1 as f64 / 1e6,
                    self.renderer.as_ref().map_or(String::new(), |r| r.debug_mem_report()),
                );
                // M0.1 (ADR-016): РїРµСЂРёРѕРґРёС‡РµСЃРєР°СЏ СЃРІРѕРґРєР° РІСЂРµРјС‘РЅ РєР°РґСЂРѕРІ вЂ” Р±Р°Р·РѕРІС‹Рµ
                // С‡РёСЃР»Р°, РЅР° РєРѕС‚РѕСЂС‹Рµ Р±СѓРґСѓС‚ СЃСЃС‹Р»Р°С‚СЊСЃСЏ РїРѕСЃР»РµРґСѓСЋС‰РёРµ СЃС‚Р°РґРёРё MT-СЂРµРЅРґРµСЂР°.
                if let Some(summary) = self.frame_stats.summary() {
                    eprintln!("{summary}");
                }
                // ADR-016 M2.0: periodic UI-thread relayout-cost summary alongside
                // the frame summary вЂ” the baseline M2's engine-thread move improves.
                if let Some(summary) = self.engine_stats.summary() {
                    eprintln!("{}", summary.display_with("ENGINE_SUMMARY"));
                }
            }
        }
        // HTML В§8.1.4.2 В«Processing modelВ»: РјРµР¶РґСѓ СЃРѕР±С‹С‚РёСЏРјРё event-loop-Р°
        // РґСЂРµРЅРёСЂСѓРµРј РЅР°РєРѕРїРёРІС€РёРµСЃСЏ task-Рё. РљР°Р¶РґС‹Р№ step РІС‹РїРѕР»РЅСЏРµС‚ РѕРґРЅСѓ task +
        // microtask checkpoint. Р”СЂРµРЅРёСЂСѓРµРј РІСЃРµ pending tasks Р·Р° РѕРґРёРЅ РїСЂРѕС…РѕРґ,
        // С‡С‚РѕР±С‹ UI РЅРµ РѕС‚СЃС‚Р°РІР°Р». Р•СЃР»Рё task Р·Р°РїР»Р°РЅРёСЂСѓРµС‚ РЅРѕРІСѓСЋ task вЂ” РѕРЅР°
        // РІС‹РїРѕР»РЅРёС‚СЃСЏ РЅР° СЃР»РµРґСѓСЋС‰РµРј about_to_wait (РєР°Рє Рё `setTimeout(..., 0)`
        // РІ Р±СЂР°СѓР·РµСЂРµ).
        let mut steps = 0;
        let mut reached_idle = true;
        while self.runtime.step() == runtime::StepResult::Ran {
            steps += 1;
            if steps >= 256 {
                // Р—Р°С‰РёС‚Р° РѕС‚ runaway: РµСЃР»Рё С‡С‚Рѕ-С‚Рѕ СЂРµРєСѓСЂСЃРёРІРЅРѕ РїР»Р°РЅРёСЂСѓРµС‚ task РІ
                // СЌС‚Сѓ Р¶Рµ РёС‚РµСЂР°С†РёСЋ, РЅРµ Р±Р»РѕРєРёСЂСѓРµРј UI Р±РѕР»СЊС€Рµ С‡РµРј РЅР° 256 task-РѕРІ;
                // РѕСЃС‚Р°С‚РѕРє РѕР±СЂР°Р±РѕС‚Р°РµС‚СЃСЏ РІ СЃР»РµРґСѓСЋС‰РµРј about_to_wait.
                reached_idle = false;
                break;
            }
        }

        // W3C `requestIdleCallback` В§3: РїРѕСЃР»Рµ РґСЂРµРЅР°Р¶Р° РѕС‡РµСЂРµРґРё task-РѕРІ event-loop
        // СЃРѕРѕР±С‰Р°РµС‚ В«idle windowВ». Phase 0 РЅРµ Р·РЅР°РµС‚ СЂРµР°Р»СЊРЅРѕРіРѕ Р±СЋРґР¶РµС‚Р° (РЅРµС‚
        // РїСЂРёРІСЏР·РєРё Рє vsync), РїРѕСЌС‚РѕРјСѓ РїРµСЂРµРґР°С‘Рј С„РёРєСЃРёСЂРѕРІР°РЅРЅС‹Рµ `IDLE_BUDGET_MS`
        // РєРѕРіРґР° РґРѕС€Р»Рё РґРѕ StepResult::Idle. Р•СЃР»Рё СѓРїС‘СЂР»РёСЃСЊ РІ cap=256 вЂ” РµСЃС‚СЊ РµС‰С‘
        // pending tasks, РЅРµ idle: РїРµСЂРµРґР°С‘Рј 0 ms, С‡С‚РѕР±С‹ СЃСЂР°Р±РѕС‚Р°Р»Рё С‚РѕР»СЊРєРѕ
        // timeout-callback-Рё (`request_idle_callback(..., timeout_ms)`).
        // Р‘РµР· СЌС‚РѕРіРѕ РІС‹Р·РѕРІР° registered idle-callback-Рё РЅРµ РїРѕР»СѓС‡Р°СЋС‚ С€Р°РЅСЃР°
        // РѕС‚СЂР°Р±РѕС‚Р°С‚СЊ РІ РїСЂРёРЅС†РёРїРµ.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let remaining_ms = if reached_idle { IDLE_BUDGET_MS } else { 0.0 };
        self.runtime.run_idle_callbacks(remaining_ms, now_ms);

        // JS timers: drain expired setTimeout/setInterval callbacks, then read
        // the next wakeup deadline to schedule ControlFlow::WaitUntil so that
        // winit wakes up exactly when the next timer fires (not only on OS events).
        // WebSocket pump runs here too so onopen/onmessage/onclose fire promptly.
        //
        // BUG-271: the WaitUntil deadline is the earliest of the JS-timer
        // deadline and the rAF-pump deadline (set below), so neither wakeup
        // source can starve the other.
        let mut next_wakeup: Option<std::time::Instant> = None;
        // BUG-480 СЃСЂРµР· 1: РїР°РјРї sub-РґРѕРєСѓРјРµРЅС‚РѕРІ <iframe>. РҐСЌРЅРґР»С‹ С„СЂРµР№РјРѕРІ Р¶РёРІСѓС‚
        // С‚РѕР»СЊРєРѕ РЅР° UI-СЃС‚РѕСЂРѕРЅРµ (РІ EngineJsState РёС… РЅРµС‚), РїРѕСЌС‚РѕРјСѓ РїСЂСЏРјС‹Рµ РІС‹Р·РѕРІС‹
        // Р±РµР· route_*: V8PersistentJs СЃР°Рј С‚СѓРЅРЅРµР»РёСЂСѓРµС‚ РЅР° JS-РїРѕС‚РѕРє (ADR-014).
        // Р’РЅРµ РіРµР№С‚Р° `js_present`: Сѓ С„СЂРµР№РјР° РјРѕР¶РµС‚ Р±С‹С‚СЊ СЃРєСЂРёРїС‚ РїСЂРё СЃС‚СЂР°РЅРёС†Рµ Р±РµР·
        // РµРґРёРЅРѕРіРѕ СЃРєСЂРёРїС‚Р°. rAF С„СЂРµР№РјРѕРІ РЅРµ С‚РёРєР°РµС‚СЃСЏ (СЃСЂРµР· 1 вЂ” СЃРј. Р±Р°Рі-С„Р°Р№Р»).
        // РРЅРґРµРєСЃ С…СЌРЅРґР»Р° РµРґРµС‚ СЂСЏРґРѕРј СЃ СЂР°РЅС‚Р°Р№РјРѕРј (СЃСЂРµР· 17): С„СЂРµР№Рј Р±РµР· СЃРєСЂРёРїС‚РѕРІ
        // РІ СЌС‚РѕС‚ СЃРїРёСЃРѕРє РЅРµ РїРѕРїР°РґР°РµС‚, РїРѕСЌС‚РѕРјСѓ РїРѕР·РёС†РёСЏ РІ РЅС‘Рј РЅРµ СЂР°РІРЅР° РїРѕР·РёС†РёРё РІ
        // `self.frames`, Р° РїСЂРѕРєСЂСѓС‚РєР° Р°РґСЂРµСЃСѓРµС‚СЃСЏ РёРјРµРЅРЅРѕ РІС‚РѕСЂРѕР№.
        let frame_js_handles: Vec<(usize, Arc<dyn PersistentJs>)> = self
            .frames
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.js.clone().map(|js| (i, js)))
            .collect();
        // BUG-480 СЃСЂРµР· 17: `window.scrollTo`/`scrollBy` РёР· СЃРєСЂРёРїС‚Р° СЃР°РјРѕРіРѕ
        // СЂРµР±С‘РЅРєР°. РљР°Р¶РґС‹Р№ С„СЂРµР№Рј РєРѕРїРёС‚ Р·Р°РїСЂРѕСЃС‹ РІ РЎР’РћР‘Рњ СЂР°РЅС‚Р°Р№РјРµ, Рё РґРѕ СЃСЂРµР·Р° 17
        // РёС… РЅРµ РґСЂРµРЅРёСЂРѕРІР°Р» РЅРёРєС‚Рѕ вЂ” СЃС‚СЂР°РЅРёС‡РЅС‹Р№ РґСЂРµРЅР°Р¶ РЅРёР¶Рµ Р·РЅР°РµС‚ С‚РѕР»СЊРєРѕ
        // `self.js_ctx`. РџСЂРёРјРµРЅСЏСЋС‚СЃСЏ РїРѕСЃР»Рµ С†РёРєР»Р°: С‚Р°Рј РЅСѓР¶РµРЅ `&mut self`.
        let mut frame_scrolls: Vec<(usize, f32)> = Vec::new();
        for (idx, fjs) in &frame_js_handles {
            fjs.tick_timers();
            fjs.pump_websockets();
            fjs.pump_sse();
            fjs.pump_workers();
            fjs.pump_broadcast_channels();
            fjs.pump_shared_workers();
            // BUG-480 СЃСЂРµР· 4: РґРѕСЃС‚Р°РІРєР° РєСЂРѕСЃСЃ-С„СЂРµР№РјРѕРІС‹С… postMessage РІРѕ С„СЂРµР№Рј.
            fjs.pump_frame_messages();
            // РќР°РІРёРіР°С†РёСЏ РёР· С„СЂРµР№РјР° РѕС‚РєР»РѕРЅСЏРµС‚СЃСЏ (СЃСЂРµР· 1) вЂ” РґСЂРµРЅРёРј, С‡С‚РѕР±С‹ Р·Р°РїСЂРѕСЃ
            // РЅРµ РєРѕРїРёР»СЃСЏ Рё РЅРµ СЃСЂР°Р±РѕС‚Р°Р» РїРѕР·Р¶Рµ РёР· РґСЂСѓРіРѕРіРѕ РјРµСЃС‚Р°.
            let _ = fjs.take_navigate_request();
            // Р“Р»Р°РґРєРёР№ (`behavior: 'smooth'`) РїСЂРёРјРµРЅСЏРµС‚СЃСЏ РјРіРЅРѕРІРµРЅРЅРѕ: СЃРІРѕРµР№
            // Р°РЅРёРјР°С†РёРё Сѓ РїСЂРѕРєСЂСѓС‚РєРё С„СЂРµР№РјР° РЅРµС‚, Р° С‚РёРєР°С‚СЊ РµС‘ Р±С‹Р»Рѕ Р±С‹ РЅРµРіРґРµ вЂ”
            // rAF РїРѕРґ-РґРѕРєСѓРјРµРЅС‚РѕРІ РЅРµ РёРґС‘С‚ (СЃСЂРµР· 1). РћС‚РєР»РѕРЅРµРЅРёРµ Р·Р°РїРёСЃР°РЅРѕ РІ
            // bugs/BUG-480-OPEN.md.
            for (target_y, _smooth) in fjs.take_page_scroll_requests() {
                frame_scrolls.push((*idx, target_y));
            }
        }
        for (idx, target_y) in frame_scrolls {
            self.apply_frame_scroll(idx, target_y);
        }
        // ADR-016 M2.2c-2d (20): gate on `self.js_present` instead of borrowing the
        // `Arc` directly (`if let Some(js) = &self.js_ctx`), so the block stays live
        // when the handle later moves engine-side under the flag. `js_present` is kept
        // in lockstep with `self.js_ctx` by `set_js_ctx`, so this is byte-identical to
        // the old gate in both modes; the routed calls below already ignored the passed
        // clone under the flag.
        //
        // ADR-016 M2.3: under the flag, skip the whole pump while a rAF turn is
        // still running on the engine thread вЂ” its `route_query_js` reads would
        // otherwise block behind the in-flight `run_animation_frame` task and
        // freeze the parked loop. The timer/nav work simply runs one pass later
        // (the engine thread + `lumen-js` thread are busy anyway). Off the flag
        // `raf_turn_inflight()` is always `false`, so the gate is byte-identical.
        if self.js_present && !self.raf_turn_inflight() {
            // BUG-839: subresource loads the network layer recorded since the
            // last step. Drained only when there is a runtime to hand them to вЂ”
            // most of a page's images finish before its JS context exists, and
            // taking the rows early would throw them away. What bounds the
            // queue instead is `resource_timing::clear()` on navigation, so a
            // page that never gets a runtime cannot leak its loads into the
            // next document's buffer.
            if let Some(json) =
                resource_timing::rows_to_json(&resource_timing::take_rows())
            {
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.deliver_resource_timings(&json);
                });
            }
            // ADR-016 M2.2c-2d: per-tick pump-Р±Р°С‚С‡ (fire-and-forget void) С‡РµСЂРµР·
            // `route_task_js`. РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґРёС‚ off-UI-thread
            // РѕРґРЅРёРј `task` (РїРѕСЂСЏРґРѕРє РІС‹Р·РѕРІРѕРІ РІРЅСѓС‚СЂРё СЃРѕС…СЂР°РЅС‘РЅ), Р° РїРѕСЃР»РµРґСѓСЋС‰РёРµ
            // `route_query_js`-С‡С‚РµРЅРёСЏ nav/timer РІСЃС‚Р°СЋС‚ РІ РѕС‡РµСЂРµРґСЊ **РїРѕСЃР»Рµ** РЅРµРіРѕ вЂ”
            // read-after-write РїРѕСЂСЏРґРѕРє РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅ, РєР°Рє РґР»СЏ routed `eval_js` РІ 2b/2c.
            // Р‘РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РїСЂСЏРјС‹Рµ РІС‹Р·РѕРІС‹, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                j.tick_timers();
                j.pump_websockets();
                j.pump_sse();
                j.pump_workers();
                j.pump_broadcast_channels();
                j.pump_shared_workers();
                // BUG-480 СЃСЂРµР· 4: РґРѕСЃС‚Р°РІРєР° РєСЂРѕСЃСЃ-С„СЂРµР№РјРѕРІС‹С… postMessage РІ СЃС‚СЂР°РЅРёС†Сѓ.
                j.pump_frame_messages();
            });
            // ADR-016 M2.2c-2c (РѕСЃС‚Р°С‚РѕРє): value-returning nav/timer С‡С‚РµРЅРёСЏ С‡РµСЂРµР·
            // `route_query_js` (С‚РѕС‚ Р¶Рµ РїР°С‚С‚РµСЂРЅ, С‡С‚Рѕ `take_dom_dirty`/`take_raf_pending`
            // РІС‹С€Рµ). РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) С‡РёС‚Р°СЋС‚СЃСЏ Р±Р»РѕРєРёСЂСѓСЋС‰РёРј `query`
            // вЂ” РІ РѕС‡РµСЂРµРґРё РїРѕСЃР»Рµ СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… `task`, РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°СЏ read-after-eval
            // РїРѕСЂСЏРґРѕРє, РѕСЃС‚Р°РІР»РµРЅРЅС‹Р№ СЃРёРЅС…СЂРѕРЅРЅС‹Рј РІ 2b; Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” `js.map`,
            // Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ РїСЂСЏРјРѕРјСѓ `js.<read>()`. `flatten` СЃС…Р»РѕРїС‹РІР°РµС‚
            // `Option<Option<_>>` (РІРЅРµС€РЅРёР№ = В«РµСЃС‚СЊ Р»Рё JS-РєРѕРЅС‚РµРєСЃС‚В», РІРЅСѓС‚СЂРµРЅРЅРёР№ = СЃР°Рј
            // СЂРµР·СѓР»СЊС‚Р°С‚ С‡С‚РµРЅРёСЏ) РІ `Option<_>`.
            if let Some(nav) =
                route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| j.take_navigate_request())
                    .flatten()
            {
                self.pending_js_navigate = Some(nav);
            }
            if let Some(wakeup_epoch_ms) =
                route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| j.take_timer_wakeup())
                    .flatten()
            {
                let now_epoch_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0);
                let delay_ms = (wakeup_epoch_ms - now_epoch_ms).max(0.0);
                let wakeup = std::time::Instant::now()
                    + std::time::Duration::from_millis(delay_ms as u64 + 1);
                next_wakeup = Some(wakeup);
            }
        }
        // BUG-480 СЃСЂРµР· 1: С‚Р°Р№РјРµСЂС‹ С„СЂРµР№РјРѕРІ СѓС‡Р°СЃС‚РІСѓСЋС‚ РІ WaitUntil РЅР°СЂР°РІРЅРµ СЃ
        // С‚Р°Р№РјРµСЂР°РјРё СЃС‚СЂР°РЅРёС†С‹, РёРЅР°С‡Рµ setTimeout СЂРµР±С‘РЅРєР° СЃСЂР°Р±Р°С‚С‹РІР°РµС‚ СЃ Р·Р°РґРµСЂР¶РєРѕР№
        // РґРѕ СЃР»РµРґСѓСЋС‰РµРіРѕ РїСЂРѕР±СѓР¶РґРµРЅРёСЏ РїРѕ С‡СѓР¶РѕРјСѓ РёСЃС‚РѕС‡РЅРёРєСѓ.
        for (_, fjs) in &frame_js_handles {
            if let Some(wakeup_epoch_ms) = fjs.take_timer_wakeup() {
                let now_epoch_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0);
                let delay_ms = (wakeup_epoch_ms - now_epoch_ms).max(0.0);
                let wakeup = std::time::Instant::now()
                    + std::time::Duration::from_millis(delay_ms as u64 + 1);
                next_wakeup = Some(match next_wakeup {
                    Some(cur) => cur.min(wakeup),
                    None => wakeup,
                });
            }
        }
        // BUG-480 СЃСЂРµР· 8: РєРѕРЅРІРµСЂС‚С‹ РјРѕСЃС‚Р° (РєСЂРѕСЃСЃ-С„СЂРµР№РјРѕРІС‹Рµ postMessage,
        // С„Р°СЃР°РґРЅС‹Рµ СЃРѕР±С‹С‚РёСЏ/RunScript) РЅРµ СѓС‡Р°СЃС‚РІСѓСЋС‚ РІ WaitUntil СЃР°РјРё РїРѕ СЃРµР±Рµ вЂ”
        // РїРѕСЃС‚Р°РІР»РµРЅРЅС‹Р№ РїРѕСЃР»Рµ Р·Р°С‚РёС…Р°РЅРёСЏ СЃС‚СЂР°РЅРёС†С‹ РєРѕРЅРІРµСЂС‚ Р¶РґР°Р» Р±С‹ СЃР»СѓС‡Р°Р№РЅРѕРіРѕ
        // РїСЂРѕР±СѓР¶РґРµРЅРёСЏ С†РёРєР»Р° (СЃРјРѕРєРё СЃСЂРµР·РѕРІ 6вЂ“8 СЌС‚Рѕ Р»РѕРІРёР»Рё СЃС‚Р°Р±РёР»СЊРЅРѕ). РџРѕРєР°
        // С…РѕС‚СЊ РѕРґРёРЅ Р¶РёРІРѕР№ РєРѕРЅС‚РµРєСЃС‚ РёРјРµРµС‚ РєРѕРЅРІРµСЂС‚ В«РґР»СЏ СЃРµР±СЏВ», РґРµСЂР¶РёРј РєРѕСЂРѕС‚РєРёР№
        // poll-РґРµРґР»Р°Р№РЅ: РїРѕР»СѓС‡Р°С‚РµР»СЊ СЂР°Р·Р±РµСЂС‘С‚ СЏС‰РёРє РЅР° Р±Р»РёР¶Р°Р№С€РµРј С‚РёРєРµ РїСѓРјРїС‹.
        // Р§РёСЃС‚С‹Рµ С‡С‚РµРЅРёСЏ, Р±РµР· РїРѕР±РѕС‡РЅС‹С… СЌС„С„РµРєС‚РѕРІ; РїРѕРґ РґРІРёР¶РєРѕРІС‹Рј РїРѕС‚РѕРєРѕРј СЃС‚СЂР°РЅРёС†Р°
        // РѕРїСЂР°С€РёРІР°РµС‚СЃСЏ С‚РµРј Р¶Рµ route_query_js-РєР°РЅР°Р»РѕРј, С‡С‚Рѕ РѕСЃС‚Р°Р»СЊРЅС‹Рµ С‡С‚РµРЅРёСЏ.
        let transport_pending = frame_js_handles.iter().any(|(_, fjs)| fjs.frame_transport_pending())
            || match self.engine_thread.as_ref() {
                Some(et) => {
                    route_query_js(
                        Some(et),
                        self.js_ctx.as_ref(),
                        |j| j.frame_transport_pending(),
                    )
                    .unwrap_or(false)
                }
                None => self
                    .js_ctx
                    .as_ref()
                    .is_some_and(|j| j.frame_transport_pending()),
            };
        if transport_pending {
            let poll = std::time::Instant::now() + std::time::Duration::from_millis(2);
            next_wakeup = Some(next_wakeup.map_or(poll, |t| t.min(poll)));
        }
        // BUG-271: rAF pump вЂ” fire pending requestAnimationFrame batches from
        // the parked event loop WITHOUT forcing a repaint. A page that keeps an
        // rAF loop alive without touching DOM/canvas used to drive an
        // unconditional `request_redraw` chain in `RedrawRequested`: full
        // display-list rebuild + GPU paint at 60 fps (~1 busy core on a static
        // page, see bugs/BUG-271-OPEN.md). Spec-wise a callback runs before the
        // *next* repaint вЂ” but when nothing was invalidated there is no repaint
        // to sync with, so batches fire here on a WaitUntil timer instead,
        // sharing the vsync gate (`last_raf_batch_ms`, RAF_MIN_INTERVAL_MS)
        // with `RedrawRequested` step 3.1 so the combined rate stays в‰¤60 Hz.
        // If a callback mutates the DOM we relayout and request a real paint,
        // so rAF-driven animations keep their 60 fps repaint cadence.
        // ADR-016 M2.2c-2d: СЃРЅРёРјР°РµРј РїРѕСЃР»РµРґРЅРёРµ РїСЂСЏРјС‹Рµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёСЏ rAF-РїР°РјРїР°
        // (`has_raf_pending` read в†’ `route_query_js`, `run_animation_frame` void в†’
        // `route_task_js`). РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) С‡С‚РµРЅРёСЏ вЂ” Р±Р»РѕРєРёСЂСѓСЋС‰РёР№
        // `query`, Р±Р°С‚С‡ rAF вЂ” `task` РІ РѕС‡РµСЂРµРґСЊ **РјРµР¶РґСѓ** РЅРёРјРё, С‚Р°Рє С‡С‚Рѕ РїРѕСЂСЏРґРѕРє
        // has_raf_pending в†’ take_raf_pending в†’ run_animation_frame в†’ take_dom_dirty
        // СЃРѕС…СЂР°РЅС‘РЅ; Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ
        // (РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІСѓСЋС‰РµРј С…СЌРЅРґР»Рµ `route_query_js` в†’ `None` в†’ `unwrap_or(false)`,
        // РєР°Рє РїСЂРµР¶РЅСЏСЏ РІРµС‚РєР° `js_ctx == None`).
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        if self.engine_thread.is_some() {
            // ADR-016 M2.3 (flag on): async parked-loop rAF pump вЂ” the
            // counterpart of `RedrawRequested` Step 3.1/4. `pump_raf_engine_thread`
            // fires the batch off-thread (at most one turn in flight) and submits
            // an async relayout on a completed DOM-dirty turn, all lock-free вЂ”
            // never a blocking engine `query` that would stall the parked loop
            // behind the JS turn.
            let raf_due = now_ms - self.last_raf_batch_ms >= RAF_MIN_INTERVAL_MS;
            if self.pump_raf_engine_thread(raf_due, now_ms) {
                self.request_redraw();
            }
            // Keep the loop warm while rAF work remains: a batch is queued, or a
            // turn is still running whose dom-dirty must be re-checked when it
            // finishes. Peek lock-free (does not consume the pending flag).
            if self.raf_pending_lockfree() || self.raf_turn_inflight() {
                let due_in_ms = (self.last_raf_batch_ms + RAF_MIN_INTERVAL_MS - now_ms).max(0.0);
                let raf_wakeup = std::time::Instant::now()
                    + std::time::Duration::from_millis(due_in_ms as u64 + 1);
                next_wakeup = Some(next_wakeup.map_or(raf_wakeup, |t| t.min(raf_wakeup)));
            }
        } else {
            // Flag off (default): original synchronous pump, byte-identical.
            let raf_due = self
                .js_ctx
                .as_ref()
                .is_some_and(|j| j.has_raf_pending());
            let raf_dom_dirty = if raf_due && now_ms - self.last_raf_batch_ms >= RAF_MIN_INTERVAL_MS {
                if let Some(j) = self.js_ctx.as_ref() {
                    j.take_raf_pending();
                }
                self.last_raf_batch_ms = now_ms;
                let raf_ts = if self.deterministic.enabled { 0.0 } else { -1.0 };
                if let Some(j) = self.js_ctx.as_ref() {
                    j.run_animation_frame(raf_ts);
                }
                self.js_ctx.as_ref().is_some_and(|j| j.take_dom_dirty())
            } else {
                false
            };
            if raf_dom_dirty {
                // rAF callback changed the DOM вЂ” rebuild layout and paint for real.
                self.relayout_raf_dirty();
                self.request_redraw();
            }
            if self.js_ctx.as_ref().is_some_and(|j| j.has_raf_pending()) {
                let due_in_ms = (self.last_raf_batch_ms + RAF_MIN_INTERVAL_MS - now_ms).max(0.0);
                let raf_wakeup = std::time::Instant::now()
                    + std::time::Duration::from_millis(due_in_ms as u64 + 1);
                next_wakeup = Some(next_wakeup.map_or(raf_wakeup, |t| t.min(raf_wakeup)));
            }
        }
        // ADR-016 M2.2: apply any off-thread layout result the engine thread has
        // committed since the last iteration (no-op when the engine thread is off).
        self.poll_engine_commit();
        // ADR-016 M0.3 + M2.2: run the debounced transform-first-zoom relayout when
        // its deadline elapses; otherwise fold the deadline into the wakeup so the
        // parked loop wakes exactly then. When the engine thread is enabled, route
        // the (inherently async, no synchronous geometry consumer) zoom relayout
        // off the UI thread; otherwise fall back to the synchronous path. Either
        // path clears the pending state and (on apply) resets the preview to 1:1.
        if let Some(deadline) = self.pending_zoom_relayout {
            if std::time::Instant::now() >= deadline {
                // Consume the debounce regardless of path so it fires once per
                // burst; the async path leaves the preview scale until the commit
                // lands, the sync path resets it immediately inside `relayout`.
                self.pending_zoom_relayout = None;
                if !self.submit_relayout_job() {
                    self.relayout();
                }
            } else {
                next_wakeup = Some(next_wakeup.map_or(deadline, |t| t.min(deadline)));
            }
        }
        // ADR-016 M2.2: while an off-thread job is in flight (submitted but not yet
        // applied), the parked winit loop would not wake on its own to pick up the
        // commit вЂ” arm a short poll deadline. Bounded by the always-landing newest
        // job (coalescing) so this clears promptly. A future slice can replace this
        // with an `EventLoopProxy` wake on commit.
        if self.engine_thread.is_some()
            && self.engine_job_generation != self.engine_applied_generation
        {
            let poll = std::time::Instant::now() + std::time::Duration::from_millis(4);
            next_wakeup = Some(next_wakeup.map_or(poll, |t| t.min(poll)));
        }
        match next_wakeup {
            Some(wakeup) => event_loop.set_control_flow(ControlFlow::WaitUntil(wakeup)),
            // BUG-271: no pending deadline вЂ” park the loop for real. Without
            // this reset a stale `WaitUntil` whose instant is already in the
            // past keeps waking the loop immediately (Poll-like spin): after
            // the last JS timer fired, `take_timer_wakeup()` returned `None`,
            // the old deadline stayed installed and the loop spun at ~20k
            // iterations/s, each doing several blocking JS round-trips
            // (~1.6 busy cores on lenta.ru with zero frames painted).
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }

        // в”Ђв”Ђ Canvas 2D: upload dirty <canvas> bitmaps to the renderer в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // JS Canvas 2D draws into per-node CPU buffers (lumen_canvas::Context2D).
        // Each frame we drain the dirty buffers and register them under the same
        // `canvas:{nid}` key the display list emits, then request a repaint.
        // ADR-016 M2.2c-2d: canvas drain (value-returning) С‡РµСЂРµР· `route_query_js`
        // (С‚РѕС‚ Р¶Рµ РїР°С‚С‚РµСЂРЅ, С‡С‚Рѕ nav/timer/nav-update РІС‹С€Рµ). РџРѕРґ С„Р»Р°РіРѕРј
        // (`LUMEN_ENGINE_THREAD=1`) вЂ” Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query`, РІСЃС‚Р°СЋС‰РёР№ РІ РѕС‡РµСЂРµРґСЊ
        // **РїРѕСЃР»Рµ** СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅРѕРіРѕ pump-`task` (read-after-write СЃРѕС…СЂР°РЅС‘РЅ);
        // Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” `js.map(read)`, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ
        // `js_ctx.map(flush_canvas_updates)`. `unwrap_or_default` РЅР° `None` (РЅРµС‚
        // С…СЌРЅРґР»Р° / СЃРѕСЃС‚РѕСЏРЅРёРµ РЅРµ Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅРѕ / РїРѕС‚РѕРє Р·Р°РІРµСЂС€С‘РЅ) РґР°С‘С‚ РїСѓСЃС‚РѕР№ РґСЂРµРЅР°Р¶.
        // ADR-016 M2.3: `drain_query_js` defers this blocking read while a rAF
        // turn is inflight (would freeze the parked loop behind it); off the flag
        // it is byte-identical to the former direct `route_query_js`.
        let canvas_updates = self.drain_query_js(|j| j.flush_canvas_updates()).unwrap_or_default();
        if !canvas_updates.is_empty() {
            if let Some(r) = self.renderer.as_mut() {
                // BUG-428: РєР»СЋС‡ `canvas:{nid}` СЃС‚СЂРѕРёС‚ РѕР±С‰РёР№ СЃ headless-РїСѓС‚С‘Рј
                // `canvas_updates_as_images` вЂ” РѕРґРёРЅ РёСЃС‚РѕС‡РЅРёРє РёСЃС‚РёРЅС‹ С„РѕСЂРјР°С‚Р°.
                for (key, image) in canvas_updates_as_images(canvas_updates) {
                    if let Err(e) = r.register_image(key.clone(), image) {
                        eprintln!("Canvas: РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅ {key}: {e}");
                    }
                }
            }
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }

        // в”Ђв”Ђ History API: pushState/replaceState URL updates в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Drain URL-update notifications from history.pushState/replaceState.
        // pushState adds a same-document back-stack entry; replaceState updates
        // the displayed URL only.  Neither triggers a page load.
        // ADR-016 M2.2c-2d: history pushState/replaceState drain С‡РµСЂРµР·
        // `route_query_js` вЂ” СЃРѕР±РёСЂР°РµРј `updates` РґРѕ `&mut self`-РјСѓС‚Р°С†РёР№ СЃС‚РµРєР°
        // РЅР°РІРёРіР°С†РёРё. РџРѕРґ С„Р»Р°РіРѕРј вЂ” `query` РїРѕСЃР»Рµ pump-`task`; Р±РµР· С„Р»Р°РіР° вЂ”
        // Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ `js.take_history_url_updates()`; `None` в†’
        // `unwrap_or_default` = РїСѓСЃС‚РѕР№ РґСЂРµРЅР°Р¶ (РєР°Рє РІРµС‚РєР° `js_ctx == None`).
        #[cfg(feature = "v8")]
        {
            let updates = self.drain_query_js(|j| j.take_history_url_updates()).unwrap_or_default();
            for (is_push, url, new_state_json) in updates {
                if is_push {
                    // pushState: save current state to nav_back as same-doc entry.
                    // BUG-829: before the first pushState `display_url` is None вЂ”
                    // the document URL lives in `source` вЂ” so the entry used to be
                    // stored with no URL at all and `fire_popstate` handed JS an
                    // empty string, which `_lumen_deliver_popstate` reads as "keep
                    // the current URL". Traversing back therefore restored the
                    // state and left `location` on the pushed URL. Fall back to the
                    // document's own URL, the same way `current_display_url` does.
                    let old_display = self
                        .display_url
                        .take()
                        .or_else(|| self.source.url_str().map(str::to_owned));
                    let old_state = std::mem::replace(
                         &mut self.current_history_state_json,
                        new_state_json,
                    );
                     self.nav_back.push(NavEntry {
                         source: self.source.clone(),
                         scroll_x: self.scroll_x,
                         scroll_y: self.scroll_y,
                         display_url: old_display,
                         same_doc_state_json: Some(old_state),
                         nav_key: self.current_nav_key.clone(),
                     });
                    self.nav_key_counter += 1;
                    self.current_nav_key = format!("nav-{}", self.nav_key_counter);
                    self.display_url = Some(url);
                } else {
                    // replaceState: update URL + state, no nav_back push.
                    self.current_history_state_json = new_state_json;
                    self.display_url = Some(url);
                }
            }
        }

        // в”Ђв”Ђ History API: history.go(n) / back / forward traversal в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Drain JS-initiated traversal deltas and apply them to the real
        // nav_back/nav_fwd stacks (single authority). Collect first so the
        // immutable `js_ctx` borrow is released before the `&mut self` calls.
        // ADR-016 M2.2c-2d: history.go/back/forward traversal drain С‡РµСЂРµР·
        // `route_query_js` вЂ” С‚Рµ Р¶Рµ РіР°СЂР°РЅС‚РёРё (query РїРѕСЃР»Рµ pump-`task` РїРѕРґ С„Р»Р°РіРѕРј;
        // Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ `js_ctx.map(take_history_traversals)` Р±РµР· РЅРµРіРѕ), `None` в†’
        // РїСѓСЃС‚РѕР№ РґСЂРµРЅР°Р¶. РЎРѕР±РёСЂР°РµРј РґРѕ `&mut self`-РјСѓС‚Р°С†РёР№ (`navigate_by`).
        #[cfg(feature = "v8")]
        {
            let traversals = self.drain_query_js(|j| j.take_history_traversals()).unwrap_or_default();
            for delta in traversals {
                self.navigate_by(delta);
            }
        }

        // в”Ђв”Ђ Navigation API: run pending intercept handler в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        if let Some(pending) = &mut self.pending_intercepted {
            let started = match pending {
                PendingIntercepted::Push { handler_started, .. }
                | PendingIntercepted::Replace { handler_started, .. }
                | PendingIntercepted::Back { handler_started, .. }
                | PendingIntercepted::Forward { handler_started, .. } => *handler_started,
            };
            if !started {
                // ADR-016 M2.2c-2b: РёР·РѕР»РёСЂРѕРІР°РЅРЅС‹Р№ fire-and-forget void-РІС‹Р·РѕРІ
                // (СЃР»РµРґРѕРј вЂ” С‚РѕР»СЊРєРѕ РјСѓС‚Р°С†РёСЏ `pending`, Р±РµР· СЃРёРЅС…СЂРѕРЅРЅРѕРіРѕ С‡С‚РµРЅРёСЏ JS),
                // РїРѕСЌС‚РѕРјСѓ РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓРµРј РµРіРѕ off-UI-thread РїСЂРё РІРєР»СЋС‡С‘РЅРЅРѕРј РґРІРёР¶РєРѕРІРѕРј
                // РїРѕС‚РѕРєРµ; РїСЂРё РІС‹РєР»СЋС‡РµРЅРЅРѕРј (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅС‹Р№
                // СЃРёРЅС…СЂРѕРЅРЅС‹Р№ `js.eval_js`. Disjoint-borrow РїРѕР»РµР№ `engine_thread`/
                // `js_ctx` СѓР¶РёРІР°РµС‚СЃСЏ СЃ Р°РєС‚РёРІРЅС‹Рј `&mut self.pending_intercepted`.
                route_eval_js(
                    self.engine_thread.as_ref(),
                    self.js_ctx.as_ref(),
                    "_lumen_run_navigate_handler()".to_owned(),
                );
                *pending = match pending {
                    PendingIntercepted::Push { url, .. } => PendingIntercepted::Push {
                        url: url.clone(),
                        handler_started: true,
                    },
                    PendingIntercepted::Replace { url, .. } => PendingIntercepted::Replace {
                        url: url.clone(),
                        handler_started: true,
                    },
                    PendingIntercepted::Back { .. } => PendingIntercepted::Back {
                        handler_started: true,
                    },
                    PendingIntercepted::Forward { .. } => PendingIntercepted::Forward {
                        handler_started: true,
                    },
                };
            }
        }

        // в”Ђв”Ђ Navigation API: drain queued navigation requests в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Shell is the single authority for `navigation.navigate()` / `back()` /
        // `forward()` / `traverseTo()`. Each entry is `(action_code, url, key, data)`.
        #[cfg(feature = "v8")]
        {
            // ADR-016 M2.2c-2c (РѕСЃС‚Р°С‚РѕРє): nav-update drain С‡РµСЂРµР· `route_query_js`
            // (С‚РѕС‚ Р¶Рµ РїР°С‚С‚РµСЂРЅ, С‡С‚Рѕ nav/timer РІ `about_to_wait`). РџРѕРґ С„Р»Р°РіРѕРј вЂ”
            // Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query` РїРѕСЃР»Рµ СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… `task`; Р±РµР· С„Р»Р°РіР° вЂ”
            // Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ `js_ctx.map(take_nav_updates)`. `None`
            // (РЅРµС‚ UI-С…СЌРЅРґР»Р° / СЃРѕСЃС‚РѕСЏРЅРёРµ РЅРµ Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅРѕ / РїРѕС‚РѕРє Р·Р°РІРµСЂС€С‘РЅ) в†’
            // `unwrap_or_default` РґР°С‘С‚ РїСѓСЃС‚РѕР№ РґСЂРµРЅР°Р¶, РєР°Рє Рё РїСЂРµР¶РЅСЏСЏ РІРµС‚РєР° `None`.
            let navs = self.drain_query_js(|j| j.take_nav_updates()).unwrap_or_default();
            for (action_code, url, key, data) in navs {
                match action_code {
                    0 if !url.is_empty() => self.navigate_to(PageSource::Url(url)),
                    1 if !url.is_empty() => self.navigate_replace(PageSource::Url(url)),
                    2 => self.navigate_back(),
                    3 => self.navigate_forward(),
                    4 => self.navigate_to_key(&key),
                    5 => self.reload(),
                    6 => {
                        let parsed: serde_json::Value =
                            serde_json::from_str(&data).unwrap_or_default();
                        let new_url = parsed
                            .get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&url)
                            .to_string();
                        let state = parsed
                            .get("state")
                            .and_then(|v| v.as_str())
                            .unwrap_or("null")
                            .to_string();
                        let title = parsed
                            .get("title")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        if let Some(pending) = self.pending_intercepted.take() {
                            match pending {
                                PendingIntercepted::Push { .. }
                                | PendingIntercepted::Replace { .. }
                                | PendingIntercepted::Back { .. }
                                | PendingIntercepted::Forward { .. } => {
                                    self.display_url = Some(new_url.clone());
                                    self.current_history_state_json = state.clone();
                                    if let Some(t) = title {
                                        self.title = Some(t.clone());
                                        if let Some(w) = self.window.as_ref() {
                                            w.set_title(&window_title(Some(&t)));
                                        }
                                    }
                                    if matches!(pending, PendingIntercepted::Push { .. }) {
                                        self.nav_back.push(NavEntry {
                                            source: PageSource::Url(new_url.clone()),
                                            scroll_x: 0.0,
                                            scroll_y: 0.0,
                                            display_url: None,
                                            same_doc_state_json: Some(state.clone()),
                                            nav_key: self.current_nav_key.clone(),
                                        });
                                        self.nav_key_counter += 1;
                                        self.current_nav_key = format!(
                                            "nav-{}",
                                            self.nav_key_counter
                                        );
                                    }
                                    self.commit_nav_state();
                                    self.fire_navigate_success();
                                    self.fire_current_entry_change();
                                }
                            }
                        }
                    }
                    7 => {
                        self.pending_intercepted.take();
                        self.fire_navigate_error();
                    }
                    _ => {}
                }
            }
        }

        // в”Ђв”Ђ Automation commands (SDC-1b/SDC-2) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Drain external commands (BiDi/MCP/graphic_tests) and route to shell actions.
        // Each request already carries the reply sender for its specific caller
        // (SDC-2) вЂ” no more fan-out through one shared, unread channel.
        let automation_cmds: Vec<AutomationRequest> = self.automation_rx.try_iter().collect();
        for (cmd, reply_tx) in automation_cmds {
            match cmd {
                AutomationCommand::Navigate(url) => {
                    // Real browsers clear the console on navigation (unless "preserve
                    // log" is set); doing the same here keeps `ConsoleLog` (DEVX-1)
                    // scoped to the page just loaded instead of accumulating across
                    // an entire --live run.
                    self.devtools_console.clear();
                    self.navigate_to(page_source_for_automation_url(&url));
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::NewTab(url) => {
                    // Same console semantics as `Navigate`: the fresh tab becomes
                    // active, so `ConsoleLog` stays scoped to the page being loaded.
                    self.devtools_console.clear();
                    self.open_new_tab();
                    self.navigate_to(page_source_for_automation_url(&url));
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::Click(target) => {
                    let point = self.resolve_automation_target(&target);
                    if let Some((x, y)) = point {
                        self.handle_click_at(x, y);
                        let _ = reply_tx.send(AutomationReply::Ack);
                    } else {
                        let _ = reply_tx.send(AutomationReply::Error("Element not found".to_string()));
                    }
                }
                AutomationCommand::Type(target, text) => {
                    // Same target resolution as `Click` вЂ” and the same honest
                    // failure when it resolves to nothing. Reporting `Ack` for
                    // an unresolvable target was half of BUG-436's "succeeds
                    // but does nothing" signature.
                    match self.resolve_automation_target(&target) {
                        Some((x, y)) => {
                            self.handle_click_at(x, y);
                            let mut consumed = true;
                            for ch in text.chars() {
                                consumed &= self.inject_char(ch);
                            }
                            if consumed {
                                let _ = reply_tx.send(AutomationReply::Ack);
                            } else {
                                let _ = reply_tx.send(AutomationReply::Error(
                                    "Element is not a mutable text field".to_string(),
                                ));
                            }
                        }
                        None => {
                            let _ = reply_tx
                                .send(AutomationReply::Error("Element not found".to_string()));
                        }
                    }
                }
                AutomationCommand::Scroll(delta) => {
                    self.scroll_by_delta(delta.x, delta.y);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::Eval(js) => {
                    // ADR-016 M2.2c-2c: value-returning `eval_js_value` С‡РµСЂРµР·
                    // `route_query_js`. РџРѕРґ С„Р»Р°РіРѕРј С‡С‚РµРЅРёРµ СѓРїРѕСЂСЏРґРѕС‡РµРЅРѕ Р·Р° СѓР¶Рµ
                    // РѕС‚РїСЂР°РІР»РµРЅРЅС‹РјРё `task`; Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ: `Some(js_ctx)`
                    // в†’ `Some(result)`, РѕС‚СЃСѓС‚СЃС‚РІРёРµ С…СЌРЅРґР»Р° в†’ `None` в†’ В«JS context
                    // not availableВ».
                    match route_query_js(
                        self.engine_thread.as_ref(),
                        self.js_ctx.as_ref(),
                        move |j| j.eval_js_value(&js),
                    ) {
                        Some(Ok(json)) => {
                            let _ = reply_tx.send(AutomationReply::Eval(json));
                        }
                        Some(Err(e)) => {
                            let _ = reply_tx.send(AutomationReply::Error(e));
                        }
                        None => {
                            let _ = reply_tx.send(AutomationReply::Error(
                                "JS context not available".to_string(),
                            ));
                        }
                    }
                }
                AutomationCommand::Screenshot => {
                    match self.render_current_page_to_png() {
                        Ok(png) => {
                            let _ = reply_tx.send(AutomationReply::Screenshot(png));
                        }
                        Err(e) => {
                            let _ = reply_tx.send(AutomationReply::Error(e));
                        }
                    }
                }
                AutomationCommand::Wait(cond, timeout_ms) => {
                    self.pending_waits.push(PendingWait {
                        cond,
                        deadline: std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms),
                        reply_tx,
                    });
                }
                AutomationCommand::Query(selector) => {
                    let nodes = self.query_automation_nodes(&selector);
                    let _ = reply_tx.send(AutomationReply::Query(nodes));
                }
                AutomationCommand::A11yTree => match self.automation_a11y_tree() {
                    Some(tree) => {
                        let _ = reply_tx.send(AutomationReply::A11yTree(Box::new(tree)));
                    }
                    None => {
                        let _ = reply_tx.send(AutomationReply::Error("no page loaded".to_string()));
                    }
                },
                AutomationCommand::ConsoleLog => {
                    // Drain any messages the JS runtime queued this tick before the
                    // periodic DevTools drain below would (same pattern, just eager
                    // so a message logged earlier this frame isn't missed).
                    let msgs = self.drain_query_js(|j| j.take_console_messages()).unwrap_or_default();
                    if !msgs.is_empty() {
                        // PERF-6: record page `console.error(...)` calls as health signals.
                        if health_log::is_enabled() {
                            let url = self.source.describe();
                            for (level, text) in &msgs {
                                if *level == 2 {
                                    health_log::log_console_error(&url, text);
                                }
                            }
                        }
                        self.devtools_console.push_batch(msgs);
                    }
                    let entries: Vec<ConsoleEntry> = self
                        .devtools_console
                        .messages()
                        .iter()
                        .map(|m| ConsoleEntry {
                            level: match m.level {
                                devtools::console_panel::ConsoleLevel::Log => DriverConsoleLevel::Log,
                                devtools::console_panel::ConsoleLevel::Warn => DriverConsoleLevel::Warn,
                                devtools::console_panel::ConsoleLevel::Error => DriverConsoleLevel::Error,
                            },
                            message: m.text.clone(),
                        })
                        .collect();
                    let _ = reply_tx.send(AutomationReply::ConsoleLog(entries));
                }
                AutomationCommand::LayoutSnapshot => {
                    let boxes = self.automation_layout_snapshot();
                    let _ = reply_tx.send(AutomationReply::LayoutSnapshot(boxes));
                }
                AutomationCommand::NetworkLog => {
                    let entries = self.automation_network_log();
                    let _ = reply_tx.send(AutomationReply::NetworkLog(entries));
                }
                AutomationCommand::SetOffline(offline) => {
                    // BUG-295 (`network.setOfflineStatus`): process-global flag
                    // consulted at the one `fetch_with_redirect` chokepoint every
                    // fetch path (navigation, JS `fetch()`/XHR, subresources)
                    // already funnels through вЂ” no per-request wiring needed here.
                    lumen_network::set_global_offline(offline);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::SetUserAgent(ua) => {
                    // BUG-295 (`emulation.setUserAgentOverride`): empty string
                    // clears the override (see `LiveWindowSession::set_user_agent`
                    // / `BidiState::user_agent_for` вЂ” `None` collapses to `""`
                    // before reaching this command).
                    let override_value = if ua.is_empty() { None } else { Some(ua.clone()) };
                    lumen_network::set_global_ua_override(override_value.clone());
                    #[cfg(feature = "v8")]
                    {
                        // Applies to the *next* navigation (install_dom reads the
                        // global at DOM-bootstrap time вЂ” see
                        // `v8_runtime::set_global_user_agent_override`'s doc).
                        lumen_js::v8_runtime::set_global_user_agent_override(override_value);
                        // Also re-inject into the *current* page right now: BiDi
                        // clients (and BUG-295's own repro) set the override then
                        // immediately `script.evaluate` `navigator.userAgent` on
                        // the already-loaded default context, with no navigation
                        // in between. Clearing (`ua.is_empty()`) is intentionally
                        // not re-applied here вЂ” the current page keeps whatever
                        // it last had; only the *next* navigation reverts to the
                        // real WEB_API_SHIM default.
                        if !ua.is_empty() {
                            let script = lumen_js::v8_runtime::user_agent_override_script(&ua);
                            let _ = route_query_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                move |j| j.eval_js_value(&script),
                            );
                        }
                    }
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::SetTimezone(timezone_id) => {
                    // BUG-295 (`browser.setTimezoneOverride`): unlike UA
                    // override there is no HTTP-layer counterpart вЂ” timezone
                    // only affects JS-visible `Intl`/`Date` behavior.
                    #[cfg(feature = "v8")]
                    {
                        // Applies to the *next* navigation (install_dom reads
                        // the global at DOM-bootstrap time вЂ” see
                        // `v8_runtime::set_global_timezone_override`'s doc).
                        lumen_js::v8_runtime::set_global_timezone_override(timezone_id.clone());
                        // Also re-inject into the *current* page right now,
                        // mirroring `SetUserAgent` above: BiDi clients set the
                        // override then immediately `script.evaluate` on the
                        // already-loaded default context, with no navigation
                        // in between. Clearing (`None`) is intentionally not
                        // re-applied here вЂ” only the *next* navigation clears
                        // the marker (matches `SetUserAgent`'s accepted gap).
                        if let Some(tz) = &timezone_id {
                            let script = lumen_js::v8_runtime::timezone_override_script(tz);
                            let _ = route_query_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                move |j| j.eval_js_value(&script),
                            );
                        }
                    }
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::AddIntercept { id, phases, url_patterns } => {
                    // BUG-295 remainder (`network.addIntercept`): synced into
                    // `lumen_network`'s process-global registry, consulted at
                    // the same `fetch_with_redirect` chokepoint as the
                    // offline/UA-override toggles above.
                    lumen_network::add_global_intercept(lumen_network::GlobalIntercept {
                        id,
                        phases,
                        url_patterns,
                    });
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::RemoveIntercept(id) => {
                    lumen_network::remove_global_intercept(&id);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::ResolveIntercept { request_id, continue_request } => {
                    // BUG-295 remainder (`network.continueRequest`/`network.failRequest`):
                    // unblocks the fetch worker thread (if any) paused on
                    // `request_id` in `lumen_network`'s registry.
                    let decision = if continue_request {
                        lumen_network::InterceptDecision::Continue
                    } else {
                        lumen_network::InterceptDecision::Fail
                    };
                    let matched = lumen_network::resolve_intercept(&request_id, decision);
                    let _ = reply_tx.send(AutomationReply::InterceptResolved(matched));
                }
                AutomationCommand::PollIntercepts => {
                    let requests = lumen_network::drain_new_intercept_announcements()
                        .into_iter()
                        .map(|(request_id, url)| InterceptedRequest { request_id, url })
                        .collect();
                    let _ = reply_tx.send(AutomationReply::Intercepts(requests));
                }
            }
        }

        // Re-check queued `Wait` requests once per frame вЂ” never block the
        // event loop on a wait (see `PendingWait` doc comment for why).
        if !self.pending_waits.is_empty() {
            let now = std::time::Instant::now();
            let drained = std::mem::take(&mut self.pending_waits);
            let mut still_pending = Vec::with_capacity(drained.len());
            for pending in drained {
                if self.check_wait_condition(&pending.cond) {
                    // BUG-438: a `DocumentReady`/`NetworkIdle` wait can resolve
                    // `true` because the navigation settled in a network/HTTP
                    // error (BUG-308's `load_failed` early-out), not because a
                    // document actually loaded. Reporting `Ack` there is
                    // exactly the "navigate/wait both say success but the
                    // previous document is still showing" bug вЂ” surface the
                    // real failure instead, the same way a wait timeout does.
                    let settled_error = matches!(
                        pending.cond,
                        WaitCondition::DocumentReady | WaitCondition::NetworkIdle
                    ) && self.load_failed;
                    if settled_error {
                        let _ = pending.reply_tx.send(AutomationReply::Error(format!(
                            "navigation failed: {}",
                            self.load_error_message.as_deref().unwrap_or("load error")
                        )));
                    } else {
                        let _ = pending.reply_tx.send(AutomationReply::Ack);
                    }
                } else if now >= pending.deadline {
                    let _ = pending.reply_tx.send(AutomationReply::Error(format!(
                        "wait timeout: {:?}",
                        pending.cond
                    )));
                } else {
                    still_pending.push(pending);
                }
            }
            self.pending_waits = still_pending;
        }

        // Ph3 pointer-events-l3: flush any `CursorMoved` samples queued this
        // tick as one coalesced `pointermove` вЂ” Pointer Events L3 В§4.1. Runs
        // once per `about_to_wait` iteration (roughly once per frame); a fast
        // mouse can queue several samples between paints, all folded into a
        // single dispatch with the rest exposed via `getCoalescedEvents()`.
        #[cfg(feature = "v8")]
        self.flush_pointer_moves();

        // в”Ђв”Ђ Native input injection (ADR-007 В§8C) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Drain injected commands and route through the same dispatch path as
        // real OS events so events have isTrusted=true.
        let injected: Vec<input::InputCommand> = self.input_rx.drain();
        for cmd in injected {
            match cmd {
                input::InputCommand::Click { x, y } => {
                    self.handle_click_at(x, y);
                }
                input::InputCommand::TypeText { text } => {
                    let chars: Vec<char> = text.chars().collect();
                    for ch in chars {
                        self.inject_char(ch);
                    }
                }
                input::InputCommand::MouseMove { x, y } => {
                    self.dispatch_mouse_move(x, y);
                }
                input::InputCommand::Scroll { x, y } => {
                    self.scroll_x = clamp_scroll(x, self.max_scroll_x());
                    self.scroll_y = clamp_scroll(y, (self.content_height - self.viewport_height_css()).max(0.0));
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
                }
                input::InputCommand::KeyDown { code } => {
                    self.inject_special_key(&code);
                }
            }
        }

        // Pointer Events L3 В§4.1: flush pointer-move samples buffered this
        // tick (real `CursorMoved` + injected `MouseMove`) as one coalesced
        // `pointermove`/`mousemove` dispatch. Safety-net flush point: press/
        // release/enter/leave dispatch sites flush eagerly for ordering, but a
        // plain move with no state change only reaches JS here.
        #[cfg(feature = "v8")]
        self.flush_pointer_moves();

        // Download manager: drain completion events from background threads.
        self.downloads.poll();

        // _lumen_network_download(url, filename): start downloads requested by
        // page scripts / <a download>. Relative URLs are resolved against the
        // active document URL.
        {
            let reqs = lumen_js::download_bindings::take_download_requests();
            if !reqs.is_empty() {
                let base = self.current_display_url().to_owned();
                for req in reqs {
                    let abs = lumen_core::url::Url::parse(&base)
                        .and_then(|b| b.resolve(&req.url))
                        .map(|u| u.to_string())
                        .unwrap_or(req.url);
                    self.downloads.start_url_download(abs, req.filename);
                }
                self.request_redraw();
            }
        }

        // _lumen_log_network_request(method, url, status, duration_ms): fold
        // JS-logged requests into the shared NetworkLog so they appear in the
        // DevTools Network panel / inspector Network tab (CC-9).
        {
            let recs = lumen_js::network_log_bindings::take_network_log_records();
            if !recs.is_empty() {
                for r in recs {
                    self.network_panel
                        .record_js_request(&r.method, &r.url, r.status, r.duration_ms);
                }
                self.request_redraw();
            }
        }

        // В§12.3 Read-later: drain completed background page fetches and persist.
        while let Ok((url, title, html)) = self.read_later_rx.try_recv() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let _ = self.read_later_store.save(&url, &title, &html, "", &[], now);
            self.refresh_read_later();
            self.request_redraw();
        }

        // Web Notifications API: deliver pending OS notifications queued by JS.
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js` (РїРѕРґ С„Р»Р°РіРѕРј вЂ” off-UI-thread
        // `query`; Р±РµР· С„Р»Р°РіР° вЂ” Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ `js.take_notification_requests()`).
        for (title, body) in self.drain_query_js(|j| j.take_notification_requests()).unwrap_or_default()
        {
            notification::show_os_notification(&title, &body);
        }

        // window.open() popup requests: each entry opens a new tab and navigates it
        // to the requested URL.  Executed after the page render so the current tab
        // stays visible while the new tab loads.
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js`.
        {
            let popups = self.drain_query_js(|j| j.take_window_open_requests()).unwrap_or_default();
            for (url, _target, _width, _height) in popups {
                // BUG-293: resolve `file://` popups to a `PageSource::File` (load
                // from disk) rather than the http-only network path. Read the
                // opener's scheme from `self.source` BEFORE `open_new_tab()`
                // resets it, so the webв†’file security check sees the real opener.
                let resolved = if url.is_empty() {
                    Ok(PageSource::Url("about:blank".to_owned()))
                } else {
                    resolve_js_navigation(&url, &self.source)
                };
                self.open_new_tab();
                match resolved {
                    Ok(source) => self.navigate_to(source),
                    Err(reason) => eprintln!("window.open Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅ: {reason}"),
                }
            }
        }

        // Fullscreen API: apply OS fullscreen on requestFullscreen() / exitFullscreen().
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js`.
        #[cfg(feature = "v8")]
        for (enter, nid) in self.drain_query_js(|j| j.take_fullscreen_requests()).unwrap_or_default()
        {
            self.fullscreen_nid = if enter { Some(nid) } else { None };
            let target = if enter {
                Some(winit::window::Fullscreen::Borderless(None))
            } else {
                None
            };
            // Apply the OS mode and capture the pre-toggle physical size; the
            // borrow of `self.window` ends with the `map`, so the &mut call
            // to `arm_fullscreen_resize` below does not conflict.
            let prev = self.window.as_ref().map(|w| {
                w.set_fullscreen(target);
                w.inner_size()
            });
            if let Some(prev) = prev {
                self.arm_fullscreen_resize(prev);
            }
        }

        // BUG-167: once the OS has applied a fullscreen toggle, reconcile the
        // page viewport to the new window size (resize + relayout). No-op unless
        // a toggle is pending.
        self.poll_fullscreen_resize();

        // Pointer Lock API (W3C Pointer Lock L2 В§4): apply pending OS cursor grab.
        // JS calls requestPointerLock() / exitPointerLock() в†’ queues a grab change
        // в†’ shell applies it here via winit.  Locked falls back to Confined on
        // platforms (e.g. Wayland) that don't support true cursor lock.
        #[cfg(feature = "v8")]
        if let (Some(grab), Some(window)) = (
            lumen_js::pointer_lock::take_pending_grab(),
            self.window.as_ref(),
        ) {
            if grab {
                let _ = window.set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                window.set_cursor_visible(false);
            } else {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
                window.set_cursor_visible(true);
            }
        }

        // CC-7 / P3-pip: Video and Document Picture-in-Picture вЂ” open/close the
        // real OS floating window. Drained from the process-global queue fed
        // by `_lumen_pip_enter` / `_lumen_pip_exit` / `_lumen_pip_request_window`
        // (see `lumen_js::pip_bindings`).
        for req in lumen_js::pip_bindings::take_pip_requests() {
            use lumen_js::pip_bindings::PipRequest;
            use panels::pip_os_window::PipAction;
            match req {
                PipRequest::Enter { nid } => match self.pip_controller.on_enter(nid) {
                    PipAction::Open(nid) => self.open_pip_os(event_loop, nid),
                    PipAction::Close => self.close_pip_os(),
                    PipAction::None => {}
                },
                PipRequest::Exit { .. } => match self.pip_controller.on_exit() {
                    PipAction::Open(nid) => self.open_pip_os(event_loop, nid),
                    PipAction::Close => self.close_pip_os(),
                    PipAction::None => {}
                },
                PipRequest::OpenDocument { width, height } => {
                    self.open_pip_os_document(event_loop, width, height);
                }
            }
        }

        // Document Picture-in-Picture (slice 1) вЂ” open/close the real OS floating
        // window. Drained from the process-global queue fed by
        // `_lumen_docpip_request_window` / `_lumen_docpip_close` (see
        // `lumen_js::documentpip_bindings`).
        for req in lumen_js::documentpip_bindings::take_docpip_requests() {
            use lumen_js::documentpip_bindings::DocPipRequest;
            use panels::doc_pip_os_window::DocPipAction;
            let action = match req {
                DocPipRequest::Open { width, height } => self.doc_pip_controller.on_open(width, height),
                DocPipRequest::Close => self.doc_pip_controller.on_close(),
                DocPipRequest::SetContent(html) => {
                    if let Some(pip) = self.doc_pip_os.as_mut() {
                        pip.content_html = html;
                    }
                    self.render_doc_pip_os();
                    DocPipAction::None
                }
            };
            match action {
                DocPipAction::Open { width, height } => self.open_doc_pip_os(event_loop, width, height),
                DocPipAction::Close => self.close_doc_pip_os(),
                DocPipAction::None => {}
            }
        }

        // Print API: window.print() exports current document as PDF (W-2).
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js`.
        #[cfg(feature = "v8")]
        for req in self.drain_query_js(|j| j.take_print_requests()).unwrap_or_default()
        {
            self.handle_print_request(&req);
        }

        // Focus management (HTML LS В§6.6.3): apply focus changes requested by JS via
        // _lumen_request_focus / _lumen_request_blur вЂ” `element.focus()`/`blur()`
        // (BUG-381) as well as the older showModal() / close() pair.
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js`.
        #[cfg(feature = "v8")]
        {
            let focus_reqs = self.drain_query_js(|j| j.take_focus_requests()).unwrap_or_default();
            if !focus_reqs.is_empty() {
                // Only the last request in the batch matters.
                if let Some(last_req) = focus_reqs.into_iter().last() {
                    let new_nid = last_req.map(|n| lumen_dom::NodeId::from_index(n as usize));
                    if new_nid != self.focused_node {
                        self.focused_node = new_nid;
                        // ADR-016 M2.2b-7: `focused_node` is set synchronously above,
                        // so `:focus`/`:focus-within` re-evaluates correctly on any
                        // later relayout (it feeds `set_interactive_state` at the top
                        // of every pass). This is a pure restyle with no synchronous
                        // page-geometry read afterwards (the follow-up only notifies
                        // the accessibility bridge), so route it off-thread.
                        self.relayout_chrome();
                        self.platform_bridge.focused_node_changed(new_nid);
                        // BUG-381: echo the applied focus back into JS. For a request
                        // that came from `element.focus()` this is a no-op (the shim
                        // already recorded it synchronously); for `showModal()`/`close()`,
                        // which move focus without going through `focus()`, it is what
                        // updates `document.activeElement` and fires the focus events.
                        let focus_idx = new_nid.map(|n| n.index() as u32);
                        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                            js.notify_focus_changed(focus_idx);
                        });
                    }
                }
            }
        }

        // CSS View Transitions API: drain snapshot/animation events from JS.
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js`.
        #[cfg(feature = "v8")]
        {
            let events = self.drain_query_js(|j| j.take_view_transition_events()).unwrap_or_default();
            for event in events {
                match event {
                    ViewTransitionEvent::Begin => {
                        // Capture current display list as the "before" snapshot.
                        self.view_transition = Some(ViewTransitionState {
                            old_dl: self.display_list.clone(),
                            start_ms: 0.0,
                            duration_ms: 300.0,
                        });
                    }
                    ViewTransitionEvent::End => {
                        // Callback finished вЂ” relayout picks up DOM mutations,
                        // then the render step blends old_dl (fading out) over
                        // the new display list.
                        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                        if let Some(vt) = &mut self.view_transition {
                            vt.start_ms = now_ms;
                        }
                        self.relayout();
                        if let Some(w) = self.window.as_ref() {
                            w.request_redraw();
                        }
                    }
                    ViewTransitionEvent::Cancel => {
                        // Transition was cancelled вЂ” abort the animation.
                        self.view_transition = None;
                    }
                }
            }
        }

        // DevTools console: drain JS console.log/warn/error messages into the panel.
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js`.
        {
            let msgs = self.drain_query_js(|j| j.take_console_messages()).unwrap_or_default();
            if !msgs.is_empty() {
                // PERF-6: record page `console.error(...)` calls as health signals.
                if health_log::is_enabled() {
                    let url = self.source.describe();
                    for (level, text) in &msgs {
                        if *level == 2 {
                            health_log::log_console_error(&url, text);
                        }
                    }
                }
                self.devtools_console.push_batch(msgs);
                if self.devtools_console.visible {
                    self.request_redraw();
                }
            }
        }

        // JS scroll requests: drain programmatic scrolls queued by scrollTo/scrollBy/
        // scrollIntoView.  Scroll position is applied directly to the existing layout
        // tree (no CSS re-computation needed вЂ” scroll only affects paint offsets), the
        // display list is rebuilt cheaply, and JS scroll-state cache is updated so
        // subsequent scrollTop/scrollLeft reads return the new values.
        // ADR-016 M2.2d: value-drain (`take_scroll_requests`) С‡РµСЂРµР· `route_query_js`,
        // write-back (`update_scroll_states` + `fire_element_scroll`) С‡РµСЂРµР·
        // `route_task_js` вЂ” СЃРЅРёРјР°РµРј РїСЂСЏРјС‹Рµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёСЏ. РџРѕРґ С„Р»Р°РіРѕРј
        // (`LUMEN_ENGINE_THREAD=1`) РґСЂРµРЅР°Р¶ РёРґС‘С‚ Р±Р»РѕРєРёСЂСѓСЋС‰РёРј `query`, Р° РїРѕСЃР»РµРґСѓСЋС‰Р°СЏ
        // write-back-`task` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ **РїРѕСЃР»Рµ** РЅРµРіРѕ (read-after-write РїРѕСЂСЏРґРѕРє
        // СЃРѕС…СЂР°РЅС‘РЅ); Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ `js.<method>()`,
        // Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
        #[cfg(feature = "v8")]
        {
            let scroll_reqs = self.drain_query_js(|j| j.take_scroll_requests()).unwrap_or_default();
            if !scroll_reqs.is_empty()
                && let Some(lb) = self.layout_box.as_mut()
            {
                let mut changed = false;
                let mut scrolled_nids: Vec<u32> = Vec::new();
                for (nid, x, y) in scroll_reqs {
                    if set_scroll_position(lb, NodeId::from_index(nid as usize), x, y) {
                        changed = true;
                        scrolled_nids.push(nid);
                    }
                }
                if changed {
                    // Rebuild display list with the updated scroll offsets.
                    let mut new_dl = paint_ordered(lb);
                    // BUG-480 СЃСЂРµР· 14: paint_ordered РїРµСЂРµСЃРѕР±РёСЂР°РµС‚ СЃРїРёСЃРѕРє РёР·
                    // layout Рё Рѕ С„СЂРµР№РјР°С… РЅРµ Р·РЅР°РµС‚ вЂ” Р±РµР· РІРєР»РµР№РєРё СЃРѕРґРµСЂР¶РёРјРѕРµ
                    // С„СЂРµР№РјР° РёСЃС‡РµР·Р»Рѕ Р±С‹ РЅР° РїРµСЂРІРѕРј Р¶Рµ СЃРєСЂРѕР»Р»Рµ РєРѕРЅС‚РµР№РЅРµСЂР°.
                    crate::frames::splice_frame_content(&mut new_dl, &self.frames);
                    let root_id = lb.node.index() as u32;
                    self.tile_grid.update_from_diff(&self.display_list, &new_dl);
                    // Cache directly вЂ” lb mutably borrows self.layout_box; only self.display_list_cache is touched here.
                    let dl_hash = lumen_paint::hash_commands(&new_dl);
                    self.display_list_cache.insert(root_id, new_dl.clone(), dl_hash, None);
                    // РџСЂСЏРјР°СЏ Р·Р°РїРёСЃСЊ РїРѕР»РµР№: `layout_box` Р·РґРµСЃСЊ Р·Р°РёРјСЃС‚РІРѕРІР°РЅ
                    // РјСѓС‚Р°Р±РµР»СЊРЅРѕ, `&mut self` С†РµР»РёРєРѕРј РІР·СЏС‚СЊ РЅРµР»СЊР·СЏ.
                    self.display_list = new_dl;
                    self.display_list_epoch = next_dl_epoch(self.display_list_epoch);
                    // Sync JS cache so scrollTop/scrollLeft reads are accurate, then fire
                    // non-bubbling scroll events on each scrolled container.
                    let states: HashMap<u32, [f32; 4]> = collect_scroll_containers(lb)
                        .iter()
                        .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                        .collect();
                    route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                        j.update_scroll_states(states);
                        for nid in scrolled_nids {
                            j.fire_element_scroll(nid);
                            // BUG-822: a programmatic container scroll is
                            // applied in full right here вЂ” there is no
                            // per-container animation to wait for вЂ” so the
                            // sequence has already ended and `scrollend`
                            // follows `scroll` in the same frame.
                            j.fire_element_scrollend(nid);
                        }
                    });
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
                }
            }
        }

        // Page-level scroll requests from JS window.scrollTo / window.scrollBy.
        // Smooth requests go through the rAF-based animation; instant ones set
        // scroll_y directly (CSS Scroll Behavior L1 В§3).
        // ADR-016 M2.2d: value-drain С‡РµСЂРµР· `route_query_js`.
        #[cfg(feature = "v8")]
        for (target_y, smooth) in self.drain_query_js(|j| j.take_page_scroll_requests()).unwrap_or_default()
        {
            if smooth {
                self.start_smooth_scroll(target_y);
            } else {
                self.scroll_to(target_y);
            }
        }

        // DOM GC idle tick: drain dead node IDs and purge JS-side per-node caches.
        // Runs every 30 s to free _lumen_listeners / _input_values entries for
        // nodes that were detached from the tree and have no live JS references.
        // ADR-016 M2.2d: dead-node computation РѕСЃС‚Р°С‘С‚СЃСЏ РЅР° UI-РїРѕС‚РѕРєРµ (РЅСѓР¶РЅС‹
        // `layout_source`-РґРѕРєСѓРјРµРЅС‚ + `&mut gc_tick`), Р° СЃР°Рј `gc_collect` вЂ” С‡РёСЃС‚С‹Р№ void вЂ”
        // СѓС…РѕРґРёС‚ С‡РµСЂРµР· `route_task_js`. Р“РµР№С‚ `Some(_js)` СЃРѕС…СЂР°РЅС‘РЅ, С‡С‚РѕР±С‹ `gc_tick.poll`
        // С‚РёРєР°Р» С‚РѕР»СЊРєРѕ РїСЂРё РЅР°Р»РёС‡РёРё JS-РєРѕРЅС‚РµРєСЃС‚Р°, РєР°Рє РїСЂРµР¶РґРµ (Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ С„Р»Р°Рі-РѕС„С„).
        // ADR-016 M2.2c-2d (20): JS-presence gate reads `self.js_present` instead of
        // borrowing the `Arc` вЂ” kept in lockstep by `set_js_ctx`, so byte-identical.
        if self.js_present
            && let Some(ls) = self.layout_source.as_ref()
        {
            let dead = {
                let doc = ls.document.lock().unwrap();
                self.gc_tick.poll(&doc)
            };
            if let Some(dead_nids) = dead {
                let ids: Vec<u32> = dead_nids
                    .iter()
                    .map(|n| n.index() as u32)
                    .collect();
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.gc_collect(&ids);
                });
            }
        }

        // Tab lifecycle: advance tier timers, trigger hibernation for overdue tabs.
        self.tick_lifecycle();

        // Focus mode (task #25): advance the Pomodoro countdown and keep
        // redrawing while the ring animates (only while active and running).
        if self.focus.active {
            self.focus.tick(now_ms);
            if self.focus.timer.running {
                self.request_redraw();
            }
        }

        // Memory pressure: poll OS every 5 s; evict caches on Medium+ pressure.
        if let Some(level) = self.memory_poll.tick(&mut self.cache_registry) {
            self.image_cache.on_memory_pressure(level);
            self.display_list_cache.on_memory_pressure(level);
            if let Some(renderer) = &mut self.renderer {
                renderer.on_layer_memory_pressure(level);
                renderer.on_atlas_memory_pressure(level);
            }
        }

        // РџРѕСЃС‚-РґСЂРµРЅР°Р¶РЅС‹Р№ check: reload, Р·Р°РїР»Р°РЅРёСЂРѕРІР°РЅРЅС‹Р№ С‡РµСЂРµР· queue_task
        // (UserInteraction source), РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ РїРѕСЃР»Рµ microtask checkpoint.
        // `take` Р°С‚РѕРјР°СЂРЅРѕ СЃР±СЂР°СЃС‹РІР°РµС‚ С„Р»Р°Рі, С‡С‚РѕР±С‹ reload РІС‹Р·РІР°Р»СЃСЏ С‚РѕР»СЊРєРѕ СЂР°Р·.
        if self.pending_reload.take() {
            self.reload();
        }

        // JS navigation: location.href=, assign(), replace(), reload().
        // Executed after the initial page render so the user sees something
        // before the redirect completes (matches browser behaviour).
        if let Some(nav) = self.pending_js_navigate.take() {
            match nav {
                JsNavigateRequest::Push(url) => {
                    click_log::log_js_nav("pushState/location.href", &url);
                    // BUG-293: same file://-resolution + webв†’file guard as popups.
                    match resolve_js_navigation(&url, &self.source) {
                        Ok(source) => self.navigate_to(source),
                        Err(reason) => eprintln!("РќР°РІРёРіР°С†РёСЏ Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅР°: {reason}"),
                    }
                }
                JsNavigateRequest::Replace(url) => {
                    click_log::log_js_nav("replaceState/location.replace", &url);
                    match resolve_js_navigation(&url, &self.source) {
                        Ok(source) => self.navigate_replace(source),
                        Err(reason) => eprintln!("РќР°РІРёРіР°С†РёСЏ Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅР°: {reason}"),
                    }
                }
                JsNavigateRequest::Reload => {
                    click_log::log_js_nav("location.reload", &self.source.describe());
                    self.reload();
                }
                // BUG-383: `form.submit()` / `form.requestSubmit()` from script.
                // The `submit` event (if any) already fired on the JS side, so
                // this runs the submission itself and nothing else.
                JsNavigateRequest::SubmitForm { form, submitter } => {
                    let form_id = NodeId::from_index(form as usize);
                    let submitter_id =
                        (submitter >= 0).then(|| NodeId::from_index(submitter as usize));
                    click_log::log_js_nav("form.submit", &self.source.describe());
                    self.run_form_submission(form_id, submitter_id, false);
                }
            }
        }
    }
}

/// A queued `AutomationCommand::Wait` request (SDC-1b), re-checked once per
/// frame in `about_to_wait` rather than blocking the event loop.
pub(crate) struct PendingWait {
    /// Condition to poll вЂ” see [`check_pending_wait_condition`].
    pub(crate) cond: WaitCondition,
    /// When this wait gives up and replies `AutomationReply::Error`.
    pub(crate) deadline: std::time::Instant,
    /// Where to send the `Ack`/`Error` reply once resolved.
    pub(crate) reply_tx: std::sync::mpsc::Sender<AutomationReply>,
}
