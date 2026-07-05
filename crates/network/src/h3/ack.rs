//! QUIC ACK generation — receiver-side packet-number tracking (RFC 9000 §13.2,
//! §19.3).
//!
//! This module is the mirror image of [`loss`](super::loss): where the loss
//! layer records the packets *we sent* and processes the ACK frames a peer sends
//! back, [`AckGenerator`] records the packets *we received* and decides when — and
//! how — to acknowledge them. The connection layer keeps one generator per
//! [`PacketNumberSpace`], feeds every decrypted packet's number into
//! [`AckGenerator::on_packet_received`], and drains an [`Frame::Ack`] out of
//! [`AckGenerator::generate_ack_frame`] when [`AckGenerator::ack_urgency`] says an
//! acknowledgement is owed.
//!
//! ## Received-range set (RFC 9000 §19.3.1)
//!
//! The generator stores the set of received packet numbers as a list of disjoint,
//! non-adjacent inclusive ranges kept sorted ascending. An ACK frame reports these
//! ranges from the largest downward: the top range becomes `largest_acked` /
//! `first_ack_range`, and each lower range is encoded as a `Gap` of unacknowledged
//! packets followed by an `ACK Range Length` of acknowledged ones, both relative to
//! the previous range (RFC 9000 §19.3.1). Because ACKs are cumulative the range set
//! is *not* cleared when a frame is generated; [`AckGenerator::on_ack_of_ack`] is the
//! only thing that discards low ranges, once the peer has acknowledged an ACK that
//! covered them (RFC 9000 §13.2.4).
//!
//! ## When to acknowledge (RFC 9000 §13.2.1, §13.2.2)
//!
//! A received ack-eliciting packet arms an acknowledgement. The generator sends it
//! *immediately* when the packet arrived out of order (a reordering or gap-filling
//! packet, which speeds the peer's loss detection), when the number of unacknowledged
//! ack-eliciting packets reaches the [`AckGenerator::ack_eliciting_threshold`]
//! (default 2, RFC 9000 §13.2.2), when the packet was ECN-CE marked, or whenever the
//! space does not permit delaying ACKs at all (Initial and Handshake always
//! acknowledge without delay). Otherwise the acknowledgement may be delayed up to the
//! peer's `max_ack_delay`; [`AckGenerator::ack_urgency`] reports the deadline so the
//! caller can arm a timer. Non-ack-eliciting packets update the range set but never,
//! on their own, arm an acknowledgement.
//!
//! ## Out of scope
//!
//! - The ACK-delay timer IO and the actual packet send — the caller arms a timer on
//!   the [`AckUrgency::Delayed`] deadline and calls back in.
//! - Bundling the produced [`Frame::Ack`] with other frames into a packet, and
//!   feeding the sent ACK into [`loss`](super::loss) as a non-ack-eliciting,
//!   not-in-flight packet.

use std::time::{Duration, Instant};

use super::loss::PacketNumberSpace;
use super::quic_frame::{AckRange, EcnCounts, Frame};

/// The largest legal QUIC variable-length integer (2^62 − 1, RFC 9000 §16); the
/// scaled ACK Delay is clamped to it so an absurd elapsed time cannot overflow the
/// wire encoding.
const VARINT_MAX: u64 = (1 << 62) - 1;

/// The default number of unacknowledged ack-eliciting packets that forces an
/// immediate acknowledgement (RFC 9000 §13.2.2 recommends acknowledging at least
/// every second ack-eliciting packet).
pub const DEFAULT_ACK_ELICITING_THRESHOLD: u64 = 2;

/// The ECN codepoint the IP layer carried on a received packet (RFC 9000 §13.4,
/// RFC 3168). The generator tallies these into the ECN counts an ACK frame reports
/// (RFC 9000 §19.3.2) and treats an [`EcnCodepoint::Ce`] as a trigger for an
/// immediate acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcnCodepoint {
    /// Not ECN-Capable Transport — the packet carried no ECN marking.
    NotEct,
    /// ECT(0) — ECN-Capable Transport, codepoint 0.
    Ect0,
    /// ECT(1) — ECN-Capable Transport, codepoint 1.
    Ect1,
    /// CE — Congestion Experienced; the network signalled congestion.
    Ce,
}

