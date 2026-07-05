//! Client-side TLS 1.3 handshake flow over QUIC (RFC 8446 §2, §4; RFC 9001 §4).
//!
//! The earlier TLS slices are the individual cryptographic primitives a QUIC
//! client needs — [`super::tls_message`] frames the handshake messages,
//! [`super::key_agreement`] runs the X25519 (EC)DHE, [`super::tls_schedule`]
//! turns the shared secret into the traffic secrets and QUIC packet keys,
//! [`super::tls_finished`] computes and verifies the Finished MAC, and
//! [`super::crypto_stream`] reassembles the CRYPTO byte stream that carries them.
//! This slice is the *state machine* that sequences those primitives into the
//! ordered client handshake, so the eventual IO layer only has to feed it the
//! bytes reassembled from each encryption level's CRYPTO stream.
//!
//! [`ClientHandshake`] is a pure state machine for the full (certificate-based,
//! non-resumption) 1-RTT handshake, fixed to the mandatory-to-implement
//! `TLS_AES_128_GCM_SHA256` cipher suite (SHA-256) and X25519 key exchange:
//!
//! - It is seeded with the client's ephemeral X25519 private key and the exact
//!   ClientHello handshake-message bytes the client sent (which QUIC carries in
//!   an Initial-level CRYPTO frame). Those bytes start the transcript.
//! - The caller feeds it the server's flight one complete handshake message at a
//!   time ([`ClientHandshake::handle_message`], each message reassembled from a
//!   CRYPTO stream by [`super::crypto_stream`]): ServerHello, then (at the
//!   Handshake encryption level) EncryptedExtensions, an optional
//!   CertificateRequest, Certificate, CertificateVerify and Finished.
//! - It enforces the RFC 8446 §4 message ordering, runs the (EC)DHE at
//!   ServerHello and reports the Handshake-level packet keys, and on the server
//!   Finished it verifies the server's MAC over the `ClientHello…CertificateVerify`
//!   transcript (RFC 8446 §4.4.4), derives the 1-RTT keys and the master /
//!   exporter / resumption secrets, and produces the client Finished message the
//!   caller sends back in a Handshake-level CRYPTO frame.
//!
//! Pure state machine, no IO. It does **not** validate the server's certificate
//! chain — the transcript-bound MAC proves the peer holds the handshake keys, but
//! proving the certificate chains to a trust anchor and matches the SNI is the
//! caller's job (the raw [`super::tls_message::Certificate`] and
//! [`super::tls_message::CertificateVerify`] are handed back for that, to be
//! authenticated with [`super::tls_cert_verify`] before the connection is used).
//! HelloRetryRequest, PSK/resumption, client authentication and post-handshake
//! messages (NewSessionTicket, KeyUpdate) are out of scope for this slice.

use super::key_agreement::{self, KeyAgreementError};
use super::tls_message::{
    self, Certificate, CertificateVerify, Handshake, KeyShareEntry, TlsError,
};
use super::tls_schedule::{
    self, ApplicationTrafficSecrets, DirectionalKeys, HandshakeTrafficSecrets,
};

/// The length of a SHA-256 transcript hash / traffic secret, in bytes.
const HASH_LEN: usize = 32;

/// Failure processing a step of the client handshake (RFC 8446 §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandshakeError {
    /// A message was received out of the order RFC 8446 §4 requires.
    UnexpectedMessage {
        /// A human-readable description of what the state machine expected next.
        expected: &'static str,
        /// The `msg_type` of the handshake message that actually arrived.
        got: u8,
    },
    /// The supplied bytes did not contain exactly one complete handshake message
    /// (a short buffer, or trailing bytes after the message). The caller must
    /// frame one message per call via [`Handshake::parse`].
    NotOneMessage,
    /// The ServerHello was a HelloRetryRequest; this slice cannot yet restart the
    /// handshake with a different key-share group (RFC 8446 §4.1.4).
    HelloRetryRequestUnsupported,
    /// The server selected a cipher suite other than `TLS_AES_128_GCM_SHA256`.
    UnsupportedCipherSuite(u16),
    /// The ServerHello did not select TLS 1.3 via a `supported_versions`
    /// extension (RFC 8446 §4.2.1).
    UnsupportedVersion,
    /// The ServerHello carried no usable X25519 `key_share` (RFC 8446 §4.2.8).
    MissingKeyShare,
    /// The X25519 (EC)DHE agreement failed on the server's key share.
    KeyAgreement(KeyAgreementError),
    /// The server's Finished MAC did not verify against the handshake transcript
    /// (RFC 8446 §4.4.4): a fatal `decrypt_error`.
    FinishedVerificationFailed,
    /// A handshake message body was malformed (RFC 8446 §4).
    Tls(TlsError),
    /// The handshake already completed; no further messages are accepted here.
    AlreadyComplete,
    /// A previous error moved the handshake into a terminal failed state.
    Failed,
}

