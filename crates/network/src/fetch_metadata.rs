//! Request mode and Fetch Metadata headers for engine-issued subresource
//! fetches (BUG-1021).
//!
//! The Fetch Standard ties every request to a *mode* (§3.1.4 "request mode":
//! `navigate`, `same-origin`, `no-cors`, `cors`, `websocket`) that depends on
//! who issued it, not only on what it loads: a CSS `background-image` is
//! `no-cors`, an `@font-face` body is `cors` (CSS Fonts 4 §4.9), a stylesheet
//! `@import` is `no-cors`. The mode is observable on the wire through the
//! Fetch Metadata request headers (`Sec-Fetch-Mode`/`-Dest`/`-Site`, W3C
//! Fetch Metadata §2) and, for a cross-origin `cors` request, through the
//! `Origin` header (Fetch §3.1 "append a request's `Origin` header").
//!
//! Before this module every subresource fetch inherited the fingerprint
//! profile's *navigation* block verbatim (`Sec-Fetch-Mode: navigate`,
//! `Sec-Fetch-Dest: document`, `Sec-Fetch-Site: none`) — a stylesheet or an
//! image that claims to be a user-typed top-level navigation, which is both
//! wrong per spec and an anti-bot tell. [`subresource_request_headers`] builds
//! the per-request replacement; the header builders already let a
//! caller-supplied header displace the same-named profile default (BUG-749 on
//! HTTP/1.1, RP-7 on HTTP/2), so the override needs no second code path.

use lumen_core::url::Url;

use crate::http::HttpProfile;
use crate::mixed_content::RequestDestination;
use crate::origin::Origin;

/// Fetch Standard §3.1.4 request mode, as serialised in `Sec-Fetch-Mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestMode {
    /// A navigation (top-level document or a nested browsing context).
    Navigate,
    /// A request that must not leave the requester's origin (worker scripts).
    SameOrigin,
    /// An opaque cross-origin load: `<img>`, CSS images, classic scripts and
    /// stylesheets without a `crossorigin` attribute, `@import`.
    NoCors,
    /// A CORS request: `@font-face`, `fetch()`/XHR, module scripts.
    Cors,
}

impl RequestMode {
    /// The `Sec-Fetch-Mode` token (Fetch Metadata §2.3).
    #[must_use]
    pub fn as_header_value(self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::SameOrigin => "same-origin",
            Self::NoCors => "no-cors",
            Self::Cors => "cors",
        }
    }

    /// The mode an engine-issued request for `destination` gets when its
    /// initiator carries no `crossorigin` attribute — the only case Lumen's
    /// subresource path knows about today.
    ///
    /// - `@font-face` is always `cors` (CSS Fonts 4 §4.9 "font fetching
    ///   requirements"), whatever the page says.
    /// - `<img>`, CSS images, `<link rel=stylesheet>`, `@import`, classic
    ///   `<script src>`, media and `<link rel=prefetch>` are `no-cors`
    ///   (HTML LS "create a potential-CORS request" with the attribute absent;
    ///   CSS Values 4 §4.5.1 for CSS-initiated images and imports).
    /// - Worker scripts are `same-origin` (HTML LS §10.2.6.4 "fetch a classic
    ///   worker script").
    /// - `fetch()`/XHR/beacon default to `cors` (Fetch §5.4 `Request`
    ///   constructor, XHR §3.5.6).
    /// - A frame document is a navigation.
    #[must_use]
    pub fn for_destination(destination: RequestDestination) -> Self {
        match destination {
            RequestDestination::Font | RequestDestination::Connect | RequestDestination::Other => {
                Self::Cors
            }
            RequestDestination::Script
            | RequestDestination::Style
            | RequestDestination::Image
            | RequestDestination::Media
            | RequestDestination::Prefetch => Self::NoCors,
            RequestDestination::Worker => Self::SameOrigin,
            RequestDestination::Document => Self::Navigate,
        }
    }

    /// The mode a page-issued `fetch()`/XHR asked for (`RequestInit.mode`,
    /// forwarded by the shim as `JsFetchRequest::mode`). `""` and anything
    /// unknown fall back to Fetch's default `cors`; `navigate` cannot be
    /// requested from script (Fetch §5.4 `Request` constructor step 23 throws),
    /// so it is treated as `cors` as well.
    #[must_use]
    pub fn from_fetch_mode(mode: &str) -> Self {
        match mode {
            "no-cors" => Self::NoCors,
            "same-origin" => Self::SameOrigin,
            _ => Self::Cors,
        }
    }
}

