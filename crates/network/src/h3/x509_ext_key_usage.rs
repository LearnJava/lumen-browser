//! X.509 `extendedKeyUsage` verification (RFC 5280 §4.2.1.12) — slice 80 of the
//! HTTP/3 sprint.
//!
//! The `keyUsage` walk ([`x509_key_usage::verify_cert_sign_usage`](super::x509_key_usage::verify_cert_sign_usage),
//! slice 73) proves each *issuing* key is permitted to sign certificates. RFC 5280
//! §4.2.1.12 carries a second, *purpose-level* permission in the `extendedKeyUsage`
//! extension: it names the application purposes for which the certified public key may be
//! used, beyond the low-level `keyUsage` bits. For a certificate presented as a TLS
//! **server** identity, the relevant purpose is `id-kp-serverAuth` (1.3.6.1.5.5.7.3.1,
//! §4.2.1.12): "TLS WWW server authentication". When the end-entity leaf carries an
//! `extendedKeyUsage` extension, that extension **must** name `serverAuth` (or the
//! catch-all `anyExtendedKeyUsage`, 2.5.29.37.0) or the certificate is not authorised to
//! authenticate a TLS server — every web browser rejects such a leaf. This module is the
//! `extendedKeyUsage` leg: the `serverAuth` companion of the `keyUsage` `keyCertSign`
//! walk, layered beside it exactly as it is layered beside the signature, name, and
//! `basicConstraints` walks.
//!
//! ## What it reads
//!
//! The `extendedKeyUsage` extension inside the `TBSCertificate`'s `extensions` field (RFC
//! 5280 §4.1.2.9, §4.2.1.12):
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
//! Extension  ::= SEQUENCE {
//!     extnID    OBJECT IDENTIFIER,
//!     critical  BOOLEAN DEFAULT FALSE,
//!     extnValue OCTET STRING }                       -- DER of the extension value
//!
//! ExtKeyUsageSyntax ::= SEQUENCE SIZE (1..MAX) OF KeyPurposeId
//! KeyPurposeId      ::= OBJECT IDENTIFIER
//! ```
//!
//! [`certificate_ext_key_usage`] navigates the `TBSCertificate` by field order — skipping
//! the six fields and two optional unique-IDs before `extensions` without interpreting them
//! — locates the `extendedKeyUsage` extension by its OID (`2.5.29.37`), and scans the
//! `SEQUENCE OF` `KeyPurposeId` inside it for `serverAuth` and `anyExtendedKeyUsage`. An
//! absent extension means the key is unrestricted as to purpose (RFC 5280 §4.2.1.12: a
//! certificate that omits `extendedKeyUsage` may be used for any purpose the other
//! extensions permit).
//!
//! ## How the chain is checked
//!
//! [`verify_server_auth_eku`] inspects the end-entity leaf — the first entry (index 0) of
//! the server's `certificate_list` (RFC 8446 §4.4.2), the certificate that *is* the TLS
//! server identity. If that leaf carries an `extendedKeyUsage` extension, it must name
//! `serverAuth` (or `anyExtendedKeyUsage`); a leaf whose `extendedKeyUsage` lists only other
//! purposes (e.g. `clientAuth`, `codeSigning`, `emailProtection`) is rejected. A leaf with
//! **no** `extendedKeyUsage` extension is not rejected here — RFC 5280 §4.2.1.12 restricts a
//! certificate's purposes only when the extension is present. The issuing certificates
//! (`chain[1..]`) are not consulted: RFC 5280 scopes `extendedKeyUsage` to the certificate
//! that carries it, and does not mandate EKU chaining up the path.
//!
//! ## What it does *not* do
//!
//! This confirms only that the *presented* leaf is authorised for TLS server
//! authentication — the fifth leg beside the signature walk ([`x509_chain`](super::x509_chain)),
//! the name walk ([`x509_name_chain`](super::x509_name_chain)), the `basicConstraints` walk
//! ([`x509_basic_constraints`](super::x509_basic_constraints)), and the `keyUsage` walk
//! ([`x509_key_usage`](super::x509_key_usage)). It does **not** verify signatures or names,
//! enforce `cA = TRUE` or the path length, interpret the `keyUsage` bits, terminate the
//! chain at a trusted root (RFC 5280 §6.1, the trust-anchor walk), or interpret purposes
//! other than `serverAuth`. Like the sibling walks it is a pure check over the presented
//! list; wiring it into the connect loop is a later slice (as slice 74 followed slice 73 for
//! `keyUsage`).
//!
//! ## Purity
//!
//! Pure DER parsing over borrowed certificate bytes: no clock, no I/O, no allocation beyond
//! the caller's slices. A sibling of [`x509_spki`](super::x509_spki),
//! [`x509_hostname`](super::x509_hostname), [`x509_validity`](super::x509_validity),
//! [`x509_chain`](super::x509_chain), [`x509_name_chain`](super::x509_name_chain),
//! [`x509_basic_constraints`](super::x509_basic_constraints), and
//! [`x509_key_usage`](super::x509_key_usage).

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

