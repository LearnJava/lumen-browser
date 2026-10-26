//! Грамматика свойств CSS Text для inline-`style` CSSOM (BUG-1325).
//!
//! `element.style.<p> = v` и `style=""` проверяют значение по грамматике свойства и хранят
//! каноническую запись (CSSOM §serialize a CSS value): неверное значение отбрасывается, а не
//! сохраняется как есть. Для ключевых слов этого хватает общего `canonical_specified_keyword`;
//! здесь — свойства со значениями-длинами и составной грамматикой: `tab-size`, `letter-spacing`,
//! `word-spacing`, `text-indent`, `text-transform`. Вызывается из JS-шима
//! (`_lumen_css_canonical_text`), каскад ими не пользуется.

use super::length::{canonical_specified_length as canonical_length, split_top_level_ws};
use super::typography::{CaseLang, FontStretch, FontStyle, FontWeight, TextTransformExtra};
use crate::style::math_value::canonical_specified_numeric_math;
use crate::style::parse::font::parse_font_weight;

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
        "font-weight" => font_weight(v),
        "font-style" => font_style(v),
        "font-stretch" => font_stretch(v),
        "line-height" => line_height(v),
        "baseline-shift" => baseline_shift(v),
        "vertical-align" => vertical_align(v),
        "initial-letter" => initial_letter(v),
        "text-transform" => {
            let (case, extra) = TextTransformExtra::parse(v)?;
            Some(extra.serialize(case))
        }
        _ => None,
    }
}

/// Значение целиком — один вызов math-функции (`calc(…)`, `min(…)`…).
fn is_math_call(v: &str) -> bool {
    v.find('(').is_some_and(|open| v.ends_with(')') && v[..open].bytes().all(|b| b.is_ascii_alphabetic()))
}

/// `font-weight` (CSS Fonts L4 §2.2): `normal | bold | bolder | lighter | <number [1,1000]>`;
/// вызов math-функции с числовым результатом хранится как `calc(<число>)`.
fn font_weight(v: &str) -> Option<String> {
    let kw = v.to_ascii_lowercase();
    if matches!(kw.as_str(), "normal" | "bold" | "bolder" | "lighter") {
        return Some(kw);
    }
    if is_math_call(v) {
        return match canonical_specified_numeric_math("font-weight", v).as_str() {
            "" => Some(v.to_string()),
            "!" => None,
            canon => Some(canon.to_string()),
        };
    }
    let w = parse_font_weight(v, FontWeight::NORMAL)?;
    Some(w.serialize())
}

/// `font-style` (CSS Fonts L4 §2.3): `normal | italic | oblique <angle [-90deg, 90deg]>?`.
/// `oblique 0deg` — это `normal`; угол хранится в записанных единицах.
fn font_style(v: &str) -> Option<String> {
    let (kw, angle) = match v.split_once(char::is_whitespace) {
        Some((kw, rest)) => (kw.to_ascii_lowercase(), Some(rest.trim())),
        None => (v.to_ascii_lowercase(), None),
    };
    let Some(angle) = angle else {
        return matches!(kw.as_str(), "normal" | "italic" | "oblique").then_some(kw);
    };
    if kw != "oblique" {
        return None;
    }
    if is_math_call(angle) {
        return match canonical_specified_numeric_math("font-style-angle", angle).as_str() {
            "" => Some(format!("oblique {angle}")),
            "!" => None,
            canon => Some(format!("oblique {canon}")),
        };
    }
    let (style, _) = FontStyle::parse(&format!("oblique {angle}"))?;
    Some(if style == FontStyle::Normal { "normal".into() } else { format!("oblique {}", angle.to_ascii_lowercase()) })
}

/// `font-stretch` (CSS Fonts L4 §2.5): `<font-stretch-absolute>` — ключевое слово или
/// `<percentage [0, ∞)>`; число без `%` недопустимо.
fn font_stretch(v: &str) -> Option<String> {
    let kw = v.to_ascii_lowercase();
    if FontStretch::from_keyword(&kw).is_some() {
        return Some(kw);
    }
    if is_math_call(v) {
        return Some(v.to_string());
    }
    let pct = kw.strip_suffix('%')?;
    let n = pct.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0)?;
    Some(format!("{n}%"))
}

/// `line-height` (CSS Inline L3 §3.1): `normal | <number [0,∞)> | <length-percentage [0,∞)>`.
fn line_height(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("normal") {
        return Some("normal".into());
    }
    if let Ok(n) = v.parse::<f32>() {
        return (n.is_finite() && n >= 0.0).then(|| format!("{n}"));
    }
    canonical_length_sorted(v, false, true)
}

/// Ключевые слова `<'alignment-baseline'>` внутри `vertical-align` (CSS Inline L3 §4.1).
const ALIGNMENT_BASELINE_KEYWORDS: [&str; 8] = [
    "baseline", "text-bottom", "alphabetic", "ideographic", "middle", "central", "mathematical", "text-top",
];

