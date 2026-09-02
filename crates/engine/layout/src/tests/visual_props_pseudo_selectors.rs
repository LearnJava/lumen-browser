use super::*;

    // ── outline (CSS Basic UI L4 §5) ────────────────────────────────────────

    #[test]
    fn outline_shorthand() {
        let root = lay("<p>x</p>", "p { outline: 3px dashed red; }");
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 3.0).abs() < 0.01);
        assert_eq!(p.style.outline_style, OutlineStyle::Dashed);
        match p.style.outline_color {
            OutlineColor::Color(c) => assert_eq!(c.r, 255),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn outline_individual_props() {
        let root = lay(
            "<p>x</p>",
            "p { outline-width: 5px; outline-style: solid; outline-color: blue; }",
        );
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 5.0).abs() < 0.01);
        assert_eq!(p.style.outline_style, OutlineStyle::Solid);
        match p.style.outline_color {
            OutlineColor::Color(c) => assert_eq!(c.b, 255),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn outline_offset_positive_and_negative() {
        let p_root = lay("<p>x</p>", "p { outline-offset: 10px; }");
        let p = first_element_child(&p_root);
        assert_eq!(p.style.outline_offset, Length::Px(10.0));

        let n_root = lay("<p>x</p>", "p { outline-offset: -3px; }");
        let n = first_element_child(&n_root);
        assert_eq!(n.style.outline_offset, Length::Px(-3.0));
    }

    #[test]
    fn outline_does_not_affect_box_width() {
        // Ключевое отличие от border: outline не занимает места в коробке.
        // Бокс с outline должен иметь ту же ширину/высоту, что без него.
        let with = lay("<p>x</p>", "p { outline: 10px solid red; }");
        let without = lay("<p>x</p>", "");

        let p_with = first_element_child(&with);
        let p_without = first_element_child(&without);
        assert!((p_with.rect.width - p_without.rect.width).abs() < 0.01,
            "outline не должен менять width: {} vs {}",
            p_with.rect.width, p_without.rect.width);
        assert!((p_with.rect.height - p_without.rect.height).abs() < 0.01);
    }

    #[test]
    fn outline_default_invisible() {
        // CSS Basic UI L4 §5: initial outline-style = none, outline-width = medium
        // (3px). Used-value outline-width = 0 при style=none, поэтому outline
        // невидим по умолчанию.
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 3.0).abs() < 0.01, "computed=medium");
        assert_eq!(p.style.outline_used_width(), 0.0, "used=0 при style=none");
        assert_eq!(p.style.outline_style, OutlineStyle::None);
        assert_eq!(p.style.outline_color, OutlineColor::Auto);
        assert_eq!(p.style.outline_offset, Length::Px(0.0));
    }

    #[test]
    fn outline_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { outline: 2px solid red; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(div.style.outline_used_width() > 0.0);
        assert_eq!(p.style.outline_style, OutlineStyle::None);
        assert_eq!(p.style.outline_used_width(), 0.0);
    }

    #[test]
    fn outline_width_line_width_keywords() {
        // CSS Basic UI L4 §5.2 — <line-width> = thin | medium | thick |
        // <length>. UA convention thin=1, medium=3, thick=5.
        let thin = lay("<p>x</p>", "p { outline: thin solid red; }");
        let p = first_element_child(&thin);
        assert!((p.style.outline_width - 1.0).abs() < 0.01);

        let med = lay("<p>x</p>", "p { outline: medium solid red; }");
        let p = first_element_child(&med);
        assert!((p.style.outline_width - 3.0).abs() < 0.01);

        let thick = lay("<p>x</p>", "p { outline: thick solid red; }");
        let p = first_element_child(&thick);
        assert!((p.style.outline_width - 5.0).abs() < 0.01);
    }

    #[test]
    fn outline_style_auto_keyword() {
        // CSS Basic UI L4 §5.3 — `auto` = UA-defined focus indicator. Хранится
        // отдельным variant-ом, чтобы UA-stylesheet `:focus-visible { outline:
        // auto }` отличался от явного `outline: solid` автора.
        let root = lay("<p>x</p>", "p { outline-style: auto; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.outline_style, OutlineStyle::Auto);
        assert!(p.style.outline_used_width() > 0.0, "auto делает outline видимым");
    }

    #[test]
    fn outline_color_auto_and_current_color() {
        // CSS Basic UI L4 §5.4 — `auto` = UA-defined contrast, `currentColor`
        // = вычисленный color элемента. Оба хранятся отдельными variant-ами.
        let auto_r = lay("<p>x</p>", "p { outline-color: auto; }");
        let p = first_element_child(&auto_r);
        assert_eq!(p.style.outline_color, OutlineColor::Auto);

        let cc_r = lay("<p>x</p>", "p { outline-color: currentColor; }");
        let p = first_element_child(&cc_r);
        assert_eq!(p.style.outline_color, OutlineColor::CurrentColor);
    }

    #[test]
    fn outline_shorthand_with_auto_style() {
        // `outline: auto` = style=auto, остальное initial.
        let root = lay("<p>x</p>", "p { outline: auto; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.outline_style, OutlineStyle::Auto);
        assert!((p.style.outline_width - 3.0).abs() < 0.01, "medium initial");
        assert_eq!(p.style.outline_color, OutlineColor::Auto);
    }

    #[test]
    fn outline_shorthand_resets_longhands() {
        // CSS Cascade L4 §3.1 — shorthand сбрасывает все longhand-а в
        // initial. Здесь сначала ставим конкретные значения, потом `outline`
        // должен затереть их к initial+token-set.
        let root = lay(
            "<p>x</p>",
            "p { outline-color: green; outline-offset: 10px; outline: 4px solid; }",
        );
        let p = first_element_child(&root);
        // shorthand сбросил color к Auto (initial) — токен solid 4px не
        // содержал цвета.
        assert_eq!(p.style.outline_color, OutlineColor::Auto);
        assert_eq!(p.style.outline_style, OutlineStyle::Solid);
        assert!((p.style.outline_width - 4.0).abs() < 0.01);
        // outline-offset — longhand, НЕ часть shorthand `outline`, не
        // сбрасывается (по spec). Проверяем, что offset сохранён.
        assert_eq!(p.style.outline_offset, Length::Px(10.0));
    }

    #[test]
    fn outline_used_width_zero_when_hidden_style_none() {
        // Used-value rule (CSS 2.1 §17.6.1 / Basic UI L4 §5.2): даже если
        // computed width задан явно, used = 0 при style=none.
        let root = lay("<p>x</p>", "p { outline-width: 20px; }");
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 20.0).abs() < 0.01, "computed=20");
        assert_eq!(p.style.outline_style, OutlineStyle::None);
        assert_eq!(p.style.outline_used_width(), 0.0, "used=0 при style=none");
    }

    // ── text-emphasis (CSS Text Decoration L4 §5) ───────────────────────────

    #[test]
    fn text_emphasis_default_none() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_emphasis_style, TextEmphasisStyle::None);
        assert!(matches!(p.style.text_emphasis_color, CssColor::CurrentColor), "initial = currentColor");
        assert_eq!(
            p.style.text_emphasis_position,
            TextEmphasisPosition::OverRight
        );
    }

    #[test]
    fn text_emphasis_style_symbol_filled_circle() {
        let root = lay("<p>x</p>", "p { text-emphasis-style: filled circle; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: true,
                shape: TextEmphasisShape::Circle
            }
        );
    }

    #[test]
    fn text_emphasis_style_only_fill_fallback_circle() {
        // Spec: shape по умолчанию = circle при horizontal writing mode.
        let root = lay("<p>x</p>", "p { text-emphasis-style: open; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: false,
                shape: TextEmphasisShape::Circle
            }
        );
    }

    #[test]
    fn text_emphasis_style_only_shape_fallback_filled() {
        let root = lay("<p>x</p>", "p { text-emphasis-style: sesame; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: true,
                shape: TextEmphasisShape::Sesame
            }
        );
    }

    #[test]
    fn text_emphasis_style_string() {
        let root = lay("<p>x</p>", "p { text-emphasis-style: \"★\"; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::String("★".to_string())
        );
    }

    #[test]
    fn text_emphasis_style_order_independent() {
        // Spec: `[ filled | open ] || [ ...shape... ]` — порядок любой.
        let r1 = lay(
            "<p>x</p>",
            "p { text-emphasis-style: triangle filled; }",
        );
        let p1 = first_element_child(&r1);
        let r2 = lay(
            "<p>x</p>",
            "p { text-emphasis-style: filled triangle; }",
        );
        let p2 = first_element_child(&r2);
        assert_eq!(p1.style.text_emphasis_style, p2.style.text_emphasis_style);
    }

    #[test]
    fn text_emphasis_color_explicit_and_currentcolor() {
        let r1 = lay("<p>x</p>", "p { text-emphasis-color: red; }");
        let p1 = first_element_child(&r1);
        assert!(matches!(p1.style.text_emphasis_color, CssColor::Rgba(Color { r: 255, .. })));

        // Override → currentColor сбрасывает в None.
        let r2 = lay(
            "<p>x</p>",
            "p { text-emphasis-color: red; text-emphasis-color: currentColor; }",
        );
        let p2 = first_element_child(&r2);
        assert!(matches!(p2.style.text_emphasis_color, CssColor::CurrentColor));
    }

    #[test]
    fn text_emphasis_position_grammar() {
        // [over | under] && [right | left]? — vertical обязателен, horizontal
        // опционален с default right.
        let r1 = lay("<p>x</p>", "p { text-emphasis-position: under left; }");
        let p1 = first_element_child(&r1);
        assert_eq!(
            p1.style.text_emphasis_position,
            TextEmphasisPosition::UnderLeft
        );

        let r2 = lay("<p>x</p>", "p { text-emphasis-position: left over; }");
        let p2 = first_element_child(&r2);
        assert_eq!(
            p2.style.text_emphasis_position,
            TextEmphasisPosition::OverLeft,
            "tokens are unordered"
        );

        // Только vertical — horizontal default right.
        let r3 = lay("<p>x</p>", "p { text-emphasis-position: under; }");
        let p3 = first_element_child(&r3);
        assert_eq!(
            p3.style.text_emphasis_position,
            TextEmphasisPosition::UnderRight
        );

        // Только horizontal — invalid (vertical обязателен).
        let r4 = lay("<p>x</p>", "p { text-emphasis-position: left; }");
        let p4 = first_element_child(&r4);
        assert_eq!(
            p4.style.text_emphasis_position,
            TextEmphasisPosition::OverRight,
            "invalid declaration ignored, initial"
        );
    }

    #[test]
    fn text_emphasis_inherited() {
        // CSS Text Decoration L4 §5 — все три text-emphasis-* longhand-а
        // inherited. Это ключевое отличие от text-decoration (там Phase 0
        // тоже inherit, но spec не-inherit с propagation).
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-emphasis: filled circle red; text-emphasis-position: under; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_emphasis_style, p.style.text_emphasis_style);
        assert_eq!(div.style.text_emphasis_color, p.style.text_emphasis_color);
        assert_eq!(
            div.style.text_emphasis_position,
            p.style.text_emphasis_position
        );
        assert_eq!(
            p.style.text_emphasis_position,
            TextEmphasisPosition::UnderRight
        );
    }

    #[test]
    fn text_emphasis_shorthand_style_plus_color() {
        let root = lay("<p>x</p>", "p { text-emphasis: filled dot blue; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: true,
                shape: TextEmphasisShape::Dot
            }
        );
        assert!(matches!(p.style.text_emphasis_color, CssColor::Rgba(Color { b: 255, .. })));
    }

    #[test]
    fn text_emphasis_shorthand_resets_longhands() {
        // Shorthand сбрасывает оба longhand-а в initial и потом применяет
        // токены. Position — отдельный longhand, не часть shorthand-а
        // (см. spec §5.6); поэтому сохраняется.
        let root = lay(
            "<p>x</p>",
            "p { text-emphasis-style: open triangle; \
                 text-emphasis-color: green; \
                 text-emphasis-position: under left; \
                 text-emphasis: red; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::None,
            "shorthand без style-токена → initial None"
        );
        assert!(matches!(p.style.text_emphasis_color, CssColor::Rgba(Color { r: 255, .. })));
        assert_eq!(
            p.style.text_emphasis_position,
            TextEmphasisPosition::UnderLeft,
            "position не входит в shorthand"
        );
    }

    #[test]
    fn text_emphasis_shorthand_none() {
        let root = lay("<p>x</p>", "p { text-emphasis: none; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_emphasis_style, TextEmphasisStyle::None);
        assert!(matches!(p.style.text_emphasis_color, CssColor::CurrentColor));
    }

    #[test]
    fn text_emphasis_shorthand_string_only() {
        let root = lay("<p>x</p>", "p { text-emphasis: \"♥\"; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::String("♥".to_string())
        );
    }

    #[test]
    fn text_emphasis_style_invalid_ignored() {
        // Невалидное значение (два shape) — declaration ignored, остаётся initial.
        let root = lay(
            "<p>x</p>",
            "p { text-emphasis-style: dot triangle; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_emphasis_style, TextEmphasisStyle::None);
    }

    // ── visibility (CSS Display L3 §4) ──────────────────────────────────────

    #[test]
    fn visibility_default_visible() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Visible);
    }

    #[test]
    fn visibility_hidden_parsed() {
        let root = lay("<p>x</p>", "p { visibility: hidden; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Hidden);
    }

    #[test]
    fn visibility_collapse_parsed() {
        let root = lay("<p>x</p>", "p { visibility: collapse; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Collapse);
    }

    #[test]
    fn visibility_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { visibility: hidden; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.visibility, Visibility::Hidden);
        assert_eq!(p.style.visibility, Visibility::Hidden);
    }

    #[test]
    fn visibility_visible_overrides_inherited_hidden() {
        // Дочерний может явно вернуть себя — это ключевая семантика CSS.
        let root = lay(
            "<div><p>x</p></div>",
            "div { visibility: hidden; } p { visibility: visible; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.visibility, Visibility::Hidden);
        assert_eq!(p.style.visibility, Visibility::Visible);
    }

    #[test]
    fn visibility_hidden_keeps_layout_height() {
        // В отличие от display:none, visibility:hidden оставляет коробку
        // в layout — она занимает место.
        let visible = lay("<p>x</p>", "");
        let hidden = lay("<p>x</p>", "p { visibility: hidden; }");
        let none = lay("<p>x</p>", "p { display: none; }");

        // Высота с hidden = высота visible.
        assert!((visible.rect.height - hidden.rect.height).abs() < 0.01,
            "visibility:hidden должен оставить высоту: visible={} hidden={}",
            visible.rect.height, hidden.rect.height);
        // Высота с display:none = 0 (бокс пропадает).
        assert!(none.rect.height < 0.1,
            "display:none должен убрать высоту: {}", none.rect.height);
    }

    // ── overflow (CSS Overflow L3) ──────────────────────────────────────────

    #[test]
    fn overflow_default_visible() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Visible);
        assert_eq!(p.style.overflow_y, Overflow::Visible);
    }

    #[test]
    fn overflow_shorthand_one_value() {
        let root = lay("<p>x</p>", "p { overflow: hidden; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Hidden);
        assert_eq!(p.style.overflow_y, Overflow::Hidden);
    }

    #[test]
    fn overflow_shorthand_two_values() {
        let root = lay("<p>x</p>", "p { overflow: scroll auto; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Scroll);
        assert_eq!(p.style.overflow_y, Overflow::Auto);
    }

    #[test]
    fn overflow_individual_x_y() {
        let root = lay(
            "<p>x</p>",
            "p { overflow-x: clip; overflow-y: scroll; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Clip);
        assert_eq!(p.style.overflow_y, Overflow::Scroll);
    }

    #[test]
    fn overflow_all_keywords() {
        for (kw, expected) in [
            ("visible", Overflow::Visible),
            ("hidden", Overflow::Hidden),
            ("clip", Overflow::Clip),
            ("scroll", Overflow::Scroll),
            ("auto", Overflow::Auto),
        ] {
            let css = format!("p {{ overflow: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            let p = first_element_child(&root);
            assert_eq!(p.style.overflow_x, expected, "kw = {kw}");
        }
    }

    #[test]
    fn overflow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { overflow: hidden; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.overflow_x, Overflow::Hidden);
        assert_eq!(p.style.overflow_x, Overflow::Visible);
    }

    // ── cursor (CSS UI L4 §8.1) ─────────────────────────────────────────────

    #[test]
    fn cursor_default_auto() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Auto);
    }

    #[test]
    fn cursor_keywords_parsed() {
        for (kw, expected) in [
            ("default", Cursor::Default),
            ("pointer", Cursor::Pointer),
            ("text", Cursor::Text),
            ("wait", Cursor::Wait),
            ("move", Cursor::Move),
            ("not-allowed", Cursor::NotAllowed),
            ("grab", Cursor::Grab),
            ("zoom-in", Cursor::ZoomIn),
            ("nesw-resize", Cursor::NeswResize),
        ] {
            let css = format!("p {{ cursor: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            let p = first_element_child(&root);
            assert_eq!(p.style.cursor, expected, "kw = {kw}");
        }
    }

    #[test]
    fn cursor_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { cursor: pointer; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.cursor, Cursor::Pointer);
        assert_eq!(p.style.cursor, Cursor::Pointer);
    }

    #[test]
    fn cursor_url_fallback_uses_keyword() {
        // CSS UI: `cursor: url(...) default` — берём последний keyword.
        // Phase 0 url() игнорируется.
        let root = lay(
            "<p>x</p>",
            "p { cursor: url(custom.png), pointer; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Pointer);
    }

    #[test]
    fn cursor_unknown_keeps_inherited() {
        let root = lay("<p>x</p>", "p { cursor: nonsense; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Auto);
    }

    // ── box-shadow (CSS Backgrounds L3 §4.6) ────────────────────────────────

    #[test]
    fn box_shadow_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.box_shadow.is_empty());
    }

    #[test]
    fn box_shadow_two_lengths() {
        // offset-x, offset-y без blur/spread/color.
        let root = lay("<p>x</p>", "p { box-shadow: 5px 10px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 1);
        let s = &p.style.box_shadow[0];
        assert!((s.offset_x - 5.0).abs() < 0.01);
        assert!((s.offset_y - 10.0).abs() < 0.01);
        assert_eq!(s.blur, 0.0);
        assert_eq!(s.spread, 0.0);
        assert!(!s.inset);
        assert!(s.color.is_none());
    }

    #[test]
    fn box_shadow_with_blur_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 2px 3px 4px red; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.blur, 4.0);
        assert_eq!(s.color.unwrap().r, 255);
    }

    #[test]
    fn box_shadow_with_blur_spread_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 1px 2px 3px 4px blue; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.spread, 4.0);
        assert_eq!(s.color.unwrap().b, 255);
    }

    #[test]
    fn box_shadow_inset() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: inset 2px 2px 5px black; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert!(s.inset);
        assert!((s.offset_x - 2.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_multiple_comma_separated() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 1px 1px red, 2px 2px blue, inset 3px 3px black; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 3);
        assert_eq!(p.style.box_shadow[0].color.unwrap().r, 255);
        assert_eq!(p.style.box_shadow[1].color.unwrap().b, 255);
        assert!(p.style.box_shadow[2].inset);
    }

    #[test]
    fn box_shadow_color_with_internal_commas() {
        // rgba(...) содержит запятые внутри — split_top_level_commas
        // не должен порвать это на куски.
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 2px 2px 4px rgba(0, 0, 0, 0.5); }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 1);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.color.unwrap().a, 128);
    }

    #[test]
    fn box_shadow_none_clears() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-shadow: 1px 1px black; } p { box-shadow: none; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // box-shadow не наследуется в любом случае; но `none` должно
        // явно сбросить.
        assert_eq!(div.style.box_shadow.len(), 1);
        assert!(p.style.box_shadow.is_empty());
    }

    #[test]
    fn box_shadow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-shadow: 2px 2px black; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.box_shadow.len(), 1);
        assert!(p.style.box_shadow.is_empty());
    }

    // ── text-shadow (CSS Text Decoration L3 §4) ─────────────────────────────

    #[test]
    fn text_shadow_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.text_shadow.is_empty());
    }

    #[test]
    fn text_shadow_two_lengths() {
        let root = lay("<p>x</p>", "p { text-shadow: 2px 3px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 1);
        let s = &p.style.text_shadow[0];
        assert!((s.offset_x - 2.0).abs() < 0.01);
        assert!((s.offset_y - 3.0).abs() < 0.01);
        assert_eq!(s.blur, 0.0);
        assert!(s.color.is_none());
    }

    #[test]
    fn text_shadow_with_blur_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 1px 2px 3px red; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.text_shadow[0];
        assert_eq!(s.blur, 3.0);
        assert_eq!(s.color.unwrap().r, 255);
    }

    #[test]
    fn text_shadow_multiple() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 1px 1px red, 2px 2px blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 2);
        assert_eq!(p.style.text_shadow[0].color.unwrap().r, 255);
        assert_eq!(p.style.text_shadow[1].color.unwrap().b, 255);
    }

    #[test]
    fn text_shadow_inherited() {
        // В отличие от box-shadow, text-shadow ДОЛЖЕН наследоваться.
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-shadow: 1px 1px black; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_shadow.len(), 1);
        assert_eq!(p.style.text_shadow.len(), 1, "text-shadow должен наследоваться");
    }

    #[test]
    fn text_shadow_none_overrides_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-shadow: 1px 1px black; } p { text-shadow: none; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_shadow.len(), 1);
        assert!(p.style.text_shadow.is_empty(), "p должен сбросить inherited");
    }

    #[test]
    fn text_shadow_color_with_internal_commas() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 2px 2px 4px rgba(0, 0, 0, 0.5); }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 1);
        assert_eq!(p.style.text_shadow[0].color.unwrap().a, 128);
    }

    // ── border-radius (CSS Backgrounds L3 §5) ───────────────────────────────

    #[test]
    fn border_radius_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(0.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(0.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(0.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_shorthand_one_value() {
        let root = lay("<p>x</p>", "p { border-radius: 8px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(8.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(8.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(8.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(8.0));
    }

    #[test]
    fn border_radius_shorthand_two_values() {
        // 2 значения: TL/BR одинаковы, TR/BL одинаковы.
        let root = lay("<p>x</p>", "p { border-radius: 4px 12px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(4.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(12.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(4.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(12.0));
    }

    #[test]
    fn border_radius_shorthand_four_values() {
        let root = lay(
            "<p>x</p>",
            "p { border-radius: 1px 2px 3px 4px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(1.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(2.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(3.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(4.0));
    }

    #[test]
    fn border_radius_individual_corners() {
        let root = lay(
            "<p>x</p>",
            "p { border-top-left-radius: 5px; border-bottom-right-radius: 10px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(5.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(0.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(10.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_em_resolves() {
        // 1em при default fs 16 = 16px; em резолвится сразу в Px.
        let root = lay("<p>x</p>", "p { border-radius: 1em; }");
        let p = first_element_child(&root);
        assert!(matches!(p.style.border_top_left_radius, Length::Px(v) if (v - 16.0).abs() < 0.01));
    }

    #[test]
    fn border_radius_elliptical_takes_first_part() {
        // `5px / 10px` (elliptical) — Phase 0 берёт только горизонтальный
        // (первый токен до `/`).
        let root = lay(
            "<p>x</p>",
            "p { border-radius: 5px / 10px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(5.0));
    }

    #[test]
    fn border_radius_negative_clamped_to_zero() {
        let root = lay("<p>x</p>", "p { border-radius: -10px; }");
        let p = first_element_child(&root);
        // Невалидное (отрицательное) — clamp до 0 в parse_radius_length.
        assert_eq!(p.style.border_top_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { border-radius: 5px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.border_top_left_radius, Length::Px(5.0));
        assert_eq!(p.style.border_top_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_percent_stored_as_percent() {
        // `border-radius: 50%` резолвинг откладывается до paint-time (known box dims).
        let root = lay("<p>x</p>", "p { border-radius: 50%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius,     Length::Percent(50.0));
        assert_eq!(p.style.border_top_right_radius,    Length::Percent(50.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Percent(50.0));
        assert_eq!(p.style.border_bottom_left_radius,  Length::Percent(50.0));
    }

    // ── text-overflow (CSS UI L4 §10.1) ─────────────────────────────────────

    #[test]
    fn text_overflow_default_clip() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_ellipsis_parsed() {
        let root = lay("<p>x</p>", "p { text-overflow: ellipsis; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Ellipsis);
    }

    #[test]
    fn text_overflow_clip_explicit() {
        let root = lay("<p>x</p>", "p { text-overflow: clip; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-overflow: ellipsis; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_unknown_keeps_default() {
        let root = lay("<p>x</p>", "p { text-overflow: nonsense; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    /// overflow:hidden + text-overflow:ellipsis + nowrap → длинный текст
    /// усекается, последний символ фрагмента — «…».
    #[test]
    fn text_overflow_ellipsis_truncates_overflowing_line() {
        // Fixed8: 8 px/char. "Hello World" = 11 chars = 88 px. Box = 64 px.
        // budget = 64 - 8(«…») = 56 px → влезает 7 chars "Hello W".
        // overflow и text-overflow — на одном элементе (p), чей стиль
        // наследует InlineRun.
        let root = lay_measured(
            "<p>Hello World</p>",
            "p { width: 64px; overflow: hidden; \
               white-space: nowrap; text-overflow: ellipsis; }",
            800.0,
        );
        let p = first_element_child(&root);
        let run = &p.children[0];
        let crate::BoxKind::InlineRun { lines, .. } = &run.kind else {
            panic!("expected InlineRun");
        };
        let line = &lines[0];
        assert_eq!(line.len(), 1, "один фрагмент после усечения");
        assert!(
            line[0].text.ends_with('\u{2026}'),
            "текст должен оканчиваться на «…», got {:?}",
            line[0].text
        );
        assert!(
            line[0].width <= 64.0,
            "ширина фрагмента должна влезать в контейнер: {}",
            line[0].width
        );
    }

    /// overflow:visible + text-overflow:ellipsis → усечения нет
    /// (spec: text-overflow не действует без overflow clip).
    #[test]
    fn text_overflow_ellipsis_no_effect_without_overflow_clip() {
        let root = lay_measured(
            "<p>Hello World</p>",
            "p { width: 64px; overflow: visible; \
               white-space: nowrap; text-overflow: ellipsis; }",
            800.0,
        );
        let p = first_element_child(&root);
        let run = &p.children[0];
        let crate::BoxKind::InlineRun { lines, .. } = &run.kind else {
            panic!("expected InlineRun");
        };
        let line = &lines[0];
        let text: String = line.iter().map(|f| f.text.as_str()).collect();
        assert!(
            !text.contains('\u{2026}'),
            "без overflow clip усечения быть не должно, got {text:?}"
        );
    }

    /// text-overflow:clip (default) → даже при overflow:hidden текст не усекается
    /// с «…»; clip происходит на уровне paint, не layout.
    #[test]
    fn text_overflow_clip_no_ellipsis() {
        let root = lay_measured(
            "<p>Hello World</p>",
            "p { width: 64px; overflow: hidden; \
               white-space: nowrap; text-overflow: clip; }",
            800.0,
        );
        let p = first_element_child(&root);
        let run = &p.children[0];
        let crate::BoxKind::InlineRun { lines, .. } = &run.kind else {
            panic!("expected InlineRun");
        };
        let line = &lines[0];
        let text: String = line.iter().map(|f| f.text.as_str()).collect();
        assert!(
            !text.contains('\u{2026}'),
            "text-overflow:clip не должен добавлять «…», got {text:?}"
        );
    }

    // ── selector matching: back-tracking edge cases ─────────────────────────

    /// `div div p` — двойной descendant. Должен матчить, когда есть два
    /// уровня div выше p. Без back-tracking тоже работает (greedy от p вверх
    /// находит ближайший div, дальше выше — другой div) — sanity check.
    #[test]
    fn selector_double_descendant_works() {
        let root = lay(
            "<div><div><p>x</p></div></div>",
            "div div p { color: red; }",
        );
        // Находим p глубоко.
        fn find_p<'a>(b: &'a LayoutBox, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
            if let lumen_dom::NodeData::Element { name, .. } = &doc.get(b.node).data
                && name.local == "p"
            {
                return Some(b);
            }
            for c in &b.children {
                if let Some(f) = find_p(c, doc) {
                    return Some(f);
                }
            }
            None
        }
        let doc = lumen_html_parser::parse("<div><div><p>x</p></div></div>");
        let p = find_p(&root, &doc).unwrap();
        assert_eq!(p.style.color.r, 255);
    }

    /// `a a span` с двумя `<a>`-предками — должен матчить через compute_style
    /// (LayoutBox-фасад не подходит, т.к. <a> inline и весь контент сплавлен
    /// в InlineRun-ы; проверяем напрямую).
    #[test]
    fn selector_nested_same_tag_descendants() {
        // HTML5 parser re-normalizes nested <a> tags (inner <a> closes outer).
        // Use <div><a><div><a><span>x</span></a></div></a></div> which produces
        // two independent a-ancestors of span.
        let doc = lumen_html_parser::parse(r#"<div><a><div><a><span>x</span></a></div></a></div>"#);
        let span_id = find_first_by_tag(&doc, doc.root(), "span").expect("span");
        let style = crate::style::compute_style(
            &doc,
            span_id,
            &lumen_css_parser::parse("a a span { color: red; }"),
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(style.color.r, 255);
    }

    /// Чисто back-tracking-зависимый случай через compute_style. Дерево:
    /// `<div><a class="x"></a><a></a><a></a><span>X</span></div>`. Селектор:
    /// `.x + a ~ span`. Greedy от span: `~ span` находит span; `+ a` — это
    /// его прямой предыдущий sibling = третий `<a>`. Затем `.x` — sibling до
    /// него = второй `<a>`, который не имеет класс `.x` → fail. Backtracking
    /// перебирает `~ span` кандидатов: span сам = node → нет; либо для
    /// later-sibling combinator берёт КАЖДЫЙ earlier sibling. С back-tracking
    /// найдётся: `~ span` candidate = span (нет), но потом для `+ a` мы
    /// фиксируемся на втором `<a>` (через рекурсию), и первый `<a>` (`.x`)
    /// удовлетворяет `.x`.
    #[test]
    fn selector_backtracking_pathological_sibling() {
        let doc = lumen_html_parser::parse(
            r#"<div><a class="x">A</a><a>B</a><a>C</a><span>SPAN</span></div>"#,
        );
        let span_id = find_first_by_tag(&doc, doc.root(), "span").expect("span");
        let sheet = lumen_css_parser::parse(".x + a ~ span { color: red; }");
        let style = crate::style::compute_style(
            &doc,
            span_id,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(
            style.color.r, 255,
            ".x + a ~ span должен сматчить span с back-tracking"
        );
    }

    fn find_first_by_tag(
        doc: &lumen_dom::Document,
        id: lumen_dom::NodeId,
        tag: &str,
    ) -> Option<lumen_dom::NodeId> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(id).data
            && name.local == tag
        {
            return Some(id);
        }
        for c in &doc.get(id).children {
            if let Some(f) = find_first_by_tag(doc, *c, tag) {
                return Some(f);
            }
        }
        None
    }

    // ── font-variant-caps (CSS Fonts L4 §6.2) ───────────────────────────────

    #[test]
    fn font_variant_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::Normal);
    }

    #[test]
    fn font_variant_small_caps_parsed() {
        let root = lay("<p>x</p>", "p { font-variant: small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::SmallCaps);
    }

    #[test]
    fn font_variant_caps_full_value_set_parsed() {
        // CSS Fonts L4 §6.2 — longhand принимает все шесть не-initial значений.
        for (css, want) in [
            ("small-caps", FontVariantCaps::SmallCaps),
            ("all-small-caps", FontVariantCaps::AllSmallCaps),
            ("petite-caps", FontVariantCaps::PetiteCaps),
            ("all-petite-caps", FontVariantCaps::AllPetiteCaps),
            ("unicase", FontVariantCaps::Unicase),
            ("titling-caps", FontVariantCaps::TitlingCaps),
            ("normal", FontVariantCaps::Normal),
        ] {
            let root = lay("<p>x</p>", &format!("p {{ font-variant-caps: {css}; }}"));
            let p = first_element_child(&root);
            assert_eq!(p.style.font_variant_caps, want, "font-variant-caps: {css}");
        }
    }

    #[test]
    fn font_variant_caps_invalid_keyword_ignored() {
        // Невалидное значение longhand-а не отменяет унаследованное
        // (CSS Cascade L4 §4.4 — declaration отбрасывается).
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant-caps: small-caps; } p { font-variant-caps: nope; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::SmallCaps);
    }

    #[test]
    fn font_variant_shorthand_picks_caps_component() {
        let root = lay("<p>x</p>", "p { font-variant: all-small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::AllSmallCaps);
    }

    #[test]
    fn font_variant_shorthand_resets_caps_to_initial() {
        // CSS Cascade L4 §3.1: shorthand выставляет ВСЕ свои longhand-ы.
        // `font-variant: common-ligatures` (лигатурная компонента, у нас не
        // реализована) обязан вернуть caps в initial, а не сохранить
        // унаследованное small-caps.
        for css in ["common-ligatures", "none"] {
            let root = lay(
                "<div><p>x</p></div>",
                &format!("div {{ font-variant: small-caps; }} p {{ font-variant: {css}; }}"),
            );
            let p = first_element_child(first_element_child(&root));
            assert_eq!(p.style.font_variant_caps, FontVariantCaps::Normal, "font-variant: {css}");
        }
    }

    #[test]
    fn font_variant_normal_keyword_resets() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; } p { font-variant: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_variant_caps, FontVariantCaps::SmallCaps);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::Normal);
    }

    #[test]
    fn font_variant_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::SmallCaps);
    }

    #[test]
    fn font_variant_caps_synthesized_into_frags() {
        // End-to-end: small-caps доезжает до фрагментов — строчные подняты в
        // верхний регистр и нарисованы уменьшенным кеглем.
        let root = lay_measured("<p>Hi</p>", "p { font-variant-caps: small-caps; font-size: 20px; }", 400.0);
        let run = first_inline_run(first_element_child(&root));
        let BoxKind::InlineRun { lines, .. } = &run.kind else { panic!("expected InlineRun") };
        let frags: Vec<(&str, f32)> = lines
            .iter()
            .flatten()
            .map(|f| (f.text.as_str(), f.style.font_size))
            .collect();
        assert_eq!(frags, vec![("H", 20.0), ("I", 16.0)]);
    }

    #[test]
    fn font_variant_caps_does_not_break_word_at_case_boundary() {
        // Разрез «H|ELLO» проходит внутри слова: перенос по нему запрещён,
        // иначе узкий контейнер разорвал бы слово пополам.
        let root = lay_measured(
            "<p>Hello</p>",
            "p { font-variant-caps: small-caps; font-size: 20px; }",
            24.0,
        );
        let run = first_inline_run(first_element_child(&root));
        let BoxKind::InlineRun { lines, .. } = &run.kind else { panic!("expected InlineRun") };
        let non_empty = lines.iter().filter(|l| !l.is_empty()).count();
        assert_eq!(non_empty, 1, "слово разорвано на {non_empty} строк: {lines:?}");
    }

    // ── font-stretch (CSS Fonts L4 §2.5) ────────────────────────────────────

    #[test]
    fn font_stretch_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch, FontStretch::NORMAL);
    }

    #[test]
    fn font_stretch_keyword_condensed() {
        let root = lay("<p>x</p>", "p { font-stretch: condensed; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 750);
    }

    #[test]
    fn font_stretch_keyword_semi_expanded_fractional() {
        // 112.5% — дробный keyword проверяет, что хранение в десятых не теряет точность.
        let root = lay("<p>x</p>", "p { font-stretch: semi-expanded; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 1125);
    }

    #[test]
    fn font_stretch_percentage_value() {
        let root = lay("<p>x</p>", "p { font-stretch: 80%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 800);
    }

    #[test]
    fn font_stretch_percentage_clamped() {
        // Spec разрешает значения вне [50%, 200%], но Phase 0 их клампит —
        // экстремальные значения бесполезны и могут переполнить u16.
        let root = lay("<p>x</p>", "p { font-stretch: 10%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 500);

        let root = lay("<p>x</p>", "p { font-stretch: 300%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 2000);
    }

    #[test]
    fn font_stretch_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-stretch: expanded; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_stretch.0, 1250);
        assert_eq!(div.style.font_stretch.0, 1250);
    }

    #[test]
    fn font_stretch_normal_resets_inheritance() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-stretch: condensed; } p { font-stretch: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_stretch.0, 750);
        assert_eq!(p.style.font_stretch, FontStretch::NORMAL);
    }

    #[test]
    fn font_stretch_as_percent_matches_os2_width_class_scale() {
        // `as_percent` — единицы matcher-а (`FaceRecord::stretch`,
        // `usWidthClass`). Дробные keyword-ы округляются к ближайшему целому:
        // шкала usWidthClass целочисленная, полуступеней у неё нет.
        assert_eq!(FontStretch::NORMAL.as_percent(), 100);
        assert_eq!(FontStretch(500).as_percent(), 50); // ultra-condensed
        assert_eq!(FontStretch(750).as_percent(), 75); // condensed
        assert_eq!(FontStretch(875).as_percent(), 88); // semi-condensed 87.5%
        assert_eq!(FontStretch(1125).as_percent(), 113); // semi-expanded 112.5%
        assert_eq!(FontStretch(2000).as_percent(), 200); // ultra-expanded
        // Округление к ближайшему, а не вверх: 80.4% → 80.
        assert_eq!(FontStretch(804).as_percent(), 80);
    }

    #[test]
    fn font_stretch_parse_keyword_and_percentage() {
        assert_eq!(FontStretch::parse("condensed"), Some(FontStretch(750)));
        assert_eq!(FontStretch::parse("80%"), Some(FontStretch(800)));
        // Диапазон из двух значений (синтаксис @font-face) → первое значение.
        assert_eq!(FontStretch::parse("75% 125%"), Some(FontStretch(750)));
        // Кламп в [50%, 200%] — держит значение в u16 без переполнения.
        assert_eq!(FontStretch::parse("10%"), Some(FontStretch(500)));
        assert_eq!(FontStretch::parse("300%"), Some(FontStretch(2000)));
        assert_eq!(FontStretch::parse("nonsense"), None);
        assert_eq!(FontStretch::parse(""), None);
    }

    // ── accent-color (CSS UI L4 §6.1) ──────────────────────────────────────

    #[test]
    fn accent_color_default_none() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.accent_color.is_none());
    }

    #[test]
    fn accent_color_named() {
        let root = lay("<p>x</p>", "p { accent-color: red; }");
        let p = first_element_child(&root);
        let c = p.style.accent_color.expect("accent set");
        assert_eq!((c.r, c.g, c.b, c.a), (255, 0, 0, 255));
    }

    #[test]
    fn accent_color_hex() {
        let root = lay("<p>x</p>", "p { accent-color: #4080ff; }");
        let p = first_element_child(&root);
        let c = p.style.accent_color.expect("accent set");
        assert_eq!((c.r, c.g, c.b), (0x40, 0x80, 0xff));
    }

    #[test]
    fn accent_color_auto_resets_inheritance() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: blue; } p { accent-color: auto; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(div.style.accent_color.is_some());
        assert!(p.style.accent_color.is_none());
    }

    #[test]
    fn accent_color_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: rgb(10, 20, 30); }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        let dc = div.style.accent_color.expect("div accent");
        let pc = p.style.accent_color.expect("p inherits accent");
        assert_eq!((dc.r, dc.g, dc.b), (10, 20, 30));
        assert_eq!((pc.r, pc.g, pc.b), (10, 20, 30));
    }

    #[test]
    fn accent_color_invalid_ignored() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: red; } p { accent-color: notacolor; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // Невалидное значение игнорируется → p наследует от div.
        assert_eq!(div.style.accent_color, p.style.accent_color);
        assert!(p.style.accent_color.is_some());
    }

    // ── :has() (CSS Selectors L4 §17.2) ─────────────────────────────────────

    /// `div:has(p)` — div, содержащий p в поддереве (через span).
    #[test]
    fn has_implicit_descendant_matches() {
        let root = lay(
            "<div><span><p>x</p></span></div><div><span>nope</span></div>",
            "div:has(p) { color: red; }",
        );
        let blocks: Vec<_> = root.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks[0].style.color.r, 255, "первый div должен сматчить");
        assert_eq!(blocks[1].style.color.r, 0, "второй div без p — нет");
    }

    /// `div:has(> .child)` — direct child only.
    #[test]
    fn has_child_combinator() {
        let root = lay(
            r#"<div><p class="child">x</p></div><div><span><p class="child">x</p></span></div>"#,
            "div:has(> .child) { color: red; }",
        );
        let blocks: Vec<_> = root.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks[0].style.color.r, 255);
        assert_eq!(blocks[1].style.color.r, 0);
    }

    /// `h2:has(+ p)` — h2 followed by p. Через compute_style напрямую.
    #[test]
    fn has_next_sibling() {
        let doc = lumen_html_parser::parse("<div><h2>A</h2><p>x</p></div><div><h2>B</h2></div>");
        let sheet = lumen_css_parser::parse("h2:has(+ p) { color: red; }");
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let div1 = doc.get(body).children[0];
        let h2_a = doc.get(div1).children[0];
        let div2 = doc.get(body).children[1];
        let h2_b = doc.get(div2).children[0];
        let style_a = crate::style::compute_style(
            &doc, h2_a, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let style_b = crate::style::compute_style(
            &doc, h2_b, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(style_a.color.r, 255, "h2 + p должен сматчить");
        assert_eq!(style_b.color.r, 0, "h2 без p после — нет");
    }

    /// `:has()` НЕ матчит сам node — descendants only.
    #[test]
    fn has_does_not_match_self() {
        let root = lay(
            "<p>x</p>",
            "p:has(p) { color: red; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 0);
    }

    /// `:has(.a, .b)` — список (OR).
    #[test]
    fn has_list_or_match() {
        let root = lay(
            r#"<div><span class="b">x</span></div>"#,
            ":has(.a, .b) { color: red; }",
        );
        let div = first_element_child(&root);
        assert_eq!(div.style.color.r, 255);
    }

    // ── direction (CSS Writing Modes L3 §2.1) ──────────────────────────────

    #[test]
    fn direction_default_ltr() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Ltr);
    }

    #[test]
    fn direction_rtl_applied() {
        let root = lay("<p>x</p>", "p { direction: rtl; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_case_insensitive() {
        // Keyword-ы CSS property values — ASCII case-insensitive
        // (Values L4 §2.4). Документ может прийти с `RTL` или `Rtl`.
        let root = lay("<p>x</p>", "p { direction: RTL; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_inherited() {
        // direction распространяется от родителя — основа bidi-каскада.
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.direction, Direction::Rtl);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_child_overrides_inherited() {
        // Inheritable, но потомок может явно переопределить — обратно на ltr.
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; } p { direction: ltr; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.direction, Direction::Rtl);
        assert_eq!(p.style.direction, Direction::Ltr);
    }

    #[test]
    fn direction_invalid_keeps_inherited() {
        // Невалидное значение — сохраняем inherited (по CSS error recovery
        // правилу: invalid declaration → ignore).
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; } p { direction: vertical; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    /// text-align: start в RTL → правый край (start = right для RTL).
    /// "ab" = 16px в контейнере 100px; правый край = 100-16 = 84px.
    #[test]
    fn text_align_start_rtl_flushes_right() {
        let root = lay_measured(
            "<p>ab</p>",
            "p { direction: rtl; text-align: start; }",
            100.0,
        );
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            // В RTL-зеркале первый фрагмент в LTR-порядке переходит на правую сторону.
            // Последний фраг должен оканчиваться у content_width=100.
            let last = lines[0].last().unwrap();
            let right_edge = last.x + last.width;
            assert!(
                (right_edge - 100.0).abs() < 0.5,
                "expected right edge ≈ 100, got {right_edge}",
            );
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: end в RTL → левый край (end = left для RTL).
    /// "ab" = 16px в контейнере 100px; левый край первого фрагмента = 0.
    #[test]
    fn text_align_end_rtl_flushes_left() {
        let root = lay_measured(
            "<p>ab</p>",
            "p { direction: rtl; text-align: end; }",
            100.0,
        );
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            // В RTL + left align первый (левый) фраг начинается с x=0.
            let min_x = lines[0].iter().map(|f| f.x).fold(f32::INFINITY, f32::min);
            assert!(
                min_x.abs() < 0.5,
                "expected leftmost frag x ≈ 0, got {min_x}",
            );
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: start в LTR → левый край (start = left для LTR, нет смещения).
    #[test]
    fn text_align_start_ltr_flushes_left() {
        let root = lay_measured(
            "<p>ab</p>",
            "p { direction: ltr; text-align: start; }",
            100.0,
        );
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            assert!((lines[0][0].x - 0.0).abs() < 0.01, "expected x=0, got {}", lines[0][0].x);
        } else {
            panic!("expected InlineRun");
        }
    }

    // ── CSS Containment L3 enforcement ──────────────────────────────────────

    /// contain:size → auto height = 0 (children don't contribute).
    #[test]
    fn contain_size_suppresses_auto_height() {
        let root = lay_measured(
            "<div><p>child</p></div>",
            "div { contain: size; } p { height: 50px; }",
            200.0,
        );
        let div = first_element_child(&root);
        // Explicit p height = 50px, but div has contain:size → div height = 0
        // (only padding+border, which are both 0 here).
        assert_eq!(div.rect.height, 0.0, "contain:size → auto height must be 0, got {}", div.rect.height);
    }

    /// contain:size with explicit height — explicit wins, children still don't contribute.
    #[test]
    fn contain_size_explicit_height_wins() {
        let root = lay_measured(
            "<div><p>child</p></div>",
            "div { contain: size; height: 80px; } p { height: 100px; }",
            200.0,
        );
        let div = first_element_child(&root);
        assert!((div.rect.height - 80.0).abs() < 0.5, "contain:size with explicit height=80, got {}", div.rect.height);
    }

    /// contain:layout parses and stores correctly.
    #[test]
    fn contain_layout_stores_flag() {
        let root = lay("<div></div>", "div { contain: layout; }");
        let div = first_element_child(&root);
        assert!(
            div.style.contain.0 & ContainFlags::LAYOUT.0 != 0,
            "contain:layout flag not set"
        );
    }

    /// contain:strict = size + layout + style + paint → auto height = 0.
    #[test]
    fn contain_strict_suppresses_auto_height() {
        let root = lay_measured(
            "<div><p>text</p></div>",
            "div { contain: strict; } p { height: 60px; }",
            200.0,
        );
        let div = first_element_child(&root);
        assert_eq!(div.rect.height, 0.0, "contain:strict → auto height must be 0, got {}", div.rect.height);
    }

    // ── CSS Container Style Queries (Phase 0) ──────────────────────────────

    pub(crate) fn style_ctx(props: &[(&str, &str)]) -> crate::ContainerContext {
        style_ctx_with_style_props(props, &[])
    }

    pub(crate) fn style_ctx_with_style_props(
        custom_props: &[(&str, &str)],
        style_props: &[(&str, &str)],
    ) -> crate::ContainerContext {
        let mut custom = std::collections::HashMap::new();
        for (k, v) in custom_props {
            custom.insert(k.to_string(), v.to_string());
        }
        let mut style = std::collections::HashMap::new();
        for (k, v) in style_props {
            style.insert(k.to_string(), v.to_string());
        }
        crate::ContainerContext {
            width: 200.0,
            height: Some(100.0),
            names: vec![],
            custom_props: custom.into(),
            style_props: style,
            font_size: 16.0,
            viewport: lumen_core::Size::new(1024.0, 768.0),
            own_containing_block_height: 100.0,
        }
    }

    #[test]
    fn style_query_with_value_true() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_with_value_false() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(!crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_with_value_missing() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_boolean_true() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_boolean_false() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_with_extra_spaces() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme:  dark )", &ctx));
    }

    #[test]
    fn style_query_not() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(crate::evaluate_container_condition("not style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_combined_with_size() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("(min-width: 150px) and style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_combined_with_size_false() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(!crate::evaluate_container_condition("(min-width: 150px) and style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_unset_returns_false() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(width: 100px)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_matches() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display: flex)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(!crate::evaluate_container_condition("style(display: block)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_keyword_case_insensitive() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display: FLEX)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_boolean_form_true_when_set() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_boolean_form_false_when_unset() {
        let ctx = style_ctx_with_style_props(&[], &[]);
        assert!(!crate::evaluate_container_condition("style(display)", &ctx));
    }

    #[test]
    fn style_query_color_keyword_matches_computed_rgb() {
        // Container's computed style is already serialized as `rgb(...)`
        // (getComputedStyle form); the query uses the author's keyword.
        let ctx = style_ctx_with_style_props(&[], &[("color", "rgb(255, 0, 0)")]);
        assert!(crate::evaluate_container_condition("style(color: red)", &ctx));
    }

    #[test]
    fn style_query_color_hex_matches_computed_rgb() {
        let ctx = style_ctx_with_style_props(&[], &[("background-color", "rgb(0, 0, 255)")]);
        assert!(crate::evaluate_container_condition(
            "style(background-color: #0000ff)",
            &ctx
        ));
    }

    #[test]
    fn style_query_color_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("color", "rgb(255, 0, 0)")]);
        assert!(!crate::evaluate_container_condition("style(color: blue)", &ctx));
    }

    #[test]
    fn style_query_non_color_value_mismatch_still_returns_false() {
        // A non-color, non-matching value must not be coerced into matching.
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(!crate::evaluate_container_condition("style(display: grid)", &ctx));
    }

    #[test]
    fn style_query_length_pt_matches_computed_px() {
        // Container's computed style is serialized in px; the query uses `pt`.
        let ctx = style_ctx_with_style_props(&[], &[("border-top-width", "2.6667px")]);
        assert!(crate::evaluate_container_condition(
            "style(border-top-width: 2pt)",
            &ctx
        ));
    }

    #[test]
    fn style_query_length_in_matches_computed_px() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "96px")]);
        assert!(crate::evaluate_container_condition("style(width: 1in)", &ctx));
    }

    #[test]
    fn style_query_length_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "96px")]);
        assert!(!crate::evaluate_container_condition("style(width: 2in)", &ctx));
    }

    #[test]
    fn style_query_em_matches_computed_px() {
        // `style_ctx` has font_size: 16.0 → `1em` resolves to 16px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "16px")]);
        assert!(crate::evaluate_container_condition("style(width: 1em)", &ctx));
    }

    #[test]
    fn style_query_em_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "16px")]);
        assert!(!crate::evaluate_container_condition("style(width: 2em)", &ctx));
    }

    #[test]
    fn style_query_percent_matches_computed_px() {
        // `style_ctx` has width: 200.0 → `50%` resolves to 100px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "100px")]);
        assert!(crate::evaluate_container_condition("style(width: 50%)", &ctx));
    }

    #[test]
    fn style_query_line_height_percent_uses_font_size_basis_not_width() {
        // `style_ctx` has font_size: 16.0, width: 200.0. `line-height: 150%`
        // must resolve against font-size (24px), not width (300px).
        let ctx = style_ctx_with_style_props(&[], &[("line-height", "24px")]);
        assert!(crate::evaluate_container_condition("style(line-height: 150%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(line-height: 50%)", &ctx));
    }

    #[test]
    fn style_query_height_percent_uses_height_basis_not_width() {
        // `style_ctx` has height: Some(100.0), width: 200.0. `height: 50%`
        // must resolve against height (50px), not width (100px).
        let ctx = style_ctx_with_style_props(&[], &[("height", "50px")]);
        assert!(crate::evaluate_container_condition("style(height: 50%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(height: 100%)", &ctx));
    }

    #[test]
    fn style_query_top_percent_uses_height_basis() {
        let ctx = style_ctx_with_style_props(&[], &[("top", "25px")]);
        assert!(crate::evaluate_container_condition("style(top: 25%)", &ctx));
    }

    #[test]
    fn style_query_height_basis_is_own_containing_block_not_own_height() {
        // Even when the container is `container-type: inline-size` (its own
        // `height` is unknown, mirroring `ctx.height: None`), `%` in a
        // `height`/`top` style() query must resolve against the container's
        // *own* containing block height — not fall back to width like the
        // Phase 0 gap used to (see CSS-SPECS.md T3 Container Queries).
        let mut ctx = style_ctx_with_style_props(&[], &[("height", "60px")]);
        ctx.height = None;
        ctx.own_containing_block_height = 300.0;
        assert!(crate::evaluate_container_condition("style(height: 20%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(height: 30%)", &ctx),
            "30% of the containing block (300px) is 90px, not 60px — must not fall back to width (200px)");
    }

    #[test]
    fn style_query_margin_top_percent_still_uses_width_basis() {
        // CSS2.1 §8.3: vertical margin/padding percentages resolve against
        // the containing block *width*, not height — unlike `height`/`top`.
        // `style_ctx` has width: 200.0 → `50%` resolves to 100px.
        let ctx = style_ctx_with_style_props(&[], &[("margin-top", "100px")]);
        assert!(crate::evaluate_container_condition("style(margin-top: 50%)", &ctx));
    }

    #[test]
    fn style_query_viewport_unit_matches_computed_px() {
        // `style_ctx` has viewport: 1024x768 → `10vw` resolves to 102.4px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "102.4px")]);
        assert!(crate::evaluate_container_condition("style(width: 10vw)", &ctx));
    }

    #[test]
    fn style_query_value_internal_whitespace_normalized() {
        // Container declares `1px  2px` (two spaces); query uses a single space.
        let ctx = style_ctx(&[("--gap", "1px  2px")]);
        assert!(crate::evaluate_container_condition("style(--gap: 1px 2px)", &ctx));
    }

    #[test]
    fn style_query_value_no_space_matches_spaced() {
        // Query without a space after the colon matches a spaced container value.
        let ctx = style_ctx(&[("--gap", "1px 2px")]);
        assert!(crate::evaluate_container_condition("style(--gap:1px 2px)", &ctx));
    }

    #[test]
    fn style_query_value_comma_whitespace_normalized() {
        // `a, b` (container) equals `a,b` (query) after comma-space normalization.
        let ctx = style_ctx(&[("--list", "a, b")]);
        assert!(crate::evaluate_container_condition("style(--list: a,b)", &ctx));
    }

    #[test]
    fn style_query_value_whitespace_difference_still_distinguishes_tokens() {
        // Normalization must not merge distinct tokens: `1px2px` != `1px 2px`.
        let ctx = style_ctx(&[("--gap", "1px2px")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 1px 2px)", &ctx));
    }

    #[test]
    fn style_query_var_chain_resolves() {
        // Container's `--gap` references `--base` via var() — resolved before compare.
        let ctx = style_ctx(&[("--base", "8px"), ("--gap", "var(--base)")]);
        assert!(crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

    #[test]
    fn style_query_var_chain_mismatch() {
        let ctx = style_ctx(&[("--base", "8px"), ("--gap", "var(--base)")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 4px)", &ctx));
    }

    #[test]
    fn style_query_var_unresolved_reference_is_false() {
        // `--gap` references an undeclared custom property with no fallback.
        let ctx = style_ctx(&[("--gap", "var(--missing)")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

    #[test]
    fn style_query_var_boolean_form_resolves() {
        let ctx = style_ctx(&[("--base", "dark"), ("--theme", "var(--base)")]);
        assert!(crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_var_fallback_used() {
        // `--gap` references an undeclared property, but with a fallback value.
        let ctx = style_ctx(&[("--gap", "var(--missing, 8px)")]);
        assert!(crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

