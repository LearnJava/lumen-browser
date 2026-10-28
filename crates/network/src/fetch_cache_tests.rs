//! Unit tests of the `fetch()` cache policy (`fetch_cache.rs`).

use super::*;
use crate::http_cache::HttpCache;

fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

/// A real snapshot, as the cache hands it out.
fn snapshot(response_headers: &[(&str, &str)]) -> CacheEntrySnapshot {
    let cache = HttpCache::new();
    cache.store("http://e.test/r", 200, b"body".to_vec(), &h(response_headers));
    cache.get("http://e.test/r").expect("stored")
}

fn fresh() -> CacheEntrySnapshot {
    snapshot(&[("Cache-Control", "max-age=3600"), ("ETag", "\"v1\"")])
}

fn stale_with_validator() -> CacheEntrySnapshot {
    snapshot(&[("Cache-Control", "max-age=0"), ("ETag", "\"v1\"")])
}

fn no_cc() -> RequestCc {
    RequestCc::default()
}

#[test]
fn cache_mode_parses_every_fetch_value() {
    assert_eq!(CacheMode::parse("default"), CacheMode::Default);
    assert_eq!(CacheMode::parse(""), CacheMode::Default);
    assert_eq!(CacheMode::parse("no-store"), CacheMode::NoStore);
    assert_eq!(CacheMode::parse("reload"), CacheMode::Reload);
    assert_eq!(CacheMode::parse("no-cache"), CacheMode::NoCache);
    assert_eq!(CacheMode::parse("force-cache"), CacheMode::ForceCache);
    assert_eq!(CacheMode::parse("only-if-cached"), CacheMode::OnlyIfCached);
}

#[test]
fn default_mode_with_a_conditional_header_becomes_no_store() {
    let conditional = h(&[("If-None-Match", "\"x\"")]);
    assert_eq!(CacheMode::Default.for_request(&conditional), CacheMode::NoStore);
    assert_eq!(CacheMode::NoCache.for_request(&conditional), CacheMode::NoCache);
    assert_eq!(CacheMode::Default.for_request(&h(&[("X-Other", "1")])), CacheMode::Default);
}

#[test]
fn request_lines_follow_fetch_section_4_5() {
    let none: Vec<(String, String)> = Vec::new();
    assert_eq!(CacheMode::NoCache.request_lines(&none), "Cache-Control: max-age=0\r\n");
    assert_eq!(
        CacheMode::NoStore.request_lines(&none),
        "Pragma: no-cache\r\nCache-Control: no-cache\r\n"
    );
    assert_eq!(CacheMode::Reload.request_lines(&none), CacheMode::NoStore.request_lines(&none));
    // Default drops the profile's navigation `Cache-Control: max-age=0`.
    assert_eq!(CacheMode::Default.request_lines(&none), "x-lumen-drop: cache-control\r\n");
    // The author's own header is left alone.
    let author = h(&[("cache-control", "foo")]);
    assert_eq!(CacheMode::NoCache.request_lines(&author), "");
    assert_eq!(CacheMode::Default.request_lines(&author), "");
    assert_eq!(CacheMode::NoStore.request_lines(&author), "Pragma: no-cache\r\n");
    assert_eq!(
        CacheMode::NoStore.request_lines(&h(&[("pragma", "foo")])),
        "Cache-Control: no-cache\r\n"
    );
}

#[test]
fn request_cc_collects_directives() {
    let cc = RequestCc::from_headers(&h(&[(
        "Cache-Control",
        "max-age=600, MAX-STALE, min-fresh=5, no-cache, only-if-cached",
    )]));
    assert_eq!(cc.max_age, Some(600));
    assert_eq!(cc.max_stale, Some(None));
    assert_eq!(cc.min_fresh, Some(5));
    assert!(cc.no_cache && cc.only_if_cached && !cc.no_store);
    assert_eq!(
        RequestCc::from_headers(&h(&[("cache-control", "max-stale=30")])).max_stale,
        Some(Some(30))
    );
}

