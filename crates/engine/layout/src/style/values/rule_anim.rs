//! CSS Gap Decorations L1 §4.7 — покадровая анимация `*-rule-width` / `*-rule-color`:
//! значения, которые планировщик переходов и `@keyframes` подкладывает под рисование
//! щелей вместо вычисленных, не пересчитывая раскладку.
//!
//! Арифметика — та же, что у Web Animations и `getComputedStyle()`
//! ([`super::rule_interp`]); здесь — типизированный результат ([`GapRuleOverride`]) и
//! мелкие помощники: какие свойства анимируются, какие токены `transition-property`
//! их покрывают, как получить вычисленное значение свойства строкой.

use lumen_core::geom::Size;

use crate::style::parse::box_sides::parse_line_width;
use crate::style::parse::color::parse_color;
use crate::style::values::rule_interp::{canonical_gap_rule_value, fmt_width};
use crate::style::values::rule_list::RuleList;
use crate::style::{Color, ComputedStyle, CssColor};

/// Свойства, которые рисование щелей умеет брать из анимации (kebab-case).
/// `*-rule-inset-*` интерполируются в Web Animations и `getComputedStyle()`, но в
/// покадровую отрисовку пока не заведены.
pub const PAINTED_GAP_RULE_PROPERTIES: [&str; 4] = [
    "column-rule-width",
    "row-rule-width",
    "column-rule-color",
    "row-rule-color",
];

/// Значения `*-rule-width` / `*-rule-color`, перекрывающие вычисленные на время
/// анимации или перехода. `None` — свойство не анимируется, действует вычисленное.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GapRuleOverride {
    /// `column-rule-width`.
    pub column_width: Option<RuleList<f32>>,
    /// `row-rule-width`.
    pub row_width: Option<RuleList<f32>>,
    /// `column-rule-color` (уже без `currentcolor`).
    pub column_color: Option<RuleList<Color>>,
    /// `row-rule-color` (уже без `currentcolor`).
    pub row_color: Option<RuleList<Color>>,
}

fn width_item(s: &str) -> Option<f32> {
    parse_line_width(s, 16.0, Size::ZERO, false).filter(|px| *px >= 0.0)
}

impl GapRuleOverride {
    /// `true`, если ни одно свойство не перекрыто.
    pub fn is_empty(&self) -> bool {
        self.column_width.is_none()
            && self.row_width.is_none()
            && self.column_color.is_none()
            && self.row_color.is_none()
    }

    /// Записывает значение `css` свойства `prop` (одно из
    /// [`PAINTED_GAP_RULE_PROPERTIES`]). `false` — свойство чужое или значение не разобралось.
    pub fn set(&mut self, prop: &str, css: &str) -> bool {
        match prop {
            "column-rule-width" | "row-rule-width" => {
                let Some(list) = RuleList::parse(css.trim(), &width_item) else {
                    return false;
                };
                if prop == "column-rule-width" {
                    self.column_width = Some(list);
                } else {
                    self.row_width = Some(list);
                }
                true
            }
            "column-rule-color" | "row-rule-color" => {
                let Some(list) = RuleList::parse(css.trim(), &|s: &str| parse_color(s)) else {
                    return false;
                };
                if prop == "column-rule-color" {
                    self.column_color = Some(list);
                } else {
                    self.row_color = Some(list);
                }
                true
            }
            _ => false,
        }
    }

    /// Накладывает `other` поверх `self`: что задано в `other`, побеждает.
    pub fn merge_from(&mut self, other: &GapRuleOverride) {
        if other.column_width.is_some() {
            self.column_width.clone_from(&other.column_width);
        }
        if other.row_width.is_some() {
            self.row_width.clone_from(&other.row_width);
        }
        if other.column_color.is_some() {
            self.column_color.clone_from(&other.column_color);
        }
        if other.row_color.is_some() {
            self.row_color.clone_from(&other.row_color);
        }
    }

    /// Подставляет перекрытые значения в `style`.
    pub fn apply_to(&self, style: &mut ComputedStyle) {
        if let Some(l) = &self.column_width {
            style.column_rule_width = l.clone();
        }
        if let Some(l) = &self.row_width {
            style.row_rule_width = l.clone();
        }
        if let Some(l) = &self.column_color {
            style.column_rule_color = l.map(|c| CssColor::Rgba(*c));
        }
        if let Some(l) = &self.row_color {
            style.row_rule_color = l.map(|c| CssColor::Rgba(*c));
        }
    }
}

/// Вычисленное значение свойства `prop` строкой (форма `repeat()` сохраняется,
/// `currentcolor` разрешён в цвет элемента — так значение участвует в интерполяции).
pub fn gap_rule_computed_css(style: &ComputedStyle, prop: &str) -> Option<String> {
    let colors = |l: &RuleList<CssColor>| {
        l.map(|c| c.resolve(style.color))
            .to_css(|c| crate::selector_query::color_to_css(*c))
    };
    match prop {
        "column-rule-width" => Some(style.column_rule_width.to_css(fmt_width)),
        "row-rule-width" => Some(style.row_rule_width.to_css(fmt_width)),
        "column-rule-color" => Some(colors(&style.column_rule_color)),
        "row-rule-color" => Some(colors(&style.row_rule_color)),
        _ => None,
    }
}

