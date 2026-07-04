//! QUIC packet header codec — RFC 9000 §17.
//!
//! A QUIC packet is a header followed by an AEAD-protected payload. This module
//! is a pure parse/serialize layer over the **header**: it reads and writes the
//! public, unprotected header fields of every packet shape a client exchanges
//! and captures the protected remainder (the packet number plus the encrypted
//! payload) as opaque bytes. It sits on the [`varint`] codec (Slice 1) the same
//! way [`quic_frame`](super::quic_frame) and the HTTP/3 [`frame`](super::frame)
//! codec do, and is the layer the later connection code parses first — before
//! it can remove header protection (RFC 9001 §5.4) or AEAD-decrypt the payload
//! into the transport frames [`quic_frame`](super::quic_frame) decodes.
//!
//! ## Scope
//!
//! Every RFC 9000 §17 packet form is represented by a [`Packet`] variant:
//!
//! - **Long header** (§17.2): the invariant `Header Form`/`Fixed Bit`/`Version`/
//!   `DCID`/`SCID` prefix (RFC 8999) followed by the type-specific body —
//!   - Initial (§17.2.2): a Token and a `Length`-delimited protected region —
//!     [`Packet::Initial`].
//!   - 0-RTT (§17.2.3) — [`Packet::ZeroRtt`] — and Handshake (§17.2.4) —
//!     [`Packet::Handshake`]: a `Length`-delimited protected region.
//!   - Retry (§17.2.5): a Retry Token running to a trailing 16-byte Integrity
//!     Tag — [`Packet::Retry`].
//! - **Version Negotiation** (§17.2.1): a long-header packet whose Version is
//!   `0`, listing the versions a server supports — [`Packet::VersionNegotiation`].
//! - **Short header** (§17.3), the 1-RTT packet: a Destination Connection ID
//!   (whose length is *not* on the wire) and a protected region running to the
//!   end of the datagram — [`Packet::Short`].
//!
//! The low bits of the first byte that carry the Reserved bits, Key Phase, and
//! Packet Number Length are **header-protected** (RFC 9001 §5.4): a codec that
//! does not remove that protection cannot interpret them, so they are preserved
//! verbatim (`reserved_and_pn_bits` / `protected_bits`) for an exact round trip
//! rather than decoded. The Packet Number itself lives inside the `protected`
//! bytes for the same reason.
//!
//! ## Out of scope (deferred to later slices)
//!
//! - Header protection and AEAD packet protection (RFC 9001) — hence the
//!   opaque `protected` regions and preserved protected bits.
//! - Packet-number decoding/encoding and packet-number spaces (RFC 9000 §17.1).
//! - Coalescing multiple packets in one datagram beyond what a caller can drive
//!   with the returned `consumed` count, and UDP/IO.

use super::varint;

// ── First-byte bit layout (RFC 9000 §17.2 / §17.3) ───────────────────────────

/// Header Form bit (RFC 9000 §17.2): set for a long header, clear for a short
/// header. The most-significant bit of the first byte.
pub const HEADER_FORM_LONG: u8 = 0x80;
/// Fixed Bit (RFC 9000 §17.2): set on every QUIC v1 packet except Version
/// Negotiation, whose remaining first-byte bits are unconstrained.
pub const FIXED_BIT: u8 = 0x40;
/// Mask selecting the 2-bit Long Packet Type field (RFC 9000 §17.2).
const LONG_TYPE_MASK: u8 = 0x30;
/// Long Packet Type value for an Initial packet (RFC 9000 §17.2.2).
const LONG_TYPE_INITIAL: u8 = 0x00;
/// Long Packet Type value for a 0-RTT packet (RFC 9000 §17.2.3).
const LONG_TYPE_0RTT: u8 = 0x10;
/// Long Packet Type value for a Handshake packet (RFC 9000 §17.2.4).
const LONG_TYPE_HANDSHAKE: u8 = 0x20;
/// Long Packet Type value for a Retry packet (RFC 9000 §17.2.5).
const LONG_TYPE_RETRY: u8 = 0x30;
/// Mask of the header-protected low bits of a long-header first byte: the two
/// Reserved bits and the two Packet Number Length bits (RFC 9000 §17.2).
const LONG_RESERVED_PN_MASK: u8 = 0x0f;
/// Latency Spin Bit of a short-header first byte (RFC 9000 §17.3.1) — not
/// header-protected.
const SHORT_SPIN_BIT: u8 = 0x20;
/// Mask of the header-protected low bits of a short-header first byte: two
/// Reserved bits, the Key Phase bit, and two Packet Number Length bits
/// (RFC 9000 §17.3.1).
const SHORT_PROTECTED_MASK: u8 = 0x1f;

