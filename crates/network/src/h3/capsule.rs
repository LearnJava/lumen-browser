//! HTTP Capsule Protocol (RFC 9297 §3) — currently only the one capsule
//! WebTransport needs, `CLOSE_WEBTRANSPORT_SESSION`
//! (draft-ietf-webtrans-http3 §4.5).
//!
//! Unlike a QUIC DATAGRAM ([`super::quic_frame::Frame::Datagram`]), a capsule
//! is not a new QUIC-level frame type — it is application data carried
//! straight in the stream's existing byte sequence (RFC 9297 §3: "a capsule
//! is length-prefixed... contained in the payload of a data stream"), so
//! encoding it is exactly the varint-prefixed byte-builder every other
//! WebTransport primitive in [`super::client_transport`] already uses to
//! write bytes to a stream — no new [`super::quic_frame::Frame`] variant, no
//! new send-scheduler path.

use super::varint::{self, VarIntTooLarge};

/// Capsule type for `CLOSE_WEBTRANSPORT_SESSION`
/// (draft-ietf-webtrans-http3 §4.5).
const CLOSE_WEBTRANSPORT_SESSION_CAPSULE_TYPE: u64 = 0x2843;

/// Encodes a `CLOSE_WEBTRANSPORT_SESSION` capsule (draft-ietf-webtrans-http3
/// §4.5): capsule type `0x2843`, then a varint length, then a 32-bit
/// big-endian Application Error Code, then the Application Error Message
/// verbatim (UTF-8, no length prefix of its own — the capsule's own length
/// covers it).
///
/// # Errors
///
/// [`VarIntTooLarge`] if the capsule's own length (4 + `reason.len()`)
/// exceeds the QUIC varint range (2^62 − 1) — unreachable for any
/// `reason` a caller can construct (`WebTransport.close()`'s spec caps
/// `reason` at 1024 UTF-8 bytes before this is ever called).
pub fn encode_close_webtransport_session(close_code: u32, reason: &[u8]) -> Result<Vec<u8>, VarIntTooLarge> {
    let payload_len = 4u64
        .checked_add(reason.len() as u64)
        .ok_or(VarIntTooLarge(u64::MAX))?;

    let mut out = Vec::with_capacity(
        varint::encoded_len(CLOSE_WEBTRANSPORT_SESSION_CAPSULE_TYPE).unwrap_or(1)
            + varint::encoded_len(payload_len).unwrap_or(1)
            + 4
            + reason.len(),
    );
    varint::encode(CLOSE_WEBTRANSPORT_SESSION_CAPSULE_TYPE, &mut out)?;
    varint::encode(payload_len, &mut out)?;
    out.extend_from_slice(&close_code.to_be_bytes());
    out.extend_from_slice(reason);
    Ok(out)
}

/// Decodes a `CLOSE_WEBTRANSPORT_SESSION` capsule (draft-ietf-webtrans-http3
/// §4.5) off the front of `buf` — the receive-side counterpart of
/// [`encode_close_webtransport_session`], used to detect a **peer**-initiated
/// close (GAP-WEBTRANSPORT, remaining sub-slice of срез 5).
///
/// Returns `(close_code, reason)` on a complete capsule. Returns `None` both
/// when `buf` does not yet hold the whole capsule (the caller — same
/// accumulate-across-polls shape as [`super::client_transport::parse_webtransport_uni_header`])
/// and when the leading varint is not the `CLOSE_WEBTRANSPORT_SESSION` type —
/// the session's Extended CONNECT stream carries no other capsule type this
/// client understands, so a mismatched type is treated as "not there yet"
/// rather than a distinct error the caller has no use for.
#[must_use]
pub fn decode_close_webtransport_session(buf: &[u8]) -> Option<(u32, Vec<u8>)> {
    let (capsule_type, type_len) = varint::decode(buf)?;
    if capsule_type != CLOSE_WEBTRANSPORT_SESSION_CAPSULE_TYPE {
        return None;
    }
    let (payload_len, len_len) = varint::decode(&buf[type_len..])?;
    let payload_len = usize::try_from(payload_len).ok()?;
    if payload_len < 4 {
        return None;
    }
    let header_len = type_len + len_len;
    let total_len = header_len.checked_add(payload_len)?;
    if buf.len() < total_len {
        return None;
    }
    let payload = &buf[header_len..total_len];
    let close_code = u32::from_be_bytes(payload[0..4].try_into().ok()?);
    Some((close_code, payload[4..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_type_length_code_and_reason() {
        let bytes = encode_close_webtransport_session(42, b"bye").unwrap();
        // Type 0x2843 needs the 2-byte varint form (> 0x3f).
        assert_eq!(&bytes[0..2], &[0x68, 0x43]);
        // Length = 4 (code) + 3 (reason) = 7, fits the 1-byte varint form.
        assert_eq!(bytes[2], 7);
        assert_eq!(&bytes[3..7], &42u32.to_be_bytes());
        assert_eq!(&bytes[7..], b"bye");
    }

    #[test]
    fn encodes_empty_reason() {
        let bytes = encode_close_webtransport_session(0, b"").unwrap();
        assert_eq!(bytes[2], 4);
        assert_eq!(&bytes[3..7], &0u32.to_be_bytes());
        assert_eq!(bytes.len(), 7);
    }

    #[test]
    fn decodes_a_capsule_this_module_encoded() {
        let bytes = encode_close_webtransport_session(42, b"bye").unwrap();
        let (close_code, reason) = decode_close_webtransport_session(&bytes).unwrap();
        assert_eq!(close_code, 42);
        assert_eq!(reason, b"bye");
    }

    #[test]
    fn decodes_an_empty_reason() {
        let bytes = encode_close_webtransport_session(0, b"").unwrap();
        let (close_code, reason) = decode_close_webtransport_session(&bytes).unwrap();
        assert_eq!(close_code, 0);
        assert!(reason.is_empty());
    }

    #[test]
    fn reports_not_yet_decodable_on_a_truncated_capsule() {
        let bytes = encode_close_webtransport_session(42, b"bye").unwrap();
        assert!(decode_close_webtransport_session(&bytes[..bytes.len() - 1]).is_none());
        assert!(decode_close_webtransport_session(&bytes[..1]).is_none());
        assert!(decode_close_webtransport_session(&[]).is_none());
    }

    #[test]
    fn rejects_a_different_capsule_type() {
        // A varint-encoded type that is not `0x2843`, followed by bytes that
        // would otherwise parse as a valid length/payload — must not be
        // mistaken for `CLOSE_WEBTRANSPORT_SESSION`.
        let mut bytes = vec![0x01];
        bytes.extend_from_slice(&encode_close_webtransport_session(0, b"x").unwrap()[2..]);
        assert!(decode_close_webtransport_session(&bytes).is_none());
    }
}
