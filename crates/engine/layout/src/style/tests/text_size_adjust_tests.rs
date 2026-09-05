//! Тесты `text-size-adjust` (CSS Text Size Adjustment Module L1 §2, BUG-513):
//! `style::values::text_size_adjust::TextSizeAdjust`.
//!
//! Кейсы зеркалят `tests/wpt/css/css-size-adjust/{parsing,inheritance,
//! animations}/*.html`, чтобы регресс здесь ловился раньше wptrunner.
//! `calc(10% * sibling-index())` (одно из полей `text-size-adjust-valid.html`)
//! не покрыт — `sibling-index()` (CSS Values L5) не реализована нигде в
//! движке, отдельный пробел вне скоупа этого бага.

use super::*;
use lumen_core::geom::Size;

const VP: Size = Size { width: 800.0, height: 600.0 };
const EM: f32 = 16.0;

fn parse(s: &str) -> Option<TextSizeAdjust> {
    TextSizeAdjust::parse(s, EM, VP, false)
}

fn computed_css(css: &str) -> String {
    style_for(&format!("text-size-adjust: {css};")).text_size_adjust.to_css()
}

// ── initial / inheritance ───────────────────────────────────────────────

#[test]
fn initial_value_is_auto() {
    assert_eq!(ComputedStyle::root().text_size_adjust, TextSizeAdjust::default());
    assert_eq!(TextSizeAdjust::default().to_css(), "auto");
}

#[test]
fn unset_declaration_keeps_initial() {
    let s = style_for("color: red;");
    assert_eq!(s.text_size_adjust.to_css(), "auto");
}

#[test]
fn child_inherits_parent_value() {
    let doc = lumen_html_parser::parse(r#"<div style="text-size-adjust: 10%"><p>x</p></div>"#);
    let sheet = lumen_css_parser::parse("");
    let root_style = ComputedStyle::root();
    let div = doc.get(doc.body().unwrap()).children[0];
    let p = doc.get(div).children[0];
    let div_style = compute_style(&doc, div, &sheet, &root_style, VP, false);
    let p_style = compute_style(&doc, p, &sheet, &div_style, VP, false);
    assert_eq!(p_style.text_size_adjust.to_css(), "10%");
}

// ── parsing: valid values (parsing/text-size-adjust-valid.html) ─────────

#[test]
fn parses_auto() {
    assert_eq!(computed_css("auto"), "auto");
}

#[test]
fn none_computes_to_100_percent() {
    // Spec §propdef: `none` is a legacy synonym whose computed value is `100%`.
    assert_eq!(computed_css("none"), "100%");
}

#[test]
fn parses_percentages() {
    assert_eq!(computed_css("200%"), "200%");
    assert_eq!(computed_css("100%"), "100%");
    assert_eq!(computed_css("0%"), "0%");
}

#[test]
fn calc_percentage_sum_folds() {
    assert_eq!(computed_css("calc(10% + 5%)"), "15%");
}

// ── parsing: invalid values (parsing/text-size-adjust-invalid.html) ─────

#[test]
fn rejects_invalid_values() {
    let invalid = ["reverse", "0", "10px", "-100%", ""];
    for v in invalid {
        assert!(parse(v).is_none(), "expected {v:?} to be invalid");
    }
}

// ── interpolation (animations/text-size-adjust-interpolation.html) ──────
// Реального рендер-пайплайна авто-инфляции текста нет (BUG-513 — тот же
// класс, что BUG-508/`DynamicRangeLimit`): `interpolate` — чистая
// библиотечная функция, юнит-тестированная здесь, но ни нативный движок CSS
// Animations/Transitions (`crate::animation`), ни Web Animations JS-шим её
// сегодня не вызывают через реальный relayout — ДОРАБОТКА того же класса.

#[test]
fn interpolate_between_percentages() {
    let from = parse("60%").unwrap();
    let to = parse("50%").unwrap();
    assert_eq!(TextSizeAdjust::interpolate(&from, &to, 0.0).to_css(), "60%");
    assert_eq!(TextSizeAdjust::interpolate(&from, &to, 0.3).to_css(), "57%");
    assert_eq!(TextSizeAdjust::interpolate(&from, &to, 1.0).to_css(), "50%");
}

#[test]
fn interpolate_clamps_at_zero() {
    // "text-size-adjust can't be negative" — WPT's own comment on this case.
    let from = parse("10%").unwrap();
    let to = parse("0%").unwrap();
    assert_eq!(TextSizeAdjust::interpolate(&from, &to, 1.5).to_css(), "0%");
}

#[test]
fn interpolate_is_discrete_when_either_side_is_auto() {
    let auto = TextSizeAdjust::Auto;
    let pct = parse("70%").unwrap();
    assert_eq!(TextSizeAdjust::interpolate(&auto, &pct, 0.0).to_css(), "auto");
    assert_eq!(TextSizeAdjust::interpolate(&auto, &pct, 1.0).to_css(), "70%");
}
