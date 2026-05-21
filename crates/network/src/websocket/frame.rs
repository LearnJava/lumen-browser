//! RFC 6455 §5 — WebSocket frame codec.

use std::io::{Read, Write};

use crate::Error;
use lumen_core::error::Result;

use super::mask;

// ── Opcode ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opcode {
    Continuation = 0x0,
    Text         = 0x1,
    Binary       = 0x2,
    Close        = 0x8,
    Ping         = 0x9,
    Pong         = 0xA,
}

impl Opcode {
    pub(crate) fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x0 => Some(Self::Continuation),
            0x1 => Some(Self::Text),
            0x2 => Some(Self::Binary),
            0x8 => Some(Self::Close),
            0x9 => Some(Self::Ping),
            0xA => Some(Self::Pong),
            _   => None,
        }
    }

    pub(crate) fn is_control(self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong)
    }
}

// ── Frame ─────────────────────────────────────────────────────────────────────

pub(crate) struct Frame {
    pub(crate) fin:     bool,
    pub(crate) opcode:  Opcode,
    pub(crate) payload: Vec<u8>,
}

/// Read one frame from `r`. Unmasks the payload if the MASK bit is set.
///
/// Per RFC 6455 §5.5.1, control frames (Ping/Pong/Close) MUST be ≤ 125 bytes.
/// We enforce a 16 MiB hard limit on all frames.
pub(crate) fn read_frame<R: Read>(r: &mut R) -> Result<Frame> {
    let mut hdr = [0u8; 2];
    r.read_exact(&mut hdr)
        .map_err(|e| Error::Network(format!("ws: read frame header: {e}")))?;

    let fin        = hdr[0] & 0x80 != 0;
    let opcode_raw = hdr[0] & 0x0F;
    let masked     = hdr[1] & 0x80 != 0;
    let len7       = (hdr[1] & 0x7F) as u64;

    let opcode = Opcode::from_u8(opcode_raw)
        .ok_or_else(|| Error::Network(format!("ws: unknown opcode 0x{opcode_raw:x}")))?;

    let payload_len = match len7 {
        126 => {
            let mut buf = [0u8; 2];
            r.read_exact(&mut buf)
                .map_err(|e| Error::Network(format!("ws: read ext len16: {e}")))?;
            u64::from(u16::from_be_bytes(buf))
        }
        127 => {
            let mut buf = [0u8; 8];
            r.read_exact(&mut buf)
                .map_err(|e| Error::Network(format!("ws: read ext len64: {e}")))?;
            u64::from_be_bytes(buf)
        }
        n => n,
    };

    const MAX_FRAME: u64 = 16 * 1024 * 1024;
    if payload_len > MAX_FRAME {
        return Err(Error::Network(format!(
            "ws: frame too large: {payload_len} bytes (limit {MAX_FRAME})"
        )));
    }

    // RFC 6455 §5.5: control frames MUST NOT exceed 125 bytes.
    if opcode.is_control() && payload_len > 125 {
        return Err(Error::Network(format!(
            "ws: control frame payload too large: {payload_len}"
        )));
    }

    let mask_key = if masked {
        let mut buf = [0u8; 4];
        r.read_exact(&mut buf)
            .map_err(|e| Error::Network(format!("ws: read mask key: {e}")))?;
        Some(buf)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len as usize];
    r.read_exact(&mut payload)
        .map_err(|e| Error::Network(format!("ws: read payload: {e}")))?;

    if let Some(key) = mask_key {
        mask::apply(&mut payload, key);
    }

    Ok(Frame { fin, opcode, payload })
}

