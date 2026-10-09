//! UX-SELECTION-API: keeping the shell's caret and the script-visible text
//! selection (`selectionStart`/`selectionEnd`/`setSelectionRange()`) one thing.
//!
//! The shell keeps the caret as char indices in [`FormControlState`]; scripts
//! see UTF-16 offsets held in the document ([`lumen_dom::TextSelection`]).
//! [`Lumen::publish_text_selection_for`] mirrors the caret into the document
//! after every shell-side change, [`Lumen::absorb_script_selection`] applies a
//! selection a script wrote back onto the caret. Iframe fields are not
//! mirrored: their shell state lives in separate per-frame maps.

use crate::*;

/// Char index → UTF-16 offset within `s`.
pub(crate) fn char_to_utf16(s: &str, idx: usize) -> u32 {
    s.chars().take(idx).map(|c| c.len_utf16() as u32).sum()
}

/// UTF-16 offset → char index within `s`; an offset inside a surrogate pair
/// rounds down to the pair's start.
pub(crate) fn utf16_to_char(s: &str, off: u32) -> usize {
    let mut units = 0u32;
    for (i, c) in s.chars().enumerate() {
        let next = units + c.len_utf16() as u32;
        if next > off {
            return i;
        }
        units = next;
    }
    s.chars().count()
}

/// `(start, end, dir)` in UTF-16 units for a caret/anchor pair over `value`.
pub(crate) fn selection_triple(value: &str, cursor: usize, anchor: Option<usize>) -> (u32, u32, u8) {
    let len = value.chars().count();
    let cursor = cursor.min(len);
    match anchor.map(|a| a.min(len)).filter(|&a| a != cursor) {
        None => {
            let c = char_to_utf16(value, cursor);
            (c, c, 0)
        }
        Some(a) if a < cursor => (char_to_utf16(value, a), char_to_utf16(value, cursor), 1),
        Some(a) => (char_to_utf16(value, cursor), char_to_utf16(value, a), 2),
    }
}

/// Caret/anchor (char indices) for a script-written selection over `value`.
pub(crate) fn caret_from_selection(value: &str, start: u32, end: u32, dir: u8) -> (usize, Option<usize>) {
    let (s, e) = (utf16_to_char(value, start), utf16_to_char(value, end));
    if s == e {
        (s, None)
    } else if dir == 2 {
        (s, Some(e))
    } else {
        (e, Some(s))
    }
}

impl Lumen {
    /// Mirror the caret of `nid` (whose current value is `value`) into the
    /// document so scripts read it.
    pub(crate) fn publish_text_selection_for(&mut self, nid: lumen_dom::NodeId, value: &str) {
        let (cursor, anchor) = match self.form_state.get(&nid) {
            Some(s) => (s.cursor.unwrap_or_else(|| value.chars().count()), s.selection_anchor),
            None => (value.chars().count(), None),
        };
        let (start, end, dir) = selection_triple(value, cursor, anchor);
        if let Some(src) = self.layout_source.as_ref()
            && let Ok(mut doc) = src.document.lock()
        {
            doc.set_text_selection(nid, start, end, dir, false);
        }
    }

    /// Apply a selection a script wrote to `nid` onto the caret. `value` is the
    /// field's current value. Blocking lock — callers are input handlers.
    pub(crate) fn absorb_script_selection(&mut self, nid: lumen_dom::NodeId, value: &str) {
        let taken = self
            .layout_source
            .as_ref()
            .and_then(|src| src.document.lock().ok())
            .and_then(|mut doc| doc.take_script_selection(nid));
        if let Some(sel) = taken {
            self.apply_script_selection(nid, value, sel);
        }
    }

    fn apply_script_selection(&mut self, nid: lumen_dom::NodeId, value: &str, sel: lumen_dom::TextSelection) {
        let (cursor, anchor) = caret_from_selection(value, sel.start, sel.end, sel.dir);
        let slot = self.form_state.entry(nid).or_default();
        slot.cursor = Some(cursor);
        slot.selection_anchor = anchor;
    }

    /// Redraw-path variant of [`Self::absorb_script_selection`]: never blocks
    /// on the document (BUG-1108), and requests a repaint when the caret moved.
    pub(crate) fn absorb_script_selection_nonblocking(&mut self) {
        let Some(nid) = self.focused_node else { return };
        let Some((_, value)) = self.focused_field_snapshot.field(nid) else { return };
        let taken = self
            .layout_source
            .as_ref()
            .and_then(|src| src.document.try_lock().ok())
            .and_then(|mut doc| doc.take_script_selection(nid));
        if let Some(sel) = taken {
            self.apply_script_selection(nid, &value, sel);
            self.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trip_with_astral_char() {
        let s = "a😀b";
        assert_eq!(char_to_utf16(s, 2), 3);
        assert_eq!(utf16_to_char(s, 3), 2);
        assert_eq!(utf16_to_char(s, 2), 1, "inside a pair rounds down");
        assert_eq!(utf16_to_char(s, 99), 3);
    }

    #[test]
    fn triple_and_back() {
        assert_eq!(selection_triple("abcd", 3, Some(1)), (1, 3, 1));
        assert_eq!(selection_triple("abcd", 1, Some(3)), (1, 3, 2));
        assert_eq!(selection_triple("abcd", 2, Some(2)), (2, 2, 0));
        assert_eq!(caret_from_selection("abcd", 1, 3, 1), (3, Some(1)));
        assert_eq!(caret_from_selection("abcd", 1, 3, 2), (1, Some(3)));
        assert_eq!(caret_from_selection("abcd", 2, 2, 0), (2, None));
    }
}
