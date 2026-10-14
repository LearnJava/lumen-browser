//! Trust roots from the operating system store (COMPAT-TRUST-OSROOTS,
//! ADR-034, `docs/research/site-compat/report.md` §6.2, decision Р5).
//!
//! Before this module the trust set was `webpki-roots` + `LUMEN_EXTRA_CA_CERT`
//! only. With Kaspersky or ESET HTTPS inspection (the antivirus re-signs every
//! site with its own root installed into the Windows store) *every* site was
//! `UnknownIssuer`, and the only way out was a per-host "Proceed anyway" until
//! restart. Chrome 105+ and Firefox 120+ trust roots the user or the
//! administrator installed; so do we.
//!
//! What is read (Windows only; other platforms contribute nothing):
//! - `ROOT` in `CurrentUser`, `LocalMachine`, the two group-policy locations and
//!   `LocalMachineEnterprise` — each certificate becomes a trust anchor;
//! - `Disallowed` in the same locations — an anchor with the same subject and
//!   public key is removed from the final set, *including* a built-in
//!   `webpki-roots` one (the administrator's explicit distrust wins).
//!
//! What is deliberately **not** read:
//! - the `CA` store. It holds *intermediates*; a user-writable `CurrentUser\CA`
//!   promoted to anchors would let any process add trust without the Windows
//!   confirmation prompt that `CurrentUser\ROOT` triggers. A proxy that
//!   re-signs with an intermediate absent from the chain it sends is still
//!   `UnknownIssuer` — a known gap, solvable only by AIA chasing;
//! - the НУЦ (Russian Trusted Root/Sub CA) roots: a root with such a subject
//!   found in the OS store is not trusted until COMPAT-TRUST-NUC (ADR-034 п.4,
//!   a separate decision with CT requirement). See [`is_nuc_subject`].
//!
//! The OS store is read **once per process** (cached): a root installed while
//! the browser runs is picked up after restart, the same as `LUMEN_EXTRA_CA_CERT`.
//! QUIC/HTTP/3 keeps the Mozilla list only (`h3::mozilla_roots`); a failed QUIC
//! handshake falls back to TCP (`try_h3_dispatch`), where the OS roots apply.

use std::sync::OnceLock;

use rustls::pki_types::CertificateDer;
use rustls::RootCertStore;

use super::cert_fields;

/// Subject markers of the Russian state CA roots (НУЦ Минцифры). Matched as a
/// byte substring of the DER `Name`, which stores `commonName` as plain ASCII.
/// COMPAT-TRUST-NUC replaces this with the SPKI list once the keys are pinned.
const NUC_SUBJECT_MARKERS: [&[u8]; 2] = [b"Russian Trusted Root CA", b"Russian Trusted Sub CA"];

/// Raw OS trust data: certificates as read, before any policy is applied.
#[derive(Debug, Default, Clone)]
pub struct OsTrust {
    /// DER certificates from the `ROOT` stores.
    pub trusted: Vec<CertificateDer<'static>>,
    /// DER certificates from the `Disallowed` stores.
    pub disallowed: Vec<CertificateDer<'static>>,
}

/// A root that exists only in the OS store (not in `webpki-roots`): the name
/// shown in the certificate panel when a connection is anchored by it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalRoot {
    /// `Name` contents of the root's subject (see [`cert_fields::issuer_and_subject_der`]).
    subject: Vec<u8>,
    name: String,
}

/// Locally installed roots that actually joined the trust set.
#[derive(Debug, Default)]
pub struct LocalRoots {
    entries: Vec<LocalRoot>,
}

/// Whether a trust anchor `subject` (DER `Name` contents) belongs to the
/// Russian state CA. Such a root from the OS store is not trusted yet.
pub fn is_nuc_subject(subject: &[u8]) -> bool {
    NUC_SUBJECT_MARKERS
        .iter()
        .any(|marker| subject.windows(marker.len()).any(|w| w == *marker))
}

