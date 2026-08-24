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
//! | `__lumen_video_play` | `(nid: f64, now_ms: f64)` | Start/resume |
//! | `__lumen_video_pause` | `(nid: f64, now_ms: f64)` | Pause |
//! | `__lumen_video_seek` | `(nid: f64, secs: f64, now_ms: f64)` | Seek |
//! | `__lumen_video_current_time` | `(nid: f64, now_ms: f64) → f64` | Position (s) |
//! | `__lumen_video_duration` | `(nid: f64) → f64` | Duration (s), Inf for loops |
//! | `__lumen_video_paused` | `(nid: f64) → bool` | Is paused? |
//! | `__lumen_video_ended` | `(nid: f64, now_ms: f64) → bool` | Has ended? |
//! | `__lumen_video_width` | `(nid: f64) → f64` | GIF pixel width |
//! | `__lumen_video_height` | `(nid: f64) → f64` | GIF pixel height |
//! | `__lumen_video_can_play_type` | `(mime: String) → String` | canPlayType probe |
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
//! Only an animated GIF is decodable, so resource selection ends in the spec's
//! «dedicated media source failure steps» for every other format — `loadstart`
//! then `error` with `MEDIA_ERR_SRC_NOT_SUPPORTED` — which is what
//! `canPlayType` has always said about them.  Every media event is *queued*,
//! never dispatched inline: the near-universal `e.volume = 0.5;
//! e.onvolumechange = …` order sees nothing at all from a synchronous
//! dispatch.  `<audio>` keeps its own, older model in `audio_element.rs` and
//! still dispatches synchronously.

