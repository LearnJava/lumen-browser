//! Server-certificate authentication for the QUIC/TLS 1.3 handshake (RFC 8446
//! §4.4.2, §4.4.3; h3::conn_cert_auth): the slice the connect loop
//! ([`conn_connect`](super::conn_connect)) and every TLS slice below it deliberately
//! left to a caller — "authenticate the server certificate with
//! [`tls_cert_verify`](super::tls_cert_verify) before the connection carries
//! application data".
//!
//! ## What this slice closes
//!
//! The TLS handshake ([`tls_handshake`](super::tls_handshake)) hands back a
//! [`HandshakeComplete`] the moment the server Finished verifies: the raw server
//! [`Certificate`](super::tls_message::Certificate), the
//! [`CertificateVerify`](super::tls_message::CertificateVerify) signature, and the
//! `Transcript-Hash(ClientHello…Certificate)` the server signed over. Two lower
//! slices supply the pieces that consume them but stop one call short of joining
//! them:
//!
//! - [`x509_spki::extract_end_entity_public_key`](super::x509_spki::extract_end_entity_public_key)
//!   decodes the end-entity certificate's `SubjectPublicKeyInfo` into a
//!   [`ServerPublicKey`](super::x509_spki::ServerPublicKey) — the key material in the
//!   exact shape each verifier expects — but "validates neither the certificate's
//!   validity dates nor its chain to a trust anchor".
//! - [`tls_cert_verify::verify_certificate_verify`](super::tls_cert_verify::verify_certificate_verify)
//!   checks a `CertificateVerify` signature under a public key for a named scheme,
//!   but expects the caller to have produced that key and the transcript hash.
//!
//! [`authenticate_server_certificate`] is that join: it pulls the end-entity
//! certificate out of the server's Certificate message, extracts its public key,
//! rejects a key whose type cannot have produced the presented signature scheme
//! ([`ServerPublicKey::accepts_scheme`](super::x509_spki::ServerPublicKey::accepts_scheme)),
//! and verifies the `CertificateVerify` signature over the captured transcript hash.
//! Success proves the peer holds the private key for the certificate it presented —
//! the *possession* half of authentication.
//!
//! ## What it defers
//!
//! - **Trust-anchor chain building.** This slice does not chain the end-entity
//!   certificate to a trusted root, check the intermediate certificates, or honour
//!   `notBefore`/`notAfter` validity dates. A forged certificate whose private key
//!   the peer holds still passes here; binding the certificate to a trusted issuer is
//!   a later slice.
//! - **Hostname (SNI) matching.** Confirming the certificate's
//!   `subjectAltName` covers the requested authority (RFC 6125) is likewise deferred.
//!
//! In short: this slice answers "does the peer hold the private key for the
//! certificate it sent, over this exact handshake?", not "should this certificate be
//! trusted for this origin?".
//!
//! ## Purity
//!
//! A pure function over a [`HandshakeComplete`]: no clock, no IO, no connection
//! state. A test builds a real end-entity certificate and a real signature over the
//! signed content and drives the whole check deterministically.

use super::tls_cert_verify::{CertVerifyError, CertVerifyRole, verify_certificate_verify};
use super::tls_handshake::HandshakeComplete;
use super::x509_spki::{ServerPublicKey, SpkiAlgorithm, SpkiError, extract_end_entity_public_key};

/// Why authenticating the server certificate failed (RFC 8446 §4.4.2, §4.4.3). Every
/// variant is a fatal handshake error: the connection must be closed rather than
/// carry application data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CertAuthError {
    /// The server's `Certificate` message carried an empty `certificate_list`
    /// (RFC 8446 §4.4.2 requires the end-entity certificate first). A fatal
    /// `decode_error`.
    NoCertificate,
    /// The end-entity certificate's `SubjectPublicKeyInfo` could not be decoded, or
    /// named an algorithm this build does not verify
    /// ([`extract_end_entity_public_key`](super::x509_spki::extract_end_entity_public_key)).
    Spki(SpkiError),
    /// The certificate's key type cannot have produced the `CertificateVerify`'s
    /// signature scheme (RFC 8446 §4.2.3 binds each scheme to a key type): e.g. an
    /// ECDSA P-256 certificate presenting an `rsa_pss_rsae_sha256` signature.
    /// Rejected before the verifier, so a key/scheme mismatch is not mistaken for a
    /// bad signature. A fatal `illegal_parameter`.
    SchemeMismatch {
        /// The certificate's public-key algorithm.
        key: SpkiAlgorithm,
        /// The `CertificateVerify` signature scheme (RFC 8446 §4.2.3 codepoint) the
        /// key cannot produce.
        scheme: u16,
    },
    /// The `CertificateVerify` signature did not verify over the signed content
    /// ([`verify_certificate_verify`](super::tls_cert_verify::verify_certificate_verify)):
    /// the peer does not hold the certificate's private key, or the transcript was
    /// altered. A fatal `decrypt_error` (RFC 8446 §4.4.3).
    Verify(CertVerifyError),
}

