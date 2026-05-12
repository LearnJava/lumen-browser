//! Display list — линейный список графических команд, выработанных из
//! дерева layout. Растеризатору (renderer) уже не нужно понимать DOM/CSS:
//! он рендерит то, что ему говорят.
//!
//! Phase 0 — только `FillRect` и `DrawText`. Тени, скругления, градиенты,
//! border-радиусы — позже, по запросу. Координаты — экранные пиксели от
//! верхнего левого угла окна.

use lumen_core::geom::Rect;
use lumen_layout::{BoxKind, Color, LayoutBox};

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    FillRect {
        rect: Rect,
        color: Color,
    },
    DrawText {
        rect: Rect,
        text: String,
        font_size: f32,
        color: Color,
    },
}

pub type DisplayList = Vec<DisplayCommand>;

/// Сериализует display list в детерминированный текст для snapshot-тестов.
///
/// Формат (одна команда — одна строка):
/// - `FillRect (x.xx, y.xx, w.xx, h.xx) #rrggbbaa`
/// - `DrawText (x.xx, y.xx, w.xx, h.xx) "text" fs.xx #rrggbbaa`
pub fn serialize_display_list(dl: &[DisplayCommand]) -> String {
    let mut out = String::new();
    for cmd in dl {
        match cmd {
            DisplayCommand::FillRect { rect, color } => {
                out.push_str(&format!(
                    "FillRect ({:.2}, {:.2}, {:.2}, {:.2}) #{:02x}{:02x}{:02x}{:02x}\n",
                    rect.x, rect.y, rect.width, rect.height,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::DrawText { rect, text, font_size, color } => {
                out.push_str(&format!(
                    "DrawText ({:.2}, {:.2}, {:.2}, {:.2}) {:?} {:.2} #{:02x}{:02x}{:02x}{:02x}\n",
                    rect.x, rect.y, rect.width, rect.height,
                    text,
                    font_size,
                    color.r, color.g, color.b, color.a,
                ));
            }
        }
    }
    out
}

pub fn build_display_list(root: &LayoutBox) -> DisplayList {
    let mut list = Vec::new();
    walk(root, &mut list);
    list
}

fn walk(b: &LayoutBox, out: &mut DisplayList) {
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            if let Some(bg) = b.style.background_color
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect {
                    rect: b.rect,
                    color: bg,
                });
            }
            for child in &b.children {
                walk(child, out);
            }
        }
        BoxKind::InlineRun { lines, .. } => {
            let line_h = b.style.font_size * b.style.line_height;
            for (line_idx, line) in lines.iter().enumerate() {
                let line_y = b.rect.y + line_idx as f32 * line_h;
                for frag in line {
                    out.push(DisplayCommand::DrawText {
                        rect: Rect::new(b.rect.x + frag.x, line_y, b.rect.width, line_h),
                        text: frag.text.clone(),
                        font_size: frag.style.font_size,
                        color: frag.style.color,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;

    fn build(html: &str, css: &str) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        build_display_list(&tree)
    }

    struct Fixed8;
    impl lumen_layout::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    fn build_wrapped(html: &str, css: &str, width: f32) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8);
        build_display_list(&tree)
    }

    fn fills(dl: &DisplayList) -> Vec<&Color> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(color),
                _ => None,
            })
            .collect()
    }

    fn texts(dl: &DisplayList) -> Vec<&str> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn empty_input_empty_list() {
        let dl = build("", "");
        assert!(dl.is_empty());
    }

    #[test]
    fn block_with_background_emits_fill() {
        let dl = build("<p>x</p>", "p { background: red; }");
        let f = fills(&dl);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].r, 255);
    }

    #[test]
    fn block_without_background_no_fill() {
        let dl = build("<p>x</p>", "");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn text_node_emits_draw_text() {
        let dl = build("<p>hello</p>", "");
        assert_eq!(texts(&dl), vec!["hello"]);
    }

    #[test]
    fn cyrillic_text_preserved() {
        let dl = build("<p>Привет, мир</p>", "");
        assert_eq!(texts(&dl), vec!["Привет, мир"]);
    }

    #[test]
    fn nested_backgrounds_in_parent_then_child_order() {
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; } p { background: blue; }",
        );
        let f = fills(&dl);
        assert_eq!(f.len(), 2);
        // Сначала parent (под текст), потом child — естественный paint-порядок.
        assert_eq!(f[0].r, 255);
        assert_eq!(f[1].b, 255);
    }

    #[test]
    fn transparent_background_omitted() {
        let dl = build("<p>x</p>", "p { background-color: transparent; }");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn skipped_boxes_emit_nothing() {
        let dl = build("<p>x</p><!-- comment --><p>y</p>", "");
        // Только два текстовых узла; комментарий не даёт команды.
        assert_eq!(texts(&dl).len(), 2);
    }

    #[test]
    fn display_none_emits_nothing() {
        let dl = build(
            r#"<p class="x">hidden</p><p>visible</p>"#,
            ".x { display: none; }",
        );
        assert_eq!(texts(&dl), vec!["visible"]);
    }

    // ── Тесты line wrapping ─────────────────────────────────────────────────

    /// При переносе текста на 2 строки должны быть эмитированы 2 DrawText.
    #[test]
    fn wrapped_text_emits_multiple_draw_text() {
        // "hello world" = 11×8 = 88px. Viewport 60px → перенос на 2 строки.
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        assert_eq!(texts(&dl), vec!["hello", "world"]);
    }

    /// Вторая строка у `DrawText` должна быть смещена по Y на line_height.
    #[test]
    fn wrapped_lines_have_correct_y_offset() {
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        let draw_texts: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(draw_texts.len(), 2);
        let line_h = 16.0_f32 * 1.2; // font_size=16, line_height=1.2
        assert!((draw_texts[0].y - 0.0).abs() < 0.01);
        assert!((draw_texts[1].y - line_h).abs() < 0.1, "y1={}", draw_texts[1].y);
    }

    /// Текст без переноса всё равно рисуется одной командой.
    #[test]
    fn no_wrap_single_draw_text() {
        let dl = build_wrapped("<p>hi</p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hi"]);
    }

    // ── Тесты inline-flow ───────────────────────────────────────────────────

    /// Текст с <span> внутри — один DrawText (одинаковый стиль → фрагменты сливаются).
    #[test]
    fn inline_same_style_merges_into_one_draw_text() {
        let dl = build_wrapped("<p>hello <span>world</span></p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hello world"]);
    }

    /// <a> с цветом → два DrawText: "Hello" и "link" с разными цветами.
    #[test]
    fn inline_different_style_emits_separate_draw_texts() {
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let t = texts(&dl);
        assert_eq!(t, vec!["Hello", "link"]);
        // Второй DrawText должен быть синим.
        let blue_cmds: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, color, .. } if text == "link" => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(blue_cmds.len(), 1);
        assert_eq!(blue_cmds[0].b, 255);
    }

    /// X-координата второго фрагмента должна быть правее первого.
    #[test]
    fn inline_fragments_have_increasing_x() {
        // "Hello" (5*8=40) + space(8) + "link" → link начинается в x=48.
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        assert!((rects[0].x - 0.0).abs() < 0.01, "Hello должно быть в x=0");
        assert!(
            rects[1].x > rects[0].x,
            "link должно быть правее: Hello.x={}, link.x={}",
            rects[0].x,
            rects[1].x
        );
    }

    /// Inline-ран переносится: второй DrawText смещён по Y.
    #[test]
    fn inline_run_wrap_y_offset() {
        // "aa" (16px) + " " (8) + "bb" (16) = 40px > 30px viewport → перенос.
        let dl = build_wrapped("<p>aa <span>bb</span></p>", "", 30.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        assert!((rects[0].y - 0.0).abs() < 0.01);
        let line_h = 16.0_f32 * 1.2;
        assert!((rects[1].y - line_h).abs() < 0.1, "y1={}", rects[1].y);
    }
}
