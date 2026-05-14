//! Криптографические хэш-функции.
//!
//! SHA-256 (FIPS 180-4) — общий примитив для нескольких подсистем:
//! HTTP Digest auth (`lumen-network::auth`), Safe Browsing hash lookup
//! (`lumen-storage::safe_browsing`), SRI validation (`lumen-core::sri`).
//!
//! **Это не security-критичный crypto в смысле KDF / шифрования / подписей.**
//! Для тех применений (TLS handshake, X.509 verification) используется
//! rustls (exception #3 в политике зависимостей §5). SHA-256 здесь — это
//! «hash для idempotent lookup-а», где функцию определяет протокольная
//! спецификация (RFC 7616 §3.4.3 Digest auth, Safe Browsing v4 §4.4,
//! W3C SRI §3.5), а не наша свобода выбора. Свой код по тем же причинам,
//! что и другие decoder-ы / parsers: «default — своё».
//!
//! MD5 (RFC 1321) живёт в `lumen-network::auth` локально — он нужен только
//! для HTTP Digest legacy и устаревшее crypto-primitive, не имеет смысла
//! вывозить наружу.

/// SHA-256 хеш произвольных байт по FIPS 180-4.
///
/// Возвращает 32-байтовый digest. Реализация — straightforward translation
/// FIPS 180-4 §6.2: padding до 56 (mod 64) с финальным 64-bit length
/// big-endian, восемь 32-битных working variables (a-h), 64 round-функции
/// с константами K[0..63] и расписанием W[0..63].
///
/// Не constant-time — для secret-data hashing (KDF, MAC) использовать
/// rustls (exception #3). Для idempotent-lookup-ов (Digest auth response,
/// Safe Browsing prefix) timing attacks неактуальны.
#[must_use]
pub fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for j in 0..16 {
            w[j] = u32::from_be_bytes([
                chunk[j * 4],
                chunk[j * 4 + 1],
                chunk[j * 4 + 2],
                chunk[j * 4 + 3],
            ]);
        }
        for j in 16..64 {
            let s0 = w[j - 15].rotate_right(7) ^ w[j - 15].rotate_right(18) ^ (w[j - 15] >> 3);
            let s1 = w[j - 2].rotate_right(17) ^ w[j - 2].rotate_right(19) ^ (w[j - 2] >> 10);
            w[j] = w[j - 16]
                .wrapping_add(s0)
                .wrapping_add(w[j - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for j in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[j])
                .wrapping_add(w[j]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Закодировать байты в lowercase hex (без префиксов, без separator-ов).
/// Длина результата ровно `bytes.len() * 2`.
#[must_use]
pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

/// `hex_lower(&sha256(input))` — самая частая комбинация (HTTP Digest auth,
/// SRI integrity-string).
#[must_use]
pub fn sha256_hex(input: &[u8]) -> String {
    hex_lower(&sha256(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_empty_string_fips_180_4() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc_fips_180_4() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_56_byte_two_block_fips_180_4() {
        // 56 байт ровно — padding не помещается в первый блок, нужен второй.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_returns_32_bytes() {
        assert_eq!(sha256(b"anything").len(), 32);
    }

    #[test]
    fn hex_lower_padding() {
        // Маленькие байты обязаны давать ведущий ноль.
        assert_eq!(hex_lower(&[0x00, 0x01, 0x0F, 0xA0, 0xFF]), "00010fa0ff");
    }

    #[test]
    fn hex_lower_empty_input() {
        assert_eq!(hex_lower(&[]), "");
    }
}
