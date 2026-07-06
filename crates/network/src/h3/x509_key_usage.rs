//! X.509 `keyUsage` verification (RFC 5280 §4.2.1.3, §6.1.4) — slice 73 of the
//! HTTP/3 sprint.
//!
//! The signature walk ([`x509_chain::verify_chain_signatures`](super::x509_chain::verify_chain_signatures),
//! slices 66–68) proves each certificate is *signed* by the one above it, the name walk
//! ([`x509_name_chain::verify_name_chain`](super::x509_name_chain::verify_name_chain),
//! slices 69–70) proves each certificate *names* the one above it as its issuer, and the
//! `basicConstraints` walk ([`x509_basic_constraints::verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints),
//! slices 71–72) proves each *issuing* certificate is a permitted CA with an adequate
//! path length. `basicConstraints` carries the *structural* permission to act as a CA;
//! RFC 5280 §4.2.1.3 carries a second, *key-level* permission in the `keyUsage`
//! extension. When a certificate has a `keyUsage` extension, its public key may verify
//! certificate signatures only if the `keyCertSign` bit (bit 5) is asserted — an issuer
//! whose `keyUsage` omits `keyCertSign` is not permitted to sign the certificate below
//! it, even if `basicConstraints` says `cA = TRUE`. This module is the `keyUsage` leg:
//! the `keyCertSign` companion of the `basicConstraints` walk, layered beside it exactly
//! as it is layered beside the signature and name walks.
//!
//! ## What it reads
//!
//! The `keyUsage` extension inside the `TBSCertificate`'s `extensions` field (RFC 5280
//! §4.1.2.9, §4.2.1.3):
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
//! KeyUsage ::= BIT STRING {
//!     digitalSignature (0), nonRepudiation   (1), keyEncipherment (2),
//!     dataEncipherment (3), keyAgreement     (4), keyCertSign     (5),
//!     cRLSign          (6), encipherOnly     (7), decipherOnly    (8) }
//! ```
//!
//! [`certificate_key_usage`] navigates the `TBSCertificate` by field order — skipping the
//! six fields and two optional unique-IDs before `extensions` without interpreting them —
//! locates the `keyUsage` extension by its OID (`2.5.29.15`), and decodes the named bits
//! of its `BIT STRING`. An absent extension means the key is unrestricted (RFC 5280
//! §4.2.1.3 places no `keyUsage` limit on a certificate that omits the extension).
//!
//! ## How the chain is checked
//!
//! [`verify_cert_sign_usage`] walks the server-presented `certificate_list`. For every
//! certificate that *issues* another — every certificate but the end-entity leaf at index
//! 0 — that carries a `keyUsage` extension, it requires the `keyCertSign` bit (RFC 5280
//! §4.2.1.3): a key that would sign the certificate below it must be permitted to.
//! An issuing certificate with **no** `keyUsage` extension is not rejected here — RFC 5280
//! §4.2.1.3 restricts a key only when the extension is present, so the absent case is left
//! to the `basicConstraints` walk that already required `cA = TRUE`.
//!
//! ## What it does *not* do
//!
//! This confirms only that each *presented* issuing key is *permitted* to sign
//! certificates — the fourth leg beside the signature walk ([`x509_chain`](super::x509_chain)),
//! the name walk ([`x509_name_chain`](super::x509_name_chain)), and the `basicConstraints`
//! walk ([`x509_basic_constraints`](super::x509_basic_constraints)). It does **not** verify
//! signatures or names, enforce `cA = TRUE` or the path length (that is the
//! `basicConstraints` walk), terminate the chain at a trusted root (RFC 5280 §6.1, a later
//! slice), or interpret the other `keyUsage` bits (`digitalSignature`, `cRLSign`, …) or the
//! `extendedKeyUsage` extension (§4.2.1.12). Like the sibling walks it is a pure check over
//! the presented list; wiring it into the connect loop is a later slice (as slice 72
//! followed slice 71 for `basicConstraints`).
//!
//! ## Purity
//!
//! Pure DER parsing over borrowed certificate bytes: no clock, no I/O, no allocation
//! beyond the caller's slices. A sibling of [`x509_spki`](super::x509_spki),
//! [`x509_hostname`](super::x509_hostname), [`x509_validity`](super::x509_validity),
//! [`x509_chain`](super::x509_chain), [`x509_name_chain`](super::x509_name_chain), and
//! [`x509_basic_constraints`](super::x509_basic_constraints).

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for `BOOLEAN`.
const TAG_BOOLEAN: u8 = 0x01;
/// The DER tag for `BIT STRING`.
const TAG_BIT_STRING: u8 = 0x03;
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

