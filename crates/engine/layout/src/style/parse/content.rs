//! Разбор свойства `content` (CSS Content L3 §2): список `ContentItem` и
//! функциональные формы `attr()`/`counter()`/`counters()`/`url()`.
//!
//! Перенесено батчем SPLIT-ST5 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_content_items` … `fn parse_content_fn`) без правок тел: изменены только пути модулей и
//! видимость тех items, которые продолжают звать `style.rs`, его тест-модули
//! и соседние модули `style::parse`.

use crate::style::ContentItem;

/// CSS Content L3 — парсер `content` value на список `ContentItem`.
/// Whitespace-separated; кавычки литералов и `()` функций соблюдаются.
pub(in crate::style) fn parse_content_items(s: &str) -> Vec<ContentItem> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            let (literal, next) = consume_css_string(s, i);
            out.push(ContentItem::String(literal));
            i = next;
        } else if c.is_ascii_alphabetic() || c == b'-' {
            // Ident, может быть keyword (open-quote/...) или function-call.
            let name_start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-')
            {
                i += 1;
            }
            let name = s[name_start..i].to_ascii_lowercase();
            if i < bytes.len() && bytes[i] == b'(' {
                // Function call. Find matching `)`.
                i += 1;
                let args_start = i;
                let mut depth = 1usize;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        // Строка внутри аргументов (`attr("a)b")`, `counters(x, "\"")`)
                        // пропускается целиком — скобка/кавычка в ней не структурна.
                        b'"' | b'\'' => {
                            i = consume_css_string(s, i).1;
                            continue;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                let args = &s[args_start..i.saturating_sub(1)];
                if let Some(item) = parse_content_fn(&name, args) {
                    out.push(item);
                }
            } else {
                // Keyword.
                match name.as_str() {
                    "open-quote" => out.push(ContentItem::OpenQuote),
                    "close-quote" => out.push(ContentItem::CloseQuote),
                    "no-open-quote" => out.push(ContentItem::NoOpenQuote),
                    "no-close-quote" => out.push(ContentItem::NoCloseQuote),
                    _ => {}  // unknown ident — skip
                }
            }
        } else {
            i += 1;  // skip unknown character
        }
    }
    out
}

/// CSS Syntax L3 §4.3.5 «consume a string token». `s[start]` — открывающая
/// кавычка. Возвращает декодированное значение и индекс за закрывающей
/// кавычкой (или `s.len()` для незакрытой строки — по спеке значение до EOF).
/// `\`+newline — склейка строк (ничего не добавляет), `\`+1–6 hex (+ один
/// пробельный) — кодпоинт (0, суррогат, >10FFFF → U+FFFD), `\`+другое — он сам.
/// Неэкранированный перевод строки — bad-string: разбор обрывается на нём.
pub(in crate::style) fn consume_css_string(s: &str, start: usize) -> (String, usize) {
    let quote = s.as_bytes()[start] as char;
    let mut out = String::new();
    let mut it = s[start + 1..].char_indices().peekable();
    let base = start + 1;
    while let Some((off, c)) = it.next() {
        if c == quote {
            return (out, base + off + 1);
        }
        match c {
            '\n' | '\r' | '\x0C' => return (out, base + off),
            '\\' => match it.peek().copied() {
                None => {} // `\` перед EOF — ничего не добавляет
                Some((_, '\n' | '\x0C')) => {
                    it.next();
                }
                Some((_, '\r')) => {
                    it.next();
                    if matches!(it.peek(), Some((_, '\n'))) {
                        it.next();
                    }
                }
                Some((_, h)) if h.is_ascii_hexdigit() => {
                    let mut code = 0u32;
                    let mut n = 0;
                    while n < 6 {
                        match it.peek().copied() {
                            Some((_, d)) if d.is_ascii_hexdigit() => {
                                code = code * 16 + d.to_digit(16).unwrap_or(0);
                                it.next();
                                n += 1;
                            }
                            _ => break,
                        }
                    }
                    match it.peek().copied() {
                        Some((_, '\r')) => {
                            it.next();
                            if matches!(it.peek(), Some((_, '\n'))) {
                                it.next();
                            }
                        }
                        Some((_, '\t' | '\n' | '\x0C' | ' ')) => {
                            it.next();
                        }
                        _ => {}
                    }
                    out.push(char::from_u32(code).filter(|_| code != 0).unwrap_or('\u{FFFD}'));
                }
                Some((_, other)) => {
                    it.next();
                    out.push(other);
                }
            },
            _ => out.push(c),
        }
    }
    (out, s.len())
}

