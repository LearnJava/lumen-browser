//! Character-index cursor arithmetic for `<input>`/`<textarea>` text editing
//! (FRAME-2 п.1: insertion/deletion at the tracked cursor position instead of
//! always at the end of the value).
//!
//! Cursor positions are **char indices** (`s.chars().count()` space), not byte
//! offsets — `<input>`/`<textarea>` values routinely carry non-ASCII text
//! (Cyrillic, emoji), and a byte-offset cursor would let Left/Right land mid
//! code point. `Vec<char>` round-tripping is O(n) per keystroke, which is fine
//! at form-field sizes; a rope/gap-buffer is not warranted here.
//!
//! Shared by [`crate::lumen::text_input`] (page) and
//! [`crate::lumen::frame_text_input`] (frame sub-document) — the same pure
//! arithmetic, just applied against a different document's `NodeId` space.

/// Number of chars in `s` — cursor positions run `0..=char_len(s)`.
pub(crate) fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Insert `ch` at char index `at` (clamped to `s`'s length).
///
/// Returns the new string and the cursor's new char index (`at.min(len) + 1`).
pub(crate) fn insert_char_at(s: &str, at: usize, ch: char) -> (String, usize) {
    let mut chars: Vec<char> = s.chars().collect();
    let at = at.min(chars.len());
    chars.insert(at, ch);
    (chars.into_iter().collect(), at + 1)
}

/// Delete the char immediately before char index `at` (Backspace).
///
/// Returns the new string and the cursor's new char index. No-op at `at == 0`.
pub(crate) fn delete_char_before(s: &str, at: usize) -> (String, usize) {
    if at == 0 {
        return (s.to_owned(), 0);
    }
    let mut chars: Vec<char> = s.chars().collect();
    let at = at.min(chars.len());
    chars.remove(at - 1);
    (chars.into_iter().collect(), at - 1)
}

/// Delete the char immediately after char index `at` (Delete/forward-delete).
///
/// Returns the new string; the cursor's char index is unchanged by design (a
/// forward delete never moves the caret). No-op at end of string.
pub(crate) fn delete_char_after(s: &str, at: usize) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    if at < chars.len() {
        chars.remove(at);
    }
    chars.into_iter().collect()
}

/// Delete the char range `[start, end)` (char indices, `start <= end`
/// expected — callers normalize an anchor/cursor pair before calling this).
/// Used to replace/clear an active text selection (FRAME-7 remainder 2)
/// before an insert or a Backspace/Delete applies. Both bounds are clamped to
/// the string's length; `start >= end` after clamping is a no-op.
pub(crate) fn delete_char_range(s: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = start.min(chars.len());
    let end = end.min(chars.len());
    if start >= end {
        return s.to_owned();
    }
    chars[..start].iter().chain(chars[end..].iter()).collect()
}

/// Chars `[start, end)` of `s` (bounds clamped) — the text a Copy/Cut of the
/// selection `start..end` puts on the clipboard (UX-CLIPBOARD).
pub(crate) fn char_range(s: &str, start: usize, end: usize) -> String {
    s.chars().skip(start).take(end.saturating_sub(start)).collect()
}

/// Insert `ins` at char index `at` (clamped). Returns the new string and the
/// cursor's char index right after the inserted text.
pub(crate) fn insert_str_at(s: &str, at: usize, ins: &str) -> (String, usize) {
    let mut chars: Vec<char> = s.chars().collect();
    let at = at.min(chars.len());
    let ins: Vec<char> = ins.chars().collect();
    let end = at + ins.len();
    chars.splice(at..at, ins);
    (chars.into_iter().collect(), end)
}

/// `maxlength` content attribute → limit (HTML LS §4.10.5.1: a valid
/// non-negative integer; anything else means "no limit"). Counted in chars,
/// like every other cursor position here.
pub(crate) fn parse_maxlength(attr: Option<&str>) -> Option<usize> {
    attr?.trim().parse::<usize>().ok()
}

/// Turn raw clipboard text into what a paste into a field inserts: line
/// breaks normalized to `\n`; a single-line `<input>` flattens each break to a
/// space (what Chromium does) while `<textarea>` keeps them; the result is
/// cut to the room `maxlength` leaves once the `replaced` selection chars are
/// gone (`current_len - replaced + inserted <= maxlength`).
pub(crate) fn sanitize_paste(
    raw: &str,
    multiline: bool,
    maxlength: Option<usize>,
    current_len: usize,
    replaced: usize,
) -> String {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let text: String = if multiline {
        normalized
    } else {
        normalized.replace('\n', " ")
    };
    match maxlength {
        Some(max) => {
            let room = max.saturating_sub(current_len.saturating_sub(replaced));
            text.chars().take(room).collect()
        }
        None => text,
    }
}

