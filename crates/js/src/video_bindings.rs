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

use rquickjs::{Ctx, Function, Object};

use crate::video_gif_store::get_video_gif_store;

/// Install HTMLVideoElement Phase 1 bindings into the JS context.
///
/// Registers the `__lumen_video_*` native functions and the JS shim that
/// patches `<video>` elements.  Must be called **after** `dom::install_dom_api`.
pub fn install_video_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    install_native_bindings(ctx)?;
    ctx.eval::<(), _>(VIDEO_SHIM)?;
    Ok(())
}

// ── Native binding registration ───────────────────────────────────────────────

fn install_native_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    let g: Object = ctx.globals();

    // __lumen_video_load(nid, src) — queue GIF decode request for the shell.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_load",
            Function::new(ctx.clone(), move |nid: f64, src: String| {
                if let Some(s) = &store {
                    s.pending_loads.lock().unwrap().push((nid as u32, src));
                }
            }),
        )?;
    }

    // __lumen_video_ready(nid) → bool — true once the shell stored the entry.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_ready",
            Function::new(ctx.clone(), move |nid: f64| -> bool {
                store
                    .as_ref()
                    .map(|s| s.playback.lock().unwrap().contains_key(&(nid as u32)))
                    .unwrap_or(false)
            }),
        )?;
    }

    // __lumen_video_play(nid, now_ms) — start/resume playback.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_play",
            Function::new(ctx.clone(), move |nid: f64, now_ms: f64| {
                if let Some(s) = &store
                    && let Some(e) = s.playback.lock().unwrap().get_mut(&(nid as u32))
                    && e.paused
                {
                    e.play_epoch_ms = Some(now_ms as u64);
                    e.paused = false;
                }
            }),
        )?;
    }

    // __lumen_video_pause(nid, now_ms) — pause, snapshot position.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_pause",
            Function::new(ctx.clone(), move |nid: f64, now_ms: f64| {
                if let Some(s) = &store
                    && let Some(e) = s.playback.lock().unwrap().get_mut(&(nid as u32))
                {
                    e.freeze(now_ms as u64);
                    e.paused = true;
                }
            }),
        )?;
    }

    // __lumen_video_seek(nid, secs, now_ms) — seek to a position.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_seek",
            Function::new(ctx.clone(), move |nid: f64, secs: f64, now_ms: f64| {
                if let Some(s) = &store
                    && let Some(e) = s.playback.lock().unwrap().get_mut(&(nid as u32))
                {
                    let target_ms = (secs * 1000.0).max(0.0) as u64;
                    e.position_ms = target_ms;
                    if !e.paused {
                        e.play_epoch_ms = Some(now_ms as u64);
                    }
                }
            }),
        )?;
    }

    // __lumen_video_current_time(nid, now_ms) → f64 — position in seconds.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_current_time",
            Function::new(ctx.clone(), move |nid: f64, now_ms: f64| -> f64 {
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
            }),
        )?;
    }

    // __lumen_video_duration(nid) → f64 — total duration in seconds.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_duration",
            Function::new(ctx.clone(), move |nid: f64| -> f64 {
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
            }),
        )?;
    }

    // __lumen_video_paused(nid) → bool.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_paused",
            Function::new(ctx.clone(), move |nid: f64| -> bool {
                store
                    .as_ref()
                    .and_then(|s| {
                        s.playback
                            .lock()
                            .unwrap()
                            .get(&(nid as u32))
                            .map(|e| e.paused)
                    })
                    .unwrap_or(true)
            }),
        )?;
    }

    // __lumen_video_ended(nid, now_ms) → bool.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_ended",
            Function::new(ctx.clone(), move |nid: f64, now_ms: f64| -> bool {
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
            }),
        )?;
    }

    // __lumen_video_width(nid) → f64 — GIF pixel width.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_width",
            Function::new(ctx.clone(), move |nid: f64| -> f64 {
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
            }),
        )?;
    }

    // __lumen_video_height(nid) → f64 — GIF pixel height.
    {
        let store = get_video_gif_store();
        g.set(
            "__lumen_video_height",
            Function::new(ctx.clone(), move |nid: f64| -> f64 {
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
            }),
        )?;
    }

    // __lumen_video_can_play_type(mime) → String — "maybe" for image/gif, "" otherwise.
    {
        g.set(
            "__lumen_video_can_play_type",
            Function::new(ctx.clone(), |mime: String| -> String {
                let m = mime.trim().to_ascii_lowercase();
                let base = m.split(';').next().unwrap_or("").trim();
                if base == "image/gif" {
                    "maybe".to_string()
                } else {
                    String::new()
                }
            }),
        )?;
    }

    Ok(())
}

