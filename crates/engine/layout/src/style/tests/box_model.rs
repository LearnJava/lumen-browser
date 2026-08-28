//! Тесты `style.rs`: бокс: границы, `box-sizing`, радиусы, `object-fit`, `vertical-align`,
//! SVG-краска.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ── Border parsing ────────────────────────────────────────────────────────

    #[test]
    fn border_shorthand_sets_all_sides() {
        let s = style_for("border: 2px solid red");
        assert!((s.border_top_width - 2.0).abs() < 0.01);
        assert!((s.border_right_width - 2.0).abs() < 0.01);
        assert!((s.border_bottom_width - 2.0).abs() < 0.01);
        assert!((s.border_left_width - 2.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Solid);
        assert_eq!(s.border_right_style, BorderStyle::Solid);
        assert_eq!(s.border_bottom_style, BorderStyle::Solid);
        assert_eq!(s.border_left_style, BorderStyle::Solid);
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        assert_eq!(s.border_top_color, CssColor::Rgba(red));
        assert_eq!(s.border_right_color, CssColor::Rgba(red));
    }

    #[test]
    fn border_width_shorthand_1_value() {
        let s = style_for("border-width: 5px");
        assert!((s.border_top_width - 5.0).abs() < 0.01);
        assert!((s.border_right_width - 5.0).abs() < 0.01);
        assert!((s.border_bottom_width - 5.0).abs() < 0.01);
        assert!((s.border_left_width - 5.0).abs() < 0.01);
    }

    #[test]
    fn border_style_sets_all_sides() {
        let s = style_for("border-style: dashed");
        assert_eq!(s.border_top_style, BorderStyle::Dashed);
        assert_eq!(s.border_bottom_style, BorderStyle::Dashed);
    }

    #[test]
    fn border_color_shorthand() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let s = style_for("border-color: blue");
        assert_eq!(s.border_top_color, CssColor::Rgba(blue));
        assert_eq!(s.border_left_color, CssColor::Rgba(blue));
    }

    #[test]
    fn border_top_side_shorthand() {
        let s = style_for("border-top: 3px dotted green");
        assert!((s.border_top_width - 3.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Dotted);
        let green = Color { r: 0, g: 128, b: 0, a: 255 };
        assert_eq!(s.border_top_color, CssColor::Rgba(green));
        // Остальные стороны — не изменены.
        assert!((s.border_right_width - 0.0).abs() < 0.01);
        assert_eq!(s.border_right_style, BorderStyle::None);
    }

    #[test]
    fn border_per_side_width_properties() {
        let s = style_for("border-left-width: 4px; border-right-width: 6px");
        assert!((s.border_left_width - 4.0).abs() < 0.01);
        assert!((s.border_right_width - 6.0).abs() < 0.01);
        assert!((s.border_top_width - 0.0).abs() < 0.01);
    }

    #[test]
    fn border_no_color_means_none() {
        let s = style_for("border: 2px solid");
        assert!(matches!(s.border_top_color, CssColor::CurrentColor));
    }

    #[test]
    fn border_style_kw_none_is_invisible() {
        assert!(!BorderStyle::None.is_visible());
        assert!(BorderStyle::Solid.is_visible());
        assert!(BorderStyle::Dashed.is_visible());
        assert!(BorderStyle::Dotted.is_visible());
        assert!(BorderStyle::Double.is_visible());
    }

    #[test]
    fn border_style_double_parses() {
        let s = style_for("border: 6px double red");
        assert_eq!(s.border_top_style, BorderStyle::Double);
        assert_eq!(s.border_right_style, BorderStyle::Double);
        assert_eq!(s.border_bottom_style, BorderStyle::Double);
        assert_eq!(s.border_left_style, BorderStyle::Double);
    }

    #[test]
    fn border_style_double_per_side() {
        let s = style_for("border-top-style: double; border-bottom-style: double");
        assert_eq!(s.border_top_style, BorderStyle::Double);
        assert_eq!(s.border_bottom_style, BorderStyle::Double);
        assert_eq!(s.border_left_style, BorderStyle::None);
    }

    // ── box-sizing parsing ─────────────────────────────────────────────────

    #[test]
    fn box_sizing_default_is_content_box() {
        let s = style_for("color: red");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_border_box_parses() {
        let s = style_for("box-sizing: border-box");
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
    }

    #[test]
    fn box_sizing_content_box_parses_back_to_default() {
        // Явное content-box после border-box возвращает к default.
        let s = style_for("box-sizing: border-box; box-sizing: content-box");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_case_insensitive() {
        let s = style_for("box-sizing: BORDER-BOX");
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
    }

    #[test]
    fn box_sizing_unknown_value_keeps_default() {
        // CSS-парсер не должен падать на мусоре — оставляет предыдущее значение.
        let s = style_for("box-sizing: padding-box");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_not_inherited() {
        // box-sizing — non-inherited (CSS Basic UI 3 §4.1).
        // Дочерний <p> не получает border-box от родительского <div>.
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { box-sizing: border-box; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.box_sizing, BoxSizing::BorderBox);
        assert_eq!(p_style.box_sizing, BoxSizing::ContentBox);
    }

    // ──────────────── CSS Images L3 §5.5: object-fit / object-position ────────────────

    #[test]
    fn object_fit_default_is_fill() {
        let s = cascade_at("<img>", "", &[0]);
        assert_eq!(s.object_fit, ObjectFit::Fill);
    }

    #[test]
    fn object_fit_keywords_parse() {
        for (val, expected) in [
            ("fill", ObjectFit::Fill),
            ("contain", ObjectFit::Contain),
            ("cover", ObjectFit::Cover),
            ("none", ObjectFit::None),
            ("scale-down", ObjectFit::ScaleDown),
        ] {
            let s = cascade_at(
                "<img>",
                &format!("img {{ object-fit: {val}; }}"),
                &[0],
            );
            assert_eq!(s.object_fit, expected, "for value {val}");
        }
    }

    #[test]
    fn object_fit_invalid_value_ignored() {
        // CSS Cascade §8.1: невалидное значение → declaration invalid →
        // используется предыдущее (initial = Fill).
        let s = cascade_at("<img>", "img { object-fit: bogus; }", &[0]);
        assert_eq!(s.object_fit, ObjectFit::Fill);
    }

    #[test]
    fn object_fit_case_insensitive() {
        // CSS Values L4 §2.4 — keywords ASCII case-insensitive.
        let s = cascade_at("<img>", "img { object-fit: COVER; }", &[0]);
        assert_eq!(s.object_fit, ObjectFit::Cover);
    }

    #[test]
    fn object_fit_not_inherited() {
        // object-fit non-inherited; <img> внутри <div> не подхватывает div { ... }
        // (хотя div и не replaced, но пример демонстрирует отсутствие inheritance
        // через initial-value у потомка-не-замены).
        let s = cascade_at(
            "<div><img></div>",
            "div { object-fit: cover; }",
            &[0, 0],
        );
        assert_eq!(s.object_fit, ObjectFit::Fill);
    }

    #[test]
    fn object_fit_inherit_keyword_pulls_parent_value() {
        // CSS Cascade L4 §7 — `inherit` всегда работает, даже для
        // non-inherited свойства.
        let s = cascade_at(
            "<div><img></div>",
            "div { object-fit: contain; } img { object-fit: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.object_fit, ObjectFit::Contain);
    }

    #[test]
    fn object_position_default_is_center() {
        let s = cascade_at("<img>", "", &[0]);
        assert_eq!(
            s.object_position,
            ObjectPosition {
                x: PositionComponent::Percent(0.5),
                y: PositionComponent::Percent(0.5),
            }
        );
    }

    #[test]
    fn object_position_two_percent_values() {
        let s = cascade_at(
            "<img>",
            "img { object-position: 25% 75%; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.25));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.75));
    }

    #[test]
    fn object_position_two_lengths() {
        let s = cascade_at(
            "<img>",
            "img { object-position: 10px 20px; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Px(10.0));
        assert_eq!(s.object_position.y, PositionComponent::Px(20.0));
    }

    #[test]
    fn object_position_single_value_centers_y() {
        // Один token → x = token, y = center (50%).
        let s = cascade_at(
            "<img>",
            "img { object-position: 10px; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Px(10.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn object_position_keyword_left_top() {
        let s = cascade_at(
            "<img>",
            "img { object-position: left top; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.0));
    }

    #[test]
    fn object_position_keyword_right_bottom() {
        let s = cascade_at(
            "<img>",
            "img { object-position: right bottom; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(1.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(1.0));
    }

    #[test]
    fn object_position_keyword_swap_top_left_means_left_top() {
        // CSS Values L4 §9.4: `top left` ≡ `left top`.
        let s = cascade_at(
            "<img>",
            "img { object-position: top left; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.0));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.0));
    }

    #[test]
    fn object_position_single_top_centers_x() {
        let s = cascade_at("<img>", "img { object-position: top; }", &[0]);
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.0));
    }

    #[test]
    fn object_position_single_center_is_50_50() {
        let s = cascade_at("<img>", "img { object-position: center; }", &[0]);
        assert_eq!(s.object_position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.object_position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn object_position_invalid_value_keeps_default() {
        // 3 token-а — пока не поддерживаем; декларация ignored.
        let s = cascade_at(
            "<img>",
            "img { object-position: left 10px top; }",
            &[0],
        );
        // initial-value сохранён.
        assert_eq!(s.object_position, ObjectPosition::default());
    }

    #[test]
    fn object_position_negative_percent_allowed() {
        // Художественное смещение `-25% 110%` валидно.
        let s = cascade_at(
            "<img>",
            "img { object-position: -25% 110%; }",
            &[0],
        );
        assert_eq!(s.object_position.x, PositionComponent::Percent(-0.25));
        assert_eq!(s.object_position.y, PositionComponent::Percent(1.1));
    }

    #[test]
    fn position_component_resolve_percent_against_free_space() {
        let pc = PositionComponent::Percent(0.5);
        assert!((pc.resolve(100.0) - 50.0).abs() < f32::EPSILON);
        // Отрицательное free_space (content больше box) — offset отрицательный.
        assert!((pc.resolve(-40.0) - (-20.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn position_component_resolve_px_ignores_free_space() {
        let pc = PositionComponent::Px(15.0);
        assert!((pc.resolve(0.0) - 15.0).abs() < f32::EPSILON);
        assert!((pc.resolve(1000.0) - 15.0).abs() < f32::EPSILON);
    }

    // -------- vertical-align (CSS 2.1 §10.8.1) --------

    #[test]
    fn vertical_align_default_is_baseline() {
        let s = cascade_at("<span></span>", "", &[0]);
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_all_keywords_parse() {
        for (val, expected) in [
            ("baseline", VerticalAlign::Baseline),
            ("sub", VerticalAlign::Sub),
            ("super", VerticalAlign::Super),
            ("top", VerticalAlign::Top),
            ("text-top", VerticalAlign::TextTop),
            ("middle", VerticalAlign::Middle),
            ("bottom", VerticalAlign::Bottom),
            ("text-bottom", VerticalAlign::TextBottom),
        ] {
            let s = cascade_at(
                "<span></span>",
                &format!("span {{ vertical-align: {val}; }}"),
                &[0],
            );
            assert_eq!(s.vertical_align, expected, "for value {val}");
        }
    }

    #[test]
    fn vertical_align_keywords_case_insensitive() {
        // CSS Values L4 §2.4 — keywords ASCII case-insensitive.
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: TEXT-Top; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::TextTop);
    }

    #[test]
    fn vertical_align_length_px() {
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: 5px; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Length(5.0));
    }

    #[test]
    fn vertical_align_negative_length() {
        // Спецификация допускает отрицательные значения — сдвиг вниз
        // от baseline.
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: -3px; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Length(-3.0));
    }

    #[test]
    fn vertical_align_em_resolved_against_element_font_size() {
        // em для vertical-align резолвится по текущему font-size (10pxx2=20).
        // Используем явный font-size 20 чтобы избежать зависимости от
        // initial 16px (UA stylesheet может его не выставлять).
        let s = cascade_at(
            "<span></span>",
            "span { font-size: 20px; vertical-align: 0.5em; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Length(10.0));
    }

    #[test]
    fn vertical_align_percent_kept_as_percent() {
        // % резолвится по line-height в layout-pass, не на этапе cascade —
        // поэтому здесь должен остаться как Percent(50.0).
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: 50%; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Percent(50.0));
    }

    #[test]
    fn vertical_align_invalid_value_ignored() {
        // Невалидное значение — declaration invalid; остаётся initial.
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: bogus; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_not_inherited() {
        // CSS 2.1 §10.8.1 — non-inherited. Ребёнок без своей декларации
        // получает initial-value, а не значение родителя.
        let s = cascade_at(
            "<div><span></span></div>",
            "div { vertical-align: super; }",
            &[0, 0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_inherit_keyword_pulls_parent_value() {
        // CSS Cascade L4 §7 — `inherit` принудительно тянет значение
        // родителя даже для non-inherited свойства.
        let s = cascade_at(
            "<div><span></span></div>",
            "div { vertical-align: sub; } span { vertical-align: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Sub);
    }

    #[test]
    fn vertical_align_initial_keyword_resets() {
        // `initial` всегда даёт initial-value свойства (Baseline).
        let s = cascade_at(
            "<span></span>",
            "span { vertical-align: top; vertical-align: initial; }",
            &[0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    #[test]
    fn vertical_align_unset_for_non_inherited_is_initial() {
        // CSS Cascade L4 §7: `unset` = `initial` для non-inherited.
        let s = cascade_at(
            "<div><span></span></div>",
            "div { vertical-align: middle; } span { vertical-align: unset; }",
            &[0, 0],
        );
        assert_eq!(s.vertical_align, VerticalAlign::Baseline);
    }

    // === clip-rule parsing (SVG §14.3.4) ===

    #[test]
    fn clip_rule_default_is_nonzero() {
        assert_eq!(ComputedStyle::root().svg_clip_rule, FillRule::NonZero);
    }

    #[test]
    fn clip_rule_parses_evenodd_and_nonzero() {
        assert_eq!(ts_prop("clip-rule", "evenodd").svg_clip_rule, FillRule::EvenOdd);
        // Case-insensitive per CSS keyword matching.
        assert_eq!(ts_prop("clip-rule", "EvenOdd").svg_clip_rule, FillRule::EvenOdd);
        assert_eq!(ts_prop("clip-rule", "nonzero").svg_clip_rule, FillRule::NonZero);
    }

    #[test]
    fn clip_rule_invalid_keeps_default() {
        // Unknown keyword leaves the initial value untouched.
        assert_eq!(ts_prop("clip-rule", "bogus").svg_clip_rule, FillRule::NonZero);
    }

    #[test]
    fn clip_rule_is_independent_of_fill_rule() {
        // The two share FillRule but are distinct properties.
        let s = ts_prop("clip-rule", "evenodd");
        assert_eq!(s.svg_clip_rule, FillRule::EvenOdd);
        assert_eq!(s.svg_fill_rule, FillRule::NonZero);
    }

    #[test]
    fn clip_rule_explicit_inherit_takes_parent_value() {
        // clip-rule is inherited (SVG §14.3.4): `clip-rule: inherit` copies the
        // parent's value; `initial` resets to nonzero.
        let mut parent = ComputedStyle::root();
        parent.svg_clip_rule = FillRule::EvenOdd;
        let vp = Size::new(800.0, 600.0);

        let mut child = ComputedStyle::root();
        let inherit = Declaration {
            property: "clip-rule".to_string(),
            value: "inherit".to_string(),
            important: false,
        };
        apply_declaration(&mut child, &inherit, 16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false);
        assert_eq!(child.svg_clip_rule, FillRule::EvenOdd);

        let mut child2 = ComputedStyle::root();
        child2.svg_clip_rule = FillRule::EvenOdd;
        let initial = Declaration {
            property: "clip-rule".to_string(),
            value: "initial".to_string(),
            important: false,
        };
        apply_declaration(&mut child2, &initial, 16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false);
        assert_eq!(child2.svg_clip_rule, FillRule::NonZero);
    }

    // === paint-order parsing (CSS Fill & Stroke L3 §6 / SVG 2 §13.7) ===

    #[test]
    fn paint_order_normal_is_default() {
        use PaintOrderSlot::{Fill, Markers, Stroke};
        assert_eq!(ts_prop("paint-order", "normal").paint_order.0, [Fill, Stroke, Markers]);
        // Root initial value is also normal.
        assert_eq!(ComputedStyle::root().paint_order, SvgPaintOrder::default());
    }

    #[test]
    fn paint_order_stroke_first_then_canonical_rest() {
        use PaintOrderSlot::{Fill, Markers, Stroke};
        // Single component → remaining appended in canonical fill, stroke, markers order.
        assert_eq!(ts_prop("paint-order", "stroke").paint_order.0, [Stroke, Fill, Markers]);
        assert_eq!(ts_prop("paint-order", "markers").paint_order.0, [Markers, Fill, Stroke]);
    }

    #[test]
    fn paint_order_two_components_keep_order() {
        use PaintOrderSlot::{Fill, Markers, Stroke};
        assert_eq!(
            ts_prop("paint-order", "stroke markers").paint_order.0,
            [Stroke, Markers, Fill]
        );
    }

    #[test]
    fn paint_order_invalid_rejected() {
        // Unknown token and repeated component are invalid → keep current value.
        assert!(SvgPaintOrder::parse("bogus").is_none());
        assert!(SvgPaintOrder::parse("fill fill").is_none());
        assert!(SvgPaintOrder::parse("").is_none());
        // apply_declaration leaves the (default) value unchanged on invalid input.
        assert_eq!(ts_prop("paint-order", "bogus").paint_order, SvgPaintOrder::default());
    }

    #[test]
    fn paint_order_fill_before_stroke_decision() {
        assert!(SvgPaintOrder::parse("normal").unwrap().fill_before_stroke());
        assert!(SvgPaintOrder::parse("fill stroke").unwrap().fill_before_stroke());
        assert!(!SvgPaintOrder::parse("stroke").unwrap().fill_before_stroke());
        assert!(!SvgPaintOrder::parse("stroke fill").unwrap().fill_before_stroke());
        // markers-first but fill still before stroke.
        assert!(SvgPaintOrder::parse("markers fill stroke").unwrap().fill_before_stroke());
    }

    #[test]
    fn paint_order_inherited_via_unset() {
        // paint-order is inherited: `unset` resolves to the inherited value.
        let mut parent = ComputedStyle::root();
        parent.paint_order = SvgPaintOrder::parse("stroke").unwrap();
        let mut s = ComputedStyle::root();
        let decl = Declaration {
            property: "paint-order".to_string(),
            value: "unset".to_string(),
            important: false,
        };
        let vp = Size::new(800.0, 600.0);
        apply_declaration(&mut s, &decl, 16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false);
        assert_eq!(s.paint_order, parent.paint_order);
    }

    // === text-anchor / dominant-baseline as CSS properties (SVG 2 §11.6 / §11.10.2) ===

    #[test]
    fn text_anchor_default_is_none() {
        // Initial: unset by CSS → None (the `start` initial / presentation attribute
        // applies at box build time).
        assert_eq!(ComputedStyle::root().text_anchor, None);
        assert_eq!(ComputedStyle::root().dominant_baseline, None);
    }

    #[test]
    fn text_anchor_parses_all_keywords() {
        use crate::box_tree::SvgTextAnchor;
        assert_eq!(ts_prop("text-anchor", "start").text_anchor, Some(SvgTextAnchor::Start));
        assert_eq!(ts_prop("text-anchor", "middle").text_anchor, Some(SvgTextAnchor::Middle));
        assert_eq!(ts_prop("text-anchor", "end").text_anchor, Some(SvgTextAnchor::End));
        // Case-insensitive.
        assert_eq!(ts_prop("text-anchor", "MIDDLE").text_anchor, Some(SvgTextAnchor::Middle));
    }

    #[test]
    fn dominant_baseline_parses_all_keywords() {
        use crate::box_tree::SvgDominantBaseline;
        assert_eq!(ts_prop("dominant-baseline", "auto").dominant_baseline, Some(SvgDominantBaseline::Auto));
        assert_eq!(ts_prop("dominant-baseline", "central").dominant_baseline, Some(SvgDominantBaseline::Central));
        assert_eq!(
            ts_prop("dominant-baseline", "text-before-edge").dominant_baseline,
            Some(SvgDominantBaseline::TextBeforeEdge)
        );
        assert_eq!(
            ts_prop("dominant-baseline", "hanging").dominant_baseline,
            Some(SvgDominantBaseline::Hanging)
        );
    }

    #[test]
    fn text_anchor_invalid_keeps_current() {
        // Unknown value → leave the cascaded value untouched (here: None).
        assert_eq!(ts_prop("text-anchor", "bogus").text_anchor, None);
        assert_eq!(ts_prop("dominant-baseline", "sideways").dominant_baseline, None);
    }

    #[test]
    fn text_anchor_inherited_via_unset() {
        // Both are inherited: `unset` resolves to the inherited value (SVG container
        // text-anchor propagates to descendant <text>).
        use crate::box_tree::{SvgDominantBaseline, SvgTextAnchor};
        let mut parent = ComputedStyle::root();
        parent.text_anchor = Some(SvgTextAnchor::End);
        parent.dominant_baseline = Some(SvgDominantBaseline::Middle);
        let vp = Size::new(800.0, 600.0);
        let mut s = ComputedStyle::root();
        apply_declaration(
            &mut s,
            &Declaration { property: "text-anchor".into(), value: "unset".into(), important: false },
            16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false,
        );
        apply_declaration(
            &mut s,
            &Declaration { property: "dominant-baseline".into(), value: "unset".into(), important: false },
            16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false,
        );
        assert_eq!(s.text_anchor, Some(SvgTextAnchor::End));
        assert_eq!(s.dominant_baseline, Some(SvgDominantBaseline::Middle));
    }

    #[test]
    fn text_anchor_initial_resets_to_none() {
        // `initial` → None (CSS unset, defers to attribute/`start` initial).
        use crate::box_tree::SvgTextAnchor;
        let mut parent = ComputedStyle::root();
        parent.text_anchor = Some(SvgTextAnchor::End);
        let vp = Size::new(800.0, 600.0);
        let mut s = ComputedStyle::root();
        apply_declaration(
            &mut s,
            &Declaration { property: "text-anchor".into(), value: "initial".into(), important: false },
            16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false,
        );
        assert_eq!(s.text_anchor, None);
    }

    // === baseline-shift parsing (SVG 1.1 §10.9.2 / CSS Inline L3 §5.2) ===

    #[test]
    fn baseline_shift_default_is_baseline() {
        use crate::box_tree::SvgBaselineShift;
        assert_eq!(ComputedStyle::root().baseline_shift, SvgBaselineShift::Baseline);
    }

    #[test]
    fn baseline_shift_keywords() {
        use crate::box_tree::SvgBaselineShift;
        assert_eq!(ts_prop("baseline-shift", "baseline").baseline_shift, SvgBaselineShift::Baseline);
        assert_eq!(ts_prop("baseline-shift", "sub").baseline_shift, SvgBaselineShift::Sub);
        assert_eq!(ts_prop("baseline-shift", "super").baseline_shift, SvgBaselineShift::Super);
        // Case-insensitive.
        assert_eq!(ts_prop("baseline-shift", "SUPER").baseline_shift, SvgBaselineShift::Super);
    }

    #[test]
    fn baseline_shift_length_and_percentage() {
        use crate::box_tree::SvgBaselineShift;
        assert_eq!(ts_prop("baseline-shift", "4px").baseline_shift, SvgBaselineShift::Length(4.0));
        // 0.5em with em_basis 16px → 8px.
        assert_eq!(ts_prop("baseline-shift", "0.5em").baseline_shift, SvgBaselineShift::Length(8.0));
        assert_eq!(ts_prop("baseline-shift", "50%").baseline_shift, SvgBaselineShift::Percentage(0.5));
    }

    #[test]
    fn baseline_shift_invalid_keeps_current() {
        use crate::box_tree::SvgBaselineShift;
        // Unknown value → leave the cascaded value untouched (here: Baseline).
        assert_eq!(ts_prop("baseline-shift", "bogus").baseline_shift, SvgBaselineShift::Baseline);
    }

    #[test]
    fn baseline_shift_not_inherited() {
        // baseline-shift is NOT inherited: `unset` resolves to the initial value,
        // not the parent's value.
        use crate::box_tree::SvgBaselineShift;
        let mut parent = ComputedStyle::root();
        parent.baseline_shift = SvgBaselineShift::Super;
        let vp = Size::new(800.0, 600.0);
        let mut s = ComputedStyle::root();
        apply_declaration(
            &mut s,
            &Declaration { property: "baseline-shift".into(), value: "unset".into(), important: false },
            16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false,
        );
        assert_eq!(s.baseline_shift, SvgBaselineShift::Baseline);
        // `inherit` explicitly pulls the parent value.
        let mut s2 = ComputedStyle::root();
        apply_declaration(
            &mut s2,
            &Declaration { property: "baseline-shift".into(), value: "inherit".into(), important: false },
            16.0, vp, FontWeight::NORMAL, &parent, &parent, false, false,
        );
        assert_eq!(s2.baseline_shift, SvgBaselineShift::Super);
    }

    // --- border-radius elliptical (CSS Backgrounds L3 §5.5) ---

    #[test]
    fn border_radius_circular_shorthand() {
        // `border-radius: 10px` — все 4 угла круговые (rx == ry).
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-radius: 10px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_left_radius,       Length::Px(10.0));
        assert_eq!(s.border_top_left_radius_y,     Length::Px(10.0));
        assert_eq!(s.border_top_right_radius,      Length::Px(10.0));
        assert_eq!(s.border_top_right_radius_y,    Length::Px(10.0));
        assert_eq!(s.border_bottom_right_radius,   Length::Px(10.0));
        assert_eq!(s.border_bottom_right_radius_y, Length::Px(10.0));
        assert_eq!(s.border_bottom_left_radius,    Length::Px(10.0));
        assert_eq!(s.border_bottom_left_radius_y,  Length::Px(10.0));
    }

    #[test]
    fn border_radius_elliptical_shorthand_uniform() {
        // `border-radius: 20px / 10px` — все углы эллиптические, rx=20 ry=10.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-radius: 20px / 10px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_left_radius,       Length::Px(20.0));
        assert_eq!(s.border_top_left_radius_y,     Length::Px(10.0));
        assert_eq!(s.border_top_right_radius,      Length::Px(20.0));
        assert_eq!(s.border_top_right_radius_y,    Length::Px(10.0));
        assert_eq!(s.border_bottom_right_radius,   Length::Px(20.0));
        assert_eq!(s.border_bottom_right_radius_y, Length::Px(10.0));
        assert_eq!(s.border_bottom_left_radius,    Length::Px(20.0));
        assert_eq!(s.border_bottom_left_radius_y,  Length::Px(10.0));
    }

    #[test]
    fn border_radius_elliptical_shorthand_per_corner() {
        // `border-radius: 10px 20px / 5px 15px` — TL/BR rx=10, TR/BL rx=20, ry=5/15.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-radius: 10px 20px / 5px 15px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_left_radius,        Length::Px(10.0)); // TL rx
        assert_eq!(s.border_top_left_radius_y,      Length::Px(5.0));  // TL ry
        assert_eq!(s.border_top_right_radius,       Length::Px(20.0)); // TR rx
        assert_eq!(s.border_top_right_radius_y,     Length::Px(15.0)); // TR ry
        assert_eq!(s.border_bottom_right_radius,    Length::Px(10.0)); // BR rx (mirrors TL)
        assert_eq!(s.border_bottom_right_radius_y,  Length::Px(5.0));  // BR ry (mirrors TL)
        assert_eq!(s.border_bottom_left_radius,     Length::Px(20.0)); // BL rx (mirrors TR)
        assert_eq!(s.border_bottom_left_radius_y,   Length::Px(15.0)); // BL ry (mirrors TR)
    }

    #[test]
    fn border_radius_individual_elliptical() {
        // `border-top-left-radius: 30px 15px` — один угол, разные rx/ry.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-top-left-radius: 30px 15px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_left_radius,   Length::Px(30.0));
        assert_eq!(s.border_top_left_radius_y, Length::Px(15.0));
        // Other corners untouched.
        assert_eq!(s.border_top_right_radius,   Length::Px(0.0));
        assert_eq!(s.border_top_right_radius_y, Length::Px(0.0));
    }

    #[test]
    fn border_radius_individual_circular_single_value() {
        // `border-top-right-radius: 8px` — одно значение → rx == ry.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-top-right-radius: 8px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_right_radius,   Length::Px(8.0));
        assert_eq!(s.border_top_right_radius_y, Length::Px(8.0));
    }

    #[test]
    fn border_radius_elliptical_four_slash_four() {
        // `border-radius: 1px 2px 3px 4px / 5px 6px 7px 8px` — полный вариант с 4+4.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-radius: 1px 2px 3px 4px / 5px 6px 7px 8px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_left_radius,        Length::Px(1.0));
        assert_eq!(s.border_top_right_radius,       Length::Px(2.0));
        assert_eq!(s.border_bottom_right_radius,    Length::Px(3.0));
        assert_eq!(s.border_bottom_left_radius,     Length::Px(4.0));
        assert_eq!(s.border_top_left_radius_y,      Length::Px(5.0));
        assert_eq!(s.border_top_right_radius_y,     Length::Px(6.0));
        assert_eq!(s.border_bottom_right_radius_y,  Length::Px(7.0));
        assert_eq!(s.border_bottom_left_radius_y,   Length::Px(8.0));
    }

    #[test]
    fn border_radius_percent_stored_deferred() {
        // `border-radius: 50%` — хранится как Length::Percent, резолвинг на paint-time.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { border-radius: 50%; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_left_radius,       Length::Percent(50.0));
        assert_eq!(s.border_top_left_radius_y,     Length::Percent(50.0));
        assert_eq!(s.border_top_right_radius,      Length::Percent(50.0));
        assert_eq!(s.border_top_right_radius_y,    Length::Percent(50.0));
        assert_eq!(s.border_bottom_right_radius,   Length::Percent(50.0));
        assert_eq!(s.border_bottom_right_radius_y, Length::Percent(50.0));
        assert_eq!(s.border_bottom_left_radius,    Length::Percent(50.0));
        assert_eq!(s.border_bottom_left_radius_y,  Length::Percent(50.0));
    }

    #[test]
    fn split_border_radius_slash_no_slash() {
        let (h, v) = split_border_radius_slash("10px 20px");
        assert_eq!(h, "10px 20px");
        assert!(v.is_none());
    }

    #[test]
    fn split_border_radius_slash_with_slash() {
        let (h, v) = split_border_radius_slash("10px 20px / 5px 15px");
        assert_eq!(h, "10px 20px");
        assert_eq!(v, Some("5px 15px"));
    }

    #[test]
    fn split_border_radius_slash_calc_not_split() {
        // `/` inside `calc()` must not be treated as shorthand separator.
        let (h, v) = split_border_radius_slash("calc(100%/2)");
        assert_eq!(h, "calc(100%/2)");
        assert!(v.is_none());
    }

    #[test]
    fn split_radius_pair_one_value() {
        let (rx, ry) = split_radius_pair("12px");
        assert_eq!(rx, "12px");
        assert!(ry.is_none());
    }

    #[test]
    fn split_radius_pair_two_values() {
        let (rx, ry) = split_radius_pair("30px 15px");
        assert_eq!(rx, "30px");
        assert_eq!(ry, Some("15px"));
    }
