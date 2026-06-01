//! HTMLAudioElement JS stubs (Phase 0).
//!
//! Installs `HTMLAudioElement`-compatible properties and methods on `<audio>`
//! DOM elements so that pages can interact with them without JS errors.
//!
//! Phase 0 scope — no actual media playback:
//! - `play()` → resolved `Promise` (fires `canplay`/`loadedmetadata` immediately)
//! - `pause()` → no-op (dispatches `pause` event)
//! - `src` getter/setter (setting schedules `loadedmetadata` + `canplay`)
//! - `currentTime` getter/setter (getter → 0)
//! - `duration` getter → `Infinity`
//! - `paused` getter → `true`
//! - `volume` getter/setter → 1.0 (clamped 0–1)
//! - `muted` getter/setter → `false` / reflects `muted` attribute
//! - `readyState` getter → 4 (`HAVE_ENOUGH_DATA`)
//! - `controls` getter/setter → reflects `controls` attribute

use rquickjs::Ctx;

/// Install HTMLAudioElement stubs into the JS context.
///
/// Patches existing `<audio>` elements and intercepts `document.createElement('audio')`
/// so that pages can call `audio.play()` etc. without throwing. Events
/// `loadedmetadata` and `canplay` are dispatched synchronously when `src` is set
/// or `play()` is called.
///
/// Must be called **after** `dom::install_dom_api`.
pub fn install_audio_element_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(AUDIO_ELEMENT_SHIM)?;
    Ok(())
}

/// JavaScript shim: HTMLAudioElement stub methods and properties.
const AUDIO_ELEMENT_SHIM: &str = r#"(function() {
  'use strict';

  function patchAudioElement(el) {
    if (el.__lumen_audio_patched) return;
    el.__lumen_audio_patched = true;

    var _src = (el.getAttribute && el.getAttribute('src')) || '';
    var _paused = true;
    var _currentTime = 0;
    var _muted = !!(el.hasAttribute && el.hasAttribute('muted'));
    var _volume = 1.0;
    var _controls = !!(el.hasAttribute && el.hasAttribute('controls'));

    function fireEvent(name) {
      try { el.dispatchEvent(new Event(name)); } catch(e) {}
    }

    Object.defineProperty(el, 'src', {
      get: function() { return _src; },
      set: function(v) {
        _src = String(v || '');
        if (el.setAttribute) el.setAttribute('src', _src);
        // HTML spec §4.8.10.5: setting src triggers load algorithm;
        // Phase 0: fire loadedmetadata + canplay immediately (no actual fetch).
        fireEvent('loadedmetadata');
        fireEvent('canplay');
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

    // readyState 4 = HAVE_ENOUGH_DATA (HTML spec §4.8.10.9).
    Object.defineProperty(el, 'readyState', {
      get: function() { return 4; },
      configurable: true,
    });

    Object.defineProperty(el, 'muted', {
      get: function() { return _muted; },
      set: function(v) { _muted = !!v; },
      configurable: true,
    });

    Object.defineProperty(el, 'volume', {
      get: function() { return _volume; },
      set: function(v) {
        var n = Number(v);
        if (isNaN(n) || n < 0 || n > 1) throw new RangeError('volume must be between 0 and 1');
        _volume = n;
      },
      configurable: true,
    });

    Object.defineProperty(el, 'controls', {
      get: function() { return _controls; },
      set: function(v) { _controls = !!v; },
      configurable: true,
    });

    el.play = function() {
      _paused = false;
      // Phase 0: fire canplay immediately, then play/playing.
      fireEvent('canplay');
      fireEvent('play');
      fireEvent('playing');
      return Promise.resolve();
    };

    el.pause = function() {
      _paused = true;
      fireEvent('pause');
    };

    el.load = function() {
      _currentTime = 0;
      _paused = true;
    };

    // canPlayType stub: always returns '' (cannot play anything in Phase 0).
    el.canPlayType = function() { return ''; };

    // fastSeek is a no-op in Phase 0.
    el.fastSeek = function(time) { _currentTime = Number(time) || 0; };

    // networkState/error stubs for feature-detection code.
    Object.defineProperty(el, 'networkState', { get: function() { return 0; }, configurable: true });
    Object.defineProperty(el, 'error',        { get: function() { return null; }, configurable: true });
    Object.defineProperty(el, 'buffered',     { get: function() { return { length: 0, start: function(){ return 0; }, end: function(){ return 0; } }; }, configurable: true });
    Object.defineProperty(el, 'seekable',     { get: function() { return { length: 0, start: function(){ return 0; }, end: function(){ return 0; } }; }, configurable: true });
    Object.defineProperty(el, 'ended',        { get: function() { return false; }, configurable: true });
    Object.defineProperty(el, 'autoplay',     { get: function() { return !!(el.hasAttribute && el.hasAttribute('autoplay')); }, configurable: true });
    Object.defineProperty(el, 'loop',         {
      get: function() { return !!(el.hasAttribute && el.hasAttribute('loop')); },
      set: function(v) { if (el.setAttribute) { if (v) el.setAttribute('loop',''); else if (el.removeAttribute) el.removeAttribute('loop'); } },
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
    Object.defineProperty(el, 'defaultPlaybackRate', {
      get: function() { return 1.0; }, set: function() {},
      configurable: true,
    });
    Object.defineProperty(el, 'playbackRate', {
      get: function() { return 1.0; }, set: function() {},
      configurable: true,
    });
    Object.defineProperty(el, 'preservesPitch', {
      get: function() { return true; }, set: function() {},
      configurable: true,
    });
  }

  // Patch any <audio> elements already in the document.
  if (typeof document !== 'undefined' && document.querySelectorAll) {
    try {
      var audios = document.querySelectorAll('audio');
      for (var i = 0; i < audios.length; i++) {
        patchAudioElement(audios[i]);
      }
    } catch(e) {}
  }

  // Intercept future document.createElement('audio') calls.
  if (typeof document !== 'undefined' && document.createElement) {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag) {
      var el = _origCreate(tag);
      if (typeof tag === 'string' && tag.toLowerCase() === 'audio') {
        patchAudioElement(el);
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
  createElement: function(tag) {
    var attrs = {};
    return {
      getAttribute: function(k){ return attrs[k] || null; },
      setAttribute: function(k,v){ attrs[k]=v; },
      hasAttribute: function(k){ return k in attrs; },
      removeAttribute: function(k){ delete attrs[k]; },
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
            install_audio_element_bindings(&ctx).expect("install should succeed without document");
        });
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).expect("install should succeed with minimal dom");
        });
    }

    #[test]
    fn play_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
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
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval("var el = document.createElement('audio'); el.duration === Infinity")
                .unwrap();
            assert!(result, "duration should be Infinity");
        });
    }

    #[test]
    fn current_time_is_zero() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval("var el = document.createElement('audio'); el.currentTime === 0")
                .unwrap();
            assert!(result, "currentTime should be 0");
        });
    }

    #[test]
    fn paused_initially_true() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval("var el = document.createElement('audio'); el.paused === true")
                .unwrap();
            assert!(result, "paused should initially be true");
        });
    }

    #[test]
    fn ready_state_is_four() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval("var el = document.createElement('audio'); el.readyState === 4")
                .unwrap();
            assert!(result, "readyState should be 4 (HAVE_ENOUGH_DATA)");
        });
    }

    #[test]
    fn play_makes_paused_false() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
