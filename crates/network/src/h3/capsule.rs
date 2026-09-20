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
}
