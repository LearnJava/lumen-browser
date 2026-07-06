//! X.509 name-chaining verification (RFC 5280 §4.1.2.4, §4.1.2.6, §6.1) — slice 69 of
//! the HTTP/3 sprint.
//!
//! The chain-signature walk ([`x509_chain::verify_chain_signatures`](super::x509_chain::verify_chain_signatures),
//! slices 66–68) proves that every certificate in the server's `certificate_list` is
//! *signed* by the one above it. But a signature link is only half of what binds a
//! chain: RFC 5280 §6.1 requires that each certificate's `issuer` distinguished name
//! *also* equals the `subject` distinguished name of the certificate that issued it.
//! The signature says "the key one step up produced this certificate"; the name
//! chaining says "the certificate one step up *claims to be* the issuer this
//! certificate names". A chain whose signatures all verify but whose names do not line
//! up is malformed — a spliced-together set of certificates rather than an ordered
//! path. The signature walk deliberately left this to a later slice ("match
//! `issuer`/`subject` distinguished names (name chaining)"); this module is that slice.
//!
//! ## What it reads
//!
//! The `issuer` and `subject` fields of the `TBSCertificate` (RFC 5280 §4.1.2.4,
//! §4.1.2.6):
//!
//! ```text
//! Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
//! TBSCertificate ::= SEQUENCE {
//!     version      [0] EXPLICIT ... DEFAULT v1,   -- context 0xA0, optional
//!     serialNumber     INTEGER,
//!     signature        AlgorithmIdentifier,
//!     issuer           Name,                       -- <- who issued this certificate
//!     validity         Validity,
//!     subject          Name,                       -- <- who this certificate is for
//!     ... }
//!
//! Name ::= RDNSequence  -- a SEQUENCE OF RelativeDistinguishedName
//! ```
//!
//! [`certificate_names`] navigates the `TBSCertificate` by field order and returns the
//! *raw* DER of the `issuer` and `subject` Names — the exact `tag ‖ length ‖ contents`
//! spans as they appear in the certificate. [`verify_name_chain`] walks the whole
//! `certificate_list` and checks that each certificate's `issuer` matches the next
//! certificate's `subject`.
//!
//! ## How names are compared
//!
//! RFC 5280 §7.1 defines a normalised name-comparison procedure (LDAP StringPrep case
//! folding and whitespace collapsing over each attribute value). This slice takes the
//! conservative subset that real path-building libraries (rustls `webpki`,
//! Chromium's verifier fast path) apply first: **byte-for-byte equality of the DER
//! encoding** of the two Names. Certificate authorities issuing a subordinate copy the
//! parent's `subject` Name into the child's `issuer` verbatim, so the encodings match
//! octet-for-octet in practice, and a byte comparison is *stricter* than §7.1 — it can
//! only reject a chain the normalised comparison would also scrutinise, never accept a
//! mismatch §7.1 would reject. **Deferred:** the full §7.1 normalised comparison, which
//! would additionally accept two Names that are logically equal but encoded differently
//! (a `PrintableString` versus `UTF8String` attribute value, or differing letter case
//! or interior whitespace). Until that lands, such a pair is reported as a
//! [`NameChainWalkError::NameMismatch`].
//!
//! ## What it does *not* do
//!
//! This confirms only that the presented certificates form a self-consistent *name*
//! chain — the complement of the signature chain ([`x509_chain`](super::x509_chain)).
//! It does **not** verify the signatures (that is [`x509_chain`](super::x509_chain)),
//! terminate the chain at a trusted root (RFC 5280 §6.1, a later slice), honour
//! `basicConstraints`/`keyUsage` (§4.2.1.9/§4.2.1.3), or apply name constraints
//! (§4.2.1.10). It is one leg of path validation, layered beside the signature walk
//! exactly as possession, identity, and validity are layered beside one another.
//!
//! ## Purity
//!
//! Pure DER parsing over borrowed certificate bytes: no clock, no I/O, no allocation
//! beyond the caller's slices. A sibling of [`x509_spki`](super::x509_spki),
//! [`x509_hostname`](super::x509_hostname), [`x509_validity`](super::x509_validity),
//! and [`x509_chain`](super::x509_chain).

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for the optional `[0] EXPLICIT` `version` field of a `TBSCertificate`.
const TAG_CONTEXT_0: u8 = 0xA0;

