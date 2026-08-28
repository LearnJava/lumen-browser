//! Тесты `style.rs`: шрифты: `font`-шортхенд, `font-*`-настройки, `line-height`.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ── IE7 line-height quirk (CSS Quirks Mode §3.2) ─────────────────────

    #[test]
    fn ie7_line_height_quirk_img_gets_1_in_quirks_mode() {
        // HTML без DOCTYPE → quirks mode; <img> должен получить line-height: 1.
        let s = cascade_at("<img>", "", &[0]);
        assert!(
            (s.line_height - 1.0).abs() < f32::EPSILON,
            "quirks <img> line_height={} (ожидалось 1.0)",
            s.line_height
        );
    }

    #[test]
    fn ie7_line_height_quirk_img_not_applied_in_standards_mode() {
        // С <!DOCTYPE html> → standards mode; line-height должен остаться normal (1.2).
        let s = cascade_at("<!DOCTYPE html><img>", "", &[0]);
        assert!(
            (s.line_height - 1.2).abs() < f32::EPSILON,
            "standards <img> line_height={} (ожидалось 1.2)",
            s.line_height
        );
    }

    #[test]
    fn ie7_line_height_quirk_author_css_overrides() {
        // Author CSS побеждает UA-правило quirk.
        let s = cascade_at("<img>", "img { line-height: 2; }", &[0]);
        assert!(
            (s.line_height - 2.0).abs() < f32::EPSILON,
            "quirks <img> с author CSS line_height={} (ожидалось 2.0)",
            s.line_height
        );
    }

    #[test]
    fn ie7_line_height_quirk_other_replaced_elements() {
        // Quirk применяется ко всем replaced-элементам.
        for tag in &["video", "canvas", "embed", "iframe", "input", "textarea", "select"] {
            let html = format!("<{tag}>");
            let s = cascade_at(&html, "", &[0]);
            assert!(
                (s.line_height - 1.0).abs() < f32::EPSILON,
                "quirks <{tag}> line_height={} (ожидалось 1.0)",
                s.line_height
            );
        }
    }

    #[test]
    fn ie7_line_height_quirk_not_applied_to_block_div() {
        // <div> — не replaced element; quirk не применяется.
        let s = cascade_at("<div></div>", "", &[0]);
        assert!(
            (s.line_height - 1.2).abs() < f32::EPSILON,
            "quirks <div> line_height={} (ожидалось 1.2)",
            s.line_height
        );
    }

    #[test]
    fn length_resolve_em_uses_basis() {
        // 1.5em при basis 20 = 30.
        assert_eq!(Length::Em(1.5).resolve(20.0, None, vp()), Some(30.0));
    }

    #[test]
    fn length_resolve_rem_ignores_basis() {
        // rem всегда от ROOT_FONT_SIZE = 16.
        assert_eq!(Length::Rem(2.0).resolve(999.0, None, vp()), Some(32.0));
    }

    #[test]
    fn length_resolve_percent_needs_basis() {
        assert_eq!(Length::Percent(50.0).resolve(16.0, Some(200.0), vp()), Some(100.0));
        assert_eq!(Length::Percent(50.0).resolve(16.0, None, vp()), None);
    }

    // ── BUG-114: `font` shorthand expands font-size/line-height ───────────

    /// Computes the style of `<p>` under the given author CSS (applied to `p`).
    fn p_style_with_css(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse("<p>x</p>");
        let sheet = lumen_css_parser::parse(css);
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let p = doc.get(body).children[0];
        compute_style(&doc, p, &sheet, &root_style, Size::new(800.0, 600.0), false)
    }

    #[test]
    fn font_shorthand_size_weight_line_height_family() {
        // BUG-114: `font: 700 13px/1.4 sans-serif` previously dropped size and
        // line-height, only applying weight.
        let s = p_style_with_css("p { font: 700 13px/1.4 sans-serif; }");
        assert_eq!(s.font_size, 13.0);
        assert_eq!(s.font_weight, FontWeight(700));
        assert!(s.line_height_is_relative);
        assert!((s.line_height - 1.4).abs() < 1e-4);
        assert_eq!(s.font_family.first().map(String::as_str), Some("sans-serif"));
    }

    #[test]
    fn font_shorthand_size_line_height_only() {
        // BUG-114: `font: 11px/1.5 monospace` — no leading section.
        let s = p_style_with_css("p { font: 11px/1.5 monospace; }");
        assert_eq!(s.font_size, 11.0);
        assert!((s.line_height - 1.5).abs() < 1e-4);
        assert_eq!(s.font_family.first().map(String::as_str), Some("monospace"));
        // Unspecified components reset to initial.
        assert_eq!(s.font_weight, FontWeight::NORMAL);
        assert_eq!(s.font_style, FontStyle::Normal);
    }

    #[test]
    fn text_font_features_emits_titl_only_for_titling_caps() {
        // Синтезируемые значения фич не дают: `c2sc` по уже поднятому в
        // верхний регистр тексту уменьшил бы капитель второй раз.
        let mut s = ComputedStyle::root();
        for caps in [
            FontVariantCaps::SmallCaps,
            FontVariantCaps::AllSmallCaps,
            FontVariantCaps::PetiteCaps,
            FontVariantCaps::AllPetiteCaps,
            FontVariantCaps::Unicase,
            FontVariantCaps::Normal,
        ] {
            s.font_variant_caps = caps;
            assert!(text_font_features(&s).is_empty(), "{}", caps.as_str());
        }
        s.font_variant_caps = FontVariantCaps::TitlingCaps;
        assert_eq!(text_font_features(&s), vec![(*b"titl", 1)]);
    }

    #[test]
    fn text_font_features_puts_feature_settings_last() {
        // CSS Fonts L4 §6.4: font-feature-settings имеет высший приоритет,
        // а шейпер применяет пары слева направо — значит автор может
        // выключить фичу капители, и её запись обязана идти раньше.
        let mut s = ComputedStyle::root();
        s.font_variant_caps = FontVariantCaps::TitlingCaps;
        s.font_feature_settings = vec![FontFeatureSetting { tag: *b"titl", value: 0 }];
        assert_eq!(text_font_features(&s), vec![(*b"titl", 1), (*b"titl", 0)]);
    }

    #[test]
    fn font_shorthand_style_variant_weight() {
        let s = p_style_with_css("p { font: italic small-caps bold 20px Georgia; }");
        assert_eq!(s.font_size, 20.0);
        assert_eq!(s.font_style, FontStyle::Italic);
        assert_eq!(s.font_variant_caps, FontVariantCaps::SmallCaps);
        assert_eq!(s.font_weight, FontWeight::BOLD);
        assert_eq!(s.font_family.first().map(String::as_str), Some("Georgia"));
    }

    #[test]
    fn font_shorthand_no_line_height_resets_to_initial() {
        // No `/line-height` → initial normal (≈1.2 relative).
        let s = p_style_with_css("p { font: 18px serif; }");
        assert_eq!(s.font_size, 18.0);
        assert!(s.line_height_is_relative);
        assert!((s.line_height - 1.2).abs() < 1e-4);
    }

    #[test]
    fn font_shorthand_resets_prior_longhands() {
        // Shorthand must reset longhands it controls (CSS Cascade L4 §3.1):
        // the earlier `font-weight: bold` is wiped by the later `font`.
        let s = p_style_with_css("p { font-weight: bold; font: 16px serif; }");
        assert_eq!(s.font_weight, FontWeight::NORMAL);
    }

    #[test]
    fn font_shorthand_multiword_family() {
        let s = p_style_with_css("p { font: 14px \"Helvetica Neue\", sans-serif; }");
        assert_eq!(s.font_size, 14.0);
        assert_eq!(s.font_family.first().map(String::as_str), Some("Helvetica Neue"));
    }

    #[test]
    fn font_shorthand_invalid_no_family_ignored() {
        // Missing family → invalid → declaration ignored, defaults kept.
        let s = p_style_with_css("p { font: 16px; }");
        assert_eq!(s.font_size, 16.0); // matches default; no panic
    }

    /// Стиль `<p>` внутри `<body>`, посчитанный по настоящей цепочке
    /// наследования (root → body → p) — нужен там, где проверяется, что
    /// значение приходит от предка, а не объявлено на самом элементе.
    fn p_style_inheriting_from_body(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse("<body><p>x</p></body>");
        let sheet = lumen_css_parser::parse(css);
        let root_style = ComputedStyle::root();
        let viewport = Size::new(800.0, 600.0);
        let body = doc.body().unwrap();
        let body_style = compute_style(&doc, body, &sheet, &root_style, viewport, false);
        let p = doc.get(body).children[0];
        compute_style(&doc, p, &sheet, &body_style, viewport, false)
    }

    #[test]
    fn font_size_var_declared_on_same_element() {
        // BUG-731: custom-properties pass раньше шёл ПОСЛЕ font-size-pre-pass,
        // поэтому объявленная на том же элементе переменная была ему не видна.
        let s = p_style_with_css("p { --fs: 21px; font-size: var(--fs); }");
        assert_eq!(s.font_size, 21.0);
    }

    #[test]
    fn font_size_var_inherited_from_ancestor() {
        // BUG-731: типовой случай дизайн-систем — переменные объявлены на
        // предке (`:root`/`body`), размеры берутся из них.
        let s = p_style_inheriting_from_body("body { --fs: 27px; } p { font-size: var(--fs); }");
        assert_eq!(s.font_size, 27.0);
    }

    #[test]
    fn font_size_var_fallback_used_when_name_undefined() {
        let s = p_style_with_css("p { font-size: var(--nope, 19px); }");
        assert_eq!(s.font_size, 19.0);
    }

    #[test]
    fn font_size_var_unresolvable_leaves_inherited_size() {
        // CSS Variables L1 §3.3: invalid at computed value time → декларация
        // не применяется, размер остаётся унаследованным.
        let s = p_style_with_css("p { font-size: var(--nope); }");
        assert_eq!(s.font_size, ROOT_FONT_SIZE);
    }

    #[test]
    fn font_shorthand_from_var_applies_size_not_only_weight() {
        // BUG-731: `font: var(--f)` применял всё, кроме размера — longhand-ы
        // считает main-pass (он var() раскрывает), а `<font-size>` — pre-pass
        // (он не раскрывал). Проверяем, что теперь совпадает с литералом.
        let s = p_style_with_css("p { --f: 700 23px/1.4 serif; font: var(--f); }");
        assert_eq!(s.font_size, 23.0);
        assert_eq!(s.font_weight, FontWeight(700));
        assert!(s.line_height_is_relative);
        assert!((s.line_height - 1.4).abs() < 1e-4);
    }

    #[test]
    fn font_size_var_chain_through_another_var() {
        let s = p_style_with_css("p { --a: var(--b); --b: 29px; font-size: var(--a); }");
        assert_eq!(s.font_size, 29.0);
    }

    #[test]
    fn font_shorthand_size_from_calc() {
        // BUG-731: `is_font_size_token` не знал про `Length::Calc`, поэтому весь
        // shorthand признавался невалидным — при том что longhand
        // `font-size: calc(...)` с тем же значением работал.
        let s = p_style_with_css("p { font: 700 calc(4px + 40px) serif; }");
        assert_eq!(s.font_size, 44.0);
        assert_eq!(s.font_weight, FontWeight(700));
    }

    #[test]
    fn font_shorthand_calc_size_and_calc_line_height() {
        // Токенизация shorthand-а должна считать пробелы и `/` внутри `calc()`
        // частью токена, а не разделителями (BUG-731).
        let s = p_style_with_css("p { font: 700 calc(0px + 44px) / calc(44px * 1.5) serif; }");
        assert_eq!(s.font_size, 44.0);
        assert!((s.line_height - 1.5).abs() < 1e-4);
    }

    #[test]
    fn font_shorthand_overrides_earlier_font_size_inherit() {
        // BUG-731: `font-size: inherit` применялся дважды — в pre-pass (там его
        // корректно перебивал более поздний `font`) и ещё раз в main-pass через
        // ветку CSS-wide keyword-ов, которая про shorthand не знает и затирала
        // размер обратно в унаследованный.
        let s = p_style_with_css("p { font-size: inherit; font: 700 44px/1.2 serif; }");
        assert_eq!(s.font_size, 44.0);
    }

    #[test]
    fn font_size_inherit_after_shorthand_still_wins() {
        // Обратный порядок: более поздний `font-size: inherit` обязан победить
        // размер из более раннего shorthand-а — иначе фикс BUG-731 сломал бы
        // каскад в другую сторону.
        let s = p_style_with_css("p { font: 700 44px/1.2 serif; font-size: inherit; }");
        assert_eq!(s.font_size, ROOT_FONT_SIZE);
        assert_eq!(s.font_weight, FontWeight(700));
    }

    #[test]
    fn split_font_shorthand_tokens_keeps_calc_intact() {
        assert_eq!(
            split_font_shorthand_tokens("700 calc(0px + 44px) / calc((1px) * 2) A, B"),
            vec!["700", "calc(0px + 44px)", "/", "calc((1px) * 2)", "A,", "B"]
        );
        assert_eq!(
            split_font_shorthand_tokens("13px/1.4 serif"),
            vec!["13px", "/", "1.4", "serif"]
        );
    }

    #[test]
    fn bgcolor_hint_body_named() {
        let s = doc_root_child_style("<body bgcolor=\"red\"></body>");
        assert_eq!(s.background_color, Some(CssColor::Rgba(rgba(255, 0, 0, 255))));
    }

    #[test]
    fn bgcolor_hint_body_hash() {
        let s = doc_root_child_style("<body bgcolor=\"#00ff00\"></body>");
        assert_eq!(s.background_color, Some(CssColor::Rgba(rgba(0, 255, 0, 255))));
    }

    #[test]
    fn bgcolor_hint_body_hashless_legacy() {
        // Главное отличие HTML legacy от CSS quirk: hashless hex принимается
        // без зависимости от document mode.
        let s = doc_root_child_style("<body bgcolor=\"0000ff\"></body>");
        assert_eq!(s.background_color, Some(CssColor::Rgba(rgba(0, 0, 255, 255))));
    }

    #[test]
    fn bgcolor_hint_table_named() {
        let s = doc_root_child_style("<table bgcolor=\"yellow\"></table>");
        assert_eq!(s.background_color, Some(CssColor::Rgba(rgba(255, 255, 0, 255))));
    }

    #[test]
    fn bgcolor_hint_not_applied_to_div() {
        // <div bgcolor="red"> — bgcolor не присутствует в spec для div,
        // hint игнорируется.
        let s = doc_root_child_style("<div bgcolor=\"red\"></div>");
        assert_eq!(s.background_color, None);
    }

    #[test]
    fn bgcolor_hint_transparent_does_not_apply() {
        // «transparent» — error в legacy-парсере, hint не применяется.
        let s = doc_root_child_style("<body bgcolor=\"transparent\"></body>");
        assert_eq!(s.background_color, None);
    }

    #[test]
    fn bgcolor_hint_overridden_by_author_css() {
        // Presentational hint имеет lowest specificity — любой author CSS
        // перекрывает (HTML5 §10 «Mapped attributes»).
        let doc = lumen_html_parser::parse("<body bgcolor=\"red\"></body>");
        let sheet = lumen_css_parser::parse("body { background-color: blue; }");
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let s = compute_style(&doc, body, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.background_color, Some(CssColor::Rgba(rgba(0, 0, 255, 255))));
    }

    #[test]
    fn bgcolor_hint_td_inside_table() {
        // td тоже принимает bgcolor.
        let doc = lumen_html_parser::parse("<table><tr><td bgcolor=\"#abcdef\">x</td></tr></table>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        // HTML5 parser inserts implicit <tbody>: body → table → tbody → tr → td.
        let table = doc.get(doc.body().unwrap()).children[0];
        let tbody = doc.get(table).children[0]; // implicit tbody
        let tr = doc.get(tbody).children[0];
        let td = doc.get(tr).children[0];
        let s = compute_style(&doc, td, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.background_color, Some(CssColor::Rgba(rgba(0xab, 0xcd, 0xef, 255))));
    }

    // --- font-variation-settings ---

    #[test]
    fn font_variation_settings_normal_is_empty() {
        // `normal` → пустой Vec (default-instance, без deltas)
        let result = parse_font_variation_settings("normal");
        assert_eq!(result, Some(vec![]));
    }

    #[test]
    fn font_variation_settings_single_axis() {
        let result = parse_font_variation_settings("\"wght\" 600");
        assert_eq!(result, Some(vec![
            FontVariationSetting { tag: *b"wght", value: 600.0 }
        ]));
    }

    #[test]
    fn font_variation_settings_multiple_axes() {
        let result = parse_font_variation_settings("\"wght\" 700, \"wdth\" 80");
        assert_eq!(result, Some(vec![
            FontVariationSetting { tag: *b"wght", value: 700.0 },
            FontVariationSetting { tag: *b"wdth", value: 80.0 },
        ]));
    }

    #[test]
    fn font_variation_settings_invalid_tag_ignored() {
        // Невалидный (не 4 символа) → None, объявление игнорируется
        assert_eq!(parse_font_variation_settings("\"wg\" 600"), None);
        assert_eq!(parse_font_variation_settings("\"wghtt\" 600"), None);
    }

    #[test]
    fn font_variation_settings_initial_is_empty() {
        // Без объявления = initial = пустой Vec
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(s.font_variation_settings.is_empty());
    }

    #[test]
    fn font_variation_settings_applied() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { font-variation-settings: \"wght\" 900; }");
        let root = ComputedStyle::root();
        let node = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, node, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.font_variation_settings, vec![
            FontVariationSetting { tag: *b"wght", value: 900.0 }
        ]);
    }

    #[test]
    fn font_variation_settings_inherited() {
        // Свойство наследуется от родителя к потомку
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { font-variation-settings: \"wght\" 800; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(span_style.font_variation_settings, vec![
            FontVariationSetting { tag: *b"wght", value: 800.0 }
        ]);
    }

    #[test]
    fn font_variation_settings_child_overrides_parent() {
        // Потомок может переопределить наследуемое значение
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse(
            "div { font-variation-settings: \"wght\" 800; } \
             span { font-variation-settings: \"wght\" 400; }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(span_style.font_variation_settings, vec![
            FontVariationSetting { tag: *b"wght", value: 400.0 }
        ]);
    }

    // ── font-feature-settings (CSS Fonts L3 §6) ──────────────────────────

    #[test]
    fn font_feature_settings_normal_is_empty() {
        // `normal` → пустой Vec (default-набор фич шейпера)
        assert_eq!(parse_font_feature_settings("normal"), Some(vec![]));
    }

    #[test]
    fn font_feature_settings_value_forms() {
        // Опущенное значение → 1; on → 1; off → 0; целое как есть
        let result = parse_font_feature_settings(
            "\"smcp\", \"liga\" off, \"kern\" on, \"salt\" 2",
        );
        assert_eq!(result, Some(vec![
            FontFeatureSetting { tag: *b"smcp", value: 1 },
            FontFeatureSetting { tag: *b"liga", value: 0 },
            FontFeatureSetting { tag: *b"kern", value: 1 },
            FontFeatureSetting { tag: *b"salt", value: 2 },
        ]));
    }

    #[test]
    fn font_feature_settings_invalid_declarations() {
        // Тег не из 4 символов, отрицательное/нечисловое значение,
        // неквотированный тег → None (объявление игнорируется)
        assert_eq!(parse_font_feature_settings("\"lig\" 1"), None);
        assert_eq!(parse_font_feature_settings("\"ligaa\" 1"), None);
        assert_eq!(parse_font_feature_settings("\"liga\" -1"), None);
        assert_eq!(parse_font_feature_settings("\"liga\" x"), None);
        assert_eq!(parse_font_feature_settings("liga 1"), None);
    }

    #[test]
    fn font_feature_settings_initial_is_empty() {
        // Без объявления = initial = пустой Vec
        let s = style_for("");
        assert!(s.font_feature_settings.is_empty());
    }

    #[test]
    fn font_feature_settings_applied() {
        let s = style_for("font-feature-settings: \"liga\" 0");
        assert_eq!(s.font_feature_settings, vec![
            FontFeatureSetting { tag: *b"liga", value: 0 }
        ]);
    }

    #[test]
    fn font_feature_settings_inherited() {
        // Свойство наследуется от родителя к потомку
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { font-feature-settings: \"smcp\" 1; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style =
            compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(span_style.font_feature_settings, vec![
            FontFeatureSetting { tag: *b"smcp", value: 1 }
        ]);
    }

    #[test]
    fn font_feature_settings_child_overrides_parent() {
        // Потомок сбрасывает наследуемое значение через `normal`
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse(
            "div { font-feature-settings: \"liga\" 0; } \
             span { font-feature-settings: normal; }",
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style =
            compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert!(span_style.font_feature_settings.is_empty());
    }

    // ── font-palette (CSS Fonts L4 §11.3) ────────────────────────────────

    #[test]
    fn font_palette_parse_forms() {
        assert_eq!(parse_font_palette("normal"), Some(FontPalette::Normal));
        assert_eq!(parse_font_palette("LIGHT"), Some(FontPalette::Light));
        assert_eq!(parse_font_palette(" dark "), Some(FontPalette::Dark));
        assert_eq!(
            parse_font_palette("--Cool"),
            Some(FontPalette::Custom("--Cool".to_string()))
        );
        assert_eq!(parse_font_palette("banana"), None);
        assert_eq!(parse_font_palette("123"), None);
        assert_eq!(parse_font_palette("--"), None);
        assert_eq!(parse_font_palette("--a b"), None);
    }

    #[test]
    fn font_palette_initial_is_normal() {
        let s = style_for("");
        assert_eq!(s.font_palette, FontPalette::Normal);
        assert!(s.font_palette_resolved.is_none());
    }

    #[test]
    fn font_palette_applied() {
        let s = style_for("font-palette: dark");
        assert_eq!(s.font_palette, FontPalette::Dark);
    }

    #[test]
    fn font_palette_invalid_value_ignored() {
        let s = style_for("font-palette: 2");
        assert_eq!(s.font_palette, FontPalette::Normal);
    }

    #[test]
    fn font_palette_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { font-palette: light; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style =
            compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(span_style.font_palette, FontPalette::Light);
    }

    #[test]
    fn font_palette_custom_resolves_against_at_rule() {
        // `font-palette: --brand` + matching `@font-palette-values` →
        // `font_palette_resolved` заполнен в compute_style.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@font-palette-values --brand { base-palette: 1; override-colors: 0 #ff0000; } \
             div { font-palette: --brand; }",
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.font_palette, FontPalette::Custom("--brand".to_string()));
        let resolved = s.font_palette_resolved.as_ref().expect("palette must resolve");
        assert_eq!(resolved.base_palette, Some(1));
        assert_eq!(resolved.overrides.len(), 1);
        assert_eq!(resolved.overrides[0].index, 0);
        assert_eq!(resolved.overrides[0].color.r, 255);

        // palette_selection маппит в renderer-facing Custom.
        match crate::font_palette::palette_selection(&s) {
            Some(crate::font_palette::FontPaletteSelection::Custom { base_palette, overrides }) => {
                assert_eq!(base_palette, 1);
                assert_eq!(overrides.len(), 1);
            }
            other => panic!("expected Custom selection, got {other:?}"),
        }
    }

    #[test]
    fn font_palette_unknown_ident_behaves_as_normal() {
        // Нет подходящего @font-palette-values → resolved None → selection None.
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { font-palette: --missing; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.font_palette, FontPalette::Custom("--missing".to_string()));
        assert!(s.font_palette_resolved.is_none());
        assert!(crate::font_palette::palette_selection(&s).is_none());
    }

    // ── font-optical-sizing (CSS Fonts L4 §7.12) ─────────────────────────

    #[test]
    fn font_optical_sizing_default_is_auto() {
        let s = style_for("");
        assert_eq!(s.font_optical_sizing, FontOpticalSizing::Auto);
    }

    #[test]
    fn font_optical_sizing_none_parsed() {
        let s = style_for("font-optical-sizing: none");
        assert_eq!(s.font_optical_sizing, FontOpticalSizing::None);
    }

    #[test]
    fn font_optical_sizing_auto_explicit() {
        let s = style_for("font-optical-sizing: auto");
        assert_eq!(s.font_optical_sizing, FontOpticalSizing::Auto);
    }

    #[test]
    fn font_optical_sizing_inherited() {
        // CSS Fonts L4 §7.12: font-optical-sizing is inherited.
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { font-optical-sizing: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.font_optical_sizing, FontOpticalSizing::None);
        assert_eq!(span_style.font_optical_sizing, FontOpticalSizing::None);
    }

    #[test]
    fn font_optical_sizing_child_overrides_parent() {
        // Child can reset to auto even when parent has none.
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse(
            "div { font-optical-sizing: none; } span { font-optical-sizing: auto; }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.font_optical_sizing, FontOpticalSizing::None);
        assert_eq!(span_style.font_optical_sizing, FontOpticalSizing::Auto);
    }

    // ── font-variant-emoji (CSS Fonts L4 §6.6) ───────────────────────────────

    #[test]
    fn font_variant_emoji_longhand_parses_all_keywords() {
        for (kw, expected) in [
            ("normal", FontVariantEmoji::Normal),
            ("text", FontVariantEmoji::Text),
            ("emoji", FontVariantEmoji::Emoji),
            ("unicode", FontVariantEmoji::Unicode),
        ] {
            let css = format!("div {{ font-variant-emoji: {kw}; }}");
            let s = cascade_at("<div>", &css, &[0]);
            assert_eq!(s.font_variant_emoji, expected, "keyword `{kw}`");
        }
    }

    #[test]
    fn font_variant_emoji_garbage_keeps_previous_value() {
        let s = cascade_at("<div>", "div { font-variant-emoji: sideways; }", &[0]);
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Normal);
    }

    #[test]
    fn font_variant_emoji_is_inherited() {
        let s = cascade_at(
            "<div><span>x</span></div>",
            "div { font-variant-emoji: emoji; }",
            &[0, 0],
        );
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Emoji);
    }

    #[test]
    fn font_variant_shorthand_carries_and_resets_emoji_component() {
        let s = cascade_at("<div>", "div { font-variant: unicode; }", &[0]);
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Unicode);
        // Both components at once.
        let s = cascade_at("<div>", "div { font-variant: small-caps emoji; }", &[0]);
        assert_eq!(s.font_variant_caps, FontVariantCaps::SmallCaps);
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Emoji);
        // A shorthand that names no emoji component resets it to initial —
        // the inherited `emoji` must not leak through (CSS Cascade L4 §3.1).
        let s = cascade_at(
            "<div><span>x</span></div>",
            "div { font-variant: emoji; } span { font-variant: small-caps; }",
            &[0, 0],
        );
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Normal);
    }

    #[test]
    fn font_shorthand_resets_font_variant_emoji() {
        let s = cascade_at(
            "<div><span>x</span></div>",
            "div { font-variant-emoji: emoji; } span { font: 12px serif; }",
            &[0, 0],
        );
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Normal);
    }

    #[test]
    fn font_variant_emoji_css_wide_keywords() {
        // `inherit` pulls the parent value back after a local override.
        let s = cascade_at(
            "<div><span>x</span></div>",
            "div { font-variant-emoji: emoji; } \
             span { font-variant-emoji: text; font-variant-emoji: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Emoji);
        // `initial` drops the inherited value.
        let s = cascade_at(
            "<div><span>x</span></div>",
            "div { font-variant-emoji: emoji; } span { font-variant-emoji: initial; }",
            &[0, 0],
        );
        assert_eq!(s.font_variant_emoji, FontVariantEmoji::Normal);
    }

    // --- font-size-adjust ---

    #[test]
    fn font_size_adjust_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { font-size-adjust: 0.5; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.font_size_adjust, FontSizeAdjust::Value(0.5));
    }

    #[test]
    fn font_size_adjust_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.font_size_adjust, FontSizeAdjust::None);
    }

    #[test]
    fn font_size_adjust_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { font-size-adjust: 0.47; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.font_size_adjust, FontSizeAdjust::Value(0.47));
        assert_eq!(span_style.font_size_adjust, FontSizeAdjust::Value(0.47));
    }

    #[test]
    fn font_size_adjust_auto() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { font-size-adjust: auto; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.font_size_adjust, FontSizeAdjust::Auto);
    }

    // ── line-height relative/absolute classification (BUG-212) ─────────────────

    #[test]
    fn line_height_px_is_absolute_number_is_relative() {
        let doc = lumen_html_parser::parse("<div id=a></div><div id=b></div>");
        let root = ComputedStyle::root();
        let vp = Size::new(800.0, 600.0);
        // `<length>` → absolute (frozen under font-size-adjust).
        let sheet_px = lumen_css_parser::parse("div { line-height: 100px; }");
        let a = doc.find_by_id("a").unwrap();
        let s_px = compute_style(&doc, a, &sheet_px, &root, vp, false);
        assert!(!s_px.line_height_is_relative, "px line-height must be absolute");
        // unitless `<number>` → relative (scales with font-size).
        let sheet_num = lumen_css_parser::parse("div { line-height: 1.5; }");
        let s_num = compute_style(&doc, a, &sheet_num, &root, vp, false);
        assert!(s_num.line_height_is_relative, "number line-height must be relative");
        // `normal` (default) → relative.
        let sheet_none = lumen_css_parser::parse("");
        let s_def = compute_style(&doc, a, &sheet_none, &root, vp, false);
        assert!(s_def.line_height_is_relative, "default/normal line-height must be relative");
    }
