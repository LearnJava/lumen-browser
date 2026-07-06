//! HTTP/3 request driver — running a [`RequestTurn`] to completion over the
//! connection's event loop (RFC 9000 §10.1, §13.2.1; RFC 9002 §6.2; RFC 9114 §4.1,
//! §6.1).
//!
//! [`request_turn`](super::request_turn) joined the request pump to a live QUIC
//! connection turn into a [`RequestTurn`]: a pure state machine that stages a
//! request's STREAM frames into the connection's send queue
//! ([`transmit`](super::request_turn::RequestTurn::transmit)) and routes the
//! per-stream frames a datagram deferred back through the pump
//! ([`ingest`](super::request_turn::RequestTurn::ingest)). But like every slice
//! below it, that turn reads no clock and blocks on no socket — it answers "here is
//! a datagram, route it" and "here are frames to send", leaving *when* to send and
//! *when* to wait to a caller. This slice is that caller for the request phase, the
//! exact mirror of [`conn_handshake::HandshakeDriver`](super::conn_handshake::HandshakeDriver)
//! for the handshake phase: it owns a [`RequestTurn`], drives the connection's
//! event loop one turn at a time, and accumulates the [`H3Response`]s the requests
//! complete.
//!
//! ## One turn — [`RequestDriver::poll`]
//!
//! A poll is one `wait` → (`ingest` + acknowledge | `dispatch_and_apply`) → `flush`
//! cycle over the connection's [`event_loop`](super::event_loop):
//!
//! - On a [`Wakeup::Datagram`](super::event_loop::Wakeup::Datagram) it ingests the
//!   packet through the turn ([`RequestTurn::ingest`](super::request_turn::RequestTurn::ingest)),
//!   collecting every completed response ([`PumpEvent::Response`]) and enqueuing a
//!   RESET_STREAM reply for every [`PumpEvent::StopSending`] the pump reported
//!   (RFC 9000 §3.5, §19.4). It then enqueues the acknowledgement now owed on the
//!   Application-Data space (RFC 9000 §13.2.1) and flushes, so the response's ACK —
//!   and any reset — reaches the peer.
//! - On a [`Wakeup::TimerExpired`](super::event_loop::Wakeup::TimerExpired) it drives
//!   the fired timers ([`ConnectionTurn::dispatch_and_apply`](super::conn_turn::ConnectionTurn::dispatch_and_apply))
//!   and flushes any resulting frames, unless a terminal effect already ended the
//!   connection (RFC 9000 §10) — in which case nothing further is sent.
//!
//! ## Driving to completion — [`RequestDriver::run`]
//!
//! [`RequestDriver::run`] loops the turn until every in-flight request has completed
//! (a response, or an abort retiring the stream), a terminal timer ends the
//! connection, or a turn budget is spent. Before each turn's wait it re-stages the
//! request pump's send half ([`RequestTurn::transmit`](super::request_turn::RequestTurn::transmit)):
//! the first turn puts the request on the wire, and every turn thereafter re-drains
//! whatever a reopened flow-control window (an inbound MAX_STREAM_DATA) newly
//! allowed (RFC 9000 §19.10). This staging lives in `run` rather than `poll` for the
//! same reason [`HandshakeDriver::run`](super::conn_handshake::HandshakeDriver::run)
//! sends the first flight in its loop rather than its poll — the wait must never
//! block with unsent request bytes buffered.
//!
//! Like every slice below it, the driver reads no clock of its own — every `now` is
//! caller-supplied ([`RequestDriver::run`] takes a `clock` closure) — and its only
//! I/O is the transport the connection turn writes over, mockable through
//! [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport). A synthetic
//! clock and a scripted transport drive a whole request round trip deterministically.

use std::io;
use std::time::Instant;

