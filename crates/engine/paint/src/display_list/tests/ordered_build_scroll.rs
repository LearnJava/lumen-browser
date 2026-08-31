//! P1/SPLIT-DL5: build_display_list_ordered/patch_scroll_layer/layer-ops
//! эмиссия тесты в `mod tests` `display_list.rs` — build_ordered/layout_for/
//! ordered_of/check_patch_equals_rebuild хелперы, `patch_scroll_layer_*`,
//! `ordered_*` (opacity/blend-mode/overflow/transform/mask-clip/z-index/
//! backface-visibility/scroll-container), count_variant хелпер.
//! Вторая половина региона DL-5 — см. `background_and_layers.rs` (первая
//! половина того же батча, iframe/audio/bg-image/gradient/mask/object-fit).
//! Перенесено байт-в-байт из `display_list.rs` без дедента (приём ST-1/DL-1).
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-5).

use super::*;
// Fixed8 уехал в display_list/tests/text_and_images.rs (батч DL-6).
use super::text_and_images::Fixed8;

    // ── build_display_list_ordered ─────────────────────────────────────

    pub(crate) fn build_ordered(html: &str, css: &str) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(
            &doc,
            &sheet,
            Size::new(800.0, 600.0),
            &Fixed8,
        );
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        build_display_list_ordered(&tree, &stacking_tree, &order).0
    }

    // ── patch_scroll_layer: эквивалентность полной пересборке ─────────────

    fn layout_for(html: &str, css: &str) -> lumen_layout::LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8)
    }

    fn ordered_of(tree: &lumen_layout::LayoutBox) -> DisplayList {
        let stacking_tree = lumen_layout::StackingTree::build(tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        build_display_list_ordered(tree, &stacking_tree, &order).0
    }

    fn assert_dl_eq(a: &[DisplayCommand], b: &[DisplayCommand]) {
        assert_eq!(a.len(), b.len(), "длины display list различаются");
        for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            assert_eq!(format!("{x:?}"), format!("{y:?}"), "команда #{i} различается");
        }
    }

    /// Скроллит единственный overflow-контейнер страницы в (x, y) и сверяет
    /// патч старого DL с полной пересборкой.
    fn check_patch_equals_rebuild(html: &str, css: &str, x: f32, y: f32) {
        let mut tree = layout_for(html, css);
        let containers = lumen_layout::collect_scroll_containers(&tree);
        assert_eq!(containers.len(), 1, "тест ожидает ровно один scroll-контейнер");
        let node = containers[0].node;
        let mut dl = ordered_of(&tree);
        assert!(
            lumen_layout::set_scroll_position(&mut tree, node, x, y),
            "set_scroll_position должен найти контейнер"
        );
        let truth = ordered_of(&tree);
        let b = lumen_layout::find_box_by_node(&tree, node).expect("бокс контейнера");
        assert!(patch_scroll_layer(&mut dl, b), "патч должен пройти");
        assert_dl_eq(&dl, &truth);
    }

    #[test]
    fn patch_scroll_layer_equals_rebuild_vertical() {
        check_patch_equals_rebuild(
            "<div class='s'><div class='tall'>x</div></div>",
            "body { margin: 0; }              .s { overflow-y: auto; overflow-x: hidden; width: 100px; height: 80px; }              .tall { height: 400px; }",
            0.0,
            40.0,
        );
    }

    #[test]
    fn patch_scroll_layer_equals_rebuild_both_axes_with_border() {
        check_patch_equals_rebuild(
            "<div class='s'><div class='big'>x</div></div>",
            "body { margin: 0; }              .s { overflow: scroll; width: 120px; height: 90px;                   border: 3px solid #0f3460; }              .big { width: 500px; height: 400px; }",
            35.0,
            60.0,
        );
    }

    #[test]
    fn patch_scroll_layer_equals_rebuild_zindexed_child_reestablished() {
        // BUG-159: z-indexed ребёнок плоского overflow:auto контейнера получает
        // переустановленный PushScrollLayer в отдельном painting-слоте — патч
        // обязан обновить обе копии.
        check_patch_equals_rebuild(
            "<div class='s'><div class='inner'></div></div>",
            "body { margin: 0; }              .s { width: 100px; height: 100px; overflow: auto; }              .inner { position: relative; z-index: 1; width: 50px; height: 200px;                       background: #0000ff; }",
            0.0,
            30.0,
        );
    }

    #[test]
    fn patch_scroll_layer_scrollbar_none_no_bars() {
        check_patch_equals_rebuild(
            "<div class='s'><div class='tall'>x</div></div>",
            "body { margin: 0; }              .s { overflow-y: auto; overflow-x: hidden; width: 100px; height: 80px;                   scrollbar-width: none; }              .tall { height: 300px; }",
            0.0,
            25.0,
        );
    }

    /// Микробенч (не гейт): выигрыш патча против полной пересборки.
    /// `cargo test -p lumen-paint --release patch_scroll_layer_bench -- --ignored --nocapture`
    #[test]
    #[ignore = "бенч, запускается вручную"]
    fn patch_scroll_layer_bench() {
        let rows: String = (0..400)
            .map(|i| format!("<div class='r c{}'>row</div>", i % 7))
            .collect();
        let html = format!("<div class='s'>{rows}</div>");
        let css = "body { margin: 0; }              .s { overflow-y: auto; overflow-x: hidden; width: 300px; height: 200px; }              .r { height: 24px; border: 1px solid #333; }              .c0 { background: #123; } .c1 { background: #234; } .c2 { background: #345; }              .c3 { background: #456; } .c4 { background: #567; } .c5 { background: #678; }              .c6 { background: #789; }";
        let mut tree = layout_for(&html, css);
        let node = lumen_layout::collect_scroll_containers(&tree)[0].node;
        let dl0 = ordered_of(&tree);
        let n = 200;
        let t0 = std::time::Instant::now();
        for i in 0..n {
            lumen_layout::set_scroll_position(&mut tree, node, 0.0, i as f32);
            let _ = ordered_of(&tree);
        }
        let rebuild = t0.elapsed();
        let t1 = std::time::Instant::now();
        for i in 0..n {
            lumen_layout::set_scroll_position(&mut tree, node, 0.0, i as f32);
            let mut dl = dl0.clone();
            let b = lumen_layout::find_box_by_node(&tree, node).unwrap();
            assert!(patch_scroll_layer(&mut dl, b));
        }
        let patch = t1.elapsed();
        println!(
            "DL {} команд: rebuild {:.3} мс/тик, patch(+clone) {:.3} мс/тик",
            dl0.len(),
            rebuild.as_secs_f64() * 1e3 / n as f64,
            patch.as_secs_f64() * 1e3 / n as f64,
        );
    }

    #[test]
    fn patch_scroll_layer_rejects_non_scroll_box() {
        let tree = layout_for("<div>x</div>", "div { width: 50px; height: 50px; }");
        let mut dl = ordered_of(&tree);
        // Корень дерева — не scroll-контейнер: патч обязан отказаться.
        assert!(!patch_scroll_layer(&mut dl, &tree));
    }

    #[test]
    fn ordered_backface_hidden_rotated_past_90deg_forces_zero_opacity() {
        // `build_display_list_ordered` is the path the real shell/screenshot
        // renderer uses (unlike the simpler `walk`/`build_display_list`).
        // A 3D-rotated box always owns its own stacking context
        // (`creates_stacking_context`), so backface culling here must ride
        // the existing opacity-0 compositing layer rather than a bare skip.
        let dl = build_ordered(
            r#"<div style="width:50px;height:50px;background:red;
                transform: rotateY(180deg); backface-visibility: hidden;">x</div>"#,
            "",
        );
        let alpha = dl.iter().find_map(|c| match c {
            DisplayCommand::PushOpacity { alpha, .. } => Some(*alpha),
            _ => None,
        });
        assert_eq!(alpha, Some(0.0), "backface-hidden rotated box must force PushOpacity 0");
    }

    #[test]
    fn ordered_backface_hidden_facing_viewer_still_opaque() {
        let dl = build_ordered(
            r#"<div style="width:50px;height:50px;background:red;
                transform: rotateY(0deg); backface-visibility: hidden;">x</div>"#,
            "",
        );
        assert!(
            !dl.iter().any(|c| matches!(c, DisplayCommand::PushOpacity { alpha, .. } if *alpha == 0.0)),
            "front-facing box must not be forced to zero opacity"
        );
    }

    /// BUG-183: a gradient `mask-image` makes the box a stacking context, so it
    /// is painted through `build_display_list_ordered` (the bucket path), not
    /// `walk`. Before the fix `box_layer_ops` did not open the mask group, so the
    /// gradient mask silently became a no-op. The ordered output must wrap the
    /// box's background between `PushMaskLinearGradient` and `PopMask`.
    #[test]
    fn ordered_mask_image_gradient_wraps_box_as_stacking_context() {
        let dl = build_ordered(
            "<div class='m'></div>",
            ".m { width: 100px; height: 100px; background: #f00; \
             mask-image: linear-gradient(to bottom, black, transparent); }",
        );
        let push = dl.iter().position(|c| {
            matches!(c, DisplayCommand::PushMaskLinearGradient { .. })
        });
        let fill = dl.iter().position(|c| matches!(c, DisplayCommand::FillRect { .. }));
        let pop = dl.iter().position(|c| matches!(c, DisplayCommand::PopMask));
        let push = push.expect("ordered path must emit PushMaskLinearGradient");
        let pop = pop.expect("ordered path must emit PopMask");
        let fill = fill.expect("masked box must still fill its background");
        assert!(push < fill && fill < pop, "mask must wrap the box background: push={push} fill={fill} pop={pop}");
    }

    /// CSS Masking L1 §4.6 — `mask-clip: content-box` must clip the masked
    /// element's painting to its content box. The ordered path wraps the mask
    /// group in a `PushClipRect` (content-box rect) / `PopClip` pair *inside*
    /// `PushMask…` / `PopMask`, so the stream is
    /// `PushMask … PushClipRect … PopClip PopMask`.
    #[test]
    fn ordered_mask_clip_content_box_clips_painting_area() {
        let dl = build_ordered(
            "<div class='m'></div>",
            ".m { width: 100px; height: 100px; border: 10px solid #000; \
             padding: 5px; background: #f00; \
             mask-image: linear-gradient(to bottom, black, transparent); \
             mask-clip: content-box; }",
        );
        let push_mask = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushMaskLinearGradient { .. }))
            .expect("must emit PushMaskLinearGradient");
        let (clip_pos, clip_rect) = dl
            .iter()
            .enumerate()
            .find_map(|(i, c)| match c {
                DisplayCommand::PushClipRect { rect } => Some((i, *rect)),
                _ => None,
            })
            .expect("mask-clip: content-box must emit a PushClipRect");
        let pop_clip = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopClip))
            .expect("must emit PopClip");
        let pop_mask = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopMask))
            .expect("must emit PopMask");
        assert!(
            push_mask < clip_pos && clip_pos < pop_clip && pop_clip < pop_mask,
            "mask-clip must nest inside the mask group: \
             push_mask={push_mask} clip={clip_pos} pop_clip={pop_clip} pop_mask={pop_mask}"
        );
        // content-box width = border-box(130) − 2·border(10) − 2·padding(5) = 100.
        assert_eq!(clip_rect.width, 100.0, "clip width must equal content-box width");
        assert_eq!(clip_rect.height, 100.0, "clip height must equal content-box height");
    }

    /// The default `mask-clip: border-box` must stay a no-op: no extra
    /// `PushClipRect` is emitted around the mask group (byte-identical to the
    /// pre-mask-clip behaviour).
    #[test]
    fn ordered_mask_clip_border_box_emits_no_clip() {
        let dl = build_ordered(
            "<div class='m'></div>",
            ".m { width: 100px; height: 100px; background: #f00; \
             mask-image: linear-gradient(to bottom, black, transparent); }",
        );
        assert!(
            !dl.iter().any(|c| matches!(c, DisplayCommand::PushClipRect { .. })),
            "border-box mask-clip must not emit a PushClipRect"
        );
    }

    /// CSS Masking L1 §4.6 — `mask-clip: fill-box` on a CSS box has no SVG
    /// geometry, so its object bounding box is the content box (CSS Box 4 §1).
    /// It must clip the mask painting to the content-box rect, exactly like
    /// `content-box`.
    #[test]
    fn ordered_mask_clip_fill_box_clips_to_content_box() {
        let dl = build_ordered(
            "<div class='m'></div>",
            ".m { width: 100px; height: 100px; border: 10px solid #000; \
             padding: 5px; background: #f00; \
             mask-image: linear-gradient(to bottom, black, transparent); \
             mask-clip: fill-box; }",
        );
        let clip_rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("mask-clip: fill-box must emit a PushClipRect (content-box)");
        // content-box width = border-box(130) − 2·border(10) − 2·padding(5) = 100.
        assert_eq!(clip_rect.width, 100.0, "fill-box clip width must equal content-box width");
        assert_eq!(clip_rect.height, 100.0, "fill-box clip height must equal content-box height");
    }

    /// CSS Masking L1 §4.6 — `stroke-box`/`view-box` fall back to the border box
    /// for CSS boxes (= `b.rect`), and `no-clip` disables the clip. None of them
    /// may emit an extra `PushClipRect`, matching the `border-box` no-op.
    #[test]
    fn ordered_mask_clip_border_equivalents_emit_no_clip() {
        for value in ["stroke-box", "view-box", "no-clip"] {
            let css = format!(
                ".m {{ width: 100px; height: 100px; border: 10px solid #000; \
                 background: #f00; \
                 mask-image: linear-gradient(to bottom, black, transparent); \
                 mask-clip: {value}; }}"
            );
            let dl = build_ordered("<div class='m'></div>", &css);
            assert!(
                !dl.iter().any(|c| matches!(c, DisplayCommand::PushClipRect { .. })),
                "mask-clip: {value} must not emit a PushClipRect (border-box equivalent)"
            );
        }
    }

    /// BUG-200: under `border-collapse: collapse` a thick cell border must survive a
    /// thinner neighbour's background. Cells overlap on the shared grid line; emitted in
    /// DOM order a later thin cell's background overpaints the earlier thick border. The
    /// ordered path must redraw cell borders after all cell backgrounds, so a 3px border
    /// appears in the command stream *after* the last cell-background fill.
    #[test]
    fn ordered_collapse_thick_border_redrawn_after_cell_backgrounds() {
        let dl = build_ordered(
            "<table><tr><td class='a'>x</td><td class='b'>y</td></tr></table>",
            "table { border-collapse: collapse; } \
             td { width: 40px; height: 20px; } \
             td.a { border: 3px solid #f85149; background: #112233; } \
             td.b { border: 1px solid #f85149; background: #445566; }",
        );
        let last_fill = dl
            .iter()
            .rposition(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .expect("cells must fill backgrounds");
        let last_thick = dl
            .iter()
            .rposition(|c| matches!(c, DisplayCommand::DrawBorder { widths, .. }
                if widths.iter().any(|&w| (w - 3.0).abs() < 0.01)))
            .expect("thick cell border must be emitted");
        assert!(
            last_thick > last_fill,
            "thick collapsed border (idx {last_thick}) must be redrawn after the last \
             cell background (idx {last_fill}), else the thin neighbour erases it",
        );
    }

    #[test]
    fn ordered_single_sc_matches_dom_order_output() {
        // На странице без stacking-triggers `build_display_list_ordered`
        // и `build_display_list` должны эмитить ровно одинаковые команды
        // (порядок DOM = paint order для одного SC).
        let html = "<div style='background:#f00;'>hello</div>";
        let css = "";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(
            &doc,
            &sheet,
            Size::new(800.0, 600.0),
            &Fixed8,
        );
        let dom = build_display_list(&tree);
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        let ordered = build_display_list_ordered(&tree, &stacking_tree, &order).0;
        assert_eq!(dom, ordered);
    }

    #[test]
    fn ordered_positive_z_child_painted_after_root_content() {
        // <div z=1 (opacity)>SC-creating</div> рядом с inline-текстом.
        // Ordered-вывод: root.bg → root.contents (включая текст) →
        // child-SC contents (заминусованный, чтобы создать SC).
        //
        // Используем opacity:0.5 как SC-trigger без z-index (auto = phase 6,
        // эмитится ПОСЛЕ root.InlineContent).
        let dl = build_ordered(
            "<p>hello</p><div>world</div>",
            "div { opacity: 0.5; }",
        );
        // Должны быть текстовые узлы из обеих секций. Главное —
        // div-content (world) появляется после p-content (hello).
        let hello_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "hello")
        });
        let world_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "world")
        });
        assert!(
            hello_idx.is_some() && world_idx.is_some(),
            "обе строки должны рендериться"
        );
        assert!(
            hello_idx.unwrap() < world_idx.unwrap(),
            "child-SC (opacity div, phase 6) рисуется ПОСЛЕ root.contents (phase 5)"
        );
    }

    #[test]
    fn ordered_negative_z_child_painted_before_root_content() {
        // div с position:relative + z-index:-1 создаёт SC с negative-z.
        // Должен рисоваться до root.InlineContent (т.е. до текста "hello").
        let dl = build_ordered(
            "<div>neg</div><p>hello</p>",
            "div { position: relative; z-index: -1; background: #0f0; }",
        );
        // neg-content (DrawText "neg" внутри div) должен идти до root.contents
        // ("hello" внутри p).
        let neg_text = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "neg")
        });
        let hello_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "hello")
        });
        assert!(neg_text.is_some(), "должен быть DrawText neg");
        assert!(hello_idx.is_some(), "должен быть DrawText hello");
        assert!(
            neg_text.unwrap() < hello_idx.unwrap(),
            "neg-z div (phase 2) рисуется ДО root.InlineContent (phase 5)"
        );
    }

    // ── layer-ops эмиссия в build_display_list_ordered ─────────────────

    /// Helper: количество вхождений варианта в DisplayList.
    pub(crate) fn count_variant(dl: &DisplayList, predicate: impl Fn(&DisplayCommand) -> bool) -> usize {
        dl.iter().filter(|c| predicate(c)).count()
    }

    #[test]
    fn ordered_opacity_lt_one_emits_push_pop_pair() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 0.5; }");
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopOpacity));
        assert_eq!(pushes, 1, "opacity<1 → один PushOpacity");
        assert_eq!(pops, 1, "и парный PopOpacity");

        // Push до контента, Pop после.
        let push_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        let pop_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopOpacity))
            .unwrap();
        let text_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "x"));
        assert!(push_idx < pop_idx);
        if let Some(text_idx) = text_idx {
            assert!(push_idx < text_idx);
            assert!(text_idx < pop_idx);
        }
    }

    #[test]
    fn ordered_transformed_child_clipped_by_overflow_hidden_ancestor() {
        // BUG-131: a transformed child creates its own stacking context and is
        // emitted in a later painting-order slot. The overflow:hidden ancestor's
        // clip is emitted inline in the parent SC bucket and closes before the
        // child SC paints — so the clip must be re-established around the child.
        // Regression: net open clip-rects at the inner box must be > 0.
        let dl = build_ordered(
            "<div class='clip'><div class='inner'></div></div>",
            ".clip { width:100px; height:100px; overflow:hidden; } \
             .inner { width:50px; height:50px; background:#0000ff; \
                      transform:translate(40px,40px); }",
        );
        // The inner transformed box (#0000ff) sits in its own SC.
        let inner_fill = dl
            .iter()
            .position(|c| matches!(
                c,
                DisplayCommand::FillRect { color, .. }
                    if color.r == 0 && color.g == 0 && color.b == 255
            ))
            .expect("inner box FillRect must be emitted");
        // It must be wrapped by a PushTransform (its own SC layer).
        let transform_before = dl[..inner_fill]
            .iter()
            .rposition(|c| matches!(c, DisplayCommand::PushTransform { .. }));
        assert!(
            transform_before.is_some(),
            "inner box must be inside its PushTransform layer"
        );
        // Net open rect-clips just before the inner FillRect: with the fix the
        // ancestor `.clip` overflow clip is re-pushed, so depth >= 1.
        let mut depth: i32 = 0;
        for c in &dl[..inner_fill] {
            match c {
                DisplayCommand::PushClipRect { .. }
                | DisplayCommand::PushClipRoundedRect { .. } => depth += 1,
                DisplayCommand::PopClip => depth -= 1,
                _ => {}
            }
        }
        assert!(
            depth >= 1,
            "transformed child must stay inside its overflow:hidden ancestor's \
             clip (open clip depth = {depth}, expected >= 1)"
        );
        // The re-established clip must be the container's content box (0,0,100,100).
        let has_container_clip = dl[..inner_fill].iter().any(|c| matches!(
            c,
            DisplayCommand::PushClipRect { rect }
                if (rect.width - 100.0).abs() < 0.5 && (rect.height - 100.0).abs() < 0.5
        ));
        assert!(
            has_container_clip,
            "container's 100x100 overflow clip must wrap the transformed child"
        );
    }

    #[test]
    fn ordered_zindexed_child_scrolls_with_overflow_auto_ancestor() {
        // BUG-159: a z-indexed (own-SC) child of a plain `overflow:auto` scroll
        // container is emitted in a later painting-order slot. The container is
        // NOT itself a stacking context, so its PushScrollLayer/PopScrollLayer are
        // inline in the parent SC bucket and close before the child SC paints.
        // Without re-establishment the child escapes the scroll layer and behaves
        // like position:fixed (does not scroll). The fix re-pushes the scroll
        // layer around the child SC — net open scroll-layer depth at the child
        // must be >= 1.
        let dl = build_ordered(
            "<div class='scroll'><div class='inner'></div></div>",
            "body { margin:0; } \
             .scroll { width:100px; height:100px; overflow:auto; } \
             .inner { position:relative; z-index:1; width:50px; height:200px; \
                      background:#0000ff; }",
        );
        let inner_fill = dl
            .iter()
            .position(|c| matches!(
                c,
                DisplayCommand::FillRect { color, .. }
                    if color.r == 0 && color.g == 0 && color.b == 255
            ))
            .expect("inner box FillRect must be emitted");
        let mut depth: i32 = 0;
        for c in &dl[..inner_fill] {
            match c {
                DisplayCommand::PushScrollLayer { .. } => depth += 1,
                DisplayCommand::PopScrollLayer => depth -= 1,
                _ => {}
            }
        }
        assert!(
            depth >= 1,
            "z-indexed child must stay inside its overflow:auto ancestor's scroll \
             layer (open scroll-layer depth = {depth}, expected >= 1)"
        );
        // The re-established scroll layer must clip to the container padding box.
        let has_container_scroll = dl[..inner_fill].iter().any(|c| matches!(
            c,
            DisplayCommand::PushScrollLayer { clip_rect, .. }
                if (clip_rect.width - 100.0).abs() < 0.5
                    && (clip_rect.height - 100.0).abs() < 0.5
        ));
        assert!(
            has_container_scroll,
            "container's 100x100 scroll layer must wrap the z-indexed child"
        );
    }

    #[test]
    fn ordered_fixed_child_does_not_inherit_ancestor_scroll_layer() {
        // BUG-159: position:fixed is anchored to the viewport — it must NOT
        // inherit a scrolling ancestor's scroll-layer translate, or a fixed
        // overlay would scroll away with the page. A fixed child of an
        // `overflow:auto` container must paint with net scroll-layer depth 0.
        let dl = build_ordered(
            "<div class='scroll'><div class='fx'></div></div>",
            "body { margin:0; } \
             .scroll { width:100px; height:100px; overflow:auto; } \
             .fx { position:fixed; top:0; left:0; width:50px; height:50px; \
                   background:#00ff00; }",
        );
        let fx_fill = dl
            .iter()
            .position(|c| matches!(
                c,
                DisplayCommand::FillRect { color, .. }
                    if color.r == 0 && color.g == 255 && color.b == 0
            ))
            .expect("fixed box FillRect must be emitted");
        let mut depth: i32 = 0;
        for c in &dl[..fx_fill] {
            match c {
                DisplayCommand::PushScrollLayer { .. } => depth += 1,
                DisplayCommand::PopScrollLayer => depth -= 1,
                _ => {}
            }
        }
        assert_eq!(
            depth, 0,
            "fixed child must NOT inherit the scrolling ancestor's scroll layer \
             (open scroll-layer depth = {depth}, expected 0)"
        );
    }

    #[test]
    fn ordered_opacity_alpha_value_preserved() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 0.25; }");
        let push = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        if let DisplayCommand::PushOpacity { alpha, .. } = push {
            assert!((alpha - 0.25).abs() < 1e-6);
        } else {
            panic!("expected PushOpacity");
        }
    }

    #[test]
    fn ordered_opacity_one_does_not_emit() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 1; }");
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        assert_eq!(pushes, 0, "opacity:1 не триггерит Push");
    }

    #[test]
    fn ordered_mix_blend_mode_emits_push_pop() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { mix-blend-mode: multiply; }",
        );
        let pushes: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::PushBlendMode { mode, .. } => Some(*mode),
                _ => None,
            })
            .collect();
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopBlendMode));
        assert_eq!(pushes, vec![BlendMode::Multiply]);
        assert_eq!(pops, 1);
    }

    #[test]
    fn ordered_mix_blend_mode_normal_does_not_emit() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { mix-blend-mode: normal; }",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushBlendMode { .. }));
        assert_eq!(pushes, 0);
    }

    #[test]
    fn ordered_overflow_hidden_on_sc_owner_emits_clip() {
        // div c opacity<1 (= SC-owner) + overflow:hidden → Push/PopClipRect
        // в SC-owner bucket. Opacity тоже эмитится; проверяем clip отдельно.
        let dl = build_ordered(
            "<div>x</div>",
            "div { opacity: 0.5; overflow: hidden; width: 100px; height: 50px; }",
        );
        let pushes_clip: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(pushes_clip.len(), 1, "overflow:hidden → один PushClipRect");
        let pops_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PopClip));
        assert_eq!(pops_clip, 1);
    }

    #[test]
    fn ordered_overflow_hidden_on_non_sc_emits_clip_inline() {
        // div c overflow:hidden НЕ создаёт SC (overflow — не SC-trigger).
        // PushClipRect эмитится inline в bucket.contents текущего SC.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: hidden; width: 100px; height: 50px; }",
        );
        let pushes_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PushClipRect { .. }));
        let pops_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PopClip));
        assert_eq!(pushes_clip, 1);
        assert_eq!(pops_clip, 1);
        // SC не появился: PushOpacity/PushBlendMode не должны быть.
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. })),
            0
        );
    }

    #[test]
    fn ordered_overflow_visible_does_not_emit_clip() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: visible; opacity: 0.5; }",
        );
        let pushes_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PushClipRect { .. }));
        assert_eq!(pushes_clip, 0, "overflow:visible не клипает");
    }

    #[test]
    fn ordered_overflow_x_alone_triggers_clip() {
        // overflow-x:hidden + overflow-y:visible → CSS Overflow L3 §3.1 coercion
        // computes overflow-y to `auto`, which is a scroll container, so the
        // clip is established via PushScrollLayer (not a plain PushClipRect).
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow-x: hidden; width: 100px; height: 50px; }",
        );
        let clips = count_variant(&dl, |c| {
            matches!(c, DisplayCommand::PushClipRect { .. } | DisplayCommand::PushScrollLayer { .. })
        });
        assert_eq!(clips, 1, "overflow-x:hidden establishes one clip layer");
    }

    #[test]
    fn ordered_combined_opacity_blend_clip_emit_lifo() {
        // SC-owner со всеми тремя триггерами: проверяем парность и LIFO.
        let dl = build_ordered(
            "<div>x</div>",
            "div {
                opacity: 0.5;
                mix-blend-mode: multiply;
                overflow: hidden;
                width: 100px;
                height: 50px;
            }",
        );
        // Извлекаем последовательность layer-ops (без других команд).
        let ops: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    DisplayCommand::PushClipRect { .. }
                        | DisplayCommand::PopClip
                        | DisplayCommand::PushBlendMode { .. }
                        | DisplayCommand::PopBlendMode
                        | DisplayCommand::PushOpacity { .. }
                        | DisplayCommand::PopOpacity
                )
            })
            .collect();
        // Ожидаемый порядок (см. box_layer_ops): Blend → Opacity (эффекты,
        // оборачивают bg/border), затем overflow-Clip — внутренний, клипает
        // только детей (BUG-123). Pop — LIFO: Clip → Opacity → Blend.
        assert_eq!(ops.len(), 6, "три триггера = 6 layer-ops");
        assert!(matches!(ops[0], DisplayCommand::PushBlendMode { .. }));
        assert!(matches!(ops[1], DisplayCommand::PushOpacity { .. }));
        assert!(matches!(ops[2], DisplayCommand::PushClipRect { .. }));
        assert!(matches!(ops[3], DisplayCommand::PopClip));
        assert!(matches!(ops[4], DisplayCommand::PopOpacity));
        assert!(matches!(ops[5], DisplayCommand::PopBlendMode));
    }

    #[test]
    fn ordered_scroll_container_bg_border_outside_scroll_layer() {
        // BUG-123: собственные background/border скролл-контейнера эмитятся
        // ДО PushScrollLayer — overflow-клип (scissor по padding-box) не
        // должен срезать рамку и подрезать фон самого контейнера.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: scroll; width: 100px; height: 50px;
                   background: #16213e; border: 2px solid #0f3460; }",
        );
        let scroll_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }))
            .expect("scroll container emits PushScrollLayer");
        let border_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .expect("scroll container emits DrawBorder");
        assert!(
            border_idx < scroll_idx,
            "DrawBorder (idx {border_idx}) must precede PushScrollLayer (idx {scroll_idx})"
        );
    }

    #[test]
    fn ordered_nested_opacity_emits_two_pairs() {
        // Внешний div с opacity, внутренний div с opacity. Каждый создаёт
        // свой SC; должно быть 2 пары PushOpacity/PopOpacity.
        let dl = build_ordered(
            r#"<div class="outer"><div class="inner">x</div></div>"#,
            ".outer { opacity: 0.5; } .inner { opacity: 0.25; }",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopOpacity));
        assert_eq!(pushes, 2);
        assert_eq!(pops, 2);
    }

    #[test]
    fn ordered_nested_transforms_emit_two_pairs() {
        // BUG-139 регрессия: внешний div с transform, внутренний div с transform.
        // Каждый создаёт свой SC; должно быть ровно 2 пары PushTransform/PopTransform.
        let dl = build_ordered(
            r#"<div class="outer"><div class="inner">x</div></div>"#,
            ".outer { transform: rotate(15deg); } .inner { transform: rotate(-15deg); }",
        );
        let pushes =
            count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopTransform));
        assert_eq!(pushes, 2, "два вложенных transform → два PushTransform");
        assert_eq!(pops, 2, "и два парных PopTransform");
    }

    #[test]
    fn ordered_nested_transforms_parent_wraps_child() {
        // BUG-139 регрессия: Pop-команды родителя должны приходить ПОСЛЕ
        // Push/Pop-команд ребёнка. До фикса родительский PopTransform эмитился
        // в PaintPhase::InlineContent (до рендера дочерних SC), из-за чего
        // вложенные transforms не компоновались.
        let dl = build_ordered(
            r#"<div class="outer"><div class="inner">x</div></div>"#,
            ".outer { transform: rotate(10deg); } .inner { transform: translateX(50px); }",
        );
        // Позиции первого и второго вхождения
        let push_positions: Vec<usize> = dl
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if matches!(c, DisplayCommand::PushTransform { .. }) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        let pop_positions: Vec<usize> = dl
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if matches!(c, DisplayCommand::PopTransform) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(push_positions.len(), 2, "ожидается 2 PushTransform");
        assert_eq!(pop_positions.len(), 2, "ожидается 2 PopTransform");
        // Родительский Push идёт раньше дочернего Push
        assert!(
            push_positions[0] < push_positions[1],
            "родительский PushTransform (idx {}) должен предшествовать дочернему (idx {})",
            push_positions[0],
            push_positions[1]
        );
        // Дочерний Pop идёт раньше родительского Pop — ключевая инварианта BUG-139
        assert!(
            pop_positions[0] < pop_positions[1],
            "дочерний PopTransform (idx {}) должен предшествовать родительскому (idx {}), \
             иначе вложенные transforms не компонуются",
            pop_positions[0],
            pop_positions[1]
        );
        // Дочерний Push идёт ПОСЛЕ родительского Push (внутри родительского слоя)
        assert!(
            push_positions[0] < push_positions[1],
            "дочерний PushTransform должен быть внутри родительского слоя"
        );
        // Дочерний Pop идёт ДО родительского Pop (закрывается раньше)
        assert!(
            pop_positions[0] < pop_positions[1],
            "дочерний PopTransform должен закрыться до родительского"
        );
    }

    #[test]
    fn ordered_no_triggers_emits_no_layer_ops() {
        // Простая страница без opacity/blend/overflow — ни одной layer-op.
        let dl = build_ordered("<p>hello</p>", "");
        let any_layer_op = dl.iter().any(|c| {
            matches!(
                c,
                DisplayCommand::PushClipRect { .. }
                    | DisplayCommand::PopClip
                    | DisplayCommand::PushBlendMode { .. }
                    | DisplayCommand::PopBlendMode
                    | DisplayCommand::PushOpacity { .. }
                    | DisplayCommand::PopOpacity
            )
        });
        assert!(!any_layer_op);
    }

    #[test]
    fn ordered_clip_rect_overflow_hidden_clips_both_axes() {
        // overflow: hidden → PushClipRect clips padding-box on both axes.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: hidden; width: 200px; height: 100px; background: #f00; }",
        );
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("должен быть PushClipRect");
        assert!(rect.width > 0.0 && rect.height > 0.0);
    }

    #[test]
    fn ordered_clip_overflow_x_hidden_y_visible_coerces_to_both_clip() {
        // CSS Overflow L3 §3.1: overflow-y:visible paired with a non-visible
        // overflow-x coerces to `auto`. `auto` is a scroll container, so the
        // clip is established via PushScrollLayer; both axes are constrained to
        // the padding box (≈100×50), no unconstrained-axis sentinel. (BUG-020.)
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow-x: hidden; overflow-y: visible; width: 100px; height: 50px; background: #f00; }",
        );
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushScrollLayer { clip_rect, .. } => Some(*clip_rect),
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("должен быть clip-слой (PushScrollLayer) для overflow-x:hidden");
        // Both axes constrained to the box after visible→auto coercion.
        assert!(rect.width < 1_000.0, "x-axis should be clipped: width={}", rect.width);
        assert!(rect.height < 1_000.0, "y-axis should be clipped after coercion: height={}", rect.height);
    }

    #[test]
    fn ordered_clip_overflow_x_visible_y_hidden_coerces_to_both_clip() {
        // Symmetric: overflow-x:visible coerces to `auto` → both axes clip via
        // a scroll layer (the auto axis is a scroll container).
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow-x: visible; overflow-y: hidden; width: 100px; height: 50px; background: #f00; }",
        );
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushScrollLayer { clip_rect, .. } => Some(*clip_rect),
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("должен быть clip-слой (PushScrollLayer) для overflow-y:hidden");
        assert!(rect.height < 1_000.0, "y-axis should be clipped: height={}", rect.height);
        assert!(rect.width < 1_000.0, "x-axis should be clipped after coercion: width={}", rect.width);
    }

    #[test]
    fn ordered_empty_tree_produces_empty_list() {
        // Деградированный случай: StackingTree без contexts, layout —
        // пустая страница (одинокий root Block без детей и без bg/border).
        let doc = lumen_html_parser::parse("");
        let sheet = lumen_css_parser::parse("");
        let tree =
            lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        let (dl, provenance) = build_display_list_ordered(
            &tree,
            &lumen_layout::StackingTree { contexts: vec![] },
            &lumen_layout::PaintOrder::default(),
        );
        assert!(dl.is_empty(), "пустой PaintOrder → пустой display list");
        assert!(provenance.spans().is_empty(), "пустой display list → пустой provenance");
    }
