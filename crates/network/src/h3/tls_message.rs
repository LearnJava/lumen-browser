//! TLS 1.3 handshake message codec (RFC 8446 §4).
//!
//! QUIC carries the TLS 1.3 handshake in CRYPTO frames (RFC 9001 §4): the same
//! `Handshake` messages a TLS-over-TCP endpoint would exchange, minus the TLS
//! record layer. This module is a pure parse/serialize layer over those
//! messages — no IO, no connection state, no crypto. It produces the exact
//! wire bytes that feed two consumers:
//!
//! - the **transcript hash** ([`super::tls_schedule::transcript_hash`]): every
//!   handshake message, in the order exchanged, is concatenated and hashed to
//!   key the `Derive-Secret` steps of slice 15's key schedule. Byte-exact
//!   round-tripping is therefore mandatory — a single differing byte changes
//!   every derived secret.
//! - the **`(EC)DHE` shared secret**: the `key_share` extension carries each
//!   peer's ephemeral public key. [`KeyShareEntry`] extracts it (an X25519
//!   public key when the group is [`GROUP_X25519`]) for the key-agreement slice
//!   that feeds [`super::tls_schedule::handshake_secret`].
//!
//! ## Scope
//!
//! - The [`Handshake`] wrapper (`msg_type` · `uint24 length` · body,
//!   RFC 8446 §4) and every message type a QUIC client sends or receives:
//!   ClientHello, ServerHello (incl. HelloRetryRequest), EncryptedExtensions,
//!   CertificateRequest, Certificate, CertificateVerify, Finished,
//!   NewSessionTicket, KeyUpdate, EndOfEarlyData.
//! - Extensions are carried generically as [`Extension`] (`type` · opaque
//!   `data`); this is byte-exact for the transcript regardless of which
//!   extensions a peer sends. [`KeyShareEntry`] and [`supported_versions`]
//!   provide typed codecs for the two extension bodies the QUIC/TLS bridge
//!   needs; the rest stay opaque.
//! - Unknown handshake types are preserved verbatim as [`Handshake::Unknown`]
//!   so the transcript layer can still hash them rather than the codec silently
//!   dropping bytes.
//!
//! ## Out of scope (later slices)
//!
//! - The X25519 key agreement that turns two [`KeyShareEntry`] public keys into
//!   the `(EC)DHE` secret, and computing / verifying the `Finished` MAC and the
//!   `CertificateVerify` signature (those need the traffic secrets and the
//!   certificate chain).
//! - CRYPTO-frame reassembly and the handshake state machine that decides which
//!   message is legal next — this codec parses one message off a buffer; the
//!   ordering rules live above it.
//! - Certificate-chain / X.509 validation (delegated to `rustls`/`webpki`).

use std::fmt;

// ── HandshakeType (RFC 8446 §4, IANA TLS HandshakeType registry) ─────────────

/// `client_hello` (RFC 8446 §4.1.2).
pub const HS_CLIENT_HELLO: u8 = 1;
/// `server_hello` (RFC 8446 §4.1.3) — also carries HelloRetryRequest.
pub const HS_SERVER_HELLO: u8 = 2;
/// `new_session_ticket` (RFC 8446 §4.6.1).
pub const HS_NEW_SESSION_TICKET: u8 = 4;
/// `end_of_early_data` (RFC 8446 §4.5).
pub const HS_END_OF_EARLY_DATA: u8 = 5;
/// `encrypted_extensions` (RFC 8446 §4.3.1).
pub const HS_ENCRYPTED_EXTENSIONS: u8 = 8;
/// `certificate` (RFC 8446 §4.4.2).
pub const HS_CERTIFICATE: u8 = 11;
/// `certificate_request` (RFC 8446 §4.3.2).
pub const HS_CERTIFICATE_REQUEST: u8 = 13;
/// `certificate_verify` (RFC 8446 §4.4.3).
pub const HS_CERTIFICATE_VERIFY: u8 = 15;
/// `finished` (RFC 8446 §4.4.4).
pub const HS_FINISHED: u8 = 20;
/// `key_update` (RFC 8446 §4.6.3).
pub const HS_KEY_UPDATE: u8 = 24;

// ── Well-known constants used to build a QUIC ClientHello ────────────────────

/// TLS 1.2 wire version `0x0303`, sent as `legacy_version` in both ClientHello
/// and ServerHello; the real TLS 1.3 version travels in `supported_versions`
/// (RFC 8446 §4.1.2, §4.2.1).
pub const LEGACY_VERSION_TLS12: u16 = 0x0303;
/// TLS 1.3 version code `0x0304`, advertised inside the `supported_versions`
/// extension (RFC 8446 §4.2.1).
pub const VERSION_TLS13: u16 = 0x0304;

/// `TLS_AES_128_GCM_SHA256` (RFC 8446 §B.4) — the mandatory-to-implement suite
/// and the one QUIC's Initial keys use (RFC 9001 §5.2).
pub const TLS_AES_128_GCM_SHA256: u16 = 0x1301;
/// `TLS_AES_256_GCM_SHA384` (RFC 8446 §B.4).
pub const TLS_AES_256_GCM_SHA384: u16 = 0x1302;
/// `TLS_CHACHA20_POLY1305_SHA256` (RFC 8446 §B.4).
pub const TLS_CHACHA20_POLY1305_SHA256: u16 = 0x1303;

/// `key_share` extension type (RFC 8446 §4.2.8).
pub const EXT_KEY_SHARE: u16 = 51;
/// `supported_versions` extension type (RFC 8446 §4.2.1).
pub const EXT_SUPPORTED_VERSIONS: u16 = 43;
/// `supported_groups` extension type (RFC 8446 §4.2.7).
pub const EXT_SUPPORTED_GROUPS: u16 = 10;
/// `signature_algorithms` extension type (RFC 8446 §4.2.3).
pub const EXT_SIGNATURE_ALGORITHMS: u16 = 13;
/// `server_name` (SNI) extension type (RFC 6066 §3).
pub const EXT_SERVER_NAME: u16 = 0;
/// `application_layer_protocol_negotiation` (ALPN) extension type (RFC 7301).
pub const EXT_ALPN: u16 = 16;
/// `quic_transport_parameters` extension type (RFC 9001 §8.2), the extension
/// QUIC uses to carry its transport parameters inside the TLS handshake.
pub const EXT_QUIC_TRANSPORT_PARAMETERS: u16 = 57;

