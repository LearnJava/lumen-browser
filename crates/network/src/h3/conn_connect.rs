//! QUIC connect loop (RFC 9000 §7, §10; RFC 9001 §4.1, §4.9; RFC 8446 §4.4.4;
//! h3::conn_connect): the top slice of the opening handshake — it joins the two
//! halves the previous two slices deliberately left apart, the *control flow* and
//! the *TLS state machine*, into one "connect until the handshake confirms" loop.
//!
//! ## The seam this slice closes
//!
//! Two slices sit just below this one, each documenting the join it leaves to a
//! caller:
//!
//! - [`conn_handshake::HandshakeDriver`](super::conn_handshake::HandshakeDriver)
//!   owns the *control flow*: it repeats wait → ingest + acknowledge | dispatch →
//!   flush until the peer confirms the handshake, a terminal timer ends the
//!   connection, or a turn budget is spent. It explicitly defers "advancing the TLS
//!   state machine itself", reached only through
//!   [`turn_mut`](super::conn_handshake::HandshakeDriver::turn_mut) "between polls,
//!   or by a later slice that wires the TLS handshake driver to this loop".
//! - [`conn_tls::TlsConnState`](super::conn_tls::TlsConnState) owns the *TLS bridge*:
//!   [`send_client_hello`](super::conn_tls::TlsConnState::send_client_hello) enqueues
//!   the first flight and [`advance`](super::conn_tls::TlsConnState::advance) drains
//!   the reassembled CRYPTO, derives each level's keys, installs them on both halves,
//!   and enqueues the client Finished. It expects "a caller (or the enclosing
//!   handshake loop)" to call [`advance`](super::conn_tls::TlsConnState::advance)
//!   "whenever the connection may have reassembled new CRYPTO".
//!
//! This slice is that caller. [`ConnectDriver`] holds both and interleaves them:
//! after every poll that ingested a datagram — the only wake that can reassemble new
//! CRYPTO — it drives the TLS state machine over what arrived and flushes the CRYPTO
//! it enqueues. The result is a single [`ConnectDriver::connect`] call that opens the
//! connection end to end: it sends the client's first flight, feeds each server
//! flight into the TLS handshake, installs the Handshake and 1-RTT keys as they are
//! derived, sends the client Finished on completion, and returns when the peer
//! confirms with HANDSHAKE_DONE.
//!
//! ## One turn
//!
//! [`ConnectDriver::poll`] runs one [`HandshakeDriver::poll`](super::conn_handshake::HandshakeDriver::poll)
//! turn and, when it reports [`PollOutcome::Ingested`](super::conn_handshake::PollOutcome::Ingested),
//! calls [`TlsConnState::advance`](super::conn_tls::TlsConnState::advance) on the same
//! [`ConnectionTurn`](super::conn_turn::ConnectionTurn). The poll already flushed the
//! datagram's owed acknowledgement; the TLS advance may install the Handshake-level
//! keys (on the ServerHello) or complete the handshake (on the server Finished),
//! enqueuing the client Finished in a Handshake-level CRYPTO frame (RFC 8446 §4.4.4)
//! — this slice flushes that outgoing CRYPTO the moment it appears. The
//! [`HandshakeComplete`](super::tls_handshake::HandshakeComplete) the completion
//! carries — the server certificate and CertificateVerify — is retained
//! ([`ConnectDriver::completed`]) for the caller to authenticate with
//! [`tls_cert_verify`](super::tls_cert_verify) before the connection carries
//! application data.
//!
//! ## What it defers
//!
//! - **Certificate authentication.** Like the two slices below it, this loop hands
//!   the server certificate back rather than chaining it to a trust anchor; the
//!   caller (or a later slice) authenticates it.
//! - **Building the ClientHello.** The exact first-flight bytes and the ephemeral
//!   X25519 private key are supplied to [`TlsConnState::new`](super::conn_tls::TlsConnState::new)
//!   by the caller.
//! - **Request dispatch.** Once [`connect`](ConnectDriver::connect) reports
//!   [`ConnectOutcome::Confirmed`], [`ConnectDriver::into_parts`] hands the
//!   [`ConnectionTurn`](super::conn_turn::ConnectionTurn) and the server certificate
//!   on to the `h3_do_request` slice.
//!
//! ## Purity
//!
//! Like every slice below it, this module reads no clock of its own:
//! [`connect`](ConnectDriver::connect) takes a clock closure it calls once per turn,
//! and every timer decision, ACK timestamp, and sent-packet stamp flows from it. A
//! synthetic clock and a [`udp::MockDatagramTransport`](super::udp::MockDatagramTransport)
//! carrying scripted, encrypted server flights drive a whole handshake
//! deterministically in tests.

