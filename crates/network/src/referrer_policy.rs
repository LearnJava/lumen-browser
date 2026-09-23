//! Referrer Policy (<https://w3c.github.io/webappsec-referrer-policy/>) §3/§8.3 —
//! the `Referer` header value a request carries, computed from the source
//! document's referrer policy rather than sent unconditionally.
//!
//! GAP-REFERRER срез 1: policy keyword parsing + the strip/downgrade
//! algorithm, applied to `fetch()`/`XMLHttpRequest`/`navigator.sendBeacon`
//! (the surfaces [BUG-859](../../../bugs/BUG-859-OPEN.md) measured directly).
//! Срез 2: the same algorithm reaches engine-issued subresource fetches too
//! (`<img>`/`<script src>`/`<link>`/`@import`/`@font-face`/…, all GET-only —
//! `Origin` does not apply there) via
//! `ResourceBase::http_client_for_subresource`, always the project default.
//! Срез 3: `<meta name=referrer>` and the `Referrer-Policy` response header
//! now override that default for the top-level document's own `fetch()`/
//! `XMLHttpRequest`/`sendBeacon`/`Worker`/`<embed>`/`<object>`/media clients
//! (`page_pipeline.rs`/`hibernate.rs`, `resource_base::document_referrer_policy`)
//! and for `<script src>` (`scripts.rs::resolve_script_sources`). Срезы 4/5:
//! every remaining subresource producer (`<link>`/`@import`/`<iframe src>`/
//! `<img>`/`@font-face`/`<track>`/video/audio) reads the same document policy.
//! Срез 6: a `referrerpolicy` element attribute (spec §6.6) now overrides the
//! document policy for that element's own request on `<script src>`
//! (`scripts.rs::resolve_script_sources`), `<link rel=stylesheet>`/`@import`
//! (`stylesheets.rs::load_linked_stylesheets`, the override also reaches the
//! sheet's own `@import`s) and `<iframe src>` (`frames.rs::spawn_frame`, via
//! `lumen_dom::IframeInfo::referrer_policy`). Still on the document default,
//! not the attribute: `<img>`/`@font-face`/`<track>`/video (their per-request
//! plumbing point doesn't carry a single element attribute the way the three
//! above do) and the preload scanner (structurally without a `Document` —
//! scans the byte stream before one exists).

use crate::origin::Origin;
use lumen_core::url::Url;

/// A referrer policy keyword, as it appears in a `Referrer-Policy` header,
/// a `<meta name=referrer>` `content` attribute, or a `referrerpolicy`
/// element attribute (spec §3 "referrer policy").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferrerPolicy {
    NoReferrer,
    NoReferrerWhenDowngrade,
    Origin,
    OriginWhenCrossOrigin,
    SameOrigin,
    StrictOrigin,
    StrictOriginWhenCrossOrigin,
    UnsafeUrl,
}

impl ReferrerPolicy {
    /// Parse a single policy keyword, case-insensitively. `None` for an
    /// unrecognized token (spec: an invalid token is simply skipped, not an
    /// error) — see [`Self::parse_list`] for the caller-facing entry point.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "no-referrer" => Some(Self::NoReferrer),
            "no-referrer-when-downgrade" => Some(Self::NoReferrerWhenDowngrade),
            "origin" => Some(Self::Origin),
            "origin-when-cross-origin" => Some(Self::OriginWhenCrossOrigin),
            "same-origin" => Some(Self::SameOrigin),
            "strict-origin" => Some(Self::StrictOrigin),
            "strict-origin-when-cross-origin" => Some(Self::StrictOriginWhenCrossOrigin),
            "unsafe-url" => Some(Self::UnsafeUrl),
            _ => None,
        }
    }

    /// A `Referrer-Policy` header (or `<meta name=referrer>`) may carry a
    /// comma-separated list of tokens; the **last valid** one wins (spec §3.1
    /// "parse a referrer policy" — later tokens override earlier ones, and
    /// unrecognized tokens are simply skipped rather than aborting the whole
    /// list).
    #[must_use]
    pub fn parse_list(s: &str) -> Option<Self> {
        s.split(',').filter_map(Self::parse).next_back()
    }

    /// The project default absent any `<meta>`/header/attribute override
    /// (`docs/plan/privacy.md` §9.1): full `Referer` for same-origin,
    /// origin-only cross-origin, none across a downgrade.
    #[must_use]
    pub fn default_policy() -> Self {
        Self::StrictOriginWhenCrossOrigin
    }
}

