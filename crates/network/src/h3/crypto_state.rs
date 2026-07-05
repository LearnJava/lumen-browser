//! QUIC 1-RTT key update state machine (RFC 9001 §6).
//!
//! Once the handshake completes both endpoints protect their 1-RTT (Application
//! Data) packets with keys derived from the TLS application traffic secrets
//! (slice 15, [`tls_schedule::ApplicationTrafficSecrets`](super::tls_schedule::ApplicationTrafficSecrets)).
//! Unlike the Initial and Handshake levels, whose keys live for the handshake
//! only, the 1-RTT keys are *updated* over the life of the connection: an
//! endpoint periodically rotates each direction's traffic secret to the next
//! generation ([`key_schedule::next_generation_secret`](super::key_schedule::next_generation_secret),
//! `HKDF-Expand-Label(secret, "quic ku", …)`) to bound the number of packets
//! protected with any one key (RFC 9001 §6.6). A single **Key Phase** bit in the
//! short-header first byte (RFC 9000 §17.3.1) tells the peer which generation
//! protects a packet, flipping on every update.
//!
//! [`OneRttKeyState`] is the pure state machine that owns this rotation for a
//! QUIC client. It holds our send keys (client direction) and the peer's receive
//! keys (server direction) at the current generation, pre-derives the *next*
//! receive generation so a phase-flipped packet can be trial-decrypted, and
//! retains the *previous* receive generation for a short window so a packet
//! reordered across an update still decrypts (RFC 9001 §6.3). It enforces the
//! §6.1 rules on when an endpoint may initiate an update — only after the
//! handshake is confirmed, and never a second update until the first is
//! acknowledged — and implements the §6.2 responder logic: detecting a
//! peer-initiated update from a differing Key Phase bit and updating our own send
//! keys in response (unless we initiated the update ourselves).
//!
//! The header-protection key `hp` is **not** rotated by a key update (RFC 9001
//! §6.1): [`OneRttKeyState`] carries each generation's `hp` unchanged and only
//! re-derives the AEAD `key` / `iv` from the advanced secret.
//!
//! Like every other slice this is pure state: it holds no clock and does no IO.
//! The caller drives it with decoded events — a received packet's Key Phase bit
//! and packet number, an acknowledgement's Key Phase, the retirement timer firing
//! ([`timer`](super::timer) owns the actual deadline) — and reads back which key
//! set to hand [`packet_crypt`](super::packet_crypt) for each packet. Key
//! discarding for the Initial / Handshake levels (RFC 9001 §4.9) belongs to the
//! connection driver, not here — those levels do not update.

use super::key_schedule::{PacketProtectionKeys, next_generation_secret};
use super::tls_schedule::DirectionalKeys;

/// The confidentiality limit for `AEAD_AES_128_GCM` (RFC 9001 §6.6): the number
/// of packets that may be protected with a single key before the key **must** be
/// updated (or the connection closed). QUIC v1 mandates this suite for Initial
/// packets and it is the suite this stack negotiates for 1-RTT, so it is the only
/// limit tracked here.
pub const AES_128_GCM_CONFIDENTIALITY_LIMIT: u64 = 1 << 23;

/// The Key Phase bit carried in a short-header packet's first byte (RFC 9000
/// §17.3.1, RFC 9001 §6): which generation of 1-RTT keys protects the packet. It
/// starts at zero and flips on every key update, so a differing value on a
/// received packet signals the peer has updated its keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPhase(bool);

impl KeyPhase {
    /// The initial Key Phase, used for the first generation of 1-RTT keys.
    #[must_use]
    pub fn zero() -> Self {
        KeyPhase(false)
    }

    /// The Key Phase for a raw bit read from (or to write into) a packet's first
    /// byte.
    #[must_use]
    pub fn from_bit(bit: bool) -> Self {
        KeyPhase(bit)
    }

    /// The raw bit to place in (or compare against) a packet's Key Phase bit.
    #[must_use]
    pub fn bit(self) -> bool {
        self.0
    }

