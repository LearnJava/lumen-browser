//! CSS Gap Decorations L1 §3–§4 в inline-`style` CSSOM: разбор значения свойства `rule*` /
//! `{column,row}-rule*` в лонгхенды и обратная сборка шортхенда.
//!
//! Вход и выход — текст CSS: так функции зовёт JS-слой `CSSStyleDeclaration`
//! (`web_api_shim_mid.js`), у которого нет типизированных значений. Внутри объекта
//! `style` хранятся только лонгхенды в каноническом виде (specified value: `0` → `0px`,
//! `thin` остаётся `thin`, `repeat()` сохраняется), шортхенд собирается при чтении.
//!
//! Что покрыто: `*-rule-{width,style,color}` (списки с `repeat()`), шортхенды
//! `{column,row,}-rule` (список `<gap-rule>`), `*-rule-{break,visibility-items}`,
//! `rule-overlap`, все `*-rule-inset*`. CSS-wide ключевое слово раскладывается во все
//! лонгхенды шортхенда.

use crate::style::parse::color::{
    canonical_specified_color, parse_color, parse_css_color_keep_srgb_form,
};
use crate::style::values::length::{canonical_specified_line_width, split_top_level_ws};
use crate::style::values::misc::{RuleInsetProp, RuleOverlap};
use crate::style::values::rule_list::{RuleItem, RuleList};

/// Результат разбора одной декларации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapDecl {
    /// Свойство не из семейства — вызывающий идёт прежним путём.
    NotGap,
    /// Значение не проходит грамматику — декларация отбрасывается.
    Invalid,
    /// Пары `(лонгхенд, каноническое значение)`.
    Longhands(Vec<(String, String)>),
}

const CSS_WIDE: [&str; 6] = [
    "inherit",
    "initial",
    "unset",
    "revert",
    "revert-layer",
    "revert-rule",
];
const LINE_STYLES: [&str; 10] = [
    "none", "hidden", "dotted", "dashed", "solid", "double", "groove", "ridge", "inset", "outset",
];
const INSET_SLOTS: [&str; 4] = ["cap-start", "cap-end", "junction-start", "junction-end"];

/// Одиночное свойство-часть (не шортхенд из трёх списков и не `*-inset*`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Width,
    Style,
    Color,
    Break,
    Visibility,
    Overlap,
}

impl Part {
    fn suffix(self) -> &'static str {
        match self {
            Self::Width => "width",
            Self::Style => "style",
            Self::Color => "color",
            Self::Break => "break",
            Self::Visibility => "visibility-items",
            Self::Overlap => "overlap",
        }
    }
}

enum Kind {
    /// `{column,row,}-rule` — список `<gap-rule>`.
    Triplet,
    Part(Part),
    Inset(RuleInsetProp),
}

struct Classified {
    axes: &'static [&'static str],
    kind: Kind,
    /// Имя без префикса оси (`""`, `-width`, `-inset-cap`, …) — нужно сборке шортхенда.
    rest: String,
}

fn classify(prop: &str) -> Option<Classified> {
    if let Some(ip) = RuleInsetProp::of(prop) {
        let axes: &'static [&'static str] = match (ip.cols, ip.rows) {
            (true, true) => &["column", "row"],
            (true, false) => &["column"],
            _ => &["row"],
        };
        let rest = prop
            .strip_prefix("column-rule-inset")
            .or_else(|| prop.strip_prefix("row-rule-inset"))
            .or_else(|| prop.strip_prefix("rule-inset"))
            .unwrap_or("")
            .to_string();
        return Some(Classified {
            axes,
            kind: Kind::Inset(ip),
            rest,
        });
    }
    let (axes, rest): (&'static [&'static str], &str) =
        if let Some(r) = prop.strip_prefix("column-rule") {
            (&["column"], r)
        } else if let Some(r) = prop.strip_prefix("row-rule") {
            (&["row"], r)
        } else {
            (&["column", "row"], prop.strip_prefix("rule")?)
        };
    let kind = match rest {
        "" => Kind::Triplet,
        "-width" => Kind::Part(Part::Width),
        "-style" => Kind::Part(Part::Style),
        "-color" => Kind::Part(Part::Color),
        "-break" => Kind::Part(Part::Break),
        "-visibility-items" => Kind::Part(Part::Visibility),
        // `rule-overlap` — единственное свойство без оси.
        "-overlap" if prop == "rule-overlap" => Kind::Part(Part::Overlap),
        _ => return None,
    };
    Some(Classified {
        axes,
        kind,
        rest: rest.to_string(),
    })
}

