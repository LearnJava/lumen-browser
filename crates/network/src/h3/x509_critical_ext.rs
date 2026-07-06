//! X.509 unrecognized-critical-extension rejection (RFC 5280 §4.2, §6.1.4(n)) —
//! slice 82 of the HTTP/3 sprint.
//!
//! The eight walks that precede this one each *interpret* a specific certificate
//! extension: identity reads `subjectAltName`
//! ([`x509_hostname`](super::x509_hostname)), the CA-permission walk reads
//! `basicConstraints` ([`x509_basic_constraints`](super::x509_basic_constraints)), the
//! signing-permission walk reads `keyUsage` ([`x509_key_usage`](super::x509_key_usage)),
//! and the purpose walk reads `extendedKeyUsage`
//! ([`x509_ext_key_usage`](super::x509_ext_key_usage)). RFC 5280 §4.2 marks every
//! extension with a `critical` flag whose meaning is a demand on the *relying party*:
//! "A certificate-using system MUST reject the certificate if it encounters a critical
//! extension it does not recognize or a critical extension that contains information that
//! it cannot process" (RFC 5280 §4.2, and the path-validation step §6.1.4(n)). A
//! validator that silently ignored a critical extension it does not process would defeat
//! the issuer's intent — for instance a CA that scoped an intermediate with a **critical**
//! `nameConstraints` (2.5.29.30) restricting it to a single domain would have that
//! restriction quietly dropped, letting the intermediate mint certificates for any name.
//! Failing closed on an unrecognized critical extension is the only safe behaviour, and
//! every web browser does it.
//!
//! This module is the relying-party leg: it does not interpret any single extension, it
//! polices the *set* of critical extensions each certificate carries against the fixed set
//! of extension OIDs this validator knows how to process, and rejects any certificate that
//! marks an extension outside that set critical.
//!
//! ## What it recognizes
//!
//! The extensions the connect loop actually processes — and therefore the only extensions
//! a certificate may mark critical without being rejected here:
//!
//! | Extension | OID | Processed by |
//! |---|---|---|
//! | `subjectAltName` | 2.5.29.17 | [`x509_hostname`](super::x509_hostname) |
//! | `keyUsage` | 2.5.29.15 | [`x509_key_usage`](super::x509_key_usage) |
//! | `basicConstraints` | 2.5.29.19 | [`x509_basic_constraints`](super::x509_basic_constraints) |
//! | `extendedKeyUsage` | 2.5.29.37 | [`x509_ext_key_usage`](super::x509_ext_key_usage) |
//!
//! Any *other* extension marked critical — `nameConstraints` (2.5.29.30),
//! `policyConstraints` (2.5.29.36), `certificatePolicies` (2.5.29.32),
//! `inhibitAnyPolicy` (2.5.29.54), a private-arc extension, anything — causes rejection,
//! because this validator would otherwise ignore a constraint the issuer marked mandatory.
//! A *non-critical* extension outside this set is fine: RFC 5280 §4.2 permits a relying
//! party to ignore an unrecognized extension that is not critical.
//!
//! ## What it reads
//!
//! Each `Extension` inside the `TBSCertificate`'s `extensions` field (RFC 5280 §4.1.2.9,
//! §4.2):
//!
//! ```text
//! Extension  ::= SEQUENCE {
//!     extnID    OBJECT IDENTIFIER,
//!     critical  BOOLEAN DEFAULT FALSE,   -- present (TRUE) marks the extension critical
//!     extnValue OCTET STRING }
//! ```
//!
//! [`certificate_has_unrecognized_critical_extension`] navigates the `TBSCertificate` by
//! field order to the `[3] EXPLICIT` `extensions`, then for every `Extension` reads its
//! `extnID` and, if the `critical` BOOLEAN is present and TRUE, checks the OID against the
//! recognized set. The extension *value* is never decoded — only whether the certificate
//! *demands* it be understood.
//!
//! ## How the chain is checked
//!
//! [`verify_no_unknown_critical_extensions`] applies the per-certificate check to *every*
//! certificate in the server's `certificate_list` (RFC 8446 §4.4.2), leaf and issuers
//! alike: RFC 5280 §6.1.4(n) is a step of processing *each* certificate in the path, so a
//! critical extension this validator cannot process is as fatal on an intermediate as on
//! the leaf.
//!
//! ## What it does *not* do
//!
//! It does not verify signatures, names, times, CA permission, or purpose — those are the
//! sibling walks. It reads no extension value and enforces no policy *content*; it only
//! confirms the validator recognizes every extension the certificate insists upon. Like
//! the sibling walks it is a pure check over the presented list.
//!
//! ## Purity
//!
//! Pure DER parsing over borrowed certificate bytes: no clock, no I/O, no allocation
//! beyond the caller's slices. A sibling of [`x509_spki`](super::x509_spki),
//! [`x509_hostname`](super::x509_hostname), [`x509_validity`](super::x509_validity),
//! [`x509_chain`](super::x509_chain), [`x509_name_chain`](super::x509_name_chain),
//! [`x509_basic_constraints`](super::x509_basic_constraints),
//! [`x509_key_usage`](super::x509_key_usage), and
//! [`x509_ext_key_usage`](super::x509_ext_key_usage).

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `BOOLEAN`.
const TAG_BOOLEAN: u8 = 0x01;
/// The DER tag for `OBJECT IDENTIFIER`.
const TAG_OID: u8 = 0x06;
/// The DER tag for the optional `[0] EXPLICIT` `version` field of a `TBSCertificate`
/// (context class, constructed, tag number 0).
const TAG_CONTEXT_0: u8 = 0xA0;
/// The DER tag for the optional `[3] EXPLICIT` `extensions` field of a `TBSCertificate`
/// (context class, constructed, tag number 3).
const TAG_CONTEXT_3: u8 = 0xA3;

