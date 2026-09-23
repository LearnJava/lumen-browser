(function() {
  'use strict';

  // BUG-1033: shared synchronous pump registry — see the identical bootstrap
  // in `audio_element.rs`'s shim (defined once, whichever of the two shims
  // loads first). GIF/FFmpeg load polling below registers on this instead of
  // an independent `setInterval`, so a page's `<audio>`/`<video>` elements
  // all resolve their load/error state in one fixed, deterministic order per
  // tick rather than racing each other's decoder thread's real response time.
  if (!globalThis._lumen_media_pumps) {
    globalThis._lumen_media_pumps = [];
    globalThis._lumen_pump_media = function() {
      var arr = globalThis._lumen_media_pumps;
      for (var i = arr.length - 1; i >= 0; i--) {
        var keep;
        try { keep = arr[i](); } catch (e) { keep = false; }
        if (keep === false) { arr.splice(i, 1); }
      }
    };
  }

  var HAS_STORE = (typeof __lumen_video_load === 'function');
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

  // GAP-MEDIADECODE срез 6: extension sniff for the containers
  // `__lumen_video_ffmpeg_load` (feature `ffmpeg-video`) may decode. Same
  // pre-fetch, extension-based limitation `isGifSrc` already accepts — the
  // real gate is `HAS_FFMPEG_LOAD` (native only registered with the feature).
  var HAS_FFMPEG_LOAD = (typeof __lumen_video_ffmpeg_load === 'function');
  function isFfmpegSrc(src) {
    if (!src) return false;
    var base = src.split('?')[0].split('#')[0].toLowerCase();
    return base.endsWith('.mp4') || base.endsWith('.webm')
      || base.endsWith('.ogg') || base.endsWith('.ogv');
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

  var MEDIA_ERR_DECODE = 3;
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

  // BUG-570: cue *data* has always been real — `appendCues` above has built
  // plain `{id,startTime,endTime,text,track,pauseOnExit}` records since
  // BUG-775 — what was missing is the JS-visible interface layer itself:
  // neither `TextTrackCue` nor `VTTCue` was ever installed as a global, so
  // `new VTTCue(...)` threw `ReferenceError` and `instanceof` checks against
  // either name were unsatisfiable. `TextTrackCue` is the spec's abstract
  // base (`interface TextTrackCue : EventTarget`, HTML LS §4.8.11.13) — WebIDL
  // gives it no constructor operation, so `new TextTrackCue(...)` must throw a
  // TypeError even though `VTTCue.prototype`'s prototype chain still runs
  // through it.
  // `EventTarget` is a page-shim global installed well before this file runs
  // in the real browser (`install_video_bindings_v8` is called after
  // `WEB_API_SHIM`, see `v8_runtime.rs`), but this file's own unit tests run
  // against a bare runtime with no page shim at all — guard the base so an
  // unrelated video-only test doesn't start failing on a top-level
  // `ReferenceError` raised just by *defining* the cue classes.
  var _lumen_cue_base = (typeof EventTarget === 'function') ? EventTarget : function () {};
  function TextTrackCue() {
    throw new TypeError("Illegal constructor");
  }
  TextTrackCue.prototype = Object.create(_lumen_cue_base.prototype);
  TextTrackCue.prototype.constructor = TextTrackCue;

  // VTTCue (WebVTT §3.1) — the only constructible cue type. Layout-affecting
  // members (`region`/`vertical`/`snapToLines`/`line`/`lineAlign`/`position`/
  // `positionAlign`/`size`/`align`) are stored and echoed back at their spec
  // defaults; nothing downstream reads them yet, since rendering still goes
  // through the plain `{startTime,endTime,text}` triple `appendCues` builds —
  // data-correct but visually inert until BUG-570's sibling rendering gaps
  // close. `track` starts null; wiring it to a real `addCue`/`removeCue` pair
  // is the separate, already-documented `CAPABILITIES.md` method-layer gap,
  // not this bug.
  function VTTCue(startTime, endTime, text) {
    if (!(this instanceof VTTCue)) {
      throw new TypeError("Failed to construct 'VTTCue': Please use the 'new' operator.");
    }
    if (arguments.length < 3) {
      throw new TypeError("Failed to construct 'VTTCue': 3 arguments required, but only " + arguments.length + " present.");
    }
    _lumen_cue_base.call(this);
    this.id = '';
    this.pauseOnExit = false;
    this.startTime = +startTime;
    this.endTime = +endTime;
    this.text = String(text);
    this.region = null;
    this.vertical = '';
    this.snapToLines = true;
    this.line = 'auto';
    this.lineAlign = 'start';
    this.position = 'auto';
    this.positionAlign = 'auto';
    this.size = 100;
    this.align = 'center';
    this.track = null;
    this.onenter = null;
    this.onexit = null;
  }
  VTTCue.prototype = Object.create(TextTrackCue.prototype);
  VTTCue.prototype.constructor = VTTCue;
  // WebVTT §3.5.17 "cue text rendering rules" — full markup parsing (`<i>`/
  // `<b>`/timestamps/…) is not implemented; this returns the cue text as one
  // plain Text node, correct for the dominant markup-free case.
  VTTCue.prototype.getCueAsHTML = function() {
    var frag = document.createDocumentFragment();
    frag.appendChild(document.createTextNode(this.text));
    return frag;
  };
  globalThis.TextTrackCue = TextTrackCue;
  globalThis.VTTCue = VTTCue;

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
      // `isTrusted` because the engine fires it, not the page — the same thing
      // BUG-838 had to add to `_lumen_resource_fire` for `<script>`/`<link>`,
      // and the only way a handler can tell this event from a synthesized one.
      var ev = new Event(type, { bubbles: false, cancelable: false, isTrusted: true });
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
    // GAP-CSPENF срез 17: `media-src` covers text tracks as well as video and
    // audio (CSP3 §6.1 «media-src»), so the same gate runs here — before the
    // `fetch()` below, so a blocked track never reaches the network. Only this
    // branch is gated: the `blob:`/`data:` branches above are read locally out
    // of the object-URL store and never touch the network at all.
    if (typeof _lumen_check_media_src === 'function' && !_lumen_check_media_src(abs)) {
      if (typeof _lumen_fire_media_src_violation === 'function') {
        _lumen_fire_media_src_violation(_lumen_media_src_last_csp_block());
      }
      // Rejecting routes through `failTrackLoad` exactly like a failed fetch:
      // `readyState = ERROR` plus an `error` event on the `<track>` element.
      return Promise.reject(new Error('blocked by Content Security Policy'));
    }
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
    // building it on completion instead would order it by how fast each file
    // happened to arrive. The position is read off the *tree* rather than taken
    // as «append», because the two entry points do not run in tree order:
    // BUG-804's parser scan runs after the document's own scripts, so a track a
    // head script appended to a markup `<video>` would otherwise stand ahead of
    // the one written above it in the markup.
    var tt = ensureTrackObject(nid);
    var list = _lumen_track_media_lists[mediaNid];
    if (!list) list = _lumen_track_media_lists[mediaNid] = [];
    if (list.indexOf(tt) < 0) {
      var kids = (typeof _lumen_get_children === 'function') ? _lumen_get_children(mediaNid) : null;
      var pos = list.length;
      if (kids) {
        pos = 0;
        for (var ci = 0; ci < kids.length; ci++) {
          if (+kids[ci] === +nid) break;
          var sib = _lumen_track_states[kids[ci]];
          if (sib && sib.track && list.indexOf(sib.track) >= 0) pos++;
        }
      }
      list.splice(pos, 0, tt);
    }

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

  // The parser's half of the same model (BUG-804). A <track> written by the
  // HTML parser never passes through the insertion hook in `dom.rs` — that one
  // covers elements minted by createElement — so the markup's tracks are picked
  // up in one pass once parsing is done, the shape `_lumen_link_hints_scan` and
  // `_lumen_style_blocks_scan` already use for their own elements. The pass runs
  // after the document's own scripts, so a handler armed below the markup (which
  // is how every `track-webvtt-*.html` test writes it) is already in place.
  //
  // This is also where the «who owns `video.textTracks`» question BUG-804 left
  // open is answered, in favour of the JS list, and it has to be: the shell's
  // snapshot (`tracks::load_video_tracks` → `__lumen_texttracks_json`) is keyed
  // by the <video> and carries no <track> identity at all, so `trackElement.track`
  // could never be the same object the media element lists — and that identity is
  // what the tests read inside the handler they were waiting for. The JS list is
  // also the more spec-correct of the two wherever they differ (`kind`'s
  // missing/invalid defaults, `mode` from `default` rather than from a
  // first-subtitles-track heuristic, `<audio>` counting as a media element at
  // all), and it already won for a script-created track since BUG-775 — leaving
  // markup on the snapshot would mean one page having two owners depending on
  // how each of its tracks happened to get there.
  //
  // What does NOT move is painting: the shell keeps its own cue store and its own
  // walk, so the overlay is unchanged, at the price of the file being fetched
  // twice. That is the approximation `_lumen_link_prepare` and the `@import` path
  // already carry, and one the createElement path has been paying since BUG-775
  // anyway — a script-built track whose <video> is in the document is fetched by
  // both halves today.
  function scanTrackElements() {
    var tracks;
    try { tracks = document.getElementsByTagName('track'); } catch (e) { return; }
    if (!tracks) return;
    for (var i = 0; i < tracks.length; i++) {
      var el = tracks[i];
      if (!el || el.__nid__ === undefined) continue;
      var nid = el.__nid__;
      // Already held by the insertion hook — a script built this one and it is
      // still waiting for a media parent. Registering it a second time below
      // would leak the hook's pending counter.
      if (typeof _lumen_resource_pending !== 'undefined' && _lumen_resource_pending
          && _lumen_resource_pending[nid] !== undefined) continue;
      // Returns true once the model has started, and true again for an element
      // it has already started — so a track a head script appended to a <video>
      // that is in the markup is not loaded twice.
      if (startTrackLoad(nid)) continue;
      // No media parent. Hand the element to the insertion hook so a later
      // re-parenting starts the model, the same contract createElement has.
      if (typeof _lumen_resource_track === 'function') _lumen_resource_track(nid, 'track');
    }
  }
  globalThis._lumen_track_elements_scan = scanTrackElements;

  // A media element's list of text tracks exists from the moment the ELEMENTS
  // do: §4.8.11.1 adds a `<track>`'s text track when the track element is
  // *inserted*, long before its file arrives. For markup that insertion already
  // happened — the shell hands this shim a fully parsed document — and the
  // document's own scripts run BEFORE the `interactive` pass above, so the list
  // has to be built here rather than there.
  //
  // Measured, not assumed: `track-webvtt-utf8.html`, `-timings-hour.html` and
  // `-header-comment.html` open with
  //     for (var i = 0; i < video.textTracks.length; i++)
  //         trackElements[i].onload = t.step_func(trackLoaded);
  // so a list that is still empty at that moment arms **no handler at all**, and
  // the test times out no matter how correctly the loads later report. Those
  // three were the only ones of the bug's nine that the scan alone did not fix.
  function listMarkupTracks(mediaNid) {
    if (typeof _lumen_get_children !== 'function') return;
    var kids;
    try { kids = _lumen_get_children(mediaNid); } catch (e) { return; }
    if (!kids) return;
    // Walked in tree order, so a plain append is the spec's order here.
    for (var i = 0; i < kids.length; i++) {
      var tag = (typeof _lumen_get_tag_name === 'function')
        ? String(_lumen_get_tag_name(kids[i]) || '').toUpperCase() : '';
      if (tag !== 'TRACK') continue;
      var st = trackState(kids[i]);
      if (st.media === null) st.media = mediaNid;
      var tt = ensureTrackObject(kids[i]);
      var list = _lumen_track_media_lists[mediaNid];
      if (!list) list = _lumen_track_media_lists[mediaNid] = [];
      if (list.indexOf(tt) < 0) list.push(tt);
    }
  }

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

  // `textTracks` is an HTMLMediaElement member, and until BUG-804 it existed
  // only as an own property `patchVideoElement` puts on each <video> wrapper —
  // so an <audio> (whose own model lives in `audio_element.rs` and never went
  // through that patch) had none at all, and the `<track>` the parser wrote
  // under one had nowhere to be listed once it started loading. Declared on the
  // prototype, so the <video> own property still shadows it and that path is
  // untouched; the wrapper is interned per nid (BUG-849), so caching on `this`
  // is as stable as the <video> version's caching on `el`.
  if (typeof HTMLMediaElement === 'function') {
    Object.defineProperty(HTMLMediaElement.prototype, 'textTracks', {
      get: function() {
        var nid = this.__nid__;
        var jsLen = (nid && _lumen_track_media_lists[nid]) ? _lumen_track_media_lists[nid].length : 0;
        var cached = this.__lumen_text_tracks;
        if (!cached || cached.length === 0 || (jsLen > 0 && cached._jsLen !== jsLen)) {
          cached = this.__lumen_text_tracks = buildTextTracks(this, nid);
        }
        return cached;
      },
      configurable: true,
    });
  }

  function patchVideoElement(el) {
    if (el.__lumen_video_patched) return;
    el.__lumen_video_patched = true;

    var nid      = el.__nid__;
    var _volume  = 1.0;
    var _muted   = !!(el.hasAttribute && el.hasAttribute('muted'));
    var _defaultRate = 1.0;
    var _rate        = 1.0;
    // Seed the native side with the markup-declared `muted` attribute — the
    // `muted` property's own setter below is only reached from script, so a
    // plain `<video muted>` would otherwise never tell the audio sink.
    if (nid && _muted && typeof __lumen_video_set_muted === 'function') __lumen_video_set_muted(nid, true);
    var _networkState = NETWORK_EMPTY;
    var _readyState   = HAVE_NOTHING;
    var _currentSrc   = '';
    var _error        = null;
    var _paused       = true;
    // Bumped by every media load algorithm run; a selection or fetch whose
    // generation is stale silently drops itself instead of racing the new one.
    var _generation   = 0;
    var _tupdateTimer = null;
    var _gifBacked = false;    // true once a GIF is successfully loaded
    var _ffmpegBacked = false; // true once an FFmpeg container is successfully loaded (срез 6)
    function decoded() { return (_gifBacked || _ffmpegBacked) && HAS_STORE; }

    function attr(name) {
      var v = (el.getAttribute && el.getAttribute(name));
      return (v === undefined || v === null) ? null : String(v);
    }
    function hasAttr(name) { return !!(el.hasAttribute && el.hasAttribute(name)); }
    function queueEvent(name) { queueTask(function () { fireEvent(el, name); }); }
    function stopTimers() {
      // The load poll itself needs no explicit stop: it lives on the shared
      // `_lumen_media_pumps` registry and self-removes once `gen !==
      // _generation` (bumped by `mediaLoadAlgorithm` right below), same as
      // every other stale-generation guard in this file.
      if (_tupdateTimer !== null) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
    }
    function isPaused() {
      return (decoded() && nid) ? __lumen_video_paused(nid) : _paused;
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
        if (decoded() && nid) __lumen_video_pause(nid, nowMs());
        _gifBacked    = false;
        _ffmpegBacked = false;
        _paused      = true;
        _readyState  = HAVE_NOTHING;
        _currentSrc  = '';
        _networkState = NETWORK_EMPTY;
      }
      _rate  = _defaultRate;
      // Mirror the reset natively — otherwise a `load()` after `playbackRate`
      // had been changed would leave the native `currentTime` timer scaled
      // by the stale rate until the page happened to set `playbackRate`
      // again (GAP-MEDIADECODE срез 18).
      if (nid && typeof __lumen_video_set_playback_rate === 'function') {
        __lumen_video_set_playback_rate(nid, _rate, nowMs());
      }
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
        // BUG-925: `loading="lazy"` (HTML LS §4.8.11) defers everything past
        // this point until the element is both "being rendered" (connected,
        // not `hidden`/`display:none` — unlike `<audio>`, `<video>` needs no
        // `controls` check, it is inherently visual) and intersects the
        // viewport. `_pendingLazyGen` guards a stale IntersectionObserver
        // callback (superseded by a newer `load()`/`src=`) against firing
        // out of order.
        var loadingAttr = attr('loading') || '';
        if (String(loadingAttr).toLowerCase() === 'lazy') {
          _pendingLazyGen = gen;
          _lumen_defer_lazy_media_load(el, function() {
            if (gen !== _generation || _pendingLazyGen !== gen) return; // superseded
            if (!_lumen_media_is_rendered(el, /* requiresControls */ false)) return; // stays pending
            _pendingLazyGen = null;
            resourceSelectionNow(gen);
          });
          return;
        }
        resourceSelectionNow(gen);
      });
    }

    function resourceSelectionNow(gen) {
      if (hasAttr('src')) { startFetch(gen, attr('src') || '', null); return; }
      var candidates = sourceChildren();
      // Step 6 «otherwise»: no src attribute and no <source> child at all.
      if (candidates.length === 0) { _networkState = NETWORK_EMPTY; return; }
      nextCandidate(gen, candidates, 0);
    }

    // Force a deferred resource selection to run now, bypassing the
    // IntersectionObserver wait — HTML LS: switching `loading` away from
    // `lazy` (property, `setAttribute`, or `removeAttribute`) starts it
    // immediately if one was pending.
    var _pendingLazyGen = null;
    function _resumeLazyLoadNow() {
      if (_pendingLazyGen === null) return;
      var gen = _pendingLazyGen;
      _pendingLazyGen = null;
      _lumen_cancel_lazy_media_load(el);
      if (gen === _generation) resourceSelectionNow(gen);
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
      // GAP-CSPENF срез 17: `media-src`/`default-src` gate, checked before the
      // URL is queued for the shell's GIF fetch below — same "not a single
      // outgoing byte" principle img-src/script-src/style-src already give
      // their producers (срезы 4/6/7). `<video>` has no `&Document`-backed gate
      // in `lumen-shell` (this whole path is JS-shim driven), so the check is a
      // native binding instead. The gate sits ahead of the format check, not
      // behind it: a blocked source is a CSP failure whatever its container
      // would have been, so a non-GIF `src` now reports `media-src` rather than
      // "unsupported media format".
      if (typeof _lumen_check_media_src === 'function' && !_lumen_check_media_src(abs)) {
        if (typeof _lumen_fire_media_src_violation === 'function') {
          _lumen_fire_media_src_violation(_lumen_media_src_last_csp_block());
        }
        failResource(gen, candidate, 'blocked by Content Security Policy');
        return;
      }
      if (startGifLoad(gen, url)) return;
      if (startFfmpegLoad(gen, url)) return;
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
      __lumen_video_load(nid, src);
      // BUG-1033: poll on the shared media pump (same fixed per-tick order
      // as `audio_element.rs`'s `pollLoad`) until the shell has decoded the
      // GIF, instead of an independent `setInterval`.
      globalThis._lumen_media_pumps.push(function() {
        if (gen !== _generation) return false;
        if (!__lumen_video_ready(nid)) return true;
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
        return false;
      });
      return true;
    }

    // ── FFmpeg-container load (GAP-MEDIADECODE срез 6) ──────────────────────
    //
    // Mirrors `startGifLoad` exactly, including the polling model: the shell
    // has no "decode this on the next tick" callback surface, so readiness is
    // observed by polling the same `__lumen_video_ready(nid)` the GIF path
    // uses (the shell would insert into the same `playback` map — срез 7, not
    // yet wired, so this currently polls forever for a real container, same
    // as any other unsupported format did before this slice).
    function startFfmpegLoad(gen, src) {
      if (!HAS_FFMPEG_LOAD || !nid) return false;
      if (!isFfmpegSrc(src)) return false;
      __lumen_video_ffmpeg_load(nid, src);
      // BUG-1033: same shared-pump model as `startGifLoad` above.
      globalThis._lumen_media_pumps.push(function() {
        if (gen !== _generation) return false;
        // GAP-MEDIADECODE срез 9: a corrupted/undecodable container never
        // reaches `playback`, so `__lumen_video_ready` alone would poll
        // forever — check failure first so such a source reports a real
        // `error` event instead of hanging silently in NETWORK_LOADING.
        if (typeof __lumen_video_failed === 'function' && __lumen_video_failed(nid)) {
          _error = makeMediaError(MEDIA_ERR_DECODE, 'unable to decode media resource');
          _readyState = HAVE_NOTHING;
          _networkState = NETWORK_NO_SOURCE;
          fireEvent(el, 'error');
          return false;
        }
        if (!__lumen_video_ready(nid)) return true;
        _ffmpegBacked = true;
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
        return false;
      });
      return true;
    }

    // ── timeupdate loop ───────────────────────────────────────────────────────

    function startTupdate() {
      if (_tupdateTimer !== null) return;
      if (typeof setInterval !== 'function') return;
      _tupdateTimer = setInterval(function() {
        if (!decoded() || __lumen_video_paused(nid)) {
          clearInterval(_tupdateTimer); _tupdateTimer = null; return;
        }
        fireEvent(el, 'timeupdate');
        checkCueChanges(el);
        var ended = __lumen_video_ended(nid, nowMs());
        if (ended) {
          clearInterval(_tupdateTimer); _tupdateTimer = null;
          if (hasAttr('loop')) {
            fireEvent(el, 'ended');
            __lumen_video_seek(nid, 0, nowMs());
            __lumen_video_play(nid, nowMs());
            startTupdate();
          } else {
            // §4.8.11.8 "reaches the end": pause the native sink so the Rust
            // tick loop (`tick_video_ffmpegs`) stops decoding past `duration` —
            // otherwise it keeps calling `decode_audio_pcm` on an exhausted
            // demuxer and logs EOF errors every tick.
            if (!isPaused()) fireEvent(el, 'pause');
            __lumen_video_pause(nid, nowMs());
            _paused = true;
            fireEvent(el, 'ended');
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

    // BUG-925: `loading` (HTML LS §4.8.11) — enumerated reflected attribute,
    // missing/invalid → 'eager'. Own accessor (like `src` above) rather than
    // the shared `_lumen_install_reflection` row (installed as a fallback in
    // `web_api_shim_tail_b.js` for the pre-patch window) so the setter can
    // resume a deferred lazy load; routed through the instance's
    // `setAttribute` override below so a bare `el.setAttribute('loading', …)`
    // gets the same resume check.
    Object.defineProperty(el, 'loading', {
      get: function() {
        var v = attr('loading');
        if (v === null) return 'eager';
        v = v.toLowerCase();
        return v === 'lazy' ? 'lazy' : 'eager';
      },
      set: function(v) { if (el.setAttribute) el.setAttribute('loading', String(v)); },
      configurable: true,
    });
    if (el.setAttribute) {
      var _origSetAttribute = el.setAttribute.bind(el);
      el.setAttribute = function(name, value) {
        _origSetAttribute(name, value);
        if (String(name).toLowerCase() === 'loading' && String(value).toLowerCase() !== 'lazy') {
          _resumeLazyLoadNow();
        }
      };
    }
    if (el.removeAttribute) {
      var _origRemoveAttribute = el.removeAttribute.bind(el);
      el.removeAttribute = function(name) {
        _origRemoveAttribute(name);
        if (String(name).toLowerCase() === 'loading') _resumeLazyLoadNow();
      };
    }

    Object.defineProperty(el, 'currentSrc',   { get: function() { return _currentSrc; },   configurable: true });
    Object.defineProperty(el, 'networkState', { get: function() { return _networkState; }, configurable: true });
    Object.defineProperty(el, 'readyState',   { get: function() { return _readyState; },   configurable: true });
    Object.defineProperty(el, 'error',        { get: function() { return _error; },        configurable: true });
    Object.defineProperty(el, 'seeking',      { get: function() { return false; },         configurable: true });

    Object.defineProperty(el, 'currentTime', {
      get: function() {
        if (decoded() && nid) return __lumen_video_current_time(nid, nowMs());
        return 0;
      },
      set: function(v) {
        var secs = Number(v) || 0;
        if (decoded() && nid) __lumen_video_seek(nid, secs, nowMs());
        // With no media resource there is nothing to seek in: §4.8.11.9 stores
        // the value as the default playback start position and fires nothing.
        if (_readyState !== HAVE_NOTHING) { queueEvent('seeking'); queueEvent('seeked'); }
        checkCueChanges(el);
      },
      configurable: true,
    });

    Object.defineProperty(el, 'duration', {
      get: function() {
        if (decoded() && nid) return __lumen_video_duration(nid);
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
        if (decoded() && nid) return __lumen_video_ended(nid, nowMs());
        return false;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'videoWidth', {
      get: function() {
        if (decoded() && nid) return __lumen_video_width(nid);
        return 0;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'videoHeight', {
      get: function() {
        if (decoded() && nid) return __lumen_video_height(nid);
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
        if (nid && typeof __lumen_video_set_volume === 'function') __lumen_video_set_volume(nid, n);
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
        if (nid && typeof __lumen_video_set_muted === 'function') __lumen_video_set_muted(nid, b);
        queueEvent('volumechange');
      },
      configurable: true,
    });

    // §4.8.11.10: same rule for `ratechange` over playbackRate and
    // defaultPlaybackRate. Neither property existed at all before BUG-825, so
    // `v.playbackRate = 2` merely created an expando.
    //
    // GAP-MEDIADECODE срез 18: the native store's `currentTime` timer is
    // rescaled by this value (`__lumen_video_set_playback_rate`); the actual
    // decode/PCM rate is NOT — a faster/slower `currentTime` on a real
    // decode backend is a separate, more invasive piece of work (see the
    // remainder note left in ROADMAP.md's GAP-MEDIADECODE row, срез 17).
    Object.defineProperty(el, 'playbackRate', {
      get: function(){ return _rate; },
      set: function(v) {
        var n = Number(v);
        if (isNaN(n) || !isFinite(n)) throw new TypeError('playbackRate must be a finite number');
        if (n === _rate) return;
        _rate = n;
        if (nid && typeof __lumen_video_set_playback_rate === 'function') {
          __lumen_video_set_playback_rate(nid, n, nowMs());
        }
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
      if (!(decoded() && nid)) return _emptyRanges;
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
      if (decoded() && nid) {
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
      if (decoded() && nid) __lumen_video_pause(nid, nowMs());
      if (_tupdateTimer !== null) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
      _paused = true;
      if (!wasPaused) { queueEvent('timeupdate'); queueEvent('pause'); }
    };

    el.load = function() { mediaLoadAlgorithm(); };

    el.canPlayType = function(type) {
      return HAS_STORE ? __lumen_video_can_play_type(type) : '';
    };

    el.fastSeek = function(t) {
      if (decoded() && nid) __lumen_video_seek(nid, Number(t) || 0, nowMs());
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

  // Patch existing <video> elements, and list the <track> children of every
  // media element the parser wrote — see `listMarkupTracks` for why the listing
  // cannot wait for the `interactive` pass that starts the loads. `<audio>` is
  // in the second loop only: its own model lives in `audio_element.rs` and must
  // not be patched here.
  if (typeof document !== 'undefined' && document.querySelectorAll) {
    try {
      var videos = document.querySelectorAll('video');
      for (var i = 0; i < videos.length; i++) patchVideoElement(videos[i]);
    } catch(e) {}
    try {
      var media = document.querySelectorAll('video, audio');
      for (var mi = 0; mi < media.length; mi++) {
        if (media[mi] && media[mi].__nid__ !== undefined) listMarkupTracks(media[mi].__nid__);
      }
    } catch(e) {}
  }

  // Intercept future document.createElement('video') calls. Forwards every
  // argument (not just `tag`) — GAP-CEREG срез 2 (BUG-890) added a second
  // `options` parameter (`{customElements: registry}`) to the native
  // `createElement`, and an arity-1 wrapper here silently dropped it on
  // every page that loads the video shim, defeating registry scoping for
  // every element, not just `<video>`.
  if (typeof document !== 'undefined' && document.createElement) {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag, options) {
      var el = _origCreate(tag, options);
      if (typeof tag === 'string' && tag.toLowerCase() === 'video') {
        patchVideoElement(el);
      }
      return el;
    };
  }
})();
