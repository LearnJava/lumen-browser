//! X.509 `basicConstraints` verification (RFC 5280 §4.2.1.9, §6.1) — slice 71 of the
//! HTTP/3 sprint.
//!
//! The signature walk ([`x509_chain::verify_chain_signatures`](super::x509_chain::verify_chain_signatures),
//! slices 66–68) proves each certificate is *signed* by the one above it, and the name
//! walk ([`x509_name_chain::verify_name_chain`](super::x509_name_chain::verify_name_chain),
//! slices 69–70) proves each certificate *names* the one above it as its issuer. Both
//! deliberately left a third leg to a later slice: neither checks that a certificate
//! used to *issue* another is actually permitted to. RFC 5280 §4.2.1.9 carries that
//! permission in the `basicConstraints` extension — a certifying certificate MUST assert
//! `cA = TRUE`, and MAY cap, via `pathLenConstraint`, how many intermediate certificates
//! are allowed to follow it toward the end-entity. Without this check a server could
//! present a *leaf* certificate (a valid, correctly-signed, correctly-named end-entity
//! certificate for some unrelated host) as an *intermediate* and mint certificates for
//! any name beneath it. This module is the `basicConstraints` leg: the complement of the
//! signature and name walks, layered beside them exactly as they are layered beside one
//! another.
//!
//! ## What it reads
//!
//! The `basicConstraints` extension inside the `TBSCertificate`'s `extensions` field
//! (RFC 5280 §4.1.2.9, §4.2.1.9):
//!
//! ```text
//! TBSCertificate ::= SEQUENCE {
//!     version         [0] EXPLICIT ... DEFAULT v1,   -- context 0xA0, optional
//!     serialNumber        INTEGER,
//!     signature           AlgorithmIdentifier,
//!     issuer              Name,
//!     validity            Validity,
//!     subject             Name,
//!     subjectPublicKeyInfo SubjectPublicKeyInfo,
//!     issuerUniqueID  [1] IMPLICIT ... OPTIONAL,     -- context 0x81, v2/v3
//!     subjectUniqueID [2] IMPLICIT ... OPTIONAL,     -- context 0x82, v2/v3
//!     extensions      [3] EXPLICIT Extensions OPTIONAL }  -- context 0xA3, v3 <- the target
//!
//! Extensions ::= SEQUENCE OF Extension
//! Extension  ::= SEQUENCE {
//!     extnID    OBJECT IDENTIFIER,
//!     critical  BOOLEAN DEFAULT FALSE,
//!     extnValue OCTET STRING }                       -- DER of the extension value
//!
//! BasicConstraints ::= SEQUENCE {
//!     cA                BOOLEAN DEFAULT FALSE,
//!     pathLenConstraint INTEGER (0..MAX) OPTIONAL }
//! ```
//!
//! [`certificate_basic_constraints`] navigates the `TBSCertificate` by field order —
//! skipping the six fields and two optional unique-IDs before `extensions` without
//! interpreting them — locates the `basicConstraints` extension by its OID
//! (`2.5.29.19`), and decodes the `cA` flag and the optional `pathLenConstraint`. An
//! absent extension (or an absent `cA`) means `cA = FALSE` (RFC 5280 §4.2.1.9): the
//! certificate is not a CA.
//!
//! ## How the chain is checked
//!
//! [`verify_ca_constraints`] walks the server-presented `certificate_list`. For every
//! certificate that *issues* another — that is, every certificate but the end-entity
//! leaf at index 0 — it requires `cA = TRUE`, and where a `pathLenConstraint` is present
//! it requires that constraint to be no smaller than the number of intermediate
//! certificates that sit between it and the leaf (RFC 5280 §6.1.4(m)). The leaf itself
//! is not required to be a CA.
//!
//! ## What it does *not* do
//!
//! This confirms only that the presented certificates are *allowed* to form an issuance
//! path — the third leg beside the signature walk ([`x509_chain`](super::x509_chain))
//! and the name walk ([`x509_name_chain`](super::x509_name_chain)). It does **not**
//! verify signatures or names, terminate the chain at a trusted root (RFC 5280 §6.1, a
//! later slice), honour `keyUsage` (§4.2.1.3, whose `keyCertSign` bit is the companion
//! permission), or apply name constraints (§4.2.1.10). Like the sibling walks it is a
//! pure check over the presented list; wiring it into the connect loop is a later slice
//! (as slice 68 followed slice 67 for signatures, and slice 70 followed slice 69 for
//! names).
//!
//! ## Purity
//!
//! Pure DER parsing over borrowed certificate bytes: no clock, no I/O, no allocation
//! beyond the caller's slices. A sibling of [`x509_spki`](super::x509_spki),
//! [`x509_hostname`](super::x509_hostname), [`x509_validity`](super::x509_validity),
//! [`x509_chain`](super::x509_chain), and [`x509_name_chain`](super::x509_name_chain).

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

