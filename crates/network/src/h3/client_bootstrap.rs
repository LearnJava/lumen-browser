//! Client connect bootstrap (RFC 9000 §7.2/§7.3, RFC 9001 §5.2, RFC 9114 §3.2).
//!
//! Every layer below has been assembled by hand from a scripted transport and
//! fixed fixtures; this module is the one place that turns a bare
//! `(transport, server name, trust store)` triple into a ready-to-drive
//! [`ConnectDriver`], generating everything a real first flight needs:
//!
//! - the client's random Initial Destination and Source Connection IDs
//!   (RFC 9000 §7.2 — the DCID the client invents seeds the Initial keys, the
//!   SCID names the client for the peer),
//! - an ephemeral X25519 key pair (RFC 8446 §4.2.8),
//! - a real TLS 1.3 ClientHello (RFC 8446 §4.1.2) carrying `server_name`
//!   (RFC 6066 §3), the `h3` ALPN token (RFC 7301, RFC 9114 §3.2), a single
//!   X25519 `key_share`, the mandatory TLS 1.3 extensions, and the QUIC
//!   `quic_transport_parameters` extension (RFC 9000 §18) whose
//!   `initial_source_connection_id` echoes the client's SCID (RFC 9000 §7.3),
//! - the Initial packet-protection keys derived from the random DCID
//!   (RFC 9001 §5.2): the client **sends** with the client secret and
//!   **receives** with the server secret (the reverse of a fixture that crafts
//!   its own inbound packets).
//!
//! The result is one [`connect_client`] call that hands back a
//! [`ConnectDriver`] positioned exactly where a test fixture leaves it — the
//! Initial space installed on both halves and the TLS bridge seeded with the
//! ClientHello — so the caller only has to
//! [`connect`](ConnectDriver::connect). Assembling the real UDP transport (DNS
//! resolution, the [`udp::UdpDatagram`](super::udp::UdpDatagram) socket, and the
//! trust anchors) and routing a request onto this driver alongside the H1/H2
//! paths remains the caller's job in `lib.rs`.

use std::time::{Duration, Instant};

use super::conn_connect::{ConnectDriver, OwnedTrustAnchor};
use super::conn_handshake::HandshakeDriver;
use super::conn_tls::TlsConnState;
use super::conn_turn::{ConnectionTurn, DEFAULT_ACK_DELAY_EXPONENT};
use super::connection::{ConnectionConfig, QuicConnection};
use super::driver::ConnectionDriver;
use super::event_loop::DatagramEventLoop;
use super::key_agreement::{self, X25519_KEY_LEN};
use super::key_schedule::InitialKeys;
use super::loss::PacketNumberSpace;
use super::pto::LossDetection;
use super::recv_path::RecvKeyRing;
use super::request_pump::RequestPump;
use super::send_state::ConnectionSendState;
use super::stream_manager::StreamManagerConfig;
use super::tls_cert_verify::signature_scheme;
use super::tls_message::{self, ClientHello, Extension, Handshake, KeyShareEntry};
use super::transport_params::{TransportParameterError, TransportParameters};
use super::udp::DatagramTransport;
use super::version_nego::QUIC_VERSION_1;

/// The length, in bytes, of the Connection IDs the client invents for its first
/// flight. Eight bytes matches the RFC 9001 Appendix A example and is well
/// within the RFC 9000 §17.2 limit of 20.
pub const CLIENT_CONNECTION_ID_LEN: usize = 8;

