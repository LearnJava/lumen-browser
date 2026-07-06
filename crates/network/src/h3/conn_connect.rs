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
//! carries is authenticated the moment it appears
//! ([`conn_cert_auth::authenticate_server_certificate`](super::conn_cert_auth::authenticate_server_certificate)):
//! the CertificateVerify signature must prove the peer holds the end-entity
//! certificate's private key over this handshake, that same certificate's
//! `subjectAltName` must name the requested host
//! ([`x509_hostname::verify_certificate_hostname`](super::x509_hostname::verify_certificate_hostname),
//! RFC 6125 §6), every certificate in the presented list must be valid at the
//! caller's wall-clock now
//! ([`x509_validity::verify_validity_chain`](super::x509_validity::verify_validity_chain),
//! RFC 5280 §4.1.2.5, §6.1.3(a)(2)), the whole presented `certificate_list` must form a
//! self-consistent signature chain, each certificate signed by the next
//! ([`x509_chain::verify_chain_signatures`](super::x509_chain::verify_chain_signatures),
//! RFC 5280 §4.1.1.3, RFC 8446 §4.4.2), that same list must form a
//! self-consistent *name* chain, each certificate's `issuer` distinguished name equal to
//! the next certificate's `subject`
//! ([`x509_name_chain::verify_name_chain`](super::x509_name_chain::verify_name_chain),
//! RFC 5280 §4.1.2.4, §4.1.2.6, §6.1), every certificate that issues
//! another must be a permitted CA, asserting `basicConstraints` `cA = TRUE` and honouring
//! its `pathLenConstraint`
//! ([`x509_basic_constraints::verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints),
//! RFC 5280 §4.2.1.9, §6.1.4), every issuing certificate that carries a
//! `keyUsage` extension must assert `keyCertSign`
//! ([`x509_key_usage::verify_cert_sign_usage`](super::x509_key_usage::verify_cert_sign_usage),
//! RFC 5280 §4.2.1.3), that same chain must actually *terminate* at a
//! trust anchor the caller supplies: the topmost certificate's `issuer` must name one of
//! [`ConnectDriver::new`]'s `trust_anchors`, and the topmost certificate must really be
//! signed by that anchor's key
//! ([`x509_trust_anchor::verify_trust_anchor`](super::x509_trust_anchor::verify_trust_anchor),
//! RFC 5280 §6.1, §4.1.1.3), and — this slice — the end-entity leaf must be authorised
//! for TLS server authentication: when it carries an `extendedKeyUsage` extension that
//! extension must name `serverAuth` (or the catch-all `anyExtendedKeyUsage`)
//! ([`x509_ext_key_usage::verify_server_auth_eku`](super::x509_ext_key_usage::verify_server_auth_eku),
//! RFC 5280 §4.2.1.12) — all nine *before* the client Finished goes on
//! the wire. The verified completion is then retained ([`ConnectDriver::completed`]).
//!
//! ## What it defers
//!
//! - **The production trust store.** [`ConnectDriver::new`] takes the caller's
//!   `trust_anchors` as a plain parameter — the same shape
//!   [`x509_trust_anchor::TrustAnchor`](super::x509_trust_anchor::TrustAnchor)
//!   documents: the existing HTTP/1.1 / HTTP/2 TLS path already populates a
//!   `rustls::RootCertStore` from `webpki_roots::TLS_SERVER_ROOTS`
//!   (`crates/network/src/lib.rs`), and the slice that wires QUIC into `HttpClient` is
//!   where the real Mozilla root list reaches this driver the same way. Nothing here
//!   hard-codes a trust store.
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

use super::conn_cert_auth::{CertAuthError, authenticate_server_certificate};
use super::conn_handshake::{HandshakeDriver, HandshakeError, PollOutcome};
use super::conn_tls::{TlsConnError, TlsConnState};
use super::conn_turn::{ConnectionTurn, TurnEffect};
use super::send_path::FlushError;
use super::tls_handshake::HandshakeComplete;
use super::udp::DatagramTransport;
use super::x509_basic_constraints::{CaConstraintsWalkError, verify_ca_constraints};
use super::x509_chain::{ChainWalkError, verify_chain_signatures};
use super::x509_ext_key_usage::{ExtKeyUsageWalkError, verify_server_auth_eku};
use super::x509_hostname::{HostnameError, verify_certificate_hostname};
use super::x509_key_usage::{KeyUsageWalkError, verify_cert_sign_usage};
use super::x509_name_chain::{NameChainWalkError, verify_name_chain};
use super::x509_trust_anchor::{TrustAnchor, TrustAnchorWalkError, verify_trust_anchor};
use super::x509_validity::{ValidityWalkError, verify_validity_chain};

/// One trust anchor a presented chain may terminate at (RFC 5280 §6.1): the anchor's
/// `subject` and `subjectPublicKeyInfo`, owned so [`ConnectDriver`] need not carry the
/// borrowed lifetime
/// [`x509_trust_anchor::TrustAnchor`](super::x509_trust_anchor::TrustAnchor) itself
/// requires. [`ConnectDriver::poll`] borrows both fields fresh each turn to build the
/// borrowed `TrustAnchor` [`verify_trust_anchor`] takes — the caller only ever holds
/// this owned form. Populated the same way the existing HTTP/1.1 / HTTP/2 TLS path
/// populates its `rustls::RootCertStore` (`tls::build_client_config`): a real trust
/// store is the caller's job, not a compiled-in list.
#[derive(Clone, Debug)]
pub struct OwnedTrustAnchor {
    /// The anchor's `subject` distinguished name DER (RFC 5280 §4.1.2.6), including its
    /// own outer `SEQUENCE` — comparable byte-for-byte against a certificate's `issuer`.
    pub subject: Vec<u8>,
    /// The anchor's `subjectPublicKeyInfo` DER (RFC 5280 §4.1.2.7), including its own
    /// outer `SEQUENCE`.
    pub subject_public_key_info: Vec<u8>,
}

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
    /// The server certificate did not authenticate (RFC 8446 §4.4.3): the
    /// CertificateVerify signature did not prove the peer holds the end-entity
    /// certificate's private key over this handshake, or the certificate could not be
    /// decoded ([`CertAuthError`](super::conn_cert_auth::CertAuthError)). The
    /// connection is abandoned before the client Finished is flushed.
    CertAuth(CertAuthError),
    /// The authenticated certificate does not name the requested host (RFC 6125 §6,
    /// RFC 9110 §4.3.4): its `subjectAltName` `dNSName` entries do not cover the
    /// [`reference_host`](ConnectDriver) the client asked for, or the certificate
    /// carried no DNS identifier at all ([`HostnameError`](super::x509_hostname::HostnameError)).
    /// Checked only after possession is proven; like [`CertAuth`](Self::CertAuth) it
    /// abandons the connection before the client Finished is flushed.
    Hostname(HostnameError),
    /// Some certificate in the server-presented chain is not valid at the current time
    /// (RFC 5280 §4.1.2.5, §6.1.3(a)(2)): *now* falls outside its
    /// `[notBefore, notAfter]` window, or its `validity` field could not be decoded
    /// ([`ValidityWalkError`](super::x509_validity::ValidityWalkError), naming the failing
    /// certificate's index and the underlying
    /// [`ValidityError`](super::x509_validity::ValidityError)). Checked after possession
    /// and identity against the wall-clock [`now_unix`](ConnectDriver) the caller supplied,
    /// over *every* certificate in the list — an expired intermediate is as fatal as an
    /// expired leaf, since it can no longer be trusted to have issued the certificate below
    /// it. Like the two before it, it abandons the connection before the client Finished is
    /// flushed.
    Validity(ValidityWalkError),
    /// The server-presented certificate chain is not internally self-consistent
    /// (RFC 5280 §4.1.1.3, RFC 8446 §4.4.2): some certificate in the `certificate_list`
    /// is not signed by the next one up, or an issuer certificate's
    /// `SubjectPublicKeyInfo` could not be extracted
    /// ([`ChainWalkError`](super::x509_chain::ChainWalkError)). Checked after
    /// possession, identity, and time hold for the end-entity certificate; like the
    /// three before it, it abandons the connection before the client Finished is
    /// flushed. Verifying the chain's internal links does not itself terminate it at a
    /// trusted root — that is [`TrustAnchor`](Self::TrustAnchor), checked last.
    Chain(ChainWalkError),
    /// The server-presented certificate chain does not form a self-consistent *name*
    /// chain (RFC 5280 §4.1.2.4, §4.1.2.6, §6.1, RFC 8446 §4.4.2): some certificate's
    /// `issuer` distinguished name does not match the `subject` distinguished name of the
    /// next certificate in the `certificate_list`, or a certificate's Names could not be
    /// extracted ([`NameChainWalkError`](super::x509_name_chain::NameChainWalkError)). The
    /// name complement of [`Chain`](Self::Chain): a chain must be both *signed* by each
    /// next certificate and *named* after it. Checked after possession, identity, time,
    /// and the chain-signature walk hold; like the four before it, it abandons the
    /// connection before the client Finished is flushed. Confirming the name links does
    /// not itself terminate the chain at a trusted root — that is
    /// [`TrustAnchor`](Self::TrustAnchor), checked last.
    NameChain(NameChainWalkError),
    /// The server-presented certificate chain is not a permitted issuance path (RFC 5280
    /// §4.2.1.9, §6.1.4, RFC 8446 §4.4.2): some certificate that *issues* another does not
    /// assert `basicConstraints` `cA = TRUE`, or its `pathLenConstraint` is smaller than
    /// the number of intermediates below it, or a certificate's `basicConstraints` could
    /// not be extracted ([`CaConstraintsWalkError`](super::x509_basic_constraints::CaConstraintsWalkError)).
    /// The CA-permission complement of [`Chain`](Self::Chain) and [`NameChain`](Self::NameChain):
    /// a chain must be *signed* by, *named* after, and *issuable* by each next certificate.
    /// Without it a valid leaf certificate could be presented as an intermediate to mint
    /// certificates for any name beneath it. Checked after possession, identity, time, the
    /// chain-signature walk, and the name walk hold; like the five before it, it abandons
    /// the connection before the client Finished is flushed. Confirming the presented
    /// certificates are permitted issuers does not itself terminate the chain at a
    /// trusted root — that is [`TrustAnchor`](Self::TrustAnchor), checked last.
    CaConstraints(CaConstraintsWalkError),
    /// The server-presented certificate chain uses a certificate for signing that is not
    /// permitted to sign certificates (RFC 5280 §4.2.1.3, §6.1.4, RFC 8446 §4.4.2): some
    /// certificate that *issues* another carries a `keyUsage` extension that does not assert
    /// `keyCertSign`, or a certificate's `keyUsage` could not be extracted
    /// ([`KeyUsageWalkError`](super::x509_key_usage::KeyUsageWalkError)). The `keyUsage`
    /// complement of [`CaConstraints`](Self::CaConstraints): an issuing certificate must be
    /// both marked a CA (`basicConstraints` `cA = TRUE`) and, when it constrains its key,
    /// permitted to sign certificates. Without it a CA certificate restricted to, say, TLS
    /// server authentication could still be accepted as a certificate signer. Checked after
    /// possession, identity, time, the chain-signature walk, the name walk, and the
    /// basicConstraints walk hold; like the six before it, it abandons the connection before
    /// the client Finished is flushed. An issuer that carries no `keyUsage` is unconstrained
    /// (§4.2.1.3) and passes. Confirming certificate-signing permission does not yet
    /// terminate the chain at a trusted root (RFC 5280 §6.1) — that is
    /// [`TrustAnchor`](Self::TrustAnchor), checked next.
    KeyUsage(KeyUsageWalkError),
    /// The server-presented certificate chain does not terminate at a trust anchor the
    /// caller supplies (RFC 5280 §6.1, §4.1.1.3, RFC 8446 §4.4.2): the topmost
    /// certificate's `issuer` does not name any [`OwnedTrustAnchor`] in
    /// [`ConnectDriver::new`]'s trust store, or the topmost certificate is not really
    /// signed by the matched anchor's key
    /// ([`TrustAnchorWalkError`](super::x509_trust_anchor::TrustAnchorWalkError)). The
    /// termination complement of [`Chain`](Self::Chain), [`NameChain`](Self::NameChain),
    /// [`CaConstraints`](Self::CaConstraints), and [`KeyUsage`](Self::KeyUsage): those four
    /// walks all stop one link short of the anchor, proving the chain is internally
    /// self-consistent without proving it is *rooted* in anything the caller trusts.
    /// Checked after possession, identity, time, and all four chain walks hold; like the
    /// seven checks before it, it abandons the connection before the client Finished is
    /// flushed.
    TrustAnchor(TrustAnchorWalkError),
    /// The server-presented end-entity leaf is not authorised for TLS server
    /// authentication (RFC 5280 §4.2.1.12, RFC 8446 §4.4.2): the leaf carries an
    /// `extendedKeyUsage` extension that names neither `id-kp-serverAuth`
    /// (1.3.6.1.5.5.7.3.1) nor the catch-all `anyExtendedKeyUsage` (2.5.29.37.0), or the
    /// leaf's `extendedKeyUsage` could not be decoded
    /// ([`ExtKeyUsageWalkError`](super::x509_ext_key_usage::ExtKeyUsageWalkError)). The
    /// purpose-level complement of [`KeyUsage`](Self::KeyUsage): where `keyUsage` governs
    /// what an *issuing* key may do, `extendedKeyUsage` governs the application purposes
    /// the leaf's key is certified for — a leaf restricted to, say, `clientAuth` or
    /// `codeSigning` must not authenticate a TLS *server*. A leaf that carries no
    /// `extendedKeyUsage` extension is purpose-unrestricted (§4.2.1.12) and passes; only
    /// the leaf is consulted, since RFC 5280 does not mandate EKU chaining. Checked last,
    /// after possession, identity, time, all four chain walks, and the trust-anchor
    /// termination hold; like the eight checks before it, it abandons the connection
    /// before the client Finished is flushed.
    ExtKeyUsage(ExtKeyUsageWalkError),
}

