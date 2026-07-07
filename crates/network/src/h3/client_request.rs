//! HTTP/3 client request dispatch (RFC 9114 §3.3, §4.1, §6.1): the `h3_do_request`
//! orchestrator that ties the connect phase and the request phase into one call.
//!
//! Every slice below this one is a piece of the client with a join it deliberately
//! leaves to a caller:
//!
//! - [`client_bootstrap::connect_client`](super::client_bootstrap::connect_client)
//!   assembles a ready-to-drive [`ConnectDriver`] from a `(transport, server name,
//!   trust store)` triple.
//! - [`ConnectDriver::connect`](super::conn_connect::ConnectDriver::connect) drives
//!   the opening handshake to confirmation, authenticating the server certificate
//!   before the client Finished goes out.
//! - [`ConnectDriver::into_request_driver`](super::conn_connect::ConnectDriver::into_request_driver)
//!   splices the confirmed connection into a [`RequestDriver`], wiring a
//!   [`RequestPump`] to the connection turn.
//! - [`RequestDriver::run`](super::request_driver::RequestDriver::run) drives placed
//!   requests to completion, accumulating their [`H3Response`]s.
//!
//! This slice is the caller that composes them. [`fetch`] is the request-phase
//! convenience — place one request on a confirmed [`RequestDriver`] and run it to
//! its single [`H3Response`] — and [`connect_and_fetch`] is the whole-client
//! convenience: hand it a fresh [`ConnectDriver`] and a request, and it opens the
//! connection, splices in the request phase, and returns the response.
//!
//! ## What it still defers
//!
//! Like every slice below it, this module is transport-generic
//! ([`udp::DatagramTransport`](super::udp::DatagramTransport)) and reads no clock of
//! its own — [`connect_and_fetch`] takes a `clock` closure it shares across both
//! phases. Building the *real* transport — resolving the authority to an address,
//! opening the [`udp::UdpDatagram`](super::udp::UdpDatagram) socket, and populating
//! the trust store from [`mozilla_roots::mozilla_trust_anchors`](super::mozilla_roots::mozilla_trust_anchors)
//! — and mapping the [`H3Response`] onto the crate's `Response` alongside the H1/H2
//! paths in `lib.rs` (with Alt-Svc dispatch, RFC 9114 §3.3) remains the caller's
//! job. A scripted [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport)
//! and a synthetic clock drive the whole request phase deterministically in tests.

use std::time::Instant;

use super::conn_connect::{ConnectDriver, ConnectError, ConnectOutcome, RequestSpliceError};
use super::conn_turn::TurnEffect;
use super::h3_exchange::{BodySink, H3Response};
use super::request_dispatch::DispatchError;
use super::request_driver::{RequestDriver, RequestDriverError, RequestOutcome};
use super::request_exchange::ClientRequest;
use super::request_pump::RequestPump;
use super::udp::DatagramTransport;

/// Why [`fetch`] could not obtain the response for the request it placed.
#[derive(Debug)]
pub enum FetchError {
    /// The request could not be built or placed on a fresh stream (RFC 9114
    /// §4.2/§7.2.1, RFC 9000 §2.1): a malformed request header or all client
    /// bidirectional stream identifiers spent
    /// ([`RequestDriver::send_request`](super::request_driver::RequestDriver::send_request)).
    Dispatch(DispatchError),
    /// A driver turn failed while running the request to completion — a socket
    /// error, a bad frame, or a rejected send action
    /// ([`RequestDriver::run`](super::request_driver::RequestDriver::run)).
    Driver(RequestDriverError),
    /// A terminal connection effect ended the connection before the response
    /// completed (RFC 9000 §10.1, §10.2): the idle timeout elapsed or the
    /// connection drained. Carries which one.
    Terminated(TurnEffect),
    /// The turn budget was spent with the request still in flight (the peer never
    /// answered within `max_turns`). The caller decides whether to retry.
    Incomplete,
    /// The driver reported completion but produced no response — a defensive guard
    /// that a single placed request always yields exactly one [`H3Response`]; it
    /// should not arise in practice.
    NoResponse,
}

