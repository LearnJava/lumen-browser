//! X.509 certificate signature verification (RFC 5280 §4.1.1.3, §4.1.2.3) — slices 66
//! and 67 of the HTTP/3 sprint.
//!
//! The three preceding X.509 slices each authenticate the *end-entity* certificate
//! the server presents: possession ([`conn_cert_auth`](super::conn_cert_auth)) proves
//! the peer holds the key, identity ([`x509_hostname`](super::x509_hostname)) proves
//! the certificate names the host, and validity ([`x509_validity`](super::x509_validity))
//! proves it is current. All three trust the end-entity certificate *as given* — none
//! asks the remaining question of WebPKI: is that certificate itself signed by an
//! authority the client trusts? A TLS 1.3 server sends a chain (RFC 8446 §4.4.2), and a
//! chain is only meaningful if each certificate's signature verifies under the *next*
//! certificate's public key, up to a trust anchor.
//!
//! [`verify_certificate_signature`] (slice 66) is the atomic building block: it verifies
//! one certificate's signature under one candidate issuer's public key.
//! [`verify_chain_signatures`] (slice 67) is the walk over that block: it takes the
//! server's whole `certificate_list`, extracts each certificate's own public key with
//! [`x509_spki::extract_end_entity_public_key`](super::x509_spki::extract_end_entity_public_key)
//! (slice 60), and checks that every certificate is signed by the one above it. A chain
//! whose internal links all verify still needs its top certificate bound to a trust
//! anchor — the walk deliberately stops one link short of that, leaving termination,
//! name-chaining, and certificate constraints (RFC 5280 §6) to later slices, exactly as
//! slice 66 leaves them to this one.
//!
//! ## What it verifies
//!
//! A certificate binds its signature to its `tbsCertificate` (RFC 5280 §4.1):
//!
//! ```text
//! Certificate ::= SEQUENCE {
//!     tbsCertificate       TBSCertificate,       -- the signed content
//!     signatureAlgorithm   AlgorithmIdentifier,  -- MUST equal tbsCertificate.signature
//!     signatureValue       BIT STRING }          -- the signature over the TBS DER
//!
//! TBSCertificate ::= SEQUENCE {
//!     version      [0] EXPLICIT ... DEFAULT v1,  -- context 0xA0, optional
//!     serialNumber     INTEGER,
//!     signature        AlgorithmIdentifier,      -- the issuer's signing algorithm
//!     ... }
//! ```
//!
//! [`verify_certificate_signature`] takes the DER of one `Certificate` and the
//! [`ServerPublicKey`](super::x509_spki::ServerPublicKey) of a candidate issuer (the
//! next certificate's `subjectPublicKeyInfo`, extracted by
//! [`x509_spki`](super::x509_spki)). It:
//!
//! 1. splits the `Certificate` into its three fields, keeping the *raw* DER of the
//!    `tbsCertificate` — the exact bytes the signature covers (RFC 5280 §4.1.1.3, "the
//!    signature is applied to the DER encoded `tbsCertificate`");
//! 2. cross-checks that the outer `signatureAlgorithm` equals the inner
//!    `tbsCertificate.signature` `AlgorithmIdentifier` (RFC 5280 §4.1.1.2), rejecting a
//!    certificate whose two algorithm declarations disagree — a signature-substitution
//!    tell;
//! 3. maps the algorithm OID to the matching
//!    [`tls_cert_verify`](super::tls_cert_verify) primitive and verifies the
//!    `signatureValue` over the raw `tbsCertificate` under the issuer's key.
//!
//! It does *not* build or order the chain, match `issuer`/`subject` distinguished
//! names, honour `basicConstraints`/`keyUsage`, or terminate at a trust anchor — those
//! are the surrounding path-validation concerns (RFC 5280 §6), wired above this
//! primitive in later slices, exactly as the possession check leaves hostname and
//! validity to sibling slices.
//!
//! ## Signature algorithms
//!
//! X.509 certificates name the signing algorithm with an `AlgorithmIdentifier` OID
//! (RFC 5280 §4.1.1.2), *not* a TLS 1.3 `SignatureScheme` codepoint. This slice
//! verifies the ECDSA and EdDSA families, reusing the digest-coupled verifiers
//! [`tls_cert_verify`](super::tls_cert_verify) already exposes:
//!
//! - **`ecdsa-with-SHA256`** (1.2.840.10045.4.3.2) under an ECDSA P-256 issuer key →
//!   [`ecdsa_p256_sha256_verify`](super::tls_cert_verify::ecdsa_p256_sha256_verify).
//! - **`ecdsa-with-SHA384`** (1.2.840.10045.4.3.3) under a P-384 issuer key →
//!   [`ecdsa_p384_sha384_verify`](super::tls_cert_verify::ecdsa_p384_sha384_verify).
//! - **`ecdsa-with-SHA512`** (1.2.840.10045.4.3.4) under a P-521 issuer key →
//!   [`ecdsa_p521_sha512_verify`](super::tls_cert_verify::ecdsa_p521_sha512_verify).
//! - **`id-Ed25519`** (1.3.101.112) under an Ed25519 issuer key →
//!   [`ed25519_verify`](super::tls_cert_verify::ed25519_verify).
//!
//! Because those verifiers couple curve and digest, this slice pairs each ECDSA
//! algorithm with its canonical curve (P-256/SHA-256, P-384/SHA-384, P-521/SHA-512).
//! A non-canonical pairing (a P-384 issuer key presenting an `ecdsa-with-SHA256`
//! signature) is rejected as [`ChainError::AlgorithmMismatch`] rather than silently
//! accepted — fail-closed. **Deferred:** RSA (`sha256WithRSAEncryption` and the PKCS#1
//! v1.5 family, plus RSASSA-PSS) certificate signatures, which need a PKCS#1 v1.5
//! verifier this crate does not yet have — its only RSA primitive is the RSASSA-PSS one
//! for TLS `CertificateVerify`. Until that lands, an RSA-signed certificate returns
//! [`ChainError::UnsupportedSignatureAlgorithm`].
//!
//! ## Purity
//!
//! Pure DER parsing and signature math over borrowed bytes: no clock, no I/O, no
//! allocation beyond the tiny signed-content borrow the verifiers take. A sibling of
//! [`x509_spki`](super::x509_spki), [`x509_hostname`](super::x509_hostname), and
//! [`x509_validity`](super::x509_validity).