/// Write one frame to `w`.
///
/// `mask_key` MUST be `Some` for client-to-server frames (RFC 6455 §5.3).
pub(crate) fn write_frame<W: Write>(
    w:        &mut W,
    fin:      bool,
    opcode:   Opcode,
    payload:  &[u8],
    mask_key: Option<[u8; 4]>,
) -> Result<()> {
    // header: byte0 + len byte(s) + optional mask key
    let mut hdr = Vec::with_capacity(14);

    let byte0 = (u8::from(fin) << 7) | (opcode as u8);
    hdr.push(byte0);

    let mask_bit: u8 = if mask_key.is_some() { 0x80 } else { 0 };
    let len = payload.len();
    match len {
        0..=125       => hdr.push(mask_bit | len as u8),
        126..=65535   => {
            hdr.push(mask_bit | 126);
            hdr.extend_from_slice(&(len as u16).to_be_bytes());
        }
        _             => {
            hdr.push(mask_bit | 127);
            hdr.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }

    if let Some(key) = mask_key {
        hdr.extend_from_slice(&key);
        w.write_all(&hdr)
            .map_err(|e| Error::Network(format!("ws: write frame header: {e}")))?;
        let mut masked = payload.to_vec();
        mask::apply(&mut masked, key);
        w.write_all(&masked)
            .map_err(|e| Error::Network(format!("ws: write payload: {e}")))?;
    } else {
        w.write_all(&hdr)
            .map_err(|e| Error::Network(format!("ws: write frame header: {e}")))?;
        w.write_all(payload)
            .map_err(|e| Error::Network(format!("ws: write payload: {e}")))?;
    }

    w.flush()
        .map_err(|e| Error::Network(format!("ws: flush: {e}")))
}

// ── Parse Close payload ───────────────────────────────────────────────────────

/// Decode the payload of a Close frame into (status_code, reason).
pub(crate) fn parse_close_payload(payload: &[u8]) -> (Option<u16>, String) {
    if payload.len() < 2 {
        return (None, String::new());
    }
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    let reason = String::from_utf8_lossy(&payload[2..]).into_owned();
    (Some(code), reason)
}

/// Encode Close payload: 2-byte big-endian code + UTF-8 reason.
pub(crate) fn make_close_payload(code: u16, reason: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(2 + reason.len());
    v.extend_from_slice(&code.to_be_bytes());
    v.extend_from_slice(reason.as_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn masked_key() -> [u8; 4] {
        [0x37, 0xfa, 0x21, 0x3d]
    }

    #[test]
    fn roundtrip_text_frame_with_mask() {
        let text = b"Hello";
        let key = masked_key();
        let mut buf = Vec::new();
        write_frame(&mut buf, true, Opcode::Text, text, Some(key)).unwrap();

        let mut cur = Cursor::new(buf);
        let frame = read_frame(&mut cur).unwrap();
        assert!(frame.fin);
        assert_eq!(frame.opcode, Opcode::Text);
        assert_eq!(frame.payload, text);
    }

    #[test]
    fn roundtrip_binary_frame_no_mask() {
        let data: Vec<u8> = (0..=255u8).collect();
        let mut buf = Vec::new();
        write_frame(&mut buf, true, Opcode::Binary, &data, None).unwrap();

        let mut cur = Cursor::new(buf);
        let frame = read_frame(&mut cur).unwrap();
        assert_eq!(frame.opcode, Opcode::Binary);
        assert_eq!(frame.payload, data);
    }

    #[test]
    fn roundtrip_126_byte_extended_length() {
        let data = vec![0xABu8; 126];
        let mut buf = Vec::new();
        write_frame(&mut buf, true, Opcode::Binary, &data, None).unwrap();
        let mut cur = Cursor::new(buf);
        let frame = read_frame(&mut cur).unwrap();
        assert_eq!(frame.payload.len(), 126);
        assert_eq!(frame.payload, data);
    }

    #[test]
    fn roundtrip_close_frame() {
        let payload = make_close_payload(1000, "going away");
        let mut buf = Vec::new();
        write_frame(&mut buf, true, Opcode::Close, &payload, Some(masked_key())).unwrap();
        let mut cur = Cursor::new(buf);
        let frame = read_frame(&mut cur).unwrap();
        let (code, reason) = parse_close_payload(&frame.payload);
        assert_eq!(code, Some(1000));
        assert_eq!(reason, "going away");
    }

    #[test]
    fn parse_close_no_payload() {
        let (code, reason) = parse_close_payload(&[]);
        assert_eq!(code, None);
        assert_eq!(reason, "");
    }

    #[test]
    fn control_frame_over_125_rejected() {
        // Manually craft a Ping frame with 126-byte payload (invalid per spec).
        let mut buf = Vec::new();
        // byte0: FIN + Ping opcode
        buf.push(0x89);
        // byte1: len=126 (extended) — forbidden for control frames
        buf.push(126);
        buf.extend_from_slice(&126u16.to_be_bytes());
        buf.extend(vec![0u8; 126]);
        let mut cur = Cursor::new(buf);
        assert!(read_frame(&mut cur).is_err());
    }

    #[test]
    fn unknown_opcode_rejected() {
        let buf = vec![0x83u8, 0]; // opcode 3 — reserved data
        let mut cur = Cursor::new(buf);
        assert!(read_frame(&mut cur).is_err());
    }

    #[test]
    fn opcode_is_control() {
        assert!(Opcode::Close.is_control());
        assert!(Opcode::Ping.is_control());
        assert!(Opcode::Pong.is_control());
        assert!(!Opcode::Text.is_control());
        assert!(!Opcode::Binary.is_control());
        assert!(!Opcode::Continuation.is_control());
    }
}
