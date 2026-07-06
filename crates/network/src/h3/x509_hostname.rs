//! X.509 hostname (SNI) verification (RFC 6125 §6, RFC 9110 §4.3.4, RFC 5280
//! §4.2.1.6) — slice 62 of the HTTP/3 sprint.
//!
//! The certificate-authentication slice
//! ([`conn_cert_auth`](super::conn_cert_auth)) proves *possession* — that the peer
//! holds the private key for the end-entity certificate it presented over this
//! handshake — and states plainly what it leaves open: "Confirming the certificate's
//! `subjectAltName` covers the requested authority (RFC 6125) is likewise deferred."
//! This module is that missing check. It answers the other half of "should this
//! certificate be trusted for this origin?": does the end-entity certificate name the
//! host the client asked for?
//!
//! ## What it matches against
//!
//! Modern practice (RFC 6125 §6.4.4, RFC 9110 §4.3.4, and every browser) matches the
//! reference hostname **only** against the certificate's `subjectAltName` extension
//! (RFC 5280 §4.2.1.6), type `dNSName`. The legacy `commonName` of the subject `Name`
//! is ignored entirely whenever a certificate carries a SAN — and CA-issued
//! certificates have carried a SAN for a decade — so this slice does not read it. A
//! certificate with no `dNSName` SAN entry has no identifier to match and fails.
//!
//! The path walked is
//!
//! ```text
//! Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
//! TBSCertificate ::= SEQUENCE {
//!     version          [0] EXPLICIT ... DEFAULT v1,   -- context 0xA0, optional
//!     serialNumber         INTEGER,
//!     signature            AlgorithmIdentifier,
//!     issuer               Name,
//!     validity             Validity,
//!     subject              Name,
//!     subjectPublicKeyInfo SubjectPublicKeyInfo,
//!     issuerUniqueID   [1] IMPLICIT ... OPTIONAL,      -- context 0x81, skipped
//!     subjectUniqueID  [2] IMPLICIT ... OPTIONAL,      -- context 0x82, skipped
//!     extensions       [3] EXPLICIT Extensions OPTIONAL }  -- context 0xA3  <- target
//!
//! Extensions ::= SEQUENCE OF Extension
//! Extension  ::= SEQUENCE {
//!     extnID    OBJECT IDENTIFIER,
//!     critical  BOOLEAN DEFAULT FALSE,                 -- optional, skipped
//!     extnValue OCTET STRING }                         -- wraps the extension DER
//!
//! SubjectAltName ::= GeneralNames ::= SEQUENCE OF GeneralName
//! GeneralName    ::= CHOICE { ... dNSName [2] IA5String ... }  -- context 0x82
//! ```
//!
//! [`verify_certificate_hostname`] navigates the `TBSCertificate` by field order to
//! the `[3]` extensions, finds the `subjectAltName` extension by its OID
//! (`2.5.29.17`), decodes its `GeneralNames`, and matches the reference hostname
//! against each `dNSName`.
//!
//! ## Matching rules
//!
//! Comparison is ASCII case-insensitive (DNS labels are case-insensitive,
//! RFC 4343). A presented name may carry a **wildcard**, honoured under the
//! conservative rules browsers enforce (RFC 6125 §6.4.3):
//!
//! - the wildcard `*` must be the **entire left-most label** (`*.example.com`, never
//!   `f*.example.com` or `foo.*.example.com`), and
//! - it matches **exactly one** label of the reference name — `*.example.com` matches
//!   `www.example.com` but neither `example.com` (too few labels) nor
//!   `a.b.example.com` (too many).
//!
//! ## What it defers
//!
//! - **`iPAddress` SAN entries.** Matching an IP-literal authority against an
//!   `iPAddress` SAN (RFC 5280 §4.2.1.6) is a separate identifier type; this slice
//!   handles DNS names only. An IP reference simply finds no matching `dNSName`.
//! - **Trust-anchor chaining and validity dates.** As
//!   [`conn_cert_auth`](super::conn_cert_auth) notes, binding the certificate to a
//!   trusted issuer and honouring `notBefore`/`notAfter` remain a later slice. This
//!   check is name-matching only.
//! - **Wiring into the connect loop.** Like the pure
//!   [`x509_spki`](super::x509_spki) extractor before
//!   [`conn_cert_auth`](super::conn_cert_auth) joined it, this slice is a pure
//!   verifier; threading the requested authority (SNI) from the connect loop
//!   ([`conn_connect`](super::conn_connect)) into this check is a later slice.
//!
//! ## Purity
//!
//! Pure DER parsing and string comparison over borrowed slices: no clock, no IO, no
//! allocation beyond lowercasing the names to compare.

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `OBJECT IDENTIFIER`.
const TAG_OID: u8 = 0x06;
/// The DER tag for `BOOLEAN` — the optional `critical` flag of an `Extension`.
const TAG_BOOLEAN: u8 = 0x01;
/// The DER tag for `OCTET STRING` — an `Extension`'s `extnValue`.
const TAG_OCTET_STRING: u8 = 0x04;
/// The DER tag for the `[0] EXPLICIT` `version` field of a `TBSCertificate`.
const TAG_CONTEXT_0: u8 = 0xA0;
/// The DER tag for the `[1] IMPLICIT` `issuerUniqueID` field (skipped if present).
const TAG_CONTEXT_1: u8 = 0x81;
/// The DER tag for the `[2] IMPLICIT` `subjectUniqueID` field (skipped if present).
const TAG_CONTEXT_2: u8 = 0x82;
/// The DER tag for the `[3] EXPLICIT` `extensions` field of a `TBSCertificate`.
const TAG_CONTEXT_3: u8 = 0xA3;
/// The DER tag for a `dNSName` `GeneralName` (`[2] IMPLICIT IA5String`, primitive).
const TAG_DNS_NAME: u8 = 0x82;

