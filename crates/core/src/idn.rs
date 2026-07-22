//! IDN (Internationalized Domain Names) — преобразование Unicode-доменов
//! в ASCII-форму для DNS / TLS / `Host:` header.
//!
//! Phase 0 — упрощённое подмножество IDNA 2008:
//!   - ASCII-метки пропускаются без изменений (быстрый путь);
//!   - не-ASCII метки прогоняются через [`punycode::encode`] и
//!     префиксуются `xn--`;
//!   - вход lowercase'тся через `str::to_lowercase` (DNS case-insensitive).
//!
//! Не реализовано:
//!   - NFC normalization — для большинства русских доменов уже в NFC,
//!     но строгая IDNA требует Unicode-таблиц;
//!   - UTS #46 mappings (deviation characters: ß, ς, ZWJ/ZWNJ);
//!   - валидация character classes (контекстуальные правила, bidi).
//!
//! [`display_host`] добавляет отдельный, не связанный с UTS #46/#39
//! Chromium-lite эвристический детектор омоглифов/mixed-script для
//! решения «показывать Unicode-хост или Punycode» в UI.

use crate::error::{Error, Result};
use crate::punycode;

/// Преобразует домен в ASCII-форму (IDNA `ToASCII`).
///
/// Разбивает по `.`, каждую метку: если все ASCII — копирует,
/// иначе кодирует Punycode и добавляет префикс `xn--`. Пустой
/// домен возвращает пустую строку. Метки lowercase'ятся.
pub fn domain_to_ascii(domain: &str) -> Result<String> {
    if domain.is_empty() {
        return Ok(String::new());
    }

    let lowered = domain.to_lowercase();
    let mut parts: Vec<String> = Vec::new();
    for label in lowered.split('.') {
        parts.push(label_to_ascii(label)?);
    }
    Ok(parts.join("."))
}

fn label_to_ascii(label: &str) -> Result<String> {
    if label.is_empty() {
        return Ok(String::new());
    }
    if label.is_ascii() {
        return Ok(label.to_string());
    }
    // Дважды encode'нная метка (уже `xn--…` с не-ASCII символами) — это
    // ошибка ввода. Здесь её не случится, потому что выше проверка is_ascii().
    let encoded = punycode::encode(label)?;
    Ok(format!("xn--{encoded}"))
}

/// Идемпотентная версия [`domain_to_ascii`] — если вход уже ASCII (например,
/// `xn--p1ai`), возвращается как есть после lowercase. Полезна, когда вход
/// может быть в любой форме.
pub fn ensure_ascii(domain: &str) -> Result<String> {
    domain_to_ascii(domain)
}

/// Ошибка для случаев, когда метка не может быть закодирована. Пока
/// инкапсулирована в общий [`Error`] — этот alias на будущее.
pub type IdnError = Error;

/// Причина, по которой [`display_host`] решил не показывать Unicode-форму
/// хоста, а вернуть Punycode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpoofReason {
    /// Метка смешивает латинские буквы с кириллическими/греческими в
    /// пределах одной метки (например, `pаypal` — латинские `p`/`y`/`l` и
    /// кириллическая `а`).
    MixedScript,
    /// Метка целиком состоит из кириллических букв, каждая из которых
    /// имеет латинский омоглиф (например, `сор` читается как `cop`), а TLD —
    /// латинский/ASCII.
    ConfusableLabel,
}

/// Решение о том, как показать хост пользователю: как есть (Unicode) или в
/// Punycode, потому что Unicode-форма рискует визуально подделать другой
/// домен (IDN homograph attack).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostDisplay {
    /// Безопасно показать в исходном/декодированном Unicode-виде.
    Unicode(String),
    /// Небезопасно — показать ASCII-форму `xn--…` вместо Unicode.
    Punycode {
        /// ASCII-форма хоста (`domain_to_ascii`).
        ascii: String,
        /// Почему решили не показывать Unicode.
        reason: SpoofReason,
    },
}