use std::time::Instant;

use super::conn_handshake::{HandshakeDriver, HandshakeError, PollOutcome};
use super::conn_tls::{TlsConnError, TlsConnState};
use super::conn_turn::{ConnectionTurn, TurnEffect};
use super::send_path::FlushError;
use super::tls_handshake::HandshakeComplete;
use super::udp::DatagramTransport;

/// What one [`ConnectDriver::poll`] turn did: the transport-level outcome of the
/// [`HandshakeDriver::poll`](super::conn_handshake::HandshakeDriver::poll) plus a
/// summary of the TLS progress the ingested CRYPTO made this turn.
///
/// A timer turn advances no TLS, so its `messages_processed` is zero and its flags
/// are `false`; a datagram turn reports how the server flight it carried moved the
/// handshake along.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectStep {
    /// The transport-level outcome of the underlying handshake poll: a datagram
    /// ingested and acknowledged, or the timers that fired.
    pub poll: PollOutcome,
    /// The number of complete TLS handshake messages framed from the newly
    /// reassembled CRYPTO and fed to the state machine this turn (zero on a timer
    /// turn or a datagram carrying no CRYPTO).
    pub messages_processed: usize,
    /// Whether the Handshake-level packet keys were derived and installed this turn
    /// (the ServerHello was processed, RFC 9001 §5.1).
    pub handshake_keys_installed: bool,
    /// Whether the TLS handshake completed this turn (the server Finished verified):
    /// the 1-RTT keys were installed, the client Finished was enqueued and flushed,
    /// and the server certificate is now available from [`ConnectDriver::completed`].
    pub completed: bool,
}

/// Why [`ConnectDriver::connect`] stopped opening the connection.
///
/// Mirrors [`conn_handshake::HandshakeOutcome`](super::conn_handshake::HandshakeOutcome):
/// the TLS handshake itself completing (the server Finished verifying) is not a stop
/// condition — the loop runs on until the peer *confirms* with HANDSHAKE_DONE — so
/// the server certificate for authentication is read from [`ConnectDriver::completed`]
/// rather than carried here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// The peer confirmed the handshake (HANDSHAKE_DONE received, RFC 9000 §19.20).
    /// The connection is ready for `h3_do_request` dispatch;
    /// [`ConnectDriver::into_parts`] hands the turn and the authenticated server
    /// certificate on.
    Confirmed,
    /// A terminal timer ended the connection before it confirmed: the idle timeout
    /// elapsed ([`TurnEffect::IdleTimeout`], RFC 9000 §10.1) or the closing /
    /// draining period expired ([`TurnEffect::Drained`], RFC 9000 §10.2). Carries
    /// which one.
    Terminated(TurnEffect),
    /// The turn budget was spent without confirming the handshake or ending the
    /// connection. The caller may `connect` again with more turns or give up and
    /// fall back to the H2 / H1.1 path.
    Incomplete,
}

/// Why a step of the connect loop failed.
#[derive(Debug)]
pub enum ConnectError {
    /// The control-flow turn failed: a socket error from the wait, an authenticated
    /// connection error from the ingest, a rejected send action, or a failed flush
    /// ([`HandshakeError`](super::conn_handshake::HandshakeError)).
    Loop(HandshakeError),
    /// The TLS state machine rejected a server handshake message, the reassembled
    /// CRYPTO was malformed, or the outgoing CRYPTO could not be enqueued
    /// ([`TlsConnError`](super::conn_tls::TlsConnError)).
    Tls(TlsConnError),
    /// Flushing the client Finished (and any owed Handshake acknowledgement) failed
    /// ([`FlushError`](super::send_path::FlushError)).
    Flush(FlushError),
}

impl core::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Loop(e) => write!(f, "QUIC connect: {e}"),
            Self::Tls(e) => write!(f, "QUIC connect: {e}"),
            Self::Flush(e) => write!(f, "QUIC connect: flush failed: {e}"),
        }
    }
}

