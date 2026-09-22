//! Certificate Transparency: embedded-SCT extraction + distinct-logs policy check
//! (RFC 6962) — ph3-tls-hardening, part A4.
//!
//! Of the three places a certificate can carry Signed Certificate Timestamps (RFC 6962
//! §3.3: the TLS `signed_certificate_timestamp` extension, a stapled-OCSP extension, or an
//! extension embedded in the leaf certificate itself), this module reads only the third —
//! the certificate's own `1.3.6.1.4.1.11129.2.4.2` extension. That is deliberate, not an
//! oversight: rustls's [`ServerCertVerifier::verify_server_cert`]
//! (rustls::client::danger::ServerCertVerifier) hands the stapled OCSP response and the
//! leaf `CertificateDer`, but **not** the raw TLS extension bytes — those are consumed
//! internally during the handshake and never reach a custom verifier (same constraint
//! [`super::ocsp`] and [`super::verifier`]'s module docs note for A3). The embedded-cert
//! path is also the common case in practice: most CAs (Let's Encrypt included) embed SCTs
//! directly rather than relying on the server to staple or negotiate them.
//!
//! ## Scope (soft-fail; data collection, not enforcement)
//!
//! Per the task brief, this slice is soft-fail: it *counts* recognized SCTs, it does not
//! reject a connection over an insufficient count. [`super::verifier::LumenVerifier`]'s
//! module docs already flag why: unlike A3 (which returns its hard-fail decision directly
//! from `verify_server_cert`, since the stapled OCSP bytes are a function parameter with no
//! shared-state risk), CT evidence here doesn't need to influence a per-connection
//! `Result` from *this* file. The consumer is a later slice ([`CertInfo`] population, A5)
//! reading `ClientConnection::peer_certificates()` after a successful handshake — a
//! per-connection source, not this verifier's shared, cached `ClientConfig`. Gating a hard
//! failure behind `TlsProfile::Strict`, per the brief, additionally requires
//! [`super::ct_logs::KNOWN_LOG_IDS`] to hold real data first — see that module's
//! "why this starts empty" note. Until then, every real connection reports
//! [`CtVerdict::Insufficient`] regardless of how many SCTs a certificate actually carries,
//! which would make a hard-fail gate reject every TLS connection, not just the exceptional
//! ones — enforcement is deferred, not implemented as a no-op flag.
//!
//! **Not verified**: an SCT's signature (RFC 6962 §3.2's `digitally-signed struct`) —
//! checking it needs the issuing log's public key and a specific input reconstruction (the
//! `TimestampedEntry` over the *pre-certificate*, not the final cert), well beyond a single
//! slice. Only the `log_id` field is read and matched against
//! [`super::ct_logs::KNOWN_LOG_IDS`], the same posture [`super::ocsp`] takes toward the
//! `BasicOCSPResponse` signature: an unauthenticated `log_id` can only ever be *undercounted*
//! (soft-fail scope, so this cannot cause a false accept), never let a forged log count
//! toward the threshold in a way that matters, because nothing here hard-fails yet.
//!
//! ## Purity
//!
//! A pure function over borrowed certificate bytes, no clock, no I/O — a sibling of
//! [`super::ocsp`] and the `h3::x509_*` extension walkers (same DER tag-length-value
//! style for the X.509 layer; a flat big-endian reader for RFC 6962's TLS-wire-format
//! `SignedCertificateTimestampList`, which is not DER).
//!
//! [`CertInfo`]: super::CertInfo

use super::ct_logs::KNOWN_LOG_IDS;

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `BOOLEAN`.
const TAG_BOOLEAN: u8 = 0x01;
/// The DER tag for `OBJECT IDENTIFIER`.
const TAG_OID: u8 = 0x06;
/// The DER tag for `OCTET STRING`.
const TAG_OCTET_STRING: u8 = 0x04;
/// The DER tag for the optional `[0] EXPLICIT` `version` field of a `TBSCertificate`
/// (context class, constructed, tag number 0).
const TAG_CONTEXT_0: u8 = 0xA0;
/// The DER tag for the optional `[3] EXPLICIT` `extensions` field of a `TBSCertificate`
/// (context class, constructed, tag number 3).
const TAG_CONTEXT_3: u8 = 0xA3;

