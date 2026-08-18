//! HTMLAudioElement JS bindings — Phase 1 (real audio playback via `AudioPlaybackProvider`).
//!
//! Upgrades the Phase 0 stub to drive actual audio playback through the platform
//! backend installed by the shell (`lumen_shell::platform::audio_player::PlatformAudioPlayer`).
//!
//! # Architecture
//!
//! Each `<audio>` DOM element gets a unique `handle` (u64) allocated from the
//! `AudioPlaybackProvider`.  All audio operations reference this handle.  The handle
//! is freed when the element's JS wrapper is finalized (GC'd).
//!
//! The JS shim calls `__lumen_audio_load(handle, url_string)` which triggers
//! background loading in the shell.  JS polls `__lumen_audio_ready_state(handle)`
//! every 50 ms until the audio is decoded (readyState ≥ 4 = HAVE_ENOUGH_DATA),
//! then fires the loadedmetadata / loadeddata / canplay / canplaythrough sequence.
//!
//! # Registered native bindings
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `__lumen_audio_alloc` | `() → f64` | Allocate a new handle |
//! | `__lumen_audio_free` | `(handle: f64)` | Release a handle |
//! | `__lumen_audio_load` | `(handle: f64, url: String)` | Start background load |
//! | `__lumen_audio_play` | `(handle: f64)` | Start/resume playback |
//! | `__lumen_audio_pause` | `(handle: f64)` | Pause |
//! | `__lumen_audio_stop` | `(handle: f64)` | Stop + reset |
//! | `__lumen_audio_seek` | `(handle: f64, secs: f64)` | Seek |
//! | `__lumen_audio_set_volume` | `(handle: f64, vol: f64)` | Set volume |
//! | `__lumen_audio_set_rate` | `(handle: f64, rate: f64)` | Set playback rate |
//! | `__lumen_audio_current_time` | `(handle: f64) → f64` | Current position (s) |
//! | `__lumen_audio_duration` | `(handle: f64) → f64` | Duration (NaN if unknown) |
//! | `__lumen_audio_paused` | `(handle: f64) → bool` | Is paused? |
//! | `__lumen_audio_ended` | `(handle: f64) → bool` | Has ended? |
//! | `__lumen_audio_ready_state` | `(handle: f64) → f64` | W3C readyState (0–4) |
//! | `__lumen_audio_has_error` | `(handle: f64) → bool` | Had load/decode error? |
//! | `__lumen_audio_can_play_type` | `(mime: String) → String` | canPlayType probe |

use std::sync::{Arc, OnceLock, RwLock};

use lumen_core::ext::AudioPlaybackProvider;

// ── Provider registry ─────────────────────────────────────────────────────────

static PROVIDER: OnceLock<RwLock<Option<Arc<dyn AudioPlaybackProvider>>>> = OnceLock::new();

fn provider_lock() -> &'static RwLock<Option<Arc<dyn AudioPlaybackProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Install the platform audio playback backend.
///
/// Must be called once by the shell before any JS context is created.
/// Thread-safe; subsequent calls replace the previous provider.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn set_audio_playback_provider(p: Arc<dyn AudioPlaybackProvider>) {
    *provider_lock().write().unwrap() = Some(p);
}

#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn get_provider() -> Option<Arc<dyn AudioPlaybackProvider>> {
    provider_lock().read().unwrap().clone()
}

