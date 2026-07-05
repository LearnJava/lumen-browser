//! QUIC Retry packet integrity + address-validation token handling
//! (RFC 9000 §17.2.5, §8.1; RFC 9001 §5.8).
//!
//! A stateless server that wishes to validate a client's address before
//! committing connection state answers the client's first Initial packet with a
//! Retry packet ([`super::packet::Packet::Retry`], codec'd in [`super::packet`]).
//! The Retry carries an opaque address-validation Token the client must echo in
//! the Token field of its subsequent Initial packets, a fresh server-chosen
//! Source Connection ID the client adopts as its new Destination Connection ID,
//! and a 16-byte Retry Integrity Tag that authenticates the whole exchange
//! against tampering by an off-path attacker (RFC 9000 §17.2.5).
//!
//! This slice is the pure crypto + state that a client runs on a received Retry:
//!
//! - [`retry_integrity_tag`] / [`verify_retry_integrity`] compute and check the
//!   Retry Integrity Tag (RFC 9001 §5.8). The tag is
//!   `AEAD_AES_128_GCM` over an empty plaintext with the version-fixed
//!   [`RETRY_KEY_V1`] / [`RETRY_NONCE_V1`], authenticating the *Retry
//!   Pseudo-Packet* — the Original Destination Connection ID the client chose for
//!   its first Initial (length-prefixed by one octet) followed by the entire
//!   Retry packet up to but not including the tag. The ODCID is never carried on
//!   the wire, so an attacker who does not observe the client's first Initial
//!   cannot forge a Retry. Validated against the RFC 9001 Appendix A.4 vector.
//! - [`RetryHandler`] is the client-side state machine: it verifies the tag,
//!   enforces that a client accepts at most one Retry per connection (a Retry
//!   received after processing an accepted Retry MUST be discarded, RFC 9000
//!   §17.2.5), and reports the [`RetryOutcome`] — the new Destination Connection
//!   ID (the Retry's Source CID) and the Token to echo.
//!
//! Pure functions and state; no IO. The caller drives it with a decoded
//! [`super::packet::Packet`] and re-derives its Initial keys from the new DCID
//! (via [`super::key_schedule`]) once a Retry is accepted.

use super::packet::{MAX_CONNECTION_ID_LEN, Packet, PacketError, RETRY_INTEGRITY_TAG_LEN};
use super::packet_protect::{self, ProtectionError};

/// QUIC v1 Retry Integrity Tag AEAD key (RFC 9001 §5.8).
pub const RETRY_KEY_V1: [u8; 16] = [
    0xbe, 0x0c, 0x69, 0x0b, 0x9f, 0x66, 0x57, 0x5a, 0x1d, 0x76, 0x6b, 0x54, 0xe3, 0x68, 0xc8, 0x4e,
];

/// QUIC v1 Retry Integrity Tag AEAD nonce (RFC 9001 §5.8).
pub const RETRY_NONCE_V1: [u8; 12] =
    [0x46, 0x15, 0x99, 0xd3, 0x5d, 0x63, 0x2b, 0xf2, 0x23, 0x98, 0x25, 0xbb];

/// Failure processing a Retry packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryError {
    /// The supplied [`Packet`] is not a [`Packet::Retry`].
    NotRetry,
    /// The Original Destination Connection ID exceeds [`MAX_CONNECTION_ID_LEN`]
    /// and cannot be length-prefixed by the single Pseudo-Packet octet.
    OdcidTooLong,
    /// The computed Retry Integrity Tag does not match the packet's tag: the
    /// Retry is forged or corrupt and MUST be discarded (RFC 9000 §17.2.5).
    IntegrityMismatch,
    /// A second Retry was received after one was already accepted (RFC 9000
    /// §17.2.5): the client MUST discard it.
    DuplicateRetry,
    /// Re-encoding the packet to build the Pseudo-Packet failed.
    Packet(PacketError),
    /// The AEAD transform failed (e.g. a bad fixed key/nonce length).
    Protection(ProtectionError),
}

