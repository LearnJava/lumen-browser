//! QUIC path validation + anti-amplification limit (RFC 9000 §8.1, §8.2).
//!
//! Before an endpoint sends much data to a peer address it has not yet
//! confirmed the peer owns, two related mechanisms protect against off-path
//! attackers using QUIC for traffic amplification:
//!
//! - **Anti-amplification limit** (§8.1): prior to validating the peer's
//!   address, an endpoint MUST NOT send more than three times as many bytes as
//!   it has received from that address. This throttles the handshake until the
//!   address is validated (by completing path validation, by receiving a
//!   Handshake packet, or by a Retry/`NEW_TOKEN` token). [`AntiAmplificationLimit`]
//!   tracks the received/sent byte counts and answers how many more bytes may be
//!   sent.
//! - **Path validation** (§8.2): to confirm a peer can both send from and
//!   receive at a given address — during the handshake and, later, when a peer
//!   migrates to a new path (§9) — an endpoint sends a `PATH_CHALLENGE`
//!   (§19.17) carrying eight bytes of unpredictable data and considers the path
//!   valid once it receives a `PATH_RESPONSE` (§19.18) echoing that exact data.
//!   [`PathValidator`] is the sender-side state machine (validating → validated
//!   or failed); [`respond_to_challenge`] is the trivial receiver-side echo.
//!
//! This is a pure state machine in the same mould as [`lifecycle`](super::lifecycle)
//! and [`loss`](super::loss): the clock is supplied by the caller (`now: Instant`),
//! the eight unpredictable challenge bytes are supplied by the caller (this module
//! generates no randomness — just as the others generate no clock), and the Probe
//! Timeout is supplied as a [`Duration`] (compute it with
//! [`RttEstimator::pto`](super::recovery::RttEstimator::pto)). It performs no IO
//! and owns no timers — it answers *when* the validation deadline falls and *what*
//! frame to send, leaving the actual send/receive and timer arming to the
//! connection layer.
//!
//! ## Out of scope (deferred to later slices)
//!
//! - The UDP send/receive and the actual arming of the validation timer.
//! - The full connection-migration orchestration (RFC 9000 §9): choosing when to
//!   probe a new path, retiring connection IDs across a migration
//!   ([`conn_id`](super::conn_id)), and resetting the congestion controller and
//!   RTT estimator for the new path (§9.4). This slice provides only the path
//!   validation primitive those steps build on.
//! - Padding a `PATH_CHALLENGE`/`PATH_RESPONSE` datagram to
//!   [`MIN_PROBE_DATAGRAM_LEN`] bytes (§8.2.1): the caller assembles the datagram
//!   with [`packet_payload`](super::packet_payload) and this module only exposes
//!   the required minimum size.

use std::time::{Duration, Instant};

use super::datagram::MIN_INITIAL_DATAGRAM_LEN;
use super::quic_frame::{Frame, PATH_DATA_LEN};

/// The multiplier applied to received bytes to bound the bytes an endpoint may
/// send before it validates the peer's address (RFC 9000 §8.1): at most three
/// times as many bytes as have been received.
pub const AMPLIFICATION_FACTOR: u64 = 3;

/// The multiplier applied to the Probe Timeout to bound path validation
/// (RFC 9000 §8.2.4): validation is abandoned after `3·PTO`, where the PTO is the
/// larger of the current PTO and the new path's PTO.
pub const PATH_VALIDATION_PTO_MULTIPLIER: u32 = 3;

/// The minimum size, in bytes, of a datagram carrying a `PATH_CHALLENGE` or
/// `PATH_RESPONSE` frame (RFC 9000 §8.2.1, §14.1): such datagrams are expanded to
/// at least this size to probe the path MTU and to bound amplification. This is
/// the same lower bound as an Initial datagram
/// ([`MIN_INITIAL_DATAGRAM_LEN`](super::datagram::MIN_INITIAL_DATAGRAM_LEN)).
pub const MIN_PROBE_DATAGRAM_LEN: usize = MIN_INITIAL_DATAGRAM_LEN;

/// The anti-amplification limit for one peer address (RFC 9000 §8.1).
///
/// Prior to validating the peer's address, an endpoint MUST NOT send more than
/// [`AMPLIFICATION_FACTOR`] times as many bytes as it has received from that
/// address. This tracks the running received/sent byte totals and, once the
/// address is validated, lifts the limit entirely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AntiAmplificationLimit {
    /// Total bytes received from the (as-yet-unvalidated) peer address.
    received: u64,
    /// Total bytes sent to the peer address while it was unvalidated.
    sent: u64,
    /// Whether the peer's address has been validated, lifting the limit.
    validated: bool,
}

impl Default for AntiAmplificationLimit {
    fn default() -> Self {
        Self::new()
    }
}

