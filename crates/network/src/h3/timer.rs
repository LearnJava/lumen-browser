//! QUIC connection timer scheduler (RFC 9000 §8.2.4, §10.1, §13.2.1; RFC 9002 §6.2).
//!
//! Every earlier transport slice that has a *deadline* exposes it as its own
//! `Option<Instant>`: the loss-detection / PTO timer
//! ([`pto::LossDetection::set_loss_detection_timer`](super::pto::LossDetection::set_loss_detection_timer),
//! RFC 9002 §6.2), the delayed-acknowledgement timer per packet-number space
//! ([`ack::AckGenerator::ack_urgency`](super::ack::AckGenerator::ack_urgency),
//! RFC 9000 §13.2.1), the connection idle timeout
//! ([`lifecycle::ConnectionLifecycle::idle_deadline`](super::lifecycle::ConnectionLifecycle::idle_deadline),
//! RFC 9000 §10.1), the path-validation timeout
//! ([`path_validation::PathValidator::deadline`](super::path_validation::PathValidator::deadline),
//! RFC 9000 §8.2.4), and the closing/draining period
//! ([`lifecycle::ConnectionLifecycle::close_deadline`](super::lifecycle::ConnectionLifecycle::close_deadline),
//! RFC 9000 §10.2).
//!
//! A QUIC endpoint drives an *event loop* around a single OS timer: each turn it
//! re-reads every state machine's deadline, arms one timer for the earliest of
//! them, and — when that timer fires — asks each machine whether *its* deadline
//! has elapsed and drives the ones that have. [`ConnectionTimers`] is the pure
//! multiplexer for that loop: the caller feeds it each machine's current deadline
//! and it answers the two questions the loop needs —
//!
//! - [`ConnectionTimers::next`] — the single earliest deadline and which timer
//!   owns it, i.e. exactly when and why to wake the socket loop next. This is the
//!   value the caller hands to the OS timer / async runtime.
//! - [`ConnectionTimers::fired`] — after waking at `now`, every timer whose
//!   deadline has elapsed, ordered earliest-first, so the caller can dispatch each
//!   one (`LossDetection::on_timeout`, `AckGenerator::generate_ack_frame`,
//!   `ConnectionLifecycle::is_idle_expired`, `PathValidator::on_timeout`, …).
//!
//! The scheduler holds no cross-timer policy: it reports precisely the deadlines
//! it was given. When the connection changes state (enters closing, discards a
//! packet-number space, validates a path) the owning connection machine cancels
//! the now-irrelevant deadlines by setting them to `None`; the scheduler simply
//! stops reporting them. It is pure — no clock of its own, no IO, no timer
//! objects — mirroring every other slice: the caller supplies `now` and owns the
//! actual OS timer.

use std::time::Instant;

use super::ack::AckUrgency;
use super::loss::PacketNumberSpace;
use super::pto::LossTimer;

/// The number of packet-number spaces that carry an independent delayed-ACK
/// timer (Initial, Handshake, Application Data — RFC 9000 §12.3).
pub const SPACE_COUNT: usize = 3;

/// Index of a packet-number space into the per-space delayed-ACK timer array.
fn space_index(space: PacketNumberSpace) -> usize {
    match space {
        PacketNumberSpace::Initial => 0,
        PacketNumberSpace::Handshake => 1,
        PacketNumberSpace::ApplicationData => 2,
    }
}

/// The packet-number space at the given delayed-ACK timer array index.
fn space_at(index: usize) -> PacketNumberSpace {
    match index {
        0 => PacketNumberSpace::Initial,
        1 => PacketNumberSpace::Handshake,
        _ => PacketNumberSpace::ApplicationData,
    }
}

