//! HTTP/3 request turn — joining the request pump to a live QUIC connection turn
//! (RFC 9000 §12.4, §19.8; RFC 9114 §4.1, §6.1).
//!
//! [`request_pump`](super::request_pump) left the HTTP/3 request tower as a pure
//! state machine speaking in *frames*: [`RequestPump::poll_transmit`](super::request_pump::RequestPump::poll_transmit)
//! hands out the [`Frame::Stream`](super::quic_frame::Frame::Stream) frames a set of
//! in-flight requests owes, and [`RequestPump::on_frame`](super::request_pump::RequestPump::on_frame)
//! digests one inbound per-stream frame into a [`PumpEvent`]. Around it,
//! [`conn_turn`](super::conn_turn) assembled a running QUIC connection into a
//! [`ConnectionTurn`]: the receive-side driver ([`driver::ConnectionDriver`](super::driver::ConnectionDriver),
//! whose [`ingest`](super::driver::ConnectionDriver::ingest) surfaces the per-stream
//! frames a datagram deferred) joined to the send-side
//! [`ConnectionSendState`](super::send_state::ConnectionSendState) (whose
//! [`enqueue`](super::send_state::ConnectionSendState::enqueue) queues a frame and
//! [`flush`](super::send_state::ConnectionSendState::flush) drains the queue onto the
//! wire).
//!
//! Neither slice knows about the other: the pump produces and consumes frames but owns
//! no connection, and the connection turn moves frames but knows nothing of requests.
//! [`RequestTurn`] is the seam between them — the `h3_do_request` dispatch this task
//! has been building toward — owning both halves and wiring the pump's two frame ports
//! to the connection's:
//!
//! ## Sending — [`RequestTurn::stage_requests`] + [`RequestTurn::flush`]
//!
//! [`RequestTurn::stage_requests`] drains [`RequestPump::poll_transmit`](super::request_pump::RequestPump::poll_transmit)
//! (segmenting each request's stream data by [`RequestTurn::max_request_frame_len`])
//! and [`enqueue`](super::send_state::ConnectionSendState::enqueue)s every produced
//! STREAM frame into the connection's Application Data send queue (STREAM frames are
//! 1-RTT only, RFC 9000 §12.5). [`RequestTurn::flush`] then drains the whole send
//! queue — those STREAM frames alongside any ACKs or probes the connection layer
//! queued — onto the transport. [`RequestTurn::transmit`] is the two in sequence.
//!
//! ## Receiving — [`RequestTurn::route_deferred`] + [`RequestTurn::ingest`]
//!
//! An [`ingest`](super::driver::ConnectionDriver::ingest) hands back the frames the
//! datagram deferred (RFC 9000 §13): the per-stream frames the request layer owns
//! (STREAM, RESET_STREAM, STOP_SENDING, MAX_STREAM_DATA, STREAM_DATA_BLOCKED) mixed
//! with the connection-layer ones it does not (ACK, NEW_TOKEN).
//! [`RequestTurn::route_deferred`] routes each per-stream frame through
//! [`RequestPump::on_frame`](super::request_pump::RequestPump::on_frame), collecting
//! the [`PumpEvent`]s (a completed [`H3Response`], a progress step, an abort), and
//! sets the rest aside as [`RequestIngest::residual`] so the connection layer still
//! processes its ACKs — nothing is silently swallowed. [`RequestTurn::ingest`] is the
//! datagram ingest and the routing together, returning the raw
//! [`IngestReport`](super::recv_path::IngestReport) too, so the caller keeps the
//! packet counts and the non-deferred effects (PATH_RESPONSE, RETIRE_CONNECTION_ID)
//! the connection layer owns.
//!
//! Like every slice below it, [`RequestTurn`] reads no clock of its own — every `now`
//! is caller-supplied — and its only I/O is the transport the connection turn writes
//! over, mockable through [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport).
//! A synthetic clock and a scripted transport drive a whole request round trip
//! deterministically.

use std::time::Instant;

use super::conn_turn::ConnectionTurn;
use super::h3_exchange::H3Response;
use super::loss::PacketNumberSpace;
use super::quic_frame::Frame;
use super::recv_path::{IngestError, IngestReport};
use super::request_dispatch::{DispatchError, SentRequest};
use super::request_exchange::ClientRequest;
use super::request_pump::{PumpEvent, RequestPump};
use super::send_path::{FlushError, FlushReport};
use super::send_state::SendStateError;
use super::udp::DatagramTransport;