/// `x25519` named group (RFC 8446 §4.2.7, RFC 7748) — the default QUIC key
/// exchange, a 32-byte public key.
pub const GROUP_X25519: u16 = 0x001d;
/// `secp256r1` (NIST P-256) named group (RFC 8446 §4.2.7).
pub const GROUP_SECP256R1: u16 = 0x0017;

/// The special `ServerHello.random` value that marks a HelloRetryRequest:
/// `SHA-256("HelloRetryRequest")` (RFC 8446 §4.1.3).
pub const HELLO_RETRY_REQUEST_RANDOM: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91,
    0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

// ── Errors ───────────────────────────────────────────────────────────────────

/// Codec error. [`Handshake::parse`] signals "need more bytes" out of band with
/// `Ok(None)`; every variant here is a genuine fault the caller cannot fix by
/// reading more data (a TLS `decode_error` / `illegal_parameter` alert).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsError {
    /// The message body is structurally invalid: a length prefix runs past the
    /// declared body, a fixed field has the wrong size, or trailing bytes
    /// remain after a fully-parsed message (TLS `decode_error`).
    Malformed(String),
    /// A field is too large to serialize into its length prefix (e.g. an
    /// extension body ≥ 2^16 bytes, a certificate ≥ 2^24 bytes). Only
    /// [`Handshake::encode`] and the `KeyShareEntry`/`supported_versions`
    /// encoders can raise this.
    Overflow(String),
}

impl TlsError {
    /// Construct a [`TlsError::Malformed`].
    fn malformed(msg: impl Into<String>) -> Self {
        Self::Malformed(msg.into())
    }
}

impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(m) => write!(f, "malformed TLS handshake message: {m}"),
            Self::Overflow(m) => write!(f, "TLS handshake field too large: {m}"),
        }
    }
}

impl std::error::Error for TlsError {}

// ── Extension (RFC 8446 §4.2) ────────────────────────────────────────────────

/// A single TLS extension: a 2-byte type and an opaque `<0..2^16-1>` body
/// (RFC 8446 §4.2). Kept generic so the transcript stays byte-exact regardless
/// of which extensions a peer sends; typed bodies (key share, supported
/// versions) are parsed on demand via [`KeyShareEntry`] / [`supported_versions`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extension {
    /// The `ExtensionType` code (e.g. [`EXT_KEY_SHARE`]).
    pub extension_type: u16,
    /// The raw extension body, exactly as it appears on the wire.
    pub data: Vec<u8>,
}

impl Extension {
    /// Construct an extension from its type code and raw body.
    #[must_use]
    pub fn new(extension_type: u16, data: Vec<u8>) -> Self {
        Self { extension_type, data }
    }
}

// ── Message bodies (RFC 8446 §4) ─────────────────────────────────────────────

/// ClientHello (RFC 8446 §4.1.2). `legacy_version` is always
/// [`LEGACY_VERSION_TLS12`] and `legacy_compression_methods` is always the
/// single null byte required of every TLS 1.3 ClientHello, so neither is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientHello {
    /// 32 bytes of client randomness.
    pub random: [u8; 32],
    /// `legacy_session_id` (`<0..32>`) — non-empty only for TLS 1.2 middlebox
    /// compatibility (RFC 8446 §4.1.2).
    pub legacy_session_id: Vec<u8>,
    /// Offered cipher suites, most-preferred first (e.g.
    /// [`TLS_AES_128_GCM_SHA256`]).
    pub cipher_suites: Vec<u16>,
    /// Extensions, in wire order (order is significant for the transcript).
    pub extensions: Vec<Extension>,
}

/// ServerHello (RFC 8446 §4.1.3). A HelloRetryRequest is a ServerHello whose
/// `random` equals [`HELLO_RETRY_REQUEST_RANDOM`]; use
/// [`ServerHello::is_hello_retry_request`] to tell them apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerHello {
    /// 32 bytes of server randomness (or [`HELLO_RETRY_REQUEST_RANDOM`]).
    pub random: [u8; 32],
    /// `legacy_session_id_echo` — echoes the ClientHello's session id.
    pub legacy_session_id_echo: Vec<u8>,
    /// The single cipher suite the server selected.
    pub cipher_suite: u16,
    /// Extensions, in wire order.
    pub extensions: Vec<Extension>,
}

impl ServerHello {
    /// Whether this ServerHello is actually a HelloRetryRequest, identified by
    /// its magic `random` value (RFC 8446 §4.1.3).
    #[must_use]
    pub fn is_hello_retry_request(&self) -> bool {
        self.random == HELLO_RETRY_REQUEST_RANDOM
    }
}

/// A single entry of a Certificate message's `certificate_list`
/// (RFC 8446 §4.4.2): the DER-encoded certificate (or raw public key) plus its
/// per-certificate extensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertificateEntry {
    /// `cert_data` (`<1..2^24-1>`) — the DER certificate or raw SPKI.
    pub cert_data: Vec<u8>,
    /// Per-certificate extensions (e.g. OCSP status, SCT).
    pub extensions: Vec<Extension>,
}

/// Certificate message (RFC 8446 §4.4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    /// `certificate_request_context` (`<0..2^8-1>`) — empty for a server
    /// certificate in the main handshake.
    pub certificate_request_context: Vec<u8>,
    /// The chain, end-entity certificate first (RFC 8446 §4.4.2).
    pub certificate_list: Vec<CertificateEntry>,
}

/// CertificateRequest message (RFC 8446 §4.3.2), sent by a server that wants
/// client authentication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertificateRequest {
    /// `certificate_request_context` (`<0..2^8-1>`) — echoed by the client's
    /// Certificate.
    pub certificate_request_context: Vec<u8>,
    /// Extensions describing acceptable certificates (e.g.
    /// `signature_algorithms`, `certificate_authorities`).
    pub extensions: Vec<Extension>,
}

