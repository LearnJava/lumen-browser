//! QUIC transport frame codec — RFC 9000 §19.
//!
//! A QUIC packet's protected payload is a sequence of frames laid end to end
//! with no length delimiter between them: every frame is self-delimiting, its
//! type a QUIC varint (RFC 9000 §12.4) followed by type-specific fields. This
//! module is a pure parse/serialize layer over that shape — no IO, no packet
//! protection, no connection state. It sits directly on the [`varint`] codec
//! (Slice 1) the same way the HTTP/3 [`frame`](super::frame) codec does, and is
//! the transport-side counterpart the later QUIC connection layer feeds after
//! removing header protection and AEAD.
//!
//! ## Scope
//!
//! All RFC 9000 §19 frame types are represented:
//!
//! - PADDING (§19.1), PING (§19.2), coalesced into [`Frame::Padding`] /
//!   [`Frame::Ping`].
//! - ACK, with and without ECN counts (§19.3) — [`Frame::Ack`].
//! - RESET_STREAM (§19.4), STOP_SENDING (§19.5).
//! - CRYPTO (§19.6), NEW_TOKEN (§19.7).
//! - STREAM (§19.8), the eight `0x08..=0x0f` variants distinguished by their
//!   OFF/LEN/FIN bits — [`Frame::Stream`].
//! - MAX_DATA (§19.9), MAX_STREAM_DATA (§19.10), MAX_STREAMS bidi/uni (§19.11).
//! - DATA_BLOCKED (§19.12), STREAM_DATA_BLOCKED (§19.13),
//!   STREAMS_BLOCKED bidi/uni (§19.14).
//! - NEW_CONNECTION_ID (§19.15), RETIRE_CONNECTION_ID (§19.16).
//! - PATH_CHALLENGE (§19.17), PATH_RESPONSE (§19.18).
//! - CONNECTION_CLOSE transport/application (§19.19).
//! - HANDSHAKE_DONE (§19.20).
//!
//! An unknown frame type, a truncated field, or an out-of-range value (e.g. a
//! NEW_CONNECTION_ID length outside `1..=20`) is a
//! [`QuicFrameError`] mapping to `FRAME_ENCODING_ERROR` (RFC 9000 §19,
//! §12.4). This mirrors the mandate that an endpoint treat an unknown frame
//! type as a connection error rather than skipping it.
//!
//! ## Out of scope (deferred to later slices)
//!
//! - Packet headers, header protection, and AEAD packet protection
//!   (RFC 9000 §17, RFC 9001).
//! - Which frames are permitted in which packet-number space, ack-eliciting /
//!   congestion-control accounting, and flow-control enforcement.
//! - IO, loss recovery, and the connection state machine.

use super::varint;

// ── Frame type codes (RFC 9000 §19 / §12.4 registry) ────────────────────────

/// PADDING (RFC 9000 §19.1).
pub const TYPE_PADDING: u64 = 0x00;
/// PING (RFC 9000 §19.2).
pub const TYPE_PING: u64 = 0x01;
/// ACK without ECN counts (RFC 9000 §19.3).
pub const TYPE_ACK: u64 = 0x02;
/// ACK carrying ECN counts (RFC 9000 §19.3).
pub const TYPE_ACK_ECN: u64 = 0x03;
/// RESET_STREAM (RFC 9000 §19.4).
pub const TYPE_RESET_STREAM: u64 = 0x04;
/// STOP_SENDING (RFC 9000 §19.5).
pub const TYPE_STOP_SENDING: u64 = 0x05;
/// CRYPTO (RFC 9000 §19.6).
pub const TYPE_CRYPTO: u64 = 0x06;
/// NEW_TOKEN (RFC 9000 §19.7).
pub const TYPE_NEW_TOKEN: u64 = 0x07;
/// STREAM, base type; the low three bits are the OFF/LEN/FIN flags
/// (RFC 9000 §19.8). Valid encoded types are `0x08..=0x0f`.
pub const TYPE_STREAM_BASE: u64 = 0x08;
/// MAX_DATA (RFC 9000 §19.9).
pub const TYPE_MAX_DATA: u64 = 0x10;
/// MAX_STREAM_DATA (RFC 9000 §19.10).
pub const TYPE_MAX_STREAM_DATA: u64 = 0x11;
/// MAX_STREAMS for bidirectional streams (RFC 9000 §19.11).
pub const TYPE_MAX_STREAMS_BIDI: u64 = 0x12;
/// MAX_STREAMS for unidirectional streams (RFC 9000 §19.11).
pub const TYPE_MAX_STREAMS_UNI: u64 = 0x13;
/// DATA_BLOCKED (RFC 9000 §19.12).
pub const TYPE_DATA_BLOCKED: u64 = 0x14;
/// STREAM_DATA_BLOCKED (RFC 9000 §19.13).
pub const TYPE_STREAM_DATA_BLOCKED: u64 = 0x15;
/// STREAMS_BLOCKED for bidirectional streams (RFC 9000 §19.14).
pub const TYPE_STREAMS_BLOCKED_BIDI: u64 = 0x16;
/// STREAMS_BLOCKED for unidirectional streams (RFC 9000 §19.14).
pub const TYPE_STREAMS_BLOCKED_UNI: u64 = 0x17;
/// NEW_CONNECTION_ID (RFC 9000 §19.15).
pub const TYPE_NEW_CONNECTION_ID: u64 = 0x18;
/// RETIRE_CONNECTION_ID (RFC 9000 §19.16).
pub const TYPE_RETIRE_CONNECTION_ID: u64 = 0x19;
/// PATH_CHALLENGE (RFC 9000 §19.17).
pub const TYPE_PATH_CHALLENGE: u64 = 0x1a;
/// PATH_RESPONSE (RFC 9000 §19.18).
pub const TYPE_PATH_RESPONSE: u64 = 0x1b;
/// CONNECTION_CLOSE signalling a QUIC transport error (RFC 9000 §19.19).
pub const TYPE_CONNECTION_CLOSE_TRANSPORT: u64 = 0x1c;
/// CONNECTION_CLOSE signalling an application error (RFC 9000 §19.19).
pub const TYPE_CONNECTION_CLOSE_APP: u64 = 0x1d;
/// HANDSHAKE_DONE (RFC 9000 §19.20).
pub const TYPE_HANDSHAKE_DONE: u64 = 0x1e;

