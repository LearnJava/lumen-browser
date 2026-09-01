//! Page loading in the shell: reload, the streaming load, image/media
//! subresources of the current document and committing a `LoadedPage`.
//!
//! SPLIT-SH1 (2026-08-26): moved verbatim out of `main.rs`. Behaviour, order of
//! operations and method bodies are unchanged; only module path and visibility
//! (`fn` -> `pub(crate) fn`, required for a caller in the parent module) differ.

use crate::*;

impl Lumen {
    /// Fetch, decode and register lazy images whose node IDs were queued by JS.
    ///
    /// Called from `relayout()` after `_lumen_deliver_lazy_images()` fires load
    /// requests for images that entered the lazy-load proximity margin.
    /// Fetched images are registered in the renderer immediately so the next
    /// repaint (already requested by `relayout`) shows them.
    #[cfg(feature = "v8")]
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn fetch_and_register_lazy_images(&mut self, requests: Vec<(u32, String)>) {
        let base = match &self.source {
            PageSource::File(p) => ResourceBase::File(p.clone()),
            PageSource::Url(u) => ResourceBase::Url(u.clone()),
            PageSource::Snapshot { base_url, .. } => ResourceBase::Url(base_url.clone()),
            PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => return,
        };
        for (nid, url) in requests {
            let bytes = match fetch_image_bytes(&url, &base, &self.event_sink, Some(self.active_cookie_jar())) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Lazy: пропуск {url}: {e}");
                    continue;
                }
            };

