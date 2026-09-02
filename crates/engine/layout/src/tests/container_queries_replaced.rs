use super::*;

    // ── CSS Container Style Queries — nested and/or/not inside a single
    // style() call (CSS Containment L3 §5.2 <style-condition> grammar) ────

    #[test]
    fn style_query_nested_and_both_true() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "2")]);
        assert!(crate::evaluate_container_condition("style((--a: 1) and (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_and_one_false() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "3")]);
        assert!(!crate::evaluate_container_condition("style((--a: 1) and (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_or_one_true() {
        let ctx = style_ctx(&[("--a", "9"), ("--b", "2")]);
        assert!(crate::evaluate_container_condition("style((--a: 1) or (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_or_both_false() {
        let ctx = style_ctx(&[("--a", "9"), ("--b", "9")]);
        assert!(!crate::evaluate_container_condition("style((--a: 1) or (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_not() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "block")]);
        assert!(crate::evaluate_container_condition("style(not (display: none))", &ctx));
    }

    #[test]
    fn style_query_nested_not_false() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "none")]);
        assert!(!crate::evaluate_container_condition("style(not (display: none))", &ctx));
    }

    #[test]
    fn style_query_nested_and_chain_of_three() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "2"), ("--c", "3")]);
        assert!(crate::evaluate_container_condition(
            "style((--a: 1) and (--b: 2) and (--c: 3))",
            &ctx
        ));
    }

    #[test]
    fn style_query_nested_and_chain_of_three_last_false() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "2"), ("--c", "9")]);
        assert!(!crate::evaluate_container_condition(
            "style((--a: 1) and (--b: 2) and (--c: 3))",
            &ctx
        ));
    }

    #[test]
    fn style_query_nested_mixed_custom_and_standard() {
        let ctx = style_ctx_with_style_props(&[("--theme", "dark")], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition(
            "style((--theme: dark) and (display: flex))",
            &ctx
        ));
    }

    #[test]
    fn style_query_single_feature_extra_grouping_paren() {
        // A single <style-feature> wrapped in one redundant grouping paren layer.
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style((--theme: dark))", &ctx));
    }

    // ── CSS Container Queries L1 ──────────────────────────────────────────

    /// @container (min-width) — rule applies when container is wide enough.
    #[test]
    fn container_query_min_width_applies() {
        // Container is 200px wide. Rule applies at min-width:150px → p gets height:40px.
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; }
             @container (min-width: 150px) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container min-width:150px should apply to 200px container, got height={}",
            p.rect.height,
        );
    }

    /// @container style(--prop: value) — rule applies when container has the custom property.
    #[test]
    fn container_style_query_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --theme: dark; }
             @container style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container style(--theme: dark) should apply, got height={}",
            p.rect.height,
        );
    }

    /// @container style(--prop: value) — rule does not apply when value differs.
    #[test]
    fn container_style_query_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --theme: light; }
             @container style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "container style(--theme: dark) should NOT apply with --theme: light, got height={}",
            p.rect.height,
        );
    }

    /// @container style(prop: value) — standard (non-custom) property, resolved
    /// against the container's own computed style.
    #[test]
    fn container_style_query_standard_property_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; position: relative; }
             @container style(position: relative) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container style(position: relative) should apply, got height={}",
            p.rect.height,
        );
    }

    /// @container style(prop: value) — standard property query does not apply
    /// when the container's computed value differs.
    #[test]
    fn container_style_query_standard_property_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; position: static; }
             @container style(position: relative) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "container style(position: relative) should NOT apply with position:static, got height={}",
            p.rect.height,
        );
    }

    /// @container style(--prop: value) — matches when the container's custom
    /// property is declared via `var()` chained to another custom property.
    #[test]
    fn container_style_query_var_chain_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --base: dark; --theme: var(--base); }
             @container style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container style(--theme: dark) should apply via var() chain, got height={}",
            p.rect.height,
        );
    }

    /// @container (min-width) and style(...) — combined condition.
    #[test]
    fn container_style_query_combined_with_size() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --theme: dark; }
             @container (min-width: 150px) and style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "combined (min-width: 150px) and style(--theme: dark) should apply, got height={}",
            p.rect.height,
        );
    }

    /// @container style(height: %) — the `%` in the query's declared value
    /// must resolve against the container's *own* containing block height
    /// (its parent's content box), not the container's own height or width.
    /// `outer` is 300px tall; `container`'s own `height: 150px` is exactly
    /// 50% of that — a basis of the container's own height (150) or its
    /// width (200) would both give a mismatch instead.
    #[test]
    fn container_style_query_height_percent_uses_parent_containing_block() {
        let root = lay_measured(
            "<div class=\"outer\"><div class=\"container\"><p></p></div></div>",
            "div.outer { height: 300px; }
             div.container { container-type: size; width: 200px; height: 150px; }
             @container style(height: 50%) { p { height: 40px; } }",
            400.0,
        );
        let outer = first_element_child(&root);
        let container = first_element_child(outer);
        let p = first_element_child(container);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "style(height: 50%) should apply (50% of the 300px parent height == the \
             container's own 150px height), got p.height={}",
            p.rect.height,
        );
    }

    /// @container (min-width) — rule does NOT apply when container is too narrow.
    #[test]
    fn container_query_min_width_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 100px; height: 100px; }
             @container (min-width: 200px) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "container min-width:200px should NOT apply to 100px container, got height={}",
            p.rect.height,
        );
    }

    /// @container (max-width) — rule applies when container is narrow.
    #[test]
    fn container_query_max_width_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: inline-size; width: 150px; height: 100px; }
             @container (max-width: 200px) { p { height: 30px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 30.0).abs() < 0.5,
            "container max-width:200px should apply to 150px container, got height={}",
            p.rect.height,
        );
    }

    /// Named @container — only applies to matching container-name.
    #[test]
    fn container_query_named_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; container-name: sidebar; width: 200px; height: 100px; }
             @container sidebar (min-width: 100px) { p { height: 50px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 50.0).abs() < 0.5,
            "named container query should match sidebar, got height={}",
            p.rect.height,
        );
    }

    /// Named @container — does NOT apply to wrong container name.
    #[test]
    fn container_query_named_wrong_name_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; container-name: main; width: 200px; height: 100px; }
             @container sidebar (min-width: 100px) { p { height: 50px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "named container 'sidebar' should NOT match 'main', got height={}",
            p.rect.height,
        );
    }

    // ── <img> replaced element ───────────────────────────────────────────

    /// Первый `BoxKind::Image` в поддереве. Поиск рекурсивный, потому что с
    /// IFC-2 `<img>` — atomic inline-level бокс и лежит не прямым ребёнком
    /// блока, а внутри анонимного `InlineBlockRow`, собирающего его строку.
    pub(crate) fn first_image_child(b: &LayoutBox) -> &LayoutBox {
        fn walk(b: &LayoutBox) -> Option<&LayoutBox> {
            for c in &b.children {
                if matches!(c.kind, BoxKind::Image { .. }) {
                    return Some(c);
                }
                if let Some(found) = walk(c) {
                    return Some(found);
                }
            }
            None
        }
        walk(b).expect("expected at least one image child")
    }

    #[test]
    fn img_creates_image_box_with_src_and_alt() {
        let root = lay(r#"<img src="logo.png" alt="logo">"#, "");
        let img = first_image_child(&root);
        match &img.kind {
            BoxKind::Image { src, alt, .. } => {
                assert_eq!(src, "logo.png");
                assert_eq!(alt, "logo");
            }
            other => panic!("expected BoxKind::Image, got {other:?}"),
        }
    }

    #[test]
    fn img_without_src_or_alt_has_empty_strings() {
        let root = lay("<img>", "");
        let img = first_image_child(&root);
        if let BoxKind::Image { src, alt, .. } = &img.kind {
            assert_eq!(src, "");
            assert_eq!(alt, "");
        }
    }

    #[test]
    fn img_html_attributes_set_dimensions() {
        // HTML5 presentational hints: width/height атрибуты → CSS свойства,
        // без CSS-каскада победившего alternative.
        let root = lay(r#"<img src="x.png" width="120" height="80">"#, "");
        let img = first_image_child(&root);
        assert!((img.rect.width - 120.0).abs() < 0.1);
        assert!((img.rect.height - 80.0).abs() < 0.1);
    }

    #[test]
    fn img_css_overrides_html_attribute_dimensions() {
        // Author CSS перекрывает presentational hints (HTML5 §10).
        let root = lay(
            r#"<img src="x.png" width="120" height="80">"#,
            "img { width: 200px; height: 50px; }",
        );
        let img = first_image_child(&root);
        assert!((img.rect.width - 200.0).abs() < 0.1, "width={}", img.rect.width);
        assert!((img.rect.height - 50.0).abs() < 0.1, "height={}", img.rect.height);
    }

    #[test]
    fn img_without_dimensions_is_zero_sized() {
        // Без атрибутов и без CSS — image не загружено, intrinsic неизвестен,
        // коробка 0×0. Это honest placeholder — будет ясно, что чего-то не
        // хватает.
        let root = lay(r#"<img src="x.png">"#, "");
        let img = first_image_child(&root);
        assert!(img.rect.width.abs() < 0.1);
        assert!(img.rect.height.abs() < 0.1);
    }

    #[test]
    fn img_invalid_width_attribute_ignored() {
        // HTML5: nonsense → ignore.
        let root = lay(r#"<img src="x" width="abc" height="-50">"#, "");
        let img = first_image_child(&root);
        assert!(img.rect.width.abs() < 0.1);
        assert!(img.rect.height.abs() < 0.1);
    }

    #[test]
    fn img_padding_and_border_extend_box() {
        // CSS box для replaced element ведёт себя как block: padding + border
        // расширяют rect (content-box). Размер картинки 100×60, padding 10,
        // border 2 → rect 124×84.
        let root = lay(
            r#"<img src="x" width="100" height="60">"#,
            "img { padding: 10px; border: 2px solid red; }",
        );
        let img = first_image_child(&root);
        assert!((img.rect.width - 124.0).abs() < 0.1, "width={}", img.rect.width);
        assert!((img.rect.height - 84.0).abs() < 0.1, "height={}", img.rect.height);
    }

    #[test]
    fn img_is_atomic_inline_not_inline_content() {
        // IFC-2: <img> делит строку с текстом, но НЕ вливается в него
        // сегментом — у сегмента нет собственной высоты (BUG-728). Значит один
        // анонимный `InlineBlockRow` на всю строку, а внутри — три куска:
        // прогон «before», картинка, прогон «after».
        let root = lay(r#"<div>before<img src="x" width="10" height="10">after</div>"#, "");
        let div = first_element_child(&root);
        assert_eq!(div.children.len(), 1, "строка должна быть одна, а не {}", div.children.len());
        let row = &div.children[0];
        assert!(
            matches!(row.kind, BoxKind::InlineBlockRow),
            "картинка с текстом обязана собраться в InlineBlockRow"
        );
        assert_eq!(row.children.len(), 3, "got {}", row.children.len());
        assert!(matches!(row.children[0].kind, BoxKind::InlineRun { .. }));
        assert!(matches!(row.children[1].kind, BoxKind::Image { .. }));
        assert!(matches!(row.children[2].kind, BoxKind::InlineRun { .. }));
    }

    #[test]
    fn img_display_none_is_skipped() {
        let root = lay(
            r#"<img src="x.png" width="100" height="50">"#,
            "img { display: none; }",
        );
        let has_image = root.children.iter().any(|c| matches!(c.kind, BoxKind::Image { .. }));
        assert!(!has_image, "img with display:none should not produce Image box");
    }

    #[test]
    fn img_attr_name_case_insensitive() {
        // HTML-парсер lower-case-ит имена тегов, но атрибуты могут попасть в
        // mixed-case. Наш get_attr — ASCII case-insensitive.
        let root = lay(r#"<img SRC="x.png" Width="50" HEIGHT="30">"#, "");
        let img = first_image_child(&root);
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "x.png");
        }
        assert!((img.rect.width - 50.0).abs() < 0.1);
        assert!((img.rect.height - 30.0).abs() < 0.1);
    }

    // ──────── <video> replaced element ────────

    fn first_video_child(b: &LayoutBox) -> &LayoutBox {
        b.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Video { .. }))
            .expect("expected at least one video child")
    }

    #[test]
    fn video_creates_video_box_with_src() {
        let root = lay(r#"<video src="clip.mp4"></video>"#, "");
        let vid = first_video_child(&root);
        match &vid.kind {
            BoxKind::Video { src, poster } => {
                assert_eq!(src, "clip.mp4");
                assert_eq!(poster, "");
            }
            other => panic!("expected BoxKind::Video, got {other:?}"),
        }
    }

    #[test]
    fn video_captures_poster_attribute() {
        let root = lay(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let vid = first_video_child(&root);
        if let BoxKind::Video { poster, .. } = &vid.kind {
            assert_eq!(poster, "thumb.jpg");
        }
    }

    #[test]
    fn video_ua_default_size_300_by_150() {
        // HTML spec §14.1: UA default intrinsic size 300×150 CSS px.
        let root = lay(r#"<video src="clip.mp4"></video>"#, "");
        let vid = first_video_child(&root);
        assert!((vid.rect.width - 300.0).abs() < 0.1, "width={}", vid.rect.width);
        assert!((vid.rect.height - 150.0).abs() < 0.1, "height={}", vid.rect.height);
    }

    #[test]
    fn video_html_attribute_dimensions_override_ua_default() {
        let root = lay(r#"<video src="clip.mp4" width="640" height="360"></video>"#, "");
        let vid = first_video_child(&root);
        assert!((vid.rect.width - 640.0).abs() < 0.1, "width={}", vid.rect.width);
        assert!((vid.rect.height - 360.0).abs() < 0.1, "height={}", vid.rect.height);
    }

    #[test]
    fn video_css_overrides_ua_default() {
        let root = lay(
            r#"<video src="clip.mp4"></video>"#,
            "video { width: 480px; height: 270px; }",
        );
        let vid = first_video_child(&root);
        assert!((vid.rect.width - 480.0).abs() < 0.1, "width={}", vid.rect.width);
        assert!((vid.rect.height - 270.0).abs() < 0.1, "height={}", vid.rect.height);
    }

    #[test]
    fn video_display_none_is_skipped() {
        let root = lay(
            r#"<video src="clip.mp4"></video>"#,
            "video { display: none; }",
        );
        let has_video = root.children.iter().any(|c| matches!(c.kind, BoxKind::Video { .. }));
        assert!(!has_video, "video with display:none should not produce Video box");
    }

    #[test]
    fn video_is_replaced_element_does_not_stretch() {
        // Replaced elements do NOT stretch to fill container width (CSS 2.1 §10.3.2).
        let root = lay(r#"<video src="clip.mp4"></video>"#, "");
        let vid = first_video_child(&root);
        // UA default 300px, not 800px (viewport width).
        assert!((vid.rect.width - 300.0).abs() < 0.1, "width={}", vid.rect.width);
    }

    // ──────── <iframe> placeholder layout ───────────────────────────────────

    fn first_iframe_child(b: &LayoutBox) -> &LayoutBox {
        b.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Iframe { .. }))
            .expect("expected at least one Iframe box")
    }

    #[test]
    fn iframe_creates_iframe_box_with_src() {
        let root = lay(r#"<iframe src="https://example.com"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        match &frame.kind {
            BoxKind::Iframe { src, .. } => assert_eq!(src, "https://example.com"),
            other => panic!("expected BoxKind::Iframe, got {other:?}"),
        }
    }

    #[test]
    fn iframe_ua_default_size_300_by_150() {
        // HTML spec §4.8.5: UA default intrinsic size is 300×150 CSS px.
        let root = lay(r#"<iframe src="x.html"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        assert!((frame.rect.width - 300.0).abs() < 0.1, "width={}", frame.rect.width);
        assert!((frame.rect.height - 150.0).abs() < 0.1, "height={}", frame.rect.height);
    }

    #[test]
    fn iframe_html_attribute_dimensions_override_ua_default() {
        let root = lay(r#"<iframe src="x.html" width="800" height="600"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        assert!((frame.rect.width - 800.0).abs() < 0.1, "width={}", frame.rect.width);
        assert!((frame.rect.height - 600.0).abs() < 0.1, "height={}", frame.rect.height);
    }

    #[test]
    fn iframe_css_overrides_ua_default() {
        let root = lay(
            r#"<iframe src="x.html"></iframe>"#,
            "iframe { width: 400px; height: 300px; }",
        );
        let frame = first_iframe_child(&root);
        assert!((frame.rect.width - 400.0).abs() < 0.1, "width={}", frame.rect.width);
        assert!((frame.rect.height - 300.0).abs() < 0.1, "height={}", frame.rect.height);
    }

    #[test]
    fn iframe_is_replaced_element_does_not_stretch() {
        // Replaced elements do NOT stretch to fill container width (CSS 2.1 §10.3.2).
        let root = lay(r#"<iframe src="x.html"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        // UA default 300px, not 800px (viewport width).
        assert!((frame.rect.width - 300.0).abs() < 0.1, "width={}", frame.rect.width);
    }

    #[test]
    fn iframe_empty_src_is_valid() {
        let root = lay(r#"<iframe></iframe>"#, "");
        let frame = first_iframe_child(&root);
        match &frame.kind {
            BoxKind::Iframe { src, .. } => assert_eq!(src, ""),
            other => panic!("expected BoxKind::Iframe, got {other:?}"),
        }
    }

    #[test]
    fn iframe_srcdoc_stored_in_box_kind() {
        let root = lay(r#"<iframe srcdoc="<p>hello</p>"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        match &frame.kind {
            BoxKind::Iframe { srcdoc, .. } => {
                assert_eq!(srcdoc.as_deref(), Some("<p>hello</p>"));
            }
            other => panic!("expected BoxKind::Iframe, got {other:?}"),
        }
    }

    #[test]
    fn build_iframe_document_empty_html_returns_document() {
        let doc = build_iframe_document("");
        // Empty input still produces a valid Document with a root node that has children.
        // lumen_html_parser::parse always inserts implicit html/head/body.
        assert!(!doc.get(doc.root()).children.is_empty());
    }

    #[test]
    fn build_iframe_document_parses_inline_html() {
        let doc = build_iframe_document("<p>hello world</p>");
        // The parsed document should contain a paragraph element somewhere in the tree.
        let mut found = false;
        let mut stack = vec![doc.root()];
        while let Some(id) = stack.pop() {
            if doc.get(id).element_name().is_some_and(|n| n.local == "p") {
                found = true;
                break;
            }
            stack.extend_from_slice(&doc.get(id).children);
        }
        assert!(found, "expected <p> in parsed srcdoc document");
    }

    // ──────── <picture> / <img srcset> source-selection integration ────────

    /// Рекурсивный поиск первого `Image`-бокса в дереве. Нужен для тестов
    /// с `<picture>`: inner `<img>` зарывается на 2 уровня (picture-обёртка
    /// сначала становится Block).
    fn find_image(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Image { .. }) {
            return Some(b);
        }
        for c in &b.children {
            if let Some(found) = find_image(c) {
                return Some(found);
            }
        }
        None
    }

    /// Рекурсивный поиск любого `LayoutBox`, у которого `BoxKind::Image`
    /// присутствует — возвращает все, чтобы посчитать.
    fn count_image_boxes(b: &LayoutBox) -> usize {
        let mut n = usize::from(matches!(b.kind, BoxKind::Image { .. }));
        for c in &b.children {
            n += count_image_boxes(c);
        }
        n
    }

    #[test]
    fn picture_uses_source_srcset_over_inner_img() {
        // `<picture>`-picker выбирает первый матчащий `<source>` до
        // fallback `<img>`. У нас один `<source>` без media-фильтра —
        // он всегда выигрывает у inner img.
        let root = lay(
            r#"<picture>
                <source srcset="hires.png">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img inside picture");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "hires.png", "picker должен был выбрать source, а не fallback");
        } else {
            panic!("expected Image");
        }
    }

    #[test]
    fn picture_media_filter_picks_matching_source() {
        // viewport 800×600 — `(min-width: 700px)` матчит, `(max-width: 500px)` нет.
        let root = lay(
            r#"<picture>
                <source media="(max-width: 500px)" srcset="small.png">
                <source media="(min-width: 700px)" srcset="big.png">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img inside picture");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "big.png");
        }
    }

    #[test]
    fn picture_falls_back_to_inner_img_when_no_source_matches() {
        // Все `<source>` отсеяны media-фильтром → picker идёт на inner `<img>`.
        let root = lay(
            r#"<picture>
                <source media="(max-width: 100px)" srcset="tiny.png">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img inside picture");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "fallback.png");
        }
    }

    #[test]
    fn img_srcset_density_picker_selects_one_x_at_dpr_1() {
        // DPR в layout фиксирован на 1.0 (Phase 0). Среди density-кандидатов
        // picker выберет 1x как ближайший — это `low.png`.
        let root = lay(r#"<img srcset="low.png 1x, high.png 2x" src="z.png">"#, "");
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "low.png");
        }
    }

    #[test]
    fn img_srcset_falls_back_to_src_when_picker_empty() {
        // srcset из одних запятых — нет валидных кандидатов; picker
        // возвращает raw src через свой внутренний fallback.
        let root = lay(r#"<img srcset=",,," src="real.png">"#, "");
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "real.png");
        }
    }

    #[test]
    fn block_with_inline_image_includes_baseline_descent_gap() {
        // BUG-180: a bare <img> is an inline-level replaced element, baseline-aligned
        // by default, so its line box — and therefore the height of the block that
        // wraps it — extends below the image by the strut descent (the classic
        // "image bottom gap"). Lumen lays a lone <img> as a block-flow child, so this
        // sub-baseline space must be added explicitly; without it an image grid drifts
        // ~descent px upward per row versus a browser (TEST-18: 22.1% → 2.1%).
        let doc = lumen_html_parser::parse(
            r#"<div id="frame"><img src="a.png" width="200" height="150"></div>"#,
        );
        let sheet = lumen_css_parser::parse("#frame { padding: 3px; }");
        let root = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let frame = find_by_tag(&root, "div", &doc).expect("frame div");
        // Fixed8.descent_px(16) = 16 * 0.2 = 3.2 (default strut descent).
        // content = img 150 + descent 3.2; border-box = + padding 6 = 159.2.
        let expected = 150.0 + 16.0 * 0.2 + 6.0;
        assert!(
            (frame.rect.height - expected).abs() < 0.01,
            "frame height {} should include the image-bottom descent gap (expected {expected})",
            frame.rect.height,
        );
    }

    #[test]
    fn block_with_top_aligned_image_has_no_descent_gap() {
        // Contrast to the baseline case: vertical-align:top anchors the replaced box
        // to the line-box top, so there is no sub-baseline gap — the frame is exactly
        // img + padding.
        let doc = lumen_html_parser::parse(
            r#"<div id="frame"><img src="a.png" width="200" height="150"></div>"#,
        );
        let sheet = lumen_css_parser::parse("#frame { padding: 3px; } img { vertical-align: top; }");
        let root = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let frame = find_by_tag(&root, "div", &doc).expect("frame div");
        assert!(
            (frame.rect.height - (150.0 + 6.0)).abs() < 0.01,
            "top-aligned image must not add the baseline descent gap, got {}",
            frame.rect.height,
        );
    }

    #[test]
    fn img_without_src_and_srcset_produces_empty_url() {
        // Битая разметка — picker возвращает None, мы падаем в legacy
        // fallback и сохраняем пустой src (как и было до интеграции).
        let root = lay("<img>", "");
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "");
        }
    }

    #[test]
    fn source_element_does_not_produce_box() {
        // `<source>` теперь Display::None — два source-а внутри `<picture>` не
        // порождают LayoutBox-ов. Проверяем по двум инвариантам: ровно один
        // Image-box в дереве (от inner `<img>`) и общее число дочерних
        // блоков у picture-обёртки = 1 (только сам `<img>`-box, плюс
        // потенциально whitespace InlineRun-ы).
        let root = lay(
            r#"<picture><source srcset="a.png"><source srcset="b.png"><img src="c.png"></picture>"#,
            "",
        );
        assert_eq!(count_image_boxes(&root), 1);
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "a.png", "первый матчащий source — победитель");
        }
    }

    #[test]
    fn picture_source_intrinsic_dims_fill_blank_style() {
        // У выбранного `<source>` есть width/height атрибуты, у inner `<img>` нет,
        // и автор CSS не задал — intrinsic dims с source-а попадают в layout-box.
        let root = lay(
            r#"<picture>
                <source srcset="big.png" width="240" height="160">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img");
        assert!((img.rect.width - 240.0).abs() < 0.1, "width={}", img.rect.width);
        assert!((img.rect.height - 160.0).abs() < 0.1, "height={}", img.rect.height);
    }

    #[test]
    fn picture_source_intrinsic_does_not_override_author_css() {
        // Author CSS перекрывает intrinsic dimensions с `<source>` — это
        // обычная presentational-hint специфика (HTML5 §10).
        let root = lay(
            r#"<picture>
                <source srcset="big.png" width="240" height="160">
                <img src="fallback.png">
            </picture>"#,
            "img { width: 100px; height: 50px; }",
        );
        let img = find_image(&root).expect("img");
        assert!((img.rect.width - 100.0).abs() < 0.1);
        assert!((img.rect.height - 50.0).abs() < 0.1);
    }

    // ──────── CSS-wide keywords (CSS Cascade L4 §7) ────────

    #[test]
    fn parse_css_wide_keyword_matches_all_four() {
        use crate::CssWideKeyword;
        assert_eq!(crate::parse_css_wide_keyword("inherit"), Some(CssWideKeyword::Inherit));
        assert_eq!(crate::parse_css_wide_keyword("INITIAL"), Some(CssWideKeyword::Initial));
        assert_eq!(crate::parse_css_wide_keyword("Unset"), Some(CssWideKeyword::Unset));
        assert_eq!(crate::parse_css_wide_keyword("revert"), Some(CssWideKeyword::Revert));
        assert_eq!(crate::parse_css_wide_keyword("  inherit  "), Some(CssWideKeyword::Inherit));
        assert_eq!(crate::parse_css_wide_keyword("red"), None);
        assert_eq!(crate::parse_css_wide_keyword("inheritance"), None);
    }

    /// Получить style вложенного `<p>` из `<div><p>x</p></div>`-тестового
    /// дерева. root → first child (anonymous wrapper или div) → first child block.
    /// Возвращает style p — там и применяется тестируемая декларация.
    pub(crate) fn nested_p_style(root: &LayoutBox) -> &ComputedStyle {
        let div = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("div block");
        let p = div
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        &p.style
    }

    fn lay_get_p_color(html: &str, css: &str) -> Color {
        let root = lay(html, css);
        nested_p_style(&root).color
    }

    #[test]
    fn css_inherit_forces_parent_color_on_non_inherited_default() {
        // Для inherited-свойств (color) — `inherit` совпадает с дефолтом
        // (если родитель сам не переопределяет). Подтверждает no-op в этом
        // тривиальном случае.
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: inherit; }",
        );
        // p наследует от div = red.
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn css_initial_resets_color_to_initial() {
        // Initial value for color — black (Color::BLACK).
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: initial; }",
        );
        assert_eq!(c, Color::BLACK);
    }

    #[test]
    fn css_unset_inherited_property_acts_as_inherit() {
        // color — inherited; `unset` для inherited = inherit → parent's red.
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: unset; }",
        );
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn css_unset_undoes_prior_declaration() {
        // p { color: blue; color: unset; } → unset вступает позже,
        // откатывает blue до inherited (red).
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: blue; color: unset; }",
        );
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn css_inherit_on_non_inherited_pulls_from_parent() {
        // background-color НЕ inherited. По умолчанию None у потомка.
        // `inherit` форсит наследование → background.color родителя.
        let root = lay(
            "<div><p>x</p></div>",
            "div { background-color: rgb(0, 100, 200); } p { background-color: inherit; }",
        );
        // Найдём p — это child div, который сам root.children[0].
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(
            p.style.background_color,
            Some(CssColor::Rgba(Color { r: 0, g: 100, b: 200, a: 255 }))
        );
    }

    #[test]
    fn css_initial_on_non_inherited_resets_to_default() {
        // background-color: red → initial → None (default).
        let root = lay(
            "<p>x</p>",
            "p { background-color: red; background-color: initial; }",
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.background_color, None);
    }

    #[test]
    fn css_font_size_inherit_uses_parent() {
        // font-size: inherit для p → parent font_size = 30px.
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 30px; } p { font-size: 40px; font-size: inherit; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.font_size - 30.0).abs() < 0.1, "fs={}", p.style.font_size);
    }

    #[test]
    fn css_font_size_initial_is_16() {
        let root = lay(
            "<p>x</p>",
            "p { font-size: 40px; font-size: initial; }",
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.font_size - 16.0).abs() < 0.1, "fs={}", p.style.font_size);
    }

    #[test]
    fn css_unset_non_inherited_resets_to_initial() {
        // background-color: red → unset → None (initial — non-inherited prop).
        let root = lay(
            "<p>x</p>",
            "p { background-color: red; background-color: unset; }",
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.background_color, None);
    }

    #[test]
    fn css_revert_falls_back_to_inherited_without_ua_hint() {
        // `color` has no UA-stylesheet hint on `<p>`, so `revert` rolls back to
        // the same value `unset` would give: the inherited value. Cases where
        // `revert` differs from `unset` (a UA hint applies) are covered in
        // `style.rs`'s `revert_*_ua_hint_*` tests.
        let c1 = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: blue; color: revert; }",
        );
        assert_eq!(c1, Color { r: 255, g: 0, b: 0, a: 255 }); // inherited
    }

    #[test]
    fn css_wide_keyword_case_insensitive_in_value() {
        // CSS keyword values — ASCII case-insensitive по CSS Values L4 §2.4.
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: INHERIT; }",
        );
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    // ──────── @property syntax-валидация (CSS Properties and Values L1 §2) ────────

    fn lay_get_custom_prop(html: &str, css: &str, key: &str) -> Option<String> {
        let root = lay(html, css);
        let p = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("first block");
        p.style.custom_props.get(key).cloned()
    }

    #[test]
    fn property_syntax_universal_accepts_anything() {
        // syntax: '*' — любое значение проходит, в т.ч. бессмысленное.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --foo { syntax: '*'; inherits: false; initial-value: 0; } p { --foo: garbage; }",
            "--foo",
        );
        assert_eq!(v, Some("garbage".to_string()));
    }

    #[test]
    fn property_syntax_length_accepts_px() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --gap: 10px; }",
            "--gap",
        );
        assert_eq!(v, Some("10px".to_string()));
    }

    #[test]
    fn property_syntax_length_rejects_color() {
        // syntax: '<length>' + value=red → invalid; declaration пропускается,
        // остаётся initial-value '0px'.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --gap: red; }",
            "--gap",
        );
        assert_eq!(v, Some("0px".to_string()));
    }

    #[test]
    fn property_syntax_length_rejects_percentage() {
        // <length> НЕ принимает `%` — это <percentage>.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --gap: 50%; }",
            "--gap",
        );
        assert_eq!(v, Some("0px".to_string()));
    }

    #[test]
    fn property_syntax_color_accepts_named_and_hex() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --bg { syntax: '<color>'; inherits: false; initial-value: black; } p { --bg: red; }",
            "--bg",
        );
        assert_eq!(v, Some("red".to_string()));
    }

    #[test]
    fn property_syntax_color_rejects_length() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --bg { syntax: '<color>'; inherits: false; initial-value: black; } p { --bg: 10px; }",
            "--bg",
        );
        assert_eq!(v, Some("black".to_string()));
    }

    #[test]
    fn property_syntax_union_length_or_percentage() {
        // `<length-percentage>` принимает оба.
        let v1 = lay_get_custom_prop(
            "<p>x</p>",
            "@property --w { syntax: '<length-percentage>'; inherits: false; initial-value: 0px; } p { --w: 50%; }",
            "--w",
        );
        assert_eq!(v1, Some("50%".to_string()));
        let v2 = lay_get_custom_prop(
            "<p>x</p>",
            "@property --w { syntax: '<length-percentage>'; inherits: false; initial-value: 0px; } p { --w: 10rem; }",
            "--w",
        );
        assert_eq!(v2, Some("10rem".to_string()));
    }

    #[test]
    fn property_syntax_or_alternative() {
        // syntax с `|`: '<length> | <color>'. Оба подходят.
        let v_len = lay_get_custom_prop(
            "<p>x</p>",
            "@property --x { syntax: '<length> | <color>'; inherits: false; initial-value: 0px; } p { --x: 5px; }",
            "--x",
        );
        assert_eq!(v_len, Some("5px".to_string()));
        let v_color = lay_get_custom_prop(
            "<p>x</p>",
            "@property --x { syntax: '<length> | <color>'; inherits: false; initial-value: 0px; } p { --x: blue; }",
            "--x",
        );
        assert_eq!(v_color, Some("blue".to_string()));
    }

    #[test]
    fn property_syntax_skips_value_with_var() {
        // value содержит `var(` — пропускается без валидации, потому что
        // expand var() происходит позже.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --base: 7px; --gap: var(--base); }",
            "--gap",
        );
        // var(--base) сохранён как есть; resolve будет при apply_declaration.
        assert_eq!(v, Some("var(--base)".to_string()));
    }

    #[test]
    fn property_invalid_initial_value_skipped() {
        // initial-value не подходит под syntax → не подставляется. Без
        // декларации потомка свойство остаётся вне custom_props.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: red; }",
            "--gap",
        );
        assert_eq!(v, None);
    }

    #[test]
    fn property_validate_integer_accepts_signed() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --n { syntax: '<integer>'; inherits: false; initial-value: 0; } p { --n: -42; }",
            "--n",
        );
        assert_eq!(v, Some("-42".to_string()));
    }

    #[test]
    fn property_validate_integer_rejects_float() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --n { syntax: '<integer>'; inherits: false; initial-value: 0; } p { --n: 3.14; }",
            "--n",
        );
        assert_eq!(v, Some("0".to_string()));
    }

    #[test]
    fn property_validate_time_accepts_seconds_and_ms() {
        let v_s = lay_get_custom_prop(
            "<p>x</p>",
            "@property --dur { syntax: '<time>'; inherits: false; initial-value: 0s; } p { --dur: 1.5s; }",
            "--dur",
        );
        assert_eq!(v_s, Some("1.5s".to_string()));

        let v_ms = lay_get_custom_prop(
            "<p>x</p>",
            "@property --dur { syntax: '<time>'; inherits: false; initial-value: 0s; } p { --dur: 200ms; }",
            "--dur",
        );
        assert_eq!(v_ms, Some("200ms".to_string()));
    }

    #[test]
    fn property_validate_time_rejects_non_time() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --dur { syntax: '<time>'; inherits: false; initial-value: 0s; } p { --dur: 100px; }",
            "--dur",
        );
        assert_eq!(v, Some("0s".to_string()));
    }

    #[test]
    fn property_validate_resolution_units() {
        // <resolution> принимает dpi / dpcm / dppx / x (alias dppx).
        for (val, expected) in [
            ("96dpi", "96dpi"),
            ("2dppx", "2dppx"),
            ("38dpcm", "38dpcm"),
            ("2x", "2x"),
        ] {
            let css = format!(
                "@property --r {{ syntax: '<resolution>'; inherits: false; initial-value: 1dppx; }} p {{ --r: {val}; }}"
            );
            let v = lay_get_custom_prop("<p>x</p>", &css, "--r");
            assert_eq!(v, Some(expected.to_string()), "value: {val}");
        }
    }

    #[test]
    fn property_validate_resolution_rejects_non_resolution() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --r { syntax: '<resolution>'; inherits: false; initial-value: 1dppx; } p { --r: 5s; }",
            "--r",
        );
        assert_eq!(v, Some("1dppx".to_string()));
    }

    // ──────── CSS counters (CSS Lists L3 §3) ────────

    fn first_block_style(root: &LayoutBox) -> &ComputedStyle {
        let p = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        &p.style
    }

    #[test]
    fn counter_reset_single_default_zero() {
        let root = lay("<p>x</p>", "p { counter-reset: section; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_reset, vec![("section".to_string(), 0)]);
    }

    #[test]
    fn counter_reset_with_explicit_value() {
        let root = lay("<p>x</p>", "p { counter-reset: section 5; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_reset, vec![("section".to_string(), 5)]);
    }

    #[test]
    fn counter_reset_multiple() {
        let root = lay(
            "<p>x</p>",
            "p { counter-reset: section 1 subsection 0 figure; }",
        );
        let s = first_block_style(&root);
        assert_eq!(
            s.counter_reset,
            vec![
                ("section".to_string(), 1),
                ("subsection".to_string(), 0),
                ("figure".to_string(), 0),  // default = 0
            ]
        );
    }

    #[test]
    fn counter_reset_none_yields_empty() {
        let root = lay("<p>x</p>", "p { counter-reset: none; }");
        let s = first_block_style(&root);
        assert!(s.counter_reset.is_empty());
    }

    #[test]
    fn counter_reset_case_insensitive_none() {
        let root = lay("<p>x</p>", "p { counter-reset: NONE; }");
        let s = first_block_style(&root);
        assert!(s.counter_reset.is_empty());
    }

    #[test]
    fn counter_increment_default_one() {
        let root = lay("<p>x</p>", "p { counter-increment: section; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_increment, vec![("section".to_string(), 1)]);
    }

    #[test]
    fn counter_increment_with_explicit_value() {
        let root = lay("<p>x</p>", "p { counter-increment: section 2; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_increment, vec![("section".to_string(), 2)]);
    }

    #[test]
    fn counter_increment_multiple_with_mixed_defaults() {
        let root = lay(
            "<p>x</p>",
            "p { counter-increment: a 3 b c 5; }",
        );
        let s = first_block_style(&root);
        assert_eq!(
            s.counter_increment,
            vec![
                ("a".to_string(), 3),
                ("b".to_string(), 1),  // default = 1
                ("c".to_string(), 5),
            ]
        );
    }

    #[test]
    fn counter_set_default_zero() {
        // CSS Lists L3 §4 — `counter-set: name` без числа → значение 0.
        let root = lay("<p>x</p>", "p { counter-set: section; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_set, vec![("section".to_string(), 0)]);
    }

    #[test]
    fn counter_set_with_explicit_value() {
        let root = lay("<p>x</p>", "p { counter-set: section 5; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_set, vec![("section".to_string(), 5)]);
    }

    #[test]
    fn counter_set_multiple_with_mixed_defaults() {
        let root = lay("<p>x</p>", "p { counter-set: a 3 b c 5; }");
        let s = first_block_style(&root);
        assert_eq!(
            s.counter_set,
            vec![
                ("a".to_string(), 3),
                ("b".to_string(), 0), // default = 0
                ("c".to_string(), 5),
            ]
        );
    }

    #[test]
    fn counter_set_none_yields_empty() {
        let root = lay("<p>x</p>", "p { counter-set: none; }");
        let s = first_block_style(&root);
        assert!(s.counter_set.is_empty());
    }

    #[test]
    fn counter_set_not_inherited_by_default() {
        // counter-set не наследуется (CSS Lists L3 §4).
        let root = lay(
            "<div><p>x</p></div>",
            "div { counter-set: section 3; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(p.style.counter_set.is_empty());
        assert!(!div.style.counter_set.is_empty());
    }

    #[test]
    fn counter_not_inherited_by_default() {
        // counter-reset / -increment не наследуются (CSS Lists L3 §3).
        let root = lay(
            "<div><p>x</p></div>",
            "div { counter-reset: section; }",
        );
        // У <p> не должно быть счётчиков.
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(p.style.counter_reset.is_empty());
        assert!(!div.style.counter_reset.is_empty());  // у div есть
    }

    #[test]
    fn counter_inherit_keyword_pulls_from_parent() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { counter-reset: section 7; } p { counter-reset: inherit; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.counter_reset, vec![("section".to_string(), 7)]);
    }

    #[test]
    fn counter_initial_keyword_resets_to_empty() {
        let root = lay(
            "<p>x</p>",
            "p { counter-reset: section 5; counter-reset: initial; }",
        );
        let s = first_block_style(&root);
        assert!(s.counter_reset.is_empty());
    }

    #[test]
    fn invalid_ident_in_counter_list_skipped() {
        // Имя с цифрой первым символом — невалидный CSS-ident, должен пропуститься.
        let root = lay(
            "<p>x</p>",
            "p { counter-reset: 1invalid valid 2; }",
        );
        let s = first_block_style(&root);
        assert_eq!(s.counter_reset, vec![("valid".to_string(), 2)]);
    }

    // ──────── @media queries (Media Queries L4) ────────

    pub(crate) fn lay_with_viewport(html: &str, css: &str, vw: f32, vh: f32) -> LayoutBox {
        use lumen_dom::Document;
        use lumen_core::Size;
        let document: Document = lumen_html_parser::parse(html);
        let stylesheet = lumen_css_parser::parse(css);
        let viewport = Size { width: vw, height: vh };
        body_layout_box(crate::layout(&document, &stylesheet, viewport))
    }

    #[test]
    fn media_min_width_matches_wide_viewport() {
        // @media (min-width: 600px) { p { color: red; } }
        // viewport 800×600 → match.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 600px) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_min_width_skips_narrow_viewport() {
        // viewport 500×600 → НЕ match (500 < 600).
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 600px) { p { color: red; } }",
            500.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // default color = BLACK (initial).
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_max_width_matches_narrow() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-width: 500px) { p { color: blue; } }",
            400.0,
            300.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn media_orientation_landscape() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (orientation: landscape) { p { color: green; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn media_orientation_portrait_does_not_match_landscape() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (orientation: portrait) { p { color: green; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_screen_type_always_matches() {
        // Phase 0 MediaContext always media_type="screen".
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media screen { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_print_type_does_not_match() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media print { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_and_combination() {
        // @media (min-width: 600px) and (orientation: landscape) → match
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 600px) and (orientation: landscape) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_or_via_comma() {
        // @media (max-width: 400px), (min-width: 700px) → match при viewport=800
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-width: 400px), (min-width: 700px) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_rule_overrides_regular() {
        // Source order: p{color:red}, потом @media(match){p{color:blue}}.
        // @media rules идут после regular в нашем cascade-ordering,
        // поэтому blue побеждает.
        let root = lay_with_viewport(
            "<p>x</p>",
            "p { color: red; } @media (min-width: 100px) { p { color: blue; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn media_unknown_feature_does_not_match() {
        // (unknown-feature: value) → Unsupported → не match.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (color-gamut: p3) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_min_width_em_applies() {
        // 48em = 768px; viewport 1024 → матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 48em) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_min_width_em_no_match_narrow() {
        // 48em = 768px; viewport 600 → не матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 48em) { p { color: red; } }",
            600.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_max_width_rem_applies() {
        // 50rem = 800px; viewport 600 → матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-width: 50rem) { p { color: blue; } }",
            600.0,
            480.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn media_width_exact_matches() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (width: 1024px) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_width_exact_no_match() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (width: 800px) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_min_aspect_ratio_matches() {
        // min-aspect-ratio: 1/1; 1024/720 > 1 → матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-aspect-ratio: 1/1) { p { color: green; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn media_max_aspect_ratio_no_match() {
        // max-aspect-ratio: 4/3 ≈ 1.333; 1024/720 ≈ 1.422 → не матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-aspect-ratio: 4/3) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_reeval_on_resize_wider() {
        // При маленьком viewport — не матчит; при увеличении — матчит.
        let css = "@media (min-width: 600px) { p { color: red; } }";
        let narrow = lay_with_viewport("<p>x</p>", css, 400.0, 600.0);
        let p_narrow = narrow.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p_narrow.style.color, Color::BLACK);

        let wide = lay_with_viewport("<p>x</p>", css, 1024.0, 600.0);
        let p_wide = wide.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p_wide.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn display_flex_parses_and_stores() {
        let root = lay("<p>x</p>", "p { display: flex; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.display, Display::Flex);
    }

    #[test]
    fn display_inline_flex_parses_and_stores() {
        // BUG-739: inline-flex — atomic inline-level бокс (CSS Display L3 §2.1),
        // а не inline-family: он получает СВОЙ бокс внутри InlineBlockRow, а не
        // уплощается в сегменты родительского InlineRun (так было до фикса).
        let root = lay("<div><span>x</span></div>", "span { display: inline-flex; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(matches!(&div.children[0].kind, BoxKind::InlineBlockRow));
        let item = &div.children[0].children[0];
        assert_eq!(item.style.display, Display::InlineFlex);
    }

    #[test]
    fn display_grid_parses_as_block_family() {
        let root = lay("<p>x</p>", "p { display: grid; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.display, Display::Grid);
    }

    #[test]
    fn display_inline_grid_creates_its_own_box() {
        // BUG-739, симметрично `display_inline_flex_parses_and_stores`.
        let root = lay("<div><span>x</span></div>", "span { display: inline-grid; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(matches!(&div.children[0].kind, BoxKind::InlineBlockRow));
        let item = &div.children[0].children[0];
        assert_eq!(item.style.display, Display::InlineGrid);
    }

    #[test]
    fn display_inline_block_creates_inline_block_row() {
        // display:inline-block элементы внутри div группируются в InlineBlockRow.
        let root = lay(
            "<div><span>a</span><span>b</span></div>",
            "span { display: inline-block; width: 50px; height: 20px; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // div должен иметь один дочерний InlineBlockRow.
        assert!(
            div.children.iter().any(|c| matches!(&c.kind, BoxKind::InlineBlockRow)),
            "expected InlineBlockRow in div, got: {:?}", div.children.iter().map(|c| &c.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn display_inline_block_parses_style() {
        // <p display:inline-block> попадает в InlineBlockRow, не как прямой Block.
        let root = lay("<p>x</p>", "p { display: inline-block; }");
        // Ищем InlineBlockRow в дереве, внутри него первый child — это <p>.
        fn find_row(b: &LayoutBox) -> Option<&LayoutBox> {
            if matches!(b.kind, BoxKind::InlineBlockRow) {
                return Some(b);
            }
            b.children.iter().find_map(find_row)
        }
        let row = find_row(&root).expect("InlineBlockRow not found");
        let p = row.children.first().expect("p not found in row");
        assert_eq!(p.style.display, Display::InlineBlock);
    }

    #[test]
    fn inline_block_row_lays_out_horizontally() {
        // Два inline-block 50×20 должны оказаться рядом по горизонтали.
        let root = lay_measured(
            "<div><span>a</span><span>b</span></div>",
            "span { display: inline-block; width: 50px; height: 20px; }",
            800.0,
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let row = div.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        assert_eq!(row.children.len(), 2, "InlineBlockRow должен содержать 2 child");
        let a = &row.children[0];
        let b_box = &row.children[1];
        // a.rect.x < b.rect.x — лежат горизонтально
        assert!(a.rect.x < b_box.rect.x, "первый span должен быть левее второго");
        // b.rect.x ≥ a.rect.x + a.rect.width
        assert!(b_box.rect.x >= a.rect.x + a.rect.width,
            "второй span не должен перекрываться с первым");
    }

    #[test]
    fn inline_block_row_without_text_has_no_strut_descent() {
        // CSS §10.8 / Edge-верификация (TEST-11/TEST-12/TEST-34):
        // ряд из baseline-aligned inline-block-ов получает strut_descent.
        // ряд из bottom-aligned inline-block-ов strut НЕ получает.
        //
        // Strut — content area шрифта ряда без half-leading (descent 0.2em у
        // тестового измерителя). Почему без него — в `box_tree.rs`, ветка
        // `BoxKind::InlineBlockRow`: `line-height: normal` здесь 1.2em, и
        // half-leading от него делает строку выше, чем в Edge (IFC-1, A/B на
        // TEST-02/04/21/56).
        let root_baseline = lay_measured(
            "<div><span></span><span></span></div>",
            "span { display: inline-block; width: 50px; height: 80px; }",
            body_w_or_default(),
        );
        let div = root_baseline.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let row = div.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        // Default vertical-align = baseline → strut 3.2px добавляется. height = 83.2.
        assert!(
            (row.rect.height - 83.2).abs() < 0.1,
            "baseline-ряд: 83.2px (80+strut), got {}",
            row.rect.height
        );
        // bottom-aligned row: no strut.
        let root_bottom = lay_measured(
            "<div><span></span><span></span></div>",
            "span { display: inline-block; width: 50px; height: 80px; vertical-align: bottom; }",
            body_w_or_default(),
        );
        let div2 = root_bottom.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let row2 = div2.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        assert!(
            (row2.rect.height - 80.0).abs() < 0.1,
            "bottom-ряд: 80px (нет strut), got {}",
            row2.rect.height
        );
    }

    #[test]
    fn inline_block_row_with_text_keeps_strut_descent() {
        // InlineRun всегда baseline-aligned → strut добавляется к ряду с текстом.
        let css = "span { display: inline-block; width: 50px; height: 20px; } \
                   div { font-size: 16px; }";
        let no_text = lay_measured("<div><span></span></div>", css, body_w_or_default());
        let with_text = lay_measured("<div>txt<span></span></div>", css, body_w_or_default());
        let row_no_text = no_text.children[0].children.iter()
            .find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        let row_with_text = with_text.children[0].children.iter()
            .find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        // span default va=baseline → strut в обоих случаях. Оба ≥ 23.2.
        let expected_min = 20.0 + 16.0 * 0.2;
        assert!(
            row_no_text.rect.height >= expected_min - 0.1,
            "Ряд без текста: ≥{expected_min:.1}px, got {}",
            row_no_text.rect.height
        );
        assert!(
            row_with_text.rect.height >= expected_min - 0.1,
            "Ряд с текстом: ≥{expected_min:.1}px, got {}",
            row_with_text.rect.height
        );
    }

    #[test]
    fn inline_block_rows_no_drift_after_block_sep() {
        // baseline-aligned ряды добавляют strut_descent, bottom-aligned — нет.
        // Fixed8 strut = 16*0.2 = 3.2. row1(83.2) + sep(40) + row2(83.2) = 206.4.
        let root = lay_measured(
            "<div>\
              <div class=ib></div><div class=ib></div>\
              <div class=sep></div>\
              <div class=ib></div><div class=ib></div>\
             </div>",
            ".ib { display: inline-block; width: 50px; height: 80px; } \
             .sep { height: 40px; }",
            body_w_or_default(),
        );
        let outer = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Default va=baseline → strut: row1(83.2) + sep(40) + row2(83.2) = 206.4.
        assert!(
            (outer.rect.height - 206.4).abs() < 0.2,
            "baseline-ряды: 206.4px (2×strut 3.2px), got {}",
            outer.rect.height
        );
        // bottom-aligned ряды: нет strut → row1(80) + sep(40) + row2(80) = 200.
        let root_bot = lay_measured(
            "<div>\
              <div class=ib></div><div class=ib></div>\
              <div class=sep></div>\
              <div class=ib></div><div class=ib></div>\
             </div>",
            ".ib { display: inline-block; width: 50px; height: 80px; vertical-align: bottom; } \
             .sep { height: 40px; }",
            body_w_or_default(),
        );
        let outer_bot = root_bot.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(
            (outer_bot.rect.height - 200.0).abs() < 0.1,
            "bottom-ряды: 200px (без strut), got {}",
            outer_bot.rect.height
        );
    }

    fn body_w_or_default() -> f32 { 800.0 }

    #[test]
    fn display_unknown_value_keeps_previous() {
        // unknown value игнорируется — лог по умолчанию остаётся.
        let root = lay("<p>x</p>", "p { display: zomg-flexed; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Default для <p> от UA = Block.
        assert_eq!(p.style.display, Display::Block);
    }