/// `id-ce-subjectAltName` (2.5.29.17, RFC 5280 §4.2.1.6) — the Subject Alternative
/// Name extension, DER-encoded as the OID content octets `0x55 0x1D 0x11`.
const OID_SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1D, 0x11];

/// Why verifying a certificate against a reference hostname failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostnameError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag
    /// where the structure required a specific one. Carries a static hint naming the
    /// field that did not decode.
    Malformed(&'static str),
    /// The reference hostname was empty, contained an empty label, or was not ASCII —
    /// it names no DNS host to match.
    InvalidReference,
    /// The certificate carried no `subjectAltName` `dNSName` entry, so there is no DNS
    /// identifier to match the reference against (RFC 6125 §6.4.4). A fatal
    /// authentication failure: the connection must not carry application data.
    NoDnsNames,
    /// The certificate's `dNSName` entries were all well-formed but none covered the
    /// reference hostname (RFC 6125 §6.3). A fatal authentication failure.
    NoMatch,
}

impl core::fmt::Display for HostnameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
            Self::InvalidReference => f.write_str("reference hostname is empty or not a DNS name"),
            Self::NoDnsNames => f.write_str("certificate has no subjectAltName dNSName entry"),
            Self::NoMatch => f.write_str("certificate does not cover the requested hostname"),
        }
    }
}