impl AntiAmplificationLimit {
    /// Create a fresh limit with no bytes counted and the address unvalidated.
    #[must_use]
    pub fn new() -> Self {
        Self {
            received: 0,
            sent: 0,
            validated: false,
        }
    }

    /// Record `bytes` received from the peer address, raising the send allowance
    /// (RFC 9000 §8.1). Count the size of each received datagram, including any
    /// that fails to decrypt.
    pub fn on_received(&mut self, bytes: u64) {
        self.received = self.received.saturating_add(bytes);
    }

    /// Record `bytes` sent to the peer address, lowering the send allowance
    /// (RFC 9000 §8.1). Count the full datagram size.
    pub fn on_sent(&mut self, bytes: u64) {
        self.sent = self.sent.saturating_add(bytes);
    }

    /// Mark the peer's address as validated, lifting the limit (RFC 9000 §8.1):
    /// an endpoint validates the address by completing path validation, by
    /// successfully processing a Handshake packet, or by receiving a valid token
    /// from a Retry or `NEW_TOKEN` frame.
    pub fn mark_validated(&mut self) {
        self.validated = true;
    }

    /// Whether the peer's address has been validated.
    #[must_use]
    pub fn is_validated(&self) -> bool {
        self.validated
    }

    /// How many more bytes may be sent to the peer address right now
    /// (RFC 9000 §8.1), or `None` once the address is validated and the limit no
    /// longer applies. While unvalidated this is
    /// `AMPLIFICATION_FACTOR·received − sent`, saturating at zero.
    #[must_use]
    pub fn send_allowance(&self) -> Option<u64> {
        if self.validated {
            return None;
        }
        Some(
            self.received
                .saturating_mul(AMPLIFICATION_FACTOR)
                .saturating_sub(self.sent),
        )
    }

    /// Whether a datagram of `bytes` bytes may be sent without exceeding the
    /// limit (RFC 9000 §8.1). Always `true` once the address is validated.
    #[must_use]
    pub fn can_send(&self, bytes: u64) -> bool {
        match self.send_allowance() {
            None => true,
            Some(allowance) => bytes <= allowance,
        }
    }

    /// Whether the endpoint is currently blocked from sending by the limit
    /// (RFC 9000 §8.1): the address is unvalidated and no allowance remains. A
    /// blocked server should arm a timer, since it cannot send probes to elicit
    /// the packets that would raise the allowance (§8.1).
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        !self.validated && self.send_allowance() == Some(0)
    }
}

/// The state of a path validation (RFC 9000 §8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathState {
    /// A `PATH_CHALLENGE` has been sent and a matching `PATH_RESPONSE` is awaited.
    Validating,
    /// A matching `PATH_RESPONSE` arrived; the path is valid (RFC 9000 §8.2.3).
    Validated,
    /// The validation timer expired with no matching response; the path is
    /// abandoned (RFC 9000 §8.2.4).
    Failed,
}

/// The sender-side path validation state machine (RFC 9000 §8.2).
///
/// The caller drives it by sending `PATH_CHALLENGE` frames (with
/// [`send_challenge`](Self::send_challenge)), feeding it received `PATH_RESPONSE`
/// data ([`on_path_response`](Self::on_path_response)), and driving the abandon
/// timer ([`on_timeout`](Self::on_timeout)). Multiple outstanding challenges are
/// retained (RFC 9000 §8.2.1 permits retransmitting a `PATH_CHALLENGE` with fresh
/// data); a `PATH_RESPONSE` matching *any* of them validates the path
/// (RFC 9000 §8.2.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathValidator {
    /// The current validation state.
    state: PathState,
    /// The challenge payloads still awaiting a matching `PATH_RESPONSE`.
    outstanding: Vec<[u8; PATH_DATA_LEN]>,
    /// The instant the validation is abandoned, once a challenge has been sent.
    deadline: Option<Instant>,
}

