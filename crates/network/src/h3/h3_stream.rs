//! HTTP/3 stream layer — unidirectional stream-type demux + per-stream frame
//! sequencing (RFC 9114 §6.2, §7.1, §4.1).
//!
//! Slices 1–11 built the wire codecs (QUIC varints, transport/HTTP-3 frames,
//! packet headers, QPACK) and the QUIC transport control logic (loss recovery,
//! PTO, per-stream reassembly, connection-level flow control). Those layers move
//! *bytes* between endpoints. This slice adds the piece that turns a decoded
//! stream of bytes into a well-formed HTTP/3 exchange: it classifies each
//! unidirectional stream by its leading **stream type** (RFC 9114 §6.2), enforces
//! the "exactly one" rule for the critical control / QPACK streams, and validates
//! the **frame grammar** of the control stream (RFC 9114 §6.2.1) and the request
//! stream (RFC 9114 §4.1, §7.1).
//!
//! Like every slice so far it is a pure state machine — no IO, no packet
//! protection, no timers. The connection layer decodes [`super::frame::Frame`]s
//! off each stream (via the slice-1 codec) and feeds them here in order; this
//! module answers whether the sequence is legal and, if not, which RFC 9114 §8.1
//! error to close the connection with.
//!
//! ## Unidirectional stream types (RFC 9114 §6.2, RFC 9204 §4.2)
//!
//! The first varint on a client- or server-initiated unidirectional stream is a
//! *stream type*. [`UniStreamType::parse`] decodes it (and, for a push stream,
//! the Push ID that immediately follows, RFC 9114 §6.2.2). Unknown/reserved types
//! are surfaced as [`UniStreamType::Reserved`] so the connection layer can abort
//! reading by policy (RFC 9114 §6.2.3) rather than the codec guessing. Each
//! endpoint may open **exactly one** control, QPACK-encoder, and QPACK-decoder
//! stream; [`UniStreamRegistry`] rejects a duplicate as `H3_STREAM_CREATION_ERROR`
//! and a missing-before-use with the same error, and flags the close of one of
//! those critical streams as `H3_CLOSED_CRITICAL_STREAM`.
//!
//! ## Control stream grammar (RFC 9114 §6.2.1)
//!
//! [`ControlStream`] enforces that the **first** frame on the control stream is
//! SETTINGS (`H3_MISSING_SETTINGS` otherwise), that SETTINGS appears at most once
//! (`H3_FRAME_UNEXPECTED` on a repeat), and that only control-stream frames
//! (SETTINGS, GOAWAY, MAX_PUSH_ID, CANCEL_PUSH, and reserved) appear — a
//! request-stream frame (DATA/HEADERS/PUSH_PROMISE) is `H3_FRAME_UNEXPECTED`.
//!
//! ## Request stream grammar (RFC 9114 §4.1, §7.1)
//!
//! [`RequestStream`] enforces the message framing on a client-initiated
//! bidirectional stream: one or more leading HEADERS frames (the header section,
//! plus any informational responses), then zero or more DATA frames (the body),
//! then at most one trailing HEADERS (the trailer section); a server may
//! interleave PUSH_PROMISE. DATA before the header section, a control-only frame,
//! or any frame after the trailer section is `H3_FRAME_UNEXPECTED`.
//!
//! ## Out of scope (later slices)
//!
//! - The QPACK encoder/decoder-stream *instruction* grammar — that is the
//!   slice-6 [`super::qpack_stream`] codec; this module only routes bytes to it.
//! - Actually reading frames off the wire, arming timers, header protection,
//!   AEAD, TLS, and `h3_do_request` dispatch.

use super::frame::{
    Frame, H3_FRAME_UNEXPECTED, TYPE_CANCEL_PUSH, TYPE_DATA, TYPE_GOAWAY, TYPE_HEADERS,
    TYPE_MAX_PUSH_ID, TYPE_PUSH_PROMISE, TYPE_SETTINGS,
};
use super::varint;

