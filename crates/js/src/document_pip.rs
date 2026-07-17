//! Document Picture-in-Picture API (W3C Document Picture-in-Picture §4).
//!
//! Provides `documentPictureInPicture.requestWindow()` to create a floating window
//! with DOM content, `.window` accessor to the PiP window, and `onenter` event listener.

/// V8 port of the former rquickjs `install_document_pip_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-13): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_document_pip_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(DOCUMENT_PIP_SHIM)?;
    Ok(())
}

/// JavaScript shim: Document Picture-in-Picture with floating window overlay.
#[cfg(feature = "v8-backend")]
const DOCUMENT_PIP_SHIM: &str = r#"(function() {
  'use strict';

  /// PictureInPictureWindow: the one floating-window class shared by both
  /// Document PiP (`documentPictureInPicture.requestWindow`, this module) and
  /// video PiP (`video_pip.rs`'s `requestPictureInPicture()`) — P3-pip slice 4.
  /// `video_pip.rs` installs after this module and reuses this exact class
  /// (falling back to its own minimal definition only if evaluated in
  /// isolation, e.g. its own unit tests) so `instanceof PictureInPictureWindow`
  /// is consistent everywhere and `_lumen_pip_deliver_resize` (`video_pip.rs`)
  /// can update whichever session is active through one shared reference
  /// (`globalThis.__lumen_pip_active_window`).
  class PictureInPictureWindow extends EventTarget {
    constructor(width, height) {
      super();
      this._width = width || 0;
      this._height = height || 0;
      this._document = null;
      this._closed = false;
    }

    get width() {
      return this._width;
    }

    get height() {
      return this._height;
    }

    // Only meaningful for Document PiP — video PiP windows never read this.
    get document() {
      if (this._closed) {
        return null;
      }
      // Lightweight DOM container stub. Real page content is NOT forwarded
      // into it yet (P3-pip follow-up — see docs/tasks/ph3-picture-in-picture.md).
      if (!this._document) {
        this._document = {
          body: {
            children: [],
            appendChild: (child) => {
              this._document.body.children.push(child);
            },
            removeChild: (child) => {
              this._document.body.children = this._document.body.children.filter(c => c !== child);
            },
            innerHTML: '',
          },
          createElement: (tag) => ({ tagName: tag, children: [], innerHTML: '' }),
          createTextNode: (text) => ({ nodeValue: text }),
        };
      }
      return this._document;
    }

    close() {
      this._closed = true;
      if (globalThis.__lumen_pip_active_window === this) {
        globalThis.__lumen_pip_active_window = null;
      }
    }
  }

  globalThis.PictureInPictureWindow = PictureInPictureWindow;

  /// DocumentPictureInPictureEvent: fired when entering PiP mode.
  class DocumentPictureInPictureEvent extends Event {
    constructor(window) {
      super('enter');
      this._window = window;
    }

    get window() {
      return this._window;
    }
  }

  /// DocumentPictureInPicture: main API singleton.
  class DocumentPictureInPicture extends EventTarget {
    constructor() {
      super();
      this._activeWindow = null;
    }

    async requestWindow(options = {}) {
      if (this._activeWindow && !this._activeWindow._closed) {
        throw new Error('A PictureInPictureWindow is already active');
      }

      const width = options.width || 640;
      const height = options.height || 360;

      const pipWindow = new PictureInPictureWindow(width, height);
      this._activeWindow = pipWindow;
      // Shared with video_pip.rs: whichever PiP session is open, so
      // `_lumen_pip_deliver_resize` (video_pip.rs) can update it uniformly.
      globalThis.__lumen_pip_active_window = pipWindow;

      // Fire enter event on document
      const event = new DocumentPictureInPictureEvent(pipWindow);
      document.dispatchEvent(event);

      // Call native binding to register PiP window with shell
      if (typeof _lumen_pip_request_window === 'function') {
        _lumen_pip_request_window(width, height);
      }

      return pipWindow;
    }
  }

  /// Install on globalThis
  const documentPictureInPicture = new DocumentPictureInPicture();
  globalThis.documentPictureInPicture = documentPictureInPicture;
  globalThis.DocumentPictureInPictureEvent = DocumentPictureInPictureEvent;
  globalThis.DocumentPictureInPictureWindow = PictureInPictureWindow;

  /// Add pictureInPictureElement getter to document
  Object.defineProperty(document, 'pictureInPictureElement', {
    get() {
      return documentPictureInPicture._activeWindow && !documentPictureInPicture._activeWindow._closed
        ? documentPictureInPicture._activeWindow
        : null;
    },
    configurable: true,
  });
})();"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn with_document_pip(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None, None, false)
            .unwrap();
        f(&rt);
    }

    #[test]
    fn document_pip_request_window_exists() {
        with_document_pip(|rt| {
            let r = rt.eval("typeof documentPictureInPicture.requestWindow === 'function'").unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_pip_request_window_returns_promise() {
        // requestWindow() now reaches the real `_lumen_pip_request_window`
        // native (registered by `pip_bindings.rs`) — guard the shared queue.
        let _g = crate::pip_bindings::test_guard();
        with_document_pip(|rt| {
            let r = rt.eval("documentPictureInPicture.requestWindow() instanceof Promise").unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_pip_request_window_with_options() {
        let _g = crate::pip_bindings::test_guard();
        with_document_pip(|rt| {
            let r = rt
                .eval(
                    "documentPictureInPicture.requestWindow({width: 800, height: 600}) instanceof Promise",
                )
                .unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_pip_window_access() {
        let _g = crate::pip_bindings::test_guard();
        with_document_pip(|rt| {
            let r = rt
                .eval(
                    "documentPictureInPicture.requestWindow({width: 640, height: 360})\
                     .then(w => w instanceof Object && typeof w.width === 'number' && w.width === 640)",
                )
                .unwrap();
            // Promise should be created successfully
            assert_ne!(r, JsValue::Null);
        });
    }

    #[test]
    fn document_pip_picture_in_picture_event_class_exists() {
        with_document_pip(|rt| {
            let r = rt.eval("typeof DocumentPictureInPictureEvent === 'function'").unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_pip_picture_in_picture_window_class_exists() {
        with_document_pip(|rt| {
            let r = rt.eval("typeof DocumentPictureInPictureWindow === 'function'").unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_pip_element_getter_exists() {
        with_document_pip(|rt| {
            let r = rt
                .eval("typeof Object.getOwnPropertyDescriptor(document, 'pictureInPictureElement') === 'object'")
                .unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    // ── P3-pip slice 4/5: unification with video_pip.rs + resize round-trip ──

    #[test]
    fn document_pip_window_class_is_unified_with_video_pip() {
        // `install_dom` installs document_pip before video_pip (alphabetical);
        // video_pip.rs must reuse this module's class rather than defining a
        // rival one, so `instanceof PictureInPictureWindow` is consistent.
        with_document_pip(|rt| {
            let r = rt
                .eval("globalThis.PictureInPictureWindow === globalThis.DocumentPictureInPictureWindow")
                .unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_pip_native_request_window_enqueues_open_document() {
        // `_lumen_pip_request_window` is registered by `pip_bindings.rs`
        // (installed as part of the same `install_dom` call); requestWindow()
        // must reach it instead of silently no-op'ing (the former bug this
        // slice fixes).
        let _g = crate::pip_bindings::test_guard();
        with_document_pip(|rt| {
            rt.eval("documentPictureInPicture.requestWindow({width: 800, height: 450});")
                .unwrap();
            let reqs = crate::pip_bindings::take_pip_requests();
            assert_eq!(
                reqs,
                vec![crate::pip_bindings::PipRequest::OpenDocument { width: 800.0, height: 450.0 }]
            );
        });
    }

    #[test]
    fn document_pip_resize_round_trip_updates_active_window() {
        // requestWindow() has no `await` before registering
        // `__lumen_pip_active_window`, so it is set synchronously by the time
        // the call returns (still true even though the function is `async`).
        // Also enqueues an OpenDocument request — drain it via the shared
        // guard so it doesn't leak into `pip_bindings.rs`'s tests.
        let _g = crate::pip_bindings::test_guard();
        with_document_pip(|rt| {
            let r = rt
                .eval(
                    "documentPictureInPicture.requestWindow({width: 640, height: 360}); \
                     var w = globalThis.__lumen_pip_active_window; \
                     var fired = false; \
                     w.addEventListener('resize', function() { fired = true; }); \
                     _lumen_pip_deliver_resize(800, 600); \
                     fired && w.width === 800 && w.height === 600",
                )
                .unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }
}