/// Имена лонгхендов, которые задаёт свойство.
fn longhand_names(c: &Classified) -> Vec<String> {
    let mut out = Vec::new();
    match &c.kind {
        Kind::Triplet => {
            for axis in c.axes {
                for part in [Part::Width, Part::Style, Part::Color] {
                    out.push(format!("{axis}-rule-{}", part.suffix()));
                }
            }
        }
        Kind::Part(Part::Overlap) => out.push("rule-overlap".to_string()),
        Kind::Part(p) => {
            for axis in c.axes {
                out.push(format!("{axis}-rule-{}", p.suffix()));
            }
        }
        Kind::Inset(ip) => {
            for axis in c.axes {
                for &slot in ip.slots() {
                    out.push(format!("{axis}-rule-inset-{}", INSET_SLOTS[slot]));
                }
            }
        }
    }
    out
}

// ─── значения-элементы ──────────────────────────────────────────────────────

fn is_css_wide(s: &str) -> bool {
    CSS_WIDE.iter().any(|k| s.trim().eq_ignore_ascii_case(k))
}

/// `<line-width>`: длина `[0,∞]` или `thin|medium|thick`; процент не допускается.
fn width_item(s: &str) -> Option<String> {
    let c = canonical_specified_line_width(s)?;
    (!c.ends_with('%')).then_some(c)
}

fn style_item(s: &str) -> Option<String> {
    let t = s.trim().to_ascii_lowercase();
    LINE_STYLES.contains(&t.as_str()).then_some(t)
}

/// `<color>`: ключевые слова остаются ключевыми словами, `rgb()`/hex — в `rgb()`;
/// форма, которую каноникализатор не знает (`color-mix()`, …), но движок разбирает —
/// остаётся как записана.
fn color_item(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || is_css_wide(t) {
        return None;
    }
    // `color-mix(in srgb, …)` и `rgb(from …)` хранят функциональную форму `color(srgb …)`
    // (CSS Color L4 §4.2) — каноникализатор свёл бы их в `rgb()`, поэтому они остаются
    // как записаны, а `getComputedStyle()` сериализует их сам.
    if matches!(parse_css_color_keep_srgb_form(t, false), Some(crate::style::CssColor::Wide(_))) {
        return Some(t.to_string());
    }
    canonical_specified_color(t).or_else(|| parse_color(t).map(|_| t.to_string()))
}

fn keyword_item(s: &str, allowed: &[&str]) -> Option<String> {
    let t = s.trim().to_ascii_lowercase();
    allowed.contains(&t.as_str()).then_some(t)
}

/// Один `<gap-rule>` = `<line-width> || <line-style> || <color>`; пропущенное — initial.
#[derive(Clone, PartialEq, Eq)]
struct Triplet {
    width: String,
    style: String,
    color: String,
}

fn triplet_item(s: &str) -> Option<Triplet> {
    let (mut w, mut st, mut c) = (None, None, None);
    let tokens = split_top_level_ws(s.trim());
    if tokens.is_empty() {
        return None;
    }
    for tok in tokens {
        if let Some(v) = style_item(tok) {
            if st.replace(v).is_some() {
                return None;
            }
        } else if let Some(v) = width_item(tok) {
            if w.replace(v).is_some() {
                return None;
            }
        } else {
            let v = color_item(tok)?;
            if c.replace(v).is_some() {
                return None;
            }
        }
    }
    Some(Triplet {
        width: w.unwrap_or_else(|| "medium".into()),
        style: st.unwrap_or_else(|| "none".into()),
        color: c.unwrap_or_else(|| "currentcolor".into()),
    })
}

// ─── разбор ─────────────────────────────────────────────────────────────────