/// CertificateVerify message (RFC 8446 §4.4.3): a signature over the handshake
/// transcript with the algorithm the signer chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertificateVerify {
    /// The `SignatureScheme` used (e.g. `0x0804` = rsa_pss_rsae_sha256).
    pub algorithm: u16,
    /// The signature (`<0..2^16-1>`).
    pub signature: Vec<u8>,
}

/// NewSessionTicket message (RFC 8446 §4.6.1), a post-handshake message that
/// offers a PSK for resumption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSessionTicket {
    /// Lifetime hint in seconds (`<= 604800`, i.e. 7 days).
    pub ticket_lifetime: u32,
    /// Random value added to the obfuscated ticket age.
    pub ticket_age_add: u32,
    /// `ticket_nonce` (`<0..255>`) — per-ticket nonce that derives the PSK.
    pub ticket_nonce: Vec<u8>,
    /// The opaque ticket (`<1..2^16-1>`).
    pub ticket: Vec<u8>,
    /// Ticket extensions (e.g. `early_data`).
    pub extensions: Vec<Extension>,
}

/// KeyUpdate `request_update` value (RFC 8446 §4.6.3): whether the peer is asked
/// to update its own sending keys in response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUpdateRequest {
    /// `update_not_requested (0)` — the sender updated; the peer need not.
    NotRequested,
    /// `update_requested (1)` — the peer should update and reply in kind.
    Requested,
}

// ── Handshake wrapper (RFC 8446 §4) ──────────────────────────────────────────

/// One TLS 1.3 handshake message, i.e. the body of the `Handshake` wrapper
/// (`msg_type` · `uint24 length` · body, RFC 8446 §4). [`Handshake::parse`]
/// pulls one off a byte buffer; [`Handshake::encode`] appends its wire form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handshake {
    /// ClientHello (RFC 8446 §4.1.2).
    ClientHello(ClientHello),
    /// ServerHello / HelloRetryRequest (RFC 8446 §4.1.3).
    ServerHello(ServerHello),
    /// EncryptedExtensions (RFC 8446 §4.3.1) — the server's non-certificate
    /// extensions, sent encrypted; body is the extension list.
    EncryptedExtensions(Vec<Extension>),
    /// CertificateRequest (RFC 8446 §4.3.2).
    CertificateRequest(CertificateRequest),
    /// Certificate (RFC 8446 §4.4.2).
    Certificate(Certificate),
    /// CertificateVerify (RFC 8446 §4.4.3).
    CertificateVerify(CertificateVerify),
    /// Finished (RFC 8446 §4.4.4) — the `verify_data` MAC over the transcript;
    /// its length is the negotiated hash length, so the body is opaque here.
    Finished(Vec<u8>),
    /// NewSessionTicket (RFC 8446 §4.6.1).
    NewSessionTicket(NewSessionTicket),
    /// EndOfEarlyData (RFC 8446 §4.5) — an empty body.
    EndOfEarlyData,
    /// KeyUpdate (RFC 8446 §4.6.3).
    KeyUpdate(KeyUpdateRequest),
    /// An unrecognised handshake type, preserved verbatim so the transcript
    /// hash still covers it (RFC 8446 §4 leaves the registry open).
    Unknown {
        /// The raw `msg_type` code.
        msg_type: u8,
        /// The unparsed message body.
        body: Vec<u8>,
    },
}

impl Handshake {
    /// The `msg_type` code this message serializes to (RFC 8446 §4).
    #[must_use]
    pub const fn msg_type(&self) -> u8 {
        match self {
            Self::ClientHello(_) => HS_CLIENT_HELLO,
            Self::ServerHello(_) => HS_SERVER_HELLO,
            Self::EncryptedExtensions(_) => HS_ENCRYPTED_EXTENSIONS,
            Self::CertificateRequest(_) => HS_CERTIFICATE_REQUEST,
            Self::Certificate(_) => HS_CERTIFICATE,
            Self::CertificateVerify(_) => HS_CERTIFICATE_VERIFY,
            Self::Finished(_) => HS_FINISHED,
            Self::NewSessionTicket(_) => HS_NEW_SESSION_TICKET,
            Self::EndOfEarlyData => HS_END_OF_EARLY_DATA,
            Self::KeyUpdate(_) => HS_KEY_UPDATE,
            Self::Unknown { msg_type, .. } => *msg_type,
        }
    }

    /// Parse one handshake message from the front of `buf`.
    ///
    /// Returns `Ok(None)` while `buf` does not yet hold the full message (the
    /// 4-byte header plus its declared body — the caller should read more from
    /// the CRYPTO stream and retry), `Ok(Some((message, consumed)))` on a
    /// complete message, and `Err` when the body is malformed.
    ///
    /// # Errors
    ///
    /// [`TlsError::Malformed`] when a length prefix overruns the declared body,
    /// a fixed field is the wrong size, or bytes remain after the message.
    pub fn parse(buf: &[u8]) -> Result<Option<(Self, usize)>, TlsError> {
        if buf.len() < 4 {
            return Ok(None);
        }
        let msg_type = buf[0];
        // uint24 length, big-endian.
        let body_len = u32::from_be_bytes([0, buf[1], buf[2], buf[3]]) as usize;
        let total = 4 + body_len;
        if buf.len() < total {
            return Ok(None);
        }
        let body = &buf[4..total];
        let message = Self::parse_body(msg_type, body)?;
        Ok(Some((message, total)))
    }