#[test]
fn default_mode_serves_fresh_revalidates_stale_and_asks_on_a_miss() {
    assert!(matches!(plan(CacheMode::Default, &no_cc(), Some(fresh())), Plan::Serve(_)));
    assert!(matches!(
        plan(CacheMode::Default, &no_cc(), Some(stale_with_validator())),
        Plan::Revalidate(_)
    ));
    // Stale and nothing to validate with: a plain request.
    let no_validator = snapshot(&[("Cache-Control", "max-age=0"), ("Expires", "Thu, 01 Jan 1970 00:00:00 GMT")]);
    assert!(no_validator.conditional_headers.is_empty());
    assert!(matches!(plan(CacheMode::Default, &no_cc(), Some(no_validator)), Plan::Network));
    assert!(matches!(plan(CacheMode::Default, &no_cc(), None), Plan::Network));
}

#[test]
fn no_cache_response_is_revalidated_even_when_fresh() {
    let e = snapshot(&[("Cache-Control", "no-cache, max-age=3600"), ("ETag", "\"v1\"")]);
    assert!(matches!(plan(CacheMode::Default, &no_cc(), Some(e)), Plan::Revalidate(_)));
}

#[test]
fn no_store_and_reload_never_look_at_the_cache() {
    assert!(matches!(plan(CacheMode::NoStore, &no_cc(), Some(fresh())), Plan::Network));
    assert!(matches!(plan(CacheMode::Reload, &no_cc(), Some(fresh())), Plan::Network));
}

#[test]
fn no_cache_mode_revalidates_when_it_can() {
    assert!(matches!(plan(CacheMode::NoCache, &no_cc(), Some(fresh())), Plan::Revalidate(_)));
    assert!(matches!(plan(CacheMode::NoCache, &no_cc(), None), Plan::Network));
}

#[test]
fn force_cache_serves_stale_and_only_if_cached_errors_on_a_miss() {
    assert!(matches!(
        plan(CacheMode::ForceCache, &no_cc(), Some(stale_with_validator())),
        Plan::Serve(_)
    ));
    assert!(matches!(plan(CacheMode::ForceCache, &no_cc(), None), Plan::Network));
    assert!(matches!(plan(CacheMode::OnlyIfCached, &no_cc(), Some(stale_with_validator())), Plan::Serve(_)));
    assert!(matches!(plan(CacheMode::OnlyIfCached, &no_cc(), None), Plan::NetworkError));
}

#[test]
fn request_directives_tighten_or_loosen_reuse() {
    let cc = |s: &str| RequestCc::from_headers(&h(&[("cache-control", s)]));
    // Fresh response, request wants it fresher than it is.
    assert!(matches!(plan(CacheMode::Default, &cc("min-fresh=7200"), Some(fresh())), Plan::Revalidate(_)));
    assert!(matches!(plan(CacheMode::Default, &cc("min-fresh=60"), Some(fresh())), Plan::Serve(_)));
    // Aged by an `Age` header beyond the request's max-age.
    let aged = snapshot(&[("Cache-Control", "max-age=3600"), ("Age", "1800"), ("ETag", "\"v1\"")]);
    assert!(matches!(plan(CacheMode::Default, &cc("max-age=600"), Some(aged)), Plan::Revalidate(_)));
    assert!(matches!(plan(CacheMode::Default, &cc("no-cache"), Some(fresh())), Plan::Revalidate(_)));
    assert!(matches!(plan(CacheMode::Default, &cc("no-store"), Some(fresh())), Plan::Network));
    // Stale by 500 s, request tolerates 1000 s of it.
    let stale = snapshot(&[("Cache-Control", "max-age=1500"), ("Age", "2000")]);
    assert!(matches!(plan(CacheMode::Default, &cc("max-stale=1000"), Some(stale)), Plan::Serve(_)));
    let stale = snapshot(&[("Cache-Control", "max-age=1500"), ("Age", "2000")]);
    assert!(!matches!(plan(CacheMode::Default, &cc("max-stale=100"), Some(stale)), Plan::Serve(_)));
    // `must-revalidate` forbids serving stale whatever max-stale says.
    let strict = snapshot(&[("Cache-Control", "max-age=1, must-revalidate"), ("Age", "50"), ("ETag", "\"v1\"")]);
    assert!(matches!(plan(CacheMode::Default, &cc("max-stale"), Some(strict)), Plan::Revalidate(_)));
}

