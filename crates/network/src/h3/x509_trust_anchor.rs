//! X.509 trust-anchor termination (RFC 5280 §6.1, §4.1.1.3, RFC 8446 §4.4.2) — slice 75
//! of the HTTP/3 sprint.
//!
//! Four sibling walks over the server's presented `certificate_list` are already in
//! place: the signature walk ([`x509_chain::verify_chain_signatures`](super::x509_chain::verify_chain_signatures),
//! slices 66–68) proves each certificate is *signed* by the one above it, the name walk
//! ([`x509_name_chain::verify_name_chain`](super::x509_name_chain::verify_name_chain),
//! slices 69–70) proves each certificate *names* the one above it as its issuer, the
//! `basicConstraints` walk ([`x509_basic_constraints::verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints),
//! slices 71–72) proves each issuing certificate is a permitted CA, and the `keyUsage`
//! walk ([`x509_key_usage::verify_cert_sign_usage`](super::x509_key_usage::verify_cert_sign_usage),
//! slices 73–74) proves each issuing certificate's key is permitted to sign
//! certificates. Every one of those walks, by its own admission, "stops one link short
//! of the anchor": none of them checks that the *topmost* certificate in the list —
//! the one whose issuer is not itself in the `certificate_list` — is actually signed by
//! something the client trusts. A chain can satisfy all four walks and still be rooted
//! in a certificate the issuer minted for itself five minutes ago. This module is that
//! missing termination: it confirms the topmost certificate's issuer names a trust
//! anchor the caller supplies, and that the topmost certificate is really signed by
//! that anchor's key.
//!
//! ## What it reads
//!
//! Two fields of the topmost certificate — the last entry of the server's
//! `certificate_list` (RFC 8446 §4.4.2), the one every other slice's chain walk treats
//! as "signed by a trust anchor outside the list":
//!
//! - its `issuer` distinguished name (RFC 5280 §4.1.2.4), extracted by
//!   [`x509_name_chain::certificate_names`](super::x509_name_chain::certificate_names)
//!   (slice 69) — the same extractor the name walk already uses for every other link;
//! - its `signatureValue`, verified by
//!   [`x509_chain::verify_certificate_signature`](super::x509_chain::verify_certificate_signature)
//!   (slice 66) under a candidate issuer's public key — the same single-link primitive
//!   the signature walk already uses for every other link.
//!
//! Neither reads anything new; this module's only original piece is [`TrustAnchor`]
//! itself and the trust-store lookup that resolves the topmost certificate's `issuer`
//! to one.
//!
//! ## The trust store is the caller's
//!
//! [`TrustAnchor`] is deliberately *not* a compiled-in list. The existing HTTP/1.1 /
//! HTTP/2 TLS path already establishes the convention: `tls::build_client_config`
//! (`crates/network/src/tls/mod.rs`) takes a `rustls::RootCertStore` as a caller-supplied
//! parameter rather than hard-coding one, and `HttpClient` (`crates/network/src/lib.rs`)
//! is the caller that populates it from `webpki_roots::TLS_SERVER_ROOTS`. This module
//! follows the same shape: [`verify_trust_anchor`] takes `anchors: &[TrustAnchor<'_>]`,
//! and a later slice — the one that wires QUIC's `HttpClient` integration — supplies the
//! real Mozilla root list the same way the existing TLS path already does. Keeping the
//! trust store external also makes this module fully deterministic to test: a unit test
//! supplies its own tiny, synthetic anchor list instead of needing a real CA's private
//! key.
//!
//! ## How the chain is checked
//!
//! [`verify_trust_anchor`] looks at only the *topmost* certificate — `chain[chain.len() -
//! 1]`, the end of the presented list, exactly the certificate every sibling walk leaves
//! unchecked at its high end:
//!
//! 1. extract its `issuer` Name;
//! 2. [`find_trust_anchor`] the caller's `anchors` for one whose `subject` equals that
//!    `issuer`, byte-for-byte (RFC 5280 §6.1: a trust anchor is identified by its
//!    `subject`, the way a chain's internal links are identified by `issuer`/`subject`
//!    equality in the name walk);
//! 3. if found, extract that anchor's public key from its `subjectPublicKeyInfo`
//!    ([`x509_spki::parse_subject_public_key_info`](super::x509_spki::parse_subject_public_key_info),
//!    this slice's addition to `x509_spki`) and verify the topmost certificate's
//!    signature under it.
//!
//! No anchor matching the issuer, or a signature that does not verify under the
//! matched anchor's key, is a hard failure: RFC 5280 §6.1 requires a valid path to
//! terminate at an anchor the relying party trusts, and a chain that does not is not a
//! valid path no matter how internally self-consistent its presented links are.
//!
//! A single-certificate chain has the end-entity certificate as its own "topmost"
//! entry — it is checked against the trust store exactly like an intermediate would be,
//! since RFC 5280 §6.1 places no floor on how many certificates a valid path carries; an
//! end-entity certificate issued directly by a trust anchor is a legitimate (if
//! unusual) path.
//!
//! ## What it does *not* do
//!
//! Wiring this into the connect loop — so an untrusted root aborts the handshake the
//! way a bad `basicConstraints` or `keyUsage` already does — is a later slice, exactly
//! as slice 72 followed slice 71 and slice 74 followed slice 73. It also does not
//! re-verify anything the four sibling walks already cover (internal signatures, name
//! chaining, `basicConstraints`, `keyUsage`), does not consult certificate revocation,
//! and does not enforce a trust anchor's own `nameConstraints` (a field
//! [`TrustAnchor`] deliberately omits — the caller's trust-store adapter is where that
//! would be threaded through, in the same later slice that supplies the real store).
//!
//! ## Purity
//!
//! Pure DER parsing and signature math over borrowed bytes: no clock, no I/O, no
//! allocation beyond what the reused [`x509_chain`](super::x509_chain) and
//! [`x509_spki`](super::x509_spki) primitives already allocate. A sibling of every
//! other `x509_*` module in this sprint.

