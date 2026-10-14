//! Evidence about a rejected certificate chain (COMPAT-TRUST-CAUSES).
//!
//! `rustls` reports a chain failure as a bare reason (`UnknownIssuer`,
//! `Expired`, ...) and drops the chain. The shell needs the chain to tell an
//! antivirus that re-signs HTTPS, a Russian state CA, a server that forgot its
//! intermediate and a wrong system clock apart — all of which look like the
//! same "not trusted" or "expired". [`super::verifier::LumenVerifier`] is the
//! only place that sees the chain, so on failure it [`record`]s a
//! [`ChainEvidence`] keyed by host, and the shell [`take`]s it when the
//! navigation error arrives. A side channel instead of a field on
//! [`lumen_core::error::CertError`]: that enum is matched all over, and the
//! evidence is advisory — a missing entry just means a generic error screen.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rustls::pki_types::CertificateDer;

use super::{cert_fields, os_roots};

/// Entries kept at once; a failing host is read back within a navigation, so
/// the map never needs to grow.
const MAX_ENTRIES: usize = 32;

/// What the cause classifier needs from one certificate of the chain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CertSummary {
    pub subject_cn: String,
    pub subject_org: String,
    pub issuer_cn: String,
    pub issuer_org: String,
    /// `notBefore` / `notAfter` as Unix seconds, `None` if unparsable.
    pub not_before: Option<i64>,
    pub not_after: Option<i64>,
}

/// The chain the server presented when verification failed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChainEvidence {
    /// Leaf first, in the order the server sent them.
    pub certs: Vec<CertSummary>,
    /// System clock at verification, Unix seconds.
    pub now_unix: i64,
    /// The last certificate sent is issued by itself (a root was sent).
    pub last_self_signed: bool,
    /// The last certificate's issuer is a trust anchor of this client — only
    /// then can a chain that still fails be a bad signature rather than a gap.
    pub last_issuer_is_anchor: bool,
    /// Some certificate names the Russian state CA (НУЦ Минцифры).
    pub nuc: bool,
}