// ── Unidirectional stream type codes (RFC 9114 §6.2, RFC 9204 §4.2) ──────────

/// Control stream type (RFC 9114 §6.2.1).
pub const STREAM_TYPE_CONTROL: u64 = 0x00;
/// Push stream type (RFC 9114 §6.2.2); followed by a Push ID varint.
pub const STREAM_TYPE_PUSH: u64 = 0x01;
/// QPACK encoder stream type (RFC 9204 §4.2).
pub const STREAM_TYPE_QPACK_ENCODER: u64 = 0x02;
/// QPACK decoder stream type (RFC 9204 §4.2).
pub const STREAM_TYPE_QPACK_DECODER: u64 = 0x03;

// ── HTTP/3 error codes added by this layer (RFC 9114 §8.1) ───────────────────
//
// `H3_FRAME_UNEXPECTED` (0x0105) is defined in [`super::frame`] and reused here.

/// `H3_STREAM_CREATION_ERROR` (RFC 9114 §8.1) — a stream was created that is not
/// permitted, e.g. a second control / QPACK-encoder / QPACK-decoder stream, or
/// use of one of those streams before it exists.
pub const H3_STREAM_CREATION_ERROR: u64 = 0x0103;
/// `H3_CLOSED_CRITICAL_STREAM` (RFC 9114 §8.1) — the control or a QPACK stream
/// was closed; these must remain open for the connection's lifetime.
pub const H3_CLOSED_CRITICAL_STREAM: u64 = 0x0104;
/// `H3_MISSING_SETTINGS` (RFC 9114 §8.1) — the first frame on the control stream
/// was not SETTINGS.
pub const H3_MISSING_SETTINGS: u64 = 0x010a;

// ── Errors ───────────────────────────────────────────────────────────────────

/// A stream-layer protocol violation. Each variant maps to exactly one RFC 9114
/// §8.1 wire error code via [`StreamLayerError::code`]; the connection layer
/// emits it in a `CONNECTION_CLOSE`. The message preserves *why* for diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamLayerError {
    /// A frame appeared where the stream's grammar does not allow it →
    /// `H3_FRAME_UNEXPECTED`.
    FrameUnexpected(String),
    /// The first control-stream frame was not SETTINGS → `H3_MISSING_SETTINGS`.
    MissingSettings,
    /// A stream was created that is not permitted (duplicate or premature use of
    /// a critical stream) → `H3_STREAM_CREATION_ERROR`.
    StreamCreation(String),
    /// A critical stream (control / QPACK encoder / QPACK decoder) was closed →
    /// `H3_CLOSED_CRITICAL_STREAM`.
    ClosedCriticalStream(String),
}

impl StreamLayerError {
    /// Map to the RFC 9114 §8.1 wire error code.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::FrameUnexpected(_) => H3_FRAME_UNEXPECTED,
            Self::MissingSettings => H3_MISSING_SETTINGS,
            Self::StreamCreation(_) => H3_STREAM_CREATION_ERROR,
            Self::ClosedCriticalStream(_) => H3_CLOSED_CRITICAL_STREAM,
        }
    }
}

impl core::fmt::Display for StreamLayerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FrameUnexpected(m) => write!(f, "H3_FRAME_UNEXPECTED: {m}"),
            Self::MissingSettings => {
                write!(f, "H3_MISSING_SETTINGS: first control-stream frame was not SETTINGS")
            }
            Self::StreamCreation(m) => write!(f, "H3_STREAM_CREATION_ERROR: {m}"),
            Self::ClosedCriticalStream(m) => write!(f, "H3_CLOSED_CRITICAL_STREAM: {m}"),
        }
    }
}

impl std::error::Error for StreamLayerError {}

// ── Unidirectional stream types ──────────────────────────────────────────────