/// The byte overhead a STREAM frame and its carrying 1-RTT packet add on top of the
/// stream data, reserved off the datagram MTU when deriving a default per-frame stream
/// budget ([`RequestTurn::with_default_frame_len`]).
///
/// A short-header packet header (first byte + Destination Connection ID + packet
/// number) is at most ~25 bytes, a STREAM frame header (type + stream-id + offset +
/// length varints) at most ~25, and the AEAD tag 16 (RFC 9000 §17.3, §19.8; RFC 9001
/// §5.3). 96 is a generous round figure so a staged STREAM frame always fits one
/// packet payload of a `max_datagram_len`-byte datagram.
pub const STREAM_FRAME_MTU_OVERHEAD: usize = 96;

/// Something that stopped a request turn from staging or transmitting a request, or
/// from ingesting a datagram and routing its frames.
///
/// The two send-side variants come from the connection's send state
/// ([`RequestTurn::stage_requests`], [`RequestTurn::transmit`], [`RequestTurn::flush`]);
/// the two receive-side variants from ingesting a datagram and routing its per-stream
/// frames ([`RequestTurn::ingest`]).
#[derive(Debug)]
pub enum RequestTurnError {
    /// A staged STREAM frame could not be queued into the Application Data scheduler
    /// (the space is not installed, or the frame overflowed the payload budget —
    /// tighten [`RequestTurn::max_request_frame_len`]). Carries the underlying
    /// [`SendStateError`].
    Enqueue(SendStateError),
    /// The connection's send-path flush failed: a packet could not be built or a
    /// datagram write failed. Carries the underlying [`FlushError`].
    Flush(FlushError),
    /// Ingesting the datagram surfaced an authenticated connection error (a malformed
    /// or barred frame, a connection-level violation). Carries the underlying
    /// [`IngestError`]; the caller closes the connection with its
    /// [`code`](super::recv_path::IngestError::code).
    Ingest(IngestError),
    /// Routing an inbound per-stream frame through the request pump failed: it
    /// breached QUIC flow control or the final-size rules, or targeted a stream with
    /// no in-flight request. Carries the underlying [`DispatchError`].
    Route(DispatchError),
}

impl core::fmt::Display for RequestTurnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Enqueue(e) => write!(f, "HTTP/3 request turn: staging a request: {e}"),
            Self::Flush(e) => write!(f, "HTTP/3 request turn: flushing: {e}"),
            Self::Ingest(e) => write!(f, "HTTP/3 request turn: ingesting a datagram: {e}"),
            Self::Route(e) => write!(f, "HTTP/3 request turn: routing a frame: {e}"),
        }
    }
}

impl std::error::Error for RequestTurnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Enqueue(e) => Some(e),
            Self::Flush(e) => Some(e),
            Self::Ingest(e) => Some(e),
            Self::Route(e) => Some(e),
        }
    }
}

impl From<SendStateError> for RequestTurnError {
    fn from(e: SendStateError) -> Self {
        Self::Enqueue(e)
    }
}

impl From<FlushError> for RequestTurnError {
    fn from(e: FlushError) -> Self {
        Self::Flush(e)
    }
}

impl From<IngestError> for RequestTurnError {
    fn from(e: IngestError) -> Self {
        Self::Ingest(e)
    }
}

impl From<DispatchError> for RequestTurnError {
    fn from(e: DispatchError) -> Self {
        Self::Route(e)
    }
}

/// The result of routing the per-stream frames a datagram deferred through the request
/// pump ([`RequestTurn::route_deferred`], [`RequestTurn::ingest`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestIngest {
    /// The outcome of routing each per-stream frame through the pump, in arrival
    /// order (a completed [`H3Response`], a progress step, an abort, a STOP_SENDING
    /// reply).
    pub events: Vec<PumpEvent>,
    /// The deferred frames the request layer does not own (ACK, NEW_TOKEN), for the
    /// connection layer to process — kept so nothing is silently dropped.
    pub residual: Vec<Frame>,
}