    /// Parse a message body of already-known length. The body length was fixed
    /// by the wrapper, so any shortfall here is `Malformed`, never "need more".
    fn parse_body(msg_type: u8, body: &[u8]) -> Result<Self, TlsError> {
        let mut r = Reader::new(body);
        let message = match msg_type {
            HS_CLIENT_HELLO => {
                let _legacy_version = r.u16()?;
                let random = r.array32()?;
                let legacy_session_id = r.opaque8()?;
                let cipher_suites = r.u16_list()?;
                let legacy_compression = r.opaque8()?;
                if legacy_compression != [0u8] {
                    return Err(TlsError::malformed(
                        "ClientHello legacy_compression_methods must be a single null byte",
                    ));
                }
                let extensions = r.extensions()?;
                Self::ClientHello(ClientHello { random, legacy_session_id, cipher_suites, extensions })
            }
            HS_SERVER_HELLO => {
                let _legacy_version = r.u16()?;
                let random = r.array32()?;
                let legacy_session_id_echo = r.opaque8()?;
                let cipher_suite = r.u16()?;
                let legacy_compression = r.u8()?;
                if legacy_compression != 0 {
                    return Err(TlsError::malformed(
                        "ServerHello legacy_compression_method must be 0",
                    ));
                }
                let extensions = r.extensions()?;
                Self::ServerHello(ServerHello {
                    random,
                    legacy_session_id_echo,
                    cipher_suite,
                    extensions,
                })
            }
            HS_ENCRYPTED_EXTENSIONS => Self::EncryptedExtensions(r.extensions()?),
            HS_CERTIFICATE_REQUEST => {
                let certificate_request_context = r.opaque8()?;
                let extensions = r.extensions()?;
                Self::CertificateRequest(CertificateRequest {
                    certificate_request_context,
                    extensions,
                })
            }
            HS_CERTIFICATE => {
                let certificate_request_context = r.opaque8()?;
                // certificate_list<0..2^24-1>: a u24-length-prefixed sequence of
                // CertificateEntry.
                let list_bytes = r.opaque24()?;
                let mut lr = Reader::new(&list_bytes);
                let mut certificate_list = Vec::new();
                while lr.remaining() > 0 {
                    let cert_data = lr.opaque24()?;
                    let extensions = lr.extensions()?;
                    certificate_list.push(CertificateEntry { cert_data, extensions });
                }
                Self::Certificate(Certificate { certificate_request_context, certificate_list })
            }
            HS_CERTIFICATE_VERIFY => {
                let algorithm = r.u16()?;
                let signature = r.opaque16()?;
                Self::CertificateVerify(CertificateVerify { algorithm, signature })
            }
            HS_FINISHED => {
                // verify_data fills the whole body (its length is the hash length,
                // known from the negotiated cipher suite, not encoded on the wire).
                let verify_data = r.rest().to_vec();
                Self::Finished(verify_data)
            }
            HS_NEW_SESSION_TICKET => {
                let ticket_lifetime = r.u32()?;
                let ticket_age_add = r.u32()?;
                let ticket_nonce = r.opaque8()?;
                let ticket = r.opaque16()?;
                let extensions = r.extensions()?;
                Self::NewSessionTicket(NewSessionTicket {
                    ticket_lifetime,
                    ticket_age_add,
                    ticket_nonce,
                    ticket,
                    extensions,
                })
            }
            HS_END_OF_EARLY_DATA => Self::EndOfEarlyData,
            HS_KEY_UPDATE => {
                let request = match r.u8()? {
                    0 => KeyUpdateRequest::NotRequested,
                    1 => KeyUpdateRequest::Requested,
                    other => {
                        return Err(TlsError::malformed(format!(
                            "KeyUpdate request_update must be 0 or 1, got {other}"
                        )));
                    }
                };
                Self::KeyUpdate(request)
            }
            other => {
                return Ok(Self::Unknown { msg_type: other, body: body.to_vec() });
            }
        };
        // Every typed message must consume its whole body; leftover bytes are a
        // decode error (RFC 8446 §4). `Finished`/`Unknown` consume by definition.
        if r.remaining() != 0 {
            return Err(TlsError::malformed(format!(
                "{} trailing byte(s) after handshake message type {msg_type}",
                r.remaining()
            )));
        }
        Ok(message)
    }

    /// Serialize this message (`msg_type` · `uint24 length` · body) onto `out`.
    ///
    /// # Errors
    ///
    /// [`TlsError::Overflow`] if the encoded body or one of its length-prefixed
    /// fields exceeds the capacity of its prefix (e.g. a body ≥ 2^24 bytes).
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), TlsError> {
        let mut body = Vec::new();
        self.encode_body(&mut body)?;
        out.push(self.msg_type());
        write_u24(body.len(), "handshake body", out)?;
        out.extend_from_slice(&body);
        Ok(())
    }

    /// Serialize just the message body (no `msg_type`/length wrapper).
    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), TlsError> {
        match self {
            Self::ClientHello(ch) => {
                write_u16(LEGACY_VERSION_TLS12, out);
                out.extend_from_slice(&ch.random);
                write_opaque8(&ch.legacy_session_id, "legacy_session_id", out)?;
                write_u16_list(&ch.cipher_suites, "cipher_suites", out)?;
                write_opaque8(&[0u8], "legacy_compression_methods", out)?;
                write_extensions(&ch.extensions, out)?;
            }
            Self::ServerHello(sh) => {
                write_u16(LEGACY_VERSION_TLS12, out);
                out.extend_from_slice(&sh.random);
                write_opaque8(&sh.legacy_session_id_echo, "legacy_session_id_echo", out)?;
                write_u16(sh.cipher_suite, out);
                out.push(0); // legacy_compression_method
                write_extensions(&sh.extensions, out)?;
            }
            Self::EncryptedExtensions(exts) => write_extensions(exts, out)?,
            Self::CertificateRequest(cr) => {
                write_opaque8(&cr.certificate_request_context, "certificate_request_context", out)?;
                write_extensions(&cr.extensions, out)?;
            }
            Self::Certificate(c) => {
                write_opaque8(&c.certificate_request_context, "certificate_request_context", out)?;
                let mut list = Vec::new();
                for entry in &c.certificate_list {
                    write_opaque24(&entry.cert_data, "cert_data", &mut list)?;
                    write_extensions(&entry.extensions, &mut list)?;
                }
                write_opaque24(&list, "certificate_list", out)?;
            }
            Self::CertificateVerify(cv) => {
                write_u16(cv.algorithm, out);
                write_opaque16(&cv.signature, "signature", out)?;
            }
            Self::Finished(verify_data) => out.extend_from_slice(verify_data),
            Self::NewSessionTicket(t) => {
                write_u32(t.ticket_lifetime, out);
                write_u32(t.ticket_age_add, out);
                write_opaque8(&t.ticket_nonce, "ticket_nonce", out)?;
                write_opaque16(&t.ticket, "ticket", out)?;
                write_extensions(&t.extensions, out)?;
            }
            Self::EndOfEarlyData => {}
            Self::KeyUpdate(req) => out.push(match req {
                KeyUpdateRequest::NotRequested => 0,
                KeyUpdateRequest::Requested => 1,
            }),
            Self::Unknown { body, .. } => out.extend_from_slice(body),
        }
        Ok(())
    }
}

