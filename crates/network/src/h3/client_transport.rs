//! HTTP/3 transport orchestration (RFC 9114 §3.3, §4.1): the real-socket
//! [`h3_do_request`] that assembles a live QUIC transport and drives a single
//! request/response over it, returning the [`H3Response`] the dispatch in
//! `lib.rs` maps onto the crate's `Response` alongside the H1/H2 paths.
//!
//! Every layer below is transport-generic and IO-free (or mockable):
//! [`client_bootstrap::connect_client`](super::client_bootstrap::connect_client)
//! assembles a [`ConnectDriver`](super::conn_connect::ConnectDriver) from a
//! `(transport, server name, trust store)` triple, and
//! [`client_request::connect_and_fetch`](super::client_request::connect_and_fetch)
//! opens the connection and fetches one request over it. This slice is the one
//! place that binds those to the operating system: it resolves the authority to
//! a socket address through the injected [`DnsResolver`], opens the real
//! [`udp::UdpDatagram`](super::udp::UdpDatagram) socket, populates the trust
//! store from
//! [`mozilla_roots::mozilla_trust_anchors`](super::mozilla_roots::mozilla_trust_anchors),
//! and reads the wall clock (a monotonic [`Instant`] for loss-detection timers
//! and a Unix-epoch second count for certificate validity).
//!
//! The transport-generic core is [`h3_exchange`]: it takes any
//! [`DatagramTransport`] and an explicit clock, so a scripted
//! [`MockDatagramTransport`](super::udp::MockDatagramTransport) drives the whole
//! composition deterministically in tests. [`h3_do_request`] is the thin IO
//! wrapper that supplies the real socket, the real clock, and the bundled
//! Mozilla roots.
//!
//! ## What it still defers
//!
//! The mapping of the [`H3Response`] onto the crate's `Response`, and the
//! Alt-Svc dispatch that routes an origin onto this QUIC path only after it
//! advertised `h3` (RFC 7838, [`alt_svc`](super::alt_svc)) from an H2/H1.1
//! response, are the remaining wiring in `fetch_single`. That mapping lives at
//! the dispatch boundary — the crate's `Response` is a `lib.rs`-private type the
//! HTTP/1.1 and HTTP/2 paths also produce there — so this module stays free of
//! it and returns the protocol-native [`H3Response`]. This module is the QUIC
//! leg that dispatch calls.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use lumen_core::ext::DnsResolver;

use super::client_bootstrap::{BootstrapError, ClientConnectConfig, connect_client};
use super::client_request::{
    ConnectFetchError, connect_and_fetch, connect_and_fetch_with_sink, fetch, fetch_with_sink,
};
use super::conn_connect::{ConnectOutcome, OwnedTrustAnchor};
use super::h3_exchange::{BodySink, H3Response};
use super::h3_request::{H3Profile, H3ResponseHead};
use super::mozilla_roots::mozilla_trust_anchors;
use super::request_driver::RequestDriver;
use super::request_exchange::ClientRequest;
use super::udp::{DatagramTransport, UdpDatagram};
use super::varint::{self, VarIntTooLarge};

/// The default HTTPS port; when the request targets it the `:authority`
/// pseudo-header omits the port (RFC 9114 §4.3.2, RFC 3986 §3.2.3).
const HTTPS_DEFAULT_PORT: u16 = 443;

/// Why [`h3_do_request`] could not obtain the response over QUIC.
#[derive(Debug)]
pub enum H3TransportError {
    /// The authority did not resolve to a usable socket address: the resolver
    /// errored or returned an empty list (NXDOMAIN). Carries a describing
    /// message.
    Resolve(String),
    /// Binding or connecting the UDP socket to the resolved peer failed
    /// (RFC 9000 §5).
    Socket(io::Error),
    /// Assembling the QUIC first flight failed — the OS entropy source was
    /// unavailable or the TLS 1.3 ClientHello could not be encoded
    /// ([`BootstrapError`]).
    Bootstrap(BootstrapError),
    /// The handshake, the request-phase splice, or the request itself failed
    /// over the live transport ([`ConnectFetchError`]). This is the signal to
    /// fall back to the H2 / H1.1 path (RFC 7838 §2.4).
    Exchange(ConnectFetchError),
}

impl core::fmt::Display for H3TransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Resolve(msg) => write!(f, "HTTP/3 transport: {msg}"),
            Self::Socket(e) => write!(f, "HTTP/3 transport: opening the UDP socket: {e}"),
            Self::Bootstrap(e) => write!(f, "HTTP/3 transport: {e}"),
            Self::Exchange(e) => write!(f, "HTTP/3 transport: {e}"),
        }
    }
}

impl std::error::Error for H3TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Socket(e) => Some(e),
            Self::Bootstrap(e) => Some(e),
            Self::Exchange(e) => Some(e),
            Self::Resolve(_) => None,
        }
    }
}

/// Resolve `host:port` to a socket address and open a connected
/// [`UdpDatagram`] to it (RFC 9000 §5): the QUIC path's real transport.
///
/// The first resolved address is used — unlike the TCP paths, a UDP `connect`
/// only fixes the peer and cannot synchronously prove reachability, so trying
/// each address in turn would mean re-driving the whole handshake per address;
/// that fallback is out of scope for this slice. The local bind address matches
/// the peer's address family (an unspecified IPv4 or IPv6 address on an
/// OS-chosen ephemeral port), so the kernel picks the source port and filters
/// inbound datagrams to the peer.
///
/// # Errors
///
/// [`H3TransportError::Resolve`] if the authority does not resolve to any
/// address, or [`H3TransportError::Socket`] if the socket cannot be bound or
/// connected.
fn open_transport(
    resolver: &dyn DnsResolver,
    host: &str,
    port: u16,
) -> Result<UdpDatagram, H3TransportError> {
    let peer = resolver
        .resolve(host, port)
        .map_err(|e| H3TransportError::Resolve(format!("resolve {host}:{port}: {e}")))?
        .into_iter()
        .next()
        .ok_or_else(|| {
            H3TransportError::Resolve(format!("resolve {host}:{port}: no addresses"))
        })?;
    let local: SocketAddr = if peer.is_ipv6() {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
    };
    UdpDatagram::connect(local, peer).map_err(H3TransportError::Socket)
}

