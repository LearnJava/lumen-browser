//! RFC 8291 (Message Encryption for Web Push) decrypt, over the aes128gcm
//! content-encoding (RFC 8188 §2.1). Receiver (browser) side only.
//!
//! No `ece`/webpush crate is vendored (ADR-027 §5: no such crate is a
//! committee-owned standard the way e.g. `psl` is) — the HKDF/ECDH/AES-GCM
//! steps are hand-rolled from the already-vendored `p256`+`hmac`+`sha2`+
//! `aes-gcm` building blocks, the same primitives
//! `crates/js/src/subtle_crypto.rs` uses for WebCrypto ECDH/HKDF/AES-GCM.
//!
//! `encrypt_for_test` produces a spec-shaped ciphertext for this module's own
//! decrypt tests — срез 4 (docs/tasks/ph3-push-api.md) has no external push
//! service to interop against ("при отсутствии внешнего push-сервиса —
//! mock-relay в тестах"), so correctness is verified by round-tripping our
//! own encrypt against our own decrypt plus the aes128gcm framing invariants
//! (salt/rs/keyid layout, single-record padding delimiter).

#![allow(missing_docs)]

use aes_gcm::{AeadInPlace, KeyInit, Nonce, Tag};
use hmac::Mac;
use p256::elliptic_curve::sec1::ToEncodedPoint;

/// RFC 8291 §3.4 fixed HKDF info string for deriving the intermediate IKM
/// from the ECDH shared secret + auth secret.
const KEY_INFO_PREFIX: &[u8] = b"WebPush: info\0";
/// RFC 8188 §2.1 fixed HKDF info string for the content-encryption key.
const CEK_INFO: &[u8] = b"Content-Encoding: aes128gcm\0";
/// RFC 8188 §2.1 fixed HKDF info string for the nonce.
const NONCE_INFO: &[u8] = b"Content-Encoding: nonce\0";
/// RFC 8188 §2.1 padding delimiter octet for the last (only) record.
const LAST_RECORD_DELIMITER: u8 = 0x02;
/// aes128gcm header: 16-byte salt + 4-byte record size + 1-byte keyid length.
const HEADER_PREFIX_LEN: usize = 16 + 4 + 1;

fn hmac_sha256(key: &[u8], data: &[u8]) -> Option<[u8; 32]> {
    let mut mac = <hmac::Hmac<sha2::Sha256> as Mac>::new_from_slice(key).ok()?;
    mac.update(data);
    let out = mac.finalize().into_bytes();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    Some(arr)
}

/// RFC 5869 HKDF extract-then-expand (SHA-256) — same generic shape as
/// `crates/js/src/subtle_crypto.rs::hkdf_derive`, reimplemented here because
/// the two crates cannot share a private helper.
fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8], length: usize) -> Option<Vec<u8>> {
    let prk = hmac_sha256(salt, ikm)?;
    let mut out = Vec::with_capacity(length);
    let mut prev: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while out.len() < length {
        let mut input = prev.clone();
        input.extend_from_slice(info);
        input.push(counter);
        let t_i = hmac_sha256(&prk, &input)?;
        let take = t_i.len().min(length - out.len());
        out.extend_from_slice(&t_i[..take]);
        prev = t_i.to_vec();
        counter = counter.checked_add(1)?;
    }
    Some(out)
}

/// Derive the aes128gcm content-encryption key + nonce shared by the sender
/// and receiver (RFC 8291 §3.4), given the ECDH shared secret, the 16-byte
/// `auth` secret, both parties' raw uncompressed SEC1 public points (65
/// bytes each), and the per-message 16-byte `salt` from the aes128gcm header.
fn derive_cek_nonce(
    ecdh_secret: &[u8],
    auth_secret: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    salt: &[u8],
) -> Option<([u8; 16], [u8; 12])> {
    let mut key_info = KEY_INFO_PREFIX.to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public);
    let ikm = hkdf(ecdh_secret, auth_secret, &key_info, 32)?;

    let cek = hkdf(&ikm, salt, CEK_INFO, 16)?;
    let nonce = hkdf(&ikm, salt, NONCE_INFO, 12)?;

    let mut cek_arr = [0u8; 16];
    cek_arr.copy_from_slice(&cek);
    let mut nonce_arr = [0u8; 12];
    nonce_arr.copy_from_slice(&nonce);
    Some((cek_arr, nonce_arr))
}