impl RequestIngest {
    /// Extracts every completed [`H3Response`] from the routed events, consuming the
    /// ingest.
    ///
    /// A convenience for callers that only want the responses this ingest completed;
    /// the other events (progress, abort, stop-sending) are dropped.
    #[must_use]
    pub fn into_responses(self) -> Vec<H3Response> {
        self.events
            .into_iter()
            .filter_map(|e| match e {
                PumpEvent::Response(resp) => Some(resp),
                _ => None,
            })
            .collect()
    }
}

/// Whether `frame` is one of the five per-stream frames the request pump routes
/// (RFC 9000 §19.4, §19.5, §19.8, §19.10, §19.13), as opposed to a connection-layer
/// frame the request turn sets aside as residual.
fn is_request_frame(frame: &Frame) -> bool {
    matches!(
        frame,
        Frame::Stream { .. }
            | Frame::ResetStream { .. }
            | Frame::StopSending { .. }
            | Frame::MaxStreamData { .. }
            | Frame::StreamDataBlocked { .. }
    )
}

/// The `h3_do_request` seam: a [`RequestPump`] wired to a live [`ConnectionTurn`].
///
/// Owns both halves and moves frames between them — outbound, it stages the pump's
/// request STREAM frames into the connection's send queue and flushes
/// ([`RequestTurn::stage_requests`], [`RequestTurn::flush`], [`RequestTurn::transmit`]);
/// inbound, it routes the per-stream frames a datagram deferred through the pump
/// ([`RequestTurn::ingest`], [`RequestTurn::route_deferred`]). New requests are placed
/// with [`RequestTurn::send_request`].
#[derive(Debug)]
pub struct RequestTurn<T: DatagramTransport> {
    /// The live QUIC connection turn: receive-side driver joined to send-side state.
    turn: ConnectionTurn<T>,
    /// The HTTP/3 request pump translating requests to and from per-stream frames.
    pump: RequestPump,
    /// The maximum stream-data bytes one staged STREAM frame carries — sized so the
    /// frame fits one packet payload of a `max_datagram_len`-byte datagram.
    max_request_frame_len: usize,
}

impl<T: DatagramTransport> RequestTurn<T> {
    /// Joins a live `turn` and a request `pump`, segmenting each staged STREAM frame's
    /// stream data at `max_request_frame_len` bytes.
    ///
    /// `max_request_frame_len` must leave room for the STREAM frame and packet headers
    /// within the connection's datagram MTU (see [`STREAM_FRAME_MTU_OVERHEAD`]);
    /// [`RequestTurn::with_default_frame_len`] derives a safe value from the turn's MTU.
    #[must_use]
    pub fn new(turn: ConnectionTurn<T>, pump: RequestPump, max_request_frame_len: usize) -> Self {
        Self { turn, pump, max_request_frame_len }
    }

    /// Joins a live `turn` and a request `pump`, deriving the per-frame stream budget
    /// from the turn's datagram MTU minus [`STREAM_FRAME_MTU_OVERHEAD`].
    #[must_use]
    pub fn with_default_frame_len(turn: ConnectionTurn<T>, pump: RequestPump) -> Self {
        let max_request_frame_len = turn
            .max_datagram_len()
            .saturating_sub(STREAM_FRAME_MTU_OVERHEAD)
            .max(1);
        Self::new(turn, pump, max_request_frame_len)
    }

    /// The live connection turn, borrowed immutably (e.g. to read the lifecycle, or
    /// wait for the next event through its driver).
    #[must_use]
    pub fn turn(&self) -> &ConnectionTurn<T> {
        &self.turn
    }

    /// The live connection turn, borrowed mutably: waiting for events, dispatching
    /// timers, and installing keys all go through it.
    pub fn turn_mut(&mut self) -> &mut ConnectionTurn<T> {
        &mut self.turn
    }

    /// The request pump, borrowed immutably (e.g. to count in-flight requests).
    #[must_use]
    pub fn pump(&self) -> &RequestPump {
        &self.pump
    }

    /// The request pump, borrowed mutably.
    pub fn pump_mut(&mut self) -> &mut RequestPump {
        &mut self.pump
    }

    /// The maximum stream-data bytes one staged STREAM frame carries.
    #[must_use]
    pub fn max_request_frame_len(&self) -> usize {
        self.max_request_frame_len
    }

    /// Sets the per-frame stream budget (see [`RequestTurn::new`]).
    pub fn set_max_request_frame_len(&mut self, max_request_frame_len: usize) {
        self.max_request_frame_len = max_request_frame_len;
    }

