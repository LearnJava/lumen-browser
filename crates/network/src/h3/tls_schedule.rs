//! TLS 1.3 key schedule — RFC 8446 §7.1 — that produces the QUIC Handshake and
//! 1-RTT (application) traffic secrets on top of the HKDF primitives from
//! [`super::key_schedule`].
//!
//! QUIC v1 reuses the TLS 1.3 key schedule verbatim (RFC 9001 §5): once the
//! handshake negotiates the `(EC)DHE` shared secret and the two endpoints have
//! the same running transcript, both derive the same per-encryption-level
//! traffic secrets, and each traffic secret is turned into a QUIC packet-
//! protection key set by [`super::key_schedule::PacketProtectionKeys`]. Slice 13
//! wired the *Initial* secrets (derivable without TLS, from the client's
//! Destination Connection ID); this slice adds the rest of the schedule — the
//! secrets TLS negotiates for the Handshake and 1-RTT encryption levels.
//!
//! ## The schedule (RFC 8446 §7.1)
//!
//! ```text
//!              0
//!              |
//!              v
//!    PSK ->  HKDF-Extract = Early Secret
//!              |
//!              v
//!        Derive-Secret(., "derived", "")
//!              |
//!              v
//!    (EC)DHE -> HKDF-Extract = Handshake Secret
//!              |
//!              +-> Derive-Secret(., "c hs traffic", CH..SH) = client_hs
//!              +-> Derive-Secret(., "s hs traffic", CH..SH) = server_hs
//!              |
//!              v
//!        Derive-Secret(., "derived", "")
//!              |
//!              v
//!    0 ->    HKDF-Extract = Master Secret
//!              |
//!              +-> Derive-Secret(., "c ap traffic", CH..SF) = client_ap
//!              +-> Derive-Secret(., "s ap traffic", CH..SF) = server_ap
//!              +-> Derive-Secret(., "exp master",  CH..SF) = exporter
//!              +-> Derive-Secret(., "res master",  CH..CF) = resumption
//! ```
//!
//! where `Derive-Secret(Secret, Label, Messages) =
//! HKDF-Expand-Label(Secret, Label, Transcript-Hash(Messages), Hash.length)`
//! (RFC 8446 §7.1) and `Transcript-Hash` is the KDF hash (SHA-256 here) over the
//! concatenated handshake messages.
//!
//! ## Scope
//!
//! - The full non-PSK key schedule: [`early_secret`], [`handshake_secret`],
//!   [`master_secret`], and the traffic-secret derivations
//!   ([`HandshakeTrafficSecrets`], [`ApplicationTrafficSecrets`]).
//! - `Derive-Secret` ([`derive_secret`]) and `Transcript-Hash`
//!   ([`transcript_hash`]) over SHA-256, the KDF hash of the QUIC v1 cipher
//!   suites `AEAD_AES_128_GCM` / `AEAD_CHACHA20_POLY1305`.
//! - Bridging a traffic secret to QUIC keys reuses
//!   [`super::key_schedule::PacketProtectionKeys::aes_128_gcm_from_secret`]; the
//!   1-RTT key update reuses [`super::key_schedule::next_generation_secret`].
//!
//! ## Out of scope (deferred to higher slices)
//!
//! - The TLS 1.3 handshake state machine itself (parsing ClientHello /
//!   ServerHello / EncryptedExtensions / Certificate / Finished, running the
//!   X25519 key agreement, and feeding the running transcript). This slice takes
//!   the `(EC)DHE` shared secret and the transcript hashes as inputs; the IO and
//!   message codecs that produce them are later slices.
//! - `Finished`-message verification (the `finished_key` HMAC) and the
//!   `AEAD_AES_256_GCM` (SHA-384) suite.
//! - No IO. Pure functions over byte slices.

use super::key_schedule::{PacketProtectionKeys, hkdf_expand_label, hkdf_extract};
use sha2::{Digest, Sha256};

