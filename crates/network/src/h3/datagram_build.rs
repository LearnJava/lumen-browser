//! QUIC outgoing datagram assembly (RFC 9000 §12.2, §14.1).
//!
//! The send-side mirror of [`super::datagram`]: where
//! [`datagram::parse_datagram`](super::datagram::parse_datagram) splits one
//! *received* UDP datagram into its coalesced packets and
//! [`datagram::encode_datagram`](super::datagram::encode_datagram) re-serializes
//! parsed [`Packet`](super::packet::Packet)s, this module coalesces the
//! *encrypted* packet byte strings produced by
//! [`packet_crypt::encrypt_packet`](super::packet_crypt::encrypt_packet) into one
//! outgoing datagram. It is the piece that turns the per-packet
//! [`packet_payload::PayloadBuilder`](super::packet_payload::PayloadBuilder) →
//! [`packet_crypt`](super::packet_crypt) pipeline into a single UDP payload ready
//! to hand to the socket, and the "assembling probe datagrams" step the QUIC
//! transport plan called for (a PTO probe is just a datagram built here from a
//! probe payload).
//!
//! [`DatagramBuilder`] enforces two invariants on the send path:
//!
//! - **Coalescing (RFC 9000 §12.2).** Only a length-delimited long-header packet
//!   (Initial / 0-RTT / Handshake, each carrying an explicit `Length`) may be
//!   *followed* by another packet. A short-header (1-RTT) packet has no `Length`
//!   and runs to the datagram's end, so it must be last; once one is appended the
//!   builder is *sealed* and rejects further packets. This is the exact mirror of
//!   the [`DatagramError::UnterminatedCoalescing`](super::datagram::DatagramError::UnterminatedCoalescing)
//!   rule the receive-side encoder enforces.
//! - **Datagram-size budget.** Every packet must fit within `max_len`, the largest
//!   UDP payload the path is known to carry — the confirmed size from
//!   [`path_mtu`](super::path_mtu). A packet that would overflow is refused
//!   without mutating the datagram, so the caller can flush the current datagram
//!   and start a fresh one.
//!
//! Because a datagram carrying a client Initial must be at least
//! [`MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN) bytes
//! (RFC 9000 §14.1), and every datagram byte must belong to a QUIC packet (loose
//! trailing bytes are illegal), the padding cannot be appended here — it lives as
//! PADDING frames *inside* a packet's payload, added before encryption via
//! [`packet_payload::PayloadBuilder::pad_to`](super::packet_payload::PayloadBuilder::pad_to).
//! [`DatagramBuilder::initial_padding_shortfall`] reports how many bytes short the
//! datagram is; since a PADDING frame is a single `0x00` byte (RFC 9000 §19.1) the
//! shortfall is 1:1 the number of PADDING bytes the caller adds to the final
//! packet's payload.
//!
//! The module is pure: it operates on a supplied key set and byte buffers, with no
//! IO, no clock, and no connection state. Retry and Version Negotiation packets
//! are not produced by [`packet_crypt::encrypt_packet`](super::packet_crypt::encrypt_packet)
//! (they carry no AEAD payload) and so never appear here.

use super::datagram::initial_padding_shortfall;
use super::key_schedule::PacketProtectionKeys;
use super::packet_crypt::{self, PacketCryptError, ProtectedHeader};

/// Something that prevented a packet from being coalesced into a datagram.
///
/// A packet that simply does not fit the remaining budget is *not* an error:
/// [`DatagramBuilder::push`] reports that as `Ok(false)` so the caller can flush
/// and retry the packet in a fresh datagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatagramBuildError {
    /// Encrypting the packet failed. Carries the underlying
    /// [`PacketCryptError`] from
    /// [`packet_crypt::encrypt_packet`](super::packet_crypt::encrypt_packet).
    Crypt(PacketCryptError),
    /// A packet was offered after a non-length-delimited (short-header) packet had
    /// already been appended. RFC 9000 §12.2 permits such a packet only as the
    /// last one, so the builder is sealed and refuses to coalesce more.
    Sealed,
}

impl core::fmt::Display for DatagramBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Crypt(e) => write!(f, "QUIC datagram build: {e}"),
            Self::Sealed => write!(
                f,
                "QUIC datagram build: a non-length-delimited packet must be last"
            ),
        }
    }
}

