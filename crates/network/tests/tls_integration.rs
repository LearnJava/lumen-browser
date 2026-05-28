//! Integration tests for TLS profile configuration.

use lumen_network::tls::{TlsProfile, TlsHandshakeInfo, CHROME_130_JA3_SNAPSHOT};

#[test]
fn test_tls_profile_creation() {
    let _standard = TlsProfile::Standard;
    let _strict = TlsProfile::Strict;
    let _tor = TlsProfile::Tor;
}

#[test]
fn test_tls_profile_eq() {
    assert_eq!(TlsProfile::Standard, TlsProfile::Standard);
    assert_ne!(TlsProfile::Standard, TlsProfile::Strict);
}

#[test]
fn test_ja3_handshake_from_chrome_snapshot() {
    let info = TlsHandshakeInfo {
        tls_version: CHROME_130_JA3_SNAPSHOT.tls_version,
        cipher_suite: CHROME_130_JA3_SNAPSHOT.expected_cipher_suites[0],
        extensions: CHROME_130_JA3_SNAPSHOT.expected_extensions.to_vec(),
        named_groups: CHROME_130_JA3_SNAPSHOT.expected_named_groups.to_vec(),
        ec_point_formats: CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.to_vec(),
        signature_algorithms: vec![],
    };

    let ja3_string = info.ja3_string();
    assert!(ja3_string.contains("772"));
    assert!(ja3_string.contains("4865"));
    assert!(ja3_string.starts_with("772,"));
}

#[test]
fn test_chrome_130_ja3_snapshot_has_valid_parameters() {
    assert_eq!(CHROME_130_JA3_SNAPSHOT.tls_version, 772);
    assert!(CHROME_130_JA3_SNAPSHOT.expected_cipher_suites.len() >= 3);
    assert!(CHROME_130_JA3_SNAPSHOT.expected_extensions.len() >= 5);
    assert!(CHROME_130_JA3_SNAPSHOT.expected_named_groups.len() >= 3);
    assert!(CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.contains(&0));
}
