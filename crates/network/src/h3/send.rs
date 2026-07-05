//! QUIC send-side frame scheduling (RFC 9000 §12.4, §13.2.1; RFC 9002 §2): the
//! first composition slice of the *send* path, the mirror of the connection-level
//! receive dispatch ([`connection`](super::connection)).
//!
//! Where [`connection::QuicConnection::process_packet`](super::connection::QuicConnection::process_packet)
//! routes each *received* frame to the state machine that owns it, this slice
//! runs the opposite direction: the frames the connection *owes* to be sent — the
//! acknowledgement a [`ack::AckGenerator`](super::ack::AckGenerator) produced, the
//! PATH_RESPONSE / RETIRE_CONNECTION_ID frames a
//! [`connection::PacketEffects`](super::connection::PacketEffects) surfaced, the
//! CRYPTO / STREAM data a send stream buffered — are queued into a
//! [`SendScheduler`] and packed, by descending priority under a byte budget, into
//! the successive packet payloads a [`packet_payload::PayloadBuilder`] represents.
//!
//! ## Priority
//!
//! RFC 9000 does not mandate an intra-packet frame order, but a sender that wants
//! low latency packs the timely control frames ahead of bulk application data.
//! [`SendPriority`] encodes the order this scheduler uses (highest first):
//!
//! 1. [`SendPriority::Close`] — a CONNECTION_CLOSE is terminal, sent ahead of
//!    everything (RFC 9000 §10.2).
//! 2. [`SendPriority::Ack`] — acknowledgements are packed first among ordinary
//!    frames so the peer learns of delivery promptly and its loss timers stay
//!    tight (RFC 9000 §13.2.1).
//! 3. [`SendPriority::Crypto`] — handshake data is sent ahead of application data
//!    so the handshake completes as fast as possible (RFC 9001 §4).
//! 4. [`SendPriority::Control`] — connection-management and flow-control frames.
//! 5. [`SendPriority::Stream`] — application data (RFC 9000 §19.8).
//! 6. [`SendPriority::Probe`] — PING / PADDING, packed last (RFC 9000 §19.1, §19.2).
//!
//! Frames of the same priority keep their enqueue order (FIFO), except that a
//! frame too large for the remaining budget is skipped so a smaller later frame of
//! the same priority can still be packed — QUIC frames within a packet carry no
//! ordering requirement (CRYPTO / STREAM order is recovered from their explicit
//! offsets, RFC 9000 §19.6, §19.8), so this reordering is safe.
//!
//! ## What this slice owns and what it defers
//!
//! Pure state: the scheduler holds queued frames and packs them into payloads. It
//! validates each frame against the packet type's permission table (RFC 9000
//! §12.4) at enqueue, so a caller cannot build a packet a peer would reject. It
//! does not generate frames (the owning state machines do), assign packet numbers,
//! encrypt ([`packet_crypt`](super::packet_crypt)), or coalesce packets into a
//! datagram ([`datagram_build`](super::datagram_build)) — those remain the send
//! engine's job in later slices. No IO, no clock.

use std::collections::{BTreeMap, VecDeque};

use super::packet_payload::{PacketType, PayloadBuilder, PayloadError};
use super::quic_frame::{Frame, QuicFrameError};

/// The priority class a frame is packed under, highest first (RFC 9000 §13.2.1,
/// RFC 9001 §4). The `Ord` derived from declaration order *is* the send order:
/// [`SendPriority::Close`] sorts first and is packed first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SendPriority {
    /// CONNECTION_CLOSE — the connection is terminating; the closing packet is
    /// packed ahead of anything else (RFC 9000 §10.2).
    Close,
    /// ACK — acknowledgements are packed first among ordinary frames so the peer
    /// learns of delivery promptly (RFC 9000 §13.2.1).
    Ack,
    /// CRYPTO — handshake progress, packed ahead of application data (RFC 9001 §4).
    Crypto,
    /// Connection-management and flow-control frames: PATH_CHALLENGE /
    /// PATH_RESPONSE, NEW_CONNECTION_ID / RETIRE_CONNECTION_ID, MAX_DATA /
    /// MAX_STREAM_DATA / MAX_STREAMS, the `*_BLOCKED` signals, RESET_STREAM /
    /// STOP_SENDING, NEW_TOKEN, and HANDSHAKE_DONE.
    Control,
    /// STREAM — application data (RFC 9000 §19.8).
    Stream,
    /// PING / PADDING — keepalive and probe padding, packed last (RFC 9000 §19.1,
    /// §19.2).
    Probe,
}

