//! QUIC connection-ID management (RFC 9000 §5.1).
//!
//! Every QUIC endpoint issues *connection IDs* the peer stamps into the packets
//! it sends, so a connection survives a change of the underlying 4-tuple (NAT
//! rebinding, migration). Two independent sets exist, and this slice models both
//! as pure state machines driven by the [`super::quic_frame`] NEW_CONNECTION_ID
//! (RFC 9000 §19.15) and RETIRE_CONNECTION_ID (RFC 9000 §19.16) frames:
//!
//! - **Remote connection IDs** ([`RemoteConnIds`]): the IDs the *peer* issues for
//!   us to use as the Destination Connection ID of the packets we send. We seed
//!   the set with the server's chosen Source Connection ID (sequence 0) from the
//!   handshake, then fold in each NEW_CONNECTION_ID frame — validating it, honouring
//!   its `Retire Prior To`, enforcing the `active_connection_id_limit` we advertised
//!   (RFC 9000 §5.1.1), and reporting which sequence numbers the connection layer
//!   must now RETIRE_CONNECTION_ID (RFC 9000 §5.1.2).
//! - **Local connection IDs** ([`LocalConnIds`]): the IDs *we* issue for the peer
//!   to use when addressing us. We assign monotonically increasing sequence
//!   numbers (RFC 9000 §5.1.1), never issue more active IDs than the peer's
//!   advertised `active_connection_id_limit`, and drop an ID when the peer sends
//!   RETIRE_CONNECTION_ID for it.
//!
//! Like every slice so far this is a pure state machine — no IO, no packet
//! protection, no timers, and no packet-level context (the "cannot retire the
//! connection ID used as the packet's Destination" check of RFC 9000 §19.16 is
//! the connection layer's job, since only it knows which ID carried the frame).
//! The connection layer drives it: it feeds decoded frames and reads back the
//! ID to stamp on outgoing packets and the RETIRE_CONNECTION_ID / NEW_CONNECTION_ID
//! frames it must emit.

use super::packet::MAX_CONNECTION_ID_LEN;
use super::quic_frame::STATELESS_RESET_TOKEN_LEN;
use std::collections::BTreeMap;

/// `CONNECTION_ID_LIMIT_ERROR` — the peer supplied more active connection IDs
/// than the `active_connection_id_limit` we advertised (RFC 9000 §20.1, §5.1.1).
pub const CONNECTION_ID_LIMIT_ERROR: u64 = 0x09;

/// `PROTOCOL_VIOLATION` — a connection-ID frame broke an invariant that is not
/// covered by a more specific code, e.g. reusing a sequence number for a
/// different ID, or retiring an ID we never issued (RFC 9000 §20.1).
pub const PROTOCOL_VIOLATION: u64 = 0x0a;

/// `FRAME_ENCODING_ERROR` — a malformed NEW_CONNECTION_ID frame (Retire Prior To
/// past the sequence number, or a connection ID outside the 1..=20 byte range)
/// (RFC 9000 §20.1, §19.15). Mirrors [`super::quic_frame::FRAME_ENCODING_ERROR`].
pub const FRAME_ENCODING_ERROR: u64 = super::quic_frame::FRAME_ENCODING_ERROR;

/// The floor on `active_connection_id_limit` every endpoint must advertise
/// (RFC 9000 §18.2): an endpoint always tolerates at least two active IDs, so
/// the peer can migrate to a fresh one without a round trip.
pub const MIN_ACTIVE_CONNECTION_ID_LIMIT: u64 = 2;

// ── Connection-level errors (RFC 9000 §20.1) ────────────────────────────────

/// A connection-ID protocol violation. Each variant maps to a single QUIC
/// connection-error code via [`ConnIdError::code`]; the variant preserves *why*
/// for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnIdError {
    /// A NEW_CONNECTION_ID frame was malformed: its `Retire Prior To` exceeded
    /// its own sequence number, or its connection ID was empty or longer than
    /// [`MAX_CONNECTION_ID_LEN`] (RFC 9000 §19.15). Maps to
    /// [`FRAME_ENCODING_ERROR`].
    Malformed,
    /// A sequence number was reused for a *different* connection ID or stateless
    /// reset token (RFC 9000 §5.1.1), or a RETIRE_CONNECTION_ID named a sequence
    /// number we have not yet issued (RFC 9000 §19.16). Maps to
    /// [`PROTOCOL_VIOLATION`].
    SequenceConflict,
    /// After processing a NEW_CONNECTION_ID frame the number of active connection
    /// IDs would exceed the `active_connection_id_limit` we advertised
    /// (RFC 9000 §5.1.1). Maps to [`CONNECTION_ID_LIMIT_ERROR`].
    LimitExceeded {
        /// The number of active IDs the peer's frame would leave us holding.
        active: u64,
        /// The `active_connection_id_limit` we advertised.
        limit: u64,
    },
}

