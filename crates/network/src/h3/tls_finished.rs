//! TLS 1.3 `Finished` message MAC (RFC 8446 §4.4.4) — slice 18 of the HTTP/3
//! sprint.
//!
//! The `Finished` message is the last message of each side's handshake flight
//! and is the handshake's key confirmation: it proves the sender holds the
//! negotiated traffic secret and binds the entire preceding transcript. QUIC
//! carries these `Finished` messages inside CRYPTO frames exactly like TLS over
//! TCP (RFC 9001 §4), so a QUIC client must verify the server's `Finished`
//! before it trusts the 1-RTT keys, and produce its own `Finished` to complete
//! the handshake.
//!
//! Given a base traffic secret `BaseKey` (RFC 8446 §4.4.4):
//!
//! ```text
//! finished_key = HKDF-Expand-Label(BaseKey, "finished", "", Hash.length)
//! verify_data  = HMAC(finished_key, Transcript-Hash(Handshake Context,
//!                                                    Certificate*,
//!                                                    CertificateVerify*))
//! ```
//!
//! `BaseKey` is the *sender's* handshake traffic secret: the
//! `server_handshake_traffic_secret` ([`super::tls_schedule::HandshakeTrafficSecrets::server`])
//! for the server's `Finished`, and the `client_handshake_traffic_secret`
//! ([`super::tls_schedule::HandshakeTrafficSecrets::client`]) for the client's.
//! The transcript hash runs over every handshake message up to but **not**
//! including the `Finished` being computed — for the server's `Finished` that is
//! `ClientHello…CertificateVerify`, for the client's it additionally includes the
//! server's `Finished`. This module takes that already-computed transcript hash
//! as input; the message codec that produces the transcript bytes is
//! [`super::tls_message`] and the traffic secrets come from
//! [`super::tls_schedule`].
//!
//! ## Scope
//!
//! - [`finished_key`] — the per-direction `finished_key` derivation.
//! - [`finished_verify_data`] — the `verify_data` MAC a sender writes into its
//!   own `Finished`.
//! - [`verify_finished`] — the constant-time check a receiver runs against a
//!   peer's `Finished` (a MAC mismatch is a fatal `decrypt_error`, RFC 8446
//!   §4.4.4).
//!
//! All functions are pure and over SHA-256 (the KDF hash of the QUIC v1 cipher
//! suites `AEAD_AES_128_GCM` / `AEAD_CHACHA20_POLY1305`), reusing the HMAC and
//! `HKDF-Expand-Label` primitives from [`super::key_schedule`]. No new
//! dependency, no IO, no handshake state machine.
//!
//! Out of scope (later slices): the `CertificateVerify` signature (RFC 8446
//! §4.4.3, a public-key operation over the same transcript, needs the peer's
//! certificate), the `AEAD_AES_256_GCM` (SHA-384) suite, and driving the
//! transcript from live CRYPTO-frame reassembly.

use super::key_schedule::{hkdf_expand_label, hmac_sha256};
use super::tls_schedule::HASH_LEN;

/// The length, in bytes, of a `Finished` `verify_data` value. With SHA-256 as
/// the KDF hash this is `Hash.length` = 32 octets (RFC 8446 §4.4.4: `verify_data`
/// is `Hash.length` long, and the HMAC output equals the hash length).
pub const FINISHED_VERIFY_DATA_LEN: usize = HASH_LEN;

/// The `finished_key` for one direction:
/// `HKDF-Expand-Label(BaseKey, "finished", "", Hash.length)` (RFC 8446 §4.4.4).
///
/// `base_key` is the sender's handshake traffic secret (see the module docs).
/// The result keys the HMAC that produces or verifies a `Finished` `verify_data`.
#[must_use]
pub fn finished_key(base_key: &[u8; HASH_LEN]) -> [u8; HASH_LEN] {
    let out = hkdf_expand_label(base_key, b"finished", b"", HASH_LEN);
    let mut key = [0u8; HASH_LEN];
    // `hkdf_expand_label` always returns exactly `out_len` bytes.
    key.copy_from_slice(&out[..HASH_LEN]);
    key
}

