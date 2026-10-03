//! CSS Gap Decorations L1 §3–§4 — computed value строкой для `getComputedStyle()`:
//! лонгхенды обеих осей и все шортхенды (`rule`, `rule-width`, `rule-inset-cap`, …).
//!
//! Computed value = as specified, но `currentcolor` разрешён в цвет элемента, а длины
//! `*-rule-inset*` сведены к px там, где процент не мешает (`0.5em` → `20px`,
//! `calc(10px + 0.5em)` → `30px`; `30%` и `calc(25% + 10px)` остаются как есть).
//! Шортхенд читается как пустая строка, если лонгхенды нельзя собрать обратно в одно
//! значение: списки разной формы или оси расходятся (CSSOM §6.7.2).

use std::collections::HashMap;

use lumen_core::geom::Size;

use crate::selector_query::{border_style_to_css, color_to_css, length_to_css, px_str};
use crate::style::calc::calc_node_contains_percent;
use crate::style::values::misc::{RuleInset, RuleInsets};
use crate::style::values::rule_list::{RuleItem, RuleList};
use crate::style::{BorderStyle, Color, ComputedStyle, CssColor, Length};

const INSET_PARTS: [&str; 4] = ["cap-start", "cap-end", "junction-start", "junction-end"];

fn color_css(c: &CssColor, current: Color) -> String {
    color_to_css(c.resolve(current))
}

/// Длина inset в computed-форме: px, если известна без базиса процента.
fn inset_length_css(l: &Length, font_size: f32) -> String {
    let has_percent = match l {
        Length::Percent(_) => true,
        Length::Calc(node) => calc_node_contains_percent(node),
        _ => false,
    };
    if !has_percent && let Some(px) = l.resolve(font_size, None, Size::ZERO) {
        return px_str(px);
    }
    length_to_css(l)
}

fn inset_css(i: &RuleInset, font_size: f32) -> String {
    match i {
        RuleInset::Length(l) => inset_length_css(l, font_size),
        RuleInset::OverlapJoin => "overlap-join".into(),
    }
}

/// `<width> <style> <color>` по спискам одной оси, если их формы совпадают.
fn axis_triplets(
    w: &RuleList<f32>,
    st: &RuleList<BorderStyle>,
    c: &RuleList<CssColor>,
    current: Color,
) -> Option<String> {
    let (wi, si, ci) = (w.items(), st.items(), c.items());
    if wi.len() != si.len() || si.len() != ci.len() {
        return None;
    }
    // `none` опускается, как у шортхенда в CSSOM (`rule_cssom::triplet_text`): значение
    // должно переживать круг `style.rule = getComputedStyle().rule`.
    let one = |w: &f32, s: &BorderStyle, c: &CssColor| {
        if *s == BorderStyle::None {
            format!("{} {}", px_str(*w), color_css(c, current))
        } else {
            format!("{} {} {}", px_str(*w), border_style_to_css(*s), color_css(c, current))
        }
    };
    let many = |w: &[f32], s: &[BorderStyle], c: &[CssColor]| -> Option<String> {
        if w.len() != s.len() || s.len() != c.len() {
            return None;
        }
        Some(
            w.iter()
                .zip(s)
                .zip(c)
                .map(|((w, s), c)| one(w, s, c))
                .collect::<Vec<_>>()
                .join(", "),
        )
    };
    let mut parts = Vec::new();
    for ((w, s), c) in wi.iter().zip(si).zip(ci) {
        parts.push(match (w, s, c) {
            (RuleItem::One(w), RuleItem::One(s), RuleItem::One(c)) => one(w, s, c),
            (RuleItem::Repeat(n, w), RuleItem::Repeat(m, s), RuleItem::Repeat(k, c))
                if n == m && m == k =>
            {
                format!("repeat({n}, {})", many(w, s, c)?)
            }
            (RuleItem::Auto(w), RuleItem::Auto(s), RuleItem::Auto(c)) => {
                format!("repeat(auto, {})", many(w, s, c)?)
            }
            _ => return None,
        });
    }
    Some(parts.join(", "))
}

/// Значение шортхенда двух осей: общее, если оси согласны, иначе пусто.
fn both_axes(col: Option<String>, row: Option<String>) -> String {
    match (col, row) {
        (Some(a), Some(b)) if a == b => a,
        _ => String::new(),
    }
}