impl std::error::Error for ConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Loop(e) => Some(e),
            Self::Tls(e) => Some(e),
            Self::Flush(e) => Some(e),
        }
    }
}

/// Drives one QUIC connection through its opening handshake to confirmation by
/// interleaving the control-flow loop ([`HandshakeDriver`]) with the TLS state
/// machine ([`TlsConnState`]).
///
/// Seed it with [`ConnectDriver::new`] from a [`HandshakeDriver`] whose
/// [`ConnectionTurn`](super::conn_turn::ConnectionTurn) has the Initial space
/// installed on both halves and a [`TlsConnState`] carrying the ClientHello, then
/// call [`connect`](ConnectDriver::connect). It sends the first flight, feeds each
/// server flight into the TLS handshake, installs the Handshake and 1-RTT keys as
/// they are derived, sends the client Finished on completion, and returns when the
/// peer confirms. After [`ConnectOutcome::Confirmed`] the authenticated server
/// certificate is at [`completed`](ConnectDriver::completed) and the driven turn is
/// recovered with [`into_parts`](ConnectDriver::into_parts).
#[derive(Debug)]
pub struct ConnectDriver<T: DatagramTransport> {
    /// The control-flow loop: wait / ingest / acknowledge / dispatch / flush, run
    /// one turn at a time by [`poll`](ConnectDriver::poll).
    handshake: HandshakeDriver<T>,
    /// The TLS-to-QUIC bridge: enqueues the ClientHello, drains reassembled CRYPTO,
    /// derives and installs keys, and enqueues the client Finished.
    tls: TlsConnState,
    /// The completion the TLS handshake produced when the server Finished verified —
    /// the server certificate and CertificateVerify for the caller to authenticate,
    /// plus the 1-RTT key material. `None` until the handshake completes.
    completed: Option<Box<HandshakeComplete>>,
}

impl<T: DatagramTransport> ConnectDriver<T> {
    /// Joins a control-flow `handshake` loop and a `tls` bridge into one connect
    /// driver. The handshake's turn should already have the Initial space installed
    /// on both halves; the first flight is sent on the first turn of
    /// [`connect`](ConnectDriver::connect).
    #[must_use]
    pub fn new(handshake: HandshakeDriver<T>, tls: TlsConnState) -> Self {
        Self { handshake, tls, completed: None }
    }

    /// The control-flow loop, borrowed immutably (e.g. to read whether the handshake
    /// has confirmed or the connection lifecycle).
    #[must_use]
    pub fn handshake(&self) -> &HandshakeDriver<T> {
        &self.handshake
    }

    /// The TLS bridge, borrowed immutably (e.g. to read the handshake
    /// [`state`](super::tls_handshake::ClientHandshake::state) or whether the client
    /// Finished has been sent).
    #[must_use]
    pub fn tls(&self) -> &TlsConnState {
        &self.tls
    }

    /// The authenticated-pending server certificate material, available once the TLS
    /// handshake completes (the server Finished verified). `None` while the handshake
    /// is still in progress.
    ///
    /// The caller authenticates
    /// [`server_certificate`](super::tls_handshake::HandshakeComplete::server_certificate)
    /// and
    /// [`server_certificate_verify`](super::tls_handshake::HandshakeComplete::server_certificate_verify)
    /// with [`tls_cert_verify`](super::tls_cert_verify) before the connection carries
    /// application data.
    #[must_use]
    pub fn completed(&self) -> Option<&HandshakeComplete> {
        self.completed.as_deref()
    }

    /// Whether the peer has confirmed the handshake (HANDSHAKE_DONE received,
    /// RFC 9000 §19.20).
    #[must_use]
    pub fn is_confirmed(&self) -> bool {
        self.handshake.is_confirmed()
    }

    /// Splits the driver into the connection turn it drove and the server
    /// certificate material, to hand both to the `h3_do_request` slice once the
    /// handshake has confirmed.
    #[must_use]
    pub fn into_parts(self) -> (ConnectionTurn<T>, Option<Box<HandshakeComplete>>) {
        (self.handshake.into_turn(), self.completed)
    }

