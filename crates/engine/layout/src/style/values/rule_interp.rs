//! CSS Gap Decorations L1 §4.7 — интерполяция `*-rule-width` / `*-rule-color` /
//! `*-rule-inset-*` между двумя значениями, заданными строками.
//!
//! Вход и выход — текст CSS: так функцию зовёт JS-слой Web Animations
//! (`_wa_interp_prop`), у которого нет типизированных значений. `None` — пара не
//! интерполируется (разные формы списков, `overlap-join`, неизвестная единица) и
//! вызывающий обязан сделать дискретный перелом на 50% (CSS Values L4 §4.4).
//!
//! Списки (`<gap-rule-list>`, §4.5) интерполируются так:
//! * без `repeat(auto, …)` — целочисленные `repeat()` раскрываются, оба списка
//!   циклически доводятся до наименьшего общего кратного длин, значения
//!   интерполируются попарно; результат — плоский список;
//! * с `repeat(auto, …)` — оба списка обязаны иметь `auto`, равное число значений
//!   до него и после него (после раскрытия целочисленных повторителей); внутренние
//!   списки `auto` приводятся к наименьшему общему кратному; результат сохраняет
//!   форму `lead…, repeat(auto, …), trail…`;
//! * иначе — не интерполируется.

use lumen_core::geom::Size;

use crate::style::parse::box_sides::parse_line_width;
use crate::style::parse::color::parse_color;
use crate::style::values::misc::RuleInset;
use crate::style::values::rule_list::{RuleItem, RuleList};
use crate::style::{Color, Length};

/// Верхняя граница длины раскрытого списка: защита от `repeat(1000000, …)`.
const MAX_EXPANDED: usize = 4096;

/// Что за значение несёт свойство — выбирает грамматику и арифметику.
#[derive(Clone, Copy)]
enum Kind {
    Width,
    Color,
    Inset,
}

/// Классифицирует имя свойства (kebab-case); `None` — не наше свойство.
fn kind_of(prop: &str) -> Option<Kind> {
    let rest = prop
        .strip_prefix("column-rule")
        .or_else(|| prop.strip_prefix("row-rule"))?;
    match rest {
        "-width" => Some(Kind::Width),
        "-color" => Some(Kind::Color),
        "-inset-cap-start" | "-inset-cap-end" | "-inset-junction-start" | "-inset-junction-end" => {
            Some(Kind::Inset)
        }
        _ => None,
    }
}

/// `true`, если `prop` (kebab-case) интерполируется этой функцией.
pub fn is_interpolable_gap_rule_property(prop: &str) -> bool {
    kind_of(prop).is_some()
}

/// Интерполирует `from` → `to` при прогрессе `t` для свойства `prop` (kebab-case).
/// `None` — перелом на 50% остаётся вызывающему.
pub fn interpolate_gap_rule_value(prop: &str, from: &str, to: &str, t: f64) -> Option<String> {
    match kind_of(prop)? {
        Kind::Width => {
            let item =
                |s: &str| parse_line_width(s, 16.0, Size::ZERO, false).filter(|px| *px >= 0.0);
            interpolate_lists(from, to, &item, &|a, b| lerp_width(*a, *b, t), &fmt_width)
        }
        Kind::Color => interpolate_lists(
            from,
            to,
            &|s: &str| parse_color(s),
            &|a, b| lerp_color(*a, *b, t),
            &|c| crate::selector_query::color_to_css(*c),
        ),
        Kind::Inset => interpolate_inset(from, to, t),
    }
}

/// Computed-форма одного значения свойства (для дискретного перелома, где арифметики
/// нет, а значение всё равно должно быть сериализовано как computed): цвета — в `rgb()`,
/// ширины — в `px` с привязкой как у `border-width`, вставки — как заданы; форма списка
/// с `repeat()` сохраняется. `None` — значение не разобралось.
pub fn canonical_gap_rule_value(prop: &str, value: &str) -> Option<String> {
    match kind_of(prop)? {
        Kind::Width => {
            let item =
                |s: &str| parse_line_width(s, 16.0, Size::ZERO, false).filter(|px| *px >= 0.0);
            let list = RuleList::parse(value.trim(), &item)?;
            Some(list.to_css(|px| fmt_width(&snap_width(f64::from(*px)))))
        }
        Kind::Color => {
            let list = RuleList::parse(value.trim(), &|s: &str| parse_color(s))?;
            Some(list.to_css(|c| crate::selector_query::color_to_css(*c)))
        }
        Kind::Inset => RuleInset::parse(value, false).map(|i| i.to_css()),
    }
}

