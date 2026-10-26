//! Natives behind `self.crypto` — `_lumen_get_random_bytes`, `_lumen_sha_digest`
//! and the `_lumen_subtle_*` family (BUG-1688).
//!
//! Web Crypto is `[Exposed=(Window,Worker)]`, so the page runtime and every worker
//! flavour need the same natives. They depend on nothing of a page (keys live in a
//! `thread_local!` of [`crate::subtle_crypto`], one store per OS thread), so the list
//! is built here once and each side registers it its own way: the page through
//! `register_v8_native` inside `install_crypto_and_typed_om`, a worker through
//! `V8JsRuntime::register_native` in `install_worker_scope_globals_v8`.

use crate::v8_compat::{
    into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4, into_v8_fn5, V8NativeFn,
};

/// `(global name, native)` for every Web Crypto native.
pub(crate) fn web_crypto_natives() -> Vec<(&'static str, Box<dyn V8NativeFn + Send>)> {
    vec![
        // `n` cryptographically-random bytes, capped at 65 536 per call (WebCrypto §10.1.3).
        (
            "_lumen_get_random_bytes",
            into_v8_fn1(|n: u32| -> Vec<u8> {
                let len = (n as usize).min(65_536);
                let mut buf = vec![0u8; len];
                getrandom::getrandom(&mut buf).unwrap_or(());
                buf
            }),
        ),
        // SHA digest by name ("SHA-1" / "SHA-256" / "SHA-384" / "SHA-512"); empty on an unknown one.
        (
            "_lumen_sha_digest",
            into_v8_fn2(|algo: String, data: Vec<u8>| -> Vec<u8> {
                // sha1::Digest trait must be in scope to call sha1::Sha1::digest().
                use sha1::Digest as _;
                match algo.as_str() {
                    "SHA-1" => sha1::Sha1::digest(&data).to_vec(),
                    "SHA-256" => sha2::Sha256::digest(&data).to_vec(),
                    "SHA-384" => sha2::Sha384::digest(&data).to_vec(),
                    "SHA-512" => sha2::Sha512::digest(&data).to_vec(),
                    _ => Vec::new(),
                }
            }),
        ),
        // SubtleCrypto: the key store and the algorithms in `crate::subtle_crypto` are plain
        // Rust (no JS-engine dependency), so these are thin wrappers.
        (
            "_lumen_subtle_generate_key",
            into_v8_fn3(|alg_json: String, extractable: bool, usages_json: String| -> String {
                crate::subtle_crypto::generate_key(&alg_json, extractable, &usages_json)
            }),
        ),
        (
            "_lumen_subtle_import_key",
            into_v8_fn5(
                |format: String, key_data: Vec<u8>, alg_json: String, extractable: bool, usages_json: String| -> String {
                    crate::subtle_crypto::import_key(&format, key_data, &alg_json, extractable, &usages_json)
                },
            ),
        ),
        (
            "_lumen_subtle_export_key",
            into_v8_fn2(|format: String, key_id: u32| -> Vec<u8> {
                crate::subtle_crypto::export_key(&format, key_id).unwrap_or_default()
            }),
        ),
        (
            "_lumen_subtle_export_key_or_err",
            into_v8_fn2(|format: String, key_id: u32| -> String {
                match crate::subtle_crypto::export_key(&format, key_id) {
                    Ok(bytes) => {
                        if bytes.first() == Some(&b'{') || bytes.first() == Some(&b'[') {
                            format!("ok:{}", String::from_utf8_lossy(&bytes))
                        } else {
                            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                            format!("hex:{hex}")
                        }
                    }
                    Err(e) => format!("err:{e}"),
                }
            }),
        ),
        (
            "_lumen_subtle_sign",
            into_v8_fn3(|alg_json: String, key_id: u32, data: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::sign_data(&alg_json, key_id, &data)
            }),
        ),
        (
            "_lumen_subtle_verify",
            into_v8_fn4(|alg_json: String, key_id: u32, sig: Vec<u8>, data: Vec<u8>| -> bool {
                crate::subtle_crypto::verify_signature(&alg_json, key_id, &sig, &data)
            }),
        ),
        (
            "_lumen_subtle_encrypt",
            into_v8_fn4(|key_id: u32, iv: Vec<u8>, aad: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_gcm_encrypt(key_id, &iv, &aad, &plaintext)
            }),
        ),
        (
            "_lumen_subtle_decrypt",
            into_v8_fn4(|key_id: u32, iv: Vec<u8>, aad: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_gcm_decrypt(key_id, &iv, &aad, &ciphertext)
            }),
        ),
        (
            "_lumen_subtle_key_info",
            into_v8_fn1(|key_id: u32| -> String { crate::subtle_crypto::key_info(key_id) }),
        ),
        (
            "_lumen_subtle_aes_cbc_encrypt",
            into_v8_fn3(|key_id: u32, iv: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_cbc_encrypt(key_id, &iv, &plaintext)
            }),
        ),
        (
            "_lumen_subtle_aes_cbc_decrypt",
            into_v8_fn3(|key_id: u32, iv: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_cbc_decrypt(key_id, &iv, &ciphertext)
            }),
        ),
        (
            "_lumen_subtle_aes_ctr_crypt",
            into_v8_fn4(|key_id: u32, counter: Vec<u8>, length: u32, data: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_ctr_crypt(key_id, &counter, length, &data)
            }),
        ),
        (
            "_lumen_subtle_derive_bits",
            into_v8_fn3(|alg_json: String, key_id: u32, length_bits: u32| -> Vec<u8> {
                crate::subtle_crypto::derive_bits(&alg_json, key_id, length_bits)
            }),
        ),
        (
            "_lumen_subtle_rsa_oaep_encrypt",
            into_v8_fn3(|key_id: u32, label: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::rsa_oaep_encrypt(key_id, &label, &plaintext)
            }),
        ),
        (
            "_lumen_subtle_rsa_oaep_decrypt",
            into_v8_fn3(|key_id: u32, label: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::rsa_oaep_decrypt(key_id, &label, &ciphertext)
            }),
        ),
    ]
}