/// The KDF hash output size in bytes (SHA-256) — every secret in this schedule
/// is exactly this wide (RFC 8446 §7.1: secrets are `Hash.length` octets).
pub const HASH_LEN: usize = 32;

// ── Transcript-Hash + Derive-Secret (RFC 8446 §7.1) ─────────────────────────

/// `Transcript-Hash(messages)` (RFC 8446 §4.4.1) — the KDF hash (SHA-256) over
/// the concatenation of the handshake messages exchanged so far. Callers pass
/// the already-concatenated message bytes.
#[must_use]
pub fn transcript_hash(messages: &[u8]) -> [u8; HASH_LEN] {
    let mut h = Sha256::new();
    h.update(messages);
    h.finalize().into()
}

/// `Derive-Secret(Secret, Label, Messages)` (RFC 8446 §7.1):
/// `HKDF-Expand-Label(Secret, Label, Transcript-Hash(Messages), Hash.length)`.
///
/// This variant takes the already-computed transcript **hash** (32 octets)
/// rather than the messages, because the handshake layer maintains the running
/// hash incrementally; [`transcript_hash`] produces it from raw messages when
/// needed. The empty-transcript case (`Messages = ""`, used by the two
/// `"derived"` steps) passes `transcript_hash(b"")`.
#[must_use]
pub fn derive_secret(secret: &[u8; HASH_LEN], label: &[u8], transcript_hash: &[u8]) -> [u8; HASH_LEN] {
    let out = hkdf_expand_label(secret, label, transcript_hash, HASH_LEN);
    let mut secret = [0u8; HASH_LEN];
    // `hkdf_expand_label` always returns exactly `out_len` bytes.
    secret.copy_from_slice(&out[..HASH_LEN]);
    secret
}

/// The transcript hash of the empty message list (`Transcript-Hash("")`), used
/// by the two `Derive-Secret(., "derived", "")` steps that transition between
/// the Early → Handshake and Handshake → Master extractions.
#[must_use]
fn empty_transcript_hash() -> [u8; HASH_LEN] {
    transcript_hash(&[])
}

// ── Early Secret (RFC 8446 §7.1) ────────────────────────────────────────────

/// The Early Secret with **no** PSK: `HKDF-Extract(0, 0)`, where both the salt
/// and the input keying material are `Hash.length` zero octets (RFC 8446 §7.1 —
/// "0" denotes a string of `Hash.length` zero bytes, and an absent PSK is
/// likewise `Hash.length` zeros). QUIC never uses an external PSK on the initial
/// handshake, so this is always the schedule's root.
#[must_use]
pub fn early_secret() -> [u8; HASH_LEN] {
    hkdf_extract(&[0u8; HASH_LEN], &[0u8; HASH_LEN])
}

// ── Handshake Secret + handshake traffic secrets (RFC 8446 §7.1) ────────────

/// The Handshake Secret: `HKDF-Extract(Derive-Secret(Early, "derived", ""),
/// (EC)DHE)` (RFC 8446 §7.1). `ecdhe` is the raw shared secret from the key
/// agreement (X25519 gives 32 octets, but any length is accepted as IKM).
#[must_use]
pub fn handshake_secret(ecdhe: &[u8]) -> [u8; HASH_LEN] {
    let derived = derive_secret(&early_secret(), b"derived", &empty_transcript_hash());
    hkdf_extract(&derived, ecdhe)
}

/// The two Handshake-level traffic secrets (RFC 8446 §7.1), one per direction,
/// derived from the Handshake Secret and the transcript hash of
/// `ClientHello…ServerHello`.
#[derive(Clone, PartialEq, Eq)]
pub struct HandshakeTrafficSecrets {
    /// `client_handshake_traffic_secret` — protects client→server Handshake
    /// packets (`Derive-Secret(., "c hs traffic", CH..SH)`).
    pub client: [u8; HASH_LEN],
    /// `server_handshake_traffic_secret` — protects server→client Handshake
    /// packets (`Derive-Secret(., "s hs traffic", CH..SH)`).
    pub server: [u8; HASH_LEN],
}