/// Length in bytes of a Retry Integrity Tag (RFC 9000 §17.2.5 / §5.8).
pub const RETRY_INTEGRITY_TAG_LEN: usize = 16;

/// Maximum length in bytes of a connection ID in QUIC v1 (RFC 9000 §17.2).
pub const MAX_CONNECTION_ID_LEN: usize = 20;

// ── Error ────────────────────────────────────────────────────────────────────

/// Packet-header codec error. The connection layer maps these to the
/// appropriate transport error (a malformed header is generally
/// `PROTOCOL_VIOLATION`, RFC 9000 §10.2); the variant preserves *why* for
/// diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PacketError {
    /// Input ended in the middle of a header field.
    UnexpectedEof,
    /// A length field (Token Length or the long-header `Length`) claimed more
    /// bytes than the buffer holds.
    LengthTooLong,
    /// A connection-ID length byte exceeded [`MAX_CONNECTION_ID_LEN`]
    /// (RFC 9000 §17.2). Carries the offending length.
    ConnectionIdTooLong(usize),
    /// A varint value exceeds the 2^62 − 1 QUIC varint maximum on encode.
    VarIntOverflow(u64),
    /// A Version Negotiation packet's Supported Versions list was not a whole
    /// number of 4-byte versions (RFC 9000 §17.2.1). Carries the trailing byte
    /// count.
    MalformedVersionNegotiation(usize),
}

impl core::fmt::Display for PacketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "QUIC packet: unexpected EOF"),
            Self::LengthTooLong => write!(f, "QUIC packet: length exceeds remaining input"),
            Self::ConnectionIdTooLong(n) => {
                write!(f, "QUIC packet: connection-id length {n} exceeds {MAX_CONNECTION_ID_LEN}")
            }
            Self::VarIntOverflow(v) => write!(f, "QUIC packet: value {v} exceeds varint maximum"),
            Self::MalformedVersionNegotiation(n) => {
                write!(f, "QUIC packet: {n} trailing bytes in version list (not a multiple of 4)")
            }
        }
    }
}

impl std::error::Error for PacketError {}

impl From<varint::VarIntTooLarge> for PacketError {
    fn from(e: varint::VarIntTooLarge) -> Self {
        Self::VarIntOverflow(e.0)
    }
}

// ── Packet ───────────────────────────────────────────────────────────────────

