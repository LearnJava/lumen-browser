//! QUIC datagram coalescing — RFC 9000 §12.2, §14.1.
//!
//! A single UDP datagram may carry more than one QUIC packet: a sender can
//! *coalesce* several packets — typically an Initial followed by a Handshake,
//! or a Handshake followed by a 1-RTT packet — into one datagram to cut round
//! trips (RFC 9000 §12.2). This module is the pure layer above the per-packet
//! header codec [`packet`](super::packet): it splits a received datagram into
//! its ordered sequence of packets and assembles a sequence of packets back
//! into one datagram. It performs no IO, no packet protection, and no
//! packet-number handling — each packet's protected region stays opaque, exactly
//! as [`Packet`] carries it.
//!
//! ## Coalescing rules (RFC 9000 §12.2)
//!
//! Only the long-header Initial / 0-RTT / Handshake packets carry an explicit
//! `Length` field, so only they can be followed by another coalesced packet.
//! A short-header (1-RTT) packet has no length and runs to the end of the
//! datagram, so it can appear only as the **last** packet; the same holds for
//! Retry and Version Negotiation, which also extend to the datagram end. On the
//! parse side this falls out for free — [`Packet::parse`] consumes the whole
//! remaining buffer for those forms, so the loop terminates. On the encode side
//! [`encode_datagram`] enforces it, rejecting a non-length-delimited packet in
//! any but the final position ([`DatagramError::UnterminatedCoalescing`]).
//!
//! ## Initial datagram expansion (RFC 9000 §14.1)
//!
//! A client MUST expand every UDP datagram that carries an Initial packet to at
//! least [`MIN_INITIAL_DATAGRAM_LEN`] bytes, to give a server a lower bound on
//! the amplification it may perform before validating the client's address. The
//! padding is added as PADDING frames *inside* an Initial packet's payload (or
//! by coalescing more packets), before AEAD encryption — never as loose bytes
//! after the last packet, since every byte of a datagram belongs to a QUIC
//! packet. Because this codec sees only the already-encrypted `protected`
//! regions it cannot add that padding itself; [`initial_padding_shortfall`]
//! reports how many bytes short a candidate datagram is so the frame-assembly
//! layer can inject the PADDING before encryption.
//!
//! ## Out of scope (deferred to later slices)
//!
//! - The actual UDP send/receive and datagram-size (Path MTU) discovery.
//! - Discarding a coalesced packet a receiver cannot process while keeping the
//!   rest of the datagram (RFC 9000 §12.2); this layer parses every packet.
//! - Rejecting coalesced packets that carry a different Destination Connection
//!   ID (RFC 9000 §12.2) — a connection-layer policy, not a codec concern.

use super::packet::{Packet, PacketError};

/// Minimum size in bytes of a UDP datagram that carries an Initial packet
/// (RFC 9000 §14.1). A client pads to at least this so a server may send up to
/// three times as much before validating the client's address (RFC 9000 §8.1).
pub const MIN_INITIAL_DATAGRAM_LEN: usize = 1200;

/// Error splitting or assembling a coalesced QUIC datagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatagramError {
    /// A contained packet header was malformed. Carries the underlying
    /// [`PacketError`] from [`Packet::parse`] / [`Packet::encode`].
    Packet(PacketError),
    /// A non-length-delimited packet (short header, Retry, or Version
    /// Negotiation) appeared before the end of the datagram on encode; RFC 9000
    /// §12.2 permits such a packet only as the last one, because it carries no
    /// `Length` and so runs to the datagram's end.
    UnterminatedCoalescing,
    /// A packet header parsed to a zero-byte consumed count, which would not
    /// advance the cursor. Defensive: a well-formed header always consumes at
    /// least its first byte, so this signals malformed input rather than looping
    /// forever.
    ZeroLengthPacket,
}

impl core::fmt::Display for DatagramError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Packet(e) => write!(f, "QUIC datagram: {e}"),
            Self::UnterminatedCoalescing => {
                write!(f, "QUIC datagram: a non-length-delimited packet must be last")
            }
            Self::ZeroLengthPacket => write!(f, "QUIC datagram: packet consumed zero bytes"),
        }
    }
}

