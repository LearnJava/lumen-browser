//! HTTP/2 frame codec — RFC 9113 §4.1 (frame layout) + §6 (10 frame types).
//!
//! Pure parse/serialize: no IO, no connection state. The connection layer
//! drives a buffer through [`Frame::parse`] and dispatches the parsed frame;
//! to send, the caller fills a [`Frame`] and calls [`Frame::encode`].
//!
//! ## Scope
//!
//! - Common 9-byte header (length 24 / type 8 / flags 8 / R+stream 32).
//! - All 10 frame types: DATA, HEADERS, PRIORITY, RST_STREAM, SETTINGS,
//!   PUSH_PROMISE, PING, GOAWAY, WINDOW_UPDATE, CONTINUATION.
//! - Padding stripping for DATA / HEADERS / PUSH_PROMISE.
//! - PRIORITY-on-HEADERS sub-payload.
//! - Unknown frame types preserved as [`Frame::Unknown`] — RFC 9113 §5.5
//!   requires the connection layer to ignore them, but surfacing them lets
//!   the layer decide rather than silently swallowing payload here.
//!
//! ## Out of scope (deferred to higher layers)
//!
//! - HPACK decoding of `block_fragment` — see HPACK module (5A.3).
//! - Semantic SETTINGS validation (`SETTINGS_ENABLE_PUSH ∈ {0,1}`, etc.).
//! - Stream-state machine and dependency-cycle checks.
//! - Padding generation on encode — the variants carry unpadded payload only;
//!   if a sender wants padding for traffic analysis, the connection layer
//!   wraps the payload before reaching the codec.

use std::fmt;

use lumen_core::error::Error;

// ── Constants ─────────────────────────────────────────────────────────────

/// Length of the common frame header (RFC 9113 §4.1).
pub const FRAME_HEADER_LEN: usize = 9;

/// Default SETTINGS_MAX_FRAME_SIZE (RFC 9113 §6.5.2): 2^14 = 16 384.
pub const MAX_FRAME_PAYLOAD_DEFAULT: u32 = 16_384;

/// Absolute upper bound on a frame payload (RFC 9113 §4.2): 2^24 − 1. A
/// `SETTINGS_MAX_FRAME_SIZE` advertised above this is itself a protocol error.
pub const MAX_FRAME_PAYLOAD_LIMIT: u32 = (1 << 24) - 1;

/// Maximum WINDOW_UPDATE increment / SETTINGS_INITIAL_WINDOW_SIZE (RFC 9113
/// §6.9.1): 2^31 − 1.
pub const MAX_FLOW_CONTROL_INCREMENT: u32 = (1 << 31) - 1;

// Frame type bytes (RFC 9113 §11.2 + IANA HTTP/2 registry).
pub const TYPE_DATA: u8 = 0x00;
pub const TYPE_HEADERS: u8 = 0x01;
pub const TYPE_PRIORITY: u8 = 0x02;
pub const TYPE_RST_STREAM: u8 = 0x03;
pub const TYPE_SETTINGS: u8 = 0x04;
pub const TYPE_PUSH_PROMISE: u8 = 0x05;
pub const TYPE_PING: u8 = 0x06;
pub const TYPE_GOAWAY: u8 = 0x07;
pub const TYPE_WINDOW_UPDATE: u8 = 0x08;
pub const TYPE_CONTINUATION: u8 = 0x09;

// Frame flag bits (RFC 9113 §6.*). The bit value can be shared across frame
// types (e.g. 0x01 is END_STREAM for DATA/HEADERS but ACK for SETTINGS/PING)
// — names below match the canonical use in each section.
pub const FLAG_ACK: u8 = 0x01;
pub const FLAG_END_STREAM: u8 = 0x01;
pub const FLAG_END_HEADERS: u8 = 0x04;
pub const FLAG_PADDED: u8 = 0x08;
pub const FLAG_PRIORITY: u8 = 0x20;

// SETTINGS parameter identifiers (RFC 9113 §6.5.2).
pub const SETTING_HEADER_TABLE_SIZE: u16 = 0x01;
pub const SETTING_ENABLE_PUSH: u16 = 0x02;
pub const SETTING_MAX_CONCURRENT_STREAMS: u16 = 0x03;
pub const SETTING_INITIAL_WINDOW_SIZE: u16 = 0x04;
pub const SETTING_MAX_FRAME_SIZE: u16 = 0x05;
pub const SETTING_MAX_HEADER_LIST_SIZE: u16 = 0x06;

// Common error codes (RFC 9113 §7) — surfaced as `u32` in RST_STREAM/GOAWAY.
pub const ERROR_NO_ERROR: u32 = 0x00;
pub const ERROR_PROTOCOL_ERROR: u32 = 0x01;
pub const ERROR_INTERNAL_ERROR: u32 = 0x02;
pub const ERROR_FLOW_CONTROL_ERROR: u32 = 0x03;
pub const ERROR_SETTINGS_TIMEOUT: u32 = 0x04;
pub const ERROR_STREAM_CLOSED: u32 = 0x05;
pub const ERROR_FRAME_SIZE_ERROR: u32 = 0x06;
pub const ERROR_REFUSED_STREAM: u32 = 0x07;
pub const ERROR_CANCEL: u32 = 0x08;
pub const ERROR_COMPRESSION_ERROR: u32 = 0x09;
pub const ERROR_CONNECT_ERROR: u32 = 0x0a;
pub const ERROR_ENHANCE_YOUR_CALM: u32 = 0x0b;
pub const ERROR_INADEQUATE_SECURITY: u32 = 0x0c;
pub const ERROR_HTTP_1_1_REQUIRED: u32 = 0x0d;

