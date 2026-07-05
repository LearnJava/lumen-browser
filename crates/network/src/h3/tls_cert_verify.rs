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
//! - [`ed25519_verify`] — the raw `ed25519` (EdDSA / Curve25519) primitive.
//! - [`rsa_pss_sha256_verify`] — the raw `rsa_pss_rsae_sha256` (RSASSA-PSS /
//!   SHA-256) signature verification primitive over an arbitrary message.
//! - [`verify_certificate_verify`] — the end-to-end check: build the signed
//!   content for the role and transcript, then verify the signature under the
//!   peer's public key for the named scheme.
//!
//! ## Scheme coverage
//!
//! Three schemes are verified here, all either TLS 1.3 mandatory-to-implement or
//! ubiquitous in deployment, each reusing pure-Rust RustCrypto verifiers:
//!
//! - `ecdsa_secp256r1_sha256` (0x0403, [`ecdsa_p256_sha256_verify`]) — ECDSA over
//!   NIST P-256 with SHA-256, an RFC 8446 §9.1 mandatory scheme, on the `p256`
//!   crate (WebAuthn ES256). Public key is the SEC1 EC point; signature is DER.
//! - `ed25519` (0x0807, [`ed25519_verify`]) — EdDSA over Curve25519 (RFC 8032),
//!   signs the message directly with no prehash and no DER wrapper, on the
//!   `ed25519-dalek` crate, which reuses the `curve25519-dalek` already in the
//!   tree via `x25519-dalek`. Public key is the raw 32-octet Ed25519 point;
//!   signature is the raw 64 octets.
//! - `rsa_pss_rsae_sha256` (0x0804, [`rsa_pss_sha256_verify`]) — RSASSA-PSS with
//!   an rsaEncryption key and SHA-256 (RFC 8446 §4.2.3, RFC 8017 §8.1), by far the
//!   most common server-certificate signature in the wild, on the `rsa` crate.
//!   Public key is the PKCS#1 DER `RSAPublicKey` (`SEQUENCE { modulus,
//!   publicExponent }`) — i.e. the `subjectPublicKey` of an rsaEncryption
//!   `SubjectPublicKeyInfo`; signature is the raw big-endian integer, one modulus
//!   length wide, that TLS carries.
//!
//! ## Deferred
//!
//! The remaining schemes named in [`signature_scheme`] (the P-384/P-521 ECDSA
//! variants, the `rsa_pss_rsae` SHA-384/SHA-512 and `rsa_pss_pss` variants, and
//! the RSA-PKCS1 / ed448 codepoints) return
//! [`CertVerifyError::UnsupportedScheme`] until a later slice wires their
//! verifiers. Extracting the public key from the end-entity certificate's
//! `SubjectPublicKeyInfo` is the caller's job (X.509 parsing is delegated, see
//! [`super::tls_message`]); this module takes the decoded key material directly
//! (the SEC1 EC point for ECDSA, the raw 32-octet point for Ed25519, the PKCS#1
//! DER `RSAPublicKey` for RSASSA-PSS).

// `signature::Verifier` is the shared trait behind every `.verify()` call below;
// ed25519-dalek re-exports the very same trait p256 does, and `rsa` 0.9 rides the
// same `signature` v2 crate, so this one import covers all three schemes.
use ed25519_dalek::{Signature as Ed25519Signature, Verifier, VerifyingKey as Ed25519VerifyingKey};
use p256::ecdsa::{Signature, VerifyingKey};
// RSASSA-PSS with SHA-256 (`rsa_pss_rsae_sha256`): only the concrete key/signature
// types and the digest are new; verification goes through the shared `Verifier`.
use rsa::RsaPublicKey;
use rsa::pkcs1::DecodeRsaPublicKey as _;
use rsa::pss::{Signature as RsaPssSignature, VerifyingKey as RsaPssVerifyingKey};
use sha2::Sha256;

/// The length in octets of a raw Ed25519 public key (RFC 8032 §5.1).
pub const ED25519_PUBLIC_KEY_LEN: usize = 32;

/// The length in octets of a raw Ed25519 signature (RFC 8032 §5.1).
pub const ED25519_SIGNATURE_LEN: usize = 64;

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

