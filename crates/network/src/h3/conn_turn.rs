//! QUIC connection turn integrator (RFC 9000 §12.2, §13.2.1; RFC 9002 §6.2.4, §7;
//! h3::conn_turn): the composition slice that joins the two halves the lower slices
//! deliberately left apart — the receive-side [`driver::ConnectionDriver`](super::driver::ConnectionDriver),
//! which decides *when* to ingest a datagram or act on a timer and reports the
//! resulting send obligations as [`driver::DriverAction`](super::driver::DriverAction)s,
//! and the send-side [`send_state::ConnectionSendState`](super::send_state::ConnectionSendState),
//! which owns the per-space frame schedulers, packet-number senders, protection
//! keys, and congestion controllers and drains them onto the wire.
//!
//! ## The seam this slice closes
//!
//! The driver's documentation is explicit that "assembling and writing the outgoing
//! datagrams is the caller's job": on a timer wake it applies the *receiver*-side
//! effects itself (abandoning a path, silently closing on idle, discarding after the
//! draining period) and reports the *sender*-side obligations — the probes to send,
//! the packets loss detection declared lost, the ACKs now owed — for a caller to
//! carry out against the send path it does not own. The send state's documentation
//! is the mirror image: it *is* that owner, but it takes no timers and makes no
//! decisions; a caller enqueues frames and calls
//! [`flush`](super::send_state::ConnectionSendState::flush). This slice is that
//! caller. It holds both, turns each [`DriverAction`](super::driver::DriverAction)
//! into the send-side operation it names, and flushes.
//!
//! ## What applying one action does
//!
//! [`ConnectionTurn::apply_action`] maps each action to exactly one send-side effect
//! and reports it as a [`TurnEffect`]:
//!
//! - [`DriverAction::SendProbe`](super::driver::DriverAction::SendProbe) → enqueue
//!   `count` ack-eliciting **PING** frames in the named space (RFC 9002 §6.2.4: an
//!   endpoint with no unacked data to retransmit sends PING to force an ACK and
//!   break the loss-timer deadlock). The exponential backoff was already advanced
//!   inside the driver's loss detection.
//! - [`DriverAction::PacketsLost`](super::driver::DriverAction::PacketsLost) → fold
//!   the lost packets' byte sizes into the space's
//!   [`recovery::CongestionController`](super::recovery::CongestionController) via
//!   [`on_packets_lost`](super::recovery::CongestionController::on_packets_lost)
//!   (RFC 9002 §7.6): the window reacts to the loss. Retransmitting the *contents*
//!   of the lost frames is a separate concern — the sent-packet registry tracks
//!   packet metadata, not the frames a packet carried — and is left to the
//!   stream/CRYPTO layer that re-derives unacked data; this slice performs only the
//!   congestion response, which is what the loss timer directly owes.
//! - [`DriverAction::SendAck`](super::driver::DriverAction::SendAck) → build the
//!   owed acknowledgement from the receiver state
//!   ([`connection::QuicConnection::generate_ack`](super::connection::QuicConnection::generate_ack))
//!   and enqueue it in the same space (RFC 9000 §13.2.1). No frame is enqueued when
//!   nothing is actually owed (a concurrent packet may have already carried the ACK).
//! - [`DriverAction::PathAbandoned`](super::driver::DriverAction::PathAbandoned),
//!   [`DriverAction::IdleTimeout`](super::driver::DriverAction::IdleTimeout),
//!   [`DriverAction::Drained`](super::driver::DriverAction::Drained) → the driver
//!   already applied these to the receiver state; they are surfaced as terminal /
//!   reactive [`TurnEffect`]s so the owner of the turn can tear the connection down
//!   or react (revert a migration). No send-side work is produced.
//!
//! After applying the actions a wake produced, the caller calls
//! [`ConnectionTurn::flush`] once: it borrows the driver's loss registries and
//! transport together ([`driver::ConnectionDriver::send_flush_parts_mut`](super::driver::ConnectionDriver::send_flush_parts_mut))
//! and hands them to the send state's flush, which coalesces every installed space's
//! queued frames into datagrams and records each sent packet back into the same
//! registries (RFC 9000 §12.2).
//!
//! ## Purity
//!
//! Like every slice below it, this module reads no clock of its own: the `now` for
//! every timer decision, ACK timestamp, and sent-packet stamp is caller-supplied,
//! and the only real I/O is the transport write, mockable through
//! [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport). A synthetic
//! clock and a scripted transport drive a whole connection turn deterministically.

