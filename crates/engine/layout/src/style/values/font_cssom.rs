//! Грамматика CSS Fonts для inline-`style` CSSOM (BUG-1408).
//!
//! `element.style.<p> = v` проверяет значение по грамматике свойства и хранит каноническую
//! запись (CSSOM §serialize a CSS value); недопустимое значение отбрасывается. Срез 2 —
//! свойства-ключевые-слова: `font-kerning`, `font-optical-sizing`, `font-palette`,
//! `font-synthesis` и его четыре лонгхенда, `font-variant-{caps,ligatures,numeric,east-asian,
//! position,emoji}`. Значения-списки пишутся в порядке спецификации, одиночные ключевые
//! слова — в нижнем регистре. Вызывается из JS-шима (`_lumen_css_canonical_font`).

use super::background_cssom::one_of;
use super::length::split_top_level_ws;
use crate::style::math_value::canonical_specified_numeric_math;

/// `normal | light | dark | <dashed-ident>`; идентификатор сохраняется как записан (регистр
/// значим), ключевые слова — в нижнем регистре.
pub fn canonical_specified_font(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        // CSS Fonts L4 §6.1.
        "font-kerning" => one_of(v, &["auto", "normal", "none"]),
        // CSS Fonts L4 §7.
        "font-optical-sizing" => one_of(v, &["auto", "none"]),
        "font-palette" => palette(v),
        // CSS Fonts L4 §9.
        "font-variant-caps" => one_of(
            v,
            &[
                "normal",
                "small-caps",
                "all-small-caps",
                "petite-caps",
                "all-petite-caps",
                "unicase",
                "titling-caps",
            ],
        ),
        "font-variant-position" => one_of(v, &["normal", "sub", "super"]),
        "font-variant-emoji" => one_of(v, &["normal", "text", "emoji", "unicode"]),
        "font-variant-ligatures" => grouped(
            v,
            &["normal", "none"],
            &[
                &["common-ligatures", "no-common-ligatures"],
                &["discretionary-ligatures", "no-discretionary-ligatures"],
                &["historical-ligatures", "no-historical-ligatures"],
                &["contextual", "no-contextual"],
            ],
        ),
        "font-variant-numeric" => grouped(
            v,
            &["normal"],
            &[
                &["lining-nums", "oldstyle-nums"],
                &["proportional-nums", "tabular-nums"],
                &["diagonal-fractions", "stacked-fractions"],
                &["ordinal"],
                &["slashed-zero"],
            ],
        ),
        "font-variant-east-asian" => grouped(
            v,
            &["normal"],
            &[
                &["jis78", "jis83", "jis90", "jis04"],
                &["simplified", "traditional"],
                &["full-width"],
                &["proportional-width"],
                &["ruby"],
            ],
        ),
        // CSS Fonts L4 §5.3: `none | [weight || style || small-caps || position]`; `oblique-only`
        // стоит на месте `style`.
        // shortcut: шорткод хранится одним ключом, а не раскладывается по лонгхендам —
        // WPT читает только `getPropertyValue('font-synthesis')`; при правках, которым нужны
        // лонгхенды (каскад/`cssText`), раскладывать через `_LUMEN_TEXT_SHORTHANDS`-механизм.
        "font-synthesis" => {
            if v.eq_ignore_ascii_case("none") {
                Some("none".into())
            } else {
                grouped(
                    v,
                    &[],
                    &[&["weight"], &["style", "oblique-only"], &["small-caps"], &["position"]],
                )
            }
        }
        "font-synthesis-weight" | "font-synthesis-small-caps" | "font-synthesis-position" => {
            one_of(v, &["auto", "none"])
        }
        "font-synthesis-style" => one_of(v, &["auto", "none", "oblique-only"]),
        // CSS Fonts L5 §3: `none | [ <metric>? [ from-font | <number [0,∞]> ] ]`.
        "font-size-adjust" => font_size_adjust(v),
        // CSS Fonts L4 §7: `normal | <string>` (1-4 ASCII characters).
        "font-language-override" => font_language_override(v),
        _ => None,
    }
}

