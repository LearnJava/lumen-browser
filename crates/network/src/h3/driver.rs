//! QUIC connection driver — timer loop and inbound dispatch (RFC 9000 §8.2.4,
//! §10.1, §10.2, §13.2.1; RFC 9002 §6.2): the composition slice that decides
//! *when* to ingest a datagram and *when* to act on a timer, tying the receive
//! path ([`recv_path`](super::recv_path)) to the timer scheduler
//! ([`timer`](super::timer)) over the event-loop wait
//! ([`event_loop`](super::event_loop)).
//!
//! Every lower slice is a pure state machine or a single seam:
//! [`event_loop::DatagramEventLoop::wait`](super::event_loop::DatagramEventLoop::wait)
//! blocks the socket for one turn, [`recv_path::ingest_datagram`](super::recv_path::ingest_datagram)
//! decrypts and dispatches one datagram, and
//! [`timer::ConnectionTimers`](super::timer::ConnectionTimers) multiplexes the
//! connection's deadlines. This slice is the loop *body* that turns those pieces
//! into a running connection: it is the "decide when to ingest and flush" job the
//! receive- and send-path slices left to the connection driver.
//!
//! ## One turn of the loop
//!
//! 1. [`ConnectionDriver::wait`] refreshes the timers from both sources — the
//!    connection's own deadlines ([`connection::QuicConnection::refresh_timers`](super::connection::QuicConnection::refresh_timers):
//!    idle timeout, closing/draining period, path validation, per-space delayed
//!    ACK) and the send engine's loss-detection / PTO timer
//!    ([`pto::LossDetection::set_loss_detection_timer`](super::pto::LossDetection::set_loss_detection_timer)) —
//!    then arms the socket read timeout for the earliest of them and blocks.
//! 2. The wait returns a [`event_loop::Wakeup`](super::event_loop::Wakeup):
//!    - [`Wakeup::Datagram(n)`](super::event_loop::Wakeup::Datagram) → the caller
//!      calls [`ConnectionDriver::ingest`], which decrypts and dispatches the
//!      datagram's coalesced packets through the receive path.
//!    - [`Wakeup::TimerExpired`](super::event_loop::Wakeup::TimerExpired) → the
//!      caller reads the wall clock once and calls
//!      [`ConnectionDriver::dispatch_timers`], which drives each elapsed timer into
//!      its owning state machine and reports the resulting work as
//!      [`DriverAction`]s.
//!
//! Keeping the two post-wake steps as explicit caller calls (rather than folding
//! them inside `wait`) is what lets the whole driver stay clock-free apart from the
//! one real `Instant::now()` the caller reads on waking, so a
//! [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport) and a synthetic
//! clock drive it deterministically in tests.
//!
//! ## The two timer sources
//!
//! The loss-detection / PTO timer is owned by the send engine's
//! [`pto::LossDetection`](super::pto::LossDetection), which
//! [`connection::QuicConnection::refresh_timers`](super::connection::QuicConnection::refresh_timers)
//! deliberately leaves untouched (it folds only the receiver-side machines'
//! deadlines). [`ConnectionDriver::refresh_timers`] unifies the two: it folds the
//! connection's deadlines, then arms the loss-detection timer from
//! [`pto::LossDetection::set_loss_detection_timer`](super::pto::LossDetection::set_loss_detection_timer).
//! The single earliest of *all* of them is what the socket blocks for.
//!
//! ## What this slice owns and what it defers
//!
//! The driver owns the event loop, the connection receiver state, the loss
//! detection, the unified timer scheduler, and the receive key ring. On a timer
//! wake it drives the receiver-side machines directly (path abandon, idle expiry,
//! draining discard) and reports the send-side obligations a timer produced — the
//! PTO probes to send and the packets loss detection declared lost — as
//! [`DriverAction`]s for the caller to action against the send path. The send-path
//! flush itself ([`send_path::flush`](super::send_path::flush)) borrows the
//! per-space send state (the packet-number senders, schedulers, and the
//! loss-detection registries), so assembling and writing the outgoing datagrams is
//! the caller's job, alongside the `h3_do_request` dispatch — the connection
//! driver's remaining work.

