//! HTTP authentication (RFC 7235 base, RFC 7617 Basic, RFC 7616 Digest).
//!
//! Этот модуль умеет:
//! - распарсить заголовок `WWW-Authenticate` в список challenge-ей;
//! - выбрать сильнейший из них (Digest > Basic);
//! - сформировать заголовок `Authorization: ...` для повторного запроса.
//!
//! Криптографические примитивы (MD5 RFC 1321, SHA-256 FIPS 180-4) реализованы
//! здесь же — это не security-критичный crypto (Digest даёт challenge-response,
//! а не шифрование), а protocol-mandated hash-функции. Своя реализация
//! соответствует принципу «default — своё»; rustls/ring используются только
//! там, где crypto действительно security-критичен (TLS, exception #3).
//!
//! Доступ к UI-провайдеру credentials делегируется trait-у
//! `HttpCredentialProvider` (`lumen-core::ext`): HttpClient на 401 формирует
//! challenge и спрашивает у провайдера user/pass.
//!
//! Phase 0 ограничения:
//! - алгоритмы Digest: только `MD5`, `MD5-sess`, `SHA-256`, `SHA-256-sess`
//!   (`SHA-512-256` — Phase 2+, серверы в проде встречаются редко);
//! - `qop` — только `auth` (без `auth-int`, потому что body integrity по
//!   старому RFC 2617 практически нигде не настроен и требует hash от тела
//!   запроса, которого у GET нет);
//! - `Authentication-Info` / nextnonce — игнорируется;
//! - proxy-auth (`Proxy-Authenticate` / `Proxy-Authorization`, 407) — не
//!   реализован: у нас пока нет concept-а HTTP-прокси.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use lumen_core::ext::{
    HttpAuthChallenge, HttpAuthScheme, HttpCredentialProvider, HttpCredentials,
};
use lumen_core::hash::{hex_lower, sha256_hex};
use lumen_core::url::Url;

// ── WWW-Authenticate parser (RFC 7235 §2.1, §4.1) ───────────────────────────

/// Один разобранный challenge: lowercased scheme + auth-params.
///
/// Ключи параметров lowercased (RFC 7235: case-insensitive), значения — как в
/// исходнике (RFC 7616 §3.4: `nonce`/`opaque` могут содержать произвольные
/// байты, чувствительные к case).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedChallenge {
    pub scheme: String,
    pub params: Vec<(String, String)>,
}

impl ParsedChallenge {
    pub fn get(&self, key: &str) -> Option<&str> {
        let key_lc = key.to_ascii_lowercase();
        self.params
            .iter()
            .find(|(k, _)| k.as_str() == key_lc)
            .map(|(_, v)| v.as_str())
    }
}

/// Разобрать заголовок `WWW-Authenticate` в список challenge-ей.
///
/// RFC 7235 §2.1: `challenges = 1#challenge`,
/// где `challenge = auth-scheme [1*SP (token68 / [auth-param *(OWS "," OWS auth-param)])]`.
///
/// Хитрость в том, что `,` — и разделитель challenges, и разделитель
/// auth-param. Различение: `,` после которого идёт `token SP token`/`token,`
/// или scheme-без-параметров — начало нового challenge; `,` после `key=value`
/// — продолжение параметров текущего. Парсер ведёт себя следующим образом:
///
/// - читаем `auth-scheme` (token);
/// - дальше — список auth-param;
/// - для каждой `,`-секции: если она `token` без `=` — это начало нового
///   challenge; если `token = value` — auth-param текущего.
///
/// Не-Basic/Digest схемы (`Negotiate`, `NTLM`, `Bearer`, …) распознаются
/// (`scheme` корректно прочитан), но без поддержки на нашей стороне —
/// клиент выкинет их при `select_best_challenge`.
pub(crate) fn parse_www_authenticate(header: &str) -> Vec<ParsedChallenge> {
    let mut out: Vec<ParsedChallenge> = Vec::new();
    let bytes = header.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        i = skip_ows(bytes, i);
        if i >= bytes.len() {
            break;
        }

        let scheme_start = i;
        while i < bytes.len() && is_token_char(bytes[i]) {
            i += 1;
        }
        if i == scheme_start {
            // не token — пропускаем символ, чтобы не залипнуть.
            i += 1;
            continue;
        }
        let scheme = header[scheme_start..i].to_ascii_lowercase();

        let mut params: Vec<(String, String)> = Vec::new();
        loop {
            i = skip_ows(bytes, i);
            if i >= bytes.len() {
                break;
            }
            if bytes[i] == b',' {
                i += 1;
                continue;
            }

            // Прочитать token.
            let token_start = i;
            while i < bytes.len() && is_token_char(bytes[i]) {
                i += 1;
            }
            if i == token_start {
                // не token — break, новая итерация попробует scheme заново.
                break;
            }
            let token = header[token_start..i].to_string();

            i = skip_ows(bytes, i);
            if i < bytes.len() && bytes[i] == b'=' {
                // auth-param
                i += 1;
                i = skip_ows(bytes, i);
                let value = parse_auth_param_value(bytes, &mut i, header);
                params.push((token.to_ascii_lowercase(), value));
            } else {
                // token без `=` — это начало нового challenge, который мы
                // прочитали в `token`. Откатываемся: завершаем текущий
                // challenge, и продолжаем парсинг с этой scheme.
                out.push(ParsedChallenge {
                    scheme,
                    params: std::mem::take(&mut params),
                });
                // Подменяем scheme на token: мы УЖЕ продвинулись за token.
                let new_scheme = token.to_ascii_lowercase();
                let mut new_params: Vec<(String, String)> = Vec::new();
                // Идентично обработать параметры нового challenge.
                process_remaining_challenge(bytes, header, &mut i, &mut new_params, &mut out, new_scheme);
                // process_remaining_challenge сам положит challenge в out;
                // мы выходим из внешнего цикла, иначе scheme-loop запутается.
                return out;
            }
        }

        out.push(ParsedChallenge { scheme, params });
    }

    out
}

