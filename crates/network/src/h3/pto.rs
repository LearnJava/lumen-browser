//! QUIC loss-detection timer + Probe Timeout (PTO) — RFC 9002 §6.2, Appendix A.
//!
//! Slice 7 ([`super::recovery`]) built the RTT estimator and NewReno controller;
//! slice 8 ([`super::loss`]) built the per-space [`SentPacketRegistry`] and the
//! ack/loss detection that decides *which* packets are acknowledged or lost. Both
//! left the single question RFC 9002 Appendix A hangs everything else on — *when*
//! does the loss-detection timer fire, and *what* does the endpoint do when it
//! does — to "a later slice". This is that slice.
//!
//! [`LossDetection`] is the top-level RFC 9002 Appendix A object. It owns the
//! three per-space registries (Initial, Handshake, Application Data, RFC 9000
//! §12.3), the [`RttEstimator`], the PTO backoff counter, and the handful of
//! connection-level flags the timer depends on. It answers two questions:
//!
//! - [`LossDetection::set_loss_detection_timer`] (RFC 9002 §A.8
//!   `SetLossDetectionTimer`): given the current time, at what instant should the
//!   single loss-detection timer be armed — or should it be cancelled? It prefers
//!   the earliest **time-threshold** loss time across spaces
//!   ([`SentPacketRegistry::loss_time`]); failing that, and while the endpoint
//!   still has unacknowledged ack-eliciting data or an unvalidated peer, it arms
//!   the **PTO** ([`LossDetection::get_pto_time_and_space`]).
//! - [`LossDetection::on_timeout`] (RFC 9002 §A.9 `OnLossDetectionTimeout`): when
//!   the armed timer fires, either declare the time-threshold losses in the
//!   relevant space (removing them from the registry and handing them back for the
//!   congestion controller) or, on a PTO, tell the caller to send one or two
//!   ack-eliciting probe packets and bump the exponential backoff counter.
//!
//! ### PTO computation (RFC 9002 §6.2.1, §A.8 `GetPtoTimeAndSpace`)
//!
//! The base PTO duration is `smoothed_rtt + max(4 * rttvar, kGranularity)`,
//! multiplied by `2 ^ pto_count` for exponential backoff on repeated firings. The
//! Application Data space additionally adds `max_ack_delay * 2 ^ pto_count`, since
//! only that space lets the peer delay acknowledgements; Initial and Handshake do
//! not (RFC 9002 §6.2.1). When no ack-eliciting packet is in flight anywhere the
//! anti-deadlock PTO is armed from *now* rather than from a send time, using the
//! Handshake space once handshake keys exist and Initial before then (RFC 9002
//! §6.2.2.1).
//!
//! Like every other slice this is a pure, deterministic state machine: no IO, no
//! packet protection, no wall-clock reads. The caller supplies `now`, drives the
//! registries via slice 8, and acts on the returned [`LossTimer`] /
//! [`TimeoutAction`].
//!
//! ### Out of scope (later slices)
//!
//! - Actually arming an OS timer and sending the probe datagrams; this module only
//!   says *when* and *what*.
//! - Header protection, AEAD packet protection, TLS 1.3, and `h3_do_request`
//!   dispatch.

use std::time::{Duration, Instant};

use super::loss::{PacketNumberSpace, SentPacket, SentPacketRegistry};
use super::recovery::{K_GRANULARITY, RttEstimator};

/// The three packet-number spaces in the fixed order RFC 9002 Appendix A iterates
/// them: Initial first (so it wins ties in the loss/PTO timer selection),
/// Handshake, then Application Data.
const SPACES: [PacketNumberSpace; 3] = [
    PacketNumberSpace::Initial,
    PacketNumberSpace::Handshake,
    PacketNumberSpace::ApplicationData,
];

/// The state the single loss-detection timer should be left in after
/// [`LossDetection::set_loss_detection_timer`] (RFC 9002 §A.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossTimer {
    /// The timer should be cancelled: no time-threshold loss is pending and there
    /// is nothing to probe (no ack-eliciting data in flight and the peer has
    /// validated the endpoint's address).
    Disarmed,
    /// The timer should be (re)armed to fire at this instant — either a
    /// time-threshold loss time or a probe timeout.
    Armed(Instant),
}

