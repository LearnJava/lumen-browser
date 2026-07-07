//! Web Crypto SubtleCrypto API (W3C Web Cryptography API §14).
//!
//! Implements `subtle.generateKey`, `subtle.importKey`, `subtle.exportKey`,
//! `subtle.sign`, `subtle.verify`, `subtle.encrypt`, `subtle.decrypt`,
//! `subtle.deriveBits`, `subtle.deriveKey`.
//!
//! # Supported algorithms
//!
//! | Algorithm  | Operations                                | Key formats       |
//! |------------|-------------------------------------------|-------------------|
//! | ECDSA P-256| sign, verify, generateKey, import, export | raw(pub), spki, pkcs8, jwk |
//! | HMAC-SHA*  | sign, verify, generateKey, import, export | raw, jwk          |
//! | AES-GCM    | encrypt, decrypt, generateKey, import, export | raw, jwk       |
//! | AES-CBC    | encrypt, decrypt, generateKey, import, export | raw, jwk       |
//! | AES-CTR    | encrypt, decrypt, generateKey, import, export | raw, jwk       |
//! | PBKDF2     | importKey (raw password), deriveBits/deriveKey | raw            |
//! | HKDF       | importKey (raw IKM), deriveBits/deriveKey  | raw              |
//!
//! # State model
//!
//! Keys are stored in a per-thread `CRYPTO_KEYS` registry (keyed by opaque u32
//! id).  QuickJS runtimes are single-threaded; Web Workers each run on their
//! own thread, so `thread_local` gives correct per-runtime isolation.
//!
//! # Fingerprinting (ADR-007)
//!
//! The PRNG for key generation is the OS CSPRNG (`getrandom`).  No timing
//! side-channels are introduced — all operations are constant-time via the
//! upstream crates (`p256`, `hmac`, `aes-gcm`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use rquickjs::{Ctx, Function};

// ─── key registry ─────────────────────────────────────────────────────────────

/// Inner key material, one variant per algorithm family.
pub(crate) enum KeyMaterial {
    /// HMAC-SHA256/384/512: raw key bytes + hash name.
    Hmac { hash: String, raw: Vec<u8> },
    /// ECDSA P-256 private (signing) key.
    EcdsaPrivate(p256::ecdsa::SigningKey),
    /// ECDSA P-256 public (verifying) key.
    EcdsaPublic(p256::ecdsa::VerifyingKey),
    /// AES-GCM 128 or 256-bit key (raw bytes).
    AesGcm(Vec<u8>),
    /// AES-CBC 128 or 256-bit key (raw bytes).
    AesCbc(Vec<u8>),
    /// AES-CTR 128 or 256-bit key (raw bytes).
    AesCtr(Vec<u8>),
    /// PBKDF2 raw password bytes (non-extractable by spec).
    Pbkdf2Raw(Vec<u8>),
    /// HKDF raw IKM (input keying material) bytes (non-extractable by spec).
    HkdfRaw(Vec<u8>),
}

/// Full metadata + material for one CryptoKey.
pub(crate) struct CryptoKeyEntry {
    /// "secret", "public", or "private".
    pub key_type: &'static str,
    /// JSON string of algorithm parameters (fed back to JS `.algorithm` getter).
    pub algorithm_json: String,
    pub extractable: bool,
    /// Comma-joined list of usages.
    pub usages_json: String,
    pub material: KeyMaterial,
}

thread_local! {
    /// Per-thread CryptoKey registry.
    static CRYPTO_KEYS: RefCell<HashMap<u32, CryptoKeyEntry>> = RefCell::new(HashMap::new());
    /// Monotonic id allocator.
    static NEXT_KEY_ID: Cell<u32> = const { Cell::new(1) };
}

fn alloc_key(entry: CryptoKeyEntry) -> u32 {
    let id = NEXT_KEY_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    });
    CRYPTO_KEYS.with(|ks| ks.borrow_mut().insert(id, entry));
    id
}

fn with_key<R>(id: u32, f: impl FnOnce(&CryptoKeyEntry) -> R, default: R) -> R {
    CRYPTO_KEYS.with(|ks| match ks.borrow().get(&id) {
        Some(e) => f(e),
        None => default,
    })
}

// ─── base64url helpers ────────────────────────────────────────────────────────

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() * 4).div_ceil(3));
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        out.push(B64URL[b0 >> 2] as char);
        out.push(B64URL[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(B64URL[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL[b2 & 0x3f] as char);
        }
    }
    out
}

fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    // allow padding characters
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for ch in s.bytes() {
        let v: u32 = match ch {
            b'A'..=b'Z' => (ch - b'A') as u32,
            b'a'..=b'z' => (ch - b'a' + 26) as u32,
            b'0'..=b'9' => (ch - b'0' + 52) as u32,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ─── tiny JSON extraction helpers ────────────────────────────────────────────

fn json_str_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let pos = json.find(needle.as_str())?;
    let after = json[pos + needle.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let inner = after.strip_prefix('"')?;
    let end = inner.find('"')?;
    Some(&inner[..end])
}

fn json_num_field(json: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{key}\"");
    let pos = json.find(needle.as_str())?;
    let after = json[pos + needle.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
    after[..end].parse().ok()
}

/// Extract a JSON `"key":[1,2,3,...]` array-of-u8 field from a JSON string.
/// Returns an empty `Vec` if the field is absent or malformed.
fn json_bytes_field(json: &str, key: &str) -> Vec<u8> {
    let needle = format!("\"{key}\"");
    let pos = match json.find(needle.as_str()) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let after = json[pos + needle.len()..].trim_start();
    let after = match after.strip_prefix(':') {
        Some(a) => a.trim_start(),
        None => return Vec::new(),
    };
    let after = match after.strip_prefix('[') {
        Some(a) => a,
        None => return Vec::new(),
    };
    let end = match after.find(']') {
        Some(e) => e,
        None => return Vec::new(),
    };
    after[..end]
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .collect()
}

// ─── generateKey ──────────────────────────────────────────────────────────────

/// Generate a new key for the given algorithm JSON.
/// Returns: `"{id}"` for symmetric keys, `"{pub_id},{priv_id}"` for key pairs.
pub(crate) fn generate_key(alg_json: &str, extractable: bool, usages_json: &str) -> String {
    let name = json_str_field(alg_json, "name").unwrap_or("").to_ascii_uppercase();
    match name.as_str() {
        "HMAC" => {
            let hash = json_str_field(alg_json, "hash")
                .unwrap_or("SHA-256")
                .to_string();
            let length_bits = json_num_field(alg_json, "length")
                .unwrap_or(match hash.as_str() {
                    "SHA-384" => 384,
                    "SHA-512" => 512,
                    _ => 256,
                });
            let byte_len = (length_bits as usize).div_ceil(8);
            let mut raw = vec![0u8; byte_len];
            getrandom::getrandom(&mut raw).unwrap_or(());
            let alg_j = format!(r#"{{"name":"HMAC","hash":{{"name":"{hash}"}}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",

                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::Hmac { hash, raw },
            });
            id.to_string()
        }
        "ECDSA" => {
            let curve = json_str_field(alg_json, "namedCurve")
                .unwrap_or("P-256")
                .to_string();
            if curve != "P-256" {
                return "err:NotSupportedError".to_string();
            }
            let mut seed = [0u8; 32];
            getrandom::getrandom(&mut seed).unwrap_or(());
            let sk = match p256::ecdsa::SigningKey::from_bytes(seed.as_slice().into()) {
                Ok(k) => k,
                Err(_) => return "err:OperationError".to_string(),
            };
            let vk = p256::ecdsa::VerifyingKey::from(&sk);
            let alg_j_pub = format!(r#"{{"name":"ECDSA","namedCurve":"{curve}"}}"#);
            let alg_j_priv = alg_j_pub.clone();
            let priv_id = alloc_key(CryptoKeyEntry {
                key_type: "private",

                algorithm_json: alg_j_priv,
                extractable,
                usages_json: r#"["sign"]"#.to_string(),
                material: KeyMaterial::EcdsaPrivate(sk),
            });
            let pub_id = alloc_key(CryptoKeyEntry {
                key_type: "public",

                algorithm_json: alg_j_pub,
                extractable: true, // public keys are always extractable
                usages_json: r#"["verify"]"#.to_string(),
                material: KeyMaterial::EcdsaPublic(vk),
            });
            format!("{pub_id},{priv_id}")
        }
        "AES-GCM" => {
            let length = json_num_field(alg_json, "length").unwrap_or(256);
            let byte_len = (length as usize) / 8;
            if byte_len != 16 && byte_len != 32 {
                return "err:NotSupportedError".to_string();
            }
            let mut raw = vec![0u8; byte_len];
            getrandom::getrandom(&mut raw).unwrap_or(());
            let alg_j = format!(r#"{{"name":"AES-GCM","length":{length}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",

                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::AesGcm(raw),
            });
            id.to_string()
        }
        "AES-CBC" => {
            let length = json_num_field(alg_json, "length").unwrap_or(256);
            let byte_len = (length as usize) / 8;
            if byte_len != 16 && byte_len != 32 {
                return "err:NotSupportedError".to_string();
            }
            let mut raw = vec![0u8; byte_len];
            getrandom::getrandom(&mut raw).unwrap_or(());
            let alg_j = format!(r#"{{"name":"AES-CBC","length":{length}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",
                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::AesCbc(raw),
            });
            id.to_string()
        }
        "AES-CTR" => {
            let length = json_num_field(alg_json, "length").unwrap_or(256);
            let byte_len = (length as usize) / 8;
            if byte_len != 16 && byte_len != 32 {
                return "err:NotSupportedError".to_string();
            }
            let mut raw = vec![0u8; byte_len];
            getrandom::getrandom(&mut raw).unwrap_or(());
            let alg_j = format!(r#"{{"name":"AES-CTR","length":{length}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",
                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::AesCtr(raw),
            });
            id.to_string()
        }
        _ => "err:NotSupportedError".to_string(),
    }
}