// ─── списки ─────────────────────────────────────────────────────────────────

/// Список, разобранный на «до `auto`», тело `auto` и «после `auto`».
struct Flat<T> {
    leading: Vec<T>,
    auto: Option<Vec<T>>,
    trailing: Vec<T>,
}

fn flatten<T: Clone>(list: &RuleList<T>) -> Option<Flat<T>> {
    let mut flat = Flat {
        leading: Vec::new(),
        auto: None,
        trailing: Vec::new(),
    };
    for it in list.items() {
        let dst = if flat.auto.is_some() {
            &mut flat.trailing
        } else {
            &mut flat.leading
        };
        match it {
            RuleItem::One(v) => dst.push(v.clone()),
            RuleItem::Repeat(n, vs) => {
                let total = (*n as usize).checked_mul(vs.len())?;
                if dst.len() + total > MAX_EXPANDED {
                    return None;
                }
                for _ in 0..*n {
                    dst.extend(vs.iter().cloned());
                }
            }
            RuleItem::Auto(vs) => flat.auto = Some(vs.clone()),
        }
    }
    Some(flat)
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Попарная интерполяция двух циклических рядов до наименьшего общего кратного.
fn zip_lcm<T>(a: &[T], b: &[T], lerp: &impl Fn(&T, &T) -> T) -> Option<Vec<T>> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let len = a.len() / gcd(a.len(), b.len()) * b.len();
    if len > MAX_EXPANDED {
        return None;
    }
    Some(
        (0..len)
            .map(|i| lerp(&a[i % a.len()], &b[i % b.len()]))
            .collect(),
    )
}

fn interpolate_lists<T: Clone>(
    from: &str,
    to: &str,
    item: &impl Fn(&str) -> Option<T>,
    lerp: &impl Fn(&T, &T) -> T,
    fmt: &impl Fn(&T) -> String,
) -> Option<String> {
    let a = flatten(&RuleList::parse(from.trim(), item)?)?;
    let b = flatten(&RuleList::parse(to.trim(), item)?)?;
    let join = |vs: &[T]| vs.iter().map(fmt).collect::<Vec<_>>().join(", ");
    match (&a.auto, &b.auto) {
        (None, None) => {
            // Без `auto` весь список лежит в `leading`.
            let out = zip_lcm(&a.leading, &b.leading, lerp)?;
            Some(join(&out))
        }
        (Some(aa), Some(ba)) => {
            if a.leading.len() != b.leading.len() || a.trailing.len() != b.trailing.len() {
                return None;
            }
            let lead = zip_lcm_same_len(&a.leading, &b.leading, lerp);
            let trail = zip_lcm_same_len(&a.trailing, &b.trailing, lerp);
            let auto = zip_lcm(aa, ba, lerp)?;
            let mut parts = Vec::new();
            if !lead.is_empty() {
                parts.push(join(&lead));
            }
            parts.push(format!("repeat(auto, {})", join(&auto)));
            if !trail.is_empty() {
                parts.push(join(&trail));
            }
            Some(parts.join(", "))
        }
        _ => None,
    }
}

/// Попарная интерполяция рядов одинаковой длины (в том числе пустых).
fn zip_lcm_same_len<T>(a: &[T], b: &[T], lerp: &impl Fn(&T, &T) -> T) -> Vec<T> {
    a.iter().zip(b).map(|(x, y)| lerp(x, y)).collect()
}