use super::conn_turn::{TurnEffect, TurnError};
use super::driver::DriverAction;
use super::event_loop::Wakeup;
use super::h3_exchange::H3Response;
use super::loss::PacketNumberSpace;
use super::request_dispatch::{DispatchError, SentRequest};
use super::request_exchange::ClientRequest;
use super::request_pump::PumpEvent;
use super::request_turn::{RequestTurn, RequestTurnError};
use super::send_path::FlushError;
use super::send_state::SendStateError;
use super::udp::DatagramTransport;

/// Something that stopped one turn of the request driver.
///
/// The variants trace the poll cycle: [`Wait`](RequestDriverError::Wait) from
/// blocking on the socket, [`Transmit`](RequestDriverError::Transmit) from staging
/// and flushing the request send half, [`Ingest`](RequestDriverError::Ingest) from
/// decoding a datagram and routing its frames, [`Turn`](RequestDriverError::Turn)
/// from a send-side action (an owed ACK, a fired timer),
/// [`Enqueue`](RequestDriverError::Enqueue) from queuing a STOP_SENDING reply, and
/// [`Flush`](RequestDriverError::Flush) from writing the queued frames.
#[derive(Debug)]
pub enum RequestDriverError {
    /// The event-loop wait failed on a non-timeout socket error
    /// ([`ConnectionDriver::wait`](super::driver::ConnectionDriver::wait)).
    Wait(io::Error),
    /// Staging or flushing the request pump's send half failed
    /// ([`RequestTurn::transmit`](super::request_turn::RequestTurn::transmit)).
    Transmit(RequestTurnError),
    /// Ingesting a datagram and routing its deferred per-stream frames failed
    /// ([`RequestTurn::ingest`](super::request_turn::RequestTurn::ingest)): an
    /// authenticated connection error, or a per-stream frame that breached flow
    /// control or the final-size rules.
    Ingest(RequestTurnError),
    /// A send-side turn action was rejected — enqueuing the owed Application-Data
    /// ACK, or applying a fired timer ([`ConnectionTurn::apply_action`](super::conn_turn::ConnectionTurn::apply_action),
    /// [`ConnectionTurn::dispatch_and_apply`](super::conn_turn::ConnectionTurn::dispatch_and_apply)).
    Turn(TurnError),
    /// Queuing a RESET_STREAM reply to a peer's STOP_SENDING failed (RFC 9000 §3.5).
    Enqueue(SendStateError),
    /// Flushing the queued frames onto the transport failed
    /// ([`ConnectionTurn::flush`](super::conn_turn::ConnectionTurn::flush)).
    Flush(FlushError),
}

impl core::fmt::Display for RequestDriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Wait(e) => write!(f, "HTTP/3 request driver: wait failed: {e}"),
            Self::Transmit(e) => write!(f, "HTTP/3 request driver: transmitting: {e}"),
            Self::Ingest(e) => write!(f, "HTTP/3 request driver: ingesting a datagram: {e}"),
            Self::Turn(e) => write!(f, "HTTP/3 request driver: applying a turn action: {e}"),
            Self::Enqueue(e) => write!(f, "HTTP/3 request driver: queuing a reset reply: {e}"),
            Self::Flush(e) => write!(f, "HTTP/3 request driver: flushing: {e}"),
        }
    }
}

impl std::error::Error for RequestDriverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wait(e) => Some(e),
            Self::Transmit(e) | Self::Ingest(e) => Some(e),
            Self::Turn(e) => Some(e),
            Self::Enqueue(e) => Some(e),
            Self::Flush(e) => Some(e),
        }
    }
}

