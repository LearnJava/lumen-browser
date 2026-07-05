//! QUIC packet protection pipeline (RFC 9001 §5.3, §5.4).
//!
//! The connection layer performs two end-to-end operations on every
//! 1-RTT-bearing packet, and this module is the single place that ties the four
//! lower slices together to perform them:
//!
//! - [`encrypt_packet`] turns a plaintext QUIC payload (a run of transport
//!   frames) into an on-wire packet: it assembles the header with the header
//!   codec [`packet`], places the clear packet number with the truncation codec
//!   [`packet_number`], AEAD-seals the payload and applies header protection with
//!   [`packet_protect`], all keyed by a [`key_schedule::PacketProtectionKeys`].
//! - [`decrypt_packet`] is the exact inverse for a received datagram: it parses
//!   the header, removes header protection, reconstructs the full 62-bit packet
//!   number, and AEAD-opens the payload, returning the plaintext and the packet
//!   number for the loss-recovery and frame-parsing layers.
//!
//! Only the AEAD suite (AES-128-GCM header protection with AES-128) is wired,
//! matching [`packet_protect`]; ChaCha20 (RFC 9001 §5.4.4) is deferred. The
//! module is pure — it operates on byte buffers and a supplied key set, with no
//! IO and no connection state. Its inverse round-trip is validated against the
//! primitives already checked against the RFC 9001 Appendix A.2/A.3 vectors, and
//! the in-the-clear header layout is checked byte-for-byte against the RFC 9001
//! Appendix A.2 client Initial header.

use super::key_schedule::PacketProtectionKeys;
use super::packet::{Packet, PacketError};
use super::packet_number::{self, PacketNumberError};
use super::packet_protect::{self, AEAD_TAG_LEN, ProtectionError};

/// The Key Phase bit of a short-header first byte (RFC 9000 §17.3.1). It is
/// header-protected on the wire, so [`ProtectedHeader::Short`] carries the
/// caller's intended value and [`encrypt_packet`] folds it into the protected
/// low bits before protection is applied.
const SHORT_KEY_PHASE_BIT: u8 = 0x04;

/// Error from the packet protection pipeline. Each variant wraps the failing
/// lower-slice error, or reports a caller misuse (a non-encrypted packet form or
/// an out-of-range packet-number width).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PacketCryptError {
    /// The header codec [`packet`] failed to parse or encode the packet header.
    Header(PacketError),
    /// The packet-number width could not be chosen for the send (the gap to the
    /// largest acknowledged number is too large to truncate, RFC 9000 §17.1).
    PacketNumber(PacketNumberError),
    /// An AEAD or header-protection transform failed — on receive this is an
    /// authentication failure or a truncated packet.
    Protection(ProtectionError),
    /// A packet form that carries no AEAD-protected payload (Retry or Version
    /// Negotiation, RFC 9000 §17.2.1/§17.2.5) was passed to the pipeline.
    NotProtected,
    /// An explicit packet-number width outside the `1..=4` bytes a QUIC packet
    /// number may occupy (RFC 9000 §17.1) was requested. Carries the width.
    InvalidPacketNumberLength(usize),
}

impl core::fmt::Display for PacketCryptError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Header(e) => write!(f, "QUIC packet crypt: {e}"),
            Self::PacketNumber(e) => write!(f, "QUIC packet crypt: {e}"),
            Self::Protection(e) => write!(f, "QUIC packet crypt: {e}"),
            Self::NotProtected => {
                write!(f, "QUIC packet crypt: packet form carries no protected payload")
            }
            Self::InvalidPacketNumberLength(n) => {
                write!(f, "QUIC packet crypt: packet-number width {n} is not in 1..=4")
            }
        }
    }
}

impl std::error::Error for PacketCryptError {}

impl From<PacketError> for PacketCryptError {
    fn from(e: PacketError) -> Self {
        Self::Header(e)
    }
}

impl From<PacketNumberError> for PacketCryptError {
    fn from(e: PacketNumberError) -> Self {
        Self::PacketNumber(e)
    }
}

impl From<ProtectionError> for PacketCryptError {
    fn from(e: ProtectionError) -> Self {
        Self::Protection(e)
    }
}

