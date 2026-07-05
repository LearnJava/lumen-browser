//! QUIC send engine (RFC 9000 §12.2, §14.1, §17.1; RFC 9002 §2): the pure
//! composition slice that turns the frames a [`SendScheduler`](super::send::SendScheduler)
//! queued into on-wire, encrypted, coalesced UDP datagrams and the
//! [`SentPacket`](super::loss::SentPacket) records that feed loss recovery.
//!
//! This is the send-path counterpart of the receive-path
//! [`connection`](super::connection): where the connection layer decrypts one
//! datagram into packets and routes the frames inward, the send engine drives the
//! frames outward — assigning each packet its packet number, encrypting it through
//! [`packet_crypt`](super::packet_crypt), and coalescing the results into one
//! datagram through [`datagram_build`](super::datagram_build).
//!
//! ## What this slice owns
//!
//! A [`SpaceSender`] owns exactly one packet-number space's monotonic packet-number
//! counter (RFC 9000 §12.3): 0 is the first number a QUIC endpoint uses in a space,
//! and every packet the space sends increments it. Given the frames a
//! [`SendScheduler`](super::send::SendScheduler) has packed, a [`PacketProtectionKeys`]
//! key set, and the [`ProtectedHeader`] describing the packet's in-the-clear fields,
//! it produces:
//!
//! - [`SpaceSender::build_packet`] — one encrypted packet plus its [`BuiltPacket`]
//!   record, the primitive the two higher-level entry points build on.
//! - [`SpaceSender::fill_datagram`] — drains every pending frame into a
//!   [`DatagramBuilder`](super::datagram_build::DatagramBuilder), coalescing as many
//!   packets as the datagram budget allows (RFC 9000 §12.2). Long-header packets
//!   (Initial / Handshake) coalesce; a short-header 1-RTT packet seals the datagram,
//!   so at most one is produced per datagram.
//! - [`SpaceSender::build_padded_initial`] — the client's first-flight Initial,
//!   padded so the datagram reaches the [`datagram::MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN) floor
//!   (RFC 9000 §14.1) that the anti-amplification limit and the handshake require.
//!
//! Each [`BuiltPacket`] carries the [`SentPacket`](super::loss::SentPacket) the
//! caller hands to the space's [`SentPacketRegistry`](super::loss::SentPacketRegistry)
//! and [`CongestionController`](super::recovery::CongestionController): its number,
//! whether it is ack-eliciting and in flight (RFC 9002 §2), and its byte size.
//!
//! ## What it defers
//!
//! Pure state only: no socket, no clock of its own (the caller supplies the
//! `now` timestamp stamped on each [`SentPacket`](super::loss::SentPacket), as the
//! other clock-driven slices do). It does not decide *which* space to flush or
//! *when* — that policy, and the socket write over [`event_loop`](super::event_loop),
//! is the connection driver's job in a later slice, alongside the `h3_do_request`
//! dispatch that routes an HTTP request onto a live QUIC connection.

use std::time::Instant;

use super::datagram_build::{DatagramBuildError, DatagramBuilder};
use super::key_schedule::PacketProtectionKeys;
use super::loss::SentPacket;
use super::packet_crypt::{self, PacketCryptError, ProtectedHeader};
use super::packet_payload::PacketType;
use super::send::{SendError, SendScheduler};
use super::varint;

/// The AEAD authentication tag every protected QUIC packet carries (RFC 9001
/// §5.3); re-exported from [`packet_protect`](super::packet_protect) for the
/// overhead computation.
const AEAD_TAG_LEN: usize = super::packet_protect::AEAD_TAG_LEN;

/// The widest packet number a QUIC packet may carry (RFC 9000 §17.1): 1..=4 bytes.
const MAX_PACKET_NUMBER_LEN: usize = 4;

/// A safe upper bound on the QUIC long-header `Length` field's varint width for a
/// datagram-sized packet. A single datagram never exceeds a jumbo path MTU, so its
/// `Length` value fits well inside the 4-byte varint range (`< 2^30`); 4 bytes is a
/// conservative bound used only to size [`max_packet_overhead`].
const MAX_LENGTH_VARINT_LEN: usize = 4;

