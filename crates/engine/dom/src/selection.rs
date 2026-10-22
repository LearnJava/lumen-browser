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

/// Number of UTF-16 code units in `s` — the `length` of a `CharacterData`
/// node (DOM §4.10) and the unit every text offset of the Web APIs counts.
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Byte offset of the UTF-16 code unit `units` inside `s`.
///
/// The engine stores text positions as UTF-8 byte offsets (the layout caret
/// and the hit-test work in bytes), while the Web APIs count UTF-16 code
/// units (DOM §4.4) — this is the boundary between the two.
///
/// An offset past the end clamps to `s.len()`; an offset that would land
/// inside a surrogate pair rounds down to the start of that character, so a
/// slice never splits it.
pub fn utf16_to_byte_offset(s: &str, units: usize) -> usize {
    let mut seen = 0usize;
    for (byte, ch) in s.char_indices() {
        if seen + ch.len_utf16() > units {
            return byte;
        }
        seen += ch.len_utf16();
    }
    s.len()
}

/// UTF-16 code-unit offset of the byte offset `bytes` inside `s` — the
/// inverse of [`utf16_to_byte_offset`].
///
/// A byte offset inside a character rounds down to the character's start.
pub fn byte_to_utf16_offset(s: &str, bytes: usize) -> usize {
    let bytes = bytes.min(s.len());
    let mut seen = 0usize;
    for (byte, ch) in s.char_indices() {
        if byte + ch.len_utf8() > bytes {
            return seen;
        }
        seen += ch.len_utf16();
    }
    seen
}

/// DOM-spec "length" of `node`: UTF-16 code-unit count for text nodes, child
/// count for element/document nodes.
///
/// Used to clamp Range endpoint offsets to valid values.
pub fn node_length(doc: &Document, node: NodeId) -> usize {
    match &doc.get(node).data {
        NodeData::Text(s) | NodeData::Comment(s) => utf16_len(s),
        NodeData::ProcessingInstruction { data, .. } => utf16_len(data),
        _ => doc.get(node).children.len(),
    }
}

/// Extracts the text covered by `range` (WHATWG DOM §4.6 `stringification`).
///
/// Same-container ranges: simple substring.
/// Cross-container ranges: walks node indices (arena insertion order ≈ document
/// order for parsed documents; valid for typical selection scenarios).
pub fn range_text(doc: &Document, range: &Range) -> String {
    range_text_filtered(doc, range, &|_| false)
}

