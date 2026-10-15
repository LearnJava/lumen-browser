//! Читаемая запись URL для человека (UX-URL-COPY): адресная строка, буфер
//! обмена, позже — пузырь URL у ссылки (UX-STATUS-BUBBLE).
//!
//! [`Url::as_str`](crate::url::Url::as_str) всегда кодирует путь, запрос и
//! фрагмент: `https://site.ru/%D0%BD%D0%BE%D0%B2…`. Человеку нужна кириллица.
//! [`readable_url`] раскодирует `%XX` там, где смысл URL не меняется, а
//! [`display_url`] дополнительно показывает хост по [`display_host`]
//! (омоглиф → Punycode).
//!
//! Остаются закодированными: управляющие символы и пробельные (включая
//! NBSP и `%20`), двунаправленные и невидимые символы, `"` `<` `>`,
//! структурные `%2F %3F %23 %26 %3D %25` и `%2B` (в запросе `+` — пробел),
//! а также байты, не образующие корректный UTF-8. Хост и схема не
//! меняются в [`readable_url`]; функция идемпотентна и не трогает ввод
//! без `scheme://` (поисковый запрос, `about:blank`, `mailto:`).

use crate::idn::{display_host, HostDisplay, SpoofReason};

/// Символы, которые нельзя показывать раскодированными.
fn stays_encoded(c: char) -> bool {
    matches!(c, '/' | '?' | '#' | '&' | '=' | '%' | '+' | '"' | '<' | '>')
        || c.is_control()
        || c.is_whitespace()
        // Двунаправленные и невидимые: подмена порядка/вида адреса.
        || matches!(
            c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFD}'
        )
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn push_pct(out: &mut String, bytes: &[u8]) {
    for b in bytes {
        out.push_str(&format!("%{b:02X}"));
    }
}

/// Раскодировать серию байтов подряд идущих `%XX`.
fn decode_run(bytes: &[u8], out: &mut String) {
    let mut rest = bytes;
    while !rest.is_empty() {
        let (valid, bad_len) = match std::str::from_utf8(rest) {
            Ok(s) => (s, None),
            Err(e) => {
                // Безопасно: `valid_up_to` — граница корректного UTF-8.
                let ok = std::str::from_utf8(&rest[..e.valid_up_to()]).unwrap_or("");
                (ok, Some(e.error_len().unwrap_or(rest.len() - e.valid_up_to())))
            }
        };
        for c in valid.chars() {
            if stays_encoded(c) {
                let mut buf = [0u8; 4];
                push_pct(out, c.encode_utf8(&mut buf).as_bytes());
            } else {
                out.push(c);
            }
        }
        let Some(n) = bad_len else { break };
        let from = valid.len();
        push_pct(out, &rest[from..from + n]);
        rest = &rest[from + n..];
    }
}

/// Байт, закодированный как `%XX` на позиции `i`.
fn pct_byte(b: &[u8], i: usize) -> Option<u8> {
    if b.get(i) != Some(&b'%') {
        return None;
    }
    Some(hex_val(*b.get(i + 1)?)? * 16 + hex_val(*b.get(i + 2)?)?)
}

/// Раскодировать `%XX` в хвосте URL (путь, запрос, фрагмент).
fn decode_tail(tail: &str) -> String {
    let b = tail.as_bytes();
    let mut out = String::with_capacity(tail.len());
    let mut i = 0;
    while i < b.len() {
        if pct_byte(b, i).is_some() {
            let mut run = Vec::new();
            while let Some(byte) = pct_byte(b, i) {
                run.push(byte);
                i += 3;
            }
            decode_run(&run, &mut out);
        } else {
            // Копируем один символ целиком (хвост может быть и не ASCII).
            let ch_len = tail[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&tail[i..i + ch_len]);
            i += ch_len;
        }
    }
    out
}

/// Границы авторитета: `(начало_хоста_с_userinfo, конец_авторитета)` после `scheme://`.
fn authority_span(url: &str) -> Option<(usize, usize)> {
    let sep = url.find("://")?;
    let scheme = &url[..sep];
    if scheme.is_empty()
        || !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let start = sep + 3;
    let end = url[start..].find(['/', '?', '#']).map_or(url.len(), |p| start + p);
    Some((start, end))
}

/// URL с раскодированными путём, запросом и фрагментом (см. модуль). Ввод
/// без `scheme://` возвращается без изменений.
pub fn readable_url(url: &str) -> String {
    match authority_span(url) {
        Some((_, end)) => format!("{}{}", &url[..end], decode_tail(&url[end..])),
        None => url.to_owned(),
    }
}

/// Хост внутри авторитета (без userinfo и порта); `None` для IPv6 и пустого.
fn host_span(url: &str, auth: (usize, usize)) -> Option<(usize, usize)> {
    let (start, end) = auth;
    let after_user = url[start..end].rfind('@').map_or(start, |p| start + p + 1);
    if url[after_user..end].starts_with('[') {
        return None;
    }
    let host_end = url[after_user..end].rfind(':').map_or(end, |p| after_user + p);
    (host_end > after_user).then_some((after_user, host_end))
}