impl core::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Loop(e) => write!(f, "QUIC connect: {e}"),
            Self::Tls(e) => write!(f, "QUIC connect: {e}"),
            Self::Flush(e) => write!(f, "QUIC connect: flush failed: {e}"),
            Self::CertAuth(e) => write!(f, "QUIC connect: certificate authentication failed: {e}"),
            Self::Hostname(e) => write!(f, "QUIC connect: certificate hostname check failed: {e}"),
            Self::Validity(e) => write!(f, "QUIC connect: certificate validity check failed: {e}"),
            Self::Chain(e) => {
                write!(f, "QUIC connect: certificate chain verification failed: {e}")
            }
            Self::NameChain(e) => {
                write!(f, "QUIC connect: certificate name-chain verification failed: {e}")
            }
            Self::CaConstraints(e) => {
                write!(f, "QUIC connect: certificate basicConstraints verification failed: {e}")
            }
            Self::KeyUsage(e) => {
                write!(f, "QUIC connect: certificate keyUsage verification failed: {e}")
            }
            Self::TrustAnchor(e) => {
                write!(f, "QUIC connect: certificate trust-anchor verification failed: {e}")
            }
            Self::ExtKeyUsage(e) => {
                write!(f, "QUIC connect: certificate extendedKeyUsage verification failed: {e}")
            }
        }
    }
}

impl std::error::Error for ConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Loop(e) => Some(e),
            Self::Tls(e) => Some(e),
            Self::Flush(e) => Some(e),
            Self::CertAuth(e) => Some(e),
            Self::Hostname(e) => Some(e),
            Self::Validity(e) => Some(e),
            Self::Chain(e) => Some(e),
            Self::NameChain(e) => Some(e),
            Self::CaConstraints(e) => Some(e),
            Self::KeyUsage(e) => Some(e),
            Self::TrustAnchor(e) => Some(e),
            Self::ExtKeyUsage(e) => Some(e),
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
    /// The host the client asked for — the SNI it put in its ClientHello and the URL
    /// authority the request targets. The end-entity certificate must name this host
    /// (RFC 6125 §6): once the handshake completes, [`poll`](ConnectDriver::poll)
    /// matches it against the certificate's `subjectAltName` before the connection may
    /// carry application data.
    reference_host: String,
    /// The wall-clock instant the certificate's validity period is judged against —
    /// seconds since the Unix epoch (1970-01-01T00:00:00Z). The loop reads only the
    /// monotonic [`Instant`] clock for its timers (which says nothing about the calendar
    /// date), so the caller supplies the wall-clock *now* here; once the handshake
    /// completes, [`poll`](ConnectDriver::poll) checks the end-entity certificate's
    /// `[notBefore, notAfter]` window against it (RFC 5280 §4.1.2.5) before the
    /// connection may carry application data.
    now_unix: i64,
    /// The caller-supplied trust store a presented chain's topmost certificate must
    /// terminate at (RFC 5280 §6.1). Owned rather than borrowed so this driver need not
    /// carry the lifetime [`x509_trust_anchor::TrustAnchor`](super::x509_trust_anchor::TrustAnchor)
    /// itself requires; [`poll`](ConnectDriver::poll) borrows from it fresh each turn.
    trust_anchors: Vec<OwnedTrustAnchor>,
}

