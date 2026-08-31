//! P1/SPLIT-DL4: хвост тестового модуля `mod tests` в `display_list.rs` —
//! ProvenanceIndex (DEVX-7)/outline/text-shadow/box-shadow/backdrop-filter/
//! background-clip/visibility:hidden/opacity:0/transform pipeline/
//! backface-visibility culling/3D depth sorting/build_display_list_with_anim.
//! Перенесено байт-в-байт из `display_list.rs` без дедента (приём ST-1/DL-1).
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-4).

use super::*;
// P1/SPLIT-DL5/DL-6 ещё не взяты: build/build_ordered/fills/count_variant/
// Fixed8 остаются в `mod tests` в display_list.rs — общие хелперы, ещё не
// вынесенные оттуда.
use super::tests::{build, build_ordered, count_variant, fills, Fixed8};

    // ───────── DEVX-7 п.4: ProvenanceIndex ─────────

    /// `DisplayCommand` must not grow — `ProvenanceIndex` is a side index
    /// specifically so the ~40-variant, rebuilt-every-frame enum stays
    /// untouched (ADR-025 §3, DEVX-7 DoD).
    #[test]
    fn display_command_size_unchanged() {
        // Captured baseline, not a magic number chosen to make the test pass:
        // this is `size_of::<DisplayCommand>()` on `main` before DEVX-7 п.4.
        // If this fails, a `DisplayCommand` variant grew — provenance must
        // stay a side index, not a field, so look there first.
        assert_eq!(std::mem::size_of::<DisplayCommand>(), 192);
    }

    fn find_box<'a>(b: &'a LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool) -> Option<&'a LayoutBox> {
        if pred(b) {
            return Some(b);
        }
        b.children.iter().find_map(|c| find_box(c, pred))
    }

    /// ADR-025 §1: identity is `(node, role)`, never `node` alone. Builds a
    /// page where two DOM elements each own a background (unambiguous
    /// `Element` origins), one element has only a `::before` (isolated
    /// `Pseudo` box, CSS default `display: inline` for `::before` folds it
    /// into a sibling `InlineRun` when one already exists — this element has
    /// none, so it does not), and the other has plain text (an
    /// `AnonymousInlineRun` box sharing the element's `NodeId` but not its
    /// role). `explain_element`/`ProvenanceIndex::spans_for` must not
    /// conflate any of these four origins.
    #[test]
    fn provenance_distinguishes_element_anon_and_pseudo_boxes() {
        let html = r#"
            <div id="outer" style="background:#008000"></div>
            <div id="inner" style="background:#ff0000">plain text</div>
        "#;
        let css = r#"#outer::before { content: "*"; color: #0000ff; }"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        let (dl, provenance) = build_display_list_ordered(&tree, &stacking_tree, &order);

        let green = Color { r: 0, g: 0x80, b: 0, a: 255 };
        let red = Color { r: 0xff, g: 0, b: 0, a: 255 };
        let bg = |c: Color| move |b: &LayoutBox| b.style.background_color.and_then(|x| x.to_color_opt()) == Some(c);
        let outer = find_box(&tree, &bg(green)).expect("outer div must have its own box");
        let inner = find_box(&tree, &bg(red)).expect("inner div must have its own box");
        let pseudo_before = find_box(&tree, &|b| matches!(b.origin.role, lumen_layout::BoxRole::Pseudo(_)))
            .expect("::before must produce an isolated Pseudo box (outer has no other inline content)");
        let anon_text = find_box(&tree, &|b| {
            b.origin.node == Some(inner.node) && matches!(b.origin.role, lumen_layout::BoxRole::AnonymousInlineRun)
        })
        .expect("plain text in a block div must produce an AnonymousInlineRun box");

        // Same NodeId, different role — the case ADR-025 §1 exists for.
        assert_eq!(inner.origin.node, anon_text.origin.node);
        assert_ne!(inner.origin, anon_text.origin);

        for (label, origin) in [
            ("outer element", outer.origin),
            ("inner element", inner.origin),
            ("::before pseudo", pseudo_before.origin),
            ("anonymous inline run", anon_text.origin),
        ] {
            let spans: Vec<_> = provenance.spans_for(origin).collect();
            assert!(!spans.is_empty(), "{label} must own at least one span");
            for s in &spans {
                assert!(s.range.start < s.range.end, "{label}: span must not be empty");
                assert!(s.range.end <= dl.len(), "{label}: span must index into the emitted list");
            }
        }

        // No two of the four distinct origins may share a span (ADR-025 §4:
        // "every emitted command falls inside exactly one span").
        let origins = [outer.origin, inner.origin, pseudo_before.origin, anon_text.origin];
        for i in 0..origins.len() {
            for j in (i + 1)..origins.len() {
                let a: Vec<_> = provenance.spans_for(origins[i]).map(|s| s.range.clone()).collect();
                let b: Vec<_> = provenance.spans_for(origins[j]).map(|s| s.range.clone()).collect();
                for ra in &a {
                    for rb in &b {
                        assert!(
                            ra.end <= rb.start || rb.end <= ra.start,
                            "spans of distinct origins must not overlap: {ra:?} vs {rb:?}"
                        );
                    }
                }
            }
        }
    }

    // ───────── outline rendering ─────────

    fn outlines(dl: &DisplayList) -> Vec<(&Color, f32, f32, OutlineStyle)> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawOutline { color, width, offset, style, .. } => {
                    Some((color, *width, *offset, *style))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn outline_solid_emits_draw_outline() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 2px solid red; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1, "ровно одна DrawOutline на div");
        let (color, width, offset, style) = o[0];
        assert_eq!(color.r, 255);
        assert!((width - 2.0).abs() < 0.01);
        assert!((offset - 0.0).abs() < 0.01);
        assert_eq!(style, OutlineStyle::Solid);
    }

    #[test]
    fn outline_none_emits_nothing() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 2px none red; }",
        );
        assert!(outlines(&dl).is_empty(), "outline:none → no DrawOutline");
    }

    #[test]
    fn outline_zero_width_emits_nothing() {
        // outline-width: 0 → invisible (CSS Basic UI L4 §5.1).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 0 solid red; }",
        );
        assert!(outlines(&dl).is_empty(), "outline-width:0 → no DrawOutline");
    }

    #[test]
    fn outline_offset_is_preserved() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; \
             outline: 2px solid red; outline-offset: 5px; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1);
        assert!((o[0].2 - 5.0).abs() < 0.01, "offset=5px должен сохраниться");
    }

    #[test]
    fn outline_color_currentcolor_resolves_to_text_color() {
        // currentColor → CSS color (Phase 0 reduces Auto/CurrentColor to color).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; color: rgb(10, 20, 30); \
             outline: 2px solid currentColor; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1);
        let (color, _, _, _) = o[0];
        assert_eq!((color.r, color.g, color.b), (10, 20, 30));
    }

    #[test]
    fn outline_after_children_in_walk() {
        // Outline parent-а должен идти ПОСЛЕ background ребёнка — иначе при
        // негативном outline-offset (Phase 2) outline парента закрывался бы
        // содержимым ребёнка. Phase 0 проверка ordering: DrawOutline
        // последняя из своего box-а.
        let dl = build(
            "<div><p></p></div>",
            "div { width: 100px; height: 50px; outline: 2px solid red; } \
             p { display: block; background: blue; width: 30px; height: 10px; }",
        );
        let outline_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawOutline { .. }))
            .expect("должна быть DrawOutline");
        // FillRect ребёнка (background: blue) должен идти раньше DrawOutline.
        let child_bg_idx = dl
            .iter()
            .enumerate()
            .find(|(_, c)| matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255))
            .map(|(i, _)| i)
            .expect("должен быть синий FillRect ребёнка");
        assert!(
            child_bg_idx < outline_idx,
            "outline (idx {outline_idx}) должен идти после child background (idx {child_bg_idx})"
        );
    }

    #[test]
    fn outline_serializes_with_short_offset_only_when_nonzero() {
        // DrawOutline с offset=0 не выводит `off=…` в сериализацию (как
        // DrawText опускает default-значения).
        let dl = vec![DisplayCommand::DrawOutline {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            width: 2.0,
            style: OutlineStyle::Solid,
            color: Color { r: 255, g: 0, b: 0, a: 255 },
            offset: 0.0,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawOutline (0.00, 0.00, 100.00, 50.00) w=2.00 s=solid #ff0000ff"));
        assert!(!s.contains("off="));

        // Non-zero offset → должен присутствовать.
        let dl2 = vec![DisplayCommand::DrawOutline {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            width: 2.0,
            style: OutlineStyle::Solid,
            color: Color { r: 255, g: 0, b: 0, a: 255 },
            offset: 5.0,
        }];
        let s2 = serialize_display_list(&dl2);
        assert!(s2.contains("off=5.00"));
    }

    // ───────── text-shadow rendering ─────────

    fn texts_with_colors(dl: &DisplayList) -> Vec<(String, [u8; 3])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, color, .. } => {
                    Some((text.clone(), [color.r, color.g, color.b]))
                }
                _ => None,
            })
            .collect()
    }

    fn text_rects(dl: &DisplayList) -> Vec<(String, [f32; 2])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, rect, .. } => {
                    Some((text.clone(), [rect.x, rect.y]))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn text_shadow_none_emits_only_main_text() {
        // Без text-shadow — ровно один DrawText на фрагмент (как раньше).
        let dl = build("<p>hello</p>", "p { color: black; }");
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].0, "hello");
    }

    #[test]
    fn text_shadow_one_emits_shadow_before_main() {
        // Один text-shadow → 2 DrawText: сначала shadow, потом main.
        // Spec painter's order: shadow рисуется ПОД основным текстом.
        let dl = build(
            "<p>hi</p>",
            "p { color: black; text-shadow: 2px 3px red; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 2, "shadow + main = 2 DrawText");
        // Painter's order: shadow первый (под main), main второй (поверх).
        assert_eq!(texts[0].1, [255, 0, 0], "первый = красная тень");
        assert_eq!(texts[1].1, [0, 0, 0], "второй = чёрный основной");
        // Тень смещена на (2, 3) px относительно main.
        let rects = text_rects(&dl);
        let dx = rects[0].1[0] - rects[1].1[0];
        let dy = rects[0].1[1] - rects[1].1[1];
        assert!((dx - 2.0).abs() < 0.01, "shadow_x смещён на 2px, got {dx}");
        assert!((dy - 3.0).abs() < 0.01, "shadow_y смещён на 3px, got {dy}");
    }

    #[test]
    fn text_shadow_multiple_reverse_order() {
        // Spec L3 §6: «first shadow is on top, subsequent shadows are
        // layered behind it». Значит painter's order: последняя в списке
        // рисуется первой (под всеми), первая — последней (над всеми, но
        // под main). Список: red(1px), green(2px), blue(3px) — порядок
        // эмиссии: blue → green → red → main.
        let dl = build(
            "<p>z</p>",
            "p { color: black; \
             text-shadow: 1px 0 red, 2px 0 green, 3px 0 blue; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 4, "3 shadows + main = 4 DrawText");
        assert_eq!(texts[0].1, [0, 0, 255], "blue painted first (deepest)");
        assert_eq!(texts[1].1, [0, 128, 0], "green painted second");
        assert_eq!(texts[2].1, [255, 0, 0], "red painted third");
        assert_eq!(texts[3].1, [0, 0, 0], "main painted last (top)");
    }

    #[test]
    fn text_shadow_color_omitted_uses_currentcolor() {
        // CSS Text Decoration L3 §6: «If <color> is not specified, the
        // value used for color (currentColor) is used.»
        let dl = build(
            "<p>x</p>",
            "p { color: rgb(10, 20, 30); text-shadow: 1px 1px; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 2);
        // Shadow color = currentColor = (10, 20, 30).
        assert_eq!(texts[0].1, [10, 20, 30]);
        assert_eq!(texts[1].1, [10, 20, 30]);
    }

    #[test]
    fn text_shadow_blur_wraps_in_push_filter() {
        // blur > 0 → DrawText завёрнут в PushFilter{Blur(sigma)} / PopFilter.
        // sigma = blur / 2.0 (то же соглашение, что box-shadow).
        // text-shadow: 2px 3px 8px red  →  sigma = 4.0
        let dl = build(
            "<p>hi</p>",
            "p { text-shadow: 2px 3px 8px red; }",
        );
        let push_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::PushFilter { filters, .. }
                if matches!(filters.as_slice(), [FilterFn::Blur(s)] if (*s - 4.0).abs() < 0.01))
        });
        assert!(push_idx.is_some(), "PushFilter{{Blur(4.0)}} должен быть в DL, got {dl:?}");
        let push_idx = push_idx.unwrap();
        // Сразу после PushFilter → DrawText тени.
        assert!(
            matches!(dl[push_idx + 1], DisplayCommand::DrawText { .. }),
            "после PushFilter ожидается DrawText"
        );
        // За DrawText тени → PopFilter.
        assert!(
            matches!(dl[push_idx + 2], DisplayCommand::PopFilter),
            "после DrawText тени ожидается PopFilter"
        );
    }

    #[test]
    fn text_shadow_no_blur_no_filter_wrap() {
        // blur == 0 → DrawText тени без PushFilter/PopFilter.
        let dl = build(
            "<p>x</p>",
            "p { text-shadow: 2px 3px red; }",
        );
        let has_filter = dl.iter().any(|c| {
            matches!(c, DisplayCommand::PushFilter { filters, .. }
                if filters.iter().any(|f| matches!(f, FilterFn::Blur(_))))
        });
        assert!(!has_filter, "без blur не должно быть PushFilter, got {dl:?}");
        // Но DrawText тени должен быть.
        let shadow_draw = dl.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).count();
        assert!(shadow_draw >= 2, "должно быть ≥2 DrawText (тень + основной)");
    }

    #[test]
    fn text_shadow_blur_multiple_each_wrapped() {
        // Два text-shadow с blur > 0 — каждый получает свой PushFilter/PopFilter.
        let dl = build(
            "<p>z</p>",
            "p { text-shadow: 1px 1px 6px red, 2px 2px 4px blue; }",
        );
        let push_count = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::PushFilter { filters, .. }
                if filters.iter().any(|f| matches!(f, FilterFn::Blur(_))))
        }).count();
        assert_eq!(push_count, 2, "два PushFilter для двух shadow с blur, got {dl:?}");
    }

    #[test]
    fn text_shadow_blur_sigma_is_half_radius_for_test52_progression() {
        // BUG-191: TEST-52 row 1 — одна и та же тень с blur-radius 0/4/10/20px.
        // Закрепляет соглашение sigma = blur-radius / 2 (CSS Text Decoration L3 §6,
        // как у box-shadow и canvas shadowBlur — стандартное отклонение Gaussian'а
        // равно половине blur-radius). Расследование BUG-191 показало, что blur
        // рендерится корректно (extent/intensity совпадают с Edge в glow-only и
        // 20px-кейсах); остаток diff TEST-52 = font-parity (Edge serif vs Inter sans),
        // KNOWN_DEBTORS BUG-128, а не дефект blur-пайплайна.
        for (radius, expect_sigma) in [(4.0_f32, 2.0_f32), (10.0, 5.0), (20.0, 10.0)] {
            let css = format!("p {{ text-shadow: 6px 6px {radius}px red; }}");
            let dl = build("<p>A</p>", &css);
            let sigma = dl.iter().find_map(|c| match c {
                DisplayCommand::PushFilter { filters, .. } => filters.iter().find_map(|f| {
                    if let FilterFn::Blur(s) = f { Some(*s) } else { None }
                }),
                _ => None,
            });
            assert!(
                matches!(sigma, Some(s) if (s - expect_sigma).abs() < 0.01),
                "blur {radius}px → sigma {expect_sigma}, got {sigma:?}"
            );
        }
        // blur 0 → нет PushFilter{Blur} (резкая тень рисуется напрямую).
        let dl0 = build("<p>A</p>", "p { text-shadow: 6px 6px 0px red; }");
        assert!(
            !dl0.iter().any(|c| matches!(c,
                DisplayCommand::PushFilter { filters, .. }
                    if filters.iter().any(|f| matches!(f, FilterFn::Blur(_))))),
            "blur 0px не должен заворачиваться в PushFilter, got {dl0:?}"
        );
    }

    // ───────── box-shadow rendering ─────────

    fn fills_with_color(dl: &DisplayList) -> Vec<(Rect, [u8; 4])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { rect, color } => {
                    Some((*rect, [color.r, color.g, color.b, color.a]))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn box_shadow_none_emits_no_extra_fill() {
        // Без box-shadow div с background даёт ровно одну FillRect.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1, [255, 0, 0, 255]);
    }

    #[test]
    fn box_shadow_outset_emits_fill_before_background() {
        // Outset shadow → 2 FillRect: сначала shadow (под bg), потом bg.
        // shadow смещена на (3, 5) px.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 3px 5px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        // Painter's order: shadow первый (под bg).
        assert_eq!(fills[0].1, [0, 0, 0, 255], "shadow первой");
        assert_eq!(fills[1].1, [255, 255, 255, 255], "background второй");
        // shadow смещена на (3, 5).
        let dx = fills[0].0.x - fills[1].0.x;
        let dy = fills[0].0.y - fills[1].0.y;
        assert!((dx - 3.0).abs() < 0.01);
        assert!((dy - 5.0).abs() < 0.01);
        // Размер shadow совпадает с box (spread=0).
        assert!((fills[0].0.width - fills[1].0.width).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_offset_emits_frame() {
        // offset (3, 5) внутри 100×50 без border / spread:
        // outer = padding-box = (0..100, 0..50).
        // inner = (3..103, 5..55) — частично за outer.
        // hole = inner ∩ outer = (3..100, 5..50).
        // Тень = 4 кольцевых рамки; нулевая bottom (50..50) и right (100..100)
        // skip-ятся. Остаются top (0..5) + left (0..3 на полосе 5..50).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             box-shadow: inset 3px 5px black; }",
        );
        let fills = fills_with_color(&dl);
        // bg + top frame + left frame = 3.
        assert_eq!(fills.len(), 3);
        // Painter's order: bg первый, inset тени поверх.
        assert_eq!(fills[0].1, [255, 0, 0, 255], "bg = red");
        // Top frame: x=0, y=0, w=100, h=5.
        assert_eq!(fills[1].1[..3], [0, 0, 0], "frame = black");
        let top = fills[1].0;
        assert!((top.x - 0.0).abs() < 0.01);
        assert!((top.y - 0.0).abs() < 0.01);
        assert!((top.width - 100.0).abs() < 0.01);
        assert!((top.height - 5.0).abs() < 0.01);
        // Left frame: x=0, y=5, w=3, h=45.
        let left = fills[2].0;
        assert!((left.x - 0.0).abs() < 0.01);
        assert!((left.y - 5.0).abs() < 0.01);
        assert!((left.width - 3.0).abs() < 0.01);
        assert!((left.height - 45.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_spread_only_emits_four_frames() {
        // Только spread, без offset: inner симметрично сжат на 10px →
        // hole = (10..90, 10..40). Все 4 рамки видимы.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 10px black; }",
        );
        let fills = fills_with_color(&dl);
        // bg + 4 frames.
        assert_eq!(fills.len(), 5);
        assert_eq!(fills[0].1, [255, 255, 255, 255], "bg = white");
        // Все 4 рамки = black.
        for fill in &fills[1..] {
            assert_eq!(fill.1[..3], [0, 0, 0]);
        }
        // Top (0, 0, 100, 10).
        let top = fills[1].0;
        assert!((top.height - 10.0).abs() < 0.01);
        // Bottom (0, 40, 100, 10).
        let bottom = fills[2].0;
        assert!((bottom.y - 40.0).abs() < 0.01);
        assert!((bottom.height - 10.0).abs() < 0.01);
        // Left (0, 10, 10, 30).
        let left = fills[3].0;
        assert!((left.x - 0.0).abs() < 0.01);
        assert!((left.width - 10.0).abs() < 0.01);
        assert!((left.height - 30.0).abs() < 0.01);
        // Right (90, 10, 10, 30).
        let right = fills[4].0;
        assert!((right.x - 90.0).abs() < 0.01);
        assert!((right.width - 10.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_large_offset_fills_whole_outer() {
        // offset_x=200 при width=100 → inner полностью справа от outer.
        // no_overlap → один FillRect, покрывающий весь padding-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 200px 0 black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2, "bg + single full-outer shadow");
        assert_eq!(fills[1].1[..3], [0, 0, 0]);
        let shadow = fills[1].0;
        assert!((shadow.width - 100.0).abs() < 0.01);
        assert!((shadow.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_negative_spread_covers_outer_skips() {
        // Отрицательный spread с большим модулем — inner полностью покрывает
        // outer (расширен наружу с каждой стороны). Тени не видно.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 -100px black; }",
        );
        let fills = fills_with_color(&dl);
        // Только bg.
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1[..3], [255, 255, 255]);
    }

    #[test]
    fn box_shadow_inset_uses_padding_box_when_border_present() {
        // box-sizing: border-box + 100×50 + border:5px → padding-box =
        // (5, 5, 90, 40). offset 0,0 + spread 5 → inner = (10, 10, 80, 30)
        // внутри padding-box. Все 4 frames лежат строго в padding-box.
        let dl = build(
            "<div></div>",
            "div { box-sizing: border-box; width: 100px; height: 50px; \
             background: white; border: 5px solid green; \
             box-shadow: inset 0 0 0 5px black; }",
        );
        let fills = fills_with_color(&dl);
        // 4 inset frames + bg + (possibly border fills через DrawBorder; они
        // не попадают в fills_with_color — DrawBorder отдельный command).
        let shadow_fills: Vec<_> = fills
            .iter()
            .filter(|(_, c)| c[..3] == [0, 0, 0])
            .collect();
        assert_eq!(shadow_fills.len(), 4, "border-aware padding-box → 4 frames");
        // Все рамки лежат внутри padding-box: x in [5..95], y in [5..45].
        for (rect, _) in &shadow_fills {
            assert!(rect.x >= 5.0 - 0.01, "left edge inside padding-box: {}", rect.x);
            assert!(
                rect.x + rect.width <= 95.0 + 0.01,
                "right edge inside padding-box: {}",
                rect.x + rect.width
            );
            assert!(rect.y >= 5.0 - 0.01, "top edge inside padding-box: {}", rect.y);
            assert!(
                rect.y + rect.height <= 45.0 + 0.01,
                "bottom edge inside padding-box: {}",
                rect.y + rect.height
            );
        }
    }

    #[test]
    fn box_shadow_inset_currentcolor_fallback() {
        // CSS Backgrounds L3 §4.6 — отсутствующий color = currentColor.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; color: blue; \
             box-shadow: inset 0 0 0 10px; }",
        );
        let fills = fills_with_color(&dl);
        // 4 inset frames (без bg).
        assert_eq!(fills.len(), 4);
        for fill in &fills {
            assert_eq!(fill.1[..3], [0, 0, 255], "frame = currentColor (blue)");
        }
    }

    #[test]
    fn box_shadow_inset_multiple_reverse_order() {
        // Spec: «first shadow is on top» — последний inset эмитим первым,
        // первый — последним (поверх всех).
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 5px red, inset 0 0 0 10px green, inset 0 0 0 15px blue; }",
        );
        let fills = fills_with_color(&dl);
        // bg + 3 inset × 4 frames = 1 + 12 = 13. Но frames с w=0 / h=0
        // skip-ятся; spread > 0 всегда даёт все 4 frames.
        assert_eq!(fills.len(), 13);
        assert_eq!(fills[0].1[..3], [255, 255, 255], "bg first");
        // Дальше — blue (последний CSS-shadow рисуется первым).
        for fill in &fills[1..5] {
            assert_eq!(fill.1[..3], [0, 0, 255]);
        }
        for fill in &fills[5..9] {
            assert_eq!(fill.1[..3], [0, 128, 0]);
        }
        // red — поверх всех (первый CSS-shadow рисуется последним).
        for fill in &fills[9..13] {
            assert_eq!(fill.1[..3], [255, 0, 0]);
        }
    }

    #[test]
    fn box_shadow_inset_and_outset_coexist() {
        // Одна inset и одна outset — outset перед bg, inset после bg.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px red, inset 0 0 0 5px blue; }",
        );
        let fills = fills_with_color(&dl);
        // outset (1) + bg (1) + inset (4 frames) = 6.
        assert_eq!(fills.len(), 6);
        assert_eq!(fills[0].1[..3], [255, 0, 0], "outset red first");
        assert_eq!(fills[1].1[..3], [255, 255, 255], "bg second");
        for fill in &fills[2..6] {
            assert_eq!(fill.1[..3], [0, 0, 255], "inset blue frames");
        }
    }

    #[test]
    fn box_shadow_inset_transparent_color_skipped() {
        // a=0 — shadow невидим, не эмитим.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             box-shadow: inset 0 0 0 10px rgba(0,0,0,0); }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1, "transparent inset shadow skipped");
        assert_eq!(fills[0].1[..3], [255, 0, 0]);
    }

    #[test]
    fn box_shadow_spread_expands_rect() {
        // spread=10 → shadow rect расширен на 10px по всем сторонам.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 0 0 0 10px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        let shadow_rect = fills[0].0;
        let bg_rect = fills[1].0;
        // shadow расширен на 10 по всем сторонам.
        assert!((shadow_rect.width - bg_rect.width - 20.0).abs() < 0.01);
        assert!((shadow_rect.height - bg_rect.height - 20.0).abs() < 0.01);
        assert!((shadow_rect.x - bg_rect.x + 10.0).abs() < 0.01);
        assert!((shadow_rect.y - bg_rect.y + 10.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_multiple_reverse_order() {
        // Spec: «first shadow is on top». Painter's order: последняя
        // shadow рисуется первой (ниже всех), первая — последней-перед-bg.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: white; \
             box-shadow: 1px 0 red, 2px 0 green, 3px 0 blue; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 4, "3 shadows + bg = 4 FillRect");
        assert_eq!(fills[0].1[..3], [0, 0, 255]); // blue первой (ниже всех)
        assert_eq!(fills[1].1[..3], [0, 128, 0]); // green
        assert_eq!(fills[2].1[..3], [255, 0, 0]); // red (поверх теней)
        assert_eq!(fills[3].1[..3], [255, 255, 255]); // bg (поверх всего)
    }

    #[test]
    fn box_shadow_color_omitted_uses_currentcolor() {
        // CSS Backgrounds L3 §4.6 — «If no color is specified, the value
        // of the color property is used».
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             color: rgb(10, 20, 30); box-shadow: 2px 2px; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].1[..3], [10, 20, 30]);
    }

    #[test]
    fn box_shadow_negative_spread_collapses_to_skip() {
        // spread=-100 на box 50×50 → final w/h = -150, отрицательный
        // → пропускаем (не эмитим бессмысленный FillRect).
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: red; \
             box-shadow: 0 0 0 -100px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1, "collapsed shadow пропускается");
    }

    #[test]
    fn box_shadow_transparent_color_skipped() {
        // a == 0 → нечего рисовать.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: red; \
             box-shadow: 5px 5px transparent; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1);
    }

    #[test]
    fn box_shadow_blur_wraps_in_push_filter() {
        // blur > 0 → FillRect завёрнут в PushFilter { Blur(sigma) } / PopFilter.
        // sigma = blur / 2 = 10.0.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px 20px black; }",
        );
        // 2 FillRect: shadow + bg (PushFilter/PopFilter не считаются fills).
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        // Размер shadow rect совпадает с box (spread=0), blur не меняет rect.
        assert!((fills[0].0.width - fills[1].0.width).abs() < 0.01);
        assert!((fills[0].0.height - fills[1].0.height).abs() < 0.01);
        // Структура: PushFilter, FillRect(shadow), PopFilter, FillRect(bg), ...
        let first = dl.first().unwrap();
        assert!(
            matches!(first, DisplayCommand::PushFilter { filters, .. }
                if matches!(filters.as_slice(), [FilterFn::Blur(s)] if (*s - 10.0).abs() < 0.01)),
            "PushFilter с Blur(10.0) перед shadow FillRect, got {first:?}"
        );
        let second = dl.get(1).unwrap();
        assert!(
            matches!(second, DisplayCommand::FillRect { color, .. } if color.r == 0),
            "shadow FillRect (black) после PushFilter"
        );
        let third = dl.get(2).unwrap();
        assert!(
            matches!(third, DisplayCommand::PopFilter),
            "PopFilter после shadow FillRect"
        );
    }

    #[test]
    fn box_shadow_no_blur_no_filter_wrap() {
        // blur == 0 → прямой FillRect без PushFilter/PopFilter.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px black; }",
        );
        let first = dl.first().unwrap();
        assert!(
            matches!(first, DisplayCommand::FillRect { .. }),
            "без blur первая команда — FillRect, не PushFilter"
        );
    }

    #[test]
    fn box_shadow_on_rounded_box_emits_rounded_shadow() {
        // BUG-138: a hard shadow on a border-radius box must follow the rounded
        // contour — the shadow is a FillRoundedRect (radii = box radii + spread),
        // not a square FillRect.
        let dl = build(
            "<div></div>",
            "div { width: 180px; height: 180px; background: blue; \
             border-radius: 40px; box-shadow: 20px 20px 0 black; }",
        );
        // First command is the shadow (painter's order: shadow before bg).
        let first = dl.first().unwrap();
        match first {
            DisplayCommand::FillRoundedRect { radii, color, .. } => {
                assert_eq!(color.r, 0, "shadow color black");
                assert!((radii.tl - 40.0).abs() < 0.01, "spread=0 → radius == box radius 40, got {}", radii.tl);
                assert!((radii.br - 40.0).abs() < 0.01);
            }
            other => panic!("rounded box shadow must be FillRoundedRect, got {other:?}"),
        }
    }

    #[test]
    fn box_shadow_spread_increases_corner_radius() {
        // CSS Backgrounds L3 §7.1.1: spread expands each non-zero corner radius
        // by the spread distance. Box 160×160, border-radius:50% (=80), spread 24
        // → shadow corner radius 80+24 = 104.
        let dl = build(
            "<div></div>",
            "div { width: 160px; height: 160px; background: yellow; \
             border-radius: 50%; box-shadow: 0 0 0 24px black; }",
        );
        let first = dl.first().unwrap();
        match first {
            DisplayCommand::FillRoundedRect { radii, rect, .. } => {
                assert!((radii.tl - 104.0).abs() < 0.01, "radius 80+24=104, got {}", radii.tl);
                // Shadow rect is 160+2*24 = 208 → radius 104 == half → perfect circle.
                assert!((rect.width - 208.0).abs() < 0.01);
            }
            other => panic!("spread shadow on circle must be FillRoundedRect, got {other:?}"),
        }
    }

    #[test]
    fn box_shadow_square_box_stays_fillrect() {
        // No border-radius → shadow remains a square FillRect (no regression).
        let dl = build(
            "<div></div>",
            "div { width: 160px; height: 160px; background: green; \
             box-shadow: 30px 30px 0 red; }",
        );
        let first = dl.first().unwrap();
        assert!(
            matches!(first, DisplayCommand::FillRect { .. }),
            "square box shadow stays FillRect, got {first:?}"
        );
    }

    // ───────── backdrop-filter display list ─────────

    #[test]
    fn backdrop_filter_emits_push_pop_commands() {
        let dl = build_ordered(
            "<div></div>",
            "div { width: 100px; height: 100px; backdrop-filter: blur(8px); }",
        );
        let has_push = dl.iter().any(|c| {
            matches!(c, DisplayCommand::PushBackdropFilter { filters, .. }
                if matches!(filters.as_slice(), [FilterFn::Blur(s)] if (*s - 8.0).abs() < 0.01))
        });
        assert!(has_push, "PushBackdropFilter(Blur(8)) должен быть в DL, got {dl:?}");
        let has_pop = dl.iter().any(|c| matches!(c, DisplayCommand::PopBackdropFilter));
        assert!(has_pop, "PopBackdropFilter должен быть в DL");
    }

    #[test]
    fn backdrop_filter_bounds_match_element_rect() {
        let dl = build_ordered(
            "<div></div>",
            "div { width: 200px; height: 100px; backdrop-filter: grayscale(1); }",
        );
        let push = dl.iter().find_map(|c| match c {
            DisplayCommand::PushBackdropFilter { bounds, .. } => Some(*bounds),
            _ => None,
        });
        let b = push.expect("PushBackdropFilter должен быть");
        assert!((b.width - 200.0).abs() < 0.01, "bounds.width = {}", b.width);
        assert!((b.height - 100.0).abs() < 0.01, "bounds.height = {}", b.height);
    }

    #[test]
    fn backdrop_filter_chain_parsed_correctly() {
        let dl = build_ordered(
            "<div></div>",
            "div { width: 50px; height: 50px; backdrop-filter: blur(4px) brightness(0.8); }",
        );
        let filters = dl.iter().find_map(|c| match c {
            DisplayCommand::PushBackdropFilter { filters, .. } => Some(filters.clone()),
            _ => None,
        }).expect("PushBackdropFilter");
        assert_eq!(filters.len(), 2);
        assert!(matches!(filters[0], FilterFn::Blur(_)));
        assert!(matches!(filters[1], FilterFn::Brightness(_)));
    }

    #[test]
    fn backdrop_filter_and_filter_both_emit() {
        // When both filter and backdrop-filter are set, both Push commands appear.
        let dl = build_ordered(
            "<div></div>",
            "div { width: 50px; height: 50px; filter: invert(1); backdrop-filter: blur(6px); }",
        );
        let has_bf = dl.iter().any(|c| matches!(c, DisplayCommand::PushBackdropFilter { .. }));
        let has_f = dl.iter().any(|c| matches!(c, DisplayCommand::PushFilter { .. }));
        assert!(has_bf, "PushBackdropFilter должен быть");
        assert!(has_f, "PushFilter должен быть");
    }

    // ───────── background-clip rendering ─────────

    fn first_bg_rect(dl: &DisplayList) -> Rect {
        dl.iter()
            .find_map(|c| match c {
                // bg = single non-shadow FillRect: ищем по цвету ≠ pre-shadow
                DisplayCommand::FillRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("должна быть хотя бы одна FillRect")
    }

    #[test]
    fn background_clip_border_box_default_uses_full_rect() {
        // BorderBox initial: bg рисуется на полный b.rect.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; }",
        );
        let bg = first_bg_rect(&dl);
        // box-sizing: content-box default → внешний размер = 100 + 2*20 + 2*5 = 150.
        assert!((bg.width - 150.0).abs() < 0.01);
        assert!((bg.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn background_clip_padding_box_shrinks_by_border() {
        // PaddingBox: bg ужимается на border (по 5px со всех сторон).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; \
             background-clip: padding-box; }",
        );
        let bg = first_bg_rect(&dl);
        // padding-box = border-box minus 2*5 border = 150 - 10 = 140.
        assert!((bg.width - 140.0).abs() < 0.01, "got width {}", bg.width);
        assert!((bg.height - 90.0).abs() < 0.01, "got height {}", bg.height);
        // Сдвиг по x на левый border (+5).
        assert!((bg.x - 5.0).abs() < 0.01, "got x {}", bg.x);
    }

    #[test]
    fn background_clip_content_box_shrinks_by_border_plus_padding() {
        // ContentBox: bg ужимается на border + padding.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; \
             background-clip: content-box; }",
        );
        let bg = first_bg_rect(&dl);
        // content-box = border-box minus 2*(5+20) = 150 - 50 = 100.
        assert!((bg.width - 100.0).abs() < 0.01, "got width {}", bg.width);
        assert!((bg.height - 50.0).abs() < 0.01, "got height {}", bg.height);
        // Сдвиг по x = border + padding = 5 + 20 = 25.
        assert!((bg.x - 25.0).abs() < 0.01, "got x {}", bg.x);
    }

    #[test]
    fn background_clip_text_falls_back_to_border_box_phase0() {
        // Phase 0 без glyph-mask: text-clip эмитим как border-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             background-clip: text; }",
        );
        let bg = first_bg_rect(&dl);
        assert!((bg.width - 100.0).abs() < 0.01);
        assert!((bg.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn background_clip_collapsed_rect_skipped() {
        // Если border + padding больше box-а → clip rect collapses to 0 → skip.
        // box-sizing:border-box + width:50 + border:30 → content = 50 - 60 = -10,
        // max(0) → 0 → FillRect bg не эмитится.
        let dl = build(
            "<div></div>",
            "div { box-sizing: border-box; width: 50px; height: 20px; \
             border: 30px solid black; \
             background: red; background-clip: content-box; }",
        );
        let bg_fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert!(bg_fills.is_empty(), "collapsed bg должен быть пропущен");
    }

    // ───────── visibility: hidden ─────────

    fn cmd_count(dl: &DisplayList) -> usize {
        dl.iter()
            .filter(|c| !matches!(c, DisplayCommand::PushClipRect { .. }
                                  | DisplayCommand::PopClip
                                  | DisplayCommand::PushOpacity { .. }
                                  | DisplayCommand::PopOpacity
                                  | DisplayCommand::PushBlendMode { .. }
                                  | DisplayCommand::PopBlendMode))
            .count()
    }

    #[test]
    fn visibility_hidden_block_suppresses_self_paint() {
        let visible = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; border: 2px solid black; }",
        );
        let hidden = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; border: 2px solid black; \
             visibility: hidden; }",
        );
        // visible: FillRect (bg) + DrawBorder.
        assert!(cmd_count(&visible) >= 2);
        // hidden: ничего из self не эмитим (никаких children → пусто).
        assert_eq!(cmd_count(&hidden), 0);
    }

    #[test]
    fn visibility_hidden_block_still_walks_visible_children() {
        // Parent hidden, child явно visible (override через inherit).
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; visibility: hidden; } \
             p { display: block; background: blue; visibility: visible; \
                 width: 20px; height: 10px; }",
        );
        // Должна быть синяя FillRect от child, но не красная от parent.
        let blues = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255)
        });
        let reds = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255 && color.b == 0)
        });
        assert!(blues.count() >= 1, "child должен рисоваться");
        assert_eq!(reds.count(), 0, "parent bg не рисуется");
    }

    #[test]
    fn visibility_hidden_skips_text() {
        // text inherits visibility=hidden → DrawText не эмитим.
        let dl = build(
            "<p>hello</p>",
            "p { visibility: hidden; color: black; }",
        );
        let texts: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(texts.is_empty(), "hidden parent → text не эмитим");
    }

    // Note: inline visibility override (parent hidden + child <span>
    // visibility:visible) зависит от того, что layout формирует отдельный
    // InlineFrag со style от span. Тест на это случае отложен — текущее
    // layout-поведение может склеивать text-nodes в один frag со
    // стилем родителя. Когда P1 разделит inline-fragments по style-runs,
    // добавим этот test обратно.

    #[test]
    fn visibility_collapse_treated_as_hidden_outside_table() {
        // CSS L3 §4: vne table-row `collapse` ведёт себя как `hidden`.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; \
             visibility: collapse; }",
        );
        let bg_fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert!(bg_fills.is_empty(), "collapse вне table → hidden");
    }

    #[test]
    fn visibility_hidden_image_skipped() {
        // visibility:hidden на `<img>` — DrawImage не эмитим.
        let dl = build(
            r#"<img src="x.png" width="50" height="50">"#,
            "img { visibility: hidden; }",
        );
        let images: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }))
            .collect();
        assert!(images.is_empty());
    }

    // ───────── opacity:0 skip ─────────

    #[test]
    fn opacity_zero_skips_block_and_subtree() {
        // opacity:0 на parent → ни parent, ни children не рисуются.
        let dl = build(
            "<div><p>x</p></div>",
            "div { opacity: 0; background: red; } \
             p { display: block; background: blue; width: 20px; height: 10px; }",
        );
        let fills_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .count();
        assert_eq!(fills_count, 0, "opacity:0 → whole subtree skipped");
    }

    #[test]
    fn opacity_zero_skips_text() {
        let dl = build(
            "<p>hello</p>",
            "p { opacity: 0; }",
        );
        let texts: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(texts.is_empty(), "opacity:0 → text skipped");
    }

    #[test]
    fn opacity_one_renders_normally() {
        // Sanity: opacity:1 default — всё рисуется.
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; } \
             p { display: block; background: blue; width: 20px; height: 10px; }",
        );
        let reds = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255 && color.b == 0)
        });
        let blues = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255 && color.r == 0)
        });
        assert!(reds.count() >= 1);
        assert!(blues.count() >= 1);
    }

    #[test]
    fn opacity_half_phase0_does_not_change_emission() {
        // Phase 0: opacity > 0 && < 1 не обрабатывается; FillRect эмитим
        // с original color без модификации (true compositing — P2 п.4+).
        let dl = build(
            "<div></div>",
            "div { background: red; opacity: 0.5; width: 50px; height: 30px; }",
        );
        let reds: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert_eq!(reds.len(), 1, "opacity:0.5 не skip-аем; alpha не множим в Phase 0");
    }

    #[test]
    fn opacity_zero_image_subtree_skipped() {
        let dl = build(
            r#"<img src="x.png" width="50" height="50">"#,
            "img { opacity: 0; }",
        );
        let any: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }
                                  | DisplayCommand::FillRect { .. }
                                  | DisplayCommand::DrawBorder { .. }))
            .collect();
        assert!(any.is_empty());
    }

    // ── transform pipeline (P2) ────────────────────────────────────────────

    #[test]
    fn transform_none_emits_no_push() {
        let dl = build("<div>x</div>", "div { background: #f00; }");
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. })),
            0,
        );
    }

    #[test]
    fn transform_translate_emits_push_pop_pair() {
        let dl = build(
            r#"<div style="background: red; transform: translate(10px, 20px);">x</div>"#,
            "",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopTransform));
        assert_eq!(pushes, 1);
        assert_eq!(pops, 1);
    }

    #[test]
    fn transform_translate_matrix_has_expected_offsets() {
        // translate(50px, 70px) с default transform-origin (Phase 0 — (0,0)):
        // matrix = T(0,0)·T(50,70)·T(-0,-0) = T(50,70).
        // 2D affine: x'=x+50, y'=y+70 → (a,b,c,d,e,f) = (1,0,0,1,50,70).
        let dl = build(
            r#"<div style="background: red; transform: translate(50px, 70px);">x</div>"#,
            "",
        );
        let push = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } => Some(matrix),
                _ => None,
            })
            .expect("PushTransform missing");
        let a = push.0[0];
        let b = push.0[1];
        let c = push.0[4];
        let d = push.0[5];
        let e = push.0[12];
        let f = push.0[13];
        assert!((a - 1.0).abs() < 1e-5);
        assert!(b.abs() < 1e-5);
        assert!(c.abs() < 1e-5);
        assert!((d - 1.0).abs() < 1e-5);
        assert!((e - 50.0).abs() < 1e-5);
        assert!((f - 70.0).abs() < 1e-5);
    }

    #[test]
    fn transform_push_wraps_box_content() {
        // PushTransform идёт до собственного FillRect фона, PopTransform — после.
        let dl = build(
            r#"<div style="background: red; transform: translate(10px, 0);">x</div>"#,
            "",
        );
        let push_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushTransform { .. }))
            .unwrap();
        let pop_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopTransform))
            .unwrap();
        let fill_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .unwrap();
        assert!(push_idx < fill_idx, "Push должен идти до контента");
        assert!(fill_idx < pop_idx, "Pop должен идти после контента");
    }

    #[test]
    fn transform_after_opacity_in_walk_order() {
        // Phase 0 simple `walk`: PushOpacity → PushTransform → content →
        // PopTransform → PopOpacity. Transform применяется ВНУТРИ opacity-
        // layer-а (его эффект — на off-screen layer перед композицией).
        let dl = build(
            r#"<div style="background: red; opacity: 0.5; transform: scale(2);">x</div>"#,
            "",
        );
        let push_op = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        let push_tr = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushTransform { .. }))
            .unwrap();
        let pop_tr = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopTransform))
            .unwrap();
        let pop_op = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopOpacity))
            .unwrap();
        assert!(push_op < push_tr);
        assert!(push_tr < pop_tr);
        assert!(pop_tr < pop_op);
    }

    #[test]
    fn transform_serialize_2d_affine_components() {
        let dl = vec![
            DisplayCommand::PushTransform {
                matrix: Mat4::from_2d_affine(2.0, 0.0, 0.0, 0.5, 10.0, -20.0),
            },
            DisplayCommand::PopTransform,
        ];
        let s = serialize_display_list(&dl);
        // a=2.000 b=0.000 c=0.000 d=0.500 e=10.000 f=-20.000.
        assert_eq!(
            s,
            "PushTransform [2.000 0.000 0.000 0.500 10.000 -20.000]\nPopTransform\n"
        );
    }

    #[test]
    fn transform_ordered_emits_via_box_layer_ops() {
        // build_display_list_ordered идёт через box_layer_ops; должен дать
        // Push/Pop пару наряду с simple walk-ом.
        let dl = build_ordered(
            r#"<div style="background: red; transform: rotate(45deg);">x</div>"#,
            "",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopTransform));
        assert_eq!(pushes, 1);
        assert_eq!(pops, 1);
    }

    #[test]
    fn isolation_isolate_emits_full_alpha_group_layer() {
        // CSS Compositing L1 §2.1 — `isolation: isolate` must open an isolated
        // group so descendant blend modes composite against a transparent
        // backdrop. We reuse the opacity offscreen layer at alpha 1.0.
        let dl = build_ordered(
            r#"<div style="background: red; isolation: isolate;">x</div>"#,
            "",
        );
        let iso = count_variant(&dl, |c| {
            matches!(c, DisplayCommand::PushOpacity { alpha, .. } if (*alpha - 1.0).abs() < 1e-6)
        });
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopOpacity));
        assert_eq!(iso, 1, "isolate must open one full-alpha group layer");
        assert_eq!(pops, 1, "the group layer must be balanced");
    }

    #[test]
    fn isolation_auto_emits_no_group_layer() {
        // The initial value `isolation: auto` never forms a group on its own.
        let dl = build_ordered(
            r#"<div style="background: red; isolation: auto;">x</div>"#,
            "",
        );
        let opacity = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        assert_eq!(opacity, 0, "isolation:auto must not open an opacity group");
    }

    #[test]
    fn isolation_with_opacity_reuses_the_opacity_layer() {
        // When `opacity < 1` already opens an (isolated) offscreen group, the
        // dedicated isolation layer is redundant — only the opacity layer at
        // its real alpha should be emitted, not a second full-alpha one.
        let dl = build_ordered(
            r#"<div style="background: red; isolation: isolate; opacity: 0.5;">x</div>"#,
            "",
        );
        let pushes: Vec<f32> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::PushOpacity { alpha, .. } => Some(*alpha),
                _ => None,
            })
            .collect();
        assert_eq!(pushes.len(), 1, "exactly one opacity group, got {pushes:?}");
        assert!((pushes[0] - 0.5).abs() < 1e-6, "must keep the real opacity, got {pushes:?}");
    }

    #[test]
    fn ordered_scroll_container_emits_scrollbar() {
        // BUG-220: scroll containers painted through the ordered (stacking-
        // context) path lost their scrollbar — box_layer_ops emitted
        // PushScrollLayer but no DrawScrollbar (only the legacy `walk` did).
        // Both paths must now draw it via the shared `emit_scrollbars` helper.
        let dl = build_ordered(
            "<div class='sc'><div class='tall'></div></div>",
            ".sc { width: 100px; height: 100px; overflow: scroll; } \
             .tall { width: 50px; height: 500px; background: #f00; }",
        );
        let bars = count_variant(&dl, |c| matches!(c, DisplayCommand::DrawScrollbar { .. }));
        assert!(bars >= 1, "ordered scroll container must emit DrawScrollbar, got {bars}");
        // The bar must follow PopScrollLayer so it renders at a fixed position
        // (not translated with the scrolled content).
        let pop = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopScrollLayer))
            .expect("scroll container emits PopScrollLayer");
        let bar = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .expect("scroll container emits DrawScrollbar");
        assert!(pop < bar, "DrawScrollbar must follow PopScrollLayer: pop={pop} bar={bar}");
    }

    #[test]
    fn transform_origin_affects_matrix() {
        // С transform-origin (10, 20) и translate(0, 0) матрица =
        // T(10+box_x, 20+box_y) · I · T(-(10+box_x), -(20+box_y)) = I.
        // Здесь box_x/box_y зависят от layout; берём rotate чтобы origin
        // действительно изменял результат. rotate(90deg) с origin (0,0) -
        // точка (1,0) → (0,1). С origin (10,0) — точка (1,0) → (10, -9).
        // Просто проверяем что матрица не identity при rotate.
        let dl = build(
            r#"<div style="background: red; transform: rotate(90deg);">x</div>"#,
            "",
        );
        let push = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } => Some(matrix),
                _ => None,
            })
            .unwrap();
        assert!(!push.is_identity(), "rotate(90deg) ≠ identity");
        // sin/cos(90°): a=cos=0, b=sin=1, c=-sin=-1, d=cos=0.
        let a = push.0[0];
        let b = push.0[1];
        let c = push.0[4];
        let d = push.0[5];
        assert!(a.abs() < 1e-5);
        assert!((b - 1.0).abs() < 1e-5);
        assert!((c + 1.0).abs() < 1e-5);
        assert!(d.abs() < 1e-5);
    }

    // ─── CSS Transforms L2 §5.1 — backface-visibility culling ───────────────

    #[test]
    fn backface_hidden_rotated_past_90deg_culls_box() {
        // rotateY(180deg) turns the face fully away from the viewer;
        // backface-visibility: hidden must drop it from the display list.
        let dl = build(
            r#"<div style="width:50px;height:50px;background:red;
                transform: rotateY(180deg); backface-visibility: hidden;">x</div>"#,
            "",
        );
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::FillRect { .. })),
            0,
            "backface-hidden box rotated past 90° must not paint"
        );
    }

    #[test]
    fn backface_hidden_facing_viewer_still_paints() {
        // rotateY(0deg) — front face still points at the viewer, so the box
        // paints normally even with backface-visibility: hidden set.
        let dl = build(
            r#"<div style="width:50px;height:50px;background:red;
                transform: rotateY(0deg); backface-visibility: hidden;">x</div>"#,
            "",
        );
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::FillRect { .. })),
            1
        );
    }

    #[test]
    fn backface_visible_paints_even_when_rotated_away() {
        // Default `backface-visibility: visible` never culls, regardless of
        // rotation.
        let dl = build(
            r#"<div style="width:50px;height:50px;background:red;
                transform: rotateY(180deg);">x</div>"#,
            "",
        );
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::FillRect { .. })),
            1
        );
    }

    // ─── CSS Transforms L2 §6.2 — 3D depth sorting ───────────────────────────

    #[test]
    fn depth_order_back_to_front() {
        // z = [нос(10), зад(-5), середина(0)] → порядок зад→середина→нос.
        let order = depth_order_by_z(&[10.0, -5.0, 0.0]);
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn depth_order_stable_for_coplanar() {
        // Равные z (все 0) → исходный document order сохраняется.
        let order = depth_order_by_z(&[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn depth_order_partial_ties_keep_order() {
        // Совпадающие глубины (5.0) у индексов 0 и 2 → стабильно 0 раньше 2.
        let order = depth_order_by_z(&[5.0, -1.0, 5.0, 2.0]);
        assert_eq!(order, vec![1, 3, 0, 2]);
    }

    #[test]
    fn depth_order_nan_treated_as_coplanar() {
        // NaN не паникует и трактуется как равный — стабильный порядок.
        let order = depth_order_by_z(&[f32::NAN, 1.0, f32::NAN]);
        assert_eq!(order.len(), 3);
        // 1.0 не имеет определённого отношения к NaN (cmp→Equal), поэтому
        // стабильная сортировка оставляет всё на местах.
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn depth_order_empty() {
        assert_eq!(depth_order_by_z(&[]), Vec::<usize>::new());
    }

    #[test]
    fn flat_context_keeps_document_order() {
        // Без preserve-3d (establishes_3d_rendering_context == false) дети
        // рисуются в document order — три фона идут red, green, blue.
        let dl = build(
            r#"<div>
                 <div style="background: red;">a</div>
                 <div style="background: green;">b</div>
                 <div style="background: blue;">c</div>
               </div>"#,
            "",
        );
        let bg: Vec<(u8, u8, u8)> = fills(&dl).iter().map(|c| (c.r, c.g, c.b)).collect();
        let red = bg.iter().position(|c| *c == (255, 0, 0));
        let green = bg.iter().position(|c| *c == (0, 128, 0));
        let blue = bg.iter().position(|c| *c == (0, 0, 255));
        assert!(red < green && green < blue, "document order: {bg:?}");
    }

    // ─── build_display_list_with_anim ────────────────────────────────────────

    use lumen_layout::{CompositorAnimFrame, CompositorOverride};
    use lumen_dom::NodeId;
    use std::collections::HashMap;

    fn build_anim(html: &str, css: &str, overrides: HashMap<NodeId, CompositorOverride>) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let frame = CompositorAnimFrame { overrides, has_active: true };
        build_display_list_with_anim(&tree, Some(&frame))
    }

    #[test]
    fn anim_no_overrides_same_as_base() {
        let html = r#"<div style="background:red;width:100px;height:50px"></div>"#;
        let base = build(html, "");
        let anim = build_anim(html, "", HashMap::new());
        assert_eq!(base.len(), anim.len(), "empty overrides: same DL length");
    }

    #[test]
    fn anim_none_frame_same_as_base() {
        let html = r#"<div style="background:blue;width:80px;height:40px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let base = build_display_list(&tree);
        let with_none = build_display_list_with_anim(&tree, None);
        assert_eq!(base.len(), with_none.len());
    }

    #[test]
    fn anim_opacity_override_emits_push_opacity() {
        // A div without opacity in style — no PushOpacity in base DL.
        let html = r#"<div style="background:green;width:100px;height:50px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));

        let base = build_display_list(&tree);
        let has_push_base = base.iter().any(|c| matches!(c, DisplayCommand::PushOpacity { .. }));
        assert!(!has_push_base, "base DL should have no PushOpacity");

        // Override opacity=0.5 for the body node (root).
        let node = tree.node;
        let mut overrides = HashMap::new();
        overrides.insert(node, CompositorOverride { opacity: Some(0.5), ..Default::default() });
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let anim_dl = build_display_list_with_anim(&tree, Some(&frame));

        let push_count = anim_dl.iter().filter(|c| matches!(c, DisplayCommand::PushOpacity { .. })).count();
        let pop_count = anim_dl.iter().filter(|c| matches!(c, DisplayCommand::PopOpacity)).count();
        assert_eq!(push_count, 1, "should emit one PushOpacity for the animated node");
        assert_eq!(pop_count, 1, "PushOpacity/PopOpacity must be balanced");

        if let Some(DisplayCommand::PushOpacity { alpha, .. }) = anim_dl.iter().find(|c| matches!(c, DisplayCommand::PushOpacity { .. })) {
            assert!((*alpha - 0.5).abs() < 1e-5, "opacity should be 0.5, got {alpha}");
        }
    }

    #[test]
    fn anim_push_pop_balanced() {
        // Any DL produced by with_anim must have balanced Push/Pop pairs.
        let html = r#"<div style="background:red;width:200px;height:100px">
            <div style="background:blue;width:100px;height:50px"></div>
        </div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let node = tree.node;
        let mut overrides = HashMap::new();
        overrides.insert(node, CompositorOverride { opacity: Some(0.7), ..Default::default() });
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let dl = build_display_list_with_anim(&tree, Some(&frame));

        let push_op = dl.iter().filter(|c| matches!(c, DisplayCommand::PushOpacity { .. })).count();
        let pop_op = dl.iter().filter(|c| matches!(c, DisplayCommand::PopOpacity)).count();
        let push_tx = dl.iter().filter(|c| matches!(c, DisplayCommand::PushTransform { .. })).count();
        let pop_tx = dl.iter().filter(|c| matches!(c, DisplayCommand::PopTransform)).count();
        assert_eq!(push_op, pop_op, "PushOpacity/PopOpacity must balance");
        assert_eq!(push_tx, pop_tx, "PushTransform/PopTransform must balance");
    }

    /// Recursively find the node of the first box whose resolved background colour
    /// equals `want` — used to target a compositor override at the right box.
    pub(crate) fn find_bg_node(b: &lumen_layout::LayoutBox, want: Color) -> Option<NodeId> {
        if b.style.background_color.and_then(|c| c.to_color_opt()) == Some(want) {
            return Some(b.node);
        }
        b.children.iter().find_map(|c| find_bg_node(c, want))
    }

    /// BUG-231: an animated background-color compositor override must replace the
    /// box's background FillRect colour in the ordered (live) paint path without
    /// relayout — the green base fill becomes the overridden orange.
    #[test]
    fn anim_background_color_override_patches_fill_ordered() {
        let html = r#"<div style="background:#008000;width:100px;height:50px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let green = Color { r: 0, g: 0x80, b: 0, a: 255 };
        let orange = Color { r: 0xff, g: 0x8f, b: 0, a: 255 };
        let node = find_bg_node(&tree, green).expect("box with green background");

        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);

        // Base ordered DL paints the green background.
        let base = build_display_list_ordered(&tree, &stacking_tree, &order).0;
        assert!(
            base.iter().any(|c| matches!(c,
                DisplayCommand::FillRect { color, .. } | DisplayCommand::FillRoundedRect { color, .. }
                if *color == green)),
            "base DL должен заливать зелёным фоном"
        );

        // Override background-color → orange.
        let mut overrides = HashMap::new();
        overrides.insert(node, CompositorOverride {
            background_color: Some(orange),
            ..Default::default()
        });
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let anim = build_display_list_ordered_with_anim(&tree, &stacking_tree, &order, Some(&frame));

        assert!(
            anim.iter().any(|c| matches!(c,
                DisplayCommand::FillRect { color, .. } | DisplayCommand::FillRoundedRect { color, .. }
                if *color == orange)),
            "override должен перекрасить фон в оранжевый"
        );
        assert!(
            !anim.iter().any(|c| matches!(c,
                DisplayCommand::FillRect { color, .. } | DisplayCommand::FillRoundedRect { color, .. }
                if *color == green)),
            "после override зелёного фона быть не должно"
        );
    }