/// What one [`RequestDriver::poll`] turn did.
///
/// A poll is exactly one `wait` → (`ingest` + acknowledge | `dispatch_and_apply`) →
/// `flush` cycle; this reports which branch the wake took so a caller (or
/// [`RequestDriver::run`]) can see the requests make progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestPoll {
    /// A datagram was ingested and routed. `packets` QUIC packets were decrypted and
    /// dispatched, `responses` requests completed this turn (their [`H3Response`]s are
    /// now in [`RequestDriver::responses`]), and `acks_queued` is 1 if an
    /// acknowledgement was owed on the Application-Data space and enqueued.
    Ingested {
        /// The number of coalesced packets decrypted and dispatched from the datagram.
        packets: usize,
        /// How many requests completed on this turn.
        responses: usize,
        /// Whether an owed Application-Data acknowledgement was queued (0 or 1).
        acks_queued: usize,
    },
    /// No datagram arrived before the earliest armed deadline elapsed; the driver
    /// dispatched the fired timers, reported here as their [`TurnEffect`]s (empty when
    /// the wake was spurious — a deadline that had not actually elapsed at `now`).
    Timers(Vec<TurnEffect>),
}

/// Why [`RequestDriver::run`] stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestOutcome {
    /// Every in-flight request completed — a response, or an abort that retired the
    /// stream. The completed responses are in [`RequestDriver::responses`], in
    /// completion order.
    Completed,
    /// A terminal connection effect ended the connection before every request
    /// completed (RFC 9000 §10.1, §10.2): the idle timeout elapsed or the connection
    /// drained. Carries the terminal [`TurnEffect`]; any responses already collected
    /// remain in [`RequestDriver::responses`].
    Terminated(TurnEffect),
    /// The turn budget was spent with requests still in flight. The caller decides
    /// whether to `run` again with a fresh budget or abandon the connection.
    Incomplete,
}

/// A [`RequestTurn`] driven over the connection's event loop, accumulating the
/// [`H3Response`]s its requests complete — the `h3_do_request` driver this task has
/// been building toward.
///
/// Place requests with [`RequestDriver::send_request`], then drive them to
/// completion with [`RequestDriver::run`] (or one turn at a time with
/// [`RequestDriver::poll`], staging sends yourself with
/// [`RequestDriver::transmit`]). Completed responses accumulate in
/// [`RequestDriver::responses`] in completion order.
#[derive(Debug)]
pub struct RequestDriver<T: DatagramTransport> {
    /// The request turn: the request pump wired to a live QUIC connection turn.
    turn: RequestTurn<T>,
    /// The responses completed so far, in completion order.
    responses: Vec<H3Response>,
}

impl<T: DatagramTransport> RequestDriver<T> {
    /// Wraps a live request `turn` (its connection already handshake-complete, with
    /// Application-Data keys installed on both directions) in a driver with no
    /// responses collected yet.
    #[must_use]
    pub fn new(turn: RequestTurn<T>) -> Self {
        Self { turn, responses: Vec::new() }
    }

    /// The request turn, borrowed immutably.
    #[must_use]
    pub fn turn(&self) -> &RequestTurn<T> {
        &self.turn
    }

    /// The request turn, borrowed mutably.
    pub fn turn_mut(&mut self) -> &mut RequestTurn<T> {
        &mut self.turn
    }

    /// The responses completed so far, in completion order.
    #[must_use]
    pub fn responses(&self) -> &[H3Response] {
        &self.responses
    }

    /// Takes the completed responses out of the driver, leaving it empty.
    #[must_use]
    pub fn take_responses(&mut self) -> Vec<H3Response> {
        core::mem::take(&mut self.responses)
    }

    /// Splits the driver into its request turn and the responses it collected.
    #[must_use]
    pub fn into_parts(self) -> (RequestTurn<T>, Vec<H3Response>) {
        (self.turn, self.responses)
    }

