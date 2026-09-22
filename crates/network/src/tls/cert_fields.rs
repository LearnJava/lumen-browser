//! Subject/issuer/validity/SAN/fingerprint extraction from a leaf certificate
//! (ph3-tls-hardening, part A5) — the real data behind
//! [`super::CertInfo`], replacing the [`super::CertInfo::stub_for`] placeholder.
//!
//! A pure function over borrowed certificate bytes, no clock, no I/O — same
//! posture and DER tag-length-value style as [`super::ct`] and
//! [`super::ocsp`]; this module and those three both walk a `Certificate`
//! (RFC 5280 §4.1) or the OCSP/SCT wire formats layered on top of it, so a
//! second DER reader here (rather than a shared one) keeps each module's
//! `Option`/soft-fail folding local to what it actually needs.
//!
//! **Best-effort**: any structural mismatch folds to `None` for the
//! whole-certificate fields (subject/issuer/validity/SAN) — callers treat
//! that the same as [`super::CertInfo::stub_for`]'s placeholder. The
//! fingerprint is computed separately over the untouched input bytes, so it
//! is available even when the rest of the parse fails.

use sha2::{Digest, Sha256};

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `SET` (and `SET OF`), constructed universal — wraps each
/// `RelativeDistinguishedName` in a `Name`.
const TAG_SET: u8 = 0x31;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `OBJECT IDENTIFIER`.
const TAG_OID: u8 = 0x06;
/// The DER tag for `BOOLEAN`.
const TAG_BOOLEAN: u8 = 0x01;
/// The DER tag for `OCTET STRING`.
const TAG_OCTET_STRING: u8 = 0x04;
/// The DER tag for `UTCTime` (`YYMMDDHHMMSSZ`, RFC 5280 §4.1.2.5.1).
const TAG_UTC_TIME: u8 = 0x17;
/// The DER tag for `GeneralizedTime` (`YYYYMMDDHHMMSSZ`, RFC 5280 §4.1.2.5.2).
const TAG_GENERALIZED_TIME: u8 = 0x18;
/// The DER tag of a SAN extension's `dNSName [2] IA5String` choice —
/// context class, primitive, tag number 2 (RFC 5280 §4.2.1.6 `GeneralName`).
const TAG_SAN_DNS_NAME: u8 = 0x82;
/// The optional `[0] EXPLICIT` `version` field of a `TBSCertificate` (context
/// class, constructed, tag number 0).
const TAG_CONTEXT_0: u8 = 0xA0;
/// The optional `[3] EXPLICIT` `extensions` field of a `TBSCertificate`
/// (context class, constructed, tag number 3).
const TAG_CONTEXT_3: u8 = 0xA3;

/// `id-at-commonName` (2.5.4.3, RFC 5280 §4.1.2.4).
const OID_COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
/// `id-at-organizationName` (2.5.4.10, RFC 5280 §4.1.2.4).
const OID_ORGANIZATION_NAME: &[u8] = &[0x55, 0x04, 0x0A];
/// `id-ce-subjectAltName` (2.5.29.17, RFC 5280 §4.2.1.6).
const OID_SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1D, 0x11];

/// Real certificate fields extracted from a leaf `CertificateDer`, ready to
/// populate [`super::CertInfo`] — everything [`super::CertInfo::stub_for`]
/// left blank or filled with a placeholder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeafFields {
    /// Subject Common Name, empty if absent or unparsable.
    pub subject_cn: String,
    /// Subject Organization, empty if absent.
    pub subject_org: String,
    /// Issuer Common Name, empty if absent or unparsable.
    pub issuer_cn: String,
    /// Issuer Organization, empty if absent.
    pub issuer_org: String,
    /// Validity start, ISO 8601 (`"2025-01-01T00:00:00Z"`), empty if unparsable.
    pub not_before: String,
    /// Validity end, ISO 8601, empty if unparsable.
    pub not_after: String,
    /// `dNSName` entries from the SAN extension, in certificate order.
    pub san_list: Vec<String>,
}