/// Which of the connection's timers a deadline belongs to.
///
/// Returned by [`ConnectionTimers::next`] and [`ConnectionTimers::fired`] so the
/// caller knows which state machine to drive when a wake-up happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
    /// The loss-detection / PTO timer (RFC 9002 §6.2). Fire it into
    /// [`pto::LossDetection::on_timeout`](super::pto::LossDetection::on_timeout).
    LossDetection,
    /// The delayed-acknowledgement timer for a packet-number space
    /// (RFC 9000 §13.2.1). When it fires the caller emits the pending ACK via
    /// [`ack::AckGenerator::generate_ack_frame`](super::ack::AckGenerator::generate_ack_frame).
    AckDelay(PacketNumberSpace),
    /// The connection idle timeout (RFC 9000 §10.1). When it fires the connection
    /// is silently closed
    /// ([`lifecycle::ConnectionLifecycle::is_idle_expired`](super::lifecycle::ConnectionLifecycle::is_idle_expired)).
    IdleTimeout,
    /// The path-validation timeout (RFC 9000 §8.2.4). Fire it into
    /// [`path_validation::PathValidator::on_timeout`](super::path_validation::PathValidator::on_timeout).
    PathValidation,
    /// The closing / draining period end (RFC 9000 §10.2). When it fires the
    /// connection state is discarded
    /// ([`lifecycle::ConnectionLifecycle::discard`](super::lifecycle::ConnectionLifecycle::discard)).
    DrainingClose,
}

impl TimerKind {
    /// The tie-break priority when two timers share the same deadline: the lower
    /// value is reported first. Loss recovery is the most time-critical, then the
    /// per-space ACK timers (Initial before Handshake before Application Data),
    /// then path validation, then the two connection-lifetime timers. The order
    /// only affects the reporting sequence — every elapsed timer is still
    /// returned by [`ConnectionTimers::fired`].
    fn priority(self) -> u8 {
        match self {
            TimerKind::LossDetection => 0,
            TimerKind::AckDelay(space) => 1 + space_index(space) as u8,
            TimerKind::PathValidation => 4,
            TimerKind::IdleTimeout => 5,
            TimerKind::DrainingClose => 6,
        }
    }
}

/// A single armed timer: the instant it is due and which timer it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmedTimer {
    /// Which timer this deadline belongs to.
    pub kind: TimerKind,
    /// The instant the timer is due to fire.
    pub deadline: Instant,
}

/// The multiplexer over a connection's individual timer deadlines
/// (RFC 9000 §8.2.4, §10.1, §13.2.1; RFC 9002 §6.2).
///
/// The caller refreshes each field every event-loop turn from the owning state
/// machine, then reads [`ConnectionTimers::next`] to arm one OS timer and, on
/// wake-up, [`ConnectionTimers::fired`] to learn which machines to drive. All
/// deadlines default to disarmed; a timer that is not set is simply never
/// reported.
#[derive(Debug, Clone, Default)]
pub struct ConnectionTimers {
    /// The loss-detection / PTO deadline, or `None` when the loss timer is
    /// disarmed.
    loss_detection: Option<Instant>,
    /// The delayed-ACK deadline per packet-number space, indexed by
    /// [`space_index`].
    ack_delay: [Option<Instant>; SPACE_COUNT],
    /// The connection idle-timeout deadline.
    idle_timeout: Option<Instant>,
    /// The path-validation deadline.
    path_validation: Option<Instant>,
    /// The closing / draining period end.
    draining_close: Option<Instant>,
}

impl ConnectionTimers {
    /// A scheduler with every timer disarmed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (or, on [`LossTimer::Disarmed`], clear) the loss-detection / PTO timer
    /// from the value [`pto::LossDetection::set_loss_detection_timer`](super::pto::LossDetection::set_loss_detection_timer)
    /// returned.
    pub fn set_loss_detection(&mut self, timer: LossTimer) {
        self.loss_detection = timer.deadline();
    }

    /// Arm (or clear) a packet-number space's delayed-ACK timer from the value
    /// [`ack::AckGenerator::ack_urgency`](super::ack::AckGenerator::ack_urgency)
    /// returned. Only [`AckUrgency::Delayed`] arms a timer:
    /// [`AckUrgency::Immediate`] means the ACK is owed *now* (the caller sends it
    /// without waiting, so no timer), and [`AckUrgency::None`] means no ACK is
    /// owed at all.
    pub fn set_ack_delay(&mut self, space: PacketNumberSpace, urgency: AckUrgency) {
        self.ack_delay[space_index(space)] = match urgency {
            AckUrgency::Delayed(deadline) => Some(deadline),
            AckUrgency::Immediate | AckUrgency::None => None,
        };
    }

    /// Set the connection idle-timeout deadline (RFC 9000 §10.1) from
    /// [`lifecycle::ConnectionLifecycle::idle_deadline`](super::lifecycle::ConnectionLifecycle::idle_deadline).
    pub fn set_idle_timeout(&mut self, deadline: Option<Instant>) {
        self.idle_timeout = deadline;
    }

