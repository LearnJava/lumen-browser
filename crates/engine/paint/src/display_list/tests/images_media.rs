//! P1/SPLIT-PT1: хвост `text_and_images.rs` — тесты `<img>` / DrawImage,
//! `loading="lazy"` / LazyImageSlot, `<video>` placeholder и `user-select` в
//! `::selection`. Перенесено байт-в-байт без дедента (приём ST-1/DL-1).

use super::*;
use super::text_and_images::{build, fills, Fixed8, BODY_RESET};

    // ── Тесты <img> / DrawImage ─────────────────────────────────────────────

    pub(crate) fn images(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }))
            .collect()
    }

    #[test]
    fn img_emits_draw_image() {
        let dl = build(r#"<img src="logo.png" alt="Logo" width="100" height="50">"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, src, alt, .. } = imgs[0] {
            assert_eq!(src, "logo.png");
            assert_eq!(alt, "Logo");
            assert!((rect.width - 100.0).abs() < 0.1);
            assert!((rect.height - 50.0).abs() < 0.1);
        }
    }

    #[test]
    fn img_with_background_and_border_paints_in_order() {
        // Painter's order для replaced element: FillRect (bg) → DrawBorder →
        // DrawImage. Image идёт последним, чтобы быть над фоном.
        let dl = build(
            r#"<img src="x" width="50" height="50">"#,
            "img { background: blue; border: 2px solid red; }",
        );
        // Должны присутствовать все три команды.
        let kinds: Vec<&str> = dl
            .iter()
            .map(|c| match c {
                DisplayCommand::FillRect { .. } => "FillRect",
                DisplayCommand::FillRoundedRect { .. } => "FillRoundedRect",
                DisplayCommand::DrawBorder { .. } => "DrawBorder",
                DisplayCommand::DrawOutline { .. } => "DrawOutline",
                DisplayCommand::DrawImage { .. } => "DrawImage",
                DisplayCommand::DrawBackgroundImage { .. } => "DrawBackgroundImage",
                DisplayCommand::DrawText { .. } => "DrawText",
                DisplayCommand::PushClipRect { .. } => "PushClipRect",
                DisplayCommand::PushClipRoundedRect { .. } => "PushClipRoundedRect",
                DisplayCommand::PushClipPath { .. } => "PushClipPath",
                DisplayCommand::PopClip => "PopClip",
                DisplayCommand::PushOpacity { .. } => "PushOpacity",
                DisplayCommand::PopOpacity => "PopOpacity",
                DisplayCommand::PushBlendMode { .. } => "PushBlendMode",
                DisplayCommand::PopBlendMode => "PopBlendMode",
                DisplayCommand::DrawLayerSnapshot { .. } => "DrawLayerSnapshot",
                DisplayCommand::PushTransform { .. } => "PushTransform",
                DisplayCommand::PopTransform => "PopTransform",
                DisplayCommand::DrawLinearGradient { .. } => "DrawLinearGradient",
                DisplayCommand::DrawRadialGradient { .. } => "DrawRadialGradient",
                DisplayCommand::DrawConicGradient { .. } => "DrawConicGradient",
                DisplayCommand::PushMaskImage { .. } => "PushMaskImage",
                DisplayCommand::PushMaskLinearGradient { .. } => "PushMaskLinearGradient",
                DisplayCommand::PushMaskRadialGradient { .. } => "PushMaskRadialGradient",
                DisplayCommand::PushMaskConicGradient { .. } => "PushMaskConicGradient",
                DisplayCommand::PopMask => "PopMask",
                DisplayCommand::PushMaskLayer { .. } => "PushMaskLayer",
                DisplayCommand::PopMaskLayer => "PopMaskLayer",
                DisplayCommand::PushFilter { .. } => "PushFilter",
                DisplayCommand::PopFilter => "PopFilter",
                DisplayCommand::PushBackdropFilter { .. } => "PushBackdropFilter",
                DisplayCommand::PopBackdropFilter => "PopBackdropFilter",
                DisplayCommand::BeginStickyLayer { .. } => "BeginStickyLayer",
                DisplayCommand::EndStickyLayer => "EndStickyLayer",
                DisplayCommand::BeginFixedLayer => "BeginFixedLayer",
                DisplayCommand::EndFixedLayer => "EndFixedLayer",
                DisplayCommand::BeginFixedBackground => "BeginFixedBackground",
                DisplayCommand::EndFixedBackground => "EndFixedBackground",
                DisplayCommand::PushScrollLayer { .. } => "PushScrollLayer",
                DisplayCommand::PopScrollLayer => "PopScrollLayer",
                DisplayCommand::DrawSvgPath { .. } => "DrawSvgPath",
                DisplayCommand::DrawSvgFill { .. } => "DrawSvgFill",
                DisplayCommand::DrawSvgStroke { .. } => "DrawSvgStroke",
                DisplayCommand::BoxModelOverlay { .. } => "BoxModelOverlay",
                DisplayCommand::DrawScrollbar { .. } => "DrawScrollbar",
                DisplayCommand::PageBreak => "PageBreak",
                DisplayCommand::DrawCrossFade { .. } => "DrawCrossFade",
                DisplayCommand::LazyImageSlot { .. } => "LazyImageSlot",
            })
            .collect();
        assert_eq!(kinds, vec!["FillRect", "DrawBorder", "DrawImage"]);
    }

    #[test]
    fn img_serialize_includes_src_and_alt() {
        let dl = build(
            r#"<img src="photo.jpg" alt="A photo" width="80" height="40">"#,
            "",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawImage"), "must contain DrawImage line");
        assert!(s.contains(r#"src="photo.jpg""#), "must contain src");
        assert!(s.contains(r#"alt="A photo""#), "must contain alt");
    }

    /// BUG-431: the bitmap belongs in the content box, same rule as `<canvas>`
    /// (BUG-099) — painting at the border box slid it under the border+padding.
    #[test]
    fn img_bitmap_is_painted_into_the_content_box() {
        let dl = build(
            r#"<img src="x" width="100" height="80">"#,
            "*{margin:0}img{border:10px solid red;padding:5px}",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (15.0, 15.0, 100.0, 80.0));
        }
    }

    // ── Тесты loading="lazy" / LazyImageSlot ───────────────────────────────

    fn lazy_slots(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::LazyImageSlot { .. }))
            .collect()
    }

    #[test]
    fn lazy_img_emits_lazy_image_slot_not_draw_image() {
        let dl = build(
            r#"<img src="hero.jpg" loading="lazy" width="200" height="100">"#,
            "",
        );
        // Must emit LazyImageSlot, not DrawImage.
        assert!(lazy_slots(&dl).len() == 1, "expected one LazyImageSlot");
        assert!(images(&dl).is_empty(), "must not emit DrawImage for lazy img");
    }

    #[test]
    fn eager_img_still_emits_draw_image() {
        let dl = build(r#"<img src="thumb.jpg" width="80" height="40">"#, "");
        assert!(images(&dl).len() == 1, "non-lazy img must emit DrawImage");
        assert!(lazy_slots(&dl).is_empty(), "non-lazy must not emit LazyImageSlot");
    }

    #[test]
    fn lazy_img_slot_has_correct_src_and_rect() {
        let dl = build(
            r#"<img src="banner.png" loading="lazy" width="300" height="150">"#,
            "",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { src, rect, .. } = slots[0] {
            assert_eq!(src, "banner.png");
            assert!((rect.width - 300.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 150.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    /// BUG-431: the lazy slot's rect is later reused as the loaded image's
    /// paint rect (shell BUG-163), so it must be content-box too, not just
    /// the eager `DrawImage` path.
    #[test]
    fn lazy_img_slot_is_content_box() {
        let dl = build(
            r#"<img src="banner.png" loading="lazy" width="300" height="150">"#,
            "*{margin:0}img{border:10px solid red;padding:5px}",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { rect, .. } = slots[0] {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (15.0, 15.0, 300.0, 150.0));
        }
    }

    #[test]
    fn lazy_img_case_insensitive() {
        let dl = build(
            r#"<img src="poster.jpg" loading="LAZY" width="50" height="50">"#,
            "",
        );
        assert_eq!(lazy_slots(&dl).len(), 1, "LAZY (uppercase) must emit LazyImageSlot");
    }

    #[test]
    fn lazy_img_node_id_set() {
        let dl = build(
            r#"<img src="lazy.png" loading="lazy" width="100" height="100">"#,
            "",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { node_id, .. } = slots[0] {
            // node_id must be > 0 (document root is 0; img elements get a non-zero id).
            assert!(*node_id > 0, "lazy img node_id must be non-zero, got {node_id}");
        }
    }

    #[test]
    fn lazy_img_slot_carries_object_fit() {
        // BUG-163: a lazy <img> keeps its loading="lazy" attribute even after the
        // shell fetches it, so it is painted via LazyImageSlot (not DrawImage).
        // The slot must therefore carry object_fit/object_position so the backend
        // can draw the loaded image with the correct CSS fitting, not a raw fill.
        let dl = build(
            r#"<img src="cover.jpg" loading="lazy" width="200" height="100" style="object-fit: cover">"#,
            "",
        );
        let slots = lazy_slots(&dl);
        assert_eq!(slots.len(), 1);
        if let DisplayCommand::LazyImageSlot { object_fit, .. } = slots[0] {
            assert_eq!(*object_fit, ObjectFit::Cover, "lazy slot must carry object-fit");
        } else {
            panic!("expected LazyImageSlot");
        }
    }

    #[test]
    fn lazy_img_serialize_contains_lazy_image_slot() {
        let dl = build(
            r#"<img src="deferred.jpg" loading="lazy" width="100" height="50">"#,
            "",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("LazyImageSlot"), "serialize must include LazyImageSlot");
        assert!(s.contains(r#"src="deferred.jpg""#), "serialize must include src");
    }

    // ── Тесты <video> / DrawImage placeholder ───────────────────────────────

    #[test]
    fn video_without_poster_emits_no_draw_image() {
        // BUG-097: an empty <video> (no poster, no decoded frame) paints nothing —
        // the element box is transparent, matching Chromium/Edge. The grey image
        // placeholder is reserved for <img>, not media.
        let dl = build(r#"<video src="clip.mp4"></video>"#, "");
        let imgs = images(&dl);
        assert!(
            imgs.is_empty(),
            "posterless video should emit no DrawImage, got {}",
            imgs.len()
        );
    }

    #[test]
    fn video_with_poster_emits_draw_image_with_poster_src() {
        // When poster is set, DrawImage uses the poster URL so shell can register it.
        let dl = build(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { src, .. } = imgs[0] {
            assert_eq!(src, "thumb.jpg");
        }
    }

    #[test]
    fn video_ua_default_rect_300_by_150() {
        // Poster present so the replaced box paints a DrawImage at the UA-default rect.
        let dl = build(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!((rect.width - 300.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 150.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    #[test]
    fn video_css_dimensions_override_ua_default() {
        let dl = build(
            r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#,
            "video { width: 640px; height: 360px; }",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!((rect.width - 640.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 360.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    /// BUG-431: the poster frame belongs in the content box, same rule as
    /// `<img>`/`<canvas>` — painting at the border box slid it under the
    /// border+padding.
    #[test]
    fn video_poster_is_painted_into_the_content_box() {
        let dl = build(
            r#"<video src="clip.mp4" poster="thumb.jpg" width="100" height="80"></video>"#,
            "*{margin:0}video{border:10px solid red;padding:5px}",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (15.0, 15.0, 100.0, 80.0));
        }
    }


    // ── CSS UI L4 §6.2: `user-select` in the `::selection` highlight ────────

    /// Fill rects emitted by `build_display_list_with_selection` over the
    /// selection `first text node .. last text node` of `<p>`'s children.
    fn selection_fills(css: &str) -> usize {
        let doc = lumen_html_parser::parse("<p>aa <span>bb</span> cc</p>");
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
        let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        fn collect_texts(
            doc: &lumen_dom::Document,
            n: lumen_dom::NodeId,
            out: &mut Vec<lumen_dom::NodeId>,
        ) {
            if matches!(doc.get(n).data, lumen_dom::NodeData::Text(_)) {
                out.push(n);
            }
            for &c in &doc.get(n).children {
                collect_texts(doc, c, out);
            }
        }
        let mut texts = Vec::new();
        collect_texts(&doc, doc.root(), &mut texts);
        let (first, last) = (texts[0], *texts.last().unwrap());
        let sel = SelectionHighlight {
            range: lumen_dom::Range {
                start: lumen_dom::DomPosition { container: first, offset: 0 },
                end: lumen_dom::DomPosition { container: last, offset: 3 },
            },
            fg_color: None,
            bg_color: Color { r: 1, g: 2, b: 3, a: 255 },
        };
        let base = fills(&build_display_list(&tree)).len();
        let with = build_display_list_with_selection(&tree, Some(&sel));
        fills(&with).len() - base
    }

    #[test]
    fn selection_highlights_every_node_between_the_endpoints() {
        // "aa " + "bb" (between, neither endpoint) + " cc" merge into ONE
        // same-style fragment whose own node is the start endpoint — one fill
        // covering all of it (the `bb` node is not dropped).
        assert_eq!(selection_fills(""), 1);
    }

    #[test]
    fn selection_skips_user_select_none_text() {
        // `bb` no longer merges with its neighbours and gets no fill: "aa " and
        // " cc" are highlighted, the `none` span between them is not.
        assert_eq!(selection_fills("span { user-select: none }"), 2);
    }