    /// The opposite phase — the value after one key update.
    #[must_use]
    fn toggled(self) -> Self {
        KeyPhase(!self.0)
    }
}

/// Which generation of receive keys to try for an incoming 1-RTT packet, decided
/// by [`OneRttKeyState::recv_decision`] from the packet's Key Phase bit and
/// number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvKeyDecision {
    /// The Key Phase matches the current generation — decrypt with the current
    /// receive keys.
    Current,
    /// The Key Phase differs and the packet is at or beyond the current phase's
    /// first packet: a peer-initiated key update (RFC 9001 §6.2). Trial-decrypt
    /// with the next-generation receive keys; on success the caller commits the
    /// update via [`OneRttKeyState::on_packet_decrypted`].
    Next,
    /// The Key Phase differs but the packet predates the last update: a packet
    /// reordered across the update (RFC 9001 §6.3). Decrypt with the retained
    /// previous-generation keys.
    Previous,
}

/// Why an attempt to initiate a key update was refused (RFC 9001 §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyUpdateError {
    /// The handshake is not yet confirmed. An endpoint MUST NOT initiate a key
    /// update before handshake confirmation (RFC 9001 §6.1, §4.1.2).
    HandshakeNotConfirmed,
    /// The previous key update has not been acknowledged. An endpoint MUST NOT
    /// initiate a subsequent update until it has received an acknowledgement for a
    /// packet sent with the current key phase (RFC 9001 §6.1).
    PreviousUpdateUnacknowledged,
}

impl core::fmt::Display for KeyUpdateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KeyUpdateError::HandshakeNotConfirmed => {
                f.write_str("cannot initiate a key update before the handshake is confirmed")
            }
            KeyUpdateError::PreviousUpdateUnacknowledged => {
                f.write_str("cannot initiate a key update until the previous one is acknowledged")
            }
        }
    }
}

impl std::error::Error for KeyUpdateError {}

/// Advance a key set to the next generation for a key update (RFC 9001 §6.1):
/// derive the next traffic secret and re-derive the AEAD `key` / `iv` from it,
/// keeping the header-protection key `hp` unchanged (it is not rotated).
fn advance_keys(current: &PacketProtectionKeys) -> PacketProtectionKeys {
    let next_secret = next_generation_secret(&current.secret);
    let mut next = PacketProtectionKeys::aes_128_gcm_from_secret(next_secret);
    // The header-protection key survives a key update untouched (RFC 9001 §6.1);
    // `aes_128_gcm_from_secret` re-derives one from the new secret, so restore it.
    next.hp.clone_from(&current.hp);
    next
}

/// The 1-RTT key state and key-update state machine for a QUIC client (RFC 9001
/// §6).
///
/// Seeded with the first-generation 1-RTT keys the TLS handshake produced, it
/// tracks the current send and receive keys, the next receive generation (for
/// trial-decrypting a phase-flipped packet), an optional retained previous
/// receive generation (for packets reordered across an update), and the Key Phase
/// bit for each direction. It answers which keys protect an outgoing packet, which
/// keys to try for an incoming one, and drives the update on either endpoint's
/// initiative.
#[derive(Debug, Clone)]
pub struct OneRttKeyState {
    /// Keys protecting our outgoing 1-RTT packets (client direction).
    send: PacketProtectionKeys,
    /// The Key Phase we stamp on outgoing packets.
    send_phase: KeyPhase,
    /// Keys decrypting incoming 1-RTT packets at the current generation (server
    /// direction).
    recv: PacketProtectionKeys,
    /// The next receive generation, pre-derived so a phase-flipped packet can be
    /// trial-decrypted before the update is committed (RFC 9001 §6.3).
    recv_next: PacketProtectionKeys,
    /// The previous receive generation, retained after an update so a packet
    /// reordered across it still decrypts (RFC 9001 §6.3); dropped by
    /// [`OneRttKeyState::retire_previous_recv_keys`].
    recv_prev: Option<PacketProtectionKeys>,
    /// The Key Phase we expect on incoming packets at the current generation.
    recv_phase: KeyPhase,
    /// Whether a packet in the current send phase has been acknowledged, gating a
    /// subsequent update (RFC 9001 §6.1).
    send_phase_acked: bool,
    /// Whether the handshake is confirmed, gating the first update (RFC 9001 §6.1).
    handshake_confirmed: bool,
    /// The lowest packet number received in the current receive phase, used to
    /// tell a peer-initiated update (at/above it) from a reordered old packet
    /// (below it) — RFC 9001 §6.3.
    recv_boundary: Option<u64>,
    /// Packets protected with the current send key, for the AEAD confidentiality
    /// limit (RFC 9001 §6.6).
    packets_sent_current_phase: u64,
}