/// The `:authority` pseudo-header value for `host:port` (RFC 9114 §4.3.2): the
/// bare host when the port is the HTTPS default, else `host:port`.
fn authority_for(host: &str, port: u16) -> String {
    if port == HTTPS_DEFAULT_PORT {
        host.to_owned()
    } else {
        format!("{host}:{port}")
    }
}

/// Drive one HTTP/3 request/response over `transport` and return the assembled
/// [`H3Response`] — the transport-generic core of [`h3_do_request`]
/// (RFC 9114 §3.3, §4.1).
///
/// `server_name` is the SNI / certificate-verification host, `authority` the
/// `:authority` pseudo-header bytes, and `trust_anchors` the roots the server
/// certificate is judged against. `now`/`now_unix` seed the connection's
/// monotonic timers and the certificate validity check; `clock` is read once
/// per turn and shared across the handshake and request phases. `connect_turns`
/// and `request_turns` bound the two loops so neither phase can spin forever.
///
/// # Errors
///
/// [`H3TransportError::Bootstrap`] if the first flight cannot be assembled, or
/// [`H3TransportError::Exchange`] if the handshake, splice, or request fails.
#[allow(clippy::too_many_arguments)]
fn h3_exchange<T: DatagramTransport>(
    transport: T,
    server_name: &str,
    authority: &[u8],
    trust_anchors: Vec<OwnedTrustAnchor>,
    now: Instant,
    now_unix: i64,
    clock: impl FnMut() -> Instant,
    method: &[u8],
    path: &[u8],
    headers: &[(&[u8], &[u8])],
    body: &[u8],
    config: &ClientConnectConfig,
    connect_turns: usize,
    request_turns: usize,
) -> Result<H3Response, H3TransportError> {
    let connect = connect_client(transport, server_name, trust_anchors, now, now_unix, config)
        .map_err(H3TransportError::Bootstrap)?;

    let req = ClientRequest {
        profile: H3Profile::default(),
        method,
        scheme: b"https",
        authority,
        path,
        protocol: None,
        headers,
        body,
        use_huffman: true,
    };
    connect_and_fetch(
        connect,
        config.request_pump(),
        &req,
        clock,
        connect_turns,
        request_turns,
    )
    .map_err(H3TransportError::Exchange)
}

/// Identical to [`h3_exchange`] but forwards body bytes to `sink` as DATA frames
/// arrive during the request phase.
#[allow(clippy::too_many_arguments)]
fn h3_exchange_with_sink<'s, T: DatagramTransport>(
    transport: T,
    server_name: &str,
    authority: &[u8],
    trust_anchors: Vec<OwnedTrustAnchor>,
    now: Instant,
    now_unix: i64,
    clock: impl FnMut() -> Instant,
    method: &[u8],
    path: &[u8],
    headers: &[(&[u8], &[u8])],
    body: &[u8],
    config: &ClientConnectConfig,
    connect_turns: usize,
    request_turns: usize,
    sink: Option<BodySink<'s>>,
) -> Result<H3Response, H3TransportError> {
    let connect = connect_client(transport, server_name, trust_anchors, now, now_unix, config)
        .map_err(H3TransportError::Bootstrap)?;

    let req = ClientRequest {
        profile: H3Profile::default(),
        method,
        scheme: b"https",
        authority,
        path,
        protocol: None,
        headers,
        body,
        use_huffman: true,
    };
    connect_and_fetch_with_sink(
        connect,
        config.request_pump(),
        &req,
        clock,
        connect_turns,
        request_turns,
        sink,
    )
    .map_err(H3TransportError::Exchange)
}

/// Fetch `https://host:port{path}` over HTTP/3, opening a fresh QUIC connection
/// (RFC 9114 §3.3): the real-transport `h3_do_request` alongside the H1/H2
/// paths in `lib.rs`.
///
/// The authority is resolved through `resolver` and a real [`UdpDatagram`]
/// socket is opened to the first address; the server certificate is judged
/// against the bundled Mozilla roots ([`mozilla_trust_anchors`]) and the current
/// wall clock. `config` supplies the advertised QUIC transport parameters and
/// the request pump; `connect_turns` and `request_turns` bound the handshake and
/// request loops. The scheme is always `https` (HTTP/3 has no cleartext form)
/// and the request uses the default [`H3Profile`] header order.
///
/// The [`H3Response`] is returned as-is; mapping it onto the crate's `Response`
/// is the dispatch boundary's job in `lib.rs`.
///
/// # Errors
///
/// [`H3TransportError`] naming the phase that failed: DNS resolution, opening
/// the socket, assembling the first flight, or the handshake/request exchange.
/// An [`H3TransportError::Exchange`] is the caller's cue to fall back to the
/// H2 / H1.1 path (RFC 7838 §2.4).
#[allow(clippy::too_many_arguments)]
pub fn h3_do_request(
    resolver: &dyn DnsResolver,
    host: &str,
    port: u16,
    method: &[u8],
    path: &[u8],
    headers: &[(&[u8], &[u8])],
    body: &[u8],
    config: &ClientConnectConfig,
    connect_turns: usize,
    request_turns: usize,
) -> Result<H3Response, H3TransportError> {
    let transport = open_transport(resolver, host, port)?;
    let now = Instant::now();
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let authority = authority_for(host, port);
    h3_exchange(
        transport,
        host,
        authority.as_bytes(),
        mozilla_trust_anchors(),
        now,
        now_unix,
        Instant::now,
        method,
        path,
        headers,
        body,
        config,
        connect_turns,
        request_turns,
    )
}