impl LossTimer {
    /// The instant the timer is armed for, or `None` when [`LossTimer::Disarmed`].
    pub fn deadline(self) -> Option<Instant> {
        match self {
            LossTimer::Armed(t) => Some(t),
            LossTimer::Disarmed => None,
        }
    }

    /// Whether the timer is armed.
    pub fn is_armed(self) -> bool {
        matches!(self, LossTimer::Armed(_))
    }
}

/// What the caller must do when the loss-detection timer fires
/// ([`LossDetection::on_timeout`], RFC 9002 §A.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeoutAction {
    /// The timer fired on the time-threshold branch: these packets in `space` were
    /// declared lost and removed from the registry. The caller feeds them to
    /// [`super::recovery::CongestionController::on_packets_lost`] (and checks
    /// [`super::loss::establishes_persistent_congestion`]).
    PacketsLost {
        /// The packet-number space the losses were detected in.
        space: PacketNumberSpace,
        /// The packets declared lost, ascending by packet number (possibly empty
        /// if they were already removed by a concurrent ACK).
        lost: Vec<SentPacket>,
    },
    /// The timer fired on a probe timeout: send `count` ack-eliciting probe
    /// packets in `space` (RFC 9002 §6.2.4). The exponential backoff counter has
    /// already been incremented, so the next PTO is longer.
    SendProbe {
        /// The packet-number space to send the probe(s) in.
        space: PacketNumberSpace,
        /// How many ack-eliciting packets to send: two while ack-eliciting data is
        /// in flight, one for the anti-deadlock probe when nothing is in flight.
        count: u8,
    },
}

/// The RFC 9002 Appendix A loss-detection and PTO state machine, tying the three
/// per-space [`SentPacketRegistry`]s and the [`RttEstimator`] together into the
/// single loss-detection timer.
#[derive(Debug, Clone)]
pub struct LossDetection {
    /// The Initial packet-number space registry.
    initial: SentPacketRegistry,
    /// The Handshake packet-number space registry.
    handshake: SentPacketRegistry,
    /// The Application Data (0-RTT and 1-RTT) packet-number space registry.
    app_data: SentPacketRegistry,
    /// The RTT estimator that feeds the PTO duration (RFC 9002 §5).
    rtt: RttEstimator,
    /// The PTO exponential-backoff exponent (RFC 9002 §6.2.1 `pto_count`): the PTO
    /// duration is scaled by `2 ^ pto_count`, reset to zero on any RTT sample.
    pto_count: u32,
    /// The peer's advertised `max_ack_delay` (RFC 9000 §18.2), added to the
    /// Application Data PTO only.
    max_ack_delay: Duration,
    /// Whether Handshake keys are available yet; selects the anti-deadlock probe
    /// space (RFC 9002 §6.2.2.1).
    has_handshake_keys: bool,
    /// Whether the TLS handshake is confirmed (RFC 9001 §4.1.2); the Application
    /// Data PTO is not armed before this.
    handshake_confirmed: bool,
    /// Whether the peer has validated this endpoint's address (RFC 9000 §8); until
    /// it has, the PTO stays armed even with nothing in flight to avoid deadlock.
    peer_completed_address_validation: bool,
}

impl LossDetection {
    /// Creates a fresh loss-detection state machine with the peer's advertised
    /// `max_ack_delay`, empty registries, and a seeded [`RttEstimator`].
    ///
    /// The connection-level flags start pessimistic: no handshake keys, handshake
    /// not confirmed, peer address not validated — the state at the very start of
    /// the QUIC handshake.
    pub fn new(max_ack_delay: Duration) -> Self {
        Self {
            initial: SentPacketRegistry::new(PacketNumberSpace::Initial),
            handshake: SentPacketRegistry::new(PacketNumberSpace::Handshake),
            app_data: SentPacketRegistry::new(PacketNumberSpace::ApplicationData),
            rtt: RttEstimator::new(),
            pto_count: 0,
            max_ack_delay,
            has_handshake_keys: false,
            handshake_confirmed: false,
            peer_completed_address_validation: false,
        }
    }

