//! WebTransport (W3C, over HTTP/3) — spec-shaped JS API, срез 1.
//!
//! ROADMAP `P3-webtransport`, decomposition in `docs/tasks/ph3-webtransport.md`.
//! The former `crates/js/src/webtransport.rs` (2026-05 Phase 0 stub, all
//! operations synchronously rejecting `'phase-0-stub'`) was deleted 2026-07-21
//! because it was unreachable dead code — never wired into `install_dom` on
//! either runtime. This is a fresh implementation.
//!
//! Срез 0 (live QUIC/H3 IO, `P3-h3`) is `done`: `crates/network/src/h3/` now
//! drives a real UDP transport and QUIC handshake (`client_bootstrap.rs`,
//! `udp.rs`). What is still missing is Extended CONNECT (RFC 9220) — nothing
//! builds a `:protocol = webtransport` CONNECT stream yet — and the QUIC
//! DATAGRAM frame (RFC 9221) is a pure codec (`h3::datagram`) not wired to a
//! socket. So this slice gives the JS surface its final spec shape
//! (`WebTransport`, `WebTransportError`, `WebTransportDatagramDuplexStream`,
//! `WebTransportBidirectionalStream`) with one native binding,
//! `_lumen_webtransport_open`, that always answers "not connected" — later
//! slices (Extended CONNECT, uni/bidi streams, datagrams) replace only the
//! Rust side of that native, not this JS shape.

