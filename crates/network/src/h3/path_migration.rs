//! QUIC connection migration (RFC 9000 §9).
//!
//! A QUIC connection is not bound to a single network path: it survives an
//! endpoint changing its local address (a client moving from Wi-Fi to cellular)
//! or its peer's address changing (a server migrating to its preferred address,
//! RFC 9000 §9.6, or a NAT rebinding the peer's port). Migration reuses the two
//! primitives of the previous slice — path validation and the anti-amplification
//! limit ([`path_validation`](super::path_validation)) — and adds the
//! orchestration that ties them to connection-ID management
//! ([`conn_id`](super::conn_id)) and loss recovery ([`recovery`](super::recovery)):
//!
//! - **Probing vs non-probing frames** (§9.1): a packet carrying only
//!   `PATH_CHALLENGE`, `PATH_RESPONSE`, `NEW_CONNECTION_ID`, and `PADDING` frames
//!   is a *probing packet* — it exercises a path without committing the
//!   connection to it. Any other frame makes the packet *non-probing*, and a
//!   non-probing packet received from a new address is what actually initiates a
//!   peer migration. [`is_probing_frame`] / [`is_probing_packet`] classify them.
//! - **Initiating migration** (§9.2): an endpoint MUST NOT migrate before the
//!   handshake is confirmed, and MUST use a connection ID not previously used on
//!   the new path (§9.5). [`ConnectionMigration::initiate`] gates on the
//!   confirmed handshake, records the fresh peer connection-ID sequence the
//!   caller obtained from [`conn_id::RemoteConnIds`](super::conn_id::RemoteConnIds),
//!   and starts validating the new path with a `PATH_CHALLENGE`.
//! - **New-path anti-amplification** (§9.3): while the peer's address on the new
//!   path is unvalidated, sending to it is bounded to three times the bytes
//!   received (§8.1), just like during the handshake. The migration state machine
//!   holds an [`AntiAmplificationLimit`](super::path_validation::AntiAmplificationLimit)
//!   for the new path when the peer's address there is not yet validated.
//! - **Loss detection and congestion control** (§9.4): on confirming the peer
//!   owns its new address, the congestion controller and RTT estimator MUST be
//!   reset to their initial values, *unless* the only change is the peer's port
//!   number (a likely NAT rebinding, whose path capacity is presumed unchanged).
//!   [`MigrationOutcome::reset_congestion_and_rtt`] tells the caller which.
//!
//! Like [`path_validation`](super::path_validation) and
//! [`lifecycle`](super::lifecycle), this is a pure state machine: the clock and
//! Probe Timeout are supplied by the caller, no randomness is generated (the
//! eight challenge bytes are supplied), and it performs no IO. It answers *what*
//! frame to send, *when* validation lapses, and *what* to reset on success,
//! leaving the actual send/receive, timer arming, connection-ID retirement
//! ([`conn_id::RemoteConnIds::switch_to`](super::conn_id::RemoteConnIds::switch_to)),
//! and congestion-state reset to the connection layer.
//!
//! ## Out of scope (deferred to later slices)
//!
//! - The UDP send/receive, the actual arming of the validation timer, and the
//!   assembly and padding of probe datagrams to
//!   [`MIN_PROBE_DATAGRAM_LEN`](super::path_validation::MIN_PROBE_DATAGRAM_LEN)
//!   (§8.2.1) — the caller drives those with
//!   [`packet_payload`](super::packet_payload).
//! - Detecting a peer migration from the arrival of a non-probing packet on a new
//!   4-tuple and simultaneous multi-path use: this slice models the local
//!   decision to migrate and the validation of one new path at a time.

use std::time::{Duration, Instant};

use super::path_validation::{AntiAmplificationLimit, PathState, PathValidator};
use super::quic_frame::{Frame, PATH_DATA_LEN};

