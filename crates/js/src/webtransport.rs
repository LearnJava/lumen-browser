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
#[cfg(feature = "v8-backend")]
pub(crate) fn install_webtransport_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn1;
    use lumen_core::ext::JsRuntime as _;

    // Sentinel per the BUG-457 invariant: a negative `i32`, never `u32::MAX`
    // widened through `IntoJsReturn for u32` (which V8 sees as the *positive*
    // 4294967295.0). `-1` means "no live WebTransport session for this URL
    // yet" — the only answer until Extended CONNECT lands.
    let open = into_v8_fn1(move |_url: String| -> i32 { -1 });
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

    // `_lumen_webtransport_open` currently always answers "not connected"
    // (no Extended CONNECT yet) — see the module doc comment. When a future
    // slice makes it return a live session handle, only this branch and the
    // stream/datagram bodies above need to change; the class shape does not.
    setTimeout(function() {
      _lumen_webtransport_open(self._url);
      var err = notConnectedError();
      self._readyReject(err);
      self._closedReject(err);
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
        super::install_webtransport_v8(&rt).unwrap();
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
