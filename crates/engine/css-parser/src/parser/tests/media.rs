use super::*;

    // ── Media Queries L4 §3.2: not / only / prefers-color-scheme ──

    fn screen_ctx(width: f32) -> MediaContext {
        MediaContext {
            media_type: "screen".into(),
            width,
            height: 600.0,
            prefers_dark: false,
            prefers_reduced_motion: false,
            forced_colors: false,
            ..Default::default()
        }
    }

    #[test]
    fn media_query_only_parses_as_no_op() {
        let q = parse_media_query("only screen and (min-width: 300px)");
        assert_eq!(q.clauses.len(), 1);
        assert!(!q.clauses[0].negated);
        // `only screen` + `and (min-width: 300px)` → 2 условия.
        assert_eq!(q.clauses[0].conditions.len(), 2);
        assert!(q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_only_keyword_does_not_eat_media_type() {
        // Forward-compat: `only` без следующего media-type / feature
        // оставляет clause пустым → Unsupported.
        let q = parse_media_query("only");
        assert_eq!(q.clauses.len(), 1);
        assert_eq!(q.clauses[0].conditions, vec![MediaCondition::Unsupported]);
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_not_inverts_match() {
        let q = parse_media_query("not screen");
        assert_eq!(q.clauses.len(), 1);
        assert!(q.clauses[0].negated);
        // screen-context — не матчит `not screen`.
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_not_matches_when_inner_false() {
        // not (min-width: 1000px) → инвертит «не достаточно широкий».
        let q = parse_media_query("not all and (min-width: 1000px)");
        assert!(q.clauses[0].negated);
        assert!(q.matches(&screen_ctx(500.0)));
        assert!(!q.matches(&screen_ctx(1200.0)));
    }

    #[test]
    fn media_query_not_with_unsupported_stays_unknown() {
        // Per §3.2: `not (unknown-feature: x)` → unknown, не true.
        let q = parse_media_query("not all and (gibberish: zzz)");
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_not_only_first_keyword_consumed() {
        // `not not` — второй not трактуется как невалидный токен → clause unknown.
        let q = parse_media_query("not not screen");
        assert!(q.clauses[0].negated);
        assert_eq!(q.clauses[0].conditions, vec![MediaCondition::Unsupported]);
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_or_with_not_clause() {
        // `not screen, print` — на screen НЕ должно матчить (not screen → false на screen);
        // на print должно матчить (print clause = MediaType(print)).
        let q = parse_media_query("not screen, print");
        assert_eq!(q.clauses.len(), 2);
        assert!(q.clauses[0].negated);
        assert!(!q.clauses[1].negated);
        assert!(!q.matches(&screen_ctx(500.0)));
        let mut print_ctx = screen_ctx(500.0);
        print_ctx.media_type = "print".into();
        assert!(q.matches(&print_ctx));
    }

    #[test]
    fn media_query_not_keyword_must_be_separated() {
        // `notepad` (или другой ident, начинающийся с `not`) — НЕ keyword.
        let q = parse_media_query("notepad");
        // Trim+lower → media-type "notepad". Не матчит на screen.
        assert!(!q.clauses[0].negated);
        assert_eq!(q.clauses[0].conditions.len(), 1);
    }

    #[test]
    fn media_query_bare_word_after_and_invalidates_clause() {
        // BUG-528: a second bare (non-parenthesized) word joined by `and`
        // is not a second media-type to test independently — it's a syntax
        // error, so the whole clause must go Unsupported (unknown, never
        // matches, `not` included) instead of silently ANDing in an
        // always-false extra condition that `not` then flips to true.
        let q = parse_media_query("not all and overflow-inline");
        assert!(q.clauses[0].negated);
        assert_eq!(q.clauses[0].conditions, vec![MediaCondition::Unsupported]);
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_comma_list_all_unparseable_entries_never_matches() {
        // Same shape as `matchmedia-utils.js`'s `query_should_be_unknown`
        // helper: `${query}, not all and ${query}` for an unrecognized
        // bare feature name. Neither comma-separated entry should match.
        let q = parse_media_query("overflow-inline, not all and overflow-inline");
        assert_eq!(q.clauses.len(), 2);
        assert!(!q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_prefers_color_scheme_light_default() {
        let q = parse_media_query("(prefers-color-scheme: light)");
        assert!(q.matches(&screen_ctx(500.0)));
    }

    #[test]
    fn media_query_prefers_color_scheme_dark_matches_when_dark() {
        let q = parse_media_query("(prefers-color-scheme: dark)");
        let mut ctx = screen_ctx(500.0);
        ctx.prefers_dark = true;
        assert!(q.matches(&ctx));
        ctx.prefers_dark = false;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_not_prefers_dark() {
        // На светлой теме `not (prefers-color-scheme: dark)` должно матчить.
        let q = parse_media_query("not all and (prefers-color-scheme: dark)");
        assert!(q.clauses[0].negated);
        assert!(q.matches(&screen_ctx(500.0)));
        let mut dark = screen_ctx(500.0);
        dark.prefers_dark = true;
        assert!(!q.matches(&dark));
    }

    // ── MQ L3 §4: exact width/height, em/rem units ──

    #[test]
    fn media_query_width_exact_px() {
        let q = parse_media_query("(width: 1024px)");
        let mut ctx = screen_ctx(1024.0);
        ctx.height = 720.0;
        assert!(q.matches(&ctx));
        ctx.width = 800.0;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_height_exact_px() {
        let q = parse_media_query("(height: 720px)");
        let mut ctx = screen_ctx(1024.0);
        ctx.height = 720.0;
        assert!(q.matches(&ctx));
        ctx.height = 600.0;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_min_width_em() {
        // 48em = 48 * 16 = 768px
        let q = parse_media_query("(min-width: 48em)");
        assert!(q.matches(&screen_ctx(1024.0)));
        assert!(!q.matches(&screen_ctx(600.0)));
    }

    #[test]
    fn media_query_max_width_rem() {
        // 50rem = 50 * 16 = 800px
        let q = parse_media_query("(max-width: 50rem)");
        assert!(q.matches(&screen_ctx(600.0)));
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_min_height_em() {
        // 30em = 30 * 16 = 480px
        let q = parse_media_query("(min-height: 30em)");
        let mut ctx = screen_ctx(800.0);
        ctx.height = 600.0;
        assert!(q.matches(&ctx));
        ctx.height = 400.0;
        assert!(!q.matches(&ctx));
    }

    // ── MQ L3 §4.3: aspect-ratio ──

    #[test]
    fn media_query_min_aspect_ratio() {
        // min-aspect-ratio: 16/9 ≈ 1.777; 1024/720 ≈ 1.422 → не матчит
        let q = parse_media_query("(min-aspect-ratio: 16/9)");
        let mut ctx = screen_ctx(1024.0);
        ctx.height = 720.0;
        assert!(!q.matches(&ctx)); // 1.422 < 1.777
        ctx.width = 1920.0;
        ctx.height = 720.0;
        assert!(q.matches(&ctx)); // 2.666 >= 1.777
    }

    #[test]
    fn media_query_max_aspect_ratio() {
        // max-aspect-ratio: 4/3 ≈ 1.333; 800/600 ≈ 1.333 → матчит
        let q = parse_media_query("(max-aspect-ratio: 4/3)");
        let mut ctx = screen_ctx(800.0);
        ctx.height = 600.0;
        assert!(q.matches(&ctx));
        ctx.width = 1920.0;
        assert!(!q.matches(&ctx)); // 3.2 > 1.333
    }

    #[test]
    fn media_query_aspect_ratio_exact() {
        // aspect-ratio: 1/1 → квадрат
        let q = parse_media_query("(aspect-ratio: 1/1)");
        let mut ctx = screen_ctx(600.0);
        ctx.height = 600.0;
        assert!(q.matches(&ctx));
        ctx.width = 800.0;
        assert!(!q.matches(&ctx));
    }

    // ── BUG-526: Media Queries L4 §Serializing a media query list ──
    // Транскрипция подтестов `match-media-parsing.html`/
    // `aspect-ratio-serialization.html` (см. bugs/BUG-526-OPEN.md).

    #[test]
    fn media_query_serialize_empty() {
        assert_eq!(parse_media_query("").serialize(), "");
        assert_eq!(parse_media_query("  ").serialize(), "");
    }

    #[test]
    fn media_query_serialize_trims_and_lowercases_media_type() {
        assert_eq!(parse_media_query("all").serialize(), "all");
        assert_eq!(parse_media_query(" all").serialize(), "all");
        assert_eq!(parse_media_query("   all   ").serialize(), "all");
        assert_eq!(parse_media_query(" foo ").serialize(), "foo");
    }

    #[test]
    fn media_query_serialize_comma_list_normalizes_spacing() {
        assert_eq!(parse_media_query("all,all").serialize(), "all, all");
        assert_eq!(parse_media_query(" all , all ").serialize(), "all, all");
    }

    #[test]
    fn media_query_serialize_empty_clauses_become_not_all() {
        assert_eq!(parse_media_query(",").serialize(), "not all, not all");
        assert_eq!(parse_media_query(" , ").serialize(), "not all, not all");
        assert_eq!(
            parse_media_query(",,").serialize(),
            "not all, not all, not all"
        );
        assert_eq!(parse_media_query(" foo,").serialize(), "foo, not all");
    }

    #[test]
    fn media_query_serialize_feature_round_trips_canonical_form() {
        assert_eq!(
            parse_media_query("(min-width: 500px)").serialize(),
            "(min-width: 500px)"
        );
        assert_eq!(
            parse_media_query("( min-width:  500px )").serialize(),
            "(min-width: 500px)"
        );
    }

    #[test]
    fn media_query_serialize_and_list_joined_with_and() {
        assert_eq!(
            parse_media_query("screen and (min-width: 500px)").serialize(),
            "screen and (min-width: 500px)"
        );
    }

    #[test]
    fn media_query_serialize_not_and_only_prefixes_preserved() {
        assert_eq!(parse_media_query("not screen").serialize(), "not screen");
        assert_eq!(parse_media_query("only screen").serialize(), "only screen");
    }

    #[test]
    fn media_query_serialize_aspect_ratio_adds_spacing_around_slash() {
        // aspect-ratio-serialization.html: `1/3` → `1 / 3` (числитель и
        // знаменатель хранятся раздельно, не пересчитанной дробью).
        assert_eq!(
            parse_media_query("(aspect-ratio: 1/3)").serialize(),
            "(aspect-ratio: 1 / 3)"
        );
        assert_eq!(
            parse_media_query("(min-aspect-ratio: 16/9)").serialize(),
            "(min-aspect-ratio: 16 / 9)"
        );
    }

    // ── MQ L4: resolution/min-resolution/max-resolution (BUG-1019) ──
    // Транскрипция `match-media-parsing.html::test_resolution_parsing`
    // построчно.

    #[test]
    fn media_query_resolution_units_round_trip() {
        assert_eq!(
            parse_media_query("(min-resolution: 1x)").serialize(),
            "(min-resolution: 1dppx)"
        );
        assert_eq!(
            parse_media_query("(resolution: 2x)").serialize(),
            "(resolution: 2dppx)"
        );
        assert_eq!(
            parse_media_query("(max-resolution: 7x)").serialize(),
            "(max-resolution: 7dppx)"
        );
        assert_eq!(
            parse_media_query("(resolution: 2dppx)").serialize(),
            "(resolution: 2dppx)"
        );
    }

    #[test]
    fn media_query_resolution_dpi_dpcm_convert_to_dppx() {
        // 600dpi / 96 = 6.25dppx
        assert_eq!(
            parse_media_query("(resolution: 600dpi)").serialize(),
            "(resolution: 6.25dppx)"
        );
        // 77dpcm * 2.54 / 96 ≈ 2.0372918dppx
        let q = parse_media_query("(resolution: 77dpcm)");
        match &q.clauses[0].conditions[0] {
            MediaCondition::Feature(MediaFeature::Resolution(v)) => {
                assert!((v.dppx() - 2.037_291_8).abs() < 0.0001);
            }
            other => panic!("expected Resolution feature, got {other:?}"),
        }
    }

    #[test]
    fn media_query_resolution_calc_keeps_calc_wrapper_after_serializing() {
        // calc() collapses to one number but the WPT-expected serialization
        // still wraps it in `calc(...)` — a bare `(resolution: 3dppx)` is
        // wrong even though the numeric value is identical.
        assert_eq!(
            parse_media_query("(min-resolution: calc(1x))").serialize(),
            "(min-resolution: calc(1dppx))"
        );
        assert_eq!(
            parse_media_query("(resolution: calc(2x))").serialize(),
            "(resolution: calc(2dppx))"
        );
        assert_eq!(
            parse_media_query("(max-resolution: calc(7x))").serialize(),
            "(max-resolution: calc(7dppx))"
        );
    }

    #[test]
    fn media_query_resolution_calc_arithmetic() {
        assert_eq!(
            parse_media_query("(resolution: calc(1x + 2x))").serialize(),
            "(resolution: calc(3dppx))"
        );
        assert_eq!(
            parse_media_query("(resolution: calc(5x - 2x))").serialize(),
            "(resolution: calc(3dppx))"
        );
        assert_eq!(
            parse_media_query("(resolution: calc(1x * 3))").serialize(),
            "(resolution: calc(3dppx))"
        );
        assert_eq!(
            parse_media_query("(resolution: calc(6x / 2))").serialize(),
            "(resolution: calc(3dppx))"
        );
    }

    #[test]
    fn media_query_resolution_matches_context() {
        let q = parse_media_query("(min-resolution: 2dppx)");
        let mut ctx = screen_ctx(1024.0);
        ctx.resolution_dppx = 1.0;
        assert!(!q.matches(&ctx));
        ctx.resolution_dppx = 2.0;
        assert!(q.matches(&ctx));
        ctx.resolution_dppx = 3.0;
        assert!(q.matches(&ctx));

        let q = parse_media_query("(max-resolution: 2dppx)");
        assert!(!q.matches(&ctx));
        ctx.resolution_dppx = 1.5;
        assert!(q.matches(&ctx));

        let q = parse_media_query("(resolution: 1.5dppx)");
        assert!(q.matches(&ctx));
    }

    #[test]
    fn media_query_resolution_unknown_unit_is_unsupported() {
        let q = parse_media_query("(resolution: 2foo)");
        assert_eq!(q.serialize(), "not all");
    }

    // ── MQ L4: boolean-context features, unclosed parens, bare-word
    // tokenization stopping at `)` (BUG-1020) ──
    // Транскрипция `match-media-parsing.html` построчно (весь файл, не
    // только 7 ранее падавших сабтестов — покрывает и уже проходившие
    // случаи, чтобы фикс не мог их тихо сломать).

    #[test]
    fn media_query_parsing_empty_and_all() {
        assert_eq!(parse_media_query("").serialize(), "");
        assert_eq!(parse_media_query(" ").serialize(), "");
        assert_eq!(parse_media_query("all").serialize(), "all");
        assert_eq!(parse_media_query(" all").serialize(), "all");
        assert_eq!(parse_media_query("   all   ").serialize(), "all");
        assert_eq!(parse_media_query("all,all").serialize(), "all, all");
        assert_eq!(parse_media_query(" all , all ").serialize(), "all, all");
    }

    #[test]
    fn media_query_parsing_boolean_context_color_and_unclosed_parens() {
        assert_eq!(parse_media_query("(color)").serialize(), "(color)");
        assert_eq!(parse_media_query("(color").serialize(), "(color)");
        assert_eq!(parse_media_query(" (color)").serialize(), "(color)");
        assert_eq!(parse_media_query(" ( color  )  ").serialize(), "(color)");
        assert_eq!(parse_media_query(" ( color   ").serialize(), "(color)");
    }

    #[test]
    fn media_query_parsing_stray_close_paren_is_invalid() {
        assert_eq!(parse_media_query("color)").serialize(), "not all");
        assert_eq!(parse_media_query("  color)").serialize(), "not all");
        assert_eq!(
            parse_media_query("  color ), ( color").serialize(),
            "not all, (color)"
        );
    }

    #[test]
    fn media_query_parsing_bare_words_and_empty_clauses() {
        assert_eq!(parse_media_query(" foo ").serialize(), "foo");
        assert_eq!(parse_media_query(",").serialize(), "not all, not all");
        assert_eq!(parse_media_query(" , ").serialize(), "not all, not all");
        assert_eq!(parse_media_query(",,").serialize(), "not all, not all, not all");
        assert_eq!(
            parse_media_query("  ,  ,  ").serialize(),
            "not all, not all, not all"
        );
        assert_eq!(parse_media_query(" foo,").serialize(), "foo, not all");
    }

    #[test]
    fn media_query_boolean_color_always_matches() {
        let q = parse_media_query("(color)");
        assert!(q.matches(&screen_ctx(1024.0)));
    }

    // ── BUG-527: boolean context реализован для discrete-фич (`color`,
    // `scripting`, `prefers-*`, `hover`/`pointer`, новые L4/L5-фичи ниже) —
    // range-фичи (`width`/`height`/`resolution`/`aspect-ratio`) требуют
    // отдельной `<`/`<=`/`>`/`>=`-grammar и остаются Unsupported. Регресс-тест
    // на то, что скоуп фикса не расширился неявно на них.
    #[test]
    fn media_query_boolean_context_width_still_unsupported() {
        let q = parse_media_query("(width)");
        assert_eq!(q.serialize(), "not all");
    }

    // ── MQ L5 §6.4: prefers-reduced-motion ──

    #[test]
    fn media_query_prefers_reduced_motion_reduce() {
        let q = parse_media_query("(prefers-reduced-motion: reduce)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_reduced_motion = true;
        assert!(q.matches(&ctx));
        ctx.prefers_reduced_motion = false;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_prefers_reduced_motion_no_preference() {
        let q = parse_media_query("(prefers-reduced-motion: no-preference)");
        let ctx = screen_ctx(1024.0); // prefers_reduced_motion = false по умолчанию
        assert!(q.matches(&ctx));
    }

    // ── MQ: forced-colors (CSS Forced Colors Mode L1) ──

    #[test]
    fn media_query_forced_colors_active() {
        let q = parse_media_query("(forced-colors: active)");
        let mut ctx = screen_ctx(1024.0);
        ctx.forced_colors = true;
        assert!(q.matches(&ctx));
        ctx.forced_colors = false;
        assert!(!q.matches(&ctx));
    }

    #[test]
    fn media_query_forced_colors_none() {
        let q = parse_media_query("(forced-colors: none)");
        let ctx = screen_ctx(1024.0); // forced_colors = false по умолчанию
        assert!(q.matches(&ctx));
        let mut active = screen_ctx(1024.0);
        active.forced_colors = true;
        assert!(!q.matches(&active));
    }

    #[test]
    fn media_query_not_forced_colors_active() {
        let q = parse_media_query("not all and (forced-colors: active)");
        assert!(q.clauses[0].negated);
        let ctx = screen_ctx(1024.0); // forced_colors = false
        assert!(q.matches(&ctx));
        let mut active = screen_ctx(1024.0);
        active.forced_colors = true;
        assert!(!q.matches(&active));
    }

    #[test]
    fn media_query_forced_colors_case_insensitive() {
        let q = parse_media_query("(forced-colors: ACTIVE)");
        let mut ctx = screen_ctx(1024.0);
        ctx.forced_colors = true;
        assert!(q.matches(&ctx));
    }

    // ── MQ: hover / any-hover / pointer / any-pointer (Media Queries L4 §5.3-5.6) ──

    #[test]
    fn media_query_hover_hover_matches_desktop() {
        // screen_ctx наследует desktop-дефолты (hover: Hover).
        let q = parse_media_query("(hover: hover)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut touch = screen_ctx(1024.0);
        touch.hover = MediaHover::None;
        assert!(!q.matches(&touch));
    }

    #[test]
    fn media_query_hover_none() {
        let q = parse_media_query("(hover: none)");
        assert!(!q.matches(&screen_ctx(1024.0)));
        let mut touch = screen_ctx(1024.0);
        touch.hover = MediaHover::None;
        assert!(q.matches(&touch));
    }

    #[test]
    fn media_query_any_hover() {
        let q = parse_media_query("(any-hover: hover)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut touch = screen_ctx(1024.0);
        touch.any_hover = MediaHover::None;
        assert!(!q.matches(&touch));
    }

    #[test]
    fn media_query_pointer_fine_matches_desktop() {
        let q = parse_media_query("(pointer: fine)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut coarse = screen_ctx(1024.0);
        coarse.pointer = MediaPointer::Coarse;
        assert!(!q.matches(&coarse));
    }

    #[test]
    fn media_query_pointer_coarse_and_none() {
        let coarse_q = parse_media_query("(pointer: coarse)");
        let none_q = parse_media_query("(pointer: none)");
        let mut ctx = screen_ctx(1024.0);
        ctx.pointer = MediaPointer::Coarse;
        assert!(coarse_q.matches(&ctx));
        assert!(!none_q.matches(&ctx));
        ctx.pointer = MediaPointer::None;
        assert!(none_q.matches(&ctx));
    }

    #[test]
    fn media_query_any_pointer() {
        let q = parse_media_query("(any-pointer: fine)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut coarse = screen_ctx(1024.0);
        coarse.any_pointer = MediaPointer::Coarse;
        assert!(!q.matches(&coarse));
    }

    #[test]
    fn media_query_hover_pointer_case_insensitive() {
        let q = parse_media_query("(POINTER: FINE)");
        assert!(q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_pointer_invalid_value_unsupported() {
        // Невалидное значение → Unsupported → clause никогда не матчит.
        let q = parse_media_query("(pointer: medium)");
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    // ── MQ L5 §5.5/§5.6: prefers-contrast / prefers-reduced-data ──

    #[test]
    fn media_query_prefers_contrast_no_preference_default() {
        // screen_ctx наследует desktop-дефолт (no-preference).
        let q = parse_media_query("(prefers-contrast: no-preference)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut more = screen_ctx(1024.0);
        more.prefers_contrast = MediaContrast::More;
        assert!(!q.matches(&more));
    }

    #[test]
    fn media_query_prefers_contrast_more_and_less() {
        let more_q = parse_media_query("(prefers-contrast: more)");
        let less_q = parse_media_query("(prefers-contrast: less)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_contrast = MediaContrast::More;
        assert!(more_q.matches(&ctx));
        assert!(!less_q.matches(&ctx));
        ctx.prefers_contrast = MediaContrast::Less;
        assert!(less_q.matches(&ctx));
        assert!(!more_q.matches(&ctx));
    }

    #[test]
    fn media_query_prefers_contrast_custom() {
        let q = parse_media_query("(prefers-contrast: custom)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_contrast = MediaContrast::Custom;
        assert!(q.matches(&ctx));
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_prefers_contrast_case_insensitive_and_invalid() {
        let q = parse_media_query("(PREFERS-CONTRAST: MORE)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_contrast = MediaContrast::More;
        assert!(q.matches(&ctx));
        // Невалидное значение → Unsupported → никогда не матчит.
        let bad = parse_media_query("(prefers-contrast: high)");
        assert!(!bad.matches(&ctx));
    }

    #[test]
    fn media_query_prefers_reduced_data_reduce() {
        let q = parse_media_query("(prefers-reduced-data: reduce)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_reduced_data = MediaReducedData::Reduce;
        assert!(q.matches(&ctx));
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_prefers_reduced_data_no_preference_default() {
        let q = parse_media_query("(prefers-reduced-data: no-preference)");
        // Desktop-дефолт — no-preference.
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut reduce = screen_ctx(1024.0);
        reduce.prefers_reduced_data = MediaReducedData::Reduce;
        assert!(!q.matches(&reduce));
    }

    // ── MQ L5 §5.7: prefers-reduced-transparency ──

    #[test]
    fn media_query_prefers_reduced_transparency_reduce() {
        let q = parse_media_query("(prefers-reduced-transparency: reduce)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_reduced_transparency = MediaReducedTransparency::Reduce;
        assert!(q.matches(&ctx));
        // Desktop-дефолт — no-preference → не матчит reduce.
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_prefers_reduced_transparency_no_preference_default() {
        let q = parse_media_query("(prefers-reduced-transparency: no-preference)");
        // Desktop-дефолт — no-preference.
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut reduce = screen_ctx(1024.0);
        reduce.prefers_reduced_transparency = MediaReducedTransparency::Reduce;
        assert!(!q.matches(&reduce));
    }

    #[test]
    fn media_query_prefers_reduced_transparency_case_insensitive_and_invalid() {
        let q = parse_media_query("(PREFERS-REDUCED-TRANSPARENCY: REDUCE)");
        let mut ctx = screen_ctx(1024.0);
        ctx.prefers_reduced_transparency = MediaReducedTransparency::Reduce;
        assert!(q.matches(&ctx));
        // Невалидное значение → Unsupported → никогда не матчит.
        let bad = parse_media_query("(prefers-reduced-transparency: low)");
        assert!(!bad.matches(&ctx));
    }

    // ── MQ L5 §6.2: scripting ──

    #[test]
    fn media_query_scripting_enabled_default() {
        // Desktop-дефолт Lumen — scripting: enabled (есть QuickJS).
        let q = parse_media_query("(scripting: enabled)");
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut none = screen_ctx(1024.0);
        none.scripting = MediaScripting::None;
        assert!(!q.matches(&none));
    }

    #[test]
    fn media_query_scripting_none() {
        let q = parse_media_query("(scripting: none)");
        // Дефолт enabled → не матчит none.
        assert!(!q.matches(&screen_ctx(1024.0)));
        let mut none = screen_ctx(1024.0);
        none.scripting = MediaScripting::None;
        assert!(q.matches(&none));
    }

    #[test]
    fn media_query_scripting_initial_only() {
        let q = parse_media_query("(scripting: initial-only)");
        assert!(!q.matches(&screen_ctx(1024.0)));
        let mut io = screen_ctx(1024.0);
        io.scripting = MediaScripting::InitialOnly;
        assert!(q.matches(&io));
    }

    #[test]
    fn media_query_scripting_case_insensitive_and_invalid() {
        // Регистр ключа/значения не важен.
        let q = parse_media_query("(SCRIPTING: ENABLED)");
        assert!(q.matches(&screen_ctx(1024.0)));
        // Невалидное значение → Unsupported → никогда не матчит.
        let bad = parse_media_query("(scripting: sometimes)");
        assert!(!bad.matches(&screen_ctx(1024.0)));
    }

    // ── MQ L5 §5.8: inverted-colors ──

    #[test]
    fn media_query_inverted_colors_inverted() {
        let q = parse_media_query("(inverted-colors: inverted)");
        let mut ctx = screen_ctx(1024.0);
        ctx.inverted_colors = MediaInvertedColors::Inverted;
        assert!(q.matches(&ctx));
        // Desktop-дефолт — none → не матчит inverted.
        assert!(!q.matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_inverted_colors_none_default() {
        let q = parse_media_query("(inverted-colors: none)");
        // Desktop-дефолт — none.
        assert!(q.matches(&screen_ctx(1024.0)));
        let mut inv = screen_ctx(1024.0);
        inv.inverted_colors = MediaInvertedColors::Inverted;
        assert!(!q.matches(&inv));
    }

    #[test]
    fn media_query_inverted_colors_case_insensitive_and_invalid() {
        let q = parse_media_query("(INVERTED-COLORS: INVERTED)");
        let mut ctx = screen_ctx(1024.0);
        ctx.inverted_colors = MediaInvertedColors::Inverted;
        assert!(q.matches(&ctx));
        // Невалидное значение → Unsupported → никогда не матчит.
        let bad = parse_media_query("(inverted-colors: maybe)");
        assert!(!bad.matches(&ctx));
    }

    // ── BUG-527: display-mode / display-state / resizable /
    // dynamic-range / video-dynamic-range / update / navigation-controls /
    // overflow-inline / overflow-block — value form ──

    #[test]
    fn media_query_display_mode_value_form() {
        let q = parse_media_query("(display-mode: browser)");
        assert!(q.matches(&screen_ctx(1024.0))); // desktop-дефолт — browser
        let standalone = parse_media_query("(display-mode: standalone)");
        assert!(!standalone.matches(&screen_ctx(1024.0)));
        assert_eq!(parse_media_query("(display-mode: bogus)").serialize(), "not all");
    }

    #[test]
    fn media_query_display_state_value_form() {
        let q = parse_media_query("(display-state: normal)");
        assert!(q.matches(&screen_ctx(1024.0))); // desktop-дефолт — normal
        let maximized = parse_media_query("(display-state: maximized)");
        assert!(!maximized.matches(&screen_ctx(1024.0)));
        assert_eq!(parse_media_query("(display-state: bogus)").serialize(), "not all");
    }

    #[test]
    fn media_query_resizable_value_form() {
        let t = parse_media_query("(resizable: true)");
        assert!(t.matches(&screen_ctx(1024.0))); // desktop-дефолт — resizable
        let f = parse_media_query("(resizable: false)");
        assert!(!f.matches(&screen_ctx(1024.0)));
        assert_eq!(parse_media_query("(resizable: 1)").serialize(), "not all");
    }

    #[test]
    fn media_query_dynamic_range_value_form() {
        let standard = parse_media_query("(dynamic-range: standard)");
        assert!(standard.matches(&screen_ctx(1024.0))); // desktop-дефолт — standard, нет HDR
        let high = parse_media_query("(dynamic-range: high)");
        assert!(!high.matches(&screen_ctx(1024.0)));
        let video_standard = parse_media_query("(video-dynamic-range: standard)");
        assert!(video_standard.matches(&screen_ctx(1024.0)));
        assert_eq!(parse_media_query("(dynamic-range: invalid)").serialize(), "not all");
    }

    #[test]
    fn media_query_update_value_form() {
        assert!(parse_media_query("(update: fast)").matches(&screen_ctx(1024.0))); // дефолт
        assert!(!parse_media_query("(update: slow)").matches(&screen_ctx(1024.0)));
        assert!(!parse_media_query("(update: none)").matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_navigation_controls_value_form() {
        assert!(parse_media_query("(navigation-controls: back-button)").matches(&screen_ctx(1024.0)));
        assert!(!parse_media_query("(navigation-controls: none)").matches(&screen_ctx(1024.0)));
        // Пробел вместо одного значения — не грамматика этой фичи.
        assert_eq!(
            parse_media_query("(navigation-controls: none back-button)").serialize(),
            "not all"
        );
    }

    #[test]
    fn media_query_overflow_inline_and_block_value_form() {
        assert!(parse_media_query("(overflow-inline: scroll)").matches(&screen_ctx(1024.0)));
        assert!(!parse_media_query("(overflow-inline: none)").matches(&screen_ctx(1024.0)));
        assert!(parse_media_query("(overflow-block: scroll)").matches(&screen_ctx(1024.0)));
        assert!(!parse_media_query("(overflow-block: none)").matches(&screen_ctx(1024.0)));
        assert!(!parse_media_query("(overflow-block: paged)").matches(&screen_ctx(1024.0)));
        assert_eq!(parse_media_query("(overflow-inline: 0)").serialize(), "not all");
    }

    // ── BUG-527: boolean context для уже реализованных discrete-фич ──

    #[test]
    fn media_query_boolean_context_scripting_enabled_default() {
        // `(scripting)` должно быть known (не Unsupported) и матчить, раз
        // desktop-дефолт — scripting: enabled.
        assert_eq!(parse_media_query("(scripting)").serialize(), "(scripting)");
        assert!(parse_media_query("(scripting)").matches(&screen_ctx(1024.0)));
        let mut none = screen_ctx(1024.0);
        none.scripting = MediaScripting::None;
        assert!(!parse_media_query("(scripting)").matches(&none));
    }

    #[test]
    fn media_query_boolean_context_prefers_color_scheme_always_true() {
        // Нет "off"-состояния (light/dark оба содержательны) — всегда true,
        // как orientation.
        assert!(parse_media_query("(prefers-color-scheme)").matches(&screen_ctx(1024.0)));
    }

    #[test]
    fn media_query_boolean_context_forced_colors_inverted_colors() {
        let mut ctx = screen_ctx(1024.0);
        assert!(!parse_media_query("(forced-colors)").matches(&ctx));
        ctx.forced_colors = true;
        assert!(parse_media_query("(forced-colors)").matches(&ctx));

        let mut ctx = screen_ctx(1024.0);
        assert!(!parse_media_query("(inverted-colors)").matches(&ctx));
        ctx.inverted_colors = MediaInvertedColors::Inverted;
        assert!(parse_media_query("(inverted-colors)").matches(&ctx));
    }

    #[test]
    fn media_query_boolean_context_preference_features_false_by_default() {
        // Все *no-preference*-дефолтные фичи → boolean false, пока
        // пользователь явно не запросил предпочтение.
        let ctx = screen_ctx(1024.0);
        assert!(!parse_media_query("(prefers-reduced-data)").matches(&ctx));
        assert!(!parse_media_query("(prefers-contrast)").matches(&ctx));
        assert!(!parse_media_query("(prefers-reduced-motion)").matches(&ctx));
        assert!(!parse_media_query("(prefers-reduced-transparency)").matches(&ctx));

        let mut reduce = ctx.clone();
        reduce.prefers_reduced_data = MediaReducedData::Reduce;
        assert!(parse_media_query("(prefers-reduced-data)").matches(&reduce));

        let mut contrast = ctx.clone();
        contrast.prefers_contrast = MediaContrast::More;
        assert!(parse_media_query("(prefers-contrast)").matches(&contrast));

        let mut motion = ctx.clone();
        motion.prefers_reduced_motion = true;
        assert!(parse_media_query("(prefers-reduced-motion)").matches(&motion));

        let mut transparency = ctx;
        transparency.prefers_reduced_transparency = MediaReducedTransparency::Reduce;
        assert!(parse_media_query("(prefers-reduced-transparency)").matches(&transparency));
    }

    #[test]
    fn media_query_boolean_context_hover_and_pointer() {
        let ctx = screen_ctx(1024.0); // hover/pointer desktop-дефолты — Hover/Fine
        assert!(parse_media_query("(hover)").matches(&ctx));
        assert!(parse_media_query("(any-hover)").matches(&ctx));
        assert!(parse_media_query("(pointer)").matches(&ctx));
        assert!(parse_media_query("(any-pointer)").matches(&ctx));

        let mut touch = ctx;
        touch.hover = MediaHover::None;
        touch.any_hover = MediaHover::None;
        touch.pointer = MediaPointer::None;
        touch.any_pointer = MediaPointer::None;
        assert!(!parse_media_query("(hover)").matches(&touch));
        assert!(!parse_media_query("(any-hover)").matches(&touch));
        assert!(!parse_media_query("(pointer)").matches(&touch));
        assert!(!parse_media_query("(any-pointer)").matches(&touch));
    }

    #[test]
    fn media_query_boolean_context_new_features() {
        let ctx = screen_ctx(1024.0);
        assert!(parse_media_query("(display-mode)").matches(&ctx));
        assert!(parse_media_query("(display-state)").matches(&ctx));
        assert!(parse_media_query("(resizable)").matches(&ctx));
        assert!(parse_media_query("(update)").matches(&ctx));
        assert!(parse_media_query("(navigation-controls)").matches(&ctx));
        assert!(parse_media_query("(overflow-inline)").matches(&ctx));
        assert!(parse_media_query("(overflow-block)").matches(&ctx));
        // dynamic-range/video-dynamic-range — spec-мандатное исключение:
        // boolean тестирует наличие HDR (`high`), не просто «фича известна».
        // Lumen не поддерживает HDR → всегда false, хотя value-форма known.
        assert!(!parse_media_query("(dynamic-range)").matches(&ctx));
        assert!(!parse_media_query("(video-dynamic-range)").matches(&ctx));
    }

    #[test]
    fn media_query_boolean_context_serializes_as_bare_name() {
        for query in [
            "(display-mode)",
            "(display-state)",
            "(resizable)",
            "(dynamic-range)",
            "(video-dynamic-range)",
            "(update)",
            "(navigation-controls)",
            "(overflow-inline)",
            "(overflow-block)",
            "(forced-colors)",
            "(inverted-colors)",
            "(prefers-reduced-data)",
            "(prefers-contrast)",
            "(prefers-reduced-motion)",
            "(prefers-reduced-transparency)",
            "(hover)",
            "(any-hover)",
            "(pointer)",
            "(any-pointer)",
        ] {
            let q = parse_media_query(query);
            // Известна (не "not all") и сериализуется обратно as-is.
            assert_eq!(q.serialize(), query, "query: {query}");
        }
    }

    // ── Стиль: @media с новыми фичами применяется в каскаде ──

    #[test]
    fn media_rule_with_em_width_applies_in_layout() {
        // Парсинг: @media (min-width: 48em) - должен создать MediaRule с query.
        let s = parse("@media (min-width: 48em) { p { color: red; } }");
        assert_eq!(s.media_rules.len(), 1);
        let ctx = MediaContext {
            media_type: "screen".into(),
            width: 1024.0, // > 768px (48em)
            height: 720.0,
            prefers_dark: false,
            prefers_reduced_motion: false,
            forced_colors: false,
            ..Default::default()
        };
        assert!(s.media_rules[0].query.matches(&ctx));
        let ctx_narrow = MediaContext { width: 600.0, ..ctx.clone() };
        assert!(!s.media_rules[0].query.matches(&ctx_narrow));
    }
