//! X25519 (Curve25519) key agreement for the TLS 1.3 `key_share` extension
//! (RFC 7748, RFC 8446 §4.2.8) — slice 17 of the HTTP/3 sprint.
//!
//! TLS 1.3 negotiates the `(EC)DHE` shared secret by each endpoint sending an
//! ephemeral public key in a [`KeyShareEntry`]. This module performs the
//! Curve25519 scalar multiplication that turns *our* private scalar plus the
//! *peer's* [`KeyShareEntry`] public value into the raw 32-byte shared secret
//! that feeds [`crate::h3::tls_schedule::handshake_secret`] (RFC 8446 §7.1).
//!
//! The X25519 primitive itself comes from `x25519-dalek` (constant-time,
//! pure-Rust; rolling our own Curve25519 would be a security antipattern). This
//! module keeps the same codec-first, side-effect-free shape as the earlier
//! slices: the core functions are deterministic and take the private scalar as
//! input so they can be validated against the RFC 7748 test vectors. Only the
//! optional [`generate_x25519_private_key`] convenience reads OS entropy.
//!
//! Out of scope (later slices): the P-256 (`secp256r1`) group, HelloRetryRequest
//! group renegotiation, and the `Finished` / `CertificateVerify` MAC/signature.

use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};

use super::tls_message::{KeyShareEntry, GROUP_X25519};

/// The length, in bytes, of an X25519 private scalar, public key, and shared
/// secret (RFC 7748 §5: Curve25519 operates on 32-octet values).
pub const X25519_KEY_LEN: usize = 32;

/// A key-agreement fault. Every variant is a genuine handshake failure the
/// caller cannot fix by reading more bytes (it maps to a TLS `illegal_parameter`
/// / `handshake_failure` alert), never a "need more data" signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyAgreementError {
    /// The peer's [`KeyShareEntry`] is for a group this module cannot agree on.
    /// Only [`GROUP_X25519`] is supported for now.
    UnsupportedGroup {
        /// The `NamedGroup` code the peer offered.
        group: u16,
    },
    /// The peer's `key_exchange` value is not [`X25519_KEY_LEN`] bytes long, so
    /// it is not a valid X25519 public key (RFC 8446 §4.2.8 + RFC 7748 §5).
    BadKeyLength {
        /// The `NamedGroup` code (always [`GROUP_X25519`] here).
        group: u16,
        /// The actual `key_exchange` length received.
        len: usize,
    },
    /// The scalar multiplication produced the all-zero output, i.e. the peer
    /// sent a small-order (non-contributory) public key. RFC 7748 §6.1 lets an
    /// implementation reject this to guarantee a contributory shared secret; a
    /// zero secret would let an attacker force a known key.
    NonContributory,
}

impl core::fmt::Display for KeyAgreementError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedGroup { group } => {
                write!(f, "unsupported key_share group 0x{group:04x}")
            }
            Self::BadKeyLength { group, len } => write!(
                f,
                "key_share group 0x{group:04x} has {len}-byte key_exchange, expected {X25519_KEY_LEN}"
            ),
            Self::NonContributory => {
                write!(f, "X25519 agreement produced a non-contributory (zero) secret")
            }
        }
    }
}

impl std::error::Error for KeyAgreementError {}

/// Derive the X25519 public key for `private_key` — the value that goes into our
/// own [`KeyShareEntry`] (RFC 7748 §6.1: `public = X25519(private, 9)`, the
/// scalar multiplication of the private key by the curve base point).
///
/// The `x25519` primitive clamps the scalar internally (RFC 7748 §5), so any
/// 32-byte value is a valid private key; the caller does not pre-clamp.
#[must_use]
pub fn x25519_public_key(private_key: &[u8; X25519_KEY_LEN]) -> [u8; X25519_KEY_LEN] {
    x25519(*private_key, X25519_BASEPOINT_BYTES)
}

/// Compute the raw `(EC)DHE` shared secret from our `private_key` and the peer's
/// `peer_public` X25519 key (RFC 7748 §6.1: `X25519(private, peer_public)`).
///
/// The result is the 32-byte input to [`crate::h3::tls_schedule::
/// handshake_secret`].
///
/// # Errors
///
/// [`KeyAgreementError::NonContributory`] if the peer sent a small-order key
/// that yields the all-zero secret (RFC 7748 §6.1).
pub fn x25519_shared_secret(
    private_key: &[u8; X25519_KEY_LEN],
    peer_public: &[u8; X25519_KEY_LEN],
) -> Result<[u8; X25519_KEY_LEN], KeyAgreementError> {
    let secret = x25519(*private_key, *peer_public);
    if secret.iter().all(|&b| b == 0) {
        return Err(KeyAgreementError::NonContributory);
    }
    Ok(secret)
}

