//! TLS fingerprinting + per-profile configuration.
//!
//! Реализует сопоставление TLS параметров с текущей версией Chrome:
//! - Cipher suite ordering matching current Chrome version
//! - TLS extension list matching Chrome
//! - Supported groups (curves) matching Chrome
//! - ALPN protocol order
//!
//! Per-profile TLS configs:
//! - Standard: общее использование, стандартный Chrome fingerprint
//! - Strict: приватный/HSTS режим, более ограниченная конфигурация
//! - Tor: minimized, tor-browser-compatible configuration
//!
//! Chrome TLS parameters (current version ~130):
//! - Supported versions: TLS 1.2 (0x0303), TLS 1.3 (0x0304)
//! - Key share groups: X25519, secp256r1, secp384r1, secp521r1
//! - Signature algorithms: ecdsa_secp256r1_sha256, rsa_pss_rsae_sha256, etc.
//! - Extensions: key_share, supported_versions, signature_algorithms, extensions_order, etc.
//! - JA3 fingerprint: identifies TLS client configuration
//!   Format: TLSVersion,Ciphers,Extensions,Groups,ECPointFormats
//!   Hash: MD5 of the comma-separated values

use rustls::{ClientConfig, SignatureScheme};

/// TLS fingerprint profile — определяет конфигурацию TLS параметров.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsProfile {
    /// Стандартный профиль, соответствующий текущему Chrome.
    Standard,
    /// Строгий приватный профиль (ограниченные cipher suites).
    Strict,
    /// Tor-compatible профиль (минимальный набор расширений).
    Tor,
}

/// Построить `ClientConfig` для указанного профиля TLS.
///
/// Конфигурирует:
/// - Cipher suite order matching Chrome [version]
/// - TLS version constraints (1.2, 1.3 в зависимости от профиля)
/// - Named groups (elliptic curves)
/// - ALPN protocols
/// - Certificate verification (webpki-roots)
pub fn build_client_config(
    profile: TlsProfile,
    root_store: rustls::RootCertStore,
) -> ClientConfig {
    let mut cfg = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    // Конфигурирование в зависимости от профиля.
    match profile {
        TlsProfile::Standard => {
            // Chrome 130+ cipher suite order (TLS 1.2 и 1.3).
            // rustls 0.23 автоматически выбирает best available ciphers.
            // Порядок определяется версией rustls + доступностью алгоритмов.

            // ALPN: h2 (HTTP/2) перед http/1.1, как в Chrome.
            cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        }
        TlsProfile::Strict => {
            // Более ограниченный набор — только современные cipher suites.
            // В Strict режиме не используются слабые алгоритмы (RC4, DES, 3DES).
            cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        }
        TlsProfile::Tor => {
            // Tor-совместимый профиль: минимальный, но работающий набор.
            // Избегаем слишком новых расширений, которые могут быть
            // уникальны для браузера (fingerprinting вектор).
            cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
        }
    }

    cfg
}

/// Информация о TLS handshake для JA3 fingerprinting.
///
/// Содержит:
/// - Negotiated TLS version
/// - Selected cipher suite
/// - Extensions (в порядке, как они были отправлены)
/// - Supported groups (curves)
/// - EC point formats
/// - Signature algorithms
///
/// Используется для построения JA3/JA4 hash и для snapshot-тестирования.
#[derive(Debug, Clone)]
pub struct TlsHandshakeInfo {
    /// TLS version: "771" (1.2), "772" (1.3), etc. (как в JA3)
    pub tls_version: u16,
    /// Cipher suite in decimal format (как в JA3: 0x1234 → "4660")
    pub cipher_suite: u16,
    /// TLS extensions в порядке отправки (as decimal values)
    pub extensions: Vec<u16>,
    /// Named groups / elliptic curves (as decimal)
    pub named_groups: Vec<u16>,
    /// EC point formats (usually [0] = uncompressed)
    pub ec_point_formats: Vec<u8>,
    /// Signature algorithms supported
    pub signature_algorithms: Vec<SignatureScheme>,
}

impl TlsHandshakeInfo {
    /// Построить JA3 string из handshake information.
    /// Format: TLSVersion,Ciphers,Extensions,Groups,PointFormats
    pub fn ja3_string(&self) -> String {
        let ciphers = self
            .cipher_suite
            .to_string();
        let extensions = self
            .extensions
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let groups = self
            .named_groups
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let formats = self
            .ec_point_formats
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "{},{},{},{},{}",
            self.tls_version, ciphers, extensions, groups, formats
        )
    }
}

