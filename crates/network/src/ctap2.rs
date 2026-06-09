//! CTAP2-over-HID roaming authenticator transport.
//!
//! Implements the FIDO CTAP2 client-to-authenticator protocol over USB HID
//! (FIDO Alliance CTAP specification §11 "USB Transport Binding").
//!
//! # Architecture
//!
//! - [`HidDevice`] — platform-agnostic 64-byte report I/O.
//! - [`CtapHidChannel`] — framing layer: CTAPHID_INIT handshake, packet
//!   fragmentation/reassembly, channel-ID management.
//! - [`Ctap2Client`] — sends CTAP2 CBOR commands and parses responses.
//! - [`CtapRoamingTransport`] — implements [`CredentialProvider`]; tries every
//!   connected FIDO2 key in turn, returns `NotAllowed` when none are present.
//! - [`CompositeCredentialProvider`] — priority-ordered list: first non-`NotAllowed`
//!   result wins. Use it to chain roaming → software fallback.
//!
//! # Phase 0
//!
//! Full protocol stack is implemented and tested via [`MockHidDevice`].
//! [`probe_usb_fido_devices`] returns an empty list — no real USB enumeration yet.
//!
//! Phase 1: add a platform backend behind [`HidDevice`] — Windows `HidD_*` +
//! `SetupDi`, Linux `hidraw`, macOS `IOHIDDevice`.

use lumen_core::ext::{
    CredentialProvider, WebAuthnCreateRequest, WebAuthnCreateResponse, WebAuthnError,
    WebAuthnGetRequest, WebAuthnGetResponse,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

/// Generate 8 random bytes from the OS CSPRNG for the CTAPHID_INIT nonce.
fn random_nonce() -> [u8; 8] {
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf).expect("OS CSPRNG unavailable");
    buf
}

// ── HID packet constants ────────────────────────────────────────────────────

/// Payload bytes in the first (initialization) HID packet.
const INIT_DATA: usize = 57; // 64 - 4(CID) - 1(cmd) - 2(bcnt)
/// Payload bytes in each continuation HID packet.
const CONT_DATA: usize = 59; // 64 - 4(CID) - 1(seq)

/// Broadcast channel ID used for the CTAPHID_INIT handshake.
const CID_BROADCAST: u32 = 0xFFFF_FFFF;

/// CTAPHID command codes (high bit set).
const CMD_INIT: u8 = 0x86;
const CMD_CBOR: u8 = 0x90;
const CMD_KEEPALIVE: u8 = 0xBB;
const CMD_ERROR: u8 = 0xBF;

/// CTAP2 command bytes (first byte of the CBOR payload).
const CTAP_MAKE_CREDENTIAL: u8 = 0x01;
const CTAP_GET_ASSERTION: u8 = 0x02;

/// CTAP2 status codes (first byte of a CTAPHID_CBOR response).
const CTAP2_OK: u8 = 0x00;

/// FIDO2 USB HID Usage: Usage Page 0xF1D0, Usage 0x01.
pub const FIDO_USAGE_PAGE: u16 = 0xF1D0;
pub const FIDO_USAGE: u16 = 0x01;

// ── Error type ───────────────────────────────────────────────────────────────

/// Error produced by the CTAP2 HID transport layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ctap2Error {
    /// No FIDO2 devices found / device disconnected.
    NoDevice,
    /// OS-level HID I/O error.
    Hid(String),
    /// CTAPHID framing error.
    Protocol(String),
    /// CBOR decode error.
    Cbor(String),
    /// Device returned a non-zero CTAP2 status code.
    DeviceStatus(u8),
    /// Response timeout (user did not touch the key).
    Timeout,
}

impl From<Ctap2Error> for WebAuthnError {
    fn from(e: Ctap2Error) -> Self {
        match e {
            Ctap2Error::NoDevice => WebAuthnError::NotAllowed,
            Ctap2Error::Timeout => WebAuthnError::NotAllowed,
            Ctap2Error::DeviceStatus(0x19) => WebAuthnError::InvalidState, // CREDENTIAL_EXCLUDED
            Ctap2Error::DeviceStatus(0x2E) => WebAuthnError::NotAllowed,   // NO_CREDENTIALS
            Ctap2Error::DeviceStatus(0x26) => WebAuthnError::Constraint,   // UNSUPPORTED_ALGORITHM
            _ => WebAuthnError::NotAllowed,
        }
    }
}

// ── HidDevice trait ──────────────────────────────────────────────────────────

/// Platform-agnostic USB HID device I/O.
///
/// Each report is exactly 65 bytes: byte 0 is the HID report ID (always 0x00
/// for FIDO2), bytes 1–64 are the CTAPHID payload.
pub trait HidDevice: Send + Sync {
    /// Write a 65-byte HID report (report-id byte first).
    fn write(&self, report: &[u8; 65]) -> Result<(), Ctap2Error>;

    /// Read a 65-byte HID report, blocking up to `timeout_ms` milliseconds.
    /// Returns `Err(Timeout)` on timeout.
    fn read_timeout(&self, timeout_ms: i32) -> Result<[u8; 65], Ctap2Error>;

    /// Human-readable manufacturer string (for logging).
    fn manufacturer(&self) -> &str;

    /// Human-readable product name (for logging).
    fn product(&self) -> &str;
}

// ── CTAPHID channel ──────────────────────────────────────────────────────────