impl std::error::Error for HostnameError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples
/// left to right. Definite-length only (DER forbids the indefinite form). A sibling
/// of [`x509_spki`](super::x509_spki)'s reader, specialised to this slice's error
/// type and to peeking-and-skipping the optional fields between `subjectPublicKeyInfo`
/// and the `[3]` extensions.
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

    /// The number of unread bytes.
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Whether every byte has been read.
    fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Read a DER definite length at the cursor (X.690 DER): a short form
    /// (`0x00..=0x7f`) is the length itself; a long form (`0x81..`) gives the count of
    /// big-endian length octets that follow. The indefinite form (`0x80`) and counts
    /// wider than four octets are rejected.
    fn read_length(&mut self) -> Result<usize, HostnameError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(HostnameError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(HostnameError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(HostnameError::Malformed("truncated long-form length"));
        }
        let mut len = 0usize;
        for _ in 0..count {
            len = (len << 8) | self.bytes[self.pos] as usize;
            self.pos += 1;
        }
        Ok(len)
    }

    /// Read one TLV, returning its tag and a slice over its contents, and advance the
    /// cursor past it.
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), HostnameError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(HostnameError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(HostnameError::Malformed("truncated: content shorter than its length"));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names
    /// the field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], HostnameError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(HostnameError::Malformed(what));
        }
        Ok(contents)
    }

    /// Skip one TLV whose tag is `tag` if the next TLV carries it; otherwise leave the
    /// cursor untouched. Used for the optional fields (`[1]`/`[2]` unique IDs) between
    /// `subjectPublicKeyInfo` and the `[3]` extensions.
    fn skip_if(&mut self, tag: u8) -> Result<(), HostnameError> {
        if self.peek_tag() == Some(tag) {
            self.read_tlv()?;
        }
        Ok(())
    }
}

/// Verify that an end-entity certificate covers a reference hostname (RFC 6125 §6,
/// RFC 9110 §4.3.4).
///
/// `cert_der` is one X.509 certificate — the `cert_data` of the first
/// [`CertificateEntry`](super::tls_message::CertificateEntry) of the server's
/// `Certificate` message, the end-entity certificate (RFC 8446 §4.4.2). `reference`
/// is the host the client asked for (the SNI / the URL authority), e.g.
/// `"www.example.com"`; a single trailing dot (the FQDN root) is tolerated.
///
/// Matching is against the certificate's `subjectAltName` `dNSName` entries only; the
/// legacy `commonName` is ignored (RFC 6125 §6.4.4). A left-most-label wildcard
/// (`*.example.com`) is honoured against exactly one reference label. On a match the
/// certificate is accepted for the origin's *name* — it says nothing about the
/// certificate's chain to a trust anchor or validity dates, which
/// [`conn_cert_auth`](super::conn_cert_auth) and a later slice cover.
///
/// # Errors
///
/// - [`HostnameError::InvalidReference`] if `reference` is empty, has an empty label,
///   or is not ASCII.
/// - [`HostnameError::Malformed`] if the certificate DER is truncated or mis-structured.
/// - [`HostnameError::NoDnsNames`] if the certificate carries no `dNSName` SAN entry.
/// - [`HostnameError::NoMatch`] if `dNSName` entries are present but none cover `reference`.
pub fn verify_certificate_hostname(cert_der: &[u8], reference: &str) -> Result<(), HostnameError> {
    let reference = normalize_reference(reference).ok_or(HostnameError::InvalidReference)?;

    // Collect the certificate's dNSName SAN entries.
    let dns_names = subject_alt_dns_names(cert_der)?;
    if dns_names.is_empty() {
        return Err(HostnameError::NoDnsNames);
    }

    // A well-formed, ASCII, lowercased presented name matches per RFC 6125 §6.4.
    for presented in &dns_names {
        if let Some(name) = normalize_presented(presented)
            && dns_name_matches(&name, &reference)
        {
            return Ok(());
        }
    }
    Err(HostnameError::NoMatch)
}