// ── Errors ────────────────────────────────────────────────────────────────

/// Codec-level error. The codec produces only two RFC 9113 §7 error codes on
/// its own:
///
/// - [`Self::FrameSize`] → `FRAME_SIZE_ERROR` (frame too large; fixed-size
///   frame of wrong length).
/// - [`Self::Protocol`] → `PROTOCOL_ERROR` (every other RFC violation: bad
///   stream id, padding overflow, malformed sub-fields).
///
/// The connection driver translates the variant into the wire error code and,
/// depending on context, sends `GOAWAY` (connection error) or `RST_STREAM`
/// (stream error). Semantic violations (e.g. unknown SETTINGS value) live in
/// the connection layer, not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    FrameSize(String),
    Protocol(String),
}

impl FrameError {
    /// Map to the RFC 9113 §7 wire error code.
    #[must_use]
    pub const fn code(&self) -> u32 {
        match self {
            Self::FrameSize(_) => ERROR_FRAME_SIZE_ERROR,
            Self::Protocol(_) => ERROR_PROTOCOL_ERROR,
        }
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameSize(s) => write!(f, "FRAME_SIZE_ERROR: {s}"),
            Self::Protocol(s) => write!(f, "PROTOCOL_ERROR: {s}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<FrameError> for Error {
    fn from(err: FrameError) -> Self {
        Self::Network(format!("h2 frame: {err}"))
    }
}

// ── PRIORITY payload ──────────────────────────────────────────────────────

/// Stream priority block — used by the PRIORITY frame and by HEADERS when the
/// PRIORITY flag is set (RFC 9113 §5.3.2 / §6.3). `weight` is the raw wire
/// byte (0..=255); the effective priority value is `weight + 1`.
///
/// PRIORITY is deprecated by RFC 9113 (the spec recommends ignoring it for
/// scheduling decisions) but remains legal on the wire, so the codec parses
/// and writes it faithfully.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priority {
    pub exclusive: bool,
    pub dependency: u32,
    pub weight: u8,
}

// ── Frame ─────────────────────────────────────────────────────────────────

/// Parsed/encodable HTTP/2 frame (RFC 9113 §6). For padded frames the carried
/// bytes are the unpadded data: the pad-length prefix and trailing padding
/// octets are stripped on parse and not regenerated on encode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// `DATA` (RFC 9113 §6.1).
    Data {
        stream_id: u32,
        end_stream: bool,
        data: Vec<u8>,
    },
    /// `HEADERS` (RFC 9113 §6.2). `block_fragment` is the raw HPACK output —
    /// HPACK decoding is the next layer's job.
    Headers {
        stream_id: u32,
        end_stream: bool,
        end_headers: bool,
        priority: Option<Priority>,
        block_fragment: Vec<u8>,
    },
    /// `PRIORITY` (RFC 9113 §6.3, deprecated but parsed).
    Priority {
        stream_id: u32,
        priority: Priority,
    },
    /// `RST_STREAM` (RFC 9113 §6.4).
    RstStream {
        stream_id: u32,
        error_code: u32,
    },
    /// `SETTINGS` (RFC 9113 §6.5). `params` is the raw (identifier, value)
    /// list — duplicate ids are preserved (last-wins semantics live above).
    Settings {
        ack: bool,
        params: Vec<(u16, u32)>,
    },
    /// `PUSH_PROMISE` (RFC 9113 §6.6).
    PushPromise {
        stream_id: u32,
        end_headers: bool,
        promised_stream_id: u32,
        block_fragment: Vec<u8>,
    },
    /// `PING` (RFC 9113 §6.7). Opaque payload is exactly 8 bytes; on ACK the
    /// peer mirrors the same bytes back.
    Ping {
        ack: bool,
        opaque_data: [u8; 8],
    },
    /// `GOAWAY` (RFC 9113 §6.8).
    Goaway {
        last_stream_id: u32,
        error_code: u32,
        debug_data: Vec<u8>,
    },
    /// `WINDOW_UPDATE` (RFC 9113 §6.9). `stream_id == 0` updates the
    /// connection-level window; non-zero updates a stream window. `increment`
    /// must be in `1..=2^31-1`; the codec rejects out-of-range on both parse
    /// and encode.
    WindowUpdate {
        stream_id: u32,
        increment: u32,
    },
    /// `CONTINUATION` (RFC 9113 §6.10).
    Continuation {
        stream_id: u32,
        end_headers: bool,
        block_fragment: Vec<u8>,
    },
    /// Extension or unknown type (RFC 9113 §5.5). The connection layer is
    /// expected to discard these unless it understands the extension.
    Unknown {
        type_byte: u8,
        flags: u8,
        stream_id: u32,
        payload: Vec<u8>,
    },
}

impl Frame {
    /// Frame type byte. For [`Self::Unknown`] this is the byte that came off
    /// the wire (or that the caller put in the variant).
    #[must_use]
    pub const fn type_byte(&self) -> u8 {
        match self {
            Self::Data { .. } => TYPE_DATA,
            Self::Headers { .. } => TYPE_HEADERS,
            Self::Priority { .. } => TYPE_PRIORITY,
            Self::RstStream { .. } => TYPE_RST_STREAM,
            Self::Settings { .. } => TYPE_SETTINGS,
            Self::PushPromise { .. } => TYPE_PUSH_PROMISE,
            Self::Ping { .. } => TYPE_PING,
            Self::Goaway { .. } => TYPE_GOAWAY,
            Self::WindowUpdate { .. } => TYPE_WINDOW_UPDATE,
            Self::Continuation { .. } => TYPE_CONTINUATION,
            Self::Unknown { type_byte, .. } => *type_byte,
        }
    }