use super::x509_chain::{ChainError, verify_certificate_signature};
use super::x509_name_chain::{NameChainError, certificate_names};
use super::x509_spki::{SpkiError, parse_subject_public_key_info};

/// A trust anchor a certificate chain may terminate at (RFC 5280 §6.1): the anchor's
/// `subject` distinguished name and `subjectPublicKeyInfo`, the two fields path
/// validation needs to accept a self-signed root without holding a full certificate for
/// it. The caller's to supply — see the module-level "The trust store is the caller's"
/// section for why this is not a compiled-in list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustAnchor<'a> {
    /// The anchor's `subject` distinguished name (RFC 5280 §4.1.2.6): the exact
    /// `tag ‖ length ‖ contents` `Name` `SEQUENCE` span, comparable byte-for-byte
    /// against a certificate's `issuer` field — the same raw-span shape
    /// [`certificate_names`] returns.
    pub subject: &'a [u8],
    /// The anchor's `subjectPublicKeyInfo` (RFC 5280 §4.1.2.7) DER, including its own
    /// outer `SEQUENCE` tag and length — the exact shape
    /// [`parse_subject_public_key_info`] decodes.
    pub subject_public_key_info: &'a [u8],
}

/// Why terminating one certificate at a trust anchor failed.
#[derive(Debug)]
pub enum TrustAnchorError {
    /// The certificate's `issuer` Name could not be extracted.
    Names(NameChainError),
    /// No anchor in the caller-supplied trust store has a `subject` matching the
    /// certificate's `issuer`: the chain does not lead to a trusted root.
    UnknownIssuer,
    /// The matched anchor's `subjectPublicKeyInfo` did not decode.
    AnchorKey(SpkiError),
    /// The certificate's signature did not verify under the matched anchor's public
    /// key (RFC 5280 §4.1.1.3): the presented chain does not actually lead to the root
    /// its `issuer` Name claims.
    Signature(ChainError),
}

impl core::fmt::Display for TrustAnchorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Names(e) => write!(f, "{e}"),
            Self::UnknownIssuer => f.write_str("issuer does not match any trusted anchor"),
            Self::AnchorKey(e) => write!(f, "trust anchor public key: {e}"),
            Self::Signature(e) => write!(f, "signature under trust anchor key: {e}"),
        }
    }
}

