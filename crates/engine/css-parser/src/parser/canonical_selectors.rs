//! Каноническая запись селекторов для CSSOM (`selectorText`) — вынесено из `selectors.rs`
//! (CSSOM-10): вложенные селекторы, `&`, относительные к `:scope` селекторы в `@scope`.

use super::*;

/// Маркер, которым [`canonical_selector_list`] подменяет `&` на время разбора:
/// селекторный парсер `&` не знает (вложенность разворачивается текстом), а
/// `:is(.x)` допустим в любой позиции compound-селектора.
const NESTING_MARKER: &str = ":is(.lumen-nesting-amp)";

/// Каноническая запись списка селекторов для CSSOM (`selectorText`) или `None`,
/// если список невалиден. `nested` — тело стилевого правила: допускается
/// ведущий комбинатор (`> .a`) и `&` в любой позиции (CSS Nesting 1 §3).
pub(crate) fn canonical_selector_list(input: &str, nested: bool) -> Option<String> {
    canonical_selector_list_with(input, nested, true)
}

/// Селекторы правила внутри `@scope` без стилевого предка: относительные к `:scope`,
/// неявный `& ` не добавляется (`.b` остаётся `.b`, `> .foo` — `> .foo`).
pub(crate) fn canonical_scope_selector_list(input: &str) -> Option<String> {
    canonical_selector_list_with(input, true, false)
}

fn canonical_selector_list_with(input: &str, nested: bool, implicit_amp: bool) -> Option<String> {
    let mut out = Vec::new();
    for item in split_selector_commas(input) {
        let item = item.trim();
        if item.is_empty() {
            return None;
        }
        let (prefix, rest) = match item.chars().next() {
            Some(c @ ('>' | '+' | '~')) if nested => (format!("{c} "), item[1..].trim_start()),
            _ => (String::new(), item),
        };
        // Верхнеуровневый `&` — это `:scope` (CSS Nesting 1 §3.1): допустим и вне вложенности.
        let substituted = replace_outside_strings(rest, '&', NESTING_MARKER);
        let parsed = if rest.is_empty() || type_selector_after_nesting(&substituted) || !is_valid_selector_list(&substituted) {
            None
        } else {
            Parser::new(&substituted).parse_selector_list_strict()
        };
        match parsed {
            Some(sels) => {
                let css = sels_to_css_str(&sels);
                // Вложенный селектор без `&` и без ведущего комбинатора записывается с
                // неявным `& ` (CSS Nesting 1 §«Syntax», CSSOM-сериализация).
                let implicit = if nested && implicit_amp && (!prefix.is_empty() || !css.contains(NESTING_MARKER)) { "& " } else { "" };
                out.push(format!("{implicit}{prefix}{}", css.replace(NESTING_MARKER, "&")));
            }
            // CSS Nesting 1 §«Mixing nesting rules and declarations»: правило, которое
            // отбрасывается при прощающем разборе `:is()`/`:where()`, но содержит `&`,
            // всё равно сериализуется как написано.
            None if nested && !rest.is_empty() && rest.contains('&') && (rest.contains(":is(") || rest.contains(":where(")) => {
                out.push(format!("{prefix}{}", collapse_selector_ws(rest)));
            }
            None => return None,
        }
    }
    if out.is_empty() { None } else { Some(out.join(", ")) }
}

/// `&div` / `&*`: тип-селектор не может следовать за `&` в одном compound (CSS Nesting 1 §3.1).
fn type_selector_after_nesting(substituted: &str) -> bool {
    substituted
        .match_indices(NESTING_MARKER)
        .any(|(at, m)| matches!(substituted[at + m.len()..].chars().next(), Some(c) if c.is_ascii_alphabetic() || c == '*'))
}

fn collapse_selector_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Делит по запятым нулевой глубины (вне скобок и строк).
pub(crate) fn split_selector_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    let mut quote: Option<char> = None;
    let mut chars = input.char_indices();
    while let Some((i, c)) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '\\') => {
                chars.next();
            }
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

fn replace_outside_strings(input: &str, from: char, to: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut quote: Option<char> = None;
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') | (None, '\\') => {
                out.push(c);
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            (Some(q), c) if c == q => {
                quote = None;
                out.push(c);
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                out.push(c);
            }
            (None, c) if c == from => out.push_str(to),
            _ => out.push(c),
        }
    }
    out
}
