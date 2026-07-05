//! QUIC packet number encoding/decoding — RFC 9000 §17.1, Appendix A.
//!
//! A QUIC packet number is a full 62-bit integer, but only its least-significant
//! 1–4 bytes travel on the wire: a sender *truncates* it to the fewest bytes that
//! still let the receiver recover the full value from the largest packet number it
//! has already seen, and the receiver *decodes* that truncation back (RFC 9000
//! §17.1). This module is the pure arithmetic layer that performs both halves. It
//! sits between the packet header codec [`packet`](super::packet), which carries
//! the truncated packet number inside its opaque `protected` region and the
//! two-bit Packet Number Length inside the header-protected first byte, and the
//! loss-recovery layer ([`loss`](super::loss), [`recovery`](super::recovery)),
//! which reasons in full packet numbers.
//!
//! ## Why truncation
//!
//! Sending the full 62-bit number in every packet would waste header bytes on a
//! high-rate connection. Because a receiver already knows roughly where the
//! sender is — no farther ahead than the packets it has acknowledged plus one
//! congestion window — the sender only needs to send enough low bytes to
//! disambiguate the new number within that window, and the receiver reconstructs
//! the high bytes from the largest number it has processed. The header-protection
//! transform (RFC 9001 §5.4) hides both the length and the truncated bytes on the
//! wire, so this codec runs *after* header protection is removed (parse) and
//! *before* it is applied (encode).
//!
//! ## Encoding (Appendix A.2)
//!
//! The number of bits must exceed the base-2 logarithm of the number of
//! contiguous unacknowledged packet numbers (the new one included), so that the
//! truncation is unambiguous over that range. [`packet_number_length`] turns that
//! into the smallest byte count `b ∈ 1..=4` with `2^(8·b − 1) ≥ num_unacked`, and
//! [`encode_packet_number`] appends that many least-significant big-endian bytes.
//! The matching Packet Number Length bits (`b − 1`) go in the header's first byte
//! via [`encode_pn_length_bits`].
//!
//! ## Decoding (Appendix A.3)
//!
//! [`decode_packet_number`] takes the largest packet number processed in the same
//! packet-number space, the truncated value, and its width in bits, and returns
//! the full packet number nearest to `largest_pn + 1` — choosing the candidate
//! within the half-window on either side, with the overflow/underflow guards the
//! RFC pseudocode specifies. [`pn_length_from_first_byte`] and
//! [`read_truncated_packet_number`] recover the width and value the header codec
//! left opaque.
//!
//! ## Out of scope (deferred to later slices)
//!
//! - Header protection and AEAD packet protection ([`packet_protect`](super::packet_protect))
//!   already exist; wiring this codec into that decrypt/encrypt path (removing
//!   protection, reading the length bits, decoding the number) is the connection
//!   layer's job, not this pure codec's.
//! - Tracking which packet number is the largest processed per space — that is
//!   the receiver's ACK-generation state, a later slice; this codec is told the
//!   value.

/// The largest valid QUIC packet number: packet numbers are encoded as a
/// variable-length integer no larger than `2^62 − 1` (RFC 9000 §17.1, §12.3).
pub const MAX_PACKET_NUMBER: u64 = (1 << 62) - 1;

/// Mask selecting the two-bit Packet Number Length field in a packet's first
/// byte (RFC 9000 §17.2 / §17.3). The field holds the byte count minus one, so a
/// value of `0..=3` denotes a 1..=4 byte packet number. These bits are
/// header-protected on the wire (RFC 9001 §5.4); this codec operates on the
/// unprotected byte.
pub const PN_LENGTH_MASK: u8 = 0x03;

/// Error encoding a packet number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketNumberError {
    /// The distance between the packet number and the largest acknowledged one
    /// exceeds `2^31`, so no 1–4 byte truncation can encode it unambiguously
    /// (RFC 9000 §17.1). A sender must never let this many packet numbers go
    /// unacknowledged before it stops sending, so this indicates a caller bug
    /// rather than a reachable wire condition.
    Unrepresentable,
}