/// An established CTAPHID channel with a specific device.
///
/// Created by [`CtapHidChannel::init`]; owns a CID allocated by the device.
pub struct CtapHidChannel<'d> {
    device: &'d dyn HidDevice,
    /// Channel ID allocated during CTAPHID_INIT.
    cid: u32,
}

impl<'d> CtapHidChannel<'d> {
    /// Perform the CTAPHID_INIT handshake and return a channel with the
    /// device-allocated CID.
    pub fn init(device: &'d dyn HidDevice) -> Result<Self, Ctap2Error> {
        let nonce = random_nonce();

        let mut report = [0u8; 65];
        report[0] = 0x00;
        write_u32(&mut report[1..5], CID_BROADCAST);
        report[5] = CMD_INIT;
        report[6] = 0x00;
        report[7] = 0x08; // BCNT = 8 nonce bytes
        report[8..16].copy_from_slice(&nonce);
        device.write(&report)?;

        // Up to 10 packets: skip unrelated ones, wait for matching INIT response.
        for _ in 0..10 {
            let pkt = device.read_timeout(3_000)?;
            let resp_cid = read_u32(&pkt[1..5]);
            let cmd = pkt[5];
            if resp_cid == CID_BROADCAST && cmd == CMD_INIT && pkt[8..16] == nonce {
                let channel_cid = read_u32(&pkt[16..20]);
                return Ok(CtapHidChannel { device, cid: channel_cid });
            }
        }
        Err(Ctap2Error::Protocol("CTAPHID_INIT: no matching response".into()))
    }

    /// Send a CTAP2 CBOR command and return the CBOR response payload (status
    /// byte stripped; `Err(DeviceStatus(n))` for non-zero status).
    pub fn send_cbor(&self, cbor: &[u8]) -> Result<Vec<u8>, Ctap2Error> {
        self.write_message(CMD_CBOR, cbor)?;
        let response = self.read_response(CMD_CBOR, 30_000)?;
        if response.is_empty() {
            return Err(Ctap2Error::Protocol("empty CBOR response".into()));
        }
        let status = response[0];
        if status != CTAP2_OK {
            return Err(Ctap2Error::DeviceStatus(status));
        }
        Ok(response[1..].to_vec())
    }

    /// Fragment `data` into CTAPHID packets and write them to the device.
    fn write_message(&self, cmd: u8, data: &[u8]) -> Result<(), Ctap2Error> {
        let total = data.len();
        let mut report = [0u8; 65];
        report[0] = 0x00;
        write_u32(&mut report[1..5], self.cid);
        report[5] = cmd;
        report[6] = ((total >> 8) & 0xFF) as u8;
        report[7] = (total & 0xFF) as u8;

        let first_chunk_len = total.min(INIT_DATA);
        report[8..8 + first_chunk_len].copy_from_slice(&data[..first_chunk_len]);
        self.device.write(&report)?;

        if total > INIT_DATA {
            let mut seq: u8 = 0;
            for chunk in data[INIT_DATA..].chunks(CONT_DATA) {
                let mut cont = [0u8; 65];
                cont[0] = 0x00;
                write_u32(&mut cont[1..5], self.cid);
                cont[5] = seq & 0x7F; // continuation: high bit clear
                cont[6..6 + chunk.len()].copy_from_slice(chunk);
                self.device.write(&cont)?;
                seq = seq.wrapping_add(1);
            }
        }
        Ok(())
    }

    /// Reassemble a response from CTAPHID packets, skipping KEEPALIVE frames.
    fn read_response(&self, expected_cmd: u8, timeout_ms: i32) -> Result<Vec<u8>, Ctap2Error> {
        // Read the initialization packet.
        let first = loop {
            let pkt = self.device.read_timeout(timeout_ms)?;
            let cid = read_u32(&pkt[1..5]);
            if cid != self.cid {
                continue;
            }
            let cmd = pkt[5];
            if cmd == CMD_KEEPALIVE {
                continue;
            }
            if cmd == CMD_ERROR {
                return Err(Ctap2Error::Protocol(format!("CTAPHID_ERROR 0x{:02x}", pkt[8])));
            }
            if cmd != expected_cmd {
                return Err(Ctap2Error::Protocol(format!(
                    "unexpected cmd 0x{:02x} (expected 0x{:02x})",
                    cmd, expected_cmd
                )));
            }
            break pkt;
        };

        let total = (usize::from(first[6]) << 8) | usize::from(first[7]);
        let mut buf = Vec::with_capacity(total);
        let first_chunk = &first[8..8 + INIT_DATA.min(total)];
        buf.extend_from_slice(first_chunk);

        let mut seq: u8 = 0;
        while buf.len() < total {
            let pkt = self.device.read_timeout(timeout_ms)?;
            let cid = read_u32(&pkt[1..5]);
            if cid != self.cid {
                continue;
            }
            let got_seq = pkt[5];
            if got_seq != (seq & 0x7F) {
                return Err(Ctap2Error::Protocol(format!(
                    "SEQ mismatch: expected {seq}, got {got_seq}"
                )));
            }
            seq = seq.wrapping_add(1);
            let remaining = total - buf.len();
            buf.extend_from_slice(&pkt[6..6 + CONT_DATA.min(remaining)]);
        }
        Ok(buf)
    }
}

// ── CBOR helpers (minimal encoder/decoder for CTAP2 payloads) ────────────────

/// Encode a CBOR integer-keyed map header.
fn cbor_map(n: u8) -> u8 {
    0xa0 | n
}

