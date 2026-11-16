//! Грамматика CSS Fonts для inline-`style` CSSOM (BUG-1408).
//!
//! `element.style.<p> = v` проверяет значение по грамматике свойства и хранит каноническую
//! запись (CSSOM §serialize a CSS value); недопустимое значение отбрасывается. Срез 2 —
//! свойства-ключевые-слова: `font-kerning`, `font-optical-sizing`, `font-palette`,
//! `font-synthesis` и его четыре лонгхенда, `font-variant-{caps,ligatures,numeric,east-asian,
//! position,emoji}`. Значения-списки пишутся в порядке спецификации, одиночные ключевые
//! слова — в нижнем регистре. Срез 4 — списки тегов OpenType (`font-feature-settings`,
//! `font-variation-settings`) и `font-variant-alternates`. Вызывается из JS-шима
//! (`_lumen_css_canonical_font`).

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
        // CSS Fonts L3 §9: `normal | <feature-tag-value>#`.
        "font-feature-settings" => font_feature_settings(v),
        // CSS Fonts L4 §3: `normal | [ <string> <number> ]#`.
        "font-variation-settings" => font_variation_settings(v),
        // CSS Fonts L4 §9: `normal | [ stylistic() || historical-forms || styleset() ||
        // character-variant() || swash() || ornaments() || annotation() ]`.
        "font-variant-alternates" => font_variant_alternates(v),
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

fn font_feature_settings(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("normal") {
        return Some("normal".into());
    }
    let mut out = Vec::new();
    for layer in split_commas(v)? {
        let toks = split_ws_quoted(layer);
        if toks.is_empty() || toks.len() > 2 {
            return None;
        }
        let tag = quote_tag(&feature_tag(toks[0])?);
        // `on` и `1` — значение по умолчанию, в записи опускаются; `off` — `0`.
        let value = match toks.get(1) {
            None => None,
            Some(t) if t.eq_ignore_ascii_case("on") => None,
            Some(t) if t.eq_ignore_ascii_case("off") => Some(0i64),
            Some(t) => Some(t.parse::<i64>().ok()?),
        };
        out.push(match value {
            None | Some(1) => tag,
            Some(n) => format!("{tag} {n}"),
        });
    }
    Some(out.join(", "))
}

fn font_variation_settings(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("normal") {
        return Some("normal".into());
    }
    let mut out = Vec::new();
    for layer in split_commas(v)? {
        let toks = split_ws_quoted(layer);
        if toks.len() != 2 {
            return None;
        }
        let tag = quote_tag(&feature_tag(toks[0])?);
        out.push(format!("{tag} {}", variation_number(toks[1])?));
    }
    Some(out.join(", "))
}

const ALTERNATES_FNS: &[(&str, bool)] = &[
    ("stylistic", false),
    ("historical-forms", false),
    ("styleset", true),
    ("character-variant", true),
    ("swash", false),
    ("ornaments", false),
    ("annotation", false),
];

fn font_variant_alternates(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("normal") {
        return Some("normal".into());
    }
    let mut picked: Vec<Option<String>> = vec![None; ALTERNATES_FNS.len()];
    for tok in split_top_level_ws(v) {
        if tok.eq_ignore_ascii_case("historical-forms") {
            if picked[1].replace("historical-forms".into()).is_some() {
                return None;
            }
            continue;
        }
        let (name, args) = alternates_call(tok)?;
        // `historical-forms` — ключевое слово, не функция (`historical-forms(x)` невалидно).
        if name == "historical-forms" {
            return None;
        }
        let idx = ALTERNATES_FNS.iter().position(|(n, _)| *n == name)?;
        let args = split_commas(args)?;
        if !ALTERNATES_FNS[idx].1 && args.len() != 1 {
            return None;
        }
        if !args.iter().all(|a| is_custom_ident(a)) {
            return None;
        }
        if picked[idx].replace(format!("{name}({})", args.join(", "))).is_some() {
            return None;
        }
    }
    let out: Vec<&str> = picked.iter().flatten().map(String::as_str).collect();
    (!out.is_empty()).then(|| out.join(" "))
}