impl Default for PathValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl PathValidator {
    /// Create a validator with no challenge yet sent, in the
    /// [`Validating`](PathState::Validating) state. Call
    /// [`send_challenge`](Self::send_challenge) to emit the first
    /// `PATH_CHALLENGE`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: PathState::Validating,
            outstanding: Vec::new(),
            deadline: None,
        }
    }

    /// The current validation state.
    #[must_use]
    pub fn state(&self) -> PathState {
        self.state
    }

    /// Whether validation is still in progress.
    #[must_use]
    pub fn is_validating(&self) -> bool {
        self.state == PathState::Validating
    }

    /// Whether the path has been validated (RFC 9000 §8.2.3).
    #[must_use]
    pub fn is_validated(&self) -> bool {
        self.state == PathState::Validated
    }

    /// Whether validation has been abandoned (RFC 9000 §8.2.4).
    #[must_use]
    pub fn is_failed(&self) -> bool {
        self.state == PathState::Failed
    }

    /// The instant path validation is abandoned, or `None` if no challenge is
    /// outstanding. The connection layer arms its timer to this deadline.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Record and return a `PATH_CHALLENGE` carrying `data`, (re)arming the
    /// abandon timer to `now + PATH_VALIDATION_PTO_MULTIPLIER·pto`
    /// (RFC 9000 §8.2.1, §8.2.4). `data` must be eight unpredictable bytes chosen
    /// by the caller; supply a fresh value on each retransmission so a response
    /// cannot be forged from an observed challenge. `pto` should be the larger of
    /// the current PTO and the new path's PTO (§8.2.4).
    ///
    /// Returns `None` without recording anything once validation has completed
    /// (validated or failed): there is nothing left to probe.
    pub fn send_challenge(
        &mut self,
        data: [u8; PATH_DATA_LEN],
        now: Instant,
        pto: Duration,
    ) -> Option<Frame> {
        if self.state != PathState::Validating {
            return None;
        }
        self.outstanding.push(data);
        self.deadline = Some(now + PATH_VALIDATION_PTO_MULTIPLIER * pto);
        Some(Frame::PathChallenge(data))
    }

    /// Process the data from a received `PATH_RESPONSE` (RFC 9000 §8.2.3).
    ///
    /// If validation is in progress and `data` matches any outstanding challenge,
    /// the path transitions to [`Validated`](PathState::Validated), the
    /// outstanding challenges and deadline are cleared, and `true` is returned.
    /// Data that matches nothing, or a response arriving after validation already
    /// completed, leaves the state unchanged and returns `false`. A response
    /// received on any network path validates the path on which the challenge was
    /// sent, so the receiving path is not checked here.
    pub fn on_path_response(&mut self, data: [u8; PATH_DATA_LEN]) -> bool {
        if self.state != PathState::Validating {
            return false;
        }
        if self.outstanding.contains(&data) {
            self.state = PathState::Validated;
            self.outstanding.clear();
            self.deadline = None;
            true
        } else {
            false
        }
    }

    /// Abandon validation if the deadline has been reached (RFC 9000 §8.2.4).
    ///
    /// If validation is in progress and `now` is at or past the deadline, the
    /// path transitions to [`Failed`](PathState::Failed) and `true` is returned;
    /// otherwise the state is unchanged and `false` is returned.
    pub fn on_timeout(&mut self, now: Instant) -> bool {
        if self.state != PathState::Validating {
            return false;
        }
        match self.deadline {
            Some(deadline) if now >= deadline => {
                self.state = PathState::Failed;
                self.outstanding.clear();
                self.deadline = None;
                true
            }
            _ => false,
        }
    }
}