/// `(subject, spki)` identity of an anchor, the key `Disallowed` matches by.
fn anchor_key(anchor: &rustls::pki_types::TrustAnchor<'_>) -> (Vec<u8>, Vec<u8>) {
    (anchor.subject.as_ref().to_vec(), anchor.subject_public_key_info.as_ref().to_vec())
}

/// Parse one DER certificate into a trust anchor, `None` if unparsable.
fn parse_anchor(der: &CertificateDer<'static>) -> Option<rustls::pki_types::TrustAnchor<'static>> {
    let mut one = RootCertStore::empty();
    one.add(der.clone()).ok()?;
    one.roots.pop()
}

/// The trusted OS certificates that survive policy: parsable, not НУЦ, not in
/// `Disallowed`. Paired with their DER (needed for the display name).
fn accepted(trust: &OsTrust) -> Vec<(rustls::pki_types::TrustAnchor<'static>, &CertificateDer<'static>)> {
    let denied: Vec<_> =
        trust.disallowed.iter().filter_map(parse_anchor).map(|a| anchor_key(&a)).collect();
    trust
        .trusted
        .iter()
        .filter_map(|der| parse_anchor(der).map(|a| (a, der)))
        .filter(|(a, _)| !is_nuc_subject(a.subject.as_ref()) && !denied.contains(&anchor_key(a)))
        .collect()
}

/// Add the OS roots to `store` and remove everything `Disallowed` names.
pub fn apply_os_trust(store: &mut RootCertStore, trust: &OsTrust) {
    for (anchor, _) in accepted(trust) {
        if !store.roots.iter().any(|a| anchor_key(a) == anchor_key(&anchor)) {
            store.roots.push(anchor);
        }
    }
    let denied: Vec<_> =
        trust.disallowed.iter().filter_map(parse_anchor).map(|a| anchor_key(&a)).collect();
    if !denied.is_empty() {
        store.roots.retain(|a| !denied.contains(&anchor_key(a)));
    }
}

impl LocalRoots {
    /// Roots that came from the OS store and are not already Mozilla roots.
    pub fn from_trust(trust: &OsTrust) -> Self {
        let mozilla: Vec<&[u8]> =
            webpki_roots::TLS_SERVER_ROOTS.iter().map(|r| r.subject.as_ref()).collect();
        let entries = accepted(trust)
            .into_iter()
            .filter(|(a, _)| !mozilla.contains(&a.subject.as_ref()))
            .map(|(a, der)| {
                let fields = cert_fields::extract_leaf_fields(der.as_ref()).unwrap_or_default();
                let name = [fields.subject_cn, fields.subject_org]
                    .into_iter()
                    .find(|s| !s.is_empty())
                    .unwrap_or_else(|| String::from("(без имени)"));
                LocalRoot { subject: a.subject.as_ref().to_vec(), name }
            })
            .collect();
        Self { entries }
    }

    /// The name of the locally installed root that anchors `chain` (as sent by
    /// the server, leaf first), `None` when the chain does not end at one.
    pub fn name_for_chain(&self, chain: &[CertificateDer<'_>]) -> Option<&str> {
        let (issuer, subject) = cert_fields::issuer_and_subject_der(chain.last()?.as_ref())?;
        self.entries
            .iter()
            .find(|e| e.subject == issuer || e.subject == subject)
            .map(|e| e.name.as_str())
    }
}

/// The OS trust data, read once per process.
fn os_trust() -> &'static OsTrust {
    static CACHE: OnceLock<OsTrust> = OnceLock::new();
    CACHE.get_or_init(read_os_trust)
}

/// Merge the cached OS roots into `store` (see the module docs).
pub fn merge_os_roots(store: &mut RootCertStore) {
    apply_os_trust(store, os_trust());
}

/// The name of the locally installed root that anchors `chain`, for the
/// certificate panel ("Проверено корнем, установленным на этом компьютере").
pub fn local_root_name(chain: &[CertificateDer<'_>]) -> Option<String> {
    static CACHE: OnceLock<LocalRoots> = OnceLock::new();
    CACHE.get_or_init(|| LocalRoots::from_trust(os_trust())).name_for_chain(chain).map(str::to_owned)
}

#[cfg(not(windows))]
fn read_os_trust() -> OsTrust {
    OsTrust::default()
}

#[cfg(windows)]
fn read_os_trust() -> OsTrust {
    win::read_all()
}

#[cfg(windows)]
mod win {
    use rustls::pki_types::CertificateDer;
    use windows_sys::Win32::Security::Cryptography::{
        CertCloseStore, CertEnumCertificatesInStore, CertOpenStore, CERT_CONTEXT,
        CERT_STORE_OPEN_EXISTING_FLAG, CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG,
        HCERTSTORE,
    };

    use super::OsTrust;

    /// `CERT_SYSTEM_STORE_*_ID << 16` — the store location flags
    /// (`wincrypt.h`; windows-sys exposes only the `_ID` halves).
    const LOCATIONS: [u32; 5] = [
        1 << 16, // CURRENT_USER
        2 << 16, // LOCAL_MACHINE
        7 << 16, // CURRENT_USER_GROUP_POLICY
        8 << 16, // LOCAL_MACHINE_GROUP_POLICY
        9 << 16, // LOCAL_MACHINE_ENTERPRISE
    ];

    pub(super) fn read_all() -> OsTrust {
        let mut trust = OsTrust::default();
        for location in LOCATIONS {
            trust.trusted.extend(read_system_store(location, "ROOT"));
            trust.disallowed.extend(read_system_store(location, "Disallowed"));
        }
        trust
    }

    /// All certificates of the system store `name` at `location`; an absent
    /// store (e.g. no group-policy store) is simply empty.
    fn read_system_store(location: u32, name: &str) -> Vec<CertificateDer<'static>> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = location | CERT_STORE_OPEN_EXISTING_FLAG | CERT_STORE_READONLY_FLAG;
        // SAFETY: `wide` is a NUL-terminated UTF-16 string alive for the call;
        // CERT_STORE_PROV_SYSTEM_W reads it as the store name; a null result is checked.
        let store = unsafe {
            CertOpenStore(CERT_STORE_PROV_SYSTEM_W, 0, 0, flags, wide.as_ptr().cast())
        };
        if store.is_null() {
            return Vec::new();
        }
        let certs = enumerate(store);
        // SAFETY: `store` is a valid handle opened above and not used afterwards.
        unsafe { CertCloseStore(store, 0) };
        certs
    }

    /// Copy every certificate out of an open store handle.
    pub(super) fn enumerate(store: HCERTSTORE) -> Vec<CertificateDer<'static>> {
        let mut certs = Vec::new();
        let mut prev: *const CERT_CONTEXT = std::ptr::null();
        loop {
            // SAFETY: `store` is a valid open store; `prev` is null or the context
            // returned by the previous call, which this call frees.
            let ctx = unsafe { CertEnumCertificatesInStore(store, prev) };
            if ctx.is_null() {
                break;
            }
            // SAFETY: a non-null context points to a CERT_CONTEXT whose `pbCertEncoded`
            // addresses `cbCertEncoded` readable bytes until the next enumeration call.
            let der = unsafe {
                std::slice::from_raw_parts((*ctx).pbCertEncoded, (*ctx).cbCertEncoded as usize)
            };
            certs.push(CertificateDer::from(der.to_vec()));
            prev = ctx;
        }
        certs
    }

    #[cfg(test)]
    pub(super) fn memory_store_with(ders: &[&[u8]]) -> Vec<CertificateDer<'static>> {
        use windows_sys::Win32::Security::Cryptography::{
            CertAddEncodedCertificateToStore, CERT_STORE_ADD_ALWAYS, CERT_STORE_PROV_MEMORY,
            X509_ASN_ENCODING,
        };
        // SAFETY: an in-memory store takes no provider parameter; null is checked below.
        let store = unsafe { CertOpenStore(CERT_STORE_PROV_MEMORY, 0, 0, 0, std::ptr::null()) };
        assert!(!store.is_null(), "memory store must open");
        for der in ders {
            // SAFETY: `store` is valid; `der` is a readable slice of the stated length.
            let ok = unsafe {
                CertAddEncodedCertificateToStore(
                    store,
                    X509_ASN_ENCODING,
                    der.as_ptr(),
                    der.len() as u32,
                    CERT_STORE_ADD_ALWAYS,
                    std::ptr::null_mut(),
                )
            };
            assert!(ok != 0, "certificate must be added");
        }
        let certs = enumerate(store);
        // SAFETY: valid handle, not used afterwards.
        unsafe { CertCloseStore(store, 0) };
        certs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WPT's self-signed test cert (CN=127.0.0.1, its own issuer) — the same
    /// fixture `tests/cases/bug785_extra_ca.rs` inlines.
    const TEST_CERT_PEM: &str = include_str!("../../../../tests/wpt/certs/ca-cert.pem");

    fn test_cert() -> CertificateDer<'static> {
        rustls_pemfile::certs(&mut TEST_CERT_PEM.as_bytes())
            .next()
            .expect("fixture has a certificate")
            .expect("fixture parses")
    }

    fn trust(trusted: &[CertificateDer<'static>], disallowed: &[CertificateDer<'static>]) -> OsTrust {
        OsTrust { trusted: trusted.to_vec(), disallowed: disallowed.to_vec() }
    }

    #[test]
    fn os_root_joins_the_store() {
        let mut store = RootCertStore::empty();
        apply_os_trust(&mut store, &trust(&[test_cert()], &[]));
        assert_eq!(store.roots.len(), 1);
    }

    #[test]
    fn unparsable_der_is_skipped() {
        let mut store = RootCertStore::empty();
        let junk = CertificateDer::from(vec![1, 2, 3]);
        apply_os_trust(&mut store, &trust(&[junk, test_cert()], &[]));
        assert_eq!(store.roots.len(), 1);
    }

    #[test]
    fn disallowed_cert_is_not_added() {
        let mut store = RootCertStore::empty();
        apply_os_trust(&mut store, &trust(&[test_cert()], &[test_cert()]));
        assert!(store.roots.is_empty());
    }

    #[test]
    fn disallowed_removes_an_existing_anchor() {
        let mut store = RootCertStore::empty();
        store.add(test_cert()).expect("add");
        apply_os_trust(&mut store, &trust(&[], &[test_cert()]));
        assert!(store.roots.is_empty());
    }

    #[test]
    fn nuc_subject_is_recognised() {
        assert!(is_nuc_subject(b"\x30\x1f\x06\x03\x55\x04\x03\x0c\x17Russian Trusted Root CA"));
        assert!(is_nuc_subject(b"...Russian Trusted Sub CA..."));
        assert!(!is_nuc_subject(b"CN=127.0.0.1"));
    }

    #[test]
    fn local_root_is_named_for_its_chain() {
        let roots = LocalRoots::from_trust(&trust(&[test_cert()], &[]));
        // The fixture is its own issuer, so a chain of just that cert is anchored by it.
        assert_eq!(roots.name_for_chain(&[test_cert()]), Some("127.0.0.1"));
        assert_eq!(roots.name_for_chain(&[]), None);
    }

    #[test]
    fn no_local_root_means_no_name() {
        let roots = LocalRoots::from_trust(&OsTrust::default());
        assert_eq!(roots.name_for_chain(&[test_cert()]), None);
    }

    #[test]
    fn issuer_and_subject_of_self_signed_cert_are_equal() {
        let der = test_cert();
        let (issuer, subject) = cert_fields::issuer_and_subject_der(der.as_ref()).expect("parse");
        assert_eq!(issuer, subject);
        assert!(!issuer.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn windows_store_enumeration_round_trips() {
        let der = test_cert();
        let read = win::memory_store_with(&[der.as_ref()]);
        assert_eq!(read, vec![der]);
    }

    #[cfg(windows)]
    #[test]
    fn real_system_stores_are_readable() {
        // CI machines have the stock Windows ROOT store; at minimum it must not panic
        // and the cached loader must be callable.
        let trust = os_trust();
        assert!(trust.trusted.len() < 10_000);
    }
}