/// Walk a certificate's `TBSCertificate` to its `subjectAltName` extension and return
/// the raw `dNSName` `GeneralName` values (still the certificate's IA5String bytes).
/// Returns an empty vector when there is no `subjectAltName` extension or it holds no
/// `dNSName` entries; a `Malformed` error when the DER does not decode.
fn subject_alt_dns_names(cert_der: &[u8]) -> Result<Vec<&[u8]>, HostnameError> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE, "certificate is not a SEQUENCE")?;
    let tbs = Der::new(certificate).read_tagged(TAG_SEQUENCE, "tbsCertificate is not a SEQUENCE")?;

    // TBSCertificate fields in order, up to the [3] extensions.
    let mut tbs = Der::new(tbs);
    tbs.skip_if(TAG_CONTEXT_0)?; // version [0] EXPLICIT — optional (absent = v1)
    tbs.read_tagged(TAG_INTEGER, "serialNumber is not an INTEGER")?;
    tbs.read_tagged(TAG_SEQUENCE, "signature AlgorithmIdentifier is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "issuer is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "validity is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "subject is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "subjectPublicKeyInfo is not a SEQUENCE")?;
    tbs.skip_if(TAG_CONTEXT_1)?; // issuerUniqueID [1] — optional, ignored
    tbs.skip_if(TAG_CONTEXT_2)?; // subjectUniqueID [2] — optional, ignored

    // extensions [3] EXPLICIT — optional. Absent means no SAN at all.
    if tbs.peek_tag() != Some(TAG_CONTEXT_3) {
        return Ok(Vec::new());
    }
    let extensions_wrapper = tbs.read_tagged(TAG_CONTEXT_3, "extensions [3] is not context-tagged")?;
    // The [3] EXPLICIT wrapper holds a single SEQUENCE OF Extension.
    let extensions = Der::new(extensions_wrapper)
        .read_tagged(TAG_SEQUENCE, "extensions is not a SEQUENCE OF")?;

    let mut extensions = Der::new(extensions);
    while !extensions.is_empty() {
        let extension = extensions.read_tagged(TAG_SEQUENCE, "Extension is not a SEQUENCE")?;
        let mut extension = Der::new(extension);
        let oid = extension.read_tagged(TAG_OID, "extnID is not an OBJECT IDENTIFIER")?;
        // critical BOOLEAN DEFAULT FALSE — present or not, skip it.
        extension.skip_if(TAG_BOOLEAN)?;
        let extn_value = extension.read_tagged(TAG_OCTET_STRING, "extnValue is not an OCTET STRING")?;
        if oid == OID_SUBJECT_ALT_NAME {
            return parse_general_names(extn_value);
        }
    }
    Ok(Vec::new())
}

/// Decode a `subjectAltName` extension value (`GeneralNames ::= SEQUENCE OF
/// GeneralName`) and return the `dNSName` (`[2]`) entries, ignoring every other
/// `GeneralName` variant.
fn parse_general_names(extn_value: &[u8]) -> Result<Vec<&[u8]>, HostnameError> {
    let general_names =
        Der::new(extn_value).read_tagged(TAG_SEQUENCE, "SubjectAltName is not a SEQUENCE OF")?;
    let mut general_names = Der::new(general_names);
    let mut dns_names = Vec::new();
    while !general_names.is_empty() {
        let (tag, contents) = general_names.read_tlv()?;
        if tag == TAG_DNS_NAME {
            dns_names.push(contents);
        }
    }
    Ok(dns_names)
}

/// Normalize a reference hostname for matching: strip a single trailing dot (the FQDN
/// root), lowercase it, and reject it if it is empty, not ASCII, or has an empty label
/// (a leading dot, a trailing dot beyond the one stripped, or `..`). Returns the
/// normalized name, or `None` if it is not a usable DNS reference.
fn normalize_reference(reference: &str) -> Option<String> {
    let trimmed = reference.strip_suffix('.').unwrap_or(reference);
    if trimmed.is_empty() || !trimmed.is_ascii() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.split('.').any(str::is_empty) {
        return None;
    }
    Some(lowered)
}

/// Normalize a presented `dNSName` from the certificate: require it to be non-empty
/// ASCII and lowercase it. Returns `None` for a name this slice will not match against
/// (empty or non-ASCII IA5String bytes), so it is skipped rather than matched.
fn normalize_presented(presented: &[u8]) -> Option<String> {
    if presented.is_empty() || !presented.is_ascii() {
        return None;
    }
    // SAFETY of unwrap avoided: ASCII bytes are always valid UTF-8.
    let text = core::str::from_utf8(presented).ok()?;
    Some(text.to_ascii_lowercase())
}