/// Приводит значение, написанное в `@keyframes`, к виду computed value: цвета — в
/// `rgb()` с разрешённым `currentcolor`, ширины — в `px` с привязкой как у `border-width`.
/// `None` — значение не разобралось.
pub fn gap_rule_endpoint_css(prop: &str, css: &str, current: Color) -> Option<String> {
    if prop.ends_with("-color") {
        let item = |s: &str| {
            if s.trim().eq_ignore_ascii_case("currentcolor") {
                Some(current)
            } else {
                parse_color(s)
            }
        };
        let list = RuleList::parse(css.trim(), &item)?;
        return Some(list.to_css(|c| crate::selector_query::color_to_css(*c)));
    }
    canonical_gap_rule_value(prop, css)
}

/// Покрывает ли токен `transition-property` свойство `prop` (`all`, шортхенды
/// `rule` / `column-rule` / `rule-width` …, сам лонгхенд).
pub fn transition_token_covers(token: &str, prop: &str) -> bool {
    let token = token.trim();
    if token.eq_ignore_ascii_case("all") {
        return true;
    }
    let Some((axis, part)) = prop.split_once("-rule-") else {
        return false;
    };
    let token = token.to_ascii_lowercase();
    token == format!("{axis}-rule")
        || token == "rule"
        || token == prop
        || token == format!("rule-{part}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_parses_lists_and_rejects_foreign_names() {
        let mut o = GapRuleOverride::default();
        assert!(o.is_empty());
        assert!(o.set("column-rule-width", "4px, repeat(auto, 2px)"));
        assert!(o.set("row-rule-color", "rgb(255, 0, 0)"));
        assert!(!o.set("column-rule-style", "solid"));
        assert!(!o.set("column-rule-width", "red"));
        assert_eq!(o.column_width.as_ref().map(|l| *l.first()), Some(4.0));
        assert!(o.row_width.is_none());
        assert!(!o.is_empty());
    }

    #[test]
    fn apply_to_replaces_only_overridden_fields() {
        let mut o = GapRuleOverride::default();
        o.set("column-rule-width", "7px");
        o.set("row-rule-color", "rgb(0, 128, 0)");
        let mut s = ComputedStyle::root();
        let row_w = s.row_rule_width.clone();
        o.apply_to(&mut s);
        assert_eq!(*s.column_rule_width.first(), 7.0);
        assert_eq!(s.row_rule_width, row_w);
        assert_eq!(
            *s.row_rule_color.first(),
            CssColor::Rgba(Color { r: 0, g: 128, b: 0, a: 255 })
        );
    }

    #[test]
    fn merge_later_wins_per_field() {
        let mut a = GapRuleOverride::default();
        a.set("column-rule-width", "1px");
        a.set("row-rule-width", "2px");
        let mut b = GapRuleOverride::default();
        b.set("column-rule-width", "9px");
        a.merge_from(&b);
        assert_eq!(*a.column_width.unwrap().first(), 9.0);
        assert_eq!(*a.row_width.unwrap().first(), 2.0);
    }

    #[test]
    fn computed_css_resolves_currentcolor() {
        let mut s = ComputedStyle::root();
        s.color = Color { r: 1, g: 2, b: 3, a: 255 };
        assert_eq!(
            gap_rule_computed_css(&s, "column-rule-color").as_deref(),
            Some("rgb(1, 2, 3)")
        );
        assert_eq!(
            gap_rule_computed_css(&s, "row-rule-width").as_deref(),
            Some("3px")
        );
        assert_eq!(gap_rule_computed_css(&s, "column-rule-style"), None);
    }

    #[test]
    fn endpoint_css_canonicalizes() {
        let cur = Color { r: 9, g: 9, b: 9, a: 255 };
        assert_eq!(
            gap_rule_endpoint_css("column-rule-color", "currentcolor, red", cur).as_deref(),
            Some("rgb(9, 9, 9), rgb(255, 0, 0)")
        );
        assert_eq!(
            gap_rule_endpoint_css("row-rule-width", "2.7px", cur).as_deref(),
            Some("2px")
        );
        assert_eq!(gap_rule_endpoint_css("row-rule-width", "bogus", cur), None);
    }

    #[test]
    fn transition_tokens_cover_shorthands() {
        for t in ["all", "rule", "column-rule", "rule-width", "column-rule-width", "RULE"] {
            assert!(transition_token_covers(t, "column-rule-width"), "{t}");
        }
        for t in ["row-rule", "rule-color", "column-rule-color", "opacity", "none"] {
            assert!(!transition_token_covers(t, "column-rule-width"), "{t}");
        }
    }
}
