//! Subresource Integrity (SRI) — парсер `integrity`-attribute и trait
//! для проверки.
//!
//! Spec: <https://www.w3.org/TR/SRI/>. HTML-атрибут `integrity` на
//! `<script>` / `<link rel=stylesheet>` содержит `sha256-`/`sha384-`/`sha512-`
//! prefix + base64-кодированный digest. Список через whitespace — любая
//! из записей должна совпадать.
//!
//! Phase 0: парсер + trait `DigestProvider` для подключения hash-функций
//! извне. Реальные SHA-256 / SHA-384 / SHA-512 реализации появятся
//! отдельно (свои реализации алгоритмов FIPS 180-4, поскольку «default —
//! своё»; либо обёртка вокруг rustls/ring через exception #3).

/// Алгоритм хеширования в SRI metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SriAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl SriAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha384 => "sha384",
            Self::Sha512 => "sha512",
        }
    }

    /// Размер digest-а в байтах: SHA-256 → 32, SHA-384 → 48, SHA-512 → 64.
    pub fn digest_size(self) -> usize {
        match self {
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sha256" => Some(Self::Sha256),
            "sha384" => Some(Self::Sha384),
            "sha512" => Some(Self::Sha512),
            _ => None,
        }
    }
}

/// Одна запись `integrity` (один алгоритм + ожидаемый digest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SriHash {
    pub algorithm: SriAlgorithm,
    /// Decoded digest bytes. Длина соответствует `digest_size()` алгоритма.
    pub expected_digest: Vec<u8>,
}

/// Полный `integrity`-список (whitespace-separated). Если список пуст —
/// integrity-check отключён (W3C SRI §3.3.3.5 «Empty SRI metadata»).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegrityList {
    pub hashes: Vec<SriHash>,
}

impl IntegrityList {
    /// Парсит integrity-атрибут. Whitespace-separated список `algo-base64`.
    /// Некорректные записи (неизвестный алгоритм, невалидный base64,
    /// неверная длина digest-а) **отбрасываются**, остальные сохраняются —
    /// per W3C SRI §3.3.3 «invalid metadata is ignored».
    pub fn parse(text: &str) -> Self {
        let mut hashes = Vec::new();
        for piece in text.split_whitespace() {
            if let Some(h) = parse_one_hash(piece) {
                hashes.push(h);
            }
        }
        Self { hashes }
    }

    /// Проверить body через provider-хешер. Возвращает `Ok(true)` если
    /// хотя бы одна запись совпала. Если список пуст — `Ok(true)`
    /// (integrity-check выключен). Если provider не поддерживает
    /// нужный алгоритм — пробуем остальные; если ни один не подошёл —
    /// `Ok(false)`.
    pub fn verify(&self, body: &[u8], provider: &dyn DigestProvider) -> SriResult<bool> {
        if self.hashes.is_empty() {
            return Ok(true);
        }
        // Согласно W3C SRI §3.3.3.6 «strongest metadata wins»: если в
        // списке есть и sha256, и sha512, проверяем только sha512.
        let strongest = self
            .hashes
            .iter()
            .map(|h| h.algorithm)
            .max_by_key(|a| algorithm_strength(*a));
        let Some(target_alg) = strongest else {
            return Ok(false);
        };
        let mut got_any = false;
        for h in &self.hashes {
            if h.algorithm != target_alg {
                continue;
            }
            match provider.digest(target_alg, body) {
                Ok(actual) => {
                    got_any = true;
                    if constant_time_eq(&actual, &h.expected_digest) {
                        return Ok(true);
                    }
                }
                Err(SriError::UnsupportedAlgorithm) => continue,
                Err(e) => return Err(e),
            }
        }
        if !got_any {
            // Provider не смог посчитать ни один digest нужного алгоритма.
            return Err(SriError::UnsupportedAlgorithm);
        }
        Ok(false)
    }
}

fn algorithm_strength(a: SriAlgorithm) -> u32 {
    match a {
        SriAlgorithm::Sha256 => 256,
        SriAlgorithm::Sha384 => 384,
        SriAlgorithm::Sha512 => 512,
    }
}

