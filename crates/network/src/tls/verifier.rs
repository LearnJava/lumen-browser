//! Custom `ServerCertVerifier` wrapping rustls's `WebPkiServerVerifier`
//! (ph3-tls-hardening, part A2).
//!
//! `LumenVerifier` delegates chain validation to the standard webpki
//! verifier unchanged — this slice adds no new trust policy, it only
//! establishes the seam later slices need:
//!
//! - A3 (OCSP stapling) and A4 (CT enforcement) both need the raw bytes
//!   handed to [`rustls::client::danger::ServerCertVerifier::verify_server_cert`]
//!   (`ocsp_response`, and the leaf's embedded SCT-list extension) — those
//!   bytes are **not** obtainable from `ClientConnection` after the
//!   handshake completes (checked against rustls 0.23.40: no
//!   `ocsp_response()` accessor exists on `CommonState`/`ClientConnection`,
//!   only the verifier ever sees them). A custom verifier is the only place
//!   in the stack that can inspect them, which is why A3/A4 must build on
//!   this file rather than reading back from a completed connection.
//! - A `ClientConfig` (and the verifier inside it) is built once per
//!   [`crate::tls::TlsProfile`] and cached/shared across every concurrent
//!   connection using that profile (`tls_config_for_profile`,
//!   `crates/network/src/lib.rs`). `LumenVerifier` must stay side-effect-free
//!   per call — no `Mutex<Option<..>>` "last seen cert" slot — or concurrent
//!   connections on the same profile would race and read each other's
//!   certificate. A3/A4's hard-fail decisions belong entirely in the
//!   `Result` returned by `verify_server_cert`; any data a later slice needs
//!   to surface to the UI (e.g. leaf cert fields for [`CertInfo`]) must come
//!   from a per-connection source — `ClientConnection::peer_certificates()`
//!   after a successful handshake — not from this verifier.
//!
//! [`CertInfo`]: crate::tls::CertInfo

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme};

/// Wraps rustls's standard webpki chain verifier.
///
/// Today this is a pure pass-through (A2's own acceptance criterion is
/// "normal HTTPS still passes") — it exists so `build_client_config` goes
/// through `.dangerous().with_custom_certificate_verifier(..)` instead of
/// the default `.with_root_certificates(..)`, which is the only seam A3/A4
/// can attach revocation/transparency policy to later.
#[derive(Debug)]
pub struct LumenVerifier {
    inner: Arc<WebPkiServerVerifier>,
}

impl LumenVerifier {
    /// Build a verifier trusting `root_store`, using `provider` for
    /// signature-algorithm support — the same provider `build_client_config`
    /// configures with Chrome's cipher/kx_group order, so verification
    /// accepts exactly the signature schemes this profile's ClientHello
    /// advertised.
    pub fn new(
        root_store: RootCertStore,
        provider: &Arc<CryptoProvider>,
    ) -> Result<Arc<Self>, rustls::client::VerifierBuilderError> {
        let inner =
            WebPkiServerVerifier::builder_with_provider(Arc::new(root_store), provider.clone())
                .build()?;
        Ok(Arc::new(Self { inner }))
    }
}

impl ServerCertVerifier for LumenVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        self.inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> Arc<CryptoProvider> {
        Arc::new(rustls::crypto::aws_lc_rs::default_provider())
    }

    #[test]
    fn builds_with_nonempty_root_store() {
        let verifier = LumenVerifier::new(crate::tls::trusted_root_store(), &provider());
        assert!(verifier.is_ok());
    }

    #[test]
    fn rejects_empty_root_store() {
        let verifier = LumenVerifier::new(RootCertStore::empty(), &provider());
        assert!(verifier.is_err());
    }

    #[test]
    fn supported_verify_schemes_is_nonempty() {
        let verifier = LumenVerifier::new(crate::tls::trusted_root_store(), &provider())
            .expect("valid root store");
        assert!(!verifier.supported_verify_schemes().is_empty());
    }
}
