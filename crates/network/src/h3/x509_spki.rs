//! X.509 end-entity `SubjectPublicKeyInfo` extraction (RFC 5280 §4.1, RFC 5480,
//! RFC 8410) — slice 60 of the HTTP/3 sprint.
//!
//! [`tls_cert_verify`](super::tls_cert_verify) verifies the server's
//! `CertificateVerify` signature — the step that authenticates the peer — but it
//! takes the *decoded* public key directly and states plainly that "extracting the
//! public key from the end-entity certificate's `SubjectPublicKeyInfo` is the
//! caller's job (X.509 parsing is delegated)". This module is that missing caller:
//! it walks the DER of the end-entity certificate the server sent in its
//! `Certificate` message (RFC 8446 §4.4.2, the first entry of
//! [`super::tls_message::Certificate::certificate_list`]) down to its
//! `subjectPublicKeyInfo` and hands back the key material in exactly the shape each
//! [`tls_cert_verify`](super::tls_cert_verify) verifier expects, so the two modules
//! compose into a full server-signature check.
//!
//! ## What it extracts
//!
//! A certificate is
//!
//! ```text
//! Certificate ::= SEQUENCE {
//!     tbsCertificate       TBSCertificate,
//!     signatureAlgorithm   AlgorithmIdentifier,
//!     signatureValue       BIT STRING }
//!
//! TBSCertificate ::= SEQUENCE {
//!     version         [0] EXPLICIT Version DEFAULT v1,   -- context tag 0xA0, optional
//!     serialNumber        CertificateSerialNumber,       -- INTEGER
//!     signature           AlgorithmIdentifier,           -- SEQUENCE
//!     issuer              Name,                          -- SEQUENCE
//!     validity            Validity,                      -- SEQUENCE
//!     subject             Name,                          -- SEQUENCE
//!     subjectPublicKeyInfo SubjectPublicKeyInfo,         -- SEQUENCE  <- the target
//!     ... }                                              -- optional fields ignored
//!
//! SubjectPublicKeyInfo ::= SEQUENCE {
//!     algorithm        AlgorithmIdentifier,              -- SEQUENCE { OID, params }
//!     subjectPublicKey BIT STRING }
//! ```
//!
//! [`extract_end_entity_public_key`] navigates that structure by field order —
//! skipping the optional `[0]` version and the five fields before
//! `subjectPublicKeyInfo` without interpreting them — reads the algorithm OID (and,
//! for the NIST curves, the named-curve OID in the parameters), and returns the
//! `subjectPublicKey` BIT STRING contents as the key material. It reads *only* what
//! the signature check needs: it does not parse the issuer, validity dates, or
//! extensions, and it does not walk the chain to a trust anchor — that path
//! validation is a separate concern (and, like the `CertificateVerify` check the
//! two sibling slices leave to a caller, is wired above this module).
//!
//! ## Key-material shapes
//!
//! The returned [`ServerPublicKey::key_material`] is the raw `subjectPublicKey` BIT
//! STRING value (the leading unused-bits octet stripped), which is already the exact
//! encoding each verifier takes:
//!
//! - **ECDSA P-256/P-384/P-521** (`id-ecPublicKey` + a named-curve OID, RFC 5480):
//!   the SEC1 uncompressed EC point `0x04 ‖ X ‖ Y`, fed to
//!   [`ecdsa_p256_sha256_verify`](super::tls_cert_verify::ecdsa_p256_sha256_verify)
//!   and its P-384/P-521 siblings.
//! - **Ed25519** (`id-Ed25519`, RFC 8410 §4): the raw 32-octet point, fed to
//!   [`ed25519_verify`](super::tls_cert_verify::ed25519_verify).
//! - **RSA** (`rsaEncryption`, RFC 8017 / RFC 4055): the PKCS#1 DER `RSAPublicKey`
//!   (`SEQUENCE { modulus, publicExponent }`), fed to
//!   [`rsa_pss_sha256_verify`](super::tls_cert_verify::rsa_pss_sha256_verify) and
//!   its SHA-384/SHA-512 siblings.
//!
//! ## Scheme binding
//!
//! [`ServerPublicKey::accepts_scheme`] answers whether a `CertificateVerify`
//! `SignatureScheme` (RFC 8446 §4.2.3) is legal for the extracted key type — an
//! ECDSA P-256 key may only sign `ecdsa_secp256r1_sha256`, an RSA key only the
//! `rsa_pss_rsae_*` schemes, and so on — so the caller can reject a key/scheme
//! mismatch (a P-256 certificate presenting an RSA-PSS signature) before it hands
//! the material to [`verify_certificate_verify`](super::tls_cert_verify::verify_certificate_verify).
//!
//! ## Purity
//!
//! Pure DER parsing over a borrowed byte slice: no clock, no allocation beyond the
//! returned key bytes, no I/O.

