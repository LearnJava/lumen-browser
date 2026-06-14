//! Visible text iteration — [`collect_visible_text`].
//!
//! Walks the layout tree and returns all text fragments visible on screen,
//! with their absolute viewport rectangles. Used by the shell's find-in-page
//! with regex support to locate and highlight search matches.

use lumen_core::geom::Rect;
use lumen_dom::NodeId;
use crate::{BoxKind, LayoutBox, Visibility};

/// A visible text fragment with its absolute screen rectangle.
///
/// Produced by [`collect_visible_text`]. Each fragment corresponds to one
/// word-wrapped piece of text — the same granularity as the inline layout
/// engine's `InlineFrag`.
#[derive(Debug, Clone)]
pub struct TextFragment {
    /// Visible text content (one word or contiguous run after line-wrapping).
    pub text: String,
    /// Absolute viewport rectangle in CSS px (top-left origin, document-relative,
    /// before scroll offset). Matches the coordinate space of `LayoutBox::rect`.
    pub rect: Rect,
    /// DOM text node that produced this fragment. `NodeId(0)` for generated
    /// content (e.g. `::before`/`::after` pseudo-elements) with no DOM source.
    pub node: NodeId,
    /// UTF-8 byte offset of `text[0]` within the source node's character data.
    pub char_offset: u32,
}

/// Walk the layout tree and collect all visible text fragments with screen coordinates.
///
/// Fragments are returned in document order (pre-order DFS). Only non-empty
/// text frags are included; image alt-text and forced line-break markers are
/// skipped. Frags with `visibility: hidden` are omitted because they are not
/// rendered. `BoxKind::Skip` subtrees (e.g. comment/doctype nodes) are also
/// skipped.
pub fn collect_visible_text(root: &LayoutBox) -> Vec<TextFragment> {
    let mut out = Vec::new();
    collect_text_rec(root, &mut out);
    out
}

