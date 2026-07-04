//! HTTP/3 frame codec — RFC 9114 §7.2 (frame layout + frame types).
//!
//! Every HTTP/3 frame on a stream is `Type (varint) · Length (varint) ·
//! Frame Payload (Length bytes)` (RFC 9114 §7.1). This module is a pure
//! parse/serialize layer over that shape — no IO, no stream state, no QPACK.
//! The connection layer feeds a byte buffer through [`Frame::parse`] and
//! dispatches; to send, it fills a [`Frame`] and calls [`Frame::encode`].
//!
//! ## Scope
//!
//! - All request/control-stream frame types: DATA, HEADERS, CANCEL_PUSH,
//!   SETTINGS, PUSH_PROMISE, GOAWAY, MAX_PUSH_ID.
//! - Reserved HTTP/2 frame types (0x02/0x06/0x08/0x09) rejected as
//!   `H3_FRAME_UNEXPECTED` (RFC 9114 §11.2.1).
//! - Unknown / greased frame types (RFC 9114 §7.2.8) surfaced as
//!   [`Frame::Reserved`] so the connection layer can ignore them by policy
//!   rather than the codec silently swallowing payload.
//! - SETTINGS validation the codec can do locally: reserved HTTP/2 setting
//!   identifiers and duplicate identifiers → `H3_SETTINGS_ERROR`
//!   (RFC 9114 §7.2.4.1).
//!
//! ## Out of scope (deferred to higher layers)
//!
//! - QPACK decoding of the HEADERS / PUSH_PROMISE field section — the field
//!   block is carried opaquely (see the QPACK slice, RFC 9204).
//! - Stream-type framing, unidirectional-stream setup, request/response
//!   semantics, and per-stream frame-ordering rules (RFC 9114 §7.1, §6.2).
//! - Whole-frame buffering: a huge DATA frame is returned only once its full
//!   `Length` is buffered. The connection layer bounds control-frame sizes and
//!   streams large bodies below this codec.

use std::collections::HashSet;
use std::fmt;

use super::varint;

// ── Frame type codes (RFC 9114 §7.2 + §11.2.1 IANA registry) ────────────────

/// DATA frame (RFC 9114 §7.2.1).
pub const TYPE_DATA: u64 = 0x00;
/// HEADERS frame (RFC 9114 §7.2.2) — carries a QPACK-encoded field section.
pub const TYPE_HEADERS: u64 = 0x01;
/// CANCEL_PUSH frame (RFC 9114 §7.2.3).
pub const TYPE_CANCEL_PUSH: u64 = 0x03;
/// SETTINGS frame (RFC 9114 §7.2.4).
pub const TYPE_SETTINGS: u64 = 0x04;
/// PUSH_PROMISE frame (RFC 9114 §7.2.5).
pub const TYPE_PUSH_PROMISE: u64 = 0x05;
/// GOAWAY frame (RFC 9114 §7.2.6).
pub const TYPE_GOAWAY: u64 = 0x07;
/// MAX_PUSH_ID frame (RFC 9114 §7.2.7).
pub const TYPE_MAX_PUSH_ID: u64 = 0x0d;

// ── SETTINGS parameter identifiers (RFC 9114 §7.2.4.1 + RFC 9204 §5) ─────────

/// SETTINGS_QPACK_MAX_TABLE_CAPACITY (RFC 9204 §5).
pub const SETTING_QPACK_MAX_TABLE_CAPACITY: u64 = 0x01;
/// SETTINGS_MAX_FIELD_SECTION_SIZE (RFC 9114 §7.2.4.1).
pub const SETTING_MAX_FIELD_SECTION_SIZE: u64 = 0x06;
/// SETTINGS_QPACK_BLOCKED_STREAMS (RFC 9204 §5).
pub const SETTING_QPACK_BLOCKED_STREAMS: u64 = 0x07;
/// SETTINGS_ENABLE_CONNECT_PROTOCOL (RFC 9220 §3).
pub const SETTING_ENABLE_CONNECT_PROTOCOL: u64 = 0x08;

