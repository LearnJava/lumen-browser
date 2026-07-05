//! TLS 1.3 `CertificateVerify` signature (RFC 8446 §4.4.3) — slice 19 of the
//! HTTP/3 sprint.
//!
//! `CertificateVerify` is the handshake message in which the certificate's
//! owner proves possession of the private key matching the end-entity
//! certificate it just sent: it is a digital signature, under the key in that
//! certificate, over the entire handshake transcript up to and including the
//! `Certificate` message. QUIC carries this message inside CRYPTO frames exactly
//! like TLS over TCP (RFC 9001 §4), so a QUIC client must verify the server's
//! `CertificateVerify` before it trusts the connection — it is the step that
//! authenticates the peer (the `Finished` MAC of [`super::tls_finished`] only
//! confirms key agreement, not identity).
//!
//! The signature is computed not over the bare transcript hash but over a
//! framed *content* (RFC 8446 §4.4.3):
//!
//! ```text
//! signed_content = octet 0x20 repeated 64 times
//!                ‖ context_string           // role-specific, see below
//!                ‖ 0x00                      // single separator byte
//!                ‖ Transcript-Hash(Handshake Context, Certificate)
//! ```
//!
//! The 64 leading `0x20` (space) octets and the terminating NUL exist so a
//! signature made for TLS 1.3 can never be mistaken for one made in an earlier
//! TLS version or a different protocol (the prefix is not valid input to those
//! signers). The context string is `"TLS 1.3, server CertificateVerify"` for a
//! signature the server makes and `"TLS 1.3, client CertificateVerify"` for one
//! a client makes, so a signature for one role cannot be replayed as the other.
//!
//! ## Scope
//!
//! - [`certificate_verify_content`] — the pure construction of the signed
//!   content from a role and an already-computed transcript hash. Scheme- and
//!   key-independent; the transcript hash comes from [`super::tls_schedule::
//!   transcript_hash`] over the messages [`super::tls_message`] encodes.
//! - [`signature_scheme`] — the `SignatureScheme` codepoints (RFC 8446 §4.2.3)
//!   a `CertificateVerify` may name in [`super::tls_message::CertificateVerify::
//!   algorithm`].
//! - [`ecdsa_p256_sha256_verify`] — the raw `ecdsa_secp256r1_sha256` (P-256 /
//!   SHA-256) signature verification primitive over an arbitrary message.
//! - [`verify_certificate_verify`] — the end-to-end check: build the signed
//!   content for the role and transcript, then verify the DER-encoded signature
//!   under the peer's public key for the named scheme.
//!
//! ## Deferred
//!
//! Only `ecdsa_secp256r1_sha256` (0x0403) is verified here — it is one of the
//! TLS 1.3 mandatory-to-implement `CertificateVerify` schemes (RFC 8446 §9.1)
//! and reuses the `p256` crate already in this crate (WebAuthn ES256), so this
//! slice adds no dependency. The other schemes named in [`signature_scheme`]
//! (`rsa_pss_rsae_sha256`, `ed25519`, the P-384/P-521 and RSA-PKCS1 variants)
//! return [`CertVerifyError::UnsupportedScheme`] until a later slice wires their
//! verifiers. Extracting the public key from the end-entity certificate's
//! `SubjectPublicKeyInfo` is the caller's job (X.509 parsing is delegated, see
//! [`super::tls_message`]); this module takes the SEC1-encoded EC point directly.

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};

/// The number of `0x20` (ASCII space) octets that prefix the signed content
/// (RFC 8446 §4.4.3).
pub const CONTENT_PADDING_LEN: usize = 64;

/// The context string a **server** signs into its `CertificateVerify`
/// (RFC 8446 §4.4.3).
pub const SERVER_CONTEXT: &[u8] = b"TLS 1.3, server CertificateVerify";

/// The context string a **client** signs into its `CertificateVerify`
/// (RFC 8446 §4.4.3), sent only when the server requested client authentication.
pub const CLIENT_CONTEXT: &[u8] = b"TLS 1.3, client CertificateVerify";