    /// Stream identifier carried in the frame header. SETTINGS/PING/GOAWAY
    /// always wire `0` per RFC 9113 §6.5 / §6.7 / §6.8.
    #[must_use]
    pub const fn stream_id(&self) -> u32 {
        match self {
            Self::Data { stream_id, .. }
            | Self::Headers { stream_id, .. }
            | Self::Priority { stream_id, .. }
            | Self::RstStream { stream_id, .. }
            | Self::PushPromise { stream_id, .. }
            | Self::WindowUpdate { stream_id, .. }
            | Self::Continuation { stream_id, .. }
            | Self::Unknown { stream_id, .. } => *stream_id,
            Self::Settings { .. } | Self::Ping { .. } | Self::Goaway { .. } => 0,
        }
    }

    /// Parse one frame from `buf`.
    ///
    /// - `Ok(None)` — `buf` does not yet hold a complete frame (header +
    ///   payload). The caller buffers more bytes and tries again.
    /// - `Ok(Some((frame, consumed)))` — frame parsed; `consumed` is always
    ///   `FRAME_HEADER_LEN + payload_length`.
    /// - `Err(FrameError)` — RFC violation. The connection layer maps to a
    ///   wire error code and closes the connection or the stream.
    ///
    /// `max_payload_size` is the SETTINGS_MAX_FRAME_SIZE the *local* endpoint
    /// has advertised to the peer (clamped to [`MAX_FRAME_PAYLOAD_LIMIT`]).
    /// Before the local SETTINGS is sent, pass [`MAX_FRAME_PAYLOAD_DEFAULT`].
    pub fn parse(buf: &[u8], max_payload_size: u32) -> Result<Option<(Self, usize)>, FrameError> {
        if buf.len() < FRAME_HEADER_LEN {
            return Ok(None);
        }
        // Length is 24 bits big-endian, stored in the high three bytes — feed
        // a leading zero into from_be_bytes to widen it cleanly into u32.
        let length = u32::from_be_bytes([0, buf[0], buf[1], buf[2]]);
        let type_byte = buf[3];
        let flags = buf[4];
        let stream_id = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]) & 0x7fff_ffff;

        if length > max_payload_size {
            return Err(FrameError::FrameSize(format!(
                "payload {length} > SETTINGS_MAX_FRAME_SIZE {max_payload_size}"
            )));
        }
        let total = FRAME_HEADER_LEN + (length as usize);
        if buf.len() < total {
            return Ok(None);
        }
        let payload = &buf[FRAME_HEADER_LEN..total];

        let frame = match type_byte {
            TYPE_DATA => parse_data(stream_id, flags, payload)?,
            TYPE_HEADERS => parse_headers(stream_id, flags, payload)?,
            TYPE_PRIORITY => parse_priority(stream_id, payload)?,
            TYPE_RST_STREAM => parse_rst_stream(stream_id, payload)?,
            TYPE_SETTINGS => parse_settings(stream_id, flags, payload)?,
            TYPE_PUSH_PROMISE => parse_push_promise(stream_id, flags, payload)?,
            TYPE_PING => parse_ping(stream_id, flags, payload)?,
            TYPE_GOAWAY => parse_goaway(stream_id, payload)?,
            TYPE_WINDOW_UPDATE => parse_window_update(stream_id, payload)?,
            TYPE_CONTINUATION => parse_continuation(stream_id, flags, payload)?,
            _ => Self::Unknown {
                type_byte,
                flags,
                stream_id,
                payload: payload.to_vec(),
            },
        };
        Ok(Some((frame, total)))
    }

    /// Serialize the frame: append the 9-byte header and payload to `out`.
    ///
    /// Validates the format invariants that the codec is responsible for
    /// (stream id range, fixed-size payloads, WINDOW_UPDATE increment,
    /// SETTINGS_ACK has no params). Does not enforce the peer's
    /// SETTINGS_MAX_FRAME_SIZE — that's the caller's job because it depends on
    /// values negotiated above the codec layer. The absolute 2^24 − 1 limit is
    /// enforced.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        let start = out.len();
        out.extend_from_slice(&[0u8; FRAME_HEADER_LEN]);
        let (type_byte, flags, stream_id) = match self {
            Self::Data {
                stream_id,
                end_stream,
                data,
            } => {
                require_nonzero_stream(*stream_id, "DATA")?;
                out.extend_from_slice(data);
                let flags = if *end_stream { FLAG_END_STREAM } else { 0 };
                (TYPE_DATA, flags, *stream_id)
            }
            Self::Headers {
                stream_id,
                end_stream,
                end_headers,
                priority,
                block_fragment,
            } => {
                require_nonzero_stream(*stream_id, "HEADERS")?;
                let mut flags = 0u8;
                if *end_stream {
                    flags |= FLAG_END_STREAM;
                }
                if *end_headers {
                    flags |= FLAG_END_HEADERS;
                }
                if let Some(p) = priority {
                    flags |= FLAG_PRIORITY;
                    write_priority(out, p);
                }
                out.extend_from_slice(block_fragment);
                (TYPE_HEADERS, flags, *stream_id)
            }
            Self::Priority {
                stream_id,
                priority,
            } => {
                require_nonzero_stream(*stream_id, "PRIORITY")?;
                write_priority(out, priority);
                (TYPE_PRIORITY, 0, *stream_id)
            }
            Self::RstStream {
                stream_id,
                error_code,
            } => {
                require_nonzero_stream(*stream_id, "RST_STREAM")?;
                out.extend_from_slice(&error_code.to_be_bytes());
                (TYPE_RST_STREAM, 0, *stream_id)
            }
            Self::Settings { ack, params } => {
                if *ack && !params.is_empty() {
                    return Err(FrameError::FrameSize(
                        "SETTINGS ACK must have empty payload".into(),
                    ));
                }
                for (id, value) in params {
                    out.extend_from_slice(&id.to_be_bytes());
                    out.extend_from_slice(&value.to_be_bytes());
                }
                let flags = if *ack { FLAG_ACK } else { 0 };
                (TYPE_SETTINGS, flags, 0)
            }
            Self::PushPromise {
                stream_id,
                end_headers,
                promised_stream_id,
                block_fragment,
            } => {
                require_nonzero_stream(*stream_id, "PUSH_PROMISE")?;
                require_nonzero_stream(*promised_stream_id, "PUSH_PROMISE.promised_stream_id")?;
                require_31bit_stream(*promised_stream_id, "promised_stream_id")?;
                out.extend_from_slice(&(*promised_stream_id & 0x7fff_ffff).to_be_bytes());
                out.extend_from_slice(block_fragment);
                let flags = if *end_headers { FLAG_END_HEADERS } else { 0 };
                (TYPE_PUSH_PROMISE, flags, *stream_id)
            }
            Self::Ping { ack, opaque_data } => {
                out.extend_from_slice(opaque_data);
                let flags = if *ack { FLAG_ACK } else { 0 };
                (TYPE_PING, flags, 0)
            }
            Self::Goaway {
                last_stream_id,
                error_code,
                debug_data,
            } => {
                require_31bit_stream(*last_stream_id, "last_stream_id")?;
                out.extend_from_slice(&(*last_stream_id & 0x7fff_ffff).to_be_bytes());
                out.extend_from_slice(&error_code.to_be_bytes());
                out.extend_from_slice(debug_data);
                (TYPE_GOAWAY, 0, 0)
            }
            Self::WindowUpdate {
                stream_id,
                increment,
            } => {
                if *increment == 0 || *increment > MAX_FLOW_CONTROL_INCREMENT {
                    return Err(FrameError::Protocol(format!(
                        "WINDOW_UPDATE increment must be in 1..=2^31-1, got {increment}"
                    )));
                }
                out.extend_from_slice(&increment.to_be_bytes());
                (TYPE_WINDOW_UPDATE, 0, *stream_id)
            }
            Self::Continuation {
                stream_id,
                end_headers,
                block_fragment,
            } => {
                require_nonzero_stream(*stream_id, "CONTINUATION")?;
                out.extend_from_slice(block_fragment);
                let flags = if *end_headers { FLAG_END_HEADERS } else { 0 };
                (TYPE_CONTINUATION, flags, *stream_id)
            }
            Self::Unknown {
                type_byte,
                flags,
                stream_id,
                payload,
            } => {
                out.extend_from_slice(payload);
                (*type_byte, *flags, *stream_id)
            }
        };

        let payload_len = out.len() - start - FRAME_HEADER_LEN;
        if payload_len > MAX_FRAME_PAYLOAD_LIMIT as usize {
            return Err(FrameError::FrameSize(format!(
                "frame payload {payload_len} > absolute limit {MAX_FRAME_PAYLOAD_LIMIT}"
            )));
        }
        require_31bit_stream(stream_id, "stream_id")?;

        // Big-endian 24-bit length: payload_len fits in u32 because we just
        // checked it's at most MAX_FRAME_PAYLOAD_LIMIT < 2^24.
        let len_be = (payload_len as u32).to_be_bytes();
        out[start] = len_be[1];
        out[start + 1] = len_be[2];
        out[start + 2] = len_be[3];
        out[start + 3] = type_byte;
        out[start + 4] = flags;
        out[start + 5..start + 9].copy_from_slice(&(stream_id & 0x7fff_ffff).to_be_bytes());
        Ok(())
    }
}

