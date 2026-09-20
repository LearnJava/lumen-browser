//! HTMLVideoElement JS bindings — Phase 1 (animated GIF playback).
//!
//! Upgrades the Phase 0 stub so that `<video src="*.gif">` files play back
//! as animated GIFs.  Non-GIF sources retain Phase 0 behaviour (immediate
//! resolved-Promise play, no real decode).
//!
//! # Architecture
//!
//! The shell decodes animated GIFs and stores them in [`VideoGifStore`]
//! (installed globally via [`set_video_gif_store`]).  Each `<video>` DOM
//! node is keyed by its `__nid__` (DOM node index).
//!
//! The JS shim calls `__lumen_video_load(nid, src)` to queue a GIF load;
//! the shell fetches + decodes on the next tick and inserts an entry into
//! the store.  JS polls `__lumen_video_ready(nid)` until true, then fires
//! `loadedmetadata` / `canplay`.  Playback is controlled via
//! `__lumen_video_play` / `__lumen_video_pause` / `__lumen_video_seek`.
//!
//! # Registered native bindings
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `__lumen_video_load` | `(nid: f64, src: String)` | Queue GIF load |
//! | `__lumen_video_ready` | `(nid: f64) → bool` | GIF decoded and ready? |
//! | `__lumen_video_failed` | `(nid: f64) → bool` | Queued load failed (GAP-MEDIADECODE срез 9)? |
//! | `__lumen_video_play` | `(nid: f64, now_ms: f64)` | Start/resume |
//! | `__lumen_video_pause` | `(nid: f64, now_ms: f64)` | Pause |
//! | `__lumen_video_seek` | `(nid: f64, secs: f64, now_ms: f64)` | Seek |
//! | `__lumen_video_current_time` | `(nid: f64, now_ms: f64) → f64` | Position (s) |
//! | `__lumen_video_duration` | `(nid: f64) → f64` | Duration (s), Inf for loops |
//! | `__lumen_video_paused` | `(nid: f64) → bool` | Is paused? |
//! | `__lumen_video_ended` | `(nid: f64, now_ms: f64) → bool` | Has ended? |
//! | `__lumen_video_width` | `(nid: f64) → f64` | GIF pixel width |
//! | `__lumen_video_height` | `(nid: f64) → f64` | GIF pixel height |
//! | `__lumen_video_set_volume` | `(nid: f64, volume: f64)` | Route `video.volume =` to the audio sink |
//! | `__lumen_video_set_muted` | `(nid: f64, muted: bool)` | Route `video.muted =` to the audio sink |
//! | `__lumen_video_set_playback_rate` | `(nid: f64, rate: f64, now_ms: f64)` | Route `video.playbackRate =` to the `currentTime` timer |
//! | `__lumen_video_can_play_type` | `(mime: String) → String` | canPlayType probe |
//! | `__lumen_video_ffmpeg_load` | `(nid: f64, src: String)` | Queue FFmpeg-container load (feature `ffmpeg-video`, GAP-MEDIADECODE срез 6) |
//! | `__lumen_texttracks_json` | `(nid: f64) → String` | JSON of parsed `<track>` cues |
//! | `__lumen_vtt_parse` | `(text: String) → String` | Parse a WebVTT file (BUG-775) |
//!
//! # The `HTMLMediaElement` state machine (BUG-825)
//!
//! Everything above is about *playback*; the shim also owns the element's
//! HTML §4.8.11 state — `networkState` / `readyState` / `currentSrc` / `error`,
//! `volume` / `muted` / `playbackRate`, and the media load + resource selection
//! algorithms behind `src =`, `load()` and `<source>` children.  It lives here
//! rather than in `dom.rs` because that is where the GIF loader it feeds is,
//! and it is also where `HTMLMediaElement` itself and `MediaError` are defined
//! (`dom.rs` builds `HTMLVideoElement`/`HTMLAudioElement` straight off
//! `HTMLElement`, so the constants had no interface to live on).
//!
//! Only an animated GIF is decodable by default, so resource selection ends in
//! the spec's «dedicated media source failure steps» for every other format —
//! `loadstart` then `error` with `MEDIA_ERR_SRC_NOT_SUPPORTED` — which is what
//! `canPlayType` has always said about them. GAP-MEDIADECODE срез 6 adds the
//! `ffmpeg-video` feature (off by default): with it, `canPlayType` answers
//! "maybe" for `video/mp4`/`video/webm`/`video/ogg` and the shim queues loads
//! via `__lumen_video_ffmpeg_load` into the same store's `pending_ffmpeg_loads`
//! — but nothing drains that queue yet (срез 7, shell-side `lumen-media-ffmpeg`
//! tick, not written), so builds with the feature on would stall on those
//! sources exactly like an unhandled GIF load would. The feature exists so the
//! JS-side plumbing can be reviewed and tested in isolation before it is wired
//! to a real decoder.  Every media event is *queued*,
//! never dispatched inline: the near-universal `e.volume = 0.5;
//! e.onvolumechange = …` order sees nothing at all from a synchronous
//! dispatch.  `<audio>` keeps its own, older model in `audio_element.rs` and
//! still dispatches synchronously.

#[cfg(feature = "v8-backend")]
use crate::text_track_store::get_text_track_store;
#[cfg(feature = "v8-backend")]
use crate::video_gif_store::get_video_gif_store;

/// Mime types `FfmpegVideoDecoder::mime_types()` claims
/// (`crates/engine/media-ffmpeg/src/decoder.rs`), duplicated here rather than
/// imported: this crate does not depend on `lumen-media-ffmpeg` (decode stays
/// shell-side), so the list is a plain string match kept in sync by hand.
/// Behind the `ffmpeg-video` feature so `canPlayType` keeps answering `""`
/// for these mimes until the shell (срез 7) can actually decode them.
#[cfg(all(feature = "v8-backend", feature = "ffmpeg-video"))]
fn is_ffmpeg_mime(base: &str) -> bool {
    matches!(base, "video/mp4" | "video/webm" | "video/ogg")
}

#[cfg(all(feature = "v8-backend", not(feature = "ffmpeg-video")))]
fn is_ffmpeg_mime(_base: &str) -> bool {
    false
}

/// Current `video.playbackRate` for `nid`, spec default `1.0` when unset —
/// GAP-MEDIADECODE срез 18. Pulled out because every reader of
/// `VideoPlaybackState::current_ms`/`is_ended`/`freeze` in this file needs
/// the same lookup.
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
fn playback_rate_of(store: &std::sync::Arc<crate::video_gif_store::VideoGifStore>, nid: u32) -> f64 {
    store
        .playback_rates
        .lock()
        .unwrap()
        .get(&nid)
        .copied()
        .unwrap_or(1.0)
}