    /// Set the path-validation deadline (RFC 9000 §8.2.4) from
    /// [`path_validation::PathValidator::deadline`](super::path_validation::PathValidator::deadline).
    pub fn set_path_validation(&mut self, deadline: Option<Instant>) {
        self.path_validation = deadline;
    }

    /// Set the closing / draining period end (RFC 9000 §10.2) from
    /// [`lifecycle::ConnectionLifecycle::close_deadline`](super::lifecycle::ConnectionLifecycle::close_deadline).
    pub fn set_draining_close(&mut self, deadline: Option<Instant>) {
        self.draining_close = deadline;
    }

    /// Disarm every timer. Useful once the connection has reached
    /// [`ConnState::Closed`](super::lifecycle::ConnState::Closed) and no further
    /// wake-ups are wanted.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// The loss-detection / PTO deadline, if armed.
    pub fn loss_detection_deadline(&self) -> Option<Instant> {
        self.loss_detection
    }

    /// The delayed-ACK deadline for a packet-number space, if armed.
    pub fn ack_delay_deadline(&self, space: PacketNumberSpace) -> Option<Instant> {
        self.ack_delay[space_index(space)]
    }

    /// The connection idle-timeout deadline, if armed.
    pub fn idle_timeout_deadline(&self) -> Option<Instant> {
        self.idle_timeout
    }

    /// The path-validation deadline, if armed.
    pub fn path_validation_deadline(&self) -> Option<Instant> {
        self.path_validation
    }

    /// The closing / draining period end, if armed.
    pub fn draining_close_deadline(&self) -> Option<Instant> {
        self.draining_close
    }

    /// Push every armed timer as an [`ArmedTimer`] onto `out`.
    fn collect(&self, out: &mut Vec<ArmedTimer>) {
        if let Some(deadline) = self.loss_detection {
            out.push(ArmedTimer {
                kind: TimerKind::LossDetection,
                deadline,
            });
        }
        for (index, slot) in self.ack_delay.iter().enumerate() {
            if let Some(deadline) = *slot {
                out.push(ArmedTimer {
                    kind: TimerKind::AckDelay(space_at(index)),
                    deadline,
                });
            }
        }
        if let Some(deadline) = self.idle_timeout {
            out.push(ArmedTimer {
                kind: TimerKind::IdleTimeout,
                deadline,
            });
        }
        if let Some(deadline) = self.path_validation {
            out.push(ArmedTimer {
                kind: TimerKind::PathValidation,
                deadline,
            });
        }
        if let Some(deadline) = self.draining_close {
            out.push(ArmedTimer {
                kind: TimerKind::DrainingClose,
                deadline,
            });
        }
    }

    /// Whether any timer is currently armed.
    pub fn is_armed(&self) -> bool {
        self.loss_detection.is_some()
            || self.ack_delay.iter().any(Option::is_some)
            || self.idle_timeout.is_some()
            || self.path_validation.is_some()
            || self.draining_close.is_some()
    }

    /// The single earliest armed timer — the deadline the caller arms one OS
    /// timer for — or `None` when nothing is armed. Ties are broken by
    /// [`TimerKind::priority`] so the result is deterministic.
    pub fn next(&self) -> Option<ArmedTimer> {
        let mut armed = Vec::new();
        self.collect(&mut armed);
        armed
            .into_iter()
            .min_by(|a, b| {
                a.deadline
                    .cmp(&b.deadline)
                    .then_with(|| a.kind.priority().cmp(&b.kind.priority()))
            })
    }

    /// Every timer whose deadline is at or before `now`, ordered earliest-first
    /// (ties broken by [`TimerKind::priority`]). The caller drives each returned
    /// timer's owning state machine; a timer still armed for a later instant is
    /// not returned.
    pub fn fired(&self, now: Instant) -> Vec<TimerKind> {
        let mut armed = Vec::new();
        self.collect(&mut armed);
        armed.retain(|t| t.deadline <= now);
        armed.sort_by(|a, b| {
            a.deadline
                .cmp(&b.deadline)
                .then_with(|| a.kind.priority().cmp(&b.kind.priority()))
        });
        armed.into_iter().map(|t| t.kind).collect()
    }

