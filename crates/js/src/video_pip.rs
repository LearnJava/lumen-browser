//! Video Picture-in-Picture API (W3C Picture-in-Picture Level 1).
//!
//! Installs:
//! - `HTMLVideoElement.prototype.requestPictureInPicture()` → `Promise<PictureInPictureWindow>`
//! - `HTMLVideoElement.prototype.disablePictureInPicture` attribute
//! - `document.exitPictureInPicture()` → `Promise<void>`
//! - `document.pictureInPictureElement` getter → current PiP video element or null
//! - `document.pictureInPictureEnabled` getter → true
//! - `PictureInPictureWindow` class with `width`, `height`, `onresize`, `resize` event
//!
//! Events fired on the video element:
//! - `enterpictureinpicture` — when video enters PiP mode
//! - `leavepictureinpicture` — when video leaves PiP mode
//!
//! Phase 0: in-memory state only; `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)` bindings
//! prepared for shell Phase 1 (OS-level floating window via winit child window or overlay).

/// Install Video Picture-in-Picture API into the JS context.
///
/// Evaluated via [`lumen_core::ext::JsRuntime::eval`]. Must be called **after**
/// `video_bindings`'s install so that `patchVideoElement` has already run on
/// existing `<video>` elements.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_video_pip_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(VIDEO_PIP_SHIM)?;
    Ok(())
}

/// JavaScript shim: W3C Picture-in-Picture Level 1.
#[cfg(feature = "v8-backend")]
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
  ///
  /// P3-pip slice 4: reuse the class `document_pip.rs` already installed (it
  /// runs first and defines the full document/close-capable version) so
  /// `instanceof PictureInPictureWindow` is consistent between video- and
  /// document-PiP. Falls back to this minimal width/height-only definition
  /// only when evaluated without `document_pip.rs` (e.g. this module's own
  /// unit tests, which install `VIDEO_PIP_SHIM` in isolation).
  var PictureInPictureWindow = (typeof globalThis.PictureInPictureWindow === 'function')
    ? globalThis.PictureInPictureWindow
    : (function() {
        class Fallback extends _EventTargetBase {
          constructor(width, height) {
            super();
            this._width = width || 0;
            this._height = height || 0;
          }
          get width() { return this._width; }
          get height() { return this._height; }
        }
        return Fallback;
      }());

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
      // Shared with document_pip.rs: whichever PiP session is open, so
      // `_lumen_pip_deliver_resize` below can update it uniformly.
      globalThis.__lumen_pip_active_window = _pipWindow;

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
    if (globalThis.__lumen_pip_active_window === _pipWindow) {
      globalThis.__lumen_pip_active_window = null;
    }
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
  /// PiP window is resized. Fires 'resize' on whichever `PictureInPictureWindow`
  /// is active — video PiP's `_pipWindow` or Document PiP's `_activeWindow`
  /// (`document_pip.rs`), tracked via the shared `__lumen_pip_active_window`
  /// (P3-pip slice 5; only one PiP session can be open at a time).
  globalThis._lumen_pip_deliver_resize = function(width, height) {
    var w = globalThis.__lumen_pip_active_window;
    if (!w) return;
    w._width = width;
    w._height = height;
    try { w.dispatchEvent(new Event('resize')); } catch(e) {}
  };
})();"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal DOM + EventTarget stubs so the shim can run without the full DOM bridge.
    fn install_minimal_dom(rt: &V8JsRuntime) {
        rt.eval(
            r#"
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
"#,
        )
        .unwrap();
    }

    #[test]
    fn install_succeeds_without_document() {
        let rt = V8JsRuntime::new().unwrap();
        super::install_video_pip_api_v8(&rt).expect("install should succeed without document");
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).expect("install should succeed with minimal dom");
    }

    #[test]
    fn picture_in_picture_window_class_exists() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt
            .eval("typeof PictureInPictureWindow === 'function'")
            .unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "PictureInPictureWindow class should be exported on globalThis"
        );
    }

    #[test]
    fn picture_in_picture_window_has_width_height() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt
            .eval(
                r#"
var w = new PictureInPictureWindow(320, 240);
w.width === 320 && w.height === 240
"#,
            )
            .unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "PictureInPictureWindow should expose width/height"
        );
    }

    #[test]
    fn request_picture_in_picture_returns_promise() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt
            .eval(
                r#"
var el = document.createElement('video');
el.requestPictureInPicture() instanceof Promise
"#,
            )
            .unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "requestPictureInPicture() should return a Promise"
        );
    }

    #[test]
    fn document_exit_picture_in_picture_is_function() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt
            .eval("typeof document.exitPictureInPicture === 'function'")
            .unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "document.exitPictureInPicture should be a function"
        );
    }

    #[test]
    fn document_picture_in_picture_element_initially_null() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt.eval("document.pictureInPictureElement === null").unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "pictureInPictureElement should be null initially"
        );
    }

    #[test]
    fn document_picture_in_picture_enabled_is_true() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt.eval("document.pictureInPictureEnabled === true").unwrap();
        assert_eq!(result, JsValue::Bool(true), "pictureInPictureEnabled should be true");
    }

    #[test]
    fn disable_picture_in_picture_attribute() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt
            .eval(
                r#"
var el = document.createElement('video');
el.disablePictureInPicture === false
"#,
            )
            .unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "disablePictureInPicture should be false by default"
        );
    }

    #[test]
    fn lumen_pip_deliver_resize_is_function() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        let result = rt
            .eval("typeof _lumen_pip_deliver_resize === 'function'")
            .unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "_lumen_pip_deliver_resize should be a global function"
        );
    }

    #[test]
    fn exit_pip_rejects_when_no_active_pip() {
        let rt = V8JsRuntime::new().unwrap();
        install_minimal_dom(&rt);
        super::install_video_pip_api_v8(&rt).unwrap();
        // exitPictureInPicture returns a Promise that rejects when no PiP is active.
        let result = rt
            .eval("document.exitPictureInPicture() instanceof Promise")
            .unwrap();
        assert_eq!(
            result,
            JsValue::Bool(true),
            "exitPictureInPicture() should return a Promise"
        );
    }
}