/// Whether `frame` is a *probing frame* (RFC 9000 §9.1). A packet built only from
/// probing frames is a probing packet: it can be sent on and validate a path
/// without committing the connection to it. Everything else is a non-probing
/// frame, and a packet containing one is a non-probing packet whose arrival on a
/// new path initiates a peer migration.
#[must_use]
pub fn is_probing_frame(frame: &Frame) -> bool {
    matches!(
        frame,
        Frame::PathChallenge(_)
            | Frame::PathResponse(_)
            | Frame::NewConnectionId { .. }
            | Frame::Padding(_)
    )
}

/// Whether the frames of a packet make it a *probing packet* (RFC 9000 §9.1):
/// there is at least one frame and every frame is a [probing
/// frame](is_probing_frame). An empty frame set is not a valid QUIC packet and is
/// reported as non-probing.
#[must_use]
pub fn is_probing_packet<'a, I>(frames: I) -> bool
where
    I: IntoIterator<Item = &'a Frame>,
{
    let mut any = false;
    for frame in frames {
        any = true;
        if !is_probing_frame(frame) {
            return false;
        }
    }
    any
}

/// A description of the new path a migration targets, controlling the two
/// path-dependent behaviours (RFC 9000 §9.3, §9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewPath {
    /// Whether the peer's address on the new path is already validated. When
    /// `false`, the anti-amplification limit (RFC 9000 §8.1) applies to the new
    /// path until path validation completes (RFC 9000 §9.3). A client that
    /// migrates its own local address while keeping the same, already-validated
    /// peer address passes `true`; validating a peer's *new* address (its
    /// preferred address, RFC 9000 §9.6, or an observed rebinding) passes `false`.
    pub peer_address_validated: bool,
    /// Whether the only change to the peer's address is its port number (e.g. a
    /// NAT rebinding). When `true` the congestion controller and RTT estimator
    /// are retained across the migration; otherwise they are reset to their
    /// initial values on success (RFC 9000 §9.4).
    pub port_only: bool,
}

impl NewPath {
    /// The descriptor for a client migrating its own local address to a fresh
    /// path while keeping the same, already-validated peer address: the peer
    /// address needs no anti-amplification limit, and the new path's capacity is
    /// unknown so congestion/RTT state is reset on success (RFC 9000 §9.2, §9.4).
    #[must_use]
    pub const fn local_migration() -> Self {
        Self {
            peer_address_validated: true,
            port_only: false,
        }
    }

    /// The descriptor for a suspected NAT rebinding of the peer's port: the new
    /// peer address is unvalidated (anti-amplification applies) but is presumed
    /// to share the old path's capacity, so congestion/RTT state is retained
    /// (RFC 9000 §9.3, §9.4).
    #[must_use]
    pub const fn peer_rebinding() -> Self {
        Self {
            peer_address_validated: false,
            port_only: true,
        }
    }
}

/// Why a migration could not be initiated (RFC 9000 §9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationError {
    /// Migration was initiated before the handshake was confirmed. An endpoint
    /// MUST NOT initiate connection migration before the handshake is confirmed
    /// (RFC 9000 §9).
    HandshakeNotConfirmed,
    /// A migration to a new path is already in progress. This state machine
    /// validates one new path at a time; complete or abandon the current
    /// migration before starting another.
    AlreadyMigrating,
}

impl core::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::HandshakeNotConfirmed => {
                write!(f, "QUIC migration: cannot migrate before the handshake is confirmed")
            }
            Self::AlreadyMigrating => {
                write!(f, "QUIC migration: a migration to a new path is already in progress")
            }
        }
    }
}

impl std::error::Error for MigrationError {}

/// The state of the connection's migration machinery (RFC 9000 §9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationState {
    /// Not migrating: the connection is using a single validated path.
    Stable,
    /// A migration is under way — the new path is being validated while the old
    /// path remains usable (RFC 9000 §9.3). Resolves to [`Stable`](Self::Stable)
    /// on the new path (validation succeeded) or on the old path (validation
    /// lapsed).
    Probing,
}

