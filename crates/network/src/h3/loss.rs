//! QUIC sent-packet registry + loss detection — RFC 9002 §6.
//!
//! Slice 7 ([`super::recovery`]) built the RTT estimator and the NewReno
//! congestion controller, but left the question of *which* in-flight packets are
//! newly acknowledged or lost to "a later slice". This is that slice: the
//! per-packet-number-space **sent-packet registry** and the **loss-detection**
//! logic (RFC 9002 §6) that decides, from an incoming ACK frame and the current
//! time, which sent packets became acknowledged and which are declared lost. The
//! caller then feeds those decisions into [`super::recovery::RttEstimator`] and
//! [`super::recovery::CongestionController`].
//!
//! Like every other slice this is a pure state machine — no IO, no packet
//! protection, no timers. It records the packets the send path emits, consumes
//! the *decoded* fields of an ACK frame ([`super::quic_frame::Frame::Ack`]), and
//! hands back owned [`SentPacket`] values. It does not itself read a clock; the
//! caller passes `now` so the logic stays deterministic and testable.
//!
//! ## Sent-packet registry (RFC 9002 §A.1)
//!
//! [`SentPacketRegistry`] is one packet-number space's view of the packets sent
//! but not yet acknowledged or declared lost. Each [`SentPacket`] records its
//! packet number, send time, whether it is ack-eliciting and counts against the
//! congestion window (`in_flight`), and its byte size. QUIC maintains three
//! independent spaces (Initial, Handshake, Application Data, RFC 9000 §12.3);
//! the caller keeps one registry per [`PacketNumberSpace`].
//!
//! ## Acknowledgement processing (RFC 9002 §A.7)
//!
//! [`SentPacketRegistry::on_ack_received`] takes the ACK frame's acknowledged
//! packet-number ranges, removes every matching packet from the registry, and
//! returns them in an [`AckOutcome`] together with an RTT sample — but only when
//! the largest acknowledged packet is newly acked and at least one newly-acked
//! packet was ack-eliciting (RFC 9002 §5.1). The caller applies `ack_delay` and
//! folds the sample into the estimator.
//!
//! ## Loss detection (RFC 9002 §6.1)
//!
//! [`SentPacketRegistry::detect_and_remove_lost_packets`] declares a sent packet
//! lost by either the **packet threshold** (a packet at least
//! [`K_PACKET_THRESHOLD`] below the largest acknowledged is lost, RFC 9002
//! §6.1.1) or the **time threshold** (a packet older than
//! `kTimeThreshold * max(latest_rtt, smoothed_rtt)` is lost, RFC 9002 §6.1.2).
//! Packets not yet lost but older than nothing set the earliest
//! [`SentPacketRegistry::loss_time`], which the caller arms a timer against.
//!
//! ## Persistent congestion (RFC 9002 §7.6)
//!
//! [`establishes_persistent_congestion`] applies the §7.6.2 test to a run of
//! lost packets: two or more ack-eliciting losses whose send times span more
//! than the persistent-congestion period (from
//! [`super::recovery::persistent_congestion_duration`]) collapse the window via
//! [`super::recovery::CongestionController::on_persistent_congestion`].
//!
//! ## Out of scope (later slices)
//!
//! - The probe-timeout timer and PTO-driven probe sending (RFC 9002 §6.2): this
//!   module supplies [`SentPacketRegistry::time_of_last_ack_eliciting_packet`]
//!   and [`SentPacketRegistry::ack_eliciting_in_flight`], the inputs the PTO
//!   timer needs, but does not run the timer.
//! - Any IO, header protection, AEAD, TLS, or datagram assembly.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::recovery::K_GRANULARITY;

// ── Loss-detection constants (RFC 9002 §6.1.1, §6.1.2) ──────────────────────

/// The packet-reordering threshold (RFC 9002 §6.1.1): a packet is declared lost
/// once a packet numbered at least this much higher has been acknowledged. The
/// RFC recommends the value 3.
pub const K_PACKET_THRESHOLD: u64 = 3;

