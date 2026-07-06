//! Mozilla root store adapter (RFC 5280 §6.1, RFC 8446 §4.4.2) — slice 83 of the
//! HTTP/3 sprint.
//!
//! Nine sibling walks over a server-presented `certificate_list` are already in place —
//! the possession/identity checks, the validity walk (slices 77–79), the signature walk
//! (slices 66–68), the name walk (slices 69–70), the `basicConstraints` walk (slices
//! 71–72), the `keyUsage` walk (slices 73–74), the trust-anchor termination (slices
//! 75–76), the `extendedKeyUsage` `serverAuth` walk (slices 80–81), and the
//! unrecognized-critical-extension rejection (slice 82). Every one of them is now wired
//! into [`ConnectDriver::poll`](super::conn_connect::ConnectDriver::poll) — except that
//! the trust-anchor termination has, until this slice, only ever been fed a *synthetic*
//! anchor list built by unit tests. [`x509_trust_anchor::verify_trust_anchor`](super::x509_trust_anchor::verify_trust_anchor)
//! and [`ConnectDriver::new`](super::conn_connect::ConnectDriver::new) both, by their own
//! admission, "take the trust store as a caller-supplied parameter rather than
//! hard-coding one … a later slice supplies the real Mozilla root list the same way the
//! existing TLS path already does." This module is that supplier.
//!
//! ## What it does
//!
//! [`mozilla_trust_anchors`] turns the compiled-in `webpki_roots::TLS_SERVER_ROOTS` — the
//! same Mozilla root list `HttpClient` already loads into its `rustls::RootCertStore` for
//! the HTTP/1.1 / HTTP/2 path (`crates/network/src/lib.rs`, `crates/network/src/dot.rs`) —
//! into the `Vec<`[`OwnedTrustAnchor`](super::conn_connect::OwnedTrustAnchor)`>` shape
//! [`ConnectDriver::new`](super::conn_connect::ConnectDriver::new) accepts. It is the h3
//! path's exact counterpart of `root_store.extend(webpki_roots::TLS_SERVER_ROOTS …)`: one
//! trust store, one source of roots, shared verbatim between the two transports.
//!
//! ## The encoding fixup: both fields are re-wrapped
//!
//! `rustls_pki_types::TrustAnchor` (what `webpki_roots` hands out) stores **both**
//! `subject` and `subject_public_key_info` as bare *values* — the contents of their
//! `SEQUENCE`, **without** the outer `SEQUENCE` tag and length. A root's `subject` bytes
//! begin at the first `RelativeDistinguishedName` `SET` (tag `0x31`), not at the enclosing
//! `Name` `SEQUENCE` (tag `0x30`); its `subject_public_key_info` bytes begin at the inner
//! `AlgorithmIdentifier` `SEQUENCE`, not at the enclosing `SubjectPublicKeyInfo`
//! `SEQUENCE`. This is `webpki`'s long-standing decomposed storage convention: it
//! reconstructs the wrappers internally, so it can afford to omit them.
//!
//! Every consumer in *this* sprint, by contrast, wants the *whole*
//! `tag ‖ length ‖ contents` `SEQUENCE` span:
//! [`x509_name_chain::certificate_names`](super::x509_name_chain::certificate_names)
//! returns a certificate's `issuer` as the full outer `Name` `SEQUENCE` (so
//! [`OwnedTrustAnchor::subject`](super::conn_connect::OwnedTrustAnchor::subject) must be
//! that same full span to compare byte-for-byte), and
//! [`x509_spki::parse_subject_public_key_info`](super::x509_spki::parse_subject_public_key_info)
//! decodes a full `SubjectPublicKeyInfo` `SEQUENCE` (its first read strips an outer
//! `0x30`). So each root's `subject` **and** `subject_public_key_info` value is re-wrapped
//! in a `SEQUENCE` header here ([`wrap_in_sequence`]) before it becomes an
//! [`OwnedTrustAnchor`]. A root imported without this fixup would never match any real
//! certificate's `issuer`, and its key would never decode — silently trusting *nothing*,
//! the most dangerous kind of failure: one that looks like it verified. (The unit test
//! [`every_public_key_is_a_wrapped_parsable_spki`](tests::every_public_key_is_a_wrapped_parsable_spki)
//! guards exactly this — an earlier draft that wrapped only the subject was caught by it.)
//!
//! `name_constraints` (a third `TrustAnchor` field carried by a handful of roots) is
//! deliberately dropped: [`OwnedTrustAnchor`](super::conn_connect::OwnedTrustAnchor) does
//! not model per-anchor `nameConstraints`, and neither does any walk in this sprint yet,
//! exactly as [`x509_trust_anchor`](super::x509_trust_anchor)'s module docs note. Enforcing
//! a root's own `nameConstraints` is a later slice; until it exists, importing a root
//! that carries them and ignoring the constraint is no *less* safe than the existing
//! `rustls` path, which delegates that enforcement to `webpki` — it is simply not yet
//! *more* complete.
//!
//! ## Purity
//!
//! Pure allocation over compiled-in bytes: no clock, no I/O, no fallible parse — the
//! source roots are already valid DER, and re-wrapping a `SEQUENCE` cannot fail. The
//! result is a fresh owned `Vec` the caller moves into a [`ConnectDriver`](super::conn_connect::ConnectDriver);
//! this module holds no state.

