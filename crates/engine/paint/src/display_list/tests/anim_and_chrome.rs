//! P1/SPLIT-DL3: хвост тестового модуля `mod tests` в `display_list.rs` —
//! static/animated split (EXPERIMENT.md §2)/text-emphasis/clip-path/
//! column-rules/position:sticky и position:fixed/list marker rendering/
//! background-blend-mode/BoxModelOverlay/MaskMode + PushMaskLayer/
//! PushScrollLayer/DrawScrollbar/PageBreak и print display list/
//! strip_background_graphics/DrawCrossFade. Перенесено байт-в-байт из
//! `display_list.rs` без дедента (приём ST-1/DL-1).
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-3).

use super::*;
// P1/SPLIT-DL2: hash_corpus/red_fill/debug_hash_one живут в
// display_list/tests/svg_table_and_hash.rs (батч DL-2), но их зовут тесты
// этого файла (батч DL-3).
use super::svg_table_and_hash::{debug_hash_one, hash_corpus, red_fill};
// build/build_ordered остаются в `mod tests` в display_list.rs — общие
// хелперы, ещё не вынесенные оттуда.
use super::tests::{build, build_ordered};
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

    // ── BoxModelOverlay ──────────────────────────────────────────────────────

    #[test]
    fn box_model_overlay_serializes_all_four_boxes() {
        use lumen_core::geom::Rect;
        let dl = vec![DisplayCommand::BoxModelOverlay {
            margin:  Rect::new(0.0,   0.0,  120.0, 100.0),
            border:  Rect::new(10.0, 10.0,  100.0,  80.0),
            padding: Rect::new(12.0, 12.0,   96.0,  76.0),
            content: Rect::new(20.0, 20.0,   80.0,  60.0),
        }];
        let s = serialize_display_list(&dl);
        assert!(s.starts_with("BoxModelOverlay"), "must start with command name");
        assert!(s.contains("margin=(0,0,120,100)"),  "margin box");
        assert!(s.contains("border=(10,10,100,80)"), "border box");
        assert!(s.contains("padding=(12,12,96,76)"), "padding box");
        assert!(s.contains("content=(20,20,80,60)"), "content box");
    }

    #[test]
    fn box_model_overlay_zero_content_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![DisplayCommand::BoxModelOverlay {
            margin:  Rect::new(0.0, 0.0, 50.0, 50.0),
            border:  Rect::new(5.0, 5.0, 40.0, 40.0),
            padding: Rect::new(7.0, 7.0, 36.0, 36.0),
            content: Rect::new(10.0, 10.0, 0.0, 0.0), // collapsed content
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("BoxModelOverlay"), "collapsed content must still serialize");
        assert!(s.contains("content=(10,10,0,0)"), "zero-size content rect");
    }

    // ── MaskMode + PushMaskLayer / PopMaskLayer ──────────────────────────────

    #[test]
    fn mask_mode_default_is_alpha() {
        assert_eq!(MaskMode::default(), MaskMode::Alpha);
    }

    #[test]
    fn push_mask_layer_alpha_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushMaskLayer {
                rect: Rect::new(10.0, 20.0, 100.0, 80.0),
                mode: MaskMode::Alpha,
            },
            DisplayCommand::PopMaskLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("PushMaskLayer"), "must contain PushMaskLayer");
        assert!(s.contains("(10.00, 20.00, 100.00, 80.00)"), "rect coords");
        assert!(s.contains("Alpha"), "mode=Alpha");
        assert!(s.contains("PopMaskLayer"), "must contain PopMaskLayer");
    }

    #[test]
    fn push_mask_layer_luminance_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushMaskLayer {
                rect: Rect::new(0.0, 0.0, 200.0, 150.0),
                mode: MaskMode::Luminance,
            },
            DisplayCommand::PopMaskLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("Luminance"), "mode=Luminance");
    }

    #[test]
    fn push_mask_layer_roundtrip_kinds() {
        use lumen_core::geom::Rect;
        let rect = Rect::new(0.0, 0.0, 50.0, 50.0);
        let dl = vec![
            DisplayCommand::PushMaskLayer { rect, mode: MaskMode::Alpha },
            DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } },
            DisplayCommand::PopMaskLayer,
        ];
        // Verify the three-command sequence serializes in order.
        let s = serialize_display_list(&dl);
        let push_pos = s.find("PushMaskLayer").expect("no PushMaskLayer");
        let fill_pos = s.find("FillRect").expect("no FillRect");
        let pop_pos  = s.find("PopMaskLayer").expect("no PopMaskLayer");
        assert!(push_pos < fill_pos, "PushMaskLayer before FillRect");
        assert!(fill_pos < pop_pos,  "FillRect before PopMaskLayer");
    }

    #[test]
    fn mask_mode_luminance_end_to_end_bakes_stops() {
        let css = ".m { width:200px; height:200px; background:#e63946; \
             mask-image: linear-gradient(to right, black, white); mask-mode: luminance; }";
        let html = "<div class=\"m\"></div>";
        // Plain builder.
        let dl = build(html, css);
        assert_baked_luma_stops(&dl, "build_display_list");

        // Stacking-context-ordered builder (used by the CPU snapshot path) —
        // mask-image makes the box a stacking context, so it goes through the
        // bucket path, not `walk`.
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let st = StackingTree::build(&tree);
        let order = PaintOrder::from_tree(&st);
        let dl_ordered = build_display_list_ordered(&tree, &st, &order).0;
        assert_baked_luma_stops(&dl_ordered, "build_display_list_ordered");
    }

    fn assert_baked_luma_stops(dl: &DisplayList, label: &str) {
        let stops = dl.iter().find_map(|c| match c {
            DisplayCommand::PushMaskLinearGradient { stops, .. } => Some(stops.clone()),
            _ => None,
        });
        let stops = stops.unwrap_or_else(|| panic!("{label}: no PushMaskLinearGradient"));
        assert_eq!(stops.first().map(|s| s.color.a), Some(0), "{label}: black → alpha 0");
        assert_eq!(stops.last().map(|s| s.color.a), Some(255), "{label}: white → alpha 255");
    }

    #[test]
    fn mask_stops_alpha_mode_unchanged() {
        let stops = vec![
            GradientStop {
                color: Color { r: 0, g: 0, b: 0, a: 255 },
                position: None,
                ..Default::default()
            },
            GradientStop {
                color: Color { r: 255, g: 255, b: 255, a: 255 },
                position: None,
                ..Default::default()
            },
        ];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Alpha);
        assert_eq!(out, stops, "alpha mode leaves stops untouched");
    }

    #[test]
    fn mask_stops_luminance_bakes_alpha() {
        // Black opaque → luma 0 → alpha 0; white opaque → luma 1 → alpha 255.
        let stops = vec![
            GradientStop {
                color: Color { r: 0, g: 0, b: 0, a: 255 },
                position: None,
                ..Default::default()
            },
            GradientStop {
                color: Color { r: 255, g: 255, b: 255, a: 255 },
                position: None,
                ..Default::default()
            },
        ];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Luminance);
        assert_eq!(out[0].color.a, 0, "black stop becomes fully transparent");
        assert_eq!(out[1].color.a, 255, "white stop stays fully opaque");
        // RGB is preserved (only the alpha channel encodes the mask value).
        assert_eq!(out[0].color.r, 0);
        assert_eq!(out[1].color.r, 255);
    }

    #[test]
    fn mask_stops_luminance_multiplies_source_alpha() {
        // White at 50% alpha → luma 1 · 0.5 ≈ alpha 128.
        let stops = vec![GradientStop {
            color: Color { r: 255, g: 255, b: 255, a: 128 },
            position: None,
            ..Default::default()
        }];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Luminance);
        assert_eq!(out[0].color.a, 128, "luminance 1.0 keeps source alpha");
    }

    #[test]
    fn mask_stops_luminance_green_weight() {
        // Pure green opaque → luma 0.7152 → alpha ≈ 182.
        let stops = vec![GradientStop {
            color: Color { r: 0, g: 255, b: 0, a: 255 },
            position: None,
            ..Default::default()
        }];
        let out = mask_stops_for_mode(&stops, lumen_layout::MaskMode::Luminance);
        assert_eq!(out[0].color.a, 182, "0.7152·255 rounds to 182");
    }

    // ─── PushScrollLayer / PopScrollLayer tests ──────────────────────────────

    #[test]
    fn overflow_scroll_emits_push_scroll_layer() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_push = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        let has_pop  = dl.iter().any(|c| matches!(c, DisplayCommand::PopScrollLayer));
        assert!(has_push, "overflow:scroll must emit PushScrollLayer");
        assert!(has_pop,  "overflow:scroll must emit PopScrollLayer");
    }

    #[test]
    fn overflow_scroll_no_push_clip_rect_for_scroll() {
        // overflow:scroll should not fall back to PushClipRect for the scroll axis
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        // There should be PushScrollLayer, not PushClipRect, for the scroll container itself.
        let scroll_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushScrollLayer { .. })).count();
        assert!(scroll_count >= 1, "expected at least one PushScrollLayer for overflow:scroll");
    }

    #[test]
    fn overflow_hidden_emits_push_clip_rect_not_scroll_layer() {
        let dl = build(
            r#"<div style="overflow:hidden;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_scroll = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        assert!(!has_scroll, "overflow:hidden must not emit PushScrollLayer");
        // overflow:hidden still clips via PushClipRect
        let has_clip = dl.iter().any(|c| matches!(c, DisplayCommand::PushClipRect { .. }));
        assert!(has_clip, "overflow:hidden must emit PushClipRect");
    }

    #[test]
    fn scroll_layer_scroll_xy_defaults_zero() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>x</p></div>"#,
            "",
        );
        if let Some(DisplayCommand::PushScrollLayer { scroll_x, scroll_y, .. }) =
            dl.iter().find(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }))
        {
            assert_eq!(*scroll_x, 0.0, "initial scroll_x should be 0");
            assert_eq!(*scroll_y, 0.0, "initial scroll_y should be 0");
        } else {
            panic!("PushScrollLayer not found");
        }
    }

    #[test]
    fn push_scroll_layer_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushScrollLayer {
                clip_rect: Rect::new(10.0, 20.0, 100.0, 50.0),
                scroll_x: 5.0,
                scroll_y: 15.0,
            },
            DisplayCommand::PopScrollLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("PushScrollLayer"), "serialized output must contain PushScrollLayer");
        assert!(s.contains("PopScrollLayer"), "serialized output must contain PopScrollLayer");
        assert!(s.contains("scroll=(5.00,15.00)"), "scroll offsets must appear in serialization");
    }

    #[test]
    fn overflow_auto_emits_push_scroll_layer() {
        // overflow:auto must produce PushScrollLayer just like overflow:scroll.
        let dl = build(
            r#"<div style="overflow:auto;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_push = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        let has_pop  = dl.iter().any(|c| matches!(c, DisplayCommand::PopScrollLayer));
        assert!(has_push, "overflow:auto must emit PushScrollLayer");
        assert!(has_pop,  "overflow:auto must emit PopScrollLayer");
    }

    // ── DrawScrollbar ─────────────────────────────────────────────────────────

    /// overflow:scroll with content taller than clip → vertical DrawScrollbar emitted.
    #[test]
    fn overflow_scroll_with_overflow_emits_draw_scrollbar_vertical() {
        // div 100×50 with a 200px-tall child → content overflows vertically.
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawScrollbar { vertical, .. } => Some(*vertical),
                _ => None,
            })
            .collect();
        assert!(!bars.is_empty(), "должен быть хотя бы один DrawScrollbar");
        assert!(bars.contains(&true), "должен быть вертикальный DrawScrollbar");
    }

    /// overflow:scroll with content fitting inside → no DrawScrollbar (no overflow).
    #[test]
    fn overflow_scroll_without_overflow_no_draw_scrollbar() {
        // div 100×200 with a 50px-tall child → no vertical overflow.
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:200px"><div style="height:50px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "нет переполнения → нет DrawScrollbar");
    }

    /// DrawScrollbar thumb_rect is inside track_rect.
    #[test]
    fn draw_scrollbar_thumb_inside_track() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("должен быть вертикальный DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { track_rect, thumb_rect, vertical: true, .. } = sb {
            // Track right edge must be at right edge of clip (within padding box).
            assert!(track_rect.width > 0.0, "track width > 0");
            assert!(thumb_rect.height > 0.0, "thumb height > 0");
            // Thumb must be inside track vertically.
            assert!(
                thumb_rect.y >= track_rect.y,
                "thumb top must be >= track top"
            );
            assert!(
                thumb_rect.y + thumb_rect.height <= track_rect.y + track_rect.height + 1.0,
                "thumb bottom must be <= track bottom"
            );
        }
    }

    /// DrawScrollbar serialization round-trip.
    #[test]
    fn draw_scrollbar_serialize() {
        let dl = vec![DisplayCommand::DrawScrollbar {
            track_rect: Rect::new(90.0, 0.0, 12.0, 50.0),
            thumb_rect: Rect::new(92.0, 5.0, 8.0, 20.0),
            vertical: true,
            thumb_color: SCROLLBAR_THUMB_COLOR,
            track_color: SCROLLBAR_TRACK_COLOR,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawScrollbar"), "serialization must contain DrawScrollbar");
        assert!(s.contains("vertical"), "serialization must mention orientation");
    }

    /// `scrollbar-width: none` suppresses DrawScrollbar while keeping scroll layer.
    #[test]
    fn scrollbar_width_none_no_draw_scrollbar() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-width:none"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "scrollbar-width:none → нет DrawScrollbar");
        // Scroll layer must still be present so content can scroll.
        let has_scroll = dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        assert!(has_scroll, "scrollbar-width:none → scroll layer должен оставаться");
    }

    /// `scrollbar-width: thin` emits DrawScrollbar with narrower track (6px gutter).
    #[test]
    fn scrollbar_width_thin_narrow_track() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-width:thin"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("thin scrollbar must emit DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { track_rect, .. } = sb {
            assert!(
                (track_rect.width - SCROLLBAR_WIDTH_THIN).abs() < 0.5,
                "thin track width should be ~{} px, got {}",
                SCROLLBAR_WIDTH_THIN,
                track_rect.width
            );
        }
    }

    /// `scrollbar-color` wires custom thumb+track colors into DrawScrollbar.
    #[test]
    fn scrollbar_color_custom_colors() {
        // red thumb, blue track
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-color:red blue"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("must emit DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { thumb_color, track_color, .. } = sb {
            // Red thumb: r≈1.0, g≈0, b≈0
            assert!(thumb_color[0] > 0.9, "thumb red channel must be ~1.0");
            assert!(thumb_color[1] < 0.1, "thumb green channel must be ~0");
            // Blue track: b≈1.0, r≈0
            assert!(track_color[2] > 0.9, "track blue channel must be ~1.0");
            assert!(track_color[0] < 0.1, "track red channel must be ~0");
        }
    }

    /// overflow:hidden does not emit DrawScrollbar (no scroll layer).
    #[test]
    fn overflow_hidden_no_scrollbar() {
        let dl = build(
            r#"<div style="overflow:hidden;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "overflow:hidden → нет DrawScrollbar");
    }

    // ── PageBreak / print display list ────────────────────────────────────────

    /// split_at_page_breaks on empty input → one empty page.
    #[test]
    fn split_empty_yields_one_empty_page() {
        let pages = split_at_page_breaks(vec![]);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].is_empty());
    }

    /// split_at_page_breaks with no PageBreak → one page with all commands.
    #[test]
    fn split_no_breaks_single_page() {
        use lumen_core::geom::Rect;
        let cmds = vec![
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 },
                color: Color { r: 255, g: 0, b: 0, a: 255 },
            },
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 10.0, width: 10.0, height: 10.0 },
                color: Color { r: 0, g: 255, b: 0, a: 255 },
            },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].len(), 2);
    }

    /// split_at_page_breaks with one PageBreak → two pages.
    #[test]
    fn split_one_break_two_pages() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 255, g: 0, b: 0, a: 255 } },
            DisplayCommand::PageBreak,
            DisplayCommand::FillRect { rect: r, color: Color { r: 0, g: 0, b: 255, a: 255 } },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].len(), 1); // one FillRect on page 0
        assert_eq!(pages[1].len(), 1); // one FillRect on page 1
        // PageBreak itself must not appear in any page
        for page in &pages {
            assert!(!page.iter().any(|c| matches!(c, DisplayCommand::PageBreak)));
        }
    }

    /// split_at_page_breaks with two PageBreaks → three pages, middle page empty.
    #[test]
    fn split_two_breaks_three_pages_middle_empty() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 1, g: 2, b: 3, a: 255 } },
            DisplayCommand::PageBreak,
            DisplayCommand::PageBreak,
            DisplayCommand::FillRect { rect: r, color: Color { r: 4, g: 5, b: 6, a: 255 } },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].len(), 1);
        assert_eq!(pages[1].len(), 0); // empty middle page
        assert_eq!(pages[2].len(), 1);
    }

    /// build_print_display_list on zero pages → empty list.
    #[test]
    fn print_dl_empty_pages() {
        let cmds = build_print_display_list(&[]);
        assert!(cmds.is_empty());
    }

    // ── strip_background_graphics (CC-8) ────────────────────────────────────

    /// `print_backgrounds = true` is a no-op: every command survives.
    #[test]
    fn strip_bg_keeps_all_when_enabled() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 };
        let mut pages = vec![vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 1, g: 2, b: 3, a: 255 } },
            DisplayCommand::DrawLinearGradient { rect: r, angle_deg: 0.0, stops: vec![], repeating: false },
        ]];
        strip_background_graphics(&mut pages, true);
        assert_eq!(pages[0].len(), 2);
    }

    /// `print_backgrounds = false` removes solid background fills + gradients +
    /// background images, but keeps text, borders and `<img>` foreground.
    #[test]
    fn strip_bg_removes_background_family_when_disabled() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 };
        let mut pages = vec![vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 1, g: 2, b: 3, a: 255 } },
            DisplayCommand::FillRoundedRect { rect: r, color: Color { r: 1, g: 2, b: 3, a: 255 }, radii: CornerRadii::default() },
            DisplayCommand::DrawLinearGradient { rect: r, angle_deg: 0.0, stops: vec![], repeating: false },
            DisplayCommand::DrawRadialGradient { rect: r, center_x_pct: 0.5, center_y_pct: 0.5, radius_x: 2.5, radius_y: 2.5, stops: vec![], repeating: false },
            DisplayCommand::DrawConicGradient { rect: r, center_x_pct: 0.5, center_y_pct: 0.5, from_angle_deg: 0.0, stops: vec![], repeating: false },
            DisplayCommand::DrawBackgroundImage {
                rect: r, origin_rect: r, src: "bg.png".to_owned(),
                size: BackgroundSize::Auto, position: ObjectPosition::default(),
                repeat: BackgroundRepeat::default(), image_rendering: ImageRendering::Auto,
            },
            DisplayCommand::DrawText {
                rect: r, text: "hi".to_owned(), font_size: 12.0,
                color: Color { r: 0, g: 0, b: 0, a: 255 }, font_family: vec![],
                font_weight: FontWeight::NORMAL, font_style: FontStyle::Normal,
                font_stretch: FontStretch::NORMAL,
                font_variation_axes: vec![], font_features: vec![], tab_size: 0.0,
                font_palette: None,
                highlight_name: None, text_orientation: None,
            },
            DisplayCommand::DrawImage {
                rect: r, src: "img.png".to_owned(), alt: String::new(),
                object_fit: ObjectFit::Fill, object_position: ObjectPosition::default(),
                image_rendering: ImageRendering::Auto,
            },
        ]];
        strip_background_graphics(&mut pages, false);
        assert_eq!(pages[0].len(), 2, "only DrawText + DrawImage survive");
        assert!(matches!(pages[0][0], DisplayCommand::DrawText { .. }));
        assert!(matches!(pages[0][1], DisplayCommand::DrawImage { .. }));
    }

    /// Filtering is applied per page across a multi-page job and keeps
    /// `Push*`/`Pop*` nesting balanced (only leaf fills are dropped).
    #[test]
    fn strip_bg_per_page_and_balanced_nesting() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 };
        let mut pages = vec![
            vec![
                DisplayCommand::PushClipRect { rect: r },
                DisplayCommand::FillRect { rect: r, color: Color { r: 9, g: 9, b: 9, a: 255 } },
                DisplayCommand::PopClip,
            ],
            vec![
                DisplayCommand::FillRect { rect: r, color: Color { r: 1, g: 1, b: 1, a: 255 } },
            ],
        ];
        strip_background_graphics(&mut pages, false);
        // Page 0: clip push/pop remain, the fill between them is gone.
        assert_eq!(pages[0].len(), 2);
        assert!(matches!(pages[0][0], DisplayCommand::PushClipRect { .. }));
        assert!(matches!(pages[0][1], DisplayCommand::PopClip));
        // Page 1: lone background fill removed → empty.
        assert!(pages[1].is_empty());
    }

    /// Empty input slice is handled without panicking.
    #[test]
    fn strip_bg_empty_pages_noop() {
        let mut pages: Vec<Vec<DisplayCommand>> = vec![];
        strip_background_graphics(&mut pages, false);
        assert!(pages.is_empty());
    }

    /// build_print_display_list on two pages inserts exactly one PageBreak.
    #[test]
    fn print_dl_two_pages_one_page_break() {
        use lumen_layout::{paginate, PaginationContext};

        let doc = lumen_html_parser::parse(
            "<div style='height:600px;background:red'></div><div style='height:600px;background:blue'></div>",
        );
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 1200.0));

        let ctx = PaginationContext {
            page_width: 800.0,
            page_height: 600.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        };
        let pages = paginate(&tree, &ctx);
        // If content fits in one page or pagination yields 0/1 page, skip assertion
        if pages.len() < 2 {
            return;
        }
        let cmds = build_print_display_list(&pages);
        let breaks = cmds.iter().filter(|c| matches!(c, DisplayCommand::PageBreak)).count();
        assert_eq!(breaks, pages.len() - 1, "N pages → N-1 PageBreaks");
    }

    // ── Tests for build_print_display_list margin-box rendering ──────────

    /// Page without page_box emits no margin-box DrawText commands.
    #[test]
    fn print_dl_no_page_box_no_margin_text() {
        use lumen_layout::{paginate, PaginationContext};

        let doc = lumen_html_parser::parse("<div style='height:100px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(400.0, 600.0));
        let ctx = PaginationContext {
            page_width: 400.0,
            page_height: 600.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        };
        let pages = paginate(&tree, &ctx);
        assert!(!pages.is_empty());
        // No page_box — no DrawText from margin boxes
        let cmds = build_print_display_list(&pages);
        let text_cmds: Vec<_> = cmds.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).collect();
        assert!(text_cmds.is_empty(), "no margin-box DrawText without page_box");
    }

    /// Page with a page_box containing bottom-center text emits a DrawText command.
    #[test]
    fn print_dl_page_box_bottom_center_emits_draw_text() {
        use lumen_layout::{
            paginate, MarginBoxPosition, PageBox, PageProperties, PaginationContext, TextMeasurer,
        };

        struct Fixed8;
        impl TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }

        let doc = lumen_html_parser::parse("<div style='height:100px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(400.0, 600.0));
        let ctx = PaginationContext {
            page_width: 400.0,
            page_height: 600.0,
            margin_top: 40.0,
            margin_bottom: 40.0,
            margin_left: 40.0,
            margin_right: 40.0,
        };
        let mut pages = paginate(&tree, &ctx);
        assert!(!pages.is_empty());

        let props = PageProperties {
            width: 400.0, height: 600.0,
            orientation: "portrait".to_string(),
            margin_top: 40.0, margin_bottom: 40.0,
            margin_left: 40.0, margin_right: 40.0,
        };
        let mut page_box = PageBox::new(0, props);
        page_box.layout_margin_boxes();
        let label = "1 / 1";
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::BottomCenter) {
            mb.content = Some(label.to_string());
            mb.layout_text(label, 10.0, 15.0, &Fixed8);
        }
        pages[0].page_box = Some(page_box);

        let cmds = build_print_display_list(&pages);
        let texts: Vec<&str> = cmds.iter().filter_map(|c| {
            if let DisplayCommand::DrawText { text, .. } = c { Some(text.as_str()) } else { None }
        }).collect();
        assert!(texts.contains(&"1 / 1"), "expected '1 / 1' in DrawText, got: {:?}", texts);
    }

    /// Margin-box DrawText positioned at page-box coordinates (not inside content transform).
    #[test]
    fn print_dl_margin_box_text_absolute_position() {
        use lumen_layout::{
            paginate, MarginBoxPosition, PageBox, PageProperties, PaginationContext, TextMeasurer,
        };

        struct Fixed8;
        impl TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }

        let doc = lumen_html_parser::parse("<div style='height:50px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(200.0, 300.0));
        let ctx = PaginationContext {
            page_width: 200.0,
            page_height: 300.0,
            margin_top: 30.0,
            margin_bottom: 30.0,
            margin_left: 30.0,
            margin_right: 30.0,
        };
        let mut pages = paginate(&tree, &ctx);

        let props = PageProperties {
            width: 200.0, height: 300.0,
            orientation: "portrait".to_string(),
            margin_top: 30.0, margin_bottom: 30.0,
            margin_left: 30.0, margin_right: 30.0,
        };
        let mut page_box = PageBox::new(0, props);
        page_box.layout_margin_boxes();
        let label = "PG1";
        // Use top-left-corner so we can predict coordinates: x=0, y=0
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::TopLeftCorner) {
            mb.content = Some(label.to_string());
            mb.layout_text(label, 10.0, 15.0, &Fixed8);
        }
        pages[0].page_box = Some(page_box);

        let cmds = build_print_display_list(&pages);
        let pg1_rect = cmds.iter().find_map(|c| {
            if let DisplayCommand::DrawText { text, rect, .. } = c {
                if text == "PG1" { Some(*rect) } else { None }
            } else { None }
        });
        let rect = pg1_rect.expect("DrawText 'PG1' not found");
        // TopLeftCorner is at page origin (0,0); fragment offset is 0,0 inside box
        assert!(rect.x >= 0.0 && rect.x < 10.0, "x should be at page origin, got {}", rect.x);
        assert!(rect.y >= 0.0 && rect.y < 10.0, "y should be at page origin, got {}", rect.y);
    }

    // ── Tests for DrawCrossFade ────────────────────────────────────────────

    /// Конструкция DrawCrossFade сохраняет все поля без потерь.
    #[test]
    fn cross_fade_construction_preserves_fields() {
        let cmd = DisplayCommand::DrawCrossFade {
            dest: Rect::new(10.0, 20.0, 100.0, 50.0),
            src_a: "first.png".to_string(),
            src_b: "second.png".to_string(),
            progress: 0.25,
        };
        if let DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } = &cmd {
            assert!((dest.x - 10.0).abs() < f32::EPSILON);
            assert!((dest.y - 20.0).abs() < f32::EPSILON);
            assert!((dest.width - 100.0).abs() < f32::EPSILON);
            assert!((dest.height - 50.0).abs() < f32::EPSILON);
            assert_eq!(src_a, "first.png");
            assert_eq!(src_b, "second.png");
            assert!((progress - 0.25).abs() < f32::EPSILON);
        } else {
            panic!("expected DrawCrossFade variant");
        }
    }

    /// serialize_display_list печатает все ключевые поля в детерминированном формате.
    #[test]
    fn cross_fade_serialize_includes_all_fields() {
        let dl = vec![DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 200.0, 100.0),
            src_a: "a.png".to_string(),
            src_b: "b.png".to_string(),
            progress: 0.5,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.starts_with("DrawCrossFade "), "should start with command name: {s}");
        assert!(s.contains("(0.00, 0.00, 200.00, 100.00)"), "should contain dest rect: {s}");
        assert!(s.contains(r#"a="a.png""#), "should contain src_a: {s}");
        assert!(s.contains(r#"b="b.png""#), "should contain src_b: {s}");
        assert!(s.contains("p=0.500"), "should contain progress: {s}");
    }

    /// Equality / Debug на варианте работают через производные —
    /// важно для snapshot-тестов и assert_eq! в downstream-крейтах.
    #[test]
    fn cross_fade_equality_and_debug() {
        let a = DisplayCommand::DrawCrossFade {
            dest: Rect::new(1.0, 2.0, 3.0, 4.0),
            src_a: "x".into(),
            src_b: "y".into(),
            progress: 0.75,
        };
        let b = a.clone();
        assert_eq!(a, b, "Clone должен сохранять равенство");
        let dbg = format!("{a:?}");
        assert!(dbg.contains("DrawCrossFade"), "Debug должен включать имя варианта: {dbg}");
        assert!(dbg.contains("0.75"), "Debug должен включать progress: {dbg}");

        // Граничные значения: progress = 0.0 (только src_a) и 1.0 (только src_b)
        // — оба валидны и различимы.
        let zero = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 10.0, 10.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 0.0,
        };
        let one = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 10.0, 10.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 1.0,
        };
        assert_ne!(zero, one, "progress=0.0 и progress=1.0 — разные команды");
    }

    /// DrawCrossFade попадает в exhaustive-match киндов (защита от
    /// «забыли добавить ветку при extension enum-а»).
    #[test]
    fn cross_fade_appears_in_kind_dispatch() {
        let cmd = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 1.0, 1.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 0.5,
        };
        // Если когда-нибудь матч в `img_with_background_and_border_paints_in_order`
        // перестанет включать DrawCrossFade — компилятор не пропустит код.
        // Здесь просто smoke-проверяем сериализацию через публичный API.
        let s = serialize_display_list(std::slice::from_ref(&cmd));
        assert!(s.contains("DrawCrossFade"));
    }

