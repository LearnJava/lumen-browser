//! QUIC Path MTU Discovery (DPLPMTUD) — RFC 9000 §14.2–14.4, RFC 8899.
//!
//! QUIC runs Datagram Packetization Layer Path MTU Discovery (DPLPMTUD,
//! RFC 8899) to find the largest UDP payload — the *maximum datagram size* — a
//! path can carry without fragmentation (RFC 9000 §14.2). Knowing this size lets
//! the sender build bigger packets, cutting per-packet overhead, and it bounds
//! the congestion window ([`recovery`](super::recovery)), whose units are
//! `max_datagram_size`.
//!
//! This module is the pure state machine that drives that search. It performs
//! no IO: the connection layer asks [`PathMtuDiscovery::next_probe`] for the
//! size of the next *PMTU probe* to send — an ack-eliciting packet padded to a
//! candidate size (RFC 9000 §14.4) — sends it, and reports the outcome back with
//! [`PathMtuDiscovery::on_probe_sent`], [`PathMtuDiscovery::on_probe_acked`], and
//! [`PathMtuDiscovery::on_probe_lost`]. The current validated size is
//! [`PathMtuDiscovery::max_datagram_size`].
//!
//! ## Search (RFC 8899 §5.3)
//!
//! The search is a binary search between the largest confirmed size and the
//! upper bound. Starting from the base size, [`PathMtuDiscovery::next_probe`]
//! proposes the midpoint of the remaining `(confirmed, search_high]` range; an
//! acknowledged probe raises the confirmed size (and the search floor), while a
//! probe that is lost [`MAX_PROBES`] times in a row lowers `search_high` below
//! that size. The search ends when the floor meets `search_high`, leaving the
//! largest acknowledged size as [`PathMtuDiscovery::max_datagram_size`].
//!
//! ## Base confirmation (RFC 8899 §5.2)
//!
//! DPLPMTUD begins in the [`PmtuState::Base`] state, confirming the path can
//! carry the [`QUIC_MIN_PLPMTU`] base size before searching upward. In QUIC that
//! is usually already true — a client's Initial datagrams are expanded to 1200
//! bytes (RFC 9000 §14.1), so completing the handshake proves the base size; a
//! connection can skip straight to the search with
//! [`PathMtuDiscovery::with_confirmed_base`]. If the base size itself cannot be
//! confirmed the machine enters [`PmtuState::Error`]: the path cannot carry
//! QUIC's minimum and the connection must abandon this path.
//!
//! ## Black-hole detection (RFC 8899 §5.4, RFC 9000 §14.4)
//!
//! If ordinary datagrams at the current size start disappearing — a path that
//! silently drops packets above some size — the connection reports
//! [`PathMtuDiscovery::on_black_hole`], which drops the confirmed size back to
//! the base and restarts the search.
//!
//! ## Out of scope
//!
//! - The actual UDP send/receive and assembling the padded probe packet.
//! - Excluding lost probe packets from the congestion controller
//!   ([`recovery`](super::recovery)): RFC 9000 §14.4 requires a lost PMTU probe
//!   *not* to be treated as a congestion signal, which is the caller's job — it
//!   must route probe losses here, not to the loss-recovery layer.
//! - Reacting to an ICMP "Packet Too Big" message (RFC 9000 §14.2.1); this
//!   machine is driven only by probe acknowledgement and loss.

/// The base — and minimum — Packetization Layer PMTU for QUIC over IPv4/IPv6,
/// in bytes of UDP payload (RFC 9000 §14.1, §8.1). A QUIC sender must be able to
/// carry a datagram at least this large; a path that cannot is unusable.
pub const QUIC_MIN_PLPMTU: usize = 1200;

/// The number of consecutive lost probes at one candidate size before that size
/// is declared unsupported (RFC 8899 §5.1.1, `MAX_PROBES`).
pub const MAX_PROBES: u8 = 3;

/// The phase of the DPLPMTUD search (RFC 8899 §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmtuState {
    /// Confirming the path can carry the base [`QUIC_MIN_PLPMTU`] size before
    /// searching upward (RFC 8899 §5.2, `BASE`).
    Base,
    /// The base size is confirmed; probing upward for a larger size
    /// (RFC 8899 §5.2, `SEARCHING`).
    Searching,
    /// The search finished; [`PathMtuDiscovery::max_datagram_size`] is the
    /// largest confirmed size (RFC 8899 §5.2, `SEARCH_COMPLETE`).
    SearchComplete,
    /// The base size could not be confirmed — the path cannot carry QUIC's
    /// minimum datagram (RFC 8899 §5.2, `ERROR`).
    Error,
}