/// Numerator of the time threshold (RFC 9002 §6.1.2): a packet is declared lost
/// once it is older than `9/8 * max(latest_rtt, smoothed_rtt)`.
const K_TIME_THRESHOLD_NUM: u32 = 9;
/// Denominator of [`K_TIME_THRESHOLD_NUM`].
const K_TIME_THRESHOLD_DEN: u32 = 8;

/// The RFC 9002 §6.1.2 time threshold, `9/8`, exposed as an `f64` for
/// documentation. The arithmetic itself scales a [`Duration`] by
/// [`K_TIME_THRESHOLD_NUM`] / [`K_TIME_THRESHOLD_DEN`] to stay exact.
pub const K_TIME_THRESHOLD: f64 = 1.125;

/// One of QUIC's three packet-number spaces (RFC 9000 §12.3). Loss detection is
/// tracked independently per space because packet numbers restart at zero in
/// each and acknowledgements never cross spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketNumberSpace {
    /// Initial packets, protected with keys derived from the connection ID
    /// (RFC 9001 §5.2).
    Initial,
    /// Handshake packets carrying the TLS handshake after Initial.
    Handshake,
    /// Application Data (1-RTT and 0-RTT) packets carrying stream data.
    ApplicationData,
}

impl PacketNumberSpace {
    /// Whether acknowledgements in this space may carry a non-zero `ack_delay`
    /// that should be subtracted from RTT samples (RFC 9002 §5.3). Only the
    /// Application Data space delays acknowledgements; Initial and Handshake ACKs
    /// are sent immediately, so their reported delay must be treated as zero.
    pub fn uses_ack_delay(self) -> bool {
        matches!(self, Self::ApplicationData)
    }
}

/// A packet recorded in a [`SentPacketRegistry`] (RFC 9002 §A.1
/// `sent_packets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentPacket {
    /// The packet's number within its packet-number space (RFC 9000 §12.3).
    pub packet_number: u64,
    /// When the packet was sent; the basis for RTT samples and the time-loss
    /// threshold.
    pub time_sent: Instant,
    /// Whether the packet elicits an acknowledgement (carries a frame other than
    /// ACK, PADDING, or CONNECTION_CLOSE, RFC 9002 §2). Only ack-eliciting
    /// packets arm the PTO and yield RTT samples.
    pub ack_eliciting: bool,
    /// Whether the packet counts against the congestion window — true for any
    /// packet carrying an ack-eliciting or PADDING frame (RFC 9002 §2
    /// "in flight").
    pub in_flight: bool,
    /// The number of bytes the packet contributed to `bytes_in_flight`; fed to
    /// [`super::recovery::CongestionController`] on ack or loss.
    pub sent_bytes: usize,
}

/// The result of processing one ACK frame (RFC 9002 §A.7).
#[derive(Debug, Clone, Default)]
pub struct AckOutcome {
    /// The packets this ACK newly acknowledged, removed from the registry, in
    /// ascending packet-number order.
    pub newly_acked: Vec<SentPacket>,
    /// An RTT sample (`now - time_sent` of the largest acknowledged packet),
    /// present only when the largest acknowledged packet was newly acked *and*
    /// at least one newly-acked packet was ack-eliciting (RFC 9002 §5.1). The
    /// caller subtracts the frame's `ack_delay` before folding it in.
    pub rtt_sample: Option<Duration>,
    /// The largest packet number this ACK newly acknowledged, if any.
    pub largest_newly_acked: Option<u64>,
}