/// STREAM frame FIN flag (RFC 9000 §19.8) — the low bit of the type.
const STREAM_FIN: u64 = 0x01;
/// STREAM frame LEN flag (RFC 9000 §19.8) — an explicit Length field is present.
const STREAM_LEN: u64 = 0x02;
/// STREAM frame OFF flag (RFC 9000 §19.8) — an explicit Offset field is present.
const STREAM_OFF: u64 = 0x04;

/// FRAME_ENCODING_ERROR (RFC 9000 §20.1) — the single wire error code this
/// codec raises: a frame could not be decoded as specified.
pub const FRAME_ENCODING_ERROR: u64 = 0x07;

/// Length in bytes of a Stateless Reset Token (RFC 9000 §10.3 / §19.15).
pub const STATELESS_RESET_TOKEN_LEN: usize = 16;

/// Length in bytes of the PATH_CHALLENGE / PATH_RESPONSE data (RFC 9000 §19.17).
pub const PATH_DATA_LEN: usize = 8;

// ── Error ────────────────────────────────────────────────────────────────────

/// Frame-codec error. Every variant is a `FRAME_ENCODING_ERROR` at the QUIC
/// transport layer (RFC 9000 §12.4, §19); [`QuicFrameError::code`] returns that
/// single wire code and the variant preserves *why* for diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuicFrameError {
    /// Input ended in the middle of a frame field.
    UnexpectedEof,
    /// A length or count field claimed more bytes than the buffer holds.
    LengthTooLong,
    /// A varint value exceeds the 2^62 − 1 QUIC varint maximum on encode.
    VarIntOverflow(u64),
    /// The frame type code is not assigned in RFC 9000 §19. An endpoint MUST
    /// treat this as a connection error (RFC 9000 §12.4).
    UnknownType(u64),
    /// A NEW_CONNECTION_ID connection-ID length was outside the valid
    /// `1..=20` range (RFC 9000 §19.15).
    InvalidConnectionIdLen(u8),
}

impl QuicFrameError {
    /// The RFC 9000 §20.1 wire error code (always `FRAME_ENCODING_ERROR`).
    #[must_use]
    pub const fn code(&self) -> u64 {
        FRAME_ENCODING_ERROR
    }
}

impl core::fmt::Display for QuicFrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "QUIC frame: unexpected EOF"),
            Self::LengthTooLong => write!(f, "QUIC frame: length exceeds remaining input"),
            Self::VarIntOverflow(v) => write!(f, "QUIC frame: value {v} exceeds varint maximum"),
            Self::UnknownType(t) => write!(f, "QUIC frame: unknown frame type 0x{t:02x}"),
            Self::InvalidConnectionIdLen(n) => {
                write!(f, "QUIC frame: connection-id length {n} outside 1..=20")
            }
        }
    }
}

impl std::error::Error for QuicFrameError {}

impl From<varint::VarIntTooLarge> for QuicFrameError {
    fn from(e: varint::VarIntTooLarge) -> Self {
        Self::VarIntOverflow(e.0)
    }
}

// ── Sub-structures ───────────────────────────────────────────────────────────

/// A single additional ACK range in an ACK frame (RFC 9000 §19.3.1). The first
/// range is stored inline on [`Frame::Ack`]; each subsequent range is a `gap`
/// of unacknowledged packets followed by a run of `length` acknowledged ones,
/// both expressed as counts relative to the previous range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AckRange {
    /// Number of contiguous unacknowledged packets preceding this range
    /// (the encoded "Gap"; the actual gap is `gap + 1` packets, RFC 9000 §19.3.1).
    pub gap: u64,
    /// Number of additional acknowledged packets in this range (the encoded
    /// "ACK Range Length"; the range spans `length + 1` packets).
    pub length: u64,
}

/// ECN counts carried by an ACK frame of type `0x03` (RFC 9000 §19.3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EcnCounts {
    /// Total packets received with the ECT(0) codepoint.
    pub ect0: u64,
    /// Total packets received with the ECT(1) codepoint.
    pub ect1: u64,
    /// Total packets received with the ECN-CE codepoint.
    pub ecn_ce: u64,
}

// ── Frame ────────────────────────────────────────────────────────────────────