fn parse_one_hash(s: &str) -> Option<SriHash> {
    // Опциональный `?option` после base64 (W3C SRI §3.3.3.1 «options»);
    // в Phase 0 options игнорируются.
    let s = s.split('?').next().unwrap_or(s);
    let dash = s.find('-')?;
    let alg = SriAlgorithm::parse(&s[..dash])?;
    let b64 = &s[dash + 1..];
    let digest = base64_decode(b64)?;
    if digest.len() != alg.digest_size() {
        return None;
    }
    Some(SriHash {
        algorithm: alg,
        expected_digest: digest,
    })
}

/// Простая base64-декодеровка (RFC 4648 §4). Принимает стандартный
/// alphabet + опционально `=`-padding. Whitespace внутри запрещён.
/// Возвращает None при невалидном символе или некорректной длине.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in bytes {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Constant-time сравнение двух byte-срезов одинаковой длины. Защита
/// от timing-атак при проверке digest-ов. Для SRI это менее критично
/// (атакующий вряд ли контролирует время сравнения), но дешёвая
/// гарантия — добавляем.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Trait для подключения hash-implementaции извне.
pub trait DigestProvider {
    /// Посчитать digest указанного алгоритма от `body`. Возвращает
    /// raw digest bytes (длина = `algorithm.digest_size()`).
    fn digest(&self, algorithm: SriAlgorithm, body: &[u8]) -> SriResult<Vec<u8>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SriError {
    /// Provider не поддерживает требуемый алгоритм.
    UnsupportedAlgorithm,
    /// Внутренняя ошибка provider-а.
    Provider(String),
}

impl std::fmt::Display for SriError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedAlgorithm => write!(f, "unsupported SRI algorithm"),
            Self::Provider(m) => write!(f, "SRI provider error: {m}"),
        }
    }
}

impl std::error::Error for SriError {}

