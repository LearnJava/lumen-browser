//! `ApplicationHandler::user_event` — приём [`LoadEvent`] из потока
//! загрузки страницы (SPLIT-SH2).
//!
//! Тело вынесено из `impl ApplicationHandler<LoadEvent> for Lumen`
//! (`super`) как есть; параметр `_event_loop` в теле не использовался
//! и в переходнике не передаётся.

use crate::*;

impl Lumen {
    pub(crate) fn on_user_event(&mut self, event: LoadEvent) {
        match event {
            // Nothing to do — its only purpose is to interrupt `ControlFlow::Wait`
            // (see the variant doc comment); the automation dispatch that runs
            // right after in `about_to_wait` handles the actual command.
            LoadEvent::AutomationWake => {}
            LoadEvent::EarlyPreloadHints(hints, base, generation) => {
                if generation != self.load_generation { return; }
                // Ранние хинты из первого chunk — отправить в sink немедленно.
                // `preload_dispatched` запоминает URL, чтобы финальный scan
                // в LoadDone их не дублировал.
                dispatch_preload_hints(&hints, &base, &self.event_sink, &mut self.preload_dispatched);
            }
            LoadEvent::DocumentBase(base, generation) => {
                if generation != self.load_generation { return; }
                self.document_base = Some((base, generation));
            }
            LoadEvent::HtmlChunk(chunk, generation) => {
                if generation != self.load_generation { return; }
                let builder = self.stream_builder
                    .get_or_insert_with(lumen_html_parser::IncrementalTreeBuilder::new);
                builder.feed_bytes(&chunk);
                if self.stream_last_paint.elapsed().as_millis() >= STREAM_PAINT_INTERVAL_MS {
                    // Клонируем снапшот для layout — builder остаётся живым.
                    let doc_snap = builder.as_doc().clone();
                    self.paint_partial_dom(&doc_snap);
                    self.stream_last_paint = std::time::Instant::now();
                }
            }
            LoadEvent::CssLoaded(boxed, generation) => {
                if generation != self.load_generation { return; }
                // PH1-2: CSS загружен параллельным потоком — мёрджим в stream_sheet.
                // Применится в следующем paint_partial_dom (16 мс throttle).
                // `merge_from` also mints a new `StylesheetRevision`, which is
                // what tells the cascade's rule-index cache that this sheet is
                // no longer the one it indexed (BUG-341 S21). It replaced a
                // hand-rolled field-by-field merge here that had fallen two
                // fields behind the struct.
                self.stream_sheet.merge_from(*boxed);
            }
            LoadEvent::ImageDecoded { src, image, animated } => {
                // PH1-2c: картинка декодирована параллельным потоком во время
                // streaming. Регистрируем в renderer-е по ключу `src` (тот же,
                // что эмитит layout в `DrawImage`), кладём в декод-кэш и просим
                // redraw — следующий кадр заменит placeholder реальной картинкой.
                let image = *image;
                // BUG-735: пиксели у рендерера — это ещё не размер для layout.
                // Intrinsic-пара доезжает до DOM только через
                // `apply_intrinsic_size`, а этот путь её никогда не звал, поэтому
                // на клиентски отрисованной странице (все картинки приходят
                // именно сюда, BUG-730) `height: auto` честно считался нулём.
                // Запоминаем размер и помечаем проход — разнесём по узлам одним
                // коалесцированным проходом на ближайшем кадре.
                self.stream_image_sizes
                    .insert(src.clone(), (image.width, image.height));
                self.stream_image_sizes_dirty = true;
                if let Some(r) = self.renderer.as_mut() {
                    // BUG-272 срез 17: cache-insert returns the Arc handle; register
                    // with it so raw_images shares the CPU cache's allocation.
                    let handle = self.image_cache.insert(lumen_image::ImageKey::new(&src), image);
                    if let Err(e) = r.register_image(src.clone(), handle) {
                        eprintln!("Streaming-картинка: не зарегистрирована {src}: {e}");
                    }
                } else {
                    // Renderer ещё не создан (окно не открыто) — отложим заливку
                    // в GPU до `resumed`.
                    self.pending_images.push((src.clone(), Arc::new(image)));
                }
                if let Some(gif) = animated {
                    // Многокадровый GIF: тикается в `RedrawRequested`.
                    self.gif_last_frame.remove(&src);
                    self.animated_gifs.insert(src, *gif);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            LoadEvent::FontLoaded { family, weight, style, unicode_range, bytes } => {
                // PH3-19 FOUT swap: web-шрифт прибыл из фонового потока.
                // Регистрируем в page_font_registry (FontProvider для renderer-а),
                // добавляем в web_fonts (для relayout MultiFontMeasurer),
                // запускаем relayout — следующий кадр использует уже загруженный шрифт.
                eprintln!("FontLoaded: «{family}» weight={weight}");
                self.page_font_registry.register_from_bytes(
                    &family,
                    weight,
                    style,
                    &unicode_range,
                    bytes.clone(),
                );
                // Update renderer's font provider so GPU glyph atlas picks up the new face.
                if let Some(r) = self.renderer.as_mut() {
                    r.set_font_provider(Some(
                        Arc::clone(&self.page_font_registry) as Arc<dyn lumen_core::FontProvider>,
                    ));
                }
                self.web_fonts.push(LoadedWebFont { family, weight, style, unicode_range, bytes });
                // Relayout with the now-registered web font (FOUT → FOIT swap).
                // ADR-016 M2.2b-8: the swap is a whole-page restyle (font metrics
                // change) with no synchronous geometry read of its own — the same
                // async-safe shape as the theme flip (M2.2b-4). The just-pushed
                // font is captured by `submit_relayout_job`'s `web_fonts` snapshot,
                // so the off-thread reflow sees it. Route it off-thread when the
                // engine thread is enabled.
                self.relayout_chrome();
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            LoadEvent::LoadDone(raw, generation) => {
                // U-1: drop a superseded navigation's final pipeline — otherwise a
                // slow earlier load would render its page over the newer one.
                if generation != self.load_generation { return; }
                eprintln!("Streaming завершён, финальный pipeline (off-thread)");
                if lumen_paint::frame_log_enabled()
                    && let Some(ms) = bench_frames::since_process_start_ms()
                {
                    eprintln!("[frame:cold-start] LoadDone (spawning render_bytes) at {ms:.0}ms");
                }
                self.stream_builder = None;
                self.stream_sheet = lumen_css_parser::Stylesheet::default();
                let viewport = self.renderer.as_ref().map_or_else(
                    || Size::new(1024.0, 720.0),
                    |r| {
                        let s = r.viewport_size();
                        Size::new(s.width, s.height)
                    },
                );
                // Storage-хэндлы требуют `&mut self` (ls_storage) — берём их на
                // UI-потоке, дальше это `Arc`-и (Send), уезжающие в рендер-поток.
                let ls_store = ls_store_for_base(&raw.base, &mut self.ls_storage);
                // BUG-836: this is the main navigation path — the entry lives in
                // the tab's map, so the store outlives the document being built.
                let ss_store = ss_store_for_base(&raw.base, &mut self.ss_storage);
                let idb_backend = idb_store_for_base(&raw.base, self.idb_dir.as_deref());
                let sw_backend = sw_store_for_base(&raw.base, &self.sw_backend);
                // BUG-171 этап 2: тяжёлый финальный pipeline (fetch скриптов →
                // QuickJS → fetch+декод картинок/CSS/шрифтов → layout) уезжает с
                // UI-потока на фоновый. Пока он крутится, event loop остаётся
                // живым (перерисовка последнего streaming-кадра, ввод). QuickJS-
                // рантайм теперь хэндл (ADR-014/B-1): создаётся на рендер-потоке и
                // безопасно (`Send`) пересылается на UI внутри `RenderDone`.
                // `preload_dispatched` забираем (рендер его дедуплицирует) и
                // возвращаем в `RenderDone`; на это время self-копия пуста, что
                // безопасно — следующая навигация пересоздаёт набор в streaming.
                let sink = self.event_sink.clone();
                let hp = Arc::clone(&self.hyp_provider);
                let cookie_jar = Some(self.active_cookie_jar());
                let sw_worker_store = Some(Arc::clone(&self.sw_worker_store));
                let cache_backend =
                    Some(Arc::clone(&self.cache_store) as Arc<dyn lumen_core::ext::CacheBackend>);
                let cookie_banner_dismiss = self.cookie_banner_dismiss;
                let deterministic = self.deterministic;
                let dark_mode = self.dark_mode;
                let proxy = self.load_proxy.clone();
                let mut preload_dispatched = std::mem::take(&mut self.preload_dispatched);
                let target = self.target_color_space();
                std::thread::spawn(move || {
                    let result = render_bytes(
                        &raw.bytes,
                        raw.content_type,
                        &raw.base,
                        sink,
                        viewport,
                        &mut preload_dispatched,
                        ls_store,
                        ss_store,
                        idb_backend,
                        sw_backend,
                        &*hp,
                        cookie_banner_dismiss,
                        deterministic,
                        dark_mode,
                        cookie_jar,
                        raw.cross_origin_isolated,
                        sw_worker_store,
                        cache_backend,
                        target,
                        raw.cache_control_no_store,
                    )
                    .map_err(|e| e.to_string());
                    // Если event loop уже закрыт — Box (вместе с JS-хэндлом)
                    // дропнется здесь, корректно завершив JS-поток.
                    let _ = proxy.send_event(LoadEvent::RenderDone(
                        Box::new(RenderOutcome { result, preload_dispatched }),
                        generation,
                    ));
                });
            }
            LoadEvent::RenderDone(outcome, generation) => {
                // BUG-171 этап 2: устаревшую навигацию отбрасываем — её страница и
                // JS-хэндл дропаются вместе с `outcome` (JS-поток завершается).
                if generation != self.load_generation { return; }
                if lumen_paint::frame_log_enabled()
                    && let Some(ms) = bench_frames::since_process_start_ms()
                {
                    eprintln!("[frame:cold-start] RenderDone received on UI thread at {ms:.0}ms");
                }
                let RenderOutcome { result, preload_dispatched } = *outcome;
                self.preload_dispatched = preload_dispatched;
                match result {
                    Ok((page, new_layout_source, new_js_ctx)) => {
                        click_log::log_load_ok(&self.source.describe(), page.title.as_deref().unwrap_or(""));
                        self.apply_loaded_page(page, Some(new_layout_source), new_js_ctx);
                        // Deliver W3C Navigation Timing L2 entry after streaming load completes.
                        // ADR-016 M2.2c-2d (20): same conversion as the reload path above —
                        // `self.js_present` gate + `route_task_js`, `nav_start` still taken
                        // unconditionally. Flag-off byte-identical; flag-on off-UI-thread.
                        #[cfg(feature = "v8")]
                        {
                            let nav_start = self.nav_start.take();
                            if let (true, Some(start), Some(url)) =
                                (self.js_present, nav_start, self.source.url_str().map(str::to_owned))
                            {
                                let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
                                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                                    j.deliver_nav_timing(&url, duration_ms);
                                });
                            }
                        }
                        click_log::log_page_ready(&self.source.describe(), self.scroll_y);
                        self.record_render_health();
                    }
                    Err(e) => {
                        self.nav_start = None;
                        // Settled navigation error — mark done so a
                        // `wait{document_ready}` resolves at once (BUG-308).
                        self.load_failed = true;
                        self.load_error_message = Some(e.clone());
                        click_log::log_load_err(&self.source.describe(), &e);
                        health_log::log_load_error(&self.source.describe(), &e);
                        eprintln!("Ошибка финального render {}: {e}", self.source.describe());
                    }
                }
            }
            LoadEvent::LoadError(msg, generation) => {
                if generation != self.load_generation { return; }
                self.nav_start = None;
                // Settled navigation error — mark done so a
                // `wait{document_ready}` resolves at once (BUG-308).
                self.load_failed = true;
                self.load_error_message = Some(msg.clone());
                click_log::log_load_err(&self.source.describe(), &msg);
                health_log::log_load_error(&self.source.describe(), &msg);
                eprintln!("Ошибка загрузки {}: {msg}", self.source.describe());
                self.stream_builder = None;
                self.stream_sheet = lumen_css_parser::Stylesheet::default();
            }
        }
    }
}