use super::conn_connect::OwnedTrustAnchor;

/// Build the real Mozilla trust store as a `Vec<`[`OwnedTrustAnchor`]`>`, ready to hand
/// to [`ConnectDriver::new`](super::conn_connect::ConnectDriver::new) as its `trust_anchors`
/// argument.
///
/// The roots come from the compiled-in `webpki_roots::TLS_SERVER_ROOTS` — the same
/// Mozilla list the HTTP/1.1 / HTTP/2 TLS path loads into its `rustls::RootCertStore`
/// (`crates/network/src/lib.rs`) — so both transports terminate their certificate chains
/// at one identical set of anchors. Both a root's `subject` and its
/// `subject_public_key_info` value are re-wrapped in a `SEQUENCE` header
/// ([`wrap_in_sequence`]) — `webpki_roots` stores each as a bare value without its outer
/// `SEQUENCE` — so the subject compares byte-for-byte against a certificate's `issuer` as
/// [`x509_name_chain::certificate_names`](super::x509_name_chain::certificate_names)
/// returns it, and the key decodes as the full `SubjectPublicKeyInfo`
/// [`x509_spki::parse_subject_public_key_info`](super::x509_spki::parse_subject_public_key_info)
/// expects. Per-anchor `nameConstraints` are dropped — see the module docs.
///
/// This is pure and infallible: the source roots are valid DER and re-wrapping a
/// `SEQUENCE` cannot fail.
#[must_use]
pub fn mozilla_trust_anchors() -> Vec<OwnedTrustAnchor> {
    webpki_roots::TLS_SERVER_ROOTS
        .iter()
        .map(|root| OwnedTrustAnchor {
            subject: wrap_in_sequence(root.subject.as_ref()),
            subject_public_key_info: wrap_in_sequence(root.subject_public_key_info.as_ref()),
        })
        .collect()
}

/// Wrap a bare DER `value` (the contents `webpki_roots` stores for a `TrustAnchor`'s
/// `subject` `Name` or `subject_public_key_info`, each without its own outer header) in a
/// `SEQUENCE`, yielding the full `tag ‖ length ‖ contents` span every consumer in this
/// sprint expects.
///
/// The output is `0x30 ‖ DER-length(value.len()) ‖ value`: a `SEQUENCE` (universal tag
/// `0x30`, RFC 5280 uses definite-length DER throughout) wrapping the given value bytes
/// unchanged. Both a `Name` and a `SubjectPublicKeyInfo` are `SEQUENCE`s at their top
/// level, so the same wrapper serves both.
fn wrap_in_sequence(value: &[u8]) -> Vec<u8> {
    /// DER `SEQUENCE` tag (universal, constructed): `0b0011_0000`.
    const TAG_SEQUENCE: u8 = 0x30;
    let mut out = Vec::with_capacity(value.len() + 4);
    out.push(TAG_SEQUENCE);
    encode_der_len(value.len(), &mut out);
    out.extend_from_slice(value);
    out
}