/// One packet-number space's sent-packet registry and loss-detection state
/// (RFC 9002 §6, §A.1).
#[derive(Debug, Clone)]
pub struct SentPacketRegistry {
    /// Which packet-number space this registry tracks.
    space: PacketNumberSpace,
    /// In-flight sent packets keyed by packet number, ordered for range scans.
    sent: BTreeMap<u64, SentPacket>,
    /// The largest packet number acknowledged in this space so far, or `None`
    /// before the first ACK (RFC 9002 §A.7 `largest_acked_packet`).
    largest_acked: Option<u64>,
    /// The earliest time a still-unacknowledged packet will cross the time-loss
    /// threshold, or `None` when no such packet is pending (RFC 9002 §A.10
    /// `loss_time`). The caller arms the loss-detection timer against it.
    loss_time: Option<Instant>,
    /// The send time of the most recent ack-eliciting packet, the PTO timer's
    /// anchor (RFC 9002 §A.6 `time_of_last_ack_eliciting_packet`).
    time_of_last_ack_eliciting_packet: Option<Instant>,
    /// Count of ack-eliciting packets currently outstanding; when zero the PTO
    /// timer is disarmed (RFC 9002 §6.2.1).
    ack_eliciting_outstanding: usize,
}

impl SentPacketRegistry {
    /// Creates an empty registry for the given packet-number space.
    pub fn new(space: PacketNumberSpace) -> Self {
        Self {
            space,
            sent: BTreeMap::new(),
            largest_acked: None,
            loss_time: None,
            time_of_last_ack_eliciting_packet: None,
            ack_eliciting_outstanding: 0,
        }
    }

    /// The packet-number space this registry tracks.
    pub fn space(&self) -> PacketNumberSpace {
        self.space
    }

    /// The largest packet number acknowledged so far, or `None` before the first
    /// ACK (RFC 9002 §A.7).
    pub fn largest_acked(&self) -> Option<u64> {
        self.largest_acked
    }

    /// The earliest time a pending packet will be declared lost by the time
    /// threshold, or `None` when none is pending (RFC 9002 §A.10 `loss_time`).
    pub fn loss_time(&self) -> Option<Instant> {
        self.loss_time
    }

    /// The send time of the most recent ack-eliciting packet, the PTO anchor
    /// (RFC 9002 §A.6). `None` when no ack-eliciting packet is outstanding.
    pub fn time_of_last_ack_eliciting_packet(&self) -> Option<Instant> {
        self.time_of_last_ack_eliciting_packet
    }

    /// Whether any ack-eliciting packet is still outstanding (RFC 9002 §6.2.1);
    /// the PTO timer is armed only while this is true.
    pub fn ack_eliciting_in_flight(&self) -> bool {
        self.ack_eliciting_outstanding > 0
    }

    /// The number of packets currently tracked (sent, not yet acked or lost).
    pub fn outstanding(&self) -> usize {
        self.sent.len()
    }

    /// Records that a packet was sent (RFC 9002 §A.5 `OnPacketSent`).
    ///
    /// Updates the PTO anchor and the ack-eliciting count for ack-eliciting
    /// packets; the caller separately reports the byte size to the congestion
    /// controller.
    pub fn on_packet_sent(&mut self, packet: SentPacket) {
        if packet.ack_eliciting {
            self.time_of_last_ack_eliciting_packet = Some(packet.time_sent);
            self.ack_eliciting_outstanding += 1;
        }
        self.sent.insert(packet.packet_number, packet);
    }