/// [`range_text`] that leaves out the text nodes for which `skip` returns
/// `true`.
///
/// The DOM has no styles, so «which text is not selectable» is supplied by the
/// caller: the shell collects the nodes whose computed `user-select` is `none`
/// from the layout tree (`lumen_layout::user_select_none_text_nodes`) and
/// passes them here, so copying a selection drops that text exactly as the
/// highlight does (CSS UI L4 §6.2).
pub fn range_text_filtered(doc: &Document, range: &Range, skip: &dyn Fn(NodeId) -> bool) -> String {
    if range.is_collapsed() {
        return String::new();
    }

    let start = range.start;
    let end = range.end;

    // Fast path: shared container
    if start.container == end.container {
        if let NodeData::Text(s) = &doc.get(start.container).data {
            if skip(start.container) {
                return String::new();
            }
            let from = utf8_floor(s, start.offset as usize);
            let to = utf8_floor(s, end.offset as usize);
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            return s[from..to].to_string();
        }
        // Non-CharacterData container (e.g. an Element after `selectAllChildren`):
        // offsets are DOM-spec child-indices, not byte offsets.
        let children = &doc.get(start.container).children;
        let (from, to) = if start.offset <= end.offset {
            (start.offset as usize, end.offset as usize)
        } else {
            (end.offset as usize, start.offset as usize)
        };
        let to = to.min(children.len());
        let mut out = String::new();
        for &child in children.iter().take(to).skip(from) {
            collect_text_filtered(doc, child, skip, &mut out);
        }
        return out;
    }

    let (first, last) = if start.container.index() < end.container.index() {
        (start, end)
    } else {
        (end, start)
    };

    let mut out = String::new();
    for idx in first.container.index()..=last.container.index() {
        // `Document::node_id_at`, not `NodeId::from_index`: this must carry
        // the slot's real generation, or the `nid == first.container`/
        // `nid == last.container` checks below silently fail once either
        // endpoint's slot has been freed and reused at least once (срез 9,
        // GAP-P3GCJSDOM), losing the offset-trim for that endpoint.
        let nid = doc.node_id_at(idx);
        if let NodeData::Text(s) = &doc.get(nid).data {
            if skip(nid) {
                continue;
            }
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

/// Appends the text of `node` (a text node, or an element's whole subtree)
/// minus the text nodes `skip` rejects.
fn collect_text_filtered(doc: &Document, node: NodeId, skip: &dyn Fn(NodeId) -> bool, out: &mut String) {
    match &doc.get(node).data {
        NodeData::Text(s) => {
            if !skip(node) {
                out.push_str(s);
            }
        }
        NodeData::Element { .. } => {
            for &child in &doc.get(node).children {
                collect_text_filtered(doc, child, skip, out);
            }
        }
        _ => {}
    }
}

/// The word around byte `offset` of `text` — the double-click selection, as a
/// byte range `(from, to)`.
///
/// A run of letters, digits, `_` and combining marks is one word; a run of
/// whitespace is selected as a unit; any other character (punctuation) stands
/// alone. At a boundary the character after `offset` decides, at the end of
/// the text the one before it. `None` for empty text.
pub fn word_bounds(text: &str, offset: usize) -> Option<(usize, usize)> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Space,
        Other,
    }
    fn class(c: char) -> Class {
        if c.is_alphanumeric() || c == '_' || is_combining_mark(c) {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    }
    /// Combining diacritics blocks (enough for a word not to split on `é`
    /// written as `e` + U+0301).
    fn is_combining_mark(c: char) -> bool {
        matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF)
    }
    let at = utf8_floor(text, offset);
    let seed = text[at..].chars().next().or_else(|| text[..at].chars().next_back())?;
    let seed_start = if at < text.len() { at } else { at - seed.len_utf8() };
    let kind = class(seed);
    let (mut from, mut to) = (seed_start, seed_start + seed.len_utf8());
    if kind != Class::Other {
        while let Some(c) = text[..from].chars().next_back() {
            if class(c) != kind {
                break;
            }
            from -= c.len_utf8();
        }
        while let Some(c) = text[to..].chars().next() {
            if class(c) != kind {
                break;
            }
            to += c.len_utf8();
        }
    }
    Some((from, to))
}

/// The word under a text position as a [`Range`] ([`word_bounds`] on the text
/// node). `None` for a non-text container or empty text; the word never
/// leaves its text node.
pub fn word_range_at(doc: &Document, pos: DomPosition) -> Option<Range> {
    let NodeData::Text(s) = &doc.get(pos.container).data else { return None };
    let (from, to) = word_bounds(s, pos.offset as usize)?;
    Some(Range {
        start: DomPosition { container: pos.container, offset: from as u32 },
        end: DomPosition { container: pos.container, offset: to as u32 },
    })
}

fn utf8_floor(s: &str, mut off: usize) -> usize {
    off = off.min(s.len());
    while off > 0 && !s.is_char_boundary(off) {
        off -= 1;
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QualName;

    fn word_at(text: &str, offset: u32) -> Option<String> {
        let mut doc = Document::new();
        let root = doc.root();
        let t = doc.create_text(text);
        doc.append_child(root, t);
        let r = word_range_at(&doc, DomPosition { container: t, offset })?;
        Some(range_text(&doc, &r))
    }

    #[test]
    fn word_range_picks_the_word_under_the_caret() {
        assert_eq!(word_at("hello brave world", 1).as_deref(), Some("hello"));
        assert_eq!(word_at("hello brave world", 8).as_deref(), Some("brave"));
        // Boundary: the character after the caret decides.
        assert_eq!(word_at("hello brave", 6).as_deref(), Some("brave"));
        assert_eq!(word_at("hello brave", 5).as_deref(), Some(" "));
        // End of text: the character before.
        assert_eq!(word_at("hello brave", 11).as_deref(), Some("brave"));
    }

    #[test]
    fn word_range_handles_punctuation_cyrillic_and_underscore() {
        assert_eq!(word_at("a, b", 1).as_deref(), Some(","));
        assert_eq!(word_at("привет, мир", 3).as_deref(), Some("привет"));
        assert_eq!(word_at("snake_case x", 2).as_deref(), Some("snake_case"));
        assert_eq!(word_at("a   b", 2).as_deref(), Some("   "));
        assert_eq!(word_at("", 0), None);
        // An offset inside a multi-byte char snaps back to its start.
        assert_eq!(word_at("мир", 1).as_deref(), Some("мир"));
    }

    // ── BUG-620: Element-container Range (selectAllChildren) must stringify ──

    #[test]
    fn range_text_filtered_drops_skipped_text_nodes() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_text("alpha ");
        let b = doc.create_text("beta ");
        let c = doc.create_text("gamma");
        doc.append_child(root, a);
        doc.append_child(root, b);
        doc.append_child(root, c);
        let range = Range {
            start: DomPosition { container: a, offset: 2 },
            end: DomPosition { container: c, offset: 3 },
        };
        assert_eq!(range_text(&doc, &range), "pha beta gam");
        assert_eq!(range_text_filtered(&doc, &range, &|n| n == b), "pha gam");
        // A fully skipped single-node range yields nothing.
        let single = Range {
            start: DomPosition { container: b, offset: 0 },
            end: DomPosition { container: b, offset: 4 },
        };
        assert_eq!(range_text_filtered(&doc, &single, &|n| n == b), "");
    }

    #[test]
    fn range_text_same_text_container_still_works() {
        let mut doc = Document::new();
        let text = doc.create_text("hello selection text");
        doc.append_child(doc.root(), text);
        let range = Range {
            start: DomPosition { container: text, offset: 0 },
            end: DomPosition { container: text, offset: 5 },
        };
        assert_eq!(range_text(&doc, &range), "hello");
    }

    #[test]
    fn range_text_element_container_child_index_offsets() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let text = doc.create_text("hello\nselection text");
        doc.append_child(div, text);
        // Mirrors `selectAllChildren(div)`: both endpoints share `div` as the
        // container, offsets are child-indices (0 and node_length(div)).
        let range = Range {
            start: DomPosition { container: div, offset: 0 },
            end: DomPosition { container: div, offset: node_length(&doc, div) as u32 },
        };
        assert_eq!(range_text(&doc, &range), "hello\nselection text");
    }

    #[test]
    fn range_text_element_container_nested_elements() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let span = doc.create_element(QualName::html("span"));
        doc.append_child(div, span);
        let inner = doc.create_text("world");
        doc.append_child(span, inner);
        let range = Range {
            start: DomPosition { container: div, offset: 0 },
            end: DomPosition { container: div, offset: node_length(&doc, div) as u32 },
        };
        assert_eq!(range_text(&doc, &range), "world");
    }

    // ── UTF-16 ↔ UTF-8 byte offsets (BUG-1464) ────────────────────────────

    #[test]
    fn utf16_len_counts_code_units_not_chars() {
        assert_eq!(utf16_len("abc"), 3);
        // "я" — one char, one code unit; "😀" — one char, two code units.
        assert_eq!(utf16_len("я"), 1);
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!(utf16_len("😀a"), 3);
    }

    #[test]
    fn utf16_to_byte_offset_cuts_on_code_units() {
        assert_eq!(utf16_to_byte_offset("abc", 0), 0);
        assert_eq!(utf16_to_byte_offset("abc", 2), 2);
        assert_eq!(utf16_to_byte_offset("abc", 3), 3);
        // Past the end clamps to the end of the string.
        assert_eq!(utf16_to_byte_offset("abc", 9), 3);
        // "привет" — two bytes per letter.
        assert_eq!(utf16_to_byte_offset("привет", 1), 2);
        assert_eq!(utf16_to_byte_offset("привет", 6), 12);
        // "😀a" — the emoji takes two code units and four bytes.
        assert_eq!(utf16_to_byte_offset("😀a", 0), 0);
        assert_eq!(utf16_to_byte_offset("😀a", 2), 4);
        assert_eq!(utf16_to_byte_offset("😀a", 3), 5);
    }

    #[test]
    fn utf16_to_byte_offset_never_splits_a_character() {
        // An offset inside a surrogate pair rounds down to the pair's start.
        assert_eq!(utf16_to_byte_offset("😀a", 1), 0);
        // A BMP letter is one code unit, so unit 1 is the boundary right
        // after it — no letter is ever cut in half.
        assert_eq!(utf16_to_byte_offset("яb", 1), 2);
    }

    #[test]
    fn byte_to_utf16_offset_is_the_inverse() {
        assert_eq!(byte_to_utf16_offset("abc", 0), 0);
        assert_eq!(byte_to_utf16_offset("abc", 2), 2);
        assert_eq!(byte_to_utf16_offset("abc", 3), 3);
        assert_eq!(byte_to_utf16_offset("abc", 99), 3);
        assert_eq!(byte_to_utf16_offset("привет", 2), 1);
        assert_eq!(byte_to_utf16_offset("привет", 12), 6);
        assert_eq!(byte_to_utf16_offset("😀a", 4), 2);
        // A byte offset inside a character rounds down to its start.
        assert_eq!(byte_to_utf16_offset("яb", 1), 0);
        assert_eq!(byte_to_utf16_offset("😀a", 2), 0);
    }

    #[test]
    fn node_length_of_text_is_utf16_units() {
        let mut doc = Document::new();
        let text = doc.create_text("😀a");
        doc.append_child(doc.root(), text);
        assert_eq!(node_length(&doc, text), 3);
    }
}