/// Сборка `[cap-start, cap-end, junction-start, junction-end]` в значение свойства
/// `*-rule-inset[suffix]`; `None`, если слоты свойства расходятся и записать их
/// одним значением нельзя.
fn inset_group(vals: &[String; 4], suffix: &str) -> Option<String> {
    let same = |ix: &[usize]| -> Option<String> {
        let first = &vals[ix[0]];
        ix.iter().all(|&i| &vals[i] == first).then(|| first.clone())
    };
    match suffix {
        "" => Some(same(&[0, 1, 2, 3]).unwrap_or_else(|| {
            format!("{} {} / {} {}", vals[0], vals[1], vals[2], vals[3])
        })),
        "-cap" => Some(same(&[0, 1]).unwrap_or_else(|| format!("{} {}", vals[0], vals[1]))),
        "-junction" => Some(same(&[2, 3]).unwrap_or_else(|| format!("{} {}", vals[2], vals[3]))),
        "-start" => same(&[0, 2]),
        "-end" => same(&[1, 3]),
        _ => None,
    }
}

fn inset_values(i: &RuleInsets, font_size: f32) -> [String; 4] {
    [0, 1, 2, 3].map(|k| inset_css(i.slot(k), font_size))
}

/// Все computed value семейства Gap Decorations (`column-rule*`, `row-rule*`, `rule*`).
pub(crate) fn insert_gap_rule_computed(style: &ComputedStyle, m: &mut HashMap<String, String>) {
    let cur = style.color;
    let fs = style.font_size;
    let (cw, cs, cc) = (&style.column_rule_width, &style.column_rule_style, &style.column_rule_color);
    let (rw, rs, rc) = (&style.row_rule_width, &style.row_rule_style, &style.row_rule_color);
    let width = |l: &RuleList<f32>| l.to_css(|v| px_str(*v));
    let line_style = |l: &RuleList<BorderStyle>| l.to_css(|v| border_style_to_css(*v).into());
    let color = |l: &RuleList<CssColor>| l.to_css(|v| color_css(v, cur));

    for (axis, w, s, c) in [("column", cw, cs, cc), ("row", rw, rs, rc)] {
        m.insert(format!("{axis}-rule-width"), width(w));
        m.insert(format!("{axis}-rule-style"), line_style(s));
        m.insert(format!("{axis}-rule-color"), color(c));
        m.insert(format!("{axis}-rule"), axis_triplets(w, s, c, cur).unwrap_or_default());
    }
    m.insert("rule-width".into(), both_axes(Some(width(cw)), Some(width(rw))));
    m.insert("rule-style".into(), both_axes(Some(line_style(cs)), Some(line_style(rs))));
    m.insert("rule-color".into(), both_axes(Some(color(cc)), Some(color(rc))));
    m.insert(
        "rule".into(),
        both_axes(axis_triplets(cw, cs, cc, cur), axis_triplets(rw, rs, rc, cur)),
    );

    // §3.2 / §3.4 — ключевые слова; `rule-*` — общее значение осей.
    let (cb, rb) = (style.column_rule_break.to_css(), style.row_rule_break.to_css());
    m.insert("column-rule-break".into(), cb.into());
    m.insert("row-rule-break".into(), rb.into());
    m.insert("rule-break".into(), if cb == rb { cb.into() } else { String::new() });
    let (cv, rv) = (
        style.column_rule_visibility_items.to_css(),
        style.row_rule_visibility_items.to_css(),
    );
    m.insert("column-rule-visibility-items".into(), cv.into());
    m.insert("row-rule-visibility-items".into(), rv.into());
    m.insert("rule-visibility-items".into(), if cv == rv { cv.into() } else { String::new() });
    m.insert("rule-overlap".into(), style.rule_overlap.to_css().into());

    // §3.3 — inset: восемь лонгхендов и шортхенды на каждую ось и на обе сразу.
    let ci = inset_values(&style.column_rule_inset, fs);
    let ri = inset_values(&style.row_rule_inset, fs);
    for (axis, vals) in [("column", &ci), ("row", &ri)] {
        for (k, part) in INSET_PARTS.iter().enumerate() {
            m.insert(format!("{axis}-rule-inset-{part}"), vals[k].clone());
        }
    }
    for suffix in ["", "-cap", "-junction", "-start", "-end"] {
        let (c, r) = (inset_group(&ci, suffix), inset_group(&ri, suffix));
        m.insert(format!("column-rule-inset{suffix}"), c.clone().unwrap_or_default());
        m.insert(format!("row-rule-inset{suffix}"), r.clone().unwrap_or_default());
        m.insert(format!("rule-inset{suffix}"), both_axes(c, r));
    }
    // Одиночные слоты `rule-inset-cap-start` и т. д.: оси должны совпасть.
    for (k, part) in INSET_PARTS.iter().enumerate() {
        m.insert(
            format!("rule-inset-{part}"),
            both_axes(Some(ci[k].clone()), Some(ri[k].clone())),
        );
    }
}
