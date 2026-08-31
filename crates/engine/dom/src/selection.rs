use super::*;

/// A position within the document (WHATWG DOM §4.4).
///
/// For `NodeData::Text` nodes `offset` is a UTF-8 byte offset within the
/// text content. For element nodes it is a child index. Use
/// [`Document::get_selection`] / [`Document::set_selection`] to persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomPosition {
    /// The node that contains this position.
    pub container: NodeId,
    /// Byte offset within the text content (text nodes) or child index
    /// (element nodes).
    pub offset: u32,
}

/// A contiguous range of document content (WHATWG DOM §4.5).
///
/// `start` must precede `end` in tree order. For a collapsed range
/// `start == end`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// First position (inclusive).
    pub start: DomPosition,
    /// Last position (exclusive).
    pub end: DomPosition,
}

impl Range {
    /// Collapsed range: both endpoints at `pos`.
    pub fn collapsed(pos: DomPosition) -> Self {
        Self { start: pos, end: pos }
    }

    /// True when start and end are the same position.
    pub fn is_collapsed(&self) -> bool {
        self.start == self.end
    }
}

/// The current document text selection (WHATWG Selection API).
///
/// Tracks anchor (mousedown) and focus (mousemove/mouseup). The selection
/// range is `min(anchor, focus)..=max(anchor, focus)` in document order.
///
/// `anchor` and `focus` are `None` when there is no active selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// Fixed start of the selection (where the user pressed the mouse button).
    pub anchor: Option<DomPosition>,
    /// Moving end of the selection (where the user released / dragged to).
    pub focus: Option<DomPosition>,
}

impl Selection {
    /// True when anchor == focus (or no selection).
    pub fn is_collapsed(&self) -> bool {
        match (&self.anchor, &self.focus) {
            (Some(a), Some(f)) => a == f,
            _ => true,
        }
    }

    /// The selection as a normalised Range (start ≤ end in node order).
    /// Returns `None` when there is no selection.
    pub fn get_range(&self) -> Option<Range> {
        let a = self.anchor?;
        let f = self.focus?;
        // Normalise so start is the position with the lower container index
        // or lower offset within the same container.
        if a.container.index() < f.container.index()
            || (a.container == f.container && a.offset <= f.offset)
        {
            Some(Range { start: a, end: f })
        } else {
            Some(Range { start: f, end: a })
        }
    }

    /// Collapse the selection to a single point.
    pub fn collapse(&mut self, pos: DomPosition) {
        self.anchor = Some(pos);
        self.focus = Some(pos);
    }

    /// Extend the focus end to `pos` (anchor stays fixed).
    pub fn extend_focus(&mut self, pos: DomPosition) {
        self.focus = Some(pos);
    }

    /// Remove the selection entirely.
    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
    }
}

/// Split a text node at `byte_offset`, creating a second text node with the
/// suffix `[byte_offset..]` and inserting it immediately after the original.
///
/// Returns the `NodeId` of the newly created second node.
/// If `byte_offset == 0` the first node becomes empty and all content moves to
/// the second. If `byte_offset >= content.len()` the first node is unchanged
/// and the second node is empty.
///
/// The caller is responsible for ensuring that `byte_offset` falls on a UTF-8
/// character boundary; if not, the offset is rounded down to the nearest
/// boundary to avoid producing invalid UTF-8.
pub fn split_text_node(doc: &mut Document, node: NodeId, byte_offset: u32) -> NodeId {
    let content = match &doc.get(node).data {
        NodeData::Text(s) => s.clone(),
        _ => return node, // not a text node — no-op, return self
    };

    // Clamp to a valid UTF-8 boundary.
    let offset = byte_offset as usize;
    let offset = if offset >= content.len() {
        content.len()
    } else {
        // Walk back to a char boundary.
        let mut b = offset;
        while b > 0 && !content.is_char_boundary(b) {
            b -= 1;
        }
        b
    };

    let first_part = content[..offset].to_string();
    let second_part = content[offset..].to_string();

    // Mutate the first (original) node in-place.
    if let NodeData::Text(s) = &mut doc.get_mut(node).data {
        *s = first_part;
    }

    // Allocate the second node and wire it into the parent.
    let second = doc.create_text(second_part);
    doc.insert_after(node, second);
    second
}

