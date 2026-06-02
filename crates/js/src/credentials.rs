//! `navigator.credentials` (WebAuthn / passkeys) bridge.
//!
//! The JS shim ([`CREDENTIALS_SHIM`]) implements `navigator.credentials.create()`
//! and `.get()` plus `PublicKeyCredential` and friends. It marshals the request
//! to the native bindings `_lumen_webauthn_create` / `_lumen_webauthn_get`
//! (registered in [`crate::dom::install_dom_api`]), which forward to the
//! process-global [`CredentialProvider`] installed by the shell via
//! [`set_credential_provider`]. With no provider installed (headless tests, dump
//! modes), both operations reject with `NotAllowedError` — the privacy-preserving
//! "no authenticator" default.
//!
//! Marshalling avoids JSON parsing in Rust (no `serde_json` in `lumen-js`): the
//! request is packed into a single `|`-separated string whose fields are all
//! base64url (so neither `|` nor `,` can appear inside a field). The response is
//! a small hand-built JSON object whose values are likewise base64url / numbers /
//! fixed strings, so `JSON.parse` on the JS side is safe.
//!
//! Mirrors the process-global pattern of [`crate::clipboard`].

use lumen_core::ext::{
    CredentialProvider, WebAuthnCreateRequest, WebAuthnGetRequest,
};
use rquickjs::Ctx;
use std::sync::{Arc, OnceLock, RwLock};

/// Process-global credential provider, installed once by the shell.
static PROVIDER: OnceLock<RwLock<Option<Arc<dyn CredentialProvider>>>> = OnceLock::new();

/// Lazily-initialised handle to the global provider slot.
fn slot() -> &'static RwLock<Option<Arc<dyn CredentialProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Install the host credential provider backing `navigator.credentials`.
///
/// Called by the shell during startup (typically with a
/// `lumen_network::VirtualAuthenticator`). Replaces any previously installed
/// provider. Safe to call from any thread.
pub fn set_credential_provider(provider: Arc<dyn CredentialProvider>) {
    if let Ok(mut guard) = slot().write() {
        *guard = Some(provider);
    }
}

/// Clone the installed provider, if any.
fn provider() -> Option<Arc<dyn CredentialProvider>> {
    slot().read().ok().and_then(|g| g.clone())
}

/// Install the `navigator.credentials` JS shim.
///
/// Must run after `install_dom_api` (requires `navigator`, `atob`/`btoa`,
/// `Promise`, `DOMException`, `Uint8Array`) and after the `_lumen_webauthn_*`
/// native bindings are registered.
pub fn install_credentials_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(CREDENTIALS_SHIM)?;
    Ok(())
}

/// Native binding `_lumen_webauthn_create(packed)` → JSON string.
///
/// `packed` fields (`|`-separated), each base64url: `rpId | rpName | userId |
/// userName | userDisplayName | challenge | origin | algsCsv | uv(1/0) |
/// excludeCsv`. `algsCsv` is comma-separated decimal COSE ids; `excludeCsv` is
/// comma-separated base64url credential IDs.
pub(crate) fn create(packed: String) -> String {
    let Some(provider) = provider() else {
        return error_json("NotAllowedError");
    };
    let f: Vec<&str> = packed.split('|').collect();
    if f.len() < 10 {
        return error_json("NotSupportedError");
    }
    let req = WebAuthnCreateRequest {
        rp_id: b64url_to_string(f[0]),
        rp_name: b64url_to_string(f[1]),
        user_id: b64url_decode(f[2]),
        user_name: b64url_to_string(f[3]),
        user_display_name: b64url_to_string(f[4]),
        challenge: b64url_decode(f[5]),
        origin: b64url_to_string(f[6]),
        pub_key_algs: parse_i64_csv(f[7]),
        require_user_verification: f[8] == "1",
        exclude_credentials: parse_b64_csv(f[9]),
    };
    match provider.create(&req) {
        Ok(r) => {
            let mut s = String::from("{\"ok\":true");
            push_b64_field(&mut s, "credentialId", &r.credential_id);
            push_b64_field(&mut s, "attestationObject", &r.attestation_object);
            push_b64_field(&mut s, "clientDataJSON", &r.client_data_json);
            push_b64_field(&mut s, "authenticatorData", &r.authenticator_data);
            match &r.public_key_der {
                Some(der) => push_b64_field(&mut s, "publicKey", der),
                None => s.push_str(",\"publicKey\":null"),
            }
            s.push_str(&format!(",\"publicKeyAlgorithm\":{}", r.public_key_alg));
            s.push_str(",\"transports\":[");
            for (i, t) in r.transports.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&format!("\"{}\"", t));
            }
            s.push_str("]}");
            s
        }
        Err(e) => error_json(e.dom_exception_name()),
    }
}

