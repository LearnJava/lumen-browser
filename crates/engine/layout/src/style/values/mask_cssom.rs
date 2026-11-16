//! Грамматика CSS Masking для inline-`style` CSSOM (BUG-1408).
//!
//! `element.style.<p> = v` проверяет значение по грамматике свойства и хранит каноническую
//! запись (CSSOM §serialize a CSS value); недопустимое значение отбрасывается. Покрыто:
//! `mask-{image,size,repeat,position,origin,clip,composite,mode}`, шорткод `mask`
//! (`expand_mask`/`mask_shorthand_value`), `clip-path`/`clip` (`clip_cssom`).
//! Слои `mask-{image,size,repeat,position*}` — та же грамматика, что у `background-*`
//! (`background_cssom`), поэтому проверка делегируется ей; `mask-composite`/`mask-mode`/
//! `mask-{origin,clip}` — свои списки ключевых слов. Вызывается из JS-шима
//! (`_lumen_css_canonical_mask`); каскад этим кодом не пользуется (его разборщики мягкие).

use super::background_cssom::{canonical_specified_background, list, one_of};
use super::length::split_top_level_ws;
use super::text_cssom::canonical_length_sorted;

/// `<geometry-box> = <shape-box> | fill-box | stroke-box | view-box`.
const COORD_BOXES: &[&str] = &[
    "border-box",
    "padding-box",
    "content-box",
    "fill-box",
    "stroke-box",
    "view-box",
];

/// Каноническая запись `value` свойства `prop` либо `None`, если значение не по грамматике
/// (присваивание игнорируется). `None` и для свойства, которого здесь нет.
pub fn canonical_specified_mask(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        "mask-image" => canonical_specified_background("background-image", v),
        "mask-size" => canonical_specified_background("background-size", v),
        "mask-repeat" => canonical_specified_background("background-repeat", v),
        "mask-position" => list(v, mask_position),
        "mask-origin" => list(v, coord_box),
        // CSS Masking L1 §4.5: `<coord-box> | no-clip`.
        "mask-clip" => list(v, |s| one_of(s, COORD_BOXES).or_else(|| one_of(s, &["no-clip"]))),
        // CSS Masking L1 §4.7: `add | subtract | intersect | exclude`.
        "mask-composite" => list(v, |s| one_of(s, &["add", "subtract", "intersect", "exclude"])),
        // CSS Masking L1 §4.3: `alpha | luminance | match-source`.
        "mask-mode" => list(v, |s| one_of(s, &["alpha", "luminance", "match-source"])),
        // CSS Masking L1 §3/§4.5 и CSS 2.1 §11.1 — своя грамматика (`clip_cssom`).
        "clip-path" | "clip" => super::clip_cssom::canonical_specified_clip(prop, v),
        _ => None,
    }
}

fn coord_box(s: &str) -> Option<String> {
    one_of(s, COORD_BOXES)
}

/// `<position>` одного слоя, но трёхзначная запись недопустима: `mask-position-invalid` считает
/// невалидными `right top 5px`, `center left 1px`, `left 4px top`, `bottom right 8%` —
/// все они дают ровно три токена (у `background-position` та же форма принимается).
fn mask_position(v: &str) -> Option<String> {
    if split_top_level_ws(v).len() == 3 {
        return None;
    }
    canonical_specified_background("background-position", v)
}

/// Лонгхенды шорткода `mask` в порядке раскладки (CSS Masking L1 §4.1).
const MASK_LAYER_LONGHANDS: [&str; 8] = [
    "mask-image",
    "mask-position",
    "mask-size",
    "mask-repeat",
    "mask-origin",
    "mask-clip",
    "mask-composite",
    "mask-mode",
];

/// `mask-border-*` — не слои: шорткод сбрасывает их в начальные значения один раз.
const MASK_BORDER_DEFAULTS: [(&str, &str); 5] = [
    ("mask-border-source", "none"),
    ("mask-border-outset", "0"),
    ("mask-border-repeat", "stretch"),
    ("mask-border-slice", "0"),
    ("mask-border-width", "auto"),
];

/// Один `<mask-layer>` после разбора; `None` — компонента ещё не было.
#[derive(Default)]
struct Layer {
    image: Option<String>,
    position: Option<String>,
    size: Option<String>,
    repeat: Option<String>,
    origin: Option<String>,
    clip: Option<String>,
    composite: Option<String>,
    mode: Option<String>,
}

