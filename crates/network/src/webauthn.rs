//! Software WebAuthn authenticator (passkeys) backing `navigator.credentials`.
//!
//! [`VirtualAuthenticator`] implements [`lumen_core::ext::CredentialProvider`]
//! with a real ES256 (COSE alg `-7`, ECDSA over NIST P-256) key store. It plays
//! both roles the WebAuthn ceremony splits between the client and the
//! authenticator: it builds `clientDataJSON`, hashes it, generates / looks up the
//! credential key, assembles `authenticatorData`, and signs assertions.
//!
//! Signatures are deterministic (RFC 6979), so a given (key, message) pair always
//! yields the same signature — no RNG is consulted at sign time, which makes the
//! output reproducible in tests. Key generation and credential IDs use the OS
//! CSPRNG (`getrandom`).
//!
//! Attestation format is `"none"` (W3C WebAuthn L2 §8.7): the AAGUID is all
//! zeros and `attStmt` is empty. This is the privacy-preserving choice and means
//! the relying party learns nothing about the authenticator model — matching
//! Lumen's anti-fingerprinting stance (ADR-007). The produced credentials are
//! standards-compliant: the public key is a valid COSE EC2 key and assertion
//! signatures verify against it with any conformant relying-party library.
//!
//! Scope: this is a platform-authenticator stand-in (resident/discoverable
//! credentials, user-verifying). A roaming CTAP2-over-USB transport is future
//! work; the [`CredentialProvider`] trait is the seam where it would slot in.

use lumen_core::ext::{
    CredentialProvider, WebAuthnCreateRequest, WebAuthnCreateResponse, WebAuthnError,
    WebAuthnGetRequest, WebAuthnGetResponse,
};
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

/// COSE algorithm identifier for ES256 (ECDSA w/ SHA-256), the only algorithm
/// this authenticator generates keys for. See the IANA COSE Algorithms registry.
const COSE_ES256: i64 = -7;

/// `authenticatorData` flag bits (W3C WebAuthn L2 §6.1).
const FLAG_USER_PRESENT: u8 = 0x01;
const FLAG_USER_VERIFIED: u8 = 0x04;
const FLAG_ATTESTED_CREDENTIAL_DATA: u8 = 0x40;

/// A credential held by the [`VirtualAuthenticator`].
struct StoredCredential {
    /// Relying-party id this credential is scoped to.
    rp_id: String,
    /// Opaque user handle returned on assertion.
    user_id: Vec<u8>,
    /// The ES256 private signing key.
    signing_key: SigningKey,
    /// Per-credential signature counter, incremented on each assertion.
    sign_count: u32,
}

/// In-memory software authenticator: generates and stores ES256 passkeys and
/// produces standards-compliant attestation / assertion responses.
///
/// Thread-safe (`Send + Sync`): credentials live behind a `Mutex`. A single
/// instance models one platform authenticator shared by the process; install it
/// process-globally via `lumen_js::set_credential_provider`.
#[derive(Default)]
pub struct VirtualAuthenticator {
    /// credentialId → stored key material.
    credentials: Mutex<HashMap<Vec<u8>, StoredCredential>>,
}

impl VirtualAuthenticator {
    /// Create an empty authenticator with no registered credentials.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of credentials currently registered (test / introspection helper).
    pub fn credential_count(&self) -> usize {
        self.credentials.lock().map(|m| m.len()).unwrap_or(0)
    }
}

impl CredentialProvider for VirtualAuthenticator {
    fn create(
        &self,
        req: &WebAuthnCreateRequest,
    ) -> Result<WebAuthnCreateResponse, WebAuthnError> {
        // We only implement ES256. Reject if the RP did not offer it.
        if req.pub_key_algs.is_empty() {
            return Err(WebAuthnError::NotSupported);
        }
        if !req.pub_key_algs.contains(&COSE_ES256) {
            return Err(WebAuthnError::Constraint);
        }

        let mut creds = self.credentials.lock().map_err(|_| WebAuthnError::NotAllowed)?;

        // If any already-registered credential is in the exclude list, the spec
        // requires creation to fail with InvalidStateError.
        if req
            .exclude_credentials
            .iter()
            .any(|id| creds.contains_key(id))
        {
            return Err(WebAuthnError::InvalidState);
        }

        let signing_key = generate_es256_key();
        let credential_id = random_bytes(16);

        let client_data_json = build_client_data_json("webauthn.create", &req.challenge, &req.origin);

        let cose_key = cose_es256_public_key(&signing_key);
        let authenticator_data = build_authenticator_data(
            &req.rp_id,
            req.require_user_verification,
            0,
            Some(AttestedCredentialData {
                credential_id: &credential_id,
                cose_public_key: &cose_key,
            }),
        );
        let attestation_object = build_none_attestation_object(&authenticator_data);
        let public_key_der = spki_der_for_es256(&signing_key);

        creds.insert(
            credential_id.clone(),
            StoredCredential {
                rp_id: req.rp_id.clone(),
                user_id: req.user_id.clone(),
                signing_key,
                sign_count: 0,
            },
        );

        Ok(WebAuthnCreateResponse {
            credential_id,
            attestation_object,
            client_data_json: client_data_json.into_bytes(),
            authenticator_data,
            public_key_alg: COSE_ES256,
            public_key_der: Some(public_key_der),
            transports: vec!["internal".to_owned()],
        })
    }