/// Insert `text` into the text node at `pos`, returning the caret position
/// immediately after the inserted text.
///
/// `pos.container` must point to a `NodeData::Text` node. If it points to an
/// element instead, the function tries to use the first text-node child; if
/// none exists it creates one and appends it.
///
/// `pos.offset` is a UTF-8 byte offset within the text content. If it exceeds
/// the content length it is clamped to the end.
pub fn insert_text_at(doc: &mut Document, pos: DomPosition, text: &str) -> DomPosition {
    if text.is_empty() {
        return pos;
    }

    // Resolve container to a text node.
    let text_node = match &doc.get(pos.container).data {
        NodeData::Text(_) => pos.container,
        NodeData::Element { .. } | NodeData::DocumentFragment => {
            // Find existing first text child or create one.
            let first_text = doc.get(pos.container).children.iter().copied().find(|&c| {
                matches!(doc.get(c).data, NodeData::Text(_))
            });
            match first_text {
                Some(id) => id,
                None => {
                    let new_text = doc.create_text("");
                    doc.append_child(pos.container, new_text);
                    new_text
                }
            }
        }
        _ => return pos,
    };

    let content = match &doc.get(text_node).data {
        NodeData::Text(s) => s.clone(),
        _ => return pos,
    };

    let offset = pos.offset as usize;
    let offset = offset.min(content.len());
    // Snap to UTF-8 boundary.
    let mut byte_off = offset;
    while byte_off > 0 && !content.is_char_boundary(byte_off) {
        byte_off -= 1;
    }

    let mut new_content = String::with_capacity(content.len() + text.len());
    new_content.push_str(&content[..byte_off]);
    new_content.push_str(text);
    new_content.push_str(&content[byte_off..]);

    let new_offset = (byte_off + text.len()) as u32;
    if let NodeData::Text(s) = &mut doc.get_mut(text_node).data {
        *s = new_content;
    }

    DomPosition { container: text_node, offset: new_offset }
}

/// Delete the content of `range` from the document, returning a collapsed
/// `DomPosition` at the start of the deleted range.
///
/// Only same-container deletions are supported (both endpoints in the same
/// text node). If `range.is_collapsed()` the function is a no-op.
/// Cross-node ranges are not yet implemented and return the start position
/// unchanged.
pub fn delete_range(doc: &mut Document, range: &Range) -> DomPosition {
    if range.is_collapsed() {
        return range.start;
    }

    // Only handle same-container for now.
    if range.start.container != range.end.container {
        return range.start;
    }

    let container = range.start.container;
    let content = match &doc.get(container).data {
        NodeData::Text(s) => s.clone(),
        _ => return range.start,
    };

    let start = (range.start.offset as usize).min(content.len());
    let end = (range.end.offset as usize).min(content.len());
    let (start, end) = if start <= end { (start, end) } else { (end, start) };

    // Snap both offsets to UTF-8 boundaries.
    let mut s = start;
    while s > 0 && !content.is_char_boundary(s) {
        s -= 1;
    }
    let mut e = end;
    while e > 0 && !content.is_char_boundary(e) {
        e -= 1;
    }

    let mut new_content = String::with_capacity(content.len() - (e - s));
    new_content.push_str(&content[..s]);
    new_content.push_str(&content[e..]);

    if let NodeData::Text(c) = &mut doc.get_mut(container).data {
        *c = new_content;
    }

    DomPosition { container, offset: s as u32 }
}

/// Insert a paragraph break (Enter key) at `pos` inside the `host`
/// contenteditable element.
///
/// Splits the text node at `pos` and inserts a `<br>` element immediately after
/// the split point. Returns a `DomPosition` at the start of the content after
/// the break (i.e. offset 0 into the second part of the split text node).
///
/// If `pos.container` is not a text node, a `<br>` is appended to `host`
/// directly and the position returned points to an empty text node after it.
///
/// `host` — the `contenteditable` root element (used as the insertion
/// container when `pos.container` has no parent or is not a text node).
// CSS: line-height, block formatting context for <p> splitting
pub fn insert_paragraph_break(doc: &mut Document, pos: DomPosition, host: NodeId) -> DomPosition {
    let is_text = matches!(doc.get(pos.container).data, NodeData::Text(_));

    if is_text {
        // Split text node at pos.
        let second = split_text_node(doc, pos.container, pos.offset);

        // Insert <br> between the two halves.
        let br = doc.create_element(QualName::html("br"));
        doc.insert_after(pos.container, br);
        // Move second text node after <br>.
        doc.insert_after(br, second);

        DomPosition { container: second, offset: 0 }
    } else {
        // Fallback: just append a <br> and an empty text node to host.
        let br = doc.create_element(QualName::html("br"));
        doc.append_child(host, br);
        let empty = doc.create_text("");
        doc.append_child(host, empty);
        DomPosition { container: empty, offset: 0 }
    }
}