/// The endpoint-tunable knobs the bootstrap folds into the ClientHello's
/// `quic_transport_parameters` (what the client is willing to receive) and into
/// the fresh [`QuicConnection`]'s pre-handshake configuration.
///
/// Every field carries a browser-reasonable default via [`Default`]; the QUIC
/// receive limits advertised here bound what the *server* may send us, and the
/// peer-side seeds in [`ConnectionConfig`] are provisional placeholders the
/// connection refines once the server's own transport parameters arrive.
#[derive(Clone, Copy, Debug)]
pub struct ClientConnectConfig {
    /// `max_idle_timeout` advertised to the peer (RFC 9000 §18.2). A connection
    /// is silently closed after this much idle time.
    pub max_idle_timeout: Duration,
    /// The UDP payload size stamped into outgoing datagrams (RFC 9000 §14.1).
    /// The client's first flight must not shrink below the 1200-byte floor.
    pub max_datagram_size: usize,
    /// `max_udp_payload_size` advertised to the peer (RFC 9000 §18.2): the
    /// largest datagram the client is prepared to receive.
    pub max_udp_payload_size: u64,
    /// `initial_max_data` — the connection-level flow-control budget the client
    /// grants the server across all streams (RFC 9000 §4.1).
    pub initial_max_data: u64,
    /// `initial_max_stream_data_bidi_local` — receive budget for a stream the
    /// client itself opened (RFC 9000 §4.1).
    pub initial_max_stream_data_bidi_local: u64,
    /// `initial_max_stream_data_bidi_remote` — receive budget for a bidi stream
    /// the server opened (RFC 9000 §4.1).
    pub initial_max_stream_data_bidi_remote: u64,
    /// `initial_max_stream_data_uni` — receive budget for a unidirectional
    /// stream the server opened (RFC 9000 §4.1).
    pub initial_max_stream_data_uni: u64,
    /// `initial_max_streams_bidi` — how many bidi streams the server may open
    /// (RFC 9000 §4.6).
    pub initial_max_streams_bidi: u64,
    /// `initial_max_streams_uni` — how many unidirectional streams the server
    /// may open; HTTP/3 needs at least three (RFC 9114 §6.2).
    pub initial_max_streams_uni: u64,
    /// `active_connection_id_limit` — how many of the client's Connection IDs
    /// the server may hold active at once (RFC 9000 §18.2).
    pub active_connection_id_limit: u64,
    /// The probe-timeout seed handed to loss detection before any RTT sample is
    /// taken (RFC 9002 §6.2.1).
    pub initial_pto: Duration,
}

impl ClientConnectConfig {
    /// The stream-manager receive limits this config advertises (RFC 9000 §18.2),
    /// as the [`StreamManagerConfig`] the request phase's [`RequestPump`] needs.
    ///
    /// These mirror the `quic_transport_parameters` the ClientHello carries
    /// ([`build_transport_parameters`]), so what the request pump believes it may
    /// *receive* matches what the peer was told it may *send*.
    #[must_use]
    pub fn stream_manager_config(&self) -> StreamManagerConfig {
        StreamManagerConfig {
            initial_max_stream_data_bidi_local: self.initial_max_stream_data_bidi_local,
            initial_max_stream_data_bidi_remote: self.initial_max_stream_data_bidi_remote,
            initial_max_stream_data_uni: self.initial_max_stream_data_uni,
            initial_max_data: self.initial_max_data,
            initial_max_streams_bidi: self.initial_max_streams_bidi,
            initial_max_streams_uni: self.initial_max_streams_uni,
        }
    }

    /// Build the request phase's [`RequestPump`] from this config
    /// ([`super::conn_connect::ConnectDriver::into_request_driver`] takes the pump
    /// the caller assembles).
    ///
    /// The pump's per-stream *send* window seed — how many request-body bytes the
    /// client may put on a stream before the server grows the window with a
    /// MAX_STREAM_DATA (RFC 9000 §19.10) — is the peer's advertised
    /// `initial_max_stream_data_bidi_remote`, which the client does not learn until
    /// the server's own transport parameters arrive. Until a later slice surfaces
    /// those from the completed handshake, it is seeded provisionally from this
    /// config's symmetric receive limit, the same way [`connect_client`] seeds the
    /// connection's peer flow-control limits before the handshake refines them. A
    /// request with no (or a small) body — the common `GET` — never reaches the
    /// seed, so the provisional value only matters for a large `POST` against a
    /// server that advertises a tighter window than assumed.
    #[must_use]
    pub fn request_pump(&self) -> RequestPump {
        RequestPump::new(self.stream_manager_config(), self.initial_max_stream_data_bidi_remote)
    }
}

impl Default for ClientConnectConfig {
    fn default() -> Self {
        Self {
            max_idle_timeout: Duration::from_secs(30),
            max_datagram_size: 1200,
            max_udp_payload_size: 1472,
            initial_max_data: 10 * 1024 * 1024,
            initial_max_stream_data_bidi_local: 1024 * 1024,
            initial_max_stream_data_bidi_remote: 1024 * 1024,
            initial_max_stream_data_uni: 1024 * 1024,
            initial_max_streams_bidi: 100,
            initial_max_streams_uni: 100,
            active_connection_id_limit: 8,
            initial_pto: Duration::from_millis(100),
        }
    }
}