/// Identical to [`h3_do_request`] but forwards body bytes to `sink` as DATA frames
/// arrive during the request phase.
///
/// # Errors
///
/// Same conditions as [`h3_do_request`].
#[allow(clippy::too_many_arguments)]
pub fn h3_do_request_with_sink<'s>(
    resolver: &dyn DnsResolver,
    host: &str,
    port: u16,
    method: &[u8],
    path: &[u8],
    headers: &[(&[u8], &[u8])],
    body: &[u8],
    config: &ClientConnectConfig,
    connect_turns: usize,
    request_turns: usize,
    sink: Option<BodySink<'s>>,
) -> Result<H3Response, H3TransportError> {
    let transport = open_transport(resolver, host, port)?;
    let now = Instant::now();
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let authority = authority_for(host, port);
    h3_exchange_with_sink(
        transport,
        host,
        authority.as_bytes(),
        mozilla_trust_anchors(),
        now,
        now_unix,
        Instant::now,
        method,
        path,
        headers,
        body,
        config,
        connect_turns,
        request_turns,
        sink,
    )
}

/// Resolve `host:port`, open a UDP socket, run the TLS 1.3 / QUIC handshake,
/// and return a confirmed [`RequestDriver<UdpDatagram>`](RequestDriver) ready for
/// sequential requests — the connection-reuse entry point (RFC 9114 §3.3).
///
/// Unlike [`h3_do_request`] (which opens, fetches, and drops the connection in one
/// call), this function keeps the connection alive in the returned driver. The caller
/// is responsible for storing the driver in a
/// [`H3ConnectionPool`](super::client_pool::H3ConnectionPool) and for dropping it
/// when it is no longer needed.
///
/// # Errors
///
/// [`H3TransportError::Resolve`] or [`H3TransportError::Socket`] if the transport
/// cannot be opened, [`H3TransportError::Bootstrap`] if the first flight fails,
/// [`H3TransportError::Exchange`] if the handshake stalled or the splice failed.
pub fn h3_connect(
    resolver: &dyn DnsResolver,
    host: &str,
    port: u16,
    config: &ClientConnectConfig,
    connect_turns: usize,
) -> Result<RequestDriver<UdpDatagram>, H3TransportError> {
    let transport = open_transport(resolver, host, port)?;
    let now = Instant::now();
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut connect =
        connect_client(transport, host, mozilla_trust_anchors(), now, now_unix, config)
            .map_err(H3TransportError::Bootstrap)?;
    match connect
        .connect(&mut Instant::now, connect_turns)
        .map_err(|e| H3TransportError::Exchange(ConnectFetchError::Connect(e)))?
    {
        ConnectOutcome::Confirmed => {}
        other => {
            return Err(H3TransportError::Exchange(ConnectFetchError::NotConfirmed(other)));
        }
    }
    connect
        .into_request_driver(config.request_pump())
        .map_err(|e| H3TransportError::Exchange(ConnectFetchError::Splice(e)))
}

/// Fetch one HTTP/3 request on `driver` (already confirmed, from the pool or from
/// [`h3_connect`]) and return the [`H3Response`] — the per-request leg for the
/// connection-reuse path (RFC 9114 §4.1).
///
/// The caller retains `driver` after the call; on success the driver can be put
/// back into the [`H3ConnectionPool`](super::client_pool::H3ConnectionPool).
/// On any error the driver should be discarded — the connection state is unknown.
///
/// # Errors
///
/// [`H3TransportError::Exchange`] wrapping a [`ConnectFetchError::Fetch`] if the
/// request turn fails or exhausts its budget.
#[allow(clippy::too_many_arguments)]
pub fn h3_fetch_on_driver(
    driver: &mut RequestDriver<UdpDatagram>,
    host: &str,
    port: u16,
    method: &[u8],
    path: &[u8],
    headers: &[(&[u8], &[u8])],
    body: &[u8],
    request_turns: usize,
) -> Result<H3Response, H3TransportError> {
    let authority = authority_for(host, port);
    let req = ClientRequest {
        profile: H3Profile::default(),
        method,
        scheme: b"https",
        authority: authority.as_bytes(),
        path,
        protocol: None,
        headers,
        body,
        use_huffman: true,
    };
    fetch(driver, &req, Instant::now, request_turns)
        .map_err(|e| H3TransportError::Exchange(ConnectFetchError::Fetch(e)))
}

/// Identical to [`h3_fetch_on_driver`] but forwards body bytes to `sink` as DATA
/// frames arrive during the request phase.
///
/// # Errors
///
/// Same conditions as [`h3_fetch_on_driver`].
#[allow(clippy::too_many_arguments)]
pub fn h3_fetch_on_driver_with_sink<'s>(
    driver: &mut RequestDriver<UdpDatagram>,
    host: &str,
    port: u16,
    method: &[u8],
    path: &[u8],
    headers: &[(&[u8], &[u8])],
    body: &[u8],
    request_turns: usize,
    sink: Option<BodySink<'s>>,
) -> Result<H3Response, H3TransportError> {
    let authority = authority_for(host, port);
    let req = ClientRequest {
        profile: H3Profile::default(),
        method,
        scheme: b"https",
        authority: authority.as_bytes(),
        path,
        protocol: None,
        headers,
        body,
        use_huffman: true,
    };
    fetch_with_sink(driver, &req, Instant::now, request_turns, sink)
        .map_err(|e| H3TransportError::Exchange(ConnectFetchError::Fetch(e)))
}