/// Native binding `_lumen_webauthn_get(packed)` → JSON string.
///
/// `packed` fields (`|`-separated), each base64url: `rpId | challenge | origin |
/// allowCsv | uv(1/0)`. `allowCsv` is comma-separated base64url credential IDs.
pub(crate) fn get(packed: String) -> String {
    let Some(provider) = provider() else {
        return error_json("NotAllowedError");
    };
    let f: Vec<&str> = packed.split('|').collect();
    if f.len() < 5 {
        return error_json("NotSupportedError");
    }
    let req = WebAuthnGetRequest {
        rp_id: b64url_to_string(f[0]),
        challenge: b64url_decode(f[1]),
        origin: b64url_to_string(f[2]),
        allow_credentials: parse_b64_csv(f[3]),
        require_user_verification: f[4] == "1",
    };
    match provider.get(&req) {
        Ok(r) => {
            let mut s = String::from("{\"ok\":true");
            push_b64_field(&mut s, "credentialId", &r.credential_id);
            push_b64_field(&mut s, "authenticatorData", &r.authenticator_data);
            push_b64_field(&mut s, "signature", &r.signature);
            push_b64_field(&mut s, "clientDataJSON", &r.client_data_json);
            match &r.user_handle {
                Some(uh) => push_b64_field(&mut s, "userHandle", uh),
                None => s.push_str(",\"userHandle\":null"),
            }
            s.push('}');
            s
        }
        Err(e) => error_json(e.dom_exception_name()),
    }
}

/// Native binding `_lumen_webauthn_uvpa()` →
/// `isUserVerifyingPlatformAuthenticatorAvailable()`.
pub(crate) fn uvpa_available() -> bool {
    provider().is_some_and(|p| p.is_user_verifying_platform_authenticator_available())
}

/// Build the `{ "ok": false, "error": <DOMException name> }` rejection payload.
fn error_json(dom_exception: &str) -> String {
    format!("{{\"ok\":false,\"error\":\"{}\"}}", dom_exception)
}

/// Append `,"name":"<base64url(bytes)>"` to a JSON object being built.
fn push_b64_field(s: &mut String, name: &str, bytes: &[u8]) {
    s.push_str(&format!(",\"{}\":\"{}\"", name, base64url_encode(bytes)));
}

/// Parse a comma-separated list of decimal integers (COSE algorithm ids).
fn parse_i64_csv(s: &str) -> Vec<i64> {
    s.split(',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.trim().parse::<i64>().ok())
        .collect()
}

/// Parse a comma-separated list of base64url credential IDs into raw bytes.
fn parse_b64_csv(s: &str) -> Vec<Vec<u8>> {
    s.split(',')
        .filter(|p| !p.is_empty())
        .map(b64url_decode)
        .collect()
}

/// Decode base64url bytes, then lossily interpret as UTF-8 (for text fields).
fn b64url_to_string(s: &str) -> String {
    String::from_utf8_lossy(&b64url_decode(s)).into_owned()
}

/// Base64url encode without padding (RFC 4648 §5).
fn base64url_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18 & 0x3f) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6 & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        }
    }
    out
}

