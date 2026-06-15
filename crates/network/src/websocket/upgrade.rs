//! RFC 6455 §4 — WebSocket opening handshake.
//!
//! Handles:
//! - Sec-WebSocket-Key generation (16 pseudo-random bytes → base64)
//! - HTTP/1.1 Upgrade request construction
//! - 101 Switching Protocols response parsing
//! - Sec-WebSocket-Accept validation (base64(SHA-1(key + GUID)))

use std::io::{Read, Write};

use crate::Error;
use lumen_core::error::Result;

const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns a Sec-WebSocket-Key (base64 of 16 pseudo-random bytes).
pub(crate) fn generate_key() -> String {
    base64_encode(&random_bytes())
}

/// Write the HTTP/1.1 Upgrade request and validate the server's 101 response.
///
/// `stream` must be a fresh (unread) TCP/TLS connection to the WS server.
/// On success the stream is in "WebSocket data mode" — caller hands it to
/// the frame codec.
pub(crate) fn perform<S: Read + Write>(
    stream:        &mut S,
    host:          &str,
    path:          &str,
    key_b64:       &str,
    extra_headers: &[(&str, &str)],
) -> Result<()> {
    send_upgrade_request(stream, host, path, key_b64, extra_headers)?;
    expect_101(stream, key_b64).map(|_| ())
}

/// Like [`perform`] but offers `permessage-deflate` extension (RFC 7692).
///
/// Returns `true` if the server confirmed the extension in its 101 response.
/// Uses `client_no_context_takeover; server_no_context_takeover` so each
/// message is independently compressed — no shared zlib state between messages.
pub(crate) fn perform_with_deflate<S: Read + Write>(
    stream:  &mut S,
    host:    &str,
    path:    &str,
    key_b64: &str,
) -> Result<bool> {
    let deflate_ext = [(
        "Sec-WebSocket-Extensions",
        "permessage-deflate; client_no_context_takeover; server_no_context_takeover",
    )];
    send_upgrade_request(stream, host, path, key_b64, &deflate_ext)?;
    expect_101(stream, key_b64)
}

// ── Request building ──────────────────────────────────────────────────────────

fn send_upgrade_request<W: Write>(
    w:             &mut W,
    host:          &str,
    path:          &str,
    key_b64:       &str,
    extra_headers: &[(&str, &str)],
) -> Result<()> {
    let path = if path.is_empty() { "/" } else { path };
    let mut req = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key_b64}\r\n\
         Sec-WebSocket-Version: 13\r\n"
    );
    for (name, value) in extra_headers {
        req.push_str(name);
        req.push_str(": ");
        req.push_str(value);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");

    w.write_all(req.as_bytes())
        .map_err(|e| Error::Network(format!("ws: upgrade write: {e}")))?;
    w.flush()
        .map_err(|e| Error::Network(format!("ws: upgrade flush: {e}")))
}

// ── Response parsing ──────────────────────────────────────────────────────────

/// Parse the 101 response. Returns `true` if the server agreed to permessage-deflate.
fn expect_101<R: Read>(r: &mut R, key_b64: &str) -> Result<bool> {
    let status = read_line(r)?;
    if !status.contains("101") {
        return Err(Error::Network(format!(
            "ws: upgrade failed (expected 101): {}",
            status.trim()
        )));
    }

    let expected = compute_accept(key_b64);
    let mut got_accept = false;
    let mut deflate_negotiated = false;
    loop {
        let line = read_line(r)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(val) = header_value(line, "Sec-WebSocket-Accept")
            && val.trim() == expected
        {
            got_accept = true;
        }
        if let Some(val) = header_value(line, "Sec-WebSocket-Extensions")
            && val.to_ascii_lowercase().contains("permessage-deflate")
        {
            deflate_negotiated = true;
        }
    }

    if !got_accept {
        return Err(Error::Network(
            "ws: missing or invalid Sec-WebSocket-Accept".into(),
        ));
    }
    Ok(deflate_negotiated)
}

/// Read one CRLF-terminated line (≤ 8 KiB) byte-by-byte.
fn read_line<R: Read>(r: &mut R) -> Result<String> {
    let mut line = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        r.read_exact(&mut byte)
            .map_err(|e| Error::Network(format!("ws: read response: {e}")))?;
        line.push(byte[0]);
        if line.ends_with(b"\r\n") {
            break;
        }
        if line.len() > 8192 {
            return Err(Error::Network("ws: response header line too long".into()));
        }
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

/// Extract the value of a header, case-insensitively. E.g. `"foo: bar"` with
/// needle `"Foo"` → `Some("bar")`.
fn header_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let colon = line.find(':')?;
    let (key, val) = line.split_at(colon);
    if key.trim().eq_ignore_ascii_case(name) {
        Some(&val[1..]) // skip ':'
    } else {
        None
    }
}

// ── Sec-WebSocket-Accept computation ─────────────────────────────────────────

/// RFC 6455 §4.1: Accept = base64(SHA-1(key_b64 + WS_GUID))
pub(crate) fn compute_accept(key_b64: &str) -> String {
    let mut input = key_b64.as_bytes().to_vec();
    input.extend_from_slice(WS_GUID);
    base64_encode(&sha1(&input))
}

