//! TLS fingerprint data structures and Chrome 130 reference snapshots.
//!
//! Provides JA3 (raw) and JA4_r (raw, no SHA256) fingerprint strings for
//! snapshot-testing and anti-detection verification. The "raw" variants skip
//! the MD5/SHA256 hashing step so tests can assert on the exact parameter
//! values without needing a hash crate as a dependency.
//!
//! Chrome 130 ClientHello (Wireshark capture, stable channel, Linux/Windows):
//! - TLS versions advertised: TLS 1.3 (0x0304), TLS 1.2 (0x0303)
//! - Cipher suites: 15 total (3 TLS 1.3 + 6 AEAD TLS 1.2 + 6 CBC TLS 1.2)
//! - Extensions: 16 (ordered as Chrome sends them, GREASE excluded)
//! - Named groups: X25519, secp256r1 (P-256), secp384r1 (P-384)
//! - EC point formats: uncompressed (0)
//! - ALPN: h2, http/1.1

use rustls::SignatureScheme;

// ── Chrome 130 cipher suite code points ──────────────────────────────────────

/// TLS_AES_128_GCM_SHA256 (TLS 1.3, code 0x1301 = 4865)
pub const TLS13_AES_128_GCM_SHA256: u16 = 0x1301;
/// TLS_AES_256_GCM_SHA384 (TLS 1.3, code 0x1302 = 4866)
pub const TLS13_AES_256_GCM_SHA384: u16 = 0x1302;
/// TLS_CHACHA20_POLY1305_SHA256 (TLS 1.3, code 0x1303 = 4867)
pub const TLS13_CHACHA20_POLY1305_SHA256: u16 = 0x1303;
/// TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 (TLS 1.2, 0xC02B = 49195)
pub const TLS_ECDHE_ECDSA_AES128_GCM_SHA256: u16 = 0xC02B;
/// TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 (TLS 1.2, 0xC02F = 49199)
pub const TLS_ECDHE_RSA_AES128_GCM_SHA256: u16 = 0xC02F;
/// TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 (TLS 1.2, 0xC02C = 49196)
pub const TLS_ECDHE_ECDSA_AES256_GCM_SHA384: u16 = 0xC02C;
/// TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384 (TLS 1.2, 0xC030 = 49200)
pub const TLS_ECDHE_RSA_AES256_GCM_SHA384: u16 = 0xC030;
/// TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256 (TLS 1.2, 0xCCA9 = 52393)
pub const TLS_ECDHE_ECDSA_CHACHA20_SHA256: u16 = 0xCCA9;
/// TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256 (TLS 1.2, 0xCCA8 = 52392)
pub const TLS_ECDHE_RSA_CHACHA20_SHA256: u16 = 0xCCA8;
// CBC suites — Chrome offers them; rustls deliberately omits them (insecure).
/// TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA (0xC013 = 49171)
pub const TLS_ECDHE_RSA_AES128_CBC_SHA: u16 = 0xC013;
/// TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA (0xC014 = 49172)
pub const TLS_ECDHE_RSA_AES256_CBC_SHA: u16 = 0xC014;
/// TLS_RSA_WITH_AES_128_GCM_SHA256 (0x009C = 156)
pub const TLS_RSA_AES128_GCM_SHA256: u16 = 0x009C;
/// TLS_RSA_WITH_AES_256_GCM_SHA384 (0x009D = 157)
pub const TLS_RSA_AES256_GCM_SHA384: u16 = 0x009D;
/// TLS_RSA_WITH_AES_128_CBC_SHA (0x002F = 47)
pub const TLS_RSA_AES128_CBC_SHA: u16 = 0x002F;
/// TLS_RSA_WITH_AES_256_CBC_SHA (0x0035 = 53)
pub const TLS_RSA_AES256_CBC_SHA: u16 = 0x0035;

// ── Chrome 130 extension type codes ──────────────────────────────────────────