/// Verify an `ed25519` signature (RFC 8446 §4.2.3, RFC 8032): EdDSA over
/// Curve25519, which signs the message directly (no prehash) and carries the
/// signature raw (no DER wrapper), unlike the ECDSA schemes.
///
/// `public_key` is the peer's raw 32-octet Ed25519 public key (the
/// `subjectPublicKey` bit string of the end-entity certificate's
/// `SubjectPublicKeyInfo`, RFC 8410 §4). `message` is the already-built signed
/// content (see [`certificate_verify_content`]); Ed25519 hashes it internally as
/// part of the signature equation, so it is passed verbatim. `signature` is the
/// raw 64-octet `R ‖ S` value.
///
/// # Errors
///
/// [`CertVerifyError::MalformedPublicKey`] if `public_key` is not a valid
/// 32-octet Ed25519 point, [`CertVerifyError::MalformedSignature`] if `signature`
/// is not 64 octets, and [`CertVerifyError::BadSignature`] if the signature does
/// not verify over `message`.
pub fn ed25519_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), CertVerifyError> {
    let key_bytes: &[u8; ED25519_PUBLIC_KEY_LEN] =
        public_key.try_into().map_err(|_| CertVerifyError::MalformedPublicKey)?;
    let verifying_key =
        Ed25519VerifyingKey::from_bytes(key_bytes).map_err(|_| CertVerifyError::MalformedPublicKey)?;
    let signature =
        Ed25519Signature::from_slice(signature).map_err(|_| CertVerifyError::MalformedSignature)?;
    verifying_key
        .verify(message, &signature)
        .map_err(|_| CertVerifyError::BadSignature)
}

