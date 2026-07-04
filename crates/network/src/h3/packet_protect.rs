//! QUIC packet protection — RFC 9001 §5.3 (AEAD payload protection) + §5.4
//! (header protection), for the `AEAD_AES_128_GCM` cipher suite QUIC v1 mandates
//! for Initial packets.
//!
//! This slice consumes the `key` / `iv` / `hp` material that
//! [`super::key_schedule::PacketProtectionKeys`] derives and turns it into the
//! two transforms that actually protect a packet on the wire:
//!
//! - **AEAD payload protection (§5.3).** The packet payload is sealed with
//!   AES-128-GCM. The nonce is the 96-bit `iv` XORed, right-aligned, with the
//!   packet number ([`quic_nonce`]); the associated data is the packet's
//!   *unprotected* header (first byte through the end of the packet-number
//!   field). [`aes_128_gcm_seal`] returns `ciphertext || tag` (the 16-byte GCM
//!   tag appended, exactly the QUIC wire layout); [`aes_128_gcm_open`] verifies
//!   the tag and returns the plaintext.
//! - **Header protection (§5.4).** After the payload is sealed, a 16-byte sample
//!   of the ciphertext (taken at a fixed offset past the packet-number field)
//!   is encrypted with the header-protection key to produce a 5-byte mask
//!   ([`aes_128_hp_mask`]). The mask hides the packet-number length and low bits
//!   of the first byte plus the packet-number octets ([`apply_header_protection`]
//!   / [`remove_header_protection`]). This is the *last* transform applied when
//!   sending and the *first* removed when receiving, because the sample is taken
//!   from the AEAD-protected ciphertext.
//!
//! ## Scope
//!
//! - Only `AEAD_AES_128_GCM` + AES header protection (RFC 9001 §5.4.3). The
//!   `AEAD_CHACHA20_POLY1305` suite (ChaCha20-based header protection, §5.4.4)
//!   is deferred — Initial packets never use it, and the negotiated 1-RTT suite
//!   is chosen by the handshake, which is a later slice.
//! - Pure functions over byte buffers plus a caller-supplied packet-number
//!   offset. No packet parsing (that is [`super::packet`]), no packet-number
//!   encoding/decoding, no IO.
//!
//! Validated against the RFC 9001 Appendix A.2 (client Initial) and A.3 (server
//! Initial) test vectors and a McGrew–Viega AES-128-GCM known-answer test.

use aes::Aes128;
use aes::cipher::generic_array::GenericArray;
// `aes::cipher::KeyInit` and `aes_gcm::KeyInit` re-export the same
// `crypto_common::KeyInit` trait, so importing it once covers both
// `Aes128::new_from_slice` and `Aes128Gcm::new_from_slice`.
use aes::cipher::{BlockEncrypt, KeyInit};
use aes_gcm::{AeadInPlace, Aes128Gcm, Nonce, Tag};

/// AEAD authentication-tag length in bytes (AES-128-GCM, RFC 9001 §5.3: the tag
/// is 16 octets and is appended to the ciphertext on the wire).
pub const AEAD_TAG_LEN: usize = 16;

/// Header-protection sample length in bytes (RFC 9001 §5.4.2: exactly one
/// AES block is sampled from the ciphertext).
pub const HP_SAMPLE_LEN: usize = 16;

/// Number of mask bytes header protection consumes (RFC 9001 §5.4.1): one for
/// the first byte's low bits plus up to four packet-number octets.
pub const HP_MASK_LEN: usize = 5;

/// Fixed distance, in bytes, from the start of the packet-number field to the
/// start of the header-protection sample (RFC 9001 §5.4.2). Because the largest
/// packet number is four octets, sampling four bytes past `pn_offset` always
/// lands inside the ciphertext regardless of the actual packet-number length.
pub const HP_SAMPLE_OFFSET_FROM_PN: usize = 4;

/// Errors from the packet-protection transforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectionError {
    /// An AEAD key was not the 16 bytes AES-128 requires.
    BadKeyLength,
    /// The AEAD nonce base (`iv`) was not the 12 bytes RFC 9001 §5.3 requires.
    BadIvLength,
    /// A header-protection key was not the 16 bytes AES-128 requires.
    BadHpKeyLength,
    /// The buffer was too short to hold the header-protection sample at the
    /// required offset, or too short for the packet-number field.
    BufferTooShort,
    /// AEAD authentication failed on open (tag mismatch or truncated input).
    AeadFailed,
}

