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
        assert!((*s.row_rule_width.first() - 4.0).abs() < 0.01, "row_rule_width={}", *s.row_rule_width.first());
        assert_eq!(*s.row_rule_style.first(), BorderStyle::Solid);
        assert_eq!(rgb(*s.row_rule_color.first()), (255, 0, 0));
        // column axis untouched
        assert_eq!(*s.column_rule_style.first(), BorderStyle::None);
    }

    #[test]
    fn row_rule_shorthand_parses_all_components() {
        let s = parse_gap_rule("row-rule: 3px dashed blue;");
        assert!((*s.row_rule_width.first() - 3.0).abs() < 0.01, "width={}", *s.row_rule_width.first());
        assert_eq!(*s.row_rule_style.first(), BorderStyle::Dashed);
        assert_eq!(rgb(*s.row_rule_color.first()), (0, 0, 255));
        assert_eq!(*s.column_rule_style.first(), BorderStyle::None, "row-rule must not touch the column axis");
    }

    #[test]
    fn column_rule_shorthand_does_not_touch_row_axis() {
        let s = parse_gap_rule("column-rule: 2px solid red;");
        assert!((*s.column_rule_width.first() - 2.0).abs() < 0.01);
        assert_eq!(*s.column_rule_style.first(), BorderStyle::Solid);
        assert_eq!(*s.row_rule_style.first(), BorderStyle::None);
        assert_eq!(*s.row_rule_width.first(), 3.0, "row_rule_width stays at its initial `medium`");
    }

    #[test]
    fn rule_shorthand_sets_both_axes() {
        let s = parse_gap_rule("rule: 5px dotted #00ff00;");
        for (w, st, c) in [
            (*s.column_rule_width.first(), *s.column_rule_style.first(), *s.column_rule_color.first()),
            (*s.row_rule_width.first(), *s.row_rule_style.first(), *s.row_rule_color.first()),
        ] {
            assert!((w - 5.0).abs() < 0.01);
            assert_eq!(st, BorderStyle::Dotted);
            assert_eq!(rgb(c), (0, 255, 0));
        }
    }

    #[test]
    fn rule_longhands_set_both_axes() {
        let s = parse_gap_rule("rule-width: 2px; rule-style: solid; rule-color: red;");
        assert!((*s.column_rule_width.first() - 2.0).abs() < 0.01);
        assert!((*s.row_rule_width.first() - 2.0).abs() < 0.01);
        assert_eq!(*s.column_rule_style.first(), BorderStyle::Solid);
        assert_eq!(*s.row_rule_style.first(), BorderStyle::Solid);
        assert_eq!(rgb(*s.column_rule_color.first()), (255, 0, 0));
        assert_eq!(rgb(*s.row_rule_color.first()), (255, 0, 0));
    }

    #[test]
    fn rule_shorthand_resets_omitted_components() {
        // `rule: solid` after a wider longhand resets width to `medium`, color to currentColor.
        let s = parse_gap_rule("row-rule-width: 9px; row-rule-color: red; row-rule: solid;");
        assert_eq!(*s.row_rule_width.first(), 3.0);
        assert_eq!(*s.row_rule_style.first(), BorderStyle::Solid);
        assert_eq!(*s.row_rule_color.first(), CssColor::CurrentColor);
    }

    #[test]
    fn rule_shorthand_invalid_is_ignored() {
        // Дубль компонента и мусорный токен делают декларацию невалидной.
        let s = parse_gap_rule("row-rule: 2px solid dashed;");
        assert_eq!(*s.row_rule_style.first(), BorderStyle::None);
        let s = parse_gap_rule("row-rule: 2px solid red bogus;");
        assert_eq!(*s.row_rule_style.first(), BorderStyle::None);
    }

    #[test]
    fn rule_line_width_keywords() {
        let s = parse_gap_rule("row-rule-width: thick; column-rule-width: thin;");
        assert_eq!(*s.row_rule_width.first(), 5.0);
        assert_eq!(*s.column_rule_width.first(), 1.0);
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
        assert!((*div_style.row_rule_width.first() - 5.0).abs() < 0.01);
        assert_eq!(*span_style.row_rule_style.first(), BorderStyle::None, "row_rule_style must not be inherited");
        assert_eq!(*span_style.column_rule_style.first(), BorderStyle::None, "column_rule_style must not be inherited");
    }

    #[test]
    fn grid_gap_is_the_legacy_alias_of_gap() {
        // CSS Grid L1 §7.3: `grid-gap` = `gap` (WPT `css-gaps/grid/*` писали `grid-gap: 10px`).
        let s = parse_gap_rule("grid-gap: 10px 20px;");
        assert_eq!(s.row_gap, Length::Px(10.0));
        assert_eq!(s.column_gap, Length::Px(20.0));
        let s = parse_gap_rule("grid-gap: 7px;");
        assert_eq!((s.row_gap, s.column_gap), (Length::Px(7.0), Length::Px(7.0)));
        assert!(lumen_css_parser::SUPPORTED_PROPERTIES.contains(&"grid-gap"));
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
    assert_eq!(*span_style.column_rule_width.first(), 7.0);
    assert_eq!(*span_style.row_rule_width.first(), 3.0);
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

// ── CSS Gap Decorations L1 §3.3: *-rule-inset* ──────────────────────────────

fn inset_css(i: &RuleInset) -> String {
    i.to_css()
}

fn slots(r: &RuleInsets) -> [String; 4] {
    [inset_css(&r.cap_start), inset_css(&r.cap_end), inset_css(&r.junction_start), inset_css(&r.junction_end)]
}

#[test]
fn rule_inset_initial_is_zero() {
    let s = parse_gap_rule("color: red;");
    assert_eq!(slots(&s.column_rule_inset), ["0px", "0px", "0px", "0px"]);
    assert_eq!(slots(&s.row_rule_inset), ["0px", "0px", "0px", "0px"]);
}

#[test]
fn rule_inset_longhands() {
    let s = parse_gap_rule(
        "column-rule-inset-cap-start: 1px; column-rule-inset-cap-end: 2px;          column-rule-inset-junction-start: 3px; column-rule-inset-junction-end: overlap-join;          row-rule-inset-cap-end: 25%;",
    );
    assert_eq!(slots(&s.column_rule_inset), ["1px", "2px", "3px", "overlap-join"]);
    assert_eq!(slots(&s.row_rule_inset), ["0px", "25%", "0px", "0px"]);
}

#[test]
fn rule_inset_start_end_cap_junction_shorthands() {
    let s = parse_gap_rule("column-rule-inset-start: 4px;");
    assert_eq!(slots(&s.column_rule_inset), ["4px", "0px", "4px", "0px"]);
    let s = parse_gap_rule("row-rule-inset-end: 5px;");
    assert_eq!(slots(&s.row_rule_inset), ["0px", "5px", "0px", "5px"]);
    let s = parse_gap_rule("column-rule-inset-cap: 6px;");
    assert_eq!(slots(&s.column_rule_inset), ["6px", "6px", "0px", "0px"]);
    let s = parse_gap_rule("column-rule-inset-junction: 7px 8px;");
    assert_eq!(slots(&s.column_rule_inset), ["0px", "0px", "7px", "8px"]);
    // `rule-inset-*` задаёт обе оси.
    let s = parse_gap_rule("rule-inset-cap: 1px 2px;");
    assert_eq!(slots(&s.column_rule_inset), ["1px", "2px", "0px", "0px"]);
    assert_eq!(slots(&s.row_rule_inset), ["1px", "2px", "0px", "0px"]);
}

#[test]
fn rule_inset_full_shorthand_fills_omitted_values() {
    let s = parse_gap_rule("column-rule-inset: 1px;");
    assert_eq!(slots(&s.column_rule_inset), ["1px", "1px", "1px", "1px"]);
    let s = parse_gap_rule("column-rule-inset: 1px 2px;");
    assert_eq!(slots(&s.column_rule_inset), ["1px", "2px", "1px", "2px"]);
    let s = parse_gap_rule("column-rule-inset: 1px 2px / 3px 4px;");
    assert_eq!(slots(&s.column_rule_inset), ["1px", "2px", "3px", "4px"]);
    let s = parse_gap_rule("row-rule-inset: 1px / 3px;");
    assert_eq!(slots(&s.row_rule_inset), ["1px", "1px", "3px", "3px"]);
    let s = parse_gap_rule("rule-inset: -50% / overlap-join;");
    for r in [&s.column_rule_inset, &s.row_rule_inset] {
        assert_eq!(slots(r), ["-50%", "-50%", "overlap-join", "overlap-join"]);
    }
}

#[test]
fn rule_inset_invalid_values_are_dropped() {
    for bad in ["auto", "none", "1px 2px 3px", "", "red", "1px /", "/ 1px", "1px / 2px / 3px"] {
        let s = parse_gap_rule(&format!("column-rule-inset: 7px; column-rule-inset: {bad};"));
        assert_eq!(slots(&s.column_rule_inset), ["7px"; 4], "column-rule-inset: {bad:?}");
    }
    // Longhand — ровно одно значение.
    let s = parse_gap_rule("row-rule-inset-cap-start: 7px; row-rule-inset-cap-start: 1px 2px;");
    assert_eq!(inset_css(&s.row_rule_inset.cap_start), "7px");
    // Шортхенд cap/junction — не больше двух.
    let s = parse_gap_rule("row-rule-inset-cap: 7px; row-rule-inset-cap: 1px 2px 3px;");
    assert_eq!(slots(&s.row_rule_inset), ["7px", "7px", "0px", "0px"]);
}

#[test]
fn rule_inset_not_inherited_and_css_wide() {
    let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
    let sheet = lumen_css_parser::parse(
        "div { rule-inset: 9px; } span { column-rule-inset-cap: inherit; row-rule-inset-start: 4px; row-rule-inset-start: initial; }",
    );
    let root = ComputedStyle::root();
    let body = doc.body().expect("body");
    let div = doc.get(body).children.first().copied().expect("div");
    let span = doc.get(div).children.first().copied().expect("span");
    let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
    let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
    assert_eq!(slots(&div_style.row_rule_inset), ["9px"; 4]);
    // `inherit` берёт только cap-start/cap-end родителя.
    assert_eq!(slots(&span_style.column_rule_inset), ["9px", "9px", "0px", "0px"]);
    assert_eq!(slots(&span_style.row_rule_inset), ["0px"; 4], "not inherited; `initial` resets");
}

#[test]
fn rule_inset_properties_are_supported_and_computed() {
    let m = crate::computed_style_to_map(&parse_gap_rule("column-rule-inset: 2px / overlap-join;"));
    assert_eq!(m.get("column-rule-inset-cap-start").map(String::as_str), Some("2px"));
    assert_eq!(m.get("column-rule-inset-junction-end").map(String::as_str), Some("overlap-join"));
    assert_eq!(m.get("row-rule-inset-cap-end").map(String::as_str), Some("0px"));
    for p in [
        "rule-inset", "column-rule-inset", "row-rule-inset", "rule-inset-start", "rule-inset-end",
        "rule-inset-cap", "rule-inset-junction", "column-rule-inset-cap", "row-rule-inset-junction",
        "column-rule-inset-start", "row-rule-inset-end", "column-rule-inset-cap-start",
        "row-rule-inset-junction-end",
    ] {
        assert!(lumen_css_parser::SUPPORTED_PROPERTIES.contains(&p), "{p} missing");
    }
}

// ── CSS Gap Decorations L1 §4.4–§4.6: списки значений и repeat() ──

fn rule_css(s: &ComputedStyle, prop: &str) -> String {
    crate::computed_style_to_map(s).get(prop).cloned().unwrap_or_default()
}

#[test]
fn rule_longhand_lists_are_stored_with_repeat() {
    let s = parse_gap_rule(
        "column-rule-width: 1px, repeat(2, 2px, 3px), repeat(auto, 4px); column-rule-style: solid, dashed; \
         column-rule-color: red, repeat(auto, rgb(0, 0, 255), green);",
    );
    assert_eq!(rule_css(&s, "column-rule-width"), "1px, repeat(2, 2px, 3px), repeat(auto, 4px)");
    assert_eq!(rule_css(&s, "column-rule-style"), "solid, dashed");
    assert_eq!(rule_css(&s, "column-rule-color"), "rgb(255, 0, 0), repeat(auto, rgb(0, 0, 255), rgb(0, 128, 0))");
    // Ось строк не тронута.
    assert_eq!(rule_css(&s, "row-rule-width"), "3px");
    // Первая щель получает первое значение.
    assert_eq!(*s.column_rule_width.value_for_gap(0, 5), 1.0);
    assert_eq!(*s.column_rule_width.value_for_gap(1, 5), 2.0);
    assert_eq!(*s.column_rule_width.value_for_gap(4, 5), 3.0);
    assert_eq!(*s.column_rule_width.value_for_gap(5, 7), 4.0);
}

#[test]
fn rule_shorthand_accepts_gap_rule_lists() {
    let s = parse_gap_rule("column-rule: 1px solid red, repeat(auto, 2px dashed blue); row-rule: 4px dotted;");
    assert_eq!(rule_css(&s, "column-rule-width"), "1px, repeat(auto, 2px)");
    assert_eq!(rule_css(&s, "column-rule-style"), "solid, repeat(auto, dashed)");
    assert_eq!(rule_css(&s, "column-rule-color"), "rgb(255, 0, 0), repeat(auto, rgb(0, 0, 255))");
    assert_eq!(
        rule_css(&s, "column-rule"),
        "1px solid rgb(255, 0, 0), repeat(auto, 2px dashed rgb(0, 0, 255))"
    );
    // `currentcolor` в computed value — уже цвет элемента.
    assert_eq!(rule_css(&s, "row-rule"), "4px dotted rgb(0, 0, 0)");
    // Стиль `none` в тройке опускается (круг через `style.rule`).
    let s = parse_gap_rule("");
    assert_eq!(rule_css(&s, "column-rule"), "3px rgb(0, 0, 0)");
    let s = parse_gap_rule("rule: 2px solid red, 3px dashed blue;");
    for axis in ["column", "row"] {
        assert_eq!(
            rule_css(&s, &format!("{axis}-rule")),
            "2px solid rgb(255, 0, 0), 3px dashed rgb(0, 0, 255)"
        );
    }
}

#[test]
fn rule_list_invalid_values_drop_the_declaration() {
    for bad in [
        "column-rule-width: 1px,",
        "column-rule-width: repeat(0, 1px)",
        "column-rule-width: repeat(auto, 1px), repeat(auto, 2px)",
        "column-rule-width: repeat(2, repeat(2, 1px))",
        "column-rule-style: solid, bogus",
        "column-rule-color: red, 3px",
        "column-rule: 1px solid, repeat(2)",
        "rule: 1px solid red, ",
    ] {
        let s = parse_gap_rule(&format!("column-rule: 7px dotted blue; {bad}"));
        assert_eq!(rule_css(&s, "column-rule"), "7px dotted rgb(0, 0, 255)", "{bad}");
    }
}

#[test]
fn rule_lists_css_wide_keywords_and_reset() {
    let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
    let sheet = lumen_css_parser::parse(
        "div { column-rule-width: 1px, 2px; row-rule: 5px solid red, 6px solid blue; }          span { column-rule-width: inherit; row-rule: 1px solid; row-rule-width: initial; }",
    );
    let root = ComputedStyle::root();
    let body = doc.body().expect("body");
    let div = doc.get(body).children.first().copied().expect("div");
    let span = doc.get(div).children.first().copied().expect("span");
    let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
    let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
    assert_eq!(rule_css(&span_style, "column-rule-width"), "1px, 2px");
    assert_eq!(rule_css(&span_style, "row-rule-width"), "3px");
    assert_eq!(rule_css(&span_style, "row-rule-style"), "solid");
}

#[test]
fn rule_computed_shorthands_merge_axes_or_stay_empty() {
    let s = parse_gap_rule("column-rule: 5px solid lime; row-rule: 5px solid lime;");
    assert_eq!(rule_css(&s, "rule"), "5px solid rgb(0, 255, 0)");
    assert_eq!(rule_css(&s, "rule-width"), "5px");
    assert_eq!(rule_css(&s, "rule-color"), "rgb(0, 255, 0)");
    // Оси расходятся — шортхенд обеих осей не собрать, шортхенд оси — собирается.
    let s = parse_gap_rule("column-rule: 5px solid lime; row-rule: 6px solid lime;");
    assert_eq!(rule_css(&s, "rule"), "");
    assert_eq!(rule_css(&s, "rule-width"), "");
    assert_eq!(rule_css(&s, "rule-style"), "solid");
    assert_eq!(rule_css(&s, "column-rule"), "5px solid rgb(0, 255, 0)");
    // Списки разной формы по трём свойствам — шортхенд пуст, лонгхенды целы.
    let s = parse_gap_rule("column-rule-width: 1px, 2px; column-rule-style: solid;");
    assert_eq!(rule_css(&s, "column-rule"), "");
    assert_eq!(rule_css(&s, "column-rule-width"), "1px, 2px");
}

#[test]
fn rule_computed_keywords_and_extended_line_styles() {
    let s = parse_gap_rule(
        "column-rule-style: groove, hidden, inset, outset, ridge; column-rule-break: none; row-rule-break: none;          column-rule-visibility-items: around; row-rule-visibility-items: around;",
    );
    assert_eq!(rule_css(&s, "column-rule-style"), "groove, hidden, inset, outset, ridge");
    assert_eq!(rule_css(&s, "rule-break"), "none");
    assert_eq!(rule_css(&s, "rule-visibility-items"), "around");
    let s = parse_gap_rule("column-rule-break: none; row-rule-break: intersection;");
    assert_eq!(rule_css(&s, "rule-break"), "");
    // `hidden` и `none` линии не рисуют, объёмные стили рисует `emit_gap_rules`.
    assert!(!BorderStyle::Hidden.is_visible());
    assert!(BorderStyle::Ridge.is_visible());
}

#[test]
fn rule_computed_widths_clamp_and_repeat_count_calc() {
    let s = parse_gap_rule(
        "font-size: 40px; column-rule-width: calc(10px - 0.5em); row-rule-width: repeat(calc(5 + 3), 10px);",
    );
    assert_eq!(rule_css(&s, "column-rule-width"), "0px");
    assert_eq!(rule_css(&s, "row-rule-width"), "repeat(8, 10px)");
    // Отрицательная простая длина по-прежнему невалидна.
    let s = parse_gap_rule("column-rule-width: -1px;");
    assert_eq!(rule_css(&s, "column-rule-width"), "3px");
}

#[test]
fn rule_computed_inset_resolves_font_relative_units() {
    let s = parse_gap_rule(
        "font-size: 40px; column-rule-inset: 0.5em 5% / calc(10px + 0.5em) overlap-join;          row-rule-inset-cap: calc(25% + 10px);",
    );
    assert_eq!(rule_css(&s, "column-rule-inset-cap-start"), "20px");
    assert_eq!(rule_css(&s, "column-rule-inset-cap-end"), "5%");
    assert_eq!(rule_css(&s, "column-rule-inset-junction-start"), "30px");
    assert_eq!(rule_css(&s, "column-rule-inset"), "20px 5% / 30px overlap-join");
    assert_eq!(rule_css(&s, "column-rule-inset-cap"), "20px 5%");
    // Слоты `start` расходятся — шортхенд `*-inset-start` не собрать.
    assert_eq!(rule_css(&s, "column-rule-inset-start"), "");
    assert_eq!(rule_css(&s, "row-rule-inset-cap"), "calc(25% + 10px)");
    assert_eq!(rule_css(&s, "rule-inset"), "");
    let s = parse_gap_rule("rule-inset: 10px 20px;");
    assert_eq!(rule_css(&s, "rule-inset"), "10px 20px / 10px 20px");
    assert_eq!(rule_css(&s, "rule-inset-cap"), "10px 20px");
}

#[test]
fn rule_computed_color_keeps_srgb_function_form() {
    // CSS Color L4 §4.2: `color-mix(in srgb, …)` и относительный `rgb(from …)` остаются в
    // форме `color(srgb …)`, остальные цвета — `rgb()`.
    let s = parse_gap_rule(
        "column-rule-color: color-mix(in srgb, red 50%, blue 50%);          row-rule-color: repeat(auto, rgb(from lime r g b)), color-mix(in srgb, lime 25%, yellow 75%);",
    );
    assert_eq!(rule_css(&s, "column-rule-color"), "color(srgb 0.5 0 0.5)");
    assert_eq!(
        rule_css(&s, "row-rule-color"),
        "repeat(auto, color(srgb 0 1 0)), color(srgb 0.75 1 0)"
    );
    let s = parse_gap_rule("rule-color: rgb(from yellow calc(255 - r) calc(255 - g) calc(255 - b));");
    assert_eq!(rule_css(&s, "rule-color"), "color(srgb 0 0 1)");
    // Шортхенд `rule` тоже хранит `color(srgb …)`.
    let s = parse_gap_rule("column-rule: 2px solid color-mix(in srgb, red 50%, blue 50%);");
    assert_eq!(rule_css(&s, "column-rule"), "2px solid color(srgb 0.5 0 0.5)");
    // Не-srgb пространство смешивания по-прежнему сводится к `rgb()`.
    let s = parse_gap_rule("column-rule-color: color-mix(in oklab, red, blue);");
    assert!(rule_css(&s, "column-rule-color").starts_with("rgb("));
}

    // ── CSS Multicol L2 §4.2 / §4.4 / §4.5: `column-height`, `column-wrap`, `columns: … / <h>` ──

    #[test]
    fn column_height_and_wrap_parse() {
        let s = parse_gap_rule("column-height: 60px; column-wrap: nowrap;");
        assert_eq!(s.column_height, Some(Length::Px(60.0)));
        assert!(s.column_wrap_nowrap);
        assert!(!s.column_rows_wrap(16.0, VP), "nowrap keeps overflow columns in the inline direction");
        let s = parse_gap_rule("column-height: 60px; column-wrap: wrap;");
        assert!(s.column_rows_wrap(16.0, VP));
        // auto: wraps only with a definite column-height.
        assert!(parse_gap_rule("column-wrap: auto;").column_height.is_none());
        assert!(!parse_gap_rule("column-wrap: wrap;").column_rows_wrap(16.0, VP));
        assert!(parse_gap_rule("column-height: 60px;").column_rows_wrap(16.0, VP));
    }

    #[test]
    fn column_height_rejects_negative_and_junk() {
        assert_eq!(parse_gap_rule("column-height: -5px;").column_height, None);
        assert_eq!(parse_gap_rule("column-height: 10px; column-height: bogus;").column_height, Some(Length::Px(10.0)));
        assert_eq!(parse_gap_rule("column-height: 10px; column-height: auto;").column_height, None);
        assert!(!parse_gap_rule("column-wrap: sideways;").column_wrap_nowrap);
    }

    #[test]
    fn columns_shorthand_takes_a_column_height() {
        let s = parse_gap_rule("columns: 3 60px / 80px;");
        assert_eq!(s.column_count, Some(3));
        assert_eq!(s.column_width, Some(Length::Px(60.0)));
        assert_eq!(s.column_height, Some(Length::Px(80.0)));
        // Without `/ <height>` the shorthand resets column-height.
        let s = parse_gap_rule("column-height: 80px; columns: 2;");
        assert_eq!(s.column_height, None);
        // An invalid height drops the whole declaration.
        let s = parse_gap_rule("column-height: 80px; columns: 2 / -1px;");
        assert_eq!(s.column_height, Some(Length::Px(80.0)));
        assert_eq!(s.column_count, None);
    }