impl ConnIdError {
    /// The RFC 9000 §20.1 connection-error code this violation maps to.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::Malformed => FRAME_ENCODING_ERROR,
            Self::SequenceConflict => PROTOCOL_VIOLATION,
            Self::LimitExceeded { .. } => CONNECTION_ID_LIMIT_ERROR,
        }
    }
}

impl core::fmt::Display for ConnIdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed => write!(f, "QUIC connection ID: malformed NEW_CONNECTION_ID frame"),
            Self::SequenceConflict => {
                write!(f, "QUIC connection ID: sequence number reused or retired ID never issued")
            }
            Self::LimitExceeded { active, limit } => write!(
                f,
                "QUIC connection ID: {active} active IDs exceeds active_connection_id_limit {limit}"
            ),
        }
    }
}

impl std::error::Error for ConnIdError {}

// ── A single connection ID plus its reset token ─────────────────────────────

/// One issued connection ID together with the 16-byte stateless reset token
/// bound to it (RFC 9000 §5.1.1, §10.3). The initial ID exchanged during the
/// handshake has no token, so [`ConnIdEntry::stateless_reset_token`] is `None`
/// there; every ID delivered by a NEW_CONNECTION_ID frame carries one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnIdEntry {
    /// The connection ID itself (1..=20 bytes, RFC 9000 §17.2).
    pub connection_id: Vec<u8>,
    /// The stateless reset token bound to this ID, or `None` for the handshake
    /// connection ID (RFC 9000 §5.1.1).
    pub stateless_reset_token: Option<[u8; STATELESS_RESET_TOKEN_LEN]>,
}

/// Validate a connection ID's length against RFC 9000 §17.2 / §19.15: a QUIC v1
/// connection ID is 1..=20 bytes. (A zero-length ID is legal on the wire but an
/// endpoint that uses one for itself cannot be *issued* new IDs, and a peer that
/// advertises one never sends NEW_CONNECTION_ID, so it never reaches this set.)
fn valid_cid_len(cid: &[u8]) -> bool {
    !cid.is_empty() && cid.len() <= MAX_CONNECTION_ID_LEN
}

// ── Remote connection IDs — the peer's IDs, consumed by us ───────────────────

/// The set of connection IDs the peer has issued for us to use as the
/// Destination Connection ID of the packets we send (RFC 9000 §5.1.2).
///
/// Seeded with the peer's handshake Source Connection ID (sequence 0), then
/// grown by [`RemoteConnIds::record_new_connection_id`] as NEW_CONNECTION_ID
/// frames arrive. It tracks the highest `Retire Prior To` the peer has requested,
/// enforces the `active_connection_id_limit` we advertised, and hands the
/// connection layer the sequence numbers it must RETIRE_CONNECTION_ID.
#[derive(Clone, Debug)]
pub struct RemoteConnIds {
    /// Active IDs by sequence number (BTreeMap keeps the lowest-numbered — the
    /// preferred current ID — at the front and gives deterministic iteration).
    active: BTreeMap<u64, ConnIdEntry>,
    /// Every sequence number ever received, with the ID+token it named, so a
    /// retransmitted frame is idempotent and a reused number is caught even
    /// after the ID has been retired (RFC 9000 §5.1.1).
    seen: BTreeMap<u64, (Vec<u8>, Option<[u8; STATELESS_RESET_TOKEN_LEN]>)>,
    /// The highest `Retire Prior To` the peer has requested; IDs below it are
    /// never installed as active (RFC 9000 §19.15).
    retire_prior_to: u64,
    /// The `active_connection_id_limit` we advertised — the cap on how many IDs
    /// the peer may keep active with us (RFC 9000 §5.1.1, §18.2).
    active_connection_id_limit: u64,
    /// The sequence number of the ID we currently stamp on outgoing packets.
    current: u64,
}