use std::time::Instant;

use super::driver::{ConnectionDriver, DriverAction};
use super::loss::{PacketNumberSpace, SentPacket};
use super::quic_frame::Frame;
use super::recovery::LostPacket;
use super::send_path::{FlushError, FlushReport};
use super::send_state::{ConnectionSendState, SendStateError};
use super::udp::DatagramTransport;

/// The default ACK Delay Exponent (RFC 9000 §18.2): the peer's `ack_delay` in an ACK
/// frame is scaled by `2^ack_delay_exponent`. Used when a connection did not
/// negotiate a different value in its transport parameters.
pub const DEFAULT_ACK_DELAY_EXPONENT: u64 = 3;

/// The observable result of applying one [`DriverAction`] to the send state
/// ([`ConnectionTurn::apply_action`]).
///
/// Every send-producing action reports what it queued so the turn's owner can see
/// the connection make progress; the terminal actions report that the connection has
/// ended (the driver already applied them to the receiver state).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnEffect {
    /// `count` ack-eliciting PING probe frames were queued in `space`
    /// (RFC 9002 §6.2.4). They flush on the next [`ConnectionTurn::flush`].
    ProbeQueued {
        /// The space the probes were queued in.
        space: PacketNumberSpace,
        /// How many PING frames were queued.
        count: u8,
    },
    /// The lost packets' bytes were removed from `space`'s congestion window and the
    /// controller entered (or extended) a recovery period (RFC 9002 §7.6).
    CongestionReacted {
        /// The space the losses were detected in.
        space: PacketNumberSpace,
        /// How many packets were folded into the congestion response.
        lost: usize,
    },
    /// An owed acknowledgement was built and queued in `space` (RFC 9000 §13.2.1).
    AckQueued(PacketNumberSpace),
    /// A delayed-ACK timer fired but nothing was actually owed in `space` (a
    /// concurrent packet already carried the acknowledgement); no frame was queued.
    AckNotOwed(PacketNumberSpace),
    /// Path validation was abandoned after its `3·PTO` deadline (RFC 9000 §8.2.4).
    /// The driver already failed the path; the owner may revert a migration.
    PathAbandoned,
    /// The idle timeout elapsed (RFC 9000 §10.1): the connection is silently closed.
    /// Terminal — the owner drops the turn and sends nothing further.
    IdleTimeout,
    /// The closing / draining period elapsed (RFC 9000 §10.2): the connection state
    /// was discarded. Terminal — the owner tears the turn down.
    Drained,
}

impl TurnEffect {
    /// Whether this effect ends the connection: the owner must stop driving the turn
    /// and send nothing further ([`TurnEffect::IdleTimeout`], [`TurnEffect::Drained`]).
    pub fn is_terminal(&self) -> bool {
        matches!(self, TurnEffect::IdleTimeout | TurnEffect::Drained)
    }
}

/// Something that stopped a [`DriverAction`] from being carried out against the send
/// state ([`ConnectionTurn::apply_action`]).
#[derive(Debug)]
pub enum TurnError {
    /// The action named a packet-number space that has no send state installed — its
    /// packet-protection keys have not been derived, so nothing can be queued for it.
    /// A probe or ACK can only be produced for a space the handshake has reached.
    SpaceNotInstalled(PacketNumberSpace),
    /// A frame could not be queued into a space's scheduler (it was refused by the
    /// packet type, overflowed the payload budget, or was unserializable). Carries
    /// the underlying [`SendStateError`].
    Enqueue(SendStateError),
}

impl core::fmt::Display for TurnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SpaceNotInstalled(space) => {
                write!(f, "QUIC turn: {space:?} space has no send state installed")
            }
            Self::Enqueue(e) => write!(f, "QUIC turn: {e}"),
        }
    }
}

