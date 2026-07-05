//! QUIC send-side connection state (RFC 9000 §12.3, §14.1; RFC 9002 §7;
//! h3::send_state): the composition slice that owns everything the send path needs
//! that the receive-side connection driver ([`driver`](super::driver)) does not —
//! the per-space packet-number senders and frame schedulers, the send-direction
//! packet-protection keys installed as the handshake derives each encryption level,
//! and the per-space congestion controllers — and folds them through
//! [`send_path::flush`](super::send_path::flush) into outgoing datagrams.
//!
//! ## Where this slice sits
//!
//! [`driver::ConnectionDriver`](super::driver::ConnectionDriver) owns the *receive*
//! half: the event-loop socket wait, the [`connection::QuicConnection`](super::connection::QuicConnection)
//! receiver state, the [`pto::LossDetection`](super::pto::LossDetection) (which owns
//! the send-side sent-packet registries and the PTO timer), the unified timer
//! scheduler, and the receive keys. Its documentation is explicit that "assembling
//! and writing the outgoing datagrams is the caller's job": the driver reports what
//! must be sent as [`driver::DriverAction`](super::driver::DriverAction)s and leaves
//! the send state to a separate owner. This slice is that owner.
//!
//! [`ConnectionSendState`] holds one [`SpaceSendState`] per installed
//! packet-number space (the [`send_engine::SpaceSender`](super::send_engine::SpaceSender)
//! packet-number counter, the [`send::SendScheduler`](super::send::SendScheduler)
//! frame queue, the [`key_schedule::PacketProtectionKeys`](super::key_schedule::PacketProtectionKeys),
//! and the space's [`recovery::CongestionController`](super::recovery::CongestionController)),
//! plus the header fields shared by every packet it builds (the QUIC version, the
//! Destination and Source Connection IDs, and the Initial address-validation token).
//!
//! ## What one flush does
//!
//! [`ConnectionSendState::flush`] borrows the per-space sent-packet registries from
//! the driver's [`pto::LossDetection`](super::pto::LossDetection)
//! ([`pto::LossDetection::registries_mut`](super::pto::LossDetection::registries_mut)),
//! pairs each installed space with its registry and freshly-built header, and hands
//! the array to [`send_path::flush`](super::send_path::flush) in send order —
//! Initial, then Handshake, then Application Data — so the long-header spaces
//! coalesce and the short-header 1-RTT space seals the datagram (RFC 9000 §12.2).
//! [`ConnectionSendState::send_padded_initial`] is the client's first-flight path:
//! it pads the lone Initial to the
//! [`datagram::MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN)
//! floor (RFC 9000 §14.1) and sends it in its own datagram.
//!
//! Congestion is tracked per space here, matching the per-space
//! [`SpaceFlush`](super::send_path::SpaceFlush) seam: each space's controller grows
//! its own `bytes_in_flight` and a discarded space's in-flight bytes fall away with
//! its controller (RFC 9001 §4.9). Unifying the three controllers into one
//! connection-wide (per-path) controller (RFC 9002 §B.2) is deferred to a later
//! slice; it would change the [`send_path`](super::send_path) seam, not this owner's
//! interface.
//!
//! The module is pure apart from the transport write, which is mockable through
//! [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport): every packet
//! number, encryption, and loss record comes from the deterministic lower slices,
//! and the `now` timestamp is caller-supplied.

use std::time::Instant;

use super::datagram::MIN_INITIAL_DATAGRAM_LEN;
use super::key_schedule::PacketProtectionKeys;
use super::loss::{PacketNumberSpace, SentPacket, SentPacketRegistry};
use super::packet_crypt::ProtectedHeader;
use super::packet_payload::PacketType;
use super::pto::LossDetection;
use super::quic_frame::Frame;
use super::recovery::CongestionController;
use super::send::{SendError, SendScheduler};
use super::send_engine::SpaceSender;
use super::send_path::{self, FlushError, FlushReport, SpaceFlush};
use super::udp::DatagramTransport;