    /// Splits the request turn back into its connection turn and request pump.
    #[must_use]
    pub fn into_parts(self) -> (ConnectionTurn<T>, RequestPump) {
        (self.turn, self.pump)
    }

    /// Places `req` onto a fresh client-initiated bidirectional stream, finishing its
    /// send half (STREAM FIN, RFC 9114 §4.1) — see
    /// [`RequestPump::send_request`](super::request_pump::RequestPump::send_request).
    /// The rendered bytes wait on the pump's send half until the next
    /// [`RequestTurn::stage_requests`] moves them into the connection's send queue.
    ///
    /// # Errors
    ///
    /// [`DispatchError::Open`] if the request cannot be built (RFC 9114 §4.2/§7.2.1)
    /// or all client bidirectional stream identifiers are spent (RFC 9000 §2.1).
    pub fn send_request(&mut self, req: &ClientRequest) -> Result<SentRequest, DispatchError> {
        self.pump.send_request(req)
    }

    /// Drains the pump's request send streams
    /// ([`RequestPump::poll_transmit`](super::request_pump::RequestPump::poll_transmit))
    /// and enqueues every produced STREAM frame into the connection's Application Data
    /// send queue, returning how many frames were staged.
    ///
    /// STREAM frames are 1-RTT only (RFC 9000 §12.5), so the Application Data space
    /// must have its send keys installed — which the handshake does before any request
    /// flows. The staged frames wait in the send queue until [`RequestTurn::flush`]
    /// drains them onto the wire.
    ///
    /// # Errors
    ///
    /// [`RequestTurnError::Enqueue`] if the Application Data space is not installed or
    /// a staged frame overflowed the payload budget (tighten
    /// [`RequestTurn::max_request_frame_len`]). Frames staged before the failure stay
    /// queued.
    pub fn stage_requests(&mut self) -> Result<usize, RequestTurnError> {
        let frames = self.pump.poll_transmit(self.max_request_frame_len);
        let staged = frames.len();
        for frame in frames {
            self.turn
                .send_mut()
                .enqueue(PacketNumberSpace::ApplicationData, frame)?;
        }
        Ok(staged)
    }

    /// Flushes every queued frame — staged request STREAM frames alongside any ACKs or
    /// probes the connection layer queued — onto the transport as coalesced datagrams
    /// (RFC 9000 §12.2), a pass-through to
    /// [`ConnectionTurn::flush`](super::conn_turn::ConnectionTurn::flush).
    ///
    /// # Errors
    ///
    /// [`FlushError`] if the send engine could not build a packet or a datagram write
    /// failed.
    pub fn flush(&mut self, now: Instant) -> Result<FlushReport, FlushError> {
        self.turn.flush(now)
    }

    /// Stages the pump's request STREAM frames ([`RequestTurn::stage_requests`]) and
    /// flushes the send queue ([`RequestTurn::flush`]) in one call.
    ///
    /// # Errors
    ///
    /// [`RequestTurnError::Enqueue`] if a frame could not be staged, or
    /// [`RequestTurnError::Flush`] if the flush failed.
    pub fn transmit(&mut self, now: Instant) -> Result<FlushReport, RequestTurnError> {
        self.stage_requests()?;
        Ok(self.flush(now)?)
    }

    /// Routes the per-stream frames among `deferred` through the request pump
    /// ([`RequestPump::on_frame`](super::request_pump::RequestPump::on_frame)),
    /// collecting the [`PumpEvent`]s and setting the connection-layer frames (ACK,
    /// NEW_TOKEN) aside as [`RequestIngest::residual`].
    ///
    /// `deferred` is the [`effects.deferred`](super::connection::PacketEffects::deferred)
    /// an ingest surfaced. The frames are routed in order; the first that errors stops
    /// the routing.
    ///
    /// # Errors
    ///
    /// [`DispatchError`] if a per-stream frame breached QUIC flow control or the
    /// final-size rules (RFC 9000 §4.1, §4.5), or a STREAM frame targeted a stream with
    /// no in-flight request or completed a malformed response (RFC 9114 §4.1). Events
    /// collected before the failure are discarded.
    pub fn route_deferred(&mut self, deferred: &[Frame]) -> Result<RequestIngest, DispatchError> {
        let mut events = Vec::new();
        let mut residual = Vec::new();
        for frame in deferred {
            if is_request_frame(frame) {
                events.push(self.pump.on_frame(frame)?);
            } else {
                residual.push(frame.clone());
            }
        }
        Ok(RequestIngest { events, residual })
    }