impl std::error::Error for TurnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SpaceNotInstalled(_) => None,
            Self::Enqueue(e) => Some(e),
        }
    }
}

impl From<SendStateError> for TurnError {
    fn from(e: SendStateError) -> Self {
        match e {
            SendStateError::SpaceNotInstalled(space) => Self::SpaceNotInstalled(space),
            other => Self::Enqueue(other),
        }
    }
}

/// One running QUIC connection's full turn: the receive-side driver joined to the
/// send-side state, with the `now`-free bridge that turns the driver's
/// [`DriverAction`]s into send-side frame enqueues and congestion updates and flushes
/// the result onto the wire.
///
/// A caller drives it one turn at a time:
///
/// 1. [`ConnectionTurn::driver_mut`] → `wait(now)` blocks for the next event.
/// 2. On [`Wakeup::Datagram`](super::event_loop::Wakeup::Datagram) → `driver_mut().ingest(..)`;
///    on [`Wakeup::TimerExpired`](super::event_loop::Wakeup::TimerExpired) →
///    [`ConnectionTurn::dispatch_and_apply`], which drives the elapsed timers and
///    applies each resulting action to the send state in one call.
/// 3. [`ConnectionTurn::flush`] drains every queued frame onto the transport.
///
/// The receive path (ingesting a datagram, installing handshake keys, reading
/// reassembled CRYPTO) is reached through [`ConnectionTurn::driver_mut`]; the send
/// path (installing send keys, enqueuing CRYPTO/STREAM frames, discarding a space)
/// through [`ConnectionTurn::send_mut`].
#[derive(Debug)]
pub struct ConnectionTurn<T: DatagramTransport> {
    /// The receive-side driver: event loop, receiver state, loss detection (owning
    /// the send-side sent-packet registries and PTO timer), timers, and receive keys.
    driver: ConnectionDriver<T>,
    /// The send-side state: per-space schedulers, packet-number senders, protection
    /// keys, and congestion controllers, plus the shared packet-header fields.
    send: ConnectionSendState,
    /// The maximum datagram payload a flush may build, in bytes — the current path
    /// MTU (RFC 9000 §14). Passed through to every [`ConnectionSendState::flush`].
    max_datagram_len: usize,
    /// The ACK Delay Exponent this connection uses when encoding an owed ACK frame's
    /// delay (RFC 9000 §18.2), from the local transport parameters.
    ack_delay_exponent: u64,
}

impl<T: DatagramTransport> ConnectionTurn<T> {
    /// Joins a receive-side `driver` and send-side `send` into one turn, flushing at
    /// most `max_datagram_len`-byte datagrams and encoding owed ACK delays with
    /// `ack_delay_exponent` (RFC 9000 §18.2; pass [`DEFAULT_ACK_DELAY_EXPONENT`] when
    /// the connection did not negotiate one).
    pub fn new(
        driver: ConnectionDriver<T>,
        send: ConnectionSendState,
        max_datagram_len: usize,
        ack_delay_exponent: u64,
    ) -> Self {
        Self { driver, send, max_datagram_len, ack_delay_exponent }
    }

    /// The receive-side driver, borrowed immutably (e.g. to read the lifecycle or the
    /// timers).
    pub fn driver(&self) -> &ConnectionDriver<T> {
        &self.driver
    }

    /// The receive-side driver, borrowed mutably: `wait`, `ingest`, key installs, and
    /// reading reassembled CRYPTO all go through it.
    pub fn driver_mut(&mut self) -> &mut ConnectionDriver<T> {
        &mut self.driver
    }

    /// The send-side state, borrowed immutably (e.g. to check pending frames or a
    /// congestion window).
    pub fn send(&self) -> &ConnectionSendState {
        &self.send
    }

    /// The send-side state, borrowed mutably: installing send keys, enqueuing
    /// CRYPTO/STREAM frames, and discarding a space all go through it.
    pub fn send_mut(&mut self) -> &mut ConnectionSendState {
        &mut self.send
    }

    /// The maximum datagram payload a flush builds, in bytes (the current path MTU).
    pub fn max_datagram_len(&self) -> usize {
        self.max_datagram_len
    }

