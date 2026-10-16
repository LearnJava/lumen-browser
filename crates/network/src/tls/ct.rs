//! Certificate Transparency: embedded-SCT extraction, signature check and the
//! distinct-operators policy (RFC 6962) — ph3-tls-hardening A4, COMPAT-TRUST-CTLOGS.
//!
//! Of the three places a certificate can carry Signed Certificate Timestamps (RFC 6962
//! §3.3: the TLS `signed_certificate_timestamp` extension, a stapled-OCSP extension, or an
//! extension embedded in the leaf certificate itself), this module reads only the third —
//! the certificate's own `1.3.6.1.4.1.11129.2.4.2` extension. That is deliberate:
//! rustls's [`ServerCertVerifier::verify_server_cert`]
//! (rustls::client::danger::ServerCertVerifier) hands the stapled OCSP response and the
//! leaf and intermediate `CertificateDer`s, but **not** the raw TLS extension bytes. The
//! embedded path is also the common case: most CAs (Let's Encrypt, the Russian national CA)
//! embed SCTs rather than rely on the server to staple them.
//!
//! ## What counts
//!
//! An SCT counts only if all of these hold:
//! 1. its `log_id` is a log in [`super::ct_logs`] (a forged or foreign id counts for nothing);
//! 2. the log's state still covers the SCT's timestamp ([`CtLog::accepts_sct_at`]);
//! 3. the SCT's signature verifies under that log's key over the pre-certificate entry
//!    ([`super::ct_sig`] — the leaf's `TBSCertificate` without the SCT extension, plus the
//!    issuer key hash). A tampered SCT, a tampered certificate or the wrong issuer fail here.
//!
//! The verdict counts distinct **operators**, not logs: two logs run by one operator are one
//! point of failure, which is the reason the policy asks for several (Chrome's rule; the
//! national-CA trust anchor asks for ≥2 operators of three, see the site-compat report §7.6).
//! Without the issuer certificate the signature cannot be checked, so nothing counts.
//!
//! ## Scope
//!
//! This module only *measures*. Whether an insufficient verdict fails a connection is the
//! caller's policy: [`super::verifier::LumenVerifier`] can require N operators (off by
//! default), the cert panel only displays the number.
//!
//! ## Purity
//!
//! A pure function over borrowed certificate bytes, no clock, no I/O — a sibling of
//! [`super::ocsp`] and the `h3::x509_*` extension walkers (same DER tag-length-value
//! style for the X.509 layer; a flat big-endian reader for RFC 6962's TLS-wire-format
//! `SignedCertificateTimestampList`, which is not DER).
//!
//! [`CtLog::accepts_sct_at`]: super::ct_logs::CtLog::accepts_sct_at

use sha2::{Digest, Sha256};

use super::ct_logs::{self, CtLog};
use super::ct_sig;

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
pub(super) const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
pub(super) const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `BOOLEAN`.
pub(super) const TAG_BOOLEAN: u8 = 0x01;
/// The DER tag for `OBJECT IDENTIFIER`.
pub(super) const TAG_OID: u8 = 0x06;
/// The DER tag for `OCTET STRING`.
pub(super) const TAG_OCTET_STRING: u8 = 0x04;
/// The DER tag for the optional `[0] EXPLICIT` `version` field of a `TBSCertificate`
/// (context class, constructed, tag number 0).
pub(super) const TAG_CONTEXT_0: u8 = 0xA0;
/// The DER tag for the optional `[3] EXPLICIT` `extensions` field of a `TBSCertificate`
/// (context class, constructed, tag number 3).
pub(super) const TAG_CONTEXT_3: u8 = 0xA3;

/// `id-ce-ctPrecertificateSCTs` (`1.3.6.1.4.1.11129.2.4.2`, RFC 6962 §3.3) — the X.509
/// extension a CA embeds SCTs into when it logs the final certificate (as opposed to the
/// pre-certificate poison extension, `...4.3`, which this module does not need).
pub(super) const OID_CT_SCT_LIST: &[u8] = &[0x2B, 0x06, 0x01, 0x04, 0x01, 0xD6, 0x79, 0x02, 0x04, 0x02];

