//! HTTP/2 connection driver — RFC 9113 §3–6.
//!
//! 5A.4: preface exchange + SETTINGS negotiation + single-stream GET.
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
//! conn.fetch(method, scheme, authority, path, extra)
//!   │  write: HEADERS (END_HEADERS | END_STREAM for GET)
//!   │  read loop until END_STREAM on this stream:
//!   │    SETTINGS      → ACK, continue
//!   │    PING          → ACK, continue
//!   │    WINDOW_UPDATE → update window, continue
//!   │    HEADERS/CONTINUATION (our stream) → accumulate block_fragment
//!   │    DATA          (our stream) → append body, check END_STREAM
//!   │    GOAWAY        → Err
//!   │    RST_STREAM    (our stream) → Err
//!   │    _             → ignore (unknown extensions, other streams)
//!   └→ Ok((status, headers, body))
//! ```
//!
//! ## Out of scope (deferred)
//!
//! - Concurrent streams (5A.5 pool multiplexing).
//! - Send-side flow control (outbound window tracking) — Phase 0 issues only
//!   GET requests with no request body, so send-window management is not needed.

use std::io::{Read, Write};

use lumen_core::error::Error;

use crate::h2::{
    frame::{
        Frame, FrameError, MAX_FRAME_PAYLOAD_DEFAULT, SETTING_HEADER_TABLE_SIZE,
        SETTING_INITIAL_WINDOW_SIZE, SETTING_MAX_FRAME_SIZE,
    },
    hpack::{Decoder, Encoder},
};
use crate::http::{H2Settings, HttpProfile};

/// Decoded HTTP response from an H2 fetch: `(status, headers, body)`.
pub type H2Response = (u16, Vec<(String, String)>, Vec<u8>);

// ── Constants ─────────────────────────────────────────────────────────────

/// Client connection preface magic (RFC 9113 §3.4).
const CLIENT_PREFACE_MAGIC: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// Default flow-control window size (RFC 9113 §6.9.2): 65 535 bytes.
const INITIAL_WINDOW: u32 = 65_535;

/// Read chunk size for `read_frame`.
const READ_CHUNK: usize = 8192;

// ── H2Conn ────────────────────────────────────────────────────────────────

/// Stateful HTTP/2 client connection.
///
/// One instance per TCP+TLS socket. After construction the connection preface
/// and SETTINGS exchange are complete; the caller can immediately call
/// [`H2Conn::fetch`].
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
}

impl<S: Read + Write> H2Conn<S> {
    /// Establish an HTTP/2 connection over `stream`.
    ///
    /// Sends the client connection preface (magic + Chrome-matching SETTINGS) and waits
    /// for the server's initial SETTINGS frame, sending the required ACK.
    /// Uses Chrome profile by default for broad server compatibility.
    pub fn connect(mut stream: S) -> Result<Self, Error> {
        // Client connection preface (RFC 9113 §3.4): magic + SETTINGS.
        let settings = H2Settings::for_profile(HttpProfile::Chrome);
        let mut preface = CLIENT_PREFACE_MAGIC.to_vec();

        // Build SETTINGS frame with Chrome-matching parameters.
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

        let mut conn = Self {
            stream,
            buf: Vec::new(),
            encoder: Encoder::new(),
            decoder: Decoder::new(),
            remote_max_frame: MAX_FRAME_PAYLOAD_DEFAULT,
            remote_init_window: INITIAL_WINDOW,
            next_stream_id: 1,
            conn_recv_window: INITIAL_WINDOW,
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
                // Server often sends an initial WINDOW_UPDATE for stream 0.
                Frame::WindowUpdate {
                    stream_id: 0,
                    increment: _,
                } => {}
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
        let sid = self.allocate_stream_id();

        // Build HPACK request header block.
        let mut req: Vec<(&[u8], &[u8])> = vec![
            (b":method", method.as_bytes()),
            (b":scheme", scheme.as_bytes()),
            (b":path", path.as_bytes()),
            (b":authority", authority.as_bytes()),
        ];
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

        // ── Receive response ───────────────────────────────────────────────
        let mut hdr_block: Vec<u8> = Vec::new();
        let mut end_headers = false;
        let mut end_stream = false;
        let mut body: Vec<u8> = Vec::new();

        while !end_headers || !end_stream {
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
        }

        // ── Decode response headers ────────────────────────────────────────
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

        let headers: Vec<(String, String)> = fields
            .into_iter()
            .filter(|f| !f.name.starts_with(b":"))
            .map(|f| {
                (
                    String::from_utf8_lossy(&f.name).into_owned(),
                    String::from_utf8_lossy(&f.value).into_owned(),
                )
            })
            .collect();

        Ok((status, headers, body))
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
}
