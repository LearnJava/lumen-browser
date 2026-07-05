//! QUIC packet payload assembly (RFC 9000 §12.4, §13.2.1, §14.1; RFC 9002 §2).
//!
//! A QUIC packet's protected payload is a back-to-back sequence of transport
//! frames ([`quic_frame`]). Before [`packet_crypt::encrypt_packet`] can seal a
//! packet, the connection layer must decide *which* frames to place in it —
//! honouring the RFC 9000 §12.4 rule that each packet type admits a different
//! frame set — and pack them into the space a single packet may occupy, padding
//! where the transport requires a minimum size (the 1200-byte client Initial
//! datagram of RFC 9000 §14.1, or a Path MTU Discovery probe of the size
//! [`path_mtu`] proposes).
//!
//! [`PacketType`] names the four frame-bearing packet types and answers, via
//! [`PacketType::permits`], the §12.4 permission table (which is finer than the
//! three loss-recovery packet-number spaces: 0-RTT and 1-RTT share the
//! Application Data space yet admit different frames — ACK, HANDSHAKE_DONE, and
//! NEW_TOKEN may not appear in a 0-RTT packet). [`PayloadBuilder`] accumulates
//! permitted frames up to a byte budget, tracking whether the assembled packet
//! is *ack-eliciting* (RFC 9000 §13.2.1 — obliges the peer to acknowledge) and
//! *in flight* (RFC 9002 §2 — counts against the congestion window), and pads to
//! a target size.
//!
//! Pure functions and state over byte buffers: no packet-number assignment (that
//! is [`packet_number`]), no header framing or encryption (that is
//! [`packet_crypt`]), and no IO. The caller places the finished payload bytes
//! into the packet header, assigns the packet number, and encrypts.
//!
//! [`quic_frame`]: super::quic_frame
//! [`packet_crypt`]: super::packet_crypt
//! [`packet_crypt::encrypt_packet`]: super::packet_crypt::encrypt_packet
//! [`packet_number`]: super::packet_number
//! [`path_mtu`]: super::path_mtu

use core::fmt;

use super::loss::PacketNumberSpace;
use super::quic_frame::{Frame, QuicFrameError};

/// One of the four QUIC packet types that carry frames (RFC 9000 §12.4).
///
/// This is a finer distinction than the three [`PacketNumberSpace`] values:
/// [`PacketType::ZeroRtt`] and [`PacketType::OneRtt`] both belong to the
/// Application Data space, yet RFC 9000 §12.4 permits a different frame set in
/// each — so the permission table ([`PacketType::permits`]) is keyed on the
/// packet type, not the number space. Retry and Version Negotiation packets
/// carry no frames and are therefore absent here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketType {
    /// Initial packet (RFC 9000 §17.2.2): CRYPTO / ACK during the handshake.
    Initial,
    /// 0-RTT packet (RFC 9000 §17.2.3): early application data, before the
    /// handshake completes.
    ZeroRtt,
    /// Handshake packet (RFC 9000 §17.2.4): CRYPTO / ACK in the Handshake space.
    Handshake,
    /// Short-header 1-RTT packet (RFC 9000 §17.3.1): application data after the
    /// handshake.
    OneRtt,
}

impl PacketType {
    /// The packet-number space this packet type belongs to (RFC 9000 §12.3):
    /// 0-RTT and 1-RTT both map to [`PacketNumberSpace::ApplicationData`].
    #[must_use]
    pub const fn number_space(self) -> PacketNumberSpace {
        match self {
            Self::Initial => PacketNumberSpace::Initial,
            Self::Handshake => PacketNumberSpace::Handshake,
            Self::ZeroRtt | Self::OneRtt => PacketNumberSpace::ApplicationData,
        }
    }