// ── Parsing helpers ───────────────────────────────────────────────────────

fn parse_data(stream_id: u32, flags: u8, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id == 0 {
        return Err(FrameError::Protocol("DATA on stream 0".into()));
    }
    let end_stream = flags & FLAG_END_STREAM != 0;
    let data = strip_padding(flags, payload)?;
    Ok(Frame::Data {
        stream_id,
        end_stream,
        data: data.to_vec(),
    })
}

fn parse_headers(stream_id: u32, flags: u8, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id == 0 {
        return Err(FrameError::Protocol("HEADERS on stream 0".into()));
    }
    let end_stream = flags & FLAG_END_STREAM != 0;
    let end_headers = flags & FLAG_END_HEADERS != 0;
    let mut rest = strip_padding(flags, payload)?;
    let priority = if flags & FLAG_PRIORITY != 0 {
        if rest.len() < 5 {
            return Err(FrameError::Protocol(
                "HEADERS with PRIORITY flag but < 5 bytes of priority payload".into(),
            ));
        }
        let p = parse_priority_fields(&rest[..5]);
        rest = &rest[5..];
        Some(p)
    } else {
        None
    };
    Ok(Frame::Headers {
        stream_id,
        end_stream,
        end_headers,
        priority,
        block_fragment: rest.to_vec(),
    })
}

fn parse_priority(stream_id: u32, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id == 0 {
        return Err(FrameError::Protocol("PRIORITY on stream 0".into()));
    }
    if payload.len() != 5 {
        return Err(FrameError::FrameSize(format!(
            "PRIORITY length must be 5, got {}",
            payload.len()
        )));
    }
    Ok(Frame::Priority {
        stream_id,
        priority: parse_priority_fields(payload),
    })
}