impl core::fmt::Display for RetryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RetryError::NotRetry => write!(f, "packet is not a Retry packet"),
            RetryError::OdcidTooLong => {
                write!(f, "original destination connection ID exceeds {MAX_CONNECTION_ID_LEN} bytes")
            }
            RetryError::IntegrityMismatch => write!(f, "Retry integrity tag mismatch"),
            RetryError::DuplicateRetry => write!(f, "a Retry was already accepted"),
            RetryError::Packet(e) => write!(f, "Retry packet re-encode failed: {e}"),
            RetryError::Protection(e) => write!(f, "Retry AEAD failed: {e}"),
        }
    }
}

impl std::error::Error for RetryError {}

/// Build the Retry Pseudo-Packet (RFC 9001 §5.8): the Original Destination
/// Connection ID length-prefixed by a single octet, followed by the entire Retry
/// packet up to (but not including) the trailing Integrity Tag.
fn retry_pseudo_packet(odcid: &[u8], packet: &Packet) -> Result<Vec<u8>, RetryError> {
    if !matches!(packet, Packet::Retry { .. }) {
        return Err(RetryError::NotRetry);
    }
    if odcid.len() > MAX_CONNECTION_ID_LEN {
        return Err(RetryError::OdcidTooLong);
    }
    let mut encoded = Vec::new();
    packet.encode(&mut encoded).map_err(RetryError::Packet)?;
    // The tag is the final `RETRY_INTEGRITY_TAG_LEN` bytes; the Pseudo-Packet
    // authenticates everything before it. `Packet::Retry` always encodes the tag
    // last, so the length is never below the tag width.
    let head = encoded
        .len()
        .checked_sub(RETRY_INTEGRITY_TAG_LEN)
        .ok_or(RetryError::NotRetry)?;

    let mut pseudo = Vec::with_capacity(1 + odcid.len() + head);
    pseudo.push(odcid.len() as u8);
    pseudo.extend_from_slice(odcid);
    pseudo.extend_from_slice(&encoded[..head]);
    Ok(pseudo)
}

/// Compute the Retry Integrity Tag for `packet` given the `odcid` the client used
/// in its first Initial packet (RFC 9001 §5.8). The tag is
/// `AEAD_AES_128_GCM(RETRY_KEY_V1, RETRY_NONCE_V1, Retry Pseudo-Packet, "")` — an
/// empty plaintext, so the AEAD output is exactly the 16-byte authentication tag.
///
/// # Errors
///
/// [`RetryError::NotRetry`] if `packet` is not a Retry, [`RetryError::OdcidTooLong`]
/// if the ODCID cannot be length-prefixed, or a wrapped codec/AEAD error.
pub fn retry_integrity_tag(
    odcid: &[u8],
    packet: &Packet,
) -> Result<[u8; RETRY_INTEGRITY_TAG_LEN], RetryError> {
    let pseudo = retry_pseudo_packet(odcid, packet)?;
    // packet_number 0 leaves the nonce equal to RETRY_NONCE_V1 (iv XOR 0 = iv),
    // and an empty plaintext yields ciphertext-free output that is just the tag.
    let sealed = packet_protect::aes_128_gcm_seal(&RETRY_KEY_V1, &RETRY_NONCE_V1, 0, &pseudo, &[])
        .map_err(RetryError::Protection)?;
    let mut tag = [0u8; RETRY_INTEGRITY_TAG_LEN];
    if sealed.len() != RETRY_INTEGRITY_TAG_LEN {
        return Err(RetryError::Protection(ProtectionError::AeadFailed));
    }
    tag.copy_from_slice(&sealed);
    Ok(tag)
}

/// Verify a received Retry packet's Integrity Tag against the `odcid` the client
/// chose for its first Initial packet (RFC 9001 §5.8). On a mismatch the Retry is
/// forged or corrupt and MUST be discarded (RFC 9000 §17.2.5).
///
/// # Errors
///
/// [`RetryError::NotRetry`] if `packet` is not a Retry, [`RetryError::IntegrityMismatch`]
/// if the tag does not verify, or a wrapped codec/AEAD error.
pub fn verify_retry_integrity(odcid: &[u8], packet: &Packet) -> Result<(), RetryError> {
    let expected = retry_integrity_tag(odcid, packet)?;
    let Packet::Retry { integrity_tag, .. } = packet else {
        return Err(RetryError::NotRetry);
    };
    if ct_eq(&expected, integrity_tag) {
        Ok(())
    } else {
        Err(RetryError::IntegrityMismatch)
    }
}