/// `id-ce-extKeyUsage` (2.5.29.37, RFC 5280 §4.2.1.12) — the extension OID whose value is
/// the `SEQUENCE OF` `KeyPurposeId` naming the authorised purposes.
const OID_EXT_KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x25];

/// `id-kp-serverAuth` (1.3.6.1.5.5.7.3.1, RFC 5280 §4.2.1.12) — TLS WWW server
/// authentication, the purpose a TLS server leaf must be authorised for.
const OID_SERVER_AUTH: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];

/// `anyExtendedKeyUsage` (2.5.29.37.0, RFC 5280 §4.2.1.12) — the catch-all purpose that
/// authorises the key for every purpose, `serverAuth` included.
const OID_ANY_EXT_KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x25, 0x00];

/// The decoded `extendedKeyUsage` extension of one certificate (RFC 5280 §4.2.1.12): which
/// of the purposes this walk cares about it authorises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtKeyUsage {
    /// Whether the `extendedKeyUsage` extension named `id-kp-serverAuth`.
    server_auth: bool,
    /// Whether the `extendedKeyUsage` extension named `anyExtendedKeyUsage` (2.5.29.37.0),
    /// which authorises every purpose.
    any: bool,
    /// Whether the `extendedKeyUsage` extension was present at all. `false` means the
    /// certificate carried no such extension, which RFC 5280 §4.2.1.12 treats as
    /// purpose-unrestricted; this field lets a caller distinguish "restricted, but
    /// `serverAuth` absent" from "silent about purpose".
    pub present: bool,
}

impl ExtKeyUsage {
    /// The value for a certificate with no `extendedKeyUsage` extension: no purpose named and
    /// [`present`](ExtKeyUsage::present) `false` (RFC 5280 §4.2.1.12 places no purpose
    /// restriction on a certificate that omits the extension).
    const ABSENT: Self = Self { server_auth: false, any: false, present: false };

    /// Whether this certificate is authorised for TLS server authentication (RFC 5280
    /// §4.2.1.12): the extension is absent (unrestricted), or it names `serverAuth`, or it
    /// names the catch-all `anyExtendedKeyUsage`.
    pub fn allows_server_auth(&self) -> bool {
        !self.present || self.server_auth || self.any
    }
}

/// Why extracting a certificate's `extendedKeyUsage` extension failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtKeyUsageError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag where the
    /// `Certificate`/`TBSCertificate`/extension structure required a specific one. Carries a
    /// static hint naming the field that did not decode.
    Malformed(&'static str),
}

impl core::fmt::Display for ExtKeyUsageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
        }
    }
}