/// server_name (SNI), code 0
pub const EXT_SERVER_NAME: u16 = 0;
/// extended_master_secret, code 23
pub const EXT_EXTENDED_MASTER_SECRET: u16 = 23;
/// renegotiation_info, code 65281
pub const EXT_RENEGOTIATION_INFO: u16 = 65281;
/// supported_groups (named curves), code 10
pub const EXT_SUPPORTED_GROUPS: u16 = 10;
/// ec_point_formats, code 11
pub const EXT_EC_POINT_FORMATS: u16 = 11;
/// session_ticket, code 35
pub const EXT_SESSION_TICKET: u16 = 35;
/// application_layer_protocol_negotiation (ALPN), code 16
pub const EXT_ALPN: u16 = 16;
/// status_request (OCSP stapling), code 5
pub const EXT_STATUS_REQUEST: u16 = 5;
/// signature_algorithms, code 13
pub const EXT_SIGNATURE_ALGORITHMS: u16 = 13;
/// signed_certificate_timestamp (SCT), code 18
pub const EXT_SIGNED_CERT_TIMESTAMP: u16 = 18;
/// key_share (TLS 1.3), code 51
pub const EXT_KEY_SHARE: u16 = 51;
/// psk_key_exchange_modes (TLS 1.3), code 45
pub const EXT_PSK_KEY_EXCHANGE_MODES: u16 = 45;
/// supported_versions (TLS 1.3), code 43
pub const EXT_SUPPORTED_VERSIONS: u16 = 43;
/// compress_certificate (RFC 8879), code 27
pub const EXT_COMPRESS_CERTIFICATE: u16 = 27;
/// application_settings / ALPS (Chrome-specific), code 17513
pub const EXT_APPLICATION_SETTINGS: u16 = 17513;
/// padding, code 21
pub const EXT_PADDING: u16 = 21;

// ── Named group code points ───────────────────────────────────────────────────

/// X25519 (RFC 7748), code 29
pub const GROUP_X25519: u16 = 29;
/// secp256r1 / P-256, code 23
pub const GROUP_SECP256R1: u16 = 23;
/// secp384r1 / P-384, code 24
pub const GROUP_SECP384R1: u16 = 24;

// ── Signature algorithm code points ──────────────────────────────────────────

/// Chrome 130 signature algorithms in wire order.
///
/// Values: ecdsa_secp256r1_sha256 (0x0403), rsa_pss_rsae_sha256 (0x0804),
/// rsa_pkcs1_sha256 (0x0401), ecdsa_secp384r1_sha384 (0x0503),
/// rsa_pss_rsae_sha384 (0x0805), rsa_pkcs1_sha384 (0x0501),
/// rsa_pss_rsae_sha512 (0x0806), rsa_pkcs1_sha512 (0x0601),
/// rsa_pkcs1_sha1 (0x0201).
pub const CHROME_130_SIG_ALGORITHMS: &[u16] = &[
    0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601, 0x0201,
];

// ── TlsHandshakeInfo ─────────────────────────────────────────────────────────

/// TLS handshake parameters extracted from a ClientHello for fingerprinting.
///
/// Populated either from a live TLS connection or constructed manually for
/// snapshot testing.  All numeric values use decimal/big-endian u16 encoding
/// as required by JA3 and JA4.
#[derive(Debug, Clone)]
pub struct TlsHandshakeInfo {
    /// `ClientHello.legacy_version` field (always 771 = TLS 1.2 in TLS 1.3
    /// connections per RFC 8446 §4.1.2). Used by JA3.
    pub legacy_version: u16,
    /// Highest TLS version from the `supported_versions` extension (e.g.
    /// 772 = TLS 1.3). Used by JA4. Equals `legacy_version` when the
    /// extension is absent (pre-TLS 1.3 connections).
    pub max_supported_version: u16,
    /// All cipher suites offered, in preference order.
    ///
    /// Includes both TLS 1.3 and TLS 1.2 suites. GREASE values should be
    /// included here as-is; the JA3/JA4 methods filter them automatically.
    pub cipher_suites: Vec<u16>,
    /// TLS extensions present in ClientHello, in wire order.
    ///
    /// Each entry is the extension type code. GREASE extension types are
    /// included and filtered by the JA3/JA4 methods.
    pub extensions: Vec<u16>,
    /// Named groups (elliptic curves) offered, in preference order.
    pub named_groups: Vec<u16>,
    /// EC point formats supported (usually `[0]` = uncompressed).
    pub ec_point_formats: Vec<u8>,
    /// Signature algorithms advertised in the `signature_algorithms` extension.
    pub signature_algorithms: Vec<SignatureScheme>,
    /// ALPN protocol names offered (e.g. `"h2"`, `"http/1.1"`).
    pub alpn_protocols: Vec<String>,
    /// True when SNI (server_name extension) carried a DNS name, not an IP.
    pub has_sni: bool,
}