/// `id-ce-ctPrecertificateSCTs` (`1.3.6.1.4.1.11129.2.4.2`, RFC 6962 §3.3) — the X.509
/// extension a CA embeds SCTs into when it logs the final certificate (as opposed to the
/// pre-certificate poison extension, `...4.3`, which this module does not need).
const OID_CT_SCT_LIST: &[u8] = &[0x2B, 0x06, 0x01, 0x04, 0x01, 0xD6, 0x79, 0x02, 0x04, 0x02];

/// The size in bytes of an RFC 6962 `LogID` (a SHA-256 hash).
const LOG_ID_LEN: usize = 32;

/// What the embedded-SCT evidence on a leaf certificate says about CT log coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtVerdict {
    /// At least two structurally valid SCTs from distinct logs in
    /// [`super::ct_logs::KNOWN_LOG_IDS`] were found. Carries the distinct-log count.
    Sufficient(usize),
    /// Fewer than two SCTs from distinct known logs were found — carries the count that
    /// *was* found (0, or 1). Soft-fail per this module's scope: never blocks a connection.
    Insufficient(usize),
}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples left to
/// right. Definite-length only (DER forbids the indefinite form). A sibling of the readers
/// in [`super::ocsp`] and the `h3::x509_*` family, specialised to this module's `Option`
/// soft-fail style.
struct Der<'a> {
    /// The bytes being read.
    bytes: &'a [u8],
    /// The offset of the next unread byte.
    pos: usize,
}

impl<'a> Der<'a> {
    /// A reader positioned at the start of `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Whether any unread bytes remain.
    fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Read a DER definite length at the cursor (X.690): a short form (`0x00..=0x7f`) is
    /// the length itself; a long form (`0x81..`) gives the count of big-endian length
    /// octets that follow. The indefinite form (`0x80`) and counts wider than four octets
    /// are rejected.
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

    /// Read one TLV, returning its tag and a slice over its contents, and advance the
    /// cursor past it.
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

