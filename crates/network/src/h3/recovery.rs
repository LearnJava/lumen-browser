//! QUIC RTT estimation + NewReno congestion control — RFC 9002 §5, §7.
//!
//! Slices 1–6 built the pure wire codecs (QUIC varints, transport and HTTP/3
//! frames, packet headers, QPACK). This slice adds the first piece of the
//! *connection layer's* control logic that does not touch the wire at all: the
//! **round-trip-time estimator** (RFC 9002 §5) and the **NewReno congestion
//! controller** (RFC 9002 §7, pseudocode in Appendix B). Both are pure state
//! machines — no IO, no packet protection, no sent-packet registry. The caller
//! (a later slice's loss-recovery layer, RFC 9002 §6) drives them by reporting
//! acknowledged and lost packets; they hand back the smoothed RTT / PTO and the
//! congestion window that gate how much unacknowledged data may be in flight.
//!
//! ## RTT estimator (RFC 9002 §5)
//!
//! [`RttEstimator`] tracks `latest_rtt`, `min_rtt`, `smoothed_rtt`, and
//! `rttvar`. Before the first sample it is seeded with the initial RTT
//! ([`K_INITIAL_RTT`], RFC 9002 §6.2.2). Each RTT sample is corrected for the
//! peer's reported `ack_delay` (capped at the peer's `max_ack_delay`) and folded
//! into the smoothed estimate with the §5.3 EWMA weights (7/8 and 3/4). The
//! probe timeout ([`RttEstimator::pto`], RFC 9002 §6.2.1) derives from the
//! smoothed RTT, four times the variance, and the max ack delay.
//!
//! ## Congestion controller (RFC 9002 §7)
//!
//! [`CongestionController`] is a straight NewReno implementation over the three
//! phases — slow start, congestion avoidance, and recovery — keyed to a
//! `max_datagram_size` (RFC 9002 §7.2). It exposes the primitives the loss
//! detector calls: [`CongestionController::on_packet_sent`],
//! [`CongestionController::on_packet_acked`],
//! [`CongestionController::on_packets_lost`], and
//! [`CongestionController::on_persistent_congestion`] (RFC 9002 §7.6). The
//! window shrinks by [`K_LOSS_REDUCTION_FACTOR`] on the first loss in an RTT and
//! collapses to the minimum window under persistent congestion.
//!
//! ## Out of scope (later slices)
//!
//! - The sent-packet registry, ack processing, and loss detection proper
//!   (RFC 9002 §6): deciding *which* packets are newly acked or lost. This
//!   module only reacts to that decision.
//! - Pacing (RFC 9002 §7.7), ECN (RFC 9002 §7.5 beyond the congestion event),
//!   and the packet-number-space bookkeeping that feeds loss detection.
//! - Any IO, header protection, AEAD, or TLS.

use std::time::{Duration, Instant};

// ── RTT constants (RFC 9002 §6.2, Appendix A.2) ─────────────────────────────

/// The default initial RTT used before any RTT sample is available
/// (RFC 9002 §6.2.2 recommends 333 ms).
pub const K_INITIAL_RTT: Duration = Duration::from_millis(333);

/// Timer granularity; a floor on the RTT-variance term of the PTO so it never
/// collapses to zero (RFC 9002 §6.2.1, Appendix A.2 recommends 1 ms).
pub const K_GRANULARITY: Duration = Duration::from_millis(1);

// ── Congestion-control constants (RFC 9002 §7.6.1, Appendix B.1) ────────────

/// Reduction applied to the congestion window on a congestion event, expressed
/// as numerator/denominator (RFC 9002 §7.3.2 defines the factor as 0.5).
const K_LOSS_REDUCTION_NUM: usize = 1;
/// Denominator of [`K_LOSS_REDUCTION_NUM`].
const K_LOSS_REDUCTION_DEN: usize = 2;

/// The congestion window is halved on the first loss of a congestion period
/// (RFC 9002 §7.3.2). Exposed as a rational for documentation; the arithmetic
/// uses [`K_LOSS_REDUCTION_NUM`] / [`K_LOSS_REDUCTION_DEN`] to stay integer.
pub const K_LOSS_REDUCTION_FACTOR: f64 = 0.5;