impl std::error::Error for ExtKeyUsageError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples left to
/// right. Definite-length only (DER forbids the indefinite form). A sibling of the readers
/// in [`x509_spki`](super::x509_spki), [`x509_hostname`](super::x509_hostname),
/// [`x509_validity`](super::x509_validity), [`x509_chain`](super::x509_chain),
/// [`x509_name_chain`](super::x509_name_chain),
/// [`x509_basic_constraints`](super::x509_basic_constraints), and
/// [`x509_key_usage`](super::x509_key_usage), specialised to this slice's error type.
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
    fn read_length(&mut self) -> Result<usize, ExtKeyUsageError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(ExtKeyUsageError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(ExtKeyUsageError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(ExtKeyUsageError::Malformed("truncated long-form length"));
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
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), ExtKeyUsageError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(ExtKeyUsageError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(ExtKeyUsageError::Malformed(
                "truncated: content shorter than its length",
            ));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names the
    /// field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], ExtKeyUsageError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(ExtKeyUsageError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Extract a certificate's `extendedKeyUsage` extension (RFC 5280 §4.2.1.12).
///
/// `cert_der` is one X.509 certificate — a `CertificateEntry.cert_data` from the server's
/// `Certificate` message (RFC 8446 §4.4.2). The result reports whether the certificate
/// authorises TLS server authentication. A certificate with no `extensions` field, or an
/// `extensions` field without an `extendedKeyUsage` entry, yields [`ExtKeyUsage::ABSENT`] (a
/// purpose-unrestricted certificate, RFC 5280 §4.2.1.12).
///
/// # Errors
///
/// [`ExtKeyUsageError::Malformed`] if the certificate DER is truncated or does not decode to
/// a `TBSCertificate`, or if the `extendedKeyUsage` extension value is present but does not
/// decode to a `SEQUENCE OF` `OBJECT IDENTIFIER`.
pub fn certificate_ext_key_usage(cert_der: &[u8]) -> Result<ExtKeyUsage, ExtKeyUsageError> {
    let extensions = match tbs_extensions(cert_der)? {
        Some(extensions) => extensions,
        // No `extensions` field at all: pre-v3 or a v3 certificate that omitted the optional
        // field. RFC 5280 §4.2.1.12: no extendedKeyUsage means the purposes are unrestricted.
        None => return Ok(ExtKeyUsage::ABSENT),
    };

    // Extensions ::= SEQUENCE OF Extension. Scan for the extendedKeyUsage entry.
    let mut extensions = Der::new(extensions);
    while !extensions.is_empty() {
        let extension = extensions.read_tagged(TAG_SEQUENCE, "extension is not a SEQUENCE")?;
        let mut extension = Der::new(extension);
        let oid = extension.read_tagged(TAG_OID, "extension has no OID")?;
        if oid != OID_EXT_KEY_USAGE {
            continue;
        }
        // Extension ::= SEQUENCE { extnID, critical BOOLEAN DEFAULT FALSE, extnValue }.
        // Skip the optional `critical` BOOLEAN if present; extnValue is the OCTET STRING.
        if extension.peek_tag() == Some(TAG_BOOLEAN) {
            extension.read_tlv()?;
        }
        let extn_value =
            extension.read_tagged(TAG_OCTET_STRING, "extnValue is not an OCTET STRING")?;
        return parse_ext_key_usage(extn_value);
    }

    // extensions present, but no extendedKeyUsage among them: unrestricted purposes.
    Ok(ExtKeyUsage::ABSENT)
}

/// Navigate a certificate's `TBSCertificate` to its `extensions` field (RFC 5280 §4.1.2.9),
/// returning the contents of the `[3] EXPLICIT` wrapper's inner `SEQUENCE OF Extension`, or
/// `None` if the certificate carries no `extensions` field.
fn tbs_extensions(cert_der: &[u8]) -> Result<Option<&[u8]>, ExtKeyUsageError> {
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

/// Decode an `extendedKeyUsage` extension value (RFC 5280 §4.2.1.12): the DER of a
/// `SEQUENCE OF` `KeyPurposeId` (each an `OBJECT IDENTIFIER`). Reports whether `serverAuth`
/// and/or `anyExtendedKeyUsage` appear among the purposes.
fn parse_ext_key_usage(extn_value: &[u8]) -> Result<ExtKeyUsage, ExtKeyUsageError> {
    let purposes = Der::new(extn_value)
        .read_tagged(TAG_SEQUENCE, "extendedKeyUsage is not a SEQUENCE")?;

    let mut server_auth = false;
    let mut any = false;
    let mut purposes = Der::new(purposes);
    while !purposes.is_empty() {
        let oid = purposes.read_tagged(TAG_OID, "KeyPurposeId is not an OBJECT IDENTIFIER")?;
        if oid == OID_SERVER_AUTH {
            server_auth = true;
        } else if oid == OID_ANY_EXT_KEY_USAGE {
            any = true;
        }
        // Any other purpose (clientAuth, codeSigning, …) is recorded by omission: it neither
        // grants nor forbids serverAuth on its own.
    }

    Ok(ExtKeyUsage { server_auth, any, present: true })
}

/// Why enforcing `extendedKeyUsage` on a server-presented chain failed (RFC 5280 §4.2.1.12,
/// RFC 8446 §4.4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtKeyUsageWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity certificate
    /// first, so there is no leaf to inspect — a malformed `Certificate` message.
    EmptyChain,
    /// The leaf certificate's `extendedKeyUsage` could not be extracted: its DER failed to
    /// decode to a `TBSCertificate`, or its `extendedKeyUsage` value was malformed.
    Certificate {
        /// Position, in the `certificate_list`, of the certificate whose `extendedKeyUsage`
        /// failed to extract. Always `0` (the leaf) for this walk.
        index: usize,
        /// The underlying extraction failure.
        error: ExtKeyUsageError,
    },
    /// The leaf carries an `extendedKeyUsage` extension, but it names neither `serverAuth`
    /// nor `anyExtendedKeyUsage` (RFC 5280 §4.2.1.12): the certificate is not authorised to
    /// authenticate a TLS server.
    NotServerAuth {
        /// Position, in the `certificate_list`, of the leaf whose `extendedKeyUsage` omits
        /// `serverAuth`. Always `0`.
        index: usize,
    },
}

impl core::fmt::Display for ExtKeyUsageWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::Certificate { index, error } => write!(f, "certificate #{index}: {error}"),
            Self::NotServerAuth { index } => write!(
                f,
                "certificate #{index} extendedKeyUsage does not authorise TLS server authentication"
            ),
        }
    }
}