/// Verify an `rsa_pss_rsae_sha256` signature (RFC 8446 §4.2.3, RFC 8017 §8.1):
/// RSASSA-PSS with the MGF1-SHA-256 mask and SHA-256 message hash, the scheme most
/// real-world server certificates sign with.
///
/// `public_key_pkcs1_der` is the peer's key as a PKCS#1 DER `RSAPublicKey`
/// (`SEQUENCE { modulus INTEGER, publicExponent INTEGER }`) — the
/// `subjectPublicKey` bit string of an rsaEncryption `SubjectPublicKeyInfo`.
/// `message` is the already-built signed content (see
/// [`certificate_verify_content`]); it is SHA-256-hashed internally as PSS
/// requires. `signature` is the raw signature octets TLS carries: the big-endian
/// integer, exactly one modulus length wide.
///
/// The salt length is recovered from the signature during EMSA-PSS-VERIFY
/// (RFC 8017 §9.1.2), so this accepts the RFC 8446 §4.2.3 salt-equals-digest-length
/// signatures interoperably without a fixed-length assumption.
///
/// # Errors
///
/// [`CertVerifyError::MalformedPublicKey`] if the PKCS#1 DER does not decode to a
/// valid RSA public key, [`CertVerifyError::MalformedSignature`] if the signature
/// octets are not a well-formed PSS signature, and [`CertVerifyError::BadSignature`]
/// if the signature does not verify over `message`.
pub fn rsa_pss_sha256_verify(
    public_key_pkcs1_der: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<(), CertVerifyError> {
    let public_key =
        RsaPublicKey::from_pkcs1_der(public_key_pkcs1_der).map_err(|_| CertVerifyError::MalformedPublicKey)?;
    let verifying_key = RsaPssVerifyingKey::<Sha256>::new(public_key);
    let signature = RsaPssSignature::try_from(signature).map_err(|_| CertVerifyError::MalformedSignature)?;
    verifying_key
        .verify(message, &signature)
        .map_err(|_| CertVerifyError::BadSignature)
}

/// Verify a peer's `CertificateVerify` end to end (RFC 8446 §4.4.3): build the
/// signed content for `role` and `transcript_hash`, then verify `signature`
/// under `public_key_sec1` for the named `scheme`.
///
/// `scheme` is the [`super::tls_message::CertificateVerify::algorithm`] value;
/// `public_key` is the peer's key material from the end-entity certificate;
/// `transcript_hash` is `Transcript-Hash(Handshake Context, Certificate)`;
/// `signature` is [`super::tls_message::CertificateVerify::signature`].
///
/// `public_key` is the decoded key material for the scheme — the SEC1 EC point
/// for `ecdsa_secp256r1_sha256`, the raw 32-octet point for `ed25519`, the PKCS#1
/// DER `RSAPublicKey` for `rsa_pss_rsae_sha256`.
///
/// # Errors
///
/// [`CertVerifyError::UnsupportedScheme`] for any scheme other than
/// `ecdsa_secp256r1_sha256`, `ed25519`, or `rsa_pss_rsae_sha256`, plus the
/// per-scheme verifier errors ([`ecdsa_p256_sha256_verify`], [`ed25519_verify`],
/// [`rsa_pss_sha256_verify`]).
pub fn verify_certificate_verify(
    scheme: u16,
    public_key: &[u8],
    role: CertVerifyRole,
    transcript_hash: &[u8],
    signature: &[u8],
) -> Result<(), CertVerifyError> {
    match scheme {
        signature_scheme::ECDSA_SECP256R1_SHA256 => {
            let content = certificate_verify_content(role, transcript_hash);
            ecdsa_p256_sha256_verify(public_key, &content, signature)
        }
        signature_scheme::ED25519 => {
            let content = certificate_verify_content(role, transcript_hash);
            ed25519_verify(public_key, &content, signature)
        }
        signature_scheme::RSA_PSS_RSAE_SHA256 => {
            let content = certificate_verify_content(role, transcript_hash);
            rsa_pss_sha256_verify(public_key, &content, signature)
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

    // ── Ed25519 primitive against the RFC 8032 §7.1 vector ─────────────────
    //
    // RFC 8032 §7.1 TEST 2 fixes a secret key, the public key it derives to, a
    // one-octet message `0x72`, and the resulting signature. We rebuild the
    // signature deterministically from the RFC secret key (Ed25519 signing is
    // deterministic, RFC 8032 §5.1.6, so the produced bytes ARE the RFC
    // signature) and first assert the derived public key equals the RFC public
    // key — pinning the vector to the RFC before feeding it to `ed25519_verify`.

    /// The RFC 8032 §7.1 TEST 2 secret key (seed).
    fn rfc8032_test2_secret_key() -> Vec<u8> {
        hex("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb")
    }

    /// The RFC 8032 §7.1 TEST 2 public key (raw 32-octet Ed25519 point) the
    /// secret key must derive to.
    fn rfc8032_test2_public_key() -> Vec<u8> {
        hex("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c")
    }

    /// The RFC 8032 §7.1 TEST 2 message (a single octet).
    const RFC8032_TEST2_MESSAGE: &[u8] = &[0x72];

    /// The RFC 8032 TEST 2 (public key, signature) pair, rebuilt from the RFC
    /// secret key. Asserts the derived public key matches the published one so
    /// the vector is pinned to the RFC, then returns the deterministic signature.
    fn rfc8032_test2_pk_sig() -> (Vec<u8>, Vec<u8>) {
        use ed25519_dalek::Signer;
        let sk = ed25519_dalek::SigningKey::from_bytes(
            &rfc8032_test2_secret_key().try_into().expect("32-byte seed"),
        );
        let pk = sk.verifying_key().to_bytes().to_vec();
        assert_eq!(pk, rfc8032_test2_public_key(), "RFC 8032 TEST 2 public key mismatch");
        let sig = sk.sign(RFC8032_TEST2_MESSAGE).to_bytes().to_vec();
        (pk, sig)
    }

    #[test]
    fn ed25519_verifies_the_rfc8032_test2_vector() {
        let (pk, sig) = rfc8032_test2_pk_sig();
        assert_eq!(ed25519_verify(&pk, RFC8032_TEST2_MESSAGE, &sig), Ok(()));
    }

    #[test]
    fn ed25519_rejects_a_tampered_message() {
        // The RFC 8032 TEST 2 signature is over `0x72`; verifying it over any
        // other message must fail.
        let (pk, sig) = rfc8032_test2_pk_sig();
        assert_eq!(ed25519_verify(&pk, &[0x73], &sig), Err(CertVerifyError::BadSignature));
    }

    #[test]
    fn ed25519_rejects_a_tampered_signature() {
        let (pk, mut sig) = rfc8032_test2_pk_sig();
        sig[0] ^= 0x01;
        assert_eq!(
            ed25519_verify(&pk, RFC8032_TEST2_MESSAGE, &sig),
            Err(CertVerifyError::BadSignature)
        );
    }

    #[test]
    fn ed25519_rejects_a_wrong_length_public_key() {
        // Ed25519 keys are exactly 32 octets; anything else is malformed.
        let (_, sig) = rfc8032_test2_pk_sig();
        assert_eq!(
            ed25519_verify(&[0u8; 31], RFC8032_TEST2_MESSAGE, &sig),
            Err(CertVerifyError::MalformedPublicKey)
        );
        assert_eq!(
            ed25519_verify(&[0u8; 33], RFC8032_TEST2_MESSAGE, &sig),
            Err(CertVerifyError::MalformedPublicKey)
        );
    }

    #[test]
    fn ed25519_rejects_a_wrong_length_signature() {
        // Ed25519 signatures are exactly 64 octets.
        let (pk, _) = rfc8032_test2_pk_sig();
        assert_eq!(
            ed25519_verify(&pk, RFC8032_TEST2_MESSAGE, &[0u8; 63]),
            Err(CertVerifyError::MalformedSignature)
        );
    }

    // ── End-to-end Ed25519 CertificateVerify over a real transcript hash ───
    //
    // Sign a CertificateVerify content with a deterministic Ed25519 key (Ed25519
    // is fully deterministic per RFC 8032), then check that
    // verify_certificate_verify accepts it and rejects every tampering.

    /// A deterministic Ed25519 signing key for the end-to-end tests (fixed seed).
    fn ed25519_e2e_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(
            &hex("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20")
                .try_into()
                .expect("32-byte seed"),
        )
    }

    #[test]
    fn end_to_end_ed25519_server_certificate_verify_roundtrips() {
        use ed25519_dalek::Signer;
        let sk = ed25519_e2e_key();
        let pk = sk.verifying_key().to_bytes().to_vec();
        let th = e2e_transcript_hash();

        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let sig = sk.sign(&content).to_bytes().to_vec();

        assert_eq!(
            verify_certificate_verify(signature_scheme::ED25519, &pk, CertVerifyRole::Server, &th, &sig),
            Ok(())
        );
    }

    #[test]
    fn end_to_end_ed25519_rejects_wrong_role_and_transcript() {
        use ed25519_dalek::Signer;
        let sk = ed25519_e2e_key();
        let pk = sk.verifying_key().to_bytes().to_vec();
        let th = e2e_transcript_hash();

        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let sig = sk.sign(&content).to_bytes().to_vec();

        // A server signature must not verify as a client CertificateVerify.
        assert_eq!(
            verify_certificate_verify(signature_scheme::ED25519, &pk, CertVerifyRole::Client, &th, &sig),
            Err(CertVerifyError::BadSignature)
        );
        // Nor over an altered transcript hash (MITM detection).
        let mut altered = th.clone();
        altered[0] ^= 0xff;
        assert_eq!(
            verify_certificate_verify(signature_scheme::ED25519, &pk, CertVerifyRole::Server, &altered, &sig),
            Err(CertVerifyError::BadSignature)
        );
    }

    // ── End-to-end RSASSA-PSS CertificateVerify over a real transcript hash ─
    //
    // RSA-PSS signing needs randomness for the salt (unlike the deterministic
    // ECDSA/Ed25519 above), so a fresh 2048-bit key is generated and the salt is
    // drawn from the OS CSPRNG. Verification recovers the salt (RFC 8017 §9.1.2),
    // so a fresh salt each run still verifies — the test's pass/fail is
    // deterministic even though the signature bytes are not. 2048 bits comfortably
    // exceeds the PSS minimum for SHA-256 (hLen + sLen + 2 = 66 octets).

    /// A freshly generated 2048-bit RSA signing key plus its PKCS#1 DER public key.
    fn rsa_e2e_key() -> (rsa::pss::SigningKey<Sha256>, Vec<u8>) {
        use rand_core::OsRng;
        use rsa::pkcs1::EncodeRsaPublicKey;
        let private_key = rsa::RsaPrivateKey::new(&mut OsRng, 2048).expect("generate RSA key");
        let public_der = private_key
            .to_public_key()
            .to_pkcs1_der()
            .expect("encode RSA public key")
            .as_bytes()
            .to_vec();
        (rsa::pss::SigningKey::<Sha256>::new(private_key), public_der)
    }

    /// Produce an `rsa_pss_rsae_sha256` signature over `content` with `signing_key`.
    fn rsa_sign(signing_key: &rsa::pss::SigningKey<Sha256>, content: &[u8]) -> Vec<u8> {
        use rand_core::OsRng;
        use rsa::signature::{RandomizedSigner, SignatureEncoding};
        signing_key.sign_with_rng(&mut OsRng, content).to_bytes().to_vec()
    }

    #[test]
    fn end_to_end_rsa_pss_server_certificate_verify_roundtrips() {
        let (signing_key, pk_der) = rsa_e2e_key();
        let th = e2e_transcript_hash();
        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let sig = rsa_sign(&signing_key, &content);

        assert_eq!(
            verify_certificate_verify(
                signature_scheme::RSA_PSS_RSAE_SHA256,
                &pk_der,
                CertVerifyRole::Server,
                &th,
                &sig,
            ),
            Ok(())
        );
    }

    #[test]
    fn end_to_end_rsa_pss_rejects_wrong_role_transcript_and_signature() {
        let (signing_key, pk_der) = rsa_e2e_key();
        let th = e2e_transcript_hash();
        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let sig = rsa_sign(&signing_key, &content);

        // Wrong role: a server signature must not verify as a client one.
        assert_eq!(
            verify_certificate_verify(
                signature_scheme::RSA_PSS_RSAE_SHA256,
                &pk_der,
                CertVerifyRole::Client,
                &th,
                &sig,
            ),
            Err(CertVerifyError::BadSignature)
        );
        // Altered transcript hash (MITM detection).
        let mut altered = th.clone();
        altered[0] ^= 0xff;
        assert_eq!(
            verify_certificate_verify(
                signature_scheme::RSA_PSS_RSAE_SHA256,
                &pk_der,
                CertVerifyRole::Server,
                &altered,
                &sig,
            ),
            Err(CertVerifyError::BadSignature)
        );
        // Tampered signature octet.
        let mut bad_sig = sig.clone();
        bad_sig[0] ^= 0x01;
        assert_eq!(
            verify_certificate_verify(
                signature_scheme::RSA_PSS_RSAE_SHA256,
                &pk_der,
                CertVerifyRole::Server,
                &th,
                &bad_sig,
            ),
            Err(CertVerifyError::BadSignature)
        );
    }

    #[test]
    fn rsa_pss_rejects_malformed_key_and_signature() {
        let (signing_key, pk_der) = rsa_e2e_key();
        let th = e2e_transcript_hash();
        let content = certificate_verify_content(CertVerifyRole::Server, &th);
        let sig = rsa_sign(&signing_key, &content);

        // A public key that is not valid PKCS#1 DER.
        assert_eq!(
            rsa_pss_sha256_verify(&[0x30, 0x00, 0x01], &content, &sig),
            Err(CertVerifyError::MalformedPublicKey)
        );
        // A signature that is not one modulus width wide fails (either the PSS
        // representative is out of range or the octet string is ill-formed).
        let short = &sig[..sig.len() / 2];
        assert!(matches!(
            rsa_pss_sha256_verify(&pk_der, &content, short),
            Err(CertVerifyError::MalformedSignature | CertVerifyError::BadSignature)
        ));
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
            signature_scheme::RSA_PSS_RSAE_SHA384,
            signature_scheme::ECDSA_SECP384R1_SHA384,
            signature_scheme::RSA_PKCS1_SHA256,
            signature_scheme::ED448,
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
