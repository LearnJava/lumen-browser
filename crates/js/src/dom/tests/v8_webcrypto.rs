//! V8 port of the fifteenth S12b-24 sub-area — Web Crypto API
//! (`crypto.getRandomValues`/`randomUUID`, `crypto.subtle.digest`) plus the
//! SubtleCrypto full API (`generateKey`/`importKey`/`sign`/`encrypt`/`decrypt`/
//! `deriveBits`/`deriveKey`). The `_lumen_subtle_*` natives had no V8 binding at
//! all before this slice (`v8_runtime.rs` carried a literal
//! `TODO(v8-s3, out of scope)` stub) — ported as a thin wrapper over
//! `crate::subtle_crypto`'s pure functions (already engine-agnostic, no `Ctx`
//! dependency) in the same commit as these tests.
//!
//! Async assertions tightened per the S12b-2 lesson: every QuickJS original
//! tolerated either the resolved value or `Null`/pending with a "microtasks may
//! not have flushed" comment, because QuickJS's `eval()` never drained the
//! microtask queue. V8 auto-checkpoints microtasks after each script (the same
//! fact the `queue_microtask_callback_runs_after_sync_tail` test in
//! `v8_perf_observers` established), so the second `eval()` in every two-step
//! test below deterministically observes the fully-resolved promise chain —
//! the loose branches and the double-read "pump" pattern are removed.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn crypto_object_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.crypto === 'object'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn crypto_get_random_values_fills_array() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = new Uint8Array(32);
                     window.crypto.getRandomValues(a);
                     a.length === 32",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn crypto_get_random_values_returns_typed_array() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = new Uint32Array(4);
                     var ret = window.crypto.getRandomValues(a);
                     ret === a",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn crypto_random_uuid_format() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("window.crypto.randomUUID()").unwrap();
    let uuid = match r {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string UUID, got {other:?}"),
    };
    // xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
    assert_eq!(uuid.len(), 36, "UUID length must be 36");
    assert_eq!(&uuid[8..9], "-");
    assert_eq!(&uuid[13..14], "-");
    assert_eq!(&uuid[18..19], "-");
    assert_eq!(&uuid[23..24], "-");
    // version nibble must be '4'
    assert_eq!(&uuid[14..15], "4", "version nibble must be 4");
    // variant nibble must be 8-b
    let variant: u8 = u8::from_str_radix(&uuid[19..20], 16).unwrap();
    assert!((8..=11).contains(&variant), "variant bits must be 10xx");
}

#[test]
fn crypto_random_uuid_unique() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u1 = window.crypto.randomUUID();
                     var u2 = window.crypto.randomUUID();
                     u1 !== u2",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn crypto_subtle_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("typeof window.crypto.subtle === 'object' && typeof window.crypto.subtle.digest === 'function'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn crypto_subtle_digest_sha256_known_vector() {
    // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var result = null;
                     var rejected = false;
                     window.crypto.subtle.digest('SHA-256', new ArrayBuffer(0)).then(function(buf) {
                         var view = new Uint8Array(buf);
                         var hex = Array.from(view).map(function(b){ return ('0'+b.toString(16)).slice(-2); }).join('');
                         result = hex;
                     }).catch(function(e){ rejected = true; });
                     result",
        )
        .unwrap();
    // The completion value is read as the script's last statement, before
    // the end-of-script microtask checkpoint runs `.then()` — so this
    // observes pre-resolution state on both engines. It still proves the
    // promise wasn't rejected synchronously; the resolved value is checked
    // by `crypto_subtle_digest_sha256_with_pump` below via a second eval.
    assert_eq!(r, lumen_core::JsValue::Null);
}

#[test]
fn crypto_subtle_digest_sha256_with_pump() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _sha256_result = null;
                 window.crypto.subtle.digest('SHA-256', new ArrayBuffer(0)).then(function(buf) {
                     var view = new Uint8Array(buf);
                     _sha256_result = Array.from(view).map(function(b){ return ('0'+b.toString(16)).slice(-2); }).join('');
                 });",
    )
    .unwrap();
    let r = rt.eval("_sha256_result").unwrap();
    let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(r, lumen_core::JsValue::String(expected.to_string()));
}

#[test]
fn crypto_subtle_digest_sha1_known_vector() {
    // SHA-1("") = da39a3ee5e6b4b0d3255bfef95601890afd80709
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _sha1_result = null;
                 window.crypto.subtle.digest('SHA-1', new ArrayBuffer(0)).then(function(buf) {
                     var view = new Uint8Array(buf);
                     _sha1_result = Array.from(view).map(function(b){ return ('0'+b.toString(16)).slice(-2); }).join('');
                 });",
    )
    .unwrap();
    let r = rt.eval("_sha1_result").unwrap();
    let expected = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
    assert_eq!(r, lumen_core::JsValue::String(expected.to_string()));
}

