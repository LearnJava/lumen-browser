//! Тесты `style.rs`: `column-rule*` / `row-rule*` / `rule*` (CSS Gap Decorations L1) в каскаде.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

    use super::*;
    use lumen_core::geom::Size;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn parse_gap_rule(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(r#"<div></div>"#);
        let sheet = lumen_css_parser::parse(&format!("div {{ display: flex; {} }}", css));
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let child = doc.get(body).children.first().copied().expect("div");
        compute_style(&doc, child, &sheet, &root, VP, false)
    }

    fn rgb(c: CssColor) -> (u8, u8, u8) {
        match c {
            CssColor::Rgba(c) => (c.r, c.g, c.b),
            other => panic!("expected Rgba, got {other:?}"),
        }
    }

    #[test]
    fn row_rule_longhands_parse() {
        let s = parse_gap_rule("row-rule-width: 4px; row-rule-style: solid; row-rule-color: #ff0000;");
        assert!((s.row_rule_width - 4.0).abs() < 0.01, "row_rule_width={}", s.row_rule_width);
        assert_eq!(s.row_rule_style, BorderStyle::Solid);
        assert_eq!(rgb(s.row_rule_color), (255, 0, 0));
        // column axis untouched
        assert_eq!(s.column_rule_style, BorderStyle::None);
    }

    #[test]
    fn row_rule_shorthand_parses_all_components() {
        let s = parse_gap_rule("row-rule: 3px dashed blue;");
        assert!((s.row_rule_width - 3.0).abs() < 0.01, "width={}", s.row_rule_width);
        assert_eq!(s.row_rule_style, BorderStyle::Dashed);
        assert_eq!(rgb(s.row_rule_color), (0, 0, 255));
        assert_eq!(s.column_rule_style, BorderStyle::None, "row-rule must not touch the column axis");
    }

    #[test]
    fn column_rule_shorthand_does_not_touch_row_axis() {
        let s = parse_gap_rule("column-rule: 2px solid red;");
        assert!((s.column_rule_width - 2.0).abs() < 0.01);
        assert_eq!(s.column_rule_style, BorderStyle::Solid);
        assert_eq!(s.row_rule_style, BorderStyle::None);
        assert_eq!(s.row_rule_width, 3.0, "row_rule_width stays at its initial `medium`");
    }

    #[test]
    fn rule_shorthand_sets_both_axes() {
        let s = parse_gap_rule("rule: 5px dotted #00ff00;");
        for (w, st, c) in [
            (s.column_rule_width, s.column_rule_style, s.column_rule_color),
            (s.row_rule_width, s.row_rule_style, s.row_rule_color),
        ] {
            assert!((w - 5.0).abs() < 0.01);
            assert_eq!(st, BorderStyle::Dotted);
            assert_eq!(rgb(c), (0, 255, 0));
        }
    }

    #[test]
    fn rule_longhands_set_both_axes() {
        let s = parse_gap_rule("rule-width: 2px; rule-style: solid; rule-color: red;");
        assert!((s.column_rule_width - 2.0).abs() < 0.01);
        assert!((s.row_rule_width - 2.0).abs() < 0.01);
        assert_eq!(s.column_rule_style, BorderStyle::Solid);
        assert_eq!(s.row_rule_style, BorderStyle::Solid);
        assert_eq!(rgb(s.column_rule_color), (255, 0, 0));
        assert_eq!(rgb(s.row_rule_color), (255, 0, 0));
    }

    #[test]
    fn rule_shorthand_resets_omitted_components() {
        // `rule: solid` after a wider longhand resets width to `medium`, color to currentColor.
        let s = parse_gap_rule("row-rule-width: 9px; row-rule-color: red; row-rule: solid;");
        assert_eq!(s.row_rule_width, 3.0);
        assert_eq!(s.row_rule_style, BorderStyle::Solid);
        assert_eq!(s.row_rule_color, CssColor::CurrentColor);
    }

    #[test]
    fn rule_shorthand_invalid_is_ignored() {
        // Дубль компонента и мусорный токен делают декларацию невалидной.
        let s = parse_gap_rule("row-rule: 2px solid dashed;");
        assert_eq!(s.row_rule_style, BorderStyle::None);
        let s = parse_gap_rule("row-rule: 2px solid red bogus;");
        assert_eq!(s.row_rule_style, BorderStyle::None);
    }

    #[test]
    fn rule_line_width_keywords() {
        let s = parse_gap_rule("row-rule-width: thick; column-rule-width: thin;");
        assert_eq!(s.row_rule_width, 5.0);
        assert_eq!(s.column_rule_width, 1.0);
    }

    #[test]
    fn rule_not_inherited() {
        // *-rule-* are non-inherited; child gets initial medium/None/CurrentColor.
        let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
        let sheet = lumen_css_parser::parse("div { row-rule: 5px solid red; column-rule: 5px solid red; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children.first().copied().expect("div");
        let span = doc.get(div).children.first().copied().expect("span");
        let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
        assert!((div_style.row_rule_width - 5.0).abs() < 0.01);
        assert_eq!(span_style.row_rule_style, BorderStyle::None, "row_rule_style must not be inherited");
        assert_eq!(span_style.column_rule_style, BorderStyle::None, "column_rule_style must not be inherited");
    }

    #[test]
    fn rule_properties_are_supported() {
        for p in ["row-rule", "row-rule-width", "row-rule-style", "row-rule-color", "rule", "rule-width", "rule-style", "rule-color"] {
            assert!(lumen_css_parser::SUPPORTED_PROPERTIES.contains(&p), "{p} missing from SUPPORTED_PROPERTIES");
        }
    }

// ── CSS Gap Decorations L1: *-rule-break / *-rule-visibility-items / rule-overlap ──

#[test]
fn rule_keyword_longhands_initial_values() {
    let s = parse_gap_rule("color: red;");
    assert_eq!(s.column_rule_break, RuleBreak::Normal);
    assert_eq!(s.row_rule_break, RuleBreak::Normal);
    assert_eq!(s.column_rule_visibility_items, RuleVisibilityItems::Normal);
    assert_eq!(s.row_rule_visibility_items, RuleVisibilityItems::Normal);
    assert_eq!(s.rule_overlap, RuleOverlap::RowOverColumn);
}

#[test]
fn rule_break_longhands_and_shorthand() {
    let s = parse_gap_rule("column-rule-break: intersection; row-rule-break: none;");
    assert_eq!(s.column_rule_break, RuleBreak::Intersection);
    assert_eq!(s.row_rule_break, RuleBreak::None);
    let s = parse_gap_rule("rule-break: intersection;");
    assert_eq!(s.column_rule_break, RuleBreak::Intersection);
    assert_eq!(s.row_rule_break, RuleBreak::Intersection);
    // Later longhand overrides one axis of the shorthand.
    let s = parse_gap_rule("rule-break: none; row-rule-break: normal;");
    assert_eq!(s.column_rule_break, RuleBreak::None);
    assert_eq!(s.row_rule_break, RuleBreak::Normal);
}

#[test]
fn rule_visibility_items_longhands_and_shorthand() {
    let s = parse_gap_rule("column-rule-visibility-items: around; row-rule-visibility-items: between;");
    assert_eq!(s.column_rule_visibility_items, RuleVisibilityItems::Around);
    assert_eq!(s.row_rule_visibility_items, RuleVisibilityItems::Between);
    let s = parse_gap_rule("rule-visibility-items: all;");
    assert_eq!(s.column_rule_visibility_items, RuleVisibilityItems::All);
    assert_eq!(s.row_rule_visibility_items, RuleVisibilityItems::All);
}

#[test]
fn rule_keyword_invalid_values_are_dropped() {
    // Mirrors css-gaps `*-invalid.html`: a bad value leaves the previous one in place.
    for bad in ["auto", "true", "10px", "default"] {
        let s = parse_gap_rule(&format!("rule-break: none; rule-break: {bad};"));
        assert_eq!(s.column_rule_break, RuleBreak::None, "rule-break: {bad}");
    }
    for bad in ["true", "10px", "default", "none", "auto"] {
        let s = parse_gap_rule(&format!(
            "rule-visibility-items: all; rule-visibility-items: {bad};"
        ));
        assert_eq!(s.row_rule_visibility_items, RuleVisibilityItems::All, "visibility: {bad}");
    }
    for bad in ["auto", "none", "10px", "10%", "true"] {
        let s = parse_gap_rule(&format!("rule-overlap: column-over-row; rule-overlap: {bad};"));
        assert_eq!(s.rule_overlap, RuleOverlap::ColumnOverRow, "rule-overlap: {bad}");
    }
}

#[test]
fn rule_overlap_parses() {
    let s = parse_gap_rule("rule-overlap: column-over-row;");
    assert_eq!(s.rule_overlap, RuleOverlap::ColumnOverRow);
}

#[test]
fn rule_keyword_properties_not_inherited() {
    let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
    let sheet = lumen_css_parser::parse(
        "div { rule-break: none; rule-visibility-items: all; rule-overlap: column-over-row; }",
    );
    let root = ComputedStyle::root();
    let body = doc.body().expect("body");
    let div = doc.get(body).children.first().copied().expect("div");
    let span = doc.get(div).children.first().copied().expect("span");
    let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
    let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
    assert_eq!(div_style.column_rule_break, RuleBreak::None);
    assert_eq!(span_style.column_rule_break, RuleBreak::Normal);
    assert_eq!(span_style.row_rule_visibility_items, RuleVisibilityItems::Normal);
    assert_eq!(span_style.rule_overlap, RuleOverlap::RowOverColumn);
}

#[test]
fn rule_keyword_css_wide_keywords() {
    // `inherit` pulls the parent's value; `initial` resets to the spec initial.
    let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
    let sheet = lumen_css_parser::parse(
        "div { rule-break: none; rule-overlap: column-over-row; column-rule-width: 7px; }          span { rule-break: inherit; rule-overlap: inherit; column-rule-width: inherit;                 row-rule-width: 9px; row-rule-width: initial; }",
    );
    let root = ComputedStyle::root();
    let body = doc.body().expect("body");
    let div = doc.get(body).children.first().copied().expect("div");
    let span = doc.get(div).children.first().copied().expect("span");
    let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
    let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
    assert_eq!(span_style.column_rule_break, RuleBreak::None);
    assert_eq!(span_style.row_rule_break, RuleBreak::None);
    assert_eq!(span_style.rule_overlap, RuleOverlap::ColumnOverRow);
    assert_eq!(span_style.column_rule_width, 7.0);
    assert_eq!(span_style.row_rule_width, 3.0);
}

#[test]
fn rule_keyword_properties_are_supported_and_computed() {
    for p in [
        "column-rule-break", "row-rule-break", "rule-break", "column-rule-visibility-items",
        "row-rule-visibility-items", "rule-visibility-items", "rule-overlap",
    ] {
        assert!(lumen_css_parser::SUPPORTED_PROPERTIES.contains(&p), "{p} missing");
    }
    let m = crate::computed_style_to_map(&ComputedStyle::root());
    assert_eq!(m.get("row-rule-break").map(String::as_str), Some("normal"));
    assert_eq!(m.get("column-rule-visibility-items").map(String::as_str), Some("normal"));
    assert_eq!(m.get("rule-overlap").map(String::as_str), Some("row-over-column"));
}