impl OneRttKeyState {
    /// Build the 1-RTT key state from the first-generation send (our) and receive
    /// (peer) key sets. The next receive generation is derived eagerly; both
    /// directions start at [`KeyPhase::zero`].
    #[must_use]
    pub fn new(send: PacketProtectionKeys, recv: PacketProtectionKeys) -> Self {
        let recv_next = advance_keys(&recv);
        Self {
            send,
            send_phase: KeyPhase::zero(),
            recv,
            recv_next,
            recv_prev: None,
            recv_phase: KeyPhase::zero(),
            send_phase_acked: false,
            handshake_confirmed: false,
            recv_boundary: None,
            packets_sent_current_phase: 0,
        }
    }

    /// Build the 1-RTT key state for a client from the [`DirectionalKeys`] the TLS
    /// handshake derived: the client direction protects our sends, the server
    /// direction our receives (RFC 9001 §5.1).
    #[must_use]
    pub fn from_client_keys(keys: DirectionalKeys) -> Self {
        Self::new(keys.client, keys.server)
    }

    /// Mark the handshake confirmed (RFC 9001 §4.1.2), permitting key updates
    /// (RFC 9001 §6.1).
    pub fn confirm_handshake(&mut self) {
        self.handshake_confirmed = true;
    }

    /// The keys protecting our outgoing 1-RTT packets.
    #[must_use]
    pub fn send_keys(&self) -> &PacketProtectionKeys {
        &self.send
    }

    /// The Key Phase bit to stamp on the next outgoing 1-RTT packet.
    #[must_use]
    pub fn send_phase(&self) -> KeyPhase {
        self.send_phase
    }

    /// The current receive keys (for the current Key Phase).
    #[must_use]
    pub fn recv_keys(&self) -> &PacketProtectionKeys {
        &self.recv
    }

    /// The Key Phase we currently expect on incoming 1-RTT packets.
    #[must_use]
    pub fn recv_phase(&self) -> KeyPhase {
        self.recv_phase
    }

    /// Whether a subsequent key update may be initiated right now (RFC 9001 §6.1):
    /// the handshake is confirmed and the current send phase has been acknowledged.
    #[must_use]
    pub fn can_initiate_key_update(&self) -> bool {
        self.handshake_confirmed && self.send_phase_acked
    }

    /// Initiate a key update (RFC 9001 §6.1): advance our send keys to the next
    /// generation and flip the send Key Phase. Subsequent packets are protected
    /// with the new keys; the peer detects the flipped phase and responds in kind.
    ///
    /// # Errors
    ///
    /// [`KeyUpdateError::HandshakeNotConfirmed`] before the handshake is confirmed,
    /// or [`KeyUpdateError::PreviousUpdateUnacknowledged`] if the current send
    /// phase has not yet been acknowledged.
    pub fn initiate_key_update(&mut self) -> Result<(), KeyUpdateError> {
        if !self.handshake_confirmed {
            return Err(KeyUpdateError::HandshakeNotConfirmed);
        }
        if !self.send_phase_acked {
            return Err(KeyUpdateError::PreviousUpdateUnacknowledged);
        }
        self.rotate_send_keys();
        Ok(())
    }

    /// Record a sent 1-RTT packet for the AEAD confidentiality limit (RFC 9001
    /// §6.6). Call once per packet protected with the current send keys.
    pub fn on_packet_sent(&mut self) {
        self.packets_sent_current_phase = self.packets_sent_current_phase.saturating_add(1);
    }