#[cfg(feature = "v8-backend")]
use crate::text_track_store::get_text_track_store;
#[cfg(feature = "v8-backend")]
use crate::video_gif_store::get_video_gif_store;

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
            if let Some(s) = &store
                && let Some(e) = s.playback.lock().unwrap().get_mut(&(nid as u32))
            {
                e.freeze(now_ms as u64);
                e.paused = true;
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
                    s.playback
                        .lock()
                        .unwrap()
                        .get(&(nid as u32))
                        .map(|e| e.current_ms(now_ms as u64) as f64 / 1000.0)
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
                    s.playback
                        .lock()
                        .unwrap()
                        .get(&(nid as u32))
                        .map(|e| e.is_ended(now_ms as u64))
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

    {
        let can_play_type = into_v8_fn1(move |mime: String| -> String {
            let m = mime.trim().to_ascii_lowercase();
            let base = m.split(';').next().unwrap_or("").trim();
            if base == "image/gif" {
                "maybe".to_string()
            } else {
                String::new()
            }
        });
        rt.register_native("__lumen_video_can_play_type", can_play_type)?;
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

/// HTMLVideoElement Phase 1 shim.
///
/// Uses `__lumen_video_*` native bindings for GIF-backed playback.  Falls
/// back to Phase 0 behaviour when the store is absent (headless/CI) or when
/// the `src` is not a `.gif` URL.
#[cfg(feature = "v8-backend")]
const VIDEO_SHIM: &str = r#"(function() {
  'use strict';

  var HAS_STORE = (typeof __lumen_video_load === 'function');
  var POLL_MS   = 50;    // readyState poll when waiting for GIF decode
  var TUPDATE_MS = 250;  // timeupdate interval during playback

  // BUG-775 — per-`<track>` state and the per-media-element TextTrack list the
  // shim owns. Declared up here because `buildTextTracks` reads the second one
  // and is defined before the loader that writes it.
  var _lumen_track_states     = Object.create(null); // <track> nid → state
  var _lumen_track_media_lists = Object.create(null); // <video>/<audio> nid → [TextTrack]

  function isGifSrc(src) {
    if (!src) return false;
    var base = src.split('?')[0].split('#')[0].toLowerCase();
    return base.endsWith('.gif');
  }

  function nowMs() {
    return (typeof performance !== 'undefined' && performance.now)
      ? performance.now()
      : Date.now();
  }

  function fireEvent(el, name) {
    try {
      var ev = new Event(name, { bubbles: false, cancelable: false });
      // `_lumen_dispatch` leaves `target` null on the at-target path (BUG-873),
      // and `event.target` is how a media/`<source>` handler reaches back to the
      // element it was armed on.
      try { ev.target = el; } catch (e) {}
      el.dispatchEvent(ev);
    } catch(e) {
      if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e);
    }
  }

  // Media element event task source (HTML §4.8.11.16). Every media event is
  // queued rather than dispatched inline: `e.volume = 0.5; e.onvolumechange = …`
  // — the order `event_volumechange.html` and most of WPT's media suite use —
  // sees nothing at all from a synchronous dispatch, and BUG-808 measured the
  // same trap for `EventWatcher`, where an immediate event is worse than none.
  function queueTask(fn) {
    if (typeof setTimeout === 'function') { setTimeout(fn, 0); return; }
    fn();  // unit-test runtimes with no timer stub run the task inline
  }

  function domException(message, name) {
    if (typeof DOMException === 'function') {
      try { return new DOMException(message, name); } catch (e) {}
    }
    var err = new Error(message);
    err.name = name;
    return err;
  }

  // ── MediaError (HTML §4.8.11.2) ─────────────────────────────────────────────
  // No constructor per IDL, so the global throws and instances are minted by
  // `makeMediaError`. `<video>.error` used to not exist at all (BUG-825).

  if (typeof globalThis.MediaError !== 'function') {
    var _MediaError = function () { throw new TypeError('Illegal constructor'); };
    Object.defineProperty(_MediaError, 'name', { value: 'MediaError', configurable: true });
    var _MEDIA_ERR_CODES = {
      MEDIA_ERR_ABORTED: 1, MEDIA_ERR_NETWORK: 2,
      MEDIA_ERR_DECODE: 3, MEDIA_ERR_SRC_NOT_SUPPORTED: 4,
    };
    for (var _mk in _MEDIA_ERR_CODES) {
      Object.defineProperty(_MediaError, _mk, { value: _MEDIA_ERR_CODES[_mk], enumerable: true });
      Object.defineProperty(_MediaError.prototype, _mk, { value: _MEDIA_ERR_CODES[_mk], enumerable: true });
    }
    globalThis.MediaError = _MediaError;
  }

  var MEDIA_ERR_SRC_NOT_SUPPORTED = 4;

  function makeMediaError(code, message) {
    var e = Object.create(globalThis.MediaError.prototype);
    Object.defineProperty(e, 'code', { value: code, enumerable: true });
    Object.defineProperty(e, 'message', { value: message || '', enumerable: true });
    return e;
  }

  // ── HTMLMediaElement (HTML §4.8.11) ─────────────────────────────────────────
  //
  // dom.rs builds `HTMLVideoElement`/`HTMLAudioElement` straight off
  // `HTMLElement` ("Lumen has no HTMLMediaElement interface yet"), so there was
  // no interface to hang the network/readiness constants on and
  // `video instanceof HTMLMediaElement` threw. Splicing it in here keeps the
  // whole media model in one file, and re-linking only changes the
  // [[Prototype]] — every reflection row dom.rs already installed on the two
  // prototypes stays an own property of them.

  var NETWORK_EMPTY = 0, NETWORK_IDLE = 1, NETWORK_LOADING = 2, NETWORK_NO_SOURCE = 3;
  var HAVE_NOTHING = 0, HAVE_METADATA = 1, HAVE_CURRENT_DATA = 2,
      HAVE_FUTURE_DATA = 3, HAVE_ENOUGH_DATA = 4;

  if (typeof globalThis.HTMLMediaElement !== 'function' && typeof HTMLElement === 'function') {
    var _HTMLMediaElement = function () { throw new TypeError('Illegal constructor'); };
    Object.defineProperty(_HTMLMediaElement, 'name', { value: 'HTMLMediaElement', configurable: true });
    _HTMLMediaElement.prototype = Object.create(HTMLElement.prototype);
    Object.defineProperty(_HTMLMediaElement.prototype, 'constructor',
      { value: _HTMLMediaElement, writable: true, configurable: true });
    var _MEDIA_CONSTS = {
      NETWORK_EMPTY: NETWORK_EMPTY, NETWORK_IDLE: NETWORK_IDLE,
      NETWORK_LOADING: NETWORK_LOADING, NETWORK_NO_SOURCE: NETWORK_NO_SOURCE,
      HAVE_NOTHING: HAVE_NOTHING, HAVE_METADATA: HAVE_METADATA,
      HAVE_CURRENT_DATA: HAVE_CURRENT_DATA, HAVE_FUTURE_DATA: HAVE_FUTURE_DATA,
      HAVE_ENOUGH_DATA: HAVE_ENOUGH_DATA,
    };
    for (var _ck in _MEDIA_CONSTS) {
      Object.defineProperty(_HTMLMediaElement, _ck, { value: _MEDIA_CONSTS[_ck], enumerable: true });
      Object.defineProperty(_HTMLMediaElement.prototype, _ck, { value: _MEDIA_CONSTS[_ck], enumerable: true });
    }
    globalThis.HTMLMediaElement = _HTMLMediaElement;
    if (typeof HTMLVideoElement === 'function')
      Object.setPrototypeOf(HTMLVideoElement.prototype, _HTMLMediaElement.prototype);
    if (typeof HTMLAudioElement === 'function')
      Object.setPrototypeOf(HTMLAudioElement.prototype, _HTMLMediaElement.prototype);
  }

  // media element nid → «a <source> child was inserted» hook, written by
  // `patchVideoElement` and read by the DOM insertion hook below.
  var _lumen_media_hooks = Object.create(null);

  // Called from dom.rs's insertion hook for a script-created <source>: HTML
  // §4.8.11.5 says inserting one into a media element whose networkState is
  // NETWORK_EMPTY invokes the media load algorithm. Answers false while the
  // parent is not a media element, which keeps the element tracked for a later
  // re-parenting — the same contract `_lumen_track_start_load` has.
  globalThis._lumen_media_source_inserted = function (nid) {
    if (typeof _lumen_get_parent !== 'function' || typeof _lumen_u2n !== 'function') return false;
    var mediaNid = _lumen_u2n(_lumen_get_parent(nid));
    if (mediaNid === null) return false;
    var tag = (typeof _lumen_get_tag_name === 'function')
      ? String(_lumen_get_tag_name(mediaNid) || '').toUpperCase() : '';
    if (tag !== 'VIDEO' && tag !== 'AUDIO') return false;
    var hook = _lumen_media_hooks[mediaNid];
    if (hook) hook();
    return true;
  };

  // ── TextTrack API (HTML §4.8.11) ────────────────────────────────────────────
  // Read-only view over the shell's parsed <track> cues. No cue mutation.

  function makeCueList(cues) {
    var list = {
      length: cues.length,
      getCueById: function(id) {
        for (var i = 0; i < cues.length; i++) { if (cues[i].id === id) return cues[i]; }
        return null;
      },
      item: function(i) { return cues[i] || null; },
    };
    for (var i = 0; i < cues.length; i++) list[i] = cues[i];
    return list;
  }

  // Append parsed `{id,start,end,text}` records to a TextTrack's cue array.
  // Mutates `track._cues` in place rather than rebuilding it: the `cues` and
  // `activeCues` getters close over that exact array, and a <track> loaded by
  // the shim (BUG-775) gets its TextTrack at insertion time and its cues only
  // when the file arrives.
  function appendCues(track, rawCues) {
    for (var j = 0; j < rawCues.length; j++) {
      var rc = rawCues[j] || {};
      track._cues.push({
        id: rc.id || '',
        startTime: +rc.start || 0,
        endTime: +rc.end || 0,
        text: rc.text || '',
        track: track,
        pauseOnExit: false,
      });
    }
  }

  // One TextTrack over a plain `{kind,label,language,mode,cues}` record. `el` is
  // the owning media element — `activeCues` needs its playback clock.
  function makeTextTrack(el, td) {
    td = td || {};
    var track = {
      kind: td.kind || '',
      label: td.label || '',
      language: td.language || '',
      mode: td.mode || 'disabled',
      id: '',
      oncuechange: null,
      _cues: [],
      _activeSig: null,
      _listeners: [],
      addEventListener: function(type, cb) {
        if (type === 'cuechange' && typeof cb === 'function') this._listeners.push(cb);
      },
      removeEventListener: function(type, cb) {
        if (type !== 'cuechange') return;
        var k = this._listeners.indexOf(cb);
        if (k >= 0) this._listeners.splice(k, 1);
      },
    };
    Object.defineProperty(track, 'cues', {
      get: function() { return this.mode === 'disabled' ? null : makeCueList(this._cues); },
      configurable: true,
    });
    Object.defineProperty(track, 'activeCues', {
      get: function() {
        if (this.mode === 'disabled') return null;
        var ct = (el && el.currentTime) || 0;
        var act = [];
        for (var k = 0; k < this._cues.length; k++) {
          var c = this._cues[k];
          if (c.startTime <= ct && ct < c.endTime) act.push(c);
        }
        return makeCueList(act);
      },
      configurable: true,
    });
    appendCues(track, td.cues || []);
    return track;
  }

  function makeTrackList(tracks) {
    var listObj = {
      length: tracks.length,
      getTrackById: function(id) {
        for (var i = 0; i < tracks.length; i++) { if (tracks[i].id === id) return tracks[i]; }
        return null;
      },
      _tracks: tracks,
    };
    for (var i = 0; i < tracks.length; i++) listObj[i] = tracks[i];
    return listObj;
  }

  function buildTextTracks(el, nid) {
    // BUG-775: a <track> the page built with createElement is fetched and parsed
    // by this shim, not by the shell (whose walk only ever sees the parsed
    // document), so its TextTrack objects are the whole list for that video —
    // and they must be the *same objects* `trackElement.track` hands out.
    var js = nid ? _lumen_track_media_lists[nid] : null;
    if (js && js.length) {
      var jsList = makeTrackList(js.slice());
      jsList._jsLen = js.length;
      return jsList;
    }
    var raw = [];
    if (typeof __lumen_texttracks_json === 'function' && nid) {
      try { raw = JSON.parse(__lumen_texttracks_json(nid) || '[]'); } catch(e) { raw = []; }
    }
    var tracks = [];
    for (var i = 0; i < raw.length; i++) tracks.push(makeTextTrack(el, raw[i] || {}));
    return makeTrackList(tracks);
  }

  function fireTrackCueChange(track) {
    var ev = { type: 'cuechange', target: track };
    if (typeof track.oncuechange === 'function') { try { track.oncuechange(ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); } }
    for (var i = 0; i < track._listeners.length; i++) {
      try { track._listeners[i].call(track, ev); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
    }
  }

  function checkCueChanges(el) {
    var tl = el.__lumen_text_tracks;
    // Late population: the shell may parse <track> files after the shim ran.
    if (!tl || tl.length === 0) { tl = el.textTracks; }
    if (!tl) return;
    var ct = el.currentTime || 0;
    for (var i = 0; i < tl.length; i++) {
      var tr = tl[i];
      if (tr.mode === 'disabled') continue;
      var sig = '';
      for (var j = 0; j < tr._cues.length; j++) {
        var c = tr._cues[j];
        if (c.startTime <= ct && ct < c.endTime) sig += j + ',';
      }
      if (sig !== tr._activeSig) { tr._activeSig = sig; fireTrackCueChange(tr); }
    }
  }

  // ── <track> loading: HTML LS §4.8.11.1 «start the track processing model» ───
  //
  // BUG-775. The shell's `tracks::load_video_tracks` walks the *parsed* document
  // exactly once per navigation, so a <track> minted with document.createElement
  // — the shape every WebVTT test and every player with custom subtitle UI uses
  // — was never fetched, never parsed and dispatched neither `load` nor `error`.
  // A page that armed `track.onload` before appending simply waited forever.
  //
  // Two deliberate deviations from the spec, both erring towards firing the
  // event rather than staying silent:
  //
  //   * the load is NOT gated on the text track mode being non-disabled. The
  //     engine has no user-preference machinery and nothing re-runs the model on
  //     a later mode change, so gating would mean a page that never sets
  //     `default` hangs on `onload` — precisely the defect this fixes. The mode
  //     itself is still computed per spec, so `cues` stays null for a disabled
  //     track and becomes readable the moment the page sets `mode`.
  //   * cues loaded here do not reach the shell's overlay renderer (that store
  //     is written by the Rust-side walk), so a script-built track is visible to
  //     the page but not painted over the video.
  var TRACK_KINDS = { subtitles: 1, captions: 1, descriptions: 1, chapters: 1, metadata: 1 };

  function trackAttr(nid, name) {
    if (typeof _lumen_get_attr !== 'function') return null;
    var v = _lumen_get_attr(nid, name);
    return (v === undefined || v === null) ? null : String(v);
  }

  function trackState(nid) {
    var st = _lumen_track_states[nid];
    if (!st) st = _lumen_track_states[nid] = { started: false, readyState: 0, track: null, media: null };
    return st;
  }

  // `load`/`error` on a <track> neither bubble nor cancel, so an at-target
  // dispatch is the whole story. `target` is assigned by hand because
  // `_lumen_dispatch` — unlike the bubbling paths — leaves it null, and
  // `event.target` is how the generated WebVTT tests reach back to the element
  // they armed (`var track = event.target; var video = track.parentNode;`).
  function fireTrackEvent(nid, type) {
    try {
      var ev = new Event(type, { bubbles: false, cancelable: false });
      if (typeof _lumen_make_element === 'function') ev.target = _lumen_make_element(nid);
      _lumen_dispatch(nid, ev);
    } catch (e) {
      if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e);
    }
  }

  // HTML §4.8.11: `kind` is an enumerated attribute whose *missing* value
  // default is subtitles and whose *invalid* value default is metadata — not
  // what the generic reflection getter does, which is why it is recomputed here.
  function trackKind(nid) {
    var k = trackAttr(nid, 'kind');
    if (k === null) return 'subtitles';
    k = k.toLowerCase();
    return TRACK_KINDS[k] ? k : 'metadata';
  }

  // «Honor user preferences for automatic text track selection», reduced to the
  // single input this engine has: the `default` content attribute.
  function trackMode(nid, kind) {
    if (trackAttr(nid, 'default') === null) return 'disabled';
    return kind === 'metadata' ? 'hidden' : 'showing';
  }

  function trackMediaElement(nid) {
    var st = trackState(nid);
    if (st.media === null || typeof _lumen_make_element !== 'function') return null;
    return _lumen_make_element(st.media);
  }

  // Get (creating on first use) the TextTrack of one <track> element. A track
  // element always has a text track, from the moment it exists — reading
  // `.track` before the file has arrived must not mint a second one that the
  // load would then replace behind the page's back.
  function ensureTrackObject(nid) {
    var st = trackState(nid);
    if (!st.track) {
      var kind = trackKind(nid);
      st.track = makeTextTrack(trackMediaElement(nid), {
        kind: kind,
        label: trackAttr(nid, 'label') || '',
        language: trackAttr(nid, 'srclang') || '',
        mode: trackMode(nid, kind),
        cues: [],
      });
    }
    return st.track;
  }

  // The three URL shapes a `<track src>` can carry. `blob:` and `data:` are read
  // locally because `fetch()` has no branch for either — a `blob:lumen/` URL
  // would be handed to the network layer and fail — and every test under
  // `webvtt/parsing/cue-text-parsing/` builds its track with createObjectURL.
  // Both are matched on the *raw* attribute, before base resolution: neither is
  // a URL `_url_resolve` has any business rewriting.
  function readTrackBody(url) {
    if (url.indexOf('blob:lumen/') === 0) {
      var blob = (typeof _object_url_store !== 'undefined') ? _object_url_store[url] : null;
      if (!blob || !blob._bytes) return Promise.reject(new Error('object URL is not registered'));
      try { return Promise.resolve(new TextDecoder().decode(new Uint8Array(blob._bytes))); }
      catch (e) { return Promise.reject(e); }
    }
    if (url.indexOf('data:') === 0) {
      var comma = url.indexOf(',');
      if (comma === -1) return Promise.reject(new Error('malformed data: URL'));
      var meta = url.slice(5, comma);
      var content = url.slice(comma + 1);
      try {
        return Promise.resolve(meta.indexOf('base64') !== -1 ? atob(content)
                                                             : decodeURIComponent(content));
      } catch (e) { return Promise.reject(e); }
    }
    var abs = (typeof _url_resolve === 'function' && typeof _lumen_document_base_url === 'function')
      ? _url_resolve(url, _lumen_document_base_url()) : url;
    return fetch(abs).then(function(resp) {
      if (!resp.ok) throw new Error('HTTP ' + resp.status);
      return resp.text();
    });
  }

  function finishTrackLoad(nid, cues) {
    var st = trackState(nid);
    st.readyState = 2; // HTMLTrackElement.LOADED
    appendCues(ensureTrackObject(nid), cues || []);
    fireTrackEvent(nid, 'load');
  }

  function failTrackLoad(nid, why) {
    var st = trackState(nid);
    st.readyState = 3; // HTMLTrackElement.ERROR
    if (typeof _lumen_console_error === 'function') {
      _lumen_console_error('track load failed: ' + why);
    }
    fireTrackEvent(nid, 'error');
  }

  // Called from the DOM insertion hook in `dom.rs` for every <track> the page
  // minted with createElement. Returns true once the model has started, which
  // is what tells the hook to stop tracking the element — the spec's «one
  // instance of the algorithm at a time» rule doubles as the already-started
  // flag, so moving a loaded track around the tree can never refetch it.
  function startTrackLoad(nid) {
    var st = trackState(nid);
    if (st.started) return true;
    // §4.8.11.1 step 3 gates on the parent being a media element and NOT on the
    // element being in a document — half of WPT's WebVTT tests never append the
    // <video> anywhere at all.
    if (typeof _lumen_get_parent !== 'function' || typeof _lumen_u2n !== 'function') return false;
    var mediaNid = _lumen_u2n(_lumen_get_parent(nid));
    if (mediaNid === null) return false;
    var tag = (typeof _lumen_get_tag_name === 'function')
      ? String(_lumen_get_tag_name(mediaNid) || '').toUpperCase() : '';
    if (tag !== 'VIDEO' && tag !== 'AUDIO') return false;

    st.started = true;
    st.media = mediaNid;
    st.readyState = 1; // HTMLTrackElement.LOADING

    // The TextTrack joins the media element's list now, before the bytes are in:
    // the list is in tree order, and building it on completion instead would
    // order it by how fast each file happened to arrive.
    var tt = ensureTrackObject(nid);
    var list = _lumen_track_media_lists[mediaNid];
    if (!list) list = _lumen_track_media_lists[mediaNid] = [];
    if (list.indexOf(tt) < 0) list.push(tt);

    var raw = trackAttr(nid, 'src');
    var src = raw === null ? '' : raw.trim();
    // Task hop, for the same two reasons as the external <script> path in
    // dom.rs: Lumen's fetch is synchronous underneath (an inline one would stall
    // the appendChild), and `track.onload = …` is routinely assigned after the
    // insertion that starts the load.
    setTimeout(function() {
      // §4.8.11.1 step 8: an empty URL fails exactly like a failed fetch.
      if (src === '') { failTrackLoad(nid, 'empty src'); return; }
      readTrackBody(src).then(function(text) {
        var res = null;
        if (typeof __lumen_vtt_parse === 'function') {
          try { res = JSON.parse(__lumen_vtt_parse(text)); } catch (e) { res = null; }
        }
        if (!res || !res.ok) throw new Error('not a valid WebVTT file');
        finishTrackLoad(nid, res.cues || []);
      }).catch(function(e) {
        failTrackLoad(nid, src + ': ' + e);
      });
    }, 0);
    return true;
  }
  globalThis._lumen_track_start_load = startTrackLoad;

  // HTMLTrackElement.track / .readyState and the readiness constants (HTML
  // §4.8.11). On the prototype rather than on each wrapper: since BUG-849 the
  // wrapper shares one prototype per interface, and this state is keyed by nid.
  if (typeof HTMLTrackElement === 'function') {
    Object.defineProperty(HTMLTrackElement.prototype, 'track', {
      get: function() { return ensureTrackObject(this.__nid__); },
      configurable: true,
    });
    Object.defineProperty(HTMLTrackElement.prototype, 'readyState', {
      get: function() { return trackState(this.__nid__).readyState; },
      configurable: true,
    });
    var TRACK_READINESS = { NONE: 0, LOADING: 1, LOADED: 2, ERROR: 3 };
    for (var rk in TRACK_READINESS) {
      Object.defineProperty(HTMLTrackElement, rk, { value: TRACK_READINESS[rk], enumerable: true });
      Object.defineProperty(HTMLTrackElement.prototype, rk, { value: TRACK_READINESS[rk], enumerable: true });
    }
  }

  function patchVideoElement(el) {
    if (el.__lumen_video_patched) return;
    el.__lumen_video_patched = true;

    var nid      = el.__nid__;
    var _volume  = 1.0;
    var _muted   = !!(el.hasAttribute && el.hasAttribute('muted'));
    var _defaultRate = 1.0;
    var _rate        = 1.0;
    var _networkState = NETWORK_EMPTY;
    var _readyState   = HAVE_NOTHING;
    var _currentSrc   = '';
    var _error        = null;
    var _paused       = true;
    // Bumped by every media load algorithm run; a selection or fetch whose
    // generation is stale silently drops itself instead of racing the new one.
    var _generation   = 0;
    var _loadTimer    = null;
    var _tupdateTimer = null;
    var _gifBacked = false; // true once a GIF is successfully loaded

    function attr(name) {
      var v = (el.getAttribute && el.getAttribute(name));
      return (v === undefined || v === null) ? null : String(v);
    }
    function hasAttr(name) { return !!(el.hasAttribute && el.hasAttribute(name)); }
    function queueEvent(name) { queueTask(function () { fireEvent(el, name); }); }
    function stopTimers() {
      if (_loadTimer !== null) { clearInterval(_loadTimer); _loadTimer = null; }
      if (_tupdateTimer !== null) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
    }
    function isPaused() {
      return (_gifBacked && HAS_STORE && nid) ? __lumen_video_paused(nid) : _paused;
    }
    // Resolution failure falls back to the raw string rather than to null: a
    // document with no base URL (a unit-test runtime, an `about:blank` tab)
    // must still reach the honest `loadstart` → `error` pair instead of going
    // silent, which is the very defect BUG-825 is about.
    function resolveUrl(u) {
      if (typeof _url_resolve === 'function' && typeof _lumen_document_base_url === 'function') {
        try {
          var r = _url_resolve(u, _lumen_document_base_url());
          if (r) return String(r);
        } catch (e) {}
      }
      return u;
    }

    // ── resource selection (HTML §4.8.11.5) ──────────────────────────────────
    //
    // BUG-825: none of this existed. `src =` and `load()` produced no event at
    // all, `readyState` answered HAVE_ENOUGH_DATA before anything was assigned,
    // and a non-GIF source got a fabricated `loadedmetadata` + `canplay` pair
    // for a file the engine had never fetched, let alone decoded. The model
    // below is the spec's, minus the decoding: an animated GIF really plays
    // (`video_gif_store`), and every other format ends in the «dedicated media
    // source failure steps» — which is the honest answer, and the one
    // `canPlayType` has always given for it.

    // §4.8.11.5 «media load algorithm».
    function mediaLoadAlgorithm() {
      var gen = ++_generation;
      stopTimers();
      if (_networkState === NETWORK_LOADING || _networkState === NETWORK_IDLE) queueEvent('abort');
      if (_networkState !== NETWORK_EMPTY) {
        queueEvent('emptied');
        if (!isPaused()) queueEvent('pause');
        if (_gifBacked && HAS_STORE && nid) __lumen_video_pause(nid, nowMs());
        _gifBacked   = false;
        _paused      = true;
        _readyState  = HAVE_NOTHING;
        _currentSrc  = '';
        _networkState = NETWORK_EMPTY;
      }
      _rate  = _defaultRate;
      _error = null;
      resourceSelection(gen);
    }

    // §4.8.11.5 «resource selection algorithm». Everything past the spec's
    // «await a stable state» runs as a task, so a page that assigns `src` and
    // arms `onloadstart` on the next line still sees the event.
    function resourceSelection(gen) {
      _networkState = NETWORK_NO_SOURCE;
      queueTask(function () {
        if (gen !== _generation) return;
        if (hasAttr('src')) { startFetch(gen, attr('src') || '', null); return; }
        var candidates = sourceChildren();
        // Step 6 «otherwise»: no src attribute and no <source> child at all.
        if (candidates.length === 0) { _networkState = NETWORK_EMPTY; return; }
        nextCandidate(gen, candidates, 0);
      });
    }

    function sourceChildren() {
      var out = [];
      var kids = (el.children && el.children.length !== undefined) ? el.children : null;
      if (!kids) return out;
      for (var i = 0; i < kids.length; i++) {
        var k = kids[i];
        if (k && String(k.tagName || '').toUpperCase() === 'SOURCE') out.push(k);
      }
      return out;
    }

    // Children branch. A candidate is skipped — with `error` fired at the
    // <source> itself and never at the media element, which is the whole point
    // of the split — when it carries no src, an unplayable `type` or a
    // non-matching `media`.
    function nextCandidate(gen, list, i) {
      if (gen !== _generation) return;
      if (i >= list.length) {
        // «Wait for a source element to be added»: nothing else in this engine
        // ever adds one mid-algorithm, so the element settles with no resource
        // and — per spec — no error of its own.
        _currentSrc = '';
        _networkState = NETWORK_NO_SOURCE;
        return;
      }
      var s = list[i];
      var raw  = (s.getAttribute && s.getAttribute('src'));
      var type = (s.getAttribute && s.getAttribute('type'));
      var mq   = (s.getAttribute && s.getAttribute('media'));
      if (raw === undefined || raw === null || String(raw) === '') { skipCandidate(gen, list, i, s); return; }
      if (type && el.canPlayType(String(type)) === '') { skipCandidate(gen, list, i, s); return; }
      if (mq && typeof matchMedia === 'function') {
        var m = null;
        try { m = matchMedia(String(mq)); } catch (e) { m = null; }
        if (m && m.matches === false) { skipCandidate(gen, list, i, s); return; }
      }
      startFetch(gen, String(raw), { list: list, index: i, el: s });
    }

    function skipCandidate(gen, list, i, sourceEl) {
      queueTask(function () {
        if (gen !== _generation) return;
        fireEvent(sourceEl, 'error');
        nextCandidate(gen, list, i + 1);
      });
    }

    // §4.8.11.5 «resource fetch algorithm», reduced to what this engine decodes.
    function startFetch(gen, url, candidate) {
      var abs = (url === '') ? null : resolveUrl(url);
      if (abs === null) { failResource(gen, candidate, 'unresolvable URL'); return; }
      _currentSrc = abs;
      _networkState = NETWORK_LOADING;
      queueEvent('loadstart');
      if (startGifLoad(gen, url)) return;
      failResource(gen, candidate, 'unsupported media format');
    }

    function failResource(gen, candidate, why) {
      queueTask(function () {
        if (gen !== _generation) return;
        if (candidate) {
          _currentSrc = '';
          fireEvent(candidate.el, 'error');
          nextCandidate(gen, candidate.list, candidate.index + 1);
          return;
        }
        // «Dedicated media source failure steps».
        _error = makeMediaError(MEDIA_ERR_SRC_NOT_SUPPORTED, why);
        _readyState = HAVE_NOTHING;
        _networkState = NETWORK_NO_SOURCE;
        fireEvent(el, 'error');
      });
    }

    // ── GIF load ─────────────────────────────────────────────────────────────

    function startGifLoad(gen, src) {
      if (!HAS_STORE || !nid) return false;
      if (!isGifSrc(src)) return false;
      if (typeof setInterval !== 'function') return false;
      __lumen_video_load(nid, src);
      // Poll until the shell has decoded the GIF.
      _loadTimer = setInterval(function() {
        if (gen !== _generation) { clearInterval(_loadTimer); _loadTimer = null; return; }
        if (!__lumen_video_ready(nid)) return;
        clearInterval(_loadTimer); _loadTimer = null;
        _gifBacked = true;
        _readyState = HAVE_METADATA;
        fireEvent(el, 'durationchange');
        fireEvent(el, 'loadedmetadata');
        _readyState = HAVE_CURRENT_DATA;
        fireEvent(el, 'loadeddata');
        _readyState = HAVE_FUTURE_DATA;
        fireEvent(el, 'canplay');
        _readyState = HAVE_ENOUGH_DATA;
        _networkState = NETWORK_IDLE;
        fireEvent(el, 'canplaythrough');
        if (hasAttr('autoplay')) el.play();
      }, POLL_MS);
      return true;
    }

    // ── timeupdate loop ───────────────────────────────────────────────────────

    function startTupdate() {
      if (_tupdateTimer !== null) return;
      if (typeof setInterval !== 'function') return;
      _tupdateTimer = setInterval(function() {
        if (!_gifBacked || !HAS_STORE || __lumen_video_paused(nid)) {
          clearInterval(_tupdateTimer); _tupdateTimer = null; return;
        }
        fireEvent(el, 'timeupdate');
        checkCueChanges(el);
        var ended = __lumen_video_ended(nid, nowMs());
        if (ended) {
          clearInterval(_tupdateTimer); _tupdateTimer = null;
          fireEvent(el, 'ended');
          if (hasAttr('loop')) {
            __lumen_video_seek(nid, 0, nowMs());
            __lumen_video_play(nid, nowMs());
            startTupdate();
          }
        }
      }, TUPDATE_MS);
    }

    // ── properties ───────────────────────────────────────────────────────────

    // `src` reflects the content attribute and, per HTML LS, returns it
    // *resolved*; the setter always re-runs the load algorithm, because the
    // spec keys that on the attribute being «set or changed», not on the value
    // actually differing.
    Object.defineProperty(el, 'src', {
      get: function() { var a = attr('src'); return a === null ? '' : (resolveUrl(a) || a); },
      set: function(v) {
        if (el.setAttribute) el.setAttribute('src', String(v === undefined || v === null ? '' : v));
        mediaLoadAlgorithm();
      },
      configurable: true,
    });

    Object.defineProperty(el, 'currentSrc',   { get: function() { return _currentSrc; },   configurable: true });
    Object.defineProperty(el, 'networkState', { get: function() { return _networkState; }, configurable: true });
    Object.defineProperty(el, 'readyState',   { get: function() { return _readyState; },   configurable: true });
    Object.defineProperty(el, 'error',        { get: function() { return _error; },        configurable: true });
    Object.defineProperty(el, 'seeking',      { get: function() { return false; },         configurable: true });

    Object.defineProperty(el, 'currentTime', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_current_time(nid, nowMs());
        return 0;
      },
      set: function(v) {
        var secs = Number(v) || 0;
        if (_gifBacked && HAS_STORE && nid) __lumen_video_seek(nid, secs, nowMs());
        // With no media resource there is nothing to seek in: §4.8.11.9 stores
        // the value as the default playback start position and fires nothing.
        if (_readyState !== HAVE_NOTHING) { queueEvent('seeking'); queueEvent('seeked'); }
        checkCueChanges(el);
      },
      configurable: true,
    });

    Object.defineProperty(el, 'duration', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_duration(nid);
        return NaN;  // §4.8.11.6: NaN while readyState is HAVE_NOTHING
      },
      configurable: true,
    });

    Object.defineProperty(el, 'paused', {
      get: function() { return isPaused(); },
      configurable: true,
    });

    Object.defineProperty(el, 'ended', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_ended(nid, nowMs());
        return false;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'videoWidth', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_width(nid);
        return 0;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'videoHeight', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_height(nid);
        return 0;
      },
      configurable: true,
    });

    // textTracks — lazily built from the shell's parsed <track> snapshot.
    // Rebuilt while empty so late shell-side population is picked up, and
    // rebuilt again whenever the shim's own list for this media element grew
    // (BUG-775: a script-inserted <track> joins it after the first read).
    Object.defineProperty(el, 'textTracks', {
      get: function() {
        var jsLen = (nid && _lumen_track_media_lists[nid]) ? _lumen_track_media_lists[nid].length : 0;
        var cached = el.__lumen_text_tracks;
        if (!cached || cached.length === 0 || (jsLen > 0 && cached._jsLen !== jsLen)) {
          cached = el.__lumen_text_tracks = buildTextTracks(el, nid);
        }
        return cached;
      },
      configurable: true,
    });

    // §4.8.11.11: `volumechange` is queued whenever *either* of the two values
    // changes — hence the equality guards, and hence the task hop (BUG-825: the
    // event fired from neither setter in neither handler form).
    Object.defineProperty(el, 'volume', {
      get: function(){ return _volume; },
      set: function(v) {
        var n = Number(v);
        if (isNaN(n) || n < 0 || n > 1) throw domException('volume must be in the range 0..1', 'IndexSizeError');
        if (n === _volume) return;
        _volume = n;
        queueEvent('volumechange');
      },
      configurable: true,
    });

    Object.defineProperty(el, 'muted', {
      get: function(){ return _muted; },
      set: function(v) {
        var b = !!v;
        if (b === _muted) return;
        _muted = b;
        queueEvent('volumechange');
      },
      configurable: true,
    });

    // §4.8.11.10: same rule for `ratechange` over playbackRate and
    // defaultPlaybackRate. Neither property existed at all before BUG-825, so
    // `v.playbackRate = 2` merely created an expando.
    Object.defineProperty(el, 'playbackRate', {
      get: function(){ return _rate; },
      set: function(v) {
        var n = Number(v);
        if (isNaN(n) || !isFinite(n)) throw new TypeError('playbackRate must be a finite number');
        if (n === _rate) return;
        _rate = n;
        queueEvent('ratechange');
      },
      configurable: true,
    });

    Object.defineProperty(el, 'defaultPlaybackRate', {
      get: function(){ return _defaultRate; },
      set: function(v) {
        var n = Number(v);
        if (isNaN(n) || !isFinite(n)) throw new TypeError('defaultPlaybackRate must be a finite number');
        if (n === _defaultRate) return;
        _defaultRate = n;
        queueEvent('ratechange');
      },
      configurable: true,
    });

    // `controls`/`loop`/`autoplay` are deliberately NOT own accessors: the ones
    // that used to sit here kept their value in a closure and never touched the
    // content attribute, so `video.controls = true` was invisible to layout and
    // paint. dom.rs already reflects all three on HTMLVideoElement.prototype.

    var _emptyRanges = { length: 0, start: function(){ return 0; }, end: function(){ return 0; } };
    function ranges() {
      if (!(_gifBacked && HAS_STORE && nid)) return _emptyRanges;
      var d = __lumen_video_duration(nid);
      if (isNaN(d) || d <= 0 || d === Infinity) return _emptyRanges;
      return { length: 1, start: function(){ return 0; }, end: function(){ return d; } };
    }
    Object.defineProperty(el, 'buffered', { get: ranges,                            configurable: true });
    Object.defineProperty(el, 'seekable', { get: ranges,                            configurable: true });
    Object.defineProperty(el, 'played',   { get: function(){ return _emptyRanges; }, configurable: true });

    // ── methods ───────────────────────────────────────────────────────────────

    el.play = function() {
      // §4.8.11.8 step 1: an element that already failed to find a playable
      // resource rejects rather than pretending to start.
      if (_error && _error.code === MEDIA_ERR_SRC_NOT_SUPPORTED) {
        return Promise.reject(domException('the media resource is not supported', 'NotSupportedError'));
      }
      if (_networkState === NETWORK_EMPTY) mediaLoadAlgorithm();
      if (_gifBacked && HAS_STORE && nid) {
        __lumen_video_play(nid, nowMs());
        _paused = false;
        queueEvent('play');
        queueEvent('playing');
        startTupdate();
        return Promise.resolve();
      }
      // No decodable resource (yet). The spec leaves this promise pending until
      // playback actually begins; a promise that never settles takes the rest of
      // a testharness file with it (the BUG-823 shape), so it resolves and the
      // element reports itself as playing-but-starved through `waiting`.
      _paused = false;
      queueEvent('play');
      queueEvent('waiting');
      return Promise.resolve();
    };

    el.pause = function() {
      if (_networkState === NETWORK_EMPTY) mediaLoadAlgorithm();
      var wasPaused = isPaused();
      if (_gifBacked && HAS_STORE && nid) __lumen_video_pause(nid, nowMs());
      if (_tupdateTimer !== null) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
      _paused = true;
      if (!wasPaused) { queueEvent('timeupdate'); queueEvent('pause'); }
    };

    el.load = function() { mediaLoadAlgorithm(); };

    el.canPlayType = function(type) {
      return HAS_STORE ? __lumen_video_can_play_type(type) : '';
    };

    el.fastSeek = function(t) {
      if (_gifBacked && HAS_STORE && nid) __lumen_video_seek(nid, Number(t) || 0, nowMs());
    };

    // A <source> appended after the element settled with no resource re-enters
    // the load algorithm (HTML §4.8.11.5); the hook is keyed by nid because the
    // insertion is noticed on the child, in dom.rs.
    if (nid) {
      _lumen_media_hooks[nid] = function () {
        if (_networkState === NETWORK_EMPTY) mediaLoadAlgorithm();
      };
    }

    // A parser-written <video> is patched before the page's first script runs,
    // so this is the element's first load — and, because every event it produces
    // is queued, an inline `onloadstart`/`onerror` still catches it.
    if (hasAttr('src') || sourceChildren().length > 0) mediaLoadAlgorithm();

    // Fire an initial cuechange for cues active at t=0 once the shell has
    // parsed the <track> files (deferred so late population is picked up).
    if (typeof setTimeout === 'function') {
      setTimeout(function() { try { checkCueChanges(el); } catch(e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); } }, 0);
    }
  }

  // Patch existing <video> elements.
  if (typeof document !== 'undefined' && document.querySelectorAll) {
    try {
      var videos = document.querySelectorAll('video');
      for (var i = 0; i < videos.length; i++) patchVideoElement(videos[i]);
    } catch(e) {}
  }

  // Intercept future document.createElement('video') calls.
  if (typeof document !== 'undefined' && document.createElement) {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag) {
      var el = _origCreate(tag);
      if (typeof tag === 'string' && tag.toLowerCase() === 'video') {
        patchVideoElement(el);
      }
      return el;
    };
  }
})();
"#;

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

    #[test]
    fn can_play_type_mp4_empty() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); el.canPlayType('video/mp4') === ''",
        );
        assert!(ok, "canPlayType('video/mp4') should return ''");
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
            rt.install_dom(empty_doc(), "", None, None, None, None, None, None, None, None, false)
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
    }
}
