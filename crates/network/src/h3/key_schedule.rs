//! QUIC key schedule — RFC 9001 §5.1, §5.2 (Initial secrets + packet-protection
//! key derivation) on top of the TLS 1.3 HKDF (RFC 8446 §7.1, RFC 5869).
//!
//! QUIC derives every packet-protection key from a per-encryption-level traffic
//! secret via `HKDF-Expand-Label`. The Initial keys are special: they are not
//! negotiated by TLS but derived deterministically from the client's original
//! Destination Connection ID and a fixed, version-specific salt (RFC 9001
//! §5.2), so both endpoints can protect the first flight before the handshake
//! completes. From a traffic secret this module produces the AEAD key, the AEAD
//! nonce base (`iv`), and the header-protection key using the QUIC labels
//! `"quic key"`, `"quic iv"`, and `"quic hp"` (RFC 9001 §5.1). Key update
//! (RFC 9001 §6) advances a secret with the `"quic ku"` label.
//!
//! ## Scope
//!
//! - `HKDF-Extract` / `HKDF-Expand` (RFC 5869) and `HKDF-Expand-Label`
//!   (RFC 8446 §7.1), all over SHA-256 — the KDF hash of the two QUIC v1
//!   mandatory-to-implement cipher suites whose PRF is SHA-256
//!   (`AEAD_AES_128_GCM` and `AEAD_CHACHA20_POLY1305`). `AEAD_AES_256_GCM`
//!   (SHA-384) is deferred.
//! - The QUIC v1 Initial salt (RFC 9001 §5.2) and the Initial-secret chain:
//!   `initial_secret` → `client in` / `server in` → per-direction
//!   [`PacketProtectionKeys`].
//! - Key update: [`next_generation_secret`] (RFC 9001 §6.1).
//!
//! ## Out of scope (deferred to higher slices)
//!
//! - Header protection (RFC 9001 §5.4) and AEAD packet protection (§5.3) — this
//!   slice only *derives* the `key`/`iv`/`hp` material; the transforms that
//!   consume it live in the next slices.
//! - The TLS 1.3 handshake that produces the Handshake and 1-RTT traffic
//!   secrets; only the Initial secrets (derivable without TLS) are wired here.
//!   [`hkdf_expand_label`] is `pub(crate)` so those slices reuse it verbatim.
//! - No IO. Pure functions over byte slices.

use sha2::{Digest, Sha256};

// ── HMAC-SHA256 (RFC 2104) ──────────────────────────────────────────────────
//
// Implemented directly on the already-present `sha2` dependency rather than
// pulling in a separate `hmac` crate: HKDF needs only this one primitive, and
// a fixed-hash HMAC is a dozen lines. Keeping it local also avoids the
// `new_from_slice` fallible constructor (whose error is unreachable for HMAC
// but would otherwise force an `unwrap`/`expect` the project forbids).

/// SHA-256 internal block size in bytes (RFC 6234) — the HMAC key padding width.
const SHA256_BLOCK: usize = 64;
/// SHA-256 output size in bytes.
const SHA256_OUT: usize = 32;

/// `HMAC-SHA256(key, msg)` (RFC 2104). Infallible: HMAC accepts a key of any
/// length, hashing keys longer than the block down first.
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; SHA256_OUT] {
    // K' — the key padded (or hashed then padded) to one SHA-256 block.
    let mut block = [0u8; SHA256_BLOCK];
    if key.len() > SHA256_BLOCK {
        let mut h = Sha256::new();
        h.update(key);
        let digest = h.finalize();
        block[..SHA256_OUT].copy_from_slice(&digest);
    } else {
        block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; SHA256_BLOCK];
    let mut opad = [0x5cu8; SHA256_BLOCK];
    for i in 0..SHA256_BLOCK {
        ipad[i] ^= block[i];
        opad[i] ^= block[i];
    }

    // H((K' ^ opad) | H((K' ^ ipad) | msg)).
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    outer.finalize().into()
}