use super::tls_cert_verify::signature_scheme;

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `OBJECT IDENTIFIER`.
const TAG_OID: u8 = 0x06;
/// The DER tag for `BIT STRING`.
const TAG_BIT_STRING: u8 = 0x03;
/// The DER tag for the optional `[0] EXPLICIT` `version` field of a
/// `TBSCertificate` (context class, constructed, tag number 0).
const TAG_CONTEXT_0: u8 = 0xA0;

/// `id-ecPublicKey` (1.2.840.10045.2.1, RFC 5480 §2.1.1) — the algorithm OID of an
/// elliptic-curve `SubjectPublicKeyInfo`. The named curve follows in the parameters.
const OID_EC_PUBLIC_KEY: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
/// `prime256v1` / `secp256r1` (1.2.840.10045.3.1.7, RFC 5480 §2.1.1.1) — NIST P-256.
const OID_SECP256R1: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
/// `secp384r1` (1.3.132.0.34, RFC 5480 §2.1.1.1) — NIST P-384.
const OID_SECP384R1: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x22];
/// `secp521r1` (1.3.132.0.35, RFC 5480 §2.1.1.1) — NIST P-521.
const OID_SECP521R1: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x23];
/// `id-Ed25519` (1.3.101.112, RFC 8410 §3) — Ed25519 (EdDSA over Curve25519).
const OID_ED25519: &[u8] = &[0x2B, 0x65, 0x70];
/// `rsaEncryption` (1.2.840.113549.1.1.1, RFC 8017 Appendix A) — an RSA key.
const OID_RSA_ENCRYPTION: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];

/// The public-key algorithm identified in a certificate's `SubjectPublicKeyInfo`,
/// mapped to the [`tls_cert_verify`](super::tls_cert_verify) verifier that consumes
/// its key material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpkiAlgorithm {
    /// `id-ecPublicKey` over `prime256v1` — an ECDSA P-256 key. Signs
    /// `ecdsa_secp256r1_sha256` only. Key material is the SEC1 EC point.
    EcdsaP256,
    /// `id-ecPublicKey` over `secp384r1` — an ECDSA P-384 key. Signs
    /// `ecdsa_secp384r1_sha384` only. Key material is the SEC1 EC point.
    EcdsaP384,
    /// `id-ecPublicKey` over `secp521r1` — an ECDSA P-521 key. Signs
    /// `ecdsa_secp521r1_sha512` only. Key material is the SEC1 EC point.
    EcdsaP521,
    /// `id-Ed25519` — an Ed25519 key. Signs `ed25519` only. Key material is the raw
    /// 32-octet point.
    Ed25519,
    /// `rsaEncryption` — an RSA key. Signs any of `rsa_pss_rsae_sha256/384/512`. Key
    /// material is the PKCS#1 DER `RSAPublicKey`.
    Rsa,
}

/// A server's end-entity public key, extracted from the certificate's
/// `SubjectPublicKeyInfo` and ready to feed to
/// [`verify_certificate_verify`](super::tls_cert_verify::verify_certificate_verify).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerPublicKey {
    /// The algorithm the key is for, selecting the verifier and the legal
    /// [`accepts_scheme`](ServerPublicKey::accepts_scheme) codepoints.
    pub algorithm: SpkiAlgorithm,
    /// The `subjectPublicKey` BIT STRING contents (unused-bits octet stripped): the
    /// SEC1 EC point for ECDSA, the raw 32-octet point for Ed25519, or the PKCS#1 DER
    /// `RSAPublicKey` for RSA — the exact encoding the matching verifier expects.
    pub key_material: Vec<u8>,
}

