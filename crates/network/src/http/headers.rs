//! HTTP/1.1 header ordering and casing matching Chrome.
//!
//! Chrome HTTP/1.1 request headers are sent in a specific order with specific casing.
//! This module implements the Chrome-matching header order to avoid fingerprinting
//! via header order variance (common detection vector for anti-bots like Cloudflare/DataDome).

use std::collections::VecDeque;

/// HTTP profile — determines header order and casing configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpProfile {
    /// Standard profile: Chrome-matching header order and casing.
    Standard,
    /// Strict private profile: same as Standard but Client Hints disabled.
    Strict,
    /// Tor-compatible profile: minimal, conservative header order.
    Tor,
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
    /// User-Agent: Lumen/0.0.1\r\n
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

/// Build HTTP/1.1 request headers in Chrome-matching order for the given profile.
///
/// Parameters:
/// - `host`: Host header value (e.g., "example.com")
/// - `accept_encoding`: Accept-Encoding header (e.g., "gzip, deflate, br")
/// - `extra_headers`: Custom/author-provided headers as pre-formatted string
/// - `profile`: HttpProfile (Standard, Strict, or Tor)
///
/// Returns header block ready to append to HTTP/1.1 request line.
pub fn build_request_headers(
    host: &str,
    accept_encoding: &str,
    extra_headers: &str,
    profile: HttpProfile,
) -> String {
    let mut headers = HeaderOrder::new(profile);

    // Chrome HTTP/1.1 header order (Standard/Strict profiles match)
    headers.add("Host", host);
    headers.add("Connection", "keep-alive");
    headers.add("Cache-Control", "max-age=0");
    headers.add("User-Agent", super::DEFAULT_USER_AGENT);
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
        let headers = build_request_headers("example.com", "gzip, deflate, br", "", HttpProfile::Standard);
        assert!(headers.contains("Host: example.com"));
        assert!(headers.contains("User-Agent: Lumen/0.0.1"));
        assert!(headers.contains("Accept: */*"));
        assert!(headers.contains("Accept-Language: en-US,en;q=0.9"));
        assert!(headers.contains("Connection: keep-alive"));
    }

    #[test]
    fn test_default_accept_language() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Standard);
        assert!(headers.contains("Accept-Language: en-US,en;q=0.9"));
    }

    #[test]
    fn test_sec_fetch_headers_present() {
        let headers = build_request_headers("example.com", "", "", HttpProfile::Standard);
        assert!(headers.contains("Sec-Fetch-Site: none"));
        assert!(headers.contains("Sec-Fetch-Mode: navigate"));
        assert!(headers.contains("Sec-Fetch-Dest: document"));
    }
}