// ── key_share extension body (RFC 8446 §4.2.8) ───────────────────────────────

/// One `KeyShareEntry` (RFC 8446 §4.2.8): a named group and the corresponding
/// ephemeral public key. For [`GROUP_X25519`] the `key_exchange` is the 32-byte
/// X25519 public value that feeds the `(EC)DHE` agreement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyShareEntry {
    /// The `NamedGroup` (e.g. [`GROUP_X25519`]).
    pub group: u16,
    /// The `key_exchange` public value (`<1..2^16-1>`).
    pub key_exchange: Vec<u8>,
}

impl KeyShareEntry {
    /// Parse the `key_share` extension body from a **ClientHello**: a
    /// `client_shares<0..2^16-1>` vector of entries (RFC 8446 §4.2.8).
    ///
    /// # Errors
    ///
    /// [`TlsError::Malformed`] if the body is truncated or has trailing bytes.
    pub fn parse_client_hello(data: &[u8]) -> Result<Vec<Self>, TlsError> {
        let mut r = Reader::new(data);
        let shares = r.opaque16()?;
        r.expect_end("key_share client_shares")?;
        let mut sr = Reader::new(&shares);
        let mut out = Vec::new();
        while sr.remaining() > 0 {
            out.push(Self::read(&mut sr)?);
        }
        Ok(out)
    }

    /// Encode a ClientHello `key_share` extension body from a list of entries.
    ///
    /// # Errors
    ///
    /// [`TlsError::Overflow`] if a `key_exchange` or the whole list exceeds its
    /// 2-byte length prefix.
    pub fn encode_client_hello(entries: &[Self]) -> Result<Vec<u8>, TlsError> {
        let mut shares = Vec::new();
        for e in entries {
            e.write(&mut shares)?;
        }
        let mut out = Vec::new();
        write_opaque16(&shares, "client_shares", &mut out)?;
        Ok(out)
    }

    /// Parse the `key_share` extension body from a **ServerHello**: a single
    /// `KeyShareEntry` with no outer list prefix (RFC 8446 §4.2.8).
    ///
    /// # Errors
    ///
    /// [`TlsError::Malformed`] if truncated or followed by trailing bytes.
    pub fn parse_server_hello(data: &[u8]) -> Result<Self, TlsError> {
        let mut r = Reader::new(data);
        let entry = Self::read(&mut r)?;
        r.expect_end("key_share server share")?;
        Ok(entry)
    }

    /// Encode a ServerHello `key_share` extension body (a single entry).
    ///
    /// # Errors
    ///
    /// [`TlsError::Overflow`] if `key_exchange` exceeds its 2-byte length prefix.
    pub fn encode_server_hello(&self) -> Result<Vec<u8>, TlsError> {
        let mut out = Vec::new();
        self.write(&mut out)?;
        Ok(out)
    }

    /// Read one entry (`group` · `opaque16 key_exchange`) from `r`.
    fn read(r: &mut Reader<'_>) -> Result<Self, TlsError> {
        let group = r.u16()?;
        let key_exchange = r.opaque16()?;
        Ok(Self { group, key_exchange })
    }

    /// Write one entry (`group` · `opaque16 key_exchange`) to `out`.
    fn write(&self, out: &mut Vec<u8>) -> Result<(), TlsError> {
        write_u16(self.group, out);
        write_opaque16(&self.key_exchange, "key_exchange", out)?;
        Ok(())
    }
}

// ── supported_versions extension body (RFC 8446 §4.2.1) ──────────────────────

/// Parse the `supported_versions` body from a **ClientHello**: a
/// `versions<2..254>` vector of 2-byte version codes (RFC 8446 §4.2.1). The
/// ServerHello form is a bare `selected_version` (2 bytes) — read it directly.
///
/// # Errors
///
/// [`TlsError::Malformed`] if truncated, odd-length, or with trailing bytes.
pub fn supported_versions(data: &[u8]) -> Result<Vec<u16>, TlsError> {
    let mut r = Reader::new(data);
    let versions = r.u8_prefixed_u16_list()?;
    r.expect_end("supported_versions")?;
    Ok(versions)
}

/// Encode a ClientHello `supported_versions` body from a version list — a
/// 1-byte byte-count prefix over the `u16` version codes (RFC 8446 §4.2.1).
///
/// # Errors
///
/// [`TlsError::Overflow`] if the list exceeds its 1-byte length prefix.
pub fn encode_supported_versions(versions: &[u16]) -> Result<Vec<u8>, TlsError> {
    let mut body = Vec::with_capacity(versions.len() * 2);
    for &v in versions {
        write_u16(v, &mut body);
    }
    let mut out = Vec::new();
    write_opaque8(&body, "supported_versions", &mut out)?;
    Ok(out)
}

// ── Reader ───────────────────────────────────────────────────────────────────

