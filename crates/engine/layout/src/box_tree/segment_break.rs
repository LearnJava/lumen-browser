//! CSS Text L3 §4.1.2 «Segment Break Transformation Rules» (BUG-1330).
//!
//! A collapsible segment break (`\n` in `white-space: normal | nowrap`) becomes a
//! space — except between two East Asian wide characters, where it is removed so
//! that Chinese / Japanese text can be wrapped in the source without gaps:
//!
//! 1. spaces and tabs around the break are removed first;
//! 2. the break is removed if the character before or after is U+200B;
//! 3. the break is removed if the East Asian Width of both neighbours is F, W or H
//!    and neither is Hangul;
//! 4. otherwise it turns into a space (done later, by the word splitter).
//!
//! Neighbours are looked up across inline-box boundaries ([`collapse_segment_breaks`]
//! works on the whole run), out-of-flow boxes never become segments and so are
//! transparent. The work happens on the segment text before wrapping, so intrinsic
//! sizing and line breaking see the same text.

use super::entry::is_collapsible_whitespace;
use super::types::InlineSegment;

/// East Asian Width F, W or H (UAX #11). `A` (ambiguous) and `N`/`Na` are not
/// included — the rule is explicitly «not A».
fn is_fullwidth_wide_or_halfwidth(c: char) -> bool {
    const RANGES: &[(u32, u32)] = &[
        // W: Hangul Jamo (Hangul — excluded by `is_hangul`, listed for completeness).
        (0x1100, 0x115F),
        // H: halfwidth won sign.
        (0x20A9, 0x20A9),
        (0x231A, 0x231B),
        (0x2329, 0x232A),
        (0x23E9, 0x23EC),
        (0x23F0, 0x23F0),
        (0x23F3, 0x23F3),
        (0x25FD, 0x25FE),
        (0x2614, 0x2615),
        (0x2648, 0x2653),
        (0x267F, 0x267F),
        (0x2693, 0x2693),
        (0x26A1, 0x26A1),
        (0x26AA, 0x26AB),
        (0x26BD, 0x26BE),
        (0x26C4, 0x26C5),
        (0x26CE, 0x26CE),
        (0x26D4, 0x26D4),
        (0x26EA, 0x26EA),
        (0x26F2, 0x26F3),
        (0x26F5, 0x26F5),
        (0x26FA, 0x26FA),
        (0x26FD, 0x26FD),
        (0x2705, 0x2705),
        (0x270A, 0x270B),
        (0x2728, 0x2728),
        (0x274C, 0x274C),
        (0x274E, 0x274E),
        (0x2753, 0x2755),
        (0x2757, 0x2757),
        (0x2795, 0x2797),
        (0x27B0, 0x27B0),
        (0x27BF, 0x27BF),
        (0x2B1B, 0x2B1C),
        (0x2B50, 0x2B50),
        (0x2B55, 0x2B55),
        // W: CJK radicals, Kangxi, ideographic description.
        (0x2E80, 0x2E99),
        (0x2E9B, 0x2EF3),
        (0x2F00, 0x2FD5),
        (0x2FF0, 0x303E),
        // F: ideographic space is U+3000 (inside the range above), W: kana, bopomofo,
        // CJK compatibility, ideographs.
        (0x3041, 0x3096),
        (0x3099, 0x30FF),
        (0x3105, 0x312F),
        (0x3131, 0x318E),
        (0x3190, 0x31E5),
        (0x31EF, 0x321E),
        (0x3220, 0x3247),
        (0x3250, 0xA48C),
        (0xA490, 0xA4C6),
        (0xA960, 0xA97C),
        (0xAC00, 0xD7A3),
        (0xF900, 0xFAFF),
        (0xFE10, 0xFE19),
        (0xFE30, 0xFE52),
        (0xFE54, 0xFE66),
        (0xFE68, 0xFE6B),
        // F: fullwidth forms; H: halfwidth forms.
        (0xFF01, 0xFF60),
        (0xFF61, 0xFFBE),
        (0xFFC2, 0xFFC7),
        (0xFFCA, 0xFFCF),
        (0xFFD2, 0xFFD7),
        (0xFFDA, 0xFFDC),
        (0xFFE0, 0xFFE6),
        (0xFFE8, 0xFFEE),
        // W: supplementary-plane scripts and emoji.
        (0x16FE0, 0x16FE4),
        (0x16FF0, 0x16FF1),
        (0x17000, 0x187F7),
        (0x18800, 0x18CD5),
        (0x18D00, 0x18D08),
        (0x1AFF0, 0x1AFFE),
        (0x1B000, 0x1B2FB),
        (0x1F004, 0x1F004),
        (0x1F0CF, 0x1F0CF),
        (0x1F18E, 0x1F18E),
        (0x1F191, 0x1F19A),
        (0x1F200, 0x1F202),
        (0x1F210, 0x1F23B),
        (0x1F240, 0x1F248),
        (0x1F250, 0x1F251),
        (0x1F260, 0x1F265),
        (0x1F300, 0x1F320),
        (0x1F32D, 0x1F335),
        (0x1F337, 0x1F37C),
        (0x1F37E, 0x1F393),
        (0x1F3A0, 0x1F3CA),
        (0x1F3CF, 0x1F3D3),
        (0x1F3E0, 0x1F3F0),
        (0x1F3F4, 0x1F3F4),
        (0x1F3F8, 0x1F43E),
        (0x1F440, 0x1F440),
        (0x1F442, 0x1F4FC),
        (0x1F4FF, 0x1F53D),
        (0x1F54B, 0x1F54E),
        (0x1F550, 0x1F567),
        (0x1F57A, 0x1F57A),
        (0x1F595, 0x1F596),
        (0x1F5A4, 0x1F5A4),
        (0x1F5FB, 0x1F64F),
        (0x1F680, 0x1F6C5),
        (0x1F6CC, 0x1F6CC),
        (0x1F6D0, 0x1F6D2),
        (0x1F6D5, 0x1F6D7),
        (0x1F6DC, 0x1F6DF),
        (0x1F6EB, 0x1F6EC),
        (0x1F6F4, 0x1F6FC),
        (0x1F7E0, 0x1F7EB),
        (0x1F7F0, 0x1F7F0),
        (0x1F90C, 0x1F93A),
        (0x1F93C, 0x1F945),
        (0x1F947, 0x1F9FF),
        (0x1FA70, 0x1FA7C),
        (0x1FA80, 0x1FA88),
        (0x1FA90, 0x1FABD),
        (0x1FABF, 0x1FAC5),
        (0x1FACE, 0x1FADB),
        (0x1FAE0, 0x1FAE8),
        (0x1FAF0, 0x1FAF8),
        (0x20000, 0x2FFFD),
        (0x30000, 0x3FFFD),
    ];
    let c = c as u32;
    if c < 0x1100 {
        return false;
    }
    RANGES
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The folded space that a whitespace-only text node leaves at the end of `text`:
/// `\n` when that node held a segment break and the text before it ends in a character
/// that can still lose the break (wide or U+200B), a plain space otherwise. Keeps the
/// break visible to [`collapse_segment_breaks`] without changing Latin text at all.
pub(crate) fn folded_gap(text: &str, had_break: bool) -> char {
    let wide_before = text
        .trim_end_matches(is_ws_run_char)
        .chars()
        .next_back()
        .is_some_and(|c| c == '\u{200b}' || is_fullwidth_wide_or_halfwidth(c));
    if had_break && wide_before { '\n' } else { ' ' }
}

/// Hangul (Jamo, compatibility Jamo, syllables, halfwidth Jamo, parenthesised and
/// circled Hangul) — Korean uses spaces between words, so a break next to it stays
/// a space.
fn is_hangul(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF
        | 0x3131..=0x318E
        | 0x3200..=0x321E
        | 0x3260..=0x327E
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF
        | 0xFFA0..=0xFFDC)
}