// ── HTTP/3 error codes (RFC 9114 §8.1) ──────────────────────────────────────

/// H3_NO_ERROR (RFC 9114 §8.1).
pub const H3_NO_ERROR: u64 = 0x0100;
/// H3_GENERAL_PROTOCOL_ERROR (RFC 9114 §8.1).
pub const H3_GENERAL_PROTOCOL_ERROR: u64 = 0x0101;
/// H3_INTERNAL_ERROR (RFC 9114 §8.1).
pub const H3_INTERNAL_ERROR: u64 = 0x0102;
/// H3_FRAME_UNEXPECTED (RFC 9114 §8.1) — a frame not permitted in the current
/// state, including a reserved HTTP/2 frame type.
pub const H3_FRAME_UNEXPECTED: u64 = 0x0105;
/// H3_FRAME_ERROR (RFC 9114 §8.1) — a malformed frame.
pub const H3_FRAME_ERROR: u64 = 0x0106;
/// H3_ID_ERROR (RFC 9114 §8.1).
pub const H3_ID_ERROR: u64 = 0x0108;
/// H3_SETTINGS_ERROR (RFC 9114 §8.1) — invalid SETTINGS content.
pub const H3_SETTINGS_ERROR: u64 = 0x0109;

// ── Errors ──────────────────────────────────────────────────────────────────

/// Codec-level error. Each variant maps to exactly one RFC 9114 §8.1 wire error
/// code via [`FrameError::code`]; the connection layer emits it in a
/// `CONNECTION_CLOSE` / `STOP_SENDING` as appropriate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Malformed frame (truncated or over-long fixed field) → `H3_FRAME_ERROR`.
    Frame(String),
    /// Reserved HTTP/2 frame type received → `H3_FRAME_UNEXPECTED`.
    Unexpected(String),
    /// Invalid SETTINGS content (reserved or duplicate id) → `H3_SETTINGS_ERROR`.
    Settings(String),
}

impl FrameError {
    /// Map to the RFC 9114 §8.1 wire error code.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::Frame(_) => H3_FRAME_ERROR,
            Self::Unexpected(_) => H3_FRAME_UNEXPECTED,
            Self::Settings(_) => H3_SETTINGS_ERROR,
        }
    }

    /// Construct a [`FrameError::Frame`] (`H3_FRAME_ERROR`).
    fn frame(msg: impl Into<String>) -> Self {
        Self::Frame(msg.into())
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(m) => write!(f, "H3_FRAME_ERROR: {m}"),
            Self::Unexpected(m) => write!(f, "H3_FRAME_UNEXPECTED: {m}"),
            Self::Settings(m) => write!(f, "H3_SETTINGS_ERROR: {m}"),
        }
    }
}

impl std::error::Error for FrameError {}

// ── Frame ───────────────────────────────────────────────────────────────────

/// A parsed HTTP/3 frame (RFC 9114 §7.2). Field sections in `Headers` /
/// `PushPromise` remain QPACK-encoded — decoding is a higher layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// DATA — an opaque chunk of the message body (RFC 9114 §7.2.1).
    Data(Vec<u8>),
    /// HEADERS — a QPACK-encoded field section (RFC 9114 §7.2.2).
    Headers(Vec<u8>),
    /// CANCEL_PUSH — abandon a server push, identified by its push ID
    /// (RFC 9114 §7.2.3).
    CancelPush(u64),
    /// SETTINGS — configuration parameters as `(identifier, value)` pairs
    /// (RFC 9114 §7.2.4). Order is preserved as sent.
    Settings(Vec<(u64, u64)>),
    /// PUSH_PROMISE — a promised push ID plus its QPACK field section
    /// (RFC 9114 §7.2.5).
    PushPromise {
        /// The push ID this promise reserves.
        push_id: u64,
        /// QPACK-encoded field section of the promised request.
        block: Vec<u8>,
    },
    /// GOAWAY — graceful shutdown, carrying the last stream ID (from server) or
    /// push ID (from client) that will be processed (RFC 9114 §7.2.6).
    GoAway(u64),
    /// MAX_PUSH_ID — the maximum push ID the client will accept
    /// (RFC 9114 §7.2.7).
    MaxPushId(u64),
    /// A reserved or greased frame type the connection layer must ignore
    /// (RFC 9114 §7.2.8). Preserves the raw type code and payload rather than
    /// swallowing them in the codec.
    Reserved {
        /// The raw frame type code.
        frame_type: u64,
        /// The unparsed frame payload.
        payload: Vec<u8>,
    },
}

