//! HTTP/1.1 header ordering and casing matching Chrome.
//!
//! Chrome HTTP/1.1 request headers are sent in a specific order with specific casing.
//! This module implements the Chrome-matching header order to avoid fingerprinting
//! via header order variance (common detection vector for anti-bots like Cloudflare/DataDome).

use std::collections::VecDeque;

/// HTTP profile — determines header order, casing, and HTTP/2 SETTINGS configuration.
///
/// Each profile matches a specific browser's fingerprint (TLS, HTTP/1.1 headers, HTTP/2 SETTINGS).
/// See ADR-007 §«Per-profile HTTP configs» for the rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpProfile {
    /// Chrome 130+ — default for compatibility. Matches current stable Chrome.
    Chrome,
    /// Firefox 130+ — minimal header set, different SETTINGS than Chrome.
    Firefox,
    /// Safari 18+ — minimal headers (Sec-* subset), conservative SETTINGS.
    Safari,
    /// Edge 130+ — similar to Chrome but with distinct alpn/extension ordering.
    Edge,
    /// Tor Browser — Tor-native TLS fingerprint + minimal headers.
    TorBrowser,
    /// Lumen-native — own optimized SETTINGS and UA (not impersonating any browser).
    Lumen,
    /// Strict private mode — Chrome-compatible but with Client Hints disabled + enhanced anti-fp.
    Strict,
}

/// Chrome HTTP/1.1 header order (in request).
///
/// This is the order Chrome uses for HTTP/1.1 requests. The order
/// is a fingerprinting vector — non-Chrome libraries often use different
/// orders. Matching Chrome's order reduces false-positive detection.
///
/// Order (Standard profile):
/// 1. Host (automatic, always first after request line)
/// 2. Connection
/// 3. Cache-Control
/// 4. User-Agent
/// 5. Accept
/// 6. Accept-Encoding
/// 7. Accept-Language
/// 8. DNT
/// 9. Sec-Fetch-Site
/// 10. Sec-Fetch-Mode
/// 11. Sec-Fetch-Dest
/// 12. Authorization (if present)
/// 13. Range (if present)
/// 14. Custom headers (author-provided)
#[derive(Debug)]
pub struct HeaderOrder {
    headers: VecDeque<(String, String)>,
}

impl HeaderOrder {
    /// Create a new header order builder for the given profile.
    pub fn new(_profile: HttpProfile) -> Self {
        Self {
            headers: VecDeque::new(),
        }
    }

    /// Add a header (key, value) to the ordered list.
    ///
    /// Headers are stored in the order they are added. The finalized
    /// header block will output them in this order.
    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.headers.push_back((key.into(), value.into()));
    }

    /// Build the HTTP/1.1 header block string for the request line.
    ///
    /// Returns a string like:
    /// ```text
    /// Host: example.com\r\n
    /// Connection: keep-alive\r\n
    /// User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) ...\r\n
    /// Accept: */*\r\n
    /// \r\n
    /// ```
    pub fn to_http_block(&self) -> String {
        let mut result = String::new();
        for (key, value) in &self.headers {
            result.push_str(key);
            result.push_str(": ");
            result.push_str(value);
            result.push_str("\r\n");
        }
        result.push_str("\r\n");
        result
    }

    /// Return headers as a list of tuples.
    pub fn as_tuples(&self) -> Vec<(String, String)> {
        self.headers.iter().cloned().collect()
    }

    /// Clear all headers.
    pub fn clear(&mut self) {
        self.headers.clear();
    }
}

