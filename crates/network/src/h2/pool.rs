//! HTTP/2 connection pool — RFC 9113 §9.1.1 (PERF-13).
//!
//! Holds at most one shared [`H2Mux`] per `(host, port, is_tls)` origin. A
//! request *borrows* the connection (an `Arc` clone) instead of taking it out
//! of the pool, so any number of concurrent requests to one origin run as
//! streams of the same TCP+TLS connection (RFC 9113 §9.1: "clients SHOULD NOT
//! open more than one HTTP/2 connection to a given host and port pair").
//!
//! ## Connect coalescing
//!
//! A page load fires dozens of subresource fetches to one origin at the same
//! moment, before any connection to it exists. Without coordination each of
//! them would open its own connection — exactly BUG-1115. So the first
//! requester for an origin *reserves* it ([`Acquire::Connect`]) and performs
//! the handshake; the others wait ([`CONNECT_WAIT`] at most) and then share
//! the connection it installs. Whether an origin speaks HTTP/2 at all is only
//! known after ALPN, so a reservation can also end with
//! [`Reservation::mark_http1`] — the origin is remembered as HTTP/1.1 and
//! later requests to it no longer wait for anything.
//!
//! ## Flow
//!
//! ```text
//! acquire(key) → Mux(m)      // live connection: send a stream on it
//! acquire(key) → Connect(r)  // we open it: connect, then r.fulfill(mux)
//!                            //   or r.mark_http1(); dropping r = failed
//! acquire(key) → Direct      // HTTP/1.1 origin, or waited in vain: go
//!                            //   through the HTTP/1.1 pool / own connect
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::mux::{H2Mux, MuxError};
use crate::pool::PoolKey;

/// Longest a requester waits for another thread's handshake to the same
/// origin before opening its own connection. A safety net against a stuck
/// reservation only: it is above the connect path's own bounds (10 s TCP
/// connect, 60 s handshake read timeout), so a reservation normally resolves
/// first. Giving up earlier bought nothing — on a slow route every
/// independent handshake is just as slow, and a live github.com load with a
/// 15 s wait opened 24 connections to one asset host instead of 1.
const CONNECT_WAIT: Duration = Duration::from_secs(75);

enum Slot {
    /// A requester is connecting; others wait on the pool's condvar.
    Connecting,
    /// Live shared connection.
    Ready(Arc<H2Mux>),
    /// The origin negotiated HTTP/1.1 — never wait for it again.
    Http1,
}

/// Outcome of [`H2Pool::acquire`].
pub(crate) enum Acquire<'a> {
    /// A live connection: submit the request to it.
    Mux(Arc<H2Mux>),
    /// No connection yet and this requester has to open it.
    Connect(Reservation<'a>),
    /// Don't use the HTTP/2 pool for this request.
    Direct,
}

/// The right (and duty) to open the connection for one origin. Resolve it
/// with [`Self::fulfill`] or [`Self::mark_http1`]; dropping it unresolved
/// (connect failed) releases the waiters to try on their own.
pub(crate) struct Reservation<'a> {
    pool: &'a H2Pool,
    key: Option<PoolKey>,
}

impl Reservation<'_> {
    /// Install the freshly opened connection and wake the waiters.
    pub(crate) fn fulfill(mut self, mux: H2Mux) -> Arc<H2Mux> {
        let mux = Arc::new(mux);
        if let Some(key) = self.key.take() {
            self.pool.set(key, Some(Slot::Ready(Arc::clone(&mux))));
        }
        mux
    }

    /// The origin answered with HTTP/1.1 (no `h2` in ALPN).
    pub(crate) fn mark_http1(mut self) {
        if let Some(key) = self.key.take() {
            self.pool.set(key, Some(Slot::Http1));
        }
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.pool.set(key, None);
        }
    }
}

/// A shared pool of HTTP/2 connections, one per origin.
#[derive(Default)]
pub struct H2Pool {
    slots: Mutex<HashMap<PoolKey, Slot>>,
    changed: Condvar,
}

impl std::fmt::Debug for H2Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H2Pool").field("origins", &self.lock().len()).finish()
    }
}

impl H2Pool {
    /// An empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PoolKey, Slot>> {
        // A panic while holding the map leaves it consistent (every update is
        // a single insert/remove), so a poisoned lock is safe to keep using.
        self.slots.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set(&self, key: PoolKey, slot: Option<Slot>) {
        let mut slots = self.lock();
        match slot {
            Some(s) => slots.insert(key, s),
            None => slots.remove(&key),
        };
        drop(slots);
        self.changed.notify_all();
    }

