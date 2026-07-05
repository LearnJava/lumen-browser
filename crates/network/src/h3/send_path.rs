//! QUIC send-path datagram flush (RFC 9000 §12.2, §12.4, §14.1; RFC 9002 §2, §6):
//! the composition slice that drives the per-space [`send_engine`](super::send_engine)
//! outward — coalescing each space's encrypted packets into outgoing UDP
//! datagrams, writing them over the [`udp::DatagramTransport`](super::udp::DatagramTransport),
//! and recording every [`SentPacket`](super::loss::SentPacket) into loss recovery.
//!
//! This is the send-path counterpart of the receive-path
//! [`connection`](super::connection). Where [`connection::QuicConnection::process_packet`](super::connection::QuicConnection::process_packet)
//! decrypts one inbound datagram into packets and routes the frames inward, this
//! slice draws the outbound datagrams out: for each datagram it folds the pending
//! frames of every packet-number space (in the caller's order) through
//! [`send_engine::SpaceSender::fill_datagram`](super::send_engine::SpaceSender::fill_datagram)
//! into a single [`datagram_build::DatagramBuilder`](super::datagram_build::DatagramBuilder),
//! then hands the coalesced bytes to the transport.
//!
//! ## Coalescing order (RFC 9000 §12.2)
//!
//! A datagram may carry several QUIC packets back to back, but only a
//! length-delimited long-header packet (Initial / Handshake) may be followed by
//! another; a short-header 1-RTT packet has no length field and therefore seals the
//! datagram. The caller passes its spaces in send order — Initial, then Handshake,
//! then the Application-Data (1-RTT) space — so the long-header spaces coalesce and
//! the 1-RTT space, arriving last, seals the datagram. [`flush`] honours that order
//! and stops folding into a datagram the moment a space seals it.
//!
//! ## What this slice owns and what it defers
//!
//! [`flush`] owns the datagram loop and the loss-recovery bookkeeping: for each
//! packet it registers the [`SentPacket`](super::loss::SentPacket) into the space's
//! [`SentPacketRegistry`](super::loss::SentPacketRegistry) (so the PTO and loss
//! detection of [`pto`](super::pto) see the in-flight packet) and adds its byte
//! count to the space's [`CongestionController`](super::recovery::CongestionController)
//! `bytes_in_flight` (RFC 9002 §7). [`send_padded_initial`] is the client's
//! first-flight path: it pads the lone Initial to the
//! [`MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN) floor
//! (RFC 9000 §14.1) through
//! [`SpaceSender::build_padded_initial`](super::send_engine::SpaceSender::build_padded_initial)
//! and sends it in its own datagram.
//!
//! The module is pure apart from the transport write, which is mockable through
//! [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport): every packet
//! number, encryption, and loss record comes from the deterministic lower slices,
//! and the `now` timestamp is caller-supplied. Deciding *when* to flush (the timer
//! loop over [`event_loop`](super::event_loop)) and the inbound decrypt path are the
//! connection driver's remaining job, alongside the `h3_do_request` dispatch.

use std::io;
use std::time::Instant;

use super::datagram_build::DatagramBuilder;
use super::key_schedule::PacketProtectionKeys;
use super::loss::{SentPacket, SentPacketRegistry};
use super::packet_crypt::ProtectedHeader;
use super::recovery::CongestionController;
use super::send::SendScheduler;
use super::send_engine::{SendEngineError, SpaceSender};
use super::udp::DatagramTransport;