/// `id-ce-keyUsage` (2.5.29.15, RFC 5280 §4.2.1.3) — the extension OID whose value is the
/// `BIT STRING` of key-usage named bits.
const OID_KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x0F];

/// A named bit of the `keyUsage` `BIT STRING` (RFC 5280 §4.2.1.3). The associated value is
/// the bit's position in the `BIT STRING` (bit 0 is the most-significant bit of the first
/// value octet), which is how the extension numbers each usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyUsageBit {
    /// `digitalSignature (0)`: verifying digital signatures other than on certificates or
    /// CRLs (e.g. entity authentication, signed data).
    DigitalSignature = 0,
    /// `nonRepudiation (1)` (renamed `contentCommitment` in later editions): verifying
    /// signatures that provide a non-repudiation service.
    NonRepudiation = 1,
    /// `keyEncipherment (2)`: enciphering private or secret keys (key transport).
    KeyEncipherment = 2,
    /// `dataEncipherment (3)`: directly enciphering raw user data.
    DataEncipherment = 3,
    /// `keyAgreement (4)`: use in a key-agreement protocol.
    KeyAgreement = 4,
    /// `keyCertSign (5)`: verifying signatures on other certificates. An issuing
    /// certificate that carries `keyUsage` must assert this bit (RFC 5280 §4.2.1.3).
    KeyCertSign = 5,
    /// `cRLSign (6)`: verifying signatures on certificate revocation lists.
    CrlSign = 6,
    /// `encipherOnly (7)`: with `keyAgreement`, restricts the key to enciphering data
    /// during key agreement.
    EncipherOnly = 7,
    /// `decipherOnly (8)`: with `keyAgreement`, restricts the key to deciphering data
    /// during key agreement.
    DecipherOnly = 8,
}

/// The decoded `keyUsage` extension of one certificate (RFC 5280 §4.2.1.3): the set of
/// named bits its `BIT STRING` asserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyUsage {
    /// The asserted named bits as a bit-set indexed by [`KeyUsageBit`] position — bit `n`
    /// of this field is set when named bit `n` of the `keyUsage` `BIT STRING` is set. Zero
    /// when the extension is absent.
    bits: u16,
    /// Whether the `keyUsage` extension was present at all. `false` means the certificate
    /// carried no such extension, which RFC 5280 §4.2.1.3 treats as an unrestricted key;
    /// this field lets a caller distinguish "restricted, but `keyCertSign` absent" from
    /// "silent about usage".
    pub present: bool,
}

impl KeyUsage {
    /// The value for a certificate with no `keyUsage` extension: no bits asserted and
    /// [`present`](KeyUsage::present) `false` (RFC 5280 §4.2.1.3 places no restriction on a
    /// key whose certificate omits the extension).
    const ABSENT: Self = Self { bits: 0, present: false };

    /// Whether the given named bit is asserted (RFC 5280 §4.2.1.3). Always `false` when the
    /// extension is absent.
    pub fn allows(&self, bit: KeyUsageBit) -> bool {
        self.bits & (1 << bit as u16) != 0
    }

    /// Whether `keyCertSign` (bit 5) is asserted — the permission an issuing certificate's
    /// key needs to sign the certificate below it (RFC 5280 §4.2.1.3). A convenience for
    /// [`allows`](KeyUsage::allows)`(`[`KeyUsageBit::KeyCertSign`]`)`.
    pub fn key_cert_sign(&self) -> bool {
        self.allows(KeyUsageBit::KeyCertSign)
    }
}

