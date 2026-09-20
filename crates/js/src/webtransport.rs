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
    let send_datagram_fetch_provider = fetch_provider.clone();
    let poll_incoming_datagrams_fetch_provider = fetch_provider.clone();

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

    // GAP-WEBTRANSPORT срез datagrams-b: `WebTransportDatagramDuplexStream.writable`'s
    // native call — sends `bytes` as a QUIC DATAGRAM (RFC 9221) on session
    // `handle` (`webtransport_send_datagram`, `h3_webtransport_send_datagram_on_driver`).
    let send_datagram = into_v8_fn2(move |handle: i32, bytes: Vec<u8>| -> String {
        let Some(ref provider) = send_datagram_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_send_datagram(handle, &bytes) {
            Ok(()) => r#"{"ok":true}"#.to_string(),
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_send_datagram", send_datagram)?;

    // GAP-WEBTRANSPORT срез datagrams-b: `WebTransportDatagramDuplexStream.readable`'s
    // non-blocking poll — drains every QUIC DATAGRAM already queued on
    // session `handle`'s socket (`webtransport_poll_incoming_datagrams`,
    // `h3_webtransport_poll_incoming_datagrams_on_driver`) and reports each
    // one's payload. `datagrams` rides as a JSON array of number arrays (one
    // per datagram), the same per-byte-array shape `bytes` uses elsewhere in
    // this module, just one level deeper since more than one datagram can
    // arrive in a single poll.
    let poll_incoming_datagrams = into_v8_fn1(move |handle: i32| -> String {
        let Some(ref provider) = poll_incoming_datagrams_fetch_provider else {
            return r#"{"ok":false,"message":"WebTransport is not supported in this context."}"#
                .to_string();
        };
        match provider.webtransport_poll_incoming_datagrams(handle) {
            Ok(datagrams) => {
                let datagrams_json = datagrams
                    .iter()
                    .map(|dg| {
                        let bytes_json = dg.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
                        format!("[{bytes_json}]")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"ok":true,"datagrams":[{datagrams_json}]}}"#)
            }
            Err(e) => {
                let message = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"ok":false,"message":"{message}"}}"#)
            }
        }
    });
    rt.register_native("_lumen_webtransport_poll_incoming_datagrams", poll_incoming_datagrams)?;

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

  // GAP-WEBTRANSPORT срез datagrams-b: `datagrams.writable`'s underlying
  // sink — waits for `session`'s live handle (same "reject synchronously
  // before `ready`" contract as `createUnidirectionalStream()`, just
  // deferred into `write()` itself, since `WritableStream` is constructed
  // eagerly in the `WebTransport` constructor, before any handle exists),
  // then sends `chunk`'s bytes as one QUIC DATAGRAM (RFC 9221) over
  // `_lumen_webtransport_send_datagram` (`h3_webtransport_send_datagram_on_driver`).
  // Unlike a stream's writable there is no `close`/`abort` to speak of — a
  // datagram carries no connection state of its own to tear down.
  function openDatagramWritable(session) {
    return new WritableStream({
      write: function(chunk) {
        if (session._handle === null) {
          return Promise.reject(notConnectedError());
        }
        var bytes;
        try {
          bytes = chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk);
        } catch (e) {
          return Promise.reject(new WebTransportError({
            source: 'stream',
            message: 'WebTransport datagrams must be BufferSource.',
          }));
        }
        var result;
        try {
          result = JSON.parse(_lumen_webtransport_send_datagram(session._handle, bytes));
        } catch (e) {
          result = { ok: false, message: 'WebTransport: malformed native response.' };
        }
        if (!result || !result.ok) {
          return Promise.reject(new WebTransportError({
            source: 'stream',
            message: (result && result.message) || 'Failed to send a WebTransport datagram.',
          }));
        }
        return Promise.resolve();
      },
    });
  }

  // GAP-WEBTRANSPORT срез datagrams-b: `datagrams.readable` — same
  // discovery-loop shape as `openIncomingUnidirectionalStreams` (waits for a
  // live handle, stops for good on `close()`/a failed `ready`), polling
  // `_lumen_webtransport_poll_incoming_datagrams` (`h3_webtransport_poll_incoming_datagrams_on_driver`)
  // instead of a stream-discovery native. A poll answer can carry more than
  // one datagram at once (`result.datagrams`, a JSON array of number
  // arrays); each becomes its own `Uint8Array` chunk, same "keep the
  // contents whole, one `enqueue` per unit" shape a WebTransport datagram
  // demands (RFC 9221 datagrams are delivered whole or not at all, unlike a
  // stream's byte-oriented `bytes`/`finished` pair).
  function openDatagramReadable(session) {
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
              result = JSON.parse(_lumen_webtransport_poll_incoming_datagrams(session._handle));
            } catch (e) {
              result = { ok: false, message: 'WebTransport: malformed native response.' };
            }
            if (!result || !result.ok) {
              controller.error(new WebTransportError({
                source: 'session',
                message: (result && result.message) || 'Failed to poll incoming WebTransport datagrams.',
              }));
              resolve();
              return;
            }
            if (result.datagrams && result.datagrams.length) {
              for (var i = 0; i < result.datagrams.length; i++) {
                controller.enqueue(new Uint8Array(result.datagrams[i]));
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
  // GAP-WEBTRANSPORT срез datagrams-b: `readable`/`writable` are now real,
  // session-bound streams (`openDatagramReadable`/`openDatagramWritable`) —
  // both read `session._handle`/`_closed`/`_readyFailed` lazily, so
  // constructing them before those settle (same ordering `_incomingBidi`/
  // `_incomingUnidi` already rely on) is safe.
  class WebTransportDatagramDuplexStream {
    constructor(session) {
      this._readable = openDatagramReadable(session);
      this._writable = openDatagramWritable(session);
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
    this._closed = false;
    this._handle = null;
    this._readyFailed = false;
    // GAP-WEBTRANSPORT срез 4d/4e/datagrams-b: real discovery/datagram
    // streams — all three read `this._handle`/`_closed`/`_readyFailed`
    // lazily on each `pull()`/`write()`, so it is safe to construct them
    // before those settle below.
    this._datagrams = new WebTransportDatagramDuplexStream(this);
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

// Split into a separate file (`webtransport/tests.rs`) rather than an inline
// module — this file was already at the 2000-line convention cap before the
// datagram slice, and a test module this size (the largest single chunk of
// content here) is what a `#[path]` split existed for elsewhere in the
// workspace (`css-parser/src/parser.rs`'s `parser/tests/*.rs`).
#[cfg(all(test, feature = "v8-backend"))]
#[path = "webtransport/tests.rs"]
mod tests_v8;
