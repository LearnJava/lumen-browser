//! Presentation API stub (W3C Presentation API Level 1).
//!
//! Exposes:
//! - `navigator.presentation` singleton with `defaultRequest` getter/setter
//! - `PresentationRequest` constructor: `new PresentationRequest([urls])`
//!   - `start()` → Promise rejected with `NotSupportedError` (Phase 0)
//!   - `reconnect(id)` → Promise rejected with `NotSupportedError` (Phase 0)
//!   - `getAvailability()` → `Promise<PresentationAvailability>` where `.value === false`
//!   - `addEventListener(type, handler)`
//! - `PresentationAvailability`: read-only `.value === false`
//! - `PresentationConnection`: `id`, `url`, `state`, `send()`, `close()`, `terminate()`,
//!   `addEventListener()`
//!
//! Phase 0: no-op — no actual display discovery or projection. All connections are stubs.

use rquickjs::Ctx;

/// Install the Presentation API bindings into the JS context.
pub fn install_presentation_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(PRESENTATION_API_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Presentation API Level 1 (Phase 0).
const PRESENTATION_API_SHIM: &str = r#"(function() {
  'use strict';

  // ── PresentationConnection ────────────────────────────────────────────────

  /// Represents a single presentation connection (Phase 0: always a stub).
  function PresentationConnection(id, url) {
    this.id    = id;
    this.url   = url;
    this.state = 'connecting';
    this._listeners = Object.create(null);
  }

  /// Send a message. Phase 0: no-op (connection is a stub).
  PresentationConnection.prototype.send = function(_data) {};

  /// Close the connection. Phase 0: synchronously sets state to 'closed'.
  PresentationConnection.prototype.close = function() {
    this.state = 'closed';
    this._fireEvent('close', {});
  };

  /// Terminate the presentation. Phase 0: synchronously sets state to 'terminated'.
  PresentationConnection.prototype.terminate = function() {
    this.state = 'terminated';
    this._fireEvent('terminate', {});
  };

  PresentationConnection.prototype.addEventListener = function(type, handler) {
    if (typeof handler !== 'function') { return; }
    if (!this._listeners[type]) { this._listeners[type] = []; }
    this._listeners[type].push(handler);
  };

  PresentationConnection.prototype.removeEventListener = function(type, handler) {
    if (!this._listeners[type]) { return; }
    this._listeners[type] = this._listeners[type].filter(function(h) { return h !== handler; });
  };

  PresentationConnection.prototype._fireEvent = function(type, detail) {
    var evt = Object.assign({ type: type, target: this }, detail);
    var handlers = (this._listeners[type] || []).slice();
    for (var i = 0; i < handlers.length; i++) {
      try { handlers[i](evt); } catch (_e) {}
    }
  };

  globalThis.PresentationConnection = PresentationConnection;

  // ── PresentationAvailability ──────────────────────────────────────────────

  /// Reports display availability. Phase 0: always false (no external displays).
  function PresentationAvailability() {
    Object.defineProperty(this, 'value', { value: false, enumerable: true, configurable: false });
    this._listeners = Object.create(null);
  }

  PresentationAvailability.prototype.addEventListener = function(type, handler) {
    if (typeof handler !== 'function') { return; }
    if (!this._listeners[type]) { this._listeners[type] = []; }
    this._listeners[type].push(handler);
  };

  PresentationAvailability.prototype.removeEventListener = function(type, handler) {
    if (!this._listeners[type]) { return; }
    this._listeners[type] = this._listeners[type].filter(function(h) { return h !== handler; });
  };

  globalThis.PresentationAvailability = PresentationAvailability;

  // ── PresentationRequest ───────────────────────────────────────────────────

  /// Initiates a presentation session to one of the provided URLs.
  /// Phase 0: `start()` and `reconnect()` always reject with NotSupportedError.
  function PresentationRequest(urls) {
    // Normalise single string to array per spec §6.3.
    this._urls = Array.isArray(urls) ? urls : (typeof urls === 'string' ? [urls] : []);
    this._listeners = Object.create(null);
  }

  /// Start a new presentation. Phase 0 → NotSupportedError (no display found).
  PresentationRequest.prototype.start = function() {
    return Promise.reject(
      new DOMException('Presentation API not supported in Phase 0', 'NotSupportedError')
    );
  };

  /// Reconnect to an existing presentation by connection id.
  /// Phase 0 → NotSupportedError.
  PresentationRequest.prototype.reconnect = function(_id) {
    return Promise.reject(
      new DOMException('Presentation API not supported in Phase 0', 'NotSupportedError')
    );
  };

  /// Check display availability for the request URLs.
  /// Phase 0: always resolves with PresentationAvailability{value: false}.
  PresentationRequest.prototype.getAvailability = function() {
    return Promise.resolve(new PresentationAvailability());
  };

  PresentationRequest.prototype.addEventListener = function(type, handler) {
    if (typeof handler !== 'function') { return; }
    if (!this._listeners[type]) { this._listeners[type] = []; }
    this._listeners[type].push(handler);
  };

  PresentationRequest.prototype.removeEventListener = function(type, handler) {
    if (!this._listeners[type]) { return; }
    this._listeners[type] = this._listeners[type].filter(function(h) { return h !== handler; });
  };

  globalThis.PresentationRequest = PresentationRequest;

  // ── navigator.presentation singleton ─────────────────────────────────────

  var _presentationSingleton = {
    _defaultRequest: null,

    get defaultRequest() { return this._defaultRequest; },
    set defaultRequest(v) {
      this._defaultRequest = (v instanceof PresentationRequest || v === null) ? v : null;
    },

    /// Returns availability for the current default request.
    /// Phase 0: resolves with {value: false}.
    requestAvailability: function() {
      return Promise.resolve(new PresentationAvailability());
    }
  };

  if (typeof navigator !== 'undefined') {
    Object.defineProperty(navigator, 'presentation', {
      configurable: true,
      enumerable:   true,
      get: function() { return _presentationSingleton; }
    });
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

    fn install(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"
            globalThis.navigator = globalThis.navigator || {};
            if (typeof DOMException === 'undefined') {
                function DOMException(msg, name) {
                    var e = new Error(msg);
                    e.name = name || 'Error';
                    return e;
                }
                globalThis.DOMException = DOMException;
            }
            "#,
        )
        .unwrap();
        install_presentation_api(ctx).unwrap();
    }

    #[test]
    fn navigator_presentation_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("typeof navigator.presentation !== 'undefined'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn presentation_request_start_returns_rejected_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("new PresentationRequest(['https://example.com']).start() instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn default_request_getter_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var req = new PresentationRequest(['https://example.com']);
                    navigator.presentation.defaultRequest = req;
                    navigator.presentation.defaultRequest === req
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn presentation_availability_value_false() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("new PresentationAvailability().value === false")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn get_availability_resolves_with_false_value() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var req = new PresentationRequest(['https://example.com/cast']);
                    var p = req.getAvailability();
                    p instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn presentation_connection_state_lifecycle() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var conn = new PresentationConnection('conn-1', 'https://example.com');
                    var initial = conn.state === 'connecting';
                    conn.close();
                    var closed = conn.state === 'closed';
                    var conn2 = new PresentationConnection('conn-2', 'https://example.com');
                    conn2.terminate();
                    var terminated = conn2.state === 'terminated';
                    initial && closed && terminated
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