impl std::error::Error for ExtKeyUsageWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Certificate { error, .. } => Some(error),
            Self::EmptyChain | Self::NotServerAuth { .. } => None,
        }
    }
}

/// Verify that a server-presented chain's end-entity leaf is authorised for TLS server
/// authentication by its `extendedKeyUsage` (RFC 5280 §4.2.1.12, RFC 8446 §4.4.2).
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message — the
/// end-entity certificate first, then each issuing intermediate. The leaf (`chain[0]`) is the
/// certificate that *is* the TLS server identity; if it carries an `extendedKeyUsage`
/// extension, that extension must name `serverAuth` (or the catch-all `anyExtendedKeyUsage`).
/// A leaf with **no** `extendedKeyUsage` extension is not restricted by this walk (RFC 5280
/// §4.2.1.12 limits a certificate's purposes only when the extension is present). The issuing
/// certificates (`chain[1..]`) are not consulted: RFC 5280 scopes `extendedKeyUsage` to the
/// certificate that carries it and does not mandate EKU chaining.
///
/// An empty chain is [`ExtKeyUsageWalkError::EmptyChain`].
///
/// This is the `serverAuth` complement of the `keyUsage` walk
/// ([`verify_cert_sign_usage`](super::x509_key_usage::verify_cert_sign_usage)): a chain
/// should pass **both**, together with the signature walk
/// ([`verify_chain_signatures`](super::x509_chain::verify_chain_signatures)), the name walk
/// ([`verify_name_chain`](super::x509_name_chain::verify_name_chain)), and the
/// `basicConstraints` walk
/// ([`verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints)). This
/// alone does **not** verify signatures or names, enforce `cA = TRUE` or the path length, or
/// terminate at a trust anchor (RFC 5280 §6.1) — those are sibling and later slices.
///
/// # Errors
///
/// - [`ExtKeyUsageWalkError::EmptyChain`] if `chain` is empty.
/// - [`ExtKeyUsageWalkError::Certificate`] if the leaf's `extendedKeyUsage` cannot be
///   extracted.
/// - [`ExtKeyUsageWalkError::NotServerAuth`] if the leaf carries an `extendedKeyUsage`
///   extension that authorises neither `serverAuth` nor `anyExtendedKeyUsage`.
pub fn verify_server_auth_eku(chain: &[&[u8]]) -> Result<(), ExtKeyUsageWalkError> {
    let leaf = chain.first().ok_or(ExtKeyUsageWalkError::EmptyChain)?;

    let usage = certificate_ext_key_usage(leaf)
        .map_err(|error| ExtKeyUsageWalkError::Certificate { index: 0, error })?;
    if !usage.allows_server_auth() {
        return Err(ExtKeyUsageWalkError::NotServerAuth { index: 0 });
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

    /// `id-kp-clientAuth` (1.3.6.1.5.5.7.3.2) — a purpose other than serverAuth, used to
    /// build a leaf whose EKU is present but does not authorise TLS server authentication.
    const OID_CLIENT_AUTH: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02];

    /// An `extendedKeyUsage` extension: `SEQUENCE { OID 2.5.29.37, [critical] BOOLEAN?, OCTET
    /// STRING { SEQUENCE OF KeyPurposeId } }`. Each purpose is one of the raw OID byte slices.
    fn eku_ext(critical: bool, purposes: &[&[u8]]) -> Vec<u8> {
        let purpose_tlvs: Vec<Vec<u8>> = purposes.iter().map(|p| tlv(TAG_OID, p)).collect();
        let purpose_refs: Vec<&[u8]> = purpose_tlvs.iter().map(|p| p.as_slice()).collect();
        let seq = tlv(TAG_SEQUENCE, &cat(&purpose_refs));
        let octet_string = tlv(TAG_OCTET_STRING, &seq);

        let mut parts: Vec<Vec<u8>> = vec![tlv(TAG_OID, OID_EXT_KEY_USAGE)];
        if critical {
            parts.push(tlv(TAG_BOOLEAN, &[0xFF]));
        }
        parts.push(octet_string);
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        tlv(TAG_SEQUENCE, &cat(&refs))
    }

    /// A non-EKU extension (a placeholder `basicConstraints`-shaped entry) the scanner must
    /// skip past.
    fn other_ext() -> Vec<u8> {
        let oid = tlv(TAG_OID, &[0x55, 0x1D, 0x13]); // id-ce-basicConstraints (2.5.29.19)
        let value = tlv(TAG_OCTET_STRING, &tlv(TAG_SEQUENCE, &tlv(TAG_BOOLEAN, &[0xFF])));
        tlv(TAG_SEQUENCE, &cat(&[&oid, &value]))
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

    /// A leaf certificate authorised for TLS server authentication: `extendedKeyUsage`
    /// present, naming `serverAuth` (and `clientAuth`, as a real dual-purpose leaf carries).
    fn server_auth_leaf() -> Vec<u8> {
        cert(Some(&[eku_ext(false, &[OID_SERVER_AUTH, OID_CLIENT_AUTH])]))
    }

    // ── certificate_ext_key_usage: extraction ──────────────────────────────

    #[test]
    fn extracts_a_leaf_authorising_server_auth() {
        let eku = certificate_ext_key_usage(&server_auth_leaf()).expect("decodes");
        assert!(eku.present);
        assert!(eku.server_auth);
        assert!(!eku.any);
        assert!(eku.allows_server_auth());
    }

    #[test]
    fn extracts_a_leaf_authorising_only_client_auth() {
        let eku =
            certificate_ext_key_usage(&cert(Some(&[eku_ext(true, &[OID_CLIENT_AUTH])]))).expect("decodes");
        assert!(eku.present);
        assert!(!eku.server_auth);
        assert!(!eku.any);
        assert!(!eku.allows_server_auth());
    }

    #[test]
    fn accepts_any_extended_key_usage() {
        let eku = certificate_ext_key_usage(&cert(Some(&[eku_ext(false, &[OID_ANY_EXT_KEY_USAGE])])))
            .expect("decodes");
        assert!(eku.present);
        assert!(!eku.server_auth);
        assert!(eku.any);
        assert!(eku.allows_server_auth());
    }

    #[test]
    fn treats_an_absent_extension_as_unrestricted() {
        // A v3 certificate with an `extensions` field that has no extendedKeyUsage entry.
        let eku = certificate_ext_key_usage(&cert(Some(&[other_ext()]))).expect("decodes");
        assert_eq!(eku, ExtKeyUsage::ABSENT);
        assert!(!eku.present);
        assert!(eku.allows_server_auth());
    }

    #[test]
    fn treats_no_extensions_field_as_unrestricted() {
        // A certificate with no `extensions` field at all.
        let eku = certificate_ext_key_usage(&cert(None)).expect("decodes");
        assert_eq!(eku, ExtKeyUsage::ABSENT);
        assert!(eku.allows_server_auth());
    }

    #[test]
    fn finds_eku_after_another_extension() {
        // The scanner must skip a leading non-EKU extension.
        let eku = certificate_ext_key_usage(&cert(Some(&[
            other_ext(),
            eku_ext(true, &[OID_SERVER_AUTH]),
        ])))
        .expect("decodes");
        assert!(eku.server_auth);
    }

    #[test]
    fn parses_a_critical_eku() {
        // extendedKeyUsage with the critical BOOLEAN must still decode.
        let eku =
            certificate_ext_key_usage(&cert(Some(&[eku_ext(true, &[OID_SERVER_AUTH])]))).expect("decodes");
        assert!(eku.server_auth);
    }

    #[test]
    fn finds_server_auth_among_several_purposes() {
        // serverAuth is the third of four listed purposes.
        let code_signing: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x03];
        let email: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x04];
        let eku = certificate_ext_key_usage(&cert(Some(&[eku_ext(
            false,
            &[OID_CLIENT_AUTH, code_signing, OID_SERVER_AUTH, email],
        )])))
        .expect("decodes");
        assert!(eku.server_auth);
        assert!(eku.allows_server_auth());
    }

    // ── certificate_ext_key_usage: malformed ───────────────────────────────

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            certificate_ext_key_usage(&not_a_cert),
            Err(ExtKeyUsageError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_certificate() {
        let c = server_auth_leaf();
        assert!(matches!(
            certificate_ext_key_usage(&c[..c.len() - 4]),
            Err(ExtKeyUsageError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_a_non_sequence_eku_value() {
        // extnValue OCTET STRING wrapping an INTEGER instead of a SEQUENCE OF OID.
        let bad_value = tlv(TAG_OCTET_STRING, &tlv(TAG_INTEGER, &[0x01]));
        let ext = tlv(
            TAG_SEQUENCE,
            &cat(&[&tlv(TAG_OID, OID_EXT_KEY_USAGE), &bad_value]),
        );
        assert!(matches!(
            certificate_ext_key_usage(&cert(Some(&[ext]))),
            Err(ExtKeyUsageError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_a_non_oid_purpose() {
        // A SEQUENCE OF whose element is an INTEGER, not an OBJECT IDENTIFIER.
        let seq = tlv(TAG_SEQUENCE, &tlv(TAG_INTEGER, &[0x01]));
        let value = tlv(TAG_OCTET_STRING, &seq);
        let ext = tlv(
            TAG_SEQUENCE,
            &cat(&[&tlv(TAG_OID, OID_EXT_KEY_USAGE), &value]),
        );
        assert!(matches!(
            certificate_ext_key_usage(&cert(Some(&[ext]))),
            Err(ExtKeyUsageError::Malformed(_)),
        ));
    }

    // ── verify_server_auth_eku: the chain walk ─────────────────────────────

    #[test]
    fn walk_accepts_a_server_auth_leaf() {
        let leaf = server_auth_leaf();
        let ca = cert(Some(&[other_ext()]));
        let chain: Vec<&[u8]> = vec![&leaf, &ca];
        assert_eq!(verify_server_auth_eku(&chain), Ok(()));
    }

    #[test]
    fn walk_accepts_a_leaf_with_no_eku() {
        // A leaf that omits extendedKeyUsage is unrestricted → accepted.
        let leaf = cert(None);
        let chain: Vec<&[u8]> = vec![&leaf];
        assert_eq!(verify_server_auth_eku(&chain), Ok(()));
    }

    #[test]
    fn walk_accepts_an_any_eku_leaf() {
        let leaf = cert(Some(&[eku_ext(false, &[OID_ANY_EXT_KEY_USAGE])]));
        let chain: Vec<&[u8]> = vec![&leaf];
        assert_eq!(verify_server_auth_eku(&chain), Ok(()));
    }

    #[test]
    fn walk_rejects_a_client_auth_only_leaf() {
        let leaf = cert(Some(&[eku_ext(true, &[OID_CLIENT_AUTH])]));
        let chain: Vec<&[u8]> = vec![&leaf];
        assert_eq!(
            verify_server_auth_eku(&chain),
            Err(ExtKeyUsageWalkError::NotServerAuth { index: 0 }),
        );
    }

    #[test]
    fn walk_ignores_a_client_auth_only_intermediate() {
        // Only the leaf's EKU is consulted; a restrictive intermediate does not sink a
        // server-auth leaf (RFC 5280 does not mandate EKU chaining).
        let leaf = server_auth_leaf();
        let ca = cert(Some(&[eku_ext(true, &[OID_CLIENT_AUTH])]));
        let chain: Vec<&[u8]> = vec![&leaf, &ca];
        assert_eq!(verify_server_auth_eku(&chain), Ok(()));
    }

    #[test]
    fn walk_rejects_an_empty_chain() {
        let chain: Vec<&[u8]> = vec![];
        assert_eq!(
            verify_server_auth_eku(&chain),
            Err(ExtKeyUsageWalkError::EmptyChain),
        );
    }

    #[test]
    fn walk_reports_a_malformed_leaf() {
        let good = server_auth_leaf();
        let truncated = good[..good.len() - 4].to_vec();
        let chain: Vec<&[u8]> = vec![&truncated];
        assert!(matches!(
            verify_server_auth_eku(&chain),
            Err(ExtKeyUsageWalkError::Certificate { index: 0, .. }),
        ));
    }

    #[test]
    fn walk_error_display_names_the_index() {
        let e = ExtKeyUsageWalkError::NotServerAuth { index: 0 };
        assert!(e.to_string().contains("#0"));
        assert!(ExtKeyUsageWalkError::EmptyChain.to_string().contains("empty"));
    }
}