/// A parsed QUIC transport frame (RFC 9000 §19). Variable-length payloads
/// (CRYPTO / STREAM data, tokens, reason phrases) are owned copies; decoding
/// their contents is a higher layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// PADDING — a run of `count` zero bytes, coalesced (RFC 9000 §19.1).
    Padding(usize),
    /// PING — solicits an acknowledgement (RFC 9000 §19.2).
    Ping,
    /// ACK — acknowledges received packets (RFC 9000 §19.3).
    Ack {
        /// The largest packet number being acknowledged.
        largest_acked: u64,
        /// Acknowledgement delay, in microseconds scaled by `2^ack_delay_exponent`.
        ack_delay: u64,
        /// Additional acknowledged packets immediately below `largest_acked`
        /// (the encoded "First ACK Range"; spans `first_ack_range + 1` packets).
        first_ack_range: u64,
        /// Further ACK ranges, each relative to the previous one.
        ranges: Vec<AckRange>,
        /// ECN counts, present iff the frame type was `0x03`.
        ecn: Option<EcnCounts>,
    },
    /// RESET_STREAM — abruptly terminates the sending part of a stream
    /// (RFC 9000 §19.4).
    ResetStream {
        /// Stream being reset.
        stream_id: u64,
        /// Application protocol error code.
        app_error_code: u64,
        /// Final size (in bytes) the sender delivered on the stream.
        final_size: u64,
    },
    /// STOP_SENDING — requests the peer stop sending on a stream
    /// (RFC 9000 §19.5).
    StopSending {
        /// Stream the receiver is no longer interested in.
        stream_id: u64,
        /// Application protocol error code.
        app_error_code: u64,
    },
    /// CRYPTO — carries cryptographic handshake data at a byte offset
    /// (RFC 9000 §19.6).
    Crypto {
        /// Byte offset of `data` in the crypto stream.
        offset: u64,
        /// Handshake data.
        data: Vec<u8>,
    },
    /// NEW_TOKEN — a token the client may send in a later Initial packet's
    /// address-validation field (RFC 9000 §19.7).
    NewToken(Vec<u8>),
    /// STREAM — application data at an offset on a stream (RFC 9000 §19.8).
    Stream {
        /// Stream carrying the data.
        stream_id: u64,
        /// Byte offset of `data` (0 when the wire OFF bit was clear).
        offset: u64,
        /// Whether the FIN bit marked this as the final stream frame.
        fin: bool,
        /// The stream data.
        data: Vec<u8>,
    },
    /// MAX_DATA — connection-wide flow-control limit (RFC 9000 §19.9).
    MaxData(u64),
    /// MAX_STREAM_DATA — per-stream flow-control limit (RFC 9000 §19.10).
    MaxStreamData {
        /// Stream the limit applies to.
        stream_id: u64,
        /// Maximum byte offset the peer may send on the stream.
        max: u64,
    },
    /// MAX_STREAMS — cumulative stream-count limit (RFC 9000 §19.11).
    MaxStreams {
        /// `true` for bidirectional streams, `false` for unidirectional.
        bidi: bool,
        /// Maximum number of streams of that type the peer may open.
        max: u64,
    },
    /// DATA_BLOCKED — sender is connection-flow-control blocked at `limit`
    /// (RFC 9000 §19.12).
    DataBlocked(u64),
    /// STREAM_DATA_BLOCKED — sender is stream-flow-control blocked
    /// (RFC 9000 §19.13).
    StreamDataBlocked {
        /// Stream on which the sender is blocked.
        stream_id: u64,
        /// The stream data limit at which the sender is blocked.
        limit: u64,
    },
    /// STREAMS_BLOCKED — sender wants to open a stream past the limit
    /// (RFC 9000 §19.14).
    StreamsBlocked {
        /// `true` for bidirectional streams, `false` for unidirectional.
        bidi: bool,
        /// The stream-count limit at which the sender is blocked.
        limit: u64,
    },
    /// NEW_CONNECTION_ID — supplies an alternative connection ID
    /// (RFC 9000 §19.15).
    NewConnectionId {
        /// Sequence number assigned to the connection ID.
        sequence_number: u64,
        /// The sender requests retirement of all IDs with a sequence number
        /// below this value.
        retire_prior_to: u64,
        /// The connection ID itself (1..=20 bytes).
        connection_id: Vec<u8>,
        /// The 16-byte stateless reset token bound to this connection ID.
        stateless_reset_token: [u8; STATELESS_RESET_TOKEN_LEN],
    },
    /// RETIRE_CONNECTION_ID — retires a previously issued connection ID by
    /// sequence number (RFC 9000 §19.16).
    RetireConnectionId(u64),
    /// PATH_CHALLENGE — 8 bytes of path-validation data (RFC 9000 §19.17).
    PathChallenge([u8; PATH_DATA_LEN]),
    /// PATH_RESPONSE — echoes a PATH_CHALLENGE's data (RFC 9000 §19.18).
    PathResponse([u8; PATH_DATA_LEN]),
    /// CONNECTION_CLOSE — closes the connection (RFC 9000 §19.19).
    ConnectionClose {
        /// The error code (a transport code when `frame_type` is `Some`, an
        /// application code otherwise).
        error_code: u64,
        /// The frame type that triggered the error, present only for a
        /// transport-level close (type `0x1c`); `None` for an application close
        /// (type `0x1d`).
        frame_type: Option<u64>,
        /// Human-readable reason phrase (may be empty).
        reason: Vec<u8>,
    },
    /// HANDSHAKE_DONE — server signals the handshake is confirmed
    /// (RFC 9000 §19.20).
    HandshakeDone,
}