/// Помощник: после определения нового scheme в середине списка прочитать
/// его параметры и завершить. Вызывается из основного парсера, чтобы не
/// дублировать вложенный auth-param loop.
fn process_remaining_challenge(
    bytes: &[u8],
    header: &str,
    i: &mut usize,
    params: &mut Vec<(String, String)>,
    out: &mut Vec<ParsedChallenge>,
    scheme: String,
) {
    loop {
        *i = skip_ows(bytes, *i);
        if *i >= bytes.len() {
            break;
        }
        if bytes[*i] == b',' {
            *i += 1;
            continue;
        }
        let token_start = *i;
        while *i < bytes.len() && is_token_char(bytes[*i]) {
            *i += 1;
        }
        if *i == token_start {
            break;
        }
        let token = header[token_start..*i].to_string();
        *i = skip_ows(bytes, *i);
        if *i < bytes.len() && bytes[*i] == b'=' {
            *i += 1;
            *i = skip_ows(bytes, *i);
            let value = parse_auth_param_value(bytes, i, header);
            params.push((token.to_ascii_lowercase(), value));
        } else {
            // Новый challenge снова. Завершаем текущий, рекурсивно.
            out.push(ParsedChallenge {
                scheme,
                params: std::mem::take(params),
            });
            let next_scheme = token.to_ascii_lowercase();
            let mut next_params: Vec<(String, String)> = Vec::new();
            process_remaining_challenge(bytes, header, i, &mut next_params, out, next_scheme);
            return;
        }
    }
    out.push(ParsedChallenge {
        scheme,
        params: std::mem::take(params),
    });
}

fn parse_auth_param_value(bytes: &[u8], i: &mut usize, header: &str) -> String {
    if *i < bytes.len() && bytes[*i] == b'"' {
        // quoted-string (RFC 7230 §3.2.6) с обработкой `\` escape.
        *i += 1;
        let mut out = String::new();
        while *i < bytes.len() && bytes[*i] != b'"' {
            if bytes[*i] == b'\\' && *i + 1 < bytes.len() {
                out.push(bytes[*i + 1] as char);
                *i += 2;
            } else {
                out.push(bytes[*i] as char);
                *i += 1;
            }
        }
        if *i < bytes.len() {
            *i += 1; // closing "
        }
        out
    } else {
        // token (без quote).
        let start = *i;
        while *i < bytes.len() && is_token_char(bytes[*i]) {
            *i += 1;
        }
        header[start..*i].to_string()
    }
}

/// RFC 7230 §3.2.6: `token = 1*tchar`, `tchar = "!" / "#" / "$" / "%" / "&" /
/// "'" / "*" / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA`.
fn is_token_char(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
        | b'^' | b'_' | b'`' | b'|' | b'~'
        | b'0'..=b'9'
        | b'A'..=b'Z'
        | b'a'..=b'z'
    )
}