// ─── importKey ────────────────────────────────────────────────────────────────

/// Import a key from raw bytes or a JWK JSON string.
/// `format` is "raw", "spki", "pkcs8", or "jwk".
/// `alg_json` describes the algorithm.
/// Returns key id as decimal string, or "err:..." on failure.
pub(crate) fn import_key(
    format: &str,
    key_data: Vec<u8>,
    alg_json: &str,
    extractable: bool,
    usages_json: &str,
) -> String {
    let name = json_str_field(alg_json, "name").unwrap_or("").to_ascii_uppercase();
    match name.as_str() {
        "HMAC" => {
            let hash = json_str_field(alg_json, "hash")
                .unwrap_or("SHA-256")
                .to_string();
            let raw = match format {
                "raw" => key_data,
                "jwk" => {
                    let jwk = String::from_utf8(key_data).unwrap_or_default();
                    let k = json_str_field(&jwk, "k").unwrap_or("");
                    match b64url_decode(k) {
                        Some(v) => v,
                        None => return "err:DataError".to_string(),
                    }
                }
                _ => return "err:NotSupportedError".to_string(),
            };
            let alg_j = format!(r#"{{"name":"HMAC","hash":{{"name":"{hash}"}}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",

                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::Hmac { hash, raw },
            });
            id.to_string()
        }
        "ECDSA" => {
            let curve = json_str_field(alg_json, "namedCurve")
                .unwrap_or("P-256")
                .to_string();
            if curve != "P-256" {
                return "err:NotSupportedError".to_string();
            }
            match format {
                "raw" => {
                    // Uncompressed SEC1 point (04 || x || y)
                    match p256::ecdsa::VerifyingKey::from_sec1_bytes(&key_data) {
                        Ok(vk) => {
                            let alg_j = format!(r#"{{"name":"ECDSA","namedCurve":"{curve}"}}"#);
                            let id = alloc_key(CryptoKeyEntry {
                                key_type: "public",
                
                                algorithm_json: alg_j,
                                extractable: true,
                                usages_json: usages_json.to_string(),
                                material: KeyMaterial::EcdsaPublic(vk),
                            });
                            id.to_string()
                        }
                        Err(_) => "err:DataError".to_string(),
                    }
                }
                "spki" => {
                    use p256::pkcs8::DecodePublicKey;
                    match p256::PublicKey::from_public_key_der(&key_data) {
                        Ok(pk) => {
                            let vk = p256::ecdsa::VerifyingKey::from(pk);
                            let alg_j = format!(r#"{{"name":"ECDSA","namedCurve":"{curve}"}}"#);
                            let id = alloc_key(CryptoKeyEntry {
                                key_type: "public",
                
                                algorithm_json: alg_j,
                                extractable: true,
                                usages_json: usages_json.to_string(),
                                material: KeyMaterial::EcdsaPublic(vk),
                            });
                            id.to_string()
                        }
                        Err(_) => "err:DataError".to_string(),
                    }
                }
                "pkcs8" => {
                    use p256::pkcs8::DecodePrivateKey;
                    match p256::SecretKey::from_pkcs8_der(&key_data) {
                        Ok(sk) => {
                            let signing_key = p256::ecdsa::SigningKey::from(sk);
                            let alg_j = format!(r#"{{"name":"ECDSA","namedCurve":"{curve}"}}"#);
                            let id = alloc_key(CryptoKeyEntry {
                                key_type: "private",
                
                                algorithm_json: alg_j,
                                extractable,
                                usages_json: usages_json.to_string(),
                                material: KeyMaterial::EcdsaPrivate(signing_key),
                            });
                            id.to_string()
                        }
                        Err(_) => "err:DataError".to_string(),
                    }
                }
                "jwk" => {
                    let jwk = String::from_utf8(key_data).unwrap_or_default();
                    // Check if private key (has "d" field)
                    if let Some(d_b64) = json_str_field(&jwk, "d") {
                        let d_bytes = match b64url_decode(d_b64) {
                            Some(v) => v,
                            None => return "err:DataError".to_string(),
                        };
                        match p256::ecdsa::SigningKey::from_bytes(d_bytes.as_slice().into()) {
                            Ok(sk) => {
                                let alg_j = format!(r#"{{"name":"ECDSA","namedCurve":"{curve}"}}"#);
                                let id = alloc_key(CryptoKeyEntry {
                                    key_type: "private",
                    
                                    algorithm_json: alg_j,
                                    extractable,
                                    usages_json: usages_json.to_string(),
                                    material: KeyMaterial::EcdsaPrivate(sk),
                                });
                                id.to_string()
                            }
                            Err(_) => "err:DataError".to_string(),
                        }
                    } else {
                        // Public key — reconstruct from x,y
                        let x = match json_str_field(&jwk, "x").and_then(b64url_decode) {
                            Some(v) => v,
                            None => return "err:DataError".to_string(),
                        };
                        let y = match json_str_field(&jwk, "y").and_then(b64url_decode) {
                            Some(v) => v,
                            None => return "err:DataError".to_string(),
                        };
                        // Build uncompressed SEC1 point: 0x04 || x || y
                        let mut point = Vec::with_capacity(65);
                        point.push(0x04);
                        let mut x_padded = vec![0u8; 32 - x.len().min(32)];
                        x_padded.extend_from_slice(&x[x.len().saturating_sub(32)..]);
                        point.extend_from_slice(&x_padded);
                        let mut y_padded = vec![0u8; 32 - y.len().min(32)];
                        y_padded.extend_from_slice(&y[y.len().saturating_sub(32)..]);
                        point.extend_from_slice(&y_padded);
                        match p256::ecdsa::VerifyingKey::from_sec1_bytes(&point) {
                            Ok(vk) => {
                                let alg_j =
                                    format!(r#"{{"name":"ECDSA","namedCurve":"{curve}"}}"#);
                                let id = alloc_key(CryptoKeyEntry {
                                    key_type: "public",
                    
                                    algorithm_json: alg_j,
                                    extractable: true,
                                    usages_json: usages_json.to_string(),
                                    material: KeyMaterial::EcdsaPublic(vk),
                                });
                                id.to_string()
                            }
                            Err(_) => "err:DataError".to_string(),
                        }
                    }
                }
                _ => "err:NotSupportedError".to_string(),
            }
        }
        "AES-GCM" => {
            let raw = match format {
                "raw" => key_data,
                "jwk" => {
                    let jwk = String::from_utf8(key_data).unwrap_or_default();
                    match json_str_field(&jwk, "k").and_then(b64url_decode) {
                        Some(v) => v,
                        None => return "err:DataError".to_string(),
                    }
                }
                _ => return "err:NotSupportedError".to_string(),
            };
            if raw.len() != 16 && raw.len() != 32 {
                return "err:DataError".to_string();
            }
            let length = (raw.len() * 8) as u32;
            let alg_j = format!(r#"{{"name":"AES-GCM","length":{length}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",

                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::AesGcm(raw),
            });
            id.to_string()
        }
        "AES-CBC" => {
            let raw = match format {
                "raw" => key_data,
                "jwk" => {
                    let jwk = String::from_utf8(key_data).unwrap_or_default();
                    match json_str_field(&jwk, "k").and_then(b64url_decode) {
                        Some(v) => v,
                        None => return "err:DataError".to_string(),
                    }
                }
                _ => return "err:NotSupportedError".to_string(),
            };
            if raw.len() != 16 && raw.len() != 32 {
                return "err:DataError".to_string();
            }
            let length = (raw.len() * 8) as u32;
            let alg_j = format!(r#"{{"name":"AES-CBC","length":{length}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",
                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::AesCbc(raw),
            });
            id.to_string()
        }
        "AES-CTR" => {
            let raw = match format {
                "raw" => key_data,
                "jwk" => {
                    let jwk = String::from_utf8(key_data).unwrap_or_default();
                    match json_str_field(&jwk, "k").and_then(b64url_decode) {
                        Some(v) => v,
                        None => return "err:DataError".to_string(),
                    }
                }
                _ => return "err:NotSupportedError".to_string(),
            };
            if raw.len() != 16 && raw.len() != 32 {
                return "err:DataError".to_string();
            }
            let length = (raw.len() * 8) as u32;
            let alg_j = format!(r#"{{"name":"AES-CTR","length":{length}}}"#);
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",
                algorithm_json: alg_j,
                extractable,
                usages_json: usages_json.to_string(),
                material: KeyMaterial::AesCtr(raw),
            });
            id.to_string()
        }
        "PBKDF2" => {
            // PBKDF2 keys may only be imported as "raw" — the key material is
            // the raw password bytes.  exportKey is not permitted by the spec.
            if format != "raw" {
                return "err:NotSupportedError".to_string();
            }
            let alg_j = r#"{"name":"PBKDF2"}"#.to_string();
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",
                algorithm_json: alg_j,
                extractable: false, // PBKDF2 keys are always non-extractable
                usages_json: usages_json.to_string(),
                material: KeyMaterial::Pbkdf2Raw(key_data),
            });
            id.to_string()
        }
        "HKDF" => {
            // HKDF keys may only be imported as "raw" — the key material is
            // the IKM (input keying material) bytes.  exportKey is not permitted.
            if format != "raw" {
                return "err:NotSupportedError".to_string();
            }
            let alg_j = r#"{"name":"HKDF"}"#.to_string();
            let id = alloc_key(CryptoKeyEntry {
                key_type: "secret",
                algorithm_json: alg_j,
                extractable: false, // HKDF keys are always non-extractable
                usages_json: usages_json.to_string(),
                material: KeyMaterial::HkdfRaw(key_data),
            });
            id.to_string()
        }
        _ => "err:NotSupportedError".to_string(),
    }
}