el.play();
el.paused === false
"#,
                )
                .unwrap();
            assert!(result, "play() should set paused to false");
        });
    }

    #[test]
    fn pause_makes_paused_true() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
el.play();
el.pause();
el.paused === true
"#,
                )
                .unwrap();
            assert!(result, "pause() should set paused to true");
        });
    }

    #[test]
    fn volume_getter_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
el.volume = 0.5;
el.volume === 0.5
"#,
                )
                .unwrap();
            assert!(result, "volume setter should work");
        });
    }

    #[test]
    fn muted_getter_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
el.muted = true;
el.muted === true
"#,
                )
                .unwrap();
            assert!(result, "muted setter should work");
        });
    }

    #[test]
    fn src_setter_fires_loadedmetadata_canplay() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Need Event in scope for dispatchEvent calls.
            ctx.eval::<(), _>(
                r#"
function Event(type) { this.type = type; }
var _events = [];
var document = {
  querySelectorAll: function() { return []; },
  createElement: function(tag) {
    var attrs = {};
    return {
      getAttribute: function(k){ return attrs[k] || null; },
      setAttribute: function(k,v){ attrs[k]=v; },
      hasAttribute: function(k){ return k in attrs; },
      removeAttribute: function(k){ delete attrs[k]; },
      dispatchEvent: function(e){ _events.push(e.type); }
    };
  }
};
"#,
            )
            .unwrap();
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
_events = [];
var el = document.createElement('audio');
el.src = 'song.mp3';
_events.indexOf('loadedmetadata') >= 0 && _events.indexOf('canplay') >= 0
"#,
                )
                .unwrap();
            assert!(result, "src setter should fire loadedmetadata and canplay");
        });
    }

    #[test]
    fn can_play_type_returns_empty_string() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
el.canPlayType('audio/mpeg') === ''
"#,
                )
                .unwrap();
            assert!(result, "canPlayType should return empty string in Phase 0");
        });
    }

    #[test]
    fn network_state_is_zero() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval("var el = document.createElement('audio'); el.networkState === 0")
                .unwrap();
            assert!(result, "networkState should be 0 (NETWORK_EMPTY)");
        });
    }

    #[test]
    fn error_is_null() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval("var el = document.createElement('audio'); el.error === null")
                .unwrap();
            assert!(result, "error should be null");
        });
    }

    #[test]
    fn current_time_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
el.currentTime = 42;
el.currentTime === 42
"#,
                )
                .unwrap();
            assert!(result, "currentTime setter should work");
        });
    }

    #[test]
    fn load_resets_current_time() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
el.currentTime = 10;
el.load();
el.currentTime === 0
"#,
                )
                .unwrap();
            assert!(result, "load() should reset currentTime to 0");
        });
    }

    #[test]
    fn volume_range_error() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_audio_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('audio');
var threw = false;
try { el.volume = 2.0; } catch(e) { threw = e instanceof RangeError; }
threw
"#,
                )
                .unwrap();
            assert!(result, "volume > 1 should throw RangeError");
        });
    }
}