/// `name(args)` → имя в нижнем регистре и аргументы без скобок.
fn alternates_call(t: &str) -> Option<(String, &str)> {
    let open = t.find('(')?;
    if open == 0 || !t.ends_with(')') {
        return None;
    }
    let name = t[..open].to_ascii_lowercase();
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    Some((name, &t[open + 1..t.len() - 1]))
}

/// `<custom-ident>`: регистр значим, CSS-wide keywords не отвергаются
/// (shortcut: `default`/`initial`/`inherit` в `styleset()` пройдут — WPT этого не проверяет).
fn is_custom_ident(s: &str) -> bool {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80;
    match s.as_bytes() {
        [] => false,
        [first, ..] if !ident(*first) => false,
        [first, ..] if first.is_ascii_digit() => false,
        [b'-', second, ..] if second.is_ascii_digit() => false,
        [b'-'] => false,
        b => b.iter().all(|c| ident(*c)),
    }
}

/// Тег OpenType: 4 ASCII-символа из 0x20–0x7E (CSS Fonts L3 §9); escape-последовательности
/// CSS-строки развёрнуты.
fn feature_tag(tok: &str) -> Option<String> {
    let tag = css_string(tok)?;
    (tag.len() == 4 && tag.bytes().all(|b| (0x20..=0x7e).contains(&b))).then_some(tag)
}

/// Значение `<number>`: литерал канонизируется (`1e3` → `1000`), math-функция сворачивается
/// типизированным вычислителем; единицы и проценты невалидны.
fn variation_number(tok: &str) -> Option<String> {
    if tok.contains('(') {
        let canon = canonical_specified_numeric_math("font-size-adjust", tok);
        return (!canon.is_empty() && canon != "!").then_some(canon);
    }
    let n = tok.parse::<f64>().ok().filter(|n| n.is_finite())?;
    Some(format!("{n}"))
}

/// Запись тега как CSS-строки в двойных кавычках; `"` и `\` экранируются.
fn quote_tag(tag: &str) -> String {
    let mut out = String::from("\"");
    for c in tag.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// CSS `<string>`: кавычка одной из двух форм, escape-последовательности (`\22`, `\"`)
/// развёрнуты. Незакрытая кавычка и `\` в конце строки невалидны.
fn css_string(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let quote = *bytes.first()?;
    if (quote != b'"' && quote != b'\'') || bytes.len() < 2 || bytes[bytes.len() - 1] != quote {
        return None;
    }
    let mut out = String::new();
    let mut chars = s[1..s.len() - 1].chars().peekable();
    while let Some(c) = chars.next() {
        if c == quote as char {
            return None;
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let next = chars.next()?;
        if next == '\n' {
            continue;
        }
        if next.is_ascii_hexdigit() {
            let mut hex = String::from(next);
            while hex.len() < 6 {
                match chars.peek() {
                    Some(h) if h.is_ascii_hexdigit() => {
                        hex.push(*h);
                        chars.next();
                    }
                    _ => break,
                }
            }
            let cp = u32::from_str_radix(&hex, 16).ok()?;
            out.push(char::from_u32(cp).filter(|c| !c.is_control())?);
            // Один пробельный символ после hex-escape — разделитель, не часть строки.
            if matches!(chars.peek(), Some(' ' | '\t' | '\n')) {
                chars.next();
            }
        } else {
            out.push(next);
        }
    }
    Some(out)
}

/// Разбиение по запятым верхнего уровня: запятая внутри строки или скобок не делит; пустой
/// элемент списка (`a,`, `,a`, `a,,b`) делает значение невалидным.
fn split_commas(s: &str) -> Option<Vec<&str>> {
    split_top_level(s, ',', true)
}

/// Разбиение по пробелам верхнего уровня, пробел внутри `<string>` и скобок не делит.
fn split_ws_quoted(s: &str) -> Vec<&str> {
    split_top_level(s, ' ', false).unwrap_or_default()
}

fn split_top_level(s: &str, sep: char, strict: bool) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            c if c == sep && depth == 0 => {
                let tok = s[start..i].trim();
                if !tok.is_empty() {
                    out.push(tok);
                } else if strict {
                    return None;
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || depth != 0 {
        return None;
    }
    let tok = s[start..].trim();
    if !tok.is_empty() {
        out.push(tok);
    } else if strict && !out.is_empty() {
        return None;
    }
    Some(out)
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