/// The candidate size currently being probed and how many probes have been sent
/// for it without acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pending {
    /// The probe size, in bytes of UDP payload.
    size: usize,
    /// Probes sent at `size` so far without an acknowledgement (capped by
    /// [`MAX_PROBES`]).
    count: u8,
}

/// The QUIC DPLPMTUD state machine (RFC 9000 §14.2–14.4, RFC 8899).
///
/// Pure state machine driven by probe acknowledgement / loss reports from the
/// connection layer; performs no IO and no probe-packet assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathMtuDiscovery {
    /// The current search phase.
    state: PmtuState,
    /// The largest confirmed max datagram size, in bytes. Never below `base`.
    plpmtu: usize,
    /// The base (minimum) size — the floor the search never drops below.
    base: usize,
    /// The upper bound the search will never probe above (the smaller of the
    /// peer's `max_udp_payload_size` and the local link MTU).
    max: usize,
    /// The current upper bound of the remaining search range; candidate sizes
    /// above this have been ruled out. The search ends when `plpmtu == search_high`.
    search_high: usize,
    /// The size of the probe currently in flight, if any. While set,
    /// [`Self::next_probe`] yields nothing — one probe is outstanding at a time.
    in_flight: Option<usize>,
    /// The candidate size being validated and its probe count, if a search step
    /// is underway.
    pending: Option<Pending>,
}

impl PathMtuDiscovery {
    /// Creates a discovery machine that first confirms the [`QUIC_MIN_PLPMTU`]
    /// base size, then searches up to `max_plpmtu` (clamped to at least the base).
    ///
    /// Starts in [`PmtuState::Base`]. If `max_plpmtu` is at or below the base the
    /// machine still confirms the base but has nothing to search, moving to
    /// [`PmtuState::SearchComplete`] once the base is acknowledged.
    pub fn new(max_plpmtu: usize) -> Self {
        let base = QUIC_MIN_PLPMTU;
        let max = max_plpmtu.max(base);
        Self {
            state: PmtuState::Base,
            plpmtu: base,
            base,
            max,
            search_high: max,
            in_flight: None,
            pending: None,
        }
    }

    /// Creates a discovery machine that treats the [`QUIC_MIN_PLPMTU`] base size
    /// as already confirmed and begins searching immediately, up to `max_plpmtu`
    /// (clamped to at least the base).
    ///
    /// This is the usual QUIC entry point: a completed handshake proves the base
    /// size, because a client expands its Initial datagrams to 1200 bytes
    /// (RFC 9000 §14.1). Starts in [`PmtuState::Searching`], or
    /// [`PmtuState::SearchComplete`] when `max_plpmtu` leaves nothing to probe.
    pub fn with_confirmed_base(max_plpmtu: usize) -> Self {
        let base = QUIC_MIN_PLPMTU;
        let max = max_plpmtu.max(base);
        let state = if max > base {
            PmtuState::Searching
        } else {
            PmtuState::SearchComplete
        };
        Self {
            state,
            plpmtu: base,
            base,
            max,
            search_high: max,
            in_flight: None,
            pending: None,
        }
    }

    /// The current validated max datagram size, in bytes of UDP payload — the
    /// value that bounds the congestion window ([`recovery`](super::recovery))
    /// and the size of the packets the sender may build.
    pub fn max_datagram_size(&self) -> usize {
        self.plpmtu
    }

    /// The current search phase.
    pub fn state(&self) -> PmtuState {
        self.state
    }

    /// Whether the search has finished (either successfully or in error) and no
    /// further probing will happen without a black-hole restart.
    pub fn is_complete(&self) -> bool {
        matches!(self.state, PmtuState::SearchComplete | PmtuState::Error)
    }

    /// The midpoint of the remaining `(plpmtu, search_high]` search range, biased
    /// upward so it is always strictly greater than the confirmed `plpmtu` and at
    /// most `search_high`. Only valid when `plpmtu < search_high`.
    fn search_midpoint(&self) -> usize {
        debug_assert!(self.plpmtu < self.search_high);
        self.plpmtu + (self.search_high - self.plpmtu).div_ceil(2)
    }