            // Animated GIF detection for lazy-loaded images.
            if lumen_image::is_gif(&bytes) {
                match lumen_image::decode_gif_animated(&bytes) {
                    Ok(gif) if gif.frame_count() > 1 => {
                        // BUG-272 срез 19: only the first frame is decoded eagerly here.
                        let first = match gif.frame_image(0) {
                            Ok(img) => img,
                            Err(e) => {
                                eprintln!("Lazy: не декодируется GIF {url}: {e}");
                                continue;
                            }
                        };
                        if let Some(src) = self.layout_source.as_ref() {
                            let mut doc = src.document.lock().unwrap();
                            let node_id = NodeId::from_index(nid as usize);
                            apply_intrinsic_size(&mut doc, node_id, first.width, first.height);
                        }
                        eprintln!(
                            "Lazy GIF-анимация: {} ({}×{}, {} кадров)",
                            url, gif.width, gif.height, gif.frame_count()
                        );
                        if let Some(r) = self.renderer.as_mut() {
                            // BUG-272 срез 17: insert into the CPU cache first, then
                            // register the returned Arc handle — raw_images shares the
                            // cache's allocation instead of a second pixel copy.
                            let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), first);
                            if let Err(e) = r.register_image(url.clone(), handle) {
                                eprintln!("Lazy GIF: не зарегистрирована {url}: {e}");
                            }
                        } else {
                            self.pending_images.push((url.clone(), Arc::new(first)));
                        }
                        self.gif_last_frame.remove(&url);
                        self.animated_gifs.insert(url, gif);
                        continue;
                    }
                    Ok(gif) => {
                        if let Ok(img) = gif.frame_image(0) {
                            if let Some(src) = self.layout_source.as_ref() {
                                let mut doc = src.document.lock().unwrap();
                                let node_id = NodeId::from_index(nid as usize);
                                apply_intrinsic_size(&mut doc, node_id, img.width, img.height);
                            }
                            eprintln!("Lazy загружена (GIF, 1 кадр): {url} ({}×{})", img.width, img.height);
                            if let Some(r) = self.renderer.as_mut() {
                                let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), img);
                                if let Err(e) = r.register_image(url.clone(), handle) {
                                    eprintln!("Lazy: не зарегистрирована {url}: {e}");
                                }
                            } else {
                                self.pending_images.push((url, Arc::new(img)));
                            }
                        }
                        continue;
                    }
                    Err(e) => {
                        eprintln!("Lazy: не декодируется GIF {url}: {e}");
                        continue;
                    }
                }
            }

            let image = match lumen_image::decode(&bytes) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("Lazy: не декодируется {url}: {e}");
                    continue;
                }
            };
            eprintln!("Lazy загружена: {} ({}×{}, {:?})", url, image.width, image.height, image.format);
            // Apply intrinsic size to DOM so next relayout picks up correct dimensions.
            if let Some(src) = self.layout_source.as_ref() {
                let mut doc = src.document.lock().unwrap();
                let node_id = NodeId::from_index(nid as usize);
                apply_intrinsic_size(&mut doc, node_id, image.width, image.height);
            }
            if let Some(r) = self.renderer.as_mut() {
                let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), image);
                if let Err(e) = r.register_image(url.clone(), handle) {
                    eprintln!("Lazy: не зарегистрирована {url}: {e}");
                }
            } else {
                self.pending_images.push((url, Arc::new(image)));
            }
        }
    }

    /// Mirror `page_tracks.tracks_by_video` into the shared
    /// [`lumen_js::TextTrackStore`] so `video.textTracks` reflects the parsed
    /// `<track>` cues. Fully replaces the store's contents (clear + repopulate),
    /// so navigating to a track-less page empties it.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn sync_text_track_store(&self) {
        let mut guard = self.text_track_store.tracks.lock().unwrap();
        guard.clear();
        for (node, tracks) in &self.page_tracks.tracks_by_video {
            let nid = node.index() as u32;
            let data: Vec<lumen_js::TextTrackData> = tracks
                .iter()
                .map(|t| lumen_js::TextTrackData {
                    kind: t.kind.clone(),
                    label: t.label.clone(),
                    language: t.language.clone(),
                    mode: t.mode.clone(),
                    cues: t
                        .cues
                        .iter()
                        .map(|c| lumen_js::CueData {
                            id: c.id.clone().unwrap_or_default(),
                            start: c.start_s,
                            end: c.end_s,
                            text: c.text.clone(),
                        })
                        .collect(),
                })
                .collect();
            guard.insert(nid, data);
        }
    }

    /// Advance GIF-backed `<video>` playback: drain pending loads, decode GIFs,
    /// register current frames, request redraws while any video is playing.
    ///
    /// Called once per render tick (Step 2.6) so video frames stay in sync with
    /// the render rate.  `elapsed_ms` is milliseconds since `self.epoch`.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn tick_video_gifs(&mut self, elapsed_ms: u64) {
        // Drain pending load requests queued by JS `__lumen_video_load`.
        let loads: Vec<(u32, String)> = self
            .video_gif_store
            .pending_loads
            .lock()
            .unwrap()
            .drain(..)
            .collect();

        for (nid, src) in loads {
            // Resolve URL relative to current page source.
            let base = match &self.source {
                PageSource::File(p) => ResourceBase::File(p.clone()),
                PageSource::Url(u) => ResourceBase::Url(u.clone()),
                PageSource::Snapshot { base_url, .. } => ResourceBase::Url(base_url.clone()),
                PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => continue,
            };

            let bytes = match fetch_image_bytes(&src, &base, &self.event_sink, Some(self.active_cookie_jar())) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("video GIF: пропуск {src}: {e}");
                    continue;
                }
            };

            if !lumen_image::is_gif(&bytes) {
                eprintln!("video GIF: {src} не является GIF");
                continue;
            }

            match lumen_image::decode_gif_animated(&bytes) {
                Ok(gif) => {
                    // BUG-272 срез 17/19: Arc so register/pending share the buffer;
                    // only the first frame is materialised eagerly.
                    let first = match gif.frame_image(0) {
                        Ok(img) => Arc::new(img),
                        Err(e) => {
                            eprintln!("video GIF: ошибка декодирования {src}: {e}");
                            continue;
                        }
                    };
                    let key = format!("video:{nid}");
                    if let Some(r) = self.renderer.as_mut() {
                        if let Err(e) = r.register_image(key.clone(), Arc::clone(&first)) {
                            eprintln!("video GIF: не зарегистрирован {key}: {e}");
                        }
                    } else {
                        self.pending_images.push((key.clone(), first));
                    }
                    // Apply intrinsic size so layout uses actual GIF dimensions.
                    if let Some(src_ref) = self.layout_source.as_ref() {
                        let mut doc = src_ref.document.lock().unwrap();
                        let node_id = lumen_dom::NodeId::from_index(nid as usize);
                        apply_intrinsic_size(&mut doc, node_id, gif.width, gif.height);
                    }
                    eprintln!(
                        "video GIF: загружен nid={nid} ({}×{}, {} кадров)",
                        gif.width, gif.height, gif.frame_count()
                    );
                    // Store frames in shell-side map (lumen_image dep stays in shell).
                    let cycle_ms: u64 = gif.total_cycle_ms();
                    let loop_count = match gif.loop_count {
                        lumen_image::GifLoopCount::Infinite | lumen_image::GifLoopCount::Finite(0) => 0u32,
                        lumen_image::GifLoopCount::Finite(n) => u32::from(n),
                    };
                    self.video_gif_store
                        .playback
                        .lock()
                        .unwrap()
                        .insert(nid, lumen_js::video_gif_store::VideoPlaybackState {
                            paused: true,
                            position_ms: 0,
                            play_epoch_ms: None,
                            cycle_ms,
                            loop_count,
                            width: gif.width,
                            height: gif.height,
                        });
                    self.video_gif_frames.insert(nid, gif);
                    // Trigger relayout so new intrinsic size takes effect.
                    self.request_redraw();
                }
                Err(e) => eprintln!("video GIF: ошибка декодирования {src}: {e}"),
            }
        }

        // Advance frames for playing videos.
        let playback = self.video_gif_store.playback.lock().unwrap();
        let mut has_playing = false;

        let updates: Vec<(u32, usize, lumen_image::Image)> = playback
            .iter()
            .filter_map(|(nid, state)| {
                if state.paused {
                    return None;
                }
                has_playing = true;
                let cycle = state.cycle_ms;
                if cycle == 0 {
                    return None;
                }
                let cur_ms = state.current_ms(elapsed_ms);
                let loop_ms = cur_ms % cycle;
                let gif = self.video_gif_frames.get(nid)?;
                let idx = gif.frame_index_at(loop_ms);
                let last = self.video_gif_last_frame.get(nid).copied().unwrap_or(usize::MAX);
                if last == idx {
                    return None;
                }
                Some((*nid, idx, gif.frame_image(idx).ok()?))
            })
            .collect();
        drop(playback);

        for (nid, idx, image) in updates {
            let key = format!("video:{nid}");
            if let Some(r) = self.renderer.as_mut()
                && let Err(e) = r.register_image(key.clone(), Arc::new(image))
            {
                eprintln!("video GIF кадр {key}[{idx}]: {e}");
            }
            self.video_gif_last_frame.insert(nid, idx);
        }

        if has_playing {
            self.request_redraw();
        }
    }

    /// Same-page fragment navigation: update `:target` CSS state and scroll to
    /// the target element. `fragment` is the id without the leading `#`; an empty
    /// string scrolls to the top and clears `:target`.
    ///
    /// Triggers a full re-layout so that `:target`-based CSS rules take effect
    /// before the scroll position is calculated.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn navigate_fragment(&mut self, fragment: String) {
        // Same-document fragment navigation must keep the JS side in sync: update
        // `location`, push a same-document history entry, and fire `hashchange`
        // (HTML LS §7.4.2 fragment-navigation). Route through the existing JS
        // `_lumen_navigate_or_fragment` path — it resolves the target, sees only
        // the fragment differs, updates `location`, queues a `HistoryUrlUpdate`
        // (drained into `nav_back` so back/forward works), and fires `hashchange`.
        let new_url = links::fragment_url(self.current_display_url(), &fragment);
        // ADR-016 M2.2c-2d (16): hashchange fire-and-forget void-dispatch через
        // `route_eval_js` — оба `_lumen_*`-вызова чистый void без синхронного чтения
        // результата следом (`location`/`hashchange` фиксируются JS-стороной, а
        // `HistoryUrlUpdate` дренится позже через `take_nav_updates`). Под флагом
        // (`LUMEN_ENGINE_THREAD=1`) уходят off-UI-thread двумя `task` в FIFO-порядке
        // (dispatch → navigate сохранён); без флага (по умолчанию) — синхронные
        // вызовы по UI-хэндлу, **байт-идентично** прежним `js.eval_js`. `escaped`
        // строится до маршрутизации; борроу `engine_thread`/`js_ctx` — раздельный.
        if self.js_present {
            let escaped = new_url.replace('\\', "\\\\").replace('\'', "\\'");
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("_lumen_dispatch_navigate('fragment', '{escaped}', true, true)"),
            );
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("_lumen_navigate_or_fragment('{escaped}', false)"),
            );
        }
        if let Some(src) = self.layout_source.as_mut() {
            let mut doc = src.document.lock().unwrap();
            if fragment.is_empty() {
                doc.set_target::<String>(None);
            } else {
                doc.set_target(Some(fragment.clone()));
            }
        }
        // Re-layout so :target cascade is applied.
        self.relayout();
        if fragment.is_empty() {
            self.scroll_to(0.0);
            return;
        }
        let node_id = self
            .layout_source
            .as_ref()
            .and_then(|src| links::find_element_by_id(&src.document.lock().unwrap(), &fragment));
        let target_rect = node_id.and_then(|nid| {
            self.layout_box.as_ref().and_then(|lb| forms::find_box_rect(lb, nid))
        });
        let target_y = target_rect.map(|r| r.y);
        click_log::log_fragment(&fragment, target_y.is_some());
        // BUG-338: bring the target into view within any nested scrolling
        // ancestors BEFORE the page-level scroll below — real fragment
        // navigation / `scrollIntoView()` walks the whole scrollable
        // ancestor chain, not just the page.
        if let (Some(nid), Some(rect)) = (node_id, target_rect) {
            self.scroll_nested_ancestors_into_view(nid, rect);
        }
        if let Some(y) = target_y {
            // CSS Scroll Behavior L1 §3: respect scroll-behavior on the scrolling box.
            // The page viewport's scroll-behavior comes from the root (<html>) element.
            if self.page_scroll_behavior() == ScrollBehavior::Smooth {
                self.start_smooth_scroll(y);
            } else {
                self.scroll_to(y);
            }
        }
    }

    /// Returns the effective `scroll-behavior` for the page viewport (CSS Scroll Behavior L1 §3).
    /// Reads from the first non-root layout box (the `<html>` element's style).
    pub(crate) fn page_scroll_behavior(&self) -> ScrollBehavior {
        self.layout_box
            .as_ref()
            .and_then(|lb| lb.children.first())
            .map(|html_box| html_box.style.scroll_behavior)
            .unwrap_or(ScrollBehavior::Auto)
    }

    /// Перезагрузить текущий источник: fetch/parse/layout/paint снова. На
    /// `PageSource::Empty` — no-op (грузить нечего). При ошибке — оставляем
    /// предыдущий display_list, печатаем причину в stderr.
    pub(crate) fn reload(&mut self) {
        if matches!(self.source, PageSource::Empty) {
            return;
        }
        // Record navigation start for PerformanceNavigationTiming (Navigation Timing L2 §4.2).
        self.nav_start = Some(std::time::Instant::now());
        // A fresh navigation supersedes any prior settled error (BUG-308).
        self.load_failed = false;
        self.load_error_message = None;
        click_log::log_load_start(&self.source.describe());
        println!("Reload: {}", self.source.describe());

        // U-1: неблокирующая навигация. Когда окно уже создано (любая навигация
        // после первого кадра — клик по ссылке, адресная строка, back/forward,
        // JS location.href=), грузим через тот же асинхронный streaming-пайплайн,
        // что и первичная загрузка в `resumed`: тело фетчится в фоновом потоке,
        // окно продолжает рисовать промежуточные кадры, а тяжёлый финальный
        // pipeline (`render_bytes`) исполняется один раз на UI-потоке в
        // `LoadEvent::LoadDone`. Раньше `reload()` делал весь fetch+parse+JS+layout
        // синхронно прямо здесь — окно мёрзло на всё время навигации.
        if self.window.is_some() {
            // Сбрасываем состояние прошлого streaming-цикла — это новая страница
            // (зеркалит блок в `resumed`). `stream_builder = None` обязателен,
            // иначе chunk-и допишутся в DOM предыдущей страницы.
            self.preload_dispatched.clear();
            // BUG-839: a Resource Timing row belongs to the document that asked
            // for the load, and the loads of THIS document are about to start.
            // Clearing here rather than when the runtime appears is the whole
            // point: `set_js_ctx` runs *after* `source.load` has already
            // fetched the page's stylesheets, scripts and images, so a clear
            // there would throw away exactly the rows the page is owed.
            resource_timing::clear();
            self.stream_images_requested.clear();
            self.stream_image_sizes.clear();
            self.stream_image_sizes_dirty = false;
            self.stream_sheet = lumen_css_parser::Stylesheet::default();
            self.stream_layout_seeded = false;
            self.stream_builder = None;
            self.load_generation = self.load_generation.wrapping_add(1);
            self.start_streaming_load(self.load_generation);
            return;
        }

        // Fallback (окна ещё нет — редкий путь, напр. headless/тесты): прежняя
        // синхронная загрузка.
        // Phase 4c: попробовать загрузить через GpuSession (WinitSession)
        // для File и Url; fallback к старому пути для Snapshot
        let load_result = if let Some(page) = self.reload_via_gpu_session() {
            // WinitSession загрузка успешна
            Ok((page, None, None))
        } else {
            // Fallback к старому пути (PageSource::Snapshot, или ошибка WinitSession)
            let viewport = self.renderer.as_ref().map_or_else(
                || Size::new(1024.0, 720.0),
                |r| {
                    let s = r.viewport_size();
                    Size::new(s.width, s.height)
                },
            );
            let ls_store = self.source.origin_str().map(|o| {
                Arc::clone(self.ls_storage.entry(o).or_insert_with(|| {
                    Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
                }))
            });
            // BUG-836: same origin key, tab-scoped map — the entry outlives this
            // document, so the next one loaded here reads what this one wrote.
            let ss_store = self.source.origin_str().map(|o| {
                Arc::clone(self.ss_storage.entry(o).or_insert_with(|| {
                    Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
                }))
            });
            let idb_backend = self
                .source
                .url_str()
                .and_then(|u| idb_store_for_url(u, self.idb_dir.as_deref()));
            let sw_backend = self.source.origin_str().map(|o| {
                Arc::new(lumen_storage::SwStore::new(Arc::clone(&self.sw_backend), o))
                    as Arc<dyn lumen_core::ext::SwBackend>
            });
            self.source.load(self.event_sink.clone(), viewport, ls_store, ss_store, idb_backend, sw_backend, &*self.hyp_provider, self.cookie_banner_dismiss)
        };

        match load_result {
            Ok((page, new_layout_source, new_js_ctx)) => {
                // Drop JS closures before layout_source to release Arc<Mutex<Document>>
                // clones held inside QuickJS closures before LayoutSource's Arc drops.
                self.set_js_ctx(None);
                self.layout_source = new_layout_source;
                self.set_js_ctx(new_js_ctx);
                // ADR-016 M2.2c-2b: зеркалим новый хэндл + DOM в движковый поток.
                self.sync_engine_js_state();
                // The new runtime starts empty; re-seed it with the current Navigation state.
                self.commit_nav_state();
                self.content_height = content_height_of(&page.display_list);
                self.content_width = content_width_of(&page.display_list);
                // On full page load, mark all tiles dirty — content has changed completely.
                self.tile_grid.mark_all_dirty(self.content_width, self.content_height);
                // BUG-480 срез 14: под-документы новой страницы заменяют старые
                // (то же, что делает `apply_loaded_page`, — этот резервный путь
                // reload'а без окна о фреймах не знал вовсе и оставлял хэндлы
                // ПРЕДЫДУЩЕГО документа живыми). Строго до записи списка: она
                // вклеивает в него содержимое фреймов.
                self.frames = page.frames;
                self.frame_env = page.frame_env;
                // FRAME-4 срез 3: см. тот же сброс в `apply_loaded_page`.
                frames::clear_frame_nav_requests(&mut self.frame_nav_requests);
                // BUG-480 срез 16: индекс в этом списке — единственное, чем
                // адресован фрейм под курсором, поэтому пережить его замену он
                // не может: указывал бы на чужой хэндл.
                self.hovered_frame = None;
                self.set_display_list(page.display_list);
                self.animation_scheduler.clear();
                self.transition_scheduler = TransitionScheduler::new();
                self.starting_style_tracker = StartingStyleTracker::new();
                self.prev_styles.clear();
                collect_box_styles(&page.layout_box, &mut self.prev_styles);
                // BUG-341 S7: this producer bypasses the restyle-aware path
                // entirely — invalidate so the next `try_relayout_raf_incremental`
                // doesn't diff a stale cache against this fresh tree.
                self.page_prev_cascade_styles = None;
                self.layout_box = Some(page.layout_box);
                // content-visibility: auto (BB-4): новая страница — ratchet с нуля.
                self.cv_relevant.clear();
                self.cv_events.clear();
                self.cv_skipped.clear();
                self.cv_auto_state.clear();
                self.refresh_cv_state();
                self.update_snap_containers();
        self.update_scroll_containers();
                // Push initial layout geometry so JS can query bounding rects
                // immediately after page load (before the first relayout).
                // ADR-016 M2.2c-2d: routed off-thread like the relayout push above —
                // the three owned-arg void calls go through `route_task_js`; the
                // `self.js_present` gate keeps the geometry collection JS-gated
                // (byte-identical with the flag off).
                #[cfg(feature = "v8")]
                if self.js_present
                    && let Some(lb_ref) = self.layout_box.as_ref()
                {
                    let viewport = self.renderer.as_ref().map_or_else(
                        || Size::new(1024.0, 720.0),
                        |r| {
                            let s = r.viewport_size();
                            Size::new(s.width, s.height)
                        },
                    );
                    let rects = collect_layout_rects(lb_ref);
                    let hit_test_tree = Arc::new(lb_ref.clone());
                    let styles = collect_computed_styles(lb_ref);
                    let customs = collect_custom_properties(lb_ref);
                    let (vw, vh) = (viewport.width, viewport.height);
                    route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                        js.update_layout_rects(rects);
                        js.update_hit_test_tree(hit_test_tree);
                        js.update_computed_styles(styles);
                        js.update_custom_properties(customs);
                        js.update_viewport_size(vw, vh);
                    });
                }
                self.title = page.title;
                if let Some(t) = &self.title {
                    self.tab_strip.set_active_title(t.as_str());
                }
                self.anim_frame = None;
                // Display list другой → старые match-rect-ы невалидны.
                // Closing полностью сбрасывает query/active — пользователю
                // нужно открыть find заново после reload, что естественно.
                self.find.close();
                self.address_bar.close();
                // Новая страница — показываем сверху-слева (либо восстанавливаем
                // offset из back/forward, как в `apply_loaded_page`).
                let (rx, ry) = self.pending_restore_scroll.take().unwrap_or((0.0, 0.0));
                self.scroll_x = rx;
                self.scroll_y = ry;
                // Любой активный drag прерывается (content_height другой,
                // thumb-геометрия пересчитана с нуля).
                self.scroll_drag = None;
                self.frame_scroll_drag = None;
                // Активные анимации старой страницы сбрасываем.
                self.scroll_anim = None;
                self.momentum_anim = None;
                self.forward_momentum_stop();
                self.touchpad_vel = (0.0, 0.0);
                // Reset CPU image cache for the reloaded page (10E.4 scroll-discard).
                self.image_cache.clear();
                if let Some(r) = self.renderer.as_mut() {
                    // Старая GPU-cache картинок относится к предыдущей странице
                    // (даже если src совпадает, content мог измениться). Чистим
                    // и регистрируем заново.
                    r.clear_images();
                    for (src, image) in &page.images {
                        // BUG-272 срез 17: `image` — Arc из IMAGE_CACHE; register
                        // клонирует указатель, raw_images разделяет аллокацию.
                        if let Err(err) = r.register_image(src.clone(), Arc::clone(image)) {
                            eprintln!("Картинка {src} не зарегистрирована: {err}");
                        }
                        self.image_cache.insert(lumen_image::ImageKey::new(src), (**image).clone());
                    }
                } else {
                    // Renderer ещё не создан — обычно невозможно (reload идёт
                    // по клавише, окно уже есть), но защитимся: складываем в
                    // pending_images, resumed подхватит.
                    self.pending_images = page.images;
                }
                if let Some(w) = self.window.as_ref() {
                    w.set_title(&window_title(self.title.as_deref()));
                    w.request_redraw();
                }
                // JS may have requested navigation via location.href= etc.
                // Store it for processing in about_to_wait (after first render).
                self.pending_js_navigate = page.js_navigate;
                let title = self.title.as_deref().unwrap_or("");
                // Deliver W3C Navigation Timing L2 entry (§4.2) to JS PerformanceObservers.
                // ADR-016 M2.2c-2d (20): last direct `self.js_ctx` read on the reload
                // nav-timing path → `self.js_present` gate + `route_task_js`. `nav_start`
                // is still taken unconditionally (as the old tuple did), so on the «no JS»
                // / «no start» / «no url» branch it is cleared exactly as before. Delivery
                // is a fire-and-forget void: under the flag (`LUMEN_ENGINE_THREAD=1`) it
                // goes off-UI-thread; flag-off (default) stays byte-identical to the old
                // direct `js.deliver_nav_timing(...)`.
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
                click_log::log_load_ok(&self.source.describe(), title);
                click_log::log_page_ready(&self.source.describe(), self.scroll_y);
                self.record_render_health();
            }
            Err(err) => {
                self.nav_start = None;
                click_log::log_load_err(&self.source.describe(), &err.to_string());
                health_log::log_load_error(&self.source.describe(), &err.to_string());
                eprintln!("Ошибка reload {}: {err}", self.source.describe());
            }
        }
    }

    /// Попытаться загрузить страницу через GpuSession (WinitSession).
    /// Возвращает LoadedPage если успешно, иначе None (fallback к старому пути).
    ///
    /// Phase 4c: использует WinitSession::render_to_gpu() вместо inline pipeline
    /// для PageSource::File и PageSource::Url.
    pub(crate) fn reload_via_gpu_session(&mut self) -> Option<LoadedPage> {
        use lumen_driver::{WinitSession, GpuSession};

        // Преобразовать PageSource в URL для WinitSession
        let url = match &self.source {
            PageSource::File(path) => {
                format!("file://{}", path.display())
            }
            PageSource::Url(u) => u.clone(),
            _ => return None, // Snapshot и Empty обработаны отдельно
        };

        let viewport = self.renderer.as_ref().map_or_else(
            || Size::new(1024.0, 720.0),
            |r| {
                let s = r.viewport_size();
                Size::new(s.width, s.height)
            },
        );

        // Создать сессию с нужным viewport
        let mut session = WinitSession::with_viewport(viewport.width, viewport.height);

        // Загрузить страницу через WinitSession
        if session.navigate(&url).is_err() {
            return None;
        }

        // Получить RenderedPage через render_to_gpu()
        let rendered = match session.render_to_gpu() {
            Ok(r) => r,
            Err(_) => return None,
        };

        // Преобразовать RenderedPage в LoadedPage
        // Преобразовать lumen_driver::JsNavigateRequest в shell::JsNavigateRequest
        let js_navigate = rendered.js_navigate.map(|nav| {
            if nav.replace {
                JsNavigateRequest::Replace(nav.url)
            } else {
                JsNavigateRequest::Push(nav.url)
            }
        });

        Some(LoadedPage {
            display_list: rendered.display_list,
            title: rendered.title,
            // BUG-272 срез 17: driver's `RenderedPage.images` is still owned
            // `Image`; wrap at this boundary (driver path unchanged).
            images: rendered.images.into_iter().map(|(s, i)| (s, Arc::new(i))).collect(),
            animated_gifs: Vec::new(), // lumen-driver path has no animated GIF support yet
            lazy_pairs: Vec::new(), // Phase 4c: TODO integrate lazy loading
            layout_box: rendered.layout_box,
            // Driver path renders via lumen-driver which uses Arc<dyn FontProvider>;
            // shell's LoadedPage now requires Arc<FontRegistry> for PH3-19 dynamic
            // registration. Use an empty registry — driver pages have no async fonts.
            font_registry: Arc::new(lumen_font::FontRegistry::new()),
            pending_web_fonts: Vec::new(),
            js_navigate,
            page_tracks: tracks::PageTracks::default(),
            // lumen-driver рендерит через свой headless-пайплайн без
            // sub-документов — фреймов на этом пути нет.
            frames: Vec::new(),
            frame_env: None,
        })
    }

    /// Запустить background-поток загрузки текущего `source`.
    ///
    /// Поток fetches байты, затем:
    ///
    /// 1. Для каждого STREAM_CHUNK_BYTES-chunk: прогоняет через `PreloadScanner`
    ///    (PH1-8, HTML LS §13.2.6.4.7), отправляет `EarlyPreloadHints`, затем
    ///    `HtmlChunk`. Hint-ы эмитятся из **каждого** chunk-а, не только первого —
    ///    это даёт реальный выигрыш для stylesheet/шрифтов, стоящих за первыми 8 КБ.
    /// 2. PH1-2: для каждого `Stylesheet`-hint запускает параллельный CSS-загрузчик,
    ///    который присылает `CssLoaded` ещё до `LoadDone`.
    /// 3. По завершении — `LoadDone(raw)` для финального pipeline.
    ///
    /// При ошибке — `LoadError`.
    ///
    /// `generation` (U-1) метит каждое испускаемое событие; `user_event`
    /// отбрасывает события устаревшего поколения, если навигацию успели сменить.
    pub(crate) fn start_streaming_load(&self, generation: u64) {
        if matches!(self.source, PageSource::Empty | PageSource::AboutBlank) {
            return;
        }
        // BUG-171: scope the subresource prefetch cache to this navigation. Runs on
        // the UI thread before the streaming thread is spawned, so producer warm-ups
        // and the UI-thread consumer all observe `generation`.
        crate::prefetch::PREFETCH_CACHE.reset(generation);
        // BUG-172: scope the decoded-image cache to this navigation too, so the
        // streaming progressive loader and the final pipeline share one decode per
        // image and a superseded navigation's images are dropped.
        crate::image_cache::IMAGE_CACHE.reset(generation);
        let source = self.source.clone();
        let sink = Arc::clone(&self.event_sink);
        let proxy = self.load_proxy.clone();
        let cookie_jar = self.active_cookie_jar();
        // BUG-268: media-контекст экрана — для гейта speculative-фетча
        // `<link rel=stylesheet media=...>`, чтобы print-only лист не грел
        // кэш и не слал CssLoaded (progressive-кадры не красятся print-стилями).
        let media_ctx = {
            let viewport = self.renderer.as_ref().map_or_else(
                || Size::new(1024.0, 720.0),
                |r| {
                    let s = r.viewport_size();
                    Size::new(s.width, s.height)
                },
            );
            screen_media_context(viewport, self.dark_mode)
        };

        std::thread::spawn(move || {
            // PH1-8: инкрементальный preload-сканер — обрабатывает каждый chunk.
            // Hint-ы отправляются ДО соответствующего HtmlChunk, чтобы fetch
            // начался параллельно с DOM-парсингом (spec §13.2.6.4.7).
            let mut preload_scanner = lumen_html_parser::PreloadScanner::new();

            // PH1-2a: для URL-источников тело стримится прямо с сокета —
            // порции прилетают в `on_chunk` по мере чтения, не дожидаясь полной
            // загрузки. Для File/Snapshot/Static тело уже в памяти, поэтому его
            // достаточно нарезать на STREAM_CHUNK_BYTES (прежнее поведение).
            let raw = if let PageSource::Url(url) = &source {
                // BUG-757: база preload-хинтов — адрес, с которого РЕАЛЬНО
                // течёт тело (его приносит сам chunk), а не запрошенный: после
                // редиректа они разные, и относительный `src` уходил на
                // до-редиректный путь ещё до того, как документ получал
                // правильную базу. Пересобираем строку только при смене hop-а.
                let mut base = ResourceBase::Url(url.clone());
                let chunk_proxy = proxy.clone();
                // Separate clones for prefetch warm-up: `cookie_jar`/`sink` below are
                // moved into the streaming call, so the per-chunk closure keeps its own.
                let cj_prefetch = Some(Arc::clone(&cookie_jar));
                let sink_prefetch = Arc::clone(&sink);
                let mut on_chunk = |chunk: &[u8], hop_url: &lumen_core::url::Url| {
                    if !matches!(&base, ResourceBase::Url(u) if u == hop_url.as_str()) {
                        base = ResourceBase::Url(hop_url.to_string());
                        // UI-поток резолвит картинки/шрифты частичного DOM от
                        // своей копии базы — сообщаем ему новую (BUG-757).
                        let _ = chunk_proxy
                            .send_event(LoadEvent::DocumentBase(base.clone(), generation));
                    }
                    feed_preload_and_emit(
                        &mut preload_scanner,
                        chunk,
                        &base,
                        &chunk_proxy,
                        generation,
                        &sink_prefetch,
                        cj_prefetch.as_ref(),
                        &media_ctx,
                    );
                    let _ = chunk_proxy.send_event(LoadEvent::HtmlChunk(chunk.to_vec(), generation));
                };
                match source.load_bytes_streaming(Arc::clone(&sink), Some(cookie_jar), &mut on_chunk) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = proxy.send_event(LoadEvent::LoadError(e.to_string(), generation));
                        return;
                    }
                }
            } else {
                let cj_prefetch = Some(Arc::clone(&cookie_jar));
                let raw = match source.load_bytes(Arc::clone(&sink), Some(cookie_jar)) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = proxy.send_event(LoadEvent::LoadError(e.to_string(), generation));
                        return;
                    }
                };
                let mut pos = 0;
                while pos < raw.bytes.len() {
                    let end = (pos + STREAM_CHUNK_BYTES).min(raw.bytes.len());
                    let chunk = &raw.bytes[pos..end];
                    feed_preload_and_emit(
                        &mut preload_scanner,
                        chunk,
                        &raw.base,
                        &proxy,
                        generation,
                        &sink,
                        cj_prefetch.as_ref(),
                        &media_ctx,
                    );
                    if proxy.send_event(LoadEvent::HtmlChunk(chunk.to_vec(), generation)).is_err() {
                        return; // event loop завершён
                    }
                    pos = end;
                }
                raw
            };

            // Финальные hint-ы из буферизованного хвоста сканера.
            let tail = preload_scanner.end();
            if !tail.is_empty() {
                let _ = proxy.send_event(LoadEvent::EarlyPreloadHints(tail, raw.base.clone(), generation));
            }

            let _ = proxy.send_event(LoadEvent::LoadDone(raw, generation));
        });
    }

    /// Обновить display list на основе снапшота частичного DOM.
    /// Применяет `stream_sheet` — CSS, загруженный параллельными потоками (PH1-2).
    pub(crate) fn paint_partial_dom(&mut self, doc: &lumen_dom::Document) {
        let Some(renderer) = self.renderer.as_ref() else { return };
        let vp_size = renderer.viewport_size();
        let viewport = Size::new(vp_size.width, vp_size.height);

        let font = match lumen_font::Font::parse(INTER_FONT) {
            Ok(f) => f,
            Err(_) => return,
        };
        let measurer = match lumen_paint::FontMeasurer::new(&font) {
            Ok(m) => m,
            Err(_) => return,
        };

        // PH1-2b: после первого («засевающего») кадра релейаутим инкрементально —
        // переиспользуем геометрию неизменённого префикса из прошлого кадра,
        // релейаутим только новые/изменённые поддеревья. Полный layout всего
        // частичного DOM на каждый 16-мс тик тормозил большие страницы.
        let null_hp = lumen_core::ext::NullHyphenationProvider;
        let layout = match (self.stream_layout_seeded, self.layout_box.as_ref()) {
            (true, Some(prev)) => lumen_layout::layout_streaming_incremental(
                doc, &self.stream_sheet, viewport, &measurer, &null_hp, false, prev,
            ),
            _ => lumen_layout::layout_measured(doc, &self.stream_sheet, viewport, &measurer),
        };
        let dl = paint_ordered(&layout);

        self.content_height = content_height_of(&dl);
        self.content_width = content_width_of(&dl);
        self.set_display_list(dl);
        // BUG-341 S7: streaming layout is a separate incremental mechanism
        // (`layout_streaming_incremental`, no `CounterMap`) — invalidate the
        // restyle cache so it isn't diffed against this tree by mistake.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(layout);
        self.stream_layout_seeded = true;
        self.update_snap_containers();
        self.update_scroll_containers();

        // PH1-2c: запустить параллельную загрузку картинок, появившихся в этом
        // частичном DOM, чтобы они дорисовывались по мере прихода (как CSS),
        // а не разом в финальном `LoadDone`.
        self.spawn_stream_image_loads(doc, viewport);

        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// PH1-2c: найти `<img>` в частичном streaming-DOM и запустить параллельные
    /// потоки fetch+decode для ещё-не-запрошенных картинок. По завершении поток
    /// шлёт `LoadEvent::ImageDecoded`, и `user_event` регистрирует картинку в
    /// renderer-е + просит redraw. Дедуп через `stream_images_requested`, так что
    /// каждый `src` грузится один раз за навигацию; `loading="lazy"` пропускается
    /// (грузится по близости к viewport уже после `LoadDone`).
    pub(crate) fn spawn_stream_image_loads(&mut self, doc: &lumen_dom::Document, viewport: Size) {
        let requests = lumen_layout::collect_image_requests(doc, viewport);
        self.spawn_image_requests(requests);
    }

    /// BUG-730: same pass over the **live** document, run after every relayout
    /// that applied a DOM mutation.
    ///
    /// [`Self::spawn_stream_image_loads`] only ever sees the partial DOM the
    /// HTML streamer hands it, i.e. markup that arrived over the wire. An
    /// `<img>` that a script appends (or an existing one whose `src` a script
    /// rewrites) after the load finished was therefore never fetched at all —
    /// no request, no decode, and paint drew the grey placeholder box forever.
    /// That is the normal way a client-rendered page shows pictures: on
    /// `tbank.ru` not one of its 33 images was ever requested.
    ///
    /// Dedup still goes through `stream_images_requested`, so an image already
    /// fetched during streaming is not re-fetched, and a URL is requested once
    /// per navigation no matter how many relayouts see it.
    pub(crate) fn spawn_dynamic_image_loads(&mut self, viewport: Size) {
        let requests = {
            let Some(src) = self.layout_source.as_ref() else { return };
            let Ok(doc) = src.document.lock() else { return };
            lumen_layout::collect_image_requests(&doc, viewport)
        };
        self.spawn_image_requests(requests);
    }

    /// BUG-735: разнести intrinsic-размеры уже декодированных картинок по `<img>`
    /// живого документа и, если DOM от этого изменился, запросить релейаут.
    ///
    /// Третий путь загрузки картинок — streaming/динамический
    /// ([`Self::spawn_image_requests`]) — регистрировал пиксели в рендерере, но
    /// никогда не сообщал размер DOM-у: `apply_intrinsic_size` звали только
    /// финальный `fetch_and_decode_images` и lazy-путь. Для клиентски
    /// отрисованной страницы (где к моменту появления `<img>` финальный pipeline
    /// уже отработал — BUG-730) это значит **все** картинки: без intrinsic-пары
    /// нет и соотношения сторон, поэтому `height: auto` даёт ноль.
    ///
    /// Проход коалесцирован: арм `ImageDecoded` только копит размеры и взводит
    /// флаг, а разнос+релейаут делается один раз за кадр — сотня декодов стоит
    /// одного релейаута, а не сотни. Сходимость держится на том, что
    /// `apply_intrinsic_size` возвращает `false`, когда дописывать нечего:
    /// второй проход по тем же узлам DOM не меняет и релейаут не заказывает.
    /// Петли «релейаут → новый запрос → новый декод» нет — `spawn_image_requests`
    /// дедуплицируется через `stream_images_requested`.
    pub(crate) fn apply_stream_intrinsic_sizes(&mut self) {
        if !self.stream_image_sizes_dirty {
            return;
        }
        self.stream_image_sizes_dirty = false;
        let Some(viewport) = self.relayout_viewport() else {
            // Вьюпорта ещё нет (рендерер не сконфигурирован) — размеры остаются
            // в карте, проход повторится на кадре, когда он появится.
            self.stream_image_sizes_dirty = true;
            return;
        };
        let changed = {
            let Some(src) = self.layout_source.as_ref() else { return };
            let Ok(mut doc) = src.document.lock() else { return };
            // Тот же picker, что эмитит ключи `src` в `DrawImage`, — url из
            // запроса совпадает с ключом карты по построению.
            let requests = lumen_layout::collect_image_requests(&doc, viewport);
            let mut changed = false;
            for req in requests {
                if req.is_lazy {
                    continue;
                }
                let Some(&(w, h)) = self.stream_image_sizes.get(&req.url) else {
                    continue;
                };
                changed |= apply_intrinsic_size(&mut doc, req.node_id, w, h);
            }
            changed
        };
        if !changed {
            return;
        }
        // Дописанные `width`/`height` — презентационный хинт, то есть вход
        // каскада. Кэш инкрементального рестайла (BUG-341) знает только о
        // мутациях, пришедших из JS, поэтому мутацию со стороны шелла ему нужно
        // объявить сбросом кэша — иначе стиль `<img>` переиспользуется прежний.
        self.page_prev_cascade_styles = None;
        self.relayout_raf_dirty();
    }

    /// База для разрешения относительных подресурсов текущего документа.
    ///
    /// BUG-757: это НЕ `self.source.resource_base()` — там лежит запрошенный
    /// адрес, а после серверного редиректа документ приехал с другого, и от
    /// него же обязаны резолвиться его картинки, шрифты и ссылки. Реальная база
    /// приходит из загрузчика (`LoadEvent::DocumentBase`) и годится только для
    /// той навигации, в которой получена, — отсюда сверка generation. Событие
    /// шлётся ровно при расхождении адресов, поэтому без редиректа (и для
    /// несетевых источников) ответ ровно прежний.
    pub(crate) fn document_resource_base(&self) -> Option<ResourceBase> {
        match &self.document_base {
            Some((base, generation)) if *generation == self.load_generation => Some(base.clone()),
            _ => self.source.resource_base(),
        }
    }

    /// Fetch+decode every not-yet-requested non-lazy image in `requests` on its
    /// own thread, reporting back through `LoadEvent::ImageDecoded`. Shared by
    /// the streaming ([`Self::spawn_stream_image_loads`]) and post-load
    /// ([`Self::spawn_dynamic_image_loads`]) producers.
    pub(crate) fn spawn_image_requests(&mut self, requests: Vec<lumen_layout::ImageRequest>) {
        let Some(base) = self.document_resource_base() else { return };
        // BUG-172: stamp the decode with this navigation's generation so the cache
        // entry is shared with the final pipeline pass (same generation) and a
        // stale producer from a superseded navigation bypasses the cache.
        let generation = self.load_generation;
        for req in requests {
            if req.is_lazy {
                continue;
            }
            if !self.stream_images_requested.insert(req.url.clone()) {
                continue;
            }
            let base = base.clone();
            let sink = Arc::clone(&self.event_sink);
            let cookie_jar = self.active_cookie_jar();
            let proxy = self.load_proxy.clone();
            let target = self.target_color_space();
            std::thread::spawn(move || {
                // Fill the shared cache so the final `fetch_and_decode_images` pass
                // reuses these pixels instead of re-fetching+re-decoding (BUG-172).
                let decoded = image_cache::IMAGE_CACHE.get_or_decode(generation, &req.url, || {
                    decode_image(&req.url, &base, &sink, Some(cookie_jar), target)
                });
                match decoded {
                    // streaming best-effort: финальный pipeline залогирует/применит.
                    None => {}
                    Some(image_cache::DecodedImage::Static(img)) => {
                        let _ = proxy.send_event(LoadEvent::ImageDecoded {
                            src: req.url,
                            image: Box::new((*img).clone()),
                            animated: None,
                        });
                    }
                    Some(image_cache::DecodedImage::Animated { first, gif }) => {
                        let _ = proxy.send_event(LoadEvent::ImageDecoded {
                            src: req.url,
                            image: Box::new((*first).clone()),
                            animated: Some(Box::new((*gif).clone())),
                        });
                    }
                }
            });
        }
    }

    /// PERF-6: emit a broken-render / white-screen health signal for the page
    /// that just finished loading. No-op unless the health journal is enabled.
    /// The signal fires only when a content-bearing DOM painted nothing at all
    /// (see [`health_log::log_render_health`] for the heuristic and its limits).
    pub(crate) fn record_render_health(&self) {
        if !health_log::is_enabled() {
            return;
        }
        let layout_boxes = self.layout_box.as_ref().map(count_layout_boxes).unwrap_or(0);
        let rendered_units = self
            .layout_box
            .as_ref()
            .map(count_rendered_units)
            .unwrap_or(0);
        let dom_nodes = self
            .layout_source
            .as_ref()
            .and_then(|ls| ls.document.lock().ok().map(|d| d.node_count()))
            .unwrap_or(0);
        health_log::log_render_health(
            &self.source.describe(),
            dom_nodes,
            layout_boxes,
            rendered_units,
        );
    }

    /// Применить результат полного pipeline (fetch + parse + CSS + images).
    /// Используется и при streaming `LoadDone`, и может быть переиспользован
    /// в будущем для других путей загрузки.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn apply_loaded_page(&mut self, page: LoadedPage, new_layout_source: Option<LayoutSource>, new_js_ctx: Option<Arc<dyn PersistentJs>>) {
        // Drop JS closures before layout_source to release Arc clones in QuickJS.
        self.set_js_ctx(None);
        self.layout_source = new_layout_source;
        self.set_js_ctx(new_js_ctx);
        // ADR-016 M2.2c-2b: зеркалим новый хэндл + DOM в состояние движкового
        // потока (no-op при выключенном `LUMEN_ENGINE_THREAD`).
        self.sync_engine_js_state();
        // The new runtime starts empty; re-seed it with the current Navigation state.
        self.commit_nav_state();
        // Cross-document unification (see `pending_post_reload_traversal`): a
        // multi-step traversal landed on a same-document entry of the document
        // that just finished loading — apply its popstate/URL update now, on
        // top of the fresh runtime `commit_nav_state` just seeded.
        if let Some((state_json, display_url)) = self.pending_post_reload_traversal.take() {
            self.apply_post_reload_traversal(state_json, display_url);
        }
        self.content_height = content_height_of(&page.display_list);
        self.content_width = content_width_of(&page.display_list);
        // Full page load: force all tiles dirty.
        self.tile_grid.mark_all_dirty(self.content_width, self.content_height);
        self.animation_scheduler.clear();
        self.transition_scheduler = TransitionScheduler::new();
        self.starting_style_tracker = StartingStyleTracker::new();
        self.prev_styles.clear();
        collect_box_styles(&page.layout_box, &mut self.prev_styles);
        self.layout_box = Some(page.layout_box);
        self.page_tracks = page.page_tracks;
        // BUG-480 срез 1: под-документы новой страницы заменяют старые целиком —
        // прежние фреймы (и их JS-контексты) падают вместе со страницей.
        self.frames = page.frames;
        self.frame_env = page.frame_env;
        // FRAME-4 срез 3: слоты старой страницы адресуют документы, которых
        // больше нет — новый ответ на них уже не придёт (generation этого
        // хозяина у следующей страницы начнётся заново), а старый, если
        // всё ещё летит, и так уйдёт в `apply_frame_navigation`'s "old_doc
        // отсутствует" ветку; чистка здесь — только против утечки памяти.
        frames::clear_frame_nav_requests(&mut self.frame_nav_requests);
        // BUG-480 срез 16: см. тот же сброс в резервном пути reload'а выше.
        self.hovered_frame = None;
        // BUG-480 срез 14: список страницы пишется ПОСЛЕ замены фреймов, а не
        // до неё — `set_display_list` вклеивает в него содержимое под-документов
        // из `self.frames`, и на прежнем порядке первый кадр новой страницы
        // склеивался с фреймами ПРЕДЫДУЩЕЙ (а на первой загрузке — ни с чем,
        // так что фрейм оставался серой заглушкой до первого relayout).
        self.set_display_list(page.display_list);
        self.sync_text_track_store();
        // content-visibility: auto (BB-4): новая страница — ratchet с нуля.
        self.cv_relevant.clear();
        self.cv_events.clear();
        self.cv_skipped.clear();
        self.cv_auto_state.clear();
        self.refresh_cv_state();
        self.update_snap_containers();
        self.update_scroll_containers();
        // BUG-382: publish the primary layout's geometry + computed styles into the
        // JS runtime unconditionally, right here, before any page-visible callback
        // of this load can run (`load`/`pageshow` below, and every timer/rAF/promise
        // job the shell drains afterwards).
        //
        // `getComputedStyle()` and `getBoundingClientRect()` do not query the layout
        // engine — both read the snapshot the shell pushes. Until this call the only
        // pushes lived inside the relayout path (`relayout()`, `reload()`) and inside
        // the lazy-image block below, which collects geometry **only** when the page
        // has `loading="lazy"` images and never pushes computed styles at all. A
        // freshly loaded page therefore answered `""` / all-zeros unless some
        // unrelated relayout (resize, font swap, scroll) happened to race ahead of
        // the first script — the reported "works in one load out of four".
        //
        // ADR-016 M2.2c-2d: routed through `route_task_js` like the other seeds; the
        // owned `HashMap`s make the closure `Send + 'static`, and the `js_present`
        // gate keeps the (side-effect-free) collection JS-gated.
        #[cfg(feature = "v8")]
        if self.js_present
            && let Some(lb_ref) = self.layout_box.as_ref()
        {
            let viewport = self.renderer.as_ref().map_or_else(
                || Size::new(1024.0, 720.0),
                |r| {
                    let s = r.viewport_size();
                    Size::new(s.width, s.height)
                },
            );
            let rects = collect_layout_rects(lb_ref);
            let hit_test_tree = Arc::new(lb_ref.clone());
            let styles = collect_computed_styles(lb_ref);
            let customs = collect_custom_properties(lb_ref);
            let (vw, vh) = (viewport.width, viewport.height);
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.update_layout_rects(rects);
                js.update_hit_test_tree(hit_test_tree);
                js.update_computed_styles(styles);
                js.update_custom_properties(customs);
                js.update_viewport_size(vw, vh);
            });
        }
        self.title = page.title.clone();
        if let Some(t) = &self.title {
            self.tab_strip.set_active_title(t.as_str());
        }
        self.anim_frame = None;
        self.find.close();
        self.address_bar.close();
        // U-1: новая страница встаёт сверху-слева; но back/forward (и bfcache)
        // просят восстановить прежний scroll-offset через `pending_restore_scroll`,
        // т.к. навигация теперь асинхронна и сброс происходит здесь, в LoadDone,
        // а не сразу после `reload()`. Координаты докламплятся при первом redraw.
        let (restore_x, restore_y) = self.pending_restore_scroll.take().unwrap_or((0.0, 0.0));
        self.scroll_x = restore_x;
        self.scroll_y = restore_y;
        self.scroll_drag = None;
        self.frame_scroll_drag = None;
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.forward_momentum_stop();
        self.touchpad_vel = (0.0, 0.0);
        self.form_state.clear();
        self.frame_text_cursor.clear();
        self.frame_text_selection_anchor.clear();
        self.text_drag = None;
        self.validation_tooltip = None;
        self.color_picker_node = None;
        self.date_picker_node = None;
        self.date_picker_year = 0;
        self.date_picker_month = 0;
        self.select_dropdown_node = None;
        // Reset paint timing guards so new page fires fresh PerformancePaintTiming entries.
        self.first_paint_delivered = false;
        self.first_contentful_paint_delivered = false;
        // A page was applied successfully — clear any prior settled-error flag
        // (BUG-308) so `document_ready` reflects this real load, not a stale one.
        self.load_failed = false;
        self.load_error_message = None;

        // Индексировать страницу в history_fts для omnibox (@history) и записать
        // в history_store для панели истории (Ctrl+H).
        // Пропускаем Empty и File sources — только HTTP(S) и bfcache snapshots.
        // DS-16: while Anonymous is the active profile, skip both writes —
        // an ephemeral session must leave no trace in the history a
        // Personal/Work switch-back would then see (ADR-020).
        if let Some(url) = self.source.url_str()
            && !self.active_profile_is_anonymous()
        {
            let title = page.title.as_deref().unwrap_or("");
            let _ = self.history_fts.index(self.next_history_id, url, title, "");
            self.next_history_id += 1;
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let _ = self.history_store.record_visit(url, title, now_secs);
        }
        // Clear GIF animation state from previous page.
        self.animated_gifs.clear();
        self.gif_last_frame.clear();
        // Populate animated GIFs from new page; reset frame tracking.
        for (url, gif) in page.animated_gifs {
            self.animated_gifs.insert(url, gif);
        }

        // Clear video GIF state from previous page.
        self.video_gif_store.playback.lock().unwrap().clear();
        self.video_gif_store.pending_loads.lock().unwrap().clear();
        self.video_gif_last_frame.clear();
        self.video_gif_frames.clear();

        // Update shields panel domain and clear per-page blocked counts.
        {
            let domain = self.source.url_str().and_then(|u| {
                // Extract hostname from the loaded URL for the shields panel.
                let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"))?;
                let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                let host = &rest[..host_end];
                let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
                if host.is_empty() { None } else { Some(host.to_ascii_lowercase()) }
            });
            self.shields.clear_log();
            self.shields.set_domain(domain);
            // BUG-411: the new host may carry its own shields exception —
            // re-point the live filter at it.
            self.sync_adblock_filter();
        }

        // Clear the network panel log so each page starts with a fresh request list.
        self.network_panel.clear_log();

        // Update permission panel origin on navigation.
        {
            let origin = self.source.url_str().and_then(|u| {
                // Build bare origin (scheme + host) for permission keying.
                let scheme_end = u.find("://")?;
                let scheme = &u[..scheme_end + 3];
                let rest = &u[scheme_end + 3..];
                let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                let host = &rest[..host_end];
                let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
                if host.is_empty() { None } else { Some(format!("{}{}", scheme, host.to_ascii_lowercase())) }
            });
            self.permission.set_origin(origin);
        }

        // PH3-19: store the page's FontRegistry for dynamic web-font registration
        // via FontLoaded events. Also clear previous page's web_fonts list so
        // relayout_with_web_fonts uses only fonts for the current page.
        self.page_font_registry = page.font_registry.clone();
        self.web_fonts.clear();

        // PH3-19: spawn one background thread per pending @font-face url() source.
        // Each thread fetch+decodes the font and sends FontLoaded; the handler
        // registers it in page_font_registry, rebuilds MultiFontMeasurer, and
        // triggers a relayout — FOUT (Flash Of Unstyled Text) swap pattern.
        if !page.pending_web_fonts.is_empty() {
            let base_opt = self.document_resource_base();
            for pf in page.pending_web_fonts {
                if let Some(base) = base_opt.clone() {
                    let sink = Arc::clone(&self.event_sink);
                    let cookie_jar = self.active_cookie_jar();
                    let proxy = self.load_proxy.clone();
                    std::thread::spawn(move || {
                        let raw = match fetch_image_bytes(&pf.url, &base, &sink, Some(cookie_jar)) {
                            Ok(b) => b,
                            Err(e) => {
                                eprintln!("@font-face «{}»: не загружен {}: {e}", pf.family, pf.url);
                                return;
                            }
                        };
                        let bytes = match lumen_font::maybe_decode_font(&raw) {
                            Ok(Some(d)) => d,
                            Ok(None) => raw,
                            Err(e) => {
                                eprintln!("@font-face «{}»: WOFF-декод провалился: {e}", pf.family);
                                return;
                            }
                        };
                        if lumen_font::Font::parse(&bytes).is_err() {
                            eprintln!("@font-face «{}»: невалидный sfnt {}", pf.family, pf.url);
                            return;
                        }
                        eprintln!("@font-face async загружен: «{}» weight={}", pf.family, pf.weight);
                        let unicode_range = pf.unicode_range_str
                            .as_deref()
                            .map(lumen_font::parse_unicode_ranges)
                            .unwrap_or_default();
                        let _ = proxy.send_event(LoadEvent::FontLoaded {
                            family: pf.family,
                            weight: pf.weight,
                            style: pf.style,
                            unicode_range,
                            bytes,
                        });
                    });
                }
            }
        }

        // Reset CPU image cache for the new page (10E.4 scroll-discard).
        self.image_cache.clear();
        if let Some(r) = self.renderer.as_mut() {
            r.set_font_provider(Some(Arc::clone(&page.font_registry) as Arc<dyn lumen_core::FontProvider>));
            // Warm the curated system-font fallback chain once, now that a
            // FontProvider (this page's FontRegistry, which wraps the system
            // font index) is available. Loads emoji / CJK / RTL / Indic / Thai
            // faces into the renderer so the codepoint cascade can resolve
            // glyphs Inter lacks. One-time: the faces persist across pages and
            // the curated families are system fonts identical for every page.
            if !self.fallbacks_preloaded {
                r.preload_curated_fallbacks();
                self.fallbacks_preloaded = true;
            }
            r.clear_images();
            for (src, image) in &page.images {
                // BUG-272 срез 17: share the Arc; raw_images no longer deep-copies.
                if let Err(err) = r.register_image(src.clone(), Arc::clone(image)) {
                    eprintln!("Картинка {src} не зарегистрирована: {err}");
                }
                self.image_cache.insert(lumen_image::ImageKey::new(src), (**image).clone());
            }
        } else {
            self.pending_images = page.images;
        }
        if let Some(w) = self.window.as_ref() {
            w.set_title(&window_title(self.title.as_deref()));
            w.request_redraw();
        }
        // Register lazy images with JS so _lumen_deliver_lazy_images can check them
        // on subsequent redraws (scroll, resize) via proximity threshold.
        //
        // After registration we run an immediate proximity check: push fresh
        // layout rects into JS, fire the IntersectionObserver, drain and fetch.
        // Without this, above-the-fold `loading="lazy"` images (most cards on
        // sites like lenta.ru) never load on first paint — `relayout()` is the
        // only other path that delivers observers, and it only runs on
        // scroll/resize/zoom, not on the initial load. (BUG-163)
        // ADR-016 M2.2c-2d: lazy-image регистрация + immediate proximity check через
        // `route_query_js` — снимаем прямое `self.js_ctx`-обращение. Вся упорядоченная
        // последовательность (register → push rects/viewport → deliver observers →
        // deliver lazy → drain requests) обёрнута в **один** `route_query_js`, чтобы под
        // флагом (`LUMEN_ENGINE_THREAD=1`) она исполнилась атомарно **в порядке** на
        // движковом потоке (value-read `take_lazy_image_requests` после void-push
        // сохраняет read-after-write), блокируя лишь ради одного результата. Owned-данные
        // (`owned_pairs`/`geom`) собираются на UI-потоке до маршрутизации (замыкание
        // `Send + 'static`); гейт `self.js_present` держит сбор геометрии
        // JS-гейтнутым — байт-идентично флаг-офф (`route_query_js(…, Some(js), …)` =
        // синхронный вызов по UI-хэндлу).
        #[cfg(feature = "v8")]
        let initial_lazy_reqs: Vec<(u32, String)> = if self.js_present {
            let owned_pairs: Vec<(u32, String)> =
                page.lazy_pairs.iter().map(|(n, u)| (*n, u.clone())).collect();
            type LazyImageGeom = (HashMap<u32, [f32; 4]>, Arc<lumen_layout::LayoutBox>, f32, f32);
            let geom: Option<LazyImageGeom> = if !owned_pairs.is_empty() {
                self.layout_box.as_ref().map(|lb_ref| {
                    let viewport = self.renderer.as_ref().map_or_else(
                        || Size::new(1024.0, 720.0),
                        |r| {
                            let s = r.viewport_size();
                            Size::new(s.width, s.height)
                        },
                    );
                    (collect_layout_rects(lb_ref), Arc::new(lb_ref.clone()), viewport.width, viewport.height)
                })
            } else {
                None
            };
            route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                let pairs: Vec<(u32, &str)> =
                    owned_pairs.iter().map(|(n, u)| (*n, u.as_str())).collect();
                js.register_lazy_images(&pairs);
                if let Some((rects, hit_test_tree, vw, vh)) = geom {
                    js.update_layout_rects(rects);
                    js.update_hit_test_tree(hit_test_tree);
                    js.update_viewport_size(vw, vh);
                    js.deliver_layout_observers();
                    js.deliver_lazy_images();
                    return js.take_lazy_image_requests();
                }
                Vec::new()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        #[cfg(feature = "v8")]
        if !initial_lazy_reqs.is_empty() {
            self.fetch_and_register_lazy_images(initial_lazy_reqs);
            // Images were registered after the request_redraw above — request
            // another so the first paint actually shows them.
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }
        // JS may have requested navigation via location.href= etc.
        self.pending_js_navigate = page.js_navigate;
        // HTML LS §8.2.3 — all resources loaded: readyState → "complete" + window.load event.
        // HTML LS §8.6 — `pageshow` fires right after `load`. `persisted=true`
        // only when this page was restored from bfcache (set by navigate_back/
        // navigate_forward); a fresh load fires `persisted=false`.
        let pageshow_persisted = std::mem::take(&mut self.pending_pageshow_persisted);
        // ADR-016 M2.2c-2d: pageshow-lifecycle void-вызовы через `route_task_js` —
        // снимаем прямое `self.js_ctx`-обращение. Оба чистый fire-and-forget без
        // синхронного чтения результата следом; под флагом (`LUMEN_ENGINE_THREAD=1`)
        // уходят off-UI-thread одним `task` (порядок сохранён), без флага (по
        // умолчанию) — синхронный вызов по UI-хэндлу, байт-идентично прежнему.
        #[cfg(feature = "v8")]
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            js.notify_window_loaded();
            js.fire_page_lifecycle("pageshow", pageshow_persisted);
        });
        #[cfg(not(feature = "v8"))]
        let _ = pageshow_persisted;

        // Rebuild accessibility tree and push to OS platform bridge (O-5).
        self.update_platform_ax_tree();

        // If zoom or <meta viewport initial-scale> is active, relayout with the
        // correct effective viewport. The initial load used the raw physical size.
        let zoom = self.zoom_factor;
        let meta_scale = self.layout_source.as_ref().map(meta_initial_scale).unwrap_or(1.0);
        if (zoom - 1.0).abs() > 0.001 || (meta_scale - 1.0).abs() > 0.001 {
            self.relayout();
        }
    }
}

