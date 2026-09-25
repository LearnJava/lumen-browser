//! MediaSession API (W3C Media Session §5).
//!
//! Installs `navigator.mediaSession` and `MediaMetadata` so that pages can
//! report playback state and rich metadata (title/artist/album/artwork) for
//! OS media controls without JS errors.
//!
//! Phase 0: the metadata and playback state are stored in JS objects but not
//! forwarded to the OS media-control surface (lock screen / SMTC / MPRIS).
//! Shell integration (P3) can read `_lumen_take_media_session_update()` to
//! pick up changes and wire them to platform APIs.
//!
//! Installed interfaces:
//! - `MediaMetadata` class — title/artist/album/artwork/chapterInfo, validated
//!   and frozen per WebIDL (`MediaImage.src` resolved against the base URL)
//! - `ChapterInformation` — read-only entries of `MediaMetadata.chapterInfo`
//! - `MediaPositionState` — duration/playbackRate/position
//! - `navigator.mediaSession` — MediaSession singleton
//!   - `metadata` getter/setter (MediaMetadata)
//!   - `playbackState` getter/setter ("none" | "paused" | "playing")
//!   - `setActionHandler(action, callback)` — play/pause/stop/seekbackward/
//!     seekforward/seekto/previoustrack/nexttrack/skipad/
//!     togglemicrophone/togglecamera/hangup/togglecaptionstrack
//!   - `setPositionState(state)`
//!   - `setCameraActive(active)` / `setMicrophoneActive(active)` (L2 §5.4)
//! - `window.MediaMetadata` exported as global