    fn get(&self, req: &WebAuthnGetRequest) -> Result<WebAuthnGetResponse, WebAuthnError> {
        let mut creds = self.credentials.lock().map_err(|_| WebAuthnError::NotAllowed)?;

        // Resolve which credential to use: first match of allow_credentials scoped
        // to rp_id, or — for a discoverable-credential request (empty allow list) —
        // any credential registered for rp_id.
        let credential_id = if req.allow_credentials.is_empty() {
            creds
                .iter()
                .find(|(_, c)| c.rp_id == req.rp_id)
                .map(|(id, _)| id.clone())
        } else {
            req.allow_credentials
                .iter()
                .find(|id| creds.get(*id).is_some_and(|c| c.rp_id == req.rp_id))
                .cloned()
        };
        let Some(credential_id) = credential_id else {
            // No usable credential — privacy-preserving NotAllowedError (the RP
            // cannot tell "unknown credential" from "user declined").
            return Err(WebAuthnError::NotAllowed);
        };

        let cred = creds
            .get_mut(&credential_id)
            .ok_or(WebAuthnError::NotAllowed)?;
        cred.sign_count = cred.sign_count.wrapping_add(1);

        let authenticator_data =
            build_authenticator_data(&req.rp_id, req.require_user_verification, cred.sign_count, None);
        let client_data_json = build_client_data_json("webauthn.get", &req.challenge, &req.origin);

        // signature = Sign(authenticatorData || SHA-256(clientDataJSON)).
        let client_data_hash = Sha256::digest(client_data_json.as_bytes());
        let mut signed = authenticator_data.clone();
        signed.extend_from_slice(&client_data_hash);
        let signature: Signature = cred.signing_key.sign(&signed);

        Ok(WebAuthnGetResponse {
            credential_id,
            authenticator_data,
            signature: signature.to_der().as_bytes().to_vec(),
            client_data_json: client_data_json.into_bytes(),
            user_handle: Some(cred.user_id.clone()),
        })
    }
}

/// Attested credential data assembled into `authenticatorData` on registration.
struct AttestedCredentialData<'a> {
    /// The freshly minted credential ID.
    credential_id: &'a [u8],
    /// CBOR-encoded COSE public key.
    cose_public_key: &'a [u8],
}

/// Build `authenticatorData` (W3C WebAuthn L2 §6.1).
///
/// Layout: `rpIdHash(32) || flags(1) || signCount(4 BE)` optionally followed by
/// attested credential data (`aaguid(16) || credIdLen(2 BE) || credId || cose`).
fn build_authenticator_data(
    rp_id: &str,
    user_verified: bool,
    sign_count: u32,
    attested: Option<AttestedCredentialData<'_>>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(37);
    out.extend_from_slice(&Sha256::digest(rp_id.as_bytes()));

    let mut flags = FLAG_USER_PRESENT;
    if user_verified {
        flags |= FLAG_USER_VERIFIED;
    }
    if attested.is_some() {
        flags |= FLAG_ATTESTED_CREDENTIAL_DATA;
    }
    out.push(flags);
    out.extend_from_slice(&sign_count.to_be_bytes());

    if let Some(att) = attested {
        // AAGUID: all zeros for "none" attestation (no model disclosure).
        out.extend_from_slice(&[0u8; 16]);
        let len = u16::try_from(att.credential_id.len()).unwrap_or(0);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(att.credential_id);
        out.extend_from_slice(att.cose_public_key);
    }
    out
}