impl std::error::Error for DatagramError {}

impl From<PacketError> for DatagramError {
    fn from(e: PacketError) -> Self {
        Self::Packet(e)
    }
}

/// Split a received UDP datagram into its ordered sequence of coalesced packets
/// (RFC 9000 §12.2).
///
/// Each packet is parsed with [`Packet::parse`]; the length-delimited long-header
/// forms (Initial / 0-RTT / Handshake) stop at their `Length` field and may be
/// followed by another packet, while Retry, Version Negotiation, and the
/// short-header form consume the datagram's tail and therefore end the sequence.
/// `local_cid_len` is the length of the connection IDs this endpoint issued and
/// is forwarded to [`Packet::parse`] to delimit a short-header Destination
/// Connection ID (whose length is not on the wire).
///
/// An empty datagram yields an empty vector.
///
/// # Errors
///
/// [`DatagramError::Packet`] if any contained header is truncated or malformed,
/// or [`DatagramError::ZeroLengthPacket`] if a parse fails to advance the cursor.
pub fn parse_datagram(buf: &[u8], local_cid_len: usize) -> Result<Vec<Packet>, DatagramError> {
    let mut packets = Vec::new();
    let mut rem = buf;
    while !rem.is_empty() {
        let (packet, consumed) = Packet::parse(rem, local_cid_len)?;
        if consumed == 0 {
            return Err(DatagramError::ZeroLengthPacket);
        }
        packets.push(packet);
        rem = &rem[consumed..];
    }
    Ok(packets)
}

/// Assemble an ordered sequence of packets into one UDP datagram payload
/// (RFC 9000 §12.2).
///
/// Every packet except the last must be length-delimited (Initial / 0-RTT /
/// Handshake), since a short-header, Retry, or Version Negotiation packet has no
/// `Length` field and would swallow whatever follows it in the datagram. An
/// empty slice yields an empty datagram.
///
/// This does not pad the datagram: RFC 9000 §14.1 Initial expansion is applied
/// inside the packet payloads before encryption (see [`initial_padding_shortfall`]),
/// which this codec never touches.
///
/// # Errors
///
/// [`DatagramError::UnterminatedCoalescing`] if a non-length-delimited packet
/// appears before the final position, or [`DatagramError::Packet`] if a packet
/// fails to encode.
pub fn encode_datagram(packets: &[Packet]) -> Result<Vec<u8>, DatagramError> {
    let mut out = Vec::new();
    for (idx, packet) in packets.iter().enumerate() {
        let is_last = idx + 1 == packets.len();
        if !is_last && !packet.is_length_delimited() {
            return Err(DatagramError::UnterminatedCoalescing);
        }
        packet.encode(&mut out)?;
    }
    Ok(out)
}

