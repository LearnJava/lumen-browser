//! HTTP/2 connection pool — RFC 9113 §9.1.1.
//!
//! Stores one live [`H2Conn`] per `(host, port, is_tls)` origin. Requests to
//! the same origin reuse the existing connection's stream multiplexing instead
//! of opening a new TLS socket and repeating the connection preface exchange.
//!
//! ## Design constraints (Phase 0)
//!
//! - **One connection per origin.** RFC 9113 §9.1.1 says clients _should_ open
//!   only a single connection; for a single-user browser this is fine.
//! - **No idle timeout.** H2 servers send `GOAWAY` when they close; the caller
//!   detects errors and lets the pool drop the connection naturally.
//! - **No concurrent multiplexing within one request.** The synchronous I/O
//!   model serialises requests on a connection — stream 1, then 3, 5 etc. True
//!   interleaved concurrent streams require async I/O (Phase 1+).
//!
//! ## Flow
//!
//! ```text
//! h2_pool.acquire(key) → Some(conn)  // reuse existing socket + stream ID seq
//!     use conn, release back
//! h2_pool.acquire(key) → None        // no entry yet
//!     fresh connect → H2Conn::connect(stream)
//!     use conn, h2_pool.release(key, conn)
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use super::conn::H2Conn;
use crate::{pool::PoolKey, RawStream};

/// A shared pool of HTTP/2 connections, one per origin.
#[derive(Default)]
pub struct H2Pool {
    entries: Mutex<HashMap<PoolKey, H2Conn<RawStream>>>,
}

impl H2Pool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove and return the pooled connection for `key`, if any.
    pub(crate) fn acquire(&self, key: &PoolKey) -> Option<H2Conn<RawStream>> {
        self.entries.lock().unwrap().remove(key)
    }

    /// Return a connection to the pool. If an entry already exists (e.g. the
    /// caller created a new conn after failing to acquire), the new one wins.
    pub(crate) fn release(&self, key: PoolKey, conn: H2Conn<RawStream>) {
        self.entries.lock().unwrap().insert(key, conn);
    }

    /// Discard the entry for `key` (called after an unrecoverable error).
    pub(crate) fn evict(&self, key: &PoolKey) {
        self.entries.lock().unwrap().remove(key);
    }

    /// Number of pooled connections (for tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(host: &str, port: u16, is_tls: bool) -> PoolKey {
        PoolKey { host: host.to_owned(), port, is_tls }
    }

    #[test]
    fn new_pool_is_empty() {
        let pool = H2Pool::new();
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn acquire_on_empty_pool_returns_none() {
        let pool = H2Pool::new();
        let key = make_key("example.com", 443, true);
        assert!(pool.acquire(&key).is_none());
    }

    #[test]
    fn evict_on_empty_pool_is_noop() {
        let pool = H2Pool::new();
        let key = make_key("example.com", 443, true);
        // Must not panic.
        pool.evict(&key);
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn default_gives_empty_pool() {
        let pool = H2Pool::default();
        assert_eq!(pool.len(), 0);
    }
}