/// A parsed QUIC packet header plus its opaque protected region (RFC 9000 §17).
///
/// The `protected` bytes on the long-header 1-RTT-bearing variants and on
/// [`Packet::Short`] hold the header-protected Packet Number followed by the
/// AEAD-encrypted payload; this codec neither removes header protection nor
/// decrypts, so they are carried verbatim. The header-protected first-byte bits
/// are likewise preserved (`reserved_and_pn_bits` / `protected_bits`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    /// Initial packet (RFC 9000 §17.2.2): carries CRYPTO/ACK during the
    /// handshake, plus a Token echoed from a Retry or NEW_TOKEN.
    Initial {
        /// QUIC version (RFC 9000 §15); `1` for QUIC v1.
        version: u32,
        /// Destination Connection ID (0..=20 bytes).
        dcid: Vec<u8>,
        /// Source Connection ID (0..=20 bytes).
        scid: Vec<u8>,
        /// The header-protected low four bits of the first byte (Reserved +
        /// Packet Number Length), preserved verbatim (`0..=15`).
        reserved_and_pn_bits: u8,
        /// The address-validation Token (empty when none).
        token: Vec<u8>,
        /// The `Length`-delimited protected region: the packet number followed
        /// by the encrypted payload.
        protected: Vec<u8>,
    },
    /// 0-RTT packet (RFC 9000 §17.2.3): early application data.
    ZeroRtt {
        /// QUIC version (RFC 9000 §15).
        version: u32,
        /// Destination Connection ID (0..=20 bytes).
        dcid: Vec<u8>,
        /// Source Connection ID (0..=20 bytes).
        scid: Vec<u8>,
        /// The header-protected low four bits of the first byte, preserved
        /// verbatim (`0..=15`).
        reserved_and_pn_bits: u8,
        /// The `Length`-delimited protected region (packet number + payload).
        protected: Vec<u8>,
    },
    /// Handshake packet (RFC 9000 §17.2.4): CRYPTO/ACK in the Handshake space.
    Handshake {
        /// QUIC version (RFC 9000 §15).
        version: u32,
        /// Destination Connection ID (0..=20 bytes).
        dcid: Vec<u8>,
        /// Source Connection ID (0..=20 bytes).
        scid: Vec<u8>,
        /// The header-protected low four bits of the first byte, preserved
        /// verbatim (`0..=15`).
        reserved_and_pn_bits: u8,
        /// The `Length`-delimited protected region (packet number + payload).
        protected: Vec<u8>,
    },
    /// Retry packet (RFC 9000 §17.2.5): a stateless server's request to retry
    /// with an address-validation token. Carries no packet number; the Retry
    /// Token runs to the trailing 16-byte Integrity Tag, and the packet extends
    /// to the end of the datagram.
    Retry {
        /// QUIC version (RFC 9000 §15).
        version: u32,
        /// Destination Connection ID (0..=20 bytes).
        dcid: Vec<u8>,
        /// Source Connection ID (0..=20 bytes).
        scid: Vec<u8>,
        /// The four Unused first-byte bits, preserved verbatim (`0..=15`).
        unused_bits: u8,
        /// The Retry Token to echo in a subsequent Initial packet.
        retry_token: Vec<u8>,
        /// The Retry Integrity Tag (RFC 9000 §5.8).
        integrity_tag: [u8; RETRY_INTEGRITY_TAG_LEN],
    },
    /// Version Negotiation packet (RFC 9000 §17.2.1): a long-header packet with
    /// Version `0`, listing the versions the server supports. Extends to the
    /// end of the datagram.
    VersionNegotiation {
        /// The first byte verbatim: only the Header Form bit is meaningful; the
        /// server sets the remaining seven bits to an arbitrary (unpredictable)
        /// value, so it is preserved rather than reconstructed.
        first_byte: u8,
        /// Destination Connection ID (echoes the client's Source CID).
        dcid: Vec<u8>,
        /// Source Connection ID (echoes the client's Destination CID).
        scid: Vec<u8>,
        /// The versions the server supports, in order.
        supported_versions: Vec<u32>,
    },
    /// Short-header (1-RTT) packet (RFC 9000 §17.3.1): application data after
    /// the handshake. The Destination Connection ID length is *not* on the wire
    /// — the receiver knows the length of the connection IDs it issued — so it
    /// must be supplied to [`Packet::parse`].
    Short {
        /// The Latency Spin Bit (RFC 9000 §17.3.1), not header-protected.
        spin: bool,
        /// The header-protected low five bits of the first byte (two Reserved
        /// bits, Key Phase, two Packet Number Length bits), preserved verbatim
        /// (`0..=31`).
        protected_bits: u8,
        /// Destination Connection ID (its length known out of band).
        dcid: Vec<u8>,
        /// The protected region running to the end of the datagram (packet
        /// number + payload).
        protected: Vec<u8>,
    },
}

