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
      el.dispatchEvent(new Event(name, { bubbles: false, cancelable: false }));
    } catch(e) {}
  }

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
    var _src     = (el.getAttribute && el.getAttribute('src')) || '';
    var _muted   = !!(el.hasAttribute && el.hasAttribute('muted'));
    var _volume  = 1.0;
    var _controls= !!(el.hasAttribute && el.hasAttribute('controls'));
    var _loop    = !!(el.hasAttribute && el.hasAttribute('loop'));
    var _autoplay= !!(el.hasAttribute && el.hasAttribute('autoplay'));
    var _loadTimer    = null;
    var _tupdateTimer = null;
    var _gifBacked = false; // true once a GIF is successfully loaded

    // ── GIF load ─────────────────────────────────────────────────────────────

    function startGifLoad(src) {
      if (!HAS_STORE || !nid) return false;
      if (!isGifSrc(src)) return false;
      __lumen_video_load(nid, src);
      fireEvent(el, 'loadstart');
      // Poll until the shell has decoded the GIF.
      _loadTimer = setInterval(function() {
        if (!__lumen_video_ready(nid)) return;
        clearInterval(_loadTimer); _loadTimer = null;
        _gifBacked = true;
        fireEvent(el, 'durationchange');
        fireEvent(el, 'loadedmetadata');
        fireEvent(el, 'loadeddata');
        fireEvent(el, 'canplay');
        fireEvent(el, 'canplaythrough');
        if (_autoplay) el.play();
      }, POLL_MS);
      return true;
    }

    // ── timeupdate loop ───────────────────────────────────────────────────────

    function startTupdate() {
      if (_tupdateTimer !== null) return;
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
          if (_loop) {
            __lumen_video_seek(nid, 0, nowMs());
            __lumen_video_play(nid, nowMs());
            startTupdate();
          }
        }
      }, TUPDATE_MS);
    }

    // ── properties ───────────────────────────────────────────────────────────

    Object.defineProperty(el, 'src', {
      get: function() { return _src; },
      set: function(v) {
        var s = String(v || '');
        if (s === _src) return;
        _src = s;
        if (el.setAttribute) el.setAttribute('src', _src);
        _gifBacked = false;
        if (_loadTimer) { clearInterval(_loadTimer); _loadTimer = null; }
        if (_tupdateTimer) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
        if (!startGifLoad(_src)) {
          // Non-GIF: Phase 0 immediate events.
          try {
            el.dispatchEvent(new Event('loadedmetadata'));
            el.dispatchEvent(new Event('canplay'));
          } catch(e) {}
        }
      },
      configurable: true,
    });

    Object.defineProperty(el, 'currentTime', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_current_time(nid, nowMs());
        return 0;
      },
      set: function(v) {
        var secs = Number(v) || 0;
        if (_gifBacked && HAS_STORE && nid) __lumen_video_seek(nid, secs, nowMs());
        fireEvent(el, 'seeking'); fireEvent(el, 'seeked');
        checkCueChanges(el);
      },
      configurable: true,
    });

    Object.defineProperty(el, 'duration', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_duration(nid);
        return Infinity;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'paused', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_paused(nid);
        return true;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'ended', {
      get: function() {
        if (_gifBacked && HAS_STORE && nid) return __lumen_video_ended(nid, nowMs());
        return false;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'readyState', {
      get: function() { return _gifBacked ? 4 : (_src ? 0 : 4); },
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

    Object.defineProperty(el, 'muted',    { get: function(){ return _muted; },    set: function(v){ _muted = !!v; }, configurable: true });
    Object.defineProperty(el, 'volume',   { get: function(){ return _volume; },   set: function(v){ _volume = Math.max(0, Math.min(1, Number(v)||0)); }, configurable: true });
    Object.defineProperty(el, 'controls', { get: function(){ return _controls; }, set: function(v){ _controls = !!v; }, configurable: true });
    Object.defineProperty(el, 'loop',     { get: function(){ return _loop; },     set: function(v){ _loop = !!v; }, configurable: true });

    // ── methods ───────────────────────────────────────────────────────────────

    el.play = function() {
      if (_gifBacked && HAS_STORE && nid) {
        __lumen_video_play(nid, nowMs());
        fireEvent(el, 'play');
        fireEvent(el, 'playing');
        startTupdate();
        return Promise.resolve();
      }
      // Phase 0 fallback.
      fireEvent(el, 'play');
      fireEvent(el, 'playing');
      return Promise.resolve();
    };

    el.pause = function() {
      if (_gifBacked && HAS_STORE && nid) {
        __lumen_video_pause(nid, nowMs());
      }
      if (_tupdateTimer) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
      fireEvent(el, 'pause');
    };

    el.load = function() {
      if (_tupdateTimer) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
      _gifBacked = false;
      if (_src) startGifLoad(_src);
    };

    el.canPlayType = function(type) {
      return HAS_STORE ? __lumen_video_can_play_type(type) : '';
    };

    // If src attribute was already set before patching, trigger load.
    if (_src) {
      if (!startGifLoad(_src)) {
        try {
          el.dispatchEvent(new Event('loadedmetadata'));
          el.dispatchEvent(new Event('canplay'));
        } catch(e) {}
      }
    }

    // Fire an initial cuechange for cues active at t=0 once the shell has
    // parsed the <track> files (deferred so late population is picked up).
    if (typeof setTimeout === 'function') {
      setTimeout(function() { try { checkCueChanges(el); } catch(e) {} }, 0);
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

    #[test]
    fn duration_infinity_without_gif() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); el.duration === Infinity",
        );
        assert!(ok, "duration should be Infinity when no GIF loaded");
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

    #[test]
    fn ready_state_with_no_src() {
        let rt = with_video();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('video'); el.readyState === 4",
        );
        assert!(ok, "readyState should be 4 with no src");
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