/// Whether a presented `dNSName` (already lowercased) covers a reference hostname
/// (already lowercased), per RFC 6125 §6.4. Both are ASCII, non-empty, and free of
/// empty labels for the reference.
///
/// A left-most-label wildcard is honoured: `*.example.com` matches exactly one
/// reference label (`www.example.com`) but never `example.com` (too few labels) nor
/// `a.b.example.com` (too many). The wildcard must be the entire left-most label; a
/// partial wildcard (`f*.example.com`) or a non-left-most wildcard falls to the literal
/// path and does not match.
fn dns_name_matches(presented: &str, reference: &str) -> bool {
    if let Some(suffix) = presented.strip_prefix("*.") {
        // The wildcard covers exactly one reference label: split off the reference's
        // left-most label and require the remainder to equal the wildcard's suffix.
        // A non-empty suffix bars a bare "*." from matching, and requiring the
        // reference to split bars a single-label reference.
        match reference.split_once('.') {
            Some((label, rest)) => !label.is_empty() && !suffix.is_empty() && rest == suffix,
            None => false,
        }
    } else {
        presented == reference
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DER construction helpers (test-only certificate builder) ───────────

    /// Encode a DER definite length: short form under 128, long form otherwise.
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

    /// Build a `tag ‖ length ‖ contents` TLV.
    fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        encode_len(contents.len(), &mut out);
        out.extend_from_slice(contents);
        out
    }

    /// Concatenate several DER blobs.
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// A `dNSName` `GeneralName`: `[2] IMPLICIT IA5String`.
    fn dns_name(name: &str) -> Vec<u8> {
        tlv(TAG_DNS_NAME, name.as_bytes())
    }

    /// A `subjectAltName` extension carrying the given `GeneralName` blobs.
    fn subject_alt_name_ext(general_names: &[&[u8]]) -> Vec<u8> {
        let names = tlv(TAG_SEQUENCE, &cat(general_names));
        let extn_value = tlv(TAG_OCTET_STRING, &names);
        let oid = tlv(TAG_OID, OID_SUBJECT_ALT_NAME);
        tlv(TAG_SEQUENCE, &cat(&[&oid, &extn_value]))
    }

    /// A generic non-SAN extension (a dummy `basicConstraints`, OID 2.5.29.19), used to
    /// prove the search skips other extensions to find the SAN.
    fn other_ext() -> Vec<u8> {
        let oid = tlv(TAG_OID, &[0x55, 0x1D, 0x13]);
        let extn_value = tlv(TAG_OCTET_STRING, &tlv(TAG_SEQUENCE, &[]));
        tlv(TAG_SEQUENCE, &cat(&[&oid, &extn_value]))
    }

    /// A SAN extension that additionally sets the `critical` BOOLEAN, to exercise the
    /// optional-flag skip.
    fn critical_subject_alt_name_ext(general_names: &[&[u8]]) -> Vec<u8> {
        let names = tlv(TAG_SEQUENCE, &cat(general_names));
        let extn_value = tlv(TAG_OCTET_STRING, &names);
        let oid = tlv(TAG_OID, OID_SUBJECT_ALT_NAME);
        let critical = tlv(TAG_BOOLEAN, &[0xFF]);
        tlv(TAG_SEQUENCE, &cat(&[&oid, &critical, &extn_value]))
    }

    /// Assemble a minimal but structurally valid v3 certificate carrying the given
    /// extensions. Every field before `[3] extensions` is a placeholder the walker
    /// skips.
    fn cert_with_extensions(extensions: &[&[u8]]) -> Vec<u8> {
        let ext_seq = tlv(TAG_SEQUENCE, &cat(extensions));
        let ext_field = tlv(TAG_CONTEXT_3, &ext_seq);
        cert_body(&ext_field)
    }

    /// A certificate with no `[3]` extensions field at all.
    fn cert_without_extensions() -> Vec<u8> {
        cert_body(&[])
    }

    /// Assemble a certificate from an (optional) trailing `[3] extensions` field.
    fn cert_body(extensions_field: &[u8]) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])); // [0] { v3 = 2 }
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2A, 0x03]));
        let issuer = tlv(TAG_SEQUENCE, &[]);
        let validity = tlv(TAG_SEQUENCE, &[]);
        let subject = tlv(TAG_SEQUENCE, &[]);
        let spki = tlv(TAG_SEQUENCE, &[]);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[&version, &serial, &sig_alg, &issuer, &validity, &subject, &spki, extensions_field]),
        );
        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    // ── exact-name matching ────────────────────────────────────────────────

    #[test]
    fn matches_an_exact_dns_name() {
        let san = subject_alt_name_ext(&[&dns_name("www.example.com")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "www.example.com"), Ok(()));
    }

    #[test]
    fn matching_is_case_insensitive() {
        let san = subject_alt_name_ext(&[&dns_name("WWW.Example.COM")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "www.example.com"), Ok(()));
        assert_eq!(verify_certificate_hostname(&cert, "WWW.EXAMPLE.COM"), Ok(()));
    }

    #[test]
    fn tolerates_a_trailing_dot_on_the_reference() {
        let san = subject_alt_name_ext(&[&dns_name("example.com")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "example.com."), Ok(()));
    }

    #[test]
    fn picks_the_matching_name_among_several() {
        let san = subject_alt_name_ext(&[
            &dns_name("one.example.com"),
            &dns_name("two.example.com"),
            &dns_name("three.example.com"),
        ]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "two.example.com"), Ok(()));
    }

    #[test]
    fn finds_the_san_after_other_extensions() {
        let san = subject_alt_name_ext(&[&dns_name("example.com")]);
        let cert = cert_with_extensions(&[&other_ext(), &other_ext(), &san]);
        assert_eq!(verify_certificate_hostname(&cert, "example.com"), Ok(()));
    }

    #[test]
    fn skips_a_critical_flag_before_the_extension_value() {
        let san = critical_subject_alt_name_ext(&[&dns_name("example.com")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "example.com"), Ok(()));
    }

    #[test]
    fn ignores_non_dns_general_names() {
        // A rfc822Name ([1]) entry sits before the dNSName; it must be ignored.
        let email = tlv(TAG_CONTEXT_1, b"admin@example.com");
        let san = subject_alt_name_ext(&[&email, &dns_name("example.com")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "example.com"), Ok(()));
    }

    // ── wildcard matching ──────────────────────────────────────────────────

    #[test]
    fn wildcard_matches_one_leftmost_label() {
        let san = subject_alt_name_ext(&[&dns_name("*.example.com")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "www.example.com"), Ok(()));
        assert_eq!(verify_certificate_hostname(&cert, "api.example.com"), Ok(()));
    }

    #[test]
    fn wildcard_does_not_match_the_bare_domain() {
        let san = subject_alt_name_ext(&[&dns_name("*.example.com")]);
        let cert = cert_with_extensions(&[&san]);
        // Too few labels: the wildcard must cover exactly one label.
        assert_eq!(
            verify_certificate_hostname(&cert, "example.com"),
            Err(HostnameError::NoMatch),
        );
    }

    #[test]
    fn wildcard_does_not_match_multiple_labels() {
        let san = subject_alt_name_ext(&[&dns_name("*.example.com")]);
        let cert = cert_with_extensions(&[&san]);
        // Too many labels: '*' matches one label, not "a.b".
        assert_eq!(
            verify_certificate_hostname(&cert, "a.b.example.com"),
            Err(HostnameError::NoMatch),
        );
    }

    #[test]
    fn partial_and_non_leftmost_wildcards_do_not_match() {
        // A partial wildcard falls to the literal path and cannot match a real host.
        let partial = subject_alt_name_ext(&[&dns_name("f*.example.com")]);
        let cert = cert_with_extensions(&[&partial]);
        assert_eq!(
            verify_certificate_hostname(&cert, "foo.example.com"),
            Err(HostnameError::NoMatch),
        );

        // A wildcard in a non-left-most label is likewise not honoured.
        let mid = subject_alt_name_ext(&[&dns_name("foo.*.example.com")]);
        let cert = cert_with_extensions(&[&mid]);
        assert_eq!(
            verify_certificate_hostname(&cert, "foo.bar.example.com"),
            Err(HostnameError::NoMatch),
        );
    }

    // ── mismatch / no SAN ──────────────────────────────────────────────────

    #[test]
    fn rejects_a_name_the_certificate_does_not_cover() {
        let san = subject_alt_name_ext(&[&dns_name("example.com")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(
            verify_certificate_hostname(&cert, "evil.test"),
            Err(HostnameError::NoMatch),
        );
    }

    #[test]
    fn no_san_extension_is_no_dns_names() {
        // A certificate with other extensions but no SAN.
        let cert = cert_with_extensions(&[&other_ext()]);
        assert_eq!(
            verify_certificate_hostname(&cert, "example.com"),
            Err(HostnameError::NoDnsNames),
        );
    }

    #[test]
    fn no_extensions_field_is_no_dns_names() {
        let cert = cert_without_extensions();
        assert_eq!(
            verify_certificate_hostname(&cert, "example.com"),
            Err(HostnameError::NoDnsNames),
        );
    }

    #[test]
    fn a_san_with_only_non_dns_names_is_no_dns_names() {
        // SAN present but carrying only an rfc822Name — no dNSName to match.
        let email = tlv(TAG_CONTEXT_1, b"admin@example.com");
        let san = subject_alt_name_ext(&[&email]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(
            verify_certificate_hostname(&cert, "example.com"),
            Err(HostnameError::NoDnsNames),
        );
    }

    // ── reference validation ───────────────────────────────────────────────

    #[test]
    fn rejects_an_empty_or_malformed_reference() {
        let san = subject_alt_name_ext(&[&dns_name("example.com")]);
        let cert = cert_with_extensions(&[&san]);
        for bad in ["", ".", ".example.com", "example..com", "exam\u{00e9}ple.com"] {
            assert_eq!(
                verify_certificate_hostname(&cert, bad),
                Err(HostnameError::InvalidReference),
                "reference {bad:?} must be rejected",
            );
        }
    }

    #[test]
    fn skips_a_non_ascii_presented_name_but_matches_a_valid_sibling() {
        // A malformed (non-ASCII) dNSName is skipped, not fatal; a valid sibling still
        // matches.
        let bad = tlv(TAG_DNS_NAME, "exa\u{00e9}mple.com".as_bytes());
        let san = subject_alt_name_ext(&[&bad, &dns_name("example.com")]);
        let cert = cert_with_extensions(&[&san]);
        assert_eq!(verify_certificate_hostname(&cert, "example.com"), Ok(()));
    }

    // ── malformed DER ──────────────────────────────────────────────────────

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            verify_certificate_hostname(&not_a_cert, "example.com"),
            Err(HostnameError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_content() {
        let san = subject_alt_name_ext(&[&dns_name("example.com")]);
        let cert = cert_with_extensions(&[&san]);
        let truncated = &cert[..cert.len() - 4];
        assert!(matches!(
            verify_certificate_hostname(truncated, "example.com"),
            Err(HostnameError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_an_empty_input() {
        assert!(matches!(
            verify_certificate_hostname(&[], "example.com"),
            Err(HostnameError::Malformed(_)),
        ));
    }

    // ── dns_name_matches unit coverage ─────────────────────────────────────

    #[test]
    fn dns_name_matches_covers_the_edges() {
        assert!(dns_name_matches("example.com", "example.com"));
        assert!(!dns_name_matches("example.com", "example.org"));
        assert!(dns_name_matches("*.example.com", "www.example.com"));
        assert!(!dns_name_matches("*.example.com", "example.com"));
        assert!(!dns_name_matches("*.example.com", "a.b.example.com"));
        // A bare "*." with an empty suffix matches nothing.
        assert!(!dns_name_matches("*.", "www."));
        // A single-label wildcard target has no dot to split.
        assert!(!dns_name_matches("*.com", "com"));
    }
}