    /// Ingests one datagram woken on the connection's driver
    /// ([`ConnectionDriver::ingest`](super::driver::ConnectionDriver::ingest)) and
    /// routes its deferred per-stream frames through the pump
    /// ([`RequestTurn::route_deferred`]).
    ///
    /// Returns the raw [`IngestReport`] — so the caller keeps the packet counts and the
    /// non-deferred effects the connection layer owns (PATH_RESPONSE and
    /// RETIRE_CONNECTION_ID to schedule, the owed-ACK signal) — paired with the
    /// [`RequestIngest`] the routing produced.
    ///
    /// `n` is the byte count the [`Wakeup::Datagram`](super::event_loop::Wakeup::Datagram)
    /// reported; the datagram bytes come from the driver's receive buffer.
    ///
    /// # Errors
    ///
    /// [`RequestTurnError::Ingest`] if the datagram carried an authenticated connection
    /// error, or [`RequestTurnError::Route`] if routing a per-stream frame failed.
    pub fn ingest(
        &mut self,
        n: usize,
        now: Instant,
    ) -> Result<(IngestReport, RequestIngest), RequestTurnError> {
        let report = self.turn.driver_mut().ingest(n, now)?;
        let routed = self.route_deferred(&report.effects.deferred)?;
        Ok((report, routed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::conn_turn::DEFAULT_ACK_DELAY_EXPONENT;
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::ConnectionDriver;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::frame::Frame as H3Frame;
    use crate::h3::h3_request::H3Profile;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::pto::LossDetection;
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::quic_frame;
    use crate::h3::recv_path::RecvKeyRing;
    use crate::h3::request_mux::MuxError;
    use crate::h3::send_state::ConnectionSendState;
    use crate::h3::stream::SendState;
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

    /// A connection turn with the given spaces installed on the send half. The tests
    /// that stage STREAM frames install Application Data (STREAM is 1-RTT only); the
    /// send keys reuse the Initial client keys — the routing and framing under test do
    /// not depend on the actual key material.
    fn conn_turn(
        spaces: &[PacketNumberSpace],
        now: Instant,
    ) -> ConnectionTurn<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), vec![0x11, 0x22, 0x33, 0x44], 1200);
        for &space in spaces {
            send.install(space, keys().client);
        }
        ConnectionTurn::new(driver(now), send, 1200, DEFAULT_ACK_DELAY_EXPONENT)
    }

    /// A permissive stream config: generous windows so flow control never interferes
    /// unless a test tightens it.
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

