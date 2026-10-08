//! Грамматика свойств CSS Text для inline-`style` CSSOM (BUG-1325).
//!
//! `element.style.<p> = v` и `style=""` проверяют значение по грамматике свойства и хранят
//! каноническую запись (CSSOM §serialize a CSS value): неверное значение отбрасывается, а не
//! сохраняется как есть. Для ключевых слов этого хватает общего `canonical_specified_keyword`;
//! здесь — свойства со значениями-длинами и составной грамматикой: `tab-size`, `letter-spacing`,
//! `word-spacing`, `text-indent`, `text-transform`. Вызывается из JS-шима
//! (`_lumen_css_canonical_text`), каскад ими не пользуется.

use super::length::{canonical_specified_length as canonical_length, split_top_level_ws};
use super::typography::{CaseLang, TextTransformExtra};

/// Каноническая запись значения `value` свойства `prop` либо `None`, если оно не соответствует
/// грамматике (присваивание тогда игнорируется). `None` и для свойства, которого здесь нет.
pub fn canonical_specified_text(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        "tab-size" => tab_size(v),
        // CSS Text L4 §11.2/§11.3: `normal | <length-percentage>`; длина может быть отрицательной.
        "letter-spacing" | "word-spacing" => {
            if v.eq_ignore_ascii_case("normal") {
                Some("normal".into())
            } else {
                canonical_length_sorted(v, false, false)
            }
        }
        "text-indent" => text_indent(v),
        "text-transform" => {
            let (case, extra) = TextTransformExtra::parse(v)?;
            Some(extra.serialize(case))
        }
        _ => None,
    }
}

/// `text-transform` над `text` по тому же коду, что и раскладка: `value` — computed-запись
/// свойства (`capitalize`, `uppercase full-width`, …), `lang` — `lang`/`xml:lang` элемента.
/// Недопустимое `value` оставляет текст как есть. Нужна `innerText`/выделению, чтобы они
/// совпадали с нарисованным текстом (BUG-1329).
pub fn transform_text_by_value(value: &str, lang: &str, text: &str) -> String {
    match TextTransformExtra::parse(value.trim()) {
        Some((case, extra)) => extra.apply(case, CaseLang::from_tag(lang), text),
        None => text.to_string(),
    }
}

/// `canonical_specified_length`, а у суммы `calc()` — слагаемые в порядке CSS Values 4 §10.10:
/// проценты, затем единицы по алфавиту; одинаковые единицы сложены, вычитание записано как
/// сложение отрицательного (`calc(2ch - 30%)` → `calc(-30% + 2ch)`). Любое выражение, кроме
/// чистой суммы длин (`*`, `/`, `min()`, вложенные скобки), остаётся как есть.
fn canonical_length_sorted(v: &str, auto: bool, non_negative: bool) -> Option<String> {
    let c = canonical_length(v, auto, non_negative)?;
    Some(sort_calc_sum(&c).unwrap_or(c))
}

fn sort_calc_sum(css: &str) -> Option<String> {
    let inner = css.strip_prefix("calc(")?.strip_suffix(')')?;
    if inner.contains(['(', '*', '/']) {
        return None;
    }
    let mut terms: Vec<(String, f32)> = Vec::new();
    let mut sign = 1.0_f32;
    let mut expect_operand = true;
    for tok in inner.split_whitespace() {
        if expect_operand {
            let split = tok.find(|c: char| c.is_ascii_alphabetic() || c == '%')?;
            let (num, unit) = tok.split_at(split);
            let value = num.parse::<f32>().ok()? * sign;
            match terms.iter_mut().find(|(u, _)| u == unit) {
                Some((_, acc)) => *acc += value,
                None => terms.push((unit.to_string(), value)),
            }
            expect_operand = false;
        } else {
            sign = match tok {
                "+" => 1.0,
                "-" => -1.0,
                _ => return None,
            };
            expect_operand = true;
        }
    }
    if expect_operand || terms.len() < 2 {
        return None;
    }
    // `%` сортируется раньше букв.
    terms.sort_by(|a, b| (a.0 != "%", &a.0).cmp(&(b.0 != "%", &b.0)));
    let body: Vec<String> = terms.iter().map(|(u, v)| format!("{v}{u}")).collect();
    Some(format!("calc({})", body.join(" + ")))
}

/// CSS Text L3 §10.1: `<number [0,∞]> | <length [0,∞]>`. Процент невалиден.
fn tab_size(v: &str) -> Option<String> {
    if let Ok(n) = v.parse::<f32>() {
        return (n >= 0.0 && n.is_finite()).then(|| format!("{n}"));
    }
    let len = canonical_length_sorted(v, false, true)?;
    (!len.contains('%')).then_some(len)
}

/// CSS Text L3 §7.1: `<length-percentage> && hanging? && each-line?` — порядок записи свободный,
/// каноническая запись: длина, `hanging`, `each-line`.
fn text_indent(v: &str) -> Option<String> {
    let mut hanging = false;
    let mut each_line = false;
    let mut length: Option<&str> = None;
    for tok in split_top_level_ws(v) {
        if tok.eq_ignore_ascii_case("hanging") {
            if std::mem::replace(&mut hanging, true) {
                return None;
            }
        } else if tok.eq_ignore_ascii_case("each-line") {
            if std::mem::replace(&mut each_line, true) {
                return None;
            }
        } else if length.replace(tok).is_some() {
            return None;
        }
    }
    let mut out = canonical_length_sorted(length?, false, false)?;
    if hanging {
        out.push_str(" hanging");
    }
    if each_line {
        out.push_str(" each-line");
    }
    Some(out)
}