impl Frame {
    /// Parse exactly one frame from the front of `input`.
    ///
    /// Returns the frame and the number of bytes it consumed. QUIC frames are
    /// self-delimiting with no length prefix, so a truncated frame is a
    /// `FRAME_ENCODING_ERROR` rather than a "need more bytes" condition — the
    /// caller has already reassembled a full packet payload before decoding.
    ///
    /// # Errors
    ///
    /// [`QuicFrameError`] on a truncated field, an over-long length/count, an
    /// unknown frame type, or an out-of-range connection-ID length.
    pub fn parse(input: &[u8]) -> Result<(Self, usize), QuicFrameError> {
        let mut buf = input;
        let ty = take_varint(&mut buf)?;
        let frame = match ty {
            TYPE_PADDING => {
                // Coalesce this and every following zero byte into one PADDING.
                let extra = buf.iter().take_while(|&&b| b == 0).count();
                buf = &buf[extra..];
                Self::Padding(extra + 1)
            }
            TYPE_PING => Self::Ping,
            TYPE_ACK | TYPE_ACK_ECN => parse_ack(&mut buf, ty == TYPE_ACK_ECN)?,
            TYPE_RESET_STREAM => Self::ResetStream {
                stream_id: take_varint(&mut buf)?,
                app_error_code: take_varint(&mut buf)?,
                final_size: take_varint(&mut buf)?,
            },
            TYPE_STOP_SENDING => Self::StopSending {
                stream_id: take_varint(&mut buf)?,
                app_error_code: take_varint(&mut buf)?,
            },
            TYPE_CRYPTO => {
                let offset = take_varint(&mut buf)?;
                let data = take_length_prefixed(&mut buf)?;
                Self::Crypto { offset, data }
            }
            TYPE_NEW_TOKEN => Self::NewToken(take_length_prefixed(&mut buf)?),
            0x08..=0x0f => parse_stream(&mut buf, ty)?,
            TYPE_MAX_DATA => Self::MaxData(take_varint(&mut buf)?),
            TYPE_MAX_STREAM_DATA => Self::MaxStreamData {
                stream_id: take_varint(&mut buf)?,
                max: take_varint(&mut buf)?,
            },
            TYPE_MAX_STREAMS_BIDI | TYPE_MAX_STREAMS_UNI => Self::MaxStreams {
                bidi: ty == TYPE_MAX_STREAMS_BIDI,
                max: take_varint(&mut buf)?,
            },
            TYPE_DATA_BLOCKED => Self::DataBlocked(take_varint(&mut buf)?),
            TYPE_STREAM_DATA_BLOCKED => Self::StreamDataBlocked {
                stream_id: take_varint(&mut buf)?,
                limit: take_varint(&mut buf)?,
            },
            TYPE_STREAMS_BLOCKED_BIDI | TYPE_STREAMS_BLOCKED_UNI => Self::StreamsBlocked {
                bidi: ty == TYPE_STREAMS_BLOCKED_BIDI,
                limit: take_varint(&mut buf)?,
            },
            TYPE_NEW_CONNECTION_ID => parse_new_connection_id(&mut buf)?,
            TYPE_RETIRE_CONNECTION_ID => Self::RetireConnectionId(take_varint(&mut buf)?),
            TYPE_PATH_CHALLENGE => Self::PathChallenge(take_array(&mut buf)?),
            TYPE_PATH_RESPONSE => Self::PathResponse(take_array(&mut buf)?),
            TYPE_CONNECTION_CLOSE_TRANSPORT | TYPE_CONNECTION_CLOSE_APP => {
                parse_connection_close(&mut buf, ty == TYPE_CONNECTION_CLOSE_TRANSPORT)?
            }
            TYPE_HANDSHAKE_DONE => Self::HandshakeDone,
            other => return Err(QuicFrameError::UnknownType(other)),
        };
        let consumed = input.len() - buf.len();
        Ok((frame, consumed))
    }

    /// The wire type code this frame serializes to (STREAM reports its base
    /// `0x08`; the OFF/LEN/FIN bits are set at encode time).
    #[must_use]
    pub const fn frame_type(&self) -> u64 {
        match self {
            Self::Padding(_) => TYPE_PADDING,
            Self::Ping => TYPE_PING,
            Self::Ack { ecn: None, .. } => TYPE_ACK,
            Self::Ack { ecn: Some(_), .. } => TYPE_ACK_ECN,
            Self::ResetStream { .. } => TYPE_RESET_STREAM,
            Self::StopSending { .. } => TYPE_STOP_SENDING,
            Self::Crypto { .. } => TYPE_CRYPTO,
            Self::NewToken(_) => TYPE_NEW_TOKEN,
            Self::Stream { .. } => TYPE_STREAM_BASE,
            Self::MaxData(_) => TYPE_MAX_DATA,
            Self::MaxStreamData { .. } => TYPE_MAX_STREAM_DATA,
            Self::MaxStreams { bidi: true, .. } => TYPE_MAX_STREAMS_BIDI,
            Self::MaxStreams { bidi: false, .. } => TYPE_MAX_STREAMS_UNI,
            Self::DataBlocked(_) => TYPE_DATA_BLOCKED,
            Self::StreamDataBlocked { .. } => TYPE_STREAM_DATA_BLOCKED,
            Self::StreamsBlocked { bidi: true, .. } => TYPE_STREAMS_BLOCKED_BIDI,
            Self::StreamsBlocked { bidi: false, .. } => TYPE_STREAMS_BLOCKED_UNI,
            Self::NewConnectionId { .. } => TYPE_NEW_CONNECTION_ID,
            Self::RetireConnectionId(_) => TYPE_RETIRE_CONNECTION_ID,
            Self::PathChallenge(_) => TYPE_PATH_CHALLENGE,
            Self::PathResponse(_) => TYPE_PATH_RESPONSE,
            Self::ConnectionClose { frame_type: Some(_), .. } => TYPE_CONNECTION_CLOSE_TRANSPORT,
            Self::ConnectionClose { frame_type: None, .. } => TYPE_CONNECTION_CLOSE_APP,
            Self::HandshakeDone => TYPE_HANDSHAKE_DONE,
        }
    }