/// The upper bound, in bytes, on everything a [`ProtectedHeader`] packet adds
/// around its frame payload: the header fields, the packet number, and the AEAD
/// tag (RFC 9000 §17, RFC 9001 §5.3).
///
/// This is a true upper bound (it assumes the widest 4-byte packet number and a
/// worst-case `Length` varint), so `max_packet_overhead(header) + payload.len()` is
/// always at least the encrypted packet's length. [`SpaceSender::fill_datagram`]
/// budgets each packet's payload against it so that the encrypted packet is
/// guaranteed to fit the datagram's remaining space and coalescing never overflows.
#[must_use]
pub fn max_packet_overhead(header: &ProtectedHeader<'_>) -> usize {
    let header_fields = match header {
        ProtectedHeader::Initial { dcid, scid, token, .. } => {
            // first byte + version + (len-prefixed DCID) + (len-prefixed SCID)
            // + (varint-len-prefixed Token) + Length varint.
            1 + 4
                + 1
                + dcid.len()
                + 1
                + scid.len()
                + varint::encoded_len(token.len() as u64).unwrap_or(8)
                + token.len()
                + MAX_LENGTH_VARINT_LEN
        }
        ProtectedHeader::ZeroRtt { dcid, scid, .. }
        | ProtectedHeader::Handshake { dcid, scid, .. } => {
            // first byte + version + (len-prefixed DCID) + (len-prefixed SCID)
            // + Length varint. No Token.
            1 + 4 + 1 + dcid.len() + 1 + scid.len() + MAX_LENGTH_VARINT_LEN
        }
        // The short header has no version, no length prefix on its DCID, and no
        // Length field (RFC 9000 §17.3.1).
        ProtectedHeader::Short { dcid, .. } => 1 + dcid.len(),
    };
    header_fields + MAX_PACKET_NUMBER_LEN + AEAD_TAG_LEN
}

/// The packet type a [`ProtectedHeader`] describes.
fn header_packet_type(header: &ProtectedHeader<'_>) -> PacketType {
    match header {
        ProtectedHeader::Initial { .. } => PacketType::Initial,
        ProtectedHeader::ZeroRtt { .. } => PacketType::ZeroRtt,
        ProtectedHeader::Handshake { .. } => PacketType::Handshake,
        ProtectedHeader::Short { .. } => PacketType::OneRtt,
    }
}

/// Whether a [`ProtectedHeader`] is a long-header (length-delimited) form, so
/// another packet may be coalesced after it (RFC 9000 §12.2).
const fn is_length_delimited(header: &ProtectedHeader<'_>) -> bool {
    !matches!(header, ProtectedHeader::Short { .. })
}

/// One encrypted, on-wire QUIC packet together with the loss-recovery record and
/// the coalescing metadata the caller needs to place it in a datagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltPacket {
    /// The complete encrypted packet bytes (header-protected, AEAD-sealed), ready
    /// to coalesce into a datagram or send on its own.
    pub bytes: Vec<u8>,
    /// The record to hand to the space's
    /// [`SentPacketRegistry`](super::loss::SentPacketRegistry) and
    /// [`CongestionController`](super::recovery::CongestionController).
    pub sent: SentPacket,
    /// Whether the packet is a long-header (length-delimited) form, so another
    /// packet may follow it in the same datagram (RFC 9000 §12.2).
    pub length_delimited: bool,
    /// Whether the packet is an Initial, for the datagram's
    /// [`initial_padding_shortfall`](super::datagram_build::DatagramBuilder::initial_padding_shortfall)
    /// accounting (RFC 9000 §14.1).
    pub is_initial: bool,
}

/// Something that prevented the send engine from producing a packet or datagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendEngineError {
    /// The scheduler could not pack the queued frames (a frame too large for the
    /// budget, or an encode failure). Carries the underlying [`SendError`].
    Schedule(SendError),
    /// Encrypting the packet failed. Carries the underlying [`PacketCryptError`].
    Crypt(PacketCryptError),
    /// Coalescing the encrypted packet into the datagram failed. Carries the
    /// underlying [`DatagramBuildError`].
    Datagram(DatagramBuildError),
    /// The [`ProtectedHeader`] describes a different packet type than this
    /// [`SpaceSender`] (or its scheduler) serves, so the encrypted packet would land
    /// in the wrong packet-number space.
    HeaderMismatch {
        /// The packet type this sender serves.
        expected: PacketType,
        /// The packet type the supplied header describes.
        found: PacketType,
    },
}