/// `id-ce-subjectAltName` (2.5.29.17, RFC 5280 §4.2.1.6) — processed by
/// [`x509_hostname`](super::x509_hostname); a critical `subjectAltName` is recognized.
const OID_SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1D, 0x11];
/// `id-ce-keyUsage` (2.5.29.15, RFC 5280 §4.2.1.3) — processed by
/// [`x509_key_usage`](super::x509_key_usage); a critical `keyUsage` is recognized.
const OID_KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x0F];
/// `id-ce-basicConstraints` (2.5.29.19, RFC 5280 §4.2.1.9) — processed by
/// [`x509_basic_constraints`](super::x509_basic_constraints); a critical
/// `basicConstraints` is recognized.
const OID_BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1D, 0x13];
/// `id-ce-extKeyUsage` (2.5.29.37, RFC 5280 §4.2.1.12) — processed by
/// [`x509_ext_key_usage`](super::x509_ext_key_usage); a critical `extendedKeyUsage` is
/// recognized.
const OID_EXT_KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x25];

/// The extension OIDs this validator processes, and therefore the only OIDs a certificate
/// may mark `critical` without being rejected (RFC 5280 §4.2, §6.1.4(n)). Any critical
/// extension whose `extnID` is not one of these is unrecognized and fatal.
const RECOGNIZED_CRITICAL: &[&[u8]] = &[
    OID_SUBJECT_ALT_NAME,
    OID_KEY_USAGE,
    OID_BASIC_CONSTRAINTS,
    OID_EXT_KEY_USAGE,
];

/// Why extracting a certificate's critical extensions failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CritExtError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag where the
    /// `Certificate`/`TBSCertificate`/extension structure required a specific one. Carries a
    /// static hint naming the field that did not decode.
    Malformed(&'static str),
}

impl core::fmt::Display for CritExtError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
        }
    }
}