impl core::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedMessage { expected, got } => {
                write!(f, "unexpected handshake message type {got}, expected {expected}")
            }
            Self::NotOneMessage => {
                write!(f, "input was not exactly one complete handshake message")
            }
            Self::HelloRetryRequestUnsupported => {
                write!(f, "HelloRetryRequest is not supported")
            }
            Self::UnsupportedCipherSuite(c) => {
                write!(f, "unsupported cipher suite {c:#06x}")
            }
            Self::UnsupportedVersion => {
                write!(f, "server did not select TLS 1.3")
            }
            Self::MissingKeyShare => {
                write!(f, "ServerHello has no usable X25519 key_share")
            }
            Self::KeyAgreement(e) => write!(f, "key agreement failed: {e}"),
            Self::FinishedVerificationFailed => {
                write!(f, "server Finished MAC did not verify")
            }
            Self::Tls(e) => write!(f, "{e}"),
            Self::AlreadyComplete => write!(f, "handshake already complete"),
            Self::Failed => write!(f, "handshake is in a failed state"),
        }
    }
}

impl std::error::Error for HandshakeError {}

impl From<TlsError> for HandshakeError {
    fn from(e: TlsError) -> Self {
        Self::Tls(e)
    }
}

impl From<KeyAgreementError> for HandshakeError {
    fn from(e: KeyAgreementError) -> Self {
        Self::KeyAgreement(e)
    }
}

/// The client handshake's progress through the server's flight (RFC 8446 §4),
/// i.e. which message [`ClientHandshake::handle_message`] expects next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeState {
    /// Awaiting the ServerHello (RFC 8446 §4.1.3).
    ExpectServerHello,
    /// ServerHello processed; awaiting EncryptedExtensions (RFC 8446 §4.3.1).
    ExpectEncryptedExtensions,
    /// Awaiting either the optional CertificateRequest (RFC 8446 §4.3.2) or the
    /// server Certificate (RFC 8446 §4.4.2).
    ExpectCertificateRequestOrCertificate,
    /// A CertificateRequest was seen; awaiting the server Certificate.
    ExpectCertificate,
    /// Awaiting the server CertificateVerify (RFC 8446 §4.4.3).
    ExpectCertificateVerify,
    /// Awaiting the server Finished (RFC 8446 §4.4.4).
    ExpectFinished,
    /// The handshake completed successfully.
    Complete,
    /// The handshake failed and accepts no further messages.
    Failed,
}

/// The 1-RTT key material and secrets produced when the server Finished verifies
/// (RFC 8446 §7.1). The three master-derived secrets are redacted from `Debug`.
#[derive(Clone)]
pub struct HandshakeComplete {
    /// The 1-RTT (application) packet-protection keys for both directions
    /// (RFC 9001 §5.1). The client protects 1-RTT packets it sends with
    /// `app_keys.client` and removes protection from server packets with
    /// `app_keys.server`.
    pub app_keys: DirectionalKeys,
    /// The 1-RTT (application) traffic secrets, retained so the caller can run a
    /// QUIC key update (RFC 9001 §6) via
    /// [`super::key_schedule::next_generation_secret`].
    pub app_secrets: ApplicationTrafficSecrets,
    /// The serialized client Finished handshake message (RFC 8446 §4.4.4) to send
    /// in a Handshake-level CRYPTO frame, completing the handshake.
    pub client_finished: Vec<u8>,
    /// The Master Secret (RFC 8446 §7.1).
    pub master_secret: [u8; HASH_LEN],
    /// The exporter master secret (RFC 8446 §7.5), for exported keying material.
    pub exporter_master_secret: [u8; HASH_LEN],
    /// The resumption master secret (RFC 8446 §7.1), the base for any
    /// NewSessionTicket PSK.
    pub resumption_master_secret: [u8; HASH_LEN],
    /// The server's Certificate message (RFC 8446 §4.4.2), for the caller to
    /// authenticate against a trust anchor before using the connection.
    pub server_certificate: Certificate,
    /// The server's CertificateVerify (RFC 8446 §4.4.3), the signature the caller
    /// checks with [`super::tls_cert_verify`] over the handshake transcript.
    pub server_certificate_verify: CertificateVerify,
}

impl core::fmt::Debug for HandshakeComplete {
    /// Redacts the master-derived secrets — logging them would leak the
    /// connection's key material. Structural fields are shown.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HandshakeComplete")
            .field("app_keys", &self.app_keys)
            .field("app_secrets", &self.app_secrets)
            .field("client_finished", &format_args!("<{} bytes>", self.client_finished.len()))
            .field("master_secret", &format_args!("<{HASH_LEN} bytes redacted>"))
            .field("exporter_master_secret", &format_args!("<{HASH_LEN} bytes redacted>"))
            .field("resumption_master_secret", &format_args!("<{HASH_LEN} bytes redacted>"))
            .field("server_certificate", &self.server_certificate)
            .field("server_certificate_verify", &self.server_certificate_verify)
            .finish()
    }
}

/// What processing one server handshake message accomplished.
#[derive(Clone, Debug)]
pub enum HandshakeEvent {
    /// The ServerHello was processed and the (EC)DHE completed: the Handshake-level
    /// packet-protection keys (RFC 9001 §5.1) are now available for both
    /// directions. The client decrypts the rest of the server flight with
    /// `server` and protects its own Handshake packets with `client`.
    HandshakeKeysReady(DirectionalKeys),
    /// The message was accepted and advanced the handshake, with nothing for the
    /// caller to act on yet (EncryptedExtensions, CertificateRequest,
    /// Certificate, CertificateVerify).
    Continue,
    /// The server Finished verified: the handshake is complete. Boxed to keep the
    /// event enum small.
    Complete(Box<HandshakeComplete>),
}