    /// Serialize this frame onto `out`. STREAM frames always emit an explicit
    /// Length field (LEN bit set) so the encoding is self-delimiting; the OFF
    /// bit is set iff `offset != 0`.
    ///
    /// # Errors
    ///
    /// [`QuicFrameError::VarIntOverflow`] if any field exceeds the QUIC varint
    /// maximum (2^62 − 1).
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), QuicFrameError> {
        match self {
            Self::Padding(count) => out.resize(out.len() + *count, 0),
            Self::Ping => put_varint(TYPE_PING, out)?,
            Self::Ack { largest_acked, ack_delay, first_ack_range, ranges, ecn } => {
                put_varint(if ecn.is_some() { TYPE_ACK_ECN } else { TYPE_ACK }, out)?;
                put_varint(*largest_acked, out)?;
                put_varint(*ack_delay, out)?;
                put_varint(ranges.len() as u64, out)?;
                put_varint(*first_ack_range, out)?;
                for range in ranges {
                    put_varint(range.gap, out)?;
                    put_varint(range.length, out)?;
                }
                if let Some(e) = ecn {
                    put_varint(e.ect0, out)?;
                    put_varint(e.ect1, out)?;
                    put_varint(e.ecn_ce, out)?;
                }
            }
            Self::ResetStream { stream_id, app_error_code, final_size } => {
                put_varint(TYPE_RESET_STREAM, out)?;
                put_varint(*stream_id, out)?;
                put_varint(*app_error_code, out)?;
                put_varint(*final_size, out)?;
            }
            Self::StopSending { stream_id, app_error_code } => {
                put_varint(TYPE_STOP_SENDING, out)?;
                put_varint(*stream_id, out)?;
                put_varint(*app_error_code, out)?;
            }
            Self::Crypto { offset, data } => {
                put_varint(TYPE_CRYPTO, out)?;
                put_varint(*offset, out)?;
                put_length_prefixed(data, out)?;
            }
            Self::NewToken(token) => {
                put_varint(TYPE_NEW_TOKEN, out)?;
                put_length_prefixed(token, out)?;
            }
            Self::Stream { stream_id, offset, fin, data } => {
                let mut ty = TYPE_STREAM_BASE | STREAM_LEN;
                if *offset != 0 {
                    ty |= STREAM_OFF;
                }
                if *fin {
                    ty |= STREAM_FIN;
                }
                put_varint(ty, out)?;
                put_varint(*stream_id, out)?;
                if *offset != 0 {
                    put_varint(*offset, out)?;
                }
                put_length_prefixed(data, out)?;
            }
            Self::MaxData(max) => {
                put_varint(TYPE_MAX_DATA, out)?;
                put_varint(*max, out)?;
            }
            Self::MaxStreamData { stream_id, max } => {
                put_varint(TYPE_MAX_STREAM_DATA, out)?;
                put_varint(*stream_id, out)?;
                put_varint(*max, out)?;
            }
            Self::MaxStreams { bidi, max } => {
                put_varint(if *bidi { TYPE_MAX_STREAMS_BIDI } else { TYPE_MAX_STREAMS_UNI }, out)?;
                put_varint(*max, out)?;
            }
            Self::DataBlocked(limit) => {
                put_varint(TYPE_DATA_BLOCKED, out)?;
                put_varint(*limit, out)?;
            }
            Self::StreamDataBlocked { stream_id, limit } => {
                put_varint(TYPE_STREAM_DATA_BLOCKED, out)?;
                put_varint(*stream_id, out)?;
                put_varint(*limit, out)?;
            }
            Self::StreamsBlocked { bidi, limit } => {
                put_varint(
                    if *bidi { TYPE_STREAMS_BLOCKED_BIDI } else { TYPE_STREAMS_BLOCKED_UNI },
                    out,
                )?;
                put_varint(*limit, out)?;
            }
            Self::NewConnectionId {
                sequence_number,
                retire_prior_to,
                connection_id,
                stateless_reset_token,
            } => {
                put_varint(TYPE_NEW_CONNECTION_ID, out)?;
                put_varint(*sequence_number, out)?;
                put_varint(*retire_prior_to, out)?;
                // Length is a single byte, valid range 1..=20 (RFC 9000 §19.15).
                let len = u8::try_from(connection_id.len())
                    .ok()
                    .filter(|&n| (1..=20).contains(&n))
                    .ok_or(QuicFrameError::InvalidConnectionIdLen(
                        connection_id.len().min(255) as u8,
                    ))?;
                out.push(len);
                out.extend_from_slice(connection_id);
                out.extend_from_slice(stateless_reset_token);
            }
            Self::RetireConnectionId(seq) => {
                put_varint(TYPE_RETIRE_CONNECTION_ID, out)?;
                put_varint(*seq, out)?;
            }
            Self::PathChallenge(data) => {
                put_varint(TYPE_PATH_CHALLENGE, out)?;
                out.extend_from_slice(data);
            }
            Self::PathResponse(data) => {
                put_varint(TYPE_PATH_RESPONSE, out)?;
                out.extend_from_slice(data);
            }
            Self::ConnectionClose { error_code, frame_type, reason } => {
                match frame_type {
                    Some(ft) => {
                        put_varint(TYPE_CONNECTION_CLOSE_TRANSPORT, out)?;
                        put_varint(*error_code, out)?;
                        put_varint(*ft, out)?;
                    }
                    None => {
                        put_varint(TYPE_CONNECTION_CLOSE_APP, out)?;
                        put_varint(*error_code, out)?;
                    }
                }
                put_length_prefixed(reason, out)?;
            }
            Self::HandshakeDone => put_varint(TYPE_HANDSHAKE_DONE, out)?,
        }
        Ok(())
    }

    /// Whether this frame is ack-eliciting (RFC 9000 §13.2.1): receipt obliges
    /// the peer to send an acknowledgement. ACK, PADDING, and CONNECTION_CLOSE
    /// are the only non-eliciting frames.
    #[must_use]
    pub const fn is_ack_eliciting(&self) -> bool {
        !matches!(
            self,
            Self::Padding(_) | Self::Ack { .. } | Self::ConnectionClose { .. }
        )
    }
}

/// Parse a full packet payload — a sequence of frames back to back — into a
/// vector (RFC 9000 §12.4).
///
/// # Errors
///
/// [`QuicFrameError`] from the first frame that fails to decode.
pub fn parse_all(mut buf: &[u8]) -> Result<Vec<Frame>, QuicFrameError> {
    let mut frames = Vec::new();
    while !buf.is_empty() {
        let (frame, consumed) = Frame::parse(buf)?;
        buf = &buf[consumed..];
        frames.push(frame);
    }
    Ok(frames)
}

