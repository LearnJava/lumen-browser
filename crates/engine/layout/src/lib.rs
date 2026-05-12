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
//! единицы кроме px, color-функции (rgb/hsl/rgba), width/height в CSS,
//! text-decoration, font-weight/style на уровне inline.

pub mod box_tree;
pub mod snapshot;
pub mod style;

pub use box_tree::{layout, layout_measured, BoxKind, InlineFrag, InlineSegment, LayoutBox};
pub use snapshot::serialize_layout_tree;
pub use style::{Color, ComputedStyle, Display};

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
}