/// Why [`connect_client`] could not assemble the first flight.
#[derive(Debug)]
pub enum BootstrapError {
    /// The OS entropy source was unavailable while generating a Connection ID,
    /// the ClientHello random, or the ephemeral X25519 private key.
    Entropy(getrandom::Error),
    /// The ClientHello or one of its length-prefixed fields overflowed its
    /// on-the-wire length prefix.
    ClientHello(tls_message::TlsError),
    /// The `quic_transport_parameters` extension could not be serialized.
    TransportParams(TransportParameterError),
}

impl core::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Entropy(e) => write!(f, "entropy source unavailable: {e}"),
            Self::ClientHello(e) => write!(f, "encoding ClientHello: {e:?}"),
            Self::TransportParams(e) => {
                write!(f, "serializing quic_transport_parameters: {e:?}")
            }
        }
    }
}

impl std::error::Error for BootstrapError {}

/// Draw `len` bytes of OS entropy into a fresh Connection ID (RFC 9000 §5.1).
fn random_connection_id(len: usize) -> Result<Vec<u8>, BootstrapError> {
    let mut id = vec![0u8; len];
    getrandom::getrandom(&mut id).map_err(BootstrapError::Entropy)?;
    Ok(id)
}

/// Encode a `server_name` extension body carrying a single `host_name`
/// (RFC 6066 §3): a `ServerNameList` of one `ServerName { name_type = 0,
/// HostName<1..2^16-1> }`.
fn sni_extension_body(server_name: &str) -> Vec<u8> {
    let host = server_name.as_bytes();
    let name_len = host.len() as u16;
    let list_len = 3 + host.len() as u16; // 1 (type) + 2 (length) + host
    let mut out = Vec::with_capacity(5 + host.len());
    out.extend_from_slice(&list_len.to_be_bytes());
    out.push(0x00); // name_type = host_name
    out.extend_from_slice(&name_len.to_be_bytes());
    out.extend_from_slice(host);
    out
}

/// Encode a `supported_groups` extension body naming only X25519
/// (RFC 8446 §4.2.7): the only group the [`key_agreement`] layer offers.
fn supported_groups_body() -> Vec<u8> {
    let groups = [tls_message::GROUP_X25519];
    let mut out = Vec::with_capacity(2 + groups.len() * 2);
    out.extend_from_slice(&((groups.len() * 2) as u16).to_be_bytes());
    for g in groups {
        out.extend_from_slice(&g.to_be_bytes());
    }
    out
}

/// Encode a `signature_algorithms` extension body (RFC 8446 §4.2.3) listing the
/// schemes the [`super::tls_cert_verify`] layer can verify a server
/// `CertificateVerify` under.
fn signature_algorithms_body() -> Vec<u8> {
    let schemes = [
        signature_scheme::ECDSA_SECP256R1_SHA256,
        signature_scheme::ECDSA_SECP384R1_SHA384,
        signature_scheme::ECDSA_SECP521R1_SHA512,
        signature_scheme::RSA_PSS_RSAE_SHA256,
        signature_scheme::RSA_PSS_RSAE_SHA384,
        signature_scheme::RSA_PSS_RSAE_SHA512,
    ];
    let mut out = Vec::with_capacity(2 + schemes.len() * 2);
    out.extend_from_slice(&((schemes.len() * 2) as u16).to_be_bytes());
    for s in schemes {
        out.extend_from_slice(&s.to_be_bytes());
    }
    out
}

/// Encode an `application_layer_protocol_negotiation` extension body offering
/// only the final `h3` token (RFC 7301, RFC 9114 §3.2): a `ProtocolNameList` of
/// one `ProtocolName<1..2^8-1>`.
fn alpn_h3_body() -> Vec<u8> {
    let proto = b"h3";
    let list_len = 1 + proto.len() as u16; // 1 (name length) + name
    let mut out = Vec::with_capacity(2 + 1 + proto.len());
    out.extend_from_slice(&list_len.to_be_bytes());
    out.push(proto.len() as u8);
    out.extend_from_slice(proto);
    out
}