/// Serialize a sequence of frames back to back onto `out` (RFC 9000 §12.4).
///
/// # Errors
///
/// [`QuicFrameError`] from the first frame that fails to encode.
pub fn encode_all(frames: &[Frame], out: &mut Vec<u8>) -> Result<(), QuicFrameError> {
    for frame in frames {
        frame.encode(out)?;
    }
    Ok(())
}

// ── Per-type parse helpers ───────────────────────────────────────────────────

/// Parse the body of an ACK frame (type already consumed); `ecn` selects the
/// `0x03` form that trails three ECN counts (RFC 9000 §19.3).
fn parse_ack(buf: &mut &[u8], ecn: bool) -> Result<Frame, QuicFrameError> {
    let largest_acked = take_varint(buf)?;
    let ack_delay = take_varint(buf)?;
    let range_count = take_varint(buf)?;
    let first_ack_range = take_varint(buf)?;
    // Bound the range vector by the bytes left: each range is ≥ 2 bytes, so a
    // count larger than the remaining input is a malformed frame rather than a
    // multi-gigabyte allocation.
    if range_count > buf.len() as u64 {
        return Err(QuicFrameError::LengthTooLong);
    }
    let mut ranges = Vec::with_capacity(range_count as usize);
    for _ in 0..range_count {
        ranges.push(AckRange { gap: take_varint(buf)?, length: take_varint(buf)? });
    }
    let ecn = if ecn {
        Some(EcnCounts {
            ect0: take_varint(buf)?,
            ect1: take_varint(buf)?,
            ecn_ce: take_varint(buf)?,
        })
    } else {
        None
    };
    Ok(Frame::Ack { largest_acked, ack_delay, first_ack_range, ranges, ecn })
}

/// Parse the body of a STREAM frame given its full type byte, whose low three
/// bits are the OFF/LEN/FIN flags (RFC 9000 §19.8). Without the LEN bit the
/// data runs to the end of the buffer.
fn parse_stream(buf: &mut &[u8], ty: u64) -> Result<Frame, QuicFrameError> {
    let stream_id = take_varint(buf)?;
    let offset = if ty & STREAM_OFF != 0 { take_varint(buf)? } else { 0 };
    let data = if ty & STREAM_LEN != 0 {
        take_length_prefixed(buf)?
    } else {
        // No Length field: the stream data extends to the end of the payload.
        let rest = buf.to_vec();
        *buf = &buf[buf.len()..];
        rest
    };
    Ok(Frame::Stream { stream_id, offset, fin: ty & STREAM_FIN != 0, data })
}

/// Parse the body of a NEW_CONNECTION_ID frame (RFC 9000 §19.15), enforcing the
/// `1..=20` connection-ID length bound.
fn parse_new_connection_id(buf: &mut &[u8]) -> Result<Frame, QuicFrameError> {
    let sequence_number = take_varint(buf)?;
    let retire_prior_to = take_varint(buf)?;
    let len = take_u8(buf)?;
    if !(1..=20).contains(&len) {
        return Err(QuicFrameError::InvalidConnectionIdLen(len));
    }
    let connection_id = take_bytes(buf, len as usize)?.to_vec();
    let stateless_reset_token = take_array::<STATELESS_RESET_TOKEN_LEN>(buf)?;
    Ok(Frame::NewConnectionId {
        sequence_number,
        retire_prior_to,
        connection_id,
        stateless_reset_token,
    })
}

/// Parse the body of a CONNECTION_CLOSE frame; `transport` selects the `0x1c`
/// form that carries the triggering frame type (RFC 9000 §19.19).
fn parse_connection_close(buf: &mut &[u8], transport: bool) -> Result<Frame, QuicFrameError> {
    let error_code = take_varint(buf)?;
    let frame_type = if transport { Some(take_varint(buf)?) } else { None };
    let reason = take_length_prefixed(buf)?;
    Ok(Frame::ConnectionClose { error_code, frame_type, reason })
}

// ── Primitive readers / writers ──────────────────────────────────────────────

/// Pull one QUIC varint off the front of `buf`, advancing it.
fn take_varint(buf: &mut &[u8]) -> Result<u64, QuicFrameError> {
    let (value, consumed) = varint::decode(buf).ok_or(QuicFrameError::UnexpectedEof)?;
    *buf = &buf[consumed..];
    Ok(value)
}

/// Pull a single byte off the front of `buf`, advancing it.
fn take_u8(buf: &mut &[u8]) -> Result<u8, QuicFrameError> {
    let (&first, rest) = buf.split_first().ok_or(QuicFrameError::UnexpectedEof)?;
    *buf = rest;
    Ok(first)
}

/// Pull exactly `n` bytes off the front of `buf`, advancing it.
fn take_bytes<'a>(buf: &mut &'a [u8], n: usize) -> Result<&'a [u8], QuicFrameError> {
    if buf.len() < n {
        return Err(QuicFrameError::UnexpectedEof);
    }
    let (head, rest) = buf.split_at(n);
    *buf = rest;
    Ok(head)
}

/// Pull a fixed-size `N`-byte array off the front of `buf`, advancing it.
fn take_array<const N: usize>(buf: &mut &[u8]) -> Result<[u8; N], QuicFrameError> {
    let bytes = take_bytes(buf, N)?;
    let mut arr = [0u8; N];
    arr.copy_from_slice(bytes);
    Ok(arr)
}