/// How many bytes short of [`MIN_INITIAL_DATAGRAM_LEN`] a candidate datagram
/// carrying an Initial packet is (RFC 9000 §14.1).
///
/// Returns `0` once the datagram meets the minimum. The frame-assembly layer
/// adds this many bytes of PADDING frames inside an Initial packet's payload
/// before encryption; it must never be appended as loose bytes after the last
/// packet, since a receiver parses every datagram byte as part of a QUIC packet.
#[must_use]
pub const fn initial_padding_shortfall(datagram_len: usize) -> usize {
    MIN_INITIAL_DATAGRAM_LEN.saturating_sub(datagram_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A length-delimited Initial packet with the given protected region.
    fn initial(protected: Vec<u8>) -> Packet {
        Packet::Initial {
            version: 1,
            dcid: vec![0xaa, 0xbb],
            scid: vec![0xcc],
            reserved_and_pn_bits: 0,
            token: Vec::new(),
            protected,
        }
    }

    /// A length-delimited Handshake packet with the given protected region.
    fn handshake(protected: Vec<u8>) -> Packet {
        Packet::Handshake {
            version: 1,
            dcid: vec![0xaa, 0xbb],
            scid: vec![0xcc],
            reserved_and_pn_bits: 0,
            protected,
        }
    }

    /// A terminal short-header packet with the given DCID and protected region.
    fn short(dcid: Vec<u8>, protected: Vec<u8>) -> Packet {
        Packet::Short { spin: false, protected_bits: 0, dcid, protected }
    }

    #[test]
    fn empty_datagram_roundtrips() {
        assert_eq!(parse_datagram(&[], 0).unwrap(), Vec::<Packet>::new());
        assert_eq!(encode_datagram(&[]).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn single_packet_roundtrips() {
        let packets = vec![initial(vec![0x01, 0x02, 0x03, 0x04])];
        let bytes = encode_datagram(&packets).unwrap();
        assert_eq!(parse_datagram(&bytes, 0).unwrap(), packets);
    }

    #[test]
    fn initial_then_handshake_coalesced() {
        // The canonical second-flight datagram: an Initial coalesced with a
        // Handshake packet. Both are length-delimited so both must survive the
        // round trip in order.
        let packets = vec![initial(vec![0xaa; 5]), handshake(vec![0xbb; 7])];
        let bytes = encode_datagram(&packets).unwrap();
        assert_eq!(parse_datagram(&bytes, 0).unwrap(), packets);
    }

    #[test]
    fn long_then_short_coalesced() {
        // A Handshake coalesced with a 1-RTT short-header packet: the short
        // header runs to the end and must be last. DCID length is known out of
        // band (here 4 bytes).
        let dcid = vec![0x11, 0x22, 0x33, 0x44];
        let packets = vec![handshake(vec![0x01; 3]), short(dcid.clone(), vec![0x02; 9])];
        let bytes = encode_datagram(&packets).unwrap();
        assert_eq!(parse_datagram(&bytes, dcid.len()).unwrap(), packets);
    }

    #[test]
    fn short_header_must_be_last_on_encode() {
        // A short-header packet ahead of another packet has no Length to delimit
        // it, so encoding must reject the sequence.
        let packets = vec![short(vec![0x11, 0x22], vec![0x02; 4]), handshake(vec![0xbb; 3])];
        assert_eq!(encode_datagram(&packets), Err(DatagramError::UnterminatedCoalescing));
    }

    #[test]
    fn short_header_last_is_allowed() {
        // The same short-header packet in the final position is fine.
        let packets = vec![short(vec![0x11, 0x22], vec![0x02; 4])];
        assert!(encode_datagram(&packets).is_ok());
    }

    #[test]
    fn truncated_datagram_is_an_error() {
        // A length-delimited Initial whose Length claims more than the buffer
        // holds: the tail parse fails rather than silently truncating.
        let mut bytes = encode_datagram(&[initial(vec![0x01, 0x02, 0x03, 0x04])]).unwrap();
        bytes.pop(); // drop the last protected byte, contradicting the Length
        assert!(matches!(parse_datagram(&bytes, 0), Err(DatagramError::Packet(_))));
    }

    #[test]
    fn parse_preserves_trailing_coalesced_packet_order() {
        // Three coalesced length-delimited packets keep their order.
        let packets =
            vec![initial(vec![0x0a]), handshake(vec![0x0b, 0x0c]), handshake(vec![0x0d])];
        let bytes = encode_datagram(&packets).unwrap();
        let parsed = parse_datagram(&bytes, 0).unwrap();
        assert_eq!(parsed, packets);
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn initial_padding_shortfall_reports_deficit() {
        assert_eq!(initial_padding_shortfall(0), MIN_INITIAL_DATAGRAM_LEN);
        assert_eq!(initial_padding_shortfall(200), MIN_INITIAL_DATAGRAM_LEN - 200);
        assert_eq!(initial_padding_shortfall(MIN_INITIAL_DATAGRAM_LEN), 0);
        assert_eq!(initial_padding_shortfall(MIN_INITIAL_DATAGRAM_LEN + 100), 0);
    }

    #[test]
    fn datagram_error_display_is_nonempty() {
        assert!(!DatagramError::UnterminatedCoalescing.to_string().is_empty());
        assert!(!DatagramError::ZeroLengthPacket.to_string().is_empty());
        assert!(!DatagramError::Packet(PacketError::UnexpectedEof).to_string().is_empty());
    }
}