impl core::fmt::Display for FetchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Dispatch(e) => write!(f, "HTTP/3 fetch: placing the request: {e}"),
            Self::Driver(e) => write!(f, "HTTP/3 fetch: {e}"),
            Self::Terminated(effect) => {
                write!(f, "HTTP/3 fetch: connection ended before the response: {effect:?}")
            }
            Self::Incomplete => write!(f, "HTTP/3 fetch: no response within the turn budget"),
            Self::NoResponse => write!(f, "HTTP/3 fetch: the request completed with no response"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Dispatch(e) => Some(e),
            Self::Driver(e) => Some(e),
            Self::Terminated(_) | Self::Incomplete | Self::NoResponse => None,
        }
    }
}

/// Place `req` on `driver`, drive it to completion, and return its single
/// [`H3Response`] (RFC 9114 §6.1): the request-phase half of an `h3_do_request`.
///
/// The `driver` must already be spliced from a confirmed connection
/// ([`ConnectDriver::into_request_driver`](super::conn_connect::ConnectDriver::into_request_driver)),
/// so its 1-RTT keys are installed on both halves. `clock` supplies each turn's
/// wall-clock instant and `max_turns` bounds the loop so a peer that never answers
/// cannot spin it forever ([`RequestDriver::run`]).
///
/// A response that completed is preferred over the run's stop reason: even if a
/// terminal timer fired on the same run, a response already in the driver is
/// returned. Only when no response landed is the stop reason surfaced as the error.
///
/// # Errors
///
/// [`FetchError`] naming the failure: the request could not be placed, a turn
/// failed, the connection ended or the budget was spent before any response
/// arrived, or (defensively) completion produced no response.
pub fn fetch<T: DatagramTransport>(
    driver: &mut RequestDriver<T>,
    req: &ClientRequest,
    clock: impl FnMut() -> Instant,
    max_turns: usize,
) -> Result<H3Response, FetchError> {
    driver.send_request(req).map_err(FetchError::Dispatch)?;
    let outcome = driver.run(clock, max_turns).map_err(FetchError::Driver)?;

    // A completed response wins over the stop reason: `run` returns `Completed` the
    // turn after the response retires the request, but a terminal timer racing the
    // same run must not discard a response already collected.
    let mut responses = driver.take_responses();
    if !responses.is_empty() {
        return Ok(responses.remove(0));
    }
    match outcome {
        RequestOutcome::Completed => Err(FetchError::NoResponse),
        RequestOutcome::Terminated(effect) => Err(FetchError::Terminated(effect)),
        RequestOutcome::Incomplete => Err(FetchError::Incomplete),
    }
}

/// Identical to [`fetch`] but forwards body bytes to `sink` as DATA frames arrive.
///
/// `sink` is passed directly to [`RequestDriver::run_with_sink`]; it fires for
/// every DATA chunk the peer sends, in order, before the final [`H3Response`]
/// is returned.
///
/// # Errors
///
/// Same conditions as [`fetch`].
pub fn fetch_with_sink<'s, T: DatagramTransport>(
    driver: &mut RequestDriver<T>,
    req: &ClientRequest,
    clock: impl FnMut() -> Instant,
    max_turns: usize,
    sink: Option<BodySink<'s>>,
) -> Result<H3Response, FetchError> {
    driver.send_request(req).map_err(FetchError::Dispatch)?;
    let outcome = driver.run_with_sink(clock, max_turns, sink).map_err(FetchError::Driver)?;

    let mut responses = driver.take_responses();
    if !responses.is_empty() {
        return Ok(responses.remove(0));
    }
    match outcome {
        RequestOutcome::Completed => Err(FetchError::NoResponse),
        RequestOutcome::Terminated(effect) => Err(FetchError::Terminated(effect)),
        RequestOutcome::Incomplete => Err(FetchError::Incomplete),
    }
}

