//! Local-automation token authentication (ADR-024 §Access model, DEVX-15).
//!
//! `--mcp-port`/`--mcp-live-port`/`--bidi-port`/`--ipc-server` bind
//! `127.0.0.1` only, but loopback-only is not the same as private — any local
//! process can connect. A per-run token, printed to stderr next to the port
//! line and required on the first message of every connection, closes that
//! gap without giving up the "no separate setup step" property that makes
//! these ports usable for scripted automation in the first place. No
//! escape hatch (`--mcp-allow-anonymous` was explicitly rejected, ADR-024 Q1)
//! — every consumer, including our own tooling, must read and send the token.
//!
//! stdio-mode MCP (no `--port`) does not use this module: only the process
//! that spawned it can reach its stdin/stdout, so a token adds nothing there.

/// Generate a fresh per-run authentication token.
///
/// 32 hex characters (128 bits) from the OS CSPRNG — same primitive already
/// used for key generation elsewhere in the workspace (`crates/js/src/subtle_crypto.rs`,
/// `crates/network/src/webauthn.rs`).
pub fn generate_token() -> String {
    let mut buf = [0u8; 16];
    getrandom::getrandom(&mut buf).expect("OS CSPRNG unavailable");
    let mut out = String::with_capacity(32);
    for byte in buf {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Constant-time token comparison.
///
/// Ordinary `==` short-circuits on the first mismatched byte, which leaks the
/// length of the matching prefix through response timing. The tokens here are
/// fixed-length local secrets rather than a webscale attack surface, but the
/// comparison is cheap enough that there is no reason not to close the gap.
pub fn tokens_match(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    if expected.len() != provided.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (a, b) in expected.iter().zip(provided.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_is_32_hex_chars() {
        let token = generate_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_token_is_not_constant() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b, "two calls produced the same token — CSPRNG not wired up");
    }

    #[test]
    fn tokens_match_identical() {
        assert!(tokens_match("abc123", "abc123"));
    }

    #[test]
    fn tokens_match_rejects_mismatch() {
        assert!(!tokens_match("abc123", "abc124"));
    }

    #[test]
    fn tokens_match_rejects_different_length() {
        assert!(!tokens_match("abc123", "abc1234"));
        assert!(!tokens_match("abc123", "abc12"));
    }

    #[test]
    fn tokens_match_rejects_empty_against_nonempty() {
        assert!(!tokens_match("", "abc123"));
        assert!(!tokens_match("abc123", ""));
    }
}
