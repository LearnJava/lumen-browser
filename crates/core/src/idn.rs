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
}
