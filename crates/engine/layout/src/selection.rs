//! Selection hit-testing: caret_at_point and selection_rects.
//!
//! Maps pixel coordinates to DOM positions (for mouse click → caret placement)
//! and DOM ranges to pixel rectangles (for selection highlight rendering).

use lumen_core::geom::Rect;
use lumen_dom::{DomPosition, Range};
use crate::{BoxKind, InlineFrag, LayoutBox, TextMeasurer};

/// Find the caret position (DOM node + UTF-8 byte offset) closest to a pixel point.
///
/// Walks the layout tree depth-first. For each `InlineRun` box whose vertical
/// span covers `y`, finds the closest character boundary to `x` and returns
/// the corresponding `DomPosition`. Returns `None` when `(x, y)` falls outside
/// all text content (e.g. on whitespace-only boxes or non-text regions).
pub fn caret_at_point(
    root: &LayoutBox,
    x: f32,
    y: f32,
    measurer: &dyn TextMeasurer,
) -> Option<DomPosition> {
    caret_in_box(root, x, y, measurer)
}

fn caret_in_box(
    b: &LayoutBox,
    x: f32,
    y: f32,
    measurer: &dyn TextMeasurer,
) -> Option<DomPosition> {
    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        let line_h = b.style.font_size * b.style.line_height;
        if line_h > 0.0 && !lines.is_empty() {
            let rel_y = y - b.rect.y;
            if rel_y >= 0.0 && rel_y < line_h * lines.len() as f32 {
                let line_idx = ((rel_y / line_h) as usize).min(lines.len() - 1);
                if let Some(pos) = caret_in_line(b, &lines[line_idx], x, measurer) {
                    return Some(pos);
                }
            }
        }
    }
    for child in &b.children {
        if let Some(pos) = caret_in_box(child, x, y, measurer) {
            return Some(pos);
        }
    }
    None
}

/// Find caret position within a single line given absolute x.
fn caret_in_line(
    b: &LayoutBox,
    line: &[InlineFrag],
    x: f32,
    measurer: &dyn TextMeasurer,
) -> Option<DomPosition> {
    if line.is_empty() {
        return None;
    }
    let rel_x = x - b.rect.x;
    // Find first frag whose right edge is past rel_x; fall back to last frag.
    let mut frag = &line[0];
    for f in line {
        if f.text.is_empty() {
            continue;
        }
        frag = f;
        if f.x + f.width >= rel_x {
            break;
        }
    }
    if frag.text.is_empty() {
        return None;
    }
    let frag_rel_x = (rel_x - frag.x).max(0.0);
    let byte_off = byte_offset_at_x(&frag.text, frag_rel_x, frag.style.font_size, measurer);
    Some(DomPosition {
        container: frag.source_node,
        offset: frag.source_char_offset + byte_off,
    })
}

/// Compute pixel rectangles that cover the selected `range` within the layout tree.
///
/// Each returned `Rect` spans one contiguous horizontal run of selected text on
/// one line. Coordinates are in the same space as `LayoutBox::rect` (viewport
/// pixels, top-left origin). Returns an empty `Vec` when the range is collapsed
/// or no frags match.
///
/// **Phase 1 limitation:** multi-node ranges (start and end in different text
/// nodes) only cover the start-node tail and end-node head; text nodes entirely
/// between start and end are not yet highlighted (requires document-order
/// traversal with range bookmarking).
pub fn selection_rects(
    root: &LayoutBox,
    range: &Range,
    measurer: &dyn TextMeasurer,
) -> Vec<Rect> {
    if range.is_collapsed() {
        return vec![];
    }
    let mut out = Vec::new();
    collect_selection_rects(root, range, measurer, &mut out);
    out
}

fn collect_selection_rects(
    b: &LayoutBox,
    range: &Range,
    measurer: &dyn TextMeasurer,
    out: &mut Vec<Rect>,
) {
    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        let line_h = b.style.font_size * b.style.line_height;
        for (line_idx, line) in lines.iter().enumerate() {
            let line_y = b.rect.y + line_idx as f32 * line_h;
            for frag in line {
                if frag.text.is_empty() {
                    continue;
                }
                let frag_end_byte = frag.source_char_offset + frag.text.len() as u32;
                let (px_start, px_end) = frag_selection_px(
                    frag,
                    frag.source_char_offset,
                    frag_end_byte,
                    range,
                    measurer,
                );
                if px_end > px_start {
                    out.push(Rect {
                        x: b.rect.x + frag.x + px_start,
                        y: line_y,
                        width: px_end - px_start,
                        height: line_h,
                    });
                }
            }
        }
    }
    for child in &b.children {
        collect_selection_rects(child, range, measurer, out);
    }
}