    /// Sends the client's first-flight ClientHello as its own padded Initial datagram
    /// (RFC 9000 §14.1, RFC 9001 §4.1). Idempotent: the enqueue is a no-op once the
    /// hello has been sent, and the padded-Initial flush sends nothing when the
    /// Initial space has nothing queued.
    fn send_first_flight(&mut self, now: Instant) -> Result<(), ConnectError> {
        self.tls
            .send_client_hello(self.handshake.turn_mut())
            .map_err(ConnectError::Tls)?;
        self.handshake
            .turn_mut()
            .send_padded_initial(now)
            .map_err(ConnectError::Flush)?;
        Ok(())
    }

    /// Runs one turn of the connect loop at `now`: one control-flow poll, then — when
    /// it ingested a datagram — the TLS advance over any CRYPTO that arrived.
    ///
    /// On a datagram wake the poll ingests and acknowledges the packet; this then
    /// drives the TLS state machine over the newly reassembled CRYPTO, installing the
    /// Handshake keys (on the ServerHello) or completing the handshake (on the server
    /// Finished). When the handshake completes it retains the server certificate
    /// ([`completed`](ConnectDriver::completed)) and flushes the client Finished the
    /// advance enqueued (RFC 8446 §4.4.4). On a timer wake it advances no TLS.
    ///
    /// # Errors
    ///
    /// [`ConnectError`] wrapping the failing step: a control-flow error from the
    /// poll, a TLS error from the advance, or a failed flush of the client Finished.
    pub fn poll(&mut self, now: Instant) -> Result<ConnectStep, ConnectError> {
        let poll = self.handshake.poll(now).map_err(ConnectError::Loop)?;
        let mut messages_processed = 0;
        let mut handshake_keys_installed = false;
        let mut completed = false;
        // Only a datagram wake can have reassembled new CRYPTO; a timer wake advances
        // no TLS.
        if matches!(poll, PollOutcome::Ingested { .. }) {
            let mut adv = self
                .tls
                .advance(self.handshake.turn_mut())
                .map_err(ConnectError::Tls)?;
            messages_processed = adv.messages_processed;
            handshake_keys_installed = adv.handshake_keys_installed;
            if let Some(complete) = adv.completed.take() {
                // The client Finished (RFC 8446 §4.4.4) was enqueued into the
                // Handshake CRYPTO stream; put it — and any owed Handshake ACK the
                // poll queued — on the wire now.
                self.handshake
                    .turn_mut()
                    .flush(now)
                    .map_err(ConnectError::Flush)?;
                self.completed = Some(complete);
                completed = true;
            }
        }
        Ok(ConnectStep { poll, messages_processed, handshake_keys_installed, completed })
    }

