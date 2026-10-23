//! Тесты `style.rs`: CSS Anchor Positioning L1.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

// ─────────────────────────────────────────────────────────────────────────────
// CSS Anchor Positioning L1 tests
// ─────────────────────────────────────────────────────────────────────────────

    use super::*;
    use crate::anchor::InsetAreaKeyword;
    use lumen_core::geom::Size;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn first_div_style(html: &str, css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let child = doc.get(body).children[0];
        compute_style(&doc, child, &sheet, &root, VP, false)
    }

    #[test]
    fn anchor_name_parsed() {
        let s = first_div_style("<div></div>", "div { anchor-name: --btn; }");
        assert_eq!(s.anchor_name.as_deref(), Some("--btn"));
    }

    #[test]
    fn anchor_name_none_clears() {
        let s = first_div_style("<div></div>", "div { anchor-name: none; }");
        assert!(s.anchor_name.is_none());
    }

    #[test]
    fn position_anchor_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; position-anchor: --tooltip-anchor; }",
        );
        assert_eq!(s.position_anchor.as_deref(), Some("--tooltip-anchor"));
    }

    #[test]
    fn inset_area_two_keywords() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; inset-area: end start; }",
        );
        assert_eq!(s.inset_area_row, InsetAreaKeyword::End);
        assert_eq!(s.inset_area_col, InsetAreaKeyword::Start);
    }

    #[test]
    fn inset_area_single_keyword_sets_both_axes() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; inset-area: center; }",
        );
        assert_eq!(s.inset_area_row, InsetAreaKeyword::Center);
        assert_eq!(s.inset_area_col, InsetAreaKeyword::Center);
    }

    #[test]
    fn position_area_alias_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; position-area: start end; }",
        );
        assert_eq!(s.inset_area_row, InsetAreaKeyword::Start);
        assert_eq!(s.inset_area_col, InsetAreaKeyword::End);
    }

    /// `position-area: <value>` → `(row, column)` bands, `None` when the
    /// declaration is rejected (the property then keeps its initial `none`).
    fn area_of(value: &str) -> Option<(InsetAreaKeyword, InsetAreaKeyword)> {
        let css = format!("div {{ position: absolute; position-area: {value}; }}");
        let s = first_div_style("<div></div>", &css);
        match (s.inset_area_row, s.inset_area_col) {
            (InsetAreaKeyword::None, InsetAreaKeyword::None) => None,
            pair => Some(pair),
        }
    }

    #[test]
    fn position_area_single_axis_word_spans_other_axis() {
        use InsetAreaKeyword::*;
        // BUG-1474: `top` is `top span-all`, not the top-left corner.
        assert_eq!(area_of("top"), Some((Start, SpanAll)));
        assert_eq!(area_of("bottom"), Some((End, SpanAll)));
        assert_eq!(area_of("left"), Some((SpanAll, Start)));
        assert_eq!(area_of("right"), Some((SpanAll, End)));
        assert_eq!(area_of("block-end"), Some((End, SpanAll)));
        assert_eq!(area_of("inline-start"), Some((SpanAll, Start)));
        assert_eq!(area_of("self-y-end"), Some((End, SpanAll)));
        // Ambiguous single words name both axes.
        assert_eq!(area_of("start"), Some((Start, Start)));
        assert_eq!(area_of("span-end"), Some((SpanEnd, SpanEnd)));
        assert_eq!(area_of("center"), Some((Center, Center)));
        assert_eq!(area_of("span-all"), Some((SpanAll, SpanAll)));
    }

    #[test]
    fn position_area_pairs_combine_in_any_order() {
        use InsetAreaKeyword::*;
        assert_eq!(area_of("span-left bottom"), Some((End, SpanStart)));
        assert_eq!(area_of("bottom span-left"), Some((End, SpanStart)));
        assert_eq!(area_of("span-bottom span-right"), Some((SpanEnd, SpanEnd)));
        assert_eq!(area_of("self-x-start self-y-start"), Some((Start, Start)));
        assert_eq!(area_of("span-self-x-end y-start"), Some((Start, SpanEnd)));
        assert_eq!(area_of("top center"), Some((Start, Center)));
        assert_eq!(area_of("center top"), Some((Start, Center)));
        assert_eq!(area_of("span-all left"), Some((SpanAll, Start)));
        assert_eq!(area_of("inline-end block-start"), Some((Start, End)));
        // `start`/`end` pairs are block then inline.
        assert_eq!(area_of("end start"), Some((End, Start)));
        assert_eq!(area_of("self-start span-self-end"), Some((Start, SpanEnd)));
    }

    #[test]
    fn position_area_rejects_invalid_combinations() {
        for bad in [
            "left inline-start", "top top", "left left", "top block-start",
            "left right", "block-start block-end", "start inline-start",
            "top start", "self-start start", "foobar", "start foobar", "top left right",
        ] {
            assert_eq!(area_of(bad), None, "{bad} must be rejected");
        }
    }

    // ── CSS Anchor Positioning L1 §3.1 — `anchor()` function ─────────────────

    #[test]
    fn anchor_func_side_only_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; position-anchor: --btn; top: anchor(bottom); }",
        );
        let f = s.anchor_top.as_ref().expect("anchor() not parsed");
        assert!(f.anchor_name.is_none());
        assert_eq!(f.side, crate::anchor::AnchorSide::Bottom);
        assert!(f.fallback.is_none());
        assert!(s.top.is_auto(), "plain top must stay auto when anchor() is used");
    }

    #[test]
    fn anchor_func_explicit_name_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; left: anchor(--btn right); }",
        );
        let f = s.anchor_left.as_ref().expect("anchor() not parsed");
        assert_eq!(f.anchor_name.as_deref(), Some("--btn"));
        assert_eq!(f.side, crate::anchor::AnchorSide::Right);
    }

    #[test]
    fn anchor_func_percentage_side_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; left: anchor(25%); }",
        );
        let f = s.anchor_left.as_ref().expect("anchor() not parsed");
        assert_eq!(f.side, crate::anchor::AnchorSide::Percentage(25.0));
    }

    #[test]
    fn anchor_func_fallback_parsed() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; bottom: anchor(top, 10px); }",
        );
        let f = s.anchor_bottom.as_ref().expect("anchor() not parsed");
        assert_eq!(f.side, crate::anchor::AnchorSide::Top);
        assert_eq!(f.fallback, Some(Length::Px(10.0)));
    }

    // ── GAP-ANCHORCSSOM-S1: `&&` grammar, new sides, `anchor-size()` ──────────

    #[test]
    fn anchor_func_side_before_name_equals_name_before_side() {
        let a = first_div_style("<div></div>", "div { position: absolute; top: anchor(--a top); }");
        let b = first_div_style("<div></div>", "div { position: absolute; top: anchor(top --a); }");
        assert_eq!(a.anchor_top, b.anchor_top);
        let f = b.anchor_top.as_ref().expect("anchor(top --a) not parsed");
        assert_eq!(f.anchor_name.as_deref(), Some("--a"));
        assert_eq!(f.side, crate::anchor::AnchorSide::Top);
    }

    #[test]
    fn anchor_func_side_before_name_keeps_fallback() {
        let s = first_div_style("<div></div>", "div { position: absolute; left: anchor(50% --a, 4px); }");
        let f = s.anchor_left.as_ref().expect("anchor() not parsed");
        assert_eq!(f.anchor_name.as_deref(), Some("--a"));
        assert_eq!(f.side, crate::anchor::AnchorSide::Percentage(50.0));
        assert_eq!(f.fallback, Some(Length::Px(4.0)));
    }

    #[test]
    fn anchor_func_new_sides_parsed() {
        use crate::anchor::AnchorSide;
        for (kw, want) in [
            ("inside", AnchorSide::Inside),
            ("outside", AnchorSide::Outside),
            ("self-start", AnchorSide::SelfStart),
            ("self-end", AnchorSide::SelfEnd),
            ("OUTSIDE", AnchorSide::Outside),
        ] {
            let s = first_div_style(
                "<div></div>",
                &format!("div {{ position: absolute; top: anchor({kw}); }}"),
            );
            let f = s.anchor_top.as_ref().unwrap_or_else(|| panic!("anchor({kw}) not parsed"));
            assert_eq!(f.side, want, "{kw}");
        }
    }

    #[test]
    fn anchor_func_repeated_or_missing_component_is_invalid() {
        for arg in ["--a --b top", "top left", "top bottom --a", "--a", "--a --b", "bogus", "top top"] {
            let s = first_div_style(
                "<div></div>",
                &format!("div {{ position: absolute; top: anchor({arg}); }}"),
            );
            assert!(s.anchor_top.is_none(), "anchor({arg}) must not parse");
            assert!(s.top.is_auto(), "anchor({arg}) must leave top auto");
        }
    }

    #[test]
    fn anchor_size_name_then_dimension_parsed() {
        use crate::anchor::AnchorSizeDimension;
        let s = first_div_style("<div></div>", "div { position: absolute; width: anchor-size(--a width); }");
        let f = s.anchor_size_w.as_ref().expect("anchor-size() not parsed");
        assert_eq!(f.anchor_name.as_deref(), Some("--a"));
        assert_eq!(f.dimension, AnchorSizeDimension::Width);
        assert!(f.fallback.is_none());
        assert!(s.width.is_none(), "plain width must stay auto when anchor-size() is used");
    }

    #[test]
    fn anchor_size_dimension_then_name_parsed() {
        use crate::anchor::AnchorSizeDimension;
        let s = first_div_style("<div></div>", "div { position: absolute; height: anchor-size(block --a); }");
        let f = s.anchor_size_h.as_ref().expect("anchor-size() not parsed");
        assert_eq!(f.anchor_name.as_deref(), Some("--a"));
        assert_eq!(f.dimension, AnchorSizeDimension::Block);
    }

    #[test]
    fn anchor_size_dimension_only_parsed() {
        use crate::anchor::AnchorSizeDimension;
        let s = first_div_style("<div></div>", "div { position: absolute; width: anchor-size(inline); }");
        let f = s.anchor_size_w.as_ref().expect("anchor-size() not parsed");
        assert!(f.anchor_name.is_none());
        assert_eq!(f.dimension, AnchorSizeDimension::Inline);
    }

    #[test]
    fn anchor_size_fallback_parsed() {
        let s = first_div_style("<div></div>", "div { position: absolute; width: anchor-size(--a width, 10px); }");
        let f = s.anchor_size_w.as_ref().expect("anchor-size() not parsed");
        assert_eq!(f.anchor_name.as_deref(), Some("--a"));
        assert_eq!(f.fallback, Some(Length::Px(10.0)));
    }

    #[test]
    fn anchor_size_legacy_comma_form_is_invalid() {
        // `anchor-size(--a, width)`: after the comma the spec wants a
        // `<length-percentage>` fallback, and `width` is not one.
        let s = first_div_style("<div></div>", "div { position: absolute; width: anchor-size(--a, width); }");
        assert!(s.anchor_size_w.is_none());
    }

    #[test]
    fn anchor_size_repeated_component_is_invalid() {
        for arg in ["--a --b width", "width height", "--a", ""] {
            let s = first_div_style(
                "<div></div>",
                &format!("div {{ position: absolute; width: anchor-size({arg}); }}"),
            );
            assert!(s.anchor_size_w.is_none(), "anchor-size({arg}) must not parse");
        }
    }

    #[test]
    fn anchor_func_cleared_by_plain_length() {
        // A later declaration without anchor() must clear the previously
        // parsed AnchorFunc (mirrors width/anchor-size() interception).
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; top: anchor(bottom); top: 5px; }",
        );
        assert!(s.anchor_top.is_none());
        assert_eq!(s.top, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn anchor_func_inset_shorthand_applies_to_all_sides() {
        let s = first_div_style(
            "<div></div>",
            "div { position: absolute; inset: anchor(top); }",
        );
        assert!(s.anchor_top.is_some());
        assert!(s.anchor_right.is_some());
        assert!(s.anchor_bottom.is_some());
        assert!(s.anchor_left.is_some());
    }

    #[test]
    fn anchor_name_not_inherited() {
        let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
        let sheet = lumen_css_parser::parse("div { anchor-name: --parent; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
        assert_eq!(div_style.anchor_name.as_deref(), Some("--parent"));
        assert!(span_style.anchor_name.is_none(), "anchor-name must not be inherited");
    }

    // ── CSS View Transitions L1 ───────────────────────────────────────────────

    #[test]
    fn view_transition_name_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-transition-name: hero; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.view_transition_name.as_deref(), Some("hero"));
    }

    #[test]
    fn view_transition_name_none_clears() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-transition-name: none; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert!(s.view_transition_name.is_none());
    }

    fn vt_style(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(css);
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        compute_style(&doc, div, &sheet, &root, VP, false)
    }

    #[test]
    fn view_transition_class_parsed_and_validated() {
        assert_eq!(vt_style("div { view-transition-class: a  b }").view_transition_class.as_deref(), Some("a b"));
        assert!(vt_style("div { view-transition-class: none }").view_transition_class.is_none());
        // `none` inside a list and CSS-wide keywords are invalid → declaration dropped.
        assert!(vt_style("div { view-transition-class: a none }").view_transition_class.is_none());
        assert!(vt_style("div { view-transition-class: inherit-not 5 }").view_transition_class.is_none());
        assert!(vt_style("div { view-transition-class: 5 }").view_transition_class.is_none());
        let s = vt_style("div { view-transition-class: a; view-transition-class: 1x }");
        assert_eq!(s.view_transition_class.as_deref(), Some("a"));
    }

    #[test]
    fn view_transition_group_parsed_and_validated() {
        assert!(vt_style("").view_transition_group.is_none());
        assert_eq!(vt_style("div { view-transition-group: contain }").view_transition_group.as_deref(), Some("contain"));
        assert_eq!(vt_style("div { view-transition-group: NEAREST }").view_transition_group.as_deref(), Some("nearest"));
        assert_eq!(vt_style("div { view-transition-group: hero }").view_transition_group.as_deref(), Some("hero"));
        assert!(vt_style("div { view-transition-group: contain; view-transition-group: normal }").view_transition_group.is_none());
        assert!(vt_style("div { view-transition-group: a b }").view_transition_group.is_none());
        assert!(vt_style("div { view-transition-group: 5 }").view_transition_group.is_none());
    }

    #[test]
    fn view_transition_scope_parsed_and_validated() {
        assert!(!vt_style("").view_transition_scope_all);
        assert!(vt_style("div { view-transition-scope: all }").view_transition_scope_all);
        assert!(vt_style("div { view-transition-scope: ALL }").view_transition_scope_all);
        assert!(!vt_style("div { view-transition-scope: all; view-transition-scope: none }").view_transition_scope_all);
        // Invalid values drop the declaration; the property is not inherited.
        assert!(vt_style("div { view-transition-scope: all; view-transition-scope: auto }").view_transition_scope_all);
        assert!(vt_style("div { view-transition-scope: all; view-transition-scope: initial }")
            .view_transition_scope_all == false);
    }

    #[test]
    fn view_transition_name_default_is_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert!(s.view_transition_name.is_none());
    }

    #[test]
    fn view_transition_name_not_inherited() {
        let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
        let sheet = lumen_css_parser::parse("div { view-transition-name: parent-el; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
        assert_eq!(div_style.view_transition_name.as_deref(), Some("parent-el"));
        assert!(span_style.view_transition_name.is_none(), "view-transition-name must not be inherited");
    }

    #[test]
    fn view_transition_name_dashed_ident() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-transition-name: --my-element; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.view_transition_name.as_deref(), Some("--my-element"));
    }

    // ── CSS Scroll-Driven Animations ─────────────────────────────────────────

    #[test]
    fn scroll_timeline_name_parsed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { scroll-timeline-name: --my-tl; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.scroll_timeline_name.as_deref(), Some("--my-tl"));
        assert_eq!(s.scroll_timeline_axis, ScrollAxis::Block);
    }

    #[test]
    fn scroll_timeline_axis_inline() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "div { scroll-timeline-name: --t; scroll-timeline-axis: inline; }",
        );
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.scroll_timeline_axis, ScrollAxis::Inline);
    }

    #[test]
    fn scroll_timeline_shorthand() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { scroll-timeline: --tl x; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.scroll_timeline_name.as_deref(), Some("--tl"));
        assert_eq!(s.scroll_timeline_axis, ScrollAxis::X);
    }

    #[test]
    fn view_timeline_shorthand() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { view-timeline: --vt y; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.view_timeline_name.as_deref(), Some("--vt"));
        assert_eq!(s.view_timeline_axis, ScrollAxis::Y);
    }

    #[test]
    fn animation_timeline_auto() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: auto; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(s.animation_timelines, vec![AnimationTimeline::Auto]);
    }

    #[test]
    fn animation_timeline_scroll_fn() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: scroll(inline root); }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(
            s.animation_timelines,
            vec![AnimationTimeline::Scroll { axis: ScrollAxis::Inline, nearest: false }]
        );
    }

    #[test]
    fn animation_timeline_view_fn() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: view(inline); }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(
            s.animation_timelines,
            vec![AnimationTimeline::View { axis: ScrollAxis::Inline }]
        );
    }

    #[test]
    fn animation_timeline_named() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { animation-timeline: --my-scroll; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_eq!(
            s.animation_timelines,
            vec![AnimationTimeline::Named("--my-scroll".into())]
        );
    }

    // ── border-collapse ────────────────────────────────────────────────────────

    #[test]
    fn p4_border_collapse_default_is_separate() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Separate);
    }

    #[test]
    fn p4_border_collapse_parse_collapse() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { border-collapse: collapse; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Collapse);
    }

    #[test]
    fn p4_border_collapse_parse_separate_explicit() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { border-collapse: separate; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Separate);
    }

    #[test]
    fn p4_border_collapse_inherited_by_cells() {
        // border-collapse is inherited: td should see the table's collapse value.
        // Must walk the ancestor chain: root → body → table → (tbody) → tr → td.
        let doc = lumen_html_parser::parse("<table><tr><td>x</td></tr></table>");
        let sheet = lumen_css_parser::parse("table { border-collapse: collapse; }");
        let root_style = ComputedStyle::root();
        let body_node = doc.body().expect("body");
        let body_style = compute_style(&doc, body_node, &sheet, &root_style, VP, false);
        // Walk children to find the table, then tbody/tr, then td.
        fn find_tag(doc: &lumen_dom::Document, parent: lumen_dom::NodeId, tag_name: &str) -> Option<lumen_dom::NodeId> {
            for &c in &doc.get(parent).children {
                if let lumen_dom::NodeData::Element { name, .. } = &doc.get(c).data
                    && name.local == tag_name
                {
                    return Some(c);
                }
                if let Some(found) = find_tag(doc, c, tag_name) { return Some(found); }
            }
            None
        }
        let table = find_tag(&doc, body_node, "table").expect("table");
        let table_style = compute_style(&doc, table, &sheet, &body_style, VP, false);
        assert_eq!(table_style.border_collapse, BorderCollapse::Collapse, "table should have collapse");
        // TD inherits via tr; tr uses table_style (or intermediate row-group).
        let tr = find_tag(&doc, table, "tr").expect("tr");
        let tr_style = compute_style(&doc, tr, &sheet, &table_style, VP, false);
        let td = find_tag(&doc, tr, "td").expect("td");
        let td_style = compute_style(&doc, td, &sheet, &tr_style, VP, false);
        assert_eq!(td_style.border_collapse, BorderCollapse::Collapse, "td inherits collapse from table");
    }

    #[test]
    fn p4_border_collapse_initial_via_keyword() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse(
            "table { border-collapse: collapse; } table { border-collapse: initial; }",
        );
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.border_collapse, BorderCollapse::Separate, "initial resets to Separate");
    }

    // ── empty-cells ────────────────────────────────────────────────────────────

    #[test]
    fn p4_empty_cells_default_is_show() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Show);
    }

    #[test]
    fn p4_empty_cells_parse_hide() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { empty-cells: hide; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Hide);
    }

    #[test]
    fn p4_empty_cells_parse_show_explicit() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse("table { empty-cells: show; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Show);
    }

    #[test]
    fn p4_empty_cells_inherited_by_cells() {
        // empty-cells is inherited: td should see the table's hide value.
        let doc = lumen_html_parser::parse("<table><tr><td>x</td></tr></table>");
        let sheet = lumen_css_parser::parse("table { empty-cells: hide; }");
        let root_style = ComputedStyle::root();
        let body_node = doc.body().expect("body");
        let body_style = compute_style(&doc, body_node, &sheet, &root_style, VP, false);
        fn find_tag(doc: &lumen_dom::Document, parent: lumen_dom::NodeId, tag_name: &str) -> Option<lumen_dom::NodeId> {
            for &c in &doc.get(parent).children {
                if let lumen_dom::NodeData::Element { name, .. } = &doc.get(c).data
                    && name.local == tag_name
                {
                    return Some(c);
                }
                if let Some(found) = find_tag(doc, c, tag_name) { return Some(found); }
            }
            None
        }
        let table = find_tag(&doc, body_node, "table").expect("table");
        let table_style = compute_style(&doc, table, &sheet, &body_style, VP, false);
        let tr = find_tag(&doc, table, "tr").expect("tr");
        let tr_style = compute_style(&doc, tr, &sheet, &table_style, VP, false);
        let td = find_tag(&doc, tr, "td").expect("td");
        let td_style = compute_style(&doc, td, &sheet, &tr_style, VP, false);
        assert_eq!(td_style.empty_cells, EmptyCells::Hide, "td inherits hide from table");
    }

    #[test]
    fn p4_empty_cells_initial_via_keyword() {
        let doc = lumen_html_parser::parse("<table></table>");
        let sheet = lumen_css_parser::parse(
            "table { empty-cells: hide; } table { empty-cells: initial; }",
        );
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let table = doc.get(body).children[0];
        let s = compute_style(&doc, table, &sheet, &root, VP, false);
        assert_eq!(s.empty_cells, EmptyCells::Show, "initial resets to Show");
    }

    #[test]
    fn p4_empty_cells_keyword_parse() {
        assert_eq!(EmptyCells::parse("show"), Some(EmptyCells::Show));
        assert_eq!(EmptyCells::parse("hide"), Some(EmptyCells::Hide));
        assert_eq!(EmptyCells::parse("bogus"), None);
    }

    // ── StyleEnvSnapshot (ADR-016 M4.1) ──────────────────────────────────────

    #[test]
    fn style_env_snapshot_captures_and_restores_interactive_state() {
        // Set a non-default interactive state, capture it, reset, then install
        // the snapshot and verify the state is restored correctly.
        use lumen_dom::NodeId;

        let hover  = NodeId::from_index(42);
        let focus  = NodeId::from_index(7);
        let active = NodeId::from_index(99);

        set_interactive_state(Some(hover), Some(focus), Some(active));
        set_forced_colors(true);
        set_print_media(true);

        let snap = StyleEnvSnapshot::capture();

        // Reset to defaults on this thread.
        clear_interactive_state();
        set_forced_colors(false);
        set_print_media(false);

        assert!(!forced_colors_active());
        assert!(!print_media_active());

        // Install snapshot — must restore the captured values.
        snap.install();

        assert!(forced_colors_active(), "forced colors must be restored");
        assert!(print_media_active(), "print media must be restored");
        assert_eq!(
            HOVER_NID.with(Cell::get),
            hover.index() as u32,
            "hover nid must be restored",
        );
        assert_eq!(
            FOCUS_NID.with(Cell::get),
            focus.index() as u32,
            "focus nid must be restored",
        );
        assert_eq!(
            ACTIVE_NID.with(Cell::get),
            active.index() as u32,
            "active nid must be restored",
        );

        // Cleanup so later tests on this thread see the screen default.
        clear_interactive_state();
        set_forced_colors(false);
        set_print_media(false);
    }

    #[test]
    fn style_env_snapshot_default_state_is_clean() {
        // A snapshot captured in the default (no interactive state) condition
        // must reinstall u32::MAX for all nids and false for bool flags.
        clear_interactive_state();
        set_forced_colors(false);
        set_print_media(false);

        let snap = StyleEnvSnapshot::capture();

        // Corrupt thread state briefly, then restore via install.
        HOVER_NID.with(|h| h.set(1));
        FORCED_COLORS.with(|f| f.set(true));

        snap.install();

        assert_eq!(HOVER_NID.with(Cell::get), u32::MAX, "hover must be MAX");
        assert!(!forced_colors_active(), "forced colors must be false");
    }


    // GAP-ANCHORCSSOM-S2: канонизатор `element.style` для верхнеуровневых
    // anchor()/anchor-size().

    #[test]
    fn canonical_anchor_puts_name_first_and_lowercases_side() {
        assert_eq!(canonical_specified_anchor("anchor(--a top)", true).as_deref(), Some("anchor(--a top)"));
        assert_eq!(canonical_specified_anchor("anchor(top --a)", true).as_deref(), Some("anchor(--a top)"));
        assert_eq!(canonical_specified_anchor("ANCHOR( TOP )", true).as_deref(), Some("anchor(top)"));
        assert_eq!(canonical_specified_anchor("anchor(--a 50%)", true).as_deref(), Some("anchor(--a 50%)"));
        assert_eq!(canonical_specified_anchor("anchor(self-end)", true).as_deref(), Some("anchor(self-end)"));
    }

    #[test]
    fn canonical_anchor_serializes_fallback_as_length() {
        assert_eq!(canonical_specified_anchor("anchor(--a left, 0)", true).as_deref(), Some("anchor(--a left, 0px)"));
        assert_eq!(canonical_specified_anchor("anchor(left --a,1px)", true).as_deref(), Some("anchor(--a left, 1px)"));
        assert_eq!(
            canonical_specified_anchor("anchor(top, calc(50% + 1px))", true).as_deref(),
            Some("anchor(top, calc(50% + 1px))")
        );
    }

    #[test]
    fn canonical_anchor_rejects_invalid_forms() {
        for bad in [
            "anchor(--a, top)", "anchor(--a top,)", "anchor(--a top bottom)", "anchor(--a top, 10px 20%)",
            "anchor(--a top, 10px, 20%)", "anchor(foo top)", "anchor(top foo)", "anchor(--a height)",
            "anchor(--a 10em)", "anchor(--a top, 1)", "anchor(--a top, bottom)", "anchor(--a top, auto",
            "anchor()", "anchor(--a)", "10px",
        ] {
            assert_eq!(canonical_specified_anchor(bad, true), None, "{bad}");
        }
    }

    #[test]
    fn canonical_anchor_size_orders_name_before_dimension() {
        assert_eq!(canonical_specified_anchor("anchor-size(--a width)", false).as_deref(), Some("anchor-size(--a width)"));
        assert_eq!(canonical_specified_anchor("anchor-size(width --a)", false).as_deref(), Some("anchor-size(--a width)"));
        assert_eq!(canonical_specified_anchor("anchor-size(self-inline)", false).as_deref(), Some("anchor-size(self-inline)"));
        assert_eq!(
            canonical_specified_anchor("anchor-size(--a block, 0)", false).as_deref(),
            Some("anchor-size(--a block, 0px)")
        );
    }

    #[test]
    fn canonical_anchor_size_rejects_invalid_forms() {
        for bad in [
            "anchor-size(--a, width)", "anchor-size(--a width,)", "anchor-size(--a width height)",
            "anchor-size(--a width, 10px 20%)", "anchor-size(foo width)", "anchor-size(--a top)",
            "anchor-size(--a 50%)", "anchor-size(--a width, 1)", "anchor-size(--a width, height)",
        ] {
            assert_eq!(canonical_specified_anchor(bad, false), None, "{bad}");
        }
    }

    #[test]
    fn canonical_anchor_not_allowed_for_anchor_only_in_size_properties() {
        assert_eq!(canonical_specified_anchor("anchor(--a top)", false), None);
        assert!(canonical_specified_anchor("anchor-size(--a width)", false).is_some());
    }

    // ── BUG-1478: computed values reported by getComputedStyle ──────────────

    fn anchor_prop(css: &str, prop: &str) -> String {
        let s = first_div_style("<div></div>", &format!("div {{ {css} }}"));
        crate::selector_query::computed_style_to_map(&s).get(prop).cloned().unwrap_or_default()
    }

    #[test]
    fn anchor_props_in_computed_map_with_initials() {
        for (prop, init) in [
            ("anchor-name", "none"), ("anchor-scope", "none"), ("position-area", "none"),
            ("position-anchor", "auto"), ("position-visibility", "anchors-visible"),
        ] {
            assert_eq!(anchor_prop("", prop), init, "{prop}");
        }
    }

    #[test]
    fn anchor_name_and_scope_lists_serialize_joined() {
        assert_eq!(anchor_prop("anchor-name: --a,--b", "anchor-name"), "--a, --b");
        assert_eq!(anchor_prop("anchor-scope: --foo ,  --bar", "anchor-scope"), "--foo, --bar");
        assert_eq!(anchor_prop("anchor-scope: all", "anchor-scope"), "all");
        assert_eq!(anchor_prop("anchor-name: --a, foo", "anchor-name"), "none");
    }

    #[test]
    fn position_area_computed_canonical_forms() {
        for (given, want) in [
            ("top left", "left top"), ("center left", "left center"), ("top center", "center top"),
            ("left span-all", "left"), ("span-all top", "top"), ("center center", "center"),
            ("block-start inline-end", "start end"), ("block-end inline-end", "end"),
            ("inline-start center", "center start"), ("block-start span-all", "block-start"),
            ("span-self-block-end self-inline-start", "span-self-end self-start"),
            ("start end", "start end"), ("start start", "start"), ("start span-all", "block-start"),
            ("span-all span-end", "span-inline-end"), ("span-all span-all", "span-all"),
            ("span-all center", "span-all center"), ("start center", "start center"),
            ("x-start y-end", "x-start y-end"), ("none", "none"),
        ] {
            let got = anchor_prop(&format!("position-area: {given}"), "position-area");
            assert_eq!(got, want, "{given}");
        }
    }

    #[test]
    fn position_visibility_flags_serialize_in_fixed_order() {
        for (given, want) in [
            ("always", "always"), ("no-overflow anchors-visible", "anchors-visible no-overflow"),
            ("no-overflow anchors-visible anchors-valid", "anchors-valid anchors-visible no-overflow"),
            ("anchors-valid anchors-valid", "anchors-visible"),
        ] {
            assert_eq!(anchor_prop(&format!("position-visibility: {given}"), "position-visibility"), want, "{given}");
        }
    }
