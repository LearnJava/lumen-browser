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
//! - Tor: minimized header fingerprint, tor-browser-compatible configuration

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