/// How urgently an acknowledgement is owed (RFC 9000 §13.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckUrgency {
    /// No acknowledgement is currently owed.
    None,
    /// An acknowledgement must be sent as soon as possible.
    Immediate,
    /// An acknowledgement is owed but may be delayed until the given instant.
    Delayed(Instant),
}

/// One disjoint inclusive range of received packet numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PnRange {
    /// Smallest received packet number in the range.
    start: u64,
    /// Largest received packet number in the range.
    end: u64,
}

/// One packet-number space's receiver-side acknowledgement state (RFC 9000 §13.2).
///
/// Tracks which packet numbers have been received (as a set of ranges), when an
/// acknowledgement is owed, and the ECN counts to report, and produces the
/// [`Frame::Ack`] the connection layer sends back.
#[derive(Debug, Clone)]
pub struct AckGenerator {
    /// Which packet-number space this generator acknowledges.
    space: PacketNumberSpace,
    /// Received packet numbers as disjoint, non-adjacent inclusive ranges, sorted
    /// ascending by `start`.
    ranges: Vec<PnRange>,
    /// The largest packet number received so far, or `None` before the first packet.
    largest_received: Option<u64>,
    /// When the largest received packet arrived; the basis for the ACK Delay field.
    largest_received_time: Option<Instant>,
    /// Count of ack-eliciting packets received since the last ACK was generated.
    ack_eliciting_since_last_ack: u64,
    /// Number of unacknowledged ack-eliciting packets that forces an immediate ACK.
    ack_eliciting_threshold: u64,
    /// The peer's advertised `max_ack_delay` (RFC 9000 §18.2); the longest an
    /// acknowledgement may be delayed in a space that permits delay.
    max_ack_delay: Duration,
    /// Whether an immediate acknowledgement is required (threshold, reordering,
    /// ECN-CE, or a space that never delays ACKs).
    immediate: bool,
    /// The deadline for a delayed acknowledgement, if one is owed but not immediate.
    deadline: Option<Instant>,
    /// Cumulative ECN counts across the connection for this space (RFC 9000 §13.4).
    ecn_counts: EcnCounts,
    /// Whether any ECN-marked packet has been seen; the ACK frame reports ECN counts
    /// only once at least one marking has been observed.
    ecn_seen: bool,
}

impl AckGenerator {
    /// Creates an empty generator for the given space with the peer's `max_ack_delay`
    /// and the default ack-eliciting threshold. Initial and Handshake spaces never
    /// delay acknowledgements regardless of `max_ack_delay`
    /// ([`PacketNumberSpace::uses_ack_delay`]).
    pub fn new(space: PacketNumberSpace, max_ack_delay: Duration) -> Self {
        Self {
            space,
            ranges: Vec::new(),
            largest_received: None,
            largest_received_time: None,
            ack_eliciting_since_last_ack: 0,
            ack_eliciting_threshold: DEFAULT_ACK_ELICITING_THRESHOLD,
            max_ack_delay,
            immediate: false,
            deadline: None,
            ecn_counts: EcnCounts { ect0: 0, ect1: 0, ecn_ce: 0 },
            ecn_seen: false,
        }
    }

    /// Overrides the ack-eliciting threshold (default
    /// [`DEFAULT_ACK_ELICITING_THRESHOLD`]); a threshold of 0 is treated as 1 so at
    /// least one ack-eliciting packet is required before an immediate ACK.
    pub fn with_ack_eliciting_threshold(mut self, threshold: u64) -> Self {
        self.ack_eliciting_threshold = threshold.max(1);
        self
    }