/// Fetch destinations a page-side caller may name in `JsFetchRequest::
/// destination` (Fetch §3.1.4 "request destination"). Anything else —
/// including `""` — serialises as `empty`, the destination of a plain
/// `fetch()`/XHR.
const KNOWN_DESTINATIONS: &[&str] = &[
    "audio", "audioworklet", "document", "embed", "font", "frame", "iframe", "image", "json",
    "manifest", "object", "paintworklet", "report", "script", "serviceworker", "sharedworker",
    "style", "track", "video", "worker", "xslt",
];

/// `Sec-Fetch-Dest` token for a destination named by the JS shim.
#[must_use]
pub fn dest_token(destination: &str) -> &'static str {
    KNOWN_DESTINATIONS.iter().copied().find(|d| *d == destination).unwrap_or("empty")
}

/// `Sec-Fetch-Dest` token for `destination` (Fetch Metadata §2.1). Fetch's
/// empty destination serialises as `empty`; a prefetch has no destination of
/// its own either (`RequestDestination::as_fetch_dest` spells it `prefetch`
/// only for the shell's internal bookkeeping).
fn sec_fetch_dest(destination: RequestDestination) -> &'static str {
    match destination {
        RequestDestination::Prefetch => "empty",
        other => match other.as_fetch_dest() {
            "" => "empty",
            dest => dest,
        },
    }
}

/// `Sec-Fetch-Site` for a request from `initiator` to `target` (Fetch
/// Metadata §2.4). No initiator (a request not tied to a document) is `none`;
/// an opaque target is `cross-site`. Same-site requires the same scheme as
/// well as the same registrable domain (§2.4 step 4 "same site").
fn sec_fetch_site(initiator: Option<&Url>, target: &Url) -> &'static str {
    let Some(initiator) = initiator else {
        return "none";
    };
    let (Ok(from), Ok(to)) = (Origin::from_url(initiator), Origin::from_url(target)) else {
        return "cross-site";
    };
    if from.same_origin(&to) {
        "same-origin"
    } else if from.scheme() == to.scheme() && crate::coop::is_same_site(&from, &to) {
        "same-site"
    } else {
        "cross-site"
    }
}

/// Whether `profile`'s fingerprint block carries Fetch Metadata at all.
///
/// Only those profiles get the per-request override: adding `Sec-Fetch-*` to
/// a profile whose navigation block has none (Firefox/Safari/Lumen as they
/// are modelled today) would itself make the subresource requests stand out
/// from the navigation that preceded them.
fn sends_fetch_metadata(profile: HttpProfile) -> bool {
    matches!(
        profile,
        HttpProfile::Chrome | HttpProfile::Strict | HttpProfile::Edge | HttpProfile::TorBrowser
    )
}

/// `Origin` header line for a cross-origin `cors` request (Fetch §3.1
/// "append a request's `Origin` header": response tainting `cors`), whatever
/// the fingerprint profile — this one is part of the CORS protocol, not a
/// fingerprint choice. Empty for any other mode, for a same-origin target and
/// for a request not tied to a document.
#[must_use]
pub fn cors_origin_header(initiator: Option<&Url>, target: &Url, mode: RequestMode) -> String {
    if mode != RequestMode::Cors {
        return String::new();
    }
    let Some(from) = initiator.and_then(|u| Origin::from_url(u).ok()) else {
        return String::new();
    };
    if Origin::from_url(target).is_ok_and(|to| from.same_origin(&to)) {
        return String::new();
    }
    format!("Origin: {}\r\n", from.serialize())
}

/// `Sec-Fetch-Site`/`-Mode`/`-Dest` lines that replace the profile's
/// navigation defaults, for profiles that send Fetch Metadata at all; empty
/// otherwise and for [`RequestMode::Navigate`] (a frame navigation keeps the
/// profile's own navigation block, which is what it already sent).
/// `dest` is the already-serialised `Sec-Fetch-Dest` token.
#[must_use]
pub fn fetch_metadata_headers(
    profile: HttpProfile,
    initiator: Option<&Url>,
    target: &Url,
    dest: &str,
    mode: RequestMode,
) -> String {
    if mode == RequestMode::Navigate || !sends_fetch_metadata(profile) {
        return String::new();
    }
    format!(
        "Sec-Fetch-Site: {}\r\nSec-Fetch-Mode: {}\r\nSec-Fetch-Dest: {dest}\r\n",
        sec_fetch_site(initiator, target),
        mode.as_header_value(),
    )
}