impl std::error::Error for TrustAnchorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Names(e) => Some(e),
            Self::UnknownIssuer => None,
            Self::AnchorKey(e) => Some(e),
            Self::Signature(e) => Some(e),
        }
    }
}

/// Why [`verify_trust_anchor`] failed to terminate a presented chain.
#[derive(Debug)]
pub enum TrustAnchorWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity
    /// certificate first, so there is nothing to terminate — a malformed `Certificate`
    /// message.
    EmptyChain,
    /// The topmost certificate — `certificate_list[index]`, the last entry — does not
    /// terminate at a trusted anchor.
    Untrusted {
        /// Position, in the `certificate_list`, of the topmost certificate (always
        /// `certificate_list.len() - 1`).
        index: usize,
        /// The underlying termination failure.
        error: TrustAnchorError,
    },
}

impl core::fmt::Display for TrustAnchorWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::Untrusted { index, error } => {
                write!(f, "certificate #{index} does not terminate at a trust anchor: {error}")
            }
        }
    }
}

impl std::error::Error for TrustAnchorWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EmptyChain => None,
            Self::Untrusted { error, .. } => Some(error),
        }
    }
}

/// Find the trust anchor in `anchors` whose `subject` equals `issuer`, byte-for-byte.
///
/// `issuer` is the raw DER `Name` span of a certificate's `issuer` field (as
/// [`certificate_names`] returns it). `None` if no anchor's `subject` matches — the
/// certificate does not name a trust anchor the caller supplied.
#[must_use]
pub fn find_trust_anchor<'a>(issuer: &[u8], anchors: &'a [TrustAnchor<'a>]) -> Option<&'a TrustAnchor<'a>> {
    anchors.iter().find(|anchor| anchor.subject == issuer)
}