    /// A request turn with Application Data installed on the send half (so STREAM
    /// frames can be staged) and a fresh request pump.
    fn request_turn(now: Instant) -> RequestTurn<MockDatagramTransport> {
        RequestTurn::with_default_frame_len(
            conn_turn(&[PacketNumberSpace::ApplicationData], now),
            pump(),
        )
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

    /// Encode the response-stream bytes for a `code`/`body` response.
    fn response_bytes(code: &[u8], body: &[u8]) -> Vec<u8> {
        let block = qpack::encode_field_section(&[status(code)], true);
        let mut out = Vec::new();
        H3Frame::Headers(block).encode(&mut out).unwrap();
        H3Frame::Data(body.to_vec()).encode(&mut out).unwrap();
        out
    }

    /// Encrypt one Initial packet carrying `frames` with packet number `pn`, for the
    /// combined-ingest tests (Initial keys decrypt the frames the connection defers).
    fn initial_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    // ---- construction / accessors --------------------------------------

    #[test]
    fn new_exposes_both_halves() {
        let now = base();
        let rt = request_turn(now);
        assert_eq!(rt.pump().active_count(), 0);
        assert!(rt.turn().send().is_installed(PacketNumberSpace::ApplicationData));
    }

    #[test]
    fn with_default_frame_len_reserves_mtu_overhead() {
        let now = base();
        let rt = request_turn(now);
        assert_eq!(rt.max_request_frame_len(), 1200 - STREAM_FRAME_MTU_OVERHEAD);
    }

    #[test]
    fn set_max_request_frame_len_updates_the_budget() {
        let now = base();
        let mut rt = request_turn(now);
        rt.set_max_request_frame_len(256);
        assert_eq!(rt.max_request_frame_len(), 256);
    }

    #[test]
    fn into_parts_returns_both_halves() {
        let now = base();
        let mut rt = request_turn(now);
        rt.send_request(&get(b"/a")).unwrap();
        let (_turn, pump) = rt.into_parts();
        assert_eq!(pump.active_count(), 1, "the sent request survives the split");
    }

    // ---- send_request + stage_requests ---------------------------------

    #[test]
    fn send_request_places_a_request_in_the_pump() {
        let now = base();
        let mut rt = request_turn(now);
        let s = rt.send_request(&get(b"/index.html")).unwrap().stream_id;
        assert_eq!(s, 0, "the first client bidi stream is 0");
        assert!(rt.pump().is_active(s));
    }

    #[test]
    fn stage_requests_enqueues_stream_frames_into_app_data() {
        let now = base();
        let mut rt = request_turn(now);
        rt.send_request(&get(b"/a")).unwrap();
        // Nothing queued in the connection until the request is staged.
        assert!(!rt.turn().send().pending_in(PacketNumberSpace::ApplicationData));
        let staged = rt.stage_requests().unwrap();
        assert_eq!(staged, 1, "the whole GET is one STREAM frame");
        assert!(rt.turn().send().pending_in(PacketNumberSpace::ApplicationData));
    }

    #[test]
    fn stage_requests_marks_the_send_half_flushed() {
        let now = base();
        let mut rt = request_turn(now);
        let s = rt.send_request(&get(b"/a")).unwrap().stream_id;
        assert!(!rt.pump().request_flushed(s));
        rt.stage_requests().unwrap();
        assert!(rt.pump().request_flushed(s));
    }

    #[test]
    fn stage_requests_segments_a_large_body_by_the_frame_budget() {
        let now = base();
        let mut rt = request_turn(now);
        rt.set_max_request_frame_len(64);
        rt.send_request(&post(b"/upload", &[b'x'; 200])).unwrap();
        let staged = rt.stage_requests().unwrap();
        assert!(staged > 1, "a 200-byte body needs several 64-byte STREAM frames");
    }

    #[test]
    fn stage_requests_without_app_data_installed_errors() {
        let now = base();
        // No Application Data space on the send half — STREAM frames cannot be queued.
        let mut rt =
            RequestTurn::with_default_frame_len(conn_turn(&[PacketNumberSpace::Initial], now), pump());
        rt.send_request(&get(b"/a")).unwrap();
        let err = rt.stage_requests().unwrap_err();
        assert!(
            matches!(
                err,
                RequestTurnError::Enqueue(SendStateError::SpaceNotInstalled(
                    PacketNumberSpace::ApplicationData
                ))
            ),
            "expected a SpaceNotInstalled enqueue error, got {err:?}"
        );
    }

    #[test]
    fn stage_requests_is_empty_with_no_requests() {
        let now = base();
        let mut rt = request_turn(now);
        assert_eq!(rt.stage_requests().unwrap(), 0);
        assert!(!rt.turn().send().pending_in(PacketNumberSpace::ApplicationData));
    }

    // ---- transmit: stage + flush over the wire -------------------------

    #[test]
    fn transmit_writes_a_datagram_carrying_the_request() {
        let now = base();
        let mut rt = request_turn(now);
        rt.send_request(&get(b"/a")).unwrap();
        let report = rt.transmit(now).expect("transmit succeeds");
        assert_eq!(report.datagrams_sent, 1, "one datagram written: {report:?}");
        assert_eq!(
            rt.turn_mut().driver_mut().events_mut().transport_mut().sent.len(),
            1
        );
    }

    #[test]
    fn transmit_leaves_the_send_half_data_sent() {
        let now = base();
        let mut rt = request_turn(now);
        let s = rt.send_request(&get(b"/a")).unwrap().stream_id;
        rt.transmit(now).unwrap();
        assert_eq!(
            rt.pump().dispatch().streams().send_stream(s).unwrap().state(),
            SendState::DataSent
        );
    }

    #[test]
    fn transmit_with_no_requests_writes_nothing() {
        let now = base();
        let mut rt = request_turn(now);
        let report = rt.transmit(now).expect("empty transmit succeeds");
        assert_eq!(report.datagrams_sent, 0);
    }

    // ---- route_deferred: inbound per-stream frames ---------------------

    #[test]
    fn route_deferred_completes_a_response() {
        let now = base();
        let mut rt = request_turn(now);
        let s = rt.send_request(&get(b"/a")).unwrap().stream_id;
        let frame = Frame::Stream {
            stream_id: s,
            offset: 0,
            fin: true,
            data: response_bytes(b"200", b"hello"),
        };
        let ingest = rt.route_deferred(&[frame]).unwrap();
        assert_eq!(ingest.events.len(), 1);
        match &ingest.events[0] {
            PumpEvent::Response(resp) => {
                assert_eq!(resp.status, 200);
                assert_eq!(resp.body, b"hello");
            }
            other => panic!("expected a Response, got {other:?}"),
        }
        assert!(ingest.residual.is_empty());
        assert!(!rt.pump().is_active(s), "a completed request is retired");
    }

    #[test]
    fn route_deferred_sets_non_request_frames_aside_as_residual() {
        let now = base();
        let mut rt = request_turn(now);
        rt.send_request(&get(b"/a")).unwrap();
        // An ACK and a NEW_TOKEN belong to the connection layer, not the request pump.
        let ack = Frame::Ack {
            largest_acked: 0,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: Vec::new(),
            ecn: None,
        };
        let new_token = Frame::NewToken(vec![1, 2, 3]);
        let ingest = rt.route_deferred(&[ack.clone(), new_token.clone()]).unwrap();
        assert!(ingest.events.is_empty(), "no per-stream frames to route");
        assert_eq!(ingest.residual, vec![ack, new_token]);
    }

    #[test]
    fn route_deferred_routes_stream_frames_and_keeps_acks() {
        let now = base();
        let mut rt = request_turn(now);
        let s = rt.send_request(&get(b"/a")).unwrap().stream_id;
        let ack = Frame::Ack {
            largest_acked: 0,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: Vec::new(),
            ecn: None,
        };
        let stream = Frame::Stream {
            stream_id: s,
            offset: 0,
            fin: true,
            data: response_bytes(b"204", b""),
        };
        // A mixed batch: the STREAM completes a response, the ACK is residual.
        let ingest = rt.route_deferred(&[ack.clone(), stream]).unwrap();
        assert_eq!(ingest.events.len(), 1);
        assert!(matches!(ingest.events[0], PumpEvent::Response(_)));
        assert_eq!(ingest.residual, vec![ack]);
    }

    #[test]
    fn route_deferred_reports_progress_before_fin() {
        let now = base();
        let mut rt = request_turn(now);
        let s = rt.send_request(&get(b"/a")).unwrap().stream_id;
        let frame = Frame::Stream {
            stream_id: s,
            offset: 0,
            fin: false,
            data: response_bytes(b"200", b"partial"),
        };
        let ingest = rt.route_deferred(&[frame]).unwrap();
        assert_eq!(ingest.events, vec![PumpEvent::Progress]);
        assert!(rt.pump().is_active(s));
    }

    #[test]
    fn route_deferred_stream_for_unknown_stream_errors() {
        let now = base();
        let mut rt = request_turn(now);
        // No request opened: a STREAM frame targets a stream with no in-flight request.
        let frame = Frame::Stream { stream_id: 0, offset: 0, fin: true, data: Vec::new() };
        let err = rt.route_deferred(&[frame]).unwrap_err();
        assert_eq!(err, DispatchError::Mux(MuxError::UnknownStream(0)));
    }

    #[test]
    fn route_deferred_into_responses_extracts_completed_responses() {
        let now = base();
        let mut rt = request_turn(now);
        let a = rt.send_request(&get(b"/a")).unwrap().stream_id;
        let b = rt.send_request(&get(b"/b")).unwrap().stream_id;
        let frames = vec![
            Frame::Stream { stream_id: a, offset: 0, fin: true, data: response_bytes(b"200", b"A") },
            Frame::Stream { stream_id: b, offset: 0, fin: true, data: response_bytes(b"404", b"B") },
        ];
        let responses = rt.route_deferred(&frames).unwrap().into_responses();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].status, 200);
        assert_eq!(responses[1].status, 404);
    }

    // ---- ingest: datagram + routing together ---------------------------

    #[test]
    fn ingest_routes_a_deferred_ack_as_residual() {
        let now = base();
        let mut transport = transport();
        // An Initial packet carrying an ACK: the connection defers the ACK, and the
        // request turn routes it — an ACK is not a per-stream frame, so it lands in
        // residual with no pump events.
        let ack = Frame::Ack {
            largest_acked: 0,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: Vec::new(),
            ecn: None,
        };
        transport.push_inbound(initial_packet(0, &[ack, Frame::Padding(24)]));

        let mut rt = RequestTurn::with_default_frame_len(
            ConnectionTurn::new(
                {
                    let mut recv_keys = RecvKeyRing::new();
                    recv_keys.install(PacketNumberSpace::Initial, keys().client);
                    ConnectionDriver::new(
                        DatagramEventLoop::new(transport),
                        connection(now),
                        LossDetection::new(Duration::from_millis(25)),
                        recv_keys,
                        4,
                    )
                },
                {
                    let mut send =
                        ConnectionSendState::new(1, dcid(), vec![0x11, 0x22, 0x33, 0x44], 1200);
                    send.install(PacketNumberSpace::ApplicationData, keys().client);
                    send
                },
                1200,
                DEFAULT_ACK_DELAY_EXPONENT,
            ),
            pump(),
        );

        let n = match rt.turn_mut().driver_mut().wait(now).unwrap() {
            crate::h3::event_loop::Wakeup::Datagram(n) => n,
            other => panic!("expected a datagram wake, got {other:?}"),
        };
        let (report, ingest) = rt.ingest(n, now).unwrap();
        assert_eq!(report.packets_processed, 1);
        assert!(ingest.events.is_empty(), "an ACK produces no request events");
        assert_eq!(ingest.residual.len(), 1, "the ACK is residual: {:?}", ingest.residual);
        assert!(matches!(ingest.residual[0], Frame::Ack { .. }));
    }

    #[test]
    fn ingest_surfaces_an_authenticated_connection_error() {
        let now = base();
        let mut transport = transport();
        // HANDSHAKE_DONE is 1-RTT only; in an Initial it is a PROTOCOL_VIOLATION.
        transport.push_inbound(initial_packet(0, &[Frame::HandshakeDone, Frame::Padding(24)]));
        let mut rt = RequestTurn::with_default_frame_len(
            ConnectionTurn::new(
                {
                    let mut recv_keys = RecvKeyRing::new();
                    recv_keys.install(PacketNumberSpace::Initial, keys().client);
                    ConnectionDriver::new(
                        DatagramEventLoop::new(transport),
                        connection(now),
                        LossDetection::new(Duration::from_millis(25)),
                        recv_keys,
                        4,
                    )
                },
                ConnectionSendState::new(1, dcid(), vec![0x11, 0x22, 0x33, 0x44], 1200),
                1200,
                DEFAULT_ACK_DELAY_EXPONENT,
            ),
            pump(),
        );
        let n = match rt.turn_mut().driver_mut().wait(now).unwrap() {
            crate::h3::event_loop::Wakeup::Datagram(n) => n,
            other => panic!("expected a datagram wake, got {other:?}"),
        };
        let err = rt.ingest(n, now).unwrap_err();
        match err {
            RequestTurnError::Ingest(e) => assert_eq!(e.code(), 0x0a),
            other => panic!("expected an Ingest error, got {other:?}"),
        }
    }

    // ---- end-to-end: request out, response in --------------------------

    #[test]
    fn round_trips_a_request_and_response() {
        let now = base();
        let mut rt = request_turn(now);
        let s = rt.send_request(&post(b"/echo", b"ping")).unwrap().stream_id;
        // Send side: the request drains onto the wire.
        let report = rt.transmit(now).unwrap();
        assert_eq!(report.datagrams_sent, 1);
        // Receive side: the response arrives on the same stream and completes.
        let frame = Frame::Stream {
            stream_id: s,
            offset: 0,
            fin: true,
            data: response_bytes(b"200", b"pong"),
        };
        let responses = rt.route_deferred(&[frame]).unwrap().into_responses();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].body, b"pong");
        assert_eq!(rt.pump().active_count(), 0);
    }
}