/// Прогнать порцию HTML через preload-сканер, эмитнуть `EarlyPreloadHints` и
/// параллельно загрузить найденные стили (PH1-2 / PH1-8). Общая логика для обоих
/// путей `start_streaming_load`: сетевого streaming-а (URL) и нарезки
/// уже-загруженного буфера (File/Snapshot/Static). Hint-ы шлются ДО передачи
/// chunk-а DOM-парсеру, чтобы fetch подресурсов стартовал раньше.
#[allow(clippy::too_many_arguments)]
fn feed_preload_and_emit(
    scanner: &mut lumen_html_parser::PreloadScanner,
    chunk: &[u8],
    base: &ResourceBase,
    proxy: &EventLoopProxy<LoadEvent>,
    generation: u64,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<&Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
) {
    use lumen_network::RequestDestination;

    let early = scanner.feed_bytes(chunk);
    if early.is_empty() {
        return;
    }
    let _ = proxy.send_event(LoadEvent::EarlyPreloadHints(early.clone(), base.clone(), generation));
    // PH1-2 + BUG-171: speculatively fetch subresources off the UI thread while the
    // HTML is still streaming. Linked stylesheets AND external classic scripts are
    // warmed into the process-global prefetch cache using the SAME subresource
    // client `parse_and_layout` uses, so the final UI-thread pass reads identical
    // bytes instantly instead of blocking on the socket (cascade + script order
    // untouched — a cache miss simply re-fetches there). Stylesheets additionally
    // parse here to feed progressive intermediate frames (the previous PH1-2 path).
    for hint in &early {
        let (raw_url, dest, is_css) = match hint {
            lumen_html_parser::PreloadHint::Stylesheet { url, media } => {
                // BUG-268: print-only лист финальный pipeline всё равно не
                // возьмёт (media-гейт в collect_link_hrefs) — не греем кэш
                // и, главное, не эмитим CssLoaded: промежуточные progressive-
                // кадры не должны красить страницу print-правилами.
                if media.as_deref().is_some_and(|m| !link_media_matches(m, media_ctx)) {
                    continue;
                }
                (url, RequestDestination::Style, true)
            }
            lumen_html_parser::PreloadHint::Script { url } => {
                (url, RequestDestination::Script, false)
            }
            _ => continue,
        };
        match base.resolve(raw_url) {
            // Local files: read is instant — no cache benefit. Only CSS needs a
            // CssLoaded event for the progressive frame; scripts are read in
            // `parse_and_layout`.
            ResolvedResource::File(path) => {
                if is_css
                    && let Ok(text) = std::fs::read_to_string(&path)
                {
                    let sheet = lumen_css_parser::parse(&text);
                    let _ = proxy.send_event(LoadEvent::CssLoaded(Box::new(sheet), generation));
                }
            }
            ResolvedResource::Url(resolved) => {
                let proxy2 = proxy.clone();
                let base = base.clone();
                let sink = Arc::clone(sink);
                let cookie_jar = cookie_jar.cloned();
                std::thread::spawn(move || {
                    use lumen_core::url::Url;
                    let Ok(parsed) = Url::parse(&resolved) else {
                        return;
                    };
                    let bytes = crate::prefetch::PREFETCH_CACHE.fetch(generation, &resolved, || {
                        let client = base.http_client_for_subresource(sink, cookie_jar);
                        client
                            .fetch_subresource(&parsed, dest)
                            .map_err(|e| e.to_string())
                    });
                    if is_css
                        && let Ok(bytes) = bytes
                    {
                        let sheet =
                            lumen_css_parser::parse(&String::from_utf8_lossy(&bytes[..]));
                        let _ = proxy2.send_event(LoadEvent::CssLoaded(Box::new(sheet), generation));
                    }
                });
            }
        }
    }
}