#[test]
fn only_if_cached_request_directive_yields_504_without_a_usable_entry() {
    let cc = RequestCc::from_headers(&h(&[("cache-control", "only-if-cached")]));
    assert!(matches!(plan(CacheMode::Default, &cc, None), Plan::GatewayTimeout));
    assert!(matches!(plan(CacheMode::Default, &cc, Some(stale_with_validator())), Plan::GatewayTimeout));
    assert!(matches!(plan(CacheMode::Default, &cc, Some(fresh())), Plan::Serve(_)));
}

#[test]
fn vary_on_request_headers_splits_the_entry() {
    let resp = h(&[("Vary", "Foo")]);
    let foo1 = h(&[("foo", "1")]);
    let marker = vary_marker(&resp, &foo1);
    let VaryMarker::Values(values) = marker else { panic!("expected values") };

    let stored = |extra: &str| {
        snapshot(&[("Cache-Control", "max-age=3600"), ("Vary", "Foo"), (VARY_MARKER_HEADER, extra)])
    };
    assert!(vary_match(stored(&values), &foo1).is_some());
    assert!(vary_match(stored(&values), &h(&[("foo", "2")])).is_none());
    assert!(vary_match(stored(&values), &[]).is_none(), "an omitted header is a different variant");
    assert!(vary_match(stored(&values), &h(&[("foo", "1"), ("other", "x")])).is_some());
}

#[test]
fn vary_star_and_cookie_are_not_stored_and_profile_headers_do_not_split() {
    let none: Vec<(String, String)> = Vec::new();
    assert_eq!(vary_marker(&h(&[("Vary", "*")]), &none), VaryMarker::Unstorable);
    assert_eq!(vary_marker(&h(&[("Vary", "Accept-Encoding, Cookie")]), &none), VaryMarker::Unstorable);
    assert_eq!(vary_marker(&h(&[("Vary", "Accept-Encoding, Accept")]), &none), VaryMarker::None);
    assert_eq!(vary_marker(&h(&[("X", "y")]), &none), VaryMarker::None);
}

#[test]
fn invalidation_covers_the_url_and_same_origin_locations() {
    let cache = HttpCache::new();
    let store = |url: &str| cache.store(url, 200, b"x".to_vec(), &h(&[("Cache-Control", "max-age=3600")]));
    store("http://e.test/a");
    store("http://e.test/loc");
    store("http://e.test/cl");
    store("http://other.test/loc");
    let url = Url::parse("http://e.test/a").unwrap();
    invalidate(
        &cache,
        &url,
        &h(&[("Location", "/loc"), ("Content-Location", "cl"), ("Link", "x")]),
    );
    assert!(cache.get("http://e.test/a").is_none());
    assert!(cache.get("http://e.test/loc").is_none());
    assert!(cache.get("http://e.test/cl").is_none());

    // A Location on another origin is left alone.
    let cache = HttpCache::new();
    cache.store("http://other.test/loc", 200, b"x".to_vec(), &h(&[("Cache-Control", "max-age=3600")]));
    invalidate(&cache, &url, &h(&[("Location", "http://other.test/loc")]));
    assert!(cache.get("http://other.test/loc").is_some());
}

#[test]
fn only_get_head_options_trace_are_safe() {
    for m in ["GET", "HEAD", "OPTIONS", "TRACE"] {
        assert!(!is_unsafe_method(m), "{m}");
    }
    for m in ["POST", "PUT", "PATCH", "DELETE", "FOO"] {
        assert!(is_unsafe_method(m), "{m}");
    }
}