/// The header fields of a packet to encrypt, borrowing the connection IDs and
/// token so the caller keeps ownership. This is the plaintext counterpart of the
/// AEAD-bearing [`Packet`] variants: the first byte's Reserved and Packet Number
/// Length bits (and, for [`ProtectedHeader::Short`], the Key Phase) are filled in
/// by [`encrypt_packet`], not the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectedHeader<'a> {
    /// Initial packet (RFC 9000 §17.2.2) with its address-validation Token.
    Initial {
        /// QUIC version (`1` for QUIC v1).
        version: u32,
        /// Destination Connection ID.
        dcid: &'a [u8],
        /// Source Connection ID.
        scid: &'a [u8],
        /// The address-validation Token (empty when none).
        token: &'a [u8],
    },
    /// 0-RTT packet (RFC 9000 §17.2.3).
    ZeroRtt {
        /// QUIC version.
        version: u32,
        /// Destination Connection ID.
        dcid: &'a [u8],
        /// Source Connection ID.
        scid: &'a [u8],
    },
    /// Handshake packet (RFC 9000 §17.2.4).
    Handshake {
        /// QUIC version.
        version: u32,
        /// Destination Connection ID.
        dcid: &'a [u8],
        /// Source Connection ID.
        scid: &'a [u8],
    },
    /// Short-header (1-RTT) packet (RFC 9000 §17.3.1).
    Short {
        /// The Latency Spin Bit.
        spin: bool,
        /// The Key Phase bit selecting the current key generation.
        key_phase: bool,
        /// Destination Connection ID (its length known out of band).
        dcid: &'a [u8],
    },
}

impl ProtectedHeader<'_> {
    /// `true` for the long-header forms, which protect the low four bits of the
    /// first byte; `false` for the short header, which protects five.
    const fn is_long_header(&self) -> bool {
        !matches!(self, ProtectedHeader::Short { .. })
    }

    /// Build the scaffold [`Packet`] this header describes: the header fields
    /// verbatim, the first byte's protected low bits set from `pn_bits` (plus the
    /// Key Phase for a short header), and `protected` as the reserved region the
    /// caller will overwrite with the packet number and sealed payload.
    fn build_scaffold(&self, pn_bits: u8, protected: Vec<u8>) -> Packet {
        match *self {
            ProtectedHeader::Initial { version, dcid, scid, token } => Packet::Initial {
                version,
                dcid: dcid.to_vec(),
                scid: scid.to_vec(),
                reserved_and_pn_bits: pn_bits,
                token: token.to_vec(),
                protected,
            },
            ProtectedHeader::ZeroRtt { version, dcid, scid } => Packet::ZeroRtt {
                version,
                dcid: dcid.to_vec(),
                scid: scid.to_vec(),
                reserved_and_pn_bits: pn_bits,
                protected,
            },
            ProtectedHeader::Handshake { version, dcid, scid } => Packet::Handshake {
                version,
                dcid: dcid.to_vec(),
                scid: scid.to_vec(),
                reserved_and_pn_bits: pn_bits,
                protected,
            },
            ProtectedHeader::Short { spin, key_phase, dcid } => Packet::Short {
                spin,
                protected_bits: pn_bits | if key_phase { SHORT_KEY_PHASE_BIT } else { 0 },
                dcid: dcid.to_vec(),
                protected,
            },
        }
    }
}

/// A decrypted packet: the parsed header, the recovered full packet number, the
/// plaintext payload, and the number of datagram bytes the packet consumed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecryptedPacket {
    /// The parsed header. Its in-the-clear fields (version, connection IDs,
    /// token) are meaningful; the first-byte protected bits and `protected`
    /// region are the verbatim on-wire (still-protected) bytes the header codec
    /// preserved, since protection is removed on the working copy, not here.
    pub header: Packet,
    /// The reconstructed full 62-bit packet number (RFC 9000 §17.1).
    pub packet_number: u64,
    /// The AEAD-decrypted payload: the run of QUIC transport frames.
    pub payload: Vec<u8>,
    /// Bytes consumed from the input, so a caller decoding a coalesced datagram
    /// (RFC 9000 §12.2) can advance to the next packet.
    pub consumed: usize,
}

/// Encrypt `payload` into an on-wire QUIC packet, choosing the packet-number
/// width from `largest_acked` (RFC 9000 §17.1, Appendix A.2).
///
/// This is the `encrypt` half of the pipeline: it assembles the header, places
/// the clear packet number, AEAD-seals `payload` with the unprotected header as
/// associated data, and applies header protection, returning the complete packet
/// bytes ready to place in a datagram.
///
/// # Errors
///
/// [`PacketCryptError::PacketNumber`] if the gap to `largest_acked` is too large
/// to truncate, [`PacketCryptError::Header`] if a header field is out of range,
/// or [`PacketCryptError::Protection`] if a key length is wrong.
pub fn encrypt_packet(
    keys: &PacketProtectionKeys,
    header: &ProtectedHeader<'_>,
    packet_number: u64,
    largest_acked: Option<u64>,
    payload: &[u8],
) -> Result<Vec<u8>, PacketCryptError> {
    let pn_length = packet_number::packet_number_length(packet_number, largest_acked)?;
    encrypt_packet_with_pn_length(keys, header, packet_number, pn_length, payload)
}

