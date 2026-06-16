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

use rquickjs::{Ctx, Function, Object};

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
pub fn set_audio_playback_provider(p: Arc<dyn AudioPlaybackProvider>) {
    *provider_lock().write().unwrap() = Some(p);
}

fn get_provider() -> Option<Arc<dyn AudioPlaybackProvider>> {
    provider_lock().read().unwrap().clone()
}

// ── Public install function ───────────────────────────────────────────────────

/// Install `HTMLAudioElement` Phase 1 bindings into the JS context.
///
/// Registers the `__lumen_audio_*` native functions and the JS shim that patches
/// `<audio>` elements with real play/pause/seek/timeupdate support.
///
/// Must be called **after** `dom::install_dom_api`.
pub fn install_audio_element_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    install_native_bindings(ctx)?;
    ctx.eval::<(), _>(AUDIO_ELEMENT_SHIM)?;
    Ok(())
}

// ── Native binding registration ───────────────────────────────────────────────

fn install_native_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    let g: Object = ctx.globals();

    // __lumen_audio_alloc() → f64
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_alloc",
            Function::new(ctx.clone(), move || -> f64 {
                p.as_ref().map_or(0.0, |p| p.alloc_handle() as f64)
            }),
        )?;
    }

    // __lumen_audio_free(handle)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_free",
            Function::new(ctx.clone(), move |handle: f64| {
                if let Some(p) = &p {
                    p.free_handle(handle as u64);
                }
            }),
        )?;
    }

    // __lumen_audio_load(handle, url)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_load",
            Function::new(ctx.clone(), move |handle: f64, url: String| {
                if let Some(p) = &p {
                    p.load(handle as u64, &url);
                }
            }),
        )?;
    }

    // __lumen_audio_play(handle)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_play",
            Function::new(ctx.clone(), move |handle: f64| {
                if let Some(p) = &p {
                    p.play(handle as u64);
                }
            }),
        )?;
    }

    // __lumen_audio_pause(handle)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_pause",
            Function::new(ctx.clone(), move |handle: f64| {
                if let Some(p) = &p {
                    p.pause(handle as u64);
                }
            }),
        )?;
    }

    // __lumen_audio_stop(handle)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_stop",
            Function::new(ctx.clone(), move |handle: f64| {
                if let Some(p) = &p {
                    p.stop(handle as u64);
                }
            }),
        )?;
    }

    // __lumen_audio_seek(handle, secs)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_seek",
            Function::new(ctx.clone(), move |handle: f64, secs: f64| {
                if let Some(p) = &p {
                    p.seek(handle as u64, secs);
                }
            }),
        )?;
    }

    // __lumen_audio_set_volume(handle, vol)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_set_volume",
            Function::new(ctx.clone(), move |handle: f64, vol: f64| {
                if let Some(p) = &p {
                    p.set_volume(handle as u64, vol);
                }
            }),
        )?;
    }

    // __lumen_audio_set_rate(handle, rate)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_set_rate",
            Function::new(ctx.clone(), move |handle: f64, rate: f64| {
                if let Some(p) = &p {
                    p.set_playback_rate(handle as u64, rate);
                }
            }),
        )?;
    }

    // __lumen_audio_current_time(handle) → f64
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_current_time",
            Function::new(ctx.clone(), move |handle: f64| -> f64 {
                p.as_ref().map_or(0.0, |p| p.current_time(handle as u64))
            }),
        )?;
    }

    // __lumen_audio_duration(handle) → f64  (NaN = unknown)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_duration",
            Function::new(ctx.clone(), move |handle: f64| -> f64 {
                p.as_ref().map_or(f64::NAN, |p| p.duration(handle as u64))
            }),
        )?;
    }

    // __lumen_audio_paused(handle) → bool
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_paused",
            Function::new(ctx.clone(), move |handle: f64| -> bool {
                p.as_ref().is_none_or(|p| p.is_paused(handle as u64))
            }),
        )?;
    }

    // __lumen_audio_ended(handle) → bool
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_ended",
            Function::new(ctx.clone(), move |handle: f64| -> bool {
                p.as_ref().is_some_and(|p| p.is_ended(handle as u64))
            }),
        )?;
    }

    // __lumen_audio_ready_state(handle) → f64 (0–4)
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_ready_state",
            Function::new(ctx.clone(), move |handle: f64| -> f64 {
                p.as_ref()
                    .map_or(0.0, |p| p.ready_state(handle as u64) as f64)
            }),
        )?;
    }

    // __lumen_audio_has_error(handle) → bool
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_has_error",
            Function::new(ctx.clone(), move |handle: f64| -> bool {
                p.as_ref().is_some_and(|p| p.has_error(handle as u64))
            }),
        )?;
    }

    // __lumen_audio_can_play_type(mime) → String
    {
        let p = get_provider();
        g.set(
            "__lumen_audio_can_play_type",
            Function::new(ctx.clone(), move |mime: String| -> String {
                p.as_ref()
                    .map_or("", |p| p.can_play_type(&mime))
                    .to_owned()
            }),
        )?;
    }

    Ok(())
}

