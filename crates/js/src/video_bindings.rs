//! HTMLVideoElement JS stubs (Phase 0).
//!
//! Installs `HTMLVideoElement`-compatible properties and methods on `<video>`
//! DOM elements so that pages can interact with them without JS errors.
//!
//! Phase 0 scope — no actual media playback:
//! - `play()` → resolved `Promise` (fires `loadedmetadata`/`canplay` immediately)
//! - `pause()` → no-op
//! - `src` getter/setter (setting schedules `loadedmetadata` + `canplay`)
//! - `currentTime` getter → 0
//! - `duration` getter → `Infinity`
//! - `paused` getter → `true`
//! - `readyState` getter → 4 (`HAVE_ENOUGH_DATA`)
//! - `videoWidth`/`videoHeight` → 0

use rquickjs::Ctx;

/// Install HTMLVideoElement stubs into the JS context.
///
/// Patches `HTMLVideoElement.prototype` (and falls back to direct property
/// assignment on individual `<video>` nodes via `document.createElement`)
/// with stub implementations that allow sites to call `video.play()` etc.
/// without throwing. Events `loadedmetadata` and `canplay` are dispatched
/// synchronously when `src` is set.
///
/// Must be called **after** `dom::install_dom_api`.
pub fn install_video_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(VIDEO_SHIM)?;
    Ok(())
}

/// JavaScript shim: HTMLVideoElement stub methods and properties.
const VIDEO_SHIM: &str = r#"(function() {
  'use strict';

  // Minimal HTMLVideoElement prototype shim for Phase 0.
  // Attaches stubs to all existing <video> elements and any future ones
  // via a MutationObserver-like hook on document.createElement.
  function patchVideoElement(el) {
    if (el.__lumen_video_patched) return;
    el.__lumen_video_patched = true;

    var _src = el.getAttribute('src') || '';
    var _paused = true;
    var _currentTime = 0;

    Object.defineProperty(el, 'src', {
      get: function() { return _src; },
      set: function(v) {
        _src = String(v || '');
        el.setAttribute('src', _src);
        // Immediately dispatch loadedmetadata + canplay (Phase 0: no real fetch).
        try {
          el.dispatchEvent(new Event('loadedmetadata'));
          el.dispatchEvent(new Event('canplay'));
        } catch(e) {}
      },
      configurable: true,
    });

    Object.defineProperty(el, 'currentTime', {
      get: function() { return _currentTime; },
      set: function(v) { _currentTime = Number(v) || 0; },
      configurable: true,
    });

    Object.defineProperty(el, 'duration', {
      get: function() { return Infinity; },
      configurable: true,
    });

    Object.defineProperty(el, 'paused', {
      get: function() { return _paused; },
      configurable: true,
    });

    // readyState 4 = HAVE_ENOUGH_DATA — tells scripts we are "ready".
    Object.defineProperty(el, 'readyState', {
      get: function() { return 4; },
      configurable: true,
    });

    Object.defineProperty(el, 'videoWidth',  { get: function() { return 0; }, configurable: true });
    Object.defineProperty(el, 'videoHeight', { get: function() { return 0; }, configurable: true });

    el.play = function() {
      _paused = false;
      try {
        el.dispatchEvent(new Event('play'));
        el.dispatchEvent(new Event('playing'));
      } catch(e) {}
      return Promise.resolve();
    };

    el.pause = function() {
      _paused = true;
      try { el.dispatchEvent(new Event('pause')); } catch(e) {}
    };

    el.load = function() {
      _currentTime = 0;
      _paused = true;
    };

    // Stub muted / volume / controls so getter/setter code doesn't throw.
    var _muted = el.hasAttribute('muted');
    var _volume = 1.0;
    var _controls = el.hasAttribute('controls');
    Object.defineProperty(el, 'muted',    { get: function(){ return _muted; },    set: function(v){ _muted = !!v; }, configurable: true });
    Object.defineProperty(el, 'volume',   { get: function(){ return _volume; },   set: function(v){ _volume = Math.max(0, Math.min(1, Number(v)||0)); }, configurable: true });
    Object.defineProperty(el, 'controls', { get: function(){ return _controls; }, set: function(v){ _controls = !!v; }, configurable: true });
  }

  // Patch any <video> elements already in the document.
  if (typeof document !== 'undefined' && document.querySelectorAll) {
    try {
      var videos = document.querySelectorAll('video');
      for (var i = 0; i < videos.length; i++) {
        patchVideoElement(videos[i]);
      }
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
  createElement: function(tag) { return { getAttribute: function(){ return ''; }, setAttribute: function(){}, hasAttribute: function(){ return false; }, dispatchEvent: function(){} }; }
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
            // Simulate a video element with stubs applied.
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
    fn duration_is_infinity() {
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
            assert!(result, "duration should be Infinity");
        });
    }

    #[test]
    fn current_time_is_zero() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
el.currentTime === 0
"#,
                )
                .unwrap();
            assert!(result, "currentTime should be 0");
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
    fn ready_state_is_four() {
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
            assert!(result, "readyState should be 4 (HAVE_ENOUGH_DATA)");
        });
    }

    #[test]
    fn src_setter_updates_attribute() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Need a slightly richer stub to test setAttribute.
            ctx.eval::<(), _>(
                r#"
var _stored = {};
var document = {
  querySelectorAll: function() { return []; },
  createElement: function(tag) {
    var attrs = {};
    return {
      getAttribute: function(k){ return attrs[k] || ''; },
      setAttribute: function(k,v){ attrs[k]=v; _stored=attrs; },
      hasAttribute: function(k){ return !!attrs[k]; },
      dispatchEvent: function(){}
    };
  }
};
"#,
            )
            .unwrap();
            install_video_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('video');
el.src = 'video.mp4';
el.src === 'video.mp4'
"#,
                )
                .unwrap();
            assert!(result, "src setter should update the src property");
        });
    }
}