/// Encrypt `payload` with an explicit packet-number width, otherwise identical to
/// [`encrypt_packet`].
///
/// [`encrypt_packet`] picks the minimal width a receiver can disambiguate; this
/// entry point lets a caller fix the width (`1..=4`), for example to reproduce a
/// fixed-width test vector. The low `pn_length` bytes of `packet_number` are
/// written, so an over-narrow width silently truncates (the caller owns the
/// disambiguation contract, RFC 9000 §17.1).
///
/// # Errors
///
/// [`PacketCryptError::InvalidPacketNumberLength`] if `pn_length` is not in
/// `1..=4`; otherwise as [`encrypt_packet`].
pub fn encrypt_packet_with_pn_length(
    keys: &PacketProtectionKeys,
    header: &ProtectedHeader<'_>,
    packet_number: u64,
    pn_length: usize,
    payload: &[u8],
) -> Result<Vec<u8>, PacketCryptError> {
    if !(1..=4).contains(&pn_length) {
        return Err(PacketCryptError::InvalidPacketNumberLength(pn_length));
    }

    // The protected region is the packet number followed by the AEAD output:
    // the ciphertext (same length as the plaintext) plus the 16-byte tag.
    let protected_len = pn_length + payload.len() + AEAD_TAG_LEN;
    let pn_bits = packet_number::encode_pn_length_bits(pn_length);
    let long_header = header.is_long_header();

    // Encode the header (and reserve the protected region) through the header
    // codec, so the Length field and every in-the-clear field are exactly what
    // `packet` would parse back.
    let scaffold = header.build_scaffold(pn_bits, vec![0u8; protected_len]);
    let mut bytes = Vec::new();
    scaffold.encode(&mut bytes)?;

    // The protected region is the tail of the encoding; the packet number begins
    // it. Overwrite the reserved leading bytes with the clear packet number.
    let pn_offset = bytes.len() - protected_len;
    let be = packet_number.to_be_bytes();
    bytes[pn_offset..pn_offset + pn_length].copy_from_slice(&be[be.len() - pn_length..]);

    // AEAD-seal the payload with the unprotected header (through the packet
    // number) as associated data, then replace the reserved payload region.
    let sealed = {
        let aad = &bytes[..pn_offset + pn_length];
        packet_protect::aes_128_gcm_seal(&keys.key, &keys.iv, packet_number, aad, payload)?
    };
    bytes.truncate(pn_offset + pn_length);
    bytes.extend_from_slice(&sealed);

    // Finally protect the header (first-byte low bits + packet number).
    packet_protect::apply_header_protection(&mut bytes, pn_offset, &keys.hp, long_header)?;
    Ok(bytes)
}