use super::tls_cert_verify::{
    CertVerifyError, ecdsa_p256_sha256_verify, ecdsa_p384_sha384_verify, ecdsa_p521_sha512_verify,
    ed25519_verify,
};
use super::x509_spki::{
    ServerPublicKey, SpkiAlgorithm, SpkiError, extract_end_entity_public_key,
};

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `OBJECT IDENTIFIER`.
const TAG_OID: u8 = 0x06;
/// The DER tag for `BIT STRING`.
const TAG_BIT_STRING: u8 = 0x03;
/// The DER tag for the optional `[0] EXPLICIT` `version` field of a `TBSCertificate`.
const TAG_CONTEXT_0: u8 = 0xA0;

/// `ecdsa-with-SHA256` (1.2.840.10045.4.3.2, RFC 5758 §3.2) — ECDSA signatures with a
/// SHA-256 digest. The digest is fixed by the OID; the curve comes from the issuer key.
const OID_ECDSA_WITH_SHA256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
/// `ecdsa-with-SHA384` (1.2.840.10045.4.3.3, RFC 5758 §3.2) — ECDSA with SHA-384.
const OID_ECDSA_WITH_SHA384: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03];
/// `ecdsa-with-SHA512` (1.2.840.10045.4.3.4, RFC 5758 §3.2) — ECDSA with SHA-512.
const OID_ECDSA_WITH_SHA512: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x04];
/// `id-Ed25519` (1.3.101.112, RFC 8410 §3) — the Ed25519 signature algorithm (same OID
/// as the Ed25519 *key* algorithm; EdDSA needs no separate digest identifier).
const OID_ED25519: &[u8] = &[0x2B, 0x65, 0x70];

/// The certificate signature algorithm this slice recognises, resolved from the
/// `signatureAlgorithm` OID and paired with the [`tls_cert_verify`](super::tls_cert_verify)
/// primitive that checks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChainSignatureAlgorithm {
    /// `ecdsa-with-SHA256` — verified with an ECDSA P-256 issuer key.
    EcdsaWithSha256,
    /// `ecdsa-with-SHA384` — verified with an ECDSA P-384 issuer key.
    EcdsaWithSha384,
    /// `ecdsa-with-SHA512` — verified with an ECDSA P-521 issuer key.
    EcdsaWithSha512,
    /// `id-Ed25519` — verified with an Ed25519 issuer key.
    Ed25519,
}

/// Why verifying a certificate's signature against a candidate issuer's key failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag
    /// where the `Certificate`/`TBSCertificate` structure required a specific one.
    /// Carries a static hint naming the field that did not decode.
    Malformed(&'static str),
    /// The certificate's outer `signatureAlgorithm` did not match its inner
    /// `tbsCertificate.signature` `AlgorithmIdentifier` (RFC 5280 §4.1.1.2), or the
    /// issuer's key type does not match the signature algorithm (a P-384 key against an
    /// `ecdsa-with-SHA256` signature). Carries a static hint. A fatal authentication
    /// failure: the certificate is not honestly self-describing.
    AlgorithmMismatch(&'static str),
    /// The `signatureAlgorithm` named an algorithm this slice does not verify —
    /// currently anything outside the ECDSA (SHA-256/384/512) and Ed25519 set, notably
    /// the RSA families whose PKCS#1 v1.5 verifier is not yet implemented.
    UnsupportedSignatureAlgorithm,
    /// The DER decoded and the algorithms agreed, but the signature did not verify over
    /// the `tbsCertificate` under the issuer's public key (RFC 5280 §4.1.1.3): the
    /// issuer did not sign this certificate. A fatal authentication failure.
    BadSignature,
}

impl core::fmt::Display for ChainError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
            Self::AlgorithmMismatch(what) => write!(f, "certificate algorithm mismatch: {what}"),
            Self::UnsupportedSignatureAlgorithm => {
                f.write_str("unsupported certificate signature algorithm")
            }
            Self::BadSignature => f.write_str("certificate signature did not verify under issuer key"),
        }
    }
}

impl std::error::Error for ChainError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples
/// left to right. Definite-length only (DER forbids the indefinite form). A sibling of
/// the readers in [`x509_spki`](super::x509_spki), [`x509_hostname`](super::x509_hostname),
/// and [`x509_validity`](super::x509_validity), specialised to this slice's error type
/// and adding a raw-span read for the `tbsCertificate` bytes the signature covers.
struct Der<'a> {
    /// The bytes being read.
    bytes: &'a [u8],
    /// The offset of the next unread byte.
    pos: usize,
}