/// Раскладывает декларацию `prop: value` в канонические лонгхенды.
pub fn expand_gap_rule_declaration(prop: &str, value: &str) -> GapDecl {
    let Some(c) = classify(prop) else {
        return GapDecl::NotGap;
    };
    let v = value.trim();
    let names = longhand_names(&c);
    if is_css_wide(v) {
        let kw = v.to_ascii_lowercase();
        return GapDecl::Longhands(names.into_iter().map(|n| (n, kw.clone())).collect());
    }
    let mut out: Vec<(String, String)> = Vec::new();
    match &c.kind {
        Kind::Triplet => {
            let Some(list) = RuleList::parse(v, &triplet_item) else {
                return GapDecl::Invalid;
            };
            for axis in c.axes {
                out.push((
                    format!("{axis}-rule-width"),
                    list.map(|t| t.width.clone()).to_css(String::clone),
                ));
                out.push((
                    format!("{axis}-rule-style"),
                    list.map(|t| t.style.clone()).to_css(String::clone),
                ));
                out.push((
                    format!("{axis}-rule-color"),
                    list.map(|t| t.color.clone()).to_css(String::clone),
                ));
            }
        }
        Kind::Part(p @ (Part::Width | Part::Style | Part::Color)) => {
            let list = match p {
                Part::Width => RuleList::parse(v, &width_item),
                Part::Style => RuleList::parse(v, &style_item),
                _ => RuleList::parse(v, &color_item),
            };
            let Some(list) = list else {
                return GapDecl::Invalid;
            };
            let text = list.to_css(String::clone);
            for n in names {
                out.push((n, text.clone()));
            }
        }
        Kind::Part(p) => {
            let kw = match p {
                Part::Break => keyword_item(v, &["none", "normal", "intersection"]),
                Part::Visibility => keyword_item(v, &["all", "around", "between", "normal"]),
                _ => RuleOverlap::parse(v).map(|o| o.to_css().to_string()),
            };
            let Some(kw) = kw else {
                return GapDecl::Invalid;
            };
            for n in names {
                out.push((n, kw.clone()));
            }
        }
        Kind::Inset(ip) => {
            let Some(parsed) = ip.parse(v, false) else {
                return GapDecl::Invalid;
            };
            for axis in c.axes {
                for (slot, val) in &parsed {
                    out.push((
                        format!("{axis}-rule-inset-{}", INSET_SLOTS[*slot]),
                        val.to_css(),
                    ));
                }
            }
        }
    }
    GapDecl::Longhands(out)
}

/// Имена лонгхендов шортхенда (или свойства-близнеца на обе оси); пусто — не из семейства.
pub fn gap_rule_longhand_names(prop: &str) -> Vec<String> {
    classify(prop)
        .map(|c| longhand_names(&c))
        .unwrap_or_default()
}

// ─── сборка шортхенда ───────────────────────────────────────────────────────

/// Форма списка: тип элемента, счётчик повторителя и число значений внутри.
fn shape(l: &RuleList<String>) -> Vec<(u8, u32, usize)> {
    l.items()
        .iter()
        .map(|it| match it {
            RuleItem::One(_) => (0, 0, 1),
            RuleItem::Repeat(n, vs) => (1, *n, vs.len()),
            RuleItem::Auto(vs) => (2, 0, vs.len()),
        })
        .collect()
}

fn triplet_text(w: &str, s: &str, c: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if w != "medium" {
        parts.push(w);
    }
    if s != "none" {
        parts.push(s);
    }
    if c != "currentcolor" {
        parts.push(c);
    }
    if parts.is_empty() {
        parts.push("medium");
    }
    parts.join(" ")
}

/// `width`/`style`/`color` одной оси → `<gap-rule-list>`; `None`, если списки разной формы.
fn compose_triplets(w: &str, s: &str, c: &str) -> Option<String> {
    let ident = |t: &str| Some(t.trim().to_string());
    let (lw, ls, lc) = (
        RuleList::parse(w, &ident)?,
        RuleList::parse(s, &ident)?,
        RuleList::parse(c, &ident)?,
    );
    let sh = shape(&lw);
    if sh != shape(&ls) || sh != shape(&lc) {
        return None;
    }
    let mut parts = Vec::new();
    for ((iw, is), ic) in lw.items().iter().zip(ls.items()).zip(lc.items()) {
        let part = match (iw, is, ic) {
            (RuleItem::One(a), RuleItem::One(b), RuleItem::One(d)) => triplet_text(a, b, d),
            (RuleItem::Repeat(n, a), RuleItem::Repeat(_, b), RuleItem::Repeat(_, d)) => {
                format!("repeat({n}, {})", join_triplets(a, b, d))
            }
            (RuleItem::Auto(a), RuleItem::Auto(b), RuleItem::Auto(d)) => {
                format!("repeat(auto, {})", join_triplets(a, b, d))
            }
            _ => return None,
        };
        parts.push(part);
    }
    Some(parts.join(", "))
}