/// The size in bytes of an RFC 6962 `LogID` (a SHA-256 hash).
pub(super) const LOG_ID_LEN: usize = 32;

/// How many distinct log operators must vouch for a certificate for [`CtVerdict::Sufficient`].
pub const MIN_OPERATORS: usize = 2;

/// What the embedded-SCT evidence on a leaf certificate says about CT log coverage.
/// Both variants carry the number of distinct **operators** with a valid SCT (see module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtVerdict {
    /// SCTs from at least [`MIN_OPERATORS`] distinct operators passed every check.
    Sufficient(usize),
    /// Fewer operators than that — the count that *was* found (0, or 1).
    Insufficient(usize),
}

impl CtVerdict {
    /// The number of distinct operators with a valid SCT.
    pub fn operators(self) -> usize {
        match self {
            Self::Sufficient(n) | Self::Insufficient(n) => n,
        }
    }
}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples left to
/// right. Definite-length only (DER forbids the indefinite form). A sibling of the readers
/// in [`super::ocsp`] and the `h3::x509_*` family, specialised to this module's `Option`
/// soft-fail style.
pub(super) struct Der<'a> {
    /// The bytes being read.
    bytes: &'a [u8],
    /// The offset of the next unread byte.
    pos: usize,
}

impl<'a> Der<'a> {
    /// A reader positioned at the start of `bytes`.
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    pub(super) fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Whether any unread bytes remain.
    pub(super) fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Read a DER definite length at the cursor (X.690): a short form (`0x00..=0x7f`) is
    /// the length itself; a long form (`0x81..`) gives the count of big-endian length
    /// octets that follow. The indefinite form (`0x80`) and counts wider than four octets
    /// are rejected.
    pub(super) fn read_length(&mut self) -> Option<usize> {
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

    /// Read one TLV, returning its tag and a slice over its contents, and advance the
    /// cursor past it.
    pub(super) fn read_tlv(&mut self) -> Option<(u8, &'a [u8])> {
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

    /// Read one TLV, returning its tag, its contents and the whole encoding (header
    /// included) — for callers that re-assemble DER around a byte-exact piece.
    pub(super) fn read_raw(&mut self) -> Option<(u8, &'a [u8], &'a [u8])> {
        let start = self.pos;
        let (tag, contents) = self.read_tlv()?;
        Some((tag, contents, &self.bytes[start..self.pos]))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents.
    pub(super) fn read_tagged(&mut self, tag: u8) -> Option<&'a [u8]> {
        let (t, contents) = self.read_tlv()?;
        (t == tag).then_some(contents)
    }
}

/// Navigate a certificate's `TBSCertificate` to its `extensions` field (RFC 5280 §4.1.2.9),
/// then return the `extnValue` contents of the extension matching `target_oid`, if present.
/// `None` on any structural mismatch or if the certificate carries no such extension — the
/// caller folds every case into "no evidence", matching this module's soft-fail scope.
fn find_extension<'a>(cert_der: &'a [u8], target_oid: &[u8]) -> Option<&'a [u8]> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE)?;
    let tbs = Der::new(certificate).read_tagged(TAG_SEQUENCE)?;

