//! Built-in filter list shipped with Lumen (Privacy shields §12).
//!
//! `DefaultFilterList` implements [`FilterListSource`] by returning a minimal
//! bundled ruleset at `fetch_rules()`.  The ruleset covers the most common
//! tracking and advertising domains; it is intentionally small so the browser
//! compiles fast.  A future phase will let the user subscribe to a full
//! EasyList/EasyPrivacy download and cache it to disk.
//!
//! ## Typical usage
//!
//! ```rust,ignore
//! let src    = DefaultFilterList;
//! let text   = src.fetch_rules().unwrap();
//! let filter = EasyListFilter::parse(&text);
//! let client = HttpClient::new().with_filter(Arc::new(filter));
//! ```

use lumen_core::error::Result;
use lumen_core::ext::FilterListSource;

/// Bundled EasyList-format ruleset shipped inside the Lumen binary.
///
/// Returns a minimal subset of well-known tracker and advertising domains.
/// Designed for `FilterListSource` → `EasyListFilter::parse` pipeline.
pub struct DefaultFilterList;

/// Minimal built-in EasyList ruleset.
///
/// Each rule is in EasyList/ABP network-filter format.  Cosmetic/element-hiding
/// lines (`##`) are excluded — this is a network-only filter.
const BUNDLED_RULES: &str = "\
! Lumen Privacy Shields — built-in ruleset (Phase 1)
! Generated from well-known tracker domains (non-exhaustive).

! ── Google tracking ──────────────────────────────────────────────────────
||google-analytics.com^
||googletagmanager.com^
||googletagservices.com^
||googlesyndication.com^
||doubleclick.net^
||googleadservices.com^
||stats.g.doubleclick.net^

! ── Facebook / Meta ──────────────────────────────────────────────────────
||connect.facebook.net^
||facebook.com/tr^

! ── Common ad networks ───────────────────────────────────────────────────
||ads.twitter.com^
||advertising.com^
||adnxs.com^
||openx.net^
||pubmatic.com^
||rubiconproject.com^
||taboola.com^
||outbrain.com^
||criteo.com^
||scorecardresearch.com^
||comscore.com^

! ── Common trackers ──────────────────────────────────────────────────────
||hotjar.com^
||mixpanel.com^
||segment.com^
||amplitude.com^
||fullstory.com^
||logrocket.com^
||heap.io^
||kissmetrics.com^
||omtrdc.net^
||demdex.net^

! ── Regex rules ──────────────────────────────────────────────────────────
/\\/beacon\\.(js|min\\.js)/
/\\/(track|tracking|pixel|analytics)\\.php/
";

impl FilterListSource for DefaultFilterList {
    /// Returns `"lumen-builtin"`.
    fn name(&self) -> &str {
        "lumen-builtin"
    }

    /// Returns the bundled EasyList-format ruleset.
    ///
    /// Always succeeds — the ruleset is compiled into the binary.
    fn fetch_rules(&self) -> Result<String> {
        Ok(BUNDLED_RULES.to_string())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::EasyListFilter;
    use lumen_core::ext::RequestFilter;
    use lumen_core::url::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("valid URL")
    }

    /// Build a filter from the bundled rules and test basic lookups.
    fn built_filter() -> EasyListFilter {
        let src = DefaultFilterList;
        let text = src.fetch_rules().unwrap();
        EasyListFilter::parse(&text)
    }

    #[test]
    fn name_is_lumen_builtin() {
        assert_eq!(DefaultFilterList.name(), "lumen-builtin");
    }

    #[test]
    fn fetch_rules_returns_non_empty_string() {
        let text = DefaultFilterList.fetch_rules().unwrap();
        assert!(!text.is_empty());
    }

    #[test]
    fn bundled_rules_block_google_analytics() {
        let f = built_filter();
        assert!(f.should_block(&url("https://google-analytics.com/collect")).is_some());
    }

    #[test]
    fn bundled_rules_block_doubleclick() {
        let f = built_filter();
        assert!(f.should_block(&url("https://doubleclick.net/ad")).is_some());
    }

    #[test]
    fn bundled_rules_block_hotjar() {
        let f = built_filter();
        assert!(f.should_block(&url("https://hotjar.com/tracking.js")).is_some());
    }

    #[test]
    fn bundled_rules_allow_normal_site() {
        let f = built_filter();
        assert!(f.should_block(&url("https://example.com/page")).is_none());
    }

    #[test]
    fn bundled_rules_allow_google_search() {
        let f = built_filter();
        // google.com itself is not blocked, only tracking sub-domains.
        assert!(f.should_block(&url("https://www.google.com/search?q=rust")).is_none());
    }

    #[test]
    fn bundled_rules_block_facebook_pixel() {
        let f = built_filter();
        assert!(f.should_block(&url("https://connect.facebook.net/signals/config/123")).is_some());
    }

    #[test]
    fn bundled_rules_block_tracking_php_via_regex() {
        let f = built_filter();
        assert!(f.should_block(&url("https://example.com/tracking.php?uid=1")).is_some());
    }

    #[test]
    fn bundled_rules_block_analytics_php_via_regex() {
        let f = built_filter();
        assert!(f.should_block(&url("https://example.com/analytics.php")).is_some());
    }

    #[test]
    fn bundled_rules_block_pixel_php_via_regex() {
        let f = built_filter();
        assert!(f.should_block(&url("https://example.com/pixel.php")).is_some());
    }

    #[test]
    fn bundled_rules_allow_regular_php_page() {
        let f = built_filter();
        assert!(f.should_block(&url("https://example.com/contact.php")).is_none());
    }
}