impl core::fmt::Display for PacketNumberError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unrepresentable => write!(
                f,
                "QUIC packet number: gap to largest-acked exceeds 2^31, cannot truncate"
            ),
        }
    }
}

impl std::error::Error for PacketNumberError {}

/// The minimum number of bytes (1..=4) needed to truncate `full_pn` so a receiver
/// can recover it, given the largest packet number it has acknowledged
/// (RFC 9000 §17.1, Appendix A.2).
///
/// `largest_acked` is `None` before any packet has been acknowledged, in which
/// case the whole range `0..=full_pn` is unacknowledged. Otherwise it must be
/// less than `full_pn` (a packet number is always larger than any already
/// acknowledged); a `largest_acked` at or above `full_pn` is treated as a
/// zero-length unacknowledged range and yields the 1-byte minimum.
///
/// The RFC requires the encoding to use more bits than the base-2 logarithm of
/// the count of contiguous unacknowledged packet numbers. Equivalently, this
/// returns the smallest `b ∈ 1..=4` with `2^(8·b − 1) ≥ num_unacked`.
///
/// # Errors
///
/// [`PacketNumberError::Unrepresentable`] if `num_unacked` exceeds `2^31`, the
/// most a 4-byte truncation can disambiguate.
pub fn packet_number_length(
    full_pn: u64,
    largest_acked: Option<u64>,
) -> Result<usize, PacketNumberError> {
    let num_unacked = match largest_acked {
        None => full_pn.saturating_add(1),
        Some(acked) => full_pn.saturating_sub(acked),
    };
    for b in 1..=4u32 {
        // 2^(8·b − 1): the exclusive upper bound a b-byte truncation disambiguates.
        if num_unacked <= (1u64 << (8 * b - 1)) {
            return Ok(b as usize);
        }
    }
    Err(PacketNumberError::Unrepresentable)
}

/// Append `full_pn` truncated to its minimal on-wire width (RFC 9000 §17.1,
/// Appendix A.2), most-significant byte first, to `out`.
///
/// The width is chosen by [`packet_number_length`] from `largest_acked`; the
/// matching two Packet Number Length bits for the header's first byte come from
/// [`encode_pn_length_bits`] applied to the returned byte count. Returns the
/// number of bytes appended (1..=4).
///
/// # Errors
///
/// [`PacketNumberError::Unrepresentable`] if the gap to `largest_acked` is too
/// large to truncate (see [`packet_number_length`]).
pub fn encode_packet_number(
    full_pn: u64,
    largest_acked: Option<u64>,
    out: &mut Vec<u8>,
) -> Result<usize, PacketNumberError> {
    let num_bytes = packet_number_length(full_pn, largest_acked)?;
    let be = full_pn.to_be_bytes();
    out.extend_from_slice(&be[be.len() - num_bytes..]);
    Ok(num_bytes)
}

/// Decode a truncated packet number back to its full 62-bit value
/// (RFC 9000 §17.1, Appendix A.3).
///
/// `largest_pn` is the largest packet number successfully processed in the same
/// packet-number space; `truncated_pn` is the value read from the packet
/// (`pn_nbits / 8` big-endian bytes, e.g. via [`read_truncated_packet_number`]);
/// `pn_nbits` is its width in bits (8, 16, 24, or 32, i.e. `8 ×` the length from
/// [`pn_length_from_first_byte`]).
///
/// Returns the candidate closest to `largest_pn + 1` that is congruent to
/// `truncated_pn` modulo `2^pn_nbits`, i.e. the value within the half-window on
/// either side of the expected number, applying the overflow/underflow guards
/// from the RFC pseudocode so the result stays a valid packet number.
#[must_use]
pub fn decode_packet_number(largest_pn: u64, truncated_pn: u64, pn_nbits: u32) -> u64 {
    let expected_pn = largest_pn.saturating_add(1);
    let pn_win = 1u64 << pn_nbits;
    let pn_hwin = pn_win / 2;
    let pn_mask = pn_win - 1;

    // Strip the low `pn_nbits` bits of the expected number and graft in the
    // received truncation. That candidate may land in the wrong window, so nudge
    // it by a full window if it falls outside the half-window around expected_pn.
    let candidate_pn = (expected_pn & !pn_mask) | truncated_pn;

    // Written as `candidate + hwin <= expected` to avoid underflowing the RFC's
    // `expected - hwin`; the sum stays well under u64::MAX (candidate < 2^62).
    if candidate_pn + pn_hwin <= expected_pn && candidate_pn < (1u64 << 62) - pn_win {
        return candidate_pn + pn_win;
    }
    if candidate_pn > expected_pn + pn_hwin && candidate_pn >= pn_win {
        return candidate_pn - pn_win;
    }
    candidate_pn
}

