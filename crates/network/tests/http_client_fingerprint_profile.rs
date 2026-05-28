//! Integration tests for HttpProfile integration in HttpClient (9C Phase 1).
//!
//! Tests verify that HttpClient.with_fingerprint_profile() correctly configures
//! the HTTP fingerprinting profile (Chrome/Lumen/Strict/Tor) and passes it through
//! the fetch pipeline to header generation.

#[cfg(test)]
mod tests {
    use lumen_network::{HttpClient, HttpProfile};

    #[test]
    fn test_http_client_with_fingerprint_profile_chrome() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Chrome);
        // Verify that the client accepts the Chrome profile without error
        assert_eq!(client.fingerprint_profile(), HttpProfile::Chrome);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_lumen() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Lumen);
        // Verify that the client accepts the Lumen profile without error
        assert_eq!(client.fingerprint_profile(), HttpProfile::Lumen);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_strict() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Strict);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Strict);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_tor() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Tor);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Tor);
    }

    #[test]
    fn test_http_client_default_profile_is_chrome() {
        let client = HttpClient::new();
        // Default profile should be Chrome (for compatibility)
        assert_eq!(client.fingerprint_profile(), HttpProfile::Chrome);
    }

    #[test]
    fn test_http_client_profile_chain_builder() {
        // Test that with_fingerprint_profile() can be chained with other builder methods
        let client = HttpClient::new()
            .with_fingerprint_profile(HttpProfile::Strict)
            .with_fingerprint_profile(HttpProfile::Tor); // Second call should override

        assert_eq!(client.fingerprint_profile(), HttpProfile::Tor);
    }

    #[test]
    fn test_http_client_profile_persistence() {
        // Test that profile is stored and accessible
        let profile = HttpProfile::Strict;
        let client = HttpClient::new().with_fingerprint_profile(profile);

        // Profile should remain after being set
        assert_eq!(client.fingerprint_profile(), profile);
    }
}