    /// Updates the maximum datagram payload a flush may build — e.g. after Path MTU
    /// Discovery raises or lowers the path MTU (RFC 9000 §14).
    pub fn set_max_datagram_len(&mut self, max_datagram_len: usize) {
        self.max_datagram_len = max_datagram_len;
    }

    /// Carries out one [`DriverAction`] against the send state, reporting the
    /// resulting [`TurnEffect`].
    ///
    /// The mapping is documented on the module: a probe queues PING frames, a loss
    /// reacts the congestion window, an owed ACK is built and queued, and the
    /// terminal actions are surfaced (the driver already applied them). No frame is
    /// written here — the effects land in the schedulers and flush on the next
    /// [`ConnectionTurn::flush`].
    ///
    /// # Errors
    ///
    /// [`TurnError`] when a probe or ACK targets a space with no send state installed,
    /// or the scheduler refuses the frame. The congestion and terminal actions never
    /// fail.
    pub fn apply_action(
        &mut self,
        action: DriverAction,
        now: Instant,
    ) -> Result<TurnEffect, TurnError> {
        match action {
            DriverAction::SendProbe { space, count } => {
                if !self.send.is_installed(space) {
                    return Err(TurnError::SpaceNotInstalled(space));
                }
                // RFC 9002 §6.2.4: with no unacked data to retransmit, PING is the
                // ack-eliciting probe that forces an ACK and breaks the deadlock.
                for _ in 0..count {
                    self.send.enqueue(space, Frame::Ping)?;
                }
                Ok(TurnEffect::ProbeQueued { space, count })
            }
            DriverAction::PacketsLost { space, lost } => {
                let n = lost.len();
                if let Some(cc) = self.send.congestion_mut(space) {
                    let converted: Vec<LostPacket> = lost.iter().map(sent_to_lost).collect();
                    cc.on_packets_lost(&converted, now);
                }
                Ok(TurnEffect::CongestionReacted { space, lost: n })
            }
            DriverAction::SendAck(space) => {
                if !self.send.is_installed(space) {
                    return Err(TurnError::SpaceNotInstalled(space));
                }
                match self
                    .driver
                    .connection_mut()
                    .generate_ack(space, now, self.ack_delay_exponent)
                {
                    Some(ack) => {
                        self.send.enqueue(space, ack)?;
                        Ok(TurnEffect::AckQueued(space))
                    }
                    None => Ok(TurnEffect::AckNotOwed(space)),
                }
            }
            DriverAction::PathAbandoned => Ok(TurnEffect::PathAbandoned),
            DriverAction::IdleTimeout => Ok(TurnEffect::IdleTimeout),
            DriverAction::Drained => Ok(TurnEffect::Drained),
        }
    }

    /// Drives every timer elapsed at `now` into the driver
    /// ([`ConnectionDriver::dispatch_timers`](super::driver::ConnectionDriver::dispatch_timers))
    /// and applies each resulting [`DriverAction`] to the send state, returning the
    /// [`TurnEffect`]s in the order the timers fired.
    ///
    /// Call this after a [`Wakeup::TimerExpired`](super::event_loop::Wakeup::TimerExpired),
    /// having read the wall clock once for `now`, then [`ConnectionTurn::flush`] to
    /// put the queued frames on the wire.
    ///
    /// # Errors
    ///
    /// The first [`TurnError`] an action produces (a probe or ACK for an
    /// uninstalled space, or a rejected frame); the effects applied before it are
    /// lost. In a well-formed connection every timer that fires for a space implies
    /// that space's keys exist, so this does not occur in practice.
    pub fn dispatch_and_apply(&mut self, now: Instant) -> Result<Vec<TurnEffect>, TurnError> {
        let actions = self.driver.dispatch_timers(now);
        let mut effects = Vec::with_capacity(actions.len());
        for action in actions {
            effects.push(self.apply_action(action, now)?);
        }
        Ok(effects)
    }

