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

#[cfg(test)]
mod tests {
    use super::*;

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