/// `id-ce-basicConstraints` (2.5.29.19, RFC 5280 §4.2.1.9) — the extension OID whose
/// value carries the `cA` flag and the optional `pathLenConstraint`.
const OID_BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1D, 0x13];

/// The decoded `basicConstraints` extension of one certificate (RFC 5280 §4.2.1.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasicConstraints {
    /// Whether the certificate asserts `cA = TRUE` — that it is a certificate authority
    /// permitted to sign other certificates. `false` when `cA` is `FALSE`, absent
    /// (DEFAULT FALSE), or the whole `basicConstraints` extension is absent.
    pub is_ca: bool,
    /// The `pathLenConstraint`, if present: the maximum number of non-self-issued
    /// intermediate certificates that may follow this one toward the end-entity (RFC
    /// 5280 §4.2.1.9). Meaningful only when [`is_ca`](BasicConstraints::is_ca) is `true`;
    /// `None` means unconstrained.
    pub path_len_constraint: Option<u32>,
    /// Whether the `basicConstraints` extension was present at all. `false` means the
    /// certificate carried no such extension, which RFC 5280 §4.2.1.9 treats as
    /// `cA = FALSE`; this field lets a caller distinguish "explicitly not a CA" from
    /// "silent about it" should a later slice need to.
    pub present: bool,
}

impl BasicConstraints {
    /// The value for a certificate with no `basicConstraints` extension: not a CA, no
    /// path-length constraint (RFC 5280 §4.2.1.9 treats an absent extension as
    /// `cA = FALSE`).
    const ABSENT: Self =
        Self { is_ca: false, path_len_constraint: None, present: false };
}

/// Why extracting a certificate's `basicConstraints` extension failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicConstraintsError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag where
    /// the `Certificate`/`TBSCertificate`/extension structure required a specific one.
    /// Carries a static hint naming the field that did not decode.
    Malformed(&'static str),
}

impl core::fmt::Display for BasicConstraintsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
        }
    }
}

