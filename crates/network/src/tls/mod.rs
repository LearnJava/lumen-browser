//! TLS fingerprinting + per-profile configuration.
//!
//! Implements Chrome-matching TLS ClientHello parameters:
//! - Cipher suite ordering matching Chrome 130 (within what rustls+ring supports).
//!   Chrome includes CBC suites; rustls omits them intentionally (insecure).
//!   The AEAD subset preserves Chrome's relative ordering.
//! - Named groups (key exchange): X25519 → secp256r1 → secp384r1 (Chrome order).
//! - ALPN: h2 before http/1.1 (Standard/Strict); http/1.1 only (Tor).
//!
//! Per-profile configs:
//! - `Standard`: Chrome fingerprint, TLS 1.2 + 1.3.
//! - `Strict`: TLS 1.3 only, same cipher preference order.
//! - `Tor`: TLS 1.3 only, X25519-only, no h2 ALPN.

pub mod fingerprint;

pub use fingerprint::{
    CertInfo, ChromeJa3Snapshot, JA4ChromeSnapshot, TlsHandshakeInfo,
    CHROME_130_JA3_SNAPSHOT, CHROME_130_JA4_SNAPSHOT,
};

use std::sync::Arc;
use rustls::ClientConfig;

use crate::http::HttpProfile;

/// TLS fingerprint profile — controls cipher suites, kx_groups, ALPN, and
/// protocol versions offered in the TLS ClientHello.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsProfile {
    /// Chrome-matching profile: TLS 1.2 + 1.3, AEAD cipher preference,
    /// X25519 → secp256r1 → secp384r1, ALPN h2 + http/1.1.
    Standard,
    /// Strict private profile: TLS 1.3 only, same cipher preference, same
    /// kx_groups, same ALPN. Rejects servers that don't support TLS 1.3.
    Strict,
    /// Tor-compatible profile: TLS 1.3 only, X25519-only kx_group,
    /// no h2 ALPN (Tor exit nodes don't bridge HTTP/2).
    Tor,
}

/// Map an `HttpProfile` to the corresponding `TlsProfile`.
///
/// - `Strict` → `TlsProfile::Strict` (TLS 1.3 only, no legacy cipher suites)
/// - `TorBrowser` → `TlsProfile::Tor` (minimal extension set, no h2)
/// - All others → `TlsProfile::Standard` (Chrome-matching)
pub fn http_to_tls_profile(http: HttpProfile) -> TlsProfile {
    match http {
        HttpProfile::Strict => TlsProfile::Strict,
        HttpProfile::TorBrowser => TlsProfile::Tor,
        _ => TlsProfile::Standard,
    }
}