/// Событие от background-потока загрузки страницы в event loop.
///
/// Загрузка разбита на четыре фазы: (0) `EarlyPreloadHints` — хинты из первых
/// байт HTML для раннего старта subresource fetch-ов; (1) chunks сырых байт для
/// инкрементального парсинга и промежуточных кадров через
/// `IncrementalTreeBuilder::feed_bytes`; (2) `LoadDone` — все байты доступны,
/// запускаем полный pipeline (CSS + изображения); (3) `LoadError` — ошибка fetch.
pub(crate) enum LoadEvent {
    /// No-op wake-up (SDC-2). `winit`'s `ControlFlow::Wait` genuinely parks
    /// the event loop until an OS window event, a scheduled `WaitUntil`
    /// deadline, or a proxied user event arrives — an `AutomationCommand`
    /// enqueued from a BiDi/MCP thread is none of those, so without this the
    /// loop could sit parked indefinitely and never drain it.
    /// `AutomationHandle::execute` sends this through `load_proxy` right
    /// after queuing a command; `user_event` below does nothing with it —
    /// merely *receiving* a proxied event is what interrupts `Wait` and
    /// triggers the next `about_to_wait` (where automation commands are
    /// actually drained).
    AutomationWake,
    /// Subresource-хинты из первого chunk HTML (HTML LS §13.2.6.4.7
    /// «Speculative HTML parsing»). Отправляются ДО первого `HtmlChunk`,
    /// чтобы sink мог начать загружать CSS/шрифты ещё в процессе парсинга.
    /// Дедупликация с финальными хинтами из `LoadDone` — через
    /// `preload_dispatched` в `Lumen`.
    /// Последнее поле — generation навигации (U-1): идентификатор load-цикла,
    /// присвоенный в `reload`/`resumed`. `user_event` отбрасывает событие, если
    /// его generation не совпадает с `Lumen::load_generation` — защита от
    /// устаревших событий гонки навигаций (быстрый back/forward или клик по двум
    /// ссылкам подряд), которые иначе подмешали бы DOM/CSS прошлой страницы.
    EarlyPreloadHints(Vec<lumen_html_parser::PreloadHint>, ResourceBase, u64),
    /// BUG-757: база документа стала известна и отличается от запрошенного
    /// адреса (сервер ответил редиректом). Отправляется из streaming-потока,
    /// как только тело потекло с финального hop-а — то есть ДО того, как
    /// частичный DOM начнёт заказывать картинки и шрифты, которые UI-поток
    /// резолвит относительно базы. Последнее поле — generation навигации (U-1).
    DocumentBase(ResourceBase, u64),
    /// Очередной chunk сырых байт HTML. UTF-8 границы не выравниваются —
    /// `IncrementalTreeBuilder::feed_bytes` буферизует незавершённые
    /// code-point-ы внутри. Последнее поле — generation навигации (U-1).
    HtmlChunk(Vec<u8>, u64),
    /// CSS загружен параллельным потоком для промежуточных streaming-кадров.
    /// Мёрджится в `Lumen::stream_sheet` и применяется в `paint_partial_dom`.
    /// Последнее поле — generation навигации (U-1).
    CssLoaded(Box<lumen_css_parser::Stylesheet>, u64),
    /// PH1-2c: картинка `<img>` декодирована параллельным потоком во время
    /// streaming. Регистрируется в renderer-е по ключу `src` и вызывает redraw —
    /// картинки появляются по мере прихода, а не разом в финальном `LoadDone`.
    /// Для анимированного GIF `animated` несёт все кадры (тикаются в
    /// `RedrawRequested`); `image` — нулевой кадр для немедленной отрисовки.
    ImageDecoded {
        src: String,
        image: Box<lumen_image::Image>,
        animated: Option<Box<lumen_image::AnimatedGif>>,
    },
    /// PH3-19: web-шрифт из @font-face url() декодирован в фоновом потоке.
    /// Регистрируется в FontRegistry + MultiFontMeasurer и вызывает relayout —
    /// текст появляется в fallback-шрифте сразу, подменяется по приходу (FOUT).
    FontLoaded {
        family: String,
        weight: u16,
        style: lumen_core::FontStyle,
        unicode_range: Vec<lumen_font::UnicodeRange>,
        bytes: Vec<u8>,
    },
    /// Все байты получены — для финального полного pipeline.
    /// Последнее поле — generation навигации (U-1).
    LoadDone(RawPage, u64),
    /// Ошибка при загрузке страницы. Последнее поле — generation навигации (U-1).
    LoadError(String, u64),
    /// BUG-171 этап 2: финальный pipeline (parse → JS → fetch подресурсов →
    /// layout) выполнен на фоновом потоке; готовый результат применяется на
    /// UI-потоке (`apply_loaded_page`) без блокировки event loop. Последнее
    /// поле — generation навигации (U-1).
    RenderDone(Box<RenderOutcome>, u64),
    /// FRAME-4 срез 3: навигация ОДНОГО фрейма (клик по ссылке/сабмит формы/
    /// шаг истории внутри него) выполнена на фоновом потоке — та же сеть+
    /// парсинг+скрипты+layout, что раньше блокировали UI-поток целиком внутри
    /// `frames::navigate_frame`, теперь `frames::run_frame_navigation` там же,
    /// но за пределами event loop. `host_doc`+`host` — ключ generation-слота
    /// (`frames::FrameNavRequest`), `generation` — снятый при отправке номер:
    /// `Lumen::on_frame_nav_done` сверяет их и роняет ответ, если хозяин успел
    /// навигировать ещё раз, пока этот запрос был в полёте. `old_doc` —
    /// документ, который заменяет `handles` (пусто, если фрейма к этому
    /// моменту уже нет — предок навигировал сам, либо страница
    /// перезагрузилась целиком).
    FrameNavDone {
        host_doc: Arc<Mutex<Document>>,
        host: NodeId,
        old_doc: Arc<Mutex<Document>>,
        generation: u64,
        handles: Vec<frames::FrameHandle>,
    },
}

/// Размер одного HTML-chunk при разбивке для инкрементального парсинга.
pub(crate) const STREAM_CHUNK_BYTES: usize = 8 * 1024;
/// Минимальный интервал между промежуточными кадрами при streaming (мс) — ~60 Гц.
pub(crate) const STREAM_PAINT_INTERVAL_MS: u128 = 16;
