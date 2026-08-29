//! Разбор списочных значений счётчиков и кавычек: `counter-reset` /
//! `counter-increment` (CSS Lists L3 §3) и `quotes` (CSS Content L3 §3.2)
//! вместе с их лексером CSS-строк.
//!
//! Перенесено батчем SPLIT-ST5 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_counter_list` … `fn parse_css_string_sequence`) без правок тел: изменены только пути модулей и
//! видимость тех items, которые продолжают звать `style.rs`, его тест-модули
//! и соседние модули `style::parse`.

use crate::style::Quotes;

/// Парсер CSS Lists L3 §3 `counter-reset` / `counter-increment` value.
/// Формат: `none | (<custom-ident> <integer>?)+`. Возвращает `Vec` пар
/// (имя, число); `default` подставляется когда integer не указан.
///
/// `none` (case-insensitive) → пустой `Vec`. Невалидные ident-ы и числа
/// — пропускаем без ошибки, как best-effort lenient parser.
pub(in crate::style) fn parse_counter_list(value: &str, default: i32) -> Vec<(String, i32)> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") || v.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut tokens = v.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        // Имя счётчика — CSS ident: ASCII alphabetic / `_` / `-` начало,
        // дальше alphanumeric / `-` / `_`. Простой strict check; пропускаем
        // токены, не похожие на ident.
        if !is_css_ident(tok) {
            continue;
        }
        // Следующий токен — опц. integer.
        let n = if let Some(&peeked) = tokens.peek() {
            if let Ok(parsed) = peeked.parse::<i32>() {
                tokens.next();
                parsed
            } else {
                default
            }
        } else {
            default
        };
        out.push((tok.to_string(), n));
    }
    out
}

pub(in crate::style) fn is_css_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// CSS Generated Content L3 §3.2 — parse the `quotes` value.
///
/// `auto` → [`Quotes::Auto`]; `none` → [`Quotes::None`]; otherwise an even
/// number of CSS strings forms `(open, close)` pairs (outermost first).
/// Returns `None` if the value is malformed (odd string count or none found).
pub(in crate::style) fn parse_quotes(value: &str) -> Option<Quotes> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("auto") {
        return Some(Quotes::Auto);
    }
    if v.eq_ignore_ascii_case("none") {
        return Some(Quotes::None);
    }
    let strings = parse_css_string_sequence(v);
    if strings.is_empty() || !strings.len().is_multiple_of(2) {
        return None;
    }
    let pairs = strings
        .chunks_exact(2)
        .map(|c| (c[0].clone(), c[1].clone()))
        .collect();
    Some(Quotes::Pairs(pairs))
}

/// Extracts consecutive CSS string literals from `s` (single- or double-quoted),
/// unescaping `\XXXXXX` hex escapes and `\<char>` literals. Non-string tokens are
/// skipped. Used by [`parse_quotes`].
fn parse_css_string_sequence(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            i += 1;
            let mut cur = String::new();
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' {
                    i += 1;
                    let mut hex = String::new();
                    while i < chars.len() && chars[i].is_ascii_hexdigit() && hex.len() < 6 {
                        hex.push(chars[i]);
                        i += 1;
                    }
                    if !hex.is_empty() {
                        if i < chars.len() && chars[i].is_whitespace() {
                            i += 1;
                        }
                        if let Ok(code) = u32::from_str_radix(&hex, 16)
                            && let Some(ch) = char::from_u32(code)
                        {
                            cur.push(ch);
                        }
                    } else if i < chars.len() {
                        cur.push(chars[i]);
                        i += 1;
                    }
                } else {
                    cur.push(chars[i]);
                    i += 1;
                }
            }
            i += 1; // skip closing quote
            out.push(cur);
        } else {
            i += 1;
        }
    }
    out
}
