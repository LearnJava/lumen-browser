//! Integration tests for TLS profile configuration and fingerprinting (9B).
//!
//! Tests verify that TLS profiles can be configured, that `TlsProfile` is
//! correctly derived from `HttpProfile`, and that JA3/JA4_r snapshot data
//! matches Chrome 130 reference values.

use lumen_network::tls::{
    TlsProfile, TlsHandshakeInfo, CHROME_130_JA3_SNAPSHOT, CHROME_130_JA4_SNAPSHOT,
    fingerprint::CHROME_130_SIG_ALGORITHMS,
};
use lumen_network::{HttpClient, HttpProfile, http_to_tls_profile};
use rustls::SignatureScheme;

fn chrome_130_handshake() -> TlsHandshakeInfo {
    TlsHandshakeInfo {
        legacy_version: 771,        // TLS 1.2 placeholder (JA3)
        max_supported_version: 772, // TLS 1.3 from supported_versions (JA4)
        cipher_suites: CHROME_130_JA3_SNAPSHOT.expected_cipher_suites.to_vec(),
        extensions: CHROME_130_JA3_SNAPSHOT.expected_extensions.to_vec(),
        named_groups: CHROME_130_JA3_SNAPSHOT.expected_named_groups.to_vec(),
        ec_point_formats: CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.to_vec(),
        signature_algorithms: CHROME_130_SIG_ALGORITHMS
            .iter()
            .map(|&v| SignatureScheme::from(v))
            .collect(),
        alpn_protocols: vec!["h2".to_owned(), "http/1.1".to_owned()],
        has_sni: true,
    }
}

// ── TlsProfile tests ──────────────────────────────────────────────────────────

#[test]
fn tls_profile_all_variants_constructible() {
    let _s = TlsProfile::Standard;
    let _st = TlsProfile::Strict;
    let _t = TlsProfile::Tor;
}

#[test]
fn tls_profile_eq() {
    assert_eq!(TlsProfile::Standard, TlsProfile::Standard);
    assert_ne!(TlsProfile::Standard, TlsProfile::Strict);
    assert_ne!(TlsProfile::Strict, TlsProfile::Tor);
}

// ── HttpProfile → TlsProfile mapping ─────────────────────────────────────────

#[test]
fn http_to_tls_chrome_is_standard() {
    assert_eq!(http_to_tls_profile(HttpProfile::Chrome), TlsProfile::Standard);
}

#[test]
fn http_to_tls_firefox_is_standard() {
    assert_eq!(http_to_tls_profile(HttpProfile::Firefox), TlsProfile::Standard);
}

#[test]
fn http_to_tls_strict_is_strict() {
    assert_eq!(http_to_tls_profile(HttpProfile::Strict), TlsProfile::Strict);
}

#[test]
fn http_to_tls_tor_is_tor() {
    assert_eq!(http_to_tls_profile(HttpProfile::TorBrowser), TlsProfile::Tor);
}

// ── HttpClient TLS profile wiring ─────────────────────────────────────────────

#[test]
fn httpclient_default_tls_profile_is_standard() {
    let client = HttpClient::new();
    assert_eq!(client.tls_profile(), TlsProfile::Standard);
}

#[test]
fn httpclient_with_fingerprint_profile_strict_sets_tls_strict() {
    let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Strict);
    assert_eq!(client.tls_profile(), TlsProfile::Strict);
}

#[test]
fn httpclient_with_fingerprint_profile_tor_sets_tls_tor() {
    let client = HttpClient::new().with_fingerprint_profile(HttpProfile::TorBrowser);
    assert_eq!(client.tls_profile(), TlsProfile::Tor);
}

#[test]
fn httpclient_with_tls_profile_override() {
    let client = HttpClient::new()
        .with_fingerprint_profile(HttpProfile::Chrome)
        .with_tls_profile(TlsProfile::Strict);
    assert_eq!(client.fingerprint_profile(), HttpProfile::Chrome);
    assert_eq!(client.tls_profile(), TlsProfile::Strict);
}

#[test]
fn httpclient_tls_profile_reset_by_http_profile() {
    // Setting HTTP profile resets TLS profile to derived value
    let client = HttpClient::new()
        .with_tls_profile(TlsProfile::Strict)
        .with_fingerprint_profile(HttpProfile::Chrome);
    assert_eq!(client.tls_profile(), TlsProfile::Standard);
}

