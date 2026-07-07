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
use super::client_request::{ConnectFetchError, connect_and_fetch, fetch};
use super::conn_connect::{ConnectOutcome, OwnedTrustAnchor};
use super::h3_exchange::H3Response;
use super::h3_request::H3Profile;
use super::mozilla_roots::mozilla_trust_anchors;
use super::request_driver::RequestDriver;
use super::request_exchange::ClientRequest;
use super::udp::{DatagramTransport, UdpDatagram};

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
        headers,
        body,
        use_huffman: true,
    };
    fetch(driver, &req, Instant::now, request_turns)
        .map_err(|e| H3TransportError::Exchange(ConnectFetchError::Fetch(e)))
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
}