fn parse_rst_stream(stream_id: u32, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id == 0 {
        return Err(FrameError::Protocol("RST_STREAM on stream 0".into()));
    }
    if payload.len() != 4 {
        return Err(FrameError::FrameSize(format!(
            "RST_STREAM length must be 4, got {}",
            payload.len()
        )));
    }
    let error_code = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    Ok(Frame::RstStream {
        stream_id,
        error_code,
    })
}

fn parse_settings(stream_id: u32, flags: u8, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id != 0 {
        return Err(FrameError::Protocol(format!(
            "SETTINGS on non-zero stream {stream_id}"
        )));
    }
    let ack = flags & FLAG_ACK != 0;
    if ack && !payload.is_empty() {
        return Err(FrameError::FrameSize(format!(
            "SETTINGS ACK must have empty payload, got {} bytes",
            payload.len()
        )));
    }
    if !payload.len().is_multiple_of(6) {
        return Err(FrameError::FrameSize(format!(
            "SETTINGS length must be a multiple of 6, got {}",
            payload.len()
        )));
    }
    let mut params = Vec::with_capacity(payload.len() / 6);
    for chunk in payload.chunks_exact(6) {
        let id = u16::from_be_bytes([chunk[0], chunk[1]]);
        let value = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
        params.push((id, value));
    }
    Ok(Frame::Settings { ack, params })
}

fn parse_push_promise(stream_id: u32, flags: u8, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id == 0 {
        return Err(FrameError::Protocol("PUSH_PROMISE on stream 0".into()));
    }
    let end_headers = flags & FLAG_END_HEADERS != 0;
    let rest = strip_padding(flags, payload)?;
    if rest.len() < 4 {
        return Err(FrameError::Protocol(
            "PUSH_PROMISE payload too short for promised stream id".into(),
        ));
    }
    let promised_stream_id =
        u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) & 0x7fff_ffff;
    if promised_stream_id == 0 {
        return Err(FrameError::Protocol(
            "PUSH_PROMISE with promised_stream_id == 0".into(),
        ));
    }
    Ok(Frame::PushPromise {
        stream_id,
        end_headers,
        promised_stream_id,
        block_fragment: rest[4..].to_vec(),
    })
}

fn parse_ping(stream_id: u32, flags: u8, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id != 0 {
        return Err(FrameError::Protocol(format!(
            "PING on non-zero stream {stream_id}"
        )));
    }
    if payload.len() != 8 {
        return Err(FrameError::FrameSize(format!(
            "PING length must be 8, got {}",
            payload.len()
        )));
    }
    let ack = flags & FLAG_ACK != 0;
    let mut opaque = [0u8; 8];
    opaque.copy_from_slice(payload);
    Ok(Frame::Ping {
        ack,
        opaque_data: opaque,
    })
}

fn parse_goaway(stream_id: u32, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id != 0 {
        return Err(FrameError::Protocol(format!(
            "GOAWAY on non-zero stream {stream_id}"
        )));
    }
    if payload.len() < 8 {
        return Err(FrameError::FrameSize(format!(
            "GOAWAY length must be >= 8, got {}",
            payload.len()
        )));
    }
    let last_stream_id =
        u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) & 0x7fff_ffff;
    let error_code = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let debug_data = payload[8..].to_vec();
    Ok(Frame::Goaway {
        last_stream_id,
        error_code,
        debug_data,
    })
}

fn parse_window_update(stream_id: u32, payload: &[u8]) -> Result<Frame, FrameError> {
    if payload.len() != 4 {
        return Err(FrameError::FrameSize(format!(
            "WINDOW_UPDATE length must be 4, got {}",
            payload.len()
        )));
    }
    let increment =
        u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) & 0x7fff_ffff;
    if increment == 0 {
        return Err(FrameError::Protocol(
            "WINDOW_UPDATE increment must not be zero".into(),
        ));
    }
    Ok(Frame::WindowUpdate {
        stream_id,
        increment,
    })
}

fn parse_continuation(stream_id: u32, flags: u8, payload: &[u8]) -> Result<Frame, FrameError> {
    if stream_id == 0 {
        return Err(FrameError::Protocol("CONTINUATION on stream 0".into()));
    }
    let end_headers = flags & FLAG_END_HEADERS != 0;
    Ok(Frame::Continuation {
        stream_id,
        end_headers,
        block_fragment: payload.to_vec(),
    })
}

fn parse_priority_fields(buf: &[u8]) -> Priority {
    let raw = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    Priority {
        exclusive: raw & 0x8000_0000 != 0,
        dependency: raw & 0x7fff_ffff,
        weight: buf[4],
    }
}

/// Strip optional padding from a PADDED-capable frame payload. RFC 9113 §6.1:
/// the first byte (when PADDED is set) is the pad length; the last
/// `pad_length` bytes are padding. If the pad length byte is `>=` the payload
/// length, that's a PROTOCOL_ERROR.
fn strip_padding(flags: u8, payload: &[u8]) -> Result<&[u8], FrameError> {
    if flags & FLAG_PADDED == 0 {
        return Ok(payload);
    }
    let Some((pad_byte, rest)) = payload.split_first() else {
        return Err(FrameError::Protocol(
            "PADDED frame with empty payload (no pad-length byte)".into(),
        ));
    };
    let pad_len = *pad_byte as usize;
    if pad_len > rest.len() {
        return Err(FrameError::Protocol(format!(
            "padding {pad_len} >= payload {} (RFC 9113 §6.1)",
            payload.len()
        )));
    }
    Ok(&rest[..rest.len() - pad_len])
}

// ── Encoding helpers ──────────────────────────────────────────────────────