// ─── арифметика значений ────────────────────────────────────────────────────

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Число без хвостовых нулей: 4 знака после запятой хватает для `px`.
fn fmt_num(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    let r = if r == 0.0 { 0.0 } else { r };
    let s = format!("{r:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn lerp_width(a: f32, b: f32, t: f64) -> f32 {
    // Ширина линии не бывает отрицательной (§4.3: `<line-width>` ≥ 0) — за пределами
    // [0, 1] результат зажимается, как у `border-width`. Computed value — «list of
    // absolute lengths, snapped as a border width» (§4.3): ненулевая ширина меньше 1px
    // становится 1px, прочие округляются вниз до целого.
    snap_width(lerp(f64::from(a), f64::from(b), t))
}

/// Привязка ширины как у `border-width`: 0 остаётся 0, ненулевая меньше 1px — 1px,
/// прочие округляются вниз; отрицательная зажимается в 0.
fn snap_width(v: f64) -> f32 {
    let v = v.max(0.0);
    if v == 0.0 {
        0.0
    } else {
        v.floor().max(1.0) as f32
    }
}

fn fmt_width(px: &f32) -> String {
    format!("{}px", fmt_num(f64::from(*px)))
}

/// CSS Color L4 §12.3: интерполяция в sRGB с предумноженной альфой.
fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    let (aa, ba) = (f64::from(a.a) / 255.0, f64::from(b.a) / 255.0);
    let alpha = lerp(aa, ba, t).clamp(0.0, 1.0);
    let chan = |x: u8, y: u8| -> u8 {
        let (px, py) = (f64::from(x) * aa, f64::from(y) * ba);
        let premult = lerp(px, py, t);
        let v = if alpha > 0.0 { premult / alpha } else { 0.0 };
        v.round().clamp(0.0, 255.0) as u8
    };
    Color {
        r: chan(a.r, b.r),
        g: chan(a.g, b.g),
        b: chan(a.b, b.b),
        a: (alpha * 255.0).round() as u8,
    }
}

/// `<length-percentage>` как пара `px + %`; `has_pct` — был ли процент в любой из
/// сторон (тогда результат с ненулевой px-частью пишется как `calc()`).
#[derive(Clone, Copy)]
struct PxPct {
    px: f64,
    pct: f64,
    has_pct: bool,
}

fn px_pct(l: &Length) -> Option<PxPct> {
    match l {
        Length::Px(v) => Some(PxPct {
            px: f64::from(*v),
            pct: 0.0,
            has_pct: false,
        }),
        Length::Percent(v) => Some(PxPct {
            px: 0.0,
            pct: f64::from(*v),
            has_pct: true,
        }),
        _ => None,
    }
}