/// Why extracting a certificate's `issuer` and `subject` Names failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameChainError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag
    /// where the `Certificate`/`TBSCertificate` structure required a specific one.
    /// Carries a static hint naming the field that did not decode.
    Malformed(&'static str),
}

impl core::fmt::Display for NameChainError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
        }
    }
}

impl std::error::Error for NameChainError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples
/// left to right. Definite-length only (DER forbids the indefinite form). A sibling of
/// the readers in [`x509_spki`](super::x509_spki), [`x509_hostname`](super::x509_hostname),
/// [`x509_validity`](super::x509_validity), and [`x509_chain`](super::x509_chain),
/// specialised to this slice's error type and adding a raw-span read for the `issuer`
/// and `subject` Name bytes the comparison needs verbatim.
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

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Read a DER definite length at the cursor (X.690): a short form (`0x00..=0x7f`)
    /// is the length itself; a long form (`0x81..`) gives the count of big-endian
    /// length octets that follow. The indefinite form (`0x80`) and counts wider than
    /// four octets are rejected.
    fn read_length(&mut self) -> Result<usize, NameChainError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(NameChainError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(NameChainError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(NameChainError::Malformed("truncated long-form length"));
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
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), NameChainError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(NameChainError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(NameChainError::Malformed("truncated: content shorter than its length"));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and return its tag *and* the full `tag ‖ length ‖ contents` span
    /// exactly as it appears in the input. The raw span is what a name comparison needs:
    /// two Names match iff their DER encodings are byte-identical, which the contents
    /// alone cannot express without re-encoding the length prefix.
    fn read_tlv_raw(&mut self) -> Result<(u8, &'a [u8]), NameChainError> {
        let start = self.pos;
        let (tag, _contents) = self.read_tlv()?;
        Ok((tag, &self.bytes[start..self.pos]))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names
    /// the field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], NameChainError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(NameChainError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Extract the raw DER of a certificate's `issuer` and `subject` Names (RFC 5280
/// §4.1.2.4, §4.1.2.6), returning `(issuer, subject)`.
///
/// `cert_der` is one X.509 certificate — a `CertificateEntry.cert_data` from the
/// server's `Certificate` message (RFC 8446 §4.4.2). The returned slices borrow from
/// `cert_der` and are the exact `tag ‖ length ‖ contents` spans of the two Name fields,
/// suitable for byte-for-byte comparison ([`verify_name_chain`]).
///
/// # Errors
///
/// [`NameChainError::Malformed`] if the certificate DER is truncated or does not
/// decode to a `TBSCertificate` with `issuer` and `subject` SEQUENCEs in the expected
/// positions.
pub fn certificate_names(cert_der: &[u8]) -> Result<(&[u8], &[u8]), NameChainError> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE, "certificate is not a SEQUENCE")?;
    let tbs = Der::new(certificate).read_tagged(TAG_SEQUENCE, "tbsCertificate is not a SEQUENCE")?;

    // TBSCertificate fields in order, up to subject.
    let mut tbs = Der::new(tbs);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version [0] EXPLICIT — optional (absent = v1)
    }
    tbs.read_tagged(TAG_INTEGER, "serialNumber is not an INTEGER")?;
    tbs.read_tagged(TAG_SEQUENCE, "signature AlgorithmIdentifier is not a SEQUENCE")?;

    let (issuer_tag, issuer) = tbs.read_tlv_raw()?;
    if issuer_tag != TAG_SEQUENCE {
        return Err(NameChainError::Malformed("issuer is not a SEQUENCE"));
    }
    tbs.read_tagged(TAG_SEQUENCE, "validity is not a SEQUENCE")?;
    let (subject_tag, subject) = tbs.read_tlv_raw()?;
    if subject_tag != TAG_SEQUENCE {
        return Err(NameChainError::Malformed("subject is not a SEQUENCE"));
    }

    Ok((issuer, subject))
}