/// Installs the `WebTransport` global constructor and its supporting classes.
///
/// Must be called after DOM install (needs `document`'s `URL`, `DOMException`,
/// `ReadableStream`/`WritableStream`, `Promise`).
///
/// `fetch_provider` is `None` in contexts with no network access at all
/// (detached documents, some test runtimes) — the session then always
/// answers "not connected", same as before срез 2b.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_webtransport_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    fetch_provider: Option<std::sync::Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn1;
    use lumen_core::ext::JsRuntime as _;

    // GAP-WEBTRANSPORT срез 2b: `_lumen_webtransport_open(url)` now drives a
    // real Extended CONNECT (RFC 9220) attempt through the fetch provider —
    // `lumen-network::HttpClient` overrides `webtransport_connect`, every
    // other provider keeps the "unsupported" default. Returns a JSON object
    // as a string (matching the `_lumen_fetch_*` cache-slot pattern would be
    // overkill for three fields read exactly once by the shim's `setTimeout`
    // callback): `{"ok":true,"status":200}` or `{"ok":false,"message":"…"}`.
    // The handle itself is not surfaced to JS yet — срез 3 (uni/bidi
    // streams) is what first needs it, and will extend this JSON then.
    let open = into_v8_fn1(move |url: String| -> String {
        let Some(ref provider) = fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_connect(&url) {
            Ok(session) => format!(r#"{{"ok":true,"status":{}}}"#, session.status),
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_open", open)?;

    rt.eval(WEBTRANSPORT_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the WebTransport spec shape.
///
/// Reference: <https://www.w3.org/TR/webtransport/>.
#[cfg(feature = "v8-backend")]
const WEBTRANSPORT_SHIM: &str = r#"(function() {
  'use strict';

  // ── URL validation (spec §5.1 "Create a WebTransport" steps 1-4) ─────────
  function parseWebTransportUrl(url) {
    var parsed;
    try {
      parsed = new URL(url);
    } catch (e) {
      throw new DOMException(
        "Failed to construct 'WebTransport': '" + String(url) + "' is not a valid URL.",
        'SyntaxError'
      );
    }
    if (parsed.protocol !== 'https:') {
      throw new DOMException(
        "Failed to construct 'WebTransport': The URL's scheme must be 'https'. '" +
          parsed.protocol + "' is not allowed.",
        'SyntaxError'
      );
    }
    if (parsed.hash) {
      throw new DOMException(
        "Failed to construct 'WebTransport': The URL must not have a fragment.",
        'SyntaxError'
      );
    }
    return parsed;
  }

  // ── WebTransportError (spec: `interface WebTransportError : DOMException`) ─
  class WebTransportError extends DOMException {
    constructor(init) {
      init = init || {};
      var message = typeof init.message === 'string' ? init.message : '';
      super(message, 'WebTransportError');
      this._wtSource = init.source === 'stream' ? 'stream' : 'session';
      this._wtStreamErrorCode =
        typeof init.streamErrorCode === 'number'
          ? (init.streamErrorCode >>> 0) & 0xff
          : null;
    }
    get source() { return this._wtSource; }
    get streamErrorCode() { return this._wtStreamErrorCode; }
    get [Symbol.toStringTag]() { return 'WebTransportError'; }
  }

  function notConnectedError() {
    return new WebTransportError({
      source: 'session',
      message: 'The WebTransport session is not connected.',
    });
  }

  // A stream half that never produces/accepts data — used for
  // `incomingBidirectionalStreams`/`incomingUnidirectionalStreams` (nothing
  // ever arrives from a peer we never connect to) and for the readable half
  // of `datagrams` (same reason). Reused rather than duplicated per call site.
  function emptyReadableStream() {
    return new ReadableStream({
      start: function() {},
      pull: function() { /* never resolves — no data will ever arrive */ },
    });
  }

  // The writable half of `datagrams`, and the model for a not-yet-existing
  // outgoing stream: every write rejects, because there is no session to
  // carry it.
  function rejectingWritableStream() {
    return new WritableStream({
      write: function() { return Promise.reject(notConnectedError()); },
    });
  }

  // ── WebTransportDatagramDuplexStream (spec §7) ────────────────────────────
  class WebTransportDatagramDuplexStream {
    constructor() {
      this._readable = emptyReadableStream();
      this._writable = rejectingWritableStream();
    }
    get readable() { return this._readable; }
    get writable() { return this._writable; }
    get maxDatagramSize() { return 0; }
    get incomingMaxAge() { return null; }
    set incomingMaxAge(v) { /* no live session to apply this to yet */ }
    get outgoingMaxAge() { return null; }
    set outgoingMaxAge(v) {}
    get incomingHighWaterMark() { return 1; }
    set incomingHighWaterMark(v) {}
    get outgoingHighWaterMark() { return 1; }
    set outgoingHighWaterMark(v) {}
  }

  // ── WebTransportBidirectionalStream (spec §6.2) ───────────────────────────
  function WebTransportBidirectionalStream(readable, writable) {
    this._readable = readable;
    this._writable = writable;
  }
  Object.defineProperty(WebTransportBidirectionalStream.prototype, 'readable', {
    get: function() { return this._readable; }, enumerable: true, configurable: true,
  });
  Object.defineProperty(WebTransportBidirectionalStream.prototype, 'writable', {
    get: function() { return this._writable; }, enumerable: true, configurable: true,
  });

  // ── WebTransport (spec §5) ─────────────────────────────────────────────────
  function WebTransport(url, options) {
    if (new.target === undefined) {
      throw new TypeError("Failed to construct 'WebTransport': Please use the 'new' operator.");
    }
    parseWebTransportUrl(url);
    options = options || {};

    this._url = url;
    this._datagrams = new WebTransportDatagramDuplexStream();
    this._incomingBidi = emptyReadableStream();
    this._incomingUnidi = emptyReadableStream();
    this._closed = false;

    var self = this;
    var readyPromise = new Promise(function(resolve, reject) {
      self._readyResolve = resolve;
      self._readyReject = reject;
    });
    var closedPromise = new Promise(function(resolve, reject) {
      self._closedResolve = resolve;
      self._closedReject = reject;
    });
    // Both settle silently (unhandled-rejection-free) — a caller reading
    // only `closed` after `ready` already told it the session never opened
    // must not also get a second, unobserved rejection.
    readyPromise.catch(function() {});
    closedPromise.catch(function() {});
    this._ready = readyPromise;
    this._closedPromise = closedPromise;

    // GAP-WEBTRANSPORT срез 2b: `_lumen_webtransport_open` now drives a real
    // Extended CONNECT (RFC 9220) attempt and reports the outcome as JSON —
    // `{ok:true,status}` on a 2xx response, `{ok:false,message}` otherwise
    // (including "not supported in this context", the old always-fail
    // answer). Streams/datagrams stay stubs until срезы 3-4 give the session
    // a handle to drive them from; `closed` is deliberately left pending on
    // success — no lifecycle wiring (срез 5) exists yet to ever settle it.
    setTimeout(function() {
      var result;
      try {
        result = JSON.parse(_lumen_webtransport_open(self._url));
      } catch (e) {
        result = { ok: false, message: 'WebTransport: malformed native response.' };
      }
      if (result && result.ok) {
        self._readyResolve(undefined);
      } else {
        var err = new WebTransportError({
          source: 'session',
          message: (result && result.message) || 'The WebTransport session is not connected.',
        });
        self._readyReject(err);
        self._closedReject(err);
      }
    }, 0);
  }

  Object.defineProperty(WebTransport.prototype, 'ready', {
    get: function() { return this._ready; }, enumerable: true, configurable: true,
  });
  Object.defineProperty(WebTransport.prototype, 'closed', {
    get: function() { return this._closedPromise; }, enumerable: true, configurable: true,
  });
  Object.defineProperty(WebTransport.prototype, 'datagrams', {
    get: function() { return this._datagrams; }, enumerable: true, configurable: true,
  });
  Object.defineProperty(WebTransport.prototype, 'incomingBidirectionalStreams', {
    get: function() { return this._incomingBidi; }, enumerable: true, configurable: true,
  });
  Object.defineProperty(WebTransport.prototype, 'incomingUnidirectionalStreams', {
    get: function() { return this._incomingUnidi; }, enumerable: true, configurable: true,
  });

  WebTransport.prototype.createBidirectionalStream = function() {
    return Promise.reject(notConnectedError());
  };
  WebTransport.prototype.createUnidirectionalStream = function() {
    return Promise.reject(notConnectedError());
  };
  WebTransport.prototype.getStats = function() {
    return Promise.resolve({});
  };
  WebTransport.prototype.close = function(closeInfo) {
    if (this._closed) return;
    this._closed = true;
    // The session was never open — closing early does not change the
    // rejection `ready`/`closed` already carry (§5.4 "Closing a
    // WebTransport session").
  };

  Object.defineProperty(globalThis, 'WebTransport', {
    value: WebTransport, writable: true, enumerable: false, configurable: true,
  });
  Object.defineProperty(globalThis, 'WebTransportError', {
    value: WebTransportError, writable: true, enumerable: false, configurable: true,
  });
  Object.defineProperty(globalThis, 'WebTransportBidirectionalStream', {
    value: WebTransportBidirectionalStream, writable: true, enumerable: false, configurable: true,
  });
  Object.defineProperty(globalThis, 'WebTransportDatagramDuplexStream', {
    value: WebTransportDatagramDuplexStream, writable: true, enumerable: false, configurable: true,
  });
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    #![allow(clippy::unwrap_used)]

    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    // Full `install_dom`, not just `DOM_EXCEPTION_POLYFILL`: the constructor
    // validates the URL through the real `URL` class (`WEB_API_SHIM`'s
    // `url_shim.js`), which itself reads `_LUMEN_PAGE_URL` seeded by
    // `install_dom` — a bare runtime has no `URL` global at all.
    fn rt_with_webtransport() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false).unwrap();
        super::install_webtransport_v8(&rt, None).unwrap();
        rt
    }

    /// A fetch provider whose `webtransport_connect` answers deterministically
    /// (`Ok`/`Err`) instead of network I/O — GAP-WEBTRANSPORT срез 2b.
    struct StubFetch {
        result: std::sync::Mutex<Option<lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>>>,
    }
    impl lumen_core::ext::JsFetchProvider for StubFetch {
        fn fetch_sync(&self, _url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            Err(lumen_core::error::Error::Network("unused in this test".to_string()))
        }
        fn webtransport_connect(&self, _url: &str) -> lumen_core::error::Result<lumen_core::ext::JsWebTransportSession> {
            match self.result.lock().unwrap().take() {
                Some(r) => r,
                None => Err(lumen_core::error::Error::Network("StubFetch called twice".to_string())),
            }
        }
    }

    /// Unlike [`rt_with_webtransport`], does not call `install_webtransport_v8`
    /// a second time — `install_dom` already installs it once, internally, via
    /// the `fetch_provider` argument here; a second `register_native` call for
    /// the same name would not be reliably observable as an override, so the
    /// provider has to go in through this one call.
    fn rt_with_webtransport_provider(
        result: lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>,
    ) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> =
            Arc::new(StubFetch { result: std::sync::Mutex::new(Some(result)) });
        rt.install_dom(doc, "", Some(provider), None, None, None, None, None, None, None, false)
            .unwrap();
        rt
    }

    fn check(rt: &V8JsRuntime, expr: &str) {
        assert_eq!(rt.eval(expr).unwrap(), JsValue::Bool(true), "assertion failed for `{expr}`");
    }

    #[test]
    fn constructor_rejects_non_https_scheme_synchronously() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                try { new WebTransport('http://example.com/wt'); return false; } \
                catch (e) { return e instanceof DOMException && e.name === 'SyntaxError'; } \
            })()",
        );
    }

    #[test]
    fn constructor_rejects_fragment() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                try { new WebTransport('https://example.com/wt#frag'); return false; } \
                catch (e) { return e instanceof DOMException && e.name === 'SyntaxError'; } \
            })()",
        );
    }

    #[test]
    fn constructor_accepts_https_and_exposes_spec_shape() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var wt = new WebTransport('https://example.com:4433/wt'); \
                return wt.ready instanceof Promise && \
                    wt.closed instanceof Promise && \
                    wt.datagrams instanceof WebTransportDatagramDuplexStream && \
                    wt.datagrams.readable instanceof ReadableStream && \
                    wt.datagrams.writable instanceof WritableStream && \
                    wt.incomingBidirectionalStreams instanceof ReadableStream && \
                    wt.incomingUnidirectionalStreams instanceof ReadableStream && \
                    typeof wt.createBidirectionalStream === 'function' && \
                    typeof wt.createUnidirectionalStream === 'function' && \
                    typeof wt.close === 'function'; \
            })()",
        );
    }

    /// `createBidirectionalStream()` rejects synchronously (no `setTimeout`
    /// involved, unlike `ready`/`closed`), so the microtask checkpoint V8
    /// runs at the end of the first `eval` already resolved the `.catch`
    /// handler by the time the second `eval` reads its result back.
    #[test]
    fn create_streams_reject_with_web_transport_error_before_ready() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtTestResult = false; \
            (function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.createBidirectionalStream().catch(function(e) { \
                    globalThis._wtTestResult = (e instanceof WebTransportError) && e.source === 'session'; \
                }); \
            })();",
        )
        .unwrap();
        check(&rt, "_wtTestResult");
    }

    /// GAP-WEBTRANSPORT срез 2b: no `setTimeout` involved here — the native
    /// binding itself is called directly, same workaround
    /// `websocket_connect_fail_fires_onerror` uses ("we can't pump the
    /// timeout in this test"), to check the JSON shape `ready`'s callback
    /// parses without depending on macrotask pumping this harness lacks.
    #[test]
    fn native_open_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_reports_ok_and_status_on_success() {
        let rt = rt_with_webtransport_provider(Ok(lumen_core::ext::JsWebTransportSession {
            handle: 0,
            status: 200,
        }));
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains("200"), "expected status 200, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_reports_provider_error_message() {
        let rt = rt_with_webtransport_provider(Err(lumen_core::error::Error::Network(
            "WebTransport Extended CONNECT rejected: status 403".to_string(),
        )));
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
                assert!(s.contains("403"), "expected the status in the message, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn web_transport_error_source_defaults_to_session() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var e = new WebTransportError({ message: 'boom' }); \
                return e instanceof DOMException && \
                    e.name === 'WebTransportError' && \
                    e.message === 'boom' && \
                    e.source === 'session' && \
                    e.streamErrorCode === null; \
            })()",
        );
    }

    #[test]
    fn web_transport_error_stream_source_and_code_roundtrip() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var e = new WebTransportError({ source: 'stream', streamErrorCode: 7 }); \
                return e.source === 'stream' && e.streamErrorCode === 7; \
            })()",
        );
    }

    #[test]
    fn close_is_idempotent() {
        let rt = rt_with_webtransport();
        check(
            &rt,
            "(function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.close(); \
                wt.close({ closeCode: 0, reason: '' }); \
                return true; \
            })()",
        );
    }
}