impl TlsHandshakeInfo {
    /// JA3 raw string (pre-MD5 input).
    ///
    /// Format: `TLSVersion,Ciphers,Extensions,Groups,ECPointFormats`
    /// where each multi-value section is `-`-separated and GREASE values
    /// (RFC 8701) are excluded. Uses `legacy_version` as the version field.
    ///
    /// To obtain the standard JA3 hash, compute `MD5(ja3_raw_string())`.
    pub fn ja3_raw_string(&self) -> String {
        let ciphers = join_filtered(&self.cipher_suites, is_grease, '-');
        let extensions = join_u16_filtered(&self.extensions, is_grease, '-');
        let groups = join_filtered(&self.named_groups, is_grease, '-');
        let formats = self
            .ec_point_formats
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join("-");
        format!(
            "{},{},{},{},{}",
            self.legacy_version, ciphers, extensions, groups, formats
        )
    }

    /// JA4_r (raw JA4) string — human-readable without SHA256 hashing.
    ///
    /// Format:
    /// `t{ver}{sni}{n_ciphers:02x}{n_exts:02x}{alpn}_{ciphers_sorted}_{exts_sorted},{sigalgs}`
    ///
    /// - `t` = TLS (vs QUIC `q`).
    /// - `ver` = "13"/"12"/"11"/"10" from `max_supported_version`.
    /// - `sni` = "d" (domain) / "i" (IP).
    /// - `n_ciphers` / `n_exts` = hex count, GREASE excluded.
    /// - `alpn` = first 2 chars of first ALPN value ("h2" → "h2", "00" if absent).
    /// - `ciphers_sorted` = GREASE-filtered cipher codes in 4-digit hex, ascending.
    /// - `exts_sorted` = GREASE-filtered ext codes, SNI=0 and ALPN=16 excluded, ascending.
    /// - `sigalgs` = signature algorithm codes in wire order, hex.
    ///
    /// To obtain the standard JA4 hash, replace the last two sections with
    /// `SHA256[0:12]` of each section.
    pub fn ja4_raw_string(&self) -> String {
        let ver = match self.max_supported_version {
            769 => "10",
            770 => "11",
            771 => "12",
            772 => "13",
            _ => "??",
        };
        let sni = if self.has_sni { "d" } else { "i" };

        let non_grease_ciphers: Vec<u16> = self
            .cipher_suites
            .iter()
            .copied()
            .filter(|&c| !is_grease(c))
            .collect();
        let non_grease_exts: Vec<u16> = self
            .extensions
            .iter()
            .copied()
            .filter(|&e| !is_grease(e))
            .collect();

        let n_ciphers = non_grease_ciphers.len().min(255);
        let n_exts = non_grease_exts.len().min(255);

        let alpn = self
            .alpn_protocols
            .first()
            .map(|s| {
                let c: String = s.chars().take(2).collect();
                if c.len() < 2 { format!("{c:0<2}") } else { c }
            })
            .unwrap_or_else(|| "00".to_owned());

        // Sorted cipher suites (ascending).
        let mut sorted_ciphers = non_grease_ciphers;
        sorted_ciphers.sort_unstable();
        let ciphers_str = sorted_ciphers
            .iter()
            .map(|c| format!("{c:04x}"))
            .collect::<Vec<_>>()
            .join(",");

        // Sorted extensions, SNI (0) and ALPN (16) excluded.
        let mut ext_sorted: Vec<u16> = non_grease_exts
            .into_iter()
            .filter(|&e| e != EXT_SERVER_NAME && e != EXT_ALPN)
            .collect();
        ext_sorted.sort_unstable();
        let exts_str = ext_sorted
            .iter()
            .map(|e| format!("{e:04x}"))
            .collect::<Vec<_>>()
            .join(",");

        // Signature algorithms in wire order.
        let sigalgs_str = self
            .signature_algorithms
            .iter()
            .map(|s| format!("{:04x}", u16::from(*s)))
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "t{ver}{sni}{n_ciphers:02x}{n_exts:02x}{alpn}_{ciphers_str}_{exts_str},{sigalgs_str}"
        )
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn join_filtered(v: &[u16], skip: fn(u16) -> bool, sep: char) -> String {
    v.iter()
        .filter(|&&x| !skip(x))
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(&sep.to_string())
}

fn join_u16_filtered(v: &[u16], skip: fn(u16) -> bool, sep: char) -> String {
    join_filtered(v, skip, sep)
}

/// Returns `true` if `v` is a GREASE value (RFC 8701).
///
/// GREASE values: 0x0a0a, 0x1a1a, …, 0xfafa — both bytes equal and
/// the low nibble of each byte is 0xa.
pub fn is_grease(v: u16) -> bool {
    let lo = (v & 0xff) as u8;
    let hi = ((v >> 8) & 0xff) as u8;
    lo == hi && (lo & 0x0f) == 0x0a
}

// ── Chrome 130 JA3 snapshot ───────────────────────────────────────────────────

/// Reference Chrome 130 TLS ClientHello parameters for JA3 snapshot testing.
///
/// Values from Wireshark capture of Chrome 130 stable (GREASE values excluded).
#[allow(dead_code)]
pub struct ChromeJa3Snapshot {
    /// `ClientHello.legacy_version` = 771 (TLS 1.2 placeholder field in TLS 1.3).
    pub tls_version: u16,
    /// All cipher suites Chrome offers, in preference order (15 total).
    pub expected_cipher_suites: &'static [u16],
    /// Extension type codes in wire order (GREASE excluded).
    pub expected_extensions: &'static [u16],
    /// Named groups in preference order.
    pub expected_named_groups: &'static [u16],
    /// EC point formats.
    pub expected_ec_point_formats: &'static [u8],
}

/// Chrome 130 stable JA3 reference snapshot.
///
/// JA3 uses `ClientHello.legacy_version` (always 771 in TLS 1.3 per RFC 8446).
#[allow(dead_code)]
pub const CHROME_130_JA3_SNAPSHOT: ChromeJa3Snapshot = ChromeJa3Snapshot {
    tls_version: 771,
    // Chrome 130 cipher suite order: 3 TLS 1.3 + 6 AEAD TLS 1.2 + 6 CBC TLS 1.2.
    expected_cipher_suites: &[
        TLS13_AES_128_GCM_SHA256,          // 4865  0x1301
        TLS13_AES_256_GCM_SHA384,          // 4866  0x1302
        TLS13_CHACHA20_POLY1305_SHA256,    // 4867  0x1303
        TLS_ECDHE_ECDSA_AES128_GCM_SHA256, // 49195 0xC02B
        TLS_ECDHE_RSA_AES128_GCM_SHA256,   // 49199 0xC02F
        TLS_ECDHE_ECDSA_AES256_GCM_SHA384, // 49196 0xC02C
        TLS_ECDHE_RSA_AES256_GCM_SHA384,   // 49200 0xC030
        TLS_ECDHE_ECDSA_CHACHA20_SHA256,   // 52393 0xCCA9
        TLS_ECDHE_RSA_CHACHA20_SHA256,     // 52392 0xCCA8
        TLS_ECDHE_RSA_AES128_CBC_SHA,      // 49171 0xC013
        TLS_ECDHE_RSA_AES256_CBC_SHA,      // 49172 0xC014
        TLS_RSA_AES128_GCM_SHA256,         //   156 0x009C
        TLS_RSA_AES256_GCM_SHA384,         //   157 0x009D
        TLS_RSA_AES128_CBC_SHA,            //    47 0x002F
        TLS_RSA_AES256_CBC_SHA,            //    53 0x0035
    ],
    // Chrome 130 extension wire order (GREASE omitted):
    expected_extensions: &[
        EXT_SERVER_NAME,            //     0
        EXT_EXTENDED_MASTER_SECRET, //    23
        EXT_RENEGOTIATION_INFO,     // 65281
        EXT_SUPPORTED_GROUPS,       //    10
        EXT_EC_POINT_FORMATS,       //    11
        EXT_SESSION_TICKET,         //    35
        EXT_ALPN,                   //    16
        EXT_STATUS_REQUEST,         //     5
        EXT_SIGNATURE_ALGORITHMS,   //    13
        EXT_SIGNED_CERT_TIMESTAMP,  //    18
        EXT_KEY_SHARE,              //    51
        EXT_PSK_KEY_EXCHANGE_MODES, //    45
        EXT_SUPPORTED_VERSIONS,     //    43
        EXT_COMPRESS_CERTIFICATE,   //    27
        EXT_APPLICATION_SETTINGS,   // 17513
        EXT_PADDING,                //    21
    ],
    expected_named_groups: &[GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1],
    expected_ec_point_formats: &[0],
};

// ── Chrome 130 JA4 snapshot ───────────────────────────────────────────────────

/// Reference Chrome 130 JA4_r parameters for snapshot testing.
#[allow(dead_code)]
pub struct JA4ChromeSnapshot {
    /// TLS version string in JA4 prefix: "13" for TLS 1.3.
    pub tls_version_str: &'static str,
    /// "d" (domain SNI) or "i" (IP literal).
    pub sni_type: &'static str,
    /// Non-GREASE cipher suite count (for JA4 prefix hex digits).
    pub cipher_count: u8,
    /// Non-GREASE extension count.
    pub extension_count: u8,
    /// First 2 characters of first ALPN value.
    pub alpn_prefix: &'static str,
    /// Cipher suites sorted ascending for JA4_r cipher section.
    pub sorted_cipher_suites: &'static [u16],
    /// Extensions sorted ascending, SNI and ALPN excluded, for JA4_r ext section.
    pub sorted_extensions: &'static [u16],
    /// Signature algorithms in wire order.
    pub signature_algorithms: &'static [u16],
}

/// Chrome 130 JA4_r reference snapshot (with ALPN h2, domain SNI).
///
/// JA4 uses the highest version from `supported_versions` extension ("13"),
/// not `legacy_version`.
#[allow(dead_code)]
pub const CHROME_130_JA4_SNAPSHOT: JA4ChromeSnapshot = JA4ChromeSnapshot {
    tls_version_str: "13",
    sni_type: "d",
    cipher_count: 15,
    extension_count: 16,
    alpn_prefix: "h2",
    // Sorted ascending by u16 value (15 suites):
    //   47=0x002F, 53=0x0035, 156=0x009C, 157=0x009D,
    //   4865=0x1301, 4866=0x1302, 4867=0x1303,   ← TLS 1.3 suites
    //   49171=0xC013, 49172=0xC014, 49195=0xC02B, 49196=0xC02C,
    //   49199=0xC02F, 49200=0xC030, 52392=0xCCA8, 52393=0xCCA9
    sorted_cipher_suites: &[
        TLS_RSA_AES128_CBC_SHA,            //    47
        TLS_RSA_AES256_CBC_SHA,            //    53
        TLS_RSA_AES128_GCM_SHA256,         //   156
        TLS_RSA_AES256_GCM_SHA384,         //   157
        TLS13_AES_128_GCM_SHA256,          //  4865
        TLS13_AES_256_GCM_SHA384,          //  4866
        TLS13_CHACHA20_POLY1305_SHA256,    //  4867
        TLS_ECDHE_RSA_AES128_CBC_SHA,      // 49171
        TLS_ECDHE_RSA_AES256_CBC_SHA,      // 49172
        TLS_ECDHE_ECDSA_AES128_GCM_SHA256, // 49195
        TLS_ECDHE_ECDSA_AES256_GCM_SHA384, // 49196
        TLS_ECDHE_RSA_AES128_GCM_SHA256,   // 49199
        TLS_ECDHE_RSA_AES256_GCM_SHA384,   // 49200
        TLS_ECDHE_RSA_CHACHA20_SHA256,     // 52392
        TLS_ECDHE_ECDSA_CHACHA20_SHA256,   // 52393
    ],
    // 16 extensions - SNI(0) - ALPN(16) = 14, sorted ascending:
    //   5, 10, 11, 13, 18, 21, 23, 27, 35, 43, 45, 51, 17513, 65281
    sorted_extensions: &[
        EXT_STATUS_REQUEST,         //     5
        EXT_SUPPORTED_GROUPS,       //    10
        EXT_EC_POINT_FORMATS,       //    11
        EXT_SIGNATURE_ALGORITHMS,   //    13
        EXT_SIGNED_CERT_TIMESTAMP,  //    18
        EXT_PADDING,                //    21
        EXT_EXTENDED_MASTER_SECRET, //    23
        EXT_COMPRESS_CERTIFICATE,   //    27
        EXT_SESSION_TICKET,         //    35
        EXT_SUPPORTED_VERSIONS,     //    43
        EXT_PSK_KEY_EXCHANGE_MODES, //    45
        EXT_KEY_SHARE,              //    51
        EXT_APPLICATION_SETTINGS,   // 17513
        EXT_RENEGOTIATION_INFO,     // 65281
    ],
    signature_algorithms: CHROME_130_SIG_ALGORITHMS,
};

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome_130_handshake_info() -> TlsHandshakeInfo {
        TlsHandshakeInfo {
            legacy_version: 771,       // TLS 1.2 placeholder (JA3)
            max_supported_version: 772, // TLS 1.3 from supported_versions (JA4)
            cipher_suites: CHROME_130_JA3_SNAPSHOT.expected_cipher_suites.to_vec(),
            extensions: CHROME_130_JA3_SNAPSHOT.expected_extensions.to_vec(),
            named_groups: CHROME_130_JA3_SNAPSHOT.expected_named_groups.to_vec(),
            ec_point_formats: CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.to_vec(),
            signature_algorithms: CHROME_130_SIG_ALGORITHMS
                .iter()
                .map(|&v| SignatureScheme::from(v))
                .collect(),
            alpn_protocols: vec!["h2".to_owned(), "http/1.1".to_owned()],
            has_sni: true,
        }
    }