/// Build the `PATH_RESPONSE` that answers a received `PATH_CHALLENGE`
/// (RFC 9000 §8.2.2): an endpoint MUST echo the challenge data verbatim. The
/// caller sends the response on the network path the challenge arrived on and
/// expands its datagram to [`MIN_PROBE_DATAGRAM_LEN`] bytes when able.
#[must_use]
pub fn respond_to_challenge(data: [u8; PATH_DATA_LEN]) -> Frame {
    Frame::PathResponse(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed origin instant, avoiding the forbidden `Instant::now()` littered
    /// across the test body.
    fn origin() -> Instant {
        Instant::now()
    }

    const PTO: Duration = Duration::from_millis(100);

    // ---- Anti-amplification limit (§8.1) ----

    #[test]
    fn amplification_allows_three_times_received() {
        let mut limit = AntiAmplificationLimit::new();
        limit.on_received(1200);
        assert_eq!(limit.send_allowance(), Some(3600));
        assert!(limit.can_send(3600));
        assert!(!limit.can_send(3601));
    }

    #[test]
    fn amplification_subtracts_sent_bytes() {
        let mut limit = AntiAmplificationLimit::new();
        limit.on_received(1000);
        limit.on_sent(2500);
        assert_eq!(limit.send_allowance(), Some(500));
    }

    #[test]
    fn amplification_saturates_at_zero_when_overspent() {
        let mut limit = AntiAmplificationLimit::new();
        limit.on_received(100);
        limit.on_sent(1000);
        assert_eq!(limit.send_allowance(), Some(0));
        assert!(limit.is_blocked());
        assert!(!limit.can_send(1));
    }

    #[test]
    fn amplification_blocked_with_no_bytes_received() {
        let limit = AntiAmplificationLimit::new();
        assert_eq!(limit.send_allowance(), Some(0));
        assert!(limit.is_blocked());
    }

    #[test]
    fn amplification_lifted_once_validated() {
        let mut limit = AntiAmplificationLimit::new();
        limit.on_sent(1_000_000);
        assert!(limit.is_blocked());
        limit.mark_validated();
        assert!(limit.is_validated());
        assert_eq!(limit.send_allowance(), None);
        assert!(!limit.is_blocked());
        assert!(limit.can_send(u64::MAX));
    }

    #[test]
    fn amplification_counts_saturate_without_overflow() {
        let mut limit = AntiAmplificationLimit::new();
        limit.on_received(u64::MAX);
        limit.on_received(u64::MAX);
        // 3·received saturates rather than wrapping.
        assert_eq!(limit.send_allowance(), Some(u64::MAX));
    }

    #[test]
    fn amplification_default_matches_new() {
        assert_eq!(AntiAmplificationLimit::default(), AntiAmplificationLimit::new());
    }

    // ---- Path validation sender side (§8.2) ----

    #[test]
    fn new_validator_is_validating_with_no_deadline() {
        let v = PathValidator::new();
        assert_eq!(v.state(), PathState::Validating);
        assert!(v.is_validating());
        assert_eq!(v.deadline(), None);
    }

    #[test]
    fn send_challenge_emits_frame_and_arms_deadline() {
        let t0 = origin();
        let mut v = PathValidator::new();
        let frame = v.send_challenge([1, 2, 3, 4, 5, 6, 7, 8], t0, PTO);
        assert_eq!(frame, Some(Frame::PathChallenge([1, 2, 3, 4, 5, 6, 7, 8])));
        assert_eq!(v.deadline(), Some(t0 + 3 * PTO));
        assert!(v.is_validating());
    }

    #[test]
    fn matching_response_validates_the_path() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        assert!(v.on_path_response([9; PATH_DATA_LEN]));
        assert!(v.is_validated());
        assert_eq!(v.deadline(), None);
    }

    #[test]
    fn non_matching_response_is_ignored() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        assert!(!v.on_path_response([7; PATH_DATA_LEN]));
        assert!(v.is_validating());
        assert_eq!(v.deadline(), Some(t0 + 3 * PTO));
    }

    #[test]
    fn response_matches_any_outstanding_challenge() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([1; PATH_DATA_LEN], t0, PTO);
        // Retransmit with fresh data (§8.2.1); the deadline re-arms.
        let frame = v.send_challenge([2; PATH_DATA_LEN], t0 + PTO, PTO);
        assert_eq!(frame, Some(Frame::PathChallenge([2; PATH_DATA_LEN])));
        assert_eq!(v.deadline(), Some(t0 + PTO + 3 * PTO));
        // A response echoing the *first* challenge still validates the path.
        assert!(v.on_path_response([1; PATH_DATA_LEN]));
        assert!(v.is_validated());
    }

    #[test]
    fn timeout_before_deadline_does_nothing() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        assert!(!v.on_timeout(t0 + 2 * PTO));
        assert!(v.is_validating());
    }

    #[test]
    fn timeout_at_deadline_fails_validation() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        assert!(v.on_timeout(t0 + 3 * PTO));
        assert!(v.is_failed());
        assert_eq!(v.deadline(), None);
    }

    #[test]
    fn response_after_failure_is_ignored() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        v.on_timeout(t0 + 3 * PTO);
        assert!(!v.on_path_response([9; PATH_DATA_LEN]));
        assert!(v.is_failed());
    }

    #[test]
    fn send_challenge_after_completion_returns_none() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        v.on_path_response([9; PATH_DATA_LEN]);
        assert_eq!(v.send_challenge([1; PATH_DATA_LEN], t0, PTO), None);
        assert!(v.is_validated());
    }

    #[test]
    fn response_after_validation_returns_false() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        v.on_path_response([9; PATH_DATA_LEN]);
        assert!(!v.on_path_response([9; PATH_DATA_LEN]));
        assert!(v.is_validated());
    }

    #[test]
    fn timeout_after_validation_does_nothing() {
        let t0 = origin();
        let mut v = PathValidator::new();
        v.send_challenge([9; PATH_DATA_LEN], t0, PTO);
        v.on_path_response([9; PATH_DATA_LEN]);
        assert!(!v.on_timeout(t0 + 100 * PTO));
        assert!(v.is_validated());
    }

    #[test]
    fn validator_default_matches_new() {
        assert_eq!(PathValidator::default(), PathValidator::new());
    }

    // ---- Path validation receiver side (§8.2.2) ----

    #[test]
    fn respond_echoes_challenge_data() {
        let data = [4, 8, 15, 16, 23, 42, 1, 2];
        assert_eq!(respond_to_challenge(data), Frame::PathResponse(data));
    }

    #[test]
    fn probe_datagram_minimum_matches_initial() {
        assert_eq!(MIN_PROBE_DATAGRAM_LEN, MIN_INITIAL_DATAGRAM_LEN);
        assert_eq!(MIN_PROBE_DATAGRAM_LEN, 1200);
    }
}
