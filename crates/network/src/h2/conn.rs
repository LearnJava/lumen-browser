//! HTTP/2 connection driver — RFC 9113 §3–6.
//!
//! 5A: ALPN negotiation (5A.1), frame codec (5A.2), HPACK (5A.3),
//! connection + single-stream GET (5A.4), pool multiplexing (5A.5),
//! flow control (5A.6), concurrent streams support (5A.4 extended).
//!
//! The connection is generic over any `Read + Write` stream so that unit tests
//! can drive it without a real TLS socket (use `std::io::Cursor` / in-memory
//! pipe). In production the caller passes a `RawStream`.
//!
//! ## Connection lifecycle
//!
//! ```text
//! H2Conn::connect(stream)
//!   │  write: client preface magic (24 bytes) + SETTINGS{}
//!   │  read:  server SETTINGS  → write: SETTINGS ACK
//!   │  (also: absorb WINDOW_UPDATE / SETTINGS ACK from server during setup)
//!   └→ Ok(H2Conn { … })
//!
//! Single-stream fetch (synchronous):
//! conn.fetch(method, scheme, authority, path, extra)
//!   │  write: HEADERS (END_HEADERS | END_STREAM for GET)
//!   │  read loop until END_STREAM on this stream:
//!   │    ... (same as before)
//!   └→ Ok((status, headers, body))
//!
//! Concurrent-stream fetch (multiple requests on one connection):
//! conn.send_request(method, scheme, authority, path, extra) → stream_id
//! conn.send_request(...)  → stream_id
//! conn.read_response_for_stream(stream_id) → (status, headers, body)
//! conn.read_response_for_stream(...)  → (status, headers, body)
//! ```
//!
//! ## Out of scope (deferred)
//!
//! - Send-side flow control beyond the initial window: [`H2Conn::fetch_with_body`]
//!   sends a request body only while it fits the peer's *initial* send window
//!   (connection- and stream-level), never blocking on WINDOW_UPDATE. A body
//!   larger than that is refused with [`H2_BODY_EXCEEDS_SEND_WINDOW`] so the
//!   caller can fall back to HTTP/1.1 instead of us having to interleave
//!   WINDOW_UPDATE reads with response frames on the same stream.

use std::collections::HashMap;
use std::io::{Read, Write};

use lumen_core::error::Error;

use crate::h2::{
    frame::{
        Frame, FrameError, MAX_FRAME_PAYLOAD_DEFAULT, SETTING_HEADER_TABLE_SIZE,
        SETTING_INITIAL_WINDOW_SIZE, SETTING_MAX_FRAME_SIZE,
    },
    hpack::{Decoder, Encoder, HeaderField},
};
use crate::http::{H2Settings, HttpProfile};

/// Decoded HTTP response from an H2 fetch: `(status, headers, body)`.
pub type H2Response = (u16, Vec<(String, String)>, Vec<u8>);

/// Tracks the state of a single concurrent stream during response accumulation.
#[derive(Debug)]
struct StreamState {
    /// Accumulated header block for the CURRENT (not-yet-classified) HEADERS
    /// sequence — reset after each informational (1xx) block is decoded and
    /// discarded, so a later block never gets concatenated with it. See
    /// `H2Conn::fetch`'s doc comment for why a single end-of-stream decode
    /// is unsafe.
    hdr_block: Vec<u8>,
    /// Whether the CURRENT header block has fully arrived (END_HEADERS flag).
    end_headers: bool,
    /// Whether we've received the end of the response body (END_STREAM flag).
    end_stream: bool,
    /// Accumulated response body.
    body: Vec<u8>,
    /// Status of the final (non-1xx) response, once its header block has
    /// been decoded. `None` while still skipping informational responses.
    final_status: Option<u16>,
    /// Decoded non-pseudo headers of the final response.
    final_headers: Vec<(String, String)>,
}

impl StreamState {
    fn new() -> Self {
        Self {
            hdr_block: Vec::new(),
            end_headers: false,
            end_stream: false,
            body: Vec::new(),
            final_status: None,
            final_headers: Vec::new(),
        }
    }

    /// Whether this stream is complete: final (non-1xx) status decoded AND
    /// the response body has been fully received.
    fn is_complete(&self) -> bool {
        self.final_status.is_some() && self.end_stream
    }
}

// ── Constants ─────────────────────────────────────────────────────────────

/// Client connection preface magic (RFC 9113 §3.4).
const CLIENT_PREFACE_MAGIC: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// Default flow-control window size (RFC 9113 §6.9.2): 65 535 bytes.
const INITIAL_WINDOW: u32 = 65_535;

/// Read chunk size for `read_frame`.
const READ_CHUNK: usize = 8192;

/// Error message returned by [`H2Conn::fetch_with_body`] when the request body
/// does not fit the peer's send window and would require blocking on
/// WINDOW_UPDATE. Callers match on it to fall back to HTTP/1.1 — it is a
/// routing signal, not a network failure.
pub const H2_BODY_EXCEEDS_SEND_WINDOW: &str = "H2 request body exceeds send window";

// ── H2Conn ────────────────────────────────────────────────────────────────

/// Stateful HTTP/2 client connection.
///
/// One instance per TCP+TLS socket. After construction the connection preface
/// and SETTINGS exchange are complete; the caller can immediately call
/// [`H2Conn::fetch`] or [`H2Conn::send_request`]/[`H2Conn::read_response_for_stream`].
pub struct H2Conn<S: Read + Write> {
    stream: S,
    /// Read-ahead buffer; frames are parsed from this.
    buf: Vec<u8>,
    /// HPACK encoder (outbound headers).
    encoder: Encoder,
    /// HPACK decoder (inbound headers).
    decoder: Decoder,
    /// SETTINGS_MAX_FRAME_SIZE from the remote peer.
    remote_max_frame: u32,
    /// SETTINGS_INITIAL_WINDOW_SIZE from the remote peer (affects streams we open).
    remote_init_window: u32,
    /// Next client-initiated stream ID (odd, starts at 1; RFC 9113 §5.1.1).
    next_stream_id: u32,
    /// Our connection-level receive window (bytes the server may still send before
    /// we send WINDOW_UPDATE). RFC 9113 §6.9 — starts at INITIAL_WINDOW.
    conn_recv_window: u32,
    /// Connection-level *send* window (RFC 9113 §6.9.1): bytes of request body we
    /// may still write across all streams before the peer must grant more with
    /// WINDOW_UPDATE. Starts at the fixed 65 535 default (the peer's SETTINGS
    /// `INITIAL_WINDOW_SIZE` governs *streams*, not the connection), grows on every
    /// stream-0 WINDOW_UPDATE we observe, shrinks by each DATA payload we send.
    conn_send_window: i64,
    /// Concurrent stream state: maps stream ID → StreamState for in-flight requests.
    /// Empty when using single-stream [`fetch`]; populated when using
    /// [`send_request`]/[`read_response_for_stream`].
    pending_streams: HashMap<u32, StreamState>,
    /// Impersonated browser profile — determines the HTTP/2 pseudo-header order
    /// (part of the HTTP/2 fingerprint anti-bot layers key on; see
    /// [`H2Conn::pseudo_headers`]).
    profile: HttpProfile,
}

