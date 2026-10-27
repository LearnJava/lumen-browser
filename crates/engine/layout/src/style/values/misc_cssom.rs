//! Каноническая запись значений `content`, `font-family`, `font-size`, `list-style-image` для
//! inline-`style` CSSOM (BUG-1551).
//!
//! `element.style.<p> = v` хранит значение в виде CSSOM §serialize a CSS value: строки — в двойных
//! кавычках, `url(a)` → `url("a")`, `counter(n, decimal)` → `counter(n)`, `.5%` → `0.5%`,
//! имя семейства из идентификаторов — без кавычек. Вызывается из JS-шима
//! (`_lumen_css_canonical_misc`); каскад этим кодом не пользуется (его разборщики мягкие).

use super::background_cssom::canonical_specified_image;
use super::text_cssom::canonical_length_sorted;
use crate::style::parse::content::consume_css_string;
use crate::style::parse::image::parse_url_with_modifiers;

/// Каноническая запись `value` свойства `prop` либо `None`, если значение не по грамматике
/// (присваивание игнорируется). `None` и для свойства, которого здесь нет.
pub fn canonical_specified_misc(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        "list-style-image" => canonical_specified_image(v),
        "font-size" => font_size(v),
        "font-family" => Some(font_family(v)),
        "content" => Some(content(v)),
        _ => None,
    }
}

/// `url("…")` вместо `url(…)`/`url('…')`; запись с модификаторами запроса сохраняет их порядок.
/// `None` — не `url()` или значение невалидно.
pub(super) fn canonical_url(s: &str) -> Option<String> {
    let (url, mods) = parse_url_with_modifiers(s)?;
    Some(format!("{}{}", url_text(&url), mods.serialize()))
}

fn url_text(url: &str) -> String {
    format!("url({})", css_string(url))
}

/// `<string>`: двойные кавычки, `"` и `\` экранированы, управляющие символы — `\<hex> `.
fn css_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => out.push_str(&format!("\\{:x} ", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

const FONT_SIZE_KEYWORDS: [&str; 11] = [
    "xx-small", "x-small", "small", "medium", "large", "x-large", "xx-large", "xxx-large", "larger", "smaller", "math",
];

/// `<absolute-size> | <relative-size> | math | <length-percentage [0,∞]>`.
fn font_size(v: &str) -> Option<String> {
    let kw = v.to_ascii_lowercase();
    if FONT_SIZE_KEYWORDS.contains(&kw.as_str()) {
        return Some(kw);
    }
    // Валидность — по общему разбору длин, но запись сохраняет единицу как написана (`10pt`, не `13.33px`).
    let canon = canonical_length_sorted(v, false, true)?;
    Some(simple_dimension(v).unwrap_or(canon))
}

/// `<number><unit>` / `<number>%` без функций: число в кратчайшей записи (`.5` → `0.5`, `-0` → `0`),
/// единица как написана. `None` — иная запись.
fn simple_dimension(v: &str) -> Option<String> {
    let split = v.find(|c: char| c.is_ascii_alphabetic() || c == '%').filter(|&i| i > 0)?;
    let (num, unit) = v.split_at(split);
    if !unit.chars().all(|c| c.is_ascii_alphabetic()) && unit != "%" {
        return None;
    }
    let n = num.parse::<f32>().ok().filter(|n| n.is_finite())?;
    Some(format!("{}{unit}", n + 0.0))
}

/// Делит `s` по `sep` вне кавычек и скобок. `sep == None` — по пробельным символам (пустые
/// элементы пропускаются).
fn split_outside(s: &str, sep: Option<char>) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut i) = (0usize, 0usize, 0usize);
    while i < s.len() {
        let c = s[i..].chars().next().unwrap_or(' ');
        match c {
            '"' | '\'' => {
                i = consume_css_string(s, i).1;
                continue;
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if depth == 0 && sep.map_or(c.is_whitespace(), |p| c == p) => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    out.push(&s[start..]);
    if sep.is_none() {
        out.retain(|t| !t.is_empty());
    }
    out
}

fn is_quoted(tok: &str) -> bool {
    tok.len() >= 2 && (tok.starts_with('"') || tok.starts_with('\'')) && consume_css_string(tok, 0).1 == tok.len()
}

const GENERIC_FAMILIES: [&str; 11] = [
    "serif", "sans-serif", "cursive", "fantasy", "monospace", "system-ui", "math", "ui-serif", "ui-sans-serif",
    "ui-monospace", "ui-rounded",
];
const WIDE_KEYWORDS: [&str; 6] = ["initial", "inherit", "unset", "revert", "revert-layer", "default"];

/// CSS Syntax §4.2 `<ident-token>` без экранирования: `-`? затем буква, `_`, не-ASCII или `-`,
/// дальше ещё и цифры.
fn is_ident(word: &str) -> bool {
    let rest = word.strip_prefix('-').unwrap_or(word);
    let mut chars = rest.chars();
    let start_ok = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '-' || !c.is_ascii());
    start_ok && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-' || !c.is_ascii())
}

/// Имя семейства в кавычках можно записать без них, если это последовательность
/// идентификаторов через одиночные пробелы и не слово-ключ (CSS Fonts L4 §2.1.1).
fn family_needs_quotes(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    GENERIC_FAMILIES.contains(&lower.as_str())
        || WIDE_KEYWORDS.contains(&lower.as_str())
        || !name.split(' ').all(is_ident)
}

/// `font-family`: список через запятую; имена из идентификаторов — без кавычек, остальные —
/// в двойных; обобщённые семейства и несколько слов без кавычек пишутся как есть (пробелы сжаты).
fn font_family(v: &str) -> String {
    let items: Vec<String> = split_outside(v, Some(','))
        .into_iter()
        .map(|item| {
            let item = item.trim();
            if is_quoted(item) {
                let name = consume_css_string(item, 0).0;
                if family_needs_quotes(&name) { css_string(&name) } else { name }
            } else {
                split_outside(item, None).join(" ")
            }
        })
        .collect();
    items.join(", ")
}

/// `content`: строки — в двойных кавычках, `url()` — со строкой, `counter()`/`counters()` без
/// стиля `decimal`; остальные компоненты остаются как написаны.
fn content(v: &str) -> String {
    let tokens: Vec<String> = split_outside(v, None).into_iter().map(content_token).collect();
    if tokens.len() == 1 && matches!(tokens[0].to_ascii_lowercase().as_str(), "normal" | "none") {
        return tokens[0].to_ascii_lowercase();
    }
    tokens.join(" ")
}

fn content_token(tok: &str) -> String {
    if is_quoted(tok) {
        return css_string(&consume_css_string(tok, 0).0);
    }
    let Some(open) = tok.find('(').filter(|_| tok.ends_with(')')) else {
        return tok.to_string();
    };
    let name = tok[..open].to_ascii_lowercase();
    let inner = &tok[open + 1..tok.len() - 1];
    match name.as_str() {
        "url" => canonical_url(tok).unwrap_or_else(|| tok.to_string()),
        "counter" | "counters" => {
            let mut args: Vec<String> =
                split_outside(inner, Some(',')).into_iter().map(|a| a.trim().to_string()).collect();
            let style_at = if name == "counter" { 1 } else { 2 };
            if args.len() == style_at + 1 && args[style_at].eq_ignore_ascii_case("decimal") {
                args.pop();
            }
            if name == "counters" && args.len() >= 2 && is_quoted(&args[1]) {
                args[1] = css_string(&consume_css_string(&args[1], 0).0);
            }
            format!("{name}({})", args.join(", "))
        }
        _ => tok.to_string(),
    }
}