/// One packet-number space's complete send-side state, borrowed for the duration of
/// a [`flush`] or [`send_padded_initial`] call.
///
/// It bundles the four inputs [`SpaceSender::fill_datagram`](super::send_engine::SpaceSender::fill_datagram)
/// needs — the monotonic packet-number counter ([`sender`](Self::sender)), the
/// scheduled frames ([`scheduler`](Self::scheduler)), the packet-protection
/// [`keys`](Self::keys), and the [`header`](Self::header) describing the packet's
/// in-the-clear fields — with the two loss-recovery sinks the flush feeds: the
/// sent-packet [`registry`](Self::registry) and the [`congestion`](Self::congestion)
/// controller.
///
/// The caller holds one per active space and passes them to [`flush`] in send order
/// (Initial, Handshake, Application-Data).
#[derive(Debug)]
pub struct SpaceFlush<'a> {
    /// The send-side packet assembler owning this space's packet-number counter.
    pub sender: &'a mut SpaceSender,
    /// The scheduler holding the frames queued for this space.
    pub scheduler: &'a mut SendScheduler,
    /// The packet-protection key set (send direction) for this space.
    pub keys: &'a PacketProtectionKeys,
    /// The header describing the packet's in-the-clear fields (connection IDs,
    /// version, token) for this space.
    pub header: ProtectedHeader<'a>,
    /// The sent-packet registry loss detection and PTO read; each built packet is
    /// recorded here.
    pub registry: &'a mut SentPacketRegistry,
    /// The congestion controller whose `bytes_in_flight` each in-flight packet
    /// grows (RFC 9002 §7).
    pub congestion: &'a mut CongestionController,
}

impl SpaceFlush<'_> {
    /// Record one built packet into this space's loss-recovery sinks: add it to the
    /// [`SentPacketRegistry`](super::loss::SentPacketRegistry) and, if it counts as
    /// in flight (RFC 9002 §2), grow the [`CongestionController`](super::recovery::CongestionController)
    /// `bytes_in_flight`.
    fn record(&mut self, packet: SentPacket) {
        if packet.in_flight {
            self.congestion.on_packet_sent(packet.sent_bytes);
        }
        self.registry.on_packet_sent(packet);
    }
}

/// A summary of what one [`flush`] wrote to the transport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlushReport {
    /// The number of UDP datagrams written to the transport.
    pub datagrams_sent: usize,
    /// The total number of QUIC packets across those datagrams.
    pub packets_sent: usize,
    /// The total number of bytes written across those datagrams.
    pub bytes_sent: usize,
}

/// Something that stopped [`flush`] or [`send_padded_initial`] from writing a
/// datagram.
#[derive(Debug)]
pub enum FlushError {
    /// The send engine could not build a packet (a queued frame too large for the
    /// datagram, an encryption failure, or a header/space mismatch). Carries the
    /// underlying [`SendEngineError`].
    Engine(SendEngineError),
    /// Writing a datagram to the transport failed. Carries the underlying I/O error.
    Io(io::Error),
    /// A space still had frames pending but the datagram budget was too small to
    /// hold even one packet — the per-packet header/AEAD overhead alone exceeds
    /// `max_datagram_len`, so the flush could make no progress. Continuing would
    /// spin forever, so this is surfaced rather than looping or silently dropping
    /// the pending frames.
    DatagramTooSmall {
        /// The datagram byte budget the flush was given.
        max_datagram_len: usize,
    },
}

impl core::fmt::Display for FlushError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Engine(e) => write!(f, "QUIC send path: {e}"),
            Self::Io(e) => write!(f, "QUIC send path: datagram write failed: {e}"),
            Self::DatagramTooSmall { max_datagram_len } => write!(
                f,
                "QUIC send path: datagram budget {max_datagram_len} too small to hold any packet"
            ),
        }
    }
}

impl std::error::Error for FlushError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Engine(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::DatagramTooSmall { .. } => None,
        }
    }
}

impl From<SendEngineError> for FlushError {
    fn from(e: SendEngineError) -> Self {
        Self::Engine(e)
    }
}