impl std::error::Error for BasicConstraintsError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples left
/// to right. Definite-length only (DER forbids the indefinite form). A sibling of the
/// readers in [`x509_spki`](super::x509_spki), [`x509_hostname`](super::x509_hostname),
/// [`x509_validity`](super::x509_validity), [`x509_chain`](super::x509_chain), and
/// [`x509_name_chain`](super::x509_name_chain), specialised to this slice's error type.
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

    /// Read a DER definite length at the cursor (X.690): a short form (`0x00..=0x7f`) is
    /// the length itself; a long form (`0x81..`) gives the count of big-endian length
    /// octets that follow. The indefinite form (`0x80`) and counts wider than four
    /// octets are rejected.
    fn read_length(&mut self) -> Result<usize, BasicConstraintsError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(BasicConstraintsError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(BasicConstraintsError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(BasicConstraintsError::Malformed("truncated long-form length"));
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
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), BasicConstraintsError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(BasicConstraintsError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(BasicConstraintsError::Malformed(
                "truncated: content shorter than its length",
            ));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names
    /// the field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], BasicConstraintsError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(BasicConstraintsError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Extract a certificate's `basicConstraints` extension (RFC 5280 §4.2.1.9).
///
/// `cert_der` is one X.509 certificate — a `CertificateEntry.cert_data` from the
/// server's `Certificate` message (RFC 8446 §4.4.2). The result reports whether the
/// certificate asserts `cA = TRUE` and its optional `pathLenConstraint`. A certificate
/// with no `extensions` field, or an `extensions` field without a `basicConstraints`
/// entry, yields [`BasicConstraints::ABSENT`] (`cA = FALSE`, RFC 5280 §4.2.1.9).
///
/// # Errors
///
/// [`BasicConstraintsError::Malformed`] if the certificate DER is truncated or does not
/// decode to a `TBSCertificate`, or if the `basicConstraints` extension value is present
/// but does not decode to a `SEQUENCE { cA BOOLEAN?, pathLenConstraint INTEGER? }`.
pub fn certificate_basic_constraints(
    cert_der: &[u8],
) -> Result<BasicConstraints, BasicConstraintsError> {
    let extensions = match tbs_extensions(cert_der)? {
        Some(extensions) => extensions,
        // No `extensions` field at all: pre-v3 or a v3 certificate that omitted the
        // optional field. RFC 5280 §4.2.1.9: no basicConstraints means cA = FALSE.
        None => return Ok(BasicConstraints::ABSENT),
    };

    // Extensions ::= SEQUENCE OF Extension. Scan for the basicConstraints entry.
    let mut extensions = Der::new(extensions);
    while !extensions.is_empty() {
        let extension = extensions.read_tagged(TAG_SEQUENCE, "extension is not a SEQUENCE")?;
        let mut extension = Der::new(extension);
        let oid = extension.read_tagged(TAG_OID, "extension has no OID")?;
        if oid != OID_BASIC_CONSTRAINTS {
            continue;
        }
        // Extension ::= SEQUENCE { extnID, critical BOOLEAN DEFAULT FALSE, extnValue }.
        // Skip the optional `critical` BOOLEAN if present; extnValue is the OCTET STRING.
        if extension.peek_tag() == Some(TAG_BOOLEAN) {
            extension.read_tlv()?;
        }
        let extn_value = extension.read_tagged(TAG_OCTET_STRING, "extnValue is not an OCTET STRING")?;
        return parse_basic_constraints(extn_value);
    }

    // extensions present, but no basicConstraints among them: cA = FALSE.
    Ok(BasicConstraints::ABSENT)
}

/// Navigate a certificate's `TBSCertificate` to its `extensions` field (RFC 5280
/// §4.1.2.9), returning the contents of the `[3] EXPLICIT` wrapper's inner `SEQUENCE OF
/// Extension`, or `None` if the certificate carries no `extensions` field.
fn tbs_extensions(cert_der: &[u8]) -> Result<Option<&[u8]>, BasicConstraintsError> {
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

    // The remaining fields — issuerUniqueID [1], subjectUniqueID [2], extensions [3] —
    // are all optional. Skip the unique-ID fields and stop at the extensions wrapper;
    // anything past extensions does not exist.
    while let Some(tag) = tbs.peek_tag() {
        if tag == TAG_CONTEXT_3 {
            // extensions [3] EXPLICIT SEQUENCE OF Extension: the [3] wrapper's contents
            // are the SEQUENCE OF Extension, whose contents are the extensions.
            let wrapper = tbs.read_tlv()?.1;
            let inner = Der::new(wrapper)
                .read_tagged(TAG_SEQUENCE, "extensions is not a SEQUENCE")?;
            return Ok(Some(inner));
        }
        // issuerUniqueID [1] / subjectUniqueID [2] (or any earlier-terminating field):
        // skip without interpreting.
        tbs.read_tlv()?;
    }

    Ok(None)
}

/// Decode a `basicConstraints` extension value (RFC 5280 §4.2.1.9): the DER of
/// `SEQUENCE { cA BOOLEAN DEFAULT FALSE, pathLenConstraint INTEGER (0..MAX) OPTIONAL }`.
fn parse_basic_constraints(extn_value: &[u8]) -> Result<BasicConstraints, BasicConstraintsError> {
    let seq = Der::new(extn_value)
        .read_tagged(TAG_SEQUENCE, "basicConstraints is not a SEQUENCE")?;
    let mut seq = Der::new(seq);

    // cA BOOLEAN DEFAULT FALSE — present only when TRUE in a canonical encoding, but
    // accept an explicit FALSE too. DER encodes TRUE as 0xFF and FALSE as 0x00.
    let is_ca = if seq.peek_tag() == Some(TAG_BOOLEAN) {
        let value = seq.read_tagged(TAG_BOOLEAN, "cA is not a BOOLEAN")?;
        match value {
            [0x00] => false,
            [0xFF] => true,
            _ => return Err(BasicConstraintsError::Malformed("cA is not a canonical BOOLEAN")),
        }
    } else {
        false
    };

    // pathLenConstraint INTEGER (0..MAX) OPTIONAL — a non-negative integer, big-endian,
    // minimally encoded (a leading 0x00 only to keep the high bit clear).
    let path_len_constraint = if seq.peek_tag() == Some(TAG_INTEGER) {
        let value = seq.read_tagged(TAG_INTEGER, "pathLenConstraint is not an INTEGER")?;
        Some(parse_non_negative_u32(value)?)
    } else {
        None
    };

    Ok(BasicConstraints { is_ca, path_len_constraint, present: true })
}

/// Decode a DER `INTEGER` known to be a small non-negative value (`pathLenConstraint`,
/// RFC 5280 §4.2.1.9). Rejects an empty, negative, or over-wide encoding.
fn parse_non_negative_u32(bytes: &[u8]) -> Result<u32, BasicConstraintsError> {
    let (&first, rest) = bytes
        .split_first()
        .ok_or(BasicConstraintsError::Malformed("empty pathLenConstraint INTEGER"))?;
    if first & 0x80 != 0 {
        return Err(BasicConstraintsError::Malformed("negative pathLenConstraint"));
    }
    // A single leading 0x00 is the DER padding that keeps a high-bit-set value positive;
    // strip it, then require the magnitude to fit in a u32.
    let magnitude = if first == 0x00 { rest } else { bytes };
    if magnitude.len() > 4 {
        return Err(BasicConstraintsError::Malformed("pathLenConstraint exceeds u32"));
    }
    let mut value = 0u32;
    for &b in magnitude {
        value = (value << 8) | b as u32;
    }
    Ok(value)
}

/// Why enforcing `basicConstraints` across a certificate chain failed (RFC 5280
/// §4.2.1.9, §6.1.4, RFC 8446 §4.4.2). Each variant pinpoints the certificate — by its
/// position in the server's `certificate_list` — at which enforcement broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaConstraintsWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity
    /// certificate first, so there is nothing to walk — a malformed `Certificate`
    /// message.
    EmptyChain,
    /// A certificate's `basicConstraints` could not be extracted: its DER failed to
    /// decode to a `TBSCertificate`, or its `basicConstraints` value was malformed.
    Certificate {
        /// Position, in the `certificate_list`, of the certificate whose
        /// `basicConstraints` failed to extract.
        index: usize,
        /// The underlying extraction failure.
        error: BasicConstraintsError,
    },
    /// The certificate at `index` issues the certificate below it but does not assert
    /// `cA = TRUE` (RFC 5280 §4.2.1.9): it is not permitted to act as a CA. A leaf
    /// certificate masquerading as an intermediate.
    NotaCa {
        /// Position, in the `certificate_list`, of the issuing certificate that is not a
        /// CA.
        index: usize,
    },
    /// The certificate at `index` asserts a `pathLenConstraint` smaller than the number
    /// of intermediate certificates that follow it toward the leaf (RFC 5280
    /// §4.2.1.9, §6.1.4(m)): the chain is longer than this CA permits beneath it.
    PathLenExceeded {
        /// Position, in the `certificate_list`, of the CA whose `pathLenConstraint` is
        /// exceeded.
        index: usize,
    },
}

impl core::fmt::Display for CaConstraintsWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::Certificate { index, error } => write!(f, "certificate #{index}: {error}"),
            Self::NotaCa { index } => {
                write!(f, "certificate #{index} issues another but is not a CA (basicConstraints cA is not TRUE)")
            }
            Self::PathLenExceeded { index } => write!(
                f,
                "certificate #{index} pathLenConstraint is smaller than the number of intermediates below it"
            ),
        }
    }
}