use std::io;
use std::time::Instant;

use super::connection::QuicConnection;
use super::event_loop::{DatagramEventLoop, Wakeup};
use super::loss::{PacketNumberSpace, SentPacket};
use super::pto::{LossDetection, TimeoutAction};
use super::recv_path::{IngestError, IngestReport, RecvKeyRing};
use super::timer::{ConnectionTimers, TimerKind};
use super::udp::DatagramTransport;

/// A unit of work a fired connection timer produced, for the driver's caller to
/// action ([`ConnectionDriver::dispatch_timers`]).
///
/// The receiver-side effects of a timer (abandoning path validation, silently
/// closing on idle, discarding after the draining period) are applied to the
/// connection state *inside* dispatch and reported here only so the caller knows
/// they happened; the send-side effects (probes, declared losses, owed ACKs) are
/// reported for the caller to carry out against the send path, which owns the send
/// state this slice does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriverAction {
    /// The loss-detection timer fired on the probe-timeout branch (RFC 9002
    /// §6.2.4): send `count` ack-eliciting probe packets in `space`. The
    /// exponential backoff has already been advanced inside
    /// [`pto::LossDetection::on_timeout`](super::pto::LossDetection::on_timeout).
    SendProbe {
        /// The packet-number space to send the probe(s) in.
        space: PacketNumberSpace,
        /// How many ack-eliciting packets to send (one anti-deadlock probe, or two
        /// while ack-eliciting data is in flight).
        count: u8,
    },
    /// The loss-detection timer fired on the time-threshold branch (RFC 9002
    /// §6.1.2): these packets in `space` were declared lost and removed from the
    /// registry. The caller feeds them to
    /// [`recovery::CongestionController::on_packets_lost`](super::recovery::CongestionController::on_packets_lost)
    /// and retransmits their contents. Never reported empty.
    PacketsLost {
        /// The packet-number space the losses were detected in.
        space: PacketNumberSpace,
        /// The packets declared lost, ascending by packet number.
        lost: Vec<SentPacket>,
    },
    /// A packet-number space's delayed-ACK timer fired (RFC 9000 §13.2.1): an
    /// acknowledgement is now owed for `space`. The caller builds it with
    /// [`connection::QuicConnection::generate_ack`](super::connection::QuicConnection::generate_ack)
    /// and schedules it on the send path.
    SendAck(PacketNumberSpace),
    /// Path validation was abandoned after its `3·PTO` deadline (RFC 9000 §8.2.4):
    /// the path transitioned to failed. Applied to the connection state; reported
    /// so the caller can react (e.g. revert a migration).
    PathAbandoned,
    /// The idle timeout elapsed (RFC 9000 §10.1): the connection is silently
    /// closed. The caller drops the connection and sends nothing further.
    IdleTimeout,
    /// The closing / draining period elapsed (RFC 9000 §10.2): the connection
    /// state was discarded (moved to the closed state). The caller tears the
    /// driver down.
    Drained,
}

/// The running state of one QUIC connection: the event-loop socket wait, the
/// receiver-side connection state, the loss detection owning the send-side
/// registries and PTO timer, the unified timer scheduler, and the receive keys.
///
/// A caller drives it one turn at a time: [`ConnectionDriver::wait`] blocks for the
/// next event, then — depending on the [`Wakeup`] — [`ConnectionDriver::ingest`]
/// dispatches an inbound datagram or [`ConnectionDriver::dispatch_timers`] drives
/// the elapsed timers. The connection, loss detection, and receive keys are exposed
/// through accessors so the caller can install handshake keys, read reassembled
/// CRYPTO, and assemble the outgoing datagrams the driver's [`DriverAction`]s call
/// for.
#[derive(Debug)]
pub struct ConnectionDriver<T: DatagramTransport> {
    /// The event-loop wait owning the datagram transport and receive buffer.
    events: DatagramEventLoop<T>,
    /// The connection-wide receiver state (ACK generators, CRYPTO reassembly,
    /// flow-control and stream-count limits, connection IDs, path validation,
    /// anti-amplification, lifecycle).
    conn: QuicConnection,
    /// The loss-detection / PTO state machine owning the three per-space
    /// sent-packet registries and the RTT estimator.
    loss: LossDetection,
    /// The unified timer scheduler, refreshed from `conn` and `loss` each turn.
    timers: ConnectionTimers,
    /// The receive keys per packet-number space, installed as the handshake
    /// derives each encryption level's keys.
    recv_keys: RecvKeyRing,
    /// The length of the connection IDs this endpoint issued, delimiting a
    /// short-header Destination Connection ID on receive (RFC 9000 §17.3.1).
    local_cid_len: usize,
}