impl ServerPublicKey {
    /// Whether `scheme` (a `SignatureScheme` codepoint, RFC 8446 §4.2.3) is a legal
    /// `CertificateVerify` algorithm for this key's type.
    ///
    /// TLS 1.3 binds each ECDSA scheme to one curve and each Ed/RSA scheme to its key
    /// type, so an ECDSA P-256 key accepts only `ecdsa_secp256r1_sha256`, an RSA key
    /// accepts the three `rsa_pss_rsae_*` schemes, and so on. A caller checks this
    /// before [`verify_certificate_verify`](super::tls_cert_verify::verify_certificate_verify)
    /// to reject a key/scheme mismatch — a certificate whose key cannot have produced
    /// the presented signature — rather than leaking it to the verifier as a bare
    /// malformed-key or bad-signature error.
    #[must_use]
    pub fn accepts_scheme(&self, scheme: u16) -> bool {
        match self.algorithm {
            SpkiAlgorithm::EcdsaP256 => scheme == signature_scheme::ECDSA_SECP256R1_SHA256,
            SpkiAlgorithm::EcdsaP384 => scheme == signature_scheme::ECDSA_SECP384R1_SHA384,
            SpkiAlgorithm::EcdsaP521 => scheme == signature_scheme::ECDSA_SECP521R1_SHA512,
            SpkiAlgorithm::Ed25519 => scheme == signature_scheme::ED25519,
            SpkiAlgorithm::Rsa => matches!(
                scheme,
                signature_scheme::RSA_PSS_RSAE_SHA256
                    | signature_scheme::RSA_PSS_RSAE_SHA384
                    | signature_scheme::RSA_PSS_RSAE_SHA512
            ),
        }
    }
}

/// Why extracting the `SubjectPublicKeyInfo` from a certificate failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpkiError {
    /// The DER was truncated, mis-nested, or carried an unexpected tag where the
    /// certificate structure required a specific one. Carries a static hint naming
    /// the field that did not decode.
    Malformed(&'static str),
    /// The `subjectPublicKeyInfo` decoded but named an algorithm (or, for
    /// `id-ecPublicKey`, a named curve) this slice does not verify.
    UnsupportedAlgorithm,
}

impl core::fmt::Display for SpkiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
            Self::UnsupportedAlgorithm => f.write_str("unsupported SubjectPublicKeyInfo algorithm"),
        }
    }
}

impl std::error::Error for SpkiError {}

