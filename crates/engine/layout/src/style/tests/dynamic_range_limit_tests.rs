//! Тесты `dynamic-range-limit` (CSS Color HDR L1 §2, BUG-508):
//! `style::values::dynamic_range_limit::DynamicRangeLimit`.
//!
//! Кейсы зеркалят `tests/wpt/css/css-color-hdr/{parsing,computed,inheritance,
//! interpolation}.html`, чтобы регресс здесь ловился раньше wptrunner.

use super::*;
use lumen_core::geom::Size;

const VP: Size = Size { width: 800.0, height: 600.0 };
const EM: f32 = 16.0;

fn parse(s: &str) -> Option<DynamicRangeLimit> {
    DynamicRangeLimit::parse(s, EM, VP, false)
}

fn computed_css(css: &str) -> String {
    style_for(&format!("dynamic-range-limit: {css};")).dynamic_range_limit.to_css()
}

// ── initial / inheritance ───────────────────────────────────────────────

#[test]
fn initial_value_is_no_limit() {
    assert_eq!(ComputedStyle::root().dynamic_range_limit, DynamicRangeLimit::default());
    assert_eq!(DynamicRangeLimit::default().to_css(), "no-limit");
}

#[test]
fn unset_declaration_keeps_initial() {
    let s = style_for("color: red;");
    assert_eq!(s.dynamic_range_limit.to_css(), "no-limit");
}