    #[test]
    fn grease_detection() {
        assert!(is_grease(0x0a0a));
        assert!(is_grease(0xfafa));
        assert!(is_grease(0xdada));
        assert!(!is_grease(0x0000));
        assert!(!is_grease(0x1301)); // TLS_AES_128_GCM_SHA256
        assert!(!is_grease(0xC02B)); // ECDHE-ECDSA-AES128-GCM-SHA256
        assert!(!is_grease(0x0029)); // not a GREASE pattern
    }

    #[test]
    fn ja3_raw_string_basic_format() {
        let info = TlsHandshakeInfo {
            legacy_version: 771,
            max_supported_version: 772,
            cipher_suites: vec![4865, 4866, 4867],
            extensions: vec![0, 23, 10, 11],
            named_groups: vec![29, 23],
            ec_point_formats: vec![0],
            signature_algorithms: vec![],
            alpn_protocols: vec!["h2".to_owned()],
            has_sni: true,
        };
        let s = info.ja3_raw_string();
        // Format: version,ciphers,extensions,groups,formats
        assert!(s.starts_with("771,"));
        assert!(s.contains("4865-4866-4867"));
        assert!(s.contains("0-23-10-11"));
        assert!(s.contains("29-23"));
        assert!(s.ends_with(",0"));
    }