impl<S: Read + Write> H2Conn<S> {
    /// Establish an HTTP/2 connection with Chrome-matching SETTINGS.
    ///
    /// Convenience wrapper for `connect_with_profile(stream, HttpProfile::Chrome)`.
    pub fn connect(stream: S) -> Result<Self, Error> {
        Self::connect_with_profile(stream, HttpProfile::Chrome)
    }

    /// Establish an HTTP/2 connection over `stream` with SETTINGS matching the given profile.
    ///
    /// Sends the client connection preface (magic + profile-matched SETTINGS) and waits
    /// for the server's initial SETTINGS frame, sending the required ACK.
    /// Pass `HttpProfile::Chrome` for Chrome-matching (ADR-007 Layer 3).
    pub fn connect_with_profile(mut stream: S, profile: HttpProfile) -> Result<Self, Error> {
        // Client connection preface (RFC 9113 §3.4): magic + SETTINGS.
        let settings = H2Settings::for_profile(profile);
        let mut preface = CLIENT_PREFACE_MAGIC.to_vec();

        // Build SETTINGS frame with profile-matching parameters.
        let settings_params = vec![
            (0x0001, settings.header_table_size),      // HEADER_TABLE_SIZE
            (0x0002, if settings.enable_push { 1 } else { 0 }), // ENABLE_PUSH
            (0x0003, settings.max_concurrent_streams.unwrap_or(0)), // MAX_CONCURRENT_STREAMS
            (0x0004, settings.initial_window_size),    // INITIAL_WINDOW_SIZE
            (0x0005, settings.max_frame_size),         // MAX_FRAME_SIZE
        ];

        Frame::Settings {
            ack: false,
            params: settings_params,
        }
        .encode(&mut preface)
        .map_err(frame_err)?;
        stream.write_all(&preface).map_err(io_err)?;
        stream.flush().map_err(io_err)?;

        // Our decoder accepts dynamic-table size updates from the peer up to the
        // value we just advertised in SETTINGS_HEADER_TABLE_SIZE (RFC 7541 §6.3).
        // Without this the proto-max stays at the 4096 default and a legal update
        // from a server that honours our larger advertised limit is rejected as
        // TableSizeTooLarge (BUG-161: ya.ru advertises 65536, sends an update).
        let mut decoder = Decoder::new();
        decoder.set_proto_max(settings.header_table_size as usize);

        let mut conn = Self {
            stream,
            buf: Vec::new(),
            encoder: Encoder::new(),
            decoder,
            remote_max_frame: MAX_FRAME_PAYLOAD_DEFAULT,
            remote_init_window: INITIAL_WINDOW,
            next_stream_id: 1,
            conn_recv_window: INITIAL_WINDOW,
            conn_send_window: INITIAL_WINDOW as i64,
            pending_streams: HashMap::new(),
            profile,
        };

        conn.await_server_settings()?;
        Ok(conn)
    }

    /// Read frames until we see the server's initial SETTINGS (non-ACK), then
    /// send SETTINGS ACK. RFC 9113 §3.4 requires this before any requests.
    fn await_server_settings(&mut self) -> Result<(), Error> {
        loop {
            let frame = self.read_frame()?;
            match frame {
                Frame::Settings { ack: false, params } => {
                    self.apply_remote_settings(&params);
                    self.send_frame(&Frame::Settings {
                        ack: true,
                        params: vec![],
                    })?;
                    return Ok(());
                }
                // Server may ACK our initial SETTINGS before sending its own.
                Frame::Settings { ack: true, .. } => {}
                // Server often sends an initial WINDOW_UPDATE for stream 0 — that
                // is the peer raising the connection-level send window above the
                // 65 535 default, so it must be credited, not dropped.
                Frame::WindowUpdate {
                    stream_id: 0,
                    increment,
                } => {
                    self.credit_conn_send_window(increment);
                }
                // Anything else (PRIORITY etc.) during setup — ignore.
                _ => {}
            }
        }
    }

    fn apply_remote_settings(&mut self, params: &[(u16, u32)]) {
        for &(id, val) in params {
            match id {
                SETTING_HEADER_TABLE_SIZE => self.encoder.set_max_size(val as usize),
                SETTING_INITIAL_WINDOW_SIZE => self.remote_init_window = val,
                SETTING_MAX_FRAME_SIZE => self.remote_max_frame = val,
                _ => {}
            }
        }
    }

    /// Credit the connection-level send window from a stream-0 WINDOW_UPDATE
    /// (RFC 9113 §6.9.1). Saturating: a peer that overflows the 2^31-1 cap is a
    /// protocol error we do not need to police here — clamping keeps our own
    /// accounting monotonic instead of wrapping into a negative budget.
    fn credit_conn_send_window(&mut self, increment: u32) {
        self.conn_send_window = self
            .conn_send_window
            .saturating_add(i64::from(increment))
            .min(i64::from(i32::MAX));
    }

    /// Send a single frame; flushes immediately.
    fn send_frame(&mut self, frame: &Frame) -> Result<(), Error> {
        let mut buf = Vec::new();
        frame.encode(&mut buf).map_err(frame_err)?;
        self.stream.write_all(&buf).map_err(io_err)?;
        self.stream.flush().map_err(io_err)?;
        Ok(())
    }

    /// Read the next complete frame from `self.stream`, buffering as needed.
    fn read_frame(&mut self) -> Result<Frame, Error> {
        let max_frame = self.remote_max_frame;
        loop {
            match Frame::parse(&self.buf, max_frame) {
                Ok(Some((frame, consumed))) => {
                    self.buf.drain(..consumed);
                    return Ok(frame);
                }
                Ok(None) => {
                    let old_len = self.buf.len();
                    self.buf.resize(old_len + READ_CHUNK, 0);
                    let n = self
                        .stream
                        .read(&mut self.buf[old_len..])
                        .map_err(io_err)?;
                    self.buf.truncate(old_len + n);
                    if n == 0 {
                        return Err(Error::Network("H2: unexpected EOF".to_owned()));
                    }
                }
                Err(e) => return Err(frame_err(e)),
            }
        }
    }

    fn allocate_stream_id(&mut self) -> u32 {
        let id = self.next_stream_id;
        self.next_stream_id += 2;
        id
    }