/// Number of PTOs that must elapse without an acknowledgement before a run of
/// lost packets is declared persistent congestion (RFC 9002 §7.6.1).
pub const K_PERSISTENT_CONGESTION_THRESHOLD: u32 = 3;

/// Converts a nanosecond count wider than [`u64`] into a [`Duration`] without
/// overflow, used by the EWMA arithmetic that works in `u128` nanoseconds.
fn duration_from_nanos_u128(nanos: u128) -> Duration {
    let secs = (nanos / 1_000_000_000) as u64;
    let sub = (nanos % 1_000_000_000) as u32;
    Duration::new(secs, sub)
}

/// The round-trip-time estimator of RFC 9002 §5.
///
/// Fed one RTT sample per newly acknowledged, ack-eliciting packet via
/// [`RttEstimator::update_rtt`]; produces the smoothed RTT and the probe timeout
/// consumed by loss detection.
#[derive(Debug, Clone)]
pub struct RttEstimator {
    /// The most recent RTT sample (RFC 9002 §5.1).
    latest_rtt: Duration,
    /// The minimum RTT observed over the connection (RFC 9002 §5.2).
    min_rtt: Duration,
    /// The exponentially weighted moving average of the RTT (RFC 9002 §5.3).
    smoothed_rtt: Duration,
    /// The mean deviation of the RTT samples (RFC 9002 §5.3).
    rttvar: Duration,
    /// Whether at least one RTT sample has been folded in; distinguishes the
    /// seeded initial estimate from a measured one.
    has_sample: bool,
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl RttEstimator {
    /// Creates an estimator seeded with [`K_INITIAL_RTT`] (RFC 9002 §6.2.2):
    /// `smoothed_rtt = K_INITIAL_RTT`, `rttvar = K_INITIAL_RTT / 2`.
    pub fn new() -> Self {
        Self {
            latest_rtt: K_INITIAL_RTT,
            min_rtt: Duration::ZERO,
            smoothed_rtt: K_INITIAL_RTT,
            rttvar: K_INITIAL_RTT / 2,
            has_sample: false,
        }
    }

    /// The most recent RTT sample (RFC 9002 §5.1).
    pub fn latest_rtt(&self) -> Duration {
        self.latest_rtt
    }

    /// The minimum RTT seen so far (RFC 9002 §5.2); [`Duration::ZERO`] before
    /// the first sample.
    pub fn min_rtt(&self) -> Duration {
        self.min_rtt
    }

    /// The smoothed RTT estimate (RFC 9002 §5.3).
    pub fn smoothed_rtt(&self) -> Duration {
        self.smoothed_rtt
    }

    /// The RTT variance estimate (RFC 9002 §5.3).
    pub fn rttvar(&self) -> Duration {
        self.rttvar
    }

    /// Whether a measured RTT sample has been folded in (as opposed to the
    /// seeded initial estimate).
    pub fn has_sample(&self) -> bool {
        self.has_sample
    }

    /// Folds a new RTT sample into the estimate (RFC 9002 §5.3).
    ///
    /// `latest_rtt` is the raw time between sending the acknowledged packet and
    /// receiving its acknowledgement. `ack_delay` is the delay the peer reported
    /// in the ACK frame; it is capped at `max_ack_delay` and subtracted only
    /// when doing so keeps the adjusted sample at or above `min_rtt`.
    pub fn update_rtt(&mut self, latest_rtt: Duration, ack_delay: Duration, max_ack_delay: Duration) {
        self.latest_rtt = latest_rtt;

        // First sample: seed directly (RFC 9002 §5.2, §5.3).
        if !self.has_sample {
            self.min_rtt = latest_rtt;
            self.smoothed_rtt = latest_rtt;
            self.rttvar = latest_rtt / 2;
            self.has_sample = true;
            return;
        }

        // min_rtt ignores ack delay entirely (RFC 9002 §5.2).
        self.min_rtt = self.min_rtt.min(latest_rtt);

        // Adjust for ack delay, but never below min_rtt (RFC 9002 §5.3).
        let ack_delay = ack_delay.min(max_ack_delay);
        let mut adjusted_rtt = latest_rtt;
        if latest_rtt >= self.min_rtt + ack_delay {
            adjusted_rtt = latest_rtt - ack_delay;
        }

        // smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt.
        let s = self.smoothed_rtt.as_nanos();
        let a = adjusted_rtt.as_nanos();
        let smoothed = (7 * s + a) / 8;

        // rttvar_sample = abs(smoothed_rtt - adjusted_rtt), using the *new*
        // smoothed value (RFC 9002 §5.3 assigns smoothed_rtt first).
        let rttvar_sample = smoothed.abs_diff(a);

        // rttvar = 3/4 * rttvar + 1/4 * rttvar_sample.
        let v = self.rttvar.as_nanos();
        let rttvar = (3 * v + rttvar_sample) / 4;

        self.smoothed_rtt = duration_from_nanos_u128(smoothed);
        self.rttvar = duration_from_nanos_u128(rttvar);
    }

    /// The probe timeout (RFC 9002 §6.2.1):
    /// `smoothed_rtt + max(4 * rttvar, K_GRANULARITY) + max_ack_delay`.
    ///
    /// `max_ack_delay` is the peer's advertised maximum ack delay; pass
    /// [`Duration::ZERO`] for Initial and Handshake packet number spaces, where
    /// the peer cannot delay acknowledgements (RFC 9002 §6.2.1).
    pub fn pto(&self, max_ack_delay: Duration) -> Duration {
        self.smoothed_rtt + (self.rttvar * 4).max(K_GRANULARITY) + max_ack_delay
    }
}

/// A packet reported lost to [`CongestionController::on_packets_lost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LostPacket {
    /// When the packet was originally sent; used to decide whether the loss
    /// falls inside the current recovery period (RFC 9002 Appendix B.2).
    pub time_sent: Instant,
    /// The number of bytes the packet contributed to `bytes_in_flight`.
    pub size: usize,
}

