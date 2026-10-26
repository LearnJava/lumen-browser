//! `<video>` playback ticks of the current document: GIF-backed and
//! media-container sessions, and the text-track store mirrored to JS.
//!
//! Moved verbatim out of `page_load.rs` (file-size gate, docs/lint-policy.md
//! §5.1). Method bodies are unchanged; only module path and visibility differ.

use crate::*;

/// Media time between two decoded frames of a container `<video>` (≈ 30 fps cap).
const VIDEO_FRAME_MS: u64 = 33;
/// How often a playing `<video>` is re-checked for its next frame — the same
/// ≈ 60 Hz as CSS animations (`ANIM_FRAME_INTERVAL`, BUG-1637). Waiting for the
/// exact due time instead undershoots: the browser loop sleeps in
/// `recv_timeout`, whose Windows wake-ups land on the ~15.6 ms timer tick, and
/// a 33 ms wait became 47–62 ms (≈ 10 frames/s measured). A re-check with
/// nothing due costs ~0.6 ms and presents nothing (`delta Identical`).
const VIDEO_POLL: std::time::Duration = std::time::Duration::from_millis(12);

/// Whether a playing video's frame at media position `cur_ms` is due, and how
/// many media milliseconds until the next one. Re-decoding is capped at
/// roughly 30 fps (`VIDEO_FRAME_MS`): each due tick decodes, converts to RGBA
/// and re-uploads the texture. A position behind the last shown frame is a
/// seek back or a `loop` restart and is due at once — reading it as "0 ms
/// since the last frame" held the old frame until the media clock caught up,
/// a whole cycle for a looping video.
fn playing_frame_wait(last_ms: Option<u64>, cur_ms: u64) -> (bool, u64) {
    let since = match last_ms {
        Some(l) if cur_ms >= l => cur_ms - l,
        _ => VIDEO_FRAME_MS,
    };
    if since >= VIDEO_FRAME_MS { (true, VIDEO_FRAME_MS) } else { (false, VIDEO_FRAME_MS - since) }
}

impl Lumen {
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
            // Resolve URL relative to current page source, adjusted for
            // `<base href>` if the live document has one (BUG-752) — same
            // deferred-load reasoning as `fetch_and_register_lazy_images`.
            let base = match &self.source {
                PageSource::File(p) => ResourceBase::File(p.clone()),
                PageSource::Url { url, .. } => ResourceBase::Url(url.clone()),
                PageSource::Snapshot { base_url, .. } => ResourceBase::Url(base_url.clone()),
                PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => continue,
            };
            let base = self
                .layout_source
                .as_ref()
                .map(|ls| effective_base(&ls.document.lock().unwrap(), &base))
                .unwrap_or(base);

            // GAP-CSPENF срез 51: `upgrade-insecure-requests` for this GIF fetch
            // — same one-shot policy read and `resolve → upgrade` order as
            // `fetch_and_register_lazy_images` (срез 43). `media-src`'s own
            // block check already ran JS-side before this URL was queued
            // (`_lumen_check_media_src`, срез 17); this tick is the shell's
            // only point with a live `&Document` for this path, so it is where
            // the scheme rewrite has to happen — срез 50 named this path (as
            // opposed to `<audio src>`/`<track src>`) still uncovered.
            let csp_gate = self.layout_source.as_ref().and_then(|ls| {
                let doc = ls.document.lock().unwrap();
                let root = doc.root();
                crate::csp_enforce::document_csp_policy(&doc, root)
            });
            // GAP-REFERRER срез 5: same one-shot read as `csp_gate` above.
            let referrer_policy = self.layout_source.as_ref().map_or_else(
                lumen_network::ReferrerPolicy::default_policy,
                |ls| crate::resource_base::document_referrer_policy(&ls.document.lock().unwrap()),
            );
            let upgraded = csp_gate.as_ref().and_then(|(policy, _)| {
                crate::csp_enforce::upgrade_insecure_url(policy, &base.resolve_str(&src))
            });
            let fetch_url: &str = upgraded.as_deref().unwrap_or(&src);

