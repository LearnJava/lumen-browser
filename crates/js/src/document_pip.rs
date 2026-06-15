/// Document Picture-in-Picture API (W3C Document Picture-in-Picture §4).
///
/// Provides `documentPictureInPicture.requestWindow()` to create a floating window
/// with DOM content, `.window` accessor to the PiP window, and `onenter` event listener.
use rquickjs::Ctx;

/// Install Document Picture-in-Picture API into the JS context.
pub fn install_document_pip_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(DOCUMENT_PIP_SHIM)?;
    Ok(())
}

/// JavaScript shim: Document Picture-in-Picture with floating window overlay.
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
      // Create a lightweight DOM container for the PiP content
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