/// The NewReno congestion controller of RFC 9002 §7 (pseudocode Appendix B).
///
/// Tracks the congestion window, bytes in flight, slow-start threshold, and the
/// start of the current recovery period. All windows are byte counts scaled to
/// `max_datagram_size`.
#[derive(Debug, Clone)]
pub struct CongestionController {
    /// The maximum size of a UDP datagram's payload, in bytes (RFC 9002 §7.2).
    max_datagram_size: usize,
    /// The congestion window: the maximum number of in-flight bytes permitted
    /// (RFC 9002 §7).
    congestion_window: usize,
    /// The number of ack-eliciting bytes currently sent but not acknowledged.
    bytes_in_flight: usize,
    /// The slow-start threshold; [`usize::MAX`] represents the "infinite" value
    /// used before the first congestion event (RFC 9002 Appendix B.2).
    ssthresh: usize,
    /// The time the current recovery period began, or `None` when not in
    /// recovery (RFC 9002 Appendix B.2 `congestion_recovery_start_time`).
    recovery_start_time: Option<Instant>,
}

impl CongestionController {
    /// Creates a controller for the given `max_datagram_size` (bytes), with the
    /// initial window of RFC 9002 §7.2 and an infinite slow-start threshold.
    ///
    /// `max_datagram_size` is clamped to at least 1 to keep the window
    /// arithmetic well-defined.
    pub fn new(max_datagram_size: usize) -> Self {
        let mds = max_datagram_size.max(1);
        Self {
            max_datagram_size: mds,
            congestion_window: Self::initial_window(mds),
            bytes_in_flight: 0,
            ssthresh: usize::MAX,
            recovery_start_time: None,
        }
    }

    /// The initial congestion window (RFC 9002 §7.2):
    /// `min(10 * mds, max(2 * mds, 14720))`.
    fn initial_window(mds: usize) -> usize {
        (10 * mds).min((2 * mds).max(14_720))
    }

    /// The minimum congestion window (RFC 9002 §7.2): `2 * max_datagram_size`.
    pub fn minimum_window(&self) -> usize {
        2 * self.max_datagram_size
    }