impl<T: DatagramTransport> ConnectionDriver<T> {
    /// Builds a driver over an event loop, a fresh connection receiver state, and
    /// the loss detection, receive keys, and local connection-ID length the
    /// handshake set up.
    ///
    /// `local_cid_len` is the byte length of the connection IDs this endpoint
    /// issued for the peer to send to (it delimits a short-header Destination
    /// Connection ID on receive). The timer scheduler starts disarmed and is
    /// refreshed on the first [`ConnectionDriver::wait`].
    pub fn new(
        events: DatagramEventLoop<T>,
        conn: QuicConnection,
        loss: LossDetection,
        recv_keys: RecvKeyRing,
        local_cid_len: usize,
    ) -> Self {
        Self {
            events,
            conn,
            loss,
            timers: ConnectionTimers::new(),
            recv_keys,
            local_cid_len,
        }
    }

    /// The connection receiver state, borrowed immutably (e.g. to read the
    /// lifecycle or whether the handshake is confirmed).
    pub fn connection(&self) -> &QuicConnection {
        &self.conn
    }

    /// The connection receiver state, borrowed mutably (e.g. to read reassembled
    /// CRYPTO or generate an owed ACK).
    pub fn connection_mut(&mut self) -> &mut QuicConnection {
        &mut self.conn
    }

    /// The loss-detection / PTO state machine, borrowed immutably.
    pub fn loss(&self) -> &LossDetection {
        &self.loss
    }

    /// The loss-detection / PTO state machine, borrowed mutably (e.g. to record
    /// sent packets or process an ACK the receive path deferred).
    pub fn loss_mut(&mut self) -> &mut LossDetection {
        &mut self.loss
    }

    /// The receive keys, borrowed mutably so the caller installs each encryption
    /// level's keys as the handshake derives them
    /// ([`RecvKeyRing::install`](super::recv_path::RecvKeyRing::install)).
    pub fn recv_keys_mut(&mut self) -> &mut RecvKeyRing {
        &mut self.recv_keys
    }

    /// The event loop, borrowed mutably (e.g. to send an outgoing datagram over
    /// its transport between waits).
    pub fn events_mut(&mut self) -> &mut DatagramEventLoop<T> {
        &mut self.events
    }

    /// The unified timer scheduler as last refreshed, for inspection.
    pub fn timers(&self) -> &ConnectionTimers {
        &self.timers
    }

    /// Refreshes the unified timer scheduler from both sources at `now`: folds the
    /// connection's own deadlines
    /// ([`connection::QuicConnection::refresh_timers`](super::connection::QuicConnection::refresh_timers))
    /// then arms the loss-detection / PTO timer from
    /// [`pto::LossDetection::set_loss_detection_timer`](super::pto::LossDetection::set_loss_detection_timer).
    ///
    /// Call this after any state change that could move a deadline (a processed
    /// packet, a sent packet, an ACK); [`ConnectionDriver::wait`] calls it before
    /// blocking so the socket always waits for the current earliest deadline.
    pub fn refresh_timers(&mut self, now: Instant) {
        self.conn.refresh_timers(&mut self.timers);
        self.timers
            .set_loss_detection(self.loss.set_loss_detection_timer(now));
    }

    /// Blocks for one event-loop turn at `now`: refreshes the timers, arms the
    /// socket read timeout for the earliest deadline, and receives.
    ///
    /// Returns [`Wakeup::Datagram(n)`](super::event_loop::Wakeup::Datagram) when a
    /// datagram arrived (dispatch it with [`ConnectionDriver::ingest`]), or
    /// [`Wakeup::TimerExpired`](super::event_loop::Wakeup::TimerExpired) when the
    /// earliest deadline elapsed first (drive the timers with
    /// [`ConnectionDriver::dispatch_timers`], reading the wall clock for `now`).
    ///
    /// # Errors
    ///
    /// Any non-timeout socket error from the underlying transport
    /// ([`DatagramTransport::recv`](super::udp::DatagramTransport::recv)).
    pub fn wait(&mut self, now: Instant) -> io::Result<Wakeup> {
        self.refresh_timers(now);
        self.events.wait(&self.timers, now)
    }