    /// Whether `frame` may appear in a packet of this type (RFC 9000 §12.4,
    /// Table 3).
    ///
    /// PADDING and PING are universal. ACK and CRYPTO appear in every type
    /// except 0-RTT. The stream, flow-control, and connection-management frames
    /// are confined to the application types (0-RTT and 1-RTT). NEW_TOKEN,
    /// PATH_RESPONSE, and HANDSHAKE_DONE are 1-RTT only. A transport
    /// CONNECTION_CLOSE (type `0x1c`) may appear in any type; an application
    /// CONNECTION_CLOSE (type `0x1d`) is barred from Initial and Handshake
    /// packets, where no application context yet exists (RFC 9000 §10.2.3).
    #[must_use]
    pub const fn permits(self, frame: &Frame) -> bool {
        use Frame::{
            Ack, ConnectionClose, Crypto, DataBlocked, HandshakeDone, MaxData, MaxStreamData,
            MaxStreams, NewConnectionId, NewToken, Padding, PathChallenge, PathResponse, Ping,
            ResetStream, RetireConnectionId, Stream, StreamDataBlocked, StreamsBlocked, StopSending,
        };
        match frame {
            // IH01 — every frame-bearing packet type.
            Padding(_) | Ping => true,
            // IH_1 — all but 0-RTT.
            Ack { .. } | Crypto { .. } => !matches!(self, Self::ZeroRtt),
            // ___1 — 1-RTT only.
            NewToken(_) | PathResponse(_) | HandshakeDone => matches!(self, Self::OneRtt),
            // __01 — the application (0-RTT / 1-RTT) types only.
            ResetStream { .. }
            | StopSending { .. }
            | Stream { .. }
            | MaxData(_)
            | MaxStreamData { .. }
            | MaxStreams { .. }
            | DataBlocked(_)
            | StreamDataBlocked { .. }
            | StreamsBlocked { .. }
            | NewConnectionId { .. }
            | RetireConnectionId(_)
            | PathChallenge(_) => matches!(self, Self::ZeroRtt | Self::OneRtt),
            // CONNECTION_CLOSE: the transport form (0x1c) is universal; the
            // application form (0x1d) is barred from Initial / Handshake.
            ConnectionClose { frame_type: Some(_), .. } => true,
            ConnectionClose { frame_type: None, .. } => matches!(self, Self::ZeroRtt | Self::OneRtt),
        }
    }
}

impl fmt::Display for PacketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Initial => "Initial",
            Self::ZeroRtt => "0-RTT",
            Self::Handshake => "Handshake",
            Self::OneRtt => "1-RTT",
        };
        f.write_str(name)
    }
}

/// Something that prevented a frame from being placed in a [`PayloadBuilder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// The frame is not permitted in this packet type (RFC 9000 §12.4). A peer
    /// that receives a frame in a packet type it is not allowed in must treat it
    /// as a `PROTOCOL_VIOLATION` (RFC 9000 §12.4), so this builder refuses to
    /// produce such a packet.
    FrameNotPermitted {
        /// The offending frame's wire type code (RFC 9000 §19).
        frame_type: u64,
        /// The packet type it was offered for.
        packet_type: PacketType,
    },
    /// The frame could not be serialized (a field exceeded the varint maximum).
    Encode(QuicFrameError),
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameNotPermitted { frame_type, packet_type } => write!(
                f,
                "frame type {frame_type:#x} is not permitted in a {packet_type} packet"
            ),
            Self::Encode(e) => write!(f, "frame encode failed: {e}"),
        }
    }
}

impl std::error::Error for PayloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(e) => Some(e),
            Self::FrameNotPermitted { .. } => None,
        }
    }
}

impl From<QuicFrameError> for PayloadError {
    fn from(e: QuicFrameError) -> Self {
        Self::Encode(e)
    }
}

/// Accumulates the frames of one QUIC packet's payload up to a byte budget
/// (RFC 9000 §12.4).
///
/// The `limit` is the number of payload bytes the packet may hold — the maximum
/// datagram size (from [`path_mtu`]) minus the header and the AEAD tag the
/// caller will add. Frames are offered with [`PayloadBuilder::try_push`], which
/// reports whether each fit; the builder tracks the two properties the
/// loss-recovery layer needs — [`PayloadBuilder::is_ack_eliciting`] (RFC 9000
/// §13.2.1) and [`PayloadBuilder::is_in_flight`] (RFC 9002 §2) — and
/// [`PayloadBuilder::pad_to`] tops the payload up to a required size.
///
/// [`path_mtu`]: super::path_mtu
#[derive(Debug, Clone)]
pub struct PayloadBuilder {
    /// The packet type governing which frames are admitted (RFC 9000 §12.4).
    packet_type: PacketType,
    /// The maximum number of payload bytes the packet may hold.
    limit: usize,
    /// The serialized frames accumulated so far.
    buf: Vec<u8>,
    /// Whether any accumulated frame is ack-eliciting (RFC 9000 §13.2.1).
    ack_eliciting: bool,
    /// Whether the packet counts as in flight (RFC 9002 §2): ack-eliciting or
    /// carrying a PADDING frame.
    in_flight: bool,
}

