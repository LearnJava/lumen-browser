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
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
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
                    eprintln!("Lazy: РїСЂРѕРїСѓСЃРє {url}: {e}");
                    continue;
                }
            };

            // Animated GIF detection for lazy-loaded images.
            if lumen_image::is_gif(&bytes) {
                match lumen_image::decode_gif_animated(&bytes) {
                    Ok(gif) if gif.frame_count() > 1 => {
                        // BUG-272 СЃСЂРµР· 19: only the first frame is decoded eagerly here.
                        let first = match gif.frame_image(0) {
                            Ok(img) => img,
                            Err(e) => {
                                eprintln!("Lazy: РЅРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ GIF {url}: {e}");
                                continue;
                            }
                        };
                        if let Some(src) = self.layout_source.as_ref() {
                            let mut doc = src.document.lock().unwrap();
                            let node_id = NodeId::from_index(nid as usize);
                            apply_intrinsic_size(&mut doc, node_id, first.width, first.height);
                        }
                        eprintln!(
                            "Lazy GIF-Р°РЅРёРјР°С†РёСЏ: {} ({}Г—{}, {} РєР°РґСЂРѕРІ)",
                            url, gif.width, gif.height, gif.frame_count()
                        );
                        if let Some(r) = self.renderer.as_mut() {
                            // BUG-272 СЃСЂРµР· 17: insert into the CPU cache first, then
                            // register the returned Arc handle вЂ” raw_images shares the
                            // cache's allocation instead of a second pixel copy.
                            let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), first);
                            if let Err(e) = r.register_image(url.clone(), handle) {
                                eprintln!("Lazy GIF: РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅР° {url}: {e}");
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
                            eprintln!("Lazy Р·Р°РіСЂСѓР¶РµРЅР° (GIF, 1 РєР°РґСЂ): {url} ({}Г—{})", img.width, img.height);
                            if let Some(r) = self.renderer.as_mut() {
                                let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), img);
                                if let Err(e) = r.register_image(url.clone(), handle) {
                                    eprintln!("Lazy: РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅР° {url}: {e}");
                                }
                            } else {
                                self.pending_images.push((url, Arc::new(img)));
                            }
                        }
                        continue;
                    }
                    Err(e) => {
                        eprintln!("Lazy: РЅРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ GIF {url}: {e}");
                        continue;
                    }
                }
            }

            let image = match lumen_image::decode(&bytes) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("Lazy: РЅРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ {url}: {e}");
                    continue;
                }
            };
            eprintln!("Lazy Р·Р°РіСЂСѓР¶РµРЅР°: {} ({}Г—{}, {:?})", url, image.width, image.height, image.format);
            // Apply intrinsic size to DOM so next relayout picks up correct dimensions.
            if let Some(src) = self.layout_source.as_ref() {
                let mut doc = src.document.lock().unwrap();
                let node_id = NodeId::from_index(nid as usize);
                apply_intrinsic_size(&mut doc, node_id, image.width, image.height);
            }
            if let Some(r) = self.renderer.as_mut() {
                let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), image);
                if let Err(e) = r.register_image(url.clone(), handle) {
                    eprintln!("Lazy: РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅР° {url}: {e}");
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
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
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
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
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
                    eprintln!("video GIF: РїСЂРѕРїСѓСЃРє {src}: {e}");
                    continue;
                }
            };

            if !lumen_image::is_gif(&bytes) {
                eprintln!("video GIF: {src} РЅРµ СЏРІР»СЏРµС‚СЃСЏ GIF");
                continue;
            }

            match lumen_image::decode_gif_animated(&bytes) {
                Ok(gif) => {
                    // BUG-272 СЃСЂРµР· 17/19: Arc so register/pending share the buffer;
                    // only the first frame is materialised eagerly.
                    let first = match gif.frame_image(0) {
                        Ok(img) => Arc::new(img),
                        Err(e) => {
                            eprintln!("video GIF: РѕС€РёР±РєР° РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ {src}: {e}");
                            continue;
                        }
                    };
                    let key = format!("video:{nid}");
                    if let Some(r) = self.renderer.as_mut() {
                        if let Err(e) = r.register_image(key.clone(), Arc::clone(&first)) {
                            eprintln!("video GIF: РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅ {key}: {e}");
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
                        "video GIF: Р·Р°РіСЂСѓР¶РµРЅ nid={nid} ({}Г—{}, {} РєР°РґСЂРѕРІ)",
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
                Err(e) => eprintln!("video GIF: РѕС€РёР±РєР° РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ {src}: {e}"),
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
                eprintln!("video GIF РєР°РґСЂ {key}[{idx}]: {e}");
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
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn navigate_fragment(&mut self, fragment: String) {
        // Same-document fragment navigation must keep the JS side in sync: update
        // `location`, push a same-document history entry, and fire `hashchange`
        // (HTML LS В§7.4.2 fragment-navigation). Route through the existing JS
        // `_lumen_navigate_or_fragment` path вЂ” it resolves the target, sees only
        // the fragment differs, updates `location`, queues a `HistoryUrlUpdate`
        // (drained into `nav_back` so back/forward works), and fires `hashchange`.
        let new_url = links::fragment_url(self.current_display_url(), &fragment);
        // ADR-016 M2.2c-2d (16): hashchange fire-and-forget void-dispatch С‡РµСЂРµР·
        // `route_eval_js` вЂ” РѕР±Р° `_lumen_*`-РІС‹Р·РѕРІР° С‡РёСЃС‚С‹Р№ void Р±РµР· СЃРёРЅС…СЂРѕРЅРЅРѕРіРѕ С‡С‚РµРЅРёСЏ
        // СЂРµР·СѓР»СЊС‚Р°С‚Р° СЃР»РµРґРѕРј (`location`/`hashchange` С„РёРєСЃРёСЂСѓСЋС‚СЃСЏ JS-СЃС‚РѕСЂРѕРЅРѕР№, Р°
        // `HistoryUrlUpdate` РґСЂРµРЅРёС‚СЃСЏ РїРѕР·Р¶Рµ С‡РµСЂРµР· `take_nav_updates`). РџРѕРґ С„Р»Р°РіРѕРј
        // (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґСЏС‚ off-UI-thread РґРІСѓРјСЏ `task` РІ FIFO-РїРѕСЂСЏРґРєРµ
        // (dispatch в†’ navigate СЃРѕС…СЂР°РЅС‘РЅ); Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Рµ
        // РІС‹Р·РѕРІС‹ РїРѕ UI-С…СЌРЅРґР»Сѓ, **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ** РїСЂРµР¶РЅРёРј `js.eval_js`. `escaped`
        // СЃС‚СЂРѕРёС‚СЃСЏ РґРѕ РјР°СЂС€СЂСѓС‚РёР·Р°С†РёРё; Р±РѕСЂСЂРѕСѓ `engine_thread`/`js_ctx` вЂ” СЂР°Р·РґРµР»СЊРЅС‹Р№.
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
        // ancestors BEFORE the page-level scroll below вЂ” real fragment
        // navigation / `scrollIntoView()` walks the whole scrollable
        // ancestor chain, not just the page.
        if let (Some(nid), Some(rect)) = (node_id, target_rect) {
            self.scroll_nested_ancestors_into_view(nid, rect);
        }
        if let Some(y) = target_y {
            // CSS Scroll Behavior L1 В§3: respect scroll-behavior on the scrolling box.
            // The page viewport's scroll-behavior comes from the root (<html>) element.
            if self.page_scroll_behavior() == ScrollBehavior::Smooth {
                self.start_smooth_scroll(y);
            } else {
                self.scroll_to(y);
            }
        }
    }

    /// Returns the effective `scroll-behavior` for the page viewport (CSS Scroll Behavior L1 В§3).
    /// Reads from the first non-root layout box (the `<html>` element's style).
    pub(crate) fn page_scroll_behavior(&self) -> ScrollBehavior {
        self.layout_box
            .as_ref()
            .and_then(|lb| lb.children.first())
            .map(|html_box| html_box.style.scroll_behavior)
            .unwrap_or(ScrollBehavior::Auto)
    }

    /// РџРµСЂРµР·Р°РіСЂСѓР·РёС‚СЊ С‚РµРєСѓС‰РёР№ РёСЃС‚РѕС‡РЅРёРє: fetch/parse/layout/paint СЃРЅРѕРІР°. РќР°
    /// `PageSource::Empty` вЂ” no-op (РіСЂСѓР·РёС‚СЊ РЅРµС‡РµРіРѕ). РџСЂРё РѕС€РёР±РєРµ вЂ” РѕСЃС‚Р°РІР»СЏРµРј
    /// РїСЂРµРґС‹РґСѓС‰РёР№ display_list, РїРµС‡Р°С‚Р°РµРј РїСЂРёС‡РёРЅСѓ РІ stderr.
    pub(crate) fn reload(&mut self) {
        if matches!(self.source, PageSource::Empty) {
            return;
        }
        // Record navigation start for PerformanceNavigationTiming (Navigation Timing L2 В§4.2).
        self.nav_start = Some(std::time::Instant::now());
        // A fresh navigation supersedes any prior settled error (BUG-308).
        self.load_failed = false;
        self.load_error_message = None;
        click_log::log_load_start(&self.source.describe());
        println!("Reload: {}", self.source.describe());

        // U-1: РЅРµР±Р»РѕРєРёСЂСѓСЋС‰Р°СЏ РЅР°РІРёРіР°С†РёСЏ. РљРѕРіРґР° РѕРєРЅРѕ СѓР¶Рµ СЃРѕР·РґР°РЅРѕ (Р»СЋР±Р°СЏ РЅР°РІРёРіР°С†РёСЏ
        // РїРѕСЃР»Рµ РїРµСЂРІРѕРіРѕ РєР°РґСЂР° вЂ” РєР»РёРє РїРѕ СЃСЃС‹Р»РєРµ, Р°РґСЂРµСЃРЅР°СЏ СЃС‚СЂРѕРєР°, back/forward,
        // JS location.href=), РіСЂСѓР·РёРј С‡РµСЂРµР· С‚РѕС‚ Р¶Рµ Р°СЃРёРЅС…СЂРѕРЅРЅС‹Р№ streaming-РїР°Р№РїР»Р°Р№РЅ,
        // С‡С‚Рѕ Рё РїРµСЂРІРёС‡РЅР°СЏ Р·Р°РіСЂСѓР·РєР° РІ `resumed`: С‚РµР»Рѕ С„РµС‚С‡РёС‚СЃСЏ РІ С„РѕРЅРѕРІРѕРј РїРѕС‚РѕРєРµ,
        // РѕРєРЅРѕ РїСЂРѕРґРѕР»Р¶Р°РµС‚ СЂРёСЃРѕРІР°С‚СЊ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹Рµ РєР°РґСЂС‹, Р° С‚СЏР¶С‘Р»С‹Р№ С„РёРЅР°Р»СЊРЅС‹Р№
        // pipeline (`render_bytes`) РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ РѕРґРёРЅ СЂР°Р· РЅР° UI-РїРѕС‚РѕРєРµ РІ
        // `LoadEvent::LoadDone`. Р Р°РЅСЊС€Рµ `reload()` РґРµР»Р°Р» РІРµСЃСЊ fetch+parse+JS+layout
        // СЃРёРЅС…СЂРѕРЅРЅРѕ РїСЂСЏРјРѕ Р·РґРµСЃСЊ вЂ” РѕРєРЅРѕ РјС‘СЂР·Р»Рѕ РЅР° РІСЃС‘ РІСЂРµРјСЏ РЅР°РІРёРіР°С†РёРё.
        if self.window.is_some() {
            // РЎР±СЂР°СЃС‹РІР°РµРј СЃРѕСЃС‚РѕСЏРЅРёРµ РїСЂРѕС€Р»РѕРіРѕ streaming-С†РёРєР»Р° вЂ” СЌС‚Рѕ РЅРѕРІР°СЏ СЃС‚СЂР°РЅРёС†Р°
            // (Р·РµСЂРєР°Р»РёС‚ Р±Р»РѕРє РІ `resumed`). `stream_builder = None` РѕР±СЏР·Р°С‚РµР»РµРЅ,
            // РёРЅР°С‡Рµ chunk-Рё РґРѕРїРёС€СѓС‚СЃСЏ РІ DOM РїСЂРµРґС‹РґСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹.
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

        // Fallback (РѕРєРЅР° РµС‰С‘ РЅРµС‚ вЂ” СЂРµРґРєРёР№ РїСѓС‚СЊ, РЅР°РїСЂ. headless/С‚РµСЃС‚С‹): РїСЂРµР¶РЅСЏСЏ
        // СЃРёРЅС…СЂРѕРЅРЅР°СЏ Р·Р°РіСЂСѓР·РєР°.
        // Phase 4c: РїРѕРїСЂРѕР±РѕРІР°С‚СЊ Р·Р°РіСЂСѓР·РёС‚СЊ С‡РµСЂРµР· GpuSession (WinitSession)
        // РґР»СЏ File Рё Url; fallback Рє СЃС‚Р°СЂРѕРјСѓ РїСѓС‚Рё РґР»СЏ Snapshot
        let load_result = if let Some(page) = self.reload_via_gpu_session() {
            // WinitSession Р·Р°РіСЂСѓР·РєР° СѓСЃРїРµС€РЅР°
            Ok((page, None, None))
        } else {
            // Fallback Рє СЃС‚Р°СЂРѕРјСѓ РїСѓС‚Рё (PageSource::Snapshot, РёР»Рё РѕС€РёР±РєР° WinitSession)
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
            // BUG-836: same origin key, tab-scoped map вЂ” the entry outlives this
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
                // ADR-016 M2.2c-2b: Р·РµСЂРєР°Р»РёРј РЅРѕРІС‹Р№ С…СЌРЅРґР» + DOM РІ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє.
                self.sync_engine_js_state();
                // The new runtime starts empty; re-seed it with the current Navigation state.
                self.commit_nav_state();
                self.content_height = content_height_of(&page.display_list);
                self.content_width = content_width_of(&page.display_list);
                // On full page load, mark all tiles dirty вЂ” content has changed completely.
                self.tile_grid.mark_all_dirty(self.content_width, self.content_height);
                // BUG-480 срез 14: под-документы новой страницы заменяют старые
                // (то же, что делает `apply_loaded_page`, — этот резервный путь
                // reload'а без окна о фреймах не знал вовсе и оставлял хэндлы
                // ПРЕДЫДУЩЕГО документа живыми). Строго до записи списка: она
                // вклеивает в него содержимое фреймов.
                self.frames = page.frames;
                self.frame_env = page.frame_env;
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
                // entirely вЂ” invalidate so the next `try_relayout_raf_incremental`
                // doesn't diff a stale cache against this fresh tree.
                self.page_prev_cascade_styles = None;
                self.layout_box = Some(page.layout_box);
                // content-visibility: auto (BB-4): РЅРѕРІР°СЏ СЃС‚СЂР°РЅРёС†Р° вЂ” ratchet СЃ РЅСѓР»СЏ.
                self.cv_relevant.clear();
                self.cv_events.clear();
                self.cv_skipped.clear();
                self.cv_auto_state.clear();
                self.refresh_cv_state();
                self.update_snap_containers();
        self.update_scroll_containers();
                // Push initial layout geometry so JS can query bounding rects
                // immediately after page load (before the first relayout).
                // ADR-016 M2.2c-2d: routed off-thread like the relayout push above вЂ”
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
                // Display list РґСЂСѓРіРѕР№ в†’ СЃС‚Р°СЂС‹Рµ match-rect-С‹ РЅРµРІР°Р»РёРґРЅС‹.
                // Closing РїРѕР»РЅРѕСЃС‚СЊСЋ СЃР±СЂР°СЃС‹РІР°РµС‚ query/active вЂ” РїРѕР»СЊР·РѕРІР°С‚РµР»СЋ
                // РЅСѓР¶РЅРѕ РѕС‚РєСЂС‹С‚СЊ find Р·Р°РЅРѕРІРѕ РїРѕСЃР»Рµ reload, С‡С‚Рѕ РµСЃС‚РµСЃС‚РІРµРЅРЅРѕ.
                self.find.close();
                self.address_bar.close();
                // РќРѕРІР°СЏ СЃС‚СЂР°РЅРёС†Р° вЂ” РїРѕРєР°Р·С‹РІР°РµРј СЃРІРµСЂС…Сѓ-СЃР»РµРІР° (Р»РёР±Рѕ РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°РµРј
                // offset РёР· back/forward, РєР°Рє РІ `apply_loaded_page`).
                let (rx, ry) = self.pending_restore_scroll.take().unwrap_or((0.0, 0.0));
                self.scroll_x = rx;
                self.scroll_y = ry;
                // Р›СЋР±РѕР№ Р°РєС‚РёРІРЅС‹Р№ drag РїСЂРµСЂС‹РІР°РµС‚СЃСЏ (content_height РґСЂСѓРіРѕР№,
                // thumb-РіРµРѕРјРµС‚СЂРёСЏ РїРµСЂРµСЃС‡РёС‚Р°РЅР° СЃ РЅСѓР»СЏ).
                self.scroll_drag = None;
                // РђРєС‚РёРІРЅС‹Рµ Р°РЅРёРјР°С†РёРё СЃС‚Р°СЂРѕР№ СЃС‚СЂР°РЅРёС†С‹ СЃР±СЂР°СЃС‹РІР°РµРј.
                self.scroll_anim = None;
                self.momentum_anim = None;
                self.forward_momentum_stop();
                self.touchpad_vel = (0.0, 0.0);
                // Reset CPU image cache for the reloaded page (10E.4 scroll-discard).
                self.image_cache.clear();
                if let Some(r) = self.renderer.as_mut() {
                    // РЎС‚Р°СЂР°СЏ GPU-cache РєР°СЂС‚РёРЅРѕРє РѕС‚РЅРѕСЃРёС‚СЃСЏ Рє РїСЂРµРґС‹РґСѓС‰РµР№ СЃС‚СЂР°РЅРёС†Рµ
                    // (РґР°Р¶Рµ РµСЃР»Рё src СЃРѕРІРїР°РґР°РµС‚, content РјРѕРі РёР·РјРµРЅРёС‚СЊСЃСЏ). Р§РёСЃС‚РёРј
                    // Рё СЂРµРіРёСЃС‚СЂРёСЂСѓРµРј Р·Р°РЅРѕРІРѕ.
                    r.clear_images();
                    for (src, image) in &page.images {
                        // BUG-272 СЃСЂРµР· 17: `image` вЂ” Arc РёР· IMAGE_CACHE; register
                        // РєР»РѕРЅРёСЂСѓРµС‚ СѓРєР°Р·Р°С‚РµР»СЊ, raw_images СЂР°Р·РґРµР»СЏРµС‚ Р°Р»Р»РѕРєР°С†РёСЋ.
                        if let Err(err) = r.register_image(src.clone(), Arc::clone(image)) {
                            eprintln!("РљР°СЂС‚РёРЅРєР° {src} РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅР°: {err}");
                        }
                        self.image_cache.insert(lumen_image::ImageKey::new(src), (**image).clone());
                    }
                } else {
                    // Renderer РµС‰С‘ РЅРµ СЃРѕР·РґР°РЅ вЂ” РѕР±С‹С‡РЅРѕ РЅРµРІРѕР·РјРѕР¶РЅРѕ (reload РёРґС‘С‚
                    // РїРѕ РєР»Р°РІРёС€Рµ, РѕРєРЅРѕ СѓР¶Рµ РµСЃС‚СЊ), РЅРѕ Р·Р°С‰РёС‚РёРјСЃСЏ: СЃРєР»Р°РґС‹РІР°РµРј РІ
                    // pending_images, resumed РїРѕРґС…РІР°С‚РёС‚.
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
                // Deliver W3C Navigation Timing L2 entry (В§4.2) to JS PerformanceObservers.
                // ADR-016 M2.2c-2d (20): last direct `self.js_ctx` read on the reload
                // nav-timing path в†’ `self.js_present` gate + `route_task_js`. `nav_start`
                // is still taken unconditionally (as the old tuple did), so on the В«no JSВ»
                // / В«no startВ» / В«no urlВ» branch it is cleared exactly as before. Delivery
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
                eprintln!("РћС€РёР±РєР° reload {}: {err}", self.source.describe());
            }
        }
    }

    /// РџРѕРїС‹С‚Р°С‚СЊСЃСЏ Р·Р°РіСЂСѓР·РёС‚СЊ СЃС‚СЂР°РЅРёС†Сѓ С‡РµСЂРµР· GpuSession (WinitSession).
    /// Р’РѕР·РІСЂР°С‰Р°РµС‚ LoadedPage РµСЃР»Рё СѓСЃРїРµС€РЅРѕ, РёРЅР°С‡Рµ None (fallback Рє СЃС‚Р°СЂРѕРјСѓ РїСѓС‚Рё).
    ///
    /// Phase 4c: РёСЃРїРѕР»СЊР·СѓРµС‚ WinitSession::render_to_gpu() РІРјРµСЃС‚Рѕ inline pipeline
    /// РґР»СЏ PageSource::File Рё PageSource::Url.
    pub(crate) fn reload_via_gpu_session(&mut self) -> Option<LoadedPage> {
        use lumen_driver::{WinitSession, GpuSession};

        // РџСЂРµРѕР±СЂР°Р·РѕРІР°С‚СЊ PageSource РІ URL РґР»СЏ WinitSession
        let url = match &self.source {
            PageSource::File(path) => {
                format!("file://{}", path.display())
            }
            PageSource::Url(u) => u.clone(),
            _ => return None, // Snapshot Рё Empty РѕР±СЂР°Р±РѕС‚Р°РЅС‹ РѕС‚РґРµР»СЊРЅРѕ
        };

        let viewport = self.renderer.as_ref().map_or_else(
            || Size::new(1024.0, 720.0),
            |r| {
                let s = r.viewport_size();
                Size::new(s.width, s.height)
            },
        );

        // РЎРѕР·РґР°С‚СЊ СЃРµСЃСЃРёСЋ СЃ РЅСѓР¶РЅС‹Рј viewport
        let mut session = WinitSession::with_viewport(viewport.width, viewport.height);

        // Р—Р°РіСЂСѓР·РёС‚СЊ СЃС‚СЂР°РЅРёС†Сѓ С‡РµСЂРµР· WinitSession
        if session.navigate(&url).is_err() {
            return None;
        }

        // РџРѕР»СѓС‡РёС‚СЊ RenderedPage С‡РµСЂРµР· render_to_gpu()
        let rendered = match session.render_to_gpu() {
            Ok(r) => r,
            Err(_) => return None,
        };

        // РџСЂРµРѕР±СЂР°Р·РѕРІР°С‚СЊ RenderedPage РІ LoadedPage
        // РџСЂРµРѕР±СЂР°Р·РѕРІР°С‚СЊ lumen_driver::JsNavigateRequest РІ shell::JsNavigateRequest
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
            // BUG-272 СЃСЂРµР· 17: driver's `RenderedPage.images` is still owned
            // `Image`; wrap at this boundary (driver path unchanged).
            images: rendered.images.into_iter().map(|(s, i)| (s, Arc::new(i))).collect(),
            animated_gifs: Vec::new(), // lumen-driver path has no animated GIF support yet
            lazy_pairs: Vec::new(), // Phase 4c: TODO integrate lazy loading
            layout_box: rendered.layout_box,
            // Driver path renders via lumen-driver which uses Arc<dyn FontProvider>;
            // shell's LoadedPage now requires Arc<FontRegistry> for PH3-19 dynamic
            // registration. Use an empty registry вЂ” driver pages have no async fonts.
            font_registry: Arc::new(lumen_font::FontRegistry::new()),
            pending_web_fonts: Vec::new(),
            js_navigate,
            page_tracks: tracks::PageTracks::default(),
            // lumen-driver СЂРµРЅРґРµСЂРёС‚ С‡РµСЂРµР· СЃРІРѕР№ headless-РїР°Р№РїР»Р°Р№РЅ Р±РµР·
            // sub-РґРѕРєСѓРјРµРЅС‚РѕРІ вЂ” С„СЂРµР№РјРѕРІ РЅР° СЌС‚РѕРј РїСѓС‚Рё РЅРµС‚.
            frames: Vec::new(),
            frame_env: None,
        })
    }

    /// Р—Р°РїСѓСЃС‚РёС‚СЊ background-РїРѕС‚РѕРє Р·Р°РіСЂСѓР·РєРё С‚РµРєСѓС‰РµРіРѕ `source`.
    ///
    /// РџРѕС‚РѕРє fetches Р±Р°Р№С‚С‹, Р·Р°С‚РµРј:
    ///
    /// 1. Р”Р»СЏ РєР°Р¶РґРѕРіРѕ STREAM_CHUNK_BYTES-chunk: РїСЂРѕРіРѕРЅСЏРµС‚ С‡РµСЂРµР· `PreloadScanner`
    ///    (PH1-8, HTML LS В§13.2.6.4.7), РѕС‚РїСЂР°РІР»СЏРµС‚ `EarlyPreloadHints`, Р·Р°С‚РµРј
    ///    `HtmlChunk`. Hint-С‹ СЌРјРёС‚СЏС‚СЃСЏ РёР· **РєР°Р¶РґРѕРіРѕ** chunk-Р°, РЅРµ С‚РѕР»СЊРєРѕ РїРµСЂРІРѕРіРѕ вЂ”
    ///    СЌС‚Рѕ РґР°С‘С‚ СЂРµР°Р»СЊРЅС‹Р№ РІС‹РёРіСЂС‹С€ РґР»СЏ stylesheet/С€СЂРёС„С‚РѕРІ, СЃС‚РѕСЏС‰РёС… Р·Р° РїРµСЂРІС‹РјРё 8 РљР‘.
    /// 2. PH1-2: РґР»СЏ РєР°Р¶РґРѕРіРѕ `Stylesheet`-hint Р·Р°РїСѓСЃРєР°РµС‚ РїР°СЂР°Р»Р»РµР»СЊРЅС‹Р№ CSS-Р·Р°РіСЂСѓР·С‡РёРє,
    ///    РєРѕС‚РѕСЂС‹Р№ РїСЂРёСЃС‹Р»Р°РµС‚ `CssLoaded` РµС‰С‘ РґРѕ `LoadDone`.
    /// 3. РџРѕ Р·Р°РІРµСЂС€РµРЅРёРё вЂ” `LoadDone(raw)` РґР»СЏ С„РёРЅР°Р»СЊРЅРѕРіРѕ pipeline.
    ///
    /// РџСЂРё РѕС€РёР±РєРµ вЂ” `LoadError`.
    ///
    /// `generation` (U-1) РјРµС‚РёС‚ РєР°Р¶РґРѕРµ РёСЃРїСѓСЃРєР°РµРјРѕРµ СЃРѕР±С‹С‚РёРµ; `user_event`
    /// РѕС‚Р±СЂР°СЃС‹РІР°РµС‚ СЃРѕР±С‹С‚РёСЏ СѓСЃС‚Р°СЂРµРІС€РµРіРѕ РїРѕРєРѕР»РµРЅРёСЏ, РµСЃР»Рё РЅР°РІРёРіР°С†РёСЋ СѓСЃРїРµР»Рё СЃРјРµРЅРёС‚СЊ.
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
        // BUG-268: media-РєРѕРЅС‚РµРєСЃС‚ СЌРєСЂР°РЅР° вЂ” РґР»СЏ РіРµР№С‚Р° speculative-С„РµС‚С‡Р°
        // `<link rel=stylesheet media=...>`, С‡С‚РѕР±С‹ print-only Р»РёСЃС‚ РЅРµ РіСЂРµР»
        // РєСЌС€ Рё РЅРµ СЃР»Р°Р» CssLoaded (progressive-РєР°РґСЂС‹ РЅРµ РєСЂР°СЃСЏС‚СЃСЏ print-СЃС‚РёР»СЏРјРё).
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
            // PH1-8: РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅС‹Р№ preload-СЃРєР°РЅРµСЂ вЂ” РѕР±СЂР°Р±Р°С‚С‹РІР°РµС‚ РєР°Р¶РґС‹Р№ chunk.
            // Hint-С‹ РѕС‚РїСЂР°РІР»СЏСЋС‚СЃСЏ Р”Рћ СЃРѕРѕС‚РІРµС‚СЃС‚РІСѓСЋС‰РµРіРѕ HtmlChunk, С‡С‚РѕР±С‹ fetch
            // РЅР°С‡Р°Р»СЃСЏ РїР°СЂР°Р»Р»РµР»СЊРЅРѕ СЃ DOM-РїР°СЂСЃРёРЅРіРѕРј (spec В§13.2.6.4.7).
            let mut preload_scanner = lumen_html_parser::PreloadScanner::new();

            // PH1-2a: РґР»СЏ URL-РёСЃС‚РѕС‡РЅРёРєРѕРІ С‚РµР»Рѕ СЃС‚СЂРёРјРёС‚СЃСЏ РїСЂСЏРјРѕ СЃ СЃРѕРєРµС‚Р° вЂ”
            // РїРѕСЂС†РёРё РїСЂРёР»РµС‚Р°СЋС‚ РІ `on_chunk` РїРѕ РјРµСЂРµ С‡С‚РµРЅРёСЏ, РЅРµ РґРѕР¶РёРґР°СЏСЃСЊ РїРѕР»РЅРѕР№
            // Р·Р°РіСЂСѓР·РєРё. Р”Р»СЏ File/Snapshot/Static С‚РµР»Рѕ СѓР¶Рµ РІ РїР°РјСЏС‚Рё, РїРѕСЌС‚РѕРјСѓ РµРіРѕ
            // РґРѕСЃС‚Р°С‚РѕС‡РЅРѕ РЅР°СЂРµР·Р°С‚СЊ РЅР° STREAM_CHUNK_BYTES (РїСЂРµР¶РЅРµРµ РїРѕРІРµРґРµРЅРёРµ).
            let raw = if let PageSource::Url(url) = &source {
                // BUG-757: Р±Р°Р·Р° preload-С…РёРЅС‚РѕРІ вЂ” Р°РґСЂРµСЃ, СЃ РєРѕС‚РѕСЂРѕРіРѕ Р Р•РђР›Р¬РќРћ
                // С‚РµС‡С‘С‚ С‚РµР»Рѕ (РµРіРѕ РїСЂРёРЅРѕСЃРёС‚ СЃР°Рј chunk), Р° РЅРµ Р·Р°РїСЂРѕС€РµРЅРЅС‹Р№: РїРѕСЃР»Рµ
                // СЂРµРґРёСЂРµРєС‚Р° РѕРЅРё СЂР°Р·РЅС‹Рµ, Рё РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹Р№ `src` СѓС…РѕРґРёР» РЅР°
                // РґРѕ-СЂРµРґРёСЂРµРєС‚РЅС‹Р№ РїСѓС‚СЊ РµС‰С‘ РґРѕ С‚РѕРіРѕ, РєР°Рє РґРѕРєСѓРјРµРЅС‚ РїРѕР»СѓС‡Р°Р»
                // РїСЂР°РІРёР»СЊРЅСѓСЋ Р±Р°Р·Сѓ. РџРµСЂРµСЃРѕР±РёСЂР°РµРј СЃС‚СЂРѕРєСѓ С‚РѕР»СЊРєРѕ РїСЂРё СЃРјРµРЅРµ hop-Р°.
                let mut base = ResourceBase::Url(url.clone());
                let chunk_proxy = proxy.clone();
                // Separate clones for prefetch warm-up: `cookie_jar`/`sink` below are
                // moved into the streaming call, so the per-chunk closure keeps its own.
                let cj_prefetch = Some(Arc::clone(&cookie_jar));
                let sink_prefetch = Arc::clone(&sink);
                let mut on_chunk = |chunk: &[u8], hop_url: &lumen_core::url::Url| {
                    if !matches!(&base, ResourceBase::Url(u) if u == hop_url.as_str()) {
                        base = ResourceBase::Url(hop_url.to_string());
                        // UI-РїРѕС‚РѕРє СЂРµР·РѕР»РІРёС‚ РєР°СЂС‚РёРЅРєРё/С€СЂРёС„С‚С‹ С‡Р°СЃС‚РёС‡РЅРѕРіРѕ DOM РѕС‚
                        // СЃРІРѕРµР№ РєРѕРїРёРё Р±Р°Р·С‹ вЂ” СЃРѕРѕР±С‰Р°РµРј РµРјСѓ РЅРѕРІСѓСЋ (BUG-757).
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
                        return; // event loop Р·Р°РІРµСЂС€С‘РЅ
                    }
                    pos = end;
                }
                raw
            };

            // Р¤РёРЅР°Р»СЊРЅС‹Рµ hint-С‹ РёР· Р±СѓС„РµСЂРёР·РѕРІР°РЅРЅРѕРіРѕ С…РІРѕСЃС‚Р° СЃРєР°РЅРµСЂР°.
            let tail = preload_scanner.end();
            if !tail.is_empty() {
                let _ = proxy.send_event(LoadEvent::EarlyPreloadHints(tail, raw.base.clone(), generation));
            }

            let _ = proxy.send_event(LoadEvent::LoadDone(raw, generation));
        });
    }

    /// РћР±РЅРѕРІРёС‚СЊ display list РЅР° РѕСЃРЅРѕРІРµ СЃРЅР°РїС€РѕС‚Р° С‡Р°СЃС‚РёС‡РЅРѕРіРѕ DOM.
    /// РџСЂРёРјРµРЅСЏРµС‚ `stream_sheet` вЂ” CSS, Р·Р°РіСЂСѓР¶РµРЅРЅС‹Р№ РїР°СЂР°Р»Р»РµР»СЊРЅС‹РјРё РїРѕС‚РѕРєР°РјРё (PH1-2).
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

        // PH1-2b: РїРѕСЃР»Рµ РїРµСЂРІРѕРіРѕ (В«Р·Р°СЃРµРІР°СЋС‰РµРіРѕВ») РєР°РґСЂР° СЂРµР»РµР№Р°СѓС‚РёРј РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕ вЂ”
        // РїРµСЂРµРёСЃРїРѕР»СЊР·СѓРµРј РіРµРѕРјРµС‚СЂРёСЋ РЅРµРёР·РјРµРЅС‘РЅРЅРѕРіРѕ РїСЂРµС„РёРєСЃР° РёР· РїСЂРѕС€Р»РѕРіРѕ РєР°РґСЂР°,
        // СЂРµР»РµР№Р°СѓС‚РёРј С‚РѕР»СЊРєРѕ РЅРѕРІС‹Рµ/РёР·РјРµРЅС‘РЅРЅС‹Рµ РїРѕРґРґРµСЂРµРІСЊСЏ. РџРѕР»РЅС‹Р№ layout РІСЃРµРіРѕ
        // С‡Р°СЃС‚РёС‡РЅРѕРіРѕ DOM РЅР° РєР°Р¶РґС‹Р№ 16-РјСЃ С‚РёРє С‚РѕСЂРјРѕР·РёР» Р±РѕР»СЊС€РёРµ СЃС‚СЂР°РЅРёС†С‹.
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
        // (`layout_streaming_incremental`, no `CounterMap`) вЂ” invalidate the
        // restyle cache so it isn't diffed against this tree by mistake.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(layout);
        self.stream_layout_seeded = true;
        self.update_snap_containers();
        self.update_scroll_containers();

        // PH1-2c: Р·Р°РїСѓСЃС‚РёС‚СЊ РїР°СЂР°Р»Р»РµР»СЊРЅСѓСЋ Р·Р°РіСЂСѓР·РєСѓ РєР°СЂС‚РёРЅРѕРє, РїРѕСЏРІРёРІС€РёС…СЃСЏ РІ СЌС‚РѕРј
        // С‡Р°СЃС‚РёС‡РЅРѕРј DOM, С‡С‚РѕР±С‹ РѕРЅРё РґРѕСЂРёСЃРѕРІС‹РІР°Р»РёСЃСЊ РїРѕ РјРµСЂРµ РїСЂРёС…РѕРґР° (РєР°Рє CSS),
        // Р° РЅРµ СЂР°Р·РѕРј РІ С„РёРЅР°Р»СЊРЅРѕРј `LoadDone`.
        self.spawn_stream_image_loads(doc, viewport);

        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// PH1-2c: РЅР°Р№С‚Рё `<img>` РІ С‡Р°СЃС‚РёС‡РЅРѕРј streaming-DOM Рё Р·Р°РїСѓСЃС‚РёС‚СЊ РїР°СЂР°Р»Р»РµР»СЊРЅС‹Рµ
    /// РїРѕС‚РѕРєРё fetch+decode РґР»СЏ РµС‰С‘-РЅРµ-Р·Р°РїСЂРѕС€РµРЅРЅС‹С… РєР°СЂС‚РёРЅРѕРє. РџРѕ Р·Р°РІРµСЂС€РµРЅРёРё РїРѕС‚РѕРє
    /// С€Р»С‘С‚ `LoadEvent::ImageDecoded`, Рё `user_event` СЂРµРіРёСЃС‚СЂРёСЂСѓРµС‚ РєР°СЂС‚РёРЅРєСѓ РІ
    /// renderer-Рµ + РїСЂРѕСЃРёС‚ redraw. Р”РµРґСѓРї С‡РµСЂРµР· `stream_images_requested`, С‚Р°Рє С‡С‚Рѕ
    /// РєР°Р¶РґС‹Р№ `src` РіСЂСѓР·РёС‚СЃСЏ РѕРґРёРЅ СЂР°Р· Р·Р° РЅР°РІРёРіР°С†РёСЋ; `loading="lazy"` РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ
    /// (РіСЂСѓР·РёС‚СЃСЏ РїРѕ Р±Р»РёР·РѕСЃС‚Рё Рє viewport СѓР¶Рµ РїРѕСЃР»Рµ `LoadDone`).
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
    /// rewrites) after the load finished was therefore never fetched at all вЂ”
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

    /// BUG-735: СЂР°Р·РЅРµСЃС‚Рё intrinsic-СЂР°Р·РјРµСЂС‹ СѓР¶Рµ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹С… РєР°СЂС‚РёРЅРѕРє РїРѕ `<img>`
    /// Р¶РёРІРѕРіРѕ РґРѕРєСѓРјРµРЅС‚Р° Рё, РµСЃР»Рё DOM РѕС‚ СЌС‚РѕРіРѕ РёР·РјРµРЅРёР»СЃСЏ, Р·Р°РїСЂРѕСЃРёС‚СЊ СЂРµР»РµР№Р°СѓС‚.
    ///
    /// РўСЂРµС‚РёР№ РїСѓС‚СЊ Р·Р°РіСЂСѓР·РєРё РєР°СЂС‚РёРЅРѕРє вЂ” streaming/РґРёРЅР°РјРёС‡РµСЃРєРёР№
    /// ([`Self::spawn_image_requests`]) вЂ” СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°Р» РїРёРєСЃРµР»Рё РІ СЂРµРЅРґРµСЂРµСЂРµ, РЅРѕ
    /// РЅРёРєРѕРіРґР° РЅРµ СЃРѕРѕР±С‰Р°Р» СЂР°Р·РјРµСЂ DOM-Сѓ: `apply_intrinsic_size` Р·РІР°Р»Рё С‚РѕР»СЊРєРѕ
    /// С„РёРЅР°Р»СЊРЅС‹Р№ `fetch_and_decode_images` Рё lazy-РїСѓС‚СЊ. Р”Р»СЏ РєР»РёРµРЅС‚СЃРєРё
    /// РѕС‚СЂРёСЃРѕРІР°РЅРЅРѕР№ СЃС‚СЂР°РЅРёС†С‹ (РіРґРµ Рє РјРѕРјРµРЅС‚Сѓ РїРѕСЏРІР»РµРЅРёСЏ `<img>` С„РёРЅР°Р»СЊРЅС‹Р№ pipeline
    /// СѓР¶Рµ РѕС‚СЂР°Р±РѕС‚Р°Р» вЂ” BUG-730) СЌС‚Рѕ Р·РЅР°С‡РёС‚ **РІСЃРµ** РєР°СЂС‚РёРЅРєРё: Р±РµР· intrinsic-РїР°СЂС‹
    /// РЅРµС‚ Рё СЃРѕРѕС‚РЅРѕС€РµРЅРёСЏ СЃС‚РѕСЂРѕРЅ, РїРѕСЌС‚РѕРјСѓ `height: auto` РґР°С‘С‚ РЅРѕР»СЊ.
    ///
    /// РџСЂРѕС…РѕРґ РєРѕР°Р»РµСЃС†РёСЂРѕРІР°РЅ: Р°СЂРј `ImageDecoded` С‚РѕР»СЊРєРѕ РєРѕРїРёС‚ СЂР°Р·РјРµСЂС‹ Рё РІР·РІРѕРґРёС‚
    /// С„Р»Р°Рі, Р° СЂР°Р·РЅРѕСЃ+СЂРµР»РµР№Р°СѓС‚ РґРµР»Р°РµС‚СЃСЏ РѕРґРёРЅ СЂР°Р· Р·Р° РєР°РґСЂ вЂ” СЃРѕС‚РЅСЏ РґРµРєРѕРґРѕРІ СЃС‚РѕРёС‚
    /// РѕРґРЅРѕРіРѕ СЂРµР»РµР№Р°СѓС‚Р°, Р° РЅРµ СЃРѕС‚РЅРё. РЎС…РѕРґРёРјРѕСЃС‚СЊ РґРµСЂР¶РёС‚СЃСЏ РЅР° С‚РѕРј, С‡С‚Рѕ
    /// `apply_intrinsic_size` РІРѕР·РІСЂР°С‰Р°РµС‚ `false`, РєРѕРіРґР° РґРѕРїРёСЃС‹РІР°С‚СЊ РЅРµС‡РµРіРѕ:
    /// РІС‚РѕСЂРѕР№ РїСЂРѕС…РѕРґ РїРѕ С‚РµРј Р¶Рµ СѓР·Р»Р°Рј DOM РЅРµ РјРµРЅСЏРµС‚ Рё СЂРµР»РµР№Р°СѓС‚ РЅРµ Р·Р°РєР°Р·С‹РІР°РµС‚.
    /// РџРµС‚Р»Рё В«СЂРµР»РµР№Р°СѓС‚ в†’ РЅРѕРІС‹Р№ Р·Р°РїСЂРѕСЃ в†’ РЅРѕРІС‹Р№ РґРµРєРѕРґВ» РЅРµС‚ вЂ” `spawn_image_requests`
    /// РґРµРґСѓРїР»РёС†РёСЂСѓРµС‚СЃСЏ С‡РµСЂРµР· `stream_images_requested`.
    pub(crate) fn apply_stream_intrinsic_sizes(&mut self) {
        if !self.stream_image_sizes_dirty {
            return;
        }
        self.stream_image_sizes_dirty = false;
        let Some(viewport) = self.relayout_viewport() else {
            // Р’СЊСЋРїРѕСЂС‚Р° РµС‰С‘ РЅРµС‚ (СЂРµРЅРґРµСЂРµСЂ РЅРµ СЃРєРѕРЅС„РёРіСѓСЂРёСЂРѕРІР°РЅ) вЂ” СЂР°Р·РјРµСЂС‹ РѕСЃС‚Р°СЋС‚СЃСЏ
            // РІ РєР°СЂС‚Рµ, РїСЂРѕС…РѕРґ РїРѕРІС‚РѕСЂРёС‚СЃСЏ РЅР° РєР°РґСЂРµ, РєРѕРіРґР° РѕРЅ РїРѕСЏРІРёС‚СЃСЏ.
            self.stream_image_sizes_dirty = true;
            return;
        };
        let changed = {
            let Some(src) = self.layout_source.as_ref() else { return };
            let Ok(mut doc) = src.document.lock() else { return };
            // РўРѕС‚ Р¶Рµ picker, С‡С‚Рѕ СЌРјРёС‚РёС‚ РєР»СЋС‡Рё `src` РІ `DrawImage`, вЂ” url РёР·
            // Р·Р°РїСЂРѕСЃР° СЃРѕРІРїР°РґР°РµС‚ СЃ РєР»СЋС‡РѕРј РєР°СЂС‚С‹ РїРѕ РїРѕСЃС‚СЂРѕРµРЅРёСЋ.
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
        // Р”РѕРїРёСЃР°РЅРЅС‹Рµ `width`/`height` вЂ” РїСЂРµР·РµРЅС‚Р°С†РёРѕРЅРЅС‹Р№ С…РёРЅС‚, С‚Рѕ РµСЃС‚СЊ РІС…РѕРґ
        // РєР°СЃРєР°РґР°. РљСЌС€ РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕРіРѕ СЂРµСЃС‚Р°Р№Р»Р° (BUG-341) Р·РЅР°РµС‚ С‚РѕР»СЊРєРѕ Рѕ
        // РјСѓС‚Р°С†РёСЏС…, РїСЂРёС€РµРґС€РёС… РёР· JS, РїРѕСЌС‚РѕРјСѓ РјСѓС‚Р°С†РёСЋ СЃРѕ СЃС‚РѕСЂРѕРЅС‹ С€РµР»Р»Р° РµРјСѓ РЅСѓР¶РЅРѕ
        // РѕР±СЉСЏРІРёС‚СЊ СЃР±СЂРѕСЃРѕРј РєСЌС€Р° вЂ” РёРЅР°С‡Рµ СЃС‚РёР»СЊ `<img>` РїРµСЂРµРёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РїСЂРµР¶РЅРёР№.
        self.page_prev_cascade_styles = None;
        self.relayout_raf_dirty();
    }

    /// Р‘Р°Р·Р° РґР»СЏ СЂР°Р·СЂРµС€РµРЅРёСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ С‚РµРєСѓС‰РµРіРѕ РґРѕРєСѓРјРµРЅС‚Р°.
    ///
    /// BUG-757: СЌС‚Рѕ РќР• `self.source.resource_base()` вЂ” С‚Р°Рј Р»РµР¶РёС‚ Р·Р°РїСЂРѕС€РµРЅРЅС‹Р№
    /// Р°РґСЂРµСЃ, Р° РїРѕСЃР»Рµ СЃРµСЂРІРµСЂРЅРѕРіРѕ СЂРµРґРёСЂРµРєС‚Р° РґРѕРєСѓРјРµРЅС‚ РїСЂРёРµС…Р°Р» СЃ РґСЂСѓРіРѕРіРѕ, Рё РѕС‚
    /// РЅРµРіРѕ Р¶Рµ РѕР±СЏР·Р°РЅС‹ СЂРµР·РѕР»РІРёС‚СЊСЃСЏ РµРіРѕ РєР°СЂС‚РёРЅРєРё, С€СЂРёС„С‚С‹ Рё СЃСЃС‹Р»РєРё. Р РµР°Р»СЊРЅР°СЏ Р±Р°Р·Р°
    /// РїСЂРёС…РѕРґРёС‚ РёР· Р·Р°РіСЂСѓР·С‡РёРєР° (`LoadEvent::DocumentBase`) Рё РіРѕРґРёС‚СЃСЏ С‚РѕР»СЊРєРѕ РґР»СЏ
    /// С‚РѕР№ РЅР°РІРёРіР°С†РёРё, РІ РєРѕС‚РѕСЂРѕР№ РїРѕР»СѓС‡РµРЅР°, вЂ” РѕС‚СЃСЋРґР° СЃРІРµСЂРєР° generation. РЎРѕР±С‹С‚РёРµ
    /// С€Р»С‘С‚СЃСЏ СЂРѕРІРЅРѕ РїСЂРё СЂР°СЃС…РѕР¶РґРµРЅРёРё Р°РґСЂРµСЃРѕРІ, РїРѕСЌС‚РѕРјСѓ Р±РµР· СЂРµРґРёСЂРµРєС‚Р° (Рё РґР»СЏ
    /// РЅРµСЃРµС‚РµРІС‹С… РёСЃС‚РѕС‡РЅРёРєРѕРІ) РѕС‚РІРµС‚ СЂРѕРІРЅРѕ РїСЂРµР¶РЅРёР№.
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
                    // streaming best-effort: С„РёРЅР°Р»СЊРЅС‹Р№ pipeline Р·Р°Р»РѕРіРёСЂСѓРµС‚/РїСЂРёРјРµРЅРёС‚.
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

    /// РџСЂРёРјРµРЅРёС‚СЊ СЂРµР·СѓР»СЊС‚Р°С‚ РїРѕР»РЅРѕРіРѕ pipeline (fetch + parse + CSS + images).
    /// РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ Рё РїСЂРё streaming `LoadDone`, Рё РјРѕР¶РµС‚ Р±С‹С‚СЊ РїРµСЂРµРёСЃРїРѕР»СЊР·РѕРІР°РЅ
    /// РІ Р±СѓРґСѓС‰РµРј РґР»СЏ РґСЂСѓРіРёС… РїСѓС‚РµР№ Р·Р°РіСЂСѓР·РєРё.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn apply_loaded_page(&mut self, page: LoadedPage, new_layout_source: Option<LayoutSource>, new_js_ctx: Option<Arc<dyn PersistentJs>>) {
        // Drop JS closures before layout_source to release Arc clones in QuickJS.
        self.set_js_ctx(None);
        self.layout_source = new_layout_source;
        self.set_js_ctx(new_js_ctx);
        // ADR-016 M2.2c-2b: Р·РµСЂРєР°Р»РёРј РЅРѕРІС‹Р№ С…СЌРЅРґР» + DOM РІ СЃРѕСЃС‚РѕСЏРЅРёРµ РґРІРёР¶РєРѕРІРѕРіРѕ
        // РїРѕС‚РѕРєР° (no-op РїСЂРё РІС‹РєР»СЋС‡РµРЅРЅРѕРј `LUMEN_ENGINE_THREAD`).
        self.sync_engine_js_state();
        // The new runtime starts empty; re-seed it with the current Navigation state.
        self.commit_nav_state();
        // Cross-document unification (see `pending_post_reload_traversal`): a
        // multi-step traversal landed on a same-document entry of the document
        // that just finished loading вЂ” apply its popstate/URL update now, on
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
        // BUG-480 СЃСЂРµР· 1: РїРѕРґ-РґРѕРєСѓРјРµРЅС‚С‹ РЅРѕРІРѕР№ СЃС‚СЂР°РЅРёС†С‹ Р·Р°РјРµРЅСЏСЋС‚ СЃС‚Р°СЂС‹Рµ С†РµР»РёРєРѕРј вЂ”
        // РїСЂРµР¶РЅРёРµ С„СЂРµР№РјС‹ (Рё РёС… JS-РєРѕРЅС‚РµРєСЃС‚С‹) РїР°РґР°СЋС‚ РІРјРµСЃС‚Рµ СЃРѕ СЃС‚СЂР°РЅРёС†РµР№.
        self.frames = page.frames;
        self.frame_env = page.frame_env;
        // BUG-480 срез 16: см. тот же сброс в резервном пути reload'а выше.
        self.hovered_frame = None;
        // BUG-480 срез 14: список страницы пишется ПОСЛЕ замены фреймов, а не
        // до неё — `set_display_list` вклеивает в него содержимое под-документов
        // из `self.frames`, и на прежнем порядке первый кадр новой страницы
        // склеивался с фреймами ПРЕДЫДУЩЕЙ (а на первой загрузке — ни с чем,
        // так что фрейм оставался серой заглушкой до первого relayout).
        self.set_display_list(page.display_list);
        self.sync_text_track_store();
        // content-visibility: auto (BB-4): РЅРѕРІР°СЏ СЃС‚СЂР°РЅРёС†Р° вЂ” ratchet СЃ РЅСѓР»СЏ.
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
        // engine вЂ” both read the snapshot the shell pushes. Until this call the only
        // pushes lived inside the relayout path (`relayout()`, `reload()`) and inside
        // the lazy-image block below, which collects geometry **only** when the page
        // has `loading="lazy"` images and never pushes computed styles at all. A
        // freshly loaded page therefore answered `""` / all-zeros unless some
        // unrelated relayout (resize, font swap, scroll) happened to race ahead of
        // the first script вЂ” the reported "works in one load out of four".
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
        // U-1: РЅРѕРІР°СЏ СЃС‚СЂР°РЅРёС†Р° РІСЃС‚Р°С‘С‚ СЃРІРµСЂС…Сѓ-СЃР»РµРІР°; РЅРѕ back/forward (Рё bfcache)
        // РїСЂРѕСЃСЏС‚ РІРѕСЃСЃС‚Р°РЅРѕРІРёС‚СЊ РїСЂРµР¶РЅРёР№ scroll-offset С‡РµСЂРµР· `pending_restore_scroll`,
        // С‚.Рє. РЅР°РІРёРіР°С†РёСЏ С‚РµРїРµСЂСЊ Р°СЃРёРЅС…СЂРѕРЅРЅР° Рё СЃР±СЂРѕСЃ РїСЂРѕРёСЃС…РѕРґРёС‚ Р·РґРµСЃСЊ, РІ LoadDone,
        // Р° РЅРµ СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ `reload()`. РљРѕРѕСЂРґРёРЅР°С‚С‹ РґРѕРєР»Р°РјРїР»СЏС‚СЃСЏ РїСЂРё РїРµСЂРІРѕРј redraw.
        let (restore_x, restore_y) = self.pending_restore_scroll.take().unwrap_or((0.0, 0.0));
        self.scroll_x = restore_x;
        self.scroll_y = restore_y;
        self.scroll_drag = None;
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.forward_momentum_stop();
        self.touchpad_vel = (0.0, 0.0);
        self.form_state.clear();
        self.frame_text_cursor.clear();
        self.validation_tooltip = None;
        self.color_picker_node = None;
        self.date_picker_node = None;
        self.date_picker_year = 0;
        self.date_picker_month = 0;
        self.select_dropdown_node = None;
        // Reset paint timing guards so new page fires fresh PerformancePaintTiming entries.
        self.first_paint_delivered = false;
        self.first_contentful_paint_delivered = false;
        // A page was applied successfully вЂ” clear any prior settled-error flag
        // (BUG-308) so `document_ready` reflects this real load, not a stale one.
        self.load_failed = false;
        self.load_error_message = None;

        // РРЅРґРµРєСЃРёСЂРѕРІР°С‚СЊ СЃС‚СЂР°РЅРёС†Сѓ РІ history_fts РґР»СЏ omnibox (@history) Рё Р·Р°РїРёСЃР°С‚СЊ
        // РІ history_store РґР»СЏ РїР°РЅРµР»Рё РёСЃС‚РѕСЂРёРё (Ctrl+H).
        // РџСЂРѕРїСѓСЃРєР°РµРј Empty Рё File sources вЂ” С‚РѕР»СЊРєРѕ HTTP(S) Рё bfcache snapshots.
        // DS-16: while Anonymous is the active profile, skip both writes вЂ”
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
            // BUG-411: the new host may carry its own shields exception вЂ”
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
        // triggers a relayout вЂ” FOUT (Flash Of Unstyled Text) swap pattern.
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
                                eprintln!("@font-face В«{}В»: РЅРµ Р·Р°РіСЂСѓР¶РµРЅ {}: {e}", pf.family, pf.url);
                                return;
                            }
                        };
                        let bytes = match lumen_font::maybe_decode_font(&raw) {
                            Ok(Some(d)) => d,
                            Ok(None) => raw,
                            Err(e) => {
                                eprintln!("@font-face В«{}В»: WOFF-РґРµРєРѕРґ РїСЂРѕРІР°Р»РёР»СЃСЏ: {e}", pf.family);
                                return;
                            }
                        };
                        if lumen_font::Font::parse(&bytes).is_err() {
                            eprintln!("@font-face В«{}В»: РЅРµРІР°Р»РёРґРЅС‹Р№ sfnt {}", pf.family, pf.url);
                            return;
                        }
                        eprintln!("@font-face async Р·Р°РіСЂСѓР¶РµРЅ: В«{}В» weight={}", pf.family, pf.weight);
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
                // BUG-272 СЃСЂРµР· 17: share the Arc; raw_images no longer deep-copies.
                if let Err(err) = r.register_image(src.clone(), Arc::clone(image)) {
                    eprintln!("РљР°СЂС‚РёРЅРєР° {src} РЅРµ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅР°: {err}");
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
        // sites like lenta.ru) never load on first paint вЂ” `relayout()` is the
        // only other path that delivers observers, and it only runs on
        // scroll/resize/zoom, not on the initial load. (BUG-163)
        // ADR-016 M2.2c-2d: lazy-image СЂРµРіРёСЃС‚СЂР°С†РёСЏ + immediate proximity check С‡РµСЂРµР·
        // `route_query_js` вЂ” СЃРЅРёРјР°РµРј РїСЂСЏРјРѕРµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёРµ. Р’СЃСЏ СѓРїРѕСЂСЏРґРѕС‡РµРЅРЅР°СЏ
        // РїРѕСЃР»РµРґРѕРІР°С‚РµР»СЊРЅРѕСЃС‚СЊ (register в†’ push rects/viewport в†’ deliver observers в†’
        // deliver lazy в†’ drain requests) РѕР±С‘СЂРЅСѓС‚Р° РІ **РѕРґРёРЅ** `route_query_js`, С‡С‚РѕР±С‹ РїРѕРґ
        // С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) РѕРЅР° РёСЃРїРѕР»РЅРёР»Р°СЃСЊ Р°С‚РѕРјР°СЂРЅРѕ **РІ РїРѕСЂСЏРґРєРµ** РЅР°
        // РґРІРёР¶РєРѕРІРѕРј РїРѕС‚РѕРєРµ (value-read `take_lazy_image_requests` РїРѕСЃР»Рµ void-push
        // СЃРѕС…СЂР°РЅСЏРµС‚ read-after-write), Р±Р»РѕРєРёСЂСѓСЏ Р»РёС€СЊ СЂР°РґРё РѕРґРЅРѕРіРѕ СЂРµР·СѓР»СЊС‚Р°С‚Р°. Owned-РґР°РЅРЅС‹Рµ
        // (`owned_pairs`/`geom`) СЃРѕР±РёСЂР°СЋС‚СЃСЏ РЅР° UI-РїРѕС‚РѕРєРµ РґРѕ РјР°СЂС€СЂСѓС‚РёР·Р°С†РёРё (Р·Р°РјС‹РєР°РЅРёРµ
        // `Send + 'static`); РіРµР№С‚ `self.js_present` РґРµСЂР¶РёС‚ СЃР±РѕСЂ РіРµРѕРјРµС‚СЂРёРё
        // JS-РіРµР№С‚РЅСѓС‚С‹Рј вЂ” Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ С„Р»Р°Рі-РѕС„С„ (`route_query_js(вЂ¦, Some(js), вЂ¦)` =
        // СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ).
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
            // Images were registered after the request_redraw above вЂ” request
            // another so the first paint actually shows them.
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }
        // JS may have requested navigation via location.href= etc.
        self.pending_js_navigate = page.js_navigate;
        // HTML LS В§8.2.3 вЂ” all resources loaded: readyState в†’ "complete" + window.load event.
        // HTML LS В§8.6 вЂ” `pageshow` fires right after `load`. `persisted=true`
        // only when this page was restored from bfcache (set by navigate_back/
        // navigate_forward); a fresh load fires `persisted=false`.
        let pageshow_persisted = std::mem::take(&mut self.pending_pageshow_persisted);
        // ADR-016 M2.2c-2d: pageshow-lifecycle void-РІС‹Р·РѕРІС‹ С‡РµСЂРµР· `route_task_js` вЂ”
        // СЃРЅРёРјР°РµРј РїСЂСЏРјРѕРµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёРµ. РћР±Р° С‡РёСЃС‚С‹Р№ fire-and-forget Р±РµР·
        // СЃРёРЅС…СЂРѕРЅРЅРѕРіРѕ С‡С‚РµРЅРёСЏ СЂРµР·СѓР»СЊС‚Р°С‚Р° СЃР»РµРґРѕРј; РїРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`)
        // СѓС…РѕРґСЏС‚ off-UI-thread РѕРґРЅРёРј `task` (РїРѕСЂСЏРґРѕРє СЃРѕС…СЂР°РЅС‘РЅ), Р±РµР· С„Р»Р°РіР° (РїРѕ
        // СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ.
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