fn collect_text_rec(b: &LayoutBox, out: &mut Vec<TextFragment>) {
    if matches!(b.kind, BoxKind::Skip) {
        return;
    }

    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        let line_h = b.style.font_size * b.style.line_height;
        for (line_idx, line) in lines.iter().enumerate() {
            let line_y = b.rect.y + line_idx as f32 * line_h;
            for frag in line {
                if frag.text.is_empty() || frag.img_src.is_some() {
                    continue;
                }
                if frag.style.visibility != Visibility::Visible {
                    continue;
                }
                out.push(TextFragment {
                    text: frag.text.clone(),
                    rect: Rect {
                        x: b.rect.x + frag.x,
                        y: line_y,
                        width: frag.width,
                        height: line_h,
                    },
                    node: frag.source_node,
                    char_offset: frag.source_char_offset,
                });
            }
        }
    }

    for child in &b.children {
        collect_text_rec(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_dom::NodeId;
    use lumen_core::geom::Rect;
    use crate::{BoxKind, InlineFrag, LayoutBox};
    use crate::style::{ComputedStyle, Visibility};

    fn make_frag(text: &str, x: f32, width: f32, source_node: NodeId, char_offset: u32) -> InlineFrag {
        InlineFrag {
            x,
            width,
            y_offset: 0.0,
            text: text.to_string(),
            style: ComputedStyle::root(),
            padding_left: 0.0,
            padding_right: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            is_first_line: true,
            source_node,
            source_char_offset: char_offset,
        }
    }

    fn make_hidden_frag(text: &str, x: f32, source_node: NodeId) -> InlineFrag {
        let mut style = ComputedStyle::root();
        style.visibility = Visibility::Hidden;
        InlineFrag {
            x,
            width: 50.0,
            y_offset: 0.0,
            text: text.to_string(),
            style,
            padding_left: 0.0,
            padding_right: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            is_first_line: true,
            source_node,
            source_char_offset: 0,
        }
    }

    fn make_img_frag(x: f32, source_node: NodeId) -> InlineFrag {
        InlineFrag {
            x,
            width: 100.0,
            y_offset: 0.0,
            text: "alt text".to_string(),
            style: ComputedStyle::root(),
            padding_left: 0.0,
            padding_right: 0.0,
            is_element_box: false,
            img_src: Some("image.png".to_string()),
            img_is_lazy: false,
            is_first_line: true,
            source_node,
            source_char_offset: 0,
        }
    }

    fn make_inline_run(rect: Rect, lines: Vec<Vec<InlineFrag>>) -> LayoutBox {
        LayoutBox {
            node: NodeId::from_index(1),
            rect,
            style: ComputedStyle::root(),
            kind: BoxKind::InlineRun { segments: vec![], lines, first_line_style: None },
            children: vec![],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        }
    }

    fn make_block(rect: Rect, children: Vec<LayoutBox>) -> LayoutBox {
        LayoutBox {
            node: NodeId::from_index(1),
            rect,
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children,
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        }
    }

    #[test]
    fn single_frag_collected() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 0.0, 50.0, node, 0);
        let b = make_inline_run(Rect { x: 10.0, y: 20.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        let frags = collect_visible_text(&b);
        assert_eq!(frags.len(), 1);
        assert_eq!(frags[0].text, "hello");
        assert_eq!(frags[0].node, node);
        assert_eq!(frags[0].char_offset, 0);
        // Absolute x = box.x(10) + frag.x(0) = 10
        assert!((frags[0].rect.x - 10.0).abs() < 0.01, "x={}", frags[0].rect.x);
        assert!((frags[0].rect.y - 20.0).abs() < 0.01, "y={}", frags[0].rect.y);
        assert!((frags[0].rect.width - 50.0).abs() < 0.01);
    }

    #[test]
    fn frag_x_includes_box_and_frag_offset() {
        let node = NodeId::from_index(2);
        let frag = make_frag("world", 30.0, 50.0, node, 6);
        let b = make_inline_run(Rect { x: 100.0, y: 0.0, width: 300.0, height: 19.2 }, vec![vec![frag]]);
        let frags = collect_visible_text(&b);
        assert_eq!(frags.len(), 1);
        // x = box.x(100) + frag.x(30) = 130
        assert!((frags[0].rect.x - 130.0).abs() < 0.01);
        assert_eq!(frags[0].char_offset, 6);
    }

    #[test]
    fn multi_line_yields_correct_y() {
        let node = NodeId::from_index(2);
        // ComputedStyle::root() → font_size=16, line_height=1.2 → line_h=19.2
        let frag1 = make_frag("line1", 0.0, 50.0, node, 0);
        let frag2 = make_frag("line2", 0.0, 50.0, NodeId::from_index(3), 6);
        let b = make_inline_run(
            Rect { x: 0.0, y: 10.0, width: 200.0, height: 38.4 },
            vec![vec![frag1], vec![frag2]],
        );
        let frags = collect_visible_text(&b);
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].text, "line1");
        // y line0 = box.y(10) + 0 * 19.2 = 10
        assert!((frags[0].rect.y - 10.0).abs() < 0.01, "y0={}", frags[0].rect.y);
        assert_eq!(frags[1].text, "line2");
        // y line1 = box.y(10) + 1 * 19.2 = 29.2
        assert!((frags[1].rect.y - 29.2).abs() < 0.1, "y1={}", frags[1].rect.y);
    }

    #[test]
    fn multiple_frags_per_line() {
        let n1 = NodeId::from_index(2);
        let n2 = NodeId::from_index(3);
        let frag1 = make_frag("hello", 0.0, 50.0, n1, 0);
        let frag2 = make_frag("world", 60.0, 50.0, n2, 0);
        let b = make_inline_run(
            Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 },
            vec![vec![frag1, frag2]],
        );
        let frags = collect_visible_text(&b);
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].text, "hello");
        assert_eq!(frags[1].text, "world");
        assert!((frags[1].rect.x - 60.0).abs() < 0.01);
    }

    #[test]
    fn empty_frag_skipped() {
        let node = NodeId::from_index(2);
        let empty = make_frag("", 0.0, 0.0, node, 0);
        let real = make_frag("text", 10.0, 40.0, node, 0);
        let b = make_inline_run(
            Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 },
            vec![vec![empty, real]],
        );
        let frags = collect_visible_text(&b);
        assert_eq!(frags.len(), 1);
        assert_eq!(frags[0].text, "text");
    }

    #[test]
    fn image_frag_skipped() {
        let node = NodeId::from_index(2);
        let img = make_img_frag(0.0, node);
        let b = make_inline_run(
            Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 },
            vec![vec![img]],
        );
        assert!(collect_visible_text(&b).is_empty());
    }

    #[test]
    fn hidden_frag_skipped() {
        let node = NodeId::from_index(2);
        let hidden = make_hidden_frag("secret", 0.0, node);
        let b = make_inline_run(
            Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 },
            vec![vec![hidden]],
        );
        assert!(collect_visible_text(&b).is_empty());
    }

    #[test]
    fn nested_block_children_collected() {
        let n1 = NodeId::from_index(2);
        let n2 = NodeId::from_index(3);
        let child1 = make_inline_run(
            Rect { x: 0.0, y: 0.0, width: 100.0, height: 19.2 },
            vec![vec![make_frag("first", 0.0, 50.0, n1, 0)]],
        );
        let child2 = make_inline_run(
            Rect { x: 0.0, y: 30.0, width: 100.0, height: 19.2 },
            vec![vec![make_frag("second", 0.0, 60.0, n2, 0)]],
        );
        let root = make_block(
            Rect { x: 0.0, y: 0.0, width: 400.0, height: 200.0 },
            vec![child1, child2],
        );
        let frags = collect_visible_text(&root);
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].text, "first");
        assert_eq!(frags[1].text, "second");
    }

    #[test]
    fn document_order_preserved() {
        let n1 = NodeId::from_index(2);
        let n2 = NodeId::from_index(3);
        let n3 = NodeId::from_index(4);
        let inline_run = make_inline_run(
            Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 },
            vec![vec![
                make_frag("a", 0.0, 10.0, n1, 0),
                make_frag("b", 15.0, 10.0, n2, 0),
                make_frag("c", 30.0, 10.0, n3, 0),
            ]],
        );
        let frags = collect_visible_text(&inline_run);
        assert_eq!(frags.len(), 3);
        assert_eq!(frags[0].text, "a");
        assert_eq!(frags[1].text, "b");
        assert_eq!(frags[2].text, "c");
    }

    #[test]
    fn skip_box_omitted() {
        let b = LayoutBox {
            node: NodeId::from_index(1),
            rect: Rect { x: 0.0, y: 0.0, width: 100.0, height: 20.0 },
            style: ComputedStyle::root(),
            kind: BoxKind::Skip,
            children: vec![],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        };
        assert!(collect_visible_text(&b).is_empty());
    }
}