/// Decrypt an aes128gcm WebPush message body (RFC 8291) using the
/// subscription's ECDH private key (SEC1 scalar, base64) and auth secret
/// (base64).
///
/// `payload` is the raw HTTP request body a push service delivers to the
/// subscription endpoint: `salt(16) || record_size(4, BE) || keyid_len(1) ||
/// keyid(keyid_len) || ciphertext+tag`. `keyid` is the application server's
/// ephemeral P-256 public key (uncompressed SEC1 point). Returns `None` on
/// any malformed input or failed authentication (wrong key, corrupted body)
/// — never panics.
pub fn decrypt(private_key_b64: &str, auth_b64: &str, payload: &[u8]) -> Option<Vec<u8>> {
    let private_key_raw = lumen_core::hash::base64_decode(private_key_b64)?;
    let auth_secret = lumen_core::hash::base64_decode(auth_b64)?;
    let ua_private = p256::SecretKey::from_slice(&private_key_raw).ok()?;
    let ua_public = ua_private.public_key().to_encoded_point(false);

    if payload.len() < HEADER_PREFIX_LEN {
        return None;
    }
    let salt = &payload[0..16];
    let keyid_len = payload[20] as usize;
    let header_len = HEADER_PREFIX_LEN + keyid_len;
    if payload.len() < header_len {
        return None;
    }
    let as_public_raw = &payload[HEADER_PREFIX_LEN..header_len];
    let ciphertext = &payload[header_len..];
    if ciphertext.len() < 16 {
        return None;
    }

    let as_public = p256::PublicKey::from_sec1_bytes(as_public_raw).ok()?;
    let shared = p256::ecdh::diffie_hellman(ua_private.to_nonzero_scalar(), as_public.as_affine());
    let ecdh_secret = shared.raw_secret_bytes();

    let (cek, nonce_bytes) = derive_cek_nonce(
        ecdh_secret.as_slice(),
        &auth_secret,
        ua_public.as_bytes(),
        as_public_raw,
        salt,
    )?;

    let (ct, tag_bytes) = ciphertext.split_at(ciphertext.len() - 16);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let tag = Tag::from_slice(tag_bytes);
    let cipher = aes_gcm::Aes128Gcm::new_from_slice(&cek).ok()?;
    let mut buf = ct.to_vec();
    cipher.decrypt_in_place_detached(nonce, b"", &mut buf, tag).ok()?;

    // RFC 8188 §2: strip trailing zero padding, then the delimiter octet
    // (0x02 — this is always the last/only record for a push message).
    while buf.last() == Some(&0) {
        buf.pop();
    }
    if buf.pop() != Some(LAST_RECORD_DELIMITER) {
        return None;
    }
    Some(buf)
}

/// Encrypt `plaintext` the way a push service would (RFC 8291), for this
/// module's own decrypt tests. Never used by production code.
#[cfg(test)]
pub(crate) fn encrypt_for_test(p256dh_b64: &str, auth_b64: &str, plaintext: &[u8]) -> Option<Vec<u8>> {
    let ua_public_raw = lumen_core::hash::base64_decode(p256dh_b64)?;
    let auth_secret = lumen_core::hash::base64_decode(auth_b64)?;
    let ua_public = p256::PublicKey::from_sec1_bytes(&ua_public_raw).ok()?;

    let as_secret = loop {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).ok()?;
        if let Ok(k) = p256::SecretKey::from_slice(&seed) {
            break k;
        }
    };
    let as_public = as_secret.public_key().to_encoded_point(false);

    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).ok()?;

    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());
    let ecdh_secret = shared.raw_secret_bytes();

    let (cek, nonce_bytes) = derive_cek_nonce(
        ecdh_secret.as_slice(),
        &auth_secret,
        &ua_public_raw,
        as_public.as_bytes(),
        &salt,
    )?;

    let mut buf = plaintext.to_vec();
    buf.push(LAST_RECORD_DELIMITER);

    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher = aes_gcm::Aes128Gcm::new_from_slice(&cek).ok()?;
    let tag = cipher.encrypt_in_place_detached(nonce, b"", &mut buf).ok()?;

    let as_public_bytes = as_public.as_bytes();
    let mut out = Vec::with_capacity(HEADER_PREFIX_LEN + as_public_bytes.len() + buf.len() + 16);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(as_public_bytes.len() as u8);
    out.extend_from_slice(as_public_bytes);
    out.extend_from_slice(&buf);
    out.extend_from_slice(tag.as_slice());
    Some(out)
}