impl core::fmt::Display for SendEngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Schedule(e) => write!(f, "QUIC send engine: {e}"),
            Self::Crypt(e) => write!(f, "QUIC send engine: {e}"),
            Self::Datagram(e) => write!(f, "QUIC send engine: {e}"),
            Self::HeaderMismatch { expected, found } => write!(
                f,
                "QUIC send engine: header describes a {found} packet but the sender serves {expected}"
            ),
        }
    }
}

impl std::error::Error for SendEngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Schedule(e) => Some(e),
            Self::Crypt(e) => Some(e),
            Self::Datagram(e) => Some(e),
            Self::HeaderMismatch { .. } => None,
        }
    }
}

impl From<SendError> for SendEngineError {
    fn from(e: SendError) -> Self {
        Self::Schedule(e)
    }
}

impl From<PacketCryptError> for SendEngineError {
    fn from(e: PacketCryptError) -> Self {
        Self::Crypt(e)
    }
}

impl From<DatagramBuildError> for SendEngineError {
    fn from(e: DatagramBuildError) -> Self {
        Self::Datagram(e)
    }
}

/// The send-side assembler for one packet-number space (RFC 9000 §12.3): it owns
/// the monotonic packet-number counter and turns scheduled frames into encrypted
/// packets.
///
/// The caller holds one [`SpaceSender`] per active space (Initial, Handshake, and
/// the shared Application-Data space used by 0-RTT / 1-RTT), pairing each with its
/// [`SendScheduler`](super::send::SendScheduler), [`PacketProtectionKeys`], and
/// [`ProtectedHeader`]. Pure: no clock, no socket.
#[derive(Clone, Debug)]
pub struct SpaceSender {
    /// The packet type (and thereby the packet-number space) this sender serves.
    packet_type: PacketType,
    /// The next packet number to assign in this space; starts at 0 (RFC 9000
    /// §12.3) and increments on every packet built.
    next_packet_number: u64,
}

impl SpaceSender {
    /// Start a sender for `packet_type` with the first packet number (0).
    #[must_use]
    pub const fn new(packet_type: PacketType) -> Self {
        Self { packet_type, next_packet_number: 0 }
    }

    /// The packet type this sender serves.
    #[must_use]
    pub const fn packet_type(&self) -> PacketType {
        self.packet_type
    }

    /// The packet number the next built packet will carry.
    #[must_use]
    pub const fn next_packet_number(&self) -> u64 {
        self.next_packet_number
    }

    /// Check that `header` matches the packet type this sender serves.
    fn check_header(&self, header: &ProtectedHeader<'_>) -> Result<(), SendEngineError> {
        let found = header_packet_type(header);
        if found == self.packet_type {
            Ok(())
        } else {
            Err(SendEngineError::HeaderMismatch { expected: self.packet_type, found })
        }
    }