// ── JavaScript shim ───────────────────────────────────────────────────────────

/// HTMLAudioElement Phase 1 shim.
///
/// Uses `__lumen_audio_*` native bindings for real playback; falls back
/// gracefully when the provider is absent (headless/CI mode — same as Phase 0).
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rquickjs::{Context, Runtime};

    use lumen_core::ext::NullAudioPlaybackProvider;

    use super::*;

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Minimal DOM stub sufficient for the shim to run without errors.
    fn install_dom_stub(ctx: &Ctx) {
        ctx.eval::<(), _>(
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

    fn install_all(ctx: &Ctx) {
        set_audio_playback_provider(Arc::new(NullAudioPlaybackProvider));
        install_native_bindings(ctx).unwrap();
        install_dom_stub(ctx);
        ctx.eval::<(), _>(AUDIO_ELEMENT_SHIM).unwrap();
    }

    #[test]
    fn install_bindings_succeeds() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| install_all(&ctx));
    }

    #[test]
    fn alloc_returns_nonzero_handle() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            set_audio_playback_provider(Arc::new(NullAudioPlaybackProvider));
            install_native_bindings(&ctx).unwrap();
            let h: f64 = ctx.eval("__lumen_audio_alloc()").unwrap();
            assert!(h > 0.0);
        });
    }

    #[test]
    fn play_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval("var el = document.createElement('audio'); el.play() instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn paused_initially_true() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval("var el = document.createElement('audio'); el.paused === true")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn volume_range_error() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"var el = document.createElement('audio');
var threw = false;
try { el.volume = 2; } catch(e) { threw = true; }
threw"#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn muted_getter_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval(
                    "var el = document.createElement('audio'); el.muted = true; el.muted === true",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn can_play_type_mp3_probably() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let r: String = ctx
                .eval("document.createElement('audio').canPlayType('audio/mpeg')")
                .unwrap();
            assert_eq!(r, "probably");
        });
    }

    #[test]
    fn can_play_type_unknown_empty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let r: String = ctx
                .eval("document.createElement('audio').canPlayType('video/x-custom')")
                .unwrap();
            assert_eq!(r, "");
        });
    }

    #[test]
    fn duration_infinity_no_provider() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // No provider — shim falls back gracefully.
            install_dom_stub(&ctx);
            ctx.eval::<(), _>(AUDIO_ELEMENT_SHIM).unwrap();
            let ok: bool = ctx
                .eval(
                    "var el = document.createElement('audio'); el.duration === Infinity",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn ready_state_zero_null_provider() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let rs: f64 = ctx
                .eval("document.createElement('audio').readyState")
                .unwrap();
            // NullAudioPlaybackProvider always returns 0.
            assert_eq!(rs, 0.0);
        });
    }

    #[test]
    fn playback_rate_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval(
                    "var el = document.createElement('audio'); el.playbackRate = 2.0; el.playbackRate === 2.0",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn loop_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval(
                    "var el = document.createElement('audio'); el.loop = true; el.loop === true",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn seek_fires_seeking_seeked() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"_events = [];
var el = document.createElement('audio');
el.currentTime = 5;
_events.indexOf('seeking') >= 0 && _events.indexOf('seeked') >= 0"#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_global_constructor() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx.eval("typeof Audio === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn new_audio_with_src() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval("var a = new Audio('test.mp3'); a.src === 'test.mp3'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn buffered_empty_before_load() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval("document.createElement('audio').buffered.length === 0")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn error_null_when_no_error() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_all(&ctx);
            let ok: bool = ctx
                .eval("document.createElement('audio').error === null")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_provider_function_exists() {
        // Just check the function compiles and runs without panic.
        set_audio_playback_provider(Arc::new(NullAudioPlaybackProvider));
    }
}