/// Generate a fresh (private_key_b64, p256dh_b64, auth_b64) triple — same
/// shape as `crates/js/src/push_api.rs::generate_push_keys` — for
/// `push_crypto`'s and `push_store`'s tests.
#[cfg(test)]
pub(crate) fn test_keypair() -> (String, String, String) {
    let secret = loop {
        let mut seed = [0u8; 32];
        if getrandom::getrandom(&mut seed).is_err() {
            continue;
        }
        if let Ok(k) = p256::SecretKey::from_slice(&seed) {
            break k;
        }
    };
    let public_point = secret.public_key().to_encoded_point(false);
    let mut auth = [0u8; 16];
    let _ = getrandom::getrandom(&mut auth);
    (
        lumen_core::hash::base64_encode(&secret.to_bytes()),
        lumen_core::hash::base64_encode(public_point.as_bytes()),
        lumen_core::hash::base64_encode(&auth),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keypair() -> (String, String, String) {
        test_keypair()
    }

    #[test]
    fn roundtrip_decrypts_to_original_plaintext() {
        let (private_key, p256dh, auth) = keypair();
        let payload = encrypt_for_test(&p256dh, &auth, b"hello push").unwrap();
        let plaintext = decrypt(&private_key, &auth, &payload).unwrap();
        assert_eq!(plaintext, b"hello push");
    }

    #[test]
    fn roundtrip_handles_empty_message() {
        let (private_key, p256dh, auth) = keypair();
        let payload = encrypt_for_test(&p256dh, &auth, b"").unwrap();
        let plaintext = decrypt(&private_key, &auth, &payload).unwrap();
        assert_eq!(plaintext, b"");
    }

    #[test]
    fn roundtrip_handles_large_message() {
        let (private_key, p256dh, auth) = keypair();
        let big = vec![b'x'; 3000];
        let payload = encrypt_for_test(&p256dh, &auth, &big).unwrap();
        let plaintext = decrypt(&private_key, &auth, &payload).unwrap();
        assert_eq!(plaintext, big);
    }

    #[test]
    fn wrong_private_key_fails_to_decrypt() {
        let (_, p256dh, auth) = keypair();
        let (other_private_key, _, _) = keypair();
        let payload = encrypt_for_test(&p256dh, &auth, b"secret").unwrap();
        assert!(decrypt(&other_private_key, &auth, &payload).is_none());
    }

    #[test]
    fn wrong_auth_secret_fails_to_decrypt() {
        let (private_key, p256dh, auth) = keypair();
        let (_, _, other_auth) = keypair();
        let payload = encrypt_for_test(&p256dh, &auth, b"secret").unwrap();
        assert!(decrypt(&private_key, &other_auth, &payload).is_none());
    }

    #[test]
    fn corrupted_ciphertext_fails_authentication() {
        let (private_key, p256dh, auth) = keypair();
        let mut payload = encrypt_for_test(&p256dh, &auth, b"hello push").unwrap();
        let last = payload.len() - 1;
        payload[last] ^= 0xFF;
        assert!(decrypt(&private_key, &auth, &payload).is_none());
    }

    #[test]
    fn truncated_payload_is_rejected_not_panicking() {
        assert!(decrypt("AAAA", "AAAA", &[]).is_none());
        assert!(decrypt("AAAA", "AAAA", &[0u8; 5]).is_none());
    }

    #[test]
    fn garbage_keys_are_rejected_not_panicking() {
        let (_, p256dh, auth) = keypair();
        let payload = encrypt_for_test(&p256dh, &auth, b"hi").unwrap();
        assert!(decrypt("not-base64!!", &auth, &payload).is_none());
        assert!(decrypt("AAAA", "not-base64!!", &payload).is_none());
    }
}
