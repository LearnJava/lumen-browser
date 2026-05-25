#!/usr/bin/env python3
"""Generate crates/engine/html-parser/src/entities.rs from WHATWG entities.json."""
import urllib.request
import json
import sys
import os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "crates", "engine", "html-parser", "src", "entities.rs")

url = "https://html.spec.whatwg.org/entities.json"
print(f"Fetching {url} ...", file=sys.stderr)
with urllib.request.urlopen(url, timeout=30) as r:
    data = json.load(r)

entries = [(name.lstrip("&"), info["characters"]) for name, info in data.items() if name.endswith(";")]
# Deduplicate (WHATWG JSON has both "&amp;" and "amp;" as keys sometimes)
seen = {}
for name, chars in entries:
    if name not in seen:
        seen[name] = chars
entries = sorted(seen.items(), key=lambda x: x[0])
print(f"Entries with semicolon: {len(entries)}", file=sys.stderr)

def to_rust_str(chars):
    """Encode a Python string as Rust string literal body."""
    out = ""
    for c in chars:
        cp = ord(c)
        if cp == ord('"'):
            out += '\\"'
        elif cp == ord('\\'):
            out += '\\\\'
        elif cp == ord('\t'):
            out += '\\t'
        elif cp == ord('\n'):
            out += '\\n'
        elif cp == ord('\r'):
            out += '\\r'
        elif 32 <= cp < 127:
            out += c
        else:
            out += "\\u{" + f"{cp:04X}" + "}"
    return out

count = len(entries)

with open(OUT, "w", encoding="utf-8", newline="\n") as f:
    f.write(f"""\
//! HTML5 named character references -- полный набор WHATWG.
//!
//! {count} имён из [WHATWG entities table]. Бинарный поиск по
//! отсортированной таблице. Имена case-sensitive (HTML5 spec).
//! Только формы с trailing `;`. Legacy без `;` не поддерживаются.
//!
//! [WHATWG entities table]: https://html.spec.whatwg.org/multipage/named-characters.html

/// Поиск named character reference по имени (без ведущего `&`, с
/// trailing `;`). Возвращает декодированную строку или `None`, если
/// имя неизвестно.
///
/// Реализация -- бинарный поиск по сортированной таблице. Имена
/// case-sensitive (HTML5 spec).
pub(crate) fn lookup_named_entity(name_with_semicolon: &str) -> Option<&'static str> {{
    NAMED_ENTITIES
        .binary_search_by_key(&name_with_semicolon, |&(k, _)| k)
        .ok()
        .map(|i| NAMED_ENTITIES[i].1)
}}

/// Таблица (name-with-semicolon, decoded-string). Отсортирована по
/// первому столбцу для бинарного поиска. Проверяется тестом ниже.
static NAMED_ENTITIES: &[(&str, &str)] = &[
""")

    for name, chars in entries:
        rust_val = to_rust_str(chars)
        f.write(f'    ("{name}", "{rust_val}"),\n')

    f.write(f"""\
];

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn table_is_sorted_for_binary_search() {{
        for pair in NAMED_ENTITIES.windows(2) {{
            assert!(
                pair[0].0 < pair[1].0,
                "NAMED_ENTITIES не отсортирована: {{:?}} >= {{:?}}",
                pair[0].0,
                pair[1].0
            );
        }}
    }}

    #[test]
    fn table_count() {{
        assert_eq!(NAMED_ENTITIES.len(), {count});
    }}

    #[test]
    fn lookup_basic_entities() {{
        assert_eq!(lookup_named_entity("amp;"), Some("&"));
        assert_eq!(lookup_named_entity("lt;"), Some("<"));
        assert_eq!(lookup_named_entity("gt;"), Some(">"));
        assert_eq!(lookup_named_entity("quot;"), Some("\\""));
        assert_eq!(lookup_named_entity("apos;"), Some("'"));
        assert_eq!(lookup_named_entity("nbsp;"), Some("\\u{{00A0}}"));
    }}

    #[test]
    fn lookup_extended_entities() {{
        assert_eq!(lookup_named_entity("copy;"), Some("\\u{{00A9}}"));
        assert_eq!(lookup_named_entity("mdash;"), Some("\\u{{2014}}"));
        assert_eq!(lookup_named_entity("ldquo;"), Some("\\u{{201C}}"));
        assert_eq!(lookup_named_entity("hellip;"), Some("\\u{{2026}}"));
        assert_eq!(lookup_named_entity("trade;"), Some("\\u{{2122}}"));
    }}

    #[test]
    fn lookup_case_sensitive() {{
        assert_eq!(lookup_named_entity("Beta;"), Some("\\u{{0392}}"));
        assert_eq!(lookup_named_entity("beta;"), Some("\\u{{03B2}}"));
        assert_eq!(lookup_named_entity("AMP;"), Some("&"));
        assert_eq!(lookup_named_entity("GT;"), Some(">"));
        assert_eq!(lookup_named_entity("LT;"), Some("<"));
    }}

    #[test]
    fn lookup_unknown_returns_none() {{
        assert_eq!(lookup_named_entity("unknownentity;"), None);
        assert_eq!(lookup_named_entity(""), None);
    }}

    #[test]
    fn lookup_requires_semicolon() {{
        assert_eq!(lookup_named_entity("amp"), None);
    }}

    #[test]
    fn lookup_new_whatwg_entities() {{
        // Entities absent in the old HTML4 subset
        assert_eq!(lookup_named_entity("Abreve;"), Some("\\u{{0102}}"));
        assert_eq!(lookup_named_entity("Nopf;"), Some("\\u{{2115}}"));   // Double-struck capital N
        assert_eq!(lookup_named_entity("nopf;"), Some("\\u{{1D55F}}"));  // Double-struck small n
        assert_eq!(lookup_named_entity("Efr;"), Some("\\u{{1D508}}"));
        assert_eq!(lookup_named_entity("there4;"), Some("\\u{{2234}}"));
    }}
}}
""")

print(f"Written {OUT}", file=sys.stderr)