impl Packet {
    /// Parse one QUIC packet header (and its protected remainder) from the
    /// front of `buf`.
    ///
    /// `local_cid_len` is the length of the connection IDs this endpoint
    /// issued; it is consulted **only** to delimit the Destination Connection
    /// ID of a short-header packet (whose length is not on the wire, RFC 9000
    /// §17.3.1) and is ignored for every long-header form. Long-header,
    /// Version Negotiation, Retry, and short-header packets that carry no
    /// explicit `Length` field (Retry / Version Negotiation / short header)
    /// consume the whole of `buf`, since those packets are not followed by a
    /// coalesced packet in the same datagram.
    ///
    /// Returns the packet and the number of bytes consumed (which, for an
    /// Initial / 0-RTT / Handshake packet, ends at its `Length`-delimited
    /// region and may leave a coalesced packet behind).
    ///
    /// # Errors
    ///
    /// [`PacketError`] if a field is truncated, a length is out of range, or a
    /// Version Negotiation version list is malformed.
    pub fn parse(buf: &[u8], local_cid_len: usize) -> Result<(Self, usize), PacketError> {
        let first = *buf.first().ok_or(PacketError::UnexpectedEof)?;
        if first & HEADER_FORM_LONG == 0 {
            return Self::parse_short(buf, first, local_cid_len);
        }
        // Long header: invariant Version / DCID / SCID prefix (RFC 8999).
        let mut cur = &buf[1..];
        let version = take_u32(&mut cur)?;
        let dcid = take_connection_id(&mut cur)?;
        let scid = take_connection_id(&mut cur)?;

        if version == 0 {
            // Version Negotiation (RFC 9000 §17.2.1): the rest is a list of
            // 32-bit versions and must divide evenly.
            let rem = cur.len() % 4;
            if rem != 0 {
                return Err(PacketError::MalformedVersionNegotiation(rem));
            }
            let mut supported_versions = Vec::with_capacity(cur.len() / 4);
            while !cur.is_empty() {
                supported_versions.push(take_u32(&mut cur)?);
            }
            let consumed = buf.len() - cur.len();
            return Ok((
                Self::VersionNegotiation { first_byte: first, dcid, scid, supported_versions },
                consumed,
            ));
        }

        let reserved_and_pn_bits = first & LONG_RESERVED_PN_MASK;
        let packet = match first & LONG_TYPE_MASK {
            LONG_TYPE_INITIAL => {
                let token = take_length_prefixed(&mut cur)?;
                let protected = take_length_region(&mut cur)?;
                Self::Initial { version, dcid, scid, reserved_and_pn_bits, token, protected }
            }
            LONG_TYPE_0RTT => {
                let protected = take_length_region(&mut cur)?;
                Self::ZeroRtt { version, dcid, scid, reserved_and_pn_bits, protected }
            }
            LONG_TYPE_HANDSHAKE => {
                let protected = take_length_region(&mut cur)?;
                Self::Handshake { version, dcid, scid, reserved_and_pn_bits, protected }
            }
            // The 2-bit mask yields exactly these four values; Retry is the last.
            _ => {
                // Retry (§17.2.5): token runs to the trailing 16-byte tag.
                if cur.len() < RETRY_INTEGRITY_TAG_LEN {
                    return Err(PacketError::UnexpectedEof);
                }
                let split = cur.len() - RETRY_INTEGRITY_TAG_LEN;
                let retry_token = cur[..split].to_vec();
                let mut integrity_tag = [0u8; RETRY_INTEGRITY_TAG_LEN];
                integrity_tag.copy_from_slice(&cur[split..]);
                cur = &cur[cur.len()..];
                Self::Retry {
                    version,
                    dcid,
                    scid,
                    unused_bits: reserved_and_pn_bits,
                    retry_token,
                    integrity_tag,
                }
            }
        };
        let consumed = buf.len() - cur.len();
        Ok((packet, consumed))
    }

    /// Parse a short-header packet, whose DCID length is known out of band.
    fn parse_short(buf: &[u8], first: u8, local_cid_len: usize) -> Result<(Self, usize), PacketError> {
        if local_cid_len > MAX_CONNECTION_ID_LEN {
            return Err(PacketError::ConnectionIdTooLong(local_cid_len));
        }
        let mut cur = &buf[1..];
        let dcid = take_bytes(&mut cur, local_cid_len)?.to_vec();
        // The protected region runs to the end of the datagram (no Length).
        let protected = cur.to_vec();
        Ok((
            Self::Short {
                spin: first & SHORT_SPIN_BIT != 0,
                protected_bits: first & SHORT_PROTECTED_MASK,
                dcid,
                protected,
            },
            buf.len(),
        ))
    }