    /// Build the ordered HTTP/2 pseudo-header list for the impersonated profile.
    ///
    /// The pseudo-header order is part of the HTTP/2 fingerprint (the
    /// "Akamai fingerprint") that anti-bot layers key on (RP-7), so it must
    /// match the browser we impersonate:
    /// - Chrome / Edge / Strict / Lumen: `:method :authority :scheme :path`
    /// - Firefox / Tor Browser: `:method :path :authority :scheme`
    /// - Safari: `:method :scheme :path :authority`
    fn pseudo_headers<'a>(
        &self,
        method: &'a str,
        scheme: &'a str,
        authority: &'a str,
        path: &'a str,
    ) -> Vec<(&'a [u8], &'a [u8])> {
        let (nm, na, ns, np): (&'a [u8], &'a [u8], &'a [u8], &'a [u8]) =
            (b":method", b":authority", b":scheme", b":path");
        let (m, s, a, p) = (
            method.as_bytes(),
            scheme.as_bytes(),
            authority.as_bytes(),
            path.as_bytes(),
        );
        match self.profile {
            HttpProfile::Firefox | HttpProfile::TorBrowser => {
                vec![(nm, m), (np, p), (na, a), (ns, s)]
            }
            HttpProfile::Safari => vec![(nm, m), (ns, s), (np, p), (na, a)],
            // Chrome / Edge / Strict / Lumen — Chrome pseudo-header order.
            _ => vec![(nm, m), (na, a), (ns, s), (np, p)],
        }
    }

    // ── Public fetch ──────────────────────────────────────────────────────

    /// Perform a single HTTP/2 request and collect the response.
    ///
    /// Returns `(status_code, response_headers, body)`. Pseudo-headers
    /// (`:status` etc.) are stripped from the returned header list.
    ///
    /// `extra_headers` — additional request headers as `(name, value)` byte
    /// slices (lowercase names, no pseudo-headers — the caller must not add
    /// `:method` / `:path` / `:scheme` / `:authority` here).
    ///
    /// ## Flow control (RFC 9113 §6.9)
    ///
    /// After each DATA frame we immediately send WINDOW_UPDATE for both the
    /// connection (stream 0) and the request stream, restoring exactly the
    /// number of bytes consumed. This prevents the server from stalling on
    /// large responses that exceed the default 65 535-byte window.
    pub fn fetch(
        &mut self,
        method: &str,
        scheme: &str,
        authority: &str,
        path: &str,
        extra_headers: &[(&[u8], &[u8])],
    ) -> Result<H2Response, Error> {
        self.fetch_with_body(method, scheme, authority, path, extra_headers, &[])
    }

    /// [`fetch`](Self::fetch) with a request body (POST/PUT/PATCH/DELETE).
    ///
    /// The body is written as DATA frames after the HEADERS block; the last one
    /// carries END_STREAM. `extra_headers` must already contain `content-type`
    /// and `content-length` — this method adds no headers of its own.
    ///
    /// Send-side flow control (RFC 9113 §6.9.1) is honoured but not *waited* on:
    /// a body that exceeds the currently granted connection- or stream-level send
    /// window is refused up front with [`H2_BODY_EXCEEDS_SEND_WINDOW`] instead of
    /// blocking for a WINDOW_UPDATE that could only arrive interleaved with
    /// response frames. In practice the peer's initial windows are ≥ 64 KiB, which
    /// covers API request bodies; larger uploads fall back to HTTP/1.1 in the
    /// caller, where the socket blocks on the OS buffer instead.
    pub fn fetch_with_body(
        &mut self,
        method: &str,
        scheme: &str,
        authority: &str,
        path: &str,
        extra_headers: &[(&[u8], &[u8])],
        body: &[u8],
    ) -> Result<H2Response, Error> {
        // Refuse BEFORE allocating a stream id / sending HEADERS: a half-sent
        // request would poison the connection for every later request on it.
        if !body.is_empty() {
            let budget = self.conn_send_window.min(self.remote_init_window as i64);
            if body.len() as i64 > budget {
                return Err(Error::Network(H2_BODY_EXCEEDS_SEND_WINDOW.to_owned()));
            }
        }

        let sid = self.allocate_stream_id();

        // Build HPACK request header block (profile-ordered pseudo-headers).
        let mut req = self.pseudo_headers(method, scheme, authority, path);
        req.extend_from_slice(extra_headers);
        let block = self.encoder.encode(&req);

        // END_STREAM on HEADERS only when there is no body to follow.
        self.send_frame(&Frame::Headers {
            stream_id: sid,
            end_stream: body.is_empty(),
            end_headers: true,
            priority: None,
            block_fragment: block,
        })?;

        // DATA frames, chunked to the peer's SETTINGS_MAX_FRAME_SIZE.
        if !body.is_empty() {
            let max_chunk = (self.remote_max_frame as usize).max(1);
            let mut offset = 0;
            while offset < body.len() {
                let end = (offset + max_chunk).min(body.len());
                let chunk = &body[offset..end];
                self.send_frame(&Frame::Data {
                    stream_id: sid,
                    end_stream: end == body.len(),
                    data: chunk.to_vec(),
                })?;
                self.conn_send_window -= chunk.len() as i64;
                offset = end;
            }
        }

        // ── Receive response ───────────────────────────────────────────────
        let mut hdr_block: Vec<u8> = Vec::new();
        let mut end_headers = false;
        let mut end_stream = false;
        let mut body: Vec<u8> = Vec::new();
        // RFC 9110 §15.2 / RFC 9113 §8.1: the server may send one or more
        // informational (1xx, e.g. 103 Early Hints) HEADERS frames before
        // the final response on the same stream. `final_status`/`final_fields`
        // hold the first NON-1xx decoded header block; see the decode-and-
        // classify step below the match for why this can't be a single
        // decode() call at the end (BUG-331 live repro on cloudflare.com).
        let mut final_status: Option<u16> = None;
        let mut final_fields: Vec<HeaderField> = Vec::new();

        while final_status.is_none() || !end_stream {
            let frame = self.read_frame()?;
            match frame {
                // ── Connection-level housekeeping ──────────────────────────
                Frame::Settings {
                    ack: false,
                    params,
                } => {
                    self.apply_remote_settings(&params);
                    self.send_frame(&Frame::Settings {
                        ack: true,
                        params: vec![],
                    })?;
                }
                Frame::Settings { ack: true, .. } => {}
                // Stream-0 updates raise the connection send window (they arrive
                // even on GET-only connections); per-stream ones concern a stream
                // whose request body, if any, is already fully written.
                Frame::WindowUpdate { stream_id: 0, increment } => {
                    self.credit_conn_send_window(increment);
                }
                Frame::WindowUpdate { .. } => {}
                Frame::Ping {
                    ack: false,
                    opaque_data,
                } => {
                    self.send_frame(&Frame::Ping {
                        ack: true,
                        opaque_data,
                    })?;
                }
                Frame::Ping { ack: true, .. } => {}
                Frame::Priority { .. } => {}

                // ── Response headers ───────────────────────────────────────
                Frame::Headers {
                    stream_id,
                    end_stream: es,
                    end_headers: eh,
                    block_fragment,
                    ..
                } if stream_id == sid => {
                    hdr_block.extend_from_slice(&block_fragment);
                    end_headers = eh;
                    if es {
                        end_stream = true;
                    }
                }
                Frame::Continuation {
                    stream_id,
                    end_headers: eh,
                    block_fragment,
                } if stream_id == sid => {
                    hdr_block.extend_from_slice(&block_fragment);
                    end_headers = eh;
                }

                // ── Response body ──────────────────────────────────────────
                Frame::Data {
                    stream_id,
                    end_stream: es,
                    data,
                } if stream_id == sid => {
                    let consumed = data.len() as u32;
                    body.extend_from_slice(&data);
                    if es {
                        end_stream = true;
                    }
                    // RFC 9113 §6.9: restore receive windows so the server can
                    // keep sending without stalling on large bodies.
                    if consumed > 0 {
                        // Connection-level window (stream_id = 0).
                        self.conn_recv_window =
                            self.conn_recv_window.saturating_sub(consumed);
                        self.send_frame(&Frame::WindowUpdate {
                            stream_id: 0,
                            increment: consumed,
                        })?;
                        // Stream-level window.
                        self.send_frame(&Frame::WindowUpdate {
                            stream_id: sid,
                            increment: consumed,
                        })?;
                    }
                }

                // ── Error frames ───────────────────────────────────────────
                Frame::Goaway { error_code, .. } => {
                    return Err(Error::Network(format!(
                        "H2 GOAWAY: error_code={error_code:#x}"
                    )));
                }
                Frame::RstStream {
                    stream_id,
                    error_code,
                } if stream_id == sid => {
                    return Err(Error::Network(format!(
                        "H2 RST_STREAM on stream {stream_id}: error_code={error_code:#x}"
                    )));
                }

                // ── Everything else ────────────────────────────────────────
                // Frames on other streams, PushPromise, Unknown extensions.
                _ => {}
            }

            // Decode as soon as a headers block completes, BEFORE the next
            // frame (possibly a second HEADERS block) can arrive. A single
            // decode() at the very end — the original approach — would
            // concatenate an informational block with the final block into
            // one HPACK byte stream; decoding that blob doesn't error (HPACK
            // is just a flat instruction sequence), it silently yields BOTH
            // header sets merged, and `:status` resolves to whichever came
            // first via `.find()` — i.e. the 1xx status, not the real one.
            if end_headers && final_status.is_none() {
                let fields = self
                    .decoder
                    .decode(&hdr_block)
                    .map_err(|e| Error::Network(format!("H2 HPACK decode: {e}")))?;
                let status = fields
                    .iter()
                    .find(|f| f.name == b":status")
                    .and_then(|f| std::str::from_utf8(&f.value).ok())
                    .and_then(|s| s.parse::<u16>().ok())
                    .ok_or_else(|| Error::Network("H2: response missing :status".to_owned()))?;
                if (100..200).contains(&status) {
                    // Informational — reset the accumulator for the block
                    // that follows and keep waiting. RFC 9113 §8.1: a 1xx
                    // HEADERS frame must not carry END_STREAM; ignore it
                    // defensively if a non-conformant server sets it anyway.
                    hdr_block.clear();
                    end_headers = false;
                    end_stream = false;
                } else {
                    final_status = Some(status);
                    final_fields = fields;
                }
            }
        }

        let headers: Vec<(String, String)> = final_fields
            .into_iter()
            .filter(|f| !f.name.starts_with(b":"))
            .map(|f| {
                (
                    String::from_utf8_lossy(&f.name).into_owned(),
                    String::from_utf8_lossy(&f.value).into_owned(),
                )
            })
            .collect();

        Ok((final_status.unwrap(), headers, body))
    }

    /// Send a single HTTP/2 request without waiting for the response.
    ///
    /// This method is used for concurrent-stream workflows: send multiple requests,
    /// then collect responses with [`read_response_for_stream`].
    ///
    /// Returns the stream ID assigned to this request. The stream will remain
    /// in-flight until [`read_response_for_stream`] completes it.
    ///
    /// `extra_headers` — additional request headers as `(name, value)` byte
    /// slices (lowercase names, no pseudo-headers).
    pub fn send_request(
        &mut self,
        method: &str,
        scheme: &str,
        authority: &str,
        path: &str,
        extra_headers: &[(&[u8], &[u8])],
    ) -> Result<u32, Error> {
        let sid = self.allocate_stream_id();

        // Initialize StreamState for this in-flight request.
        self.pending_streams.insert(sid, StreamState::new());

        // Build HPACK request header block (profile-ordered pseudo-headers).
        let mut req = self.pseudo_headers(method, scheme, authority, path);
        req.extend_from_slice(extra_headers);
        let block = self.encoder.encode(&req);

        // HEADERS with END_STREAM (GET / HEAD have no request body).
        self.send_frame(&Frame::Headers {
            stream_id: sid,
            end_stream: true,
            end_headers: true,
            priority: None,
            block_fragment: block,
        })?;

        Ok(sid)
    }

    /// Read and assemble the complete response for a specific stream ID.
    ///
    /// Reads frames from the connection until the response for this stream
    /// is complete (both headers and body received, END_STREAM flag seen).
    /// Handles connection-level frames (SETTINGS, PING, WINDOW_UPDATE) for
    /// all streams automatically.
    ///
    /// Returns `(status_code, response_headers, body)`. Pseudo-headers
    /// are stripped from the returned header list.
    ///
    /// After this method returns, the stream ID is invalid and may be reused
    /// (by allocating a new one). Calling with a non-existent stream ID
    /// returns an error.
    pub fn read_response_for_stream(&mut self, sid: u32) -> Result<H2Response, Error> {
        if !self.pending_streams.contains_key(&sid) {
            return Err(Error::Network(format!("H2: no pending stream {sid}")));
        }

        while !self.pending_streams[&sid].is_complete() {
            let frame = self.read_frame()?;
            match frame {
                // ── Connection-level housekeeping ──────────────────────────
                Frame::Settings {
                    ack: false,
                    params,
                } => {
                    self.apply_remote_settings(&params);
                    self.send_frame(&Frame::Settings {
                        ack: true,
                        params: vec![],
                    })?;
                }
                Frame::Settings { ack: true, .. } => {}
                // Stream-0 updates raise the connection send window (they arrive
                // even on GET-only connections); per-stream ones concern a stream
                // whose request body, if any, is already fully written.
                Frame::WindowUpdate { stream_id: 0, increment } => {
                    self.credit_conn_send_window(increment);
                }
                Frame::WindowUpdate { .. } => {}
                Frame::Ping {
                    ack: false,
                    opaque_data,
                } => {
                    self.send_frame(&Frame::Ping {
                        ack: true,
                        opaque_data,
                    })?;
                }
                Frame::Ping { ack: true, .. } => {}
                Frame::Priority { .. } => {}

                // ── Response headers ───────────────────────────────────────
                Frame::Headers {
                    stream_id,
                    end_stream: es,
                    end_headers: eh,
                    block_fragment,
                    ..
                } if stream_id == sid => {
                    if let Some(stream) = self.pending_streams.get_mut(&sid) {
                        stream.hdr_block.extend_from_slice(&block_fragment);
                        stream.end_headers = eh;
                        if es {
                            stream.end_stream = true;
                        }
                    }
                }
                Frame::Continuation {
                    stream_id,
                    end_headers: eh,
                    block_fragment,
                } if stream_id == sid => {
                    if let Some(stream) = self.pending_streams.get_mut(&sid) {
                        stream.hdr_block.extend_from_slice(&block_fragment);
                        stream.end_headers = eh;
                    }
                }

                // ── Response body ──────────────────────────────────────────
                Frame::Data {
                    stream_id,
                    end_stream: es,
                    data,
                } if stream_id == sid => {
                    let consumed = data.len() as u32;
                    if let Some(stream) = self.pending_streams.get_mut(&sid) {
                        stream.body.extend_from_slice(&data);
                        if es {
                            stream.end_stream = true;
                        }
                    }
                    // RFC 9113 §6.9: restore receive windows.
                    if consumed > 0 {
                        self.conn_recv_window =
                            self.conn_recv_window.saturating_sub(consumed);
                        self.send_frame(&Frame::WindowUpdate {
                            stream_id: 0,
                            increment: consumed,
                        })?;
                        self.send_frame(&Frame::WindowUpdate {
                            stream_id: sid,
                            increment: consumed,
                        })?;
                    }
                }

                // ── Error frames ───────────────────────────────────────────
                Frame::Goaway { error_code, .. } => {
                    return Err(Error::Network(format!(
                        "H2 GOAWAY: error_code={error_code:#x}"
                    )));
                }
                Frame::RstStream {
                    stream_id,
                    error_code,
                } if stream_id == sid => {
                    return Err(Error::Network(format!(
                        "H2 RST_STREAM on stream {stream_id}: error_code={error_code:#x}"
                    )));
                }

                // ── Everything else ────────────────────────────────────────
                // Frames on other streams, PushPromise, Unknown extensions.
                _ => {}
            }

            // Decode as soon as a headers block completes, before a later
            // block for the same stream can be appended to it — see
            // `H2Conn::fetch`'s matching step for why concatenating an
            // informational (1xx) block with the final one is unsafe.
            let headers_ready = self
                .pending_streams
                .get(&sid)
                .map(|s| s.end_headers && s.final_status.is_none())
                .unwrap_or(false);
            if headers_ready {
                let hdr_bytes =
                    std::mem::take(&mut self.pending_streams.get_mut(&sid).unwrap().hdr_block);
                let fields = self
                    .decoder
                    .decode(&hdr_bytes)
                    .map_err(|e| Error::Network(format!("H2 HPACK decode: {e}")))?;
                let status = fields
                    .iter()
                    .find(|f| f.name == b":status")
                    .and_then(|f| std::str::from_utf8(&f.value).ok())
                    .and_then(|s| s.parse::<u16>().ok())
                    .ok_or_else(|| Error::Network("H2: response missing :status".to_owned()))?;
                if let Some(stream) = self.pending_streams.get_mut(&sid) {
                    if (100..200).contains(&status) {
                        // Informational — RFC 9113 §8.1 forbids END_STREAM on
                        // a 1xx block; ignore it defensively if set anyway.
                        stream.end_headers = false;
                        stream.end_stream = false;
                    } else {
                        stream.final_status = Some(status);
                        stream.final_headers = fields
                            .into_iter()
                            .filter(|f| !f.name.starts_with(b":"))
                            .map(|f| {
                                (
                                    String::from_utf8_lossy(&f.name).into_owned(),
                                    String::from_utf8_lossy(&f.value).into_owned(),
                                )
                            })
                            .collect();
                    }
                }
            }
        }

        // Stream is complete; extract and return the response.
        let stream = self.pending_streams.remove(&sid).unwrap();
        Ok((stream.final_status.unwrap(), stream.final_headers, stream.body))
    }
}