    /// Flushes every installed space's queued frames onto the transport as coalesced
    /// datagrams, recording each sent packet into the driver's loss registries and
    /// its space's congestion controller (RFC 9000 §12.2).
    ///
    /// Borrows the loss detection and the transport from the driver together
    /// ([`ConnectionDriver::send_flush_parts_mut`](super::driver::ConnectionDriver::send_flush_parts_mut))
    /// and hands them to [`ConnectionSendState::flush`](super::send_state::ConnectionSendState::flush).
    ///
    /// # Errors
    ///
    /// [`FlushError`] from the send path: a packet could not be built, a datagram
    /// write failed, or [`ConnectionTurn::max_datagram_len`] is too small to hold any
    /// packet.
    pub fn flush(&mut self, now: Instant) -> Result<FlushReport, FlushError> {
        let max = self.max_datagram_len;
        let (loss, transport) = self.driver.send_flush_parts_mut();
        self.send.flush(loss, transport, max, now)
    }

    /// Sends the client's first-flight Initial as its own padded datagram (RFC 9000
    /// §14.1) and records it into the driver's Initial loss registry and congestion
    /// controller.
    ///
    /// Returns `Ok(None)` when the Initial space is not installed or has nothing
    /// queued.
    ///
    /// # Errors
    ///
    /// [`FlushError`] from the send path: the padded Initial could not be built or the
    /// datagram write failed.
    pub fn send_padded_initial(&mut self, now: Instant) -> Result<Option<SentPacket>, FlushError> {
        let (loss, transport) = self.driver.send_flush_parts_mut();
        self.send.send_padded_initial(loss, transport, now)
    }

    /// Splits the turn back into its receive-side driver and send-side state, e.g. to
    /// hand ownership on or reclaim the transport.
    pub fn into_parts(self) -> (ConnectionDriver<T>, ConnectionSendState) {
        (self.driver, self.send)
    }
}

