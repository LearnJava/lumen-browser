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
        BoxKind::Text(s) => {
            out.push(DisplayCommand::DrawText {
                rect: b.rect,
                text: s.clone(),
                font_size: b.style.font_size,
                color: b.style.color,
            });
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
}