/// Is a segment break between `before` and `after` removed (rules 2 and 3 above)?
pub(crate) fn segment_break_is_removed(before: char, after: char) -> bool {
    if before == '\u{200b}' || after == '\u{200b}' {
        return true;
    }
    is_fullwidth_wide_or_halfwidth(before)
        && is_fullwidth_wide_or_halfwidth(after)
        && !is_hangul(before)
        && !is_hangul(after)
}

/// Whether `seg` carries text that the segment-break rules apply to: plain text in
/// `white-space: normal | nowrap`.
fn collapses_segment_breaks(seg: &InlineSegment) -> bool {
    !seg.forced_break
        && seg.img_src.is_none()
        && !seg.text.is_empty()
        && !seg.style.white_space.preserves_newlines()
}

fn is_ws_run_char(c: char) -> bool {
    is_collapsible_whitespace(c)
}

/// Removes the whitespace runs that contain a segment break between two characters
/// for which [`segment_break_is_removed`] holds. Runs inside one segment are handled
/// directly; a run at the edge of a segment is matched with the neighbouring text
/// segment, so inline-box boundaries («aa<span>…</span>⏎bbb») are transparent.
pub(crate) fn collapse_segment_breaks(segs: &mut [InlineSegment]) {
    // Interior runs — no neighbours outside the segment needed.
    for seg in segs.iter_mut() {
        if collapses_segment_breaks(seg)
            && seg.text.contains('\n')
            && let Some(t) = remove_interior_breaks(&seg.text)
        {
            seg.text = t;
        }
    }
    // Runs split by a segment boundary: trailing run of `prev` + leading run of `cur`.
    let mut prev: Option<usize> = None;
    for j in 0..segs.len() {
        if !collapses_segment_breaks(&segs[j]) {
            prev = None;
            continue;
        }
        let Some(i) = prev else {
            prev = Some(j);
            continue;
        };
        let t_i = segs[i].text.len() - segs[i].text.trim_end_matches(is_ws_run_char).len();
        let l_j = segs[j].text.len() - segs[j].text.trim_start_matches(is_ws_run_char).len();
        let before = segs[i].text.trim_end_matches(is_ws_run_char).chars().next_back();
        let after = segs[j].text.trim_start_matches(is_ws_run_char).chars().next();
        if let (Some(a), Some(b)) = (before, after)
            && (t_i > 0 || l_j > 0)
            && (segs[i].text[segs[i].text.len() - t_i..].contains('\n')
                || segs[j].text[..l_j].contains('\n'))
            && segment_break_is_removed(a, b)
        {
            let keep = segs[i].text.len() - t_i;
            segs[i].text.truncate(keep);
            segs[j].text.drain(..l_j);
        }
        // A whitespace-only segment must not hide the real neighbour.
        if !segs[j].text.trim_matches(is_ws_run_char).is_empty() {
            prev = Some(j);
        }
    }
}