impl core::fmt::Display for ProtectionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            ProtectionError::BadKeyLength => "AEAD key must be 16 bytes",
            ProtectionError::BadIvLength => "AEAD iv must be 12 bytes",
            ProtectionError::BadHpKeyLength => "header-protection key must be 16 bytes",
            ProtectionError::BufferTooShort => "packet buffer too short for protection",
            ProtectionError::AeadFailed => "AEAD authentication failed",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for ProtectionError {}

// ── AEAD payload protection (RFC 9001 §5.3) ─────────────────────────────────

/// Construct the AEAD nonce for a packet (RFC 9001 §5.3): the 12-byte `iv` with
/// the 62-bit packet number encoded big-endian and XORed into the right-hand
/// (least-significant) bytes. `iv` must be exactly [`AEAD_IV_LEN`] bytes.
///
/// [`AEAD_IV_LEN`]: super::key_schedule::AEAD_IV_LEN
#[must_use]
fn quic_nonce(iv: &[u8; super::key_schedule::AEAD_IV_LEN], packet_number: u64) -> [u8; super::key_schedule::AEAD_IV_LEN] {
    let mut nonce = *iv;
    let pn = packet_number.to_be_bytes();
    // The 8-byte packet number is right-aligned against the 12-byte nonce, so it
    // XORs into the last 8 octets; the leading 4 octets of `iv` pass through.
    let start = super::key_schedule::AEAD_IV_LEN - pn.len();
    for (dst, src) in nonce[start..].iter_mut().zip(pn.iter()) {
        *dst ^= *src;
    }
    nonce
}

/// Coerce an `iv` slice to the fixed nonce-base array, rejecting a wrong length.
fn iv_array(iv: &[u8]) -> Result<[u8; super::key_schedule::AEAD_IV_LEN], ProtectionError> {
    let mut arr = [0u8; super::key_schedule::AEAD_IV_LEN];
    if iv.len() != arr.len() {
        return Err(ProtectionError::BadIvLength);
    }
    arr.copy_from_slice(iv);
    Ok(arr)
}