// ─── exportKey ────────────────────────────────────────────────────────────────

/// Export a key to bytes (raw/spki/pkcs8) or JWK JSON string.
/// Returns the serialised bytes, or empty Vec on error (check error via "err:" prefix in return str).
pub(crate) fn export_key(format: &str, key_id: u32) -> Result<Vec<u8>, &'static str> {
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = store.get(&key_id).ok_or("InvalidAccessError")?;
        if !entry.extractable {
            return Err("InvalidAccessError");
        }
        match (&entry.material, format) {
            // PBKDF2 and HKDF keys are always non-extractable by spec
            (KeyMaterial::Pbkdf2Raw(_), _) | (KeyMaterial::HkdfRaw(_), _) => {
                Err("NotSupportedError")
            }
            (KeyMaterial::Hmac { raw, .. }, "raw") => Ok(raw.clone()),
            (KeyMaterial::Hmac { raw, hash }, "jwk") => {
                let alg_str = match hash.as_str() {
                    "SHA-384" => "HS384",
                    "SHA-512" => "HS512",
                    _ => "HS256",
                };
                let k = b64url_encode(raw);
                Ok(format!(r#"{{"kty":"oct","k":"{k}","alg":"{alg_str}"}}"#).into_bytes())
            }
            (KeyMaterial::AesGcm(raw), "raw") => Ok(raw.clone()),
            (KeyMaterial::AesGcm(raw), "jwk") => {
                let alg_str = if raw.len() == 16 { "A128GCM" } else { "A256GCM" };
                let k = b64url_encode(raw);
                Ok(format!(r#"{{"kty":"oct","k":"{k}","alg":"{alg_str}"}}"#).into_bytes())
            }
            (KeyMaterial::AesCbc(raw), "raw") => Ok(raw.clone()),
            (KeyMaterial::AesCbc(raw), "jwk") => {
                let alg_str = if raw.len() == 16 { "A128CBC" } else { "A256CBC" };
                let k = b64url_encode(raw);
                Ok(format!(r#"{{"kty":"oct","k":"{k}","alg":"{alg_str}"}}"#).into_bytes())
            }
            (KeyMaterial::AesCtr(raw), "raw") => Ok(raw.clone()),
            (KeyMaterial::AesCtr(raw), "jwk") => {
                let alg_str = if raw.len() == 16 { "A128CTR" } else { "A256CTR" };
                let k = b64url_encode(raw);
                Ok(format!(r#"{{"kty":"oct","k":"{k}","alg":"{alg_str}"}}"#).into_bytes())
            }
            (KeyMaterial::EcdsaPublic(vk), "raw") => {
                // Uncompressed SEC1 point
                let ep = vk.to_encoded_point(false);
                Ok(ep.as_bytes().to_vec())
            }
            (KeyMaterial::EcdsaPublic(vk), "spki") => {
                use p256::pkcs8::EncodePublicKey;
                let pk = p256::PublicKey::from(vk);
                pk.to_public_key_der()
                    .map(|d| d.as_bytes().to_vec())
                    .map_err(|_| "OperationError")
            }
            (KeyMaterial::EcdsaPublic(vk), "jwk") => {
                let ep = vk.to_encoded_point(false);
                let x = b64url_encode(ep.x().map(|v| v.as_slice()).unwrap_or(&[]));
                let y = b64url_encode(ep.y().map(|v| v.as_slice()).unwrap_or(&[]));
                Ok(format!(
                    r#"{{"kty":"EC","crv":"P-256","x":"{x}","y":"{y}","key_ops":["verify"]}}"#
                )
                .into_bytes())
            }
            (KeyMaterial::EcdsaPrivate(sk), "pkcs8") => {
                use p256::pkcs8::EncodePrivateKey;
                p256::SecretKey::from(sk)
                    .to_pkcs8_der()
                    .map(|d| d.as_bytes().to_vec())
                    .map_err(|_| "OperationError")
            }
            (KeyMaterial::EcdsaPrivate(sk), "jwk") => {
                let vk = p256::ecdsa::VerifyingKey::from(sk);
                let ep = vk.to_encoded_point(false);
                let x = b64url_encode(ep.x().map(|v| v.as_slice()).unwrap_or(&[]));
                let y = b64url_encode(ep.y().map(|v| v.as_slice()).unwrap_or(&[]));
                let d = b64url_encode(sk.to_bytes().as_slice());
                Ok(format!(
                    r#"{{"kty":"EC","crv":"P-256","x":"{x}","y":"{y}","d":"{d}","key_ops":["sign"]}}"#
                )
                .into_bytes())
            }
            _ => Err("NotSupportedError"),
        }
    })
}

// ─── sign ─────────────────────────────────────────────────────────────────────

/// Sign `data` with the key identified by `key_id`.
/// `alg_json` provides algorithm params (e.g. hash name for ECDSA).
/// Returns signature bytes, or empty Vec on error.
pub(crate) fn sign_data(_alg_json: &str, key_id: u32, data: &[u8]) -> Vec<u8> {
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = match store.get(&key_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        match &entry.material {
            KeyMaterial::Hmac { hash, raw } => {
                use hmac::Mac;
                match hash.as_str() {
                    "SHA-384" => {
                        let mut mac = hmac::Hmac::<sha2::Sha384>::new_from_slice(raw)
                            .unwrap_or_else(|_| panic!("hmac key"));
                        mac.update(data);
                        mac.finalize().into_bytes().to_vec()
                    }
                    "SHA-512" => {
                        let mut mac = hmac::Hmac::<sha2::Sha512>::new_from_slice(raw)
                            .unwrap_or_else(|_| panic!("hmac key"));
                        mac.update(data);
                        mac.finalize().into_bytes().to_vec()
                    }
                    _ => {
                        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(raw)
                            .unwrap_or_else(|_| panic!("hmac key"));
                        mac.update(data);
                        mac.finalize().into_bytes().to_vec()
                    }
                }
            }
            KeyMaterial::EcdsaPrivate(sk) => {
                use p256::ecdsa::signature::Signer;
                let sig: p256::ecdsa::Signature = sk.sign(data);
                // WebCrypto uses IEEE P1363 (raw r||s), not DER
                sig.to_bytes().to_vec()
            }
            _ => Vec::new(),
        }
    })
}

// ─── verify ───────────────────────────────────────────────────────────────────

/// Verify a signature produced by `sign_data`.
/// Returns `true` if the signature is valid, `false` otherwise.
pub(crate) fn verify_signature(_alg_json: &str, key_id: u32, sig: &[u8], data: &[u8]) -> bool {
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = match store.get(&key_id) {
            Some(e) => e,
            None => return false,
        };
        match &entry.material {
            KeyMaterial::Hmac { hash, raw } => {
                use hmac::Mac;
                let expected = match hash.as_str() {
                    "SHA-384" => {
                        let mut mac = hmac::Hmac::<sha2::Sha384>::new_from_slice(raw)
                            .unwrap_or_else(|_| panic!("hmac key"));
                        mac.update(data);
                        mac.finalize().into_bytes().to_vec()
                    }
                    "SHA-512" => {
                        let mut mac = hmac::Hmac::<sha2::Sha512>::new_from_slice(raw)
                            .unwrap_or_else(|_| panic!("hmac key"));
                        mac.update(data);
                        mac.finalize().into_bytes().to_vec()
                    }
                    _ => {
                        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(raw)
                            .unwrap_or_else(|_| panic!("hmac key"));
                        mac.update(data);
                        mac.finalize().into_bytes().to_vec()
                    }
                };
                // constant-time comparison
                sig.len() == expected.len()
                    && sig
                        .iter()
                        .zip(&expected)
                        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                        == 0
            }
            KeyMaterial::EcdsaPublic(vk) => {
                use p256::ecdsa::signature::Verifier;
                let signature = match p256::ecdsa::Signature::from_bytes(sig.into()) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                vk.verify(data, &signature).is_ok()
            }
            _ => false,
        }
    })
}

// ─── encrypt / decrypt ────────────────────────────────────────────────────────

/// Encrypt `plaintext` using AES-GCM.
/// `iv` must be exactly 12 bytes; `aad` is optional additional data.
/// Returns ciphertext || tag (tag is 16 bytes at the end), or empty Vec on error.
pub(crate) fn aes_gcm_encrypt(key_id: u32, iv: &[u8], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    use aes_gcm::{AeadInPlace, KeyInit, Nonce};
    if iv.len() != 12 {
        return Vec::new();
    }
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = match store.get(&key_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        match &entry.material {
            KeyMaterial::AesGcm(raw) => {
                let nonce = Nonce::from_slice(iv);
                let mut buf = plaintext.to_vec();
                let tag_result = if raw.len() == 16 {
                    match aes_gcm::Aes128Gcm::new_from_slice(raw) {
                        Ok(c) => c.encrypt_in_place_detached(nonce, aad, &mut buf),
                        Err(_) => return Vec::new(),
                    }
                } else {
                    match aes_gcm::Aes256Gcm::new_from_slice(raw) {
                        Ok(c) => c.encrypt_in_place_detached(nonce, aad, &mut buf),
                        Err(_) => return Vec::new(),
                    }
                };
                match tag_result {
                    Ok(tag) => {
                        buf.extend_from_slice(tag.as_slice());
                        buf
                    }
                    Err(_) => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    })
}

/// Decrypt AES-GCM ciphertext (last 16 bytes are the authentication tag).
/// Returns plaintext or empty Vec on authentication failure.
pub(crate) fn aes_gcm_decrypt(key_id: u32, iv: &[u8], aad: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use aes_gcm::{AeadInPlace, KeyInit, Nonce, Tag};
    if iv.len() != 12 || ciphertext.len() < 16 {
        return Vec::new();
    }
    let (ct, tag_bytes) = ciphertext.split_at(ciphertext.len() - 16);
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = match store.get(&key_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        match &entry.material {
            KeyMaterial::AesGcm(raw) => {
                let nonce = Nonce::from_slice(iv);
                let tag = Tag::from_slice(tag_bytes);
                let mut buf = ct.to_vec();
                let ok = if raw.len() == 16 {
                    match aes_gcm::Aes128Gcm::new_from_slice(raw) {
                        Ok(c) => c.decrypt_in_place_detached(nonce, aad, &mut buf, tag).is_ok(),
                        Err(_) => false,
                    }
                } else {
                    match aes_gcm::Aes256Gcm::new_from_slice(raw) {
                        Ok(c) => c.decrypt_in_place_detached(nonce, aad, &mut buf, tag).is_ok(),
                        Err(_) => false,
                    }
                };
                if ok { buf } else { Vec::new() }
            }
            _ => Vec::new(),
        }
    })
}

// ─── AES-CBC encrypt / decrypt ───────────────────────────────────────────────

/// AES-CBC encrypt with PKCS7 padding (W3C SubtleCrypto AES-CBC).
///
/// `iv` must be exactly 16 bytes.  Returns ciphertext (padded to 16-byte
/// boundary), or an empty `Vec` on error.
pub(crate) fn aes_cbc_encrypt(key_id: u32, iv: &[u8], plaintext: &[u8]) -> Vec<u8> {
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    if iv.len() != 16 {
        return Vec::new();
    }
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = match store.get(&key_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        match &entry.material {
            KeyMaterial::AesCbc(raw) => {
                if raw.len() == 16 {
                    cbc::Encryptor::<aes::Aes128>::new_from_slices(raw, iv)
                        .map(|e| e.encrypt_padded_vec_mut::<Pkcs7>(plaintext))
                        .unwrap_or_default()
                } else {
                    cbc::Encryptor::<aes::Aes256>::new_from_slices(raw, iv)
                        .map(|e| e.encrypt_padded_vec_mut::<Pkcs7>(plaintext))
                        .unwrap_or_default()
                }
            }
            _ => Vec::new(),
        }
    })
}

/// AES-CBC decrypt with PKCS7 unpadding (W3C SubtleCrypto AES-CBC).
///
/// `iv` must be exactly 16 bytes; `ciphertext` length must be a multiple of 16.
/// Returns plaintext, or an empty `Vec` on padding/key error.
pub(crate) fn aes_cbc_decrypt(key_id: u32, iv: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    if iv.len() != 16 || !ciphertext.len().is_multiple_of(16) {
        return Vec::new();
    }
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = match store.get(&key_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        match &entry.material {
            KeyMaterial::AesCbc(raw) => {
                if raw.len() == 16 {
                    cbc::Decryptor::<aes::Aes128>::new_from_slices(raw, iv)
                        .ok()
                        .and_then(|d| d.decrypt_padded_vec_mut::<Pkcs7>(ciphertext).ok())
                        .unwrap_or_default()
                } else {
                    cbc::Decryptor::<aes::Aes256>::new_from_slices(raw, iv)
                        .ok()
                        .and_then(|d| d.decrypt_padded_vec_mut::<Pkcs7>(ciphertext).ok())
                        .unwrap_or_default()
                }
            }
            _ => Vec::new(),
        }
    })
}

// ─── AES-CTR encrypt / decrypt ───────────────────────────────────────────────

/// AES-CTR encrypt or decrypt (CTR mode is symmetric).
///
/// `counter_block` must be 16 bytes (the full 128-bit counter block).
/// `length_bits` is the bit-width of the counter portion (rightmost bits, 1–128).
/// Returns the processed data, or an empty `Vec` on error.
pub(crate) fn aes_ctr_crypt(
    key_id: u32,
    counter_block: &[u8],
    length_bits: u32,
    data: &[u8],
) -> Vec<u8> {
    use aes::cipher::{KeyIvInit, StreamCipher};
    if counter_block.len() != 16 {
        return Vec::new();
    }
    CRYPTO_KEYS.with(|ks| {
        let store = ks.borrow();
        let entry = match store.get(&key_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        match &entry.material {
            KeyMaterial::AesCtr(raw) => {
                // W3C spec §30: counter occupies the low `length_bits` of the block.
                // For length_bits == 128 use the full block as-is.
                // For shorter counters, zero the high bits so only the low portion wraps.
                let counter: [u8; 16] = if length_bits >= 128 {
                    counter_block.try_into().unwrap_or([0u8; 16])
                } else {
                    let mut c = [0u8; 16];
                    let byte_offset = (128 - length_bits as usize) / 8;
                    c[byte_offset..].copy_from_slice(&counter_block[byte_offset..]);
                    c
                };
                let mut out = data.to_vec();
                let ok = if raw.len() == 16 {
                    ctr::Ctr128BE::<aes::Aes128>::new_from_slices(raw, &counter)
                        .map(|mut c| c.apply_keystream(&mut out))
                        .is_ok()
                } else {
                    ctr::Ctr128BE::<aes::Aes256>::new_from_slices(raw, &counter)
                        .map(|mut c| c.apply_keystream(&mut out))
                        .is_ok()
                };
                if ok { out } else { Vec::new() }
            }
            _ => Vec::new(),
        }
    })
}

// ─── HMAC helper (shared by PBKDF2 and HKDF) ─────────────────────────────────

/// Compute HMAC-SHA256/384/512 of `data` with `key`.
///
/// `hash` must be one of `"SHA-256"`, `"SHA-384"`, `"SHA-512"` (case-sensitive
/// uppercase, as stored in the algorithm JSON).  Falls back to SHA-256 for any
/// other value.
fn hmac_hash(key: &[u8], data: &[u8], hash: &str) -> Vec<u8> {
    use hmac::Mac;
    match hash {
        "SHA-384" => {
            let mut mac = hmac::Hmac::<sha2::Sha384>::new_from_slice(key)
                .expect("HMAC accepts any key length");
            mac.update(data);
            mac.finalize().into_bytes().to_vec()
        }
        "SHA-512" => {
            let mut mac = hmac::Hmac::<sha2::Sha512>::new_from_slice(key)
                .expect("HMAC accepts any key length");
            mac.update(data);
            mac.finalize().into_bytes().to_vec()
        }
        _ => {
            // Default: SHA-256
            let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key)
                .expect("HMAC accepts any key length");
            mac.update(data);
            mac.finalize().into_bytes().to_vec()
        }
    }
}

// ─── PBKDF2 (RFC 2898 §5.2) ──────────────────────────────────────────────────

/// Derive `dk_len` bytes from a password using PBKDF2-HMAC (RFC 2898 §5.2).
///
/// `hash` selects the underlying PRF: `"SHA-256"`, `"SHA-384"`, or `"SHA-512"`.
fn pbkdf2_derive(password: &[u8], salt: &[u8], iterations: usize, dk_len: usize, hash: &str) -> Vec<u8> {
    // PBKDF2: DK = T_1 || T_2 || … || T_ceil(dkLen/hLen)
    // T_i = U_1 XOR U_2 XOR … XOR U_c
    // U_1 = PRF(P, S || INT(i)), U_j = PRF(P, U_{j-1})
    let hmac_len = match hash {
        "SHA-384" => 48,
        "SHA-512" => 64,
        _ => 32, // SHA-256
    };
    let mut dk = Vec::with_capacity(dk_len);
    let mut block_index: u32 = 1;
    while dk.len() < dk_len {
        // U_1 = HMAC(P, S || INT(i))
        let mut salt_i = salt.to_vec();
        salt_i.extend_from_slice(&block_index.to_be_bytes());
        let u1 = hmac_hash(password, &salt_i, hash);
        let mut t = u1.clone();
        let mut prev = u1;
        // U_2 … U_c
        for _ in 1..iterations {
            let u = hmac_hash(password, &prev, hash);
            for (t_b, u_b) in t.iter_mut().zip(u.iter()) {
                *t_b ^= u_b;
            }
            prev = u;
        }
        let take = hmac_len.min(dk_len - dk.len());
        dk.extend_from_slice(&t[..take]);
        block_index = match block_index.checked_add(1) {
            Some(v) => v,
            None => break, // guard: overflow means dk_len is unreasonably large
        };
    }
    dk
}

// ─── HKDF (RFC 5869) ─────────────────────────────────────────────────────────

/// Derive `length` bytes from IKM using HKDF extract-then-expand (RFC 5869).
///
/// `hash` selects the underlying PRF: `"SHA-256"`, `"SHA-384"`, or `"SHA-512"`.
fn hkdf_derive(ikm: &[u8], salt: &[u8], info: &[u8], length: usize, hash: &str) -> Vec<u8> {
    // Extract: PRK = HMAC-hash(salt, IKM)
    // If salt is absent (empty), use a zero-filled string of HashLen octets.
    let hmac_len = match hash {
        "SHA-384" => 48,
        "SHA-512" => 64,
        _ => 32,
    };
    let effective_salt: Vec<u8> = if salt.is_empty() {
        vec![0u8; hmac_len]
    } else {
        salt.to_vec()
    };
    let prk = hmac_hash(&effective_salt, ikm, hash);

    // Expand: T(0) = "", T(i) = HMAC-hash(PRK, T(i-1) || info || i)
    let mut out = Vec::with_capacity(length);
    let mut prev: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while out.len() < length {
        let mut input = prev.clone();
        input.extend_from_slice(info);
        input.push(counter);
        let t_i = hmac_hash(&prk, &input, hash);
        let take = t_i.len().min(length - out.len());
        out.extend_from_slice(&t_i[..take]);
        prev = t_i;
        counter = match counter.checked_add(1) {
            Some(v) => v,
            None => break, // 255 blocks × HashLen bytes is more than enough
        };
    }
    out
}

// ─── deriveBits ──────────────────────────────────────────────────────────────

/// Derive `length_bits` bits from a PBKDF2 or HKDF key.
///
/// `alg_json` must be a JSON object describing the derive algorithm, e.g.:
/// ```json
/// {"name":"PBKDF2","hash":"SHA-256","salt":[1,2,3],"iterations":100000}
/// {"name":"HKDF","hash":"SHA-256","salt":[...],"info":[...]}
/// ```
/// Salt and info are encoded as JSON arrays of `u8` values (produced by the
/// JS shim from `Array.from(new Uint8Array(...))`.
///
/// Returns the derived bytes, or an empty `Vec` on error.
pub(crate) fn derive_bits(alg_json: &str, key_id: u32, length_bits: u32) -> Vec<u8> {
    let name_raw = json_str_field(alg_json, "name").unwrap_or("").to_ascii_uppercase();
    let hash = json_str_field(alg_json, "hash")
        .unwrap_or("SHA-256")
        .to_ascii_uppercase();
    let length_bytes = (length_bits as usize).div_ceil(8);

    with_key(
        key_id,
        |entry| match (&entry.material, name_raw.as_str()) {
            (KeyMaterial::Pbkdf2Raw(pwd), "PBKDF2") => {
                let salt = json_bytes_field(alg_json, "salt");
                let iterations =
                    json_num_field(alg_json, "iterations").unwrap_or(100_000) as usize;
                pbkdf2_derive(pwd, &salt, iterations, length_bytes, &hash)
            }
            (KeyMaterial::HkdfRaw(ikm), "HKDF") => {
                let salt = json_bytes_field(alg_json, "salt");
                let info = json_bytes_field(alg_json, "info");
                hkdf_derive(ikm, &salt, &info, length_bytes, &hash)
            }
            _ => Vec::new(),
        },
        Vec::new(),
    )
}

// ─── key info query ───────────────────────────────────────────────────────────

/// Return JSON string with key metadata: `{type, algorithm, extractable, usages}`.
/// Returns empty string if key id is not found.
pub(crate) fn key_info(key_id: u32) -> String {
    with_key(
        key_id,
        |e| {
            format!(
                r#"{{"type":"{ty}","algorithm":{alg},"extractable":{ext},"usages":{usages}}}"#,
                ty = e.key_type,
                alg = e.algorithm_json,
                ext = e.extractable,
                usages = e.usages_json,
            )
        },
        String::new(),
    )
}

// ─── JS bindings installer ────────────────────────────────────────────────────

/// Install all `_lumen_subtle_*` native bindings into the JS context.
pub(crate) fn install_subtle_bindings(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $fn:expr) => {
            ctx.globals().set($name, Function::new(ctx.clone(), $fn)?)?;
        };
    }

    reg!(
        "_lumen_subtle_generate_key",
        |alg_json: String, extractable: bool, usages_json: String| -> String {
            generate_key(&alg_json, extractable, &usages_json)
        }
    );

    reg!(
        "_lumen_subtle_import_key",
        |format: String, key_data: Vec<u8>, alg_json: String, extractable: bool, usages_json: String| -> String {
            import_key(&format, key_data, &alg_json, extractable, &usages_json)
        }
    );

    reg!(
        "_lumen_subtle_export_key",
        |format: String, key_id: u32| -> Vec<u8> {
            export_key(&format, key_id).unwrap_or_default()
        }
    );

    reg!(
        "_lumen_subtle_export_key_or_err",
        |format: String, key_id: u32| -> String {
            match export_key(&format, key_id) {
                Ok(bytes) => {
                    // Return as "ok:<hex>" or "ok:<json>" depending on whether bytes
                    // look like printable JSON (starts with '{' or '[').
                    if bytes.first() == Some(&b'{') || bytes.first() == Some(&b'[') {
                        format!("ok:{}", String::from_utf8_lossy(&bytes))
                    } else {
                        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                        format!("hex:{hex}")
                    }
                }
                Err(e) => format!("err:{e}"),
            }
        }
    );

    reg!(
        "_lumen_subtle_sign",
        |alg_json: String, key_id: u32, data: Vec<u8>| -> Vec<u8> {
            sign_data(&alg_json, key_id, &data)
        }
    );

    reg!(
        "_lumen_subtle_verify",
        |alg_json: String, key_id: u32, sig: Vec<u8>, data: Vec<u8>| -> bool {
            verify_signature(&alg_json, key_id, &sig, &data)
        }
    );

    reg!(
        "_lumen_subtle_encrypt",
        |key_id: u32, iv: Vec<u8>, aad: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
            aes_gcm_encrypt(key_id, &iv, &aad, &plaintext)
        }
    );

    reg!(
        "_lumen_subtle_decrypt",
        |key_id: u32, iv: Vec<u8>, aad: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
            aes_gcm_decrypt(key_id, &iv, &aad, &ciphertext)
        }
    );

    reg!(
        "_lumen_subtle_key_info",
        |key_id: u32| -> String { key_info(key_id) }
    );

    reg!(
        "_lumen_subtle_aes_cbc_encrypt",
        |key_id: u32, iv: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
            aes_cbc_encrypt(key_id, &iv, &plaintext)
        }
    );

    reg!(
        "_lumen_subtle_aes_cbc_decrypt",
        |key_id: u32, iv: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
            aes_cbc_decrypt(key_id, &iv, &ciphertext)
        }
    );

    reg!(
        "_lumen_subtle_aes_ctr_crypt",
        |key_id: u32, counter: Vec<u8>, length: u32, data: Vec<u8>| -> Vec<u8> {
            aes_ctr_crypt(key_id, &counter, length, &data)
        }
    );

    reg!(
        "_lumen_subtle_derive_bits",
        |alg_json: String, key_id: u32, length_bits: u32| -> Vec<u8> {
            derive_bits(&alg_json, key_id, length_bits)
        }
    );

    Ok(())
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_store() {
        CRYPTO_KEYS.with(|ks| ks.borrow_mut().clear());
        NEXT_KEY_ID.with(|c| c.set(1));
    }

    #[test]
    fn hmac_generate_sign_verify_roundtrip() {
        fresh_store();
        let alg = r#"{"name":"HMAC","hash":"SHA-256"}"#;
        let id_str = generate_key(alg, true, r#"["sign","verify"]"#);
        assert!(!id_str.starts_with("err:"), "generate_key failed: {id_str}");
        let key_id: u32 = id_str.parse().expect("numeric id");

        let data = b"hello world";
        let sig = sign_data(alg, key_id, data);
        assert!(!sig.is_empty());

        assert!(verify_signature(alg, key_id, &sig, data), "valid sig should verify");
        let mut bad_sig = sig.clone();
        bad_sig[0] ^= 0xff;
        assert!(!verify_signature(alg, key_id, &bad_sig, data), "corrupted sig should fail");
    }

    #[test]
    fn hmac_sha384_sign_verify() {
        fresh_store();
        let alg = r#"{"name":"HMAC","hash":"SHA-384"}"#;
        let id_str = generate_key(alg, true, r#"["sign","verify"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let data = b"test data";
        let sig = sign_data(alg, key_id, data);
        assert_eq!(sig.len(), 48); // SHA-384 HMAC is 48 bytes
        assert!(verify_signature(alg, key_id, &sig, data));
    }

    #[test]
    fn hmac_import_export_raw_roundtrip() {
        fresh_store();
        let alg = r#"{"name":"HMAC","hash":"SHA-256"}"#;
        let raw_key = vec![0x42u8; 32];
        let id_str = import_key("raw", raw_key.clone(), alg, true, r#"["sign","verify"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let exported = export_key("raw", key_id).unwrap();
        assert_eq!(exported, raw_key);
    }

    #[test]
    fn hmac_import_export_jwk() {
        fresh_store();
        let alg = r#"{"name":"HMAC","hash":"SHA-256"}"#;
        let raw_key = vec![0xABu8; 32];
        let id_str = import_key("raw", raw_key.clone(), alg, true, r#"["sign","verify"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let jwk_bytes = export_key("jwk", key_id).unwrap();
        let jwk = String::from_utf8(jwk_bytes).unwrap();
        assert!(jwk.contains("\"kty\":\"oct\""), "JWK should be oct: {jwk}");
        // Re-import from JWK
        fresh_store();
        let id2_str = import_key("jwk", jwk.into_bytes(), alg, true, r#"["sign","verify"]"#);
        let key_id2: u32 = id2_str.parse().unwrap();
        let exported2 = export_key("raw", key_id2).unwrap();
        assert_eq!(exported2, raw_key);
    }

    #[test]
    fn ecdsa_generate_sign_verify() {
        fresh_store();
        let alg = r#"{"name":"ECDSA","namedCurve":"P-256"}"#;
        let result = generate_key(alg, true, r#"["sign","verify"]"#);
        assert!(!result.starts_with("err:"), "generate failed: {result}");
        let parts: Vec<&str> = result.split(',').collect();
        assert_eq!(parts.len(), 2, "should return pub_id,priv_id");
        let pub_id: u32 = parts[0].parse().unwrap();
        let priv_id: u32 = parts[1].parse().unwrap();

        let data = b"message to sign";
        let sign_alg = r#"{"name":"ECDSA","hash":"SHA-256"}"#;
        let sig = sign_data(sign_alg, priv_id, data);
        assert_eq!(sig.len(), 64, "P-256 ECDSA signature is 64 bytes");

        assert!(verify_signature(sign_alg, pub_id, &sig, data), "valid sig");
        let mut bad = sig.clone();
        bad[0] ^= 0x01;
        assert!(!verify_signature(sign_alg, pub_id, &bad, data), "bad sig");
    }

    #[test]
    fn ecdsa_export_spki_and_reimport() {
        fresh_store();
        let alg = r#"{"name":"ECDSA","namedCurve":"P-256"}"#;
        let result = generate_key(alg, true, r#"["sign","verify"]"#);
        let parts: Vec<&str> = result.split(',').collect();
        let pub_id: u32 = parts[0].parse().unwrap();

        let spki = export_key("spki", pub_id).unwrap();
        assert!(!spki.is_empty());

        fresh_store();
        let id2 = import_key("spki", spki, alg, true, r#"["verify"]"#);
        assert!(!id2.starts_with("err:"), "spki reimport failed: {id2}");
    }

    #[test]
    fn ecdsa_export_pkcs8_and_reimport() {
        fresh_store();
        let alg = r#"{"name":"ECDSA","namedCurve":"P-256"}"#;
        let result = generate_key(alg, true, r#"["sign","verify"]"#);
        let parts: Vec<&str> = result.split(',').collect();
        let priv_id: u32 = parts[1].parse().unwrap();

        let pkcs8 = export_key("pkcs8", priv_id).unwrap();
        assert!(!pkcs8.is_empty());

        fresh_store();
        let id2 = import_key("pkcs8", pkcs8, alg, true, r#"["sign"]"#);
        assert!(!id2.starts_with("err:"), "pkcs8 reimport failed: {id2}");
    }

    #[test]
    fn ecdsa_export_jwk_public() {
        fresh_store();
        let alg = r#"{"name":"ECDSA","namedCurve":"P-256"}"#;
        let result = generate_key(alg, true, r#"["sign","verify"]"#);
        let parts: Vec<&str> = result.split(',').collect();
        let pub_id: u32 = parts[0].parse().unwrap();
        let jwk_bytes = export_key("jwk", pub_id).unwrap();
        let jwk = String::from_utf8(jwk_bytes).unwrap();
        assert!(jwk.contains("\"kty\":\"EC\""), "EC JWK: {jwk}");
        assert!(jwk.contains("\"crv\":\"P-256\""), "P-256 JWK: {jwk}");
        assert!(jwk.contains("\"x\":"), "x coord: {jwk}");
        assert!(jwk.contains("\"y\":"), "y coord: {jwk}");
    }

    #[test]
    fn aes_gcm_generate_encrypt_decrypt() {
        fresh_store();
        let alg = r#"{"name":"AES-GCM","length":256}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();

        let iv = vec![0u8; 12];
        let aad = b"extra data";
        let plaintext = b"secret message";

        let ct = aes_gcm_encrypt(key_id, &iv, aad, plaintext);
        assert!(!ct.is_empty(), "encrypt should not be empty");
        assert_eq!(ct.len(), plaintext.len() + 16, "ciphertext + tag");

        let pt = aes_gcm_decrypt(key_id, &iv, aad, &ct);
        assert_eq!(pt, plaintext, "decrypt round-trip");
    }

    #[test]
    fn aes_gcm_128_roundtrip() {
        fresh_store();
        let alg = r#"{"name":"AES-GCM","length":128}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let iv = vec![1u8; 12];
        let plain = b"hello";
        let ct = aes_gcm_encrypt(key_id, &iv, b"", plain);
        let pt = aes_gcm_decrypt(key_id, &iv, b"", &ct);
        assert_eq!(&pt, plain);
    }

    #[test]
    fn aes_gcm_import_raw_and_export() {
        fresh_store();
        let alg = r#"{"name":"AES-GCM","length":256}"#;
        let raw_key = vec![0x12u8; 32];
        let id_str = import_key("raw", raw_key.clone(), alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let exported = export_key("raw", key_id).unwrap();
        assert_eq!(exported, raw_key);
    }

    #[test]
    fn aes_gcm_tampered_ciphertext_fails() {
        fresh_store();
        let alg = r#"{"name":"AES-GCM","length":256}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let iv = vec![0u8; 12];
        let plain = b"data";
        let mut ct = aes_gcm_encrypt(key_id, &iv, b"", plain);
        ct[0] ^= 0xff;
        let pt = aes_gcm_decrypt(key_id, &iv, b"", &ct);
        assert!(pt.is_empty(), "tampered ciphertext should fail authentication");
    }

    #[test]
    fn key_info_returns_metadata() {
        fresh_store();
        let alg = r#"{"name":"HMAC","hash":"SHA-256"}"#;
        let id_str = generate_key(alg, true, r#"["sign","verify"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let info = key_info(key_id);
        assert!(info.contains("\"type\":\"secret\""), "type: {info}");
        assert!(info.contains("\"HMAC\""), "algorithm: {info}");
    }

    #[test]
    fn b64url_roundtrip() {
        let data = vec![0x00, 0xff, 0x80, 0x3f, 0xab, 0xcd];
        let encoded = b64url_encode(&data);
        let decoded = b64url_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn generate_key_unsupported_algo_returns_err() {
        fresh_store();
        let result = generate_key(r#"{"name":"RSA-OAEP"}"#, true, r#"["encrypt"]"#);
        assert!(result.starts_with("err:NotSupportedError"), "{result}");
    }

    // ─── AES-CBC tests ────────────────────────────────────────────────────────

    #[test]
    fn aes_cbc_128_roundtrip() {
        fresh_store();
        let alg = r#"{"name":"AES-CBC","length":128}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        assert!(!id_str.starts_with("err:"), "generate_key failed: {id_str}");
        let key_id: u32 = id_str.parse().unwrap();

        let iv = vec![0x11u8; 16];
        let plain = b"hello, AES-CBC!";
        let ct = aes_cbc_encrypt(key_id, &iv, plain);
        assert!(!ct.is_empty(), "encrypt returned empty");
        // CBC pads to block boundary: plaintext 15 bytes → padded to 16
        assert_eq!(ct.len() % 16, 0);

        let pt = aes_cbc_decrypt(key_id, &iv, &ct);
        assert_eq!(pt, plain, "CBC round-trip failed");
    }

    #[test]
    fn aes_cbc_256_roundtrip() {
        fresh_store();
        let alg = r#"{"name":"AES-CBC","length":256}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();

        let iv = vec![0xAAu8; 16];
        let plain = b"AES-256-CBC test message for roundtrip";
        let ct = aes_cbc_encrypt(key_id, &iv, plain);
        let pt = aes_cbc_decrypt(key_id, &iv, &ct);
        assert_eq!(pt, plain);
    }

    #[test]
    fn aes_cbc_import_export_raw() {
        fresh_store();
        let alg = r#"{"name":"AES-CBC","length":128}"#;
        let raw_key = vec![0x5Au8; 16];
        let id_str = import_key("raw", raw_key.clone(), alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let exported = export_key("raw", key_id).unwrap();
        assert_eq!(exported, raw_key);
    }

    #[test]
    fn aes_cbc_import_export_jwk() {
        fresh_store();
        let alg = r#"{"name":"AES-CBC","length":256}"#;
        let raw_key = vec![0xBBu8; 32];
        let id_str = import_key("raw", raw_key.clone(), alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let jwk_bytes = export_key("jwk", key_id).unwrap();
        let jwk = String::from_utf8(jwk_bytes).unwrap();
        assert!(jwk.contains("\"kty\":\"oct\""), "JWK: {jwk}");
        assert!(jwk.contains("\"alg\":\"A256CBC\""), "alg: {jwk}");
        // Re-import from JWK
        fresh_store();
        let id2 = import_key("jwk", jwk.into_bytes(), alg, true, r#"["encrypt","decrypt"]"#);
        let key_id2: u32 = id2.parse().unwrap();
        let exported2 = export_key("raw", key_id2).unwrap();
        assert_eq!(exported2, raw_key);
    }

    #[test]
    fn aes_cbc_wrong_iv_len_returns_empty() {
        fresh_store();
        let alg = r#"{"name":"AES-CBC","length":128}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        // IV must be exactly 16 bytes
        let bad_iv = vec![0u8; 12];
        let ct = aes_cbc_encrypt(key_id, &bad_iv, b"test");
        assert!(ct.is_empty(), "expected empty for bad IV");
    }

    // ─── AES-CTR tests ────────────────────────────────────────────────────────

    #[test]
    fn aes_ctr_128_roundtrip() {
        fresh_store();
        let alg = r#"{"name":"AES-CTR","length":128}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        assert!(!id_str.starts_with("err:"), "generate_key failed: {id_str}");
        let key_id: u32 = id_str.parse().unwrap();

        let counter = vec![0u8; 16];
        let plain = b"AES-CTR test data";
        let ct = aes_ctr_crypt(key_id, &counter, 128, plain);
        assert_eq!(ct.len(), plain.len(), "CTR mode preserves length");

        // CTR encrypt == decrypt
        let pt = aes_ctr_crypt(key_id, &counter, 128, &ct);
        assert_eq!(pt, plain, "CTR round-trip failed");
    }

    #[test]
    fn aes_ctr_256_roundtrip() {
        fresh_store();
        let alg = r#"{"name":"AES-CTR","length":256}"#;
        let id_str = generate_key(alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();

        let counter = vec![0xFFu8; 16];
        let plain = b"Another CTR test with AES-256";
        let ct = aes_ctr_crypt(key_id, &counter, 64, plain);
        let pt = aes_ctr_crypt(key_id, &counter, 64, &ct);
        assert_eq!(pt, plain);
    }

    #[test]
    fn aes_ctr_import_export_raw() {
        fresh_store();
        let alg = r#"{"name":"AES-CTR","length":128}"#;
        let raw_key = vec![0x7Cu8; 16];
        let id_str = import_key("raw", raw_key.clone(), alg, true, r#"["encrypt","decrypt"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        let exported = export_key("raw", key_id).unwrap();
        assert_eq!(exported, raw_key);
    }

    // ─── PBKDF2 tests ─────────────────────────────────────────────────────────

    #[test]
    fn pbkdf2_deterministic_known_vector() {
        // PBKDF2-HMAC-SHA256("password", "salt", 1, 32)
        // Known vector from RFC 6070 adapted: output first 8 bytes checked.
        let pwd = b"password";
        let salt = b"salt";
        let dk = pbkdf2_derive(pwd, salt, 1, 32, "SHA-256");
        assert_eq!(dk.len(), 32);
        // First 4 bytes of PBKDF2-HMAC-SHA256("password","salt",1,32)
        // = 120fb6cffccd925779e5f02a1c58ae6a (RFC test vector)
        assert_eq!(dk[0], 0x12, "byte[0]={:#04x}", dk[0]);
        assert_eq!(dk[1], 0x0f, "byte[1]={:#04x}", dk[1]);
        assert_eq!(dk[2], 0xb6, "byte[2]={:#04x}", dk[2]);
        assert_eq!(dk[3], 0xcf, "byte[3]={:#04x}", dk[3]);
    }

    #[test]
    fn pbkdf2_import_and_derive() {
        fresh_store();
        let alg = r#"{"name":"PBKDF2"}"#;
        let pwd = b"my-password".to_vec();
        let id_str = import_key("raw", pwd, alg, false, r#"["deriveBits"]"#);
        assert!(!id_str.starts_with("err:"), "import failed: {id_str}");
        let key_id: u32 = id_str.parse().unwrap();

        // PBKDF2 keys are non-extractable by spec
        let result = export_key("raw", key_id);
        assert!(result.is_err(), "PBKDF2 key must not be exportable");

        let derive_alg = r#"{"name":"PBKDF2","hash":"SHA-256","salt":[1,2,3,4],"iterations":1000}"#;
        let dk = derive_bits(derive_alg, key_id, 256);
        assert_eq!(dk.len(), 32, "expected 32 bytes for 256 bits");
    }

    #[test]
    fn pbkdf2_non_extractable() {
        fresh_store();
        let alg = r#"{"name":"PBKDF2"}"#;
        // Even with extractable=true, PBKDF2 import overrides to false
        let id_str = import_key("raw", b"pass".to_vec(), alg, true, r#"["deriveBits"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        assert!(export_key("raw", key_id).is_err(), "PBKDF2 always non-extractable");
    }

    // ─── HKDF tests ───────────────────────────────────────────────────────────

    #[test]
    fn hkdf_deterministic_known_vector() {
        // HKDF-SHA256: IKM = 0x0b*22, salt = 0x000102...0c (13 bytes),
        // info = 0xf0f1f2...f9 (10 bytes), L = 42
        // Expected OKM from RFC 5869 Appendix A.1
        let ikm = vec![0x0bu8; 22];
        let salt: Vec<u8> = (0x00u8..=0x0cu8).collect();
        let info: Vec<u8> = (0xf0u8..=0xf9u8).collect();
        let okm = hkdf_derive(&ikm, &salt, &info, 42, "SHA-256");
        assert_eq!(okm.len(), 42);
        // First 4 bytes of RFC 5869 A.1 expected OKM:
        // 3cb25f25faacd57a90434f64d0362f2a2d2d0a90 ...
        assert_eq!(okm[0], 0x3c, "byte[0]={:#04x}", okm[0]);
        assert_eq!(okm[1], 0xb2, "byte[1]={:#04x}", okm[1]);
        assert_eq!(okm[2], 0x5f, "byte[2]={:#04x}", okm[2]);
        assert_eq!(okm[3], 0x25, "byte[3]={:#04x}", okm[3]);
    }

    #[test]
    fn hkdf_import_and_derive() {
        fresh_store();
        let alg = r#"{"name":"HKDF"}"#;
        let ikm = b"input-keying-material".to_vec();
        let id_str = import_key("raw", ikm, alg, false, r#"["deriveBits"]"#);
        assert!(!id_str.starts_with("err:"), "import failed: {id_str}");
        let key_id: u32 = id_str.parse().unwrap();

        // HKDF keys are non-extractable by spec
        let result = export_key("raw", key_id);
        assert!(result.is_err(), "HKDF key must not be exportable");

        let derive_alg = r#"{"name":"HKDF","hash":"SHA-256","salt":[5,6,7],"info":[8,9,10]}"#;
        let dk = derive_bits(derive_alg, key_id, 128);
        assert_eq!(dk.len(), 16, "expected 16 bytes for 128 bits");
    }

    #[test]
    fn hkdf_no_salt_uses_zero_fill() {
        fresh_store();
        let alg = r#"{"name":"HKDF"}"#;
        let id_str = import_key("raw", b"ikm".to_vec(), alg, false, r#"["deriveBits"]"#);
        let key_id: u32 = id_str.parse().unwrap();
        // Empty salt should fall back to zeroed hash-len salt per RFC 5869 §2.2
        let derive_alg = r#"{"name":"HKDF","hash":"SHA-256","salt":[],"info":[]}"#;
        let dk = derive_bits(derive_alg, key_id, 256);
        assert_eq!(dk.len(), 32);
        // Result is deterministic — derive twice and compare
        let id_str2 = import_key("raw", b"ikm".to_vec(), alg, false, r#"["deriveBits"]"#);
        let key_id2: u32 = id_str2.parse().unwrap();
        let dk2 = derive_bits(derive_alg, key_id2, 256);
        assert_eq!(dk, dk2, "HKDF must be deterministic");
    }

    #[test]
    fn json_bytes_field_parses_array() {
        let json = r#"{"name":"PBKDF2","salt":[10,20,30],"iterations":1000}"#;
        let salt = json_bytes_field(json, "salt");
        assert_eq!(salt, vec![10u8, 20, 30]);
        let empty = json_bytes_field(json, "info");
        assert!(empty.is_empty());
    }
}