impl RemoteConnIds {
    /// Seed the set with the peer's handshake connection ID (sequence 0, no
    /// reset token), advertising `active_connection_id_limit` as the cap on how
    /// many active IDs we will hold. The limit is clamped up to
    /// [`MIN_ACTIVE_CONNECTION_ID_LIMIT`] since RFC 9000 §18.2 forbids a smaller
    /// value.
    #[must_use]
    pub fn new(initial_cid: Vec<u8>, active_connection_id_limit: u64) -> Self {
        let mut active = BTreeMap::new();
        active.insert(
            0,
            ConnIdEntry { connection_id: initial_cid.clone(), stateless_reset_token: None },
        );
        let mut seen = BTreeMap::new();
        seen.insert(0, (initial_cid, None));
        Self {
            active,
            seen,
            retire_prior_to: 0,
            active_connection_id_limit: active_connection_id_limit
                .max(MIN_ACTIVE_CONNECTION_ID_LIMIT),
            current: 0,
        }
    }

    /// Fold a NEW_CONNECTION_ID frame (RFC 9000 §19.15) into the set.
    ///
    /// Returns the sequence numbers the connection layer must now acknowledge
    /// with a RETIRE_CONNECTION_ID frame (RFC 9000 §5.1.2): any active ID below
    /// an advanced `Retire Prior To`, plus this very ID when its own sequence
    /// number is already below the retire threshold. The list is free of
    /// duplicates and each number is reported at most once across calls.
    ///
    /// # Errors
    ///
    /// - [`ConnIdError::Malformed`] if `retire_prior_to > sequence_number` or the
    ///   connection ID is empty / longer than [`MAX_CONNECTION_ID_LEN`].
    /// - [`ConnIdError::SequenceConflict`] if `sequence_number` was already
    ///   received with a different connection ID or reset token.
    /// - [`ConnIdError::LimitExceeded`] if installing this ID would leave more
    ///   active IDs than the advertised `active_connection_id_limit`.
    pub fn record_new_connection_id(
        &mut self,
        sequence_number: u64,
        retire_prior_to: u64,
        connection_id: Vec<u8>,
        stateless_reset_token: [u8; STATELESS_RESET_TOKEN_LEN],
    ) -> Result<Vec<u64>, ConnIdError> {
        // RFC 9000 §19.15: Retire Prior To is bounded by the sequence number,
        // and the connection ID length is 1..=20.
        if retire_prior_to > sequence_number || !valid_cid_len(&connection_id) {
            return Err(ConnIdError::Malformed);
        }

        // RFC 9000 §5.1.1: a sequence number binds one ID+token for the life of
        // the connection. A byte-identical retransmission is a no-op; a differing
        // one is a protocol violation.
        if let Some((prev_cid, prev_token)) = self.seen.get(&sequence_number) {
            if *prev_cid != connection_id || *prev_token != Some(stateless_reset_token) {
                return Err(ConnIdError::SequenceConflict);
            }
            // Duplicate frame — already accounted for; advance nothing.
            return Ok(Vec::new());
        }
        self.seen
            .insert(sequence_number, (connection_id.clone(), Some(stateless_reset_token)));

        let mut retired = Vec::new();

        // RFC 9000 §19.15: an increased Retire Prior To retires every active ID
        // below it. Retire Prior To only ever moves forward.
        if retire_prior_to > self.retire_prior_to {
            self.retire_prior_to = retire_prior_to;
            let stale: Vec<u64> =
                self.active.range(..retire_prior_to).map(|(seq, _)| *seq).collect();
            for seq in stale {
                self.active.remove(&seq);
                retired.push(seq);
            }
        }

        if sequence_number < self.retire_prior_to {
            // The new ID is already obsolete: never install it, just retire it.
            retired.push(sequence_number);
        } else {
            self.active.insert(
                sequence_number,
                ConnIdEntry {
                    connection_id,
                    stateless_reset_token: Some(stateless_reset_token),
                },
            );
        }

        // RFC 9000 §5.1.1: after processing, the active set must fit the limit
        // we advertised. Pending retirements above have already left the set.
        let active = self.active.len() as u64;
        if active > self.active_connection_id_limit {
            return Err(ConnIdError::LimitExceeded {
                active,
                limit: self.active_connection_id_limit,
            });
        }

        // If the ID we were using got retired, migrate to the lowest active one.
        if !self.active.contains_key(&self.current)
            && let Some((&seq, _)) = self.active.iter().next()
        {
            self.current = seq;
        }

        Ok(retired)
    }