/// The QUIC transport parameters the client advertises: its own receive limits
/// plus the mandatory `initial_source_connection_id` echoing `scid`
/// (RFC 9000 §7.3, §18).
fn build_transport_parameters(scid: &[u8], config: &ClientConnectConfig) -> TransportParameters {
    TransportParameters {
        original_destination_connection_id: None,
        max_idle_timeout_ms: Some(config.max_idle_timeout.as_millis() as u64),
        stateless_reset_token: None,
        max_udp_payload_size: Some(config.max_udp_payload_size),
        initial_max_data: Some(config.initial_max_data),
        initial_max_stream_data_bidi_local: Some(config.initial_max_stream_data_bidi_local),
        initial_max_stream_data_bidi_remote: Some(config.initial_max_stream_data_bidi_remote),
        initial_max_stream_data_uni: Some(config.initial_max_stream_data_uni),
        initial_max_streams_bidi: Some(config.initial_max_streams_bidi),
        initial_max_streams_uni: Some(config.initial_max_streams_uni),
        ack_delay_exponent: None,
        max_ack_delay_ms: None,
        disable_active_migration: false,
        preferred_address: None,
        active_connection_id_limit: Some(config.active_connection_id_limit),
        initial_source_connection_id: Some(scid.to_vec()),
        retry_source_connection_id: None,
        unknown: Vec::new(),
    }
}

/// Build the exact ClientHello handshake-message bytes (`msg_type · uint24
/// length · body`, RFC 8446 §4) the TLS bridge feeds into the Initial CRYPTO
/// stream and folds into the transcript hash.
///
/// The `key_share` carries the X25519 public key of `client_private`, so the
/// same private key must later reach [`TlsConnState::new`].
///
/// # Errors
///
/// [`BootstrapError::ClientHello`] if a length-prefixed field overflows, or
/// [`BootstrapError::TransportParams`] if the transport parameters cannot be
/// serialized.
fn build_client_hello_message(
    random: [u8; 32],
    client_private: &[u8; X25519_KEY_LEN],
    server_name: &str,
    scid: &[u8],
    config: &ClientConnectConfig,
) -> Result<Vec<u8>, BootstrapError> {
    let share = key_agreement::x25519_key_share(client_private);
    let key_share_body =
        KeyShareEntry::encode_client_hello(&[share]).map_err(BootstrapError::ClientHello)?;
    let supported_versions_body = tls_message::encode_supported_versions(&[tls_message::VERSION_TLS13])
        .map_err(BootstrapError::ClientHello)?;
    let transport_params_body = build_transport_parameters(scid, config)
        .serialize()
        .map_err(BootstrapError::TransportParams)?;

    let extensions = vec![
        Extension::new(tls_message::EXT_SERVER_NAME, sni_extension_body(server_name)),
        Extension::new(tls_message::EXT_SUPPORTED_GROUPS, supported_groups_body()),
        Extension::new(tls_message::EXT_SIGNATURE_ALGORITHMS, signature_algorithms_body()),
        Extension::new(tls_message::EXT_SUPPORTED_VERSIONS, supported_versions_body),
        Extension::new(tls_message::EXT_KEY_SHARE, key_share_body),
        Extension::new(tls_message::EXT_ALPN, alpn_h3_body()),
        Extension::new(tls_message::EXT_QUIC_TRANSPORT_PARAMETERS, transport_params_body),
    ];

    let hello = ClientHello {
        random,
        legacy_session_id: Vec::new(),
        cipher_suites: vec![tls_message::TLS_AES_128_GCM_SHA256],
        extensions,
    };
    let mut out = Vec::new();
    Handshake::ClientHello(hello)
        .encode(&mut out)
        .map_err(BootstrapError::ClientHello)?;
    Ok(out)
}