impl core::fmt::Display for CertAuthError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoCertificate => f.write_str("server Certificate carried no end-entity certificate"),
            Self::Spki(e) => write!(f, "end-entity certificate: {e}"),
            Self::SchemeMismatch { key, scheme } => write!(
                f,
                "certificate key {key:?} cannot produce CertificateVerify scheme 0x{scheme:04x}"
            ),
            Self::Verify(e) => write!(f, "CertificateVerify: {e}"),
        }
    }
}

impl std::error::Error for CertAuthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spki(e) => Some(e),
            Self::Verify(e) => Some(e),
            Self::NoCertificate | Self::SchemeMismatch { .. } => None,
        }
    }
}

/// Authenticate a completed handshake's server certificate (RFC 8446 §4.4.3): prove
/// the peer holds the private key for the end-entity certificate it presented, over
/// this exact handshake transcript.
///
/// Takes the [`HandshakeComplete`] the TLS handshake produced and:
/// 1. reads the end-entity certificate — the first
///    [`CertificateEntry`](super::tls_message::CertificateEntry) of the server's
///    Certificate message (RFC 8446 §4.4.2);
/// 2. extracts its public key with
///    [`extract_end_entity_public_key`](super::x509_spki::extract_end_entity_public_key);
/// 3. rejects a key whose type cannot have produced the CertificateVerify's signature
///    scheme ([`ServerPublicKey::accepts_scheme`](super::x509_spki::ServerPublicKey::accepts_scheme));
/// 4. verifies the CertificateVerify signature over
///    [`certificate_transcript_hash`](super::tls_handshake::HandshakeComplete::certificate_transcript_hash),
///    the `Transcript-Hash(ClientHello…Certificate)` the server signed.
///
/// On success it returns the extracted [`ServerPublicKey`] — the authenticated
/// end-entity key, for a later slice to chain to a trust anchor.
///
/// This checks *possession* only: it does not chain the certificate to a trusted
/// root or match it to the requested hostname. Those are separate, later checks.
///
/// # Errors
///
/// [`CertAuthError`] naming the failing step: no end-entity certificate, an
/// undecodable or unsupported `SubjectPublicKeyInfo`, a key/scheme mismatch, or a
/// signature that did not verify.
pub fn authenticate_server_certificate(
    complete: &HandshakeComplete,
) -> Result<ServerPublicKey, CertAuthError> {
    // The end-entity certificate is first in the list (RFC 8446 §4.4.2).
    let entry = complete
        .server_certificate
        .certificate_list
        .first()
        .ok_or(CertAuthError::NoCertificate)?;
    let key = extract_end_entity_public_key(&entry.cert_data).map_err(CertAuthError::Spki)?;

    let cv = &complete.server_certificate_verify;
    // Reject a key/scheme mismatch before the verifier so it is not reported as a
    // malformed-key or bad-signature error (RFC 8446 §4.2.3).
    if !key.accepts_scheme(cv.algorithm) {
        return Err(CertAuthError::SchemeMismatch { key: key.algorithm, scheme: cv.algorithm });
    }

    verify_certificate_verify(
        cv.algorithm,
        &key.key_material,
        CertVerifyRole::Server,
        &complete.certificate_transcript_hash,
        &cv.signature,
    )
    .map_err(CertAuthError::Verify)?;

    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::tls_cert_verify::{certificate_verify_content, signature_scheme};
    use crate::h3::tls_handshake::HandshakeComplete;
    use crate::h3::tls_message::{Certificate, CertificateEntry, CertificateVerify};
    use crate::h3::tls_schedule::ApplicationTrafficSecrets;

    // ── DER certificate builder (a minimal, structurally valid end-entity cert) ──

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

    /// `id-Ed25519` OID (RFC 8410 §3): 1.3.101.112.
    const OID_ED25519: &[u8] = &[0x2B, 0x65, 0x70];
    /// `id-ecPublicKey` OID (RFC 5480): 1.2.840.10045.2.1.
    const OID_EC_PUBLIC_KEY: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
    /// `prime256v1` (P-256) OID (RFC 5480): 1.2.840.10045.3.1.7.
    const OID_SECP256R1: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

    /// Wrap an `AlgorithmIdentifier` and raw key octets into a `SubjectPublicKeyInfo`
    /// and then into a minimal v3 certificate whose fields before the SPKI are
    /// placeholders the extractor skips.
    fn cert_with_key(alg_id: &[u8], key_octets: &[u8]) -> Vec<u8> {
        let mut bit_string = vec![0x00];
        bit_string.extend_from_slice(key_octets);
        let spki = tlv(0x30, &cat(&[alg_id, &tlv(0x03, &bit_string)]));

        let version = tlv(0xA0, &tlv(0x02, &[0x02])); // [0] { v3 = 2 }
        let serial = tlv(0x02, &[0x01]);
        let sig_alg = tlv(0x30, &tlv(0x06, OID_EC_PUBLIC_KEY));
        let issuer = tlv(0x30, &[]);
        let validity = tlv(0x30, &[]);
        let subject = tlv(0x30, &[]);
        let tbs = tlv(
            0x30,
            &cat(&[&version, &serial, &sig_alg, &issuer, &validity, &subject, &spki]),
        );
        let outer_sig_alg = tlv(0x30, &tlv(0x06, OID_EC_PUBLIC_KEY));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        tlv(0x30, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    fn ed25519_alg_id() -> Vec<u8> {
        tlv(0x30, &tlv(0x06, OID_ED25519))
    }

    fn ec_p256_alg_id() -> Vec<u8> {
        tlv(0x30, &cat(&[&tlv(0x06, OID_EC_PUBLIC_KEY), &tlv(0x06, OID_SECP256R1)]))
    }

    /// The transcript hash the CertificateVerify signs over (any 32-octet value; the
    /// authenticator treats it as opaque and passes it verbatim to the verifier).
    const TRANSCRIPT: [u8; 32] = [0x5A; 32];

    /// Assemble a [`HandshakeComplete`] whose certificate and CertificateVerify are
    /// the only fields the authenticator reads; the key material is irrelevant here.
    fn complete_with(cert_der: Vec<u8>, cv: CertificateVerify) -> HandshakeComplete {
        let app_secrets = ApplicationTrafficSecrets { client: [0; 32], server: [0; 32] };
        HandshakeComplete {
            app_keys: app_secrets.packet_keys(),
            app_secrets,
            client_finished: Vec::new(),
            master_secret: [0; 32],
            exporter_master_secret: [0; 32],
            resumption_master_secret: [0; 32],
            server_certificate: Certificate {
                certificate_request_context: Vec::new(),
                certificate_list: vec![CertificateEntry { cert_data: cert_der, extensions: Vec::new() }],
            },
            server_certificate_verify: cv,
            certificate_transcript_hash: TRANSCRIPT,
        }
    }

    // ── Ed25519: a real certificate + a real signature authenticate ─────────

    #[test]
    fn authenticates_an_ed25519_certificate_with_a_valid_signature() {
        use ed25519_dalek::{Signer, SigningKey};

        let signing = SigningKey::from_bytes(&[0x42; 32]);
        let public = signing.verifying_key().to_bytes();
        let cert = cert_with_key(&ed25519_alg_id(), &public);

        // Sign the exact content the server signs (RFC 8446 §4.4.3).
        let content = certificate_verify_content(CertVerifyRole::Server, &TRANSCRIPT);
        let signature = signing.sign(&content).to_bytes().to_vec();
        let cv = CertificateVerify { algorithm: signature_scheme::ED25519, signature };

        let complete = complete_with(cert, cv);
        let key = authenticate_server_certificate(&complete).expect("authenticates");
        assert_eq!(key.algorithm, SpkiAlgorithm::Ed25519);
        assert_eq!(key.key_material, public);
    }

    // ── ECDSA P-256: a real certificate + a real signature authenticate ─────

    #[test]
    fn authenticates_a_p256_certificate_with_a_valid_signature() {
        use p256::ecdsa::signature::Signer;
        use p256::ecdsa::{Signature, SigningKey};

        let signing = SigningKey::from_slice(&[0x11; 32]).expect("valid P-256 scalar");
        let sec1 = signing.verifying_key().to_encoded_point(false).as_bytes().to_vec();
        let cert = cert_with_key(&ec_p256_alg_id(), &sec1);

        let content = certificate_verify_content(CertVerifyRole::Server, &TRANSCRIPT);
        let signature: Signature = signing.sign(&content);
        let cv = CertificateVerify {
            algorithm: signature_scheme::ECDSA_SECP256R1_SHA256,
            signature: signature.to_der().as_bytes().to_vec(),
        };

        let complete = complete_with(cert, cv);
        let key = authenticate_server_certificate(&complete).expect("authenticates");
        assert_eq!(key.algorithm, SpkiAlgorithm::EcdsaP256);
    }

    // ── A tampered transcript makes the signature fail (BadSignature) ───────

    #[test]
    fn rejects_a_signature_over_a_different_transcript() {
        use ed25519_dalek::{Signer, SigningKey};

        let signing = SigningKey::from_bytes(&[0x42; 32]);
        let public = signing.verifying_key().to_bytes();
        let cert = cert_with_key(&ed25519_alg_id(), &public);

        // Sign over a *different* transcript hash than the one in the completion.
        let other = [0xA5; 32];
        let content = certificate_verify_content(CertVerifyRole::Server, &other);
        let signature = signing.sign(&content).to_bytes().to_vec();
        let cv = CertificateVerify { algorithm: signature_scheme::ED25519, signature };

        let complete = complete_with(cert, cv);
        assert_eq!(
            authenticate_server_certificate(&complete),
            Err(CertAuthError::Verify(CertVerifyError::BadSignature)),
        );
    }

    // ── A client-role signature does not authenticate as the server ─────────

    #[test]
    fn rejects_a_client_role_signature() {
        use ed25519_dalek::{Signer, SigningKey};

        let signing = SigningKey::from_bytes(&[0x42; 32]);
        let public = signing.verifying_key().to_bytes();
        let cert = cert_with_key(&ed25519_alg_id(), &public);

        // Correct key and transcript, but the client context string — the only
        // difference in the signed content (RFC 8446 §4.4.3).
        let content = certificate_verify_content(CertVerifyRole::Client, &TRANSCRIPT);
        let signature = signing.sign(&content).to_bytes().to_vec();
        let cv = CertificateVerify { algorithm: signature_scheme::ED25519, signature };

        let complete = complete_with(cert, cv);
        assert_eq!(
            authenticate_server_certificate(&complete),
            Err(CertAuthError::Verify(CertVerifyError::BadSignature)),
        );
    }

    // ── Key/scheme mismatch is rejected before the verifier ─────────────────

    #[test]
    fn rejects_a_key_scheme_mismatch() {
        use ed25519_dalek::{Signer, SigningKey};

        // An Ed25519 certificate but a CertificateVerify naming an RSA scheme.
        let signing = SigningKey::from_bytes(&[0x42; 32]);
        let public = signing.verifying_key().to_bytes();
        let cert = cert_with_key(&ed25519_alg_id(), &public);

        let content = certificate_verify_content(CertVerifyRole::Server, &TRANSCRIPT);
        let signature = signing.sign(&content).to_bytes().to_vec();
        let cv = CertificateVerify {
            algorithm: signature_scheme::RSA_PSS_RSAE_SHA256,
            signature,
        };

        let complete = complete_with(cert, cv);
        assert_eq!(
            authenticate_server_certificate(&complete),
            Err(CertAuthError::SchemeMismatch {
                key: SpkiAlgorithm::Ed25519,
                scheme: signature_scheme::RSA_PSS_RSAE_SHA256,
            }),
        );
    }

    // ── A malformed end-entity certificate surfaces the SPKI error ──────────

    #[test]
    fn rejects_a_malformed_certificate() {
        let cv = CertificateVerify {
            algorithm: signature_scheme::ED25519,
            signature: vec![0u8; 64],
        };
        // Not a DER SEQUENCE at all.
        let complete = complete_with(vec![0x01, 0x02, 0x03], cv);
        assert!(matches!(
            authenticate_server_certificate(&complete),
            Err(CertAuthError::Spki(_)),
        ));
    }

    // ── An empty certificate list is a NoCertificate error ──────────────────

    #[test]
    fn rejects_an_empty_certificate_list() {
        let cv = CertificateVerify {
            algorithm: signature_scheme::ED25519,
            signature: vec![0u8; 64],
        };
        let mut complete = complete_with(Vec::new(), cv);
        complete.server_certificate.certificate_list.clear();
        assert_eq!(
            authenticate_server_certificate(&complete),
            Err(CertAuthError::NoCertificate),
        );
    }
}