/// Build the CBOR attestation object with format `"none"` (W3C WebAuthn §8.7).
///
/// `{ "fmt": "none", "attStmt": {}, "authData": <bytes> }` in CTAP2 canonical
/// CBOR (text keys ordered by length: `fmt` < `attStmt` < `authData`).
fn build_none_attestation_object(auth_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    cbor_map_header(&mut out, 3);
    cbor_text(&mut out, "fmt");
    cbor_text(&mut out, "none");
    cbor_text(&mut out, "attStmt");
    cbor_map_header(&mut out, 0);
    cbor_text(&mut out, "authData");
    cbor_bytes(&mut out, auth_data);
    out
}

/// Encode the public key of `key` as a COSE_Key EC2 structure (RFC 9052 §7).
///
/// CBOR map with integer keys in CTAP2 canonical order: `1`(kty)=`2`(EC2),
/// `3`(alg)=`-7`(ES256), `-1`(crv)=`1`(P-256), `-2`(x)=32 bytes, `-3`(y)=32 bytes.
fn cose_es256_public_key(key: &SigningKey) -> Vec<u8> {
    let point = key.verifying_key().to_encoded_point(false);
    // Uncompressed SEC1 point: 0x04 || X(32) || Y(32).
    let bytes = point.as_bytes();
    let x = &bytes[1..33];
    let y = &bytes[33..65];

    let mut out = Vec::new();
    cbor_map_header(&mut out, 5);
    cbor_uint(&mut out, 1); // kty
    cbor_uint(&mut out, 2); // EC2
    cbor_uint(&mut out, 3); // alg
    cbor_int(&mut out, COSE_ES256); // ES256
    cbor_int(&mut out, -1); // crv
    cbor_uint(&mut out, 1); // P-256
    cbor_int(&mut out, -2); // x
    cbor_bytes(&mut out, x);
    cbor_int(&mut out, -3); // y
    cbor_bytes(&mut out, y);
    out
}

/// SubjectPublicKeyInfo DER for an ES256 (P-256) public key (`getPublicKey()`).
///
/// Uses the fixed 26-byte ASN.1 prefix for `id-ecPublicKey` + `prime256v1`
/// followed by the 65-byte uncompressed SEC1 point.
fn spki_der_for_es256(key: &SigningKey) -> Vec<u8> {
    const SPKI_P256_PREFIX: [u8; 26] = [
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    ];
    let point = key.verifying_key().to_encoded_point(false);
    let mut out = Vec::with_capacity(SPKI_P256_PREFIX.len() + 65);
    out.extend_from_slice(&SPKI_P256_PREFIX);
    out.extend_from_slice(point.as_bytes());
    out
}

/// Serialise `clientDataJSON` (W3C WebAuthn L2 §5.8.1).
///
/// `{"type":<type>,"challenge":<base64url>,"origin":<origin>,"crossOrigin":false}`.
fn build_client_data_json(ceremony_type: &str, challenge: &[u8], origin: &str) -> String {
    format!(
        "{{\"type\":\"{}\",\"challenge\":\"{}\",\"origin\":\"{}\",\"crossOrigin\":false}}",
        json_escape(ceremony_type),
        base64url_encode(challenge),
        json_escape(origin),
    )
}

// ── CBOR primitives (subset needed for COSE keys + attestation objects) ──────

/// Write a CBOR major-type-5 (map) header for `n` entries (`n` ≤ 23 in practice).
fn cbor_map_header(out: &mut Vec<u8>, n: u8) {
    debug_assert!(n <= 23, "only small maps are emitted here");
    out.push(0xa0 | n);
}

/// Write a CBOR unsigned integer (major type 0).
fn cbor_uint(out: &mut Vec<u8>, v: u64) {
    cbor_type_len(out, 0x00, v);
}

/// Write a CBOR integer, choosing unsigned (major 0) or negative (major 1).
fn cbor_int(out: &mut Vec<u8>, v: i64) {
    if v >= 0 {
        cbor_uint(out, v as u64);
    } else {
        // Negative integer encodes n = -1 - v with major type 1.
        cbor_type_len(out, 0x20, (-1 - v) as u64);
    }
}