/// Compute the `Referer` header value for a request to `target_url`,
/// initiated by a document whose own URL is `referrer_url`, under `policy` —
/// spec §8.3 "determine request's referrer", restricted to the same-document
/// case (no `<a referrerpolicy>`/`iframe` override source, no client-hints
/// stripping).
///
/// Returns `None` when the policy says to send no `Referer` at all: the
/// policy is `no-referrer`, the referring document's own URL has no tuple
/// origin (`about:`/`data:`/`file:` — spec's "referrer source is not a URL
/// whose scheme is a fetch scheme"), or policy-and-downgrade rules say to
/// withhold it.
#[must_use]
pub fn compute_referrer(policy: ReferrerPolicy, referrer_url: &Url, target_url: &Url) -> Option<String> {
    if policy == ReferrerPolicy::NoReferrer {
        return None;
    }
    // Opaque source (about:/data:/file:/javascript:) carries no origin to
    // reveal — spec's "referrer source" must itself be a URL, and only
    // http(s) sources have one in this engine (`Origin::from_url`).
    let referrer_origin = Origin::from_url(referrer_url).ok()?;
    let target_origin = Origin::from_url(target_url).ok();
    let same_origin = target_origin
        .as_ref()
        .is_some_and(|t| referrer_origin.same_origin(t));
    // "Downgrade" (spec §3 "is url potentially trustworthy?" gate on
    // no-referrer-when-downgrade/strict-origin*): referrer is trustworthy
    // (https/wss/loopback) and the target is not — includes an opaque
    // target origin (unparseable/non-fetch scheme), which is never
    // trustworthy.
    let is_downgrade = referrer_origin.is_potentially_trustworthy()
        && !target_origin
            .as_ref()
            .is_some_and(Origin::is_potentially_trustworthy);

    let full = || format!("{}{}", referrer_origin.serialize(), referrer_url.path_and_query());
    let origin_only = || referrer_origin.serialize();

    let result = match policy {
        ReferrerPolicy::NoReferrer => unreachable!("handled above"),
        ReferrerPolicy::NoReferrerWhenDowngrade => {
            if is_downgrade { None } else { Some(full()) }
        }
        ReferrerPolicy::Origin => Some(origin_only()),
        ReferrerPolicy::OriginWhenCrossOrigin => {
            Some(if same_origin { full() } else { origin_only() })
        }
        ReferrerPolicy::SameOrigin => {
            if same_origin { Some(full()) } else { None }
        }
        ReferrerPolicy::StrictOrigin => {
            if is_downgrade { None } else { Some(origin_only()) }
        }
        ReferrerPolicy::StrictOriginWhenCrossOrigin => {
            if is_downgrade {
                None
            } else if same_origin {
                Some(full())
            } else {
                Some(origin_only())
            }
        }
        ReferrerPolicy::UnsafeUrl => Some(full()),
    };

    // Spec §8.3 step after policy branching: a `result` longer than 4096
    // bytes collapses to the origin, regardless of which branch produced it.
    result.map(|r| if r.len() > 4096 { origin_only() } else { r })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn parse_known_keywords() {
        assert_eq!(ReferrerPolicy::parse("no-referrer"), Some(ReferrerPolicy::NoReferrer));
        assert_eq!(
            ReferrerPolicy::parse("Strict-Origin-When-Cross-Origin"),
            Some(ReferrerPolicy::StrictOriginWhenCrossOrigin)
        );
        assert_eq!(ReferrerPolicy::parse("bogus"), None);
    }

    #[test]
    fn parse_list_last_valid_wins() {
        assert_eq!(
            ReferrerPolicy::parse_list("origin, bogus, unsafe-url"),
            Some(ReferrerPolicy::UnsafeUrl)
        );
        assert_eq!(ReferrerPolicy::parse_list("bogus, also-bogus"), None);
    }

    #[test]
    fn strict_origin_when_cross_origin_full_for_same_origin() {
        let r = compute_referrer(
            ReferrerPolicy::default_policy(),
            &url("https://example.com/page?x=1"),
            &url("https://example.com/api"),
        );
        assert_eq!(r.as_deref(), Some("https://example.com/page?x=1"));
    }

    #[test]
    fn strict_origin_when_cross_origin_origin_only_cross_origin() {
        let r = compute_referrer(
            ReferrerPolicy::default_policy(),
            &url("https://example.com/page"),
            &url("https://other.example/api"),
        );
        assert_eq!(r.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn strict_origin_when_cross_origin_none_on_downgrade() {
        let r = compute_referrer(
            ReferrerPolicy::default_policy(),
            &url("https://example.com/page"),
            &url("http://example.com/api"),
        );
        assert_eq!(r, None);
    }

    #[test]
    fn no_referrer_always_none() {
        let r = compute_referrer(
            ReferrerPolicy::NoReferrer,
            &url("https://example.com/page"),
            &url("https://example.com/api"),
        );
        assert_eq!(r, None);
    }

    #[test]
    fn unsafe_url_always_full_even_cross_origin_downgrade() {
        let r = compute_referrer(
            ReferrerPolicy::UnsafeUrl,
            &url("https://example.com/page?x=1"),
            &url("http://other.example/api"),
        );
        assert_eq!(r.as_deref(), Some("https://example.com/page?x=1"));
    }

    #[test]
    fn same_origin_policy_none_cross_origin() {
        let r = compute_referrer(
            ReferrerPolicy::SameOrigin,
            &url("https://example.com/page"),
            &url("https://other.example/api"),
        );
        assert_eq!(r, None);
    }

    #[test]
    fn opaque_referrer_source_yields_none() {
        let r = compute_referrer(
            ReferrerPolicy::UnsafeUrl,
            &url("data:text/plain,hi"),
            &url("https://example.com/api"),
        );
        assert_eq!(r, None);
    }

    #[test]
    fn full_referrer_over_4096_bytes_collapses_to_origin() {
        // Spec §8.3: a `result` longer than 4096 bytes is replaced with the
        // origin, independent of which policy branch produced `full()`.
        let long_path = format!("/{}", "a".repeat(4096));
        let r = compute_referrer(
            ReferrerPolicy::UnsafeUrl,
            &url(&format!("https://example.com{long_path}")),
            &url("https://example.com/api"),
        );
        assert_eq!(r.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn full_referrer_under_4096_bytes_stays_full() {
        let short_path = format!("/{}", "a".repeat(10));
        let r = compute_referrer(
            ReferrerPolicy::UnsafeUrl,
            &url(&format!("https://example.com{short_path}")),
            &url("https://example.com/api"),
        );
        assert_eq!(r.as_deref(), Some(format!("https://example.com{short_path}").as_str()));
    }
}