/// A forward-only cursor over a fully-buffered message body. Every accessor
/// returns [`TlsError::Malformed`] on underrun, because the enclosing length
/// prefix already promised the bytes are present.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Wrap a byte slice at offset 0.
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Bytes not yet consumed.
    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Consume and return the next `n` bytes, or `Malformed` if fewer remain.
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], TlsError> {
        if self.remaining() < n {
            return Err(TlsError::malformed(format!(
                "{what}: need {n} byte(s), {} remaining",
                self.remaining()
            )));
        }
        let slice = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// Consume all remaining bytes.
    fn rest(&mut self) -> &'a [u8] {
        let slice = &self.buf[self.pos..];
        self.pos = self.buf.len();
        slice
    }

    /// Read a big-endian `u8`.
    fn u8(&mut self) -> Result<u8, TlsError> {
        Ok(self.take(1, "u8")?[0])
    }

    /// Read a big-endian `u16`.
    fn u16(&mut self) -> Result<u16, TlsError> {
        let b = self.take(2, "u16")?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    /// Read a big-endian `u32`.
    fn u32(&mut self) -> Result<u32, TlsError> {
        let b = self.take(4, "u32")?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a fixed 32-byte array (a TLS `Random` / key share value).
    fn array32(&mut self) -> Result<[u8; 32], TlsError> {
        let mut out = [0u8; 32];
        out.copy_from_slice(self.take(32, "array32")?);
        Ok(out)
    }

    /// Read an `opaque<0..2^8-1>` byte vector (1-byte length prefix).
    fn opaque8(&mut self) -> Result<Vec<u8>, TlsError> {
        let len = self.u8()? as usize;
        Ok(self.take(len, "opaque8 body")?.to_vec())
    }

    /// Read an `opaque<0..2^16-1>` byte vector (2-byte length prefix).
    fn opaque16(&mut self) -> Result<Vec<u8>, TlsError> {
        let len = self.u16()? as usize;
        Ok(self.take(len, "opaque16 body")?.to_vec())
    }

    /// Read an `opaque<0..2^24-1>` byte vector (3-byte length prefix).
    fn opaque24(&mut self) -> Result<Vec<u8>, TlsError> {
        let b = self.take(3, "opaque24 length")?;
        let len = u32::from_be_bytes([0, b[0], b[1], b[2]]) as usize;
        Ok(self.take(len, "opaque24 body")?.to_vec())
    }

    /// Read a `u16` vector behind a **2-byte** byte-count prefix — the
    /// cipher-suite and supported-groups shape (RFC 8446 §4.1.2, §4.2.7). The
    /// prefix is a byte count and must be a whole number of `u16`s.
    /// `supported_versions` (ClientHello) uses a 1-byte prefix instead, handled
    /// by [`Self::u8_prefixed_u16_list`].
    fn u16_list(&mut self) -> Result<Vec<u16>, TlsError> {
        let bytes = self.opaque16()?;
        u16s_from_bytes(&bytes, "u16 list")
    }

    /// Extensions vector (`Extension extensions<..>`): a 2-byte byte-count
    /// prefix, then `Extension` records until it is exhausted (RFC 8446 §4.2).
    fn extensions(&mut self) -> Result<Vec<Extension>, TlsError> {
        // An absent extensions vector (nothing left in the body) is legal for
        // messages where the field is optional; treat end-of-body as empty.
        if self.remaining() == 0 {
            return Ok(Vec::new());
        }
        let block = self.opaque16()?;
        let mut er = Reader::new(&block);
        let mut out = Vec::new();
        while er.remaining() > 0 {
            let extension_type = er.u16()?;
            let data = er.opaque16()?;
            out.push(Extension { extension_type, data });
        }
        Ok(out)
    }

    /// `supported_versions` ClientHello form: a 1-byte byte-count prefix whose
    /// body is `u16` version codes (RFC 8446 §4.2.1).
    fn u8_prefixed_u16_list(&mut self) -> Result<Vec<u16>, TlsError> {
        let bytes = self.opaque8()?;
        u16s_from_bytes(&bytes, "supported_versions")
    }

    /// Assert the cursor is at the end of its buffer (no trailing bytes).
    fn expect_end(&self, what: &str) -> Result<(), TlsError> {
        if self.remaining() != 0 {
            return Err(TlsError::malformed(format!(
                "{what}: {} trailing byte(s)",
                self.remaining()
            )));
        }
        Ok(())
    }
}

/// Interpret a byte buffer as a sequence of big-endian `u16`s.
fn u16s_from_bytes(bytes: &[u8], what: &str) -> Result<Vec<u16>, TlsError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(TlsError::malformed(format!(
            "{what}: {} byte(s) is not a whole number of u16s",
            bytes.len()
        )));
    }
    Ok(bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect())
}

// ── Writers ──────────────────────────────────────────────────────────────────