/// The packet type (and thereby the long/short header form) a packet-number space's
/// packets carry (RFC 9000 §17): Initial → Initial, Handshake → Handshake, and
/// Application Data → the short-header 1-RTT form.
fn packet_type_of(space: PacketNumberSpace) -> PacketType {
    match space {
        PacketNumberSpace::Initial => PacketType::Initial,
        PacketNumberSpace::Handshake => PacketType::Handshake,
        PacketNumberSpace::ApplicationData => PacketType::OneRtt,
    }
}

/// Something that stopped a frame from being queued into a space's send scheduler.
#[derive(Debug)]
pub enum SendStateError {
    /// The target space has no send state installed yet — its packet-protection keys
    /// have not been derived, so no packet can be built for it. A frame cannot be
    /// queued for a space the handshake has not reached.
    SpaceNotInstalled(PacketNumberSpace),
    /// The scheduler refused the frame (it is not permitted in this packet type, it
    /// overflows the payload budget, or it could not be serialized). Carries the
    /// underlying [`SendError`].
    Schedule(SendError),
}

impl core::fmt::Display for SendStateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SpaceNotInstalled(space) => {
                write!(f, "QUIC send state: {space:?} space has no send keys installed yet")
            }
            Self::Schedule(e) => write!(f, "QUIC send state: {e}"),
        }
    }
}

impl std::error::Error for SendStateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SpaceNotInstalled(_) => None,
            Self::Schedule(e) => Some(e),
        }
    }
}

impl From<SendError> for SendStateError {
    fn from(e: SendError) -> Self {
        Self::Schedule(e)
    }
}

/// One packet-number space's complete send-side state: the packet-number counter,
/// the frame scheduler, the send-direction packet-protection keys, and the space's
/// congestion controller.
#[derive(Debug)]
struct SpaceSendState {
    /// The send-side packet assembler owning this space's packet-number counter
    /// (RFC 9000 §12.3).
    sender: SpaceSender,
    /// The priority queue of frames owed in this space's packets (RFC 9000 §12.4).
    scheduler: SendScheduler,
    /// The send-direction packet-protection keys for this space (RFC 9001 §5.3),
    /// replaced when the handshake performs a key update.
    keys: PacketProtectionKeys,
    /// The congestion controller whose `bytes_in_flight` this space's in-flight
    /// packets grow (RFC 9002 §7).
    congestion: CongestionController,
}

/// The send half of one QUIC connection: the per-space send state installed so far
/// and the header fields every outgoing packet shares.
///
/// A space is absent until [`ConnectionSendState::install`] derives its keys, and
/// falls away again on [`ConnectionSendState::discard`] (RFC 9001 §4.9). Frames are
/// queued per space with [`ConnectionSendState::enqueue`] and drained onto a
/// transport with [`ConnectionSendState::flush`] (or, for the client's first flight,
/// [`ConnectionSendState::send_padded_initial`]), which borrows the matching
/// per-space sent-packet registries from the driver's [`pto::LossDetection`](super::pto::LossDetection).
#[derive(Debug)]
pub struct ConnectionSendState {
    /// The Initial space send state, present once Initial keys are installed.
    initial: Option<SpaceSendState>,
    /// The Handshake space send state, present once Handshake keys are installed.
    handshake: Option<SpaceSendState>,
    /// The Application Data (1-RTT) space send state, present once 1-RTT keys are
    /// installed.
    app_data: Option<SpaceSendState>,
    /// The maximum datagram size a fresh [`CongestionController`] is sized for
    /// (RFC 9002 §7.2 `max_datagram_size`); the initial window scales from it.
    max_datagram_size: usize,
    /// The QUIC version stamped into every long-header packet (RFC 9000 §17.2).
    version: u32,
    /// The Destination Connection ID — the peer's chosen ID we address packets to
    /// (RFC 9000 §5.1).
    dcid: Vec<u8>,
    /// The Source Connection ID — the ID this endpoint issued, carried in every
    /// long-header packet (RFC 9000 §17.2).
    scid: Vec<u8>,
    /// The address-validation Token echoed in Initial packets (RFC 9000 §8.1.2),
    /// empty until a Retry or NEW_TOKEN supplies one.
    token: Vec<u8>,
}

