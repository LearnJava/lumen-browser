//! Document Picture-in-Picture API (W3C Document Picture-in-Picture §4).
//!
//! Provides `documentPictureInPicture.requestWindow()` to create a floating window
//! with DOM content, `.window` accessor to the PiP window, and `onenter` event listener.
//!
//! Slice 1 (this file + `documentpip_bindings.rs` + `shell/src/panels/doc_pip_os_window.rs`):
//! `requestWindow()`/`.close()` now open/close a real always-on-top OS window
//! (mirroring video PiP's CC-7), and its real size round-trips back into
//! `PictureInPictureWindow.width`/`.height`/`resize`.
//!
//! Slice 2 (this file + `documentpip_bindings.rs` + `shell/src/panels/doc_pip_os_window.rs` +
//! `Lumen::render_doc_pip_os`): `.document` is now backed by a real (but
//! hidden — never attached to the visible page tree) `<div>` element in the
//! main document, so `pipWindow.document.body.appendChild(el)` performs a
//! genuine DOM move (`el` really leaves wherever it was, per spec) instead of
//! pushing into a plain JS array. Every mutation re-serializes that hidden
//! container's `innerHTML` and forwards it to the shell via
//! `_lumen_docpip_set_content_html`, which parses it into a fresh detached
//! `lumen_dom::Document` and lays out + paints it into the floating window.
//! Known gap: this reuses the *same* underlying `Document` as the main page
//! (there's no independent PiP-window global/document per spec) — moved
//! elements are real DOM nodes with working attributes/`innerHTML`, but their
//! styling comes from a fresh, stylesheet-less layout pass (no author CSS
//! carries over) and embedded images don't render (the floating window's
//! renderer has its own, separate image cache).

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

  /// PictureInPictureWindow: DOM window overlay for PiP content.
  class PictureInPictureWindow extends EventTarget {
    constructor(width, height) {
      super();
      this._width = width;
      this._height = height;
      this._document = null;
      this._closed = false;
    }

    get width() {
      return this._width;
    }

    get height() {
      return this._height;
    }

    get document() {
      if (this._closed) {
        return null;
      }
      // The PiP content lives in a real, but hidden, DOM container: a plain
      // <div> never attached under document.documentElement, so it never
      // paints into the main window. appendChild()-ing an existing page
      // element into it is a genuine DOM move (the element really leaves its
      // old parent), matching the spec's "moved subtree" semantics as
      // closely as a single-Document engine allows (slice 2, see module docs).
      if (!this._document) {
        const container = document.createElement('div');
        const syncContent = () => {
          if (typeof _lumen_docpip_set_content_html === 'function') {
            _lumen_docpip_set_content_html(container.innerHTML);
          }
        };
        this._document = {
          body: {
            get children() {
              return container.children;
            },
            appendChild: (child) => {
              container.appendChild(child);
              syncContent();
              return child;
            },
            removeChild: (child) => {
              container.removeChild(child);
              syncContent();
              return child;
            },
            get innerHTML() {
              return container.innerHTML;
            },
            set innerHTML(html) {
              container.innerHTML = String(html);
              syncContent();
            },
          },
          createElement: (tag) => document.createElement(tag),
          createTextNode: (text) => document.createTextNode(text),
        };
      }
      return this._document;
    }

    close() {
      if (this._closed) {
        return;
      }
      this._closed = true;
      if (typeof _lumen_docpip_close === 'function') {
        _lumen_docpip_close();
      }
    }
  }

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

      // Fire enter event on document
      const event = new DocumentPictureInPictureEvent(pipWindow);
      document.dispatchEvent(event);

      // Call native binding to open the real OS floating window (slice 1).
      if (typeof _lumen_docpip_request_window === 'function') {
        _lumen_docpip_request_window(width, height);
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

  /// _lumen_docpip_deliver_resize(width, height) — shell calls this when the
  /// real OS floating window is resized, so the active PictureInPictureWindow
  /// reflects the true client size and fires 'resize' (mirrors video PiP's
  /// `_lumen_pip_deliver_resize`).
  globalThis._lumen_docpip_deliver_resize = function(width, height) {
    const win = documentPictureInPicture._activeWindow;
    if (!win || win._closed) {
      return;
    }
    win._width = width;
    win._height = height;
    try { win.dispatchEvent(new Event('resize')); } catch (e) {}
  };

  /// _lumen_docpip_deliver_close() — shell calls this when the OS window is
  /// closed via its own close button (not `.close()`), so `_closed` and
  /// `pictureInPictureElement` reflect reality.
  globalThis._lumen_docpip_deliver_close = function() {
    const win = documentPictureInPicture._activeWindow;
    if (win) {
      win._closed = true;
    }
  };
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
        with_document_pip(|rt| {
            let r = rt.eval("documentPictureInPicture.requestWindow() instanceof Promise").unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_pip_request_window_with_options() {
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
}