/// Значение строкового аргумента функции: `"a\"b"` → `a"b`; без кавычек —
/// как есть (после `trim`).
fn decode_string_arg(args: &str) -> String {
    let t = args.trim();
    if t.starts_with(['"', '\'']) {
        consume_css_string(t, 0).0
    } else {
        t.to_string()
    }
}

/// Делит аргументы по запятым вне строк (`counters(c, ",")`), обрезая пробелы.
fn split_top_level_commas(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut seg = 0;
    let mut i = 0;
    while i < args.len() {
        match args.as_bytes()[i] {
            b'"' | b'\'' => {
                i = consume_css_string(args, i).1;
                continue;
            }
            b',' => {
                parts.push(args[seg..i].trim());
                seg = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(args[seg..].trim());
    parts
}

fn parse_content_fn(name: &str, args: &str) -> Option<ContentItem> {
    match name {
        "url" => {
            Some(ContentItem::Url(decode_string_arg(args)))
        }
        "attr" => {
            let attr_name = decode_string_arg(args);
            if attr_name.is_empty() {
                None
            } else {
                Some(ContentItem::Attr(attr_name))
            }
        }
        "counter" => {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            let counter_name = parts.first()?.to_string();
            if counter_name.is_empty() {
                return None;
            }
            let style = parts.get(1).map(|s| s.to_string());
            Some(ContentItem::Counter {
                name: counter_name,
                style,
            })
        }
        "counters" => {
            let parts = split_top_level_commas(args);
            let counter_name = parts.first()?.to_string();
            let separator_raw = parts.get(1)?;
            let separator = decode_string_arg(separator_raw);
            let style = parts.get(2).map(|s| s.to_string());
            Some(ContentItem::Counters {
                name: counter_name,
                separator,
                style,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(s: &str) -> Vec<String> {
        parse_content_items(s)
            .into_iter()
            .filter_map(|i| match i {
                ContentItem::String(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hex_escape_decodes_with_optional_space() {
        assert_eq!(strings(r#""x\41 y""#), ["xAy"]);
        assert_eq!(strings(r#""x\0041y""#), ["xAy"]);
        assert_eq!(strings(r#""x\f101y""#), ["x\u{F101}y"]);
        assert_eq!(strings(r#""\201C""#), ["\u{201C}"]);
    }

    #[test]
    fn escaped_newline_and_quote() {
        assert_eq!(strings(r#""L1\A L2""#), ["L1\nL2"]);
        assert_eq!(strings(r#""x\"y""#), [r#"x"y"#]);
        assert_eq!(strings(r#"'it\'s'"#), ["it's"]);
        assert_eq!(strings("\"a\\\nb\""), ["ab"]); // `\`+newline — склейка
    }

    #[test]
    fn invalid_codepoints_become_replacement() {
        assert_eq!(strings(r#""\0""#), ["\u{FFFD}"]);
        assert_eq!(strings(r#""\D800 ""#), ["\u{FFFD}"]);
        assert_eq!(strings(r#""\110000 ""#), ["\u{FFFD}"]);
    }

    #[test]
    fn items_after_escaped_quote_survive() {
        let items = parse_content_items(r#""\"" counter(c) "!""#);
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], ContentItem::String(t) if t == "\""));
    }

    #[test]
    fn function_string_args_are_decoded() {
        let items = parse_content_items(r#"counters(c, "\2C  ") attr("a)b")"#);
        assert!(matches!(&items[0], ContentItem::Counters { separator, .. } if separator == ", "));
        assert!(matches!(&items[1], ContentItem::Attr(n) if n == "a)b"));
        let items = parse_content_items(r#"counters(c, ",")"#);
        assert!(matches!(&items[0], ContentItem::Counters { separator, .. } if separator == ","));
    }
}