/// Append a DER definite-length encoding of `len` to `out` (X.690 §8.1.3): the short form
/// (a single byte) for lengths under `0x80`, otherwise the long form (a `0x80 | n` count
/// byte followed by the `n` big-endian minimal-width length octets).
fn encode_der_len(len: usize, out: &mut Vec<u8>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::x509_spki::parse_subject_public_key_info;

    #[test]
    fn loads_a_non_empty_store() {
        let anchors = mozilla_trust_anchors();
        // webpki_roots ships well over 100 Mozilla roots; the exact count drifts with
        // upstream, so assert only that the store is populated, not an exact number.
        assert!(anchors.len() >= 100, "expected the real Mozilla store, got {}", anchors.len());
        assert_eq!(anchors.len(), webpki_roots::TLS_SERVER_ROOTS.len());
    }

    #[test]
    fn every_subject_is_a_wrapped_sequence_over_the_raw_root_value() {
        // Each imported subject must be exactly 0x30 ‖ len ‖ <raw webpki value>.
        for (anchor, root) in mozilla_trust_anchors().iter().zip(webpki_roots::TLS_SERVER_ROOTS) {
            let value = root.subject.as_ref();
            assert_eq!(anchor.subject.first(), Some(&0x30), "subject must open with SEQUENCE tag");

            let mut expected = vec![0x30u8];
            encode_der_len(value.len(), &mut expected);
            expected.extend_from_slice(value);
            assert_eq!(anchor.subject, expected);

            // And the wrapped span's declared length really covers the value: the bytes
            // after the header equal the raw root value byte-for-byte.
            assert!(anchor.subject.ends_with(value));
        }
    }

    #[test]
    fn every_public_key_is_a_wrapped_parsable_spki() {
        // subject_public_key_info is re-wrapped (webpki stores it without its outer
        // SEQUENCE) and must then decode as a full SubjectPublicKeyInfo — the shape
        // verify_trust_anchor feeds to x509_spki. Without the wrapping, x509_spki reads
        // the inner AlgorithmIdentifier as the outer SPKI and rejects the root: a store
        // that silently trusts nothing. Every root must use a key algorithm x509_spki
        // supports (ECDSA P-256/384/521, Ed25519, RSA), so every one must decode.
        for (anchor, root) in mozilla_trust_anchors().iter().zip(webpki_roots::TLS_SERVER_ROOTS) {
            let mut expected = vec![0x30u8];
            encode_der_len(root.subject_public_key_info.as_ref().len(), &mut expected);
            expected.extend_from_slice(root.subject_public_key_info.as_ref());
            assert_eq!(anchor.subject_public_key_info, expected);

            parse_subject_public_key_info(&anchor.subject_public_key_info)
                .expect("each Mozilla root's re-wrapped SPKI must decode");
        }
    }

    #[test]
    fn wraps_a_short_value_with_the_short_length_form() {
        let value = [0x31, 0x03, 0x02, 0x01, 0x2A]; // arbitrary short RDNSequence-shaped bytes
        let wrapped = wrap_in_sequence(&value);
        assert_eq!(wrapped[0], 0x30);
        assert_eq!(wrapped[1] as usize, value.len());
        assert_eq!(&wrapped[2..], &value);
    }

    #[test]
    fn wraps_a_long_value_with_the_long_length_form() {
        // 200 bytes needs the long form: 0x81 count byte, then a single 0xC8 length octet.
        let value = vec![0x04u8; 200];
        let wrapped = wrap_in_sequence(&value);
        assert_eq!(wrapped[0], 0x30);
        assert_eq!(wrapped[1], 0x81);
        assert_eq!(wrapped[2], 200);
        assert_eq!(&wrapped[3..], &value[..]);
    }

    #[test]
    fn der_len_short_and_long_forms() {
        let mut out = Vec::new();
        encode_der_len(0x7F, &mut out);
        assert_eq!(out, [0x7F]); // short form: one byte, no count prefix

        out.clear();
        encode_der_len(0x80, &mut out);
        assert_eq!(out, [0x81, 0x80]); // long form: count 1, then the length octet

        out.clear();
        encode_der_len(0x0102, &mut out);
        assert_eq!(out, [0x82, 0x01, 0x02]); // long form: count 2, big-endian minimal width
    }
}