impl ConnectionSendState {
    /// Creates an empty send state — no space installed yet — for a connection using
    /// `version`, addressing the peer's `dcid`, sending under our `scid`, and sizing
    /// fresh congestion controllers for `max_datagram_size` (RFC 9002 §7.2).
    ///
    /// The Initial address-validation token starts empty; a Retry sets it with
    /// [`ConnectionSendState::set_token`].
    pub fn new(version: u32, dcid: Vec<u8>, scid: Vec<u8>, max_datagram_size: usize) -> Self {
        Self {
            initial: None,
            handshake: None,
            app_data: None,
            max_datagram_size,
            version,
            dcid,
            scid,
            token: Vec::new(),
        }
    }

    /// The QUIC version stamped into long-header packets.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The Destination Connection ID packets are addressed to.
    pub fn dcid(&self) -> &[u8] {
        &self.dcid
    }

    /// The Source Connection ID carried in long-header packets.
    pub fn scid(&self) -> &[u8] {
        &self.scid
    }

    /// The Initial address-validation Token (empty when none).
    pub fn token(&self) -> &[u8] {
        &self.token
    }

    /// Sets the Initial address-validation Token echoed in Initial packets — the
    /// server-supplied Token from a Retry (RFC 9000 §8.1.2) or a saved NEW_TOKEN.
    pub fn set_token(&mut self, token: Vec<u8>) {
        self.token = token;
    }

    /// Replaces the Destination Connection ID, e.g. after a Retry or a server's chosen
    /// Source Connection ID is learned from its first Initial (RFC 9000 §7.2).
    pub fn set_dcid(&mut self, dcid: Vec<u8>) {
        self.dcid = dcid;
    }

    /// Installs (or, on a key update, replaces the keys of) the send state for
    /// `space` with `keys`.
    ///
    /// A first install creates the space's packet-number sender and frame scheduler
    /// (of the packet type the space uses) and a fresh congestion controller. A
    /// later install for an already-present space replaces only the keys, preserving
    /// the packet-number counter, any queued frames, and the congestion state — the
    /// behaviour a 1-RTT key update needs (RFC 9001 §6).
    pub fn install(&mut self, space: PacketNumberSpace, keys: PacketProtectionKeys) {
        let max_datagram_size = self.max_datagram_size;
        let slot = self.slot_mut(space);
        match slot {
            Some(existing) => existing.keys = keys,
            None => {
                let packet_type = packet_type_of(space);
                *slot = Some(SpaceSendState {
                    sender: SpaceSender::new(packet_type),
                    scheduler: SendScheduler::new(packet_type),
                    keys,
                    congestion: CongestionController::new(max_datagram_size),
                });
            }
        }
    }

    /// Drops a space's send state (RFC 9001 §4.9): QUIC discards the Initial space
    /// once Handshake keys exist and the Handshake space once the handshake is
    /// confirmed. Its queued frames, packet-number counter, and in-flight congestion
    /// bytes fall away with it; the caller pairs this with
    /// [`pto::LossDetection::discard_space`](super::pto::LossDetection::discard_space).
    pub fn discard(&mut self, space: PacketNumberSpace) {
        *self.slot_mut(space) = None;
    }

    /// Whether `space` has send state installed (its keys have been derived).
    pub fn is_installed(&self, space: PacketNumberSpace) -> bool {
        self.slot(space).is_some()
    }

    /// Queues `frame` for the next packet in `space`.
    ///
    /// # Errors
    ///
    /// [`SendStateError::SpaceNotInstalled`] if the space has no keys installed;
    /// [`SendStateError::Schedule`] if the scheduler refuses the frame (not permitted
    /// in this packet type, over the payload budget, or unserializable).
    pub fn enqueue(&mut self, space: PacketNumberSpace, frame: Frame) -> Result<(), SendStateError> {
        let state = self
            .slot_mut(space)
            .as_mut()
            .ok_or(SendStateError::SpaceNotInstalled(space))?;
        state.scheduler.enqueue(frame)?;
        Ok(())
    }

    /// Whether any installed space has a frame queued to send.
    pub fn has_pending(&self) -> bool {
        [&self.initial, &self.handshake, &self.app_data]
            .into_iter()
            .flatten()
            .any(|s| s.scheduler.has_pending())
    }