/// Why extracting a certificate's `keyUsage` extension failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUsageError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag where
    /// the `Certificate`/`TBSCertificate`/extension structure required a specific one.
    /// Carries a static hint naming the field that did not decode.
    Malformed(&'static str),
}

impl core::fmt::Display for KeyUsageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
        }
    }
}

impl std::error::Error for KeyUsageError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples left
/// to right. Definite-length only (DER forbids the indefinite form). A sibling of the
/// readers in [`x509_spki`](super::x509_spki), [`x509_hostname`](super::x509_hostname),
/// [`x509_validity`](super::x509_validity), [`x509_chain`](super::x509_chain),
/// [`x509_name_chain`](super::x509_name_chain), and
/// [`x509_basic_constraints`](super::x509_basic_constraints), specialised to this slice's
/// error type.
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
    /// octets that follow. The indefinite form (`0x80`) and counts wider than four octets
    /// are rejected.
    fn read_length(&mut self) -> Result<usize, KeyUsageError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(KeyUsageError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(KeyUsageError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(KeyUsageError::Malformed("truncated long-form length"));
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
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), KeyUsageError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(KeyUsageError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(KeyUsageError::Malformed(
                "truncated: content shorter than its length",
            ));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names the
    /// field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], KeyUsageError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(KeyUsageError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Extract a certificate's `keyUsage` extension (RFC 5280 §4.2.1.3).
///
/// `cert_der` is one X.509 certificate — a `CertificateEntry.cert_data` from the server's
/// `Certificate` message (RFC 8446 §4.4.2). The result reports which named usage bits the
/// certificate asserts. A certificate with no `extensions` field, or an `extensions` field
/// without a `keyUsage` entry, yields [`KeyUsage::ABSENT`] (an unrestricted key, RFC 5280
/// §4.2.1.3).
///
/// # Errors
///
/// [`KeyUsageError::Malformed`] if the certificate DER is truncated or does not decode to a
/// `TBSCertificate`, or if the `keyUsage` extension value is present but does not decode to
/// a `BIT STRING`.
pub fn certificate_key_usage(cert_der: &[u8]) -> Result<KeyUsage, KeyUsageError> {
    let extensions = match tbs_extensions(cert_der)? {
        Some(extensions) => extensions,
        // No `extensions` field at all: pre-v3 or a v3 certificate that omitted the
        // optional field. RFC 5280 §4.2.1.3: no keyUsage means the key is unrestricted.
        None => return Ok(KeyUsage::ABSENT),
    };

    // Extensions ::= SEQUENCE OF Extension. Scan for the keyUsage entry.
    let mut extensions = Der::new(extensions);
    while !extensions.is_empty() {
        let extension = extensions.read_tagged(TAG_SEQUENCE, "extension is not a SEQUENCE")?;
        let mut extension = Der::new(extension);
        let oid = extension.read_tagged(TAG_OID, "extension has no OID")?;
        if oid != OID_KEY_USAGE {
            continue;
        }
        // Extension ::= SEQUENCE { extnID, critical BOOLEAN DEFAULT FALSE, extnValue }.
        // Skip the optional `critical` BOOLEAN if present; extnValue is the OCTET STRING.
        if extension.peek_tag() == Some(TAG_BOOLEAN) {
            extension.read_tlv()?;
        }
        let extn_value = extension.read_tagged(TAG_OCTET_STRING, "extnValue is not an OCTET STRING")?;
        return parse_key_usage(extn_value);
    }

    // extensions present, but no keyUsage among them: unrestricted key.
    Ok(KeyUsage::ABSENT)
}

/// Navigate a certificate's `TBSCertificate` to its `extensions` field (RFC 5280 §4.1.2.9),
/// returning the contents of the `[3] EXPLICIT` wrapper's inner `SEQUENCE OF Extension`, or
/// `None` if the certificate carries no `extensions` field.
fn tbs_extensions(cert_der: &[u8]) -> Result<Option<&[u8]>, KeyUsageError> {
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

/// Decode a `keyUsage` extension value (RFC 5280 §4.2.1.3): the DER of a `BIT STRING` whose
/// named bits are the asserted usages.
///
/// A DER `BIT STRING`'s first content octet is the count of unused (padding) bits in the
/// final octet (`0..=7`); the value bits follow, most-significant bit first, so named bit
/// `n` is bit `0x80 >> (n % 8)` of value octet `n / 8`.
fn parse_key_usage(extn_value: &[u8]) -> Result<KeyUsage, KeyUsageError> {
    let bit_string = Der::new(extn_value)
        .read_tagged(TAG_BIT_STRING, "keyUsage is not a BIT STRING")?;
    let (&unused, value) = bit_string
        .split_first()
        .ok_or(KeyUsageError::Malformed("empty keyUsage BIT STRING"))?;
    if unused > 7 {
        return Err(KeyUsageError::Malformed("keyUsage BIT STRING unused-bit count exceeds 7"));
    }

    // Fold the named bits into the bit-set. Only the nine defined positions (0..=8) are
    // interpreted; any bit beyond `decipherOnly` is ignored, as is a padding bit an
    // over-long encoding might carry.
    let mut bits = 0u16;
    for named in 0u16..=8 {
        let octet = named as usize / 8;
        let mask = 0x80u8 >> (named % 8);
        if value.get(octet).is_some_and(|byte| byte & mask != 0) {
            bits |= 1 << named;
        }
    }

    Ok(KeyUsage { bits, present: true })
}

/// Why enforcing `keyUsage` across a certificate chain failed (RFC 5280 §4.2.1.3, §6.1.4,
/// RFC 8446 §4.4.2). Each variant pinpoints the certificate — by its position in the
/// server's `certificate_list` — at which enforcement broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUsageWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity certificate
    /// first, so there is nothing to walk — a malformed `Certificate` message.
    EmptyChain,
    /// A certificate's `keyUsage` could not be extracted: its DER failed to decode to a
    /// `TBSCertificate`, or its `keyUsage` value was malformed.
    Certificate {
        /// Position, in the `certificate_list`, of the certificate whose `keyUsage` failed
        /// to extract.
        index: usize,
        /// The underlying extraction failure.
        error: KeyUsageError,
    },
    /// The certificate at `index` issues the certificate below it and carries a `keyUsage`
    /// extension, but that extension does not assert `keyCertSign` (RFC 5280 §4.2.1.3): its
    /// key is not permitted to sign certificates.
    NotCertSign {
        /// Position, in the `certificate_list`, of the issuing certificate whose `keyUsage`
        /// omits `keyCertSign`.
        index: usize,
    },
}

impl core::fmt::Display for KeyUsageWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::Certificate { index, error } => write!(f, "certificate #{index}: {error}"),
            Self::NotCertSign { index } => write!(
                f,
                "certificate #{index} issues another but its keyUsage does not assert keyCertSign"
            ),
        }
    }
}

