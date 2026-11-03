//! HTTP fingerprinting + per-profile configuration.
//!
//! Implements matching HTTP layer parameters with current Chrome version:
//! - HTTP/1.1 header ordering (User-Agent, Accept, Accept-Encoding, Accept-Language, etc.)
//! - HTTP/1.1 header casing matching Chrome
//! - HTTP/2 SETTINGS frame values matching Chrome
//! - HTTP/2 stream priority pattern matching Chrome
//! - Accept-Language default value (`en-US,en;q=0.9`), replaceable by the shell
//!   ([`accept_language`])
//! - Client Hints handling per profile
//!
//! Per-profile HTTP configs:
//! - Standard: general use, Chrome-matching header order and values
//! - Strict: private/HSTS mode, Client Hints disabled
//! - Tor: request signature pinned to current Tor Browser (Firefox ESR 128,
//!   Windows-uniform UA), so a Lumen "Tor mode" request matches genuine Tor
//!   Browser traffic instead of presenting a unique minimal fingerprint

pub mod accept_language;
pub mod headers;
pub mod h2_settings;
pub mod client_hints;

pub use accept_language::{
    accept_language_header, expand_language_list, global_accept_language, set_global_accept_language,
};
pub use headers::{
    HttpProfile, HeaderOrder, build_request_headers, build_request_headers_for,
    build_request_headers_with_identity, h2_fingerprint_headers,
    h2_fingerprint_headers_with_identity, sends_global_privacy_control,
};
pub use h2_settings::{H2Settings, H2StreamPriority};
pub use client_hints::{
    ClientHintsProfile, should_send_client_hints, client_hints_headers, low_entropy_client_hints,
    low_entropy_client_hints_for,
};

/// HTTP/1.1 User-Agent value for Lumen-native profile.
///
/// Tracks the workspace version (`CARGO_PKG_VERSION`) so it never drifts on a
/// version bump — see the versioning policy in `CLAUDE.md`.
pub const DEFAULT_USER_AGENT: &str = concat!("Lumen/", env!("CARGO_PKG_VERSION"));

/// Major version of the stable Chrome the `Chrome`/`Edge`/`Strict` profiles
/// impersonate. The single source lives in `lumen_core` (BUG-1614) so the JS
/// layer and Client Hints read the same number.
pub use lumen_core::CHROME_MAJOR;

/// Chrome / Edge User-Agent strings — defined in `lumen_core::identity` so the
/// JS layer derives `navigator.*` from the very same literals (BUG-1614).
pub use lumen_core::{CHROME_USER_AGENT, EDGE_USER_AGENT};

/// Firefox profile User-Agent (Linux X11).
pub const FIREFOX_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";

/// Safari profile User-Agent (macOS).
pub const SAFARI_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

/// The `User-Agent` a profile sends — the one string the JS layer
/// (`navigator.userAgent`, `userAgentData`) and Client Hints must agree with.
#[must_use]
pub fn profile_user_agent(profile: HttpProfile) -> &'static str {
    match profile {
        HttpProfile::Chrome | HttpProfile::Strict => lumen_core::chrome_user_agent(),
        HttpProfile::Edge => EDGE_USER_AGENT,
        HttpProfile::Firefox => FIREFOX_USER_AGENT,
        HttpProfile::Safari => SAFARI_USER_AGENT,
        HttpProfile::TorBrowser => TOR_BROWSER_USER_AGENT,
        HttpProfile::Lumen => DEFAULT_USER_AGENT,
    }
}

/// Default Accept-Language header (does not leak real locale). The shell
/// replaces it with the user's locale via [`set_global_accept_language`].
pub const DEFAULT_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

/// Chrome `Accept` header for a **top-level document navigation**.
///
/// Real Chrome sends this exact string when navigating to an HTML document;
/// a bare `*/*` (what a generic HTTP client sends) is a cheap anti-bot signal
/// (Cloudflare/DataDome flag it). Used by the `Chrome`/`Edge`/`Strict` profiles
/// on the navigation request (RP-7). Subresource-destination-specific `Accept`
/// values (CSS `text/css,*/*;q=0.1`, images, etc.) are a separate refinement —
/// this module currently treats every request as a document navigation
/// (`Sec-Fetch-Dest: document`).
pub const CHROME_NAVIGATE_ACCEPT: &str =
    "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";

/// Tor Browser User-Agent — pinned uniformly across **all** host platforms
/// (Windows NT 10.0, no `Win64`/architecture token), based on the current
/// Tor Browser stable (Firefox ESR 128).
///
/// Tor Browser deliberately reports the same UA for every user regardless of
/// the real OS so that the entire Tor Browser population shares one signature
/// (anti-fingerprinting). Lumen's Tor profile pins the identical string so a
/// Lumen "Tor mode" request is indistinguishable from a genuine Tor Browser
/// request at the HTTP layer (ADR-007 §6, task 9F.3).
pub const TOR_BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; rv:128.0) Gecko/20100101 Firefox/128.0";

/// Tor Browser `Accept` header for top-level document navigations —
/// the Firefox ESR 128 default. Matched verbatim so the Tor profile does not
/// stand out from real Tor Browser traffic.
pub const TOR_BROWSER_ACCEPT: &str =
    "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8";

/// Tor Browser `Accept-Language` — pinned to the Tor Browser default locale
/// (`en-US,en;q=0.5`); never reflects the user's real locale.
pub const TOR_BROWSER_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.5";