    /// Pack the highest-priority pending frames into one packet payload of at most
    /// `payload_limit` bytes, assign the next packet number, and encrypt the packet.
    ///
    /// Returns `Ok(None)` when the scheduler produced an empty payload (nothing was
    /// pending, or nothing fit under the padding-free path), leaving the
    /// packet-number counter untouched; otherwise `Ok(Some(_))` with the encrypted
    /// [`BuiltPacket`] and the counter advanced.
    ///
    /// `largest_acked` (the space's [`SentPacketRegistry::largest_acked`](super::loss::SentPacketRegistry::largest_acked))
    /// chooses the packet-number width (RFC 9000 §17.1); `now` is stamped on the
    /// [`SentPacket`](super::loss::SentPacket).
    ///
    /// # Errors
    ///
    /// [`SendEngineError::HeaderMismatch`] if `header` is for a different packet
    /// type; [`SendEngineError::Schedule`] if a queued frame cannot be packed under
    /// `payload_limit`; [`SendEngineError::Crypt`] if encryption fails.
    pub fn build_packet(
        &mut self,
        scheduler: &mut SendScheduler,
        keys: &PacketProtectionKeys,
        header: &ProtectedHeader<'_>,
        largest_acked: Option<u64>,
        payload_limit: usize,
        now: Instant,
    ) -> Result<Option<BuiltPacket>, SendEngineError> {
        self.check_header(header)?;
        let payload = scheduler.build_next(payload_limit)?;
        if payload.is_empty() {
            return Ok(None);
        }
        let pn = self.next_packet_number;
        let bytes = packet_crypt::encrypt_packet(keys, header, pn, largest_acked, payload.as_bytes())?;
        self.next_packet_number += 1;
        Ok(Some(BuiltPacket {
            length_delimited: is_length_delimited(header),
            is_initial: matches!(header, ProtectedHeader::Initial { .. }),
            sent: SentPacket {
                packet_number: pn,
                time_sent: now,
                ack_eliciting: payload.is_ack_eliciting(),
                in_flight: payload.is_in_flight(),
                sent_bytes: bytes.len(),
            },
            bytes,
        }))
    }

    /// Drain every pending frame into `datagram`, coalescing as many packets as its
    /// remaining budget allows (RFC 9000 §12.2), and return the
    /// [`SentPacket`](super::loss::SentPacket) records in send order.
    ///
    /// Each packet's payload is budgeted against [`max_packet_overhead`] so the
    /// encrypted packet is guaranteed to fit the datagram, so coalescing never
    /// overflows. Long-header (Initial / Handshake) packets coalesce until the
    /// frames are drained or the datagram has no room; a short-header 1-RTT packet
    /// seals the datagram (RFC 9000 §12.2), so `fill_datagram` produces at most one
    /// per call for the Application-Data space and leaves the rest for the next
    /// datagram.
    ///
    /// When the datagram runs out of room before the frames are drained, the
    /// remaining frames stay queued in `scheduler` for the caller's next datagram.
    ///
    /// # Errors
    ///
    /// [`SendEngineError::HeaderMismatch`] if `header` is for a different packet
    /// type; [`SendEngineError::Schedule`] if a single queued frame is too large for
    /// the datagram's remaining budget; [`SendEngineError::Crypt`] if encryption
    /// fails; [`SendEngineError::Datagram`] if coalescing fails.
    pub fn fill_datagram(
        &mut self,
        datagram: &mut DatagramBuilder,
        scheduler: &mut SendScheduler,
        keys: &PacketProtectionKeys,
        header: &ProtectedHeader<'_>,
        largest_acked: Option<u64>,
        now: Instant,
    ) -> Result<Vec<SentPacket>, SendEngineError> {
        self.check_header(header)?;
        let overhead = max_packet_overhead(header);
        let length_delimited = is_length_delimited(header);
        let is_initial = matches!(header, ProtectedHeader::Initial { .. });
        let mut sent = Vec::new();

        while scheduler.has_pending() && !datagram.is_sealed() {
            let room = datagram.remaining();
            if room <= overhead {
                // No room for even a minimal further packet; leave the rest queued.
                break;
            }
            let payload_limit = room - overhead;
            let payload = match scheduler.build_next(payload_limit) {
                Ok(payload) => payload,
                // The smallest queued frame does not fit this datagram's shrinking
                // remainder: if the datagram already carries a packet, leave the
                // frame for the caller's next (full-size) datagram. If the datagram
                // is still empty, the frame is larger than a whole datagram, which
                // is a genuine error the caller must resolve.
                Err(e @ SendError::FrameTooLarge { .. }) => {
                    if datagram.is_empty() {
                        return Err(e.into());
                    }
                    break;
                }
                Err(e) => return Err(e.into()),
            };
            if payload.is_empty() {
                break;
            }
            let pn = self.next_packet_number;
            let bytes =
                packet_crypt::encrypt_packet(keys, header, pn, largest_acked, payload.as_bytes())?;
            // The overhead bound guarantees `bytes.len() <= overhead + payload_limit
            // == room`, so the packet fits and `push_encrypted` returns `true`.
            let pushed = datagram.push_encrypted(&bytes, length_delimited, is_initial)?;
            debug_assert!(pushed, "max_packet_overhead must bound the encrypted packet size");
            if !pushed {
                break;
            }
            sent.push(SentPacket {
                packet_number: pn,
                time_sent: now,
                ack_eliciting: payload.is_ack_eliciting(),
                in_flight: payload.is_in_flight(),
                sent_bytes: bytes.len(),
            });
            self.next_packet_number += 1;
        }
        Ok(sent)
    }