    /// The packet-number space this generator acknowledges.
    pub fn space(&self) -> PacketNumberSpace {
        self.space
    }

    /// The largest packet number received so far, or `None` before the first packet.
    pub fn largest_received(&self) -> Option<u64> {
        self.largest_received
    }

    /// Whether any packet has been received (and therefore an ACK could be produced).
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Records receipt of packet `pn`, carrying ECN codepoint `ecn`, arriving at
    /// `now`. `ack_eliciting` is true when the packet carried any frame other than
    /// ACK, PADDING, or CONNECTION_CLOSE (RFC 9002 §2). Updates the received-range
    /// set, the ECN counts, and the acknowledgement urgency.
    pub fn on_packet_received(
        &mut self,
        pn: u64,
        ack_eliciting: bool,
        ecn: EcnCodepoint,
        now: Instant,
    ) {
        let previous_largest = self.largest_received;

        self.insert_pn(pn);

        if self.largest_received.is_none_or(|l| pn > l) {
            self.largest_received = Some(pn);
            self.largest_received_time = Some(now);
        }

        match ecn {
            EcnCodepoint::NotEct => {}
            EcnCodepoint::Ect0 => {
                self.ecn_counts.ect0 = self.ecn_counts.ect0.saturating_add(1);
                self.ecn_seen = true;
            }
            EcnCodepoint::Ect1 => {
                self.ecn_counts.ect1 = self.ecn_counts.ect1.saturating_add(1);
                self.ecn_seen = true;
            }
            EcnCodepoint::Ce => {
                self.ecn_counts.ecn_ce = self.ecn_counts.ecn_ce.saturating_add(1);
                self.ecn_seen = true;
            }
        }

        // A CE-marked packet always warrants an immediate acknowledgement so the
        // sender reacts to congestion promptly (RFC 9000 §13.2.1).
        if ecn == EcnCodepoint::Ce {
            self.immediate = true;
        }

        if !ack_eliciting {
            return;
        }

        self.ack_eliciting_since_last_ack = self.ack_eliciting_since_last_ack.saturating_add(1);

        // In-order arrival is the very first packet, or one exactly above the current
        // largest. Anything else — a late (reordered) packet or one that skips ahead
        // of a gap — is out of order and warrants an immediate ACK (RFC 9000 §13.2.1).
        let in_order = match previous_largest {
            None => true,
            Some(l) => pn == l + 1,
        };

        if !in_order
            || self.ack_eliciting_since_last_ack >= self.ack_eliciting_threshold
            || !self.space.uses_ack_delay()
        {
            self.immediate = true;
        } else if self.deadline.is_none() {
            self.deadline = Some(now + self.max_ack_delay);
        }
    }

    /// How urgently an acknowledgement is owed right now (RFC 9000 §13.2.1). An
    /// immediate obligation takes precedence over a delayed one.
    pub fn ack_urgency(&self) -> AckUrgency {
        if self.immediate {
            AckUrgency::Immediate
        } else if let Some(deadline) = self.deadline {
            AckUrgency::Delayed(deadline)
        } else {
            AckUrgency::None
        }
    }

    /// Whether an acknowledgement should be sent at `now`: an immediate one is owed,
    /// or a delayed one whose deadline has passed.
    pub fn should_send_ack(&self, now: Instant) -> bool {
        match self.ack_urgency() {
            AckUrgency::Immediate => true,
            AckUrgency::Delayed(deadline) => now >= deadline,
            AckUrgency::None => false,
        }
    }