impl<'a> Der<'a> {
    /// A reader positioned at the start of `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// The number of unread bytes.
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Read a DER definite length at the cursor (X.690): a short form (`0x00..=0x7f`)
    /// is the length itself; a long form (`0x81..`) gives the count of big-endian
    /// length octets that follow. The indefinite form (`0x80`) and counts wider than
    /// four octets are rejected.
    fn read_length(&mut self) -> Result<usize, ChainError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(ChainError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(ChainError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(ChainError::Malformed("truncated long-form length"));
        }
        let mut len = 0usize;
        for _ in 0..count {
            len = (len << 8) | self.bytes[self.pos] as usize;
            self.pos += 1;
        }
        Ok(len)
    }

    /// Read one TLV, returning its tag and a slice over its contents, and advance the
    /// cursor past it.
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), ChainError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(ChainError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(ChainError::Malformed("truncated: content shorter than its length"));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and return its tag, its contents, *and* the full `tag ‖ length ‖
    /// contents` span exactly as it appears in the input. The raw span is what a
    /// certificate signature covers (the DER-encoded `tbsCertificate`, RFC 5280
    /// §4.1.1.3), which the contents alone cannot reconstruct without re-encoding.
    fn read_tlv_raw(&mut self) -> Result<(u8, &'a [u8], &'a [u8]), ChainError> {
        let start = self.pos;
        let (tag, contents) = self.read_tlv()?;
        let raw = &self.bytes[start..self.pos];
        Ok((tag, contents, raw))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names
    /// the field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], ChainError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(ChainError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Verify one certificate's signature under a candidate issuer's public key (RFC 5280
/// §4.1.1.3).
///
/// `cert_der` is the DER of the certificate whose signature is being checked — a
/// `CertificateEntry.cert_data` from the server's `Certificate` message (RFC 8446
/// §4.4.2). `issuer_public_key` is the public key of the certificate one step up the
/// chain (its `subjectPublicKeyInfo`, as extracted by
/// [`extract_end_entity_public_key`](super::x509_spki::extract_end_entity_public_key)),
/// the key that must have produced `cert_der`'s signature.
///
/// On success the issuer's key verifies `cert_der`'s `signatureValue` over its raw
/// `tbsCertificate`. This is a single link check; it says nothing about name chaining,
/// certificate constraints, or trust-anchor termination — those are layered above.
///
/// # Errors
///
/// - [`ChainError::Malformed`] if the certificate DER is truncated or mis-structured.
/// - [`ChainError::AlgorithmMismatch`] if the outer `signatureAlgorithm` disagrees with
///   the inner `tbsCertificate.signature`, or the issuer key type does not match the
///   signature algorithm.
/// - [`ChainError::UnsupportedSignatureAlgorithm`] for a signature algorithm outside the
///   ECDSA (SHA-256/384/512) and Ed25519 set this slice verifies.
/// - [`ChainError::BadSignature`] if the signature does not verify under the issuer key.
pub fn verify_certificate_signature(
    cert_der: &[u8],
    issuer_public_key: &ServerPublicKey,
) -> Result<(), ChainError> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE, "certificate is not a SEQUENCE")?;
    let mut certificate = Der::new(certificate);

    // Keep the raw tbsCertificate bytes: the signature is over this exact DER.
    let (tbs_tag, tbs_contents, tbs_raw) = certificate.read_tlv_raw()?;
    if tbs_tag != TAG_SEQUENCE {
        return Err(ChainError::Malformed("tbsCertificate is not a SEQUENCE"));
    }
    let (outer_alg_tag, _outer_alg_contents, outer_alg_raw) = certificate.read_tlv_raw()?;
    if outer_alg_tag != TAG_SEQUENCE {
        return Err(ChainError::Malformed("signatureAlgorithm is not a SEQUENCE"));
    }
    let signature_value = certificate.read_tagged(TAG_BIT_STRING, "signatureValue is not a BIT STRING")?;

    // A BIT STRING's first content octet is the count of unused trailing bits; a
    // signature occupies whole octets, so it must be zero.
    let (&unused_bits, signature) = signature_value
        .split_first()
        .ok_or(ChainError::Malformed("empty signatureValue BIT STRING"))?;
    if unused_bits != 0 {
        return Err(ChainError::Malformed("signatureValue BIT STRING has unused bits"));
    }

    // TBSCertificate ::= SEQUENCE { version [0]?, serialNumber, signature, ... }.
    // Reach the inner `signature` AlgorithmIdentifier to cross-check it against the
    // outer signatureAlgorithm (RFC 5280 §4.1.1.2).
    let mut tbs = Der::new(tbs_contents);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version [0] EXPLICIT — optional
    }
    tbs.read_tagged(TAG_INTEGER, "serialNumber is not an INTEGER")?;
    let (inner_alg_tag, _inner_alg_contents, inner_alg_raw) = tbs.read_tlv_raw()?;
    if inner_alg_tag != TAG_SEQUENCE {
        return Err(ChainError::Malformed("tbsCertificate.signature is not a SEQUENCE"));
    }

    // RFC 5280 §4.1.1.2: the two AlgorithmIdentifiers MUST be identical.
    if outer_alg_raw != inner_alg_raw {
        return Err(ChainError::AlgorithmMismatch(
            "signatureAlgorithm differs from tbsCertificate.signature",
        ));
    }

    // AlgorithmIdentifier ::= SEQUENCE { algorithm OID, parameters ANY OPTIONAL }.
    let mut outer_alg = Der::new(_outer_alg_contents);
    let oid = outer_alg.read_tagged(TAG_OID, "signatureAlgorithm has no OID")?;
    let algorithm = classify_signature_algorithm(oid)?;

    verify(algorithm, issuer_public_key, tbs_raw, signature)
}