    /// Build the client's first-flight Initial packet, padding its payload so the
    /// standalone datagram reaches `min_datagram_len` (RFC 9000 §14.1).
    ///
    /// A client's Initial-bearing datagram must be at least
    /// [`MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN) bytes so the server's amplification budget
    /// (RFC 9000 §8.1) is large enough to complete the handshake. Because PADDING
    /// (RFC 9000 §19.1) must live inside a packet's payload before encryption, this
    /// method pads the single Initial packet's payload until the encrypted packet —
    /// the whole datagram, since a first-flight Initial travels alone — meets the
    /// floor. Padding makes the packet count as in flight (RFC 9002 §2).
    ///
    /// Returns `Ok(None)` if the scheduler had nothing to send (no empty Initial is
    /// produced). Pass [`MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN) for `min_datagram_len` in the
    /// ordinary case.
    ///
    /// # Errors
    ///
    /// [`SendEngineError::HeaderMismatch`] if `header` is not an Initial;
    /// [`SendEngineError::Schedule`] if the queued frames cannot be packed;
    /// [`SendEngineError::Crypt`] if encryption fails.
    pub fn build_padded_initial(
        &mut self,
        scheduler: &mut SendScheduler,
        keys: &PacketProtectionKeys,
        header: &ProtectedHeader<'_>,
        largest_acked: Option<u64>,
        now: Instant,
        min_datagram_len: usize,
    ) -> Result<Option<BuiltPacket>, SendEngineError> {
        if !matches!(header, ProtectedHeader::Initial { .. }) {
            return Err(SendEngineError::HeaderMismatch {
                expected: PacketType::Initial,
                found: header_packet_type(header),
            });
        }
        self.check_header(header)?;
        // Give the payload builder room up to the whole datagram so it can absorb
        // the PADDING needed to reach the floor.
        let mut payload = scheduler.build_next(min_datagram_len)?;
        if payload.is_empty() {
            return Ok(None);
        }
        let pn = self.next_packet_number;
        let mut bytes = packet_crypt::encrypt_packet(keys, header, pn, largest_acked, payload.as_bytes())?;
        // Pad the payload until the encrypted packet reaches the datagram floor.
        // Each round adds the byte deficit to the payload; the packet grows one-for-
        // one except for a possible one-byte `Length` varint bump, so this settles
        // in at most two rounds. The guard bounds it defensively.
        for _ in 0..4 {
            if bytes.len() >= min_datagram_len {
                break;
            }
            let deficit = min_datagram_len - bytes.len();
            let added = payload.pad_to(payload.len() + deficit);
            if added == 0 {
                // The payload builder's limit caps further padding; stop.
                break;
            }
            bytes = packet_crypt::encrypt_packet(keys, header, pn, largest_acked, payload.as_bytes())?;
        }
        self.next_packet_number += 1;
        Ok(Some(BuiltPacket {
            length_delimited: true,
            is_initial: true,
            sent: SentPacket {
                packet_number: pn,
                time_sent: now,
                ack_eliciting: payload.is_ack_eliciting(),
                in_flight: payload.is_in_flight(),
                sent_bytes: bytes.len(),
            },
            bytes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::datagram::MIN_INITIAL_DATAGRAM_LEN;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::packet_crypt::decrypt_packet;
    use crate::h3::quic_frame::{self, Frame};

    /// A fixed instant for the `time_sent` stamp; the module never reads the clock.
    fn now() -> Instant {
        Instant::now()
    }

    /// Decode a hex string (ignoring whitespace) into bytes.
    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    /// The RFC 9001 Appendix A client Destination Connection ID.
    fn dcid() -> Vec<u8> {
        hex("8394c8f03e515708")
    }

    /// Both directions' Initial keys derived from that DCID.
    fn keys() -> InitialKeys {
        InitialKeys::derive(&dcid())
    }

    fn crypto(offset: u64, len: usize) -> Frame {
        Frame::Crypto { offset, data: vec![0xAB; len] }
    }

    fn ack() -> Frame {
        Frame::Ack {
            largest_acked: 3,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: Vec::new(),
            ecn: None,
        }
    }

    #[test]
    fn new_sender_starts_at_packet_number_zero() {
        let s = SpaceSender::new(PacketType::Initial);
        assert_eq!(s.next_packet_number(), 0);
        assert_eq!(s.packet_type(), PacketType::Initial);
    }

    #[test]
    fn build_packet_assigns_and_advances_the_packet_number() {
        let ks = keys();
        let d = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &[], token: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        sched.enqueue(crypto(0, 16)).unwrap();

        let p0 = sender.build_packet(&mut sched, &ks.client, &header, None, 1200, now())
            .unwrap()
            .expect("a packet");
        assert_eq!(p0.sent.packet_number, 0);
        assert_eq!(sender.next_packet_number(), 1);
        assert!(p0.length_delimited);
        assert!(p0.is_initial);
        assert_eq!(p0.sent.sent_bytes, p0.bytes.len());

        sched.enqueue(crypto(16, 16)).unwrap();
        let p1 = sender.build_packet(&mut sched, &ks.client, &header, None, 1200, now())
            .unwrap()
            .expect("a packet");
        assert_eq!(p1.sent.packet_number, 1);
        assert_eq!(sender.next_packet_number(), 2);
    }

    #[test]
    fn build_packet_round_trips_through_decrypt() {
        let ks = keys();
        let d = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &[], token: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        sched.enqueue(crypto(0, 32)).unwrap();

        let packet = sender.build_packet(&mut sched, &ks.client, &header, None, 1200, now())
            .unwrap()
            .expect("a packet");
        // The server opens the client's Initial with the same derived key set.
        let got = decrypt_packet(&ks.client, &packet.bytes, 0, 0).expect("decrypt");
        assert_eq!(got.packet_number, 0);
        let frames = quic_frame::parse_all(&got.payload).expect("parse frames");
        assert!(matches!(frames[0], Frame::Crypto { offset: 0, .. }), "{frames:?}");
    }

    #[test]
    fn build_packet_returns_none_when_nothing_pending() {
        let ks = keys();
        let d = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &[], token: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        let none = sender.build_packet(&mut sched, &ks.client, &header, None, 1200, now()).unwrap();
        assert!(none.is_none());
        // The counter must not advance for an empty payload.
        assert_eq!(sender.next_packet_number(), 0);
    }

    #[test]
    fn ack_only_packet_is_not_ack_eliciting_or_in_flight() {
        let ks = keys();
        let d = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &[], token: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        sched.enqueue(ack()).unwrap();
        let p = sender.build_packet(&mut sched, &ks.client, &header, None, 1200, now())
            .unwrap()
            .expect("a packet");
        assert!(!p.sent.ack_eliciting);
        assert!(!p.sent.in_flight);
    }

    #[test]
    fn crypto_packet_is_ack_eliciting_and_in_flight() {
        let ks = keys();
        let d = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &[], token: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        sched.enqueue(crypto(0, 8)).unwrap();
        let p = sender.build_packet(&mut sched, &ks.client, &header, None, 1200, now())
            .unwrap()
            .expect("a packet");
        assert!(p.sent.ack_eliciting);
        assert!(p.sent.in_flight);
    }

    #[test]
    fn header_mismatch_is_rejected() {
        let ks = keys();
        let d = dcid();
        // Handshake header offered to an Initial-space sender.
        let header = ProtectedHeader::Handshake { version: 1, dcid: &d, scid: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Handshake);
        sched.enqueue(crypto(0, 8)).unwrap();
        let err = sender
            .build_packet(&mut sched, &ks.client, &header, None, 1200, now())
            .unwrap_err();
        assert_eq!(
            err,
            SendEngineError::HeaderMismatch {
                expected: PacketType::Initial,
                found: PacketType::Handshake,
            }
        );
    }

    #[test]
    fn max_packet_overhead_bounds_the_encrypted_packet() {
        let ks = keys();
        let d = dcid();
        let scid = hex("c2c3c4c5");
        let token = hex("0102030405");
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &scid, token: &token };
        let overhead = max_packet_overhead(&header);
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        let payload_len = 40;
        sched.enqueue(crypto(0, payload_len)).unwrap();
        // The frame packs to a payload slightly larger than its data; measure the
        // encrypted packet against overhead + the actual payload length.
        let payload_bytes = {
            let mut s2 = SendScheduler::new(PacketType::Initial);
            s2.enqueue(crypto(0, payload_len)).unwrap();
            s2.build_next(1200).unwrap().as_bytes().len()
        };
        let p = sender.build_packet(&mut sched, &ks.client, &header, None, 1200, now())
            .unwrap()
            .expect("a packet");
        assert!(
            p.bytes.len() <= overhead + payload_bytes,
            "encrypted {} must be <= overhead {overhead} + payload {payload_bytes}",
            p.bytes.len()
        );
    }

    #[test]
    fn fill_datagram_coalesces_multiple_handshake_packets() {
        let ks = keys();
        let d = dcid();
        let scid = hex("aabbccdd");
        let header = ProtectedHeader::Handshake { version: 1, dcid: &d, scid: &scid };
        let mut sender = SpaceSender::new(PacketType::Handshake);
        let mut sched = SendScheduler::new(PacketType::Handshake);
        // Several CRYPTO frames, each too large to share a tiny datagram with the
        // next, so coalescing produces more than one packet.
        for i in 0u64..4 {
            sched.enqueue(crypto(i * 30, 30)).unwrap();
        }
        let mut datagram = DatagramBuilder::new(1200);
        let sent = sender
            .fill_datagram(&mut datagram, &mut sched, &ks.client, &header, None, now())
            .expect("fill");
        assert!(!sched.has_pending(), "everything was drained");
        assert!(!sent.is_empty());
        assert_eq!(sender.next_packet_number(), sent.len() as u64);
        assert!(datagram.len() <= datagram.max_len());
        // The coalesced datagram decodes back into that many packets.
        assert!(!datagram.is_empty());
    }

    #[test]
    fn fill_datagram_stops_at_one_short_header_packet() {
        let ks = keys();
        let d = hex("1122334455667788");
        let header = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &d };
        let mut sender = SpaceSender::new(PacketType::OneRtt);
        let mut sched = SendScheduler::new(PacketType::OneRtt);
        // Two STREAM frames large enough that a small datagram holds only one; the
        // short-header packet seals the datagram, so exactly one packet is built.
        sched
            .enqueue(Frame::Stream { stream_id: 0, offset: 0, fin: true, data: vec![0x11; 60] })
            .unwrap();
        sched
            .enqueue(Frame::Stream { stream_id: 4, offset: 0, fin: true, data: vec![0x22; 60] })
            .unwrap();
        let mut datagram = DatagramBuilder::new(100);
        let sent = sender
            .fill_datagram(&mut datagram, &mut sched, &ks.server, &header, None, now())
            .expect("fill");
        // A short-header packet seals the datagram, so exactly one is built.
        assert_eq!(sent.len(), 1);
        assert!(datagram.is_sealed());
        assert!(sched.has_pending(), "the rest waits for the next datagram");
    }

    #[test]
    fn fill_datagram_leaves_frames_for_the_next_datagram_when_full() {
        let ks = keys();
        let d = dcid();
        let scid = hex("55667788");
        let header = ProtectedHeader::Handshake { version: 1, dcid: &d, scid: &scid };
        let mut sender = SpaceSender::new(PacketType::Handshake);
        let mut sched = SendScheduler::new(PacketType::Handshake);
        for i in 0u64..6 {
            sched.enqueue(crypto(i * 50, 50)).unwrap();
        }
        // A small datagram cannot hold all six frames.
        let mut datagram = DatagramBuilder::new(200);
        let sent = sender
            .fill_datagram(&mut datagram, &mut sched, &ks.client, &header, None, now())
            .expect("fill");
        assert!(!sent.is_empty());
        assert!(sched.has_pending(), "some frames must remain queued");
        assert!(datagram.len() <= 200);
    }

    #[test]
    fn build_padded_initial_reaches_the_datagram_floor() {
        let ks = keys();
        let d = dcid();
        let scid = hex("c295a3b1");
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &scid, token: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        sched.enqueue(crypto(0, 40)).unwrap();
        let p = sender
            .build_padded_initial(&mut sched, &ks.client, &header, None, now(), MIN_INITIAL_DATAGRAM_LEN)
            .unwrap()
            .expect("a packet");
        assert!(
            p.bytes.len() >= MIN_INITIAL_DATAGRAM_LEN,
            "padded Initial datagram must reach the {MIN_INITIAL_DATAGRAM_LEN}-byte floor, got {}",
            p.bytes.len()
        );
        assert!(p.sent.in_flight, "a padded packet is in flight");
        // It still decrypts and the CRYPTO frame survives amid the PADDING.
        let got = decrypt_packet(&ks.client, &p.bytes, 0, 0).expect("decrypt");
        let frames = quic_frame::parse_all(&got.payload).expect("parse");
        assert!(frames.iter().any(|f| matches!(f, Frame::Crypto { .. })), "{frames:?}");
    }

    #[test]
    fn build_padded_initial_none_when_empty() {
        let ks = keys();
        let d = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &d, scid: &[], token: &[] };
        let mut sender = SpaceSender::new(PacketType::Initial);
        let mut sched = SendScheduler::new(PacketType::Initial);
        let none = sender
            .build_padded_initial(&mut sched, &ks.client, &header, None, now(), MIN_INITIAL_DATAGRAM_LEN)
            .unwrap();
        assert!(none.is_none());
        assert_eq!(sender.next_packet_number(), 0);
    }

    #[test]
    fn build_padded_initial_rejects_a_non_initial_header() {
        let ks = keys();
        let d = dcid();
        let scid = hex("00112233");
        let header = ProtectedHeader::Handshake { version: 1, dcid: &d, scid: &scid };
        let mut sender = SpaceSender::new(PacketType::Handshake);
        let mut sched = SendScheduler::new(PacketType::Handshake);
        sched.enqueue(crypto(0, 8)).unwrap();
        let err = sender
            .build_padded_initial(&mut sched, &ks.client, &header, None, now(), MIN_INITIAL_DATAGRAM_LEN)
            .unwrap_err();
        assert!(matches!(err, SendEngineError::HeaderMismatch { .. }));
    }

    #[test]
    fn fill_datagram_records_are_registrable_for_loss() {
        use crate::h3::loss::{PacketNumberSpace, SentPacketRegistry};
        let ks = keys();
        let d = dcid();
        let scid = hex("deadbeef");
        let header = ProtectedHeader::Handshake { version: 1, dcid: &d, scid: &scid };
        let mut sender = SpaceSender::new(PacketType::Handshake);
        let mut sched = SendScheduler::new(PacketType::Handshake);
        sched.enqueue(crypto(0, 20)).unwrap();
        sched.enqueue(crypto(20, 20)).unwrap();
        let mut datagram = DatagramBuilder::new(1200);
        let sent = sender
            .fill_datagram(&mut datagram, &mut sched, &ks.client, &header, None, now())
            .expect("fill");
        // The records feed the sent-packet registry unchanged.
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::Handshake);
        for s in &sent {
            reg.on_packet_sent(*s);
        }
        assert_eq!(reg.outstanding(), sent.len());
        assert!(reg.ack_eliciting_in_flight());
    }

    #[test]
    fn error_display_and_source_are_wired() {
        let e = SendEngineError::HeaderMismatch {
            expected: PacketType::Initial,
            found: PacketType::Handshake,
        };
        assert!(!format!("{e}").is_empty());
        // A wrapped schedule error exposes its source.
        let e2 = SendEngineError::from(SendError::FrameTooLarge { limit: 4 });
        use std::error::Error;
        assert!(e2.source().is_some());
    }
}