    let mut tbs = Der::new(tbs);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version [0] EXPLICIT — optional
    }
    tbs.read_tagged(TAG_INTEGER)?; // serialNumber
    tbs.read_tagged(TAG_SEQUENCE)?; // signature AlgorithmIdentifier
    tbs.read_tagged(TAG_SEQUENCE)?; // issuer
    tbs.read_tagged(TAG_SEQUENCE)?; // validity
    tbs.read_tagged(TAG_SEQUENCE)?; // subject
    tbs.read_tagged(TAG_SEQUENCE)?; // subjectPublicKeyInfo

    // issuerUniqueID [1] / subjectUniqueID [2] / extensions [3] are all optional; find
    // extensions among whatever remains, skipping anything else unread.
    while let Some(tag) = tbs.peek_tag() {
        if tag != TAG_CONTEXT_3 {
            tbs.read_tlv()?;
            continue;
        }
        let wrapper = tbs.read_tlv()?.1; // extensions [3] EXPLICIT
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

/// A big-endian reader over RFC 6962's TLS wire format (not DER): fixed-width integers and
/// `opaque <1..2^16-1>`-style length-prefixed byte vectors.
struct Wire<'a> {
    /// The bytes being read.
    bytes: &'a [u8],
    /// The offset of the next unread byte.
    pos: usize,
}

impl<'a> Wire<'a> {
    /// A reader positioned at the start of `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Whether any unread bytes remain.
    fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Read a fixed number of bytes, advancing the cursor.
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    /// Read a big-endian `u16` length prefix, advancing the cursor.
    fn read_u16_len(&mut self) -> Option<usize> {
        let bytes = self.take(2)?;
        Some(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
    }
}

/// One `SignedCertificateTimestamp` (RFC 6962 §3.2) split into the fields the signature
/// check needs.
pub(super) struct Sct<'a> {
    /// `sct_version` (0 = v1, the only defined value).
    pub version: u8,
    /// `LogID`: SHA-256 of the issuing log's public key.
    pub log_id: [u8; LOG_ID_LEN],
    /// Milliseconds since the Unix epoch at which the log accepted the entry.
    pub timestamp_ms: u64,
    /// `CtExtensions` bytes (without their length prefix) — part of the signed data.
    pub extensions: &'a [u8],
    /// `SignatureAndHashAlgorithm.hash`.
    pub hash_algorithm: u8,
    /// `SignatureAndHashAlgorithm.signature`.
    pub signature_algorithm: u8,
    /// The log's signature (DER for ECDSA).
    pub signature: &'a [u8],
}

/// Parse the double-encoded `SignedCertificateTimestampList` extension value (RFC 6962
/// §3.3: the X.509 `extnValue` OCTET STRING wraps another DER OCTET STRING, whose contents
/// are the raw TLS-wire-format list) into its SCTs. A structurally invalid entry stops the
/// walk (returning what was successfully read so far) rather than failing the whole
/// extraction — one malformed SCT among several valid ones should not hide the valid evidence.
fn parse_sct_list(extn_value: &[u8]) -> Vec<Sct<'_>> {
    let Some(list_bytes) = Der::new(extn_value).read_tagged(TAG_OCTET_STRING) else {
        return Vec::new();
    };

    let mut outer = Wire::new(list_bytes);
    let Some(total_len) = outer.read_u16_len() else {
        return Vec::new();
    };
    let Some(entries) = outer.take(total_len) else {
        return Vec::new();
    };

    let mut scts = Vec::new();
    let mut entries = Wire::new(entries);
    while !entries.is_empty() {
        let Some(sct_len) = entries.read_u16_len() else { break };
        let Some(sct) = entries.take(sct_len) else { break };
        let Some(sct) = parse_one_sct(sct) else { break };
        scts.push(sct);
    }
    scts
}

/// Parse one `SerializedSCT`. Layout: `sct_version`(1) + `log_id`(32) + `timestamp`(8) +
/// `extensions` (u16-len-prefixed) + `hash_algorithm`(1) + `signature_algorithm`(1) +
/// `signature` (u16-len-prefixed). Trailing bytes after the signature make it malformed.
fn parse_one_sct(sct: &[u8]) -> Option<Sct<'_>> {
    let mut r = Wire::new(sct);
    let version = r.take(1)?[0];
    let log_id: [u8; LOG_ID_LEN] = r.take(LOG_ID_LEN)?.try_into().ok()?;
    let timestamp_ms = u64::from_be_bytes(r.take(8)?.try_into().ok()?);
    let ext_len = r.read_u16_len()?;
    let extensions = r.take(ext_len)?;
    let algorithms = r.take(2)?;
    let sig_len = r.read_u16_len()?;
    let signature = r.take(sig_len)?;
    r.is_empty().then_some(Sct {
        version,
        log_id,
        timestamp_ms,
        extensions,
        hash_algorithm: algorithms[0],
        signature_algorithm: algorithms[1],
        signature,
    })
}