impl std::error::Error for DatagramBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Crypt(e) => Some(e),
            Self::Sealed => None,
        }
    }
}

impl From<PacketCryptError> for DatagramBuildError {
    fn from(e: PacketCryptError) -> Self {
        Self::Crypt(e)
    }
}

/// Coalesces encrypted QUIC packets into one outgoing UDP datagram under a size
/// budget (RFC 9000 §12.2, §14.1).
///
/// Push packets front to back; each is encrypted and appended if it fits and the
/// coalescing rule allows it. When the builder is full or a short-header packet
/// has been appended, take the bytes with [`DatagramBuilder::into_bytes`] and send
/// them, then start a new builder for the next datagram.
#[derive(Clone, Debug)]
pub struct DatagramBuilder {
    /// The largest datagram this builder will produce (the confirmed path MTU).
    max_len: usize,
    /// The coalesced packet bytes accumulated so far.
    bytes: Vec<u8>,
    /// Set once a non-length-delimited (short-header) packet has been appended;
    /// nothing may follow it (RFC 9000 §12.2).
    sealed: bool,
    /// Set if any appended packet is an Initial, so the datagram must reach
    /// [`MIN_INITIAL_DATAGRAM_LEN`] (RFC 9000 §14.1).
    carries_initial: bool,
}

impl DatagramBuilder {
    /// Create a builder that will produce a datagram of at most `max_len` bytes.
    ///
    /// `max_len` is the confirmed path-MTU datagram size (see
    /// [`path_mtu`](super::path_mtu)); for a client's first flight it is the
    /// [`MIN_INITIAL_DATAGRAM_LEN`] floor.
    #[must_use]
    pub const fn new(max_len: usize) -> Self {
        Self { max_len, bytes: Vec::new(), sealed: false, carries_initial: false }
    }

    /// Encrypt one packet and coalesce it into the datagram.
    ///
    /// The packet is sealed with [`packet_crypt::encrypt_packet`](super::packet_crypt::encrypt_packet)
    /// (choosing the packet-number width from `largest_acked`, RFC 9000 §17.1) and
    /// appended if it fits the remaining budget. Returns `Ok(true)` when appended,
    /// or `Ok(false)` when it would overflow `max_len` — in which case nothing is
    /// mutated and the caller should flush the current datagram and retry the
    /// packet (with the same packet number) in a fresh builder.
    ///
    /// Whether the packet is length-delimited is taken from `header`: the
    /// long-header forms may be followed by another packet, while a
    /// [`ProtectedHeader::Short`] seals the datagram.
    ///
    /// # Errors
    ///
    /// [`DatagramBuildError::Crypt`] if encryption fails, or
    /// [`DatagramBuildError::Sealed`] if a packet is pushed after a short-header
    /// packet has already ended the datagram.
    pub fn push(
        &mut self,
        keys: &PacketProtectionKeys,
        header: &ProtectedHeader<'_>,
        packet_number: u64,
        largest_acked: Option<u64>,
        payload: &[u8],
    ) -> Result<bool, DatagramBuildError> {
        if self.sealed {
            return Err(DatagramBuildError::Sealed);
        }
        let packet = packet_crypt::encrypt_packet(keys, header, packet_number, largest_acked, payload)?;
        let length_delimited = !matches!(header, ProtectedHeader::Short { .. });
        let is_initial = matches!(header, ProtectedHeader::Initial { .. });
        Ok(self.append(&packet, length_delimited, is_initial))
    }

    /// Coalesce an already-encrypted packet's bytes.
    ///
    /// The lower-level counterpart of [`DatagramBuilder::push`] for a caller that
    /// encrypted the packet itself (for example with a fixed packet-number width).
    /// `length_delimited` must be `true` for a long-header packet (Initial / 0-RTT
    /// / Handshake) and `false` for a short-header packet; `is_initial` records
    /// whether the packet is an Initial for the [`initial_padding_shortfall`]
    /// accounting. Returns `Ok(true)` when appended, `Ok(false)` when it would
    /// overflow.
    ///
    /// # Errors
    ///
    /// [`DatagramBuildError::Sealed`] if pushed after a non-length-delimited
    /// packet.
    pub fn push_encrypted(
        &mut self,
        packet: &[u8],
        length_delimited: bool,
        is_initial: bool,
    ) -> Result<bool, DatagramBuildError> {
        if self.sealed {
            return Err(DatagramBuildError::Sealed);
        }
        Ok(self.append(packet, length_delimited, is_initial))
    }