    /// The registry for `space`, borrowed immutably.
    pub fn registry(&self, space: PacketNumberSpace) -> &SentPacketRegistry {
        match space {
            PacketNumberSpace::Initial => &self.initial,
            PacketNumberSpace::Handshake => &self.handshake,
            PacketNumberSpace::ApplicationData => &self.app_data,
        }
    }

    /// The registry for `space`, borrowed mutably so the caller can record sent
    /// packets ([`SentPacketRegistry::on_packet_sent`]) and process ACKs
    /// ([`SentPacketRegistry::on_ack_received`]).
    pub fn registry_mut(&mut self, space: PacketNumberSpace) -> &mut SentPacketRegistry {
        match space {
            PacketNumberSpace::Initial => &mut self.initial,
            PacketNumberSpace::Handshake => &mut self.handshake,
            PacketNumberSpace::ApplicationData => &mut self.app_data,
        }
    }

    /// All three per-space registries borrowed mutably at once, in the fixed order
    /// `[Initial, Handshake, ApplicationData]`.
    ///
    /// [`LossDetection::registry_mut`] hands back one registry at a time, which
    /// cannot build the array of simultaneous per-space borrows the send-path flush
    /// needs: [`send_path::flush`](super::send_path::flush) takes every space's send
    /// state together so it can coalesce their packets into one datagram (RFC 9000
    /// §12.2), and each space's [`SpaceFlush`](super::send_path::SpaceFlush) carries
    /// its own `&mut SentPacketRegistry`. Splitting the disjoint struct-field borrows
    /// in a single call is what lets those live at the same time.
    pub fn registries_mut(&mut self) -> [&mut SentPacketRegistry; 3] {
        [&mut self.initial, &mut self.handshake, &mut self.app_data]
    }

    /// The RTT estimator, borrowed immutably.
    pub fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// The RTT estimator, borrowed mutably so the caller can fold in a sample.
    /// Folding in an RTT sample should be paired with [`LossDetection::reset_pto_count`]
    /// per RFC 9002 §6.2.1 (`pto_count` is reset on every RTT update).
    pub fn rtt_mut(&mut self) -> &mut RttEstimator {
        &mut self.rtt
    }

    /// The current PTO backoff exponent (`pto_count`, RFC 9002 §6.2.1).
    pub fn pto_count(&self) -> u32 {
        self.pto_count
    }

    /// Resets the PTO backoff to zero (RFC 9002 §6.2.1): done whenever a fresh RTT
    /// sample arrives or an ack newly acknowledges data, undoing the exponential
    /// growth from earlier probe timeouts.
    pub fn reset_pto_count(&mut self) {
        self.pto_count = 0;
    }

    /// Records that Handshake keys are now available (RFC 9002 §6.2.2.1); switches
    /// the anti-deadlock probe from the Initial to the Handshake space.
    pub fn set_has_handshake_keys(&mut self, has: bool) {
        self.has_handshake_keys = has;
    }

    /// Records whether the TLS handshake is confirmed (RFC 9001 §4.1.2). The
    /// Application Data PTO is only armed once it is.
    pub fn set_handshake_confirmed(&mut self, confirmed: bool) {
        self.handshake_confirmed = confirmed;
    }

    /// Records whether the peer has validated this endpoint's address (RFC 9000
    /// §8). Until it has, the PTO stays armed even with nothing in flight.
    pub fn set_peer_completed_address_validation(&mut self, validated: bool) {
        self.peer_completed_address_validation = validated;
    }

    /// Discards a packet-number space (RFC 9002 §A.4 `OnPacketNumberSpaceDiscarded`).
    ///
    /// Drops every sent packet tracked in `space` and resets the PTO backoff to
    /// zero. QUIC discards the Initial space once Handshake keys are installed and
    /// the Handshake space once the handshake is confirmed (RFC 9001 §4.9); the
    /// caller then re-arms the timer with [`LossDetection::set_loss_detection_timer`].
    pub fn discard_space(&mut self, space: PacketNumberSpace) {
        *self.registry_mut(space) = SentPacketRegistry::new(space);
        self.pto_count = 0;
    }