/// Encode a CBOR byte string.
fn cbor_bstr(out: &mut Vec<u8>, b: &[u8]) {
    cbor_head(out, 0x40, b.len() as u64);
    out.extend_from_slice(b);
}

/// Encode a CBOR text string.
fn cbor_tstr(out: &mut Vec<u8>, s: &str) {
    cbor_head(out, 0x60, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// Encode a CBOR unsigned integer.
fn cbor_uint(out: &mut Vec<u8>, v: u64) {
    cbor_head(out, 0x00, v);
}

/// Encode a CBOR integer (positive or negative).
fn cbor_int(out: &mut Vec<u8>, v: i64) {
    if v >= 0 {
        cbor_head(out, 0x00, v as u64);
    } else {
        cbor_head(out, 0x20, (-1 - v) as u64);
    }
}

fn cbor_head(out: &mut Vec<u8>, major: u8, len: u64) {
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

// ── Minimal CBOR decoder ─────────────────────────────────────────────────────

/// A CBOR value (minimal subset sufficient for CTAP2 responses).
#[derive(Debug, Clone)]
enum CborVal {
    Uint(u64),
    Bytes(Vec<u8>),
    Text(String),
    /// bool/null/undefined — stored without value (not needed for CTAP2 map lookups).
    Bool,
    Map(Vec<(CborVal, CborVal)>),
    /// Array values are decoded and discarded; this placeholder marks the position.
    Array,
}

impl CborVal {
    fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            CborVal::Bytes(b) => Some(b),
            _ => None,
        }
    }

    fn as_text(&self) -> Option<&str> {
        match self {
            CborVal::Text(s) => Some(s),
            _ => None,
        }
    }

    fn as_map(&self) -> Option<&[(CborVal, CborVal)]> {
        match self {
            CborVal::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Look up by positive integer key in a map.
    fn map_get_uint_key(&self, key: u64) -> Option<&CborVal> {
        self.as_map()?.iter().find_map(|(k, v)| {
            if matches!(k, CborVal::Uint(n) if *n == key) {
                Some(v)
            } else {
                None
            }
        })
    }
}

/// Decode one CBOR item from the start of `data`.
/// Returns `(value, remaining)`.
fn decode_cbor(data: &[u8]) -> Result<(CborVal, &[u8]), Ctap2Error> {
    let err = |s: &str| Ctap2Error::Cbor(s.to_owned());
    let (&head, rest) = data.split_first().ok_or_else(|| err("empty input"))?;
    let major = head & 0xe0;
    let info = head & 0x1f;

    let (len, rest) = decode_cbor_len(info, rest)?;

    match major {
        0x00 => Ok((CborVal::Uint(len), rest)),
        0x20 => Ok((CborVal::Uint(u64::MAX - len + 1), rest)), // negative — store as saturating
        0x40 => {
            if rest.len() < len as usize {
                return Err(err("bstr too short"));
            }
            let (b, rest) = rest.split_at(len as usize);
            Ok((CborVal::Bytes(b.to_vec()), rest))
        }
        0x60 => {
            if rest.len() < len as usize {
                return Err(err("tstr too short"));
            }
            let (b, rest) = rest.split_at(len as usize);
            Ok((CborVal::Text(String::from_utf8_lossy(b).into_owned()), rest))
        }
        0x80 => {
            // Decode and discard each item to advance the cursor.
            let mut cur = rest;
            for _ in 0..len {
                let (_, next) = decode_cbor(cur)?;
                cur = next;
            }
            Ok((CborVal::Array, cur))
        }
        0xa0 => {
            let mut pairs = Vec::with_capacity(len as usize);
            let mut cur = rest;
            for _ in 0..len {
                let (k, next) = decode_cbor(cur)?;
                let (v, next) = decode_cbor(next)?;
                pairs.push((k, v));
                cur = next;
            }
            Ok((CborVal::Map(pairs), cur))
        }
        0xe0 => match head {
            0xf4..=0xf7 => Ok((CborVal::Bool, rest)), // false/true/null/undefined
            _ => Err(err("unsupported simple value")),
        },
        _ => Err(err("unsupported CBOR major type")),
    }
}

fn decode_cbor_len(info: u8, rest: &[u8]) -> Result<(u64, &[u8]), Ctap2Error> {
    let err = |s: &str| Ctap2Error::Cbor(s.to_owned());
    match info {
        0..=23 => Ok((info as u64, rest)),
        24 => {
            let (&n, rest) = rest.split_first().ok_or_else(|| err("missing 1-byte len"))?;
            Ok((n as u64, rest))
        }
        25 => {
            if rest.len() < 2 {
                return Err(err("missing 2-byte len"));
            }
            Ok((u16::from_be_bytes([rest[0], rest[1]]) as u64, &rest[2..]))
        }
        26 => {
            if rest.len() < 4 {
                return Err(err("missing 4-byte len"));
            }
            Ok((u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as u64, &rest[4..]))
        }
        _ => Err(err("unsupported additional info")),
    }
}

// ── CTAP2 command builders ────────────────────────────────────────────────────

/// Build a CTAP2 `authenticatorMakeCredential` (0x01) CBOR payload.
///
/// The first byte is the command code; the rest is a CBOR map.
fn build_make_credential(req: &WebAuthnCreateRequest) -> Vec<u8> {
    let client_data_json = build_client_data_json("webauthn.create", &req.challenge, &req.origin);
    let client_data_hash = Sha256::digest(client_data_json.as_bytes());

    // Map with up to 5 entries: keys 1..5 (clientDataHash, rp, user, pubKeyCredParams, options)
    let mut out = Vec::new();
    out.push(CTAP_MAKE_CREDENTIAL);

    let map_len: u8 = if req.exclude_credentials.is_empty() { 4 } else { 5 };
    out.push(cbor_map(map_len));

    // 1: clientDataHash
    cbor_uint(&mut out, 1);
    cbor_bstr(&mut out, &client_data_hash);

    // 2: rp {id, name}
    cbor_uint(&mut out, 2);
    out.push(cbor_map(2));
    cbor_tstr(&mut out, "id");
    cbor_tstr(&mut out, &req.rp_id);
    cbor_tstr(&mut out, "name");
    cbor_tstr(&mut out, &req.rp_name);

    // 3: user {id, name, displayName}
    cbor_uint(&mut out, 3);
    out.push(cbor_map(3));
    cbor_tstr(&mut out, "id");
    cbor_bstr(&mut out, &req.user_id);
    cbor_tstr(&mut out, "name");
    cbor_tstr(&mut out, &req.user_name);
    cbor_tstr(&mut out, "displayName");
    cbor_tstr(&mut out, &req.user_display_name);

    // 4: pubKeyCredParams [{type, alg}…] — ES256 only
    cbor_uint(&mut out, 4);
    let alg_count = req.pub_key_algs.iter().filter(|&&a| a == -7).count().min(1) as u8;
    cbor_head(&mut out, 0x80, alg_count as u64);
    if req.pub_key_algs.contains(&-7) {
        out.push(cbor_map(2));
        cbor_tstr(&mut out, "type");
        cbor_tstr(&mut out, "public-key");
        cbor_tstr(&mut out, "alg");
        cbor_int(&mut out, -7);
    }

    // 5: excludeList (optional) [{id, type}…]
    if !req.exclude_credentials.is_empty() {
        cbor_uint(&mut out, 5);
        cbor_head(&mut out, 0x80, req.exclude_credentials.len() as u64);
        for id in &req.exclude_credentials {
            out.push(cbor_map(2));
            cbor_tstr(&mut out, "id");
            cbor_bstr(&mut out, id);
            cbor_tstr(&mut out, "type");
            cbor_tstr(&mut out, "public-key");
        }
    }

    // Store client_data_json alongside for the caller.
    // We return it via an out-of-band closure trick using thread-local.
    // Instead, return in a tuple — but our calling site needs it too.
    // We annotate with a thread_local for simplicity.
    set_last_client_data_json(client_data_json);
    out
}

/// Build a CTAP2 `authenticatorGetAssertion` (0x02) CBOR payload.
fn build_get_assertion(req: &WebAuthnGetRequest) -> Vec<u8> {
    let client_data_json = build_client_data_json("webauthn.get", &req.challenge, &req.origin);
    let client_data_hash = Sha256::digest(client_data_json.as_bytes());

    let mut out = Vec::new();
    out.push(CTAP_GET_ASSERTION);

    let map_len: u8 = if req.allow_credentials.is_empty() { 2 } else { 3 };
    out.push(cbor_map(map_len));

    // 1: rpId
    cbor_uint(&mut out, 1);
    cbor_tstr(&mut out, &req.rp_id);

    // 2: clientDataHash
    cbor_uint(&mut out, 2);
    cbor_bstr(&mut out, &client_data_hash);

    // 3: allowList (optional)
    if !req.allow_credentials.is_empty() {
        cbor_uint(&mut out, 3);
        cbor_head(&mut out, 0x80, req.allow_credentials.len() as u64);
        for id in &req.allow_credentials {
            out.push(cbor_map(2));
            cbor_tstr(&mut out, "id");
            cbor_bstr(&mut out, id);
            cbor_tstr(&mut out, "type");
            cbor_tstr(&mut out, "public-key");
        }
    }

    set_last_client_data_json(client_data_json);
    out
}

// Thread-local for returning clientDataJSON alongside the CBOR payload.
thread_local! {
    static LAST_CLIENT_DATA_JSON: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn set_last_client_data_json(s: String) {
    LAST_CLIENT_DATA_JSON.with(|c| *c.borrow_mut() = s);
}

fn take_last_client_data_json() -> String {
    LAST_CLIENT_DATA_JSON.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

// ── CTAP2 response parsers ────────────────────────────────────────────────────

/// Parsed `authenticatorMakeCredential` response.
struct MakeCredentialResponse {
    auth_data: Vec<u8>,
    /// Raw CTAP2 response bytes (usable as attestation_object).
    raw: Vec<u8>,
}

fn parse_make_credential_response(data: &[u8]) -> Result<MakeCredentialResponse, Ctap2Error> {
    let (val, _) = decode_cbor(data)?;
    // key 1 = fmt (text), key 2 = authData (bytes), key 3 = attStmt (map)
    val.map_get_uint_key(1)
        .and_then(|v| v.as_text())
        .ok_or_else(|| Ctap2Error::Cbor("missing fmt".into()))?;
    let auth_data = val
        .map_get_uint_key(2)
        .and_then(|v| v.as_bytes())
        .ok_or_else(|| Ctap2Error::Cbor("missing authData".into()))?
        .to_vec();
    Ok(MakeCredentialResponse {
        auth_data,
        raw: data.to_vec(),
    })
}

/// Parsed `authenticatorGetAssertion` response.
struct GetAssertionResponse {
    credential_id: Vec<u8>,
    auth_data: Vec<u8>,
    signature: Vec<u8>,
    user_handle: Option<Vec<u8>>,
}

fn parse_get_assertion_response(data: &[u8]) -> Result<GetAssertionResponse, Ctap2Error> {
    let (val, _) = decode_cbor(data)?;
    let credential_id = val
        .map_get_uint_key(1)
        .and_then(|v| v.as_map())
        .and_then(|m| {
            m.iter().find_map(|(k, v)| {
                if k.as_text() == Some("id") {
                    v.as_bytes().map(|b| b.to_vec())
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| Ctap2Error::Cbor("missing credential.id".into()))?;
    let auth_data = val
        .map_get_uint_key(2)
        .and_then(|v| v.as_bytes())
        .ok_or_else(|| Ctap2Error::Cbor("missing authData".into()))?
        .to_vec();
    let signature = val
        .map_get_uint_key(3)
        .and_then(|v| v.as_bytes())
        .ok_or_else(|| Ctap2Error::Cbor("missing signature".into()))?
        .to_vec();
    let user_handle = val
        .map_get_uint_key(4)
        .and_then(|v| v.as_map())
        .and_then(|m| {
            m.iter().find_map(|(k, v)| {
                if k.as_text() == Some("id") {
                    v.as_bytes().map(|b| b.to_vec())
                } else {
                    None
                }
            })
        });
    Ok(GetAssertionResponse {
        credential_id,
        auth_data,
        signature,
        user_handle,
    })
}

/// Extract the credential ID from the `authenticatorData` byte string.
///
/// Layout (W3C WebAuthn §6.1): `rpIdHash(32) || flags(1) || signCount(4) ||
/// aaguid(16) || credIdLen(2 BE) || credId || cose`.
/// Returns `None` if `AT` flag (bit 6) is not set or bytes are too short.
pub fn extract_credential_id(auth_data: &[u8]) -> Option<Vec<u8>> {
    if auth_data.len() < 37 {
        return None;
    }
    let flags = auth_data[32];
    if flags & 0x40 == 0 {
        return None; // AT bit not set
    }
    if auth_data.len() < 53 {
        return None; // too short for AAGUID + len
    }
    // bytes 37..53 = AAGUID (16 bytes)
    let cred_id_len = u16::from_be_bytes([auth_data[53], auth_data[54]]) as usize;
    if auth_data.len() < 55 + cred_id_len {
        return None;
    }
    Some(auth_data[55..55 + cred_id_len].to_vec())
}

// ── clientDataJSON ────────────────────────────────────────────────────────────

fn build_client_data_json(ceremony_type: &str, challenge: &[u8], origin: &str) -> String {
    format!(
        "{{\"type\":\"{}\",\"challenge\":\"{}\",\"origin\":\"{}\",\"crossOrigin\":false}}",
        json_escape(ceremony_type),
        base64url(challenge),
        json_escape(origin),
    )
}

fn base64url(data: &[u8]) -> String {
    const A: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(A[(n >> 18 & 0x3f) as usize] as char);
        out.push(A[(n >> 12 & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(A[(n >> 6 & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(A[(n & 0x3f) as usize] as char);
        }
    }
    out
}

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

// ── Little helpers ────────────────────────────────────────────────────────────

fn write_u32(buf: &mut [u8], v: u32) {
    buf[..4].copy_from_slice(&v.to_be_bytes());
}

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

// ── Physical device probe (Phase 0 stub) ─────────────────────────────────────

/// Enumerate connected FIDO2 USB HID devices.
///
/// **Phase 0:** returns an empty list; no platform HID backend is implemented yet.
/// Phase 1: replace with Windows `HidD_GetHidGuid` + `SetupDiEnumDeviceInterfaces`,
/// Linux `/dev/hidrawN` sysfs scan, macOS `IOHIDManager`.
pub fn probe_usb_fido_devices() -> Vec<Box<dyn HidDevice>> {
    // Phase 1: platform HID enumeration goes here.
    vec![]
}

// ── CtapRoamingTransport ──────────────────────────────────────────────────────

/// [`CredentialProvider`] that uses a connected FIDO2 USB security key.
///
/// On each operation it calls [`probe_usb_fido_devices`] to discover keys,
/// tries them in order, and returns the first successful result. Returns
/// `NotAllowed` if no device is present or all devices fail.
///
/// Phase 0: always returns `NotAllowed` because `probe_usb_fido_devices` is a stub.
#[derive(Default)]
pub struct CtapRoamingTransport;

impl CtapRoamingTransport {
    /// Create a new roaming transport.
    pub fn new() -> Self {
        Self
    }
}

impl CredentialProvider for CtapRoamingTransport {
    fn create(&self, req: &WebAuthnCreateRequest) -> Result<WebAuthnCreateResponse, WebAuthnError> {
        if !req.pub_key_algs.contains(&-7) {
            return Err(WebAuthnError::Constraint);
        }
        for device in probe_usb_fido_devices() {
            match try_create_on_device(device.as_ref(), req) {
                Ok(r) => return Ok(r),
                Err(_) => continue,
            }
        }
        Err(WebAuthnError::NotAllowed)
    }

    fn get(&self, req: &WebAuthnGetRequest) -> Result<WebAuthnGetResponse, WebAuthnError> {
        for device in probe_usb_fido_devices() {
            match try_get_on_device(device.as_ref(), req) {
                Ok(r) => return Ok(r),
                Err(_) => continue,
            }
        }
        Err(WebAuthnError::NotAllowed)
    }

    fn is_user_verifying_platform_authenticator_available(&self) -> bool {
        !probe_usb_fido_devices().is_empty()
    }
}

fn try_create_on_device(
    device: &dyn HidDevice,
    req: &WebAuthnCreateRequest,
) -> Result<WebAuthnCreateResponse, WebAuthnError> {
    let channel = CtapHidChannel::init(device).map_err(WebAuthnError::from)?;
    let cbor = build_make_credential(req);
    let client_data_json = take_last_client_data_json();
    let raw_resp = channel.send_cbor(&cbor).map_err(WebAuthnError::from)?;
    let parsed = parse_make_credential_response(&raw_resp).map_err(WebAuthnError::from)?;

    let credential_id = extract_credential_id(&parsed.auth_data)
        .ok_or(WebAuthnError::NotAllowed)?;

    Ok(WebAuthnCreateResponse {
        credential_id,
        attestation_object: parsed.raw,
        client_data_json: client_data_json.into_bytes(),
        authenticator_data: parsed.auth_data,
        public_key_alg: -7,
        public_key_der: None,
        transports: vec!["usb".to_owned()],
    })
}

fn try_get_on_device(
    device: &dyn HidDevice,
    req: &WebAuthnGetRequest,
) -> Result<WebAuthnGetResponse, WebAuthnError> {
    let channel = CtapHidChannel::init(device).map_err(WebAuthnError::from)?;
    let cbor = build_get_assertion(req);
    let client_data_json = take_last_client_data_json();
    let raw_resp = channel.send_cbor(&cbor).map_err(WebAuthnError::from)?;
    let parsed = parse_get_assertion_response(&raw_resp).map_err(WebAuthnError::from)?;

    Ok(WebAuthnGetResponse {
        credential_id: parsed.credential_id,
        authenticator_data: parsed.auth_data,
        signature: parsed.signature,
        client_data_json: client_data_json.into_bytes(),
        user_handle: parsed.user_handle,
    })
}

// ── CompositeCredentialProvider ───────────────────────────────────────────────

/// A [`CredentialProvider`] that delegates to a priority-ordered list.
///
/// Each provider is tried in order; the first result that is not
/// `Err(NotAllowed)` is returned. If all return `NotAllowed`, the composite
/// also returns `NotAllowed`.
///
/// Typical shell wiring:
/// ```ignore
/// set_credential_provider(Arc::new(CompositeCredentialProvider::new(vec![
///     Arc::new(CtapRoamingTransport::new()),   // USB key first
///     Arc::new(VirtualAuthenticator::new()),   // software fallback
/// ])));
/// ```
pub struct CompositeCredentialProvider {
    providers: Vec<Arc<dyn CredentialProvider>>,
}

impl CompositeCredentialProvider {
    /// Create a composite from an ordered list of providers.
    pub fn new(providers: Vec<Arc<dyn CredentialProvider>>) -> Self {
        Self { providers }
    }
}

impl CredentialProvider for CompositeCredentialProvider {
    fn create(&self, req: &WebAuthnCreateRequest) -> Result<WebAuthnCreateResponse, WebAuthnError> {
        for p in &self.providers {
            match p.create(req) {
                Err(WebAuthnError::NotAllowed) => continue,
                other => return other,
            }
        }
        Err(WebAuthnError::NotAllowed)
    }

    fn get(&self, req: &WebAuthnGetRequest) -> Result<WebAuthnGetResponse, WebAuthnError> {
        for p in &self.providers {
            match p.get(req) {
                Err(WebAuthnError::NotAllowed) => continue,
                other => return other,
            }
        }
        Err(WebAuthnError::NotAllowed)
    }

    fn is_user_verifying_platform_authenticator_available(&self) -> bool {
        self.providers
            .iter()
            .any(|p| p.is_user_verifying_platform_authenticator_available())
    }
}

// ── MockHidDevice (test helper) ───────────────────────────────────────────────

/// A scripted in-memory [`HidDevice`] for unit tests.
///
/// Produces responses from a pre-loaded queue and records writes for
/// verification. The queue is consumed FIFO; panics on underflow.
pub struct MockHidDevice {
    /// Writes received from the client (CID|CMD|BCNT|data…).
    pub writes: Mutex<Vec<[u8; 65]>>,
    /// Pre-loaded read responses served in FIFO order.
    responses: Mutex<Vec<[u8; 65]>>,
    name: String,
}

impl MockHidDevice {
    /// Create a blank mock with no queued responses.
    pub fn new(name: &str) -> Self {
        Self {
            writes: Mutex::new(vec![]),
            responses: Mutex::new(vec![]),
            name: name.to_owned(),
        }
    }

    /// Push a raw 65-byte HID report to the response queue.
    pub fn push_response(&self, report: [u8; 65]) {
        self.responses.lock().unwrap().push(report);
    }

    /// Build and queue a CTAPHID_INIT response for the given nonce + CID.
    pub fn queue_init_response(&self, nonce: &[u8; 8], allocated_cid: u32) {
        let mut r = [0u8; 65];
        write_u32(&mut r[1..5], CID_BROADCAST);
        r[5] = CMD_INIT;
        r[6] = 0x00;
        r[7] = 0x11; // BCNT = 17
        r[8..16].copy_from_slice(nonce);
        write_u32(&mut r[16..20], allocated_cid);
        r[20] = 0x02; // CTAP2 protocol version
        r[21] = 0x01; // major
        r[22] = 0x00; // minor
        r[23] = 0x00; // build
        r[24] = 0x04; // caps: CBOR
        self.push_response(r);
    }

    /// Build and queue a successful CTAPHID_CBOR response with the given payload.
    pub fn queue_cbor_response(&self, cid: u32, payload: &[u8]) {
        let total = 1 + payload.len(); // status byte + payload
        let mut all = vec![CTAP2_OK];
        all.extend_from_slice(payload);

        // First packet
        let mut r = [0u8; 65];
        write_u32(&mut r[1..5], cid);
        r[5] = CMD_CBOR;
        r[6] = ((total >> 8) & 0xFF) as u8;
        r[7] = (total & 0xFF) as u8;
        let first_len = INIT_DATA.min(all.len());
        r[8..8 + first_len].copy_from_slice(&all[..first_len]);
        self.push_response(r);

        // Continuation packets
        if all.len() > INIT_DATA {
            let mut seq: u8 = 0;
            for chunk in all[INIT_DATA..].chunks(CONT_DATA) {
                let mut cr = [0u8; 65];
                write_u32(&mut cr[1..5], cid);
                cr[5] = seq & 0x7F;
                cr[6..6 + chunk.len()].copy_from_slice(chunk);
                self.push_response(cr);
                seq = seq.wrapping_add(1);
            }
        }
    }

    /// Return all written reports (as slices) for inspection.
    pub fn written_reports(&self) -> Vec<[u8; 65]> {
        self.writes.lock().unwrap().clone()
    }
}

impl HidDevice for MockHidDevice {
    fn write(&self, report: &[u8; 65]) -> Result<(), Ctap2Error> {
        self.writes.lock().unwrap().push(*report);
        Ok(())
    }

    fn read_timeout(&self, _timeout_ms: i32) -> Result<[u8; 65], Ctap2Error> {
        self.responses
            .lock()
            .unwrap()
            .pop()
            .ok_or(Ctap2Error::Timeout)
    }

    fn manufacturer(&self) -> &str {
        "Test Vendor"
    }

    fn product(&self) -> &str {
        &self.name
    }
}

// MockHidDevice pops from the back; reverse the queue so first-pushed = first-served.
impl MockHidDevice {
    /// Reverse the internal response queue so items are served FIFO.
    pub fn seal(&self) {
        self.responses.lock().unwrap().reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CID: u32 = 0x0100_0001;

    #[test]
    fn cbor_decode_uint() {
        let data = [0x01u8]; // uint(1)
        let (v, rest) = decode_cbor(&data).unwrap();
        assert!(matches!(v, CborVal::Uint(1)));
        assert!(rest.is_empty());
    }

    #[test]
    fn cbor_decode_bytes() {
        let mut data = vec![0x43u8]; // bstr(3)
        data.extend_from_slice(b"abc");
        let (v, rest) = decode_cbor(&data).unwrap();
        assert_eq!(v.as_bytes(), Some(b"abc" as &[u8]));
        assert!(rest.is_empty());
    }

    #[test]
    fn cbor_decode_text() {
        let mut data = vec![0x63u8]; // tstr(3)
        data.extend_from_slice(b"foo");
        let (v, rest) = decode_cbor(&data).unwrap();
        assert_eq!(v.as_text(), Some("foo"));
        assert!(rest.is_empty());
    }

    #[test]
    fn cbor_decode_map_uint_keys() {
        // {1: h'deadbeef', 2: "hello"}
        let mut data = vec![0xa2u8]; // map(2)
        data.push(0x01); // key 1
        data.push(0x44); // bstr(4)
        data.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        data.push(0x02); // key 2
        data.push(0x65); // tstr(5)
        data.extend_from_slice(b"hello");
        let (v, _) = decode_cbor(&data).unwrap();
        let b = v.map_get_uint_key(1).unwrap().as_bytes().unwrap();
        assert_eq!(b, &[0xde, 0xad, 0xbe, 0xef]);
        let t = v.map_get_uint_key(2).unwrap().as_text().unwrap();
        assert_eq!(t, "hello");
    }

    #[test]
    fn hid_packet_fragmentation_single() {
        // A payload ≤ 57 bytes fits in one packet.
        let payload = vec![0x01u8; 20];
        let mock = MockHidDevice::new("test");
        let ch = CtapHidChannel { device: &mock, cid: TEST_CID };
        ch.write_message(CMD_CBOR, &payload).unwrap();
        let writes = mock.written_reports();
        assert_eq!(writes.len(), 1, "single packet expected");
        let pkt = &writes[0];
        assert_eq!(pkt[0], 0x00, "report ID must be 0");
        assert_eq!(read_u32(&pkt[1..5]), TEST_CID, "CID matches");
        assert_eq!(pkt[5], CMD_CBOR, "command matches");
        assert_eq!((pkt[6] as usize) << 8 | pkt[7] as usize, 20, "BCNT = 20");
        assert_eq!(&pkt[8..28], &payload[..], "payload embedded");
    }

    #[test]
    fn hid_packet_fragmentation_multi() {
        // Payload > 57 bytes → init + 1 continuation.
        let payload = vec![0xabu8; 60]; // 57 in init + 3 in cont
        let mock = MockHidDevice::new("test");
        let ch = CtapHidChannel { device: &mock, cid: TEST_CID };
        ch.write_message(CMD_CBOR, &payload).unwrap();
        let writes = mock.written_reports();
        assert_eq!(writes.len(), 2, "init + 1 continuation");
        // Init packet
        assert_eq!(writes[0][5], CMD_CBOR);
        assert_eq!(&writes[0][8..65], &payload[..57]);
        // Continuation packet: byte 5 is SEQ = 0 (high bit clear)
        assert_eq!(writes[1][5], 0x00, "SEQ = 0");
        assert_eq!(&writes[1][6..9], &payload[57..60]);
    }

    #[test]
    fn hid_reassemble_single_packet() {
        // queue_cbor_response prepends CTAP2_OK automatically.
        let data = vec![0x01u8, 0x02, 0x03, 0x04];
        let mock = MockHidDevice::new("test");
        mock.queue_cbor_response(TEST_CID, &data);
        mock.seal();
        let ch = CtapHidChannel { device: &mock, cid: TEST_CID };
        let resp = ch.read_response(CMD_CBOR, 1000).unwrap();
        assert_eq!(resp[0], CTAP2_OK);
        assert_eq!(&resp[1..], &data[..]);
    }

    #[test]
    fn hid_reassemble_large_response() {
        // 100 bytes of payload → init + 1 cont packet
        let payload: Vec<u8> = (0u8..100).collect();
        let mock = MockHidDevice::new("test");
        mock.queue_cbor_response(TEST_CID, &payload);
        mock.seal();
        let ch = CtapHidChannel { device: &mock, cid: TEST_CID };
        let resp = ch.read_response(CMD_CBOR, 1000).unwrap();
        // resp[0] is CTAP2_OK prepended by queue_cbor_response
        assert_eq!(resp[0], CTAP2_OK);
        assert_eq!(&resp[1..], &payload[..]);
    }

    #[test]
    fn probe_returns_empty_in_phase_0() {
        let devices = probe_usb_fido_devices();
        assert!(devices.is_empty(), "Phase 0: no physical devices");
    }

    #[test]
    fn roaming_transport_returns_not_allowed_with_no_devices() {
        let transport = CtapRoamingTransport::new();
        let req = WebAuthnCreateRequest {
            rp_id: "example.com".into(),
            rp_name: "Example".into(),
            user_id: vec![1, 2, 3],
            user_name: "user".into(),
            user_display_name: "User".into(),
            challenge: vec![9, 8, 7],
            origin: "https://example.com".into(),
            pub_key_algs: vec![-7],
            require_user_verification: false,
            exclude_credentials: vec![],
        };
        assert!(matches!(transport.create(&req), Err(WebAuthnError::NotAllowed)));
    }

    #[test]
    fn composite_falls_back_to_second_provider() {
        use crate::webauthn::VirtualAuthenticator;

        let composite = CompositeCredentialProvider::new(vec![
            Arc::new(CtapRoamingTransport::new()), // always NotAllowed (Phase 0)
            Arc::new(VirtualAuthenticator::new()),  // software fallback
        ]);
        let req = WebAuthnCreateRequest {
            rp_id: "example.com".into(),
            rp_name: "Example".into(),
            user_id: vec![1, 2, 3],
            user_name: "user".into(),
            user_display_name: "User".into(),
            challenge: vec![9, 8, 7],
            origin: "https://example.com".into(),
            pub_key_algs: vec![-7],
            require_user_verification: false,
            exclude_credentials: vec![],
        };
        let resp = composite.create(&req).unwrap();
        assert_eq!(resp.public_key_alg, -7);
        // VirtualAuthenticator marks transport as "internal"
        assert_eq!(resp.transports, vec!["internal"]);
    }

    #[test]
    fn composite_all_not_allowed_propagates() {
        let composite = CompositeCredentialProvider::new(vec![
            Arc::new(CtapRoamingTransport::new()),
        ]);
        let req = WebAuthnCreateRequest {
            rp_id: "example.com".into(),
            rp_name: "Example".into(),
            user_id: vec![1],
            user_name: "u".into(),
            user_display_name: "U".into(),
            challenge: vec![1],
            origin: "https://example.com".into(),
            pub_key_algs: vec![-7],
            require_user_verification: false,
            exclude_credentials: vec![],
        };
        assert!(matches!(composite.create(&req), Err(WebAuthnError::NotAllowed)));
    }

    #[test]
    fn extract_credential_id_from_auth_data_parses_correctly() {
        // Build minimal authenticatorData with AT flag set.
        let mut auth_data = vec![0u8; 37]; // rpIdHash(32) + flags(1) + signCount(4)
        auth_data[32] = 0x41; // UP | AT
        auth_data.extend_from_slice(&[0u8; 16]); // AAGUID (16 bytes)
        let cred_id = vec![0xde, 0xad, 0xbe, 0xef, 0x01, 0x02];
        let len = (cred_id.len() as u16).to_be_bytes();
        auth_data.extend_from_slice(&len);
        auth_data.extend_from_slice(&cred_id);

        let extracted = extract_credential_id(&auth_data).unwrap();
        assert_eq!(extracted, cred_id);
    }

    #[test]
    fn base64url_encoding() {
        assert_eq!(base64url(&[]), "");
        assert_eq!(base64url(&[0]), "AA");
        assert_eq!(base64url(&[0, 1, 2]), "AAEC");
        // Bytes that hit the '-' and '_' characters (indices 62, 63).
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn client_data_json_format() {
        let j = build_client_data_json("webauthn.create", &[0, 1, 2], "https://a.test");
        assert!(j.contains("\"type\":\"webauthn.create\""));
        assert!(j.contains("\"challenge\":\"AAEC\""));
        assert!(j.contains("\"origin\":\"https://a.test\""));
        assert!(j.contains("\"crossOrigin\":false"));
    }
}