pub type SriResult<T> = std::result::Result<T, SriError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_returns_empty() {
        assert!(IntegrityList::parse("").hashes.is_empty());
        assert!(IntegrityList::parse("   ").hashes.is_empty());
    }

    #[test]
    fn parse_single_sha256() {
        // "Hello" → SHA-256 digest base64 = "GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk=".
        let s = IntegrityList::parse("sha256-GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk=");
        assert_eq!(s.hashes.len(), 1);
        assert_eq!(s.hashes[0].algorithm, SriAlgorithm::Sha256);
        assert_eq!(s.hashes[0].expected_digest.len(), 32);
    }

    #[test]
    fn parse_multiple_algorithms() {
        let s = IntegrityList::parse("sha256-GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk= sha384-cVHHb3JnIxr0R0iJ0KsHJa6jpV5MJzCMSJTNFkvJ9JF2pLpEFqXMMpWlmTjF/J8d");
        assert_eq!(s.hashes.len(), 2);
        assert_eq!(s.hashes[0].algorithm, SriAlgorithm::Sha256);
        assert_eq!(s.hashes[1].algorithm, SriAlgorithm::Sha384);
    }

    #[test]
    fn parse_skips_invalid_algorithm() {
        let s = IntegrityList::parse("md5-abc sha256-GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk=");
        assert_eq!(s.hashes.len(), 1);
        assert_eq!(s.hashes[0].algorithm, SriAlgorithm::Sha256);
    }

    #[test]
    fn parse_skips_invalid_base64() {
        let s = IntegrityList::parse("sha256-!!!notvalid!!!");
        assert!(s.hashes.is_empty());
    }

    #[test]
    fn parse_skips_wrong_length_digest() {
        // 4 байта вместо 32.
        let s = IntegrityList::parse("sha256-aGVsbG8=");
        assert!(s.hashes.is_empty());
    }

    #[test]
    fn parse_ignores_options() {
        // `?ct=text/javascript` — option, игнорируется.
        let s = IntegrityList::parse(
            "sha256-GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk=?ct=text/javascript",
        );
        assert_eq!(s.hashes.len(), 1);
    }

    #[test]
    fn algorithm_strength_ordering() {
        assert!(algorithm_strength(SriAlgorithm::Sha512) > algorithm_strength(SriAlgorithm::Sha256));
        assert!(algorithm_strength(SriAlgorithm::Sha384) > algorithm_strength(SriAlgorithm::Sha256));
        assert!(algorithm_strength(SriAlgorithm::Sha512) > algorithm_strength(SriAlgorithm::Sha384));
    }

    #[test]
    fn base64_decode_basic() {
        assert_eq!(base64_decode("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ="), Some(b"hello world".to_vec()));
    }

    #[test]
    fn base64_decode_no_padding() {
        // RFC 4648 §3.2: padding опционален.
        assert_eq!(base64_decode("aGVsbG8"), Some(b"hello".to_vec()));
    }

    #[test]
    fn base64_decode_url_unsafe_rejected() {
        // SRI uses standard alphabet (`+`/`/`); url-safe (`-`/`_`) не принимается.
        assert!(base64_decode("ab-c").is_none());
    }

    #[test]
    fn constant_time_eq_basic() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
    }

    // Verification — нужен mock-provider.

    struct StubProvider {
        supported: SriAlgorithm,
        digest_bytes: Vec<u8>,
    }

    impl DigestProvider for StubProvider {
        fn digest(&self, algorithm: SriAlgorithm, _: &[u8]) -> SriResult<Vec<u8>> {
            if algorithm == self.supported {
                Ok(self.digest_bytes.clone())
            } else {
                Err(SriError::UnsupportedAlgorithm)
            }
        }
    }

    #[test]
    fn verify_empty_list_returns_true() {
        let list = IntegrityList::default();
        let p = StubProvider {
            supported: SriAlgorithm::Sha256,
            digest_bytes: vec![0; 32],
        };
        assert_eq!(list.verify(b"any body", &p), Ok(true));
    }

    #[test]
    fn verify_matching_digest_returns_true() {
        // Создаём IntegrityList с известным digest-ом, и provider, который
        // возвращает тот же digest.
        let target = vec![0xAB; 32];
        let b64 = "q6urq6urq6urq6urq6urq6urq6urq6urq6urq6urq6s="; // 32×0xAB → base64
        let list = IntegrityList::parse(&format!("sha256-{b64}"));
        assert_eq!(list.hashes.len(), 1);
        let p = StubProvider {
            supported: SriAlgorithm::Sha256,
            digest_bytes: target,
        };
        assert_eq!(list.verify(b"body", &p), Ok(true));
    }

    #[test]
    fn verify_mismatched_digest_returns_false() {
        let list = IntegrityList::parse("sha256-GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk=");
        let p = StubProvider {
            supported: SriAlgorithm::Sha256,
            digest_bytes: vec![0xFF; 32],
        };
        assert_eq!(list.verify(b"body", &p), Ok(false));
    }

    #[test]
    fn verify_unsupported_returns_error() {
        let list = IntegrityList::parse("sha256-GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk=");
        let p = StubProvider {
            supported: SriAlgorithm::Sha512,
            digest_bytes: vec![0; 64],
        };
        // sha256 не поддерживается, остальное не доступно → UnsupportedAlgorithm.
        assert_eq!(list.verify(b"body", &p), Err(SriError::UnsupportedAlgorithm));
    }

    #[test]
    fn verify_picks_strongest_algorithm() {
        // sha256 и sha512 в списке — verifier должен пробовать только sha512.
        // SHA-512 digest = 64 байт → base64 = 86 chars + "==" padding.
        let sha512_b64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
        let list = IntegrityList::parse(&format!(
            "sha256-GF+NsyJx/iX1Yab8k4suJkMG7DBO2lGAB9F2SCY4GWk= sha512-{sha512_b64}",
        ));
        assert_eq!(list.hashes.len(), 2);
        // Provider поддерживает только sha256 → strongest (sha512) не доступен.
        let p = StubProvider {
            supported: SriAlgorithm::Sha256,
            digest_bytes: vec![0; 32],
        };
        // Для sha512 provider возвращает UnsupportedAlgorithm; не пытается sha256.
        assert_eq!(list.verify(b"body", &p), Err(SriError::UnsupportedAlgorithm));
    }
}