impl PayloadBuilder {
    /// Start an empty payload for `packet_type` that may grow to `limit` bytes.
    #[must_use]
    pub fn new(packet_type: PacketType, limit: usize) -> Self {
        Self {
            packet_type,
            limit,
            buf: Vec::new(),
            ack_eliciting: false,
            in_flight: false,
        }
    }

    /// Try to append `frame` to the payload.
    ///
    /// Returns `Ok(true)` if the frame was placed, or `Ok(false)` if it did not
    /// fit in the remaining budget (the builder is left unchanged, so the caller
    /// may finish this packet and carry the frame into the next one).
    ///
    /// # Errors
    ///
    /// [`PayloadError::FrameNotPermitted`] if the frame may not appear in this
    /// packet type (RFC 9000 §12.4), or [`PayloadError::Encode`] if it cannot be
    /// serialized.
    pub fn try_push(&mut self, frame: &Frame) -> Result<bool, PayloadError> {
        if !self.packet_type.permits(frame) {
            return Err(PayloadError::FrameNotPermitted {
                frame_type: frame.frame_type(),
                packet_type: self.packet_type,
            });
        }
        // Serialize into a scratch buffer so a frame that overflows the budget
        // leaves the payload untouched.
        let mut scratch = Vec::new();
        frame.encode(&mut scratch)?;
        if self.buf.len() + scratch.len() > self.limit {
            return Ok(false);
        }
        self.buf.extend_from_slice(&scratch);
        self.ack_eliciting |= frame.is_ack_eliciting();
        // A packet is in flight if it is ack-eliciting or carries PADDING
        // (RFC 9002 §2).
        self.in_flight |= frame.is_ack_eliciting() || matches!(frame, Frame::Padding(_));
        Ok(true)
    }

    /// Pad the payload up to `target_len` bytes with PADDING (RFC 9000 §19.1),
    /// never exceeding the builder's `limit`.
    ///
    /// Used to satisfy the 1200-byte client Initial datagram minimum (RFC 9000
    /// §14.1) and to size a Path MTU Discovery probe. Returns the number of
    /// padding bytes added. Adding any padding makes the packet count as in
    /// flight (RFC 9002 §2).
    pub fn pad_to(&mut self, target_len: usize) -> usize {
        let target = target_len.min(self.limit);
        if self.buf.len() >= target {
            return 0;
        }
        let added = target - self.buf.len();
        self.buf.resize(target, 0);
        self.in_flight = true;
        added
    }

    /// The packet type this payload is being built for.
    #[must_use]
    pub const fn packet_type(&self) -> PacketType {
        self.packet_type
    }

