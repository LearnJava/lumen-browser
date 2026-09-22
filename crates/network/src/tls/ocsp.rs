//! Stapled OCSP response parsing (RFC 6960) — ph3-tls-hardening, part A3.
//!
//! rustls hands a custom [`ServerCertVerifier`](rustls::client::danger::ServerCertVerifier)
//! the raw bytes of the server's stapled OCSP response (TLS `status_request` extension,
//! RFC 6066 §8) via `verify_server_cert`'s `ocsp_response` parameter — the *only* place in
//! the stack that ever sees them (see [`crate::tls::verifier`] module docs). This module
//! decodes those bytes far enough to answer one question: does the responder say this
//! certificate is `good`, `revoked`, or something else?
//!
//! ## Scope (soft-fail, Chrome-style)
//!
//! Per the task brief: `revoked` is the only hard-fail outcome. A stapled response, once
//! present, is trusted for its revocation verdict alone:
//!
//! - `responseStatus != successful`, no `responseBytes`, an unrecognised `responseType`,
//!   `certStatus: unknown`, or any structural decode failure all fold into
//!   [`OcspVerdict::Unknown`] — "no usable revocation info", never blocking.
//! - `certStatus: good` → [`OcspVerdict::Good`], recorded but not itself trust-affecting
//!   (chain trust is already `WebPkiServerVerifier`'s job; this only *adds* a revocation
//!   check, never substitutes for one).
//! - `certStatus: revoked` → [`OcspVerdict::Revoked`] — the verifier turns this into a
//!   hard failure ([`crate::tls::cert_error`] already maps `rustls::CertificateError::Revoked`
//!   to `CertError::Revoked`, from part A1).
//!
//! **Not verified**: the `BasicOCSPResponse` signature (RFC 6960 §4.2.2.2) — doing so
//! needs the OCSP responder's own certificate (often delegated, requiring its own trust
//! check back to the issuing CA) which is out of scope for this slice. This is the
//! direction that matters least for safety: an unauthenticated staple can only cause a
//! spurious hard-fail (a forged `revoked`, a denial of service against one connection),
//! never a false `good` that bypasses trust — chain validation via
//! [`WebPkiServerVerifier`](rustls::client::WebPkiServerVerifier) happens independently and
//! first. `thisUpdate`/`nextUpdate` freshness is likewise deferred — a stale response is
//! treated the same as `good`, not yet downgraded to `Unknown`.
//!
//! ## Purity
//!
//! A pure function over borrowed response bytes, no clock, no I/O — a sibling of the DER
//! readers in `crate::h3::x509_*` (same tag-length-value walking style, specialised to
//! this module's error type rather than a shared one, matching that module family's
//! convention).

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `ENUMERATED`.
const TAG_ENUMERATED: u8 = 0x0A;
/// The DER tag for `OBJECT IDENTIFIER`.
const TAG_OID: u8 = 0x06;
/// The DER tag for `OCTET STRING`.
const TAG_OCTET_STRING: u8 = 0x04;
/// The DER tag for `OCSPResponse.responseBytes`'s `[0] EXPLICIT` wrapper (context class,
/// constructed, tag number 0).
const TAG_CONTEXT_0: u8 = 0xA0;
/// The DER tag for `CertStatus`'s `good [0] IMPLICIT NULL` (context class, primitive, tag
/// number 0) — `NULL`'s own tag (0x05) is replaced by the IMPLICIT context tag, per X.690
/// §8.14, and its content is always empty.
const TAG_CERT_STATUS_GOOD: u8 = 0x80;
/// The DER tag for `CertStatus`'s `revoked [1] IMPLICIT RevokedInfo` (context class,
/// constructed, tag number 1) — constructed because `RevokedInfo` is a `SEQUENCE`.
const TAG_CERT_STATUS_REVOKED: u8 = 0xA1;

/// `id-pkix-ocsp-basic` (1.3.6.1.5.5.7.48.1.1, RFC 6960 §4.2.1) — the only
/// `ResponseBytes.responseType` this module understands. Any other value means the
/// `response` OCTET STRING is not a `BasicOCSPResponse` this parser can read.
const OID_OCSP_BASIC: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x30, 0x01, 0x01];

