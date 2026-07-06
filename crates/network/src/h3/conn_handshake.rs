//! QUIC handshake driving loop (RFC 9000 §7, §10.1, §10.2; RFC 9001 §4.1;
//! h3::conn_handshake): the orchestration slice that runs a
//! [`conn_turn::ConnectionTurn`](super::conn_turn::ConnectionTurn) turn after turn
//! until the handshake is confirmed — the "decide when to ingest and flush"
//! event loop the driver and turn slices deliberately left to a caller.
//!
//! ## What this slice closes
//!
//! Every slice below it stops one call short of a loop. The
//! [`driver::ConnectionDriver`](super::driver::ConnectionDriver) blocks for *one*
//! event and reports what a timer produced; the
//! [`conn_turn::ConnectionTurn`](super::conn_turn::ConnectionTurn) applies the
//! actions from *one* wake and flushes *once*. Both document that "driving them
//! one turn at a time is the caller's job". This slice is that caller for the
//! connection's opening phase: it repeats
//!
//! 1. [`ConnectionTurn::driver_mut`](super::conn_turn::ConnectionTurn::driver_mut)
//!    → [`wait`](super::driver::ConnectionDriver::wait) blocks for the next event,
//! 2. on a [`Wakeup::Datagram`](super::event_loop::Wakeup::Datagram) →
//!    [`ingest`](super::driver::ConnectionDriver::ingest) the datagram, then
//!    enqueue any acknowledgement now owed for each installed space (an Initial or
//!    Handshake ACK is *immediate*, not a delayed timer, so it is produced here
//!    rather than by [`dispatch_and_apply`](super::conn_turn::ConnectionTurn::dispatch_and_apply)),
//! 3. on a [`Wakeup::TimerExpired`](super::event_loop::Wakeup::TimerExpired) →
//!    [`dispatch_and_apply`](super::conn_turn::ConnectionTurn::dispatch_and_apply)
//!    drives the elapsed timers (PTO probes, declared losses, delayed ACKs) into
//!    the send state,
//! 4. [`flush`](super::conn_turn::ConnectionTurn::flush) drains every queued frame
//!    onto the wire,
//!
//! until [`QuicConnection::handshake_confirmed`](super::connection::QuicConnection::handshake_confirmed)
//! turns true (the peer sent HANDSHAKE_DONE, RFC 9000 §19.20), a *terminal*
//! [`TurnEffect`](super::conn_turn::TurnEffect) ends the connection
//! ([`IdleTimeout`](super::conn_turn::TurnEffect::IdleTimeout) /
//! [`Drained`](super::conn_turn::TurnEffect::Drained), RFC 9000 §10.1/§10.2), or a
//! caller-set turn budget is spent.
//!
//! ## What it defers
//!
//! Advancing the TLS state machine itself — feeding the reassembled CRYPTO stream
//! ([`read_crypto`](super::connection::QuicConnection::read_crypto)) to the
//! handshake, deriving each encryption level's keys, installing them on both the
//! receive ([`recv_keys_mut`](super::driver::ConnectionDriver::recv_keys_mut)) and
//! send ([`send_mut`](super::conn_turn::ConnectionTurn::send_mut)) halves, and
//! enqueuing the outgoing CRYPTO in response — is a separate concern reached
//! through those accessors ([`HandshakeDriver::turn_mut`]) between polls, or by a
//! later slice that wires the TLS handshake driver to this loop. This slice owns
//! only the *control flow*: the wait / ingest / acknowledge / dispatch / flush
//! turn and the stop conditions.
//!
//! ## Purity
//!
//! Like every slice below it, this module reads no clock of its own: each poll
//! takes the caller-supplied `now`, and [`HandshakeDriver::run`] takes a clock
//! closure it calls once per turn — a synthetic clock and a
//! [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport) drive a whole
//! handshake deterministically in tests.

use std::io;
use std::time::Instant;