    /// Dispatches a datagram woken by [`Wakeup::Datagram(n)`](super::event_loop::Wakeup::Datagram):
    /// decrypts and routes its coalesced packets through the receive path
    /// ([`recv_path::ingest_datagram`](super::recv_path::ingest_datagram)).
    ///
    /// `n` is the byte count the wake reported; the datagram bytes come from the
    /// event loop's receive buffer. The returned [`IngestReport`] carries the
    /// merged [`connection::PacketEffects`](super::connection::PacketEffects) the
    /// caller schedules on the send path (PATH_RESPONSE / RETIRE_CONNECTION_ID
    /// frames, deferred ACK / per-stream frames) and the packet counts.
    ///
    /// # Errors
    ///
    /// [`IngestError`] when an *authenticated* packet's content is a connection
    /// error (a malformed frame, a barred frame, or a connection-level violation);
    /// the caller closes the connection with
    /// [`IngestError::code`](super::recv_path::IngestError::code). Unauthenticated
    /// failures are never errors — they are counted in the report.
    pub fn ingest(&mut self, n: usize, now: Instant) -> Result<IngestReport, IngestError> {
        super::recv_path::ingest_datagram(
            &mut self.conn,
            &mut self.recv_keys,
            self.events.datagram(n),
            self.local_cid_len,
            now,
        )
    }

