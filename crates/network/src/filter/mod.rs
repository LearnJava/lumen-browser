//! Request filter implementations for Lumen's privacy-first block list engine.
//!
//! This module provides:
//! - [`EasyListFilter`] — parses and matches EasyList / Adblock Plus network rules
//! - [`HostsFilter`]    — parses and matches `/etc/hosts`-format block lists
//! - [`CompositeFilter`] — chains multiple `RequestFilter` implementations
//!
//! All types implement [`lumen_core::ext::RequestFilter`] and are safe to share
//! across threads (`Send + Sync`).
//!
//! ## Typical usage
//!
//! ```rust,ignore
//! let easylist = EasyListFilter::parse(&easylist_text);
//! let hosts    = HostsFilter::parse(&hosts_text);
//! let filter   = CompositeFilter::new(vec![Box::new(easylist), Box::new(hosts)]);
//! let client   = HttpClient::new().with_filter(Arc::new(filter));
//! ```

pub mod easylist;
pub mod hosts;

pub use easylist::EasyListFilter;
pub use hosts::HostsFilter;

use lumen_core::ext::RequestFilter;
use lumen_core::url::Url;

// ── CompositeFilter ────────────────────────────────────────────────────────

/// Chains multiple [`RequestFilter`] implementations.
///
/// `should_block` returns the first non-`None` reason from the inner filters,
/// in the order they were provided.  If all inner filters return `None`, the
/// request is allowed.
pub struct CompositeFilter {
    filters: Vec<Box<dyn RequestFilter>>,
}

impl CompositeFilter {
    /// Create a composite filter from a list of inner filters.
    pub fn new(filters: Vec<Box<dyn RequestFilter>>) -> Self {
        Self { filters }
    }
}

impl RequestFilter for CompositeFilter {
    fn should_block(&self, url: &Url) -> Option<String> {
        self.filters.iter().find_map(|f| f.should_block(url))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::url::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("valid URL")
    }

    #[test]
    fn composite_blocks_if_any_filter_blocks() {
        let easylist = EasyListFilter::parse("||adserver.com^");
        let hosts    = HostsFilter::parse("0.0.0.0 tracker.net");
        let composite = CompositeFilter::new(vec![
            Box::new(easylist),
            Box::new(hosts),
        ]);
        // EasyList blocks this:
        assert!(composite.should_block(&url("https://adserver.com/ad.js")).is_some());
        // Hosts blocks this:
        assert!(composite.should_block(&url("https://tracker.net/pixel")).is_some());
        // Neither blocks this:
        assert!(composite.should_block(&url("https://example.com/page")).is_none());
    }

    #[test]
    fn composite_returns_reason_from_first_matching_filter() {
        let easylist = EasyListFilter::parse("||shared.com^");
        let hosts    = HostsFilter::parse("0.0.0.0 shared.com");
        let composite = CompositeFilter::new(vec![
            Box::new(easylist),
            Box::new(hosts),
        ]);
        // EasyList is first; its reason wins.
        let reason = composite.should_block(&url("https://shared.com/")).unwrap();
        assert_eq!(reason, "easylist");
    }

    #[test]
    fn empty_composite_allows_everything() {
        let composite = CompositeFilter::new(vec![]);
        assert!(composite.should_block(&url("https://example.com/")).is_none());
    }
}