impl core::fmt::Debug for HandshakeTrafficSecrets {
    /// Redacts the secrets — logging them would leak the connection's Handshake
    /// keys. Only the presence of both directions is shown.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HandshakeTrafficSecrets")
            .field("client", &format_args!("<{HASH_LEN} bytes redacted>"))
            .field("server", &format_args!("<{HASH_LEN} bytes redacted>"))
            .finish()
    }
}

impl HandshakeTrafficSecrets {
    /// Derive both directions' Handshake traffic secrets from the Handshake
    /// Secret and the `ClientHello…ServerHello` transcript hash (RFC 8446 §7.1).
    #[must_use]
    pub fn derive(handshake_secret: &[u8; HASH_LEN], transcript_ch_sh: &[u8]) -> Self {
        Self {
            client: derive_secret(handshake_secret, b"c hs traffic", transcript_ch_sh),
            server: derive_secret(handshake_secret, b"s hs traffic", transcript_ch_sh),
        }
    }

    /// The QUIC Handshake-level packet-protection keys for both directions,
    /// bridging each traffic secret through the `AEAD_AES_128_GCM` labels
    /// (RFC 9001 §5.1).
    #[must_use]
    pub fn packet_keys(&self) -> DirectionalKeys {
        DirectionalKeys {
            client: PacketProtectionKeys::aes_128_gcm_from_secret(self.client),
            server: PacketProtectionKeys::aes_128_gcm_from_secret(self.server),
        }
    }
}

// ── Master Secret + application traffic secrets (RFC 8446 §7.1) ──────────────

/// The Master Secret: `HKDF-Extract(Derive-Secret(Handshake, "derived", ""), 0)`
/// (RFC 8446 §7.1), where the IKM is `Hash.length` zero octets.
#[must_use]
pub fn master_secret(handshake_secret: &[u8; HASH_LEN]) -> [u8; HASH_LEN] {
    let derived = derive_secret(handshake_secret, b"derived", &empty_transcript_hash());
    hkdf_extract(&derived, &[0u8; HASH_LEN])
}

/// The two 1-RTT (application) traffic secrets (RFC 8446 §7.1), one per
/// direction, derived from the Master Secret and the transcript hash of
/// `ClientHello…server Finished`. These are the `_0` generation; a QUIC key
/// update advances them with [`super::key_schedule::next_generation_secret`].
#[derive(Clone, PartialEq, Eq)]
pub struct ApplicationTrafficSecrets {
    /// `client_application_traffic_secret_0` — protects client→server 1-RTT
    /// packets (`Derive-Secret(., "c ap traffic", CH..SF)`).
    pub client: [u8; HASH_LEN],
    /// `server_application_traffic_secret_0` — protects server→client 1-RTT
    /// packets (`Derive-Secret(., "s ap traffic", CH..SF)`).
    pub server: [u8; HASH_LEN],
}

impl core::fmt::Debug for ApplicationTrafficSecrets {
    /// Redacts the secrets — logging them would leak the connection's 1-RTT
    /// keys. Only the presence of both directions is shown.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ApplicationTrafficSecrets")
            .field("client", &format_args!("<{HASH_LEN} bytes redacted>"))
            .field("server", &format_args!("<{HASH_LEN} bytes redacted>"))
            .finish()
    }
}

impl ApplicationTrafficSecrets {
    /// Derive both directions' 1-RTT traffic secrets from the Master Secret and
    /// the `ClientHello…server Finished` transcript hash (RFC 8446 §7.1).
    #[must_use]
    pub fn derive(master_secret: &[u8; HASH_LEN], transcript_ch_sf: &[u8]) -> Self {
        Self {
            client: derive_secret(master_secret, b"c ap traffic", transcript_ch_sf),
            server: derive_secret(master_secret, b"s ap traffic", transcript_ch_sf),
        }
    }