/// Extra request headers (`Name: value\r\n` lines) that carry `mode` and
/// `destination` for an engine-issued subresource fetch of `target` by the
/// document at `initiator`: the CORS `Origin` ([`cors_origin_header`]) plus
/// Fetch Metadata ([`fetch_metadata_headers`]).
#[must_use]
pub fn subresource_request_headers(
    profile: HttpProfile,
    initiator: Option<&Url>,
    target: &Url,
    destination: RequestDestination,
    mode: RequestMode,
) -> String {
    if mode == RequestMode::Navigate {
        return String::new();
    }
    let mut out = cors_origin_header(initiator, target, mode);
    out.push_str(&fetch_metadata_headers(
        profile,
        initiator,
        target,
        sec_fetch_dest(destination),
        mode,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn modes_follow_the_initiator_table() {
        // css/fetching/fetch-resources.sub.html asserts exactly these.
        assert_eq!(RequestMode::for_destination(RequestDestination::Image), RequestMode::NoCors);
        assert_eq!(RequestMode::for_destination(RequestDestination::Style), RequestMode::NoCors);
        assert_eq!(RequestMode::for_destination(RequestDestination::Font), RequestMode::Cors);
        assert_eq!(RequestMode::for_destination(RequestDestination::Worker), RequestMode::SameOrigin);
    }

    #[test]
    fn same_origin_font_gets_cors_metadata_without_origin() {
        let doc = url("http://127.0.0.1:18300/css/fetching/fetch-resources.sub.html");
        let font = url("http://127.0.0.1:18300/fonts/pass.woff");
        let h = subresource_request_headers(
            HttpProfile::Chrome,
            Some(&doc),
            &font,
            RequestDestination::Font,
            RequestMode::Cors,
        );
        assert_eq!(
            h,
            "Sec-Fetch-Site: same-origin\r\nSec-Fetch-Mode: cors\r\nSec-Fetch-Dest: font\r\n"
        );
    }

    #[test]
    fn cross_origin_cors_request_carries_origin() {
        let doc = url("https://app.example.com/page");
        let font = url("https://cdn.example.com/f.woff2");
        let h = subresource_request_headers(
            HttpProfile::Firefox,
            Some(&doc),
            &font,
            RequestDestination::Font,
            RequestMode::Cors,
        );
        // Firefox profile: no Fetch Metadata, but the CORS `Origin` stays.
        assert_eq!(h, "Origin: https://app.example.com\r\n");
    }

    #[test]
    fn no_cors_image_has_no_origin_and_reports_site() {
        let doc = url("https://www.example.com/");
        let img = url("https://img.example.com/a.png");
        let h = subresource_request_headers(
            HttpProfile::Chrome,
            Some(&doc),
            &img,
            RequestDestination::Image,
            RequestMode::NoCors,
        );
        assert_eq!(h, "Sec-Fetch-Site: same-site\r\nSec-Fetch-Mode: no-cors\r\nSec-Fetch-Dest: image\r\n");
        let other = url("http://img.example.com/a.png");
        let h = subresource_request_headers(
            HttpProfile::Chrome,
            Some(&doc),
            &other,
            RequestDestination::Image,
            RequestMode::NoCors,
        );
        assert!(h.starts_with("Sec-Fetch-Site: cross-site\r\n"), "{h}");
    }

    #[test]
    fn no_document_is_site_none_and_navigation_is_untouched() {
        let target = url("https://example.com/x.css");
        let h = subresource_request_headers(
            HttpProfile::Chrome,
            None,
            &target,
            RequestDestination::Style,
            RequestMode::NoCors,
        );
        assert!(h.starts_with("Sec-Fetch-Site: none\r\n"), "{h}");
        assert!(
            subresource_request_headers(
                HttpProfile::Chrome,
                None,
                &target,
                RequestDestination::Document,
                RequestMode::Navigate,
            )
            .is_empty()
        );
    }

    #[test]
    fn page_fetch_modes_and_destinations() {
        assert_eq!(RequestMode::from_fetch_mode(""), RequestMode::Cors);
        assert_eq!(RequestMode::from_fetch_mode("no-cors"), RequestMode::NoCors);
        assert_eq!(RequestMode::from_fetch_mode("same-origin"), RequestMode::SameOrigin);
        assert_eq!(RequestMode::from_fetch_mode("navigate"), RequestMode::Cors);
        assert_eq!(dest_token("style"), "style");
        assert_eq!(dest_token(""), "empty");
        assert_eq!(dest_token("bogus"), "empty");
    }

    #[test]
    fn prefetch_and_connect_serialise_as_empty_dest() {
        assert_eq!(sec_fetch_dest(RequestDestination::Prefetch), "empty");
        assert_eq!(sec_fetch_dest(RequestDestination::Connect), "empty");
        assert_eq!(sec_fetch_dest(RequestDestination::Style), "style");
    }
}