    /// Whether `space` is installed and has a frame queued to send.
    pub fn pending_in(&self, space: PacketNumberSpace) -> bool {
        self.slot(space).is_some_and(|s| s.scheduler.has_pending())
    }

    /// The congestion controller for `space`, if the space is installed.
    pub fn congestion(&self, space: PacketNumberSpace) -> Option<&CongestionController> {
        self.slot(space).map(|s| &s.congestion)
    }

    /// The congestion controller for `space`, borrowed mutably (e.g. to fold in an
    /// ack or a declared loss), if the space is installed.
    pub fn congestion_mut(&mut self, space: PacketNumberSpace) -> Option<&mut CongestionController> {
        self.slot_mut(space).as_mut().map(|s| &mut s.congestion)
    }

    /// The next packet number `space`'s next built packet will carry, if installed.
    pub fn next_packet_number(&self, space: PacketNumberSpace) -> Option<u64> {
        self.slot(space).map(|s| s.sender.next_packet_number())
    }

    /// Flushes every installed space's queued frames onto `transport` as coalesced
    /// datagrams, recording each sent packet into `loss` and its space's congestion
    /// controller.
    ///
    /// The spaces are folded in send order — Initial, Handshake, Application Data —
    /// so the long-header packets coalesce and the first short-header (1-RTT) packet
    /// seals the datagram (RFC 9000 §12.2). Each space's sent-packet registry is
    /// borrowed from `loss` ([`pto::LossDetection::registries_mut`](super::pto::LossDetection::registries_mut)).
    ///
    /// # Errors
    ///
    /// [`FlushError`] from [`send_path::flush`](super::send_path::flush): the send
    /// engine could not build a packet, a datagram write failed, or `max_datagram_len`
    /// is too small to hold any packet.
    pub fn flush<T: DatagramTransport>(
        &mut self,
        loss: &mut LossDetection,
        transport: &mut T,
        max_datagram_len: usize,
        now: Instant,
    ) -> Result<FlushReport, FlushError> {
        let Self { initial, handshake, app_data, version, dcid, scid, token, .. } = self;
        let version = *version;
        let [reg_initial, reg_handshake, reg_app] = loss.registries_mut();

        let mut spaces: Vec<SpaceFlush<'_>> = Vec::new();
        push_flush(
            &mut spaces,
            initial.as_mut(),
            reg_initial,
            ProtectedHeader::Initial { version, dcid, scid, token },
        );
        push_flush(
            &mut spaces,
            handshake.as_mut(),
            reg_handshake,
            ProtectedHeader::Handshake { version, dcid, scid },
        );
        push_flush(
            &mut spaces,
            app_data.as_mut(),
            reg_app,
            ProtectedHeader::Short { spin: false, key_phase: false, dcid },
        );

        send_path::flush(transport, &mut spaces, max_datagram_len, now)
    }

    /// Sends the client's first-flight Initial as its own datagram, padded to the
    /// [`MIN_INITIAL_DATAGRAM_LEN`] floor (RFC 9000 §14.1), and records it into
    /// `loss` and the Initial congestion controller.
    ///
    /// Returns `Ok(None)` when the Initial space is not installed or has nothing
    /// queued (no empty Initial is sent).
    ///
    /// # Errors
    ///
    /// [`FlushError`] from [`send_path::send_padded_initial`](super::send_path::send_padded_initial):
    /// the padded Initial could not be built or the datagram write failed.
    pub fn send_padded_initial<T: DatagramTransport>(
        &mut self,
        loss: &mut LossDetection,
        transport: &mut T,
        now: Instant,
    ) -> Result<Option<SentPacket>, FlushError> {
        let Some(state) = self.initial.as_mut() else {
            return Ok(None);
        };
        let mut space = SpaceFlush {
            header: ProtectedHeader::Initial {
                version: self.version,
                dcid: &self.dcid,
                scid: &self.scid,
                token: &self.token,
            },
            sender: &mut state.sender,
            scheduler: &mut state.scheduler,
            keys: &state.keys,
            registry: loss.registry_mut(PacketNumberSpace::Initial),
            congestion: &mut state.congestion,
        };
        send_path::send_padded_initial(transport, &mut space, MIN_INITIAL_DATAGRAM_LEN, now)
    }