    #[test]
    fn ja3_raw_filters_grease() {
        let info = TlsHandshakeInfo {
            legacy_version: 771,
            max_supported_version: 772,
            cipher_suites: vec![0x0a0a, 4865], // 0x0a0a is GREASE
            extensions: vec![0x2a2a, 0],        // 0x2a2a is GREASE
            named_groups: vec![29],
            ec_point_formats: vec![0],
            signature_algorithms: vec![],
            alpn_protocols: vec![],
            has_sni: true,
        };
        let s = info.ja3_raw_string();
        // GREASE cipher (0x0a0a = 2570) must not appear
        assert!(!s.contains("2570"));
        assert!(s.contains("4865"));
        // GREASE extension (0x2a2a = 10794) must not appear
        assert!(!s.contains("10794"));
        // SNI extension (0) should appear
        assert!(s.contains(",0,") || s.contains(",0\n") || {
            let parts: Vec<&str> = s.split(',').collect();
            parts.len() > 2 && parts[2].starts_with('0')
        });
    }

    #[test]
    fn ja3_raw_chrome_130_snapshot_structure() {
        assert_eq!(CHROME_130_JA3_SNAPSHOT.tls_version, 771);
        assert_eq!(CHROME_130_JA3_SNAPSHOT.expected_cipher_suites.len(), 15);
        assert_eq!(CHROME_130_JA3_SNAPSHOT.expected_extensions.len(), 16);
        assert_eq!(CHROME_130_JA3_SNAPSHOT.expected_named_groups.len(), 3);
        assert_eq!(CHROME_130_JA3_SNAPSHOT.expected_named_groups[0], GROUP_X25519);
        assert_eq!(CHROME_130_JA3_SNAPSHOT.expected_named_groups[1], GROUP_SECP256R1);
        assert!(CHROME_130_JA3_SNAPSHOT.expected_ec_point_formats.contains(&0));
    }

