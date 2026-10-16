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

use super::{bypass, ct, failure};
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
    /// `Name` contents of every trust anchor, for [`failure::summarize`].
    anchor_subjects: Vec<Vec<u8>>,
    /// COMPAT-TRUST-CTLOGS: when `Some(n)`, a chain that webpki accepted is still rejected
    /// unless SCTs of at least `n` distinct log operators verify ([`ct::evaluate_ct`]).
    /// `None` (the default) leaves CT out of the handshake decision. The seam exists for
    /// trust anchors that are acceptable only under CT (the national CA); no profile turns
    /// it on yet.
    ct_min_operators: Option<usize>,
}

/// The chain was valid but its SCTs did not reach the operator count the verifier requires.
#[derive(Debug)]
struct CtInsufficient {
    found: usize,
    required: usize,
}

impl std::fmt::Display for CtInsufficient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Certificate Transparency: valid SCTs from {} log operator(s), {} required",
            self.found, self.required
        )
    }
}

impl std::error::Error for CtInsufficient {}

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
        Self::with_ct_requirement(root_store, provider, None)
    }

    /// Like [`Self::new`], additionally requiring SCTs of `ct_min_operators` distinct log
    /// operators on every accepted chain (`None` — no CT requirement).
    pub fn with_ct_requirement(
        root_store: RootCertStore,
        provider: &Arc<CryptoProvider>,
        ct_min_operators: Option<usize>,
    ) -> Result<Arc<Self>, rustls::client::VerifierBuilderError> {
        let anchor_subjects = root_store.roots.iter().map(|a| a.subject.as_ref().to_vec()).collect();
        let inner =
            WebPkiServerVerifier::builder_with_provider(Arc::new(root_store), provider.clone())
                .build()?;
        Ok(Arc::new(Self { inner, anchor_subjects, ct_min_operators }))
    }
}

/// Combine webpki's chain verdict, the stapled-OCSP verdict and the user's
/// "Proceed anyway" override into the handshake result.
///
/// A6: a bypass turns a chain failure into a pass. BUG-1615: it never
/// overrides `Revoked` — a revoked certificate is not a configuration
/// mistake the user can accept (the bypass registry also refuses HSTS hosts,
/// RFC 6797 §8.4, see [`bypass::allow_host`]).
fn apply_bypass_policy(
    chain_result: Result<ServerCertVerified, RustlsError>,
    ocsp_verdict: OcspVerdict,
    bypassed: bool,
) -> Result<ServerCertVerified, RustlsError> {
    if ocsp_verdict == OcspVerdict::Revoked {
        return Err(RustlsError::InvalidCertificate(CertificateError::Revoked));
    }
    match chain_result {
        Err(_) if bypassed => Ok(ServerCertVerified::assertion()),
        other => other,
    }
}

/// COMPAT-TRUST-CTLOGS: turn a chain that passed webpki into a CT failure when the SCTs of
/// `found` distinct operators fall short of `required`. A user's "Proceed anyway" for the
/// host overrides it like any other chain failure.
fn apply_ct_requirement(
    result: Result<ServerCertVerified, RustlsError>,
    found: usize,
    required: Option<usize>,
    bypassed: bool,
) -> Result<ServerCertVerified, RustlsError> {
    match (&result, required) {
        (Ok(_), Some(required)) if found < required && !bypassed => Err(RustlsError::InvalidCertificate(
            CertificateError::Other(rustls::OtherError(Arc::new(CtInsufficient { found, required }))),
        )),
        _ => result,
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
        // A3: layer the stapled-OCSP revocation check on top of webpki's chain trust.
        // Soft-fail everywhere but `Revoked` — see `ocsp` module docs for the full policy.
        let ocsp_verdict = ocsp::parse_stapled_response(ocsp_response);
        let bypassed = bypass::is_allowed(&server_name_host(server_name));
        let mut result = apply_bypass_policy(chain_result, ocsp_verdict, bypassed);
        if let Some(required) = self.ct_min_operators {
            let found = ct::evaluate_ct(end_entity, intermediates.first().map(AsRef::as_ref)).operators();
            result = apply_ct_requirement(result, found, Some(required), bypassed);
        }
        if result.is_err() {
            // COMPAT-TRUST-CAUSES: the chain is gone once rustls reports the
            // bare reason, so keep what the cause classifier needs.
            let mut chain = vec![end_entity.clone()];
            chain.extend(intermediates.iter().cloned());
            let now_unix = i64::try_from(now.as_secs()).unwrap_or(i64::MAX);
            failure::record(&server_name_host(server_name), failure::summarize(&chain, &self.anchor_subjects, now_unix));
        }
        result
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

    #[test]
    fn bypass_does_not_override_revoked() {
        let chain = Ok(ServerCertVerified::assertion());
        assert!(matches!(
            apply_bypass_policy(chain, OcspVerdict::Revoked, true),
            Err(RustlsError::InvalidCertificate(CertificateError::Revoked))
        ));
        let bad_chain = Err(RustlsError::InvalidCertificate(CertificateError::Expired));
        assert!(matches!(
            apply_bypass_policy(bad_chain, OcspVerdict::Revoked, true),
            Err(RustlsError::InvalidCertificate(CertificateError::Revoked))
        ));
    }

    #[test]
    fn bypass_overrides_chain_failure_only_when_set() {
        let bad = || Err(RustlsError::InvalidCertificate(CertificateError::Expired));
        assert!(apply_bypass_policy(bad(), OcspVerdict::Good, true).is_ok());
        assert!(apply_bypass_policy(bad(), OcspVerdict::Good, false).is_err());
        assert!(apply_bypass_policy(Ok(ServerCertVerified::assertion()), OcspVerdict::Good, false).is_ok());
    }

    #[test]
    fn ct_requirement_rejects_a_chain_with_too_few_operators() {
        let ok = || Ok(ServerCertVerified::assertion());
        let err = apply_ct_requirement(ok(), 1, Some(2), false).unwrap_err();
        assert!(matches!(&err, RustlsError::InvalidCertificate(CertificateError::Other(_))), "{err:?}");
        assert!(apply_ct_requirement(ok(), 2, Some(2), false).is_ok());
        assert!(apply_ct_requirement(ok(), 0, None, false).is_ok());
    }

    #[test]
    fn ct_requirement_is_waived_by_a_bypass_and_never_hides_an_earlier_failure() {
        assert!(apply_ct_requirement(Ok(ServerCertVerified::assertion()), 0, Some(2), true).is_ok());
        let expired = Err(RustlsError::InvalidCertificate(CertificateError::Expired));
        let kept = apply_ct_requirement(expired, 0, Some(2), false).unwrap_err();
        assert!(matches!(kept, RustlsError::InvalidCertificate(CertificateError::Expired)));
    }

    #[test]
    fn ct_failure_surfaces_as_a_cert_error_other() {
        let err = apply_ct_requirement(Ok(ServerCertVerified::assertion()), 0, Some(2), false).unwrap_err();
        let io = std::io::Error::new(std::io::ErrorKind::InvalidData, err);
        let Some(lumen_core::error::CertError::Other(text)) = crate::tls::cert_error::from_io_error(&io) else {
            panic!("expected CertError::Other");
        };
        assert!(text.contains("0 log operator(s), 2 required"), "{text}");
    }
}