// ── Chrome 130 JA3 snapshot tests ─────────────────────────────────────────────

#[test]
fn chrome_130_ja3_snapshot_15_cipher_suites() {
    assert_eq!(CHROME_130_JA3_SNAPSHOT.expected_cipher_suites.len(), 15);
}

#[test]
fn chrome_130_ja3_snapshot_16_extensions() {
    assert_eq!(CHROME_130_JA3_SNAPSHOT.expected_extensions.len(), 16);
}

#[test]
fn chrome_130_ja3_snapshot_named_groups_order() {
    let groups = CHROME_130_JA3_SNAPSHOT.expected_named_groups;
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0], 29, "X25519 first");
    assert_eq!(groups[1], 23, "secp256r1 second");
    assert_eq!(groups[2], 24, "secp384r1 third");
}

#[test]
fn chrome_130_ja3_raw_uses_legacy_version() {
    let info = chrome_130_handshake();
    assert!(info.ja3_raw_string().starts_with("771,"));
}

#[test]
fn chrome_130_ja3_raw_cipher_count() {
    let info = chrome_130_handshake();
    let s = info.ja3_raw_string();
    let sections: Vec<&str> = s.splitn(6, ',').collect();
    assert!(sections.len() >= 5);
    assert_eq!(sections[1].split('-').count(), 15);
}

// ── Chrome 130 JA4 snapshot tests ─────────────────────────────────────────────

#[test]
fn chrome_130_ja4_snapshot_cipher_count() {
    assert_eq!(CHROME_130_JA4_SNAPSHOT.cipher_count, 15);
}

#[test]
fn chrome_130_ja4_snapshot_extension_count() {
    assert_eq!(CHROME_130_JA4_SNAPSHOT.extension_count, 16);
}

#[test]
fn chrome_130_ja4_snapshot_sorted_ciphers_ascending() {
    let s = CHROME_130_JA4_SNAPSHOT.sorted_cipher_suites;
    for w in s.windows(2) {
        assert!(w[0] <= w[1], "sorted_cipher_suites must be ascending: {w:?}");
    }
}

#[test]
fn chrome_130_ja4_snapshot_sorted_exts_ascending() {
    let s = CHROME_130_JA4_SNAPSHOT.sorted_extensions;
    for w in s.windows(2) {
        assert!(w[0] <= w[1], "sorted_extensions must be ascending: {w:?}");
    }
}

#[test]
fn chrome_130_ja4_snapshot_no_sni_or_alpn_in_exts() {
    let exts = CHROME_130_JA4_SNAPSHOT.sorted_extensions;
    assert!(!exts.contains(&0), "SNI (0) must not be in JA4 sorted_extensions");
    assert!(!exts.contains(&16), "ALPN (16) must not be in JA4 sorted_extensions");
}

#[test]
fn chrome_130_ja4_raw_prefix() {
    let info = chrome_130_handshake();
    let s = info.ja4_raw_string();
    assert!(s.starts_with("t13d0f10h2_"), "JA4_r prefix: {s}");
}

#[test]
fn chrome_130_ja4_raw_cipher_section_sorted_and_correct_count() {
    let info = chrome_130_handshake();
    let s = info.ja4_raw_string();
    let cipher_section = s.split('_').nth(1).unwrap_or("");
    let codes: Vec<u16> = cipher_section
        .split(',')
        .filter_map(|h| u16::from_str_radix(h, 16).ok())
        .collect();
    assert_eq!(codes.len(), 15);
    for w in codes.windows(2) {
        assert!(w[0] <= w[1], "cipher section must be ascending: {w:?}");
    }
}

#[test]
fn chrome_130_ja4_raw_ext_section_excludes_sni_alpn() {
    let info = chrome_130_handshake();
    let s = info.ja4_raw_string();
    let ext_and_sig = s.split('_').nth(2).unwrap_or("");
    let ext_part = ext_and_sig.split(',').next().unwrap_or("");
    let codes: Vec<&str> = ext_part.split(',').collect();
    assert!(!codes.contains(&"0000"), "SNI must not appear in JA4_r ext section");
    assert!(!codes.contains(&"0010"), "ALPN must not appear in JA4_r ext section");
}