// ── Selection API helpers (used by lumen-js bindings) ──────────────────────

/// Returns the full text content of `node` — concatenation of all descendant text nodes.
///
/// Equivalent to `node.textContent` for element and document-fragment nodes.
pub fn node_text_content(doc: &Document, node: NodeId) -> String {
    let mut out = String::new();
    dom_collect_text(doc, node, &mut out);
    out
}

/// Locate the text node and local byte range covering `[start, end)` of
/// `node`'s `textContent` (same traversal as [`node_text_content`]).
///
/// Returns `None` when the range crosses a text-node boundary (the target
/// text spans two adjacent nodes) or falls outside the content — callers
/// should treat that as "can't safely edit in place" and skip the edit rather
/// than guess.
pub fn locate_text_offset_range(
    doc: &Document,
    node: NodeId,
    start: usize,
    end: usize,
) -> Option<(NodeId, u32, u32)> {
    fn walk(
        doc: &Document,
        node: NodeId,
        cursor: &mut usize,
        start: usize,
        end: usize,
    ) -> Option<(NodeId, u32, u32)> {
        for &child in &doc.get(node).children {
            match &doc.get(child).data {
                NodeData::Text(s) => {
                    let node_start = *cursor;
                    let node_end = node_start + s.len();
                    *cursor = node_end;
                    if start >= node_start && end <= node_end {
                        return Some((
                            child,
                            (start - node_start) as u32,
                            (end - node_start) as u32,
                        ));
                    }
                }
                NodeData::Element { .. } => {
                    if let Some(found) = walk(doc, child, cursor, start, end) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let mut cursor = 0usize;
    walk(doc, node, &mut cursor, start, end)
}

/// Number of direct DOM children of `node`.
///
/// For text nodes this is always 0. Used to validate child-index offsets in Range.
pub fn node_child_count(doc: &Document, node: NodeId) -> usize {
    doc.get(node).children.len()
}

/// DOM-spec "length" of `node`: UTF-16 code-unit count for text nodes, child
/// count for element/document nodes.
///
/// Approximated as Rust `char` count (correct for BMP text).
/// Used to clamp Range endpoint offsets to valid values.
pub fn node_length(doc: &Document, node: NodeId) -> usize {
    match &doc.get(node).data {
        NodeData::Text(s) => s.chars().count(),
        _ => doc.get(node).children.len(),
    }
}

/// Extracts the text covered by `range` (WHATWG DOM §4.6 `stringification`).
///
/// Same-container ranges: simple substring.
/// Cross-container ranges: walks node indices (arena insertion order ≈ document
/// order for parsed documents; valid for typical selection scenarios).
pub fn range_text(doc: &Document, range: &Range) -> String {
    if range.is_collapsed() {
        return String::new();
    }

    let start = range.start;
    let end = range.end;

    // Fast path: single text node
    if start.container == end.container {
        if let NodeData::Text(s) = &doc.get(start.container).data {
            let from = utf8_floor(s, start.offset as usize);
            let to = utf8_floor(s, end.offset as usize);
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            return s[from..to].to_string();
        }
        return String::new();
    }

    let (first, last) = if start.container.index() < end.container.index() {
        (start, end)
    } else {
        (end, start)
    };

    let mut out = String::new();
    for idx in first.container.index()..=last.container.index() {
        let nid = NodeId::from_index(idx);
        if let NodeData::Text(s) = &doc.get(nid).data {
            if nid == first.container {
                let off = utf8_floor(s, first.offset as usize);
                out.push_str(&s[off..]);
            } else if nid == last.container {
                let off = utf8_floor(s, last.offset as usize);
                out.push_str(&s[..off]);
            } else {
                out.push_str(s);
            }
        }
    }
    out
}

fn utf8_floor(s: &str, mut off: usize) -> usize {
    off = off.min(s.len());
    while off > 0 && !s.is_char_boundary(off) {
        off -= 1;
    }
    off
}