    /// The immutable send-state slot for `space`.
    fn slot(&self, space: PacketNumberSpace) -> Option<&SpaceSendState> {
        match space {
            PacketNumberSpace::Initial => self.initial.as_ref(),
            PacketNumberSpace::Handshake => self.handshake.as_ref(),
            PacketNumberSpace::ApplicationData => self.app_data.as_ref(),
        }
    }

    /// The mutable send-state slot for `space`.
    fn slot_mut(&mut self, space: PacketNumberSpace) -> &mut Option<SpaceSendState> {
        match space {
            PacketNumberSpace::Initial => &mut self.initial,
            PacketNumberSpace::Handshake => &mut self.handshake,
            PacketNumberSpace::ApplicationData => &mut self.app_data,
        }
    }
}

/// Push a [`SpaceFlush`] for an installed space, pairing its send state with the
/// registry and header the caller built.
///
/// When `slot` is `None` the space is not installed and nothing is pushed; `registry`
/// and `header` are consumed regardless, which is what lets the caller move all three
/// per-space registries out of the array [`pto::LossDetection::registries_mut`](super::pto::LossDetection::registries_mut)
/// returns without tripping an unused-binding warning for absent spaces.
fn push_flush<'a>(
    spaces: &mut Vec<SpaceFlush<'a>>,
    slot: Option<&'a mut SpaceSendState>,
    registry: &'a mut SentPacketRegistry,
    header: ProtectedHeader<'a>,
) {
    if let Some(state) = slot {
        spaces.push(SpaceFlush {
            header,
            sender: &mut state.sender,
            scheduler: &mut state.scheduler,
            keys: &state.keys,
            registry,
            congestion: &mut state.congestion,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::packet_crypt::decrypt_packet;
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::Duration;

    /// A fixed instant for the `time_sent` stamp; the module reads no clock of its own.
    fn now() -> Instant {
        Instant::now()
    }

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn transport() -> MockDatagramTransport {
        MockDatagramTransport::new(loopback(1), loopback(2))
    }

    /// The RFC 9001 Appendix A client Destination Connection ID.
    fn dcid() -> Vec<u8> {
        vec![0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08]
    }

    fn client_keys() -> PacketProtectionKeys {
        InitialKeys::derive(&dcid()).client
    }

    /// A send state with the given spaces installed under the Initial keys (the test
    /// only checks framing and packet-number/registry routing, so reusing the Initial
    /// keys across spaces is harmless — every space decrypts with the same keys).
    fn send_state(spaces: &[PacketNumberSpace]) -> ConnectionSendState {
        let mut state = ConnectionSendState::new(1, dcid(), vec![0x11, 0x22, 0x33, 0x44], 1200);
        for &space in spaces {
            state.install(space, client_keys());
        }
        state
    }

    fn loss() -> LossDetection {
        LossDetection::new(Duration::from_millis(25))
    }

    fn crypto(offset: u64, len: usize) -> Frame {
        Frame::Crypto { offset, data: vec![0xAB; len] }
    }

    // ---- install / discard ----------------------------------------------

    #[test]
    fn a_fresh_state_has_no_space_installed() {
        let state = send_state(&[]);
        assert!(!state.is_installed(PacketNumberSpace::Initial));
        assert!(!state.is_installed(PacketNumberSpace::Handshake));
        assert!(!state.is_installed(PacketNumberSpace::ApplicationData));
        assert!(!state.has_pending());
    }

    #[test]
    fn install_makes_a_space_present_and_counters_start_at_zero() {
        let state = send_state(&[PacketNumberSpace::Initial]);
        assert!(state.is_installed(PacketNumberSpace::Initial));
        assert_eq!(state.next_packet_number(PacketNumberSpace::Initial), Some(0));
        assert!(state.congestion(PacketNumberSpace::Initial).is_some());
    }

    #[test]
    fn a_key_update_reinstall_keeps_the_packet_number_counter() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 16))
            .unwrap();
        let mut tx = transport();
        let mut ld = loss();
        state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        // The first packet consumed number 0; the counter now points at 1.
        assert_eq!(state.next_packet_number(PacketNumberSpace::Initial), Some(1));

        // Re-installing (a key update) must not reset the counter to 0.
        state.install(PacketNumberSpace::Initial, client_keys());
        assert_eq!(state.next_packet_number(PacketNumberSpace::Initial), Some(1));
    }

    #[test]
    fn discard_removes_a_space() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        state.discard(PacketNumberSpace::Initial);
        assert!(!state.is_installed(PacketNumberSpace::Initial));
        let err = state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 4))
            .unwrap_err();
        assert!(matches!(err, SendStateError::SpaceNotInstalled(PacketNumberSpace::Initial)));
    }

    // ---- enqueue --------------------------------------------------------

    #[test]
    fn enqueue_into_an_uninstalled_space_is_rejected() {
        let mut state = send_state(&[]);
        let err = state
            .enqueue(PacketNumberSpace::Handshake, crypto(0, 4))
            .unwrap_err();
        match err {
            SendStateError::SpaceNotInstalled(space) => {
                assert_eq!(space, PacketNumberSpace::Handshake);
            }
            other => panic!("expected SpaceNotInstalled, got {other:?}"),
        }
    }

    #[test]
    fn enqueue_marks_the_space_pending() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        assert!(!state.has_pending());
        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 16))
            .unwrap();
        assert!(state.has_pending());
        assert!(state.pending_in(PacketNumberSpace::Initial));
        assert!(!state.pending_in(PacketNumberSpace::Handshake));
    }

    // ---- flush ----------------------------------------------------------

    #[test]
    fn flush_with_nothing_pending_writes_no_datagram() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        let mut tx = transport();
        let mut ld = loss();
        let report = state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        assert_eq!(report, FlushReport::default());
        assert!(tx.sent.is_empty());
    }

    #[test]
    fn flush_sends_one_space_and_records_it_into_loss_and_congestion() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 32))
            .unwrap();
        let mut tx = transport();
        let mut ld = loss();

        let report = state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        assert_eq!(report.datagrams_sent, 1);
        assert_eq!(report.packets_sent, 1);
        assert_eq!(tx.sent.len(), 1);
        // The CRYPTO packet is ack-eliciting and in flight (RFC 9002 §2): loss
        // detection and the Initial congestion controller both saw it.
        assert_eq!(ld.registry(PacketNumberSpace::Initial).outstanding(), 1);
        assert!(ld.registry(PacketNumberSpace::Initial).ack_eliciting_in_flight());
        assert_eq!(
            state
                .congestion(PacketNumberSpace::Initial)
                .unwrap()
                .bytes_in_flight(),
            tx.sent[0].len()
        );
        assert!(!state.has_pending(), "the scheduler drained");
    }

    #[test]
    fn flush_coalesces_initial_and_handshake_into_one_datagram() {
        let mut state = send_state(&[PacketNumberSpace::Initial, PacketNumberSpace::Handshake]);
        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 16))
            .unwrap();
        state
            .enqueue(PacketNumberSpace::Handshake, crypto(0, 16))
            .unwrap();
        let mut tx = transport();
        let mut ld = loss();

        let report = state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        // Both long-header packets ride in a single datagram (RFC 9000 §12.2).
        assert_eq!(report.datagrams_sent, 1);
        assert_eq!(report.packets_sent, 2);
        assert_eq!(tx.sent.len(), 1);
        // Each space's own registry recorded its packet — the routing is per space.
        assert_eq!(ld.registry(PacketNumberSpace::Initial).outstanding(), 1);
        assert_eq!(ld.registry(PacketNumberSpace::Handshake).outstanding(), 1);
    }

    #[test]
    fn flush_routes_frames_to_the_matching_space_registry() {
        // Only the Handshake space has data; the Initial registry must stay empty,
        // proving frames are not misrouted across spaces.
        let mut state = send_state(&[PacketNumberSpace::Initial, PacketNumberSpace::Handshake]);
        state
            .enqueue(PacketNumberSpace::Handshake, crypto(0, 16))
            .unwrap();
        let mut tx = transport();
        let mut ld = loss();

        state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        assert_eq!(ld.registry(PacketNumberSpace::Initial).outstanding(), 0);
        assert_eq!(ld.registry(PacketNumberSpace::Handshake).outstanding(), 1);
    }

    #[test]
    fn flushed_initial_round_trips_through_decrypt() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 32))
            .unwrap();
        let mut tx = transport();
        let mut ld = loss();

        state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        // A peer with the same Initial keys opens the datagram's first packet.
        let got = decrypt_packet(&client_keys(), &tx.sent[0], 0, 0).expect("decrypt");
        assert_eq!(got.packet_number, 0);
        let frames = quic_frame::parse_all(&got.payload).expect("parse frames");
        assert!(matches!(frames[0], Frame::Crypto { offset: 0, .. }), "{frames:?}");
    }

    #[test]
    fn flush_advances_the_packet_number_across_calls() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        let mut tx = transport();
        let mut ld = loss();

        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 16))
            .unwrap();
        state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        state
            .enqueue(PacketNumberSpace::Initial, crypto(16, 16))
            .unwrap();
        state.flush(&mut ld, &mut tx, 1200, now()).unwrap();

        assert_eq!(state.next_packet_number(PacketNumberSpace::Initial), Some(2));
        // The second datagram's packet carries number 1.
        let got = decrypt_packet(&client_keys(), &tx.sent[1], 0, 0).expect("decrypt");
        assert_eq!(got.packet_number, 1);
    }

    // ---- send_padded_initial --------------------------------------------

    #[test]
    fn send_padded_initial_pads_to_the_floor_and_records() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 16))
            .unwrap();
        let mut tx = transport();
        let mut ld = loss();

        let sent = state
            .send_padded_initial(&mut ld, &mut tx, now())
            .unwrap()
            .expect("a padded Initial");
        assert_eq!(sent.packet_number, 0);
        assert_eq!(tx.sent.len(), 1);
        assert!(
            tx.sent[0].len() >= MIN_INITIAL_DATAGRAM_LEN,
            "padded datagram is {} bytes, below the {MIN_INITIAL_DATAGRAM_LEN} floor",
            tx.sent[0].len()
        );
        assert_eq!(ld.registry(PacketNumberSpace::Initial).outstanding(), 1);
        assert_eq!(
            state
                .congestion(PacketNumberSpace::Initial)
                .unwrap()
                .bytes_in_flight(),
            tx.sent[0].len()
        );
    }

    #[test]
    fn send_padded_initial_with_nothing_pending_sends_nothing() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        let mut tx = transport();
        let mut ld = loss();
        let out = state.send_padded_initial(&mut ld, &mut tx, now()).unwrap();
        assert!(out.is_none());
        assert!(tx.sent.is_empty());
        assert_eq!(ld.registry(PacketNumberSpace::Initial).outstanding(), 0);
    }

    #[test]
    fn send_padded_initial_without_the_initial_space_sends_nothing() {
        let mut state = send_state(&[PacketNumberSpace::Handshake]);
        let mut tx = transport();
        let mut ld = loss();
        let out = state.send_padded_initial(&mut ld, &mut tx, now()).unwrap();
        assert!(out.is_none());
        assert!(tx.sent.is_empty());
    }

    // ---- header fields --------------------------------------------------

    #[test]
    fn set_token_is_echoed_into_the_initial_header() {
        let mut state = send_state(&[PacketNumberSpace::Initial]);
        state.set_token(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(state.token(), &[0xDE, 0xAD, 0xBE, 0xEF]);
        state
            .enqueue(PacketNumberSpace::Initial, crypto(0, 16))
            .unwrap();
        let mut tx = transport();
        let mut ld = loss();
        // The token widens the header; the datagram must still decrypt, proving the
        // token rode in the Initial header the AEAD authenticates.
        state.flush(&mut ld, &mut tx, 1200, now()).unwrap();
        let got = decrypt_packet(&client_keys(), &tx.sent[0], 0, 0).expect("decrypt with token");
        assert_eq!(got.packet_number, 0);
    }
}
