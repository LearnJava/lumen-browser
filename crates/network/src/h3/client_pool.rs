//! QUIC connection pool for HTTP/3 reuse (RFC 9000 §10.1.2, RFC 9114 §3.3).
//!
//! A [`RequestDriver<UdpDatagram>`](super::request_driver::RequestDriver) remains
//! usable after a request completes — it holds the live QUIC connection state and
//! accepts a new [`send_request`](super::request_driver::RequestDriver::send_request)
//! call immediately. This pool stores confirmed drivers keyed by origin
//! (`h3::alt_svc::origin_key(host, port)`) so the next request to the same origin
//! skips the TLS 1.3 handshake (≈1 RTT) and reuses the existing QUIC connection.
//!
//! ## Idle expiry
//!
//! Entries are evicted on access when they have been idle for more than
//! [`MAX_IDLE`]. The default QUIC idle timeout advertised by
//! [`ClientConnectConfig`](super::client_bootstrap::ClientConnectConfig) is 30 s;
//! `MAX_IDLE` is set to 25 s so the pool always evicts before the peer does.
//!
//! ## Sequential-only reuse
//!
//! The H3 leg in `lib.rs` is synchronous — one request at a time. The pool holds
//! at most one driver per origin; a concurrent second request to the same origin
//! opens its own fresh connection, and whichever leg finishes last wins the slot.
//!
//! ## Failure handling
//!
//! When a pooled driver fails (the server closed the connection or idle timeout
//! fired), the caller drops the driver (already taken out of the pool) and falls
//! back to [`h3_connect`](super::client_transport::h3_connect) for a fresh
//! connection, mirroring the RFC 7838 §2.4 "broken alternative" pattern on the
//! Alt-Svc cache.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::request_driver::RequestDriver;
use super::udp::UdpDatagram;

/// Connections idle for longer than this are treated as stale and evicted.
///
/// The server closes the connection within 30 s (the default `max_idle_timeout`
/// in [`ClientConnectConfig`](super::client_bootstrap::ClientConnectConfig));
/// 25 s ensures the pool never hands back a connection the peer already closed.
const MAX_IDLE: Duration = Duration::from_secs(25);

struct PoolEntry {
    driver: RequestDriver<UdpDatagram>,
    last_used: Instant,
}

/// A pool of live QUIC / HTTP-3 [`RequestDriver`](super::request_driver::RequestDriver)s
/// keyed by `h3::alt_svc::origin_key(host, port)`.
///
/// Shared inside [`HttpClient`](crate::HttpClient) via `Arc<Mutex<H3ConnectionPool>>`;
/// the `Mutex` is only held during HashMap operations — never across I/O.
pub struct H3ConnectionPool {
    entries: HashMap<String, PoolEntry>,
}

impl H3ConnectionPool {
    /// Create an empty pool.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    /// Remove and return the driver for `key` if one exists and is not stale.
    ///
    /// Returns `None` if no entry exists for `key`, or if the entry has been
    /// idle for more than [`MAX_IDLE`].
    pub(crate) fn take(
        &mut self,
        key: &str,
        now: Instant,
    ) -> Option<RequestDriver<UdpDatagram>> {
        match self.entries.remove(key) {
            Some(entry) if now.duration_since(entry.last_used) <= MAX_IDLE => {
                Some(entry.driver)
            }
            // Expired or absent: silently drop.
            _ => None,
        }
    }

    /// Store `driver` under `key` for reuse on the next request to the same origin.
    ///
    /// Replaces any previous entry; the superseded connection is closed on drop.
    pub(crate) fn put(
        &mut self,
        key: String,
        driver: RequestDriver<UdpDatagram>,
        now: Instant,
    ) {
        self.entries.insert(key, PoolEntry { driver, last_used: now });
    }

    /// Whether the pool holds an entry for `key`.
    #[cfg(test)]
    pub(crate) fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }
}

impl Default for H3ConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh pool has no entries and returns `None` for any key.
    #[test]
    fn empty_pool_returns_none() {
        let mut pool = H3ConnectionPool::new();
        assert!(!pool.contains("example.com:443"));
        assert!(pool.take("example.com:443", Instant::now()).is_none());
    }

    /// `MAX_IDLE` is shorter than the default server idle timeout (30 s).
    #[test]
    fn max_idle_is_below_server_idle_timeout() {
        assert!(MAX_IDLE.as_secs() < 30, "pool evicts before the peer closes");
    }
}