fn join_triplets(w: &[String], s: &[String], c: &[String]) -> String {
    w.iter()
        .zip(s)
        .zip(c)
        .map(|((a, b), d)| triplet_text(a, b, d))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Собирает значение шортхенда `prop` из лонгхендов (`get` отдаёт сохранённое значение
/// или `None`). `None` — собрать нельзя: лонгхенд не задан, значения не сходятся между
/// осями или списки разной формы.
pub fn gap_rule_shorthand_value(
    prop: &str,
    get: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let c = classify(prop)?;
    let names = longhand_names(&c);
    let vals: Vec<String> = names.iter().map(|n| get(n)).collect::<Option<_>>()?;
    // Все лонгхенды получили одно CSS-wide ключевое слово — оно и есть значение шортхенда.
    if let Some(first) = vals.first()
        && is_css_wide(first)
        && vals.iter().all(|v| v == first)
    {
        return Some(first.clone());
    }
    let mut per_axis: Vec<String> = Vec::new();
    match &c.kind {
        Kind::Triplet => {
            for axis in c.axes {
                let at = |p: Part| get(&format!("{axis}-rule-{}", p.suffix()));
                per_axis.push(compose_triplets(
                    &at(Part::Width)?,
                    &at(Part::Style)?,
                    &at(Part::Color)?,
                )?);
            }
        }
        Kind::Part(_) => per_axis = vals,
        Kind::Inset(ip) => {
            let slots = ip.slots();
            for axis in c.axes {
                let v: Vec<String> = slots
                    .iter()
                    .map(|&s| get(&format!("{axis}-rule-inset-{}", INSET_SLOTS[s])))
                    .collect::<Option<_>>()?;
                let all_eq = v.iter().all(|x| *x == v[0]);
                let text = match (c.rest.as_str(), all_eq) {
                    (_, true) => v[0].clone(),
                    ("", false) => format!("{} {} / {} {}", v[0], v[1], v[2], v[3]),
                    ("-cap" | "-junction", false) => format!("{} {}", v[0], v[1]),
                    _ => return None,
                };
                per_axis.push(text);
            }
        }
    }
    let first = per_axis.first()?.clone();
    per_axis.iter().all(|x| *x == first).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp(prop: &str, v: &str) -> Vec<(String, String)> {
        match expand_gap_rule_declaration(prop, v) {
            GapDecl::Longhands(l) => l,
            other => panic!("{prop}: {v} -> {other:?}"),
        }
    }

    fn get_in<'a>(l: &'a [(String, String)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |n| l.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone())
    }

    fn value_of(l: &[(String, String)], name: &str) -> String {
        get_in(l)(name).unwrap_or_else(|| panic!("нет {name}"))
    }

    #[test]
    fn unknown_property_is_not_gap() {
        assert_eq!(
            expand_gap_rule_declaration("margin", "1px"),
            GapDecl::NotGap
        );
        assert_eq!(
            expand_gap_rule_declaration("rule-foo", "1px"),
            GapDecl::NotGap
        );
    }

    #[test]
    fn rule_shorthand_expands_to_three_lists() {
        let l = exp("column-rule", "repeat(auto, blue 6px, 5px solid red)");
        assert_eq!(value_of(&l, "column-rule-width"), "repeat(auto, 6px, 5px)");
        assert_eq!(
            value_of(&l, "column-rule-style"),
            "repeat(auto, none, solid)"
        );
        assert_eq!(value_of(&l, "column-rule-color"), "repeat(auto, blue, red)");
        assert_eq!(l.len(), 3);
        assert_eq!(exp("rule", "double").len(), 6);
    }

    #[test]
    fn rule_shorthand_defaults_and_roundtrip() {
        let l = exp("column-rule", "double");
        assert_eq!(value_of(&l, "column-rule-width"), "medium");
        assert_eq!(value_of(&l, "column-rule-color"), "currentcolor");
        assert_eq!(
            gap_rule_shorthand_value("column-rule", &get_in(&l)).as_deref(),
            Some("double")
        );
        let l = exp("column-rule", "currentcolor none medium");
        assert_eq!(
            gap_rule_shorthand_value("column-rule", &get_in(&l)).as_deref(),
            Some("medium")
        );
        let l = exp("column-rule", "blue 6px");
        assert_eq!(
            gap_rule_shorthand_value("column-rule", &get_in(&l)).as_deref(),
            Some("6px blue")
        );
    }

    #[test]
    fn rule_shorthand_needs_aligned_lists() {
        let l = vec![
            (
                "column-rule-width".to_string(),
                "15px, 25px, 35px".to_string(),
            ),
            ("column-rule-style".to_string(), "solid, dotted".to_string()),
            ("column-rule-color".to_string(), "green".to_string()),
        ];
        assert_eq!(gap_rule_shorthand_value("column-rule", &get_in(&l)), None);
    }

    #[test]
    fn invalid_values_are_rejected() {
        for (p, v) in [
            ("column-rule", "red 5px solid red"),
            ("column-rule", "auto"),
            ("column-rule", ""),
            ("column-rule-width", "thin medium thick"),
            ("column-rule-width", "30%"),
            ("column-rule", "30% solid"),
            ("column-rule-style", "dashed dotted solid"),
            ("column-rule-color", "red blue green"),
            ("rule-break", "auto"),
            ("column-rule-visibility-items", "10px"),
            ("rule-overlap", "none"),
            ("column-rule-inset", "10px / 20px / 10px"),
            ("column-rule-inset-cap", "10px blue"),
            ("column-rule-inset-start", "10px 20px"),
        ] {
            assert_eq!(
                expand_gap_rule_declaration(p, v),
                GapDecl::Invalid,
                "{p}: {v}"
            );
        }
    }

    #[test]
    fn css_wide_keyword_fans_out() {
        let l = exp("rule", "inherit");
        assert_eq!(l.len(), 6);
        assert!(l.iter().all(|(_, v)| v == "inherit"));
        assert_eq!(
            gap_rule_shorthand_value("rule", &get_in(&l)).as_deref(),
            Some("inherit")
        );
    }

    #[test]
    fn inset_shorthand_serialization() {
        for (v, want) in [
            ("0", "0px"),
            ("10px 20px", "10px 20px / 10px 20px"),
            ("10px / 20px", "10px 10px / 20px 20px"),
            ("10px 20px / -5px", "10px 20px / -5px -5px"),
            (
                "overlap-join overlap-join / overlap-join overlap-join",
                "overlap-join",
            ),
            (
                "overlap-join / 10px",
                "overlap-join overlap-join / 10px 10px",
            ),
        ] {
            let l = exp("column-rule-inset", v);
            assert_eq!(
                gap_rule_shorthand_value("column-rule-inset", &get_in(&l)).as_deref(),
                Some(want),
                "{v}"
            );
        }
    }

    #[test]
    fn inset_partial_shorthands() {
        let l = exp("rule-inset-start", "5px");
        assert_eq!(l.len(), 4);
        assert_eq!(value_of(&l, "row-rule-inset-junction-start"), "5px");
        assert_eq!(
            gap_rule_shorthand_value("rule-inset-start", &get_in(&l)).as_deref(),
            Some("5px")
        );
        let l = exp("column-rule-inset-cap", "5% 10%");
        assert_eq!(
            gap_rule_shorthand_value("column-rule-inset-cap", &get_in(&l)).as_deref(),
            Some("5% 10%")
        );
        // Оси разошлись — у шортхенда на обе оси значения нет.
        let mut l = exp("rule-inset", "1px");
        l[0].1 = "2px".into();
        assert_eq!(gap_rule_shorthand_value("rule-inset", &get_in(&l)), None);
    }

    #[test]
    fn keyword_properties_cover_both_axes() {
        let l = exp("rule-break", "INTERSECTION");
        assert_eq!(value_of(&l, "column-rule-break"), "intersection");
        assert_eq!(value_of(&l, "row-rule-break"), "intersection");
        assert_eq!(
            exp("rule-overlap", "row-over-column"),
            [("rule-overlap".to_string(), "row-over-column".to_string())]
        );
    }

    #[test]
    fn color_item_canonicalizes_functional_form() {
        let l = exp("column-rule-color", "repeat(2, #00ff00, red)");
        assert_eq!(
            value_of(&l, "column-rule-color"),
            "repeat(2, rgb(0, 255, 0), red)"
        );
    }
}