/// Decode a base64url string (padding and `+`/`/` variants tolerated).
fn b64url_decode(s: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// JavaScript shim: `navigator.credentials` (WebAuthn) + `PublicKeyCredential`.
const CREDENTIALS_SHIM: &str = r#"(function(){
  if (typeof navigator === 'undefined') return;

  function mkErr(name, msg){
    try { return new DOMException(msg || name, name); }
    catch(_) { var e = new Error(msg || name); e.name = name; return e; }
  }
  function bufToB64url(buf){
    var bytes;
    if (buf instanceof ArrayBuffer) bytes = new Uint8Array(buf);
    else if (buf && buf.buffer instanceof ArrayBuffer) bytes = new Uint8Array(buf.buffer, buf.byteOffset||0, buf.byteLength);
    else if (buf == null) bytes = new Uint8Array(0);
    else bytes = new Uint8Array(buf);
    var bin = '';
    for (var i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return btoa(bin).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
  }
  function strToB64url(s){
    var bytes = new TextEncoder().encode(String(s == null ? '' : s));
    return bufToB64url(bytes.buffer);
  }
  function b64urlToBuf(s){
    s = String(s).replace(/-/g,'+').replace(/_/g,'/');
    while (s.length % 4) s += '=';
    var bin = atob(s);
    var bytes = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes.buffer;
  }

  // Constructor stubs so `instanceof` checks in relying-party code work, and so
  // response objects carry the right prototype.
  function AuthenticatorResponse(){}
  function AuthenticatorAttestationResponse(){}
  AuthenticatorAttestationResponse.prototype = Object.create(AuthenticatorResponse.prototype);
  function AuthenticatorAssertionResponse(){}
  AuthenticatorAssertionResponse.prototype = Object.create(AuthenticatorResponse.prototype);
  function Credential(){}
  function PublicKeyCredential(){ throw new TypeError('Illegal constructor'); }
  PublicKeyCredential.prototype = Object.create(Credential.prototype);
  function CredentialsContainer(){}

  function makeAttestation(o){
    var resp = Object.create(AuthenticatorAttestationResponse.prototype);
    resp.attestationObject = b64urlToBuf(o.attestationObject);
    resp.clientDataJSON = b64urlToBuf(o.clientDataJSON);
    resp.getAuthenticatorData = function(){ return b64urlToBuf(o.authenticatorData); };
    resp.getPublicKey = function(){ return o.publicKey ? b64urlToBuf(o.publicKey) : null; };
    resp.getPublicKeyAlgorithm = function(){ return o.publicKeyAlgorithm; };
    resp.getTransports = function(){ return (o.transports || []).slice(); };
    var cred = Object.create(PublicKeyCredential.prototype);
    cred.id = o.credentialId;
    cred.rawId = b64urlToBuf(o.credentialId);
    cred.type = 'public-key';
    cred.authenticatorAttachment = 'platform';
    cred.response = resp;
    cred.getClientExtensionResults = function(){ return {}; };
    cred.toJSON = function(){
      return { id: o.credentialId, rawId: o.credentialId, type: 'public-key', authenticatorAttachment: 'platform',
               response: { attestationObject: o.attestationObject, clientDataJSON: o.clientDataJSON,
                           authenticatorData: o.authenticatorData, publicKeyAlgorithm: o.publicKeyAlgorithm,
                           transports: o.transports || [] },
               clientExtensionResults: {} };
    };
    return cred;
  }

  function makeAssertion(o){
    var resp = Object.create(AuthenticatorAssertionResponse.prototype);
    resp.authenticatorData = b64urlToBuf(o.authenticatorData);
    resp.clientDataJSON = b64urlToBuf(o.clientDataJSON);
    resp.signature = b64urlToBuf(o.signature);
    resp.userHandle = o.userHandle ? b64urlToBuf(o.userHandle) : null;
    var cred = Object.create(PublicKeyCredential.prototype);
    cred.id = o.credentialId;
    cred.rawId = b64urlToBuf(o.credentialId);
    cred.type = 'public-key';
    cred.authenticatorAttachment = 'platform';
    cred.response = resp;
    cred.getClientExtensionResults = function(){ return {}; };
    cred.toJSON = function(){
      return { id: o.credentialId, rawId: o.credentialId, type: 'public-key', authenticatorAttachment: 'platform',
               response: { authenticatorData: o.authenticatorData, clientDataJSON: o.clientDataJSON,
                           signature: o.signature, userHandle: o.userHandle || null },
               clientExtensionResults: {} };
    };
    return cred;
  }

  function currentOrigin(){ try { return location.origin; } catch(_) { return ''; } }
  function currentHost(){ try { return location.hostname; } catch(_) { return ''; } }

  var container = Object.create(CredentialsContainer.prototype);

  container.create = function(options){
    return new Promise(function(resolve, reject){
      try {
        if (!options || !options.publicKey) { reject(mkErr('NotSupportedError', 'publicKey options required')); return; }
        if (typeof _lumen_webauthn_create !== 'function') { reject(mkErr('NotAllowedError', 'no authenticator')); return; }
        var pk = options.publicKey, rp = pk.rp || {}, user = pk.user || {};
        var algs = (pk.pubKeyCredParams || []).map(function(p){ return p.alg; })
                     .filter(function(a){ return typeof a === 'number'; });
        if (!algs.length) algs = [-7];
        var uv = pk.authenticatorSelection && pk.authenticatorSelection.userVerification === 'required';
        var exclude = (pk.excludeCredentials || []).map(function(c){ return bufToB64url(c.id); }).join(',');
        var packed = [
          strToB64url(rp.id || currentHost()),
          strToB64url(rp.name || ''),
          bufToB64url(user.id),
          strToB64url(user.name || ''),
          strToB64url(user.displayName || ''),
          bufToB64url(pk.challenge),
          strToB64url(currentOrigin()),
          algs.join(','),
          uv ? '1' : '0',
          exclude
        ].join('|');
        var o = JSON.parse(_lumen_webauthn_create(packed));
        if (!o.ok) { reject(mkErr(o.error || 'NotAllowedError', 'WebAuthn create failed')); return; }
        resolve(makeAttestation(o));
      } catch(e) { reject(e); }
    });
  };

  container.get = function(options){
    return new Promise(function(resolve, reject){
      try {
        if (!options || !options.publicKey) { reject(mkErr('NotSupportedError', 'publicKey options required')); return; }
        if (typeof _lumen_webauthn_get !== 'function') { reject(mkErr('NotAllowedError', 'no authenticator')); return; }
        var pk = options.publicKey;
        var allow = (pk.allowCredentials || []).map(function(c){ return bufToB64url(c.id); }).join(',');
        var uv = pk.userVerification === 'required';
        var packed = [
          strToB64url(pk.rpId || currentHost()),
          bufToB64url(pk.challenge),
          strToB64url(currentOrigin()),
          allow,
          uv ? '1' : '0'
        ].join('|');
        var o = JSON.parse(_lumen_webauthn_get(packed));
        if (!o.ok) { reject(mkErr(o.error || 'NotAllowedError', 'WebAuthn get failed')); return; }
        resolve(makeAssertion(o));
      } catch(e) { reject(e); }
    });
  };

  // Non-WebAuthn credential types (password/federated) are not stored.
  container.preventSilentAccess = function(){ return Promise.resolve(); };
  container.store = function(c){ return Promise.resolve(c); };

  PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable = function(){
    var ok = (typeof _lumen_webauthn_uvpa === 'function') ? !!_lumen_webauthn_uvpa() : false;
    return Promise.resolve(ok);
  };
  PublicKeyCredential.isConditionalMediationAvailable = function(){ return Promise.resolve(false); };

  try { Object.defineProperty(navigator, 'credentials', { value: container, configurable: true, enumerable: true }); }
  catch(_) { navigator.credentials = container; }

  var g = (typeof globalThis !== 'undefined') ? globalThis : this;
  g.PublicKeyCredential = PublicKeyCredential;
  g.CredentialsContainer = CredentialsContainer;
  g.Credential = Credential;
  g.AuthenticatorResponse = AuthenticatorResponse;
  g.AuthenticatorAttestationResponse = AuthenticatorAttestationResponse;
  g.AuthenticatorAssertionResponse = AuthenticatorAssertionResponse;
})();"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_roundtrip() {
        for case in [vec![], vec![0u8], vec![1, 2, 3], vec![0xfb, 0xff], (0..=255).collect()] {
            let enc = base64url_encode(&case);
            assert!(!enc.contains('+') && !enc.contains('/') && !enc.contains('='));
            assert_eq!(b64url_decode(&enc), case);
        }
    }

    #[test]
    fn b64url_to_string_decodes_utf8() {
        let enc = base64url_encode("https://пример.test".as_bytes());
        assert_eq!(b64url_to_string(&enc), "https://пример.test");
    }

    #[test]
    fn parse_i64_csv_handles_negatives_and_blanks() {
        assert_eq!(parse_i64_csv("-7,-257,"), vec![-7, -257]);
        assert_eq!(parse_i64_csv(""), Vec::<i64>::new());
    }

    #[test]
    fn parse_b64_csv_decodes_ids() {
        let a = base64url_encode(&[1, 2, 3]);
        let b = base64url_encode(&[9, 9]);
        assert_eq!(parse_b64_csv(&format!("{a},{b}")), vec![vec![1, 2, 3], vec![9, 9]]);
        assert!(parse_b64_csv("").is_empty());
    }

    #[test]
    fn create_without_provider_rejects_not_allowed() {
        // No provider installed in this isolated path → NotAllowedError.
        // (Provider is process-global; other tests may install one, so only
        // assert the error shape when none is present.)
        if provider().is_none() {
            let out = create("a|b|c|d|e|f|g|-7|0|".to_owned());
            assert!(out.contains("\"ok\":false"));
            assert!(out.contains("NotAllowedError"));
        }
    }

    #[test]
    fn create_and_get_through_installed_provider() {
        // Install a VirtualAuthenticator-like double via the trait.
        use lumen_core::ext::{
            CredentialProvider, WebAuthnCreateRequest, WebAuthnCreateResponse, WebAuthnError,
            WebAuthnGetRequest, WebAuthnGetResponse,
        };
        struct Echo;
        impl CredentialProvider for Echo {
            fn create(&self, req: &WebAuthnCreateRequest) -> Result<WebAuthnCreateResponse, WebAuthnError> {
                assert_eq!(req.rp_id, "example.com");
                assert_eq!(req.user_name, "alice");
                assert_eq!(req.pub_key_algs, vec![-7]);
                assert!(req.require_user_verification);
                Ok(WebAuthnCreateResponse {
                    credential_id: vec![1, 2, 3],
                    attestation_object: vec![4, 5],
                    client_data_json: b"{}".to_vec(),
                    authenticator_data: vec![6],
                    public_key_alg: -7,
                    public_key_der: None,
                    transports: vec!["internal".to_owned()],
                })
            }
            fn get(&self, req: &WebAuthnGetRequest) -> Result<WebAuthnGetResponse, WebAuthnError> {
                assert_eq!(req.allow_credentials, vec![vec![1, 2, 3]]);
                Ok(WebAuthnGetResponse {
                    credential_id: vec![1, 2, 3],
                    authenticator_data: vec![6],
                    signature: vec![7, 8],
                    client_data_json: b"{}".to_vec(),
                    user_handle: Some(vec![9]),
                })
            }
        }
        set_credential_provider(Arc::new(Echo));

        let rp = base64url_encode(b"example.com");
        let name = base64url_encode(b"alice");
        let uid = base64url_encode(&[0]);
        let chal = base64url_encode(&[1]);
        let origin = base64url_encode(b"https://example.com");
        let packed = format!("{rp}|{rp}|{uid}|{name}|{name}|{chal}|{origin}|-7|1|");
        let out = create(packed);
        assert!(out.contains("\"ok\":true"), "{out}");
        assert!(out.contains("\"publicKey\":null"));
        assert!(out.contains("\"publicKeyAlgorithm\":-7"));
        assert!(out.contains("\"transports\":[\"internal\"]"));
        // credentialId [1,2,3] → base64url "AQID".
        assert!(out.contains("\"credentialId\":\"AQID\""));

        let allow = base64url_encode(&[1, 2, 3]);
        let gpacked = format!("{rp}|{chal}|{origin}|{allow}|1");
        let gout = get(gpacked);
        assert!(gout.contains("\"ok\":true"), "{gout}");
        assert!(gout.contains("\"signature\":\"Bwg\""));
        assert!(gout.contains("\"userHandle\":\"CQ\""));
    }
}