/// Seal a packet payload with AES-128-GCM (RFC 9001 §5.3). Returns
/// `ciphertext || tag` — the plaintext encrypted followed by the 16-byte GCM
/// authentication tag, which is exactly the protected-payload layout on the
/// QUIC wire.
///
/// `aad` is the packet's unprotected header (first byte through the end of the
/// packet-number field). `key` must be 16 bytes and `iv` 12 bytes.
pub fn aes_128_gcm_seal(
    key: &[u8],
    iv: &[u8],
    packet_number: u64,
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, ProtectionError> {
    let iv = iv_array(iv)?;
    let cipher = Aes128Gcm::new_from_slice(key).map_err(|_| ProtectionError::BadKeyLength)?;
    let nonce = quic_nonce(&iv, packet_number);
    let mut buf = plaintext.to_vec();
    let tag = cipher
        .encrypt_in_place_detached(Nonce::from_slice(&nonce), aad, &mut buf)
        .map_err(|_| ProtectionError::AeadFailed)?;
    buf.extend_from_slice(tag.as_slice());
    Ok(buf)
}

/// Open a sealed packet payload with AES-128-GCM (RFC 9001 §5.3), verifying the
/// trailing 16-byte tag. `protected` is `ciphertext || tag`; `aad` is the packet's
/// unprotected header. Returns the plaintext, or [`ProtectionError::AeadFailed`]
/// if authentication fails or the input is shorter than the tag.
pub fn aes_128_gcm_open(
    key: &[u8],
    iv: &[u8],
    packet_number: u64,
    aad: &[u8],
    protected: &[u8],
) -> Result<Vec<u8>, ProtectionError> {
    let iv = iv_array(iv)?;
    if protected.len() < AEAD_TAG_LEN {
        return Err(ProtectionError::AeadFailed);
    }
    let cipher = Aes128Gcm::new_from_slice(key).map_err(|_| ProtectionError::BadKeyLength)?;
    let nonce = quic_nonce(&iv, packet_number);
    let (ciphertext, tag_bytes) = protected.split_at(protected.len() - AEAD_TAG_LEN);
    let mut buf = ciphertext.to_vec();
    cipher
        .decrypt_in_place_detached(Nonce::from_slice(&nonce), aad, &mut buf, Tag::from_slice(tag_bytes))
        .map_err(|_| ProtectionError::AeadFailed)?;
    Ok(buf)
}

// ── Header protection (RFC 9001 §5.4) ───────────────────────────────────────

/// Derive the 5-byte header-protection mask from a ciphertext sample
/// (RFC 9001 §5.4.3): `AES-ECB(hp_key, sample)` truncated to the first
/// [`HP_MASK_LEN`] bytes. `hp_key` must be 16 bytes and `sample` at least
/// [`HP_SAMPLE_LEN`] bytes.
pub fn aes_128_hp_mask(hp_key: &[u8], sample: &[u8]) -> Result<[u8; HP_MASK_LEN], ProtectionError> {
    if sample.len() < HP_SAMPLE_LEN {
        return Err(ProtectionError::BufferTooShort);
    }
    let cipher = Aes128::new_from_slice(hp_key).map_err(|_| ProtectionError::BadHpKeyLength)?;
    let mut block = GenericArray::clone_from_slice(&sample[..HP_SAMPLE_LEN]);
    cipher.encrypt_block(&mut block);
    let mut mask = [0u8; HP_MASK_LEN];
    mask.copy_from_slice(&block[..HP_MASK_LEN]);
    Ok(mask)
}

/// The low bits of the first byte header protection masks: 4 bits for a long
/// header (the reserved + packet-number-length bits, RFC 9000 §17.2), 5 bits for
/// a short header (adding the key-phase bit, §17.3).
const fn first_byte_mask(long_header: bool) -> u8 {
    if long_header { 0x0f } else { 0x1f }
}

/// Compute the sample offset and validate the packet buffer is long enough for
/// both the sample and the maximum packet-number field.
fn sample_bounds(packet_len: usize, pn_offset: usize) -> Result<usize, ProtectionError> {
    let sample_offset = pn_offset
        .checked_add(HP_SAMPLE_OFFSET_FROM_PN)
        .ok_or(ProtectionError::BufferTooShort)?;
    let sample_end = sample_offset
        .checked_add(HP_SAMPLE_LEN)
        .ok_or(ProtectionError::BufferTooShort)?;
    if sample_end > packet_len {
        return Err(ProtectionError::BufferTooShort);
    }
    Ok(sample_offset)
}

/// Apply header protection to an assembled packet in place (RFC 9001 §5.4.1, the
/// *sender* side). The packet number in `packet[pn_offset..]` must still be in
/// the clear: its length is read from the low 2 bits of the (unprotected) first
/// byte, then the first byte's low bits and those packet-number octets are XORed
/// with the mask. `pn_offset` is the byte index where the packet number begins;
/// `long_header` selects which first-byte bits are protected.
pub fn apply_header_protection(
    packet: &mut [u8],
    pn_offset: usize,
    hp_key: &[u8],
    long_header: bool,
) -> Result<(), ProtectionError> {
    let sample_offset = sample_bounds(packet.len(), pn_offset)?;
    let mask = aes_128_hp_mask(hp_key, &packet[sample_offset..sample_offset + HP_SAMPLE_LEN])?;

    // On send the first byte is unprotected, so the packet-number length is
    // directly readable from its low 2 bits (RFC 9000 §17.2/§17.3).
    let pn_len = (packet[0] & 0x03) as usize + 1;
    if pn_offset + pn_len > packet.len() {
        return Err(ProtectionError::BufferTooShort);
    }
    packet[0] ^= mask[0] & first_byte_mask(long_header);
    for i in 0..pn_len {
        packet[pn_offset + i] ^= mask[1 + i];
    }
    Ok(())
}

/// Remove header protection from a received packet in place (RFC 9001 §5.4.1,
/// the *receiver* side) and return the recovered packet-number length. The first
/// byte's protected low bits are unmasked first, revealing the packet-number
/// length in its low 2 bits, then that many packet-number octets are unmasked.
/// `pn_offset` is the byte index where the packet number begins; `long_header`
/// selects which first-byte bits are protected.
pub fn remove_header_protection(
    packet: &mut [u8],
    pn_offset: usize,
    hp_key: &[u8],
    long_header: bool,
) -> Result<usize, ProtectionError> {
    let sample_offset = sample_bounds(packet.len(), pn_offset)?;
    let mask = aes_128_hp_mask(hp_key, &packet[sample_offset..sample_offset + HP_SAMPLE_LEN])?;

    // Unmask the first byte first; only then are its low 2 bits the true
    // packet-number length (RFC 9001 §5.4.1).
    packet[0] ^= mask[0] & first_byte_mask(long_header);
    let pn_len = (packet[0] & 0x03) as usize + 1;
    if pn_offset + pn_len > packet.len() {
        return Err(ProtectionError::BufferTooShort);
    }
    for i in 0..pn_len {
        packet[pn_offset + i] ^= mask[1 + i];
    }
    Ok(pn_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a hex string into bytes for comparing against RFC test vectors.
    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    // ── AEAD (§5.3) ─────────────────────────────────────────────────────────

    #[test]
    fn quic_nonce_xors_packet_number_right_aligned() {
        // RFC 9001 A.2: client iv, packet number 2 → last octet flips 5c→5e.
        let iv: [u8; 12] = hex("fa044b2f42a3fd3b46fb255c").try_into().unwrap();
        assert_eq!(quic_nonce(&iv, 2).to_vec(), hex("fa044b2f42a3fd3b46fb255e"));
        // Packet number 0 leaves the iv unchanged.
        assert_eq!(quic_nonce(&iv, 0).to_vec(), iv.to_vec());
        // A large packet number spills into higher nonce octets. 2^32 lands in
        // the fourth-from-last octet: iv[7] 3b flips to 3a.
        assert_eq!(quic_nonce(&iv, 0x0000_0001_0000_0000).to_vec(), hex("fa044b2f42a3fd3a46fb255c"));
    }

    #[test]
    fn aes_128_gcm_matches_mcgrew_viega_test_case() {
        // McGrew & Viega GCM test case 3 (a widely used AES-128-GCM KAT). With a
        // 96-bit nonce and packet number 0, our QUIC nonce equals the iv, so this
        // exercises the whole seal path against a published answer. 64-byte
        // plaintext, empty AAD; expected output is ciphertext || 16-byte tag.
        let key = hex("feffe9928665731c6d6a8f9467308308");
        let iv = hex("cafebabefacedbaddecaf888");
        let plaintext = hex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
             1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255",
        );
        let expected = hex(
            "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e\
             21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091473f5985\
             4d5c2af327cd64a62cf35abd2ba6fab4",
        );
        let sealed = aes_128_gcm_seal(&key, &iv, 0, b"", &plaintext).expect("seal");
        assert_eq!(sealed, expected);
        // And it round-trips back to the plaintext.
        let opened = aes_128_gcm_open(&key, &iv, 0, b"", &sealed).expect("open");
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn aead_round_trip_with_aad_and_nonzero_pn() {
        let key = hex("1f369613dd76d5467730efcbe3b1a22d");
        let iv = hex("fa044b2f42a3fd3b46fb255c");
        let aad = b"c300000001088394c8f03e5157080000449e00000002";
        let plaintext = b"HTTP/3 CRYPTO frame payload";
        let sealed = aes_128_gcm_seal(&key, &iv, 42, aad, plaintext).expect("seal");
        assert_eq!(sealed.len(), plaintext.len() + AEAD_TAG_LEN);
        let opened = aes_128_gcm_open(&key, &iv, 42, aad, &sealed).expect("open");
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn aead_open_rejects_tampered_tag() {
        let key = hex("1f369613dd76d5467730efcbe3b1a22d");
        let iv = hex("fa044b2f42a3fd3b46fb255c");
        let mut sealed = aes_128_gcm_seal(&key, &iv, 7, b"aad", b"secret").expect("seal");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert_eq!(aes_128_gcm_open(&key, &iv, 7, b"aad", &sealed), Err(ProtectionError::AeadFailed));
    }

    #[test]
    fn aead_open_rejects_wrong_aad() {
        let key = hex("1f369613dd76d5467730efcbe3b1a22d");
        let iv = hex("fa044b2f42a3fd3b46fb255c");
        let sealed = aes_128_gcm_seal(&key, &iv, 7, b"header-a", b"secret").expect("seal");
        assert_eq!(aes_128_gcm_open(&key, &iv, 7, b"header-b", &sealed), Err(ProtectionError::AeadFailed));
    }

    #[test]
    fn aead_rejects_bad_key_and_iv_lengths() {
        assert_eq!(aes_128_gcm_seal(&[0u8; 15], &[0u8; 12], 0, b"", b"x"), Err(ProtectionError::BadKeyLength));
        assert_eq!(aes_128_gcm_seal(&[0u8; 16], &[0u8; 11], 0, b"", b"x"), Err(ProtectionError::BadIvLength));
        assert_eq!(aes_128_gcm_open(&[0u8; 16], &[0u8; 12], 0, b"", &[0u8; 4]), Err(ProtectionError::AeadFailed));
    }

    // ── Header protection (§5.4) ────────────────────────────────────────────

    #[test]
    fn hp_mask_matches_rfc9001_client_initial() {
        // RFC 9001 A.2.
        let hp = hex("9f50449e04a0e810283a1e9933adedd2");
        let sample = hex("d1b1c98dd7689fb8ec11d242b123dc9b");
        assert_eq!(aes_128_hp_mask(&hp, &sample).unwrap().to_vec(), hex("437b9aec36"));
    }

    #[test]
    fn hp_mask_matches_rfc9001_server_initial() {
        // RFC 9001 A.3.
        let hp = hex("c206b8d9b9f0f37644430b490eeaa314");
        let sample = hex("2cd0991cd25b0aac406a5816b6394100");
        assert_eq!(aes_128_hp_mask(&hp, &sample).unwrap().to_vec(), hex("2ec0d8356a"));
    }

    #[test]
    fn apply_header_protection_matches_rfc9001_client_initial() {
        // RFC 9001 A.2: first byte c3, 4-byte packet number 00000002, long header.
        // The 16-byte header-protection sample follows the packet-number field.
        let hp = hex("9f50449e04a0e810283a1e9933adedd2");
        let mut packet = vec![0xc3];
        packet.extend_from_slice(&hex("00000002")); // pn_offset = 1, pn_len = 4
        packet.extend_from_slice(&hex("d1b1c98dd7689fb8ec11d242b123dc9b")); // sample at offset 5
        apply_header_protection(&mut packet, 1, &hp, true).expect("apply");
        // RFC A.2 protected first byte c0 and protected packet number 7b9aec34.
        assert_eq!(packet[0], 0xc0);
        assert_eq!(&packet[1..5], hex("7b9aec34").as_slice());
    }

    #[test]
    fn apply_header_protection_matches_rfc9001_server_initial() {
        // RFC 9001 A.3: first byte c1, 2-byte packet number 0001, long header.
        // The sample is always taken 4 bytes past the packet-number start, so for
        // a 2-byte packet number two payload octets precede the 16-byte sample.
        let hp = hex("c206b8d9b9f0f37644430b490eeaa314");
        let mut packet = vec![0xc1];
        packet.extend_from_slice(&hex("0001")); // pn_offset = 1, pn_len = 2
        packet.extend_from_slice(&hex("f00d")); // 2 payload octets before the sample
        packet.extend_from_slice(&hex("2cd0991cd25b0aac406a5816b6394100")); // sample at offset 5
        apply_header_protection(&mut packet, 1, &hp, true).expect("apply");
        // RFC A.3 protected first byte cf and protected packet number c0d9.
        assert_eq!(packet[0], 0xcf);
        assert_eq!(&packet[1..3], hex("c0d9").as_slice());
    }

    #[test]
    fn remove_header_protection_reverses_apply_client() {
        // Removing protection from the A.2 protected header restores it and
        // reports the 4-byte packet-number length.
        let hp = hex("9f50449e04a0e810283a1e9933adedd2");
        let mut packet = vec![0xc0];
        packet.extend_from_slice(&hex("7b9aec34"));
        packet.extend_from_slice(&hex("d1b1c98dd7689fb8ec11d242b123dc9b"));
        let pn_len = remove_header_protection(&mut packet, 1, &hp, true).expect("remove");
        assert_eq!(pn_len, 4);
        assert_eq!(packet[0], 0xc3);
        assert_eq!(&packet[1..5], hex("00000002").as_slice());
    }

    #[test]
    fn remove_header_protection_reverses_apply_server() {
        let hp = hex("c206b8d9b9f0f37644430b490eeaa314");
        let mut packet = vec![0xcf];
        packet.extend_from_slice(&hex("c0d9"));
        packet.extend_from_slice(&hex("f00d")); // 2 payload octets before the sample
        packet.extend_from_slice(&hex("2cd0991cd25b0aac406a5816b6394100"));
        let pn_len = remove_header_protection(&mut packet, 1, &hp, true).expect("remove");
        assert_eq!(pn_len, 2);
        assert_eq!(packet[0], 0xc1);
        assert_eq!(&packet[1..3], hex("0001").as_slice());
    }

    #[test]
    fn header_protection_round_trips_short_header() {
        // Short (1-RTT) headers protect 5 low bits of the first byte, including
        // the key-phase bit. Round-trip a synthetic packet.
        let hp = hex("9f50449e04a0e810283a1e9933adedd2");
        let mut packet = vec![0x42]; // short header, some low bits set, pn_len = 3
        packet.extend_from_slice(&hex("abcdef")); // 3-byte packet number
        packet.push(0x99); // 1 payload octet so the sample lands at pn_offset + 4
        packet.extend_from_slice(&hex("d1b1c98dd7689fb8ec11d242b123dc9b"));
        let original = packet.clone();
        apply_header_protection(&mut packet, 1, &hp, false).expect("apply");
        assert_ne!(packet, original); // protection actually changed the bytes
        let pn_len = remove_header_protection(&mut packet, 1, &hp, false).expect("remove");
        assert_eq!(pn_len, 3);
        assert_eq!(packet, original);
    }

    #[test]
    fn header_protection_rejects_short_buffer() {
        let hp = hex("9f50449e04a0e810283a1e9933adedd2");
        // Sample needs pn_offset + 4 + 16 bytes; this buffer is far too short.
        let mut packet = vec![0xc3, 0x00, 0x00, 0x00, 0x02];
        assert_eq!(
            apply_header_protection(&mut packet, 1, &hp, true),
            Err(ProtectionError::BufferTooShort)
        );
    }

    #[test]
    fn hp_mask_rejects_bad_key_length() {
        let sample = hex("d1b1c98dd7689fb8ec11d242b123dc9b");
        assert_eq!(aes_128_hp_mask(&[0u8; 15], &sample), Err(ProtectionError::BadHpKeyLength));
    }

    // ── End-to-end: seal → header-protect → strip → open ───────────────────

    #[test]
    fn full_packet_protection_round_trip() {
        // Derive real Initial keys, protect a payload, apply header protection,
        // then reverse the whole chain and recover the plaintext — the exact
        // sequence a QUIC endpoint performs (RFC 9001 §5.4.1: header protection is
        // applied after, and removed before, AEAD).
        let keys = super::super::key_schedule::InitialKeys::derive(&hex("8394c8f03e515708"));
        let k = &keys.client;

        // Synthetic unprotected header: first byte (long, pn_len = 4) + connection
        // material + a 4-byte packet number. pn_offset points at the packet number.
        let pn: u64 = 2;
        let mut header = vec![0xc3];
        header.extend_from_slice(&hex("00000001088394c8f03e515708000044")); // arbitrary conn/length bytes
        let pn_offset = header.len();
        header.extend_from_slice(&(pn as u32).to_be_bytes()); // 4-byte packet number

        let payload = b"the quick brown fox jumps over the lazy dog";
        let sealed = aes_128_gcm_seal(&k.key, &k.iv, pn, &header, payload).expect("seal");

        // Assemble the full packet and apply header protection.
        let mut packet = header.clone();
        packet.extend_from_slice(&sealed);
        apply_header_protection(&mut packet, pn_offset, &k.hp, true).expect("apply");

        // Receiver: strip header protection, recover the header (AAD) and payload.
        let mut received = packet.clone();
        let pn_len = remove_header_protection(&mut received, pn_offset, &k.hp, true).expect("remove");
        assert_eq!(pn_len, 4);
        let recovered_header = &received[..pn_offset + pn_len];
        let recovered_payload = &received[pn_offset + pn_len..];
        let opened = aes_128_gcm_open(&k.key, &k.iv, pn, recovered_header, recovered_payload).expect("open");
        assert_eq!(opened.as_slice(), payload.as_slice());
        assert_eq!(recovered_header, header.as_slice());
    }
}