/// Append a big-endian `u16`.
fn write_u16(v: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// Append a big-endian `u32`.
fn write_u32(v: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// Append a big-endian `uint24` from a `usize`, erroring if `v ≥ 2^24`.
fn write_u24(v: usize, what: &str, out: &mut Vec<u8>) -> Result<(), TlsError> {
    if v > 0x00ff_ffff {
        return Err(TlsError::Overflow(format!("{what}: {v} exceeds 2^24-1")));
    }
    let b = (v as u32).to_be_bytes();
    out.extend_from_slice(&b[1..]);
    Ok(())
}

/// Append an `opaque<0..2^8-1>` (1-byte length prefix + body).
fn write_opaque8(data: &[u8], what: &str, out: &mut Vec<u8>) -> Result<(), TlsError> {
    if data.len() > 0xff {
        return Err(TlsError::Overflow(format!("{what}: {} bytes exceeds 2^8-1", data.len())));
    }
    out.push(data.len() as u8);
    out.extend_from_slice(data);
    Ok(())
}

/// Append an `opaque<0..2^16-1>` (2-byte length prefix + body).
fn write_opaque16(data: &[u8], what: &str, out: &mut Vec<u8>) -> Result<(), TlsError> {
    if data.len() > 0xffff {
        return Err(TlsError::Overflow(format!("{what}: {} bytes exceeds 2^16-1", data.len())));
    }
    write_u16(data.len() as u16, out);
    out.extend_from_slice(data);
    Ok(())
}

/// Append an `opaque<0..2^24-1>` (3-byte length prefix + body).
fn write_opaque24(data: &[u8], what: &str, out: &mut Vec<u8>) -> Result<(), TlsError> {
    write_u24(data.len(), what, out)?;
    out.extend_from_slice(data);
    Ok(())
}

/// Append a `u16` vector behind a **2-byte** byte-count prefix — the
/// cipher-suite / supported-groups shape (RFC 8446 §4.1.2, §4.2.7).
/// `supported_versions` (ClientHello) uses a 1-byte prefix; see
/// [`encode_supported_versions`].
fn write_u16_list(values: &[u16], what: &str, out: &mut Vec<u8>) -> Result<(), TlsError> {
    let mut body = Vec::with_capacity(values.len() * 2);
    for &v in values {
        write_u16(v, &mut body);
    }
    write_opaque16(&body, what, out)
}

/// Append an extensions vector (`Extension extensions<..>`, RFC 8446 §4.2): a
/// 2-byte byte-count prefix, then each `type · opaque16 data` record.
fn write_extensions(extensions: &[Extension], out: &mut Vec<u8>) -> Result<(), TlsError> {
    let mut block = Vec::new();
    for ext in extensions {
        write_u16(ext.extension_type, &mut block);
        write_opaque16(&ext.data, "extension_data", &mut block)?;
    }
    write_opaque16(&block, "extensions", out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a hex string into bytes (test helper).
    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    /// Encode a message, parse it back, and assert both the value and that the
    /// whole buffer was consumed.
    fn roundtrip(msg: &Handshake) {
        let mut buf = Vec::new();
        msg.encode(&mut buf).expect("encode");
        let (got, consumed) = Handshake::parse(&buf).expect("no error").expect("complete");
        assert_eq!(&got, msg, "round-trip value");
        assert_eq!(consumed, buf.len(), "consumed whole message");
    }

    fn ext(t: u16, data: &[u8]) -> Extension {
        Extension::new(t, data.to_vec())
    }

    #[test]
    fn roundtrip_client_hello() {
        let ch = Handshake::ClientHello(ClientHello {
            random: [0x11; 32],
            legacy_session_id: vec![0xaa; 32],
            cipher_suites: vec![
                TLS_AES_128_GCM_SHA256,
                TLS_AES_256_GCM_SHA384,
                TLS_CHACHA20_POLY1305_SHA256,
            ],
            extensions: vec![
                ext(EXT_SUPPORTED_VERSIONS, &encode_supported_versions(&[VERSION_TLS13]).unwrap()),
                ext(
                    EXT_KEY_SHARE,
                    &KeyShareEntry::encode_client_hello(&[KeyShareEntry {
                        group: GROUP_X25519,
                        key_exchange: vec![0x42; 32],
                    }])
                    .unwrap(),
                ),
            ],
        });
        roundtrip(&ch);
    }

    #[test]
    fn roundtrip_all_message_types() {
        roundtrip(&Handshake::ServerHello(ServerHello {
            random: [0x22; 32],
            legacy_session_id_echo: Vec::new(),
            cipher_suite: TLS_AES_128_GCM_SHA256,
            extensions: vec![ext(EXT_SUPPORTED_VERSIONS, &VERSION_TLS13.to_be_bytes())],
        }));
        roundtrip(&Handshake::EncryptedExtensions(vec![ext(EXT_ALPN, b"\x00\x03\x02h3")]));
        roundtrip(&Handshake::CertificateRequest(CertificateRequest {
            certificate_request_context: vec![0x01, 0x02],
            extensions: vec![ext(EXT_SIGNATURE_ALGORITHMS, &[0x00, 0x02, 0x08, 0x04])],
        }));
        roundtrip(&Handshake::Certificate(Certificate {
            certificate_request_context: Vec::new(),
            certificate_list: vec![
                CertificateEntry { cert_data: vec![0xde; 300], extensions: Vec::new() },
                CertificateEntry {
                    cert_data: vec![0xad; 50],
                    extensions: vec![ext(5, &[0x00])],
                },
            ],
        }));
        roundtrip(&Handshake::CertificateVerify(CertificateVerify {
            algorithm: 0x0804,
            signature: vec![0x99; 256],
        }));
        roundtrip(&Handshake::Finished(vec![0x33; 32]));
        roundtrip(&Handshake::NewSessionTicket(NewSessionTicket {
            ticket_lifetime: 7200,
            ticket_age_add: 0xdead_beef,
            ticket_nonce: vec![0x00, 0x01],
            ticket: vec![0x55; 128],
            extensions: Vec::new(),
        }));
        roundtrip(&Handshake::EndOfEarlyData);
        roundtrip(&Handshake::KeyUpdate(KeyUpdateRequest::NotRequested));
        roundtrip(&Handshake::KeyUpdate(KeyUpdateRequest::Requested));
        roundtrip(&Handshake::Unknown { msg_type: 250, body: vec![1, 2, 3, 4] });
    }

    /// Parse the RFC 8448 §3 ServerHello wire bytes and re-encode them
    /// byte-for-byte — the strongest structural check against a real message.
    #[test]
    fn rfc8448_server_hello_roundtrips_byte_exact() {
        // RFC 8448 §3 "Simple 1-RTT Handshake", the server's ServerHello.
        let wire = hex(
            "020000560303a6af06a4121860dc5e6e60249cd34c95930c8ac5cb1434dac15577\
             2ed3e2692800130100002e00330024001d0020c9828876112095fe66762bdbf7c6\
             72e156d6cc253b833df1dd69b1b04e751f0f002b00020304",
        );
        let (msg, consumed) = Handshake::parse(&wire).expect("no error").expect("complete");
        assert_eq!(consumed, wire.len());
        let Handshake::ServerHello(sh) = &msg else {
            panic!("expected ServerHello, got {msg:?}");
        };
        assert_eq!(sh.cipher_suite, TLS_AES_128_GCM_SHA256);
        assert!(!sh.is_hello_retry_request());
        // supported_versions (ServerHello form) selects TLS 1.3, and the
        // key_share carries an x25519 public value of 32 bytes.
        let sv = sh
            .extensions
            .iter()
            .find(|e| e.extension_type == EXT_SUPPORTED_VERSIONS)
            .expect("supported_versions present");
        assert_eq!(sv.data, VERSION_TLS13.to_be_bytes());
        let ks = sh
            .extensions
            .iter()
            .find(|e| e.extension_type == EXT_KEY_SHARE)
            .expect("key_share present");
        let entry = KeyShareEntry::parse_server_hello(&ks.data).expect("parse key share");
        assert_eq!(entry.group, GROUP_X25519);
        assert_eq!(entry.key_exchange.len(), 32);

        // Re-encoding reproduces the exact input bytes (transcript fidelity).
        let mut reencoded = Vec::new();
        msg.encode(&mut reencoded).expect("encode");
        assert_eq!(reencoded, wire);
    }

    #[test]
    fn hello_retry_request_detected() {
        let hrr = ServerHello {
            random: HELLO_RETRY_REQUEST_RANDOM,
            legacy_session_id_echo: Vec::new(),
            cipher_suite: TLS_AES_128_GCM_SHA256,
            extensions: vec![ext(EXT_SUPPORTED_VERSIONS, &VERSION_TLS13.to_be_bytes())],
        };
        assert!(hrr.is_hello_retry_request());
        // A HelloRetryRequest is a ServerHello on the wire — it round-trips.
        roundtrip(&Handshake::ServerHello(hrr));
    }

    #[test]
    fn parse_needs_full_message() {
        let msg = Handshake::Finished(vec![0x33; 32]);
        let mut buf = Vec::new();
        msg.encode(&mut buf).unwrap();
        // Header alone, and every truncation short of the full body, yields None.
        assert_eq!(Handshake::parse(&buf[..0]).unwrap(), None, "empty");
        assert_eq!(Handshake::parse(&buf[..3]).unwrap(), None, "partial header");
        assert_eq!(Handshake::parse(&buf[..buf.len() - 1]).unwrap(), None, "1 byte short");
        assert!(Handshake::parse(&buf).unwrap().is_some(), "complete");
    }

    #[test]
    fn parse_leaves_trailing_message() {
        let mut buf = Vec::new();
        Handshake::EndOfEarlyData.encode(&mut buf).unwrap();
        let first_len = buf.len();
        Handshake::Finished(vec![7; 16]).encode(&mut buf).unwrap();
        let (first, consumed) = Handshake::parse(&buf).unwrap().unwrap();
        assert_eq!(first, Handshake::EndOfEarlyData);
        assert_eq!(consumed, first_len);
        // The remainder parses as the second message.
        let (second, _) = Handshake::parse(&buf[consumed..]).unwrap().unwrap();
        assert_eq!(second, Handshake::Finished(vec![7; 16]));
    }

    #[test]
    fn rejects_trailing_bytes_in_body() {
        // A ServerHello body followed by one extra byte inside its declared
        // length must be rejected, not silently accepted.
        let mut body = Vec::new();
        Handshake::ServerHello(ServerHello {
            random: [0; 32],
            legacy_session_id_echo: Vec::new(),
            cipher_suite: TLS_AES_128_GCM_SHA256,
            extensions: Vec::new(),
        })
        .encode_body(&mut body)
        .unwrap();
        body.push(0xff); // trailing garbage
        let mut wire = vec![HS_SERVER_HELLO];
        write_u24(body.len(), "t", &mut wire).unwrap();
        wire.extend_from_slice(&body);
        assert!(matches!(Handshake::parse(&wire), Err(TlsError::Malformed(_))));
    }

    #[test]
    fn rejects_bad_client_hello_compression() {
        // legacy_compression_methods must be exactly {0}; {1,0} is illegal.
        let mut body = Vec::new();
        write_u16(LEGACY_VERSION_TLS12, &mut body);
        body.extend_from_slice(&[0u8; 32]); // random
        write_opaque8(&[], "sid", &mut body).unwrap();
        write_u16_list(&[TLS_AES_128_GCM_SHA256], "cipher_suites", &mut body).unwrap();
        write_opaque8(&[1u8, 0u8], "compression", &mut body).unwrap(); // wrong
        write_extensions(&[], &mut body).unwrap();
        let mut wire = vec![HS_CLIENT_HELLO];
        write_u24(body.len(), "t", &mut wire).unwrap();
        wire.extend_from_slice(&body);
        assert!(matches!(Handshake::parse(&wire), Err(TlsError::Malformed(_))));
    }

    #[test]
    fn key_share_client_hello_roundtrip() {
        let entries = vec![
            KeyShareEntry { group: GROUP_X25519, key_exchange: vec![0xab; 32] },
            KeyShareEntry { group: GROUP_SECP256R1, key_exchange: vec![0x04; 65] },
        ];
        let body = KeyShareEntry::encode_client_hello(&entries).unwrap();
        assert_eq!(KeyShareEntry::parse_client_hello(&body).unwrap(), entries);
    }

    #[test]
    fn supported_versions_client_hello_roundtrip() {
        let versions = vec![VERSION_TLS13, 0x0303];
        let body = encode_supported_versions(&versions).unwrap();
        assert_eq!(supported_versions(&body).unwrap(), versions);
    }

    #[test]
    fn overflow_on_oversized_signature() {
        let cv = Handshake::CertificateVerify(CertificateVerify {
            algorithm: 0x0804,
            signature: vec![0u8; 0x1_0000], // exactly 2^16, one past the u16 prefix
        });
        let mut out = Vec::new();
        assert!(matches!(cv.encode(&mut out), Err(TlsError::Overflow(_))));
    }

    /// The codec composes with slice 15: concatenating a ClientHello and a
    /// ServerHello gives the `CH..SH` transcript the key schedule hashes.
    #[test]
    fn transcript_feeds_key_schedule() {
        use crate::h3::tls_schedule;

        let ch = Handshake::ClientHello(ClientHello {
            random: [0x11; 32],
            legacy_session_id: Vec::new(),
            cipher_suites: vec![TLS_AES_128_GCM_SHA256],
            extensions: vec![ext(EXT_SUPPORTED_VERSIONS, &VERSION_TLS13.to_be_bytes())],
        });
        let sh = Handshake::ServerHello(ServerHello {
            random: [0x22; 32],
            legacy_session_id_echo: Vec::new(),
            cipher_suite: TLS_AES_128_GCM_SHA256,
            extensions: Vec::new(),
        });
        let mut transcript = Vec::new();
        ch.encode(&mut transcript).unwrap();
        sh.encode(&mut transcript).unwrap();

        let hs = tls_schedule::handshake_secret(&[0x99; 32]);
        let secrets = tls_schedule::HandshakeTrafficSecrets::derive(&hs, &transcript);
        // Distinct directions, and the derivation is deterministic in the
        // transcript we produced.
        assert_ne!(secrets.client, secrets.server);
    }
}