impl Frame {
    /// The wire frame type code this variant serializes to.
    #[must_use]
    pub const fn frame_type(&self) -> u64 {
        match self {
            Self::Data(_) => TYPE_DATA,
            Self::Headers(_) => TYPE_HEADERS,
            Self::CancelPush(_) => TYPE_CANCEL_PUSH,
            Self::Settings(_) => TYPE_SETTINGS,
            Self::PushPromise { .. } => TYPE_PUSH_PROMISE,
            Self::GoAway(_) => TYPE_GOAWAY,
            Self::MaxPushId(_) => TYPE_MAX_PUSH_ID,
            Self::Reserved { frame_type, .. } => *frame_type,
        }
    }

    /// Parse one frame from the front of `buf`.
    ///
    /// Returns `Ok(None)` while `buf` does not yet hold a complete frame (the
    /// caller should read more bytes and retry), `Ok(Some((frame, consumed)))`
    /// on a full frame, and `Err` on an RFC 9114 violation.
    ///
    /// # Errors
    ///
    /// - [`FrameError::Frame`] — truncated/over-long fixed field, or a total
    ///   length that overflows `usize`.
    /// - [`FrameError::Unexpected`] — a reserved HTTP/2 frame type.
    /// - [`FrameError::Settings`] — reserved or duplicate SETTINGS identifier.
    pub fn parse(buf: &[u8]) -> Result<Option<(Self, usize)>, FrameError> {
        let Some((frame_type, tlen)) = varint::decode(buf) else {
            return Ok(None);
        };
        let Some((length, llen)) = varint::decode(&buf[tlen..]) else {
            return Ok(None);
        };
        let header_len = tlen + llen;
        // `Length` is a 62-bit varint; keep the arithmetic in u64 so a large
        // advertised length on a 32-bit `usize` target cannot wrap.
        let total = (header_len as u64)
            .checked_add(length)
            .ok_or_else(|| FrameError::frame("frame length overflows u64"))?;
        if (buf.len() as u64) < total {
            return Ok(None);
        }
        // Safe: total ≤ buf.len(), which is a valid usize.
        let total = total as usize;
        let payload = &buf[header_len..total];
        let frame = Self::from_type_payload(frame_type, payload)?;
        Ok(Some((frame, total)))
    }

    /// Build a frame from its type code and already-delimited payload.
    fn from_type_payload(frame_type: u64, payload: &[u8]) -> Result<Self, FrameError> {
        match frame_type {
            TYPE_DATA => Ok(Self::Data(payload.to_vec())),
            TYPE_HEADERS => Ok(Self::Headers(payload.to_vec())),
            TYPE_CANCEL_PUSH => Ok(Self::CancelPush(single_varint(payload, "CANCEL_PUSH")?)),
            TYPE_SETTINGS => Ok(Self::Settings(parse_settings(payload)?)),
            TYPE_PUSH_PROMISE => {
                let (push_id, consumed) = varint::decode(payload)
                    .ok_or_else(|| FrameError::frame("PUSH_PROMISE: truncated push id"))?;
                Ok(Self::PushPromise {
                    push_id,
                    block: payload[consumed..].to_vec(),
                })
            }
            TYPE_GOAWAY => Ok(Self::GoAway(single_varint(payload, "GOAWAY")?)),
            TYPE_MAX_PUSH_ID => Ok(Self::MaxPushId(single_varint(payload, "MAX_PUSH_ID")?)),
            // Frame types reserved because HTTP/2 used them; receiving one on an
            // HTTP/3 connection is a connection error (RFC 9114 §11.2.1).
            0x02 | 0x06 | 0x08 | 0x09 => Err(FrameError::Unexpected(format!(
                "reserved HTTP/2 frame type 0x{frame_type:02x}"
            ))),
            other => Ok(Self::Reserved {
                frame_type: other,
                payload: payload.to_vec(),
            }),
        }
    }

