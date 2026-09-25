//! Per-text-node glyph geometry for point → text-offset hit testing.
//!
//! GAP-HLHITTEST: `CSS.highlights.highlightsFromPoint(x, y)` (CSS Custom
//! Highlight API L1 §5) has to know which character of which DOM text node
//! is painted under a viewport point, then ask whether a highlight's range
//! covers that character. [`crate::collect_client_rects`] only answers per
//! *element*; this module keeps the same `InlineRun` line walk but records
//! each `InlineFrag` against its source text node together with the
//! UTF-16 offset span (DOM offsets are UTF-16 code units) it came from.
//!
//! Character boxes inside one fragment are split in proportion to their
//! UTF-16 length, not measured glyph by glyph: the JS thread that consumes
//! this table has no `TextMeasurer` of the page's fonts. A fragment is at
//! most one word (`wrap_inline_run` splits on whitespace), so the error is
//! bounded by one word's glyph-width variance, and it is exact for
//! monospace text.

use std::collections::HashMap;

use crate::{BoxKind, LayoutBox};

/// One laid-out fragment of a DOM text node, in viewport-relative CSS px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFragRect {
    /// `[x, y, width, height]` — same frame and line-height rule as
    /// `getBoundingClientRect()` (`LayoutBox::used_line_height` per line).
    pub rect: [f32; 4],
    /// UTF-16 offset of the fragment's first code unit in the text node.
    pub start: u32,
    /// UTF-16 offset one past the fragment's last code unit.
    pub end: u32,
}

/// Map `text node index → fragments in line order` for every text node that
/// produced at least one non-empty `InlineFrag`.
pub fn collect_text_frag_rects(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
) -> HashMap<u32, Vec<TextFragRect>> {
    let mut out: HashMap<u32, Vec<TextFragRect>> = HashMap::new();
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        if let BoxKind::InlineRun { lines, .. } = &b.kind {
            let line_h = b.used_line_height;
            for (line_idx, line) in lines.iter().enumerate() {
                let line_y = b.rect.y + line_idx as f32 * line_h;
                for frag in line {
                    if frag.text.is_empty() || frag.img_src.is_some() {
                        continue;
                    }
                    let lumen_dom::NodeData::Text(source) = &doc.get(frag.source_node).data else {
                        continue;
                    };
                    let byte_start = frag.source_char_offset as usize;
                    let byte_end = byte_start.saturating_add(frag.text.len());
                    let start = utf16_offset_of_byte(source, byte_start);
                    let end = utf16_offset_of_byte(source, byte_end);
                    if end <= start {
                        continue;
                    }
                    out.entry(frag.source_node.index() as u32).or_default().push(TextFragRect {
                        rect: [b.rect.x + frag.x, line_y, frag.width, line_h],
                        start,
                        end,
                    });
                }
            }
        }
        stack.extend(b.children.iter().rev());
    }
    out
}

/// Every `(text node index, UTF-16 offset)` whose character box contains
/// `(x, y)`. The character at offset `i` spans `[i, i + 1)`; a point on the
/// left edge of a box belongs to that box, on the right edge to the next.
/// Usually zero or one hit; overlapping content (a negative margin, an
/// absolutely positioned run over flow text) yields one per text node.
pub fn text_hits_at_point(
    table: &HashMap<u32, Vec<TextFragRect>>,
    x: f32,
    y: f32,
) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for (&nid, frags) in table {
        for f in frags {
            let [fx, fy, fw, fh] = f.rect;
            if fw <= 0.0 || x < fx || x >= fx + fw || y < fy || y >= fy + fh {
                continue;
            }
            let units = f.end - f.start;
            let idx = (((x - fx) / fw) * units as f32) as u32;
            out.push((nid, f.start + idx.min(units - 1)));
            break;
        }
    }
    out.sort_unstable();
    out
}

/// UTF-16 length of `s[..byte]`, with `byte` clamped to `s.len()` and moved
/// back to the nearest char boundary — `source_char_offset` is a best-effort
/// mapping (whitespace collapsing and `text-transform` can shift it).
fn utf16_offset_of_byte(s: &str, byte: usize) -> u32 {
    let mut b = byte.min(s.len());
    while !s.is_char_boundary(b) {
        b -= 1;
    }
    s[..b].encode_utf16().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(rect: [f32; 4], start: u32, end: u32) -> TextFragRect {
        TextFragRect { rect, start, end }
    }

    #[test]
    fn utf16_offset_clamps_and_snaps_to_char_boundary() {
        assert_eq!(utf16_offset_of_byte("abc", 2), 2);
        assert_eq!(utf16_offset_of_byte("abc", 99), 3);
        // 'я' is 2 UTF-8 bytes, 1 UTF-16 unit; byte 1 is mid-char.
        assert_eq!(utf16_offset_of_byte("яb", 1), 0);
        assert_eq!(utf16_offset_of_byte("яb", 2), 1);
        // U+1F600 is 4 UTF-8 bytes, 2 UTF-16 units.
        assert_eq!(utf16_offset_of_byte("\u{1F600}x", 4), 2);
    }

    #[test]
    fn hit_maps_x_to_character_index_proportionally() {
        let mut t = HashMap::new();
        t.insert(7, vec![frag([10.0, 0.0, 100.0, 20.0], 0, 10)]);
        assert_eq!(text_hits_at_point(&t, 10.0, 5.0), vec![(7, 0)]);
        assert_eq!(text_hits_at_point(&t, 40.0, 5.0), vec![(7, 3)]);
        assert_eq!(text_hits_at_point(&t, 109.9, 5.0), vec![(7, 9)]);
    }

    #[test]
    fn misses_outside_fragment_boxes() {
        let mut t = HashMap::new();
        t.insert(7, vec![frag([10.0, 0.0, 100.0, 20.0], 0, 10)]);
        assert!(text_hits_at_point(&t, 9.0, 5.0).is_empty());
        assert!(text_hits_at_point(&t, 110.0, 5.0).is_empty());
        assert!(text_hits_at_point(&t, 50.0, 20.0).is_empty());
        assert!(text_hits_at_point(&t, 50.0, -1.0).is_empty());
    }

    #[test]
    fn second_line_fragment_keeps_its_own_offset_span() {
        let mut t = HashMap::new();
        t.insert(3, vec![
            frag([0.0, 0.0, 40.0, 20.0], 0, 4),
            frag([0.0, 20.0, 50.0, 20.0], 5, 10),
        ]);
        assert_eq!(text_hits_at_point(&t, 15.0, 30.0), vec![(3, 6)]);
    }
}