/// Open an RFC 9220 Extended CONNECT session on `driver` (already confirmed, from
/// [`h3_connect`] or the pool) and drive it to its final response head — the
/// WebTransport session handshake's HTTP/3 leg (RFC 9220 §3, WHATWG WebTransport
/// §5.1).
///
/// Builds a `CONNECT` request carrying the `:protocol = webtransport` (or
/// whatever `protocol` names) pseudo-header, places it via
/// [`RequestDriver::open_extended_connect`] — which, unlike an ordinary request,
/// leaves the send half open: a successful Extended CONNECT stream is the
/// session's control stream for as long as the session lives, so it is
/// deliberately never FIN'd — then drives `driver.transmit`/`driver.poll` turn by
/// turn (not [`RequestDriver::run`], which only stops once [`is_done`], i.e. on
/// completion or FIN, neither of which a live Extended CONNECT stream produces)
/// until [`RequestDriver::extended_connect_head`] reports a final head.
///
/// Returns the stream identifier (the caller needs it for later slices — uni/bidi
/// streams and datagrams associate with this same Extended CONNECT stream) paired
/// with the response head. The caller decides pass/fail from the head's
/// `:status`: this function's job is only "a final response head arrived", not
/// judging whether it is a 2xx.
///
/// The caller keeps `driver` alive afterward — later slices need the live
/// connection for streams and datagrams; this function never drops or closes it.
///
/// # Errors
///
/// [`H3TransportError::Exchange`] wrapping:
/// - [`ConnectFetchError::ExtendedConnectDispatch`] if the request cannot be
///   built or placed (RFC 9114 §4.2/§7.2.1, RFC 9000 §2.1);
/// - [`ConnectFetchError::ExtendedConnectDriver`] if a driver turn fails (a
///   socket error, a bad frame, a rejected send action);
/// - [`ConnectFetchError::ExtendedConnectIncomplete`] if `request_turns` turns
///   pass with no final head (the peer never answered).
pub fn h3_extended_connect_on_driver<T: DatagramTransport>(
    driver: &mut RequestDriver<T>,
    host: &str,
    port: u16,
    protocol: &[u8],
    path: &[u8],
    headers: &[(&[u8], &[u8])],
    request_turns: usize,
) -> Result<(u64, H3ResponseHead), H3TransportError> {
    let authority = authority_for(host, port);
    let req = ClientRequest {
        profile: H3Profile::default(),
        method: b"CONNECT",
        scheme: b"https",
        authority: authority.as_bytes(),
        path,
        protocol: Some(protocol),
        headers,
        body: b"",
        use_huffman: true,
    };
    let sent = driver
        .open_extended_connect(&req)
        .map_err(|e| H3TransportError::Exchange(ConnectFetchError::ExtendedConnectDispatch(e)))?;
    let stream_id = sent.stream_id;

    for _ in 0..request_turns {
        driver
            .transmit(Instant::now())
            .map_err(|e| H3TransportError::Exchange(ConnectFetchError::ExtendedConnectDriver(e)))?;
        if let Some(head) = driver.extended_connect_head(stream_id) {
            return Ok((stream_id, head.clone()));
        }
        driver
            .poll(Instant::now())
            .map_err(|e| H3TransportError::Exchange(ConnectFetchError::ExtendedConnectDriver(e)))?;
        if let Some(head) = driver.extended_connect_head(stream_id) {
            return Ok((stream_id, head.clone()));
        }
    }
    Err(H3TransportError::Exchange(ConnectFetchError::ExtendedConnectIncomplete))
}

/// draft-ietf-webtrans-http3 §4.2: the QUIC varint stream type identifying a
/// client-initiated WebTransport unidirectional stream, prefixing the session
/// id that demultiplexes it to a session.
const WEBTRANSPORT_UNI_STREAM_TYPE: u64 = 0x54;

/// Why [`h3_webtransport_open_uni_stream_on_driver`] could not open a
/// WebTransport unidirectional stream.
#[derive(Debug)]
pub enum WebTransportStreamError {
    /// Every client-initiated unidirectional QUIC stream identifier has been
    /// handed out (RFC 9000 §2.1: `2^60` per type, the same bound
    /// [`super::request_mux::OpenError::StreamsExhausted`] applies to bidi
    /// streams) — unreachable on any real session.
    StreamsExhausted,
    /// The stream header (stream type, then session id — both QUIC varints)
    /// could not be encoded because a value exceeded the varint's 62-bit range
    /// (RFC 9000 §16). `session_id` is itself a QUIC stream identifier, always
    /// well inside that range, so this only fires on a value from a future
    /// caller that is not.
    Header(VarIntTooLarge),
    /// A driver turn failed while flushing the stream header onto the wire — a
    /// socket error, a bad frame, or a rejected send action.
    Driver(super::request_driver::RequestDriverError),
    /// [`h3_webtransport_write_stream_on_driver`] was asked to write to a
    /// `stream_id` [`h3_webtransport_open_uni_stream_on_driver`] never opened
    /// on this driver (or that `stream_id` belongs to some other stream
    /// space entirely) — the caller passed back a stale or foreign id.
    UnknownStream(u64),
    /// [`h3_webtransport_reset_uni_stream_on_driver`] could not queue the
    /// RESET_STREAM frame it built — the Application Data space has no send
    /// keys installed yet, or the frame overflowed the scheduler's payload
    /// budget (unreachable for a frame this small on any real path MTU).
    Enqueue(super::send_state::SendStateError),
}

impl core::fmt::Display for WebTransportStreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::StreamsExhausted => {
                write!(f, "WebTransport: client unidirectional stream identifiers exhausted")
            }
            Self::Header(e) => write!(f, "WebTransport: stream header: {e}"),
            Self::Driver(e) => write!(f, "WebTransport: opening unidirectional stream: {e}"),
            Self::UnknownStream(id) => write!(f, "WebTransport: unknown stream id {id}"),
            Self::Enqueue(e) => write!(f, "WebTransport: queuing RESET_STREAM: {e}"),
        }
    }
}