/// `SignatureScheme` codepoints (RFC 8446 §4.2.3) that may appear as a
/// `CertificateVerify` algorithm. Only [`signature_scheme::ECDSA_SECP256R1_SHA256`]
/// is verified by this slice; the rest are recognised for completeness and
/// rejected with [`CertVerifyError::UnsupportedScheme`].
pub mod signature_scheme {
    /// `ecdsa_secp256r1_sha256` — ECDSA over the NIST P-256 curve with SHA-256.
    pub const ECDSA_SECP256R1_SHA256: u16 = 0x0403;
    /// `ecdsa_secp384r1_sha384` — ECDSA over NIST P-384 with SHA-384.
    pub const ECDSA_SECP384R1_SHA384: u16 = 0x0503;
    /// `ecdsa_secp521r1_sha512` — ECDSA over NIST P-521 with SHA-512.
    pub const ECDSA_SECP521R1_SHA512: u16 = 0x0603;
    /// `rsa_pss_rsae_sha256` — RSASSA-PSS with an rsaEncryption key and SHA-256.
    pub const RSA_PSS_RSAE_SHA256: u16 = 0x0804;
    /// `rsa_pss_rsae_sha384` — RSASSA-PSS with an rsaEncryption key and SHA-384.
    pub const RSA_PSS_RSAE_SHA384: u16 = 0x0805;
    /// `rsa_pss_rsae_sha512` — RSASSA-PSS with an rsaEncryption key and SHA-512.
    pub const RSA_PSS_RSAE_SHA512: u16 = 0x0806;
    /// `ed25519` — EdDSA over Curve25519 (signs the message directly, no prehash).
    pub const ED25519: u16 = 0x0807;
    /// `ed448` — EdDSA over Curve448.
    pub const ED448: u16 = 0x0808;
    /// `rsa_pkcs1_sha256` — RSASSA-PKCS1-v1_5 with SHA-256 (legal only in
    /// certificates, never in a TLS 1.3 `CertificateVerify`, RFC 8446 §4.4.3).
    pub const RSA_PKCS1_SHA256: u16 = 0x0401;
    /// `rsa_pss_pss_sha256` — RSASSA-PSS with a PSS-only (RSASSA-PSS OID) key.
    pub const RSA_PSS_PSS_SHA256: u16 = 0x0809;
}

/// Which side signed the `CertificateVerify`, selecting the context string that
/// is bound into the signed content (RFC 8446 §4.4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertVerifyRole {
    /// The server's `CertificateVerify` — signed content uses [`SERVER_CONTEXT`].
    Server,
    /// The client's `CertificateVerify` — signed content uses [`CLIENT_CONTEXT`].
    Client,
}

impl CertVerifyRole {
    /// The context string this role signs into the content (RFC 8446 §4.4.3).
    #[must_use]
    pub fn context(self) -> &'static [u8] {
        match self {
            Self::Server => SERVER_CONTEXT,
            Self::Client => CLIENT_CONTEXT,
        }
    }
}

/// Why a `CertificateVerify` failed to verify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertVerifyError {
    /// The named `SignatureScheme` is recognised but not verified by this slice.
    UnsupportedScheme(u16),
    /// The supplied public key could not be decoded for the named scheme (e.g.
    /// a SEC1 point that is not on the P-256 curve).
    MalformedPublicKey,
    /// The signature bytes could not be decoded for the named scheme (e.g. an
    /// ill-formed DER ECDSA signature).
    MalformedSignature,
    /// The signature decoded but did not verify over the signed content — the
    /// peer does not hold the certificate's private key, or the transcript was
    /// altered. A fatal `decrypt_error` (RFC 8446 §4.4.3).
    BadSignature,
}

impl core::fmt::Display for CertVerifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedScheme(s) => write!(f, "unsupported CertificateVerify scheme 0x{s:04x}"),
            Self::MalformedPublicKey => f.write_str("malformed CertificateVerify public key"),
            Self::MalformedSignature => f.write_str("malformed CertificateVerify signature"),
            Self::BadSignature => f.write_str("CertificateVerify signature did not verify"),
        }
    }
}

impl std::error::Error for CertVerifyError {}

/// Build the content a `CertificateVerify` signature is computed over
/// (RFC 8446 §4.4.3): 64 `0x20` octets, the role's context string, a single
/// `0x00`, then the transcript hash.
///
/// `transcript_hash` is `Transcript-Hash(Handshake Context, Certificate)` — the
/// hash of every handshake message up to and including the `Certificate` that
/// carried the signing key (32 octets under SHA-256, but any length is accepted
/// and appended verbatim). The result is the exact byte string passed to the
/// signature algorithm's verifier.
#[must_use]
pub fn certificate_verify_content(role: CertVerifyRole, transcript_hash: &[u8]) -> Vec<u8> {
    let context = role.context();
    let mut content = Vec::with_capacity(CONTENT_PADDING_LEN + context.len() + 1 + transcript_hash.len());
    content.extend(std::iter::repeat_n(0x20u8, CONTENT_PADDING_LEN));
    content.extend_from_slice(context);
    content.push(0x00);
    content.extend_from_slice(transcript_hash);
    content
}