    /// The number of packets protected with the current send key (RFC 9001 §6.6).
    #[must_use]
    pub fn packets_sent_current_phase(&self) -> u64 {
        self.packets_sent_current_phase
    }

    /// Whether the AEAD confidentiality limit for the current send key has been
    /// reached (RFC 9001 §6.6): the endpoint MUST initiate a key update (or close
    /// the connection) rather than protect another packet with this key.
    #[must_use]
    pub fn confidentiality_limit_reached(&self) -> bool {
        self.packets_sent_current_phase >= AES_128_GCM_CONFIDENTIALITY_LIMIT
    }

    /// Record an acknowledgement of a packet sent with `acked_phase` (RFC 9001
    /// §6.1). Once a packet in the current send phase is acknowledged a subsequent
    /// key update may be initiated.
    pub fn on_ack(&mut self, acked_phase: KeyPhase) {
        if acked_phase == self.send_phase {
            self.send_phase_acked = true;
        }
    }

    /// Decide which receive-key generation to try for an incoming 1-RTT packet
    /// from its Key Phase bit and packet number (RFC 9001 §6.2, §6.3). The caller
    /// fetches the keys with [`OneRttKeyState::keys_for`], AEAD-decrypts, and — on
    /// success — reports the outcome to [`OneRttKeyState::on_packet_decrypted`].
    #[must_use]
    pub fn recv_decision(&self, incoming_phase: KeyPhase, packet_number: u64) -> RecvKeyDecision {
        if incoming_phase == self.recv_phase {
            RecvKeyDecision::Current
        } else if self.recv_prev.is_some() && self.recv_boundary.is_some_and(|b| packet_number < b) {
            // A phase-flipped packet numbered below the current phase's first
            // packet is a packet reordered from the previous phase (RFC 9001 §6.3).
            RecvKeyDecision::Previous
        } else {
            // Otherwise a differing phase is a peer-initiated key update (RFC 9001
            // §6.2): try the next generation and commit on success.
            RecvKeyDecision::Next
        }
    }

    /// The receive keys for a [`RecvKeyDecision`], or `None` for
    /// [`RecvKeyDecision::Previous`] when no previous generation is retained.
    #[must_use]
    pub fn keys_for(&self, decision: RecvKeyDecision) -> Option<&PacketProtectionKeys> {
        match decision {
            RecvKeyDecision::Current => Some(&self.recv),
            RecvKeyDecision::Next => Some(&self.recv_next),
            RecvKeyDecision::Previous => self.recv_prev.as_ref(),
        }
    }

    /// Commit the state change after a packet at `packet_number` decrypted under
    /// the keys `decision` selected (RFC 9001 §6.2, §6.3).
    ///
    /// - [`RecvKeyDecision::Current`] records the phase boundary.
    /// - [`RecvKeyDecision::Previous`] is a reordered old packet — no change.
    /// - [`RecvKeyDecision::Next`] commits a key update: the next generation
    ///   becomes current, the current is retained as previous, a fresh next
    ///   generation is derived, and the receive phase flips. If we have not already
    ///   updated our send keys (i.e. we did not initiate this update), they are
    ///   advanced in response (RFC 9001 §6.2).
    ///
    /// Call only after the AEAD decryption actually succeeded.
    pub fn on_packet_decrypted(&mut self, decision: RecvKeyDecision, packet_number: u64) {
        match decision {
            RecvKeyDecision::Current => {
                self.recv_boundary = Some(match self.recv_boundary {
                    Some(b) => b.min(packet_number),
                    None => packet_number,
                });
            }
            RecvKeyDecision::Previous => {}
            RecvKeyDecision::Next => {
                // Promote the next generation to current, retain the old current as
                // the previous generation, and derive the following generation.
                let old_recv = std::mem::replace(&mut self.recv, self.recv_next.clone());
                self.recv_next = advance_keys(&self.recv);
                self.recv_prev = Some(old_recv);
                self.recv_phase = self.recv_phase.toggled();
                self.recv_boundary = Some(packet_number);
                // Respond by updating our send keys unless the phases already match,
                // which means we initiated this update ourselves (RFC 9001 §6.2).
                if self.send_phase != self.recv_phase {
                    self.rotate_send_keys();
                }
            }
        }
    }

