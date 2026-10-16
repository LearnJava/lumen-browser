//! Trust in the Russian state CA (НУЦ Минцифры), COMPAT-TRUST-NUC, ADR-034 §3.
//!
//! Since 2026-08-03 the large Russian banks and state services serve certificates chained to
//! `Russian Trusted Root CA`. It is in no public root program, so before this module every such
//! site was `UnknownIssuer` and the only way in was a blind "Proceed anyway".
//!
//! The root is **not** a normal trust anchor. [`super::verifier::LumenVerifier`] makes a second
//! pass, after the ordinary one (Mozilla roots, OS roots, AIA) ended in `UnknownIssuer`, against
//! this one anchor, and accepts the chain only when
//! 1. the user consented to the national CA for this exact host ([`consent_host`]; the shell
//!    offers it on the error screen and stores the list in the profile), and
//! 2. SCTs of at least [`MIN_OPERATORS`] distinct log operators verify ([`super::ct::evaluate_ct`]),
//!    so a certificate the national CA issued secretly for the host is refused.
//!
//! The root is compiled in and checked against [`ROOT_SPKI_SHA256`] before use; if the embedded
//! bytes ever stop matching the pin the anchor set is empty and nothing is accepted. A copy of
//! this root found in the OS store is recognised by the same pin and is not given extra rights
//! (`os_roots` keeps it out of the trust set). Lumen never writes a root into the OS.

use std::collections::BTreeSet;
use std::sync::{OnceLock, RwLock};

use rustls::pki_types::CertificateDer;
use rustls::RootCertStore;
use sha2::{Digest, Sha256};

use super::cert_fields;

/// Distinct CT log operators whose valid SCTs a НУЦ chain must carry.
pub const MIN_OPERATORS: usize = 2;

/// `Russian Trusted Root CA` (RSA 4096, valid 2022-03-01 … 2032-02-27), DER, from
/// <https://gu-st.ru/content/lending/russian_trusted_root_ca_pem.crt>.
const ROOT_DER: &[u8] = include_bytes!("nuc_root.der");

/// SHA-256 of the root's `subjectPublicKeyInfo` (the `openssl x509 -pubkey | openssl pkey
/// -pubin -outform DER | sha256sum` value). The certificate itself is replaceable (re-issue
/// with the same key); the key is the identity.
pub const ROOT_SPKI_SHA256: [u8; 32] = [
    0x02, 0xb8, 0x22, 0x0c, 0x07, 0x07, 0x28, 0xdb, 0x77, 0x1d, 0x9a, 0xc5, 0x9e, 0x54, 0x52, 0x1c, 0x4e, 0xdd, 0xad,
    0x21, 0xa7, 0x83, 0xbb, 0x26, 0xcf, 0xdf, 0x19, 0xc1, 0xdb, 0x0f, 0xae, 0x37,
];

/// Whether a DER certificate carries the pinned national-CA key.
pub fn is_pinned_cert(cert_der: &[u8]) -> bool {
    cert_fields::spki_der(cert_der).is_some_and(|spki| Sha256::digest(spki).as_slice() == ROOT_SPKI_SHA256)
}

/// The one trust anchor of the second pass: the embedded root, or an empty store if the
/// embedded bytes do not match the pin (fail closed).
pub fn root_store() -> RootCertStore {
    let mut store = RootCertStore::empty();
    if is_pinned_cert(ROOT_DER) {
        let _ = store.add(CertificateDer::from(ROOT_DER));
    }
    store
}

/// Whether `chain` (leaf first) leads to the national CA by name: some certificate is issued
/// by the pinned root's subject. A label for the certificate panel — acceptance is decided by
/// signature verification in the verifier, never by this.
pub fn chain_names_root(chain: &[CertificateDer<'_>]) -> bool {
    static ROOT_SUBJECT: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let Some(root) = ROOT_SUBJECT
        .get_or_init(|| cert_fields::issuer_and_subject_der(ROOT_DER).map(|(_, subject)| subject.to_vec()))
        .as_deref()
    else {
        return false;
    };
    chain.iter().any(|c| cert_fields::issuer_and_subject_der(c.as_ref()).is_some_and(|(issuer, _)| issuer == root))
}

fn consents() -> &'static RwLock<BTreeSet<String>> {
    static CONSENTS: OnceLock<RwLock<BTreeSet<String>>> = OnceLock::new();
    CONSENTS.get_or_init(|| RwLock::new(BTreeSet::new()))
}

