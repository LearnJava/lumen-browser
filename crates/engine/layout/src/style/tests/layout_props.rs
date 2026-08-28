//! Тесты `style.rs`: свойства раскладки: flexbox, `display`, `contain`, `overflow`,
//! `appearance` и соседние перечисления.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ── flex-direction ────────────────────────────────────────────────────

    #[test]
    fn flex_direction_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_direction, FlexDirection::Row);
    }

    #[test]
    fn flex_direction_values() {
        let cases = [
            ("row", FlexDirection::Row),
            ("row-reverse", FlexDirection::RowReverse),
            ("column", FlexDirection::Column),
            ("column-reverse", FlexDirection::ColumnReverse),
        ];
        for (css_val, expected) in cases {
            let doc = lumen_html_parser::parse("<div></div>");
            let sheet =
                lumen_css_parser::parse(&format!("div {{ flex-direction: {css_val}; }}"));
            let root = ComputedStyle::root();
            let node = doc.get(doc.body().unwrap()).children[0];
            let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
            assert_eq!(s.flex_direction, expected, "flex-direction: {css_val}");
        }
    }

    #[test]
    fn flex_direction_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { flex-direction: column; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.flex_direction, FlexDirection::Column);
        assert_eq!(span_style.flex_direction, FlexDirection::Row); // initial, не наследуется
    }

    #[test]
    fn flex_direction_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-direction: diagonal; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_direction, FlexDirection::Row);
    }

    // ── flex-wrap ─────────────────────────────────────────────────────────

    #[test]
    fn flex_wrap_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_wrap, FlexWrap::Nowrap);
    }

    #[test]
    fn flex_wrap_values() {
        let cases = [
            ("nowrap", FlexWrap::Nowrap),
            ("wrap", FlexWrap::Wrap),
            ("wrap-reverse", FlexWrap::WrapReverse),
        ];
        for (css_val, expected) in cases {
            let doc = lumen_html_parser::parse("<div></div>");
            let sheet = lumen_css_parser::parse(&format!("div {{ flex-wrap: {css_val}; }}"));
            let root = ComputedStyle::root();
            let node = doc.get(doc.body().unwrap()).children[0];
            let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
            assert_eq!(s.flex_wrap, expected, "flex-wrap: {css_val}");
        }
    }

    #[test]
    fn flex_wrap_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { flex-wrap: wrap; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.flex_wrap, FlexWrap::Wrap);
        assert_eq!(span_style.flex_wrap, FlexWrap::Nowrap); // initial, не наследуется
    }

    #[test]
    fn flex_wrap_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-wrap: yes; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_wrap, FlexWrap::Nowrap);
    }

    // ── flex-flow shorthand ───────────────────────────────────────────────

    #[test]
    fn flex_flow_shorthand() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-flow: column wrap; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_direction, FlexDirection::Column);
        assert_eq!(s.flex_wrap, FlexWrap::Wrap);
    }

    #[test]
    fn flex_flow_shorthand_direction_only() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-flow: row-reverse; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_direction, FlexDirection::RowReverse);
        assert_eq!(s.flex_wrap, FlexWrap::Nowrap); // reset to initial
    }

    #[test]
    fn flex_flow_shorthand_wrap_only() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-flow: wrap-reverse; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_direction, FlexDirection::Row); // reset to initial
        assert_eq!(s.flex_wrap, FlexWrap::WrapReverse);
    }

    // ── flex-grow ─────────────────────────────────────────────────────────

    #[test]
    fn flex_grow_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_grow, 0.0);
    }

    #[test]
    fn flex_grow_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-grow: 2.5; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_grow, 2.5);
    }

    #[test]
    fn flex_grow_negative_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-grow: -1; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_grow, 0.0); // initial, negative rejected
    }

    #[test]
    fn flex_grow_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { flex-grow: 3; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_s = compute_style(&doc, span, &sheet, &div_s, Size::new(800.0, 600.0), false);
        assert_eq!(div_s.flex_grow, 3.0);
        assert_eq!(span_s.flex_grow, 0.0); // initial, не наследуется
    }

    // ── flex-shrink ───────────────────────────────────────────────────────

    #[test]
    fn flex_shrink_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_shrink, 1.0);
    }

    #[test]
    fn flex_shrink_zero() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-shrink: 0; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_shrink, 0.0);
    }

    #[test]
    fn flex_shrink_negative_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-shrink: -0.5; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_shrink, 1.0); // initial
    }

    #[test]
    fn flex_shrink_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { flex-shrink: 4; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_s = compute_style(&doc, span, &sheet, &div_s, Size::new(800.0, 600.0), false);
        assert_eq!(div_s.flex_shrink, 4.0);
        assert_eq!(span_s.flex_shrink, 1.0); // initial
    }

    // ── flex-basis ────────────────────────────────────────────────────────

    #[test]
    fn flex_basis_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_basis, FlexBasis::Auto);
    }

    #[test]
    fn flex_basis_content() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-basis: content; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_basis, FlexBasis::Content);
    }

    #[test]
    fn flex_basis_px() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex-basis: 120px; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_basis, FlexBasis::Length(Length::Px(120.0)));
    }

    #[test]
    fn flex_basis_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { flex-basis: 50px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_s = compute_style(&doc, span, &sheet, &div_s, Size::new(800.0, 600.0), false);
        assert_eq!(div_s.flex_basis, FlexBasis::Length(Length::Px(50.0)));
        assert_eq!(span_s.flex_basis, FlexBasis::Auto); // initial
    }

    // ── flex shorthand ────────────────────────────────────────────────────

    #[test]
    fn flex_shorthand_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex: none; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_grow, 0.0);
        assert_eq!(s.flex_shrink, 0.0);
        assert_eq!(s.flex_basis, FlexBasis::Auto);
    }

    #[test]
    fn flex_shorthand_auto() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex: auto; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_grow, 1.0);
        assert_eq!(s.flex_shrink, 1.0);
        assert_eq!(s.flex_basis, FlexBasis::Auto);
    }

    #[test]
    fn flex_shorthand_single_number() {
        // flex: N → grow=N, shrink=1, basis=0
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex: 3; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_grow, 3.0);
        assert_eq!(s.flex_shrink, 1.0);
        assert_eq!(s.flex_basis, FlexBasis::Length(Length::Px(0.0)));
    }

    #[test]
    fn flex_shorthand_three_values() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { flex: 2 1 100px; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.flex_grow, 2.0);
        assert_eq!(s.flex_shrink, 1.0);
        assert_eq!(s.flex_basis, FlexBasis::Length(Length::Px(100.0)));
    }

    // ── order ─────────────────────────────────────────────────────────────────

    #[test]
    fn order_initial_zero() {
        let style = ComputedStyle::root();
        assert_eq!(style.order, 0);
    }

    #[test]
    fn order_positive() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { order: 3; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.order, 3);
    }

    #[test]
    fn order_negative() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { order: -1; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.order, -1);
    }

    #[test]
    fn order_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { order: 5; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.order, 5);
        assert_eq!(span_style.order, 0);
    }

    #[test]
    fn order_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { order: auto; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.order, 0);
    }

    // ── resize ────────────────────────────────────────────────────────────────

    #[test]
    fn resize_initial_none() {
        let style = ComputedStyle::root();
        assert_eq!(style.resize, Resize::None);
    }

    #[test]
    fn resize_both() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { resize: both; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.resize, Resize::Both);
    }

    #[test]
    fn resize_horizontal_vertical() {
        let doc = lumen_html_parser::parse("<span></span><em></em>");
        let hs = lumen_css_parser::parse("span { resize: horizontal; }");
        let vs = lumen_css_parser::parse("em { resize: vertical; }");
        let root = ComputedStyle::root();
        let span = doc.get(doc.body().unwrap()).children[0];
        let em = doc.get(doc.body().unwrap()).children[1];
        let h = compute_style(&doc, span, &hs, &root, Size::new(800.0, 600.0), false);
        let v = compute_style(&doc, em, &vs, &root, Size::new(800.0, 600.0), false);
        assert_eq!(h.resize, Resize::Horizontal);
        assert_eq!(v.resize, Resize::Vertical);
    }

    #[test]
    fn resize_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { resize: both; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.resize, Resize::Both);
        assert_eq!(span_style.resize, Resize::None);
    }

    #[test]
    fn resize_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { resize: all; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.resize, Resize::None);
    }

    #[test]
    fn resize_allowed_axes_physical() {
        assert_eq!(Resize::None.allowed_axes(WritingMode::HorizontalTb), (false, false));
        assert_eq!(Resize::Both.allowed_axes(WritingMode::HorizontalTb), (true, true));
        assert_eq!(Resize::Horizontal.allowed_axes(WritingMode::HorizontalTb), (true, false));
        assert_eq!(Resize::Vertical.allowed_axes(WritingMode::HorizontalTb), (false, true));
        // Physical axes ignore writing-mode.
        assert_eq!(Resize::Horizontal.allowed_axes(WritingMode::VerticalRl), (true, false));
        assert_eq!(Resize::Vertical.allowed_axes(WritingMode::VerticalRl), (false, true));
    }

    #[test]
    fn resize_allowed_axes_logical() {
        // horizontal-tb: block-axis = vertical, inline-axis = horizontal.
        assert_eq!(Resize::Block.allowed_axes(WritingMode::HorizontalTb), (false, true));
        assert_eq!(Resize::Inline.allowed_axes(WritingMode::HorizontalTb), (true, false));
        // vertical-rl/lr and sideways-rl/lr: block-axis = horizontal, inline-axis = vertical.
        for wm in [
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            assert_eq!(Resize::Block.allowed_axes(wm), (true, false));
            assert_eq!(Resize::Inline.allowed_axes(wm), (false, true));
        }
    }

    // --- touch-action ---

    #[test]
    fn touch_action_basic() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { touch-action: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.touch_action, TouchAction::None);
    }

    #[test]
    fn touch_action_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.touch_action, TouchAction::Auto);
    }

    #[test]
    fn touch_action_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { touch-action: manipulation; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.touch_action, TouchAction::Manipulation);
        assert_eq!(span_style.touch_action, TouchAction::Auto);
    }

    #[test]
    fn touch_action_pan_values() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { touch-action: pan-y; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.touch_action, TouchAction::PanY);
    }

    // --- appearance ---

    #[test]
    fn appearance_basic() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { appearance: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.appearance, Appearance::None);
    }

    #[test]
    fn appearance_base_select() {
        let doc = lumen_html_parser::parse("<select></select>");
        let sheet = lumen_css_parser::parse("select { appearance: base-select; }");
        let root = ComputedStyle::root();
        let sel = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, sel, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.appearance, Appearance::BaseSelect);
    }

    #[test]
    fn appearance_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.appearance, Appearance::Auto);
    }

    #[test]
    fn appearance_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { appearance: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.appearance, Appearance::None);
        assert_eq!(span_style.appearance, Appearance::Auto);
    }

    #[test]
    fn appearance_webkit_prefix() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { -webkit-appearance: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.appearance, Appearance::None);
    }

    #[test]
    fn appearance_none_removes_input_ua_styling() {
        let doc = lumen_html_parser::parse("<input />");
        let sheet = lumen_css_parser::parse("input { appearance: none; }");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        // appearance: none should remove borders and padding
        assert_eq!(style.appearance, Appearance::None);
        assert_eq!(style.border_top_width, 0.0);
        assert_eq!(style.border_right_width, 0.0);
        assert_eq!(style.border_bottom_width, 0.0);
        assert_eq!(style.border_left_width, 0.0);
        assert_eq!(style.padding_top, Length::Px(0.0));
        assert_eq!(style.padding_right, Length::Px(0.0));
        assert_eq!(style.padding_bottom, Length::Px(0.0));
        assert_eq!(style.padding_left, Length::Px(0.0));
        // Check background is transparent (alpha = 0)
        match style.background_color {
            Some(CssColor::Rgba(Color { a, .. })) => assert_eq!(a, 0),
            _ => panic!("Expected rgba color"),
        }
    }

    #[test]
    fn appearance_none_removes_button_ua_styling() {
        let doc = lumen_html_parser::parse("<button></button>");
        let sheet = lumen_css_parser::parse("button { appearance: none; }");
        let root = ComputedStyle::root();
        let button = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, button, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.border_top_width, 0.0);
        assert_eq!(style.padding_top, Length::Px(0.0));
        match style.background_color {
            Some(CssColor::Rgba(Color { a, .. })) => assert_eq!(a, 0),
            _ => panic!("Expected rgba color"),
        }
    }

    #[test]
    fn appearance_none_removes_select_ua_styling() {
        let doc = lumen_html_parser::parse("<select></select>");
        let sheet = lumen_css_parser::parse("select { appearance: none; }");
        let root = ComputedStyle::root();
        let select = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, select, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.border_top_width, 0.0);
        assert_eq!(style.padding_top, Length::Px(0.0));
    }

    #[test]
    fn appearance_none_preserves_author_border_and_background() {
        // BUG-211: with `appearance: none`, UA-default border/background must be
        // stripped *before* the author cascade so author-specified values win.
        // Previously the strip ran after the cascade and clobbered them, leaving
        // content-sized fields with width-0 borders and a transparent background.
        let doc = lumen_html_parser::parse("<input value=\"ab\" />");
        let sheet = lumen_css_parser::parse(
            "input { appearance: none; border: 2px solid #003366; background: #b3d9ff; }",
        );
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.appearance, Appearance::None);
        // Author border width survives (was clobbered to 0.0 before the fix).
        assert_eq!(style.border_top_width, 2.0);
        assert_eq!(style.border_right_width, 2.0);
        assert_eq!(style.border_bottom_width, 2.0);
        assert_eq!(style.border_left_width, 2.0);
        // Author background survives (was clobbered to transparent before the fix).
        match style.background_color {
            Some(CssColor::Rgba(Color { r, g, b, a })) => {
                assert_eq!((r, g, b, a), (0xb3, 0xd9, 0xff, 0xff));
            }
            other => panic!("expected author rgba background, got {other:?}"),
        }
    }

    #[test]
    fn appearance_auto_preserves_ua_styling() {
        let doc = lumen_html_parser::parse("<input />");
        let sheet = lumen_css_parser::parse(""); // appearance: auto (default)
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        // appearance: auto should preserve UA styling (border from apply_ua_form_controls)
        assert_eq!(style.border_top_width, 1.0);
        assert_eq!(style.border_right_width, 1.0);
        assert_eq!(style.appearance, Appearance::Auto);
    }

    #[test]
    fn appearance_none_removes_textarea_ua_styling() {
        let doc = lumen_html_parser::parse("<textarea></textarea>");
        let sheet = lumen_css_parser::parse("textarea { appearance: none; }");
        let root = ComputedStyle::root();
        let textarea = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, textarea, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.border_top_width, 0.0);
        assert_eq!(style.padding_top, Length::Px(0.0));
        match style.background_color {
            Some(CssColor::Rgba(Color { a, .. })) => assert_eq!(a, 0),
            _ => panic!("Expected rgba color"),
        }
    }

    // --- field-sizing ---

    #[test]
    fn field_sizing_default_is_fixed() {
        let doc = lumen_html_parser::parse("<input />");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.field_sizing, FieldSizing::Fixed);
    }

    #[test]
    fn field_sizing_content_parses() {
        let doc = lumen_html_parser::parse("<input />");
        let sheet = lumen_css_parser::parse("input { field-sizing: content; }");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.field_sizing, FieldSizing::Content);
    }

    #[test]
    fn field_sizing_content_suppresses_ua_width() {
        // With field-sizing: content, apply_ua_form_controls should NOT set width/height.
        let doc = lumen_html_parser::parse("<input />");
        let sheet = lumen_css_parser::parse("input { field-sizing: content; }");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.field_sizing, FieldSizing::Content);
        assert!(style.width.is_none(), "field-sizing: content must clear UA width");
        assert!(style.height.is_none(), "field-sizing: content must clear UA height");
    }

    #[test]
    fn field_sizing_fixed_preserves_ua_dimensions() {
        // Default (fixed): UA dimensions are preserved.
        let doc = lumen_html_parser::parse("<input />");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.width, Some(Length::Px(174.0)));
        assert_eq!(style.height, Some(Length::Px(21.0)));
    }

    #[test]
    fn field_sizing_not_inherited() {
        let doc = lumen_html_parser::parse("<div><input /></div>");
        let sheet = lumen_css_parser::parse("div { field-sizing: content; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let input = doc.get(div).children[0];
        let input_style = compute_style(&doc, input, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.field_sizing, FieldSizing::Content);
        // field-sizing is not inherited → input keeps Fixed
        assert_eq!(input_style.field_sizing, FieldSizing::Fixed);
    }

    // --- contain-intrinsic-size (CSS Box Sizing L4 §5) ---

    #[test]
    fn contain_intrinsic_size_default_is_none() {
        let s = ComputedStyle::root();
        assert!(s.contain_intrinsic_width.is_none());
        assert!(s.contain_intrinsic_height.is_none());
    }

    #[test]
    fn contain_intrinsic_size_shorthand_two_values() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { contain-intrinsic-size: 200px 100px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.contain_intrinsic_width, Some(Length::Px(200.0)));
        assert_eq!(s.contain_intrinsic_height, Some(Length::Px(100.0)));
    }

    #[test]
    fn contain_intrinsic_size_shorthand_one_value_both_axes() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { contain-intrinsic-size: 50px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.contain_intrinsic_width, Some(Length::Px(50.0)));
        assert_eq!(s.contain_intrinsic_height, Some(Length::Px(50.0)));
    }

    #[test]
    fn contain_intrinsic_size_auto_keyword_uses_length() {
        // `auto <length>` — the `auto` last-remembered hint is accepted and the
        // length is used as the placeholder.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { contain-intrinsic-size: auto 300px auto 150px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.contain_intrinsic_width, Some(Length::Px(300.0)));
        assert_eq!(s.contain_intrinsic_height, Some(Length::Px(150.0)));
    }

    #[test]
    fn contain_intrinsic_size_none_is_no_placeholder() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { contain-intrinsic-size: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(s.contain_intrinsic_width.is_none());
        assert!(s.contain_intrinsic_height.is_none());
    }

    #[test]
    fn contain_intrinsic_height_longhand_and_logical_alias() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "div { contain-intrinsic-height: 80px; contain-intrinsic-inline-size: 40px; }",
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.contain_intrinsic_height, Some(Length::Px(80.0)));
        // inline-size maps to width under horizontal-tb.
        assert_eq!(s.contain_intrinsic_width, Some(Length::Px(40.0)));
    }

    #[test]
    fn contain_intrinsic_size_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { contain-intrinsic-size: 200px 100px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.contain_intrinsic_height, Some(Length::Px(100.0)));
        assert!(span_style.contain_intrinsic_width.is_none());
        assert!(span_style.contain_intrinsic_height.is_none());
    }

    // --- Display extended values ---

    #[test]
    fn display_flow_root() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { display: flow-root; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::FlowRoot);
    }

    #[test]
    fn display_contents() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { display: contents; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::Contents);
    }

    #[test]
    fn display_table_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { display: table; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::Table);
    }

    #[test]
    fn display_table_cell_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { display: table-cell; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::TableCell);
    }

    #[test]
    fn display_list_item_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { display: list-item; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::ListItem);
    }

    #[test]
    fn display_ua_table_element() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let table = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, table, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::Table);
    }

    #[test]
    fn display_ua_li_element() {
        let doc = lumen_html_parser::parse("<li></li>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let li = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, li, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::ListItem);
    }

    #[test]
    fn display_invalid_keeps_current() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { display: bogus-value; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::Block);
    }

    // --- contain ---

    #[test]
    fn contain_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { contain: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.contain, ContainFlags::NONE);
    }

    #[test]
    fn contain_strict() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { contain: strict; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.contain, ContainFlags::STRICT);
    }

    #[test]
    fn contain_keywords_combined() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { contain: layout paint; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let expected = ContainFlags(ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0);
        assert_eq!(style.contain, expected);
    }

    #[test]
    fn contain_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { contain: content; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.contain, ContainFlags::CONTENT);
        assert_eq!(span_style.contain, ContainFlags::NONE);
    }

    // --- content-visibility ---

    #[test]
    fn content_visibility_hidden() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { content-visibility: hidden; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.content_visibility, ContentVisibility::Hidden);
    }

    #[test]
    fn content_visibility_auto() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { content-visibility: auto; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.content_visibility, ContentVisibility::Auto);
    }

    #[test]
    fn content_visibility_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.content_visibility, ContentVisibility::Visible);
    }

    #[test]
    fn content_visibility_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { content-visibility: hidden; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.content_visibility, ContentVisibility::Hidden);
        assert_eq!(span_style.content_visibility, ContentVisibility::Visible);
    }

    // --- interpolate-size (CSS Sizing L4 §4.5) ---

    #[test]
    fn interpolate_size_allow_keywords() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { interpolate-size: allow-keywords; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.interpolate_size, InterpolateSizeMode::AllowKeywords);
    }

    #[test]
    fn interpolate_size_numeric_only() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { interpolate-size: numeric-only; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.interpolate_size, InterpolateSizeMode::NumericOnly);
    }

    #[test]
    fn interpolate_size_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.interpolate_size, InterpolateSizeMode::NumericOnly);
    }

    #[test]
    fn interpolate_size_inherited() {
        // CSS Sizing L4 §4.5 — interpolate-size IS inherited.
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { interpolate-size: allow-keywords; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style =
            compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.interpolate_size, InterpolateSizeMode::AllowKeywords);
        assert_eq!(span_style.interpolate_size, InterpolateSizeMode::AllowKeywords);
    }

    #[test]
    fn interpolate_size_unset_inherits() {
        // `unset` on an inherited property = inherit.
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse(
            "div { interpolate-size: allow-keywords; } span { interpolate-size: unset; }",
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style =
            compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(span_style.interpolate_size, InterpolateSizeMode::AllowKeywords);
    }

    // --- container-type ---

    #[test]
    fn container_type_inline_size() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { container-type: inline-size; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.container_type, ContainerType::InlineSize);
    }

    #[test]
    fn container_type_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.container_type, ContainerType::Normal);
    }

    #[test]
    fn container_name_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { container-name: sidebar; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.container_name, vec!["sidebar"]);
    }

    #[test]
    fn container_shorthand() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { container: sidebar / inline-size; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.container_name, vec!["sidebar"]);
        assert_eq!(style.container_type, ContainerType::InlineSize);
    }

    // ── CSS Overflow L3 §2 — apply_declaration parsing ──

    #[test]
    fn overflow_scroll_single_value_sets_both_axes() {
        let s = ts_prop("overflow", "scroll");
        assert_eq!(s.overflow_x, Overflow::Scroll);
        assert_eq!(s.overflow_y, Overflow::Scroll);
    }

    #[test]
    fn overflow_auto_single_value_sets_both_axes() {
        let s = ts_prop("overflow", "auto");
        assert_eq!(s.overflow_x, Overflow::Auto);
        assert_eq!(s.overflow_y, Overflow::Auto);
    }

    #[test]
    fn overflow_two_value_scroll_auto() {
        // CSS Overflow L3: two-value form — first token = x, second = y.
        let s = ts_prop("overflow", "scroll auto");
        assert_eq!(s.overflow_x, Overflow::Scroll);
        assert_eq!(s.overflow_y, Overflow::Auto);
    }

    #[test]
    fn overflow_x_scroll_only_sets_x_axis() {
        let s = ts_prop("overflow-x", "scroll");
        assert_eq!(s.overflow_x, Overflow::Scroll);
        assert_eq!(s.overflow_y, Overflow::Visible);
    }

    #[test]
    fn overflow_y_auto_only_sets_y_axis() {
        let s = ts_prop("overflow-y", "auto");
        assert_eq!(s.overflow_x, Overflow::Visible);
        assert_eq!(s.overflow_y, Overflow::Auto);
    }