    /// The current congestion window in bytes (RFC 9002 §7).
    pub fn congestion_window(&self) -> usize {
        self.congestion_window
    }

    /// The bytes currently in flight (sent but unacknowledged).
    pub fn bytes_in_flight(&self) -> usize {
        self.bytes_in_flight
    }

    /// The slow-start threshold, or `None` while it is still infinite (before
    /// the first congestion event, RFC 9002 Appendix B.2).
    pub fn ssthresh(&self) -> Option<usize> {
        (self.ssthresh != usize::MAX).then_some(self.ssthresh)
    }

    /// Whether the controller is in slow start (`congestion_window < ssthresh`,
    /// RFC 9002 §7.3.1).
    pub fn in_slow_start(&self) -> bool {
        self.congestion_window < self.ssthresh
    }

    /// The number of additional bytes that may be sent right now
    /// (`congestion_window - bytes_in_flight`, saturating at 0).
    pub fn available_window(&self) -> usize {
        self.congestion_window.saturating_sub(self.bytes_in_flight)
    }

    /// Whether at least one more datagram may be sent under the current window
    /// (RFC 9002 §7): `bytes_in_flight < congestion_window`.
    pub fn can_send(&self) -> bool {
        self.bytes_in_flight < self.congestion_window
    }

    /// Records that `sent_bytes` of ack-eliciting, in-flight data left the host
    /// (RFC 9002 Appendix B.4).
    pub fn on_packet_sent(&mut self, sent_bytes: usize) {
        self.bytes_in_flight += sent_bytes;
    }

    /// Whether `sent_time` falls within the current recovery period
    /// (RFC 9002 Appendix B.2 `InCongestionRecovery`).
    fn in_congestion_recovery(&self, sent_time: Instant) -> bool {
        match self.recovery_start_time {
            Some(start) => sent_time <= start,
            None => false,
        }
    }

    /// Processes one newly acknowledged packet (RFC 9002 Appendix B.5).
    ///
    /// Removes `acked_bytes` from `bytes_in_flight`, then grows the window —
    /// unless the packet was sent during the current recovery period, in which
    /// case the window is left unchanged (RFC 9002 §7.3.2). In slow start the
    /// window grows by the acknowledged bytes; in congestion avoidance it grows
    /// by roughly one datagram per window of data acknowledged.
    pub fn on_packet_acked(&mut self, sent_time: Instant, acked_bytes: usize) {
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(acked_bytes);

        // A packet acked from before recovery started does not grow the window.
        if self.in_congestion_recovery(sent_time) {
            return;
        }

        if self.in_slow_start() {
            // Slow start (RFC 9002 §7.3.1).
            self.congestion_window += acked_bytes;
        } else {
            // Congestion avoidance (RFC 9002 §7.3.3).
            self.congestion_window += self.max_datagram_size * acked_bytes / self.congestion_window;
        }
    }

    /// Enters (or stays in) a recovery period in response to a congestion signal
    /// for a packet sent at `sent_time` (RFC 9002 Appendix B.6
    /// `OnCongestionEvent`).
    ///
    /// The window is halved on the first event of a recovery period; subsequent
    /// events for packets already inside the period are ignored so the window is
    /// reduced at most once per round trip.
    pub fn on_congestion_event(&mut self, sent_time: Instant, now: Instant) {
        // Only reduce the window once per recovery period.
        if self.in_congestion_recovery(sent_time) {
            return;
        }
        self.recovery_start_time = Some(now);
        self.ssthresh = self.congestion_window * K_LOSS_REDUCTION_NUM / K_LOSS_REDUCTION_DEN;
        self.congestion_window = self.ssthresh.max(self.minimum_window());
    }

    /// Processes a batch of lost packets (RFC 9002 Appendix B.7
    /// `OnPacketsLost`).
    ///
    /// Removes their bytes from `bytes_in_flight` and enters a recovery period
    /// keyed to the most recently sent packet in the batch. A no-op on an empty
    /// slice.
    pub fn on_packets_lost(&mut self, lost: &[LostPacket], now: Instant) {
        let Some(first) = lost.first() else {
            return;
        };
        let mut largest_sent = first.time_sent;
        for packet in lost {
            self.bytes_in_flight = self.bytes_in_flight.saturating_sub(packet.size);
            if packet.time_sent > largest_sent {
                largest_sent = packet.time_sent;
            }
        }
        self.on_congestion_event(largest_sent, now);
    }