    /// The connection ID we currently stamp on outgoing packets, or `None` if no
    /// active ID remains (the peer retired everything, a connection error).
    #[must_use]
    pub fn current(&self) -> Option<&[u8]> {
        self.active.get(&self.current).map(|e| e.connection_id.as_slice())
    }

    /// The sequence number of the [`RemoteConnIds::current`] ID.
    #[must_use]
    pub const fn current_sequence(&self) -> u64 {
        self.current
    }

    /// Number of connection IDs currently active (not yet retired).
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Look up an active entry by sequence number, e.g. to recover the stateless
    /// reset token bound to it (RFC 9000 §10.3).
    #[must_use]
    pub fn get(&self, sequence_number: u64) -> Option<&ConnIdEntry> {
        self.active.get(&sequence_number)
    }

    /// Whether `token` matches the reset token of any active ID — the test an
    /// endpoint runs on an unattributable short-header packet to detect a
    /// stateless reset (RFC 9000 §10.3.1).
    #[must_use]
    pub fn is_stateless_reset(&self, token: &[u8; STATELESS_RESET_TOKEN_LEN]) -> bool {
        self.active
            .values()
            .any(|e| e.stateless_reset_token.as_ref() == Some(token))
    }

    /// Voluntarily migrate to a different active ID (e.g. after a path change),
    /// returning the sequence number to RETIRE_CONNECTION_ID for the ID we left.
    /// The old ID is removed from the active set. `None` if `sequence_number` is
    /// not active or is already the current ID.
    pub fn switch_to(&mut self, sequence_number: u64) -> Option<u64> {
        if sequence_number == self.current || !self.active.contains_key(&sequence_number) {
            return None;
        }
        let old = self.current;
        self.active.remove(&old);
        self.current = sequence_number;
        Some(old)
    }
}

// ── Local connection IDs — our IDs, issued to the peer ───────────────────────

/// One connection ID we have issued to the peer, plus its reset token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalConnId {
    /// Sequence number we assigned this ID (RFC 9000 §5.1.1).
    pub sequence_number: u64,
    /// The connection ID bytes (1..=20).
    pub connection_id: Vec<u8>,
    /// The stateless reset token bound to it, or `None` for the handshake ID.
    pub stateless_reset_token: Option<[u8; STATELESS_RESET_TOKEN_LEN]>,
}

/// The set of connection IDs *we* have issued for the peer to address us
/// (RFC 9000 §5.1.1). Assigns monotonically increasing sequence numbers, refuses
/// to issue more active IDs than the peer's advertised limit, and drops an ID
/// when the peer retires it.
#[derive(Clone, Debug)]
pub struct LocalConnIds {
    /// Active IDs we have issued, by sequence number.
    active: BTreeMap<u64, LocalConnId>,
    /// The next sequence number to assign (monotonic, RFC 9000 §5.1.1).
    next_sequence_number: u64,
    /// The peer's advertised `active_connection_id_limit` — the cap on how many
    /// of our IDs may be active at once (RFC 9000 §5.1.1, §18.2).
    peer_limit: u64,
}

impl LocalConnIds {
    /// Seed with our handshake connection ID (sequence 0, no reset token). The
    /// peer's `active_connection_id_limit` caps how many additional IDs we may
    /// issue; it is clamped up to [`MIN_ACTIVE_CONNECTION_ID_LIMIT`] per
    /// RFC 9000 §18.2.
    #[must_use]
    pub fn new(initial_cid: Vec<u8>, peer_limit: u64) -> Self {
        let mut active = BTreeMap::new();
        active.insert(
            0,
            LocalConnId {
                sequence_number: 0,
                connection_id: initial_cid,
                stateless_reset_token: None,
            },
        );
        Self {
            active,
            next_sequence_number: 1,
            peer_limit: peer_limit.max(MIN_ACTIVE_CONNECTION_ID_LIMIT),
        }
    }