impl std::error::Error for KeyUsageWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Certificate { error, .. } => Some(error),
            Self::EmptyChain | Self::NotCertSign { .. } => None,
        }
    }
}

/// Verify that every issuing certificate in a server-presented chain is permitted by its
/// `keyUsage` to sign certificates (RFC 5280 §4.2.1.3, §6.1.4, RFC 8446 §4.4.2).
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message — the
/// end-entity certificate first, then each issuing intermediate. Every certificate but the
/// leaf issues the one below it, so each of `chain[1..]` that carries a `keyUsage` extension
/// must assert `keyCertSign`; an issuing certificate with **no** `keyUsage` extension is not
/// restricted by this walk (RFC 5280 §4.2.1.3 limits a key only when the extension is
/// present). The leaf (`chain[0]`) is the subject of the connection, not an issuer, and its
/// `keyUsage` is not consulted here.
///
/// A single-element chain has no issuing certificate and verifies vacuously; an empty chain
/// is [`KeyUsageWalkError::EmptyChain`].
///
/// This is the `keyCertSign` complement of the `basicConstraints` walk
/// ([`verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints)): a
/// chain should pass **both**, together with the signature walk
/// ([`verify_chain_signatures`](super::x509_chain::verify_chain_signatures)) and the name
/// walk ([`verify_name_chain`](super::x509_name_chain::verify_name_chain)). This alone does
/// **not** verify signatures or names, enforce `cA = TRUE` or the path length, or terminate
/// at a trust anchor (RFC 5280 §6.1) — those are sibling and later slices.
///
/// # Errors
///
/// - [`KeyUsageWalkError::EmptyChain`] if `chain` is empty.
/// - [`KeyUsageWalkError::Certificate`] if a certificate's `keyUsage` cannot be extracted,
///   naming that certificate's position.
/// - [`KeyUsageWalkError::NotCertSign`] if an issuing certificate carries a `keyUsage`
///   extension that omits `keyCertSign`, naming its position.
pub fn verify_cert_sign_usage(chain: &[&[u8]]) -> Result<(), KeyUsageWalkError> {
    if chain.is_empty() {
        return Err(KeyUsageWalkError::EmptyChain);
    }

    // Every certificate above the leaf issues the one below it. If it carries a keyUsage
    // extension it must permit keyCertSign; an absent extension imposes no restriction.
    for (index, cert_der) in chain.iter().enumerate().skip(1) {
        let usage = certificate_key_usage(cert_der)
            .map_err(|error| KeyUsageWalkError::Certificate { index, error })?;
        if usage.present && !usage.key_cert_sign() {
            return Err(KeyUsageWalkError::NotCertSign { index });
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

    /// A DER `BIT STRING` value from a set of named bits: the minimal value octets with the
    /// bits placed most-significant-first, prefixed by the unused-bit count of the final
    /// octet.
    fn bit_string(bits: &[KeyUsageBit]) -> Vec<u8> {
        // Number of value octets is governed by the highest set bit (1..=2 for keyUsage).
        let highest = bits.iter().map(|b| *b as usize).max();
        let octet_count = match highest {
            None => 0,
            Some(n) => n / 8 + 1,
        };
        let mut value = vec![0u8; octet_count];
        for &b in bits {
            let n = b as usize;
            value[n / 8] |= 0x80 >> (n % 8);
        }
        // Unused bits = padding in the final octet. For a set of named bits ending at the
        // highest bit, the trailing zeros of the last octet are unused; report them so the
        // encoding is DER-canonical, though the parser tolerates any 0..=7.
        let unused = match highest {
            None => 0,
            Some(n) => (7 - (n % 8)) as u8,
        };
        let mut out = vec![unused];
        out.extend_from_slice(&value);
        out
    }

    /// A `keyUsage` extension: `SEQUENCE { OID 2.5.29.15, [critical] BOOLEAN?, OCTET STRING
    /// { BIT STRING } }`.
    fn key_usage_ext(critical: bool, bits: &[KeyUsageBit]) -> Vec<u8> {
        let value = tlv(TAG_BIT_STRING, &bit_string(bits));
        let octet_string = tlv(TAG_OCTET_STRING, &value);

        let mut parts: Vec<Vec<u8>> = vec![tlv(TAG_OID, OID_KEY_USAGE)];
        if critical {
            parts.push(tlv(TAG_BOOLEAN, &[0xFF]));
        }
        parts.push(octet_string);
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        tlv(TAG_SEQUENCE, &cat(&refs))
    }

    /// A non-keyUsage extension (a placeholder `basicConstraints`-shaped entry) the scanner
    /// must skip past.
    fn other_ext() -> Vec<u8> {
        let oid = tlv(TAG_OID, &[0x55, 0x1D, 0x13]); // id-ce-basicConstraints (2.5.29.19)
        let value = tlv(TAG_OCTET_STRING, &tlv(TAG_SEQUENCE, &tlv(TAG_BOOLEAN, &[0xFF])));
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
        let signature = tlv(TAG_BIT_STRING, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    /// A CA certificate: `keyUsage` present asserting `keyCertSign` (and `cRLSign`, as a
    /// real CA carries).
    fn ca_cert() -> Vec<u8> {
        cert(Some(&[key_usage_ext(
            true,
            &[KeyUsageBit::KeyCertSign, KeyUsageBit::CrlSign],
        )]))
    }

    /// A leaf certificate: `keyUsage` present asserting `digitalSignature` +
    /// `keyEncipherment` (a typical TLS server leaf), but not `keyCertSign`.
    fn leaf_cert() -> Vec<u8> {
        cert(Some(&[key_usage_ext(
            true,
            &[KeyUsageBit::DigitalSignature, KeyUsageBit::KeyEncipherment],
        )]))
    }

    // ── certificate_key_usage: extraction ──────────────────────────────────

    #[test]
    fn extracts_a_ca_asserting_cert_sign() {
        let ku = certificate_key_usage(&ca_cert()).expect("decodes");
        assert!(ku.present);
        assert!(ku.key_cert_sign());
        assert!(ku.allows(KeyUsageBit::CrlSign));
        assert!(!ku.allows(KeyUsageBit::DigitalSignature));
    }

    #[test]
    fn extracts_a_leaf_without_cert_sign() {
        let ku = certificate_key_usage(&leaf_cert()).expect("decodes");
        assert!(ku.present);
        assert!(!ku.key_cert_sign());
        assert!(ku.allows(KeyUsageBit::DigitalSignature));
        assert!(ku.allows(KeyUsageBit::KeyEncipherment));
    }

    #[test]
    fn treats_an_absent_extension_as_unrestricted() {
        // A v3 certificate with an `extensions` field that has no keyUsage entry.
        let ku = certificate_key_usage(&cert(Some(&[other_ext()]))).expect("decodes");
        assert_eq!(ku, KeyUsage::ABSENT);
        assert!(!ku.present);
        assert!(!ku.key_cert_sign());
    }

    #[test]
    fn treats_no_extensions_field_as_unrestricted() {
        // A certificate with no `extensions` field at all.
        let ku = certificate_key_usage(&cert(None)).expect("decodes");
        assert_eq!(ku, KeyUsage::ABSENT);
    }

    #[test]
    fn finds_key_usage_after_another_extension() {
        // The scanner must skip a leading non-keyUsage extension.
        let ku = certificate_key_usage(&cert(Some(&[
            other_ext(),
            key_usage_ext(true, &[KeyUsageBit::KeyCertSign]),
        ])))
        .expect("decodes");
        assert!(ku.key_cert_sign());
    }

    #[test]
    fn parses_a_non_critical_key_usage() {
        // keyUsage without the critical BOOLEAN must still decode.
        let ku = certificate_key_usage(&cert(Some(&[key_usage_ext(
            false,
            &[KeyUsageBit::KeyCertSign],
        )])))
        .expect("decodes");
        assert!(ku.key_cert_sign());
    }

    #[test]
    fn parses_the_decipher_only_bit_in_the_second_octet() {
        // decipherOnly (bit 8) needs a second value octet, exercising the octet indexing.
        let ku = certificate_key_usage(&cert(Some(&[key_usage_ext(
            true,
            &[KeyUsageBit::KeyAgreement, KeyUsageBit::DecipherOnly],
        )])))
        .expect("decodes");
        assert!(ku.allows(KeyUsageBit::KeyAgreement));
        assert!(ku.allows(KeyUsageBit::DecipherOnly));
        assert!(!ku.allows(KeyUsageBit::EncipherOnly));
        assert!(!ku.key_cert_sign());
    }

    // ── certificate_key_usage: malformed ───────────────────────────────────

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            certificate_key_usage(&not_a_cert),
            Err(KeyUsageError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_certificate() {
        let c = ca_cert();
        assert!(matches!(
            certificate_key_usage(&c[..c.len() - 4]),
            Err(KeyUsageError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_a_non_bit_string_value() {
        // extnValue that is an INTEGER instead of a BIT STRING.
        let value = tlv(TAG_INTEGER, &[0x05]);
        let octet_string = tlv(TAG_OCTET_STRING, &value);
        let ext = tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_OID, OID_KEY_USAGE), &octet_string]));
        assert!(matches!(
            certificate_key_usage(&cert(Some(&[ext]))),
            Err(KeyUsageError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_an_empty_bit_string() {
        // A BIT STRING with not even the unused-bit-count octet.
        let octet_string = tlv(TAG_OCTET_STRING, &tlv(TAG_BIT_STRING, &[]));
        let ext = tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_OID, OID_KEY_USAGE), &octet_string]));
        assert!(matches!(
            certificate_key_usage(&cert(Some(&[ext]))),
            Err(KeyUsageError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_an_over_large_unused_bit_count() {
        // A BIT STRING claiming 8 unused bits is malformed (max is 7).
        let octet_string = tlv(TAG_OCTET_STRING, &tlv(TAG_BIT_STRING, &[0x08, 0x00]));
        let ext = tlv(TAG_SEQUENCE, &cat(&[&tlv(TAG_OID, OID_KEY_USAGE), &octet_string]));
        assert!(matches!(
            certificate_key_usage(&cert(Some(&[ext]))),
            Err(KeyUsageError::Malformed(_)),
        ));
    }

    // ── verify_cert_sign_usage: happy paths ────────────────────────────────

    #[test]
    fn accepts_a_leaf_and_cert_sign_issuer() {
        verify_cert_sign_usage(&[&leaf_cert(), &ca_cert()])
            .expect("the issuer's keyUsage permits keyCertSign");
    }

    #[test]
    fn accepts_an_issuer_with_no_key_usage() {
        // An issuer that omits keyUsage is unrestricted by this walk (basicConstraints
        // already required cA = TRUE for it).
        verify_cert_sign_usage(&[&leaf_cert(), &cert(None)])
            .expect("an absent keyUsage imposes no restriction");
    }

    #[test]
    fn accepts_a_three_certificate_chain_of_cert_sign_issuers() {
        verify_cert_sign_usage(&[&leaf_cert(), &ca_cert(), &ca_cert()])
            .expect("both issuing CAs permit keyCertSign");
    }

    #[test]
    fn accepts_a_single_certificate_vacuously() {
        // A lone certificate issues nothing, so there is no issuing key to constrain.
        verify_cert_sign_usage(&[&leaf_cert()]).expect("a lone certificate has no issuers");
    }

    // ── verify_cert_sign_usage: rejections ─────────────────────────────────

    #[test]
    fn rejects_an_issuer_without_cert_sign() {
        // The issuer is a leaf keyUsage (digitalSignature only) masquerading as an
        // intermediate: it carries keyUsage but not keyCertSign.
        assert_eq!(
            verify_cert_sign_usage(&[&leaf_cert(), &leaf_cert()]),
            Err(KeyUsageWalkError::NotCertSign { index: 1 }),
        );
    }

    #[test]
    fn reports_the_deeper_non_cert_sign_when_both_issuers_break() {
        // Both issuers carry keyUsage without keyCertSign; the walk reports the first
        // (lowest index) it meets.
        assert_eq!(
            verify_cert_sign_usage(&[&leaf_cert(), &leaf_cert(), &leaf_cert()]),
            Err(KeyUsageWalkError::NotCertSign { index: 1 }),
        );
    }

    #[test]
    fn rejects_an_issuer_certificate_that_does_not_decode() {
        let garbage = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            verify_cert_sign_usage(&[&leaf_cert(), &garbage]),
            Err(KeyUsageWalkError::Certificate { index: 1, error: KeyUsageError::Malformed(_) }),
        ));
    }

    #[test]
    fn rejects_an_empty_chain() {
        assert_eq!(
            verify_cert_sign_usage(&[]),
            Err(KeyUsageWalkError::EmptyChain),
        );
    }

    #[test]
    fn does_not_consult_the_leaf_key_usage() {
        // The leaf carries keyUsage without keyCertSign, but it issues nothing, so the walk
        // over a single-issuer chain ignores it and passes on the CA issuer.
        verify_cert_sign_usage(&[&leaf_cert(), &ca_cert()])
            .expect("the leaf's own keyUsage is not consulted");
    }
}