/// The `verify_data` a sender writes into its `Finished` message:
/// `HMAC(finished_key, transcript_hash)` (RFC 8446 §4.4.4).
///
/// `base_key` is the sender's handshake traffic secret; `transcript_hash` is
/// `Transcript-Hash` over every handshake message preceding this `Finished`
/// (32 octets under SHA-256, but any length is accepted as the HMAC message).
#[must_use]
pub fn finished_verify_data(base_key: &[u8; HASH_LEN], transcript_hash: &[u8]) -> [u8; FINISHED_VERIFY_DATA_LEN] {
    hmac_sha256(&finished_key(base_key), transcript_hash)
}

/// Verify a peer's `Finished`: recompute the expected `verify_data` from the
/// peer's base traffic secret and the transcript hash, and compare it against
/// the received value in constant time (RFC 8446 §4.4.4 — a mismatch is a fatal
/// `decrypt_error`).
///
/// `base_key` is the *peer's* handshake traffic secret (the direction whose
/// `Finished` is being checked). The comparison is length-checked first
/// (a wrong-length `verify_data` can never match), then runs in constant time so
/// verification does not leak how many leading bytes matched.
#[must_use]
pub fn verify_finished(base_key: &[u8; HASH_LEN], transcript_hash: &[u8], received: &[u8]) -> bool {
    let expected = finished_verify_data(base_key, transcript_hash);
    ct_eq(&expected, received)
}

