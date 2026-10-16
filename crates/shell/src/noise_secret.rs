//! The per-profile secret behind canvas / audio fingerprint noise
//! (COMPAT-PRIVACY-UNBREAK-NOISE).
//!
//! One random master secret lives in `<data>/fp_noise_secret` (created on the
//! first run, 16 bytes of OS randomness as hex). Each persistent profile
//! (Personal, Work, …) derives its own 64-bit secret from it, so the noise is
//! the same after a restart and different between profiles. The Anonymous
//! profile and Tor Browser get no stable secret — a fresh one per run, as
//! before: a new fingerprint on every launch is what those modes want.

use std::path::Path;
use std::sync::OnceLock;

const FILE_NAME: &str = "fp_noise_secret";

/// The master secret of this installation: read from `dir`, or generated and
/// stored there. If the file cannot be written the secret is still returned
/// (stable for this run only).
pub fn master_secret(dir: &Path) -> [u8; 16] {
    let path = dir.join(FILE_NAME);
    if let Some(bytes) = std::fs::read_to_string(&path).ok().and_then(|s| parse_hex(s.trim())) {
        return bytes;
    }
    let mut bytes = [0u8; 16];
    if getrandom::getrandom(&mut bytes).is_err() {
        // No OS randomness: fall back to the clock, never to a constant.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        bytes = nanos.to_le_bytes();
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&path, to_hex(&bytes));
    bytes
}

/// The process-wide master secret, read once from the browser data directory.
pub fn installation_secret() -> [u8; 16] {
    static MASTER: OnceLock<[u8; 16]> = OnceLock::new();
    *MASTER.get_or_init(|| master_secret(&crate::adblock::browser_data_dir()))
}

/// The noise secret of one profile (`profile_key` — a stable profile id).
pub fn derive(master: &[u8; 16], profile_key: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in master.iter().chain(b"/profile/").chain(profile_key.as_bytes()) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    // One avalanche round so profile keys that differ in a byte do not give
    // seeds that differ only in the low bits.
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 33)
}

fn to_hex(bytes: &[u8; 16]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_hex(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_secret_is_created_once_and_read_back() {
        let dir = std::env::temp_dir().join(format!("lumen-noise-secret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = master_secret(&dir);
        assert_eq!(first, master_secret(&dir), "a restart reads the same secret");
        assert_ne!(first, [0u8; 16]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_replaced() {
        let dir = std::env::temp_dir().join(format!("lumen-noise-secret-bad-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(FILE_NAME), "not hex").unwrap();
        let s = master_secret(&dir);
        assert_eq!(s, master_secret(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profiles_get_different_secrets_from_one_master() {
        let m = [9u8; 16];
        assert_eq!(derive(&m, "1"), derive(&m, "1"));
        assert_ne!(derive(&m, "1"), derive(&m, "2"));
        assert_ne!(derive(&m, "1"), derive(&[8u8; 16], "1"));
    }
}
