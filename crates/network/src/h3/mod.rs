//! HTTP/3 client implementation (RFC 9114) over QUIC (RFC 9000).
//!
//! Slice breakdown (mirrors the HTTP/2 sprint 5A layering — pure codecs first,
//! IO/connection state later):
//!
//! - Slice 1 — QUIC variable-length integer codec ([`varint`], RFC 9000 §16)
//!   and the HTTP/3 frame codec ([`frame`], RFC 9114 §7.2). Pure parse/
//!   serialize, no IO, no connection state.
//! - Slice 2+ (planned) — QPACK header compression (RFC 9204), QUIC transport
//!   (UDP datagrams, TLS 1.3 handshake, packet protection, loss recovery,
//!   congestion control), unidirectional/request stream framing, and
//!   `h3_do_request` dispatch alongside the existing H1/H2 paths.
//!
//! The codecs here are the shared foundation: QUIC varints delimit both
//! transport-layer fields and HTTP/3 frames, and the frame codec is driven by
//! the connection layer once QUIC streams exist.

pub mod frame;
pub mod varint;
