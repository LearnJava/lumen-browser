/// SRI (Subresource Integrity) implementation per W3C SRI Level 1.
///
/// Parses `integrity` attribute tokens like `sha256-BASE64 sha512-BASE64`
/// and verifies that a response body matches at least one token with the
/// strongest listed algorithm.
use sha2::{Digest, Sha256, Sha384, Sha512};

/// Hash algorithm accepted in the `integrity` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SriAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

/// One parsed token from an `integrity` string.
pub struct SriToken {
    pub algorithm: SriAlgorithm,
    /// Raw hash bytes (decoded from base64).
    pub hash: Vec<u8>,
}

/// Parses a space-separated list of integrity tokens.
///
/// Unknown algorithm prefixes are silently skipped per the SRI spec.
/// Returns an empty vec if no valid tokens are found.
pub fn parse_integrity_metadata(integrity: &str) -> Vec<SriToken> {
    let mut tokens = Vec::new();
    for token in integrity.split_ascii_whitespace() {
        // Strip optional options (e.g. "sha256-BASE64?foo=bar")
        let token = token.split('?').next().unwrap_or(token);
        let Some((alg_str, hash_b64)) = token.split_once('-') else {
            continue;
        };
        let algorithm = match alg_str {
            "sha256" => SriAlgorithm::Sha256,
            "sha384" => SriAlgorithm::Sha384,
            "sha512" => SriAlgorithm::Sha512,
            _ => continue,
        };
        let Some(hash) = b64_decode(hash_b64) else {
            continue;
        };
        tokens.push(SriToken { algorithm, hash });
    }
    tokens
}

/// Returns `true` if `body` passes the SRI check encoded in `integrity`.
///
/// Rules (W3C SRI §3.3.5):
/// - Empty or absent `integrity` → always pass.
/// - All tokens use unknown algorithms → pass (forward-compatibility).
/// - Otherwise pick the strongest algorithm present; body must match at
///   least one token with that algorithm.
pub fn check_sri(body: &[u8], integrity: &str) -> bool {
    if integrity.is_empty() {
        return true;
    }
    let tokens = parse_integrity_metadata(integrity);
    if tokens.is_empty() {
        // No recognised tokens — unknown algorithms, spec says pass.
        return true;
    }
    let strongest = tokens.iter().map(|t| t.algorithm).max().unwrap();
    for token in tokens.iter().filter(|t| t.algorithm == strongest) {
        let digest: Vec<u8> = match token.algorithm {
            SriAlgorithm::Sha256 => Sha256::digest(body).to_vec(),
            SriAlgorithm::Sha384 => Sha384::digest(body).to_vec(),
            SriAlgorithm::Sha512 => Sha512::digest(body).to_vec(),
        };
        if digest == token.hash {
            return true;
        }
    }
    false
}

/// Minimal base64 decoder that handles both standard (`+/`) and URL-safe (`-_`)
/// alphabets and accepts optional `=` padding.
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for ch in s.bytes() {
        let v: u32 = match ch {
            b'A'..=b'Z' => (ch - b'A') as u32,
            b'a'..=b'z' => (ch - b'a' + 26) as u32,
            b'0'..=b'9' => (ch - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256, Sha384, Sha512};

    fn sha256_b64(data: &[u8]) -> String {
        let hash = Sha256::digest(data);
        b64_encode(&hash)
    }

    fn sha384_b64(data: &[u8]) -> String {
        let hash = Sha384::digest(data);
        b64_encode(&hash)
    }

    fn sha512_b64(data: &[u8]) -> String {
        let hash = Sha512::digest(data);
        b64_encode(&hash)
    }

    fn b64_encode(data: &[u8]) -> String {
        const ALPHA: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as usize;
            let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
            out.push(ALPHA[b0 >> 2] as char);
            out.push(ALPHA[((b0 & 3) << 4) | (b1 >> 4)] as char);
            out.push(if chunk.len() > 1 { ALPHA[((b1 & 0xf) << 2) | (b2 >> 6)] as char } else { '=' });
            out.push(if chunk.len() > 2 { ALPHA[b2 & 0x3f] as char } else { '=' });
        }
        out
    }

    #[test]
    fn sri_sha256_match() {
        let body = b"hello world";
        let integrity = format!("sha256-{}", sha256_b64(body));
        assert!(check_sri(body, &integrity));
    }

    #[test]
    fn sri_sha256_mismatch() {
        let body = b"hello world";
        let wrong_integrity = format!("sha256-{}", sha256_b64(b"other content"));
        assert!(!check_sri(body, &wrong_integrity));
    }

    #[test]
    fn sri_sha384_match() {
        let body = b"test data for sha384";
        let integrity = format!("sha384-{}", sha384_b64(body));
        assert!(check_sri(body, &integrity));
    }

    #[test]
    fn sri_sha512_match() {
        let body = b"test data for sha512";
        let integrity = format!("sha512-{}", sha512_b64(body));
        assert!(check_sri(body, &integrity));
    }

    #[test]
    fn sri_unknown_algorithm_passes() {
        // Unknown algorithms must not block the fetch (forward-compatibility).
        assert!(check_sri(b"anything", "sha9999-YWJj"));
        assert!(check_sri(b"anything", "md5-YWJj"));
    }

    #[test]
    fn sri_multiple_hashes_strongest_wins() {
        let body = b"resource body";
        // sha256 is wrong, sha512 is correct — strongest (sha512) must pass.
        let bad_sha256 = format!("sha256-{}", sha256_b64(b"wrong"));
        let good_sha512 = format!("sha512-{}", sha512_b64(body));
        let integrity = format!("{bad_sha256} {good_sha512}");
        assert!(check_sri(body, &integrity));

        // Both sha512 are wrong — must fail.
        let bad_sha512 = format!("sha512-{}", sha512_b64(b"wrong"));
        let integrity2 = format!("{bad_sha256} {bad_sha512}");
        assert!(!check_sri(body, &integrity2));
    }
}