/// Install `HTMLAudioElement` Phase 1 bindings into a V8 runtime (Ph3 V8
/// migration S5-S7 batch 3; the rquickjs twin was removed in S12b-B21):
/// state is the process-global [`AudioPlaybackProvider`] (installed once via
/// `set_audio_playback_provider`, backend-agnostic), so no new
/// `V8JsRuntime` plumbing is needed — each native captures its own
/// `get_provider()` clone. The JS shim is unchanged.
///
/// Must be called **after** `v8_runtime.rs::install_dom`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_audio_element_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2};
    use lumen_core::ext::JsRuntime as _;

    {
        let p = get_provider();
        let alloc = into_v8_fn0(move || -> f64 { p.as_ref().map_or(0.0, |p| p.alloc_handle() as f64) });
        rt.register_native("__lumen_audio_alloc", alloc)?;
    }

    {
        let p = get_provider();
        let free = into_v8_fn1(move |handle: f64| {
            if let Some(p) = &p {
                p.free_handle(handle as u64);
            }
        });
        rt.register_native("__lumen_audio_free", free)?;
    }

    {
        let p = get_provider();
        let load = into_v8_fn2(move |handle: f64, url: String| {
            if let Some(p) = &p {
                p.load(handle as u64, &url);
            }
        });
        rt.register_native("__lumen_audio_load", load)?;
    }

    {
        let p = get_provider();
        let play = into_v8_fn1(move |handle: f64| {
            if let Some(p) = &p {
                p.play(handle as u64);
            }
        });
        rt.register_native("__lumen_audio_play", play)?;
    }

    {
        let p = get_provider();
        let pause = into_v8_fn1(move |handle: f64| {
            if let Some(p) = &p {
                p.pause(handle as u64);
            }
        });
        rt.register_native("__lumen_audio_pause", pause)?;
    }

    {
        let p = get_provider();
        let stop = into_v8_fn1(move |handle: f64| {
            if let Some(p) = &p {
                p.stop(handle as u64);
            }
        });
        rt.register_native("__lumen_audio_stop", stop)?;
    }

    {
        let p = get_provider();
        let seek = into_v8_fn2(move |handle: f64, secs: f64| {
            if let Some(p) = &p {
                p.seek(handle as u64, secs);
            }
        });
        rt.register_native("__lumen_audio_seek", seek)?;
    }

    {
        let p = get_provider();
        let set_volume = into_v8_fn2(move |handle: f64, vol: f64| {
            if let Some(p) = &p {
                p.set_volume(handle as u64, vol);
            }
        });
        rt.register_native("__lumen_audio_set_volume", set_volume)?;
    }

    {
        let p = get_provider();
        let set_rate = into_v8_fn2(move |handle: f64, rate: f64| {
            if let Some(p) = &p {
                p.set_playback_rate(handle as u64, rate);
            }
        });
        rt.register_native("__lumen_audio_set_rate", set_rate)?;
    }

    {
        let p = get_provider();
        let current_time = into_v8_fn1(move |handle: f64| -> f64 {
            p.as_ref().map_or(0.0, |p| p.current_time(handle as u64))
        });
        rt.register_native("__lumen_audio_current_time", current_time)?;
    }

    {
        let p = get_provider();
        let duration = into_v8_fn1(move |handle: f64| -> f64 {
            p.as_ref().map_or(f64::NAN, |p| p.duration(handle as u64))
        });
        rt.register_native("__lumen_audio_duration", duration)?;
    }

    {
        let p = get_provider();
        let paused = into_v8_fn1(move |handle: f64| -> bool {
            p.as_ref().is_none_or(|p| p.is_paused(handle as u64))
        });
        rt.register_native("__lumen_audio_paused", paused)?;
    }

    {
        let p = get_provider();
        let ended = into_v8_fn1(move |handle: f64| -> bool {
            p.as_ref().is_some_and(|p| p.is_ended(handle as u64))
        });
        rt.register_native("__lumen_audio_ended", ended)?;
    }

    {
        let p = get_provider();
        let ready_state = into_v8_fn1(move |handle: f64| -> f64 {
            p.as_ref().map_or(0.0, |p| p.ready_state(handle as u64) as f64)
        });
        rt.register_native("__lumen_audio_ready_state", ready_state)?;
    }

    {
        let p = get_provider();
        let has_error = into_v8_fn1(move |handle: f64| -> bool {
            p.as_ref().is_some_and(|p| p.has_error(handle as u64))
        });
        rt.register_native("__lumen_audio_has_error", has_error)?;
    }

    {
        let p = get_provider();
        let can_play_type = into_v8_fn1(move |mime: String| -> String {
            p.as_ref().map_or("", |p| p.can_play_type(&mime)).to_owned()
        });
        rt.register_native("__lumen_audio_can_play_type", can_play_type)?;
    }

    rt.eval(AUDIO_ELEMENT_SHIM)?;
    Ok(())
}

// ── JavaScript shim ───────────────────────────────────────────────────────────