            let bytes = match fetch_image_bytes(
                fetch_url, &base, &self.event_sink, Some(self.active_cookie_jar()), referrer_policy,
            ) {
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
                        if apply_intrinsic_size(&mut doc, node_id, gif.width, gif.height, self.relayout_viewport().unwrap_or(Size::new(0.0, 0.0))) {
                            self.note_shell_attr_writes(vec![nid]);
                        }
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

        // Advance frames for playing videos. A paused video is not skipped
        // outright (GAP-MEDIADECODE срез 8): `__lumen_video_seek` rewrites
        // `position_ms` even while paused (HTML spec: seeking a paused
        // <video> must still show the frame at the new position), and
        // `current_ms()` on a paused entry is just `position_ms` — so the
        // `last == idx` check below already skips the no-op case (unchanged
        // position while paused) at zero extra decode cost.
        let playback = self.video_gif_store.playback.lock().unwrap();
        let mut has_playing = false;

        let updates: Vec<(u32, usize, lumen_image::Image)> = playback
            .iter()
            .filter_map(|(nid, state)| {
                // `playback` is shared with container videos; their frames are
                // paced by `tick_video_sessions`, not by this GIF poll.
                if !state.paused && self.video_gif_frames.contains_key(nid) {
                    has_playing = true;
                }
                let cycle = state.cycle_ms;
                if cycle == 0 {
                    return None;
                }
                let rate = self
                    .video_gif_store
                    .playback_rates
                    .lock()
                    .unwrap()
                    .get(nid)
                    .copied()
                    .unwrap_or(1.0);
                let cur_ms = state.current_ms(elapsed_ms, rate);
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

        let has_updates = !updates.is_empty();
        for (nid, idx, image) in updates {
            let key = format!("video:{nid}");
            if let Some(r) = self.renderer.as_mut()
                && let Err(e) = r.register_image(key.clone(), Arc::new(image))
            {
                eprintln!("video GIF кадр {key}[{idx}]: {e}");
            }
            self.video_gif_last_frame.insert(nid, idx);
        }

        // Кадр, зарегистрированный выше, уходит в отрисовку этого же
        // RedrawRequested; следующий проверяем по таймеру, а не немедленным
        // `request_redraw` (тот гнал ~300 перерисовок/с всей страницы).
        if has_playing || has_updates {
            self.schedule_video_redraw(VIDEO_POLL);
        }
    }

    /// Разбудить цикл к следующему кадру видео через `page_anim_redraw_at`
    /// (BUG-1637: тот же таймер из `about_to_wait`, что и у CSS-анимаций).
    /// Видео-тики идут внутри `RedrawRequested` до отрисовки, поэтому кадр,
    /// зарегистрированный в этом тике, показывается в этом же кадре; пока ролик
    /// играет, немедленный `request_redraw` крутил цикл без пауз — ~300
    /// перерисовок всей страницы в секунду ради ~30 новых кадров, и поток
    /// окна не успевал декодировать (рывки).
    /// Clock for reading `VideoGifStore::playback`: the page's
    /// `performance.now()` that JS wrote `play_epoch_ms` in (`js_now_ms`);
    /// the shell's own clock only until the first `play`/`seek`, when every
    /// entry is paused and the clock is not read at all.
    pub(crate) fn media_clock_ms(&self) -> u64 {
        self.video_gif_store
            .js_now_ms()
            .unwrap_or_else(|| self.epoch.elapsed().as_millis() as u64)
    }

    fn schedule_video_redraw(&mut self, after: std::time::Duration) {
        let at = std::time::Instant::now() + after;
        self.page_anim_redraw_at = Some(self.page_anim_redraw_at.map_or(at, |t| t.min(at)));
    }

    /// Advance media-container `<video>` playback: drain pending loads, open
    /// decode sessions, register current frames, request redraws while playing.
    ///
    /// GAP-MEDIADECODE срез 7 / UX-VIDEO-NATIVE — the container-backed
    /// counterpart of [`Lumen::tick_video_gifs`]. The container is opened
    /// through the `VideoDecoder` installed in `window_mode.rs`
    /// (`lumen-native` in the standard build, FFmpeg under `ffmpeg-video`);
    /// with none installed `pending_container_loads` never receives an entry
    /// (`__lumen_video_container_load` is registered only alongside a decoder).
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn tick_video_sessions(&mut self, elapsed_ms: u64) {
        let decoder = lumen_js::video_gif_store::get_video_decoder();

        // Drain pending load requests queued by JS `__lumen_video_container_load`.
        let loads: Vec<(u32, String)> = self
            .video_gif_store
            .pending_container_loads
            .lock()
            .unwrap()
            .drain(..)
            .collect();

        for (nid, src) in loads {
            // A fresh load for this node supersedes any earlier failure — a
            // `load()` retry after fixing `src` must not stay haunted by the
            // previous attempt (GAP-MEDIADECODE срез 9).
            self.video_gif_store.load_failures.lock().unwrap().remove(&nid);

            let base = match &self.source {
                PageSource::File(p) => ResourceBase::File(p.clone()),
                PageSource::Url { url, .. } => ResourceBase::Url(url.clone()),
                PageSource::Snapshot { base_url, .. } => ResourceBase::Url(base_url.clone()),
                PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => continue,
            };
            let base = self
                .layout_source
                .as_ref()
                .map(|ls| effective_base(&ls.document.lock().unwrap(), &base))
                .unwrap_or(base);
            // GAP-REFERRER срез 5: this producer's only point with a live
            // `&Document`, same read as `fetch_and_register_lazy_images`.
            let referrer_policy = self.layout_source.as_ref().map_or_else(
                lumen_network::ReferrerPolicy::default_policy,
                |ls| crate::resource_base::document_referrer_policy(&ls.document.lock().unwrap()),
            );

            // BUG-692: контейнерный путь `<video src>` — та же перезапись схемы
            // (`upgrade-insecure-requests`, Fetch §4.1 шаг 5), что у GIF-пути
            // выше; ключ `load_failures`/лог остаются на сыром `src`.
            let upgraded = self.layout_source.as_ref().and_then(|ls| {
                let doc = ls.document.lock().unwrap();
                let root = doc.root();
                let (policy, _) = crate::csp_enforce::document_csp_policy(&doc, root)?;
                crate::csp_enforce::upgrade_insecure_url(&policy, &base.resolve_str(&src))
            });
            let fetch_url: &str = upgraded.as_deref().unwrap_or(&src);

            let bytes = match crate::subresources::fetch_video_bytes(
                fetch_url,
                &base,
                &self.event_sink,
                Some(self.active_cookie_jar()),
                referrer_policy,
            ) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("video: пропуск {src}: {e}");
                    self.video_gif_store.load_failures.lock().unwrap().insert(nid, e.to_string());
                    self.request_redraw();
                    continue;
                }
            };

            let Some(decoder) = decoder.as_ref() else {
                let msg = "нет установленного VideoDecoder".to_string();
                eprintln!("video: {msg}: {src}");
                self.video_gif_store.load_failures.lock().unwrap().insert(nid, msg);
                self.request_redraw();
                continue;
            };
            let mut session = match decoder.open(&bytes) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("video: ошибка декодирования {src}: {e}");
                    // COMPAT-MEDIA-OSDEC-WIN-BAR: H.264/HEVC без декодера ОС.
                    if let Some(what) = lumen_media_native::missing_os_codec(&bytes)
                        && self.media_bar.note_missing(what)
                    {
                        self.relayout_chrome_host();
                    }
                    self.video_gif_store.load_failures.lock().unwrap().insert(nid, e.to_string());
                    self.request_redraw();
                    continue;
                }
            };