/// Bridge a peer [`KeyShareEntry`] into the `(EC)DHE` shared secret, validating
/// that it is an X25519 share of the right length before the agreement.
///
/// This is the entry point the TLS/QUIC handshake uses on the received
/// ServerHello (or, from the server's view, a ClientHello) key share, feeding
/// the result straight into [`crate::h3::tls_schedule::handshake_secret`].
///
/// # Errors
///
/// - [`KeyAgreementError::UnsupportedGroup`] if `peer.group` is not
///   [`GROUP_X25519`].
/// - [`KeyAgreementError::BadKeyLength`] if `peer.key_exchange` is not
///   [`X25519_KEY_LEN`] bytes.
/// - [`KeyAgreementError::NonContributory`] if the peer's key is small-order.
pub fn x25519_ecdhe_from_key_share(
    private_key: &[u8; X25519_KEY_LEN],
    peer: &KeyShareEntry,
) -> Result<[u8; X25519_KEY_LEN], KeyAgreementError> {
    if peer.group != GROUP_X25519 {
        return Err(KeyAgreementError::UnsupportedGroup { group: peer.group });
    }
    let peer_public: [u8; X25519_KEY_LEN] =
        peer.key_exchange.as_slice().try_into().map_err(|_| {
            KeyAgreementError::BadKeyLength {
                group: peer.group,
                len: peer.key_exchange.len(),
            }
        })?;
    x25519_shared_secret(private_key, &peer_public)
}

/// Build our own X25519 [`KeyShareEntry`] (group [`GROUP_X25519`], the derived
/// public key) for `private_key`, ready to place in a ClientHello or ServerHello
/// `key_share` extension (RFC 8446 §4.2.8).
#[must_use]
pub fn x25519_key_share(private_key: &[u8; X25519_KEY_LEN]) -> KeyShareEntry {
    KeyShareEntry {
        group: GROUP_X25519,
        key_exchange: x25519_public_key(private_key).to_vec(),
    }
}