impl std::error::Error for WebTransportStreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Header(e) => Some(e),
            Self::Driver(e) => Some(e),
            Self::Enqueue(e) => Some(e),
            Self::StreamsExhausted | Self::UnknownStream(_) => None,
        }
    }
}

/// Opens a WebTransport unidirectional stream (draft-ietf-webtrans-http3 §4.2)
/// on `driver`'s connection, for the session `session_id` names — the Extended
/// CONNECT stream identifier [`h3_extended_connect_on_driver`] returned when
/// the session was established.
///
/// Allocates the `uni_stream_number`-th client-initiated unidirectional QUIC
/// stream (RFC 9000 §2.1: the low two bits `0b10` mark client-initiated
/// unidirectional, so the *n*-th such stream is identifier `4n + 2`) — the
/// caller owns `uni_stream_number`, incrementing it per session starting at
/// `0`, since it is a fully separate identifier space from the client
/// bidirectional streams [`h3_extended_connect_on_driver`]/ordinary requests
/// use and needs no coordination with them. Writes the WebTransport stream
/// header (stream type `0x54`, then `session_id`, both QUIC varints —
/// draft-ietf-webtrans-http3 §4.2) as the stream's first bytes and flushes it
/// onto the wire.
///
/// Returns the opened stream's QUIC identifier; the caller writes further
/// application bytes to it directly through
/// `driver.turn_mut().pump_mut().dispatch_mut().streams_mut()` (the same
/// accessor chain this function uses) — a unidirectional WebTransport stream
/// carries no response, so unlike [`h3_extended_connect_on_driver`] this
/// issues one `transmit` and returns rather than polling for a reply; the
/// stream stays open (no FIN) for those later writes regardless of whether
/// this first flush cleared the socket immediately or is still queued behind
/// flow control.
///
/// # Errors
///
/// [`WebTransportStreamError::StreamsExhausted`], [`WebTransportStreamError::Header`],
/// or [`WebTransportStreamError::Driver`] — see their docs.
pub fn h3_webtransport_open_uni_stream_on_driver<T: DatagramTransport>(
    driver: &mut RequestDriver<T>,
    uni_stream_number: u64,
    peer_initial_max_stream_data_uni: u64,
    session_id: u64,
) -> Result<u64, WebTransportStreamError> {
    let stream_id = uni_stream_number
        .checked_mul(4)
        .and_then(|n| n.checked_add(2))
        .ok_or(WebTransportStreamError::StreamsExhausted)?;

    let mut header = Vec::with_capacity(varint::encoded_len(WEBTRANSPORT_UNI_STREAM_TYPE).unwrap_or(1) + 8);
    varint::encode(WEBTRANSPORT_UNI_STREAM_TYPE, &mut header).map_err(WebTransportStreamError::Header)?;
    varint::encode(session_id, &mut header).map_err(WebTransportStreamError::Header)?;

    driver
        .turn_mut()
        .pump_mut()
        .dispatch_mut()
        .streams_mut()
        .open_send_stream(stream_id, peer_initial_max_stream_data_uni)
        .write(&header);

    driver.transmit(Instant::now()).map_err(WebTransportStreamError::Driver)?;
    Ok(stream_id)
}

/// Writes application bytes to a WebTransport unidirectional stream already
/// opened by [`h3_webtransport_open_uni_stream_on_driver`] and flushes them
/// onto the wire.
///
/// Queues `data` on the stream's existing [`super::stream::SendStream`] (the
/// stream-type/session-id header `h3_webtransport_open_uni_stream_on_driver`
/// wrote stays untouched at the front of the stream, since `SendStream::write`
/// only ever appends) and issues one `transmit` — same "one flush, no
/// response to wait for" shape as opening the stream itself, since a
/// WebTransport unidirectional stream carries no reply.
///
/// # Errors
///
/// [`WebTransportStreamError::UnknownStream`] if `stream_id` was never opened
/// on `driver` (send half absent — the caller passed a stale or foreign id),
/// or [`WebTransportStreamError::Driver`] if the flushing turn fails.
pub fn h3_webtransport_write_stream_on_driver<T: DatagramTransport>(
    driver: &mut RequestDriver<T>,
    stream_id: u64,
    data: &[u8],
) -> Result<(), WebTransportStreamError> {
    driver
        .turn_mut()
        .pump_mut()
        .dispatch_mut()
        .streams_mut()
        .send_stream_mut(stream_id)
        .ok_or(WebTransportStreamError::UnknownStream(stream_id))?
        .write(data);

    driver.transmit(Instant::now()).map_err(WebTransportStreamError::Driver)
}

/// Gracefully closes a WebTransport unidirectional stream's sending half
/// (RFC 9000 §3.1, STREAM FIN) — `WritableStreamDefaultWriter.close()` on the
/// stream [`h3_webtransport_open_uni_stream_on_driver`] opened.
///
/// Marks the stream's [`super::stream::SendStream`] finished
/// ([`super::stream::SendStream::finish`]) and flushes: no further
/// [`h3_webtransport_write_stream_on_driver`] call reaches the wire after
/// this (`SendStream::write` silently drops once `finish` was called, RFC
/// 9000 §3.1), and the send half moves to `DataSent` once the FIN itself
/// clears the socket — the normal, non-error end of a WebTransport
/// unidirectional stream (draft-ietf-webtrans-http3 §4.2 says nothing special
/// happens on the wire beyond the QUIC FIN).
///
/// # Errors
///
/// [`WebTransportStreamError::UnknownStream`] if `stream_id` was never opened
/// on `driver`, or [`WebTransportStreamError::Driver`] if the flushing turn
/// fails.
pub fn h3_webtransport_close_uni_stream_on_driver<T: DatagramTransport>(
    driver: &mut RequestDriver<T>,
    stream_id: u64,
) -> Result<(), WebTransportStreamError> {
    driver
        .turn_mut()
        .pump_mut()
        .dispatch_mut()
        .streams_mut()
        .send_stream_mut(stream_id)
        .ok_or(WebTransportStreamError::UnknownStream(stream_id))?
        .finish();

    driver.transmit(Instant::now()).map_err(WebTransportStreamError::Driver)
}