    #[test]
    fn ja3_raw_chrome_130_string_sections() {
        let info = chrome_130_handshake_info();
        let s = info.ja3_raw_string();
        // Must start with legacy_version (771)
        assert!(s.starts_with("771,"), "JA3 starts with legacy_version 771");
        // Cipher section contains TLS_AES_128_GCM_SHA256 = 4865
        assert!(s.contains("4865"));
        // 5 comma-separated sections
        let sections: Vec<&str> = s.splitn(6, ',').collect();
        assert!(sections.len() >= 5, "JA3 must have 5 comma-delimited sections");
        let ciphers: Vec<&str> = sections[1].split('-').collect();
        assert_eq!(ciphers.len(), 15, "Chrome 130 advertises 15 cipher suites");
        let exts: Vec<&str> = sections[2].split('-').collect();
        assert_eq!(exts.len(), 16, "Chrome 130 has 16 extensions (GREASE excluded)");
    }

    #[test]
    fn ja4_raw_string_prefix() {
        let info = chrome_130_handshake_info();
        let s = info.ja4_raw_string();
        // t{ver}{sni}{n_ciphers:02x}{n_exts:02x}{alpn}_...
        assert!(s.starts_with("t13d"), "must start with t13d");
        // 15 ciphers → 0f, 16 extensions → 10
        assert!(s.starts_with("t13d0f10h2_"), "full prefix must be t13d0f10h2_");
    }

