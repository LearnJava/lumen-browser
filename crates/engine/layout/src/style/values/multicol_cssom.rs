//! CSSOM-представление свойств многоколоночности: разбор `element.style.<prop> = …` и значения
//! `getComputedStyle()` (BUG-1402).
//!
//! Грамматики (CSS Multi-column L1 §3, L2 §4):
//! `column-count: auto | <integer [1,∞]>`, `column-width: auto | <length [0,∞]>`,
//! `column-height: auto | <length [0,∞]>` (проценты запрещены),
//! `columns: [ <column-width> || <column-count> ] [ / <column-height> ]?`.
//! Шорткат сворачивается до кратчайшей формы: `auto`-части опускаются, ширина идёт перед числом
//! колонок, пустой `/ auto` отбрасывается.

use std::collections::HashMap;

use crate::grid_computed::absolute_length;
use crate::selector_query::length_to_css;
use crate::style::shorthand::split_top_level_ws;
use crate::style::values::length::parse_overflow_clip_margin_length;
use crate::style::ComputedStyle;

/// `<length [0,∞]>` без процентов в каноническом виде (`0` → `0px`).
fn canonical_length(tok: &str) -> Option<String> {
    parse_overflow_clip_margin_length(tok).map(|l| length_to_css(&l))
}

/// `<integer [1,∞]>`: только цифры, без знака и дробной части.
fn parse_count(tok: &str) -> Option<u32> {
    if tok.is_empty() || !tok.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tok.parse::<u32>().ok().filter(|n| *n > 0)
}

/// Делит `columns` по `/` на нулевой глубине скобок: `(голова, Some(хвост))` либо `(всё, None)`.
/// Второй `/` на верхнем уровне — ошибка грамматики (`None`).
fn split_slash(s: &str) -> Option<(&str, Option<&str>)> {
    let mut depth = 0i32;
    let mut slash = None;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '/' if depth == 0 => {
                if slash.is_some() {
                    return None;
                }
                slash = Some(i);
            }
            _ => {}
        }
    }
    Some(match slash {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    })
}

/// Собирает значение шортката из уже канонических частей. Пустые (`auto`) части опускаются;
/// высота без ширины и числа колонок даёт `auto / <высота>`.
fn join_columns(width: Option<&str>, count: Option<&str>, height: Option<&str>) -> String {
    let mut head = String::new();
    for part in [width, count].into_iter().flatten() {
        if !head.is_empty() {
            head.push(' ');
        }
        head.push_str(part);
    }
    if head.is_empty() {
        head.push_str("auto");
    }
    match height {
        Some(h) => format!("{head} / {h}"),
        None => head,
    }
}

/// Разбор `columns`: `Some((ширина, число, высота))` в канонических строках (`None` в кортеже — `auto`).
#[allow(clippy::type_complexity)]
fn parse_columns(s: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let (head, tail) = split_slash(s)?;
    let height = match tail.map(str::trim) {
        None => None,
        Some(t) if t.eq_ignore_ascii_case("auto") => None,
        Some(t) => Some(canonical_length(t)?),
    };
    let tokens = split_top_level_ws(head.trim());
    if tokens.is_empty() || tokens.len() > 2 {
        return None;
    }
    let (mut width, mut count) = (None, None);
    for tok in tokens {
        if tok.eq_ignore_ascii_case("auto") {
            // `auto` занимает любое из двух мест: «auto auto» допустимо, а три токена
            // («10px auto auto») отсечены выше.
            continue;
        }
        if let Some(n) = parse_count(tok) {
            if count.is_some() {
                return None;
            }
            count = Some(n.to_string());
        } else {
            // Ни `auto`, ни число — только длина; две длины или два числа недопустимы.
            let w = canonical_length(tok)?;
            if width.is_some() {
                return None;
            }
            width = Some(w);
        }
    }
    Some((width, count, height))
}

/// CSSOM: проверка и каноническая запись значения `column-count`, `column-width`,
/// `column-height` или `columns` для inline-`style`. `None` — значение недопустимо, присваивание
/// игнорируется.
pub fn canonical_specified_multicol(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        "column-count" => {
            if v.eq_ignore_ascii_case("auto") {
                Some("auto".to_string())
            } else {
                parse_count(v).map(|n| n.to_string())
            }
        }
        "column-width" | "column-height" => {
            if v.eq_ignore_ascii_case("auto") {
                Some("auto".to_string())
            } else {
                canonical_length(v)
            }
        }
        "columns" => {
            let (w, c, h) = parse_columns(v)?;
            Some(join_columns(w.as_deref(), c.as_deref(), h.as_deref()))
        }
        _ => None,
    }
}

/// Вставляет в `m` вычисленные значения `column-count/width/height/fill/span/wrap` и шортката
/// `columns` (`column-gap` кладёт [`crate::grid_computed::insert_grid_computed`]).
pub(crate) fn insert_multicol_computed(style: &ComputedStyle, m: &mut HashMap<String, String>) {
    let em = style.font_size;
    let abs = |l: &Option<crate::style::Length>| l.as_ref().map(|l| absolute_length(l, em, 1.0));
    let width = abs(&style.column_width);
    let height = abs(&style.column_height);
    let count = style.column_count.map(|n| n.to_string());

    m.insert("column-count".into(), count.clone().unwrap_or_else(|| "auto".into()));
    m.insert("column-width".into(), width.clone().unwrap_or_else(|| "auto".into()));
    m.insert("column-height".into(), height.clone().unwrap_or_else(|| "auto".into()));
    m.insert(
        "column-fill".into(),
        if style.column_fill_balance_all {
            "balance-all"
        } else if style.column_fill_balance {
            "balance"
        } else {
            "auto"
        }
        .into(),
    );
    m.insert("column-span".into(), if style.column_span_all { "all" } else { "none" }.into());
    // CSS Fragmentation L3 §5.1 — grouped here with the other fragmentation-related longhands.
    let bdb = if style.box_decoration_clone { "clone" } else { "slice" };
    m.insert("box-decoration-break".into(), bdb.into());
    m.insert("-webkit-box-decoration-break".into(), bdb.into());
    m.insert(
        "column-wrap".into(),
        if style.column_wrap_nowrap {
            "nowrap"
        } else if style.column_wrap_explicit {
            "wrap"
        } else {
            "auto"
        }
        .into(),
    );
    m.insert("columns".into(), join_columns(width.as_deref(), count.as_deref(), height.as_deref()));
}