/// Map a `signatureAlgorithm` OID to the [`ChainSignatureAlgorithm`] this slice
/// verifies, or [`ChainError::UnsupportedSignatureAlgorithm`] for anything else.
fn classify_signature_algorithm(oid: &[u8]) -> Result<ChainSignatureAlgorithm, ChainError> {
    if oid == OID_ECDSA_WITH_SHA256 {
        Ok(ChainSignatureAlgorithm::EcdsaWithSha256)
    } else if oid == OID_ECDSA_WITH_SHA384 {
        Ok(ChainSignatureAlgorithm::EcdsaWithSha384)
    } else if oid == OID_ECDSA_WITH_SHA512 {
        Ok(ChainSignatureAlgorithm::EcdsaWithSha512)
    } else if oid == OID_ED25519 {
        Ok(ChainSignatureAlgorithm::Ed25519)
    } else {
        Err(ChainError::UnsupportedSignatureAlgorithm)
    }
}

/// Dispatch to the matching [`tls_cert_verify`](super::tls_cert_verify) primitive,
/// pairing each ECDSA algorithm with its canonical issuer-key curve. A non-canonical
/// pairing (or an Ed25519 signature with a non-Ed25519 key) is
/// [`ChainError::AlgorithmMismatch`]; any verifier failure — bad point, malformed
/// signature, or a signature that does not check out — collapses to
/// [`ChainError::BadSignature`], since from the chain's perspective the link simply did
/// not verify.
fn verify(
    algorithm: ChainSignatureAlgorithm,
    issuer_public_key: &ServerPublicKey,
    tbs_raw: &[u8],
    signature: &[u8],
) -> Result<(), ChainError> {
    let key = &issuer_public_key.key_material;
    let result = match (algorithm, issuer_public_key.algorithm) {
        (ChainSignatureAlgorithm::EcdsaWithSha256, SpkiAlgorithm::EcdsaP256) => {
            ecdsa_p256_sha256_verify(key, tbs_raw, signature)
        }
        (ChainSignatureAlgorithm::EcdsaWithSha384, SpkiAlgorithm::EcdsaP384) => {
            ecdsa_p384_sha384_verify(key, tbs_raw, signature)
        }
        (ChainSignatureAlgorithm::EcdsaWithSha512, SpkiAlgorithm::EcdsaP521) => {
            ecdsa_p521_sha512_verify(key, tbs_raw, signature)
        }
        (ChainSignatureAlgorithm::Ed25519, SpkiAlgorithm::Ed25519) => {
            ed25519_verify(key, tbs_raw, signature)
        }
        _ => {
            return Err(ChainError::AlgorithmMismatch(
                "issuer key type does not match signature algorithm",
            ));
        }
    };
    result.map_err(|e| match e {
        // Any verifier outcome that is not "verified" means this issuer did not sign
        // this certificate; the chain cares only about that single verdict.
        CertVerifyError::MalformedPublicKey
        | CertVerifyError::MalformedSignature
        | CertVerifyError::BadSignature
        | CertVerifyError::UnsupportedScheme(_) => ChainError::BadSignature,
    })
}

/// Why walking a certificate chain's internal signatures failed (RFC 5280 §4.1.1.3,
/// RFC 8446 §4.4.2). Each variant pinpoints the certificate — by its position in the
/// server's `certificate_list` — at which the walk broke, so the caller can log or
/// alert precisely which link is bad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity
    /// certificate first, so there is nothing to walk — a malformed `Certificate`
    /// message.
    EmptyChain,
    /// The candidate issuer certificate (one step up from the certificate it should
    /// have signed) did not yield a usable public key: its `SubjectPublicKeyInfo`
    /// failed to decode or named an algorithm this build cannot extract.
    IssuerKey {
        /// Position, in the `certificate_list`, of the issuer certificate whose
        /// `SubjectPublicKeyInfo` failed to extract.
        issuer_index: usize,
        /// The underlying `SubjectPublicKeyInfo`-extraction failure.
        error: SpkiError,
    },
    /// The certificate at `subject_index` is not signed by the next certificate in the
    /// list (its candidate issuer): the single-link check
    /// ([`verify_certificate_signature`]) failed.
    Link {
        /// Position, in the `certificate_list`, of the certificate whose signature did
        /// not verify under its candidate issuer's key.
        subject_index: usize,
        /// The underlying single-link verification failure.
        error: ChainError,
    },
}

impl core::fmt::Display for ChainWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::IssuerKey { issuer_index, error } => {
                write!(f, "issuer certificate #{issuer_index}: {error}")
            }
            Self::Link { subject_index, error } => {
                write!(f, "certificate #{subject_index} not signed by its issuer: {error}")
            }
        }
    }
}

impl std::error::Error for ChainWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IssuerKey { error, .. } => Some(error),
            Self::Link { error, .. } => Some(error),
            Self::EmptyChain => None,
        }
    }
}