/// A minimal reader over a DER-encoded byte slice: it walks tag-length-value
/// triples left to right, enough to navigate a certificate to its
/// `subjectPublicKeyInfo`. Definite-length only (DER forbids the indefinite form).
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

    /// Read a DER definite length at the cursor (RFC 5280 uses X.690 DER): a short
    /// form (`0x00..=0x7f`) is the length itself; a long form (`0x81..`) gives the
    /// count of big-endian length octets that follow. The indefinite form (`0x80`)
    /// and counts wider than four octets are rejected.
    fn read_length(&mut self) -> Result<usize, SpkiError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(SpkiError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            // 0 = the indefinite form (forbidden in DER); >4 exceeds any length this
            // parser can meaningfully handle for a certificate field.
            return Err(SpkiError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(SpkiError::Malformed("truncated long-form length"));
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
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), SpkiError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(SpkiError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(SpkiError::Malformed("truncated: content shorter than its length"));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what`
    /// names the field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], SpkiError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(SpkiError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Extract the end-entity certificate's public key from its DER (RFC 5280 §4.1).
///
/// `cert_der` is one X.509 certificate — the `cert_data` of the first
/// [`CertificateEntry`](super::tls_message::CertificateEntry) of the server's
/// `Certificate` message, which by RFC 8446 §4.4.2 is the end-entity certificate.
/// The result carries the algorithm and the key material in the exact shape
/// [`verify_certificate_verify`](super::tls_cert_verify::verify_certificate_verify)
/// consumes.
///
/// This reads only the fields needed to reach `subjectPublicKeyInfo`; it validates
/// neither the certificate's validity dates nor its chain to a trust anchor.
///
/// # Errors
///
/// [`SpkiError::Malformed`] if the DER is truncated or mis-structured, and
/// [`SpkiError::UnsupportedAlgorithm`] if the key names an algorithm — or an EC named
/// curve — outside the P-256/P-384/P-521, Ed25519, and RSA set this slice verifies.
pub fn extract_end_entity_public_key(cert_der: &[u8]) -> Result<ServerPublicKey, SpkiError> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE, "certificate is not a SEQUENCE")?;
    let mut certificate = Der::new(certificate);
    let tbs = certificate.read_tagged(TAG_SEQUENCE, "tbsCertificate is not a SEQUENCE")?;

    // TBSCertificate fields in order; only subjectPublicKeyInfo is interpreted.
    let mut tbs = Der::new(tbs);
    // version [0] EXPLICIT — optional (absent means v1); skip it if present.
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?;
    }
    tbs.read_tagged(TAG_INTEGER, "serialNumber is not an INTEGER")?;
    tbs.read_tagged(TAG_SEQUENCE, "signature AlgorithmIdentifier is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "issuer is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "validity is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "subject is not a SEQUENCE")?;
    let spki = tbs.read_tagged(TAG_SEQUENCE, "subjectPublicKeyInfo is not a SEQUENCE")?;

    // SubjectPublicKeyInfo ::= SEQUENCE { algorithm AlgorithmIdentifier, subjectPublicKey BIT STRING }.
    let mut spki = Der::new(spki);
    let alg_id = spki.read_tagged(TAG_SEQUENCE, "SPKI algorithm is not a SEQUENCE")?;
    let subject_public_key = spki.read_tagged(TAG_BIT_STRING, "subjectPublicKey is not a BIT STRING")?;

    // A BIT STRING's first content octet is the number of unused trailing bits; a
    // public key always occupies whole octets, so it must be zero.
    let (&unused_bits, key_material) = subject_public_key
        .split_first()
        .ok_or(SpkiError::Malformed("empty subjectPublicKey BIT STRING"))?;
    if unused_bits != 0 {
        return Err(SpkiError::Malformed("subjectPublicKey BIT STRING has unused bits"));
    }

    // AlgorithmIdentifier ::= SEQUENCE { algorithm OID, parameters ANY OPTIONAL }.
    let mut alg_id = Der::new(alg_id);
    let oid = alg_id.read_tagged(TAG_OID, "algorithm identifier has no OID")?;
    let algorithm = classify_algorithm(oid, &mut alg_id)?;

    Ok(ServerPublicKey { algorithm, key_material: key_material.to_vec() })
}

/// Map an `AlgorithmIdentifier` OID (and, for `id-ecPublicKey`, the named-curve OID
/// in `params`) to the [`SpkiAlgorithm`] this slice verifies.
fn classify_algorithm(oid: &[u8], params: &mut Der<'_>) -> Result<SpkiAlgorithm, SpkiError> {
    if oid == OID_EC_PUBLIC_KEY {
        // RFC 5480 §2.1.1: the parameters carry the named-curve OID.
        let curve = params.read_tagged(TAG_OID, "EC parameters have no named-curve OID")?;
        match curve {
            OID_SECP256R1 => Ok(SpkiAlgorithm::EcdsaP256),
            OID_SECP384R1 => Ok(SpkiAlgorithm::EcdsaP384),
            OID_SECP521R1 => Ok(SpkiAlgorithm::EcdsaP521),
            _ => Err(SpkiError::UnsupportedAlgorithm),
        }
    } else if oid == OID_ED25519 {
        // RFC 8410 §3: id-Ed25519 takes no parameters.
        Ok(SpkiAlgorithm::Ed25519)
    } else if oid == OID_RSA_ENCRYPTION {
        // RFC 4055: rsaEncryption's parameters are an (ignored) NULL.
        Ok(SpkiAlgorithm::Rsa)
    } else {
        Err(SpkiError::UnsupportedAlgorithm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::tls_cert_verify::{
        CertVerifyRole, certificate_verify_content, verify_certificate_verify,
    };

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

    /// An `AlgorithmIdentifier` for an EC key: `SEQUENCE { id-ecPublicKey, curveOid }`.
    fn ec_alg_id(curve_oid: &[u8]) -> Vec<u8> {
        let oid = tlv(TAG_OID, OID_EC_PUBLIC_KEY);
        let curve = tlv(TAG_OID, curve_oid);
        tlv(TAG_SEQUENCE, &cat(&[&oid, &curve]))
    }

    /// An `AlgorithmIdentifier` for an Ed25519 key: `SEQUENCE { id-Ed25519 }` (no params).
    fn ed25519_alg_id() -> Vec<u8> {
        tlv(TAG_SEQUENCE, &tlv(TAG_OID, OID_ED25519))
    }

    /// An `AlgorithmIdentifier` for an RSA key: `SEQUENCE { rsaEncryption, NULL }`.
    fn rsa_alg_id() -> Vec<u8> {
        let oid = tlv(TAG_OID, OID_RSA_ENCRYPTION);
        let null = tlv(0x05, &[]);
        tlv(TAG_SEQUENCE, &cat(&[&oid, &null]))
    }

    /// Wrap an `AlgorithmIdentifier` and raw key octets into a `SubjectPublicKeyInfo`.
    fn spki(alg_id: &[u8], key_octets: &[u8]) -> Vec<u8> {
        let mut bit_string = vec![0x00]; // zero unused bits
        bit_string.extend_from_slice(key_octets);
        let bit = tlv(TAG_BIT_STRING, &bit_string);
        tlv(TAG_SEQUENCE, &cat(&[alg_id, &bit]))
    }

    /// Wrap a `SubjectPublicKeyInfo` into a minimal but structurally valid v3
    /// certificate. Every field before the SPKI is a placeholder the extractor skips.
    fn cert_with_spki(spki: &[u8]) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])); // [0] { v3 = 2 }
        cert_body(&version, spki)
    }

    /// A v1 certificate (no `[0]` version field) around a SPKI.
    fn cert_v1_with_spki(spki: &[u8]) -> Vec<u8> {
        cert_body(&[], spki)
    }

    /// Assemble a certificate from an (optional) version prefix and the target SPKI.
    fn cert_body(version: &[u8], spki: &[u8]) -> Vec<u8> {
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, OID_EC_PUBLIC_KEY));
        let issuer = tlv(TAG_SEQUENCE, &[]);
        let validity = tlv(TAG_SEQUENCE, &[]);
        let subject = tlv(TAG_SEQUENCE, &[]);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[version, &serial, &sig_alg, &issuer, &validity, &subject, spki]),
        );
        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, OID_EC_PUBLIC_KEY));
        let signature = tlv(TAG_BIT_STRING, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    // ── ECDSA P-256: extraction + end-to-end compose with the verifier ─────

    #[test]
    fn extracts_p256_key_and_verifies_a_real_signature() {
        use p256::ecdsa::signature::Signer;
        use p256::ecdsa::{Signature, SigningKey};

        // A fixed signing key gives a reproducible SEC1 public point.
        let signing = SigningKey::from_slice(&[0x11; 32]).expect("valid P-256 scalar");
        let point = signing.verifying_key().to_encoded_point(false);
        let sec1 = point.as_bytes();

        let cert = cert_with_spki(&spki(&ec_alg_id(OID_SECP256R1), sec1));
        let key = extract_end_entity_public_key(&cert).expect("SPKI extracts");

        assert_eq!(key.algorithm, SpkiAlgorithm::EcdsaP256);
        assert_eq!(key.key_material, sec1);
        assert!(key.accepts_scheme(signature_scheme::ECDSA_SECP256R1_SHA256));

        // Compose with the verifier: sign the CertificateVerify content and check it
        // through the extracted key, proving the two modules interlock.
        let transcript = [0x5A; 32];
        let content = certificate_verify_content(CertVerifyRole::Server, &transcript);
        let signature: Signature = signing.sign(&content);
        let der = signature.to_der();
        verify_certificate_verify(
            signature_scheme::ECDSA_SECP256R1_SHA256,
            &key.key_material,
            CertVerifyRole::Server,
            &transcript,
            der.as_bytes(),
        )
        .expect("the extracted key verifies the signature it signed");
    }

    // ── Ed25519: extraction + end-to-end compose ───────────────────────────

    #[test]
    fn extracts_ed25519_key_and_verifies_a_real_signature() {
        use ed25519_dalek::{Signer, SigningKey};

        let signing = SigningKey::from_bytes(&[0x22; 32]);
        let public = signing.verifying_key().to_bytes();

        let cert = cert_with_spki(&spki(&ed25519_alg_id(), &public));
        let key = extract_end_entity_public_key(&cert).expect("SPKI extracts");

        assert_eq!(key.algorithm, SpkiAlgorithm::Ed25519);
        assert_eq!(key.key_material, public);
        assert!(key.accepts_scheme(signature_scheme::ED25519));

        let transcript = [0x7C; 32];
        let content = certificate_verify_content(CertVerifyRole::Server, &transcript);
        let signature = signing.sign(&content);
        verify_certificate_verify(
            signature_scheme::ED25519,
            &key.key_material,
            CertVerifyRole::Server,
            &transcript,
            &signature.to_bytes(),
        )
        .expect("the extracted Ed25519 key verifies the signature it signed");
    }

    // ── P-384 / P-521 named-curve mapping ──────────────────────────────────

    #[test]
    fn maps_the_p384_and_p521_named_curves() {
        // The key octets are opaque to the extractor; only the curve OID selects the
        // algorithm, so a placeholder point suffices to check the mapping.
        let p384 = cert_with_spki(&spki(&ec_alg_id(OID_SECP384R1), &[0x04; 97]));
        assert_eq!(
            extract_end_entity_public_key(&p384).expect("extracts").algorithm,
            SpkiAlgorithm::EcdsaP384,
        );

        let p521 = cert_with_spki(&spki(&ec_alg_id(OID_SECP521R1), &[0x04; 133]));
        assert_eq!(
            extract_end_entity_public_key(&p521).expect("extracts").algorithm,
            SpkiAlgorithm::EcdsaP521,
        );
    }

    // ── RSA: key material round-trips, no signing needed ───────────────────

    #[test]
    fn extracts_the_rsa_pkcs1_public_key_verbatim() {
        // A PKCS#1 RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }.
        let modulus = tlv(TAG_INTEGER, &[0x00, 0xC1, 0x00, 0x7F]);
        let exponent = tlv(TAG_INTEGER, &[0x01, 0x00, 0x01]);
        let rsa_public_key = tlv(TAG_SEQUENCE, &cat(&[&modulus, &exponent]));

        let cert = cert_with_spki(&spki(&rsa_alg_id(), &rsa_public_key));
        let key = extract_end_entity_public_key(&cert).expect("SPKI extracts");

        assert_eq!(key.algorithm, SpkiAlgorithm::Rsa);
        // The BIT STRING contents are handed back exactly — the PKCS#1 DER the RSA-PSS
        // verifier decodes.
        assert_eq!(key.key_material, rsa_public_key);
    }

    // ── v1 certificate (no explicit version) ───────────────────────────────

    #[test]
    fn parses_a_v1_certificate_without_the_version_field() {
        let cert = cert_v1_with_spki(&spki(&ed25519_alg_id(), &[0x33; 32]));
        let key = extract_end_entity_public_key(&cert).expect("v1 cert extracts");
        assert_eq!(key.algorithm, SpkiAlgorithm::Ed25519);
        assert_eq!(key.key_material, vec![0x33; 32]);
    }

    // ── accepts_scheme: the key/scheme binding matrix ──────────────────────

    #[test]
    fn accepts_scheme_binds_each_key_type_to_its_schemes() {
        let ec = ServerPublicKey { algorithm: SpkiAlgorithm::EcdsaP256, key_material: vec![0x04] };
        assert!(ec.accepts_scheme(signature_scheme::ECDSA_SECP256R1_SHA256));
        // Right key type, wrong curve: a P-256 key cannot sign the P-384 scheme.
        assert!(!ec.accepts_scheme(signature_scheme::ECDSA_SECP384R1_SHA384));
        // Wrong key type entirely.
        assert!(!ec.accepts_scheme(signature_scheme::RSA_PSS_RSAE_SHA256));
        assert!(!ec.accepts_scheme(signature_scheme::ED25519));

        let rsa = ServerPublicKey { algorithm: SpkiAlgorithm::Rsa, key_material: vec![0x30] };
        // An RSA key accepts all three PSS digests but no ECDSA/Ed scheme.
        assert!(rsa.accepts_scheme(signature_scheme::RSA_PSS_RSAE_SHA256));
        assert!(rsa.accepts_scheme(signature_scheme::RSA_PSS_RSAE_SHA384));
        assert!(rsa.accepts_scheme(signature_scheme::RSA_PSS_RSAE_SHA512));
        assert!(!rsa.accepts_scheme(signature_scheme::ECDSA_SECP256R1_SHA256));

        let ed = ServerPublicKey { algorithm: SpkiAlgorithm::Ed25519, key_material: vec![0; 32] };
        assert!(ed.accepts_scheme(signature_scheme::ED25519));
        assert!(!ed.accepts_scheme(signature_scheme::ED448));
    }

    // ── unsupported algorithms / curves ────────────────────────────────────

    #[test]
    fn rejects_an_unknown_algorithm_oid() {
        // A DSA-ish placeholder OID (1.2.840.10040.4.1) the extractor does not verify.
        let unknown_oid: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x38, 0x04, 0x01];
        let alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, unknown_oid));
        let cert = cert_with_spki(&spki(&alg, &[0xAA; 20]));
        assert_eq!(
            extract_end_entity_public_key(&cert),
            Err(SpkiError::UnsupportedAlgorithm),
        );
    }

    #[test]
    fn rejects_an_unknown_ec_named_curve() {
        // secp256k1 (1.3.132.0.10) — a real curve, but not one TLS 1.3 verifies here.
        let secp256k1: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x0A];
        let cert = cert_with_spki(&spki(&ec_alg_id(secp256k1), &[0x04; 65]));
        assert_eq!(
            extract_end_entity_public_key(&cert),
            Err(SpkiError::UnsupportedAlgorithm),
        );
    }

    // ── malformed DER ──────────────────────────────────────────────────────

    #[test]
    fn rejects_a_non_sequence_top_level() {
        // An INTEGER where the certificate SEQUENCE should be.
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            extract_end_entity_public_key(&not_a_cert),
            Err(SpkiError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_content() {
        let cert = cert_with_spki(&spki(&ed25519_alg_id(), &[0x44; 32]));
        // Lop off the final octets so an inner length overruns the buffer.
        let truncated = &cert[..cert.len() - 5];
        assert!(matches!(
            extract_end_entity_public_key(truncated),
            Err(SpkiError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_a_bit_string_with_unused_bits() {
        // Hand-build a SPKI whose BIT STRING claims 3 unused bits.
        let alg = ed25519_alg_id();
        let mut bad_bits = vec![0x03]; // 3 unused bits — illegal for a key
        bad_bits.extend_from_slice(&[0x55; 32]);
        let bit = tlv(TAG_BIT_STRING, &bad_bits);
        let spki = tlv(TAG_SEQUENCE, &cat(&[&alg, &bit]));
        let cert = cert_with_spki(&spki);
        assert_eq!(
            extract_end_entity_public_key(&cert),
            Err(SpkiError::Malformed("subjectPublicKey BIT STRING has unused bits")),
        );
    }

    #[test]
    fn rejects_an_empty_input() {
        assert!(matches!(
            extract_end_entity_public_key(&[]),
            Err(SpkiError::Malformed(_)),
        ));
    }

    // ── long-form length parsing ───────────────────────────────────────────

    #[test]
    fn parses_a_long_form_length() {
        // An RSA key large enough that its BIT STRING needs a two-octet length,
        // exercising the long-form path of read_length.
        let big_key = vec![0x7F; 260];
        let cert = cert_with_spki(&spki(&rsa_alg_id(), &big_key));
        let key = extract_end_entity_public_key(&cert).expect("long-form length parses");
        assert_eq!(key.algorithm, SpkiAlgorithm::Rsa);
        assert_eq!(key.key_material, big_key);
    }
}