/// OWS (optional whitespace) per RFC 7230 §3.2.3 = *( SP / HTAB ).
fn skip_ows(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

// ── Challenge selection ─────────────────────────────────────────────────────

/// Выбрать сильнейший challenge для построения Authorization.
///
/// Приоритет: Digest > Basic. Внутри Digest предпочитается SHA-256 над MD5
/// (RFC 7616 §3.7: «client should choose the algorithm that provides the
/// strongest security»). Прочие схемы (Negotiate, NTLM, Bearer) игнорируются.
pub(crate) fn select_best_challenge(
    challenges: &[ParsedChallenge],
) -> Option<(HttpAuthScheme, &ParsedChallenge)> {
    // 1. Digest с sha-256.
    if let Some(c) = challenges
        .iter()
        .find(|c| c.scheme == "digest" && c.get("algorithm").is_some_and(is_sha256_algo))
    {
        return Some((HttpAuthScheme::Digest, c));
    }
    // 2. Digest с MD5 (включая default — RFC 7616 §3.3: missing algorithm = MD5).
    if let Some(c) = challenges.iter().find(|c| c.scheme == "digest") {
        return Some((HttpAuthScheme::Digest, c));
    }
    // 3. Basic.
    if let Some(c) = challenges.iter().find(|c| c.scheme == "basic") {
        return Some((HttpAuthScheme::Basic, c));
    }
    None
}

fn is_sha256_algo(algo: &str) -> bool {
    let lc = algo.to_ascii_lowercase();
    lc == "sha-256" || lc == "sha-256-sess"
}

fn is_md5_algo(algo: &str) -> bool {
    let lc = algo.to_ascii_lowercase();
    lc.is_empty() || lc == "md5" || lc == "md5-sess"
}

fn is_session_algo(algo: &str) -> bool {
    let lc = algo.to_ascii_lowercase();
    lc.ends_with("-sess")
}

// ── Преобразование в HttpAuthChallenge (для провайдера) ─────────────────────

/// Из лучшего challenge сформировать публичный `HttpAuthChallenge` —
/// то, что увидит `HttpCredentialProvider`. Realm берётся из `realm`-параметра,
/// либо пустая строка если отсутствует.
pub(crate) fn challenge_for_provider(
    origin: &str,
    scheme: HttpAuthScheme,
    parsed: &ParsedChallenge,
) -> HttpAuthChallenge {
    let realm = parsed.get("realm").unwrap_or("").to_string();
    HttpAuthChallenge {
        origin: origin.to_string(),
        realm,
        scheme,
    }
}

// ── Basic (RFC 7617) ────────────────────────────────────────────────────────

/// Сформировать значение для header `Authorization` по схеме `Basic`.
///
/// `Authorization: Basic <base64(user:pass)>` (RFC 7617 §2). UTF-8 charset —
/// `charset="UTF-8"` параметр в challenge не меняет логику для нас: строки
/// уже UTF-8 в Rust, encoded напрямую.
pub(crate) fn build_basic_authorization(creds: &HttpCredentials) -> String {
    let user_pass = format!("{}:{}", creds.username, creds.password);
    format!("Basic {}", base64_encode_std(user_pass.as_bytes()))
}

// ── Digest (RFC 7616) ───────────────────────────────────────────────────────

/// Атомарный счётчик nc (nonce-count) — RFC 7616 §3.4.4: «nc value SHOULD
/// be incremented for each request». Глобальный счётчик per-HttpClient
/// (а не per-nonce) проще и тоже валиден; сервер видит monotonically
/// increasing значения для одной nonce.
static GLOBAL_NC: AtomicU32 = AtomicU32::new(0);

/// Сформировать значение для header `Authorization` по схеме `Digest`.
///
/// Реализует RFC 7616 §3.4 для qop=auth с MD5 / MD5-sess / SHA-256 / SHA-256-sess.
/// Возвращает `None`, если challenge невалиден (нет nonce, неподдерживаемый
/// algorithm, нет realm — обязательное поле).
///
/// Параметры:
/// - `method` — HTTP-метод (для нас всегда `"GET"`);
/// - `uri` — request-target (path+query, как в request-line, без origin).
pub(crate) fn build_digest_authorization(
    creds: &HttpCredentials,
    parsed: &ParsedChallenge,
    method: &str,
    uri: &str,
) -> Option<String> {
    let realm = parsed.get("realm")?;
    let nonce = parsed.get("nonce")?;
    let algorithm = parsed.get("algorithm").unwrap_or(""); // default = MD5
    let opaque = parsed.get("opaque");

    // qop-options: comma-separated. Берём «auth», если есть; иначе legacy-mode
    // (RFC 2069 — нет qop, нет cnonce, нет nc; response = MD5(HA1:nonce:HA2)).
    let qop_list = parsed.get("qop").unwrap_or("");
    let has_qop_auth = qop_list
        .split(',')
        .any(|q| q.trim().eq_ignore_ascii_case("auth"));

    let use_sha256 = is_sha256_algo(algorithm);
    if !use_sha256 && !is_md5_algo(algorithm) {
        // Алгоритм неизвестен — мы не умеем строить response.
        return None;
    }
    let session_mode = is_session_algo(algorithm);

    let hash_str: fn(&[u8]) -> String = if use_sha256 { sha256_hex } else { md5_hex };

    let nc = GLOBAL_NC.fetch_add(1, Ordering::Relaxed) + 1;
    let nc_str = format!("{nc:08x}");
    let cnonce = generate_cnonce();

    // HA1
    let mut ha1 = hash_str(format!("{}:{}:{}", creds.username, realm, creds.password).as_bytes());
    if session_mode {
        ha1 = hash_str(format!("{ha1}:{nonce}:{cnonce}").as_bytes());
    }

    // HA2 (qop=auth и legacy — оба считают MD5(method:uri))
    let ha2 = hash_str(format!("{method}:{uri}").as_bytes());

    // response-digest
    let response = if has_qop_auth {
        hash_str(format!("{ha1}:{nonce}:{nc_str}:{cnonce}:auth:{ha2}").as_bytes())
    } else {
        // RFC 2069 legacy.
        hash_str(format!("{ha1}:{nonce}:{ha2}").as_bytes())
    };

    // Собираем header. Имена параметров регистронезависимы; используем форму
    // из RFC 7616.
    let mut header = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\"",
        escape_quoted(&creds.username),
        escape_quoted(realm),
        escape_quoted(nonce),
        escape_quoted(uri),
        response
    );
    if !algorithm.is_empty() {
        // RFC 7616 §3.4: «algorithm» — token, без кавычек.
        header.push_str(&format!(", algorithm={algorithm}"));
    }
    if has_qop_auth {
        header.push_str(&format!(
            ", qop=auth, nc={nc_str}, cnonce=\"{}\"",
            escape_quoted(&cnonce)
        ));
    }
    if let Some(op) = opaque {
        header.push_str(&format!(", opaque=\"{}\"", escape_quoted(op)));
    }
    Some(header)
}