impl SendPriority {
    /// The priority class `frame` is packed under.
    #[must_use]
    pub const fn of(frame: &Frame) -> Self {
        match frame {
            Frame::ConnectionClose { .. } => Self::Close,
            Frame::Ack { .. } => Self::Ack,
            Frame::Crypto { .. } => Self::Crypto,
            Frame::Stream { .. } => Self::Stream,
            Frame::Ping | Frame::Padding(_) => Self::Probe,
            // Everything else is connection-management / flow control.
            Frame::ResetStream { .. }
            | Frame::StopSending { .. }
            | Frame::NewToken(_)
            | Frame::MaxData(_)
            | Frame::MaxStreamData { .. }
            | Frame::MaxStreams { .. }
            | Frame::DataBlocked(_)
            | Frame::StreamDataBlocked { .. }
            | Frame::StreamsBlocked { .. }
            | Frame::NewConnectionId { .. }
            | Frame::RetireConnectionId(_)
            | Frame::PathChallenge(_)
            | Frame::PathResponse(_)
            | Frame::HandshakeDone => Self::Control,
        }
    }
}

/// Something that prevented a frame from being queued or packed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// The frame may not appear in this packet type (RFC 9000 §12.4); the peer
    /// would treat such a packet as a `PROTOCOL_VIOLATION`, so the scheduler
    /// refuses to queue it.
    NotPermitted {
        /// The offending frame's wire type code (RFC 9000 §19).
        frame_type: u64,
        /// The packet type it was offered for.
        packet_type: PacketType,
    },
    /// A queued frame is larger than `limit` bytes, so it cannot fit even an empty
    /// packet — packing would make no progress. The caller must raise the budget
    /// (a larger path MTU) or drop the frame.
    FrameTooLarge {
        /// The payload byte budget the frame overflowed.
        limit: usize,
    },
    /// A frame could not be serialized (a field exceeded the varint maximum).
    Encode(QuicFrameError),
}

impl core::fmt::Display for SendError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotPermitted { frame_type, packet_type } => write!(
                f,
                "frame type {frame_type:#x} is not permitted in a {packet_type} packet"
            ),
            Self::FrameTooLarge { limit } => {
                write!(f, "a queued frame exceeds the {limit}-byte payload budget")
            }
            Self::Encode(e) => write!(f, "frame encode failed: {e}"),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(e) => Some(e),
            Self::NotPermitted { .. } | Self::FrameTooLarge { .. } => None,
        }
    }
}

impl From<PayloadError> for SendError {
    fn from(e: PayloadError) -> Self {
        match e {
            PayloadError::FrameNotPermitted { frame_type, packet_type } => {
                Self::NotPermitted { frame_type, packet_type }
            }
            PayloadError::Encode(e) => Self::Encode(e),
        }
    }
}

/// A priority queue of frames owed to be sent in one packet type, packed into
/// successive packet payloads by descending [`SendPriority`] under a byte budget
/// (RFC 9000 §12.4).
///
/// Feed it with [`SendScheduler::enqueue`] (the acknowledgement, control, and data
/// frames the connection owes), then drain it packet by packet with
/// [`SendScheduler::build_next`] while [`SendScheduler::has_pending`] holds. Pure:
/// no packet numbers, no encryption, no IO.
#[derive(Debug, Clone)]
pub struct SendScheduler {
    /// The packet type governing which frames are admitted (RFC 9000 §12.4).
    packet_type: PacketType,
    /// The queued frames, grouped by priority; a [`BTreeMap`] keeps the groups in
    /// ascending (highest-priority-first) key order for packing.
    queues: BTreeMap<SendPriority, VecDeque<Frame>>,
    /// The total number of queued frames, so [`SendScheduler::has_pending`] and
    /// [`SendScheduler::pending`] are `O(1)`.
    pending: usize,
}