    /// Whether this packet uses the long header (RFC 9000 §17.2). Retry and
    /// Version Negotiation are long-header forms; only [`Packet::Short`] is not.
    #[must_use]
    pub const fn is_long_header(&self) -> bool {
        !matches!(self, Self::Short { .. })
    }

    /// Serialize this packet header and its protected region onto `out`.
    ///
    /// For an Initial / 0-RTT / Handshake packet the `Length` field is derived
    /// as the byte count of `protected`; the header-protected first-byte bits
    /// stored on the variant are written back verbatim.
    ///
    /// # Errors
    ///
    /// [`PacketError::VarIntOverflow`] if a length exceeds the QUIC varint
    /// maximum, or [`PacketError::ConnectionIdTooLong`] if a connection ID
    /// exceeds [`MAX_CONNECTION_ID_LEN`].
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), PacketError> {
        match self {
            Self::Initial { version, dcid, scid, reserved_and_pn_bits, token, protected } => {
                out.push(HEADER_FORM_LONG | FIXED_BIT | LONG_TYPE_INITIAL | reserved_and_pn_bits);
                put_long_prefix(*version, dcid, scid, out)?;
                put_length_prefixed(token, out)?;
                put_varint(protected.len() as u64, out)?;
                out.extend_from_slice(protected);
            }
            Self::ZeroRtt { version, dcid, scid, reserved_and_pn_bits, protected } => {
                out.push(HEADER_FORM_LONG | FIXED_BIT | LONG_TYPE_0RTT | reserved_and_pn_bits);
                put_long_prefix(*version, dcid, scid, out)?;
                put_varint(protected.len() as u64, out)?;
                out.extend_from_slice(protected);
            }
            Self::Handshake { version, dcid, scid, reserved_and_pn_bits, protected } => {
                out.push(HEADER_FORM_LONG | FIXED_BIT | LONG_TYPE_HANDSHAKE | reserved_and_pn_bits);
                put_long_prefix(*version, dcid, scid, out)?;
                put_varint(protected.len() as u64, out)?;
                out.extend_from_slice(protected);
            }
            Self::Retry { version, dcid, scid, unused_bits, retry_token, integrity_tag } => {
                out.push(HEADER_FORM_LONG | FIXED_BIT | LONG_TYPE_RETRY | unused_bits);
                put_long_prefix(*version, dcid, scid, out)?;
                out.extend_from_slice(retry_token);
                out.extend_from_slice(integrity_tag);
            }
            Self::VersionNegotiation { first_byte, dcid, scid, supported_versions } => {
                out.push(*first_byte);
                // Version Negotiation carries Version 0 (RFC 9000 §17.2.1).
                put_long_prefix(0, dcid, scid, out)?;
                for &v in supported_versions {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Self::Short { spin, protected_bits, dcid, protected } => {
                let mut first = FIXED_BIT | protected_bits;
                if *spin {
                    first |= SHORT_SPIN_BIT;
                }
                out.push(first);
                out.extend_from_slice(dcid);
                out.extend_from_slice(protected);
            }
        }
        Ok(())
    }
}

// ── Primitive readers / writers ──────────────────────────────────────────────

/// Pull a big-endian `u32` (e.g. the Version field) off the front of `buf`.
fn take_u32(buf: &mut &[u8]) -> Result<u32, PacketError> {
    let bytes = take_bytes(buf, 4)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Pull one QUIC varint off the front of `buf`, advancing it.
fn take_varint(buf: &mut &[u8]) -> Result<u64, PacketError> {
    let (value, consumed) = varint::decode(buf).ok_or(PacketError::UnexpectedEof)?;
    *buf = &buf[consumed..];
    Ok(value)
}

/// Pull exactly `n` bytes off the front of `buf`, advancing it.
fn take_bytes<'a>(buf: &mut &'a [u8], n: usize) -> Result<&'a [u8], PacketError> {
    if buf.len() < n {
        return Err(PacketError::UnexpectedEof);
    }
    let (head, rest) = buf.split_at(n);
    *buf = rest;
    Ok(head)
}