    /// Processes an ACK frame's acknowledged ranges (RFC 9002 §A.7).
    ///
    /// `largest_acked`, `first_ack_range`, and `ranges` are the decoded fields of
    /// a [`super::quic_frame::Frame::Ack`]. Every packet in the acknowledged
    /// ranges that is still tracked is removed and returned in the
    /// [`AckOutcome`], ascending by packet number. `now` is the receive time,
    /// used for the RTT sample.
    ///
    /// The RTT sample is produced only when the largest acknowledged packet was
    /// itself newly acked and at least one newly-acked packet was ack-eliciting
    /// (RFC 9002 §5.1); the caller subtracts `ack_delay`.
    pub fn on_ack_received(
        &mut self,
        largest_acked: u64,
        first_ack_range: u64,
        ranges: &[super::quic_frame::AckRange],
        now: Instant,
    ) -> AckOutcome {
        // Update the running largest-acked (never decreases; RFC 9002 §A.7).
        self.largest_acked = Some(match self.largest_acked {
            Some(prev) => prev.max(largest_acked),
            None => largest_acked,
        });

        let mut newly_acked: Vec<SentPacket> = Vec::new();
        let mut had_ack_eliciting = false;
        let mut largest_time_sent: Option<Instant> = None;

        for (hi, lo) in acked_ranges(largest_acked, first_ack_range, ranges) {
            // Collect the packet numbers to remove without holding a borrow.
            let hits: Vec<u64> = self.sent.range(lo..=hi).map(|(&pn, _)| pn).collect();
            for pn in hits {
                if let Some(packet) = self.sent.remove(&pn) {
                    if packet.ack_eliciting {
                        had_ack_eliciting = true;
                        self.ack_eliciting_outstanding =
                            self.ack_eliciting_outstanding.saturating_sub(1);
                    }
                    if pn == largest_acked {
                        largest_time_sent = Some(packet.time_sent);
                    }
                    newly_acked.push(packet);
                }
            }
        }

        newly_acked.sort_by_key(|p| p.packet_number);
        let largest_newly_acked = newly_acked.last().map(|p| p.packet_number);

        // RTT sample: only when the largest acked was newly acked and any
        // newly-acked packet was ack-eliciting (RFC 9002 §5.1).
        let rtt_sample = match (largest_time_sent, had_ack_eliciting) {
            (Some(time_sent), true) if largest_newly_acked == Some(largest_acked) => {
                Some(now.saturating_duration_since(time_sent))
            }
            _ => None,
        };

        AckOutcome { newly_acked, rtt_sample, largest_newly_acked }
    }

    /// Declares and removes lost packets (RFC 9002 §A.10
    /// `DetectAndRemoveLostPackets`).
    ///
    /// A tracked packet numbered at or below the largest acknowledged is lost if
    /// it is either at least [`K_PACKET_THRESHOLD`] below the largest
    /// acknowledged (packet threshold, RFC 9002 §6.1.1) or older than
    /// `kTimeThreshold * max(latest_rtt, smoothed_rtt)` (time threshold, RFC 9002
    /// §6.1.2). Returns the lost packets ascending by packet number and updates
    /// [`loss_time`](Self::loss_time) to the earliest send time at which a
    /// still-pending packet will cross the time threshold.
    ///
    /// A no-op returning an empty vector when no ACK has yet been received.
    pub fn detect_and_remove_lost_packets(
        &mut self,
        now: Instant,
        latest_rtt: Duration,
        smoothed_rtt: Duration,
    ) -> Vec<SentPacket> {
        self.loss_time = None;
        let Some(largest_acked) = self.largest_acked else {
            return Vec::new();
        };

        // loss_delay = kTimeThreshold * max(latest_rtt, smoothed_rtt), floored at
        // kGranularity so a near-zero RTT never declares everything lost
        // (RFC 9002 §6.1.2).
        let loss_delay =
            scale_duration(latest_rtt.max(smoothed_rtt), K_TIME_THRESHOLD_NUM, K_TIME_THRESHOLD_DEN)
                .max(K_GRANULARITY);

        let mut lost: Vec<SentPacket> = Vec::new();
        let mut earliest_loss_time: Option<Instant> = None;

        // Only packets numbered at or below the largest acked can be lost; higher
        // numbers have not been given a chance to be acknowledged yet.
        let candidates: Vec<u64> = self.sent.range(..=largest_acked).map(|(&pn, _)| pn).collect();
        for pn in candidates {
            let Some(packet) = self.sent.get(&pn) else {
                continue;
            };

            // Packet-threshold test (RFC 9002 §6.1.1).
            let by_packet_threshold = largest_acked >= pn + K_PACKET_THRESHOLD;
            // Time-threshold test (RFC 9002 §6.1.2): time_sent + loss_delay <= now.
            let loss_deadline = packet.time_sent.checked_add(loss_delay);
            let by_time_threshold = loss_deadline.is_some_and(|deadline| deadline <= now);

            if by_packet_threshold || by_time_threshold {
                if packet.ack_eliciting {
                    self.ack_eliciting_outstanding =
                        self.ack_eliciting_outstanding.saturating_sub(1);
                }
                if let Some(packet) = self.sent.remove(&pn) {
                    lost.push(packet);
                }
            } else if let Some(deadline) = loss_deadline {
                // Not yet lost: the earliest such deadline arms the loss timer.
                earliest_loss_time = Some(match earliest_loss_time {
                    Some(existing) => existing.min(deadline),
                    None => deadline,
                });
            }
        }

        self.loss_time = earliest_loss_time;
        lost
    }
}