// ── Error helpers ─────────────────────────────────────────────────────────

fn io_err(e: std::io::Error) -> Error {
    Error::Network(format!("H2 I/O: {e}"))
}

fn frame_err(e: FrameError) -> Error {
    Error::Network(format!("H2 frame: {e}"))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h2::frame::{Frame, SETTING_MAX_FRAME_SIZE};

    /// In-memory bidirectional stream for testing: client writes to `client_tx`,
    /// server reads from `client_tx`; server writes to `server_tx`, client
    /// reads from `server_tx`.
    struct MockStream {
        /// Data written by the other side (our input).
        rx: std::io::Cursor<Vec<u8>>,
        /// Data we have written (captured for assertions).
        tx: Vec<u8>,
        /// Pre-loaded bytes to feed to the reader after the cursor is exhausted.
        pending: std::collections::VecDeque<u8>,
    }

    impl MockStream {
        fn new(server_data: Vec<u8>) -> Self {
            Self {
                rx: std::io::Cursor::new(server_data),
                tx: Vec::new(),
                pending: std::collections::VecDeque::new(),
            }
        }

        fn written(&self) -> &[u8] {
            &self.tx
        }
    }

    impl Read for MockStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.rx.position() < self.rx.get_ref().len() as u64 {
                return self.rx.read(buf);
            }
            if !self.pending.is_empty() {
                let n = buf.len().min(self.pending.len());
                for b in buf.iter_mut().take(n) {
                    *b = self.pending.pop_front().unwrap();
                }
                return Ok(n);
            }
            // EOF
            Ok(0)
        }
    }

    impl Write for MockStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.tx.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Build a minimal server-side sequence: SETTINGS + SETTINGS_ACK.
    fn server_preface_bytes() -> Vec<u8> {
        let mut buf = Vec::new();
        Frame::Settings {
            ack: false,
            params: vec![],
        }
        .encode(&mut buf)
        .unwrap();
        buf
    }

    /// Build server SETTINGS (with params) + SETTINGS_ACK.
    fn server_preface_with_params(params: Vec<(u16, u32)>) -> Vec<u8> {
        let mut buf = Vec::new();
        Frame::Settings { ack: false, params }.encode(&mut buf).unwrap();
        buf
    }

    /// Encode a simple 200 response: HEADERS + DATA.
    fn encode_response_200(sid: u32, body: &[u8]) -> Vec<u8> {
        use crate::h2::hpack::Encoder;
        let mut enc = Encoder::new();
        let block = enc.encode(&[(b":status", b"200"), (b"content-type", b"text/plain")]);

        let mut buf = Vec::new();
        Frame::Headers {
            stream_id: sid,
            end_stream: body.is_empty(),
            end_headers: true,
            priority: None,
            block_fragment: block,
        }
        .encode(&mut buf)
        .unwrap();
        if !body.is_empty() {
            Frame::Data {
                stream_id: sid,
                end_stream: true,
                data: body.to_vec(),
            }
            .encode(&mut buf)
            .unwrap();
        }
        buf
    }

    /// Encode a `103 Early Hints` informational HEADERS block, followed by
    /// the real `200` response (HEADERS + optional DATA) on the same
    /// stream — RFC 9110 §15.2 / RFC 9113 §8.1. Both blocks are encoded
    /// through ONE `Encoder` instance, mirroring how a real server keeps a
    /// single HPACK encoding context for the whole connection.
    fn encode_response_with_early_hint(sid: u32, body: &[u8]) -> Vec<u8> {
        use crate::h2::hpack::Encoder;
        let mut enc = Encoder::new();
        let mut buf = Vec::new();

        let informational = enc.encode(&[(b":status", b"103"), (b"link", b"</style.css>; rel=preload")]);
        Frame::Headers {
            stream_id: sid,
            end_stream: false,
            end_headers: true,
            priority: None,
            block_fragment: informational,
        }
        .encode(&mut buf)
        .unwrap();

        let final_block = enc.encode(&[(b":status", b"200"), (b"content-type", b"text/plain")]);
        Frame::Headers {
            stream_id: sid,
            end_stream: body.is_empty(),
            end_headers: true,
            priority: None,
            block_fragment: final_block,
        }
        .encode(&mut buf)
        .unwrap();
        if !body.is_empty() {
            Frame::Data {
                stream_id: sid,
                end_stream: true,
                data: body.to_vec(),
            }
            .encode(&mut buf)
            .unwrap();
        }
        buf
    }

    // ── connect() ─────────────────────────────────────────────────────────

    #[test]
    fn connect_sends_preface_and_acks_server_settings() {
        let server_data = server_preface_bytes();
        let mock = MockStream::new(server_data);
        let conn = H2Conn::connect(mock).unwrap();

        let written = conn.stream.written();
        // Must start with client preface magic.
        assert!(
            written.starts_with(CLIENT_PREFACE_MAGIC),
            "client preface magic missing"
        );
        // Must contain our Chrome-matching SETTINGS.
        let after_magic = &written[CLIENT_PREFACE_MAGIC.len()..];
        let (frame, _) = Frame::parse(after_magic, MAX_FRAME_PAYLOAD_DEFAULT)
            .unwrap()
            .unwrap();
        // Verify Chrome-matching SETTINGS: [header_table_size, enable_push, max_concurrent, initial_window, max_frame]
        match frame {
            Frame::Settings { ack: false, params } => {
                assert_eq!(params.len(), 5, "Chrome SETTINGS should have 5 parameters");
                assert_eq!(params[0], (1, 65536), "HEADER_TABLE_SIZE should be 65536");
                assert_eq!(params[1], (2, 1), "ENABLE_PUSH should be 1");
                assert_eq!(params[2], (3, 1000), "MAX_CONCURRENT_STREAMS should be 1000");
                assert_eq!(params[3], (4, 6291456), "INITIAL_WINDOW_SIZE should be 6291456");
                assert_eq!(params[4], (5, 16384), "MAX_FRAME_SIZE should be 16384");
            }
            _ => panic!("Expected Settings frame with Chrome parameters"),
        }
        // Must contain SETTINGS ACK for server's SETTINGS.
        // Find it after our SETTINGS frame.
        // Chrome SETTINGS frame: 9-byte header + (5 params * 6 bytes) = 9 + 30 = 39 bytes
        let offset = CLIENT_PREFACE_MAGIC.len() + 39;
        let (ack_frame, _) = Frame::parse(&written[offset..], MAX_FRAME_PAYLOAD_DEFAULT)
            .unwrap()
            .unwrap();
        assert_eq!(
            ack_frame,
            Frame::Settings {
                ack: true,
                params: vec![]
            }
        );
    }

    #[test]
    fn connect_applies_remote_max_frame_size() {
        let server_data =
            server_preface_with_params(vec![(SETTING_MAX_FRAME_SIZE, 32_768)]);
        let mock = MockStream::new(server_data);
        let conn = H2Conn::connect(mock).unwrap();
        assert_eq!(conn.remote_max_frame, 32_768);
    }

    #[test]
    fn connect_with_profile_firefox_sends_firefox_settings() {
        let server_data = server_preface_bytes();
        let mock = MockStream::new(server_data);
        let conn = H2Conn::connect_with_profile(mock, HttpProfile::Firefox).unwrap();
        let written = conn.stream.written();
        let after_magic = &written[CLIENT_PREFACE_MAGIC.len()..];
        let (frame, _) = Frame::parse(after_magic, MAX_FRAME_PAYLOAD_DEFAULT)
            .unwrap()
            .unwrap();
        match frame {
            Frame::Settings { ack: false, params } => {
                // Firefox uses initial_window_size = 2147483647 (max i32)
                let window = params.iter().find(|(id, _)| *id == 4).map(|(_, v)| *v);
                assert_eq!(window, Some(2147483647), "Firefox INITIAL_WINDOW_SIZE should be max i32");
            }
            _ => panic!("Expected Settings frame"),
        }
    }

    #[test]
    fn connect_with_profile_tor_sends_conservative_settings() {
        let server_data = server_preface_bytes();
        let mock = MockStream::new(server_data);
        let conn = H2Conn::connect_with_profile(mock, HttpProfile::TorBrowser).unwrap();
        let written = conn.stream.written();
        let after_magic = &written[CLIENT_PREFACE_MAGIC.len()..];
        let (frame, _) = Frame::parse(after_magic, MAX_FRAME_PAYLOAD_DEFAULT)
            .unwrap()
            .unwrap();
        match frame {
            Frame::Settings { ack: false, params } => {
                // TorBrowser uses conservative settings: header_table_size = 4096 (RFC default)
                let table_size = params.iter().find(|(id, _)| *id == 1).map(|(_, v)| *v);
                assert_eq!(table_size, Some(4096), "TorBrowser HEADER_TABLE_SIZE should be RFC default 4096");
                // max_concurrent_streams = 100
                let max_streams = params.iter().find(|(id, _)| *id == 3).map(|(_, v)| *v);
                assert_eq!(max_streams, Some(100), "TorBrowser MAX_CONCURRENT_STREAMS should be 100");
            }
            _ => panic!("Expected Settings frame"),
        }
    }

    #[test]
    fn connect_is_alias_for_chrome_profile() {
        // connect() must behave identically to connect_with_profile(Chrome)
        let server_data = server_preface_bytes();
        let mock = MockStream::new(server_data.clone());
        let conn_default = H2Conn::connect(mock).unwrap();

        let mock2 = MockStream::new(server_data);
        let conn_chrome = H2Conn::connect_with_profile(mock2, HttpProfile::Chrome).unwrap();

        assert_eq!(conn_default.stream.written(), conn_chrome.stream.written());
    }

    // ── fetch() ───────────────────────────────────────────────────────────

    fn make_connected_conn(extra_server: Vec<u8>) -> H2Conn<MockStream> {
        let mut server_data = server_preface_bytes();
        server_data.extend_from_slice(&extra_server);
        H2Conn::connect(MockStream::new(server_data)).unwrap()
    }

    #[test]
    fn fetch_sends_headers_frame() {
        let resp = encode_response_200(1, b"hello");
        let mut conn = make_connected_conn(resp);
        let (status, _hdrs, body) = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"hello");
    }

    #[test]
    fn fetch_empty_body_with_end_stream_on_headers() {
        let resp = encode_response_200(1, b"");
        let mut conn = make_connected_conn(resp);
        let (status, _hdrs, body) = conn
            .fetch("GET", "https", "example.com", "/empty", &[])
            .unwrap();
        assert_eq!(status, 200);
        assert!(body.is_empty());
    }

    #[test]
    fn fetch_returns_non_pseudo_headers() {
        let resp = encode_response_200(1, b"data");
        let mut conn = make_connected_conn(resp);
        let (_status, hdrs, _body) = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap();
        // :status must be stripped; content-type must be present.
        assert!(!hdrs.iter().any(|(k, _)| k == ":status"));
        assert!(hdrs.iter().any(|(k, v)| k == "content-type" && v == "text/plain"));
    }

    #[test]
    fn fetch_skips_1xx_informational_headers_before_final_status() {
        // BUG-331 live repro (cloudflare.com over h2, 2026-08-06): the old
        // code decoded the whole stream's accumulated header bytes ONCE at
        // the end, so a 103 Early Hints block followed by the real 200
        // silently concatenated into one HPACK byte stream — decoding that
        // doesn't error, it just merges both field lists, and `:status`
        // resolved to 103 (the first match) instead of the real 200.
        let resp = encode_response_with_early_hint(1, b"hello");
        let mut conn = make_connected_conn(resp);
        let (status, hdrs, body) = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap();
        assert_eq!(status, 200, "final status must win over the 103 informational one");
        assert_eq!(body, b"hello");
        assert!(hdrs.iter().any(|(k, v)| k == "content-type" && v == "text/plain"));
        assert!(
            !hdrs.iter().any(|(k, _)| k == "link"),
            "informational-only header must not leak into the final header list"
        );
    }

    #[test]
    fn fetch_with_extra_headers() {
        let resp = encode_response_200(1, b"");
        let mut conn = make_connected_conn(resp);
        let (status, _, _) = conn
            .fetch(
                "GET",
                "https",
                "example.com",
                "/",
                &[(b"accept", b"text/html"), (b"user-agent", b"lumen/0")],
            )
            .unwrap();
        assert_eq!(status, 200);
        // Verify the HEADERS frame we sent includes the extra headers.
        // (just verify no error — full header decode tested elsewhere)
    }

    #[test]
    fn fetch_handles_settings_mid_response() {
        // Server sends SETTINGS in the middle of the response.
        let mut resp_bytes = Vec::new();
        Frame::Settings {
            ack: false,
            params: vec![],
        }
        .encode(&mut resp_bytes)
        .unwrap();
        resp_bytes.extend_from_slice(&encode_response_200(1, b"ok"));

        let mut conn = make_connected_conn(resp_bytes);
        let (status, _, body) = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"ok");
    }

    #[test]
    fn fetch_handles_ping_mid_response() {
        let mut resp_bytes = Vec::new();
        Frame::Ping {
            ack: false,
            opaque_data: [1, 2, 3, 4, 5, 6, 7, 8],
        }
        .encode(&mut resp_bytes)
        .unwrap();
        resp_bytes.extend_from_slice(&encode_response_200(1, b"pong"));

        let mut conn = make_connected_conn(resp_bytes);
        let (status, _, body) = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"pong");
    }

    #[test]
    fn fetch_rst_stream_returns_error() {
        let mut resp_bytes = Vec::new();
        Frame::RstStream {
            stream_id: 1,
            error_code: 0x01, // PROTOCOL_ERROR
        }
        .encode(&mut resp_bytes)
        .unwrap();
        let mut conn = make_connected_conn(resp_bytes);
        let err = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap_err();
        assert!(format!("{err}").contains("RST_STREAM"));
    }

    #[test]
    fn fetch_goaway_returns_error() {
        let mut resp_bytes = Vec::new();
        Frame::Goaway {
            last_stream_id: 0,
            error_code: 0x01,
            debug_data: vec![],
        }
        .encode(&mut resp_bytes)
        .unwrap();
        let mut conn = make_connected_conn(resp_bytes);
        let err = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap_err();
        assert!(format!("{err}").contains("GOAWAY"));
    }

    #[test]
    fn second_fetch_uses_stream_id_3() {
        // Two sequential fetches should use stream ids 1 and 3.
        let mut server_data = server_preface_bytes();
        server_data.extend_from_slice(&encode_response_200(1, b"first"));
        server_data.extend_from_slice(&encode_response_200(3, b"second"));

        let mut conn = H2Conn::connect(MockStream::new(server_data)).unwrap();
        let (s1, _, b1) = conn
            .fetch("GET", "https", "example.com", "/1", &[])
            .unwrap();
        let (s2, _, b2) = conn
            .fetch("GET", "https", "example.com", "/2", &[])
            .unwrap();
        assert_eq!((s1, b1.as_slice()), (200, b"first".as_slice()));
        assert_eq!((s2, b2.as_slice()), (200, b"second".as_slice()));
    }

    // ── Flow control (5A.6) ───────────────────────────────────────────────

    /// Collect all WINDOW_UPDATE frames from a byte buffer; returns
    /// `(stream_id, increment)` pairs in order.
    ///
    /// Skips the client connection preface magic (24 non-frame bytes) that
    /// the client writes before any frames during `H2Conn::connect`.
    fn collect_window_updates(buf: &[u8]) -> Vec<(u32, u32)> {
        use crate::h2::frame::MAX_FRAME_PAYLOAD_DEFAULT;
        let start = if buf.starts_with(CLIENT_PREFACE_MAGIC) {
            CLIENT_PREFACE_MAGIC.len()
        } else {
            0
        };
        let mut result = Vec::new();
        let mut pos = start;
        while pos < buf.len() {
            match Frame::parse(&buf[pos..], MAX_FRAME_PAYLOAD_DEFAULT) {
                Ok(Some((Frame::WindowUpdate { stream_id, increment }, consumed))) => {
                    result.push((stream_id, increment));
                    pos += consumed;
                }
                Ok(Some((_, consumed))) => {
                    pos += consumed;
                }
                _ => break,
            }
        }
        result
    }

    #[test]
    fn fetch_with_body_sends_window_update_for_data() {
        // Server sends a DATA frame with 11 bytes.
        let body_data = b"hello world";
        let resp = encode_response_200(1, body_data);
        let mut conn = make_connected_conn(resp);
        let (status, _, body) = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, body_data);

        // Client MUST have sent WINDOW_UPDATE for connection (stream 0) and
        // stream 1 with increment = 11 (bytes consumed from DATA).
        let written = conn.stream.written();
        let updates = collect_window_updates(written);
        assert!(
            updates
                .iter()
                .any(|&(sid, inc)| sid == 0 && inc == body_data.len() as u32),
            "missing connection-level WINDOW_UPDATE; found: {updates:?}"
        );
        assert!(
            updates
                .iter()
                .any(|&(sid, inc)| sid == 1 && inc == body_data.len() as u32),
            "missing stream-level WINDOW_UPDATE; found: {updates:?}"
        );
    }

    #[test]
    fn fetch_empty_body_sends_no_window_update() {
        // END_STREAM on HEADERS, no DATA frames → no DATA consumed → no WINDOW_UPDATE.
        let resp = encode_response_200(1, b"");
        let mut conn = make_connected_conn(resp);
        conn.fetch("GET", "https", "example.com", "/", &[])
            .unwrap();

        let written = conn.stream.written();
        let updates = collect_window_updates(written);
        assert!(
            updates.is_empty(),
            "unexpected WINDOW_UPDATE for empty body: {updates:?}"
        );
    }

    #[test]
    fn fetch_multi_data_frames_sends_window_update_per_frame() {
        // Two DATA frames (5 bytes + 6 bytes) — we should get WINDOW_UPDATE
        // after each, restoring the exact amount consumed.
        let mut resp_bytes = Vec::new();
        use crate::h2::hpack::Encoder;
        // HEADERS first (no END_STREAM yet).
        let block = Encoder::new().encode(&[(b":status", b"200")]);
        Frame::Headers {
            stream_id: 1,
            end_stream: false,
            end_headers: true,
            priority: None,
            block_fragment: block,
        }
        .encode(&mut resp_bytes)
        .unwrap();
        // First DATA chunk.
        Frame::Data {
            stream_id: 1,
            end_stream: false,
            data: b"hello".to_vec(),
        }
        .encode(&mut resp_bytes)
        .unwrap();
        // Second DATA chunk with END_STREAM.
        Frame::Data {
            stream_id: 1,
            end_stream: true,
            data: b" world".to_vec(),
        }
        .encode(&mut resp_bytes)
        .unwrap();

        let mut conn = make_connected_conn(resp_bytes);
        let (status, _, body) = conn
            .fetch("GET", "https", "example.com", "/", &[])
            .unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"hello world");

        // Expect WINDOW_UPDATE for (stream=0, inc=5), (stream=1, inc=5),
        // (stream=0, inc=6), (stream=1, inc=6) — two pairs, one per chunk.
        let written = conn.stream.written();
        let updates = collect_window_updates(written);
        let conn_updates: Vec<u32> = updates.iter().filter(|&&(sid, _)| sid == 0).map(|&(_, inc)| inc).collect();
        let stream_updates: Vec<u32> = updates.iter().filter(|&&(sid, _)| sid == 1).map(|&(_, inc)| inc).collect();
        assert_eq!(conn_updates.iter().sum::<u32>(), 11, "conn increments: {conn_updates:?}");
        assert_eq!(stream_updates.iter().sum::<u32>(), 11, "stream increments: {stream_updates:?}");
        assert!(conn_updates.contains(&5) && conn_updates.contains(&6), "{conn_updates:?}");
    }

    #[test]
    fn concurrent_send_request_allocates_stream_ids() {
        let server_data = Vec::new();
        let mut conn = make_connected_conn(server_data);

        let sid1 = conn.send_request("GET", "https", "example.com", "/", &[]).unwrap();
        let sid2 = conn.send_request("GET", "https", "example.com", "/page", &[]).unwrap();
        let sid3 = conn.send_request("GET", "https", "example.com", "/api", &[]).unwrap();

        // Client-initiated stream IDs are odd and sequential.
        assert_eq!(sid1, 1);
        assert_eq!(sid2, 3);
        assert_eq!(sid3, 5);

        // pending_streams should now contain these stream IDs.
        assert!(conn.pending_streams.contains_key(&sid1));
        assert!(conn.pending_streams.contains_key(&sid2));
        assert!(conn.pending_streams.contains_key(&sid3));
    }

    #[test]
    fn concurrent_send_request_sends_headers_frames() {
        let server_data = Vec::new();
        let mut conn = make_connected_conn(server_data);

        let _sid1 = conn.send_request("GET", "https", "example.com", "/", &[]).unwrap();

        // Check that a HEADERS frame was written.
        let written = conn.stream.written();
        // Skip the preface (24 bytes) + SETTINGS (9+30 bytes) + SETTINGS ACK (9 bytes).
        let expected_offset = 24 + 39 + 9;
        let frame = Frame::parse(&written[expected_offset..], MAX_FRAME_PAYLOAD_DEFAULT)
            .unwrap()
            .unwrap();
        let (parsed_frame, _) = frame;

        match parsed_frame {
            Frame::Headers {
                stream_id,
                end_stream,
                end_headers,
                ..
            } => {
                assert_eq!(stream_id, 1);
                assert!(end_stream, "GET requests should have END_STREAM");
                assert!(end_headers, "Headers should be complete");
            }
            _ => panic!("Expected HEADERS frame, got {parsed_frame:?}"),
        }
    }

    #[test]
    fn concurrent_read_response_for_stream() {
        let mut resp_bytes = Vec::new();
        use crate::h2::hpack::Encoder;
        // Response for stream 1: status 200, body "hello"
        let block = Encoder::new().encode(&[(b":status", b"200")]);
        Frame::Headers {
            stream_id: 1,
            end_stream: false,
            end_headers: true,
            priority: None,
            block_fragment: block,
        }
        .encode(&mut resp_bytes)
        .unwrap();
        Frame::Data {
            stream_id: 1,
            end_stream: true,
            data: b"hello".to_vec(),
        }
        .encode(&mut resp_bytes)
        .unwrap();

        let mut conn = make_connected_conn(resp_bytes);
        let _sid = conn.send_request("GET", "https", "example.com", "/", &[]).unwrap();

        let (status, headers, body) = conn.read_response_for_stream(1).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"hello");
        assert!(!headers.iter().any(|(name, _)| name.starts_with(":")));

        // Stream should no longer be pending.
        assert!(!conn.pending_streams.contains_key(&1));
    }

    #[test]
    fn concurrent_read_response_for_stream_skips_1xx_informational_headers() {
        // Same defect as `fetch_skips_1xx_informational_headers_before_final_status`,
        // exercised through the concurrent-stream API (`send_request` +
        // `read_response_for_stream`) which has its own accumulation state
        // (`StreamState`) and previously had the identical single-decode bug.
        let resp_bytes = encode_response_with_early_hint(1, b"hello");

        let mut conn = make_connected_conn(resp_bytes);
        let _sid = conn.send_request("GET", "https", "example.com", "/", &[]).unwrap();

        let (status, headers, body) = conn.read_response_for_stream(1).unwrap();
        assert_eq!(status, 200, "final status must win over the 103 informational one");
        assert_eq!(body, b"hello");
        assert!(headers.iter().any(|(k, v)| k == "content-type" && v == "text/plain"));
        assert!(
            !headers.iter().any(|(k, _)| k == "link"),
            "informational-only header must not leak into the final header list"
        );
        assert!(!conn.pending_streams.contains_key(&1));
    }

    #[test]
    fn concurrent_read_response_for_invalid_stream_errors() {
        let server_data = Vec::new();
        let mut conn = make_connected_conn(server_data);

        let err = conn.read_response_for_stream(999).unwrap_err();
        assert!(err.to_string().contains("no pending stream"));
    }

    #[test]
    fn concurrent_multiple_streams_single_response() {
        let mut resp_bytes = Vec::new();
        use crate::h2::hpack::Encoder;
        // Response for stream 1 only.
        let block = Encoder::new().encode(&[(b":status", b"201")]);
        Frame::Headers {
            stream_id: 1,
            end_stream: false,
            end_headers: true,
            priority: None,
            block_fragment: block,
        }
        .encode(&mut resp_bytes)
        .unwrap();
        Frame::Data {
            stream_id: 1,
            end_stream: true,
            data: b"resp1".to_vec(),
        }
        .encode(&mut resp_bytes)
        .unwrap();

        let mut conn = make_connected_conn(resp_bytes);
        let sid1 = conn.send_request("GET", "https", "example.com", "/1", &[]).unwrap();
        let _sid2 = conn.send_request("GET", "https", "example.com", "/2", &[]).unwrap();

        // Read response for stream 1 (stream 2 is still pending).
        let (status, _, body) = conn.read_response_for_stream(sid1).unwrap();
        assert_eq!(status, 201);
        assert_eq!(body, b"resp1");

        // Stream 1 should be removed, stream 3 (allocated for sid2) should remain.
        assert!(!conn.pending_streams.contains_key(&1));
        assert!(conn.pending_streams.contains_key(&3));
    }
}