/// [`readable_url`] плюс хост по [`display_host`]: спуф-риск (смешение
/// алфавитов, омоглифы) — Punycode-форма и причина. Это единый форматтер
/// «как показывать адрес человеку» для строки адреса, буфера и пузыря URL.
pub fn display_url(url: &str) -> (String, Option<SpoofReason>) {
    let readable = readable_url(url);
    let Some(auth) = authority_span(&readable) else {
        return (readable, None);
    };
    let Some((hs, he)) = host_span(&readable, auth) else {
        return (readable, None);
    };
    match display_host(&readable[hs..he]) {
        HostDisplay::Punycode { ascii, reason } => {
            (format!("{}{}{}", &readable[..hs], ascii, &readable[he..]), Some(reason))
        }
        HostDisplay::Unicode(_) => (readable, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_cyrillic_path() {
        assert_eq!(
            readable_url("https://site.ru/%D0%BD%D0%BE%D0%B2%D0%BE%D1%81%D1%82%D0%B8/%D1%91%D0%B6"),
            "https://site.ru/новости/ёж"
        );
    }

    #[test]
    fn decodes_query_and_fragment() {
        assert_eq!(
            readable_url("https://x.ru/s?q=%D1%91%D0%B6%D0%B8%D0%BA#%D1%80%D0%B0%D0%B7%D0%B4%D0%B5%D0%BB"),
            "https://x.ru/s?q=ёжик#раздел"
        );
    }

    #[test]
    fn structural_and_dangerous_stay_encoded() {
        for enc in ["%20", "%2F", "%3F", "%23", "%26", "%3D", "%25", "%22", "%3C", "%3E", "%2B", "%09", "%0A"] {
            let u = format!("https://x.ru/a{enc}b");
            assert_eq!(readable_url(&u), u, "{enc}");
        }
        // Нижний регистр нормализуется, но не раскодируется.
        assert_eq!(readable_url("https://x.ru/a%2fb"), "https://x.ru/a%2Fb");
    }

    #[test]
    fn bidi_and_invisible_stay_encoded() {
        // U+202E RIGHT-TO-LEFT OVERRIDE, U+200B ZERO WIDTH SPACE, U+00A0 NBSP.
        assert_eq!(readable_url("https://x.ru/%E2%80%AEgnp"), "https://x.ru/%E2%80%AEgnp");
        assert_eq!(readable_url("https://x.ru/a%E2%80%8Bb"), "https://x.ru/a%E2%80%8Bb");
        assert_eq!(readable_url("https://x.ru/a%C2%A0b"), "https://x.ru/a%C2%A0b");
    }

    #[test]
    fn invalid_utf8_stays_encoded() {
        assert_eq!(readable_url("https://x.ru/%FF%D1%91"), "https://x.ru/%FFё");
        assert_eq!(readable_url("https://x.ru/%D1"), "https://x.ru/%D1");
        assert_eq!(readable_url("https://x.ru/%D1%D1%91"), "https://x.ru/%D1ё");
    }

    #[test]
    fn printable_ascii_is_decoded() {
        assert_eq!(readable_url("https://x.ru/%41%7E"), "https://x.ru/A~");
    }

    #[test]
    fn non_urls_are_untouched() {
        for s in ["rust %D1%91", "about:blank", "mailto:a%20b@x.ru", "vk.com/%D1%91", ""] {
            assert_eq!(readable_url(s), s);
        }
    }

    #[test]
    fn host_scheme_userinfo_port_untouched_and_idempotent() {
        let u = "https://u%D1%91@яндекс.рф:8080/%D1%91?a=1";
        let once = readable_url(u);
        assert_eq!(once, "https://u%D1%91@яндекс.рф:8080/ё?a=1");
        assert_eq!(readable_url(&once), once);
    }

    #[test]
    fn stray_percent_is_kept() {
        assert_eq!(readable_url("https://x.ru/100%"), "https://x.ru/100%");
        assert_eq!(readable_url("https://x.ru/%zz%D1%91"), "https://x.ru/%zzё");
        assert_eq!(readable_url("https://x.ru/%D"), "https://x.ru/%D");
    }

    #[test]
    fn display_url_swaps_homoglyph_host_to_punycode() {
        // Кириллическая «а» в латинском «apple.com».
        let (text, reason) = display_url("https://аpple.com/%D1%91");
        assert!(text.starts_with("https://xn--"), "{text}");
        assert!(text.ends_with("/ё"), "{text}");
        assert_eq!(reason, Some(SpoofReason::MixedScript));
    }

    #[test]
    fn display_url_keeps_honest_hosts() {
        let (text, reason) = display_url("https://яндекс.рф:81/%D1%91");
        assert_eq!(text, "https://яндекс.рф:81/ё");
        assert_eq!(reason, None);
        let (text, reason) = display_url("https://[::1]:8080/%D1%91");
        assert_eq!(text, "https://[::1]:8080/ё");
        assert_eq!(reason, None);
    }
}