    /// Issue a fresh connection ID to the peer, returning the [`LocalConnId`] the
    /// connection layer serialises into a NEW_CONNECTION_ID frame (with
    /// `retire_prior_to = 0`; use [`LocalConnIds::issue_retiring`] to also
    /// request retirement of older IDs).
    ///
    /// # Errors
    ///
    /// [`ConnIdError::LimitExceeded`] if we already hold the peer's advertised
    /// `active_connection_id_limit` active IDs (RFC 9000 §5.1.1) — the peer must
    /// retire one before we may issue another.
    pub fn issue(
        &mut self,
        connection_id: Vec<u8>,
        stateless_reset_token: [u8; STATELESS_RESET_TOKEN_LEN],
    ) -> Result<LocalConnId, ConnIdError> {
        if !valid_cid_len(&connection_id) {
            return Err(ConnIdError::Malformed);
        }
        let active = self.active.len() as u64;
        if active >= self.peer_limit {
            return Err(ConnIdError::LimitExceeded { active: active + 1, limit: self.peer_limit });
        }
        let sequence_number = self.next_sequence_number;
        self.next_sequence_number += 1;
        let entry = LocalConnId {
            sequence_number,
            connection_id,
            stateless_reset_token: Some(stateless_reset_token),
        };
        self.active.insert(sequence_number, entry.clone());
        Ok(entry)
    }

    /// Drop one of our IDs because the peer sent RETIRE_CONNECTION_ID for it
    /// (RFC 9000 §19.16), freeing a slot under the peer's limit.
    ///
    /// # Errors
    ///
    /// [`ConnIdError::SequenceConflict`] if `sequence_number` names an ID we have
    /// never issued (a sequence number at or above the next one to assign) —
    /// RFC 9000 §19.16 makes that a `PROTOCOL_VIOLATION`. Retiring an already-
    /// retired (or never-active) ID below that bound is a tolerated no-op.
    pub fn retire(&mut self, sequence_number: u64) -> Result<(), ConnIdError> {
        if sequence_number >= self.next_sequence_number {
            return Err(ConnIdError::SequenceConflict);
        }
        self.active.remove(&sequence_number);
        Ok(())
    }

    /// Whether a peer-supplied Destination Connection ID matches one of the IDs
    /// we currently have issued — the routing check the connection layer runs on
    /// an incoming packet.
    #[must_use]
    pub fn accepts(&self, connection_id: &[u8]) -> bool {
        self.active.values().any(|e| e.connection_id == connection_id)
    }

    /// Number of IDs we currently have issued and active.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// The next sequence number [`LocalConnIds::issue`] will assign.
    #[must_use]
    pub const fn next_sequence_number(&self) -> u64 {
        self.next_sequence_number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A distinct 16-byte reset token keyed off `seed` for test readability.
    fn token(seed: u8) -> [u8; STATELESS_RESET_TOKEN_LEN] {
        [seed; STATELESS_RESET_TOKEN_LEN]
    }

    #[test]
    fn remote_seeds_handshake_id_at_sequence_zero() {
        let r = RemoteConnIds::new(vec![1, 2, 3, 4], 4);
        assert_eq!(r.current(), Some([1, 2, 3, 4].as_slice()));
        assert_eq!(r.current_sequence(), 0);
        assert_eq!(r.active_count(), 1);
        // The handshake ID carries no reset token.
        assert_eq!(r.get(0).unwrap().stateless_reset_token, None);
    }

    #[test]
    fn remote_limit_clamped_to_minimum() {
        // An advertised limit below 2 is illegal (RFC 9000 §18.2); we clamp up so
        // a peer that issues a second ID is not spuriously rejected.
        let mut r = RemoteConnIds::new(vec![0xaa], 0);
        let retired = r
            .record_new_connection_id(1, 0, vec![0xbb], token(1))
            .expect("second ID fits the clamped limit of 2");
        assert!(retired.is_empty());
        assert_eq!(r.active_count(), 2);
    }

    #[test]
    fn remote_records_and_keeps_current() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        let retired = r.record_new_connection_id(1, 0, vec![9, 9], token(1)).unwrap();
        assert!(retired.is_empty());
        assert_eq!(r.active_count(), 2);
        // Adding an ID does not move the current pointer off sequence 0.
        assert_eq!(r.current_sequence(), 0);
        assert_eq!(r.get(1).unwrap().stateless_reset_token, Some(token(1)));
    }