impl<T: DatagramTransport> ConnectDriver<T> {
    /// Joins a control-flow `handshake` loop and a `tls` bridge into one connect
    /// driver targeting `reference_host`. The handshake's turn should already have the
    /// Initial space installed on both halves; the first flight is sent on the first
    /// turn of [`connect`](ConnectDriver::connect).
    ///
    /// `reference_host` is the authority the request targets (the SNI the ClientHello
    /// carries): the server's end-entity certificate must name it (RFC 6125 §6) for the
    /// handshake to complete. `now_unix` is the wall-clock instant the certificate's
    /// validity period is judged against (seconds since the Unix epoch): the end-entity
    /// certificate must be valid at it (RFC 5280 §4.1.2.5) for the handshake to complete.
    /// `trust_anchors` is the trust store the presented chain's topmost certificate must
    /// terminate at (RFC 5280 §6.1) — the caller's job to populate, the same way the
    /// existing HTTP/1.1 / HTTP/2 TLS path populates its `rustls::RootCertStore`.
    #[must_use]
    pub fn new(
        handshake: HandshakeDriver<T>,
        tls: TlsConnState,
        reference_host: impl Into<String>,
        now_unix: i64,
        trust_anchors: Vec<OwnedTrustAnchor>,
    ) -> Self {
        Self {
            handshake,
            tls,
            completed: None,
            reference_host: reference_host.into(),
            now_unix,
            trust_anchors,
        }
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

    /// The completed handshake material, available once the TLS handshake completes
    /// (the server Finished verified), its server certificate authenticated (the
    /// CertificateVerify signature checked, RFC 8446 §4.4.3), that certificate verified
    /// to name the requested host (RFC 6125 §6), verified valid at the caller's
    /// wall-clock now (RFC 5280 §4.1.2.5), *and* the presented chain verified internally
    /// self-consistent — each certificate signed by the next (RFC 5280 §4.1.1.3), named
    /// after it (each `issuer` matching the next `subject`, RFC 5280 §4.1.2.4, §4.1.2.6,
    /// §6.1), permitted to issue it (each issuer a CA, `basicConstraints`
    /// `cA = TRUE` with any `pathLenConstraint` honoured, RFC 5280 §4.2.1.9, §6.1.4),
    /// permitted to sign it (each issuer's `keyUsage`, when present, asserting `keyCertSign`,
    /// RFC 5280 §4.2.1.3), really rooted in one of the caller's `trust_anchors`
    /// (the topmost certificate's `issuer` names an anchor and its signature verifies
    /// under that anchor's key, RFC 5280 §6.1, §4.1.1.3), *and* authorised for TLS server
    /// authentication (the end-entity leaf's `extendedKeyUsage`, when present, naming
    /// `serverAuth` or `anyExtendedKeyUsage`, RFC 5280 §4.2.1.12). `None` while the
    /// handshake is still in progress; a completion whose certificate fails to
    /// authenticate, does not cover [`reference_host`](ConnectDriver), is outside its
    /// validity period, heads a chain whose signature, name, CA-permission, or
    /// certificate-signing links do not verify, does not terminate at a trusted anchor,
    /// or whose leaf is not authorised for TLS server authentication never reaches here —
    /// [`poll`](ConnectDriver::poll) returns [`ConnectError::CertAuth`],
    /// [`ConnectError::Hostname`], [`ConnectError::Validity`], [`ConnectError::Chain`],
    /// [`ConnectError::NameChain`], [`ConnectError::CaConstraints`],
    /// [`ConnectError::KeyUsage`], [`ConnectError::TrustAnchor`], or
    /// [`ConnectError::ExtKeyUsage`] instead.
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
    /// Finished). When the handshake completes it authenticates the server certificate
    /// ([`authenticate_server_certificate`](super::conn_cert_auth::authenticate_server_certificate))
    /// verifies it names [`reference_host`](ConnectDriver)
    /// ([`verify_certificate_hostname`](super::x509_hostname::verify_certificate_hostname),
    /// RFC 6125 §6), verifies every certificate in the presented list is valid at
    /// [`now_unix`](ConnectDriver)
    /// ([`verify_validity_chain`](super::x509_validity::verify_validity_chain),
    /// RFC 5280 §4.1.2.5, §6.1.3(a)(2)), verifies the presented chain is internally self-consistent by
    /// signature ([`verify_chain_signatures`](super::x509_chain::verify_chain_signatures),
    /// RFC 5280 §4.1.1.3, RFC 8446 §4.4.2), and verifies it is self-consistent by name —
    /// each `issuer` matching the next `subject`
    /// ([`verify_name_chain`](super::x509_name_chain::verify_name_chain),
    /// RFC 5280 §4.1.2.4, §4.1.2.6, §6.1), and verifies every issuing certificate is a
    /// permitted CA — `basicConstraints` `cA = TRUE` with any `pathLenConstraint` honoured
    /// ([`verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints),
    /// RFC 5280 §4.2.1.9, §6.1.4), verifies every issuing certificate is permitted to
    /// sign certificates — its `keyUsage`, when present, asserting `keyCertSign`
    /// ([`verify_cert_sign_usage`](super::x509_key_usage::verify_cert_sign_usage),
    /// RFC 5280 §4.2.1.3), verifies that chain actually terminates at one of the
    /// caller's [`trust_anchors`](ConnectDriver::new)
    /// ([`verify_trust_anchor`](super::x509_trust_anchor::verify_trust_anchor),
    /// RFC 5280 §6.1, §4.1.1.3) — and verifies the end-entity leaf is authorised for TLS
    /// server authentication — its `extendedKeyUsage`, when present, naming `serverAuth`
    /// or `anyExtendedKeyUsage`
    /// ([`verify_server_auth_eku`](super::x509_ext_key_usage::verify_server_auth_eku),
    /// RFC 5280 §4.2.1.12) — before flushing the client Finished the
    /// advance enqueued (RFC 8446 §4.4.3, §4.4.4) and retaining the completion
    /// ([`completed`](ConnectDriver::completed)). A certificate that fails to
    /// authenticate, that does not cover the requested host, that is outside its validity
    /// period, that heads a chain whose signature, name, CA-permission, or
    /// certificate-signing links do not verify, that does not terminate at a trusted
    /// anchor, or whose leaf is not authorised for TLS server authentication abandons the
    /// connection before the client Finished goes out. On a timer wake it advances no TLS.
    ///
    /// # Errors
    ///
    /// [`ConnectError`] wrapping the failing step: a control-flow error from the poll,
    /// a TLS error from the advance, a server certificate that failed to authenticate,
    /// does not name the requested host, is outside its validity period, heads a
    /// chain whose signature, name, CA-permission, or certificate-signing links do not
    /// verify, does not terminate at a trusted anchor, whose leaf is not authorised for
    /// TLS server authentication, or a failed flush of the client Finished.
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
                // Authenticate the server certificate before completing (RFC 8446
                // §4.4.3): the CertificateVerify must prove the peer holds the
                // end-entity certificate's private key over the ClientHello…Certificate
                // transcript. A failure abandons the connection *before* the client
                // Finished is flushed, so we never signal a successful handshake to an
                // unauthenticated peer.
                authenticate_server_certificate(&complete).map_err(ConnectError::CertAuth)?;
                // Possession is proven; now check *identity* (RFC 6125 §6, RFC 9110
                // §4.3.4): the same end-entity certificate must name the host the client
                // asked for. A certificate whose subjectAltName does not cover
                // `reference_host` is as fatal as a bad signature — abandon before the
                // client Finished goes out. The first entry is the end-entity
                // certificate (RFC 8446 §4.4.2); authentication above already proved it
                // is present.
                let entry = complete
                    .server_certificate
                    .certificate_list
                    .first()
                    .ok_or(ConnectError::CertAuth(CertAuthError::NoCertificate))?;
                verify_certificate_hostname(&entry.cert_data, &self.reference_host)
                    .map_err(ConnectError::Hostname)?;
                // Possession and identity hold; now check *time* (RFC 5280 §4.1.2.5,
                // §6.1.3(a)(2)): *every* certificate in the presented list — not just the
                // end-entity leaf — must be valid at the wall-clock now the caller supplied.
                // A certificate not yet valid or already expired is as fatal as a bad
                // signature or a mis-named host; in particular an expired intermediate is as
                // fatal as an expired leaf, since a certificate outside its validity window
                // can no longer be trusted to have issued the one below it. The loop's own
                // clock is monotonic and says nothing about the calendar date, so the
                // validity window is judged against `now_unix`, not the `Instant` timers run
                // on. The chain is assembled once here and reused by every walk below.
                let chain: Vec<&[u8]> = complete
                    .server_certificate
                    .certificate_list
                    .iter()
                    .map(|e| e.cert_data.as_slice())
                    .collect();
                verify_validity_chain(&chain, self.now_unix).map_err(ConnectError::Validity)?;
                // Possession, identity, and time hold for the whole chain; now check that it
                // is internally self-consistent (RFC 5280 §4.1.1.3, RFC 8446 §4.4.2): every
                // certificate in the list must be signed by the next one up. A chain whose
                // internal links do not verify is as fatal as a bad end-entity signature, a
                // mis-named host, or an expired certificate — abandon before the client
                // Finished goes out. A single-certificate list has no internal links and
                // passes vacuously; terminating the chain at a trusted root (RFC 5280 §6.1)
                // is the trust-anchor walk below.
                verify_chain_signatures(&chain).map_err(ConnectError::Chain)?;
                // The signatures link the chain; now check its *names* line up
                // (RFC 5280 §4.1.2.4, §4.1.2.6, §6.1, RFC 8446 §4.4.2): every
                // certificate's `issuer` distinguished name must equal the `subject`
                // distinguished name of the next certificate up. A signed-but-mis-named
                // chain is a spliced-together set of certificates rather than an ordered
                // issuance path — as fatal as a broken signature link, so abandon before
                // the client Finished goes out. Like the signature walk it passes a
                // single-certificate list vacuously and stops one link short of the trust
                // anchor (RFC 5280 §6.1) — the trust-anchor walk below checks that link.
                verify_name_chain(&chain).map_err(ConnectError::NameChain)?;
                // The signatures link the chain and its names line up; now check every
                // issuing certificate is *permitted* to issue (RFC 5280 §4.2.1.9, §6.1.4,
                // RFC 8446 §4.4.2): each certificate above the leaf must assert
                // `basicConstraints` `cA = TRUE` and honour its `pathLenConstraint`.
                // Without this a valid, correctly-signed, correctly-named *leaf* could be
                // presented as an *intermediate* and mint certificates for any name beneath
                // it — as fatal as a broken signature or name link, so abandon before the
                // client Finished goes out. Like the two walks before it a single-certificate
                // list has no issuer and passes vacuously, and it stops short of terminating
                // the chain at a trust anchor (RFC 5280 §6.1) — the trust-anchor walk below
                // checks that link.
                verify_ca_constraints(&chain).map_err(ConnectError::CaConstraints)?;
                // Each issuer is a permitted CA; now check every issuing certificate is also
                // permitted to *sign certificates* specifically (RFC 5280 §4.2.1.3, §6.1.4,
                // RFC 8446 §4.4.2): if a certificate above the leaf carries a `keyUsage`
                // extension it must assert `keyCertSign`. The `keyUsage` complement of the
                // basicConstraints walk: `cA = TRUE` marks a CA, but a CA whose key is
                // restricted (say, to TLS server auth) must not be accepted as a certificate
                // signer — as fatal as a non-CA issuer, so abandon before the client Finished
                // goes out. An issuer carrying no `keyUsage` is unconstrained (§4.2.1.3) and
                // passes; like the walks before it a single-certificate list has no issuer and
                // passes vacuously, and it stops short of terminating the chain at a trust
                // anchor (RFC 5280 §6.1) — checked next.
                verify_cert_sign_usage(&chain).map_err(ConnectError::KeyUsage)?;
                // Every internal link, name, CA-permission, and signing-permission check
                // holds; now check the chain actually *terminates* at a trust anchor the
                // caller trusts (RFC 5280 §6.1, §4.1.1.3): the topmost certificate's
                // `issuer` must name one of `self.trust_anchors`, and that certificate must
                // really be signed by the matched anchor's key. Every walk above stops one
                // link short of the anchor — a chain internally consistent in signature,
                // name, CA-permission, and signing-permission could still be rooted in a
                // certificate nobody asked this connection to trust, as fatal as any link
                // before it, so abandon before the client Finished goes out. `TrustAnchor`
                // borrows fresh from the owned `trust_anchors` store each turn.
                let anchors: Vec<TrustAnchor<'_>> = self
                    .trust_anchors
                    .iter()
                    .map(|a| TrustAnchor {
                        subject: &a.subject,
                        subject_public_key_info: &a.subject_public_key_info,
                    })
                    .collect();
                verify_trust_anchor(&chain, &anchors).map_err(ConnectError::TrustAnchor)?;
                // The chain is signed, named, CA-permitted, signing-permitted, and rooted
                // in a trusted anchor; the last check is *purpose* (RFC 5280 §4.2.1.12,
                // RFC 8446 §4.4.2): the end-entity leaf must be authorised for TLS server
                // authentication. If it carries an `extendedKeyUsage` extension that
                // extension must name `serverAuth` (or the catch-all `anyExtendedKeyUsage`);
                // a leaf restricted to another purpose (say `clientAuth` or `codeSigning`)
                // must not authenticate a TLS server, as fatal as any check before it, so
                // abandon before the client Finished goes out. A leaf carrying no
                // `extendedKeyUsage` is purpose-unrestricted (§4.2.1.12) and passes; only
                // the leaf is consulted, since RFC 5280 does not mandate EKU chaining up the
                // path.
                verify_server_auth_eku(&chain).map_err(ConnectError::ExtKeyUsage)?;
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
    use crate::h3::x509_validity::ValidityError;
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
    use crate::h3::tls_cert_verify::{CertVerifyRole, certificate_verify_content, signature_scheme};
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

    /// A connect driver over the scripted transport `t` targeting `host` and judging
    /// the certificate's validity period against `now_unix`, seeded with
    /// [`trust_anchors`] as its trust store, with the Initial space installed on both
    /// halves and the TLS bridge seeded with the ClientHello.
    fn connect_driver_for_at(
        t: MockDatagramTransport,
        now: Instant,
        host: &str,
        now_unix: i64,
    ) -> ConnectDriver<MockDatagramTransport> {
        connect_driver_with_anchors(t, now, host, now_unix, trust_anchors())
    }

    /// Like [`connect_driver_for_at`] but with an explicit trust store, so a fixture can
    /// exercise the trust-anchor walk itself (an empty store, or one naming an anchor
    /// under the wrong key).
    fn connect_driver_with_anchors(
        t: MockDatagramTransport,
        now: Instant,
        host: &str,
        now_unix: i64,
        anchors: Vec<OwnedTrustAnchor>,
    ) -> ConnectDriver<MockDatagramTransport> {
        let mut send = ConnectionSendState::new(1, dcid(), scid(), 1200);
        send.install(PacketNumberSpace::Initial, initial_keys().client);
        let turn = ConnectionTurn::new(driver(t, now), send, 1200, DEFAULT_ACK_DELAY_EXPONENT);
        let handshake = HandshakeDriver::new(turn);
        let tls = TlsConnState::new(CLIENT_PRIV, client_hello_bytes());
        ConnectDriver::new(handshake, tls, host, now_unix, anchors)
    }

    /// A connect driver over `t` targeting `host` at [`SERVER_NOW`] — inside the fixture
    /// certificate's validity window, so only the hostname differs from the passing case.
    fn connect_driver_for(
        t: MockDatagramTransport,
        now: Instant,
        host: &str,
    ) -> ConnectDriver<MockDatagramTransport> {
        connect_driver_for_at(t, now, host, SERVER_NOW)
    }

    /// A connect driver targeting [`SERVER_HOSTNAME`] at [`SERVER_NOW`] — the host the
    /// fixture certificate names and a *now* inside its validity window, so both the
    /// hostname and validity checks pass.
    fn connect_driver(
        t: MockDatagramTransport,
        now: Instant,
    ) -> ConnectDriver<MockDatagramTransport> {
        connect_driver_for(t, now, SERVER_HOSTNAME)
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

    /// A fixed Ed25519 server signing key: its public value goes into the end-entity
    /// certificate, and it signs the CertificateVerify the connect loop authenticates.
    const SERVER_ED25519_SEED: [u8; 32] = [0x42; 32];

    /// The server's Ed25519 signing key.
    fn server_ed25519() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&SERVER_ED25519_SEED)
    }

