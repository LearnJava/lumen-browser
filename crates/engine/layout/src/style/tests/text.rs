//! Тесты `style.rs`: текст: `text-decoration`, `text-wrap`, перенос строк, режимы письма
//! и логические свойства.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ── text-decoration parsing ────────────────────────────────────────────

    #[test]
    fn text_decoration_underline_sets_only_underline() {
        let p = parse_text_decoration_shorthand("underline");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
        assert!(p.color.is_none());
    }

    #[test]
    fn text_decoration_none_returns_empty() {
        let p = parse_text_decoration_shorthand("none");
        assert!(p.line.unwrap().is_empty());
    }

    #[test]
    fn text_decoration_multiple_keywords_combine() {
        let p = parse_text_decoration_shorthand("overline underline");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(d.overline);
        assert!(!d.line_through);
    }

    #[test]
    fn text_decoration_line_through_with_hyphen() {
        let p = parse_text_decoration_shorthand("line-through");
        assert!(p.line.unwrap().line_through);
    }

    #[test]
    fn text_decoration_none_with_other_clears_all() {
        // `none` всегда побеждает: интуитивный сброс.
        let p = parse_text_decoration_shorthand("underline none");
        assert!(p.line.unwrap().is_empty());
    }

    #[test]
    fn text_decoration_blink_and_style_tokens_ignored_for_line() {
        // `blink` — поглощаем (CSS2 deprecated); `solid` — это style, не line.
        let p = parse_text_decoration_shorthand("underline blink solid");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
        assert!(p.color.is_none(), "no color token → None");
        assert_eq!(p.style, Some(TextDecorationStyle::Solid));
    }

    #[test]
    fn text_decoration_unrecognized_only_returns_none_line() {
        let p = parse_text_decoration_shorthand("blink");
        assert!(p.line.is_none());
        let p = parse_text_decoration_shorthand("");
        assert!(p.line.is_none());
    }

    #[test]
    fn text_decoration_is_case_insensitive() {
        let p = parse_text_decoration_shorthand("UNDERLINE Line-Through");
        let d = p.line.unwrap();
        assert!(d.underline);
        assert!(d.line_through);
    }

    // ── text-decoration-color ───────────────────────────────────────────────

    #[test]
    fn text_decoration_color_named_in_shorthand() {
        // `text-decoration: underline red` — линия + цвет.
        let p = parse_text_decoration_shorthand("underline red");
        assert!(p.line.unwrap().underline);
        assert_eq!(p.color, Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })));
    }

    #[test]
    fn text_decoration_color_hex_in_shorthand() {
        let p = parse_text_decoration_shorthand("overline #00ff00");
        assert!(p.line.unwrap().overline);
        assert_eq!(p.color, Some(CssColor::Rgba(Color { r: 0, g: 255, b: 0, a: 255 })));
    }

    #[test]
    fn text_decoration_color_rgb_function_in_shorthand() {
        // Color-функция с пробелами (modern CSS syntax) — токены должны
        // склеиваться обратно.
        let p = parse_text_decoration_shorthand("line-through rgb(0 0 255)");
        assert!(p.line.unwrap().line_through);
        assert_eq!(p.color, Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })));
    }

    #[test]
    fn text_decoration_color_property_named() {
        // Отдельное свойство text-decoration-color.
        let s = style_for("text-decoration-color: blue");
        assert_eq!(s.text_decoration_color, CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn text_decoration_color_currentcolor_resets() {
        // `currentcolor` сбрасывает text-decoration-color в None.
        let s = style_for("text-decoration-color: red; text-decoration-color: currentcolor");
        assert_eq!(s.text_decoration_color, CssColor::CurrentColor);
    }

    #[test]
    fn text_decoration_color_not_inherited_to_separate_branch() {
        // Через каскад наследуется (как и text-decoration-line в Phase 0):
        // дочерний `<p>` получает родительский text-decoration-color.
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-color: red; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.text_decoration_color, CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }));
        let p = doc.get(div).children[0];
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(p_style.text_decoration_color, CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_shorthand_sets_color_via_apply() {
        // Полный путь через apply_declaration.
        let s = style_for("text-decoration: underline blue");
        assert!(s.text_decoration_line.underline);
        assert_eq!(s.text_decoration_color, CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn text_decoration_color_default_is_none() {
        // По умолчанию text-decoration-color = None → currentColor при
        // рендеринге.
        let s = ComputedStyle::root();
        assert!(matches!(s.text_decoration_color, CssColor::CurrentColor));
    }

    // ── text-decoration-style ──────────────────────────────────────────────

    #[test]
    fn text_decoration_style_default_is_solid() {
        let s = ComputedStyle::root();
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_style_longhand_keywords() {
        assert_eq!(style_for("text-decoration-style: double").text_decoration_style,
                   TextDecorationStyle::Double);
        assert_eq!(style_for("text-decoration-style: dotted").text_decoration_style,
                   TextDecorationStyle::Dotted);
        assert_eq!(style_for("text-decoration-style: dashed").text_decoration_style,
                   TextDecorationStyle::Dashed);
        assert_eq!(style_for("text-decoration-style: wavy").text_decoration_style,
                   TextDecorationStyle::Wavy);
        assert_eq!(style_for("text-decoration-style: solid").text_decoration_style,
                   TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_style_invalid_ignored() {
        // Невалидное значение — declaration ignored, initial остаётся.
        let s = style_for("text-decoration-style: invalid-value");
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_style_case_insensitive() {
        assert_eq!(style_for("text-decoration-style: WAVY").text_decoration_style,
                   TextDecorationStyle::Wavy);
        assert_eq!(style_for("text-decoration-style: Dotted").text_decoration_style,
                   TextDecorationStyle::Dotted);
    }

    #[test]
    fn text_decoration_style_in_shorthand() {
        // `text-decoration: underline wavy red` — все три компонента.
        let s = style_for("text-decoration: underline wavy red");
        assert!(s.text_decoration_line.underline);
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Wavy);
        assert_eq!(s.text_decoration_color, CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn text_decoration_style_shorthand_resets_to_initial() {
        // CSS Text Decoration L3 §2.1: shorthand сбрасывает все longhand-ы
        // (кроме thickness — она исключена из L3 shorthand-а).
        let s = style_for("text-decoration-style: wavy; text-decoration: underline");
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid,
                   "shorthand сбросил style к initial");
        assert!(s.text_decoration_line.underline);
    }

    #[test]
    fn text_decoration_style_inherited_via_cascade() {
        // Phase 0 каскадирует text-decoration-style через inherit (как и
        // line / color).
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-style: dotted; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.text_decoration_style, TextDecorationStyle::Dotted);
        let p = doc.get(div).children[0];
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(p_style.text_decoration_style, TextDecorationStyle::Dotted);
    }

    // ── text-decoration-thickness ──────────────────────────────────────────

    #[test]
    fn text_decoration_thickness_default_is_auto() {
        let s = ComputedStyle::root();
        assert_eq!(s.text_decoration_thickness, TextDecorationThickness::Auto);
    }

    #[test]
    fn text_decoration_thickness_keywords() {
        assert_eq!(style_for("text-decoration-thickness: auto").text_decoration_thickness,
                   TextDecorationThickness::Auto);
        assert_eq!(style_for("text-decoration-thickness: from-font").text_decoration_thickness,
                   TextDecorationThickness::FromFont);
    }

    #[test]
    fn text_decoration_thickness_length_px() {
        let s = style_for("text-decoration-thickness: 3px");
        match s.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 3.0).abs() < 0.01),
            other => panic!("expected Length(3.0), got {other:?}"),
        }
    }

    #[test]
    fn text_decoration_thickness_length_em_resolved() {
        // 0.5em при font-size 16 → 8px (resolve через em_basis).
        let s = style_for("text-decoration-thickness: 0.5em");
        match s.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 8.0).abs() < 0.01,
                                                            "0.5em @ 16px = 8, got {px}"),
            other => panic!("expected Length, got {other:?}"),
        }
    }

    #[test]
    fn text_decoration_thickness_percentage() {
        // 25% хранится как fraction 0.25.
        let s = style_for("text-decoration-thickness: 25%");
        match s.text_decoration_thickness {
            TextDecorationThickness::Percentage(f) => assert!((f - 0.25).abs() < 0.001),
            other => panic!("expected Percentage(0.25), got {other:?}"),
        }
    }

    #[test]
    fn text_decoration_thickness_invalid_ignored() {
        let s = style_for("text-decoration-thickness: foobar");
        assert_eq!(s.text_decoration_thickness, TextDecorationThickness::Auto);
    }

    #[test]
    fn text_decoration_thickness_case_insensitive() {
        assert_eq!(style_for("text-decoration-thickness: AUTO").text_decoration_thickness,
                   TextDecorationThickness::Auto);
        assert_eq!(style_for("text-decoration-thickness: From-Font").text_decoration_thickness,
                   TextDecorationThickness::FromFont);
    }

    #[test]
    fn text_decoration_thickness_not_in_l3_shorthand() {
        // CSS Text Decoration L3 §2.1 — thickness НЕ входит в shorthand.
        // Установка через longhand + shorthand не должна сбрасывать thickness.
        let s = style_for("text-decoration-thickness: 5px; text-decoration: underline");
        match s.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 5.0).abs() < 0.01,
                                                            "shorthand НЕ должен сбрасывать thickness"),
            other => panic!("expected Length(5.0), got {other:?}"),
        }
        assert!(s.text_decoration_line.underline);
    }

    #[test]
    fn text_decoration_thickness_inherited_via_cascade() {
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-thickness: 4px; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let p = doc.get(div).children[0];
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        match p_style.text_decoration_thickness {
            TextDecorationThickness::Length(px) => assert!((px - 4.0).abs() < 0.01),
            other => panic!("expected inherited Length(4.0), got {other:?}"),
        }
    }

    // ── CSS-wide keywords для text-decoration-style / -thickness ───────────

    #[test]
    fn text_decoration_style_initial_keyword_resets() {
        // `initial` сбрасывает к спецификационному initial (Solid).
        let s = style_for("text-decoration-style: wavy; text-decoration-style: initial");
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Solid);
    }

    #[test]
    fn text_decoration_thickness_initial_keyword_resets() {
        let s = style_for("text-decoration-thickness: 5px; text-decoration-thickness: initial");
        assert_eq!(s.text_decoration_thickness, TextDecorationThickness::Auto);
    }

    // ── CSS Text Module Level 4 §6.4 — text-wrap-mode / text-wrap-style / text-wrap ──

    #[test]
    fn text_wrap_defaults_are_initial() {
        let s = cascade_at("<p></p>", "", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_mode_keywords_parse() {
        for (val, expected) in [
            ("wrap", TextWrapMode::Wrap),
            ("nowrap", TextWrapMode::Nowrap),
        ] {
            let s = cascade_at(
                "<p></p>",
                &format!("p {{ text-wrap-mode: {val}; }}"),
                &[0],
            );
            assert_eq!(s.text_wrap_mode, expected, "for value {val}");
        }
    }

    #[test]
    fn text_wrap_style_keywords_parse() {
        for (val, expected) in [
            ("auto", TextWrapStyle::Auto),
            ("balance", TextWrapStyle::Balance),
            ("stable", TextWrapStyle::Stable),
            ("pretty", TextWrapStyle::Pretty),
        ] {
            let s = cascade_at(
                "<p></p>",
                &format!("p {{ text-wrap-style: {val}; }}"),
                &[0],
            );
            assert_eq!(s.text_wrap_style, expected, "for value {val}");
        }
    }

    #[test]
    fn text_wrap_mode_case_insensitive() {
        let s = cascade_at("<p></p>", "p { text-wrap-mode: NOWRAP; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
    }

    #[test]
    fn text_wrap_invalid_longhand_ignored() {
        // Невалидное значение longhand → declaration invalid → initial.
        let s = cascade_at("<p></p>", "p { text-wrap-mode: bogus; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        let s = cascade_at("<p></p>", "p { text-wrap-style: bogus; }", &[0]);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_shorthand_single_mode() {
        let s = cascade_at("<p></p>", "p { text-wrap: nowrap; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_shorthand_single_style() {
        let s = cascade_at("<p></p>", "p { text-wrap: balance; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_shorthand_mode_then_style() {
        let s = cascade_at("<p></p>", "p { text-wrap: nowrap pretty; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Pretty);
    }

    #[test]
    fn text_wrap_shorthand_style_then_mode() {
        // `<'mode'> || <'style'>` — порядок свободный.
        let s = cascade_at("<p></p>", "p { text-wrap: pretty nowrap; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Pretty);
    }

    #[test]
    fn text_wrap_shorthand_resets_longhands() {
        // Shorthand сбрасывает обе компоненты к initial, даже если в правиле
        // только одна указана.
        let s = cascade_at(
            "<p></p>",
            "p { text-wrap-mode: nowrap; text-wrap-style: pretty; text-wrap: balance; }",
            &[0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_shorthand_invalid_token_aborts() {
        // Нераспознанный токен ⇒ shorthand отбрасывается; обе longhand остаются
        // initial после reset (см. doc-comment на apply_text_wrap_shorthand).
        let s = cascade_at("<p></p>", "p { text-wrap: bogus pretty; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_shorthand_duplicate_slot_aborts() {
        // Два token-а из одного слота (две стилистические опции) ⇒ невалидно.
        let s = cascade_at("<p></p>", "p { text-wrap: balance pretty; }", &[0]);
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_mode_inherited() {
        // CSS Text 4 §6.4.1 — text-wrap-mode inherited.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
    }

    #[test]
    fn text_wrap_style_inherited() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-style: balance; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_child_override_wins() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; } p { text-wrap-mode: wrap; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
    }

    #[test]
    fn text_wrap_initial_keyword_resets() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-style: pretty; } p { text-wrap-style: initial; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn text_wrap_unset_for_inherited_is_inherit() {
        // CSS Cascade L4 §7: `unset` для inherited-свойства ≡ `inherit`.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; } p { text-wrap-mode: unset; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
    }

    #[test]
    fn text_wrap_shorthand_css_wide_keyword_inherit_both() {
        // CSS-wide-keyword на shorthand применяется к обоим longhand-ам.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; text-wrap-style: balance; } \
             p { text-wrap: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Nowrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Balance);
    }

    #[test]
    fn text_wrap_shorthand_css_wide_keyword_initial_both() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { text-wrap-mode: nowrap; text-wrap-style: balance; } \
             p { text-wrap: initial; }",
            &[0, 0],
        );
        assert_eq!(s.text_wrap_mode, TextWrapMode::Wrap);
        assert_eq!(s.text_wrap_style, TextWrapStyle::Auto);
    }

    #[test]
    fn linear_progress_is_identity() {
        let f = TimingFunction::Linear;
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.25), 0.25));
        assert!(approx(f.progress(0.5), 0.5));
        assert!(approx(f.progress(0.75), 0.75));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn progress_clamps_t_out_of_range() {
        let f = TimingFunction::Linear;
        assert!(approx(f.progress(-0.5), 0.0));
        assert!(approx(f.progress(2.0), 1.0));
    }

    #[test]
    fn ease_keyword_endpoints() {
        let f = TimingFunction::parse("ease").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
        // Midpoint of ease (cubic-bezier(0.25, 0.1, 0.25, 1.0)) ≈ 0.802 per
        // spec curves — well above 0.5, как и должно быть для ease-out shape.
        let mid = f.progress(0.5);
        assert!(mid > 0.7 && mid < 0.85, "ease(0.5) was {mid}");
    }

    #[test]
    fn ease_in_starts_slow() {
        // cubic-bezier(0.42, 0.0, 1.0, 1.0) — output быстро растёт во второй
        // половине, медленно в первой. progress(0.25) должен быть < 0.25.
        let f = TimingFunction::parse("ease-in").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
        assert!(f.progress(0.25) < 0.15);
    }

    #[test]
    fn ease_out_starts_fast() {
        // cubic-bezier(0.0, 0.0, 0.58, 1.0) — output быстро растёт в первой
        // половине. progress(0.25) должен быть > 0.25.
        let f = TimingFunction::parse("ease-out").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
        assert!(f.progress(0.25) > 0.35);
    }

    #[test]
    fn ease_in_out_is_symmetric_around_half() {
        // cubic-bezier(0.42, 0.0, 0.58, 1.0) — симметрично:
        // f(0.5) ≈ 0.5; f(t) + f(1-t) ≈ 1.
        let f = TimingFunction::parse("ease-in-out").unwrap();
        assert!(approx(f.progress(0.5), 0.5));
        let a = f.progress(0.2);
        let b = f.progress(0.8);
        assert!(approx(a + b, 1.0), "ease-in-out asymmetric: {a} + {b}");
    }

    #[test]
    fn cubic_bezier_diagonal_equals_linear() {
        // cubic-bezier(0, 0, 1, 1) ≡ linear (control points collinear с (0,0)→(1,1)).
        let f = TimingFunction::CubicBezier(0.0, 0.0, 1.0, 1.0);
        for &t in &[0.0_f32, 0.1, 0.3, 0.5, 0.7, 0.9, 1.0] {
            assert!(
                (f.progress(t) - t).abs() < 1e-3,
                "diagonal bezier deviated at t={t}: {}",
                f.progress(t)
            );
        }
    }

    #[test]
    fn cubic_bezier_overshoot_allowed() {
        // Контрольные y вне [0,1] → output может выходить за [0,1] (анимации
        // "spring" / bounce). Спека не clamp-ает output.
        let f = TimingFunction::CubicBezier(0.5, 1.5, 0.5, -0.5);
        let mid = f.progress(0.5);
        // По симметрии в середине ≈ 0.5, но в первой четверти > 1 не успеет
        // — overshoot скорее в y2. Главное — обработка корректна.
        let y_at_quarter = f.progress(0.25);
        let y_at_three_quarters = f.progress(0.75);
        // Симметричная кривая: f(t) + f(1-t) ≈ 1.
        assert!(approx(y_at_quarter + y_at_three_quarters, 1.0));
        assert!(approx(mid, 0.5));
    }

    #[test]
    fn steps_jump_end_default() {
        // steps(4, jump-end): 4 шага 0, 1/4, 2/4, 3/4 на интервалах
        // [0, 1/4), [1/4, 2/4), [2/4, 3/4), [3/4, 1); t=1 → 1.
        let f = TimingFunction::Steps(4, StepPosition::JumpEnd);
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.1), 0.0));
        assert!(approx(f.progress(0.25), 0.25));
        assert!(approx(f.progress(0.49), 0.25));
        assert!(approx(f.progress(0.5), 0.5));
        assert!(approx(f.progress(0.75), 0.75));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn steps_jump_start() {
        // steps(4, jump-start): 4 шага 1/4, 2/4, 3/4, 1 (прыжок при t=0).
        let f = TimingFunction::Steps(4, StepPosition::JumpStart);
        assert!(approx(f.progress(0.0), 0.25));
        assert!(approx(f.progress(0.1), 0.25));
        assert!(approx(f.progress(0.25), 0.5));
        assert!(approx(f.progress(0.5), 0.75));
        assert!(approx(f.progress(0.75), 1.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn steps_jump_none() {
        // steps(4, jump-none): 4 уровня 0, 1/3, 2/3, 1 (нет прыжков на границах).
        let f = TimingFunction::Steps(4, StepPosition::JumpNone);
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.24), 0.0));
        assert!(approx(f.progress(0.25), 1.0 / 3.0));
        assert!(approx(f.progress(0.5), 2.0 / 3.0));
        assert!(approx(f.progress(0.75), 1.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn steps_jump_both() {
        // steps(4, jump-both): 5 шагов 1/5, 2/5, 3/5, 4/5, 1 (прыжки на обеих границах).
        let f = TimingFunction::Steps(4, StepPosition::JumpBoth);
        assert!(approx(f.progress(0.0), 0.2));
        assert!(approx(f.progress(0.1), 0.2));
        assert!(approx(f.progress(0.25), 0.4));
        assert!(approx(f.progress(0.5), 0.6));
        assert!(approx(f.progress(0.75), 0.8));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn step_start_keyword_jumps_immediately() {
        let f = TimingFunction::parse("step-start").unwrap();
        assert!(approx(f.progress(0.0), 1.0));
        assert!(approx(f.progress(0.5), 1.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn step_end_keyword_jumps_at_end() {
        let f = TimingFunction::parse("step-end").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.5), 0.0));
        assert!(approx(f.progress(0.99), 0.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    // === quotes parsing (CSS Generated Content L3 §3.2) ===

    #[test]
    fn quotes_auto_and_none() {
        assert_eq!(ts_prop("quotes", "auto").quotes, Quotes::Auto);
        assert_eq!(ts_prop("quotes", "none").quotes, Quotes::None);
    }

    #[test]
    fn quotes_explicit_pairs() {
        let s = ts_prop("quotes", "\"«\" \"»\" \"‹\" \"›\"");
        assert_eq!(
            s.quotes,
            Quotes::Pairs(vec![
                ("«".to_string(), "»".to_string()),
                ("‹".to_string(), "›".to_string()),
            ])
        );
    }

    #[test]
    fn quotes_odd_string_count_rejected() {
        // Three strings → malformed → value unchanged (stays initial Auto).
        let s = ts_prop("quotes", "\"a\" \"b\" \"c\"");
        assert_eq!(s.quotes, Quotes::Auto);
    }

    #[test]
    fn quotes_hex_escape_decoded() {
        // \201C “ and \201D ”.
        let s = ts_prop("quotes", "\"\\201C\" \"\\201D\"");
        assert_eq!(
            s.quotes,
            Quotes::Pairs(vec![("\u{201C}".to_string(), "\u{201D}".to_string())])
        );
    }

    #[test]
    fn quotes_pair_for_depth_clamps() {
        let q = Quotes::Pairs(vec![
            ("«".to_string(), "»".to_string()),
            ("‹".to_string(), "›".to_string()),
        ]);
        assert_eq!(q.pair_for_depth(0), Some(("«", "»")));
        assert_eq!(q.pair_for_depth(1), Some(("‹", "›")));
        // Beyond the last pair → clamp to last.
        assert_eq!(q.pair_for_depth(5), Some(("‹", "›")));
        // Auto uses the built-in English pairs.
        assert_eq!(Quotes::Auto.pair_for_depth(0), Some(("\u{201C}", "\u{201D}")));
        assert_eq!(Quotes::Auto.pair_for_depth(1), Some(("\u{2018}", "\u{2019}")));
        // none → no glyphs.
        assert_eq!(Quotes::None.pair_for_depth(0), None);
    }

    // ──────────── CSS Logical Properties L1 ────────────

    #[test]
    fn margin_inline_start_maps_to_margin_left() {
        let s = cascade_at("<div>", "div { margin-inline-start: 10px; }", &[0]);
        assert_eq!(s.margin_left, LengthOrAuto::Length(Length::Px(10.0)));
    }

    #[test]
    fn margin_inline_end_maps_to_margin_right() {
        let s = cascade_at("<div>", "div { margin-inline-end: 20px; }", &[0]);
        assert_eq!(s.margin_right, LengthOrAuto::Length(Length::Px(20.0)));
    }

    #[test]
    fn ua_body_default_margin_is_8px() {
        // HTML Rendering §14.3.3: body { margin: 8px }. BUG-204 — без этого
        // правила body прижимался к краю viewport и контент сдвигался на 8px.
        let s = cascade_at("<div></div>", "", &[]);
        assert_eq!(s.margin_top, LengthOrAuto::Length(Length::Px(8.0)));
        assert_eq!(s.margin_right, LengthOrAuto::Length(Length::Px(8.0)));
        assert_eq!(s.margin_bottom, LengthOrAuto::Length(Length::Px(8.0)));
        assert_eq!(s.margin_left, LengthOrAuto::Length(Length::Px(8.0)));
    }

    #[test]
    fn ua_body_margin_overridden_by_author_reset() {
        // Author `body { margin: 0 }` (или `* { margin: 0 }` reset) перекрывает UA-правило.
        let s = cascade_at("<div></div>", "body { margin: 0; }", &[]);
        assert_eq!(s.margin_top, LengthOrAuto::ZERO);
        assert_eq!(s.margin_left, LengthOrAuto::ZERO);
    }

    #[test]
    fn margin_block_start_maps_to_margin_top() {
        let s = cascade_at("<div>", "div { margin-block-start: 5px; }", &[0]);
        assert_eq!(s.margin_top, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn margin_block_end_maps_to_margin_bottom() {
        let s = cascade_at("<div>", "div { margin-block-end: 15px; }", &[0]);
        assert_eq!(s.margin_bottom, LengthOrAuto::Length(Length::Px(15.0)));
    }

    #[test]
    fn margin_inline_shorthand_two_values() {
        let s = cascade_at("<div>", "div { margin-inline: 8px 12px; }", &[0]);
        assert_eq!(s.margin_left,  LengthOrAuto::Length(Length::Px(8.0)));
        assert_eq!(s.margin_right, LengthOrAuto::Length(Length::Px(12.0)));
    }

    #[test]
    fn margin_block_shorthand_one_value() {
        let s = cascade_at("<div>", "div { margin-block: 6px; }", &[0]);
        assert_eq!(s.margin_top,    LengthOrAuto::Length(Length::Px(6.0)));
        assert_eq!(s.margin_bottom, LengthOrAuto::Length(Length::Px(6.0)));
    }

    #[test]
    fn padding_inline_start_maps_to_padding_left() {
        let s = cascade_at("<div>", "div { padding-inline-start: 10px; }", &[0]);
        assert_eq!(s.padding_left, Length::Px(10.0));
    }

    #[test]
    fn padding_inline_end_maps_to_padding_right() {
        let s = cascade_at("<div>", "div { padding-inline-end: 20px; }", &[0]);
        assert_eq!(s.padding_right, Length::Px(20.0));
    }

    #[test]
    fn padding_block_shorthand_two_values() {
        let s = cascade_at("<div>", "div { padding-block: 4px 8px; }", &[0]);
        assert_eq!(s.padding_top,    Length::Px(4.0));
        assert_eq!(s.padding_bottom, Length::Px(8.0));
    }

    #[test]
    fn padding_inline_shorthand_one_value() {
        let s = cascade_at("<div>", "div { padding-inline: 7px; }", &[0]);
        assert_eq!(s.padding_left,  Length::Px(7.0));
        assert_eq!(s.padding_right, Length::Px(7.0));
    }

    #[test]
    fn inset_inline_start_maps_to_left() {
        let s = cascade_at("<div>", "div { position: absolute; inset-inline-start: 5px; }", &[0]);
        assert_eq!(s.left, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn inset_block_end_maps_to_bottom() {
        let s = cascade_at("<div>", "div { position: absolute; inset-block-end: 3px; }", &[0]);
        assert_eq!(s.bottom, LengthOrAuto::Length(Length::Px(3.0)));
    }

    #[test]
    fn inset_inline_shorthand() {
        let s = cascade_at("<div>", "div { position: absolute; inset-inline: 2px 4px; }", &[0]);
        assert_eq!(s.left,  LengthOrAuto::Length(Length::Px(2.0)));
        assert_eq!(s.right, LengthOrAuto::Length(Length::Px(4.0)));
    }

    #[test]
    fn inset_block_shorthand() {
        let s = cascade_at("<div>", "div { position: absolute; inset-block: 1px; }", &[0]);
        assert_eq!(s.top,    LengthOrAuto::Length(Length::Px(1.0)));
        assert_eq!(s.bottom, LengthOrAuto::Length(Length::Px(1.0)));
    }

    #[test]
    fn border_inline_start_maps_to_border_left() {
        let s = cascade_at("<div>", "div { border-inline-start: 3px solid red; }", &[0]);
        assert_eq!(s.border_left_style, BorderStyle::Solid);
        assert!((s.border_left_width - 3.0).abs() < 0.1);
    }

    #[test]
    fn border_block_end_maps_to_border_bottom() {
        let s = cascade_at("<div>", "div { border-block-end: 2px dashed blue; }", &[0]);
        assert_eq!(s.border_bottom_style, BorderStyle::Dashed);
        assert!((s.border_bottom_width - 2.0).abs() < 0.1);
    }

    #[test]
    fn border_inline_start_width_longhand() {
        let s = cascade_at("<div>", "div { border-inline-start-width: 5px; }", &[0]);
        assert!((s.border_left_width - 5.0).abs() < 0.1);
    }

    #[test]
    fn border_block_start_style_longhand() {
        let s = cascade_at("<div>", "div { border-block-start-style: dotted; }", &[0]);
        assert_eq!(s.border_top_style, BorderStyle::Dotted);
    }

    #[test]
    fn border_inline_end_color_longhand() {
        let s = cascade_at("<div>", "div { border-inline-end-color: #ff0000; }", &[0]);
        assert_eq!(s.border_right_color, CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn text_underline_position_initial_auto() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_underline_position, TextUnderlinePosition::Auto);
    }

    #[test]
    fn text_underline_position_from_font() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-position: from-font; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_underline_position, TextUnderlinePosition::FromFont);
    }

    #[test]
    fn text_underline_position_under() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-position: under; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_underline_position, TextUnderlinePosition::Under);
    }

    #[test]
    fn text_underline_position_left_right() {
        let doc = lumen_html_parser::parse("<span></span><em></em>");
        let left_sheet = lumen_css_parser::parse("span { text-underline-position: left; }");
        let right_sheet = lumen_css_parser::parse("em { text-underline-position: right; }");
        let root = ComputedStyle::root();
        let span = doc.get(doc.body().unwrap()).children[0];
        let em = doc.get(doc.body().unwrap()).children[1];
        let left_style = compute_style(&doc, span, &left_sheet, &root, Size::new(800.0, 600.0), false);
        let right_style = compute_style(&doc, em, &right_sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(left_style.text_underline_position, TextUnderlinePosition::Left);
        assert_eq!(right_style.text_underline_position, TextUnderlinePosition::Right);
    }

    #[test]
    fn text_underline_position_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-position: under; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.text_underline_position, TextUnderlinePosition::Under);
        assert_eq!(span_style.text_underline_position, TextUnderlinePosition::Under);
    }

    #[test]
    fn text_underline_position_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-position: banana; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_underline_position, TextUnderlinePosition::Auto);
    }

    // ── text-underline-offset ─────────────────────────────────────────────────

    #[test]
    fn text_underline_offset_initial_none() {
        let style = ComputedStyle::root();
        assert_eq!(style.text_underline_offset, None);
    }

    #[test]
    fn text_underline_offset_px() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-offset: 4px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_underline_offset, Some(4.0));
    }

    #[test]
    fn text_underline_offset_auto_resets_to_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-offset: auto; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_underline_offset, None);
    }

    #[test]
    fn text_underline_offset_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-offset: 6px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.text_underline_offset, Some(6.0));
        assert_eq!(span_style.text_underline_offset, Some(6.0));
    }

    #[test]
    fn text_underline_offset_negative_px() {
        // Negative offset shifts underline toward text (CSS allows it).
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-underline-offset: -2px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_underline_offset, Some(-2.0));
    }

    // ── text-decoration-skip-ink ──────────────────────────────────────────────

    #[test]
    fn text_decoration_skip_ink_initial_auto() {
        let style = ComputedStyle::root();
        assert_eq!(style.text_decoration_skip_ink, TextDecorationSkipInk::Auto);
    }

    #[test]
    fn text_decoration_skip_ink_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-skip-ink: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_decoration_skip_ink, TextDecorationSkipInk::None);
    }

    #[test]
    fn text_decoration_skip_ink_all() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-skip-ink: all; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_decoration_skip_ink, TextDecorationSkipInk::All);
    }

    #[test]
    fn text_decoration_skip_ink_auto_explicit() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-skip-ink: auto; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_decoration_skip_ink, TextDecorationSkipInk::Auto);
    }

    #[test]
    fn text_decoration_skip_ink_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-skip-ink: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.text_decoration_skip_ink, TextDecorationSkipInk::None);
        assert_eq!(span_style.text_decoration_skip_ink, TextDecorationSkipInk::None);
    }

    #[test]
    fn text_decoration_skip_ink_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-decoration-skip-ink: edges; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        // Invalid keyword — property stays at inherited initial (Auto).
        assert_eq!(style.text_decoration_skip_ink, TextDecorationSkipInk::Auto);
    }

    // ── line-break ────────────────────────────────────────────────────────────

    #[test]
    fn line_break_initial_auto() {
        let style = ComputedStyle::root();
        assert_eq!(style.line_break, LineBreak::Auto);
    }

    #[test]
    fn line_break_strict() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { line-break: strict; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_break, LineBreak::Strict);
    }

    #[test]
    fn line_break_all_values() {
        for (css, expected) in [
            ("loose", LineBreak::Loose),
            ("normal", LineBreak::Normal),
            ("strict", LineBreak::Strict),
            ("anywhere", LineBreak::Anywhere),
        ] {
            let doc = lumen_html_parser::parse("<div></div>");
            let sheet = lumen_css_parser::parse(&format!("div {{ line-break: {css}; }}"));
            let root = ComputedStyle::root();
            let div = doc.get(doc.body().unwrap()).children[0];
            let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
            assert_eq!(style.line_break, expected, "css={css}");
        }
    }

    #[test]
    fn line_break_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { line-break: strict; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.line_break, LineBreak::Strict);
        assert_eq!(span_style.line_break, LineBreak::Strict);
    }

    #[test]
    fn line_break_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { line-break: always; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_break, LineBreak::Auto);
    }

    // --- text-align-last ---

    #[test]
    fn text_align_last_basic() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-align-last: justify; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_align_last, TextAlignLast::Justify);
    }

    #[test]
    fn text_align_last_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_align_last, TextAlignLast::Auto);
    }

    #[test]
    fn text_align_last_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { text-align-last: right; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.text_align_last, TextAlignLast::Right);
        assert_eq!(span_style.text_align_last, TextAlignLast::Auto);
    }

    #[test]
    fn text_align_last_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-align-last: bogus; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_align_last, TextAlignLast::Auto);
    }

    // --- line-height-step (CSS Rhythmic Sizing L1 §2) ---

    #[test]
    fn line_height_step_px() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { line-height-step: 18px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!((style.line_height_step - 18.0).abs() < f32::EPSILON);
    }

    #[test]
    fn line_height_step_em_resolves_to_font_size() {
        // 1.5em at font-size 20px → 30px.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { font-size: 20px; line-height-step: 1.5em; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!((style.line_height_step - 30.0).abs() < 0.01, "got {}", style.line_height_step);
    }

    #[test]
    fn line_height_step_initial_zero() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_height_step, 0.0);
    }

    #[test]
    fn line_height_step_none_disables() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { line-height-step: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_height_step, 0.0);
    }

    #[test]
    fn line_height_step_negative_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { line-height-step: -4px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_height_step, 0.0);
    }

    #[test]
    fn line_height_step_inherited() {
        // CSS Rhythmic Sizing L1 §2 — line-height-step IS inherited.
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { line-height-step: 12px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style =
            compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert!((span_style.line_height_step - 12.0).abs() < f32::EPSILON);
    }

    // ── writing-mode ──────────────────────────────────────────────────────────

    #[test]
    fn writing_mode_initial_horizontal_tb() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.writing_mode, WritingMode::HorizontalTb);
    }

    #[test]
    fn writing_mode_vertical_rl() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { writing-mode: vertical-rl; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.writing_mode, WritingMode::VerticalRl);
    }

    #[test]
    fn writing_mode_vertical_lr() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { writing-mode: vertical-lr; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.writing_mode, WritingMode::VerticalLr);
    }

    #[test]
    fn writing_mode_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { writing-mode: vertical-rl; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.writing_mode, WritingMode::VerticalRl);
        assert_eq!(span_style.writing_mode, WritingMode::VerticalRl);
    }

    #[test]
    fn writing_mode_legacy_tb_rl_alias() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { writing-mode: tb-rl; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.writing_mode, WritingMode::VerticalRl);
    }

    // ── text-orientation ──────────────────────────────────────────────────────

    #[test]
    fn text_orientation_initial_mixed() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_orientation, TextOrientation::Mixed);
    }

    #[test]
    fn text_orientation_upright() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-orientation: upright; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_orientation, TextOrientation::Upright);
    }

    #[test]
    fn text_orientation_sideways() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { text-orientation: sideways; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.text_orientation, TextOrientation::Sideways);
    }

    #[test]
    fn text_orientation_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { writing-mode: vertical-rl; text-orientation: upright; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.text_orientation, TextOrientation::Upright);
        assert_eq!(span_style.text_orientation, TextOrientation::Upright);
    }