    #[test]
    fn remote_retire_prior_to_retires_lower_ids_and_migrates_current() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        r.record_new_connection_id(2, 0, vec![2], token(2)).unwrap();
        // ID 3 asks to retire everything below sequence 2.
        let retired = r.record_new_connection_id(3, 2, vec![3], token(3)).unwrap();
        assert_eq!(retired, vec![0, 1]);
        // Current was 0, now retired → migrate to lowest active (2).
        assert_eq!(r.current_sequence(), 2);
        assert_eq!(r.active_count(), 2); // seqs 2 and 3
    }

    #[test]
    fn remote_id_below_retire_threshold_is_retired_not_installed() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        // Advance retire_prior_to to 2 (retires 0 and 1... only 0 exists).
        let first = r.record_new_connection_id(2, 2, vec![2], token(2)).unwrap();
        assert_eq!(first, vec![0]);
        // A late ID with sequence 1 < retire_prior_to 2: retire it, never install.
        let retired = r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        assert_eq!(retired, vec![1]);
        assert!(r.get(1).is_none());
    }

    #[test]
    fn remote_malformed_retire_prior_to_beyond_sequence() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        let err = r.record_new_connection_id(1, 2, vec![1], token(1)).unwrap_err();
        assert_eq!(err, ConnIdError::Malformed);
        assert_eq!(err.code(), FRAME_ENCODING_ERROR);
    }

    #[test]
    fn remote_malformed_connection_id_length() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        assert_eq!(
            r.record_new_connection_id(1, 0, vec![], token(1)).unwrap_err(),
            ConnIdError::Malformed
        );
        let too_long = vec![7u8; MAX_CONNECTION_ID_LEN + 1];
        assert_eq!(
            r.record_new_connection_id(1, 0, too_long, token(1)).unwrap_err(),
            ConnIdError::Malformed
        );
    }

    #[test]
    fn remote_duplicate_frame_is_idempotent() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        // Exact retransmission: no change, no new retirements.
        let retired = r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        assert!(retired.is_empty());
        assert_eq!(r.active_count(), 2);
    }

    #[test]
    fn remote_sequence_conflict_on_differing_id() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        let err = r.record_new_connection_id(1, 0, vec![9], token(1)).unwrap_err();
        assert_eq!(err, ConnIdError::SequenceConflict);
        assert_eq!(err.code(), PROTOCOL_VIOLATION);
    }

    #[test]
    fn remote_sequence_conflict_on_differing_token() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        assert_eq!(
            r.record_new_connection_id(1, 0, vec![1], token(2)).unwrap_err(),
            ConnIdError::SequenceConflict
        );
    }

    #[test]
    fn remote_conflict_detected_after_retirement() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        // Retire seq 1 via a higher retire_prior_to.
        r.record_new_connection_id(2, 2, vec![2], token(2)).unwrap();
        assert!(r.get(1).is_none());
        // Reusing sequence 1 with a different ID is still a conflict.
        assert_eq!(
            r.record_new_connection_id(1, 0, vec![9], token(9)).unwrap_err(),
            ConnIdError::SequenceConflict
        );
    }

    #[test]
    fn remote_limit_exceeded() {
        let mut r = RemoteConnIds::new(vec![0], 2); // limit 2
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap(); // active = 2
        let err = r.record_new_connection_id(2, 0, vec![2], token(2)).unwrap_err();
        assert_eq!(err, ConnIdError::LimitExceeded { active: 3, limit: 2 });
        assert_eq!(err.code(), CONNECTION_ID_LIMIT_ERROR);
    }

    #[test]
    fn remote_retire_prior_to_frees_room_under_limit() {
        let mut r = RemoteConnIds::new(vec![0], 2); // limit 2
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap(); // active = {0,1}
        // ID 2 retires everything below 2, so active becomes {2}: within limit.
        let retired = r.record_new_connection_id(2, 2, vec![2], token(2)).unwrap();
        assert_eq!(retired, vec![0, 1]);
        assert_eq!(r.active_count(), 1);
    }

    #[test]
    fn remote_stateless_reset_detection() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        r.record_new_connection_id(1, 0, vec![1], token(0x42)).unwrap();
        assert!(r.is_stateless_reset(&token(0x42)));
        assert!(!r.is_stateless_reset(&token(0x00)));
    }

    #[test]
    fn remote_switch_to_migrates_and_retires_old() {
        let mut r = RemoteConnIds::new(vec![0], 4);
        r.record_new_connection_id(1, 0, vec![1], token(1)).unwrap();
        let old = r.switch_to(1).unwrap();
        assert_eq!(old, 0);
        assert_eq!(r.current_sequence(), 1);
        assert!(r.get(0).is_none());
        // Switching to the current or an unknown ID is a no-op.
        assert_eq!(r.switch_to(1), None);
        assert_eq!(r.switch_to(99), None);
    }

    #[test]
    fn remote_retiring_current_via_self_referential_frame_keeps_a_usable_id() {
        // A NEW_CONNECTION_ID whose own sequence equals its retire_prior_to
        // retires everything below it and installs itself, so a current ID always
        // survives (RFC 9000 §19.15 guarantees the frame provides a replacement).
        let mut r = RemoteConnIds::new(vec![0], 4);
        let retired = r.record_new_connection_id(1, 1, vec![1], token(1)).unwrap();
        assert_eq!(retired, vec![0]);
        assert_eq!(r.current_sequence(), 1);
        assert!(r.current().is_some());
    }

    #[test]
    fn local_seeds_and_issues_monotonic_sequences() {
        let mut l = LocalConnIds::new(vec![0xaa], 3);
        assert_eq!(l.active_count(), 1);
        assert_eq!(l.next_sequence_number(), 1);
        let a = l.issue(vec![1], token(1)).unwrap();
        assert_eq!(a.sequence_number, 1);
        let b = l.issue(vec![2], token(2)).unwrap();
        assert_eq!(b.sequence_number, 2);
        assert_eq!(l.active_count(), 3);
    }

    #[test]
    fn local_issue_refused_at_peer_limit() {
        let mut l = LocalConnIds::new(vec![0], 2); // limit 2, seq 0 already active
        l.issue(vec![1], token(1)).unwrap(); // active = 2
        let err = l.issue(vec![2], token(2)).unwrap_err();
        assert_eq!(err, ConnIdError::LimitExceeded { active: 3, limit: 2 });
        assert_eq!(err.code(), CONNECTION_ID_LIMIT_ERROR);
    }

    #[test]
    fn local_issue_rejects_bad_length() {
        let mut l = LocalConnIds::new(vec![0], 4);
        assert_eq!(l.issue(vec![], token(1)).unwrap_err(), ConnIdError::Malformed);
    }

    #[test]
    fn local_retire_frees_a_slot() {
        let mut l = LocalConnIds::new(vec![0], 2);
        l.issue(vec![1], token(1)).unwrap(); // active = {0,1}
        assert_eq!(l.issue(vec![2], token(2)).unwrap_err().code(), CONNECTION_ID_LIMIT_ERROR);
        l.retire(0).unwrap(); // peer retired seq 0 → active = {1}
        // Now a fresh ID fits again (with a new, higher sequence number).
        let c = l.issue(vec![3], token(3)).unwrap();
        assert_eq!(c.sequence_number, 2);
    }

    #[test]
    fn local_retire_unissued_is_protocol_violation() {
        let mut l = LocalConnIds::new(vec![0], 4);
        // next_sequence_number is 1, so seq 5 was never issued.
        let err = l.retire(5).unwrap_err();
        assert_eq!(err, ConnIdError::SequenceConflict);
        assert_eq!(err.code(), PROTOCOL_VIOLATION);
    }

    #[test]
    fn local_retire_already_retired_is_noop() {
        let mut l = LocalConnIds::new(vec![0], 4);
        l.issue(vec![1], token(1)).unwrap();
        l.retire(1).unwrap();
        // Retransmitted RETIRE for the same (already gone) ID is tolerated.
        assert!(l.retire(1).is_ok());
    }

    #[test]
    fn local_accepts_issued_ids() {
        let mut l = LocalConnIds::new(vec![0xaa], 4);
        l.issue(vec![0xbb, 0xcc], token(1)).unwrap();
        assert!(l.accepts(&[0xaa]));
        assert!(l.accepts(&[0xbb, 0xcc]));
        assert!(!l.accepts(&[0xff]));
    }
}