fn is_css_wide(v: &str) -> bool {
    matches!(v, "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "revert-rule")
}

/// CSS Masking L1 §4.1: `mask` = `<mask-layer>#`. Пары `(лонгхенд, каноническое значение)`
/// в порядке спецификации; `None` — значение не по грамматике (объявление отбрасывается).
pub fn expand_mask(value: &str) -> Option<Vec<(&'static str, String)>> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    let lower = v.to_ascii_lowercase();
    // CSS Cascade L4 §7.1: CSS-wide keyword раздаётся каждому лонгхенду как есть.
    if is_css_wide(&lower) {
        let mut out: Vec<(&'static str, String)> = MASK_LAYER_LONGHANDS
            .iter()
            .map(|p| (*p, lower.clone()))
            .collect();
        out.extend(MASK_BORDER_DEFAULTS.iter().map(|(p, _)| (*p, lower.clone())));
        return Some(out);
    }
    let layers: Vec<Layer> = crate::style::split_top_level_commas(v)
        .into_iter()
        .map(|s| parse_layer(s.trim()))
        .collect::<Option<_>>()?;
    let join = |f: fn(&Layer) -> String| layers.iter().map(f).collect::<Vec<_>>().join(", ");
    let mut out: Vec<(&'static str, String)> = vec![
        ("mask-image", join(|l| l.image.clone().unwrap_or_else(|| "none".into()))),
        ("mask-position", join(|l| l.position.clone().unwrap_or_else(|| "0% 0%".into()))),
        ("mask-size", join(|l| l.size.clone().unwrap_or_else(|| "auto".into()))),
        ("mask-repeat", join(|l| l.repeat.clone().unwrap_or_else(|| "repeat".into()))),
        ("mask-origin", join(|l| l.origin.clone().unwrap_or_else(|| "border-box".into()))),
        ("mask-clip", join(|l| l.clip.clone().unwrap_or_else(|| "border-box".into()))),
        ("mask-composite", join(|l| l.composite.clone().unwrap_or_else(|| "add".into()))),
        ("mask-mode", join(|l| l.mode.clone().unwrap_or_else(|| "match-source".into()))),
    ];
    out.extend(MASK_BORDER_DEFAULTS.iter().map(|(p, d)| (*p, (*d).to_string())));
    Some(out)
}

/// Каноническая запись шорткода из раскрытых лонгхендов (CSSOM §serialize a shorthand):
/// компоненты в порядке грамматики, умолчания опускаются, слой без компонентов — `none`.
/// `None` — лонгхенды не заданы или числа слоёв не совпадают (шорткод не сериализуется).
pub fn mask_shorthand_value(get: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut lists: Vec<Vec<String>> = Vec::new();
    for (i, p) in MASK_LAYER_LONGHANDS.iter().enumerate() {
        let raw = get(p)?;
        let items: Vec<String> = crate::style::split_top_level_commas(&raw)
            .into_iter()
            .map(|s| s.trim().to_string())
            .collect();
        if i > 0 && items.len() != lists[0].len() {
            return None;
        }
        lists.push(items);
    }
    let n = lists[0].len();
    if n == 1 && is_css_wide(&lists[0][0]) && lists.iter().all(|l| l[0] == lists[0][0]) {
        return Some(lists[0][0].clone());
    }
    let v = |k: usize, i: usize| lists[k][i].as_str();
    let out: Vec<String> = (0..n)
        .map(|i| {
            let (image, position, size) = (v(0, i), v(1, i), v(2, i));
            let (repeat, origin, clip) = (v(3, i), v(4, i), v(5, i));
            let (composite, mode) = (v(6, i), v(7, i));
            let mut parts: Vec<String> = Vec::new();
            if image != "none" {
                parts.push(image.to_string());
            }
            if position != "0% 0%" || size != "auto" {
                if size == "auto" {
                    parts.push(position.to_string());
                } else {
                    parts.push(format!("{position} / {size}"));
                }
            }
            if repeat != "repeat" {
                parts.push(repeat.to_string());
            }
            if origin == clip {
                if origin != "border-box" {
                    parts.push(origin.to_string());
                }
            } else if clip == "no-clip" {
                // Один токен `no-clip` задаёт только `mask-clip`: умолчание origin опускается.
                if origin != "border-box" {
                    parts.push(origin.to_string());
                }
                parts.push("no-clip".to_string());
            } else {
                parts.push(origin.to_string());
                parts.push(clip.to_string());
            }
            if composite != "add" {
                parts.push(composite.to_string());
            }
            if mode != "match-source" {
                parts.push(mode.to_string());
            }
            if parts.is_empty() { "none".to_string() } else { parts.join(" ") }
        })
        .collect();
    Some(out.join(", "))
}

/// Токены слоя: пробелы верхнего уровня плюс `/` отдельным токеном (`1px 2px/contain`).
fn layer_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in split_top_level_ws(s) {
        let mut depth = 0i32;
        let mut quote = 0u8;
        let mut start = 0;
        let bytes = tok.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'"' | b'\'' if quote == 0 => quote = b,
                b if b == quote => quote = 0,
                b'(' if quote == 0 => depth += 1,
                b')' if quote == 0 => depth -= 1,
                b'/' if quote == 0 && depth == 0 => {
                    if i > start {
                        out.push(tok[start..i].to_string());
                    }
                    out.push("/".to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
        if start < tok.len() {
            out.push(tok[start..].to_string());
        }
    }
    out
}

fn is_position_keyword(t: &str) -> bool {
    one_of(t, &["left", "right", "top", "bottom", "center"]).is_some()
}

fn is_position_token(t: &str) -> bool {
    is_position_keyword(t) || canonical_length_sorted(t, false, false).is_some()
}

fn is_size_token(t: &str) -> bool {
    one_of(t, &["contain", "cover"]).is_some() || canonical_length_sorted(t, true, true).is_some()
}

fn is_repeat_token(t: &str) -> bool {
    one_of(t, &["repeat", "repeat-x", "repeat-y", "space", "round", "no-repeat"]).is_some()
}

/// Разбирает один слой; `None` — значение не по грамматике `<mask-layer>`.
fn parse_layer(s: &str) -> Option<Layer> {
    if s.is_empty() {
        return None;
    }
    let toks = layer_tokens(s);
    let mut l = Layer::default();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();
        if t == "/" {
            if l.position.is_none() || l.size.is_some() {
                return None;
            }
            i += 1;
            let start = i;
            while i < toks.len() && i - start < 2 && is_size_token(&toks[i]) {
                i += 1;
            }
            if i == start {
                return None;
            }
            l.size = Some(canonical_specified_background("background-size", &toks[start..i].join(" "))?);
            continue;
        }
        if t.eq_ignore_ascii_case("none") || t.contains('(') {
            if l.image.is_some() {
                return None;
            }
            l.image = Some(canonical_specified_background("background-image", t)?);
        } else if one_of(t, &["add", "subtract", "intersect", "exclude"]).is_some() {
            if l.composite.replace(t.to_ascii_lowercase()).is_some() {
                return None;
            }
        } else if one_of(t, &["alpha", "luminance", "match-source"]).is_some() {
            if l.mode.replace(t.to_ascii_lowercase()).is_some() {
                return None;
            }
        } else if is_repeat_token(t) {
            let start = i;
            while i < toks.len() && i - start < 2 && is_repeat_token(&toks[i]) {
                i += 1;
            }
            if l.repeat.is_some() {
                return None;
            }
            l.repeat = Some(canonical_specified_background("background-repeat", &toks[start..i].join(" "))?);
            continue;
        } else if one_of(t, COORD_BOXES).is_some() || one_of(t, &["no-clip"]).is_some() {
            if t.eq_ignore_ascii_case("no-clip") {
                if l.clip.replace("no-clip".into()).is_some() {
                    return None;
                }
            } else if l.origin.is_none() {
                l.origin = Some(t.to_ascii_lowercase());
            } else if l.clip.is_none() {
                l.clip = Some(t.to_ascii_lowercase());
            } else {
                return None;
            }
        } else if is_position_token(t) {
            let start = i;
            while i < toks.len() && i - start < 4 && is_position_token(&toks[i]) {
                i += 1;
            }
            if l.position.is_some() {
                return None;
            }
            l.position = Some(mask_position(&toks[start..i].join(" "))?);
            continue;
        } else {
            return None;
        }
        i += 1;
    }
    // CSS Masking L1 §4.5: один `<geometry-box>` задаёт и origin, и clip.
    if l.clip.is_none() {
        l.clip = l.origin.clone();
    }
    Some(l)
}