fn write_priority(out: &mut Vec<u8>, p: &Priority) {
    let mut raw = p.dependency & 0x7fff_ffff;
    if p.exclusive {
        raw |= 0x8000_0000;
    }
    out.extend_from_slice(&raw.to_be_bytes());
    out.push(p.weight);
}

fn require_nonzero_stream(stream_id: u32, frame: &str) -> Result<(), FrameError> {
    if stream_id == 0 {
        Err(FrameError::Protocol(format!(
            "{frame} requires non-zero stream id"
        )))
    } else {
        Ok(())
    }
}

fn require_31bit_stream(stream_id: u32, field: &str) -> Result<(), FrameError> {
    if stream_id & 0x8000_0000 != 0 {
        Err(FrameError::Protocol(format!(
            "{field} = {stream_id:#x} exceeds 31 bits"
        )))
    } else {
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(frame: &Frame) -> Frame {
        let mut buf = Vec::new();
        frame.encode(&mut buf).expect("encode");
        let (parsed, consumed) =
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_LIMIT).expect("parse").expect("complete");
        assert_eq!(consumed, buf.len(), "parsed bytes mismatch");
        parsed
    }

    // ── Header / common ───────────────────────────────────────────────────

    #[test]
    fn parse_returns_none_when_header_incomplete() {
        for i in 0..FRAME_HEADER_LEN {
            let buf = vec![0u8; i];
            assert_eq!(Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap(), None);
        }
    }

    #[test]
    fn parse_returns_none_when_payload_incomplete() {
        // A SETTINGS frame, length=6, but only header present.
        let mut buf = vec![0, 0, 6, TYPE_SETTINGS, 0, 0, 0, 0, 0];
        assert_eq!(Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap(), None);
        // Add a partial payload (3 of 6 bytes) — still not complete.
        buf.extend_from_slice(&[0; 3]);
        assert_eq!(Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap(), None);
    }

    #[test]
    fn parse_rejects_oversize_payload() {
        // Declare length=17000, max=16384.
        let buf = [0, 0x42, 0x68, TYPE_DATA, 0, 0, 0, 0, 1];
        let err = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap_err();
        assert!(matches!(err, FrameError::FrameSize(_)), "got {err:?}");
        assert_eq!(err.code(), ERROR_FRAME_SIZE_ERROR);
    }

    #[test]
    fn parse_ignores_reserved_high_bit_of_stream_id() {
        // PING with R=1 in stream id; codec must mask it out and accept.
        let mut buf = vec![0, 0, 8, TYPE_PING, 0, 0x80, 0, 0, 0];
        buf.extend_from_slice(&[0u8; 8]);
        let (frame, _) = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap().unwrap();
        assert_eq!(frame, Frame::Ping { ack: false, opaque_data: [0; 8] });
    }

    #[test]
    fn encode_clears_reserved_high_bit() {
        // Caller passed a 32-bit stream id with the high bit set — encoder
        // should refuse rather than silently mask, to catch caller bugs.
        let bad = Frame::Data {
            stream_id: 0xffff_ffff,
            end_stream: false,
            data: b"x".to_vec(),
        };
        let mut buf = Vec::new();
        let err = bad.encode(&mut buf).unwrap_err();
        assert!(matches!(err, FrameError::Protocol(_)), "got {err:?}");
    }

    #[test]
    fn unknown_frame_round_trips() {
        let f = Frame::Unknown {
            type_byte: 0xfe,
            flags: 0x42,
            stream_id: 9,
            payload: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(round_trip(&f), f);
    }

    // ── DATA ──────────────────────────────────────────────────────────────

    #[test]
    fn data_round_trip() {
        let f = Frame::Data {
            stream_id: 7,
            end_stream: true,
            data: b"hello world".to_vec(),
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn data_padded_strips_correctly() {
        // PADDED DATA on the wire: pad_len=3, "abc", 3*0x00.
        let payload = vec![3, b'a', b'b', b'c', 0, 0, 0];
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        let mut buf = vec![len[1], len[2], len[3], TYPE_DATA, FLAG_PADDED, 0, 0, 0, 1];
        buf.extend(payload);
        let (f, _) = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap().unwrap();
        assert_eq!(
            f,
            Frame::Data {
                stream_id: 1,
                end_stream: false,
                data: b"abc".to_vec(),
            }
        );
    }

    #[test]
    fn data_padded_rejects_oversize_padding() {
        // pad_len = 5, payload = 5 bytes total (1 pad-length + 4 data) — pad
        // overflows.
        let payload = vec![5, 0, 0, 0, 0];
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        let mut buf = vec![len[1], len[2], len[3], TYPE_DATA, FLAG_PADDED, 0, 0, 0, 1];
        buf.extend(payload);
        let err = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap_err();
        assert!(matches!(err, FrameError::Protocol(_)));
    }

    #[test]
    fn data_padded_rejects_empty_payload() {
        // PADDED flag, length=0 → no room for pad-length byte.
        let buf = [0, 0, 0, TYPE_DATA, FLAG_PADDED, 0, 0, 0, 1];
        let err = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap_err();
        assert!(matches!(err, FrameError::Protocol(_)));
    }

    #[test]
    fn data_rejects_stream_zero_on_parse() {
        let buf = [0, 0, 1, TYPE_DATA, 0, 0, 0, 0, 0, 0xaa];
        let err = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap_err();
        assert!(matches!(err, FrameError::Protocol(_)));
    }

    #[test]
    fn data_rejects_stream_zero_on_encode() {
        let f = Frame::Data {
            stream_id: 0,
            end_stream: false,
            data: vec![],
        };
        let mut out = Vec::new();
        assert!(matches!(f.encode(&mut out), Err(FrameError::Protocol(_))));
    }

    // ── HEADERS ───────────────────────────────────────────────────────────

    #[test]
    fn headers_round_trip_minimal() {
        let f = Frame::Headers {
            stream_id: 3,
            end_stream: true,
            end_headers: true,
            priority: None,
            block_fragment: vec![0x88, 0x77, 0x66],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn headers_round_trip_with_priority() {
        let f = Frame::Headers {
            stream_id: 5,
            end_stream: false,
            end_headers: true,
            priority: Some(Priority {
                exclusive: true,
                dependency: 3,
                weight: 15,
            }),
            block_fragment: vec![0xab, 0xcd],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn headers_priority_flag_short_payload_rejected() {
        // PRIORITY flag set but only 4 bytes (need 5).
        let payload = vec![0, 0, 0, 1];
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        let mut buf = vec![
            len[1],
            len[2],
            len[3],
            TYPE_HEADERS,
            FLAG_PRIORITY,
            0,
            0,
            0,
            1,
        ];
        buf.extend(payload);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::Protocol(_))
        ));
    }

    // ── PRIORITY ──────────────────────────────────────────────────────────

    #[test]
    fn priority_round_trip() {
        let f = Frame::Priority {
            stream_id: 11,
            priority: Priority {
                exclusive: false,
                dependency: 7,
                weight: 0,
            },
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn priority_wrong_length_rejected() {
        let payload = vec![0; 4];
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        let mut buf = vec![len[1], len[2], len[3], TYPE_PRIORITY, 0, 0, 0, 0, 1];
        buf.extend(payload);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::FrameSize(_))
        ));
    }

    // ── RST_STREAM ────────────────────────────────────────────────────────

    #[test]
    fn rst_stream_round_trip() {
        let f = Frame::RstStream {
            stream_id: 9,
            error_code: ERROR_CANCEL,
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn rst_stream_wrong_length_rejected() {
        let payload = vec![0; 3];
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        let mut buf = vec![len[1], len[2], len[3], TYPE_RST_STREAM, 0, 0, 0, 0, 1];
        buf.extend(payload);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::FrameSize(_))
        ));
    }

    // ── SETTINGS ──────────────────────────────────────────────────────────

    #[test]
    fn settings_round_trip_params() {
        let f = Frame::Settings {
            ack: false,
            params: vec![
                (SETTING_HEADER_TABLE_SIZE, 4096),
                (SETTING_ENABLE_PUSH, 0),
                (SETTING_MAX_CONCURRENT_STREAMS, 100),
                (SETTING_INITIAL_WINDOW_SIZE, 65_535),
                (SETTING_MAX_FRAME_SIZE, 16_384),
                (SETTING_MAX_HEADER_LIST_SIZE, 8192),
            ],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn settings_ack_round_trip() {
        let f = Frame::Settings {
            ack: true,
            params: vec![],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn settings_ack_with_params_rejected_on_encode() {
        let bad = Frame::Settings {
            ack: true,
            params: vec![(SETTING_ENABLE_PUSH, 0)],
        };
        let mut out = Vec::new();
        assert!(matches!(
            bad.encode(&mut out),
            Err(FrameError::FrameSize(_))
        ));
    }

    #[test]
    fn settings_ack_with_payload_rejected_on_parse() {
        // ACK flag + non-empty payload.
        let mut buf = vec![0, 0, 6, TYPE_SETTINGS, FLAG_ACK, 0, 0, 0, 0];
        buf.extend(&[0u8; 6]);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::FrameSize(_))
        ));
    }

    #[test]
    fn settings_non_multiple_of_six_rejected() {
        let buf = vec![0, 0, 5, TYPE_SETTINGS, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::FrameSize(_))
        ));
    }

    #[test]
    fn settings_non_zero_stream_rejected() {
        let buf = vec![0, 0, 0, TYPE_SETTINGS, 0, 0, 0, 0, 1];
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::Protocol(_))
        ));
    }

    // ── PUSH_PROMISE ──────────────────────────────────────────────────────

    #[test]
    fn push_promise_round_trip() {
        let f = Frame::PushPromise {
            stream_id: 3,
            end_headers: true,
            promised_stream_id: 4,
            block_fragment: vec![0xde, 0xad, 0xbe, 0xef],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn push_promise_padded_strips() {
        // pad_len=2, promised=4, fragment="xy", padding 2*0x00.
        let mut payload = Vec::new();
        payload.push(2u8);
        payload.extend_from_slice(&4u32.to_be_bytes());
        payload.extend_from_slice(b"xy");
        payload.extend_from_slice(&[0, 0]);
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        let mut buf = vec![
            len[1],
            len[2],
            len[3],
            TYPE_PUSH_PROMISE,
            FLAG_PADDED | FLAG_END_HEADERS,
            0,
            0,
            0,
            3,
        ];
        buf.extend(payload);
        let (f, _) = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap().unwrap();
        assert_eq!(
            f,
            Frame::PushPromise {
                stream_id: 3,
                end_headers: true,
                promised_stream_id: 4,
                block_fragment: b"xy".to_vec(),
            }
        );
    }

    #[test]
    fn push_promise_zero_promised_stream_rejected() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u32.to_be_bytes());
        let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
        let mut buf = vec![len[1], len[2], len[3], TYPE_PUSH_PROMISE, 0, 0, 0, 0, 1];
        buf.extend(payload);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::Protocol(_))
        ));
    }

    // ── PING ──────────────────────────────────────────────────────────────

    #[test]
    fn ping_round_trip() {
        let f = Frame::Ping {
            ack: false,
            opaque_data: [1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn ping_ack_round_trip() {
        let f = Frame::Ping {
            ack: true,
            opaque_data: [0xaa; 8],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn ping_wrong_length_rejected() {
        let mut buf = vec![0, 0, 7, TYPE_PING, 0, 0, 0, 0, 0];
        buf.extend(&[0u8; 7]);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::FrameSize(_))
        ));
    }

    #[test]
    fn ping_non_zero_stream_rejected() {
        let mut buf = vec![0, 0, 8, TYPE_PING, 0, 0, 0, 0, 7];
        buf.extend(&[0u8; 8]);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::Protocol(_))
        ));
    }

    // ── GOAWAY ────────────────────────────────────────────────────────────

    #[test]
    fn goaway_round_trip_no_debug() {
        let f = Frame::Goaway {
            last_stream_id: 13,
            error_code: ERROR_NO_ERROR,
            debug_data: vec![],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn goaway_round_trip_with_debug() {
        let f = Frame::Goaway {
            last_stream_id: 99,
            error_code: ERROR_PROTOCOL_ERROR,
            debug_data: b"server going down".to_vec(),
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn goaway_too_short_rejected() {
        let mut buf = vec![0, 0, 7, TYPE_GOAWAY, 0, 0, 0, 0, 0];
        buf.extend(&[0u8; 7]);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::FrameSize(_))
        ));
    }

    #[test]
    fn goaway_non_zero_stream_rejected() {
        let mut buf = vec![0, 0, 8, TYPE_GOAWAY, 0, 0, 0, 0, 1];
        buf.extend(&[0u8; 8]);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::Protocol(_))
        ));
    }

    // ── WINDOW_UPDATE ─────────────────────────────────────────────────────

    #[test]
    fn window_update_connection_level_round_trip() {
        let f = Frame::WindowUpdate {
            stream_id: 0,
            increment: 65_535,
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn window_update_stream_level_round_trip() {
        let f = Frame::WindowUpdate {
            stream_id: 1,
            increment: 1,
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn window_update_zero_increment_rejected_on_parse() {
        let mut buf = vec![0, 0, 4, TYPE_WINDOW_UPDATE, 0, 0, 0, 0, 1];
        buf.extend(&[0u8; 4]);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::Protocol(_))
        ));
    }

    #[test]
    fn window_update_zero_increment_rejected_on_encode() {
        let f = Frame::WindowUpdate {
            stream_id: 0,
            increment: 0,
        };
        let mut out = Vec::new();
        assert!(matches!(f.encode(&mut out), Err(FrameError::Protocol(_))));
    }

    #[test]
    fn window_update_wrong_length_rejected() {
        let mut buf = vec![0, 0, 3, TYPE_WINDOW_UPDATE, 0, 0, 0, 0, 1];
        buf.extend(&[0u8; 3]);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::FrameSize(_))
        ));
    }

    // ── CONTINUATION ──────────────────────────────────────────────────────

    #[test]
    fn continuation_round_trip() {
        let f = Frame::Continuation {
            stream_id: 7,
            end_headers: false,
            block_fragment: vec![0xc0, 0xff, 0xee],
        };
        assert_eq!(round_trip(&f), f);
    }

    #[test]
    fn continuation_stream_zero_rejected() {
        let mut buf = vec![0, 0, 1, TYPE_CONTINUATION, 0, 0, 0, 0, 0];
        buf.push(0xaa);
        assert!(matches!(
            Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT),
            Err(FrameError::Protocol(_))
        ));
    }

    // ── Sequential parsing ────────────────────────────────────────────────

    #[test]
    fn parse_consumes_only_one_frame_per_call() {
        let mut buf = Vec::new();
        Frame::Ping {
            ack: false,
            opaque_data: [1; 8],
        }
        .encode(&mut buf)
        .unwrap();
        let after_first = buf.len();
        Frame::WindowUpdate {
            stream_id: 0,
            increment: 42,
        }
        .encode(&mut buf)
        .unwrap();

        let (f1, consumed1) = Frame::parse(&buf, MAX_FRAME_PAYLOAD_DEFAULT).unwrap().unwrap();
        assert_eq!(consumed1, after_first);
        assert!(matches!(f1, Frame::Ping { .. }));

        let (f2, consumed2) = Frame::parse(&buf[consumed1..], MAX_FRAME_PAYLOAD_DEFAULT)
            .unwrap()
            .unwrap();
        assert_eq!(consumed1 + consumed2, buf.len());
        assert!(matches!(f2, Frame::WindowUpdate { increment: 42, .. }));
    }

    // ── Diagnostics helpers ───────────────────────────────────────────────

    #[test]
    fn type_byte_matches_variant() {
        assert_eq!(
            Frame::Data {
                stream_id: 1,
                end_stream: false,
                data: vec![]
            }
            .type_byte(),
            TYPE_DATA
        );
        assert_eq!(
            Frame::Goaway {
                last_stream_id: 0,
                error_code: 0,
                debug_data: vec![]
            }
            .type_byte(),
            TYPE_GOAWAY
        );
        assert_eq!(
            Frame::Unknown {
                type_byte: 0xab,
                flags: 0,
                stream_id: 0,
                payload: vec![]
            }
            .type_byte(),
            0xab
        );
    }

    #[test]
    fn frame_error_codes_match_rfc() {
        assert_eq!(
            FrameError::Protocol("x".into()).code(),
            ERROR_PROTOCOL_ERROR
        );
        assert_eq!(
            FrameError::FrameSize("x".into()).code(),
            ERROR_FRAME_SIZE_ERROR
        );
    }
}