/// Client-side TLS 1.3 handshake flow state machine (RFC 8446 §4, RFC 9001 §4).
///
/// Fixed to `TLS_AES_128_GCM_SHA256` (SHA-256) and X25519. Seed it with
/// [`ClientHandshake::new`], then drive it with [`ClientHandshake::handle_message`]
/// once per server handshake message.
#[derive(Clone)]
pub struct ClientHandshake {
    /// Where the flow is in the server's flight.
    state: HandshakeState,
    /// The client's ephemeral X25519 private key, whose public value the client
    /// offered in its ClientHello `key_share`.
    client_x25519_private: [u8; key_agreement::X25519_KEY_LEN],
    /// The running handshake transcript: every handshake message's exact wire
    /// bytes, concatenated in order (RFC 8446 §4.4.1), starting with ClientHello.
    transcript: Vec<u8>,
    /// The Handshake Secret (RFC 8446 §7.1), retained from ServerHello to derive
    /// the Master Secret once the server Finished arrives.
    handshake_secret: Option<[u8; HASH_LEN]>,
    /// The Handshake traffic secrets (RFC 8446 §7.1); their finished_keys verify
    /// the server Finished and compute the client Finished.
    hs_traffic: Option<HandshakeTrafficSecrets>,
    /// The server's Certificate, captured for the caller to authenticate.
    server_certificate: Option<Certificate>,
    /// The server's CertificateVerify, held from its arrival until the handshake
    /// completes and it is handed to the caller.
    pending_certificate_verify: Option<CertificateVerify>,
}

impl core::fmt::Debug for ClientHandshake {
    /// Redacts the private key and secrets; shows the state and transcript length.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClientHandshake")
            .field("state", &self.state)
            .field("transcript_len", &self.transcript.len())
            .field("has_handshake_keys", &self.hs_traffic.is_some())
            .finish()
    }
}

impl ClientHandshake {
    /// Start a client handshake.
    ///
    /// `client_x25519_private` is the ephemeral X25519 private key whose public
    /// value the client placed in its ClientHello `key_share`. `client_hello` is
    /// the exact serialized ClientHello handshake message (the bytes the client
    /// sent in its Initial-level CRYPTO frame), which seeds the transcript.
    #[must_use]
    pub fn new(
        client_x25519_private: [u8; key_agreement::X25519_KEY_LEN],
        client_hello: &[u8],
    ) -> Self {
        Self {
            state: HandshakeState::ExpectServerHello,
            client_x25519_private,
            transcript: client_hello.to_vec(),
            handshake_secret: None,
            hs_traffic: None,
            server_certificate: None,
            pending_certificate_verify: None,
        }
    }

    /// The current handshake state.
    #[must_use]
    pub fn state(&self) -> HandshakeState {
        self.state
    }