// ── JavaScript shim ───────────────────────────────────────────────────────────

/// HTMLVideoElement Phase 1 shim.
///
/// Uses `__lumen_video_*` native bindings for GIF-backed playback.  Falls
/// back to Phase 0 behaviour when the store is absent (headless/CI) or when
/// the `src` is not a `.gif` URL.
const VIDEO_SHIM: &str = r#"(function() {
  'use strict';

  var HAS_STORE = (typeof __lumen_video_load === 'function');
  var POLL_MS   = 50;    // readyState poll when waiting for GIF decode
  var TUPDATE_MS = 250;  // timeupdate interval during playback

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};
    use std::sync::Mutex;

    /// Serializes tests that install and read the process-global
    /// [`crate::video_gif_store`] singleton.  Without this guard, parallel
    /// tests race: one test's `set_video_gif_store` overwrites the global
    /// between another test's own `set` and the `install`/`load` that captures
    /// it, so the load lands in the wrong store (BUG-166).
    static STORE_GUARD: Mutex<()> = Mutex::new(());

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Minimal DOM stubs so the shim can run without the full DOM bridge.
    fn install_minimal_dom(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
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

    #[test]
    fn install_succeeds_without_document() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_video_bindings(&ctx).expect("install should succeed without document");
        });
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).expect("install should succeed with minimal dom");
        });
    }

    #[test]
    fn play_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
var p = el.play();
p instanceof Promise
"#,
                )
                .unwrap();
            assert!(result, "play() should return a Promise");
        });
    }

    #[test]
    fn duration_infinity_without_gif() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
el.duration === Infinity
"#,
                )
                .unwrap();
            assert!(result, "duration should be Infinity when no GIF loaded");
        });
    }

    #[test]
    fn paused_initially_true() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
el.paused === true
"#,
                )
                .unwrap();
            assert!(result, "paused should initially be true");
        });
    }

    #[test]
    fn ready_state_with_no_src() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
el.readyState === 4
"#,
                )
                .unwrap();
            assert!(result, "readyState should be 4 with no src");
        });
    }

    #[test]
    fn can_play_type_gif() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
el.canPlayType('image/gif') === 'maybe'
"#,
                )
                .unwrap();
            assert!(result, "canPlayType('image/gif') should return 'maybe'");
        });
    }

    #[test]
    fn can_play_type_mp4_empty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
el.canPlayType('video/mp4') === ''
"#,
                )
                .unwrap();
            assert!(result, "canPlayType('video/mp4') should return ''");
        });
    }

    #[test]
    fn native_video_load_registers_pending() {
        use crate::video_gif_store::set_video_gif_store;
        use std::sync::Arc;
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(crate::video_gif_store::VideoGifStore::default());
        set_video_gif_store(store.clone());

        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_video_bindings(&ctx).unwrap();
            ctx.eval::<(), _>("__lumen_video_load(99, 'test.gif');")
                .unwrap();
        });

        let loads = store.pending_loads.lock().unwrap();
        assert!(!loads.is_empty(), "load should be queued");
        assert!(loads.iter().any(|(n, s)| *n == 99 && s == "test.gif"));
    }

    #[test]
    fn native_video_ready_false_before_decode() {
        use crate::video_gif_store::set_video_gif_store;
        use std::sync::Arc;
        let _guard = STORE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let store = Arc::new(crate::video_gif_store::VideoGifStore::default());
        set_video_gif_store(store.clone());

        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_video_bindings(&ctx).unwrap();
            let ready: bool = ctx.eval("__lumen_video_ready(55)").unwrap();
            assert!(!ready, "should not be ready before decode");
        });
    }
}