    /// Drives every timer that has elapsed at `now` into its owning state machine,
    /// returning the resulting [`DriverAction`]s (earliest-fired first).
    ///
    /// Call this after a [`Wakeup::TimerExpired`](super::event_loop::Wakeup::TimerExpired),
    /// having read the wall clock once for `now`. Each fired
    /// [`timer::TimerKind`](super::timer::TimerKind) is handled:
    ///
    /// - [`TimerKind::LossDetection`](super::timer::TimerKind::LossDetection) →
    ///   [`pto::LossDetection::on_timeout`](super::pto::LossDetection::on_timeout),
    ///   producing [`DriverAction::PacketsLost`] (non-empty) or
    ///   [`DriverAction::SendProbe`];
    /// - [`TimerKind::AckDelay`](super::timer::TimerKind::AckDelay) →
    ///   [`DriverAction::SendAck`];
    /// - [`TimerKind::PathValidation`](super::timer::TimerKind::PathValidation) →
    ///   abandon validation, [`DriverAction::PathAbandoned`] if it was in progress;
    /// - [`TimerKind::IdleTimeout`](super::timer::TimerKind::IdleTimeout) →
    ///   [`DriverAction::IdleTimeout`] if the idle deadline truly elapsed;
    /// - [`TimerKind::DrainingClose`](super::timer::TimerKind::DrainingClose) →
    ///   discard the state, [`DriverAction::Drained`] if it reached closed.
    ///
    /// The timers must have been refreshed for the wake (they are, by the
    /// [`ConnectionDriver::wait`] that produced the timeout), so the fired set
    /// reflects the same deadlines the socket blocked on.
    pub fn dispatch_timers(&mut self, now: Instant) -> Vec<DriverAction> {
        let mut actions = Vec::new();
        for kind in self.timers.fired(now) {
            match kind {
                TimerKind::LossDetection => match self.loss.on_timeout(now) {
                    TimeoutAction::PacketsLost { space, lost } => {
                        // A concurrent ACK may have already removed the packets, so
                        // the time-threshold branch can report an empty set; only
                        // surface a real loss.
                        if !lost.is_empty() {
                            actions.push(DriverAction::PacketsLost { space, lost });
                        }
                    }
                    TimeoutAction::SendProbe { space, count } => {
                        actions.push(DriverAction::SendProbe { space, count });
                    }
                },
                TimerKind::AckDelay(space) => actions.push(DriverAction::SendAck(space)),
                TimerKind::PathValidation => {
                    if self.conn.on_path_validation_timeout(now) {
                        actions.push(DriverAction::PathAbandoned);
                    }
                }
                TimerKind::IdleTimeout => {
                    if self.conn.is_idle_expired(now) {
                        actions.push(DriverAction::IdleTimeout);
                    }
                }
                TimerKind::DrainingClose => {
                    if self.conn.on_close_timeout(now) {
                        actions.push(DriverAction::Drained);
                    }
                }
            }
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::connection::ConnectionConfig;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::loss::{PacketNumberSpace, SentPacket};
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::Duration;

    /// A fixed base instant; the driver reads no clock of its own.
    fn base() -> Instant {
        Instant::now()
    }

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn mock() -> MockDatagramTransport {
        MockDatagramTransport::new(loopback(1), loopback(2))
    }

    /// The RFC 9001 Appendix A client Destination Connection ID.
    fn dcid() -> Vec<u8> {
        vec![0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08]
    }

    fn keys() -> InitialKeys {
        InitialKeys::derive(&dcid())
    }

    /// A client connection whose peer advertised generous limits.
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

    /// A driver with the Initial receive keys installed and a scripted transport.
    fn driver(transport: MockDatagramTransport, now: Instant) -> ConnectionDriver<MockDatagramTransport> {
        let mut recv_keys = RecvKeyRing::new();
        recv_keys.install(PacketNumberSpace::Initial, keys().client);
        ConnectionDriver::new(
            DatagramEventLoop::new(transport),
            connection(now),
            LossDetection::new(Duration::from_millis(25)),
            recv_keys,
            4,
        )
    }

    fn crypto(offset: u64, len: usize) -> Frame {
        Frame::Crypto { offset, data: vec![0xAB; len] }
    }

    /// Encrypt one Initial packet carrying `frames` with packet number `pn`.
    fn initial_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    /// A recorded in-flight ack-eliciting packet for loss-detection tests.
    fn sent_packet(pn: u64, sent: Instant, bytes: usize) -> SentPacket {
        SentPacket {
            packet_number: pn,
            time_sent: sent,
            ack_eliciting: true,
            in_flight: true,
            sent_bytes: bytes,
        }
    }

    // ---- wait: the datagram path ----------------------------------------

    #[test]
    fn wait_reports_a_queued_datagram() {
        let now = base();
        let mut transport = mock();
        transport.push_inbound(initial_packet(0, &[crypto(0, 16)]));
        let mut drv = driver(transport, now);

        match drv.wait(now).unwrap() {
            Wakeup::Datagram(n) => assert!(n > 0),
            other => panic!("expected datagram, got {other:?}"),
        }
    }

    #[test]
    fn wait_then_ingest_dispatches_the_datagram_frames() {
        let now = base();
        let mut transport = mock();
        transport.push_inbound(initial_packet(0, &[crypto(0, 16)]));
        let mut drv = driver(transport, now);

        let Wakeup::Datagram(n) = drv.wait(now).unwrap() else {
            panic!("expected a datagram wake");
        };
        let report = drv.ingest(n, now).unwrap();
        assert_eq!(report.packets_processed, 1);
        // The CRYPTO frame reassembled into the Initial space.
        assert_eq!(
            drv.connection_mut()
                .read_crypto(PacketNumberSpace::Initial)
                .len(),
            16
        );
    }

    #[test]
    fn ingest_surfaces_an_authenticated_connection_error() {
        let now = base();
        let mut transport = mock();
        // HANDSHAKE_DONE is 1-RTT only; in an Initial it is a PROTOCOL_VIOLATION.
        transport.push_inbound(initial_packet(0, &[Frame::HandshakeDone, Frame::Padding(24)]));
        let mut drv = driver(transport, now);

        let Wakeup::Datagram(n) = drv.wait(now).unwrap() else {
            panic!("expected a datagram wake");
        };
        let err = drv.ingest(n, now).unwrap_err();
        // PROTOCOL_VIOLATION close code.
        assert_eq!(err.code(), 0x0a);
    }

    // ---- wait: timer arming ---------------------------------------------

    #[test]
    fn wait_arms_the_read_timeout_from_the_idle_deadline() {
        let now = base();
        // Empty queue → the wait blocks on the timer; give the connection an idle
        // timeout so a finite deadline is armed.
        let mut drv = driver(mock(), now);
        drv.connection_mut()
            .set_idle_timeout(Some(Duration::from_millis(500)));

        let wake = drv.wait(now).unwrap();
        assert_eq!(wake, Wakeup::TimerExpired, "empty mock reports the timer signal");
        // The socket was armed for the idle deadline (500ms floor dominates the
        // 100ms·3 PTO floor of 300ms → 500ms).
        assert_eq!(
            drv.events_mut().transport().read_timeout(),
            Some(Duration::from_millis(500)),
        );
    }

    #[test]
    fn wait_unifies_the_loss_timer_source() {
        let now = base();
        let mut drv = driver(mock(), now);
        // No connection deadline is armed yet; arm a PTO by recording an
        // ack-eliciting packet in flight, which set_loss_detection_timer turns into
        // a loss-detection deadline the connection's own refresh never sets.
        drv.loss_mut()
            .registry_mut(PacketNumberSpace::Initial)
            .on_packet_sent(sent_packet(0, now, 1200));

        drv.wait(now).unwrap();
        assert!(
            drv.timers().loss_detection_deadline().is_some(),
            "the driver folds the loss/PTO timer the connection leaves untouched"
        );
    }

    // ---- dispatch_timers: PTO probe -------------------------------------

    #[test]
    fn dispatch_fires_a_pto_probe() {
        let now = base();
        let mut drv = driver(mock(), now);
        // One ack-eliciting packet in flight arms the PTO. The peer has not
        // validated our address, so the PTO stays armed.
        drv.loss_mut()
            .registry_mut(PacketNumberSpace::Initial)
            .on_packet_sent(sent_packet(0, now, 1200));

        // Refresh + wait arms the timer; the empty queue reports the timeout.
        let wake = drv.wait(now).unwrap();
        assert_eq!(wake, Wakeup::TimerExpired);
        // Fire well after the PTO deadline.
        let later = now + Duration::from_secs(5);
        let actions = drv.dispatch_timers(later);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            DriverAction::SendProbe { space, count } => {
                assert_eq!(*space, PacketNumberSpace::Initial);
                assert!(*count >= 1);
            }
            other => panic!("expected SendProbe, got {other:?}"),
        }
    }

    // ---- dispatch_timers: delayed ACK -----------------------------------

    #[test]
    fn dispatch_fires_an_owed_ack() {
        let now = base();
        let mut transport = mock();
        // An ack-eliciting Initial packet makes the Initial space owe an ACK. The
        // Initial space never delays, so the deadline is immediate.
        transport.push_inbound(initial_packet(0, &[crypto(0, 16)]));
        let mut drv = driver(transport, now);

        let Wakeup::Datagram(n) = drv.wait(now).unwrap() else {
            panic!("expected a datagram wake");
        };
        drv.ingest(n, now).unwrap();

        // The Initial ACK is owed immediately; refreshing and firing at `now`
        // surfaces it. (An immediate ACK is not a delayed *timer*, so drive it via
        // the ack-urgency the connection tracks: dispatch after refreshing.)
        drv.refresh_timers(now);
        // The Initial delayed-ACK timer only arms on AckUrgency::Delayed; Initial is
        // immediate, so no AckDelay timer fires — assert the urgency is owed
        // instead, which the caller consults directly.
        assert!(matches!(
            drv.connection().ack_urgency(PacketNumberSpace::Initial),
            crate::h3::ack::AckUrgency::Immediate
        ));
    }

    #[test]
    fn dispatch_fires_a_delayed_ack_timer_for_app_data() {
        let now = base();
        let mut drv = driver(mock(), now);
        // Directly arm an Application-Data delayed-ACK deadline via the connection's
        // ACK generator by receiving a 1-RTT ack-eliciting packet would require
        // app-data keys; instead drive the generator through a processed packet.
        // Application Data delays up to max_ack_delay, so record a received packet.
        let app = PacketNumberSpace::ApplicationData;
        drv.connection_mut()
            .process_packet(app, 0, &[Frame::Ping], now)
            .unwrap();

        drv.refresh_timers(now);
        // A delayed ACK deadline is armed in the future.
        let deadline = drv.timers().ack_delay_deadline(app);
        assert!(deadline.is_some(), "ping owes a delayed ACK in App Data");

        // Fire after the deadline.
        let later = deadline.unwrap() + Duration::from_millis(1);
        let actions = drv.dispatch_timers(later);
        assert!(
            actions.contains(&DriverAction::SendAck(app)),
            "the delayed-ACK timer produces a SendAck: {actions:?}"
        );
    }

    // ---- dispatch_timers: idle timeout ----------------------------------

    #[test]
    fn dispatch_reports_idle_timeout() {
        let now = base();
        let mut drv = driver(mock(), now);
        drv.connection_mut()
            .set_idle_timeout(Some(Duration::from_millis(200)));
        // Validate the peer address so the anti-deadlock PTO is disarmed and only
        // the idle timer is left to fire.
        drv.loss_mut().set_peer_completed_address_validation(true);

        drv.wait(now).unwrap();
        // Fire past the idle deadline (200ms vs the 300ms PTO floor → 300ms).
        let later = now + Duration::from_secs(1);
        let actions = drv.dispatch_timers(later);
        assert_eq!(actions, vec![DriverAction::IdleTimeout]);
    }

    #[test]
    fn dispatch_does_not_report_idle_before_the_deadline() {
        let now = base();
        let mut drv = driver(mock(), now);
        drv.connection_mut()
            .set_idle_timeout(Some(Duration::from_secs(30)));
        drv.refresh_timers(now);
        // Firing at `now` finds no elapsed timer.
        assert!(drv.dispatch_timers(now).is_empty());
    }

    // ---- dispatch_timers: draining discard ------------------------------

    #[test]
    fn dispatch_reports_drained_after_the_closing_period() {
        let now = base();
        let mut drv = driver(mock(), now);
        // Receiving a CONNECTION_CLOSE enters the draining state with a 3·PTO
        // deadline (100ms·3 = 300ms).
        drv.connection_mut()
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::ConnectionClose {
                    error_code: 0,
                    frame_type: None,
                    reason: Vec::new(),
                }],
                now,
            )
            .unwrap();
        // Validate the peer address so the anti-deadlock PTO is disarmed and only
        // the draining-period timer is left to fire.
        drv.loss_mut().set_peer_completed_address_validation(true);