    /// Builds the [`Frame::Ack`] acknowledging every packet received so far and clears
    /// the pending-acknowledgement state (the range set persists, since ACKs are
    /// cumulative). Returns `None` when no packet has been received. `now` and
    /// `ack_delay_exponent` (the peer's, RFC 9000 §18.2) scale the ACK Delay field;
    /// spaces that do not delay acknowledgements report a delay of 0.
    pub fn generate_ack_frame(&mut self, now: Instant, ack_delay_exponent: u64) -> Option<Frame> {
        if self.ranges.is_empty() {
            return None;
        }

        // Ranges are sorted ascending; the ACK frame reports them from the top down.
        let top = *self.ranges.last()?;
        let largest_acked = top.end;
        let first_ack_range = top.end - top.start;

        let mut ranges = Vec::with_capacity(self.ranges.len() - 1);
        let mut previous_smallest = top.start;
        for range in self.ranges.iter().rev().skip(1) {
            // Ranges are disjoint and non-adjacent, so `previous_smallest - end >= 2`
            // and the encoded Gap (actual gap minus one, RFC 9000 §19.3.1) is >= 0.
            let gap = previous_smallest - range.end - 2;
            let length = range.end - range.start;
            ranges.push(AckRange { gap, length });
            previous_smallest = range.start;
        }

        let ack_delay = if self.space.uses_ack_delay() {
            let elapsed = self
                .largest_received_time
                .map(|t| now.saturating_duration_since(t))
                .unwrap_or_default();
            let scaled = elapsed.as_micros() >> ack_delay_exponent;
            scaled.min(VARINT_MAX as u128) as u64
        } else {
            0
        };

        let ecn = if self.ecn_seen { Some(self.ecn_counts) } else { None };

        // The acknowledgement obligation is discharged; the received ranges stay.
        self.immediate = false;
        self.deadline = None;
        self.ack_eliciting_since_last_ack = 0;

        Some(Frame::Ack { largest_acked, ack_delay, first_ack_range, ranges, ecn })
    }

    /// Discards received ranges the peer no longer needs acknowledged, given that it
    /// has acknowledged one of our ACK frames whose `largest_acked` was
    /// `acked_largest` (RFC 9000 §13.2.4). Every packet number at or below
    /// `acked_largest` is dropped, except that the range containing the overall
    /// largest received packet is always retained so future ACKs still report it.
    pub fn on_ack_of_ack(&mut self, acked_largest: u64) {
        let Some(top_start) = self.ranges.last().map(|r| r.start) else {
            return;
        };
        // The highest range is retained in full — an ACK must always report the
        // largest received packet — so nothing below it can be discarded when it is
        // the only (and lowest) range.
        if top_start == 0 {
            return;
        }
        // Never discard the packet numbers in the highest range: clamp the cutoff to
        // strictly below its start.
        let cutoff = acked_largest.min(top_start - 1);

        self.ranges.retain(|r| r.end > cutoff);
        if let Some(first) = self.ranges.first_mut()
            && first.start <= cutoff
        {
            first.start = cutoff + 1;
        }
    }

    /// The cumulative ECN counts observed for this space (RFC 9000 §13.4).
    pub fn ecn_counts(&self) -> EcnCounts {
        self.ecn_counts
    }