            let (width, height) = session.dimensions();
            let rgba = match session.frame_at(0.0) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("video: не декодирован первый кадр {src}: {e}");
                    self.video_gif_store.load_failures.lock().unwrap().insert(nid, e.to_string());
                    self.request_redraw();
                    continue;
                }
            };
            let image = Arc::new(lumen_image::Image {
                width,
                height,
                format: lumen_image::PixelFormat::Rgba8,
                data: rgba,
                icc_profile: None,
                vector_source: None,
            });
            // HTML LS §4.8.11.7: with a `poster` the poster stays the
            // "poster frame" until playback starts (or a seek), so the first
            // decoded frame is held back. Marking position 0 as already shown
            // keeps the paused tick below from registering it either; `play()`
            // or a seek moves the position and the tick draws the real frame.
            let has_poster = self.layout_source.as_ref().is_some_and(|ls| {
                let doc = ls.document.lock().unwrap();
                doc.get(lumen_dom::NodeId::from_index(nid as usize))
                    .get_attr("poster")
                    .is_some_and(|p| !p.trim().is_empty())
            });
            let key = format!("video:{nid}");
            if has_poster {
                // Frame withheld — see above.
            } else if let Some(r) = self.renderer.as_mut() {
                if let Err(e) = r.register_image(key.clone(), Arc::clone(&image)) {
                    eprintln!("video: не зарегистрирован {key}: {e}");
                }
            } else {
                self.pending_images.push((key.clone(), image));
            }
            if let Some(src_ref) = self.layout_source.as_ref() {
                let mut doc = src_ref.document.lock().unwrap();
                let node_id = lumen_dom::NodeId::from_index(nid as usize);
                if apply_intrinsic_size(&mut doc, node_id, width, height, self.relayout_viewport().unwrap_or(Size::new(0.0, 0.0))) {
                    self.note_shell_attr_writes(vec![nid]);
                }
            }
            let cycle_ms = session.duration_secs().map_or(0, |s| (s * 1000.0) as u64);
            eprintln!("video: загружен nid={nid} ({width}×{height}, {cycle_ms}мс)");
            self.video_gif_store.playback.lock().unwrap().insert(
                nid,
                lumen_js::video_gif_store::VideoPlaybackState {
                    paused: true,
                    position_ms: 0,
                    play_epoch_ms: None,
                    cycle_ms,
                    loop_count: 1,
                    width,
                    height,
                },
            );
            self.video_sessions.insert(nid, session);
            if has_poster {
                self.video_last_ms.insert(nid, 0);
            } else {
                self.video_last_ms.remove(&nid);
            }
            // GAP-MEDIADECODE срез 14: a reload of the same node (e.g. a new
            // `src`) must not keep feeding PCM decoded against the OLD
            // session into a sink still open from the previous one.
            self.video_audio_sinks.remove(&nid);
            self.video_last_audio_ms.remove(&nid);
            self.request_redraw();
        }

        // COMPAT-MSE: сессии поверх `MediaSource` заводятся здесь же, до цикла выдачи кадров.
        self.tick_mse_sessions();

        // Advance frames for playing container-backed videos. `playback` is the
        // store shared with the GIF path (comment on `VideoGifStore::playback`),
        // so filter to nodes this map actually owns a session for.
        //
        // A paused video is not skipped outright (GAP-MEDIADECODE срез 8):
        // `__lumen_video_seek` rewrites `position_ms` even while paused (HTML
        // spec: seeking a paused <video> must still show the frame at the new
        // position). While paused there is no continuous tick driving a
        // re-decode, so the 30fps throttle below — meant to cap re-decode
        // rate during smooth playback — would otherwise also suppress the one
        // decode a paused seek needs; skip it and decode unconditionally
        // whenever the position actually moved.
        let playback = self.video_gif_store.playback.lock().unwrap();
        let mut due: Vec<(u32, u64, bool)> = Vec::new();
        let mut next_frame_ms: Option<u64> = None;
        for (nid, state) in playback.iter() {
            if !self.video_sessions.contains_key(nid) {
                continue;
            }
            // GAP-MEDIADECODE срез 14: mirror pause onto any already-open PCM
            // sink immediately, not just when this node happens to be `due`
            // below — otherwise pausing mid-playback would let whatever PCM
            // is still queued keep audibly playing until it drains.
            if let Some(sink) = self.video_audio_sinks.get(nid) {
                sink.set_paused(state.paused);
                let (volume, muted) = self
                    .video_gif_store
                    .audio_levels
                    .lock()
                    .unwrap()
                    .get(nid)
                    .copied()
                    .unwrap_or((1.0, false));
                sink.set_volume(if muted { 0.0 } else { volume });
            }
            let rate = self
                .video_gif_store
                .playback_rates
                .lock()
                .unwrap()
                .get(nid)
                .copied()
                .unwrap_or(1.0);
            let cur_ms = state.current_ms(elapsed_ms, rate);
            let last = self.video_last_ms.get(nid).copied();
            if state.paused {
                if last != Some(cur_ms) {
                    due.push((*nid, cur_ms, false));
                }
                continue;
            }
            // GAP-MEDIADECODE срез 15: the JS `timeupdate` poller (250ms
            // period) calls `__lumen_video_pause` once it observes `ended`,
            // but that lags up to one poll tick behind. Stop advancing here
            // too, so a redraw tick in that window can't keep calling
            // `frame_at`/`decode_audio_pcm` past the demuxer's EOF.
            if state.is_ended(elapsed_ms, rate) {
                continue;
            }
            let (is_due, wait_ms) = playing_frame_wait(last, cur_ms);
            if is_due {
                due.push((*nid, cur_ms, true));
            }
            // Media milliseconds → wall clock at this node's playback rate.
            let wall_ms = (wait_ms as f64 / rate.abs().max(0.0625)).ceil() as u64;
            next_frame_ms = Some(next_frame_ms.map_or(wall_ms, |m: u64| m.min(wall_ms)));
        }
        drop(playback);

        for (nid, cur_ms, is_playing) in due {
            let Some(session) = self.video_sessions.get_mut(&nid) else { continue };
            let secs = cur_ms as f64 / 1000.0;
            let frame_result = session.frame_at(secs);
            // GAP-MEDIADECODE срез 14: decode the audio track's next PCM
            // chunk right AFTER `frame_at`, not before. `frame_at` always
            // re-seeks the demuxer to `secs` and decodes forward
            // (`VideoDecodeSession::frame_at`); `decode_audio_pcm`
            // has no seek of its own and just continues reading from wherever
            // that left the shared read cursor. Calling it right after keeps
            // the audio chunk anchored close to the position just shown,
            // instead of drifting from an independently-advancing cursor.
            // Only for playing nodes — a paused seek redraws the frame but
            // must not make a sound.
            let pcm_chunk = is_playing
                .then(|| session.audio_track())
                .flatten()
                .and_then(|track| {
                    let last_audio_ms = self
                        .video_last_audio_ms
                        .get(&nid)
                        .copied()
                        .unwrap_or_else(|| cur_ms.saturating_sub(33));
                    // Cap a single chunk's span so a long stall (window
                    // minimized, a slow tick) cannot force one huge decode.
                    let ms_elapsed = cur_ms.saturating_sub(last_audio_ms).clamp(1, 500);
                    let samples_per_channel =
                        ((ms_elapsed as f64 / 1000.0) * f64::from(track.sample_rate)).round() as usize;
                    if samples_per_channel == 0 {
                        return None;
                    }
                    match session.decode_audio_pcm(samples_per_channel) {
                        Ok(samples) => Some((track, samples)),
                        Err(e) => {
                            eprintln!("video аудио: ошибка декодирования nid={nid}: {e}");
                            None
                        }
                    }
                });

            match frame_result {
                Ok(rgba) => {
                    let (width, height) = session.dimensions();
                    let key = format!("video:{nid}");
                    if let Some(r) = self.renderer.as_mut()
                        && let Err(e) = r.register_image(
                            key.clone(),
                            Arc::new(lumen_image::Image {
                                width,
                                height,
                                format: lumen_image::PixelFormat::Rgba8,
                                data: rgba,
                                icc_profile: None,
                                vector_source: None,
                            }),
                        )
                    {
                        eprintln!("video кадр {key}: {e}");
                    }
                    self.video_last_ms.insert(nid, cur_ms);
                }
                Err(e) => eprintln!("video: ошибка кадра nid={nid}: {e}"),
            }

            if let Some((track, samples)) = pcm_chunk {
                // A missing/unavailable audio device is retried every due
                // tick (no "already failed" marker) — same non-dedup stance
                // the video path already takes for `frame_at` errors just
                // above, and simpler than tracking a second failure set for
                // a rare, non-fatal condition.
                if let std::collections::hash_map::Entry::Vacant(e) =
                    self.video_audio_sinks.entry(nid)
                {
                    match crate::platform::video_audio_sink::VideoPcmAudioSink::new() {
                        Some(sink) => {
                            eprintln!(
                                "video аудио: открыт вывод nid={nid} ({} Гц, {} кан.)",
                                track.sample_rate, track.channels
                            );
                            // Apply whatever `volume`/`muted` the page had
                            // already set before this sink existed — the
                            // per-tick mirror above only reaches sinks that
                            // were already open at the start of the tick.
                            let (volume, muted) = self
                                .video_gif_store
                                .audio_levels
                                .lock()
                                .unwrap()
                                .get(&nid)
                                .copied()
                                .unwrap_or((1.0, false));
                            sink.set_volume(if muted { 0.0 } else { volume });
                            e.insert(sink);
                        }
                        None => eprintln!("video аудио: нет аудио-устройства nid={nid}"),
                    }
                }
                if let Some(sink) = self.video_audio_sinks.get(&nid) {
                    sink.push_pcm(samples, track.sample_rate, track.channels);
                }
                self.video_last_audio_ms.insert(nid, cur_ms);
            }
        }

        // Кадры этого тика уже в отрисовке этого же кадра (в том числе кадр
        // перемотки на паузе); пока ролик играет — проснуться к следующему.
        if let Some(ms) = next_frame_ms {
            self.schedule_video_redraw(std::time::Duration::from_millis(ms.max(1)).min(VIDEO_POLL));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{playing_frame_wait, VIDEO_FRAME_MS};

    #[test]
    fn first_frame_is_due_at_once() {
        assert_eq!(playing_frame_wait(None, 0), (true, VIDEO_FRAME_MS));
    }

    #[test]
    fn next_frame_waits_out_the_30fps_cap() {
        assert_eq!(playing_frame_wait(Some(1_000), 1_010), (false, VIDEO_FRAME_MS - 10));
        assert_eq!(playing_frame_wait(Some(1_000), 1_000 + VIDEO_FRAME_MS), (true, VIDEO_FRAME_MS));
    }

    /// `loop` restart: last shown frame at the end of the cycle, position back
    /// near zero — the frame is due now, not after another full cycle.
    #[test]
    fn position_behind_the_last_frame_is_due_at_once() {
        assert_eq!(playing_frame_wait(Some(6_034), 12), (true, VIDEO_FRAME_MS));
    }
}
