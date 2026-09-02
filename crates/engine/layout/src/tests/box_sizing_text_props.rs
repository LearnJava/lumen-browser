use super::*;

    // ── Relative units: em / rem / % ────────────────────────────────────────

    #[test]
    fn font_size_em_relative_to_parent() {
        // root fs 16 → div fs 20 → p fs 2em = 40.
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 20px; } p { font-size: 2em; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.font_size - 40.0).abs() < 0.01, "got {}", p.style.font_size);
    }

    #[test]
    fn font_size_rem_relative_to_root() {
        // rem всегда от 16 (ROOT_FONT_SIZE), независимо от parent.
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 100px; } p { font-size: 1.5rem; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.font_size - 24.0).abs() < 0.01, "got {}", p.style.font_size);
    }

    #[test]
    fn font_size_percent_relative_to_parent() {
        // 150% от 16 = 24.
        let root = lay("<p>x</p>", "p { font-size: 150%; }");
        let p = first_element_child(&root);
        assert!((p.style.font_size - 24.0).abs() < 0.01, "got {}", p.style.font_size);
    }

    #[test]
    fn padding_em_uses_current_font_size() {
        // padding: 2em должен использовать computed font-size самого элемента,
        // даже если font-size в правиле объявлен после padding.
        let root = lay("<p>x</p>", "p { padding: 2em; font-size: 20px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.padding_top, Length::Em(2.0), "got {:?}", p.style.padding_top);
    }

    #[test]
    fn margin_rem_independent_of_inherit() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 99px; } p { margin: 1rem; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.margin_top, LengthOrAuto::Length(Length::Rem(1.0)));
    }

    #[test]
    fn line_height_percent_becomes_coefficient() {
        // 150% = 1.5.
        let root = lay("<p>x</p>", "p { line-height: 150%; }");
        let p = first_element_child(&root);
        assert!((p.style.line_height - 1.5).abs() < 0.001);
    }

    #[test]
    fn line_height_em_is_coefficient() {
        // 1.5em — то же, что unitless 1.5 (CSS определяет line-height: <number>
        // как «коэффициент * font-size»; em делает то же численно).
        let root = lay("<p>x</p>", "p { line-height: 1.5em; }");
        let p = first_element_child(&root);
        assert!((p.style.line_height - 1.5).abs() < 0.001);
    }

    #[test]
    fn percent_in_margin_stored_typed() {
        // % в margin хранится как Length::Percent и разрешается при layout,
        // когда известна ширина containing block.
        let root = lay("<p>x</p>", "p { margin: 50%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.margin_top, LengthOrAuto::Length(Length::Percent(50.0)));
    }

    // ── Тесты text-align ───────────────────────────────────────────────────

    pub(crate) fn first_inline_run(b: &LayoutBox) -> &LayoutBox {
        for c in &b.children {
            if matches!(c.kind, BoxKind::InlineRun { .. }) {
                return c;
            }
            let found = first_inline_run(c);
            if matches!(found.kind, BoxKind::InlineRun { .. }) {
                return found;
            }
        }
        b
    }

    /// text-align: center сдвигает фрагменты к середине строки.
    /// "ab" = 2×8=16px в контейнере 100px: offset = (100-16)/2 = 42px.
    #[test]
    fn text_align_center_shifts_frags() {
        let root = lay_measured("<p>ab</p>", "p { text-align: center; }", 100.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty(), "expected at least one line");
            let x = lines[0][0].x;
            // (100 - 16) / 2 = 42; p имеет нулевой padding, так что content_width = 100
            assert!((x - 42.0).abs() < 0.5, "expected x≈42, got {x}");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: right сдвигает фрагменты к правому краю.
    /// "ab" = 16px в контейнере 100px: offset = 100-16 = 84px.
    #[test]
    fn text_align_right_shifts_frags() {
        let root = lay_measured("<p>ab</p>", "p { text-align: right; }", 100.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            let x = lines[0][0].x;
            assert!((x - 84.0).abs() < 0.5, "expected x≈84, got {x}");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: left — фрагменты начинаются с x=0.
    #[test]
    fn text_align_left_frags_start_at_zero() {
        let root = lay_measured("<p>ab</p>", "p { text-align: left; }", 100.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            assert!((lines[0][0].x - 0.0).abs() < 0.01, "expected x=0, got {}", lines[0][0].x);
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align наследуется дочерними элементами.
    #[test]
    fn text_align_is_inherited() {
        let root = lay("<div><p>x</p></div>", "div { text-align: right; }");
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.text_align, TextAlign::Right);
    }

    /// text-align: center — последняя строка тоже выравнивается.
    #[test]
    fn text_align_center_applies_to_each_line() {
        // "aa bb" при viewport 30px (3×8=24 < 30; "aa bb" = 40 > 30) → 2 строки.
        // "aa" = 16px, offset = (30-16)/2 = 7; "bb" тоже 16px, offset = 7.
        let root = lay_measured("<p>aa bb</p>", "p { text-align: center; }", 30.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert_eq!(lines.len(), 2, "expected 2 lines");
            for (i, line) in lines.iter().enumerate() {
                let x = line[0].x;
                assert!((x - 7.0).abs() < 0.5, "line[{i}] expected x≈7, got {x}");
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    // ── Тесты CSS width / height ───────────────────────────────────────────

    /// width: 200px задаёт rect.width = 200 (без padding).
    #[test]
    fn explicit_width_sets_rect_width() {
        // viewport 800px; p без padding → rect.width должен быть 200.
        let root = lay("<p>x</p>", "p { width: 200px; }");
        let p = first_element_child(&root);
        assert!(
            (p.rect.width - 200.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// width учитывает padding: rect.width = width + padding_left + padding_right.
    #[test]
    fn explicit_width_plus_padding() {
        let root = lay("<p>x</p>", "p { width: 200px; padding: 10px; }");
        let p = first_element_child(&root);
        // content_box 200 + padding 10+10 = 220.
        assert!(
            (p.rect.width - 220.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// height: 100px задаёт rect.height = 100.
    #[test]
    fn explicit_height_overrides_content_height() {
        let root = lay("<p>x</p>", "p { height: 100px; }");
        let p = first_element_child(&root);
        assert!(
            (p.rect.height - 100.0).abs() < 0.01,
            "rect.height={}", p.rect.height
        );
    }

    /// height учитывает padding: rect.height = height + padding_top + padding_bottom.
    #[test]
    fn explicit_height_plus_padding() {
        let root = lay("<p>x</p>", "p { height: 80px; padding: 5px; }");
        let p = first_element_child(&root);
        assert!(
            (p.rect.height - 90.0).abs() < 0.01,
            "rect.height={}", p.rect.height
        );
    }

    /// Дочерние элементы используют content_width от явно заданного width.
    #[test]
    fn children_constrained_by_explicit_width() {
        // div { width: 300px } → content_width = 300.
        // Вложенный <p> без width → rect.width = content_width = 300.
        let root = lay("<div><p>x</p></div>", "div { width: 300px; }");
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.width - 300.0).abs() < 0.01,
            "p.rect.width={}", p.rect.width
        );
    }

    /// width: auto не устанавливает явную ширину.
    #[test]
    fn width_auto_keeps_auto_layout() {
        let root = lay("<p>x</p>", "p { width: auto; }");
        let p = first_element_child(&root);
        // auto → заполняет viewport 800px.
        assert!(
            (p.rect.width - 800.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// width / height не наследуются.
    #[test]
    fn width_height_not_inherited() {
        let root = lay("<div><p>x</p></div>", "div { width: 400px; height: 200px; }");
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // <p> наследует только inherited properties — width/height нет.
        assert!(p.style.width.is_none(), "width should not be inherited");
        assert!(p.style.height.is_none(), "height should not be inherited");
    }

    // ── Тесты CSS min-/max- ширины и высоты (§10.4) ────────────────────────

    /// max-width режет заданную width вниз.
    #[test]
    fn max_width_clamps_width_down() {
        let root = lay("<p>x</p>", "p { width: 500px; max-width: 300px; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 300.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-width поднимает заданную width вверх.
    #[test]
    fn min_width_clamps_width_up() {
        let root = lay("<p>x</p>", "p { width: 100px; min-width: 250px; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 250.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-width побеждает max-width при конфликте (CSS 2.1 §10.4).
    #[test]
    fn min_width_beats_max_width() {
        let root = lay(
            "<p>x</p>",
            "p { width: 100px; min-width: 400px; max-width: 200px; }",
        );
        let p = first_element_child(&root);
        assert!((p.rect.width - 400.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// max-height режет height вниз.
    #[test]
    fn max_height_clamps_height_down() {
        let root = lay("<p>x</p>", "p { height: 500px; max-height: 200px; }");
        let p = first_element_child(&root);
        assert!((p.rect.height - 200.0).abs() < 0.01, "rect.height={}", p.rect.height);
    }

    /// Находит первый Block-ребёнок, включая разворачивание InlineBlockRow.
    fn first_inline_block_child(b: &LayoutBox) -> &LayoutBox {
        // InlineBlockRow — анонимный контейнер; разворачиваем его.
        for c in &b.children {
            if matches!(c.kind, BoxKind::InlineBlockRow) {
                for ic in &c.children {
                    if matches!(ic.kind, BoxKind::Block) {
                        return ic;
                    }
                }
            }
            if matches!(c.kind, BoxKind::Block) {
                return c;
            }
        }
        panic!("expected at least one inline-block child");
    }

    /// max-height clamps display:inline-block element height.
    #[test]
    fn max_height_clamps_inline_block() {
        let root = lay(
            r#"<div style="width:300px"><div style="display:inline-block;height:160px;max-height:80px;width:60px"></div></div>"#,
            "",
        );
        let outer = first_element_child(&root);
        let ib = first_inline_block_child(outer);
        assert!((ib.rect.height - 80.0).abs() < 0.5,
            "max-height should clamp 160→80, got {}", ib.rect.height);
    }

    /// min-height lifts display:inline-block element height.
    #[test]
    fn min_height_lifts_inline_block() {
        let root = lay(
            r#"<div style="width:300px"><div style="display:inline-block;height:40px;min-height:100px;width:60px"></div></div>"#,
            "",
        );
        let outer = first_element_child(&root);
        let ib = first_inline_block_child(outer);
        assert!((ib.rect.height - 100.0).abs() < 0.5,
            "min-height should lift 40→100, got {}", ib.rect.height);
    }

    /// vertical-align:bottom выравнивает inline-block элементы по нижнему краю.
    #[test]
    fn vertical_align_bottom_inline_block() {
        // Два inline-block элемента с vertical-align:bottom.
        // Высокий (120px) и низкий (60px) должны совпасть по нижнему краю.
        // Без пробелов между тегами, чтобы не было InlineSpace.
        let root = lay(
            r#"<div style="width:500px"><div style="display:inline-block;width:60px;height:60px;vertical-align:bottom"></div><div style="display:inline-block;width:60px;height:120px;vertical-align:bottom"></div></div>"#,
            "* { box-sizing: border-box; }",
        );
        let outer = first_element_child(&root);
        let ibr = outer.children.iter().find(|c| matches!(c.kind, BoxKind::InlineBlockRow))
            .expect("expected InlineBlockRow");
        // Собираем только Block-детей (пропускаем InlineSpace)
        let blocks: Vec<_> = ibr.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks.len(), 2, "expected 2 block children, got {}", blocks.len());
        // Определяем короткий и высокий по высоте
        let (short, tall) = if blocks[0].rect.height < blocks[1].rect.height {
            (blocks[0], blocks[1])
        } else {
            (blocks[1], blocks[0])
        };
        let short_bottom = short.rect.y + short.rect.height;
        let tall_bottom  = tall.rect.y  + tall.rect.height;
        assert!((short_bottom - tall_bottom).abs() < 0.5,
            "bottom edges should match: short_bottom={} tall_bottom={}", short_bottom, tall_bottom);
        // Короткий должен быть сдвинут вниз на (row_h - short_h) = 120 - 60 = 60
        assert!((short.rect.y - 60.0).abs() < 0.5,
            "short elem should be shifted down by 60px, got y={}", short.rect.y);
    }

    /// vertical-align:bottom для inline-block внутри inline-block (nested).
    #[test]
    fn vertical_align_bottom_nested_inline_block() {
        // Структура TEST-11: пара inline-block с vertical-align:bottom внутри
        // внешнего inline-block контейнера с vertical-align:bottom.
        let root = lay(
            r#"<div style="width:974px">
              <div style="display:inline-block;margin-bottom:24px;vertical-align:bottom">
                <div style="display:inline-block;width:60px;height:80px;margin-right:8px;vertical-align:bottom"></div>
                <div style="display:inline-block;width:60px;height:160px;max-height:80px;vertical-align:bottom"></div>
              </div>
            </div>"#,
            "* { box-sizing: border-box; }",
        );
        let outer = first_element_child(&root);
        // outer → InlineBlockRow → pair
        let ibr = outer.children.iter().find(|c| matches!(c.kind, BoxKind::InlineBlockRow))
            .expect("outer InlineBlockRow");
        let pair = ibr.children.iter().find(|c| matches!(c.kind, BoxKind::Block))
            .expect("pair");
        // pair height should be 80px (max-height clamped)
        assert!((pair.rect.height - 80.0).abs() < 0.5,
            "pair height should be 80, got {}", pair.rect.height);
    }

    /// min-height поднимает high content-height до минимума.
    #[test]
    fn min_height_clamps_height_up() {
        // <p> с одной строкой текста и без явной height → ~19px (16*1.2);
        // min-height: 100 → 100.
        let root = lay("<p>x</p>", "p { min-height: 100px; }");
        let p = first_element_child(&root);
        assert!((p.rect.height - 100.0).abs() < 0.01, "rect.height={}", p.rect.height);
    }

    /// max-width: none — ограничение снимается.
    #[test]
    fn max_width_none_means_no_constraint() {
        let root = lay("<p>x</p>", "p { width: 500px; max-width: none; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 500.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// Отрицательные значения отбрасываются (поле остаётся None).
    #[test]
    fn negative_min_max_ignored() {
        let root = lay(
            "<p>x</p>",
            "p { width: 200px; min-width: -50px; max-width: -10px; }",
        );
        let p = first_element_child(&root);
        assert!(p.style.min_width.is_none(), "negative min-width should be rejected");
        assert!(p.style.max_width.is_none(), "negative max-width should be rejected");
        assert!((p.rect.width - 200.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-/max- не наследуются.
    #[test]
    fn min_max_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { min-width: 100px; max-height: 50px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(p.style.min_width.is_none(), "min-width should not be inherited");
        assert!(p.style.max_height.is_none(), "max-height should not be inherited");
        // У div сам должен быть выставлен.
        assert_eq!(div.style.min_width, Some(Length::Px(100.0)));
        assert_eq!(div.style.max_height, Some(Length::Px(50.0)));
    }

    /// max-width в border-box работает как ограничение всей коробки.
    #[test]
    fn max_width_with_border_box_includes_padding() {
        // border-box: max-width=200 — это вся коробка, padding внутри.
        let root = lay(
            "<p>x</p>",
            "p { box-sizing: border-box; width: 500px; max-width: 200px; padding: 10px; }",
        );
        let p = first_element_child(&root);
        assert!((p.rect.width - 200.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-width в content-box: min относится к contentу, padding/border
    /// прибавляются сверху. Подняли width=50 (= rect 70 с padding=10) до
    /// min-width=200 (= rect 220 с padding=10).
    #[test]
    fn min_width_content_box_adds_padding() {
        let root = lay(
            "<p>x</p>",
            "p { width: 50px; min-width: 200px; padding: 10px; }",
        );
        let p = first_element_child(&root);
        assert!((p.rect.width - 220.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    // ── Тесты CSS borders ──────────────────────────────────────────────────

    /// `border: 2px solid red` — shorthand устанавливает ширину, стиль, цвет.
    #[test]
    fn border_shorthand_sets_all_sides() {
        let root = lay("<p>x</p>", "p { border: 2px solid red; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_width - 2.0).abs() < 0.01);
        assert!((p.style.border_right_width - 2.0).abs() < 0.01);
        assert!((p.style.border_bottom_width - 2.0).abs() < 0.01);
        assert!((p.style.border_left_width - 2.0).abs() < 0.01);
        assert_eq!(p.style.border_top_style, BorderStyle::Solid);
        assert_eq!(p.style.border_bottom_style, BorderStyle::Solid);
        let CssColor::Rgba(top_color) = p.style.border_top_color else { panic!("border-color should be set") };
        assert_eq!(top_color.r, 255);
        assert_eq!(top_color.g, 0);
        assert_eq!(top_color.b, 0);
    }

    /// Border увеличивает высоту бокса (border-box sizing).
    #[test]
    fn border_increases_box_height() {
        let root = lay("<p>x</p>", "p { border: 5px solid black; }");
        let p = first_element_child(&root);
        // 19.2 (text) + 5 + 5 = 29.2
        assert!(
            (p.rect.height - 29.2).abs() < 0.1,
            "rect.height={}", p.rect.height
        );
    }

    /// Border увеличивает ширину при явно заданном `width`.
    #[test]
    fn border_plus_explicit_width_adds_to_rect() {
        let root = lay("<p>x</p>", "p { width: 100px; border: 3px solid black; }");
        let p = first_element_child(&root);
        // rect.width = width + border_left + border_right = 100 + 3 + 3 = 106
        assert!(
            (p.rect.width - 106.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// Без border-color поле равно None (currentColor).
    #[test]
    fn border_color_defaults_to_none() {
        let root = lay("<p>x</p>", "p { border: 1px solid; }");
        let p = first_element_child(&root);
        assert!(matches!(p.style.border_top_color, CssColor::CurrentColor), "should be CurrentColor");
    }

    /// `border-top: 3px dashed blue` — только верхняя сторона.
    #[test]
    fn border_side_shorthand_sets_one_side() {
        let root = lay("<p>x</p>", "p { border-top: 3px dashed blue; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_width - 3.0).abs() < 0.01);
        assert_eq!(p.style.border_top_style, BorderStyle::Dashed);
        let CssColor::Rgba(c) = p.style.border_top_color else { panic!("top color set") };
        assert_eq!(c.b, 255);
        // Остальные стороны без изменений.
        assert_eq!(p.style.border_right_width, 0.0);
        assert_eq!(p.style.border_right_style, BorderStyle::None);
    }

    /// `border-style: solid dashed dotted solid` — 4 значения по CSS.
    #[test]
    fn border_style_four_values() {
        let root = lay("<p>x</p>", "p { border-style: solid dashed dotted solid; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_style, BorderStyle::Solid);
        assert_eq!(p.style.border_right_style, BorderStyle::Dashed);
        assert_eq!(p.style.border_bottom_style, BorderStyle::Dotted);
        assert_eq!(p.style.border_left_style, BorderStyle::Solid);
    }

    /// `border: none` — стиль None, ширина 0.
    #[test]
    fn border_none_clears_border() {
        let root = lay("<p>x</p>", "p { border: 5px solid red; border: none; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_style, BorderStyle::None);
    }

    // ── Тесты CSS box-sizing ───────────────────────────────────────────────

    /// content-box (default): rect.width = width + padding + border.
    #[test]
    fn content_box_width_adds_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { width: 100px; padding: 10px; border: 2px solid black; box-sizing: content-box; }",
        );
        let p = first_element_child(&root);
        // 100 (content) + 10*2 (padding) + 2*2 (border) = 124
        assert!(
            (p.rect.width - 124.0).abs() < 0.01,
            "rect.width={}",
            p.rect.width
        );
    }

    /// border-box: rect.width = width (включая padding и border).
    #[test]
    fn border_box_width_includes_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { width: 100px; padding: 10px; border: 2px solid black; box-sizing: border-box; }",
        );
        let p = first_element_child(&root);
        // border-box: rect.width = width = 100
        assert!(
            (p.rect.width - 100.0).abs() < 0.01,
            "rect.width={}",
            p.rect.width
        );
    }

    /// border-box: контент-зона сжимается, чтобы width влез вместе с padding+border.
    #[test]
    fn border_box_children_use_shrunken_content_width() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; padding: 10px; border: 5px solid black; box-sizing: border-box; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // div rect.width = 200. content_width = 200 - 10*2 - 5*2 = 170.
        assert!((div.rect.width - 200.0).abs() < 0.01, "div={}", div.rect.width);
        assert!(
            (p.rect.width - 170.0).abs() < 0.01,
            "p={}",
            p.rect.width
        );
    }

    /// border-box: height тоже включает padding и border.
    #[test]
    fn border_box_height_includes_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { height: 100px; padding: 10px; border: 5px solid black; box-sizing: border-box; }",
        );
        let p = first_element_child(&root);
        assert!(
            (p.rect.height - 100.0).abs() < 0.01,
            "rect.height={}",
            p.rect.height
        );
    }

    /// content-box (default): height = h + padding + border.
    #[test]
    fn content_box_height_adds_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { height: 100px; padding: 10px; border: 5px solid black; }",
        );
        let p = first_element_child(&root);
        // 100 + 10*2 + 5*2 = 130
        assert!(
            (p.rect.height - 130.0).abs() < 0.01,
            "rect.height={}",
            p.rect.height
        );
    }

    /// border-box не меняет поведение, если нет ни padding, ни border.
    #[test]
    fn border_box_equivalent_to_content_box_without_padding_border() {
        let root_cb = lay("<p>x</p>", "p { width: 200px; box-sizing: content-box; }");
        let root_bb = lay("<p>x</p>", "p { width: 200px; box-sizing: border-box; }");
        let p_cb = first_element_child(&root_cb);
        let p_bb = first_element_child(&root_bb);
        assert!((p_cb.rect.width - p_bb.rect.width).abs() < 0.01);
    }

    /// box-sizing не наследуется на уровне layout — у вложенного <p> остаётся content-box.
    #[test]
    fn box_sizing_does_not_inherit_into_child_layout() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-sizing: border-box; } p { width: 100px; padding: 5px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // p использует content-box (default) → 100 + 5*2 = 110.
        assert!(
            (p.rect.width - 110.0).abs() < 0.01,
            "p.rect.width={}",
            p.rect.width
        );
    }

    // ── Тесты :is() и :where() ─────────────────────────────────────────────

    /// `:is(.a, .b)` матчит любой элемент с одним из классов.
    #[test]
    fn pseudo_is_matches_any_of_list() {
        let (root, doc) = lay_with_doc(
            r#"<p class="a">a</p><p class="b">b</p><p class="c">c</p>"#,
            ":is(.a, .b) { color: red; }",
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p") {
                ps.push(c);
            }
        }
        assert_eq!(ps[0].style.color.r, 255, "a should match");
        assert_eq!(ps[1].style.color.r, 255, "b should match");
        assert_eq!(ps[2].style.color.r, 0, "c should not match");
    }

    /// `:is(h1, h2)` с типами.
    #[test]
    fn pseudo_is_matches_type_selectors() {
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><h2>y</h2><h3>z</h3>",
            ":is(h1, h2) { color: red; }",
        );
        let h1 = find_by_tag(&root, "h1", &doc).unwrap();
        let h2 = find_by_tag(&root, "h2", &doc).unwrap();
        let h3 = find_by_tag(&root, "h3", &doc).unwrap();
        assert_eq!(h1.style.color.r, 255);
        assert_eq!(h2.style.color.r, 255);
        assert_eq!(h3.style.color.r, 0);
    }

    /// `:is(...)` корректно работает в составе complex-селектора.
    #[test]
    fn pseudo_is_inside_descendant_complex() {
        let (root, doc) = lay_with_doc(
            "<article><h1>a</h1><h2>b</h2></article><h1>top</h1>",
            "article :is(h1, h2) { color: red; }",
        );
        let article = find_by_tag(&root, "article", &doc).unwrap();
        let h1_in = find_by_tag(article, "h1", &doc).unwrap();
        let h2_in = find_by_tag(article, "h2", &doc).unwrap();
        assert_eq!(h1_in.style.color.r, 255);
        assert_eq!(h2_in.style.color.r, 255);
        // h1 на верхнем уровне не внутри article — не матчит.
        let top_h1 = root
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "h1"))
            .unwrap();
        assert_eq!(top_h1.style.color.r, 0);
    }

    /// `:where(...)` матчит так же, как `:is`, но specificity = 0 — любое более
    /// специфичное правило (например, type-селектор) победит.
    #[test]
    fn pseudo_where_specificity_is_zero() {
        // :where(#x) даёт 0; p имеет specificity (0,0,1). p должен победить.
        let root = lay(
            r#"<p id="x">v</p>"#,
            ":where(#x) { color: red; } p { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255, "p должен выиграть у :where(#x)");
        assert_eq!(p.style.color.r, 0);
    }

    /// `:is(#x)` сохраняет specificity id — побеждает type-селектор.
    #[test]
    fn pseudo_is_keeps_inner_id_specificity() {
        let root = lay(
            r#"<p id="x">v</p>"#,
            ":is(#x) { color: red; } p { color: blue; }",
        );
        let p = first_element_child(&root);
        // :is(#x) даёт (1,0,0); p даёт (0,0,1). Должен выиграть :is.
        assert_eq!(p.style.color.r, 255);
        assert_eq!(p.style.color.b, 0);
    }

    /// `:is` берёт максимальную specificity из списка.
    #[test]
    fn pseudo_is_uses_max_specificity_in_list() {
        // :is(.foo, #x) — даже если матчит .foo, specificity = (1,0,0) от #x.
        // Конкурирующее правило `.foo` с (0,1,0) проигрывает.
        let root = lay(
            r#"<p class="foo">v</p>"#,
            ":is(.foo, #x) { color: red; } .foo { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255, ":is(.foo, #x) должен победить .foo");
    }

    /// Пустые `:is()` / `:where()` — Unsupported, не матчат.
    #[test]
    fn pseudo_is_empty_does_not_match() {
        let root = lay("<p>x</p>", ":is() { color: red; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 0);
    }

    // ── Тесты case-insensitive [attr=val i] ────────────────────────────────

    /// Без флага `i` сравнение значения case-sensitive — `[type=Submit]` не
    /// матчит `type="submit"`.
    #[test]
    fn attr_equals_default_case_sensitive() {
        let root = lay(
            r#"<input type="submit">"#,
            "[type=Submit] { color: red; }",
        );
        let input = first_element_child(&root);
        assert_eq!(input.style.color.r, 0);
    }

    /// Флаг `i` делает `[type=Submit i]` совпадающим с `type="submit"`.
    #[test]
    fn attr_equals_case_insensitive_matches() {
        let root = lay(
            r#"<input type="submit">"#,
            "[type=Submit i] { color: red; }",
        );
        let input = first_element_child(&root);
        assert_eq!(input.style.color.r, 255);
    }

    /// Флаг `s` явно ставит case-sensitive (тождественно отсутствию флага).
    #[test]
    fn attr_equals_case_sensitive_explicit_does_not_match() {
        let root = lay(
            r#"<input type="submit">"#,
            "[type=Submit s] { color: red; }",
        );
        let input = first_element_child(&root);
        assert_eq!(input.style.color.r, 0);
    }

    /// `i` работает с `^=` (префикс). Используем `<p>` — атрибутный селектор
    /// без type-части матчит любой элемент.
    #[test]
    fn attr_prefix_case_insensitive() {
        let root = lay(
            r#"<p data-url="HTTPS://example.com">x</p>"#,
            r#"[data-url^="https" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `$=` (суффикс).
    #[test]
    fn attr_suffix_case_insensitive() {
        let root = lay(
            r#"<p data-file="page.PDF">x</p>"#,
            r#"[data-file$=".pdf" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `*=` (подстрока).
    #[test]
    fn attr_substring_case_insensitive() {
        let root = lay(
            r#"<p data-url="https://EXAMPLE.com/path">x</p>"#,
            r#"[data-url*="example" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `~=` (whitespace-разделённое слово).
    #[test]
    fn attr_includes_case_insensitive() {
        let root = lay(
            r#"<p class="foo BAR baz">x</p>"#,
            r#"[class~="bar" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `|=` (lang-style dash-match).
    #[test]
    fn attr_dashmatch_case_insensitive() {
        let root = lay(
            r#"<p lang="EN-US">x</p>"#,
            r#"[lang|="en" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` — это **ASCII** case-insensitive: cyrillic case различается.
    /// `[lang=РУ i]` не матчит `lang="ру"`.
    #[test]
    fn attr_case_insensitive_does_not_fold_cyrillic() {
        let root = lay(
            r#"<p lang="ру">x</p>"#,
            "[lang=РУ i] { color: red; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.color.r, 0,
            "ASCII case-fold не должен ронять cyrillic case"
        );
    }

    // ── Тесты !important в каскаде (CSS Cascade L4 §8.1) ───────────────────

    /// !important побеждает normal даже при меньшей specificity.
    /// `p { color: red !important }` (0,0,1) должен победить `#x { color: blue }` (1,0,0).
    #[test]
    fn important_beats_higher_specificity() {
        let root = lay(
            r#"<p id="x">v</p>"#,
            "p { color: red !important; } #x { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255, "important должен победить #x");
        assert_eq!(p.style.color.b, 0);
    }

    /// Между двумя !important выигрывает большая specificity.
    #[test]
    fn important_among_two_resolves_by_specificity() {
        let root = lay(
            r#"<p id="x" class="c">v</p>"#,
            "p { color: red !important; } #x { color: blue !important; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255, "#x !important должен победить p !important");
    }

    /// Между двумя !important равной specificity — позже объявленное.
    #[test]
    fn important_with_equal_specificity_later_wins() {
        let root = lay(
            "<p>v</p>",
            "p { color: red !important; } p { color: blue !important; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
        assert_eq!(p.style.color.r, 0);
    }

    /// !important работает поверх inheritance: ребёнок получает важный цвет.
    #[test]
    fn important_inherits_to_child() {
        let root = lay(
            "<div><p>v</p></div>",
            "div { color: red !important; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.color.r, 255);
    }

    /// Без !important specificity решает обычным образом.
    #[test]
    fn normal_cascade_unchanged_without_important() {
        let root = lay(
            r#"<p id="x">v</p>"#,
            "p { color: red; } #x { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
        assert_eq!(p.style.color.r, 0);
    }

    // ── viewport units (vh/vw/vmin/vmax) ───────────────────────────────────

    /// `width: 50vw` — половина ширины viewport. Default lay() — 800x600.
    #[test]
    fn width_vw_uses_viewport() {
        let root = lay("<p>x</p>", "p { width: 50vw; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 400.0).abs() < 0.01, "width = {}", p.rect.width);
    }

    /// `height: 25vh` — четверть высоты viewport.
    #[test]
    fn height_vh_uses_viewport() {
        // 25vh от 600 = 150.
        let root = lay("<p>x</p>", "p { height: 25vh; }");
        let p = first_element_child(&root);
        assert!((p.rect.height - 150.0).abs() < 0.01, "height = {}", p.rect.height);
    }

    /// `padding` через vw.
    #[test]
    fn padding_vw_uses_viewport() {
        // 10vw от 800 = 80.
        let root = lay("<p>x</p>", "p { padding: 10vw; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.padding_top, Length::Vw(10.0));
        assert_eq!(p.style.padding_left, Length::Vw(10.0));
    }

    /// `font-size` через vh влияет на размер шрифта (наследуется в InlineRun).
    #[test]
    fn font_size_vh_uses_viewport() {
        // 5vh от 600 = 30.
        let root = lay("<p>x</p>", "p { font-size: 5vh; }");
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        assert!((inline.style.font_size - 30.0).abs() < 0.01, "fs = {}", inline.style.font_size);
    }

    /// `vmin` — меньшая сторона viewport (800 vs 600 → 600).
    #[test]
    fn width_vmin_uses_smaller_side() {
        // 50vmin от min(800, 600) = 600 → 300.
        let root = lay("<p>x</p>", "p { width: 50vmin; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 300.0).abs() < 0.01, "width = {}", p.rect.width);
    }

    /// `vmax` — большая сторона viewport (800 vs 600 → 800).
    #[test]
    fn width_vmax_uses_larger_side() {
        // 50vmax от max(800, 600) = 800 → 400.
        let root = lay("<p>x</p>", "p { width: 50vmax; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 400.0).abs() < 0.01, "width = {}", p.rect.width);
    }

    /// `border-width` через vh.
    #[test]
    fn border_width_vh_uses_viewport() {
        // 1vh от 600 = 6.
        let root = lay("<p>x</p>", "p { border: 1vh solid red; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_width - 6.0).abs() < 0.01);
        assert!((p.style.border_right_width - 6.0).abs() < 0.01);
    }

    // ── font-style: italic / oblique / normal ───────────────────────────────

    /// `<em>` получает italic через UA stylesheet.
    #[test]
    fn em_element_is_italic_by_default() {
        // <em> внутри <p> — inline; UA stylesheet делает его italic.
        let root = lay("<p>hi <em>there</em></p>", "");
        let p = first_element_child(&root);
        // <p> сам Normal; внутренний фрагмент <em> в InlineRun должен быть Italic.
        assert_eq!(p.style.font_style, FontStyle::Normal);
        let inline = p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { segments, .. } = &inline.kind {
            // Должно быть два сегмента: "hi " (Normal) и "there" (Italic).
            let italic = segments.iter().find(|s| s.style.font_style == FontStyle::Italic);
            assert!(italic.is_some(), "ожидался italic сегмент");
            assert_eq!(italic.unwrap().text, "there");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// `<i>`, `<cite>`, `<dfn>`, `<address>`, `<var>` тоже italic по UA.
    /// Проверяем напрямую через compute_style — обходить дерево не нужно,
    /// тег элемента всегда первый child корня.
    #[test]
    fn i_cite_dfn_address_var_are_italic() {
        for tag in ["i", "cite", "dfn", "address", "var"] {
            let html = format!("<{tag}>x</{tag}>");
            let doc = lumen_html_parser::parse(&html);
            let id = doc.get(doc.body().unwrap()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
                false,
            );
            assert_eq!(style.font_style, FontStyle::Italic, "tag = {tag}");
        }
    }

    /// CSS `font-style: italic` на `<p>`.
    #[test]
    fn font_style_italic_via_css() {
        let root = lay("<p>x</p>", "p { font-style: italic; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_style, FontStyle::Italic);
    }

    /// CSS `font-style: oblique`.
    #[test]
    fn font_style_oblique_via_css() {
        let root = lay("<p>x</p>", "p { font-style: oblique; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_style, FontStyle::Oblique);
    }

    /// CSS `font-style: normal` на `<em>` сбрасывает UA-italic.
    #[test]
    fn font_style_normal_overrides_ua_italic() {
        // Но в InlineRun сегменте — нужно проверить, что override применился.
        // Проще: сделать <em> блочным через display:block + font-style:normal.
        let root = lay(
            "<em>x</em>",
            "em { display: block; font-style: normal; }",
        );
        let em = first_element_child(&root);
        assert_eq!(em.style.font_style, FontStyle::Normal);
    }

    /// font-style наследуется: ребёнок берёт italic от родителя.
    #[test]
    fn font_style_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-style: italic; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_style, FontStyle::Italic);
        assert_eq!(p.style.font_style, FontStyle::Italic);
    }

    // ── font-weight: normal / bold / lighter / bolder / numeric ─────────────

    /// `<strong>` / `<b>` / `<h1>`-`<h6>` / `<th>` получают bold через UA.
    #[test]
    fn semantic_tags_are_bold_by_default() {
        for tag in ["b", "strong", "h1", "h2", "h3", "h4", "h5", "h6", "th"] {
            let html = format!("<{tag}>x</{tag}>");
            let doc = lumen_html_parser::parse(&html);
            let id = doc.get(doc.body().unwrap()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
                false,
            );
            assert_eq!(style.font_weight, FontWeight::BOLD, "tag = {tag}");
        }
    }

    /// UA stylesheet: `<h1>`–`<h6>` получают увеличенный font-size и
    /// вертикальные margin (HTML Rendering §15.3.3). Регрессия BUG-106:
    /// без этих дефолтов заголовки рендерились 16px без отступов, из-за чего
    /// таблицы (TEST-64) уезжали вверх относительно Edge.
    #[test]
    fn headings_get_ua_font_size_and_margins() {
        let root_fs = ComputedStyle::root().font_size;
        // (tag, font-size factor, vertical margin em)
        let cases = [
            ("h1", 2.0_f32, 0.67_f32),
            ("h2", 1.5, 0.83),
            ("h3", 1.17, 1.0),
            ("h4", 1.0, 1.33),
            ("h5", 0.83, 1.67),
            ("h6", 0.67, 2.33),
        ];
        for (tag, size_factor, margin_em) in cases {
            let html = format!("<{tag}>x</{tag}>");
            let doc = lumen_html_parser::parse(&html);
            let id = doc.get(doc.body().unwrap()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
                false,
            );
            assert!(
                (style.font_size - root_fs * size_factor).abs() < 0.01,
                "{tag} font-size: expected {}, got {}",
                root_fs * size_factor,
                style.font_size,
            );
            assert_eq!(
                style.margin_top,
                LengthOrAuto::Length(Length::Em(margin_em)),
                "{tag} margin-top",
            );
            assert_eq!(
                style.margin_bottom,
                LengthOrAuto::Length(Length::Em(margin_em)),
                "{tag} margin-bottom",
            );
        }
    }

    /// UA-дефолты заголовка перекрываются author-CSS (font-size через
    /// pre-pass, margin через main-pass каскада).
    #[test]
    fn heading_ua_defaults_overridden_by_author_css() {
        let doc = lumen_html_parser::parse("<h3>x</h3>");
        let id = doc.get(doc.body().unwrap()).children[0];
        let ss = lumen_css_parser::parse("h3 { font-size: 30px; margin-top: 5px; }");
        let style = crate::style::compute_style(
            &doc,
            id,
            &ss,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert!((style.font_size - 30.0).abs() < 0.01, "author font-size wins");
        assert_eq!(style.margin_top, LengthOrAuto::Length(Length::Px(5.0)));
    }

    /// CSS `font-weight: bold` → 700.
    #[test]
    fn font_weight_bold_keyword() {
        let root = lay("<p>x</p>", "p { font-weight: bold; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_weight, FontWeight(700));
    }

    /// Численное значение.
    #[test]
    fn font_weight_numeric() {
        let root = lay("<p>x</p>", "p { font-weight: 300; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_weight, FontWeight(300));
    }

    /// `lighter` от 700 = 400 (по таблице CSS Fonts L4).
    #[test]
    fn font_weight_lighter_relative_to_parent() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-weight: 700; } p { font-weight: lighter; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_weight, FontWeight(700));
        assert_eq!(p.style.font_weight, FontWeight(400));
    }

    /// `bolder` от 400 = 700.
    #[test]
    fn font_weight_bolder_relative_to_parent() {
        let root = lay(
            "<div><p>x</p></div>",
            "p { font-weight: bolder; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // div наследует normal=400; p получает bolder = 700.
        assert_eq!(div.style.font_weight, FontWeight(400));
        assert_eq!(p.style.font_weight, FontWeight(700));
    }

    /// `font-weight: normal` сбрасывает UA bold у `<strong>`.
    #[test]
    fn font_weight_normal_overrides_ua_bold() {
        let root = lay(
            "<strong>x</strong>",
            "strong { display: block; font-weight: normal; }",
        );
        let strong = first_element_child(&root);
        assert_eq!(strong.style.font_weight, FontWeight::NORMAL);
    }

    /// font-weight наследуется.
    #[test]
    fn font_weight_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-weight: 800; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_weight, FontWeight(800));
    }

    /// Невалидное значение игнорируется.
    #[test]
    fn font_weight_invalid_keeps_inherited() {
        let root = lay(
            "<p>x</p>",
            "p { font-weight: nonsense; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.font_weight, FontWeight::NORMAL);
    }

    // ── text-transform: uppercase / lowercase / capitalize ─────────────────

    /// Достаёт первый текстовый сегмент из InlineRun первого block-child.
    pub(crate) fn first_inline_text(root: &LayoutBox) -> String {
        let p = first_element_child(root);
        for c in &p.children {
            if let BoxKind::InlineRun { segments, .. } = &c.kind
                && let Some(s) = segments.first()
            {
                return s.text.clone();
            }
        }
        panic!("no inline segments found");
    }

    #[test]
    fn text_transform_uppercase_ascii() {
        let root = lay("<p>hello world</p>", "p { text-transform: uppercase; }");
        assert_eq!(first_inline_text(&root), "HELLO WORLD");
    }

    #[test]
    fn text_transform_lowercase_ascii() {
        let root = lay("<p>HELLO World</p>", "p { text-transform: lowercase; }");
        assert_eq!(first_inline_text(&root), "hello world");
    }

    #[test]
    fn text_transform_capitalize_ascii() {
        let root = lay("<p>hello world</p>", "p { text-transform: capitalize; }");
        assert_eq!(first_inline_text(&root), "Hello World");
    }

    #[test]
    fn text_transform_uppercase_cyrillic() {
        // Русские буквы должны нормально case-folиться.
        let root = lay("<p>привет мир</p>", "p { text-transform: uppercase; }");
        assert_eq!(first_inline_text(&root), "ПРИВЕТ МИР");
    }

    #[test]
    fn text_transform_lowercase_cyrillic() {
        let root = lay("<p>ПРИВЕТ Мир</p>", "p { text-transform: lowercase; }");
        assert_eq!(first_inline_text(&root), "привет мир");
    }

    #[test]
    fn text_transform_capitalize_cyrillic() {
        let root = lay("<p>привет мир</p>", "p { text-transform: capitalize; }");
        assert_eq!(first_inline_text(&root), "Привет Мир");
    }

    #[test]
    fn text_transform_none_default() {
        let root = lay("<p>Hello WORLD</p>", "");
        assert_eq!(first_inline_text(&root), "Hello WORLD");
    }

    #[test]
    fn text_transform_inherited() {
        let root = lay(
            "<div><p>hi</p></div>",
            "div { text-transform: uppercase; }",
        );
        let div = first_element_child(&root);
        assert_eq!(div.style.text_transform, TextTransform::Uppercase);
        let p = first_element_child(div);
        assert_eq!(p.style.text_transform, TextTransform::Uppercase);
    }

    // ── text-indent ─────────────────────────────────────────────────────────

    #[test]
    fn text_indent_basic() {
        // Парсинг + применение к ComputedStyle.
        let root = lay("<p>hello</p>", "p { text-indent: 30px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_indent, Length::Px(30.0));
    }

    #[test]
    fn text_indent_em_stores_typed() {
        // text-indent: 2em хранится как Length::Em(2.0); разрешается при layout.
        let root = lay("<p>x</p>", "p { text-indent: 2em; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_indent, Length::Em(2.0));
    }

    #[test]
    fn text_indent_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-indent: 25px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_indent, Length::Px(25.0));
        assert_eq!(p.style.text_indent, Length::Px(25.0));
    }

    #[test]
    fn text_indent_shifts_first_line() {
        // С text-indent первое слово начинается со сдвигом.
        // Используем lay_measured (Fixed8 = 8px на символ) на 800 ширину.
        let root = lay_measured(
            "<p>hi</p>",
            "p { text-indent: 40px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            // Первая строка, первый фрагмент. x должен быть = 40.
            let first_frag = &lines[0][0];
            assert!((first_frag.x - 40.0).abs() < 0.01, "first.x = {}", first_frag.x);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn text_indent_only_first_line() {
        // text-indent применяется только к первой строке. Если контент
        // переносится на 2+ строк, последующие начинаются с x=0.
        // Fixed8: 8px на символ. max_width = 80 → ~10 символов с indent 16.
        let root = lay_measured(
            "<p>aaaa bbbb cccc dddd</p>",
            "p { text-indent: 16px; }",
            80.0,
        );
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            // Первая строка должна стартовать с offset.
            assert!((lines[0][0].x - 16.0).abs() < 0.01, "line[0][0].x = {}", lines[0][0].x);
            // Вторая (и далее) строка стартует с 0.
            assert!(lines.len() > 1, "expected multiple lines, got {}", lines.len());
            assert!((lines[1][0].x - 0.0).abs() < 0.01, "line[1][0].x = {}", lines[1][0].x);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn text_indent_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_indent, Length::Px(0.0));
    }

    // ── letter-spacing ──────────────────────────────────────────────────────

    #[test]
    fn letter_spacing_basic_parse() {
        let root = lay("<p>x</p>", "p { letter-spacing: 4px; }");
        let p = first_element_child(&root);
        assert!((p.style.letter_spacing - 4.0).abs() < 0.01);
    }

    #[test]
    fn letter_spacing_normal_keyword() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { letter-spacing: 5px; } p { letter-spacing: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((div.style.letter_spacing - 5.0).abs() < 0.01);
        assert_eq!(p.style.letter_spacing, 0.0);
    }

    #[test]
    fn letter_spacing_negative() {
        // Отрицательные значения валидны (сжимают текст).
        let root = lay("<p>x</p>", "p { letter-spacing: -2px; }");
        let p = first_element_child(&root);
        assert!((p.style.letter_spacing - (-2.0)).abs() < 0.01);
    }

    #[test]
    fn letter_spacing_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { letter-spacing: 3px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.letter_spacing - 3.0).abs() < 0.01);
    }

    #[test]
    fn letter_spacing_extends_word_width() {
        // 4 char word "abcd" с letter-spacing 5: width = 4*8 + 3*5 = 47.
        // Без letter-spacing было бы 32.
        let root = lay_measured(
            "<p>abcd</p>",
            "p { letter-spacing: 5px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let inline = p.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            let frag = &lines[0][0];
            assert!((frag.width - 47.0).abs() < 0.01, "frag.width = {}", frag.width);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn letter_spacing_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.letter_spacing, 0.0);
    }

    // ── word-spacing ────────────────────────────────────────────────────────

    #[test]
    fn word_spacing_basic_parse() {
        let root = lay("<p>x</p>", "p { word-spacing: 10px; }");
        let p = first_element_child(&root);
        assert!((p.style.word_spacing - 10.0).abs() < 0.01);
    }

    #[test]
    fn word_spacing_normal_keyword() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { word-spacing: 6px; } p { word-spacing: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((div.style.word_spacing - 6.0).abs() < 0.01);
        assert_eq!(p.style.word_spacing, 0.0);
    }

    #[test]
    fn word_spacing_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { word-spacing: 4px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.word_spacing - 4.0).abs() < 0.01);
    }

    #[test]
    fn word_spacing_only_at_word_boundary() {
        // word-spacing влияет только на gap между словами, не на ширину
        // отдельного слова. Сравниваем с/без word-spacing на одно слово.
        // Fixed8: 8px per char. "abcd" один word — word-spacing не должен
        // изменить width.
        let with = lay_measured("<p>abcd</p>", "p { word-spacing: 100px; }", 800.0);
        let without = lay_measured("<p>abcd</p>", "", 800.0);

        let p_with = first_element_child(&with);
        let p_without = first_element_child(&without);
        let inline_w = p_with.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        let inline_wo = p_without.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();

        let w_width = if let BoxKind::InlineRun { lines, .. } = &inline_w.kind {
            lines[0][0].width
        } else { panic!() };
        let wo_width = if let BoxKind::InlineRun { lines, .. } = &inline_wo.kind {
            lines[0][0].width
        } else { panic!() };
        assert!((w_width - wo_width).abs() < 0.01,
            "word-spacing не должен менять ширину одиночного слова: {w_width} vs {wo_width}");
    }

    #[test]
    fn word_spacing_extends_two_word_run() {
        // Два слова "ab cd": Fixed8, без word-spacing = 2*16+8 = 40.
        // С word-spacing 12: 2*16 + (8+12) = 52.
        let root = lay_measured("<p>ab cd</p>", "p { word-spacing: 12px; }", 800.0);
        let p = first_element_child(&root);
        let inline = p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            // Слова сольются в один frag (одинаковый стиль).
            let frag = &lines[0][0];
            assert!((frag.width - 52.0).abs() < 0.01, "merged frag.width = {}", frag.width);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn word_spacing_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.word_spacing, 0.0);
    }

    // ── font-family ─────────────────────────────────────────────────────────

    #[test]
    fn font_family_single_name() {
        let root = lay("<p>x</p>", "p { font-family: Arial; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_family, vec!["Arial".to_string()]);
    }

    #[test]
    fn font_family_priority_list() {
        let root = lay(
            "<p>x</p>",
            "p { font-family: Arial, Helvetica, sans-serif; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Arial".to_string(), "Helvetica".to_string(), "sans-serif".to_string()]
        );
    }

    #[test]
    fn font_family_quoted_with_spaces() {
        let root = lay(
            "<p>x</p>",
            r#"p { font-family: "Times New Roman", serif; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Times New Roman".to_string(), "serif".to_string()]
        );
    }

    #[test]
    fn font_family_unquoted_multiword() {
        // Без кавычек тоже валидно для имён без запятых, whitespace схлопывается.
        let root = lay(
            "<p>x</p>",
            "p { font-family: Times New Roman, serif; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Times New Roman".to_string(), "serif".to_string()]
        );
    }

    #[test]
    fn font_family_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-family: Verdana, sans-serif; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_family, div.style.font_family);
        assert_eq!(p.style.font_family[0], "Verdana");
    }

    #[test]
    fn font_family_default_is_document_serif() {
        // BUG-128: дефолт документа — UA `serif`, а не пустой список (пустой
        // зарезервирован за chrome UI, см. `style::DEFAULT_FONT_FAMILY`).
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_family, vec!["serif".to_string()]);
    }

    #[test]
    fn font_family_single_quotes_also_work() {
        let root = lay(
            "<p>x</p>",
            "p { font-family: 'Open Sans', sans-serif; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Open Sans".to_string(), "sans-serif".to_string()]
        );
    }

    // ── white-space: nowrap ─────────────────────────────────────────────────

    #[test]
    fn white_space_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.white_space, WhiteSpace::Normal);
    }

    #[test]
    fn white_space_nowrap_parsed() {
        let root = lay("<p>x</p>", "p { white-space: nowrap; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.white_space, WhiteSpace::Nowrap);
    }

    #[test]
    fn white_space_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { white-space: nowrap; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.white_space, WhiteSpace::Nowrap);
    }

    #[test]
    fn white_space_nowrap_disables_wrap() {
        // Без nowrap: 4 слова по 2 char + space (8+8+8+8 + 3*8 = 56 px) на 30 px ширине
        // → переносится на несколько строк.
        // С nowrap: всё на одной строке.
        let normal = lay_measured("<p>aa bb cc dd</p>", "", 30.0);
        let nowrap = lay_measured(
            "<p>aa bb cc dd</p>",
            "p { white-space: nowrap; }",
            30.0,
        );

        let n_p = first_element_child(&normal);
        let nw_p = first_element_child(&nowrap);
        let n_inline = n_p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        let nw_inline = nw_p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();

        let n_lines = if let BoxKind::InlineRun { lines, .. } = &n_inline.kind {
            lines.len()
        } else { panic!() };
        let nw_lines = if let BoxKind::InlineRun { lines, .. } = &nw_inline.kind {
            lines.len()
        } else { panic!() };

        assert!(n_lines > 1, "default ожидает перенос на несколько строк, got {n_lines}");
        assert_eq!(nw_lines, 1, "nowrap должен дать одну строку");
    }

    #[test]
    fn white_space_normal_keyword_resets_inherited_nowrap() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { white-space: nowrap; } p { white-space: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.white_space, WhiteSpace::Nowrap);
        assert_eq!(p.style.white_space, WhiteSpace::Normal);
    }

    // ── opacity ─────────────────────────────────────────────────────────────

    #[test]
    fn opacity_default_one() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_number_value() {
        let root = lay("<p>x</p>", "p { opacity: 0.5; }");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_percent_value() {
        let root = lay("<p>x</p>", "p { opacity: 25%; }");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_clamped_below_zero() {
        let root = lay("<p>x</p>", "p { opacity: -0.5; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.opacity, 0.0);
    }

    #[test]
    fn opacity_clamped_above_one() {
        let root = lay("<p>x</p>", "p { opacity: 2.5; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.opacity, 1.0);
    }

    #[test]
    fn opacity_not_inherited() {
        // CSS opacity не наследуется в layout cascade (визуально она применяется
        // ко всему layer-у, но в computed-style-каскаде каждый элемент имеет
        // свой opacity = 1 по умолчанию).
        let root = lay(
            "<div><p>x</p></div>",
            "div { opacity: 0.3; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((div.style.opacity - 0.3).abs() < f32::EPSILON);
        assert!((p.style.opacity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_invalid_keeps_default() {
        let root = lay("<p>x</p>", "p { opacity: nonsense; }");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 1.0).abs() < f32::EPSILON);
    }