    /// Inserts packet number `pn` into the sorted, disjoint, non-adjacent range set,
    /// merging with an adjacent range on either or both sides. A duplicate is a no-op.
    fn insert_pn(&mut self, pn: u64) {
        // Already covered by an existing range → duplicate.
        if self.ranges.iter().any(|r| r.start <= pn && pn <= r.end) {
            return;
        }

        let touches_left = |r: &PnRange| pn > 0 && r.end == pn - 1;
        let touches_right = |r: &PnRange| r.start == pn + 1;

        let left = self.ranges.iter().position(touches_left);
        let right = self.ranges.iter().position(touches_right);

        match (left, right) {
            (Some(l), Some(r)) => {
                // Bridge two ranges into one; remove the higher index first.
                let (lo, hi) = if l < r { (l, r) } else { (r, l) };
                let hi_end = self.ranges[hi].end;
                let lo_start = self.ranges[lo].start;
                self.ranges.remove(hi);
                self.ranges[lo] = PnRange { start: lo_start, end: hi_end };
            }
            (Some(l), None) => self.ranges[l].end = pn,
            (None, Some(r)) => self.ranges[r].start = pn,
            (None, None) => {
                let pos = self.ranges.partition_point(|r| r.start < pn);
                self.ranges.insert(pos, PnRange { start: pn, end: pn });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::quic_frame;

    fn app_gen() -> AckGenerator {
        AckGenerator::new(PacketNumberSpace::ApplicationData, Duration::from_millis(25))
    }

    /// Reconstructs the set of acknowledged packet numbers from an ACK frame, so a
    /// generated frame can be checked against the packets that were fed in.
    fn acked_set(frame: &Frame) -> Vec<u64> {
        let Frame::Ack { largest_acked, first_ack_range, ranges, .. } = frame else {
            panic!("not an ACK frame");
        };
        let mut acked = Vec::new();
        let mut largest = *largest_acked;
        let mut smallest = largest - *first_ack_range;
        for pn in smallest..=largest {
            acked.push(pn);
        }
        for range in ranges {
            // Next range's largest = previous smallest - gap - 2 (RFC 9000 §19.3.1).
            largest = smallest - range.gap - 2;
            smallest = largest - range.length;
            for pn in smallest..=largest {
                acked.push(pn);
            }
        }
        acked.sort_unstable();
        acked
    }

    #[test]
    fn empty_generator_produces_no_frame() {
        let mut acker = app_gen();
        let now = Instant::now();
        assert!(acker.is_empty());
        assert_eq!(acker.ack_urgency(), AckUrgency::None);
        assert!(acker.generate_ack_frame(now, 3).is_none());
    }

    #[test]
    fn single_packet_acks_itself() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        assert_eq!(acked_set(&frame), vec![0]);
    }

    #[test]
    fn contiguous_run_is_one_range() {
        let mut acker = app_gen();
        let now = Instant::now();
        for pn in 0..=5 {
            acker.on_packet_received(pn, true, EcnCodepoint::NotEct, now);
        }
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        if let Frame::Ack { first_ack_range, ranges, largest_acked, .. } = &frame {
            assert_eq!(*largest_acked, 5);
            assert_eq!(*first_ack_range, 5);
            assert!(ranges.is_empty());
        } else {
            panic!("not ack");
        }
        assert_eq!(acked_set(&frame), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn gaps_become_ack_ranges() {
        let mut acker = app_gen();
        let now = Instant::now();
        // Two disjoint groups: {0,1,2} and {5,6}; 3,4 missing.
        for pn in [0, 1, 2, 5, 6] {
            acker.on_packet_received(pn, true, EcnCodepoint::NotEct, now);
        }
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        if let Frame::Ack { largest_acked, first_ack_range, ranges, .. } = &frame {
            assert_eq!(*largest_acked, 6);
            assert_eq!(*first_ack_range, 1); // 5..=6
            assert_eq!(ranges.len(), 1);
            // Gap of packets 3,4 = actual gap 2, encoded 2 - 1... encoded = prev_smallest(5) - end(2) - 2 = 1.
            assert_eq!(ranges[0].gap, 1);
            assert_eq!(ranges[0].length, 2); // 0..=2
        } else {
            panic!("not ack");
        }
        assert_eq!(acked_set(&frame), vec![0, 1, 2, 5, 6]);
    }

    #[test]
    fn out_of_order_arrival_merges_and_roundtrips() {
        let mut acker = app_gen();
        let now = Instant::now();
        for pn in [9, 1, 0, 3, 2, 7, 8] {
            acker.on_packet_received(pn, true, EcnCodepoint::NotEct, now);
        }
        // Groups: {0,1,2,3}, {7,8,9}.
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        assert_eq!(acked_set(&frame), vec![0, 1, 2, 3, 7, 8, 9]);
    }

    #[test]
    fn generated_frame_survives_wire_roundtrip() {
        let mut acker = app_gen();
        let now = Instant::now();
        for pn in [0, 1, 2, 5, 6, 10] {
            acker.on_packet_received(pn, true, EcnCodepoint::NotEct, now);
        }
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        let mut buf = Vec::new();
        frame.encode(&mut buf).expect("encode");
        let (parsed, consumed) = quic_frame::Frame::parse(&buf).expect("parse");
        assert_eq!(consumed, buf.len());
        assert_eq!(parsed, frame);
        assert_eq!(acked_set(&parsed), vec![0, 1, 2, 5, 6, 10]);
    }

    #[test]
    fn duplicate_packet_is_noop_for_range_set() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(4, true, EcnCodepoint::NotEct, now);
        acker.on_packet_received(4, true, EcnCodepoint::NotEct, now);
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        assert_eq!(acked_set(&frame), vec![4]);
    }

    #[test]
    fn threshold_forces_immediate_after_two_ack_eliciting() {
        let mut acker = app_gen();
        let now = Instant::now();
        // First in-order ack-eliciting → delayed.
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        match acker.ack_urgency() {
            AckUrgency::Delayed(d) => assert_eq!(d, now + Duration::from_millis(25)),
            other => panic!("expected delayed, got {other:?}"),
        }
        // Second in-order ack-eliciting → threshold reached → immediate.
        acker.on_packet_received(1, true, EcnCodepoint::NotEct, now);
        assert_eq!(acker.ack_urgency(), AckUrgency::Immediate);
    }

    #[test]
    fn reordering_forces_immediate() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        // Skip ahead of a gap (packet 2 before 1) → out of order → immediate.
        acker.on_packet_received(2, true, EcnCodepoint::NotEct, now);
        assert_eq!(acker.ack_urgency(), AckUrgency::Immediate);
    }

    #[test]
    fn non_ack_eliciting_does_not_arm_ack() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, false, EcnCodepoint::NotEct, now);
        acker.on_packet_received(1, false, EcnCodepoint::NotEct, now);
        assert_eq!(acker.ack_urgency(), AckUrgency::None);
        // But the packets are still acknowledged when some other trigger fires.
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        assert_eq!(acked_set(&frame), vec![0, 1]);
    }