/// Кириллические буквы с распространённым латинским омоглифом (внешне
/// неотличимы в большинстве шрифтов UI). Источник: подмножество таблицы
/// Unicode confusables — <https://www.unicode.org/Public/security/latest/confusables.txt>.
const CYRILLIC_LATIN_HOMOGLYPHS: &[char] = &[
    'а', // CYRILLIC A U+0430 ~ Latin a
    'е', // CYRILLIC IE U+0435 ~ Latin e
    'о', // CYRILLIC O U+043E ~ Latin o
    'р', // CYRILLIC ER U+0440 ~ Latin p
    'с', // CYRILLIC ES U+0441 ~ Latin c
    'х', // CYRILLIC HA U+0445 ~ Latin x
    'у', // CYRILLIC U U+0443 ~ Latin y
    'і', // CYRILLIC BYELORUSSIAN-UKRAINIAN I U+0456 ~ Latin i
    'ѕ', // CYRILLIC DZE U+0455 ~ Latin s
    'ј', // CYRILLIC JE U+0458 ~ Latin j
    'ԁ', // CYRILLIC KOMI DE U+0501 ~ Latin d
    'и', // CYRILLIC I U+0438 ~ Latin n (italic lookalike)
];

fn is_cyrillic(c: char) -> bool {
    matches!(c as u32, 0x0400..=0x04FF)
}

fn is_greek(c: char) -> bool {
    matches!(c as u32, 0x0370..=0x03FF)
}

fn has_cyrillic_homoglyph(c: char) -> bool {
    CYRILLIC_LATIN_HOMOGLYPHS.contains(&c)
}

/// Правило (а): метка смешивает латиницу с кириллицей/греческим.
fn label_is_mixed_script(label: &str) -> bool {
    let has_latin = label.chars().any(|c| c.is_ascii_alphabetic());
    let has_other_script = label.chars().any(|c| is_cyrillic(c) || is_greek(c));
    has_latin && has_other_script
}

/// Правило (б): метка целиком из кириллических букв, каждая из которых
/// имеет латинский омоглиф.
fn label_is_confusable_cyrillic(label: &str) -> bool {
    !label.is_empty() && label.chars().all(has_cyrillic_homoglyph)
}