/// Hosts compare as lowercase DNS names without a trailing dot.
fn normalize(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Record the user's consent to the national CA for `host` (exact name, no subdomains).
pub fn consent_host(host: &str) {
    let host = normalize(host);
    if !host.is_empty()
        && let Ok(mut set) = consents().write()
    {
        set.insert(host);
    }
}

/// Withdraw the consent for `host`.
pub fn revoke_host(host: &str) {
    if let Ok(mut set) = consents().write() {
        set.remove(&normalize(host));
    }
}

/// Withdraw the consent for `site` and every host under it (clearing the site's data).
pub fn revoke_site(site: &str) {
    let site = normalize(site);
    let suffix = format!(".{site}");
    if let Ok(mut set) = consents().write() {
        set.retain(|h| *h != site && !h.ends_with(&suffix));
    }
}

/// Withdraw every consent (clearing the history).
pub fn revoke_all() {
    load_consents("");
}

/// Whether the user consented to the national CA for `host`.
pub fn has_consent(host: &str) -> bool {
    consents().read().is_ok_and(|set| set.contains(&normalize(host)))
}

fn render(set: &BTreeSet<String>) -> String {
    set.iter().map(String::as_str).collect::<Vec<_>>().join("
")
}

fn parse(text: &str) -> BTreeSet<String> {
    text.lines().map(normalize).filter(|h| !h.is_empty()).collect()
}

/// The consented hosts as one name per line, sorted — the form the profile stores.
pub fn consents_to_text() -> String {
    consents().read().map(|set| render(&set)).unwrap_or_default()
}

/// Replace the consents with the stored `text` (one host per line; blanks ignored).
pub fn load_consents(text: &str) {
    if let Ok(mut set) = consents().write() {
        *set = parse(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_root_matches_its_pin() {
        assert!(is_pinned_cert(ROOT_DER));
        assert_eq!(root_store().roots.len(), 1);
    }

    #[test]
    fn another_certificate_does_not_match_the_pin() {
        let leaf = rustls_pemfile::certs(&mut include_str!("testdata/sberbank_chain.pem").as_bytes())
            .next()
            .expect("leaf")
            .expect("pem");
        assert!(!is_pinned_cert(leaf.as_ref()));
        assert!(!is_pinned_cert(b"not a certificate"));
    }

    #[test]
    fn a_chain_issued_by_the_root_names_it() {
        let chain: Vec<_> = rustls_pemfile::certs(&mut include_str!("testdata/sberbank_chain.pem").as_bytes())
            .map(|c| c.expect("pem"))
            .collect();
        assert!(chain_names_root(&chain), "the sub CA is issued by the root");
        assert!(!chain_names_root(&chain[..1]), "the leaf is issued by the sub CA");
        assert!(!chain_names_root(&[]));
    }

    // The registry is process-global and tests run in parallel: each test uses its own host names.
    #[test]
    fn consent_is_per_exact_host_and_normalised() {
        assert!(!has_consent("online.nuc-test.example"));
        consent_host(" Online.NUC-Test.EXAMPLE. ");
        assert!(has_consent("online.nuc-test.example"));
        assert!(!has_consent("nuc-test.example"), "no parent domain");
        assert!(!has_consent("evil.online.nuc-test.example"), "no subdomains");
        revoke_host("ONLINE.nuc-test.example");
        assert!(!has_consent("online.nuc-test.example"));
    }

    #[test]
    fn revoking_a_site_takes_its_subdomains_with_it() {
        consent_host("a.revoke-test.example");
        consent_host("revoke-test.example");
        consent_host("notrevoke-test.example");
        revoke_site("revoke-test.example");
        assert!(!has_consent("a.revoke-test.example") && !has_consent("revoke-test.example"));
        assert!(has_consent("notrevoke-test.example"), "a longer name is another site");
        revoke_host("notrevoke-test.example");
    }

    #[test]
    fn consents_round_trip_through_the_profile_text() {
        let set = parse("b.example

  
A.example.
");
        assert_eq!(render(&set), "a.example
b.example");
        assert_eq!(parse(&render(&set)), set);
        consent_host("stored.nuc-test.example");
        assert!(consents_to_text().lines().any(|h| h == "stored.nuc-test.example"));
        revoke_host("stored.nuc-test.example");
    }
}