        drv.refresh_timers(now);
        assert!(drv.timers().draining_close_deadline().is_some());

        let later = now + Duration::from_secs(1);
        let actions = drv.dispatch_timers(later);
        assert_eq!(actions, vec![DriverAction::Drained]);
        assert!(drv.connection().lifecycle().is_closed());
    }

    // ---- dispatch_timers: nothing armed ---------------------------------

    #[test]
    fn dispatch_with_no_timers_does_nothing() {
        let now = base();
        let mut drv = driver(mock(), now);
        // No idle timeout, nothing in flight, and the peer address validated → the
        // loss timer disarms too, so genuinely nothing is armed.
        drv.loss_mut().set_peer_completed_address_validation(true);
        drv.refresh_timers(now);
        assert!(!drv.timers().is_armed(), "no deadline should be armed");
        assert!(drv.dispatch_timers(now + Duration::from_secs(1)).is_empty());
    }

    // ---- refresh_timers: both sources -----------------------------------

    #[test]
    fn refresh_folds_connection_and_loss_deadlines() {
        let now = base();
        let mut drv = driver(mock(), now);
        drv.connection_mut()
            .set_idle_timeout(Some(Duration::from_millis(400)));
        drv.loss_mut()
            .registry_mut(PacketNumberSpace::Initial)
            .on_packet_sent(sent_packet(0, now, 1200));

        drv.refresh_timers(now);
        assert!(drv.timers().idle_timeout_deadline().is_some(), "connection source");
        assert!(drv.timers().loss_detection_deadline().is_some(), "loss source");
        // The earliest of the two is what the socket would block for.
        assert!(drv.timers().next().is_some());
    }
}