/// `baseline-shift` (CSS Inline L3 §4.2): `<length-percentage> | sub | super | top | center | bottom`;
/// `baseline` — SVG-запись нулевого сдвига, оставлена для совместимости.
fn baseline_shift(v: &str) -> Option<String> {
    let kw = v.to_ascii_lowercase();
    if matches!(kw.as_str(), "baseline" | "sub" | "super" | "top" | "center" | "bottom") {
        return Some(kw);
    }
    canonical_length(v, false, false)
}

/// `vertical-align` (CSS Inline L3 §4): `[ first | last ] || <'alignment-baseline'> ||
/// <'baseline-shift'>`; каждая часть не более одного раза. В записи пропущены начальные
/// значения (`baseline`, нулевой сдвиг), порядок — `first|last`, выравнивание, сдвиг; пустая
/// запись — `baseline`.
fn vertical_align(v: &str) -> Option<String> {
    let (mut edge, mut align, mut shift): (Option<String>, Option<String>, Option<String>) = (None, None, None);
    for tok in split_top_level_ws(v) {
        let kw = tok.to_ascii_lowercase();
        let taken = if matches!(kw.as_str(), "first" | "last") {
            edge.replace(kw)
        } else if ALIGNMENT_BASELINE_KEYWORDS.contains(&kw.as_str()) {
            align.replace(kw)
        } else if matches!(kw.as_str(), "sub" | "super" | "top" | "center" | "bottom") {
            shift.replace(kw)
        } else {
            shift.replace(canonical_length(tok, false, false)?)
        };
        if taken.is_some() {
            return None;
        }
    }
    let parts: Vec<String> = [
        edge,
        align.filter(|a| a != "baseline"),
        shift.filter(|s| s != "0px"),
    ]
    .into_iter()
    .flatten()
    .collect();
    Some(if parts.is_empty() { "baseline".into() } else { parts.join(" ") })
}

/// `initial-letter` (CSS Inline L3 §5.1): `normal | <number [1,∞)> <integer [1,∞)> |
/// <number [1,∞)> && [ drop | raise ]?`; ключевое слово в записи идёт после числа.
fn initial_letter(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("normal") {
        return Some("normal".into());
    }
    let toks = split_top_level_ws(v);
    let is_kw = |t: &str| t.eq_ignore_ascii_case("drop") || t.eq_ignore_ascii_case("raise");
    let size = |t: &str| {
        if is_math_call(t) {
            return Some(t.to_string());
        }
        t.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 1.0).map(|n| format!("{n}"))
    };
    match toks.as_slice() {
        [n] => size(n),
        [n, k] if is_kw(k) => Some(format!("{} {}", size(n)?, k.to_ascii_lowercase())),
        [k, n] if is_kw(k) => Some(format!("{} {}", size(n)?, k.to_ascii_lowercase())),
        [n, i] => {
            let sink = if is_math_call(i) {
                i.to_string()
            } else {
                i.parse::<i64>().ok().filter(|n| *n >= 1)?.to_string()
            };
            Some(format!("{} {sink}", size(n)?))
        }
        _ => None,
    }
}

/// `text-transform` над `text` по тому же коду, что и раскладка: `value` — computed-запись
/// свойства (`capitalize`, `uppercase full-width`, …), `lang` — `lang`/`xml:lang` элемента.
/// Недопустимое `value` оставляет текст как есть. Нужна `innerText`/выделению, чтобы они
/// совпадали с нарисованным текстом (BUG-1329).
pub fn transform_text_by_value(value: &str, lang: &str, text: &str) -> String {
    transform_text_after_by_value(value, lang, "", text)
}

/// [`transform_text_by_value`] для текста, продолжающего inline-строку: `before` — текст слева
/// от `text` в той же строке (BUG-1486). `capitalize` не начинает слово посреди слова,
/// начатого соседним узлом.
pub fn transform_text_after_by_value(value: &str, lang: &str, before: &str, text: &str) -> String {
    match TextTransformExtra::parse(value.trim()) {
        Some((case, extra)) => extra.apply_after(case, CaseLang::from_tag(lang), before, text),
        None => text.to_string(),
    }
}

/// `canonical_specified_length`, а у суммы `calc()` — слагаемые в порядке CSS Values 4 §10.10:
/// проценты, затем единицы по алфавиту; одинаковые единицы сложены, вычитание записано как
/// сложение отрицательного (`calc(2ch - 30%)` → `calc(-30% + 2ch)`). Любое выражение, кроме
/// чистой суммы длин (`*`, `/`, `min()`, вложенные скобки), остаётся как есть.
pub(super) fn canonical_length_sorted(v: &str, auto: bool, non_negative: bool) -> Option<String> {
    let c = canonical_length(v, auto, non_negative)?;
    Some(sort_calc_sum(&c).unwrap_or(c))
}

pub(in crate::style) fn sort_calc_sum(css: &str) -> Option<String> {
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
    // Слагаемое после первого — `+ x`, а отрицательное записано вычитанием (`calc(1em - 2px)`).
    let mut body = String::new();
    for (i, (u, v)) in terms.iter().enumerate() {
        match (i, *v < 0.0) {
            (0, _) => body.push_str(&format!("{v}{u}")),
            (_, true) => body.push_str(&format!(" - {}{u}", -v)),
            (_, false) => body.push_str(&format!(" + {v}{u}")),
        }
    }
    Some(format!("calc({body})"))
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