    /// Whether the handshake has completed successfully.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.state == HandshakeState::Complete
    }

    /// Process exactly one complete server handshake message.
    ///
    /// `raw` must be exactly one handshake message as it appeared on the wire
    /// (the caller frames the CRYPTO byte stream with [`Handshake::parse`]); its
    /// bytes are appended verbatim to the transcript. Returns the
    /// [`HandshakeEvent`] the message produced.
    ///
    /// # Errors
    ///
    /// Any [`HandshakeError`]; a failed call moves the handshake into
    /// [`HandshakeState::Failed`] and every later call returns
    /// [`HandshakeError::Failed`].
    pub fn handle_message(&mut self, raw: &[u8]) -> Result<HandshakeEvent, HandshakeError> {
        let result = self.handle_message_inner(raw);
        // A genuine fault poisons the handshake; completing is not a fault, so a
        // stray message after Complete leaves the Complete state intact.
        if result.is_err() && self.state != HandshakeState::Complete {
            self.state = HandshakeState::Failed;
        }
        result
    }

    /// The fallible body of [`handle_message`](Self::handle_message); the wrapper
    /// handles the failed-state transition.
    fn handle_message_inner(&mut self, raw: &[u8]) -> Result<HandshakeEvent, HandshakeError> {
        match self.state {
            HandshakeState::Complete => return Err(HandshakeError::AlreadyComplete),
            HandshakeState::Failed => return Err(HandshakeError::Failed),
            _ => {}
        }

        // Frame exactly one complete message; reject a short buffer or extra bytes.
        let (message, consumed) = match Handshake::parse(raw)? {
            Some(parsed) => parsed,
            None => return Err(HandshakeError::NotOneMessage),
        };
        if consumed != raw.len() {
            return Err(HandshakeError::NotOneMessage);
        }

        match self.state {
            HandshakeState::ExpectServerHello => self.on_server_hello(message, raw),
            HandshakeState::ExpectEncryptedExtensions => {
                self.expect_encrypted_extensions(message, raw)
            }
            HandshakeState::ExpectCertificateRequestOrCertificate => {
                self.expect_cert_request_or_certificate(message, raw)
            }
            HandshakeState::ExpectCertificate => self.expect_certificate(message, raw),
            HandshakeState::ExpectCertificateVerify => {
                self.expect_certificate_verify(message, raw)
            }
            HandshakeState::ExpectFinished => self.on_server_finished(message, raw),
            // Handled above.
            HandshakeState::Complete | HandshakeState::Failed => unreachable!(),
        }
    }

    /// Handle the ServerHello: run the X25519 (EC)DHE, derive the Handshake
    /// Secret and traffic secrets over the `ClientHello…ServerHello` transcript,
    /// and hand back the Handshake-level packet keys (RFC 8446 §4.1.3, §7.1).
    fn on_server_hello(
        &mut self,
        message: Handshake,
        raw: &[u8],
    ) -> Result<HandshakeEvent, HandshakeError> {
        let sh = match message {
            Handshake::ServerHello(sh) => sh,
            other => return Err(unexpected("ServerHello", &other)),
        };
        if sh.is_hello_retry_request() {
            return Err(HandshakeError::HelloRetryRequestUnsupported);
        }
        if sh.cipher_suite != tls_message::TLS_AES_128_GCM_SHA256 {
            return Err(HandshakeError::UnsupportedCipherSuite(sh.cipher_suite));
        }

        // TLS 1.3 must be selected through supported_versions (RFC 8446 §4.2.1).
        // In a ServerHello the extension body is a bare 2-byte `selected_version`
        // (the ClientHello list form does not apply), so read it directly.
        let selected = find_extension(&sh.extensions, tls_message::EXT_SUPPORTED_VERSIONS)
            .ok_or(HandshakeError::UnsupportedVersion)?;
        if selected.len() != 2
            || u16::from_be_bytes([selected[0], selected[1]]) != tls_message::VERSION_TLS13
        {
            return Err(HandshakeError::UnsupportedVersion);
        }

        // The server's X25519 key share feeds the (EC)DHE (RFC 8446 §4.2.8).
        let share = KeyShareEntry::parse_server_hello(
            find_extension(&sh.extensions, tls_message::EXT_KEY_SHARE)
                .ok_or(HandshakeError::MissingKeyShare)?,
        )?;
        if share.group != tls_message::GROUP_X25519 {
            return Err(HandshakeError::MissingKeyShare);
        }
        let ecdhe =
            key_agreement::x25519_ecdhe_from_key_share(&self.client_x25519_private, &share)?;

        // Extend the transcript, then derive over ClientHello…ServerHello.
        self.transcript.extend_from_slice(raw);
        let handshake_secret = tls_schedule::handshake_secret(&ecdhe);
        let transcript_ch_sh = tls_schedule::transcript_hash(&self.transcript);
        let hs_traffic = HandshakeTrafficSecrets::derive(&handshake_secret, &transcript_ch_sh);
        let keys = hs_traffic.packet_keys();

        self.handshake_secret = Some(handshake_secret);
        self.hs_traffic = Some(hs_traffic);
        self.state = HandshakeState::ExpectEncryptedExtensions;
        Ok(HandshakeEvent::HandshakeKeysReady(keys))
    }

    /// Accept EncryptedExtensions (RFC 8446 §4.3.1) and advance.
    fn expect_encrypted_extensions(
        &mut self,
        message: Handshake,
        raw: &[u8],
    ) -> Result<HandshakeEvent, HandshakeError> {
        match message {
            Handshake::EncryptedExtensions(_) => {
                self.transcript.extend_from_slice(raw);
                self.state = HandshakeState::ExpectCertificateRequestOrCertificate;
                Ok(HandshakeEvent::Continue)
            }
            other => Err(unexpected("EncryptedExtensions", &other)),
        }
    }

    /// Accept the optional CertificateRequest (RFC 8446 §4.3.2) or the server
    /// Certificate (RFC 8446 §4.4.2).
    fn expect_cert_request_or_certificate(
        &mut self,
        message: Handshake,
        raw: &[u8],
    ) -> Result<HandshakeEvent, HandshakeError> {
        match message {
            Handshake::CertificateRequest(_) => {
                // Client authentication is out of scope: we note the request (so
                // its bytes enter the transcript) but do not send a client
                // Certificate; the server may still complete the handshake.
                self.transcript.extend_from_slice(raw);
                self.state = HandshakeState::ExpectCertificate;
                Ok(HandshakeEvent::Continue)
            }
            Handshake::Certificate(cert) => {
                self.accept_certificate(cert, raw);
                Ok(HandshakeEvent::Continue)
            }
            other => Err(unexpected("CertificateRequest or Certificate", &other)),
        }
    }

    /// Accept the server Certificate after a CertificateRequest (RFC 8446 §4.4.2).
    fn expect_certificate(
        &mut self,
        message: Handshake,
        raw: &[u8],
    ) -> Result<HandshakeEvent, HandshakeError> {
        match message {
            Handshake::Certificate(cert) => {
                self.accept_certificate(cert, raw);
                Ok(HandshakeEvent::Continue)
            }
            other => Err(unexpected("Certificate", &other)),
        }
    }

    /// Record the server Certificate and advance to CertificateVerify.
    fn accept_certificate(&mut self, cert: Certificate, raw: &[u8]) {
        self.transcript.extend_from_slice(raw);
        self.server_certificate = Some(cert);
        self.state = HandshakeState::ExpectCertificateVerify;
    }

    /// Accept the server CertificateVerify (RFC 8446 §4.4.3). The signature is not
    /// checked here — it is handed to the caller in [`HandshakeComplete`] to
    /// verify against the certificate's public key.
    fn expect_certificate_verify(
        &mut self,
        message: Handshake,
        raw: &[u8],
    ) -> Result<HandshakeEvent, HandshakeError> {
        match message {
            Handshake::CertificateVerify(cv) => {
                self.transcript.extend_from_slice(raw);
                // Stash the CertificateVerify alongside the Certificate by carrying
                // it through to completion via the state; store it on the struct.
                self.pending_certificate_verify = Some(cv);
                self.state = HandshakeState::ExpectFinished;
                Ok(HandshakeEvent::Continue)
            }
            other => Err(unexpected("CertificateVerify", &other)),
        }
    }

    /// Handle the server Finished (RFC 8446 §4.4.4): verify its MAC over
    /// `ClientHello…CertificateVerify`, derive the 1-RTT keys and master-derived
    /// secrets, and build the client Finished to send back.
    fn on_server_finished(
        &mut self,
        message: Handshake,
        raw: &[u8],
    ) -> Result<HandshakeEvent, HandshakeError> {
        let verify_data = match message {
            Handshake::Finished(vd) => vd,
            other => return Err(unexpected("Finished", &other)),
        };
        // These are always populated by the time we reach ExpectFinished.
        let hs_traffic = self.hs_traffic.as_ref().ok_or(HandshakeError::Failed)?;
        let handshake_secret = self.handshake_secret.ok_or(HandshakeError::Failed)?;

        // The server Finished MAC is over the transcript up to and including
        // CertificateVerify, i.e. before appending the Finished (RFC 8446 §4.4.4).
        let transcript_ch_cv = tls_schedule::transcript_hash(&self.transcript);
        if !super::tls_finished::verify_finished(
            &hs_traffic.server,
            &transcript_ch_cv,
            &verify_data,
        ) {
            return Err(HandshakeError::FinishedVerificationFailed);
        }

        // Now fold the server Finished into the transcript: the 1-RTT traffic
        // secrets and the client Finished MAC are over ClientHello…server Finished
        // (RFC 8446 §7.1, §4.4.4).
        self.transcript.extend_from_slice(raw);
        let transcript_ch_sf = tls_schedule::transcript_hash(&self.transcript);

        let master_secret = tls_schedule::master_secret(&handshake_secret);
        let app_secrets =
            ApplicationTrafficSecrets::derive(&master_secret, &transcript_ch_sf);
        let app_keys = app_secrets.packet_keys();
        let exporter_master_secret =
            tls_schedule::exporter_master_secret(&master_secret, &transcript_ch_sf);

        // The client Finished MAC uses the client handshake traffic secret over
        // the same ClientHello…server Finished transcript (RFC 8446 §4.4.4).
        let client_verify_data = super::tls_finished::finished_verify_data(
            &hs_traffic.client,
            &transcript_ch_sf,
        );
        let mut client_finished = Vec::new();
        Handshake::Finished(client_verify_data.to_vec()).encode(&mut client_finished)?;

        // The resumption master secret is over ClientHello…client Finished.
        self.transcript.extend_from_slice(&client_finished);
        let transcript_ch_cf = tls_schedule::transcript_hash(&self.transcript);
        let resumption_master_secret =
            tls_schedule::resumption_master_secret(&master_secret, &transcript_ch_cf);

        let server_certificate =
            self.server_certificate.take().ok_or(HandshakeError::Failed)?;
        let server_certificate_verify =
            self.pending_certificate_verify.take().ok_or(HandshakeError::Failed)?;

        self.state = HandshakeState::Complete;
        Ok(HandshakeEvent::Complete(Box::new(HandshakeComplete {
            app_keys,
            app_secrets,
            client_finished,
            master_secret,
            exporter_master_secret,
            resumption_master_secret,
            server_certificate,
            server_certificate_verify,
        })))
    }
}