impl std::error::Error for CritExtError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples left to
/// right. Definite-length only (DER forbids the indefinite form). A sibling of the readers
/// in [`x509_spki`](super::x509_spki), [`x509_hostname`](super::x509_hostname),
/// [`x509_validity`](super::x509_validity), [`x509_chain`](super::x509_chain),
/// [`x509_name_chain`](super::x509_name_chain),
/// [`x509_basic_constraints`](super::x509_basic_constraints),
/// [`x509_key_usage`](super::x509_key_usage), and
/// [`x509_ext_key_usage`](super::x509_ext_key_usage), specialised to this slice's error
/// type.
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

    /// Whether any unread bytes remain.
    fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Read a DER definite length at the cursor (X.690): a short form (`0x00..=0x7f`) is the
    /// length itself; a long form (`0x81..`) gives the count of big-endian length octets that
    /// follow. The indefinite form (`0x80`) and counts wider than four octets are rejected.
    fn read_length(&mut self) -> Result<usize, CritExtError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(CritExtError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(CritExtError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(CritExtError::Malformed("truncated long-form length"));
        }
        let mut len = 0usize;
        for _ in 0..count {
            len = (len << 8) | self.bytes[self.pos] as usize;
            self.pos += 1;
        }
        Ok(len)
    }

    /// Read one TLV, returning its tag and a slice over its contents, and advance the cursor
    /// past it.
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), CritExtError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(CritExtError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(CritExtError::Malformed(
                "truncated: content shorter than its length",
            ));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names the
    /// field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], CritExtError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(CritExtError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Whether one certificate carries an extension it marks `critical` whose `extnID` this
/// validator does not process (RFC 5280 §4.2, §6.1.4(n)).
///
/// `cert_der` is one X.509 certificate — a `CertificateEntry.cert_data` from the server's
/// `Certificate` message (RFC 8446 §4.4.2). The result is `true` when the certificate
/// carries at least one extension whose `critical` flag is `TRUE` and whose OID is outside
/// [`RECOGNIZED_CRITICAL`], i.e. an extension the certificate insists be understood but
/// this validator cannot process. A certificate with no `extensions` field, or one whose
/// critical extensions are all recognized, yields `false`.
///
/// The extension *value* is never decoded: this reports only whether the certificate
/// *demands* processing of an extension the validator does not know, not whether that
/// extension's content is well-formed.
///
/// # Errors
///
/// [`CritExtError::Malformed`] if the certificate DER is truncated or does not decode to a
/// `TBSCertificate` down to and including its `extensions` field.
pub fn certificate_has_unrecognized_critical_extension(
    cert_der: &[u8],
) -> Result<bool, CritExtError> {
    let extensions = match tbs_extensions(cert_der)? {
        Some(extensions) => extensions,
        // No `extensions` field at all: pre-v3, or a v3 certificate that omitted the optional
        // field. A certificate with no extensions demands nothing of the relying party.
        None => return Ok(false),
    };

    // Extensions ::= SEQUENCE OF Extension. Inspect the `critical` flag of each.
    let mut extensions = Der::new(extensions);
    while !extensions.is_empty() {
        let extension = extensions.read_tagged(TAG_SEQUENCE, "extension is not a SEQUENCE")?;
        let mut extension = Der::new(extension);
        let oid = extension.read_tagged(TAG_OID, "extension has no OID")?;

        // Extension ::= SEQUENCE { extnID, critical BOOLEAN DEFAULT FALSE, extnValue }.
        // The `critical` BOOLEAN is present only when the extension is critical: DER encodes
        // a DEFAULT-valued field by omitting it (X.690 §11.5), so a present BOOLEAN carrying
        // FALSE (0x00) is a technical violation we still read tolerantly — critical only when
        // its single content octet is non-zero (X.690 §11.1: TRUE is any non-zero octet).
        let critical = if extension.peek_tag() == Some(TAG_BOOLEAN) {
            let value = extension.read_tagged(TAG_BOOLEAN, "critical is not a BOOLEAN")?;
            value.iter().any(|&b| b != 0)
        } else {
            false
        };

        if critical && !RECOGNIZED_CRITICAL.contains(&oid) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Navigate a certificate's `TBSCertificate` to its `extensions` field (RFC 5280 §4.1.2.9),
/// returning the contents of the `[3] EXPLICIT` wrapper's inner `SEQUENCE OF Extension`, or
/// `None` if the certificate carries no `extensions` field.
fn tbs_extensions(cert_der: &[u8]) -> Result<Option<&[u8]>, CritExtError> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate =
        Der::new(cert_der).read_tagged(TAG_SEQUENCE, "certificate is not a SEQUENCE")?;
    let tbs = Der::new(certificate).read_tagged(TAG_SEQUENCE, "tbsCertificate is not a SEQUENCE")?;

    // TBSCertificate fields in order, up to extensions; only extensions is interpreted.
    let mut tbs = Der::new(tbs);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version [0] EXPLICIT — optional (absent = v1)
    }
    tbs.read_tagged(TAG_INTEGER, "serialNumber is not an INTEGER")?;
    tbs.read_tagged(TAG_SEQUENCE, "signature AlgorithmIdentifier is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "issuer is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "validity is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "subject is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "subjectPublicKeyInfo is not a SEQUENCE")?;

    // The remaining fields — issuerUniqueID [1], subjectUniqueID [2], extensions [3] — are
    // all optional. Skip the unique-ID fields and stop at the extensions wrapper; anything
    // past extensions does not exist.
    while let Some(tag) = tbs.peek_tag() {
        if tag == TAG_CONTEXT_3 {
            // extensions [3] EXPLICIT SEQUENCE OF Extension: the [3] wrapper's contents are
            // the SEQUENCE OF Extension, whose contents are the extensions.
            let wrapper = tbs.read_tlv()?.1;
            let inner =
                Der::new(wrapper).read_tagged(TAG_SEQUENCE, "extensions is not a SEQUENCE")?;
            return Ok(Some(inner));
        }
        // issuerUniqueID [1] / subjectUniqueID [2] (or any earlier-terminating field): skip
        // without interpreting.
        tbs.read_tlv()?;
    }

    Ok(None)
}

/// Why enforcing the unrecognized-critical-extension rule over a server-presented chain
/// failed (RFC 5280 §4.2, §6.1.4(n), RFC 8446 §4.4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CritExtWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity certificate
    /// first, so there is nothing to check — a malformed `Certificate` message.
    EmptyChain,
    /// A certificate's extensions could not be extracted: its DER failed to decode to a
    /// `TBSCertificate` down to its `extensions` field.
    Certificate {
        /// Position, in the `certificate_list`, of the certificate whose extensions failed to
        /// extract (0 = the end-entity leaf).
        index: usize,
        /// The underlying extraction failure.
        error: CritExtError,
    },
    /// A certificate marks an extension `critical` whose `extnID` this validator does not
    /// process (RFC 5280 §4.2, §6.1.4(n)): the certificate insists on a constraint the
    /// relying party cannot honour, so it must be rejected.
    UnrecognizedCritical {
        /// Position, in the `certificate_list`, of the certificate carrying the unrecognized
        /// critical extension (0 = the end-entity leaf).
        index: usize,
    },
}