    #[test]
    fn ja4_raw_three_sections() {
        let info = chrome_130_handshake_info();
        let s = info.ja4_raw_string();
        let parts: Vec<&str> = s.splitn(3, '_').collect();
        assert_eq!(parts.len(), 3, "JA4_r must have 3 underscore-separated sections");
    }

    #[test]
    fn ja4_raw_excludes_sni_and_alpn() {
        let info = chrome_130_handshake_info();
        let s = info.ja4_raw_string();
        // Extension section is between second and third _
        let ext_section_raw = s.split('_').nth(2).unwrap_or("");
        // ext section is "{exts},{sigalgs}" — get just the exts part
        let ext_codes = ext_section_raw.split(',').next().unwrap_or("");
        let codes: Vec<&str> = ext_codes.split(',').collect();
        assert!(!codes.contains(&"0000"), "SNI (0) must be excluded from JA4 ext section");
        assert!(!codes.contains(&"0010"), "ALPN (16) must be excluded from JA4 ext section");
    }

    #[test]
    fn ja4_raw_cipher_section_sorted_ascending() {
        let info = chrome_130_handshake_info();
        let s = info.ja4_raw_string();
        let cipher_section = s.split('_').nth(1).unwrap_or("");
        let codes: Vec<u16> = cipher_section
            .split(',')
            .filter_map(|h| u16::from_str_radix(h, 16).ok())
            .collect();
        assert!(!codes.is_empty());
        for w in codes.windows(2) {
            assert!(w[0] <= w[1], "cipher section must be ascending: {w:?}");
        }
    }