fn interpolate_inset(from: &str, to: &str, t: f64) -> Option<String> {
    let parse = |s: &str| match RuleInset::parse(s, false)? {
        RuleInset::Length(l) => px_pct(&l),
        RuleInset::OverlapJoin => None,
    };
    let (a, b) = (parse(from)?, parse(to)?);
    let px = lerp(a.px, b.px, t);
    let pct = match (a.has_pct, b.has_pct) {
        (true, true) => lerp(a.pct, b.pct, t),
        (false, true) => b.pct * t,
        (true, false) => a.pct * (1.0 - t),
        (false, false) => 0.0,
    };
    if !(a.has_pct || b.has_pct) {
        return Some(format!("{}px", fmt_num(px)));
    }
    if fmt_num(px) == "0" {
        return Some(format!("{}%", fmt_num(pct)));
    }
    let sign = if px < 0.0 { '-' } else { '+' };
    Some(format!(
        "calc({}% {sign} {}px)",
        fmt_num(pct),
        fmt_num(px.abs())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(from: &str, to: &str, t: f64) -> Option<String> {
        interpolate_gap_rule_value("column-rule-width", from, to, t)
    }

    #[test]
    fn width_single_value() {
        assert_eq!(w("10px", "40px", 0.3).as_deref(), Some("19px"));
        assert_eq!(w("10px", "40px", -0.3).as_deref(), Some("1px"));
        assert_eq!(w("10px", "40px", 1.5).as_deref(), Some("55px"));
    }

    #[test]
    fn width_is_clamped_to_zero() {
        assert_eq!(w("3px", "40px", -0.3).as_deref(), Some("0px"));
    }

    #[test]
    fn width_keywords_resolve_before_lerp() {
        assert_eq!(w("thin", "11px", 0.3).as_deref(), Some("4px"));
        assert_eq!(w("thick", "15px", 0.6).as_deref(), Some("11px"));
    }

    #[test]
    fn width_lists_go_to_lcm() {
        assert_eq!(
            w("10px, 15px, 20px", "40px, 40px, 40px", 0.3).as_deref(),
            Some("19px, 22px, 26px")
        );
        assert_eq!(
            w("repeat(2, 20px)", "30px", 0.5).as_deref(),
            Some("25px, 25px")
        );
        assert_eq!(
            w("10px, 20px", "30px, 40px, 50px", 1.0).as_deref(),
            Some("30px, 40px, 50px, 30px, 40px, 50px")
        );
    }

    #[test]
    fn width_integer_repeat_expands() {
        assert_eq!(
            w(
                "repeat(2, 20px, 20px), repeat(1, 30px)",
                "repeat(2, 40px, 40px), repeat(1, 40px)",
                0.5
            )
            .as_deref(),
            Some("30px, 30px, 30px, 30px, 35px")
        );
    }

    #[test]
    fn width_auto_repeat_keeps_form() {
        assert_eq!(
            w("repeat(auto, 20px)", "repeat(auto, 30px, 30px)", 0.5).as_deref(),
            Some("repeat(auto, 25px, 25px)")
        );
        assert_eq!(
            w(
                "repeat(2, 20px), repeat(auto, 20px), 20px",
                "30px, 30px, repeat(auto, 30px), 30px",
                0.5
            )
            .as_deref(),
            Some("25px, 25px, repeat(auto, 25px), 25px")
        );
    }

    #[test]
    fn width_mismatched_auto_shapes_do_not_interpolate() {
        assert_eq!(
            w(
                "20px, repeat(auto, 20px)",
                "30px, 30px, repeat(auto, 30px)",
                0.5
            ),
            None
        );
        assert_eq!(
            w(
                "20px, repeat(auto, 20px)",
                "30px, repeat(auto, 30px), 30px",
                0.5
            ),
            None
        );
        assert_eq!(w("repeat(auto, 20px)", "30px", 0.5), None);
    }

    #[test]
    fn color_premultiplied() {
        let c = |f: &str, t: &str, x: f64| interpolate_gap_rule_value("row-rule-color", f, t, x);
        assert_eq!(c("black", "red", 0.5).as_deref(), Some("rgb(128, 0, 0)"));
        assert_eq!(
            c("orange", "blue", 0.2).as_deref(),
            Some("rgb(204, 132, 51)")
        );
        assert_eq!(
            c("red, blue", "blue", 1.0).as_deref(),
            Some("rgb(0, 0, 255), rgb(0, 0, 255)")
        );
        assert_eq!(c("currentcolor", "red", 0.5), None);
    }

    #[test]
    fn inset_length_and_percent() {
        let i = |f: &str, t: &str, x: f64| {
            interpolate_gap_rule_value("row-rule-inset-cap-start", f, t, x)
        };
        assert_eq!(i("5px", "15px", 0.3).as_deref(), Some("8px"));
        assert_eq!(i("0px", "10px", -0.3).as_deref(), Some("-3px"));
        assert_eq!(i("0%", "-40%", 0.3).as_deref(), Some("-12%"));
        assert_eq!(i("-100%", "1px", 0.0).as_deref(), Some("-100%"));
        assert_eq!(
            i("-100%", "1px", 0.3).as_deref(),
            Some("calc(-70% + 0.3px)")
        );
        assert_eq!(
            i("-100%", "1px", -0.3).as_deref(),
            Some("calc(-130% - 0.3px)")
        );
        assert_eq!(i("-100%", "1px", 1.0).as_deref(), Some("calc(0% + 1px)"));
        assert_eq!(i("overlap-join", "1px", 0.3), None);
    }

    #[test]
    fn canonical_keeps_list_form_and_serializes_computed() {
        let c = |p: &str, v: &str| canonical_gap_rule_value(p, v);
        assert_eq!(
            c("column-rule-color", "red, repeat(auto, red)").as_deref(),
            Some("rgb(255, 0, 0), repeat(auto, rgb(255, 0, 0))")
        );
        assert_eq!(
            c("column-rule-width", "thin, repeat(2, 20px)").as_deref(),
            Some("1px, repeat(2, 20px)")
        );
        assert_eq!(c("row-rule-inset-cap-start", "overlap-join").as_deref(), Some("overlap-join"));
        assert_eq!(c("column-rule-color", "nonsense"), None);
        assert_eq!(c("column-rule-style", "solid"), None);
    }

    #[test]
    fn unknown_property_is_none() {
        assert_eq!(
            interpolate_gap_rule_value("column-rule-style", "solid", "dotted", 0.5),
            None
        );
        assert!(is_interpolable_gap_rule_property(
            "row-rule-inset-junction-end"
        ));
        assert!(!is_interpolable_gap_rule_property("rule-width"));
    }
}