/// Why [`connect_and_fetch`] could not complete the request.
#[derive(Debug)]
pub enum ConnectFetchError {
    /// A step of the opening handshake failed — a control-flow error, a TLS error,
    /// or a server certificate that did not authenticate
    /// ([`ConnectError`](super::conn_connect::ConnectError)).
    Connect(ConnectError),
    /// The connect loop stopped without confirming the handshake (RFC 9000 §19.20):
    /// a terminal timer ended the connection ([`ConnectOutcome::Terminated`]) or the
    /// turn budget was spent ([`ConnectOutcome::Incomplete`]) before HANDSHAKE_DONE.
    /// Carries which; the H2 / H1.1 fallback is the caller's decision.
    NotConfirmed(ConnectOutcome),
    /// Splicing the confirmed connection into the request phase was refused
    /// ([`RequestSpliceError`](super::conn_connect::RequestSpliceError)). This should
    /// not arise after a [`ConnectOutcome::Confirmed`], which guarantees the guard
    /// passes; it is surfaced rather than unwrapped.
    Splice(RequestSpliceError),
    /// The handshake confirmed but the request phase failed ([`FetchError`]).
    Fetch(FetchError),
}

impl core::fmt::Display for ConnectFetchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "HTTP/3 request: {e}"),
            Self::NotConfirmed(outcome) => {
                write!(f, "HTTP/3 request: handshake did not confirm: {outcome:?}")
            }
            Self::Splice(e) => write!(f, "HTTP/3 request: {e}"),
            Self::Fetch(e) => write!(f, "HTTP/3 request: {e}"),
        }
    }
}

impl std::error::Error for ConnectFetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect(e) => Some(e),
            Self::Splice(e) => Some(e),
            Self::Fetch(e) => Some(e),
            Self::NotConfirmed(_) => None,
        }
    }
}

/// Open `connect`'s connection, splice in the request phase over `pump`, and fetch
/// `req` — the whole-client `h3_do_request` over a ready [`ConnectDriver`]
/// (RFC 9114 §3.3, §4.1).
///
/// `clock` is read once per turn and shared across both phases; `connect_turns`
/// bounds the handshake loop and `request_turns` the request loop, so neither an
/// unresponsive handshake nor an unanswered request can spin forever.
///
/// The steps mirror the module docs: [`ConnectDriver::connect`] to
/// [`ConnectOutcome::Confirmed`] (any other outcome is
/// [`ConnectFetchError::NotConfirmed`], leaving the H2 / H1.1 fallback to the
/// caller), then [`ConnectDriver::into_request_driver`] to splice the confirmed
/// connection to `pump`, then [`fetch`] for the single response. The server
/// certificate authenticated during the handshake has already gated confirmation,
/// so it is not threaded on.
///
/// # Errors
///
/// [`ConnectFetchError`] naming the phase that failed: the handshake, the splice, or
/// the request.
pub fn connect_and_fetch<T: DatagramTransport>(
    mut connect: ConnectDriver<T>,
    pump: RequestPump,
    req: &ClientRequest,
    mut clock: impl FnMut() -> Instant,
    connect_turns: usize,
    request_turns: usize,
) -> Result<H3Response, ConnectFetchError> {
    match connect
        .connect(&mut clock, connect_turns)
        .map_err(ConnectFetchError::Connect)?
    {
        ConnectOutcome::Confirmed => {}
        other => return Err(ConnectFetchError::NotConfirmed(other)),
    }
    let mut driver = connect
        .into_request_driver(pump)
        .map_err(ConnectFetchError::Splice)?;
    fetch(&mut driver, req, &mut clock, request_turns).map_err(ConnectFetchError::Fetch)
}

