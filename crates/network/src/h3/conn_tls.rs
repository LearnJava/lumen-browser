//! Wires the client TLS 1.3 handshake state machine to a QUIC connection's CRYPTO
//! streams (RFC 9001 §4, §4.9; h3::conn_tls): the slice the handshake driving-loop
//! ([`conn_handshake`](super::conn_handshake)) deliberately left to a caller —
//! "feed the reassembled CRYPTO to the handshake, derive each level's keys, install
//! them on both halves, and enqueue the outgoing CRYPTO in response".
//!
//! ## What this slice closes
//!
//! Every lower slice stops one call short of the TLS state machine. The
//! [`connection::QuicConnection`](super::connection::QuicConnection) reassembles the
//! per-level CRYPTO byte stream ([`read_crypto`](super::connection::QuicConnection::read_crypto))
//! but does not interpret it; the pure
//! [`tls_handshake::ClientHandshake`](super::tls_handshake::ClientHandshake) sequences
//! the handshake messages and derives the keys but does no IO; the
//! [`conn_turn::ConnectionTurn`](super::conn_turn::ConnectionTurn) installs keys and
//! enqueues frames but does not know *which* keys or *which* CRYPTO. This slice is
//! the bridge between them:
//!
//! 1. [`send_client_hello`](TlsConnState::send_client_hello) enqueues the client's
//!    first-flight ClientHello into the Initial-level CRYPTO stream (RFC 9001 §4.1).
//! 2. [`advance`](TlsConnState::advance) drains the newly-reassembled CRYPTO at the
//!    Initial and Handshake encryption levels, frames it into TLS handshake messages
//!    ([`tls_message::Handshake::parse`](super::tls_message::Handshake::parse)), and
//!    feeds each to the state machine. For every [`HandshakeEvent`] it:
//!    - on [`HandshakeKeysReady`](super::tls_handshake::HandshakeEvent::HandshakeKeysReady)
//!      installs the Handshake-level packet keys on the receive
//!      ([`RecvKeyRing`](super::recv_path::RecvKeyRing)) and send
//!      ([`ConnectionSendState`](super::send_state::ConnectionSendState)) halves
//!      (client direction protects our sends, server direction our receives,
//!      RFC 9001 §5.1),
//!    - on [`Complete`](super::tls_handshake::HandshakeEvent::Complete) installs the
//!      1-RTT (Application-Data) keys the same way and enqueues the client Finished
//!      into the Handshake-level CRYPTO stream (RFC 8446 §4.4.4), completing the
//!      client's contribution to the handshake.
//!
//! ## What it defers
//!
//! - **Certificate authentication.** Like [`tls_handshake`](super::tls_handshake),
//!   this slice does not chain the server certificate to a trust anchor or match it
//!   to the SNI; the raw [`server_certificate`](super::tls_handshake::HandshakeComplete::server_certificate)
//!   and [`server_certificate_verify`](super::tls_handshake::HandshakeComplete::server_certificate_verify)
//!   are handed back in the [`TlsAdvance::completed`] payload for a caller (or a
//!   later slice) to authenticate with [`tls_cert_verify`](super::tls_cert_verify)
//!   before the connection carries application data.
//! - **Building the ClientHello.** The exact first-flight bytes and the ephemeral
//!   X25519 private key are supplied by the caller; assembling a full ClientHello
//!   (ALPN, QUIC transport parameters, SNI) is a separate concern.
//! - **HANDSHAKE_DONE / key discard.** The peer confirms the handshake with
//!   HANDSHAKE_DONE, tracked by the connection
//!   ([`handshake_confirmed`](super::connection::QuicConnection::handshake_confirmed));
//!   discarding the Initial / Handshake keys once they are no longer needed
//!   (RFC 9001 §4.9) belongs to the connection driver.
//!
//! ## Purity
//!
//! Pure state, no clock and no IO of its own: [`advance`](TlsConnState::advance)
//! reaches the connection and the send state only through the borrowed
//! [`ConnectionTurn`], so a test drives a whole TLS handshake by feeding decoded
//! CRYPTO frames straight into the connection and reading back the installed keys.

use super::conn_turn::ConnectionTurn;
use super::key_agreement::X25519_KEY_LEN;
use super::loss::PacketNumberSpace;
use super::quic_frame::Frame;
use super::send_state::SendStateError;
use super::tls_handshake::{
    ClientHandshake, HandshakeComplete, HandshakeError, HandshakeEvent, HandshakeState,
};
use super::tls_message::{Handshake, TlsError};
use super::udp::DatagramTransport;

