//! P1/SPLIT-DL5: iframe/audio/bg-image/gradient/mask/object-fit тесты в
//! `mod tests` `display_list.rs` — `<iframe>`/`<audio>` placeholder,
//! background-image url()/gradient (BUG-087), background-origin/mask-origin/
//! mask-composite, `fit_image`/object-fit (BUG-181), layer-ops serialize.
//! Регион (2 043 плановые / 2 042 факт. строки) не уместился под потолок
//! ≤2000 строк на файл — разрезан на два по границе `build_display_list_ordered`
//! (см. `ordered_build_scroll.rs`, парный файл того же батча).
//! Перенесено байт-в-байт из `display_list.rs` без дедента (приём ST-1/DL-1).
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-5).

use super::*;
// build/images уехали в display_list/tests/text_and_images.rs (батч DL-6).
use super::text_and_images::{build, images};

    // ── Тесты <iframe> / DrawImage placeholder ──────────────────────────────

    /// BUG-431: the grey placeholder belongs in the content box, same rule as
    /// `<img>`/`<video>`/`<canvas>` — painting at the border box slid it under
    /// the border+padding.
    #[test]
    fn iframe_placeholder_is_painted_into_the_content_box() {
        let dl = build(
            r#"<iframe src="https://example.com" width="100" height="80"></iframe>"#,
            "*{margin:0}iframe{border:10px solid red;padding:5px}",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, src, .. } = imgs[0] {
            assert_eq!(src, "https://example.com");
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (15.0, 15.0, 100.0, 80.0));
        }
    }

    // ── Тесты <audio> ─────────────────────────────────────────────────────────

    #[test]
    fn audio_without_controls_emits_nothing() {
        // <audio> without controls → 0×0 box → no FillRect emitted.
        let dl = build(r#"<audio src="song.mp3"></audio>"#, "");
        let fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .collect();
        assert!(
            fills.is_empty(),
            "audio without controls should emit no FillRect, got {} commands",
            fills.len()
        );
    }

    #[test]
    fn audio_with_controls_emits_fill_rect() {
        // <audio controls> → 40px grey bar → at least one FillRect.
        let dl = build(r#"<audio src="song.mp3" controls></audio>"#, "");
        let fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .collect();
        assert!(!fills.is_empty(), "audio with controls should emit a FillRect");
    }

    #[test]
    fn audio_with_controls_ua_default_height_40() {
        // UA default for <audio controls>: height = 40px.
        let dl = build(r#"<audio src="song.mp3" controls></audio>"#, "");
        let fill = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::FillRect { .. }));
        if let Some(DisplayCommand::FillRect { rect, .. }) = fill {
            assert!(
                (rect.height - 40.0).abs() < 0.1,
                "audio controls height should be 40px, got {}",
                rect.height
            );
        }
    }

    #[test]
    fn audio_with_controls_css_height_override() {
        // Explicit CSS height overrides UA default.
        let dl = build(
            r#"<audio src="song.mp3" controls></audio>"#,
            "audio { height: 60px; }",
        );
        let fill = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::FillRect { .. }));
        if let Some(DisplayCommand::FillRect { rect, .. }) = fill {
            assert!(
                (rect.height - 60.0).abs() < 0.1,
                "CSS height should override UA default, got {}",
                rect.height
            );
        }
    }

    // ── Тесты background-image url() / DrawBackgroundImage ─────────────────

    fn bg_images(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBackgroundImage { .. }))
            .collect()
    }

    #[test]
    fn block_background_image_url_emits_draw_background_image() {
        let dl = build(
            "<div>x</div>",
            "div { width: 80px; height: 40px; background-image: url(bg.png); }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1, "должна быть одна команда DrawBackgroundImage");
        if let DisplayCommand::DrawBackgroundImage { rect, src, .. } = bgs[0] {
            assert_eq!(src, "bg.png");
            assert!((rect.width - 80.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((rect.height - 40.0).abs() < 0.1, "rect.height={}", rect.height);
        }
    }

    #[test]
    fn background_image_none_emits_nothing() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; background-image: none; }",
        );
        assert!(bg_images(&dl).is_empty());
    }

    #[test]
    fn background_image_default_emits_nothing() {
        // initial value `none` (CSS Backgrounds L3 §3.10): отсутствие свойства
        // не должно эмитить DrawBackgroundImage.
        let dl = build("<div>x</div>", "div { width: 50px; height: 20px; }");
        assert!(bg_images(&dl).is_empty());
    }

    #[test]
    fn background_image_linear_gradient_emits_draw_linear_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: linear-gradient(to right, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawLinearGradient");
        if let DisplayCommand::DrawLinearGradient { angle_deg, stops, repeating, .. } = grads[0] {
            assert!((angle_deg - 90.0).abs() < 0.1, "expected 90° for 'to right', got {angle_deg}");
            assert_eq!(stops.len(), 2);
            assert!(!repeating);
        }
    }

    #[test]
    fn background_image_linear_gradient_with_border_radius_clips_rounded() {
        // BUG-631: a gradient background on a rounded box must clip to the
        // same rounded painting area as a solid `background-color`, not a
        // square `PushClipRect` — otherwise the gradient fills the corners.
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; border-radius: 8px; \
             background-image: linear-gradient(to right, red, blue); }",
        );
        let grad_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .expect("expected DrawLinearGradient");
        assert!(
            matches!(dl[grad_idx - 1], DisplayCommand::PushClipRoundedRect { .. }),
            "gradient must be preceded by PushClipRoundedRect, got {:?}",
            dl[grad_idx - 1]
        );
        if let DisplayCommand::PushClipRoundedRect { radii, .. } = dl[grad_idx - 1] {
            assert_eq!(radii, [8.0, 8.0, 8.0, 8.0]);
        }
        assert!(
            matches!(dl[grad_idx + 1], DisplayCommand::PopClip),
            "gradient's rounded clip must be closed"
        );
    }

    #[test]
    fn background_image_radial_gradient_emits_draw_radial_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: radial-gradient(circle at 50% 50%, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawRadialGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawRadialGradient");
        if let DisplayCommand::DrawRadialGradient { center_x_pct, center_y_pct, stops, .. } = grads[0] {
            assert!((center_x_pct - 0.5).abs() < 0.01);
            assert!((center_y_pct - 0.5).abs() < 0.01);
            assert_eq!(stops.len(), 2);
        }
    }

    #[test]
    fn background_image_conic_gradient_emits_draw_conic_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: conic-gradient(from 90deg at 30% 70%, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawConicGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawConicGradient");
        if let DisplayCommand::DrawConicGradient {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating, ..
        } = grads[0]
        {
            assert!((center_x_pct - 0.3).abs() < 0.01);
            assert!((center_y_pct - 0.7).abs() < 0.01);
            assert!((from_angle_deg - 90.0).abs() < 0.1);
            assert_eq!(stops.len(), 2);
            assert!(!repeating);
        }
    }

    // ── BUG-087: sized/positioned/repeated gradient background layers ──────────

    fn linear_grads(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect()
    }

    #[test]
    fn gradient_tile_rects_single_no_repeat() {
        // 80×80 tile, centered (50% 50%) inside a 200×120 area, no-repeat → 1 rect.
        let origin = Rect::new(0.0, 0.0, 200.0, 120.0);
        let rects = gradient_tile_rects(
            80.0,
            80.0,
            ObjectPosition { x: PositionComponent::Percent(0.5), y: PositionComponent::Percent(0.5) },
            BackgroundRepeat::NoRepeat,
            origin,
            origin,
        );
        assert_eq!(rects.len(), 1);
        // off = (200-80)*0.5 = 60 ; (120-80)*0.5 = 20
        assert!((rects[0].x - 60.0).abs() < 0.01, "x={}", rects[0].x);
        assert!((rects[0].y - 20.0).abs() < 0.01, "y={}", rects[0].y);
        assert!((rects[0].width - 80.0).abs() < 0.01);
        assert!((rects[0].height - 80.0).abs() < 0.01);
    }

    #[test]
    fn gradient_tile_rects_repeat_x_covers_area() {
        // 20px-wide tiles, full height, repeat-x across a 200px area → tiles span it.
        let origin = Rect::new(0.0, 0.0, 200.0, 100.0);
        let rects = gradient_tile_rects(
            20.0,
            100.0,
            ObjectPosition { x: PositionComponent::Percent(0.0), y: PositionComponent::Percent(0.0) },
            BackgroundRepeat::RepeatX,
            origin,
            origin,
        );
        // 200/20 = 10 tiles, single row.
        assert_eq!(rects.len(), 10, "expected 10 stripes, got {}", rects.len());
        assert!(rects.iter().all(|r| (r.height - 100.0).abs() < 0.01));
        // Tiles span from left to right edge.
        assert!((rects[0].x - 0.0).abs() < 0.01);
        assert!((rects[9].x - 180.0).abs() < 0.01, "last x={}", rects[9].x);
    }

    #[test]
    fn sized_gradient_layer_emits_tile_not_full_box() {
        // BUG-087: a gradient with explicit `background-size` must paint a tile of
        // that size (clipped to the box), not stretch across the whole box.
        let dl = build(
            "<div>x</div>",
            "div { width: 200px; height: 120px; \
             background: linear-gradient(to right, red, blue) center / 80px 80px no-repeat; }",
        );
        let grads = linear_grads(&dl);
        assert_eq!(grads.len(), 1, "one gradient tile expected");
        if let DisplayCommand::DrawLinearGradient { rect, .. } = grads[0] {
            assert!((rect.width - 80.0).abs() < 0.1, "tile width should be 80, got {}", rect.width);
            assert!((rect.height - 80.0).abs() < 0.1, "tile height should be 80, got {}", rect.height);
        }
        // Sized tiling must be wrapped in a clip to the painting area.
        assert!(
            dl.iter().any(|c| matches!(c, DisplayCommand::PushClipRect { .. })),
            "sized gradient must be clipped to the box"
        );
    }

    #[test]
    fn repeat_x_gradient_layer_emits_multiple_tiles() {
        // BUG-087: repeat-x sized gradient emits one command per visible stripe.
        let dl = build(
            "<div>x</div>",
            "div { width: 100px; height: 50px; \
             background: linear-gradient(to bottom, red, blue) left top / 20px 100% repeat-x; }",
        );
        let grads = linear_grads(&dl);
        assert!(grads.len() >= 5, "expected ≥5 stripes for 100px/20px, got {}", grads.len());
        for g in &grads {
            if let DisplayCommand::DrawLinearGradient { rect, .. } = g {
                assert!((rect.width - 20.0).abs() < 0.1, "stripe width 20, got {}", rect.width);
            }
        }
    }

    #[test]
    fn unsized_gradient_layer_still_fills_box() {
        // Regression guard: a gradient WITHOUT background-size keeps the historical
        // single full-box command (no tiling, no extra clip) so existing snapshots
        // stay byte-identical.
        let dl = build(
            "<div>x</div>",
            "div { width: 200px; height: 120px; \
             background: linear-gradient(to right, red, blue); }",
        );
        let grads = linear_grads(&dl);
        assert_eq!(grads.len(), 1, "single full-box gradient");
        if let DisplayCommand::DrawLinearGradient { rect, .. } = grads[0] {
            assert!((rect.width - 200.0).abs() < 0.1, "full box width, got {}", rect.width);
            assert!((rect.height - 120.0).abs() < 0.1, "full box height, got {}", rect.height);
        }
    }

    #[test]
    fn background_image_repeating_conic_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: repeating-conic-gradient(red 0deg, blue 90deg); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawConicGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawConicGradient (repeating)");
        if let DisplayCommand::DrawConicGradient { repeating, .. } = grads[0] {
            assert!(*repeating);
        }
    }

    #[test]
    fn background_image_conic_gradient_serialize_includes_from_angle() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: conic-gradient(from 45deg, red, blue); }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawConicGradient"), "should contain DrawConicGradient line");
        assert!(s.contains("from=45.0deg"), "should record from-angle: {s}");
    }

    #[test]
    fn background_image_repeating_linear_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: repeating-linear-gradient(45deg, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawLinearGradient for repeating");
        if let DisplayCommand::DrawLinearGradient { angle_deg, repeating, .. } = grads[0] {
            assert!((angle_deg - 45.0).abs() < 0.1);
            assert!(*repeating);
        }
    }

    #[test]
    fn background_image_linear_gradient_default_angle_is_to_bottom() {
        // No direction specified → default is "to bottom" = 180°.
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: linear-gradient(red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1);
        if let DisplayCommand::DrawLinearGradient { angle_deg, .. } = grads[0] {
            assert!((angle_deg - 180.0).abs() < 0.1, "expected 180° default, got {angle_deg}");
        }
    }

    #[test]
    fn background_image_paints_after_color_before_border() {
        // CSS Backgrounds L3 §3.10 — painting order: bg-color → bg-image → border.
        let dl = build(
            "<div></div>",
            "div { width: 60px; height: 30px; \
             background-color: red; background-image: url(b.png); \
             border: 2px solid blue; }",
        );
        let kinds: Vec<&str> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { .. } => Some("FillRect"),
                DisplayCommand::DrawBackgroundImage { .. } => Some("DrawBackgroundImage"),
                DisplayCommand::DrawBorder { .. } => Some("DrawBorder"),
                _ => None,
            })
            .collect();
        // Allow surrounding commands; check relative order of the three.
        let fill = kinds.iter().position(|k| *k == "FillRect").expect("FillRect emitted");
        let bg = kinds.iter().position(|k| *k == "DrawBackgroundImage").expect("bg-image emitted");
        let border = kinds.iter().position(|k| *k == "DrawBorder").expect("border emitted");
        assert!(fill < bg, "bg-color must precede bg-image (kinds={kinds:?})");
        assert!(bg < border, "bg-image must precede border (kinds={kinds:?})");
    }

    #[test]
    fn background_image_serialize_includes_src() {
        let dl = build(
            "<div>x</div>",
            "div { width: 40px; height: 10px; background-image: url(\"hero.jpg\"); }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBackgroundImage"), "should contain DrawBackgroundImage line");
        assert!(s.contains(r#"src="hero.jpg""#), "should contain quoted src");
    }

    #[test]
    fn background_image_paint_emits_draw_background_image_with_paint_src() {
        // CSS Paint API (Houdini) Phase 0 — `background-image: paint(name)` must emit
        // DrawBackgroundImage with src prefixed "paint:" for renderer identification.
        let dl = build(
            "<div></div>",
            "div { width: 80px; height: 40px; background-image: paint(my-worklet); }",
        );
        let paint_bg = dl.iter().find(|c| {
            matches!(c, DisplayCommand::DrawBackgroundImage { src, .. } if src.starts_with("paint:"))
        });
        assert!(paint_bg.is_some(), "paint() must emit DrawBackgroundImage with 'paint:' src");
        if let Some(DisplayCommand::DrawBackgroundImage { src, .. }) = paint_bg {
            assert_eq!(src, "paint:my-worklet", "src must be paint:<name>");
        }
    }

    #[test]
    fn background_image_respects_background_clip_padding_box() {
        // background-clip: padding-box ужимает rect под border на каждой стороне.
        // box-sizing по умолчанию content-box: width=100 — это контент,
        // полная коробка с border 5×2 = 110×70. PaddingBox shrink → 100×60.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 5px solid red; background-clip: padding-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, .. } = bgs[0] {
            assert!((rect.width - 100.0).abs() < 0.1, "got {}", rect.width);
            assert!((rect.height - 60.0).abs() < 0.1, "got {}", rect.height);
        }
    }

    // ── Тесты background-origin ────────────────────────────────────────────────

    #[test]
    fn background_origin_default_padding_box_equals_clip_border_box() {
        // Default: background-origin: padding-box, background-clip: border-box.
        // With no border: origin_rect == clip rect == border-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            assert!((rect.width - 100.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((origin_rect.width - 100.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
            assert!((rect.height - 60.0).abs() < 0.1);
            assert!((origin_rect.height - 60.0).abs() < 0.1);
        }
    }

    #[test]
    fn background_origin_content_box_excludes_padding_and_border() {
        // box-sizing: content-box, width=100, height=60, border=5px, padding=10px.
        // border-box: 130×90. padding-box: 120×80. content-box (origin): 100×60.
        // background-clip: border-box by default → rect is 130×90.
        // background-origin: content-box → origin_rect is 100×60.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 5px solid red; padding: 10px; \
             background-origin: content-box; background-clip: border-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            // clip rect (border-box) = 130×90
            assert!((rect.width - 130.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((rect.height - 90.0).abs() < 0.1, "rect.height={}", rect.height);
            // origin_rect (content-box) = 100×60
            assert!((origin_rect.width - 100.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
            assert!((origin_rect.height - 60.0).abs() < 0.1, "origin_rect.height={}", origin_rect.height);
        }
    }

    #[test]
    fn background_origin_border_box_equals_clip_border_box() {
        // background-origin: border-box means positioning starts at border edge.
        // With 5px border: both rect and origin_rect include border area.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 5px solid red; background-origin: border-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            // Both clip (border-box default) and origin (border-box explicit) = 110×70
            assert!((rect.width - 110.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((origin_rect.width - 110.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
            assert!((rect.width - origin_rect.width).abs() < 0.1, "rects should match");
            assert!((rect.height - origin_rect.height).abs() < 0.1, "rects should match");
        }
    }

    #[test]
    fn background_origin_padding_box_with_border_shrinks_origin() {
        // background-origin: padding-box (default), background-clip: border-box.
        // With 8px border: border-box=116×76, padding-box=100×60.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 8px solid black; background-origin: padding-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            assert!((rect.width - 116.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((origin_rect.width - 100.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
        }
    }

    // ── Тесты mask-origin / mask-position (CSS Masking L1 §4.4–§4.5) ────────────

    fn push_mask_gradient_rect(dl: &DisplayList) -> lumen_core::geom::Rect {
        dl.iter()
            .find_map(|c| match c {
                DisplayCommand::PushMaskLinearGradient { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("gradient mask must emit PushMaskLinearGradient")
    }

    // ── mask-composite: intersect через вложение групп (CSS Masking L1 §4.7) ──

    /// Имена mask-команд подряд — достаточно, чтобы проверить и количество
    /// открытых групп, и их вложенность (Push… Push… Pop Pop).
    fn mask_group_shape(dl: &DisplayList) -> Vec<&'static str> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::PushMaskImage { .. } => Some("PushImage"),
                DisplayCommand::PushMaskLinearGradient { .. } => Some("PushLinear"),
                DisplayCommand::PopMask => Some("Pop"),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn mask_composite_intersect_nests_one_group_per_layer() {
        // Вложение перемножает альфы = Porter-Duff source-in = `intersect`.
        // Верхний слой (url) снаружи, нижний (градиент) внутри; закрываются
        // двумя PopMask.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; \
             mask-image: url(a.png), linear-gradient(black, transparent); \
             mask-composite: intersect, add; }",
        );
        assert_eq!(
            mask_group_shape(&dl),
            vec!["PushImage", "PushLinear", "Pop", "Pop"]
        );
    }

    #[test]
    fn mask_composite_add_renders_top_layer_only() {
        // `add` вложением не выражается (нужна сборка маски в офскрине) —
        // остаётся прежнее поведение: рендерится только верхний слой.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; \
             mask-image: url(a.png), linear-gradient(black, transparent); }",
        );
        assert_eq!(mask_group_shape(&dl), vec!["PushImage", "Pop"]);
    }

    #[test]
    fn mask_composite_intersect_on_bottom_layer_falls_back() {
        // У нижнего слоя оператор применяется к прозрачному фону: `intersect`
        // там вычистил бы маску целиком, и браузеры расходятся в трактовке —
        // уходим в fallback «только верхний слой», а не гасим элемент.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; \
             mask-image: url(a.png), linear-gradient(black, transparent); \
             mask-composite: intersect, intersect; }",
        );
        assert_eq!(mask_group_shape(&dl), vec!["PushImage", "Pop"]);
    }

    #[test]
    fn mask_composite_intersect_with_none_layer_falls_back() {
        // Слой `none` — прозрачная маска; в `intersect` он обнулил бы результат,
        // поэтому такая цепочка не вкладывается.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; \
             mask-image: url(a.png), none; mask-composite: intersect, add; }",
        );
        assert_eq!(mask_group_shape(&dl), vec!["PushImage", "Pop"]);
    }

    #[test]
    fn mask_clip_intersects_across_nested_layers() {
        // content-box 100×60 + padding 10 + border 5 → padding-box 120×80,
        // content-box 100×60. Клипы слоёв пересекаются → берётся content-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; border: 5px solid red; padding: 10px; \
             mask-image: linear-gradient(black, transparent), linear-gradient(black, white); \
             mask-clip: padding-box, content-box; mask-composite: intersect, add; }",
        );
        let clip = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("mask-clip must emit PushClipRect");
        assert!((clip.width - 100.0).abs() < 0.1, "clip.width={}", clip.width);
        assert!((clip.height - 60.0).abs() < 0.1, "clip.height={}", clip.height);
    }

    #[test]
    fn mask_origin_default_border_box_uses_full_box() {
        // mask-origin initial value is `border-box` (CSS Masking L1 §4.5), unlike
        // background-origin (`padding-box`). content-box 100×60 + padding 10 +
        // border 5 → border-box 130×90; the gradient mask covers the full box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; \
             mask-image: linear-gradient(to bottom, black, transparent); \
             border: 5px solid red; padding: 10px; }",
        );
        let rect = push_mask_gradient_rect(&dl);
        assert!((rect.width - 130.0).abs() < 0.1, "rect.width={}", rect.width);
        assert!((rect.height - 90.0).abs() < 0.1, "rect.height={}", rect.height);
    }

    #[test]
    fn mask_origin_content_box_shrinks_push_mask_rect() {
        // mask-origin: content-box positions the mask over the content box only.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; \
             mask-image: linear-gradient(to bottom, black, transparent); \
             border: 5px solid red; padding: 10px; mask-origin: content-box; }",
        );
        let rect = push_mask_gradient_rect(&dl);
        assert!((rect.width - 100.0).abs() < 0.1, "rect.width={}", rect.width);
        assert!((rect.height - 60.0).abs() < 0.1, "rect.height={}", rect.height);
    }

    #[test]
    fn mask_position_threaded_into_push_mask_image() {
        // mask-position must reach the PushMaskImage command (no longer hardcoded
        // to background_initial). 25% 75% → Percent(0.25)/Percent(0.75).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; \
             mask-image: url(m.png); mask-position: 25% 75%; }",
        );
        let pos = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushMaskImage { position, .. } => Some(*position),
                _ => None,
            })
            .expect("url mask must emit PushMaskImage");
        assert_eq!(pos.x, PositionComponent::Percent(0.25), "mask-position x");
        assert_eq!(pos.y, PositionComponent::Percent(0.75), "mask-position y");
    }

    #[test]
    fn img_without_dimensions_emits_zero_rect() {
        // Без размеров — placeholder 0×0; команда всё равно эмитится,
        // потому что DOM-узел существует. Renderer просто не нарисует ничего.
        let dl = build(r#"<img src="x">"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!(rect.width.abs() < 0.1);
            assert!(rect.height.abs() < 0.1);
        }
    }

    #[test]
    fn multiple_imgs_emit_multiple_draw_image() {
        let dl = build(
            r#"<img src="a.png" width="10" height="10"><img src="b.png" width="20" height="20">"#,
            "",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 2);
    }

    // ── Тесты fit_image_rect / fit_image_quad (CSS Images L3 §5.5) ──────────

    fn box100() -> Rect {
        Rect::new(0.0, 0.0, 100.0, 100.0)
    }

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn approx_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) -> bool {
        approx_eq(r.x, x) && approx_eq(r.y, y) && approx_eq(r.width, w) && approx_eq(r.height, h)
    }

    #[test]
    fn fit_fill_stretches_to_box() {
        let placed = fit_image_rect(box100(), (50, 200), ObjectFit::Fill, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn fit_contain_letterboxes_wide_image() {
        // 200×100 в 100×100: scale=0.5, placed=100×50, центрируется по y.
        let placed = fit_image_rect(box100(), (200, 100), ObjectFit::Contain, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 25.0, 100.0, 50.0));
    }

    #[test]
    fn fit_contain_pillarboxes_tall_image() {
        // 100×200 в 100×100: scale=0.5, placed=50×100, центрируется по x.
        let placed = fit_image_rect(box100(), (100, 200), ObjectFit::Contain, ObjectPosition::default());
        assert!(approx_rect(placed, 25.0, 0.0, 50.0, 100.0));
    }

    #[test]
    fn fit_cover_overflows_wide_image() {
        // 200×100 в 100×100 при cover: scale=1.0, placed=200×100, центр →
        // x=-50, y=0.
        let placed = fit_image_rect(box100(), (200, 100), ObjectFit::Cover, ObjectPosition::default());
        assert!(approx_rect(placed, -50.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn fit_none_keeps_intrinsic_size() {
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, ObjectPosition::default());
        // 50×50 центрируется в 100×100.
        assert!(approx_rect(placed, 25.0, 25.0, 50.0, 50.0));
    }

    #[test]
    fn fit_scale_down_picks_none_when_smaller() {
        // 50×50 меньше 100×100 — none даёт меньшую площадь, чем contain.
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::ScaleDown, ObjectPosition::default());
        assert!(approx_rect(placed, 25.0, 25.0, 50.0, 50.0));
    }

    #[test]
    fn fit_scale_down_picks_contain_when_larger() {
        // 200×200 больше 100×100 — contain даёт меньшую площадь.
        let placed = fit_image_rect(box100(), (200, 200), ObjectFit::ScaleDown, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn fit_position_top_left_aligns_to_origin() {
        let pos = ObjectPosition {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        };
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, pos);
        assert!(approx_rect(placed, 0.0, 0.0, 50.0, 50.0));
    }

    #[test]
    fn fit_position_bottom_right_aligns_to_corner() {
        let pos = ObjectPosition {
            x: PositionComponent::Percent(1.0),
            y: PositionComponent::Percent(1.0),
        };
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, pos);
        assert!(approx_rect(placed, 50.0, 50.0, 50.0, 50.0));
    }

    #[test]
    fn fit_zero_intrinsic_size_returns_box() {
        let placed = fit_image_rect(box100(), (0, 100), ObjectFit::Cover, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    // ── BUG-181 (TEST-19): геометрия `object-fit` на реальных размерах
    // картинок теста (perceptron 852×725, agi 1024×1024) в боксе 180×120 @
    // (26,26). Расследование показало: все 5 режимов + object-position
    // позиционируют картинку пиксель-в-пиксель с Edge (средние RGB совпадают,
    // лучший сдвиг 0,0, letterbox корректен). Остаток ~9% TEST-19 =
    // image-resampling AA на высокочастотном контенте (диаграмма + rusty-
    // текстура agi) — BUG-219, TEST-19 → KNOWN_DEBTORS. Тесты фиксируют
    // геометрию, чтобы regression в placement не спрятался за resampling-шумом.

    /// Бокс `.box` теста 19: первая ячейка border-box (26,26)+180×120.
    fn test19_box() -> Rect {
        Rect::new(26.0, 26.0, 180.0, 120.0)
    }

    #[test]
    fn bug181_perceptron_contain_letterboxes_horizontally() {
        // 852×725 в 180×120 при contain: scale=min(180/852,120/725)=0.16552 →
        // placed 141.02×120, центрируется по x (бары .box-градиента слева/справа).
        let placed =
            fit_image_rect(test19_box(), (852, 725), ObjectFit::Contain, ObjectPosition::default());
        let s = (180.0_f32 / 852.0).min(120.0 / 725.0);
        let cw = 852.0 * s;
        assert!(approx_rect(placed, 26.0 + (180.0 - cw) / 2.0, 26.0, cw, 120.0));
    }

    #[test]
    fn bug181_perceptron_cover_overflows_vertically() {
        // cover: scale=max(180/852,120/725)=0.21127 → placed 180×153.17,
        // обрезается по высоте (центр), верх/низ уходят за бокс.
        let placed =
            fit_image_rect(test19_box(), (852, 725), ObjectFit::Cover, ObjectPosition::default());
        let s = (180.0_f32 / 852.0).max(120.0 / 725.0);
        let ch = 725.0 * s;
        assert!(approx_rect(placed, 26.0, 26.0 + (120.0 - ch) / 2.0, 180.0, ch));
    }

    #[test]
    fn bug181_perceptron_none_center_crops_full_resolution() {
        // none: картинка в натуральную величину 852×725, центрируется в боксе
        // → видна только центральная вырезка (UV-clip в fit_image_quad).
        let placed =
            fit_image_rect(test19_box(), (852, 725), ObjectFit::None, ObjectPosition::default());
        assert!(approx_rect(placed, 26.0 - (852.0 - 180.0) / 2.0, 26.0 - (725.0 - 120.0) / 2.0, 852.0, 725.0));
    }

    #[test]
    fn bug181_agi_cover_square_image_position_bottom_right() {
        // 1024×1024 cover в 180×120: scale=max(180,120)/1024=0.17578 →
        // placed 180×180 (квадрат). object-position: right bottom → по y
        // прижата к низу (off_y = 120-180 = -60), по x свободного места нет.
        let pos = ObjectPosition {
            x: PositionComponent::Percent(1.0),
            y: PositionComponent::Percent(1.0),
        };
        let placed = fit_image_rect(test19_box(), (1024, 1024), ObjectFit::Cover, pos);
        assert!(approx_rect(placed, 26.0, 26.0 - 60.0, 180.0, 180.0));
    }

    #[test]
    fn bug181_agi_cover_position_25_75() {
        // 1024×1024 cover, object-position: 25% 75% → off_y = (120-180)*0.75
        // = -45 (по x свободного места нет, off_x=0).
        let pos = ObjectPosition {
            x: PositionComponent::Percent(0.25),
            y: PositionComponent::Percent(0.75),
        };
        let placed = fit_image_rect(test19_box(), (1024, 1024), ObjectFit::Cover, pos);
        assert!(approx_rect(placed, 26.0, 26.0 + (120.0 - 180.0) * 0.75, 180.0, 180.0));
    }

    #[test]
    fn quad_contain_returns_full_uvs() {
        // contain не выходит за box → uv = [0,0]..[1,1].
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 100),
            ObjectFit::Contain,
            ObjectPosition::default(),
        )
        .expect("contain visible");
        assert!(approx_rect(visible, 0.0, 25.0, 100.0, 50.0));
        assert!(approx_eq(uv0[0], 0.0) && approx_eq(uv0[1], 0.0));
        assert!(approx_eq(uv1[0], 1.0) && approx_eq(uv1[1], 1.0));
    }

    #[test]
    fn quad_cover_crops_uvs_horizontally() {
        // 200×100 cover в 100×100: placement=200×100 at x=-50; visible=
        // box100; UV: u0=(0-(-50))/200=0.25, u1=(100-(-50))/200=0.75.
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 100),
            ObjectFit::Cover,
            ObjectPosition::default(),
        )
        .expect("cover visible");
        assert!(approx_rect(visible, 0.0, 0.0, 100.0, 100.0));
        assert!(approx_eq(uv0[0], 0.25) && approx_eq(uv0[1], 0.0));
        assert!(approx_eq(uv1[0], 0.75) && approx_eq(uv1[1], 1.0));
    }

    #[test]
    fn quad_none_with_oversized_image_crops_uvs() {
        // none при 200×200 в 100×100 — placement=200×200 at (-50,-50);
        // visible=box100; UV: 0.25..0.75 по обеим осям.
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 200),
            ObjectFit::None,
            ObjectPosition::default(),
        )
        .expect("none-larger visible");
        assert!(approx_rect(visible, 0.0, 0.0, 100.0, 100.0));
        assert!(approx_eq(uv0[0], 0.25) && approx_eq(uv0[1], 0.25));
        assert!(approx_eq(uv1[0], 0.75) && approx_eq(uv1[1], 0.75));
    }

    #[test]
    fn quad_zero_intrinsic_returns_none() {
        assert!(fit_image_quad(
            box100(),
            (0, 0),
            ObjectFit::Fill,
            ObjectPosition::default()
        )
        .is_none());
    }

    #[test]
    fn quad_serialize_includes_fit_and_position() {
        // Когда fit/position отличны от дефолтов — в snapshot-серилизатор
        // попадают «fit=» и «pos=» поля. Проверяем через ручной DisplayList,
        // чтобы не возиться с CSS-парсингом object-fit.
        let dl = vec![DisplayCommand::DrawImage {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            src: "x".into(),
            alt: String::new(),
            object_fit: ObjectFit::Cover,
            object_position: ObjectPosition {
                x: PositionComponent::Px(10.0),
                y: PositionComponent::Percent(0.0),
            },
            image_rendering: ImageRendering::Auto,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("fit=cover"), "{s}");
        assert!(s.contains("pos=10.00px 0.00%"), "{s}");
    }

    #[test]
    fn quad_serialize_omits_defaults() {
        let dl = vec![DisplayCommand::DrawImage {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            src: "x".into(),
            alt: String::new(),
            object_fit: ObjectFit::Fill,
            object_position: ObjectPosition::default(),
            image_rendering: ImageRendering::Auto,
        }];
        let s = serialize_display_list(&dl);
        assert!(!s.contains("fit="), "{s}");
        assert!(!s.contains("pos="), "{s}");
    }

    #[test]
    fn push_clip_rect_serializes() {
        let dl = vec![DisplayCommand::PushClipRect {
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
        }];
        let s = serialize_display_list(&dl);
        assert_eq!(s, "PushClipRect (10.00, 20.00, 100.00, 50.00)\n");
    }

    #[test]
    fn pop_clip_serializes() {
        let dl = vec![DisplayCommand::PopClip];
        assert_eq!(serialize_display_list(&dl), "PopClip\n");
    }

    #[test]
    fn push_opacity_serializes_with_alpha() {
        let dl = vec![DisplayCommand::PushOpacity { alpha: 0.5, bounds: None }];
        assert_eq!(serialize_display_list(&dl), "PushOpacity 0.500\n");
    }

    #[test]
    fn pop_opacity_serializes() {
        let dl = vec![DisplayCommand::PopOpacity];
        assert_eq!(serialize_display_list(&dl), "PopOpacity\n");
    }

    #[test]
    fn push_blend_mode_serializes_with_name() {
        let dl = vec![DisplayCommand::PushBlendMode {
            mode: BlendMode::Multiply,
            bounds: Rect::new(0.0, 0.0, 10.0, 20.0),
        }];
        assert_eq!(
            serialize_display_list(&dl),
            "PushBlendMode multiply bounds=(0,0,10,20)\n",
        );
    }

    #[test]
    fn pop_blend_mode_serializes() {
        let dl = vec![DisplayCommand::PopBlendMode];
        assert_eq!(serialize_display_list(&dl), "PopBlendMode\n");
    }

    #[test]
    fn blend_mode_from_keyword_all_16_modes() {
        let cases = [
            ("normal", BlendMode::Normal),
            ("multiply", BlendMode::Multiply),
            ("screen", BlendMode::Screen),
            ("overlay", BlendMode::Overlay),
            ("darken", BlendMode::Darken),
            ("lighten", BlendMode::Lighten),
            ("color-dodge", BlendMode::ColorDodge),
            ("color-burn", BlendMode::ColorBurn),
            ("hard-light", BlendMode::HardLight),
            ("soft-light", BlendMode::SoftLight),
            ("difference", BlendMode::Difference),
            ("exclusion", BlendMode::Exclusion),
            ("hue", BlendMode::Hue),
            ("saturation", BlendMode::Saturation),
            ("color", BlendMode::Color),
            ("luminosity", BlendMode::Luminosity),
        ];
        for (kw, expected) in cases {
            assert_eq!(
                BlendMode::from_keyword(kw),
                Some(expected),
                "keyword {kw:?} → {expected:?}"
            );
        }
    }

    #[test]
    fn blend_mode_from_keyword_case_insensitive() {
        assert_eq!(
            BlendMode::from_keyword("MULTIPLY"),
            Some(BlendMode::Multiply)
        );
        assert_eq!(
            BlendMode::from_keyword("Color-Dodge"),
            Some(BlendMode::ColorDodge)
        );
        assert_eq!(
            BlendMode::from_keyword("hArD-LiGhT"),
            Some(BlendMode::HardLight)
        );
    }

    #[test]
    fn blend_mode_from_keyword_unknown_returns_none() {
        assert_eq!(BlendMode::from_keyword(""), None);
        assert_eq!(BlendMode::from_keyword("bogus"), None);
        // CSS использует kebab-case с дефисом; underscore — не валидный
        assert_eq!(BlendMode::from_keyword("color_dodge"), None);
        // Без префикса/суффикса
        assert_eq!(BlendMode::from_keyword("dodge"), None);
        // С пробелами не парсим — должна быть отдельная команда trim caller-ом
        assert_eq!(BlendMode::from_keyword(" multiply "), None);
    }

    #[test]
    fn blend_mode_default_is_normal() {
        assert_eq!(BlendMode::default(), BlendMode::Normal);
    }

    #[test]
    fn nested_layer_ops_serialize_in_order() {
        let dl = vec![
            DisplayCommand::PushClipRect {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            DisplayCommand::PushOpacity { alpha: 0.7, bounds: None },
            DisplayCommand::FillRect {
                rect: Rect::new(10.0, 10.0, 50.0, 50.0),
                color: Color::BLACK,
            },
            DisplayCommand::PopOpacity,
            DisplayCommand::PopClip,
        ];
        let s = serialize_display_list(&dl);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "PushClipRect (0.00, 0.00, 100.00, 100.00)");
        assert_eq!(lines[1], "PushOpacity 0.700");
        assert!(lines[2].starts_with("FillRect"));
        assert_eq!(lines[3], "PopOpacity");
        assert_eq!(lines[4], "PopClip");
    }