impl SendScheduler {
    /// Start an empty scheduler for `packet_type`.
    #[must_use]
    pub fn new(packet_type: PacketType) -> Self {
        Self { packet_type, queues: BTreeMap::new(), pending: 0 }
    }

    /// The packet type this scheduler packs frames for.
    #[must_use]
    pub const fn packet_type(&self) -> PacketType {
        self.packet_type
    }

    /// Queue `frame` for sending under its [`SendPriority`].
    ///
    /// # Errors
    ///
    /// [`SendError::NotPermitted`] if the frame may not appear in this packet type
    /// (RFC 9000 §12.4) — validated here so a caller cannot assemble a packet the
    /// peer would reject.
    pub fn enqueue(&mut self, frame: Frame) -> Result<(), SendError> {
        if !self.packet_type.permits(&frame) {
            return Err(SendError::NotPermitted {
                frame_type: frame.frame_type(),
                packet_type: self.packet_type,
            });
        }
        let priority = SendPriority::of(&frame);
        self.queues.entry(priority).or_default().push_back(frame);
        self.pending += 1;
        Ok(())
    }

    /// Whether any frame is still queued.
    #[must_use]
    pub const fn has_pending(&self) -> bool {
        self.pending != 0
    }

    /// The number of frames still queued.
    #[must_use]
    pub const fn pending(&self) -> usize {
        self.pending
    }