impl std::error::Error for CaConstraintsWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Certificate { error, .. } => Some(error),
            Self::EmptyChain | Self::NotaCa { .. } | Self::PathLenExceeded { .. } => None,
        }
    }
}

/// Verify that every issuing certificate in a server-presented chain is a permitted CA
/// (RFC 5280 §4.2.1.9, §6.1.4, RFC 8446 §4.4.2).
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message —
/// the end-entity certificate first, then each issuing intermediate. Every certificate
/// but the leaf issues the one below it, so each of `chain[1..]` must assert
/// `cA = TRUE`; the leaf (`chain[0]`) is the subject of the connection, not an issuer,
/// and is not required to be a CA. Where an issuing certificate carries a
/// `pathLenConstraint`, this checks that the number of intermediate certificates between
/// it and the leaf — `index - 1` of them — does not exceed it (RFC 5280 §6.1.4(m)).
///
/// A single-element chain has no issuing certificate and verifies vacuously; an empty
/// chain is [`CaConstraintsWalkError::EmptyChain`].
///
/// This is the `basicConstraints` complement of the signature walk
/// ([`verify_chain_signatures`](super::x509_chain::verify_chain_signatures)) and the
/// name walk ([`verify_name_chain`](super::x509_name_chain::verify_name_chain)): a chain
/// should pass **all three**. This alone does **not** verify signatures or names,
/// terminate at a trust anchor (RFC 5280 §6.1), or honour `keyUsage` (§4.2.1.3) — those
/// are sibling and later slices.
///
/// # Errors
///
/// - [`CaConstraintsWalkError::EmptyChain`] if `chain` is empty.
/// - [`CaConstraintsWalkError::Certificate`] if a certificate's `basicConstraints`
///   cannot be extracted, naming that certificate's position.
/// - [`CaConstraintsWalkError::NotaCa`] if an issuing certificate is not a CA, naming its
///   position.
/// - [`CaConstraintsWalkError::PathLenExceeded`] if an issuing certificate's
///   `pathLenConstraint` is smaller than the number of intermediates below it, naming its
///   position.
pub fn verify_ca_constraints(chain: &[&[u8]]) -> Result<(), CaConstraintsWalkError> {
    if chain.is_empty() {
        return Err(CaConstraintsWalkError::EmptyChain);
    }

    // Every certificate above the leaf issues the one below it and so must be a CA. At
    // position `index`, the certificates between this one and the leaf are chain[1..index]
    // — `index - 1` intermediates — which a pathLenConstraint here must accommodate.
    for (index, cert_der) in chain.iter().enumerate().skip(1) {
        let constraints = certificate_basic_constraints(cert_der)
            .map_err(|error| CaConstraintsWalkError::Certificate { index, error })?;
        if !constraints.is_ca {
            return Err(CaConstraintsWalkError::NotaCa { index });
        }
        if let Some(max) = constraints.path_len_constraint {
            let intermediates_below = (index - 1) as u32;
            if intermediates_below > max {
                return Err(CaConstraintsWalkError::PathLenExceeded { index });
            }
        }
    }

    Ok(())
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

    /// A `basicConstraints` extension: `SEQUENCE { OID 2.5.29.19, [critical] BOOLEAN?,
    /// OCTET STRING { SEQUENCE { cA BOOLEAN?, pathLen INTEGER? } } }`.
    fn basic_constraints_ext(
        critical: bool,
        ca: Option<bool>,
        path_len: Option<u32>,
    ) -> Vec<u8> {
        let mut inner = Vec::new();
        if let Some(ca) = ca {
            inner.extend_from_slice(&tlv(TAG_BOOLEAN, &[if ca { 0xFF } else { 0x00 }]));
        }
        if let Some(path_len) = path_len {
            // Minimal big-endian encoding, with a 0x00 pad if the high bit would be set.
            let mut octets: Vec<u8> = path_len.to_be_bytes().to_vec();
            while octets.len() > 1 && octets[0] == 0 {
                octets.remove(0);
            }
            if octets[0] & 0x80 != 0 {
                octets.insert(0, 0x00);
            }
            inner.extend_from_slice(&tlv(TAG_INTEGER, &octets));
        }
        let value = tlv(TAG_SEQUENCE, &inner);
        let octet_string = tlv(TAG_OCTET_STRING, &value);

        let mut parts: Vec<Vec<u8>> = vec![tlv(TAG_OID, OID_BASIC_CONSTRAINTS)];
        if critical {
            parts.push(tlv(TAG_BOOLEAN, &[0xFF]));
        }
        parts.push(octet_string);
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        tlv(TAG_SEQUENCE, &cat(&refs))
    }

    /// A non-basicConstraints extension (a placeholder `keyUsage`-shaped entry) the
    /// scanner must skip past.
    fn other_ext() -> Vec<u8> {
        let oid = tlv(TAG_OID, &[0x55, 0x1D, 0x0F]); // id-ce-keyUsage (2.5.29.15)
        let value = tlv(TAG_OCTET_STRING, &tlv(0x03, &[0x00, 0x80])); // BIT STRING placeholder
        tlv(TAG_SEQUENCE, &cat(&[&oid, &value]))
    }

    /// Assemble a v3 certificate whose `extensions` field is the given list of extension
    /// TLVs (or a certificate with no `extensions` field at all when `extensions` is
    /// `None`). Every field before `extensions` is a placeholder the walker skips.
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
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    /// A CA certificate: `basicConstraints` present with `cA = TRUE` and an optional
    /// `pathLenConstraint`.
    fn ca_cert(path_len: Option<u32>) -> Vec<u8> {
        cert(Some(&[basic_constraints_ext(true, Some(true), path_len)]))
    }

    /// A leaf certificate: `basicConstraints` present with `cA = FALSE`.
    fn leaf_cert() -> Vec<u8> {
        cert(Some(&[basic_constraints_ext(false, Some(false), None)]))
    }

    // ── certificate_basic_constraints: extraction ──────────────────────────

    #[test]
    fn extracts_a_ca_with_no_path_len() {
        let bc = certificate_basic_constraints(&ca_cert(None)).expect("decodes");
        assert_eq!(
            bc,
            BasicConstraints { is_ca: true, path_len_constraint: None, present: true },
        );
    }

    #[test]
    fn extracts_a_ca_with_a_path_len() {
        let bc = certificate_basic_constraints(&ca_cert(Some(3))).expect("decodes");
        assert_eq!(
            bc,
            BasicConstraints { is_ca: true, path_len_constraint: Some(3), present: true },
        );
    }

    #[test]
    fn extracts_an_explicit_non_ca() {
        let bc = certificate_basic_constraints(&leaf_cert()).expect("decodes");
        assert_eq!(
            bc,
            BasicConstraints { is_ca: false, path_len_constraint: None, present: true },
        );
    }

    #[test]
    fn treats_an_absent_extension_as_not_a_ca() {
        // A v3 certificate with an `extensions` field that has no basicConstraints entry.
        let bc = certificate_basic_constraints(&cert(Some(&[other_ext()]))).expect("decodes");
        assert_eq!(bc, BasicConstraints::ABSENT);
        assert!(!bc.present);
    }

    #[test]
    fn treats_no_extensions_field_as_not_a_ca() {
        // A certificate with no `extensions` field at all.
        let bc = certificate_basic_constraints(&cert(None)).expect("decodes");
        assert_eq!(bc, BasicConstraints::ABSENT);
    }

    #[test]
    fn treats_an_empty_basic_constraints_as_not_a_ca() {
        // basicConstraints present but empty: cA defaults to FALSE.
        let bc = certificate_basic_constraints(&cert(Some(&[basic_constraints_ext(
            false, None, None,
        )])))
        .expect("decodes");
        assert_eq!(
            bc,
            BasicConstraints { is_ca: false, path_len_constraint: None, present: true },
        );
    }

    #[test]
    fn finds_basic_constraints_after_another_extension() {
        // The scanner must skip a leading non-basicConstraints extension.
        let bc = certificate_basic_constraints(&cert(Some(&[
            other_ext(),
            basic_constraints_ext(true, Some(true), Some(1)),
        ])))
        .expect("decodes");
        assert_eq!(
            bc,
            BasicConstraints { is_ca: true, path_len_constraint: Some(1), present: true },
        );
    }

    #[test]
    fn parses_a_critical_basic_constraints() {
        // basicConstraints is normally marked critical; the critical BOOLEAN must be
        // skipped to reach the extnValue.
        let bc = certificate_basic_constraints(&cert(Some(&[basic_constraints_ext(
            true,
            Some(true),
            Some(0),
        )])))
        .expect("decodes");
        assert_eq!(
            bc,
            BasicConstraints { is_ca: true, path_len_constraint: Some(0), present: true },
        );
    }

    #[test]
    fn parses_a_multi_octet_path_len() {
        // pathLenConstraint = 200 needs a 0x00 pad (high bit set), exercising the
        // non-negative-integer decoder's leading-zero strip.
        let bc = certificate_basic_constraints(&ca_cert(Some(200))).expect("decodes");
        assert_eq!(bc.path_len_constraint, Some(200));
    }

    // ── certificate_basic_constraints: malformed ───────────────────────────

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            certificate_basic_constraints(&not_a_cert),
            Err(BasicConstraintsError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_certificate() {
        let c = ca_cert(Some(2));
        assert!(matches!(
            certificate_basic_constraints(&c[..c.len() - 4]),
            Err(BasicConstraintsError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_a_negative_path_len() {
        // A pathLenConstraint INTEGER whose high bit is set is negative — illegal.
        let value = tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_BOOLEAN, &[0xFF]), &tlv(TAG_INTEGER, &[0x80])]));
        let octet_string = tlv(TAG_OCTET_STRING, &value);
        let ext = tlv(
            TAG_SEQUENCE,
            &cat(&[&tlv(TAG_OID, OID_BASIC_CONSTRAINTS), &octet_string]),
        );
        assert!(matches!(
            certificate_basic_constraints(&cert(Some(&[ext]))),
            Err(BasicConstraintsError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_a_non_canonical_ca_boolean() {
        // DER requires TRUE = 0xFF; 0x01 is a BER-only encoding this rejects.
        let value = tlv(TAG_SEQUENCE, &tlv(TAG_BOOLEAN, &[0x01]));
        let octet_string = tlv(TAG_OCTET_STRING, &value);
        let ext = tlv(
            TAG_SEQUENCE,
            &cat(&[&tlv(TAG_OID, OID_BASIC_CONSTRAINTS), &octet_string]),
        );
        assert!(matches!(
            certificate_basic_constraints(&cert(Some(&[ext]))),
            Err(BasicConstraintsError::Malformed(_)),
        ));
    }

    // ── verify_ca_constraints: happy paths ─────────────────────────────────

    #[test]
    fn accepts_a_leaf_and_ca_issuer() {
        // leaf ← CA(pathLen 0): the CA below which sits exactly the leaf (0 intermediates).
        verify_ca_constraints(&[&leaf_cert(), &ca_cert(Some(0))])
            .expect("the issuer is a CA that permits the leaf below it");
    }

    #[test]
    fn accepts_a_leaf_and_ca_issuer_without_path_len() {
        // An unconstrained CA imposes no path-length limit.
        verify_ca_constraints(&[&leaf_cert(), &ca_cert(None)]).expect("unconstrained CA");
    }

    #[test]
    fn accepts_a_three_certificate_chain_within_path_len() {
        // leaf ← intermediate(pathLen 0) ← root(pathLen 1). One intermediate sits below
        // the root (index 2 → 1 intermediate), which its pathLen of 1 permits.
        verify_ca_constraints(&[&leaf_cert(), &ca_cert(Some(0)), &ca_cert(Some(1))])
            .expect("each CA permits the intermediates beneath it");
    }

    #[test]
    fn accepts_a_single_certificate_vacuously() {
        // A lone certificate issues nothing, so there is no CA to constrain.
        verify_ca_constraints(&[&leaf_cert()]).expect("a lone certificate has no issuers");
    }

    // ── verify_ca_constraints: rejections ──────────────────────────────────

    #[test]
    fn rejects_a_non_ca_issuer() {
        // The issuer is a leaf (cA = FALSE) masquerading as an intermediate.
        assert_eq!(
            verify_ca_constraints(&[&leaf_cert(), &leaf_cert()]),
            Err(CaConstraintsWalkError::NotaCa { index: 1 }),
        );
    }

    #[test]
    fn rejects_an_issuer_with_no_basic_constraints() {
        // A certificate with no basicConstraints extension is not a CA.
        assert_eq!(
            verify_ca_constraints(&[&leaf_cert(), &cert(None)]),
            Err(CaConstraintsWalkError::NotaCa { index: 1 }),
        );
    }

    #[test]
    fn rejects_a_chain_that_exceeds_path_len() {
        // leaf ← intermediate(pathLen 0) ← root(pathLen 0). The root's pathLen of 0
        // forbids any intermediate below it, but the intermediate at index 1 is one.
        assert_eq!(
            verify_ca_constraints(&[&leaf_cert(), &ca_cert(Some(0)), &ca_cert(Some(0))]),
            Err(CaConstraintsWalkError::PathLenExceeded { index: 2 }),
        );
    }

    #[test]
    fn reports_the_deeper_non_ca_when_both_issuers_break() {
        // Both issuers are non-CAs; the walk reports the first (lowest index) it meets.
        assert_eq!(
            verify_ca_constraints(&[&leaf_cert(), &leaf_cert(), &leaf_cert()]),
            Err(CaConstraintsWalkError::NotaCa { index: 1 }),
        );
    }

    #[test]
    fn rejects_an_issuer_certificate_that_does_not_decode() {
        let garbage = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            verify_ca_constraints(&[&leaf_cert(), &garbage]),
            Err(CaConstraintsWalkError::Certificate { index: 1, error: BasicConstraintsError::Malformed(_) }),
        ));
    }

    #[test]
    fn rejects_an_empty_chain() {
        assert_eq!(
            verify_ca_constraints(&[]),
            Err(CaConstraintsWalkError::EmptyChain),
        );
    }
}