/// The result of a migration completing successfully (RFC 9000 §9.4, §9.5). The
/// connection layer applies these when [`ConnectionMigration::on_path_response`]
/// validates the new path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// The sequence number of the peer connection ID now active on the
    /// migrated-to path (RFC 9000 §9.5). The caller switches its
    /// [`RemoteConnIds`](super::conn_id::RemoteConnIds) to this ID (retiring the
    /// old one) with
    /// [`switch_to`](super::conn_id::RemoteConnIds::switch_to).
    pub active_cid_seq: u64,
    /// Whether the congestion controller and RTT estimator MUST be reset to their
    /// initial values for the new path (RFC 9000 §9.4). `false` only when the
    /// migration was a port-only peer change, which retains the send rate.
    pub reset_congestion_and_rtt: bool,
}

/// The connection migration state machine (RFC 9000 §9).
///
/// Driven by the caller: [`confirm_handshake`](Self::confirm_handshake) once the
/// handshake completes, [`initiate`](Self::initiate) to start migrating to a new
/// path, [`on_received`](Self::on_received) / [`on_sent`](Self::on_sent) to feed
/// the new path's anti-amplification limit, [`on_path_response`](Self::on_path_response)
/// for a received `PATH_RESPONSE`, and [`on_timeout`](Self::on_timeout) for the
/// validation deadline. It composes [`PathValidator`] and
/// [`AntiAmplificationLimit`] and reports what the caller must do to
/// [`conn_id`](super::conn_id) and [`recovery`](super::recovery) on success.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionMigration {
    /// The current migration state.
    state: MigrationState,
    /// Whether the handshake is confirmed; migration is forbidden before it
    /// (RFC 9000 §9).
    handshake_confirmed: bool,
    /// The validator for the path being migrated to, present only while
    /// [`Probing`](MigrationState::Probing).
    validator: Option<PathValidator>,
    /// The anti-amplification limit for the new path, present only while probing
    /// a path whose peer address is not yet validated (RFC 9000 §9.3).
    anti_amplification: Option<AntiAmplificationLimit>,
    /// The peer connection-ID sequence to stamp on the new path (RFC 9000 §9.5),
    /// present only while probing.
    new_cid_seq: Option<u64>,
    /// Whether a successful migration resets congestion/RTT state (RFC 9000 §9.4),
    /// captured from the [`NewPath`] at [`initiate`](Self::initiate) time.
    reset_on_success: bool,
}

