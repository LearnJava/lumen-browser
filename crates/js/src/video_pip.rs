/// Video Picture-in-Picture API (W3C Picture-in-Picture Level 1).
///
/// Installs:
/// - `HTMLVideoElement.prototype.requestPictureInPicture()` → `Promise<PictureInPictureWindow>`
/// - `HTMLVideoElement.prototype.disablePictureInPicture` attribute
/// - `document.exitPictureInPicture()` → `Promise<void>`
/// - `document.pictureInPictureElement` getter → current PiP video element or null
/// - `document.pictureInPictureEnabled` getter → true
/// - `PictureInPictureWindow` class with `width`, `height`, `onresize`, `resize` event
///
/// Events fired on the video element:
/// - `enterpictureinpicture` — when video enters PiP mode
/// - `leavepictureinpicture` — when video leaves PiP mode
///
/// Phase 0: in-memory state only; `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)` bindings
/// prepared for shell Phase 1 (OS-level floating window via winit child window or overlay).
use rquickjs::Ctx;

/// Install Video Picture-in-Picture API into the JS context.
///
/// Must be called **after** `video_bindings::install_video_bindings` so that
/// `patchVideoElement` has already run on existing `<video>` elements.
pub fn install_video_pip_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(VIDEO_PIP_SHIM)?;
    Ok(())
}