/// V8 port of `install_video_bindings` (Ph3 V8 migration S5-S7 batch 3; the
/// rquickjs twin was removed in S12b-B22): state is the process-global
/// [`VideoGifStore`](crate::video_gif_store::VideoGifStore) (installed once
/// via `set_video_gif_store`, backend-agnostic), so no new `V8JsRuntime`
/// plumbing is needed — each native captures its own `get_video_gif_store()`
/// clone exactly like the rquickjs original. The JS shim is unchanged.
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_video_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn1, into_v8_fn2, into_v8_fn3};
    use lumen_core::ext::JsRuntime as _;

    {
        let store = get_video_gif_store();
        let load = into_v8_fn2(move |nid: f64, src: String| {
            if let Some(s) = &store {
                s.pending_loads.lock().unwrap().push((nid as u32, src));
            }
        });
        rt.register_native("__lumen_video_load", load)?;
    }

    {
        let store = get_video_gif_store();
        let ready = into_v8_fn1(move |nid: f64| -> bool {
            store
                .as_ref()
                .map(|s| s.playback.lock().unwrap().contains_key(&(nid as u32)))
                .unwrap_or(false)
        });
        rt.register_native("__lumen_video_ready", ready)?;
    }

    // GAP-MEDIADECODE срез 9: whether the load queued for `nid` failed
    // (currently only the FFmpeg tick writes into `load_failures` — a
    // corrupted/undecodable container never reaches `playback`, so without
    // this the shim's poll loop would spin on `__lumen_video_ready` forever
    // instead of dispatching `error`). Registered unconditionally, like
    // `ready` above, since it only reads shared state and costs nothing
    // when the `ffmpeg-video` feature is off (the map is simply never
    // written to).
    {
        let store = get_video_gif_store();
        let failed = into_v8_fn1(move |nid: f64| -> bool {
            store
                .as_ref()
                .map(|s| s.load_failures.lock().unwrap().contains_key(&(nid as u32)))
                .unwrap_or(false)
        });
        rt.register_native("__lumen_video_failed", failed)?;
    }

    {
        let store = get_video_gif_store();
        let play = into_v8_fn2(move |nid: f64, now_ms: f64| {
            if let Some(s) = &store
                && let Some(e) = s.playback.lock().unwrap().get_mut(&(nid as u32))
                && e.paused
            {
                e.play_epoch_ms = Some(now_ms as u64);
                e.paused = false;
            }
        });
        rt.register_native("__lumen_video_play", play)?;
    }

    {
        let store = get_video_gif_store();
        let pause = into_v8_fn2(move |nid: f64, now_ms: f64| {
            if let Some(s) = &store {
                let rate = playback_rate_of(s, nid as u32);
                if let Some(e) = s.playback.lock().unwrap().get_mut(&(nid as u32)) {
                    e.freeze(now_ms as u64, rate);
                    e.paused = true;
                }
            }
        });
        rt.register_native("__lumen_video_pause", pause)?;
    }

    {
        let store = get_video_gif_store();
        let seek = into_v8_fn3(move |nid: f64, secs: f64, now_ms: f64| {
            if let Some(s) = &store
                && let Some(e) = s.playback.lock().unwrap().get_mut(&(nid as u32))
            {
                let target_ms = (secs * 1000.0).max(0.0) as u64;
                e.position_ms = target_ms;
                if !e.paused {
                    e.play_epoch_ms = Some(now_ms as u64);
                }
            }
        });
        rt.register_native("__lumen_video_seek", seek)?;
    }

    {
        let store = get_video_gif_store();
        let current_time = into_v8_fn2(move |nid: f64, now_ms: f64| -> f64 {
            store
                .as_ref()
                .and_then(|s| {
                    let rate = playback_rate_of(s, nid as u32);
                    s.playback
                        .lock()
                        .unwrap()
                        .get(&(nid as u32))
                        .map(|e| e.current_ms(now_ms as u64, rate) as f64 / 1000.0)
                })
                .unwrap_or(0.0)
        });
        rt.register_native("__lumen_video_current_time", current_time)?;
    }

    {
        let store = get_video_gif_store();
        let duration = into_v8_fn1(move |nid: f64| -> f64 {
            store
                .as_ref()
                .and_then(|s| {
                    s.playback
                        .lock()
                        .unwrap()
                        .get(&(nid as u32))
                        .map(|e| e.duration_secs())
                })
                .unwrap_or(f64::INFINITY)
        });
        rt.register_native("__lumen_video_duration", duration)?;
    }

    {
        let store = get_video_gif_store();
        let paused = into_v8_fn1(move |nid: f64| -> bool {
            store
                .as_ref()
                .and_then(|s| s.playback.lock().unwrap().get(&(nid as u32)).map(|e| e.paused))
                .unwrap_or(true)
        });
        rt.register_native("__lumen_video_paused", paused)?;
    }

    {
        let store = get_video_gif_store();
        let ended = into_v8_fn2(move |nid: f64, now_ms: f64| -> bool {
            store
                .as_ref()
                .and_then(|s| {
                    let rate = playback_rate_of(s, nid as u32);
                    s.playback
                        .lock()
                        .unwrap()
                        .get(&(nid as u32))
                        .map(|e| e.is_ended(now_ms as u64, rate))
                })
                .unwrap_or(false)
        });
        rt.register_native("__lumen_video_ended", ended)?;
    }

    {
        let store = get_video_gif_store();
        let width = into_v8_fn1(move |nid: f64| -> f64 {
            store
                .as_ref()
                .and_then(|s| {
                    s.playback
                        .lock()
                        .unwrap()
                        .get(&(nid as u32))
                        .map(|e| f64::from(e.width))
                })
                .unwrap_or(0.0)
        });
        rt.register_native("__lumen_video_width", width)?;
    }

    {
        let store = get_video_gif_store();
        let height = into_v8_fn1(move |nid: f64| -> f64 {
            store
                .as_ref()
                .and_then(|s| {
                    s.playback
                        .lock()
                        .unwrap()
                        .get(&(nid as u32))
                        .map(|e| f64::from(e.height))
                })
                .unwrap_or(0.0)
        });
        rt.register_native("__lumen_video_height", height)?;
    }

    // GAP-MEDIADECODE, остаток среза 15: route `video.volume =`/`video.muted =`
    // to the FFmpeg audio sink. Written to `audio_levels`, not `playback`,
    // because `playback` entries are wholesale replaced on every decode
    // completion (see the field doc on `VideoGifStore::audio_levels`) — a
    // write here must survive that.
    {
        let store = get_video_gif_store();
        let set_volume = into_v8_fn2(move |nid: f64, volume: f64| {
            if let Some(s) = &store {
                let mut levels = s.audio_levels.lock().unwrap();
                let entry = levels.entry(nid as u32).or_insert((1.0, false));
                entry.0 = volume as f32;
            }
        });
        rt.register_native("__lumen_video_set_volume", set_volume)?;
    }

    {
        let store = get_video_gif_store();
        let set_muted = into_v8_fn2(move |nid: f64, muted: bool| {
            if let Some(s) = &store {
                let mut levels = s.audio_levels.lock().unwrap();
                let entry = levels.entry(nid as u32).or_insert((1.0, false));
                entry.1 = muted;
            }
        });
        rt.register_native("__lumen_video_set_muted", set_muted)?;
    }

    // GAP-MEDIADECODE срез 18: route `video.playbackRate =` to the native
    // `currentTime` timer. The old rate must be read and applied via
    // `freeze()` BEFORE the new rate is stored — otherwise the elapsed
    // portion since `play_epoch_ms` would get retroactively rescaled by the
    // new rate on the very next `current_ms` call, corrupting whatever
    // position had already accumulated under the old rate (same reasoning
    // as why `seek` re-anchors the epoch instead of touching `position_ms`
    // under the existing rate).
    {
        let store = get_video_gif_store();
        let set_rate = into_v8_fn3(move |nid: f64, rate: f64, now_ms: f64| {
            if let Some(s) = &store {
                let nid = nid as u32;
                let old_rate = playback_rate_of(s, nid);
                if let Some(e) = s.playback.lock().unwrap().get_mut(&nid)
                    && !e.paused
                {
                    e.freeze(now_ms as u64, old_rate);
                    e.play_epoch_ms = Some(now_ms as u64);
                }
                s.playback_rates.lock().unwrap().insert(nid, rate);
            }
        });
        rt.register_native("__lumen_video_set_playback_rate", set_rate)?;
    }

    {
        let can_play_type = into_v8_fn1(move |mime: String| -> String {
            let m = mime.trim().to_ascii_lowercase();
            let base = m.split(';').next().unwrap_or("").trim();
            if base == "image/gif" || is_ffmpeg_mime(base) {
                "maybe".to_string()
            } else {
                String::new()
            }
        });
        rt.register_native("__lumen_video_can_play_type", can_play_type)?;
    }

    // GAP-MEDIADECODE срез 6: queue a container load for the shell's (not yet
    // written, срез 7) FFmpeg tick. Mirrors `__lumen_video_load` exactly —
    // same `pending_*` queue shape, same store, no decode here.
    #[cfg(feature = "ffmpeg-video")]
    {
        let store = get_video_gif_store();
        let ffmpeg_load = into_v8_fn2(move |nid: f64, src: String| {
            if let Some(s) = &store {
                s.pending_ffmpeg_loads.lock().unwrap().push((nid as u32, src));
            }
        });
        rt.register_native("__lumen_video_ffmpeg_load", ffmpeg_load)?;
    }

    {
        let store = get_text_track_store();
        let texttracks_json = into_v8_fn1(move |nid: f64| -> String {
            store
                .as_ref()
                .map(|s| s.tracks_json(nid as u32))
                .unwrap_or_else(|| "[]".to_string())
        });
        rt.register_native("__lumen_texttracks_json", texttracks_json)?;
    }

    // BUG-775: `<track>` elements minted with `document.createElement` are
    // loaded by the shim itself — the shell's `tracks::load_video_tracks` only
    // ever walks the *parsed* document, once per navigation. The WebVTT parser
    // must not become a second implementation on the JS side, so the shim hands
    // the file body back here and gets the same `lumen_dom::vtt::parse_vtt` the
    // shell uses. A parse error is reported as `ok: false` rather than as an
    // empty cue list: HTML §4.8.11.1 fires `error` for «not a valid WebVTT
    // file» and `load` for a valid file that happens to declare no cues, and
    // `webvtt/parsing/file-parsing/signature-invalid.html` asserts exactly that
    // split over eleven malformed headers.
    {
        let vtt_parse = into_v8_fn1(move |text: String| -> String {
            match lumen_dom::vtt::parse_vtt(&text) {
                Ok(cues) => {
                    let arr: Vec<serde_json::Value> = cues
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "id": c.id.clone().unwrap_or_default(),
                                "start": c.start_s,
                                "end": c.end_s,
                                "text": c.text,
                            })
                        })
                        .collect();
                    serde_json::json!({ "ok": true, "cues": arr }).to_string()
                }
                Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
            }
        });
        rt.register_native("__lumen_vtt_parse", vtt_parse)?;
    }

    rt.eval(VIDEO_SHIM)?;
    Ok(())
}