/// Assemble a ready-to-drive [`ConnectDriver`] for `server_name` over
/// `transport`, judging the server certificate against `trust_anchors` and the
/// wall clock `now_unix` (seconds since the Unix epoch).
///
/// The returned driver has the Initial space installed on both halves — sending
/// with the client secret, receiving with the server secret (RFC 9001 §5.2) —
/// and the TLS bridge seeded with a fresh ClientHello. The caller drives the
/// handshake with [`ConnectDriver::connect`], passing a `now` clock closure and
/// a per-turn budget.
///
/// # Errors
///
/// [`BootstrapError`] if the OS entropy source is unavailable or the ClientHello
/// cannot be encoded.
pub fn connect_client<T: DatagramTransport>(
    transport: T,
    server_name: &str,
    trust_anchors: Vec<OwnedTrustAnchor>,
    now: Instant,
    now_unix: i64,
    config: &ClientConnectConfig,
) -> Result<ConnectDriver<T>, BootstrapError> {
    // RFC 9000 §7.2: the client invents both Connection IDs for its first
    // flight. The DCID it picks seeds the Initial keys (RFC 9001 §5.2); the
    // SCID names the client to the peer and is echoed in its transport params.
    let dcid = random_connection_id(CLIENT_CONNECTION_ID_LEN)?;
    let scid = random_connection_id(CLIENT_CONNECTION_ID_LEN)?;

    let client_private =
        key_agreement::generate_x25519_private_key().map_err(BootstrapError::Entropy)?;
    let mut random = [0u8; 32];
    getrandom::getrandom(&mut random).map_err(BootstrapError::Entropy)?;

    let client_hello =
        build_client_hello_message(random, &client_private, server_name, &scid, config)?;

    // RFC 9001 §5.2: both Initial secrets derive from the client's chosen DCID.
    // The client protects its own packets with the client secret and removes
    // protection from the server's with the server secret.
    let initial = InitialKeys::derive(&dcid);
    let mut recv_keys = RecvKeyRing::new();
    recv_keys.install(PacketNumberSpace::Initial, initial.server);

    let connection = QuicConnection::new_client(
        ConnectionConfig {
            peer_initial_cid: dcid.clone(),
            local_initial_cid: scid.clone(),
            active_connection_id_limit: config.active_connection_id_limit,
            // Provisional seeds until the server's transport parameters arrive.
            peer_active_connection_id_limit: config.active_connection_id_limit,
            peer_initial_max_data: config.initial_max_data,
            peer_initial_max_streams_bidi: config.initial_max_streams_bidi,
            peer_initial_max_streams_uni: config.initial_max_streams_uni,
            pto: config.initial_pto,
        },
        now,
    );

    let recv_driver = ConnectionDriver::new(
        DatagramEventLoop::new(transport),
        connection,
        LossDetection::new(config.initial_pto),
        recv_keys,
        CLIENT_CONNECTION_ID_LEN,
    );

    let mut send = ConnectionSendState::new(
        QUIC_VERSION_1,
        dcid,
        scid,
        config.max_datagram_size,
    );
    send.install(PacketNumberSpace::Initial, initial.client);

    let turn = ConnectionTurn::new(
        recv_driver,
        send,
        config.max_datagram_size,
        DEFAULT_ACK_DELAY_EXPONENT,
    );
    let handshake = HandshakeDriver::new(turn);
    let tls = TlsConnState::new(client_private, client_hello);
    Ok(ConnectDriver::new(handshake, tls, server_name, now_unix, trust_anchors))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn transport() -> MockDatagramTransport {
        MockDatagramTransport::new(loopback(1), loopback(2))
    }

    /// Pull the one extension of `ty` out of a parsed ClientHello.
    fn extension(hello: &ClientHello, ty: u16) -> Option<&Extension> {
        hello.extensions.iter().find(|e| e.extension_type == ty)
    }

    fn parse_hello(bytes: &[u8]) -> ClientHello {
        let (msg, consumed) = Handshake::parse(bytes)
            .expect("ClientHello parses")
            .expect("a complete ClientHello");
        assert_eq!(consumed, bytes.len(), "the whole message is consumed");
        match msg {
            Handshake::ClientHello(ch) => ch,
            other => panic!("expected ClientHello, got {other:?}"),
        }
    }

    #[test]
    fn client_hello_carries_sni_alpn_and_key_share() {
        let private = [0x11u8; 32];
        let scid = vec![0xAA; CLIENT_CONNECTION_ID_LEN];
        let config = ClientConnectConfig::default();
        let bytes =
            build_client_hello_message([0xCD; 32], &private, "example.com", &scid, &config)
                .expect("ClientHello builds");
        let hello = parse_hello(&bytes);

        assert_eq!(hello.cipher_suites, vec![tls_message::TLS_AES_128_GCM_SHA256]);

        // SNI carries the host verbatim (RFC 6066 §3): after the 5-byte
        // ServerNameList/ServerName framing come the host bytes.
        let sni = extension(&hello, tls_message::EXT_SERVER_NAME).expect("SNI present");
        assert_eq!(&sni.data[5..], b"example.com");

        // ALPN offers exactly `h3` (RFC 9114 §3.2).
        let alpn = extension(&hello, tls_message::EXT_ALPN).expect("ALPN present");
        assert_eq!(alpn.data, vec![0x00, 0x03, 0x02, b'h', b'3']);

        // The key_share names X25519 and carries this key's public value.
        let ks = extension(&hello, tls_message::EXT_KEY_SHARE).expect("key_share present");
        let shares = KeyShareEntry::parse_client_hello(&ks.data).expect("key_share parses");
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].group, tls_message::GROUP_X25519);
        assert_eq!(shares[0].key_exchange, key_agreement::x25519_public_key(&private));
    }

    #[test]
    fn client_hello_offers_tls13_only() {
        let config = ClientConnectConfig::default();
        let bytes = build_client_hello_message(
            [0x01; 32],
            &[0x22; 32],
            "h3.example",
            &[0xBB; CLIENT_CONNECTION_ID_LEN],
            &config,
        )
        .expect("ClientHello builds");
        let hello = parse_hello(&bytes);

        let sv = extension(&hello, tls_message::EXT_SUPPORTED_VERSIONS).expect("supported_versions");
        let versions = tls_message::supported_versions(&sv.data).expect("versions parse");
        assert_eq!(versions, vec![tls_message::VERSION_TLS13]);

        let groups = extension(&hello, tls_message::EXT_SUPPORTED_GROUPS).expect("groups");
        assert_eq!(groups.data, vec![0x00, 0x02, 0x00, 0x1d]);
    }

    #[test]
    fn transport_parameters_echo_source_connection_id() {
        let scid = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let config = ClientConnectConfig::default();
        let bytes = build_client_hello_message([0x00; 32], &[0x33; 32], "srv", &scid, &config)
            .expect("ClientHello builds");
        let hello = parse_hello(&bytes);

        let tp_ext =
            extension(&hello, tls_message::EXT_QUIC_TRANSPORT_PARAMETERS).expect("transport params");
        let tp = TransportParameters::parse(&tp_ext.data).expect("transport params parse");
        assert_eq!(tp.initial_source_connection_id.as_deref(), Some(scid.as_slice()));
        assert_eq!(tp.active_connection_id_limit(), config.active_connection_id_limit);
        assert_eq!(tp.initial_max_data, Some(config.initial_max_data));
        assert_eq!(tp.initial_max_streams_uni, Some(config.initial_max_streams_uni));
    }

    #[test]
    fn connect_client_yields_an_unconfirmed_driver() {
        let now = Instant::now();
        let driver = connect_client(
            transport(),
            "example.com",
            Vec::new(),
            now,
            1_700_000_000,
            &ClientConnectConfig::default(),
        )
        .expect("bootstrap succeeds");

        // Fresh out of the bootstrap: nothing has been exchanged, so the peer
        // has neither confirmed the handshake nor presented a certificate.
        assert!(!driver.is_confirmed());
        assert!(driver.completed().is_none());
    }

    #[test]
    fn connect_client_invents_distinct_connection_ids() {
        // Two bootstraps must not collide on their random Connection IDs, so
        // their Initial-derived keys — and hence their first flights — differ.
        // We observe the randomness indirectly through the ClientHello's
        // `initial_source_connection_id`, which echoes the fresh SCID.
        let now = Instant::now();
        let config = ClientConnectConfig::default();
        let a = connect_client(transport(), "a.example", Vec::new(), now, 0, &config)
            .expect("bootstrap a");
        let b = connect_client(transport(), "a.example", Vec::new(), now, 0, &config)
            .expect("bootstrap b");
        // The drivers own their transports; distinctness is asserted at the
        // ClientHello layer instead, which is deterministic given the SCID.
        drop((a, b));

        let hello_a = build_client_hello_message(
            [0; 32],
            &[1; 32],
            "a.example",
            &random_connection_id(CLIENT_CONNECTION_ID_LEN).unwrap(),
            &config,
        )
        .unwrap();
        let hello_b = build_client_hello_message(
            [0; 32],
            &[1; 32],
            "a.example",
            &random_connection_id(CLIENT_CONNECTION_ID_LEN).unwrap(),
            &config,
        )
        .unwrap();
        assert_ne!(hello_a, hello_b, "distinct SCIDs yield distinct ClientHellos");
    }
}