/// РџСЂРѕРіРЅР°С‚СЊ РїРѕСЂС†РёСЋ HTML С‡РµСЂРµР· preload-СЃРєР°РЅРµСЂ, СЌРјРёС‚РЅСѓС‚СЊ `EarlyPreloadHints` Рё
/// РїР°СЂР°Р»Р»РµР»СЊРЅРѕ Р·Р°РіСЂСѓР·РёС‚СЊ РЅР°Р№РґРµРЅРЅС‹Рµ СЃС‚РёР»Рё (PH1-2 / PH1-8). РћР±С‰Р°СЏ Р»РѕРіРёРєР° РґР»СЏ РѕР±РѕРёС…
/// РїСѓС‚РµР№ `start_streaming_load`: СЃРµС‚РµРІРѕРіРѕ streaming-Р° (URL) Рё РЅР°СЂРµР·РєРё
/// СѓР¶Рµ-Р·Р°РіСЂСѓР¶РµРЅРЅРѕРіРѕ Р±СѓС„РµСЂР° (File/Snapshot/Static). Hint-С‹ С€Р»СЋС‚СЃСЏ Р”Рћ РїРµСЂРµРґР°С‡Рё
/// chunk-Р° DOM-РїР°СЂСЃРµСЂСѓ, С‡С‚РѕР±С‹ fetch РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ СЃС‚Р°СЂС‚РѕРІР°Р» СЂР°РЅСЊС€Рµ.
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
    // untouched вЂ” a cache miss simply re-fetches there). Stylesheets additionally
    // parse here to feed progressive intermediate frames (the previous PH1-2 path).
    for hint in &early {
        let (raw_url, dest, is_css) = match hint {
            lumen_html_parser::PreloadHint::Stylesheet { url, media } => {
                // BUG-268: print-only Р»РёСЃС‚ С„РёРЅР°Р»СЊРЅС‹Р№ pipeline РІСЃС‘ СЂР°РІРЅРѕ РЅРµ
                // РІРѕР·СЊРјС‘С‚ (media-РіРµР№С‚ РІ collect_link_hrefs) вЂ” РЅРµ РіСЂРµРµРј РєСЌС€
                // Рё, РіР»Р°РІРЅРѕРµ, РЅРµ СЌРјРёС‚РёРј CssLoaded: РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹Рµ progressive-
                // РєР°РґСЂС‹ РЅРµ РґРѕР»Р¶РЅС‹ РєСЂР°СЃРёС‚СЊ СЃС‚СЂР°РЅРёС†Сѓ print-РїСЂР°РІРёР»Р°РјРё.
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
            // Local files: read is instant вЂ” no cache benefit. Only CSS needs a
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

/// РЎРѕР±С‹С‚РёРµ РѕС‚ background-РїРѕС‚РѕРєР° Р·Р°РіСЂСѓР·РєРё СЃС‚СЂР°РЅРёС†С‹ РІ event loop.
///
/// Р—Р°РіСЂСѓР·РєР° СЂР°Р·Р±РёС‚Р° РЅР° С‡РµС‚С‹СЂРµ С„Р°Р·С‹: (0) `EarlyPreloadHints` вЂ” С…РёРЅС‚С‹ РёР· РїРµСЂРІС‹С…
/// Р±Р°Р№С‚ HTML РґР»СЏ СЂР°РЅРЅРµРіРѕ СЃС‚Р°СЂС‚Р° subresource fetch-РѕРІ; (1) chunks СЃС‹СЂС‹С… Р±Р°Р№С‚ РґР»СЏ
/// РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕРіРѕ РїР°СЂСЃРёРЅРіР° Рё РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹С… РєР°РґСЂРѕРІ С‡РµСЂРµР·
/// `IncrementalTreeBuilder::feed_bytes`; (2) `LoadDone` вЂ” РІСЃРµ Р±Р°Р№С‚С‹ РґРѕСЃС‚СѓРїРЅС‹,
/// Р·Р°РїСѓСЃРєР°РµРј РїРѕР»РЅС‹Р№ pipeline (CSS + РёР·РѕР±СЂР°Р¶РµРЅРёСЏ); (3) `LoadError` вЂ” РѕС€РёР±РєР° fetch.
pub(crate) enum LoadEvent {
    /// No-op wake-up (SDC-2). `winit`'s `ControlFlow::Wait` genuinely parks
    /// the event loop until an OS window event, a scheduled `WaitUntil`
    /// deadline, or a proxied user event arrives вЂ” an `AutomationCommand`
    /// enqueued from a BiDi/MCP thread is none of those, so without this the
    /// loop could sit parked indefinitely and never drain it.
    /// `AutomationHandle::execute` sends this through `load_proxy` right
    /// after queuing a command; `user_event` below does nothing with it вЂ”
    /// merely *receiving* a proxied event is what interrupts `Wait` and
    /// triggers the next `about_to_wait` (where automation commands are
    /// actually drained).
    AutomationWake,
    /// Subresource-С…РёРЅС‚С‹ РёР· РїРµСЂРІРѕРіРѕ chunk HTML (HTML LS В§13.2.6.4.7
    /// В«Speculative HTML parsingВ»). РћС‚РїСЂР°РІР»СЏСЋС‚СЃСЏ Р”Рћ РїРµСЂРІРѕРіРѕ `HtmlChunk`,
    /// С‡С‚РѕР±С‹ sink РјРѕРі РЅР°С‡Р°С‚СЊ Р·Р°РіСЂСѓР¶Р°С‚СЊ CSS/С€СЂРёС„С‚С‹ РµС‰С‘ РІ РїСЂРѕС†РµСЃСЃРµ РїР°СЂСЃРёРЅРіР°.
    /// Р”РµРґСѓРїР»РёРєР°С†РёСЏ СЃ С„РёРЅР°Р»СЊРЅС‹РјРё С…РёРЅС‚Р°РјРё РёР· `LoadDone` вЂ” С‡РµСЂРµР·
    /// `preload_dispatched` РІ `Lumen`.
    /// РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1): РёРґРµРЅС‚РёС„РёРєР°С‚РѕСЂ load-С†РёРєР»Р°,
    /// РїСЂРёСЃРІРѕРµРЅРЅС‹Р№ РІ `reload`/`resumed`. `user_event` РѕС‚Р±СЂР°СЃС‹РІР°РµС‚ СЃРѕР±С‹С‚РёРµ, РµСЃР»Рё
    /// РµРіРѕ generation РЅРµ СЃРѕРІРїР°РґР°РµС‚ СЃ `Lumen::load_generation` вЂ” Р·Р°С‰РёС‚Р° РѕС‚
    /// СѓСЃС‚Р°СЂРµРІС€РёС… СЃРѕР±С‹С‚РёР№ РіРѕРЅРєРё РЅР°РІРёРіР°С†РёР№ (Р±С‹СЃС‚СЂС‹Р№ back/forward РёР»Рё РєР»РёРє РїРѕ РґРІСѓРј
    /// СЃСЃС‹Р»РєР°Рј РїРѕРґСЂСЏРґ), РєРѕС‚РѕСЂС‹Рµ РёРЅР°С‡Рµ РїРѕРґРјРµС€Р°Р»Рё Р±С‹ DOM/CSS РїСЂРѕС€Р»РѕР№ СЃС‚СЂР°РЅРёС†С‹.
    EarlyPreloadHints(Vec<lumen_html_parser::PreloadHint>, ResourceBase, u64),
    /// BUG-757: Р±Р°Р·Р° РґРѕРєСѓРјРµРЅС‚Р° СЃС‚Р°Р»Р° РёР·РІРµСЃС‚РЅР° Рё РѕС‚Р»РёС‡Р°РµС‚СЃСЏ РѕС‚ Р·Р°РїСЂРѕС€РµРЅРЅРѕРіРѕ
    /// Р°РґСЂРµСЃР° (СЃРµСЂРІРµСЂ РѕС‚РІРµС‚РёР» СЂРµРґРёСЂРµРєС‚РѕРј). РћС‚РїСЂР°РІР»СЏРµС‚СЃСЏ РёР· streaming-РїРѕС‚РѕРєР°,
    /// РєР°Рє С‚РѕР»СЊРєРѕ С‚РµР»Рѕ РїРѕС‚РµРєР»Рѕ СЃ С„РёРЅР°Р»СЊРЅРѕРіРѕ hop-Р° вЂ” С‚Рѕ РµСЃС‚СЊ Р”Рћ С‚РѕРіРѕ, РєР°Рє
    /// С‡Р°СЃС‚РёС‡РЅС‹Р№ DOM РЅР°С‡РЅС‘С‚ Р·Р°РєР°Р·С‹РІР°С‚СЊ РєР°СЂС‚РёРЅРєРё Рё С€СЂРёС„С‚С‹, РєРѕС‚РѕСЂС‹Рµ UI-РїРѕС‚РѕРє
    /// СЂРµР·РѕР»РІРёС‚ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ Р±Р°Р·С‹. РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    DocumentBase(ResourceBase, u64),
    /// РћС‡РµСЂРµРґРЅРѕР№ chunk СЃС‹СЂС‹С… Р±Р°Р№С‚ HTML. UTF-8 РіСЂР°РЅРёС†С‹ РЅРµ РІС‹СЂР°РІРЅРёРІР°СЋС‚СЃСЏ вЂ”
    /// `IncrementalTreeBuilder::feed_bytes` Р±СѓС„РµСЂРёР·СѓРµС‚ РЅРµР·Р°РІРµСЂС€С‘РЅРЅС‹Рµ
    /// code-point-С‹ РІРЅСѓС‚СЂРё. РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    HtmlChunk(Vec<u8>, u64),
    /// CSS Р·Р°РіСЂСѓР¶РµРЅ РїР°СЂР°Р»Р»РµР»СЊРЅС‹Рј РїРѕС‚РѕРєРѕРј РґР»СЏ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹С… streaming-РєР°РґСЂРѕРІ.
    /// РњС‘СЂРґР¶РёС‚СЃСЏ РІ `Lumen::stream_sheet` Рё РїСЂРёРјРµРЅСЏРµС‚СЃСЏ РІ `paint_partial_dom`.
    /// РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    CssLoaded(Box<lumen_css_parser::Stylesheet>, u64),
    /// PH1-2c: РєР°СЂС‚РёРЅРєР° `<img>` РґРµРєРѕРґРёСЂРѕРІР°РЅР° РїР°СЂР°Р»Р»РµР»СЊРЅС‹Рј РїРѕС‚РѕРєРѕРј РІРѕ РІСЂРµРјСЏ
    /// streaming. Р РµРіРёСЃС‚СЂРёСЂСѓРµС‚СЃСЏ РІ renderer-Рµ РїРѕ РєР»СЋС‡Сѓ `src` Рё РІС‹Р·С‹РІР°РµС‚ redraw вЂ”
    /// РєР°СЂС‚РёРЅРєРё РїРѕСЏРІР»СЏСЋС‚СЃСЏ РїРѕ РјРµСЂРµ РїСЂРёС…РѕРґР°, Р° РЅРµ СЂР°Р·РѕРј РІ С„РёРЅР°Р»СЊРЅРѕРј `LoadDone`.
    /// Р”Р»СЏ Р°РЅРёРјРёСЂРѕРІР°РЅРЅРѕРіРѕ GIF `animated` РЅРµСЃС‘С‚ РІСЃРµ РєР°РґСЂС‹ (С‚РёРєР°СЋС‚СЃСЏ РІ
    /// `RedrawRequested`); `image` вЂ” РЅСѓР»РµРІРѕР№ РєР°РґСЂ РґР»СЏ РЅРµРјРµРґР»РµРЅРЅРѕР№ РѕС‚СЂРёСЃРѕРІРєРё.
    ImageDecoded {
        src: String,
        image: Box<lumen_image::Image>,
        animated: Option<Box<lumen_image::AnimatedGif>>,
    },
    /// PH3-19: web-С€СЂРёС„С‚ РёР· @font-face url() РґРµРєРѕРґРёСЂРѕРІР°РЅ РІ С„РѕРЅРѕРІРѕРј РїРѕС‚РѕРєРµ.
    /// Р РµРіРёСЃС‚СЂРёСЂСѓРµС‚СЃСЏ РІ FontRegistry + MultiFontMeasurer Рё РІС‹Р·С‹РІР°РµС‚ relayout вЂ”
    /// С‚РµРєСЃС‚ РїРѕСЏРІР»СЏРµС‚СЃСЏ РІ fallback-С€СЂРёС„С‚Рµ СЃСЂР°Р·Сѓ, РїРѕРґРјРµРЅСЏРµС‚СЃСЏ РїРѕ РїСЂРёС…РѕРґСѓ (FOUT).
    FontLoaded {
        family: String,
        weight: u16,
        style: lumen_core::FontStyle,
        unicode_range: Vec<lumen_font::UnicodeRange>,
        bytes: Vec<u8>,
    },
    /// Р’СЃРµ Р±Р°Р№С‚С‹ РїРѕР»СѓС‡РµРЅС‹ вЂ” РґР»СЏ С„РёРЅР°Р»СЊРЅРѕРіРѕ РїРѕР»РЅРѕРіРѕ pipeline.
    /// РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    LoadDone(RawPage, u64),
    /// РћС€РёР±РєР° РїСЂРё Р·Р°РіСЂСѓР·РєРµ СЃС‚СЂР°РЅРёС†С‹. РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    LoadError(String, u64),
    /// BUG-171 СЌС‚Р°Рї 2: С„РёРЅР°Р»СЊРЅС‹Р№ pipeline (parse в†’ JS в†’ fetch РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ в†’
    /// layout) РІС‹РїРѕР»РЅРµРЅ РЅР° С„РѕРЅРѕРІРѕРј РїРѕС‚РѕРєРµ; РіРѕС‚РѕРІС‹Р№ СЂРµР·СѓР»СЊС‚Р°С‚ РїСЂРёРјРµРЅСЏРµС‚СЃСЏ РЅР°
    /// UI-РїРѕС‚РѕРєРµ (`apply_loaded_page`) Р±РµР· Р±Р»РѕРєРёСЂРѕРІРєРё event loop. РџРѕСЃР»РµРґРЅРµРµ
    /// РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    RenderDone(Box<RenderOutcome>, u64),
}

/// Р Р°Р·РјРµСЂ РѕРґРЅРѕРіРѕ HTML-chunk РїСЂРё СЂР°Р·Р±РёРІРєРµ РґР»СЏ РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕРіРѕ РїР°СЂСЃРёРЅРіР°.
pub(crate) const STREAM_CHUNK_BYTES: usize = 8 * 1024;
/// РњРёРЅРёРјР°Р»СЊРЅС‹Р№ РёРЅС‚РµСЂРІР°Р» РјРµР¶РґСѓ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹РјРё РєР°РґСЂР°РјРё РїСЂРё streaming (РјСЃ) вЂ” ~60 Р“С†.
pub(crate) const STREAM_PAINT_INTERVAL_MS: u128 = 16;