/// Install MediaSession API shim into the JS context.
///
/// Adds `navigator.mediaSession` with all W3C Media Session §5 methods and
/// exports `MediaMetadata` as a global. Changes to metadata and playbackState
/// are stored in JS state; `_lumen_take_media_session_update()` returns a JSON
/// snapshot for shell/OS integration.
///
/// Evaluated via [`lumen_core::ext::JsRuntime::eval`]. Must be called **after**
/// DOM install so that `navigator`, `Event`, and `JSON` are already defined.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_media_session_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(MEDIA_SESSION_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the MediaSession API (W3C Media Session §5).
#[cfg(feature = "v8-backend")]
const MEDIA_SESSION_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator === 'undefined') return;

  // ── WebIDL conversion helpers ─────────────────────────────────────────────
  // Dictionary conversion (WebIDL §3.2.18): undefined/null → empty dict, any
  // other non-object → TypeError.
  function toDict(v, ctx) {
    if (v === undefined || v === null) return {};
    if (typeof v !== 'object' && typeof v !== 'function') {
      throw new TypeError(ctx + ' is not of a dictionary type.');
    }
    return v;
  }
  // sequence<T> conversion: requires an iterable object.
  function toSequence(v, ctx) {
    if (v === null || (typeof v !== 'object' && typeof v !== 'function') ||
        typeof v[Symbol.iterator] !== 'function') {
      throw new TypeError(ctx + ' is not iterable.');
    }
    return Array.from(v);
  }
  // Base URL of the entry settings object (approximated by the document).
  function baseUrl() {
    try {
      if (typeof document !== 'undefined' && document) {
        return document.baseURI || document.URL;
      }
    } catch (_) {}
    try { return location.href; } catch (_) {}
    return undefined;
  }
  // MediaImage (§6.5): `src` is required; the stored copy carries only the
  // three dictionary members, `src` parsed against the base URL (invalid URL
  // → TypeError), and is frozen.
  function convertMediaImage(v) {
    var d = toDict(v, "Failed to read the 'artwork' member: MediaImage");
    if (d.src === undefined) {
      throw new TypeError("Failed to read the 'src' property from 'MediaImage': Required member is undefined.");
    }
    var src = String(d.src);
    var sizes = d.sizes === undefined ? '' : String(d.sizes);
    var type = d.type === undefined ? '' : String(d.type);
    if (typeof URL === 'function') {
      var base = baseUrl();
      try {
        src = (base === undefined ? new URL(src) : new URL(src, base)).href;
      } catch (_) {
        throw new TypeError("Failed to set artwork: invalid URL '" + src + "'.");
      }
    }
    return Object.freeze({ src: src, sizes: sizes, type: type });
  }
  // FrozenArray<MediaImage>: converts every element before anything is
  // stored, so a failure leaves the previous value intact.
  function convertArtwork(v) {
    var seq = toSequence(v, "The 'artwork' member");
    var out = [];
    for (var i = 0; i < seq.length; i++) out.push(convertMediaImage(seq[i]));
    return Object.freeze(out);
  }

  // WebIDL: interface members are enumerable; ES class accessors are not.
  function markEnumerable(proto, names) {
    names.forEach(function(n) {
      var desc = Object.getOwnPropertyDescriptor(proto, n);
      desc.enumerable = true;
      Object.defineProperty(proto, n, desc);
    });
  }

  // ── ChapterInformation (§6.3) ──────────────────────────────────────────────
  // No IDL constructor: only MediaMetadata builds instances (via CHAPTER_KEY).
  // `class` gives the IDL shape for free — non-writable `prototype`,
  // `get title`-named getters, `new` required.
  var CHAPTER_KEY = {};
  var _chapterState = new WeakMap();
  function chapterSlot(self) {
    var st = _chapterState.get(self);
    if (!st) throw new TypeError('Illegal invocation');
    return st;
  }
  class ChapterInformation {
    constructor(key = undefined, init = undefined) {
      if (key !== CHAPTER_KEY) throw new TypeError('Illegal constructor');
      var d = toDict(init, 'ChapterInformationInit');
      var startTime = 0;
      if (d.startTime !== undefined) {
        startTime = Number(d.startTime);
        if (!isFinite(startTime)) {
          throw new TypeError("The 'startTime' member is not a finite floating-point value.");
        }
      }
      _chapterState.set(this, {
        title: d.title === undefined ? '' : String(d.title),
        startTime: startTime,
        artwork: d.artwork === undefined ? Object.freeze([]) : convertArtwork(d.artwork)
      });
      Object.freeze(this);
    }
    get title() { return chapterSlot(this).title; }
    get startTime() { return chapterSlot(this).startTime; }
    get artwork() { return chapterSlot(this).artwork; }
  }
  markEnumerable(ChapterInformation.prototype, ['title', 'startTime', 'artwork']);
  Object.defineProperty(ChapterInformation.prototype, Symbol.toStringTag, {
    value: 'ChapterInformation', configurable: true
  });

  // ── MediaMetadata (§6.1) ───────────────────────────────────────────────────
  // IDL: constructor(optional MediaMetadataInit init = {}); title/artist/album
  // are DOMString attributes, artwork a FrozenArray<MediaImage> attribute,
  // chapterInfo a readonly FrozenArray<ChapterInformation>. State lives in a
  // WeakMap so unknown init members never leak onto the instance (BUG-636).
  var _metaState = new WeakMap();
  function metaSlot(self) {
    var st = _metaState.get(self);
    if (!st) throw new TypeError('Illegal invocation');
    return st;
  }
  class MediaMetadata {
    constructor(init = undefined) {
      var d = toDict(init, "Failed to construct 'MediaMetadata': parameter 1");
      // Convert everything first: a throw must not produce a half-built object.
      var artwork = d.artwork === undefined ? Object.freeze([]) : convertArtwork(d.artwork);
      var chapters = [];
      if (d.chapterInfo !== undefined) {
        var seq = toSequence(d.chapterInfo, "The 'chapterInfo' member");
        for (var i = 0; i < seq.length; i++) {
          chapters.push(new ChapterInformation(CHAPTER_KEY, seq[i]));
        }
      }
      _metaState.set(this, {
        title:  d.title  === undefined ? '' : String(d.title),
        artist: d.artist === undefined ? '' : String(d.artist),
        album:  d.album  === undefined ? '' : String(d.album),
        artwork: artwork,
        chapterInfo: Object.freeze(chapters)
      });
    }
    get title() { return metaSlot(this).title; }
    set title(v) { metaSlot(this).title = String(v); _updateSeq++; }
    get artist() { return metaSlot(this).artist; }
    set artist(v) { metaSlot(this).artist = String(v); _updateSeq++; }
    get album() { return metaSlot(this).album; }
    set album(v) { metaSlot(this).album = String(v); _updateSeq++; }
    get artwork() { return metaSlot(this).artwork; }
    // Conversion runs before the store, so an invalid entry keeps the old value.
    set artwork(v) { var st = metaSlot(this); st.artwork = convertArtwork(v); _updateSeq++; }
    get chapterInfo() { return metaSlot(this).chapterInfo; }
  }
  markEnumerable(MediaMetadata.prototype, ['title', 'artist', 'album', 'artwork', 'chapterInfo']);
  Object.defineProperty(MediaMetadata.prototype, Symbol.toStringTag, {
    value: 'MediaMetadata', configurable: true
  });

  // ── Allowed playback states (W3C Media Session §5.1) ──────────────────────
  var VALID_PLAYBACK_STATES = { 'none': true, 'paused': true, 'playing': true };

  // ── Valid action types (W3C Media Session §5.3) ───────────────────────────
  var VALID_ACTIONS = {
    'play': true, 'pause': true, 'stop': true,
    'seekbackward': true, 'seekforward': true, 'seekto': true,
    'previoustrack': true, 'nexttrack': true, 'skipad': true,
    'togglemicrophone': true, 'togglecamera': true, 'togglescreenshare': true,
    'hangup': true, 'previousslide': true, 'nextslide': true,
    'enterpictureinpicture': true, 'voiceactivity': true,
    // Not in the current spec enum; kept for pages written against the
    // earlier draft.
    'togglecaptionstrack': true
  };

  // ── MediaSession singleton ─────────────────────────────────────────────────
  var _metadata       = null;
  var _playbackState  = 'none';
  var _actionHandlers = {};
  var _positionState  = null;
  var _cameraActive   = false;
  var _micActive      = false;
  // Incremented whenever state changes so shell can detect stale reads.
  var _updateSeq = 0;

  var mediaSession = {
    // W3C §5.1: metadata getter/setter.
    get metadata() { return _metadata; },
    // IDL `MediaMetadata?`: null/undefined clear it, anything else that is
    // not a MediaMetadata is a TypeError.
    set metadata(v) {
      if (v === null || v === undefined) {
        _metadata = null;
      } else if (_metaState.has(v)) {
        _metadata = v;
      } else {
        throw new TypeError("Failed to set the 'metadata' property on 'MediaSession': The provided value is not of type 'MediaMetadata'.");
      }
      _updateSeq++;
    },

    // W3C §5.1: playbackState getter/setter.
    get playbackState() { return _playbackState; },
    set playbackState(v) {
      if (VALID_PLAYBACK_STATES[v]) {
        _playbackState = v;
        _updateSeq++;
      }
    },

    // W3C §5.3: register/unregister an action handler.
    // `action` is a MediaSessionAction enum: an unknown value fails argument
    // conversion with TypeError (BUG-636), as does a non-callable handler.
    setActionHandler: function(action, callback) {
      if (arguments.length < 2) {
        throw new TypeError("Failed to execute 'setActionHandler' on 'MediaSession': 2 arguments required, but only " +
          arguments.length + ' present.');
      }
      action = String(action);
      if (!Object.prototype.hasOwnProperty.call(VALID_ACTIONS, action)) {
        throw new TypeError("Failed to execute 'setActionHandler' on 'MediaSession': The provided value '" +
          action + "' is not a valid enum value of type MediaSessionAction.");
      }
      if (callback === null || callback === undefined) {
        delete _actionHandlers[action];
      } else if (typeof callback === 'function') {
        _actionHandlers[action] = callback;
      } else {
        throw new TypeError("Failed to execute 'setActionHandler' on 'MediaSession': The callback provided as parameter 2 is not a function.");
      }
    },

    // W3C §5.4: update position state. IDL: setPositionState(optional
    // MediaPositionState state = {}), `duration` unrestricted double,
    // `playbackRate`/`position` restricted doubles. An empty dictionary
    // clears the state; otherwise a missing or negative duration, a negative
    // position, position > duration, or playbackRate == 0 throws TypeError
    // (BUG-636 — the shim used to substitute defaults silently).
    setPositionState: function(state) {
      var d = toDict(state, "Failed to execute 'setPositionState' on 'MediaSession': parameter 1");
      var err = "Failed to execute 'setPositionState' on 'MediaSession': ";
      function restricted(name) {
        var n = Number(d[name]);
        if (!isFinite(n)) throw new TypeError(err + 'The provided ' + name + ' is not a finite floating-point value.');
        return n;
      }
      var hasDuration = d.duration !== undefined;
      var hasRate = d.playbackRate !== undefined;
      var hasPosition = d.position !== undefined;
      var duration = hasDuration ? Number(d.duration) : undefined;
      var playbackRate = hasRate ? restricted('playbackRate') : 1;
      var position = hasPosition ? restricted('position') : 0;
      if (!hasDuration && !hasRate && !hasPosition) {
        _positionState = null;
        _updateSeq++;
        return;
      }
      if (!hasDuration) throw new TypeError(err + 'The duration must be provided.');
      if (isNaN(duration) || duration < 0) throw new TypeError(err + 'The provided duration cannot be less than zero.');
      if (position < 0) throw new TypeError(err + 'The provided position cannot be less than zero.');
      if (position > duration) throw new TypeError(err + 'The provided position cannot be greater than the duration.');
      if (playbackRate === 0) throw new TypeError(err + 'The provided playbackRate cannot be equal to zero.');
      _positionState = { duration: duration, playbackRate: playbackRate, position: position };
      _updateSeq++;
    },

    // W3C Media Session L2 §5.4: camera/microphone active state.
    setCameraActive: function(active) {
      _cameraActive = Boolean(active);
      _updateSeq++;
    },
    setMicrophoneActive: function(active) {
      _micActive = Boolean(active);
      _updateSeq++;
    }
  };

  // ── Shell integration helper ───────────────────────────────────────────────
  // Returns a JSON-serialisable snapshot of the current session state, or null
  // if nothing changed since the last call (same _updateSeq).
  // Shell (P3) polls this in about_to_wait to forward metadata to OS.
  var _lastSeqSeen = -1;
  globalThis._lumen_take_media_session_update = function() {
    if (_updateSeq === _lastSeqSeen) return null;
    _lastSeqSeen = _updateSeq;
    return {
      metadata: _metadata ? {
        title:   _metadata.title,
        artist:  _metadata.artist,
        album:   _metadata.album,
        artwork: _metadata.artwork,
        chapterInfo: _metadata.chapterInfo.map(function(c) {
          return { title: c.title, startTime: c.startTime, artwork: c.artwork };
        })
      } : null,
      playbackState: _playbackState,
      positionState: _positionState,
      cameraActive:  _cameraActive,
      micActive:     _micActive
    };
  };

  // Deliver a media session action from the OS (e.g. OS media keys).
  // Shell calls _lumen_fire_media_action('play') etc. to trigger handlers.
  globalThis._lumen_fire_media_action = function(action, details) {
    var handler = _actionHandlers[action];
    if (typeof handler === 'function') {
      try { handler(details || {}); } catch (_) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(_); }
    }
  };

  // ── Install on navigator ───────────────────────────────────────────────────
  try {
    Object.defineProperty(navigator, 'mediaSession', {
      value: mediaSession, writable: false, configurable: true, enumerable: true
    });
  } catch(_) {
    navigator.mediaSession = mediaSession;
  }

  // ── Global exports ─────────────────────────────────────────────────────────
  // WebIDL interface objects: writable, configurable, not enumerable.
  [['MediaMetadata', MediaMetadata], ['ChapterInformation', ChapterInformation]].forEach(function(e) {
    try {
      Object.defineProperty(window, e[0], { value: e[1], writable: true, configurable: true, enumerable: false });
    } catch(_) {}
  });
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_media_session(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            var window = globalThis;
            var navigator = {};
            globalThis.navigator = navigator;
            "#,
        )
        .unwrap();
        super::install_media_session_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn media_session_installed() {
        with_media_session(|rt| {
            let ok = rt
                .eval(
                    "typeof navigator.mediaSession === 'object' && navigator.mediaSession !== null",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn media_metadata_class_exists() {
        with_media_session(|rt| {
            let ok = rt
                .eval("typeof window.MediaMetadata === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn playback_state_default_none() {
        with_media_session(|rt| {
            let state = rt.eval("navigator.mediaSession.playbackState").unwrap();
            assert_eq!(state, JsValue::String("none".to_string()));
        });
    }

    #[test]
    fn playback_state_setter() {
        with_media_session(|rt| {
            rt.eval("navigator.mediaSession.playbackState = 'playing';")
                .unwrap();
            let state = rt.eval("navigator.mediaSession.playbackState").unwrap();
            assert_eq!(state, JsValue::String("playing".to_string()));
        });
    }

    #[test]
    fn invalid_playback_state_ignored() {
        with_media_session(|rt| {
            rt.eval("navigator.mediaSession.playbackState = 'invalid_value';")
                .unwrap();
            let state = rt.eval("navigator.mediaSession.playbackState").unwrap();
            assert_eq!(state, JsValue::String("none".to_string()));
        });
    }

    #[test]
    fn metadata_null_initially() {
        with_media_session(|rt| {
            let null_meta = rt.eval("navigator.mediaSession.metadata === null").unwrap();
            assert_eq!(null_meta, JsValue::Bool(true));
        });
    }

    #[test]
    fn media_metadata_creation() {
        with_media_session(|rt| {
            let title = rt
                .eval(
                    r#"
                  var m = new window.MediaMetadata({
                    title: 'Test Song',
                    artist: 'Test Artist',
                    album: 'Test Album'
                  });
                  m.title
                "#,
                )
                .unwrap();
            assert_eq!(title, JsValue::String("Test Song".to_string()));
        });
    }

    #[test]
    fn metadata_setter() {
        with_media_session(|rt| {
            rt.eval(
                r#"navigator.mediaSession.metadata = new window.MediaMetadata({
                    title: 'Hello',
                    artist: 'World'
                });"#,
            )
            .unwrap();
            let artist = rt.eval("navigator.mediaSession.metadata.artist").unwrap();
            assert_eq!(artist, JsValue::String("World".to_string()));
        });
    }

    #[test]
    fn set_action_handler_stores_callback() {
        with_media_session(|rt| {
            rt.eval("navigator.mediaSession.setActionHandler('play', function() {});")
                .unwrap();
            // No error means it worked; the handler is stored internally.
            let ok = rt.eval("true").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn fire_media_action_calls_handler() {
        with_media_session(|rt| {
            rt.eval(
                r#"
                globalThis._played = false;
                navigator.mediaSession.setActionHandler('play', function() {
                  globalThis._played = true;
                });
                "#,
            )
            .unwrap();
            rt.eval("globalThis._lumen_fire_media_action('play');")
                .unwrap();
            let played = rt.eval("globalThis._played").unwrap();
            assert_eq!(played, JsValue::Bool(true));
        });
    }

    #[test]
    fn set_action_handler_null_removes_callback() {
        with_media_session(|rt| {
            rt.eval(
                r#"
                globalThis._pausedCount = 0;
                navigator.mediaSession.setActionHandler('pause', function() {
                  globalThis._pausedCount++;
                });
                navigator.mediaSession.setActionHandler('pause', null);
                globalThis._lumen_fire_media_action('pause');
                "#,
            )
            .unwrap();
            let count = rt.eval("globalThis._pausedCount").unwrap();
            assert_eq!(count, JsValue::Number(0.0));
        });
    }

    #[test]
    fn set_position_state_stores_values() {
        with_media_session(|rt| {
            rt.eval(
                r#"navigator.mediaSession.setPositionState({
                    duration: 300,
                    playbackRate: 1.5,
                    position: 42
                });"#,
            )
            .unwrap();
            // No error means success; internal _positionState updated.
            let ok = rt.eval("true").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn take_media_session_update_returns_snapshot() {
        with_media_session(|rt| {
            rt.eval(
                r#"navigator.mediaSession.playbackState = 'playing';
                navigator.mediaSession.metadata = new window.MediaMetadata({ title: 'X' });"#,
            )
            .unwrap();
            let has_update = rt
                .eval("globalThis._lumen_take_media_session_update() !== null")
                .unwrap();
            assert_eq!(has_update, JsValue::Bool(true));
        });
    }

    #[test]
    fn take_media_session_update_null_when_no_change() {
        with_media_session(|rt| {
            // Prime: consume first update.
            rt.eval("globalThis._lumen_take_media_session_update();")
                .unwrap();
            // Second call with no change should return null.
            let null_update = rt
                .eval("globalThis._lumen_take_media_session_update() === null")
                .unwrap();
            assert_eq!(null_update, JsValue::Bool(true));
        });
    }

    /// Runs `checks` (a JS array expression of booleans) after defining a
    /// `throwsType(f)` helper and asserts every entry is `true`.
    fn assert_all_true(rt: &V8JsRuntime, checks: &str) {
        let src = format!(
            "function throwsType(f) {{ try {{ f(); return false; }} \
               catch (e) {{ return e instanceof TypeError; }} }} \
             ({checks}).map(function(b, i) {{ return b ? 'ok' : 'FAIL#' + i; }}).join(',')"
        );
        let res = rt.eval(&src).unwrap();
        assert!(
            matches!(&res, JsValue::String(s) if !s.contains("FAIL")),
            "{res:?}"
        );
    }

    /// BUG-636: mirrors `mediasession/mediametadata.html` — dictionary
    /// conversion, `chapterInfo`, re-shaped and frozen `artwork`.
    #[test]
    fn media_metadata_validates_and_freezes() {
        with_media_session(|rt| {
            assert_all_true(
                rt,
                "[ throwsType(function() { new MediaMetadata('foobar'); }), \
                   throwsType(function() { new MediaMetadata(42); }), \
                   throwsType(function() { MediaMetadata({}); }), \
                   new MediaMetadata().chapterInfo.length === 0, \
                   new MediaMetadata({}).artwork.length === 0, \
                   new MediaMetadata({ junk: 1 }).junk === undefined, \
                   (function() { \
                     var m = new MediaMetadata({ artwork: [{ src: 'http://foo.com/', extra: 1 }] }); \
                     var a = m.artwork; \
                     var pushThrows = throwsType(function() { a.push({ src: 'x' }); }); \
                     a[0].src = 'bar'; a[0].other = 2; \
                     return pushThrows && Object.isFrozen(a) && Object.isFrozen(a[0]) && \
                       !('extra' in a[0]) && !('other' in a[0]) && \
                       a[0].sizes === '' && a[0].type === '' && m.artwork === a; \
                   })(), \
                   throwsType(function() { new MediaMetadata({ artwork: [{}] }); }), \
                   (function() { \
                     var m = new MediaMetadata(); \
                     var t = throwsType(function() { m.artwork = [{ type: 'image/png' }]; }); \
                     return t && m.artwork.length === 0; \
                   })(), \
                   (function() { \
                     var m = new MediaMetadata({ chapterInfo: [ \
                       { title: 'C1', startTime: 0, artwork: [{ src: 'http://c/1', sizes: '1x1' }] }, \
                       { title: 'C2', startTime: 16 } ] }); \
                     var c = m.chapterInfo; \
                     m.chapterInfo = []; \
                     return m.chapterInfo === c && c.length === 2 && Object.isFrozen(c) && \
                       Object.isFrozen(c[0]) && c[0].title === 'C1' && c[1].startTime === 16 && \
                       c[0].artwork[0].sizes === '1x1' && c[1].artwork.length === 0; \
                   })(), \
                   throwsType(function() { \
                     new MediaMetadata({ chapterInfo: [{ artwork: [{ type: 'x' }] }] }); }), \
                   throwsType(function() { navigator.mediaSession.metadata = {}; }) \
                 ]",
            );
        });
    }

    /// BUG-636: mirrors `mediasession/positionstate.html` and
    /// `setactionhandler.html` — invalid input throws `TypeError`.
    #[test]
    fn position_state_and_action_handler_validate() {
        with_media_session(|rt| {
            assert_all_true(
                rt,
                "[ !throwsType(function() { navigator.mediaSession.setPositionState(null); }), \
                   !throwsType(function() { navigator.mediaSession.setPositionState(); }), \
                   !throwsType(function() { navigator.mediaSession.setPositionState({ duration: 0 }); }), \
                   !throwsType(function() { navigator.mediaSession.setPositionState( \
                     { duration: 60.9, position: 10.1, playbackRate: -2 }); }), \
                   !throwsType(function() { navigator.mediaSession.setPositionState( \
                     { duration: Infinity, position: 5 }); }), \
                   throwsType(function() { navigator.mediaSession.setPositionState({ duration: -1 }); }), \
                   throwsType(function() { navigator.mediaSession.setPositionState( \
                     { duration: 10, position: -1 }); }), \
                   throwsType(function() { navigator.mediaSession.setPositionState( \
                     { duration: 10, position: 20 }); }), \
                   throwsType(function() { navigator.mediaSession.setPositionState( \
                     { duration: 60.9, position: 10.1, playbackRate: 0 }); }), \
                   throwsType(function() { navigator.mediaSession.setPositionState( \
                     { position: 10.1, playbackRate: 1 }); }), \
                   throwsType(function() { navigator.mediaSession.setActionHandler('invalid', null); }), \
                   !throwsType(function() { navigator.mediaSession.setActionHandler('voiceactivity', null); }), \
                   !throwsType(function() { navigator.mediaSession.setActionHandler('nextslide', null); }), \
                   throwsType(function() { navigator.mediaSession.setActionHandler('play', 5); }) \
                 ]",
            );
        });
    }

    /// BUG-636: `MediaImage.src` is parsed against the document base URL and
    /// an unparsable URL throws. Needs the full page runtime for `URL` and
    /// `document.baseURI`.
    #[test]
    fn media_image_src_resolved_against_base() {
        let rt = V8JsRuntime::new().unwrap();
        let doc = std::sync::Arc::new(std::sync::Mutex::new(lumen_dom::Document::new()));
        rt.install_dom(
            doc,
            "http://example.org/a/b.html",
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
        assert_all_true(
            &rt,
            "[ typeof URL === 'function', \
               new MediaMetadata({ artwork: [{ src: '../foo' }] }).artwork[0].src === \
                 'http://example.org/foo', \
               new MediaMetadata({ artwork: [{ src: '/x/y' }] }).artwork[0].src === \
                 'http://example.org/x/y', \
               new MediaMetadata({ artwork: [{ src: 'http://example.com' }] }).artwork[0].src === \
                 'http://example.com/', \
               throwsType(function() { \
                 new MediaMetadata({ artwork: [{ src: 'http://example.com:demo' }] }); }), \
               throwsType(function() { \
                 new MediaMetadata({ artwork: [{ src: 'http://[example.com]' }] }); }) \
             ]",
        );
    }

    #[test]
    fn set_camera_active() {
        with_media_session(|rt| {
            rt.eval("navigator.mediaSession.setCameraActive(true);")
                .unwrap();
            let has_update = rt
                .eval("globalThis._lumen_take_media_session_update() !== null")
                .unwrap();
            assert_eq!(has_update, JsValue::Bool(true));
        });
    }
}