/// Verify that every certificate in a server-presented chain is signed by the next one
/// up (RFC 5280 §4.1.1.3, RFC 8446 §4.4.2).
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message —
/// the end-entity certificate first, then each issuing intermediate. For every adjacent
/// pair `(chain[i], chain[i + 1])` this extracts `chain[i + 1]`'s public key with
/// [`extract_end_entity_public_key`](super::x509_spki::extract_end_entity_public_key)
/// (the extractor reads any certificate's `SubjectPublicKeyInfo`, not only an
/// end-entity's) and checks that it produced `chain[i]`'s signature with
/// [`verify_certificate_signature`]. The last certificate has no successor in the list —
/// its issuer is a trust anchor outside the chain — so its own signature is not checked
/// here.
///
/// A single-element chain has no internal links and verifies vacuously: the presented
/// end-entity certificate is self-consistent by definition, and binding it to a trust
/// anchor is a separate concern. An empty chain is [`ChainWalkError::EmptyChain`].
///
/// This confirms only that the presented certificates form a self-consistent signature
/// chain. It does **not** terminate the chain at a trusted root (RFC 5280 §6.1),
/// match `issuer`/`subject` distinguished names (name chaining, §4.1.2.4/§4.1.2.6), or
/// honour `basicConstraints`/`keyUsage` (§4.2.1.9/§4.2.1.3) — those are later slices,
/// layered above this walk exactly as this walk layers above the single-link check.
///
/// # Errors
///
/// - [`ChainWalkError::EmptyChain`] if `chain` is empty.
/// - [`ChainWalkError::IssuerKey`] if a certificate's `SubjectPublicKeyInfo` cannot be
///   extracted, naming that certificate's position.
/// - [`ChainWalkError::Link`] if a certificate's signature does not verify under the
///   next certificate's key, naming that certificate's position.
pub fn verify_chain_signatures(chain: &[&[u8]]) -> Result<(), ChainWalkError> {
    if chain.is_empty() {
        return Err(ChainWalkError::EmptyChain);
    }

    // Each certificate but the last (whose issuer is a trust anchor not in the list)
    // must be signed by the certificate one step up.
    for subject_index in 0..chain.len() - 1 {
        let issuer_index = subject_index + 1;
        let issuer_key = extract_end_entity_public_key(chain[issuer_index])
            .map_err(|error| ChainWalkError::IssuerKey { issuer_index, error })?;
        verify_certificate_signature(chain[subject_index], &issuer_key)
            .map_err(|error| ChainWalkError::Link { subject_index, error })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DER construction helpers (test-only certificate builder) ───────────

    /// Encode a DER definite length: short form under 128, long form otherwise.
    fn encode_len(len: usize, out: &mut Vec<u8>) {
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

    /// Build a `tag ‖ length ‖ contents` TLV.
    fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        encode_len(contents.len(), &mut out);
        out.extend_from_slice(contents);
        out
    }

    /// Concatenate several DER blobs.
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// An `AlgorithmIdentifier ::= SEQUENCE { OID }` (no parameters — the encoding RFC
    /// 5758 §3.2 and RFC 8410 §3 use for the ECDSA and Ed25519 signature algorithms).
    fn alg_id(oid: &[u8]) -> Vec<u8> {
        tlv(TAG_SEQUENCE, &tlv(TAG_OID, oid))
    }

    /// A placeholder `SubjectPublicKeyInfo` for the *subject* of the test certificate.
    /// The signature check never inspects it, so its shape is irrelevant here.
    fn placeholder_spki() -> Vec<u8> {
        let alg = alg_id(&[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01]); // id-ecPublicKey
        let bit = tlv(TAG_BIT_STRING, &cat(&[&[0x00], &[0x04; 65]]));
        tlv(TAG_SEQUENCE, &cat(&[&alg, &bit]))
    }

    /// Assemble a `tbsCertificate` whose `signature` AlgorithmIdentifier is `sig_alg`.
    /// Every other field is a structurally valid placeholder the signature covers but
    /// the verifier does not interpret.
    fn tbs_certificate(sig_alg: &[u8]) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])); // [0] { v3 }
        let serial = tlv(TAG_INTEGER, &[0x13, 0x37]);
        let issuer = tlv(TAG_SEQUENCE, &[]);
        let validity = tlv(TAG_SEQUENCE, &[]);
        let subject = tlv(TAG_SEQUENCE, &[]);
        let spki = placeholder_spki();
        tlv(
            TAG_SEQUENCE,
            &cat(&[&version, &serial, sig_alg, &issuer, &validity, &subject, &spki]),
        )
    }

    /// Wrap a `tbsCertificate`, its `signatureAlgorithm`, and a `signatureValue` into a
    /// `Certificate`.
    fn certificate(tbs: &[u8], sig_alg: &[u8], signature: &[u8]) -> Vec<u8> {
        let mut bits = vec![0x00]; // zero unused bits
        bits.extend_from_slice(signature);
        let sig_value = tlv(TAG_BIT_STRING, &bits);
        tlv(TAG_SEQUENCE, &cat(&[tbs, sig_alg, &sig_value]))
    }

    /// Extract a `ServerPublicKey` from a real SEC1/Ed25519 public key by wrapping it in
    /// a throwaway certificate the SPKI extractor understands.
    fn issuer_key_from(spki_alg: &[u8], curve_oid: Option<&[u8]>, key_octets: &[u8]) -> ServerPublicKey {
        let alg = match curve_oid {
            Some(curve) => tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_OID, spki_alg), &tlv(TAG_OID, curve)])),
            None => tlv(TAG_SEQUENCE, &tlv(TAG_OID, spki_alg)),
        };
        let bit = tlv(TAG_BIT_STRING, &cat(&[&[0x00], key_octets]));
        let spki = tlv(TAG_SEQUENCE, &cat(&[&alg, &bit]));
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[
                &tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])),
                &tlv(TAG_INTEGER, &[0x01]),
                &alg_id(OID_ECDSA_WITH_SHA256),
                &tlv(TAG_SEQUENCE, &[]),
                &tlv(TAG_SEQUENCE, &[]),
                &tlv(TAG_SEQUENCE, &[]),
                &spki,
            ]),
        );
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), &[0xDE, 0xAD]);
        extract_end_entity_public_key(&cert).expect("throwaway issuer SPKI extracts")
    }

    /// `id-ecPublicKey` (1.2.840.10045.2.1) and the P-256 named curve OID.
    const OID_EC_PUBLIC_KEY: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
    const OID_SECP256R1: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
    /// `id-Ed25519` reused as the SPKI key-algorithm OID.
    const OID_ED25519_KEY: &[u8] = &[0x2B, 0x65, 0x70];

    // ── ECDSA P-256 / SHA-256 happy path ───────────────────────────────────

    #[test]
    fn verifies_a_p256_signed_certificate() {
        use p256::ecdsa::signature::Signer;
        use p256::ecdsa::{Signature, SigningKey};

        let issuer_signing = SigningKey::from_slice(&[0x42; 32]).expect("valid P-256 scalar");
        let issuer_point = issuer_signing.verifying_key().to_encoded_point(false);
        let issuer_key = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), issuer_point.as_bytes());

        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        // The issuer signs the raw tbsCertificate DER (ECDSA SHA-256-hashes internally).
        let signature: Signature = issuer_signing.sign(&tbs);
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), signature.to_der().as_bytes());

        verify_certificate_signature(&cert, &issuer_key)
            .expect("the issuer key verifies the certificate it signed");
    }

    // ── Ed25519 happy path ─────────────────────────────────────────────────

    #[test]
    fn verifies_an_ed25519_signed_certificate() {
        use ed25519_dalek::{Signer, SigningKey};

        let issuer_signing = SigningKey::from_bytes(&[0x24; 32]);
        let issuer_public = issuer_signing.verifying_key().to_bytes();
        let issuer_key = issuer_key_from(OID_ED25519_KEY, None, &issuer_public);

        let tbs = tbs_certificate(&alg_id(OID_ED25519));
        let signature = issuer_signing.sign(&tbs);
        let cert = certificate(&tbs, &alg_id(OID_ED25519), &signature.to_bytes());

        verify_certificate_signature(&cert, &issuer_key)
            .expect("the Ed25519 issuer key verifies the certificate it signed");
    }

    // ── Tampered TBS / wrong issuer → BadSignature ─────────────────────────

    #[test]
    fn rejects_a_certificate_signed_by_a_different_key() {
        use p256::ecdsa::signature::Signer;
        use p256::ecdsa::{Signature, SigningKey};

        let real_signer = SigningKey::from_slice(&[0x01; 32]).expect("scalar");
        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        let signature: Signature = real_signer.sign(&tbs);
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), signature.to_der().as_bytes());

        // A different key must not verify the signature.
        let other = SigningKey::from_slice(&[0x02; 32]).expect("scalar");
        let other_point = other.verifying_key().to_encoded_point(false);
        let wrong_issuer = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), other_point.as_bytes());

        assert_eq!(
            verify_certificate_signature(&cert, &wrong_issuer),
            Err(ChainError::BadSignature),
        );
    }

    #[test]
    fn rejects_a_tampered_tbs_certificate() {
        use p256::ecdsa::signature::Signer;
        use p256::ecdsa::{Signature, SigningKey};

        let signing = SigningKey::from_slice(&[0x55; 32]).expect("scalar");
        let point = signing.verifying_key().to_encoded_point(false);
        let issuer_key = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), point.as_bytes());

        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        let signature: Signature = signing.sign(&tbs);
        let mut cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), signature.to_der().as_bytes());

        // Flip a byte inside the serialNumber (well within the tbsCertificate) so the
        // signed content no longer matches the signature.
        let serial_byte = cert
            .iter()
            .position(|&b| b == 0x37)
            .expect("serial marker present");
        cert[serial_byte] ^= 0xFF;

        assert_eq!(
            verify_certificate_signature(&cert, &issuer_key),
            Err(ChainError::BadSignature),
        );
    }

    // ── Algorithm cross-check and key-type binding ─────────────────────────

    #[test]
    fn rejects_outer_inner_algorithm_disagreement() {
        // Inner tbsCertificate.signature says SHA-256, outer signatureAlgorithm says
        // SHA-384 — RFC 5280 §4.1.1.2 forbids the mismatch.
        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA384), &[0x30, 0x00]);
        let issuer = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), &[0x04; 65]);
        assert_eq!(
            verify_certificate_signature(&cert, &issuer),
            Err(ChainError::AlgorithmMismatch(
                "signatureAlgorithm differs from tbsCertificate.signature",
            )),
        );
    }

    #[test]
    fn rejects_issuer_key_type_that_does_not_match_signature_algorithm() {
        use p256::ecdsa::signature::Signer;
        use p256::ecdsa::{Signature, SigningKey};

        // A validly ECDSA-signed certificate, but the caller offers an Ed25519 issuer
        // key — the pairing is impossible, so it must be an AlgorithmMismatch, not a
        // bare BadSignature.
        let signing = SigningKey::from_slice(&[0x77; 32]).expect("scalar");
        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        let signature: Signature = signing.sign(&tbs);
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), signature.to_der().as_bytes());

        let ed_issuer = issuer_key_from(OID_ED25519_KEY, None, &[0x11; 32]);
        assert_eq!(
            verify_certificate_signature(&cert, &ed_issuer),
            Err(ChainError::AlgorithmMismatch(
                "issuer key type does not match signature algorithm",
            )),
        );
    }

    #[test]
    fn rejects_non_canonical_curve_digest_pairing() {
        // ecdsa-with-SHA256 declared, but a P-384 issuer key offered: the coupled
        // verifiers do not cover P-384/SHA-256, so it is rejected fail-closed.
        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), &[0x30, 0x00]);
        let p384_oid: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x22];
        let issuer = issuer_key_from(OID_EC_PUBLIC_KEY, Some(p384_oid), &[0x04; 97]);
        assert_eq!(
            verify_certificate_signature(&cert, &issuer),
            Err(ChainError::AlgorithmMismatch(
                "issuer key type does not match signature algorithm",
            )),
        );
    }

    // ── Unsupported / deferred algorithms ──────────────────────────────────

    #[test]
    fn rejects_an_rsa_signature_as_unsupported() {
        // sha256WithRSAEncryption (1.2.840.113549.1.1.11): a real, common signature
        // algorithm this slice defers until a PKCS#1 v1.5 verifier exists.
        let rsa_oid: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B];
        let rsa_alg = tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_OID, rsa_oid), &tlv(0x05, &[])]));
        let tbs = tbs_certificate(&rsa_alg);
        let cert = certificate(&tbs, &rsa_alg, &[0xAB; 256]);
        let issuer = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), &[0x04; 65]);
        assert_eq!(
            verify_certificate_signature(&cert, &issuer),
            Err(ChainError::UnsupportedSignatureAlgorithm),
        );
    }

    // ── Malformed DER ──────────────────────────────────────────────────────

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        let issuer = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), &[0x04; 65]);
        assert!(matches!(
            verify_certificate_signature(&not_a_cert, &issuer),
            Err(ChainError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_certificate() {
        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), &[0x30, 0x00]);
        let issuer = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), &[0x04; 65]);
        assert!(matches!(
            verify_certificate_signature(&cert[..cert.len() - 3], &issuer),
            Err(ChainError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_signature_value_with_unused_bits() {
        let tbs = tbs_certificate(&alg_id(OID_ECDSA_WITH_SHA256));
        let sig_alg = alg_id(OID_ECDSA_WITH_SHA256);
        // Hand-build a Certificate whose signatureValue BIT STRING claims 4 unused bits.
        let mut bad_bits = vec![0x04];
        bad_bits.extend_from_slice(&[0x30, 0x00]);
        let sig_value = tlv(TAG_BIT_STRING, &bad_bits);
        let cert = tlv(TAG_SEQUENCE, &cat(&[&tbs, &sig_alg, &sig_value]));
        let issuer = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), &[0x04; 65]);
        assert_eq!(
            verify_certificate_signature(&cert, &issuer),
            Err(ChainError::Malformed("signatureValue BIT STRING has unused bits")),
        );
    }

    #[test]
    fn parses_a_v1_certificate_without_the_version_field() {
        use p256::ecdsa::signature::Signer;
        use p256::ecdsa::{Signature, SigningKey};

        let signing = SigningKey::from_slice(&[0x66; 32]).expect("scalar");
        let point = signing.verifying_key().to_encoded_point(false);
        let issuer_key = issuer_key_from(OID_EC_PUBLIC_KEY, Some(OID_SECP256R1), point.as_bytes());

        // A v1 tbsCertificate omits the [0] version field entirely.
        let serial = tlv(TAG_INTEGER, &[0x0A]);
        let sig_alg = alg_id(OID_ECDSA_WITH_SHA256);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[
                &serial,
                &sig_alg,
                &tlv(TAG_SEQUENCE, &[]),
                &tlv(TAG_SEQUENCE, &[]),
                &tlv(TAG_SEQUENCE, &[]),
                &placeholder_spki(),
            ]),
        );
        let signature: Signature = signing.sign(&tbs);
        let cert = certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), signature.to_der().as_bytes());

        verify_certificate_signature(&cert, &issuer_key).expect("v1 certificate verifies");
    }

    // ── Chain walk (slice 67) ──────────────────────────────────────────────

    /// Build a certificate whose own `subjectPublicKeyInfo` carries the SEC1 P-256 point
    /// `subject_point` and whose `signatureValue` is `issuer_signing`'s ECDSA-with-SHA256
    /// signature over the tbsCertificate. The subject SPKI makes the certificate usable
    /// as an *issuer* for the certificate one link below it; the signature makes it a
    /// *subject* verifiable under the certificate one link above.
    fn p256_cert(subject_point: &[u8], issuer_signing: &p256::ecdsa::SigningKey) -> Vec<u8> {
        use p256::ecdsa::Signature;
        use p256::ecdsa::signature::Signer;

        let spki_alg =
            tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_OID, OID_EC_PUBLIC_KEY), &tlv(TAG_OID, OID_SECP256R1)]));
        let spki_bits = tlv(TAG_BIT_STRING, &cat(&[&[0x00], subject_point]));
        let spki = tlv(TAG_SEQUENCE, &cat(&[&spki_alg, &spki_bits]));

        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02]));
        let serial = tlv(TAG_INTEGER, &[0x2A]);
        let sig_alg = alg_id(OID_ECDSA_WITH_SHA256);
        let empty = tlv(TAG_SEQUENCE, &[]);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[&version, &serial, &sig_alg, &empty, &empty, &empty, &spki]),
        );
        let signature: Signature = issuer_signing.sign(&tbs);
        certificate(&tbs, &alg_id(OID_ECDSA_WITH_SHA256), signature.to_der().as_bytes())
    }

    /// A P-256 signing key derived from a one-byte seed, plus its SEC1 public point.
    fn keypair(seed: u8) -> (p256::ecdsa::SigningKey, Vec<u8>) {
        let signing = p256::ecdsa::SigningKey::from_slice(&[seed; 32]).expect("valid P-256 scalar");
        let point = signing.verifying_key().to_encoded_point(false).as_bytes().to_vec();
        (signing, point)
    }

    #[test]
    fn verifies_a_two_certificate_chain() {
        // root signs the intermediate (root is NOT in the chain); the intermediate signs
        // the leaf. Chain = [leaf, intermediate]; the walk checks the leaf under the
        // intermediate's key and stops (the intermediate's own issuer is a trust anchor).
        let (root, _) = keypair(0x01);
        let (inter, inter_point) = keypair(0x02);
        let (_, leaf_point) = keypair(0x03);

        let intermediate = p256_cert(&inter_point, &root);
        let leaf = p256_cert(&leaf_point, &inter);

        verify_chain_signatures(&[&leaf, &intermediate]).expect("each link is signed by the next");
    }

    #[test]
    fn verifies_a_three_certificate_chain() {
        let (root, _) = keypair(0x01);
        let (top, top_point) = keypair(0x02);
        let (inter, inter_point) = keypair(0x03);
        let (_, leaf_point) = keypair(0x04);

        let top_cert = p256_cert(&top_point, &root);
        let intermediate = p256_cert(&inter_point, &top);
        let leaf = p256_cert(&leaf_point, &inter);

        verify_chain_signatures(&[&leaf, &intermediate, &top_cert])
            .expect("every internal link verifies");
    }

    #[test]
    fn rejects_a_leaf_not_signed_by_its_issuer() {
        // The leaf is signed by an unrelated key, not the intermediate's.
        let (root, _) = keypair(0x01);
        let (_, inter_point) = keypair(0x02);
        let (impostor, _) = keypair(0x09);
        let (_, leaf_point) = keypair(0x03);

        let intermediate = p256_cert(&inter_point, &root);
        let leaf = p256_cert(&leaf_point, &impostor);

        assert_eq!(
            verify_chain_signatures(&[&leaf, &intermediate]),
            Err(ChainWalkError::Link { subject_index: 0, error: ChainError::BadSignature }),
        );
    }

    #[test]
    fn reports_the_broken_link_position_in_the_middle() {
        // leaf←inter verifies, but inter is signed by an impostor rather than `top`, so
        // the walk breaks at subject_index 1 (the intermediate).
        let (root, _) = keypair(0x01);
        let (_, top_point) = keypair(0x02);
        let (inter, inter_point) = keypair(0x03);
        let (impostor, _) = keypair(0x09);
        let (_, leaf_point) = keypair(0x04);

        let top_cert = p256_cert(&top_point, &root);
        let intermediate = p256_cert(&inter_point, &impostor);
        let leaf = p256_cert(&leaf_point, &inter);

        assert_eq!(
            verify_chain_signatures(&[&leaf, &intermediate, &top_cert]),
            Err(ChainWalkError::Link { subject_index: 1, error: ChainError::BadSignature }),
        );
    }

    #[test]
    fn rejects_an_issuer_with_undecodable_spki() {
        // The candidate issuer is not a certificate at all, so its public key cannot be
        // extracted — reported against the issuer's position, not as a bad signature.
        let (inter, _) = keypair(0x02);
        let (_, leaf_point) = keypair(0x03);
        let leaf = p256_cert(&leaf_point, &inter);
        let garbage = tlv(TAG_INTEGER, &[0x01]);

        assert!(matches!(
            verify_chain_signatures(&[&leaf, &garbage]),
            Err(ChainWalkError::IssuerKey { issuer_index: 1, error: SpkiError::Malformed(_) }),
        ));
    }

    #[test]
    fn rejects_an_empty_chain() {
        assert_eq!(verify_chain_signatures(&[]), Err(ChainWalkError::EmptyChain));
    }

    #[test]
    fn accepts_a_single_certificate_vacuously() {
        // One certificate has no internal links; binding it to a trust anchor is a
        // separate check, so the walk succeeds vacuously.
        let (root, _) = keypair(0x01);
        let (_, leaf_point) = keypair(0x03);
        let leaf = p256_cert(&leaf_point, &root);

        verify_chain_signatures(&[&leaf]).expect("a lone certificate has no links to break");
    }
}