    /// The number of requests still in flight — placed but not yet completed or
    /// aborted (RFC 9114 §6.1). [`RequestDriver::run`] stops once this reaches zero.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.turn.pump().active_count()
    }

    /// Whether every placed request has completed (no request is in flight). Note a
    /// driver with no request ever placed is trivially done.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.in_flight() == 0
    }

    /// Places `req` onto a fresh client-initiated bidirectional stream, finishing its
    /// send half (STREAM FIN, RFC 9114 §4.1) — a pass-through to
    /// [`RequestTurn::send_request`](super::request_turn::RequestTurn::send_request).
    /// The rendered bytes wait on the pump's send half until the next
    /// [`RequestDriver::transmit`] (which [`RequestDriver::run`] performs before every
    /// turn) moves them onto the wire.
    ///
    /// # Errors
    ///
    /// [`DispatchError`] if the request cannot be built (RFC 9114 §4.2/§7.2.1) or all
    /// client bidirectional stream identifiers are spent (RFC 9000 §2.1).
    pub fn send_request(&mut self, req: &ClientRequest) -> Result<SentRequest, DispatchError> {
        self.turn.send_request(req)
    }

    /// Stages the request pump's send half and flushes it — a pass-through to
    /// [`RequestTurn::transmit`](super::request_turn::RequestTurn::transmit).
    ///
    /// [`RequestDriver::run`] calls this before every turn; a caller polling by hand
    /// calls it to put newly-placed (or newly-unblocked) request bytes on the wire.
    ///
    /// # Errors
    ///
    /// [`RequestDriverError::Transmit`] wrapping the staging or flush failure.
    pub fn transmit(&mut self, now: Instant) -> Result<(), RequestDriverError> {
        self.turn.transmit(now).map_err(RequestDriverError::Transmit)?;
        Ok(())
    }

    /// Runs exactly one turn of the loop at `now`: blocks for the next event, then
    /// either ingests the datagram — collecting completed responses and queuing a
    /// RESET_STREAM reply for each STOP_SENDING — and acknowledges it, or drives the
    /// elapsed timers, and flushes any resulting frames.
    ///
    /// This does *not* stage the request send half; [`RequestDriver::run`] does that
    /// before each poll, or a hand-driven caller calls [`RequestDriver::transmit`]
    /// first (see the module docs). On a terminal timer the connection has ended and
    /// nothing further is sent (RFC 9000 §10).
    ///
    /// # Errors
    ///
    /// [`RequestDriverError`] wrapping the failing step: a socket error from the
    /// wait, an authenticated connection error or a bad frame from the ingest, a
    /// rejected send action, a rejected reset enqueue, or a failed flush.
    pub fn poll(&mut self, now: Instant) -> Result<RequestPoll, RequestDriverError> {
        let wake = self
            .turn
            .turn_mut()
            .driver_mut()
            .wait(now)
            .map_err(RequestDriverError::Wait)?;

        match wake {
            Wakeup::Datagram(n) => {
                let (report, ingest) =
                    self.turn.ingest(n, now).map_err(RequestDriverError::Ingest)?;
                let mut responses = 0;
                let mut resets = Vec::new();
                for event in ingest.events {
                    match event {
                        PumpEvent::Response(resp) => {
                            self.responses.push(resp);
                            responses += 1;
                        }
                        // The peer asked us to stop sending on this request stream
                        // (RFC 9000 §19.5); the pump built the RESET_STREAM reply
                        // (§3.5) — queue it to go out on this turn's flush.
                        PumpEvent::StopSending { reset, .. } => resets.push(reset),
                        // Progress, an abort (the stream is already retired), or an
                        // ignored frame change no response state here.
                        PumpEvent::Progress | PumpEvent::Aborted { .. } | PumpEvent::Ignored => {}
                    }
                }
                for reset in resets {
                    self.turn
                        .turn_mut()
                        .send_mut()
                        .enqueue(PacketNumberSpace::ApplicationData, reset)
                        .map_err(RequestDriverError::Enqueue)?;
                }
                let acks_queued = self.acknowledge(now)?;
                self.turn.flush(now).map_err(RequestDriverError::Flush)?;
                Ok(RequestPoll::Ingested { packets: report.packets_processed, responses, acks_queued })
            }
            Wakeup::TimerExpired => {
                let effects = self
                    .turn
                    .turn_mut()
                    .dispatch_and_apply(now)
                    .map_err(RequestDriverError::Turn)?;
                // A terminal timer ended the connection; send nothing further.
                if !effects.iter().any(TurnEffect::is_terminal) {
                    self.turn.flush(now).map_err(RequestDriverError::Flush)?;
                }
                Ok(RequestPoll::Timers(effects))
            }
        }
    }

    /// Enqueues the acknowledgement owed on the Application-Data space at `now`,
    /// returning 1 if one was queued and 0 otherwise. The request phase is 1-RTT
    /// only, so Application Data is the sole space that carries request/response
    /// traffic (RFC 9000 §12.5); its keys are installed by the time any request
    /// flows. Reuses [`ConnectionTurn::apply_action`](super::conn_turn::ConnectionTurn::apply_action)'s
    /// `SendAck` path, which builds the frame from the receiver state and enqueues it
    /// only when one is actually owed (RFC 9000 §13.2.1).
    fn acknowledge(&mut self, now: Instant) -> Result<usize, RequestDriverError> {
        if !self.turn.turn().send().is_installed(PacketNumberSpace::ApplicationData) {
            return Ok(0);
        }
        let effect = self
            .turn
            .turn_mut()
            .apply_action(DriverAction::SendAck(PacketNumberSpace::ApplicationData), now)
            .map_err(RequestDriverError::Turn)?;
        Ok(usize::from(matches!(effect, TurnEffect::AckQueued(_))))
    }

    /// Drives the loop until every in-flight request completes, a terminal timer ends
    /// the connection, or `max_turns` turns are spent, reading `clock` once per turn
    /// for the wall-clock instant a wake acts at.
    ///
    /// Each turn re-stages the request pump's send half
    /// ([`RequestDriver::transmit`]) before polling, so the first turn puts the
    /// placed requests on the wire and every turn thereafter re-drains what a reopened
    /// flow-control window newly allowed (RFC 9000 §19.10).
    ///
    /// Returns [`RequestOutcome::Completed`] the moment [`RequestDriver::is_done`] is
    /// true (checked before each turn, so a driver with nothing in flight returns
    /// without any I/O), [`RequestOutcome::Terminated`] when a poll surfaces a
    /// terminal [`TurnEffect`], or [`RequestOutcome::Incomplete`] when the budget is
    /// spent with requests still in flight.
    ///
    /// `max_turns` bounds the loop so a peer that never answers cannot spin it
    /// forever; a caller that gets [`RequestOutcome::Incomplete`] decides whether to
    /// `run` again or abandon the connection.
    ///
    /// # Errors
    ///
    /// The first [`RequestDriverError`] any turn produces.
    pub fn run(
        &mut self,
        mut clock: impl FnMut() -> Instant,
        max_turns: usize,
    ) -> Result<RequestOutcome, RequestDriverError> {
        for _ in 0..max_turns {
            if self.is_done() {
                return Ok(RequestOutcome::Completed);
            }
            let now = clock();
            self.transmit(now)?;
            if let RequestPoll::Timers(effects) = self.poll(now)?
                && let Some(terminal) = effects.into_iter().find(TurnEffect::is_terminal)
            {
                return Ok(RequestOutcome::Terminated(terminal));
            }
        }
        if self.is_done() {
            Ok(RequestOutcome::Completed)
        } else {
            Ok(RequestOutcome::Incomplete)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::conn_turn::{ConnectionTurn, DEFAULT_ACK_DELAY_EXPONENT};
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::ConnectionDriver;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::frame::Frame as H3Frame;
    use crate::h3::h3_request::H3Profile;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::pto::LossDetection;
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::recv_path::RecvKeyRing;
    use crate::h3::request_pump::RequestPump;
    use crate::h3::send_state::ConnectionSendState;
    use crate::h3::request_turn::RequestTurn;
    use crate::h3::stream_manager::StreamManagerConfig;
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

    /// The four-byte local connection ID this connection is addressed by (matching the
    /// `cid_len` the driver is built with, so a short-header packet decodes).
    fn local_cid() -> Vec<u8> {
        vec![0x11, 0x22, 0x33, 0x44]
    }

    fn keys() -> InitialKeys {
        InitialKeys::derive(&dcid())
    }

    fn connection(now: Instant) -> QuicConnection {
        QuicConnection::new_client(
            ConnectionConfig {
                peer_initial_cid: dcid(),
                local_initial_cid: local_cid(),
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

    /// A connection driver over `transport` with Application-Data receive keys
    /// installed (so a scripted 1-RTT response decrypts) and a four-byte local CID.
    fn driver(
        transport: MockDatagramTransport,
        now: Instant,
    ) -> ConnectionDriver<MockDatagramTransport> {
        let mut recv_keys = RecvKeyRing::new();
        recv_keys.install(PacketNumberSpace::ApplicationData, keys().client);
        ConnectionDriver::new(
            DatagramEventLoop::new(transport),
            connection(now),
            LossDetection::new(Duration::from_millis(25)),
            recv_keys,
            4,
        )
    }

    /// A connection turn over `transport` with Application-Data installed on both
    /// directions — the state a completed handshake leaves behind, so STREAM frames
    /// flow in both directions (STREAM is 1-RTT only, RFC 9000 §12.5). The send keys
    /// reuse the client Initial keys; the routing under test does not depend on the
    /// actual key material, only that encrypt and decrypt agree.
    fn conn_turn(
        transport: MockDatagramTransport,
        now: Instant,
    ) -> ConnectionTurn<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), local_cid(), 1200);
        send.install(PacketNumberSpace::ApplicationData, keys().client);
        ConnectionTurn::new(driver(transport, now), send, 1200, DEFAULT_ACK_DELAY_EXPONENT)
    }

    fn stream_config() -> StreamManagerConfig {
        StreamManagerConfig {
            initial_max_stream_data_bidi_local: 1 << 20,
            initial_max_stream_data_bidi_remote: 1 << 20,
            initial_max_stream_data_uni: 1 << 20,
            initial_max_data: 1 << 20,
            initial_max_streams_bidi: 100,
            initial_max_streams_uni: 100,
        }
    }

    fn pump() -> RequestPump {
        RequestPump::new(stream_config(), 1 << 20)
    }

    /// A request driver over `transport` ready to place and drive requests.
    fn request_driver(
        transport: MockDatagramTransport,
        now: Instant,
    ) -> RequestDriver<MockDatagramTransport> {
        RequestDriver::new(RequestTurn::with_default_frame_len(conn_turn(transport, now), pump()))
    }

    fn get(path: &'static [u8]) -> ClientRequest<'static> {
        ClientRequest {
            profile: H3Profile::Chrome,
            method: b"GET",
            scheme: b"https",
            authority: b"example.com",
            path,
            headers: &[],
            body: b"",
            use_huffman: true,
        }
    }

    fn post(path: &'static [u8], body: &'static [u8]) -> ClientRequest<'static> {
        ClientRequest {
            profile: H3Profile::Chrome,
            method: b"POST",
            scheme: b"https",
            authority: b"example.com",
            path,
            headers: &[],
            body,
            use_huffman: true,
        }
    }

    fn status(code: &[u8]) -> HeaderField {
        HeaderField::new(b":status".to_vec(), code.to_vec())
    }

    /// The response-stream bytes for a `code`/`body` response (HEADERS + DATA).
    fn response_bytes(code: &[u8], body: &[u8]) -> Vec<u8> {
        let block = qpack::encode_field_section(&[status(code)], true);
        let mut out = Vec::new();
        H3Frame::Headers(block).encode(&mut out).unwrap();
        H3Frame::Data(body.to_vec()).encode(&mut out).unwrap();
        out
    }

    /// Encrypt one short-header (1-RTT) packet carrying `frames` with packet number
    /// `pn`, standing in for the datagram the server sends in the request phase. The
    /// four-byte DCID matches the connection's local CID and the driver's `cid_len`.
    fn one_rtt_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = local_cid();
        let header = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &dcid };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    /// A STREAM frame carrying the response for `code`/`body` on `stream_id`, with FIN
    /// set (the server's end of the response, RFC 9114 §4.1).
    fn response_stream(stream_id: u64, code: &[u8], body: &[u8]) -> Frame {
        Frame::Stream { stream_id, offset: 0, fin: true, data: response_bytes(code, body) }
    }

    // ---- construction / accessors --------------------------------------

    #[test]
    fn new_starts_with_no_responses_and_nothing_in_flight() {
        let now = base();
        let d = request_driver(transport(), now);
        assert!(d.responses().is_empty());
        assert_eq!(d.in_flight(), 0);
        assert!(d.is_done(), "a driver with no request placed is trivially done");
    }

    #[test]
    fn send_request_places_a_request_in_flight() {
        let now = base();
        let mut d = request_driver(transport(), now);
        let sent = d.send_request(&get(b"/a")).unwrap();
        assert_eq!(sent.stream_id, 0, "the first client bidi stream is 0");
        assert_eq!(d.in_flight(), 1);
        assert!(!d.is_done());
    }

    #[test]
    fn into_parts_returns_the_turn_and_collected_responses() {
        let now = base();
        let mut d = request_driver(transport(), now);
        d.send_request(&get(b"/a")).unwrap();
        let (turn, responses) = d.into_parts();
        assert_eq!(turn.pump().active_count(), 1);
        assert!(responses.is_empty());
    }

    // ---- transmit ------------------------------------------------------

    #[test]
    fn transmit_puts_the_request_on_the_wire() {
        let now = base();
        let mut transport = transport();
        transport.push_inbound(one_rtt_packet(0, &[response_stream(0, b"200", b"ok")]));
        let mut d = request_driver(transport, now);
        d.send_request(&get(b"/a")).unwrap();
        d.transmit(now).unwrap();
        // The request stream is fully written after one transmit; a second stages
        // nothing new.
        assert!(d.turn().pump().request_flushed(0));
    }

    // ---- poll: datagram round trip -------------------------------------

    #[test]
    fn poll_ingests_a_response_and_completes_the_request() {
        let now = base();
        let mut transport = transport();
        transport.push_inbound(one_rtt_packet(0, &[response_stream(0, b"200", b"pong")]));
        let mut d = request_driver(transport, now);
        let stream = d.send_request(&post(b"/echo", b"ping")).unwrap().stream_id;
        assert_eq!(stream, 0);
        d.transmit(now).unwrap();

        let poll = d.poll(now).unwrap();
        match poll {
            RequestPoll::Ingested { packets, responses, acks_queued } => {
                assert_eq!(packets, 1);
                assert_eq!(responses, 1);
                assert_eq!(acks_queued, 1, "a 1-RTT response owes an ACK");
            }
            other => panic!("expected an ingested datagram, got {other:?}"),
        }
        assert_eq!(d.responses().len(), 1);
        assert_eq!(d.responses()[0].status, 200);
        assert_eq!(d.responses()[0].body, b"pong");
        assert!(d.is_done(), "the response retired the only in-flight request");
    }

    #[test]
    fn poll_on_an_empty_transport_dispatches_timers() {
        let now = base();
        // No inbound datagram: the wait times out and the driver dispatches timers.
        let mut d = request_driver(transport(), now);
        d.send_request(&get(b"/a")).unwrap();
        let poll = d.poll(now).unwrap();
        assert!(matches!(poll, RequestPoll::Timers(_)), "empty transport wakes on the timer");
    }

    // ---- run: drive to completion --------------------------------------

    #[test]
    fn run_drives_a_request_to_completion() {
        let now = base();
        let mut transport = transport();
        transport.push_inbound(one_rtt_packet(0, &[response_stream(0, b"200", b"body")]));
        let mut d = request_driver(transport, now);
        d.send_request(&get(b"/index")).unwrap();

        let outcome = d.run(|| now, 8).expect("run drives the request");
        assert_eq!(outcome, RequestOutcome::Completed);
        assert_eq!(d.responses().len(), 1);
        assert_eq!(d.responses()[0].status, 200);
        assert_eq!(d.responses()[0].body, b"body");
    }

    #[test]
    fn run_returns_completed_when_nothing_is_in_flight() {
        let now = base();
        let mut d = request_driver(transport(), now);
        // Done is checked before the first turn, so no I/O happens.
        let outcome = d.run(|| now, 4).expect("run over an idle driver");
        assert_eq!(outcome, RequestOutcome::Completed);
        assert!(d.responses().is_empty());
    }

    #[test]
    fn run_reports_incomplete_when_the_peer_never_answers() {
        let now = base();
        // Empty transport: the request goes out but no response ever arrives, so the
        // budget is spent with the request still in flight.
        let mut d = request_driver(transport(), now);
        d.send_request(&get(b"/never")).unwrap();
        let outcome = d.run(|| now, 3).expect("run exhausts its budget");
        assert_eq!(outcome, RequestOutcome::Incomplete);
        assert!(d.responses().is_empty());
        assert_eq!(d.in_flight(), 1);
    }

    #[test]
    fn run_collects_two_responses_from_two_streams() {
        let now = base();
        let mut transport = transport();
        // Two responses on two request streams (0 and 4), coalesced into one datagram
        // (RFC 9000 §12.2 permits many frames per packet).
        transport.push_inbound(one_rtt_packet(
            0,
            &[response_stream(0, b"200", b"first"), response_stream(4, b"404", b"second")],
        ));
        let mut d = request_driver(transport, now);
        let s0 = d.send_request(&get(b"/a")).unwrap().stream_id;
        let s4 = d.send_request(&get(b"/b")).unwrap().stream_id;
        assert_eq!((s0, s4), (0, 4));

        let outcome = d.run(|| now, 8).expect("run drives both requests");
        assert_eq!(outcome, RequestOutcome::Completed);
        assert_eq!(d.responses().len(), 2);
        let statuses: Vec<u16> = d.responses().iter().map(|r| r.status).collect();
        assert!(statuses.contains(&200) && statuses.contains(&404), "both responses: {statuses:?}");
    }

    #[test]
    fn take_responses_drains_the_driver() {
        let now = base();
        let mut transport = transport();
        transport.push_inbound(one_rtt_packet(0, &[response_stream(0, b"200", b"x")]));
        let mut d = request_driver(transport, now);
        d.send_request(&get(b"/a")).unwrap();
        d.run(|| now, 8).unwrap();
        let taken = d.take_responses();
        assert_eq!(taken.len(), 1);
        assert!(d.responses().is_empty(), "take leaves the driver empty");
    }

    // ---- poll: authenticated connection error --------------------------

    #[test]
    fn poll_surfaces_a_bad_frame_as_an_ingest_error() {
        let now = base();
        let mut transport = transport();
        // A STREAM frame carrying response data on stream 8 — a client-initiated bidi
        // stream we never opened (only stream 0 has an in-flight request). The QUIC
        // layer decrypts and defers it, but the request pump has no exchange for it
        // and rejects the routing (RFC 9114 §4.1), surfacing as an Ingest error.
        transport.push_inbound(one_rtt_packet(0, &[response_stream(8, b"200", b"stray")]));
        let mut d = request_driver(transport, now);
        d.send_request(&get(b"/a")).unwrap();
        d.transmit(now).unwrap();
        let err = d.poll(now).unwrap_err();
        assert!(matches!(err, RequestDriverError::Ingest(_)), "unexpected error: {err:?}");
    }
}
