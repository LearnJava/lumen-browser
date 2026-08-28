//! Тесты `style.rs`: изображения и фоны: слои `background`, градиенты, `image-set()`,
//! `transform`/`filter`.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // -------- background-position (CSS Backgrounds L3 §3.5) --------

    #[test]
    fn background_position_default_is_top_left() {
        // CSS Backgrounds L3 §3.5 — initial state: no layers (empty Vec).
        // Default position is 0% 0% (from BackgroundLayer::default), applied when a layer exists.
        let s = cascade_at("<div></div>", "", &[0]);
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_position_two_percent_values() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 25% 75%; }",
            &[0],
        );
        assert_eq!(s.background_layers[0].position.x, PositionComponent::Percent(0.25));
        assert_eq!(s.background_layers[0].position.y, PositionComponent::Percent(0.75));
    }

    #[test]
    fn background_position_two_lengths() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 10px 20px; }",
            &[0],
        );
        assert_eq!(s.background_layers[0].position.x, PositionComponent::Px(10.0));
        assert_eq!(s.background_layers[0].position.y, PositionComponent::Px(20.0));
    }

    #[test]
    fn background_position_single_value_centers_y() {
        // Один token — второй компонент defaults to `center` (50%).
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 30%; }",
            &[0],
        );
        assert_eq!(s.background_layers[0].position.x, PositionComponent::Percent(0.30));
        assert_eq!(s.background_layers[0].position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn background_position_keyword_right_bottom() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: right bottom; }",
            &[0],
        );
        assert_eq!(s.background_layers[0].position.x, PositionComponent::Percent(1.0));
        assert_eq!(s.background_layers[0].position.y, PositionComponent::Percent(1.0));
    }

    #[test]
    fn background_position_keyword_center() {
        let s = cascade_at(
            "<div></div>",
            "div { background-position: center; }",
            &[0],
        );
        assert_eq!(s.background_layers[0].position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.background_layers[0].position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn background_position_invalid_value_ignored() {
        // Невалидное value → declaration invalid → no layer created.
        let s = cascade_at(
            "<div></div>",
            "div { background-position: bogus; }",
            &[0],
        );
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_position_not_inherited() {
        // CSS Backgrounds L3 — non-inherited; ребёнок без своей декларации
        // получает initial (empty layers), а не родительское `right bottom`.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-position: right bottom; }",
            &[0, 0],
        );
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_position_inherit_keyword_pulls_parent_value() {
        // CSS Cascade L4 §7 — `inherit` принудительно тянет parent value
        // даже для non-inherited.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-position: center; } p { background-position: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.background_layers[0].position.x, PositionComponent::Percent(0.5));
        assert_eq!(s.background_layers[0].position.y, PositionComponent::Percent(0.5));
    }

    #[test]
    fn background_position_initial_resets_to_top_left() {
        // `initial` resets background-position → empty layers.
        let s = cascade_at(
            "<div></div>",
            "div { background-position: 80% 90%; background-position: initial; }",
            &[0],
        );
        assert!(s.background_layers.is_empty());
    }

    // -------- image-rendering (CSS Images L3 §6.1) --------

    #[test]
    fn image_rendering_default_is_auto() {
        let s = cascade_at("<img>", "", &[0]);
        assert_eq!(s.image_rendering, ImageRendering::Auto);
    }

    #[test]
    fn image_rendering_all_keywords_parse() {
        for (val, expected) in [
            ("auto", ImageRendering::Auto),
            ("smooth", ImageRendering::Smooth),
            ("high-quality", ImageRendering::HighQuality),
            ("crisp-edges", ImageRendering::CrispEdges),
            ("pixelated", ImageRendering::Pixelated),
        ] {
            let s = cascade_at(
                "<img>",
                &format!("img {{ image-rendering: {val}; }}"),
                &[0],
            );
            assert_eq!(s.image_rendering, expected, "for value {val}");
        }
    }

    #[test]
    fn image_rendering_case_insensitive() {
        let s = cascade_at(
            "<img>",
            "img { image-rendering: PIXELATED; }",
            &[0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Pixelated);
    }

    #[test]
    fn image_rendering_invalid_value_ignored() {
        let s = cascade_at(
            "<img>",
            "img { image-rendering: bogus; }",
            &[0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Auto);
    }

    #[test]
    fn image_rendering_inherited() {
        // CSS Images L3 §6.1 — inherited. Ребёнок без своей декларации
        // получает значение от родителя.
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: pixelated; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Pixelated);
    }

    #[test]
    fn image_rendering_child_override_wins() {
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: pixelated; } img { image-rendering: smooth; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Smooth);
    }

    #[test]
    fn image_rendering_initial_keyword_resets() {
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: pixelated; } img { image-rendering: initial; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::Auto);
    }

    #[test]
    fn image_rendering_unset_for_inherited_is_inherit() {
        // CSS Cascade L4 §7: `unset` для inherited-свойства == `inherit`.
        let s = cascade_at(
            "<div><img></div>",
            "div { image-rendering: crisp-edges; } img { image-rendering: unset; }",
            &[0, 0],
        );
        assert_eq!(s.image_rendering, ImageRendering::CrispEdges);
    }

    // ── CSS Backgrounds L3 §3.7 / §3.8 — background-origin / background-clip ──

    #[test]
    fn background_origin_default_is_padding_box() {
        // Initial state = empty layers. PaddingBox is default inside BackgroundLayer::default().
        let s = cascade_at("<div></div>", "", &[0]);
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_origin_all_keywords_parse() {
        for (val, expected) in [
            ("border-box", BackgroundOrigin::BorderBox),
            ("padding-box", BackgroundOrigin::PaddingBox),
            ("content-box", BackgroundOrigin::ContentBox),
        ] {
            let s = cascade_at(
                "<div></div>",
                &format!("div {{ background-origin: {val}; }}"),
                &[0],
            );
            assert_eq!(s.background_layers[0].origin, expected, "for value {val}");
        }
    }

    #[test]
    fn background_origin_case_insensitive() {
        let s = cascade_at("<div></div>", "div { background-origin: BORDER-BOX; }", &[0]);
        assert_eq!(s.background_layers[0].origin, BackgroundOrigin::BorderBox);
    }

    #[test]
    fn background_origin_invalid_value_ignored() {
        // Invalid value → no layer created.
        let s = cascade_at("<div></div>", "div { background-origin: bogus; }", &[0]);
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_origin_not_inherited() {
        // CSS Backgrounds L3 §3.7 — non-inherited; child gets empty layers.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-origin: content-box; }",
            &[0, 0],
        );
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_origin_inherit_keyword_takes_parent() {
        // `inherit` явно тянет значение родителя даже для non-inherited.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-origin: content-box; } p { background-origin: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.background_layers[0].origin, BackgroundOrigin::ContentBox);
    }

    #[test]
    fn background_origin_initial_keyword_resets() {
        // `initial` clears layers → empty Vec.
        let s = cascade_at(
            "<div></div>",
            "div { background-origin: content-box; } div { background-origin: initial; }",
            &[0],
        );
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_origin_unset_for_non_inherited_is_initial() {
        // CSS Cascade L4 §7: `unset` для non-inherited == `initial` → empty layers.
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-origin: content-box; } p { background-origin: unset; }",
            &[0, 0],
        );
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_clip_default_is_border_box() {
        // Initial state = empty layers. BorderBox is default inside BackgroundLayer::default().
        let s = cascade_at("<div></div>", "", &[0]);
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_clip_all_keywords_parse() {
        for (val, expected) in [
            ("border-box", BackgroundClip::BorderBox),
            ("padding-box", BackgroundClip::PaddingBox),
            ("content-box", BackgroundClip::ContentBox),
            ("text", BackgroundClip::Text),
        ] {
            let s = cascade_at(
                "<div></div>",
                &format!("div {{ background-clip: {val}; }}"),
                &[0],
            );
            assert_eq!(s.background_layers[0].clip, expected, "for value {val}");
        }
    }

    #[test]
    fn background_clip_case_insensitive() {
        let s = cascade_at("<div></div>", "div { background-clip: TEXT; }", &[0]);
        assert_eq!(s.background_layers[0].clip, BackgroundClip::Text);
    }

    #[test]
    fn background_clip_invalid_value_ignored() {
        // Invalid value → no layer created.
        let s = cascade_at("<div></div>", "div { background-clip: bogus; }", &[0]);
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_clip_not_inherited() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-clip: padding-box; }",
            &[0, 0],
        );
        assert!(s.background_layers.is_empty());
    }

    #[test]
    fn background_clip_inherit_keyword_takes_parent() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { background-clip: text; } p { background-clip: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.background_layers[0].clip, BackgroundClip::Text);
    }

    #[test]
    fn background_clip_initial_keyword_resets() {
        // `initial` clears layers → empty Vec.
        let s = cascade_at(
            "<div></div>",
            "div { background-clip: text; } div { background-clip: initial; }",
            &[0],
        );
        assert!(s.background_layers.is_empty());
    }

    // ── CSS Backgrounds L3 §3 — multiple backgrounds ──

    #[test]
    fn multiple_backgrounds_two_images_in_shorthand() {
        // CSS Backgrounds L3 §3: comma-separated layers. First = top layer.
        let s = cascade_at(
            "<div></div>",
            "div { background: url(a.png), url(b.png); }",
            &[0],
        );
        assert_eq!(s.background_layers.len(), 2);
        assert_eq!(s.background_layers[0].image, BackgroundImage::Url("a.png".into()));
        assert_eq!(s.background_layers[1].image, BackgroundImage::Url("b.png".into()));
    }

    #[test]
    fn multiple_backgrounds_image_list_property() {
        // background-image: comma list creates matching layers.
        let s = cascade_at(
            "<div></div>",
            "div { background-image: url(x.png), url(y.png), url(z.png); }",
            &[0],
        );
        assert_eq!(s.background_layers.len(), 3);
        assert_eq!(s.background_layers[2].image, BackgroundImage::Url("z.png".into()));
    }

    #[test]
    fn multiple_backgrounds_color_only_in_last_layer() {
        // background shorthand: color applies only from last layer's declaration.
        let s = cascade_at(
            "<div></div>",
            "div { background: url(a.png) no-repeat, red; }",
            &[0],
        );
        assert_eq!(s.background_layers.len(), 2);
        assert_eq!(s.background_layers[0].repeat, BackgroundRepeat::NoRepeat);
        // Last layer had the color "red"
        assert!(s.background_color.is_some());
    }

    #[test]
    fn multiple_backgrounds_shorthand_with_position_size() {
        // background: url(...) 50% 50% / cover no-repeat
        let s = cascade_at(
            "<div></div>",
            "div { background: url(img.png) 50% 50% / cover no-repeat; }",
            &[0],
        );
        assert_eq!(s.background_layers.len(), 1);
        let layer = &s.background_layers[0];
        assert_eq!(layer.image, BackgroundImage::Url("img.png".into()));
        assert_eq!(layer.position.x, PositionComponent::Percent(0.5));
        assert_eq!(layer.position.y, PositionComponent::Percent(0.5));
        assert_eq!(layer.size, BackgroundSize::Cover);
        assert_eq!(layer.repeat, BackgroundRepeat::NoRepeat);
    }

    #[test]
    fn multiple_backgrounds_repeat_cycles_over_layers() {
        // background-image with 3 images, background-repeat with 2 → cycles.
        let s = cascade_at(
            "<div></div>",
            "div { background-image: url(a.png), url(b.png), url(c.png); \
             background-repeat: no-repeat, repeat-x; }",
            &[0],
        );
        assert_eq!(s.background_layers.len(), 3);
        assert_eq!(s.background_layers[0].repeat, BackgroundRepeat::NoRepeat);
        assert_eq!(s.background_layers[1].repeat, BackgroundRepeat::RepeatX);
        assert_eq!(s.background_layers[2].repeat, BackgroundRepeat::NoRepeat); // cycles
    }

    #[test]
    fn background_shorthand_resets_all_layers() {
        // Setting background shorthand clears previous layers.
        let s = cascade_at(
            "<div></div>",
            "div { background-image: url(a.png), url(b.png); \
             background: url(c.png); }",
            &[0],
        );
        assert_eq!(s.background_layers.len(), 1);
        assert_eq!(s.background_layers[0].image, BackgroundImage::Url("c.png".into()));
    }

    // ── CSS Images L4 §5 — image-set() / §4 — cross-fade() ──

    #[test]
    fn image_set_stored_as_raw_url_string() {
        // CSS Images L4 §5: image-set() stored verbatim so paint can resolve per-DPR.
        let s = cascade_at(
            "<div></div>",
            r#"div { background-image: image-set("a.png" 1x, "a@2x.png" 2x); }"#,
            &[0],
        );
        assert_eq!(s.background_layers.len(), 1);
        assert!(
            matches!(&s.background_layers[0].image, BackgroundImage::Url(u) if u.contains("image-set(")),
            "image-set() should be stored as Url containing the raw function string"
        );
    }

    #[test]
    fn webkit_image_set_stored_as_raw_url_string() {
        // -webkit-image-set() vendor prefix handled identically.
        let s = cascade_at(
            "<div></div>",
            r#"div { background-image: -webkit-image-set(url("x.png") 1x); }"#,
            &[0],
        );
        assert_eq!(s.background_layers.len(), 1);
        assert!(
            matches!(&s.background_layers[0].image, BackgroundImage::Url(u) if u.contains("image-set(")),
            "-webkit-image-set() should be stored as Url containing the raw function string"
        );
    }

    #[test]
    fn cross_fade_l4_two_images_with_percentage() {
        // CSS Images L4 §4: cross-fade( <image> <pct>?, <image> ) → CrossFade.
        // `url(a) 30%` ⇒ a at 30% opacity, b at 70% ⇒ t (fraction of b) = 0.70.
        let s = cascade_at(
            "<div></div>",
            r#"div { background-image: cross-fade(url("a.png") 30%, url("b.png")); }"#,
            &[0],
        );
        assert_eq!(s.background_layers.len(), 1);
        match &s.background_layers[0].image {
            BackgroundImage::CrossFade { a, b, t } => {
                assert_eq!(a.as_ref(), &BackgroundImage::Url("a.png".into()));
                assert_eq!(b.as_ref(), &BackgroundImage::Url("b.png".into()));
                assert!((t - 0.70).abs() < 0.001, "t should be 0.70, got {t}");
            }
            other => panic!("expected CrossFade, got {other:?}"),
        }
    }

    #[test]
    fn cross_fade_unprefixed_legacy_three_arg_rejected() {
        // The legacy three-argument webkit form WITHOUT the -webkit- prefix is
        // invalid per CSS Images L4 (trailing bare `<percentage>` is not an
        // `<image>`). Edge/Chromium drop the declaration; Lumen must too —
        // BUG-101 / TEST-59. The cell stays empty (BackgroundImage::None).
        let s = cascade_at(
            "<div></div>",
            r#"div { background-image: cross-fade(url("a.png"), url("b.png"), 30%); }"#,
            &[0],
        );
        assert_eq!(s.background_layers.len(), 1);
        assert!(
            matches!(&s.background_layers[0].image, BackgroundImage::None),
            "unprefixed legacy 3-arg cross-fade() must be invalid → None, got {:?}",
            s.background_layers[0].image
        );
    }

    #[test]
    fn webkit_cross_fade_parsed() {
        // -webkit-cross-fade(<from>, <to>, <pct>) vendor-prefixed legacy form.
        let s = cascade_at(
            "<div></div>",
            r#"div { background-image: -webkit-cross-fade(url("x.png"), url("y.png"), 50%); }"#,
            &[0],
        );
        assert_eq!(s.background_layers.len(), 1);
        assert!(
            matches!(&s.background_layers[0].image, BackgroundImage::CrossFade { t, .. } if (t - 0.5).abs() < 0.001),
            "expected CrossFade with t≈0.5"
        );
    }

    #[test]
    fn cross_fade_t_clamped_to_unit_interval() {
        // An out-of-range opacity percentage is clamped into 0.0..=1.0.
        let s = cascade_at(
            "<div></div>",
            r#"div { background-image: -webkit-cross-fade(url("a.png"), url("b.png"), 150%); }"#,
            &[0],
        );
        if let BackgroundImage::CrossFade { t, .. } = &s.background_layers[0].image {
            assert!(*t <= 1.0, "t should be clamped to ≤ 1.0, got {t}");
        } else {
            panic!("expected CrossFade");
        }
    }

    // --- backdrop-filter ---

    #[test]
    fn backdrop_filter_blur() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { backdrop-filter: blur(4px); }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(!style.backdrop_filter.is_empty());
        assert!(matches!(style.backdrop_filter[0], FilterFn::Blur(_)));
    }

    #[test]
    fn backdrop_filter_none() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("div { backdrop-filter: none; }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(style.backdrop_filter.is_empty());
    }

    #[test]
    fn backdrop_filter_initial() {
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse("");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        assert!(style.backdrop_filter.is_empty());
    }

    #[test]
    fn backdrop_filter_not_inherited() {
        let doc = lumen_html_parser::parse("<div><span></span></div>");
        let sheet = lumen_css_parser::parse("div { backdrop-filter: blur(4px); }");
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, Size::new(800.0, 600.0), false);
        let span = doc.get(div).children[0];
        let span_style = compute_style(&doc, span, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert!(!div_style.backdrop_filter.is_empty());
        assert!(span_style.backdrop_filter.is_empty());
    }

    // ── CSS Compositing L1 §8.3 — background-blend-mode ──

    #[test]
    fn background_blend_mode_default_is_normal() {
        let layer = BackgroundLayer::default();
        assert_eq!(layer.blend_mode, MixBlendMode::Normal);
    }

    #[test]
    fn background_blend_mode_parse_all_keywords() {
        // MixBlendMode::parse covers all CSS keywords for background-blend-mode.
        let cases = [
            ("normal", MixBlendMode::Normal),
            ("multiply", MixBlendMode::Multiply),
            ("screen", MixBlendMode::Screen),
            ("overlay", MixBlendMode::Overlay),
            ("darken", MixBlendMode::Darken),
            ("lighten", MixBlendMode::Lighten),
            ("color-dodge", MixBlendMode::ColorDodge),
            ("color-burn", MixBlendMode::ColorBurn),
            ("hard-light", MixBlendMode::HardLight),
            ("soft-light", MixBlendMode::SoftLight),
            ("difference", MixBlendMode::Difference),
            ("exclusion", MixBlendMode::Exclusion),
            ("hue", MixBlendMode::Hue),
            ("saturation", MixBlendMode::Saturation),
            ("color", MixBlendMode::Color),
            ("luminosity", MixBlendMode::Luminosity),
            ("plus-lighter", MixBlendMode::PlusLighter),
        ];
        for (kw, expected) in cases {
            assert_eq!(MixBlendMode::parse(kw), Some(expected), "keyword: {kw}");
        }
    }

    #[test]
    fn background_blend_mode_parse_invalid_returns_none() {
        assert_eq!(MixBlendMode::parse("bogus"), None);
        assert_eq!(MixBlendMode::parse("color_dodge"), None);
        assert_eq!(MixBlendMode::parse(""), None);
    }

    #[test]
    fn background_blend_mode_parse_case_insensitive() {
        assert_eq!(MixBlendMode::parse("MULTIPLY"), Some(MixBlendMode::Multiply));
        assert_eq!(MixBlendMode::parse("Color-Dodge"), Some(MixBlendMode::ColorDodge));
    }

    #[test]
    fn background_blend_mode_cycling_direct() {
        // Verify that BackgroundLayer stores blend_mode and it cycles correctly.
        let mut layers: Vec<BackgroundLayer> = (0..3)
            .map(|_| BackgroundLayer::default())
            .collect();
        let modes = [MixBlendMode::Multiply, MixBlendMode::Screen];
        let n = modes.len();
        for (i, layer) in layers.iter_mut().enumerate() {
            layer.blend_mode = modes[i % n];
        }
        assert_eq!(layers[0].blend_mode, MixBlendMode::Multiply);
        assert_eq!(layers[1].blend_mode, MixBlendMode::Screen);
        assert_eq!(layers[2].blend_mode, MixBlendMode::Multiply);
    }

    #[test]
    fn background_blend_mode_field_roundtrip() {
        // BackgroundLayer preserves blend_mode through clone (used by ..old spread in bg-image).
        let layer = BackgroundLayer { blend_mode: MixBlendMode::Overlay, ..BackgroundLayer::default() };
        let cloned = layer.clone();
        assert_eq!(cloned.blend_mode, MixBlendMode::Overlay);
        // Spread syntax: new layer from old preserves blend_mode.
        let new_layer = BackgroundLayer { image: BackgroundImage::None, ..cloned };
        assert_eq!(new_layer.blend_mode, MixBlendMode::Overlay);
    }

    // --- CSS 3D transforms: TransformFn variants ---

    #[test]
    fn parse_transform_translatez() {
        let t = parse_transform_list("translateZ(50px)");
        assert_eq!(t, vec![TransformFn::TranslateZ(50.0)]);
    }

    #[test]
    fn parse_transform_translate3d() {
        let t = parse_transform_list("translate3d(10px, 20px, 30px)");
        assert_eq!(t, vec![TransformFn::Translate3d(10.0, 20.0, 30.0)]);
    }

    #[test]
    fn parse_transform_rotatex() {
        let t = parse_transform_list("rotateX(45deg)");
        assert!(matches!(t[..], [TransformFn::RotateX(_)]));
        if let TransformFn::RotateX(a) = t[0] {
            assert!((a - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
        }
    }

    #[test]
    fn parse_transform_rotatey() {
        let t = parse_transform_list("rotateY(90deg)");
        assert!(matches!(t[..], [TransformFn::RotateY(_)]));
    }

    #[test]
    fn parse_transform_rotatez() {
        let t = parse_transform_list("rotateZ(180deg)");
        assert!(matches!(t[..], [TransformFn::RotateZ(_)]));
    }

    #[test]
    fn parse_transform_rotate3d() {
        let t = parse_transform_list("rotate3d(1, 0, 0, 45deg)");
        assert!(matches!(t[..], [TransformFn::Rotate3d(_, _, _, _)]));
        if let TransformFn::Rotate3d(x, y, z, _) = t[0] {
            assert_eq!((x, y, z), (1.0, 0.0, 0.0));
        }
    }

    #[test]
    fn parse_transform_scale3d() {
        let t = parse_transform_list("scale3d(2, 3, 4)");
        assert_eq!(t, vec![TransformFn::Scale3d(2.0, 3.0, 4.0)]);
    }

    #[test]
    fn parse_transform_scalez() {
        let t = parse_transform_list("scaleZ(0.5)");
        assert_eq!(t, vec![TransformFn::ScaleZ(0.5)]);
    }

    #[test]
    fn parse_transform_matrix3d() {
        // identity matrix3d
        let t = parse_transform_list(
            "matrix3d(1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1)"
        );
        assert!(matches!(t[..], [TransformFn::Matrix3d(_)]));
    }

    #[test]
    fn parse_transform_perspective_fn() {
        let t = parse_transform_list("perspective(800px)");
        assert_eq!(t, vec![TransformFn::Perspective(800.0)]);
    }

    #[test]
    fn parse_transform_style_flat_default() {
        let s = ts_prop("transform-style", "flat");
        assert_eq!(s.transform_style, TransformStyle::Flat);
    }

    #[test]
    fn parse_transform_style_preserve_3d() {
        let s = ts_prop("transform-style", "preserve-3d");
        assert_eq!(s.transform_style, TransformStyle::Preserve3d);
    }

    #[test]
    fn parse_perspective_origin_default() {
        let s = ComputedStyle::root();
        assert_eq!(
            s.perspective_origin,
            (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5))
        );
    }

    #[test]
    fn parse_perspective_origin_keywords() {
        let s = ts_prop("perspective-origin", "left top");
        assert_eq!(s.perspective_origin.0, PositionComponent::Percent(0.0));
        assert_eq!(s.perspective_origin.1, PositionComponent::Percent(0.0));
    }

    #[test]
    fn parse_perspective_origin_percent() {
        let s = ts_prop("perspective-origin", "25% 75%");
        assert_eq!(s.perspective_origin.0, PositionComponent::Percent(0.25));
        assert_eq!(s.perspective_origin.1, PositionComponent::Percent(0.75));
    }

    #[test]
    fn parse_backface_visibility_hidden() {
        let s = ts_prop("backface-visibility", "hidden");
        assert_eq!(s.backface_visibility, BackfaceVisibility::Hidden);
        // Case-insensitive per CSS keyword rules.
        let s = ts_prop("backface-visibility", "HIDDEN");
        assert_eq!(s.backface_visibility, BackfaceVisibility::Hidden);
    }

    #[test]
    fn parse_backface_visibility_initial_visible() {
        let s = ComputedStyle::root();
        assert_eq!(s.backface_visibility, BackfaceVisibility::Visible);
        let s = ts_prop("backface-visibility", "visible");
        assert_eq!(s.backface_visibility, BackfaceVisibility::Visible);
    }

    #[test]
    fn parse_backface_visibility_invalid_ignored() {
        // Invalid value → declaration ignored, prior value kept.
        let mut s = ComputedStyle::root();
        let vp = Size::new(800.0, 600.0);
        let root = ComputedStyle::root();
        let hidden = Declaration { property: "backface-visibility".to_string(), value: "hidden".to_string(), important: false };
        apply_declaration(&mut s, &hidden, 16.0, vp, FontWeight::NORMAL, &root, &root, false, false);
        let bogus = Declaration { property: "backface-visibility".to_string(), value: "translucent".to_string(), important: false };
        apply_declaration(&mut s, &bogus, 16.0, vp, FontWeight::NORMAL, &root, &root, false, false);
        assert_eq!(s.backface_visibility, BackfaceVisibility::Hidden);
    }

    #[test]
    fn backface_visibility_not_inherited() {
        // backface-visibility — non-inherited (CSS Transforms L2 §5.1).
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { backface-visibility: hidden; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        assert_eq!(div_style.backface_visibility, BackfaceVisibility::Hidden);
        assert_eq!(p_style.backface_visibility, BackfaceVisibility::Visible);
    }

    // ── gradient color-interpolation-method (CSS Images L4 §3.1) ──────────────

    /// `extract_gradient_interpolation` strips the `in <space>` clause and
    /// returns the parsed space + hue method, preserving an accompanying angle
    /// in any order.
    #[test]
    fn gradient_interp_extract_space_and_angle() {
        let (clean, sp, hue) = extract_gradient_interpolation("45deg in oklch");
        assert_eq!(clean, "45deg");
        assert_eq!(sp, Some(MixColorSpace::Oklch));
        assert_eq!(hue, HueInterpolationMethod::Shorter, "default hue method");

        let (clean, sp, hue) = extract_gradient_interpolation("in oklch 45deg");
        assert_eq!(clean, "45deg");
        assert_eq!(sp, Some(MixColorSpace::Oklch));
        assert_eq!(hue, HueInterpolationMethod::Shorter);

        // Polar hue-interpolation keyword is parsed and captured.
        let (clean, sp, hue) = extract_gradient_interpolation("to right in hsl longer hue");
        assert_eq!(clean, "to right");
        assert_eq!(sp, Some(MixColorSpace::Hsl));
        assert_eq!(hue, HueInterpolationMethod::Longer);

        // `increasing hue` likewise captured.
        let (_, sp, hue) = extract_gradient_interpolation("in oklch increasing hue");
        assert_eq!(sp, Some(MixColorSpace::Oklch));
        assert_eq!(hue, HueInterpolationMethod::Increasing);

        // No interpolation clause — prelude untouched, no space, default hue.
        let (clean, sp, hue) = extract_gradient_interpolation("to bottom right");
        assert_eq!(clean, "to bottom right");
        assert_eq!(sp, None);
        assert_eq!(hue, HueInterpolationMethod::Shorter);
    }

    /// `longer hue` must produce a visibly different densified stop list than the
    /// default `shorter hue` for the same polar-space gradient (the long arc
    /// sweeps through intermediate hues the short arc skips).
    #[test]
    fn gradient_hue_longer_differs_from_shorter() {
        let short = parse_background_gradient("linear-gradient(in oklch, red, blue)");
        let long = parse_background_gradient("linear-gradient(in oklch longer hue, red, blue)");
        let stops_of = |g: ParsedGradient| match g {
            ParsedGradient::Linear { stops, .. } => stops,
            other => panic!("expected linear, got {other:?}"),
        };
        let s = stops_of(short);
        let l = stops_of(long);
        assert!(s.len() > 2 && l.len() > 2, "both should be densified");
        // Same endpoints, but at least one interior stop must differ.
        let differs = s.iter().zip(l.iter()).any(|(a, b)| {
            a.color.r != b.color.r || a.color.g != b.color.g || a.color.b != b.color.b
        });
        assert!(differs, "longer-hue arc must yield different intermediate colours");
    }

    /// `to <corner>` keywords parse to `GradientCorner`, with `angle_deg` left
    /// as the square-box placeholder (BUG-647) — only [`GradientCorner::angle_deg`]
    /// resolves the true, aspect-ratio-dependent angle.
    #[test]
    fn gradient_corner_keyword_parses_to_corner_variant() {
        let g = parse_background_gradient("linear-gradient(to bottom right, red, blue)");
        let ParsedGradient::Linear { angle_deg, corner, .. } = g else { panic!("expected linear") };
        assert!((angle_deg - 135.0).abs() < 0.01, "square-box placeholder");
        assert_eq!(corner, Some(GradientCorner::BottomRight));

        // An explicit angle must not carry a corner.
        let g = parse_background_gradient("linear-gradient(45deg, red, blue)");
        let ParsedGradient::Linear { corner, .. } = g else { panic!("expected linear") };
        assert_eq!(corner, None, "explicit <angle> has no corner keyword");
    }

    /// `GradientCorner::angle_deg` reduces to 45/135/225/315° on a square box
    /// (matching the pre-BUG-647 hardcoded behaviour) but tilts toward vertical
    /// — not horizontal — as the box gets wider, per CSS Images L3 §3.1's
    /// "perpendicular to the diagonal of the two unnamed corners" construction.
    /// The 170.5° figure is pixel-measured off a real Edge render of a 960×160
    /// box (`graphic_tests/76-motion-path.html`'s `.track-diag`), not derived.
    #[test]
    fn gradient_corner_angle_matches_edge_reference() {
        // Square box: reduces to the familiar diagonal angles.
        assert!((GradientCorner::TopRight.angle_deg(100.0, 100.0) - 45.0).abs() < 0.01);
        assert!((GradientCorner::BottomRight.angle_deg(100.0, 100.0) - 135.0).abs() < 0.01);
        assert!((GradientCorner::BottomLeft.angle_deg(100.0, 100.0) - 225.0).abs() < 0.01);
        assert!((GradientCorner::TopLeft.angle_deg(100.0, 100.0) - 315.0).abs() < 0.01);

        // Wide box (960×160): tilts toward vertical (180°/"to bottom"), the
        // opposite of the naive "tilts toward the long axis" guess.
        let angle = GradientCorner::BottomRight.angle_deg(960.0, 160.0);
        assert!((angle - 170.54).abs() < 0.1, "expected ~170.5°, got {angle}");
    }

    /// A plain `linear-gradient(red, blue)` (no `in <space>`) keeps exactly two
    /// stops — the densify path must not fire without an interpolation method.
    #[test]
    fn gradient_srgb_default_not_densified() {
        let g = parse_background_gradient("linear-gradient(red, blue)");
        match g {
            ParsedGradient::Linear { stops, angle_deg, .. } => {
                assert_eq!(stops.len(), 2, "no interpolation method → no extra stops");
                assert!((angle_deg - 180.0).abs() < 0.01, "default direction = to bottom");
            }
            other => panic!("expected linear, got {other:?}"),
        }
    }

    /// `in oklab` subdivides the stop list, preserves the direction, and the
    /// ~50% stop matches perceptual interpolation — distinct from the naive
    /// sRGB blend (sRGB red→blue midpoint is rgb(127,0,127) with **no green**,
    /// whereas oklab introduces a visible green component, ~rgb(140,83,162)).
    #[test]
    fn gradient_oklab_densifies_and_differs_from_srgb() {
        let g = parse_background_gradient("linear-gradient(90deg in oklab, red, blue)");
        let ParsedGradient::Linear { stops, angle_deg, .. } = g else {
            panic!("expected linear");
        };
        assert!((angle_deg - 90.0).abs() < 0.01, "angle preserved past `in oklab`");
        assert!(stops.len() > 2, "oklab interpolation should add intermediate stops");
        // First/last endpoints unchanged.
        assert_eq!((stops[0].color.r, stops[0].color.b), (255, 0), "starts red");
        let last = stops.last().unwrap().color;
        assert_eq!((last.r, last.b), (0, 255), "ends blue");
        // Midpoint (~50%): oklab introduces green that the sRGB blend lacks.
        let mid = stops
            .iter()
            .min_by(|a, b| {
                let key = |s: &GradientStop| match s.position {
                    Some(Length::Percent(v)) => (v - 50.0).abs(),
                    _ => f32::INFINITY,
                };
                key(a).partial_cmp(&key(b)).unwrap()
            })
            .unwrap()
            .color;
        assert!(
            mid.g > 20,
            "oklab midpoint has visible green (sRGB would be 0), got g={}",
            mid.g
        );
    }

    /// Densification keeps resolved stop positions monotonic and within [0,100],
    /// and an unknown interpolation space falls back gracefully (parses fine,
    /// no densify because `MixColorSpace::from_css` rejects the token).
    #[test]
    fn gradient_interp_positions_and_unknown_space() {
        let g = parse_background_gradient("radial-gradient(in oklab, red 10%, lime 40%, blue 90%)");
        let ParsedGradient::Radial { stops, .. } = g else {
            panic!("expected radial");
        };
        assert!(stops.len() > 3);
        let mut prev = -1.0_f32;
        for st in &stops {
            if let Some(Length::Percent(p)) = st.position {
                assert!(p >= prev - 0.01, "positions monotonic: {p} after {prev}");
                assert!((0.0..=100.0).contains(&p), "position in range: {p}");
                prev = p;
            }
        }

        // Unknown space token: not stripped, treated as a (skipped) prelude
        // token; stops parse normally and are not densified.
        let g = parse_background_gradient("linear-gradient(in bogus, red, blue)");
        if let ParsedGradient::Linear { stops, .. } = g {
            assert_eq!(stops.len(), 2, "unknown space → no densify");
        } else {
            panic!("expected linear");
        }
    }

    // ── radial-gradient shape / size (CSS Images L3 §3.5, BUG-239) ─────────────

    #[test]
    fn radial_shape_size_parses_circle_and_ellipse() {
        let circle = parse_background_gradient("radial-gradient(circle, red, blue)");
        let ParsedGradient::Radial { shape, size, .. } = circle else { panic!("radial") };
        assert_eq!(shape, RadialShape::Circle);
        assert_eq!(size, RadialSize::FarthestCorner, "default size");

        let ellipse = parse_background_gradient("radial-gradient(ellipse at center, red, blue)");
        let ParsedGradient::Radial { shape, .. } = ellipse else { panic!("radial") };
        assert_eq!(shape, RadialShape::Ellipse);

        // No shape keyword → ellipse default (CSS Images L3 §3.5).
        let bare = parse_background_gradient("radial-gradient(red, blue)");
        let ParsedGradient::Radial { shape, .. } = bare else { panic!("radial") };
        assert_eq!(shape, RadialShape::Ellipse);

        let cs = parse_background_gradient("radial-gradient(circle closest-side, red, blue)");
        let ParsedGradient::Radial { shape, size, .. } = cs else { panic!("radial") };
        assert_eq!((shape, size), (RadialShape::Circle, RadialSize::ClosestSide));
    }

    #[test]
    fn radial_radii_circle_is_farthest_corner_distance() {
        // Centred circle in 240×120 → farthest corner at (120, 60): r = hypot.
        let (rx, ry) = radial_gradient_radii(
            RadialShape::Circle, RadialSize::FarthestCorner, 0.5, 0.5, 240.0, 120.0,
        );
        let expected = 120.0_f32.hypot(60.0);
        assert!((rx - expected).abs() < 0.5 && (rx - ry).abs() < 1e-3, "circle isotropic: {rx},{ry}");
    }

    #[test]
    fn radial_radii_ellipse_farthest_corner_matches_spec() {
        // ellipse at center in 240×120: farthest-side aspect = 120/60 = 2; the
        // ellipse passes through the corner (120,60) → ry = √(60²+60²) ≈ 84.85,
        // rx = 2·ry ≈ 169.7 (CSS Images L3 §3.5.1).
        let (rx, ry) = radial_gradient_radii(
            RadialShape::Ellipse, RadialSize::FarthestCorner, 0.5, 0.5, 240.0, 120.0,
        );
        assert!((ry - 84.85).abs() < 0.5, "ry ≈ 84.85, got {ry}");
        assert!((rx - 169.7).abs() < 1.0, "rx ≈ 169.7, got {rx}");
    }

    #[test]
    fn radial_radii_ellipse_closest_side() {
        // Off-centre ellipse, closest-side: rx = nearest h-edge, ry = nearest v-edge.
        let (rx, ry) = radial_gradient_radii(
            RadialShape::Ellipse, RadialSize::ClosestSide, 0.25, 0.25, 200.0, 100.0,
        );
        assert!((rx - 50.0).abs() < 0.5, "rx = min(50,150)=50, got {rx}");
        assert!((ry - 25.0).abs() < 0.5, "ry = min(25,75)=25, got {ry}");
    }