/// How an edit changes the value — decides whether it joins the previous undo
/// group (UX-UNDO).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EditKind {
    /// Typed character; `word_start` = a non-space typed right after a space.
    Typing { word_start: bool },
    Backspace,
    DeleteForward,
    /// Paste, cut: always its own group.
    Other,
}

/// Value and caret of a field at one point of its undo history.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct FieldSnapshot {
    pub(crate) value: String,
    pub(crate) cursor: usize,
}

/// Pause that closes a run of typing into its own undo step.
const UNDO_GROUP_PAUSE: std::time::Duration = std::time::Duration::from_millis(1000);
/// Oldest steps are dropped past this many.
const UNDO_LIMIT: usize = 200;

/// Per-field undo/redo stack (UX-UNDO). Snapshot based: a step stores the
/// value and caret *before* the edit, so undo restores both. Consecutive
/// typing / Backspace / Delete merge into one step until a pause, a word
/// start, a change of kind or a caret jump.
#[derive(Default, Debug)]
pub(crate) struct FieldHistory {
    undo: Vec<FieldSnapshot>,
    redo: Vec<FieldSnapshot>,
    /// Value the last recorded edit (or undo/redo) left behind; a field whose
    /// value differs was changed from outside (script, form reset), so the
    /// stacks no longer describe it.
    after: Option<String>,
    last: Option<(EditKind, usize, std::time::Instant)>,
}