// ── HKDF (RFC 5869) + HKDF-Expand-Label (RFC 8446 §7.1) ─────────────────────

/// `HKDF-Extract(salt, IKM)` over SHA-256 (RFC 5869 §2.2) — the pseudorandom
/// key is simply `HMAC-Hash(salt, IKM)`.
///
/// `pub(crate)` so the TLS 1.3 key schedule (`tls_schedule`) reuses it for the
/// Early / Handshake / Master secret extractions verbatim.
#[must_use]
pub(crate) fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; SHA256_OUT] {
    hmac_sha256(salt, ikm)
}

/// `HKDF-Expand(PRK, info, L)` over SHA-256 (RFC 5869 §2.3). Returns `out_len`
/// octets of output key material. `out_len` must not exceed `255 * 32` octets
/// (the RFC 5869 limit); QUIC only ever asks for 12–32, so the single-octet
/// counter never wraps.
#[must_use]
fn hkdf_expand(prk: &[u8; SHA256_OUT], info: &[u8], out_len: usize) -> Vec<u8> {
    debug_assert!(out_len <= 255 * SHA256_OUT, "HKDF-Expand length exceeds RFC 5869 limit");
    let mut okm = Vec::with_capacity(out_len);
    // T(0) is the empty string; T(i) = HMAC(PRK, T(i-1) | info | i).
    let mut prev: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while okm.len() < out_len {
        let mut msg = Vec::with_capacity(prev.len() + info.len() + 1);
        msg.extend_from_slice(&prev);
        msg.extend_from_slice(info);
        msg.push(counter);
        let t = hmac_sha256(prk, &msg);
        let take = (out_len - okm.len()).min(SHA256_OUT);
        okm.extend_from_slice(&t[..take]);
        prev = t.to_vec();
        // Safe: the `out_len` bound keeps at most 255 iterations, so the final
        // block fills `okm` and exits the loop before this would wrap.
        counter = counter.wrapping_add(1);
    }
    okm
}

/// `HKDF-Expand-Label(Secret, Label, Context, Length)` (RFC 8446 §7.1). The
/// `HkdfLabel` structure prefixes the label with `"tls13 "`; QUIC labels such
/// as `"quic key"` therefore appear on the wire as `"tls13 quic key"`, which is
/// what provides key separation between QUIC and TLS (RFC 9001 §5.1).
///
/// `pub(crate)` so the later Handshake / 1-RTT key slices reuse it for the
/// secrets TLS negotiates.
#[must_use]
pub(crate) fn hkdf_expand_label(
    secret: &[u8; SHA256_OUT],
    label: &[u8],
    context: &[u8],
    out_len: usize,
) -> Vec<u8> {
    // struct { uint16 length; opaque label<7..255>; opaque context<0..255>; }
    let full_label_len = TLS13_LABEL_PREFIX.len() + label.len();
    let mut info = Vec::with_capacity(2 + 1 + full_label_len + 1 + context.len());
    info.extend_from_slice(&(out_len as u16).to_be_bytes());
    info.push(full_label_len as u8);
    info.extend_from_slice(TLS13_LABEL_PREFIX);
    info.extend_from_slice(label);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    hkdf_expand(secret, &info, out_len)
}

/// The `"tls13 "` prefix every `HKDF-Expand-Label` label carries (RFC 8446 §7.1).
const TLS13_LABEL_PREFIX: &[u8] = b"tls13 ";

// ── QUIC v1 constants (RFC 9001 §5.1, §5.2) ─────────────────────────────────

/// The QUIC version 1 Initial salt (RFC 9001 §5.2). Mixed with the client's
/// original Destination Connection ID to seed the Initial secret.
pub const INITIAL_SALT_V1: [u8; 20] = [
    0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3, 0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
    0xcc, 0xbb, 0x7f, 0x0a,
];