impl core::fmt::Display for CritExtWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::Certificate { index, error } => write!(f, "certificate #{index}: {error}"),
            Self::UnrecognizedCritical { index } => write!(
                f,
                "certificate #{index} marks an unrecognized extension critical"
            ),
        }
    }
}

impl std::error::Error for CritExtWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Certificate { error, .. } => Some(error),
            Self::EmptyChain | Self::UnrecognizedCritical { .. } => None,
        }
    }
}

/// Verify that no certificate in a server-presented chain marks an extension `critical` that
/// this validator does not process (RFC 5280 §4.2, §6.1.4(n), RFC 8446 §4.4.2).
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message — the
/// end-entity certificate first, then each issuing intermediate. Every certificate in the
/// list is checked, leaf and issuers alike: RFC 5280 §6.1.4(n) is a step of processing
/// *each* certificate in the path, so a critical extension this validator cannot process is
/// as fatal on an intermediate as on the leaf. The recognized extensions are exactly those
/// the connect loop's other walks process — `subjectAltName`, `keyUsage`,
/// `basicConstraints`, and `extendedKeyUsage` ([`RECOGNIZED_CRITICAL`]); any other extension
/// marked critical is fatal, because ignoring it would silently drop a constraint the issuer
/// marked mandatory. A *non-critical* unrecognized extension is permitted (RFC 5280 §4.2).
///
/// An empty chain is [`CritExtWalkError::EmptyChain`].
///
/// This is the relying-party complement of the eight interpreting walks (the signature walk
/// [`verify_chain_signatures`](super::x509_chain::verify_chain_signatures), the name walk
/// [`verify_name_chain`](super::x509_name_chain::verify_name_chain), the `basicConstraints`
/// walk [`verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints), the
/// `keyUsage` walk [`verify_cert_sign_usage`](super::x509_key_usage::verify_cert_sign_usage),
/// the trust-anchor walk
/// [`verify_trust_anchor`](super::x509_trust_anchor::verify_trust_anchor), and the
/// `extendedKeyUsage` walk
/// [`verify_server_auth_eku`](super::x509_ext_key_usage::verify_server_auth_eku)): where each
/// of those interprets one extension, this confirms the validator recognizes *every*
/// extension any certificate insists upon. It verifies no signature, name, time, permission,
/// or purpose, and decodes no extension value.
///
/// # Errors
///
/// - [`CritExtWalkError::EmptyChain`] if `chain` is empty.
/// - [`CritExtWalkError::Certificate`] if a certificate's extensions cannot be extracted.
/// - [`CritExtWalkError::UnrecognizedCritical`] if a certificate marks an extension critical
///   whose OID is not one this validator processes.
pub fn verify_no_unknown_critical_extensions(chain: &[&[u8]]) -> Result<(), CritExtWalkError> {
    if chain.is_empty() {
        return Err(CritExtWalkError::EmptyChain);
    }

    for (index, cert) in chain.iter().enumerate() {
        let unrecognized = certificate_has_unrecognized_critical_extension(cert)
            .map_err(|error| CritExtWalkError::Certificate { index, error })?;
        if unrecognized {
            return Err(CritExtWalkError::UnrecognizedCritical { index });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DER tag for `OCTET STRING` — the `extnValue` wrapper of an `Extension`. Used only
    /// by the test certificate builder; the walk never reads an extension value.
    const TAG_OCTET_STRING: u8 = 0x04;

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

    /// `id-ce-nameConstraints` (2.5.29.30) — an extension this validator does not process,
    /// used to build a certificate whose critical flag must be rejected.
    const OID_NAME_CONSTRAINTS: &[u8] = &[0x55, 0x1D, 0x1E];

    /// Build one `Extension`: `SEQUENCE { OID, [critical] BOOLEAN?, OCTET STRING extnValue }`.
    /// The extension value is an empty OCTET STRING — this walk never decodes it.
    fn ext(oid: &[u8], critical: bool) -> Vec<u8> {
        let mut parts: Vec<Vec<u8>> = vec![tlv(TAG_OID, oid)];
        if critical {
            parts.push(tlv(TAG_BOOLEAN, &[0xFF]));
        }
        parts.push(tlv(TAG_OCTET_STRING, &[]));
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        tlv(TAG_SEQUENCE, &cat(&refs))
    }

    /// Build one `Extension` whose `critical` BOOLEAN is present but encodes FALSE (0x00) —
    /// a technical DER violation the walk still treats as non-critical.
    fn ext_critical_false(oid: &[u8]) -> Vec<u8> {
        let parts: Vec<Vec<u8>> =
            vec![tlv(TAG_OID, oid), tlv(TAG_BOOLEAN, &[0x00]), tlv(TAG_OCTET_STRING, &[])];
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        tlv(TAG_SEQUENCE, &cat(&refs))
    }

    /// Assemble a v3 certificate whose `extensions` field is the given list of extension TLVs
    /// (or a certificate with no `extensions` field at all when `extensions` is `None`). Every
    /// field before `extensions` is a placeholder the walker skips.
    fn cert(extensions: Option<&[Vec<u8>]>) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])); // [0] { v3 = 2 }
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
            tbs_parts.push(tlv(TAG_CONTEXT_3, &ext_seq)); // [3] EXPLICIT
        }
        let tbs_refs: Vec<&[u8]> = tbs_parts.iter().map(|p| p.as_slice()).collect();
        let tbs = tlv(TAG_SEQUENCE, &cat(&tbs_refs));

        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]); // BIT STRING
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    // ── certificate_has_unrecognized_critical_extension ─────────────────────

    #[test]
    fn no_extensions_field_recognizes_nothing_to_reject() {
        assert_eq!(
            certificate_has_unrecognized_critical_extension(&cert(None)),
            Ok(false),
        );
    }

    #[test]
    fn recognized_critical_extensions_pass() {
        // Every extension this validator processes, all marked critical.
        let c = cert(Some(&[
            ext(OID_SUBJECT_ALT_NAME, true),
            ext(OID_KEY_USAGE, true),
            ext(OID_BASIC_CONSTRAINTS, true),
            ext(OID_EXT_KEY_USAGE, true),
        ]));
        assert_eq!(certificate_has_unrecognized_critical_extension(&c), Ok(false));
    }

    #[test]
    fn unrecognized_but_non_critical_extension_passes() {
        // nameConstraints is not processed, but a non-critical unrecognized extension is fine.
        let c = cert(Some(&[ext(OID_NAME_CONSTRAINTS, false)]));
        assert_eq!(certificate_has_unrecognized_critical_extension(&c), Ok(false));
    }

    #[test]
    fn unrecognized_critical_extension_is_flagged() {
        let c = cert(Some(&[ext(OID_NAME_CONSTRAINTS, true)]));
        assert_eq!(certificate_has_unrecognized_critical_extension(&c), Ok(true));
    }

    #[test]
    fn unrecognized_critical_after_recognized_ones_is_flagged() {
        // The scanner must keep looking past recognized critical extensions.
        let c = cert(Some(&[
            ext(OID_KEY_USAGE, true),
            ext(OID_BASIC_CONSTRAINTS, true),
            ext(OID_NAME_CONSTRAINTS, true),
        ]));
        assert_eq!(certificate_has_unrecognized_critical_extension(&c), Ok(true));
    }

    #[test]
    fn critical_boolean_encoding_false_is_not_critical() {
        // A `critical` BOOLEAN present but 0x00 (a DER violation) is read as non-critical, so
        // an unrecognized extension so-encoded is not rejected.
        let c = cert(Some(&[ext_critical_false(OID_NAME_CONSTRAINTS)]));
        assert_eq!(certificate_has_unrecognized_critical_extension(&c), Ok(false));
    }

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            certificate_has_unrecognized_critical_extension(&not_a_cert),
            Err(CritExtError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_certificate() {
        let c = cert(Some(&[ext(OID_NAME_CONSTRAINTS, true)]));
        assert!(matches!(
            certificate_has_unrecognized_critical_extension(&c[..c.len() - 4]),
            Err(CritExtError::Malformed(_)),
        ));
    }

    // ── verify_no_unknown_critical_extensions: the chain walk ───────────────

    #[test]
    fn walk_accepts_a_chain_of_recognized_criticals() {
        let leaf = cert(Some(&[ext(OID_SUBJECT_ALT_NAME, true), ext(OID_EXT_KEY_USAGE, true)]));
        let ca = cert(Some(&[ext(OID_BASIC_CONSTRAINTS, true), ext(OID_KEY_USAGE, true)]));
        let chain: Vec<&[u8]> = vec![&leaf, &ca];
        assert_eq!(verify_no_unknown_critical_extensions(&chain), Ok(()));
    }

    #[test]
    fn walk_accepts_a_chain_with_no_extensions() {
        let leaf = cert(None);
        let ca = cert(None);
        let chain: Vec<&[u8]> = vec![&leaf, &ca];
        assert_eq!(verify_no_unknown_critical_extensions(&chain), Ok(()));
    }

    #[test]
    fn walk_rejects_a_leaf_with_an_unrecognized_critical() {
        let leaf = cert(Some(&[ext(OID_NAME_CONSTRAINTS, true)]));
        let ca = cert(None);
        let chain: Vec<&[u8]> = vec![&leaf, &ca];
        assert_eq!(
            verify_no_unknown_critical_extensions(&chain),
            Err(CritExtWalkError::UnrecognizedCritical { index: 0 }),
        );
    }

    #[test]
    fn walk_rejects_an_intermediate_with_an_unrecognized_critical() {
        // An unrecognized critical extension on an issuer is as fatal as on the leaf.
        let leaf = cert(Some(&[ext(OID_SUBJECT_ALT_NAME, true)]));
        let ca = cert(Some(&[ext(OID_NAME_CONSTRAINTS, true)]));
        let chain: Vec<&[u8]> = vec![&leaf, &ca];
        assert_eq!(
            verify_no_unknown_critical_extensions(&chain),
            Err(CritExtWalkError::UnrecognizedCritical { index: 1 }),
        );
    }

    #[test]
    fn walk_rejects_an_empty_chain() {
        let chain: Vec<&[u8]> = vec![];
        assert_eq!(
            verify_no_unknown_critical_extensions(&chain),
            Err(CritExtWalkError::EmptyChain),
        );
    }

    #[test]
    fn walk_reports_a_malformed_certificate() {
        let good = cert(Some(&[ext(OID_KEY_USAGE, true)]));
        let truncated = good[..good.len() - 4].to_vec();
        let chain: Vec<&[u8]> = vec![&truncated];
        assert!(matches!(
            verify_no_unknown_critical_extensions(&chain),
            Err(CritExtWalkError::Certificate { index: 0, .. }),
        ));
    }

    #[test]
    fn walk_error_display_names_the_index() {
        let e = CritExtWalkError::UnrecognizedCritical { index: 1 };
        assert!(e.to_string().contains("#1"));
        assert!(CritExtWalkError::EmptyChain.to_string().contains("empty"));
    }
}