/// Pull a single-byte-length-prefixed connection ID off the front of `buf`,
/// enforcing the [`MAX_CONNECTION_ID_LEN`] bound (RFC 9000 §17.2).
fn take_connection_id(buf: &mut &[u8]) -> Result<Vec<u8>, PacketError> {
    let (&len, rest) = buf.split_first().ok_or(PacketError::UnexpectedEof)?;
    *buf = rest;
    let len = len as usize;
    if len > MAX_CONNECTION_ID_LEN {
        return Err(PacketError::ConnectionIdTooLong(len));
    }
    Ok(take_bytes(buf, len)?.to_vec())
}

/// Pull a varint-length-prefixed byte string (e.g. the Initial Token) off the
/// front of `buf`.
fn take_length_prefixed(buf: &mut &[u8]) -> Result<Vec<u8>, PacketError> {
    let len = take_varint(buf)?;
    let len = usize::try_from(len).map_err(|_| PacketError::LengthTooLong)?;
    if buf.len() < len {
        return Err(PacketError::LengthTooLong);
    }
    Ok(take_bytes(buf, len)?.to_vec())
}

/// Pull the long-header `Length`-delimited protected region (packet number +
/// payload) off the front of `buf` (RFC 9000 §17.2.2).
fn take_length_region(buf: &mut &[u8]) -> Result<Vec<u8>, PacketError> {
    take_length_prefixed(buf)
}

/// Append a QUIC varint to `out`, mapping an overflow into the codec error.
fn put_varint(value: u64, out: &mut Vec<u8>) -> Result<(), PacketError> {
    varint::encode(value, out)?;
    Ok(())
}

/// Append a varint length prefix followed by the bytes of `data`.
fn put_length_prefixed(data: &[u8], out: &mut Vec<u8>) -> Result<(), PacketError> {
    put_varint(data.len() as u64, out)?;
    out.extend_from_slice(data);
    Ok(())
}

/// Append the invariant long-header prefix after the first byte: Version, then
/// each connection ID as a single-byte length followed by its bytes (RFC 9000
/// §17.2 / RFC 8999).
fn put_long_prefix(version: u32, dcid: &[u8], scid: &[u8], out: &mut Vec<u8>) -> Result<(), PacketError> {
    out.extend_from_slice(&version.to_be_bytes());
    put_connection_id(dcid, out)?;
    put_connection_id(scid, out)?;
    Ok(())
}