    /// Encode a DER definite length (short form under 128, long form otherwise).
    fn der_len(len: usize, out: &mut Vec<u8>) {
        if len < 0x80 {
            out.push(len as u8);
            return;
        }
        let mut octets = len.to_be_bytes().to_vec();
        while octets.first() == Some(&0) {
            octets.remove(0);
        }
        out.push(0x80 | octets.len() as u8);
        out.extend_from_slice(&octets);
    }

    /// Build a `tag ‖ length ‖ contents` DER TLV.
    fn der_tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        der_len(contents.len(), &mut out);
        out.extend_from_slice(contents);
        out
    }

    /// The DNS host the server's certificate names and the client asks for; the
    /// connect loop matches one against the other (RFC 6125 §6).
    const SERVER_HOSTNAME: &str = "example.test";

    /// A wall-clock *now* (seconds since the Unix epoch) inside the fixture
    /// certificate's `[2020-01-01, 2030-01-01)` validity window: 2025-01-01T00:00:00Z.
    /// The connect loop judges the certificate's validity period against it
    /// (RFC 5280 §4.1.2.5), so with the fixture window it passes.
    const SERVER_NOW: i64 = 1_735_689_600;

    /// The `validity` SEQUENCE the fixture certificate carries: `notBefore`
    /// 2020-01-01T00:00:00Z, `notAfter` 2030-01-01T00:00:00Z, both `UTCTime`
    /// (RFC 5280 §4.1.2.5.1), the exact shape [`x509_validity`](super::super::x509_validity)
    /// walks. [`SERVER_NOW`] sits inside it.
    fn validity_extension() -> Vec<u8> {
        let not_before = der_tlv(0x17, b"200101000000Z"); // UTCTime notBefore
        let not_after = der_tlv(0x17, b"300101000000Z"); // UTCTime notAfter
        der_tlv(0x30, &[not_before.as_slice(), not_after.as_slice()].concat())
    }

    /// A `validity` SEQUENCE already expired at [`SERVER_NOW`] (2025-01-01): `notBefore`
    /// 2000-01-01, `notAfter` 2010-01-01, both `UTCTime` (RFC 5280 §4.1.2.5.1, `YY` 00–49
    /// → 20YY). A certificate carrying it is outside its window at `SERVER_NOW`, so the
    /// validity walk rejects it — used for the expired *intermediate* fixture, where the
    /// leaf is still in date.
    fn expired_validity_extension() -> Vec<u8> {
        let not_before = der_tlv(0x17, b"000101000000Z"); // UTCTime notBefore = 2000-01-01
        let not_after = der_tlv(0x17, b"100101000000Z"); // UTCTime notAfter = 2010-01-01
        der_tlv(0x30, &[not_before.as_slice(), not_after.as_slice()].concat())
    }

    /// A single `subjectAltName` Extension entry (OID 2.5.29.17) with one `dNSName`
    /// (`[2]`) naming `host`, exactly the shape
    /// [`x509_hostname`](super::super::x509_hostname) walks. One entry of the `extensions`
    /// field [`extensions_field`] wraps.
    fn subject_alt_name_entry(host: &str) -> Vec<u8> {
        let dns_name = der_tlv(0x82, host.as_bytes()); // dNSName [2] IA5String
        let general_names = der_tlv(0x30, &dns_name); // GeneralNames ::= SEQUENCE OF
        let extn_value = der_tlv(0x04, &general_names); // extnValue OCTET STRING
        let san_oid = der_tlv(0x06, &[0x55, 0x1D, 0x11]); // id-ce-subjectAltName
        der_tlv(0x30, &[san_oid.as_slice(), extn_value.as_slice()].concat())
    }

    /// A single `basicConstraints` Extension entry (OID 2.5.29.19) asserting `cA = TRUE`,
    /// marked critical — exactly the shape
    /// [`x509_basic_constraints`](super::super::x509_basic_constraints) decodes. An
    /// issuing (intermediate/root) certificate carries it so the CA-constraints walk
    /// admits it as a permitted issuer (RFC 5280 §4.2.1.9).
    fn basic_constraints_ca_entry() -> Vec<u8> {
        let bc_oid = der_tlv(0x06, &[0x55, 0x1D, 0x13]); // id-ce-basicConstraints
        let critical = der_tlv(0x01, &[0xFF]); // critical BOOLEAN TRUE
        let ca_seq = der_tlv(0x30, &der_tlv(0x01, &[0xFF])); // SEQUENCE { cA BOOLEAN TRUE }
        let extn_value = der_tlv(0x04, &ca_seq); // extnValue OCTET STRING
        der_tlv(0x30, &[bc_oid.as_slice(), critical.as_slice(), extn_value.as_slice()].concat())
    }

    /// A single `keyUsage` Extension entry (OID 2.5.29.15, RFC 5280 §4.2.1.3), marked
    /// critical, whose `BIT STRING` asserts `keyCertSign` (bit 5) when `key_cert_sign` and
    /// only `digitalSignature` (bit 0) otherwise — exactly the shape
    /// [`x509_key_usage`](super::super::x509_key_usage) decodes. An issuing certificate
    /// carrying the `keyCertSign` form is permitted to sign the certificate below it; the
    /// `digitalSignature`-only form is a CA whose key may *not* sign certificates, which the
    /// keyUsage walk rejects (RFC 5280 §4.2.1.3).
    fn key_usage_entry(key_cert_sign: bool) -> Vec<u8> {
        let ku_oid = der_tlv(0x06, &[0x55, 0x1D, 0x0F]); // id-ce-keyUsage
        let critical = der_tlv(0x01, &[0xFF]); // critical BOOLEAN TRUE
        // A DER BIT STRING: first content octet is the unused-bit count, then the value
        // octets, most-significant bit first, so named bit `n` is `0x80 >> n`. keyCertSign
        // is bit 5 (0x04, six bits used → two unused); digitalSignature is bit 0 (0x80, one
        // bit used → seven unused).
        let bit_string = if key_cert_sign {
            der_tlv(0x03, &[0x02, 0x04])
        } else {
            der_tlv(0x03, &[0x07, 0x80])
        };
        let extn_value = der_tlv(0x04, &bit_string); // extnValue OCTET STRING
        der_tlv(0x30, &[ku_oid.as_slice(), critical.as_slice(), extn_value.as_slice()].concat())
    }

    /// Wrap a list of Extension entries in a `TBSCertificate`'s `[3] EXPLICIT SEQUENCE OF
    /// Extension` field (RFC 5280 §4.1.2.9).
    fn extensions_field(entries: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = entries.iter().flat_map(|e| e.iter().copied()).collect();
        let sequence = der_tlv(0x30, &body); // SEQUENCE OF Extension
        der_tlv(0xA3, &sequence) // [3] EXPLICIT
    }

    /// The `extensions [3]` field a leaf certificate carries: just a `subjectAltName`
    /// naming `host`. No `basicConstraints`, so the CA-constraints walk treats it as
    /// `cA = FALSE` — fine, a leaf issues nothing.
    fn leaf_extensions(host: &str) -> Vec<u8> {
        extensions_field(&[subject_alt_name_entry(host)])
    }

    /// The `extensions [3]` field an issuing certificate carries: a `subjectAltName`
    /// naming `host` *and* a `basicConstraints` asserting `cA = TRUE`, so the
    /// CA-constraints walk admits it as a permitted issuer (RFC 5280 §4.2.1.9).
    fn ca_extensions(host: &str) -> Vec<u8> {
        extensions_field(&[subject_alt_name_entry(host), basic_constraints_ca_entry()])
    }

    /// Build the `tbsCertificate` of a minimal, structurally valid v3 certificate
    /// carrying `subject_pubkey` (an Ed25519 public key) in its `SubjectPublicKeyInfo`
    /// (RFC 5280 §4.1, RFC 8410) and a `subjectAltName` naming `host`; every field
    /// before the SPKI is a placeholder the extractor skips. Returned raw so a caller
    /// can sign these exact bytes (RFC 5280 §4.1.1.3) and [`seal_certificate`] them.
    fn ed25519_tbs(host: &str, subject_pubkey: &[u8; 32]) -> Vec<u8> {
        ed25519_tbs_ca(host, subject_pubkey, false)
    }

    /// Like [`ed25519_tbs`] but with an explicit `is_ca` flag: when `true` the certificate
    /// additionally carries a `basicConstraints` `cA = TRUE` extension (RFC 5280 §4.2.1.9),
    /// so the CA-constraints walk admits it as a permitted issuer. A leaf passes `false`;
    /// an intermediate passes `true`.
    fn ed25519_tbs_ca(host: &str, subject_pubkey: &[u8; 32], is_ca: bool) -> Vec<u8> {
        ed25519_tbs_ca_with_validity(host, subject_pubkey, is_ca, &validity_extension())
    }

    /// Like [`ed25519_tbs_ca`] but with an explicit `validity` SEQUENCE, so a fixture can
    /// place a certificate outside its `[notBefore, notAfter]` window at [`SERVER_NOW`]
    /// (RFC 5280 §4.1.2.5) — used to exercise the chain-wide validity walk on a certificate
    /// other than the leaf.
    fn ed25519_tbs_ca_with_validity(
        host: &str,
        subject_pubkey: &[u8; 32],
        is_ca: bool,
        validity: &[u8],
    ) -> Vec<u8> {
        // id-Ed25519 OID 1.3.101.112 (RFC 8410 §3).
        let ed_oid = der_tlv(0x06, &[0x2B, 0x65, 0x70]);
        let alg_id = der_tlv(0x30, &ed_oid);
        let mut bit = vec![0x00];
        bit.extend_from_slice(subject_pubkey);
        let spki = der_tlv(0x30, &[alg_id.as_slice(), der_tlv(0x03, &bit).as_slice()].concat());

        let version = der_tlv(0xA0, &der_tlv(0x02, &[0x02])); // [0] { v3 = 2 }
        let serial = der_tlv(0x02, &[0x01]);
        let sig_alg = der_tlv(0x30, &der_tlv(0x06, &[0x2B, 0x65, 0x70]));
        let empty = der_tlv(0x30, &[]);
        let extensions = if is_ca { ca_extensions(host) } else { leaf_extensions(host) };
        der_tlv(
            0x30,
            &[
                version.as_slice(),
                serial.as_slice(),
                sig_alg.as_slice(),
                empty.as_slice(), // issuer
                validity,         // validity
                empty.as_slice(), // subject
                spki.as_slice(),
                extensions.as_slice(), // [3] subjectAltName
            ]
            .concat(),
        )
    }

    /// Wrap a `tbsCertificate`, the id-Ed25519 `signatureAlgorithm`, and `signature`
    /// (the raw signature octets, without the BIT STRING's zero unused-bits prefix) into
    /// a DER `Certificate` (RFC 5280 §4.1).
    fn seal_certificate(tbs: &[u8], signature: &[u8]) -> Vec<u8> {
        let outer_sig_alg = der_tlv(0x30, &der_tlv(0x06, &[0x2B, 0x65, 0x70]));
        let mut bits = vec![0x00]; // zero unused bits
        bits.extend_from_slice(signature);
        let sig_value = der_tlv(0x03, &bits);
        der_tlv(0x30, &[tbs, outer_sig_alg.as_slice(), sig_value.as_slice()].concat())
    }

    /// A fixed Ed25519 trust-anchor signing key: the root the trust-anchor walk
    /// terminates every "completes" fixture chain at (RFC 5280 §6.1). Never appears in
    /// any presented `certificate_list` — only its `subjectPublicKeyInfo` reaches the
    /// [`trust_anchors`] store the connect driver checks against.
    const ROOT_ED25519_SEED: [u8; 32] = [0x07; 32];

    /// The trust anchor's Ed25519 signing key.
    fn root_ed25519() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&ROOT_ED25519_SEED)
    }

    /// An Ed25519 `SubjectPublicKeyInfo` DER carrying `public` (RFC 5280 §4.1.2.7),
    /// including its own outer `SEQUENCE` — the exact shape
    /// [`OwnedTrustAnchor::subject_public_key_info`] carries and
    /// [`x509_spki::parse_subject_public_key_info`](super::super::x509_spki::parse_subject_public_key_info)
    /// decodes.
    fn ed25519_spki_der(public: &[u8; 32]) -> Vec<u8> {
        let ed_oid = der_tlv(0x06, &[0x2B, 0x65, 0x70]);
        let alg_id = der_tlv(0x30, &ed_oid);
        let mut bit = vec![0x00];
        bit.extend_from_slice(public);
        der_tlv(0x30, &[alg_id.as_slice(), der_tlv(0x03, &bit).as_slice()].concat())
    }

    /// The default trust store every [`connect_driver`]-family fixture is seeded with:
    /// two anchors backed by the same [`root_ed25519`] key, one for each issuer shape
    /// this module's fixtures use for a chain's topmost certificate — the empty-SEQUENCE
    /// placeholder [`ed25519_tbs`]/[`ed25519_tbs_ca`] leave as `issuer`, and the explicit
    /// `name_rdn("Lumen Test Root")` [`ed25519_tbs_named`]/[`ed25519_tbs_named_ext`]
    /// fixtures carry. Every "completes" fixture in this module really terminates at one
    /// of these two, so [`ConnectError::TrustAnchor`] never fires for a happy path.
    fn trust_anchors() -> Vec<OwnedTrustAnchor> {
        let spki = ed25519_spki_der(&root_ed25519().verifying_key().to_bytes());
        vec![
            OwnedTrustAnchor { subject: der_tlv(0x30, &[]), subject_public_key_info: spki.clone() },
            OwnedTrustAnchor { subject: name_rdn("Lumen Test Root"), subject_public_key_info: spki },
        ]
    }

    /// A minimal end-entity certificate naming `host` and carrying the server's Ed25519
    /// public key. Its `tbsCertificate` is really signed by the [`root_ed25519`] trust
    /// anchor (RFC 5280 §4.1.1.3): in a single-certificate chain this is the *topmost*
    /// certificate, the one [`verify_trust_anchor`] checks (RFC 5280 §6.1) — unlike the
    /// internal chain-signature walk, which has no link to check here.
    fn server_certificate_der_for(host: &str) -> Vec<u8> {
        use ed25519_dalek::Signer;
        let tbs = ed25519_tbs(host, &server_ed25519().verifying_key().to_bytes());
        let signature = root_ed25519().sign(&tbs).to_bytes().to_vec();
        seal_certificate(&tbs, &signature)
    }

    /// The end-entity certificate naming [`SERVER_HOSTNAME`], used by the single-cert
    /// fixture server flights.
    fn server_certificate_der() -> Vec<u8> {
        server_certificate_der_for(SERVER_HOSTNAME)
    }

    /// A fixed Ed25519 intermediate signing key: its public value goes into the
    /// intermediate certificate, and it signs the end-entity certificate in the two-cert
    /// chain fixtures so the chain's single internal link verifies.
    const INTERMEDIATE_ED25519_SEED: [u8; 32] = [0x99; 32];

    /// The intermediate's Ed25519 signing key.
    fn intermediate_ed25519() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&INTERMEDIATE_ED25519_SEED)
    }

    /// The end-entity certificate naming [`SERVER_HOSTNAME`] (server key in its SPKI),
    /// its `tbsCertificate` really signed by `issuer` under id-Ed25519 (RFC 5280
    /// §4.1.1.3). With `issuer` the intermediate key the chain's link verifies; with any
    /// other key it does not.
    fn end_entity_signed_by(issuer: &ed25519_dalek::SigningKey) -> Vec<u8> {
        use ed25519_dalek::Signer;
        let tbs = ed25519_tbs(SERVER_HOSTNAME, &server_ed25519().verifying_key().to_bytes());
        let signature = issuer.sign(&tbs).to_bytes().to_vec();
        seal_certificate(&tbs, &signature)
    }

    /// A minimal intermediate certificate carrying the intermediate's Ed25519 public key
    /// in its `SubjectPublicKeyInfo` — the key the chain walk extracts to verify the
    /// end-entity's signature — and a `basicConstraints` `cA = TRUE` so the CA-constraints
    /// walk admits it as a permitted issuer (RFC 5280 §4.2.1.9). It is the last
    /// certificate in the fixture chain — the *topmost* one — so its `tbsCertificate` is
    /// really signed by the [`root_ed25519`] trust anchor rather than a placeholder, and
    /// [`verify_trust_anchor`] checks it (RFC 5280 §6.1).
    fn intermediate_certificate_der() -> Vec<u8> {
        use ed25519_dalek::Signer;
        let tbs =
            ed25519_tbs_ca("intermediate.test", &intermediate_ed25519().verifying_key().to_bytes(), true);
        let signature = root_ed25519().sign(&tbs).to_bytes().to_vec();
        seal_certificate(&tbs, &signature)
    }

    /// Like [`intermediate_certificate_der`] but carrying a validity window already expired
    /// at [`SERVER_NOW`] ([`expired_validity_extension`]). The leaf below it is in date, so
    /// only the *chain-wide* validity walk (RFC 5280 §6.1.3(a)(2)) — which covers every
    /// certificate, not just the leaf — rejects the chain.
    fn intermediate_certificate_der_expired() -> Vec<u8> {
        use ed25519_dalek::Signer;
        let tbs = ed25519_tbs_ca_with_validity(
            "intermediate.test",
            &intermediate_ed25519().verifying_key().to_bytes(),
            true,
            &expired_validity_extension(),
        );
        let signature = root_ed25519().sign(&tbs).to_bytes().to_vec();
        seal_certificate(&tbs, &signature)
    }

    /// A two-certificate `Certificate` message like [`certificate_bytes_chain`]`(true)` —
    /// the end-entity signed by the intermediate whose key it carries — but whose
    /// intermediate is *expired* at [`SERVER_NOW`]. The leaf is in date and the internal
    /// link verifies, so the chain fails only on the chain-wide validity walk, at index 1.
    fn certificate_bytes_chain_expired_intermediate() -> Vec<u8> {
        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![
                CertificateEntry {
                    cert_data: end_entity_signed_by(&intermediate_ed25519()),
                    extensions: Vec::new(),
                },
                CertificateEntry {
                    cert_data: intermediate_certificate_der_expired(),
                    extensions: Vec::new(),
                },
            ],
        }))
    }

    /// A DER `Name` (RDNSequence) carrying a single `commonName` attribute valued `cn` —
    /// the shape the name-chain walk ([`x509_name_chain`](super::super::x509_name_chain))
    /// compares byte-for-byte. Two Names built from the same `cn` are byte-identical;
    /// two from different `cn`s are not.
    fn name_rdn(cn: &str) -> Vec<u8> {
        let oid_cn = der_tlv(0x06, &[0x55, 0x04, 0x03]); // id-at-commonName 2.5.4.3
        let value = der_tlv(0x13, cn.as_bytes()); // PrintableString
        let atv = der_tlv(0x30, &[oid_cn.as_slice(), value.as_slice()].concat());
        let rdn = der_tlv(0x31, &atv); // RelativeDistinguishedName SET OF
        der_tlv(0x30, &rdn) // RDNSequence SEQUENCE OF
    }

    /// Like [`ed25519_tbs_ca`] but with explicit `issuer` and `subject` distinguished-name
    /// DER in place of the empty-SEQUENCE placeholders, so a fixture can exercise the
    /// name-chain walk ([`x509_name_chain`](super::super::x509_name_chain)). Every other
    /// field — the SPKI carrying `subject_pubkey`, the validity window, the
    /// `subjectAltName` naming `host`, and (when `is_ca`) the `basicConstraints`
    /// `cA = TRUE` — matches [`ed25519_tbs_ca`], so the certificate still authenticates,
    /// names the host, and is in date; only its issuer/subject Names change.
    fn ed25519_tbs_named(
        host: &str,
        subject_pubkey: &[u8; 32],
        issuer: &[u8],
        subject: &[u8],
        is_ca: bool,
    ) -> Vec<u8> {
        let ed_oid = der_tlv(0x06, &[0x2B, 0x65, 0x70]);
        let alg_id = der_tlv(0x30, &ed_oid);
        let mut bit = vec![0x00];
        bit.extend_from_slice(subject_pubkey);
        let spki = der_tlv(0x30, &[alg_id.as_slice(), der_tlv(0x03, &bit).as_slice()].concat());

        let version = der_tlv(0xA0, &der_tlv(0x02, &[0x02])); // [0] { v3 = 2 }
        let serial = der_tlv(0x02, &[0x01]);
        let sig_alg = der_tlv(0x30, &der_tlv(0x06, &[0x2B, 0x65, 0x70]));
        let validity = validity_extension();
        let extensions = if is_ca { ca_extensions(host) } else { leaf_extensions(host) };
        der_tlv(
            0x30,
            &[
                version.as_slice(),
                serial.as_slice(),
                sig_alg.as_slice(),
                issuer,              // issuer distinguished name
                validity.as_slice(), // validity
                subject,             // subject distinguished name
                spki.as_slice(),
                extensions.as_slice(), // [3] subjectAltName
            ]
            .concat(),
        )
    }

    /// A two-certificate `Certificate` message whose end-entity is really signed by the
    /// intermediate (the *signature* link always verifies) and whose issuer/subject Names
    /// optionally line up. The end-entity carries the server key and names
    /// [`SERVER_HOSTNAME`] in its `subjectAltName`, so possession, identity, and time all
    /// pass and only the *name* link is under test. With `names_match` the end-entity's
    /// `issuer` Name equals the intermediate's `subject` Name (the name walk passes);
    /// otherwise they differ (a [`NameMismatch`](super::super::x509_name_chain::NameChainWalkError::NameMismatch)
    /// the walk rejects).
    fn certificate_bytes_named_chain(names_match: bool) -> Vec<u8> {
        use ed25519_dalek::Signer;
        let intermediate_subject = name_rdn("Lumen Test Intermediate");
        let end_entity_issuer = if names_match {
            name_rdn("Lumen Test Intermediate")
        } else {
            name_rdn("Impostor Intermediate")
        };
        let end_entity_subject = name_rdn(SERVER_HOSTNAME);

        // End-entity: server key in its SPKI, its issuer/subject the Names above, its
        // tbsCertificate really signed by the intermediate so the signature link holds.
        let ee_tbs = ed25519_tbs_named(
            SERVER_HOSTNAME,
            &server_ed25519().verifying_key().to_bytes(),
            &end_entity_issuer,
            &end_entity_subject,
            false, // the end-entity leaf is not a CA
        );
        let ee_sig = intermediate_ed25519().sign(&ee_tbs).to_bytes().to_vec();
        let end_entity = seal_certificate(&ee_tbs, &ee_sig);

        // Intermediate: its own key in its SPKI (the key the signature walk extracts to
        // check the end-entity), its subject the Name the end-entity names as issuer in
        // the matching case, and basicConstraints cA = TRUE so it is a permitted issuer.
        // Last (topmost) in the list, so it is really signed by the trust-anchor root
        // key rather than a placeholder, and names the anchor's subject as issuer.
        let int_tbs = ed25519_tbs_named(
            "intermediate.test",
            &intermediate_ed25519().verifying_key().to_bytes(),
            &name_rdn("Lumen Test Root"),
            &intermediate_subject,
            true, // the intermediate issues the end-entity, so it must be a CA
        );
        let int_sig = root_ed25519().sign(&int_tbs).to_bytes().to_vec();
        let intermediate = seal_certificate(&int_tbs, &int_sig);

        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![
                CertificateEntry { cert_data: end_entity, extensions: Vec::new() },
                CertificateEntry { cert_data: intermediate, extensions: Vec::new() },
            ],
        }))
    }

    /// A two-certificate `Certificate` message whose signature and name links both verify
    /// but whose issuing (intermediate) certificate is **not** a CA — it carries no
    /// `basicConstraints` (RFC 5280 §4.2.1.9), so the CA-constraints walk rejects it as a
    /// permitted issuer. The end-entity still authenticates, names [`SERVER_HOSTNAME`], is
    /// in date, is signed by the intermediate, and names it as its issuer, so possession,
    /// identity, time, the signature walk, and the name walk all pass and only the
    /// basicConstraints check fails — isolating the CA-permission leg.
    fn certificate_bytes_non_ca_issuer() -> Vec<u8> {
        use ed25519_dalek::Signer;
        let intermediate_subject = name_rdn("Lumen Test Intermediate");

        // End-entity: really signed by the intermediate (signature link holds) and naming
        // the intermediate's subject as its issuer (name link holds).
        let ee_tbs = ed25519_tbs_named(
            SERVER_HOSTNAME,
            &server_ed25519().verifying_key().to_bytes(),
            &name_rdn("Lumen Test Intermediate"),
            &name_rdn(SERVER_HOSTNAME),
            false,
        );
        let ee_sig = intermediate_ed25519().sign(&ee_tbs).to_bytes().to_vec();
        let end_entity = seal_certificate(&ee_tbs, &ee_sig);

        // Intermediate: subject matches the end-entity's issuer and its key signed the
        // end-entity, but it is NOT a CA (is_ca = false, no basicConstraints).
        let int_tbs = ed25519_tbs_named(
            "intermediate.test",
            &intermediate_ed25519().verifying_key().to_bytes(),
            &name_rdn("Lumen Test Root"),
            &intermediate_subject,
            false, // the issuer masquerades as a CA without asserting cA = TRUE
        );
        let intermediate = seal_certificate(&int_tbs, &[0xBE, 0xEF]);

        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![
                CertificateEntry { cert_data: end_entity, extensions: Vec::new() },
                CertificateEntry { cert_data: intermediate, extensions: Vec::new() },
            ],
        }))
    }

    /// Like [`ed25519_tbs_named`] but with an explicit `extensions [3]` field, so a fixture
    /// can give an issuing certificate a `keyUsage` extension beside its `basicConstraints`.
    /// The SPKI carries `subject_pubkey`; every field before the extensions is the shared
    /// placeholder [`ed25519_tbs_named`] uses.
    fn ed25519_tbs_named_ext(
        issuer: &[u8],
        subject: &[u8],
        subject_pubkey: &[u8; 32],
        extensions: &[u8],
    ) -> Vec<u8> {
        let ed_oid = der_tlv(0x06, &[0x2B, 0x65, 0x70]);
        let alg_id = der_tlv(0x30, &ed_oid);
        let mut bit = vec![0x00];
        bit.extend_from_slice(subject_pubkey);
        let spki = der_tlv(0x30, &[alg_id.as_slice(), der_tlv(0x03, &bit).as_slice()].concat());

        let version = der_tlv(0xA0, &der_tlv(0x02, &[0x02])); // [0] { v3 = 2 }
        let serial = der_tlv(0x02, &[0x01]);
        let sig_alg = der_tlv(0x30, &der_tlv(0x06, &[0x2B, 0x65, 0x70]));
        let validity = validity_extension();
        der_tlv(
            0x30,
            &[
                version.as_slice(),
                serial.as_slice(),
                sig_alg.as_slice(),
                issuer,              // issuer distinguished name
                validity.as_slice(), // validity
                subject,             // subject distinguished name
                spki.as_slice(),
                extensions,
            ]
            .concat(),
        )
    }

    /// A two-certificate `Certificate` message whose signature, name, and basicConstraints
    /// links all verify but whose issuing (intermediate) certificate carries a `keyUsage`
    /// extension whose `keyCertSign` bit is set according to `key_cert_sign` (RFC 5280
    /// §4.2.1.3). The end-entity authenticates, names [`SERVER_HOSTNAME`], is in date, is
    /// really signed by the intermediate, names it as issuer, and the intermediate is a CA
    /// (`basicConstraints` `cA = TRUE`), so possession, identity, time, the signature walk,
    /// the name walk, and the CA-constraints walk all pass and only the *keyUsage* check is
    /// under test — it passes when `key_cert_sign` and fails ([`NotCertSign`](super::super::x509_key_usage::KeyUsageWalkError::NotCertSign)
    /// at index 1) otherwise.
    fn certificate_bytes_issuer_key_usage(key_cert_sign: bool) -> Vec<u8> {
        use ed25519_dalek::Signer;
        let intermediate_subject = name_rdn("Lumen Test Intermediate");

        // End-entity: really signed by the intermediate (signature link holds) and naming
        // the intermediate's subject as its issuer (name link holds).
        let ee_tbs = ed25519_tbs_named(
            SERVER_HOSTNAME,
            &server_ed25519().verifying_key().to_bytes(),
            &name_rdn("Lumen Test Intermediate"),
            &name_rdn(SERVER_HOSTNAME),
            false,
        );
        let ee_sig = intermediate_ed25519().sign(&ee_tbs).to_bytes().to_vec();
        let end_entity = seal_certificate(&ee_tbs, &ee_sig);

        // Intermediate: a CA (basicConstraints cA = TRUE) whose key signed the end-entity
        // and whose subject matches the end-entity's issuer, additionally carrying a
        // keyUsage extension — with keyCertSign set or clear — so only the keyUsage leg
        // decides. Last (topmost) in the list, so it is really signed by the trust-anchor
        // root key and names the anchor's subject as issuer.
        let issuer_extensions = extensions_field(&[
            subject_alt_name_entry("intermediate.test"),
            basic_constraints_ca_entry(),
            key_usage_entry(key_cert_sign),
        ]);
        let int_tbs = ed25519_tbs_named_ext(
            &name_rdn("Lumen Test Root"),
            &intermediate_subject,
            &intermediate_ed25519().verifying_key().to_bytes(),
            &issuer_extensions,
        );
        let int_sig = root_ed25519().sign(&int_tbs).to_bytes().to_vec();
        let intermediate = seal_certificate(&int_tbs, &int_sig);

        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![
                CertificateEntry { cert_data: end_entity, extensions: Vec::new() },
                CertificateEntry { cert_data: intermediate, extensions: Vec::new() },
            ],
        }))
    }

    /// A single `extendedKeyUsage` Extension entry (OID 2.5.29.37, RFC 5280 §4.2.1.12)
    /// whose `SEQUENCE OF KeyPurposeId` names `id-kp-serverAuth` (1.3.6.1.5.5.7.3.1) when
    /// `server_auth` and only `id-kp-clientAuth` (…3.2) otherwise — exactly the shape
    /// [`x509_ext_key_usage`](super::super::x509_ext_key_usage) decodes. A leaf carrying the
    /// `serverAuth` form is authorised for TLS server authentication; the `clientAuth`-only
    /// form is not, which the extendedKeyUsage walk rejects (RFC 5280 §4.2.1.12).
    fn ext_key_usage_entry(server_auth: bool) -> Vec<u8> {
        let eku_oid = der_tlv(0x06, &[0x55, 0x1D, 0x25]); // id-ce-extKeyUsage
        let purpose: &[u8] = if server_auth {
            &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01] // id-kp-serverAuth
        } else {
            &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02] // id-kp-clientAuth
        };
        let purposes = der_tlv(0x30, &der_tlv(0x06, purpose)); // SEQUENCE OF KeyPurposeId
        let extn_value = der_tlv(0x04, &purposes); // extnValue OCTET STRING
        der_tlv(0x30, &[eku_oid.as_slice(), extn_value.as_slice()].concat())
    }

    /// A single-certificate `Certificate` message whose end-entity leaf authenticates,
    /// names [`SERVER_HOSTNAME`], is in date, and — as the topmost certificate — is really
    /// signed by the [`root_ed25519`] trust anchor with the empty-`SEQUENCE` placeholder
    /// issuer the default [`trust_anchors`] store carries, but whose `extendedKeyUsage`
    /// names `serverAuth` when `server_auth` and only `clientAuth` otherwise (RFC 5280
    /// §4.2.1.12). Possession, identity, time, every (vacuous) chain walk, and the
    /// trust-anchor termination all pass, so only the *extendedKeyUsage* leg decides: the
    /// connect loop completes when `server_auth` and rejects with
    /// [`ExtKeyUsageWalkError::NotServerAuth`](super::super::x509_ext_key_usage::ExtKeyUsageWalkError::NotServerAuth)
    /// at index 0 otherwise.
    fn certificate_bytes_leaf_ext_key_usage(server_auth: bool) -> Vec<u8> {
        use ed25519_dalek::Signer;
        let empty = der_tlv(0x30, &[]);
        let leaf_ext = extensions_field(&[
            subject_alt_name_entry(SERVER_HOSTNAME),
            ext_key_usage_entry(server_auth),
        ]);
        let tbs = ed25519_tbs_named_ext(
            &empty,
            &empty,
            &server_ed25519().verifying_key().to_bytes(),
            &leaf_ext,
        );
        let signature = root_ed25519().sign(&tbs).to_bytes().to_vec();
        let leaf = seal_certificate(&tbs, &signature);
        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![CertificateEntry { cert_data: leaf, extensions: Vec::new() }],
        }))
    }

    fn certificate_bytes() -> Vec<u8> {
        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![CertificateEntry {
                cert_data: server_certificate_der(),
                extensions: Vec::new(),
            }],
        }))
    }

    /// The server CertificateVerify, signing the `Transcript-Hash(ClientHello…
    /// Certificate)` of `transcript_ch_cert` with the server's Ed25519 key exactly as
    /// a real server does (RFC 8446 §4.4.3), so the connect loop authenticates it.
    fn certificate_verify_bytes(transcript_ch_cert: &[u8]) -> Vec<u8> {
        use ed25519_dalek::Signer;
        let th = tls_schedule::transcript_hash(transcript_ch_cert);
        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let signature = server_ed25519().sign(&content).to_bytes().to_vec();
        enc(&Handshake::CertificateVerify(tls_message::CertificateVerify {
            algorithm: signature_scheme::ED25519,
            signature,
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
    /// Finished) as one concatenated CRYPTO payload. The CertificateVerify signs the
    /// `ClientHello…Certificate` transcript and the Finished MACs the
    /// `ClientHello…CertificateVerify` transcript, exactly as a real server does, so
    /// the flight both authenticates (RFC 8446 §4.4.3) and completes (§4.4.4).
    fn handshake_flight(ch: &[u8], sh: &[u8]) -> Vec<u8> {
        handshake_flight_with_cert(ch, sh, &certificate_bytes())
    }

    /// [`handshake_flight`] over an explicit Certificate handshake message `cert`, so a
    /// test can drive the loop with a multi-certificate `certificate_list`. The
    /// CertificateVerify signs the `ClientHello…Certificate` transcript *including*
    /// `cert`, and the Finished MACs through it, so the flight stays self-consistent
    /// whatever chain `cert` carries.
    fn handshake_flight_with_cert(ch: &[u8], sh: &[u8], cert: &[u8]) -> Vec<u8> {
        let ee = encrypted_extensions_bytes();
        // The CertificateVerify signs Transcript-Hash(ClientHello…Certificate).
        let mut transcript = ch.to_vec();
        for m in [sh, ee.as_slice(), cert] {
            transcript.extend_from_slice(m);
        }
        let cv = certificate_verify_bytes(&transcript);
        // The server Finished MACs Transcript-Hash(ClientHello…CertificateVerify).
        transcript.extend_from_slice(&cv);
        let hs_traffic = handshake_traffic(ch, sh);
        let sf = server_finished_bytes(&hs_traffic.server, &transcript);
        let mut flight = Vec::new();
        for m in [ee.as_slice(), cert, cv.as_slice(), sf.as_slice()] {
            flight.extend_from_slice(m);
        }
        flight
    }

    /// A two-certificate `Certificate` message: the end-entity (naming
    /// [`SERVER_HOSTNAME`], carrying the server key) followed by the intermediate whose
    /// key signed it. `linked` decides the internal link: signed by the intermediate
    /// (verifies) or by the server's own key (a broken link the walk rejects).
    fn certificate_bytes_chain(linked: bool) -> Vec<u8> {
        let issuer = if linked { intermediate_ed25519() } else { server_ed25519() };
        enc(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![
                CertificateEntry {
                    cert_data: end_entity_signed_by(&issuer),
                    extensions: Vec::new(),
                },
                CertificateEntry {
                    cert_data: intermediate_certificate_der(),
                    extensions: Vec::new(),
                },
            ],
        }))
    }

    /// A server flight whose TLS handshake completes (the Finished MAC verifies) but
    /// whose CertificateVerify does *not* authenticate: it is a well-formed Ed25519
    /// signature over the wrong transcript, so the peer has not proven possession of
    /// the certificate's key over this handshake (RFC 8446 §4.4.3). The server
    /// Finished is MAC'd over the transcript that includes this bad CertificateVerify,
    /// so TLS completes and the failure surfaces only at certificate authentication.
    fn unauthenticated_flight(ch: &[u8], sh: &[u8]) -> Vec<u8> {
        let ee = encrypted_extensions_bytes();
        let cert = certificate_bytes();
        // Signed over unrelated bytes, not ClientHello…Certificate.
        let cv = certificate_verify_bytes(b"a transcript this handshake never had");
        let mut transcript = ch.to_vec();
        for m in [sh, &ee, &cert, &cv] {
            transcript.extend_from_slice(m);
        }
        let hs_traffic = handshake_traffic(ch, sh);
        let sf = server_finished_bytes(&hs_traffic.server, &transcript);
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

    // ---- poll: an unauthenticated certificate aborts the connect -------

    #[test]
    fn poll_rejects_a_certificate_that_does_not_authenticate() {
        use crate::h3::conn_cert_auth::CertAuthError;
        use crate::h3::tls_cert_verify::CertVerifyError;

        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The TLS handshake completes, but the CertificateVerify signature is over the
        // wrong transcript: the peer has not proven possession of the certificate.
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, unauthenticated_flight(&ch, &sh), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the certificate must not authenticate");
        assert!(matches!(
            err,
            ConnectError::CertAuth(CertAuthError::Verify(CertVerifyError::BadSignature)),
        ));
        // The completion is never retained for an unauthenticated peer, and the client
        // Finished — enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a certificate that does not name the host aborts the connect ----

    #[test]
    fn poll_rejects_a_certificate_that_does_not_name_the_host() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The fixture certificate names SERVER_HOSTNAME and authenticates cleanly (the
        // CertificateVerify is valid); only the requested host differs, so possession
        // passes and the failure is purely the hostname (identity) check.
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        let mut cd = connect_driver_for(t, now, "wrong.example");

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the certificate must not cover the host");
        assert!(matches!(err, ConnectError::Hostname(HostnameError::NoMatch)));
        // The completion is never retained for a mis-named certificate, and the client
        // Finished — enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a matching hostname lets the handshake complete ---------

    #[test]
    fn poll_completes_when_the_certificate_names_the_host() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        // The default driver asks for SERVER_HOSTNAME, exactly what the certificate
        // names, so the identity check passes and the handshake completes.
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let step = cd.poll(now).expect("poll over the flight");
        assert!(step.completed);
        assert!(cd.completed().is_some());
        // The client Finished was flushed: the Handshake CRYPTO queue drained.
        assert!(!cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: an expired certificate aborts the connect ---------------

    #[test]
    fn poll_rejects_an_expired_certificate() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The fixture certificate authenticates and names SERVER_HOSTNAME cleanly; only
        // the wall-clock now is past its notAfter (2030-01-01), so possession and
        // identity pass and the failure is purely the validity (time) check.
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        // 2035-01-01T00:00:00Z, past the fixture certificate's notAfter.
        let after_expiry = 2_051_222_400;
        let mut cd = connect_driver_for_at(t, now, SERVER_HOSTNAME, after_expiry);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the certificate must be expired");
        assert!(matches!(
            err,
            ConnectError::Validity(ValidityWalkError::Certificate {
                index: 0,
                error: ValidityError::Expired { .. }
            })
        ));
        // The completion is never retained for an expired certificate, and the client
        // Finished — enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a not-yet-valid certificate aborts the connect ----------

    #[test]
    fn poll_rejects_a_not_yet_valid_certificate() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        // 2015-01-01T00:00:00Z, before the fixture certificate's notBefore (2020-01-01).
        let before_validity = 1_420_070_400;
        let mut cd = connect_driver_for_at(t, now, SERVER_HOSTNAME, before_validity);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the certificate must not yet be valid");
        assert!(matches!(
            err,
            ConnectError::Validity(ValidityWalkError::Certificate {
                index: 0,
                error: ValidityError::NotYetValid { .. }
            })
        ));
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: an expired *intermediate* aborts the connect ------------

    #[test]
    fn poll_rejects_a_chain_with_an_expired_intermediate() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The leaf authenticates, names SERVER_HOSTNAME, and is in date at SERVER_NOW; only
        // the intermediate above it is expired (notAfter 2010-01-01). A leaf-only validity
        // check would let this chain through — the chain-wide walk (RFC 5280 §6.1.3(a)(2))
        // catches the stale issuer and abandons before the client Finished.
        let cert = certificate_bytes_chain_expired_intermediate();
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now); // now_unix = SERVER_NOW, inside the leaf window

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the intermediate must be expired");
        // The walk names index 1 — the intermediate — not the in-date leaf at index 0.
        assert!(matches!(
            err,
            ConnectError::Validity(ValidityWalkError::Certificate {
                index: 1,
                error: ValidityError::Expired { .. }
            })
        ));
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a self-consistent chain lets the handshake complete ------

    #[test]
    fn poll_completes_when_the_certificate_chain_verifies() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // A two-cert chain whose end-entity is really signed by the intermediate: the
        // end-entity still authenticates, names SERVER_HOSTNAME, and is in date, and the
        // chain's single internal link verifies, so the handshake completes.
        let cert = certificate_bytes_chain(true);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let step = cd.poll(now).expect("poll over the flight");
        assert!(step.completed);
        let complete = cd.completed().expect("completion retained");
        assert_eq!(complete.server_certificate.certificate_list.len(), 2, "end-entity + issuer");
        // The client Finished was flushed: the Handshake CRYPTO queue drained.
        assert!(!cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a broken chain link aborts the connect ------------------

    #[test]
    fn poll_rejects_a_certificate_chain_with_a_broken_link() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The end-entity authenticates, names SERVER_HOSTNAME, and is in date, but its
        // tbsCertificate is signed by the server's own key, not the intermediate whose
        // key heads the chain — so possession, identity, and time pass and the failure
        // is purely the chain-signature (link) check.
        let cert = certificate_bytes_chain(false);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the chain link must not verify");
        assert!(matches!(
            err,
            ConnectError::Chain(ChainWalkError::Link { subject_index: 0, .. }),
        ));
        // The completion is never retained for a broken chain, and the client Finished —
        // enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a self-consistent name chain lets the handshake complete ----

    #[test]
    fn poll_completes_when_the_certificate_name_chain_verifies() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // A two-cert chain whose signature link verifies AND whose issuer/subject Names
        // line up: the end-entity authenticates, names SERVER_HOSTNAME, is in date, is
        // signed by the intermediate, and names the intermediate as its issuer — so every
        // check passes and the handshake completes.
        let cert = certificate_bytes_named_chain(true);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let step = cd.poll(now).expect("poll over the flight");
        assert!(step.completed);
        let complete = cd.completed().expect("completion retained");
        assert_eq!(complete.server_certificate.certificate_list.len(), 2, "end-entity + issuer");
        // The client Finished was flushed: the Handshake CRYPTO queue drained.
        assert!(!cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a broken name link aborts the connect -------------------

    #[test]
    fn poll_rejects_a_certificate_chain_with_a_mismatched_name() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The end-entity authenticates, names SERVER_HOSTNAME, is in date, and is really
        // signed by the intermediate — but it names a *different* issuer than the
        // intermediate's subject, so possession, identity, time, and the signature walk
        // all pass and the failure is purely the name-chaining check.
        let cert = certificate_bytes_named_chain(false);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the name link must not match");
        assert!(matches!(
            err,
            ConnectError::NameChain(NameChainWalkError::NameMismatch { subject_index: 0 }),
        ));
        // The completion is never retained for a mis-named chain, and the client Finished —
        // enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a permitted CA issuer lets the handshake complete -------

    #[test]
    fn poll_completes_when_the_issuer_is_a_ca() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // A two-cert chain whose signature and name links both verify AND whose issuing
        // intermediate asserts basicConstraints cA = TRUE: possession, identity, time, the
        // signature walk, the name walk, and the CA-constraints walk all pass, so the
        // handshake completes.
        let cert = certificate_bytes_named_chain(true);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let step = cd.poll(now).expect("poll over the flight");
        assert!(step.completed);
        let complete = cd.completed().expect("completion retained");
        assert_eq!(complete.server_certificate.certificate_list.len(), 2, "end-entity + CA issuer");
        // The client Finished was flushed: the Handshake CRYPTO queue drained.
        assert!(!cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a non-CA issuer aborts the connect ----------------------

    #[test]
    fn poll_rejects_a_chain_whose_issuer_is_not_a_ca() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The end-entity authenticates, names SERVER_HOSTNAME, is in date, is really signed
        // by the intermediate, and names it as issuer — but the intermediate carries no
        // basicConstraints, so possession, identity, time, the signature walk, and the name
        // walk all pass and the failure is purely the CA-constraints (basicConstraints) check.
        let cert = certificate_bytes_non_ca_issuer();
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the issuer must not be a permitted CA");
        assert!(matches!(
            err,
            ConnectError::CaConstraints(CaConstraintsWalkError::NotaCa { index: 1 }),
        ));
        // The completion is never retained for a non-CA issuer, and the client Finished —
        // enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: an issuer whose keyUsage permits cert signing completes -

    #[test]
    fn poll_completes_when_the_issuer_key_usage_allows_cert_sign() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // A two-cert chain whose signature, name, and basicConstraints links all verify AND
        // whose issuing intermediate carries a keyUsage extension asserting keyCertSign:
        // possession, identity, time, the signature walk, the name walk, the CA-constraints
        // walk, and the keyUsage walk all pass, so the handshake completes.
        let cert = certificate_bytes_issuer_key_usage(true);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let step = cd.poll(now).expect("poll over the flight");
        assert!(step.completed);
        let complete = cd.completed().expect("completion retained");
        assert_eq!(complete.server_certificate.certificate_list.len(), 2, "end-entity + CA issuer");
        // The client Finished was flushed: the Handshake CRYPTO queue drained.
        assert!(!cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: an issuer whose keyUsage forbids cert signing aborts -----

    #[test]
    fn poll_rejects_a_chain_whose_issuer_key_usage_forbids_cert_sign() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The end-entity authenticates, names SERVER_HOSTNAME, is in date, is really signed
        // by the intermediate, and names it as issuer, and the intermediate is a CA — but the
        // intermediate's keyUsage omits keyCertSign, so possession, identity, time, the
        // signature walk, the name walk, and the CA-constraints walk all pass and the failure
        // is purely the keyUsage (keyCertSign) check.
        let cert = certificate_bytes_issuer_key_usage(false);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the issuer key may not sign certificates");
        assert!(matches!(
            err,
            ConnectError::KeyUsage(KeyUsageWalkError::NotCertSign { index: 1 }),
        ));
        // The completion is never retained for a non-signing issuer, and the client Finished —
        // enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: an untrusted issuer aborts the connect ------------------

    #[test]
    fn poll_rejects_a_chain_whose_issuer_names_no_trusted_anchor() {
        use crate::h3::x509_trust_anchor::TrustAnchorError;
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The default single-cert fixture authenticates, names SERVER_HOSTNAME, is in
        // date, and (vacuously) has no internal chain link to break — but the driver's
        // trust store is empty, so the topmost (only) certificate's issuer names no
        // anchor at all: possession, identity, time, and every chain walk pass and the
        // failure is purely the trust-anchor termination check.
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        let mut cd = connect_driver_with_anchors(t, now, SERVER_HOSTNAME, SERVER_NOW, Vec::new());

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("an empty trust store must not terminate the chain");
        assert!(matches!(
            err,
            ConnectError::TrustAnchor(TrustAnchorWalkError::Untrusted {
                index: 0,
                error: TrustAnchorError::UnknownIssuer,
            }),
        ));
        // The completion is never retained for an untrusted chain, and the client Finished —
        // enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    #[test]
    fn poll_rejects_a_chain_not_really_signed_by_the_named_anchor() {
        use crate::h3::x509_chain::ChainError;
        use crate::h3::x509_trust_anchor::TrustAnchorError;
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The default single-cert fixture authenticates, names SERVER_HOSTNAME, is in
        // date, and has no internal chain link to break — and the trust store carries an
        // anchor whose subject matches its issuer, but under an impostor key that never
        // signed it: possession, identity, time, and every chain walk pass and the
        // failure is purely the trust-anchor signature check.
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight(&ch, &sh), &ch, &sh));
        let impostor_spki =
            ed25519_spki_der(&intermediate_ed25519().verifying_key().to_bytes());
        let anchors = vec![OwnedTrustAnchor {
            subject: der_tlv(0x30, &[]),
            subject_public_key_info: impostor_spki,
        }];
        let mut cd = connect_driver_with_anchors(t, now, SERVER_HOSTNAME, SERVER_NOW, anchors);

        cd.poll(now).expect("poll over the ServerHello");
        let err = cd.poll(now).expect_err("the impostor key must not verify the topmost signature");
        assert!(matches!(
            err,
            ConnectError::TrustAnchor(TrustAnchorWalkError::Untrusted {
                index: 0,
                error: TrustAnchorError::Signature(ChainError::BadSignature),
            }),
        ));
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a leaf authorised for serverAuth completes --------------

    #[test]
    fn poll_completes_when_the_leaf_extended_key_usage_allows_server_auth() {
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // A single-cert leaf whose extendedKeyUsage names serverAuth: possession, identity,
        // time, every (vacuous) chain walk, the trust-anchor termination, and the
        // extendedKeyUsage walk all pass, so the handshake completes.
        let cert = certificate_bytes_leaf_ext_key_usage(true);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let step = cd.poll(now).expect("poll over the flight");
        assert!(step.completed);
        assert!(cd.completed().is_some());
        // The client Finished was flushed: the Handshake CRYPTO queue drained.
        assert!(!cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
    }

    // ---- poll: a leaf authorised only for clientAuth aborts the connect -

    #[test]
    fn poll_rejects_a_leaf_whose_extended_key_usage_forbids_server_auth() {
        use crate::h3::x509_ext_key_usage::ExtKeyUsageWalkError;
        let now = base();
        let ch = client_hello_bytes();
        let sh = server_hello_bytes();
        let mut t = transport();
        // The leaf authenticates, names SERVER_HOSTNAME, is in date, and terminates at a
        // trusted anchor — but its extendedKeyUsage names only clientAuth, so possession,
        // identity, time, every chain walk, and the trust-anchor termination all pass and
        // the failure is purely the extendedKeyUsage (serverAuth) check.
        let cert = certificate_bytes_leaf_ext_key_usage(false);
        t.push_inbound(server_initial(0, sh.clone()));
        t.push_inbound(server_handshake(0, handshake_flight_with_cert(&ch, &sh, &cert), &ch, &sh));
        let mut cd = connect_driver(t, now);

        cd.poll(now).expect("poll over the ServerHello");
        let err =
            cd.poll(now).expect_err("a clientAuth-only leaf may not authenticate a TLS server");
        assert!(matches!(
            err,
            ConnectError::ExtKeyUsage(ExtKeyUsageWalkError::NotServerAuth { index: 0 }),
        ));
        // The completion is never retained for an unauthorised leaf, and the client
        // Finished — enqueued by the TLS advance — was not flushed.
        assert!(cd.completed().is_none());
        assert!(cd.handshake().turn().send().pending_in(PacketNumberSpace::Handshake));
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