    /// Collapses the window to the minimum on established persistent congestion
    /// (RFC 9002 §7.6, Appendix B.8).
    ///
    /// Call after [`persistent_congestion_duration`] confirms that a run of lost
    /// packets spans more than the persistent-congestion period. Resets the
    /// recovery period so the next ack resumes slow start from the minimum
    /// window.
    pub fn on_persistent_congestion(&mut self) {
        self.congestion_window = self.minimum_window();
        self.recovery_start_time = None;
    }
}

/// The persistent-congestion period (RFC 9002 §7.6.1):
/// `pto(max_ack_delay) * K_PERSISTENT_CONGESTION_THRESHOLD`, computed from the
/// smoothed RTT rather than the exponentially backed-off PTO.
///
/// A run of lost packets whose earliest and latest sends are further apart than
/// this establishes persistent congestion; the caller then invokes
/// [`CongestionController::on_persistent_congestion`].
pub fn persistent_congestion_duration(rtt: &RttEstimator, max_ack_delay: Duration) -> Duration {
    rtt.pto(max_ack_delay) * K_PERSISTENT_CONGESTION_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RTT estimator ───────────────────────────────────────────────────────

    /// A fresh estimator is seeded with the initial RTT (RFC 9002 §6.2.2).
    #[test]
    fn rtt_seeded_with_initial() {
        let rtt = RttEstimator::new();
        assert!(!rtt.has_sample());
        assert_eq!(rtt.smoothed_rtt(), K_INITIAL_RTT);
        assert_eq!(rtt.rttvar(), K_INITIAL_RTT / 2);
        assert_eq!(rtt.min_rtt(), Duration::ZERO);
    }

    /// The first sample seeds smoothed_rtt and rttvar directly (RFC 9002 §5.3).
    #[test]
    fn rtt_first_sample_seeds_directly() {
        let mut rtt = RttEstimator::new();
        rtt.update_rtt(Duration::from_millis(100), Duration::from_millis(20), Duration::from_millis(25));
        assert!(rtt.has_sample());
        assert_eq!(rtt.latest_rtt(), Duration::from_millis(100));
        assert_eq!(rtt.min_rtt(), Duration::from_millis(100));
        assert_eq!(rtt.smoothed_rtt(), Duration::from_millis(100));
        assert_eq!(rtt.rttvar(), Duration::from_millis(50));
    }

    /// The EWMA weights the smoothed RTT 7/8 toward the old value (RFC 9002
    /// §5.3): after a 100 ms seed and a 200 ms sample (no ack delay),
    /// smoothed = 7/8*100 + 1/8*200 = 112.5 ms.
    #[test]
    fn rtt_ewma_weights() {
        let mut rtt = RttEstimator::new();
        rtt.update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::ZERO);
        rtt.update_rtt(Duration::from_millis(200), Duration::ZERO, Duration::ZERO);
        assert_eq!(rtt.smoothed_rtt(), Duration::from_micros(112_500));
        // rttvar = 3/4*50 + 1/4*|112.5 - 200| = 37.5 + 21.875 = 59.375 ms.
        assert_eq!(rtt.rttvar(), Duration::from_nanos(59_375_000));
        // min_rtt stays at the earlier, smaller sample.
        assert_eq!(rtt.min_rtt(), Duration::from_millis(100));
    }

    /// Ack delay is subtracted from the sample when doing so stays at or above
    /// min_rtt (RFC 9002 §5.3).
    #[test]
    fn rtt_ack_delay_subtracted() {
        let mut rtt = RttEstimator::new();
        rtt.update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::from_millis(25));
        // Sample 150 ms with 25 ms ack delay: adjusted = 125 ms (>= min 100).
        rtt.update_rtt(Duration::from_millis(150), Duration::from_millis(25), Duration::from_millis(25));
        // smoothed = 7/8*100 + 1/8*125 = 103.125 ms.
        assert_eq!(rtt.smoothed_rtt(), Duration::from_nanos(103_125_000));
    }

    /// Ack delay is capped at max_ack_delay before subtraction (RFC 9002 §5.3).
    #[test]
    fn rtt_ack_delay_capped_at_max() {
        let mut rtt = RttEstimator::new();
        rtt.update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::from_millis(10));
        // Reported ack delay 40 ms but max is 10 ms → subtract only 10 ms.
        rtt.update_rtt(Duration::from_millis(150), Duration::from_millis(40), Duration::from_millis(10));
        // adjusted = 150 - 10 = 140; smoothed = 7/8*100 + 1/8*140 = 105 ms.
        assert_eq!(rtt.smoothed_rtt(), Duration::from_millis(105));
    }

    /// Ack delay is *not* subtracted when it would push the sample below min_rtt
    /// (RFC 9002 §5.3).
    #[test]
    fn rtt_ack_delay_not_below_min() {
        let mut rtt = RttEstimator::new();
        rtt.update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::from_millis(50));
        // Sample equals min_rtt; subtracting 50 ms would drop below min → keep 100.
        rtt.update_rtt(Duration::from_millis(100), Duration::from_millis(50), Duration::from_millis(50));
        // adjusted = 100; smoothed = 7/8*100 + 1/8*100 = 100 ms.
        assert_eq!(rtt.smoothed_rtt(), Duration::from_millis(100));
    }

    /// min_rtt tracks the smallest raw sample, ignoring ack delay (RFC 9002
    /// §5.2).
    #[test]
    fn rtt_min_tracks_smallest() {
        let mut rtt = RttEstimator::new();
        rtt.update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::ZERO);
        rtt.update_rtt(Duration::from_millis(60), Duration::ZERO, Duration::ZERO);
        rtt.update_rtt(Duration::from_millis(80), Duration::ZERO, Duration::ZERO);
        assert_eq!(rtt.min_rtt(), Duration::from_millis(60));
    }

    /// PTO before any sample uses the seeded estimate (RFC 9002 §6.2.1):
    /// 333 + max(4*166.5, 1) + max_ack_delay.
    #[test]
    fn pto_from_initial() {
        let rtt = RttEstimator::new();
        // 333 + 666 + 25 = 1024 ms.
        assert_eq!(rtt.pto(Duration::from_millis(25)), Duration::from_millis(1024));
    }

    /// The PTO variance term is floored at K_GRANULARITY (RFC 9002 §6.2.1).
    #[test]
    fn pto_variance_floored_at_granularity() {
        let mut rtt = RttEstimator::new();
        // A steady stream of identical samples drives rttvar toward zero.
        for _ in 0..40 {
            rtt.update_rtt(Duration::from_millis(50), Duration::ZERO, Duration::ZERO);
        }
        assert!(rtt.rttvar() < K_GRANULARITY / 4);
        // pto = smoothed(~50ms) + max(4*rttvar, 1ms) + 0 = smoothed + 1ms.
        assert_eq!(rtt.pto(Duration::ZERO), rtt.smoothed_rtt() + K_GRANULARITY);
    }

    // ── Congestion controller: construction ─────────────────────────────────

    /// The initial window is min(10*mds, max(2*mds, 14720)) (RFC 9002 §7.2).
    #[test]
    fn cc_initial_window() {
        // mds = 1200 → min(12000, max(2400, 14720)) = 12000.
        assert_eq!(CongestionController::new(1200).congestion_window(), 12_000);
        // mds = 1500 → min(15000, max(3000, 14720)) = 14720.
        assert_eq!(CongestionController::new(1500).congestion_window(), 14_720);
        // A tiny mds is floored by the 14720 clamp inside the max().
        assert_eq!(CongestionController::new(100).congestion_window(), 1_000);
    }

    /// A fresh controller has an infinite ssthresh and is in slow start.
    #[test]
    fn cc_initial_state() {
        let cc = CongestionController::new(1200);
        assert_eq!(cc.ssthresh(), None);
        assert!(cc.in_slow_start());
        assert_eq!(cc.bytes_in_flight(), 0);
        assert_eq!(cc.minimum_window(), 2400);
        assert!(cc.can_send());
    }

    // ── Congestion controller: flight accounting ────────────────────────────

    /// Sending and acking move bytes in and out of flight.
    #[test]
    fn cc_bytes_in_flight_accounting() {
        let mut cc = CongestionController::new(1200);
        let t0 = Instant::now();
        cc.on_packet_sent(1200);
        cc.on_packet_sent(1200);
        assert_eq!(cc.bytes_in_flight(), 2400);
        cc.on_packet_acked(t0, 1200);
        assert_eq!(cc.bytes_in_flight(), 1200);
    }

    /// can_send / available_window reflect the window minus flight.
    #[test]
    fn cc_available_window() {
        let mut cc = CongestionController::new(1200); // window 12000
        cc.on_packet_sent(12_000);
        assert_eq!(cc.available_window(), 0);
        assert!(!cc.can_send());
        // Over-committing saturates rather than underflowing.
        cc.on_packet_sent(1200);
        assert_eq!(cc.available_window(), 0);
    }

    // ── Congestion controller: slow start & avoidance ───────────────────────

    /// In slow start the window grows by the acknowledged bytes (RFC 9002
    /// §7.3.1).
    #[test]
    fn cc_slow_start_growth() {
        let mut cc = CongestionController::new(1200);
        let start = cc.congestion_window();
        let t0 = Instant::now();
        cc.on_packet_sent(1200);
        cc.on_packet_acked(t0, 1200);
        assert_eq!(cc.congestion_window(), start + 1200);
    }

    /// Once cwnd reaches ssthresh the controller switches to the additive
    /// congestion-avoidance increase (RFC 9002 §7.3.3).
    #[test]
    fn cc_congestion_avoidance_growth() {
        let mut cc = CongestionController::new(1200);
        let t0 = Instant::now();
        let later = t0 + Duration::from_millis(100);
        // Trigger a loss to set a finite ssthresh and leave slow start.
        cc.on_packet_sent(2400);
        cc.on_packets_lost(&[LostPacket { time_sent: t0, size: 1200 }], t0);
        let cwnd = cc.congestion_window();
        assert!(!cc.in_slow_start());
        // Ack a datagram sent after recovery started: +mds*acked/cwnd.
        cc.on_packet_acked(later, 1200);
        let expected = cwnd + 1200 * 1200 / cwnd;
        assert_eq!(cc.congestion_window(), expected);
    }

    // ── Congestion controller: recovery ─────────────────────────────────────

    /// A loss halves the window and sets ssthresh (RFC 9002 §7.3.2).
    #[test]
    fn cc_loss_halves_window() {
        let mut cc = CongestionController::new(1200); // window 12000
        let t0 = Instant::now();
        cc.on_packet_sent(6000);
        cc.on_packets_lost(&[LostPacket { time_sent: t0, size: 1200 }], t0 + Duration::from_millis(1));
        assert_eq!(cc.ssthresh(), Some(6000));
        assert_eq!(cc.congestion_window(), 6000);
        // The lost bytes leave flight.
        assert_eq!(cc.bytes_in_flight(), 4800);
    }

    /// The window never drops below the minimum on a congestion event
    /// (RFC 9002 §7.2).
    #[test]
    fn cc_loss_floored_at_minimum() {
        let mut cc = CongestionController::new(1200); // min window 2400
        let t0 = Instant::now();
        // Drive several congestion events to push cwnd toward the floor.
        for i in 0..6 {
            let sent = t0 + Duration::from_millis(i * 100);
            let now = sent + Duration::from_millis(1);
            cc.on_congestion_event(sent, now);
        }
        assert_eq!(cc.congestion_window(), cc.minimum_window());
    }

    /// The window is reduced at most once per recovery period: a second loss for
    /// a packet sent before recovery started is ignored (RFC 9002 §7.3.2).
    #[test]
    fn cc_one_reduction_per_period() {
        let mut cc = CongestionController::new(1200);
        let t0 = Instant::now();
        let recovery = t0 + Duration::from_millis(10);
        cc.on_congestion_event(t0, recovery);
        let reduced = cc.congestion_window();
        // Another loss for a packet sent before `recovery` — no further cut.
        cc.on_congestion_event(t0 - Duration::from_millis(1), recovery + Duration::from_millis(1));
        assert_eq!(cc.congestion_window(), reduced);
    }

    /// A loss for a packet sent *after* recovery started opens a new period and
    /// reduces the window again (RFC 9002 §7.3.2).
    #[test]
    fn cc_new_period_reduces_again() {
        let mut cc = CongestionController::new(1200);
        let t0 = Instant::now();
        let recovery1 = t0 + Duration::from_millis(10);
        cc.on_congestion_event(t0, recovery1);
        let after_first = cc.congestion_window();
        // A packet sent after recovery1 is a fresh loss.
        let sent2 = recovery1 + Duration::from_millis(5);
        cc.on_congestion_event(sent2, sent2 + Duration::from_millis(1));
        assert!(cc.congestion_window() < after_first || cc.congestion_window() == cc.minimum_window());
    }

    /// Acking a packet sent during recovery does not grow the window
    /// (RFC 9002 §7.3.2).
    #[test]
    fn cc_ack_in_recovery_no_growth() {
        let mut cc = CongestionController::new(1200);
        let t0 = Instant::now();
        let recovery = t0 + Duration::from_millis(10);
        cc.on_packet_sent(3600);
        cc.on_congestion_event(t0, recovery);
        let cwnd = cc.congestion_window();
        // Packet sent at t0 (<= recovery_start) → no growth, only flight removal.
        cc.on_packet_acked(t0, 1200);
        assert_eq!(cc.congestion_window(), cwnd);
        assert_eq!(cc.bytes_in_flight(), 2400);
    }

    // ── Congestion controller: batch loss & persistent congestion ───────────

    /// on_packets_lost removes every packet's bytes and keys recovery to the
    /// most recent send (RFC 9002 Appendix B.7).
    #[test]
    fn cc_batch_loss() {
        let mut cc = CongestionController::new(1200);
        let t0 = Instant::now();
        cc.on_packet_sent(3600);
        let lost = [
            LostPacket { time_sent: t0, size: 1200 },
            LostPacket { time_sent: t0 + Duration::from_millis(5), size: 1200 },
        ];
        cc.on_packets_lost(&lost, t0 + Duration::from_millis(6));
        assert_eq!(cc.bytes_in_flight(), 1200);
        // Recovery keyed to the later send: a packet at t0 is now "in recovery".
        assert!(cc.congestion_window() < 12_000);
    }

    /// An empty loss batch is a no-op.
    #[test]
    fn cc_empty_loss_noop() {
        let mut cc = CongestionController::new(1200);
        let before = cc.congestion_window();
        cc.on_packets_lost(&[], Instant::now());
        assert_eq!(cc.congestion_window(), before);
        assert_eq!(cc.ssthresh(), None);
    }

    /// Persistent congestion collapses the window to the minimum and clears the
    /// recovery period (RFC 9002 §7.6, Appendix B.8).
    #[test]
    fn cc_persistent_congestion_collapse() {
        let mut cc = CongestionController::new(1200);
        let t0 = Instant::now();
        cc.on_congestion_event(t0, t0 + Duration::from_millis(1));
        cc.on_persistent_congestion();
        assert_eq!(cc.congestion_window(), cc.minimum_window());
        // Recovery cleared: the next ack resumes growth.
        cc.on_packet_acked(t0 + Duration::from_secs(10), 1200);
        assert_eq!(cc.congestion_window(), cc.minimum_window() + 1200);
    }

    /// The persistent-congestion period is 3 * PTO from the smoothed RTT
    /// (RFC 9002 §7.6.1).
    #[test]
    fn persistent_congestion_period() {
        let mut rtt = RttEstimator::new();
        rtt.update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::ZERO);
        // pto = 100 + max(4*50, 1) + 25 = 100 + 200 + 25 = 325 ms.
        let pto = rtt.pto(Duration::from_millis(25));
        assert_eq!(pto, Duration::from_millis(325));
        assert_eq!(
            persistent_congestion_duration(&rtt, Duration::from_millis(25)),
            pto * 3,
        );
    }
}