/// Read a big-endian truncated packet number from its on-wire bytes.
///
/// `bytes` is the 1..=4 packet-number octets from the header (their count is the
/// Packet Number Length; see [`pn_length_from_first_byte`]). The result is passed
/// to [`decode_packet_number`] together with `8 × bytes.len()` as the bit width.
/// A slice longer than 8 bytes would shift its most-significant octets out; the
/// caller supplies at most 4.
#[must_use]
pub fn read_truncated_packet_number(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
}

/// The two-bit Packet Number Length field value for a `num_bytes`-byte packet
/// number, to be OR-ed into a packet's (unprotected) first byte (RFC 9000
/// §17.2 / §17.3): the field carries the byte count minus one.
///
/// `num_bytes` is expected in `1..=4`; a value of `0` saturates to the 1-byte
/// encoding (field `0`), and only the low two bits are kept.
#[must_use]
pub const fn encode_pn_length_bits(num_bytes: usize) -> u8 {
    (num_bytes.saturating_sub(1) as u8) & PN_LENGTH_MASK
}

/// The packet-number byte count (1..=4) encoded in a packet's (unprotected) first
/// byte (RFC 9000 §17.2 / §17.3): the low two bits hold the byte count minus one.
///
/// These bits are header-protected on the wire (RFC 9001 §5.4), so `first_byte`
/// must already have had header protection removed.
#[must_use]
pub const fn pn_length_from_first_byte(first_byte: u8) -> usize {
    (first_byte & PN_LENGTH_MASK) as usize + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_grows_with_the_unacked_gap() {
        // With nothing acknowledged the range is 0..=full_pn.
        assert_eq!(packet_number_length(0, None).unwrap(), 1);
        assert_eq!(packet_number_length(127, None).unwrap(), 1); // num_unacked 128 ≤ 2^7
        assert_eq!(packet_number_length(128, None).unwrap(), 2); // num_unacked 129 > 2^7
        // The 2^(8b−1) boundaries: num_unacked exactly at a threshold still fits b.
        assert_eq!(packet_number_length(1 << 15, Some(0)).unwrap(), 2); // 32768 ≤ 2^15
        assert_eq!(packet_number_length((1 << 15) + 1, Some(0)).unwrap(), 3);
        assert_eq!(packet_number_length(1 << 23, Some(0)).unwrap(), 3); // 2^23 ≤ 2^23
        assert_eq!(packet_number_length((1 << 23) + 1, Some(0)).unwrap(), 4);
        assert_eq!(packet_number_length(1 << 31, Some(0)).unwrap(), 4); // 2^31 ≤ 2^31
    }

    #[test]
    fn length_rejects_an_oversized_gap() {
        assert_eq!(
            packet_number_length((1 << 31) + 1, Some(0)),
            Err(PacketNumberError::Unrepresentable)
        );
    }

    #[test]
    fn encode_matches_rfc_a2_example() {
        // RFC 9000 Appendix A.2: full 0xac5c02, largest acked 0xabe8b3 → 2 bytes,
        // truncated 0x5c02.
        let mut out = Vec::new();
        let n = encode_packet_number(0x00ac_5c02, Some(0x00ab_e8b3), &mut out).unwrap();
        assert_eq!(n, 2);
        assert_eq!(out, vec![0x5c, 0x02]);
    }

    #[test]
    fn encode_matches_rfc_a2_second_example() {
        // RFC 9000 §17.1: largest processed 0xa82f30ea, new 0xa82f9b32 → a 16-bit
        // packet number 0x9b32 is required (an 8-bit one would be ambiguous).
        let mut out = Vec::new();
        let n = encode_packet_number(0xa82f_9b32, Some(0xa82f_30ea), &mut out).unwrap();
        assert_eq!(n, 2);
        assert_eq!(out, vec![0x9b, 0x32]);
    }

    #[test]
    fn decode_matches_rfc_a3_example() {
        // RFC 9000 Appendix A.3: largest 0xa82f30ea, truncated 0x9b32 (16 bits)
        // decodes back to 0xa82f9b32.
        assert_eq!(decode_packet_number(0xa82f_30ea, 0x9b32, 16), 0xa82f_9b32);
    }

    #[test]
    fn decode_adds_a_window_when_truncation_wrapped_low() {
        // True pn 512 arrives while the largest processed is 399: its 8-bit
        // truncation is 0x00, below the expected low byte, so a full window is
        // added back.
        assert_eq!(decode_packet_number(399, 0x00, 8), 512);
    }

    #[test]
    fn decode_subtracts_a_window_when_truncation_wrapped_high() {
        // A reordered pn 250 arrives while the largest processed is 260: its 8-bit
        // truncation 0xfa sits above the expected low byte, so a window is removed.
        assert_eq!(decode_packet_number(260, 0xfa, 8), 250);
    }

    #[test]
    fn decode_returns_the_bare_candidate_within_the_window() {
        // Truncation lands squarely in the half-window: no adjustment.
        assert_eq!(decode_packet_number(0x1000, 0x02, 8), 0x1002);
    }

    #[test]
    fn encode_then_decode_roundtrips_across_widths() {
        // For a spread of numbers, truncating against the previous packet and
        // decoding against it must recover the original.
        for &full in &[
            1u64,
            0xff,
            0x100,
            0x1234,
            0x00ac_5c02,
            0xa82f_9b32,
            0x0001_0000_0000,
            MAX_PACKET_NUMBER - 5,
        ] {
            let largest_acked = full.checked_sub(3);
            let mut out = Vec::new();
            let n = encode_packet_number(full, largest_acked, &mut out).unwrap();
            let truncated = read_truncated_packet_number(&out);
            let largest_pn = full - 1; // the previous packet was processed
            assert_eq!(
                decode_packet_number(largest_pn, truncated, n as u32 * 8),
                full,
                "roundtrip failed for {full:#x}"
            );
        }
    }

    #[test]
    fn length_bits_roundtrip_through_the_first_byte() {
        for num_bytes in 1..=4usize {
            let bits = encode_pn_length_bits(num_bytes);
            assert!(bits <= PN_LENGTH_MASK);
            // The bits sit in the low two positions of an otherwise arbitrary byte.
            let first_byte = 0xc0 | bits;
            assert_eq!(pn_length_from_first_byte(first_byte), num_bytes);
        }
    }

    #[test]
    fn encode_pn_length_bits_saturates_on_zero() {
        assert_eq!(encode_pn_length_bits(0), 0);
    }

    #[test]
    fn read_truncated_assembles_big_endian() {
        assert_eq!(read_truncated_packet_number(&[0x5c, 0x02]), 0x5c02);
        assert_eq!(read_truncated_packet_number(&[0x01, 0x02, 0x03, 0x04]), 0x0102_0304);
        assert_eq!(read_truncated_packet_number(&[]), 0);
    }

    #[test]
    fn no_ack_uses_the_full_range() {
        // Before any acknowledgement, num_unacked = full_pn + 1: a pn of 200
        // needs 2 bytes (201 > 2^7) even though the value fits in one.
        assert_eq!(packet_number_length(200, None).unwrap(), 2);
    }

    #[test]
    fn error_display_is_nonempty() {
        assert!(!PacketNumberError::Unrepresentable.to_string().is_empty());
    }
}