/// Решает, безопасно ли показать хост в Unicode, или его нужно показать в
/// Punycode из-за риска визуальной подмены (детектор омоглифов/mixed-script).
///
/// Правила (детерминированные, Chromium-lite, без полного UTS #39):
///   (а) метка смешивает латиницу с кириллицей/греческим → spoof;
///   (б) метка целиком из кириллических омоглифов латиницы, а TLD —
///       латинский/ASCII → spoof;
///   (в) чистая кириллица с кириллическим TLD (`.рф`) — не spoof.
///
/// ASCII-хосты и пустая строка возвращаются как есть (`HostDisplay::Unicode`)
/// без дальнейшего анализа.
pub fn display_host(host: &str) -> HostDisplay {
    if host.is_empty() || host.is_ascii() {
        return HostDisplay::Unicode(host.to_string());
    }

    let lowered = host.to_lowercase();
    let labels: Vec<&str> = lowered.split('.').collect();
    let tld = labels.last().copied().unwrap_or("");
    let tld_is_ascii = tld.is_ascii();

    let reason = labels
        .iter()
        .find(|label| label_is_mixed_script(label))
        .map(|_| SpoofReason::MixedScript)
        .or_else(|| {
            if !tld_is_ascii {
                return None;
            }
            labels
                .iter()
                .find(|label| **label != tld && label_is_confusable_cyrillic(label))
                .map(|_| SpoofReason::ConfusableLabel)
        });

    match reason {
        Some(reason) => match domain_to_ascii(host) {
            Ok(ascii) => HostDisplay::Punycode { ascii, reason },
            Err(_) => HostDisplay::Unicode(host.to_string()),
        },
        None => HostDisplay::Unicode(host.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_domain() {
        assert_eq!(domain_to_ascii("").unwrap(), "");
    }

    #[test]
    fn pure_ascii_passthrough() {
        assert_eq!(domain_to_ascii("example.com").unwrap(), "example.com");
    }

    #[test]
    fn ascii_already_lowercase_unchanged() {
        assert_eq!(domain_to_ascii("Sub.Example.COM").unwrap(), "sub.example.com");
    }

    #[test]
    fn rf_tld() {
        assert_eq!(domain_to_ascii("президент.рф").unwrap(), "xn--d1abbgf6aiiy.xn--p1ai");
    }

    #[test]
    fn mixed_ascii_and_idn_labels() {
        // ASCII-поддомен + IDN-домен + IDN-TLD.
        assert_eq!(
            domain_to_ascii("api.пример.рф").unwrap(),
            "api.xn--e1afmkfd.xn--p1ai"
        );
    }

    #[test]
    fn already_punycode_passthrough() {
        // Если вход уже в xn-- форме — это просто ASCII, copy-as-is.
        assert_eq!(
            domain_to_ascii("xn--d1abbgf6aiiy.xn--p1ai").unwrap(),
            "xn--d1abbgf6aiiy.xn--p1ai"
        );
    }

    #[test]
    fn cyrillic_case_normalized() {
        // ПРЕЗИДЕНТ.РФ — кириллица сама лоуэркейстится при punycode encode
        // (мы не делаем NFC, но to_lowercase для basic Cyrillic работает).
        assert_eq!(
            domain_to_ascii("ПРЕЗИДЕНТ.РФ").unwrap(),
            "xn--d1abbgf6aiiy.xn--p1ai"
        );
    }

    #[test]
    fn trailing_dot() {
        // FQDN с trailing dot: пустая последняя метка сохраняется.
        assert_eq!(domain_to_ascii("example.com.").unwrap(), "example.com.");
    }

    #[test]
    fn ensure_ascii_for_already_ascii() {
        assert_eq!(ensure_ascii("example.com").unwrap(), "example.com");
    }

    #[test]
    fn ensure_ascii_for_idn() {
        assert_eq!(ensure_ascii("рф").unwrap(), "xn--p1ai");
    }

    #[test]
    fn display_host_mixed_script_apple_spoof() {
        // "аpple.com" — кириллическая «а» + латинские "pple" в одной метке.
        match display_host("аpple.com") {
            HostDisplay::Punycode { ascii, reason } => {
                assert_eq!(ascii, "xn--pple-43d.com");
                assert_eq!(reason, SpoofReason::MixedScript);
            }
            other => panic!("expected Punycode, got {other:?}"),
        }
    }

    #[test]
    fn display_host_pure_ascii_unicode() {
        assert_eq!(
            display_host("google.com"),
            HostDisplay::Unicode("google.com".to_string())
        );
    }

    #[test]
    fn display_host_pure_cyrillic_with_rf_tld_unicode() {
        assert_eq!(
            display_host("яндекс.рф"),
            HostDisplay::Unicode("яндекс.рф".to_string())
        );
    }

    #[test]
    fn display_host_mixed_script_paypal_spoof() {
        match display_host("раураl.com") {
            HostDisplay::Punycode { reason, .. } => {
                assert_eq!(reason, SpoofReason::MixedScript);
            }
            other => panic!("expected Punycode, got {other:?}"),
        }
    }

    #[test]
    fn display_host_confusable_all_cyrillic_label_with_latin_tld() {
        // "сор" — все буквы кириллические омоглифы латиницы (c/o/p), TLD .com латинский.
        match display_host("сор.com") {
            HostDisplay::Punycode { reason, .. } => {
                assert_eq!(reason, SpoofReason::ConfusableLabel);
            }
            other => panic!("expected Punycode, got {other:?}"),
        }
    }

    #[test]
    fn display_host_empty_or_ascii_as_is() {
        assert_eq!(display_host(""), HostDisplay::Unicode(String::new()));
        assert_eq!(
            display_host("Sub.Example.COM"),
            HostDisplay::Unicode("Sub.Example.COM".to_string())
        );
    }
}