use super::conn_turn::{ConnectionTurn, TurnEffect, TurnError};
use super::driver::DriverAction;
use super::event_loop::Wakeup;
use super::loss::PacketNumberSpace;
use super::recv_path::IngestError;
use super::send_path::FlushError;
use super::udp::DatagramTransport;

/// The packet-number spaces the loop acknowledges after ingesting a datagram, in
/// handshake order (RFC 9000 §12.3): an Initial or Handshake ACK is owed
/// immediately, and the Application-Data space carries the acknowledgements that
/// keep a confirmed connection alive.
const ACK_SPACES: [PacketNumberSpace; 3] = [
    PacketNumberSpace::Initial,
    PacketNumberSpace::Handshake,
    PacketNumberSpace::ApplicationData,
];

/// What one [`HandshakeDriver::poll`] turn did.
///
/// A poll is exactly one `wait` → (`ingest` + acknowledge | `dispatch_and_apply`)
/// → `flush` cycle; this reports which branch the wake took so a caller (or
/// [`HandshakeDriver::run`]) can see the connection make progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PollOutcome {
    /// A datagram was ingested and any acknowledgement it made owed was enqueued
    /// and flushed.
    Ingested {
        /// The number of coalesced packets the datagram carried that were
        /// decrypted and dispatched.
        packets: usize,
        /// The number of spaces for which an owed ACK frame was queued this turn.
        acks_queued: usize,
    },
    /// The earliest armed deadline elapsed before any datagram arrived; these are
    /// the [`TurnEffect`]s the fired timers produced (empty when a timer woke the
    /// wait but nothing had actually elapsed at `now`).
    Timers(Vec<TurnEffect>),
}

/// Why [`HandshakeDriver::run`] stopped driving the handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandshakeOutcome {
    /// The peer confirmed the handshake (HANDSHAKE_DONE received, RFC 9000
    /// §19.20). The connection is ready for `h3_do_request` dispatch.
    Confirmed,
    /// A terminal timer ended the connection before it confirmed: the idle
    /// timeout elapsed ([`TurnEffect::IdleTimeout`], RFC 9000 §10.1) or the
    /// closing / draining period expired ([`TurnEffect::Drained`], RFC 9000
    /// §10.2). Carries which one.
    Terminated(TurnEffect),
    /// The turn budget was spent without confirming the handshake or ending the
    /// connection. The caller may `run` again with more turns or give up and fall
    /// back to the H2 / H1.1 path.
    Incomplete,
}

/// Something that stopped a handshake turn.
#[derive(Debug)]
pub enum HandshakeError {
    /// The event-loop wait failed on a non-timeout socket error
    /// ([`ConnectionDriver::wait`](super::driver::ConnectionDriver::wait)).
    Wait(io::Error),
    /// An authenticated inbound packet carried a connection error
    /// ([`ConnectionDriver::ingest`](super::driver::ConnectionDriver::ingest)):
    /// the connection must be closed with [`IngestError::code`](super::recv_path::IngestError::code).
    Ingest(IngestError),
    /// A [`DriverAction`] or an owed ACK could not be carried out against the send
    /// state ([`ConnectionTurn::apply_action`](super::conn_turn::ConnectionTurn::apply_action)).
    Turn(TurnError),
    /// A flush failed to build or write an outgoing datagram
    /// ([`ConnectionTurn::flush`](super::conn_turn::ConnectionTurn::flush)).
    Flush(FlushError),
}

impl core::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Wait(e) => write!(f, "QUIC handshake: wait failed: {e}"),
            Self::Ingest(e) => write!(f, "QUIC handshake: ingest failed: {e}"),
            Self::Turn(e) => write!(f, "QUIC handshake: {e}"),
            Self::Flush(e) => write!(f, "QUIC handshake: flush failed: {e}"),
        }
    }
}

impl std::error::Error for HandshakeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wait(e) => Some(e),
            Self::Ingest(e) => Some(e),
            Self::Turn(e) => Some(e),
            Self::Flush(e) => Some(e),
        }
    }
}