fn escape_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

/// Сгенерировать cnonce — псевдослучайная строка, уникальная per-request.
/// RFC 7616 §3.4.5: «opaque, unique string»; сервер cnonce не проверяет, но
/// предсказуемый ослабляет защиту против rainbow-таблиц по нашему nc.
///
/// Используем std::time monotonic + counter, hex-encoded. Не cryptographic
/// strong, но для Digest cnonce этого достаточно (см. RFC 7616 §5.10:
/// «client nonce» — anti-chosen-plaintext-attack, не secret).
fn generate_cnonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ts:016x}{c:08x}")
}

// ── Base64 standard alphabet (RFC 4648 §4) ──────────────────────────────────

/// Стандартный base64 с padding `=` (RFC 4648 §4). Алфавит `A-Za-z0-9+/`.
/// Для Basic auth (отличается от RFC 4648 §5 base64url в `+/` vs `-_`).
fn base64_encode_std(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = bytes[i + 1] as u32;
        let b2 = bytes[i + 2] as u32;
        let bits = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((bits >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((bits >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((bits >> 6) & 0x3F) as usize] as char);
        out.push(ALPHABET[(bits & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let b0 = bytes[i] as u32;
        let bits = b0 << 16;
        out.push(ALPHABET[((bits >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((bits >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let b0 = bytes[i] as u32;
        let b1 = bytes[i + 1] as u32;
        let bits = (b0 << 16) | (b1 << 8);
        out.push(ALPHABET[((bits >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((bits >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((bits >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

// ── MD5 (RFC 1321) ──────────────────────────────────────────────────────────
//
// 128-bit hash. Криптографически сломан (collision attacks), но в HTTP Digest
// используется как challenge-response без претензии на security beyond
// «password не в plain». Реализация по RFC 1321 §3.

fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee, 0xf57c_0faf, 0x4787_c62a, 0xa830_4613,
        0xfd46_9501, 0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1, 0x895c_d7be, 0x6b90_1122, 0xfd98_7193,
        0xa679_438e, 0x49b4_0821, 0xf61e_2562, 0xc040_b340, 0x265e_5a51, 0xe9b6_c7aa, 0xd62f_105d,
        0x0244_1453, 0xd8a1_e681, 0xe7d3_fbc8, 0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed,
        0xa9e3_e905, 0xfcef_a3f8, 0x676f_02d9, 0x8d2a_4c8a, 0xfffa_3942, 0x8771_f681, 0x6d9d_6122,
        0xfde5_380c, 0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70, 0x289b_7ec6, 0xeaa1_27fa,
        0xd4ef_3085, 0x0488_1d05, 0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665, 0xf429_2244,
        0x432a_ff97, 0xab94_23a7, 0xfc93_a039, 0x655b_59c3, 0x8f0c_cc92, 0xffef_f47d, 0x8584_5dd1,
        0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1, 0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb,
        0xeb86_d391,
    ];
    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    // Padding: append 0x80, потом 0x00 до длины ≡ 56 (mod 64), потом 64-bit
    // length in bits little-endian.
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in padded.chunks(64) {
        let mut m = [0u32; 16];
        for j in 0..16 {
            m[j] = u32::from_le_bytes([
                chunk[j * 4],
                chunk[j * 4 + 1],
                chunk[j * 4 + 2],
                chunk[j * 4 + 3],
            ]);
        }
        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;
        for i in 0..64 {
            let (f, g): (u32, usize) = if i < 16 {
                ((b & c) | (!b & d), i)
            } else if i < 32 {
                ((d & b) | (!d & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

fn md5_hex(input: &[u8]) -> String {
    let digest = md5(input);
    hex_lower(&digest)
}

// SHA-256 / hex_lower живут в `lumen_core::hash` — общий примитив для
// Digest auth, SRI и Safe Browsing.

// ── Origin helpers ──────────────────────────────────────────────────────────

/// Сформировать origin-строку `scheme://host[:port]` для передачи в
/// `HttpCredentialProvider`. Host — ASCII (Punycode для IDN), default-port
/// (80 для http / 443 для https) опускается — origin становится канонической
/// строкой, по которой провайдер ищет creds.
pub(crate) fn origin_of(url: &Url) -> String {
    let scheme = url.scheme();
    let host = url.host_ascii().unwrap_or_default();
    let port = url.effective_port();
    let default_port = match scheme {
        "http" => Some(80u16),
        "https" => Some(443u16),
        _ => None,
    };
    if port == default_port {
        format!("{scheme}://{host}")
    } else if let Some(p) = port {
        format!("{scheme}://{host}:{p}")
    } else {
        format!("{scheme}://{host}")
    }
}

// ── StaticCredentialProvider ────────────────────────────────────────────────

/// Простой credential-провайдер с фиксированной табличкой `(origin, realm) →
/// (user, pass)`. Используется для тестов, CI-сценариев и curl-style
/// конфигурации (`--user user:pass`).
///
/// Lookup: сначала точное совпадение `(origin, realm)`, затем
/// `(origin, "")` (любой realm на этом origin), затем `("", realm)`
/// (любой origin с этим realm — для прокси-сценариев в будущем), затем
/// `("", "")` — fallback default.
pub struct StaticCredentialProvider {
    entries: Mutex<HashMap<(String, String), HttpCredentials>>,
}

impl StaticCredentialProvider {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Точное совпадение `(origin, realm)`.
    #[must_use]
    pub fn with(self, origin: &str, realm: &str, user: &str, pass: &str) -> Self {
        self.add(origin, realm, user, pass);
        self
    }

    /// Зарегистрировать creds после конструирования. `&self` (не `&mut`) —
    /// у нас Mutex; провайдер можно делить через Arc и доливать creds в
    /// процессе работы (например, после UI-popup).
    pub fn add(&self, origin: &str, realm: &str, user: &str, pass: &str) {
        self.entries.lock().unwrap().insert(
            (origin.to_string(), realm.to_string()),
            HttpCredentials {
                username: user.to_string(),
                password: pass.to_string(),
            },
        );
    }
}

impl Default for StaticCredentialProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpCredentialProvider for StaticCredentialProvider {
    fn credentials(&self, challenge: &HttpAuthChallenge) -> Option<HttpCredentials> {
        let entries = self.entries.lock().unwrap();
        // 1. (origin, realm) exact
        if let Some(c) = entries.get(&(challenge.origin.clone(), challenge.realm.clone())) {
            return Some(c.clone());
        }
        // 2. (origin, "") — любой realm на этом origin
        if let Some(c) = entries.get(&(challenge.origin.clone(), String::new())) {
            return Some(c.clone());
        }
        // 3. ("", realm) — любой origin
        if let Some(c) = entries.get(&(String::new(), challenge.realm.clone())) {
            return Some(c.clone());
        }
        // 4. ("", "") — default
        entries
            .get(&(String::new(), String::new()))
            .cloned()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // MD5 test vectors from RFC 1321 Appendix A.5.
    #[test]
    fn md5_empty_string() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_short() {
        assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn md5_message_digest() {
        assert_eq!(md5_hex(b"message digest"), "f96b697d7cb7938d525a2f31aaf161d0");
    }

    #[test]
    fn md5_alphabet() {
        assert_eq!(
            md5_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }

    #[test]
    fn md5_long() {
        // 80 цифр — два полных 64-байтных блока, padding в третьем.
        assert_eq!(
            md5_hex(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    // SHA-256 — общий примитив в `lumen_core::hash`, FIPS 180-4 vectors
    // покрыты unit-тестами там. Здесь — только смоук: чёрная коробка sha256_hex
    // правильно склеивается с Digest формулой (см. `digest_sha256_response_deterministic`).

    // Base64 standard test vectors from RFC 4648 §10.
    #[test]
    fn base64_std_examples() {
        assert_eq!(base64_encode_std(b""), "");
        assert_eq!(base64_encode_std(b"f"), "Zg==");
        assert_eq!(base64_encode_std(b"fo"), "Zm8=");
        assert_eq!(base64_encode_std(b"foo"), "Zm9v");
        assert_eq!(base64_encode_std(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode_std(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode_std(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_std_binary_with_plus_slash() {
        // 0xFB 0xFF: 11111011 11111111 → +/ symbols in alphabet.
        let out = base64_encode_std(&[0xFB, 0xFF]);
        assert_eq!(out, "+/8=");
    }

    // ── Parser tests ────────────────────────────────────────────────────────

    #[test]
    fn parse_single_basic_challenge() {
        let challenges = parse_www_authenticate(r#"Basic realm="WallyWorld""#);
        assert_eq!(challenges.len(), 1);
        assert_eq!(challenges[0].scheme, "basic");
        assert_eq!(challenges[0].get("realm"), Some("WallyWorld"));
    }

    #[test]
    fn parse_basic_with_charset() {
        let challenges =
            parse_www_authenticate(r#"Basic realm="foo", charset="UTF-8""#);
        assert_eq!(challenges.len(), 1);
        assert_eq!(challenges[0].get("realm"), Some("foo"));
        assert_eq!(challenges[0].get("charset"), Some("UTF-8"));
    }

    #[test]
    fn parse_digest_full_rfc7616_example() {
        // RFC 7616 §3.9.1 example WWW-Authenticate header.
        let h = r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=SHA-256, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#;
        let challenges = parse_www_authenticate(h);
        assert_eq!(challenges.len(), 1);
        let c = &challenges[0];
        assert_eq!(c.scheme, "digest");
        assert_eq!(c.get("realm"), Some("http-auth@example.org"));
        assert_eq!(c.get("qop"), Some("auth, auth-int"));
        assert_eq!(c.get("algorithm"), Some("SHA-256"));
        assert_eq!(c.get("nonce"), Some("7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v"));
        assert_eq!(c.get("opaque"), Some("FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS"));
    }

    #[test]
    fn parse_two_challenges_digest_then_basic() {
        let h = r#"Digest realm="r1", nonce="n1", Basic realm="r2""#;
        let challenges = parse_www_authenticate(h);
        assert_eq!(challenges.len(), 2);
        assert_eq!(challenges[0].scheme, "digest");
        assert_eq!(challenges[0].get("realm"), Some("r1"));
        assert_eq!(challenges[1].scheme, "basic");
        assert_eq!(challenges[1].get("realm"), Some("r2"));
    }

    #[test]
    fn parse_two_challenges_basic_then_digest() {
        let h = r#"Basic realm="r2", Digest realm="r1", nonce="n1""#;
        let challenges = parse_www_authenticate(h);
        assert_eq!(challenges.len(), 2);
        assert_eq!(challenges[0].scheme, "basic");
        assert_eq!(challenges[1].scheme, "digest");
        assert_eq!(challenges[1].get("nonce"), Some("n1"));
    }

    #[test]
    fn parse_quoted_string_with_escaped_quote() {
        // realm="say \"hi\"" — кавычка escape-нута обратным слэшем.
        let h = r#"Basic realm="say \"hi\"""#;
        let challenges = parse_www_authenticate(h);
        assert_eq!(challenges.len(), 1);
        assert_eq!(challenges[0].get("realm"), Some(r#"say "hi""#));
    }

    #[test]
    fn parse_token_value_without_quotes() {
        // algorithm=MD5 — без кавычек (algorithm = token).
        let h = "Digest realm=\"r\", nonce=\"n\", algorithm=MD5";
        let challenges = parse_www_authenticate(h);
        assert_eq!(challenges[0].get("algorithm"), Some("MD5"));
    }

    #[test]
    fn parse_unknown_scheme_preserved() {
        let h = r#"Bearer realm="api", error="invalid_token""#;
        let challenges = parse_www_authenticate(h);
        assert_eq!(challenges.len(), 1);
        assert_eq!(challenges[0].scheme, "bearer");
        assert_eq!(challenges[0].get("error"), Some("invalid_token"));
    }

    #[test]
    fn parse_case_insensitive_param_names() {
        let h = r#"Basic REALM="x", Realm="y""#;
        let challenges = parse_www_authenticate(h);
        // Оба ключа в параметрах сводятся к "realm"; get вернёт первый.
        assert_eq!(challenges[0].params.len(), 2);
        assert_eq!(challenges[0].get("realm"), Some("x"));
    }

    // ── Challenge selection ────────────────────────────────────────────────

    #[test]
    fn select_prefers_digest_over_basic() {
        let challenges = parse_www_authenticate(r#"Basic realm="r1", Digest realm="r2", nonce="n""#);
        let (scheme, _) = select_best_challenge(&challenges).unwrap();
        assert_eq!(scheme, HttpAuthScheme::Digest);
    }

    #[test]
    fn select_prefers_sha256_digest_over_md5_digest() {
        let h = r#"Digest realm="r1", nonce="n1", algorithm=MD5, Digest realm="r2", nonce="n2", algorithm=SHA-256"#;
        let challenges = parse_www_authenticate(h);
        let (scheme, parsed) = select_best_challenge(&challenges).unwrap();
        assert_eq!(scheme, HttpAuthScheme::Digest);
        assert_eq!(parsed.get("algorithm"), Some("SHA-256"));
    }

    #[test]
    fn select_returns_none_for_unsupported_only() {
        let challenges = parse_www_authenticate(r#"Bearer realm="api""#);
        assert!(select_best_challenge(&challenges).is_none());
    }

    #[test]
    fn select_returns_basic_when_only_basic() {
        let challenges = parse_www_authenticate(r#"Basic realm="x""#);
        let (scheme, _) = select_best_challenge(&challenges).unwrap();
        assert_eq!(scheme, HttpAuthScheme::Basic);
    }

    // ── Basic builder ──────────────────────────────────────────────────────

    #[test]
    fn basic_authorization_rfc7617_example() {
        // RFC 7617 §2: Aladdin / open sesame.
        let creds = HttpCredentials {
            username: "Aladdin".into(),
            password: "open sesame".into(),
        };
        let header = build_basic_authorization(&creds);
        assert_eq!(header, "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
    }

    #[test]
    fn basic_authorization_with_utf8_password() {
        let creds = HttpCredentials {
            username: "user".into(),
            password: "пароль".into(),
        };
        let header = build_basic_authorization(&creds);
        // "user:пароль" в UTF-8 = 75 73 65 72 3A D0 BF D0 B0 D1 80 D0 BE D0 BB D1 8C
        assert!(header.starts_with("Basic "));
        // Verify decode is sensible
        let encoded = &header["Basic ".len()..];
        assert_eq!(encoded, "dXNlcjrQv9Cw0YDQvtC70Yw=");
    }

    // ── Digest builder ─────────────────────────────────────────────────────

    #[test]
    fn digest_authorization_md5_rfc2617_example() {
        // RFC 2617 §3.5 (classic example):
        //   user="Mufasa", pass="Circle Of Life", realm="testrealm@host.com"
        //   nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", uri="/dir/index.html"
        //   qop=auth, cnonce="0a4f113b", nc=00000001 ⇒ response = 6629fae49393a05397450978507c4ef1
        //
        // У нас cnonce и nc автоматически выставляются — точную response не
        // воспроизведём. Тест проверяет, что builder включает все нужные поля
        // в правильной форме.
        let creds = HttpCredentials {
            username: "Mufasa".into(),
            password: "Circle Of Life".into(),
        };
        let parsed = ParsedChallenge {
            scheme: "digest".into(),
            params: vec![
                ("realm".into(), "testrealm@host.com".into()),
                ("qop".into(), "auth".into()),
                (
                    "nonce".into(),
                    "dcd98b7102dd2f0e8b11d0f600bfb0c093".into(),
                ),
                ("opaque".into(), "5ccc069c403ebaf9f0171e9517f40e41".into()),
            ],
        };
        let header = build_digest_authorization(&creds, &parsed, "GET", "/dir/index.html").unwrap();
        assert!(header.starts_with("Digest "));
        assert!(header.contains("username=\"Mufasa\""));
        assert!(header.contains("realm=\"testrealm@host.com\""));
        assert!(header.contains("nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\""));
        assert!(header.contains("uri=\"/dir/index.html\""));
        assert!(header.contains("qop=auth"));
        assert!(header.contains("nc="));
        assert!(header.contains("cnonce=\""));
        assert!(header.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""));
        // response — 32 hex digits (MD5).
        let response_marker = "response=\"";
        let r_start = header.find(response_marker).unwrap() + response_marker.len();
        let r_end = header[r_start..].find('"').unwrap() + r_start;
        let response = &header[r_start..r_end];
        assert_eq!(response.len(), 32);
        assert!(response.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn digest_authorization_sha256_response_is_64_hex() {
        let creds = HttpCredentials {
            username: "u".into(),
            password: "p".into(),
        };
        let parsed = ParsedChallenge {
            scheme: "digest".into(),
            params: vec![
                ("realm".into(), "r".into()),
                ("qop".into(), "auth".into()),
                ("nonce".into(), "abc".into()),
                ("algorithm".into(), "SHA-256".into()),
            ],
        };
        let header = build_digest_authorization(&creds, &parsed, "GET", "/").unwrap();
        let response_marker = "response=\"";
        let r_start = header.find(response_marker).unwrap() + response_marker.len();
        let r_end = header[r_start..].find('"').unwrap() + r_start;
        assert_eq!(r_end - r_start, 64); // SHA-256 hex
        assert!(header.contains("algorithm=SHA-256"));
    }

    #[test]
    fn digest_md5_response_deterministic_with_fixed_nc_cnonce() {
        // Воспроизводим RFC 2617 §3.5 ровно: HA1, HA2, response для известных
        // user/pass/realm/uri/nonce/cnonce/nc. Не через builder (он генерит
        // cnonce/nc сам), а напрямую через хэш-функции — это смоук-тест,
        // что MD5 и формула response верны.
        let user = "Mufasa";
        let realm = "testrealm@host.com";
        let password = "Circle Of Life";
        let method = "GET";
        let uri = "/dir/index.html";
        let nonce = "dcd98b7102dd2f0e8b11d0f600bfb0c093";
        let nc = "00000001";
        let cnonce = "0a4f113b";
        let qop = "auth";

        let ha1 = md5_hex(format!("{user}:{realm}:{password}").as_bytes());
        assert_eq!(ha1, "939e7578ed9e3c518a452acee763bce9");
        let ha2 = md5_hex(format!("{method}:{uri}").as_bytes());
        assert_eq!(ha2, "39aff3a2bab6126f332b942af96d3366");
        let response = md5_hex(format!("{ha1}:{nonce}:{nc}:{cnonce}:{qop}:{ha2}").as_bytes());
        assert_eq!(response, "6629fae49393a05397450978507c4ef1");
    }

    #[test]
    fn digest_sha256_response_deterministic() {
        // RFC 7616 §3.9.1 — SHA-256 sample. Spec test-vector:
        //   user="Mufasa", pass="Circle of Life", realm="http-auth@example.org",
        //   uri="/dir/index.html", method=GET, qop=auth,
        //   nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v",
        //   nc=00000001, cnonce="f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ"
        //   ⇒ response = "753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1"
        let user = "Mufasa";
        let realm = "http-auth@example.org";
        let password = "Circle of Life";
        let method = "GET";
        let uri = "/dir/index.html";
        let nonce = "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v";
        let nc = "00000001";
        let cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        let qop = "auth";

        let ha1 = sha256_hex(format!("{user}:{realm}:{password}").as_bytes());
        let ha2 = sha256_hex(format!("{method}:{uri}").as_bytes());
        let response = sha256_hex(format!("{ha1}:{nonce}:{nc}:{cnonce}:{qop}:{ha2}").as_bytes());
        assert_eq!(
            response,
            "753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1"
        );
    }

    #[test]
    fn digest_md5_sess_uses_chained_ha1() {
        // RFC 7616: для md5-sess HA1 пере-хэшируется с nonce/cnonce.
        let user = "u";
        let realm = "r";
        let password = "p";
        let nonce = "N";
        let cnonce = "C";
        let inner = md5_hex(format!("{user}:{realm}:{password}").as_bytes());
        let ha1_sess = md5_hex(format!("{inner}:{nonce}:{cnonce}").as_bytes());
        // Просто sanity: HA1_sess ≠ обычный HA1.
        assert_ne!(ha1_sess, inner);
        assert_eq!(ha1_sess.len(), 32);
    }

    #[test]
    fn digest_missing_nonce_returns_none() {
        let creds = HttpCredentials {
            username: "u".into(),
            password: "p".into(),
        };
        let parsed = ParsedChallenge {
            scheme: "digest".into(),
            params: vec![("realm".into(), "r".into())], // no nonce
        };
        assert!(build_digest_authorization(&creds, &parsed, "GET", "/").is_none());
    }

    #[test]
    fn digest_missing_realm_returns_none() {
        let creds = HttpCredentials {
            username: "u".into(),
            password: "p".into(),
        };
        let parsed = ParsedChallenge {
            scheme: "digest".into(),
            params: vec![("nonce".into(), "N".into())], // no realm
        };
        assert!(build_digest_authorization(&creds, &parsed, "GET", "/").is_none());
    }

    #[test]
    fn digest_unsupported_algorithm_returns_none() {
        let creds = HttpCredentials {
            username: "u".into(),
            password: "p".into(),
        };
        let parsed = ParsedChallenge {
            scheme: "digest".into(),
            params: vec![
                ("realm".into(), "r".into()),
                ("nonce".into(), "N".into()),
                ("algorithm".into(), "SHA-512-256".into()),
            ],
        };
        assert!(build_digest_authorization(&creds, &parsed, "GET", "/").is_none());
    }

    #[test]
    fn digest_legacy_rfc2069_no_qop() {
        // Без qop вообще — legacy RFC 2069: response = MD5(HA1:nonce:HA2).
        let creds = HttpCredentials {
            username: "u".into(),
            password: "p".into(),
        };
        let parsed = ParsedChallenge {
            scheme: "digest".into(),
            params: vec![
                ("realm".into(), "r".into()),
                ("nonce".into(), "N".into()),
            ],
        };
        let header = build_digest_authorization(&creds, &parsed, "GET", "/").unwrap();
        // Не содержит qop, nc, cnonce.
        assert!(!header.contains("qop="));
        assert!(!header.contains("nc="));
        assert!(!header.contains("cnonce="));
    }

    #[test]
    fn challenge_for_provider_uses_realm_and_origin() {
        let parsed = ParsedChallenge {
            scheme: "digest".into(),
            params: vec![("realm".into(), "Admin".into())],
        };
        let c = challenge_for_provider("https://example.com:8443", HttpAuthScheme::Digest, &parsed);
        assert_eq!(c.origin, "https://example.com:8443");
        assert_eq!(c.realm, "Admin");
        assert_eq!(c.scheme, HttpAuthScheme::Digest);
    }

    #[test]
    fn challenge_for_provider_empty_realm_when_absent() {
        let parsed = ParsedChallenge {
            scheme: "basic".into(),
            params: vec![],
        };
        let c = challenge_for_provider("http://x/", HttpAuthScheme::Basic, &parsed);
        assert_eq!(c.realm, "");
    }

    #[test]
    fn escape_quoted_passes_through_safe_chars() {
        assert_eq!(escape_quoted("hello"), "hello");
        assert_eq!(escape_quoted("a/b+c"), "a/b+c");
    }

    #[test]
    fn escape_quoted_escapes_backslash_and_quote() {
        assert_eq!(escape_quoted("a\"b"), "a\\\"b");
        assert_eq!(escape_quoted("a\\b"), "a\\\\b");
    }
}