/// The encryption levels whose CRYPTO stream carries the TLS handshake, drained in
/// order by [`TlsConnState::advance`] (RFC 9001 §4.1): the ServerHello arrives at
/// the Initial level and unlocks the Handshake-level keys, after which the rest of
/// the server flight (EncryptedExtensions … Finished) arrives at the Handshake
/// level. The Application-Data level carries only post-handshake messages, which
/// this slice does not process.
const HANDSHAKE_LEVELS: [PacketNumberSpace; 2] =
    [PacketNumberSpace::Initial, PacketNumberSpace::Handshake];

/// What one [`TlsConnState::advance`] accomplished.
///
/// A summary of the CRYPTO consumed and the key material installed this call, so a
/// caller (or the enclosing handshake loop) can see the TLS handshake make
/// progress. `completed` carries the [`HandshakeComplete`] the moment the server
/// Finished verified, for the caller to authenticate the server certificate.
#[derive(Debug, Default)]
pub struct TlsAdvance {
    /// The number of complete TLS handshake messages framed from the reassembled
    /// CRYPTO and fed to the state machine this call.
    pub messages_processed: usize,
    /// Whether the Handshake-level packet keys were derived and installed this call
    /// (i.e. the ServerHello was processed, RFC 9001 §5.1).
    pub handshake_keys_installed: bool,
    /// The completion produced when the server Finished verified: the 1-RTT keys are
    /// already installed and the client Finished already enqueued, and the payload
    /// carries the server certificate and CertificateVerify for the caller to
    /// authenticate. `None` while the handshake is still in progress.
    pub completed: Option<Box<HandshakeComplete>>,
}

/// Why a step of the TLS-to-QUIC wiring failed.
#[derive(Debug)]
pub enum TlsConnError {
    /// The TLS state machine rejected a server handshake message
    /// ([`ClientHandshake::handle_message`](super::tls_handshake::ClientHandshake::handle_message)):
    /// an out-of-order message, an unsupported parameter, or a Finished MAC that did
    /// not verify. The connection must be closed with a TLS alert.
    Handshake(HandshakeError),
    /// The reassembled CRYPTO bytes were not a valid TLS handshake message framing
    /// ([`Handshake::parse`](super::tls_message::Handshake::parse)).
    Framing(TlsError),
    /// The derived outgoing CRYPTO (ClientHello or client Finished) could not be
    /// enqueued because its encryption level's send keys were not installed or the
    /// scheduler refused it
    /// ([`ConnectionSendState::enqueue`](super::send_state::ConnectionSendState::enqueue)).
    Enqueue(SendStateError),
}

impl core::fmt::Display for TlsConnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Handshake(e) => write!(f, "QUIC TLS: handshake message rejected: {e}"),
            Self::Framing(e) => write!(f, "QUIC TLS: malformed CRYPTO framing: {e}"),
            Self::Enqueue(e) => write!(f, "QUIC TLS: could not enqueue CRYPTO: {e}"),
        }
    }
}

impl std::error::Error for TlsConnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Handshake(e) => Some(e),
            Self::Framing(e) => Some(e),
            Self::Enqueue(e) => Some(e),
        }
    }
}

/// Bridges a client [`ClientHandshake`] to a QUIC connection's CRYPTO streams and
/// key installation.
///
/// Seed it with [`TlsConnState::new`] from the ephemeral X25519 private key and the
/// exact ClientHello bytes, send the first flight with
/// [`send_client_hello`](TlsConnState::send_client_hello), then call
/// [`advance`](TlsConnState::advance) whenever the connection may have reassembled
/// new CRYPTO (typically after each ingested datagram). It installs each encryption
/// level's keys on both halves of the borrowed [`ConnectionTurn`] and enqueues the
/// outgoing CRYPTO; the handshake completes when the server Finished verifies.
#[derive(Debug)]
pub struct TlsConnState {
    /// The pure TLS 1.3 client handshake state machine.
    tls: ClientHandshake,
    /// The exact ClientHello bytes to place in the Initial-level CRYPTO stream; the
    /// same bytes seeded the state machine's transcript.
    client_hello: Vec<u8>,
    /// Whether the ClientHello has been enqueued (the first flight is sent once).
    hello_sent: bool,
    /// CRYPTO bytes read at the Initial level that did not yet form a complete
    /// handshake message; a message split across datagrams is completed on a later
    /// [`advance`](TlsConnState::advance).
    initial_residual: Vec<u8>,
    /// CRYPTO bytes read at the Handshake level not yet forming a complete message.
    handshake_residual: Vec<u8>,
    /// The byte offset of the next outgoing Initial-level CRYPTO frame (RFC 9000
    /// §19.6): the ClientHello starts at zero.
    initial_send_offset: u64,
    /// The byte offset of the next outgoing Handshake-level CRYPTO frame: the client
    /// Finished starts at zero on the Handshake stream.
    handshake_send_offset: u64,
}

