//! Per-text-node glyph geometry for point → text-offset hit testing.
//!
//! GAP-HLHITTEST: `CSS.highlights.highlightsFromPoint(x, y)` (CSS Custom
//! Highlight API L1 §5) has to know which character of which DOM text node
//! is painted under a viewport point, then ask whether a highlight's range
//! covers that character. [`crate::collect_client_rects`] only answers per
//! *element*; this module keeps the same `InlineRun` line walk but records
//! each `InlineFrag` (split per [`frag_source_spans`] where one fragment
//! merged several text nodes) against its source text node together with the
//! UTF-16 offset span (DOM offsets are UTF-16 code units) it came from.
//!
//! Character boxes inside one fragment are split in proportion to their
//! UTF-16 length, not measured glyph by glyph: the JS thread that consumes
//! this table has no `TextMeasurer` of the page's fonts. A fragment is at
//! most one word (`wrap_inline_run` splits on whitespace), so the error is
//! bounded by one word's glyph-width variance, and it is exact for
//! monospace text.

use std::collections::HashMap;

use lumen_dom::NodeId;

use crate::{BoxKind, InlineFrag, LayoutBox};

/// The part of one [`InlineFrag`] that came from a single DOM text node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FragSpan<'a> {
    /// The DOM text node these glyphs came from.
    pub source_node: NodeId,
    /// UTF-8 byte offset of `text[0]` within that node's content.
    pub source_char_offset: u32,
    /// This span's slice of `InlineFrag::text`, without the collapsed
    /// inter-node space that separates it from the next span.
    pub text: &'a str,
    /// Start x, relative to the fragment's own `x`.
    pub x: f32,
    /// Width of this span's glyphs.
    pub width: f32,
}

/// Split `frag` into one [`FragSpan`] per DOM text node — a single span for
/// an ordinary fragment, several for one `wrap_inline_run` merged across
/// same-style text nodes ([`InlineFrag::merged_sources`]).
pub fn frag_source_spans(frag: &InlineFrag) -> Vec<FragSpan<'_>> {
    let len = frag.text.len();
    let byte = |b: u32| {
        let mut b = (b as usize).min(len);
        while !frag.text.is_char_boundary(b) {
            b -= 1;
        }
        b
    };
    let mut out = Vec::with_capacity(1 + frag.merged_sources.len());
    let (mut node, mut off, mut start, mut x) = (frag.source_node, frag.source_char_offset, 0usize, 0.0f32);
    for m in &frag.merged_sources {
        let end = byte(m.text_byte).max(start);
        out.push(FragSpan {
            source_node: node,
            source_char_offset: off,
            text: frag.text[start..end].trim_end_matches(' '),
            x,
            width: (m.prev_end_x - x).max(0.0),
        });
        (node, off, start, x) = (m.source_node, m.source_char_offset, end, m.x);
    }
    out.push(FragSpan {
        source_node: node,
        source_char_offset: off,
        text: &frag.text[start..],
        x,
        width: (frag.width - x).max(0.0),
    });
    out
}

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
                    for span in frag_source_spans(frag) {
                        let lumen_dom::NodeData::Text(source) = &doc.get(span.source_node).data else {
                            continue;
                        };
                        let byte_start = span.source_char_offset as usize;
                        let byte_end = byte_start.saturating_add(span.text.len());
                        let start = utf16_offset_of_byte(source, byte_start);
                        let end = utf16_offset_of_byte(source, byte_end);
                        if end <= start {
                            continue;
                        }
                        out.entry(span.source_node.index() as u32).or_default().push(TextFragRect {
                            rect: [b.rect.x + frag.x + span.x, line_y, span.width, line_h],
                            start,
                            end,
                        });
                    }
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
