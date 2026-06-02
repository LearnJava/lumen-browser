//! End-to-end test of `navigator.credentials` (WebAuthn) through the real
//! QuickJS runtime and JS shim.
//!
//! Uses a canned [`CredentialProvider`] double (no `lumen-network` dependency):
//! we only verify the JS marshalling — that `create()` / `get()` build the packed
//! request correctly, parse the response JSON, and surface a spec-shaped
//! `PublicKeyCredential` with the right ArrayBuffers, accessors, and prototypes.

use std::sync::{Arc, Mutex, OnceLock};

/// Serialises tests that install the process-global credential provider, so one
/// test's provider cannot service another test's native call (cargo runs tests
/// in parallel within a single process).
fn provider_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

use lumen_core::ext::{
    CredentialProvider, WebAuthnCreateRequest, WebAuthnCreateResponse, WebAuthnError,
    WebAuthnGetRequest, WebAuthnGetResponse,
};
use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::{set_credential_provider, QuickJsRuntime};

/// Records the last request it saw and returns canned, easily-recognisable bytes.
struct CannedAuthenticator {
    last_create: Mutex<Option<WebAuthnCreateRequest>>,
    last_get: Mutex<Option<WebAuthnGetRequest>>,
}

impl CredentialProvider for CannedAuthenticator {
    fn create(&self, req: &WebAuthnCreateRequest) -> Result<WebAuthnCreateResponse, WebAuthnError> {
        *self.last_create.lock().unwrap() = Some(req.clone());
        Ok(WebAuthnCreateResponse {
            credential_id: vec![1, 2, 3], // base64url "AQID"
            attestation_object: vec![10, 20, 30],
            client_data_json: b"{\"type\":\"webauthn.create\"}".to_vec(),
            authenticator_data: vec![40, 50],
            public_key_alg: -7,
            public_key_der: Some(vec![60, 70]),
            transports: vec!["internal".to_owned(), "hybrid".to_owned()],
        })
    }

    fn get(&self, req: &WebAuthnGetRequest) -> Result<WebAuthnGetResponse, WebAuthnError> {
        *self.last_get.lock().unwrap() = Some(req.clone());
        Ok(WebAuthnGetResponse {
            credential_id: vec![1, 2, 3],
            authenticator_data: vec![40, 50],
            signature: vec![7, 8], // base64url "Bwg"
            client_data_json: b"{\"type\":\"webauthn.get\"}".to_vec(),
            user_handle: Some(vec![9]), // base64url "CQ"
        })
    }
}

fn make_rt() -> QuickJsRuntime {
    let rt = QuickJsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/login", None, None, None, None, None)
        .unwrap();
    rt
}

fn bool_eval(rt: &QuickJsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

fn str_eval(rt: &QuickJsRuntime, script: &str) -> String {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::String(s)) => s,
        Ok(other) => panic!("expected string from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

#[test]
fn navigator_credentials_exists() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "typeof navigator.credentials === 'object' && typeof navigator.credentials.create === 'function' && typeof navigator.credentials.get === 'function'"
    ));
    assert!(bool_eval(
        &rt,
        "typeof PublicKeyCredential === 'function' && typeof PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable === 'function'"
    ));
}