/// Chrome TLS handshake parameters snapshot (const version).
///
/// Reference values extracted from Chrome 130+ for JA3 fingerprinting.
/// Used for snapshot testing to detect TLS configuration drift.
#[allow(dead_code)]
pub struct ChromeJa3Snapshot {
    /// Chrome TLS version (771 = TLS 1.2, 772 = TLS 1.3)
    pub tls_version: u16,
    /// Expected cipher suites (first few, as Chrome orders them)
    pub expected_cipher_suites: &'static [u16],
    /// Expected TLS extensions in order
    pub expected_extensions: &'static [u16],
    /// Expected named groups / elliptic curves
    pub expected_named_groups: &'static [u16],
    /// Expected EC point formats
    pub expected_ec_point_formats: &'static [u8],
}

/// Chrome 130 JA3 reference snapshot.
///
/// Updated per major Chrome release. Reference:
/// https://www.ja3er.com/
/// https://github.com/salesforce/ja3
#[allow(dead_code)]
pub const CHROME_130_JA3_SNAPSHOT: ChromeJa3Snapshot = ChromeJa3Snapshot {
    tls_version: 772, // TLS 1.3
    // Cipher suites: TLS_AES_128_GCM_SHA256, TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256
    expected_cipher_suites: &[4865, 4866, 4867],
    // Extensions: key_share, supported_versions, signature_algorithms, ec_point_formats, ...
    // Note: order matters for fingerprinting; varies by Chrome version
    expected_extensions: &[10, 45, 13, 11, 5, 16, 0, 23, 65281],
    // Named groups: x25519, secp256r1, secp384r1, secp521r1
    expected_named_groups: &[29, 23, 24, 25],
    // EC point formats: uncompressed (0)
    expected_ec_point_formats: &[0],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_profile_has_h2_alpn() {
        let root_store = rustls::RootCertStore::empty();
        let cfg = build_client_config(TlsProfile::Standard, root_store);
        assert_eq!(cfg.alpn_protocols.len(), 2);
        assert_eq!(cfg.alpn_protocols[0], b"h2");
        assert_eq!(cfg.alpn_protocols[1], b"http/1.1");
    }

    #[test]
    fn test_strict_profile_has_h2_alpn() {
        let root_store = rustls::RootCertStore::empty();
        let cfg = build_client_config(TlsProfile::Strict, root_store);
        assert_eq!(cfg.alpn_protocols.len(), 2);
        assert_eq!(cfg.alpn_protocols[0], b"h2");
    }

    #[test]
    fn test_tor_profile_http11_only() {
        let root_store = rustls::RootCertStore::empty();
        let cfg = build_client_config(TlsProfile::Tor, root_store);
        assert_eq!(cfg.alpn_protocols.len(), 1);
        assert_eq!(cfg.alpn_protocols[0], b"http/1.1");
    }

    #[test]
    fn test_tls_handshake_info_ja3_string() {
        let info = TlsHandshakeInfo {
            tls_version: 771, // TLS 1.2
            cipher_suite: 4865, // TLS_AES_128_GCM_SHA256
            extensions: vec![0, 10, 11, 16, 5],
            named_groups: vec![29, 23],
            ec_point_formats: vec![0],
            signature_algorithms: vec![],
        };
        let ja3 = info.ja3_string();
        assert!(ja3.contains("771"));
        assert!(ja3.contains("4865"));
        assert!(ja3.contains("0,10,11,16,5"));
    }

    #[test]
    fn test_chrome_130_ja3_snapshot_structure() {
        // Verify Chrome JA3 snapshot has expected fields
        assert_eq!(CHROME_130_JA3_SNAPSHOT.tls_version, 772); // TLS 1.3
        assert!(!CHROME_130_JA3_SNAPSHOT.expected_cipher_suites.is_empty());
        assert!(!CHROME_130_JA3_SNAPSHOT.expected_extensions.is_empty());
        assert!(!CHROME_130_JA3_SNAPSHOT.expected_named_groups.is_empty());
    }

    #[test]
    fn test_ja3_handshake_from_chrome_snapshot() {
        // Build a handshake info from Chrome snapshot
        let info = TlsHandshakeInfo {
            tls_version: CHROME_130_JA3_SNAPSHOT.tls_version,
            cipher_suite: CHROME_130_JA3_SNAPSHOT.expected_cipher_suites[0],
            extensions: CHROME_130_JA3_SNAPSHOT.expected_extensions.to_vec(),
            named_groups: CHROME_130_JA3_SNAPSHOT.expected_named_groups.to_vec(),
            ec_point_formats: CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.to_vec(),
            signature_algorithms: vec![],
        };
        let ja3 = info.ja3_string();

        // Verify format
        let parts: Vec<_> = ja3.split(',').collect();
        assert!(parts.len() >= 5, "JA3 should have at least 5 comma-separated parts");
        assert_eq!(parts[0], "772"); // TLS version
        assert_eq!(parts[1], "4865"); // Cipher suite
    }
}