/// Compute the selected [px_start, px_end) x-range within a single frag.
///
/// Returns (0.0, 0.0) when the frag is not within the selection range.
fn frag_selection_px(
    frag: &InlineFrag,
    frag_start_byte: u32,
    frag_end_byte: u32,
    range: &Range,
    measurer: &dyn TextMeasurer,
) -> (f32, f32) {
    let font_size = frag.style.font_size;
    let text = &frag.text;

    let same_start = range.start.container == frag.source_node;
    let same_end = range.end.container == frag.source_node;

    // sel_byte_start / sel_byte_end: byte offsets within `frag.text`.
    let sel_start_in_frag = if same_start {
        // Clamp: range may start before this frag (if same node, different word)
        range.start.offset.max(frag_start_byte).min(frag_end_byte) - frag_start_byte
    } else if !same_start && !same_end {
        // Fully between start and end node — not handled in Phase 1
        return (0.0, 0.0);
    } else {
        0
    };

    let sel_end_in_frag = if same_end {
        range.end.offset.max(frag_start_byte).min(frag_end_byte) - frag_start_byte
    } else if same_start {
        // Range extends past this frag's node — select to end of frag
        frag.text.len() as u32
    } else {
        return (0.0, 0.0);
    };

    if sel_end_in_frag == 0 || sel_end_in_frag <= sel_start_in_frag {
        return (0.0, 0.0);
    }

    let px_start = x_at_byte(text, sel_start_in_frag as usize, font_size, measurer);
    let px_end = x_at_byte(text, sel_end_in_frag as usize, font_size, measurer);
    (px_start, px_end)
}

/// Compute x offset in pixels to the boundary before the character at `byte_offset`
/// within `text`. Uses `measurer` for per-character widths.
fn x_at_byte(text: &str, byte_offset: usize, font_size: f32, measurer: &dyn TextMeasurer) -> f32 {
    let mut acc = 0.0f32;
    let mut off = 0usize;
    for ch in text.chars() {
        if off >= byte_offset {
            break;
        }
        acc += measurer.char_width(ch, font_size);
        off += ch.len_utf8();
    }
    acc
}