/// A minimal DER reader over a byte slice, walking tag-length-value triples
/// left to right. Definite-length only (DER forbids the indefinite form) — a
/// sibling of the readers in [`super::ct`] and [`super::ocsp`].
struct Der<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Der<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn read_length(&mut self) -> Option<usize> {
        let first = *self.bytes.get(self.pos)?;
        self.pos += 1;
        if first < 0x80 {
            return Some(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || self.bytes.len() - self.pos < count {
            return None;
        }
        let mut len = 0usize;
        for _ in 0..count {
            len = (len << 8) | self.bytes[self.pos] as usize;
            self.pos += 1;
        }
        Some(len)
    }

    fn read_tlv(&mut self) -> Option<(u8, &'a [u8])> {
        let tag = *self.bytes.get(self.pos)?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.bytes.len() - self.pos < len {
            return None;
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Some((tag, contents))
    }

    fn read_tagged(&mut self, tag: u8) -> Option<&'a [u8]> {
        let (t, contents) = self.read_tlv()?;
        (t == tag).then_some(contents)
    }
}

/// Read a `Name`'s `RelativeDistinguishedName` sequence, returning the first
/// `commonName` and `organizationName` attribute values found (RFC 5280
/// §4.1.2.4 permits at most one value of each type per RDN in practice, and
/// this module only ever needs the first).
fn read_cn_and_org(name_bytes: &[u8]) -> (String, String) {
    let mut cn = String::new();
    let mut org = String::new();
    let mut rdns = Der::new(name_bytes);
    while let Some(rdn) = rdns.read_tagged(TAG_SET) {
        let mut atvs = Der::new(rdn);
        while let Some(atv) = atvs.read_tagged(TAG_SEQUENCE) {
            let mut atv = Der::new(atv);
            let Some(oid) = atv.read_tagged(TAG_OID) else { continue };
            let Some((_, value)) = atv.read_tlv() else { continue };
            if oid == OID_COMMON_NAME && cn.is_empty() {
                cn = String::from_utf8_lossy(value).into_owned();
            } else if oid == OID_ORGANIZATION_NAME && org.is_empty() {
                org = String::from_utf8_lossy(value).into_owned();
            }
        }
    }
    (cn, org)
}

/// Parse a DER `Time` (`UTCTime` or `GeneralizedTime`) into ISO 8601
/// (`"YYYY-MM-DDTHH:MM:SSZ"`). `None` on any tag, length, or non-digit
/// mismatch — both forms are fixed-width ASCII digits plus a trailing `Z`
/// (fractional seconds and explicit UTC offsets are vanishingly rare in
/// publicly issued certs and unsupported here).
fn parse_der_time(tag: u8, contents: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(contents).ok()?;
    let (year, rest) = match tag {
        TAG_UTC_TIME => {
            let yy: u32 = text.get(0..2)?.parse().ok()?;
            let year = if yy < 50 { 2000 + yy } else { 1900 + yy };
            (year, text.get(2..)?)
        }
        TAG_GENERALIZED_TIME => {
            let year: u32 = text.get(0..4)?.parse().ok()?;
            (year, text.get(4..)?)
        }
        _ => return None,
    };
    if rest.len() != 11 || !rest.ends_with('Z') {
        return None;
    }
    let month = &rest[0..2];
    let day = &rest[2..4];
    let hour = &rest[4..6];
    let minute = &rest[6..8];
    let second = &rest[8..10];
    if ![month, day, hour, minute, second]
        .iter()
        .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(format!("{year:04}-{month}-{day}T{hour}:{minute}:{second}Z"))
}

/// Read the `Validity ::= SEQUENCE { notBefore Time, notAfter Time }` field,
/// returning both bounds as ISO 8601 strings (empty on parse failure).
fn read_validity(validity_bytes: &[u8]) -> (String, String) {
    let mut v = Der::new(validity_bytes);
    let not_before = v
        .read_tlv()
        .and_then(|(tag, contents)| parse_der_time(tag, contents))
        .unwrap_or_default();
    let not_after = v
        .read_tlv()
        .and_then(|(tag, contents)| parse_der_time(tag, contents))
        .unwrap_or_default();
    (not_before, not_after)
}

/// Navigate to the `TBSCertificate`'s `extensions [3]` field and return the
/// `extnValue` contents of the extension matching `target_oid`, mirroring
/// [`super::ct`]'s identically named helper (kept module-local rather than
/// shared — see this module's docs).
fn find_extension<'a>(tbs: &mut Der<'a>, target_oid: &[u8]) -> Option<&'a [u8]> {
    while let Some(tag) = tbs.peek_tag() {
        if tag != TAG_CONTEXT_3 {
            tbs.read_tlv()?;
            continue;
        }
        let wrapper = tbs.read_tlv()?.1;
        let mut extensions = Der::new(Der::new(wrapper).read_tagged(TAG_SEQUENCE)?);
        while !extensions.is_empty() {
            let extension = extensions.read_tagged(TAG_SEQUENCE)?;
            let mut extension = Der::new(extension);
            let oid = extension.read_tagged(TAG_OID)?;
            if extension.peek_tag() == Some(TAG_BOOLEAN) {
                extension.read_tlv()?; // critical — irrelevant here
            }
            let value = extension.read_tagged(TAG_OCTET_STRING)?;
            if oid == target_oid {
                return Some(value);
            }
        }
        return None;
    }
    None
}