/// Append a connection ID as a single-byte length followed by its bytes,
/// enforcing the [`MAX_CONNECTION_ID_LEN`] bound.
fn put_connection_id(cid: &[u8], out: &mut Vec<u8>) -> Result<(), PacketError> {
    if cid.len() > MAX_CONNECTION_ID_LEN {
        return Err(PacketError::ConnectionIdTooLong(cid.len()));
    }
    // Length fits in a byte because it is bounded by 20.
    out.push(cid.len() as u8);
    out.extend_from_slice(cid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a packet, parse it back with the given local CID length, and
    /// assert the value round-trips and the whole buffer is consumed.
    fn roundtrip(packet: &Packet, local_cid_len: usize) {
        let mut buf = Vec::new();
        packet.encode(&mut buf).expect("encode");
        let (decoded, consumed) = Packet::parse(&buf, local_cid_len).expect("parse");
        assert_eq!(&decoded, packet, "roundtrip value");
        assert_eq!(consumed, buf.len(), "consumed whole packet");
    }

    #[test]
    fn roundtrip_initial() {
        roundtrip(
            &Packet::Initial {
                version: 1,
                dcid: vec![0x01, 0x02, 0x03, 0x04],
                scid: vec![0xaa, 0xbb],
                reserved_and_pn_bits: 0b0000_0011,
                token: vec![0xde, 0xad, 0xbe, 0xef],
                protected: vec![0x10; 40],
            },
            0,
        );
    }

    #[test]
    fn roundtrip_initial_empty_cids_and_token() {
        roundtrip(
            &Packet::Initial {
                version: 1,
                dcid: Vec::new(),
                scid: Vec::new(),
                reserved_and_pn_bits: 0,
                token: Vec::new(),
                protected: vec![0x00, 0x01, 0x02],
            },
            0,
        );
    }

    #[test]
    fn roundtrip_zero_rtt_and_handshake() {
        roundtrip(
            &Packet::ZeroRtt {
                version: 1,
                dcid: vec![9, 8, 7],
                scid: vec![1],
                reserved_and_pn_bits: 0b0000_1010,
                protected: vec![0xfe; 12],
            },
            0,
        );
        roundtrip(
            &Packet::Handshake {
                version: 1,
                dcid: vec![5, 5, 5, 5, 5],
                scid: vec![6, 6],
                reserved_and_pn_bits: 0b0000_0101,
                protected: vec![0x77; 8],
            },
            0,
        );
    }

    #[test]
    fn roundtrip_retry() {
        roundtrip(
            &Packet::Retry {
                version: 1,
                dcid: vec![0x11, 0x22],
                scid: vec![0x33, 0x44, 0x55],
                unused_bits: 0b0000_1111,
                retry_token: vec![0xa1, 0xa2, 0xa3, 0xa4, 0xa5],
                integrity_tag: [0x5a; RETRY_INTEGRITY_TAG_LEN],
            },
            0,
        );
    }

    #[test]
    fn roundtrip_retry_empty_token() {
        roundtrip(
            &Packet::Retry {
                version: 1,
                dcid: Vec::new(),
                scid: Vec::new(),
                unused_bits: 0,
                retry_token: Vec::new(),
                integrity_tag: [0u8; RETRY_INTEGRITY_TAG_LEN],
            },
            0,
        );
    }

    #[test]
    fn roundtrip_version_negotiation() {
        roundtrip(
            &Packet::VersionNegotiation {
                first_byte: 0xc0,
                dcid: vec![0x01, 0x02, 0x03],
                scid: vec![0x04, 0x05],
                supported_versions: vec![0x0000_0001, 0xff00_001d, 0x1a2a_3a4a],
            },
            0,
        );
    }

    #[test]
    fn roundtrip_version_negotiation_empty_list() {
        roundtrip(
            &Packet::VersionNegotiation {
                first_byte: 0x80,
                dcid: vec![0xab],
                scid: Vec::new(),
                supported_versions: Vec::new(),
            },
            0,
        );
    }

    #[test]
    fn roundtrip_short_header() {
        roundtrip(
            &Packet::Short {
                spin: true,
                protected_bits: 0b0001_0011,
                dcid: vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
                protected: vec![0xcd; 20],
            },
            8,
        );
    }

    #[test]
    fn roundtrip_short_header_zero_len_cid() {
        roundtrip(
            &Packet::Short {
                spin: false,
                protected_bits: 0,
                dcid: Vec::new(),
                protected: vec![0x99; 5],
            },
            0,
        );
    }

    #[test]
    fn initial_leaves_coalesced_packet() {
        // An Initial packet whose Length-delimited region does not run to the
        // end of the buffer: parse must stop at Length and report the shorter
        // consumed count, leaving the trailing bytes for a coalesced packet.
        let packet = Packet::Initial {
            version: 1,
            dcid: vec![0xaa],
            scid: vec![0xbb],
            reserved_and_pn_bits: 0,
            token: Vec::new(),
            protected: vec![0x01, 0x02, 0x03, 0x04],
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();
        let header_len = buf.len();
        buf.extend_from_slice(&[0xde, 0xad]); // coalesced remainder
        let (decoded, consumed) = Packet::parse(&buf, 0).unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(consumed, header_len, "consumed only up to Length");
        assert_eq!(&buf[consumed..], &[0xde, 0xad]);
    }

    #[test]
    fn detects_header_form() {
        // Long-header first byte (MSB set) → long; short otherwise.
        let long = Packet::Handshake {
            version: 1,
            dcid: Vec::new(),
            scid: Vec::new(),
            reserved_and_pn_bits: 0,
            protected: vec![0],
        };
        assert!(long.is_long_header());
        let short = Packet::Short {
            spin: false,
            protected_bits: 0,
            dcid: Vec::new(),
            protected: vec![0],
        };
        assert!(!short.is_long_header());
    }

    #[test]
    fn first_byte_wire_bits() {
        // Initial: 1 1 00 rrpp → 0xc0 | reserved_pn.
        let mut buf = Vec::new();
        Packet::Initial {
            version: 1,
            dcid: Vec::new(),
            scid: Vec::new(),
            reserved_and_pn_bits: 0b0000_0011,
            token: Vec::new(),
            protected: vec![0],
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf[0], 0xc3, "Initial first byte");

        // Handshake type bits = 0b10 → 0xe0 base.
        buf.clear();
        Packet::Handshake {
            version: 1,
            dcid: Vec::new(),
            scid: Vec::new(),
            reserved_and_pn_bits: 0,
            protected: vec![0],
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf[0], 0xe0, "Handshake first byte");

        // Short: 0 1 spin 000pp with spin set → 0x60 | protected.
        buf.clear();
        Packet::Short {
            spin: true,
            protected_bits: 0b0000_0001,
            dcid: Vec::new(),
            protected: vec![0],
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf[0], 0x61, "Short first byte with spin");
    }

    #[test]
    fn version_zero_parses_as_version_negotiation() {
        // A long-header packet with Version 0 is Version Negotiation regardless
        // of the type bits.
        let mut buf = vec![0x80]; // long header
        buf.extend_from_slice(&0u32.to_be_bytes()); // version 0
        buf.push(1); // dcid len
        buf.push(0x42); // dcid
        buf.push(0); // scid len 0
        buf.extend_from_slice(&1u32.to_be_bytes()); // one supported version
        let (packet, consumed) = Packet::parse(&buf, 0).unwrap();
        assert_eq!(consumed, buf.len());
        match packet {
            Packet::VersionNegotiation { dcid, supported_versions, .. } => {
                assert_eq!(dcid, vec![0x42]);
                assert_eq!(supported_versions, vec![1]);
            }
            other => panic!("expected VersionNegotiation, got {other:?}"),
        }
    }

    #[test]
    fn rejects_connection_id_too_long() {
        // DCID length byte of 21 exceeds the 20-byte QUIC v1 maximum.
        let mut buf = vec![0xc0];
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.push(21); // dcid len = 21 > 20
        buf.extend_from_slice(&[0u8; 21]);
        assert_eq!(Packet::parse(&buf, 0), Err(PacketError::ConnectionIdTooLong(21)));
    }

    #[test]
    fn rejects_short_local_cid_too_long() {
        let buf = vec![0x40, 0x00]; // short header
        assert_eq!(
            Packet::parse(&buf, MAX_CONNECTION_ID_LEN + 1),
            Err(PacketError::ConnectionIdTooLong(MAX_CONNECTION_ID_LEN + 1))
        );
    }

    #[test]
    fn rejects_malformed_version_negotiation() {
        // Version 0 then a 3-byte (non-multiple-of-4) supported-versions tail.
        let mut buf = vec![0x80];
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.push(0); // dcid len
        buf.push(0); // scid len
        buf.extend_from_slice(&[0x01, 0x02, 0x03]); // 3 trailing bytes
        assert_eq!(Packet::parse(&buf, 0), Err(PacketError::MalformedVersionNegotiation(3)));
    }

    #[test]
    fn rejects_truncated_retry_tag() {
        // Retry with fewer than 16 trailing bytes for the integrity tag.
        let mut buf = vec![0xf0]; // long header, Retry type
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.push(0); // dcid len
        buf.push(0); // scid len
        buf.extend_from_slice(&[0u8; 8]); // only 8 bytes, need ≥16
        assert_eq!(Packet::parse(&buf, 0), Err(PacketError::UnexpectedEof));
    }

    #[test]
    fn rejects_initial_length_beyond_buffer() {
        // Initial whose Length field claims more bytes than remain.
        let mut buf = vec![0xc0];
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.push(0); // dcid len
        buf.push(0); // scid len
        buf.push(0); // token len 0
        buf.push(0x20); // Length = 32 (1-byte varint) but no payload follows
        assert_eq!(Packet::parse(&buf, 0), Err(PacketError::LengthTooLong));
    }

    #[test]
    fn empty_buffer_is_eof() {
        assert_eq!(Packet::parse(&[], 0), Err(PacketError::UnexpectedEof));
    }
}