    /// The QUIC 1-RTT packet-protection keys for both directions, bridging each
    /// traffic secret through the `AEAD_AES_128_GCM` labels (RFC 9001 §5.1).
    #[must_use]
    pub fn packet_keys(&self) -> DirectionalKeys {
        DirectionalKeys {
            client: PacketProtectionKeys::aes_128_gcm_from_secret(self.client),
            server: PacketProtectionKeys::aes_128_gcm_from_secret(self.server),
        }
    }
}

/// The exporter master secret (RFC 8446 §7.5):
/// `Derive-Secret(Master, "exp master", ClientHello…server Finished)`.
#[must_use]
pub fn exporter_master_secret(master_secret: &[u8; HASH_LEN], transcript_ch_sf: &[u8]) -> [u8; HASH_LEN] {
    derive_secret(master_secret, b"exp master", transcript_ch_sf)
}

/// The resumption master secret (RFC 8446 §7.1):
/// `Derive-Secret(Master, "res master", ClientHello…client Finished)`.
#[must_use]
pub fn resumption_master_secret(master_secret: &[u8; HASH_LEN], transcript_ch_cf: &[u8]) -> [u8; HASH_LEN] {
    derive_secret(master_secret, b"res master", transcript_ch_cf)
}

// ── QUIC key bridge ─────────────────────────────────────────────────────────