/// The decoded type of a unidirectional stream (RFC 9114 §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UniStreamType {
    /// The control stream (RFC 9114 §6.2.1). Exactly one per endpoint.
    Control,
    /// A push stream (RFC 9114 §6.2.2), carrying the Push ID that follows the
    /// stream type on the wire.
    Push(u64),
    /// The QPACK encoder stream (RFC 9204 §4.2). Exactly one per endpoint.
    QpackEncoder,
    /// The QPACK decoder stream (RFC 9204 §4.2). Exactly one per endpoint.
    QpackDecoder,
    /// A reserved or greased stream type (RFC 9114 §6.2.3) the endpoint must
    /// ignore. Preserves the raw type code rather than the codec guessing.
    Reserved(u64),
}

impl UniStreamType {
    /// Parse the stream-type prefix from the front of `buf`.
    ///
    /// Returns `Ok(None)` while `buf` does not yet hold the full prefix (for a
    /// push stream that means both the type and the Push ID varint), and
    /// `Ok(Some((ty, consumed)))` once the prefix is complete, where `consumed`
    /// is the number of leading bytes the prefix occupied. There is no error
    /// path: every varint is a valid stream type (unknown ones become
    /// [`UniStreamType::Reserved`]).
    #[must_use]
    pub fn parse(buf: &[u8]) -> Option<(Self, usize)> {
        let (ty, tlen) = varint::decode(buf)?;
        match ty {
            STREAM_TYPE_CONTROL => Some((Self::Control, tlen)),
            STREAM_TYPE_PUSH => {
                let (push_id, plen) = varint::decode(&buf[tlen..])?;
                Some((Self::Push(push_id), tlen + plen))
            }
            STREAM_TYPE_QPACK_ENCODER => Some((Self::QpackEncoder, tlen)),
            STREAM_TYPE_QPACK_DECODER => Some((Self::QpackDecoder, tlen)),
            other => Some((Self::Reserved(other), tlen)),
        }
    }

    /// Whether this is a critical stream that must stay open for the connection's
    /// lifetime (control, QPACK encoder, QPACK decoder — RFC 9114 §6.2.1,
    /// RFC 9204 §4.2). Closing one is `H3_CLOSED_CRITICAL_STREAM`.
    #[must_use]
    pub const fn is_critical(&self) -> bool {
        matches!(self, Self::Control | Self::QpackEncoder | Self::QpackDecoder)
    }
}

/// Tracks the singleton unidirectional streams an endpoint has opened, enforcing
/// the "exactly one control / QPACK-encoder / QPACK-decoder stream" rule
/// (RFC 9114 §6.2.1, RFC 9204 §4.2). Push and reserved streams are unconstrained
/// in count, so they are not tracked here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UniStreamRegistry {
    control: bool,
    qpack_encoder: bool,
    qpack_decoder: bool,
}

impl UniStreamRegistry {
    /// A fresh registry with no streams opened yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { control: false, qpack_encoder: false, qpack_decoder: false }
    }

    /// Register a newly opened unidirectional stream of type `ty`.
    ///
    /// # Errors
    ///
    /// [`StreamLayerError::StreamCreation`] if a control, QPACK-encoder, or
    /// QPACK-decoder stream of the same kind has already been opened (RFC 9114
    /// §6.2.1, RFC 9204 §4.2). Push and reserved streams always succeed.
    pub fn open(&mut self, ty: UniStreamType) -> Result<(), StreamLayerError> {
        let slot = match ty {
            UniStreamType::Control => &mut self.control,
            UniStreamType::QpackEncoder => &mut self.qpack_encoder,
            UniStreamType::QpackDecoder => &mut self.qpack_decoder,
            UniStreamType::Push(_) | UniStreamType::Reserved(_) => return Ok(()),
        };
        if *slot {
            return Err(StreamLayerError::StreamCreation(format!(
                "second {ty:?} stream"
            )));
        }
        *slot = true;
        Ok(())
    }

    /// Whether the control stream has been opened.
    #[must_use]
    pub const fn has_control(&self) -> bool {
        self.control
    }

    /// Whether the QPACK encoder stream has been opened.
    #[must_use]
    pub const fn has_qpack_encoder(&self) -> bool {
        self.qpack_encoder
    }

    /// Whether the QPACK decoder stream has been opened.
    #[must_use]
    pub const fn has_qpack_decoder(&self) -> bool {
        self.qpack_decoder
    }

    /// Report that a stream of type `ty` closed.
    ///
    /// # Errors
    ///
    /// [`StreamLayerError::ClosedCriticalStream`] if `ty` is a critical stream
    /// (control / QPACK encoder / QPACK decoder), which must remain open for the
    /// connection's lifetime (RFC 9114 §6.2.1). Push/reserved streams closing is
    /// fine.
    pub fn close(&self, ty: UniStreamType) -> Result<(), StreamLayerError> {
        if ty.is_critical() {
            return Err(StreamLayerError::ClosedCriticalStream(format!(
                "{ty:?} stream closed"
            )));
        }
        Ok(())
    }
}