#[test]
fn crypto_subtle_digest_unsupported_algo_rejects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _unsup_rejected = false;
                 window.crypto.subtle.digest('MD5', new ArrayBuffer(0)).catch(function(e) {
                     _unsup_rejected = true;
                 });",
    )
    .unwrap();
    let r = rt.eval("_unsup_rejected").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn subtle_generate_key_hmac_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "typeof window.crypto.subtle.generateKey === 'function' && \
                 typeof window.crypto.subtle.sign === 'function' && \
                 typeof window.crypto.subtle.verify === 'function' && \
                 typeof window.crypto.subtle.encrypt === 'function' && \
                 typeof window.crypto.subtle.decrypt === 'function' && \
                 typeof window.crypto.subtle.importKey === 'function' && \
                 typeof window.crypto.subtle.exportKey === 'function'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn subtle_hmac_generate_and_sign_resolves() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _hmac_done = false; var _hmac_sig = null;
                 window.crypto.subtle.generateKey(
                     {name:'HMAC', hash:'SHA-256'},
                     true,
                     ['sign','verify']
                 ).then(function(k) {
                     return window.crypto.subtle.sign('HMAC', k, new TextEncoder().encode('hello'));
                 }).then(function(sig) {
                     _hmac_sig = new Uint8Array(sig).length;
                     _hmac_done = true;
                 });"
    ).unwrap();
    let r = rt.eval("_hmac_done && _hmac_sig === 32").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn subtle_ecdsa_generate_key_pair() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _ec_ok = false;
                 window.crypto.subtle.generateKey(
                     {name:'ECDSA', namedCurve:'P-256'},
                     true,
                     ['sign','verify']
                 ).then(function(kp) {
                     _ec_ok = (kp.privateKey instanceof CryptoKey) && (kp.publicKey instanceof CryptoKey);
                 });"
    ).unwrap();
    let r = rt.eval("_ec_ok").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn subtle_aes_gcm_encrypt_decrypt() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _aes_done = false; var _aes_pt = null;
                 var _aes_iv = new Uint8Array(12);
                 window.crypto.subtle.generateKey(
                     {name:'AES-GCM', length:256},
                     true,
                     ['encrypt','decrypt']
                 ).then(function(k) {
                     var plain = new TextEncoder().encode('secret');
                     return window.crypto.subtle.encrypt(
                         {name:'AES-GCM', iv: _aes_iv},
                         k,
                         plain
                     ).then(function(ct) {
                         return window.crypto.subtle.decrypt(
                             {name:'AES-GCM', iv: _aes_iv},
                             k,
                             ct
                         );
                     });
                 }).then(function(pt) {
                     _aes_pt = new TextDecoder().decode(pt);
                     _aes_done = true;
                 });"
    ).unwrap();
    let r = rt.eval("_aes_done ? _aes_pt : null").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("secret".to_string()));
}

#[test]
fn subtle_crypto_key_is_instance_of_crypto_key() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _ck_ok = false;
                 window.crypto.subtle.generateKey(
                     {name:'AES-GCM', length:128},
                     true,
                     ['encrypt','decrypt']
                 ).then(function(k) {
                     _ck_ok = k instanceof CryptoKey && k.type === 'secret' && k.extractable === true;
                 });"
    ).unwrap();
    let r = rt.eval("_ck_ok").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn subtle_aes_cbc_encrypt_decrypt() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _cbc_done = false; var _cbc_pt = null;
                 var _cbc_iv = new Uint8Array(16);
                 window.crypto.subtle.generateKey(
                     {name:'AES-CBC', length:256},
                     true,
                     ['encrypt','decrypt']
                 ).then(function(k) {
                     var plain = new TextEncoder().encode('AES-CBC test');
                     return window.crypto.subtle.encrypt(
                         {name:'AES-CBC', iv: _cbc_iv},
                         k,
                         plain
                     ).then(function(ct) {
                         return window.crypto.subtle.decrypt(
                             {name:'AES-CBC', iv: _cbc_iv},
                             k,
                             ct
                         );
                     });
                 }).then(function(pt) {
                     _cbc_pt = new TextDecoder().decode(pt);
                     _cbc_done = true;
                 });"
    ).unwrap();
    let r = rt.eval("_cbc_done ? _cbc_pt : null").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("AES-CBC test".to_string()));
}

#[test]
fn subtle_derive_bits_pbkdf2() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _pbkdf2_done = false; var _pbkdf2_len = 0;
                 window.crypto.subtle.importKey(
                     'raw',
                     new TextEncoder().encode('password'),
                     {name:'PBKDF2'},
                     false,
                     ['deriveBits']
                 ).then(function(k) {
                     return window.crypto.subtle.deriveBits(
                         {name:'PBKDF2', hash:'SHA-256',
                          salt: new TextEncoder().encode('salt'),
                          iterations: 1000},
                         k,
                         256
                     );
                 }).then(function(bits) {
                     _pbkdf2_len = new Uint8Array(bits).length;
                     _pbkdf2_done = true;
                 });"
    ).unwrap();
    let r = rt.eval("_pbkdf2_done ? _pbkdf2_len : -1").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(32.0)); // 256 bits = 32 bytes
}

#[test]
fn subtle_derive_key_hkdf_then_aes_gcm() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _hkdf_done = false; var _hkdf_pt = null;
                 window.crypto.subtle.importKey(
                     'raw',
                     new TextEncoder().encode('input-keying-material'),
                     {name:'HKDF'},
                     false,
                     ['deriveKey']
                 ).then(function(baseKey) {
                     return window.crypto.subtle.deriveKey(
                         {name:'HKDF', hash:'SHA-256',
                          salt: new Uint8Array(16),
                          info: new TextEncoder().encode('context')},
                         baseKey,
                         {name:'AES-GCM', length:256},
                         false,
                         ['encrypt','decrypt']
                     );
                 }).then(function(aesKey) {
                     var iv = new Uint8Array(12);
                     var plain = new TextEncoder().encode('hkdf-derived');
                     return window.crypto.subtle.encrypt(
                         {name:'AES-GCM', iv: iv}, aesKey, plain
                     ).then(function(ct) {
                         return window.crypto.subtle.decrypt(
                             {name:'AES-GCM', iv: iv}, aesKey, ct
                         );
                     });
                 }).then(function(pt) {
                     _hkdf_pt = new TextDecoder().decode(pt);
                     _hkdf_done = true;
                 });"
    ).unwrap();
    let r = rt.eval("_hkdf_done ? _hkdf_pt : null").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hkdf-derived".to_string()));
}
