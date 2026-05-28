//! Client Hints handling per HTTP profile.
//!
//! Client Hints (Sec-CH-* headers) are optional headers that allow servers
//! to request specific client information. Chrome sends them when the server
//! requests them via Accept-CH header.
//!
//! Lumen's policy per ADR-007:
//! - Standard profile: send Client Hints when requested (UA, viewport, etc.)
//! - Strict profile: do not send Client Hints (privacy-first)
//! - Tor profile: do not send Client Hints (Tor Browser policy)

use crate::http::HttpProfile;

/// Client Hints profile — determines which hints to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientHintsProfile {
    /// Send Client Hints when server requests them (Standard behavior).
    Enabled,
    /// Do not send Client Hints (Strict/Tor behavior).
    Disabled,
}

impl ClientHintsProfile {
    /// Create ClientHintsProfile for the given HTTP profile.
    pub fn for_http_profile(profile: HttpProfile) -> Self {
        match profile {
            HttpProfile::Standard => ClientHintsProfile::Enabled,
            HttpProfile::Strict | HttpProfile::Tor => ClientHintsProfile::Disabled,
        }
    }
}

/// Determine whether to send Client Hints headers for the given HTTP profile.
///
/// Returns true if Client Hints should be sent (Standard profile and server
/// requested them via Accept-CH header), false otherwise (Strict/Tor profile
/// or server did not request them).
pub fn should_send_client_hints(
    profile: HttpProfile,
    server_requested: bool,
) -> bool {
    let ch_profile = ClientHintsProfile::for_http_profile(profile);
    ch_profile == ClientHintsProfile::Enabled && server_requested
}

/// Build Client Hints headers for the given UA string (Lumen).
///
/// Standard Client Hints sent by Chrome:
/// - Sec-CH-UA: `"Lumen/0.0.1"`
/// - Sec-CH-UA-Mobile: `?0` (not mobile)
/// - Sec-CH-UA-Platform: `"Windows"` / `"Linux"` / `"macOS"` (detected from OS)
///
/// Note: Sec-CH-UA values are quoted and prefixed with version (e.g., `"Lumen/0.0.1"`)
pub fn client_hints_headers(
    profile: HttpProfile,
    server_requested: bool,
    os_platform: &str,
) -> Vec<(String, String)> {
    if !should_send_client_hints(profile, server_requested) {
        return Vec::new();
    }

    vec![
        ("Sec-CH-UA".to_string(), r#""Lumen/0.0.1""#.to_string()),
        ("Sec-CH-UA-Mobile".to_string(), "?0".to_string()),
        ("Sec-CH-UA-Platform".to_string(), format!(r#""{}""#, os_platform)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_hints_enabled_for_standard() {
        assert!(should_send_client_hints(HttpProfile::Standard, true));
    }

    #[test]
    fn test_client_hints_disabled_for_strict() {
        assert!(!should_send_client_hints(HttpProfile::Strict, true));
    }

    #[test]
    fn test_client_hints_disabled_for_tor() {
        assert!(!should_send_client_hints(HttpProfile::Tor, true));
    }

    #[test]
    fn test_client_hints_disabled_if_not_requested() {
        assert!(!should_send_client_hints(HttpProfile::Standard, false));
    }

    #[test]
    fn test_client_hints_headers_standard() {
        let hints = client_hints_headers(HttpProfile::Standard, true, "Windows");
        assert_eq!(hints.len(), 3);
        assert!(hints.iter().any(|(k, v)| k == "Sec-CH-UA" && v.contains("Lumen")));
        assert!(hints.iter().any(|(k, v)| k == "Sec-CH-UA-Mobile" && v == "?0"));
        assert!(hints.iter().any(|(k, v)| k == "Sec-CH-UA-Platform" && v.contains("Windows")));
    }

    #[test]
    fn test_client_hints_headers_empty_for_strict() {
        let hints = client_hints_headers(HttpProfile::Strict, true, "Windows");
        assert!(hints.is_empty());
    }
}