impl Default for ConnectionMigration {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionMigration {
    /// Create a migration state machine in the [`Stable`](MigrationState::Stable)
    /// state with the handshake not yet confirmed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: MigrationState::Stable,
            handshake_confirmed: false,
            validator: None,
            anti_amplification: None,
            new_cid_seq: None,
            reset_on_success: false,
        }
    }

    /// Mark the handshake as confirmed (RFC 9000 §9), permitting migration.
    pub fn confirm_handshake(&mut self) {
        self.handshake_confirmed = true;
    }

    /// Whether the handshake has been confirmed.
    #[must_use]
    pub fn is_handshake_confirmed(&self) -> bool {
        self.handshake_confirmed
    }

    /// The current migration state.
    #[must_use]
    pub fn state(&self) -> MigrationState {
        self.state
    }

    /// Whether a migration to a new path is in progress (RFC 9000 §9.3).
    #[must_use]
    pub fn is_migrating(&self) -> bool {
        self.state == MigrationState::Probing
    }

    /// The state of the new-path validation, or `None` when not migrating.
    #[must_use]
    pub fn probe_state(&self) -> Option<PathState> {
        self.validator.as_ref().map(PathValidator::state)
    }

    /// The peer connection-ID sequence in use on the path being probed
    /// (RFC 9000 §9.5), or `None` when not migrating.
    #[must_use]
    pub fn probing_cid_seq(&self) -> Option<u64> {
        self.new_cid_seq
    }

    /// The instant the in-progress migration's validation is abandoned, or `None`
    /// when not migrating or before a challenge has been sent. The connection
    /// layer arms its timer to this deadline.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.validator.as_ref().and_then(PathValidator::deadline)
    }

    /// Begin migrating to a new path (RFC 9000 §9.2), emitting the first
    /// `PATH_CHALLENGE` carrying `data` and arming the abandon timer to
    /// `now + 3·pto` (RFC 9000 §8.2.4).
    ///
    /// `data` must be eight unpredictable bytes chosen by the caller. `new_cid_seq`
    /// is the sequence number of an unused peer connection ID the caller obtained
    /// from [`RemoteConnIds`](super::conn_id::RemoteConnIds) to stamp on the new
    /// path (RFC 9000 §9.5). `path` describes the new path's amplification and
    /// congestion behaviour ([`NewPath`]).
    ///
    /// # Errors
    ///
    /// - [`MigrationError::HandshakeNotConfirmed`] if the handshake is not yet
    ///   confirmed (RFC 9000 §9).
    /// - [`MigrationError::AlreadyMigrating`] if a migration is already in
    ///   progress.
    pub fn initiate(
        &mut self,
        data: [u8; PATH_DATA_LEN],
        now: Instant,
        pto: Duration,
        new_cid_seq: u64,
        path: NewPath,
    ) -> Result<Frame, MigrationError> {
        if !self.handshake_confirmed {
            return Err(MigrationError::HandshakeNotConfirmed);
        }
        if self.state == MigrationState::Probing {
            return Err(MigrationError::AlreadyMigrating);
        }

        let mut validator = PathValidator::new();
        // A fresh validator is always in the Validating state, so this yields the
        // challenge frame.
        let frame = validator
            .send_challenge(data, now, pto)
            .expect("a fresh PathValidator emits its first challenge");
        self.validator = Some(validator);
        self.anti_amplification = if path.peer_address_validated {
            None
        } else {
            Some(AntiAmplificationLimit::new())
        };
        self.new_cid_seq = Some(new_cid_seq);
        self.reset_on_success = !path.port_only;
        self.state = MigrationState::Probing;
        Ok(frame)
    }

    /// Record and return a fresh `PATH_CHALLENGE` retransmission on the path being
    /// probed, (re)arming the abandon timer (RFC 9000 §8.2.1). `data` must be a
    /// fresh eight unpredictable bytes so a response cannot be forged from an
    /// observed challenge.
    ///
    /// Returns `None` when not migrating or once the probe has completed.
    pub fn send_challenge(
        &mut self,
        data: [u8; PATH_DATA_LEN],
        now: Instant,
        pto: Duration,
    ) -> Option<Frame> {
        self.validator
            .as_mut()
            .and_then(|v| v.send_challenge(data, now, pto))
    }

    /// Record `bytes` received on the path being probed, raising its
    /// anti-amplification send allowance (RFC 9000 §8.1, §9.3). A no-op when not
    /// migrating or when the new path's peer address was already validated.
    pub fn on_received(&mut self, bytes: u64) {
        if let Some(limit) = self.anti_amplification.as_mut() {
            limit.on_received(bytes);
        }
    }

    /// Record `bytes` sent on the path being probed, lowering its
    /// anti-amplification send allowance (RFC 9000 §8.1, §9.3). A no-op when not
    /// migrating or when the new path's peer address was already validated.
    pub fn on_sent(&mut self, bytes: u64) {
        if let Some(limit) = self.anti_amplification.as_mut() {
            limit.on_sent(bytes);
        }
    }

    /// How many more bytes may be sent on the path being probed right now
    /// (RFC 9000 §8.1, §9.3), or `None` when no anti-amplification limit applies
    /// (not migrating, or the new path's peer address is already validated).
    #[must_use]
    pub fn send_allowance(&self) -> Option<u64> {
        self.anti_amplification
            .as_ref()
            .and_then(AntiAmplificationLimit::send_allowance)
    }

    /// Whether a datagram of `bytes` bytes may be sent on the path being probed
    /// without exceeding its anti-amplification limit (RFC 9000 §8.1, §9.3).
    /// Always `true` when no limit applies.
    #[must_use]
    pub fn can_send(&self, bytes: u64) -> bool {
        match self.send_allowance() {
            None => true,
            Some(allowance) => bytes <= allowance,
        }
    }

    /// Whether the endpoint is blocked from sending on the path being probed by
    /// its anti-amplification limit (RFC 9000 §8.1, §9.3). `false` when no limit
    /// applies.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.anti_amplification
            .as_ref()
            .is_some_and(AntiAmplificationLimit::is_blocked)
    }

    /// Process the data of a received `PATH_RESPONSE` on the path being probed
    /// (RFC 9000 §8.2.3, §9.3).
    ///
    /// If it matches an outstanding challenge, the migration completes: the state
    /// returns to [`Stable`](MigrationState::Stable) on the new path and a
    /// [`MigrationOutcome`] is returned telling the caller which peer connection
    /// ID is now active (RFC 9000 §9.5) and whether to reset the congestion
    /// controller and RTT estimator (RFC 9000 §9.4). Returns `None` when not
    /// migrating or when the data matches no outstanding challenge.
    pub fn on_path_response(&mut self, data: [u8; PATH_DATA_LEN]) -> Option<MigrationOutcome> {
        let validator = self.validator.as_mut()?;
        if !validator.on_path_response(data) {
            return None;
        }
        let outcome = MigrationOutcome {
            active_cid_seq: self
                .new_cid_seq
                .expect("new_cid_seq is set whenever a validator is present"),
            reset_congestion_and_rtt: self.reset_on_success,
        };
        self.clear();
        Some(outcome)
    }

    /// Abandon the in-progress migration if its validation deadline has been
    /// reached (RFC 9000 §8.2.4, §9.3): the connection returns to
    /// [`Stable`](MigrationState::Stable) on the *old* path and continues there.
    /// Returns `true` if the migration was abandoned, `false` otherwise.
    pub fn on_timeout(&mut self, now: Instant) -> bool {
        let Some(validator) = self.validator.as_mut() else {
            return false;
        };
        if validator.on_timeout(now) {
            self.clear();
            true
        } else {
            false
        }
    }

    /// Reset the probing fields back to the non-migrating [`Stable`] state,
    /// preserving the confirmed-handshake flag.
    fn clear(&mut self) {
        self.state = MigrationState::Stable;
        self.validator = None;
        self.anti_amplification = None;
        self.new_cid_seq = None;
        self.reset_on_success = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::quic_frame::{AckRange, STATELESS_RESET_TOKEN_LEN};

    /// A fixed origin instant, avoiding the forbidden `Instant::now()` littered
    /// across the test body.
    fn origin() -> Instant {
        Instant::now()
    }

    const PTO: Duration = Duration::from_millis(100);
    const DATA: [u8; PATH_DATA_LEN] = [1, 2, 3, 4, 5, 6, 7, 8];

    fn new_connection_id() -> Frame {
        Frame::NewConnectionId {
            sequence_number: 1,
            retire_prior_to: 0,
            connection_id: vec![9, 9, 9, 9],
            stateless_reset_token: [0; STATELESS_RESET_TOKEN_LEN],
        }
    }

    // ---- Probing / non-probing classification (§9.1) ----

    #[test]
    fn probing_frames_are_recognised() {
        assert!(is_probing_frame(&Frame::PathChallenge(DATA)));
        assert!(is_probing_frame(&Frame::PathResponse(DATA)));
        assert!(is_probing_frame(&new_connection_id()));
        assert!(is_probing_frame(&Frame::Padding(10)));
    }

    #[test]
    fn non_probing_frames_are_recognised() {
        assert!(!is_probing_frame(&Frame::Ping));
        assert!(!is_probing_frame(&Frame::HandshakeDone));
        assert!(!is_probing_frame(&Frame::Stream {
            stream_id: 0,
            offset: 0,
            fin: false,
            data: vec![1, 2, 3],
        }));
        assert!(!is_probing_frame(&Frame::Ack {
            largest_acked: 0,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: Vec::<AckRange>::new(),
            ecn: None,
        }));
    }

    #[test]
    fn probing_packet_requires_all_probing_frames() {
        let probing = [Frame::PathChallenge(DATA), Frame::Padding(4)];
        assert!(is_probing_packet(&probing));

        let mixed = [Frame::PathChallenge(DATA), Frame::Ping];
        assert!(!is_probing_packet(&mixed));
    }

    #[test]
    fn empty_packet_is_not_probing() {
        let empty: [Frame; 0] = [];
        assert!(!is_probing_packet(&empty));
    }

    #[test]
    fn all_padding_packet_is_probing() {
        let padding = [Frame::Padding(1200)];
        assert!(is_probing_packet(&padding));
    }

    // ---- Handshake gate (§9) ----

    #[test]
    fn migration_before_handshake_is_refused() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        assert!(!m.is_handshake_confirmed());
        let err = m
            .initiate(DATA, t0, PTO, 1, NewPath::local_migration())
            .unwrap_err();
        assert_eq!(err, MigrationError::HandshakeNotConfirmed);
        assert_eq!(m.state(), MigrationState::Stable);
        assert!(!m.is_migrating());
    }

    #[test]
    fn confirm_handshake_permits_migration() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        let frame = m
            .initiate(DATA, t0, PTO, 7, NewPath::local_migration())
            .unwrap();
        assert_eq!(frame, Frame::PathChallenge(DATA));
        assert!(m.is_migrating());
        assert_eq!(m.state(), MigrationState::Probing);
        assert_eq!(m.probe_state(), Some(PathState::Validating));
        assert_eq!(m.probing_cid_seq(), Some(7));
        assert_eq!(m.deadline(), Some(t0 + 3 * PTO));
    }

    // ---- Initiating migration (§9.2) ----

    #[test]
    fn double_initiate_is_refused() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 1, NewPath::local_migration()).unwrap();
        let err = m
            .initiate([9; PATH_DATA_LEN], t0, PTO, 2, NewPath::local_migration())
            .unwrap_err();
        assert_eq!(err, MigrationError::AlreadyMigrating);
        // The original probe is untouched.
        assert_eq!(m.probing_cid_seq(), Some(1));
    }

    // ---- Successful migration completion (§9.4, §9.5) ----

    #[test]
    fn matching_response_completes_local_migration_with_reset() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 5, NewPath::local_migration()).unwrap();

        let outcome = m.on_path_response(DATA).expect("path validated");
        assert_eq!(outcome.active_cid_seq, 5);
        // A full path change resets congestion/RTT state.
        assert!(outcome.reset_congestion_and_rtt);
        // Migration completed; back to a single stable path.
        assert_eq!(m.state(), MigrationState::Stable);
        assert!(!m.is_migrating());
        assert_eq!(m.deadline(), None);
        assert_eq!(m.probe_state(), None);
    }

    #[test]
    fn port_only_migration_retains_congestion_state() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 3, NewPath::peer_rebinding()).unwrap();

        let outcome = m.on_path_response(DATA).expect("path validated");
        assert_eq!(outcome.active_cid_seq, 3);
        // A port-only change keeps the send rate (RFC 9000 §9.4).
        assert!(!outcome.reset_congestion_and_rtt);
    }

    #[test]
    fn non_matching_response_leaves_migration_in_progress() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 1, NewPath::local_migration()).unwrap();
        assert!(m.on_path_response([9; PATH_DATA_LEN]).is_none());
        assert!(m.is_migrating());
        assert_eq!(m.deadline(), Some(t0 + 3 * PTO));
    }

    #[test]
    fn response_when_not_migrating_is_ignored() {
        let mut m = ConnectionMigration::new();
        assert!(m.on_path_response(DATA).is_none());
        assert_eq!(m.state(), MigrationState::Stable);
    }

    #[test]
    fn challenge_retransmission_rearms_and_still_validates() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 1, NewPath::local_migration()).unwrap();
        // Retransmit with fresh data (§8.2.1); the deadline re-arms.
        let frame = m.send_challenge([2; PATH_DATA_LEN], t0 + PTO, PTO);
        assert_eq!(frame, Some(Frame::PathChallenge([2; PATH_DATA_LEN])));
        assert_eq!(m.deadline(), Some(t0 + PTO + 3 * PTO));
        // A response echoing the *first* challenge still validates.
        let outcome = m.on_path_response(DATA).expect("path validated");
        assert_eq!(outcome.active_cid_seq, 1);
    }

    #[test]
    fn send_challenge_when_not_migrating_returns_none() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        assert_eq!(m.send_challenge(DATA, t0, PTO), None);
    }

    // ---- Abandoned migration (§8.2.4, §9.3) ----

    #[test]
    fn timeout_abandons_migration_back_to_old_path() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 4, NewPath::local_migration()).unwrap();
        assert!(m.on_timeout(t0 + 3 * PTO));
        // Reverts to the stable old path; no outcome, no reset.
        assert_eq!(m.state(), MigrationState::Stable);
        assert!(!m.is_migrating());
        assert_eq!(m.probing_cid_seq(), None);
    }

    #[test]
    fn timeout_before_deadline_keeps_migrating() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 4, NewPath::local_migration()).unwrap();
        assert!(!m.on_timeout(t0 + 2 * PTO));
        assert!(m.is_migrating());
    }

    #[test]
    fn timeout_when_not_migrating_does_nothing() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        assert!(!m.on_timeout(t0 + 100 * PTO));
    }

    #[test]
    fn can_migrate_again_after_abandoning() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        m.initiate(DATA, t0, PTO, 1, NewPath::local_migration()).unwrap();
        m.on_timeout(t0 + 3 * PTO);
        // A second migration attempt is now permitted.
        let frame = m.initiate([2; PATH_DATA_LEN], t0 + 4 * PTO, PTO, 2, NewPath::local_migration());
        assert!(frame.is_ok());
        assert_eq!(m.probing_cid_seq(), Some(2));
    }

    // ---- New-path anti-amplification (§9.3) ----

    #[test]
    fn unvalidated_peer_address_applies_amplification_limit() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        // Validating a peer's new (unvalidated) address, e.g. preferred address.
        let path = NewPath {
            peer_address_validated: false,
            port_only: false,
        };
        m.initiate(DATA, t0, PTO, 1, path).unwrap();
        // With nothing received, the endpoint is amplification-blocked.
        assert_eq!(m.send_allowance(), Some(0));
        assert!(m.is_blocked());
        m.on_received(1200);
        assert_eq!(m.send_allowance(), Some(3600));
        assert!(m.can_send(3600));
        assert!(!m.can_send(3601));
        m.on_sent(3600);
        assert_eq!(m.send_allowance(), Some(0));
        assert!(m.is_blocked());
    }

    #[test]
    fn validated_peer_address_has_no_amplification_limit() {
        let t0 = origin();
        let mut m = ConnectionMigration::new();
        m.confirm_handshake();
        // A local migration keeps the already-validated peer address.
        m.initiate(DATA, t0, PTO, 1, NewPath::local_migration()).unwrap();
        assert_eq!(m.send_allowance(), None);
        assert!(!m.is_blocked());
        assert!(m.can_send(u64::MAX));
    }

    #[test]
    fn amplification_counters_are_no_ops_when_not_migrating() {
        let mut m = ConnectionMigration::new();
        m.on_received(5000);
        m.on_sent(5000);
        assert_eq!(m.send_allowance(), None);
        assert!(!m.is_blocked());
        assert!(m.can_send(u64::MAX));
    }

    // ---- Descriptors ----

    #[test]
    fn new_path_descriptors_carry_expected_flags() {
        assert_eq!(
            NewPath::local_migration(),
            NewPath {
                peer_address_validated: true,
                port_only: false,
            }
        );
        assert_eq!(
            NewPath::peer_rebinding(),
            NewPath {
                peer_address_validated: false,
                port_only: true,
            }
        );
    }

    #[test]
    fn default_matches_new() {
        assert_eq!(ConnectionMigration::default(), ConnectionMigration::new());
    }
}