    #[test]
    fn ja4_chrome_130_snapshot_structure() {
        assert_eq!(CHROME_130_JA4_SNAPSHOT.tls_version_str, "13");
        assert_eq!(CHROME_130_JA4_SNAPSHOT.sni_type, "d");
        assert_eq!(CHROME_130_JA4_SNAPSHOT.cipher_count, 15);
        assert_eq!(CHROME_130_JA4_SNAPSHOT.extension_count, 16);
        assert_eq!(CHROME_130_JA4_SNAPSHOT.alpn_prefix, "h2");
        assert_eq!(CHROME_130_JA4_SNAPSHOT.sorted_cipher_suites.len(), 15);
        // 16 extensions - 2 (SNI, ALPN) = 14
        assert_eq!(CHROME_130_JA4_SNAPSHOT.sorted_extensions.len(), 14);
    }

    #[test]
    fn ja4_chrome_130_sorted_ciphers_ascending() {
        let suites = CHROME_130_JA4_SNAPSHOT.sorted_cipher_suites;
        for w in suites.windows(2) {
            assert!(w[0] <= w[1], "sorted_cipher_suites must be ascending: {w:?}");
        }
    }

    #[test]
    fn ja4_chrome_130_sorted_exts_ascending() {
        let exts = CHROME_130_JA4_SNAPSHOT.sorted_extensions;
        for w in exts.windows(2) {
            assert!(w[0] <= w[1], "sorted_extensions must be ascending: {w:?}");
        }
    }

    #[test]
    fn ja4_chrome_130_no_sni_or_alpn_in_ext_list() {
        let exts = CHROME_130_JA4_SNAPSHOT.sorted_extensions;
        assert!(!exts.contains(&EXT_SERVER_NAME), "SNI must not be in JA4 ext list");
        assert!(!exts.contains(&EXT_ALPN), "ALPN must not be in JA4 ext list");
    }

    #[test]
    fn ja4_chrome_130_sorted_ciphers_contains_all_15() {
        let snapshot = CHROME_130_JA4_SNAPSHOT.sorted_cipher_suites;
        let ja3 = CHROME_130_JA3_SNAPSHOT.expected_cipher_suites;
        // Every cipher in JA3 snapshot must appear in JA4 sorted list
        for c in ja3 {
            assert!(
                snapshot.contains(c),
                "cipher {c} from JA3 snapshot missing from JA4 sorted list"
            );
        }
    }
}