/// Constant-time byte-slice equality: returns `true` iff the slices have equal
/// length and equal contents, taking time independent of *where* they first
/// differ, so tag comparison cannot leak the correct value byte by byte.
#[must_use]
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The result of accepting a valid Retry (RFC 9000 §17.2.5): the values the
/// client applies before re-sending its Initial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryOutcome {
    /// The new Destination Connection ID — the Retry's Source Connection ID —
    /// the client stamps on its retried Initial packets and derives fresh
    /// Initial keys from (RFC 9000 §17.2.5, RFC 9001 §5.2).
    pub new_dcid: Vec<u8>,
    /// The address-validation Token to echo in the Token field of subsequent
    /// Initial packets (RFC 9000 §8.1).
    pub token: Vec<u8>,
}

/// Client-side Retry state machine (RFC 9000 §17.2.5).
///
/// A client accepts at most one Retry per connection: a Retry received after one
/// has already been accepted MUST be discarded. [`RetryHandler`] verifies the
/// integrity tag and enforces that rule, so the connection layer never re-derives
/// keys from a replayed or injected second Retry.
#[derive(Clone, Debug, Default)]
pub struct RetryHandler {
    /// Whether a Retry has already been accepted on this connection.
    accepted: bool,
}

impl RetryHandler {
    /// A fresh handler that has not yet accepted a Retry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a Retry has already been accepted on this connection.
    #[must_use]
    pub fn has_accepted(&self) -> bool {
        self.accepted
    }