// ── Frame classification (RFC 9114 §7.2) ─────────────────────────────────────

/// Whether `frame_type` is a control-stream frame (SETTINGS, GOAWAY,
/// MAX_PUSH_ID, CANCEL_PUSH — RFC 9114 §7.2). CANCEL_PUSH is technically valid on
/// both the client and server control streams; this classifier is used to reject
/// it on a *request* stream.
#[must_use]
pub const fn is_control_frame_type(frame_type: u64) -> bool {
    matches!(
        frame_type,
        TYPE_SETTINGS | TYPE_GOAWAY | TYPE_MAX_PUSH_ID | TYPE_CANCEL_PUSH
    )
}

/// Whether `frame_type` is a request/response-stream frame (DATA, HEADERS,
/// PUSH_PROMISE — RFC 9114 §7.2). Used to reject these on the control stream.
#[must_use]
pub const fn is_request_frame_type(frame_type: u64) -> bool {
    matches!(frame_type, TYPE_DATA | TYPE_HEADERS | TYPE_PUSH_PROMISE)
}

// ── Control stream grammar (RFC 9114 §6.2.1) ─────────────────────────────────

/// Frame sequencer for the HTTP/3 control stream (RFC 9114 §6.2.1). The first
/// frame must be SETTINGS; thereafter only GOAWAY, MAX_PUSH_ID, CANCEL_PUSH, and
/// reserved frames are permitted (SETTINGS at most once).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControlStream {
    settings_seen: bool,
}

impl ControlStream {
    /// A control-stream sequencer awaiting its mandatory SETTINGS frame.
    #[must_use]
    pub const fn new() -> Self {
        Self { settings_seen: false }
    }

    /// Whether the mandatory SETTINGS frame has been accepted.
    #[must_use]
    pub const fn settings_seen(&self) -> bool {
        self.settings_seen
    }

    /// Validate the next frame on the control stream.
    ///
    /// # Errors
    ///
    /// - [`StreamLayerError::MissingSettings`] if the first frame is not SETTINGS.
    /// - [`StreamLayerError::FrameUnexpected`] for a repeated SETTINGS, a
    ///   request-stream frame (DATA/HEADERS/PUSH_PROMISE), or any other frame not
    ///   valid on the control stream (RFC 9114 §7.2).
    pub fn accept(&mut self, frame: &Frame) -> Result<(), StreamLayerError> {
        let ty = frame.frame_type();
        if !self.settings_seen {
            // RFC 9114 §6.2.1: the control stream's first frame is SETTINGS. A
            // reserved (greased) frame type is *not* allowed to precede it.
            if ty != TYPE_SETTINGS {
                return Err(StreamLayerError::MissingSettings);
            }
            self.settings_seen = true;
            return Ok(());
        }
        match ty {
            TYPE_SETTINGS => Err(StreamLayerError::FrameUnexpected(
                "second SETTINGS on control stream".into(),
            )),
            _ if is_request_frame_type(ty) => Err(StreamLayerError::FrameUnexpected(format!(
                "request frame 0x{ty:02x} on control stream"
            ))),
            // GOAWAY, MAX_PUSH_ID, CANCEL_PUSH, and reserved/greased types are
            // all valid to ignore-or-handle after SETTINGS.
            _ => Ok(()),
        }
    }
}