/// Extract the `log_id` of every structurally valid SCT embedded in `cert_der`'s
/// `1.3.6.1.4.1.11129.2.4.2` extension. An empty result means either the certificate
/// carries no such extension, or its DER/wire-format decoding failed at the first entry —
/// both fold to "no evidence" per this module's soft-fail scope.
pub fn extract_sct_log_ids(cert_der: &[u8]) -> Vec<[u8; LOG_ID_LEN]> {
    match find_extension(cert_der, OID_CT_SCT_LIST) {
        Some(extn_value) => parse_sct_list(extn_value).into_iter().map(|sct| sct.log_id).collect(),
        None => Vec::new(),
    }
}

/// Operators of the logs whose SCT in `leaf_der` passes every check in the module docs,
/// deduplicated. `lookup` resolves a `LogID` to a log — [`ct_logs::find`] in production, a
/// test table in unit tests.
fn valid_operators<'t>(
    leaf_der: &[u8],
    issuer_der: Option<&[u8]>,
    lookup: impl Fn(&[u8; LOG_ID_LEN]) -> Option<&'t CtLog>,
) -> Vec<&'t str> {
    let mut operators = Vec::new();
    let (Some(issuer_der), Some(extn_value)) = (issuer_der, find_extension(leaf_der, OID_CT_SCT_LIST)) else {
        return operators;
    };
    let scts = parse_sct_list(extn_value);
    let (Some(tbs), Some(issuer_spki)) =
        (ct_sig::precert_tbs(leaf_der), ct_sig::subject_public_key_info(issuer_der))
    else {
        return operators;
    };
    let issuer_key_hash: [u8; 32] = Sha256::digest(issuer_spki).into();
    for sct in &scts {
        let Some(log) = lookup(&sct.log_id) else { continue };
        if log.accepts_sct_at(sct.timestamp_ms) && ct_sig::verify(sct, log, &issuer_key_hash, &tbs) {
            operators.push(log.operator);
        }
    }
    operators.sort_unstable();
    operators.dedup();
    operators
}

/// Judge `leaf_der` against a specific log table; split out from [`evaluate_ct`] so the
/// threshold logic is testable with synthetic logs.
fn evaluate_ct_against<'t>(
    leaf_der: &[u8],
    issuer_der: Option<&[u8]>,
    lookup: impl Fn(&[u8; LOG_ID_LEN]) -> Option<&'t CtLog>,
) -> CtVerdict {
    let n = valid_operators(leaf_der, issuer_der, lookup).len();
    if n >= MIN_OPERATORS {
        CtVerdict::Sufficient(n)
    } else {
        CtVerdict::Insufficient(n)
    }
}