/// AEAD_AES_128_GCM key length in bytes — the Initial packet-protection AEAD.
pub const AES_128_KEY_LEN: usize = 16;
/// AEAD nonce (IV) length in bytes (RFC 9001 §5.3: the AEAD nonce is 12 octets).
pub const AEAD_IV_LEN: usize = 12;
/// AEAD_AES_128_GCM header-protection key length in bytes (RFC 9001 §5.4.3).
pub const AES_128_HP_LEN: usize = 16;

// ── Packet-protection key material ──────────────────────────────────────────

/// The three secrets that protect packets at one encryption level for one
/// direction (RFC 9001 §5.1): the AEAD `key`, the AEAD nonce base `iv`, and the
/// header-protection key `hp`. The traffic `secret` they were derived from is
/// retained so a key update (RFC 9001 §6) can advance it.
///
/// For Initial keys (`AEAD_AES_128_GCM`) `key` and `hp` are 16 bytes and `iv`
/// is 12; later levels may negotiate other lengths but always a 12-byte `iv`.
#[derive(Clone, PartialEq, Eq)]
pub struct PacketProtectionKeys {
    /// AEAD key applied to the packet payload (RFC 9001 §5.3).
    pub key: Vec<u8>,
    /// AEAD nonce base, XORed with the packet number to form the nonce
    /// (RFC 9001 §5.3).
    pub iv: Vec<u8>,
    /// Header-protection key (RFC 9001 §5.4).
    pub hp: Vec<u8>,
    /// The traffic secret these keys were derived from, kept for key update
    /// (RFC 9001 §6.1).
    pub secret: [u8; SHA256_OUT],
}

impl core::fmt::Debug for PacketProtectionKeys {
    /// Redacts all key material — logging QUIC keys would leak the connection's
    /// confidentiality. Only the byte lengths are shown.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PacketProtectionKeys")
            .field("key", &format_args!("<{} bytes redacted>", self.key.len()))
            .field("iv", &format_args!("<{} bytes redacted>", self.iv.len()))
            .field("hp", &format_args!("<{} bytes redacted>", self.hp.len()))
            .field("secret", &format_args!("<{SHA256_OUT} bytes redacted>"))
            .finish()
    }
}

impl PacketProtectionKeys {
    /// Derive the AEAD key, nonce base, and header-protection key from a traffic
    /// `secret` using the `AEAD_AES_128_GCM` lengths (RFC 9001 §5.1). This is
    /// the cipher suite QUIC v1 mandates for Initial packets; other suites reuse
    /// the same labels with their own key length.
    #[must_use]
    pub fn aes_128_gcm_from_secret(secret: [u8; SHA256_OUT]) -> Self {
        Self {
            key: hkdf_expand_label(&secret, b"quic key", b"", AES_128_KEY_LEN),
            iv: hkdf_expand_label(&secret, b"quic iv", b"", AEAD_IV_LEN),
            hp: hkdf_expand_label(&secret, b"quic hp", b"", AES_128_HP_LEN),
            secret,
        }
    }
}

/// The Initial keys for both directions of a connection (RFC 9001 §5.2),
/// derived from the client's original Destination Connection ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialKeys {
    /// Keys that protect client-sent Initial packets.
    pub client: PacketProtectionKeys,
    /// Keys that protect server-sent Initial packets.
    pub server: PacketProtectionKeys,
}

impl InitialKeys {
    /// Derive both directions' Initial keys from the client's original
    /// Destination Connection ID (RFC 9001 §5.2).
    #[must_use]
    pub fn derive(client_dcid: &[u8]) -> Self {
        Self {
            client: PacketProtectionKeys::aes_128_gcm_from_secret(client_initial_secret(client_dcid)),
            server: PacketProtectionKeys::aes_128_gcm_from_secret(server_initial_secret(client_dcid)),
        }
    }
}

// ── Initial-secret chain (RFC 9001 §5.2) ────────────────────────────────────