    /// The number of payload bytes accumulated so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether no frames have been added yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The number of payload bytes still available before the `limit`.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.buf.len())
    }

    /// Whether the assembled packet is ack-eliciting (RFC 9000 §13.2.1): it
    /// carries at least one frame other than PADDING, ACK, and CONNECTION_CLOSE,
    /// so the peer must acknowledge it.
    #[must_use]
    pub const fn is_ack_eliciting(&self) -> bool {
        self.ack_eliciting
    }

    /// Whether the assembled packet counts as in flight (RFC 9002 §2): it is
    /// ack-eliciting or carries a PADDING frame, so its bytes count against the
    /// congestion window and it is tracked for loss.
    #[must_use]
    pub const fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    /// Borrow the accumulated payload bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Consume the builder and return the accumulated payload bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::quic_frame::{self, Frame};

    const ALL_TYPES: [PacketType; 4] =
        [PacketType::Initial, PacketType::ZeroRtt, PacketType::Handshake, PacketType::OneRtt];

    fn stream_frame(data: &[u8]) -> Frame {
        Frame::Stream { stream_id: 0, offset: 0, fin: false, data: data.to_vec() }
    }

    fn ack_frame() -> Frame {
        Frame::Ack { largest_acked: 5, ack_delay: 0, first_ack_range: 0, ranges: Vec::new(), ecn: None }
    }

    #[test]
    fn number_space_mapping() {
        assert_eq!(PacketType::Initial.number_space(), PacketNumberSpace::Initial);
        assert_eq!(PacketType::Handshake.number_space(), PacketNumberSpace::Handshake);
        assert_eq!(PacketType::ZeroRtt.number_space(), PacketNumberSpace::ApplicationData);
        assert_eq!(PacketType::OneRtt.number_space(), PacketNumberSpace::ApplicationData);
    }

    #[test]
    fn padding_and_ping_are_universal() {
        for ty in ALL_TYPES {
            assert!(ty.permits(&Frame::Padding(3)), "{ty} PADDING");
            assert!(ty.permits(&Frame::Ping), "{ty} PING");
        }
    }

    #[test]
    fn ack_and_crypto_barred_from_zero_rtt() {
        let crypto = Frame::Crypto { offset: 0, data: vec![1, 2, 3] };
        for ty in [PacketType::Initial, PacketType::Handshake, PacketType::OneRtt] {
            assert!(ty.permits(&ack_frame()), "{ty} ACK");
            assert!(ty.permits(&crypto), "{ty} CRYPTO");
        }
        assert!(!PacketType::ZeroRtt.permits(&ack_frame()));
        assert!(!PacketType::ZeroRtt.permits(&crypto));
    }

    #[test]
    fn one_rtt_only_frames() {
        let one_rtt_only = [
            Frame::NewToken(vec![9]),
            Frame::PathResponse([0; 8]),
            Frame::HandshakeDone,
        ];
        for frame in &one_rtt_only {
            for ty in [PacketType::Initial, PacketType::ZeroRtt, PacketType::Handshake] {
                assert!(!ty.permits(frame), "{ty} must reject {frame:?}");
            }
            assert!(PacketType::OneRtt.permits(frame), "1-RTT must allow {frame:?}");
        }
    }

    #[test]
    fn application_frames_barred_from_initial_and_handshake() {
        let app_frames = [
            stream_frame(b"x"),
            Frame::ResetStream { stream_id: 0, app_error_code: 0, final_size: 0 },
            Frame::StopSending { stream_id: 0, app_error_code: 0 },
            Frame::MaxData(10),
            Frame::MaxStreamData { stream_id: 0, max: 10 },
            Frame::MaxStreams { bidi: true, max: 10 },
            Frame::DataBlocked(10),
            Frame::StreamDataBlocked { stream_id: 0, limit: 10 },
            Frame::StreamsBlocked { bidi: false, limit: 10 },
            Frame::RetireConnectionId(0),
            Frame::PathChallenge([0; 8]),
        ];
        for frame in &app_frames {
            assert!(!PacketType::Initial.permits(frame), "Initial must reject {frame:?}");
            assert!(!PacketType::Handshake.permits(frame), "Handshake must reject {frame:?}");
            assert!(PacketType::ZeroRtt.permits(frame), "0-RTT must allow {frame:?}");
            assert!(PacketType::OneRtt.permits(frame), "1-RTT must allow {frame:?}");
        }
    }

    #[test]
    fn path_challenge_allowed_in_zero_rtt_but_response_only_one_rtt() {
        assert!(PacketType::ZeroRtt.permits(&Frame::PathChallenge([1; 8])));
        assert!(PacketType::OneRtt.permits(&Frame::PathChallenge([1; 8])));
        assert!(!PacketType::ZeroRtt.permits(&Frame::PathResponse([1; 8])));
        assert!(PacketType::OneRtt.permits(&Frame::PathResponse([1; 8])));
    }

    #[test]
    fn connection_close_forms() {
        let transport = Frame::ConnectionClose { error_code: 0, frame_type: Some(0), reason: vec![] };
        let application = Frame::ConnectionClose { error_code: 0, frame_type: None, reason: vec![] };
        for ty in ALL_TYPES {
            assert!(ty.permits(&transport), "{ty} transport CONNECTION_CLOSE");
        }
        assert!(!PacketType::Initial.permits(&application));
        assert!(!PacketType::Handshake.permits(&application));
        assert!(PacketType::ZeroRtt.permits(&application));
        assert!(PacketType::OneRtt.permits(&application));
    }

    #[test]
    fn new_connection_id_barred_from_handshake() {
        let ncid = Frame::NewConnectionId {
            sequence_number: 1,
            retire_prior_to: 0,
            connection_id: vec![1, 2, 3, 4],
            stateless_reset_token: [0; 16],
        };
        assert!(!PacketType::Initial.permits(&ncid));
        assert!(!PacketType::Handshake.permits(&ncid));
        assert!(PacketType::OneRtt.permits(&ncid));
    }

    #[test]
    fn push_reports_not_permitted() {
        let mut b = PayloadBuilder::new(PacketType::Initial, 1200);
        let err = b.try_push(&stream_frame(b"hello")).unwrap_err();
        assert_eq!(
            err,
            PayloadError::FrameNotPermitted {
                frame_type: 0x08,
                packet_type: PacketType::Initial,
            }
        );
        // Rejected frame left the payload empty.
        assert!(b.is_empty());
    }

    #[test]
    fn push_accumulates_and_round_trips() {
        let mut b = PayloadBuilder::new(PacketType::Initial, 1200);
        let crypto = Frame::Crypto { offset: 0, data: vec![0xaa; 20] };
        assert!(b.try_push(&crypto).unwrap());
        assert!(b.try_push(&Frame::Ping).unwrap());
        assert!(b.is_ack_eliciting());
        assert!(b.is_in_flight());

        let parsed = quic_frame::parse_all(b.as_bytes()).unwrap();
        assert_eq!(parsed, vec![crypto, Frame::Ping]);
    }

    #[test]
    fn ack_only_packet_is_not_ack_eliciting_nor_in_flight() {
        let mut b = PayloadBuilder::new(PacketType::OneRtt, 1200);
        assert!(b.try_push(&ack_frame()).unwrap());
        assert!(!b.is_ack_eliciting());
        assert!(!b.is_in_flight());
    }

    #[test]
    fn padding_alone_is_in_flight_but_not_ack_eliciting() {
        let mut b = PayloadBuilder::new(PacketType::OneRtt, 1200);
        assert!(b.try_push(&Frame::Padding(10)).unwrap());
        assert!(!b.is_ack_eliciting());
        assert!(b.is_in_flight());
    }

    #[test]
    fn frame_that_does_not_fit_is_rejected_without_mutation() {
        // Budget only large enough for the PING (1 byte).
        let mut b = PayloadBuilder::new(PacketType::OneRtt, 1);
        assert!(b.try_push(&Frame::Ping).unwrap());
        assert_eq!(b.len(), 1);
        assert_eq!(b.remaining(), 0);
        // A second frame cannot fit; the builder is unchanged.
        assert!(!b.try_push(&Frame::Ping).unwrap());
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn pad_to_reaches_target_and_sets_in_flight() {
        let mut b = PayloadBuilder::new(PacketType::Initial, 1200);
        let crypto = Frame::Crypto { offset: 0, data: vec![0; 30] };
        b.try_push(&crypto).unwrap();
        let before = b.len();
        let added = b.pad_to(1200);
        assert_eq!(added, 1200 - before);
        assert_eq!(b.len(), 1200);
        assert!(b.is_in_flight());
        // The padding decodes as a single coalesced PADDING frame after CRYPTO.
        let parsed = quic_frame::parse_all(b.as_bytes()).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(matches!(parsed[1], Frame::Padding(_)));
    }

    #[test]
    fn pad_to_never_exceeds_limit() {
        let mut b = PayloadBuilder::new(PacketType::Initial, 100);
        let added = b.pad_to(1200);
        assert_eq!(added, 100);
        assert_eq!(b.len(), 100);
    }

    #[test]
    fn pad_to_below_current_length_is_a_noop() {
        let mut b = PayloadBuilder::new(PacketType::OneRtt, 1200);
        b.try_push(&Frame::Padding(50)).unwrap();
        assert_eq!(b.pad_to(10), 0);
        assert_eq!(b.len(), 50);
    }

    #[test]
    fn into_bytes_returns_payload() {
        let mut b = PayloadBuilder::new(PacketType::OneRtt, 1200);
        b.try_push(&Frame::Ping).unwrap();
        let bytes = b.into_bytes();
        assert_eq!(quic_frame::parse_all(&bytes).unwrap(), vec![Frame::Ping]);
    }
}