/// Abruptly terminates a WebTransport unidirectional stream's sending half
/// with `error_code` (RFC 9000 §3.1/§19.4, RESET_STREAM) —
/// `WritableStreamDefaultWriter.abort(reason)` on the stream
/// [`h3_webtransport_open_uni_stream_on_driver`] opened.
///
/// Unlike [`h3_webtransport_close_uni_stream_on_driver`]'s FIN, a RESET_STREAM
/// is not something [`super::stream::SendStream::poll_transmit`] ever emits —
/// it is a control frame, not stream data, so it is built here directly (final
/// size = the stream's write offset at the moment of reset, RFC 9000 §19.4)
/// and queued straight into the connection's Application Data send scheduler
/// ([`super::send_state::ConnectionSendState::enqueue`]) before the discarded
/// unsent bytes and reset bookkeeping are recorded on the [`super::stream::SendStream`]
/// itself ([`super::stream::SendStream::reset`]).
///
/// # Errors
///
/// [`WebTransportStreamError::UnknownStream`] if `stream_id` was never opened
/// on `driver`, [`WebTransportStreamError::Enqueue`] if the RESET_STREAM frame
/// could not be queued, or [`WebTransportStreamError::Driver`] if the flushing
/// turn fails.
pub fn h3_webtransport_reset_uni_stream_on_driver<T: DatagramTransport>(
    driver: &mut RequestDriver<T>,
    stream_id: u64,
    error_code: u64,
) -> Result<(), WebTransportStreamError> {
    let dispatch = driver.turn_mut().pump_mut().dispatch_mut();
    let send = dispatch
        .streams_mut()
        .send_stream_mut(stream_id)
        .ok_or(WebTransportStreamError::UnknownStream(stream_id))?;
    let final_size = send.write_offset();
    send.reset(error_code);

    driver
        .turn_mut()
        .turn_mut()
        .send_mut()
        .enqueue(
            super::loss::PacketNumberSpace::ApplicationData,
            super::quic_frame::Frame::ResetStream { stream_id, app_error_code: error_code, final_size },
        )
        .map_err(WebTransportStreamError::Enqueue)?;

    driver.transmit(Instant::now()).map_err(WebTransportStreamError::Driver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::udp::MockDatagramTransport;
    use lumen_core::error::{Error, Result as CoreResult};
    use std::net::{Ipv4Addr, SocketAddrV4};

    /// A [`DnsResolver`] returning a fixed address list (or an error), so the
    /// transport-opening path is exercised without a real name lookup.
    struct FixedResolver(CoreResult<Vec<SocketAddr>>);

    impl DnsResolver for FixedResolver {
        fn resolve(&self, _host: &str, _port: u16) -> CoreResult<Vec<SocketAddr>> {
            match &self.0 {
                Ok(addrs) => Ok(addrs.clone()),
                Err(e) => Err(Error::Network(format!("{e}"))),
            }
        }
    }

    use super::super::stream::SendState;

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn transport() -> MockDatagramTransport {
        MockDatagramTransport::new(loopback(1), loopback(2))
    }

    // ---- authority_for --------------------------------------------------

    #[test]
    fn authority_omits_the_default_https_port() {
        assert_eq!(authority_for("example.com", 443), "example.com");
        assert_eq!(authority_for("example.com", 8443), "example.com:8443");
    }

    // ---- open_transport -------------------------------------------------

    #[test]
    fn open_transport_reports_an_empty_resolution() {
        let resolver = FixedResolver(Ok(Vec::new()));
        let err = open_transport(&resolver, "nx.example", 443).unwrap_err();
        match err {
            H3TransportError::Resolve(msg) => assert!(msg.contains("no addresses"), "{msg}"),
            other => panic!("expected Resolve, got {other:?}"),
        }
    }

    #[test]
    fn open_transport_propagates_a_resolver_error() {
        let resolver = FixedResolver(Err(Error::Network("boom".to_owned())));
        let err = open_transport(&resolver, "bad.example", 443).unwrap_err();
        match err {
            H3TransportError::Resolve(msg) => assert!(msg.contains("boom"), "{msg}"),
            other => panic!("expected Resolve, got {other:?}"),
        }
    }

    #[test]
    fn open_transport_binds_a_socket_for_a_resolved_address() {
        // A real loopback address resolves and the connected UDP socket binds to
        // an ephemeral local port of the matching family.
        let resolver = FixedResolver(Ok(vec![loopback(4433)]));
        let udp = open_transport(&resolver, "localhost", 4433).expect("socket opens");
        let local = udp.local_addr().expect("local addr");
        assert!(local.is_ipv4(), "IPv4 peer binds an IPv4 local address");
        assert_ne!(local.port(), 0, "the OS assigned an ephemeral port");
        assert_eq!(udp.peer_addr().expect("peer addr"), loopback(4433));
    }

    // ---- h3_exchange ----------------------------------------------------

    #[test]
    fn h3_exchange_reports_unconfirmed_over_a_silent_transport() {
        // A scripted transport that never answers: the handshake cannot confirm,
        // so the composition stops at the connect phase and never reaches the
        // response. Exercises connect_client + connect_and_fetch as one, plus the
        // ClientRequest this module builds from the request parts.
        let now = Instant::now();
        let config = ClientConnectConfig::default();
        let err = h3_exchange(
            transport(),
            "example.com",
            b"example.com",
            Vec::new(),
            now,
            1_700_000_000,
            || now,
            b"GET",
            b"/",
            &[],
            b"",
            &config,
            4,
            4,
        )
        .unwrap_err();
        match err {
            H3TransportError::Exchange(ConnectFetchError::NotConfirmed(_)) => {}
            other => panic!("expected Exchange(NotConfirmed), got {other:?}"),
        }
    }

    // ---- h3_extended_connect_on_driver (RFC 9220 §3) --------------------

    use crate::h3::conn_turn::{ConnectionTurn, DEFAULT_ACK_DELAY_EXPONENT};
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::ConnectionDriver;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::frame::Frame as H3Frame;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::loss::PacketNumberSpace;
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::pto::LossDetection;
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::recv_path::RecvKeyRing;
    use crate::h3::request_pump::RequestPump;
    use crate::h3::request_turn::RequestTurn;
    use crate::h3::send_state::ConnectionSendState;
    use crate::h3::stream_manager::StreamManagerConfig;
    use std::time::Duration;

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

    /// A confirmed-connection-shaped [`RequestDriver`] over `t`: Application-Data
    /// installed on both directions, ready to place requests — standing in for
    /// what [`h3_connect`] would have returned.
    fn extended_connect_driver(
        t: MockDatagramTransport,
        now: Instant,
    ) -> RequestDriver<MockDatagramTransport> {
        let mut recv_keys = RecvKeyRing::new();
        recv_keys.install(PacketNumberSpace::ApplicationData, keys().client);
        let driver = ConnectionDriver::new(
            DatagramEventLoop::new(t),
            connection(now),
            LossDetection::new(Duration::from_millis(25)),
            recv_keys,
            4,
        );
        let mut send = ConnectionSendState::new(1, dcid(), local_cid(), 1200);
        send.install(PacketNumberSpace::ApplicationData, keys().client);
        let turn = ConnectionTurn::new(driver, send, 1200, DEFAULT_ACK_DELAY_EXPONENT);
        RequestDriver::new(RequestTurn::with_default_frame_len(turn, pump()))
    }

    /// Encode the response-stream bytes for an Extended CONNECT response head:
    /// just a HEADERS frame carrying `:status` — no body (RFC 9220 §3 responses
    /// carry no message body).
    fn extended_connect_response_bytes(code: &[u8]) -> Vec<u8> {
        let block =
            qpack::encode_field_section(&[HeaderField::new(b":status".to_vec(), code.to_vec())], true);
        let mut out = Vec::new();
        H3Frame::Headers(block).encode(&mut out).unwrap();
        out
    }

    /// Encrypt one short-header (1-RTT) packet carrying `frames` with packet
    /// number `pn`.
    fn one_rtt_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = local_cid();
        let header = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &dcid };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    /// A STREAM frame carrying an Extended CONNECT response head for `code` on
    /// `stream_id`, with **no** FIN — an Extended CONNECT session's control
    /// stream is deliberately never closed on success (RFC 9220 §3).
    fn extended_connect_response_stream(stream_id: u64, code: &[u8]) -> Frame {
        Frame::Stream {
            stream_id,
            offset: 0,
            fin: false,
            data: extended_connect_response_bytes(code),
        }
    }

    #[test]
    fn extended_connect_resolves_on_a_2xx_head_with_no_fin() {
        let now = Instant::now();
        let mut t = transport();
        t.push_inbound(one_rtt_packet(0, &[extended_connect_response_stream(0, b"200")]));
        let mut driver = extended_connect_driver(t, now);

        let (stream_id, head) =
            h3_extended_connect_on_driver(&mut driver, "example.com", 443, b"webtransport", b"/wt", &[], 8)
                .expect("extended connect resolves");
        assert_eq!(stream_id, 0);
        assert_eq!(head.status, 200);
        // The stream is still alive (no FIN was ever sent) — the driver can keep
        // using it for later slices.
        assert!(driver.turn().pump().is_active(stream_id));
    }

    #[test]
    fn extended_connect_resolves_on_a_non_2xx_head_too() {
        // This function's job is only "a final head arrived" — the caller
        // decides pass/fail from the status. A 403 resolves `Ok` just like a 200.
        let now = Instant::now();
        let mut t = transport();
        t.push_inbound(one_rtt_packet(0, &[extended_connect_response_stream(0, b"403")]));
        let mut driver = extended_connect_driver(t, now);

        let (_, head) =
            h3_extended_connect_on_driver(&mut driver, "example.com", 443, b"webtransport", b"/wt", &[], 8)
                .expect("a non-2xx head still resolves Ok");
        assert_eq!(head.status, 403);
    }

    #[test]
    fn extended_connect_times_out_when_the_peer_never_answers() {
        let now = Instant::now();
        // No inbound datagram at all: the request goes out but nothing ever
        // answers, so the turn budget is spent without a final head.
        let mut driver = extended_connect_driver(transport(), now);

        let err = h3_extended_connect_on_driver(
            &mut driver,
            "example.com",
            443,
            b"webtransport",
            b"/wt",
            &[],
            3,
        )
        .unwrap_err();
        match err {
            H3TransportError::Exchange(ConnectFetchError::ExtendedConnectIncomplete) => {}
            other => panic!("expected ExtendedConnectIncomplete, got {other:?}"),
        }
    }

    #[test]
    fn webtransport_uni_stream_allocates_the_first_identifier_and_writes_the_header() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);

        let stream_id = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 0)
            .expect("opens the first uni stream");
        assert_eq!(stream_id, 2, "the first client uni stream identifier is 4*0 + 2");

        let send = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .expect("the uni stream's send half was opened");
        // Header: 2-byte varint 0x54 (stream type — 84 exceeds the 1-byte 0-63
        // range) + 1-byte varint 0 (session id).
        assert_eq!(send.write_offset(), 3);
    }

    #[test]
    fn webtransport_uni_stream_identifiers_advance_by_four() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);

        let first = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 0).unwrap();
        let second = h3_webtransport_open_uni_stream_on_driver(&mut driver, 1, 1 << 20, 0).unwrap();
        assert_eq!(first, 2);
        assert_eq!(second, 6);
    }

    #[test]
    fn webtransport_uni_stream_header_carries_the_session_id() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);

        // A session id above the 1-byte varint boundary (0x3f) forces the
        // session-id half of the header to 2 bytes too, exercising the varint
        // length switch rather than just the degenerate zero case.
        let stream_id = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 100)
            .expect("opens the uni stream");
        let send = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .expect("the uni stream's send half was opened");
        // 2-byte stream type (0x54) + 2-byte session id (100) = 4 bytes.
        assert_eq!(send.write_offset(), 4);
    }

    #[test]
    fn webtransport_uni_stream_number_overflow_is_reported_not_wrapped() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);

        let err = h3_webtransport_open_uni_stream_on_driver(&mut driver, u64::MAX, 1 << 20, 0)
            .unwrap_err();
        match err {
            WebTransportStreamError::StreamsExhausted => {}
            other => panic!("expected StreamsExhausted, got {other:?}"),
        }
    }

    #[test]
    fn webtransport_write_stream_appends_after_the_header() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);
        let stream_id = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 0)
            .expect("opens the uni stream");

        h3_webtransport_write_stream_on_driver(&mut driver, stream_id, b"hello")
            .expect("writes to the open stream");

        let send = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .expect("the uni stream's send half still exists");
        // 3-byte header (varint 0x54 + varint session id 0) + 5-byte payload.
        assert_eq!(send.write_offset(), 8);
    }

    #[test]
    fn webtransport_write_stream_on_an_unknown_id_is_reported() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);

        let err = h3_webtransport_write_stream_on_driver(&mut driver, 42, b"hello").unwrap_err();
        match err {
            WebTransportStreamError::UnknownStream(id) => assert_eq!(id, 42),
            other => panic!("expected UnknownStream, got {other:?}"),
        }
    }

    #[test]
    fn webtransport_write_stream_can_be_called_more_than_once() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);
        let stream_id = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 0)
            .expect("opens the uni stream");

        h3_webtransport_write_stream_on_driver(&mut driver, stream_id, b"foo").unwrap();
        h3_webtransport_write_stream_on_driver(&mut driver, stream_id, b"bar").unwrap();

        let send = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .unwrap();
        // 3-byte header + 3 + 3 payload bytes across two writes.
        assert_eq!(send.write_offset(), 9);
    }

    #[test]
    fn webtransport_close_stream_marks_the_send_half_finished() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);
        let stream_id = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 0)
            .expect("opens the uni stream");
        h3_webtransport_write_stream_on_driver(&mut driver, stream_id, b"bye").unwrap();

        h3_webtransport_close_uni_stream_on_driver(&mut driver, stream_id)
            .expect("closes the open stream");

        let send = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .expect("the uni stream's send half still exists");
        // `finish()` marks the FIN pending; `transmit()` inside the close call
        // already flushed it onto the (mock) wire, so the header + payload +
        // FIN chunk moved the state past `Send`.
        assert_eq!(send.state(), SendState::DataSent);
    }

    #[test]
    fn webtransport_write_after_close_is_silently_dropped() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);
        let stream_id = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 0)
            .expect("opens the uni stream");
        h3_webtransport_close_uni_stream_on_driver(&mut driver, stream_id).unwrap();
        let offset_at_close = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .unwrap()
            .write_offset();

        // The native binding itself keeps accepting the call (the JS layer is
        // responsible for refusing writes on a closed `WritableStream`); the
        // QUIC layer just drops the bytes, per `SendStream::write`'s contract
        // once `finish()` has been called.
        h3_webtransport_write_stream_on_driver(&mut driver, stream_id, b"too late").unwrap();

        let offset_after = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .unwrap()
            .write_offset();
        assert_eq!(offset_after, offset_at_close, "no bytes queued after finish()");
    }

    #[test]
    fn webtransport_close_on_an_unknown_id_is_reported() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);

        let err = h3_webtransport_close_uni_stream_on_driver(&mut driver, 42).unwrap_err();
        match err {
            WebTransportStreamError::UnknownStream(id) => assert_eq!(id, 42),
            other => panic!("expected UnknownStream, got {other:?}"),
        }
    }

    #[test]
    fn webtransport_reset_stream_moves_to_reset_sent_and_discards_unsent_data() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);
        let stream_id = h3_webtransport_open_uni_stream_on_driver(&mut driver, 0, 1 << 20, 0)
            .expect("opens the uni stream");
        h3_webtransport_write_stream_on_driver(&mut driver, stream_id, b"partial").unwrap();

        h3_webtransport_reset_uni_stream_on_driver(&mut driver, stream_id, 0x42)
            .expect("resets the open stream");

        let send = driver
            .turn_mut()
            .pump_mut()
            .dispatch_mut()
            .streams_mut()
            .send_stream(stream_id)
            .expect("the uni stream's send half still exists");
        assert_eq!(send.state(), SendState::ResetSent);
        assert_eq!(send.reset_error(), Some(0x42));
    }

    #[test]
    fn webtransport_reset_stream_on_an_unknown_id_is_reported() {
        let now = Instant::now();
        let mut driver = extended_connect_driver(transport(), now);

        let err = h3_webtransport_reset_uni_stream_on_driver(&mut driver, 42, 1).unwrap_err();
        match err {
            WebTransportStreamError::UnknownStream(id) => assert_eq!(id, 42),
            other => panic!("expected UnknownStream, got {other:?}"),
        }
    }
}