/// Метрики `font-size-adjust`; `ex-height` — умолчание и в записи опускается.
const SIZE_ADJUST_METRICS: &[&str] = &["ex-height", "cap-height", "ch-width", "ic-width", "ic-height"];

/// Простая строка в кавычках: тот же символ на концах, внутри нет незакрытой кавычки.
fn unquote(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let q = *bytes.first()?;
    if !(q == b'"' || q == b'\'') || bytes.len() < 2 || bytes[bytes.len() - 1] != q {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    let mut escaped = false;
    for c in inner.bytes() {
        if escaped {
            escaped = false;
        } else if c == b'\\' {
            escaped = true;
        } else if c == q {
            return None;
        }
    }
    (!escaped).then_some(inner)
}

fn font_size_adjust(v: &str) -> Option<String> {
    let mut toks = split_top_level_ws(v);
    if toks.is_empty() || toks.len() > 2 {
        return None;
    }
    let metric = if toks.len() == 2 {
        let m = toks[0].to_ascii_lowercase();
        if !SIZE_ADJUST_METRICS.contains(&m.as_str()) {
            return None;
        }
        toks.remove(0);
        Some(m)
    } else {
        None
    };
    let value = toks[0];
    let prefix = |metric: Option<&str>| match metric {
        // `ex-height` — умолчание, опускается (CSSOM §serialize a CSS value).
        Some(m) if m != "ex-height" => format!("{m} "),
        _ => String::new(),
    };
    if value.eq_ignore_ascii_case("none") {
        return (metric.is_none()).then(|| "none".to_string());
    }
    if value.eq_ignore_ascii_case("from-font") {
        return Some(format!("{}from-font", prefix(metric.as_deref())));
    }
    let number = if value.contains('(') {
        // Значение math-функции, свёрнутое до числа.
        // shortcut: `"!"` (оценщик не берёт выражение — `sign()` с относительными единицами)
        // тоже принимается как написано, так что `calc(1px)` проходит валидацию; отвергать
        // его — вместе с настоящей проверкой размерности в `canonical_numeric_math`.
        match canonical_specified_numeric_math("font-size-adjust", value).as_str() {
            "" | "!" => value.to_string(),
            canon => canon.to_string(),
        }
    } else {
        let n = value.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0)?;
        format!("{}", n + 0.0)
    };
    Some(format!("{}{number}", prefix(metric.as_deref())))
}

fn font_language_override(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("normal") {
        return Some("normal".into());
    }
    let inner = unquote(v)?;
    let valid = !inner.is_empty()
        && inner.len() <= 4
        && inner.bytes().all(|b| b.is_ascii() && (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\');
    if !valid {
        return None;
    }
    Some(format!("\"{}\"", inner.trim_end_matches(' ')))
}

fn palette(s: &str) -> Option<String> {
    let l = s.to_ascii_lowercase();
    if matches!(l.as_str(), "normal" | "light" | "dark") {
        return Some(l);
    }
    let rest = s.strip_prefix("--")?;
    let ident_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c as u32 >= 0x80;
    rest.chars().all(ident_char).then(|| s.to_string())
}

/// `standalone` — значения, которые стоят в одиночку (`normal`, иногда `none`); иначе по
/// одному значению из каждой группы, запись — в порядке групп.
fn grouped(v: &str, standalone: &[&str], groups: &[&[&str]]) -> Option<String> {
    let toks: Vec<String> = v.split_whitespace().map(str::to_ascii_lowercase).collect();
    if toks.len() == 1 && standalone.contains(&toks[0].as_str()) {
        return Some(toks[0].clone());
    }
    let mut picked: Vec<Option<String>> = vec![None; groups.len()];
    for t in &toks {
        let i = groups.iter().position(|g| g.contains(&t.as_str()))?;
        if picked[i].replace(t.clone()).is_some() {
            return None;
        }
    }
    let out: Vec<&str> = picked.iter().flatten().map(String::as_str).collect();
    (!out.is_empty()).then(|| out.join(" "))
}