/// Decrypt the first packet at the front of `buf`, the inverse of
/// [`encrypt_packet`].
///
/// `local_cid_len` is the length of the connection IDs this endpoint issued (it
/// delimits a short-header Destination Connection ID, RFC 9000 §17.3.1);
/// `largest_pn` is the largest packet number already processed in this packet's
/// number space, used to reconstruct the truncated number (RFC 9000 §17.1,
/// Appendix A.3). Header protection is removed and the payload opened on a
/// working copy; `buf` is not modified. For a coalesced datagram, advance by
/// [`DecryptedPacket::consumed`] and call again.
///
/// # Errors
///
/// [`PacketCryptError::Header`] if the header is malformed,
/// [`PacketCryptError::NotProtected`] for a Retry or Version Negotiation packet,
/// or [`PacketCryptError::Protection`] if header protection or AEAD
/// authentication fails.
pub fn decrypt_packet(
    keys: &PacketProtectionKeys,
    buf: &[u8],
    local_cid_len: usize,
    largest_pn: u64,
) -> Result<DecryptedPacket, PacketCryptError> {
    let (header, consumed) = Packet::parse(buf, local_cid_len)?;
    let (protected_len, long_header) = match &header {
        Packet::Initial { protected, .. }
        | Packet::ZeroRtt { protected, .. }
        | Packet::Handshake { protected, .. } => (protected.len(), true),
        Packet::Short { protected, .. } => (protected.len(), false),
        Packet::Retry { .. } | Packet::VersionNegotiation { .. } => {
            return Err(PacketCryptError::NotProtected);
        }
    };

    // The protected region is the tail of the consumed bytes; the packet number
    // begins it. Work on a copy so `buf` is untouched.
    let pn_offset = consumed - protected_len;
    let mut packet_bytes = buf[..consumed].to_vec();

    // Remove header protection to reveal the packet-number length and octets.
    let pn_length =
        packet_protect::remove_header_protection(&mut packet_bytes, pn_offset, &keys.hp, long_header)?;
    let truncated =
        packet_number::read_truncated_packet_number(&packet_bytes[pn_offset..pn_offset + pn_length]);
    let packet_number = packet_number::decode_packet_number(largest_pn, truncated, (pn_length * 8) as u32);

    // AEAD-open with the now-unprotected header (through the packet number) as
    // associated data.
    let payload = {
        let (aad, sealed) = packet_bytes.split_at(pn_offset + pn_length);
        packet_protect::aes_128_gcm_open(&keys.key, &keys.iv, packet_number, aad, sealed)?
    };

    Ok(DecryptedPacket { header, packet_number, payload, consumed })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn initial_keys() -> InitialKeys {
        InitialKeys::derive(&a2_dcid())
    }

    #[test]
    fn rfc9001_a2_client_initial_header_layout_is_byte_exact() {
        // Encrypt a 1162-byte payload with a fixed 4-byte packet number 2, as in
        // the RFC 9001 A.2 client Initial. The header fields and the Length are
        // in the clear (only the first-byte low bits and packet number are
        // protected), so the encoding from the version field through the Length
        // must equal the RFC's header byte-for-byte, regardless of payload
        // content. Length = pn(4) + payload(1162) + tag(16) = 1182 = 0x449e.
        let keys = initial_keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let out = encrypt_packet_with_pn_length(&keys.client, &header, 2, 4, &vec![0u8; 1162])
            .expect("encrypt");
        // Byte 0 is header-protected, so skip it; assert version..Length.
        assert_eq!(out[1..18], hex("00000001 08 8394c8f03e515708 00 00 449e")[..]);
    }

    #[test]
    fn initial_round_trip_recovers_payload_and_number() {
        let keys = initial_keys();
        let dcid = a2_dcid();
        let scid = hex("c295a3b1");
        let payload = hex("06004022 0102030405060708");
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &scid, token: &[] };
        let packet = encrypt_packet(&keys.client, &header, 42, Some(30), &payload).expect("encrypt");

        let got = decrypt_packet(&keys.client, &packet, 0, 41).expect("decrypt");
        assert_eq!(got.packet_number, 42);
        assert_eq!(got.payload, payload);
        assert_eq!(got.consumed, packet.len());
        match got.header {
            Packet::Initial { version, dcid: d, scid: s, token, .. } => {
                assert_eq!(version, 1);
                assert_eq!(d, dcid);
                assert_eq!(s, scid);
                assert!(token.is_empty());
            }
            other => panic!("expected Initial, got {other:?}"),
        }
    }

    #[test]
    fn zero_rtt_and_handshake_round_trip() {
        let keys = initial_keys();
        let dcid = a2_dcid();
        let scid = hex("0a0b0c0d");
        let payload = hex("08004010 deadbeef");

        for header in [
            ProtectedHeader::ZeroRtt { version: 1, dcid: &dcid, scid: &scid },
            ProtectedHeader::Handshake { version: 1, dcid: &dcid, scid: &scid },
        ] {
            let packet = encrypt_packet(&keys.client, &header, 7, None, &payload).expect("encrypt");
            let got = decrypt_packet(&keys.client, &packet, 0, 6).expect("decrypt");
            assert_eq!(got.packet_number, 7);
            assert_eq!(got.payload, payload);
        }
    }

    #[test]
    fn short_header_round_trip_preserves_spin_and_key_phase() {
        let keys = initial_keys();
        // A short-header Destination Connection ID of length 8; the receiver must
        // be told that length out of band.
        let dcid = hex("1122334455667788");
        let payload = hex("0100000000"); // PING + PADDING
        let header = ProtectedHeader::Short { spin: true, key_phase: true, dcid: &dcid };
        let packet = encrypt_packet(&keys.server, &header, 100, Some(90), &payload).expect("encrypt");

        let got = decrypt_packet(&keys.server, &packet, dcid.len(), 99).expect("decrypt");
        assert_eq!(got.packet_number, 100);
        assert_eq!(got.payload, payload);
        match got.header {
            Packet::Short { spin, dcid: d, .. } => {
                assert!(spin);
                assert_eq!(d, dcid);
            }
            other => panic!("expected Short, got {other:?}"),
        }
    }

    #[test]
    fn tampered_payload_fails_authentication() {
        let keys = initial_keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut packet = encrypt_packet(&keys.client, &header, 3, None, &hex("06004010 00112233"))
            .expect("encrypt");
        // Flip a byte deep in the ciphertext (past the header + packet number).
        let last = packet.len() - 1;
        packet[last] ^= 0x01;
        let err = decrypt_packet(&keys.client, &packet, 0, 2).expect_err("must fail");
        assert_eq!(err, PacketCryptError::Protection(ProtectionError::AeadFailed));
    }

    #[test]
    fn wrong_direction_keys_fail_authentication() {
        let keys = initial_keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        // Sealed by the client secret, opened with the server secret → AEAD fails.
        let packet = encrypt_packet(&keys.client, &header, 5, None, &hex("06004008 aabbccdd"))
            .expect("encrypt");
        let err = decrypt_packet(&keys.server, &packet, 0, 4).expect_err("must fail");
        assert_eq!(err, PacketCryptError::Protection(ProtectionError::AeadFailed));
    }

    #[test]
    fn retry_and_version_negotiation_are_not_protected() {
        let keys = initial_keys();
        // A Retry packet (RFC 9000 §17.2.5): first byte f0, version 1, empty
        // DCID, one-byte SCID, a token, and a 16-byte integrity tag.
        let retry = Packet::Retry {
            version: 1,
            dcid: vec![],
            scid: hex("aa"),
            unused_bits: 0,
            retry_token: hex("74657374"),
            integrity_tag: [0u8; 16],
        };
        let mut buf = Vec::new();
        retry.encode(&mut buf).expect("encode retry");
        let err = decrypt_packet(&keys.client, &buf, 0, 0).expect_err("retry has no payload");
        assert_eq!(err, PacketCryptError::NotProtected);

        let vn = Packet::VersionNegotiation {
            first_byte: 0x80,
            dcid: hex("0102"),
            scid: vec![],
            supported_versions: vec![1],
        };
        let mut buf = Vec::new();
        vn.encode(&mut buf).expect("encode vn");
        let err = decrypt_packet(&keys.client, &buf, 0, 0).expect_err("vn has no payload");
        assert_eq!(err, PacketCryptError::NotProtected);
    }

    #[test]
    fn explicit_pn_length_out_of_range_is_rejected() {
        let keys = initial_keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        for bad in [0usize, 5] {
            let err = encrypt_packet_with_pn_length(&keys.client, &header, 1, bad, &hex("00"))
                .expect_err("bad width");
            assert_eq!(err, PacketCryptError::InvalidPacketNumberLength(bad));
        }
    }

    #[test]
    fn packet_number_width_follows_largest_acked() {
        let keys = initial_keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        // A large gap to largest-acked forces a wider packet number; the
        // round-trip still recovers it exactly.
        let pn = 0x0100_0000;
        let packet = encrypt_packet(&keys.client, &header, pn, Some(0), &hex("06004004 00")).expect("encrypt");
        let got = decrypt_packet(&keys.client, &packet, 0, pn - 1).expect("decrypt");
        assert_eq!(got.packet_number, pn);
    }

    #[test]
    fn coalesced_initial_packets_decrypt_in_sequence() {
        let keys = initial_keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let first_payload = hex("06004008 11111111");
        let second_payload = hex("06004008 22222222");
        let mut datagram = encrypt_packet(&keys.client, &header, 0, None, &first_payload).expect("first");
        let second = encrypt_packet(&keys.client, &header, 1, None, &second_payload).expect("second");
        datagram.extend_from_slice(&second);

        let a = decrypt_packet(&keys.client, &datagram, 0, 0).expect("decrypt first");
        assert_eq!(a.packet_number, 0);
        assert_eq!(a.payload, first_payload);
        // The Length-delimited Initial leaves the coalesced packet behind.
        assert!(a.consumed < datagram.len());

        let b = decrypt_packet(&keys.client, &datagram[a.consumed..], 0, 0).expect("decrypt second");
        assert_eq!(b.packet_number, 1);
        assert_eq!(b.payload, second_payload);
    }

    #[test]
    fn header_protection_masks_first_byte() {
        // The protected packet's first byte must differ from the unprotected
        // long-header first byte for an Initial with a 1-byte packet number:
        // 0x80|0x40|0x00|0x00 = 0xc0. If header protection were skipped the byte
        // would still be 0xc0, so a difference proves it ran.
        let keys = initial_keys();
        let dcid = a2_dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let packet = encrypt_packet(&keys.client, &header, 0, None, &hex("06004008 abcdef01")).expect("encrypt");
        assert_ne!(packet[0], 0xc0, "header protection should have masked the first byte");
    }
}
