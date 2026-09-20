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
    use crate::v8_compat::{into_v8_fn1, into_v8_fn2, into_v8_fn3};
    use lumen_core::ext::JsRuntime as _;

    let uni_fetch_provider = fetch_provider.clone();
    let bidi_fetch_provider = fetch_provider.clone();
    let write_fetch_provider = fetch_provider.clone();
    let close_fetch_provider = fetch_provider.clone();
    let abort_fetch_provider = fetch_provider.clone();
    let read_fetch_provider = fetch_provider.clone();
    let poll_incoming_uni_fetch_provider = fetch_provider.clone();
    let read_incoming_uni_fetch_provider = fetch_provider.clone();
    let poll_incoming_bidi_fetch_provider = fetch_provider.clone();
    let read_incoming_bidi_fetch_provider = fetch_provider.clone();

    // GAP-WEBTRANSPORT срез 3b: `_lumen_webtransport_open(url)` now also
    // reports the session `handle` `webtransport_connect` allocated — срез
    // 2b left it out of the JSON on purpose ("not surfaced to JS yet");
    // `createUnidirectionalStream()` is the first caller that needs it, to
    // pass back into `_lumen_webtransport_open_uni_stream(handle)` below.
    // Still `{"ok":true,"status":200,"handle":0}` or
    // `{"ok":false,"message":"…"}` — a JSON string, matching the
    // `_lumen_fetch_*` cache-slot pattern would be overkill for three fields
    // read exactly once by the shim's `setTimeout` callback.
    let open = into_v8_fn1(move |url: String| -> String {
        let Some(ref provider) = fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_connect(&url) {
            Ok(session) => {
                format!(r#"{{"ok":true,"status":{},"handle":{}}}"#, session.status, session.handle)
            }
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_open", open)?;

    // GAP-WEBTRANSPORT срез 3b: `createUnidirectionalStream()`'s first native
    // call — opens a client-initiated QUIC uni-stream on the session
    // `handle` names (`h3_webtransport_open_uni_stream_on_driver`, срез 3a)
    // and reports its stream id. No write-bytes primitive exists yet (a
    // later slice), so this only proves the plumbing: session handle → live
    // driver → a real stream opened on the wire.
    let open_uni = into_v8_fn1(move |handle: i32| -> String {
        let Some(ref provider) = uni_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_open_uni_stream(handle) {
            Ok(stream_id) => format!(r#"{{"ok":true,"streamId":{stream_id}}}"#),
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_open_uni_stream", open_uni)?;

    // GAP-WEBTRANSPORT срез 4b: `createBidirectionalStream()`'s transport
    // call — opens a client-initiated QUIC bidi-stream on the session
    // `handle` names (`h3_webtransport_open_bidi_stream_on_driver`, срез 4a)
    // and reports its stream id. The write half of the resolved stream
    // reuses `_lumen_webtransport_write_stream`/`_lumen_webtransport_close_stream`/
    // `_lumen_webtransport_abort_stream` below (stream-id-generic, same as
    // the uni-stream's writable); the read half is `_lumen_webtransport_read_stream`
    // below (срез 4c) — a unidirectional stream carries no read half by
    // definition (RFC 9000 §2.1), so `createUnidirectionalStream()` needs no
    // counterpart.
    let open_bidi = into_v8_fn1(move |handle: i32| -> String {
        let Some(ref provider) = bidi_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_open_bidi_stream(handle) {
            Ok(stream_id) => format!(r#"{{"ok":true,"streamId":{stream_id}}}"#),
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_open_bidi_stream", open_bidi)?;

    // GAP-WEBTRANSPORT срез 3c: the write-bytes primitive the previous
    // slice's comment ("no write-bytes primitive exists yet") deferred —
    // writes `bytes` to the already-open `streamId` on session `handle`
    // (`webtransport_write_uni_stream`, `h3_webtransport_write_stream_on_driver`).
    // `stream_id` travels as `f64` (JS has no native u64) — every id this
    // session hands out is `4n+2` for a small `n`, always exactly
    // representable, so the round trip through `f64` loses nothing.
    let write_stream = into_v8_fn3(move |handle: i32, stream_id: f64, bytes: Vec<u8>| -> String {
        let Some(ref provider) = write_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_write_uni_stream(handle, stream_id as u64, &bytes) {
            Ok(()) => r#"{"ok":true}"#.to_string(),
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_write_stream", write_stream)?;

    // GAP-WEBTRANSPORT срез 3d: `WritableStreamDefaultWriter.close()`'s native
    // counterpart — sends a QUIC STREAM FIN on the already-open `streamId`,
    // no further `write()`/`close()`/`abort()` has any effect afterward
    // (`h3_webtransport_close_uni_stream_on_driver`).
    let close_stream = into_v8_fn2(move |handle: i32, stream_id: f64| -> String {
        let Some(ref provider) = close_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_close_uni_stream(handle, stream_id as u64) {
            Ok(()) => r#"{"ok":true}"#.to_string(),
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_close_stream", close_stream)?;

    // GAP-WEBTRANSPORT срез 3d: `WritableStreamDefaultWriter.abort(reason)`'s
    // native counterpart — sends a QUIC RESET_STREAM with `error_code` on the
    // already-open `streamId`, discarding any unsent bytes
    // (`h3_webtransport_reset_uni_stream_on_driver`). `error_code` travels as
    // `f64` masked to a `u8` in the shim before this call (RFC 9114 §8.1 caps
    // application error codes carried this way at one byte for WebTransport,
    // mirroring `WebTransportError.streamErrorCode`'s own `0xff` mask).
    let abort_stream = into_v8_fn3(move |handle: i32, stream_id: f64, error_code: f64| -> String {
        let Some(ref provider) = abort_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_abort_uni_stream(handle, stream_id as u64, error_code as u64) {
            Ok(()) => r#"{"ok":true}"#.to_string(),
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_abort_stream", abort_stream)?;

    // GAP-WEBTRANSPORT срез 4c: the read-bytes primitive
    // `WebTransportBidirectionalStream.readable`'s `pull()` polls —
    // `webtransport_read_bidi_stream`/`h3_webtransport_read_stream_on_driver`
    // drain one non-blocking sweep of the session's transport and hand back
    // whatever became readable on `streamId`. `bytes` rides as a JSON number
    // array (matching this module's other JSON-string replies) rather than a
    // second native-call shape; `finished` tells the shim's poll loop when to
    // stop calling and close the `ReadableStream` instead.
    let read_stream = into_v8_fn2(move |handle: i32, stream_id: f64| -> String {
        let Some(ref provider) = read_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_read_bidi_stream(handle, stream_id as u64) {
            Ok((bytes, finished)) => {
                let bytes_json =
                    bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
                format!(r#"{{"ok":true,"bytes":[{bytes_json}],"finished":{finished}}}"#)
            }
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_read_stream", read_stream)?;

    // GAP-WEBTRANSPORT срез 4d: `incomingUnidirectionalStreams`'s discovery
    // poll — `webtransport_poll_incoming_uni_streams`/
    // `h3_webtransport_poll_new_peer_streams_on_driver` drain one
    // non-blocking sweep of the session's transport and report every
    // peer-initiated unidirectional stream whose WebTransport header has now
    // been fully parsed and stripped, ready to read. `streamIds` rides as a
    // JSON number array, same shape as `read_stream`'s `bytes`.
    let poll_incoming_uni = into_v8_fn1(move |handle: i32| -> String {
        let Some(ref provider) = poll_incoming_uni_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_poll_incoming_uni_streams(handle) {
            Ok(ids) => {
                let ids_json = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
                format!(r#"{{"ok":true,"streamIds":[{ids_json}]}}"#)
            }
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_poll_incoming_uni_streams", poll_incoming_uni)?;

    // GAP-WEBTRANSPORT срез 4d: the read-bytes primitive for one incoming
    // unidirectional stream `_lumen_webtransport_poll_incoming_uni_streams`
    // reported ready — same `(bytes, finished)` shape as
    // `_lumen_webtransport_read_stream`, backed by
    // `webtransport_read_incoming_uni_stream`.
    let read_incoming_uni = into_v8_fn2(move |handle: i32, stream_id: f64| -> String {
        let Some(ref provider) = read_incoming_uni_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_read_incoming_uni_stream(handle, stream_id as u64) {
            Ok((bytes, finished)) => {
                let bytes_json = bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
                format!(r#"{{"ok":true,"bytes":[{bytes_json}],"finished":{finished}}}"#)
            }
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_read_incoming_uni_stream", read_incoming_uni)?;

    // GAP-WEBTRANSPORT срез 4e: `incomingBidirectionalStreams`'s discovery
    // poll — the bidi counterpart of `_lumen_webtransport_poll_incoming_uni_streams`
    // (`webtransport_poll_incoming_bidi_streams`,
    // `h3_webtransport_poll_new_peer_streams_on_driver`). A reported id
    // already has its send half registered on the Rust side, so the shim's
    // `writable` for it reuses `openUniStreamWritable` unchanged, same as
    // `createBidirectionalStream()`'s.
    let poll_incoming_bidi = into_v8_fn1(move |handle: i32| -> String {
        let Some(ref provider) = poll_incoming_bidi_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_poll_incoming_bidi_streams(handle) {
            Ok(ids) => {
                let ids_json = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
                format!(r#"{{"ok":true,"streamIds":[{ids_json}]}}"#)
            }
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_poll_incoming_bidi_streams", poll_incoming_bidi)?;

    // GAP-WEBTRANSPORT срез 4e: the read-bytes primitive for one incoming
    // bidirectional stream `_lumen_webtransport_poll_incoming_bidi_streams`
    // reported ready — same `(bytes, finished)` shape as
    // `_lumen_webtransport_read_incoming_uni_stream`, backed by
    // `webtransport_read_incoming_bidi_stream`.
    let read_incoming_bidi = into_v8_fn2(move |handle: i32, stream_id: f64| -> String {
        let Some(ref provider) = read_incoming_bidi_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_read_incoming_bidi_stream(handle, stream_id as u64) {
            Ok((bytes, finished)) => {
                let bytes_json = bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
                format!(r#"{{"ok":true,"bytes":[{bytes_json}],"finished":{finished}}}"#)
            }
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_read_incoming_bidi_stream", read_incoming_bidi)?;

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

  // GAP-WEBTRANSPORT срез 3c/3d: the writable half of a uni-stream that
  // opened on the wire (`createUnidirectionalStream()` got a real QUIC stream
  // id back) — `write(chunk)` sends `chunk`'s bytes over
  // `_lumen_webtransport_write_stream(handle, streamId, bytes)`
  // (`h3_webtransport_write_stream_on_driver`, срез 3c); `close()` sends a
  // QUIC STREAM FIN (`_lumen_webtransport_close_stream`,
  // `h3_webtransport_close_uni_stream_on_driver`, срез 3d) — the normal end of
  // a WebTransport unidirectional stream, e.g. `writer.close()`; `abort(reason)`
  // sends a QUIC RESET_STREAM (`_lumen_webtransport_abort_stream`,
  // `h3_webtransport_reset_uni_stream_on_driver`, срез 3d), discarding any
  // unsent bytes — the error code is `reason.streamErrorCode` when `reason` is
  // a `WebTransportError` (spec-shaped abort, e.g. a caller-supplied
  // `WebTransportError` reason), else `0` (an arbitrary JS value carries no
  // wire-representable code).
  function openUniStreamWritable(handle, streamId) {
    return new WritableStream({
      write: function(chunk) {
        var bytes;
        try {
          bytes = chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk);
        } catch (e) {
          return Promise.reject(new WebTransportError({
            source: 'stream',
            message: 'WebTransport stream chunks must be BufferSource.',
          }));
        }
        var result;
        try {
          result = JSON.parse(_lumen_webtransport_write_stream(handle, streamId, bytes));
        } catch (e) {
          result = { ok: false, message: 'WebTransport: malformed native response.' };
        }
        if (!result || !result.ok) {
          return Promise.reject(new WebTransportError({
            source: 'stream',
            message: (result && result.message) || 'Failed to write to a WebTransport unidirectional stream.',
          }));
        }
        return Promise.resolve();
      },
      close: function() {
        var result;
        try {
          result = JSON.parse(_lumen_webtransport_close_stream(handle, streamId));
        } catch (e) {
          result = { ok: false, message: 'WebTransport: malformed native response.' };
        }
        if (!result || !result.ok) {
          return Promise.reject(new WebTransportError({
            source: 'stream',
            message: (result && result.message) || 'Failed to close a WebTransport unidirectional stream.',
          }));
        }
        return Promise.resolve();
      },
      abort: function(reason) {
        var errorCode = (reason instanceof WebTransportError && typeof reason.streamErrorCode === 'number')
          ? reason.streamErrorCode
          : 0;
        var result;
        try {
          result = JSON.parse(_lumen_webtransport_abort_stream(handle, streamId, errorCode));
        } catch (e) {
          result = { ok: false, message: 'WebTransport: malformed native response.' };
        }
        if (!result || !result.ok) {
          return Promise.reject(new WebTransportError({
            source: 'stream',
            message: (result && result.message) || 'Failed to abort a WebTransport unidirectional stream.',
          }));
        }
        return Promise.resolve();
      },
    });
  }

  // GAP-WEBTRANSPORT срез 4c: the readable half of a bidi-stream that opened
  // on the wire — pulls bytes via `_lumen_webtransport_read_stream(handle,
  // streamId)` (`h3_webtransport_read_stream_on_driver`, a non-blocking
  // sweep of the QUIC transport every call). Each `pull()` polls in a
  // `setTimeout(0)` loop until either bytes arrive (`enqueue` once and
  // return — the stream calls `pull()` again for the next chunk), the
  // stream's receive half is finished (`close()`), or the native call itself
  // fails (`error()` with a `WebTransportError`).
  function openBidiStreamReadable(handle, streamId) {
    return new ReadableStream({
      pull: function(controller) {
        return new Promise(function(resolve) {
          function attempt() {
            var result;
            try {
              result = JSON.parse(_lumen_webtransport_read_stream(handle, streamId));
            } catch (e) {
              result = { ok: false, message: 'WebTransport: malformed native response.' };
            }
            if (!result || !result.ok) {
              controller.error(new WebTransportError({
                source: 'stream',
                message: (result && result.message) || 'Failed to read a WebTransport stream.',
              }));
              resolve();
              return;
            }
            if (result.bytes && result.bytes.length) {
              controller.enqueue(new Uint8Array(result.bytes));
              resolve();
              return;
            }
            if (result.finished) {
              controller.close();
              resolve();
              return;
            }
            setTimeout(attempt, 0);
          }
          attempt();
        });
      },
    });
  }

  // GAP-WEBTRANSPORT срез 4d: the readable half of one incoming
  // (peer-initiated) unidirectional stream — `_lumen_webtransport_poll_incoming_uni_streams`
  // has already classified `streamId` and stripped its WebTransport header;
  // this only pulls the application bytes that follow, via
  // `_lumen_webtransport_read_incoming_uni_stream`
  // (`webtransport_read_incoming_uni_stream`). Same poll-loop shape as
  // `openBidiStreamReadable` — a WebTransport incoming uni stream is
  // receive-only from our side, so unlike an incoming bidi stream this needs
  // no writable half.
  function openIncomingUniStreamReadable(handle, streamId) {
    return new ReadableStream({
      pull: function(controller) {
        return new Promise(function(resolve) {
          function attempt() {
            var result;
            try {
              result = JSON.parse(_lumen_webtransport_read_incoming_uni_stream(handle, streamId));
            } catch (e) {
              result = { ok: false, message: 'WebTransport: malformed native response.' };
            }
            if (!result || !result.ok) {
              controller.error(new WebTransportError({
                source: 'stream',
                message: (result && result.message) || 'Failed to read an incoming WebTransport stream.',
              }));
              resolve();
              return;
            }
            if (result.bytes && result.bytes.length) {
              controller.enqueue(new Uint8Array(result.bytes));
              resolve();
              return;
            }
            if (result.finished) {
              controller.close();
              resolve();
              return;
            }
            setTimeout(attempt, 0);
          }
          attempt();
        });
      },
    });
  }

  // GAP-WEBTRANSPORT срез 4e: the readable half of one incoming
  // (peer-initiated) bidirectional stream — same poll-loop shape as
  // `openIncomingUniStreamReadable`, pulling via
  // `_lumen_webtransport_read_incoming_bidi_stream`
  // (`webtransport_read_incoming_bidi_stream`). Unlike an incoming uni
  // stream, this one also gets a `writable` — the Rust side has already
  // registered the stream's send half by the time
  // `openIncomingBidirectionalStreams` reports the id, so
  // `openUniStreamWritable` (unmodified — a `SendStream` does not know its
  // own direction) works on it exactly as it does for a stream we opened
  // ourselves.
  function openIncomingBidiStreamReadable(handle, streamId) {
    return new ReadableStream({
      pull: function(controller) {
        return new Promise(function(resolve) {
          function attempt() {
            var result;
            try {
              result = JSON.parse(_lumen_webtransport_read_incoming_bidi_stream(handle, streamId));
            } catch (e) {
              result = { ok: false, message: 'WebTransport: malformed native response.' };
            }
            if (!result || !result.ok) {
              controller.error(new WebTransportError({
                source: 'stream',
                message: (result && result.message) || 'Failed to read an incoming WebTransport stream.',
              }));
              resolve();
              return;
            }
            if (result.bytes && result.bytes.length) {
              controller.enqueue(new Uint8Array(result.bytes));
              resolve();
              return;
            }
            if (result.finished) {
              controller.close();
              resolve();
              return;
            }
            setTimeout(attempt, 0);
          }
          attempt();
        });
      },
    });
  }

  // GAP-WEBTRANSPORT срез 4e: `incomingBidirectionalStreams` itself — the
  // bidi counterpart of `openIncomingUnidirectionalStreams` below, same
  // discovery-loop shape (polls `_lumen_webtransport_poll_incoming_bidi_streams`,
  // waits for a live handle, stops for good on `close()`/a failed `ready`),
  // but each discovered id is wrapped as a full `WebTransportBidirectionalStream`
  // (`openIncomingBidiStreamReadable` for `readable`, `openUniStreamWritable`
  // for `writable`) instead of a read-only `ReadableStream` — a peer-initiated
  // bidirectional stream carries a writable half back to the peer, unlike an
  // incoming unidirectional one.
  function openIncomingBidirectionalStreams(session) {
    return new ReadableStream({
      pull: function(controller) {
        return new Promise(function(resolve) {
          function attempt() {
            if (session._closed || session._readyFailed) {
              controller.close();
              resolve();
              return;
            }
            if (session._handle === null) {
              setTimeout(attempt, 0);
              return;
            }
            var result;
            try {
              result = JSON.parse(_lumen_webtransport_poll_incoming_bidi_streams(session._handle));
            } catch (e) {
              result = { ok: false, message: 'WebTransport: malformed native response.' };
            }
            if (!result || !result.ok) {
              controller.error(new WebTransportError({
                source: 'session',
                message: (result && result.message) || 'Failed to poll incoming WebTransport streams.',
              }));
              resolve();
              return;
            }
            if (result.streamIds && result.streamIds.length) {
              for (var i = 0; i < result.streamIds.length; i++) {
                controller.enqueue(new WebTransportBidirectionalStream(
                  openIncomingBidiStreamReadable(session._handle, result.streamIds[i]),
                  openUniStreamWritable(session._handle, result.streamIds[i])
                ));
              }
              resolve();
              return;
            }
            setTimeout(attempt, 0);
          }
          attempt();
        });
      },
    });
  }

  // GAP-WEBTRANSPORT срез 4d: `incomingUnidirectionalStreams` itself — a
  // `ReadableStream` of `ReadableStream`s, one per peer-initiated
  // unidirectional stream discovered on `session`. Polls
  // `_lumen_webtransport_poll_incoming_uni_streams(handle)` in the same
  // `setTimeout(0)` loop shape as `openBidiStreamReadable`, but before the
  // session has a live `handle` (constructor still running, or `ready`
  // rejected) it just waits rather than calling the native — nothing can
  // have arrived on a session that never connected. `session._readyFailed`
  // (set by the constructor's `ready` rejection branch) stops the loop for
  // good instead of polling forever on a session that will never open.
  function openIncomingUnidirectionalStreams(session) {
    return new ReadableStream({
      pull: function(controller) {
        return new Promise(function(resolve) {
          function attempt() {
            if (session._closed || session._readyFailed) {
              controller.close();
              resolve();
              return;
            }
            if (session._handle === null) {
              setTimeout(attempt, 0);
              return;
            }
            var result;
            try {
              result = JSON.parse(_lumen_webtransport_poll_incoming_uni_streams(session._handle));
            } catch (e) {
              result = { ok: false, message: 'WebTransport: malformed native response.' };
            }
            if (!result || !result.ok) {
              controller.error(new WebTransportError({
                source: 'session',
                message: (result && result.message) || 'Failed to poll incoming WebTransport streams.',
              }));
              resolve();
              return;
            }
            if (result.streamIds && result.streamIds.length) {
              for (var i = 0; i < result.streamIds.length; i++) {
                controller.enqueue(openIncomingUniStreamReadable(session._handle, result.streamIds[i]));
              }
              resolve();
              return;
            }
            setTimeout(attempt, 0);
          }
          attempt();
        });
      },
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
    this._closed = false;
    this._handle = null;
    this._readyFailed = false;
    // GAP-WEBTRANSPORT срез 4d/4e: real discovery streams — both read
    // `this._handle`/`_closed`/`_readyFailed` lazily on each `pull()`, so it
    // is safe to construct them before those settle below.
    this._incomingBidi = openIncomingBidirectionalStreams(this);
    this._incomingUnidi = openIncomingUnidirectionalStreams(this);

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

    // GAP-WEBTRANSPORT срез 3b: `_lumen_webtransport_open` now also reports
    // the session `handle` (`{ok:true,status,handle}` on a 2xx response,
    // `{ok:false,message}` otherwise, including "not supported in this
    // context", the old always-fail answer) — stashed on `self._handle` so
    // `createUnidirectionalStream()` can address the live session. Datagrams
    // and incoming streams stay stubs until срез 4/lifecycle срез 5;
    // `closed` is deliberately left pending on success — no lifecycle wiring
    // exists yet to ever settle it.
    setTimeout(function() {
      var result;
      try {
        result = JSON.parse(_lumen_webtransport_open(self._url));
      } catch (e) {
        result = { ok: false, message: 'WebTransport: malformed native response.' };
      }
      if (result && result.ok) {
        self._handle = result.handle;
        self._readyResolve(undefined);
      } else {
        self._readyFailed = true;
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

  // GAP-WEBTRANSPORT срез 4b/4c: same "no live handle → reject synchronously"
  // shape as `createUnidirectionalStream()` (срез 3b); otherwise opens a
  // real QUIC bidi-stream on it (`_lumen_webtransport_open_bidi_stream`,
  // срез 4a's transport primitive) and resolves a
  // `WebTransportBidirectionalStream` whose `writable` reuses
  // `openUniStreamWritable` (a `SendStream`'s write/close/abort do not know
  // their own direction) and whose `readable` polls the peer's bytes back
  // through `openBidiStreamReadable` (срез 4c).
  WebTransport.prototype.createBidirectionalStream = function() {
    if (this._handle === null) {
      return Promise.reject(notConnectedError());
    }
    var result;
    try {
      result = JSON.parse(_lumen_webtransport_open_bidi_stream(this._handle));
    } catch (e) {
      result = { ok: false, message: 'WebTransport: malformed native response.' };
    }
    if (!result || !result.ok) {
      return Promise.reject(new WebTransportError({
        source: 'stream',
        message: (result && result.message) || 'Failed to open a WebTransport bidirectional stream.',
      }));
    }
    return Promise.resolve(new WebTransportBidirectionalStream(
      openBidiStreamReadable(this._handle, result.streamId),
      openUniStreamWritable(this._handle, result.streamId)
    ));
  };
  // GAP-WEBTRANSPORT срез 3b/3c: rejects synchronously (session not `ready`
  // yet or `ready` failed, same as `createBidirectionalStream()`'s
  // unconditional reject before срез 3b) when there is no live handle;
  // otherwise opens a real QUIC uni-stream on it
  // (`_lumen_webtransport_open_uni_stream`, срез 3a's transport primitive)
  // and resolves a `WritableStream` whose `write()` sends bytes over that
  // stream (`openUniStreamWritable`, срез 3c).
  WebTransport.prototype.createUnidirectionalStream = function() {
    if (this._handle === null) {
      return Promise.reject(notConnectedError());
    }
    var result;
    try {
      result = JSON.parse(_lumen_webtransport_open_uni_stream(this._handle));
    } catch (e) {
      result = { ok: false, message: 'WebTransport: malformed native response.' };
    }
    if (!result || !result.ok) {
      return Promise.reject(new WebTransportError({
        source: 'stream',
        message: (result && result.message) || 'Failed to open a WebTransport unidirectional stream.',
      }));
    }
    return Promise.resolve(openUniStreamWritable(this._handle, result.streamId));
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

    /// Queued answers [`StubFetch::webtransport_read_bidi_stream`] hands back
    /// in order, one per call.
    type ReadStreamResults = std::sync::Mutex<
        std::collections::VecDeque<lumen_core::error::Result<(Vec<u8>, bool)>>,
    >;

    /// A fetch provider whose `webtransport_connect` answers deterministically
    /// (`Ok`/`Err`) instead of network I/O — GAP-WEBTRANSPORT срез 2b. Its
    /// `webtransport_open_uni_stream` (срез 3b) and `webtransport_write_uni_stream`
    /// (срез 3c) are each a fixed result rather than driving a live handle.
    struct StubFetch {
        result: std::sync::Mutex<Option<lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>>>,
        uni_stream_result: lumen_core::error::Result<u64>,
        /// GAP-WEBTRANSPORT срез 4b: same shape as `uni_stream_result`, for
        /// `webtransport_open_bidi_stream`.
        bidi_stream_result: lumen_core::error::Result<u64>,
        write_stream_result: lumen_core::error::Result<()>,
        /// The `(handle, stream_id, bytes)` triple the last `write` call
        /// received, if any — lets a test assert the JS layer forwarded the
        /// right stream id and payload, not just that it resolved.
        last_write: std::sync::Mutex<Option<(i32, u64, Vec<u8>)>>,
        close_stream_result: lumen_core::error::Result<()>,
        abort_stream_result: lumen_core::error::Result<()>,
        /// The `(handle, stream_id)` pair the last `close` call received, if
        /// any — GAP-WEBTRANSPORT срез 3d.
        last_close: std::sync::Mutex<Option<(i32, u64)>>,
        /// The `(handle, stream_id, error_code)` triple the last `abort` call
        /// received, if any — GAP-WEBTRANSPORT срез 3d.
        last_abort: std::sync::Mutex<Option<(i32, u64, u64)>>,
        /// GAP-WEBTRANSPORT срез 4c: the queued answers
        /// `webtransport_read_bidi_stream` hands back in order, one per call
        /// (`pop_front`) — lets a test script a multi-poll sequence (bytes
        /// then finished, or an error). Empty defaults to `Ok((vec![], true))`
        /// so a test that never touches `readable` still terminates the
        /// shim's poll loop instead of spinning.
        read_stream_results: ReadStreamResults,
        /// GAP-WEBTRANSPORT срез 4d: same queued-answer shape as
        /// `read_stream_results`, for `webtransport_poll_incoming_uni_streams`.
        /// Empty defaults to `Ok(vec![])` so a test that never touches
        /// `incomingUnidirectionalStreams` still terminates the shim's
        /// discovery loop instead of spinning.
        poll_incoming_uni_results:
            std::sync::Mutex<std::collections::VecDeque<lumen_core::error::Result<Vec<u64>>>>,
        /// GAP-WEBTRANSPORT срез 4d: same queued-answer shape as
        /// `read_stream_results`, for `webtransport_read_incoming_uni_stream`.
        read_incoming_uni_results: ReadStreamResults,
        /// GAP-WEBTRANSPORT срез 4e: same queued-answer shape as
        /// `poll_incoming_uni_results`, for `webtransport_poll_incoming_bidi_streams`.
        poll_incoming_bidi_results:
            std::sync::Mutex<std::collections::VecDeque<lumen_core::error::Result<Vec<u64>>>>,
        /// GAP-WEBTRANSPORT срез 4e: same queued-answer shape as
        /// `read_stream_results`, for `webtransport_read_incoming_bidi_stream`.
        read_incoming_bidi_results: ReadStreamResults,
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
        fn webtransport_open_uni_stream(&self, _handle: i32) -> lumen_core::error::Result<u64> {
            match &self.uni_stream_result {
                Ok(id) => Ok(*id),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_open_bidi_stream(&self, _handle: i32) -> lumen_core::error::Result<u64> {
            match &self.bidi_stream_result {
                Ok(id) => Ok(*id),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_write_uni_stream(
            &self,
            handle: i32,
            stream_id: u64,
            data: &[u8],
        ) -> lumen_core::error::Result<()> {
            *self.last_write.lock().unwrap() = Some((handle, stream_id, data.to_vec()));
            match &self.write_stream_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_close_uni_stream(&self, handle: i32, stream_id: u64) -> lumen_core::error::Result<()> {
            *self.last_close.lock().unwrap() = Some((handle, stream_id));
            match &self.close_stream_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_abort_uni_stream(
            &self,
            handle: i32,
            stream_id: u64,
            error_code: u64,
        ) -> lumen_core::error::Result<()> {
            *self.last_abort.lock().unwrap() = Some((handle, stream_id, error_code));
            match &self.abort_stream_result {
                Ok(()) => Ok(()),
                Err(e) => Err(lumen_core::error::Error::Network(e.to_string())),
            }
        }
        fn webtransport_read_bidi_stream(
            &self,
            _handle: i32,
            _stream_id: u64,
        ) -> lumen_core::error::Result<(Vec<u8>, bool)> {
            match self.read_stream_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok((Vec::new(), true)),
            }
        }
        fn webtransport_poll_incoming_uni_streams(&self, _handle: i32) -> lumen_core::error::Result<Vec<u64>> {
            match self.poll_incoming_uni_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok(Vec::new()),
            }
        }
        fn webtransport_read_incoming_uni_stream(
            &self,
            _handle: i32,
            _stream_id: u64,
        ) -> lumen_core::error::Result<(Vec<u8>, bool)> {
            match self.read_incoming_uni_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok((Vec::new(), true)),
            }
        }
        fn webtransport_poll_incoming_bidi_streams(&self, _handle: i32) -> lumen_core::error::Result<Vec<u64>> {
            match self.poll_incoming_bidi_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok(Vec::new()),
            }
        }
        fn webtransport_read_incoming_bidi_stream(
            &self,
            _handle: i32,
            _stream_id: u64,
        ) -> lumen_core::error::Result<(Vec<u8>, bool)> {
            match self.read_incoming_bidi_results.lock().unwrap().pop_front() {
                Some(r) => r,
                None => Ok((Vec::new(), true)),
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
        rt_with_webtransport_provider_and_uni_result(result, Ok(7))
    }

    /// Like [`rt_with_webtransport_provider`], but also controls what
    /// `webtransport_open_uni_stream` answers — GAP-WEBTRANSPORT срез 3b.
    fn rt_with_webtransport_provider_and_uni_result(
        result: lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>,
        uni_stream_result: lumen_core::error::Result<u64>,
    ) -> V8JsRuntime {
        rt_with_webtransport_provider_full(result, uni_stream_result, Ok(())).0
    }

    /// Like [`rt_with_webtransport_provider_and_uni_result`], but also
    /// controls what `webtransport_write_uni_stream` answers and returns the
    /// `Arc<Mutex<...>>` the test can inspect for the last write's
    /// `(handle, stream_id, bytes)` — GAP-WEBTRANSPORT срез 3c.
    fn rt_with_webtransport_provider_full(
        result: lumen_core::error::Result<lumen_core::ext::JsWebTransportSession>,
        uni_stream_result: lumen_core::error::Result<u64>,
        write_stream_result: lumen_core::error::Result<()>,
    ) -> (V8JsRuntime, Arc<StubFetch>) {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        let stub = Arc::new(StubFetch {
            result: std::sync::Mutex::new(Some(result)),
            uni_stream_result,
            bidi_stream_result: Ok(9),
            write_stream_result,
            last_write: std::sync::Mutex::new(None),
            close_stream_result: Ok(()),
            abort_stream_result: Ok(()),
            last_close: std::sync::Mutex::new(None),
            last_abort: std::sync::Mutex::new(None),
            read_stream_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
        });
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = stub.clone();
        rt.install_dom(doc, "", Some(provider), None, None, None, None, None, None, None, false)
            .unwrap();
        (rt, stub)
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

    /// GAP-WEBTRANSPORT срез 3b: same shape as
    /// `create_streams_reject_with_web_transport_error_before_ready` for the
    /// other stream constructor — `createUnidirectionalStream()` now takes a
    /// different code path (checks `self._handle` instead of unconditionally
    /// rejecting), so it needs its own coverage that the no-handle-yet case
    /// still rejects synchronously with the same `WebTransportError` shape.
    #[test]
    fn create_unidirectional_stream_rejects_before_ready() {
        let rt = rt_with_webtransport();
        rt.eval(
            "globalThis._wtTestResult = false; \
            (function() { \
                var wt = new WebTransport('https://example.com/wt'); \
                wt.createUnidirectionalStream().catch(function(e) { \
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

    /// GAP-WEBTRANSPORT срез 3b: the JSON `_lumen_webtransport_open` reports
    /// on success now also carries the session `handle`, so
    /// `createUnidirectionalStream()` can address it later.
    #[test]
    fn native_open_reports_handle_on_success() {
        let rt = rt_with_webtransport_provider(Ok(lumen_core::ext::JsWebTransportSession {
            handle: 42,
            status: 200,
        }));
        let r = rt.eval("_lumen_webtransport_open('https://example.com/wt')").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""handle":42"#), "expected handle 42, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 3b: direct native-call coverage for
    /// `_lumen_webtransport_open_uni_stream`, same "no `setTimeout` pumping
    /// in this harness" workaround as the `_lumen_webtransport_open` tests
    /// above.
    #[test]
    fn native_open_uni_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_open_uni_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_uni_stream_reports_stream_id_on_success() {
        let rt = rt_with_webtransport_provider_and_uni_result(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(6),
        );
        let r = rt.eval("_lumen_webtransport_open_uni_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamId":6"#), "expected streamId 6, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_uni_stream_reports_provider_error_message() {
        let rt = rt_with_webtransport_provider_and_uni_result(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Err(lumen_core::error::Error::Network("WebTransport session not found".to_string())),
        );
        let r = rt.eval("_lumen_webtransport_open_uni_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
                assert!(s.contains("session not found"), "expected the message, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 3c: direct native-call coverage for
    /// `_lumen_webtransport_write_stream`, same "no `setTimeout` pumping in
    /// this harness" workaround as the other native-call tests above.
    #[test]
    fn native_write_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_write_stream(0, 2, new Uint8Array([1]))").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_write_stream_forwards_handle_stream_id_and_bytes() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 5, status: 200 }),
            Ok(2),
            Ok(()),
        );
        let r = rt.eval("_lumen_webtransport_write_stream(5, 2, new Uint8Array([104, 105]))").unwrap();
        match r {
            lumen_core::JsValue::String(s) => assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}"),
            other => panic!("expected a String, got {other:?}"),
        }
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (5, 2, vec![104, 105]));
    }

    #[test]
    fn native_write_stream_reports_provider_error_message() {
        let (rt, _stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(2),
            Err(lumen_core::error::Error::Network("WebTransport session not found".to_string())),
        );
        let r = rt.eval("_lumen_webtransport_write_stream(0, 2, new Uint8Array([1]))").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
                assert!(s.contains("session not found"), "expected the message, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `createUnidirectionalStream()` resolves a real
    /// `WritableStream` whose `write()` reaches
    /// `_lumen_webtransport_write_stream` with the session's handle and the
    /// stream id `createUnidirectionalStream()` itself opened — proves the
    /// two natives compose through the shim, not just each in isolation.
    #[test]
    fn create_unidirectional_stream_write_reaches_the_native_with_the_right_ids() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtWriteOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                var writer = stream.getWriter(); \
                return writer.write(new Uint8Array([1, 2, 3])); \
            }).then(function() { \
                globalThis._wtWriteOk = true; \
            });",
        )
        .unwrap();
        // `ready` resolves from a `setTimeout(0)` callback (the constructor's
        // native call, срез 3b) — this harness needs an explicit timer pump
        // (`_lumen_tick_timers`, see `internal_globals.rs`'s
        // `engine_state_stays_writable`) to run it before the promise chain
        // above can proceed.
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtWriteOk");
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (3, 2, vec![1, 2, 3]));
    }

    #[test]
    fn create_unidirectional_stream_write_rejects_on_provider_error() {
        let (rt, _stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(2),
            Err(lumen_core::error::Error::Network("boom".to_string())),
        );
        rt.eval(
            "globalThis._wtWriteRejected = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                var writer = stream.getWriter(); \
                return writer.write(new Uint8Array([1])); \
            }).catch(function(e) { \
                globalThis._wtWriteRejected = (e instanceof WebTransportError) && e.source === 'stream'; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtWriteRejected");
    }

    /// GAP-WEBTRANSPORT срез 3d: direct native-call coverage for
    /// `_lumen_webtransport_close_stream`, same "no `setTimeout` pumping in
    /// this harness" workaround as the other native-call tests above.
    #[test]
    fn native_close_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_close_stream(0, 2)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_abort_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_abort_stream(0, 2, 5)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `createUnidirectionalStream()` resolves a `WritableStream`
    /// whose `getWriter().close()` reaches `_lumen_webtransport_close_stream`
    /// with the session's handle and the stream id
    /// `createUnidirectionalStream()` itself opened.
    #[test]
    fn create_unidirectional_stream_close_reaches_the_native_with_the_right_ids() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtCloseOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().close(); \
            }).then(function() { \
                globalThis._wtCloseOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtCloseOk");
        let last = *stub.last_close.lock().unwrap();
        assert_eq!(last, Some((3, 2)));
    }

    #[test]
    fn create_unidirectional_stream_close_rejects_on_provider_error() {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        let stub = Arc::new(StubFetch {
            result: std::sync::Mutex::new(Some(Ok(lumen_core::ext::JsWebTransportSession {
                handle: 0,
                status: 200,
            }))),
            uni_stream_result: Ok(2),
            bidi_stream_result: Ok(9),
            write_stream_result: Ok(()),
            last_write: std::sync::Mutex::new(None),
            close_stream_result: Err(lumen_core::error::Error::Network("boom".to_string())),
            abort_stream_result: Ok(()),
            last_close: std::sync::Mutex::new(None),
            last_abort: std::sync::Mutex::new(None),
            read_stream_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
        });
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = stub.clone();
        rt.install_dom(doc, "", Some(provider), None, None, None, None, None, None, None, false)
            .unwrap();
        rt.eval(
            "globalThis._wtCloseRejected = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().close(); \
            }).catch(function(e) { \
                globalThis._wtCloseRejected = (e instanceof WebTransportError) && e.source === 'stream'; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtCloseRejected");
    }

    /// End-to-end: `createUnidirectionalStream()` resolves a `WritableStream`
    /// whose `getWriter().abort(reason)` reaches
    /// `_lumen_webtransport_abort_stream` with the session's handle, the
    /// stream id, and the `reason`'s `streamErrorCode` when `reason` is a
    /// `WebTransportError`.
    #[test]
    fn create_unidirectional_stream_abort_forwards_the_stream_error_code() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtAbortOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().abort(new WebTransportError({ source: 'stream', streamErrorCode: 9 })); \
            }).then(function() { \
                globalThis._wtAbortOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtAbortOk");
        let last = *stub.last_abort.lock().unwrap();
        assert_eq!(last, Some((3, 2, 9)));
    }

    #[test]
    fn create_unidirectional_stream_abort_defaults_to_zero_for_a_plain_reason() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtAbortOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createUnidirectionalStream(); \
            }).then(function(stream) { \
                return stream.getWriter().abort('some reason'); \
            }).then(function() { \
                globalThis._wtAbortOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtAbortOk");
        let last = *stub.last_abort.lock().unwrap();
        assert_eq!(last, Some((3, 2, 0)));
    }

    /// GAP-WEBTRANSPORT срез 4b: direct native-call coverage for
    /// `_lumen_webtransport_open_bidi_stream`, same "no `setTimeout` pumping
    /// in this harness" workaround as the uni-stream native-call tests.
    #[test]
    fn native_open_bidi_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_open_bidi_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_open_bidi_stream_reports_stream_id_on_success() {
        let (rt, _stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 0, status: 200 }),
            Ok(2),
            Ok(()),
        );
        let r = rt.eval("_lumen_webtransport_open_bidi_stream(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamId":9"#), "expected streamId 9, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// `createBidirectionalStream()` rejects synchronously with the same
    /// "no live handle yet" shape as `createUnidirectionalStream()`
    /// (`create_unidirectional_stream_rejects_before_ready`).
    #[test]
    fn create_bidirectional_stream_rejects_before_ready() {
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

    /// End-to-end: `createBidirectionalStream()` resolves a real
    /// `WebTransportBidirectionalStream` whose `writable.write()` reaches
    /// `_lumen_webtransport_write_stream` with the session's handle and the
    /// stream id `createBidirectionalStream()` itself opened — same
    /// composition proof as `create_unidirectional_stream_write_reaches_the_native_with_the_right_ids`,
    /// for the bidi transport primitive (срез 4a/4b).
    #[test]
    fn create_bidirectional_stream_write_reaches_the_native_with_the_right_ids() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        rt.eval(
            "globalThis._wtWriteOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createBidirectionalStream(); \
            }).then(function(stream) { \
                return (stream instanceof WebTransportBidirectionalStream) && \
                    stream.readable instanceof ReadableStream ? \
                    stream.writable.getWriter().write(new Uint8Array([1, 2, 3])) : \
                    Promise.reject(new Error('not a WebTransportBidirectionalStream')); \
            }).then(function() { \
                globalThis._wtWriteOk = true; \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtWriteOk");
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (3, 9, vec![1, 2, 3]));
    }

    /// GAP-WEBTRANSPORT срез 4c: direct native-call coverage for
    /// `_lumen_webtransport_read_stream`, same "no provider → unsupported"
    /// shape as every other WebTransport native.
    #[test]
    fn native_read_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_read_stream(0, 0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 4d: direct native-call coverage for
    /// `_lumen_webtransport_poll_incoming_uni_streams`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_poll_incoming_uni_streams_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_poll_incoming_uni_streams(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_poll_incoming_uni_streams_reports_stream_ids_on_success() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_uni_results.lock().unwrap().push_back(Ok(vec![3, 7]));
        let r = rt.eval("_lumen_webtransport_poll_incoming_uni_streams(3)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamIds":[3,7]"#), "expected streamIds [3,7], got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 4d: direct native-call coverage for
    /// `_lumen_webtransport_read_incoming_uni_stream`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_read_incoming_uni_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_read_incoming_uni_stream(0, 3)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `incomingUnidirectionalStreams` yields a nested
    /// `ReadableStream` once `_lumen_webtransport_poll_incoming_uni_streams`
    /// reports a discovered id, and that nested stream's bytes come from
    /// `_lumen_webtransport_read_incoming_uni_stream` — proves the discovery
    /// loop and the per-stream read loop compose, not just each in
    /// isolation.
    #[test]
    fn incoming_unidirectional_streams_yields_a_readable_then_bytes() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_uni_results.lock().unwrap().push_back(Ok(vec![9]));
        {
            let mut reads = stub.read_incoming_uni_results.lock().unwrap();
            reads.push_back(Ok((vec![42u8, 43, 44], false)));
            reads.push_back(Ok((Vec::new(), true)));
        }
        rt.eval(
            "globalThis._wtIncomingLen = -1; globalThis._wtIncomingDone = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                var reader = globalThis._wt.incomingUnidirectionalStreams.getReader(); \
                return reader.read(); \
            }).then(function(res) { \
                var innerReader = res.value.getReader(); \
                return innerReader.read().then(function(res1) { \
                    globalThis._wtIncomingLen = res1.value ? res1.value.length : -1; \
                    return innerReader.read(); \
                }).then(function(res2) { \
                    globalThis._wtIncomingDone = res2.done === true; \
                }); \
            });",
        )
        .unwrap();
        // Unlike `createBidirectionalStream()`'s readable (one tick: the
        // stream already exists by the time `.then()` reads it), this chain
        // has one more asynchronous hop — the session's `ready` resolution
        // and `incomingUnidirectionalStreams`'s first `pull()` do not
        // necessarily settle within the same tick's microtask cascade — so
        // it needs a few more calls, same as production driving this via
        // the shell's per-frame `_lumen_tick_timers()`.
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(&rt, "_wtIncomingLen === 3");
        check(&rt, "_wtIncomingDone");
    }

    /// GAP-WEBTRANSPORT срез 4e: direct native-call coverage for
    /// `_lumen_webtransport_poll_incoming_bidi_streams`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_poll_incoming_bidi_streams_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_poll_incoming_bidi_streams(0)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    #[test]
    fn native_poll_incoming_bidi_streams_reports_stream_ids_on_success() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_bidi_results.lock().unwrap().push_back(Ok(vec![5, 13]));
        let r = rt.eval("_lumen_webtransport_poll_incoming_bidi_streams(3)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":true"#), "expected ok:true, got {s}");
                assert!(s.contains(r#""streamIds":[5,13]"#), "expected streamIds [5,13], got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// GAP-WEBTRANSPORT срез 4e: direct native-call coverage for
    /// `_lumen_webtransport_read_incoming_bidi_stream`, same "no provider"
    /// shape as the other native-call tests above.
    #[test]
    fn native_read_incoming_bidi_stream_reports_unsupported_with_no_provider() {
        let rt = rt_with_webtransport();
        let r = rt.eval("_lumen_webtransport_read_incoming_bidi_stream(0, 5)").unwrap();
        match r {
            lumen_core::JsValue::String(s) => {
                assert!(s.contains(r#""ok":false"#), "expected ok:false, got {s}");
            }
            other => panic!("expected a String, got {other:?}"),
        }
    }

    /// End-to-end: `incomingBidirectionalStreams` yields a full
    /// `WebTransportBidirectionalStream` once
    /// `_lumen_webtransport_poll_incoming_bidi_streams` reports a discovered
    /// id — its `readable` pulls bytes through
    /// `_lumen_webtransport_read_incoming_bidi_stream` (same discovery+read
    /// composition proof as `incoming_unidirectional_streams_yields_a_readable_then_bytes`)
    /// and its `writable` reaches the ordinary `_lumen_webtransport_write_stream`
    /// unchanged — proving the send half the Rust side pre-registers on
    /// discovery is actually usable from JS, not just present on the Rust
    /// side.
    #[test]
    fn incoming_bidirectional_streams_yields_a_stream_readable_and_writable() {
        let (rt, stub) = rt_with_webtransport_provider_full(
            Ok(lumen_core::ext::JsWebTransportSession { handle: 3, status: 200 }),
            Ok(2),
            Ok(()),
        );
        stub.poll_incoming_bidi_results.lock().unwrap().push_back(Ok(vec![13]));
        {
            let mut reads = stub.read_incoming_bidi_results.lock().unwrap();
            reads.push_back(Ok((vec![7u8, 8, 9], false)));
            reads.push_back(Ok((Vec::new(), true)));
        }
        rt.eval(
            "globalThis._wtIncomingLen = -1; globalThis._wtIncomingDone = false; \
            globalThis._wtIncomingWriteOk = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                var reader = globalThis._wt.incomingBidirectionalStreams.getReader(); \
                return reader.read(); \
            }).then(function(res) { \
                var stream = res.value; \
                var writePromise = (stream instanceof WebTransportBidirectionalStream) ? \
                    stream.writable.getWriter().write(new Uint8Array([1, 2, 3])) : \
                    Promise.reject(new Error('not a WebTransportBidirectionalStream')); \
                var innerReader = stream.readable.getReader(); \
                var readPromise = innerReader.read().then(function(res1) { \
                    globalThis._wtIncomingLen = res1.value ? res1.value.length : -1; \
                    return innerReader.read(); \
                }).then(function(res2) { \
                    globalThis._wtIncomingDone = res2.done === true; \
                }); \
                return Promise.all([writePromise, readPromise]); \
            }).then(function() { \
                globalThis._wtIncomingWriteOk = true; \
            });",
        )
        .unwrap();
        for _ in 0..3 {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        check(&rt, "_wtIncomingLen === 3");
        check(&rt, "_wtIncomingDone");
        check(&rt, "_wtIncomingWriteOk");
        let last = stub.last_write.lock().unwrap().clone().expect("write was recorded");
        assert_eq!(last, (3, 13, vec![1, 2, 3]));
    }

    /// End-to-end: `createBidirectionalStream()`'s `readable` polls
    /// `_lumen_webtransport_read_stream` — a first poll answering bytes with
    /// `finished:false` yields one chunk, and a second answering
    /// `finished:true` with no bytes closes the stream (`reader.read()`'s
    /// `done`), without a `setTimeout` retry needed for either since
    /// `pull()` calls the native synchronously before ever queuing a timer.
    #[test]
    fn create_bidirectional_stream_readable_yields_bytes_then_closes() {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        let mut reads = std::collections::VecDeque::new();
        reads.push_back(Ok((vec![1u8, 2, 3], false)));
        reads.push_back(Ok((Vec::new(), true)));
        let stub = Arc::new(StubFetch {
            result: std::sync::Mutex::new(Some(Ok(lumen_core::ext::JsWebTransportSession {
                handle: 3,
                status: 200,
            }))),
            uni_stream_result: Ok(2),
            bidi_stream_result: Ok(9),
            write_stream_result: Ok(()),
            last_write: std::sync::Mutex::new(None),
            close_stream_result: Ok(()),
            abort_stream_result: Ok(()),
            last_close: std::sync::Mutex::new(None),
            last_abort: std::sync::Mutex::new(None),
            read_stream_results: std::sync::Mutex::new(reads),
            poll_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_uni_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            poll_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
            read_incoming_bidi_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
        });
        let provider: Arc<dyn lumen_core::ext::JsFetchProvider> = stub.clone();
        rt.install_dom(doc, "", Some(provider), None, None, None, None, None, None, None, false)
            .unwrap();
        rt.eval(
            "globalThis._wtFirstLen = -1; globalThis._wtDone = false; \
            globalThis._wt = new WebTransport('https://example.com/wt'); \
            globalThis._wt.ready.then(function() { \
                return globalThis._wt.createBidirectionalStream(); \
            }).then(function(stream) { \
                var reader = stream.readable.getReader(); \
                return reader.read().then(function(res1) { \
                    globalThis._wtFirstLen = res1.value ? res1.value.length : -1; \
                    return reader.read(); \
                }).then(function(res2) { \
                    globalThis._wtDone = res2.done === true; \
                }); \
            });",
        )
        .unwrap();
        rt.eval("_lumen_tick_timers()").unwrap();
        check(&rt, "_wtFirstLen === 3");
        check(&rt, "_wtDone");
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