    /// Append `packet` if it fits, updating the sealed / Initial flags. Returns
    /// whether it was appended. Assumes `self.sealed` was already checked.
    fn append(&mut self, packet: &[u8], length_delimited: bool, is_initial: bool) -> bool {
        if packet.len() > self.remaining() {
            return false;
        }
        self.bytes.extend_from_slice(packet);
        self.carries_initial |= is_initial;
        // A short-header (non-length-delimited) packet runs to the datagram's end,
        // so no packet may follow it (RFC 9000 §12.2).
        self.sealed |= !length_delimited;
        true
    }

    /// The datagram-size budget this builder was created with.
    #[must_use]
    pub const fn max_len(&self) -> usize {
        self.max_len
    }

    /// The number of bytes coalesced so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether no packet has been coalesced yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The remaining budget for further packets (`max_len` − current length).
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.max_len.saturating_sub(self.bytes.len())
    }

    /// Whether a short-header packet has ended the datagram, so no further packet
    /// may be coalesced (RFC 9000 §12.2).
    #[must_use]
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// Whether the datagram carries an Initial packet and so must reach
    /// [`MIN_INITIAL_DATAGRAM_LEN`] (RFC 9000 §14.1).
    #[must_use]
    pub const fn carries_initial(&self) -> bool {
        self.carries_initial
    }

    /// How many bytes short of [`MIN_INITIAL_DATAGRAM_LEN`] the datagram is, or `0`
    /// if it carries no Initial (RFC 9000 §14.1).
    ///
    /// A non-zero result is the number of PADDING frames (`0x00` bytes, RFC 9000
    /// §19.1) the caller must add to a packet's payload *before encryption* to make
    /// the datagram large enough — padding cannot be appended here, since every
    /// datagram byte must belong to a QUIC packet.
    #[must_use]
    pub fn initial_padding_shortfall(&self) -> usize {
        if self.carries_initial {
            initial_padding_shortfall(self.bytes.len())
        } else {
            0
        }
    }

    /// The coalesced datagram bytes so far, for inspection.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the builder and yield the finished datagram bytes, ready to send.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::datagram::MIN_INITIAL_DATAGRAM_LEN;
    use crate::h3::key_schedule::InitialKeys;

    /// Decode a hex string (ignoring whitespace) into bytes.
    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    /// The RFC 9001 Appendix A client Destination Connection ID.
    fn a2_dcid() -> Vec<u8> {
        hex("8394c8f03e515708")
    }

    /// Both directions' Initial keys derived from the RFC 9001 A.1 DCID.
    fn keys() -> InitialKeys {
        InitialKeys::derive(&a2_dcid())
    }

    /// A payload (a PING followed by PADDING) large enough that a short-header
    /// packet built from it satisfies the header-protection sampling requirement
    /// (RFC 9001 §5.4.2: the sample is 16 bytes at packet-number offset + 4, so a
    /// packet must be large enough to contain it — tiny short-header packets, with
    /// their 1-byte first byte + connection ID, cannot).
    fn short_payload() -> Vec<u8> {
        let mut v = vec![0x01u8]; // PING
        v.resize(24, 0); // PADDING to 24 bytes
        v
    }

    #[test]
    fn empty_builder_reports_empty_and_full_budget() {
        let b = DatagramBuilder::new(1200);
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
        assert_eq!(b.remaining(), 1200);
        assert_eq!(b.max_len(), 1200);
        assert!(!b.is_sealed());
        assert!(!b.carries_initial());
        assert_eq!(b.initial_padding_shortfall(), 0);
        assert!(b.into_bytes().is_empty());
    }

    #[test]
    fn single_initial_packet_is_coalesced() {
        let k = keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut b = DatagramBuilder::new(1200);
        let appended = b.push(&k.client, &header, 0, None, &hex("06004022 0102")).expect("push");
        assert!(appended);
        assert!(!b.is_empty());
        assert!(b.carries_initial());
        assert!(!b.is_sealed());
        assert_eq!(b.len(), b.as_bytes().len());
    }

    #[test]
    fn long_header_packets_coalesce_and_datagram_matches_concatenation() {
        let k = keys();
        let dcid = a2_dcid();
        let scid = hex("c295a3b1");
        let initial = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &scid, token: &[] };
        let handshake = ProtectedHeader::Handshake { version: 1, dcid: &dcid, scid: &scid };

        // Encrypt the two packets independently to compare against the coalesced
        // datagram: coalescing is exactly concatenation of the encrypted bytes.
        let p0 = packet_crypt::encrypt_packet(&k.client, &initial, 0, None, &hex("06004022 aa"))
            .expect("encrypt");
        let p1 = packet_crypt::encrypt_packet(&k.client, &handshake, 0, None, &hex("06004010 bb"))
            .expect("encrypt");

        let mut b = DatagramBuilder::new(1200);
        assert!(b.push(&k.client, &initial, 0, None, &hex("06004022 aa")).expect("push"));
        assert!(b.push(&k.client, &handshake, 0, None, &hex("06004010 bb")).expect("push"));
        assert!(!b.is_sealed());

        let mut expected = p0.clone();
        expected.extend_from_slice(&p1);
        assert_eq!(b.into_bytes(), expected);
    }

    #[test]
    fn short_header_seals_the_datagram() {
        let k = keys();
        let dcid = hex("1122334455667788");
        let header = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &dcid };
        let mut b = DatagramBuilder::new(1200);
        assert!(b.push(&k.server, &header, 5, None, &short_payload()).expect("push"));
        assert!(b.is_sealed());
    }

    #[test]
    fn pushing_after_a_short_header_is_rejected() {
        let k = keys();
        let dcid = hex("1122334455667788");
        let short = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &dcid };
        let mut b = DatagramBuilder::new(1200);
        b.push(&k.server, &short, 5, None, &short_payload()).expect("first push");
        let err = b.push(&k.server, &short, 6, None, &short_payload()).expect_err("must reject");
        assert_eq!(err, DatagramBuildError::Sealed);
    }

    #[test]
    fn short_before_long_seals_and_bars_the_long() {
        let k = keys();
        let cid = a2_dcid();
        let short = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &cid };
        let handshake = ProtectedHeader::Handshake { version: 1, dcid: &cid, scid: &[] };
        let mut b = DatagramBuilder::new(1200);
        b.push(&k.client, &short, 1, None, &short_payload()).expect("short push");
        let err = b.push(&k.client, &handshake, 0, None, &hex("06004010 cc")).expect_err("barred");
        assert_eq!(err, DatagramBuildError::Sealed);
    }

    #[test]
    fn a_packet_that_would_overflow_is_refused_without_mutation() {
        let k = keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        // A budget too small for any packet.
        let mut b = DatagramBuilder::new(4);
        let appended = b.push(&k.client, &header, 0, None, &hex("06004022 0102")).expect("push");
        assert!(!appended);
        assert!(b.is_empty());
        assert!(!b.carries_initial());
        assert_eq!(b.remaining(), 4);
    }

    #[test]
    fn second_packet_overflowing_leaves_the_first_intact() {
        let k = keys();
        let dcid = a2_dcid();
        let scid = hex("c295a3b1");
        let initial = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &scid, token: &[] };
        let handshake = ProtectedHeader::Handshake { version: 1, dcid: &dcid, scid: &scid };

        let p0 = packet_crypt::encrypt_packet(&k.client, &initial, 0, None, &hex("06004022 aa"))
            .expect("encrypt");

        // Budget fits the first packet but not a second.
        let mut b = DatagramBuilder::new(p0.len() + 5);
        assert!(b.push(&k.client, &initial, 0, None, &hex("06004022 aa")).expect("push"));
        let appended = b.push(&k.client, &handshake, 0, None, &hex("06004010 bb")).expect("push");
        assert!(!appended);
        // The first packet is untouched.
        assert_eq!(b.into_bytes(), p0);
    }

    #[test]
    fn initial_shortfall_reports_bytes_to_min_datagram() {
        let k = keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut b = DatagramBuilder::new(MIN_INITIAL_DATAGRAM_LEN);
        b.push(&k.client, &header, 0, None, &hex("06004022 0102")).expect("push");
        let shortfall = b.initial_padding_shortfall();
        assert_eq!(shortfall, MIN_INITIAL_DATAGRAM_LEN - b.len());
        assert!(shortfall > 0);
    }

    #[test]
    fn padding_the_payload_by_the_shortfall_reaches_the_min_datagram() {
        // Mirrors the intended caller flow: read the shortfall, add that many
        // PADDING (0x00) bytes to the payload, re-encrypt, and the datagram now
        // meets MIN_INITIAL_DATAGRAM_LEN. A PADDING frame is one 0x00 byte, so the
        // datagram grows 1:1 with the payload padding. The base payload is already
        // large enough that the packet's Length is a two-byte varint (≥ 64), so it
        // keeps that width across the padding — as a real client Initial (which is
        // always padded toward 1200) does.
        let k = keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let base_payload = {
            let mut v = hex("06004022 0102"); // a CRYPTO frame stub
            v.resize(80, 0); // PADDING, so Length is already a two-byte varint
            v
        };

        let mut probe = DatagramBuilder::new(MIN_INITIAL_DATAGRAM_LEN);
        probe.push(&k.client, &header, 0, None, &base_payload).expect("push");
        let shortfall = probe.initial_padding_shortfall();

        let mut padded = base_payload.clone();
        padded.extend(std::iter::repeat_n(0u8, shortfall));

        let mut b = DatagramBuilder::new(MIN_INITIAL_DATAGRAM_LEN);
        assert!(b.push(&k.client, &header, 0, None, &padded).expect("push"));
        assert_eq!(b.len(), MIN_INITIAL_DATAGRAM_LEN);
        assert_eq!(b.initial_padding_shortfall(), 0);
    }

    #[test]
    fn non_initial_datagram_has_no_shortfall() {
        let k = keys();
        let dcid = a2_dcid();
        let scid = hex("0a0b0c0d");
        let header = ProtectedHeader::Handshake { version: 1, dcid: &dcid, scid: &scid };
        let mut b = DatagramBuilder::new(1200);
        b.push(&k.client, &header, 0, None, &hex("06004010 bb")).expect("push");
        assert!(!b.carries_initial());
        assert_eq!(b.initial_padding_shortfall(), 0);
    }

    #[test]
    fn push_encrypted_matches_push() {
        let k = keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let payload = hex("06004022 0102");
        let packet = packet_crypt::encrypt_packet(&k.client, &header, 0, None, &payload)
            .expect("encrypt");

        let mut via_push = DatagramBuilder::new(1200);
        via_push.push(&k.client, &header, 0, None, &payload).expect("push");

        let mut via_encrypted = DatagramBuilder::new(1200);
        assert!(via_encrypted.push_encrypted(&packet, true, true).expect("push_encrypted"));

        assert_eq!(via_push.into_bytes(), via_encrypted.clone().into_bytes());
        assert!(via_encrypted.carries_initial());
    }

    #[test]
    fn push_encrypted_short_header_seals() {
        let mut b = DatagramBuilder::new(1200);
        // Any bytes stand in for a short-header packet here; the flag drives sealing.
        assert!(b.push_encrypted(&[0x40, 0xaa, 0xbb], false, false).expect("push"));
        assert!(b.is_sealed());
        let err = b.push_encrypted(&[0x00], true, false).expect_err("sealed");
        assert_eq!(err, DatagramBuildError::Sealed);
    }

    #[test]
    fn coalesced_datagram_round_trips_through_parse_datagram() {
        // The datagram this builder produces must parse back into its packets with
        // the receive-side splitter, confirming the coalescing is on-wire valid.
        let k = keys();
        let dcid = a2_dcid();
        let scid = hex("c295a3b1");
        let initial = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &scid, token: &[] };
        let short = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &dcid };

        let mut b = DatagramBuilder::new(1200);
        b.push(&k.client, &initial, 0, None, &hex("06004022 aa")).expect("push initial");
        b.push(&k.client, &short, 0, None, &short_payload()).expect("push short");
        let datagram = b.into_bytes();

        // local_cid_len = 8 to delimit the short-header Destination Connection ID.
        let packets = crate::h3::datagram::parse_datagram(&datagram, dcid.len()).expect("parse");
        assert_eq!(packets.len(), 2);
        assert!(matches!(packets[0], crate::h3::packet::Packet::Initial { .. }));
        assert!(matches!(packets[1], crate::h3::packet::Packet::Short { .. }));
    }
}