    /// Whether a previous receive generation is still retained for reordered
    /// packets (RFC 9001 §6.3).
    #[must_use]
    pub fn has_previous_recv_keys(&self) -> bool {
        self.recv_prev.is_some()
    }

    /// Discard the retained previous receive generation (RFC 9001 §6.3). The
    /// connection driver calls this once the retention period (typically three
    /// times the PTO) after an update has elapsed; the deadline lives with the
    /// connection timer, not here.
    pub fn retire_previous_recv_keys(&mut self) {
        self.recv_prev = None;
    }

    /// Advance our send keys to the next generation and flip the send Key Phase,
    /// resetting the acknowledgement gate and the per-phase packet counter.
    fn rotate_send_keys(&mut self) {
        self.send = advance_keys(&self.send);
        self.send_phase = self.send_phase.toggled();
        self.send_phase_acked = false;
        self.packets_sent_current_phase = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A distinct traffic secret for tests, filled with `seed` in every byte.
    fn secret(seed: u8) -> [u8; 32] {
        [seed; 32]
    }

    /// Build a key set from a secret with a recognisable header-protection key so
    /// the "hp is not rotated" invariant is observable.
    fn keys(seed: u8) -> PacketProtectionKeys {
        let mut k = PacketProtectionKeys::aes_128_gcm_from_secret(secret(seed));
        // Mark the hp so we can assert it survives a key update untouched.
        k.hp = vec![0xAB; k.hp.len()];
        k
    }

    /// A state seeded with two distinct generations for send / recv.
    fn state() -> OneRttKeyState {
        OneRttKeyState::new(keys(0x11), keys(0x22))
    }

    #[test]
    fn new_starts_at_phase_zero_without_previous_keys() {
        let s = state();
        assert_eq!(s.send_phase(), KeyPhase::zero());
        assert_eq!(s.recv_phase(), KeyPhase::zero());
        assert!(!s.has_previous_recv_keys());
        assert_eq!(s.packets_sent_current_phase(), 0);
    }

    #[test]
    fn recv_next_is_one_generation_ahead_of_recv() {
        let s = state();
        // The pre-derived next generation must equal advancing the current recv
        // secret with the "quic ku" label.
        let expected = next_generation_secret(&s.recv.secret);
        assert_eq!(s.keys_for(RecvKeyDecision::Next).unwrap().secret, expected);
    }

    #[test]
    fn from_client_keys_maps_client_to_send_and_server_to_recv() {
        let dk = DirectionalKeys {
            client: keys(0x33),
            server: keys(0x44),
        };
        let s = OneRttKeyState::from_client_keys(dk);
        assert_eq!(s.send_keys().secret, secret(0x33));
        assert_eq!(s.recv_keys().secret, secret(0x44));
    }

    #[test]
    fn advance_keys_rotates_key_and_iv_but_keeps_hp() {
        let base = keys(0x55);
        let next = advance_keys(&base);
        // Secret advances; key and iv follow it; hp is carried over unchanged.
        assert_eq!(next.secret, next_generation_secret(&base.secret));
        assert_ne!(next.key, base.key);
        assert_ne!(next.iv, base.iv);
        assert_eq!(next.hp, base.hp);
    }

    #[test]
    fn initiate_rejected_before_handshake_confirmed() {
        let mut s = state();
        assert_eq!(
            s.initiate_key_update(),
            Err(KeyUpdateError::HandshakeNotConfirmed)
        );
    }

    #[test]
    fn initiate_rejected_before_current_phase_acked() {
        let mut s = state();
        s.confirm_handshake();
        assert_eq!(
            s.initiate_key_update(),
            Err(KeyUpdateError::PreviousUpdateUnacknowledged)
        );
    }

    #[test]
    fn initiate_flips_phase_and_rotates_send_keys() {
        let mut s = state();
        s.confirm_handshake();
        s.on_ack(KeyPhase::zero());
        let before = s.send_keys().secret;
        s.initiate_key_update().expect("update allowed");
        assert_eq!(s.send_phase(), KeyPhase::zero().toggled());
        assert_eq!(s.send_keys().secret, next_generation_secret(&before));
        // The new phase is not yet acknowledged, so a further update is refused.
        assert!(!s.can_initiate_key_update());
    }

    #[test]
    fn on_ack_of_wrong_phase_does_not_enable_update() {
        let mut s = state();
        s.confirm_handshake();
        // Acknowledging a phase we are not currently in must not open the gate.
        s.on_ack(KeyPhase::zero().toggled());
        assert!(!s.can_initiate_key_update());
        s.on_ack(KeyPhase::zero());
        assert!(s.can_initiate_key_update());
    }

    #[test]
    fn packet_sent_counter_and_confidentiality_limit() {
        let mut s = state();
        assert!(!s.confidentiality_limit_reached());
        for _ in 0..3 {
            s.on_packet_sent();
        }
        assert_eq!(s.packets_sent_current_phase(), 3);
        assert!(!s.confidentiality_limit_reached());
    }

    #[test]
    fn rotating_send_keys_resets_packet_counter() {
        let mut s = state();
        s.confirm_handshake();
        s.on_ack(KeyPhase::zero());
        s.on_packet_sent();
        s.on_packet_sent();
        s.initiate_key_update().expect("update allowed");
        assert_eq!(s.packets_sent_current_phase(), 0);
    }

    #[test]
    fn matching_phase_uses_current_keys() {
        let s = state();
        assert_eq!(
            s.recv_decision(KeyPhase::zero(), 10),
            RecvKeyDecision::Current
        );
    }

    #[test]
    fn differing_phase_without_previous_is_a_peer_update() {
        let s = state();
        // No previous generation retained, so a flipped phase can only be a new
        // peer-initiated update.
        assert_eq!(
            s.recv_decision(KeyPhase::zero().toggled(), 10),
            RecvKeyDecision::Next
        );
    }

    #[test]
    fn peer_initiated_update_commits_and_responds() {
        let mut s = state();
        let recv_gen0 = s.recv_keys().secret;
        let recv_gen1 = s.keys_for(RecvKeyDecision::Next).unwrap().secret;
        let send_gen0 = s.send_keys().secret;

        // Peer sends a phase-1 packet: detect the update and decrypt with next keys.
        let d = s.recv_decision(KeyPhase::zero().toggled(), 100);
        assert_eq!(d, RecvKeyDecision::Next);
        s.on_packet_decrypted(d, 100);

        // Receive generation advanced, the phase flipped, the old generation is
        // retained, and we responded by advancing our send keys.
        assert_eq!(s.recv_phase(), KeyPhase::zero().toggled());
        assert_eq!(s.recv_keys().secret, recv_gen1);
        assert!(s.has_previous_recv_keys());
        assert_eq!(s.keys_for(RecvKeyDecision::Previous).unwrap().secret, recv_gen0);
        assert_eq!(s.send_phase(), KeyPhase::zero().toggled());
        assert_eq!(s.send_keys().secret, next_generation_secret(&send_gen0));
    }

    #[test]
    fn self_initiated_update_does_not_double_rotate_send() {
        let mut s = state();
        s.confirm_handshake();
        s.on_ack(KeyPhase::zero());
        s.initiate_key_update().expect("update allowed");
        let send_after_initiate = s.send_keys().secret;
        assert_eq!(s.send_phase(), KeyPhase::zero().toggled());

        // The peer responds with a phase-1 packet. We commit the receive update but
        // must NOT rotate send again (phases already match).
        let d = s.recv_decision(KeyPhase::zero().toggled(), 200);
        s.on_packet_decrypted(d, 200);
        assert_eq!(s.recv_phase(), KeyPhase::zero().toggled());
        assert_eq!(s.send_keys().secret, send_after_initiate);
        assert_eq!(s.send_phase(), KeyPhase::zero().toggled());
    }

    #[test]
    fn reordered_old_packet_below_boundary_uses_previous_keys() {
        let mut s = state();
        // Establish the current phase boundary at packet 50.
        let d0 = s.recv_decision(KeyPhase::zero(), 50);
        s.on_packet_decrypted(d0, 50);
        // Peer updates at packet 100.
        let d1 = s.recv_decision(KeyPhase::zero().toggled(), 100);
        s.on_packet_decrypted(d1, 100);
        // A phase-0 packet numbered below the new boundary is a reordered old one.
        assert_eq!(s.recv_decision(KeyPhase::zero(), 40), RecvKeyDecision::Previous);
    }

    #[test]
    fn phase_flipped_packet_at_or_above_boundary_is_a_further_update() {
        let mut s = state();
        let d1 = s.recv_decision(KeyPhase::zero().toggled(), 100);
        s.on_packet_decrypted(d1, 100);
        // After the update the boundary is 100; a phase-0 packet at/above it is not
        // a reordered old packet — it is treated as the next update to trial.
        assert_eq!(s.recv_decision(KeyPhase::zero(), 150), RecvKeyDecision::Next);
    }

    #[test]
    fn retire_previous_recv_keys_drops_the_generation() {
        let mut s = state();
        let d = s.recv_decision(KeyPhase::zero().toggled(), 100);
        s.on_packet_decrypted(d, 100);
        assert!(s.has_previous_recv_keys());
        s.retire_previous_recv_keys();
        assert!(!s.has_previous_recv_keys());
        assert!(s.keys_for(RecvKeyDecision::Previous).is_none());
    }

    #[test]
    fn two_consecutive_self_initiated_updates() {
        let mut s = state();
        s.confirm_handshake();
        let send_gen0 = s.send_keys().secret;

        // First update.
        s.on_ack(KeyPhase::zero());
        s.initiate_key_update().expect("first update");
        let send_gen1 = next_generation_secret(&send_gen0);
        assert_eq!(s.send_keys().secret, send_gen1);
        assert_eq!(s.send_phase(), KeyPhase::from_bit(true));

        // Cannot update again until the new phase is acknowledged.
        assert_eq!(
            s.initiate_key_update(),
            Err(KeyUpdateError::PreviousUpdateUnacknowledged)
        );

        // Second update after the phase-1 ack.
        s.on_ack(KeyPhase::from_bit(true));
        s.initiate_key_update().expect("second update");
        assert_eq!(s.send_keys().secret, next_generation_secret(&send_gen1));
        assert_eq!(s.send_phase(), KeyPhase::from_bit(false));
    }

    #[test]
    fn current_phase_boundary_tracks_the_lowest_packet_number() {
        let mut s = state();
        let d1 = s.recv_decision(KeyPhase::zero(), 80);
        s.on_packet_decrypted(d1, 80);
        let d2 = s.recv_decision(KeyPhase::zero(), 60);
        s.on_packet_decrypted(d2, 60);
        // Update at 100; a reordered packet at 70 is still below the boundary (60),
        // so `70 < 60` is false — it is not classified as an old-generation packet.
        let du = s.recv_decision(KeyPhase::zero().toggled(), 100);
        s.on_packet_decrypted(du, 100);
        // Boundary of the new phase is 100; a phase-0 packet at 70 < 100 → previous.
        assert_eq!(s.recv_decision(KeyPhase::zero(), 70), RecvKeyDecision::Previous);
    }

    #[test]
    fn key_update_error_displays() {
        // Exercise the Display impl so the error can be surfaced in logs.
        assert!(
            !KeyUpdateError::HandshakeNotConfirmed
                .to_string()
                .is_empty()
        );
        assert!(
            !KeyUpdateError::PreviousUpdateUnacknowledged
                .to_string()
                .is_empty()
        );
    }
}