    /// Process a received Retry against the `odcid` the client used in its first
    /// Initial packet. On success the handler records that a Retry was accepted
    /// and returns the [`RetryOutcome`] to apply.
    ///
    /// # Errors
    ///
    /// [`RetryError::DuplicateRetry`] if a Retry was already accepted,
    /// [`RetryError::IntegrityMismatch`] if the tag does not verify,
    /// [`RetryError::NotRetry`] if `packet` is not a Retry, or a wrapped
    /// codec/AEAD error. On any error the handler state is unchanged.
    pub fn accept(&mut self, odcid: &[u8], packet: &Packet) -> Result<RetryOutcome, RetryError> {
        if self.accepted {
            return Err(RetryError::DuplicateRetry);
        }
        verify_retry_integrity(odcid, packet)?;
        let Packet::Retry { scid, retry_token, .. } = packet else {
            return Err(RetryError::NotRetry);
        };
        self.accepted = true;
        Ok(RetryOutcome { new_dcid: scid.clone(), token: retry_token.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a hex string into bytes (test helper).
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// The RFC 9001 Appendix A.4 Retry packet and the ODCID it authenticates.
    fn rfc9001_a4() -> (Vec<u8>, Packet) {
        let odcid = hex("8394c8f03e515708");
        let wire = hex("ff000000010008f067a5502a4262b5746f6b656e04a265ba2eff4d829058fb3f0f2496ba");
        let (packet, consumed) = Packet::parse(&wire, 0).unwrap();
        assert_eq!(consumed, wire.len());
        (odcid, packet)
    }

    #[test]
    fn a4_vector_fields() {
        let (_odcid, packet) = rfc9001_a4();
        let Packet::Retry { version, dcid, scid, retry_token, integrity_tag, .. } = &packet else {
            panic!("expected Retry");
        };
        assert_eq!(*version, 1);
        assert!(dcid.is_empty());
        assert_eq!(scid, &hex("f067a5502a4262b5"));
        assert_eq!(retry_token, b"token");
        assert_eq!(integrity_tag.as_slice(), hex("04a265ba2eff4d829058fb3f0f2496ba"));
    }

    #[test]
    fn integrity_tag_matches_rfc9001_a4() {
        let (odcid, packet) = rfc9001_a4();
        let tag = retry_integrity_tag(&odcid, &packet).unwrap();
        assert_eq!(tag.as_slice(), hex("04a265ba2eff4d829058fb3f0f2496ba"));
    }

    #[test]
    fn verify_accepts_valid_retry() {
        let (odcid, packet) = rfc9001_a4();
        assert_eq!(verify_retry_integrity(&odcid, &packet), Ok(()));
    }

    #[test]
    fn verify_rejects_wrong_odcid() {
        let (mut odcid, packet) = rfc9001_a4();
        odcid[0] ^= 0x01;
        assert_eq!(verify_retry_integrity(&odcid, &packet), Err(RetryError::IntegrityMismatch));
    }

    #[test]
    fn verify_rejects_truncated_odcid() {
        let (odcid, packet) = rfc9001_a4();
        // A shorter ODCID changes the length prefix and the authenticated bytes.
        assert_eq!(
            verify_retry_integrity(&odcid[..odcid.len() - 1], &packet),
            Err(RetryError::IntegrityMismatch)
        );
    }

    #[test]
    fn verify_rejects_tampered_tag() {
        let (odcid, mut packet) = rfc9001_a4();
        if let Packet::Retry { integrity_tag, .. } = &mut packet {
            integrity_tag[0] ^= 0xff;
        }
        assert_eq!(verify_retry_integrity(&odcid, &packet), Err(RetryError::IntegrityMismatch));
    }

    #[test]
    fn verify_rejects_tampered_token() {
        let (odcid, mut packet) = rfc9001_a4();
        if let Packet::Retry { retry_token, .. } = &mut packet {
            retry_token.push(0x00);
        }
        assert_eq!(verify_retry_integrity(&odcid, &packet), Err(RetryError::IntegrityMismatch));
    }

    #[test]
    fn tag_and_verify_reject_non_retry() {
        let odcid = hex("8394c8f03e515708");
        let packet = Packet::Short {
            spin: false,
            protected_bits: 0,
            dcid: vec![1, 2, 3, 4],
            protected: vec![0; 32],
        };
        assert_eq!(retry_integrity_tag(&odcid, &packet), Err(RetryError::NotRetry));
        assert_eq!(verify_retry_integrity(&odcid, &packet), Err(RetryError::NotRetry));
    }

    #[test]
    fn tag_rejects_oversized_odcid() {
        let (_odcid, packet) = rfc9001_a4();
        let big = vec![0u8; MAX_CONNECTION_ID_LEN + 1];
        assert_eq!(retry_integrity_tag(&big, &packet), Err(RetryError::OdcidTooLong));
    }

    #[test]
    fn max_length_odcid_is_accepted_by_the_codec() {
        // A 20-byte ODCID still fits the single-octet length prefix.
        let (_odcid, packet) = rfc9001_a4();
        let max = vec![0u8; MAX_CONNECTION_ID_LEN];
        assert!(retry_integrity_tag(&max, &packet).is_ok());
    }

    #[test]
    fn handler_accepts_then_reports_outcome() {
        let (odcid, packet) = rfc9001_a4();
        let mut handler = RetryHandler::new();
        assert!(!handler.has_accepted());
        let outcome = handler.accept(&odcid, &packet).unwrap();
        assert_eq!(outcome.new_dcid, hex("f067a5502a4262b5"));
        assert_eq!(outcome.token, b"token");
        assert!(handler.has_accepted());
    }

    #[test]
    fn handler_rejects_second_retry() {
        let (odcid, packet) = rfc9001_a4();
        let mut handler = RetryHandler::new();
        handler.accept(&odcid, &packet).unwrap();
        assert_eq!(handler.accept(&odcid, &packet), Err(RetryError::DuplicateRetry));
    }

    #[test]
    fn handler_bad_tag_leaves_state_unchanged() {
        let (mut odcid, packet) = rfc9001_a4();
        odcid[0] ^= 0x01;
        let mut handler = RetryHandler::new();
        assert_eq!(handler.accept(&odcid, &packet), Err(RetryError::IntegrityMismatch));
        // A failed accept must not consume the one-Retry budget.
        assert!(!handler.has_accepted());
    }

    #[test]
    fn ct_eq_matches_semantic_equality() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(ct_eq(b"", b""));
    }
}