/// Verify that the topmost certificate of a server-presented chain terminates at a
/// trust anchor in `anchors` (RFC 5280 §6.1, §4.1.1.3, RFC 8446 §4.4.2).
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message —
/// the end-entity certificate first, then each issuing intermediate. The *topmost*
/// entry, `chain[chain.len() - 1]`, is the one every sibling walk in this sprint leaves
/// unchecked at its high end (its issuer is not itself in the list); this confirms that
/// entry's `issuer` names a caller-supplied [`TrustAnchor`] and that the certificate is
/// really signed by that anchor's key.
///
/// A single-element chain has its end-entity certificate as the topmost entry and is
/// checked against the trust store exactly like an intermediate would be (RFC 5280 §6.1
/// places no floor on path length). An empty chain is
/// [`TrustAnchorWalkError::EmptyChain`].
///
/// This is the termination complement of the signature walk
/// ([`verify_chain_signatures`](super::x509_chain::verify_chain_signatures)), the name
/// walk ([`verify_name_chain`](super::x509_name_chain::verify_name_chain)), the
/// `basicConstraints` walk
/// ([`verify_ca_constraints`](super::x509_basic_constraints::verify_ca_constraints)),
/// and the `keyUsage` walk
/// ([`verify_cert_sign_usage`](super::x509_key_usage::verify_cert_sign_usage)): a chain
/// should pass all five before it is trusted for an origin.
///
/// # Errors
///
/// - [`TrustAnchorWalkError::EmptyChain`] if `chain` is empty.
/// - [`TrustAnchorWalkError::Untrusted`] if the topmost certificate's `issuer` cannot be
///   extracted, no anchor's `subject` matches it, the matched anchor's public key
///   cannot be extracted, or the topmost certificate's signature does not verify under
///   it.
pub fn verify_trust_anchor(
    chain: &[&[u8]],
    anchors: &[TrustAnchor<'_>],
) -> Result<(), TrustAnchorWalkError> {
    let Some(index) = chain.len().checked_sub(1) else {
        return Err(TrustAnchorWalkError::EmptyChain);
    };
    verify_terminates_at_anchor(chain[index], anchors)
        .map_err(|error| TrustAnchorWalkError::Untrusted { index, error })
}

/// Verify that `certificate` is really signed by a trust anchor in `anchors` whose
/// `subject` matches `certificate`'s `issuer`. The single-certificate check
/// [`verify_trust_anchor`] applies to the topmost entry of a chain.
fn verify_terminates_at_anchor(
    certificate: &[u8],
    anchors: &[TrustAnchor<'_>],
) -> Result<(), TrustAnchorError> {
    let (issuer, _subject) = certificate_names(certificate).map_err(TrustAnchorError::Names)?;
    let anchor = find_trust_anchor(issuer, anchors).ok_or(TrustAnchorError::UnknownIssuer)?;
    let anchor_key = parse_subject_public_key_info(anchor.subject_public_key_info)
        .map_err(TrustAnchorError::AnchorKey)?;
    verify_certificate_signature(certificate, &anchor_key).map_err(TrustAnchorError::Signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

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

    const TAG_SEQUENCE: u8 = 0x30;
    const TAG_INTEGER: u8 = 0x02;
    const TAG_BIT_STRING: u8 = 0x03;
    const TAG_OID: u8 = 0x06;
    const TAG_CONTEXT_0: u8 = 0xA0;
    /// `id-Ed25519` (1.3.101.112, RFC 8410 §3).
    const OID_ED25519: &[u8] = &[0x2B, 0x65, 0x70];

    /// A DER `Name` (RDNSequence) carrying a single `commonName` valued `cn` — the same
    /// shape [`x509_name_chain`](super::super::x509_name_chain)'s own fixtures build,
    /// comparable byte-for-byte.
    fn name_rdn(cn: &str) -> Vec<u8> {
        let oid_cn = tlv(TAG_OID, &[0x55, 0x04, 0x03]); // id-at-commonName 2.5.4.3
        let value = tlv(0x13, cn.as_bytes()); // PrintableString
        let atv = tlv(TAG_SEQUENCE, &cat(&[&oid_cn, &value]));
        let rdn = tlv(0x31, &atv); // RelativeDistinguishedName SET OF
        tlv(TAG_SEQUENCE, &rdn) // RDNSequence SEQUENCE OF
    }

    /// An Ed25519 `SubjectPublicKeyInfo` DER carrying `public`, including its own outer
    /// `SEQUENCE` — the exact shape [`TrustAnchor::subject_public_key_info`] and
    /// [`parse_subject_public_key_info`] expect.
    fn ed25519_spki(public: &[u8; 32]) -> Vec<u8> {
        let alg_id = tlv(TAG_SEQUENCE, &tlv(TAG_OID, OID_ED25519));
        let mut bits = vec![0x00];
        bits.extend_from_slice(public);
        let bit_string = tlv(TAG_BIT_STRING, &bits);
        tlv(TAG_SEQUENCE, &cat(&[&alg_id, &bit_string]))
    }

    /// An Ed25519-signed v3 certificate: `issuer`/`subject` Names and `subjectPublicKeyInfo`
    /// exactly as given, signed by `signer` over its own `tbsCertificate` (RFC 5280
    /// §4.1.1.3). Every field before `subjectPublicKeyInfo` other than `issuer`/`subject`
    /// is a structurally valid placeholder the modules under test do not interpret.
    fn ed25519_certificate(issuer: &[u8], subject: &[u8], subject_key: &[u8; 32], signer: &SigningKey) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])); // [0] { v3 = 2 }
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, OID_ED25519));
        let validity = tlv(TAG_SEQUENCE, &[]);
        let spki = ed25519_spki(subject_key);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[&version, &serial, &sig_alg, issuer, &validity, subject, &spki]),
        );
        let signature = signer.sign(&tbs).to_bytes().to_vec();
        let mut sig_bits = vec![0x00];
        sig_bits.extend_from_slice(&signature);
        let sig_value = tlv(TAG_BIT_STRING, &sig_bits);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &sig_alg, &sig_value]))
    }

    /// A fixed root Ed25519 signing key: the trust anchor's key, never appearing in any
    /// presented `certificate_list`.
    fn root_signing() -> SigningKey {
        SigningKey::from_bytes(&[0x71; 32])
    }

    /// A different Ed25519 key, standing in for an impostor that is *not* the trust
    /// anchor's key.
    fn impostor_signing() -> SigningKey {
        SigningKey::from_bytes(&[0x72; 32])
    }

    /// The root's `subject` Name (also the topmost certificate's `issuer`, when it is
    /// really rooted).
    fn root_name() -> Vec<u8> {
        name_rdn("Lumen Test Root")
    }

    /// The [`TrustAnchor`] for [`root_signing`], built from its `subject` and
    /// `subjectPublicKeyInfo`.
    fn root_anchor<'a>(root_name: &'a [u8], root_spki: &'a [u8]) -> TrustAnchor<'a> {
        TrustAnchor { subject: root_name, subject_public_key_info: root_spki }
    }

    // ── find_trust_anchor ───────────────────────────────────────────────────

    #[test]
    fn finds_the_anchor_matching_the_issuer() {
        let name = root_name();
        let spki = ed25519_spki(&root_signing().verifying_key().to_bytes());
        let anchors = [root_anchor(&name, &spki)];
        let found = find_trust_anchor(&name, &anchors).expect("the anchor matches");
        assert_eq!(found.subject, name.as_slice());
    }

    #[test]
    fn finds_nothing_for_an_unmatched_issuer() {
        let name = root_name();
        let spki = ed25519_spki(&root_signing().verifying_key().to_bytes());
        let anchors = [root_anchor(&name, &spki)];
        assert!(find_trust_anchor(&name_rdn("Someone Else"), &anchors).is_none());
    }

    #[test]
    fn finds_nothing_in_an_empty_store() {
        assert!(find_trust_anchor(&root_name(), &[]).is_none());
    }

    // ── verify_trust_anchor: happy path ─────────────────────────────────────

    #[test]
    fn accepts_a_single_certificate_really_signed_by_its_named_anchor() {
        let root = root_signing();
        let root_name = root_name();
        let root_spki = ed25519_spki(&root.verifying_key().to_bytes());
        let anchors = [root_anchor(&root_name, &root_spki)];

        let leaf_key = impostor_signing().verifying_key().to_bytes();
        let leaf = ed25519_certificate(&root_name, &name_rdn("leaf.test"), &leaf_key, &root);

        verify_trust_anchor(&[&leaf], &anchors)
            .expect("the leaf is really signed by the anchor it names as issuer");
    }

    #[test]
    fn accepts_the_topmost_of_a_two_certificate_chain() {
        let root = root_signing();
        let root_name = root_name();
        let root_spki = ed25519_spki(&root.verifying_key().to_bytes());
        let anchors = [root_anchor(&root_name, &root_spki)];

        // The topmost (last) entry is the intermediate; the leaf (index 0) is
        // irrelevant to this walk — only chain[chain.len() - 1] is consulted.
        let intermediate_key = impostor_signing().verifying_key().to_bytes();
        let intermediate =
            ed25519_certificate(&root_name, &name_rdn("Lumen Test Intermediate"), &intermediate_key, &root);
        let leaf = ed25519_certificate(
            &name_rdn("Lumen Test Intermediate"),
            &name_rdn("leaf.test"),
            &impostor_signing().verifying_key().to_bytes(),
            &impostor_signing(),
        );

        verify_trust_anchor(&[&leaf, &intermediate], &anchors)
            .expect("the topmost (intermediate) certificate terminates at the anchor");
    }

    // ── verify_trust_anchor: rejections ──────────────────────────────────────

    #[test]
    fn rejects_an_empty_chain() {
        assert!(matches!(
            verify_trust_anchor(&[], &[]),
            Err(TrustAnchorWalkError::EmptyChain),
        ));
    }

    #[test]
    fn rejects_a_certificate_whose_issuer_matches_no_anchor() {
        let root = root_signing();
        let root_name = root_name();
        let unrelated_spki = ed25519_spki(&root.verifying_key().to_bytes());
        // The trust store only knows a *different* root than the one the leaf names.
        let different_root_name = name_rdn("A Different Root");
        let anchors = [root_anchor(&different_root_name, &unrelated_spki)];

        let leaf = ed25519_certificate(
            &root_name,
            &name_rdn("leaf.test"),
            &impostor_signing().verifying_key().to_bytes(),
            &root,
        );

        let err = verify_trust_anchor(&[&leaf], &anchors).expect_err("no anchor names this issuer");
        assert!(matches!(
            err,
            TrustAnchorWalkError::Untrusted { index: 0, error: TrustAnchorError::UnknownIssuer },
        ));
    }

    #[test]
    fn rejects_a_certificate_not_really_signed_by_the_matched_anchor() {
        let root = root_signing();
        let root_name = root_name();
        let root_spki = ed25519_spki(&root.verifying_key().to_bytes());
        let anchors = [root_anchor(&root_name, &root_spki)];

        // Names the real root as issuer, but an impostor key signed it instead.
        let impostor = impostor_signing();
        let leaf =
            ed25519_certificate(&root_name, &name_rdn("leaf.test"), &impostor.verifying_key().to_bytes(), &impostor);

        let err = verify_trust_anchor(&[&leaf], &anchors)
            .expect_err("the impostor's signature must not verify under the real root's key");
        assert!(matches!(
            err,
            TrustAnchorWalkError::Untrusted {
                index: 0,
                error: TrustAnchorError::Signature(ChainError::BadSignature),
            },
        ));
    }

    #[test]
    fn rejects_a_certificate_whose_issuer_does_not_decode() {
        let root_name = root_name();
        let root_spki = ed25519_spki(&root_signing().verifying_key().to_bytes());
        let anchors = [root_anchor(&root_name, &root_spki)];
        let garbage = tlv(TAG_INTEGER, &[0x01]);
        let err = verify_trust_anchor(&[&garbage], &anchors).expect_err("malformed certificate");
        assert!(matches!(
            err,
            TrustAnchorWalkError::Untrusted { index: 0, error: TrustAnchorError::Names(_) },
        ));
    }

    #[test]
    fn rejects_a_matched_anchor_with_an_unparsable_key() {
        let root_name = root_name();
        // The anchor's subject matches, but its subjectPublicKeyInfo is not valid DER.
        let anchors = [root_anchor(&root_name, &[0xFF, 0x00])];
        let leaf = ed25519_certificate(
            &root_name,
            &name_rdn("leaf.test"),
            &impostor_signing().verifying_key().to_bytes(),
            &root_signing(),
        );
        let err = verify_trust_anchor(&[&leaf], &anchors).expect_err("the anchor key is malformed");
        assert!(matches!(
            err,
            TrustAnchorWalkError::Untrusted { index: 0, error: TrustAnchorError::AnchorKey(_) },
        ));
    }

    // ── error plumbing ───────────────────────────────────────────────────────

    #[test]
    fn walk_error_display_and_source_cover_every_variant() {
        assert_eq!(format!("{}", TrustAnchorWalkError::EmptyChain), "certificate chain is empty");
        assert!(std::error::Error::source(&TrustAnchorWalkError::EmptyChain).is_none());

        let untrusted = TrustAnchorWalkError::Untrusted { index: 1, error: TrustAnchorError::UnknownIssuer };
        assert!(format!("{untrusted}").contains("certificate #1"));
        assert!(std::error::Error::source(&untrusted).is_some());

        for error in [
            TrustAnchorError::UnknownIssuer,
            TrustAnchorError::AnchorKey(SpkiError::UnsupportedAlgorithm),
            TrustAnchorError::Signature(ChainError::BadSignature),
        ] {
            let _ = format!("{error}");
        }
        assert!(std::error::Error::source(&TrustAnchorError::UnknownIssuer).is_none());
        assert!(std::error::Error::source(&TrustAnchorError::AnchorKey(SpkiError::UnsupportedAlgorithm)).is_some());
        assert!(std::error::Error::source(&TrustAnchorError::Signature(ChainError::BadSignature)).is_some());
    }
}