/// `OCSPResponseStatus`'s `successful (0)` (RFC 6960 §4.2.1) — every other status code
/// means the responder declined to answer (malformed request, internal error, try later,
/// signature required, unauthorized), not that it lacks revocation data about this cert.
const RESPONSE_STATUS_SUCCESSFUL: u8 = 0;

/// What a stapled OCSP response says about the certificate it covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcspVerdict {
    /// The responder's first `SingleResponse` reports `certStatus: good`.
    Good,
    /// The responder's first `SingleResponse` reports `certStatus: revoked`. The only
    /// verdict the verifier turns into a hard failure.
    Revoked,
    /// No usable revocation info: absent staple, a non-`successful` response status, a
    /// `responseType` other than `id-pkix-ocsp-basic`, `certStatus: unknown`, an empty
    /// `responses` list, or a structurally malformed response. Chrome-style soft-fail —
    /// never blocks the connection.
    Unknown,
}

/// Decode a stapled OCSP response (the `ocsp_response` bytes
/// [`ServerCertVerifier::verify_server_cert`](rustls::client::danger::ServerCertVerifier::verify_server_cert)
/// receives) into an [`OcspVerdict`].
///
/// An empty slice — no staple present, still the common case since OCSP stapling is
/// opt-in server-side — is [`OcspVerdict::Unknown`] without attempting to parse anything.
/// Any decode failure past that point also folds into `Unknown` rather than propagating an
/// error: per this module's soft-fail scope, "the staple didn't parse" and "the staple says
/// nothing useful" are the same outcome to the caller.
pub fn parse_stapled_response(ocsp_response: &[u8]) -> OcspVerdict {
    if ocsp_response.is_empty() {
        return OcspVerdict::Unknown;
    }
    decode(ocsp_response).unwrap_or(OcspVerdict::Unknown)
}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples left to
/// right. Definite-length only (DER forbids the indefinite form). A sibling of the readers
/// in `crate::h3::x509_*`, specialised to this module's decode-to-`Option` style (soft-fail
/// throughout, so failures collapse to `None` rather than a named error variant).
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

    /// Whether any unread bytes remain.
    fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
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

/// Decode an `OCSPResponse` (RFC 6960 §4.2.1) down to the first `SingleResponse`'s
/// `certStatus`. `None` on any structural mismatch — the caller folds that into
/// [`OcspVerdict::Unknown`].
fn decode(bytes: &[u8]) -> Option<OcspVerdict> {
    // OCSPResponse ::= SEQUENCE { responseStatus ENUMERATED, responseBytes [0] OPTIONAL }
    let response = Der::new(bytes).read_tagged(TAG_SEQUENCE)?;
    let mut response = Der::new(response);
    let status = response.read_tagged(TAG_ENUMERATED)?;
    if status != [RESPONSE_STATUS_SUCCESSFUL] {
        return Some(OcspVerdict::Unknown);
    }
    if response.peek_tag() != Some(TAG_CONTEXT_0) {
        // successful but no responseBytes: malformed per RFC 6960, but soft-fail scope
        // treats every non-actionable shape the same way.
        return Some(OcspVerdict::Unknown);
    }
    let (_, wrapper) = response.read_tlv()?;

    // ResponseBytes ::= SEQUENCE { responseType OID, response OCTET STRING }
    let response_bytes = Der::new(wrapper).read_tagged(TAG_SEQUENCE)?;
    let mut response_bytes = Der::new(response_bytes);
    let response_type = response_bytes.read_tagged(TAG_OID)?;
    if response_type != OID_OCSP_BASIC {
        return Some(OcspVerdict::Unknown);
    }
    let basic_der = response_bytes.read_tagged(TAG_OCTET_STRING)?;

    decode_basic_response(basic_der)
}

