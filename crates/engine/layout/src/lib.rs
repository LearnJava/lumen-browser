//! Layout-движок для Lumen.
//!
//! Block-flow + inline-flow с word-wrapping. Блочные элементы стэкаются
//! вертикально. Текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`,
//! `<strong>`, и т.д.) объединяются в `InlineRun` — анонимный бокс, где
//! слова переносятся как единый поток. Style cascade — specificity-based
//! (CSS3), полный набор Selectors-Level-3 включая `:nth-*` и `:not`.
//!
//! Snapshot-тестирование: `serialize_layout_tree` даёт детерминированный
//! текст layout-дерева для golden-сравнений (см. `tests/snapshot_tests.rs`).
//!
//! Не поддерживается (Phase 2+): flex, grid, float, absolute positioning,
//! font-weight/style на уровне inline.

pub mod animation;
pub mod box_tree;
pub mod property_trees;
pub mod snapshot;
pub mod stacking;
pub mod style;

pub use animation::{AnimValue, AnimationInterpolator, NoopInterpolator};
pub use box_tree::{layout, layout_measured, BoxKind, InlineFrag, InlineSegment, LayoutBox};
pub use property_trees::{
    ClipNode, ClipTree, EffectNode, EffectTree, Mat4, PropertyTreeNodeId, PropertyTrees,
    ScrollNode, ScrollTree, TransformNode, TransformTree,
};
pub use snapshot::serialize_layout_tree;
pub use stacking::{
    box_can_own_stacking_context, creates_stacking_context, PaintOrder, PaintPhase,
    StackingContext, StackingContextId, StackingTree,
};
pub use style::{
    parse_css_wide_keyword, AlignValue, BackgroundAttachment, BackgroundImage, BackgroundRepeat,
    BackgroundSize, BorderStyle, BoxShadow, BoxSizing, BreakValue, ClipPath, Color, ComputedStyle,
    Content, ContentItem, CssWideKeyword, Cursor, Direction, Display, FilterFn, FontStretch,
    FontStyle, FontVariant, FontWeight, Hyphens, Isolation, ListStylePosition, ListStyleType,
    MixBlendMode, ObjectFit, ObjectPosition, Overflow, OverflowWrap, OverscrollBehavior,
    PointerEvents, Position, PositionComponent, ScrollBehavior, ScrollSnapAlign,
    ScrollSnapAlignKeyword, ScrollSnapAxis, ScrollSnapStop, ScrollSnapStrictness, ScrollSnapType,
    ScrollbarGutter, ScrollbarWidth, TextAlign, TextDecorationLine, TextOverflow, TextShadow,
    TextTransform, TransformFn, UserSelect, Visibility, WhiteSpace, WordBreak,
};

/// Интерфейс измерения ширины символов для line wrapping.
///
/// Реализуется на стороне вызывающего кода (paint/shell), где есть доступ
/// к шрифтовым данным. Layout использует его только в `layout_measured()`.
pub trait TextMeasurer {
    /// Ширина символа `ch` при размере шрифта `font_size_px` пикселей.
    /// Возвращает 0.0 для неизвестных символов.
    fn char_width(&self, ch: char, font_size_px: f32) -> f32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;

    fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    /// Измеритель с фиксированной шириной 8px на символ.
    struct Fixed8;
    impl TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    fn lay_measured(html: &str, css: &str, width: f32) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8)
    }

    fn first_block_child(b: &LayoutBox) -> &LayoutBox {
        b.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Block))
            .expect("expected at least one block child")
    }

    fn first_element_child(b: &LayoutBox) -> &LayoutBox {
        first_block_child(b)
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
        assert!(p.style.background_color.is_some());
        assert_eq!(p.style.background_color.unwrap().r, 255);
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
    fn find_by_tag<'a>(b: &'a LayoutBox, tag: &str, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
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
    fn lay_with_doc(html: &str, css: &str) -> (LayoutBox, lumen_dom::Document) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
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
        // :hover в Phase 0 никогда не матчит.
        assert_eq!(p.style.color.r, 0);
    }

    // ── :placeholder-shown (CSS Selectors L4 §15.1) ──

    fn first_named(doc: &lumen_dom::Document, root: &LayoutBox, local: &str) -> Color {
        for c in walk_layout(root) {
            if let lumen_dom::NodeData::Element { name, .. } = &doc.get(c.node).data
                && name.local == local
            {
                return c.style.color;
            }
        }
        panic!("element <{local}> not found");
    }

    fn walk_layout(root: &LayoutBox) -> Vec<&LayoutBox> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(b) = stack.pop() {
            out.push(b);
            for c in b.children.iter().rev() {
                stack.push(c);
            }
        }
        out
    }

    #[test]
    fn placeholder_shown_matches_input_with_placeholder() {
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn placeholder_shown_no_placeholder_attr_no_match() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_whitespace_only_placeholder_no_match() {
        // " " после trim — пустая строка → не матчит.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="   ">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_filled_input_no_match() {
        // value-атрибут с непустым контентом → placeholder скрыт.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name" value="John">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_empty_value_still_matches() {
        // value="" — пользователь ничего не ввёл, placeholder виден.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name" value="">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn placeholder_shown_textarea_matches_when_empty() {
        // <textarea> с placeholder и без текстового контента → матчит.
        let (root, doc) = lay_with_doc(
            r#"<textarea placeholder="Bio"></textarea>"#,
            "textarea:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn placeholder_shown_textarea_with_text_does_not_match() {
        // <textarea> с текстом — значение задано через DOM children,
        // placeholder скрыт.
        let (root, doc) = lay_with_doc(
            r#"<textarea placeholder="Bio">My biography</textarea>"#,
            "textarea:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 0);
    }

    #[test]
    fn placeholder_shown_non_form_control_skipped() {
        // <div placeholder="...">x</div> — placeholder не имеет смысла на
        // не-form элементе; pseudo-class не матчит.
        let (root, doc) = lay_with_doc(
            r#"<div placeholder="hint">x</div>"#,
            "div:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 0);
    }

    /// Цвет первого layout-box-а с указанным `id`-атрибутом. `panic!`, если
    /// такого нет. Используется в form-state pseudo тестах, где нужно
    /// различать несколько input-ов в одном документе.
    fn color_by_id(doc: &lumen_dom::Document, root: &LayoutBox, id: &str) -> Color {
        for c in walk_layout(root) {
            if let lumen_dom::NodeData::Element { .. } = &doc.get(c.node).data
                && let Some(v) = doc.get(c.node).get_attr("id")
                && v == id
            {
                return c.style.color;
            }
        }
        panic!("element id={id} not found");
    }

    // ──────────────── :required / :optional ────────────────

    #[test]
    fn required_matches_input_with_required_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input required>"#,
            "input:required { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn required_no_match_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:required { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn optional_matches_input_without_required_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:optional { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn optional_no_match_when_required_present() {
        let (root, doc) = lay_with_doc(
            r#"<input required>"#,
            "input:optional { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn required_matches_select_and_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<select id="s" required></select><textarea id="t" required></textarea>"#,
            ":required { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "t").r, 255);
    }

    #[test]
    fn required_skipped_for_hidden_input() {
        // <input type="hidden"> не поддерживает required (HTML5 §4.10.3).
        let (root, doc) = lay_with_doc(
            r#"<input type="hidden" required>"#,
            "input:required { color: red; } input:optional { color: blue; }",
        );
        let c = first_named(&doc, &root, "input");
        assert_eq!(c.r, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn required_matches_checkbox_radio_file() {
        let (root, doc) = lay_with_doc(
            r#"<input id="c" type="checkbox" required>
               <input id="r" type="radio" required>
               <input id="f" type="file" required>"#,
            ":required { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
        assert_eq!(color_by_id(&doc, &root, "r").r, 255);
        assert_eq!(color_by_id(&doc, &root, "f").r, 255);
    }

    #[test]
    fn required_skipped_for_button_and_div() {
        let (root, doc) = lay_with_doc(
            r#"<button id="b" required></button><div id="d" required>x</div>"#,
            ":required { color: red; } :optional { color: blue; }",
        );
        let b = color_by_id(&doc, &root, "b");
        assert_eq!((b.r, b.b), (0, 0), "<button> не имеет required");
        let d = color_by_id(&doc, &root, "d");
        assert_eq!((d.r, d.b), (0, 0), "<div> не имеет required");
    }

    // ──────────────── :read-only / :read-write ────────────────

    #[test]
    fn read_write_matches_plain_input() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_only_matches_readonly_input() {
        let (root, doc) = lay_with_doc(
            r#"<input readonly>"#,
            "input:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_only_matches_disabled_input() {
        let (root, doc) = lay_with_doc(
            r#"<input disabled>"#,
            "input:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_write_matches_plain_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<textarea></textarea>"#,
            "textarea:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn read_only_matches_readonly_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<textarea readonly></textarea>"#,
            "textarea:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn read_only_matches_non_text_input_types() {
        // Не-text-like input types — `:read-only` per HTML5 §4.16.4.
        let (root, doc) = lay_with_doc(
            r#"<input id="h" type="hidden">
               <input id="s" type="submit">
               <input id="r" type="range">
               <input id="c" type="checkbox">"#,
            ":read-only { color: red; } :read-write { color: blue; }",
        );
        assert_eq!(color_by_id(&doc, &root, "h").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "r").r, 255);
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
    }

    #[test]
    fn read_write_matches_contenteditable_true() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true">x</div>"#,
            "div:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_write_matches_contenteditable_empty_attr() {
        // HTML5: contenteditable="" эквивалентно "true".
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable>x</div>"#,
            "div:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_only_matches_contenteditable_false() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="false">x</div>"#,
            "div:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_only_matches_default_div() {
        // Per spec: «matches all other HTML elements» — обычный <div> read-only.
        let (root, doc) = lay_with_doc(
            r#"<div>x</div>"#,
            "div:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_write_inherits_contenteditable_from_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true"><p id="inner">x</p></div>"#,
            "p:read-write { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "inner").r, 255);
    }

    #[test]
    fn read_only_when_descendant_overrides_to_false() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true"><p contenteditable="false" id="inner">x</p></div>"#,
            "p:read-only { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "inner").r, 255);
    }

    // ──────────────── :disabled / :enabled ────────────────

    #[test]
    fn disabled_matches_input_with_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input disabled>"#,
            "input:disabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn enabled_matches_input_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:enabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn disabled_matches_button_select_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<button id="b" disabled>x</button>
               <select id="s" disabled></select>
               <textarea id="t" disabled></textarea>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "t").r, 255);
    }

    #[test]
    fn disabled_matches_fieldset_self() {
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled></fieldset>"#,
            "fieldset:disabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "fieldset").r, 255);
    }

    #[test]
    fn disabled_inherited_from_fieldset_ancestor() {
        // Inputs внутри <fieldset disabled> вне <legend> — disabled.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <input id="i">
                 <select id="s"></select>
               </fieldset>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "i").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
    }

    #[test]
    fn enabled_inside_first_legend_of_disabled_fieldset() {
        // HTML5 §4.10.16: input внутри первого <legend> ребёнка
        // disabled-<fieldset> сохраняет enabled-state.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <legend><input id="legend_input"></legend>
                 <input id="body_input">
               </fieldset>"#,
            ":disabled { color: red; } :enabled { color: blue; }",
        );
        let legend = color_by_id(&doc, &root, "legend_input");
        assert_eq!((legend.r, legend.b), (0, 255), "input в legend остаётся :enabled");
        let body = color_by_id(&doc, &root, "body_input");
        assert_eq!((body.r, body.b), (255, 0), "input вне legend — :disabled");
    }

    #[test]
    fn second_legend_in_disabled_fieldset_still_disabled() {
        // Только ПЕРВЫЙ <legend>-ребёнок «спасает» от disabled. Второй —
        // обычный потомок, попадает под disabled.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <legend>first</legend>
                 <legend><input id="second_legend_input"></legend>
               </fieldset>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "second_legend_input").r, 255);
    }

    #[test]
    fn disabled_option_via_optgroup_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<select>
                 <optgroup disabled>
                   <option id="o">x</option>
                 </optgroup>
               </select>"#,
            "option:disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "o").r, 255);
    }

    #[test]
    fn disabled_option_via_own_attr() {
        let (root, doc) = lay_with_doc(
            r#"<select><option id="o" disabled>x</option></select>"#,
            "option:disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "o").r, 255);
    }

    #[test]
    fn disabled_does_not_apply_to_div() {
        // <div disabled> — disabled на не-form элементе игнорируется. Ни
        // :disabled, ни :enabled не матчат.
        let (root, doc) = lay_with_doc(
            r#"<div disabled>x</div>"#,
            ":disabled { color: red; } :enabled { color: blue; }",
        );
        let c = first_named(&doc, &root, "div");
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn id_wins_over_class() {
        // id specificity (1,0,0) > class (0,1,0). Порядок правил в CSS — class
        // после id — не должен пересилить.
        let root = lay(
            r#"<p id="x" class="c">v</p>"#,
            "#x { color: red; } .c { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255, "id should win over class");
        assert_eq!(p.style.color.b, 0);
    }

    #[test]
    fn class_wins_over_type() {
        // class (0,1,0) > type (0,0,1). Type идёт после в порядке — но проиграет.
        let root = lay(r#"<p class="c">v</p>"#, ".c { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    #[test]
    fn equal_specificity_last_wins() {
        let root = lay("<p>v</p>", "p { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
    }

    // ── Тесты inline-flow ───────────────────────────────────────────────────

    /// <span> внутри <p> не разрывает строку: высота = одна линия.
    #[test]
    fn inline_span_does_not_break_line() {
        let root = lay_measured("<p>hello <span>world</span></p>", "", 800.0);
        // "hello world" = 11 слов × 8px = 88px; при 800px — одна строка.
        assert!(
            (root.rect.height - 19.2).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// <a> получает цвет из CSS, текст соседнего текстового узла — родительский.
    #[test]
    fn inline_link_inherits_own_color() {
        let root = lay("<p>text <a>link</a></p>", "a { color: blue; }");
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { segments, .. } = &inline.kind {
            // Первый сегмент — текстовый узел "text " (наследует цвет <p>)
            assert_eq!(segments[0].style.color.b, 0, "text node must not be blue");
            // Второй сегмент — текст внутри <a> (синий)
            assert_eq!(segments[1].style.color.b, 255, "link must be blue");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// Inline-ран переносится так же, как обычный текст.
    #[test]
    fn inline_run_wraps_across_viewport() {
        // "aa bb" = 5 × 8 = 40px при Fixed8. Viewport 30px → перенос после "aa".
        let root = lay_measured("<p>aa <em>bb</em></p>", "", 30.0);
        // 2 строки × 19.2 = 38.4
        assert!(
            (root.rect.height - 38.4).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// Блочные элементы между inline-контентом не смешиваются в один InlineRun.
    #[test]
    fn block_between_inline_creates_separate_run() {
        // <div> — блочный элемент; текст до и после — разные InlineRun-ы.
        let root = lay("<p>before</p><div>mid</div><p>after</p>", "");
        // 3 блока по 19.2 = 57.6
        assert!(
            (root.rect.height - 57.6).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    // ── Функциональные pseudo: :nth-*, :*-of-type, :not ───────────────────

    /// Собирает все элементы с тегом `tag` из children корневого LayoutBox.
    fn block_children_by_tag<'a>(
        root: &'a LayoutBox,
        doc: &lumen_dom::Document,
        tag: &str,
    ) -> Vec<&'a LayoutBox> {
        root.children
            .iter()
            .filter(|c| {
                matches!(
                    &doc.get(c.node).data,
                    lumen_dom::NodeData::Element { name, .. } if name.local == tag
                )
            })
            .collect()
    }

    #[test]
    fn nth_child_odd_matches_1_3_5() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p><p>e</p>",
            "p:nth-child(odd) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps.len(), 5);
        for (i, p) in ps.iter().enumerate() {
            let one_based = (i + 1) as i32;
            let expected_red = one_based % 2 == 1;
            assert_eq!(
                p.style.color.r == 255,
                expected_red,
                "index={one_based}"
            );
        }
    }

    #[test]
    fn nth_child_specific_index() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-child(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn nth_child_formula_2n() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p>",
            "p:nth-child(2n) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // 2n: 2, 4, ...
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
        assert_eq!(ps[3].style.color.r, 255);
    }

    #[test]
    fn nth_last_child_matches_from_end() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-last-child(1) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // Последний матчит.
        assert_eq!(ps[2].style.color.r, 255);
        assert_eq!(ps[0].style.color.r, 0);
    }

    #[test]
    fn nth_of_type_counts_only_matching_tag() {
        // <h1><p1><h2><p2><p3> — :nth-of-type(2) для p должен попасть в p2.
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><h2>x</h2><p>p2</p><p>p3</p>",
            "p:nth-of-type(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // p1 — это of-type index 1 → 0, p2 → 2 → 255, p3 → 3 → 0.
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn first_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><p>p2</p>",
            "p:first-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn last_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<p>p1</p><p>p2</p><h1>x</h1>",
            "p:last-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        // p2 — последний `<p>` (h1 после него — другой тип), значит матчит.
        assert_eq!(ps[1].style.color.r, 255);
    }

    #[test]
    fn not_class_excludes() {
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p><p>c</p>"#,
            "p:not(.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "a should match");
        assert_eq!(ps[1].style.color.r, 0, "b.hl should NOT match");
        assert_eq!(ps[2].style.color.r, 255, "c should match");
    }

    #[test]
    fn not_with_compound_excludes_full() {
        // :not(p.hl) — исключает только p с классом hl, не любой <p> и не любой `.hl`.
        let (root, doc) = lay_with_doc(
            r#"<p>x</p><p class="hl">y</p><div class="hl">z</div>"#,
            "*:not(p.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        let divs = block_children_by_tag(&root, &doc, "div");
        assert_eq!(ps[0].style.color.r, 255, "p без класса — матчит");
        assert_eq!(ps[1].style.color.r, 0, "p.hl — исключается");
        assert_eq!(divs[0].style.color.r, 255, "div.hl — не исключается");
    }

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
        assert!((p.style.padding_top - 40.0).abs() < 0.01, "got {}", p.style.padding_top);
    }

    #[test]
    fn margin_rem_independent_of_inherit() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 99px; } p { margin: 1rem; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.margin_top - 16.0).abs() < 0.01);
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
    fn percent_in_margin_is_ignored() {
        // % в margin требует containing-block-width — пока не реализовано,
        // должно молча игнорироваться (margin остаётся 0).
        let root = lay("<p>x</p>", "p { margin: 50%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.margin_top, 0.0);
    }

    // ── Тесты text-align ───────────────────────────────────────────────────

    fn first_inline_run(b: &LayoutBox) -> &LayoutBox {
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
        assert_eq!(div.style.min_width, Some(100.0));
        assert_eq!(div.style.max_height, Some(50.0));
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
        let top_color = p.style.border_top_color.expect("border-color should be set");
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
        assert!(p.style.border_top_color.is_none(), "should be None = currentColor");
    }

    /// `border-top: 3px dashed blue` — только верхняя сторона.
    #[test]
    fn border_side_shorthand_sets_one_side() {
        let root = lay("<p>x</p>", "p { border-top: 3px dashed blue; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_width - 3.0).abs() < 0.01);
        assert_eq!(p.style.border_top_style, BorderStyle::Dashed);
        let c = p.style.border_top_color.expect("top color set");
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
        assert!((p.style.padding_top - 80.0).abs() < 0.01);
        assert!((p.style.padding_left - 80.0).abs() < 0.01);
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
            let id = doc.get(doc.root()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
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
            let id = doc.get(doc.root()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
            );
            assert_eq!(style.font_weight, FontWeight::BOLD, "tag = {tag}");
        }
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
    fn first_inline_text(root: &LayoutBox) -> String {
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
        assert!((p.style.text_indent - 30.0).abs() < 0.01);
    }

    #[test]
    fn text_indent_em_resolves_to_font_size() {
        // 2em при default fs 16 = 32px.
        let root = lay("<p>x</p>", "p { text-indent: 2em; }");
        let p = first_element_child(&root);
        assert!((p.style.text_indent - 32.0).abs() < 0.01);
    }

    #[test]
    fn text_indent_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-indent: 25px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((div.style.text_indent - 25.0).abs() < 0.01);
        assert!((p.style.text_indent - 25.0).abs() < 0.01);
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
        assert_eq!(p.style.text_indent, 0.0);
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
    fn font_family_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.font_family.is_empty());
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

    // ── outline (CSS UI L4 §3) ──────────────────────────────────────────────

    #[test]
    fn outline_shorthand() {
        let root = lay("<p>x</p>", "p { outline: 3px dashed red; }");
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 3.0).abs() < 0.01);
        assert_eq!(p.style.outline_style, BorderStyle::Dashed);
        assert_eq!(p.style.outline_color.unwrap().r, 255);
    }

    #[test]
    fn outline_individual_props() {
        let root = lay(
            "<p>x</p>",
            "p { outline-width: 5px; outline-style: solid; outline-color: blue; }",
        );
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 5.0).abs() < 0.01);
        assert_eq!(p.style.outline_style, BorderStyle::Solid);
        assert_eq!(p.style.outline_color.unwrap().b, 255);
    }

    #[test]
    fn outline_offset_positive_and_negative() {
        let p_root = lay("<p>x</p>", "p { outline-offset: 10px; }");
        let p = first_element_child(&p_root);
        assert!((p.style.outline_offset - 10.0).abs() < 0.01);

        let n_root = lay("<p>x</p>", "p { outline-offset: -3px; }");
        let n = first_element_child(&n_root);
        assert!((n.style.outline_offset - (-3.0)).abs() < 0.01);
    }

    #[test]
    fn outline_does_not_affect_box_width() {
        // Ключевое отличие от border: outline не занимает места в коробке.
        // Бокс с outline должен иметь ту же ширину/высоту, что без него.
        let with = lay("<p>x</p>", "p { outline: 10px solid red; }");
        let without = lay("<p>x</p>", "");

        let p_with = first_element_child(&with);
        let p_without = first_element_child(&without);
        assert!((p_with.rect.width - p_without.rect.width).abs() < 0.01,
            "outline не должен менять width: {} vs {}",
            p_with.rect.width, p_without.rect.width);
        assert!((p_with.rect.height - p_without.rect.height).abs() < 0.01);
    }

    #[test]
    fn outline_default_invisible() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.outline_width, 0.0);
        assert_eq!(p.style.outline_style, BorderStyle::None);
        assert!(p.style.outline_color.is_none());
        assert_eq!(p.style.outline_offset, 0.0);
    }

    #[test]
    fn outline_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { outline: 2px solid red; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(div.style.outline_width > 0.0);
        assert_eq!(p.style.outline_width, 0.0);
    }

    // ── visibility (CSS Display L3 §4) ──────────────────────────────────────

    #[test]
    fn visibility_default_visible() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Visible);
    }

    #[test]
    fn visibility_hidden_parsed() {
        let root = lay("<p>x</p>", "p { visibility: hidden; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Hidden);
    }

    #[test]
    fn visibility_collapse_parsed() {
        let root = lay("<p>x</p>", "p { visibility: collapse; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Collapse);
    }

    #[test]
    fn visibility_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { visibility: hidden; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.visibility, Visibility::Hidden);
        assert_eq!(p.style.visibility, Visibility::Hidden);
    }

    #[test]
    fn visibility_visible_overrides_inherited_hidden() {
        // Дочерний может явно вернуть себя — это ключевая семантика CSS.
        let root = lay(
            "<div><p>x</p></div>",
            "div { visibility: hidden; } p { visibility: visible; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.visibility, Visibility::Hidden);
        assert_eq!(p.style.visibility, Visibility::Visible);
    }

    #[test]
    fn visibility_hidden_keeps_layout_height() {
        // В отличие от display:none, visibility:hidden оставляет коробку
        // в layout — она занимает место.
        let visible = lay("<p>x</p>", "");
        let hidden = lay("<p>x</p>", "p { visibility: hidden; }");
        let none = lay("<p>x</p>", "p { display: none; }");

        // Высота с hidden = высота visible.
        assert!((visible.rect.height - hidden.rect.height).abs() < 0.01,
            "visibility:hidden должен оставить высоту: visible={} hidden={}",
            visible.rect.height, hidden.rect.height);
        // Высота с display:none = 0 (бокс пропадает).
        assert!(none.rect.height < 0.1,
            "display:none должен убрать высоту: {}", none.rect.height);
    }

    // ── overflow (CSS Overflow L3) ──────────────────────────────────────────

    #[test]
    fn overflow_default_visible() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Visible);
        assert_eq!(p.style.overflow_y, Overflow::Visible);
    }

    #[test]
    fn overflow_shorthand_one_value() {
        let root = lay("<p>x</p>", "p { overflow: hidden; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Hidden);
        assert_eq!(p.style.overflow_y, Overflow::Hidden);
    }

    #[test]
    fn overflow_shorthand_two_values() {
        let root = lay("<p>x</p>", "p { overflow: scroll auto; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Scroll);
        assert_eq!(p.style.overflow_y, Overflow::Auto);
    }

    #[test]
    fn overflow_individual_x_y() {
        let root = lay(
            "<p>x</p>",
            "p { overflow-x: clip; overflow-y: scroll; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Clip);
        assert_eq!(p.style.overflow_y, Overflow::Scroll);
    }

    #[test]
    fn overflow_all_keywords() {
        for (kw, expected) in [
            ("visible", Overflow::Visible),
            ("hidden", Overflow::Hidden),
            ("clip", Overflow::Clip),
            ("scroll", Overflow::Scroll),
            ("auto", Overflow::Auto),
        ] {
            let css = format!("p {{ overflow: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            let p = first_element_child(&root);
            assert_eq!(p.style.overflow_x, expected, "kw = {kw}");
        }
    }

    #[test]
    fn overflow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { overflow: hidden; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.overflow_x, Overflow::Hidden);
        assert_eq!(p.style.overflow_x, Overflow::Visible);
    }

    // ── cursor (CSS UI L4 §8.1) ─────────────────────────────────────────────

    #[test]
    fn cursor_default_auto() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Auto);
    }

    #[test]
    fn cursor_keywords_parsed() {
        for (kw, expected) in [
            ("default", Cursor::Default),
            ("pointer", Cursor::Pointer),
            ("text", Cursor::Text),
            ("wait", Cursor::Wait),
            ("move", Cursor::Move),
            ("not-allowed", Cursor::NotAllowed),
            ("grab", Cursor::Grab),
            ("zoom-in", Cursor::ZoomIn),
            ("nesw-resize", Cursor::NeswResize),
        ] {
            let css = format!("p {{ cursor: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            let p = first_element_child(&root);
            assert_eq!(p.style.cursor, expected, "kw = {kw}");
        }
    }

    #[test]
    fn cursor_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { cursor: pointer; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.cursor, Cursor::Pointer);
        assert_eq!(p.style.cursor, Cursor::Pointer);
    }

    #[test]
    fn cursor_url_fallback_uses_keyword() {
        // CSS UI: `cursor: url(...) default` — берём последний keyword.
        // Phase 0 url() игнорируется.
        let root = lay(
            "<p>x</p>",
            "p { cursor: url(custom.png), pointer; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Pointer);
    }

    #[test]
    fn cursor_unknown_keeps_inherited() {
        let root = lay("<p>x</p>", "p { cursor: nonsense; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Auto);
    }

    // ── box-shadow (CSS Backgrounds L3 §4.6) ────────────────────────────────

    #[test]
    fn box_shadow_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.box_shadow.is_empty());
    }

    #[test]
    fn box_shadow_two_lengths() {
        // offset-x, offset-y без blur/spread/color.
        let root = lay("<p>x</p>", "p { box-shadow: 5px 10px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 1);
        let s = &p.style.box_shadow[0];
        assert!((s.offset_x - 5.0).abs() < 0.01);
        assert!((s.offset_y - 10.0).abs() < 0.01);
        assert_eq!(s.blur, 0.0);
        assert_eq!(s.spread, 0.0);
        assert!(!s.inset);
        assert!(s.color.is_none());
    }

    #[test]
    fn box_shadow_with_blur_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 2px 3px 4px red; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.blur, 4.0);
        assert_eq!(s.color.unwrap().r, 255);
    }

    #[test]
    fn box_shadow_with_blur_spread_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 1px 2px 3px 4px blue; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.spread, 4.0);
        assert_eq!(s.color.unwrap().b, 255);
    }

    #[test]
    fn box_shadow_inset() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: inset 2px 2px 5px black; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert!(s.inset);
        assert!((s.offset_x - 2.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_multiple_comma_separated() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 1px 1px red, 2px 2px blue, inset 3px 3px black; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 3);
        assert_eq!(p.style.box_shadow[0].color.unwrap().r, 255);
        assert_eq!(p.style.box_shadow[1].color.unwrap().b, 255);
        assert!(p.style.box_shadow[2].inset);
    }

    #[test]
    fn box_shadow_color_with_internal_commas() {
        // rgba(...) содержит запятые внутри — split_top_level_commas
        // не должен порвать это на куски.
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 2px 2px 4px rgba(0, 0, 0, 0.5); }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 1);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.color.unwrap().a, 128);
    }

    #[test]
    fn box_shadow_none_clears() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-shadow: 1px 1px black; } p { box-shadow: none; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // box-shadow не наследуется в любом случае; но `none` должно
        // явно сбросить.
        assert_eq!(div.style.box_shadow.len(), 1);
        assert!(p.style.box_shadow.is_empty());
    }

    #[test]
    fn box_shadow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-shadow: 2px 2px black; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.box_shadow.len(), 1);
        assert!(p.style.box_shadow.is_empty());
    }

    // ── text-shadow (CSS Text Decoration L3 §4) ─────────────────────────────

    #[test]
    fn text_shadow_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.text_shadow.is_empty());
    }

    #[test]
    fn text_shadow_two_lengths() {
        let root = lay("<p>x</p>", "p { text-shadow: 2px 3px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 1);
        let s = &p.style.text_shadow[0];
        assert!((s.offset_x - 2.0).abs() < 0.01);
        assert!((s.offset_y - 3.0).abs() < 0.01);
        assert_eq!(s.blur, 0.0);
        assert!(s.color.is_none());
    }

    #[test]
    fn text_shadow_with_blur_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 1px 2px 3px red; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.text_shadow[0];
        assert_eq!(s.blur, 3.0);
        assert_eq!(s.color.unwrap().r, 255);
    }

    #[test]
    fn text_shadow_multiple() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 1px 1px red, 2px 2px blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 2);
        assert_eq!(p.style.text_shadow[0].color.unwrap().r, 255);
        assert_eq!(p.style.text_shadow[1].color.unwrap().b, 255);
    }

    #[test]
    fn text_shadow_inherited() {
        // В отличие от box-shadow, text-shadow ДОЛЖЕН наследоваться.
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-shadow: 1px 1px black; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_shadow.len(), 1);
        assert_eq!(p.style.text_shadow.len(), 1, "text-shadow должен наследоваться");
    }

    #[test]
    fn text_shadow_none_overrides_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-shadow: 1px 1px black; } p { text-shadow: none; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_shadow.len(), 1);
        assert!(p.style.text_shadow.is_empty(), "p должен сбросить inherited");
    }

    #[test]
    fn text_shadow_color_with_internal_commas() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 2px 2px 4px rgba(0, 0, 0, 0.5); }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 1);
        assert_eq!(p.style.text_shadow[0].color.unwrap().a, 128);
    }

    // ── border-radius (CSS Backgrounds L3 §5) ───────────────────────────────

    #[test]
    fn border_radius_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, 0.0);
        assert_eq!(p.style.border_top_right_radius, 0.0);
        assert_eq!(p.style.border_bottom_right_radius, 0.0);
        assert_eq!(p.style.border_bottom_left_radius, 0.0);
    }

    #[test]
    fn border_radius_shorthand_one_value() {
        let root = lay("<p>x</p>", "p { border-radius: 8px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, 8.0);
        assert_eq!(p.style.border_top_right_radius, 8.0);
        assert_eq!(p.style.border_bottom_right_radius, 8.0);
        assert_eq!(p.style.border_bottom_left_radius, 8.0);
    }

    #[test]
    fn border_radius_shorthand_two_values() {
        // 2 значения: TL/BR одинаковы, TR/BL одинаковы.
        let root = lay("<p>x</p>", "p { border-radius: 4px 12px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, 4.0);
        assert_eq!(p.style.border_top_right_radius, 12.0);
        assert_eq!(p.style.border_bottom_right_radius, 4.0);
        assert_eq!(p.style.border_bottom_left_radius, 12.0);
    }

    #[test]
    fn border_radius_shorthand_four_values() {
        let root = lay(
            "<p>x</p>",
            "p { border-radius: 1px 2px 3px 4px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, 1.0);
        assert_eq!(p.style.border_top_right_radius, 2.0);
        assert_eq!(p.style.border_bottom_right_radius, 3.0);
        assert_eq!(p.style.border_bottom_left_radius, 4.0);
    }

    #[test]
    fn border_radius_individual_corners() {
        let root = lay(
            "<p>x</p>",
            "p { border-top-left-radius: 5px; border-bottom-right-radius: 10px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, 5.0);
        assert_eq!(p.style.border_top_right_radius, 0.0);
        assert_eq!(p.style.border_bottom_right_radius, 10.0);
        assert_eq!(p.style.border_bottom_left_radius, 0.0);
    }

    #[test]
    fn border_radius_em_resolves() {
        // 1em при default fs 16 = 16px.
        let root = lay("<p>x</p>", "p { border-radius: 1em; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_left_radius - 16.0).abs() < 0.01);
    }

    #[test]
    fn border_radius_elliptical_takes_first_part() {
        // `5px / 10px` (elliptical) — Phase 0 берёт только горизонтальный
        // (первый токен до `/`).
        let root = lay(
            "<p>x</p>",
            "p { border-radius: 5px / 10px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, 5.0);
    }

    #[test]
    fn border_radius_negative_clamped_to_zero() {
        let root = lay("<p>x</p>", "p { border-radius: -10px; }");
        let p = first_element_child(&root);
        // Невалидное (отрицательное) — clamp до 0 после resolve.
        // Наш resolve_box_length вернёт -10, мы делаем max(0.0).
        assert_eq!(p.style.border_top_left_radius, 0.0);
    }

    #[test]
    fn border_radius_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { border-radius: 5px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.border_top_left_radius, 5.0);
        assert_eq!(p.style.border_top_left_radius, 0.0);
    }

    // ── text-overflow (CSS UI L4 §10.1) ─────────────────────────────────────

    #[test]
    fn text_overflow_default_clip() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_ellipsis_parsed() {
        let root = lay("<p>x</p>", "p { text-overflow: ellipsis; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Ellipsis);
    }

    #[test]
    fn text_overflow_clip_explicit() {
        let root = lay("<p>x</p>", "p { text-overflow: clip; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-overflow: ellipsis; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_unknown_keeps_default() {
        let root = lay("<p>x</p>", "p { text-overflow: nonsense; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    // ── selector matching: back-tracking edge cases ─────────────────────────

    /// `div div p` — двойной descendant. Должен матчить, когда есть два
    /// уровня div выше p. Без back-tracking тоже работает (greedy от p вверх
    /// находит ближайший div, дальше выше — другой div) — sanity check.
    #[test]
    fn selector_double_descendant_works() {
        let root = lay(
            "<div><div><p>x</p></div></div>",
            "div div p { color: red; }",
        );
        // Находим p глубоко.
        fn find_p<'a>(b: &'a LayoutBox, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
            if let lumen_dom::NodeData::Element { name, .. } = &doc.get(b.node).data
                && name.local == "p"
            {
                return Some(b);
            }
            for c in &b.children {
                if let Some(f) = find_p(c, doc) {
                    return Some(f);
                }
            }
            None
        }
        let doc = lumen_html_parser::parse("<div><div><p>x</p></div></div>");
        let p = find_p(&root, &doc).unwrap();
        assert_eq!(p.style.color.r, 255);
    }

    /// `a a span` с двумя `<a>`-предками — должен матчить через compute_style
    /// (LayoutBox-фасад не подходит, т.к. <a> inline и весь контент сплавлен
    /// в InlineRun-ы; проверяем напрямую).
    #[test]
    fn selector_nested_same_tag_descendants() {
        let doc = lumen_html_parser::parse(r#"<a><a><span>x</span></a></a>"#);
        let span_id = find_first_by_tag(&doc, doc.root(), "span").expect("span");
        let style = crate::style::compute_style(
            &doc,
            span_id,
            &lumen_css_parser::parse("a a span { color: red; }"),
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
        );
        assert_eq!(style.color.r, 255);
    }

    /// Чисто back-tracking-зависимый случай через compute_style. Дерево:
    /// `<div><a class="x"></a><a></a><a></a><span>X</span></div>`. Селектор:
    /// `.x + a ~ span`. Greedy от span: `~ span` находит span; `+ a` — это
    /// его прямой предыдущий sibling = третий `<a>`. Затем `.x` — sibling до
    /// него = второй `<a>`, который не имеет класс `.x` → fail. Backtracking
    /// перебирает `~ span` кандидатов: span сам = node → нет; либо для
    /// later-sibling combinator берёт КАЖДЫЙ earlier sibling. С back-tracking
    /// найдётся: `~ span` candidate = span (нет), но потом для `+ a` мы
    /// фиксируемся на втором `<a>` (через рекурсию), и первый `<a>` (`.x`)
    /// удовлетворяет `.x`.
    #[test]
    fn selector_backtracking_pathological_sibling() {
        let doc = lumen_html_parser::parse(
            r#"<div><a class="x">A</a><a>B</a><a>C</a><span>SPAN</span></div>"#,
        );
        let span_id = find_first_by_tag(&doc, doc.root(), "span").expect("span");
        let sheet = lumen_css_parser::parse(".x + a ~ span { color: red; }");
        let style = crate::style::compute_style(
            &doc,
            span_id,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
        );
        assert_eq!(
            style.color.r, 255,
            ".x + a ~ span должен сматчить span с back-tracking"
        );
    }

    fn find_first_by_tag(
        doc: &lumen_dom::Document,
        id: lumen_dom::NodeId,
        tag: &str,
    ) -> Option<lumen_dom::NodeId> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(id).data
            && name.local == tag
        {
            return Some(id);
        }
        for c in &doc.get(id).children {
            if let Some(f) = find_first_by_tag(doc, *c, tag) {
                return Some(f);
            }
        }
        None
    }

    // ── font-variant (CSS Fonts L4 §6, упрощённый) ──────────────────────────

    #[test]
    fn font_variant_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant, FontVariant::Normal);
    }

    #[test]
    fn font_variant_small_caps_parsed() {
        let root = lay("<p>x</p>", "p { font-variant: small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant, FontVariant::SmallCaps);
    }

    #[test]
    fn font_variant_caps_alias() {
        // CSS Fonts L4 §6.4: font-variant-caps — отдельное property,
        // парсится тем же кодом для small-caps значения.
        let root = lay("<p>x</p>", "p { font-variant-caps: small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant, FontVariant::SmallCaps);
    }

    #[test]
    fn font_variant_normal_keyword_resets() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; } p { font-variant: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_variant, FontVariant::SmallCaps);
        assert_eq!(p.style.font_variant, FontVariant::Normal);
    }

    #[test]
    fn font_variant_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_variant, FontVariant::SmallCaps);
    }

    // ── font-stretch (CSS Fonts L4 §2.5) ────────────────────────────────────

    #[test]
    fn font_stretch_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch, FontStretch::NORMAL);
    }

    #[test]
    fn font_stretch_keyword_condensed() {
        let root = lay("<p>x</p>", "p { font-stretch: condensed; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 750);
    }

    #[test]
    fn font_stretch_keyword_semi_expanded_fractional() {
        // 112.5% — дробный keyword проверяет, что хранение в десятых не теряет точность.
        let root = lay("<p>x</p>", "p { font-stretch: semi-expanded; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 1125);
    }

    #[test]
    fn font_stretch_percentage_value() {
        let root = lay("<p>x</p>", "p { font-stretch: 80%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 800);
    }

    #[test]
    fn font_stretch_percentage_clamped() {
        // Spec разрешает значения вне [50%, 200%], но Phase 0 их клампит —
        // экстремальные значения бесполезны и могут переполнить u16.
        let root = lay("<p>x</p>", "p { font-stretch: 10%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 500);

        let root = lay("<p>x</p>", "p { font-stretch: 300%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 2000);
    }

    #[test]
    fn font_stretch_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-stretch: expanded; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_stretch.0, 1250);
        assert_eq!(div.style.font_stretch.0, 1250);
    }

    #[test]
    fn font_stretch_normal_resets_inheritance() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-stretch: condensed; } p { font-stretch: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_stretch.0, 750);
        assert_eq!(p.style.font_stretch, FontStretch::NORMAL);
    }

    // ── accent-color (CSS UI L4 §6.1) ──────────────────────────────────────

    #[test]
    fn accent_color_default_none() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.accent_color.is_none());
    }

    #[test]
    fn accent_color_named() {
        let root = lay("<p>x</p>", "p { accent-color: red; }");
        let p = first_element_child(&root);
        let c = p.style.accent_color.expect("accent set");
        assert_eq!((c.r, c.g, c.b, c.a), (255, 0, 0, 255));
    }

    #[test]
    fn accent_color_hex() {
        let root = lay("<p>x</p>", "p { accent-color: #4080ff; }");
        let p = first_element_child(&root);
        let c = p.style.accent_color.expect("accent set");
        assert_eq!((c.r, c.g, c.b), (0x40, 0x80, 0xff));
    }

    #[test]
    fn accent_color_auto_resets_inheritance() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: blue; } p { accent-color: auto; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(div.style.accent_color.is_some());
        assert!(p.style.accent_color.is_none());
    }

    #[test]
    fn accent_color_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: rgb(10, 20, 30); }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        let dc = div.style.accent_color.expect("div accent");
        let pc = p.style.accent_color.expect("p inherits accent");
        assert_eq!((dc.r, dc.g, dc.b), (10, 20, 30));
        assert_eq!((pc.r, pc.g, pc.b), (10, 20, 30));
    }

    #[test]
    fn accent_color_invalid_ignored() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: red; } p { accent-color: notacolor; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // Невалидное значение игнорируется → p наследует от div.
        assert_eq!(div.style.accent_color, p.style.accent_color);
        assert!(p.style.accent_color.is_some());
    }

    // ── :has() (CSS Selectors L4 §17.2) ─────────────────────────────────────

    /// `div:has(p)` — div, содержащий p в поддереве (через span).
    #[test]
    fn has_implicit_descendant_matches() {
        let root = lay(
            "<div><span><p>x</p></span></div><div><span>nope</span></div>",
            "div:has(p) { color: red; }",
        );
        let blocks: Vec<_> = root.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks[0].style.color.r, 255, "первый div должен сматчить");
        assert_eq!(blocks[1].style.color.r, 0, "второй div без p — нет");
    }

    /// `div:has(> .child)` — direct child only.
    #[test]
    fn has_child_combinator() {
        let root = lay(
            r#"<div><p class="child">x</p></div><div><span><p class="child">x</p></span></div>"#,
            "div:has(> .child) { color: red; }",
        );
        let blocks: Vec<_> = root.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks[0].style.color.r, 255);
        assert_eq!(blocks[1].style.color.r, 0);
    }

    /// `h2:has(+ p)` — h2 followed by p. Через compute_style напрямую.
    #[test]
    fn has_next_sibling() {
        let doc = lumen_html_parser::parse("<div><h2>A</h2><p>x</p></div><div><h2>B</h2></div>");
        let sheet = lumen_css_parser::parse("h2:has(+ p) { color: red; }");
        let root_style = ComputedStyle::root();
        let div1 = doc.get(doc.root()).children[0];
        let h2_a = doc.get(div1).children[0];
        let div2 = doc.get(doc.root()).children[1];
        let h2_b = doc.get(div2).children[0];
        let style_a = crate::style::compute_style(
            &doc, h2_a, &sheet, &root_style, Size::new(800.0, 600.0));
        let style_b = crate::style::compute_style(
            &doc, h2_b, &sheet, &root_style, Size::new(800.0, 600.0));
        assert_eq!(style_a.color.r, 255, "h2 + p должен сматчить");
        assert_eq!(style_b.color.r, 0, "h2 без p после — нет");
    }

    /// `:has()` НЕ матчит сам node — descendants only.
    #[test]
    fn has_does_not_match_self() {
        let root = lay(
            "<p>x</p>",
            "p:has(p) { color: red; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 0);
    }

    /// `:has(.a, .b)` — список (OR).
    #[test]
    fn has_list_or_match() {
        let root = lay(
            r#"<div><span class="b">x</span></div>"#,
            ":has(.a, .b) { color: red; }",
        );
        let div = first_element_child(&root);
        assert_eq!(div.style.color.r, 255);
    }

    // ── direction (CSS Writing Modes L3 §2.1) ──────────────────────────────

    #[test]
    fn direction_default_ltr() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Ltr);
    }

    #[test]
    fn direction_rtl_applied() {
        let root = lay("<p>x</p>", "p { direction: rtl; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_case_insensitive() {
        // Keyword-ы CSS property values — ASCII case-insensitive
        // (Values L4 §2.4). Документ может прийти с `RTL` или `Rtl`.
        let root = lay("<p>x</p>", "p { direction: RTL; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_inherited() {
        // direction распространяется от родителя — основа bidi-каскада.
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.direction, Direction::Rtl);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_child_overrides_inherited() {
        // Inheritable, но потомок может явно переопределить — обратно на ltr.
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; } p { direction: ltr; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.direction, Direction::Rtl);
        assert_eq!(p.style.direction, Direction::Ltr);
    }

    #[test]
    fn direction_invalid_keeps_inherited() {
        // Невалидное значение — сохраняем inherited (по CSS error recovery
        // правилу: invalid declaration → ignore).
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; } p { direction: vertical; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    // ── <img> replaced element ───────────────────────────────────────────

    fn first_image_child(b: &LayoutBox) -> &LayoutBox {
        b.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Image { .. }))
            .expect("expected at least one image child")
    }

    #[test]
    fn img_creates_image_box_with_src_and_alt() {
        let root = lay(r#"<img src="logo.png" alt="logo">"#, "");
        let img = first_image_child(&root);
        match &img.kind {
            BoxKind::Image { src, alt } => {
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
        if let BoxKind::Image { src, alt } = &img.kind {
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
    fn img_not_treated_as_inline_content() {
        // <img> в Phase 0 — block-level. Текст до и после не объединяется с
        // ним в один InlineRun.
        let root = lay(r#"<div>before<img src="x" width="10" height="10">after</div>"#, "");
        let div = first_element_child(&root);
        // div должен иметь 3 потомка: InlineRun("before") + Image + InlineRun("after").
        assert_eq!(div.children.len(), 3, "got {}", div.children.len());
        assert!(matches!(div.children[0].kind, BoxKind::InlineRun { .. }));
        assert!(matches!(div.children[1].kind, BoxKind::Image { .. }));
        assert!(matches!(div.children[2].kind, BoxKind::InlineRun { .. }));
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
    fn nested_p_style(root: &LayoutBox) -> &ComputedStyle {
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
            Some(Color { r: 0, g: 100, b: 200, a: 255 })
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
    fn css_revert_treated_like_unset_in_phase0() {
        // Phase 0: revert == unset. Тест дублирует css_unset_*.
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

    fn lay_with_viewport(html: &str, css: &str, vw: f32, vh: f32) -> LayoutBox {
        use lumen_dom::Document;
        use lumen_core::Size;
        let document: Document = lumen_html_parser::parse(html);
        let stylesheet = lumen_css_parser::parse(css);
        let viewport = Size { width: vw, height: vh };
        crate::layout(&document, &stylesheet, viewport)
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
    fn display_flex_parses_and_stores() {
        let root = lay("<p>x</p>", "p { display: flex; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.display, Display::Flex);
    }

    #[test]
    fn display_inline_flex_parses_and_stores() {
        // inline-flex element внутри div — должен попасть в InlineRun
        // (трактуется как inline-family).
        let root = lay("<div><span>x</span></div>", "span { display: inline-flex; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // div содержит InlineRun (inline-flex span внутри).
        assert!(matches!(&div.children[0].kind, BoxKind::InlineRun { .. }));
    }

    #[test]
    fn display_grid_parses_as_block_family() {
        let root = lay("<p>x</p>", "p { display: grid; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.display, Display::Grid);
    }

    #[test]
    fn display_inline_grid_parses_as_inline_family() {
        let root = lay("<div><span>x</span></div>", "span { display: inline-grid; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(matches!(&div.children[0].kind, BoxKind::InlineRun { .. }));
    }

    #[test]
    fn display_unknown_value_keeps_previous() {
        // unknown value игнорируется — лог по умолчанию остаётся.
        let root = lay("<p>x</p>", "p { display: zomg-flexed; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Default для <p> от UA = Block.
        assert_eq!(p.style.display, Display::Block);
    }

    // ──────── clip-path / transform / filter ────────

    fn first_p_style(root: &LayoutBox) -> &ComputedStyle {
        let p = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        &p.style
    }

    #[test]
    fn clip_path_inset_parses() {
        let root = lay("<p>x</p>", "p { clip-path: inset(10px 20px 30px 40px); }");
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Inset(parts)) => {
                assert_eq!(parts, vec![10.0, 20.0, 30.0, 40.0]);
            }
            _ => panic!("expected Inset, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_circle_with_center() {
        let root = lay("<p>x</p>", "p { clip-path: circle(50px at 100px 200px); }");
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Circle { radius, center }) => {
                assert!((radius - 50.0).abs() < 0.01);
                assert_eq!(center, Some((100.0, 200.0)));
            }
            _ => panic!("expected Circle, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_ellipse() {
        let root = lay("<p>x</p>", "p { clip-path: ellipse(30px 60px); }");
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Ellipse { rx, ry, center: None }) => {
                assert!((rx - 30.0).abs() < 0.01);
                assert!((ry - 60.0).abs() < 0.01);
            }
            _ => panic!("expected Ellipse, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_polygon() {
        let root = lay(
            "<p>x</p>",
            "p { clip-path: polygon(0 0, 100px 0, 50px 100px); }",
        );
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Polygon(verts)) => {
                assert_eq!(verts.len(), 3);
                assert_eq!(verts[0], (0.0, 0.0));
                assert_eq!(verts[1], (100.0, 0.0));
                assert_eq!(verts[2], (50.0, 100.0));
            }
            _ => panic!("expected Polygon, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_none_clears() {
        let root = lay("<p>x</p>", "p { clip-path: circle(50px); clip-path: none; }");
        assert_eq!(first_p_style(&root).clip_path, None);
    }

    #[test]
    fn transform_translate() {
        let root = lay("<p>x</p>", "p { transform: translate(10px, 20px); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t, vec![TransformFn::Translate(10.0, 20.0)]);
    }

    #[test]
    fn transform_rotate_normalizes_to_radians() {
        let root = lay("<p>x</p>", "p { transform: rotate(90deg); }");
        let t = first_p_style(&root).transform.clone();
        match &t[..] {
            [TransformFn::Rotate(rad)] => {
                assert!((rad - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
            }
            _ => panic!("expected single Rotate, got {t:?}"),
        }
    }

    #[test]
    fn transform_scale_single_arg_uniform() {
        let root = lay("<p>x</p>", "p { transform: scale(1.5); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t, vec![TransformFn::Scale(1.5, 1.5)]);
    }

    #[test]
    fn transform_scale_two_args() {
        let root = lay("<p>x</p>", "p { transform: scale(2, 0.5); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t, vec![TransformFn::Scale(2.0, 0.5)]);
    }

    #[test]
    fn transform_matrix() {
        let root = lay("<p>x</p>", "p { transform: matrix(1, 0, 0, 1, 50, 100); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(
            t,
            vec![TransformFn::Matrix([1.0, 0.0, 0.0, 1.0, 50.0, 100.0])]
        );
    }

    #[test]
    fn transform_list_multiple() {
        let root = lay(
            "<p>x</p>",
            "p { transform: translate(10px, 0) rotate(45deg) scale(2); }",
        );
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t.len(), 3);
        assert!(matches!(t[0], TransformFn::Translate(_, _)));
        assert!(matches!(t[1], TransformFn::Rotate(_)));
        assert!(matches!(t[2], TransformFn::Scale(_, _)));
    }

    #[test]
    fn transform_none_clears() {
        let root = lay(
            "<p>x</p>",
            "p { transform: rotate(45deg); transform: none; }",
        );
        assert!(first_p_style(&root).transform.is_empty());
    }

    #[test]
    fn filter_blur() {
        let root = lay("<p>x</p>", "p { filter: blur(5px); }");
        let f = first_p_style(&root).filter.clone();
        assert_eq!(f, vec![FilterFn::Blur(5.0)]);
    }

    #[test]
    fn filter_percentage_normalized() {
        let root = lay("<p>x</p>", "p { filter: grayscale(50%); }");
        let f = first_p_style(&root).filter.clone();
        match &f[..] {
            [FilterFn::Grayscale(v)] => assert!((v - 0.5).abs() < 1e-5),
            _ => panic!("expected Grayscale, got {f:?}"),
        }
    }

    #[test]
    fn filter_chain() {
        let root = lay(
            "<p>x</p>",
            "p { filter: blur(2px) brightness(1.2) saturate(0.8); }",
        );
        let f = first_p_style(&root).filter.clone();
        assert_eq!(f.len(), 3);
        assert!(matches!(f[0], FilterFn::Blur(_)));
        assert!(matches!(f[1], FilterFn::Brightness(_)));
        assert!(matches!(f[2], FilterFn::Saturate(_)));
    }

    #[test]
    fn filter_hue_rotate_radians() {
        let root = lay("<p>x</p>", "p { filter: hue-rotate(180deg); }");
        let f = first_p_style(&root).filter.clone();
        match &f[..] {
            [FilterFn::HueRotate(rad)] => {
                assert!((rad - std::f32::consts::PI).abs() < 1e-5);
            }
            _ => panic!("expected HueRotate, got {f:?}"),
        }
    }

    #[test]
    fn filter_none_clears() {
        let root = lay("<p>x</p>", "p { filter: blur(5px); filter: none; }");
        assert!(first_p_style(&root).filter.is_empty());
    }

    #[test]
    fn filter_unknown_skipped() {
        let root = lay("<p>x</p>", "p { filter: blur(5px) zomg(1); brightness(1); }");
        // zomg() игнорируется, остальное парсится.
        let f = first_p_style(&root).filter.clone();
        // brightness вне filter declaration — отдельный selector? Нет,
        // оно в той же декларации `filter: blur(5px) zomg(1)` — zomg
        // skipped, blur остался.
        assert!(matches!(f[0], FilterFn::Blur(_)));
    }

    #[test]
    fn clip_transform_filter_not_inherited() {
        // Эти свойства не наследуются.
        let root = lay(
            "<div><p>x</p></div>",
            "div { clip-path: circle(50px); transform: rotate(45deg); filter: blur(5px); }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(p.style.clip_path.is_none());
        assert!(p.style.transform.is_empty());
        assert!(p.style.filter.is_empty());
        assert!(div.style.clip_path.is_some());
        assert!(!div.style.transform.is_empty());
        assert!(!div.style.filter.is_empty());
    }

    // ──────── gap / aspect-ratio ────────

    #[test]
    fn gap_shorthand_single_value() {
        let root = lay("<p>x</p>", "p { gap: 10px; }");
        let s = first_p_style(&root);
        assert!((s.row_gap - 10.0).abs() < 0.01);
        assert!((s.column_gap - 10.0).abs() < 0.01);
    }

    #[test]
    fn gap_shorthand_two_values() {
        let root = lay("<p>x</p>", "p { gap: 10px 20px; }");
        let s = first_p_style(&root);
        assert!((s.row_gap - 10.0).abs() < 0.01);
        assert!((s.column_gap - 20.0).abs() < 0.01);
    }

    #[test]
    fn row_gap_individual() {
        let root = lay("<p>x</p>", "p { row-gap: 15px; }");
        assert!((first_p_style(&root).row_gap - 15.0).abs() < 0.01);
    }

    #[test]
    fn column_gap_individual() {
        let root = lay("<p>x</p>", "p { column-gap: 25px; }");
        assert!((first_p_style(&root).column_gap - 25.0).abs() < 0.01);
    }

    #[test]
    fn gap_em_resolved() {
        // em разрешается относительно font-size элемента.
        let root = lay("<p>x</p>", "p { font-size: 20px; gap: 1.5em; }");
        let s = first_p_style(&root);
        assert!((s.row_gap - 30.0).abs() < 0.01);
    }

    #[test]
    fn gap_negative_clamped_to_zero() {
        // gap не может быть отрицательным.
        let root = lay("<p>x</p>", "p { gap: -5px; }");
        assert_eq!(first_p_style(&root).row_gap, 0.0);
    }

    #[test]
    fn aspect_ratio_single_number() {
        let root = lay("<p>x</p>", "p { aspect-ratio: 1.5; }");
        assert_eq!(first_p_style(&root).aspect_ratio, Some((1.5, 1.0)));
    }

    #[test]
    fn aspect_ratio_w_h_pair() {
        let root = lay("<p>x</p>", "p { aspect-ratio: 16 / 9; }");
        assert_eq!(first_p_style(&root).aspect_ratio, Some((16.0, 9.0)));
    }

    #[test]
    fn aspect_ratio_auto() {
        let root = lay("<p>x</p>", "p { aspect-ratio: auto; }");
        assert_eq!(first_p_style(&root).aspect_ratio, None);
    }

    #[test]
    fn aspect_ratio_negative_rejected() {
        let root = lay("<p>x</p>", "p { aspect-ratio: -1 / 2; }");
        assert_eq!(first_p_style(&root).aspect_ratio, None);
    }

    #[test]
    fn aspect_ratio_invalid_kept_unchanged() {
        let root = lay("<p>x</p>", "p { aspect-ratio: 16 / abc; }");
        assert_eq!(first_p_style(&root).aspect_ratio, None);
    }

    // ──────── CSS Multi-column L1 ────────

    #[test]
    fn column_count_integer() {
        let root = lay("<p>x</p>", "p { column-count: 3; }");
        assert_eq!(first_p_style(&root).column_count, Some(3));
    }

    #[test]
    fn column_count_auto() {
        let root = lay("<p>x</p>", "p { column-count: auto; }");
        assert_eq!(first_p_style(&root).column_count, None);
    }

    #[test]
    fn column_count_zero_rejected() {
        let root = lay("<p>x</p>", "p { column-count: 0; }");
        assert_eq!(first_p_style(&root).column_count, None);
    }

    #[test]
    fn column_width_length() {
        let root = lay("<p>x</p>", "p { column-width: 200px; }");
        assert_eq!(first_p_style(&root).column_width, Some(200.0));
    }

    #[test]
    fn column_width_auto() {
        let root = lay("<p>x</p>", "p { column-width: auto; }");
        assert_eq!(first_p_style(&root).column_width, None);
    }

    #[test]
    fn columns_shorthand_both() {
        let root = lay("<p>x</p>", "p { columns: 200px 3; }");
        let s = first_p_style(&root);
        assert_eq!(s.column_width, Some(200.0));
        assert_eq!(s.column_count, Some(3));
    }

    #[test]
    fn columns_shorthand_width_only() {
        let root = lay("<p>x</p>", "p { columns: 250px; }");
        let s = first_p_style(&root);
        assert_eq!(s.column_width, Some(250.0));
        assert_eq!(s.column_count, None);
    }

    #[test]
    fn columns_shorthand_count_only() {
        let root = lay("<p>x</p>", "p { columns: 4; }");
        let s = first_p_style(&root);
        assert_eq!(s.column_count, Some(4));
        assert_eq!(s.column_width, None);
    }

    #[test]
    fn column_rule_individual() {
        let root = lay(
            "<p>x</p>",
            "p { column-rule-width: 2px; column-rule-style: solid; }",
        );
        let s = first_p_style(&root);
        assert!((s.column_rule_width - 2.0).abs() < 1e-6);
        assert_eq!(s.column_rule_style, BorderStyle::Solid);
    }

    #[test]
    fn column_rule_shorthand() {
        let root = lay("<p>x</p>", "p { column-rule: 3px dashed; }");
        let s = first_p_style(&root);
        assert!((s.column_rule_width - 3.0).abs() < 1e-6);
        assert_eq!(s.column_rule_style, BorderStyle::Dashed);
    }

    #[test]
    fn column_span_all() {
        let root = lay("<p>x</p>", "p { column-span: all; }");
        assert!(first_p_style(&root).column_span_all);
    }

    #[test]
    fn column_fill_balance() {
        let root = lay("<p>x</p>", "p { column-fill: balance; }");
        assert!(first_p_style(&root).column_fill_balance);
    }

    #[test]
    fn break_before_avoid() {
        let root = lay("<p>x</p>", "p { break-before: avoid; }");
        assert_eq!(first_p_style(&root).break_before, BreakValue::Avoid);
    }

    #[test]
    fn break_after_page() {
        let root = lay("<p>x</p>", "p { break-after: page; }");
        assert_eq!(first_p_style(&root).break_after, BreakValue::Page);
    }

    #[test]
    fn break_inside_avoid_column() {
        let root = lay("<p>x</p>", "p { break-inside: avoid-column; }");
        assert_eq!(first_p_style(&root).break_inside, BreakValue::Avoid);
    }

    #[test]
    fn column_count_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { column-count: 3; }",
        );
        // Дочерний p не должен унаследовать column-count (CSS Multi-column L1 §3.2 — не наследуется).
        let p_style = nested_p_style(&root);
        assert_eq!(p_style.column_count, None);
    }

    // ──────── CSS Environment Variables L1 — env() ────────

    #[test]
    fn env_fallback_used_when_unknown() {
        // env() с unknown name + fallback → fallback применяется.
        let root = lay(
            "<p>x</p>",
            "p { padding: env(safe-area-inset-top, 12px); }",
        );
        assert!((first_p_style(&root).padding_top - 12.0).abs() < 1e-6);
    }

    #[test]
    fn env_without_fallback_invalidates_decl() {
        // env() с unknown name и без fallback — декларация невалидна.
        let root = lay(
            "<p>x</p>",
            "p { padding: env(safe-area-inset-top); }",
        );
        assert!((first_p_style(&root).padding_top - 0.0).abs() < 1e-6);
    }

    #[test]
    fn env_with_indices_ignored_phase0() {
        // `env(name 0, fallback)` — индекс игнорируется, имя = name.
        let root = lay(
            "<p>x</p>",
            "p { padding: env(viewport-segment-width 0 0, 25px); }",
        );
        assert!((first_p_style(&root).padding_top - 25.0).abs() < 1e-6);
    }

    #[test]
    fn env_inside_calc() {
        // calc(env(...) + 5px) — env разворачивается до calc().
        let root = lay(
            "<p>x</p>",
            "p { padding: calc(env(safe-area-inset-top, 10px) + 5px); }",
        );
        assert!((first_p_style(&root).padding_top - 15.0).abs() < 1e-6);
    }

    #[test]
    fn env_inside_var_fallback() {
        // var(--foo, env(name, 8px)) — env как fallback внутри var().
        let root = lay(
            "<p>x</p>",
            "p { padding: var(--missing, env(safe-area-inset-top, 8px)); }",
        );
        assert!((first_p_style(&root).padding_top - 8.0).abs() < 1e-6);
    }

    // ──────── CSS Scroll Snap L1 ────────

    #[test]
    fn scroll_snap_type_none() {
        let root = lay("<p>x</p>", "p { scroll-snap-type: none; }");
        assert_eq!(first_p_style(&root).scroll_snap_type.axis, ScrollSnapAxis::None);
    }

    #[test]
    fn scroll_snap_type_x_mandatory() {
        let root = lay("<p>x</p>", "p { scroll-snap-type: x mandatory; }");
        let s = first_p_style(&root);
        assert_eq!(s.scroll_snap_type.axis, ScrollSnapAxis::X);
        assert_eq!(s.scroll_snap_type.strictness, ScrollSnapStrictness::Mandatory);
    }

    #[test]
    fn scroll_snap_align_single_keyword() {
        let root = lay("<p>x</p>", "p { scroll-snap-align: center; }");
        let s = first_p_style(&root);
        assert_eq!(s.scroll_snap_align.block, ScrollSnapAlignKeyword::Center);
        assert_eq!(s.scroll_snap_align.inline, ScrollSnapAlignKeyword::Center);
    }

    #[test]
    fn scroll_snap_align_two_keywords() {
        let root = lay("<p>x</p>", "p { scroll-snap-align: start end; }");
        let s = first_p_style(&root);
        assert_eq!(s.scroll_snap_align.block, ScrollSnapAlignKeyword::Start);
        assert_eq!(s.scroll_snap_align.inline, ScrollSnapAlignKeyword::End);
    }

    #[test]
    fn scroll_snap_stop_always() {
        let root = lay("<p>x</p>", "p { scroll-snap-stop: always; }");
        assert_eq!(first_p_style(&root).scroll_snap_stop, ScrollSnapStop::Always);
    }

    #[test]
    fn scroll_margin_individual() {
        let root = lay("<p>x</p>", "p { scroll-margin-top: 10px; scroll-margin-left: 5px; }");
        let s = first_p_style(&root);
        assert!((s.scroll_margin_top - 10.0).abs() < 1e-6);
        assert!((s.scroll_margin_left - 5.0).abs() < 1e-6);
    }

    #[test]
    fn scroll_margin_shorthand_4_values() {
        let root = lay("<p>x</p>", "p { scroll-margin: 1px 2px 3px 4px; }");
        let s = first_p_style(&root);
        assert!((s.scroll_margin_top - 1.0).abs() < 1e-6);
        assert!((s.scroll_margin_right - 2.0).abs() < 1e-6);
        assert!((s.scroll_margin_bottom - 3.0).abs() < 1e-6);
        assert!((s.scroll_margin_left - 4.0).abs() < 1e-6);
    }

    #[test]
    fn scroll_padding_shorthand_1_value() {
        let root = lay("<p>x</p>", "p { scroll-padding: 5px; }");
        let s = first_p_style(&root);
        assert!((s.scroll_padding_top - 5.0).abs() < 1e-6);
        assert!((s.scroll_padding_right - 5.0).abs() < 1e-6);
        assert!((s.scroll_padding_bottom - 5.0).abs() < 1e-6);
        assert!((s.scroll_padding_left - 5.0).abs() < 1e-6);
    }

    // ──────── CSS Overscroll Behavior L1 ────────

    #[test]
    fn overscroll_behavior_contain() {
        let root = lay("<p>x</p>", "p { overscroll-behavior: contain; }");
        let s = first_p_style(&root);
        assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::Contain);
        assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::Contain);
    }

    #[test]
    fn overscroll_behavior_two_values() {
        let root = lay("<p>x</p>", "p { overscroll-behavior: contain none; }");
        let s = first_p_style(&root);
        assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::Contain);
        assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::None);
    }

    #[test]
    fn overscroll_behavior_individual_axis() {
        let root = lay("<p>x</p>", "p { overscroll-behavior-x: none; overscroll-behavior-y: auto; }");
        let s = first_p_style(&root);
        assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::None);
        assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::Auto);
    }

    #[test]
    fn scroll_snap_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { scroll-snap-type: x mandatory; }",
        );
        let p = nested_p_style(&root);
        // Не наследуется.
        assert_eq!(p.scroll_snap_type.axis, ScrollSnapAxis::None);
    }

    // ──────── mask-* + scrollbar-* ────────

    #[test]
    fn mask_image_url() {
        let root = lay("<p>x</p>", "p { mask-image: url(\"mask.png\"); }");
        assert_eq!(
            first_p_style(&root).mask_image,
            BackgroundImage::Url("mask.png".into())
        );
    }

    #[test]
    fn mask_image_none_clears() {
        let root = lay("<p>x</p>", "p { mask-image: url(m.png); mask-image: none; }");
        assert_eq!(first_p_style(&root).mask_image, BackgroundImage::None);
    }

    #[test]
    fn mask_repeat_no_repeat() {
        let root = lay("<p>x</p>", "p { mask-repeat: no-repeat; }");
        assert_eq!(first_p_style(&root).mask_repeat, BackgroundRepeat::NoRepeat);
    }

    #[test]
    fn mask_size_cover() {
        let root = lay("<p>x</p>", "p { mask-size: cover; }");
        assert_eq!(first_p_style(&root).mask_size, BackgroundSize::Cover);
    }

    #[test]
    fn scrollbar_width_thin() {
        let root = lay("<p>x</p>", "p { scrollbar-width: thin; }");
        assert_eq!(first_p_style(&root).scrollbar_width, ScrollbarWidth::Thin);
    }

    #[test]
    fn scrollbar_width_none() {
        let root = lay("<p>x</p>", "p { scrollbar-width: none; }");
        assert_eq!(first_p_style(&root).scrollbar_width, ScrollbarWidth::None);
    }

    #[test]
    fn scrollbar_width_inherited() {
        let root = lay("<div><p>x</p></div>", "div { scrollbar-width: thin; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.scrollbar_width, ScrollbarWidth::Thin);
    }

    #[test]
    fn scrollbar_color_pair() {
        let root = lay(
            "<p>x</p>",
            "p { scrollbar-color: red blue; }",
        );
        let (thumb, track) = first_p_style(&root).scrollbar_color.unwrap();
        assert_eq!(thumb, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(track, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn scrollbar_color_with_rgb_functions() {
        let root = lay(
            "<p>x</p>",
            "p { scrollbar-color: rgb(100, 100, 100) rgb(200, 200, 200); }",
        );
        let (thumb, _) = first_p_style(&root).scrollbar_color.unwrap();
        assert_eq!(thumb, Color { r: 100, g: 100, b: 100, a: 255 });
    }

    #[test]
    fn scrollbar_color_auto() {
        let root = lay("<p>x</p>", "p { scrollbar-color: red blue; scrollbar-color: auto; }");
        assert!(first_p_style(&root).scrollbar_color.is_none());
    }

    #[test]
    fn scrollbar_gutter_stable() {
        let root = lay("<p>x</p>", "p { scrollbar-gutter: stable; }");
        assert_eq!(first_p_style(&root).scrollbar_gutter, ScrollbarGutter::Stable);
    }

    #[test]
    fn scrollbar_gutter_stable_both_edges() {
        let root = lay("<p>x</p>", "p { scrollbar-gutter: stable both-edges; }");
        assert_eq!(
            first_p_style(&root).scrollbar_gutter,
            ScrollbarGutter::StableBothEdges
        );
    }

    // ──────── transform-origin / perspective / list-style-* / transition-* ────────

    #[test]
    fn transform_origin_x_y_z() {
        let root = lay("<p>x</p>", "p { transform-origin: 10px 20px 30px; }");
        assert_eq!(first_p_style(&root).transform_origin, (10.0, 20.0, 30.0));
    }

    #[test]
    fn transform_origin_partial_defaults_to_zero() {
        let root = lay("<p>x</p>", "p { transform-origin: 50px; }");
        assert_eq!(first_p_style(&root).transform_origin, (50.0, 0.0, 0.0));
    }

    #[test]
    fn transform_origin_not_inherited() {
        let root = lay("<div><p>x</p></div>", "div { transform-origin: 10px 20px; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.transform_origin, (0.0, 0.0, 0.0));
        assert_eq!(div.style.transform_origin, (10.0, 20.0, 0.0));
    }

    #[test]
    fn perspective_length() {
        let root = lay("<p>x</p>", "p { perspective: 800px; }");
        assert_eq!(first_p_style(&root).perspective, Some(800.0));
    }

    #[test]
    fn perspective_none() {
        let root = lay("<p>x</p>", "p { perspective: 800px; perspective: none; }");
        assert_eq!(first_p_style(&root).perspective, None);
    }

    #[test]
    fn perspective_zero_treated_as_none() {
        let root = lay("<p>x</p>", "p { perspective: 0px; }");
        assert_eq!(first_p_style(&root).perspective, None);
    }

    #[test]
    fn list_style_type_decimal() {
        let root = lay("<p>x</p>", "p { list-style-type: decimal; }");
        assert_eq!(first_p_style(&root).list_style_type, ListStyleType::Decimal);
    }

    #[test]
    fn list_style_type_none() {
        let root = lay("<p>x</p>", "p { list-style-type: none; }");
        assert_eq!(first_p_style(&root).list_style_type, ListStyleType::None);
    }

    #[test]
    fn list_style_type_lower_roman() {
        let root = lay("<p>x</p>", "p { list-style-type: lower-roman; }");
        assert_eq!(first_p_style(&root).list_style_type, ListStyleType::LowerRoman);
    }

    #[test]
    fn list_style_position_inside() {
        let root = lay("<p>x</p>", "p { list-style-position: inside; }");
        assert_eq!(first_p_style(&root).list_style_position, ListStylePosition::Inside);
    }

    #[test]
    fn list_style_image_url() {
        let root = lay("<p>x</p>", "p { list-style-image: url(\"bullet.png\"); }");
        assert_eq!(
            first_p_style(&root).list_style_image,
            Some("bullet.png".to_string())
        );
    }

    #[test]
    fn list_style_shorthand_combines() {
        let root = lay("<p>x</p>", "p { list-style: square inside; }");
        let s = first_p_style(&root);
        assert_eq!(s.list_style_type, ListStyleType::Square);
        assert_eq!(s.list_style_position, ListStylePosition::Inside);
    }

    #[test]
    fn list_style_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { list-style-type: square; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.list_style_type, ListStyleType::Square);
    }

    #[test]
    fn transition_property_single() {
        let root = lay("<p>x</p>", "p { transition-property: opacity; }");
        assert_eq!(
            first_p_style(&root).transition_properties,
            vec!["opacity".to_string()]
        );
    }

    #[test]
    fn transition_property_list() {
        let root = lay("<p>x</p>", "p { transition-property: opacity, transform, color; }");
        let s = first_p_style(&root);
        assert_eq!(s.transition_properties.len(), 3);
        assert_eq!(s.transition_properties[0], "opacity");
        assert_eq!(s.transition_properties[2], "color");
    }

    #[test]
    fn transition_property_none_clears() {
        let root = lay(
            "<p>x</p>",
            "p { transition-property: opacity; transition-property: none; }",
        );
        assert!(first_p_style(&root).transition_properties.is_empty());
    }

    #[test]
    fn transition_duration_seconds_and_ms() {
        let root = lay("<p>x</p>", "p { transition-duration: 0.5s, 200ms, 1s; }");
        let durations = &first_p_style(&root).transition_durations;
        assert_eq!(durations.len(), 3);
        assert!((durations[0] - 0.5).abs() < 1e-5);
        assert!((durations[1] - 0.2).abs() < 1e-5);
        assert!((durations[2] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn transition_delay_parses() {
        let root = lay("<p>x</p>", "p { transition-delay: 100ms; }");
        let s = first_p_style(&root);
        assert!((s.transition_delays[0] - 0.1).abs() < 1e-5);
    }

    // ──────── CSS Text typography (tab-size, caret-color, overflow-wrap, word-break, hyphens) ────────

    #[test]
    fn tab_size_integer_in_spaces() {
        let root = lay("<p>x</p>", "p { tab-size: 4; }");
        // integer 4 → 32px (8px-per-space).
        assert!((first_p_style(&root).tab_size - 32.0).abs() < 0.01);
    }

    #[test]
    fn tab_size_length() {
        let root = lay("<p>x</p>", "p { tab-size: 40px; }");
        assert!((first_p_style(&root).tab_size - 40.0).abs() < 0.01);
    }

    #[test]
    fn tab_size_default_64() {
        let root = lay("<p>x</p>", "p { color: red; }");
        assert!((first_p_style(&root).tab_size - 64.0).abs() < 0.01);
    }

    #[test]
    fn tab_size_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { tab-size: 100px; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.tab_size - 100.0).abs() < 0.01);
    }

    #[test]
    fn caret_color_named() {
        let root = lay("<p>x</p>", "p { caret-color: red; }");
        assert_eq!(
            first_p_style(&root).caret_color,
            Some(Color { r: 255, g: 0, b: 0, a: 255 })
        );
    }

    #[test]
    fn caret_color_auto() {
        let root = lay("<p>x</p>", "p { caret-color: red; caret-color: auto; }");
        assert_eq!(first_p_style(&root).caret_color, None);
    }

    #[test]
    fn caret_color_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { caret-color: blue; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.caret_color, Some(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn overflow_wrap_break_word() {
        let root = lay("<p>x</p>", "p { overflow-wrap: break-word; }");
        assert_eq!(first_p_style(&root).overflow_wrap, OverflowWrap::BreakWord);
    }

    #[test]
    fn word_wrap_alias_overflow_wrap() {
        // `word-wrap` legacy alias.
        let root = lay("<p>x</p>", "p { word-wrap: anywhere; }");
        assert_eq!(first_p_style(&root).overflow_wrap, OverflowWrap::Anywhere);
    }

    #[test]
    fn word_break_keep_all() {
        let root = lay("<p>x</p>", "p { word-break: keep-all; }");
        assert_eq!(first_p_style(&root).word_break, WordBreak::KeepAll);
    }

    #[test]
    fn word_break_break_all() {
        let root = lay("<p>x</p>", "p { word-break: break-all; }");
        assert_eq!(first_p_style(&root).word_break, WordBreak::BreakAll);
    }

    #[test]
    fn hyphens_auto() {
        let root = lay("<p>x</p>", "p { hyphens: auto; }");
        assert_eq!(first_p_style(&root).hyphens, Hyphens::Auto);
    }

    #[test]
    fn hyphens_none() {
        let root = lay("<p>x</p>", "p { hyphens: none; }");
        assert_eq!(first_p_style(&root).hyphens, Hyphens::None);
    }

    #[test]
    fn hyphens_default_manual() {
        let root = lay("<p>x</p>", "p { color: red; }");
        assert_eq!(first_p_style(&root).hyphens, Hyphens::Manual);
    }

    #[test]
    fn text_typography_all_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { tab-size: 50px; overflow-wrap: break-word; word-break: keep-all; hyphens: auto; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.tab_size - 50.0).abs() < 0.01);
        assert_eq!(p.style.overflow_wrap, OverflowWrap::BreakWord);
        assert_eq!(p.style.word_break, WordBreak::KeepAll);
        assert_eq!(p.style.hyphens, Hyphens::Auto);
        // А значения у div те же.
        assert!((div.style.tab_size - 50.0).abs() < 0.01);
    }

    // ──────── will-change / pointer-events / user-select / scroll-behavior ────────

    #[test]
    fn will_change_auto_is_empty_list() {
        let root = lay("<p>x</p>", "p { will-change: auto; }");
        assert!(first_p_style(&root).will_change.is_empty());
    }

    #[test]
    fn will_change_property_list() {
        let root = lay("<p>x</p>", "p { will-change: transform, opacity; }");
        let s = first_p_style(&root);
        assert_eq!(
            s.will_change,
            vec!["transform".to_string(), "opacity".to_string()]
        );
    }

    #[test]
    fn will_change_invalid_ident_skipped() {
        let root = lay("<p>x</p>", "p { will-change: 1invalid, transform; }");
        let s = first_p_style(&root);
        assert_eq!(s.will_change, vec!["transform".to_string()]);
    }

    #[test]
    fn pointer_events_none() {
        let root = lay("<p>x</p>", "p { pointer-events: none; }");
        assert_eq!(first_p_style(&root).pointer_events, PointerEvents::None);
    }

    #[test]
    fn pointer_events_all() {
        let root = lay("<p>x</p>", "p { pointer-events: all; }");
        assert_eq!(first_p_style(&root).pointer_events, PointerEvents::All);
    }

    #[test]
    fn user_select_none() {
        let root = lay("<p>x</p>", "p { user-select: none; }");
        assert_eq!(first_p_style(&root).user_select, UserSelect::None);
    }

    #[test]
    fn user_select_text() {
        let root = lay("<p>x</p>", "p { user-select: text; }");
        assert_eq!(first_p_style(&root).user_select, UserSelect::Text);
    }

    #[test]
    fn user_select_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { user-select: none; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Inherited.
        assert_eq!(p.style.user_select, UserSelect::None);
    }

    #[test]
    fn scroll_behavior_smooth() {
        let root = lay("<p>x</p>", "p { scroll-behavior: smooth; }");
        assert_eq!(first_p_style(&root).scroll_behavior, ScrollBehavior::Smooth);
    }

    #[test]
    fn scroll_behavior_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { scroll-behavior: smooth; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.scroll_behavior, ScrollBehavior::Smooth);
    }

    #[test]
    fn pointer_events_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { pointer-events: none; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // НЕ наследуется — у p default Auto.
        assert_eq!(p.style.pointer_events, PointerEvents::Auto);
        assert_eq!(div.style.pointer_events, PointerEvents::None);
    }

    #[test]
    fn unknown_keyword_keeps_default() {
        let root = lay("<p>x</p>", "p { pointer-events: garbage; user-select: weird; }");
        let s = first_p_style(&root);
        assert_eq!(s.pointer_events, PointerEvents::Auto);
        assert_eq!(s.user_select, UserSelect::Auto);
    }

    // ──────── background-* (CSS Backgrounds L3) ────────

    #[test]
    fn background_image_url_parses() {
        let root = lay("<p>x</p>", "p { background-image: url(\"bg.png\"); }");
        let s = first_p_style(&root);
        assert_eq!(s.background_image, BackgroundImage::Url("bg.png".into()));
    }

    #[test]
    fn background_image_url_unquoted() {
        let root = lay("<p>x</p>", "p { background-image: url(bg.png); }");
        assert_eq!(
            first_p_style(&root).background_image,
            BackgroundImage::Url("bg.png".into())
        );
    }

    #[test]
    fn background_image_none() {
        let root = lay(
            "<p>x</p>",
            "p { background-image: url(\"x.png\"); background-image: none; }",
        );
        assert_eq!(first_p_style(&root).background_image, BackgroundImage::None);
    }

    #[test]
    fn background_image_gradient_kept_as_string() {
        let root = lay(
            "<p>x</p>",
            "p { background-image: linear-gradient(to right, red, blue); }",
        );
        match &first_p_style(&root).background_image {
            BackgroundImage::Gradient(s) => assert!(s.contains("linear-gradient")),
            _ => panic!("expected Gradient"),
        }
    }

    #[test]
    fn background_repeat_values() {
        for (s, expected) in [
            ("repeat", BackgroundRepeat::Repeat),
            ("no-repeat", BackgroundRepeat::NoRepeat),
            ("repeat-x", BackgroundRepeat::RepeatX),
            ("repeat-y", BackgroundRepeat::RepeatY),
            ("round", BackgroundRepeat::Round),
            ("space", BackgroundRepeat::Space),
        ] {
            let css = format!("p {{ background-repeat: {s}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).background_repeat, expected);
        }
    }

    #[test]
    fn background_size_keywords() {
        for (s, expected) in [
            ("auto", BackgroundSize::Auto),
            ("cover", BackgroundSize::Cover),
            ("contain", BackgroundSize::Contain),
        ] {
            let css = format!("p {{ background-size: {s}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).background_size, expected);
        }
    }

    #[test]
    fn background_size_length_single() {
        let root = lay("<p>x</p>", "p { background-size: 200px; }");
        match first_p_style(&root).background_size {
            BackgroundSize::Length(w, h) => {
                assert!((w - 200.0).abs() < 0.01);
                assert_eq!(h, None);
            }
            _ => panic!("expected Length"),
        }
    }

    #[test]
    fn background_size_length_pair() {
        let root = lay("<p>x</p>", "p { background-size: 200px 100px; }");
        match first_p_style(&root).background_size {
            BackgroundSize::Length(w, h) => {
                assert!((w - 200.0).abs() < 0.01);
                assert_eq!(h, Some(100.0));
            }
            _ => panic!("expected Length"),
        }
    }

    #[test]
    fn background_attachment_values() {
        for (s, expected) in [
            ("scroll", BackgroundAttachment::Scroll),
            ("fixed", BackgroundAttachment::Fixed),
            ("local", BackgroundAttachment::Local),
        ] {
            let css = format!("p {{ background-attachment: {s}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).background_attachment, expected);
        }
    }

    #[test]
    fn background_properties_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { background-image: url(x.png); background-repeat: no-repeat; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.background_image, BackgroundImage::None);
        assert_eq!(p.style.background_repeat, BackgroundRepeat::Repeat);
    }

    // ──────── place-items / align-* / justify-* (CSS Box Alignment L3) ────────

    #[test]
    fn align_items_center() {
        let root = lay("<p>x</p>", "p { align-items: center; }");
        assert_eq!(first_p_style(&root).align_items, AlignValue::Center);
    }

    #[test]
    fn justify_content_space_between() {
        let root = lay("<p>x</p>", "p { justify-content: space-between; }");
        assert_eq!(first_p_style(&root).justify_content, AlignValue::SpaceBetween);
    }

    #[test]
    fn flex_start_alias() {
        // CSS spec: flex-start alias для start (вне flex-контекста).
        let root = lay("<p>x</p>", "p { align-items: flex-start; }");
        assert_eq!(first_p_style(&root).align_items, AlignValue::Start);
    }

    #[test]
    fn place_items_single_value() {
        let root = lay("<p>x</p>", "p { place-items: center; }");
        let s = first_p_style(&root);
        // Single value применяется к обоим осям.
        assert_eq!(s.align_items, AlignValue::Center);
        assert_eq!(s.justify_items, AlignValue::Center);
    }

    #[test]
    fn place_items_two_values() {
        let root = lay("<p>x</p>", "p { place-items: start end; }");
        let s = first_p_style(&root);
        assert_eq!(s.align_items, AlignValue::Start);
        assert_eq!(s.justify_items, AlignValue::End);
    }

    #[test]
    fn place_self_shorthand() {
        let root = lay("<p>x</p>", "p { place-self: center stretch; }");
        let s = first_p_style(&root);
        assert_eq!(s.align_self, AlignValue::Center);
        assert_eq!(s.justify_self, AlignValue::Stretch);
    }

    #[test]
    fn place_content_shorthand() {
        let root = lay("<p>x</p>", "p { place-content: space-around; }");
        let s = first_p_style(&root);
        assert_eq!(s.align_content, AlignValue::SpaceAround);
        assert_eq!(s.justify_content, AlignValue::SpaceAround);
    }

    #[test]
    fn align_unknown_value_ignored() {
        let root = lay("<p>x</p>", "p { align-items: garbage; }");
        // default (Auto) сохраняется.
        assert_eq!(first_p_style(&root).align_items, AlignValue::Auto);
    }

    #[test]
    fn alignment_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { align-items: center; justify-content: space-between; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // У p должны быть defaults.
        assert_eq!(p.style.align_items, AlignValue::Auto);
        assert_eq!(p.style.justify_content, AlignValue::Auto);
        // У div — заданные.
        assert_eq!(div.style.align_items, AlignValue::Center);
        assert_eq!(div.style.justify_content, AlignValue::SpaceBetween);
    }

    #[test]
    fn align_value_parse_all_keywords() {
        for (s, expected) in [
            ("auto", AlignValue::Auto),
            ("normal", AlignValue::Normal),
            ("stretch", AlignValue::Stretch),
            ("start", AlignValue::Start),
            ("end", AlignValue::End),
            ("center", AlignValue::Center),
            ("baseline", AlignValue::Baseline),
            ("space-between", AlignValue::SpaceBetween),
            ("space-around", AlignValue::SpaceAround),
            ("space-evenly", AlignValue::SpaceEvenly),
            ("flex-start", AlignValue::Start),
            ("flex-end", AlignValue::End),
            ("self-start", AlignValue::Start),
            ("CENTER", AlignValue::Center),  // case-insensitive
        ] {
            assert_eq!(AlignValue::parse(s), Some(expected), "input: {s}");
        }
    }

    #[test]
    fn align_value_parse_unknown_returns_none() {
        assert_eq!(AlignValue::parse("garbage"), None);
        assert_eq!(AlignValue::parse(""), None);
    }

    #[test]
    fn gap_and_aspect_ratio_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { gap: 20px; aspect-ratio: 2; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.row_gap, 0.0);
        assert_eq!(p.style.aspect_ratio, None);
        assert!((div.style.row_gap - 20.0).abs() < 0.01);
        assert!(div.style.aspect_ratio.is_some());
    }

    #[test]
    fn media_prefers_color_scheme_light_default() {
        // Phase 0: prefers_dark=false → 'light' matches.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (prefers-color-scheme: light) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    // ── CSS Quirks Mode — UA-rule для <table> ──────────────────────────────

    /// В Quirks-mode (нет DOCTYPE) `<table>` сбрасывает font-size к
    /// initial-значению, не наследует от родителя.
    #[test]
    fn quirks_table_font_size_resets_to_initial() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert!(
            (body.style.font_size - 30.0).abs() < 0.01,
            "body должен наследовать заявленные 30px"
        );
        assert!(
            (table.style.font_size - 16.0).abs() < 0.01,
            "table в Quirks должен сбросить font-size к initial 16, получено {}",
            table.style.font_size
        );
    }

    /// В Standards mode (`<!DOCTYPE html>`) `<table>` наследует font-size
    /// от родителя как обычный элемент.
    #[test]
    fn standards_table_font_size_inherits() {
        let root = lay(
            "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert!(
            (table.style.font_size - 30.0).abs() < 0.01,
            "table в Standards должен наследовать 30px, получено {}",
            table.style.font_size
        );
    }

    /// В Quirks color у `<table>` сбрасывается к BLACK, не наследуется.
    #[test]
    fn quirks_table_color_resets_to_black() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { color: red; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert_eq!(body.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(table.style.color, Color::BLACK);
    }

    /// В Standards color наследуется.
    #[test]
    fn standards_table_color_inherits() {
        let root = lay(
            "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
            "body { color: red; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// В Quirks font-weight у `<table>` сбрасывается к NORMAL.
    #[test]
    fn quirks_table_font_weight_resets_to_normal() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-weight: bold; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert_eq!(body.style.font_weight, FontWeight::BOLD);
        assert_eq!(table.style.font_weight, FontWeight::NORMAL);
    }

    /// В Quirks font-style у `<table>` сбрасывается к Normal.
    #[test]
    fn quirks_table_font_style_resets_to_normal() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-style: italic; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert_eq!(body.style.font_style, FontStyle::Italic);
        assert_eq!(table.style.font_style, FontStyle::Normal);
    }

    /// В Quirks text-align у `<table>` сбрасывается к Left.
    #[test]
    fn quirks_table_text_align_resets_to_left() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { text-align: center; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert_eq!(body.style.text_align, TextAlign::Center);
        assert_eq!(table.style.text_align, TextAlign::Left);
    }

    /// В Quirks white-space у `<table>` сбрасывается к Normal.
    #[test]
    fn quirks_table_white_space_resets_to_normal() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { white-space: nowrap; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert_eq!(body.style.white_space, WhiteSpace::Nowrap);
        assert_eq!(table.style.white_space, WhiteSpace::Normal);
    }

    /// Author CSS поверх Quirks-reset выигрывает: spec-rule идёт как
    /// низший cascade origin (UA).
    #[test]
    fn quirks_table_author_css_wins_over_reset() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; } table { font-size: 24px; color: blue; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert!(
            (table.style.font_size - 24.0).abs() < 0.01,
            "author CSS должен переопределить Quirks-reset"
        );
        assert_eq!(table.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    /// Дочерние элементы `<table>` в Quirks наследуют от сброшенных
    /// значений таблицы, не от прародителя.
    #[test]
    fn quirks_table_children_inherit_reset_values() {
        // <body>=30px → <table>=16 (reset) → <td>=16 (inherits from table).
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        // <tbody> wrap: html-parser сам не добавляет implicit `<tbody>`,
        // поэтому <tr> может быть прямым ребёнком <table>. <td> внутри.
        // Идём вглубь, пока не найдём td.
        fn find_td(b: &LayoutBox) -> Option<&LayoutBox> {
            for c in &b.children {
                if matches!(&c.kind, BoxKind::Block) {
                    if let Some(td) = find_td(c) {
                        return Some(td);
                    }
                    return Some(c);
                }
            }
            None
        }
        let td = find_td(table).expect("td не найден");
        assert!(
            (td.style.font_size - 16.0).abs() < 0.01,
            "td должен унаследовать от table сброшенные 16px, получено {}",
            td.style.font_size
        );
    }

    /// Не-`<table>` элементы в Quirks-mode не сбрасывают inherited.
    #[test]
    fn quirks_non_table_inherits_normally() {
        let root = lay(
            "<body><p>x</p></body>",
            "body { font-size: 30px; color: red; }",
        );
        let body = first_element_child(&root);
        let p = first_element_child(body);
        assert!(
            (p.style.font_size - 30.0).abs() < 0.01,
            "<p> в Quirks-mode должен наследовать font-size, получено {}",
            p.style.font_size
        );
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// LimitedQuirks (HTML 4.01 Transitional) — table-reset не применяется
    /// (spec §4.1: только в Quirks-mode).
    #[test]
    fn limited_quirks_does_not_apply_table_reset() {
        let root = lay(
            "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; color: red; }",
        );
        let body = first_element_child(&root);
        let table = first_element_child(body);
        assert!(
            (table.style.font_size - 30.0).abs() < 0.01,
            "table в LimitedQuirks должен наследовать font-size как в Standards"
        );
        assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    // ── CSS Quirks Mode §3.4 — «hashless hex color quirk» ──────────────────

    /// В Quirks-mode `color: ff0000` (без `#`) парсится как red.
    /// Это эквивалент `color: #ff0000` (CSS Quirks Mode §3.4).
    #[test]
    fn quirks_hashless_hex_in_color_property() {
        // Нет DOCTYPE → Quirks.
        let root = lay(
            "<body><p>x</p></body>",
            "p { color: ff0000; }",
        );
        let body = first_element_child(&root);
        let p = first_element_child(body);
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// В Standards-mode `color: ff0000` (без `#`) — невалидное значение,
    /// игнорируется. Цвет наследуется (по умолчанию BLACK).
    #[test]
    fn standards_hashless_hex_rejected_in_color_property() {
        let root = lay(
            "<!DOCTYPE html><body><p>x</p></body>",
            "p { color: ff0000; }",
        );
        let body = first_element_child(&root);
        let p = first_element_child(body);
        // ff0000 без `#` — невалидно в Standards, color остаётся inherited
        // от body (BLACK).
        assert_eq!(p.style.color, Color::BLACK);
    }

    /// В Quirks `background-color: 00ff00` (6-hex без `#`) парсится как green.
    #[test]
    fn quirks_hashless_hex_in_background_color() {
        let root = lay(
            "<body><p>x</p></body>",
            "p { background-color: 00ff00; }",
        );
        let body = first_element_child(&root);
        let p = first_element_child(body);
        assert_eq!(p.style.background_color, Some(Color { r: 0, g: 255, b: 0, a: 255 }));
    }

    /// В Quirks 3-hex bare digit-ы тоже парсятся: `f00` → red.
    #[test]
    fn quirks_hashless_hex_3_digit_short() {
        let root = lay(
            "<body><p>x</p></body>",
            "p { color: f00; }",
        );
        let body = first_element_child(&root);
        let p = first_element_child(body);
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// В Quirks border-color принимает bare hex.
    #[test]
    fn quirks_hashless_hex_in_border_color() {
        let root = lay(
            "<body><p>x</p></body>",
            "p { border: 1px solid 0000ff; }",
        );
        let body = first_element_child(&root);
        let p = first_element_child(body);
        assert_eq!(
            p.style.border_top_color,
            Some(Color { r: 0, g: 0, b: 255, a: 255 }),
        );
    }

    /// LimitedQuirks (HTML 4.01 Transitional) — hashless hex quirk
    /// НЕ применяется (spec §1.1.1: «full quirks mode only»).
    #[test]
    fn limited_quirks_hashless_hex_rejected() {
        let root = lay(
            "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><p>x</p></body>",
            "p { color: ff0000; }",
        );
        let body = first_element_child(&root);
        let p = first_element_child(body);
        // В LimitedQuirks bare hex — invalid, как в Standards.
        assert_eq!(p.style.color, Color::BLACK);
    }
}