/// Why walking a certificate chain's name links failed (RFC 5280 §4.1.2.4, §4.1.2.6,
/// §6.1, RFC 8446 §4.4.2). Each variant pinpoints the certificate — by its position in
/// the server's `certificate_list` — at which the walk broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameChainWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity
    /// certificate first, so there is nothing to walk — a malformed `Certificate`
    /// message.
    EmptyChain,
    /// A certificate's `issuer`/`subject` Names could not be extracted: its DER failed
    /// to decode to a `TBSCertificate` with the two Name fields.
    Certificate {
        /// Position, in the `certificate_list`, of the certificate whose Names failed
        /// to extract.
        index: usize,
        /// The underlying Name-extraction failure.
        error: NameChainError,
    },
    /// The certificate at `subject_index` names an `issuer` that does not match the
    /// `subject` of the next certificate in the list (its candidate issuer): the DER of
    /// the two Names is not byte-identical (RFC 5280 §6.1). A malformed chain — the
    /// certificates are not an ordered issuance path.
    NameMismatch {
        /// Position, in the `certificate_list`, of the certificate whose `issuer` Name
        /// did not match the next certificate's `subject` Name.
        subject_index: usize,
    },
}

impl core::fmt::Display for NameChainWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::Certificate { index, error } => {
                write!(f, "certificate #{index}: {error}")
            }
            Self::NameMismatch { subject_index } => write!(
                f,
                "certificate #{subject_index} issuer name does not match its issuer's subject name"
            ),
        }
    }
}

impl std::error::Error for NameChainWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Certificate { error, .. } => Some(error),
            Self::EmptyChain | Self::NameMismatch { .. } => None,
        }
    }
}

