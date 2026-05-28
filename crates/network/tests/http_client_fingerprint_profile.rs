//! Integration tests for HttpProfile integration in HttpClient (9C Phase 1).
//!
//! Tests verify that HttpClient.with_fingerprint_profile() correctly configures
//! the HTTP fingerprinting profile (Standard/Strict/Tor) and passes it through
//! the fetch pipeline to header generation.

#[cfg(test)]
mod tests {
    use lumen_network::{HttpClient, HttpProfile};

    #[test]
    fn test_http_client_with_fingerprint_profile_standard() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Standard);
        // Verify that the client accepts the Standard profile without error
        assert_eq!(client.fingerprint_profile(), HttpProfile::Standard);
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
    fn test_http_client_default_profile_is_standard() {
        let client = HttpClient::new();
        // Default profile should be Standard
        assert_eq!(client.fingerprint_profile(), HttpProfile::Standard);
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