/// Constant-time byte-slice equality: returns `true` iff the slices have equal
/// length and equal contents, taking time independent of *where* they first
/// differ. Used for MAC comparison so a timing side channel cannot reveal the
/// correct `verify_data` byte by byte.
#[must_use]
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a hex string into bytes for comparing against RFC test vectors.
    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    /// Convert a 32-byte hex string to a fixed array for secret inputs.
    fn hex32(s: &str) -> [u8; HASH_LEN] {
        let v = hex(s);
        let mut a = [0u8; HASH_LEN];
        a.copy_from_slice(&v);
        a
    }

    // ── RFC 8448 §3 "Simple 1-RTT Handshake" vectors ───────────────────────
    //
    // The server's handshake traffic secret (BaseKey) and the transcript hash
    // of ClientHello…CertificateVerify (the input to the server's Finished MAC),
    // reproduced from the handshake messages in RFC 8448 §3.

    /// `server_handshake_traffic_secret` (RFC 8448 §3).
    fn server_hs_secret() -> [u8; HASH_LEN] {
        hex32("b67b7d690cc16c4e75e54213cb2d37b4e9c912bcded9105d42befd59d391ad38")
    }

    /// `client_handshake_traffic_secret` (RFC 8448 §3).
    fn client_hs_secret() -> [u8; HASH_LEN] {
        hex32("b3eddb126e067f35a780b3abf45e2d8f3b1a950738f52e9600746a0e27a55a21")
    }

    /// `Transcript-Hash(ClientHello…CertificateVerify)` (RFC 8448 §3), the input
    /// to the server's Finished MAC. SHA-256 over the concatenated handshake
    /// messages from that trace.
    fn server_finished_transcript_hash() -> Vec<u8> {
        hex("edb7725fa7a3473b031ec8ef65a2485493900138a2b91291407d7951a06110ed")
    }

    #[test]
    fn server_finished_key_matches_rfc8448() {
        // RFC 8448 §3: HKDF-Expand-Label(server_hs_secret, "finished", "", 32).
        assert_eq!(
            finished_key(&server_hs_secret()).to_vec(),
            hex("008d3b66f816ea559f96b537e885c31fc068bf492c652f01f288a1d8cdc19fc8")
        );
    }

    #[test]
    fn client_finished_key_matches_rfc8448() {
        // RFC 8448 §3: HKDF-Expand-Label(client_hs_secret, "finished", "", 32).
        assert_eq!(
            finished_key(&client_hs_secret()).to_vec(),
            hex("b80ad01015fb2f0bd65ff7d4da5d6bf83f84821d1f87fdc7d3c75b5a7b42d9c4")
        );
    }

    #[test]
    fn server_finished_verify_data_matches_rfc8448() {
        // End-to-end RFC 8448 §3 vector: BaseKey + transcript hash → verify_data
        // in the server's Finished message.
        assert_eq!(
            finished_verify_data(&server_hs_secret(), &server_finished_transcript_hash()).to_vec(),
            hex("9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f0718")
        );
    }

    #[test]
    fn verify_finished_accepts_the_rfc8448_finished() {
        // The receiver-side check accepts the exact verify_data the sender
        // computed over the same BaseKey and transcript.
        let th = server_finished_transcript_hash();
        let received = hex("9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f0718");
        assert!(verify_finished(&server_hs_secret(), &th, &received));
    }

    #[test]
    fn verify_finished_rejects_a_tampered_mac() {
        // Flipping any bit of the received verify_data must fail verification.
        let th = server_finished_transcript_hash();
        let mut received = hex("9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f0718");
        received[0] ^= 0x01;
        assert!(!verify_finished(&server_hs_secret(), &th, &received));
    }

    #[test]
    fn verify_finished_rejects_a_wrong_length_mac() {
        // A verify_data of the wrong length can never match, even if it is a
        // prefix of the correct value.
        let th = server_finished_transcript_hash();
        let short = hex("9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f07"); // 31 bytes
        assert_eq!(short.len(), FINISHED_VERIFY_DATA_LEN - 1);
        assert!(!verify_finished(&server_hs_secret(), &th, &short));
        assert!(!verify_finished(&server_hs_secret(), &th, &[]));
    }

    #[test]
    fn verify_finished_rejects_a_changed_transcript() {
        // The Finished binds the transcript: verifying the correct MAC against a
        // different transcript hash must fail (this is what detects a
        // man-in-the-middle altering earlier handshake messages).
        let received = hex("9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f0718");
        let mut th = server_finished_transcript_hash();
        th[0] ^= 0xff;
        assert!(!verify_finished(&server_hs_secret(), &th, &received));
    }

    #[test]
    fn verify_finished_rejects_the_wrong_direction_key() {
        // The server's verify_data must not verify under the client's base key:
        // the two directions use different finished_keys.
        let th = server_finished_transcript_hash();
        let received = hex("9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f0718");
        assert!(!verify_finished(&client_hs_secret(), &th, &received));
    }

    #[test]
    fn finished_key_differs_per_direction() {
        // Client and server derive different finished_keys from their own
        // traffic secrets, so one side's Finished can never be forged by the
        // other's key material.
        assert_ne!(finished_key(&server_hs_secret()), finished_key(&client_hs_secret()));
    }

    #[test]
    fn verify_data_length_is_hash_length() {
        let vd = finished_verify_data(&server_hs_secret(), &server_finished_transcript_hash());
        assert_eq!(vd.len(), FINISHED_VERIFY_DATA_LEN);
        assert_eq!(FINISHED_VERIFY_DATA_LEN, HASH_LEN);
    }

    #[test]
    fn derivation_is_deterministic() {
        let a = finished_verify_data(&server_hs_secret(), &server_finished_transcript_hash());
        let b = finished_verify_data(&server_hs_secret(), &server_finished_transcript_hash());
        assert_eq!(a, b);
    }

    #[test]
    fn round_trip_a_synthetic_finished() {
        // A sender computes verify_data; the receiver, given the same base key
        // and transcript, accepts it — over an arbitrary (non-RFC) transcript.
        use super::super::tls_schedule::transcript_hash;
        let base = hex32("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff");
        let th = transcript_hash(b"ClientHello..server Finished");
        let vd = finished_verify_data(&base, &th);
        assert!(verify_finished(&base, &th, &vd));
        // But a different base key rejects it.
        let other = hex32("ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100");
        assert!(!verify_finished(&other, &th, &vd));
    }

    #[test]
    fn ct_eq_matches_semantic_equality() {
        // The constant-time comparison agrees with `==` on equal, unequal-content,
        // and unequal-length inputs.
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(ct_eq(b"", b""));
    }
}