    /// Read one TLV and require it to carry `tag`, returning its contents.
    fn read_tagged(&mut self, tag: u8) -> Option<&'a [u8]> {
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

/// Parse the double-encoded `SignedCertificateTimestampList` extension value (RFC 6962
/// §3.3: the X.509 `extnValue` OCTET STRING wraps another DER OCTET STRING, whose contents
/// are the raw TLS-wire-format list) down to each entry's 32-byte `log_id` (RFC 6962 §3.2).
/// A structurally invalid entry stops the walk (returning what was successfully read so
/// far) rather than failing the whole extraction — one malformed SCT among several valid
/// ones should not hide the valid evidence.
fn parse_sct_list(extn_value: &[u8]) -> Vec<[u8; LOG_ID_LEN]> {
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

    let mut log_ids = Vec::new();
    let mut entries = Wire::new(entries);
    while !entries.is_empty() {
        let Some(sct_len) = entries.read_u16_len() else { break };
        let Some(sct) = entries.take(sct_len) else { break };
        let Some(log_id) = parse_one_sct(sct) else { break };
        log_ids.push(log_id);
    }
    log_ids
}

/// Parse one `SerializedSCT` (RFC 6962 §3.2's `SignedCertificateTimestamp`, minus the
/// reconstructed `digitally-signed` input) down to its `log_id`. Layout: `sct_version`(1) +
/// `log_id`(32) + `timestamp`(8) + `extensions` (u16-len-prefixed) + `hash_algorithm`(1) +
/// `signature_algorithm`(1) + `signature` (u16-len-prefixed). Every field but `log_id` is
/// walked past unread — this module does not verify the signature (see module docs).
fn parse_one_sct(sct: &[u8]) -> Option<[u8; LOG_ID_LEN]> {
    let mut r = Wire::new(sct);
    r.take(1)?; // sct_version
    let log_id_bytes = r.take(LOG_ID_LEN)?;
    let log_id: [u8; LOG_ID_LEN] = log_id_bytes.try_into().ok()?;
    r.take(8)?; // timestamp
    let ext_len = r.read_u16_len()?;
    r.take(ext_len)?; // extensions
    r.take(2)?; // hash_algorithm + signature_algorithm
    let sig_len = r.read_u16_len()?;
    r.take(sig_len)?; // signature
    Some(log_id)
}

/// Extract the `log_id` of every structurally valid SCT embedded in `cert_der`'s
/// `1.3.6.1.4.1.11129.2.4.2` extension. An empty result means either the certificate
/// carries no such extension, or its DER/wire-format decoding failed at the first entry —
/// both fold to "no evidence" per this module's soft-fail scope.
pub fn extract_sct_log_ids(cert_der: &[u8]) -> Vec<[u8; LOG_ID_LEN]> {
    match find_extension(cert_der, OID_CT_SCT_LIST) {
        Some(extn_value) => parse_sct_list(extn_value),
        None => Vec::new(),
    }
}

/// Evaluate `cert_der`'s embedded-SCT evidence against a specific known-logs table —
/// [`evaluate_ct`] calls this with [`super::ct_logs::KNOWN_LOG_IDS`]; split out so the
/// distinct-logs threshold logic is testable independent of that table's (currently empty)
/// real contents.
fn evaluate_ct_against(cert_der: &[u8], known_logs: &[[u8; LOG_ID_LEN]]) -> CtVerdict {
    let mut recognized: Vec<[u8; LOG_ID_LEN]> = extract_sct_log_ids(cert_der)
        .into_iter()
        .filter(|id| known_logs.contains(id))
        .collect();
    recognized.sort_unstable();
    recognized.dedup();

    if recognized.len() >= 2 {
        CtVerdict::Sufficient(recognized.len())
    } else {
        CtVerdict::Insufficient(recognized.len())
    }
}

/// Evaluate a leaf certificate's embedded-SCT evidence against
/// [`super::ct_logs::KNOWN_LOG_IDS`]: Chrome-style policy, at least two SCTs from distinct
/// known logs required for [`CtVerdict::Sufficient`]. Soft-fail only — see module docs for
/// why this does not yet gate the connection.
pub fn evaluate_ct(cert_der: &[u8]) -> CtVerdict {
    evaluate_ct_against(cert_der, KNOWN_LOG_IDS)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // ── evaluate_ct_against: the policy threshold ───────────────────────

    #[test]
    fn two_recognized_distinct_logs_is_sufficient() {
        let a = log_id(0x01);
        let b = log_id(0x02);
        let value = sct_list_extn_value(&[sct(a), sct(b)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        assert_eq!(evaluate_ct_against(&c, &[a, b]), CtVerdict::Sufficient(2));
    }

    #[test]
    fn one_recognized_log_is_insufficient() {
        let a = log_id(0x01);
        let value = sct_list_extn_value(&[sct(a)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        assert_eq!(evaluate_ct_against(&c, &[a]), CtVerdict::Insufficient(1));
    }

    #[test]
    fn no_scts_at_all_is_insufficient_zero() {
        let c = cert(None);
        assert_eq!(evaluate_ct_against(&c, &[log_id(0x01)]), CtVerdict::Insufficient(0));
    }

    #[test]
    fn unrecognized_logs_do_not_count() {
        let a = log_id(0x01);
        let b = log_id(0x02);
        let value = sct_list_extn_value(&[sct(a), sct(b)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        // Neither log_id is in the known-logs table passed here.
        assert_eq!(evaluate_ct_against(&c, &[log_id(0x99)]), CtVerdict::Insufficient(0));
    }

    #[test]
    fn duplicate_scts_from_the_same_log_count_once() {
        let a = log_id(0x01);
        let value = sct_list_extn_value(&[sct(a), sct(a)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        assert_eq!(evaluate_ct_against(&c, &[a]), CtVerdict::Insufficient(1));
    }

    #[test]
    fn three_distinct_recognized_logs_is_sufficient_with_full_count() {
        let a = log_id(0x01);
        let b = log_id(0x02);
        let cc = log_id(0x03);
        let value = sct_list_extn_value(&[sct(a), sct(b), sct(cc)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        assert_eq!(evaluate_ct_against(&c, &[a, b, cc]), CtVerdict::Sufficient(3));
    }

    // ── evaluate_ct: the real (empty) log table ─────────────────────────

    #[test]
    fn evaluate_ct_is_insufficient_today_regardless_of_sct_count() {
        // KNOWN_LOG_IDS is empty pending graduation (see ct_logs module docs) — even a
        // certificate carrying plenty of SCTs cannot be judged sufficient yet.
        let a = log_id(0x01);
        let b = log_id(0x02);
        let value = sct_list_extn_value(&[sct(a), sct(b)]);
        let c = cert(Some(&[ext(OID_CT_SCT_LIST, &value)]));
        assert_eq!(evaluate_ct(&c), CtVerdict::Insufficient(0));
    }

    #[test]
    fn evaluate_ct_on_cert_without_scts_is_insufficient_zero() {
        assert_eq!(evaluate_ct(&cert(None)), CtVerdict::Insufficient(0));
    }
}
