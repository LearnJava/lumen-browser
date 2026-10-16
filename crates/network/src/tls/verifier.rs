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

use super::{aia, bypass, ct, failure};
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

impl LumenVerifier {
    /// Re-run the webpki verification after finding the issuers the server left out
    /// (COMPAT-TRUST-AIA). Each round looks up the issuer of the chain's top certificate —
    /// [`aia::Aia::issuers_of`] — and verifies again; it stops at the first outcome other
    /// than `UnknownIssuer`. Returns that outcome with every certificate found, or `None`
    /// when no issuer could be found at all (the caller keeps its `UnknownIssuer`).
    fn verify_with_issuers(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Option<(Result<ServerCertVerified, RustlsError>, Vec<CertificateDer<'static>>)> {
        let mut pool: Vec<CertificateDer<'_>> = intermediates.to_vec();
        let mut found: Vec<CertificateDer<'static>> = Vec::new();
        let mut result = None;
        for _ in 0..aia::MAX_LOOKUPS {
            let top = aia::chain_top(end_entity.as_ref(), &pool)?.to_vec();
            let issuers: Vec<_> =
                aia::global().issuers_of(&top).into_iter().filter(|c| !pool.contains(c)).collect();
            if issuers.is_empty() {
                break;
            }
            pool.extend(issuers.iter().cloned());
            found.extend(issuers);
            let attempt = self.inner.verify_server_cert(end_entity, &pool, server_name, ocsp_response, now);
            let again = matches!(&attempt, Err(RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)));
            result = Some(attempt);
            if !again {
                break;
            }
        }
        result.map(|r| (r, found))
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
        let mut chain_result =
            self.inner
                .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now);
        // COMPAT-TRUST-AIA: a server that did not send its intermediate. Look the missing
        // certificates up (OS `CA` store, then the leaf's AIA URL) and verify again; the
        // extended list also feeds the CT check below.
        let mut found_intermediates = Vec::new();
        let unknown_issuer =
            matches!(&chain_result, Err(RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)));
        if unknown_issuer
            && aia::enabled()
            && let Some((result, found)) =
                self.verify_with_issuers(end_entity, intermediates, server_name, ocsp_response, now)
        {
            chain_result = result;
            found_intermediates = found;
        }
        let sent_count = intermediates.len();
        let full_chain: Vec<CertificateDer<'_>> =
            intermediates.iter().cloned().chain(found_intermediates).collect();
        let intermediates = full_chain.as_slice();
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
            // bare reason, so keep what the cause classifier needs — the chain
            // as the server sent it, not as completed from AIA.
            let mut chain = vec![end_entity.clone()];
            chain.extend(intermediates.iter().take(sent_count).cloned());
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

    // ── COMPAT-TRUST-AIA ────────────────────────────────────────────────────

    use super::super::aia::tests::{
        pem_to_der, serve, INTERMEDIATE_DER, INTERMEDIATE_P7C, LEAF_P7C_PEM, LEAF_PEM, ROOT_PEM, TEST_ADDR,
    };

    /// A verifier that trusts the AIA fixtures' root and nothing else.
    fn aia_verifier(trust_root: bool) -> Arc<LumenVerifier> {
        let mut store = RootCertStore::empty();
        if trust_root {
            store.add(CertificateDer::from(pem_to_der(ROOT_PEM))).expect("fixture root");
        } else {
            store.roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().take(1).cloned());
        }
        LumenVerifier::new(store, &provider()).expect("verifier")
    }

    fn verify_leaf(verifier: &LumenVerifier, leaf_pem: &str, sent: &[CertificateDer<'_>]) -> Result<ServerCertVerified, RustlsError> {
        let leaf = CertificateDer::from(pem_to_der(leaf_pem));
        let name = ServerName::try_from("aia.test").expect("name");
        verifier.verify_server_cert(&leaf, sent, &name, &[], UnixTime::now())
    }

    #[test]
    fn aia_completes_a_chain_without_the_intermediate_and_caches_it() {
        let (addr, hits) = serve("200 OK", INTERMEDIATE_DER.to_vec());
        TEST_ADDR.with(|a| a.set(Some(addr)));
        let verifier = aia_verifier(true);
        assert!(verify_leaf(&verifier, LEAF_PEM, &[]).is_ok());
        assert!(verify_leaf(&verifier, LEAF_PEM, &[]).is_ok(), "second visit");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1, "second visit must not download");
    }

    #[test]
    fn aia_accepts_a_pkcs7_response() {
        let (addr, hits) = serve("200 OK", INTERMEDIATE_P7C.to_vec());
        TEST_ADDR.with(|a| a.set(Some(addr)));
        assert!(verify_leaf(&aia_verifier(true), LEAF_P7C_PEM, &[]).is_ok());
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn aia_does_not_trust_what_it_downloads() {
        // The intermediate is found, but its root is not a trust anchor here.
        let (addr, hits) = serve("200 OK", INTERMEDIATE_DER.to_vec());
        TEST_ADDR.with(|a| a.set(Some(addr)));
        let err = verify_leaf(&aia_verifier(false), include_str!("testdata/aia/leaf3.pem"), &[]).unwrap_err();
        assert!(matches!(err, RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)), "{err:?}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_chain_the_server_sent_in_full_never_touches_the_network() {
        // Nothing listens: a fetch would fail the test by timing out, not by an assert.
        TEST_ADDR.with(|a| a.set(Some("127.0.0.1:1".parse().expect("addr"))));
        let sent = [CertificateDer::from(INTERMEDIATE_DER.to_vec())];
        assert!(verify_leaf(&aia_verifier(true), LEAF_PEM, &sent).is_ok());
    }
}