/// The connection's Initial secret: `HKDF-Extract(initial_salt, client_dcid)`
/// (RFC 9001 §5.2). Both directions' secrets expand from this.
#[must_use]
pub fn initial_secret(client_dcid: &[u8]) -> [u8; SHA256_OUT] {
    hkdf_extract(&INITIAL_SALT_V1, client_dcid)
}

/// The client's Initial traffic secret (RFC 9001 §5.2):
/// `HKDF-Expand-Label(initial_secret, "client in", "", 32)`.
#[must_use]
pub fn client_initial_secret(client_dcid: &[u8]) -> [u8; SHA256_OUT] {
    expand_secret(&initial_secret(client_dcid), b"client in")
}

/// The server's Initial traffic secret (RFC 9001 §5.2):
/// `HKDF-Expand-Label(initial_secret, "server in", "", 32)`.
#[must_use]
pub fn server_initial_secret(client_dcid: &[u8]) -> [u8; SHA256_OUT] {
    expand_secret(&initial_secret(client_dcid), b"server in")
}

/// The next-generation traffic secret for a key update (RFC 9001 §6.1):
/// `HKDF-Expand-Label(secret, "quic ku", "", 32)`. The header-protection key is
/// **not** rotated by a key update, so callers keep their existing `hp` and only
/// re-derive `key`/`iv` from this secret.
#[must_use]
pub fn next_generation_secret(secret: &[u8; SHA256_OUT]) -> [u8; SHA256_OUT] {
    expand_secret(secret, b"quic ku")
}

