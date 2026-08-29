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
            let quote = c;
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let literal = &s[start..i];
            out.push(ContentItem::String(literal.to_string()));
            if i < bytes.len() {
                i += 1; // closing quote
            }
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

fn parse_content_fn(name: &str, args: &str) -> Option<ContentItem> {
    match name {
        "url" => {
            let inner = args.trim().trim_matches(['"', '\''].as_ref());
            Some(ContentItem::Url(inner.to_string()))
        }
        "attr" => {
            let attr_name = args.trim().trim_matches(['"', '\''].as_ref());
            if attr_name.is_empty() {
                None
            } else {
                Some(ContentItem::Attr(attr_name.to_string()))
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
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            let counter_name = parts.first()?.to_string();
            let separator_raw = parts.get(1)?;
            let separator = separator_raw.trim_matches(['"', '\''].as_ref()).to_string();
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
