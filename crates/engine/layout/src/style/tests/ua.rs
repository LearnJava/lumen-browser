//! Тесты `style.rs`: UA-таблица и презентационные HTML-атрибуты, включая quirks.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ── HTML5 §2.4.6 «rules for parsing a legacy color value» ─────────────

    #[test]
    fn legacy_color_empty_is_error() {
        assert_eq!(parse_legacy_color_html_attr(""), None);
    }

    #[test]
    fn legacy_color_whitespace_only_is_error() {
        // Spec step 3 trim → empty → fail.
        assert_eq!(parse_legacy_color_html_attr("   "), None);
        assert_eq!(parse_legacy_color_html_attr("\t\n\r"), None);
    }

    #[test]
    fn legacy_color_transparent_keyword_is_error() {
        // Spec step 4: «transparent» — единственный keyword, дающий error.
        assert_eq!(parse_legacy_color_html_attr("transparent"), None);
        assert_eq!(parse_legacy_color_html_attr("TRANSPARENT"), None);
        assert_eq!(parse_legacy_color_html_attr("  Transparent  "), None);
    }

    #[test]
    fn legacy_color_named_lookup() {
        assert_eq!(parse_legacy_color_html_attr("red"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("RED"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("Blue"), Some(rgba(0, 0, 255, 255)));
        assert_eq!(parse_legacy_color_html_attr("rebeccapurple"), Some(rgba(102, 51, 153, 255)));
    }

    #[test]
    fn legacy_color_hash_short_hex() {
        // Spec step 6: 4-char #rgb с hex-digits expand до #rrggbb.
        assert_eq!(parse_legacy_color_html_attr("#f00"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#0f0"), Some(rgba(0, 255, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#abc"), Some(rgba(170, 187, 204, 255)));
    }

    #[test]
    fn legacy_color_hash_long_hex() {
        // Spec steps 8+: # удаляется, остальное идёт в общий padding-procedure.
        assert_eq!(parse_legacy_color_html_attr("#ff0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#FF0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("#abcdef"), Some(rgba(0xab, 0xcd, 0xef, 255)));
    }

    #[test]
    fn legacy_color_hashless_hex_6_digits() {
        // HTML legacy (в отличие от CSS quirk!) принимает hashless hex.
        // 6 digits → split на 3 по 2 → strip leading zeros (none) → r,g,b.
        assert_eq!(parse_legacy_color_html_attr("ff0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("00ff00"), Some(rgba(0, 255, 0, 255)));
    }

    #[test]
    fn legacy_color_hashless_3_digits_no_expand() {
        // ВАЖНО: для hashless `f00` short-hex expand (step 6) НЕ работает —
        // он только для 4-char `#xyz`. «f00» проходит через общий path:
        // split на 3 по 1 → length=1, не пакуется до 2 → r=0xf, g=0, b=0.
        assert_eq!(parse_legacy_color_html_attr("f00"), Some(rgba(15, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_garbage_replaced_with_zeros() {
        // Spec step 9: не-hex chars заменяются на «0». «garbage» → «0a0ba0e».
        // Длина 7 не multiple of 3 → padding до 9 → «0a0ba0e00».
        // Split: «0a0», «ba0», «e00». length=3. Все ведущие? red[0]='0',
        // green[0]='b' — нет, не strip. Truncate до 2 каждый: «0a», «ba», «e0».
        assert_eq!(parse_legacy_color_html_attr("garbage"), Some(rgba(0x0a, 0xba, 0xe0, 255)));
    }

    #[test]
    fn legacy_color_pads_short_to_multiple_of_three() {
        // «1» → padding → «100» → split «1»,«0»,«0» → r=1, g=0, b=0.
        // length=1, не > 2, не truncate. Парсим: «1»→1, «0»→0, «0»→0.
        assert_eq!(parse_legacy_color_html_attr("1"), Some(rgba(1, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_strips_common_leading_zeros() {
        // «000a000b000c» → length=4, > 2. Все ведущие «0»? red[0]='0',
        // green[0]='0', blue[0]='0' → strip. length=3, всё ещё все ведущие
        // '0' → strip. length=2, останавливаемся. red=«0a», green=«0b», blue=«0c».
        assert_eq!(parse_legacy_color_html_attr("000a000b000c"), Some(rgba(0x0a, 0x0b, 0x0c, 255)));
    }

    #[test]
    fn legacy_color_truncates_after_strip() {
        // «aabbccdd0aabbccdd0aabbccdd0» — 27 chars, length=9. Step 12:
        // length > 8 → срезаем leading 1 (=length-8) из каждого, length=8.
        // Затем step 13 / 14. Проверяем что точно не паникует и валидный
        // цвет, без захода в детали значения.
        let result = parse_legacy_color_html_attr("aabbccdd0aabbccdd0aabbccdd0");
        assert!(result.is_some());
    }

    #[test]
    fn legacy_color_strips_hash_from_long_string() {
        // С `#` префиксом, но не вписывается в step 6 (длина ≠ 4): идёт через
        // step 8 (strip `#`) + общий процесс. «#xyz» с не-hex → `0`-replace.
        // Здесь «#ff» → strip `#` → «ff» → pad до 3 → «ff0» → split «f»,«f»,«0»
        // → length=1 → r=15, g=15, b=0.
        assert_eq!(parse_legacy_color_html_attr("#ff"), Some(rgba(15, 15, 0, 255)));
    }

    #[test]
    fn legacy_color_4char_hash_with_non_hex_takes_general_path() {
        // «#xyz» — длина 4, начинается с `#`, но `x` не hex → step 6 не
        // срабатывает. Идёт общий путь: strip `#` → «xyz» → replace non-hex
        // → «000» → split «0»,«0»,«0» → r=g=b=0.
        assert_eq!(parse_legacy_color_html_attr("#xyz"), Some(rgba(0, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_non_bmp_replaced_with_two_zeros() {
        // U+1F3A8 (🎨) > U+FFFF → заменяется на «00». «🎨» → «00» → pad до 3
        // → «000» → r=g=b=0.
        assert_eq!(parse_legacy_color_html_attr("🎨"), Some(rgba(0, 0, 0, 255)));
    }

    #[test]
    fn legacy_color_trim_outer_whitespace() {
        // Spec step 3 strip leading/trailing whitespace — но не внутренний
        // (мусор внутри идёт через replace-non-hex).
        assert_eq!(parse_legacy_color_html_attr("  red  "), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color_html_attr("\t#ff0000\n"), Some(rgba(255, 0, 0, 255)));
    }

    // ── apply_bgcolor_presentational_hint integration ────────────────────

    // ── BUG-128: UA `font-family: monospace` для <pre>/<code>/… ───────────

    /// Computes the style of the first child of `<body>` in `html`.
    fn first_child_style(html: &str, css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let node = doc.get(body).children[0];
        compute_style(&doc, node, &sheet, &root_style, Size::new(800.0, 600.0), false)
    }

    #[test]
    fn ua_monospace_for_pre_and_code() {
        for tag in ["pre", "code", "kbd", "samp", "tt"] {
            let s = first_child_style(&format!("<{tag}>x</{tag}>"), "");
            assert_eq!(
                s.font_family,
                vec!["monospace".to_string()],
                "<{tag}> должен получать UA font-family: monospace"
            );
        }
    }

    #[test]
    fn ua_monospace_does_not_leak_to_other_elements() {
        let s = first_child_style("<p>x</p>", "");
        assert_eq!(
            s.font_family,
            default_font_family(),
            "<p> не должен получать UA font-family: monospace — только дефолт документа"
        );
    }

    /// BUG-128: дефолт документа — `serif`, а НЕ пустой список. Пустой список
    /// в рендере зарезервирован за chrome UI (bundled Golos Text, DS-4), так
    /// что страница без объявленного `font-family` рисовалась бы шрифтом
    /// интерфейса вместо системного serif-а.
    #[test]
    fn document_default_font_family_is_serif() {
        assert_eq!(ComputedStyle::root().font_family, vec!["serif".to_string()]);
        let s = first_child_style("<p>x</p>", "");
        assert_eq!(s.font_family, vec!["serif".to_string()]);
    }

    /// `font-family: initial` откатывает к дефолту документа, а не к пустому
    /// списку (тот же инвариант, вход через ключевое слово CSS-wide).
    #[test]
    fn font_family_initial_keyword_restores_document_default() {
        let s = first_child_style("<p>x</p>", "p { font-family: Arial; font-family: initial; }");
        assert_eq!(s.font_family, default_font_family());
    }

    #[test]
    fn author_font_family_overrides_ua_monospace() {
        let s = first_child_style("<code>x</code>", "code { font-family: Arial; }");
        assert_eq!(s.font_family, vec!["Arial".to_string()]);
    }

    // ── BUG-603: apply_background_image_presentational_hint ──────────────

    #[test]
    fn background_hint_table_sets_url_layer() {
        let s = doc_root_child_style("<table background=\"/images/threecolors.png\"></table>");
        assert_eq!(s.background_layers.len(), 1);
        assert_eq!(s.background_layers[0].image, BackgroundImage::Url("/images/threecolors.png".into()));
    }

    #[test]
    fn background_hint_not_applied_to_div() {
        let s = doc_root_child_style("<div background=\"/x.png\"></div>");
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_hint_empty_value_ignored() {
        let s = doc_root_child_style("<table background=\"\"></table>");
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_hint_overridden_by_author_css() {
        let doc = lumen_html_parser::parse("<table background=\"/x.png\"></table>");
        let sheet = lumen_css_parser::parse("table { background-image: url(/y.png); }");
        let root_style = ComputedStyle::root();
        let table = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, table, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.background_layers[0].image, BackgroundImage::Url("/y.png".into()));
    }

    // ── BUG-603: apply_bordercolor_presentational_hint ────────────────────

    #[test]
    fn bordercolor_hint_table_sets_all_four_sides() {
        let s = doc_root_child_style("<table bordercolor=\"red\"></table>");
        let red = CssColor::Rgba(rgba(255, 0, 0, 255));
        assert_eq!(s.border_top_color, red);
        assert_eq!(s.border_right_color, red);
        assert_eq!(s.border_bottom_color, red);
        assert_eq!(s.border_left_color, red);
    }

    #[test]
    fn bordercolor_hint_not_applied_to_div() {
        let s = doc_root_child_style("<div bordercolor=\"red\"></div>");
        assert_eq!(s.border_top_color, CssColor::CurrentColor);
    }

    // ── BUG-603: apply_cellspacing_presentational_hint ────────────────────

    #[test]
    fn cellspacing_hint_sets_both_components() {
        let s = doc_root_child_style("<table cellspacing=\"10\"></table>");
        assert_eq!(s.border_spacing_h, 10.0);
        assert_eq!(s.border_spacing_v, 10.0);
    }

    #[test]
    fn cellspacing_hint_negative_ignored() {
        let s = doc_root_child_style("<table cellspacing=\"-5\"></table>");
        assert_eq!(s.border_spacing_h, 0.0);
        assert_eq!(s.border_spacing_v, 0.0);
    }

    #[test]
    fn cellspacing_hint_not_applied_to_td() {
        // cellspacing — атрибут только <table>, не распространяется на ячейки.
        let doc = lumen_html_parser::parse("<table><tr><td cellspacing=\"10\">x</td></tr></table>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let table = doc.get(doc.body().unwrap()).children[0];
        let tbody = doc.get(table).children[0];
        let tr = doc.get(tbody).children[0];
        let td = doc.get(tr).children[0];
        let s = compute_style(&doc, td, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_spacing_h, 0.0);
    }

    // ── apply_text_color_presentational_hint integration ─────────────────

    #[test]
    fn text_hint_body_named() {
        let s = doc_root_child_style("<body text=\"red\"></body>");
        assert_eq!(s.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn text_hint_body_hash() {
        let s = doc_root_child_style("<body text=\"#00ff00\"></body>");
        assert_eq!(s.color, rgba(0, 255, 0, 255));
    }

    #[test]
    fn text_hint_body_hashless_legacy() {
        // Hashless hex принимается legacy-парсером без зависимости от
        // document mode — как и в bgcolor.
        let s = doc_root_child_style("<body text=\"0000ff\"></body>");
        assert_eq!(s.color, rgba(0, 0, 255, 255));
    }

    #[test]
    fn text_hint_transparent_does_not_apply() {
        // «transparent» — error в legacy-парсере, hint не применяется
        // → color остаётся default (BLACK через initial).
        let s = doc_root_child_style("<body text=\"transparent\"></body>");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn text_hint_not_applied_to_div() {
        // <div text="red"> — `text` атрибут не присутствует в spec для div,
        // hint игнорируется.
        let s = doc_root_child_style("<div text=\"red\"></div>");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn text_hint_overridden_by_author_css() {
        // Presentational hint имеет lowest specificity — author CSS перекрывает.
        let doc = lumen_html_parser::parse("<body text=\"red\"></body>");
        let sheet = lumen_css_parser::parse("body { color: blue; }");
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let s = compute_style(&doc, body, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.color, rgba(0, 0, 255, 255));
    }

    #[test]
    fn text_hint_body_inherits_to_children() {
        // CSS `color` — inherited; legacy `text` на `<body>` должно через
        // наследование красить потомков без явного color.
        let doc = lumen_html_parser::parse("<body text=\"red\"><div>x</div></body>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let div = doc.get(body).children[0]; // first child of body = <div>
        let body_style = compute_style(&doc, body, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let div_style = compute_style(&doc, div, &sheet, &body_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn font_color_hint_named() {
        // <font color="red"> сам по себе. doc_root_child_style вернёт стиль
        // <font>-элемента; tree builder может обернуть его в <body> —
        // используем явный обход.
        let doc = lumen_html_parser::parse("<font color=\"red\">x</font>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let s = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn font_color_hint_hash() {
        let doc = lumen_html_parser::parse("<font color=\"#abcdef\">x</font>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let s = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.color, rgba(0xab, 0xcd, 0xef, 255));
    }

    #[test]
    fn font_color_hint_overridden_by_author_css() {
        let doc = lumen_html_parser::parse("<font color=\"red\">x</font>");
        let sheet = lumen_css_parser::parse("font { color: blue; }");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let s = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.color, rgba(0, 0, 255, 255));
    }

    #[test]
    fn font_color_hint_inherits_to_children() {
        let doc =
            lumen_html_parser::parse("<font color=\"red\"><span>x</span></font>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let font = find_first_element(&doc, doc.root(), "font").expect("font found");
        let span = find_first_element(&doc, font, "span").expect("span found");
        let font_style = compute_style(&doc, font, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let span_style =
            compute_style(&doc, span, &sheet, &font_style, Size::new(800.0, 600.0), false);
        assert_eq!(span_style.color, rgba(255, 0, 0, 255));
    }

    #[test]
    fn color_attr_on_div_does_not_apply() {
        // `color` атрибут — presentational hint только для `<font>`. На
        // `<div color="red">` игнорируется.
        let doc = lumen_html_parser::parse("<div color=\"red\">x</div>");
        let sheet = lumen_css_parser::parse("");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.color, Color::BLACK);
    }

    fn find_first_element(
        doc: &lumen_dom::Document,
        from: lumen_dom::NodeId,
        local: &str,
    ) -> Option<lumen_dom::NodeId> {
        let node = doc.get(from);
        if let lumen_dom::NodeData::Element { name, .. } = &node.data
            && name.local == local
        {
            return Some(from);
        }
        for child in &node.children {
            if let Some(found) = find_first_element(doc, *child, local) {
                return Some(found);
            }
        }
        None
    }

    // ── table cell width quirk (CSS Quirks Mode §4.1) ─────────────────────

    #[test]
    fn td_width_attr_quirks_mode_sets_min_width() {
        // Без DOCTYPE → quirks mode; CSS Quirks §4.1: width attr → min-width.
        // HTML5 parser inserts implicit tbody: body→table→tbody→tr→td = path [0,0,0,0].
        let s = cascade_at("<table><tr><td width=\"200\">", "", &[0, 0, 0, 0]);
        assert_eq!(s.width, None);
        assert_eq!(s.min_width, Some(Length::Px(200.0)));
    }

    #[test]
    fn td_width_attr_standards_mode_sets_width() {
        // <!DOCTYPE html> → standards mode; width attr → CSS width.
        // cascade_at starts from body, so table is at index 0 (not 1).
        // HTML5 parser inserts implicit tbody: body→table→tbody→tr→td = path [0,0,0,0].
        let s = cascade_at("<!DOCTYPE html><table><tr><td width=\"200\">", "", &[0, 0, 0, 0]);
        assert_eq!(s.width, Some(Length::Px(200.0)));
        assert_eq!(s.min_width, None);
    }

    #[test]
    fn th_width_attr_quirks_mode_sets_min_width() {
        // <th> аналогично <td> — тот же quirk.
        // HTML5 parser inserts implicit tbody: body→table→tbody→tr→th = path [0,0,0,0].
        let s = cascade_at("<table><tr><th width=\"120\">", "", &[0, 0, 0, 0]);
        assert_eq!(s.width, None);
        assert_eq!(s.min_width, Some(Length::Px(120.0)));
    }

    #[test]
    fn td_width_attr_percent_quirks_mode_sets_min_width_percent() {
        // Процентное значение тоже обрабатывается.
        // HTML5 parser inserts implicit tbody: body→table→tbody→tr→td = path [0,0,0,0].
        let s = cascade_at("<table><tr><td width=\"50%\">", "", &[0, 0, 0, 0]);
        assert_eq!(s.width, None);
        assert_eq!(s.min_width, Some(Length::Percent(50.0)));
    }

    #[test]
    fn table_width_attr_sets_width_in_quirks_mode() {
        // <table width="..."> → CSS width (quirk только для td/th).
        // cascade_at starts from body; table is at body.children[0].
        let s = cascade_at("<table width=\"800\"><tr><td>", "", &[0]);
        assert_eq!(s.width, Some(Length::Px(800.0)));
    }

    #[test]
    fn table_width_attr_sets_width_in_standards_mode() {
        // DOCTYPE → standards mode; cascade_at starts from body, table is at index 0.
        let s = cascade_at("<!DOCTYPE html><table width=\"800\"><tr><td>", "", &[0]);
        assert_eq!(s.width, Some(Length::Px(800.0)));
    }

    #[test]
    fn td_height_attr_sets_height_quirks_mode() {
        // height attr → CSS height без quirks-варианта.
        // HTML5 parser inserts implicit tbody: body→table→tbody→tr→td = path [0,0,0,0].
        let s = cascade_at("<table><tr><td height=\"50\">", "", &[0, 0, 0, 0]);
        assert_eq!(s.height, Some(Length::Px(50.0)));
    }

    #[test]
    fn td_height_attr_sets_height_standards_mode() {
        // cascade_at starts from body; table at [0], implicit tbody at [0,0].
        let s = cascade_at("<!DOCTYPE html><table><tr><td height=\"50\">", "", &[0, 0, 0, 0]);
        assert_eq!(s.height, Some(Length::Px(50.0)));
    }

    #[test]
    fn td_default_padding_is_one_px() {
        // UA stylesheet (HTML Rendering §15.3.8): td/th get padding: 1px.
        // HTML5 parser inserts implicit tbody: body→table→tbody→tr→td = path [0,0,0,0].
        let s = cascade_at("<table><tr><td>", "", &[0, 0, 0, 0]);
        assert_eq!(s.padding_top, Length::Px(1.0));
        assert_eq!(s.padding_right, Length::Px(1.0));
        assert_eq!(s.padding_bottom, Length::Px(1.0));
        assert_eq!(s.padding_left, Length::Px(1.0));
    }

    #[test]
    fn th_default_padding_is_one_px() {
        let s = cascade_at("<table><tr><th>", "", &[0, 0, 0, 0]);
        assert_eq!(s.padding_left, Length::Px(1.0));
        assert_eq!(s.padding_bottom, Length::Px(1.0));
    }

    #[test]
    fn td_author_padding_overrides_ua_default() {
        // Author `padding` wins over the 1px UA default.
        let s = cascade_at("<table><tr><td>", "td { padding: 10px; }", &[0, 0, 0, 0]);
        assert_eq!(s.padding_top, Length::Px(10.0));
        assert_eq!(s.padding_left, Length::Px(10.0));
    }

    #[test]
    fn td_cellpadding_zero_attr_overrides_ua_default() {
        // <table cellpadding="0"> restores zero padding (legacy layout tables).
        let s = cascade_at("<table cellpadding=\"0\"><tr><td>", "", &[0, 0, 0, 0]);
        assert_eq!(s.padding_top, Length::Px(0.0));
        assert_eq!(s.padding_left, Length::Px(0.0));
    }

    #[test]
    fn td_cellpadding_numeric_attr_sets_padding() {
        // <table cellpadding="5"> sets 5px padding on every cell.
        let s = cascade_at("<table cellpadding=\"5\"><tr><td>", "", &[0, 0, 0, 0]);
        assert_eq!(s.padding_top, Length::Px(5.0));
        assert_eq!(s.padding_right, Length::Px(5.0));
        assert_eq!(s.padding_bottom, Length::Px(5.0));
        assert_eq!(s.padding_left, Length::Px(5.0));
    }

    #[test]
    fn td_width_author_css_overrides_quirks_hint() {
        // Author CSS width перекрывает min-width presentational hint.
        // HTML5 parser inserts implicit tbody: body→table→tbody→tr→td = path [0,0,0,0].
        let s = cascade_at(
            "<table><tr><td width=\"200\">",
            "td { width: 300px; }",
            &[0, 0, 0, 0],
        );
        // Author CSS устанавливает width; hint установил min_width.
        assert_eq!(s.width, Some(Length::Px(300.0)));
        assert_eq!(s.min_width, Some(Length::Px(200.0)));
    }

    #[test]
    fn non_table_elements_not_affected_by_width_hint() {
        // div с width атрибутом — не presentational hint (div — не td/th/table).
        let s = cascade_at("<div width=\"200\">", "", &[0]);
        assert_eq!(s.width, None);
    }

    #[test]
    fn line_clamp_integer_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { -webkit-line-clamp: 3; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_clamp, Some(3));
    }

    #[test]
    fn line_clamp_standard_property() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { line-clamp: 5; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_clamp, Some(5));
    }

    #[test]
    fn line_clamp_initial_value_is_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_clamp, None);
    }

    #[test]
    fn line_clamp_none_keyword() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { -webkit-line-clamp: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_clamp, None);
    }

    #[test]
    fn line_clamp_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { -webkit-line-clamp: 2; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.line_clamp, Some(2));
        assert_eq!(span_style.line_clamp, None);
    }

    #[test]
    fn line_clamp_zero_is_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { -webkit-line-clamp: 0; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.line_clamp, None);
    }

    #[test]
    fn orphans_initial_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.orphans, 2);
    }

    #[test]
    fn widows_initial_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.widows, 2);
    }

    #[test]
    fn orphans_explicit_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { orphans: 4; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.orphans, 4);
    }

    #[test]
    fn widows_explicit_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { widows: 3; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.widows, 3);
    }

    #[test]
    fn orphans_inherited() {
        let doc = lumen_html_parser::parse("<div><p></p></div>");
        let sheet = lumen_css_parser::parse("div { orphans: 5; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.orphans, 5);
        assert_eq!(p_style.orphans, 5);
    }

    #[test]
    fn widows_inherited() {
        let doc = lumen_html_parser::parse("<div><p></p></div>");
        let sheet = lumen_css_parser::parse("div { widows: 6; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.widows, 6);
        assert_eq!(p_style.widows, 6);
    }

    #[test]
    fn orphans_zero_rejected() {
        // 0 не является валидным значением (<integer> >= 1).
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { orphans: 0; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.orphans, 2);
    }

    #[test]
    fn widows_zero_rejected() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { widows: 0; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.widows, 2);
    }

    // ── form controls UA stylesheet ──────────────────────────────────────────

    #[test]
    fn form_input_ua_width_height() {
        let doc = lumen_html_parser::parse("<input type=\"text\">");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.width, Some(Length::Px(174.0)));
        assert_eq!(style.height, Some(Length::Px(21.0)));
        assert_eq!(style.border_top_width, 1.0);
    }

    #[test]
    fn form_input_hidden_display_none() {
        let doc = lumen_html_parser::parse("<input type=\"hidden\">");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::None);
    }

    #[test]
    fn form_input_checkbox_size() {
        let doc = lumen_html_parser::parse("<input type=\"checkbox\">");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.width, Some(Length::Px(13.0)));
        assert_eq!(style.height, Some(Length::Px(13.0)));
    }

    #[test]
    fn form_textarea_ua_dimensions() {
        let doc = lumen_html_parser::parse("<textarea></textarea>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let ta = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, ta, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.width, Some(Length::Px(200.0)));
        assert_eq!(style.height, Some(Length::Px(48.0)));
        assert_eq!(style.border_top_width, 1.0);
    }

    #[test]
    fn form_button_ua_height() {
        let doc = lumen_html_parser::parse("<button>OK</button>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let btn = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, btn, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.height, Some(Length::Px(21.0)));
        assert_eq!(style.border_top_width, 1.0);
    }

    #[test]
    fn form_select_ua_height() {
        let doc = lumen_html_parser::parse("<select></select>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let sel = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, sel, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.height, Some(Length::Px(21.0)));
        assert_eq!(style.border_top_width, 1.0);
    }

    #[test]
    fn form_author_overrides_ua() {
        let doc = lumen_html_parser::parse("<input type=\"text\">");
        let sheet = lumen_css_parser::parse("input { width: 300px; height: 40px; }");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.width, Some(Length::Px(300.0)));
        assert_eq!(style.height, Some(Length::Px(40.0)));
    }

    // CSS color-scheme UA form element colors — F-4

    #[test]
    fn ua_form_colors_light_mode_input() {
        // Light mode: input gets white background and dark border.
        let (border, bg, fg) = ua_form_element_colors("input", false);
        assert_eq!(border, CssColor::Rgba(Color { r: 118, g: 118, b: 118, a: 255 }));
        assert_eq!(bg, CssColor::Rgba(Color { r: 255, g: 255, b: 255, a: 255 }));
        assert_eq!(fg, Color { r: 0, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn ua_form_colors_dark_mode_input() {
        // Dark mode: input gets dark background, lighter border, white text.
        let (border, bg, fg) = ua_form_element_colors("input", true);
        assert_eq!(border, CssColor::Rgba(Color { r: 97, g: 97, b: 97, a: 255 }));
        assert_eq!(bg, CssColor::Rgba(Color { r: 30, g: 30, b: 30, a: 255 }));
        assert_eq!(fg, Color { r: 255, g: 255, b: 255, a: 255 });
    }

    #[test]
    fn ua_form_colors_dark_mode_button_distinct_bg() {
        // Dark mode: button has a lighter background than text inputs.
        let (_, input_bg, _) = ua_form_element_colors("input", true);
        let (_, btn_bg, _) = ua_form_element_colors("button", true);
        assert_ne!(input_bg, btn_bg, "button bg should differ from input bg in dark mode");
        assert_eq!(btn_bg, CssColor::Rgba(Color { r: 58, g: 58, b: 60, a: 255 }));
    }

    #[test]
    fn ua_form_colors_applied_to_computed_style_dark() {
        // When dark_mode=true, compute_style applies dark UA colors to <input>.
        let doc = lumen_html_parser::parse("<input type=\"text\">");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let input = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, input, &sheet, &root, Size::new(800.0, 600.0), true);
        assert_eq!(style.background_color, Some(CssColor::Rgba(Color { r: 30, g: 30, b: 30, a: 255 })));
        assert_eq!(style.color, Color { r: 255, g: 255, b: 255, a: 255 });
        assert_eq!(style.border_top_color, CssColor::Rgba(Color { r: 97, g: 97, b: 97, a: 255 }));
    }

    // CSS Shapes L1 — shape-outside
    #[test]
    fn shape_outside_none_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.shape_outside, ShapeOutside::None);
    }

    #[test]
    fn shape_outside_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { shape-outside: circle(50%); }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.shape_outside, ShapeOutside::Value("circle(50%)".to_string()));
    }

    #[test]
    fn shape_outside_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { shape-outside: circle(50%); }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.shape_outside, ShapeOutside::Value("circle(50%)".to_string()));
        assert_eq!(span_style.shape_outside, ShapeOutside::None);
    }

    #[test]
    fn shape_outside_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { shape-outside: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.shape_outside, ShapeOutside::None);
    }

    // CSS Shapes L1 — shape-margin
    #[test]
    fn shape_margin_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.shape_margin, Length::Px(0.0));
    }

    #[test]
    fn shape_margin_px() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { shape-margin: 10px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.shape_margin, Length::Px(10.0));
    }

    #[test]
    fn shape_margin_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { shape-margin: 5px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.shape_margin, Length::Px(5.0));
        assert_eq!(span_style.shape_margin, Length::Px(0.0));
    }

    #[test]
    fn shape_margin_negative_clamped() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { shape-margin: -5px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        // Negative shape-margin is invalid per spec — ignored.
        assert_eq!(style.shape_margin, Length::Px(0.0));
    }

    // CSS Shapes L1 — shape-image-threshold
    #[test]
    fn shape_image_threshold_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!((style.shape_image_threshold - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn shape_image_threshold_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { shape-image-threshold: 0.5; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!((style.shape_image_threshold - 0.5).abs() < 1e-5);
    }

    #[test]
    fn shape_image_threshold_clamped() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { shape-image-threshold: 1.5; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!((style.shape_image_threshold - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn shape_image_threshold_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { shape-image-threshold: 0.8; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert!((div_style.shape_image_threshold - 0.8).abs() < 1e-5);
        assert!((span_style.shape_image_threshold - 0.0).abs() < f32::EPSILON);
    }

    // CSS Motion Path L1 — offset-path
    #[test]
    fn offset_path_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_path, None);
    }

    #[test]
    fn offset_path_value() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(r#"div { offset-path: path("M 0 0 L 100 100"); }"#);
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(style.offset_path.is_some());
    }

    #[test]
    fn offset_path_none_resets() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(r#"div { offset-path: none; }"#);
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_path, None);
    }

    #[test]
    fn offset_path_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse(r#"div { offset-path: path("M0 0 L100 0"); }"#);
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert!(div_style.offset_path.is_some());
        assert_eq!(span_style.offset_path, None);
    }

    // CSS Motion Path L1 — offset-distance
    #[test]
    fn offset_distance_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_distance, Length::Px(0.0));
    }

    #[test]
    fn offset_distance_px() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { offset-distance: 50px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_distance, Length::Px(50.0));
    }

    #[test]
    fn offset_distance_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { offset-distance: 20px; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.offset_distance, Length::Px(20.0));
        assert_eq!(span_style.offset_distance, Length::Px(0.0));
    }

    #[test]
    fn offset_distance_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { offset-distance: bogus; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_distance, Length::Px(0.0));
    }

    // CSS Motion Path L1 — offset-rotate
    #[test]
    fn offset_rotate_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_rotate, OffsetRotate::Auto);
    }

    #[test]
    fn offset_rotate_reverse() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { offset-rotate: reverse; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_rotate, OffsetRotate::Reverse);
    }

    #[test]
    fn offset_rotate_angle() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { offset-rotate: 90deg; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        if let OffsetRotate::Angle(rad) = style.offset_rotate {
            assert!((rad - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        } else {
            panic!("expected OffsetRotate::Angle");
        }
    }

    #[test]
    fn offset_rotate_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { offset-rotate: reverse; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.offset_rotate, OffsetRotate::Reverse);
        assert_eq!(span_style.offset_rotate, OffsetRotate::Auto);
    }

    // CSS Motion Path L1 — offset-anchor
    #[test]
    fn offset_anchor_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_anchor, None);
    }

    #[test]
    fn offset_anchor_auto() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { offset-anchor: auto; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_anchor, None);
    }

    #[test]
    fn offset_anchor_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { offset-anchor: 50% 50%; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert!(div_style.offset_anchor.is_some());
        assert_eq!(span_style.offset_anchor, None);
    }

    #[test]
    fn offset_anchor_invalid_ignored() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { offset-anchor: bogus; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.offset_anchor, None);
    }

    /// BUG-724: пустое значение 1-4-токенного шорткода не должно ронять поток
    /// вёрстки. React-приложения пишут `style="border-radius: ; height: ;"` для
    /// неустановленных пропсов, и до фикса первый такой узел валил
    /// `lumen-engine` паникой `index out of bounds` в `expand_border_4`.
    #[test]
    fn empty_border_shorthand_is_ignored_not_panic() {
        let doc = lumen_html_parser::parse(
            "<div style=\"border-radius: ; border-width: ; border-color: ;\"></div>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(s.border_top_left_radius, Length::Px(0.0));
        assert_eq!(s.border_top_width, 0.0);
        assert_eq!(expand_border_4(""), ["", "", "", ""]);
        assert_eq!(expand_border_4("   "), ["   ", "   ", "   ", "   "]);
    }

    // ── <dialog> UA display:none rule (HTML5 §15.3.9) ──────────────────────────

    #[test]
    fn dialog_without_open_is_display_none() {
        let doc = lumen_html_parser::parse("<dialog>Hello</dialog>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let dlg = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, dlg, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::None);
    }

    #[test]
    fn dialog_with_open_is_visible() {
        let doc = lumen_html_parser::parse("<dialog open>Hello</dialog>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let dlg = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, dlg, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_ne!(style.display, Display::None);
    }

    #[test]
    fn dialog_author_can_override_ua_display() {
        let doc = lumen_html_parser::parse("<dialog>Hello</dialog>");
        let sheet = lumen_css_parser::parse("dialog { display: block; }");
        let root = ComputedStyle::root();
        let dlg = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, dlg, &sheet, &root, Size::new(800.0, 600.0), false);
        // Author CSS overrides the UA display:none.
        assert_ne!(style.display, Display::None);
    }

    /// HTML rendering §15.5 — form controls default to `display: inline-block`
    /// so they flow horizontally with surrounding inline content.
    #[test]
    fn form_controls_default_to_inline_block() {
        let doc = lumen_html_parser::parse(
            "<input><button>x</button><select></select><textarea></textarea><meter></meter><progress></progress>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let body = doc.get(doc.body().unwrap());
        for &child in &body.children {
            let style = compute_style(&doc, child, &sheet, &root, Size::new(800.0, 600.0), false);
            assert_eq!(
                style.display,
                Display::InlineBlock,
                "form control should default to inline-block"
            );
        }
    }

    /// Author `display:` overrides the UA inline-block default for form controls.
    #[test]
    fn form_control_author_display_overrides_inline_block() {
        let doc = lumen_html_parser::parse("<input>");
        let sheet = lumen_css_parser::parse("input { display: block; }");
        let root = ComputedStyle::root();
        let inp = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, inp, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.display, Display::Block);
    }

    /// HTML rendering §15.5.3 — `<option>` does not paint flow text in a closed
    /// `<select>` (its UA display is `none`). `<optgroup>` stays in flow so the
    /// styles of descendant options are still computed for selector matching.
    #[test]
    fn option_defaults_to_display_none_optgroup_stays_in_flow() {
        let doc = lumen_html_parser::parse(
            "<select><optgroup label=g><option>A</option></optgroup></select>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let sel = doc.get(doc.body().unwrap()).children[0];
        let optgroup = doc.get(sel).children[0];
        let og_style = compute_style(&doc, optgroup, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_ne!(og_style.display, Display::None, "optgroup should stay in flow");
        let option = doc.get(optgroup).children[0];
        let opt_style = compute_style(&doc, option, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(opt_style.display, Display::None, "option should be display:none");
    }

    /// `@media (prefers-color-scheme: dark)` must match when `dark_mode=true`
    /// and must NOT match when `dark_mode=false`.
    #[test]
    fn media_prefers_color_scheme_dark_mode_false() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@media (prefers-color-scheme: dark) { div { color: #ffffff; } }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        // dark_mode=false → media query should NOT match → color stays initial (0,0,0)
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.color.r, 0, "dark_mode=false: @media dark should not apply");
    }

    /// `@media (prefers-color-scheme: dark)` must match when `dark_mode=true`.
    #[test]
    fn media_prefers_color_scheme_dark_mode_true() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@media (prefers-color-scheme: dark) { div { color: #ffffff; } }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        // dark_mode=true → media query matches → color becomes white (255,255,255)
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), true);
        assert_eq!(style.color.r, 255, "dark_mode=true: @media dark should apply");
        assert_eq!(style.color.g, 255);
        assert_eq!(style.color.b, 255);
    }

    /// `@media (prefers-color-scheme: light)` must match when `dark_mode=false`.
    #[test]
    fn media_prefers_color_scheme_light_mode() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(
            "@media (prefers-color-scheme: light) { div { color: #ff0000; } }"
        );
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert_eq!(style.color.r, 255, "light mode: @media light should apply");
        assert_eq!(style.color.g, 0);
        assert_eq!(style.color.b, 0);
    }