/// Expand a 32-byte traffic secret to another 32-byte secret under `label`.
fn expand_secret(secret: &[u8; SHA256_OUT], label: &[u8]) -> [u8; SHA256_OUT] {
    let out = hkdf_expand_label(secret, label, b"", SHA256_OUT);
    let mut secret = [0u8; SHA256_OUT];
    // `hkdf_expand_label` always returns exactly `out_len` bytes.
    secret.copy_from_slice(&out[..SHA256_OUT]);
    secret
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

    // RFC 9001 Appendix A.1 uses this client Destination Connection ID.
    fn sample_dcid() -> Vec<u8> {
        hex("8394c8f03e515708")
    }

    #[test]
    fn hmac_sha256_rfc4231_test_case_2() {
        // RFC 4231 §4.3: key = "Jefe", data = "what do ya want for nothing?".
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            mac.to_vec(),
            hex("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
        );
    }

    #[test]
    fn hmac_sha256_long_key_is_hashed_first() {
        // A key longer than the 64-byte block is hashed down; just check it runs
        // and produces a full-length tag distinct from the short-key case.
        let long_key = vec![0xaau8; 131];
        let mac = hmac_sha256(&long_key, b"Test Using Larger Than Block-Size Key - Hash Key First");
        assert_eq!(mac.len(), 32);
        // RFC 4231 §4.6 expected tag.
        assert_eq!(
            mac.to_vec(),
            hex("60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54")
        );
    }

    #[test]
    fn hkdf_rfc5869_test_case_1() {
        // RFC 5869 Appendix A.1 (SHA-256).
        let ikm = hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let salt = hex("000102030405060708090a0b0c");
        let info = hex("f0f1f2f3f4f5f6f7f8f9");
        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(
            prk.to_vec(),
            hex("077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5")
        );
        let okm = hkdf_expand(&prk, &info, 42);
        assert_eq!(
            okm,
            hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865")
        );
    }

    #[test]
    fn initial_secret_matches_rfc9001() {
        // RFC 9001 Appendix A.1.
        assert_eq!(
            initial_secret(&sample_dcid()).to_vec(),
            hex("7db5df06e7a69e432496adedb00851923595221596ae2ae9fb8115c1e9ed0a44")
        );
    }

    #[test]
    fn client_initial_secret_matches_rfc9001() {
        assert_eq!(
            client_initial_secret(&sample_dcid()).to_vec(),
            hex("c00cf151ca5be075ed0ebfb5c80323c42d6b7db67881289af4008f1f6c357aea")
        );
    }

    #[test]
    fn server_initial_secret_matches_rfc9001() {
        assert_eq!(
            server_initial_secret(&sample_dcid()).to_vec(),
            hex("3c199828fd139efd216c155ad844cc81fb82fa8d7446fa7d78be803acdda951b")
        );
    }

    #[test]
    fn client_packet_keys_match_rfc9001() {
        // RFC 9001 Appendix A.1: client key / iv / hp.
        let keys = PacketProtectionKeys::aes_128_gcm_from_secret(client_initial_secret(&sample_dcid()));
        assert_eq!(keys.key, hex("1f369613dd76d5467730efcbe3b1a22d"));
        assert_eq!(keys.iv, hex("fa044b2f42a3fd3b46fb255c"));
        assert_eq!(keys.hp, hex("9f50449e04a0e810283a1e9933adedd2"));
    }

    #[test]
    fn server_packet_keys_match_rfc9001() {
        // RFC 9001 Appendix A.1: server key / iv / hp.
        let keys = PacketProtectionKeys::aes_128_gcm_from_secret(server_initial_secret(&sample_dcid()));
        assert_eq!(keys.key, hex("cf3a5331653c364c88f0f379b6067e37"));
        assert_eq!(keys.iv, hex("0ac1493ca1905853b0bba03e"));
        assert_eq!(keys.hp, hex("c206b8d9b9f0f37644430b490eeaa314"));
    }

    #[test]
    fn derive_initial_keys_wires_both_directions() {
        let keys = InitialKeys::derive(&sample_dcid());
        assert_eq!(keys.client.key, hex("1f369613dd76d5467730efcbe3b1a22d"));
        assert_eq!(keys.server.key, hex("cf3a5331653c364c88f0f379b6067e37"));
        // The two directions must never share key material.
        assert_ne!(keys.client.key, keys.server.key);
        assert_ne!(keys.client.hp, keys.server.hp);
    }

    #[test]
    fn key_lengths_are_aes_128_gcm() {
        let keys = InitialKeys::derive(&sample_dcid());
        assert_eq!(keys.client.key.len(), AES_128_KEY_LEN);
        assert_eq!(keys.client.iv.len(), AEAD_IV_LEN);
        assert_eq!(keys.client.hp.len(), AES_128_HP_LEN);
    }

    #[test]
    fn key_update_advances_secret_and_changes_key() {
        let base = client_initial_secret(&sample_dcid());
        let next = next_generation_secret(&base);
        // A key update must produce a different traffic secret and thus a
        // different AEAD key.
        assert_ne!(next, base);
        let k0 = PacketProtectionKeys::aes_128_gcm_from_secret(base);
        let k1 = PacketProtectionKeys::aes_128_gcm_from_secret(next);
        assert_ne!(k0.key, k1.key);
        assert_ne!(k0.iv, k1.iv);
    }

    #[test]
    fn key_update_is_deterministic() {
        let base = server_initial_secret(&sample_dcid());
        assert_eq!(next_generation_secret(&base), next_generation_secret(&base));
    }

    #[test]
    fn debug_redacts_key_material() {
        let keys = InitialKeys::derive(&sample_dcid());
        let rendered = format!("{:?}", keys.client);
        assert!(rendered.contains("redacted"));
        // No raw key byte should appear in the debug string.
        assert!(!rendered.contains("1f3696"));
    }

    #[test]
    fn distinct_dcids_yield_distinct_keys() {
        let a = InitialKeys::derive(&hex("8394c8f03e515708"));
        let b = InitialKeys::derive(&hex("0001020304050607"));
        assert_ne!(a.client.key, b.client.key);
        assert_ne!(a.server.iv, b.server.iv);
    }
}