    /// Whether any timer's deadline is at or before `now`.
    pub fn has_fired(&self, now: Instant) -> bool {
        self.loss_detection.is_some_and(|d| d <= now)
            || self.ack_delay.iter().any(|s| s.is_some_and(|d| d <= now))
            || self.idle_timeout.is_some_and(|d| d <= now)
            || self.path_validation.is_some_and(|d| d <= now)
            || self.draining_close.is_some_and(|d| d <= now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A fixed base instant to offset every deadline from — the clock is supplied
    /// by the caller, so the tests build every `Instant` from one base.
    fn base() -> Instant {
        Instant::now()
    }

    #[test]
    fn empty_scheduler_reports_nothing() {
        let timers = ConnectionTimers::new();
        assert!(!timers.is_armed());
        assert_eq!(timers.next(), None);
        assert!(timers.fired(base()).is_empty());
        assert!(!timers.has_fired(base()));
    }

    #[test]
    fn loss_timer_armed_and_disarmed() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(50)));
        assert!(timers.is_armed());
        assert_eq!(
            timers.next(),
            Some(ArmedTimer {
                kind: TimerKind::LossDetection,
                deadline: now + Duration::from_millis(50),
            })
        );
        timers.set_loss_detection(LossTimer::Disarmed);
        assert!(!timers.is_armed());
        assert_eq!(timers.next(), None);
    }

    #[test]
    fn ack_delay_only_arms_on_delayed() {
        let now = base();
        let mut timers = ConnectionTimers::new();

        timers.set_ack_delay(
            PacketNumberSpace::ApplicationData,
            AckUrgency::Delayed(now + Duration::from_millis(25)),
        );
        assert_eq!(
            timers.ack_delay_deadline(PacketNumberSpace::ApplicationData),
            Some(now + Duration::from_millis(25))
        );

        // Immediate means "send now", not a future timer.
        timers.set_ack_delay(PacketNumberSpace::ApplicationData, AckUrgency::Immediate);
        assert_eq!(
            timers.ack_delay_deadline(PacketNumberSpace::ApplicationData),
            None
        );

        timers.set_ack_delay(
            PacketNumberSpace::ApplicationData,
            AckUrgency::Delayed(now + Duration::from_millis(25)),
        );
        timers.set_ack_delay(PacketNumberSpace::ApplicationData, AckUrgency::None);
        assert_eq!(
            timers.ack_delay_deadline(PacketNumberSpace::ApplicationData),
            None
        );
    }

    #[test]
    fn per_space_ack_timers_are_independent() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_ack_delay(
            PacketNumberSpace::Initial,
            AckUrgency::Delayed(now + Duration::from_millis(10)),
        );
        timers.set_ack_delay(
            PacketNumberSpace::Handshake,
            AckUrgency::Delayed(now + Duration::from_millis(20)),
        );
        assert_eq!(
            timers.ack_delay_deadline(PacketNumberSpace::Initial),
            Some(now + Duration::from_millis(10))
        );
        assert_eq!(
            timers.ack_delay_deadline(PacketNumberSpace::Handshake),
            Some(now + Duration::from_millis(20))
        );
        assert_eq!(
            timers.ack_delay_deadline(PacketNumberSpace::ApplicationData),
            None
        );
    }

    #[test]
    fn next_picks_earliest_deadline() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(100)));
        timers.set_idle_timeout(Some(now + Duration::from_millis(40)));
        timers.set_path_validation(Some(now + Duration::from_millis(70)));

        let next = timers.next().expect("armed");
        assert_eq!(next.kind, TimerKind::IdleTimeout);
        assert_eq!(next.deadline, now + Duration::from_millis(40));
    }

    #[test]
    fn next_breaks_ties_by_priority() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        let deadline = now + Duration::from_millis(30);
        // Idle timeout and loss detection share the same instant; loss detection
        // has the higher priority and must win.
        timers.set_idle_timeout(Some(deadline));
        timers.set_loss_detection(LossTimer::Armed(deadline));
        let next = timers.next().expect("armed");
        assert_eq!(next.kind, TimerKind::LossDetection);
        assert_eq!(next.deadline, deadline);
    }

    #[test]
    fn fired_returns_only_elapsed_timers_earliest_first() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(10)));
        timers.set_idle_timeout(Some(now + Duration::from_millis(5)));
        timers.set_path_validation(Some(now + Duration::from_millis(50)));

        let wake = now + Duration::from_millis(20);
        let fired = timers.fired(wake);
        // Idle (5ms) fired before loss (10ms); path validation (50ms) not yet.
        assert_eq!(fired, vec![TimerKind::IdleTimeout, TimerKind::LossDetection]);
        assert!(timers.has_fired(wake));
    }

    #[test]
    fn fired_ties_ordered_by_priority() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        let deadline = now + Duration::from_millis(10);
        timers.set_draining_close(Some(deadline));
        timers.set_ack_delay(
            PacketNumberSpace::Initial,
            AckUrgency::Delayed(deadline),
        );
        timers.set_loss_detection(LossTimer::Armed(deadline));

        let fired = timers.fired(now + Duration::from_millis(15));
        assert_eq!(
            fired,
            vec![
                TimerKind::LossDetection,
                TimerKind::AckDelay(PacketNumberSpace::Initial),
                TimerKind::DrainingClose,
            ]
        );
    }

    #[test]
    fn fired_excludes_future_deadlines() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(100)));
        assert!(timers.fired(now).is_empty());
        assert!(!timers.has_fired(now));
    }

    #[test]
    fn deadline_exactly_at_now_has_fired() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_idle_timeout(Some(now));
        assert!(timers.has_fired(now));
        assert_eq!(timers.fired(now), vec![TimerKind::IdleTimeout]);
    }

    #[test]
    fn clear_disarms_everything() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(10)));
        timers.set_idle_timeout(Some(now + Duration::from_millis(20)));
        timers.set_ack_delay(
            PacketNumberSpace::ApplicationData,
            AckUrgency::Delayed(now + Duration::from_millis(30)),
        );
        assert!(timers.is_armed());
        timers.clear();
        assert!(!timers.is_armed());
        assert_eq!(timers.next(), None);
    }

    #[test]
    fn all_timer_kinds_can_be_the_next() {
        let now = base();
        // Idle timeout as the sole armed timer.
        let mut timers = ConnectionTimers::new();
        timers.set_path_validation(Some(now + Duration::from_millis(15)));
        assert_eq!(timers.next().map(|t| t.kind), Some(TimerKind::PathValidation));

        timers.clear();
        timers.set_draining_close(Some(now + Duration::from_millis(15)));
        assert_eq!(timers.next().map(|t| t.kind), Some(TimerKind::DrainingClose));

        timers.clear();
        timers.set_ack_delay(
            PacketNumberSpace::Handshake,
            AckUrgency::Delayed(now + Duration::from_millis(15)),
        );
        assert_eq!(
            timers.next().map(|t| t.kind),
            Some(TimerKind::AckDelay(PacketNumberSpace::Handshake))
        );
    }

    #[test]
    fn re_reading_a_deadline_replaces_it() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_idle_timeout(Some(now + Duration::from_millis(10)));
        // A later event-loop turn recomputes a fresh, later idle deadline.
        timers.set_idle_timeout(Some(now + Duration::from_millis(60)));
        assert_eq!(
            timers.idle_timeout_deadline(),
            Some(now + Duration::from_millis(60))
        );
        timers.set_idle_timeout(None);
        assert_eq!(timers.idle_timeout_deadline(), None);
    }

    #[test]
    fn priority_ordering_is_total_and_distinct_per_kind() {
        // Sanity-check the tie-break order used by `next` / `fired`.
        assert!(TimerKind::LossDetection.priority() < TimerKind::PathValidation.priority());
        assert!(TimerKind::PathValidation.priority() < TimerKind::IdleTimeout.priority());
        assert!(TimerKind::IdleTimeout.priority() < TimerKind::DrainingClose.priority());
        assert!(
            TimerKind::AckDelay(PacketNumberSpace::Initial).priority()
                < TimerKind::AckDelay(PacketNumberSpace::Handshake).priority()
        );
        assert!(
            TimerKind::AckDelay(PacketNumberSpace::Handshake).priority()
                < TimerKind::AckDelay(PacketNumberSpace::ApplicationData).priority()
        );
    }

    #[test]
    fn full_house_reports_earliest_across_all_sources() {
        let now = base();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(80)));
        timers.set_ack_delay(
            PacketNumberSpace::ApplicationData,
            AckUrgency::Delayed(now + Duration::from_millis(25)),
        );
        timers.set_idle_timeout(Some(now + Duration::from_millis(90)));
        timers.set_path_validation(Some(now + Duration::from_millis(60)));
        timers.set_draining_close(Some(now + Duration::from_millis(70)));

        let next = timers.next().expect("armed");
        assert_eq!(next.kind, TimerKind::AckDelay(PacketNumberSpace::ApplicationData));
        assert_eq!(next.deadline, now + Duration::from_millis(25));
    }
}
