//! WebDriver BiDi server (Phase 1, §6.11, ADR-006).
//!
//! `lumen --bidi-port N` starts a W3C WebDriver BiDi WebSocket server on
//! `127.0.0.1:N`. Three-layer structure:
//!   - `server`    — TCP accept loop (one thread per connection)
//!   - `transport` — WebSocket framing (RFC 6455) + read/write loop
//!   - `protocol`  — pure BiDi state machine (no I/O)
//!
//! Implemented modules: `session.*`, `browsingContext.*`, `script.*`,
//! `network.*`, `input.*`, `browser.*`, `emulation.*`.
//!
//! Live wiring to the actual engine (`domContentLoaded`, cookie events) is
//! a P3 handoff — roadmap 8H.3.

mod protocol;
mod server;
mod transport;

pub use server::spawn;
