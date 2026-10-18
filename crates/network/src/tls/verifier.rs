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

use super::{aia, bypass, cert_fields, ct, failure, nuc};
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
    /// COMPAT-TRUST-NUC: the second pass against the national CA root. `None` only if the
    /// embedded root fails its pin (then nothing is ever accepted through it).
    nuc: Option<NucPolicy>,
}

/// The second verification pass for chains that end at the Russian state CA ([`nuc`]).
#[derive(Debug)]
struct NucPolicy {
    /// webpki verifier whose only anchor is the pinned national root.
    verifier: Arc<WebPkiServerVerifier>,
    /// Distinct log operators with a valid SCT for `(leaf, direct issuer)`; replaceable in tests.
    operators: fn(&[u8], Option<&[u8]>) -> usize,
}

fn operators_of(leaf: &[u8], issuer: Option<&[u8]>) -> usize {
    ct::evaluate_ct(leaf, issuer).operators()
}

/// The chain was valid but its SCTs did not reach the operator count the verifier requires.
#[derive(Debug)]
pub(super) struct CtInsufficient {
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
        let nuc = WebPkiServerVerifier::builder_with_provider(Arc::new(nuc::root_store()), provider.clone())
            .build()
            .ok()
            .map(|verifier| NucPolicy { verifier, operators: operators_of });
        Ok(Arc::new(Self { inner, anchor_subjects, ct_min_operators, nuc }))
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

/// The certificate in `intermediates` that issued `leaf` (by name), else the first one — the
/// issuer a SCT signature is checked against.
fn direct_issuer<'a>(leaf: &[u8], intermediates: &'a [CertificateDer<'_>]) -> Option<&'a [u8]> {
    let wanted = cert_fields::issuer_and_subject_der(leaf).map(|(issuer, _)| issuer);
    intermediates
        .iter()
        .map(AsRef::as_ref)
        .find(|c| wanted.is_some() && cert_fields::issuer_and_subject_der(c).map(|(_, subject)| subject) == wanted)
        .or_else(|| intermediates.first().map(AsRef::as_ref))
}

/// Verdict of the national-CA pass. `None` — the chain does not lead to that root either, the
/// ordinary `UnknownIssuer` stands; a chain that does but fails for another reason (expired, wrong
/// name) reports that reason; a chain that verifies is accepted only with SCTs of
/// [`nuc::MIN_OPERATORS`] distinct operators.
fn decide_nuc(
    attempt: Result<ServerCertVerified, RustlsError>,
    operators: impl FnOnce() -> usize,
) -> Option<Result<ServerCertVerified, RustlsError>> {
    match attempt {
        Err(RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)) => None,
        Err(other) => Some(Err(other)),
        Ok(ok) => {
            let found = operators();
            Some(apply_ct_requirement(Ok(ok), found, Some(nuc::MIN_OPERATORS), false))
        }
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
        // COMPAT-TRUST-NUC: still no known issuer — try the national CA, but only for a host the
        // user agreed to and only with Certificate Transparency evidence.
        let still_unknown =
            matches!(&chain_result, Err(RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)));
        if still_unknown
            && let Some(nuc) = &self.nuc
            && nuc::has_consent(&server_name_host(server_name))
        {
            let attempt =
                nuc.verifier.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now);
            let count = || (nuc.operators)(end_entity, direct_issuer(end_entity, intermediates));
            if let Some(verdict) = decide_nuc(attempt, count) {
                chain_result = verdict;
            }
        }
        // A3: layer the stapled-OCSP revocation check on top of webpki's chain trust.
        // Soft-fail everywhere but `Revoked` — see `ocsp` module docs for the full policy.
        let ocsp_verdict = ocsp::parse_stapled_response(ocsp_response);
        let host = server_name_host(server_name);
        // BUG-1618: the cert panel reads the verdict back by host; the connection never sees the staple.
        ocsp::record_stapled(&host, ocsp_verdict);
        let bypassed = bypass::is_allowed(&host);
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
    fn ct_failure_surfaces_as_cert_error_ct_insufficient() {
        let err = apply_ct_requirement(Ok(ServerCertVerified::assertion()), 0, Some(2), false).unwrap_err();
        let io = std::io::Error::new(std::io::ErrorKind::InvalidData, err);
        assert_eq!(crate::tls::cert_error::from_io_error(&io), Some(lumen_core::error::CertError::CtInsufficient));
        assert_eq!(
            CtInsufficient { found: 0, required: 2 }.to_string(),
            "Certificate Transparency: valid SCTs from 0 log operator(s), 2 required"
        );
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

    // ── COMPAT-TRUST-NUC ────────────────────────────────────────────────────

    /// 2026-10-14: inside the captured sberbank.ru leaf (2026-07-27 … 2027-07-27) and sub CA.
    const NUC_NOW_SECS: u64 = 1_791_979_200;

    fn sber_chain() -> Vec<CertificateDer<'static>> {
        rustls_pemfile::certs(&mut include_str!("testdata/sberbank_chain.pem").as_bytes())
            .map(|c| c.expect("pem"))
            .collect()
    }

