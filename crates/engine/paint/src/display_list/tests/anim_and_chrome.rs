//! P1/SPLIT-DL3: хвост тестового модуля `mod tests` в `display_list.rs` —
//! static/animated split (EXPERIMENT.md §2)/text-emphasis/clip-path/
//! column-rules/position:sticky и position:fixed/list marker rendering/
//! background-blend-mode/BoxModelOverlay/MaskMode + PushMaskLayer/
//! PushScrollLayer/DrawScrollbar/PageBreak и print display list/
//! apply_print_color_adjust/DrawCrossFade. Перенесено байт-в-байт из
//! `display_list.rs` без дедента (приём ST-1/DL-1).
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-3).

use super::*;
// P1/SPLIT-DL2: hash_corpus/red_fill/debug_hash_one живут в
// display_list/tests/svg_table_and_hash.rs (батч DL-2), но их зовут тесты
// этого файла (батч DL-3).
use super::svg_table_and_hash::{debug_hash_one, hash_corpus, red_fill};
// build уехал в display_list/tests/text_and_images.rs (батч DL-6).
use super::text_and_images::build;
// build_ordered уехал в display_list/tests/ordered_build_scroll.rs (батч DL-5).
use super::ordered_build_scroll::build_ordered;
// find_bg_node уехал в display_list/tests/shadows_and_transforms.rs (батч DL-4).
use super::shadows_and_transforms::find_bg_node;
use lumen_dom::NodeId;

    // ── Static/animated split (EXPERIMENT.md §2) ─────────────────────────────

    /// Push/Pop-глубина среза сбалансирована и не уходит ниже нуля.
    fn assert_segment_balanced(seg: &[DisplayCommand]) {
        let mut depth: i32 = 0;
        for c in seg {
            match c {
                DisplayCommand::PushTransform { .. }
                | DisplayCommand::PushClipRect { .. }
                | DisplayCommand::PushClipRoundedRect { .. }
                | DisplayCommand::PushClipPath { .. }
                | DisplayCommand::PushOpacity { .. }
                | DisplayCommand::PushBlendMode { .. }
                | DisplayCommand::PushFilter { .. }
                | DisplayCommand::PushBackdropFilter { .. }
                | DisplayCommand::PushMaskImage { .. }
                | DisplayCommand::PushMaskLinearGradient { .. }
                | DisplayCommand::PushMaskRadialGradient { .. }
                | DisplayCommand::PushMaskConicGradient { .. }
                | DisplayCommand::PushMaskLayer { .. }
                | DisplayCommand::PushScrollLayer { .. }
                | DisplayCommand::BeginStickyLayer { .. } => depth += 1,
                DisplayCommand::PopTransform
                | DisplayCommand::PopClip
                | DisplayCommand::PopOpacity
                | DisplayCommand::PopBlendMode
                | DisplayCommand::PopFilter
                | DisplayCommand::PopBackdropFilter
                | DisplayCommand::PopMask
                | DisplayCommand::PopMaskLayer
                | DisplayCommand::PopScrollLayer
                | DisplayCommand::EndStickyLayer => {
                    depth -= 1;
                    assert!(depth >= 0, "Pop ниже входной глубины сегмента");
                }
                _ => {}
            }
        }
        assert_eq!(depth, 0, "сегмент несбалансирован по Push/Pop");
    }

    fn split_fixture(
        html: &str,
        overrides: HashMap<NodeId, CompositorOverride>,
    ) -> (DisplayList, Vec<std::ops::Range<usize>>, DisplayList) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let (list, ranges) = build_display_list_ordered_with_anim_split(
            &tree,
            &stacking_tree,
            &order,
            Some(&frame),
        );
        let plain =
            build_display_list_ordered_with_anim(&tree, &stacking_tree, &order, Some(&frame));
        (list, ranges, plain)
    }

    #[test]
    fn anim_split_list_identical_to_with_anim() {
        // Split-сборка обязана давать байт-в-байт тот же список, что обычная
        // anim-сборка — диапазоны лишь метаданные поверх него.
        let html = r#"<div style="background:#008000;width:100px;height:50px"></div>
            <div style="background:#123456;width:100px;height:50px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let green = Color { r: 0, g: 0x80, b: 0, a: 255 };
        let node = find_bg_node(&tree, green).expect("box with green background");
        let mut overrides = HashMap::new();
        overrides.insert(
            node,
            CompositorOverride { opacity: Some(0.4), ..Default::default() },
        );
        let (list, ranges, plain) = split_fixture(html, {
            let mut o = HashMap::new();
            o.insert(node, CompositorOverride { opacity: Some(0.4), ..Default::default() });
            o
        });
        assert_eq!(list, plain, "split-список должен совпадать с anim-списком");
        assert!(!ranges.is_empty(), "override на боксе должен дать диапазон");
    }

    #[test]
    fn anim_split_range_covers_animated_box_only() {
        // Два соседних бокса; transform-override на зелёном. Его заливка —
        // внутри диапазона, заливка соседа — снаружи; сегмент сбалансирован.
        let html = r#"<div style="background:#008000;width:100px;height:50px"></div>
            <div style="background:#123456;width:100px;height:50px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let green = Color { r: 0, g: 0x80, b: 0, a: 255 };
        let other = Color { r: 0x12, g: 0x34, b: 0x56, a: 255 };
        let node = find_bg_node(&tree, green).expect("box with green background");
        let mut overrides = HashMap::new();
        overrides.insert(
            node,
            CompositorOverride {
                transform: Some(vec![lumen_layout::TransformFn::Translate(30.0, 0.0)]),
                ..Default::default()
            },
        );
        let (list, ranges, _) = split_fixture(html, overrides);
        assert_eq!(ranges.len(), 1, "ровно один анимируемый сегмент");
        let r = ranges[0].clone();
        let in_range = |i: usize| i >= r.start && i < r.end;
        let green_idx = list
            .iter()
            .position(|c| matches!(c, DisplayCommand::FillRect { color, .. } if *color == green))
            .expect("зелёная заливка");
        let other_idx = list
            .iter()
            .position(|c| matches!(c, DisplayCommand::FillRect { color, .. } if *color == other))
            .expect("заливка соседа");
        assert!(in_range(green_idx), "заливка анимируемого бокса — в диапазоне");
        assert!(!in_range(other_idx), "заливка статичного соседа — вне диапазона");
        assert!(
            list[r.clone()]
                .iter()
                .any(|c| matches!(c, DisplayCommand::PushTransform { .. })),
            "override-transform внутри сегмента"
        );
        assert_segment_balanced(&list[r]);
    }

    #[test]
    fn anim_split_root_override_yields_no_ranges() {
        let html = r#"<div style="background:#008000;width:100px;height:50px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let mut overrides = HashMap::new();
        overrides.insert(
            tree.node,
            CompositorOverride { opacity: Some(0.5), ..Default::default() },
        );
        let (_, ranges, _) = split_fixture(html, overrides);
        assert!(ranges.is_empty(), "override на корне — split неприменим");
    }

    #[test]
    fn hash_skipping_equals_materialized_static() {
        let html = r#"<div style="background:#008000;width:100px;height:50px;border:2px solid #000"></div>
            <div style="background:#123456;width:100px;height:50px;border:2px solid #fff"></div>
            <div style="background:#654321;width:60px;height:20px"></div>"#;
        let dl = build_ordered(html, "");
        assert!(dl.len() >= 4, "нужен список из нескольких команд, есть {}", dl.len());
        let skip = vec![1usize..2, 3usize..4];
        let mut materialized: DisplayList = Vec::new();
        let mut prev = 0usize;
        for r in &skip {
            materialized.extend_from_slice(&dl[prev..r.start]);
            prev = r.end;
        }
        materialized.extend_from_slice(&dl[prev..]);
        let h_skip = hash_display_list_skipping(&dl, &skip, &[], 0.0, 0.0, 1024, 720);
        let h_mat = hash_display_list(&materialized, &[], 0.0, 0.0, 1024, 720);
        assert_eq!(h_skip, h_mat, "skip-хэш должен совпадать с хэшем статики");
        // Пустой skip эквивалентен обычному хэшу.
        assert_eq!(
            hash_display_list_skipping(&dl, &[], &[], 0.0, 0.0, 1024, 720),
            hash_display_list(&dl, &[], 0.0, 0.0, 1024, 720),
        );
    }

    /// Гейт среза 35 (BUG-405, пункт 70) — ключ полосы из слитого хэша
    /// scroll-инвариантен и равен ключу материализованной статики.
    ///
    /// Это свойство, на котором стоит полоса: кадр с выколотыми сегментами и
    /// кадр, где тех же сегментов нет вовсе, обязаны дать один ключ, иначе
    /// каждый анимационный тик читался бы как смена содержимого. Побитового
    /// равенства со старой парой у слитого хэша нет по построению (см. его
    /// док), поэтому гейтим свойства, а не числа.
    #[test]
    fn dual_key_equals_materialized_static() {
        let content = hash_corpus();
        let skip = vec![1usize..3, 5usize..9];
        let mut materialized: DisplayList = Vec::new();
        let mut prev = 0usize;
        for r in &skip {
            materialized.extend_from_slice(&content[prev..r.start]);
            prev = r.end;
        }
        materialized.extend_from_slice(&content[prev..]);

        let overlay = vec![DisplayCommand::FillRect {
            rect: Rect::new(1.0, 2.0, 3.0, 4.0),
            color: Color { r: 1, g: 2, b: 3, a: 4 },
        }];
        let (_, key_skipped) =
            hash_display_list_dual(&content, &overlay, &skip, (0.0, 40.0), (1024, 720), (1024, 1800));
        let (_, key_materialized) =
            hash_display_list_dual(&materialized, &[], &[], (0.0, 0.0), (800, 600), (1024, 1800));
        assert_eq!(
            key_skipped, key_materialized,
            "ключ полосы обязан зависеть только от статики и размеров полосы",
        );

        // Скролл, размер поверхности и overlay в ключ не входят; размер полосы
        // входит.
        let (_, key_scrolled) =
            hash_display_list_dual(&content, &[], &skip, (7.0, 999.0), (640, 480), (1024, 1800));
        assert_eq!(key_skipped, key_scrolled, "ключ обязан быть scroll-инвариантен");
        let (_, key_other_band) =
            hash_display_list_dual(&content, &overlay, &skip, (0.0, 40.0), (1024, 720), (1024, 2400));
        assert_ne!(key_skipped, key_other_band, "смена размера полосы обязана менять ключ");
    }

    /// Гейт среза 35: хэш кадра из слитого прохода различает всё, что различал
    /// раздельный, — состояние вьюпорта, полосы `content`/`overlay` и любую
    /// команду, которую [`hash_command_into`] считает разной.
    #[test]
    fn dual_frame_hash_is_total() {
        let dual = |c: &[DisplayCommand], o: &[DisplayCommand], sx, sy, w, h| {
            hash_display_list_dual(c, o, &[], (sx, sy), (w, h), (1024, 1800)).0
        };
        let content = vec![red_fill(5.0)];
        let base = dual(&content, &[], 0.0, 0.0, 1024, 720);
        assert_eq!(base, dual(&content, &[], 0.0, 0.0, 1024, 720), "детерминизм");
        assert_ne!(base, dual(&[red_fill(6.0)], &[], 0.0, 0.0, 1024, 720), "команда");
        assert_ne!(base, dual(&content, &[], 0.0, 40.0, 1024, 720), "scroll_y");
        assert_ne!(base, dual(&content, &[], 12.0, 0.0, 1024, 720), "scroll_x");
        assert_ne!(base, dual(&content, &[], 0.0, 0.0, 800, 720), "width");
        assert_ne!(base, dual(&content, &[], 0.0, 0.0, 1024, 600), "height");
        // Полоса, в которой лежит команда, значима: перенос из content в
        // overlay обязан менять хэш.
        assert_ne!(
            dual(&content, &[], 0.0, 0.0, 1024, 720),
            dual(&[], &content, 0.0, 0.0, 1024, 720),
            "полоса команды",
        );
        // Дайджест на команду не должен огрублять фолд: всё, что различает
        // Debug, обязано различать и пара «кадр + ключ».
        let corpus = hash_corpus();
        for (i, a) in corpus.iter().enumerate() {
            for (j, b) in corpus.iter().enumerate().skip(i + 1) {
                if debug_hash_one(a) != debug_hash_one(b) {
                    assert_ne!(
                        dual(std::slice::from_ref(a), &[], 0.0, 0.0, 1024, 720),
                        dual(std::slice::from_ref(b), &[], 0.0, 0.0, 1024, 720),
                        "слитый хэш грубее Debug на corpus[{i}] vs corpus[{j}]",
                    );
                }
            }
        }
    }

    /// Гейт среза 39 (BUG-405): переиспользованная свёртка content-части даёт
    /// РОВНО ту же пару хэшей, что и полный обход.
    ///
    /// Это условие корректности мемоизации: кадр решает по этим числам, можно
    /// ли пропустить отрисовку, поэтому расхождение показало бы устаревшие
    /// пиксели. Проверяется на всех входах, которые в свёртку не входят и
    /// дописываются поверх неё каждый кадр.
    #[test]
    fn memo_fold_matches_full_walk() {
        let content = hash_corpus();
        let overlay = vec![red_fill(9.0)];
        let skip = vec![1usize..3, 5usize..9];
        let folds = fold_content_dual(&content, &skip);

        // Кортеж входов, которые кадр дописывает поверх свёртки: скролл,
        // размер поверхности, размер полосы, наличие overlay.
        type HashInputs = ((f32, f32), (u32, u32), (u32, u32), bool);
        let cases: [HashInputs; 5] = [
            ((0.0, 0.0), (1024, 720), (1024, 1800), false),
            ((0.0, 40.0), (1024, 720), (1024, 1800), true),
            ((12.5, 999.0), (640, 480), (1024, 1800), true),
            ((0.0, 40.0), (1024, 720), (1024, 2400), true),
            ((0.0, 40.0), (800, 600), (800, 1400), false),
        ];
        for (scroll, surface, band, with_overlay) in cases {
            let ov: &[DisplayCommand] = if with_overlay { &overlay } else { &[] };
            let full = hash_display_list_dual(&content, ov, &skip, scroll, surface, band);
            let (memo, used) = hash_display_list_dual_memo(
                &content,
                ov,
                &skip,
                scroll,
                surface,
                band,
                Some(folds),
            );
            assert_eq!(
                full, memo,
                "мемоизация разошлась с полным обходом на {scroll:?}/{surface:?}/{band:?}",
            );
            assert_eq!(used, folds, "кадр обязан вернуть ту свёртку, которой считал");
        }
    }

    /// Гейт среза 39: свёртка меняется на ЛЮБОМ изменении списка или набора
    /// выколотых диапазонов — именно она и есть то, что версия обязана
    /// сторожить. Плюс `None` считает то же, что готовая свёртка.
    #[test]
    fn fold_tracks_content_and_skip() {
        let content = hash_corpus();
        let skip = [1usize..3, 5usize..7];
        let base = fold_content_dual(&content, &skip);

        // Правка НА МЕСТЕ — тот же адрес и та же длина, поэтому её ловит только
        // версия; свёртка обязана её видеть.
        let mut patched = content.clone();
        patched[0] = red_fill(1234.0);
        assert_ne!(base, fold_content_dual(&patched, &skip), "правка команды");

        let mut shorter = content.clone();
        shorter.pop();
        assert_ne!(base, fold_content_dual(&shorter, &skip), "длина списка");

        let other_skip = [2usize..4, 5usize..7];
        assert_ne!(
            base.1,
            fold_content_dual(&content, &other_skip).1,
            "набор выколотых диапазонов входит в ключ полосы",
        );

        // `None` — просто «посчитать заново», результат обязан совпасть.
        let (hashes_none, folds_none) = hash_display_list_dual_memo(
            &content,
            &[],
            &skip,
            (0.0, 40.0),
            (1024, 720),
            (1024, 1800),
            None,
        );
        assert_eq!(folds_none, base, "None обязан посчитать ту же свёртку");
        assert_eq!(
            hashes_none,
            hash_display_list_dual(&content, &[], &skip, (0.0, 40.0), (1024, 720), (1024, 1800)),
            "None обязан дать те же хэши, что и старая функция",
        );
    }

    #[test]
    fn compose_plan_replays_enclosing_context() {
        use lumen_layout::property_trees::Mat4 as M;
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let content = vec![
            DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 500.0, 500.0) },
            DisplayCommand::PushTransform { matrix: M::translation_2d(10.0, 10.0) },
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 50.0, 50.0), color: red },
            // сегмент: анимируемый бокс
            DisplayCommand::PushTransform { matrix: M::translation_2d(100.0, 0.0) },
            DisplayCommand::FillRect { rect: Rect::new(200.0, 0.0, 40.0, 40.0), color: red },
            DisplayCommand::PopTransform,
            // конец сегмента
            DisplayCommand::PopTransform,
            DisplayCommand::PopClip,
        ];
        let ranges = std::slice::from_ref(&(3usize..6));
        let (plan, eff) = anim_split_compose_plan(&content, ranges)
            .expect("контекст clip+transform реплеябелен");
        assert_eq!(eff, vec![3usize..6], "без конфликтов диапазоны не меняются");
        // Реплей: PushClipRect + PushTransform, сегмент (3 команды), два Pop-а.
        assert_eq!(plan.len(), 2 + 3 + 2);
        assert!(matches!(plan[0], DisplayCommand::PushClipRect { .. }));
        assert!(matches!(plan[1], DisplayCommand::PushTransform { .. }));
        assert!(matches!(plan[plan.len() - 2], DisplayCommand::PopTransform));
        assert!(matches!(plan[plan.len() - 1], DisplayCommand::PopClip));
        assert_segment_balanced(&plan);
    }

    #[test]
    fn compose_plan_tail_splits_on_overlapping_later_static() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // Статичная команда ПОСЛЕ сегмента перекрывает его bbox — она (и всё
        // после неё) уходит в оверлей tail-split-ом, painter's order сохранён.
        let content = vec![
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 40.0, 40.0), color: red },
            DisplayCommand::FillRect { rect: Rect::new(20.0, 20.0, 40.0, 40.0), color: red },
        ];
        let (plan, eff) =
            anim_split_compose_plan(&content, std::slice::from_ref(&(0usize..1)))
                .expect("конфликт решается tail-split-ом");
        assert_eq!(eff, vec![0usize..1, 1usize..2], "хвост от конфликта до конца");
        assert_eq!(plan.len(), 2, "сегмент + хвост, без реплей-обёрток");
        // Непересекающаяся статика — план строится без хвоста.
        let content_ok = vec![
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 40.0, 40.0), color: red },
            DisplayCommand::FillRect { rect: Rect::new(100.0, 100.0, 40.0, 40.0), color: red },
        ];
        let (_, eff_ok) =
            anim_split_compose_plan(&content_ok, std::slice::from_ref(&(0usize..1)))
                .expect("непересекающаяся статика");
        assert_eq!(eff_ok, vec![0usize..1]);
    }

    #[test]
    fn compose_plan_bails_when_tail_cut_too_early() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // Конфликт в самом начале длинного списка: хвост поглотил бы больше
        // половины — полоса вырождается, split отклоняется целиком.
        let mut content = vec![
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 40.0, 40.0), color: red },
            DisplayCommand::FillRect { rect: Rect::new(20.0, 20.0, 40.0, 40.0), color: red },
        ];
        for k in 0..6 {
            content.push(DisplayCommand::FillRect {
                rect: Rect::new(1000.0 + 100.0 * k as f32, 1000.0, 10.0, 10.0),
                color: red,
            });
        }
        assert!(anim_split_compose_plan(&content, std::slice::from_ref(&(0usize..1))).is_none());
    }

    #[test]
    fn compose_plan_bails_on_non_replayable_context() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // Сегмент внутри opacity-группы: реплей исказил бы групповую
        // композицию — план не строится.
        let content = vec![
            DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 40.0, 40.0), color: red },
            DisplayCommand::PopOpacity,
        ];
        assert!(anim_split_compose_plan(&content, std::slice::from_ref(&(1usize..2))).is_none());
    }

    #[test]
    fn compose_plan_respects_transformed_overlap() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        use lumen_layout::property_trees::Mat4 as M;
        // Сегмент сдвинут transform-ом на (200, 0): без учёта матрицы его
        // локальный rect (0,0,40,40) «пересёкся» бы с поздней статикой в
        // (10,10) — но эффективные координаты не пересекаются.
        let content = vec![
            DisplayCommand::PushTransform { matrix: M::translation_2d(200.0, 0.0) },
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 40.0, 40.0), color: red },
            DisplayCommand::PopTransform,
            DisplayCommand::FillRect { rect: Rect::new(10.0, 10.0, 20.0, 20.0), color: red },
        ];
        let (_, eff) = anim_split_compose_plan(&content, std::slice::from_ref(&(0usize..3)))
            .expect("эффективные координаты не пересекаются");
        assert_eq!(eff, vec![0usize..3], "без конфликта — без хвоста");
        // А статика, накрывающая сдвинутую позицию, — пересекается: хвост.
        let content_hit = vec![
            DisplayCommand::PushTransform { matrix: M::translation_2d(200.0, 0.0) },
            DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 40.0, 40.0), color: red },
            DisplayCommand::PopTransform,
            DisplayCommand::FillRect { rect: Rect::new(210.0, 10.0, 20.0, 20.0), color: red },
        ];
        let (_, eff_hit) =
            anim_split_compose_plan(&content_hit, std::slice::from_ref(&(0usize..3)))
                .expect("конфликт решается tail-split-ом");
        assert_eq!(eff_hit, vec![0usize..3, 3usize..4]);
    }

    // ── text-emphasis rendering ───────────────────────────────────────────────

    #[test]
    fn text_emphasis_filled_circle_emits_marks_above_text() {
        let dl = build(
            "<p>ab</p>",
            "p { text-emphasis-style: filled circle; font-size: 16px; }",
        );
        // Должен быть основной DrawText + 2 DrawText-а для marks (по одному на символ).
        let texts: Vec<_> = dl
            .iter()
            .filter_map(|c| {
                if let DisplayCommand::DrawText { text, .. } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        // Два mark DrawText-а с символом ● (U+25CF).
        let mark_count = texts.iter().filter(|&&t| t == "\u{25CF}").count();
        assert_eq!(mark_count, 2, "по одному mark на каждый символ 'a' и 'b'");
    }

    #[test]
    fn text_emphasis_none_emits_no_marks() {
        let dl = build("<p>ab</p>", "p { font-size: 16px; }");
        let texts: Vec<_> = dl
            .iter()
            .filter_map(|c| {
                if let DisplayCommand::DrawText { text, .. } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        // Только один DrawText с "ab", никаких mark-ов.
        assert_eq!(texts.len(), 1, "без text-emphasis — только основной DrawText");
        assert_eq!(texts[0], "ab");
    }

    #[test]
    fn text_emphasis_under_position_mark_below_text() {
        let dl = build(
            "<p>x</p>",
            "p { text-emphasis-style: filled dot; text-emphasis-position: under right; font-size: 16px; }",
        );
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| {
                if let DisplayCommand::DrawText { rect, text, .. } = c {
                    if text == "\u{2022}" { Some(*rect) } else { None }
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(rects.len(), 1, "один mark для 'x'");
        // Ищем основной DrawText для сравнения y.
        let base_y = dl.iter().find_map(|c| {
            if let DisplayCommand::DrawText { rect, text, .. } = c {
                if text == "x" { Some(rect.y) } else { None }
            } else {
                None
            }
        });
        if let Some(base_y) = base_y {
            assert!(
                rects[0].y > base_y,
                "under mark должен быть ниже текста: mark_y={} base_y={}",
                rects[0].y, base_y
            );
        }
    }

    #[test]
    fn text_emphasis_custom_string_used_as_mark() {
        let dl = build(
            "<p>abc</p>",
            "p { text-emphasis-style: \"*\"; font-size: 16px; }",
        );
        let mark_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "*"))
            .count();
        assert_eq!(mark_count, 3, "три символа → три mark '*'");
    }

    // ── clip-path ──────────────────────────────────────────────────────────

    #[test]
    fn clip_path_inset_1() {
        use super::clip_path_to_rect;
        use lumen_layout::{ClipPath, ShapeValue};
        let r = Rect::new(10.0, 20.0, 100.0, 80.0);
        let clip = ClipPath::Inset(vec![ShapeValue::Px(5.0)]);
        let cr = clip_path_to_rect(&clip, r);
        assert_eq!(cr, Rect::new(15.0, 25.0, 90.0, 70.0));
    }

    #[test]
    fn clip_path_inset_4() {
        use super::clip_path_to_rect;
        use lumen_layout::{ClipPath, ShapeValue};
        let r = Rect::new(0.0, 0.0, 200.0, 100.0);
        // top=10 right=20 bottom=30 left=40
        let clip = ClipPath::Inset(vec![
            ShapeValue::Px(10.0),
            ShapeValue::Px(20.0),
            ShapeValue::Px(30.0),
            ShapeValue::Px(40.0),
        ]);
        let cr = clip_path_to_rect(&clip, r);
        assert_eq!(cr, Rect::new(40.0, 10.0, 140.0, 60.0));
    }

    /// BUG-140: проценты inset — top/bottom от height, left/right от width.
    #[test]
    fn clip_path_inset_percent() {
        use super::clip_path_to_rect;
        use lumen_layout::{ClipPath, ShapeValue};
        let r = Rect::new(0.0, 0.0, 200.0, 100.0);
        let clip = ClipPath::Inset(vec![ShapeValue::Pct(10.0)]);
        let cr = clip_path_to_rect(&clip, r);
        // top/bottom = 10% от 100 = 10; left/right = 10% от 200 = 20
        assert_eq!(cr, Rect::new(20.0, 10.0, 160.0, 80.0));
    }

    #[test]
    fn clip_path_circle_default_center() {
        use super::{clip_path_to_rect, clip_path_to_shape, ResolvedClipShape};
        use lumen_layout::{ClipPath, ShapeValue};
        let r = Rect::new(0.0, 0.0, 100.0, 60.0);
        let clip = ClipPath::Circle { radius: ShapeValue::Px(25.0), center: None };
        let cr = clip_path_to_rect(&clip, r);
        // center = (50, 30); bounding box = (25, 5, 50, 50)
        assert_eq!(cr, Rect::new(25.0, 5.0, 50.0, 50.0));
        let shape = clip_path_to_shape(&clip, r);
        assert_eq!(shape, Some(ResolvedClipShape::Circle { cx: 50.0, cy: 30.0, r: 25.0 }));
    }

    /// BUG-140 (TEST-109 c0): `circle(40% at 50% 50%)` — радиус от
    /// sqrt(w²+h²)/√2, центр от width/height.
    #[test]
    fn clip_path_circle_percent_radius() {
        use super::{clip_path_to_shape, ResolvedClipShape};
        use lumen_layout::{ClipPath, ShapeValue};
        let r = Rect::new(100.0, 200.0, 220.0, 220.0);
        let clip = ClipPath::Circle {
            radius: ShapeValue::Pct(40.0),
            center: Some((ShapeValue::Pct(50.0), ShapeValue::Pct(50.0))),
        };
        match clip_path_to_shape(&clip, r) {
            Some(ResolvedClipShape::Circle { cx, cy, r: rad }) => {
                assert!((cx - 210.0).abs() < 0.01);
                assert!((cy - 310.0).abs() < 0.01);
                // sqrt((220² + 220²)/2) = 220 → 40% = 88
                assert!((rad - 88.0).abs() < 0.01, "radius {rad}");
            }
            other => panic!("expected Circle, got {other:?}"),
        }
    }

    #[test]
    fn clip_path_ellipse_explicit_center() {
        use super::clip_path_to_rect;
        use lumen_layout::{ClipPath, ShapeValue};
        let r = Rect::new(10.0, 10.0, 200.0, 100.0);
        let clip = ClipPath::Ellipse {
            rx: ShapeValue::Px(40.0),
            ry: ShapeValue::Px(20.0),
            center: Some((ShapeValue::Px(100.0), ShapeValue::Px(50.0))),
        };
        let cr = clip_path_to_rect(&clip, r);
        // cx = 10+100=110, cy = 10+50=60
        assert_eq!(cr, Rect::new(70.0, 40.0, 80.0, 40.0));
    }

    #[test]
    fn clip_path_polygon_bounding_box() {
        use super::{clip_path_to_rect, clip_path_to_shape, ResolvedClipShape};
        use lumen_layout::{ClipPath, FillRule, ShapeValue};
        let r = Rect::new(0.0, 0.0, 200.0, 200.0);
        // triangle: (100,0) (200,200) (0,200)
        let clip = ClipPath::Polygon(
            vec![
                (ShapeValue::Px(100.0), ShapeValue::Px(0.0)),
                (ShapeValue::Px(200.0), ShapeValue::Px(200.0)),
                (ShapeValue::Px(0.0), ShapeValue::Px(200.0)),
            ],
            FillRule::NonZero,
        );
        let cr = clip_path_to_rect(&clip, r);
        assert_eq!(cr, Rect::new(0.0, 0.0, 200.0, 200.0));
        // BUG-140 (TEST-109 c2): точная форма — полигон, не bbox.
        let shape = clip_path_to_shape(&clip, r);
        assert_eq!(
            shape,
            Some(ResolvedClipShape::Polygon {
                verts: vec![(100.0, 0.0), (200.0, 200.0), (0.0, 200.0)],
                even_odd: false,
            })
        );
    }

    #[test]
    fn clip_path_path_resolves_to_polygon() {
        use super::{clip_path_to_shape, ResolvedClipShape};
        use lumen_layout::{ClipPath, FillRule};
        // path() хранит уже флэттенные px-точки в системе пути; clip_path_to_shape
        // только смещает их на позицию border-box (r.x/r.y).
        let r = Rect::new(20.0, 30.0, 100.0, 100.0);
        let clip = ClipPath::Path(vec![(0.0, 0.0), (100.0, 0.0), (50.0, 80.0)], FillRule::NonZero);
        let shape = clip_path_to_shape(&clip, r);
        assert_eq!(
            shape,
            Some(ResolvedClipShape::Polygon {
                verts: vec![(20.0, 30.0), (120.0, 30.0), (70.0, 110.0)],
                even_odd: false,
            })
        );
    }

    #[test]
    fn clip_path_path_emits_push_clip_path() {
        let dl = build(
            "<div></div>",
            r#"div { width:100px; height:100px; background:red; clip-path:path("M 0 0 L 100 0 L 50 80 Z"); }"#,
        );
        let push = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::PushClipPath { .. }))
            .count();
        assert_eq!(push, 1, "clip-path:path() должен эмитить PushClipPath");
    }

    #[test]
    fn clip_path_emits_push_pop_clip() {
        // clip-path:inset(10px) on a div must emit PushClipRect/PopClip
        let dl = build(
            "<div></div>",
            "div { width:100px; height:50px; clip-path:inset(10px); background:red; }",
        );
        let push_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::PushClipRect { .. }))
            .count();
        assert!(push_count >= 1, "clip-path:inset должен эмитить PushClipRect");
        let pop_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::PopClip))
            .count();
        assert_eq!(push_count, pop_count, "Push/Pop должны быть сбалансированы");
    }

    /// BUG-140 (TEST-109 c0/c1): clip-path эмитится ВНУТРИ PushTransform —
    /// клип задан в локальной системе элемента и переносится его transform-ом.
    #[test]
    fn clip_path_emitted_inside_transform() {
        let dl = build(
            "<div></div>",
            "div { width:100px; height:50px; transform:rotate(25deg); \
             clip-path:circle(40% at 50% 50%); background:red; }",
        );
        let t_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushTransform { .. }))
            .expect("PushTransform must be emitted");
        let c_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushClipPath { .. }))
            .expect("PushClipPath must be emitted (percent circle parsed)");
        assert!(
            t_idx < c_idx,
            "PushTransform ({t_idx}) должен предшествовать PushClipPath ({c_idx})"
        );
        let pop_t = dl
            .iter()
            .rposition(|c| matches!(c, DisplayCommand::PopTransform))
            .expect("PopTransform");
        let pop_c = dl
            .iter()
            .rposition(|c| matches!(c, DisplayCommand::PopClip))
            .expect("PopClip");
        assert!(pop_c < pop_t, "PopClip ({pop_c}) должен закрыться до PopTransform ({pop_t})");
    }

    // ── emit_column_rules ──────────────────────────────────────────────────

    fn column_rule_cmds(dl: &DisplayList) -> Vec<&DisplayCommand> {
        // Column rules emitted as DrawBorder with widths=[0, rule_w, 0, 0].
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { widths: [0.0, w, 0.0, 0.0], .. } if *w > 0.0))
            .collect()
    }

    #[test]
    fn column_rule_emits_separators_between_columns() {
        // 3 columns → 2 separators.
        let dl = build(
            r#"<div style="column-count:3;column-gap:30px;
                           column-rule:4px solid red;
                           width:300px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 2, "3 columns → 2 column-rule separators, got {}", rules.len());
    }

    #[test]
    fn column_rule_none_style_emits_nothing() {
        // column-rule-style defaults to None → no separators.
        let dl = build(
            r#"<div style="column-count:2;column-gap:20px;
                           column-rule-width:4px;
                           width:200px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "column-rule-style:none should emit no separators");
    }

    #[test]
    fn column_rule_zero_width_emits_nothing() {
        let dl = build(
            r#"<div style="column-count:3;column-gap:20px;
                           column-rule:0px solid blue;
                           width:300px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "column-rule-width:0 should emit no separators");
    }

    #[test]
    fn column_rule_single_column_emits_nothing() {
        let dl = build(
            r#"<div style="column-count:1;column-gap:20px;
                           column-rule:4px solid green;
                           width:200px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "1 column → no separators");
    }

    #[test]
    fn column_rule_no_column_props_emits_nothing() {
        // No column-count or column-width → not a multicol container.
        let dl = build(
            r#"<div style="column-rule:4px solid red;width:200px;height:100px"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "no column-count/width → no separators");
    }

    #[test]
    fn column_rule_separates_overflow_columns() {
        // `column-fill:auto` + a definite height: 400px of content in 100px columns spills
        // into overflow columns past `column-count` (Multicol L1 §7.1) — every column gets a rule.
        let dl = build(
            r#"<div style="width:100px;height:100px;columns:2;column-fill:auto;column-gap:10px;
                           column-rule:10px solid gold">
                 <div style="height:400px;background:cyan"></div>
               </div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        // 4 columns of 45px (content 400px / 100px) → 3 separators, not column-count - 1 = 1.
        assert_eq!(rules.len(), 3, "got {}", rules.len());
    }

    #[test]
    fn column_rule_cap_inset_shortens_the_line() {
        // CSS Gap Decorations L1 §3.3: `column-rule-inset: 4px` trims 4px off both block ends.
        let dl = build(
            r#"<div style="width:100px;height:100px;columns:2;column-gap:10px;
                           column-rule:10px solid gold;column-rule-inset:4px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 1);
        let DisplayCommand::DrawBorder { rect, .. } = rules[0] else { panic!("DrawBorder") };
        assert!((rect.height - 92.0).abs() < 0.01 && (rect.y - 4.0).abs() < 0.01, "rect {rect:?}");
    }

    // ── CSS Multicol L2 rows of columns: column + row rules (WPT css-gaps/multicol 004/014/034) ──

    fn row_rule_cmds(dl: &DisplayList) -> Vec<(f32, f32, f32, f32)> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawBorder { rect, widths: [0.0, 0.0, h, 0.0], .. } if *h > 0.0 => {
                    Some((rect.x, rect.y, rect.width, rect.height))
                }
                _ => None,
            })
            .collect()
    }

    fn col_rule_rects(dl: &DisplayList) -> Vec<(f32, f32, f32, f32)> {
        let mut v: Vec<_> = column_rule_cmds(dl)
            .into_iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawBorder { rect, .. } => Some((rect.x, rect.y, rect.width, rect.height)),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap());
        v
    }

    const ROWS_CSS: &str = "width:200px;column-count:3;column-width:60px;column-gap:10px;row-gap:10px;        column-height:60px;column-wrap:wrap;column-fill:auto;column-rule:4px solid blue;row-rule:4px solid gold;";
    const SIX_P: &str = r#"<p style="height:60px;margin:0"></p><p style="height:60px;margin:0"></p><p style="height:60px;margin:0"></p>
                           <p style="height:60px;margin:0"></p><p style="height:60px;margin:0"></p><p style="height:60px;margin:0"></p>"#;

    #[test]
    fn multicol_rows_draw_column_rules_per_row_and_a_row_rule() {
        // multicol-gap-decorations-004: two rows → 2 column gaps × 2 rows of column rules and
        // one row rule through the 10px row gap (y 60..70, rule 4px centred at y 63).
        let dl = build(&format!(r#"<div style="{ROWS_CSS}">{SIX_P}</div>"#), "");
        let cols = col_rule_rects(&dl);
        assert_eq!(cols.len(), 4, "{cols:?}");
        assert_eq!((cols[0].0, cols[0].1, cols[0].3), (63.0, 0.0, 60.0));
        assert_eq!((cols[2].0, cols[2].1, cols[2].3), (63.0, 70.0, 60.0));
        let rows = row_rule_cmds(&dl);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].0, rows[0].1, rows[0].2, rows[0].3), (0.0, 63.0, 200.0, 4.0));
    }

    #[test]
    fn multicol_rows_hide_rules_next_to_empty_cells() {
        // Four items: row 2 holds one. `normal` column visibility is `between` — no column rule
        // in row 2; the row rule stays whole (`normal` = `all` for rows).
        let html = r#"<p style="height:60px;margin:0"></p><p style="height:60px;margin:0"></p><p style="height:60px;margin:0"></p>
                      <p style="height:60px;margin:0"></p>"#;
        let dl = build(&format!(r#"<div style="{ROWS_CSS}">{html}</div>"#), "");
        assert_eq!(col_rule_rects(&dl).len(), 2, "only row 1 has two filled neighbours");
        assert_eq!(row_rule_cmds(&dl).len(), 1);
    }

    #[test]
    fn multicol_rows_row_rule_break_intersection_cuts_at_column_gaps() {
        let dl = build(&format!(r#"<div style="{ROWS_CSS}row-rule-break:intersection">{SIX_P}</div>"#), "");
        let rows = row_rule_cmds(&dl);
        assert_eq!(rows.len(), 3, "one piece per column: {rows:?}");
        assert_eq!((rows[0].0, rows[0].2), (0.0, 60.0));
    }

    #[test]
    fn multicol_without_column_height_ignores_column_wrap() {
        let dl = build(
            r#"<div style="width:100px;height:100px;columns:2;column-gap:10px;column-wrap:wrap;
                           column-rule:10px solid gold;background:white"></div>"#,
            "",
        );
        assert_eq!(column_rule_cmds(&dl).len(), 1);
        assert!(row_rule_cmds(&dl).is_empty());
    }

    // ── position:sticky display list tests ──────────────────────────────────

    #[test]
    fn sticky_top_emits_begin_end_layer() {
        let dl = build(
            r#"<div style="position:sticky;top:10px;background:blue;width:200px;height:50px"></div>"#,
            "",
        );
        let has_begin = dl.iter().any(|c| matches!(c, DisplayCommand::BeginStickyLayer { top: Some(t), .. } if (*t - 10.0).abs() < 0.01));
        let has_end = dl.iter().any(|c| matches!(c, DisplayCommand::EndStickyLayer));
        assert!(has_begin, "expected BeginStickyLayer with top=10 in display list");
        assert!(has_end, "expected EndStickyLayer in display list");
    }

    #[test]
    fn sticky_begin_before_fill_rect() {
        let dl = build(
            r#"<div style="position:sticky;top:0px;background:red;width:100px;height:40px"></div>"#,
            "",
        );
        let begin_idx = dl.iter().position(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. })).unwrap();
        let fill_idx = dl.iter().position(|c| matches!(c, DisplayCommand::FillRect { .. })).unwrap();
        let end_idx = dl.iter().position(|c| matches!(c, DisplayCommand::EndStickyLayer)).unwrap();
        assert!(begin_idx < fill_idx, "BeginStickyLayer must come before FillRect");
        assert!(fill_idx < end_idx, "FillRect must come before EndStickyLayer");
    }

    #[test]
    fn sticky_auto_top_no_layer() {
        // position:sticky with no insets (all auto) — still emits layer (spec allows sticky
        // with auto insets; it behaves like static but is logically sticky-positioned).
        let dl = build(
            r#"<div style="position:sticky;background:green;width:100px;height:40px"></div>"#,
            "",
        );
        let has_begin = dl.iter().any(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. }));
        // With all-auto insets the layer is still emitted (no inset = no clamping in renderer).
        assert!(has_begin, "BeginStickyLayer emitted even for all-auto sticky");
    }

    #[test]
    fn sticky_bottom_inset_stored() {
        let dl = build(
            r#"<div style="position:sticky;bottom:20px;background:blue;width:200px;height:50px"></div>"#,
            "",
        );
        let has_bottom = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::BeginStickyLayer { bottom: Some(b), .. } if (*b - 20.0).abs() < 0.01
        ));
        assert!(has_bottom, "expected BeginStickyLayer with bottom=20");
    }

    #[test]
    fn non_sticky_no_layer() {
        // position:relative does not produce a sticky layer.
        let dl = build(
            r#"<div style="position:relative;top:10px;background:blue;width:200px;height:50px"></div>"#,
            "",
        );
        let has_begin = dl.iter().any(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. }));
        assert!(!has_begin, "position:relative must not emit BeginStickyLayer");
    }

    #[test]
    fn fixed_emits_begin_end_fixed_layer_bracketing_fill() {
        // ADR-016 M3.2.1c-2: position:fixed brackets its box with a payload-free
        // BeginFixedLayer/EndFixedLayer pair (no draw-time offset — pure metadata).
        let dl = build(
            r#"<div style="position:fixed;top:0;left:0;background:blue;width:100px;height:40px"></div>"#,
            "",
        );
        let begin_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::BeginFixedLayer))
            .expect("expected BeginFixedLayer in display list");
        let fill_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .expect("expected the fixed box FillRect");
        let end_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::EndFixedLayer))
            .expect("expected EndFixedLayer in display list");
        assert!(begin_idx < fill_idx, "BeginFixedLayer must come before FillRect");
        assert!(fill_idx < end_idx, "FillRect must come before EndFixedLayer");
    }

    #[test]
    fn non_fixed_no_fixed_layer() {
        // Only position:fixed emits the fixed-layer bracket; sticky uses its own pair.
        let dl = build(
            r#"<div style="position:sticky;top:10px;background:blue;width:200px;height:50px"></div>"#,
            "",
        );
        let has_fixed = dl.iter().any(|c| matches!(c, DisplayCommand::BeginFixedLayer));
        assert!(!has_fixed, "position:sticky must not emit BeginFixedLayer");
    }

    #[test]
    fn column_rule_separator_centered_in_gap() {
        // 2 columns, 40px gap, 4px rule → rule centered at gap_left + (40-4)/2 = gap_left + 18.
        let dl = build(
            r#"<div style="column-count:2;column-gap:40px;
                           column-rule:4px solid red;
                           width:280px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 1, "2 columns → 1 separator");
        if let DisplayCommand::DrawBorder { rect, widths: [_, rule_w, _, _], .. } = rules[0] {
            // col_w = (280 - 40) / 2 = 120px; gap_left = 120; sep_x = 120 + 18 = 138.
            assert!((rect.x - 138.0).abs() < 0.5, "sep_x expected ~138, got {}", rect.x);
            assert!((*rule_w - 4.0).abs() < 0.01, "rule width expected 4, got {}", rule_w);
        }
    }

    // ── CSS Gap Decorations L1: column-rule / row-rule in flex/grid ────────

    fn horizontal_rule_cmds(dl: &DisplayList) -> Vec<&DisplayCommand> {
        // Row rules are emitted as DrawBorder with widths=[0, 0, rule_h, 0].
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { widths: [0.0, 0.0, h, 0.0], .. } if *h > 0.0))
            .collect()
    }

    const GRID_2X2: &str = r#"<div style="display:grid;grid-template-columns:100px 100px;
        grid-template-rows:50px 50px;gap:20px;width:220px;height:120px;{}">
        <div></div><div></div><div></div><div></div></div>"#;

    #[test]
    fn grid_row_rule_emits_one_horizontal_segment() {
        let html = GRID_2X2.replace("{}", "row-rule:2px solid red");
        let dl = build(&html, "");
        assert_eq!(horizontal_rule_cmds(&dl).len(), 1, "grid 2x2 → one row gap");
        assert_eq!(column_rule_cmds(&dl).len(), 0, "row-rule alone must not draw vertical rules");
    }

    #[test]
    fn grid_column_rule_emits_one_vertical_segment() {
        let html = GRID_2X2.replace("{}", "column-rule:2px solid red");
        let dl = build(&html, "");
        assert_eq!(column_rule_cmds(&dl).len(), 1, "grid 2x2 → one column gap");
        assert_eq!(horizontal_rule_cmds(&dl).len(), 0, "column-rule alone must not draw horizontal rules");
    }

    #[test]
    fn grid_rule_shorthand_emits_both_axes() {
        let html = GRID_2X2.replace("{}", "rule:2px solid red");
        let dl = build(&html, "");
        assert_eq!(column_rule_cmds(&dl).len(), 1);
        assert_eq!(horizontal_rule_cmds(&dl).len(), 1);
    }

    #[test]
    fn grid_axes_use_independent_styles() {
        let html = GRID_2X2.replace("{}", "column-rule:4px solid red;row-rule:2px dashed blue");
        let dl = build(&html, "");
        let cols = column_rule_cmds(&dl);
        let rows = horizontal_rule_cmds(&dl);
        assert_eq!((cols.len(), rows.len()), (1, 1));
        if let DisplayCommand::DrawBorder { widths: [_, w, _, _], styles, .. } = cols[0] {
            assert!((*w - 4.0).abs() < 0.01);
            assert_eq!(styles[1], BorderStyle::Solid);
        }
        if let DisplayCommand::DrawBorder { widths: [_, _, h, _], styles, .. } = rows[0] {
            assert!((*h - 2.0).abs() < 0.01);
            assert_eq!(styles[2], BorderStyle::Dashed);
        }
    }

    #[test]
    fn rule_overlap_decides_which_axis_paints_on_top() {
        fn axis_order(overlap: &str) -> Vec<bool> {
            let html = GRID_2X2.replace("{}", &format!("rule:2px solid red;rule-overlap:{overlap}"));
            let dl = build(&html, "");
            // `true` = a vertical (column) rule, `false` = a horizontal (row) rule.
            dl.iter()
                .filter_map(|c| match c {
                    DisplayCommand::DrawBorder { widths: [0.0, w, 0.0, 0.0], .. } if *w > 0.0 => Some(true),
                    DisplayCommand::DrawBorder { widths: [0.0, 0.0, h, 0.0], .. } if *h > 0.0 => Some(false),
                    _ => None,
                })
                .collect()
        }
        assert_eq!(axis_order("row-over-column"), vec![true, false], "rows painted last");
        assert_eq!(axis_order("column-over-row"), vec![false, true], "columns painted last");
    }

    #[test]
    fn rule_inset_shortens_gap_segments_at_container_edges() {
        // GRID_2X2 content box: 220×120 at x=0; column gap spans the full height,
        // the row gap the full width.
        let html = GRID_2X2.replace("{}", "rule:2px solid red;column-rule-inset:10px 20px;row-rule-inset:30px 5px");
        let dl = build(&html, "");
        let cols = column_rule_cmds(&dl);
        let rows = horizontal_rule_cmds(&dl);
        assert_eq!((cols.len(), rows.len()), (1, 1));
        let (DisplayCommand::DrawBorder { rect: c, .. }, DisplayCommand::DrawBorder { rect: r, .. }) = (cols[0], rows[0])
        else {
            panic!("expected DrawBorder");
        };
        assert!((c.height - 90.0).abs() < 0.6, "120 - 10 - 20, got {}", c.height);
        assert!((r.width - 185.0).abs() < 0.6, "220 - 30 - 5, got {}", r.width);
        // Percentages and `overlap-join` resolve to 0 at a container edge.
        let html = GRID_2X2.replace("{}", "rule:2px solid red;rule-inset:50%");
        let dl = build(&html, "");
        let DisplayCommand::DrawBorder { rect, .. } = column_rule_cmds(&dl)[0] else { panic!("expected DrawBorder") };
        assert!((rect.height - 120.0).abs() < 0.6, "got {}", rect.height);
    }

    /// CSS Gap Decorations L1 §4.6: a list of values is dealt to the gaps in order.
    #[test]
    fn rule_value_lists_are_assigned_per_gap() {
        let html = r#"<div style="display:grid;grid-template-columns:50px 50px 50px;gap:10px;
            width:200px;height:20px;column-rule:{}"><div></div><div></div><div></div></div>"#;
        let widths = |rule: &str| -> Vec<f32> {
            let dl = build(&html.replace("{}", rule), "");
            column_rule_cmds(&dl)
                .iter()
                .map(|c| match c {
                    DisplayCommand::DrawBorder { widths: [_, w, _, _], .. } => *w,
                    _ => unreachable!(),
                })
                .collect()
        };
        assert_eq!(widths("2px solid red, 4px solid red"), [2.0, 4.0]);
        assert_eq!(widths("2px solid red, repeat(auto, 4px solid red, 6px solid red)"), [2.0, 4.0]);
        // A `none` entry skips its gap but still consumes a value.
        assert_eq!(widths("2px none, 6px solid red"), [6.0]);
        assert_eq!(widths("repeat(2, 3px solid blue)"), [3.0, 3.0]);
    }

    #[test]
    fn rule_inset_collapsing_a_segment_removes_it() {
        let html = GRID_2X2.replace("{}", "rule:2px solid red;rule-inset:110px");
        let dl = build(&html, "");
        assert_eq!(column_rule_cmds(&dl).len() + horizontal_rule_cmds(&dl).len(), 0);
    }

    /// 3×3 grid, дорожки 50px, щели 20px (190×190). `{style}` — свойства контейнера,
    /// `{items}` — дети.
    const GRID_3X3: &str = r#"<div style="display:grid;grid-template-columns:50px 50px 50px;        grid-template-rows:50px 50px 50px;gap:20px;width:190px;height:190px;{style}">{items}</div>"#;

    fn grid3(style: &str, items: &str) -> String {
        GRID_3X3.replace("{style}", style).replace("{items}", items)
    }

    /// Вертикальные куски колоночных правил: `(x, y, height)`, по x и y.
    fn column_pieces(dl: &DisplayList) -> Vec<(i32, i32, i32)> {
        let mut v: Vec<_> = column_rule_cmds(dl)
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawBorder { rect, .. } => {
                    Some((rect.x.round() as i32, rect.y.round() as i32, rect.height.round() as i32))
                }
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    const CELL: &str = "<div></div>";

    /// §3.2: элемент, пересекающий щель, прерывает её; `normal` в grid рвёт на «Т», `none` — нет.
    #[test]
    fn grid_rule_break_cuts_gap_at_spanning_item() {
        // Элемент на колонки 1–2 в строке 2 закрывает колоночную щель 0 в этой строке.
        let items = "<div style=\"grid-area:1/1\"></div><div style=\"grid-area:1/2\"></div><div style=\"grid-area:1/3\"></div>                     <div style=\"grid-area:2/1/3/3\"></div><div style=\"grid-area:2/3\"></div>                     <div style=\"grid-area:3/1\"></div><div style=\"grid-area:3/2\"></div><div style=\"grid-area:3/3\"></div>";
        let dl = build(&grid3("column-rule:2px solid red", items), "");
        let cols = column_pieces(&dl);
        // Щель 0 (x=50..70) закрыта спаннером в строке 1 (y=70..120): остаются куски строк 0 и 2.
        // Щель 1 (x=120..140) идёт сквозь всю высоту.
        assert!(cols.iter().filter(|c| c.0 < 100).count() == 2, "{cols:?}");
        assert!(cols.iter().any(|c| c.0 < 100 && c.1 == 0 && c.2 == 50), "{cols:?}");
        assert!(cols.iter().any(|c| c.0 < 100 && c.1 == 140 && c.2 == 50), "{cols:?}");
        assert!(cols.iter().any(|c| c.0 > 100 && c.2 == 190), "{cols:?}");
        // `none` — линия идёт позади элемента.
        let dl = build(&grid3("column-rule:2px solid red;column-rule-break:none", items), "");
        assert_eq!(column_pieces(&dl).len(), 2, "none: две сплошные линии");
    }

    /// §3.4: `between` скрывает кусок щели рядом с пустой клеткой, `all` — нет.
    #[test]
    fn grid_rule_visibility_items_hides_next_to_empty_cells() {
        // Пустая клетка: строка 2, колонка 3.
        let items = "<div style=\"grid-area:1/1\"></div><div style=\"grid-area:1/2\"></div><div style=\"grid-area:1/3\"></div>                     <div style=\"grid-area:2/1\"></div><div style=\"grid-area:2/2\"></div>                     <div style=\"grid-area:3/1\"></div><div style=\"grid-area:3/2\"></div><div style=\"grid-area:3/3\"></div>";
        let all = build(&grid3("column-rule:2px solid red;column-rule-break:none", items), "");
        assert_eq!(column_pieces(&all).len(), 2);
        let between = build(
            &grid3("column-rule:2px solid red;column-rule-break:none;column-rule-visibility-items:between", items),
            "",
        );
        // Щель 1 (колонки 2|3) в строке 2 касается пустой клетки → линия рвётся там.
        let cols = column_pieces(&between);
        assert_eq!(cols.iter().filter(|c| c.0 > 100).count(), 2, "{cols:?}");
        assert_eq!(cols.iter().filter(|c| c.0 < 100).count(), 1, "{cols:?}");
        let around = build(
            &grid3("column-rule:2px solid red;column-rule-break:none;column-rule-visibility-items:around", items),
            "",
        );
        assert_eq!(column_pieces(&around).len(), 2, "around: клетка слева занята");
    }

    /// §3.3: `junction`-отступ сдвигает концы кусков у стыка, `cap` — у края контейнера.
    #[test]
    fn grid_rule_inset_junction_applies_at_cuts() {
        let items = format!("{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}");
        let dl = build(
            &grid3("column-rule:2px solid red;column-rule-break:intersection;column-rule-inset:0 / 5px", &items),
            "",
        );
        let cols = column_pieces(&dl);
        // Щель 0: 0..50 (cap-начало 0, junction-конец 5 → 45), 70..120 (5 с обеих сторон → 40), 140..190 (→ 45).
        let mut g0: Vec<_> = cols.iter().filter(|c| c.0 < 100).map(|c| (c.1, c.2)).collect();
        g0.sort();
        assert_eq!(g0, vec![(0, 45), (75, 40), (145, 45)], "{cols:?}");
    }

    /// `overlap-join` заходит в стык на полширины щели плюс полширины линии.
    #[test]
    fn grid_rule_inset_overlap_join_extends_into_junction() {
        let items = format!("{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}{CELL}");
        let dl = build(
            &grid3(
                "rule:4px solid red;column-rule-break:intersection;column-rule-inset:0 / overlap-join",
                &items,
            ),
            "",
        );
        let cols = column_pieces(&dl);
        // Внутренний кусок щели 0 (строка 1): 70..120 → продлён на 10 + 2 = 12px с каждой стороны.
        assert!(cols.iter().any(|c| c.0 < 100 && c.1 == 58 && c.2 == 74), "{cols:?}");
    }

    /// The ordered (stacking-context) path — the one the live window and
    /// `--screenshot` use — must draw gap rules too, not only `walk`.
    #[test]
    fn grid_rules_drawn_by_ordered_path() {
        let html = GRID_2X2.replace("{}", "rule:2px solid red").replace("<div></div>", "<div style=\"background:blue\"></div>");
        let dl = super::ordered_build_scroll::build_ordered(&html, "");
        assert_eq!(column_rule_cmds(&dl).len(), 1, "ordered path: one column rule");
        assert_eq!(horizontal_rule_cmds(&dl).len(), 1, "ordered path: one row rule");
        // Rules sit inside the overflow clip, after the children's fills.
        let last_fill = dl.iter().rposition(|c| matches!(c, DisplayCommand::FillRect { .. })).unwrap();
        let first_rule = dl.iter().position(|c| matches!(c, DisplayCommand::DrawBorder { .. })).unwrap();
        assert!(first_rule > last_fill, "gap rules must paint over the children");
    }

    // ── CSS Lists L3 §2.1 — list marker geometric rendering ─────────────────

    /// disc marker emits FillRoundedRect (filled circle), not DrawText.
    #[test]
    fn disc_marker_emits_filled_rounded_rect() {
        let dl = build(
            r#"<ul style="padding-left:32px"><li style="color:red">A</li></ul>"#,
            "",
        );
        let circles: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::FillRoundedRect { radii, .. } => Some(radii),
            _ => None,
        }).collect();
        assert!(!circles.is_empty(), "disc marker must emit FillRoundedRect");
        // All radii equal (it's a circle): tl == tl_y == tr == tr_y == ...
        let r = circles[0];
        assert!((r.tl - r.tl_y).abs() < 0.01, "disc radii should be equal (circle)");
        assert!((r.tl - r.tr).abs() < 0.01, "disc radii should be equal (circle)");
    }

    /// disc marker renders no Unicode bullet text.
    #[test]
    fn disc_marker_no_bullet_text() {
        let dl = build(
            r#"<ul style="padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        let bullet_texts: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawText { text, .. } if text.contains('\u{2022}') => Some(text.as_str()),
            _ => None,
        }).collect();
        assert!(bullet_texts.is_empty(), "disc should not render Unicode bullet •");
    }

    /// circle marker emits DrawBorder (hollow circle outline), not DrawText.
    #[test]
    fn circle_marker_emits_draw_border() {
        let dl = build(
            r#"<ul style="list-style-type:circle;padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        let borders: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawBorder { radii, .. } if radii.tl > 0.0 => Some(radii),
            _ => None,
        }).collect();
        assert!(!borders.is_empty(), "circle marker must emit DrawBorder with rounded corners");
    }

    /// square marker emits FillRect (filled square), not DrawText.
    #[test]
    fn square_marker_emits_fill_rect() {
        let dl = build(
            r#"<ul style="list-style-type:square;padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        // FillRect count: one for the square marker (li has no background by default)
        // We just check at least one FillRect exists from the square marker.
        let rects: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRect { .. })).collect();
        assert!(!rects.is_empty(), "square marker must emit FillRect");
    }

    /// decimal (ordered) marker renders as DrawText with counter string.
    /// Note: Lumen has no UA stylesheet, so list-style-type must be set explicitly.
    #[test]
    fn decimal_marker_emits_draw_text() {
        let dl = build(
            r#"<ol style="list-style-type:decimal;padding-left:32px"><li>A</li><li>B</li></ol>"#,
            "",
        );
        let counter_texts: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawText { text, .. } if text.starts_with("1.") || text.starts_with("2.") => Some(text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(counter_texts.len(), 2, "2 decimal markers should produce 2 DrawText commands");
    }

    /// list-style-type:none produces no marker output.
    #[test]
    fn list_style_none_no_marker() {
        let dl = build(
            r#"<ul style="list-style-type:none;padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        // No FillRoundedRect from markers (li has no background), no DrawBorder with positive radii from markers.
        let circles: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(circles.is_empty(), "list-style-type:none should not emit any marker shape");
    }

    /// lower-alpha marker renders letter counter text (explicit list-style-type — no UA stylesheet).
    #[test]
    fn lower_alpha_marker_emits_text() {
        let dl = build(
            r#"<ul style="list-style-type:lower-alpha;padding-left:32px"><li>A</li><li>B</li></ul>"#,
            "",
        );
        let alpha_texts: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawText { text, .. } if text.starts_with("a.") || text.starts_with("b.") => Some(text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(alpha_texts.len(), 2, "lower-alpha markers: expected 'a. ' and 'b. '");
    }

    /// BUG-185: `::marker { content: "→ " }` on a `list-style-type: disc` list must
    /// paint the override string, not the disc bullet glyph. The marker carries both
    /// `list_style_type: Disc` and the content text; the text wins.
    #[test]
    fn marker_content_override_renders_text_not_bullet() {
        let dl = build(
            r#"<ul class="cm" style="list-style-type:disc;padding-left:32px"><li>Arrow A</li></ul>"#,
            r#".cm li::marker { content: "→ "; color: #68d391; }"#,
        );
        // The disc bullet must be suppressed: no FillRoundedRect from the marker.
        let discs = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. }))
            .count();
        assert_eq!(discs, 0,
            "content override must suppress the disc bullet, got {discs} FillRoundedRect");
        // The arrow string is painted instead.
        let arrows = dl.iter().any(|c| matches!(c,
            DisplayCommand::DrawText { text, .. } if text.contains('\u{2192}')));
        assert!(arrows, "content override must paint the arrow text");
    }

    // ── CSS Compositing L1 §8.3 — background-blend-mode ──

    /// Normal blend mode → no PushBlendMode/PopBlendMode emitted.
    #[test]
    fn background_blend_mode_normal_no_blend_commands() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue);background-blend-mode:normal;width:100px;height:100px"></div>"#,
            "",
        );
        let blend_cmds: Vec<_> = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::PushBlendMode { .. } | DisplayCommand::PopBlendMode)
        }).collect();
        assert!(blend_cmds.is_empty(), "normal blend mode must not emit any blend commands");
    }

    /// Single layer with non-normal blend mode: it is the bottom-most layer, so
    /// CSS Compositing L1 §8.3 says it blends against transparent background-color.
    /// For premultiplied alpha, multiply(src, transparent) = src — no visual effect.
    /// We suppress PushBlendMode to avoid incorrect blending against the stacking context.
    #[test]
    fn background_blend_mode_single_layer_bottom_suppressed() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue);background-blend-mode:multiply;width:100px;height:100px"></div>"#,
            "",
        );
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { .. })).count();
        let idx_grad = dl.iter().position(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }));
        assert_eq!(push_count, 0, "single bottom layer: blend suppressed (identity against transparent)");
        assert!(idx_grad.is_some(), "DrawLinearGradient still emitted");
    }

    /// Two layers: first has multiply, second normal → one blend pair for first layer only.
    #[test]
    fn background_blend_mode_two_layers_only_first_blended() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue),linear-gradient(green,yellow);background-blend-mode:multiply,normal;width:100px;height:100px"></div>"#,
            "",
        );
        // Exactly one PushBlendMode and one PopBlendMode total.
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { .. })).count();
        let pop_count  = dl.iter().filter(|c| matches!(c, DisplayCommand::PopBlendMode)).count();
        assert_eq!(push_count, 1, "only one layer with non-normal blend mode → one PushBlendMode");
        assert_eq!(pop_count,  1, "matching PopBlendMode count");
    }

    /// Two layers with same blend mode: bottom suppressed, top blended.
    /// This is the most common pattern in background-blend-mode CSS.
    #[test]
    fn background_blend_mode_two_same_mode_only_top_blended() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue),linear-gradient(green,yellow);background-blend-mode:multiply;width:100px;height:100px"></div>"#,
            "",
        );
        // Bottom layer suppressed, top layer wrapped → exactly 1 PushBlendMode.
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { .. })).count();
        let pop_count  = dl.iter().filter(|c| matches!(c, DisplayCommand::PopBlendMode)).count();
        assert_eq!(push_count, 1, "two layers same blend: bottom suppressed, top wrapped → 1 PushBlendMode");
        assert_eq!(pop_count,  1, "matching PopBlendMode");
        // Verify order: bottom gradient → PushBlendMode → top gradient → PopBlendMode
        let positions: Vec<usize> = dl.iter().enumerate().filter_map(|(i, c)| {
            if matches!(c, DisplayCommand::DrawLinearGradient { .. } | DisplayCommand::PushBlendMode { .. } | DisplayCommand::PopBlendMode) {
                Some(i)
            } else { None }
        }).collect();
        assert!(positions.len() == 4, "expecting: grad(bottom), PushBlend, grad(top), PopBlend");
        assert!(matches!(&dl[positions[0]], DisplayCommand::DrawLinearGradient { .. }), "first: bottom gradient");
        assert!(matches!(&dl[positions[1]], DisplayCommand::PushBlendMode { .. }), "second: PushBlendMode");
        assert!(matches!(&dl[positions[2]], DisplayCommand::DrawLinearGradient { .. }), "third: top gradient");
        assert!(matches!(&dl[positions[3]], DisplayCommand::PopBlendMode), "fourth: PopBlendMode");
    }

    /// background-blend-mode cycles when fewer values than layers.
    /// Bottom layer blend is suppressed (CSS Compositing L1 §8.3 isolated group).
    #[test]
    fn background_blend_mode_cycling() {
        // 3 layers, 1 value → all three have multiply, but bottom-most is suppressed.
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue),linear-gradient(green,yellow),linear-gradient(cyan,magenta);background-blend-mode:multiply;width:100px;height:100px"></div>"#,
            "",
        );
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { mode: BlendMode::Multiply, .. })).count();
        assert_eq!(push_count, 2, "cycling: 3 layers but bottom-most suppressed → 2 PushBlendMode");
    }