/// Verify that every certificate in a server-presented chain names the next one up as
/// its issuer (RFC 5280 §4.1.2.4, §4.1.2.6, §6.1, RFC 8446 §4.4.2).
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message —
/// the end-entity certificate first, then each issuing intermediate. For every adjacent
/// pair `(chain[i], chain[i + 1])` this checks that `chain[i]`'s `issuer` Name is
/// byte-for-byte equal to `chain[i + 1]`'s `subject` Name: the certificate one step up
/// is exactly the authority `chain[i]` claims issued it. The last certificate has no
/// successor in the list — its issuer is a trust anchor outside the chain — so its own
/// `issuer` Name is not matched here.
///
/// A single-element chain has no internal links and verifies vacuously; an empty chain
/// is [`NameChainWalkError::EmptyChain`].
///
/// This is the name complement of the signature walk
/// ([`verify_chain_signatures`](super::x509_chain::verify_chain_signatures)): a chain
/// should pass **both** — each link signed by the next *and* named after the next. This
/// alone does **not** verify signatures, terminate at a trust anchor (RFC 5280 §6.1),
/// or apply name constraints (§4.2.1.10) — those are sibling and later slices.
///
/// # Errors
///
/// - [`NameChainWalkError::EmptyChain`] if `chain` is empty.
/// - [`NameChainWalkError::Certificate`] if a certificate's Names cannot be extracted,
///   naming that certificate's position.
/// - [`NameChainWalkError::NameMismatch`] if a certificate's `issuer` Name does not
///   match the next certificate's `subject` Name, naming that certificate's position.
pub fn verify_name_chain(chain: &[&[u8]]) -> Result<(), NameChainWalkError> {
    if chain.is_empty() {
        return Err(NameChainWalkError::EmptyChain);
    }

    // Each certificate but the last (whose issuer is a trust anchor not in the list)
    // must name the certificate one step up as its issuer.
    for subject_index in 0..chain.len() - 1 {
        let issuer_index = subject_index + 1;
        let (subject_issuer_name, _) = certificate_names(chain[subject_index])
            .map_err(|error| NameChainWalkError::Certificate { index: subject_index, error })?;
        let (_, issuer_subject_name) = certificate_names(chain[issuer_index])
            .map_err(|error| NameChainWalkError::Certificate { index: issuer_index, error })?;
        if subject_issuer_name != issuer_subject_name {
            return Err(NameChainWalkError::NameMismatch { subject_index });
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

    /// A `Name` — an `RDNSequence` carrying a single `commonName`-shaped attribute whose
    /// value is `cn`. The exact attribute shape does not matter to the byte comparison;
    /// only that two Names built from the same `cn` are byte-identical and two from
    /// different `cn`s are not.
    fn name(cn: &str) -> Vec<u8> {
        // SET OF { SEQUENCE { OID commonName, PrintableString cn } } wrapped in the
        // RDNSequence SEQUENCE — a structurally plausible distinguished name.
        let oid_cn: &[u8] = &[0x55, 0x04, 0x03]; // id-at-commonName (2.5.4.3)
        let atv = tlv(TAG_SEQUENCE, &cat(&[&tlv(0x06, oid_cn), &tlv(0x13, cn.as_bytes())]));
        let rdn = tlv(0x31, &atv); // SET OF
        tlv(TAG_SEQUENCE, &rdn)
    }

    /// Assemble a minimal but structurally valid v3 certificate whose `issuer` and
    /// `subject` are the given Names. Every other field is a placeholder the walker
    /// skips.
    fn cert(issuer: &[u8], subject: &[u8]) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])); // [0] { v3 = 2 }
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let validity = tlv(TAG_SEQUENCE, &[]);
        let spki = tlv(TAG_SEQUENCE, &[]);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[&version, &serial, &sig_alg, issuer, &validity, subject, &spki]),
        );
        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    // ── certificate_names: field extraction ────────────────────────────────

    #[test]
    fn extracts_issuer_and_subject_names() {
        let issuer = name("Example Root CA");
        let subject = name("example.com");
        let c = cert(&issuer, &subject);
        let (got_issuer, got_subject) = certificate_names(&c).expect("names decode");
        assert_eq!(got_issuer, issuer.as_slice());
        assert_eq!(got_subject, subject.as_slice());
    }

    #[test]
    fn extracts_names_from_a_v1_certificate_without_the_version_field() {
        // A v1 tbsCertificate omits the [0] version prefix; the walker must still reach
        // issuer and subject.
        let issuer = name("Root");
        let subject = name("Leaf");
        let serial = tlv(TAG_INTEGER, &[0x0A]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let validity = tlv(TAG_SEQUENCE, &[]);
        let spki = tlv(TAG_SEQUENCE, &[]);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[&serial, &sig_alg, &issuer, &validity, &subject, &spki]),
        );
        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        let c = tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]));

        let (got_issuer, got_subject) = certificate_names(&c).expect("names decode");
        assert_eq!(got_issuer, issuer.as_slice());
        assert_eq!(got_subject, subject.as_slice());
    }

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            certificate_names(&not_a_cert),
            Err(NameChainError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_certificate() {
        let c = cert(&name("A"), &name("B"));
        assert!(matches!(
            certificate_names(&c[..c.len() - 4]),
            Err(NameChainError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_an_empty_input() {
        assert!(matches!(
            certificate_names(&[]),
            Err(NameChainError::Malformed(_)),
        ));
    }

    // ── verify_name_chain: happy paths ─────────────────────────────────────

    #[test]
    fn verifies_a_two_certificate_name_chain() {
        // leaf.issuer == intermediate.subject; the intermediate's own issuer (the root,
        // not in the chain) is not checked.
        let root = name("Root CA");
        let intermediate_subject = name("Intermediate CA");
        let leaf_subject = name("leaf.example.com");

        let intermediate = cert(&root, &intermediate_subject);
        let leaf = cert(&intermediate_subject, &leaf_subject);

        verify_name_chain(&[&leaf, &intermediate]).expect("each issuer names the next subject");
    }

    #[test]
    fn verifies_a_three_certificate_name_chain() {
        let root = name("Root CA");
        let top_subject = name("Top Intermediate");
        let mid_subject = name("Mid Intermediate");
        let leaf_subject = name("leaf.example.com");

        let top = cert(&root, &top_subject);
        let mid = cert(&top_subject, &mid_subject);
        let leaf = cert(&mid_subject, &leaf_subject);

        verify_name_chain(&[&leaf, &mid, &top]).expect("every name link lines up");
    }

    #[test]
    fn accepts_a_single_certificate_vacuously() {
        // One certificate has no internal links; binding it to a trust anchor is a
        // separate check, so the walk succeeds vacuously.
        let leaf = cert(&name("Some CA"), &name("leaf.example.com"));
        verify_name_chain(&[&leaf]).expect("a lone certificate has no name links to break");
    }

    // ── verify_name_chain: mismatches ──────────────────────────────────────

    #[test]
    fn rejects_a_leaf_whose_issuer_name_does_not_match() {
        // The leaf names a different issuer than the intermediate's subject.
        let root = name("Root CA");
        let intermediate_subject = name("Intermediate CA");
        let leaf_subject = name("leaf.example.com");

        let intermediate = cert(&root, &intermediate_subject);
        let leaf = cert(&name("Some Other CA"), &leaf_subject);

        assert_eq!(
            verify_name_chain(&[&leaf, &intermediate]),
            Err(NameChainWalkError::NameMismatch { subject_index: 0 }),
        );
    }

    #[test]
    fn reports_the_broken_name_link_in_the_middle() {
        // leaf←mid names line up, but mid.issuer does not match top.subject.
        let root = name("Root CA");
        let top_subject = name("Top Intermediate");
        let mid_subject = name("Mid Intermediate");
        let leaf_subject = name("leaf.example.com");

        let top = cert(&root, &top_subject);
        let mid = cert(&name("Impostor CA"), &mid_subject);
        let leaf = cert(&mid_subject, &leaf_subject);

        assert_eq!(
            verify_name_chain(&[&leaf, &mid, &top]),
            Err(NameChainWalkError::NameMismatch { subject_index: 1 }),
        );
    }

    #[test]
    fn distinguishes_names_that_differ_only_in_case() {
        // Byte comparison is stricter than RFC 5280 §7.1: a case-only difference in the
        // encoded value is a mismatch here (the deferred normalised comparison would
        // scrutinise it). This pins the documented conservative behaviour.
        let intermediate = cert(&name("Root CA"), &name("Example CA"));
        let leaf = cert(&name("example ca"), &name("leaf.example.com"));
        assert_eq!(
            verify_name_chain(&[&leaf, &intermediate]),
            Err(NameChainWalkError::NameMismatch { subject_index: 0 }),
        );
    }

    #[test]
    fn rejects_an_issuer_certificate_that_does_not_decode() {
        // The candidate issuer is not a certificate at all, so its subject Name cannot
        // be extracted — reported against the issuer's position.
        let leaf = cert(&name("Intermediate CA"), &name("leaf.example.com"));
        let garbage = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            verify_name_chain(&[&leaf, &garbage]),
            Err(NameChainWalkError::Certificate { index: 1, error: NameChainError::Malformed(_) }),
        ));
    }

    #[test]
    fn rejects_a_subject_certificate_that_does_not_decode() {
        // The subject-position certificate is garbage — reported against index 0.
        let intermediate = cert(&name("Root CA"), &name("Intermediate CA"));
        let garbage = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            verify_name_chain(&[&garbage, &intermediate]),
            Err(NameChainWalkError::Certificate { index: 0, error: NameChainError::Malformed(_) }),
        ));
    }

    #[test]
    fn rejects_an_empty_chain() {
        assert_eq!(verify_name_chain(&[]), Err(NameChainWalkError::EmptyChain));
    }
}
