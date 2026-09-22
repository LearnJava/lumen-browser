//! Custom `ServerCertVerifier` wrapping rustls's `WebPkiServerVerifier`
//! (ph3-tls-hardening, parts A2 + A3).
//!
//! `LumenVerifier` delegates chain validation to the standard webpki
//! verifier first, unchanged, then layers a revocation check on top using
//! the stapled OCSP response ([`crate::tls::ocsp`], part A3) — the seam this
//! module exists to provide:
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
//!   certificate. A3's hard-fail decision (`CertError::Revoked`) is returned
//!   directly from `verify_server_cert`; A4's data (and any later slice's,
//!   e.g. leaf cert fields for [`CertInfo`]) must come from a per-connection
//!   source — `ClientConnection::peer_certificates()` after a successful
//!   handshake — not from this verifier.
//!
//! [`CertInfo`]: crate::tls::CertInfo
//!
//! ph3-tls-hardening A6: a host recorded in [`super::bypass`] ("Proceed
//! anyway" on the shell's cert interstitial) downgrades what would otherwise
//! be a hard-fail — both webpki chain rejection and the A3 stapled-OCSP
//! `revoked` check above — back to a pass. This is deliberately the *only*
//! seam that can turn a rejected certificate into `Ok`: it requires an
//! explicit, previous, per-host user decision recorded by the shell, not an
//! implicit default.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme};

use super::bypass;
use super::ocsp::{self, OcspVerdict};

/// Render a rustls `ServerName` down to the plain hostname string
/// [`bypass::is_allowed`] keys on. `ServerName` is `#[non_exhaustive]`
/// (`rustls-pki-types`) — an unrecognised future variant has no meaningful
/// hostname to bypass by, so it falls back to an empty string (never
/// matches an override, i.e. fails closed).
fn server_name_host(name: &ServerName<'_>) -> String {
    match name {
        ServerName::DnsName(d) => d.as_ref().to_owned(),
        ServerName::IpAddress(ip) => std::net::IpAddr::from(*ip).to_string(),
        _ => String::new(),
    }
}

/// Wraps rustls's standard webpki chain verifier and layers a stapled-OCSP
/// revocation check on top (A3): a `certStatus: revoked` staple hard-fails
/// with `RustlsError::InvalidCertificate(CertificateError::Revoked)`, which
/// [`crate::tls::cert_error::from_io_error`] already maps to
/// `CertError::Revoked` (part A1). Every other OCSP outcome — no staple,
/// `good`, or any unparseable shape — is soft-fail per [`ocsp`]'s own scope
/// and does not affect the result webpki already computed.
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
        let chain_result =
            self.inner
                .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now);
        let verified = match chain_result {
            Ok(v) => v,
            Err(_) if bypass::is_allowed(&server_name_host(server_name)) => {
                // A6: user already clicked "Proceed anyway" for this host —
                // accept without re-validating anything else below either.
                return Ok(ServerCertVerified::assertion());
            }
            Err(e) => return Err(e),
        };
        // A3: layer the stapled-OCSP revocation check on top of webpki's chain trust.
        // Soft-fail everywhere but `Revoked` — see `ocsp` module docs for the full policy.
        if ocsp::parse_stapled_response(ocsp_response) == OcspVerdict::Revoked
            && !bypass::is_allowed(&server_name_host(server_name))
        {
            return Err(RustlsError::InvalidCertificate(CertificateError::Revoked));
        }
        Ok(verified)
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

    #[test]
    fn server_name_host_reads_dns_name() {
        let name = ServerName::try_from("example.com").expect("valid DNS name");
        assert_eq!(server_name_host(&name), "example.com");
    }

    #[test]
    fn server_name_host_reads_ip_address() {
        let name = ServerName::try_from("127.0.0.1").expect("valid IP address");
        assert_eq!(server_name_host(&name), "127.0.0.1");
    }
}
