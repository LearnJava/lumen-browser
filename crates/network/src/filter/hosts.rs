//! Hosts-file format block list parser.
//!
//! Parses the standard `/etc/hosts` format (and common ad-blocking hosts file
//! variants) and implements [`RequestFilter`] by blocking any URL whose
//! hostname appears in the list.
//!
//! **Supported line formats:**
//! ```text
//! 0.0.0.0 tracker.example.com          # standard hosts format
//! 127.0.0.1 tracker.example.com        # also standard
//! tracker.example.com                  # bare hostname (pi-hole / StevenBlack)
//! # comment                            # ignored
//! ```
//!
//! Wildcards (e.g. `*.tracker.com`) are **not** supported in this format;
//! use [`EasyListFilter`][super::easylist::EasyListFilter] for wildcard matching.

use std::collections::HashSet;

use lumen_core::url::Url;
use lumen_core::ext::RequestFilter;

/// Hosts-file `RequestFilter`.
///
/// Blocks any URL whose hostname is present in the loaded hosts list.
/// Matching is case-insensitive (hostnames are lowercased at parse time).
#[derive(Debug, Default)]
pub struct HostsFilter {
    blocked: HashSet<String>,
}

impl HostsFilter {
    /// Parse a hosts-file text and return a filter.
    pub fn parse(text: &str) -> Self {
        let mut blocked = HashSet::new();
        for line in text.lines() {
            let line = line.trim();
            // Skip empty lines and comments.
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Strip inline comments.
            let line = match line.find('#') {
                Some(pos) => line[..pos].trim(),
                None => line,
            };
            let mut parts = line.split_whitespace();
            let first = match parts.next() {
                Some(t) => t,
                None => continue,
            };
            // Determine if first token is an IP address or a hostname.
            let hostname = if is_ip(first) {
                // `0.0.0.0 hostname` / `127.0.0.1 hostname` format.
                match parts.next() {
                    Some(h) if !h.is_empty() => h,
                    _ => continue,
                }
            } else {
                // Bare `hostname` format (pi-hole style).
                first
            };
            // Skip localhost / loopback entries — these are real routing entries.
            if hostname == "localhost" || hostname == "local" {
                continue;
            }
            blocked.insert(hostname.to_lowercase());
        }
        Self { blocked }
    }

    /// Number of blocked hostnames.
    pub fn len(&self) -> usize {
        self.blocked.len()
    }

    /// Returns `true` if the block list is empty.
    pub fn is_empty(&self) -> bool {
        self.blocked.is_empty()
    }
}

impl RequestFilter for HostsFilter {
    fn should_block(&self, url: &Url) -> Option<String> {
        let host = url.host().to_lowercase();
        if self.blocked.contains(host.as_str()) {
            return Some("hosts".to_string());
        }
        None
    }
}

/// Returns `true` if `s` looks like an IPv4 or IPv6 address (not a hostname).
fn is_ip(s: &str) -> bool {
    // IPv4: all components are digits separated by `.`.
    if s.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        return true;
    }
    // IPv6: contains `:`.
    if s.contains(':') {
        return true;
    }
    false
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::url::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("valid URL")
    }

    // ── Standard format ───────────────────────────────────────────────────

    #[test]
    fn ipv4_zero_format() {
        let f = HostsFilter::parse("0.0.0.0 tracker.example.com");
        assert!(f.should_block(&url("https://tracker.example.com/ad")).is_some());
    }

    #[test]
    fn ipv4_loopback_format() {
        let f = HostsFilter::parse("127.0.0.1 ads.example.com");
        assert!(f.should_block(&url("https://ads.example.com/")).is_some());
    }

    #[test]
    fn bare_hostname_format() {
        let f = HostsFilter::parse("adserver.io");
        assert!(f.should_block(&url("https://adserver.io/banner")).is_some());
    }

    // ── Non-blocking ──────────────────────────────────────────────────────

    #[test]
    fn unblocked_host_allowed() {
        let f = HostsFilter::parse("0.0.0.0 tracker.example.com");
        assert!(f.should_block(&url("https://example.com/")).is_none());
    }

    #[test]
    fn subdomain_not_blocked_by_parent_rule() {
        // HostsFilter does exact matching; no wildcard subdomain expansion.
        let f = HostsFilter::parse("0.0.0.0 ads.example.com");
        assert!(f.should_block(&url("https://cdn.ads.example.com/")).is_none());
    }

    // ── Comments and edge cases ───────────────────────────────────────────

    #[test]
    fn comment_lines_ignored() {
        let f = HostsFilter::parse("# This is a comment\n0.0.0.0 tracker.net\n");
        assert!(f.should_block(&url("https://tracker.net/x")).is_some());
    }

    #[test]
    fn inline_comment_stripped() {
        let f = HostsFilter::parse("0.0.0.0 tracker.net  # some comment");
        assert!(f.should_block(&url("https://tracker.net/")).is_some());
    }

    #[test]
    fn localhost_entries_skipped() {
        let f = HostsFilter::parse("127.0.0.1 localhost\n0.0.0.0 tracker.com");
        assert!(f.should_block(&url("http://localhost/")).is_none());
        assert!(f.should_block(&url("https://tracker.com/")).is_some());
    }

    #[test]
    fn empty_list_allows_everything() {
        let f = HostsFilter::parse("");
        assert!(f.should_block(&url("https://example.com/")).is_none());
        assert!(f.is_empty());
    }

    // ── Case insensitivity ────────────────────────────────────────────────

    #[test]
    fn case_insensitive_match() {
        let f = HostsFilter::parse("0.0.0.0 Tracker.Example.COM");
        assert!(f.should_block(&url("https://tracker.example.com/")).is_some());
    }

    // ── len ───────────────────────────────────────────────────────────────

    #[test]
    fn len_counts_unique_hosts() {
        let text = "0.0.0.0 a.com\n0.0.0.0 b.com\n# comment\n0.0.0.0 a.com";
        let f = HostsFilter::parse(text);
        // Deduplication: a.com appears twice but counts once.
        assert_eq!(f.len(), 2);
    }
}