#[test]
fn child_inherits_parent_value() {
    let doc = lumen_html_parser::parse(r#"<div style="dynamic-range-limit: standard"><p>x</p></div>"#);
    let sheet = lumen_css_parser::parse("");
    let root_style = ComputedStyle::root();
    let div = doc.get(doc.body().unwrap()).children[0];
    let p = doc.get(div).children[0];
    let div_style = compute_style(&doc, div, &sheet, &root_style, VP, false);
    let p_style = compute_style(&doc, p, &sheet, &div_style, VP, false);
    assert_eq!(p_style.dynamic_range_limit.to_css(), "standard");
}

// ── parsing: valid bare keywords ────────────────────────────────────────

#[test]
fn parses_bare_keywords() {
    assert_eq!(computed_css("standard"), "standard");
    assert_eq!(computed_css("constrained"), "constrained");
    assert_eq!(computed_css("no-limit"), "no-limit");
}

// ── computed value: normalization/collapse (computed.html) ─────────────

#[test]
fn mix_collapses_to_keyword_when_one_component_wins() {
    assert_eq!(computed_css("dynamic-range-limit-mix(no-limit 100%, standard 0%)"), "no-limit");
    assert_eq!(computed_css("dynamic-range-limit-mix(no-limit 100%, no-limit 0%)"), "no-limit");
}

#[test]
fn mix_renormalizes_by_percentage_sum() {
    // sum=200 → halved.
    assert_eq!(
        computed_css("dynamic-range-limit-mix(no-limit 100%, standard 100%)"),
        "dynamic-range-limit-mix(standard 50%, no-limit 50%)"
    );
    // sum=200, three components.
    assert_eq!(
        computed_css("dynamic-range-limit-mix(no-limit 80%, standard 60%, constrained 60%)"),
        "dynamic-range-limit-mix(standard 30%, constrained 30%, no-limit 40%)"
    );
}

#[test]
fn mix_sums_duplicate_keyword_components() {
    assert_eq!(
        computed_css("dynamic-range-limit-mix(no-limit 25%, standard 25%, standard 25%, standard 25%)"),
        "dynamic-range-limit-mix(standard 75%, no-limit 25%)"
    );
}

#[test]
fn mix_canonical_order_is_standard_constrained_no_limit() {
    assert_eq!(
        computed_css("dynamic-range-limit-mix(constrained 75%, standard 25%)"),
        "dynamic-range-limit-mix(standard 25%, constrained 75%)"
    );
}

#[test]
fn mix_flattens_nested_mix() {
    assert_eq!(
        computed_css(
            "dynamic-range-limit-mix(dynamic-range-limit-mix(standard 10%, no-limit 30%) 20%, standard 80%)"
        ),
        "dynamic-range-limit-mix(standard 85%, no-limit 15%)"
    );
    assert_eq!(
        computed_css(
            "dynamic-range-limit-mix(no-limit 10%, dynamic-range-limit-mix(standard 25%, constrained 75%) 20%, \
             dynamic-range-limit-mix(constrained 10%, no-limit 30%) 20%)"
        ),
        "dynamic-range-limit-mix(standard 10%, constrained 40%, no-limit 50%)"
    );
}

#[test]
fn mix_calc_percentage_argument() {
    // sign(10em - 1px) with em=16px → 160px-1px > 0 → sign=1 → 50%*1 = 50%.
    assert_eq!(
        computed_css("dynamic-range-limit-mix(standard calc(50% * sign(10em - 1px)), constrained 50%)"),
        "dynamic-range-limit-mix(standard 50%, constrained 50%)"
    );
}

// ── parsing: invalid values (parsing.html) ──────────────────────────────

#[test]
fn rejects_invalid_values() {
    let invalid = [
        "",
        "none",
        "default",
        "hdr",
        "sdr",
        "low",
        "dynamic-range-limit-mix(no-limit 80%, standard 20%, )",
        "dynamic-range-limit-mix(no-limit, standard 20%)",
        "dynamic-range-limit-mix(constrained, no-limit, 80%)",
        "dynamic-range-limit-mix(no-limit 1%)",
        "dynamic-range-limit-mix(no-limit 80% standard 20%)",
        "dynamic-range-limit-mix(low, no-limit, 10%)",
        "dynamic-range-limit-mix(no-limit 101%, standard 1%)",
        "dynamic-range-limit-mix(no-limit -1%, standard 1%)",
        "dynamic-range-limit-mix(standard, no-limit, 0.1)",
        "dynamic-range-limit-mix(no-limit 0%, standard 0%)",
        "dynamic-range-limit-mix(dynamic-range-limit-mix(no-limit 1%, standard 2%) 3%, \
         dynamic-range-limit-mix(constrained 0%, no-limit 0%) 6%)",
    ];
    for v in invalid {
        assert!(parse(v).is_none(), "expected {v:?} to be invalid");
    }
}

#[test]
fn accepts_percent_before_keyword_component() {
    // CSS `&&` combinator: `<value> && <percentage>` — either order.
    assert_eq!(parse("dynamic-range-limit-mix(80% no-limit, 20% standard)"), parse(
        "dynamic-range-limit-mix(no-limit 80%, standard 20%)"
    ));
}

// ── interpolation (interpolation.html) ──────────────────────────────────

#[test]
fn interpolate_between_bare_keywords() {
    let from = parse("no-limit").unwrap();
    let to = parse("standard").unwrap();
    assert_eq!(DynamicRangeLimit::interpolate(&from, &to, 0.0).to_css(), "no-limit");
    assert_eq!(
        DynamicRangeLimit::interpolate(&from, &to, 0.25).to_css(),
        "dynamic-range-limit-mix(standard 25%, no-limit 75%)"
    );
    assert_eq!(
        DynamicRangeLimit::interpolate(&from, &to, 0.75).to_css(),
        "dynamic-range-limit-mix(standard 75%, no-limit 25%)"
    );
    assert_eq!(DynamicRangeLimit::interpolate(&from, &to, 1.0).to_css(), "standard");
}

#[test]
fn interpolate_between_two_mixes_three_way() {
    let from = parse("dynamic-range-limit-mix(constrained 90%, standard 10%)").unwrap();
    let to = parse("dynamic-range-limit-mix(no-limit 10%, standard 90%)").unwrap();
    assert_eq!(
        DynamicRangeLimit::interpolate(&from, &to, 0.0).to_css(),
        "dynamic-range-limit-mix(standard 10%, constrained 90%)"
    );
    assert_eq!(
        DynamicRangeLimit::interpolate(&from, &to, 0.5).to_css(),
        "dynamic-range-limit-mix(standard 50%, constrained 45%, no-limit 5%)"
    );
    assert_eq!(
        DynamicRangeLimit::interpolate(&from, &to, 1.0).to_css(),
        "dynamic-range-limit-mix(standard 90%, no-limit 10%)"
    );
}
