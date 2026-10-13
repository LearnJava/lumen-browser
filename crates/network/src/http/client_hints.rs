//! Client Hints handling per HTTP profile.
//!
//! Client Hints (Sec-CH-* headers) are optional headers that allow servers
//! to request specific client information.
//!
//! Lumen's policy per ADR-007:
//! - Chrome/Firefox/Safari/Edge/Lumen profiles: send Client Hints when requested (UA, viewport, etc.)
//! - TorBrowser: no Client Hints (Tor Browser is Firefox, which has none).
//! - Strict keeps the Chrome UA, so it sends Chrome's low-entropy hints too:
//!   a Chrome UA without `Sec-CH-UA` is a bot signal (BUG-1614).

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
            HttpProfile::Chrome
            | HttpProfile::Strict
            | HttpProfile::Firefox
            | HttpProfile::Safari
            | HttpProfile::Edge
            | HttpProfile::Lumen => ClientHintsProfile::Enabled,
            HttpProfile::TorBrowser => ClientHintsProfile::Disabled,
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

/// Low-entropy Client Hints a profile sends on **every** request, like Chrome
/// does: `Sec-CH-UA` (GREASE brand list of the profile's Chromium major),
/// `Sec-CH-UA-Mobile`, `Sec-CH-UA-Platform`. Empty for profiles whose real
/// browser has no UA-CH (Firefox, Safari, Tor, Lumen) and for `Disabled`
/// profiles. Derived from the same [`lumen_core::Identity`] the JS layer uses,
/// so `navigator.userAgentData` and the headers cannot disagree (BUG-1614).
#[must_use]
pub fn low_entropy_client_hints(profile: HttpProfile) -> Vec<(&'static str, String)> {
    low_entropy_client_hints_for(profile, lumen_core::identity_variant())
}

/// [`low_entropy_client_hints`] for an explicit identity variant. The variant
/// only affects the Chrome / Strict profiles (adds the `Lumen` brand).
#[must_use]
pub fn low_entropy_client_hints_for(
    profile: HttpProfile,
    variant: lumen_core::IdentityVariant,
) -> Vec<(&'static str, String)> {
    if ClientHintsProfile::for_http_profile(profile) == ClientHintsProfile::Disabled {
        return Vec::new();
    }
    let ua = match profile {
        HttpProfile::Chrome | HttpProfile::Strict => variant.user_agent(),
        _ => super::profile_user_agent(profile),
    };
    lumen_core::Identity::from_user_agent(ua).low_entropy_client_hints()
}

/// Build Client Hints headers for the profile. `server_requested` gates the
/// opt-in hints; the low-entropy set is what [`low_entropy_client_hints`] returns.
pub fn client_hints_headers(
    profile: HttpProfile,
    server_requested: bool,
    _os_platform: &str,
) -> Vec<(String, String)> {
    if !should_send_client_hints(profile, server_requested) {
        return Vec::new();
    }
    low_entropy_client_hints(profile)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_hints_enabled_for_chrome() {
        assert!(should_send_client_hints(HttpProfile::Chrome, true));
    }

    #[test]
    fn test_client_hints_enabled_for_lumen() {
        assert!(should_send_client_hints(HttpProfile::Lumen, true));
    }

    #[test]
    fn test_client_hints_enabled_for_strict() {
        assert!(should_send_client_hints(HttpProfile::Strict, true));
    }

    #[test]
    fn test_client_hints_disabled_for_tor_browser() {
        assert!(!should_send_client_hints(HttpProfile::TorBrowser, true));
    }

    #[test]
    fn test_client_hints_disabled_if_not_requested() {
        assert!(!should_send_client_hints(HttpProfile::Chrome, false));
    }

    #[test]
    fn test_client_hints_headers_chrome() {
        let hints = client_hints_headers(HttpProfile::Chrome, true, "Windows");
        assert_eq!(hints.len(), 3);
        let ua = hints.iter().find(|(k, _)| k == "Sec-CH-UA").unwrap();
        assert!(ua.1.contains(&format!("\"Chromium\";v=\"{}\"", lumen_core::CHROME_MAJOR)));
        assert!(!ua.1.contains("Lumen"));
        assert!(hints.iter().any(|(k, v)| k == "Sec-CH-UA-Mobile" && v == "?0"));
        assert!(hints.iter().any(|(k, v)| k == "Sec-CH-UA-Platform" && v == "\"Windows\""));
    }

    #[test]
    fn test_low_entropy_hints_only_for_chromium_profiles() {
        assert_eq!(low_entropy_client_hints(HttpProfile::Edge).len(), 3);
        for p in [HttpProfile::Firefox, HttpProfile::Safari, HttpProfile::TorBrowser, HttpProfile::Lumen] {
            assert!(low_entropy_client_hints(p).is_empty(), "{p:?}");
        }
    }
}