/// Expands an ACK frame's ranges into inclusive `(high, low)` packet-number
/// spans, descending (RFC 9000 §19.3.1). The first span runs from
/// `largest_acked` down `first_ack_range` packets; each subsequent [`AckRange`]
/// skips `gap + 1` unacknowledged packets, then covers `length + 1` more.
///
/// Ranges that would underflow below zero are clamped and terminate the
/// iteration, so a malformed ACK cannot panic the loss detector.
fn acked_ranges(
    largest_acked: u64,
    first_ack_range: u64,
    ranges: &[super::quic_frame::AckRange],
) -> Vec<(u64, u64)> {
    let mut spans = Vec::with_capacity(1 + ranges.len());
    let Some(mut low) = largest_acked.checked_sub(first_ack_range) else {
        return spans;
    };
    spans.push((largest_acked, low));

    for range in ranges {
        // Next range's largest = current smallest - gap - 2 (RFC 9000 §19.3.1).
        let Some(next_high) = low.checked_sub(range.gap).and_then(|v| v.checked_sub(2)) else {
            break;
        };
        let Some(next_low) = next_high.checked_sub(range.length) else {
            break;
        };
        spans.push((next_high, next_low));
        low = next_low;
    }
    spans
}

/// Scales a [`Duration`] by the rational `num / den` without floating point,
/// working in `u128` nanoseconds to avoid overflow.
fn scale_duration(d: Duration, num: u32, den: u32) -> Duration {
    let nanos = d.as_nanos() * num as u128 / den.max(1) as u128;
    let secs = (nanos / 1_000_000_000) as u64;
    let sub = (nanos % 1_000_000_000) as u32;
    Duration::new(secs, sub)
}