    /// Get the shared connection for `key`, the duty to open it, or a
    /// verdict to bypass HTTP/2 pooling (see the module docs).
    pub(crate) fn acquire(&self, key: &PoolKey) -> Acquire<'_> {
        let deadline = Instant::now() + CONNECT_WAIT;
        let mut waited = false;
        let mut slots = self.lock();
        loop {
            match slots.get(key) {
                Some(Slot::Ready(mux)) if mux.is_usable() => return Acquire::Mux(Arc::clone(mux)),
                Some(Slot::Http1) => return Acquire::Direct,
                Some(Slot::Connecting) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Acquire::Direct;
                    }
                    waited = true;
                    slots = self
                        .changed
                        .wait_timeout(slots, deadline - now)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0;
                }
                // Somebody else's handshake just failed: don't queue behind
                // the next attempt, connect independently like before pooling.
                _ if waited => return Acquire::Direct,
                // Empty, or a connection that died / got GOAWAY.
                _ => {
                    slots.insert(key.clone(), Slot::Connecting);
                    return Acquire::Connect(Reservation { pool: self, key: Some(key.clone()) });
                }
            }
        }
    }

    /// Offer a connection opened outside a reservation (the requester was
    /// told [`Acquire::Direct`] but the origin turned out to speak HTTP/2).
    /// Installed only if the origin has no live connection or pending
    /// handshake; either way the caller keeps using its own handle.
    pub(crate) fn offer(&self, key: &PoolKey, mux: H2Mux) -> Arc<H2Mux> {
        let mux = Arc::new(mux);
        let mut slots = self.lock();
        let occupied = matches!(slots.get(key), Some(Slot::Connecting))
            || matches!(slots.get(key), Some(Slot::Ready(m)) if m.is_usable());
        if !occupied {
            slots.insert(key.clone(), Slot::Ready(Arc::clone(&mux)));
            drop(slots);
            self.changed.notify_all();
        }
        mux
    }

    /// Drop `mux` from the pool if it is still the entry for `key` (after a
    /// failure on it). A newer connection someone else installed is kept.
    pub(crate) fn evict(&self, key: &PoolKey, mux: &Arc<H2Mux>) {
        let mut slots = self.lock();
        if matches!(slots.get(key), Some(Slot::Ready(m)) if Arc::ptr_eq(m, mux)) {
            slots.remove(key);
        }
    }

    /// Number of origins with a live shared connection.
    pub fn live_connections(&self) -> usize {
        self.lock()
            .values()
            .filter(|s| matches!(s, Slot::Ready(m) if m.is_usable()))
            .count()
    }

    /// Number of tracked origins in any state (for tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }
}

/// How many times one request is resent after its connection lost it
/// unprocessed — the same budget as Chromium's `HttpNetworkTransaction`
/// (`kMaxRetryAttempts = 2`). Bounded, so a peer that drops every connection
/// cannot keep a request looping.
pub(crate) const H2_RESEND_LIMIT: u32 = 2;

/// Where [`send_resending`] stopped.
pub(crate) enum Sent<'a, R> {
    /// A response, or an error that must not be resent (or the resend budget
    /// is spent).
    Done(Result<R, MuxError>),
    /// There is no live connection to send on: open one. `Some` — this
    /// requester holds the origin's reservation; `None` — bypass the pool
    /// (HTTP/1.1 origin, no pool, or a failed handshake elsewhere).
    Connect(Option<Reservation<'a>>),
}

/// Send one request over the origin's shared HTTP/2 connection, resending it
/// on a fresh connection whenever the peer certainly did not process it
/// ([`MuxError::retryable`]: queued behind a dying connection, refused by
/// GOAWAY, lost before any response header) — BUG-1177.
///
/// `opened` is a connection this requester has just opened itself (the
/// request that triggers the handshake must be resent exactly like the ones
/// that found the connection in the pool); `None` starts from the pool. A
/// dead connection is evicted before the next attempt, so the requesters it
/// failed coalesce on one new handshake instead of each opening its own.
pub(crate) fn send_resending<'a, R>(
    pool: Option<&'a H2Pool>,
    key: &PoolKey,
    opened: Option<Arc<H2Mux>>,
    resends_left: &mut u32,
    send: &dyn Fn(&H2Mux) -> Result<R, MuxError>,
) -> Sent<'a, R> {
    let mut next = opened;
    loop {
        let mux = match next.take() {
            Some(m) => m,
            None => match pool.map(|p| p.acquire(key)) {
                Some(Acquire::Mux(m)) => m,
                Some(Acquire::Connect(r)) => return Sent::Connect(Some(r)),
                Some(Acquire::Direct) | None => return Sent::Connect(None),
            },
        };
        match send(&mux) {
            Err(e) if e.retryable && *resends_left > 0 => {
                if let Some(p) = pool {
                    p.evict(key, &mux);
                }
                *resends_left -= 1;
            }
            other => return Sent::Done(other),
        }
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
        assert_eq!(pool.live_connections(), 0);
    }

    #[test]
    fn first_acquire_reserves_the_origin() {
        let pool = H2Pool::new();
        let key = make_key("example.com", 443, true);
        assert!(matches!(pool.acquire(&key), Acquire::Connect(_)));
    }

    #[test]
    fn dropped_reservation_frees_the_origin() {
        let pool = H2Pool::new();
        let key = make_key("example.com", 443, true);
        match pool.acquire(&key) {
            Acquire::Connect(r) => drop(r),
            _ => panic!("expected a reservation"),
        }
        assert_eq!(pool.len(), 0);
        assert!(matches!(pool.acquire(&key), Acquire::Connect(_)));
    }

    #[test]
    fn http1_origin_is_not_reserved_again() {
        let pool = H2Pool::new();
        let key = make_key("example.com", 443, true);
        match pool.acquire(&key) {
            Acquire::Connect(r) => r.mark_http1(),
            _ => panic!("expected a reservation"),
        }
        assert!(matches!(pool.acquire(&key), Acquire::Direct));
    }

    #[test]
    fn waiter_goes_direct_when_the_handshake_fails() {
        let pool = Arc::new(H2Pool::new());
        let key = make_key("example.com", 443, true);
        let Acquire::Connect(r) = pool.acquire(&key) else {
            panic!("expected a reservation");
        };
        let waiter = {
            let pool = Arc::clone(&pool);
            let key = key.clone();
            std::thread::spawn(move || matches!(pool.acquire(&key), Acquire::Direct))
        };
        std::thread::sleep(Duration::from_millis(50));
        drop(r);
        assert!(waiter.join().unwrap_or(false), "waiter must bypass after a failed handshake");
    }

    #[test]
    fn different_origins_do_not_block_each_other() {
        let pool = H2Pool::new();
        let _a = pool.acquire(&make_key("a.example", 443, true));
        assert!(matches!(pool.acquire(&make_key("b.example", 443, true)), Acquire::Connect(_)));
    }
}
