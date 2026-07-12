//! Integration tests for HttpProfile integration in HttpClient (9C Phase 1).
//!
//! Tests verify:
//! - HttpClient.with_fingerprint_profile() correctly configures profile
//! - build_request_headers() produces Chrome-matching header order
//! - Accept-Language is `en-US,en;q=0.9` (does not leak real locale)
//! - Sec-Fetch-* headers present in Chrome/Edge/TorBrowser, absent in Firefox
//! - Client Hints disabled for Strict/TorBrowser profiles
//! - H2 SETTINGS wire format matches per-profile values

#[cfg(test)]
mod tests {
    use lumen_network::{HttpClient, HttpProfile};
    use lumen_network::http::{build_request_headers, H2Settings};
    use lumen_network::http::client_hints::{should_send_client_hints, client_hints_headers};

    // ── Profile getter/setter ──────────────────────────────────────────────

    #[test]
    fn test_http_client_default_profile_is_chrome() {
        let client = HttpClient::new();
        assert_eq!(client.fingerprint_profile(), HttpProfile::Chrome);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_chrome() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Chrome);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Chrome);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_lumen() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Lumen);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Lumen);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_strict() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Strict);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Strict);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_tor_browser() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::TorBrowser);
        assert_eq!(client.fingerprint_profile(), HttpProfile::TorBrowser);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_firefox() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Firefox);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Firefox);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_safari() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Safari);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Safari);
    }

    #[test]
    fn test_http_client_with_fingerprint_profile_edge() {
        let client = HttpClient::new().with_fingerprint_profile(HttpProfile::Edge);
        assert_eq!(client.fingerprint_profile(), HttpProfile::Edge);
    }

    #[test]
    fn test_http_client_profile_chain_builder() {
        let client = HttpClient::new()
            .with_fingerprint_profile(HttpProfile::Strict)
            .with_fingerprint_profile(HttpProfile::TorBrowser);
        assert_eq!(client.fingerprint_profile(), HttpProfile::TorBrowser);
    }

    // ── 9C.1 Chrome HTTP/1.1 header order ────────────────────────────────

    #[test]
    fn chrome_headers_start_with_host_connection_cache_control() {
        let h = build_request_headers("example.com", "gzip, deflate, br", "", HttpProfile::Chrome);
        // Chrome order: Host, Connection, Cache-Control, User-Agent, Accept, ...
        let lines: Vec<&str> = h.lines().collect();
        assert!(lines[0].starts_with("Host:"), "Host must be first: {:?}", lines[0]);
        assert!(lines[1].starts_with("Connection:"), "Connection must be second: {:?}", lines[1]);
        assert!(lines[2].starts_with("Cache-Control:"), "Cache-Control must be third: {:?}", lines[2]);
        assert!(lines[3].starts_with("User-Agent:"), "User-Agent must be fourth: {:?}", lines[3]);
        assert!(lines[4].starts_with("Accept:"), "Accept must be fifth: {:?}", lines[4]);
    }

    #[test]
    fn chrome_headers_accept_encoding_before_accept_language() {
        let h = build_request_headers("example.com", "gzip, deflate, br", "", HttpProfile::Chrome);
        let ae_pos = h.find("Accept-Encoding:").unwrap();
        let al_pos = h.find("Accept-Language:").unwrap();
        assert!(ae_pos < al_pos, "Accept-Encoding must come before Accept-Language");
    }

    #[test]
    fn chrome_headers_sec_fetch_at_end_of_fingerprint_block() {
        let h = build_request_headers("example.com", "", "", HttpProfile::Chrome);
        // Sec-Fetch-Site must come after Accept-Language
        let al_pos = h.find("Accept-Language:").unwrap();
        let sf_pos = h.find("Sec-Fetch-Site:").unwrap();
        assert!(sf_pos > al_pos, "Sec-Fetch-Site must follow Accept-Language");
    }

    // ── 9C.4 Accept-Language default ──────────────────────────────────────

    #[test]
    fn chrome_accept_language_is_en_us() {
        let h = build_request_headers("example.com", "", "", HttpProfile::Chrome);
        assert!(h.contains("Accept-Language: en-US,en;q=0.9"),
            "Chrome profile must use en-US,en;q=0.9, got: {h}");
    }

    #[test]
    fn strict_accept_language_is_en_us() {
        let h = build_request_headers("example.com", "", "", HttpProfile::Strict);
        assert!(h.contains("Accept-Language: en-US,en;q=0.9"),
            "Strict profile must use en-US,en;q=0.9");
    }

    #[test]
    fn tor_accept_language_is_en_us_low_quality() {
        let h = build_request_headers("example.com", "", "", HttpProfile::TorBrowser);
        assert!(h.contains("Accept-Language: en-US,en;q=0.5"),
            "TorBrowser must use en-US,en;q=0.5 (Tor Browser default)");
    }

    #[test]
    fn firefox_accept_language_is_en_us() {
        let h = build_request_headers("example.com", "", "", HttpProfile::Firefox);
        assert!(h.contains("Accept-Language: en-US,en;q=0.9"),
            "Firefox profile must use en-US,en;q=0.9");
    }

    // ── 9C.1 Profile-specific header differences ─────────────────────────

    #[test]
    fn firefox_no_sec_fetch_no_dnt() {
        let h = build_request_headers("example.com", "", "", HttpProfile::Firefox);
        assert!(!h.contains("Sec-Fetch-Site"), "Firefox must NOT send Sec-Fetch-Site");
        assert!(!h.contains("Sec-Fetch-Mode"), "Firefox must NOT send Sec-Fetch-Mode");
        assert!(!h.contains("DNT:"), "Firefox must NOT send DNT");
    }

    #[test]
    fn chrome_has_sec_fetch_and_dnt() {
        let h = build_request_headers("example.com", "", "", HttpProfile::Chrome);
        assert!(h.contains("Sec-Fetch-Site: none"), "Chrome must send Sec-Fetch-Site: none");
        assert!(h.contains("Sec-Fetch-Mode: navigate"), "Chrome must send Sec-Fetch-Mode: navigate");
        assert!(h.contains("Sec-Fetch-Dest: document"), "Chrome must send Sec-Fetch-Dest: document");
        assert!(h.contains("DNT: 1"), "Chrome must send DNT: 1");
    }

    #[test]
    fn edge_has_sec_fetch_like_chrome() {
        let h = build_request_headers("example.com", "", "", HttpProfile::Edge);
        assert!(h.contains("Sec-Fetch-Site: none"), "Edge must send Sec-Fetch-Site");
        assert!(h.contains("Sec-Fetch-Mode: navigate"), "Edge must send Sec-Fetch-Mode");
    }

    #[test]
    fn tor_sends_firefox_sec_fetch_but_no_dnt() {
        // Tor Browser (Firefox ESR 128) DOES send Sec-Fetch-* on navigations —
        // matching real Tor Browser traffic — but does not send DNT (9F.3).
        let h = build_request_headers("example.com", "", "", HttpProfile::TorBrowser);
        assert!(h.contains("Sec-Fetch-Site: none"), "TorBrowser must send Sec-Fetch-Site: none");
        assert!(h.contains("Sec-Fetch-Mode: navigate"), "TorBrowser must send Sec-Fetch-Mode: navigate");
        assert!(h.contains("Sec-Fetch-Dest: document"), "TorBrowser must send Sec-Fetch-Dest: document");
        assert!(h.contains("Sec-Fetch-User: ?1"), "TorBrowser must send Sec-Fetch-User: ?1");
        assert!(h.contains("Upgrade-Insecure-Requests: 1"), "TorBrowser must send Upgrade-Insecure-Requests");
        assert!(!h.contains("DNT:"), "TorBrowser must NOT send DNT");
    }

    #[test]
    fn extra_headers_appended_after_fingerprint_block() {
        let extra = "Range: bytes=0-999\r\nAuthorization: Bearer token\r\n";
        let h = build_request_headers("example.com", "", extra, HttpProfile::Chrome);
        // Range and Authorization must appear after Sec-Fetch-* block
        let sf_pos = h.find("Sec-Fetch-Dest:").unwrap();
        let range_pos = h.find("Range:").unwrap();
        assert!(range_pos > sf_pos, "Range header must come after Sec-Fetch-Dest");
    }

    // ── 9C.2 HTTP/2 SETTINGS per profile ─────────────────────────────────

    #[test]
    fn h2_settings_chrome_matches_expected() {
        let s = H2Settings::for_profile(HttpProfile::Chrome);
        assert_eq!(s.header_table_size, 65536, "Chrome SETTINGS_HEADER_TABLE_SIZE");
        assert_eq!(s.max_concurrent_streams, Some(1000), "Chrome MAX_CONCURRENT_STREAMS");
        assert_eq!(s.initial_window_size, 6_291_456, "Chrome INITIAL_WINDOW_SIZE = 6MB");
        assert_eq!(s.max_frame_size, 16384, "Chrome MAX_FRAME_SIZE");
    }

    #[test]
    fn h2_settings_firefox_large_window() {
        let s = H2Settings::for_profile(HttpProfile::Firefox);
        assert_eq!(s.initial_window_size, 2_147_483_647, "Firefox uses max i32 window");
    }

    #[test]
    fn h2_settings_tor_conservative() {
        let s = H2Settings::for_profile(HttpProfile::TorBrowser);
        assert_eq!(s.header_table_size, 4096, "TorBrowser uses RFC default table size");
        assert_eq!(s.initial_window_size, 65535, "TorBrowser uses RFC default window");
        assert_eq!(s.max_concurrent_streams, Some(100), "TorBrowser limits concurrent streams");
    }

    #[test]
    fn h2_settings_wire_format_chrome_6_params_without_compression_limit() {
        let s = H2Settings::for_profile(HttpProfile::Chrome);
        let wire = s.to_wire_format();
        // 5 params × 6 bytes = 30 bytes (no header_compression_size_limit for Chrome)
        assert_eq!(wire.len(), 30, "Chrome wire format: 5 params × 6 bytes");
        // First param: HEADER_TABLE_SIZE (0x0001) = 65536
        assert_eq!(&wire[0..2], &[0x00, 0x01]);
        assert_eq!(&wire[2..6], &65536u32.to_be_bytes());
    }

    // ── 9C.5 Client Hints opt-out ─────────────────────────────────────────

    #[test]
    fn client_hints_disabled_for_strict_even_when_server_requests() {
        assert!(!should_send_client_hints(HttpProfile::Strict, true),
            "Strict profile must never send Client Hints");
    }

    #[test]
    fn client_hints_disabled_for_tor_even_when_server_requests() {
        assert!(!should_send_client_hints(HttpProfile::TorBrowser, true),
            "TorBrowser must never send Client Hints");
    }

    #[test]
    fn client_hints_enabled_for_chrome_when_server_requests() {
        assert!(should_send_client_hints(HttpProfile::Chrome, true),
            "Chrome profile must send Client Hints when server requests");
    }

    #[test]
    fn client_hints_disabled_when_server_does_not_request() {
        assert!(!should_send_client_hints(HttpProfile::Chrome, false),
            "Client Hints must not be sent unsolicited");
    }

    #[test]
    fn client_hints_headers_chrome_contain_sec_ch_ua() {
        let hints = client_hints_headers(HttpProfile::Chrome, true, "Windows");
        assert!(hints.iter().any(|(k, _)| k == "Sec-CH-UA"),
            "Chrome hints must include Sec-CH-UA");
        assert!(hints.iter().any(|(k, v)| k == "Sec-CH-UA-Mobile" && v == "?0"),
            "Chrome hints must indicate non-mobile (?0)");
        assert!(hints.iter().any(|(k, v)| k == "Sec-CH-UA-Platform" && v.contains("Windows")),
            "Chrome hints must include platform");
    }

    #[test]
    fn client_hints_headers_strict_empty() {
        let hints = client_hints_headers(HttpProfile::Strict, true, "Windows");
        assert!(hints.is_empty(), "Strict profile must return no Client Hints headers");
    }
}