#[test]
fn create_returns_public_key_credential() {
    let _guard = provider_lock().lock().unwrap();
    let canned = Arc::new(CannedAuthenticator {
        last_create: Mutex::new(None),
        last_get: Mutex::new(None),
    });
    set_credential_provider(canned.clone());
    let rt = make_rt();

    // Run create(), stash the resolved credential, drain microtasks.
    rt.eval(
        r#"
        globalThis.__cred = null;
        globalThis.__err = null;
        navigator.credentials.create({ publicKey: {
            rp: { id: 'example.com', name: 'Example' },
            user: { id: new Uint8Array([5,6]).buffer, name: 'alice@example.com', displayName: 'Alice' },
            challenge: new Uint8Array([100,101,102]).buffer,
            pubKeyCredParams: [{ type: 'public-key', alg: -7 }, { type: 'public-key', alg: -257 }],
            authenticatorSelection: { userVerification: 'required' }
        }}).then(function(c){ globalThis.__cred = c; }, function(e){ globalThis.__err = e.name; });
        _lumen_drain_microtasks();
        "#,
    )
    .unwrap();

    assert_eq!(str_eval(&rt, "String(__err)"), "null", "create rejected");
    assert!(bool_eval(&rt, "__cred !== null"), "credential not resolved");
    assert_eq!(str_eval(&rt, "__cred.type"), "public-key");
    assert_eq!(str_eval(&rt, "__cred.id"), "AQID"); // base64url([1,2,3])
    assert!(bool_eval(&rt, "__cred.rawId instanceof ArrayBuffer && __cred.rawId.byteLength === 3"));
    assert!(bool_eval(
        &rt,
        "__cred.response instanceof AuthenticatorAttestationResponse && __cred instanceof PublicKeyCredential"
    ));
    assert!(bool_eval(
        &rt,
        "__cred.response.attestationObject instanceof ArrayBuffer && __cred.response.attestationObject.byteLength === 3"
    ));
    assert!(bool_eval(&rt, "__cred.response.getPublicKeyAlgorithm() === -7"));
    assert!(bool_eval(&rt, "__cred.response.getAuthenticatorData().byteLength === 2"));
    assert!(bool_eval(&rt, "__cred.response.getPublicKey().byteLength === 2"));
    assert!(bool_eval(
        &rt,
        "JSON.stringify(__cred.response.getTransports()) === '[\"internal\",\"hybrid\"]'"
    ));

    // The provider saw a correctly-unpacked request.
    let req = canned.last_create.lock().unwrap().clone().unwrap();
    assert_eq!(req.rp_id, "example.com");
    assert_eq!(req.rp_name, "Example");
    assert_eq!(req.user_name, "alice@example.com");
    assert_eq!(req.user_display_name, "Alice");
    assert_eq!(req.user_id, vec![5, 6]);
    assert_eq!(req.challenge, vec![100, 101, 102]);
    assert_eq!(req.origin, "https://example.com");
    assert_eq!(req.pub_key_algs, vec![-7, -257]);
    assert!(req.require_user_verification);
}

#[test]
fn get_returns_assertion() {
    let _guard = provider_lock().lock().unwrap();
    let canned = Arc::new(CannedAuthenticator {
        last_create: Mutex::new(None),
        last_get: Mutex::new(None),
    });
    set_credential_provider(canned.clone());
    let rt = make_rt();

    rt.eval(
        r#"
        globalThis.__a = null;
        globalThis.__aerr = null;
        navigator.credentials.get({ publicKey: {
            challenge: new Uint8Array([1,2,3,4]).buffer,
            rpId: 'example.com',
            allowCredentials: [{ type: 'public-key', id: new Uint8Array([1,2,3]).buffer }],
            userVerification: 'required'
        }}).then(function(a){ globalThis.__a = a; }, function(e){ globalThis.__aerr = e.name; });
        _lumen_drain_microtasks();
        "#,
    )
    .unwrap();

    assert_eq!(str_eval(&rt, "String(__aerr)"), "null", "get rejected");
    assert!(bool_eval(&rt, "__a !== null"));
    assert!(bool_eval(
        &rt,
        "__a.response instanceof AuthenticatorAssertionResponse"
    ));
    assert!(bool_eval(&rt, "__a.response.signature.byteLength === 2"));
    assert!(bool_eval(&rt, "__a.response.userHandle.byteLength === 1"));
    assert!(bool_eval(&rt, "__a.response.authenticatorData.byteLength === 2"));

    let req = canned.last_get.lock().unwrap().clone().unwrap();
    assert_eq!(req.rp_id, "example.com");
    assert_eq!(req.challenge, vec![1, 2, 3, 4]);
    assert_eq!(req.origin, "https://example.com");
    assert_eq!(req.allow_credentials, vec![vec![1, 2, 3]]);
    assert!(req.require_user_verification);
}

#[test]
fn create_without_public_key_rejects_not_supported() {
    let rt = make_rt();
    rt.eval(
        r#"
        globalThis.__e2 = '';
        navigator.credentials.create({ password: {} }).then(
            function(){ globalThis.__e2 = 'resolved'; },
            function(e){ globalThis.__e2 = e.name; });
        _lumen_drain_microtasks();
        "#,
    )
    .unwrap();
    assert_eq!(str_eval(&rt, "__e2"), "NotSupportedError");
}