/// Verify an `ecdsa_secp256r1_sha256` signature (RFC 8446 §4.2.3): ECDSA over
/// NIST P-256 with SHA-256, the DER-encoded `(r, s)` form TLS carries.
///
/// `public_key_sec1` is the peer's public key as a SEC1-encoded EC point
/// (the uncompressed `0x04 ‖ X ‖ Y`, 65 octets, as it appears in the end-entity
/// certificate's `SubjectPublicKeyInfo`). `message` is the already-built signed
/// content (see [`certificate_verify_content`]); it is SHA-256-hashed internally
/// as ECDSA requires. `der_signature` is the ASN.1 DER `SEQUENCE { r, s }`.
///
/// # Errors
///
/// [`CertVerifyError::MalformedPublicKey`] if the point is not a valid P-256
/// public key, [`CertVerifyError::MalformedSignature`] if the DER does not
/// decode, and [`CertVerifyError::BadSignature`] if the signature does not
/// verify.
pub fn ecdsa_p256_sha256_verify(
    public_key_sec1: &[u8],
    message: &[u8],
    der_signature: &[u8],
) -> Result<(), CertVerifyError> {
    let verifying_key =
        VerifyingKey::from_sec1_bytes(public_key_sec1).map_err(|_| CertVerifyError::MalformedPublicKey)?;
    let signature = Signature::from_der(der_signature).map_err(|_| CertVerifyError::MalformedSignature)?;
    verifying_key
        .verify(message, &signature)
        .map_err(|_| CertVerifyError::BadSignature)
}

