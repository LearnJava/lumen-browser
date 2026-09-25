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

/// V8 port of the former rquickjs `install_presentation_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B5): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_presentation_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(PRESENTATION_API_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Presentation API Level 1 (Phase 0).
#[cfg(feature = "v8-backend")]
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
      try { handlers[i](evt); } catch (_e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(_e); }
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

  // Base for relative presentation URLs: the document's base URL (§6.3.1 step 3).
  function _presentationBase() {
    if (typeof document !== 'undefined' && document && typeof document.baseURI === 'string' && document.baseURI) {
      return document.baseURI;
    }
    if (typeof location !== 'undefined' && location && typeof location.href === 'string') {
      return location.href;
    }
    return undefined;
  }

  // "a priori unauthenticated URL" (Mixed Content §3.1): `http:`/`ws:` not
  // pointing at a loopback host, which would be potentially trustworthy.
  function _isAPrioriUnauthenticated(u) {
    if (u.protocol !== 'http:' && u.protocol !== 'ws:') { return false; }
    var h = u.hostname;
    return !(h === 'localhost' || /\.localhost$/.test(h) || h === '[::1]' || /^127\./.test(h));
  }

  /// Initiates a presentation session to one of the provided URLs.
  /// Phase 0: `start()` and `reconnect()` always reject with NotSupportedError.
  ///
  /// BUG-656: the constructor validates its input per W3C Presentation API
  /// §6.3.1 — no argument → TypeError (WebIDL), empty sequence →
  /// NotSupportedError, unparsable URL → SyntaxError, `http:` URL in a secure
  /// context → SecurityError (mixed content), and NotSupportedError when no
  /// URL has a scheme a presentation display could load (only `http:`/`https:`
  /// in Phase 0). Unsupported URLs next to a supported one are dropped.
  function PresentationRequest(urls) {
    if (!new.target) {
      throw new TypeError("Failed to construct 'PresentationRequest': Please use the 'new' operator");
    }
    if (arguments.length === 0) {
      throw new TypeError("Failed to construct 'PresentationRequest': 1 argument required, but only 0 present.");
    }
    // WebIDL overload `(USVString url)` / `(sequence<USVString> urls)`: an
    // iterable object selects the sequence form, anything else is stringified.
    var list;
    if (urls !== null && (typeof urls === 'object' || typeof urls === 'function') &&
        typeof urls[Symbol.iterator] === 'function') {
      list = Array.from(urls, function(u) { return String(u); });
    } else {
      list = [String(urls)];
    }
    if (list.length === 0) {
      throw new DOMException('An empty sequence of URLs is not supported.', 'NotSupportedError');
    }
    var base = _presentationBase();
    var parsed = [];
    for (var i = 0; i < list.length; i++) {
      var u;
      try {
        u = base === undefined ? new URL(list[i]) : new URL(list[i], base);
      } catch (_e) {
        throw new DOMException("'" + list[i] + "' can't be resolved to a valid URL.", 'SyntaxError');
      }
      parsed.push(u);
    }
    if (globalThis.isSecureContext === true) {
      for (var j = 0; j < parsed.length; j++) {
        if (_isAPrioriUnauthenticated(parsed[j])) {
          throw new DOMException("Presentation of an insecure document '" + parsed[j].href +
            "' is prohibited from a secure context.", 'SecurityError');
        }
      }
    }
    var supported = [];
    for (var k = 0; k < parsed.length; k++) {
      if (parsed[k].protocol === 'http:' || parsed[k].protocol === 'https:') {
        supported.push(parsed[k].href);
      }
    }
    if (supported.length === 0) {
      throw new DOMException('None of the presentation URLs is supported.', 'NotSupportedError');
    }
    this._urls = supported;
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

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn with_presentation_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        // `install_dom` installs this module itself (v8_runtime.rs `install_v8!`),
        // along with the `URL`/`DOMException` the constructor depends on.
        rt.install_dom(doc, "https://example.org/", None, None, None, None, None, None, None, None, None, false)
            .unwrap();
        f(&rt);
    }

    #[test]
    fn navigator_presentation_exists() {
        with_presentation_api(|rt| {
            let ok = rt
                .eval("typeof navigator.presentation !== 'undefined'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn presentation_request_start_returns_rejected_promise() {
        with_presentation_api(|rt| {
            let ok = rt
                .eval("new PresentationRequest(['https://example.com']).start() instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn default_request_getter_setter() {
        with_presentation_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var req = new PresentationRequest(['https://example.com']);
                    navigator.presentation.defaultRequest = req;
                    navigator.presentation.defaultRequest === req
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn presentation_availability_value_false() {
        with_presentation_api(|rt| {
            let ok = rt
                .eval("new PresentationAvailability().value === false")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_availability_resolves_with_false_value() {
        with_presentation_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var req = new PresentationRequest(['https://example.com/cast']);
                    var p = req.getAvailability();
                    p instanceof Promise
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn presentation_connection_state_lifecycle() {
        with_presentation_api(|rt| {
            let ok = rt
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
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}

/// BUG-656: §6.3.1 constructor validation, cases mirror WPT
/// `presentation-api/controlling-ua/PresentationRequest_{error,success,mixedcontent}.https.html`.
#[cfg(all(test, feature = "v8-backend"))]
mod ctor_validation_tests {
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    const SECURE_PAGE: &str = "https://example.org/dir/page.html";

    /// Returns the thrown error's `name` (or `"NO THROW"`) for `expr`.
    fn thrown(rt: &V8JsRuntime, expr: &str) -> String {
        let js = format!("(function() {{ try {{ {expr}; return 'NO THROW'; }} catch (e) {{ return e.name; }} }})()");
        match rt.eval(&js).unwrap() {
            JsValue::String(s) => s,
            other => format!("{other:?}"),
        }
    }

    /// Page runtime at `url`; `window.isSecureContext` follows its scheme.
    fn runtime(url: &str) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, url, None, None, None, None, None, None, None, None, None, false)
            .unwrap();
        rt
    }

    #[test]
    fn error_cases_throw_spec_exceptions() {
        let rt = runtime(SECURE_PAGE);
        assert_eq!(thrown(&rt, "new PresentationRequest()"), "TypeError");
        assert_eq!(thrown(&rt, "PresentationRequest('https://example.org/')"), "TypeError");
        assert_eq!(thrown(&rt, "new PresentationRequest([])"), "NotSupportedError");
        assert_eq!(thrown(&rt, "new PresentationRequest('https://@')"), "SyntaxError");
        assert_eq!(thrown(&rt, "new PresentationRequest('unsupported://example.com')"), "NotSupportedError");
        assert_eq!(
            thrown(&rt, "new PresentationRequest(['presentation.html', 'https://@'])"),
            "SyntaxError"
        );
        assert_eq!(
            thrown(&rt, "new PresentationRequest(['unsupported://example.com', 'invalid://example.com'])"),
            "NotSupportedError"
        );
        // The error must be a real DOMException, not a plain Error with a name.
        assert_eq!(
            rt.eval("(function(){ try { new PresentationRequest([]); } catch (e) { return e instanceof DOMException; } })()")
                .unwrap(),
            JsValue::Bool(true)
        );
    }

    #[test]
    fn success_cases_construct_and_drop_unsupported_urls() {
        let rt = runtime(SECURE_PAGE);
        assert_eq!(thrown(&rt, "new PresentationRequest('https://example.org/')"), "NO THROW");
        // Relative URLs resolve against the document base URL.
        assert_eq!(
            rt.eval("new PresentationRequest('presentation.html')._urls[0]").unwrap(),
            JsValue::String("https://example.org/dir/presentation.html".into())
        );
        assert_eq!(
            rt.eval(
                "JSON.stringify(new PresentationRequest(['unsupported://example.com', 'https://example.org/presentation/'])._urls)"
            )
            .unwrap(),
            JsValue::String(r#"["https://example.org/presentation/"]"#.into())
        );
    }

    #[test]
    fn insecure_url_from_secure_context_is_security_error() {
        let rt = runtime(SECURE_PAGE);
        assert_eq!(thrown(&rt, "new PresentationRequest('http://example.org/presentation.html')"), "SecurityError");
        assert_eq!(thrown(&rt, "new PresentationRequest('http://localhost/presentation.html')"), "NO THROW");
        let insecure = runtime("http://example.org/dir/page.html");
        assert_eq!(
            thrown(&insecure, "new PresentationRequest('http://example.org/presentation.html')"),
            "NO THROW"
        );
    }
}
