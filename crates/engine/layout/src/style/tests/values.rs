//! Тесты `style.rs`: значения и единицы: `calc()`, математические функции, относительные
//! и viewport-единицы, `@property`, подстановки `var()`/`attr()`.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ── Relative units: parse_length + resolve ────────────────────────────

    #[test]
    fn parse_length_recognizes_units() {
        assert_eq!(parse_length("10px"), Some(Length::Px(10.0)));
        assert_eq!(parse_length("1.5em"), Some(Length::Em(1.5)));
        assert_eq!(parse_length("2rem"), Some(Length::Rem(2.0)));
        assert_eq!(parse_length("50%"), Some(Length::Percent(50.0)));
        assert_eq!(parse_length("0"), Some(Length::Px(0.0)));
        // Пробелы вокруг числа допустимы.
        assert_eq!(parse_length(" 10 px "), Some(Length::Px(10.0)));
        // Мусор → None.
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_length("px"), None);
    }

    // ── CSS Quirks Mode §3.3: unitless length quirk ───────────────────────

    #[test]
    fn unitless_length_quirks_mode_accepts_as_px() {
        // quirks=true: unitless non-zero → px
        assert_eq!(parse_length_q("10", true), Some(Length::Px(10.0)));
        assert_eq!(parse_length_q("1.5", true), Some(Length::Px(1.5)));
        assert_eq!(parse_length_q("-5", true), Some(Length::Px(-5.0)));
    }

    #[test]
    fn unitless_length_standards_mode_rejects_nonzero() {
        // quirks=false: unitless non-zero → None (CSS Values §6)
        assert_eq!(parse_length_q("10", false), None);
        assert_eq!(parse_length_q("1.5", false), None);
        assert_eq!(parse_length_q("-5", false), None);
    }

    #[test]
    fn unitless_zero_always_valid() {
        // `0` валиден без единицы в обоих режимах (CSS Values §6)
        assert_eq!(parse_length_q("0", true), Some(Length::Px(0.0)));
        assert_eq!(parse_length_q("0", false), Some(Length::Px(0.0)));
        assert_eq!(parse_length_q("0.0", true), Some(Length::Px(0.0)));
        assert_eq!(parse_length_q("0.0", false), Some(Length::Px(0.0)));
    }

    #[test]
    fn unitless_quirk_does_not_affect_dimensioned_values() {
        // Значения с единицами работают в обоих режимах
        assert_eq!(parse_length_q("10px", false), Some(Length::Px(10.0)));
        assert_eq!(parse_length_q("2em", false), Some(Length::Em(2.0)));
        assert_eq!(parse_length_q("50%", false), Some(Length::Percent(50.0)));
    }

    // ── CSS Values L4 §5.1.1 — ch / ex font-relative units ────────────────
    #[test]
    fn parse_length_recognizes_ch_ex() {
        assert_eq!(parse_length("60ch"), Some(Length::Ch(60.0)));
        assert_eq!(parse_length("2.5ex"), Some(Length::Ex(2.5)));
        // ch/ex are valid <font-size> tokens (font shorthand relies on this).
        assert!(is_font_size_token("2ch"));
        assert!(is_font_size_token("2ex"));
    }

    #[test]
    fn length_resolve_ch_ex_fallback_is_half_em() {
        // Outside a layout pass FONT_CH_EX is unset → spec 0.5em fallback:
        // 10ch at em_basis 20 = 10 * 0.5 * 20 = 100.
        pop_ch_ex_context(None);
        assert_eq!(Length::Ch(10.0).resolve(20.0, None, vp()), Some(100.0));
        assert_eq!(Length::Ex(4.0).resolve(20.0, None, vp()), Some(40.0));
    }

    #[test]
    fn length_resolve_ch_ex_uses_font_metric_context() {
        // With real metrics published (ch = 9px, ex = 7px per unit) the em_basis
        // is ignored — the absolute per-unit px drives the result.
        let prev = push_ch_ex_context(Some((9.0, 7.0)));
        assert_eq!(Length::Ch(3.0).resolve(20.0, None, vp()), Some(27.0));
        assert_eq!(Length::Ex(2.0).resolve(20.0, None, vp()), Some(14.0));
        pop_ch_ex_context(prev);
        // Context restored → fallback again.
        assert_eq!(Length::Ch(1.0).resolve(20.0, None, vp()), Some(10.0));
    }

    // ── viewport units ────────────────────────────────────────────────────

    #[test]
    fn parse_length_recognizes_viewport_units() {
        assert_eq!(parse_length("50vh"), Some(Length::Vh(50.0)));
        assert_eq!(parse_length("50vw"), Some(Length::Vw(50.0)));
        assert_eq!(parse_length("10vmin"), Some(Length::Vmin(10.0)));
        assert_eq!(parse_length("10vmax"), Some(Length::Vmax(10.0)));
        // Дробные значения тоже.
        assert_eq!(parse_length("1.5vh"), Some(Length::Vh(1.5)));
    }

    #[test]
    fn length_resolve_vh_uses_viewport_height() {
        // 50vh от viewport (1024 x 768) = 384.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vh(50.0).resolve(16.0, None, v), Some(384.0));
    }

    #[test]
    fn length_resolve_vw_uses_viewport_width() {
        // 25vw от viewport (1024 x 768) = 256.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vw(25.0).resolve(16.0, None, v), Some(256.0));
    }

    #[test]
    fn length_resolve_vmin_uses_smaller_dimension() {
        // 50vmin от viewport (1024 x 768) — min = 768; 50% = 384.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vmin(50.0).resolve(16.0, None, v), Some(384.0));
    }

    #[test]
    fn length_resolve_vmax_uses_larger_dimension() {
        // 50vmax от viewport (1024 x 768) — max = 1024; 50% = 512.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vmax(50.0).resolve(16.0, None, v), Some(512.0));
    }

    // ──────────────── CSS Variables L1: custom properties + var() ────────────────

    // BUG-341 S9 — gates by *identity*, not by output.
    //
    // Every assertion elsewhere in this file checks what `custom_props`
    // contains, and a per-node deep copy satisfies all of them; it is just
    // slow. That is the exact failure mode S8 uncovered in `graft_geometry`
    // (a reuse mechanism that reused nothing passed every differential test).
    // So the sharing itself has to be asserted, by pointer.

    // ──────────────── CSS Properties and Values L1 §1.1: @property ────────────────

    #[test]
    fn starting_style_does_not_leak_into_static_cascade() {
        // BUG-199 / TEST-71 — CSS Transitions L2 §3.4. `@starting-style` provides
        // the *before-change* style for entry transitions only; it must NOT affect
        // the static (settled) computed style of an element already present at load.
        // Here `.box-a` has opacity:1 normally and opacity:0/scale(0.5) inside
        // @starting-style. The settled cascade must keep opacity==1 and an empty
        // transform list — otherwise the box would render shrunk/invisible.
        let s = cascade_at(
            "<div class=\"box-a\"></div>",
            "@starting-style { .box-a { opacity: 0; transform: scale(0.5); } } \
             .box-a { opacity: 1; transition: opacity 0.4s, transform 0.4s; }",
            &[0],
        );
        assert!(
            (s.opacity - 1.0).abs() < 1e-6,
            "@starting-style opacity:0 leaked into static cascade, got {}",
            s.opacity
        );
        assert!(
            s.transform.is_empty(),
            "@starting-style transform:scale(0.5) leaked into static cascade: {:?}",
            s.transform
        );
    }

    #[test]
    fn at_property_initial_value_used_when_no_declaration() {
        // var(--c) без декларации, но --c зарегистрирована с initial-value.
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_property_inherits_false_blocks_inheritance() {
        // --c унаследовалось бы от :root, но `inherits: false` → потомок
        // его не видит и берёт initial-value (blue).
        let s = cascade_at(
            "<div><p>x</p></div>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: blue; } \
             div { --c: red; } \
             p { color: var(--c); }",
            &[0, 0],
        );
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn at_property_inherits_true_passes_to_child() {
        // С `inherits: true` — потомок видит родительское значение.
        let s = cascade_at(
            "<div><p>x</p></div>",
            "@property --c { syntax: \"*\"; inherits: true; initial-value: blue; } \
             div { --c: red; } \
             p { color: var(--c); }",
            &[0, 0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_property_local_declaration_overrides_initial() {
        // Локальная декларация --c=green побеждает initial-value=red.
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; } \
             p { --c: green; color: var(--c); }",
            &[0],
        );
        // CSS3 green = rgb(0, 128, 0).
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn at_property_without_initial_value_no_fallback() {
        // syntax="*" без initial-value: имя зарегистрировано (inherits:false),
        // но var(--c) не найдёт значения → declaration invalid, color остаётся
        // inherited (root() = black).
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn at_property_initial_value_visible_to_child_inherits_true() {
        // На корне нет декларации --c. Регистрация дала ему initial-value=red
        // и inherits:true. Дочерний `p` должен унаследовать initial-value
        // через стандартный наследование-каскад.
        let s = cascade_at(
            "<div><p>x</p></div>",
            "@property --c { syntax: \"*\"; inherits: true; initial-value: red; } \
             p { color: var(--c); }",
            &[0, 0],
        );
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn at_property_last_registration_wins() {
        // Две регистрации одного имени: последняя побеждает (HashMap insert
        // в `registry`-build перезапишет первую).
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; inherits: false; initial-value: red; } \
             @property --c { syntax: \"*\"; inherits: false; initial-value: green; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn invalid_at_property_does_not_register() {
        // @property без `inherits` — невалидно: имя не регистрируется, var()
        // без значения → declaration invalid → color остаётся inherited.
        let s = cascade_at(
            "<p>x</p>",
            "@property --c { syntax: \"*\"; initial-value: red; } \
             p { color: var(--c); }",
            &[0],
        );
        assert_eq!(s.color, Color::BLACK);
    }

    // ──────────────── CSS Values L4 §10 — calc() ────────────────

    fn resolved_calc(s: &str, em: f32, pb: Option<f32>, vp: Size) -> Option<f32> {
        let len = parse_length(s)?;
        len.resolve(em, pb, vp)
    }

    #[test]
    fn calc_simple_add_px() {
        let v = resolved_calc("calc(10px + 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn calc_simple_sub_px() {
        let v = resolved_calc("calc(50px - 8px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(42.0));
    }

    #[test]
    fn calc_mul_unitless_left() {
        let v = resolved_calc("calc(2 * 10px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn calc_mul_unitless_right() {
        let v = resolved_calc("calc(10px * 3)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn calc_div_by_unitless() {
        let v = resolved_calc("calc(20px / 4)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(5.0));
    }

    #[test]
    fn calc_div_by_zero_is_none() {
        let v = resolved_calc("calc(10px / 0)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, None);
    }

    #[test]
    fn calc_precedence_mul_before_add() {
        // 2 + 3 * 4 = 14 (не 20).
        let v = resolved_calc("calc(2px + 3 * 4px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(14.0));
    }

    #[test]
    fn calc_parens_override_precedence() {
        let v = resolved_calc("calc((2 + 3) * 4px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn calc_em_uses_em_basis() {
        // 2em = 2 * 24 = 48 при em_basis=24.
        let v = resolved_calc("calc(2em + 10px)", 24.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(58.0));
    }

    #[test]
    fn calc_rem_uses_root_fs() {
        // 1rem = 16; 1rem + 4 = 20.
        let v = resolved_calc("calc(1rem + 4px)", 24.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn calc_viewport_units() {
        // 100vw = 800, 50vh = 300 при viewport (800,600). 800 + 300 = 1100.
        let v = resolved_calc(
            "calc(100vw - 50vh)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(500.0)); // 800 - 300
    }

    #[test]
    fn calc_percent_uses_basis() {
        // 50% от 200 = 100; 100 - 10 = 90.
        let v = resolved_calc(
            "calc(50% - 10px)",
            16.0,
            Some(200.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(90.0));
    }

    #[test]
    fn calc_percent_without_basis_is_none() {
        // % без containing block — None (declaration ignored).
        let v = resolved_calc("calc(50% + 10px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, None);
    }

    #[test]
    fn calc_unary_negative() {
        // -10px + 20px = 10.
        let v = resolved_calc("calc(-10px + 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn calc_unary_negative_after_paren() {
        let v = resolved_calc("calc(20px + (-5px))", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(15.0));
    }

    #[test]
    fn calc_decimal_values() {
        let v = resolved_calc("calc(0.5 * 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn calc_case_insensitive_prefix() {
        // CSS keyword `calc` ASCII case-insensitive.
        let v = resolved_calc("CALC(5px + 5px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn calc_unknown_unit_invalid() {
        // `xyz` — completely unknown unit → None.
        assert!(parse_length("calc(10xyz + 5px)").is_none());
        // `pt` is now supported — calc with pt returns a Calc node.
        assert!(parse_length("calc(10pt + 5px)").is_some());
    }

    #[test]
    fn calc_in_width_property_applies() {
        // Интеграция: width: calc(10px * 2 + 20px) = 40px при layout-resolve.
        let s = style_for("width: calc(10px * 2 + 20px)");
        let vp = Size::new(800.0, 600.0);
        let resolved = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((resolved - 40.0).abs() < 0.01, "got {resolved}");
    }

    #[test]
    fn calc_in_padding_property_applies() {
        // padding shorthand берёт одно length — calc() даёт 5+3=8px при resolve.
        let s = style_for("padding: calc(5px + 3px)");
        let vp = Size::new(800.0, 600.0);
        assert!((s.padding_top.resolve_or_zero(16.0, 0.0, vp) - 8.0).abs() < 0.01);
        assert!((s.padding_right.resolve_or_zero(16.0, 0.0, vp) - 8.0).abs() < 0.01);
    }

    #[test]
    fn padding_two_values_shorthand() {
        // padding: vertical horizontal → top=bottom=8, left=right=12.
        let s = style_for("padding: 8px 12px");
        assert_eq!(s.padding_top, Length::Px(8.0), "top");
        assert_eq!(s.padding_right, Length::Px(12.0), "right");
        assert_eq!(s.padding_bottom, Length::Px(8.0), "bottom");
        assert_eq!(s.padding_left, Length::Px(12.0), "left");
    }

    #[test]
    fn padding_four_values_shorthand() {
        // padding: top right bottom left.
        let s = style_for("padding: 4px 8px 12px 16px");
        assert_eq!(s.padding_top, Length::Px(4.0), "top");
        assert_eq!(s.padding_right, Length::Px(8.0), "right");
        assert_eq!(s.padding_bottom, Length::Px(12.0), "bottom");
        assert_eq!(s.padding_left, Length::Px(16.0), "left");
    }

    #[test]
    fn margin_four_values_shorthand() {
        // margin: 0 6px 6px 0 — реальный CSS из графических тестов.
        let s = style_for("margin: 0 6px 6px 0");
        assert_eq!(s.margin_top, LengthOrAuto::ZERO, "top");
        assert_eq!(s.margin_right, LengthOrAuto::Length(Length::Px(6.0)), "right");
        assert_eq!(s.margin_bottom, LengthOrAuto::Length(Length::Px(6.0)), "bottom");
        assert_eq!(s.margin_left, LengthOrAuto::ZERO, "left");
    }

    #[test]
    fn calc_with_var_inside() {
        // var() сначала разворачивается → calc(10px + 5px), resolve = 15px.
        let s = style_for("--gap: 10px; padding: calc(var(--gap) + 5px)");
        let vp = Size::new(800.0, 600.0);
        assert!((s.padding_top.resolve_or_zero(16.0, 0.0, vp) - 15.0).abs() < 0.01);
    }

    #[test]
    fn calc_unbalanced_paren_invalid() {
        assert!(parse_length("calc(10px + 5px").is_none());
        assert!(parse_length("calc((10px + 5px)").is_none());
    }

    #[test]
    fn calc_empty_invalid() {
        assert!(parse_length("calc()").is_none());
    }

    // ──────────────── CSS Values L4 §10.6: min() / max() / clamp() ────────────────

    #[test]
    fn min_two_lengths_picks_smaller() {
        let v = resolved_calc("min(50px, 100px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn min_many_lengths() {
        let v = resolved_calc("min(30px, 10px, 20px, 5px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(5.0));
    }

    #[test]
    fn min_mixed_units_resolves_to_px() {
        // 2em = 32, 50% от 100 = 50, 24px → min = 24px.
        let v = resolved_calc(
            "min(2em, 50%, 24px)",
            16.0,
            Some(100.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(24.0));
    }

    #[test]
    fn max_picks_larger() {
        let v = resolved_calc("max(50px, 100px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(100.0));
    }

    #[test]
    fn max_with_viewport_unit() {
        // 100vw = 800; max(800, 200, 1000px) = 1000.
        let v = resolved_calc(
            "max(100vw, 200px, 1000px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(1000.0));
    }

    #[test]
    fn clamp_value_inside_range() {
        // clamp(10, 50, 100) = 50.
        let v = resolved_calc(
            "clamp(10px, 50px, 100px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn clamp_value_below_min() {
        // clamp(20, 5, 100) = 20 (min wins).
        let v = resolved_calc(
            "clamp(20px, 5px, 100px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn clamp_value_above_max() {
        // clamp(10, 200, 100) = 100 (max wins).
        let v = resolved_calc(
            "clamp(10px, 200px, 100px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(100.0));
    }

    #[test]
    fn clamp_min_greater_than_max() {
        // CSS spec: clamp(min, val, max) ≡ max(min, min(val, max)).
        // При min=50, max=10: inner=min(val, 10), max(50, inner) = 50.
        let v = resolved_calc(
            "clamp(50px, 30px, 10px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn min_max_nested_inside_calc() {
        // calc(10px + min(20px, 30px)) = 10 + 20 = 30.
        let v = resolved_calc(
            "calc(10px + min(20px, 30px))",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn calc_nested_inside_max() {
        // max(calc(10px * 2), 15px) = max(20, 15) = 20.
        let v = resolved_calc(
            "max(calc(10px * 2), 15px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn clamp_inside_min() {
        // min(clamp(10, 50, 100), 80) = min(50, 80) = 50.
        let v = resolved_calc(
            "min(clamp(10px, 50px, 100px), 80px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(50.0));
    }

    #[test]
    fn min_with_calc_expression_inside() {
        // min(2 * 10px, 30px) = min(20, 30) = 20.
        // Здесь `2 * 10px` это обычное calc-expression внутри min,
        // не требует обёртки calc(...).
        let v = resolved_calc(
            "min(2 * 10px, 30px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(20.0));
    }

    #[test]
    fn clamp_wrong_arg_count_invalid() {
        // clamp требует ровно 3 аргумента.
        assert!(parse_length("clamp(10px, 20px)").is_none());
        assert!(parse_length("clamp(10px, 20px, 30px, 40px)").is_none());
    }

    #[test]
    fn min_empty_invalid() {
        assert!(parse_length("min()").is_none());
    }

    #[test]
    fn max_empty_invalid() {
        assert!(parse_length("max()").is_none());
    }

    #[test]
    fn min_in_width_property_applies() {
        // width: min(50px, 200px) = 50px при resolve.
        let s = style_for("width: min(50px, 200px)");
        let vp = Size::new(800.0, 600.0);
        let v = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 50.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn clamp_in_width_property_applies() {
        // width: clamp(50px, 100px, 200px) = 100px при resolve.
        let s = style_for("width: clamp(50px, 100px, 200px)");
        let vp = Size::new(800.0, 600.0);
        let v = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 100.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn min_with_var_inside() {
        // var() → строка → min() работает; resolve даёт меньшее.
        let s = style_for("--w: 80px; width: min(var(--w), 50px)");
        let vp = Size::new(800.0, 600.0);
        let v = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 50.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn min_case_insensitive() {
        // CSS function names ASCII case-insensitive.
        let v = resolved_calc("MIN(10px, 20px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn unknown_function_invalid() {
        // Реально несуществующие функции → declaration invalid.
        // (sin/cos/abs реализованы — см. секцию scientific math funcs ниже).
        assert!(parse_length("xyzzy(45deg)").is_none());
        assert!(parse_length("nonexistent(10px)").is_none());
    }

    #[test]
    fn nested_calc_inside_calc() {
        // calc(calc(10px + 5px) * 2) = 30. Раньше nested calc был
        // отложен — теперь работает через function-call в factor.
        let v = resolved_calc(
            "calc(calc(10px + 5px) * 2)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(30.0));
    }

    // ──── CSS Values L4 §10.7-10.9: scientific math funcs ────

    fn rc_unitless(s: &str) -> Option<f32> {
        resolved_calc(s, 16.0, None, Size::new(800.0, 600.0))
    }

    // §10.7 trigonometry

    #[test]
    fn sin_radians_zero() {
        assert!(approx(rc_unitless("sin(0)").unwrap(), 0.0));
    }

    #[test]
    fn sin_45_degrees() {
        // sin(45deg) = √2/2 ≈ 0.7071.
        let v = rc_unitless("sin(45deg)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_1_SQRT_2), "got {v}");
    }

    #[test]
    fn cos_180_degrees() {
        let v = rc_unitless("cos(180deg)").unwrap();
        assert!(approx(v, -1.0), "got {v}");
    }

    #[test]
    fn cos_half_turn() {
        // 0.5turn = 180deg → cos = -1.
        let v = rc_unitless("cos(0.5turn)").unwrap();
        assert!(approx(v, -1.0), "got {v}");
    }

    #[test]
    fn tan_45_degrees() {
        let v = rc_unitless("tan(45deg)").unwrap();
        assert!(approx(v, 1.0), "got {v}");
    }

    #[test]
    fn asin_1_returns_radians() {
        // asin(1) = π/2 rad.
        let v = rc_unitless("asin(1)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_PI_2), "got {v}");
    }

    #[test]
    fn atan_one_returns_pi_quarter() {
        let v = rc_unitless("atan(1)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_PI_4), "got {v}");
    }

    #[test]
    fn atan2_y_x() {
        // atan2(1, 1) = π/4.
        let v = rc_unitless("atan2(1, 1)").unwrap();
        assert!(approx(v, std::f32::consts::FRAC_PI_4), "got {v}");
    }

    #[test]
    fn sin_unitless_is_radians() {
        // По CSS spec число без unit в sin — радианы.
        // sin(π/2) = 1.
        let v = rc_unitless("sin(1.5707963)").unwrap();
        assert!(approx(v, 1.0), "got {v}");
    }

    #[test]
    fn grad_unit_converts_to_radians() {
        // 200grad = π (полукруг). sin(π) ≈ 0.
        let v = rc_unitless("sin(200grad)").unwrap();
        assert!(v.abs() < 1e-4, "got {v}");
    }

    // §10.8 exponential

    #[test]
    fn pow_2_10() {
        assert!(approx(rc_unitless("pow(2, 10)").unwrap(), 1024.0));
    }

    #[test]
    fn sqrt_16() {
        assert!(approx(rc_unitless("sqrt(16)").unwrap(), 4.0));
    }

    #[test]
    fn sqrt_negative_returns_none() {
        // sqrt(-1) = NaN → None.
        assert_eq!(rc_unitless("sqrt(-1)"), None);
    }

    #[test]
    fn exp_zero_is_one() {
        assert!(approx(rc_unitless("exp(0)").unwrap(), 1.0));
    }

    #[test]
    fn log_e_is_one() {
        // log(e) с одним аргументом = ln(e) = 1.
        let v = rc_unitless(&format!("log({})", std::f32::consts::E)).unwrap();
        assert!(approx(v, 1.0), "got {v}");
    }

    #[test]
    fn log_base_2_of_8() {
        // log(8, 2) = 3.
        let v = rc_unitless("log(8, 2)").unwrap();
        assert!(approx(v, 3.0), "got {v}");
    }

    #[test]
    fn log_of_zero_returns_none() {
        // ln(0) = -∞ → not finite → None.
        assert_eq!(rc_unitless("log(0)"), None);
    }

    #[test]
    fn hypot_two_args_3_4() {
        // hypot(3, 4) = 5 (классический Pythagoras).
        assert!(approx(rc_unitless("hypot(3, 4)").unwrap(), 5.0));
    }

    #[test]
    fn hypot_variadic_three_args() {
        // hypot(2, 3, 6) = sqrt(4+9+36) = sqrt(49) = 7.
        assert!(approx(rc_unitless("hypot(2, 3, 6)").unwrap(), 7.0));
    }

    #[test]
    fn hypot_single_arg_is_abs() {
        // hypot(-5) = sqrt(25) = 5.
        assert!(approx(rc_unitless("hypot(-5)").unwrap(), 5.0));
    }

    // §10.9 sign / stepping

    #[test]
    fn abs_negative_to_positive() {
        let v = resolved_calc("abs(-10px)", 16.0, None, Size::new(800.0, 600.0));
        assert_eq!(v, Some(10.0));
    }

    #[test]
    fn abs_in_calc() {
        // calc(100px - abs(-20px)) = 100 - 20 = 80.
        let v = resolved_calc(
            "calc(100px - abs(-20px))",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(80.0));
    }

    #[test]
    fn sign_positive_negative_zero() {
        assert_eq!(rc_unitless("sign(5)"), Some(1.0));
        assert_eq!(rc_unitless("sign(-3)"), Some(-1.0));
        assert_eq!(rc_unitless("sign(0)"), Some(0.0));
    }

    #[test]
    fn mod_basic() {
        // 10 mod 3 = 1 (result имеет знак делителя).
        assert!(approx(rc_unitless("mod(10, 3)").unwrap(), 1.0));
    }

    #[test]
    fn mod_negative_dividend() {
        // mod(-1, 3) = 2 (CSS mod: знак делителя; -1 % 3 = -1, +3 = 2, %3 = 2).
        assert!(approx(rc_unitless("mod(-1, 3)").unwrap(), 2.0));
    }

    #[test]
    fn rem_negative_dividend() {
        // rem(-1, 3) = -1 (truncated remainder: знак делимого).
        assert!(approx(rc_unitless("rem(-1, 3)").unwrap(), -1.0));
    }

    #[test]
    fn mod_by_zero_invalid() {
        assert_eq!(rc_unitless("mod(10, 0)"), None);
    }

    #[test]
    fn round_to_integer() {
        assert!(approx(rc_unitless("round(3.7)").unwrap(), 4.0));
        assert!(approx(rc_unitless("round(3.4)").unwrap(), 3.0));
    }

    #[test]
    fn round_to_step() {
        // round(13, 5) = 15 (ближайшее кратное 5).
        assert!(approx(rc_unitless("round(13, 5)").unwrap(), 15.0));
        // round(12, 5) = 10.
        assert!(approx(rc_unitless("round(12, 5)").unwrap(), 10.0));
    }

    #[test]
    fn round_to_step_in_width() {
        // width: round(13px, 5px) = 15px при resolve.
        let s = style_for("width: round(13px, 5px)");
        let vp = Size::new(800.0, 600.0);
        let v = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 15.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn round_with_zero_step_invalid() {
        assert_eq!(rc_unitless("round(13, 0)"), None);
    }

    // CSS Values L4 §10.5.1 — strategy keyword (nearest/up/down/to-zero).

    #[test]
    fn round_up_to_integer() {
        // round(up, 3.1) = 4 — ceil дробного.
        assert!(approx(rc_unitless("round(up, 3.1)").unwrap(), 4.0));
        // round(up, 3.0) = 3 — целое не двигается.
        assert!(approx(rc_unitless("round(up, 3)").unwrap(), 3.0));
    }

    #[test]
    fn round_down_to_integer() {
        // round(down, 3.9) = 3 — floor дробного.
        assert!(approx(rc_unitless("round(down, 3.9)").unwrap(), 3.0));
    }

    #[test]
    fn round_to_zero_basic() {
        // round(to-zero, 3.9) = 3 — trunc положительного.
        assert!(approx(rc_unitless("round(to-zero, 3.9)").unwrap(), 3.0));
        // round(to-zero, -3.9) = -3 — отличается от floor(-3.9) = -4.
        assert!(approx(rc_unitless("round(to-zero, -3.9)").unwrap(), -3.0));
    }

    #[test]
    fn round_up_negative() {
        // round(up, -3.1) = -3 — ceil к +∞.
        assert!(approx(rc_unitless("round(up, -3.1)").unwrap(), -3.0));
    }

    #[test]
    fn round_down_negative() {
        // round(down, -3.1) = -4 — floor к -∞.
        assert!(approx(rc_unitless("round(down, -3.1)").unwrap(), -4.0));
    }

    #[test]
    fn round_nearest_explicit() {
        // Явный nearest эквивалентен без-strategy форме.
        assert!(approx(rc_unitless("round(nearest, 3.7)").unwrap(), 4.0));
        assert!(approx(rc_unitless("round(nearest, 3.4)").unwrap(), 3.0));
    }

    #[test]
    fn round_strategy_with_step() {
        // round(up, 13, 5) = 15 — ceil(13/5)*5 = 3*5.
        assert!(approx(rc_unitless("round(up, 13, 5)").unwrap(), 15.0));
        // round(down, 13, 5) = 10.
        assert!(approx(rc_unitless("round(down, 13, 5)").unwrap(), 10.0));
        // round(up, 11, 5) = 15.
        assert!(approx(rc_unitless("round(up, 11, 5)").unwrap(), 15.0));
        // round(to-zero, -11, 5) = -10 (vs down = -15).
        assert!(approx(rc_unitless("round(to-zero, -11, 5)").unwrap(), -10.0));
    }

    #[test]
    fn round_strategy_case_insensitive() {
        // Keyword-ы CSS-стандарт case-insensitive (Values L4 §2.4).
        assert!(approx(rc_unitless("round(UP, 3.1)").unwrap(), 4.0));
        assert!(approx(rc_unitless("round(To-Zero, -3.9)").unwrap(), -3.0));
    }

    #[test]
    fn round_strategy_in_width() {
        // width: round(up, 13px, 5px) = 15px при resolve.
        let s = style_for("width: round(up, 13px, 5px)");
        let vp = Size::new(800.0, 600.0);
        let v = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 15.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn round_strategy_zero_step_invalid() {
        // step=0 → declaration invalid, как и для round без strategy.
        assert_eq!(rc_unitless("round(up, 13, 0)"), None);
    }

    #[test]
    fn round_unknown_strategy_invalid() {
        // `floor` не keyword в strategy — declaration invalid.
        // (lexer пропустит ident `floor`, но parse_function_call для round
        // ждёт после ident либо `,` со strategy, либо expr; одинокий ident-без-`(`
        // в parse_calc_factor возвращает None.)
        assert_eq!(rc_unitless("round(floor, 3.7)"), None);
    }

    #[test]
    fn round_strategy_without_value_invalid() {
        // strategy + `,` + пусто → parse_arg_list падает.
        assert_eq!(rc_unitless("round(up,)"), None);
        // strategy без запятой → ident-arg в parse_calc_factor возвращает None.
        assert_eq!(rc_unitless("round(up 3.1)"), None);
    }

    // Интеграция

    #[test]
    fn math_func_nested_in_calc_and_min() {
        // min(abs(-50px), sqrt(900) * 1px) = min(50, 30) = 30.
        let v = resolved_calc(
            "min(abs(-50px), sqrt(900) * 1px)",
            16.0,
            None,
            Size::new(800.0, 600.0),
        );
        assert_eq!(v, Some(30.0));
    }

    #[test]
    fn pow_in_width_property() {
        // width: pow(2, 5) * 1px = 32px при resolve.
        let s = style_for("width: calc(pow(2, 5) * 1px)");
        let vp = Size::new(800.0, 600.0);
        let v = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 32.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn sin_with_var_arg() {
        // var() разворачивается до парсинга calc — sin принимает результат.
        let s = style_for("--a: 90deg; width: calc(sin(var(--a)) * 100px)");
        let vp = Size::new(800.0, 600.0);
        // sin(π/2) = 1, поэтому width = 100.
        let v = s.width.as_ref().unwrap().resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 100.0).abs() < 1e-3, "got {v}");
    }

    #[test]
    fn wrong_arity_invalid() {
        // sin требует ровно 1 аргумент.
        assert!(parse_length("sin(1, 2)").is_none());
        // pow требует ровно 2.
        assert!(parse_length("pow(2)").is_none());
        assert!(parse_length("pow(2, 3, 4)").is_none());
        // hypot — 1+, поэтому 0 — invalid.
        assert!(parse_length("hypot()").is_none());
    }

    #[test]
    fn math_func_case_insensitive() {
        // CSS function names ASCII case-insensitive.
        assert_eq!(rc_unitless("ABS(-5)"), Some(5.0));
        assert_eq!(rc_unitless("Sqrt(9)"), Some(3.0));
    }

    // ──────────── CSS Units ────────────

    #[test]
    fn pt_converts_to_px() {
        // 12pt = 12 * 4/3 = 16px
        let s = cascade_at("<div>", "div { width: 12pt; }", &[0]);
        assert_eq!(s.width, Some(Length::Px(16.0)));
    }

    #[test]
    fn pc_converts_to_px() {
        // 1pc = 16px
        let s = cascade_at("<div>", "div { width: 1pc; }", &[0]);
        assert_eq!(s.width, Some(Length::Px(16.0)));
    }

    #[test]
    fn in_converts_to_px() {
        // 1in = 96px
        let s = cascade_at("<div>", "div { width: 1in; }", &[0]);
        assert_eq!(s.width, Some(Length::Px(96.0)));
    }

    #[test]
    fn cm_converts_to_px() {
        // 2.54cm = 1in = 96px
        let s = cascade_at("<div>", "div { width: 2.54cm; }", &[0]);
        let w = s.width.unwrap();
        if let Length::Px(v) = w { assert!((v - 96.0).abs() < 0.1, "v={v}"); }
        else { panic!("expected Px, got {w:?}"); }
    }

    #[test]
    fn mm_converts_to_px() {
        // 25.4mm = 1in = 96px
        let s = cascade_at("<div>", "div { width: 25.4mm; }", &[0]);
        let w = s.width.unwrap();
        if let Length::Px(v) = w { assert!((v - 96.0).abs() < 0.5, "v={v}"); }
        else { panic!("expected Px, got {w:?}"); }
    }

    #[test]
    fn ch_approximated_as_half_em() {
        // Cascade stores the authored Ch unit verbatim (BUG-339: it used to fold
        // into Em at cascade time, but resolution moved to `Length::resolve`).
        // 2ch ≈ 1em is the spec §5.1.1 fallback applied there when FONT_CH_EX is
        // unset: at default font-size 16px, 2ch = 2 * 0.5 * 16 = 16px = 1em.
        pop_ch_ex_context(None);
        let s = cascade_at("<div>", "div { width: 2ch; }", &[0]);
        assert_eq!(s.width, Some(Length::Ch(2.0)));
        assert_eq!(s.width.unwrap().resolve(16.0, None, vp()), Some(16.0));
    }

    #[test]
    fn ex_approximated_as_half_em() {
        pop_ch_ex_context(None);
        let s = cascade_at("<div>", "div { width: 4ex; }", &[0]);
        assert_eq!(s.width, Some(Length::Ex(4.0)));
        assert_eq!(s.width.unwrap().resolve(16.0, None, vp()), Some(32.0));
    }

    #[test]
    fn svh_same_as_vh() {
        let a = cascade_at("<div>", "div { height: 50svh; }", &[0]);
        let b = cascade_at("<div>", "div { height: 50vh; }", &[0]);
        assert_eq!(a.height, b.height);
    }

    #[test]
    fn dvw_same_as_vw() {
        let a = cascade_at("<div>", "div { width: 30dvw; }", &[0]);
        let b = cascade_at("<div>", "div { width: 30vw; }", &[0]);
        assert_eq!(a.width, b.width);
    }

    #[test]
    fn calc_with_pt_unit() {
        // calc(12pt) = 16px
        let s = cascade_at("<div>", "div { width: calc(12pt); }", &[0]);
        let w = s.width.unwrap();
        if let Length::Calc(_) = &w {
            // Calc node stored — resolves at layout time, not checked here.
        } else {
            panic!("expected Calc, got {w:?}");
        }
    }

    #[test]
    fn calc_with_svh_unit() {
        let s = cascade_at("<div>", "div { height: calc(50svh); }", &[0]);
        assert!(matches!(s.height, Some(Length::Calc(_))));
    }

    // --- cq* units ---

    #[test]
    fn cq_units_parse() {
        let vp = Size::new(1024.0, 768.0);
        assert_eq!(parse_length("50cqw"), Some(Length::Cqw(50.0)));
        assert_eq!(parse_length("30cqh"), Some(Length::Cqh(30.0)));
        assert_eq!(parse_length("10cqi"), Some(Length::Cqi(10.0)));
        assert_eq!(parse_length("20cqb"), Some(Length::Cqb(20.0)));
        assert_eq!(parse_length("5cqmin"), Some(Length::Cqmin(5.0)));
        assert_eq!(parse_length("5cqmax"), Some(Length::Cqmax(5.0)));
        // Without container context, cq* resolve to None.
        assert_eq!(Length::Cqw(50.0).resolve(16.0, None, vp), None);
        assert_eq!(Length::Cqh(30.0).resolve(16.0, None, vp), None);
    }

    #[test]
    fn cq_units_resolve_with_context() {
        let vp = Size::new(1024.0, 768.0);
        // Set a container context: 800px wide, 600px tall (size container).
        set_cq_context(800.0, Some(600.0));

        assert_eq!(Length::Cqw(50.0).resolve(16.0, None, vp), Some(400.0)); // 50% of 800
        assert_eq!(Length::Cqi(10.0).resolve(16.0, None, vp), Some(80.0));  // 10% of 800
        assert_eq!(Length::Cqh(25.0).resolve(16.0, None, vp), Some(150.0)); // 25% of 600
        assert_eq!(Length::Cqb(50.0).resolve(16.0, None, vp), Some(300.0)); // 50% of 600
        assert_eq!(Length::Cqmin(10.0).resolve(16.0, None, vp), Some(60.0)); // 10% of min(800,600)
        assert_eq!(Length::Cqmax(10.0).resolve(16.0, None, vp), Some(80.0)); // 10% of max(800,600)

        clear_cq_context();
        // After clearing, cq* units return None again.
        assert_eq!(Length::Cqw(50.0).resolve(16.0, None, vp), None);
    }

    #[test]
    fn cq_units_inline_size_container() {
        let vp = Size::new(1024.0, 768.0);
        // inline-size container: block axis not queryable → height is None → stored as 0.0.
        set_cq_context(400.0, None);

        assert_eq!(Length::Cqw(25.0).resolve(16.0, None, vp), Some(100.0)); // 25% of 400
        // cqh / cqb / cqmin / cqmax return None when block size is unavailable.
        assert_eq!(Length::Cqh(50.0).resolve(16.0, None, vp), None);
        assert_eq!(Length::Cqb(50.0).resolve(16.0, None, vp), None);
        assert_eq!(Length::Cqmin(10.0).resolve(16.0, None, vp), None);
        assert_eq!(Length::Cqmax(10.0).resolve(16.0, None, vp), None);

        clear_cq_context();
    }

    #[test]
    fn cq_units_in_calc() {
        let vp = Size::new(1024.0, 768.0);
        // calc(50cqw + 20px) inside a 600px container → 300 + 20 = 320px.
        set_cq_context(600.0, Some(400.0));
        let calc_len = parse_length("calc(50cqw + 20px)").expect("calc parse");
        let px = calc_len.resolve(16.0, None, vp);
        assert_eq!(px, Some(320.0));
        clear_cq_context();
    }

    // BUG-020 regression: CSS Overflow L3 §2.1 axis coercion.
    #[test]
    fn overflow_axis_coercion_visible_plus_hidden() {
        // overflow-x: hidden; overflow-y: visible → overflow-y becomes auto.
        let (ox, oy) = coerce_overflow_axes(Overflow::Hidden, Overflow::Visible);
        assert_eq!(ox, Overflow::Hidden);
        assert_eq!(oy, Overflow::Auto);
        // overflow-x: visible; overflow-y: hidden → overflow-x becomes auto.
        let (ox, oy) = coerce_overflow_axes(Overflow::Visible, Overflow::Hidden);
        assert_eq!(ox, Overflow::Auto);
        assert_eq!(oy, Overflow::Hidden);
    }

    #[test]
    fn overflow_axis_coercion_both_visible_unchanged() {
        let (ox, oy) = coerce_overflow_axes(Overflow::Visible, Overflow::Visible);
        assert_eq!(ox, Overflow::Visible);
        assert_eq!(oy, Overflow::Visible);
    }

    #[test]
    fn overflow_axis_coercion_both_hidden_unchanged() {
        let (ox, oy) = coerce_overflow_axes(Overflow::Hidden, Overflow::Hidden);
        assert_eq!(ox, Overflow::Hidden);
        assert_eq!(oy, Overflow::Hidden);
    }

    #[test]
    fn overflow_axis_coercion_visible_plus_scroll() {
        let (ox, oy) = coerce_overflow_axes(Overflow::Visible, Overflow::Scroll);
        assert_eq!(ox, Overflow::Auto);
        assert_eq!(oy, Overflow::Scroll);
    }

    #[test]
    fn overflow_axis_coercion_visible_plus_auto() {
        let (ox, oy) = coerce_overflow_axes(Overflow::Auto, Overflow::Visible);
        assert_eq!(ox, Overflow::Auto);
        assert_eq!(oy, Overflow::Auto);
    }

    // === CSS Values L4 §7.7 attr() typed substitution ===

    fn make_doc_with_div(html: &str) -> (lumen_dom::Document, lumen_dom::NodeId) {
        let doc = lumen_html_parser::parse(html);
        let body = doc.body().expect("body");
        let node = doc.get(body).children[0];
        (doc, node)
    }

    #[test]
    fn attr_typed_width_px() {
        // attr(data-w px) with data-w="200" should set width to 200px.
        let (doc, node) = make_doc_with_div(r#"<div data-w="200"></div>"#);
        let sheet = lumen_css_parser::parse("div { width: attr(data-w px); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        assert_eq!(style.width, Some(Length::Px(200.0)), "width should be 200px via attr(data-w px)");
    }

    #[test]
    fn attr_typed_fallback_when_absent() {
        // attr(data-missing px, 50px) — attribute absent, fallback 50px used.
        let (doc, node) = make_doc_with_div("<div></div>");
        let sheet = lumen_css_parser::parse("div { width: attr(data-missing px, 50px); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        assert_eq!(style.width, Some(Length::Px(50.0)), "fallback 50px should apply when attr absent");
    }

    #[test]
    fn attr_typed_absent_no_fallback_skipped() {
        // attr(data-missing px) with no fallback — declaration invalid, width stays None.
        let (doc, node) = make_doc_with_div("<div></div>");
        let sheet = lumen_css_parser::parse("div { width: attr(data-missing px); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        assert_eq!(style.width, None, "absent attr without fallback should leave width at None");
    }

    #[test]
    fn attr_typed_color() {
        // attr(data-bg color) — attribute value used as CSS color for background-color.
        let (doc, node) = make_doc_with_div(r#"<div data-bg="red"></div>"#);
        let sheet = lumen_css_parser::parse("div { background-color: attr(data-bg color); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        // red = rgb(255, 0, 0)
        let bg = style.background_color.expect("background-color should be set via attr(data-bg color)");
        let CssColor::Rgba(c) = bg else { panic!("expected Rgba, got {:?}", bg) };
        assert_eq!(c.r, 255, "red component");
        assert_eq!(c.g, 0,   "green component");
        assert_eq!(c.b, 0,   "blue component");
    }

    #[test]
    fn css_function_direct_call_resolves() {
        // CSS Functions and Mixins L1 — a direct call in a property value
        // (`width: --double(10px);`) should bind the positional argument and
        // resolve `result:` via calc().
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --double(--x) { result: calc(var(--x) * 2); } \
             .box { width: --double(10px); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(20.0));
    }

    #[test]
    fn css_function_default_parameter_used_when_arg_omitted() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --pad(--n: 5px) { result: var(--n); } \
             .box { margin-left: --pad(); }",
            &[0],
        );
        assert_eq!(s.margin_left, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn css_function_call_through_custom_property_chain_resolves() {
        // A call reached indirectly through `var()` — the author computed a
        // custom property from a function call, then referenced it — must
        // resolve the same as a direct call.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --double(--x) { result: calc(var(--x) * 2); } \
             .box { --gap: --double(10px); width: var(--gap); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(20.0));
    }

    #[test]
    fn css_function_local_declaration_feeds_result() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --clamped(--min, --val, --max) { \
                 --c: clamp(var(--min), var(--val), var(--max)); \
                 result: var(--c); \
             } \
             .box { width: --clamped(10px, 5px, 50px); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(10.0));
    }

    #[test]
    fn css_function_missing_required_argument_invalidates_declaration() {
        // No default for `--x` and no argument supplied → invalid at
        // computed-value time, same treatment as an unresolvable `var()`.
        // `width` must stay unset (initial/inherited), not panic or use 0.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --double(--x) { result: calc(var(--x) * 2); } \
             .box { width: --double(); }",
            &[0],
        );
        assert_eq!(s.width, None);
    }

    #[test]
    fn css_function_unknown_call_invalidates_declaration() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            ".box { width: --not-defined(10px); }",
            &[0],
        );
        assert_eq!(s.width, None);
    }

    #[test]
    fn css_function_self_recursion_invalidates_instead_of_hanging() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --loop(--x) { result: --loop(var(--x)); } \
             .box { width: --loop(10px); }",
            &[0],
        );
        assert_eq!(s.width, None);
    }