/// Decode a `BasicOCSPResponse` (RFC 6960 §4.2.1) down to the first `SingleResponse`'s
/// `certStatus`. The signature fields (`signatureAlgorithm`, `signature`, optional `certs`)
/// are present in the DER but intentionally unread — see the module-level "Not verified"
/// note.
fn decode_basic_response(bytes: &[u8]) -> Option<OcspVerdict> {
    // BasicOCSPResponse ::= SEQUENCE { tbsResponseData, signatureAlgorithm, signature, .. }
    let basic = Der::new(bytes).read_tagged(TAG_SEQUENCE)?;
    let tbs_response_data = Der::new(basic).read_tagged(TAG_SEQUENCE)?;

    // ResponseData ::= SEQUENCE { version [0] DEFAULT v1, responderID, producedAt,
    //                             responses SEQUENCE OF SingleResponse, .. }
    let mut tbs = Der::new(tbs_response_data);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version [0] EXPLICIT — optional, skip unread
    }
    tbs.read_tlv()?; // responderID (CHOICE: [1] byName or [2] byKey) — skip unread
    tbs.read_tlv()?; // producedAt GeneralizedTime — skip unread (freshness deferred)

    let responses = tbs.read_tagged(TAG_SEQUENCE)?;
    let mut responses = Der::new(responses);
    if responses.is_empty() {
        return Some(OcspVerdict::Unknown);
    }
    // Read only the first SingleResponse — Lumen staples exactly one certificate's status
    // per connection (the leaf), so a well-formed responder's list has exactly one entry.
    let single_response = responses.read_tagged(TAG_SEQUENCE)?;

    // SingleResponse ::= SEQUENCE { certID, certStatus, thisUpdate, nextUpdate [0]?, .. }
    let mut single = Der::new(single_response);
    single.read_tlv()?; // certID SEQUENCE — skip unread (not matched against the leaf)
    let (cert_status_tag, _) = single.read_tlv()?;

    Some(match cert_status_tag {
        TAG_CERT_STATUS_GOOD => OcspVerdict::Good,
        TAG_CERT_STATUS_REVOKED => OcspVerdict::Revoked,
        _ => OcspVerdict::Unknown, // unknown [2] IMPLICIT UnknownInfo, or anything else
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DER construction helpers (test-only OCSP response builder) ─────────

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

    /// A placeholder `CertID` — its contents are never interpreted by the decoder.
    fn cert_id() -> Vec<u8> {
        tlv(TAG_SEQUENCE, &tlv(TAG_OCTET_STRING, b"placeholder"))
    }

    /// A `GeneralizedTime` placeholder (tag 0x18) — never interpreted by the decoder.
    fn general_time() -> Vec<u8> {
        tlv(0x18, b"20260101000000Z")
    }

    /// A `SingleResponse` with the given `certStatus` TLV.
    fn single_response(cert_status: &[u8]) -> Vec<u8> {
        tlv(TAG_SEQUENCE, &cat(&[&cert_id(), cert_status, &general_time()]))
    }

    /// A `BasicOCSPResponse` wrapping one `SingleResponse`, with a placeholder
    /// `signatureAlgorithm`/`signature` (never read by the decoder).
    fn basic_ocsp_response(responses: &[Vec<u8>]) -> Vec<u8> {
        let refs: Vec<&[u8]> = responses.iter().map(|r| r.as_slice()).collect();
        let responses_seq = tlv(TAG_SEQUENCE, &cat(&refs));

        // responderID: byKey [2] IMPLICIT KeyHash (OCTET STRING placeholder)
        let responder_id = tlv(0x82, b"placeholder-key-hash");
        let produced_at = general_time();
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[&responder_id, &produced_at, &responses_seq]),
        );

        let sig_alg = tlv(TAG_SEQUENCE, &tlv(TAG_OID, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &sig_alg, &signature]))
    }

    /// A full `OCSPResponse` wrapping the given `BasicOCSPResponse` DER.
    fn ocsp_response(basic_der: &[u8]) -> Vec<u8> {
        let status = tlv(TAG_ENUMERATED, &[RESPONSE_STATUS_SUCCESSFUL]);
        let response_type = tlv(TAG_OID, OID_OCSP_BASIC);
        let response_octets = tlv(TAG_OCTET_STRING, basic_der);
        let response_bytes_seq = tlv(TAG_SEQUENCE, &cat(&[&response_type, &response_octets]));
        let response_bytes_wrapper = tlv(TAG_CONTEXT_0, &response_bytes_seq);
        tlv(TAG_SEQUENCE, &cat(&[&status, &response_bytes_wrapper]))
    }

    fn good_response() -> Vec<u8> {
        let single = single_response(&tlv(TAG_CERT_STATUS_GOOD, &[]));
        ocsp_response(&basic_ocsp_response(&[single]))
    }

    fn revoked_response() -> Vec<u8> {
        // RevokedInfo ::= SEQUENCE { revocationTime GeneralizedTime, reason [0]? } —
        // contents unread by the decoder, a placeholder suffices.
        let revoked_info = tlv(0x30, &general_time());
        let single = single_response(&tlv(TAG_CERT_STATUS_REVOKED, &revoked_info));
        ocsp_response(&basic_ocsp_response(&[single]))
    }

    fn unknown_status_response() -> Vec<u8> {
        let single = single_response(&tlv(0x82, &[])); // unknown [2] IMPLICIT NULL
        ocsp_response(&basic_ocsp_response(&[single]))
    }

    // ── happy paths ──────────────────────────────────────────────────────

    #[test]
    fn empty_bytes_is_unknown() {
        assert_eq!(parse_stapled_response(&[]), OcspVerdict::Unknown);
    }

    #[test]
    fn good_cert_status_is_good() {
        assert_eq!(parse_stapled_response(&good_response()), OcspVerdict::Good);
    }

    #[test]
    fn revoked_cert_status_is_revoked() {
        assert_eq!(parse_stapled_response(&revoked_response()), OcspVerdict::Revoked);
    }

    #[test]
    fn unknown_cert_status_is_unknown() {
        assert_eq!(parse_stapled_response(&unknown_status_response()), OcspVerdict::Unknown);
    }

    #[test]
    fn reads_only_the_first_of_several_responses() {
        let good = single_response(&tlv(TAG_CERT_STATUS_GOOD, &[]));
        let revoked_info = tlv(0x30, &general_time());
        let revoked = single_response(&tlv(TAG_CERT_STATUS_REVOKED, &revoked_info));
        let response = ocsp_response(&basic_ocsp_response(&[good, revoked]));
        assert_eq!(parse_stapled_response(&response), OcspVerdict::Good);
    }

    // ── soft-fail shapes ─────────────────────────────────────────────────

    #[test]
    fn non_successful_status_is_unknown() {
        let status = tlv(TAG_ENUMERATED, &[1]); // malformedRequest
        let response = tlv(TAG_SEQUENCE, &status);
        assert_eq!(parse_stapled_response(&response), OcspVerdict::Unknown);
    }

    #[test]
    fn successful_status_without_response_bytes_is_unknown() {
        let status = tlv(TAG_ENUMERATED, &[RESPONSE_STATUS_SUCCESSFUL]);
        let response = tlv(TAG_SEQUENCE, &status);
        assert_eq!(parse_stapled_response(&response), OcspVerdict::Unknown);
    }

    #[test]
    fn unrecognised_response_type_is_unknown() {
        let status = tlv(TAG_ENUMERATED, &[RESPONSE_STATUS_SUCCESSFUL]);
        let response_type = tlv(TAG_OID, &[0x2A, 0x03]); // not id-pkix-ocsp-basic
        let response_octets = tlv(TAG_OCTET_STRING, b"opaque");
        let response_bytes_seq = tlv(TAG_SEQUENCE, &cat(&[&response_type, &response_octets]));
        let response_bytes_wrapper = tlv(TAG_CONTEXT_0, &response_bytes_seq);
        let response = tlv(TAG_SEQUENCE, &cat(&[&status, &response_bytes_wrapper]));
        assert_eq!(parse_stapled_response(&response), OcspVerdict::Unknown);
    }

    #[test]
    fn empty_responses_list_is_unknown() {
        let response = ocsp_response(&basic_ocsp_response(&[]));
        assert_eq!(parse_stapled_response(&response), OcspVerdict::Unknown);
    }

    #[test]
    fn truncated_response_is_unknown() {
        let full = good_response();
        assert_eq!(parse_stapled_response(&full[..full.len() - 4]), OcspVerdict::Unknown);
    }

    #[test]
    fn garbage_bytes_are_unknown() {
        assert_eq!(parse_stapled_response(b"not a valid OCSP response at all"), OcspVerdict::Unknown);
    }

    #[test]
    fn non_sequence_top_level_is_unknown() {
        let not_a_response = tlv(TAG_ENUMERATED, &[0]);
        assert_eq!(parse_stapled_response(&not_a_response), OcspVerdict::Unknown);
    }
}