/// Pull a varint-length-prefixed byte string off the front of `buf`.
fn take_length_prefixed(buf: &mut &[u8]) -> Result<Vec<u8>, QuicFrameError> {
    let len = take_varint(buf)?;
    let len = usize::try_from(len).map_err(|_| QuicFrameError::LengthTooLong)?;
    if buf.len() < len {
        return Err(QuicFrameError::LengthTooLong);
    }
    Ok(take_bytes(buf, len)?.to_vec())
}

/// Append a QUIC varint to `out`, mapping an overflow into the codec error.
fn put_varint(value: u64, out: &mut Vec<u8>) -> Result<(), QuicFrameError> {
    varint::encode(value, out)?;
    Ok(())
}

/// Append a varint length prefix followed by the bytes of `data`.
fn put_length_prefixed(data: &[u8], out: &mut Vec<u8>) -> Result<(), QuicFrameError> {
    put_varint(data.len() as u64, out)?;
    out.extend_from_slice(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a frame, parse it back, and assert both the value and that the
    /// whole buffer was consumed.
    fn roundtrip(frame: &Frame) {
        let mut buf = Vec::new();
        frame.encode(&mut buf).expect("encode");
        let (decoded, consumed) = Frame::parse(&buf).expect("parse");
        assert_eq!(&decoded, frame, "roundtrip value");
        assert_eq!(consumed, buf.len(), "consumed whole frame");
    }

    #[test]
    fn roundtrip_simple_frames() {
        roundtrip(&Frame::Ping);
        roundtrip(&Frame::HandshakeDone);
        roundtrip(&Frame::MaxData(1_000_000));
        roundtrip(&Frame::DataBlocked(42));
        roundtrip(&Frame::RetireConnectionId(7));
        roundtrip(&Frame::MaxStreamData { stream_id: 4, max: 65_536 });
        roundtrip(&Frame::StreamDataBlocked { stream_id: 8, limit: 100 });
        roundtrip(&Frame::MaxStreams { bidi: true, max: 100 });
        roundtrip(&Frame::MaxStreams { bidi: false, max: 3 });
        roundtrip(&Frame::StreamsBlocked { bidi: true, limit: 10 });
        roundtrip(&Frame::StreamsBlocked { bidi: false, limit: 0 });
        roundtrip(&Frame::StopSending { stream_id: 0, app_error_code: 0x101 });
        roundtrip(&Frame::ResetStream { stream_id: 3, app_error_code: 2, final_size: 999 });
    }

    #[test]
    fn ping_wire_shape() {
        // PING is a single type byte 0x01 with no payload.
        let mut buf = Vec::new();
        Frame::Ping.encode(&mut buf).unwrap();
        assert_eq!(buf, [0x01]);
    }

    #[test]
    fn padding_coalesces_run_of_zeros() {
        // Five leading zero bytes → one Padding(5); a trailing PING remains.
        let buf = [0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        let (frame, consumed) = Frame::parse(&buf).unwrap();
        assert_eq!(frame, Frame::Padding(5));
        assert_eq!(consumed, 5);
        let (ping, _) = Frame::parse(&buf[consumed..]).unwrap();
        assert_eq!(ping, Frame::Ping);
    }

    #[test]
    fn padding_encode_roundtrip() {
        let mut buf = Vec::new();
        Frame::Padding(3).encode(&mut buf).unwrap();
        assert_eq!(buf, [0x00, 0x00, 0x00]);
        assert_eq!(Frame::parse(&buf).unwrap(), (Frame::Padding(3), 3));
    }

    #[test]
    fn ack_without_ecn_roundtrips() {
        let frame = Frame::Ack {
            largest_acked: 10,
            ack_delay: 3,
            first_ack_range: 2,
            ranges: vec![AckRange { gap: 1, length: 4 }, AckRange { gap: 0, length: 0 }],
            ecn: None,
        };
        roundtrip(&frame);
        assert_eq!(frame.frame_type(), TYPE_ACK);
        assert!(!frame.is_ack_eliciting());
    }

    #[test]
    fn ack_with_ecn_roundtrips_and_switches_type() {
        let frame = Frame::Ack {
            largest_acked: 100,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: vec![],
            ecn: Some(EcnCounts { ect0: 5, ect1: 6, ecn_ce: 1 }),
        };
        roundtrip(&frame);
        assert_eq!(frame.frame_type(), TYPE_ACK_ECN);
    }

    #[test]
    fn crypto_and_new_token_roundtrip() {
        roundtrip(&Frame::Crypto { offset: 0, data: b"\x16\x03\x03handshake".to_vec() });
        roundtrip(&Frame::Crypto { offset: 4096, data: vec![] });
        roundtrip(&Frame::NewToken(b"opaque-token-bytes".to_vec()));
    }

    #[test]
    fn stream_sets_off_bit_only_when_offset_nonzero() {
        // offset == 0 → OFF bit clear, type 0x0a (base|LEN).
        let mut buf = Vec::new();
        Frame::Stream { stream_id: 4, offset: 0, fin: false, data: b"hi".to_vec() }
            .encode(&mut buf)
            .unwrap();
        assert_eq!(buf[0], 0x0a);
        // offset != 0 → OFF bit set, type 0x0e (base|LEN|OFF).
        buf.clear();
        Frame::Stream { stream_id: 4, offset: 8, fin: true, data: b"hi".to_vec() }
            .encode(&mut buf)
            .unwrap();
        assert_eq!(buf[0], 0x0f); // base|LEN|OFF|FIN
    }

    #[test]
    fn stream_variants_roundtrip() {
        roundtrip(&Frame::Stream { stream_id: 0, offset: 0, fin: false, data: b"body".to_vec() });
        roundtrip(&Frame::Stream { stream_id: 4, offset: 1024, fin: true, data: vec![] });
        roundtrip(&Frame::Stream { stream_id: 8, offset: 0, fin: true, data: b"x".to_vec() });
    }

    #[test]
    fn stream_without_len_bit_reads_to_end() {
        // Type 0x08 (base only): stream_id 4, then the remaining bytes are data.
        let buf = [0x08, 0x04, b'a', b'b', b'c'];
        let (frame, consumed) = Frame::parse(&buf).unwrap();
        assert_eq!(
            frame,
            Frame::Stream { stream_id: 4, offset: 0, fin: false, data: b"abc".to_vec() }
        );
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn new_connection_id_roundtrips() {
        roundtrip(&Frame::NewConnectionId {
            sequence_number: 1,
            retire_prior_to: 0,
            connection_id: vec![0xde, 0xad, 0xbe, 0xef],
            stateless_reset_token: [7u8; 16],
        });
    }

    #[test]
    fn new_connection_id_rejects_bad_length_on_decode() {
        // Sequence 0, retire 0, length byte 0 (invalid: must be ≥ 1).
        let buf = [TYPE_NEW_CONNECTION_ID as u8, 0x00, 0x00, 0x00];
        assert_eq!(Frame::parse(&buf), Err(QuicFrameError::InvalidConnectionIdLen(0)));
        // Length 21 is above the maximum of 20.
        let buf = [TYPE_NEW_CONNECTION_ID as u8, 0x00, 0x00, 21];
        assert_eq!(Frame::parse(&buf), Err(QuicFrameError::InvalidConnectionIdLen(21)));
    }

    #[test]
    fn new_connection_id_rejects_bad_length_on_encode() {
        let frame = Frame::NewConnectionId {
            sequence_number: 0,
            retire_prior_to: 0,
            connection_id: vec![], // empty is invalid
            stateless_reset_token: [0u8; 16],
        };
        let mut buf = Vec::new();
        assert_eq!(frame.encode(&mut buf), Err(QuicFrameError::InvalidConnectionIdLen(0)));
    }

    #[test]
    fn path_challenge_and_response_roundtrip() {
        roundtrip(&Frame::PathChallenge([1, 2, 3, 4, 5, 6, 7, 8]));
        roundtrip(&Frame::PathResponse([8, 7, 6, 5, 4, 3, 2, 1]));
    }

    #[test]
    fn connection_close_transport_and_app() {
        let transport = Frame::ConnectionClose {
            error_code: FRAME_ENCODING_ERROR,
            frame_type: Some(TYPE_STREAM_BASE),
            reason: b"bad frame".to_vec(),
        };
        roundtrip(&transport);
        assert_eq!(transport.frame_type(), TYPE_CONNECTION_CLOSE_TRANSPORT);
        assert!(!transport.is_ack_eliciting());

        let app = Frame::ConnectionClose {
            error_code: 0x100,
            frame_type: None,
            reason: vec![],
        };
        roundtrip(&app);
        assert_eq!(app.frame_type(), TYPE_CONNECTION_CLOSE_APP);
    }

    #[test]
    fn unknown_frame_type_is_error() {
        // 0x20 is unassigned in RFC 9000 §19.
        assert_eq!(Frame::parse(&[0x20]), Err(QuicFrameError::UnknownType(0x20)));
    }

    #[test]
    fn truncated_field_is_eof() {
        // RESET_STREAM needs three varints; give only the type and one.
        let buf = [TYPE_RESET_STREAM as u8, 0x04];
        assert_eq!(Frame::parse(&buf), Err(QuicFrameError::UnexpectedEof));
        // Empty input has no type byte.
        assert_eq!(Frame::parse(&[]), Err(QuicFrameError::UnexpectedEof));
    }

    #[test]
    fn length_prefixed_over_buffer_is_error() {
        // CRYPTO offset 0, length 5, but no data bytes follow.
        let buf = [TYPE_CRYPTO as u8, 0x00, 0x05];
        assert_eq!(Frame::parse(&buf), Err(QuicFrameError::LengthTooLong));
    }

    #[test]
    fn ack_range_count_bounded_by_buffer() {
        // An ACK claiming 200 ranges but carrying none must be rejected rather
        // than pre-allocating for a count the buffer cannot hold.
        let mut b = vec![TYPE_ACK as u8];
        varint::encode(0, &mut b).unwrap(); // largest_acked
        varint::encode(0, &mut b).unwrap(); // ack_delay
        varint::encode(200, &mut b).unwrap(); // range_count
        varint::encode(0, &mut b).unwrap(); // first_ack_range
        // No range bytes follow → count exceeds the remaining input.
        assert_eq!(Frame::parse(&b), Err(QuicFrameError::LengthTooLong));
    }

    #[test]
    fn parse_all_and_encode_all_roundtrip_a_packet() {
        let frames = vec![
            Frame::Ack {
                largest_acked: 5,
                ack_delay: 1,
                first_ack_range: 5,
                ranges: vec![],
                ecn: None,
            },
            Frame::Crypto { offset: 0, data: b"CH".to_vec() },
            Frame::Stream { stream_id: 0, offset: 0, fin: true, data: b"GET /".to_vec() },
            Frame::Padding(2),
        ];
        let mut buf = Vec::new();
        encode_all(&frames, &mut buf).unwrap();
        let decoded = parse_all(&buf).unwrap();
        assert_eq!(decoded, frames);
    }

    #[test]
    fn ack_eliciting_classification() {
        assert!(Frame::Ping.is_ack_eliciting());
        assert!(Frame::Stream { stream_id: 0, offset: 0, fin: false, data: vec![] }
            .is_ack_eliciting());
        assert!(!Frame::Padding(1).is_ack_eliciting());
    }

    #[test]
    fn large_varint_values_roundtrip() {
        roundtrip(&Frame::MaxData(varint::MAX_VARINT));
        roundtrip(&Frame::Crypto { offset: varint::MAX_VARINT, data: vec![0xab] });
    }
}