/// Build an [`HandshakeError::UnexpectedMessage`] from the message that arrived.
fn unexpected(expected: &'static str, got: &Handshake) -> HandshakeError {
    HandshakeError::UnexpectedMessage { expected, got: got.msg_type() }
}

/// Find a TLS extension body by its type code (RFC 8446 §4.2).
fn find_extension(exts: &[tls_message::Extension], ext_type: u16) -> Option<&[u8]> {
    exts.iter()
        .find(|e| e.extension_type == ext_type)
        .map(|e| e.data.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::tls_message::{
        Certificate, CertificateEntry, CertificateRequest, ClientHello, Extension, Handshake,
        ServerHello,
    };

    /// A fixed client ephemeral X25519 private key for reproducible tests.
    const CLIENT_PRIV: [u8; 32] = [0x11; 32];
    /// A fixed server ephemeral X25519 private key for reproducible tests.
    const SERVER_PRIV: [u8; 32] = [0x22; 32];

    /// Serialize a handshake message to its wire bytes (panicking on the
    /// impossible encode error — these fixtures never overflow a length prefix).
    fn enc(msg: &Handshake) -> Vec<u8> {
        let mut out = Vec::new();
        msg.encode(&mut out).expect("fixture message encodes");
        out
    }

    /// A minimal but well-formed ClientHello whose only extension is the client's
    /// X25519 key share (its exact content is irrelevant to the flow beyond
    /// seeding the transcript).
    fn client_hello_bytes() -> Vec<u8> {
        let share = key_agreement::x25519_key_share(&CLIENT_PRIV);
        let key_share_body =
            KeyShareEntry::encode_client_hello(&[share]).expect("key_share encodes");
        let ch = Handshake::ClientHello(ClientHello {
            random: [0xAB; 32],
            legacy_session_id: Vec::new(),
            cipher_suites: vec![tls_message::TLS_AES_128_GCM_SHA256],
            extensions: vec![Extension::new(tls_message::EXT_KEY_SHARE, key_share_body)],
        });
        enc(&ch)
    }

    /// A ServerHello selecting TLS 1.3 + `TLS_AES_128_GCM_SHA256` and echoing the
    /// server's X25519 key share.
    fn server_hello_bytes() -> Vec<u8> {
        let share = key_agreement::x25519_key_share(&SERVER_PRIV);
        let key_share_body = share.encode_server_hello().expect("key_share encodes");
        let sh = Handshake::ServerHello(ServerHello {
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
        });
        enc(&sh)
    }

    /// EncryptedExtensions with an empty extension list.
    fn encrypted_extensions_bytes() -> Vec<u8> {
        enc(&Handshake::EncryptedExtensions(Vec::new()))
    }

    /// A server Certificate with a single dummy leaf entry.
    fn certificate_bytes() -> Vec<u8> {
        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![CertificateEntry {
                cert_data: vec![0x30, 0x03, 0x01, 0x02, 0x03],
                extensions: Vec::new(),
            }],
        }))
    }

    /// A server CertificateVerify with a dummy signature.
    fn certificate_verify_bytes() -> Vec<u8> {
        enc(&Handshake::CertificateVerify(tls_message::CertificateVerify {
            algorithm: 0x0804, // rsa_pss_rsae_sha256
            signature: vec![0xDE, 0xAD, 0xBE, 0xEF],
        }))
    }

    /// Compute the two Handshake traffic secrets exactly as the flow does, from
    /// the ClientHello+ServerHello transcript, so a test can forge a matching
    /// server Finished and predict the client Finished.
    fn handshake_traffic(ch: &[u8], sh: &[u8]) -> ([u8; 32], HandshakeTrafficSecrets) {
        let ecdhe = key_agreement::x25519_shared_secret(
            &CLIENT_PRIV,
            &key_agreement::x25519_public_key(&SERVER_PRIV),
        )
        .expect("shared secret");
        let hs_secret = tls_schedule::handshake_secret(&ecdhe);
        let mut transcript = ch.to_vec();
        transcript.extend_from_slice(sh);
        let th = tls_schedule::transcript_hash(&transcript);
        (hs_secret, HandshakeTrafficSecrets::derive(&hs_secret, &th))
    }

    /// Build a server Finished whose `verify_data` is correct for the transcript
    /// `ClientHello…CertificateVerify`.
    fn server_finished_bytes(
        server_secret: &[u8; 32],
        transcript_ch_cv: &[u8],
    ) -> Vec<u8> {
        let th = tls_schedule::transcript_hash(transcript_ch_cv);
        let vd = super::super::tls_finished::finished_verify_data(server_secret, &th);
        enc(&Handshake::Finished(vd.to_vec()))
    }

    /// Drive a full successful handshake and return the completion.
    fn run_full_handshake() -> (ClientHandshake, Box<HandshakeComplete>) {
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let ee = encrypted_extensions_bytes();
        let cert = certificate_bytes();
        let cv = certificate_verify_bytes();

        let (hs_secret, hs_traffic) = handshake_traffic(&ch, &sh);
        let mut transcript_ch_cv = ch.clone();
        transcript_ch_cv.extend_from_slice(&sh);
        transcript_ch_cv.extend_from_slice(&ee);
        transcript_ch_cv.extend_from_slice(&cert);
        transcript_ch_cv.extend_from_slice(&cv);
        let sf = server_finished_bytes(&hs_traffic.server, &transcript_ch_cv);
        let _ = hs_secret;

        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        assert_eq!(hs.state(), HandshakeState::ExpectServerHello);

        match hs.handle_message(&sh).expect("ServerHello") {
            HandshakeEvent::HandshakeKeysReady(keys) => {
                assert_eq!(keys, hs_traffic.packet_keys());
            }
            other => panic!("expected HandshakeKeysReady, got {other:?}"),
        }
        assert!(matches!(
            hs.handle_message(&ee).expect("EE"),
            HandshakeEvent::Continue
        ));
        assert!(matches!(
            hs.handle_message(&cert).expect("Certificate"),
            HandshakeEvent::Continue
        ));
        assert!(matches!(
            hs.handle_message(&cv).expect("CertificateVerify"),
            HandshakeEvent::Continue
        ));
        let complete = match hs.handle_message(&sf).expect("Finished") {
            HandshakeEvent::Complete(c) => c,
            other => panic!("expected Complete, got {other:?}"),
        };
        (hs, complete)
    }

    #[test]
    fn full_handshake_completes_and_matches_independent_derivation() {
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let ee = encrypted_extensions_bytes();
        let cert = certificate_bytes();
        let cv = certificate_verify_bytes();
        let (hs_secret, hs_traffic) = handshake_traffic(&ch, &sh);

        let (handshake, complete) = run_full_handshake();
        assert!(handshake.is_complete());
        assert_eq!(handshake.state(), HandshakeState::Complete);

        // Independently derive the 1-RTT keys and secrets over ClientHello…SF.
        let mut transcript_ch_sf = ch.clone();
        transcript_ch_sf.extend_from_slice(&sh);
        transcript_ch_sf.extend_from_slice(&ee);
        transcript_ch_sf.extend_from_slice(&cert);
        transcript_ch_sf.extend_from_slice(&cv);
        let mut transcript_ch_cv = transcript_ch_sf.clone();
        let sf = server_finished_bytes(&hs_traffic.server, &transcript_ch_cv);
        transcript_ch_sf.extend_from_slice(&sf);

        let master = tls_schedule::master_secret(&hs_secret);
        let th_sf = tls_schedule::transcript_hash(&transcript_ch_sf);
        let app = ApplicationTrafficSecrets::derive(&master, &th_sf);
        assert_eq!(complete.app_keys, app.packet_keys());
        assert_eq!(complete.app_secrets, app);
        assert_eq!(complete.master_secret, master);
        assert_eq!(
            complete.exporter_master_secret,
            tls_schedule::exporter_master_secret(&master, &th_sf)
        );

        // The emitted client Finished carries the correct verify_data over CH…SF.
        let expected_cf_vd =
            super::super::tls_finished::finished_verify_data(&hs_traffic.client, &th_sf);
        let (parsed, _) = Handshake::parse(&complete.client_finished)
            .expect("client Finished parses")
            .expect("complete message");
        assert_eq!(parsed, Handshake::Finished(expected_cf_vd.to_vec()));

        // Resumption secret is over CH…client Finished.
        transcript_ch_sf.extend_from_slice(&complete.client_finished);
        let th_cf = tls_schedule::transcript_hash(&transcript_ch_sf);
        assert_eq!(
            complete.resumption_master_secret,
            tls_schedule::resumption_master_secret(&master, &th_cf)
        );
        let _ = &mut transcript_ch_cv;
    }

    #[test]
    fn server_auth_messages_are_handed_back() {
        let (_hs, complete) = run_full_handshake();
        assert_eq!(complete.server_certificate.certificate_list.len(), 1);
        assert_eq!(complete.server_certificate_verify.algorithm, 0x0804);
        assert_eq!(complete.server_certificate_verify.signature, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn optional_certificate_request_is_accepted() {
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let ee = encrypted_extensions_bytes();
        let cr = enc(&Handshake::CertificateRequest(CertificateRequest {
            certificate_request_context: Vec::new(),
            extensions: Vec::new(),
        }));
        let cert = certificate_bytes();
        let cv = certificate_verify_bytes();

        let (_hs_secret, hs_traffic) = handshake_traffic(&ch, &sh);
        let mut t = ch.clone();
        for m in [&sh, &ee, &cr, &cert, &cv] {
            t.extend_from_slice(m);
        }
        let sf = server_finished_bytes(&hs_traffic.server, &t);

        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        hs.handle_message(&sh).expect("ServerHello");
        hs.handle_message(&ee).expect("EE");
        assert_eq!(
            hs.state(),
            HandshakeState::ExpectCertificateRequestOrCertificate
        );
        assert!(matches!(
            hs.handle_message(&cr).expect("CertificateRequest"),
            HandshakeEvent::Continue
        ));
        assert_eq!(hs.state(), HandshakeState::ExpectCertificate);
        hs.handle_message(&cert).expect("Certificate");
        hs.handle_message(&cv).expect("CertificateVerify");
        assert!(matches!(
            hs.handle_message(&sf).expect("Finished"),
            HandshakeEvent::Complete(_)
        ));
    }

    #[test]
    fn out_of_order_message_is_rejected_and_poisons_the_handshake() {
        let ch = client_hello_bytes();
        let ee = encrypted_extensions_bytes();
        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        // EncryptedExtensions before ServerHello is out of order.
        let err = hs.handle_message(&ee).expect_err("out of order");
        assert!(matches!(
            err,
            HandshakeError::UnexpectedMessage {
                got,
                ..
            } if got == tls_message::HS_ENCRYPTED_EXTENSIONS
        ));
        assert_eq!(hs.state(), HandshakeState::Failed);
        // Every later call now fails fast.
        let sh = server_hello_bytes();
        assert_eq!(
            hs.handle_message(&sh).expect_err("failed state"),
            HandshakeError::Failed
        );
    }

    #[test]
    fn tampered_server_finished_is_rejected() {
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let ee = encrypted_extensions_bytes();
        let cert = certificate_bytes();
        let cv = certificate_verify_bytes();
        let (_s, hs_traffic) = handshake_traffic(&ch, &sh);
        let mut t = ch.clone();
        for m in [&sh, &ee, &cert, &cv] {
            t.extend_from_slice(m);
        }
        let mut sf = server_finished_bytes(&hs_traffic.server, &t);
        // Flip a byte of the verify_data (last byte of the message).
        let last = sf.len() - 1;
        sf[last] ^= 0x01;

        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        hs.handle_message(&sh).expect("ServerHello");
        hs.handle_message(&ee).expect("EE");
        hs.handle_message(&cert).expect("Certificate");
        hs.handle_message(&cv).expect("CertificateVerify");
        assert_eq!(
            hs.handle_message(&sf).expect_err("bad MAC"),
            HandshakeError::FinishedVerificationFailed
        );
        assert_eq!(hs.state(), HandshakeState::Failed);
    }

    #[test]
    fn wrong_cipher_suite_is_rejected() {
        let ch = client_hello_bytes();
        let share = key_agreement::x25519_key_share(&SERVER_PRIV);
        let sh = enc(&Handshake::ServerHello(ServerHello {
            random: [0xCD; 32],
            legacy_session_id_echo: Vec::new(),
            cipher_suite: tls_message::TLS_AES_256_GCM_SHA384,
            extensions: vec![
                Extension::new(
                    tls_message::EXT_SUPPORTED_VERSIONS,
                    tls_message::VERSION_TLS13.to_be_bytes().to_vec(),
                ),
                Extension::new(
                    tls_message::EXT_KEY_SHARE,
                    share.encode_server_hello().unwrap(),
                ),
            ],
        }));
        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        assert_eq!(
            hs.handle_message(&sh).expect_err("bad suite"),
            HandshakeError::UnsupportedCipherSuite(tls_message::TLS_AES_256_GCM_SHA384)
        );
    }

    #[test]
    fn missing_key_share_is_rejected() {
        let ch = client_hello_bytes();
        let sh = enc(&Handshake::ServerHello(ServerHello {
            random: [0xCD; 32],
            legacy_session_id_echo: Vec::new(),
            cipher_suite: tls_message::TLS_AES_128_GCM_SHA256,
            extensions: vec![Extension::new(
                tls_message::EXT_SUPPORTED_VERSIONS,
                tls_message::VERSION_TLS13.to_be_bytes().to_vec(),
            )],
        }));
        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        assert_eq!(
            hs.handle_message(&sh).expect_err("no key share"),
            HandshakeError::MissingKeyShare
        );
    }

    #[test]
    fn hello_retry_request_is_rejected() {
        let ch = client_hello_bytes();
        let sh = enc(&Handshake::ServerHello(ServerHello {
            random: tls_message::HELLO_RETRY_REQUEST_RANDOM,
            legacy_session_id_echo: Vec::new(),
            cipher_suite: tls_message::TLS_AES_128_GCM_SHA256,
            extensions: vec![Extension::new(
                tls_message::EXT_SUPPORTED_VERSIONS,
                tls_message::VERSION_TLS13.to_be_bytes().to_vec(),
            )],
        }));
        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        assert_eq!(
            hs.handle_message(&sh).expect_err("HRR"),
            HandshakeError::HelloRetryRequestUnsupported
        );
    }

    #[test]
    fn non_tls13_version_is_rejected() {
        let ch = client_hello_bytes();
        let share = key_agreement::x25519_key_share(&SERVER_PRIV);
        let sh = enc(&Handshake::ServerHello(ServerHello {
            random: [0xCD; 32],
            legacy_session_id_echo: Vec::new(),
            cipher_suite: tls_message::TLS_AES_128_GCM_SHA256,
            extensions: vec![
                Extension::new(tls_message::EXT_SUPPORTED_VERSIONS, vec![0x03, 0x03]),
                Extension::new(
                    tls_message::EXT_KEY_SHARE,
                    share.encode_server_hello().unwrap(),
                ),
            ],
        }));
        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        assert_eq!(
            hs.handle_message(&sh).expect_err("tls 1.2"),
            HandshakeError::UnsupportedVersion
        );
    }

    #[test]
    fn trailing_bytes_after_a_message_are_rejected() {
        let ch = client_hello_bytes();
        let mut sh = server_hello_bytes();
        sh.push(0x00); // one extra byte beyond the complete ServerHello
        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        assert_eq!(
            hs.handle_message(&sh).expect_err("trailing"),
            HandshakeError::NotOneMessage
        );
    }

    #[test]
    fn incomplete_message_is_rejected() {
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut hs = ClientHandshake::new(CLIENT_PRIV, &ch);
        // Truncate to less than the full message.
        assert_eq!(
            hs.handle_message(&sh[..sh.len() - 1]).expect_err("short"),
            HandshakeError::NotOneMessage
        );
    }

    #[test]
    fn extra_message_after_complete_is_rejected_without_poisoning() {
        let (mut hs, _complete) = run_full_handshake();
        let ee = encrypted_extensions_bytes();
        assert_eq!(
            hs.handle_message(&ee).expect_err("after complete"),
            HandshakeError::AlreadyComplete
        );
        // Complete state is preserved, not turned into Failed.
        assert_eq!(hs.state(), HandshakeState::Complete);
        assert!(hs.is_complete());
    }
}