/// Write a CBOR text string (major type 3).
fn cbor_text(out: &mut Vec<u8>, s: &str) {
    cbor_type_len(out, 0x60, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// Write a CBOR byte string (major type 2).
fn cbor_bytes(out: &mut Vec<u8>, b: &[u8]) {
    cbor_type_len(out, 0x40, b.len() as u64);
    out.extend_from_slice(b);
}

/// Write a CBOR head (major type in the high bits of `major`) with `len` using
/// the shortest applicable additional-info encoding.
fn cbor_type_len(out: &mut Vec<u8>, major: u8, len: u64) {
    if len <= 23 {
        out.push(major | len as u8);
    } else if len <= 0xff {
        out.push(major | 24);
        out.push(len as u8);
    } else if len <= 0xffff {
        out.push(major | 25);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(major | 26);
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
}

// ── small dependency-free helpers ───────────────────────────────────────────

/// Generate a fresh ES256 (P-256) signing key from OS randomness.
///
/// Loops until 32 random bytes form a valid scalar in `[1, n)`; the rejection
/// probability per draw is ~2⁻³², so this effectively never iterates twice.
fn generate_es256_key() -> SigningKey {
    loop {
        let bytes = random_bytes(32);
        if let Ok(key) = SigningKey::from_slice(&bytes) {
            return key;
        }
    }
}

/// Fill `n` bytes from the OS CSPRNG. Panics only if the OS RNG is unavailable,
/// which is unrecoverable for a security primitive.
fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).expect("OS CSPRNG unavailable");
    buf
}

/// Base64url encode without padding (RFC 4648 §5) — the WebAuthn buffer encoding.
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

/// Escape a string for embedding inside a JSON double-quoted value.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;

    fn create_req(algs: Vec<i64>) -> WebAuthnCreateRequest {
        WebAuthnCreateRequest {
            rp_id: "example.com".to_owned(),
            rp_name: "Example".to_owned(),
            user_id: vec![1, 2, 3, 4],
            user_name: "alice@example.com".to_owned(),
            user_display_name: "Alice".to_owned(),
            challenge: vec![9, 8, 7, 6, 5],
            origin: "https://example.com".to_owned(),
            pub_key_algs: algs,
            require_user_verification: true,
            exclude_credentials: vec![],
        }
    }

    #[test]
    fn create_then_get_roundtrip_signature_verifies() {
        let auth = VirtualAuthenticator::new();
        let reg = auth.create(&create_req(vec![COSE_ES256])).unwrap();
        assert_eq!(auth.credential_count(), 1);
        assert_eq!(reg.public_key_alg, COSE_ES256);
        assert!(reg.transports.contains(&"internal".to_owned()));

        // Recover the public key from the SPKI DER and verify a fresh assertion.
        let spki = reg.public_key_der.clone().unwrap();
        let vk = VerifyingKey::from_sec1_bytes(&spki[spki.len() - 65..]).unwrap();

        let get_req = WebAuthnGetRequest {
            rp_id: "example.com".to_owned(),
            challenge: vec![42, 42, 42],
            origin: "https://example.com".to_owned(),
            allow_credentials: vec![reg.credential_id.clone()],
            require_user_verification: true,
        };
        let assertion = auth.get(&get_req).unwrap();
        assert_eq!(assertion.credential_id, reg.credential_id);
        assert_eq!(assertion.user_handle, Some(vec![1, 2, 3, 4]));

        // signature is over authData || SHA-256(clientDataJSON).
        let mut signed = assertion.authenticator_data.clone();
        signed.extend_from_slice(&Sha256::digest(&assertion.client_data_json));
        let sig = Signature::from_der(&assertion.signature).unwrap();
        assert!(vk.verify(&signed, &sig).is_ok());
    }

    #[test]
    fn rejects_when_es256_not_offered() {
        let auth = VirtualAuthenticator::new();
        // RS256 only (-257) — unsupported.
        let err = auth.create(&create_req(vec![-257])).unwrap_err();
        assert_eq!(err, WebAuthnError::Constraint);
    }

    #[test]
    fn rejects_empty_algorithm_list() {
        let auth = VirtualAuthenticator::new();
        let err = auth.create(&create_req(vec![])).unwrap_err();
        assert_eq!(err, WebAuthnError::NotSupported);
    }

    #[test]
    fn exclude_credentials_triggers_invalid_state() {
        let auth = VirtualAuthenticator::new();
        let reg = auth.create(&create_req(vec![COSE_ES256])).unwrap();
        let mut req = create_req(vec![COSE_ES256]);
        req.exclude_credentials = vec![reg.credential_id.clone()];
        assert_eq!(auth.create(&req).unwrap_err(), WebAuthnError::InvalidState);
    }

    #[test]
    fn get_without_credential_is_not_allowed() {
        let auth = VirtualAuthenticator::new();
        let req = WebAuthnGetRequest {
            rp_id: "example.com".to_owned(),
            challenge: vec![1],
            origin: "https://example.com".to_owned(),
            allow_credentials: vec![],
            require_user_verification: false,
        };
        assert_eq!(auth.get(&req).unwrap_err(), WebAuthnError::NotAllowed);
    }

    #[test]
    fn discoverable_credential_get_with_empty_allow_list() {
        let auth = VirtualAuthenticator::new();
        let reg = auth.create(&create_req(vec![COSE_ES256])).unwrap();
        let req = WebAuthnGetRequest {
            rp_id: "example.com".to_owned(),
            challenge: vec![5, 5],
            origin: "https://example.com".to_owned(),
            allow_credentials: vec![],
            require_user_verification: true,
        };
        let assertion = auth.get(&req).unwrap();
        assert_eq!(assertion.credential_id, reg.credential_id);
    }

    #[test]
    fn sign_count_increments_per_assertion() {
        let auth = VirtualAuthenticator::new();
        let reg = auth.create(&create_req(vec![COSE_ES256])).unwrap();
        let req = WebAuthnGetRequest {
            rp_id: "example.com".to_owned(),
            challenge: vec![0],
            origin: "https://example.com".to_owned(),
            allow_credentials: vec![reg.credential_id.clone()],
            require_user_verification: false,
        };
        let a1 = auth.get(&req).unwrap();
        let a2 = auth.get(&req).unwrap();
        // signCount lives in authData bytes [33..37] (big-endian u32).
        let c1 = u32::from_be_bytes(a1.authenticator_data[33..37].try_into().unwrap());
        let c2 = u32::from_be_bytes(a2.authenticator_data[33..37].try_into().unwrap());
        assert_eq!(c1, 1);
        assert_eq!(c2, 2);
    }

    #[test]
    fn get_scoped_to_rp_id() {
        let auth = VirtualAuthenticator::new();
        let reg = auth.create(&create_req(vec![COSE_ES256])).unwrap();
        // Right credential id but wrong rp_id → no match → NotAllowed.
        let req = WebAuthnGetRequest {
            rp_id: "evil.com".to_owned(),
            challenge: vec![1],
            origin: "https://evil.com".to_owned(),
            allow_credentials: vec![reg.credential_id.clone()],
            require_user_verification: false,
        };
        assert_eq!(auth.get(&req).unwrap_err(), WebAuthnError::NotAllowed);
    }

    #[test]
    fn client_data_json_is_well_formed() {
        let json = build_client_data_json("webauthn.create", &[0, 1, 2], "https://a.test");
        assert!(json.contains("\"type\":\"webauthn.create\""));
        assert!(json.contains("\"origin\":\"https://a.test\""));
        assert!(json.contains("\"crossOrigin\":false"));
        // base64url of [0,1,2] = "AAEC".
        assert!(json.contains("\"challenge\":\"AAEC\""));
    }

    #[test]
    fn authenticator_data_flags_create_vs_get() {
        let create = build_authenticator_data(
            "example.com",
            true,
            0,
            Some(AttestedCredentialData {
                credential_id: &[1, 2],
                cose_public_key: &[9],
            }),
        );
        // create: UP|UV|AT.
        assert_eq!(
            create[32],
            FLAG_USER_PRESENT | FLAG_USER_VERIFIED | FLAG_ATTESTED_CREDENTIAL_DATA
        );
        let get = build_authenticator_data("example.com", false, 7, None);
        assert_eq!(get[32], FLAG_USER_PRESENT);
        assert_eq!(get.len(), 37); // no attested data
        assert_eq!(u32::from_be_bytes(get[33..37].try_into().unwrap()), 7);
    }

    #[test]
    fn cose_key_is_canonical_ec2() {
        let key = generate_es256_key();
        let cose = cose_es256_public_key(&key);
        // map(5) header, then key 1 (0x01) value 2 (0x02), key 3 (0x03) value -7 (0x26).
        assert_eq!(cose[0], 0xa5);
        assert_eq!(&cose[1..4], &[0x01, 0x02, 0x03]);
        assert_eq!(cose[4], 0x26); // ES256 = -7
    }

    #[test]
    fn base64url_no_padding() {
        assert_eq!(base64url_encode(&[]), "");
        assert_eq!(base64url_encode(&[0]), "AA");
        assert_eq!(base64url_encode(&[0, 1, 2]), "AAEC");
        // bytes that exercise the - and _ chars (62/63).
        assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn cbor_negative_integers() {
        let mut out = Vec::new();
        cbor_int(&mut out, -1);
        cbor_int(&mut out, -7);
        assert_eq!(out, vec![0x20, 0x26]);
    }
}