/// Build a `ClientConfig` for the given `TlsProfile`.
///
/// Uses `ClientConfig::builder_with_provider` to explicitly set:
/// - Cipher suite order matching Chrome 130 (AEAD-only subset; CBC suites
///   that Chrome includes are intentionally omitted by rustls as insecure).
/// - Named groups (kx_groups): X25519, secp256r1, secp384r1 in Chrome order.
/// - Protocol versions: TLS 1.2 + 1.3 for Standard, TLS 1.3 only for
///   Strict and Tor.
/// - ALPN: h2 + http/1.1 for Standard/Strict; http/1.1 only for Tor.
pub fn build_client_config(profile: TlsProfile, root_store: rustls::RootCertStore) -> ClientConfig {
    use rustls::crypto::aws_lc_rs as crypto;

    let mut provider = crypto::default_provider();

    // Chrome 130 AEAD cipher preference (TLS 1.3 suites first, then TLS 1.2 ECDHE).
    // Order: AES-128-GCM → AES-256-GCM → CHACHA20 for both TLS versions,
    // ECDSA before RSA within each cipher family (Chrome's actual order).
    // CBC suites Chrome includes are omitted — insecure and not supported by aws-lc-rs.
    let chrome_aead_ciphers: Vec<rustls::SupportedCipherSuite> = vec![
        // TLS 1.3 (Chrome prefers AES-128-GCM first)
        crypto::cipher_suite::TLS13_AES_128_GCM_SHA256,
        crypto::cipher_suite::TLS13_AES_256_GCM_SHA384,
        crypto::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
        // TLS 1.2 AEAD (ECDSA before RSA, AES-128 before AES-256 before CHACHA20)
        crypto::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        crypto::cipher_suite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        crypto::cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
        crypto::cipher_suite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        crypto::cipher_suite::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
        crypto::cipher_suite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
    ];

    // Chrome 130 kx_group order: X25519 (preferred), secp256r1, secp384r1.
    let chrome_kx_groups: Vec<&'static dyn rustls::crypto::SupportedKxGroup> = vec![
        crypto::kx_group::X25519,
        crypto::kx_group::SECP256R1,
        crypto::kx_group::SECP384R1,
    ];

    match profile {
        TlsProfile::Standard => {
            provider.cipher_suites = chrome_aead_ciphers;
            provider.kx_groups = chrome_kx_groups;
        }
        TlsProfile::Strict => {
            // TLS 1.3 only: omit TLS 1.2 cipher suites entirely.
            provider.cipher_suites = vec![
                crypto::cipher_suite::TLS13_AES_128_GCM_SHA256,
                crypto::cipher_suite::TLS13_AES_256_GCM_SHA384,
                crypto::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
            ];
            provider.kx_groups = chrome_kx_groups;
        }
        TlsProfile::Tor => {
            // Minimal: TLS 1.3 only, X25519 key exchange only.
            provider.cipher_suites = vec![
                crypto::cipher_suite::TLS13_AES_128_GCM_SHA256,
                crypto::cipher_suite::TLS13_AES_256_GCM_SHA384,
                crypto::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
            ];
            provider.kx_groups = vec![crypto::kx_group::X25519];
        }
    }

    let versions: &[&rustls::SupportedProtocolVersion] = match profile {
        TlsProfile::Standard => &[&rustls::version::TLS13, &rustls::version::TLS12],
        TlsProfile::Strict | TlsProfile::Tor => &[&rustls::version::TLS13],
    };

    let mut cfg = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(versions)
        .expect("protocol versions valid for the configured cipher suites")
        .with_root_certificates(root_store)
        .with_no_client_auth();

    cfg.alpn_protocols = match profile {
        TlsProfile::Standard | TlsProfile::Strict => {
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        }
        TlsProfile::Tor => vec![b"http/1.1".to_vec()],
    };

    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_root_store() -> rustls::RootCertStore {
        rustls::RootCertStore::empty()
    }

    #[test]
    fn standard_profile_has_h2_alpn() {
        let cfg = build_client_config(TlsProfile::Standard, empty_root_store());
        assert_eq!(cfg.alpn_protocols, vec![b"h2".to_vec(), b"http/1.1".to_vec()]);
    }

    #[test]
    fn strict_profile_has_h2_alpn() {
        let cfg = build_client_config(TlsProfile::Strict, empty_root_store());
        assert_eq!(cfg.alpn_protocols[0], b"h2");
    }

    #[test]
    fn tor_profile_http11_only() {
        let cfg = build_client_config(TlsProfile::Tor, empty_root_store());
        assert_eq!(cfg.alpn_protocols, vec![b"http/1.1".to_vec()]);
    }

    #[test]
    fn tls_profile_eq() {
        assert_eq!(TlsProfile::Standard, TlsProfile::Standard);
        assert_ne!(TlsProfile::Standard, TlsProfile::Strict);
        assert_ne!(TlsProfile::Strict, TlsProfile::Tor);
    }

    #[test]
    fn http_to_tls_profile_mapping() {
        assert_eq!(http_to_tls_profile(HttpProfile::Chrome), TlsProfile::Standard);
        assert_eq!(http_to_tls_profile(HttpProfile::Firefox), TlsProfile::Standard);
        assert_eq!(http_to_tls_profile(HttpProfile::Safari), TlsProfile::Standard);
        assert_eq!(http_to_tls_profile(HttpProfile::Edge), TlsProfile::Standard);
        assert_eq!(http_to_tls_profile(HttpProfile::Lumen), TlsProfile::Standard);
        assert_eq!(http_to_tls_profile(HttpProfile::Strict), TlsProfile::Strict);
        assert_eq!(http_to_tls_profile(HttpProfile::TorBrowser), TlsProfile::Tor);
    }

    #[test]
    fn all_tls_profiles_buildable() {
        for profile in &[TlsProfile::Standard, TlsProfile::Strict, TlsProfile::Tor] {
            let _ = build_client_config(*profile, empty_root_store());
        }
    }
}