    /// The size of the next PMTU probe to send, or `None` if none is wanted right
    /// now.
    ///
    /// Returns `None` while a probe is already in flight (only one probe is
    /// outstanding at a time), when the search is complete, or when the path
    /// failed base confirmation. Otherwise returns the candidate size: the base
    /// size in [`PmtuState::Base`], or the current search midpoint (or the size
    /// being retried after a prior loss) in [`PmtuState::Searching`]. The caller
    /// builds an ack-eliciting datagram padded to that size and then calls
    /// [`Self::on_probe_sent`].
    pub fn next_probe(&mut self) -> Option<usize> {
        if self.in_flight.is_some() {
            return None;
        }
        match self.state {
            PmtuState::Base => {
                // Confirm the base size. Reuse the pending target across retries.
                let size = self.pending.map_or(self.base, |p| p.size);
                if self.pending.is_none() {
                    self.pending = Some(Pending { size, count: 0 });
                }
                Some(size)
            }
            PmtuState::Searching => {
                if self.plpmtu >= self.search_high {
                    self.state = PmtuState::SearchComplete;
                    return None;
                }
                let size = match self.pending {
                    Some(p) => p.size,
                    None => {
                        let mid = self.search_midpoint();
                        self.pending = Some(Pending { size: mid, count: 0 });
                        mid
                    }
                };
                Some(size)
            }
            PmtuState::SearchComplete | PmtuState::Error => None,
        }
    }

    /// Records that a probe of `size` bytes has been sent. Marks the probe as in
    /// flight and counts the attempt toward [`MAX_PROBES`].
    ///
    /// `size` is expected to match the value [`Self::next_probe`] returned; a
    /// mismatch is tolerated (the machine tracks the pending candidate size,
    /// which governs the outcome handling).
    pub fn on_probe_sent(&mut self, size: usize) {
        self.in_flight = Some(size);
        if let Some(p) = self.pending.as_mut() {
            p.count = p.count.saturating_add(1);
        }
    }

    /// Records that the outstanding PMTU probe of `size` bytes was acknowledged:
    /// the path can carry a datagram this large.
    ///
    /// Raises the confirmed [`Self::max_datagram_size`] to `size` (if larger) and
    /// advances the search floor, so the next [`Self::next_probe`] targets a
    /// larger candidate. The search moves to [`PmtuState::SearchComplete`] once
    /// the confirmed size reaches `search_high`. A stale acknowledgement (not for
    /// the in-flight probe) is ignored.
    pub fn on_probe_acked(&mut self, size: usize) {
        if self.in_flight != Some(size) {
            return;
        }
        self.in_flight = None;
        self.pending = None;
        if size > self.plpmtu {
            self.plpmtu = size.min(self.max);
        }
        match self.state {
            PmtuState::Base => {
                // Base confirmed — start (or finish) the upward search.
                self.state = if self.plpmtu < self.search_high {
                    PmtuState::Searching
                } else {
                    PmtuState::SearchComplete
                };
            }
            PmtuState::Searching => {
                if self.plpmtu >= self.search_high {
                    self.state = PmtuState::SearchComplete;
                }
            }
            PmtuState::SearchComplete | PmtuState::Error => {}
        }
    }

    /// Records that the outstanding PMTU probe of `size` bytes was lost.
    ///
    /// A lost probe is retried up to [`MAX_PROBES`] times; once exhausted, the
    /// candidate size is declared unsupported. In [`PmtuState::Searching`] that
    /// lowers `search_high` below the failed size and continues (ending the
    /// search if nothing larger than the confirmed size remains); in
    /// [`PmtuState::Base`] it means the path cannot carry QUIC's minimum, moving
    /// to [`PmtuState::Error`]. A stale loss report (not for the in-flight probe)
    /// is ignored.
    ///
    /// The caller must not also feed this loss to the congestion controller: a
    /// lost PMTU probe is not a congestion signal (RFC 9000 §14.4).
    pub fn on_probe_lost(&mut self, size: usize) {
        if self.in_flight != Some(size) {
            return;
        }
        self.in_flight = None;
        let Some(pending) = self.pending else {
            return;
        };
        if pending.count < MAX_PROBES {
            // Retry the same candidate on the next `next_probe` call.
            return;
        }
        // Candidate exhausted its probes — it is unsupported.
        self.pending = None;
        match self.state {
            PmtuState::Base => {
                self.state = PmtuState::Error;
            }
            PmtuState::Searching => {
                // Everything at or above `size` is ruled out.
                self.search_high = pending.size.saturating_sub(1).max(self.plpmtu);
                if self.plpmtu >= self.search_high {
                    self.state = PmtuState::SearchComplete;
                }
            }
            PmtuState::SearchComplete | PmtuState::Error => {}
        }
    }