    #[test]
    fn initial_space_acks_immediately() {
        let mut acker =
            AckGenerator::new(PacketNumberSpace::Initial, Duration::from_millis(25));
        let now = Instant::now();
        // Even a single in-order ack-eliciting packet is immediate: Initial never delays.
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        assert_eq!(acker.ack_urgency(), AckUrgency::Immediate);
    }

    #[test]
    fn initial_space_reports_zero_ack_delay() {
        let mut acker =
            AckGenerator::new(PacketNumberSpace::Initial, Duration::from_millis(25));
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        let later = now + Duration::from_millis(100);
        let frame = acker.generate_ack_frame(later, 3).expect("frame");
        if let Frame::Ack { ack_delay, .. } = frame {
            assert_eq!(ack_delay, 0);
        } else {
            panic!("not ack");
        }
    }

    #[test]
    fn app_space_scales_ack_delay_by_exponent() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        // 8192 microseconds elapsed; exponent 3 → 8192 >> 3 = 1024.
        let later = now + Duration::from_micros(8192);
        let frame = acker.generate_ack_frame(later, 3).expect("frame");
        if let Frame::Ack { ack_delay, .. } = frame {
            assert_eq!(ack_delay, 1024);
        } else {
            panic!("not ack");
        }
    }

    #[test]
    fn ecn_ce_marks_immediate_and_counts() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, false, EcnCodepoint::Ce, now);
        // CE forces immediate even though the packet was not ack-eliciting.
        assert_eq!(acker.ack_urgency(), AckUrgency::Immediate);
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        if let Frame::Ack { ecn: Some(counts), .. } = frame {
            assert_eq!(counts, EcnCounts { ect0: 0, ect1: 0, ecn_ce: 1 });
        } else {
            panic!("expected ECN counts");
        }
    }

    #[test]
    fn ecn_counts_accumulate_and_roundtrip() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::Ect0, now);
        acker.on_packet_received(1, true, EcnCodepoint::Ect0, now);
        acker.on_packet_received(2, true, EcnCodepoint::Ect1, now);
        assert_eq!(acker.ecn_counts(), EcnCounts { ect0: 2, ect1: 1, ecn_ce: 0 });
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        let mut buf = Vec::new();
        frame.encode(&mut buf).expect("encode");
        let (parsed, _consumed) = quic_frame::Frame::parse(&buf).expect("parse");
        assert_eq!(parsed, frame);
    }

    #[test]
    fn generating_clears_pending_but_keeps_ranges() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        acker.on_packet_received(1, true, EcnCodepoint::NotEct, now);
        assert_eq!(acker.ack_urgency(), AckUrgency::Immediate);
        let _ = acker.generate_ack_frame(now, 3).expect("frame");
        // Obligation discharged.
        assert_eq!(acker.ack_urgency(), AckUrgency::None);
        // But a fresh ACK still reports the earlier packets (cumulative).
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        assert_eq!(acked_set(&frame), vec![0, 1]);
    }

    #[test]
    fn on_ack_of_ack_drops_low_ranges_but_keeps_top() {
        let mut acker = app_gen();
        let now = Instant::now();
        for pn in [0, 1, 2, 5, 6, 10] {
            acker.on_packet_received(pn, true, EcnCodepoint::NotEct, now);
        }
        // Peer acknowledged an ACK covering up to packet 6.
        acker.on_ack_of_ack(6);
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        // Everything <= 6 dropped; only the top range {10} remains.
        assert_eq!(acked_set(&frame), vec![10]);
    }

    #[test]
    fn on_ack_of_ack_never_empties_top_range() {
        let mut acker = app_gen();
        let now = Instant::now();
        for pn in [0, 1, 2, 3] {
            acker.on_packet_received(pn, true, EcnCodepoint::NotEct, now);
        }
        // The single range is the highest range, so it is retained in full even when
        // the peer acknowledges beyond the largest received packet.
        acker.on_ack_of_ack(100);
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        assert_eq!(acked_set(&frame), vec![0, 1, 2, 3]);
    }

    #[test]
    fn custom_threshold_of_three() {
        let mut acker = app_gen().with_ack_eliciting_threshold(3);
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        acker.on_packet_received(1, true, EcnCodepoint::NotEct, now);
        // Two in-order ack-eliciting packets: below threshold 3 → still delayed.
        assert!(matches!(acker.ack_urgency(), AckUrgency::Delayed(_)));
        acker.on_packet_received(2, true, EcnCodepoint::NotEct, now);
        assert_eq!(acker.ack_urgency(), AckUrgency::Immediate);
    }

    #[test]
    fn should_send_ack_respects_delayed_deadline() {
        let mut acker = app_gen();
        let now = Instant::now();
        acker.on_packet_received(0, true, EcnCodepoint::NotEct, now);
        assert!(!acker.should_send_ack(now));
        assert!(acker.should_send_ack(now + Duration::from_millis(25)));
    }

    #[test]
    fn fills_gap_from_below_merges_three_into_one() {
        let mut acker = app_gen();
        let now = Instant::now();
        // Create {0,1} and {3,4}, then fill 2 to bridge into {0..=4}.
        for pn in [0, 1, 3, 4] {
            acker.on_packet_received(pn, false, EcnCodepoint::NotEct, now);
        }
        acker.on_packet_received(2, false, EcnCodepoint::NotEct, now);
        let frame = acker.generate_ack_frame(now, 3).expect("frame");
        if let Frame::Ack { ranges, first_ack_range, .. } = &frame {
            assert!(ranges.is_empty());
            assert_eq!(*first_ack_range, 4);
        } else {
            panic!("not ack");
        }
        assert_eq!(acked_set(&frame), vec![0, 1, 2, 3, 4]);
    }
}