/// Whether a run of lost packets establishes persistent congestion (RFC 9002
/// §7.6.2).
///
/// Persistent congestion is declared when two or more **ack-eliciting** lost
/// packets have send times spanning more than `period` — the value from
/// [`super::recovery::persistent_congestion_duration`]. The caller then collapses
/// the congestion window via
/// [`super::recovery::CongestionController::on_persistent_congestion`].
///
/// Returns `false` for fewer than two ack-eliciting losses or a span within the
/// period. This implements the practical core of §7.6.2; the full algorithm also
/// requires the losses to bracket an RTT sample, which the caller guarantees by
/// only invoking loss detection after the first RTT sample.
pub fn establishes_persistent_congestion(lost: &[SentPacket], period: Duration) -> bool {
    let mut earliest: Option<Instant> = None;
    let mut latest: Option<Instant> = None;
    let mut count = 0usize;

    for packet in lost.iter().filter(|p| p.ack_eliciting) {
        count += 1;
        earliest = Some(match earliest {
            Some(e) => e.min(packet.time_sent),
            None => packet.time_sent,
        });
        latest = Some(match latest {
            Some(l) => l.max(packet.time_sent),
            None => packet.time_sent,
        });
    }

    match (count >= 2, earliest, latest) {
        (true, Some(e), Some(l)) => l.saturating_duration_since(e) > period,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::quic_frame::AckRange;
    use super::*;

    /// A packet sent at `base + offset_ms`, ack-eliciting and in flight.
    fn pkt(pn: u64, base: Instant, offset_ms: u64) -> SentPacket {
        SentPacket {
            packet_number: pn,
            time_sent: base + Duration::from_millis(offset_ms),
            ack_eliciting: true,
            in_flight: true,
            sent_bytes: 1200,
        }
    }

    // ── acked_ranges decoding ────────────────────────────────────────────────

    /// A single-range ACK expands to one inclusive span (RFC 9000 §19.3.1).
    #[test]
    fn acked_ranges_single() {
        // largest 10, first_ack_range 4 → [6, 10].
        assert_eq!(acked_ranges(10, 4, &[]), vec![(10, 6)]);
    }

    /// Additional ranges skip gap+1 packets then cover length+1 (RFC 9000
    /// §19.3.1).
    #[test]
    fn acked_ranges_multiple() {
        // largest 20, first 2 → [18,20]; gap 1 length 3 → next_high = 18-1-2=15,
        // next_low = 15-3 = 12 → [12,15].
        let ranges = [AckRange { gap: 1, length: 3 }];
        assert_eq!(acked_ranges(20, 2, &ranges), vec![(20, 18), (15, 12)]);
    }

    /// An underflowing range terminates iteration rather than panicking.
    #[test]
    fn acked_ranges_underflow_clamped() {
        // largest 3, first_ack_range 5 would go below zero → empty.
        assert!(acked_ranges(3, 5, &[]).is_empty());
        // A valid first range but an oversized gap stops after the first span.
        let ranges = [AckRange { gap: 100, length: 0 }];
        assert_eq!(acked_ranges(5, 1, &ranges), vec![(5, 4)]);
    }

    // ── on_packet_sent / registry bookkeeping ────────────────────────────────

    /// Sending an ack-eliciting packet arms the PTO anchor and the outstanding
    /// count (RFC 9002 §A.5).
    #[test]
    fn sent_tracks_ack_eliciting() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(pkt(0, base, 0));
        reg.on_packet_sent(pkt(1, base, 5));
        assert_eq!(reg.outstanding(), 2);
        assert!(reg.ack_eliciting_in_flight());
        assert_eq!(reg.time_of_last_ack_eliciting_packet(), Some(base + Duration::from_millis(5)));
    }

    /// A non-ack-eliciting packet does not arm the PTO anchor (RFC 9002 §2).
    #[test]
    fn sent_non_ack_eliciting_no_pto_anchor() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        let mut p = pkt(0, base, 0);
        p.ack_eliciting = false;
        reg.on_packet_sent(p);
        assert_eq!(reg.outstanding(), 1);
        assert!(!reg.ack_eliciting_in_flight());
        assert_eq!(reg.time_of_last_ack_eliciting_packet(), None);
    }

    // ── on_ack_received ──────────────────────────────────────────────────────

    /// An ACK removes the acknowledged packets and yields an RTT sample from the
    /// largest (RFC 9002 §A.7, §5.1).
    #[test]
    fn ack_removes_and_samples_rtt() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(pkt(0, base, 0));
        reg.on_packet_sent(pkt(1, base, 0));
        reg.on_packet_sent(pkt(2, base, 0));
        // ACK largest 2, first_ack_range 2 → acks 0,1,2.
        let now = base + Duration::from_millis(100);
        let out = reg.on_ack_received(2, 2, &[], now);
        assert_eq!(out.newly_acked.len(), 3);
        assert_eq!(out.largest_newly_acked, Some(2));
        assert_eq!(out.rtt_sample, Some(Duration::from_millis(100)));
        assert_eq!(reg.outstanding(), 0);
        assert!(!reg.ack_eliciting_in_flight());
        assert_eq!(reg.largest_acked(), Some(2));
    }

    /// No RTT sample when the largest acknowledged packet was already acked or
    /// not tracked (RFC 9002 §5.1).
    #[test]
    fn ack_no_sample_when_largest_not_newly_acked() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(pkt(0, base, 0));
        reg.on_packet_sent(pkt(1, base, 0));
        // First ACK takes packet 5 (never sent) as largest, acking only 0..=1
        // via a lower range would be malformed; instead ack only up to 1 but
        // claim largest 5 → largest not newly acked, so no sample.
        let now = base + Duration::from_millis(50);
        let out = reg.on_ack_received(5, 0, &[AckRange { gap: 2, length: 1 }], now);
        // largest 5 not tracked; the trailing range acks [1,... ] → packet 1.
        // next_high = 5-1(first_ack_range=0 → low=5) ... compute: low=5, gap2 →
        // next_high = 5-2-2 = 1, length1 → next_low = 0 → covers [0,1].
        assert_eq!(out.newly_acked.len(), 2);
        assert_eq!(out.rtt_sample, None);
        assert_eq!(reg.largest_acked(), Some(5));
    }

    /// No RTT sample when only non-ack-eliciting packets are acknowledged
    /// (RFC 9002 §5.1).
    #[test]
    fn ack_no_sample_without_ack_eliciting() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        let mut p = pkt(0, base, 0);
        p.ack_eliciting = false;
        reg.on_packet_sent(p);
        let out = reg.on_ack_received(0, 0, &[], base + Duration::from_millis(30));
        assert_eq!(out.newly_acked.len(), 1);
        assert_eq!(out.rtt_sample, None);
    }

    /// largest_acked never decreases across ACKs (RFC 9002 §A.7).
    #[test]
    fn ack_largest_monotonic() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        for pn in 0..5 {
            reg.on_packet_sent(pkt(pn, base, 0));
        }
        reg.on_ack_received(4, 0, &[], base + Duration::from_millis(10));
        assert_eq!(reg.largest_acked(), Some(4));
        // A later, reordered ACK for a smaller packet does not lower it.
        reg.on_ack_received(1, 0, &[], base + Duration::from_millis(20));
        assert_eq!(reg.largest_acked(), Some(4));
    }

    // ── detect_and_remove_lost_packets ───────────────────────────────────────

    /// Loss detection is a no-op before the first ACK (RFC 9002 §A.10).
    #[test]
    fn loss_none_before_ack() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(pkt(0, base, 0));
        let lost = reg.detect_and_remove_lost_packets(
            base + Duration::from_secs(10),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        assert!(lost.is_empty());
        assert_eq!(reg.outstanding(), 1);
    }

    /// The packet threshold declares packets kPacketThreshold below the largest
    /// acked lost (RFC 9002 §6.1.1).
    #[test]
    fn loss_by_packet_threshold() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        for pn in 0..5 {
            reg.on_packet_sent(pkt(pn, base, 0));
        }
        // Ack packet 4 only (first_ack_range 0). Packets 0 and 1 are >= 3 below 4.
        reg.on_ack_received(4, 0, &[], base + Duration::from_millis(1));
        // Detect almost immediately with a large RTT so only the packet
        // threshold fires (loss_delay ≈ 112.5ms far exceeds the 1ms elapsed).
        let lost = reg.detect_and_remove_lost_packets(
            base + Duration::from_millis(1),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        let lost_pns: Vec<u64> = lost.iter().map(|p| p.packet_number).collect();
        // 4 acked/removed; 0 and 1 lost (4 - 3 = 1 ≥ pn); 2,3 within threshold.
        assert_eq!(lost_pns, vec![0, 1]);
        // 2 and 3 remain, pending.
        assert_eq!(reg.outstanding(), 2);
    }

    /// The time threshold declares packets older than 9/8·max(rtt) lost
    /// (RFC 9002 §6.1.2).
    #[test]
    fn loss_by_time_threshold() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(pkt(0, base, 0)); // sent at base
        reg.on_packet_sent(pkt(1, base, 100)); // sent 100ms later
        // Ack packet 1 (the later one); packet 0 is within the packet threshold
        // (1 - 3 underflows) so only the time threshold can lose it.
        reg.on_ack_received(1, 0, &[], base + Duration::from_millis(100));
        // rtt = 80ms → loss_delay = 9/8 * 80 = 90ms. Packet 0 sent at base;
        // now = base + 100ms ≥ base + 90ms → lost.
        let now = base + Duration::from_millis(100);
        let lost = reg.detect_and_remove_lost_packets(
            now,
            Duration::from_millis(80),
            Duration::from_millis(80),
        );
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].packet_number, 0);
    }

    /// A packet not yet lost sets loss_time to its send time plus the loss delay
    /// (RFC 9002 §A.10).
    #[test]
    fn loss_time_armed_for_pending() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(pkt(0, base, 0));
        reg.on_packet_sent(pkt(1, base, 0));
        reg.on_ack_received(1, 0, &[], base + Duration::from_millis(10));
        // rtt 100ms → loss_delay = 112.5ms. now = base+10ms: packet 0 not yet
        // lost (10 < 112.5) and within packet threshold, so it arms loss_time.
        let lost = reg.detect_and_remove_lost_packets(
            base + Duration::from_millis(10),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        assert!(lost.is_empty());
        assert_eq!(reg.loss_time(), Some(base + Duration::from_micros(112_500)));
    }

    /// loss_delay is floored at kGranularity for a near-zero RTT (RFC 9002
    /// §6.1.2).
    #[test]
    fn loss_delay_floored_at_granularity() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(pkt(0, base, 0));
        reg.on_packet_sent(pkt(1, base, 0));
        reg.on_ack_received(1, 0, &[], base);
        // rtt 0 → loss_delay floored at 1ms. At now = base (0 elapsed), packet 0
        // is not lost yet but loss_time = base + 1ms.
        let lost = reg.detect_and_remove_lost_packets(base, Duration::ZERO, Duration::ZERO);
        assert!(lost.is_empty());
        assert_eq!(reg.loss_time(), Some(base + K_GRANULARITY));
    }

    /// A lost ack-eliciting packet decrements the outstanding count (RFC 9002
    /// §6.2.1).
    #[test]
    fn loss_updates_ack_eliciting_count() {
        let base = Instant::now();
        let mut reg = SentPacketRegistry::new(PacketNumberSpace::ApplicationData);
        for pn in 0..5 {
            reg.on_packet_sent(pkt(pn, base, 0));
        }
        reg.on_ack_received(4, 0, &[], base + Duration::from_millis(1));
        // 4 acked → 4 outstanding. A large RTT keeps the time threshold from
        // firing, so only the packet threshold loses 0 and 1 → 2 left.
        reg.detect_and_remove_lost_packets(
            base + Duration::from_millis(1),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        assert!(reg.ack_eliciting_in_flight());
        assert_eq!(reg.outstanding(), 2);
    }

    // ── establishes_persistent_congestion ────────────────────────────────────

    /// Two ack-eliciting losses spanning more than the period establish
    /// persistent congestion (RFC 9002 §7.6.2).
    #[test]
    fn persistent_congestion_spanning_period() {
        let base = Instant::now();
        let lost = [pkt(0, base, 0), pkt(1, base, 400)];
        // period 300ms, span 400ms > 300ms → true.
        assert!(establishes_persistent_congestion(&lost, Duration::from_millis(300)));
        // period 500ms, span 400ms → false.
        assert!(!establishes_persistent_congestion(&lost, Duration::from_millis(500)));
    }

    /// A single loss, or losses within the period, do not establish persistent
    /// congestion (RFC 9002 §7.6.2).
    #[test]
    fn persistent_congestion_insufficient() {
        let base = Instant::now();
        assert!(!establishes_persistent_congestion(&[pkt(0, base, 0)], Duration::from_millis(1)));
        assert!(!establishes_persistent_congestion(&[], Duration::from_millis(1)));
    }

    /// Non-ack-eliciting losses are ignored for persistent congestion (RFC 9002
    /// §7.6.2).
    #[test]
    fn persistent_congestion_ignores_non_ack_eliciting() {
        let base = Instant::now();
        let mut a = pkt(0, base, 0);
        let mut b = pkt(1, base, 400);
        a.ack_eliciting = false;
        b.ack_eliciting = false;
        assert!(!establishes_persistent_congestion(&[a, b], Duration::from_millis(100)));
    }

    /// The Application Data space subtracts ack delay; Initial/Handshake do not
    /// (RFC 9002 §5.3).
    #[test]
    fn space_ack_delay_rule() {
        assert!(PacketNumberSpace::ApplicationData.uses_ack_delay());
        assert!(!PacketNumberSpace::Initial.uses_ack_delay());
        assert!(!PacketNumberSpace::Handshake.uses_ack_delay());
    }
}