/// Parse a SAN extension's `extnValue` (`GeneralNames ::= SEQUENCE OF
/// GeneralName`) down to its `dNSName` entries, in order. Any other
/// `GeneralName` choice (`iPAddress`, `rfc822Name`, …) is skipped.
fn parse_san_dns_names(extn_value: &[u8]) -> Vec<String> {
    let Some(names) = Der::new(extn_value).read_tagged(TAG_SEQUENCE) else {
        return Vec::new();
    };
    let mut names = Der::new(names);
    let mut out = Vec::new();
    while let Some((tag, contents)) = names.read_tlv() {
        if tag == TAG_SAN_DNS_NAME {
            out.push(String::from_utf8_lossy(contents).into_owned());
        }
    }
    out
}

/// Extract subject/issuer/validity/SAN fields from a DER-encoded leaf
/// certificate. `None` on any structural mismatch in the parts this module
/// reads — the caller falls back to [`super::CertInfo::stub_for`]-style
/// blanks, same as every other soft-fail path in this crate's `tls` module.
pub fn extract_leaf_fields(cert_der: &[u8]) -> Option<LeafFields> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE)?;
    let tbs = Der::new(certificate).read_tagged(TAG_SEQUENCE)?;

    let mut tbs = Der::new(tbs);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version [0] EXPLICIT — optional
    }
    tbs.read_tagged(TAG_INTEGER)?; // serialNumber
    tbs.read_tagged(TAG_SEQUENCE)?; // signature AlgorithmIdentifier
    let issuer = tbs.read_tagged(TAG_SEQUENCE)?; // issuer Name
    let validity = tbs.read_tagged(TAG_SEQUENCE)?; // validity
    let subject = tbs.read_tagged(TAG_SEQUENCE)?; // subject Name
    tbs.read_tagged(TAG_SEQUENCE)?; // subjectPublicKeyInfo

    let (issuer_cn, issuer_org) = read_cn_and_org(issuer);
    let (subject_cn, subject_org) = read_cn_and_org(subject);
    let (not_before, not_after) = read_validity(validity);
    let san_list = find_extension(&mut tbs, OID_SUBJECT_ALT_NAME)
        .map(parse_san_dns_names)
        .unwrap_or_default();

    Some(LeafFields {
        subject_cn,
        subject_org,
        issuer_cn,
        issuer_org,
        not_before,
        not_after,
        san_list,
    })
}