/// Build HTTP/1.1 request headers for the given profile.
///
/// Each profile constructs headers in a specific order and set matching a real browser.
///
/// Parameters:
/// - `host`: Host header value (e.g., "example.com")
/// - `accept_encoding`: Accept-Encoding header (e.g., "gzip, deflate, br")
/// - `extra_headers`: Custom/author-provided headers as pre-formatted string
/// - `profile`: HttpProfile (Chrome, Firefox, Safari, Edge, TorBrowser, Lumen, Strict)
///
/// Returns header block ready to append to HTTP/1.1 request line.
pub fn build_request_headers(
    host: &str,
    accept_encoding: &str,
    extra_headers: &str,
    profile: HttpProfile,
) -> String {
    let mut headers = HeaderOrder::new(profile);

    match profile {
        HttpProfile::Chrome | HttpProfile::Strict => {
            // Chrome 130+ HTTP/1.1 header order
            headers.add("Host", host);
            headers.add("Connection", "keep-alive");
            headers.add("Cache-Control", "max-age=0");
            headers.add("User-Agent", super::CHROME_USER_AGENT);
            headers.add("Accept", "*/*");

            if !accept_encoding.is_empty() {
                headers.add("Accept-Encoding", accept_encoding);
            }

            headers.add("Accept-Language", super::DEFAULT_ACCEPT_LANGUAGE);

            // DNT (Do Not Track) — Chrome sends by default
            headers.add("DNT", "1");

            // Sec-Fetch-* headers (Chromium 76+) — sent by default in Chrome
            headers.add("Sec-Fetch-Site", "none");
            headers.add("Sec-Fetch-Mode", "navigate");
            headers.add("Sec-Fetch-Dest", "document");
        }
        HttpProfile::Firefox => {
            // Firefox 130+ HTTP/1.1 header order (minimal, no Sec-Fetch-*, no DNT)
            headers.add("Host", host);
            headers.add("User-Agent", "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0");
            headers.add("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8");

            if !accept_encoding.is_empty() {
                headers.add("Accept-Encoding", accept_encoding);
            }

            headers.add("Accept-Language", super::DEFAULT_ACCEPT_LANGUAGE);
            headers.add("Connection", "keep-alive");
            headers.add("Cache-Control", "max-age=0");
        }
        HttpProfile::Safari => {
            // Safari 18+ HTTP/1.1 header order (very minimal)
            headers.add("Host", host);
            headers.add("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15");
            headers.add("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8");

            if !accept_encoding.is_empty() {
                headers.add("Accept-Encoding", accept_encoding);
            }

            headers.add("Accept-Language", "en-US,en;q=0.9");
            headers.add("Connection", "keep-alive");
        }
        HttpProfile::Edge => {
            // Edge 130+ HTTP/1.1 header order (similar to Chrome with minor differences)
            headers.add("Host", host);
            headers.add("Connection", "keep-alive");
            headers.add("Cache-Control", "max-age=0");
            headers.add("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0");
            headers.add("Accept", "*/*");

            if !accept_encoding.is_empty() {
                headers.add("Accept-Encoding", accept_encoding);
            }

            headers.add("Accept-Language", super::DEFAULT_ACCEPT_LANGUAGE);

            // Edge includes Sec-Fetch-* like Chrome
            headers.add("Sec-Fetch-Site", "none");
            headers.add("Sec-Fetch-Mode", "navigate");
            headers.add("Sec-Fetch-Dest", "document");
        }
        HttpProfile::TorBrowser => {
            // Tor Browser request signature, matching current Tor Browser
            // stable (Firefox ESR 128). The goal is NOT a minimal header set —
            // a minimal set is itself a unique fingerprint — but a byte-for-byte
            // match with genuine Tor Browser navigation requests so a Lumen
            // "Tor mode" user blends into the Tor Browser population.
            //
            // Firefox/Tor Browser HTTP/1.1 header order for a top-level
            // document navigation: Host, User-Agent, Accept, Accept-Language,
            // Accept-Encoding, Connection, Upgrade-Insecure-Requests,
            // Sec-Fetch-Dest, Sec-Fetch-Mode, Sec-Fetch-Site, Sec-Fetch-User,
            // Priority. The UA is pinned to Windows for every host OS (see
            // `TOR_BROWSER_USER_AGENT`).
            headers.add("Host", host);
            headers.add("User-Agent", super::TOR_BROWSER_USER_AGENT);
            headers.add("Accept", super::TOR_BROWSER_ACCEPT);
            headers.add("Accept-Language", super::TOR_BROWSER_ACCEPT_LANGUAGE);

            if !accept_encoding.is_empty() {
                headers.add("Accept-Encoding", accept_encoding);
            }

            headers.add("Connection", "keep-alive");
            headers.add("Upgrade-Insecure-Requests", "1");

            // Sec-Fetch metadata — Firefox ESR 128 (and thus Tor Browser)
            // sends these on navigations; their absence would single Lumen out.
            headers.add("Sec-Fetch-Dest", "document");
            headers.add("Sec-Fetch-Mode", "navigate");
            headers.add("Sec-Fetch-Site", "none");
            headers.add("Sec-Fetch-User", "?1");

            // Firefox 128 sends an RFC 9218 Priority header on the initial
            // document request.
            headers.add("Priority", "u=0, i");
        }
        HttpProfile::Lumen => {
            // Lumen-native — own fingerprint (not impersonating any browser)
            headers.add("Host", host);
            headers.add("Connection", "keep-alive");
            headers.add("Cache-Control", "max-age=0");
            headers.add("User-Agent", super::DEFAULT_USER_AGENT);
            headers.add("Accept", "*/*");

            if !accept_encoding.is_empty() {
                headers.add("Accept-Encoding", accept_encoding);
            }

            headers.add("Accept-Language", super::DEFAULT_ACCEPT_LANGUAGE);
        }
    }

    // Append any extra headers from caller (CORS, Authorization, etc.)
    // Note: caller must ensure no duplicate Host/Connection/etc.
    let header_block = headers.to_http_block();

    // extra_headers already contain \r\n, so append directly
    format!("{}{}", header_block.trim_end_matches("\r\n\r\n"), "\r\n")
        + extra_headers
        + "\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_order_contains_required_headers() {
        let headers = build_request_headers("example.com", "gzip, deflate, br", "", HttpProfile::Chrome);
        assert!(headers.contains("Host: example.com"));
        assert!(headers.contains("User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64)"));
        assert!(headers.contains("Accept: */*"));
        assert!(headers.contains("Accept-Language: en-US,en;q=0.9"));
        assert!(headers.contains("Connection: keep-alive"));
    }

    #[test]
    fn test_default_accept_language() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Chrome);
        assert!(headers.contains("Accept-Language: en-US,en;q=0.9"));
    }

    #[test]
    fn test_sec_fetch_headers_present() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Chrome);
        assert!(headers.contains("Sec-Fetch-Site: none"));
        assert!(headers.contains("Sec-Fetch-Mode: navigate"));
        assert!(headers.contains("Sec-Fetch-Dest: document"));
    }

    #[test]
    fn test_firefox_profile_lacks_sec_fetch() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Firefox);
        assert!(headers.contains("User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0"));
        assert!(!headers.contains("Sec-Fetch-Site"));
        assert!(!headers.contains("Sec-Fetch-Mode"));
        assert!(!headers.contains("Sec-Fetch-Dest"));
        assert!(!headers.contains("DNT:"));
    }

    #[test]
    fn test_safari_profile_minimal_headers() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Safari);
        assert!(headers.contains("User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15"));
        assert!(headers.contains("Host: example.com"));
        assert!(headers.contains("Accept: text/html,application/xhtml+xml"));
        assert!(!headers.contains("Sec-Fetch-Site"));
    }

    #[test]
    fn test_edge_profile_chrome_like() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Edge);
        assert!(headers.contains("User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"));
        assert!(headers.contains("Sec-Fetch-Site: none"));
        assert!(headers.contains("Sec-Fetch-Mode: navigate"));
    }

    #[test]
    fn test_tor_browser_profile_pins_windows_firefox_ua() {
        // Tor Browser pins a Windows UA for every host OS (uniform population),
        // based on Firefox ESR 128 — no `Win64`/arch token, no `X11; Linux`.
        let headers = build_request_headers("example.com", "", "", HttpProfile::TorBrowser);
        assert!(headers.contains(
            "User-Agent: Mozilla/5.0 (Windows NT 10.0; rv:128.0) Gecko/20100101 Firefox/128.0"
        ));
        assert!(!headers.contains("X11; Linux"), "must not leak the real Linux host OS");
        assert!(!headers.contains("Win64"), "Tor Browser UA omits the architecture token");
        // Tor Browser does not send the DNT header (privacy.donottrackheader off).
        assert!(!headers.contains("DNT:"));
    }

    #[test]
    fn test_tor_browser_profile_sends_firefox_sec_fetch() {
        // Modern Tor Browser (Firefox ESR 128) DOES send Sec-Fetch-* and
        // Upgrade-Insecure-Requests on navigations; their absence (the old
        // "minimal" behaviour) would make Lumen's Tor mode trivially distinct.
        let headers = build_request_headers("example.com", "", "", HttpProfile::TorBrowser);
        assert!(headers.contains("Upgrade-Insecure-Requests: 1"));
        assert!(headers.contains("Sec-Fetch-Dest: document"));
        assert!(headers.contains("Sec-Fetch-Mode: navigate"));
        assert!(headers.contains("Sec-Fetch-Site: none"));
        assert!(headers.contains("Sec-Fetch-User: ?1"));
        assert!(headers.contains("Priority: u=0, i"));
    }

    #[test]
    fn test_tor_browser_profile_pinned_accept_and_locale() {
        let headers = build_request_headers("example.com", "gzip, deflate, br", "", HttpProfile::TorBrowser);
        assert!(headers.contains(
            "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
        ));
        // Locale is pinned to the Tor Browser default, never the real locale.
        assert!(headers.contains("Accept-Language: en-US,en;q=0.5"));
        assert!(headers.contains("Accept-Encoding: gzip, deflate, br"));
    }

    #[test]
    fn test_tor_browser_header_order_matches_firefox() {
        // Header order is part of the fingerprint: it must follow Firefox's
        // navigation order exactly.
        let headers = build_request_headers("example.com", "gzip, deflate, br", "", HttpProfile::TorBrowser);
        let order = [
            "Host:",
            "User-Agent:",
            "Accept:",
            "Accept-Language:",
            "Accept-Encoding:",
            "Connection:",
            "Upgrade-Insecure-Requests:",
            "Sec-Fetch-Dest:",
            "Sec-Fetch-Mode:",
            "Sec-Fetch-Site:",
            "Sec-Fetch-User:",
            "Priority:",
        ];
        let mut last = 0usize;
        for name in order {
            let pos = headers.find(name).unwrap_or_else(|| panic!("missing header {name}"));
            assert!(pos >= last, "header {name} is out of Firefox order");
            last = pos;
        }
    }

    #[test]
    fn test_lumen_profile_custom_ua() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Lumen);
        assert!(headers.contains(&format!("User-Agent: Lumen/{}", env!("CARGO_PKG_VERSION"))));
        assert!(headers.contains("Accept: */*"));
        assert!(headers.contains("Accept-Language: en-US,en;q=0.9"));
    }
}
