use super::*;

    /// Navigate the document layout tree root → html → body and return the
    /// body `LayoutBox`. Tests were written for the old flat DOM structure
    /// (before the HTML5 parser started injecting implicit html/head/body
    /// wrappers). This helper adapts them without touching production code.
    pub(crate) fn body_layout_box(mut root: LayoutBox) -> LayoutBox {
        // root children: [html block, ...]
        if let Some(html_idx) = root
            .children
            .iter()
            .position(|c| matches!(c.kind, BoxKind::Block))
        {
            let mut html_box = root.children.remove(html_idx);
            // html children: [body block, ...]
            if let Some(body_idx) = html_box
                .children
                .iter()
                .position(|c| matches!(c.kind, BoxKind::Block))
            {
                return html_box.children.remove(body_idx);
            }
            return html_box;
        }
        root
    }

    /// UA `body { margin: 8px }` (HTML Rendering §14.3.3, BUG-204) shifts all
    /// normal-flow content by 8px. These layout-unit helpers test child geometry
    /// in isolation, so they neutralise the UA body margin with an explicit reset
    /// — exactly as real pages do via `* { margin: 0 }`. Tests that specifically
    /// verify the UA body margin use the `cascade_at` style helper instead.
    const BODY_RESET: &str = "body{margin:0}";

    pub(crate) fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
        body_layout_box(layout(&doc, &sheet, Size::new(800.0, 600.0)))
    }

    pub(crate) fn lay_viewport(html: &str, css: &str, vp: Size) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
        body_layout_box(layout(&doc, &sheet, vp))
    }

    /// Измеритель с фиксированной шириной 8px на символ.
    pub(crate) struct Fixed8;
    impl TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    pub(crate) fn lay_measured(html: &str, css: &str, width: f32) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
        body_layout_box(layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8))
    }

    /// Like `lay()` but returns the full layout tree root (document box),
    /// not the body box. Use when a test explicitly needs to inspect
    /// the `<html>` or `<body>` layout boxes.
    pub(crate) fn lay_full(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    /// Like `lay_full`, but also returns the `Document` — needed by callers of
    /// `collect_layout_rects`/`collect_computed_styles` (BUG-488), which walk
    /// DOM ancestry to attribute inline-element boxes.
    fn lay_full_with_doc(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        (doc, root)
    }

    /// Like `lay_full_with_doc`, but lays out with `Fixed8` instead of `layout()`'s
    /// no-op measurer — needed by any test asserting on the *width* of wrapped
    /// text (`layout()` never measures a glyph, so every text frag is 0-wide).
    fn lay_full_measured_with_doc(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        (doc, root)
    }

    /// BUG-382: an element with inline content owns two boxes with the same
    /// `NodeId` — its principal block box and the anonymous run holding the text.
    /// Both snapshot collectors must answer with the principal one; the earlier
    /// `insert` handed JS the inner run, so `getBoundingClientRect().height` was
    /// the line height and `getComputedStyle().width` was `auto`.
    #[test]
    fn snapshot_collectors_keep_the_principal_box() {
        let (doc, root) = lay_full_with_doc(
            "<html><body><div id=a>x</div></body></html>",
            "body{margin:0} #a{width:50px;height:20px}",
        );
        // Walk in the same order the collectors do, recording every box per node.
        fn walk<'a>(b: &'a LayoutBox, out: &mut Vec<(u32, &'a LayoutBox)>) {
            out.push((b.node.index() as u32, b));
            for c in &b.children {
                walk(c, out);
            }
        }
        let mut boxes = Vec::new();
        walk(&root, &mut boxes);

        let div_nid = boxes
            .iter()
            .find(|(nid, _)| boxes.iter().filter(|(n, _)| n == nid).count() > 1)
            .map(|(nid, _)| *nid)
            .expect("expected a NodeId owning more than one box");
        let principal = boxes
            .iter()
            .find(|(nid, _)| *nid == div_nid)
            .map(|(_, b)| *b)
            .expect("principal box");
        assert_eq!(principal.rect.height, 20.0, "specified height must win");

        let rects = collect_layout_rects(&root, &doc);
        assert_eq!(
            rects[&div_nid],
            [
                principal.rect.x,
                principal.rect.y,
                principal.rect.width,
                principal.rect.height
            ],
            "rect snapshot must describe the element's own box"
        );

        let styles = collect_computed_styles(&root, &doc);
        assert_eq!(
            styles[&div_nid].get("width").map(String::as_str),
            Some("50px"),
            "style snapshot must describe the element's own box"
        );
    }

    /// BUG-488: a plain inline-level element (`<span>`, `<em>`, …) owns no
    /// `LayoutBox` of its own — its content is flattened into the enclosing
    /// `InlineRun`'s segments, so neither collector's key set ever contained
    /// the inline element's own `NodeId`. `getComputedStyle()`/
    /// `getBoundingClientRect()` on such an element answered `""` / an
    /// all-zero rect regardless of content, indistinguishable from "element
    /// doesn't exist".
    #[test]
    fn snapshot_collectors_cover_plain_inline_elements() {
        let (doc, root) = lay_full_measured_with_doc(
            "<html><body><div>before <span id=s style=\"color:red\">hi</span> after</div></body></html>",
            "body{margin:0}",
        );
        let span_nid = find_first_dom_node_by_selector(&doc, "#s")
            .expect("span must be findable in the DOM")
            .index() as u32;

        let rects = collect_layout_rects(&root, &doc);
        let rect = rects
            .get(&span_nid)
            .expect("inline element must have a rect entry");
        assert!(rect[2] > 0.0, "span must have a nonzero width: {rect:?}");
        assert!(rect[3] > 0.0, "span must have a nonzero height: {rect:?}");

        let styles = collect_computed_styles(&root, &doc);
        let style = styles
            .get(&span_nid)
            .expect("inline element must have a computed-style entry");
        assert_eq!(
            style.get("color").map(String::as_str),
            Some("rgb(255, 0, 0)"),
            "inline element's own declared style must be visible"
        );
        assert_eq!(style.get("display").map(String::as_str), Some("inline"));
    }

    /// BUG-732: `getComputedStyle(el).getPropertyValue('--x')` answered `""`
    /// because nothing published custom properties at all. The snapshot carries
    /// *computed* values — `var()` chains substituted, an unresolvable
    /// reference reported as the empty string (guaranteed-invalid).
    #[test]
    fn custom_property_snapshot_resolves_var_chains() {
        let root = lay_full(
            "<html><body><div id=a>x</div></body></html>",
            ":root{--base:8px;--gap:var(--base)} #a{--own:2px;--broken:var(--nope)}",
        );
        let props = collect_custom_properties(&root);
        let div_nid = {
            fn find(b: &LayoutBox, out: &mut Option<u32>) {
                if b.style.custom_props.contains_key("--own") && out.is_none() {
                    *out = Some(b.node.index() as u32);
                }
                for c in &b.children {
                    find(c, out);
                }
            }
            let mut found = None;
            find(&root, &mut found);
            found.expect("the div declaring --own must own a box")
        };
        let div = &props[&div_nid];
        assert_eq!(div.get("--own").map(String::as_str), Some("2px"));
        assert_eq!(
            div.get("--gap").map(String::as_str),
            Some("8px"),
            "inherited var() chain must be substituted, not published raw"
        );
        assert_eq!(
            div.get("--broken").map(String::as_str),
            Some(""),
            "an unresolvable var() is guaranteed-invalid, i.e. the empty string"
        );
    }

    /// The whole point of the separate, `Arc`-shared snapshot: nodes that only
    /// inherit a set of variables must share one allocation with the node that
    /// declared it, not carry a copy each (see `collect_custom_properties`).
    #[test]
    fn custom_property_snapshot_shares_one_allocation_per_distinct_map() {
        let root = lay_full(
            "<html><body><div id=a><p>x</p></div></body></html>",
            ":root{--base:8px}",
        );
        let props = collect_custom_properties(&root);
        assert!(props.len() > 1, "every node inherits --base");
        let mut maps = props.values();
        let first = maps.next().expect("at least one node");
        for other in maps {
            assert!(
                std::sync::Arc::ptr_eq(first, other),
                "one declared set must stay one allocation across the document"
            );
        }
    }

    pub(crate) fn first_element_child(b: &LayoutBox) -> &LayoutBox {
        fn is_element(k: &BoxKind) -> bool {
            matches!(
                k,
                BoxKind::Block
                    | BoxKind::FormControl { .. }
                    | BoxKind::TableRow
                    | BoxKind::Table
                    | BoxKind::TableRowGroup
            )
        }
        // Form controls and other inline-block elements are wrapped in an
        // anonymous InlineBlockRow (and text in an InlineRun); descend through
        // those anonymous containers to find the first real element box.
        fn rec(b: &LayoutBox) -> Option<&LayoutBox> {
            for c in &b.children {
                if is_element(&c.kind) {
                    return Some(c);
                }
                if matches!(c.kind, BoxKind::InlineBlockRow | BoxKind::InlineRun { .. })
                    && let Some(found) = rec(c)
                {
                    return Some(found);
                }
            }
            None
        }
        rec(b).expect("expected at least one element child")
    }

    /// DFS search: first box in tree (including `b` itself) matching the predicate.
    pub(crate) fn find_box(b: &LayoutBox, pred: impl Fn(&BoxKind) -> bool + Copy) -> Option<&LayoutBox> {
        if pred(&b.kind) {
            return Some(b);
        }
        for c in &b.children {
            if let Some(found) = find_box(c, pred) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn empty_document() {
        let root = lay("", "");
        assert_eq!(root.rect.width, 800.0);
        assert_eq!(root.rect.height, 0.0);
    }

    #[test]
    fn single_paragraph_height_one_line() {
        let root = lay("<p>hello</p>", "");
        // root → <p> → text. Высота: font_size 16 * line_height 1.2 = 19.2
        assert!(
            (root.rect.height - 19.2).abs() < 0.1,
            "got {}",
            root.rect.height
        );
    }

    #[test]
    fn stacked_blocks_height_sums() {
        let root = lay("<p>a</p><p>b</p><p>c</p>", "");
        // 3 строки по 19.2
        assert!((root.rect.height - 57.6).abs() < 0.1);
    }

    #[test]
    fn whitespace_only_text_skipped() {
        let root = lay("<p>a</p>\n  \n<p>b</p>", "");
        // Пробельные узлы между <p> не должны давать вертикального пространства.
        assert!((root.rect.height - 38.4).abs() < 0.1);
    }

    #[test]
    fn css_color_applied_via_type_selector() {
        let root = lay("<p>x</p>", "p { color: red; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.color,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }
        );
    }

    #[test]
    fn class_selector_matches() {
        let root = lay(r#"<div class="hero">x</div>"#, ".hero { color: red; }");
        let div = first_element_child(&root);
        assert_eq!(div.style.color.r, 255);
    }

    #[test]
    fn id_selector_matches() {
        let root = lay(r#"<div id="main">x</div>"#, "#main { color: red; }");
        let div = first_element_child(&root);
        assert_eq!(div.style.color.r, 255);
    }

    #[test]
    fn cyrillic_class_matches() {
        let root = lay(r#"<p class="привет">x</p>"#, ".привет { color: red; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    #[test]
    fn last_rule_wins_without_specificity() {
        let root = lay("<p>x</p>", "p { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
        assert_eq!(p.style.color.r, 0);
    }

    #[test]
    fn font_size_inherited_to_text() {
        let root = lay("<p>x</p>", "p { font-size: 32px; }");
        let p = first_element_child(&root);
        // Текст живёт в InlineRun; стиль контейнера наследует font-size от <p>.
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        assert_eq!(inline.style.font_size, 32.0);
        // 32 * 1.2 = 38.4
        assert!((inline.rect.height - 38.4).abs() < 0.1);
    }

    #[test]
    fn hex_color_full() {
        let root = lay("<p>x</p>", "p { color: #ff8800; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
        assert_eq!(p.style.color.g, 136);
        assert_eq!(p.style.color.b, 0);
    }

    #[test]
    fn hex_color_short() {
        let root = lay("<p>x</p>", "p { color: #f80; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
        assert_eq!(p.style.color.g, 136);
        assert_eq!(p.style.color.b, 0);
    }

    #[test]
    fn display_none_skipped() {
        let root = lay("<p>visible</p><p class=\"x\">hidden</p>", ".x { display: none; }");
        // Один блок отрисуется, второй пропустится (skip).
        // Только одна строка высотой 19.2
        assert!((root.rect.height - 19.2).abs() < 0.1);
    }

    #[test]
    fn padding_increases_height() {
        let root = lay("<p>x</p>", "p { padding: 10px; }");
        let p = first_element_child(&root);
        // Высота: 19.2 (текст) + 10 + 10 (padding) = 39.2
        assert!((p.rect.height - 39.2).abs() < 0.1);
    }

    #[test]
    fn margin_offsets_position() {
        let root = lay("<p>x</p>", "p { margin: 20px; }");
        let p = first_element_child(&root);
        assert!((p.rect.x - 20.0).abs() < 0.01);
        assert!((p.rect.y - 20.0).abs() < 0.01);
        // Ширина: 800 - 20 - 20 = 760
        assert!((p.rect.width - 760.0).abs() < 0.01);
    }

    #[test]
    fn background_color_stored() {
        let root = lay("<p>x</p>", "p { background-color: #ff0000; }");
        let p = first_element_child(&root);
        assert!(matches!(p.style.background_color, Some(CssColor::Rgba(_))));
        assert!(matches!(p.style.background_color, Some(CssColor::Rgba(Color { r: 255, .. }))));
    }

    #[test]
    fn color_fn_display_p3_parsed_as_wide() {
        let root = lay("<p>x</p>", "p { background-color: color(display-p3 1 0 0); }");
        let p = first_element_child(&root);
        assert!(
            matches!(p.style.background_color, Some(CssColor::Wide(f)) if f.space == ColorSpace::DisplayP3),
            "display-p3 should parse to CssColor::Wide with DisplayP3 space"
        );
    }

    #[test]
    fn color_fn_srgb_parsed_as_wide() {
        let root = lay("<p>x</p>", "p { background-color: color(srgb 0.5 0.5 0.5); }");
        let p = first_element_child(&root);
        assert!(
            matches!(p.style.background_color, Some(CssColor::Wide(f)) if f.space == ColorSpace::Srgb),
            "srgb should parse to CssColor::Wide with Srgb space"
        );
    }

    #[test]
    fn color_fn_rec2020_parsed_as_wide() {
        let root = lay("<p>x</p>", "p { background-color: color(rec2020 0.3 0.6 0.9); }");
        let p = first_element_child(&root);
        assert!(
            matches!(p.style.background_color, Some(CssColor::Wide(f)) if f.space == ColorSpace::Rec2020),
            "rec2020 should parse to CssColor::Wide with Rec2020 space"
        );
    }

    #[test]
    fn color_fn_display_p3_with_alpha() {
        let root = lay("<p>x</p>", "p { background-color: color(display-p3 1 0 0 / 0.5); }");
        let p = first_element_child(&root);
        if let Some(CssColor::Wide(f)) = p.style.background_color {
            assert!((f.r - 1.0).abs() < 0.001);
            assert!(f.g.abs() < 0.001);
            assert!(f.b.abs() < 0.001);
            assert!((f.a - 0.5).abs() < 0.001);
        } else {
            panic!("expected Wide color with alpha");
        }
    }

    #[test]
    fn color_fn_display_p3_to_srgb_red() {
        // display-p3 red (1 0 0) → sRGB: P3-red выходит за gamut sRGB.
        let f = ColorFloat { r: 1.0, g: 0.0, b: 0.0, a: 1.0, space: ColorSpace::DisplayP3 };
        let c = f.to_srgb_color();
        assert!(c.r > 200, "r={}", c.r);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn head_and_its_metadata_are_hidden() {
        // <title> и <style> содержимое не должно рендериться как видимый
        // текст. Высота итогового layout-а должна совпадать с высотой только
        // одного <p>visible</p> внутри <body>.
        let just_body = lay("<html><body><p>visible</p></body></html>", "");
        let with_head = lay(
            r#"<html>
                <head>
                    <title>Не должно рендериться</title>
                    <style>p { color: red; }</style>
                    <meta charset="utf-8">
                </head>
                <body><p>visible</p></body>
            </html>"#,
            "",
        );
        // Высоты должны совпадать с точностью до окружающих whitespace text-node-ов
        // (которые сами по себе skip-аются как пустые).
        assert!(
            (with_head.rect.height - just_body.rect.height).abs() < 0.1,
            "head content leaked: just_body={}, with_head={}",
            just_body.rect.height,
            with_head.rect.height,
        );
    }

    #[test]
    fn nested_inheritance() {
        let root = lay(
            "<div><p>nested</p></div>",
            "div { font-size: 24px; color: blue; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // font-size наследуется с div к p
        assert_eq!(p.style.font_size, 24.0);
        // color тоже
        assert_eq!(p.style.color.b, 255);
    }

    // ── Тесты line wrapping ─────────────────────────────────────────────────

    /// Fixed8: "hello world" = 11 символов × 8px = 88px.
    /// При viewport 60px ("hello" = 40px влезает, "world" = 40px → перенос).
    #[test]
    fn wrap_two_words_into_two_lines() {
        let root = lay_measured("<p>hello world</p>", "", 60.0);
        // root → <p> → text (2 строки). 2 × (16 * 1.2) = 38.4
        assert!(
            (root.rect.height - 38.4).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// При достаточно широком viewport слова не переносятся.
    #[test]
    fn no_wrap_when_text_fits() {
        // "hello" = 5×8 = 40px, viewport 100px — переноса нет.
        let root = lay_measured("<p>hello</p>", "", 100.0);
        assert!((root.rect.height - 19.2).abs() < 0.1, "height={}", root.rect.height);
    }

    /// Перенос работает корректно для кириллического текста.
    #[test]
    fn wrap_cyrillic_text() {
        // "Привет мир" = 10 × 8 = 80px при Fixed8.
        // Viewport 50px: "Привет" = 6×8=48px ≤ 50, " " + "мир" = 8+24=32 → 48+8+24=80 > 50.
        let root = lay_measured("<p>Привет мир</p>", "", 50.0);
        // 2 строки
        assert!((root.rect.height - 38.4).abs() < 0.1, "height={}", root.rect.height);
    }

    /// Одно слово, которое само по себе шире viewport, остаётся в одной строке.
    #[test]
    fn single_wide_word_stays_on_one_line() {
        // "superlongword" = 13×8 = 104px > 80px viewport — всё равно одна строка.
        let root = lay_measured("<p>superlongword</p>", "", 80.0);
        assert!((root.rect.height - 19.2).abs() < 0.1, "height={}", root.rect.height);
    }

    /// layout() без измеритея = одна строка независимо от ширины.
    #[test]
    fn layout_without_measurer_no_wrap() {
        let root = lay("<p>a b c d e f g h i j</p>", "");
        // layout() без measurer — всегда одна строка
        assert!((root.rect.height - 19.2).abs() < 0.1);
    }

    // ── Тесты расширенных селекторов ───────────────────────────────────────

    /// Находит первого потомка-блока с заданным тегом, рекурсивно.
    pub(crate) fn find_by_tag<'a>(b: &'a LayoutBox, tag: &str, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(b.node).data
            && name.local == tag
        {
            return Some(b);
        }
        for c in &b.children {
            if let Some(f) = find_by_tag(c, tag, doc) {
                return Some(f);
            }
        }
        None
    }

    /// Утилита: layout + Document, чтобы можно было искать элемент по тегу.
    /// Возвращает LayoutBox тела документа (<body>), а не корня.
    pub(crate) fn lay_with_doc(html: &str, css: &str) -> (LayoutBox, lumen_dom::Document) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = body_layout_box(layout(&doc, &sheet, Size::new(800.0, 600.0)));
        (root, doc)
    }

    #[test]
    fn compound_type_and_class_matches() {
        let (root, doc) = lay_with_doc(
            r#"<p class="hl">x</p><p>y</p>"#,
            "p.hl { color: red; }",
        );
        let mut paragraphs = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                paragraphs.push(c);
            }
        }
        assert_eq!(paragraphs.len(), 2);
        // Первый <p class="hl"> — красный, второй <p> — наследует чёрный.
        assert_eq!(paragraphs[0].style.color.r, 255);
        assert_eq!(paragraphs[1].style.color.r, 0);
    }

    #[test]
    fn descendant_combinator_matches() {
        let (root, doc) = lay_with_doc(
            "<div><p>nested</p></div><p>top</p>",
            "div p { color: red; }",
        );
        // Найдём <p> внутри <div> и <p> прямо в root.
        let div_box = root
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div"))
            .unwrap();
        let nested_p = find_by_tag(div_box, "p", &doc).unwrap();
        assert_eq!(nested_p.style.color.r, 255, "nested <p> should be red");

        let top_p = root
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p"))
            .unwrap();
        assert_eq!(top_p.style.color.r, 0, "top-level <p> should NOT match");
    }

    #[test]
    fn child_combinator_only_direct() {
        let (root, doc) = lay_with_doc(
            "<ul><li>a</li><div><li>b</li></div></ul>",
            "ul > li { color: red; }",
        );
        let ul = find_by_tag(&root, "ul", &doc).unwrap();
        // Прямой <li> — красный.
        let direct_li = ul
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "li"))
            .unwrap();
        assert_eq!(direct_li.style.color.r, 255);
        // Вложенный <li> — не должен матчить, наследует чёрный.
        let div = find_by_tag(ul, "div", &doc).unwrap();
        let nested_li = find_by_tag(div, "li", &doc).unwrap();
        assert_eq!(nested_li.style.color.r, 0);
    }

    #[test]
    fn next_sibling_combinator_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>t</h1><p>a</p><p>b</p>",
            "h1 + p { color: red; }",
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        // Только первый <p> сразу после <h1> матчит.
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn later_sibling_combinator_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>t</h1><p>a</p><p>b</p>",
            "h1 ~ p { color: red; }",
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        // Оба <p> после <h1> матчат.
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 255);
    }

    #[test]
    fn attribute_equals_matches() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="ru">x</p><p lang="en">y</p>"#,
            r#"[lang="ru"] { color: red; }"#,
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn attribute_presence_matches() {
        // <a> — inline-элемент, поэтому собирается в InlineRun. Чтобы получить
        // независимые блочные children для проверки style, используем <div>.
        let (root, doc) = lay_with_doc(
            r#"<div data-x="1">a</div><div>b</div>"#,
            "[data-x] { color: red; }",
        );
        let mut divs = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
            {
                divs.push(c);
            }
        }
        assert_eq!(divs[0].style.color.r, 255);
        assert_eq!(divs[1].style.color.r, 0);
    }

    #[test]
    fn attribute_dash_match_for_lang() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="ru-RU">x</p><p lang="ruler">y</p>"#,
            r#"[lang|="ru"] { color: red; }"#,
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        // "ru-RU" матчит (`ru` или `ru-…`), "ruler" — нет.
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn pseudo_first_child_matches() {
        let (root, doc) = lay_with_doc("<p>a</p><p>b</p><p>c</p>", "p:first-child { color: red; }");
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn pseudo_last_child_matches() {
        let (root, doc) = lay_with_doc("<p>a</p><p>b</p><p>c</p>", "p:last-child { color: red; }");
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        assert_eq!(ps[2].style.color.r, 255);
        assert_eq!(ps[0].style.color.r, 0);
    }

    #[test]
    fn pseudo_hover_never_matches() {
        let root = lay("<p>x</p>", "p:hover { color: red; }");
        let p = first_element_child(&root);
        // :hover без set_interactive_state не матчит.
        assert_eq!(p.style.color.r, 0);
    }

    // ── Interactive pseudo-classes: :hover / :focus / :active ────────────────

    fn node_named(lb: &LayoutBox, doc: &lumen_dom::Document, local: &str) -> Option<lumen_dom::NodeId> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(lb.node).data
            && name.local == local { return Some(lb.node); }
        for c in &lb.children { if let Some(n) = node_named(c, doc, local) { return Some(n); } }
        None
    }

    fn lb_named<'a>(lb: &'a LayoutBox, doc: &lumen_dom::Document, local: &str) -> Option<&'a LayoutBox> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(lb.node).data
            && name.local == local { return Some(lb); }
        for c in &lb.children { if let Some(f) = lb_named(c, doc, local) { return Some(f); } }
        None
    }

    #[test]
    fn hover_matches_when_node_is_hovered() {
        let html = "<p>x</p>";
        let css = "p:hover { color: red; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let p_nid = first_element_child(&root_lb).node;
        set_interactive_state(Some(p_nid), None, None);
        let root_hover = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let p_hover = first_element_child(&root_hover);
        assert_eq!(p_hover.style.color.r, 255, ":hover should apply (color red)");
        assert_eq!(p_hover.style.color.g, 0);
    }

    #[test]
    fn hover_matches_ancestor_of_hovered_node() {
        // :hover applies to all ancestors of the hovered node (CSS Selectors L4 §4.3).
        // Use block-level <p> child so it gets its own LayoutBox (inline elements don't).
        let html = "<div><p>x</p></div>";
        let css = "div:hover { background-color: blue; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let p_nid = node_named(&root_lb, &doc, "p").expect("<p> not found");
        set_interactive_state(Some(p_nid), None, None);
        let root_hover = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let div_bg = lb_named(&root_hover, &doc, "div").expect("<div> not found").style.background_color;
        assert!(
            matches!(div_bg, Some(CssColor::Rgba(Color { b: 255, .. }))),
            "parent :hover should match when child is hovered"
        );
    }

    #[test]
    fn hover_does_not_match_non_hovered_node() {
        // Use block-level <div> as the non-hovered element to get a LayoutBox.
        let html = "<p>x</p><div>y</div>";
        let css = "p:hover { color: red; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let div_nid = node_named(&root_lb, &doc, "div").expect("<div> not found");
        set_interactive_state(Some(div_nid), None, None);
        let root_hover = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let p = first_element_child(&root_hover);
        assert_eq!(p.style.color.r, 0, "non-hovered <p> should not match :hover");
    }

    #[test]
    fn focus_matches_exact_node() {
        let html = "<input type='text' />";
        let css = "input:focus { border-color: blue; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let input_nid = first_element_child(&root_lb).node;
        set_interactive_state(None, Some(input_nid), None);
        let root_focus = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let input = first_element_child(&root_focus);
        assert!(
            matches!(input.style.border_top_color, CssColor::Rgba(Color { b: 255, .. })),
            ":focus border-color blue"
        );
    }

    #[test]
    fn active_matches_element_and_ancestor() {
        let html = "<div><button>click</button></div>";
        let css = "div:active { background-color: red; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let btn_nid = node_named(&root_lb, &doc, "button").expect("<button> not found");
        set_interactive_state(None, None, Some(btn_nid));
        let root_active = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let div_bg = lb_named(&root_active, &doc, "div").expect("<div> not found").style.background_color;
        assert!(
            matches!(div_bg, Some(CssColor::Rgba(Color { r: 255, .. }))),
            "parent :active should match when child is active"
        );
    }