// ── JavaScript shim ───────────────────────────────────────────────────────────

/// HTMLVideoElement Phase 1 shim, verbatim `include_str!` (GAP-MEDIADECODE
/// срез 5, mirrors the `dom.rs` SPLIT-JS3 split): this file had grown past
/// the 2000-line cap, so the JS payload moved to `shim/video_element.js` and
/// is read as-is — edit the `.js`, not this Rust module.
///
/// Uses `__lumen_video_*` native bindings for GIF-backed playback.  Falls
/// back to Phase 0 behaviour when the store is absent (headless/CI) or when
/// the `src` is not a `.gif` URL.
#[cfg(feature = "v8-backend")]
const VIDEO_SHIM: &str = include_str!("shim/video_element.js");

/// V8 test coverage for the `HTMLVideoElement` shim (the rquickjs twin was
/// removed in S12b-B22; this module ports its 12 tests to V8 verbatim).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use std::sync::{Arc, Mutex};

    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    use crate::v8_runtime::V8JsRuntime;

    use super::*;

    /// Serializes tests that install and read the process-global
    /// [`crate::video_gif_store`] / [`crate::text_track_store`] singletons.
    /// Without this guard, parallel tests race: one test's `set_*_store`
    /// overwrites the global between another test's own `set` and the
    /// `install`/`load` that captures it, so the load lands in the wrong
    /// store (BUG-166).
    static STORE_GUARD: Mutex<()> = Mutex::new(());

    /// Minimal DOM stubs so the shim can run without the full DOM bridge.
    fn install_minimal_dom(rt: &V8JsRuntime) {
        rt.eval(
            r#"
var document = {
  querySelectorAll: function() { return []; },
  createElement: function(tag) {
    var attrs = {};
    return {
      __nid__: 42,
      getAttribute: function(k){ return attrs[k] || ''; },
      setAttribute: function(k,v){ attrs[k]=v; },
      hasAttribute: function(k){ return !!attrs[k]; },
      dispatchEvent: function(){}
    };
  }
};
"#,
        )
        .unwrap();
    }

    fn with_video() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        install_video_bindings_v8(&rt).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn install_succeeds_without_document() {
        let rt = V8JsRuntime::new().unwrap();
        install_video_bindings_v8(&rt).expect("install should succeed without document");
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        install_video_bindings_v8(&rt).expect("install should succeed with minimal dom");
    }

    #[test]
    fn play_returns_promise() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); el.play() instanceof Promise",
        );
        assert!(ok, "play() should return a Promise");
    }

    /// §4.8.11.6: with no media resource the duration is NaN, not Infinity —
    /// the old stub's Infinity read as «an endless live stream is loaded».
    #[test]
    fn duration_nan_without_resource() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); Number.isNaN(el.duration)",
        );
        assert!(ok, "duration should be NaN while readyState is HAVE_NOTHING");
    }

    #[test]
    fn paused_initially_true() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); el.paused === true",
        );
        assert!(ok, "paused should initially be true");
    }

    /// BUG-825: a fresh `<video>` used to report HAVE_ENOUGH_DATA — «the
    /// resource is fully loaded» — before anything had been assigned to it.
    #[test]
    fn ready_state_with_no_src() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video');
             el.readyState === 0 && el.networkState === 0
               && el.currentSrc === '' && el.error === null",
        );
        assert!(ok, "a fresh <video> is HAVE_NOTHING / NETWORK_EMPTY with no source");
    }

    #[test]
    fn can_play_type_gif() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); el.canPlayType('image/gif') === 'maybe'",
        );
        assert!(ok, "canPlayType('image/gif') should return 'maybe'");
    }

    // These mp4/webm assertions are specifically about the *default* (no
    // `ffmpeg-video`) contract; with the feature on, `canPlayType` legitimately
    // answers "maybe" instead (see `can_play_type_ffmpeg_mimes_maybe_with_feature`).
    #[cfg(not(feature = "ffmpeg-video"))]
    #[test]
    fn can_play_type_mp4_empty() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); el.canPlayType('video/mp4') === ''",
        );
        assert!(ok, "canPlayType('video/mp4') should return ''");
    }

    /// GAP-MEDIADECODE срез 6: with the feature off (the default), FFmpeg
    /// containers must still fail cleanly — the whole point of gating
    /// `canPlayType` on `ffmpeg-video` is that a default build never claims a
    /// format it cannot decode.
    #[cfg(not(feature = "ffmpeg-video"))]
    #[test]
    fn can_play_type_webm_ogg_empty_without_feature() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video');
             el.canPlayType('video/webm') === '' && el.canPlayType('video/ogg') === ''",
        );
        assert!(ok, "canPlayType should stay '' for FFmpeg mimes without the ffmpeg-video feature");
    }

    #[test]
    fn native_video_load_registers_pending() {
        use crate::video_gif_store::set_video_gif_store;
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(crate::video_gif_store::VideoGifStore::default());
        set_video_gif_store(store.clone());

        let rt = V8JsRuntime::new().unwrap();
        install_video_bindings_v8(&rt).unwrap();
        rt.eval("__lumen_video_load(99, 'test.gif');").unwrap();

        let loads = store.pending_loads.lock().unwrap();
        assert!(!loads.is_empty(), "load should be queued");
        assert!(loads.iter().any(|(n, s)| *n == 99 && s == "test.gif"));
    }

    #[test]
    fn text_tracks_exposed_from_store() {
        use crate::text_track_store::{
            set_text_track_store, CueData, TextTrackData, TextTrackStore,
        };
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let tstore = Arc::new(TextTrackStore::default());
        tstore.tracks.lock().unwrap().insert(
            42,
            vec![TextTrackData {
                kind: "subtitles".to_string(),
                label: "English".to_string(),
                language: "en".to_string(),
                mode: "showing".to_string(),
                cues: vec![CueData {
                    id: "c1".to_string(),
                    start: 0.0,
                    end: 5.0,
                    text: "Hi".to_string(),
                }],
            }],
        );
        set_text_track_store(tstore);

        let rt = with_video();
        let ok = bool_eval(
            &rt,
            r#"
var el = document.createElement('video');
var tt = el.textTracks;
tt.length === 1
  && tt[0].kind === 'subtitles'
  && tt[0].language === 'en'
  && tt[0].mode === 'showing'
  && tt[0].cues.length === 1
  && tt[0].cues[0].text === 'Hi'
  && tt[0].cues[0].startTime === 0
  && tt[0].cues[0].endTime === 5
  && tt[0].activeCues.length === 1
  && tt.getTrackById('') === tt[0]
"#,
        );
        assert!(ok, "textTracks should expose the shell-parsed cues");
    }

    #[test]
    fn text_tracks_empty_without_store_entry() {
        use crate::text_track_store::{set_text_track_store, TextTrackStore};
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        // Fresh empty store so a prior test's nid=42 entry can't leak in.
        set_text_track_store(Arc::new(TextTrackStore::default()));

        let rt = with_video();
        let len = rt
            .eval("document.createElement('video').textTracks.length")
            .unwrap();
        assert_eq!(len, JsValue::Number(0.0), "no store entry → empty TextTrackList");
    }

    /// GAP-MEDIADECODE срез 6: with `ffmpeg-video` on, `canPlayType` claims the
    /// three container mimes `FfmpegVideoDecoder::mime_types()` decodes.
    #[cfg(feature = "ffmpeg-video")]
    #[test]
    fn can_play_type_ffmpeg_mimes_maybe_with_feature() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video');
             el.canPlayType('video/mp4') === 'maybe'
               && el.canPlayType('video/webm') === 'maybe'
               && el.canPlayType('video/ogg') === 'maybe'
               && el.canPlayType('video/mp4; codecs=\"avc1.42E01E\"') === 'maybe'",
        );
        assert!(ok, "canPlayType should answer 'maybe' for FFmpeg-decodable containers");
    }

    /// The load native queues into the *same* store the GIF loader uses
    /// (`pending_ffmpeg_loads`, not `pending_loads`) — no new store type.
    #[cfg(feature = "ffmpeg-video")]
    #[test]
    fn native_video_ffmpeg_load_registers_pending() {
        use crate::video_gif_store::set_video_gif_store;
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(crate::video_gif_store::VideoGifStore::default());
        set_video_gif_store(store.clone());

        let rt = V8JsRuntime::new().unwrap();
        install_video_bindings_v8(&rt).unwrap();
        rt.eval("__lumen_video_ffmpeg_load(99, 'test.mp4');").unwrap();

        let loads = store.pending_ffmpeg_loads.lock().unwrap();
        assert!(!loads.is_empty(), "ffmpeg load should be queued");
        assert!(loads.iter().any(|(n, s)| *n == 99 && s == "test.mp4"));
        assert!(store.pending_loads.lock().unwrap().is_empty(), "must not touch the GIF queue");
    }

    #[test]
    fn native_video_ready_false_before_decode() {
        use crate::video_gif_store::set_video_gif_store;
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(crate::video_gif_store::VideoGifStore::default());
        set_video_gif_store(store.clone());

        let rt = V8JsRuntime::new().unwrap();
        install_video_bindings_v8(&rt).unwrap();
        let ready = rt.eval("__lumen_video_ready(55)").unwrap();
        assert_eq!(ready, JsValue::Bool(false), "should not be ready before decode");
    }

    /// GAP-MEDIADECODE срез 9: `__lumen_video_failed` reads `load_failures`
    /// directly — no tick loop involved, this only checks the native binding
    /// wires to the right store field.
    #[test]
    fn native_video_failed_reflects_load_failures() {
        use crate::video_gif_store::set_video_gif_store;
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(crate::video_gif_store::VideoGifStore::default());
        store.load_failures.lock().unwrap().insert(77, "corrupt container".to_string());
        set_video_gif_store(store.clone());

        let rt = V8JsRuntime::new().unwrap();
        install_video_bindings_v8(&rt).unwrap();
        let failed_77 = rt.eval("__lumen_video_failed(77)").unwrap();
        let failed_78 = rt.eval("__lumen_video_failed(78)").unwrap();
        assert_eq!(failed_77, JsValue::Bool(true), "77 has a recorded failure");
        assert_eq!(failed_78, JsValue::Bool(false), "78 has no failure recorded");
    }

    /// GAP-MEDIADECODE срез 18: `__lumen_video_set_playback_rate` scales the
    /// elapsed portion of `currentTime` going forward, without retroactively
    /// touching the position already accumulated under the old rate.
    #[test]
    fn native_video_set_playback_rate_scales_current_time() {
        use crate::video_gif_store::{set_video_gif_store, VideoGifStore, VideoPlaybackState};
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(VideoGifStore::default());
        store.playback.lock().unwrap().insert(1, VideoPlaybackState {
            paused: false,
            position_ms: 0,
            play_epoch_ms: Some(1000),
            cycle_ms: 0,
            loop_count: 0,
            width: 0,
            height: 0,
        });
        set_video_gif_store(store.clone());

        let rt = V8JsRuntime::new().unwrap();
        install_video_bindings_v8(&rt).unwrap();
        // Doubling the rate exactly at the existing epoch must not shift the
        // position already accumulated (still 0 at this instant).
        rt.eval("__lumen_video_set_playback_rate(1, 2.0, 1000);").unwrap();
        let before = rt.eval("__lumen_video_current_time(1, 1000)").unwrap();
        assert_eq!(before, JsValue::Number(0.0), "rate change must not jump currentTime");

        // 1000ms of real time later, the doubled rate should read as 2s, not 1s.
        let after = rt.eval("__lumen_video_current_time(1, 2000)").unwrap();
        assert_eq!(after, JsValue::Number(2.0), "elapsed time after the change should be scaled by the new rate");
    }

    /// A paused node's `position_ms` must not shift when the rate changes —
    /// only the elapsed-since-epoch portion is scaled, and a paused node has
    /// no epoch.
    #[test]
    fn native_video_set_playback_rate_does_not_move_paused_position() {
        use crate::video_gif_store::{set_video_gif_store, VideoGifStore, VideoPlaybackState};
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(VideoGifStore::default());
        store.playback.lock().unwrap().insert(1, VideoPlaybackState {
            paused: true,
            position_ms: 5000,
            play_epoch_ms: None,
            cycle_ms: 0,
            loop_count: 0,
            width: 0,
            height: 0,
        });
        set_video_gif_store(store.clone());

        let rt = V8JsRuntime::new().unwrap();
        install_video_bindings_v8(&rt).unwrap();
        rt.eval("__lumen_video_set_playback_rate(1, 2.0, 1000);").unwrap();
        let cur = rt.eval("__lumen_video_current_time(1, 9000)").unwrap();
        assert_eq!(cur, JsValue::Number(5.0), "a paused node's position must be unaffected by rate");
    }

    // ── BUG-825: the HTMLMediaElement state machine on <video> ────────────────
    //
    // These need the real DOM (the stub above has no `Event`, no listener
    // registry and no timers), so they go through `install_dom`.
    mod media_element {
        use std::sync::{Arc, Mutex};

        use lumen_core::ext::JsRuntime as _;
        use lumen_dom::{Document, QualName};

        use crate::v8_runtime::V8JsRuntime;

        fn rt_with_dom() -> V8JsRuntime {
            let mut doc = Document::new();
            let html = doc.create_element(QualName::html("html"));
            let body = doc.create_element(QualName::html("body"));
            doc.append_child(doc.root(), html);
            doc.append_child(html, body);
            let rt = V8JsRuntime::new().unwrap();
            rt.install_dom(
                Arc::new(Mutex::new(doc)),
                "",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            )
            .unwrap();
            rt
        }

        /// Turn the event loop far enough for the queued media tasks (and the
        /// tasks they queue in turn) to run.
        fn settle(rt: &V8JsRuntime) {
            for _ in 0..8 {
                rt.eval("_lumen_tick_timers()").unwrap();
            }
        }

        fn truthy(rt: &V8JsRuntime, expr: &str) -> bool {
            matches!(rt.eval(expr).unwrap(), lumen_core::JsValue::Bool(true))
        }

        /// The shape `event_volumechange.html` uses: the handler is armed on the
        /// line *after* the assignment, so a synchronous dispatch reaches
        /// nobody. This is why the event is queued rather than fired inline.
        #[test]
        fn volume_change_is_queued_so_a_later_handler_still_sees_it() {
            let rt = rt_with_dom();
            rt.eval(
                "var seen = [];
                 var v = document.createElement('video');
                 v.volume = 0.5;
                 v.onvolumechange = function() { seen.push(['on', v.volume]); };
                 v.addEventListener('volumechange', function() { seen.push(['listener', v.volume]); });",
            )
            .unwrap();
            assert!(truthy(&rt, "seen.length === 0"), "volumechange fired synchronously");
            settle(&rt);
            assert!(
                truthy(
                    &rt,
                    "seen.length === 2 && seen[0][1] === 0.5 && seen[1][1] === 0.5"
                ),
                "both handler forms must see the queued volumechange"
            );
        }

        /// `muted` is the second input to the same event, and a write that does
        /// not change the value fires nothing (§4.8.11.11 keys on the value
        /// changing, not on the setter running).
        #[test]
        fn muted_fires_volumechange_only_on_a_real_change() {
            let rt = rt_with_dom();
            rt.eval(
                "var n = 0;
                 var v = document.createElement('video');
                 v.addEventListener('volumechange', function() { n++; });
                 v.muted = true;
                 v.muted = true;
                 v.volume = 1;",
            )
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "n === 1 && v.muted === true"), "expected exactly one volumechange");
        }

        /// GAP-MEDIADECODE срез 18: `playbackRate` fires `ratechange` on a
        /// real change and stays silent on a no-op write, the same rule
        /// `volume`/`muted` already follow — and the native call it triggers
        /// (`__lumen_video_set_playback_rate`) must not throw even with no
        /// decoded resource behind the node.
        #[test]
        fn playback_rate_fires_ratechange_only_on_a_real_change() {
            let rt = rt_with_dom();
            rt.eval(
                "var n = 0;
                 var v = document.createElement('video');
                 v.addEventListener('ratechange', function() { n++; });
                 v.playbackRate = 2;
                 v.playbackRate = 2;
                 v.playbackRate = 1.5;",
            )
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "n === 2 && v.playbackRate === 1.5"), "expected exactly two ratechange events");
        }

        /// The volume range check is a DOMException, not a silent clamp — the
        /// old setter clamped, so a page could never tell it had been wrong.
        #[test]
        fn volume_out_of_range_throws_index_size_error() {
            let rt = rt_with_dom();
            assert!(
                truthy(
                    &rt,
                    "var v = document.createElement('video');
                     var name = null;
                     try { v.volume = 2; } catch (e) { name = e.name; }
                     name === 'IndexSizeError' && v.volume === 1"
                ),
                "an out-of-range volume must throw and leave the value alone"
            );
        }

        /// `playbackRate`/`defaultPlaybackRate` did not exist at all: assignment
        /// made an expando and `ratechange` was never dispatched.
        #[test]
        fn playback_rate_exists_and_queues_ratechange() {
            let rt = rt_with_dom();
            rt.eval(
                "var n = 0;
                 var v = document.createElement('video');
                 v.addEventListener('ratechange', function() { n++; });
                 var before = [v.playbackRate, v.defaultPlaybackRate];
                 v.playbackRate = 2;
                 v.defaultPlaybackRate = 2;
                 v.playbackRate = 2;",
            )
            .unwrap();
            settle(&rt);
            assert!(
                truthy(&rt, "before[0] === 1 && before[1] === 1 && v.playbackRate === 2 && n === 2"),
                "both rates default to 1 and each real change queues one ratechange"
            );
        }

        /// The core of the bug: assigning `src` now runs the resource selection
        /// algorithm. The engine decodes no video format but GIF, so an mp4
        /// ends in the dedicated media source failure steps — `loadstart` then
        /// `error`, with a real `MediaError` — instead of the fabricated
        /// `loadedmetadata` + `canplay` pair the old shim answered with.
        // GAP-MEDIADECODE срез 6: this whole scenario is specific to the
        // no-`ffmpeg-video` default — with the feature on, `movie.mp4` is
        // queued via `startFfmpegLoad` instead of failing outright, and stays
        // pending forever until срез 7 wires a real decoder (documented in
        // `install_video_bindings_v8`'s module doc).
        #[cfg(not(feature = "ffmpeg-video"))]
        #[test]
        fn assigning_src_runs_resource_selection_and_reports_the_failure() {
            let rt = rt_with_dom();
            rt.eval(
                "var log = [];
                 var v = document.createElement('video');
                 v.addEventListener('loadstart', function() { log.push('loadstart:' + v.networkState); });
                 v.addEventListener('error', function() { log.push('error:' + v.error.code + ':' + v.networkState); });
                 v.src = 'http://127.0.0.1:1/movie.mp4';",
            )
            .unwrap();
            assert!(truthy(&rt, "log.length === 0"), "selection must not run inside the setter");
            settle(&rt);
            assert!(
                truthy(&rt, "log.length === 2 && log[0] === 'loadstart:2' && log[1] === 'error:4:3'"),
                "expected loadstart (NETWORK_LOADING) then error (MEDIA_ERR_SRC_NOT_SUPPORTED, NETWORK_NO_SOURCE)"
            );
            assert!(
                truthy(
                    &rt,
                    "v.currentSrc === 'http://127.0.0.1:1/movie.mp4'
                       && v.readyState === 0
                       && v.error instanceof MediaError
                       && v.error.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED"
                ),
                "currentSrc names the selected resource and error is a MediaError"
            );
        }

        /// `load()` was a no-op that fired nothing. It must re-enter the whole
        /// algorithm, which for an element that already had a resource means
        /// `abort` + `emptied` before the new attempt.
        // Same `ffmpeg-video`-changes-the-contract reason as the test above.
        #[cfg(not(feature = "ffmpeg-video"))]
        #[test]
        fn load_reruns_the_algorithm_with_abort_and_emptied() {
            let rt = rt_with_dom();
            rt.eval(
                "var v = document.createElement('video');
                 v.src = 'http://127.0.0.1:1/movie.mp4';",
            )
            .unwrap();
            settle(&rt);
            rt.eval(
                "var log = [];
                 ['abort', 'emptied', 'loadstart', 'error'].forEach(function(t) {
                     v.addEventListener(t, function() { log.push(t); });
                 });
                 v.load();",
            )
            .unwrap();
            settle(&rt);
            assert!(
                truthy(&rt, "log.join(',') === 'emptied,loadstart,error'"),
                "load() should empty the element and try again"
            );
        }

        /// The children branch: a failing candidate fires `error` at the
        /// `<source>` element, never at the media element, and the next
        /// candidate is tried. A `type` the engine cannot play skips the
        /// candidate without even a fetch.
        // Same `ffmpeg-video`-changes-the-contract reason: both candidates here
        // are `.webm`/`.mp4`, which the feature makes pending rather than failed.
        #[cfg(not(feature = "ffmpeg-video"))]
        #[test]
        fn source_children_report_failure_on_the_source_element() {
            let rt = rt_with_dom();
            rt.eval(
                "var log = [];
                 var v = document.createElement('video');
                 v.addEventListener('error', function() { log.push('media-error'); });
                 var a = document.createElement('source');
                 a.setAttribute('src', 'http://127.0.0.1:1/a.webm');
                 a.setAttribute('type', 'video/webm');
                 a.addEventListener('error', function() { log.push('a'); });
                 var b = document.createElement('source');
                 b.setAttribute('src', 'http://127.0.0.1:1/b.mp4');
                 b.addEventListener('error', function() { log.push('b'); });
                 v.appendChild(a);
                 v.appendChild(b);",
            )
            .unwrap();
            settle(&rt);
            assert!(
                truthy(&rt, "log.join(',') === 'a,b'"),
                "each candidate fails on its own <source>, and the media element stays error-free"
            );
            assert!(
                truthy(&rt, "v.error === null && v.networkState === 3"),
                "children mode ends at NETWORK_NO_SOURCE with no MediaError"
            );
        }

        /// `play()` on an element whose resource selection failed rejects with
        /// NotSupportedError instead of resolving as if playback had started.
        // Same `ffmpeg-video`-changes-the-contract reason as above.
        #[cfg(not(feature = "ffmpeg-video"))]
        #[test]
        fn play_rejects_once_the_resource_is_known_unsupported() {
            let rt = rt_with_dom();
            rt.eval(
                "var v = document.createElement('video');
                 v.src = 'http://127.0.0.1:1/movie.mp4';",
            )
            .unwrap();
            settle(&rt);
            rt.eval("var name = null; v.play().catch(function(e) { name = e.name; });")
                .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "name === 'NotSupportedError'"), "play() should reject");
        }

        /// `HTMLMediaElement` did not exist, so neither did the network/readiness
        /// constants every media test reads them off.
        #[test]
        fn media_element_interface_and_constants_exist() {
            let rt = rt_with_dom();
            assert!(
                truthy(
                    &rt,
                    "var v = document.createElement('video');
                     v instanceof HTMLMediaElement
                       && v instanceof HTMLVideoElement
                       && document.createElement('audio') instanceof HTMLMediaElement
                       && HTMLMediaElement.NETWORK_NO_SOURCE === 3
                       && v.HAVE_ENOUGH_DATA === 4
                       && v.NETWORK_EMPTY === 0"
                ),
                "HTMLMediaElement must sit between HTMLElement and the two media interfaces"
            );
        }

        /// The `controls`/`loop` accessors the shim used to install kept their
        /// value in a closure, so the content attribute layout and paint read
        /// never moved. They are gone; dom.rs's reflection owns them.
        #[test]
        fn controls_and_loop_write_through_to_the_content_attribute() {
            let rt = rt_with_dom();
            assert!(
                truthy(
                    &rt,
                    "var v = document.createElement('video');
                     v.controls = true; v.loop = true;
                     v.hasAttribute('controls') && v.hasAttribute('loop')
                       && v.controls === true && v.loop === true"
                ),
                "controls/loop must reflect the content attribute"
            );
        }
    }

    // ── BUG-570: VTTCue/TextTrackCue/TrackEvent global constructors ───────────
    //
    // Needs the real DOM (`EventTarget`/`document.createDocumentFragment` live
    // in the full `web_api_shim`, not the bare `with_video()` stub).
    mod vtt_cue {
        use std::sync::{Arc, Mutex};

        use lumen_core::ext::JsRuntime as _;
        use lumen_dom::{Document, QualName};

        use crate::v8_runtime::V8JsRuntime;

        fn rt_with_dom() -> V8JsRuntime {
            let mut doc = Document::new();
            let html = doc.create_element(QualName::html("html"));
            let body = doc.create_element(QualName::html("body"));
            doc.append_child(doc.root(), html);
            doc.append_child(html, body);
            let rt = V8JsRuntime::new().unwrap();
            rt.install_dom(
                Arc::new(Mutex::new(doc)),
                "",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            )
            .unwrap();
            rt
        }

        fn truthy(rt: &V8JsRuntime, expr: &str) -> bool {
            matches!(rt.eval(expr).unwrap(), lumen_core::JsValue::Bool(true))
        }

        #[test]
        fn vtt_cue_constructor_sets_spec_defaults() {
            let rt = rt_with_dom();
            assert!(
                truthy(
                    &rt,
                    "var cue = new VTTCue(3, 12, 'foo bar');
                     cue.startTime === 3 && cue.endTime === 12 && cue.text === 'foo bar'
                       && cue.id === '' && cue.region === null && cue.pauseOnExit === false
                       && cue.snapToLines === true && cue.line === 'auto'
                       && cue.lineAlign === 'start' && cue.position === 'auto'
                       && cue.positionAlign === 'auto' && cue.size === 100
                       && cue.align === 'center'"
                ),
                "new VTTCue(...) must populate the WebVTT §3.1 defaults"
            );
        }

        /// The generated WPT title names this exact scenario: a value exactly
        /// representable as a double but not as a float must round-trip intact.
        #[test]
        fn vtt_cue_line_position_size_stay_double_precision() {
            let rt = rt_with_dom();
            assert!(
                truthy(
                    &rt,
                    "var cue = new VTTCue(0, 1, 'text');
                     var v = 1.000000000000004;
                     cue.line = v; cue.position = v; cue.size = v;
                     cue.line === v && cue.position === v && cue.size === v"
                ),
                "line/position/size must not be truncated to float precision"
            );
        }

        #[test]
        fn get_cue_as_html_returns_document_fragment_with_text_node() {
            let rt = rt_with_dom();
            // `frag instanceof DocumentFragment` is not asserted here: Lumen's
            // `createDocumentFragment()` returns a plain, prototype-less object
            // literal (see `_lumen_make_document_fragment`'s own comment), so
            // `instanceof` against any constructor is false for every fragment
            // in this engine — a pre-existing, unrelated limitation, not
            // something this fix should paper over.
            assert!(
                truthy(
                    &rt,
                    "var cue = new VTTCue(0, 0, '');
                     var frag = cue.getCueAsHTML();
                     frag.nodeType === 11
                       && frag.childNodes.length === 1
                       && frag.childNodes[0].data === ''"
                ),
                "getCueAsHTML() should wrap the cue text in a document fragment"
            );
        }

        #[test]
        fn text_track_cue_is_not_constructible_but_is_the_vtt_cue_base() {
            let rt = rt_with_dom();
            assert!(
                truthy(&rt, "TextTrackCue !== VTTCue"),
                "TextTrackCue and VTTCue must be separate interfaces"
            );
            assert!(
                truthy(
                    &rt,
                    "var threw = false;
                     try { new TextTrackCue(0, 0, ''); } catch (e) { threw = e instanceof TypeError; }
                     threw"
                ),
                "TextTrackCue has no constructor operation and must throw TypeError"
            );
            assert!(
                truthy(&rt, "new VTTCue(0, 1, 'x') instanceof TextTrackCue"),
                "VTTCue must inherit from TextTrackCue"
            );
        }

        #[test]
        fn track_event_constructor_exposes_readonly_track() {
            let rt = rt_with_dom();
            assert!(
                truthy(
                    &rt,
                    "var ev = new TrackEvent('foo');
                     ev instanceof TrackEvent && ev instanceof Event && ev.track === null"
                ),
                "TrackEvent('foo') must default track to null"
            );
            assert!(
                truthy(
                    &rt,
                    "var ev = new TrackEvent('foo', { track: 42 });
                     ev.track = {};
                     ev.track === 42"
                ),
                "TrackEvent.track is readonly — a later assignment must be ignored"
            );
        }
    }

    // ── BUG-775: <track> load/error ───────────────────────────────────────────
    //
    // These need the real DOM (the stub above has no arena nids, no parents and
    // no `_lumen_dispatch`), so they go through `install_dom` like the dom.rs
    // suites do. No fetch provider is installed, which is deliberate: a
    // `data:`/`blob:` track exercises the whole model without the network, and a
    // relative `src` exercises the failure path for free.
    mod track_loading {
        use std::sync::{Arc, Mutex};

        use lumen_core::ext::JsRuntime as _;
        use lumen_dom::{Document, QualName};

        use crate::v8_runtime::V8JsRuntime;

        fn empty_doc() -> Arc<Mutex<Document>> {
            let mut doc = Document::new();
            let html = doc.create_element(QualName::html("html"));
            let body = doc.create_element(QualName::html("body"));
            doc.append_child(doc.root(), html);
            doc.append_child(html, body);
            Arc::new(Mutex::new(doc))
        }

        fn rt_with_dom() -> V8JsRuntime {
            let rt = V8JsRuntime::new().unwrap();
            rt.install_dom(empty_doc(), "", None, None, None, None, None, None, None, None, None, false)
                .unwrap();
            rt
        }

        /// Turn the event loop far enough for the model's task hop and the
        /// promise chain that follows it to complete.
        fn settle(rt: &V8JsRuntime) {
            for _ in 0..8 {
                rt.eval("_lumen_tick_timers()").unwrap();
            }
        }

        fn truthy(rt: &V8JsRuntime, expr: &str) -> bool {
            matches!(rt.eval(expr).unwrap(), lumen_core::JsValue::Bool(true))
        }

        const VTT: &str = "WEBVTT%0A%0A00:00:00.000 --> 00:00:01.000%0Atext";

        /// The whole shape `webvtt/parsing/file-parsing/tests/*` is generated in:
        /// arm `onload`/`onerror`, append the track to a `<video>` that is itself
        /// never appended anywhere, then read `video.textTracks[0].cues` from the
        /// handler. Before BUG-775 neither handler was ever called.
        #[test]
        fn track_load_event_fires_and_populates_text_tracks() {
            let rt = rt_with_dom();
            rt.eval(&format!(
                "var log = [];
                 var video = document.createElement('video');
                 var track = document.createElement('track');
                 track.src = 'data:text/vtt,{VTT}';
                 track['default'] = true;
                 track.kind = 'subtitles';
                 track.onload = function(e) {{ log.push(['load', e.target === track]); }};
                 track.onerror = function() {{ log.push(['error']); }};
                 video.appendChild(track);"
            ))
            .unwrap();
            // The load must not have happened inside appendChild: the spec queues
            // a task, and the near-universal `onload = …`-after-append ordering
            // depends on it.
            assert!(truthy(&rt, "log.length === 0"), "load fired synchronously");
            settle(&rt);
            assert!(
                truthy(&rt, "log.length === 1 && log[0][0] === 'load' && log[0][1] === true"),
                "expected exactly one `load` with event.target === the track element"
            );
            assert!(
                truthy(
                    &rt,
                    "var cs = video.textTracks[0].cues;
                     video.textTracks.length === 1 && cs.length === 1
                       && cs[0].text === 'text' && cs[0].startTime === 0 && cs[0].endTime === 1"
                ),
                "cues should be readable off the media element"
            );
        }

        /// `track.track` is the same TextTrack the media element lists, and it
        /// exists before the file arrives — `cue-text-parsing/common.js` reads
        /// cues through it rather than through `video.textTracks`.
        #[test]
        fn track_element_track_is_the_same_object_as_the_list_entry() {
            let rt = rt_with_dom();
            rt.eval(&format!(
                "var video = document.createElement('video');
                 var track = document.createElement('track');
                 track.src = 'data:text/vtt,{VTT}';
                 track['default'] = true;
                 var early = track.track;
                 video.appendChild(track);"
            ))
            .unwrap();
            assert!(
                truthy(&rt, "early === track.track && early === video.textTracks[0]"),
                "the TextTrack read before the load must survive it"
            );
            settle(&rt);
            assert!(
                truthy(&rt, "early === track.track && early.cues.length === 1"),
                "cues must land in the already-handed-out TextTrack"
            );
        }

        /// A file that is not a valid WebVTT file fires `error`, not `load` with
        /// zero cues — the split `signature-invalid.html` asserts.
        #[test]
        fn invalid_webvtt_signature_fires_error() {
            let rt = rt_with_dom();
            rt.eval(
                "var got = [];
                 var video = document.createElement('video');
                 var track = document.createElement('track');
                 track.src = 'data:text/vtt,WEBSRT%0A%0A00:00:00.000 --> 00:00:01.000%0Ax';
                 track['default'] = true;
                 track.onload = function() { got.push('load'); };
                 track.onerror = function() { got.push('error'); };
                 video.appendChild(track);",
            )
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "got.length === 1 && got[0] === 'error'"), "expected a single `error`");
            assert!(truthy(&rt, "track.readyState === HTMLTrackElement.ERROR"), "readyState should be ERROR");
        }

        /// A `<track>` that is not parented to a media element must not start the
        /// model at all (HTML LS §4.8.11.1 step 3) — and must still start it once
        /// it is, so the element stays tracked rather than being dropped.
        #[test]
        fn load_starts_only_once_the_parent_is_a_media_element() {
            let rt = rt_with_dom();
            rt.eval(&format!(
                "var got = [];
                 var box_ = document.createElement('div');
                 var track = document.createElement('track');
                 track.src = 'data:text/vtt,{VTT}';
                 track['default'] = true;
                 track.onload = function() {{ got.push('load'); }};
                 box_.appendChild(track);
                 document.body.appendChild(box_);"
            ))
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "got.length === 0"), "a <div> parent must not start the load");
            assert!(truthy(&rt, "track.readyState === HTMLTrackElement.NONE"), "readyState should still be NONE");

            rt.eval("var video = document.createElement('video'); video.appendChild(track);")
                .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "got.length === 1"), "re-parenting under <video> must start it");
        }

        /// The model runs at most once per element: moving a loaded track around
        /// the tree may not refetch it or fire a second `load`.
        #[test]
        fn a_loaded_track_is_not_reloaded_when_moved() {
            let rt = rt_with_dom();
            rt.eval(&format!(
                "var loads = 0;
                 var a = document.createElement('video');
                 var b = document.createElement('video');
                 var track = document.createElement('track');
                 track.src = 'data:text/vtt,{VTT}';
                 track['default'] = true;
                 track.onload = function() {{ loads++; }};
                 a.appendChild(track);"
            ))
            .unwrap();
            settle(&rt);
            rt.eval("b.appendChild(track);").unwrap();
            settle(&rt);
            assert!(truthy(&rt, "loads === 1"), "expected exactly one load event, got a re-run");
        }

        /// `kind` is an enumerated attribute: missing → subtitles, invalid →
        /// metadata (and a metadata track defaults to `hidden`, not `showing`).
        /// Without `default` the mode stays `disabled`, where `cues` is null.
        #[test]
        fn kind_and_mode_follow_the_enumerated_attribute_rules() {
            let rt = rt_with_dom();
            rt.eval(&format!(
                "function mk(attrs) {{
                     var v = document.createElement('video');
                     var t = document.createElement('track');
                     t.src = 'data:text/vtt,{VTT}';
                     for (var k in attrs) t.setAttribute(k, attrs[k]);
                     v.appendChild(t);
                     return t;
                 }}
                 var plain    = mk({{}});
                 var bogus    = mk({{ kind: 'nonsense', 'default': '' }});
                 var captions = mk({{ kind: 'CAPTIONS', 'default': '' }});"
            ))
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "plain.track.kind === 'subtitles'"), "missing kind → subtitles");
            assert!(truthy(&rt, "plain.track.mode === 'disabled' && plain.track.cues === null"),
                "no `default` → disabled, and a disabled track reports null cues");
            assert!(truthy(&rt, "bogus.track.kind === 'metadata' && bogus.track.mode === 'hidden'"),
                "invalid kind → metadata, whose default mode is hidden");
            assert!(truthy(&rt, "captions.track.kind === 'captions' && captions.track.mode === 'showing'"),
                "kind is ASCII case-insensitive and a default subtitle/caption track shows");
            // The cues are parsed either way, so setting the mode is enough to
            // read them — the engine never re-runs the model on a mode change.
            assert!(truthy(&rt, "plain.track.mode = 'showing'; plain.track.cues.length === 1"),
                "a disabled track still parsed its cues");
        }

        /// A `<track>` with no `src` fails rather than staying silent
        /// (§4.8.11.1 step 8 treats the empty URL as a failed fetch).
        #[test]
        fn empty_src_fires_error() {
            let rt = rt_with_dom();
            rt.eval(
                "var got = [];
                 var video = document.createElement('video');
                 var track = document.createElement('track');
                 track.onerror = function() { got.push('error'); };
                 video.appendChild(track);",
            )
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "got.length === 1"), "empty src should fire error");
        }

        /// `URL.createObjectURL(blob)` is how every `cue-text-parsing` test
        /// sources its track; `fetch()` has no `blob:` branch, so the loader
        /// reads the object-URL store directly.
        #[test]
        fn blob_object_url_source_loads() {
            let rt = rt_with_dom();
            rt.eval(
                "var got = null;
                 var video = document.createElement('video');
                 var track = document.createElement('track');
                 var url = URL.createObjectURL(
                     new Blob(['WEBVTT\\n\\n00:00.000 --> 00:01.000\\nhi'], { type: 'text/vtt' }));
                 track.src = url;
                 track['default'] = true;
                 track.onload = function() { got = track.track.cues[0].text; };
                 video.appendChild(track);",
            )
            .unwrap();
            settle(&rt);
            assert_eq!(
                rt.eval("got").unwrap(),
                lumen_core::JsValue::String("hi".to_string()),
                "a blob: track should load and parse"
            );
        }

        /// The parser itself stays on the Rust side — one implementation, shared
        /// with the shell's overlay walk.
        #[test]
        fn vtt_parse_native_reports_the_header_split() {
            let rt = rt_with_dom();
            assert!(
                truthy(
                    &rt,
                    "JSON.parse(__lumen_vtt_parse('WEBVTT\\n\\n00:00.000 --> 00:01.000\\nx')).ok === true"
                ),
                "a valid file parses"
            );
            assert!(
                truthy(&rt, "JSON.parse(__lumen_vtt_parse('WEBSRT\\n')).ok === false"),
                "a bad signature is a parse failure, not an empty cue list"
            );
        }

        // ── BUG-804: the same model for a <track> the PARSER wrote ────────────
        //
        // The distinction the tests below turn on is invisible in the JS text:
        // a node built here through `Document::create_element` never passes
        // through the shim's `createElement`, so it is not in
        // `_lumen_resource_pending` — which is exactly the state a parser-written
        // element is in, and the state BUG-775's machinery could not reach.

        /// `html > body > <media> > <track src kind default>`, built the way the
        /// HTML parser builds it. `src` empty means «no src attribute at all».
        fn doc_with_markup_track(media: &str, src: &str) -> Arc<Mutex<Document>> {
            use lumen_dom::{Attribute, NodeData};

            let mut doc = Document::new();
            let html = doc.create_element(QualName::html("html"));
            let body = doc.create_element(QualName::html("body"));
            let media_el = doc.create_element(QualName::html(media));
            let track = doc.create_element(QualName::html("track"));
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(track).data {
                if !src.is_empty() {
                    attrs.push(Attribute { name: QualName::html("src"), value: src.to_string() });
                }
                attrs.push(Attribute {
                    name: QualName::html("kind"),
                    value: "subtitles".to_string(),
                });
                attrs.push(Attribute { name: QualName::html("default"), value: String::new() });
            }
            doc.append_child(doc.root(), html);
            doc.append_child(html, body);
            doc.append_child(body, media_el);
            doc.append_child(media_el, track);
            Arc::new(Mutex::new(doc))
        }

        fn rt_with_markup_track(media: &str, src: &str) -> V8JsRuntime {
            let rt = V8JsRuntime::new().unwrap();
            rt.install_dom(
                doc_with_markup_track(media, src),
                "",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
            )
            .unwrap();
            rt
        }

        /// The whole shape of the nine `track-webvtt-*.html` files that used to
        /// hang: the `<track>` is in the markup, the handler is armed by a script
        /// below it, and the handler reads `track.track.cues`. Driven through
        /// `_lumen_apply_ready_state` rather than the scan directly, so the test
        /// also covers the wiring — the scan being unreachable would look exactly
        /// like the bug.
        #[test]
        fn parser_written_track_reports_load_and_owns_the_media_list() {
            let rt = rt_with_markup_track("video", &format!("data:text/vtt,{VTT}"));
            rt.eval(
                "var log = [];
                 var track = document.getElementsByTagName('track')[0];
                 var video = document.getElementsByTagName('video')[0];
                 track.addEventListener('load', function(e) { log.push(e.target === track && e.isTrusted === true); });
                 track.onerror = function() { log.push('error'); };",
            )
            .unwrap();
            assert!(truthy(&rt, "log.length === 0"), "nothing may fire before the document is parsed");
            // The list exists before any load does: §4.8.11.1 lists a track when
            // the ELEMENT is inserted, and the three `track-webvtt-*` tests that
            // arm their handlers by looping over `video.textTracks.length` fail
            // outright when it reads 0 here.
            assert!(
                truthy(&rt, "video.textTracks.length === 1 && video.textTracks[0] === track.track"),
                "a markup track must be listed before its file is asked for"
            );
            rt.eval("_lumen_apply_ready_state('interactive')").unwrap();
            settle(&rt);
            assert!(
                truthy(&rt, "log.length === 1 && log[0] === true"),
                "expected exactly one trusted `load` carrying the element as its target"
            );
            assert!(
                truthy(&rt, "track.readyState === HTMLTrackElement.LOADED"),
                "readyState should be LOADED"
            );
            // The half the shell's snapshot cannot supply, and the reason the JS
            // list owns `textTracks` for markup tracks too.
            assert!(
                truthy(
                    &rt,
                    "video.textTracks.length === 1 && track.track === video.textTracks[0]
                       && track.track.cues.length === 1 && track.track.cues[0].text === 'text'"
                ),
                "track.track must be the media element's own list entry, with cues"
            );
        }

        /// §4.8.11.1 step 3 says «media element», and `<audio>` is one — the
        /// shell's walk (`tracks::collect_video_tracks`) only ever looked at
        /// `<video>`, so this track was not merely silent, it was never fetched.
        #[test]
        fn parser_written_track_under_audio_loads_too() {
            let rt = rt_with_markup_track("audio", &format!("data:text/vtt,{VTT}"));
            rt.eval(
                "var loads = 0;
                 var track = document.getElementsByTagName('track')[0];
                 var audio = document.getElementsByTagName('audio')[0];
                 track.onload = function() { loads++; };
                 _lumen_apply_ready_state('interactive');",
            )
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "loads === 1"), "an <audio> child track must load");
            assert!(
                truthy(&rt, "audio.textTracks.length === 1 && audio.textTracks[0].cues.length === 1"),
                "and must reach the media element's list"
            );
        }

        /// A markup `<track>` with no `src` is dropped by the shell's collector
        /// before it is ever listed; §4.8.11.1 step 8 wants an `error` instead.
        #[test]
        fn parser_written_track_without_src_reports_error() {
            let rt = rt_with_markup_track("video", "");
            rt.eval(
                "var got = [];
                 var track = document.getElementsByTagName('track')[0];
                 track.onload = function() { got.push('load'); };
                 track.onerror = function() { got.push('error'); };
                 _lumen_apply_ready_state('interactive');",
            )
            .unwrap();
            settle(&rt);
            assert!(
                truthy(&rt, "got.length === 1 && got[0] === 'error'"),
                "a src-less markup track owes exactly one `error`"
            );
            assert!(
                truthy(&rt, "track.readyState === HTMLTrackElement.ERROR"),
                "readyState should be ERROR"
            );
        }

        /// The two entry points must not both claim the same element: a track a
        /// head script appended to a `<video>` that is in the markup has already
        /// run through the insertion hook by the time the scan walks the tree.
        #[test]
        fn the_parser_scan_does_not_reload_a_script_inserted_track() {
            let rt = rt_with_markup_track("video", &format!("data:text/vtt,{VTT}"));
            rt.eval(&format!(
                "var loads = 0;
                 var video = document.getElementsByTagName('video')[0];
                 var added = document.createElement('track');
                 added.src = 'data:text/vtt,{VTT}';
                 added['default'] = true;
                 added.onload = function() {{ loads++; }};
                 video.appendChild(added);"
            ))
            .unwrap();
            settle(&rt);
            assert!(truthy(&rt, "loads === 1"), "the insertion hook should have loaded it once");
            rt.eval("_lumen_apply_ready_state('interactive')").unwrap();
            settle(&rt);
            assert!(truthy(&rt, "loads === 1"), "the parser scan must not load it a second time");
            // Both tracks are in the list, in tree order, and each element's own
            // `.track` is its entry.
            assert!(
                truthy(
                    &rt,
                    "var markup = document.getElementsByTagName('track')[0];
                     video.textTracks.length === 2 && video.textTracks[0] === markup.track
                       && video.textTracks[1] === added.track"
                ),
                "the list must hold both tracks, in tree order"
            );
        }
    }
}
