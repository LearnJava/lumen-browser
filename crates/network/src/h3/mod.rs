//! HTTP/3 client implementation (RFC 9114) over QUIC (RFC 9000).
//!
//! Slice breakdown (mirrors the HTTP/2 sprint 5A layering — pure codecs first,
//! IO/connection state later):
//!
//! - Slice 1 — QUIC variable-length integer codec ([`varint`], RFC 9000 §16)
//!   and the HTTP/3 frame codec ([`frame`], RFC 9114 §7.2). Pure parse/
//!   serialize, no IO, no connection state.
//! - Slice 2 — QPACK field-section codec ([`qpack`], RFC 9204), static table
//!   only (the wire behaviour of a peer advertising a zero-size dynamic
//!   table). Pure encode/decode of the header block carried in HEADERS /
//!   PUSH_PROMISE frames; no dynamic table, no encoder/decoder streams.
//! - Slice 3 — the `Alt-Svc` discovery layer ([`alt_svc`], RFC 7838): parses
//!   the response header that advertises HTTP/3 for an origin and caches the
//!   `h3` alternatives per origin with TTL expiry. Pure parse + in-memory
//!   cache, no IO on the parse path (only the `*_now` cache wrappers read the
//!   clock). This is the trigger that later routes a request onto QUIC.
//! - Slice 4 — the QUIC transport frame codec ([`quic_frame`], RFC 9000 §19):
//!   pure parse/serialize of every QUIC frame type (PADDING…HANDSHAKE_DONE)
//!   the connection layer exchanges inside a packet's protected payload, on
//!   the same [`varint`] primitive as the HTTP/3 frame codec. No packet
//!   protection, no packet-number spaces, no IO.
//! - Slice 5 — the QUIC packet header codec ([`packet`], RFC 9000 §17): pure
//!   parse/serialize of every packet shape (Initial, 0-RTT, Handshake, Retry,
//!   Version Negotiation, and the short 1-RTT header), carrying the
//!   header-protected first-byte bits and the AEAD-protected payload verbatim
//!   as opaque bytes. No header protection, no packet protection, no IO. This
//!   is the frame the connection layer parses first, before removing header
//!   protection and AEAD-decrypting the payload into [`quic_frame`] frames.
//! - Slice 6 — the QPACK dynamic table + instruction streams ([`qpack_stream`],
//!   RFC 9204 §3.2, §4.3, §4.4): the shared dynamic table (byte-budget
//!   capacity, FIFO eviction, absolute/relative indexing) plus the encoder
//!   stream (Set Capacity / Insert With Name Reference / Insert With Literal
//!   Name / Duplicate) and the decoder stream (Section Acknowledgment / Stream
//!   Cancellation / Insert Count Increment). Pure parse/serialize plus the
//!   in-memory table; applying an encoder stream reproduces the peer's table
//!   state. No IO, no unidirectional-stream framing.
//! - Slice 7 — the QUIC RTT estimator + NewReno congestion controller
//!   ([`recovery`], RFC 9002 §5, §7): pure state machines the loss-recovery
//!   layer drives with acked/lost packets. The estimator produces the smoothed
//!   RTT and probe timeout (RFC 9002 §6.2.1); the controller tracks the
//!   congestion window through slow start, congestion avoidance, and recovery,
//!   halving it on loss (RFC 9002 §7.3.2) and collapsing it under persistent
//!   congestion (RFC 9002 §7.6). No sent-packet registry, no loss detection, no
//!   IO — that is the next slice.
//! - Slice 8 — the QUIC sent-packet registry + loss detection ([`loss`],
//!   RFC 9002 §6): the per-packet-number-space registry of in-flight packets,
//!   ack processing that removes newly-acknowledged packets and produces the RTT
//!   sample, and the packet-threshold (§6.1.1) and time-threshold (§6.1.2) loss
//!   detection that decides which packets are lost and feeds [`recovery`]. Pure
//!   state machine driven by decoded ACK frames and a caller-supplied clock; no
//!   PTO timer, no IO.
//! - Slice 9+ (planned) — the rest of the QUIC transport (UDP datagrams,
//!   header protection, TLS 1.3 handshake, packet protection, the PTO timer and
//!   probe sending (RFC 9002 §6.2), unidirectional/request stream framing, and
//!   `h3_do_request` dispatch alongside the existing H1/H2 paths.
//!
//! The codecs here are the shared foundation: QUIC varints delimit both
//! transport-layer fields and HTTP/3 frames, the QUIC frame codec carries the
//! transport payload, the HTTP/3 frame codec carries an opaque QPACK field
//! block, [`qpack`] turns that block into header fields, and [`alt_svc`]
//! decides when an origin is eligible for the QUIC path at all.

pub mod alt_svc;
pub mod frame;
pub mod loss;
pub mod packet;
pub mod qpack;
pub mod qpack_stream;
pub mod quic_frame;
pub mod recovery;
pub mod varint;