    /// Pack the highest-priority queued frames that fit into one packet payload of
    /// at most `limit` bytes, removing the packed frames from the queue and leaving
    /// the rest for the next call.
    ///
    /// Returns the [`PayloadBuilder`] holding the packed frames — the caller reads
    /// [`PayloadBuilder::is_ack_eliciting`] / [`PayloadBuilder::is_in_flight`] for
    /// loss recovery, optionally pads it ([`PayloadBuilder::pad_to`]), and hands
    /// the bytes to the packet-encryption path. Call repeatedly while
    /// [`SendScheduler::has_pending`] holds to drain every frame across packets.
    ///
    /// # Errors
    ///
    /// [`SendError::FrameTooLarge`] if a frame remains queued but the produced
    /// payload is empty — the smallest queued frame does not fit even an empty
    /// packet, so packing cannot make progress. [`SendError::Encode`] if a frame
    /// cannot be serialized.
    pub fn build_next(&mut self, limit: usize) -> Result<PayloadBuilder, SendError> {
        let mut builder = PayloadBuilder::new(self.packet_type, limit);
        for queue in self.queues.values_mut() {
            let mut kept = VecDeque::with_capacity(queue.len());
            while let Some(frame) = queue.pop_front() {
                if builder.try_push(&frame)? {
                    self.pending -= 1;
                } else {
                    // Too large for the remaining budget; keep it queued and try
                    // the next frame, which may be smaller and still fit.
                    kept.push_back(frame);
                }
            }
            *queue = kept;
        }
        if builder.is_empty() && self.has_pending() {
            return Err(SendError::FrameTooLarge { limit });
        }
        Ok(builder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::quic_frame::{self, Frame};

    fn ack() -> Frame {
        Frame::Ack {
            largest_acked: 5,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: Vec::new(),
            ecn: None,
        }
    }

    fn crypto(offset: u64, len: usize) -> Frame {
        Frame::Crypto { offset, data: vec![0xAB; len] }
    }

    fn stream(data: &[u8]) -> Frame {
        Frame::Stream { stream_id: 0, offset: 0, fin: false, data: data.to_vec() }
    }

    #[test]
    fn empty_scheduler_has_nothing_pending() {
        let s = SendScheduler::new(PacketType::OneRtt);
        assert!(!s.has_pending());
        assert_eq!(s.pending(), 0);
        assert_eq!(s.packet_type(), PacketType::OneRtt);
    }

    #[test]
    fn priority_classification() {
        assert_eq!(SendPriority::of(&ack()), SendPriority::Ack);
        assert_eq!(SendPriority::of(&crypto(0, 4)), SendPriority::Crypto);
        assert_eq!(SendPriority::of(&stream(b"x")), SendPriority::Stream);
        assert_eq!(SendPriority::of(&Frame::Ping), SendPriority::Probe);
        assert_eq!(SendPriority::of(&Frame::Padding(3)), SendPriority::Probe);
        assert_eq!(SendPriority::of(&Frame::MaxData(10)), SendPriority::Control);
        assert_eq!(
            SendPriority::of(&Frame::PathResponse([0; 8])),
            SendPriority::Control
        );
        assert_eq!(
            SendPriority::of(&Frame::ConnectionClose {
                error_code: 0,
                frame_type: None,
                reason: Vec::new()
            }),
            SendPriority::Close
        );
    }

    #[test]
    fn priority_order_is_close_ack_crypto_control_stream_probe() {
        // Ord is the send order.
        assert!(SendPriority::Close < SendPriority::Ack);
        assert!(SendPriority::Ack < SendPriority::Crypto);
        assert!(SendPriority::Crypto < SendPriority::Control);
        assert!(SendPriority::Control < SendPriority::Stream);
        assert!(SendPriority::Stream < SendPriority::Probe);
    }

    #[test]
    fn enqueue_rejects_frame_not_permitted_in_packet_type() {
        // STREAM is barred from an Initial packet (RFC 9000 §12.4).
        let mut s = SendScheduler::new(PacketType::Initial);
        let err = s.enqueue(stream(b"body")).unwrap_err();
        match err {
            SendError::NotPermitted { packet_type, .. } => {
                assert_eq!(packet_type, PacketType::Initial);
            }
            other => panic!("expected NotPermitted, got {other:?}"),
        }
        assert!(!s.has_pending());
    }

    #[test]
    fn enqueue_permits_ack_and_crypto_in_initial() {
        let mut s = SendScheduler::new(PacketType::Initial);
        s.enqueue(ack()).unwrap();
        s.enqueue(crypto(0, 8)).unwrap();
        assert_eq!(s.pending(), 2);
    }

    #[test]
    fn build_next_packs_in_priority_order() {
        // Enqueue out of priority order; expect ACK, CRYPTO, STREAM in the payload.
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(stream(b"body")).unwrap();
        s.enqueue(ack()).unwrap();
        s.enqueue(crypto(0, 4)).unwrap();
        let payload = s.build_next(1_200).unwrap();
        assert!(!s.has_pending());
        let frames = quic_frame::parse_all(payload.as_bytes()).unwrap();
        assert!(matches!(frames[0], Frame::Ack { .. }), "{frames:?}");
        assert!(matches!(frames[1], Frame::Crypto { .. }), "{frames:?}");
        assert!(matches!(frames[2], Frame::Stream { .. }), "{frames:?}");
    }

    #[test]
    fn fifo_within_a_priority_class() {
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(crypto(0, 4)).unwrap();
        s.enqueue(crypto(100, 4)).unwrap();
        let payload = s.build_next(1_200).unwrap();
        let frames = quic_frame::parse_all(payload.as_bytes()).unwrap();
        let offsets: Vec<u64> = frames
            .iter()
            .filter_map(|f| match f {
                Frame::Crypto { offset, .. } => Some(*offset),
                _ => None,
            })
            .collect();
        assert_eq!(offsets, vec![0, 100]);
    }

    #[test]
    fn build_next_respects_the_byte_limit_and_carries_the_rest() {
        // A tight budget takes the ACK (small) but not the large CRYPTO.
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(ack()).unwrap();
        s.enqueue(crypto(0, 200)).unwrap();
        let first = s.build_next(20).unwrap();
        let f0 = quic_frame::parse_all(first.as_bytes()).unwrap();
        assert!(f0.iter().all(|f| matches!(f, Frame::Ack { .. })));
        assert!(s.has_pending(), "the large CRYPTO must remain queued");
        assert_eq!(s.pending(), 1);
        // A second packet with room drains the CRYPTO.
        let second = s.build_next(1_200).unwrap();
        let f1 = quic_frame::parse_all(second.as_bytes()).unwrap();
        assert!(f1.iter().any(|f| matches!(f, Frame::Crypto { .. })));
        assert!(!s.has_pending());
    }

    #[test]
    fn a_smaller_lower_frame_is_packed_when_an_earlier_one_does_not_fit() {
        // Two CRYPTO frames of the same priority: a big one first, a small one
        // after. A budget that fits only the small one skips the big one.
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(crypto(0, 300)).unwrap();
        s.enqueue(crypto(1_000, 2)).unwrap();
        let payload = s.build_next(20).unwrap();
        let frames = quic_frame::parse_all(payload.as_bytes()).unwrap();
        let offsets: Vec<u64> = frames
            .iter()
            .filter_map(|f| match f {
                Frame::Crypto { offset, .. } => Some(*offset),
                _ => None,
            })
            .collect();
        assert_eq!(offsets, vec![1_000], "only the small later CRYPTO fits");
        assert_eq!(s.pending(), 1, "the big CRYPTO stays queued");
    }

    #[test]
    fn a_frame_larger_than_an_empty_packet_is_an_error() {
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(crypto(0, 500)).unwrap();
        // Budget smaller than the single frame → no progress possible.
        let err = s.build_next(10).unwrap_err();
        assert_eq!(err, SendError::FrameTooLarge { limit: 10 });
    }

    #[test]
    fn multiple_packets_drain_every_frame() {
        let mut s = SendScheduler::new(PacketType::OneRtt);
        for i in 0u64..5 {
            s.enqueue(crypto(i * 50, 40)).unwrap();
        }
        let mut packets = 0;
        let mut total = 0;
        while s.has_pending() {
            let payload = s.build_next(60).unwrap();
            total += quic_frame::parse_all(payload.as_bytes()).unwrap().len();
            packets += 1;
            assert!(packets <= 5, "must terminate");
        }
        assert_eq!(total, 5);
    }

    #[test]
    fn ack_only_payload_is_not_ack_eliciting_but_a_stream_is() {
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(ack()).unwrap();
        let payload = s.build_next(1_200).unwrap();
        assert!(!payload.is_ack_eliciting());

        let mut s2 = SendScheduler::new(PacketType::OneRtt);
        s2.enqueue(stream(b"body")).unwrap();
        let payload2 = s2.build_next(1_200).unwrap();
        assert!(payload2.is_ack_eliciting());
    }

    #[test]
    fn connection_close_is_packed_before_an_ack() {
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(ack()).unwrap();
        s.enqueue(Frame::ConnectionClose {
            error_code: 0,
            frame_type: None,
            reason: b"bye".to_vec(),
        })
        .unwrap();
        let payload = s.build_next(1_200).unwrap();
        let frames = quic_frame::parse_all(payload.as_bytes()).unwrap();
        assert!(
            matches!(frames[0], Frame::ConnectionClose { .. }),
            "{frames:?}"
        );
    }

    #[test]
    fn empty_scheduler_builds_an_empty_payload_without_error() {
        let mut s = SendScheduler::new(PacketType::OneRtt);
        let payload = s.build_next(1_200).unwrap();
        assert!(payload.is_empty());
    }

    #[test]
    fn pending_count_tracks_packed_frames() {
        let mut s = SendScheduler::new(PacketType::OneRtt);
        s.enqueue(ack()).unwrap();
        s.enqueue(crypto(0, 4)).unwrap();
        s.enqueue(stream(b"x")).unwrap();
        assert_eq!(s.pending(), 3);
        s.build_next(1_200).unwrap();
        assert_eq!(s.pending(), 0);
    }

    #[test]
    fn not_permitted_error_maps_from_payload_error() {
        // PathResponse / HandshakeDone are 1-RTT only; barred from Handshake.
        let mut s = SendScheduler::new(PacketType::Handshake);
        assert!(matches!(
            s.enqueue(Frame::HandshakeDone),
            Err(SendError::NotPermitted { .. })
        ));
    }
}
