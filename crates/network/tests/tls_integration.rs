//! Integration tests for TLS profile configuration.
//!
//! Tests verify that TLS profiles can be configured and used for different
//! fingerprinting strategies (Standard, Strict, Tor).

use lumen_network::tls::{TlsProfile, TlsHandshakeInfo, CHROME_130_JA3_SNAPSHOT};

#[test]
fn test_tls_profile_creation() {
    // Verify all profiles can be created
    let _standard = TlsProfile::Standard;
    let _strict = TlsProfile::Strict;
    let _tor = TlsProfile::Tor;
}

#[test]
fn test_tls_profile_eq() {
    // Test profile comparison
    assert_eq!(TlsProfile::Standard, TlsProfile::Standard);
    assert_ne!(TlsProfile::Standard, TlsProfile::Strict);
    assert_ne!(TlsProfile::Strict, TlsProfile::Tor);
}

#[test]
fn test_ja3_handshake_info_from_chrome_snapshot() {
    // Test building JA3 handshake info from Chrome snapshot
    // Note: JA3 format is TLSVersion,Ciphers,Extensions,Groups,ECPointFormats
    // where Ciphers/Extensions/Groups/ECPointFormats are comma-separated numbers.
    // So when parsed, parts[1] will contain the single cipher suite (4865),
    // parts[2] will contain all extensions (10,45,13,11,5,16,0,23,65281), etc.
    let info = TlsHandshakeInfo {
        tls_version: CHROME_130_JA3_SNAPSHOT.tls_version,
        cipher_suite: CHROME_130_JA3_SNAPSHOT.expected_cipher_suites[0],
        extensions: CHROME_130_JA3_SNAPSHOT.expected_extensions.to_vec(),
        named_groups: CHROME_130_JA3_SNAPSHOT.expected_named_groups.to_vec(),
        ec_point_formats: CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.to_vec(),
        signature_algorithms: vec![],
    };

    let ja3_string = info.ja3_string();

    // Verify JA3 format: TLSVersion,Cipher,Extensions,Groups,ECFormats
    // When split by comma, we get more parts because Extensions/Groups are themselves comma-separated
    assert!(ja3_string.contains("772"), "Should contain TLS 1.3 version (772)");
    assert!(ja3_string.contains("4865"), "Should contain first cipher suite");
    assert!(ja3_string.starts_with("772,"), "Should start with TLS version");
}

#[test]
fn test_chrome_130_ja3_snapshot_has_valid_parameters() {
    // Verify Chrome snapshot has reasonable TLS parameters
    assert_eq!(CHROME_130_JA3_SNAPSHOT.tls_version, 772, "Chrome 130 uses TLS 1.3");

    // Should have at least 3 cipher suites
    assert!(
        CHROME_130_JA3_SNAPSHOT.expected_cipher_suites.len() >= 3,
        "Chrome should offer multiple cipher suites"
    );

    // Should have at least 5 extensions
    assert!(
        CHROME_130_JA3_SNAPSHOT.expected_extensions.len() >= 5,
        "Chrome should include multiple extensions"
    );

    // Should have at least 3 named groups
    assert!(
        CHROME_130_JA3_SNAPSHOT.expected_named_groups.len() >= 3,
        "Chrome should support multiple elliptic curves"
    );

    // EC point formats should have uncompressed (0)
    assert!(
        CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.contains(&0),
        "Chrome should support uncompressed EC point format"
    );
}
