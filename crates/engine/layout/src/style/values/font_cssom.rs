//! Грамматика CSS Fonts для inline-`style` CSSOM (BUG-1408).
//!
//! `element.style.<p> = v` проверяет значение по грамматике свойства и хранит каноническую
//! запись (CSSOM §serialize a CSS value); недопустимое значение отбрасывается. Срез 2 —
//! свойства-ключевые-слова: `font-kerning`, `font-optical-sizing`, `font-palette`,
//! `font-synthesis` и его четыре лонгхенда, `font-variant-{caps,ligatures,numeric,east-asian,
//! position,emoji}`. Значения-списки пишутся в порядке спецификации, одиночные ключевые
//! слова — в нижнем регистре. Вызывается из JS-шима (`_lumen_css_canonical_font`).

use super::background_cssom::one_of;

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
        _ => None,
    }
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