/// `text` with every interior whitespace run holding a `\n` removed when its
/// neighbours allow it; `None` when nothing changes.
fn remove_interior_breaks(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut chars = text.char_indices().peekable();
    let mut last_non_ws: Option<char> = None;
    while let Some((start, c)) = chars.next() {
        if !is_ws_run_char(c) {
            out.push(c);
            last_non_ws = Some(c);
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, n)) = chars.peek() {
            if !is_ws_run_char(n) {
                break;
            }
            end = i + n.len_utf8();
            chars.next();
        }
        let run = &text[start..end];
        let next = text[end..].chars().next();
        match (last_non_ws, next) {
            (Some(a), Some(b)) if run.contains('\n') && segment_break_is_removed(a, b) => {
                changed = true;
            }
            _ => out.push_str(run),
        }
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_table_lookup_hits_both_ends_and_misses_between() {
        assert!(is_fullwidth_wide_or_halfwidth('\u{20a9}'));
        assert!(is_fullwidth_wide_or_halfwidth('\u{3000}'));
        assert!(is_fullwidth_wide_or_halfwidth('\u{3fffd}'));
        assert!(!is_fullwidth_wide_or_halfwidth('a'));
        assert!(!is_fullwidth_wide_or_halfwidth('\u{ff}'));
        assert!(!is_fullwidth_wide_or_halfwidth('\u{303f}'));
    }

    #[test]
    fn wide_pairs_drop_the_break() {
        assert!(segment_break_is_removed('語', '中'));
        assert!(segment_break_is_removed('Ｌ', 'Ｗ'));
        assert!(segment_break_is_removed('Ｌ', 'ｶ'));
        assert!(segment_break_is_removed('ｶ', 'ｸ'));
    }

    #[test]
    fn latin_hangul_and_ambiguous_keep_the_space() {
        assert!(!segment_break_is_removed('L', 'W'));
        assert!(!segment_break_is_removed('語', 'W'));
        assert!(!segment_break_is_removed('한', '국'));
        assert!(!segment_break_is_removed('語', '한'));
        // U+00B0 DEGREE SIGN is East Asian Ambiguous.
        assert!(!segment_break_is_removed('語', '\u{b0}'));
    }

    #[test]
    fn zero_width_space_on_either_side_drops_the_break() {
        assert!(segment_break_is_removed('\u{200b}', 'b'));
        assert!(segment_break_is_removed('a', '\u{200b}'));
    }

    #[test]
    fn interior_runs_with_spaces_and_several_breaks() {
        assert_eq!(remove_interior_breaks("日本語\n中国话").as_deref(), Some("日本語中国话"));
        assert_eq!(remove_interior_breaks("日本語  \n  中国话").as_deref(), Some("日本語中国话"));
        assert_eq!(remove_interior_breaks("日本語 \n \n  \n中国话").as_deref(), Some("日本語中国话"));
        // A run without a segment break is a plain space, not touched here.
        assert_eq!(remove_interior_breaks("日本語 中国话"), None);
        assert_eq!(remove_interior_breaks("FULL\nWIDTH"), None);
    }
}
