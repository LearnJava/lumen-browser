//! HTTP/2 client implementation (RFC 9113).
//!
//! Sprint 5A breakdown:
//!
//! - 5A.1 — ALPN negotiation (in [`crate::lib`], `default_tls_config`).
//! - 5A.2 — frame codec ([`frame`]).
//! - 5A.3 — HPACK header compression (planned).
//! - 5A.4 — connection driver: preface + SETTINGS exchange + single GET (planned).
//! - 5A.5 — stream multiplexing inside a single connection (planned).
//! - 5A.6 — flow control + WINDOW_UPDATE (planned).
//!
//! The codec is pure (no IO, no connection state). Higher layers build on top:
//! the connection driver feeds bytes through [`frame::Frame::parse`] and emits
//! frames through [`frame::Frame::encode`]; HPACK consumes/produces field block
//! fragments carried inside HEADERS/CONTINUATION/PUSH_PROMISE.

pub mod frame;
pub mod hpack;