    /// Drives the connect loop until the peer confirms the handshake, a terminal
    /// timer ends the connection, or `max_turns` turns are spent, reading `clock`
    /// once per turn for the wall-clock instant each turn acts at.
    ///
    /// The first turn sends the client's first flight
    /// ([`send_first_flight`](ConnectDriver::send_first_flight)) before its poll;
    /// every turn thereafter feeds the server's response into the TLS handshake. It
    /// returns [`ConnectOutcome::Confirmed`] the moment
    /// [`is_confirmed`](ConnectDriver::is_confirmed) is true (checked before each
    /// turn, so an already-confirmed connection returns without any I/O),
    /// [`ConnectOutcome::Terminated`] when a turn surfaces a terminal
    /// [`TurnEffect`], or [`ConnectOutcome::Incomplete`] when the budget is spent.
    ///
    /// `max_turns` bounds the loop so a peer that never completes the handshake
    /// cannot spin it forever; a caller that gets [`ConnectOutcome::Incomplete`]
    /// decides whether to `connect` again or fall back to H2 / H1.1.
    ///
    /// # Errors
    ///
    /// The first [`ConnectError`] any turn produces.
    pub fn connect(
        &mut self,
        mut clock: impl FnMut() -> Instant,
        max_turns: usize,
    ) -> Result<ConnectOutcome, ConnectError> {
        for _ in 0..max_turns {
            if self.is_confirmed() {
                return Ok(ConnectOutcome::Confirmed);
            }
            let now = clock();
            if !self.tls.client_hello_sent() {
                self.send_first_flight(now)?;
            }
            let step = self.poll(now)?;
            if let PollOutcome::Timers(effects) = step.poll
                && let Some(terminal) = effects.into_iter().find(TurnEffect::is_terminal)
            {
                return Ok(ConnectOutcome::Terminated(terminal));
            }
        }
        if self.is_confirmed() {
            Ok(ConnectOutcome::Confirmed)
        } else {
            Ok(ConnectOutcome::Incomplete)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::conn_handshake::PollOutcome;
    use crate::h3::conn_turn::{ConnectionTurn, DEFAULT_ACK_DELAY_EXPONENT};
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::ConnectionDriver;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::key_agreement;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::loss::PacketNumberSpace;
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::pto::LossDetection;
    use crate::h3::quic_frame::{self, Frame};
    use crate::h3::recv_path::RecvKeyRing;
    use crate::h3::send_state::ConnectionSendState;
    use crate::h3::tls_handshake::HandshakeState;
    use crate::h3::tls_message::{
        self, Certificate, CertificateEntry, ClientHello, Extension, Handshake, KeyShareEntry,
        ServerHello,
    };
    use crate::h3::tls_schedule::{self, HandshakeTrafficSecrets};
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::{Duration, Instant};

    /// A fixed client ephemeral X25519 private key for reproducible tests.
    const CLIENT_PRIV: [u8; 32] = [0x11; 32];
    /// A fixed server ephemeral X25519 private key for reproducible tests.
    const SERVER_PRIV: [u8; 32] = [0x22; 32];

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

    fn initial_keys() -> InitialKeys {
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
        recv_keys.install(PacketNumberSpace::Initial, initial_keys().client);
        ConnectionDriver::new(
            DatagramEventLoop::new(t),
            connection(now),
            LossDetection::new(Duration::from_millis(25)),
            recv_keys,
            4,
        )
    }

    /// A connect driver over the scripted transport `t`, with the Initial space
    /// installed on both halves and the TLS bridge seeded with the ClientHello.
    fn connect_driver(
        t: MockDatagramTransport,
        now: Instant,
    ) -> ConnectDriver<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), scid(), 1200);
        send.install(PacketNumberSpace::Initial, initial_keys().client);
        let turn = ConnectionTurn::new(driver(t, now), send, 1200, DEFAULT_ACK_DELAY_EXPONENT);
        let handshake = HandshakeDriver::new(turn);
        let tls = TlsConnState::new(CLIENT_PRIV, client_hello_bytes());
        ConnectDriver::new(handshake, tls)
    }

    // ---- TLS handshake message fixtures (mirroring conn_tls) ------------

    fn enc(msg: &Handshake) -> Vec<u8> {
        let mut out = Vec::new();
        msg.encode(&mut out).expect("fixture message encodes");
        out
    }

    fn client_hello_bytes() -> Vec<u8> {
        let share = key_agreement::x25519_key_share(&CLIENT_PRIV);
        let key_share_body =
            KeyShareEntry::encode_client_hello(&[share]).expect("key_share encodes");
        enc(&Handshake::ClientHello(ClientHello {
            random: [0xAB; 32],
            legacy_session_id: Vec::new(),
            cipher_suites: vec![tls_message::TLS_AES_128_GCM_SHA256],
            extensions: vec![Extension::new(tls_message::EXT_KEY_SHARE, key_share_body)],
        }))
    }

    fn server_hello_bytes() -> Vec<u8> {
        let share = key_agreement::x25519_key_share(&SERVER_PRIV);
        let key_share_body = share.encode_server_hello().expect("key_share encodes");
        enc(&Handshake::ServerHello(ServerHello {
            random: [0xCD; 32],
            legacy_session_id_echo: Vec::new(),
            cipher_suite: tls_message::TLS_AES_128_GCM_SHA256,
            extensions: vec![
                Extension::new(
                    tls_message::EXT_SUPPORTED_VERSIONS,
                    tls_message::VERSION_TLS13.to_be_bytes().to_vec(),
                ),
                Extension::new(tls_message::EXT_KEY_SHARE, key_share_body),
            ],
        }))
    }

    fn encrypted_extensions_bytes() -> Vec<u8> {
        enc(&Handshake::EncryptedExtensions(Vec::new()))
    }

    fn certificate_bytes() -> Vec<u8> {
        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![CertificateEntry {
                cert_data: vec![0x30, 0x03, 0x01, 0x02, 0x03],
                extensions: Vec::new(),
            }],
        }))
    }

    fn certificate_verify_bytes() -> Vec<u8> {
        enc(&Handshake::CertificateVerify(tls_message::CertificateVerify {
            algorithm: 0x0804,
            signature: vec![0xDE, 0xAD, 0xBE, 0xEF],
        }))
    }

    /// Derive the Handshake traffic secrets exactly as the TLS bridge does, both to
    /// forge a matching server Finished and to protect the server's Handshake-level
    /// packets with the keys the client will install.
    fn handshake_traffic(ch: &[u8], sh: &[u8]) -> HandshakeTrafficSecrets {
        let ecdhe = key_agreement::x25519_shared_secret(
            &CLIENT_PRIV,
            &key_agreement::x25519_public_key(&SERVER_PRIV),
        )
        .expect("shared secret");
        let hs_secret = tls_schedule::handshake_secret(&ecdhe);
        let mut transcript = ch.to_vec();
        transcript.extend_from_slice(sh);
        let th = tls_schedule::transcript_hash(&transcript);
        HandshakeTrafficSecrets::derive(&hs_secret, &th)
    }

    fn server_finished_bytes(server_secret: &[u8; 32], transcript_ch_cv: &[u8]) -> Vec<u8> {
        let th = tls_schedule::transcript_hash(transcript_ch_cv);
        let vd = crate::h3::tls_finished::finished_verify_data(server_secret, &th);
        enc(&Handshake::Finished(vd.to_vec()))
    }

    /// The full Handshake-level server flight (EE, Certificate, CertificateVerify,
    /// Finished) as one concatenated CRYPTO payload.
    fn handshake_flight(ch: &[u8], sh: &[u8]) -> Vec<u8> {
        let ee = encrypted_extensions_bytes();
        let cert = certificate_bytes();
        let cv = certificate_verify_bytes();
        let hs_traffic = handshake_traffic(ch, sh);
        let mut transcript_ch_cv = ch.to_vec();
        for m in [sh, &ee, &cert, &cv] {
            transcript_ch_cv.extend_from_slice(m);
        }
        let sf = server_finished_bytes(&hs_traffic.server, &transcript_ch_cv);
        let mut flight = Vec::new();
        for m in [&ee, &cert, &cv, &sf] {
            flight.extend_from_slice(m);
        }
        flight
    }

    // ---- encrypted server packet builders ------------------------------

    /// Encrypt a server Initial packet carrying `crypto` at Initial CRYPTO offset 0.
    /// Protected with the Initial keys derived from the client's DCID (RFC 9001
    /// §5.2); mirrors the conn_handshake fixture pattern of using the `client` keys
    /// for the loopback direction.
    fn server_initial(pn: u64, crypto: Vec<u8>) -> Vec<u8> {
        let dcid = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(&[Frame::Crypto { offset: 0, data: crypto }], &mut payload)
            .expect("encode frames");
        encrypt_packet(&initial_keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    /// Encrypt a server Handshake packet carrying `crypto` at Handshake CRYPTO offset
    /// 0, protected with the server-direction Handshake keys the client installs on
    /// the ServerHello (RFC 9001 §5.1).
    fn server_handshake(pn: u64, crypto: Vec<u8>, ch: &[u8], sh: &[u8]) -> Vec<u8> {
        let keys = handshake_traffic(ch, sh).packet_keys().server;
        let dcid = dcid();
        let header = ProtectedHeader::Handshake { version: 1, dcid: &dcid, scid: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(&[Frame::Crypto { offset: 0, data: crypto }], &mut payload)
            .expect("encode frames");
        encrypt_packet(&keys, &header, pn, None, &payload).expect("encrypt")
    }

    fn sent_count(cd: &mut ConnectDriver<MockDatagramTransport>) -> usize {
        cd.handshake
            .turn_mut()
            .driver_mut()
            .events_mut()
            .transport_mut()
            .sent
            .len()
    }

    // ---- construction / accessors --------------------------------------

    #[test]
    fn new_exposes_the_parts_and_no_completion() {
        let now = base();
        let cd = connect_driver(transport(), now);
        assert!(!cd.is_confirmed());
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().is_installed(PacketNumberSpace::Initial));
        assert!(!cd.tls().client_hello_sent());
    }

    #[test]
    fn into_parts_recovers_the_turn_and_no_certificate_yet() {
        let now = base();
        let cd = connect_driver(transport(), now);
        let (turn, cert) = cd.into_parts();
        assert!(turn.send().is_installed(PacketNumberSpace::Initial));
        assert!(cert.is_none());
    }

    // ---- connect: first flight -----------------------------------------

    #[test]
    fn connect_sends_the_padded_client_hello_on_the_first_turn() {
        let now = base();
        // The empty transport reports the timer signal each turn; the connection
        // never confirms, so connect spends its budget and returns Incomplete after
        // sending the first flight once.
        let mut cd = connect_driver(transport(), now);
        let outcome = cd.connect(|| now, 2).expect("connect runs");
        assert_eq!(outcome, ConnectOutcome::Incomplete);
        assert!(cd.tls().client_hello_sent(), "the ClientHello was enqueued");
        assert_eq!(sent_count(&mut cd), 1, "exactly one padded Initial went out");
    }

    // ---- poll: ServerHello installs the Handshake keys -----------------

    #[test]
    fn poll_over_server_hello_installs_the_handshake_keys() {
        let now = base();
        let mut t = transport();
        t.push_inbound(server_initial(0, server_hello_bytes()));
        let mut cd = connect_driver(t, now);

        let step = cd.poll(now).expect("poll succeeds");
        assert!(matches!(step.poll, PollOutcome::Ingested { .. }));
        assert_eq!(step.messages_processed, 1);
        assert!(step.handshake_keys_installed);
        assert!(!step.completed);
        assert_eq!(cd.tls().tls().state(), HandshakeState::ExpectEncryptedExtensions);
        assert!(cd.handshake().turn().send().is_installed(PacketNumberSpace::Handshake));
        assert!(cd.completed().is_none());
    }

    // ---- poll: completion flushes the client Finished ------------------

    #[test]
    fn poll_completes_the_handshake_and_flushes_the_client_finished() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The server's two flights arrive in order: the Initial ServerHello unlocks
        // the Handshake keys, then the Handshake flight completes the handshake.
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        let mut cd = connect_driver(t, now);

        // Turn 1: ServerHello.
        let step1 = cd.poll(now).expect("poll over the ServerHello");
        assert!(step1.handshake_keys_installed);
        assert!(!step1.completed);

        // Turn 2: the rest of the flight completes the handshake.
        let step2 = cd.poll(now).expect("poll over the flight");
        assert_eq!(step2.messages_processed, 4, "EE, Certificate, CertificateVerify, Finished");
        assert!(step2.completed);
        assert!(cd.tls().is_complete());
        // The server certificate is now available for authentication.
        let complete = cd.completed().expect("completion retained");
        assert_eq!(complete.server_certificate.certificate_list.len(), 1);
        // The 1-RTT keys are installed and the client Finished was flushed (the
        // Handshake CRYPTO queue drained).
        assert!(cd.handshake().turn().send().is_installed(PacketNumberSpace::ApplicationData));
        assert!(!cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- connect: drive to confirmation --------------------------------

    #[test]
    fn connect_returns_confirmed_after_handshake_done() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        let mut cd = connect_driver(t, now);

        // Complete the TLS handshake over the two scripted flights.
        cd.poll(now).expect("ServerHello");
        cd.poll(now).expect("flight");
        assert!(cd.tls().is_complete());

        // The peer confirms with HANDSHAKE_DONE (1-RTT, RFC 9000 §19.20). The 1-RTT
        // keys were installed on completion, so process it directly to stand in for
        // the encrypted 1-RTT packet the transport would deliver.
        cd.handshake
            .turn_mut()
            .driver_mut()
            .connection_mut()
            .process_packet(PacketNumberSpace::ApplicationData, 0, &[Frame::HandshakeDone], now)
            .expect("handshake-done processes");

        let outcome = cd.connect(|| now, 4).expect("connect runs");
        assert_eq!(outcome, ConnectOutcome::Confirmed);
    }

    #[test]
    fn connect_returns_confirmed_when_already_confirmed() {
        let now = base();
        let mut t = transport();
        t.push_inbound(server_initial(0, server_hello_bytes()));
        let mut cd = connect_driver(t, now);
        // Confirm directly before any turn runs.
        cd.handshake
            .turn_mut()
            .driver_mut()
            .connection_mut()
            .process_packet(PacketNumberSpace::ApplicationData, 0, &[Frame::HandshakeDone], now)
            .expect("handshake-done processes");

        let outcome = cd.connect(|| now, 4).expect("connect runs");
        assert_eq!(outcome, ConnectOutcome::Confirmed);
        // Confirmed is checked before the first turn, so no first flight was sent.
        assert!(!cd.tls().client_hello_sent());
        assert_eq!(sent_count(&mut cd), 0, "an already-confirmed connection is not driven");
    }

    // ---- connect: terminal + budget stops ------------------------------

    #[test]
    fn connect_stops_on_idle_timeout() {
        let now = base();
        let mut cd = connect_driver(transport(), now);
        cd.handshake
            .turn_mut()
            .driver_mut()
            .connection_mut()
            .set_idle_timeout(Some(Duration::from_millis(200)));
        // Disarm the anti-deadlock PTO so only the idle timer is left to fire.
        cd.handshake
            .turn_mut()
            .driver_mut()
            .loss_mut()
            .set_peer_completed_address_validation(true);

        let times = [now, now + Duration::from_secs(1)];
        let mut i = 0usize;
        let clock = || {
            let t = times[i.min(times.len() - 1)];
            i += 1;
            t
        };

        let outcome = cd.connect(clock, 5).expect("connect runs");
        assert_eq!(outcome, ConnectOutcome::Terminated(TurnEffect::IdleTimeout));
        assert!(!cd.is_confirmed());
    }

    #[test]
    fn connect_returns_incomplete_when_the_budget_is_spent() {
        let now = base();
        let mut cd = connect_driver(transport(), now);
        cd.handshake
            .turn_mut()
            .driver_mut()
            .loss_mut()
            .set_peer_completed_address_validation(true);

        let outcome = cd.connect(|| now, 3).expect("connect runs");
        assert_eq!(outcome, ConnectOutcome::Incomplete);
    }

    // ---- poll: timer turn advances no TLS ------------------------------

    #[test]
    fn poll_on_a_timer_turn_advances_no_tls() {
        let now = base();
        let mut cd = connect_driver(transport(), now);
        cd.handshake
            .turn_mut()
            .driver_mut()
            .connection_mut()
            .set_idle_timeout(Some(Duration::from_millis(500)));

        let step = cd.poll(now).expect("poll succeeds");
        assert_eq!(step.poll, PollOutcome::Timers(Vec::new()));
        assert_eq!(step.messages_processed, 0);
        assert!(!step.handshake_keys_installed);
        assert!(!step.completed);
    }

    // ---- poll: error propagation ---------------------------------------

    #[test]
    fn poll_propagates_an_ingest_connection_error() {
        let now = base();
        // HANDSHAKE_DONE is 1-RTT only; in an Initial it is a PROTOCOL_VIOLATION.
        let mut t = transport();
        let dcid = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(&[Frame::HandshakeDone, Frame::Padding(24)], &mut payload)
            .expect("encode frames");
        let bad = encrypt_packet(&initial_keys().client, &header, 0, None, &payload).expect("enc");
        t.push_inbound(bad);
        let mut cd = connect_driver(t, now);

        let err = cd.poll(now).expect_err("authenticated connection error surfaces");
        match err {
            ConnectError::Loop(HandshakeError::Ingest(e)) => assert_eq!(e.code(), 0x0a),
            other => panic!("expected an ingest error, got {other:?}"),
        }
    }

    #[test]
    fn poll_propagates_a_tls_rejection() {
        let now = base();
        let mut t = transport();
        // EncryptedExtensions before the ServerHello is out of order: the CRYPTO
        // decrypts and reassembles, but the TLS state machine rejects it.
        t.push_inbound(server_initial(0, encrypted_extensions_bytes()));
        let mut cd = connect_driver(t, now);

        let err = cd.poll(now).expect_err("out-of-order TLS message");
        assert!(matches!(err, ConnectError::Tls(TlsConnError::Handshake(_))));
        assert_eq!(cd.tls().tls().state(), HandshakeState::Failed);
    }
}