    /// Whether any packet-number space still has an ack-eliciting packet in flight
    /// (RFC 9002 §6.2.1). The PTO is disarmed only when this is false *and* the
    /// peer has validated the address.
    pub fn any_ack_eliciting_in_flight(&self) -> bool {
        SPACES.iter().any(|&s| self.registry(s).ack_eliciting_in_flight())
    }

    /// The earliest pending time-threshold loss time and its space, or `None` when
    /// no space has a pending loss time (RFC 9002 §A.8 `GetLossTimeAndSpace`).
    ///
    /// Ties are broken toward Initial, then Handshake, matching the RFC's
    /// strictly-less comparison over the fixed iteration order.
    pub fn get_loss_time_and_space(&self) -> Option<(Instant, PacketNumberSpace)> {
        let mut best: Option<(Instant, PacketNumberSpace)> = None;
        for &space in &SPACES {
            if let Some(t) = self.registry(space).loss_time()
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, space));
            }
        }
        best
    }

    /// The base PTO duration (RFC 9002 §6.2.1) with exponential backoff:
    /// `(smoothed_rtt + max(4 * rttvar, kGranularity)) * 2 ^ pto_count`, plus
    /// `max_ack_delay * 2 ^ pto_count` for the Application Data space.
    ///
    /// Arithmetic saturates rather than overflowing, so a pathological `pto_count`
    /// yields a far-future duration instead of a panic.
    fn pto_duration(&self, include_ack_delay: bool) -> Duration {
        let backoff = self.backoff();
        let base = self.rtt.smoothed_rtt() + self.rtt.rttvar().saturating_mul(4).max(K_GRANULARITY);
        let mut duration = base.saturating_mul(backoff);
        if include_ack_delay {
            duration = duration.saturating_add(self.max_ack_delay.saturating_mul(backoff));
        }
        duration
    }

    /// The exponential-backoff multiplier `2 ^ pto_count`, saturated at
    /// [`u32::MAX`] once `pto_count` reaches 32 (RFC 9002 §6.2.1).
    fn backoff(&self) -> u32 {
        1u32.checked_shl(self.pto_count).unwrap_or(u32::MAX)
    }

    /// The instant the PTO should fire and the space it belongs to, or `None` when
    /// no PTO can be armed (RFC 9002 §A.8 `GetPtoTimeAndSpace`).
    ///
    /// With no ack-eliciting packet in flight anywhere, the anti-deadlock PTO is
    /// anchored at `now` in the Handshake space (or Initial before Handshake keys
    /// exist). Otherwise it is the earliest, across spaces with ack-eliciting data
    /// in flight, of `time_of_last_ack_eliciting_packet + pto_duration` — skipping
    /// the Application Data space until the handshake is confirmed.
    pub fn get_pto_time_and_space(&self, now: Instant) -> Option<(Instant, PacketNumberSpace)> {
        if !self.any_ack_eliciting_in_flight() {
            // Anti-deadlock PTO (RFC 9002 §6.2.2.1): armed from the current time,
            // in Handshake once keys exist, else Initial.
            let space = if self.has_handshake_keys {
                PacketNumberSpace::Handshake
            } else {
                PacketNumberSpace::Initial
            };
            let duration = self.pto_duration(false);
            return now.checked_add(duration).map(|t| (t, space));
        }

        let mut best: Option<(Instant, PacketNumberSpace)> = None;
        for &space in &SPACES {
            let registry = self.registry(space);
            if !registry.ack_eliciting_in_flight() {
                continue;
            }
            if space == PacketNumberSpace::ApplicationData && !self.handshake_confirmed {
                // Do not arm the Application Data PTO before the handshake is
                // confirmed (RFC 9002 §A.8); return whatever earlier space set.
                break;
            }
            let include_ack_delay = space == PacketNumberSpace::ApplicationData;
            let Some(anchor) = registry.time_of_last_ack_eliciting_packet() else {
                continue;
            };
            let Some(candidate) = anchor.checked_add(self.pto_duration(include_ack_delay)) else {
                continue;
            };
            if best.is_none_or(|(bt, _)| candidate < bt) {
                best = Some((candidate, space));
            }
        }
        best
    }

    /// Computes the state the single loss-detection timer should be left in
    /// (RFC 9002 §A.8 `SetLossDetectionTimer`).
    ///
    /// A pending time-threshold loss time takes priority; otherwise the timer is
    /// disarmed when nothing is in flight and the peer has validated the address,
    /// and armed at the PTO in every other case. `now` anchors the anti-deadlock
    /// PTO.
    pub fn set_loss_detection_timer(&self, now: Instant) -> LossTimer {
        if let Some((loss_time, _)) = self.get_loss_time_and_space() {
            return LossTimer::Armed(loss_time);
        }

        if !self.any_ack_eliciting_in_flight() && self.peer_completed_address_validation {
            // No time-threshold loss, nothing to probe, peer validated: cancel.
            return LossTimer::Disarmed;
        }

        match self.get_pto_time_and_space(now) {
            Some((pto_time, _)) => LossTimer::Armed(pto_time),
            None => LossTimer::Disarmed,
        }
    }

    /// Handles the loss-detection timer firing (RFC 9002 §A.9
    /// `OnLossDetectionTimeout`).
    ///
    /// On the time-threshold branch it detects and removes the lost packets in the
    /// relevant space (via [`SentPacketRegistry::detect_and_remove_lost_packets`],
    /// leaving [`SentPacketRegistry::loss_time`] refreshed) and returns them. On a
    /// probe timeout it increments the backoff counter and asks the caller to send
    /// one or two ack-eliciting probe packets. Re-arm the timer afterwards with
    /// [`LossDetection::set_loss_detection_timer`].
    pub fn on_timeout(&mut self, now: Instant) -> TimeoutAction {
        if let Some((_, space)) = self.get_loss_time_and_space() {
            let latest_rtt = self.rtt.latest_rtt();
            let smoothed_rtt = self.rtt.smoothed_rtt();
            let lost = self
                .registry_mut(space)
                .detect_and_remove_lost_packets(now, latest_rtt, smoothed_rtt);
            return TimeoutAction::PacketsLost { space, lost };
        }

        // Probe timeout: two probes while data is in flight, one anti-deadlock
        // probe otherwise (RFC 9002 §A.9).
        let count = if self.any_ack_eliciting_in_flight() { 2 } else { 1 };
        let space = self
            .get_pto_time_and_space(now)
            .map(|(_, s)| s)
            .unwrap_or(if self.has_handshake_keys {
                PacketNumberSpace::Handshake
            } else {
                PacketNumberSpace::Initial
            });
        self.pto_count = self.pto_count.saturating_add(1);
        TimeoutAction::SendProbe { space, count }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::loss::SentPacket;

    /// A registered ack-eliciting, in-flight packet sent at `time_sent`.
    fn ack_eliciting(packet_number: u64, time_sent: Instant) -> SentPacket {
        SentPacket {
            packet_number,
            time_sent,
            ack_eliciting: true,
            in_flight: true,
            sent_bytes: 1200,
        }
    }

    /// A [`LossDetection`] with a known, stable RTT so PTO durations are
    /// predictable: smoothed_rtt = 100ms, rttvar driven toward a small value.
    fn seeded() -> (LossDetection, Instant) {
        let mut ld = LossDetection::new(Duration::from_millis(25));
        let base = Instant::now();
        // Fold in a couple of 100ms samples so smoothed_rtt settles near 100ms and
        // rttvar shrinks; Initial/Handshake spaces carry zero ack_delay.
        ld.rtt_mut()
            .update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::ZERO);
        ld.rtt_mut()
            .update_rtt(Duration::from_millis(100), Duration::ZERO, Duration::ZERO);
        (ld, base)
    }

    #[test]
    fn new_state_is_pessimistic() {
        let ld = LossDetection::new(Duration::from_millis(25));
        assert_eq!(ld.pto_count(), 0);
        assert!(!ld.any_ack_eliciting_in_flight());
        assert!(ld.get_loss_time_and_space().is_none());
    }

    #[test]
    fn loss_timer_disarms_when_idle_and_validated() {
        let (mut ld, now) = seeded();
        ld.set_peer_completed_address_validation(true);
        assert_eq!(ld.set_loss_detection_timer(now), LossTimer::Disarmed);
    }

    #[test]
    fn anti_deadlock_pto_armed_from_now_before_validation() {
        let (ld, now) = seeded();
        // Nothing in flight, peer not validated: anti-deadlock PTO from `now`.
        let timer = ld.set_loss_detection_timer(now);
        let deadline = timer.deadline().expect("armed");
        // base PTO = smoothed_rtt(≈100ms) + max(4*rttvar, 1ms); anchored at now, no
        // ack_delay in the anti-deadlock case, so at least ~100ms out.
        assert!(deadline >= now + Duration::from_millis(100));
    }

    #[test]
    fn anti_deadlock_space_follows_handshake_keys() {
        let (mut ld, now) = seeded();
        assert_eq!(
            ld.get_pto_time_and_space(now).map(|(_, s)| s),
            Some(PacketNumberSpace::Initial)
        );
        ld.set_has_handshake_keys(true);
        assert_eq!(
            ld.get_pto_time_and_space(now).map(|(_, s)| s),
            Some(PacketNumberSpace::Handshake)
        );
    }

    #[test]
    fn pto_anchored_at_last_ack_eliciting_send() {
        let (mut ld, now) = seeded();
        let sent_at = now;
        ld.registry_mut(PacketNumberSpace::Handshake)
            .on_packet_sent(ack_eliciting(0, sent_at));
        let (pto, space) = ld.get_pto_time_and_space(now).expect("pto");
        assert_eq!(space, PacketNumberSpace::Handshake);
        // Anchored at the send time (not `now`) since data is in flight.
        assert_eq!(pto, sent_at + ld.pto_duration(false));
    }

    #[test]
    fn loss_time_takes_priority_over_pto() {
        let (mut ld, now) = seeded();
        // Send two packets, ack the later one so the earlier crosses the time
        // threshold and sets loss_time.
        let reg = ld.registry_mut(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(ack_eliciting(0, now));
        reg.on_packet_sent(ack_eliciting(1, now + Duration::from_millis(1)));
        // Ack packet 1 only, early enough that packet 0 is still pending (below
        // the ~112ms time-loss deadline), so it sets loss_time rather than being
        // declared lost.
        let ack_now = now + Duration::from_millis(50);
        reg.on_ack_received(1, 0, &[], ack_now);
        // Run loss detection so loss_time is populated for packet 0.
        let latest = ld.rtt().latest_rtt();
        let smoothed = ld.rtt().smoothed_rtt();
        ld.registry_mut(PacketNumberSpace::ApplicationData)
            .detect_and_remove_lost_packets(ack_now, latest, smoothed);
        let loss = ld.get_loss_time_and_space();
        assert!(loss.is_some(), "loss_time should be set for the un-acked packet");
        let timer = ld.set_loss_detection_timer(ack_now);
        assert_eq!(timer, LossTimer::Armed(loss.unwrap().0));
    }

    #[test]
    fn on_timeout_pto_increments_backoff_and_sends_two() {
        let (mut ld, now) = seeded();
        ld.registry_mut(PacketNumberSpace::Handshake)
            .on_packet_sent(ack_eliciting(0, now));
        let fire = now + Duration::from_secs(1);
        let action = ld.on_timeout(fire);
        assert_eq!(
            action,
            TimeoutAction::SendProbe {
                space: PacketNumberSpace::Handshake,
                count: 2
            }
        );
        assert_eq!(ld.pto_count(), 1);
        // Backoff doubled the next PTO duration.
        let doubled = ld.pto_duration(false);
        assert!(doubled >= Duration::from_millis(200));
    }

    #[test]
    fn on_timeout_anti_deadlock_sends_one() {
        let (mut ld, now) = seeded();
        // Nothing in flight → anti-deadlock single probe.
        let action = ld.on_timeout(now + Duration::from_secs(1));
        assert_eq!(
            action,
            TimeoutAction::SendProbe {
                space: PacketNumberSpace::Initial,
                count: 1
            }
        );
        assert_eq!(ld.pto_count(), 1);
    }

    #[test]
    fn on_timeout_loss_branch_returns_lost_packets() {
        let (mut ld, now) = seeded();
        let reg = ld.registry_mut(PacketNumberSpace::ApplicationData);
        reg.on_packet_sent(ack_eliciting(0, now));
        reg.on_packet_sent(ack_eliciting(1, now + Duration::from_millis(1)));
        // Ack packet 1 early: packet 0 is one below (no packet-threshold) and not
        // yet past the ~112ms time deadline, so detection defers it via loss_time.
        let ack_now = now + Duration::from_millis(50);
        reg.on_ack_received(1, 0, &[], ack_now);
        let latest = ld.rtt().latest_rtt();
        let smoothed = ld.rtt().smoothed_rtt();
        ld.registry_mut(PacketNumberSpace::ApplicationData)
            .detect_and_remove_lost_packets(ack_now, latest, smoothed);
        assert!(ld.get_loss_time_and_space().is_some());
        // The timer fires past the deadline → loss branch declares packet 0 lost.
        let action = ld.on_timeout(now + Duration::from_millis(200));
        match action {
            TimeoutAction::PacketsLost { space, lost } => {
                assert_eq!(space, PacketNumberSpace::ApplicationData);
                assert_eq!(lost.len(), 1);
                assert_eq!(lost[0].packet_number, 0);
            }
            other => panic!("expected PacketsLost, got {other:?}"),
        }
    }

    #[test]
    fn app_data_pto_skipped_until_handshake_confirmed() {
        let (mut ld, now) = seeded();
        ld.set_has_handshake_keys(true);
        ld.registry_mut(PacketNumberSpace::ApplicationData)
            .on_packet_sent(ack_eliciting(0, now));
        // Handshake not confirmed → App Data PTO must not be armed.
        assert!(ld.get_pto_time_and_space(now).is_none());
        ld.set_handshake_confirmed(true);
        let (_, space) = ld.get_pto_time_and_space(now).expect("pto after confirm");
        assert_eq!(space, PacketNumberSpace::ApplicationData);
    }

    #[test]
    fn app_data_pto_includes_max_ack_delay() {
        let (mut ld, _now) = seeded();
        ld.set_handshake_confirmed(true);
        let handshake_duration = ld.pto_duration(false);
        let app_duration = ld.pto_duration(true);
        // App Data PTO is longer by exactly max_ack_delay (backoff = 1 here).
        assert_eq!(app_duration, handshake_duration + Duration::from_millis(25));
    }

    #[test]
    fn earliest_space_wins_pto() {
        let (mut ld, now) = seeded();
        // Handshake anchored earlier than Application Data.
        ld.set_handshake_confirmed(true);
        ld.registry_mut(PacketNumberSpace::Handshake)
            .on_packet_sent(ack_eliciting(0, now));
        ld.registry_mut(PacketNumberSpace::ApplicationData)
            .on_packet_sent(ack_eliciting(0, now + Duration::from_millis(500)));
        let (_, space) = ld.get_pto_time_and_space(now).expect("pto");
        assert_eq!(space, PacketNumberSpace::Handshake);
    }

    #[test]
    fn discard_space_clears_registry_and_resets_backoff() {
        let (mut ld, now) = seeded();
        ld.registry_mut(PacketNumberSpace::Initial)
            .on_packet_sent(ack_eliciting(0, now));
        // Drive a PTO to bump the backoff.
        ld.on_timeout(now + Duration::from_secs(1));
        assert_eq!(ld.pto_count(), 1);
        ld.discard_space(PacketNumberSpace::Initial);
        assert_eq!(ld.pto_count(), 0);
        assert_eq!(ld.registry(PacketNumberSpace::Initial).outstanding(), 0);
        assert!(!ld.registry(PacketNumberSpace::Initial).ack_eliciting_in_flight());
    }

    #[test]
    fn reset_pto_count_zeroes_backoff() {
        let (mut ld, now) = seeded();
        ld.registry_mut(PacketNumberSpace::Handshake)
            .on_packet_sent(ack_eliciting(0, now));
        ld.on_timeout(now + Duration::from_secs(1));
        assert_eq!(ld.pto_count(), 1);
        ld.reset_pto_count();
        assert_eq!(ld.pto_count(), 0);
    }
}