impl From<io::Error> for FlushError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Flush every space's pending frames onto `transport` as coalesced datagrams,
/// recording each packet into loss recovery.
///
/// Each iteration builds one datagram of at most `max_datagram_len` bytes by folding
/// the spaces in the order given through
/// [`SpaceSender::fill_datagram`](super::send_engine::SpaceSender::fill_datagram)
/// into a shared [`DatagramBuilder`](super::datagram_build::DatagramBuilder): the
/// long-header spaces (Initial / Handshake) coalesce, and the first short-header
/// (1-RTT) packet seals the datagram (RFC 9000 §12.2), so any spaces after a sealing
/// space are skipped for that datagram. The datagram is written to the transport and
/// the loop repeats while any space still has pending frames, so a space larger than
/// one datagram drains across successive datagrams.
///
/// Pass the spaces in send order — Initial, Handshake, then Application-Data — so the
/// coalescing rule above places at most one 1-RTT packet, last, per datagram.
///
/// # Errors
///
/// [`FlushError::Engine`] if the send engine cannot build a packet;
/// [`FlushError::Io`] if a datagram write fails; [`FlushError::DatagramTooSmall`] if
/// a space has pending frames but `max_datagram_len` is too small to hold any packet.
pub fn flush<T: DatagramTransport>(
    transport: &mut T,
    spaces: &mut [SpaceFlush<'_>],
    max_datagram_len: usize,
    now: Instant,
) -> Result<FlushReport, FlushError> {
    let mut report = FlushReport::default();

    while spaces.iter().any(|s| s.scheduler.has_pending()) {
        let mut builder = DatagramBuilder::new(max_datagram_len);

        for space in spaces.iter_mut() {
            if builder.is_sealed() {
                // A short-header packet already sealed this datagram (RFC 9000
                // §12.2); nothing more may coalesce onto it.
                break;
            }
            if !space.scheduler.has_pending() {
                continue;
            }
            let largest_acked = space.registry.largest_acked();
            let sent = space.sender.fill_datagram(
                &mut builder,
                space.scheduler,
                space.keys,
                &space.header,
                largest_acked,
                now,
            )?;
            for packet in sent {
                space.record(packet);
                report.packets_sent += 1;
            }
        }

        if builder.is_empty() {
            // Some space reports pending frames (the `while` guard held) yet no
            // packet was placed: the datagram budget is smaller than the per-packet
            // overhead, so the flush cannot progress.
            return Err(FlushError::DatagramTooSmall { max_datagram_len });
        }

        let bytes = builder.into_bytes();
        report.bytes_sent += bytes.len();
        transport.send(&bytes)?;
        report.datagrams_sent += 1;
    }

    Ok(report)
}

/// Send the client's first-flight Initial as its own datagram, padded to the
/// `min_datagram_len` floor (RFC 9000 §14.1), and record it into loss recovery.
///
/// A client's Initial-bearing datagram must be at least
/// [`MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN) bytes so
/// the server's anti-amplification budget (RFC 9000 §8.1) is large enough to complete
/// the handshake. Because PADDING lives inside a packet's payload before encryption,
/// this delegates to
/// [`SpaceSender::build_padded_initial`](super::send_engine::SpaceSender::build_padded_initial),
/// which pads the single Initial packet, then writes the lone datagram and records
/// the packet.
///
/// Returns `Ok(None)` if `space` had nothing queued (no empty Initial is sent). Pass
/// [`MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN) for
/// `min_datagram_len` in the ordinary case. `space.header` must be a
/// [`ProtectedHeader::Initial`](super::packet_crypt::ProtectedHeader::Initial).
///
/// # Errors
///
/// [`FlushError::Engine`] if the header is not an Initial or the padded Initial
/// cannot be built; [`FlushError::Io`] if the datagram write fails.
pub fn send_padded_initial<T: DatagramTransport>(
    transport: &mut T,
    space: &mut SpaceFlush<'_>,
    min_datagram_len: usize,
    now: Instant,
) -> Result<Option<SentPacket>, FlushError> {
    let largest_acked = space.registry.largest_acked();
    let built = space.sender.build_padded_initial(
        space.scheduler,
        space.keys,
        &space.header,
        largest_acked,
        now,
        min_datagram_len,
    )?;
    let Some(built) = built else {
        return Ok(None);
    };
    // A first-flight Initial travels alone in its own datagram.
    transport.send(&built.bytes)?;
    let sent = built.sent;
    space.record(sent);
    Ok(Some(sent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::datagram::MIN_INITIAL_DATAGRAM_LEN;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::loss::PacketNumberSpace;
    use crate::h3::packet_crypt::decrypt_packet;
    use crate::h3::packet_payload::PacketType;
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    /// A fixed instant for the `time_sent` stamp; the module never reads the clock.
    fn now() -> Instant {
        Instant::now()
    }

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn transport() -> MockDatagramTransport {
        MockDatagramTransport::new(loopback(1), loopback(2))
    }

    /// A transport whose `send` always fails, to prove [`flush`] propagates a real
    /// socket write failure rather than swallowing it.
    #[derive(Debug, Default)]
    struct FailingSendTransport {
        /// Datagrams whose `send` was attempted (and failed), for the count.
        attempts: usize,
    }

    impl DatagramTransport for FailingSendTransport {
        fn send(&mut self, _datagram: &[u8]) -> io::Result<()> {
            self.attempts += 1;
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "peer reset"))
        }
        fn recv(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        }
        fn set_read_timeout(&mut self, _timeout: Option<std::time::Duration>) -> io::Result<()> {
            Ok(())
        }
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok(loopback(1))
        }
        fn peer_addr(&self) -> io::Result<SocketAddr> {
            Ok(loopback(2))
        }
    }

    /// The RFC 9001 Appendix A client Destination Connection ID.
    fn dcid() -> Vec<u8> {
        vec![0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08]
    }

    fn keys() -> InitialKeys {
        InitialKeys::derive(&dcid())
    }

    fn crypto(offset: u64, len: usize) -> Frame {
        Frame::Crypto { offset, data: vec![0xAB; len] }
    }

    /// A fully-populated [`SpaceFlush`] owner: the borrowed pieces live in the test's
    /// stack, so this holds the owners and lends a [`SpaceFlush`] on demand.
    struct SpaceOwner {
        packet_type: PacketType,
        sender: SpaceSender,
        scheduler: SendScheduler,
        keys: PacketProtectionKeys,
        registry: SentPacketRegistry,
        congestion: CongestionController,
        dcid: Vec<u8>,
    }

    impl SpaceOwner {
        fn new(packet_type: PacketType, space: PacketNumberSpace) -> Self {
            Self {
                packet_type,
                sender: SpaceSender::new(packet_type),
                scheduler: SendScheduler::new(packet_type),
                keys: keys().client,
                registry: SentPacketRegistry::new(space),
                congestion: CongestionController::new(1200),
                dcid: dcid(),
            }
        }

        fn initial() -> Self {
            Self::new(PacketType::Initial, PacketNumberSpace::Initial)
        }

        fn handshake() -> Self {
            Self::new(PacketType::Handshake, PacketNumberSpace::Handshake)
        }

        fn one_rtt() -> Self {
            Self::new(PacketType::OneRtt, PacketNumberSpace::ApplicationData)
        }

        fn flush(&mut self) -> SpaceFlush<'_> {
            // Build the header inline from disjoint field borrows: `header` borrows
            // only `self.dcid` (shared) while the other fields are borrowed mutably,
            // which the borrow checker accepts as long as they stay distinct fields.
            let header = match self.packet_type {
                PacketType::Initial => ProtectedHeader::Initial {
                    version: 1,
                    dcid: &self.dcid,
                    scid: &[],
                    token: &[],
                },
                PacketType::Handshake => ProtectedHeader::Handshake {
                    version: 1,
                    dcid: &self.dcid,
                    scid: &[],
                },
                PacketType::OneRtt => ProtectedHeader::Short {
                    spin: false,
                    key_phase: false,
                    dcid: &self.dcid,
                },
                PacketType::ZeroRtt => unreachable!("tests do not exercise 0-RTT"),
            };
            SpaceFlush {
                header,
                sender: &mut self.sender,
                scheduler: &mut self.scheduler,
                keys: &self.keys,
                registry: &mut self.registry,
                congestion: &mut self.congestion,
            }
        }
    }

    // ---- flush: the happy path ------------------------------------------

    #[test]
    fn flush_with_nothing_pending_writes_no_datagram() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        let report = flush(&mut tx, &mut [owner.flush()], 1200, now()).unwrap();
        assert_eq!(report, FlushReport::default());
        assert!(tx.sent.is_empty());
    }

    #[test]
    fn flush_sends_one_datagram_for_one_space() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 32)).unwrap();

        let report = flush(&mut tx, &mut [owner.flush()], 1200, now()).unwrap();
        assert_eq!(report.datagrams_sent, 1);
        assert_eq!(report.packets_sent, 1);
        assert_eq!(report.bytes_sent, tx.sent[0].len());
        assert_eq!(tx.sent.len(), 1);
        // Draining the scheduler leaves nothing pending.
        assert!(!owner.scheduler.has_pending());
    }

    #[test]
    fn flush_records_in_flight_packet_into_loss_recovery() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 32)).unwrap();

        flush(&mut tx, &mut [owner.flush()], 1200, now()).unwrap();
        // A CRYPTO packet is ack-eliciting and in flight (RFC 9002 §2).
        assert_eq!(owner.registry.outstanding(), 1);
        assert!(owner.registry.ack_eliciting_in_flight());
        assert_eq!(owner.congestion.bytes_in_flight(), tx.sent[0].len());
    }

    #[test]
    fn flushed_datagram_round_trips_through_decrypt() {
        let mut tx = transport();
        let ks = keys();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 32)).unwrap();

        flush(&mut tx, &mut [owner.flush()], 1200, now()).unwrap();
        // The server opens the coalesced datagram's first packet with the same keys.
        let got = decrypt_packet(&ks.client, &tx.sent[0], 0, 0).expect("decrypt");
        assert_eq!(got.packet_number, 0);
        let frames = quic_frame::parse_all(&got.payload).expect("parse frames");
        assert!(matches!(frames[0], Frame::Crypto { offset: 0, .. }), "{frames:?}");
    }

    // ---- flush: coalescing ----------------------------------------------

    #[test]
    fn flush_coalesces_initial_and_handshake_into_one_datagram() {
        let mut tx = transport();
        let mut initial = SpaceOwner::initial();
        let mut handshake = SpaceOwner::handshake();
        initial.scheduler.enqueue(crypto(0, 16)).unwrap();
        handshake.scheduler.enqueue(crypto(0, 16)).unwrap();

        let report = flush(
            &mut tx,
            &mut [initial.flush(), handshake.flush()],
            1200,
            now(),
        )
        .unwrap();
        // Both long-header packets coalesce into a single datagram.
        assert_eq!(report.datagrams_sent, 1);
        assert_eq!(report.packets_sent, 2);
        assert_eq!(tx.sent.len(), 1);
        assert_eq!(initial.registry.outstanding(), 1);
        assert_eq!(handshake.registry.outstanding(), 1);
    }

    #[test]
    fn short_header_space_seals_the_datagram() {
        // With the 1-RTT space first, its short-header packet seals the datagram, so
        // the Handshake space after it must go in a second datagram.
        let mut tx = transport();
        let mut one_rtt = SpaceOwner::one_rtt();
        let mut handshake = SpaceOwner::handshake();
        one_rtt.scheduler.enqueue(crypto(0, 16)).unwrap();
        handshake.scheduler.enqueue(crypto(0, 16)).unwrap();

        let report = flush(
            &mut tx,
            &mut [one_rtt.flush(), handshake.flush()],
            1200,
            now(),
        )
        .unwrap();
        assert_eq!(report.datagrams_sent, 2, "1-RTT seals; handshake needs its own");
        assert_eq!(report.packets_sent, 2);
    }

    #[test]
    fn flush_drains_a_large_space_across_multiple_datagrams() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        // Three CRYPTO frames whose sum far exceeds one small datagram.
        owner.scheduler.enqueue(crypto(0, 200)).unwrap();
        owner.scheduler.enqueue(crypto(200, 200)).unwrap();
        owner.scheduler.enqueue(crypto(400, 200)).unwrap();

        let report = flush(&mut tx, &mut [owner.flush()], 300, now()).unwrap();
        assert!(
            report.datagrams_sent >= 2,
            "a 300-byte budget cannot hold all three frames: {report:?}"
        );
        assert_eq!(report.datagrams_sent, tx.sent.len());
        assert!(!owner.scheduler.has_pending(), "everything drained");
        // Every datagram respects the budget.
        for dg in &tx.sent {
            assert!(dg.len() <= 300, "datagram {} bytes exceeds budget", dg.len());
        }
    }

    // ---- flush: error paths ---------------------------------------------

    #[test]
    fn flush_rejects_a_datagram_budget_too_small_for_any_packet() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 4)).unwrap();

        // A budget below the per-packet header + AEAD overhead cannot hold a packet.
        let err = flush(&mut tx, &mut [owner.flush()], 8, now()).unwrap_err();
        match err {
            FlushError::DatagramTooSmall { max_datagram_len } => {
                assert_eq!(max_datagram_len, 8);
            }
            other => panic!("expected DatagramTooSmall, got {other:?}"),
        }
        assert!(tx.sent.is_empty());
    }

    #[test]
    fn flush_propagates_a_frame_too_large_for_the_datagram() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        // A frame larger than the whole datagram can never be packed.
        owner.scheduler.enqueue(crypto(0, 2000)).unwrap();

        let err = flush(&mut tx, &mut [owner.flush()], 300, now()).unwrap_err();
        assert!(matches!(err, FlushError::Engine(_)), "{err:?}");
    }

    #[test]
    fn flush_propagates_a_transport_write_failure() {
        let mut tx = FailingSendTransport::default();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 32)).unwrap();

        let err = flush(&mut tx, &mut [owner.flush()], 1200, now()).unwrap_err();
        match err {
            FlushError::Io(e) => assert_eq!(e.kind(), io::ErrorKind::ConnectionReset),
            other => panic!("expected Io, got {other:?}"),
        }
        assert_eq!(tx.attempts, 1, "the datagram write was attempted once");
    }

    // ---- send_padded_initial --------------------------------------------

    #[test]
    fn send_padded_initial_pads_the_lone_datagram_to_the_floor() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 16)).unwrap();

        let sent = send_padded_initial(&mut tx, &mut owner.flush(), MIN_INITIAL_DATAGRAM_LEN, now())
            .unwrap()
            .expect("a padded Initial");
        assert_eq!(sent.packet_number, 0);
        assert_eq!(tx.sent.len(), 1);
        assert!(
            tx.sent[0].len() >= MIN_INITIAL_DATAGRAM_LEN,
            "padded datagram is {} bytes, below the {MIN_INITIAL_DATAGRAM_LEN} floor",
            tx.sent[0].len()
        );
        // The padded Initial counts as in flight and is recorded.
        assert_eq!(owner.registry.outstanding(), 1);
        assert_eq!(owner.congestion.bytes_in_flight(), tx.sent[0].len());
    }

    #[test]
    fn send_padded_initial_with_nothing_pending_sends_nothing() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        let out =
            send_padded_initial(&mut tx, &mut owner.flush(), MIN_INITIAL_DATAGRAM_LEN, now()).unwrap();
        assert!(out.is_none());
        assert!(tx.sent.is_empty());
        assert_eq!(owner.registry.outstanding(), 0);
    }

    #[test]
    fn send_padded_initial_rejects_a_non_initial_header() {
        let mut tx = transport();
        let mut owner = SpaceOwner::handshake();
        owner.scheduler.enqueue(crypto(0, 16)).unwrap();

        let err =
            send_padded_initial(&mut tx, &mut owner.flush(), MIN_INITIAL_DATAGRAM_LEN, now()).unwrap_err();
        assert!(matches!(err, FlushError::Engine(_)), "{err:?}");
    }

    #[test]
    fn send_padded_initial_round_trips_through_decrypt() {
        let mut tx = transport();
        let ks = keys();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 16)).unwrap();

        send_padded_initial(&mut tx, &mut owner.flush(), MIN_INITIAL_DATAGRAM_LEN, now()).unwrap();
        let got = decrypt_packet(&ks.client, &tx.sent[0], 0, 0).expect("decrypt");
        let frames = quic_frame::parse_all(&got.payload).expect("parse frames");
        // The CRYPTO frame survives alongside the PADDING that reached the floor.
        assert!(
            frames.iter().any(|f| matches!(f, Frame::Crypto { offset: 0, .. })),
            "{frames:?}"
        );
    }

    // ---- report accounting ----------------------------------------------

    #[test]
    fn report_bytes_sum_matches_written_datagrams() {
        let mut tx = transport();
        let mut owner = SpaceOwner::initial();
        owner.scheduler.enqueue(crypto(0, 200)).unwrap();
        owner.scheduler.enqueue(crypto(200, 200)).unwrap();

        let report = flush(&mut tx, &mut [owner.flush()], 300, now()).unwrap();
        let total: usize = tx.sent.iter().map(Vec::len).sum();
        assert_eq!(report.bytes_sent, total);
        assert_eq!(report.datagrams_sent, tx.sent.len());
    }
}