/// Projects a sent-packet record onto the loss input the congestion controller takes
/// (RFC 9002 §7.6): only the send time and the byte count matter for the congestion
/// response — the packet number and ack-eliciting flag do not.
fn sent_to_lost(packet: &SentPacket) -> LostPacket {
    LostPacket { time_sent: packet.time_sent, size: packet.sent_bytes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::DriverAction;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::loss::{PacketNumberSpace, SentPacket};
    use crate::h3::packet_crypt::decrypt_packet;
    use crate::h3::pto::LossDetection;
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::recv_path::RecvKeyRing;
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::Duration;

    /// A fixed base instant; the module reads no clock of its own.
    fn base() -> Instant {
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

    fn keys() -> InitialKeys {
        InitialKeys::derive(&dcid())
    }

    fn connection(now: Instant) -> QuicConnection {
        QuicConnection::new_client(
            ConnectionConfig {
                peer_initial_cid: dcid(),
                local_initial_cid: vec![0x11, 0x22, 0x33, 0x44],
                active_connection_id_limit: 8,
                peer_active_connection_id_limit: 8,
                peer_initial_max_data: 1_000_000,
                peer_initial_max_streams_bidi: 100,
                peer_initial_max_streams_uni: 100,
                pto: Duration::from_millis(100),
            },
            now,
        )
    }

    fn driver(now: Instant) -> ConnectionDriver<MockDatagramTransport> {
        let mut recv_keys = RecvKeyRing::new();
        recv_keys.install(PacketNumberSpace::Initial, keys().client);
        ConnectionDriver::new(
            DatagramEventLoop::new(transport()),
            connection(now),
            LossDetection::new(Duration::from_millis(25)),
            recv_keys,
            4,
        )
    }

    /// A turn with the Initial space installed on both halves (send keys reuse the
    /// Initial client keys — the tests only check framing and routing).
    fn turn(spaces: &[PacketNumberSpace], now: Instant) -> ConnectionTurn<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), vec![0x11, 0x22, 0x33, 0x44], 1200);
        for &space in spaces {
            send.install(space, keys().client);
        }
        ConnectionTurn::new(driver(now), send, 1200, DEFAULT_ACK_DELAY_EXPONENT)
    }

    fn sent(pn: u64, now: Instant, bytes: usize) -> SentPacket {
        SentPacket {
            packet_number: pn,
            time_sent: now,
            ack_eliciting: true,
            in_flight: true,
            sent_bytes: bytes,
        }
    }

    // ---- construction / accessors --------------------------------------

    #[test]
    fn new_exposes_both_halves_and_the_mtu() {
        let now = base();
        let turn = turn(&[PacketNumberSpace::Initial], now);
        assert_eq!(turn.max_datagram_len(), 1200);
        assert!(turn.send().is_installed(PacketNumberSpace::Initial));
        assert!(!turn.driver().connection().handshake_confirmed());
    }

    #[test]
    fn set_max_datagram_len_updates_the_flush_budget() {
        let now = base();
        let mut turn = turn(&[], now);
        turn.set_max_datagram_len(1350);
        assert_eq!(turn.max_datagram_len(), 1350);
    }

    // ---- SendProbe -----------------------------------------------------

    #[test]
    fn a_probe_queues_ping_frames_in_the_space() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        let effect = turn
            .apply_action(
                DriverAction::SendProbe { space: PacketNumberSpace::Initial, count: 2 },
                now,
            )
            .expect("probe applies");
        assert_eq!(
            effect,
            TurnEffect::ProbeQueued { space: PacketNumberSpace::Initial, count: 2 }
        );
        assert!(turn.send().pending_in(PacketNumberSpace::Initial));
    }

    #[test]
    fn a_queued_probe_reaches_the_wire_as_a_ping_frame() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        turn.apply_action(
            DriverAction::SendProbe { space: PacketNumberSpace::Initial, count: 1 },
            now,
        )
        .unwrap();
        // Ride the probe alongside a CRYPTO frame so the Initial packet's protected
        // region clears the RFC 9001 §5.4.2 header-protection sample floor (a lone
        // PING packet is too short; the send path pads the datagram, not the first
        // packet's payload — that framing constraint is send_path's own concern).
        turn.send_mut()
            .enqueue(PacketNumberSpace::Initial, Frame::Crypto { offset: 0, data: vec![0xAB; 16] })
            .unwrap();
        turn.flush(now).expect("flush succeeds");

        // Decode the written datagram and confirm it carries the PING probe. An
        // Initial packet is symmetric-decrypted with the same client keys that
        // sealed it (RFC 9001 §5.2); the CID length arg is unused for long headers.
        let written = &turn.driver_mut().events_mut().transport_mut().sent;
        assert_eq!(written.len(), 1, "exactly one datagram sent");
        let plaintext = decrypt_packet(&keys().client, &written[0], 0, 0)
            .expect("client keys decrypt the packet")
            .payload;
        let frames = quic_frame::parse_all(&plaintext).expect("payload decodes");
        assert!(
            frames.iter().any(|f| matches!(f, Frame::Ping)),
            "probe packet carries PING: {frames:?}"
        );
    }

    #[test]
    fn flush_drains_queued_frames_over_the_drivers_transport() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        // A 16-byte CRYPTO frame is large enough to satisfy header protection, so a
        // plain flush (not the padded-Initial path) writes a datagram.
        turn.send_mut()
            .enqueue(PacketNumberSpace::Initial, Frame::Crypto { offset: 0, data: vec![0xAB; 16] })
            .unwrap();
        let report = turn.flush(now).expect("flush succeeds");
        assert_eq!(report.datagrams_sent, 1, "one datagram written: {report:?}");
        assert_eq!(
            turn.driver_mut().events_mut().transport_mut().sent.len(),
            1,
            "the datagram reached the driver's transport"
        );
        assert!(!turn.send().pending_in(PacketNumberSpace::Initial), "queue drained");
    }

    #[test]
    fn a_probe_for_an_uninstalled_space_errors() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        let err = turn
            .apply_action(
                DriverAction::SendProbe { space: PacketNumberSpace::Handshake, count: 1 },
                now,
            )
            .expect_err("no handshake send state");
        assert!(matches!(
            err,
            TurnError::SpaceNotInstalled(PacketNumberSpace::Handshake)
        ));
    }

    // ---- PacketsLost ---------------------------------------------------

    #[test]
    fn a_loss_shrinks_the_congestion_window_and_reports_the_count() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        // Grow bytes-in-flight so a loss has something to remove.
        let cc = turn
            .send_mut()
            .congestion_mut(PacketNumberSpace::Initial)
            .unwrap();
        let window_before = cc.congestion_window();
        cc.on_packet_sent(1200);

        let effect = turn
            .apply_action(
                DriverAction::PacketsLost {
                    space: PacketNumberSpace::Initial,
                    lost: vec![sent(0, now, 1200)],
                },
                now,
            )
            .expect("loss applies");
        assert_eq!(
            effect,
            TurnEffect::CongestionReacted { space: PacketNumberSpace::Initial, lost: 1 }
        );
        let cc = turn.send().congestion(PacketNumberSpace::Initial).unwrap();
        assert_eq!(cc.bytes_in_flight(), 0, "lost bytes left the flight");
        assert!(
            cc.congestion_window() < window_before,
            "window shrank on loss: {} !< {}",
            cc.congestion_window(),
            window_before
        );
    }

    #[test]
    fn a_loss_for_an_uninstalled_space_is_a_noop_not_an_error() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        // Handshake has no send state; a stray loss for it must not panic or error.
        let effect = turn
            .apply_action(
                DriverAction::PacketsLost {
                    space: PacketNumberSpace::Handshake,
                    lost: vec![sent(0, now, 1200)],
                },
                now,
            )
            .expect("loss for absent space is tolerated");
        assert_eq!(
            effect,
            TurnEffect::CongestionReacted { space: PacketNumberSpace::Handshake, lost: 1 }
        );
    }

    // ---- SendAck -------------------------------------------------------

    #[test]
    fn an_owed_ack_is_built_and_queued() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        // Make the receiver owe an ACK: process a received ack-eliciting packet
        // (a PING is ack-eliciting, RFC 9002 §2).
        turn.driver_mut()
            .connection_mut()
            .process_packet(PacketNumberSpace::Initial, 0, &[Frame::Ping], now)
            .expect("ping processes");

        let effect = turn
            .apply_action(DriverAction::SendAck(PacketNumberSpace::Initial), now)
            .expect("ack applies");
        assert_eq!(effect, TurnEffect::AckQueued(PacketNumberSpace::Initial));
        assert!(turn.send().pending_in(PacketNumberSpace::Initial));
    }

    #[test]
    fn an_unowed_ack_queues_nothing() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        // Nothing received → nothing owed.
        let effect = turn
            .apply_action(DriverAction::SendAck(PacketNumberSpace::Initial), now)
            .expect("ack applies");
        assert_eq!(effect, TurnEffect::AckNotOwed(PacketNumberSpace::Initial));
        assert!(!turn.send().pending_in(PacketNumberSpace::Initial));
    }

    #[test]
    fn an_ack_for_an_uninstalled_space_errors() {
        let now = base();
        let mut turn = turn(&[PacketNumberSpace::Initial], now);
        let err = turn
            .apply_action(DriverAction::SendAck(PacketNumberSpace::Handshake), now)
            .expect_err("no handshake send state");
        assert!(matches!(
            err,
            TurnError::SpaceNotInstalled(PacketNumberSpace::Handshake)
        ));
    }

    // ---- terminal actions ----------------------------------------------

    #[test]
    fn terminal_actions_map_to_terminal_effects() {
        let now = base();
        let mut turn = turn(&[], now);
        assert_eq!(
            turn.apply_action(DriverAction::PathAbandoned, now).unwrap(),
            TurnEffect::PathAbandoned
        );
        assert!(!TurnEffect::PathAbandoned.is_terminal());

        let idle = turn.apply_action(DriverAction::IdleTimeout, now).unwrap();
        assert_eq!(idle, TurnEffect::IdleTimeout);
        assert!(idle.is_terminal());

        let drained = turn.apply_action(DriverAction::Drained, now).unwrap();
        assert_eq!(drained, TurnEffect::Drained);
        assert!(drained.is_terminal());
    }

    // ---- into_parts ----------------------------------------------------

    #[test]
    fn into_parts_returns_both_halves() {
        let now = base();
        let turn = turn(&[PacketNumberSpace::Initial], now);
        let (driver, send) = turn.into_parts();
        assert!(send.is_installed(PacketNumberSpace::Initial));
        assert!(!driver.connection().handshake_confirmed());
    }
}