/// Generate a fresh ephemeral X25519 private scalar from the OS CSPRNG.
///
/// The X25519 scalar multiplication clamps the scalar (RFC 7748 §5), so any
/// 32 random bytes form a valid private key — no rejection sampling is needed.
///
/// # Errors
///
/// Propagates [`getrandom::Error`] if the OS entropy source is unavailable.
pub fn generate_x25519_private_key() -> Result<[u8; X25519_KEY_LEN], getrandom::Error> {
    let mut key = [0u8; X25519_KEY_LEN];
    getrandom::getrandom(&mut key)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a hex string into bytes (test helper).
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Decode a 32-byte hex string into a fixed array (test helper).
    fn hex32(s: &str) -> [u8; 32] {
        hex(s).try_into().unwrap()
    }

    #[test]
    fn rfc7748_scalar_mult_vector_1() {
        // RFC 7748 §5.2, first X25519 test vector: X25519(scalar, u-coord).
        let scalar = hex32("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
        let u = hex32("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
        let out = x25519_shared_secret(&scalar, &u).unwrap();
        assert_eq!(
            out.to_vec(),
            hex("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552")
        );
    }

    #[test]
    fn rfc7748_scalar_mult_vector_2() {
        // RFC 7748 §5.2, second X25519 test vector.
        let scalar = hex32("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d");
        let u = hex32("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493");
        let out = x25519_shared_secret(&scalar, &u).unwrap();
        assert_eq!(
            out.to_vec(),
            hex("95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957")
        );
    }

    #[test]
    fn rfc7748_diffie_hellman_example() {
        // RFC 7748 §6.1: Alice and Bob derive the same shared secret.
        let alice_priv =
            hex32("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob_priv =
            hex32("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");

        // Public keys match the RFC's expected values.
        assert_eq!(
            x25519_public_key(&alice_priv).to_vec(),
            hex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
        );
        assert_eq!(
            x25519_public_key(&bob_priv).to_vec(),
            hex("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")
        );

        let alice_pub = x25519_public_key(&alice_priv);
        let bob_pub = x25519_public_key(&bob_priv);
        let alice_secret = x25519_shared_secret(&alice_priv, &bob_pub).unwrap();
        let bob_secret = x25519_shared_secret(&bob_priv, &alice_pub).unwrap();

        // Both sides agree, and it is the RFC's expected shared secret.
        assert_eq!(alice_secret, bob_secret);
        assert_eq!(
            alice_secret.to_vec(),
            hex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742")
        );
    }

    #[test]
    fn round_trip_public_key_and_shared_secret() {
        // A deterministic private pair (fixed bytes) agrees on a shared secret.
        let a = [0x11u8; 32];
        let b = [0x22u8; 32];
        let a_pub = x25519_public_key(&a);
        let b_pub = x25519_public_key(&b);
        assert_eq!(
            x25519_shared_secret(&a, &b_pub).unwrap(),
            x25519_shared_secret(&b, &a_pub).unwrap()
        );
    }

    #[test]
    fn small_order_key_is_non_contributory() {
        // The all-zero public key is a small-order point; any private scalar
        // multiplied by it yields the all-zero secret (RFC 7748 §6.1).
        let private = [0x42u8; 32];
        let zero_pub = [0u8; 32];
        assert_eq!(
            x25519_shared_secret(&private, &zero_pub),
            Err(KeyAgreementError::NonContributory)
        );
    }

    #[test]
    fn ecdhe_from_key_share_matches_direct_agreement() {
        // The KeyShareEntry bridge equals the raw scalar multiplication.
        let our_priv = [0x33u8; 32];
        let peer_priv = [0x44u8; 32];
        let peer_share = x25519_key_share(&peer_priv);
        assert_eq!(peer_share.group, GROUP_X25519);

        let via_bridge = x25519_ecdhe_from_key_share(&our_priv, &peer_share).unwrap();
        let peer_pub = x25519_public_key(&peer_priv);
        let direct = x25519_shared_secret(&our_priv, &peer_pub).unwrap();
        assert_eq!(via_bridge, direct);
    }

    #[test]
    fn ecdhe_from_key_share_rejects_wrong_group() {
        let our_priv = [0x33u8; 32];
        let bad = KeyShareEntry {
            group: super::super::tls_message::GROUP_SECP256R1,
            key_exchange: vec![0x04; 65],
        };
        assert_eq!(
            x25519_ecdhe_from_key_share(&our_priv, &bad),
            Err(KeyAgreementError::UnsupportedGroup {
                group: super::super::tls_message::GROUP_SECP256R1
            })
        );
    }

    #[test]
    fn ecdhe_from_key_share_rejects_bad_length() {
        let our_priv = [0x33u8; 32];
        let bad = KeyShareEntry {
            group: GROUP_X25519,
            key_exchange: vec![0xab; 31],
        };
        assert_eq!(
            x25519_ecdhe_from_key_share(&our_priv, &bad),
            Err(KeyAgreementError::BadKeyLength {
                group: GROUP_X25519,
                len: 31
            })
        );
    }

    #[test]
    fn generated_private_keys_produce_agreeing_secrets() {
        // The OS-CSPRNG helper yields a usable ephemeral key pair.
        let a = generate_x25519_private_key().unwrap();
        let b = generate_x25519_private_key().unwrap();
        let a_pub = x25519_public_key(&a);
        let b_pub = x25519_public_key(&b);
        assert_eq!(
            x25519_shared_secret(&a, &b_pub).unwrap(),
            x25519_shared_secret(&b, &a_pub).unwrap()
        );
    }

    #[test]
    fn ecdhe_secret_feeds_handshake_secret() {
        // End-to-end with slice 15: the agreed secret drives the Handshake Secret.
        use super::super::tls_schedule::handshake_secret;
        let our_priv = [0x55u8; 32];
        let peer_priv = [0x66u8; 32];
        let peer_share = x25519_key_share(&peer_priv);
        let ecdhe = x25519_ecdhe_from_key_share(&our_priv, &peer_share).unwrap();
        // Both endpoints reach the same Handshake Secret from the same (EC)DHE.
        let our_pub = x25519_public_key(&our_priv);
        let peer_ecdhe =
            x25519_ecdhe_from_key_share(&peer_priv, &x25519_key_share(&our_priv)).unwrap();
        assert_eq!(ecdhe, peer_ecdhe);
        assert_eq!(handshake_secret(&ecdhe), handshake_secret(&peer_ecdhe));
        let _ = our_pub;
    }
}