// ── SHA-1 (RFC 3174) ─────────────────────────────────────────────────────────
// Minimal implementation — used only for WebSocket handshake.

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = h;

        for (i, &wi) in w[..80].iter().enumerate() {
            let (f, k): (u32, u32) = match i {
                0..=19  => ((b & c) | ((!b) & d),          0x5A82_7999),
                20..=39 => (b ^ c ^ d,                      0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d),   0x8F1B_BCDC),
                _       => (b ^ c ^ d,                      0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, &word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ── Base64 (RFC 4648 §4, standard alphabet) ──────────────────────────────────

pub(crate) fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18) & 0x3F] as char);
        out.push(CHARS[(n >> 12) & 0x3F] as char);
        out.push(if chunk.len() > 1 { CHARS[(n >> 6) & 0x3F] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[n & 0x3F] as char } else { '=' });
    }
    out
}

// ── PRNG for key generation ───────────────────────────────────────────────────
// Not cryptographically secure — the spec asks for "unguessable" primarily for
// cache-busting, not security. A proper CSPRNG (getrandom) can be swapped in
// when a JS-facing API (crypto.getRandomValues) is added.

fn random_bytes() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 17))
        .unwrap_or(0xDEAD_BEEF_0000_0000);

    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut out = [0u8; 16];
    for chunk in out.chunks_mut(8) {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let bytes = state.to_le_bytes();
        let n = chunk.len();
        chunk.copy_from_slice(&bytes[..n]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6455 §1.3 — the example key/accept pair from the specification.
    #[test]
    fn accept_matches_rfc_example() {
        // Spec key: "dGhlIHNhbXBsZSBub25jZQ=="
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        // Expected accept: "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        assert_eq!(compute_accept(key), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn sha1_empty_input() {
        // SHA-1("") = da39a3ee5e6b4b0d3255bfef95601890afd80709
        let hash = sha1(b"");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn sha1_abc() {
        // SHA-1("abc") = a9993e364706816aba3e25717850c26c9cd0d89d
        let hash = sha1(b"abc");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn base64_encode_rfc_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn generate_key_is_24_chars() {
        // 16 bytes → 24 base64 chars (with padding).
        let key = generate_key();
        assert_eq!(key.len(), 24, "key: {key}");
    }

    #[test]
    fn generate_key_is_valid_base64() {
        let key = generate_key();
        assert!(
            key.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
            "invalid base64 char in key: {key}"
        );
    }

    #[test]
    fn upgrade_request_contains_required_headers() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let mut buf = Vec::new();
        send_upgrade_request(&mut buf, "echo.example.com", "/chat", key, &[]).unwrap();
        let req = String::from_utf8(buf).unwrap();
        assert!(req.contains("GET /chat HTTP/1.1\r\n"));
        assert!(req.contains("Host: echo.example.com\r\n"));
        assert!(req.contains("Upgrade: websocket\r\n"));
        assert!(req.contains("Connection: Upgrade\r\n"));
        assert!(req.contains(&format!("Sec-WebSocket-Key: {key}\r\n")));
        assert!(req.contains("Sec-WebSocket-Version: 13\r\n"));
        assert!(req.ends_with("\r\n\r\n"));
    }

    #[test]
    fn upgrade_request_default_path_slash() {
        let mut buf = Vec::new();
        send_upgrade_request(&mut buf, "host", "", "key==", &[]).unwrap();
        let req = String::from_utf8(buf).unwrap();
        assert!(req.starts_with("GET / HTTP/1.1\r\n"));
    }

    #[test]
    fn expect_101_ok() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_accept(key);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        let mut cur = std::io::Cursor::new(response.as_bytes());
        // No deflate header → returns false (no deflate negotiated), but not an error.
        assert!(!expect_101(&mut cur, key).unwrap());
    }

    #[test]
    fn expect_101_wrong_status() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let response = "HTTP/1.1 400 Bad Request\r\n\r\n";
        let mut cur = std::io::Cursor::new(response.as_bytes());
        let err = expect_101(&mut cur, key).unwrap_err();
        assert!(err.to_string().contains("400"));
    }

    #[test]
    fn expect_101_bad_accept() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let response = "HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: wrong==\r\n\r\n";
        let mut cur = std::io::Cursor::new(response.as_bytes());
        assert!(expect_101(&mut cur, key).is_err());
    }

    /// Upgrade request with permessage-deflate must include the extension header.
    #[test]
    fn upgrade_request_includes_permessage_deflate_header() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let ext_header = [(
            "Sec-WebSocket-Extensions",
            "permessage-deflate; client_no_context_takeover; server_no_context_takeover",
        )];
        let mut buf = Vec::new();
        send_upgrade_request(&mut buf, "example.com", "/ws", key, &ext_header).unwrap();
        let req = String::from_utf8(buf).unwrap();
        assert!(req.contains("Sec-WebSocket-Extensions:"));
        assert!(req.contains("permessage-deflate"));
        assert!(req.contains("client_no_context_takeover"));
        assert!(req.contains("server_no_context_takeover"));
    }

    /// If the server responds with permessage-deflate in Sec-WebSocket-Extensions, expect_101 returns true.
    #[test]
    fn expect_101_deflate_negotiated() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_accept(key);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept}\r\n\
             Sec-WebSocket-Extensions: permessage-deflate; server_no_context_takeover\r\n\
             \r\n"
        );
        let mut cur = std::io::Cursor::new(response.as_bytes());
        assert!(expect_101(&mut cur, key).unwrap());
    }

    /// If the server does not include the extension, expect_101 returns false (no deflate).
    #[test]
    fn expect_101_deflate_not_negotiated_when_absent() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_accept(key);
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept}\r\n\
             \r\n"
        );
        let mut cur = std::io::Cursor::new(response.as_bytes());
        assert!(!expect_101(&mut cur, key).unwrap());
    }
}