/// Drives one QUIC connection's [`ConnectionTurn`] through its opening handshake:
/// it repeats the wait / ingest / acknowledge / dispatch / flush turn until the
/// handshake confirms, a terminal timer ends the connection, or a turn budget is
/// spent.
///
/// The TLS state machine that consumes the reassembled CRYPTO, derives the
/// handshake and 1-RTT keys, and enqueues the outgoing CRYPTO is not owned here;
/// it is reached through [`HandshakeDriver::turn_mut`] (or wired to this loop by a
/// later slice). This slice is the control flow only.
#[derive(Debug)]
pub struct HandshakeDriver<T: DatagramTransport> {
    /// The connection turn this loop drives: the receive-side driver joined to the
    /// send-side state, with the bridge that turns timer actions into send-side
    /// frames.
    turn: ConnectionTurn<T>,
}

impl<T: DatagramTransport> HandshakeDriver<T> {
    /// Wraps a [`ConnectionTurn`] in a handshake-driving loop. The turn should
    /// already have the Initial space installed on both halves (the client's first
    /// flight is sent with [`ConnectionTurn::send_padded_initial`](super::conn_turn::ConnectionTurn::send_padded_initial)
    /// before, or on the first turn of, driving).
    pub fn new(turn: ConnectionTurn<T>) -> Self {
        Self { turn }
    }

    /// The connection turn, borrowed immutably (e.g. to read the connection
    /// lifecycle or whether the handshake has confirmed).
    pub fn turn(&self) -> &ConnectionTurn<T> {
        &self.turn
    }

    /// The connection turn, borrowed mutably: installing handshake / 1-RTT keys,
    /// reading reassembled CRYPTO, and enqueuing the outgoing CRYPTO between polls
    /// all go through it.
    pub fn turn_mut(&mut self) -> &mut ConnectionTurn<T> {
        &mut self.turn
    }

    /// Whether the peer has confirmed the handshake (HANDSHAKE_DONE received,
    /// RFC 9000 §19.20).
    pub fn is_confirmed(&self) -> bool {
        self.turn.driver().connection().handshake_confirmed()
    }

    /// Splits the loop back into the connection turn it drove, e.g. to hand it to
    /// the request-dispatch slice once the handshake has confirmed.
    pub fn into_turn(self) -> ConnectionTurn<T> {
        self.turn
    }

    /// Runs exactly one turn of the loop at `now`: blocks for the next event, then
    /// either ingests the datagram and acknowledges it or drives the elapsed
    /// timers, and flushes any resulting frames.
    ///
    /// On a datagram wake it ingests the packet and, for each installed space,
    /// enqueues the acknowledgement now owed (immediate for Initial / Handshake,
    /// RFC 9000 §13.2.1) — the send-side counterpart the timer-driven
    /// [`dispatch_and_apply`](super::conn_turn::ConnectionTurn::dispatch_and_apply)
    /// only produces for *delayed* ACKs. On a timer wake it drives the fired
    /// timers. It then flushes, except when a terminal effect already ended the
    /// connection (nothing further is sent, RFC 9000 §10).
    ///
    /// # Errors
    ///
    /// [`HandshakeError`] wrapping the failing step: a socket error from the wait,
    /// an authenticated connection error from the ingest, a rejected send action,
    /// or a failed flush.
    pub fn poll(&mut self, now: Instant) -> Result<PollOutcome, HandshakeError> {
        let wake = self
            .turn
            .driver_mut()
            .wait(now)
            .map_err(HandshakeError::Wait)?;

        match wake {
            Wakeup::Datagram(n) => {
                let report = self
                    .turn
                    .driver_mut()
                    .ingest(n, now)
                    .map_err(HandshakeError::Ingest)?;
                let acks_queued = self.acknowledge(now)?;
                self.turn.flush(now).map_err(HandshakeError::Flush)?;
                Ok(PollOutcome::Ingested {
                    packets: report.packets_processed,
                    acks_queued,
                })
            }
            Wakeup::TimerExpired => {
                let effects = self
                    .turn
                    .dispatch_and_apply(now)
                    .map_err(HandshakeError::Turn)?;
                // A terminal timer ended the connection; send nothing further.
                if !effects.iter().any(TurnEffect::is_terminal) {
                    self.turn.flush(now).map_err(HandshakeError::Flush)?;
                }
                Ok(PollOutcome::Timers(effects))
            }
        }
    }