/// Return the UTF-8 byte offset of the character boundary closest to `rel_x`
/// pixels from the start of `text`. Uses midpoint heuristic: snap to next
/// boundary if `rel_x` is past the midpoint of the current character.
fn byte_offset_at_x(
    text: &str,
    rel_x: f32,
    font_size: f32,
    measurer: &dyn TextMeasurer,
) -> u32 {
    let mut acc = 0.0f32;
    let mut off = 0usize;
    for ch in text.chars() {
        let w = measurer.char_width(ch, font_size);
        if rel_x < acc + w / 2.0 {
            return off as u32;
        }
        acc += w;
        off += ch.len_utf8();
    }
    off as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_dom::{NodeId, Selection};
    use lumen_core::geom::Rect;
    use crate::{BoxKind, InlineFrag, LayoutBox};
    use crate::style::ComputedStyle;

    struct Fixed10;
    impl TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    fn make_frag(text: &str, x: f32, source_node: NodeId, source_char_offset: u32) -> InlineFrag {
        let width = text.chars().count() as f32 * 10.0;
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
            source_char_offset,
        }
    }

    fn make_inline_run_box(rect: Rect, lines: Vec<Vec<InlineFrag>>) -> LayoutBox {
        let style = ComputedStyle::root();
        LayoutBox {
            node: NodeId::from_index(1),
            rect,
            style,
            kind: BoxKind::InlineRun {
                segments: vec![],
                lines,
                first_line_style: None,
            },
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
            dirty: Default::default(),
        }
    }

    // ── caret_at_point ────────────────────────────────────────────────────────

    #[test]
    fn caret_at_start_of_frag() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // Click at x=0 → before 'h' → offset 0
        let pos = caret_at_point(&b, 0.0, 5.0, &Fixed10).unwrap();
        assert_eq!(pos.container, node);
        assert_eq!(pos.offset, 0);
    }

    #[test]
    fn caret_after_first_char_midpoint() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // 'h' spans [0,10). Midpoint at 5. Click at 6 → after 'h' → offset 1
        let pos = caret_at_point(&b, 6.0, 5.0, &Fixed10).unwrap();
        assert_eq!(pos.container, node);
        assert_eq!(pos.offset, 1);
    }

    #[test]
    fn caret_at_end_of_frag() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hi", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // "hi" = 20px. Click at 25 → past end → offset 2
        let pos = caret_at_point(&b, 25.0, 5.0, &Fixed10).unwrap();
        assert_eq!(pos.container, node);
        assert_eq!(pos.offset, 2);
    }

    #[test]
    fn caret_uses_source_char_offset() {
        let node = NodeId::from_index(2);
        // Frag starts at byte 5 within the source node
        let frag = make_frag("world", 0.0, node, 5);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // Click at x=0 → offset = 5 + 0 = 5
        let pos = caret_at_point(&b, 0.0, 5.0, &Fixed10).unwrap();
        assert_eq!(pos.offset, 5);
    }

    #[test]
    fn caret_second_line() {
        let node = NodeId::from_index(2);
        let frag1 = make_frag("line1", 0.0, node, 0);
        let frag2 = make_frag("line2", 0.0, NodeId::from_index(3), 6);
        // line_h = 16 * 1.2 = 19.2; second line starts at y=19.2
        let b = make_inline_run_box(
            Rect { x: 0.0, y: 0.0, width: 200.0, height: 38.4 },
            vec![vec![frag1], vec![frag2]],
        );
        let pos = caret_at_point(&b, 0.0, 22.0, &Fixed10).unwrap();
        assert_eq!(pos.container, NodeId::from_index(3));
        assert_eq!(pos.offset, 6);
    }

    #[test]
    fn caret_returns_none_outside_all_text() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hi", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 100.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // y=5 is above the inline run
        assert!(caret_at_point(&b, 5.0, 5.0, &Fixed10).is_none());
    }

    #[test]
    fn caret_with_box_x_offset() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 0.0, node, 0);
        // Box starts at x=50; frag at x=0 within box
        let b = make_inline_run_box(Rect { x: 50.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // Absolute x=56 → rel_x=6 → past midpoint of 'h' → offset 1
        let pos = caret_at_point(&b, 56.0, 5.0, &Fixed10).unwrap();
        assert_eq!(pos.offset, 1);
    }

    #[test]
    fn caret_multibyte_utf8() {
        let node = NodeId::from_index(2);
        // "é" is 2 bytes in UTF-8; "ab" is 1 byte each
        let frag = make_frag("aéb", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // Click at x=15 → past 'a'(10) and past midpoint of 'é'(10+5=15, midpoint=15) → offset 1 (byte after 'a')
        // x=15 → acc at 'a'=0, w('a')=10, midpoint=5; x=15 >= 5 → advance; acc=10, 'é' w=10 midpoint=15; x=15 not < 15 → advance; byte_off = 1+2 = 3... let me recalculate:
        // 'a': acc=0, w=10, midpoint=5; rel_x=15 ≥ 5 → advance; off=1
        // 'é': acc=10, w=10, midpoint=15; rel_x=15 not < 15 → advance; off=3 (1+2)
        // 'b': acc=20, w=10, midpoint=25; rel_x=15 < 25 → return off=3
        let pos = caret_at_point(&b, 15.0, 5.0, &Fixed10).unwrap();
        assert_eq!(pos.offset, 3); // byte offset: a(1) + é(2) = 3
    }

    // ── selection_rects ───────────────────────────────────────────────────────

    #[test]
    fn selection_rects_empty_for_collapsed_range() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        let range = Range::collapsed(DomPosition { container: node, offset: 2 });
        assert!(selection_rects(&b, &range, &Fixed10).is_empty());
    }

    #[test]
    fn selection_rects_full_frag() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        let range = Range {
            start: DomPosition { container: node, offset: 0 },
            end: DomPosition { container: node, offset: 5 },
        };
        let rects = selection_rects(&b, &range, &Fixed10);
        assert_eq!(rects.len(), 1);
        assert!((rects[0].x - 0.0).abs() < 0.01);
        assert!((rects[0].width - 50.0).abs() < 0.01);
    }

    #[test]
    fn selection_rects_partial_frag() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 0.0, node, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        // Select "ell" = bytes 1..4 (10px each)
        let range = Range {
            start: DomPosition { container: node, offset: 1 },
            end: DomPosition { container: node, offset: 4 },
        };
        let rects = selection_rects(&b, &range, &Fixed10);
        assert_eq!(rects.len(), 1);
        assert!((rects[0].x - 10.0).abs() < 0.01, "x={}", rects[0].x);
        assert!((rects[0].width - 30.0).abs() < 0.01, "w={}", rects[0].width);
    }

    #[test]
    fn selection_rects_x_includes_box_offset() {
        let node = NodeId::from_index(2);
        let frag = make_frag("hello", 10.0, node, 0); // frag.x = 10
        let b = make_inline_run_box(Rect { x: 50.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        let range = Range {
            start: DomPosition { container: node, offset: 0 },
            end: DomPosition { container: node, offset: 5 },
        };
        let rects = selection_rects(&b, &range, &Fixed10);
        assert_eq!(rects.len(), 1);
        // x = box.x(50) + frag.x(10) + px_start(0) = 60
        assert!((rects[0].x - 60.0).abs() < 0.01, "x={}", rects[0].x);
    }

    #[test]
    fn selection_rects_two_lines() {
        let node = NodeId::from_index(2);
        let frag1 = make_frag("hello", 0.0, node, 0);
        let frag2 = make_frag("world", 0.0, node, 6);
        let b = make_inline_run_box(
            Rect { x: 0.0, y: 0.0, width: 200.0, height: 38.4 },
            vec![vec![frag1], vec![frag2]],
        );
        let range = Range {
            start: DomPosition { container: node, offset: 0 },
            end: DomPosition { container: node, offset: 11 }, // 6 + 5
        };
        let rects = selection_rects(&b, &range, &Fixed10);
        assert_eq!(rects.len(), 2, "both lines should be highlighted");
    }

    #[test]
    fn selection_rects_no_match_wrong_node() {
        let node = NodeId::from_index(2);
        let other = NodeId::from_index(3);
        let frag = make_frag("hello", 0.0, other, 0);
        let b = make_inline_run_box(Rect { x: 0.0, y: 0.0, width: 200.0, height: 19.2 }, vec![vec![frag]]);
        let range = Range {
            start: DomPosition { container: node, offset: 0 },
            end: DomPosition { container: node, offset: 5 },
        };
        assert!(selection_rects(&b, &range, &Fixed10).is_empty());
    }

    // ── Selection type ────────────────────────────────────────────────────────

    #[test]
    fn selection_default_is_empty() {
        let sel: Selection = Selection::default();
        assert!(sel.anchor.is_none());
        assert!(sel.focus.is_none());
        assert!(sel.is_collapsed());
        assert!(sel.get_range().is_none());
    }

    #[test]
    fn selection_collapse_sets_both_endpoints() {
        let mut sel = Selection::default();
        let node = NodeId::from_index(2);
        sel.collapse(DomPosition { container: node, offset: 3 });
        assert!(sel.is_collapsed());
        let r = sel.get_range().unwrap();
        assert_eq!(r.start.offset, 3);
        assert_eq!(r.end.offset, 3);
    }

    #[test]
    fn selection_extend_focus_creates_range() {
        let mut sel = Selection::default();
        let node = NodeId::from_index(2);
        sel.collapse(DomPosition { container: node, offset: 1 });
        sel.extend_focus(DomPosition { container: node, offset: 5 });
        assert!(!sel.is_collapsed());
        let r = sel.get_range().unwrap();
        assert_eq!(r.start.offset, 1);
        assert_eq!(r.end.offset, 5);
    }

    #[test]
    fn selection_extend_focus_backwards_normalises_range() {
        let mut sel = Selection::default();
        let node = NodeId::from_index(2);
        sel.collapse(DomPosition { container: node, offset: 5 });
        sel.extend_focus(DomPosition { container: node, offset: 1 });
        let r = sel.get_range().unwrap();
        // get_range normalises: start ≤ end (by offset, same node)
        assert_eq!(r.start.offset, 1);
        assert_eq!(r.end.offset, 5);
    }

    #[test]
    fn selection_clear_resets_all() {
        let mut sel = Selection::default();
        let node = NodeId::from_index(2);
        sel.collapse(DomPosition { container: node, offset: 3 });
        sel.clear();
        assert!(sel.is_collapsed());
        assert!(sel.anchor.is_none());
    }

    // ── Range type ────────────────────────────────────────────────────────────

    #[test]
    fn range_collapsed_is_same_start_end() {
        let node = NodeId::from_index(2);
        let pos = DomPosition { container: node, offset: 7 };
        let r = Range::collapsed(pos);
        assert!(r.is_collapsed());
        assert_eq!(r.start, r.end);
    }

    #[test]
    fn range_not_collapsed_when_different_offsets() {
        let node = NodeId::from_index(2);
        let r = Range {
            start: DomPosition { container: node, offset: 0 },
            end: DomPosition { container: node, offset: 1 },
        };
        assert!(!r.is_collapsed());
    }

    // ── byte_offset_at_x and x_at_byte ───────────────────────────────────────

    #[test]
    fn byte_offset_at_x_before_first_char() {
        // rel_x=0 → before midpoint of first char → offset 0
        assert_eq!(byte_offset_at_x("hello", 0.0, 16.0, &Fixed10), 0);
    }

    #[test]
    fn byte_offset_at_x_past_all_chars() {
        // rel_x=100 → past all 5 chars (50px) → offset 5
        assert_eq!(byte_offset_at_x("hello", 100.0, 16.0, &Fixed10), 5);
    }

    #[test]
    fn x_at_byte_zero_offset() {
        assert!((x_at_byte("hello", 0, 16.0, &Fixed10) - 0.0).abs() < 0.01);
    }

    #[test]
    fn x_at_byte_full_string() {
        assert!((x_at_byte("hello", 5, 16.0, &Fixed10) - 50.0).abs() < 0.01);
    }

    #[test]
    fn x_at_byte_mid_string() {
        // After "hel" = 30px
        assert!((x_at_byte("hello", 3, 16.0, &Fixed10) - 30.0).abs() < 0.01);
    }
}