/// A pair of QUIC packet-protection key sets, one for each direction of a
/// connection at a single encryption level (Handshake or 1-RTT). Mirrors
/// [`super::key_schedule::InitialKeys`] for the TLS-negotiated levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectionalKeys {
    /// Keys that protect client-sent packets at this encryption level.
    pub client: PacketProtectionKeys,
    /// Keys that protect server-sent packets at this encryption level.
    pub server: PacketProtectionKeys,
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

    /// Convert a 32-byte hex vector to a fixed array for secret comparisons.
    fn hex32(s: &str) -> [u8; HASH_LEN] {
        let v = hex(s);
        let mut a = [0u8; HASH_LEN];
        a.copy_from_slice(&v);
        a
    }

    #[test]
    fn empty_transcript_hash_is_sha256_of_nothing() {
        // The well-known SHA-256 of the empty string.
        assert_eq!(
            empty_transcript_hash().to_vec(),
            hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
    }

    #[test]
    fn transcript_hash_matches_sha256() {
        // SHA-256("abc") — FIPS 180-4 example.
        assert_eq!(
            transcript_hash(b"abc").to_vec(),
            hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
    }

    #[test]
    fn early_secret_no_psk_matches_rfc8448() {
        // RFC 8448 §3: Early Secret with an all-zero PSK.
        assert_eq!(
            early_secret().to_vec(),
            hex("33ad0a1c607ec03b09e6cd9893680ce210adf300aa1f2660e1b22e10f170f92a")
        );
    }

    #[test]
    fn derived_from_early_matches_rfc8448() {
        // Derive-Secret(Early, "derived", "") — RFC 8448 §3.
        let derived = derive_secret(&early_secret(), b"derived", &empty_transcript_hash());
        assert_eq!(
            derived.to_vec(),
            hex("6f2615a108c702c5678f54fc9dbab69716c076189c48250cebeac3576c3611ba")
        );
    }

    #[test]
    fn handshake_secret_matches_rfc8448() {
        // RFC 8448 §3: (EC)DHE shared secret → Handshake Secret.
        let ecdhe = hex("8bd4054fb55b9d63fdfbacf9f04b9f0d35e6d63f537563efd46272900f89492d");
        assert_eq!(
            handshake_secret(&ecdhe).to_vec(),
            hex("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac")
        );
    }

    #[test]
    fn handshake_traffic_secrets_match_rfc8448() {
        // RFC 8448 §3: hash of ClientHello..ServerHello, then the two
        // handshake traffic secrets.
        let hs = hex32("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        let th = hex("860c06edc07858ee8e78f0e7428c58edd6b43f2ca3e6e95f02ed063cf0e1cad8");
        let secrets = HandshakeTrafficSecrets::derive(&hs, &th);
        assert_eq!(
            secrets.client.to_vec(),
            hex("b3eddb126e067f35a780b3abf45e2d8f3b1a950738f52e9600746a0e27a55a21")
        );
        assert_eq!(
            secrets.server.to_vec(),
            hex("b67b7d690cc16c4e75e54213cb2d37b4e9c912bcded9105d42befd59d391ad38")
        );
    }

    #[test]
    fn master_secret_matches_rfc8448() {
        // RFC 8448 §3: Handshake Secret → Master Secret.
        let hs = hex32("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        assert_eq!(
            master_secret(&hs).to_vec(),
            hex("18df06843d13a08bf2a449844c5f8a478001bc4d4c627984d5a41da8d0402919")
        );
    }

    // Note: the RFC 8448 §3 exact vectors for the 1-RTT / exporter / resumption
    // secrets are *not* asserted here, because they are `Derive-Secret` over the
    // `ClientHello…server-Finished` transcript hash — a value this pure slice
    // does not reconstruct (it comes from the handshake message codecs of a later
    // slice). The `Derive-Secret` machinery itself is already pinned to RFC 8448
    // by `handshake_traffic_secrets_match_rfc8448` (same primitive, real
    // vectors); the tests below pin the application-level *wiring* — labels,
    // direction split, and the QUIC key bridge — against that proven primitive.

    #[test]
    fn application_traffic_secrets_use_correct_labels() {
        // The 1-RTT secrets must be exactly `Derive-Secret(master, "c/s ap
        // traffic", transcript)`. Cross-check the struct against direct
        // `derive_secret` calls so a label typo would fail.
        let ms = hex32("18df06843d13a08bf2a449844c5f8a478001bc4d4c627984d5a41da8d0402919");
        let th = transcript_hash(b"ClientHello..server Finished");
        let secrets = ApplicationTrafficSecrets::derive(&ms, &th);
        assert_eq!(secrets.client, derive_secret(&ms, b"c ap traffic", &th));
        assert_eq!(secrets.server, derive_secret(&ms, b"s ap traffic", &th));
        // The two directions and the two encryption levels never coincide.
        assert_ne!(secrets.client, secrets.server);
    }

    #[test]
    fn application_and_handshake_secrets_are_independent() {
        // Same transcript, but the application secrets come from the Master
        // Secret and the handshake secrets from the Handshake Secret with
        // different labels — none may collide.
        let hs = hex32("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        let ms = master_secret(&hs);
        let th = transcript_hash(b"transcript");
        let hs_secrets = HandshakeTrafficSecrets::derive(&hs, &th);
        let ap_secrets = ApplicationTrafficSecrets::derive(&ms, &th);
        assert_ne!(hs_secrets.client, ap_secrets.client);
        assert_ne!(hs_secrets.server, ap_secrets.server);
    }

    #[test]
    fn exporter_and_resumption_use_distinct_labels() {
        // The exporter and resumption master secrets differ from each other and
        // from the application traffic secrets even over the same transcript —
        // the only thing separating them is the `HKDF-Expand-Label` label.
        let ms = hex32("18df06843d13a08bf2a449844c5f8a478001bc4d4c627984d5a41da8d0402919");
        let th = transcript_hash(b"ClientHello..Finished");
        let exp = exporter_master_secret(&ms, &th);
        let res = resumption_master_secret(&ms, &th);
        let ap = ApplicationTrafficSecrets::derive(&ms, &th);
        assert_eq!(exp, derive_secret(&ms, b"exp master", &th));
        assert_eq!(res, derive_secret(&ms, b"res master", &th));
        assert_ne!(exp, res);
        assert_ne!(exp, ap.client);
        assert_ne!(res, ap.server);
    }

    #[test]
    fn end_to_end_handshake_leaf_from_rfc8448_inputs() {
        // Drive the schedule from the RFC 8448 §3 ECDHE secret and the
        // ClientHello…ServerHello transcript hash, confirming the handshake-level
        // leaf secret (both are real RFC vectors), then exercise the Master →
        // 1-RTT path structurally (its exact vector needs the CH…SF hash).
        let ecdhe = hex("8bd4054fb55b9d63fdfbacf9f04b9f0d35e6d63f537563efd46272900f89492d");
        let ch_sh = hex("860c06edc07858ee8e78f0e7428c58edd6b43f2ca3e6e95f02ed063cf0e1cad8");

        let hs = handshake_secret(&ecdhe);
        let hs_secrets = HandshakeTrafficSecrets::derive(&hs, &ch_sh);
        assert_eq!(
            hs_secrets.server.to_vec(),
            hex("b67b7d690cc16c4e75e54213cb2d37b4e9c912bcded9105d42befd59d391ad38")
        );

        // The 1-RTT path runs and produces distinct, direction-split keys.
        let ms = master_secret(&hs);
        let ap = ApplicationTrafficSecrets::derive(&ms, &transcript_hash(b"CH..SF"));
        let keys = ap.packet_keys();
        assert_ne!(keys.client.key, keys.server.key);
    }

    #[test]
    fn traffic_secrets_bridge_to_quic_keys() {
        // Each traffic secret must yield AES-128-GCM key lengths and the two
        // directions must never share key material.
        let hs = hex32("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        let th = hex("860c06edc07858ee8e78f0e7428c58edd6b43f2ca3e6e95f02ed063cf0e1cad8");
        let keys = HandshakeTrafficSecrets::derive(&hs, &th).packet_keys();
        assert_eq!(keys.client.key.len(), 16);
        assert_eq!(keys.client.iv.len(), 12);
        assert_eq!(keys.client.hp.len(), 16);
        assert_ne!(keys.client.key, keys.server.key);
        assert_ne!(keys.client.hp, keys.server.hp);
    }

    #[test]
    fn distinct_ecdhe_yields_distinct_handshake_secret() {
        let a = handshake_secret(&hex(&"00".repeat(32)));
        let b = handshake_secret(&hex(&"11".repeat(32)));
        assert_ne!(a, b);
    }

    #[test]
    fn distinct_transcripts_yield_distinct_traffic_secrets() {
        let hs = hex32("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        let a = HandshakeTrafficSecrets::derive(&hs, &transcript_hash(b"messages A"));
        let b = HandshakeTrafficSecrets::derive(&hs, &transcript_hash(b"messages B"));
        assert_ne!(a.client, b.client);
        assert_ne!(a.server, b.server);
    }

    #[test]
    fn derivation_is_deterministic() {
        let ecdhe = hex("8bd4054fb55b9d63fdfbacf9f04b9f0d35e6d63f537563efd46272900f89492d");
        assert_eq!(handshake_secret(&ecdhe), handshake_secret(&ecdhe));
    }

    #[test]
    fn client_and_server_handshake_secrets_differ() {
        let hs = hex32("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        let s = HandshakeTrafficSecrets::derive(&hs, &transcript_hash(b"ch..sh"));
        assert_ne!(s.client, s.server);
    }

    #[test]
    fn key_update_advances_application_secret() {
        use super::super::key_schedule::next_generation_secret;
        let ms = hex32("18df06843d13a08bf2a449844c5f8a478001bc4d4c627984d5a41da8d0402919");
        let ap = ApplicationTrafficSecrets::derive(&ms, &transcript_hash(b"ch..sf"));
        let next = next_generation_secret(&ap.client);
        assert_ne!(next, ap.client);
    }

    #[test]
    fn debug_redacts_secret_material() {
        let hs = hex32("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac");
        let s = HandshakeTrafficSecrets::derive(&hs, &transcript_hash(b"x"));
        let rendered = format!("{s:?}");
        assert!(rendered.contains("redacted"));
        assert!(!rendered.contains("b3eddb"));
    }
}
