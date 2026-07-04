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
//! - Slice 9 — the QUIC loss-detection timer + PTO ([`pto`], RFC 9002 §6.2,
//!   Appendix A): [`pto::LossDetection`] ties the three per-space registries and
//!   the RTT estimator into the single loss-detection timer. It computes
//!   `SetLossDetectionTimer` (earliest time-threshold loss time, else the
//!   exponentially-backed-off probe timeout, else disarm) and drives
//!   `OnLossDetectionTimeout` (declare time-threshold losses, or send one/two
//!   ack-eliciting probes and bump the backoff), including the anti-deadlock PTO
//!   and the Application-Data-until-handshake-confirmed guard. Pure state machine
//!   driven by a caller-supplied clock; no timer IO, no probe assembly.
//! - Slice 10 — the QUIC stream data model ([`stream`], RFC 9000 §2, §3, §4):
//!   the per-stream receive reassembly buffer, per-stream flow-control
//!   accounting on both directions (RFC 9000 §4.1), the final-size invariants
//!   (RFC 9000 §4.5), and the send/receive stream state machines (RFC 9000 §3).
//!   [`stream::RecvStream`] merges out-of-order / overlapping STREAM frames into
//!   an ordered byte stream and re-advertises the receive window; [`stream::
//!   SendStream`] buffers application data and emits STREAM frames bounded by the
//!   peer's flow-control limit, advancing to `DataRecvd` on acknowledgement. Pure
//!   state machine driven by decoded frames; no connection-level flow control, no
//!   retransmission, no IO.
//! - Slice 11 — the connection-level flow control + stream-count limits
//!   ([`conn_flow`], RFC 9000 §4.1, §4.6): the connection-wide `MAX_DATA` budget
//!   that caps the sum of stream data across all streams (independent of each
//!   stream's own `MAX_STREAM_DATA`) and the `MAX_STREAMS` budget that caps how
//!   many streams of each direction an endpoint may open. [`conn_flow::
//!   SendConnFlow`] / [`conn_flow::RecvConnFlow`] track the send/receive halves of
//!   the connection data budget; [`conn_flow::SendStreamLimit`] / [`conn_flow::
//!   RecvStreamLimit`] track the send/receive halves of the stream-count budget,
//!   including the block signals (`DATA_BLOCKED` / `STREAMS_BLOCKED`) and the
//!   re-advertisement as data is consumed and streams complete. Pure state
//!   machine driven by the connection layer's sent/received/opened/closed
//!   reports; no IO.
//! - Slice 12 — the HTTP/3 stream layer ([`h3_stream`], RFC 9114 §6.2, §7.1,
//!   §4.1): unidirectional stream-type demux ([`h3_stream::UniStreamType`] —
//!   control / push+Push-ID / QPACK encoder / QPACK decoder / reserved), the
//!   "exactly one control / QPACK-encoder / QPACK-decoder stream" rule
//!   ([`h3_stream::UniStreamRegistry`], `H3_STREAM_CREATION_ERROR` on a duplicate,
//!   `H3_CLOSED_CRITICAL_STREAM` on closing one), the control-stream frame grammar
//!   ([`h3_stream::ControlStream`] — first frame is SETTINGS else
//!   `H3_MISSING_SETTINGS`, SETTINGS at most once, no request frames), and the
//!   request/response-stream frame grammar ([`h3_stream::RequestStream`] —
//!   HEADERS+ → DATA* → optional trailer HEADERS, interleaved PUSH_PROMISE,
//!   everything else `H3_FRAME_UNEXPECTED`). Pure state machine over decoded
//!   [`frame::Frame`]s; no IO. Reuses [`frame`]'s type codes and
//!   `H3_FRAME_UNEXPECTED`.
//! - Slice 13 — the QUIC key schedule ([`key_schedule`], RFC 9001 §5.1, §5.2):
//!   the TLS 1.3 HKDF (`HKDF-Extract` / `HKDF-Expand` / `HKDF-Expand-Label`,
//!   RFC 5869 + RFC 8446 §7.1) built on the existing SHA-256 dependency, the
//!   QUIC v1 Initial salt, and the Initial-secret chain that derives both
//!   directions' packet-protection keys (`key` / `iv` / `hp`, labels
//!   `"quic key"` / `"quic iv"` / `"quic hp"`) deterministically from the
//!   client's original Destination Connection ID, plus the `"quic ku"` key
//!   update (§6.1). Pure functions; validated against the RFC 9001 Appendix A.1
//!   test vectors. The header-protection and AEAD transforms that consume this
//!   material are the next slices.
//! - Slice 14+ (planned) — the rest of the QUIC transport (UDP datagrams,
//!   header protection, TLS 1.3 handshake, packet protection, actually arming the
//!   PTO timer and assembling probe datagrams, the QPACK encoder/decoder stream
//!   instruction wiring, and `h3_do_request` dispatch alongside the existing
//!   H1/H2 paths.
//!
//! The codecs here are the shared foundation: QUIC varints delimit both
//! transport-layer fields and HTTP/3 frames, the QUIC frame codec carries the
//! transport payload, the HTTP/3 frame codec carries an opaque QPACK field
//! block, [`qpack`] turns that block into header fields, and [`alt_svc`]
//! decides when an origin is eligible for the QUIC path at all.

pub mod alt_svc;
pub mod conn_flow;
pub mod frame;
pub mod h3_stream;
pub mod key_schedule;
pub mod loss;
pub mod packet;
pub mod pto;
pub mod qpack;
pub mod qpack_stream;
pub mod quic_frame;
pub mod recovery;
pub mod stream;
pub mod varint;