    /// Serialize this frame (type · length · payload) onto `out`.
    ///
    /// # Errors
    ///
    /// [`FrameError::Frame`] if a length or identifier exceeds the QUIC varint
    /// maximum (2^62 − 1) and cannot be encoded.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        match self {
            Self::Data(d) => write_frame(TYPE_DATA, d, out),
            Self::Headers(h) => write_frame(TYPE_HEADERS, h, out),
            Self::CancelPush(id) => write_varint_frame(TYPE_CANCEL_PUSH, *id, out),
            Self::GoAway(id) => write_varint_frame(TYPE_GOAWAY, *id, out),
            Self::MaxPushId(id) => write_varint_frame(TYPE_MAX_PUSH_ID, *id, out),
            Self::Settings(params) => {
                let mut payload = Vec::new();
                for &(id, val) in params {
                    put_varint(id, &mut payload)?;
                    put_varint(val, &mut payload)?;
                }
                write_frame(TYPE_SETTINGS, &payload, out)
            }
            Self::PushPromise { push_id, block } => {
                let mut payload = Vec::new();
                put_varint(*push_id, &mut payload)?;
                payload.extend_from_slice(block);
                write_frame(TYPE_PUSH_PROMISE, &payload, out)
            }
            Self::Reserved { frame_type, payload } => write_frame(*frame_type, payload, out),
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Decode a varint that must consume the entire `payload` (RFC 9114 frames
/// whose body is a single varint reject trailing bytes as `H3_FRAME_ERROR`).
fn single_varint(payload: &[u8], name: &str) -> Result<u64, FrameError> {
    let (value, consumed) =
        varint::decode(payload).ok_or_else(|| FrameError::frame(format!("{name}: truncated varint")))?;
    if consumed != payload.len() {
        return Err(FrameError::frame(format!("{name}: {} trailing byte(s)", payload.len() - consumed)));
    }
    Ok(value)
}

/// Parse a SETTINGS payload into `(identifier, value)` pairs, enforcing the two
/// local rules of RFC 9114 §7.2.4.1: reserved HTTP/2 identifiers and duplicate
/// identifiers are `H3_SETTINGS_ERROR`.
fn parse_settings(mut payload: &[u8]) -> Result<Vec<(u64, u64)>, FrameError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    while !payload.is_empty() {
        let (id, c1) = varint::decode(payload)
            .ok_or_else(|| FrameError::frame("SETTINGS: truncated identifier"))?;
        payload = &payload[c1..];
        let (value, c2) = varint::decode(payload)
            .ok_or_else(|| FrameError::frame("SETTINGS: truncated value"))?;
        payload = &payload[c2..];
        // 0x00/0x02/0x03/0x04/0x05 were HTTP/2 settings; their presence in an
        // HTTP/3 SETTINGS frame is an error (RFC 9114 §7.2.4.1).
        if matches!(id, 0x00 | 0x02 | 0x03 | 0x04 | 0x05) {
            return Err(FrameError::Settings(format!("reserved HTTP/2 setting 0x{id:02x}")));
        }
        if !seen.insert(id) {
            return Err(FrameError::Settings(format!("duplicate setting 0x{id:x}")));
        }
        out.push((id, value));
    }
    Ok(out)
}

/// Encode a varint into `out`, mapping the overflow error to `H3_FRAME_ERROR`.
fn put_varint(value: u64, out: &mut Vec<u8>) -> Result<(), FrameError> {
    varint::encode(value, out).map_err(|e| FrameError::frame(e.to_string()))
}

/// Write a full frame (type · length · payload) given a raw type code.
fn write_frame(frame_type: u64, payload: &[u8], out: &mut Vec<u8>) -> Result<(), FrameError> {
    put_varint(frame_type, out)?;
    put_varint(payload.len() as u64, out)?;
    out.extend_from_slice(payload);
    Ok(())
}

/// Write a frame whose entire payload is one varint (CANCEL_PUSH/GOAWAY/…).
fn write_varint_frame(frame_type: u64, value: u64, out: &mut Vec<u8>) -> Result<(), FrameError> {
    let mut payload = Vec::new();
    put_varint(value, &mut payload)?;
    write_frame(frame_type, &payload, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode then parse yields the original frame and consumes the whole buffer.
    fn roundtrip(frame: &Frame) {
        let mut buf = Vec::new();
        frame.encode(&mut buf).expect("encode");
        let (got, consumed) = Frame::parse(&buf).expect("no error").expect("complete");
        assert_eq!(&got, frame, "round-trip value");
        assert_eq!(consumed, buf.len(), "consumed whole frame");
    }

    #[test]
    fn roundtrip_all_variants() {
        roundtrip(&Frame::Data(b"hello body".to_vec()));
        roundtrip(&Frame::Data(Vec::new())); // empty DATA is legal
        roundtrip(&Frame::Headers(vec![0x00, 0x00, 0xc1, 0xc0])); // opaque QPACK
        roundtrip(&Frame::CancelPush(7));
        roundtrip(&Frame::GoAway(0));
        roundtrip(&Frame::GoAway(4_611_686_018_427_387_903)); // MAX_VARINT
        roundtrip(&Frame::MaxPushId(100));
        roundtrip(&Frame::Settings(vec![
            (SETTING_MAX_FIELD_SECTION_SIZE, 65536),
            (SETTING_QPACK_MAX_TABLE_CAPACITY, 4096),
            (SETTING_QPACK_BLOCKED_STREAMS, 16),
            (SETTING_ENABLE_CONNECT_PROTOCOL, 1),
        ]));
        roundtrip(&Frame::Settings(Vec::new())); // empty SETTINGS is legal
        roundtrip(&Frame::PushPromise {
            push_id: 3,
            block: vec![0xde, 0xad, 0xbe, 0xef],
        });
        roundtrip(&Frame::Reserved {
            frame_type: 0x21, // grease: 0x1f*0 + 0x21
            payload: vec![1, 2, 3],
        });
    }

    #[test]
    fn data_frame_wire_shape() {
        // DATA(type 0x00) length 3 payload "abc".
        let mut buf = Vec::new();
        Frame::Data(b"abc".to_vec()).encode(&mut buf).unwrap();
        assert_eq!(buf, [0x00, 0x03, b'a', b'b', b'c']);
    }

    #[test]
    fn settings_wire_shape() {
        // One setting: MAX_FIELD_SECTION_SIZE(0x06) = 0x40 (2-byte varint 0x4040).
        let mut buf = Vec::new();
        Frame::Settings(vec![(0x06, 0x40)]).encode(&mut buf).unwrap();
        // type 0x04, length 0x03, id 0x06, value 0x40 40.
        assert_eq!(buf, [0x04, 0x03, 0x06, 0x40, 0x40]);
    }

    #[test]
    fn parse_returns_none_until_complete() {
        let mut buf = Vec::new();
        Frame::Data(b"abcdef".to_vec()).encode(&mut buf).unwrap();
        // header is 2 bytes (type + length), payload 6.
        assert_eq!(Frame::parse(&buf[..0]).unwrap(), None);
        assert_eq!(Frame::parse(&buf[..1]).unwrap(), None, "type only");
        assert_eq!(Frame::parse(&buf[..2]).unwrap(), None, "header, no payload");
        assert_eq!(Frame::parse(&buf[..5]).unwrap(), None, "1 payload byte short");
        assert!(Frame::parse(&buf).unwrap().is_some());
    }

    #[test]
    fn parse_two_frames_sequentially() {
        let mut buf = Vec::new();
        Frame::Headers(vec![0xc1]).encode(&mut buf).unwrap();
        let first_len = buf.len();
        Frame::Data(b"xy".to_vec()).encode(&mut buf).unwrap();

        let (f1, c1) = Frame::parse(&buf).unwrap().unwrap();
        assert_eq!(f1, Frame::Headers(vec![0xc1]));
        assert_eq!(c1, first_len);
        let (f2, c2) = Frame::parse(&buf[c1..]).unwrap().unwrap();
        assert_eq!(f2, Frame::Data(b"xy".to_vec()));
        assert_eq!(c1 + c2, buf.len());
    }

    #[test]
    fn reserved_http2_frame_types_are_unexpected() {
        for &ty in &[0x02u64, 0x06, 0x08, 0x09] {
            // type · length 0 · (no payload)
            let buf = [ty as u8, 0x00];
            let err = Frame::parse(&buf).unwrap_err();
            assert!(matches!(err, FrameError::Unexpected(_)), "type 0x{ty:02x}");
            assert_eq!(err.code(), H3_FRAME_UNEXPECTED);
        }
    }

    #[test]
    fn unknown_frame_type_is_reserved_not_error() {
        // Type 0x1f*N + 0x21 with N=1 → 0x40 (a 2-byte varint grease value).
        let grease: u64 = 0x1f + 0x21;
        let mut buf = Vec::new();
        Frame::Reserved { frame_type: grease, payload: vec![9, 9] }
            .encode(&mut buf)
            .unwrap();
        let (frame, _) = Frame::parse(&buf).unwrap().unwrap();
        assert_eq!(frame, Frame::Reserved { frame_type: grease, payload: vec![9, 9] });
    }

    #[test]
    fn settings_rejects_reserved_http2_identifier() {
        // id 0x02 (HTTP/2 ENABLE_PUSH) value 0.
        let buf = [0x04u8, 0x02, 0x02, 0x00];
        let err = Frame::parse(&buf).unwrap_err();
        assert!(matches!(err, FrameError::Settings(_)));
        assert_eq!(err.code(), H3_SETTINGS_ERROR);
    }

    #[test]
    fn settings_rejects_duplicate_identifier() {
        let mut payload = Vec::new();
        varint::encode(0x06, &mut payload).unwrap();
        varint::encode(1, &mut payload).unwrap();
        varint::encode(0x06, &mut payload).unwrap();
        varint::encode(2, &mut payload).unwrap();
        let mut buf = Vec::new();
        write_frame(TYPE_SETTINGS, &payload, &mut buf).unwrap();
        let err = Frame::parse(&buf).unwrap_err();
        assert!(matches!(err, FrameError::Settings(_)));
    }

    #[test]
    fn settings_truncated_value_is_frame_error() {
        // type 0x04, length 1, one identifier byte but no value.
        let buf = [0x04u8, 0x01, 0x06];
        let err = Frame::parse(&buf).unwrap_err();
        assert!(matches!(err, FrameError::Frame(_)));
        assert_eq!(err.code(), H3_FRAME_ERROR);
    }

    #[test]
    fn goaway_with_trailing_bytes_is_frame_error() {
        // GOAWAY payload must be exactly one varint; give it two bytes for a
        // 1-byte varint.
        let buf = [0x07u8, 0x02, 0x01, 0xff];
        let err = Frame::parse(&buf).unwrap_err();
        assert!(matches!(err, FrameError::Frame(_)));
    }

    #[test]
    fn frame_type_accessor_matches_encoding() {
        let frames = [
            Frame::Data(vec![]),
            Frame::Headers(vec![]),
            Frame::CancelPush(1),
            Frame::Settings(vec![]),
            Frame::PushPromise { push_id: 1, block: vec![] },
            Frame::GoAway(1),
            Frame::MaxPushId(1),
            Frame::Reserved { frame_type: 0x21, payload: vec![] },
        ];
        for f in &frames {
            let mut buf = Vec::new();
            f.encode(&mut buf).unwrap();
            let (ty, _) = varint::decode(&buf).unwrap();
            assert_eq!(ty, f.frame_type(), "{f:?}");
        }
    }
}