impl FieldHistory {
    /// Record an edit that turns `before` into `after_value`, leaving the caret
    /// at `after_cursor`.
    pub(crate) fn record(
        &mut self,
        before: FieldSnapshot,
        kind: EditKind,
        after_value: &str,
        after_cursor: usize,
        now: std::time::Instant,
    ) {
        if self.after.as_deref().is_some_and(|a| a != before.value) {
            self.undo.clear();
            self.last = None;
        }
        let merge = !self.undo.is_empty()
            && self.last.is_some_and(|(k, cur, at)| {
                kind != EditKind::Other
                    && !matches!(kind, EditKind::Typing { word_start: true })
                    && std::mem::discriminant(&k) == std::mem::discriminant(&kind)
                    && cur == before.cursor
                    && now.duration_since(at) < UNDO_GROUP_PAUSE
            });
        if !merge {
            self.undo.push(before);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.after = Some(after_value.to_owned());
        self.last = Some((kind, after_cursor, now));
    }

    /// Step back: `current` goes to the redo stack, the previous state is returned.
    pub(crate) fn undo(&mut self, current: &FieldSnapshot) -> Option<FieldSnapshot> {
        self.step(current, true)
    }

    /// Step forward again after an undo.
    pub(crate) fn redo(&mut self, current: &FieldSnapshot) -> Option<FieldSnapshot> {
        self.step(current, false)
    }

    fn step(&mut self, current: &FieldSnapshot, back: bool) -> Option<FieldSnapshot> {
        if self.after.as_deref().is_some_and(|a| a != current.value) {
            *self = FieldHistory::default();
            return None;
        }
        let (from, to) = if back { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        let target = from.pop()?;
        to.push(current.clone());
        self.after = Some(target.value.clone());
        self.last = None;
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(v: &str, c: usize) -> FieldSnapshot {
        FieldSnapshot { value: v.to_owned(), cursor: c }
    }

    #[test]
    fn history_groups_typing_and_undoes_whole_run() {
        let t0 = std::time::Instant::now();
        let ms = |n| t0 + std::time::Duration::from_millis(n);
        let mut h = FieldHistory::default();
        let ty = EditKind::Typing { word_start: false };
        h.record(snap("", 0), ty, "a", 1, ms(0));
        h.record(snap("a", 1), ty, "ab", 2, ms(100));
        h.record(snap("ab", 2), ty, "abc", 3, ms(200));
        assert_eq!(h.undo(&snap("abc", 3)), Some(snap("", 0)));
        assert_eq!(h.undo(&snap("", 0)), None);
        assert_eq!(h.redo(&snap("", 0)), Some(snap("abc", 3)));
    }

    #[test]
    fn history_splits_on_pause_word_start_and_kind() {
        let t0 = std::time::Instant::now();
        let ms = |n| t0 + std::time::Duration::from_millis(n);
        let mut h = FieldHistory::default();
        let ty = EditKind::Typing { word_start: false };
        h.record(snap("", 0), ty, "a", 1, ms(0));
        h.record(snap("a", 1), ty, "ab", 2, ms(2000));
        h.record(snap("ab", 2), EditKind::Typing { word_start: true }, "ab c", 4, ms(2100));
        h.record(snap("ab c", 4), EditKind::Backspace, "ab ", 3, ms(2200));
        assert_eq!(h.undo(&snap("ab ", 3)), Some(snap("ab c", 4)));
        assert_eq!(h.undo(&snap("ab c", 4)), Some(snap("ab", 2)));
        assert_eq!(h.undo(&snap("ab", 2)), Some(snap("a", 1)));
        assert_eq!(h.undo(&snap("a", 1)), Some(snap("", 0)));
    }

    #[test]
    fn history_new_edit_clears_redo_and_external_change_resets() {
        let t0 = std::time::Instant::now();
        let mut h = FieldHistory::default();
        h.record(snap("a", 1), EditKind::Other, "ab", 2, t0);
        assert!(h.undo(&snap("ab", 2)).is_some());
        h.record(snap("a", 1), EditKind::Other, "ax", 2, t0);
        assert_eq!(h.redo(&snap("ax", 2)), None);
        // the script rewrote the value: nothing to undo into
        assert_eq!(h.undo(&snap("zzz", 3)), None);
        assert_eq!(h.undo(&snap("ax", 2)), None);
    }

    #[test]
    fn insert_in_middle() {
        assert_eq!(insert_char_at("ac", 1, 'b'), ("abc".to_owned(), 2));
    }

    #[test]
    fn insert_clamps_past_end() {
        assert_eq!(insert_char_at("ab", 99, 'c'), ("abc".to_owned(), 3));
    }

    #[test]
    fn insert_multibyte_char_by_char_index() {
        // Cyrillic п is multi-byte in UTF-8 — a byte-offset cursor would panic
        // or split the code point here.
        assert_eq!(insert_char_at("привт", 4, 'е'), ("привет".to_owned(), 5));
    }

    #[test]
    fn delete_before_middle() {
        assert_eq!(delete_char_before("abc", 2), ("ac".to_owned(), 1));
    }

    #[test]
    fn delete_before_at_zero_is_noop() {
        assert_eq!(delete_char_before("abc", 0), ("abc".to_owned(), 0));
    }

    #[test]
    fn delete_after_middle() {
        assert_eq!(delete_char_after("abc", 1), "ac".to_owned());
    }

    #[test]
    fn delete_after_at_end_is_noop() {
        assert_eq!(delete_char_after("abc", 3), "abc".to_owned());
    }

    #[test]
    fn char_len_counts_chars_not_bytes() {
        assert_eq!(char_len("привет"), 6);
    }

    #[test]
    fn delete_char_range_middle() {
        assert_eq!(delete_char_range("abcde", 1, 3), "ade".to_owned());
    }

    #[test]
    fn delete_char_range_clamps_end_past_len() {
        assert_eq!(delete_char_range("abc", 1, 99), "a".to_owned());
    }

    #[test]
    fn delete_char_range_start_ge_end_is_noop() {
        assert_eq!(delete_char_range("abc", 2, 2), "abc".to_owned());
        assert_eq!(delete_char_range("abc", 2, 1), "abc".to_owned());
    }

    #[test]
    fn delete_char_range_multibyte() {
        assert_eq!(delete_char_range("привет", 2, 5), "прт".to_owned());
    }

    #[test]
    fn char_range_slices_by_chars() {
        assert_eq!(char_range("привет", 1, 4), "рив");
        assert_eq!(char_range("abc", 2, 9), "c");
        assert_eq!(char_range("abc", 2, 1), "");
    }

    #[test]
    fn insert_str_moves_cursor_past_text() {
        assert_eq!(insert_str_at("ad", 1, "bc"), ("abcd".to_owned(), 3));
        assert_eq!(insert_str_at("ad", 9, "é"), ("adé".to_owned(), 3));
    }

    #[test]
    fn maxlength_parse() {
        assert_eq!(parse_maxlength(Some(" 5 ")), Some(5));
        assert_eq!(parse_maxlength(Some("-1")), None);
        assert_eq!(parse_maxlength(Some("x")), None);
        assert_eq!(parse_maxlength(None), None);
    }

    #[test]
    fn paste_sanitize_newlines_and_maxlength() {
        assert_eq!(sanitize_paste("a\r\nb\rc", false, None, 0, 0), "a b c");
        assert_eq!(sanitize_paste("a\r\nb\rc", true, None, 0, 0), "a\nb\nc");
        assert_eq!(sanitize_paste("абвгд", false, Some(5), 3, 1), "абв");
        assert_eq!(sanitize_paste("xyz", false, Some(2), 4, 0), "");
    }
}
