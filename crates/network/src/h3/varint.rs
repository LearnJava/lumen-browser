//! QUIC variable-length integer codec (RFC 9000 §16).
//!
//! A QUIC varint packs its byte-length into the two most-significant bits of
//! the first byte: prefix `00` → 1 byte (6-bit value), `01` → 2 bytes (14-bit),
//! `10` → 4 bytes (30-bit), `11` → 8 bytes (62-bit). The remaining bits are the
//! value in network byte order. The encoding is used pervasively by both the
//! QUIC transport and the HTTP/3 framing layer (RFC 9114), so it lives in its
//! own leaf module with no IO and no dependencies.
//!
//! Pure parse/serialize: [`decode`] pulls one varint off the front of a buffer
//! (returning `None` when the buffer is too short to hold the full integer),
//! and [`encode`] appends the shortest wire form of a value. There is no
//! "malformed varint" — any byte sequence long enough decodes — so [`decode`]
//! cannot fail; only [`encode`] can, when a value exceeds [`MAX_VARINT`].

/// Maximum value representable by a QUIC varint: 2^62 − 1 (RFC 9000 §16).
pub const MAX_VARINT: u64 = (1 << 62) - 1;

/// Error returned by [`encode`] when a value does not fit in a QUIC varint
/// (i.e. exceeds [`MAX_VARINT`]). Carries the offending value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VarIntTooLarge(pub u64);

impl core::fmt::Display for VarIntTooLarge {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "value {} exceeds QUIC varint maximum 2^62-1", self.0)
    }
}

impl std::error::Error for VarIntTooLarge {}

/// Number of bytes the varint encoding of `value` occupies (1, 2, 4, or 8), or
/// `None` if `value` exceeds [`MAX_VARINT`] and cannot be encoded.
#[must_use]
pub fn encoded_len(value: u64) -> Option<usize> {
    match value {
        0..=0x3f => Some(1),
        0x40..=0x3fff => Some(2),
        0x4000..=0x3fff_ffff => Some(4),
        0x4000_0000..=MAX_VARINT => Some(8),
        _ => None,
    }
}

/// Append the shortest QUIC varint encoding of `value` to `out`.
///
/// # Errors
///
/// Returns [`VarIntTooLarge`] if `value` exceeds [`MAX_VARINT`] (2^62 − 1); the
/// two prefix bits leave only 62 bits for the magnitude.
pub fn encode(value: u64, out: &mut Vec<u8>) -> Result<(), VarIntTooLarge> {
    match encoded_len(value) {
        Some(1) => out.push(value as u8),
        Some(2) => out.extend_from_slice(&((value as u16) | 0x4000).to_be_bytes()),
        Some(4) => out.extend_from_slice(&((value as u32) | 0x8000_0000).to_be_bytes()),
        Some(8) => out.extend_from_slice(&(value | 0xc000_0000_0000_0000).to_be_bytes()),
        _ => return Err(VarIntTooLarge(value)),
    }
    Ok(())
}

/// Decode one QUIC varint from the front of `buf`.
///
/// Returns `Some((value, consumed))` where `consumed` is the number of bytes
/// the varint occupied (1, 2, 4, or 8), or `None` if `buf` is empty or shorter
/// than the length signalled by the first byte's prefix (caller should read
/// more bytes and retry).
#[must_use]
pub fn decode(buf: &[u8]) -> Option<(u64, usize)> {
    let first = *buf.first()?;
    // The two most-significant bits select the length: 2^prefix bytes.
    let len = 1usize << (first >> 6);
    if buf.len() < len {
        return None;
    }
    // Low 6 bits of the first byte are the top of the value; the rest follow
    // big-endian.
    let mut value = u64::from(first & 0x3f);
    for &b in &buf[1..len] {
        value = (value << 8) | u64::from(b);
    }
    Some((value, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip the four RFC 9000 §16 example values plus each boundary.
    #[test]
    fn roundtrip_rfc_examples_and_boundaries() {
        // (value, expected encoded length)
        let cases = [
            (0u64, 1usize),
            (0x3f, 1),          // largest 1-byte
            (0x40, 2),          // smallest 2-byte
            (0x3fff, 2),        // largest 2-byte
            (0x4000, 4),        // smallest 4-byte
            (0x3fff_ffff, 4),   // largest 4-byte
            (0x4000_0000, 8),   // smallest 8-byte
            (151_288_809_941_952_652, 8), // RFC 9000 §16 example (0x2197...)
            (MAX_VARINT, 8),    // largest representable
        ];
        for (value, want_len) in cases {
            assert_eq!(encoded_len(value), Some(want_len), "encoded_len({value})");
            let mut buf = Vec::new();
            encode(value, &mut buf).expect("encode in range");
            assert_eq!(buf.len(), want_len, "wire length for {value}");
            let (got, consumed) = decode(&buf).expect("decode full buffer");
            assert_eq!(got, value, "decoded value");
            assert_eq!(consumed, want_len, "consumed bytes");
        }
    }

    /// RFC 9000 §16 fixed wire encodings for the canonical examples.
    #[test]
    fn matches_rfc_wire_bytes() {
        // 37 → single byte 0x25.
        let mut b = Vec::new();
        encode(37, &mut b).unwrap();
        assert_eq!(b, [0x25]);

        // 15293 → two bytes 0x7b 0xbd.
        b.clear();
        encode(15293, &mut b).unwrap();
        assert_eq!(b, [0x7b, 0xbd]);

        // 494878333 → four bytes 0x9d 0x7f 0x3e 0x7d.
        b.clear();
        encode(494_878_333, &mut b).unwrap();
        assert_eq!(b, [0x9d, 0x7f, 0x3e, 0x7d]);

        // 151288809941952652 → eight bytes c2 19 7c 5e ff 14 e8 8c.
        b.clear();
        encode(151_288_809_941_952_652, &mut b).unwrap();
        assert_eq!(b, [0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c]);
    }

    /// Values above 2^62 − 1 cannot be encoded.
    #[test]
    fn rejects_too_large() {
        assert_eq!(encoded_len(MAX_VARINT + 1), None);
        let mut buf = Vec::new();
        assert_eq!(encode(MAX_VARINT + 1, &mut buf), Err(VarIntTooLarge(MAX_VARINT + 1)));
        assert!(buf.is_empty(), "nothing written on error");
        assert_eq!(encode(u64::MAX, &mut buf), Err(VarIntTooLarge(u64::MAX)));
    }

    /// A truncated buffer yields `None` until every byte of the varint arrives.
    #[test]
    fn decode_needs_full_length() {
        let mut buf = Vec::new();
        encode(494_878_333, &mut buf).unwrap(); // 4-byte encoding
        assert_eq!(decode(&buf[..0]), None, "empty");
        assert_eq!(decode(&buf[..1]), None, "1 of 4");
        assert_eq!(decode(&buf[..3]), None, "3 of 4");
        assert!(decode(&buf).is_some(), "complete");
    }

    /// `decode` consumes exactly the varint and leaves trailing bytes untouched.
    #[test]
    fn decode_leaves_trailing_bytes() {
        let mut buf = Vec::new();
        encode(0x40, &mut buf).unwrap(); // 2-byte
        buf.extend_from_slice(&[0xaa, 0xbb]); // trailing payload
        let (value, consumed) = decode(&buf).unwrap();
        assert_eq!(value, 0x40);
        assert_eq!(consumed, 2);
        assert_eq!(&buf[consumed..], &[0xaa, 0xbb]);
    }
}