// ── Request stream grammar (RFC 9114 §4.1, §7.1) ─────────────────────────────

/// The frame-sequencing state of a request/response stream (RFC 9114 §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestState {
    /// No frame accepted yet; awaiting the first HEADERS (the header section).
    Init,
    /// One or more HEADERS accepted, no DATA yet (header section / interim
    /// responses).
    Headers,
    /// At least one DATA frame accepted (the message body).
    Data,
    /// The trailing HEADERS (trailer section) has been accepted; the stream is
    /// complete and admits no further frames.
    Trailers,
}

/// Frame sequencer for a request/response stream (RFC 9114 §4.1, §7.1): a header
/// section (one or more HEADERS, allowing informational responses), then a body
/// (zero or more DATA), then an optional trailer section (one HEADERS). A server
/// may interleave PUSH_PROMISE after the header section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestStream {
    state: RequestState,
}

impl Default for RequestStream {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestStream {
    /// A request-stream sequencer awaiting the first HEADERS frame.
    #[must_use]
    pub const fn new() -> Self {
        Self { state: RequestState::Init }
    }

    /// The current sequencing state.
    #[must_use]
    pub const fn state(&self) -> RequestState {
        self.state
    }

    /// Validate the next frame on a request/response stream and advance the
    /// state machine.
    ///
    /// # Errors
    ///
    /// [`StreamLayerError::FrameUnexpected`] if the frame violates the RFC 9114
    /// §4.1 message framing: a control-only frame (SETTINGS/GOAWAY/MAX_PUSH_ID/
    /// CANCEL_PUSH), DATA before the header section, PUSH_PROMISE before the
    /// header section, or any frame after the trailer section.
    pub fn accept(&mut self, frame: &Frame) -> Result<(), StreamLayerError> {
        let ty = frame.frame_type();
        // Control-stream frames never belong on a request stream (RFC 9114 §7.2).
        if is_control_frame_type(ty) {
            return Err(StreamLayerError::FrameUnexpected(format!(
                "control frame 0x{ty:02x} on request stream"
            )));
        }
        match self.state {
            RequestState::Init => match ty {
                TYPE_HEADERS => {
                    self.state = RequestState::Headers;
                    Ok(())
                }
                TYPE_DATA => Err(StreamLayerError::FrameUnexpected(
                    "DATA before header section".into(),
                )),
                TYPE_PUSH_PROMISE => Err(StreamLayerError::FrameUnexpected(
                    "PUSH_PROMISE before header section".into(),
                )),
                // Reserved/greased frame types are ignored anywhere (RFC 9114
                // §9): they neither advance nor break the grammar.
                _ => Ok(()),
            },
            RequestState::Headers => match ty {
                // A further HEADERS is an interim (1xx) response's header section
                // or the final response's — both stay in the header phase. A
                // trailer section only follows the body, so it cannot appear here.
                TYPE_HEADERS | TYPE_PUSH_PROMISE => Ok(()),
                TYPE_DATA => {
                    self.state = RequestState::Data;
                    Ok(())
                }
                _ => Ok(()),
            },
            RequestState::Data => match ty {
                TYPE_DATA | TYPE_PUSH_PROMISE => Ok(()),
                // The first HEADERS after the body is the trailer section; no
                // frame may follow it.
                TYPE_HEADERS => {
                    self.state = RequestState::Trailers;
                    Ok(())
                }
                _ => Ok(()),
            },
            RequestState::Trailers => {
                // Nothing (not even a reserved frame) may follow the trailers.
                Err(StreamLayerError::FrameUnexpected(format!(
                    "frame 0x{ty:02x} after trailer section"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── UniStreamType::parse ────────────────────────────────────────────────

    #[test]
    fn parse_control_encoder_decoder_types() {
        assert_eq!(UniStreamType::parse(&[0x00]), Some((UniStreamType::Control, 1)));
        assert_eq!(UniStreamType::parse(&[0x02]), Some((UniStreamType::QpackEncoder, 1)));
        assert_eq!(UniStreamType::parse(&[0x03]), Some((UniStreamType::QpackDecoder, 1)));
    }

    #[test]
    fn parse_push_stream_reads_push_id() {
        // type 0x01, push id 0x05.
        assert_eq!(UniStreamType::parse(&[0x01, 0x05]), Some((UniStreamType::Push(5), 2)));
    }

    #[test]
    fn parse_push_stream_incomplete_when_id_missing() {
        // type present, push id varint not yet buffered.
        assert_eq!(UniStreamType::parse(&[0x01]), None);
    }

    #[test]
    fn parse_empty_buffer_is_incomplete() {
        assert_eq!(UniStreamType::parse(&[]), None);
    }

    #[test]
    fn parse_unknown_type_is_reserved() {
        // A 2-byte varint grease value 0x1f*1 + 0x21 = 0x40 → wire 0x40 0x40.
        let grease: u64 = 0x1f + 0x21;
        let mut buf = Vec::new();
        varint::encode(grease, &mut buf).unwrap();
        assert_eq!(UniStreamType::parse(&buf), Some((UniStreamType::Reserved(grease), buf.len())));
    }

    #[test]
    fn critical_classification() {
        assert!(UniStreamType::Control.is_critical());
        assert!(UniStreamType::QpackEncoder.is_critical());
        assert!(UniStreamType::QpackDecoder.is_critical());
        assert!(!UniStreamType::Push(1).is_critical());
        assert!(!UniStreamType::Reserved(0x40).is_critical());
    }

    // ── UniStreamRegistry ───────────────────────────────────────────────────

    #[test]
    fn registry_accepts_one_of_each_critical() {
        let mut reg = UniStreamRegistry::new();
        assert!(reg.open(UniStreamType::Control).is_ok());
        assert!(reg.open(UniStreamType::QpackEncoder).is_ok());
        assert!(reg.open(UniStreamType::QpackDecoder).is_ok());
        assert!(reg.has_control() && reg.has_qpack_encoder() && reg.has_qpack_decoder());
    }

    #[test]
    fn registry_rejects_second_control_stream() {
        let mut reg = UniStreamRegistry::new();
        reg.open(UniStreamType::Control).unwrap();
        let err = reg.open(UniStreamType::Control).unwrap_err();
        assert!(matches!(err, StreamLayerError::StreamCreation(_)));
        assert_eq!(err.code(), H3_STREAM_CREATION_ERROR);
    }

    #[test]
    fn registry_rejects_second_qpack_encoder() {
        let mut reg = UniStreamRegistry::new();
        reg.open(UniStreamType::QpackEncoder).unwrap();
        assert!(matches!(
            reg.open(UniStreamType::QpackEncoder).unwrap_err(),
            StreamLayerError::StreamCreation(_)
        ));
    }

    #[test]
    fn registry_allows_many_push_and_reserved_streams() {
        let mut reg = UniStreamRegistry::new();
        for id in 0..5 {
            reg.open(UniStreamType::Push(id)).unwrap();
        }
        reg.open(UniStreamType::Reserved(0x40)).unwrap();
        reg.open(UniStreamType::Reserved(0x40)).unwrap();
    }

    #[test]
    fn registry_close_of_critical_stream_is_error() {
        let reg = UniStreamRegistry::new();
        let err = reg.close(UniStreamType::Control).unwrap_err();
        assert!(matches!(err, StreamLayerError::ClosedCriticalStream(_)));
        assert_eq!(err.code(), H3_CLOSED_CRITICAL_STREAM);
    }

    #[test]
    fn registry_close_of_push_stream_is_ok() {
        let reg = UniStreamRegistry::new();
        assert!(reg.close(UniStreamType::Push(1)).is_ok());
        assert!(reg.close(UniStreamType::Reserved(0x40)).is_ok());
    }

    // ── ControlStream ───────────────────────────────────────────────────────

    #[test]
    fn control_requires_settings_first() {
        let mut cs = ControlStream::new();
        let err = cs.accept(&Frame::GoAway(0)).unwrap_err();
        assert!(matches!(err, StreamLayerError::MissingSettings));
        assert_eq!(err.code(), H3_MISSING_SETTINGS);
        assert!(!cs.settings_seen());
    }

    #[test]
    fn control_reserved_frame_cannot_precede_settings() {
        // A greased frame before SETTINGS is still H3_MISSING_SETTINGS.
        let mut cs = ControlStream::new();
        let reserved = Frame::Reserved { frame_type: 0x1f + 0x21, payload: vec![] };
        assert!(matches!(cs.accept(&reserved).unwrap_err(), StreamLayerError::MissingSettings));
    }

    #[test]
    fn control_accepts_settings_then_control_frames() {
        let mut cs = ControlStream::new();
        cs.accept(&Frame::Settings(vec![])).unwrap();
        assert!(cs.settings_seen());
        cs.accept(&Frame::MaxPushId(10)).unwrap();
        cs.accept(&Frame::CancelPush(3)).unwrap();
        cs.accept(&Frame::Reserved { frame_type: 0x40, payload: vec![] }).unwrap();
        cs.accept(&Frame::GoAway(0)).unwrap();
    }

    #[test]
    fn control_rejects_second_settings() {
        let mut cs = ControlStream::new();
        cs.accept(&Frame::Settings(vec![])).unwrap();
        let err = cs.accept(&Frame::Settings(vec![])).unwrap_err();
        assert!(matches!(err, StreamLayerError::FrameUnexpected(_)));
        assert_eq!(err.code(), H3_FRAME_UNEXPECTED);
    }

    #[test]
    fn control_rejects_request_frames() {
        for frame in [
            Frame::Data(vec![1]),
            Frame::Headers(vec![1]),
            Frame::PushPromise { push_id: 1, block: vec![] },
        ] {
            let mut cs = ControlStream::new();
            cs.accept(&Frame::Settings(vec![])).unwrap();
            let err = cs.accept(&frame).unwrap_err();
            assert!(matches!(err, StreamLayerError::FrameUnexpected(_)), "{frame:?}");
        }
    }

    // ── RequestStream ───────────────────────────────────────────────────────

    #[test]
    fn request_headers_data_trailers_sequence() {
        let mut rs = RequestStream::new();
        assert_eq!(rs.state(), RequestState::Init);
        rs.accept(&Frame::Headers(vec![1])).unwrap();
        assert_eq!(rs.state(), RequestState::Headers);
        rs.accept(&Frame::Data(vec![2])).unwrap();
        assert_eq!(rs.state(), RequestState::Data);
        rs.accept(&Frame::Data(vec![3])).unwrap();
        rs.accept(&Frame::Headers(vec![4])).unwrap(); // trailers
        assert_eq!(rs.state(), RequestState::Trailers);
    }

    #[test]
    fn request_body_optional() {
        // HEADERS with no body is a legal exchange (e.g. 204 response).
        let mut rs = RequestStream::new();
        rs.accept(&Frame::Headers(vec![1])).unwrap();
        assert_eq!(rs.state(), RequestState::Headers);
    }

    #[test]
    fn request_interim_responses_multiple_leading_headers() {
        // 1xx informational HEADERS then the final response HEADERS.
        let mut rs = RequestStream::new();
        rs.accept(&Frame::Headers(vec![1])).unwrap();
        rs.accept(&Frame::Headers(vec![2])).unwrap();
        assert_eq!(rs.state(), RequestState::Headers);
        rs.accept(&Frame::Data(vec![3])).unwrap();
        assert_eq!(rs.state(), RequestState::Data);
    }

    #[test]
    fn request_data_before_headers_is_unexpected() {
        let mut rs = RequestStream::new();
        let err = rs.accept(&Frame::Data(vec![1])).unwrap_err();
        assert!(matches!(err, StreamLayerError::FrameUnexpected(_)));
        assert_eq!(err.code(), H3_FRAME_UNEXPECTED);
    }

    #[test]
    fn request_push_promise_before_headers_is_unexpected() {
        let mut rs = RequestStream::new();
        let err = rs
            .accept(&Frame::PushPromise { push_id: 1, block: vec![] })
            .unwrap_err();
        assert!(matches!(err, StreamLayerError::FrameUnexpected(_)));
    }

    #[test]
    fn request_push_promise_after_headers_ok() {
        let mut rs = RequestStream::new();
        rs.accept(&Frame::Headers(vec![1])).unwrap();
        rs.accept(&Frame::PushPromise { push_id: 1, block: vec![] }).unwrap();
        rs.accept(&Frame::Data(vec![2])).unwrap();
        rs.accept(&Frame::PushPromise { push_id: 2, block: vec![] }).unwrap();
    }

    #[test]
    fn request_rejects_control_frames() {
        for frame in [
            Frame::Settings(vec![]),
            Frame::GoAway(0),
            Frame::MaxPushId(1),
            Frame::CancelPush(1),
        ] {
            let mut rs = RequestStream::new();
            rs.accept(&Frame::Headers(vec![1])).unwrap();
            let err = rs.accept(&frame).unwrap_err();
            assert!(matches!(err, StreamLayerError::FrameUnexpected(_)), "{frame:?}");
        }
    }

    #[test]
    fn request_no_frame_after_trailers() {
        let mut rs = RequestStream::new();
        rs.accept(&Frame::Headers(vec![1])).unwrap();
        rs.accept(&Frame::Data(vec![2])).unwrap();
        rs.accept(&Frame::Headers(vec![3])).unwrap(); // trailers
        for frame in [Frame::Data(vec![4]), Frame::Headers(vec![5])] {
            let mut after = rs;
            let err = after.accept(&frame).unwrap_err();
            assert!(matches!(err, StreamLayerError::FrameUnexpected(_)), "{frame:?}");
        }
    }

    #[test]
    fn request_reserved_frame_ignored_mid_stream() {
        let mut rs = RequestStream::new();
        // A greased frame before HEADERS is ignored (does not start the body).
        rs.accept(&Frame::Reserved { frame_type: 0x40, payload: vec![] }).unwrap();
        assert_eq!(rs.state(), RequestState::Init);
        rs.accept(&Frame::Headers(vec![1])).unwrap();
        rs.accept(&Frame::Reserved { frame_type: 0x40, payload: vec![] }).unwrap();
        assert_eq!(rs.state(), RequestState::Headers);
    }

    #[test]
    fn frame_classifiers() {
        assert!(is_control_frame_type(TYPE_SETTINGS));
        assert!(is_control_frame_type(TYPE_GOAWAY));
        assert!(is_control_frame_type(TYPE_MAX_PUSH_ID));
        assert!(is_control_frame_type(TYPE_CANCEL_PUSH));
        assert!(!is_control_frame_type(TYPE_DATA));
        assert!(is_request_frame_type(TYPE_DATA));
        assert!(is_request_frame_type(TYPE_HEADERS));
        assert!(is_request_frame_type(TYPE_PUSH_PROMISE));
        assert!(!is_request_frame_type(TYPE_SETTINGS));
    }
}