/// Verify a peer's `CertificateVerify` end to end (RFC 8446 §4.4.3): build the
/// signed content for `role` and `transcript_hash`, then verify `signature`
/// under `public_key_sec1` for the named `scheme`.
///
/// `scheme` is the [`super::tls_message::CertificateVerify::algorithm`] value;
/// `public_key_sec1` is the SEC1 EC point from the end-entity certificate;
/// `transcript_hash` is `Transcript-Hash(Handshake Context, Certificate)`;
/// `signature` is [`super::tls_message::CertificateVerify::signature`].
///
/// # Errors
///
/// [`CertVerifyError::UnsupportedScheme`] for any scheme other than
/// `ecdsa_secp256r1_sha256`, plus the [`ecdsa_p256_sha256_verify`] errors.
pub fn verify_certificate_verify(
    scheme: u16,
    public_key_sec1: &[u8],
    role: CertVerifyRole,
    transcript_hash: &[u8],
    signature: &[u8],
) -> Result<(), CertVerifyError> {
    match scheme {
        signature_scheme::ECDSA_SECP256R1_SHA256 => {
            let content = certificate_verify_content(role, transcript_hash);
            ecdsa_p256_sha256_verify(public_key_sec1, &content, signature)
        }
        other => Err(CertVerifyError::UnsupportedScheme(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::SigningKey;

    /// Decode a hex string into bytes for comparing against RFC test vectors.
    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    // ── Signed-content construction (RFC 8446 §4.4.3) ──────────────────────

    #[test]
    fn content_layout_matches_the_spec() {
        // 64 spaces, then the context, then one NUL, then the transcript hash.
        let th = hex("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");
        let content = certificate_verify_content(CertVerifyRole::Server, &th);

        assert_eq!(content.len(), CONTENT_PADDING_LEN + SERVER_CONTEXT.len() + 1 + th.len());
        assert!(content[..CONTENT_PADDING_LEN].iter().all(|&b| b == 0x20));
        let ctx_end = CONTENT_PADDING_LEN + SERVER_CONTEXT.len();
        assert_eq!(&content[CONTENT_PADDING_LEN..ctx_end], SERVER_CONTEXT);
        assert_eq!(content[ctx_end], 0x00);
        assert_eq!(&content[ctx_end + 1..], &th[..]);
    }

    #[test]
    fn server_and_client_contexts_differ() {
        // A signature for one role must not be replayable as the other: the only
        // difference in the signed content is the single word server/client.
        let th = hex("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff");
        let server = certificate_verify_content(CertVerifyRole::Server, &th);
        let client = certificate_verify_content(CertVerifyRole::Client, &th);
        assert_ne!(server, client);
        assert_eq!(SERVER_CONTEXT.len(), CLIENT_CONTEXT.len());
        assert_eq!(CertVerifyRole::Server.context(), SERVER_CONTEXT);
        assert_eq!(CertVerifyRole::Client.context(), CLIENT_CONTEXT);
    }

    #[test]
    fn empty_transcript_hash_still_builds_the_frame() {
        let content = certificate_verify_content(CertVerifyRole::Client, &[]);
        assert_eq!(content.len(), CONTENT_PADDING_LEN + CLIENT_CONTEXT.len() + 1);
        assert_eq!(*content.last().expect("non-empty"), 0x00);
    }

    // ── ECDSA P-256 / SHA-256 primitive against the RFC 6979 vector ────────
    //
    // RFC 6979 Appendix A.2.5 gives a deterministic ECDSA P-256/SHA-256 test
    // vector: a fixed private key, message "sample", and the resulting (r, s).
    // We reconstruct the public key from the private key, assemble the DER
    // signature from (r, s), and verify — exercising the exact code path
    // `verify_certificate_verify` uses for `ecdsa_secp256r1_sha256`.

    /// RFC 6979 A.2.5 private key `x`.
    fn rfc6979_private_key() -> Vec<u8> {
        hex("C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721")
    }

    /// The SEC1 uncompressed public key for [`rfc6979_private_key`], from the
    /// `Ux`/`Uy` given in RFC 6979 A.2.5 (`0x04 ‖ Ux ‖ Uy`).
    fn rfc6979_public_key_sec1() -> Vec<u8> {
        hex(
            "04\
             60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6\
             7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299",
        )
    }

    /// The RFC 6979 A.2.5 (r, s) for message "sample" with SHA-256, as the
    /// 64-byte fixed `r ‖ s` form `Signature::from_slice` accepts.
    fn rfc6979_sample_sig_rs() -> Vec<u8> {
        hex(
            "EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716\
             F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8",
        )
    }

    /// The RFC 6979 A.2.5 signature as ASN.1 DER (the form TLS carries), built
    /// from the fixed (r, s) so the DER-decoding path is exercised too.
    fn rfc6979_sample_sig_der() -> Vec<u8> {
        let sig = Signature::from_slice(&rfc6979_sample_sig_rs()).expect("valid r||s");
        sig.to_der().as_bytes().to_vec()
    }

    #[test]
    fn ecdsa_verifies_the_rfc6979_sample_vector() {
        // The public key derived from the RFC private key matches the published
        // Ux/Uy point (sanity on the vector itself).
        let sk = SigningKey::from_slice(&rfc6979_private_key()).expect("valid P-256 scalar");
        let derived = sk.verifying_key().to_encoded_point(false).as_bytes().to_vec();
        assert_eq!(derived, rfc6979_public_key_sec1());

        // And the published signature over "sample" verifies under that key.
        assert_eq!(
            ecdsa_p256_sha256_verify(&rfc6979_public_key_sec1(), b"sample", &rfc6979_sample_sig_der()),
            Ok(())
        );
    }

    #[test]
    fn ecdsa_rejects_a_tampered_message() {
        // The RFC 6979 signature is over "sample"; verifying it over "samplE"
        // (one bit flipped) must fail.
        assert_eq!(
            ecdsa_p256_sha256_verify(&rfc6979_public_key_sec1(), b"samplE", &rfc6979_sample_sig_der()),
            Err(CertVerifyError::BadSignature)
        );
    }

    #[test]
    fn ecdsa_rejects_a_tampered_signature() {
        let mut der = rfc6979_sample_sig_der();
        let last = der.len() - 1;
        der[last] ^= 0x01;
        // Flipping the last DER byte either breaks decoding or the signature.
        let err = ecdsa_p256_sha256_verify(&rfc6979_public_key_sec1(), b"sample", &der)
            .expect_err("tampered signature must not verify");
        assert!(matches!(err, CertVerifyError::BadSignature | CertVerifyError::MalformedSignature));
    }

    #[test]
    fn ecdsa_rejects_a_malformed_public_key() {
        // A point that is not on the curve / not 65 SEC1 bytes.
        assert_eq!(
            ecdsa_p256_sha256_verify(&[0x04, 0x00, 0x01], b"sample", &rfc6979_sample_sig_der()),
            Err(CertVerifyError::MalformedPublicKey)
        );
    }

    #[test]
    fn ecdsa_rejects_a_malformed_signature() {
        assert_eq!(
            ecdsa_p256_sha256_verify(&rfc6979_public_key_sec1(), b"sample", &[0xff, 0xff, 0xff]),
            Err(CertVerifyError::MalformedSignature)
        );
    }

    // ── End-to-end CertificateVerify over a real transcript hash ───────────
    //
    // Deterministically sign a CertificateVerify content with a fresh P-256 key
    // (ECDSA signatures under RustCrypto are RFC 6979 deterministic), then check
    // that verify_certificate_verify accepts it and rejects every tampering.

    /// A deterministic signing key for the end-to-end tests (fixed scalar).
    fn e2e_key() -> SigningKey {
        SigningKey::from_slice(&hex(
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        ))
        .expect("valid P-256 scalar")
    }

    fn e2e_transcript_hash() -> Vec<u8> {
        // Any 32-byte SHA-256 output stands in for Transcript-Hash(…Certificate).
        use super::super::tls_schedule::transcript_hash;
        transcript_hash(b"ClientHello..Certificate").to_vec()
    }

    #[test]
    fn end_to_end_server_certificate_verify_roundtrips() {
        let sk = e2e_key();
        let pk = sk.verifying_key().to_encoded_point(false).as_bytes().to_vec();
        let th = e2e_transcript_hash();

        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let sig: Signature = sk.sign(&content);
        let der = sig.to_der().as_bytes().to_vec();

        assert_eq!(
            verify_certificate_verify(
                signature_scheme::ECDSA_SECP256R1_SHA256,
                &pk,
                CertVerifyRole::Server,
                &th,
                &der,
            ),
            Ok(())
        );
    }

    #[test]
    fn end_to_end_rejects_wrong_role() {
        // A signature the server made must not verify as a client CertificateVerify.
        let sk = e2e_key();
        let pk = sk.verifying_key().to_encoded_point(false).as_bytes().to_vec();
        let th = e2e_transcript_hash();

        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let sig: Signature = sk.sign(&content);
        let der = sig.to_der().as_bytes().to_vec();

        assert_eq!(
            verify_certificate_verify(
                signature_scheme::ECDSA_SECP256R1_SHA256,
                &pk,
                CertVerifyRole::Client,
                &th,
                &der,
            ),
            Err(CertVerifyError::BadSignature)
        );
    }

    #[test]
    fn end_to_end_rejects_altered_transcript() {
        // The signature binds the transcript: a changed hash must fail (this is
        // what detects a MITM altering the Certificate or earlier messages).
        let sk = e2e_key();
        let pk = sk.verifying_key().to_encoded_point(false).as_bytes().to_vec();
        let th = e2e_transcript_hash();

        let sig: Signature = sk.sign(&certificate_verify_content(CertVerifyRole::Server, &th));
        let der = sig.to_der().as_bytes().to_vec();

        let mut altered = th.clone();
        altered[0] ^= 0xff;
        assert_eq!(
            verify_certificate_verify(
                signature_scheme::ECDSA_SECP256R1_SHA256,
                &pk,
                CertVerifyRole::Server,
                &altered,
                &der,
            ),
            Err(CertVerifyError::BadSignature)
        );
    }

    #[test]
    fn end_to_end_rejects_wrong_key() {
        let sk = e2e_key();
        let th = e2e_transcript_hash();
        let sig: Signature = sk.sign(&certificate_verify_content(CertVerifyRole::Server, &th));
        let der = sig.to_der().as_bytes().to_vec();

        // A different key's public point must reject the signature.
        let other = SigningKey::from_slice(&hex(
            "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100",
        ))
        .expect("valid P-256 scalar");
        let other_pk = other.verifying_key().to_encoded_point(false).as_bytes().to_vec();

        assert_eq!(
            verify_certificate_verify(
                signature_scheme::ECDSA_SECP256R1_SHA256,
                &other_pk,
                CertVerifyRole::Server,
                &th,
                &der,
            ),
            Err(CertVerifyError::BadSignature)
        );
    }

    #[test]
    fn unsupported_schemes_are_reported() {
        let th = e2e_transcript_hash();
        for scheme in [
            signature_scheme::RSA_PSS_RSAE_SHA256,
            signature_scheme::ED25519,
            signature_scheme::ECDSA_SECP384R1_SHA384,
            signature_scheme::RSA_PKCS1_SHA256,
        ] {
            assert_eq!(
                verify_certificate_verify(scheme, &rfc6979_public_key_sec1(), CertVerifyRole::Server, &th, &[]),
                Err(CertVerifyError::UnsupportedScheme(scheme))
            );
        }
    }

    #[test]
    fn error_display_is_nonempty() {
        // Every variant renders a message (Error impl sanity).
        for e in [
            CertVerifyError::UnsupportedScheme(0x0804),
            CertVerifyError::MalformedPublicKey,
            CertVerifyError::MalformedSignature,
            CertVerifyError::BadSignature,
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