/// Identical to [`connect_and_fetch`] but forwards body bytes to `sink` as DATA
/// frames arrive during the request phase.
///
/// `sink` is passed through to [`fetch_with_sink`] and ultimately to
/// [`RequestDriver::run_with_sink`]; it fires for every DATA chunk in order before
/// the final [`H3Response`] is returned.  The connect (handshake) phase is
/// unaffected — no body flows there.
///
/// # Errors
///
/// Same conditions as [`connect_and_fetch`].
pub fn connect_and_fetch_with_sink<'s, T: DatagramTransport>(
    mut connect: ConnectDriver<T>,
    pump: RequestPump,
    req: &ClientRequest,
    mut clock: impl FnMut() -> Instant,
    connect_turns: usize,
    request_turns: usize,
    sink: Option<BodySink<'s>>,
) -> Result<H3Response, ConnectFetchError> {
    match connect
        .connect(&mut clock, connect_turns)
        .map_err(ConnectFetchError::Connect)?
    {
        ConnectOutcome::Confirmed => {}
        other => return Err(ConnectFetchError::NotConfirmed(other)),
    }
    let mut driver = connect
        .into_request_driver(pump)
        .map_err(ConnectFetchError::Splice)?;
    fetch_with_sink(&mut driver, req, &mut clock, request_turns, sink)
        .map_err(ConnectFetchError::Fetch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::client_bootstrap::{ClientConnectConfig, connect_client};
    use crate::h3::conn_turn::{ConnectionTurn, DEFAULT_ACK_DELAY_EXPONENT};
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::ConnectionDriver;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::frame::Frame as H3Frame;
    use crate::h3::h3_request::H3Profile;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::loss::PacketNumberSpace;
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::pto::LossDetection;
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::recv_path::RecvKeyRing;
    use crate::h3::request_turn::RequestTurn;
    use crate::h3::send_state::ConnectionSendState;
    use crate::h3::stream_manager::StreamManagerConfig;
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::{Duration, Instant};

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

    /// The four-byte local connection ID the request driver is addressed by.
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

    /// A connection driver over `t` with Application-Data receive keys installed (so
    /// a scripted 1-RTT response decrypts) and a four-byte local CID.
    fn driver(t: MockDatagramTransport, now: Instant) -> ConnectionDriver<MockDatagramTransport> {
        let mut recv_keys = RecvKeyRing::new();
        recv_keys.install(PacketNumberSpace::ApplicationData, keys().client);
        ConnectionDriver::new(
            DatagramEventLoop::new(t),
            connection(now),
            LossDetection::new(Duration::from_millis(25)),
            recv_keys,
            4,
        )
    }

    /// A connection turn over `t` with Application-Data installed on both directions
    /// — the state a completed handshake leaves behind, so STREAM frames flow both
    /// ways (STREAM is 1-RTT only, RFC 9000 §12.5).
    fn conn_turn(t: MockDatagramTransport, now: Instant) -> ConnectionTurn<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), local_cid(), 1200);
        send.install(PacketNumberSpace::ApplicationData, keys().client);
        ConnectionTurn::new(driver(t, now), send, 1200, DEFAULT_ACK_DELAY_EXPONENT)
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

    /// A request driver over `t` ready to place and drive requests, standing in for a
    /// confirmed connection spliced into the request phase.
    fn request_driver(
        t: MockDatagramTransport,
        now: Instant,
    ) -> RequestDriver<MockDatagramTransport> {
        RequestDriver::new(RequestTurn::with_default_frame_len(conn_turn(t, now), pump()))
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
    /// `pn`, standing in for the datagram the server sends in the request phase.
    fn one_rtt_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = local_cid();
        let header = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &dcid };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    /// A STREAM frame carrying the response for `code`/`body` on `stream_id`, FIN set.
    fn response_stream(stream_id: u64, code: &[u8], body: &[u8]) -> Frame {
        Frame::Stream { stream_id, offset: 0, fin: true, data: response_bytes(code, body) }
    }

    // ---- fetch ---------------------------------------------------------

    #[test]
    fn fetch_drives_one_request_to_its_response() {
        let now = base();
        let mut t = transport();
        t.push_inbound(one_rtt_packet(0, &[response_stream(0, b"200", b"hello")]));
        let mut d = request_driver(t, now);

        let resp = fetch(&mut d, &get(b"/index"), || now, 8).expect("fetch succeeds");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
        // The single response was drained out of the driver.
        assert!(d.responses().is_empty());
        assert!(d.is_done());
    }

    #[test]
    fn fetch_reports_incomplete_when_the_peer_never_answers() {
        let now = base();
        // Empty transport: the request goes out but no response ever arrives.
        let mut d = request_driver(transport(), now);
        let err = fetch(&mut d, &get(b"/never"), || now, 3).unwrap_err();
        assert!(matches!(err, FetchError::Incomplete), "unexpected error: {err:?}");
        assert_eq!(d.in_flight(), 1, "the request is still in flight");
    }

    #[test]
    fn fetch_surfaces_a_bad_frame_as_a_driver_error() {
        let now = base();
        let mut t = transport();
        // A response on stream 8 — a client bidi stream we never opened; the pump has
        // no exchange for it and rejects the routing (RFC 9114 §4.1).
        t.push_inbound(one_rtt_packet(0, &[response_stream(8, b"200", b"stray")]));
        let mut d = request_driver(t, now);
        let err = fetch(&mut d, &get(b"/a"), || now, 8).unwrap_err();
        assert!(matches!(err, FetchError::Driver(_)), "unexpected error: {err:?}");
    }

    #[test]
    fn fetch_reads_the_clock_once_per_turn() {
        let now = base();
        let mut t = transport();
        t.push_inbound(one_rtt_packet(0, &[response_stream(0, b"204", b"")]));
        let mut d = request_driver(t, now);
        let mut ticks = 0u32;
        let resp = fetch(&mut d, &get(b"/x"), || { ticks += 1; now }, 8).expect("fetch");
        assert_eq!(resp.status, 204);
        assert!(ticks >= 1, "the clock closure was read at least once");
    }

    // ---- connect_and_fetch: connect-phase failure ----------------------

    #[test]
    fn connect_and_fetch_reports_unconfirmed_when_the_server_is_silent() {
        // A real bootstrap over a transport that never answers: the handshake cannot
        // confirm, so the orchestrator stops at the connect phase without ever
        // reaching the splice or the request.
        let now = base();
        let config = ClientConnectConfig::default();
        let connect = connect_client(transport(), "example.com", Vec::new(), now, 0, &config)
            .expect("bootstrap assembles the first flight");
        let err =
            connect_and_fetch(connect, config.request_pump(), &get(b"/"), || now, 4, 4).unwrap_err();
        match err {
            ConnectFetchError::NotConfirmed(outcome) => {
                assert_eq!(outcome, ConnectOutcome::Incomplete, "silent server: budget spent");
            }
            other => panic!("expected NotConfirmed, got {other:?}"),
        }
    }

    // ---- ClientConnectConfig::request_pump / stream_manager_config -----

    #[test]
    fn request_pump_config_mirrors_the_advertised_limits() {
        let config = ClientConnectConfig::default();
        let smc = config.stream_manager_config();
        assert_eq!(smc.initial_max_data, config.initial_max_data);
        assert_eq!(
            smc.initial_max_stream_data_bidi_local,
            config.initial_max_stream_data_bidi_local
        );
        assert_eq!(smc.initial_max_streams_uni, config.initial_max_streams_uni);
        // The pump builds; a placed request lands on the first client bidi stream.
        let mut p = config.request_pump();
        let sent = p.send_request(&get(b"/a")).expect("request places");
        assert_eq!(sent.stream_id, 0);
    }
}