    /// Reports a suspected black hole: ordinary datagrams at the current size are
    /// being lost (RFC 8899 §5.4, RFC 9000 §14.4).
    ///
    /// Drops the confirmed size back to the base and restarts the upward search,
    /// discarding any in-flight probe and search progress. Because the base size
    /// was validated earlier (the connection is established), the machine returns
    /// to [`PmtuState::Searching`] rather than re-confirming the base — unless the
    /// base is already the maximum, in which case there is nothing to search.
    pub fn on_black_hole(&mut self) {
        self.plpmtu = self.base;
        self.search_high = self.max;
        self.in_flight = None;
        self.pending = None;
        self.state = if self.base < self.max {
            PmtuState::Searching
        } else {
            PmtuState::SearchComplete
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the search to completion by acknowledging every probe up to a real
    /// ceiling `path_mtu` (probes above it are lost), returning the machine.
    fn run_to_completion(max: usize, path_mtu: usize) -> PathMtuDiscovery {
        let mut d = PathMtuDiscovery::with_confirmed_base(max);
        // A generous bound on iterations: binary search over the range.
        for _ in 0..256 {
            let Some(size) = d.next_probe() else { break };
            d.on_probe_sent(size);
            if size <= path_mtu {
                d.on_probe_acked(size);
            } else {
                // Lose this candidate until its probe budget is exhausted. Only
                // one probe is in flight at a time, so each retry is send→loss.
                d.on_probe_lost(size);
                loop {
                    match d.next_probe() {
                        Some(again) if again == size => {
                            d.on_probe_sent(again);
                            d.on_probe_lost(again);
                        }
                        // Budget exhausted: the machine has moved to a smaller
                        // candidate (or finished) — hand control back to the loop.
                        _ => break,
                    }
                }
            }
        }
        d
    }

    #[test]
    fn starts_at_base_min() {
        let d = PathMtuDiscovery::new(1500);
        assert_eq!(d.max_datagram_size(), QUIC_MIN_PLPMTU);
        assert_eq!(d.state(), PmtuState::Base);
        assert!(!d.is_complete());
    }

    #[test]
    fn confirmed_base_starts_searching() {
        let d = PathMtuDiscovery::with_confirmed_base(1500);
        assert_eq!(d.state(), PmtuState::Searching);
        assert_eq!(d.max_datagram_size(), QUIC_MIN_PLPMTU);
    }

    #[test]
    fn no_headroom_completes_immediately() {
        // max at or below base leaves nothing to search.
        let mut d = PathMtuDiscovery::with_confirmed_base(QUIC_MIN_PLPMTU);
        assert_eq!(d.state(), PmtuState::SearchComplete);
        assert!(d.is_complete());
        assert_eq!(d.next_probe(), None);

        let d2 = PathMtuDiscovery::with_confirmed_base(1000); // clamped up to base
        assert_eq!(d2.max_datagram_size(), QUIC_MIN_PLPMTU);
        assert_eq!(d2.state(), PmtuState::SearchComplete);
    }

    #[test]
    fn base_probe_confirms_then_searches() {
        let mut d = PathMtuDiscovery::new(1500);
        let size = d.next_probe().expect("base probe");
        assert_eq!(size, QUIC_MIN_PLPMTU);
        d.on_probe_sent(size);
        d.on_probe_acked(size);
        assert_eq!(d.state(), PmtuState::Searching);
    }

    #[test]
    fn base_loss_exhausted_enters_error() {
        let mut d = PathMtuDiscovery::new(1500);
        // MAX_PROBES send→loss cycles at the base size exhaust its budget.
        for _ in 0..MAX_PROBES {
            let size = d.next_probe().expect("base probe");
            assert_eq!(size, QUIC_MIN_PLPMTU);
            d.on_probe_sent(size);
            d.on_probe_lost(size);
        }
        assert_eq!(d.state(), PmtuState::Error);
        assert!(d.is_complete());
        assert_eq!(d.next_probe(), None);
        // The base size remains the effective (unusable) datagram size.
        assert_eq!(d.max_datagram_size(), QUIC_MIN_PLPMTU);
    }

    #[test]
    fn one_probe_outstanding_at_a_time() {
        let mut d = PathMtuDiscovery::with_confirmed_base(1500);
        let size = d.next_probe().expect("first probe");
        d.on_probe_sent(size);
        // No second probe while one is in flight.
        assert_eq!(d.next_probe(), None);
    }

    #[test]
    fn midpoint_biases_upward() {
        // base 1200, max 1500 → first midpoint = 1200 + ceil(300/2) = 1350.
        let mut d = PathMtuDiscovery::with_confirmed_base(1500);
        assert_eq!(d.next_probe(), Some(1350));
    }

    #[test]
    fn ack_raises_confirmed_size() {
        let mut d = PathMtuDiscovery::with_confirmed_base(1500);
        let size = d.next_probe().expect("probe");
        d.on_probe_sent(size);
        d.on_probe_acked(size);
        assert_eq!(d.max_datagram_size(), size);
        assert!(size > QUIC_MIN_PLPMTU);
    }

    #[test]
    fn probe_retried_up_to_max_probes() {
        let mut d = PathMtuDiscovery::with_confirmed_base(1500);
        let size = d.next_probe().expect("probe");
        d.on_probe_sent(size);
        // Loss #1 and #2 keep the same candidate.
        d.on_probe_lost(size);
        let retry1 = d.next_probe().expect("retry 1");
        assert_eq!(retry1, size);
        d.on_probe_sent(retry1);
        d.on_probe_lost(retry1);
        let retry2 = d.next_probe().expect("retry 2");
        assert_eq!(retry2, size);
        d.on_probe_sent(retry2);
        // Third loss exhausts the budget: the candidate is dropped, and the next
        // probe is a smaller midpoint.
        d.on_probe_lost(retry2);
        let smaller = d.next_probe().expect("smaller probe");
        assert!(smaller < size);
    }

    #[test]
    fn converges_to_true_path_mtu() {
        // Real path ceiling 1400 between base 1200 and max 1500.
        let d = run_to_completion(1500, 1400);
        assert_eq!(d.state(), PmtuState::SearchComplete);
        assert!(d.is_complete());
        // The confirmed size must not exceed the true path MTU and must be within
        // one byte of it (binary search converges to the boundary).
        assert!(d.max_datagram_size() <= 1400);
        assert_eq!(d.max_datagram_size(), 1400);
    }

    #[test]
    fn converges_when_full_range_supported() {
        let d = run_to_completion(1500, 1500);
        assert_eq!(d.state(), PmtuState::SearchComplete);
        assert_eq!(d.max_datagram_size(), 1500);
    }

    #[test]
    fn converges_when_only_base_supported() {
        // Path can carry nothing above the base.
        let d = run_to_completion(1500, QUIC_MIN_PLPMTU);
        assert_eq!(d.state(), PmtuState::SearchComplete);
        assert_eq!(d.max_datagram_size(), QUIC_MIN_PLPMTU);
    }

    #[test]
    fn confirmed_size_never_exceeds_max() {
        let d = run_to_completion(1400, 9000);
        assert_eq!(d.max_datagram_size(), 1400);
        assert_eq!(d.state(), PmtuState::SearchComplete);
    }

    #[test]
    fn black_hole_resets_to_base_and_restarts() {
        let mut d = run_to_completion(1500, 1500);
        assert_eq!(d.max_datagram_size(), 1500);
        d.on_black_hole();
        assert_eq!(d.max_datagram_size(), QUIC_MIN_PLPMTU);
        assert_eq!(d.state(), PmtuState::Searching);
        // The search restarts and can re-converge.
        let again = {
            for _ in 0..64 {
                let Some(size) = d.next_probe() else { break };
                d.on_probe_sent(size);
                d.on_probe_acked(size);
            }
            d.max_datagram_size()
        };
        assert_eq!(again, 1500);
    }

    #[test]
    fn stale_ack_and_loss_are_ignored() {
        let mut d = PathMtuDiscovery::with_confirmed_base(1500);
        let size = d.next_probe().expect("probe");
        d.on_probe_sent(size);
        // Reports for a different size do nothing.
        d.on_probe_acked(size + 7);
        assert!(d.max_datagram_size() < size);
        d.on_probe_lost(size + 7);
        // The real probe is still in flight.
        assert_eq!(d.next_probe(), None);
        d.on_probe_acked(size);
        assert_eq!(d.max_datagram_size(), size);
    }

    #[test]
    fn no_probe_after_completion() {
        let mut d = run_to_completion(1500, 1300);
        assert!(d.is_complete());
        assert_eq!(d.next_probe(), None);
        // Idempotent completion.
        assert_eq!(d.next_probe(), None);
    }
}