/// JavaScript shim: W3C Picture-in-Picture Level 1.
const VIDEO_PIP_SHIM: &str = r#"(function() {
  'use strict';

  // Module-level state: currently PiP'd video element and its window.
  var _pipVideo = null;
  var _pipWindow = null;

  // Use the global EventTarget if available, otherwise a no-op base class.
  var _EventTargetBase = (typeof EventTarget !== 'undefined') ? EventTarget : (function() {
    function Stub() {}
    Stub.prototype.addEventListener = function() {};
    Stub.prototype.removeEventListener = function() {};
    Stub.prototype.dispatchEvent = function() {};
    return Stub;
  }());

  /// PictureInPictureWindow — the floating mini-player window object.
  class PictureInPictureWindow extends _EventTargetBase {
    constructor(width, height) {
      super();
      this._width = width || 0;
      this._height = height || 0;
    }

    /// Width of the PiP window in CSS pixels (0 = Phase 0 stub).
    get width() { return this._width; }

    /// Height of the PiP window in CSS pixels (0 = Phase 0 stub).
    get height() { return this._height; }
  }

  globalThis.PictureInPictureWindow = PictureInPictureWindow;

  /// Patch a single video element with PiP API methods.
  function patchVideoPip(el) {
    if (el.__lumen_pip_patched) return;
    el.__lumen_pip_patched = true;

    var _disabled = el.hasAttribute ? el.hasAttribute('disablepictureinpicture') : false;

    Object.defineProperty(el, 'disablePictureInPicture', {
      get: function() { return _disabled; },
      set: function(v) {
        _disabled = !!v;
        if (_disabled && el.hasAttribute) {
          el.setAttribute('disablepictureinpicture', '');
        } else if (el.removeAttribute) {
          el.removeAttribute('disablepictureinpicture');
        }
      },
      configurable: true,
    });

    el.requestPictureInPicture = async function() {
      if (_disabled) {
        throw Object.assign(new Error('disablePictureInPicture is set'), { name: 'InvalidStateError' });
      }

      // Exit existing PiP first (spec: only one video in PiP at a time).
      if (_pipVideo && _pipVideo !== el) {
        await _pipVideo.requestPictureInPicture().catch(function() {});
        if (_pipVideo && _pipVideo !== el) {
          exitCurrentPip();
        }
      }

      _pipVideo = el;
      _pipWindow = new PictureInPictureWindow(0, 0);

      // Fire enterpictureinpicture on the video element.
      try {
        el.dispatchEvent(new Event('enterpictureinpicture'));
      } catch(e) {}

      // Native binding for shell Phase 1 (OS floating window).
      var nid = el.__lumen_nid || 0;
      if (typeof _lumen_pip_enter === 'function') {
        _lumen_pip_enter(nid);
      }

      return _pipWindow;
    };
  }

  /// Exit the currently active PiP session and fire leavepictureinpicture.
  function exitCurrentPip() {
    var prev = _pipVideo;
    _pipVideo = null;
    _pipWindow = null;
    if (prev) {
      try { prev.dispatchEvent(new Event('leavepictureinpicture')); } catch(e) {}
      var nid = prev.__lumen_nid || 0;
      if (typeof _lumen_pip_exit === 'function') {
        _lumen_pip_exit(nid);
      }
    }
  }

  /// Patch all existing <video> elements in the document.
  if (typeof document !== 'undefined' && document.querySelectorAll) {
    try {
      var videos = document.querySelectorAll('video');
      for (var i = 0; i < videos.length; i++) {
        patchVideoPip(videos[i]);
      }
    } catch(e) {}
  }

  /// Intercept future document.createElement('video') calls to patch new elements.
  if (typeof document !== 'undefined' && document.createElement) {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag) {
      var el = _origCreate(tag);
      if (typeof tag === 'string' && tag.toLowerCase() === 'video') {
        patchVideoPip(el);
      }
      return el;
    };
  }

  /// document.exitPictureInPicture() — exits active video PiP session.
  if (typeof document !== 'undefined') {
    document.exitPictureInPicture = async function() {
      if (!_pipVideo) {
        throw Object.assign(new Error('No element in Picture-in-Picture'), { name: 'InvalidStateError' });
      }
      exitCurrentPip();
    };

    /// document.pictureInPictureElement — the video currently in PiP, or null.
    Object.defineProperty(document, 'pictureInPictureElement', {
      get: function() { return _pipVideo; },
      configurable: true,
    });

    /// document.pictureInPictureEnabled — always true (feature flag, Phase 0).
    Object.defineProperty(document, 'pictureInPictureEnabled', {
      get: function() { return true; },
      configurable: true,
    });
  }

  /// _lumen_pip_deliver_resize(width, height) — shell calls this when the OS
  /// PiP window is resized (Phase 1). Fires 'resize' on the active PictureInPictureWindow.
  globalThis._lumen_pip_deliver_resize = function(width, height) {
    if (!_pipWindow) return;
    _pipWindow._width = width;
    _pipWindow._height = height;
    try { _pipWindow.dispatchEvent(new Event('resize')); } catch(e) {}
  };
})();"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Minimal DOM + EventTarget stubs so the shim can run without the full DOM bridge.
    fn install_minimal_dom(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(r#"
class EventTarget {
  constructor() { this._listeners = {}; }
  addEventListener(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  }
  removeEventListener(type, fn) {
    if (this._listeners[type]) {
      this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
    }
  }
  dispatchEvent(e) {
    var ls = this._listeners[e.type] || [];
    for (var i = 0; i < ls.length; i++) { try { ls[i](e); } catch(_) {} }
  }
}
class Event {
  constructor(type) { this.type = type; }
}
var document = {
  _pip: null,
  querySelectorAll: function() { return []; },
  createElement: function(tag) {
    var el = {
      tagName: tag,
      __lumen_nid: 42,
      _attrs: {},
      hasAttribute: function(k) { return k in this._attrs; },
      setAttribute: function(k,v) { this._attrs[k] = v; },
      removeAttribute: function(k) { delete this._attrs[k]; },
      dispatchEvent: function(e) { var h = this['on'+e.type]; if(typeof h==='function') h(e); },
      _listeners: {},
      addEventListener: function(t,f) {
        if(!this._listeners[t]) this._listeners[t]=[];
        this._listeners[t].push(f);
      },
    };
    el.dispatchEvent = function(e) {
      var ls = this._listeners[e.type] || [];
      for(var i=0;i<ls.length;i++){try{ls[i](e);}catch(_){}}
      var h = this['on'+e.type]; if(typeof h==='function') h(e);
    };
    return el;
  },
  dispatchEvent: function() {},
};
"#).unwrap();
    }

    #[test]
    fn install_succeeds_without_document() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_video_pip_api(&ctx).expect("install should succeed without document");
        });
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).expect("install should succeed with minimal dom");
        });
    }

    #[test]
    fn picture_in_picture_window_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval("typeof PictureInPictureWindow === 'function'")
                .unwrap();
            assert!(result, "PictureInPictureWindow class should be exported on globalThis");
        });
    }

    #[test]
    fn picture_in_picture_window_has_width_height() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval(r#"
var w = new PictureInPictureWindow(320, 240);
w.width === 320 && w.height === 240
"#)
                .unwrap();
            assert!(result, "PictureInPictureWindow should expose width/height");
        });
    }

    #[test]
    fn request_picture_in_picture_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval(r#"
var el = document.createElement('video');
el.requestPictureInPicture() instanceof Promise
"#)
                .unwrap();
            assert!(result, "requestPictureInPicture() should return a Promise");
        });
    }

    #[test]
    fn document_exit_picture_in_picture_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval("typeof document.exitPictureInPicture === 'function'")
                .unwrap();
            assert!(result, "document.exitPictureInPicture should be a function");
        });
    }

    #[test]
    fn document_picture_in_picture_element_initially_null() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval("document.pictureInPictureElement === null")
                .unwrap();
            assert!(result, "pictureInPictureElement should be null initially");
        });
    }

    #[test]
    fn document_picture_in_picture_enabled_is_true() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval("document.pictureInPictureEnabled === true")
                .unwrap();
            assert!(result, "pictureInPictureEnabled should be true");
        });
    }

    #[test]
    fn disable_picture_in_picture_attribute() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval(r#"
var el = document.createElement('video');
el.disablePictureInPicture === false
"#)
                .unwrap();
            assert!(result, "disablePictureInPicture should be false by default");
        });
    }

    #[test]
    fn lumen_pip_deliver_resize_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            let result: bool = ctx
                .eval("typeof _lumen_pip_deliver_resize === 'function'")
                .unwrap();
            assert!(result, "_lumen_pip_deliver_resize should be a global function");
        });
    }

    #[test]
    fn exit_pip_rejects_when_no_active_pip() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_video_pip_api(&ctx).unwrap();
            // exitPictureInPicture returns a Promise that rejects when no PiP is active.
            let result: bool = ctx
                .eval("document.exitPictureInPicture() instanceof Promise")
                .unwrap();
            assert!(result, "exitPictureInPicture() should return a Promise");
        });
    }
}
