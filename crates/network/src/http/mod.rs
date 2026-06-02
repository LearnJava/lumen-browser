//! HTTP fingerprinting + per-profile configuration.
//!
//! Implements matching HTTP layer parameters with current Chrome version:
//! - HTTP/1.1 header ordering (User-Agent, Accept, Accept-Encoding, Accept-Language, etc.)
//! - HTTP/1.1 header casing matching Chrome
//! - HTTP/2 SETTINGS frame values matching Chrome
//! - HTTP/2 stream priority pattern matching Chrome
//! - Accept-Language default value (`en-US,en;q=0.9`)
//! - Client Hints handling per profile
//!
//! Per-profile HTTP configs:
//! - Standard: general use, Chrome-matching header order and values
//! - Strict: private/HSTS mode, Client Hints disabled
//! - Tor: request signature pinned to current Tor Browser (Firefox ESR 128,
//!   Windows-uniform UA), so a Lumen "Tor mode" request matches genuine Tor
//!   Browser traffic instead of presenting a unique minimal fingerprint

pub mod headers;
pub mod h2_settings;
pub mod client_hints;

pub use headers::{HttpProfile, HeaderOrder, build_request_headers};
pub use h2_settings::{H2Settings, H2StreamPriority};
pub use client_hints::{ClientHintsProfile, should_send_client_hints, client_hints_headers};

/// HTTP/1.1 User-Agent value for Lumen.
pub const DEFAULT_USER_AGENT: &str = "Lumen/0.0.1";

/// Default Accept-Language header (does not leak real locale).
pub const DEFAULT_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

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