/// Summarise `chain` (leaf first). `anchor_subjects` holds the DER `Name`
/// contents of every trust anchor ([`rustls::pki_types::TrustAnchor::subject`]).
pub fn summarize(
    chain: &[CertificateDer<'_>],
    anchor_subjects: &[Vec<u8>],
    now_unix: i64,
) -> ChainEvidence {
    let mut ev = ChainEvidence { now_unix, ..ChainEvidence::default() };
    for der in chain {
        let f = cert_fields::extract_leaf_fields(der.as_ref()).unwrap_or_default();
        ev.certs.push(CertSummary {
            subject_cn: f.subject_cn,
            subject_org: f.subject_org,
            issuer_cn: f.issuer_cn,
            issuer_org: f.issuer_org,
            not_before: iso_to_unix(&f.not_before),
            not_after: iso_to_unix(&f.not_after),
        });
        if let Some((issuer, subject)) = cert_fields::issuer_and_subject_der(der.as_ref()) {
            ev.nuc |= os_roots::is_nuc_subject(issuer) || os_roots::is_nuc_subject(subject);
        }
    }
    if let Some((issuer, subject)) =
        chain.last().and_then(|d| cert_fields::issuer_and_subject_der(d.as_ref()))
    {
        ev.last_self_signed = issuer == subject;
        ev.last_issuer_is_anchor = anchor_subjects.iter().any(|a| a.as_slice() == issuer);
    }
    ev
}

/// `"2025-01-01T00:00:00Z"` → Unix seconds (UTC), `None` for any other shape.
fn iso_to_unix(iso: &str) -> Option<i64> {
    let b = iso.as_bytes();
    let shaped = b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z';
    if !shaped {
        return None;
    }
    let num = |r: std::ops::Range<usize>| iso.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    Some(unix_from_civil(y, m, d) + hh * 3_600 + mm * 60 + ss)
}

/// Unix seconds of 00:00:00 UTC on the given proleptic Gregorian date
/// (Howard Hinnant's `days_from_civil`).
pub(crate) fn unix_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

fn registry() -> &'static Mutex<HashMap<String, ChainEvidence>> {
    static REG: OnceLock<Mutex<HashMap<String, ChainEvidence>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember the chain that failed for `host` (the latest failure wins).
pub fn record(host: &str, evidence: ChainEvidence) {
    let Ok(mut map) = registry().lock() else { return };
    if map.len() >= MAX_ENTRIES && !map.contains_key(host) {
        map.clear();
    }
    map.insert(host.to_ascii_lowercase(), evidence);
}

/// Take the evidence recorded for `host`, if any.
pub fn take(host: &str) -> Option<ChainEvidence> {
    registry().lock().ok()?.remove(&host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates_convert_to_unix() {
        assert_eq!(iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_unix("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(iso_to_unix("2025-01-01T12:30:15Z"), Some(1_735_734_615));
        assert_eq!(iso_to_unix(""), None);
        assert_eq!(iso_to_unix("2025-13-01T00:00:00Z"), None);
    }

    // ── DER fixtures: just enough certificate for `summarize` ──────────────

    fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        match body.len() {
            n if n < 0x80 => out.push(n as u8),
            n => out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]),
        }
        out.extend_from_slice(body);
        out
    }

    fn name(cn: &str, org: &str) -> Vec<u8> {
        let atv = |oid: &[u8], v: &str| {
            let pair = [tlv(0x06, oid), tlv(0x0C, v.as_bytes())].concat();
            tlv(0x31, &tlv(0x30, &pair))
        };
        tlv(0x30, &[atv(&[0x55, 0x04, 0x03], cn), atv(&[0x55, 0x04, 0x0A], org)].concat())
    }

    fn cert(issuer: &[u8], subject: &[u8], not_before: &str, not_after: &str) -> CertificateDer<'static> {
        let validity = tlv(0x30, &[tlv(0x17, not_before.as_bytes()), tlv(0x17, not_after.as_bytes())].concat());
        let tbs = tlv(
            0x30,
            &[
                tlv(0xA0, &tlv(0x02, &[0x02])),
                tlv(0x02, &[0x01]),
                tlv(0x30, &tlv(0x06, &[0x2A, 0x03])),
                issuer.to_vec(),
                validity,
                subject.to_vec(),
                tlv(0x30, &[]),
            ]
            .concat(),
        );
        let der = tlv(0x30, &[tbs, tlv(0x30, &tlv(0x06, &[0x2A, 0x03])), tlv(0x03, &[0x00, 0xDE])].concat());
        CertificateDer::from(der)
    }

    /// `Name` contents without the outer SEQUENCE header, the form a trust
    /// anchor's subject has.
    fn anchor_subject(cn: &str, org: &str) -> Vec<u8> {
        let full = name(cn, org);
        let header = if full[1] < 0x80 { 2 } else { 4 };
        full[header..].to_vec()
    }

    #[test]
    fn leaf_signed_by_a_product_root_is_summarised() {
        let issuer = name("Kaspersky Anti-Virus Personal Root Certificate", "AO Kaspersky Lab");
        let leaf = cert(&issuer, &name("bank.example", ""), "260101000000Z", "270101000000Z");
        let ev = summarize(&[leaf], &[anchor_subject("DigiCert Global Root", "DigiCert")], 1_791_979_200);
        assert_eq!(ev.certs.len(), 1);
        assert_eq!(ev.certs[0].issuer_cn, "Kaspersky Anti-Virus Personal Root Certificate");
        assert_eq!(ev.certs[0].issuer_org, "AO Kaspersky Lab");
        assert_eq!(ev.certs[0].not_after, iso_to_unix("2027-01-01T00:00:00Z"));
        assert!(!ev.last_self_signed);
        assert!(!ev.last_issuer_is_anchor);
        assert!(!ev.nuc);
        assert_eq!(ev.now_unix, 1_791_979_200);
    }

    #[test]
    fn chain_ending_at_a_known_anchor_and_a_gap_are_told_apart() {
        let root = name("Example Root", "Example Trust");
        let leaf = cert(&root, &name("site.example", ""), "260101000000Z", "270101000000Z");
        let anchors = [anchor_subject("Example Root", "Example Trust")];
        assert!(summarize(&[leaf], &anchors, 0).last_issuer_is_anchor);
        let gap_issuer = name("Example Issuing CA", "Example Trust");
        let gap = cert(&gap_issuer, &name("site.example", ""), "260101000000Z", "270101000000Z");
        assert!(!summarize(&[gap], &anchors, 0).last_issuer_is_anchor);
    }

    #[test]
    fn russian_state_ca_and_self_signed_root_are_flagged() {
        let sub = name("Russian Trusted Sub CA", "The Ministry of Digital Development");
        let root = name("Russian Trusted Root CA", "The Ministry of Digital Development");
        let leaf = cert(&sub, &name("sberbank.ru", ""), "260101000000Z", "270101000000Z");
        let inter = cert(&root, &sub, "220101000000Z", "320101000000Z");
        let top = cert(&root, &root, "220101000000Z", "320101000000Z");
        let ev = summarize(&[leaf.clone(), inter, top], &[], 0);
        assert!(ev.nuc);
        assert!(ev.last_self_signed);
        assert!(!summarize(&[leaf], &[], 0).last_self_signed);
    }

    #[test]
    fn record_then_take_is_once_and_case_insensitive() {
        let ev = ChainEvidence { now_unix: 7, ..ChainEvidence::default() };
        record("Failing.Example", ev.clone());
        assert_eq!(take("failing.example"), Some(ev));
        assert_eq!(take("failing.example"), None);
    }

    #[test]
    fn registry_stays_bounded() {
        for i in 0..(MAX_ENTRIES * 3) {
            record(&format!("bounded{i}.example"), ChainEvidence::default());
        }
        assert!(registry().lock().unwrap().len() <= MAX_ENTRIES);
    }
}