    /// Verify the captured chain for `host`; nothing may listen where AIA would fetch from.
    fn verify_sber(verifier: &LumenVerifier, host: &str, sent: &[CertificateDer<'_>]) -> Result<ServerCertVerified, RustlsError> {
        TEST_ADDR.with(|a| a.set(Some("127.0.0.1:1".parse().expect("addr"))));
        let chain = sber_chain();
        let name = ServerName::try_from(host).expect("name");
        let now = UnixTime::since_unix_epoch(std::time::Duration::from_secs(NUC_NOW_SECS));
        verifier.verify_server_cert(&chain[0], sent, &name, &[], now)
    }

    fn nuc_verifier(operators: fn(&[u8], Option<&[u8]>) -> usize) -> Arc<LumenVerifier> {
        let mut verifier = LumenVerifier::new(crate::tls::trusted_root_store(), &provider()).expect("verifier");
        Arc::get_mut(&mut verifier).expect("unique").nuc.as_mut().expect("embedded root is pinned").operators =
            operators;
        verifier
    }

    #[test]
    fn nuc_chain_needs_the_users_consent_then_ct_from_two_operators() {
        // sberbank.ru (the captured leaf's name); one test owns the host so parallel tests cannot interfere.
        let host = "sberbank.ru";
        nuc::revoke_host(host);
        let sent = &sber_chain()[1..];
        let real = nuc_verifier(operators_of);

        // No consent: the ordinary verdict stands, the error screen can offer the consent.
        let err = verify_sber(&real, host, sent).unwrap_err();
        assert!(matches!(err, RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)), "{err:?}");

        // Consent and the real SCTs (three operators): accepted.
        nuc::consent_host(host);
        assert!(verify_sber(&real, host, sent).is_ok());

        // One operator is not enough, two is.
        let one = nuc_verifier(|_, _| 1);
        let err = verify_sber(&one, host, sent).unwrap_err();
        assert_eq!(
            crate::tls::cert_error::from_io_error(&std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
            Some(lumen_core::error::CertError::CtInsufficient)
        );
        assert!(verify_sber(&nuc_verifier(|_, _| 2), host, sent).is_ok());

        // Consent is withdrawn: refused again.
        nuc::revoke_host(host);
        assert!(verify_sber(&real, host, sent).is_err());
    }

    #[test]
    fn nuc_consent_for_one_host_does_not_open_another() {
        nuc::consent_host("other-bank.nuc-test.example");
        let err = verify_sber(&nuc_verifier(operators_of), "www.sberbank.ru", &sber_chain()[1..]).unwrap_err();
        assert!(matches!(err, RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)), "{err:?}");
        nuc::revoke_host("other-bank.nuc-test.example");
    }

    #[test]
    fn nuc_consent_does_not_hide_a_wrong_name() {
        let host = "wrong-name.nuc-test.example";
        nuc::consent_host(host);
        let err = verify_sber(&nuc_verifier(operators_of), host, &sber_chain()[1..]).unwrap_err();
        assert!(
            matches!(err, RustlsError::InvalidCertificate(CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. })),
            "{err:?}"
        );
        nuc::revoke_host(host);
    }

    #[test]
    fn nuc_consent_does_not_trust_an_unrelated_chain() {
        // The AIA fixtures' root is not the national CA: consent changes nothing for its chain.
        nuc::consent_host("aia.test");
        let err = verify_leaf(&aia_verifier(false), LEAF_PEM, &[]).unwrap_err();
        assert!(matches!(err, RustlsError::InvalidCertificate(CertificateError::UnknownIssuer)), "{err:?}");
        nuc::revoke_host("aia.test");
    }

    #[test]
    fn direct_issuer_prefers_the_certificate_that_issued_the_leaf() {
        let chain = sber_chain();
        let reversed = [chain[1].clone(), chain[1].clone()];
        assert_eq!(direct_issuer(chain[0].as_ref(), &chain[1..]), Some(chain[1].as_ref()));
        assert!(direct_issuer(chain[0].as_ref(), &reversed).is_some());
        assert_eq!(direct_issuer(chain[0].as_ref(), &[]), None);
    }
}