impl TlsConnState {
    /// Start the TLS-to-QUIC wiring for a client.
    ///
    /// `client_x25519_private` is the ephemeral X25519 private key whose public
    /// value the client offered in its ClientHello `key_share`; `client_hello` is
    /// the exact serialized ClientHello handshake message (the bytes to send in the
    /// Initial-level CRYPTO frame), which also seeds the handshake transcript.
    #[must_use]
    pub fn new(client_x25519_private: [u8; X25519_KEY_LEN], client_hello: Vec<u8>) -> Self {
        let tls = ClientHandshake::new(client_x25519_private, &client_hello);
        Self {
            tls,
            client_hello,
            hello_sent: false,
            initial_residual: Vec::new(),
            handshake_residual: Vec::new(),
            initial_send_offset: 0,
            handshake_send_offset: 0,
        }
    }

    /// The underlying TLS handshake state machine, borrowed immutably (e.g. to read
    /// its [`state`](super::tls_handshake::ClientHandshake::state)).
    #[must_use]
    pub fn tls(&self) -> &ClientHandshake {
        &self.tls
    }

    /// Whether the TLS handshake has completed (the server Finished verified and the
    /// client Finished was enqueued). QUIC still awaits HANDSHAKE_DONE from the peer
    /// for *confirmation*.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.tls.state() == HandshakeState::Complete
    }

    /// Whether the first-flight ClientHello has been enqueued.
    #[must_use]
    pub fn client_hello_sent(&self) -> bool {
        self.hello_sent
    }

    /// Enqueue the ClientHello into the Initial-level CRYPTO stream, sending the
    /// client's first flight (RFC 9001 §4.1). Idempotent: a second call is a no-op.
    ///
    /// The Initial send space must already be installed (its keys derive
    /// deterministically from the client's Destination Connection ID, RFC 9001
    /// §5.2); the caller flushes the resulting datagram after this returns.
    ///
    /// # Errors
    ///
    /// [`TlsConnError::Enqueue`] if the Initial send space is not installed or the
    /// scheduler refuses the frame.
    pub fn send_client_hello<T: DatagramTransport>(
        &mut self,
        turn: &mut ConnectionTurn<T>,
    ) -> Result<(), TlsConnError> {
        if self.hello_sent {
            return Ok(());
        }
        let data = self.client_hello.clone();
        let len = data.len() as u64;
        turn.send_mut()
            .enqueue(
                PacketNumberSpace::Initial,
                Frame::Crypto { offset: self.initial_send_offset, data },
            )
            .map_err(TlsConnError::Enqueue)?;
        self.initial_send_offset += len;
        self.hello_sent = true;
        Ok(())
    }

    /// Advance the TLS handshake by consuming any newly-reassembled CRYPTO at the
    /// Initial and Handshake encryption levels, installing the keys each derives and
    /// enqueuing the client Finished on completion.
    ///
    /// Call whenever the connection may have reassembled new CRYPTO (after an
    /// ingested datagram). It reads the contiguous CRYPTO prefix at each level,
    /// buffers a message split across datagrams until it is complete, and feeds each
    /// complete message to the state machine. Returns a [`TlsAdvance`] summarising
    /// the messages consumed and whether the Handshake or 1-RTT keys were installed.
    ///
    /// # Errors
    ///
    /// [`TlsConnError`] wrapping the failing step: a TLS message the state machine
    /// rejected, a malformed CRYPTO framing, or an outgoing CRYPTO that could not be
    /// enqueued.
    pub fn advance<T: DatagramTransport>(
        &mut self,
        turn: &mut ConnectionTurn<T>,
    ) -> Result<TlsAdvance, TlsConnError> {
        let mut summary = TlsAdvance::default();
        for level in HANDSHAKE_LEVELS {
            self.drain_level(turn, level, &mut summary)?;
        }
        Ok(summary)
    }

    /// Drain the reassembled CRYPTO at one encryption level, framing it into
    /// handshake messages and driving the state machine for each.
    fn drain_level<T: DatagramTransport>(
        &mut self,
        turn: &mut ConnectionTurn<T>,
        level: PacketNumberSpace,
        summary: &mut TlsAdvance,
    ) -> Result<(), TlsConnError> {
        let fresh = turn.driver_mut().connection_mut().read_crypto(level);
        // Take the level's residual out so the borrow ends before touching the
        // state machine; the ApplicationData level carries no handshake CRYPTO.
        let Some(residual) = self.residual_mut(level) else {
            return Ok(());
        };
        let mut buf = std::mem::take(residual);
        buf.extend_from_slice(&fresh);

        // Frame as many complete handshake messages as the buffer holds; a partial
        // trailing message stays in the residual for the next call.
        let mut pos = 0;
        while let Some((_, consumed)) =
            Handshake::parse(&buf[pos..]).map_err(TlsConnError::Framing)?
        {
            let start = pos;
            pos += consumed;
            let event = self
                .tls
                .handle_message(&buf[start..pos])
                .map_err(TlsConnError::Handshake)?;
            self.apply_event(turn, event, summary)?;
            summary.messages_processed += 1;
        }

        buf.drain(..pos);
        if let Some(residual) = self.residual_mut(level) {
            *residual = buf;
        }
        Ok(())
    }

    /// Act on one [`HandshakeEvent`]: install the derived keys on both halves and,
    /// on completion, enqueue the client Finished.
    fn apply_event<T: DatagramTransport>(
        &mut self,
        turn: &mut ConnectionTurn<T>,
        event: HandshakeEvent,
        summary: &mut TlsAdvance,
    ) -> Result<(), TlsConnError> {
        match event {
            HandshakeEvent::HandshakeKeysReady(keys) => {
                // Client direction protects our sends, server direction our receives
                // (RFC 9001 §5.1).
                turn.driver_mut()
                    .recv_keys_mut()
                    .install(PacketNumberSpace::Handshake, keys.server);
                turn.send_mut()
                    .install(PacketNumberSpace::Handshake, keys.client);
                summary.handshake_keys_installed = true;
            }
            HandshakeEvent::Continue => {}
            HandshakeEvent::Complete(complete) => {
                // Install the 1-RTT (Application-Data) keys on both halves.
                turn.driver_mut()
                    .recv_keys_mut()
                    .install(PacketNumberSpace::ApplicationData, complete.app_keys.server.clone());
                turn.send_mut().install(
                    PacketNumberSpace::ApplicationData,
                    complete.app_keys.client.clone(),
                );
                // Send the client Finished in a Handshake-level CRYPTO frame
                // (RFC 8446 §4.4.4).
                let data = complete.client_finished.clone();
                let len = data.len() as u64;
                turn.send_mut()
                    .enqueue(
                        PacketNumberSpace::Handshake,
                        Frame::Crypto { offset: self.handshake_send_offset, data },
                    )
                    .map_err(TlsConnError::Enqueue)?;
                self.handshake_send_offset += len;
                summary.completed = Some(complete);
            }
        }
        Ok(())
    }

    /// The residual CRYPTO buffer for an encryption level, or `None` for the
    /// Application-Data level, which carries no handshake CRYPTO here.
    fn residual_mut(&mut self, level: PacketNumberSpace) -> Option<&mut Vec<u8>> {
        match level {
            PacketNumberSpace::Initial => Some(&mut self.initial_residual),
            PacketNumberSpace::Handshake => Some(&mut self.handshake_residual),
            PacketNumberSpace::ApplicationData => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::connection::{ConnectionConfig, QuicConnection};
    use crate::h3::driver::ConnectionDriver;
    use crate::h3::event_loop::DatagramEventLoop;
    use crate::h3::key_agreement;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::pto::LossDetection;
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

    /// A connection turn with the Initial space installed on both halves.
    fn turn(now: Instant) -> ConnectionTurn<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), scid(), 1200);
        send.install(PacketNumberSpace::Initial, initial_keys().client);
        ConnectionTurn::new(
            driver(transport(), now),
            send,
            1200,
            crate::h3::conn_turn::DEFAULT_ACK_DELAY_EXPONENT,
        )
    }

    /// A connection turn with *no* send space installed, to exercise the enqueue
    /// error path.
    fn turn_without_send(now: Instant) -> ConnectionTurn<MockDatagramTransport> {
        let send = ConnectionSendState::new(1, dcid(), scid(), 1200);
        ConnectionTurn::new(
            driver(transport(), now),
            send,
            1200,
            crate::h3::conn_turn::DEFAULT_ACK_DELAY_EXPONENT,
        )
    }

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

    /// Derive the Handshake traffic secrets exactly as the flow does, to forge a
    /// matching server Finished.
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

    /// Build the server Finished whose verify_data is correct over
    /// `ClientHello…CertificateVerify`.
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

    /// Feed decoded CRYPTO into the connection at `level`, starting at `offset`.
    fn feed_crypto(
        t: &mut ConnectionTurn<MockDatagramTransport>,
        level: PacketNumberSpace,
        offset: u64,
        data: Vec<u8>,
        now: Instant,
    ) {
        t.driver_mut()
            .connection_mut()
            .process_packet(level, 0, &[Frame::Crypto { offset, data }], now)
            .expect("crypto frame processes");
    }

    // ---- construction / first flight -----------------------------------

    #[test]
    fn send_client_hello_enqueues_initial_crypto() {
        let now = base();
        let mut t = turn(now);
        let ch = client_hello_bytes();
        let mut tls = TlsConnState::new(CLIENT_PRIV, ch.clone());
        assert!(!tls.client_hello_sent());

        tls.send_client_hello(&mut t).expect("enqueues");
        assert!(tls.client_hello_sent());
        assert!(t.send().pending_in(PacketNumberSpace::Initial));
    }

    #[test]
    fn send_client_hello_is_idempotent() {
        let now = base();
        let mut t = turn(now);
        let mut tls = TlsConnState::new(CLIENT_PRIV, client_hello_bytes());
        tls.send_client_hello(&mut t).expect("first");
        // Draining the queue then a second call must not enqueue a duplicate.
        t.flush(now).expect("flush");
        tls.send_client_hello(&mut t).expect("second is a no-op");
        assert!(!t.send().pending_in(PacketNumberSpace::Initial));
    }

    #[test]
    fn send_client_hello_without_send_keys_errors() {
        let now = base();
        let mut t = turn_without_send(now);
        let mut tls = TlsConnState::new(CLIENT_PRIV, client_hello_bytes());
        let err = tls.send_client_hello(&mut t).expect_err("no Initial send keys");
        assert!(matches!(
            err,
            TlsConnError::Enqueue(SendStateError::SpaceNotInstalled(PacketNumberSpace::Initial))
        ));
    }

    // ---- advance: ServerHello installs Handshake keys ------------------

    #[test]
    fn advance_processes_server_hello_and_installs_handshake_keys() {
        let now = base();
        let mut t = turn(now);
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut tls = TlsConnState::new(CLIENT_PRIV, ch.clone());

        feed_crypto(&mut t, PacketNumberSpace::Initial, 0, sh, now);
        let adv = tls.advance(&mut t).expect("advance");

        assert_eq!(adv.messages_processed, 1);
        assert!(adv.handshake_keys_installed);
        assert!(adv.completed.is_none());
        assert_eq!(tls.tls().state(), HandshakeState::ExpectEncryptedExtensions);
        // The Handshake space is now installed on both halves.
        assert!(t.send().is_installed(PacketNumberSpace::Handshake));
        assert!(!t.driver().connection().handshake_confirmed());
    }

    // ---- advance: full handshake completes -----------------------------

    #[test]
    fn advance_completes_and_enqueues_client_finished() {
        let now = base();
        let mut t = turn(now);
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut tls = TlsConnState::new(CLIENT_PRIV, ch.clone());

        // ServerHello at the Initial level unlocks the Handshake keys.
        feed_crypto(&mut t, PacketNumberSpace::Initial, 0, sh.clone(), now);
        tls.advance(&mut t).expect("advance over ServerHello");

        // The rest of the flight arrives at the Handshake level.
        let flight = handshake_flight(&ch, &sh);
        feed_crypto(&mut t, PacketNumberSpace::Handshake, 0, flight, now);
        let adv = tls.advance(&mut t).expect("advance over the flight");

        // EncryptedExtensions, Certificate, CertificateVerify, Finished.
        assert_eq!(adv.messages_processed, 4);
        let complete = adv.completed.expect("handshake completed");
        assert_eq!(complete.server_certificate.certificate_list.len(), 1);
        assert!(tls.is_complete());
        // The 1-RTT keys are installed on both halves.
        assert!(t.send().is_installed(PacketNumberSpace::ApplicationData));
        // The client Finished was enqueued at the Handshake level.
        assert!(t.send().pending_in(PacketNumberSpace::Handshake));
    }

    #[test]
    fn advance_drives_the_whole_handshake_in_one_call() {
        let now = base();
        let mut t = turn(now);
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut tls = TlsConnState::new(CLIENT_PRIV, ch.clone());

        // Both levels' CRYPTO is available before the single advance: Initial is
        // drained first (ServerHello → Handshake keys), then Handshake (flight →
        // complete), all in one call because advance walks the levels in order.
        feed_crypto(&mut t, PacketNumberSpace::Initial, 0, sh.clone(), now);
        feed_crypto(&mut t, PacketNumberSpace::Handshake, 0, handshake_flight(&ch, &sh), now);

        let adv = tls.advance(&mut t).expect("advance");
        assert_eq!(adv.messages_processed, 5);
        assert!(adv.handshake_keys_installed);
        assert!(adv.completed.is_some());
        assert!(tls.is_complete());
    }

    // ---- advance: streaming / partial reassembly -----------------------

    #[test]
    fn advance_buffers_a_message_split_across_datagrams() {
        let now = base();
        let mut t = turn(now);
        let sh = server_hello_bytes();
        let mut tls = TlsConnState::new(CLIENT_PRIV, client_hello_bytes());

        // First half of the ServerHello: not a complete message yet.
        let split = sh.len() / 2;
        feed_crypto(&mut t, PacketNumberSpace::Initial, 0, sh[..split].to_vec(), now);
        let adv1 = tls.advance(&mut t).expect("advance over the first half");
        assert_eq!(adv1.messages_processed, 0);
        assert!(!adv1.handshake_keys_installed);
        assert_eq!(tls.tls().state(), HandshakeState::ExpectServerHello);

        // The rest completes the message on the next advance.
        feed_crypto(
            &mut t,
            PacketNumberSpace::Initial,
            split as u64,
            sh[split..].to_vec(),
            now,
        );
        let adv2 = tls.advance(&mut t).expect("advance over the rest");
        assert_eq!(adv2.messages_processed, 1);
        assert!(adv2.handshake_keys_installed);
    }

    #[test]
    fn advance_with_no_crypto_is_a_noop() {
        let now = base();
        let mut t = turn(now);
        let mut tls = TlsConnState::new(CLIENT_PRIV, client_hello_bytes());
        let adv = tls.advance(&mut t).expect("advance");
        assert_eq!(adv.messages_processed, 0);
        assert!(!adv.handshake_keys_installed);
        assert!(adv.completed.is_none());
    }

    // ---- advance: error paths ------------------------------------------

    #[test]
    fn advance_rejects_an_out_of_order_message() {
        let now = base();
        let mut t = turn(now);
        let mut tls = TlsConnState::new(CLIENT_PRIV, client_hello_bytes());
        // EncryptedExtensions before the ServerHello is out of order.
        feed_crypto(
            &mut t,
            PacketNumberSpace::Initial,
            0,
            encrypted_extensions_bytes(),
            now,
        );
        let err = tls.advance(&mut t).expect_err("out of order");
        assert!(matches!(err, TlsConnError::Handshake(_)));
        assert_eq!(tls.tls().state(), HandshakeState::Failed);
    }

    #[test]
    fn advance_rejects_a_tampered_server_finished() {
        let now = base();
        let mut t = turn(now);
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut tls = TlsConnState::new(CLIENT_PRIV, ch.clone());

        feed_crypto(&mut t, PacketNumberSpace::Initial, 0, sh.clone(), now);
        tls.advance(&mut t).expect("ServerHello");

        // Flip the last byte of the flight (the Finished verify_data).
        let mut flight = handshake_flight(&ch, &sh);
        let last = flight.len() - 1;
        flight[last] ^= 0x01;
        feed_crypto(&mut t, PacketNumberSpace::Handshake, 0, flight, now);
        let err = tls.advance(&mut t).expect_err("bad MAC");
        assert!(matches!(err, TlsConnError::Handshake(_)));
        assert_eq!(tls.tls().state(), HandshakeState::Failed);
    }
}