/// Judge a leaf certificate's embedded SCTs against the bundled log table: at least
/// [`MIN_OPERATORS`] distinct operators with a valid SCT for [`CtVerdict::Sufficient`].
/// `issuer_der` is the certificate that signed the leaf (first intermediate); without it
/// the signatures cannot be checked and nothing counts.
pub fn evaluate_ct(leaf_der: &[u8], issuer_der: Option<&[u8]>) -> CtVerdict {
    evaluate_ct_against(leaf_der, issuer_der, ct_logs::find)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::ct_logs::LogState;

    // ── DER + wire construction helpers (test-only certificate/SCT builder) ────

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

    fn wire_u16_prefixed(contents: &[u8]) -> Vec<u8> {
        let mut out = (contents.len() as u16).to_be_bytes().to_vec();
        out.extend_from_slice(contents);
        out
    }

    /// Build one `SerializedSCT` with the given `log_id`, empty extensions and signature.
    fn sct(log_id: [u8; LOG_ID_LEN]) -> Vec<u8> {
        let mut out = vec![0u8]; // sct_version = v1
        out.extend_from_slice(&log_id);
        out.extend_from_slice(&[0u8; 8]); // timestamp
        out.extend_from_slice(&wire_u16_prefixed(&[])); // extensions
        out.extend_from_slice(&[4, 3]); // hash_algorithm, signature_algorithm (placeholders)
        out.extend_from_slice(&wire_u16_prefixed(&[0xDE, 0xAD])); // signature
        out
    }

    /// Build the double-encoded `extnValue` for an `id-ce-ctPrecertificateSCTs` extension
    /// wrapping the given SCTs.
    fn sct_list_extn_value(scts: &[Vec<u8>]) -> Vec<u8> {
        let entries: Vec<u8> = scts.iter().flat_map(|s| wire_u16_prefixed(s)).collect();
        let list = wire_u16_prefixed(&entries);
        tlv(TAG_OCTET_STRING, &list) // double-encoding: inner OCTET STRING
    }

    /// Build one `Extension`: `SEQUENCE { OID, [critical] BOOLEAN?, OCTET STRING extnValue }`.
    fn ext(oid: &[u8], value: &[u8]) -> Vec<u8> {
        let parts: Vec<Vec<u8>> = vec![tlv(TAG_OID, oid), tlv(TAG_OCTET_STRING, value)];
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        tlv(TAG_SEQUENCE, &cat(&refs))
    }

    /// Assemble a v3 certificate whose `extensions` field is the given list of extension
    /// TLVs (or no `extensions` field at all when `None`). Every field before `extensions`
    /// is a placeholder — this module never reads them.
    fn cert(extensions: Option<&[Vec<u8>]>) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02]));
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2A, 0x03]));
        let issuer = tlv(TAG_SEQUENCE, &[]);
        let validity = tlv(TAG_SEQUENCE, &[]);
        let subject = tlv(TAG_SEQUENCE, &[]);
        let spki = tlv(TAG_SEQUENCE, &[]);

        let mut tbs_parts: Vec<Vec<u8>> =
            vec![version, serial, sig_alg, issuer, validity, subject, spki];
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

    fn log_id(byte: u8) -> [u8; LOG_ID_LEN] {
        [byte; LOG_ID_LEN]
    }

    // ── extract_sct_log_ids ──────────────────────────────────────────────

    #[test]
    fn no_extensions_field_yields_no_scts() {
        assert!(extract_sct_log_ids(&cert(None)).is_empty());
    }

    #[test]
    fn extensions_without_ct_extension_yields_no_scts() {
        let c = cert(Some(&[ext(&[0x55, 0x1D, 0x11], &[])])); // unrelated OID (subjectAltName)
        assert!(extract_sct_log_ids(&c).is_empty());
    }

    #[test]
    fn one_sct_is_extracted() {
        let id = log_id(0xAA);
        let value = sct_list_extn_value(&[sct(id)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        assert_eq!(extract_sct_log_ids(&c), vec![id]);
    }

    #[test]
    fn two_scts_from_distinct_logs_are_extracted_in_order() {
        let a = log_id(0xAA);
        let b = log_id(0xBB);
        let value = sct_list_extn_value(&[sct(a), sct(b)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        assert_eq!(extract_sct_log_ids(&c), vec![a, b]);
    }

    #[test]
    fn ct_extension_among_other_extensions_is_found() {
        let id = log_id(0xCC);
        let value = sct_list_extn_value(&[sct(id)]);
        let c = cert(Some(&[
            ext(&[0x55, 0x1D, 0x0F], &[]), // keyUsage, unrelated
            ext(OID_CT_SCT_LIST, &value),
        ]));
        assert_eq!(extract_sct_log_ids(&c), vec![id]);
    }

    #[test]
    fn truncated_sct_list_yields_no_scts() {
        let value = sct_list_extn_value(&[sct(log_id(0xAA))]);
        let truncated = &value[..value.len() - 4];
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, truncated)]));
        assert!(extract_sct_log_ids(&c).is_empty());
    }

    #[test]
    fn malformed_certificate_yields_no_scts() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(extract_sct_log_ids(&not_a_cert).is_empty());
    }

    // ── precert_tbs ─────────────────────────────────────────────────────

    #[test]
    fn precert_tbs_drops_only_the_sct_extension() {
        let a = ext(&[0x55, 0x1D, 0x0F], &[1]);
        let b = ext(&[0x55, 0x1D, 0x11], &[2]);
        let sct_ext = ext(OID_CT_SCT_LIST, &sct_list_extn_value(&[sct(log_id(1))]));
        let with = cert(Some(&[a.clone(), sct_ext, b.clone()]));
        let without = cert(Some(&[a, b]));
        assert_eq!(ct_sig::precert_tbs(&with), ct_sig::precert_tbs(&without));
    }

    // ── evaluate_ct on a real chain: sberbank.ru, SCTs from VK, Yandex and the Ministry ──

    const SBERBANK_CHAIN: &str = include_str!("testdata/sberbank_chain.pem");

    /// `(leaf, issuer)` of the captured chain.
    fn sberbank_chain() -> (Vec<u8>, Vec<u8>) {
        let mut pem = SBERBANK_CHAIN.as_bytes();
        let mut certs = rustls_pemfile::certs(&mut pem).map(|c| c.expect("pem").to_vec());
        (certs.next().expect("leaf"), certs.next().expect("issuer"))
    }

    /// The leaf's SCTs, in certificate order.
    fn leaf_scts(leaf: &[u8]) -> Vec<Sct<'_>> {
        parse_sct_list(find_extension(leaf, OID_CT_SCT_LIST).expect("sct extension"))
    }

    fn offset_in(whole: &[u8], part: &[u8]) -> usize {
        part.as_ptr() as usize - whole.as_ptr() as usize
    }

    /// A table holding the leaf's three real logs (real keys) with the operator and state
    /// the test chooses per SCT, in certificate order.
    fn remapped(leaf: &[u8], over: [(&'static str, LogState); 3]) -> Vec<CtLog> {
        leaf_scts(leaf)
            .iter()
            .zip(over)
            .map(|(sct, (operator, state))| {
                let real = ct_logs::find(&sct.log_id).expect("real log");
                CtLog { operator, name: real.name, state, id: real.id, spki: real.spki.clone() }
            })
            .collect()
    }

    fn lookup_in<'t>(table: &'t [CtLog]) -> impl Fn(&[u8; LOG_ID_LEN]) -> Option<&'t CtLog> + 't {
        move |id| table.iter().find(|log| &log.id == id)
    }

    #[test]
    fn real_chain_has_three_valid_scts_from_three_operators() {
        let (leaf, issuer) = sberbank_chain();
        assert_eq!(leaf_scts(&leaf).len(), 3);
        assert_eq!(evaluate_ct(&leaf, Some(&issuer)), CtVerdict::Sufficient(3));
    }

    #[test]
    fn without_the_issuer_nothing_counts() {
        let (leaf, _) = sberbank_chain();
        assert_eq!(evaluate_ct(&leaf, None), CtVerdict::Insufficient(0));
    }

    #[test]
    fn a_wrong_issuer_counts_nothing() {
        let (leaf, _) = sberbank_chain();
        assert_eq!(evaluate_ct(&leaf, Some(&leaf)), CtVerdict::Insufficient(0));
    }

    #[test]
    fn a_tampered_certificate_counts_nothing() {
        let (mut leaf, issuer) = sberbank_chain();
        let at = leaf.windows(8).position(|w| w == b"Sberbank").expect("subject O");
        leaf[at] ^= 0x01;
        assert_eq!(evaluate_ct(&leaf, Some(&issuer)), CtVerdict::Insufficient(0));
    }

    #[test]
    fn a_corrupted_signature_does_not_count() {
        let (mut leaf, issuer) = sberbank_chain();
        let last = {
            let scts = leaf_scts(&leaf);
            offset_in(&leaf, scts[0].signature) + scts[0].signature.len() - 1
        };
        leaf[last] ^= 0x01;
        assert_eq!(evaluate_ct(&leaf, Some(&issuer)), CtVerdict::Sufficient(2));
    }

    #[test]
    fn a_foreign_log_id_does_not_count() {
        let (mut leaf, issuer) = sberbank_chain();
        let at = {
            let scts = leaf_scts(&leaf);
            let base = offset_in(&leaf, scts[1].extensions) - 8 - 2 - LOG_ID_LEN;
            // `extensions` is preceded by its u16 length and the 8-byte timestamp.
            assert_eq!(&leaf[base..base + LOG_ID_LEN], &scts[1].log_id);
            base
        };
        leaf[at] ^= 0x01;
        assert_eq!(evaluate_ct(&leaf, Some(&issuer)), CtVerdict::Sufficient(2));
    }

    #[test]
    fn logs_of_one_operator_count_once() {
        let (leaf, issuer) = sberbank_chain();
        let table = remapped(&leaf, [("A", LogState::Usable), ("A", LogState::ReadOnly), ("B", LogState::Usable)]);
        assert_eq!(
            evaluate_ct_against(&leaf, Some(&issuer), lookup_in(&table)),
            CtVerdict::Sufficient(2)
        );
        let one = remapped(&leaf, [("A", LogState::Usable); 3]);
        assert_eq!(
            evaluate_ct_against(&leaf, Some(&issuer), lookup_in(&one)),
            CtVerdict::Insufficient(1)
        );
    }

    #[test]
    fn a_log_retired_before_the_sct_does_not_count() {
        let (leaf, issuer) = sberbank_chain();
        let ts = leaf_scts(&leaf)[0].timestamp_ms;
        let before = remapped(
            &leaf,
            [("A", LogState::Retired { at_ms: ts }), ("B", LogState::Usable), ("C", LogState::Usable)],
        );
        assert_eq!(evaluate_ct_against(&leaf, Some(&issuer), lookup_in(&before)), CtVerdict::Sufficient(2));
        let after = remapped(
            &leaf,
            [("A", LogState::Retired { at_ms: ts + 1 }), ("B", LogState::Usable), ("C", LogState::Usable)],
        );
        assert_eq!(evaluate_ct_against(&leaf, Some(&issuer), lookup_in(&after)), CtVerdict::Sufficient(3));
    }

    #[test]
    fn unknown_logs_do_not_count() {
        let (leaf, issuer) = sberbank_chain();
        assert_eq!(evaluate_ct_against(&leaf, Some(&issuer), |_| None), CtVerdict::Insufficient(0));
    }

    #[test]
    fn certificate_without_scts_is_insufficient_zero() {
        let (_, issuer) = sberbank_chain();
        assert_eq!(evaluate_ct(&cert(None), Some(&issuer)), CtVerdict::Insufficient(0));
        assert_eq!(evaluate_ct(&cert(None), None), CtVerdict::Insufficient(0));
    }

    #[test]
    fn synthetic_sct_with_unknown_log_is_insufficient_zero() {
        let value = sct_list_extn_value(&[sct(log_id(0x01)), sct(log_id(0x02))]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        let (_, issuer) = sberbank_chain();
        assert_eq!(evaluate_ct(&c, Some(&issuer)), CtVerdict::Insufficient(0));
    }
}