/// SHA-256 fingerprint of the whole DER-encoded certificate, formatted as
/// upper-case hex bytes separated by colons (`"AA:BB:CC:…"`), matching
/// [`super::CertInfo::fingerprint_sha256`]'s documented format.
pub fn sha256_fingerprint_hex(cert_der: &[u8]) -> String {
    let digest = Sha256::digest(cert_der);
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DER construction helpers (test-only certificate builder) ───────────

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

    fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        encode_len(contents.len(), &mut out);
        out.extend_from_slice(contents);
        out
    }

    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    fn atv(oid: &[u8], value_tag: u8, value: &[u8]) -> Vec<u8> {
        tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_OID, oid), &tlv(value_tag, value)]))
    }

    fn rdn(attrs: &[Vec<u8>]) -> Vec<u8> {
        let refs: Vec<&[u8]> = attrs.iter().map(|a| a.as_slice()).collect();
        tlv(TAG_SET, &cat(&refs))
    }

    fn name(rdns: &[Vec<u8>]) -> Vec<u8> {
        let refs: Vec<&[u8]> = rdns.iter().map(|r| r.as_slice()).collect();
        tlv(TAG_SEQUENCE, &cat(&refs))
    }

    fn cn_org_name(cn: &str, org: Option<&str>) -> Vec<u8> {
        let mut rdns = vec![rdn(&[atv(OID_COMMON_NAME, 0x0C, cn.as_bytes())])];
        if let Some(org) = org {
            rdns.push(rdn(&[atv(OID_ORGANIZATION_NAME, 0x0C, org.as_bytes())]));
        }
        name(&rdns)
    }

    fn utc_time(s: &str) -> Vec<u8> {
        tlv(TAG_UTC_TIME, s.as_bytes())
    }

    fn ext(oid: &[u8], value: &[u8]) -> Vec<u8> {
        tlv(
            TAG_SEQUENCE,
            &cat(&[&tlv(TAG_OID, oid), &tlv(TAG_OCTET_STRING, value)]),
        )
    }

    fn san_extension(dns_names: &[&str]) -> Vec<u8> {
        let entries: Vec<Vec<u8>> = dns_names
            .iter()
            .map(|n| tlv(TAG_SAN_DNS_NAME, n.as_bytes()))
            .collect();
        let refs: Vec<&[u8]> = entries.iter().map(|e| e.as_slice()).collect();
        let names = tlv(TAG_SEQUENCE, &cat(&refs));
        ext(OID_SUBJECT_ALT_NAME, &names)
    }

    /// Assemble a full v3 certificate with the given issuer/subject names,
    /// validity, and extensions.
    fn cert(
        issuer: &[u8],
        validity: &[u8],
        subject: &[u8],
        extensions: Option<&[Vec<u8>]>,
    ) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02]));
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2A, 0x03]));
        let spki = tlv(TAG_SEQUENCE, &[]);

        let mut tbs_parts: Vec<Vec<u8>> = vec![
            version,
            serial,
            sig_alg,
            issuer.to_vec(),
            validity.to_vec(),
            subject.to_vec(),
            spki,
        ];
        if let Some(extensions) = extensions {
            let refs: Vec<&[u8]> = extensions.iter().map(|e| e.as_slice()).collect();
            let ext_seq = tlv(TAG_SEQUENCE, &cat(&refs));
            tbs_parts.push(tlv(TAG_CONTEXT_3, &ext_seq));
        }
        let tbs_refs: Vec<&[u8]> = tbs_parts.iter().map(|p| p.as_slice()).collect();
        let tbs = tlv(TAG_SEQUENCE, &cat(&tbs_refs));

        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    fn validity(not_before: &str, not_after: &str) -> Vec<u8> {
        tlv(TAG_SEQUENCE, &cat(&[&utc_time(not_before), &utc_time(not_after)]))
    }

    // ── extract_leaf_fields ──────────────────────────────────────────────

    #[test]
    fn full_certificate_extracts_all_fields() {
        let issuer = cn_org_name("Example CA", Some("Example Org"));
        let subject = cn_org_name("example.com", None);
        let val = validity("250101000000Z", "260101000000Z");
        let san = san_extension(&["example.com", "www.example.com"]);
        let c = cert(&issuer, &val, &subject, Some(&[san]));

        let fields = extract_leaf_fields(&c).expect("well-formed cert parses");
        assert_eq!(fields.subject_cn, "example.com");
        assert_eq!(fields.issuer_cn, "Example CA");
        assert_eq!(fields.issuer_org, "Example Org");
        assert_eq!(fields.not_before, "2025-01-01T00:00:00Z");
        assert_eq!(fields.not_after, "2026-01-01T00:00:00Z");
        assert_eq!(fields.san_list, vec!["example.com", "www.example.com"]);
    }

    #[test]
    fn certificate_without_extensions_has_empty_san() {
        let issuer = cn_org_name("Example CA", None);
        let subject = cn_org_name("example.com", None);
        let val = validity("250101000000Z", "260101000000Z");
        let c = cert(&issuer, &val, &subject, None);

        let fields = extract_leaf_fields(&c).expect("well-formed cert parses");
        assert!(fields.san_list.is_empty());
        assert!(fields.subject_org.is_empty());
    }

    #[test]
    fn generalized_time_validity_is_parsed() {
        let issuer = cn_org_name("Example CA", None);
        let subject = cn_org_name("example.com", None);
        let val = tlv(
            TAG_SEQUENCE,
            &cat(&[
                &tlv(TAG_GENERALIZED_TIME, b"20250101000000Z"),
                &tlv(TAG_GENERALIZED_TIME, b"20260101000000Z"),
            ]),
        );
        let c = cert(&issuer, &val, &subject, None);

        let fields = extract_leaf_fields(&c).expect("well-formed cert parses");
        assert_eq!(fields.not_before, "2025-01-01T00:00:00Z");
        assert_eq!(fields.not_after, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn two_digit_year_pivot_matches_rfc5280() {
        // yy=49 -> 2049 (below the pivot), yy=50 -> 1950 (at/above the pivot).
        assert_eq!(
            parse_der_time(TAG_UTC_TIME, b"490101000000Z"),
            Some("2049-01-01T00:00:00Z".to_owned())
        );
        assert_eq!(
            parse_der_time(TAG_UTC_TIME, b"500101000000Z"),
            Some("1950-01-01T00:00:00Z".to_owned())
        );
    }

    #[test]
    fn malformed_certificate_yields_none() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(extract_leaf_fields(&not_a_cert).is_none());
    }

    #[test]
    fn truncated_certificate_yields_none() {
        let issuer = cn_org_name("Example CA", None);
        let subject = cn_org_name("example.com", None);
        let val = validity("250101000000Z", "260101000000Z");
        let c = cert(&issuer, &val, &subject, None);
        let truncated = &c[..c.len() - 5];
        assert!(extract_leaf_fields(truncated).is_none());
    }

    // ── sha256_fingerprint_hex ───────────────────────────────────────────

    #[test]
    fn fingerprint_is_deterministic_and_colon_separated() {
        let a = sha256_fingerprint_hex(b"certificate bytes");
        let b = sha256_fingerprint_hex(b"certificate bytes");
        assert_eq!(a, b);
        assert_eq!(a.matches(':').count(), 31); // 32 bytes -> 31 separators
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
    }

    #[test]
    fn fingerprint_differs_for_different_input() {
        let a = sha256_fingerprint_hex(b"certificate one");
        let b = sha256_fingerprint_hex(b"certificate two");
        assert_ne!(a, b);
    }
}