/// HTMLAudioElement Phase 1 shim.
///
/// Uses `__lumen_audio_*` native bindings for real playback; falls back
/// gracefully when the provider is absent (headless/CI mode — same as Phase 0).
#[cfg(feature = "v8-backend")]
const AUDIO_ELEMENT_SHIM: &str = r#"(function() {
  'use strict';

  var HAS_PROVIDER = (typeof __lumen_audio_alloc === 'function');
  var POLL_MS      = 50;   // readyState poll interval
  var TUPDATE_MS   = 250;  // timeupdate interval

  function fireEvent(el, name) {
    try {
      var ev = new Event(name, { bubbles: false, cancelable: false });
      el.dispatchEvent(ev);
    } catch(e) {}
  }

  function patchAudioElement(el) {
    if (el.__lumen_audio_patched) return;
    el.__lumen_audio_patched = true;

    var _handle  = HAS_PROVIDER ? __lumen_audio_alloc() : 0;
    var _src     = (el.getAttribute && el.getAttribute('src')) || '';
    var _volume  = 1.0;
    var _muted   = !!(el.hasAttribute && el.hasAttribute('muted'));
    var _loop    = !!(el.hasAttribute && el.hasAttribute('loop'));
    var _autoplay= !!(el.hasAttribute && el.hasAttribute('autoplay'));
    var _rate    = 1.0;
    var _loadStarted  = false;
    var _loadTimer    = null;
    var _tupdateTimer = null;

    // ── loading ──────────────────────────────────────────────────────────────

    function startLoad(url) {
      if (!HAS_PROVIDER || !url) return;
      _loadStarted = true;
      __lumen_audio_load(_handle, url);
      fireEvent(el, 'loadstart');
      fireEvent(el, 'progress');

      _loadTimer = setInterval(function() {
        if (__lumen_audio_has_error(_handle)) {
          clearInterval(_loadTimer); _loadTimer = null;
          fireEvent(el, 'error');
          return;
        }
        var rs = __lumen_audio_ready_state(_handle);
        if (rs >= 1) { fireEvent(el, 'durationchange'); fireEvent(el, 'loadedmetadata'); }
        if (rs >= 2) { fireEvent(el, 'loadeddata'); }
        if (rs >= 3) { fireEvent(el, 'canplay'); }
        if (rs >= 4) {
          clearInterval(_loadTimer); _loadTimer = null;
          fireEvent(el, 'canplaythrough');
          if (_autoplay) el.play();
        }
      }, POLL_MS);
    }

    // ── timeupdate loop ──────────────────────────────────────────────────────

    function startTupdate() {
      if (_tupdateTimer !== null) return;
      _tupdateTimer = setInterval(function() {
        if (!HAS_PROVIDER || __lumen_audio_paused(_handle)) {
          clearInterval(_tupdateTimer); _tupdateTimer = null; return;
        }
        fireEvent(el, 'timeupdate');
        if (__lumen_audio_ended(_handle)) {
          clearInterval(_tupdateTimer); _tupdateTimer = null;
          fireEvent(el, 'ended');
          if (_loop) { __lumen_audio_seek(_handle, 0); __lumen_audio_play(_handle); startTupdate(); }
        }
      }, TUPDATE_MS);
    }

    // ── properties ──────────────────────────────────────────────────────────

    Object.defineProperty(el, 'src', {
      get: function() { return _src; },
      set: function(v) {
        var s = String(v || '');
        if (s === _src && _loadStarted) return;
        _src = s; _loadStarted = false;
        if (el.setAttribute) el.setAttribute('src', _src);
        if (_src) startLoad(_src);
      },
      configurable: true,
    });

    Object.defineProperty(el, 'currentTime', {
      get: function() { return HAS_PROVIDER ? __lumen_audio_current_time(_handle) : 0; },
      set: function(v) {
        if (HAS_PROVIDER) __lumen_audio_seek(_handle, Number(v) || 0);
        fireEvent(el, 'seeking'); fireEvent(el, 'seeked');
      },
      configurable: true,
    });

    Object.defineProperty(el, 'duration', {
      get: function() {
        if (!HAS_PROVIDER) return Infinity;
        var d = __lumen_audio_duration(_handle);
        return isNaN(d) ? Infinity : d;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'paused', {
      get: function() { return HAS_PROVIDER ? __lumen_audio_paused(_handle) : true; },
      configurable: true,
    });

    Object.defineProperty(el, 'ended', {
      get: function() { return HAS_PROVIDER ? __lumen_audio_ended(_handle) : false; },
      configurable: true,
    });

    Object.defineProperty(el, 'readyState', {
      get: function() { return HAS_PROVIDER ? __lumen_audio_ready_state(_handle) : 4; },
      configurable: true,
    });

    Object.defineProperty(el, 'networkState', {
      get: function() {
        if (!HAS_PROVIDER) return 0;
        var rs = __lumen_audio_ready_state(_handle);
        return rs === 0 ? 0 : rs >= 4 ? 1 : 2;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'error', {
      get: function() {
        if (HAS_PROVIDER && __lumen_audio_has_error(_handle))
          return { code: 4, message: 'MEDIA_ERR_SRC_NOT_SUPPORTED' };
        return null;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'volume', {
      get: function() { return _volume; },
      set: function(v) {
        var n = Number(v);
        if (isNaN(n) || n < 0 || n > 1) throw new RangeError('volume must be between 0 and 1');
        _volume = n;
        if (HAS_PROVIDER) __lumen_audio_set_volume(_handle, _muted ? 0 : _volume);
        fireEvent(el, 'volumechange');
      },
      configurable: true,
    });

    Object.defineProperty(el, 'muted', {
      get: function() { return _muted; },
      set: function(v) {
        _muted = !!v;
        if (HAS_PROVIDER) __lumen_audio_set_volume(_handle, _muted ? 0 : _volume);
        fireEvent(el, 'volumechange');
      },
      configurable: true,
    });

    Object.defineProperty(el, 'playbackRate', {
      get: function() { return _rate; },
      set: function(v) {
        _rate = Number(v) || 1.0;
        if (HAS_PROVIDER) __lumen_audio_set_rate(_handle, _rate);
        fireEvent(el, 'ratechange');
      },
      configurable: true,
    });

    Object.defineProperty(el, 'defaultPlaybackRate', {
      get: function() { return 1.0; }, set: function() {},
      configurable: true,
    });

    Object.defineProperty(el, 'controls', {
      get: function() { return !!(el.hasAttribute && el.hasAttribute('controls')); },
      set: function(v) {
        if (v) { if (el.setAttribute) el.setAttribute('controls', ''); }
        else   { if (el.removeAttribute) el.removeAttribute('controls'); }
      },
      configurable: true,
    });

    Object.defineProperty(el, 'loop', {
      get: function() { return _loop; },
      set: function(v) { _loop = !!v; },
      configurable: true,
    });

    Object.defineProperty(el, 'autoplay', {
      get: function() { return _autoplay; },
      set: function(v) { _autoplay = !!v; },
      configurable: true,
    });

    Object.defineProperty(el, 'preload', {
      get: function() { return (el.getAttribute && el.getAttribute('preload')) || 'auto'; },
      set: function(v) { if (el.setAttribute) el.setAttribute('preload', String(v)); },
      configurable: true,
    });

    Object.defineProperty(el, 'defaultMuted', {
      get: function() { return !!(el.hasAttribute && el.hasAttribute('muted')); },
      configurable: true,
    });

    Object.defineProperty(el, 'preservesPitch', {
      get: function() { return true; }, set: function() {},
      configurable: true,
    });

    var _emptyRanges = {
      length: 0,
      start: function() { return 0; },
      end:   function() { return 0; },
    };

    Object.defineProperty(el, 'buffered', {
      get: function() {
        if (!HAS_PROVIDER) return _emptyRanges;
        var d = __lumen_audio_duration(_handle);
        if (isNaN(d) || d <= 0) return _emptyRanges;
        return { length: 1, start: function() { return 0; }, end: function() { return d; } };
      },
      configurable: true,
    });

    Object.defineProperty(el, 'seekable', {
      get: function() {
        if (!HAS_PROVIDER) return _emptyRanges;
        var d = __lumen_audio_duration(_handle);
        if (isNaN(d) || d <= 0) return _emptyRanges;
        return { length: 1, start: function() { return 0; }, end: function() { return d; } };
      },
      configurable: true,
    });

    Object.defineProperty(el, 'played', {
      get: function() { return _emptyRanges; },
      configurable: true,
    });

    // ── methods ──────────────────────────────────────────────────────────────

    el.play = function() {
      if (!_loadStarted && _src) startLoad(_src);

      if (!HAS_PROVIDER) {
        fireEvent(el, 'play'); fireEvent(el, 'playing');
        return Promise.resolve();
      }

      var rs = __lumen_audio_ready_state(_handle);
      if (rs < 4) {
        // Wait for load to complete.
        return new Promise(function(resolve, reject) {
          var attempts = 0;
          var t = setInterval(function() {
            if (++attempts > 200) {
              clearInterval(t);
              reject(new DOMException('Playback timed out', 'NotSupportedError'));
              return;
            }
            if (__lumen_audio_has_error(_handle)) {
              clearInterval(t);
              reject(new DOMException('Media load failed', 'NotSupportedError'));
              return;
            }
            if (__lumen_audio_ready_state(_handle) >= 4) {
              clearInterval(t);
              __lumen_audio_play(_handle);
              fireEvent(el, 'play'); fireEvent(el, 'playing');
              startTupdate();
              resolve();
            }
          }, POLL_MS);
        });
      }

      __lumen_audio_play(_handle);
      fireEvent(el, 'play'); fireEvent(el, 'playing');
      startTupdate();
      return Promise.resolve();
    };

    el.pause = function() {
      if (HAS_PROVIDER) __lumen_audio_pause(_handle);
      if (_tupdateTimer !== null) { clearInterval(_tupdateTimer); _tupdateTimer = null; }
      fireEvent(el, 'pause');
    };

    el.load = function() {
      el.pause();
      if (HAS_PROVIDER) __lumen_audio_stop(_handle);
      _loadStarted = false;
      if (_src) startLoad(_src);
    };

    el.canPlayType = function(mime) {
      if (!mime) return '';
      if (HAS_PROVIDER && typeof __lumen_audio_can_play_type === 'function')
        return __lumen_audio_can_play_type(String(mime));
      var t = String(mime).split(';')[0].trim().toLowerCase();
      if (t === 'audio/mpeg' || t === 'audio/mp3' || t === 'audio/ogg' ||
          t === 'audio/wav'  || t === 'audio/wave' || t === 'audio/flac') return 'probably';
      if (t === 'audio/webm' || t === 'audio/aac' || t === 'audio/mp4') return 'maybe';
      return '';
    };

    el.fastSeek = function(t) {
      if (HAS_PROVIDER) __lumen_audio_seek(_handle, Number(t) || 0);
    };

    // Kick off initial load when src attribute is already set.
    if (_src && !_loadStarted) startLoad(_src);
  }

  // ── Patch existing <audio> elements ─────────────────────────────────────────

  if (typeof document !== 'undefined' && document.querySelectorAll) {
    try {
      var els = document.querySelectorAll('audio');
      for (var i = 0; i < els.length; i++) patchAudioElement(els[i]);
    } catch(e) {}
  }

  // ── Intercept document.createElement('audio') ────────────────────────────────

  if (typeof document !== 'undefined' && document.createElement) {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag) {
      var el = _origCreate(tag);
      if (typeof tag === 'string' && tag.toLowerCase() === 'audio') patchAudioElement(el);
      return el;
    };
  }

  // ── new Audio(src?) ───────────────────────────────────────────────────────────

  if (typeof document !== 'undefined') {
    globalThis.Audio = function AudioConstructor(src) {
      var el = document.createElement('audio');
      if (src !== undefined) el.src = String(src);
      return el;
    };
  }
})();
"#;

// ── Tests ─────────────────────────────────────────────────────────────────────

/// V8 test coverage for the `HTMLAudioElement` shim (the rquickjs twin was
/// removed in S12b-B21; this module ports its 16 tests to V8 verbatim).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use std::sync::Arc;

    use lumen_core::ext::{JsRuntime as _, NullAudioPlaybackProvider};
    use lumen_core::JsValue;

    use crate::v8_runtime::V8JsRuntime;

    use super::*;

    /// Minimal DOM stub sufficient for the shim to run without errors.
    fn install_dom_stub(rt: &V8JsRuntime) {
        rt.eval(
            r#"
var _events = [];
function Event(name) { this.type = name; }
function DOMException(msg, name) { this.message = msg; this.name = name; }
// Timer stubs: timers are no-ops in unit tests (no real event loop).
var setInterval  = function() { return 0; };
var clearInterval = function() {};
var setTimeout   = function(fn) { return 0; };
var clearTimeout  = function() {};
var document = {
  querySelectorAll: function() { return []; },
  createElement: function(tag) {
    var attrs = {};
    return {
      getAttribute:    function(k)   { return Object.prototype.hasOwnProperty.call(attrs, k) ? attrs[k] : null; },
      setAttribute:    function(k,v) { attrs[k] = v; },
      hasAttribute:    function(k)   { return Object.prototype.hasOwnProperty.call(attrs, k); },
      removeAttribute: function(k)   { delete attrs[k]; },
      dispatchEvent:   function(e)   { _events.push(e.type); return true; },
    };
  },
};
"#,
        )
        .unwrap();
    }

    /// The DOM stub must be installed *before* the bindings: `install_audio_element_bindings_v8`
    /// registers natives AND evals the shim in one call, and the shim's `document`-interception
    /// (createElement patch, `new Audio()`) only runs when `document` already exists.
    fn with_audio() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        install_dom_stub(&rt);
        set_audio_playback_provider(Arc::new(NullAudioPlaybackProvider));
        super::install_audio_element_bindings_v8(&rt).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn install_bindings_succeeds() {
        let _rt = with_audio();
    }

    #[test]
    fn alloc_returns_nonzero_handle() {
        let rt = with_audio();
        match rt.eval("__lumen_audio_alloc()").unwrap() {
            JsValue::Number(n) => assert!(n > 0.0),
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn play_returns_promise() {
        let rt = with_audio();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('audio'); el.play() instanceof Promise",
        );
        assert!(ok);
    }

    #[test]
    fn paused_initially_true() {
        let rt = with_audio();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('audio'); el.paused === true",
        );
        assert!(ok);
    }

    #[test]
    fn volume_range_error() {
        let rt = with_audio();
        let ok = bool_eval(
            &rt,
            r#"var el = document.createElement('audio');
var threw = false;
try { el.volume = 2; } catch(e) { threw = true; }
threw"#,
        );
        assert!(ok);
    }

    #[test]
    fn muted_getter_setter() {
        let rt = with_audio();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('audio'); el.muted = true; el.muted === true",
        );
        assert!(ok);
    }

    #[test]
    fn can_play_type_mp3_probably() {
        let rt = with_audio();
        let r = rt
            .eval("document.createElement('audio').canPlayType('audio/mpeg')")
            .unwrap();
        assert_eq!(r, JsValue::String("probably".into()));
    }

    #[test]
    fn can_play_type_unknown_empty() {
        let rt = with_audio();
        let r = rt
            .eval("document.createElement('audio').canPlayType('video/x-custom')")
            .unwrap();
        assert_eq!(r, JsValue::String(String::new()));
    }

    #[test]
    fn duration_infinity_no_provider() {
        // No provider — natives never registered, shim falls back gracefully.
        let rt = V8JsRuntime::new().unwrap();
        install_dom_stub(&rt);
        rt.eval(super::AUDIO_ELEMENT_SHIM).unwrap();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('audio'); el.duration === Infinity",
        );
        assert!(ok);
    }

    #[test]
    fn ready_state_zero_null_provider() {
        let rt = with_audio();
        let rs = rt
            .eval("document.createElement('audio').readyState")
            .unwrap();
        // NullAudioPlaybackProvider always returns 0.
        assert_eq!(rs, JsValue::Number(0.0));
    }

    #[test]
    fn playback_rate_setter() {
        let rt = with_audio();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('audio'); el.playbackRate = 2.0; el.playbackRate === 2.0",
        );
        assert!(ok);
    }

    #[test]
    fn loop_setter() {
        let rt = with_audio();
        let ok = bool_eval(
            &rt,
            "var el = document.createElement('audio'); el.loop = true; el.loop === true",
        );
        assert!(ok);
    }

    #[test]
    fn seek_fires_seeking_seeked() {
        let rt = with_audio();
        let ok = bool_eval(
            &rt,
            r#"_events = [];
var el = document.createElement('audio');
el.currentTime = 5;
_events.indexOf('seeking') >= 0 && _events.indexOf('seeked') >= 0"#,
        );
        assert!(ok);
    }

    #[test]
    fn audio_global_constructor() {
        let rt = with_audio();
        let ok = bool_eval(&rt, "typeof Audio === 'function'");
        assert!(ok);
    }

    #[test]
    fn new_audio_with_src() {
        let rt = with_audio();
        let ok = bool_eval(&rt, "var a = new Audio('test.mp3'); a.src === 'test.mp3'");
        assert!(ok);
    }

    #[test]
    fn buffered_empty_before_load() {
        let rt = with_audio();
        let ok = bool_eval(&rt, "document.createElement('audio').buffered.length === 0");
        assert!(ok);
    }

    #[test]
    fn error_null_when_no_error() {
        let rt = with_audio();
        let ok = bool_eval(&rt, "document.createElement('audio').error === null");
        assert!(ok);
    }

    #[test]
    fn set_provider_function_exists() {
        // Just check the function compiles and runs without panic.
        set_audio_playback_provider(Arc::new(NullAudioPlaybackProvider));
    }
}