    /// Enqueues the acknowledgement owed for each installed space at `now`,
    /// returning how many were queued. Reuses
    /// [`ConnectionTurn::apply_action`](super::conn_turn::ConnectionTurn::apply_action)'s
    /// `SendAck` path, which builds the frame from the receiver state and enqueues
    /// it only when one is actually owed (RFC 9000 §13.2.1).
    fn acknowledge(&mut self, now: Instant) -> Result<usize, HandshakeError> {
        let mut queued = 0;
        for space in ACK_SPACES {
            if !self.turn.send().is_installed(space) {
                continue;
            }
            let effect = self
                .turn
                .apply_action(DriverAction::SendAck(space), now)
                .map_err(HandshakeError::Turn)?;
            if matches!(effect, TurnEffect::AckQueued(_)) {
                queued += 1;
            }
        }
        Ok(queued)
    }

    /// Drives the loop until the handshake confirms, a terminal timer ends the
    /// connection, or `max_turns` turns are spent, reading `clock` once per turn
    /// for the wall-clock instant a wake produces.
    ///
    /// Returns [`HandshakeOutcome::Confirmed`] the moment
    /// [`QuicConnection::handshake_confirmed`](super::connection::QuicConnection::handshake_confirmed)
    /// is true (checked before each turn, so an already-confirmed connection
    /// returns without any I/O), [`HandshakeOutcome::Terminated`] when a poll
    /// surfaces a terminal [`TurnEffect`], or [`HandshakeOutcome::Incomplete`]
    /// when the budget is exhausted.
    ///
    /// `max_turns` bounds the loop so a peer that never completes the handshake
    /// cannot spin it forever; a caller that gets [`HandshakeOutcome::Incomplete`]
    /// decides whether to `run` again or fall back to H2 / H1.1.
    ///
    /// # Errors
    ///
    /// The first [`HandshakeError`] any turn produces.
    pub fn run(
        &mut self,
        mut clock: impl FnMut() -> Instant,
        max_turns: usize,
    ) -> Result<HandshakeOutcome, HandshakeError> {
        for _ in 0..max_turns {
            if self.is_confirmed() {
                return Ok(HandshakeOutcome::Confirmed);
            }
            let now = clock();
            if let PollOutcome::Timers(effects) = self.poll(now)?
                && let Some(terminal) = effects.into_iter().find(TurnEffect::is_terminal)
            {
                return Ok(HandshakeOutcome::Terminated(terminal));
            }
        }
        if self.is_confirmed() {
            Ok(HandshakeOutcome::Confirmed)
        } else {
            Ok(HandshakeOutcome::Incomplete)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::ConnectionDriver;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::pto::LossDetection;
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::recv_path::RecvKeyRing;
    use crate::h3::send_state::ConnectionSendState;
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

    fn scid() -> Vec<u8> {
        vec![0x11, 0x22, 0x33, 0x44]
    }

    fn keys() -> InitialKeys {
        InitialKeys::derive(&dcid())
    }

    fn connection(now: Instant) -> QuicConnection {
        QuicConnection::new_client(
            ConnectionConfig {
                peer_initial_cid: dcid(),
                local_initial_cid: scid(),
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

    fn driver(t: MockDatagramTransport, now: Instant) -> ConnectionDriver<MockDatagramTransport> {
        let mut recv_keys = RecvKeyRing::new();
        recv_keys.install(PacketNumberSpace::Initial, keys().client);
        ConnectionDriver::new(
            DatagramEventLoop::new(t),
            connection(now),
            LossDetection::new(Duration::from_millis(25)),
            recv_keys,
            4,
        )
    }

    /// A handshake driver with the Initial space installed on both halves and the
    /// scripted transport `t`.
    fn handshake(t: MockDatagramTransport, now: Instant) -> HandshakeDriver<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), scid(), 1200);
        send.install(PacketNumberSpace::Initial, keys().client);
        let turn = ConnectionTurn::new(
            driver(t, now),
            send,
            1200,
            crate::h3::conn_turn::DEFAULT_ACK_DELAY_EXPONENT,
        );
        HandshakeDriver::new(turn)
    }

    /// Encrypt one Initial packet carrying `frames` with packet number `pn`.
    fn initial_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    fn crypto(offset: u64, len: usize) -> Frame {
        Frame::Crypto { offset, data: vec![0xAB; len] }
    }

    // ---- construction / accessors --------------------------------------

    #[test]
    fn new_exposes_the_turn() {
        let now = base();
        let hs = handshake(transport(), now);
        assert!(hs.turn().send().is_installed(PacketNumberSpace::Initial));
        assert!(!hs.is_confirmed());
    }

    #[test]
    fn into_turn_recovers_the_connection_turn() {
        let now = base();
        let hs = handshake(transport(), now);
        let turn = hs.into_turn();
        assert!(turn.send().is_installed(PacketNumberSpace::Initial));
        assert!(!turn.driver().connection().handshake_confirmed());
    }

    // ---- poll: the datagram path ---------------------------------------

    #[test]
    fn poll_ingests_a_datagram_and_acknowledges_it() {
        let now = base();
        let mut t = transport();
        // An ack-eliciting Initial (CRYPTO) makes the Initial space owe an
        // immediate ACK.
        t.push_inbound(initial_packet(0, &[crypto(0, 16)]));
        let mut hs = handshake(t, now);

        let outcome = hs.poll(now).expect("poll succeeds");
        assert_eq!(outcome, PollOutcome::Ingested { packets: 1, acks_queued: 1 });
        // The CRYPTO reassembled into the Initial space.
        assert_eq!(
            hs.turn_mut()
                .driver_mut()
                .connection_mut()
                .read_crypto(PacketNumberSpace::Initial)
                .len(),
            16
        );
    }

    #[test]
    fn poll_flushes_the_owed_ack_over_the_transport() {
        let now = base();
        let mut t = transport();
        t.push_inbound(initial_packet(0, &[crypto(0, 16)]));
        let mut hs = handshake(t, now);

        hs.poll(now).expect("poll succeeds");
        // The owed Initial ACK was built, queued, and flushed as one datagram.
        assert_eq!(
            hs.turn_mut().driver_mut().events_mut().transport_mut().sent.len(),
            1,
            "the acknowledgement reached the wire"
        );
        assert!(
            !hs.turn().send().pending_in(PacketNumberSpace::Initial),
            "the ACK queue drained on flush"
        );
    }

    #[test]
    fn poll_does_not_acknowledge_a_space_with_no_send_state() {
        let now = base();
        let mut t = transport();
        // An ack-eliciting Initial arrives, so the receiver owes an Initial ACK...
        t.push_inbound(initial_packet(0, &[crypto(0, 16)]));
        // ...but the send side has no space installed (its keys are not derived),
        // so no ACK can be queued (RFC 9000 §12.3: a frame needs a protected
        // packet, which needs keys). The driver still holds the Initial *receive*
        // keys, so the datagram is decrypted and processed.
        let send = ConnectionSendState::new(1, dcid(), scid(), 1200);
        let turn = ConnectionTurn::new(
            driver(t, now),
            send,
            1200,
            crate::h3::conn_turn::DEFAULT_ACK_DELAY_EXPONENT,
        );
        let mut hs = HandshakeDriver::new(turn);

        let outcome = hs.poll(now).expect("poll succeeds");
        assert_eq!(outcome, PollOutcome::Ingested { packets: 1, acks_queued: 0 });
        assert!(!hs.turn().send().pending_in(PacketNumberSpace::Initial));
        assert_eq!(
            hs.turn_mut().driver_mut().events_mut().transport_mut().sent.len(),
            0,
            "nothing could be sent with no send keys"
        );
    }

    #[test]
    fn poll_propagates_an_ingest_connection_error() {
        let now = base();
        let mut t = transport();
        // HANDSHAKE_DONE is 1-RTT only; in an Initial it is a PROTOCOL_VIOLATION.
        t.push_inbound(initial_packet(0, &[Frame::HandshakeDone, Frame::Padding(24)]));
        let mut hs = handshake(t, now);

        let err = hs.poll(now).expect_err("authenticated connection error surfaces");
        match err {
            HandshakeError::Ingest(e) => assert_eq!(e.code(), 0x0a),
            other => panic!("expected an ingest error, got {other:?}"),
        }
    }

    // ---- poll: the timer path ------------------------------------------

    #[test]
    fn poll_on_an_empty_queue_fires_timers_and_does_nothing_yet() {
        let now = base();
        let mut hs = handshake(transport(), now);
        hs.turn_mut()
            .driver_mut()
            .connection_mut()
            .set_idle_timeout(Some(Duration::from_millis(500)));

        // The empty mock reports the timer signal; nothing has elapsed at `now`.
        let outcome = hs.poll(now).expect("poll succeeds");
        assert_eq!(outcome, PollOutcome::Timers(Vec::new()));
    }

    // ---- run: confirmation stop ----------------------------------------

    #[test]
    fn run_returns_confirmed_when_already_confirmed() {
        let now = base();
        let mut t = transport();
        t.push_inbound(initial_packet(0, &[crypto(0, 16)]));
        let mut hs = handshake(t, now);
        // Confirm the handshake directly (HANDSHAKE_DONE in 1-RTT, RFC 9000 §19.20).
        hs.turn_mut()
            .driver_mut()
            .connection_mut()
            .process_packet(PacketNumberSpace::ApplicationData, 0, &[Frame::HandshakeDone], now)
            .expect("handshake-done processes");

        let outcome = hs.run(|| now, 4).expect("run succeeds");
        assert_eq!(outcome, HandshakeOutcome::Confirmed);
        // Confirmed is checked before the first turn, so no I/O happened: the
        // queued datagram is untouched and nothing was sent.
        assert_eq!(
            hs.turn_mut().driver_mut().events_mut().transport_mut().sent.len(),
            0,
            "an already-confirmed connection is not driven"
        );
    }

    // ---- run: terminal stop --------------------------------------------

    #[test]
    fn run_stops_on_idle_timeout() {
        let now = base();
        let mut hs = handshake(transport(), now);
        hs.turn_mut()
            .driver_mut()
            .connection_mut()
            .set_idle_timeout(Some(Duration::from_millis(200)));
        // Validate the peer address so the anti-deadlock PTO is disarmed and only
        // the idle timer is left to fire.
        hs.turn_mut()
            .driver_mut()
            .loss_mut()
            .set_peer_completed_address_validation(true);

        // A clock that jumps past the idle deadline on the second turn.
        let times = [now, now + Duration::from_secs(1)];
        let mut i = 0usize;
        let clock = || {
            let t = times[i.min(times.len() - 1)];
            i += 1;
            t
        };

        let outcome = hs.run(clock, 5).expect("run succeeds");
        assert_eq!(outcome, HandshakeOutcome::Terminated(TurnEffect::IdleTimeout));
        assert!(!hs.is_confirmed());
    }

    // ---- run: budget stop ----------------------------------------------

    #[test]
    fn run_returns_incomplete_when_the_budget_is_spent() {
        let now = base();
        let mut hs = handshake(transport(), now);
        // No idle timeout, nothing in flight, peer validated → no timer ever fires
        // anything terminal, and the empty mock loops on the timer signal.
        hs.turn_mut()
            .driver_mut()
            .loss_mut()
            .set_peer_completed_address_validation(true);

        let outcome = hs.run(|| now, 3).expect("run succeeds");
        assert_eq!(outcome, HandshakeOutcome::Incomplete);
    }
}
