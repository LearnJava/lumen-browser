//! CSS Text L3 §5.5 `line-break` — soft wrap opportunities *inside* a run of
//! text that carries no whitespace.
//!
//! The inline line-breaker splits text on whitespace, which is enough for
//! space-separated scripts but leaves CJK text as a single giant "word": a
//! Japanese or Chinese paragraph has no spaces at all. UAX #14 allows a break
//! between two ideographs, and `line-break` picks how strictly the rules around
//! kana and punctuation are applied:
//!
//! | value     | line may start with                                     |
//! |-----------|---------------------------------------------------------|
//! | `strict`  | neither iteration marks nor small kana                   |
//! | `normal`  | iteration marks (`々ゝヽ`), `〜`, `゠`                     |
//! | `loose`   | the above + small kana, `ー`, `・`, `：`, `；`, `‐`, `–`   |
//! | `anywhere`| any typographic character unit (CSS Text L4 §5.3)        |
//!
//! `auto` is UA-defined; Lumen resolves it to `normal`, matching the other
//! engines.
//!
//! This module is a table lookup only — it never measures text. The greedy
//! "how much fits on this line" decision lives in `box_tree::wrap_inline_run`.

use crate::style::LineBreak;

/// Byte offsets inside `text` at which a soft wrap is allowed.
///
/// Offsets are strictly between `0` and `text.len()` and always fall on a
/// `char` boundary. The result is empty for text that carries no CJK character
/// (unless `strictness` is [`LineBreak::Anywhere`], which breaks any script).
pub fn break_opportunities(text: &str, strictness: LineBreak) -> Vec<usize> {
    if strictness == LineBreak::Anywhere {
        return anywhere_opportunities(text);
    }
    let mut out = Vec::new();
    let mut prev: Option<(char, Class)> = None;
    for (off, ch) in text.char_indices() {
        let cls = classify(ch);
        if let Some((prev_ch, prev_cls)) = prev
            && allows_break(prev_ch, prev_cls, ch, cls, strictness)
        {
            out.push(off);
        }
        prev = Some((ch, cls));
    }
    out
}

/// `line-break: anywhere` — a soft wrap opportunity around every typographic
/// character unit (CSS Text L4 §5.3).
///
/// Combining marks and variation selectors stay glued to their base character:
/// a "typographic character unit" is the grapheme cluster, not the code point.
fn anywhere_opportunities(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut prev_ch: Option<char> = None;
    for (off, ch) in text.char_indices() {
        if let Some(prev) = prev_ch {
            // ZWJ binds the two sides together (emoji sequences).
            if off > 0 && !is_combining(ch) && prev != ZWJ {
                out.push(off);
            }
        }
        prev_ch = Some(ch);
    }
    out
}

/// U+200D ZERO WIDTH JOINER.
const ZWJ: char = '\u{200D}';

/// UAX #14 line-break class, reduced to what CJK wrapping needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Opening bracket / quote (`OP`) — a line must not start after it.
    Open,
    /// Closing bracket, ideographic comma and full stop (`CL`, `CP`, `IS`) —
    /// a line must not start with it.
    Close,
    /// Exclamation and question marks (`EX`) — a line must not start with it.
    Exclamation,
    /// Non-starter (`NS`) — small kana, iteration marks and friends. Whether a
    /// line may start with it depends on `line-break`.
    NonStarter(NonStarter),
    /// Currency and other prefixes (`PR`) — a line must not start after it.
    Prefix,
    /// Percent, degree and other postfixes (`PO`) — a line must not start with it.
    Postfix,
    /// Everything else, including plain ideographs.
    Other,
}

/// Non-starter sub-classes, named after the least permissive `line-break`
/// value that still lets a line start with the character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NonStarter {
    /// Small kana, `ー`, `・`, `：`, `；`, `‐`, `–` — only `loose` allows it.
    LooseOnly,
    /// Iteration marks (`々〻ゝゞヽヾ`), `〜`, `゠` — `normal` (hence `auto`)
    /// already allows it; only `strict` forbids it.
    NormalOk,
}

/// Is a break allowed between `before` and `after`?
fn allows_break(
    before: char,
    before_cls: Class,
    after: char,
    after_cls: Class,
    strictness: LineBreak,
) -> bool {
    // At least one side must be CJK. In pure Latin text whitespace, hyphenation
    // and `overflow-wrap` already decide where the breaks are — adding UAX #14
    // opportunities there would break words nobody asked to break.
    if !is_cjk(before) && !is_cjk(after) {
        return false;
    }
    !no_break_after(before_cls, strictness) && !no_break_before(after_cls, strictness)
}

/// Classes a line must not start *after*.
fn no_break_after(cls: Class, strictness: LineBreak) -> bool {
    match cls {
        Class::Open => true,
        // `loose` breaks the number away from its currency sign.
        Class::Prefix => strictness != LineBreak::Loose,
        _ => false,
    }
}

/// Classes a line must not start *with*.
fn no_break_before(cls: Class, strictness: LineBreak) -> bool {
    match cls {
        Class::Close | Class::Exclamation => true,
        Class::Postfix => strictness != LineBreak::Loose,
        Class::NonStarter(NonStarter::LooseOnly) => strictness != LineBreak::Loose,
        Class::NonStarter(NonStarter::NormalOk) => strictness == LineBreak::Strict,
        _ => false,
    }
}

/// UAX #14 class of a single character (only the CJK-relevant subset).
fn classify(ch: char) -> Class {
    match ch {
        // OP — opening brackets and quotes, halfwidth and fullwidth.
        '(' | '[' | '{' | '‘' | '“' | '⦅' | '｟' | '｢' | '（' | '［' | '｛' | '〔' | '〈'
        | '《' | '「' | '『' | '【' | '〖' | '〘' | '〚' | '〝' => Class::Open,
        // CL / CP / IS — closing brackets, ideographic comma and full stop.
        ')' | ']' | '}' | '’' | '”' | '⦆' | '｠' | '｣' | '）' | '］' | '｝' | '〕' | '〉'
        | '》' | '」' | '』' | '】' | '〗' | '〙' | '〛' | '〞' | '、' | '。' | '，' | '．'
        | '､' | '｡' | ',' | '.' => Class::Close,
        // EX — exclamation and question marks.
        '!' | '?' | '！' | '？' | '‼' | '⁇' | '⁈' | '⁉' => Class::Exclamation,
        // NS — small kana and the prolonged sound mark stay with the previous
        // syllable unless the author asked for `loose`.
        'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'っ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ' | 'ゕ' | 'ゖ'
        | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ッ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ' | 'ヵ' | 'ヶ'
        | 'ー' | '・' | '：' | '；' | ':' | ';' | '‐' | '–' => {
            Class::NonStarter(NonStarter::LooseOnly)
        }
        // NS — iteration marks and wave-dash-like connectors.
        '々' | '〻' | 'ゝ' | 'ゞ' | 'ヽ' | 'ヾ' | '〜' | '～' | '゠' => {
            Class::NonStarter(NonStarter::NormalOk)
        }
        // Small katakana phonetic extensions (U+31F0…U+31FF).
        '\u{31F0}'..='\u{31FF}' => Class::NonStarter(NonStarter::LooseOnly),
        // PR — currency and other prefixes.
        '$' | '£' | '€' | '¥' | '＄' | '￥' | '￡' | '￠' | '＃' => Class::Prefix,
        // PO — percent, degree and other postfixes.
        '%' | '‰' | '°' | '′' | '″' | '％' | '℃' | '℉' => Class::Postfix,
        _ => Class::Other,
    }
}

/// Does the character belong to a CJK / kana / hangul block?
///
/// Used as the gate for the whole module: a break opportunity is only produced
/// when at least one of the two neighbouring characters is CJK, so Latin text
/// keeps wrapping exactly as before.
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF   // Hangul Jamo
        | 0x2E80..=0x2EFF // CJK Radicals Supplement
        | 0x2F00..=0x2FDF // Kangxi Radicals
        | 0x3000..=0x303F // CJK Symbols and Punctuation
        | 0x3040..=0x30FF // Hiragana + Katakana
        | 0x3130..=0x318F // Hangul Compatibility Jamo
        | 0x31F0..=0x31FF // Katakana Phonetic Extensions
        | 0x3200..=0x32FF // Enclosed CJK Letters and Months
        | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xA000..=0xA4CF // Yi Syllables
        | 0xAC00..=0xD7AF // Hangul Syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0xFE30..=0xFE4F // CJK Compatibility Forms
        | 0xFF00..=0xFF60 // Halfwidth and Fullwidth Forms (fullwidth ASCII)
        | 0xFF61..=0xFFDC // Halfwidth kana and jamo
        | 0xFFE0..=0xFFE6 // Fullwidth signs
        | 0x20000..=0x2FA1F // CJK Unified Ideographs Extension B…F
    )
}

/// Combining marks and variation selectors that must not be split from the
/// base character. Covers the common ranges, not the full Unicode property.
fn is_combining(ch: char) -> bool {
    matches!(ch as u32,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x0483..=0x0489 // Combining Cyrillic
        | 0x0591..=0x05BD // Hebrew points
        | 0x064B..=0x065F // Arabic marks
        | 0x0E31..=0x0E3A // Thai vowel signs and tone marks
        | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x200D          // ZWJ
        | 0x20D0..=0x20F0 // Combining Diacritical Marks for Symbols
        | 0x3099..=0x309A // Kana voiced sound marks
        | 0xFE00..=0xFE0F // Variation selectors
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte offsets → char indices, so the assertions stay readable for
    /// multi-byte CJK text.
    fn char_breaks(text: &str, strictness: LineBreak) -> Vec<usize> {
        let opps = break_opportunities(text, strictness);
        text.char_indices()
            .enumerate()
            .filter(|(_, (off, _))| opps.contains(off))
            .map(|(idx, _)| idx)
            .collect()
    }

    #[test]
    fn latin_text_has_no_opportunities() {
        assert!(break_opportunities("Hello", LineBreak::Auto).is_empty());
        assert!(break_opportunities("longwordwithout", LineBreak::Strict).is_empty());
        // Latin punctuation must not open a break either.
        assert!(break_opportunities("f(x)=y!", LineBreak::Normal).is_empty());
    }

    #[test]
    fn empty_and_single_char() {
        assert!(break_opportunities("", LineBreak::Auto).is_empty());
        assert!(break_opportunities("漢", LineBreak::Auto).is_empty());
    }

    #[test]
    fn breaks_between_ideographs() {
        // 4 ideographs → breaks before chars 1, 2, 3.
        assert_eq!(char_breaks("日本語版", LineBreak::Auto), vec![1, 2, 3]);
    }

    #[test]
    fn opportunities_are_char_boundaries() {
        let text = "日本語版";
        for off in break_opportunities(text, LineBreak::Auto) {
            assert!(text.is_char_boundary(off), "offset {off} splits a char");
            assert!(off > 0 && off < text.len());
        }
    }

    #[test]
    fn no_break_after_opening_bracket() {
        // 「 opens a quote: a line must not start with 本, nor with the
        // closing 」 — only before 「 and after 」.
        assert_eq!(char_breaks("日「本」語", LineBreak::Auto), vec![1, 4]);
    }

    #[test]
    fn no_break_before_ideographic_comma() {
        // 。 must not start a line; the break after it is fine.
        assert_eq!(char_breaks("日本。語", LineBreak::Auto), vec![1, 3]);
    }

    #[test]
    fn small_kana_starts_a_line_only_in_loose() {
        // きゃく — ゃ is a small kana, so `strict`/`normal` keep it glued.
        assert_eq!(char_breaks("きゃく", LineBreak::Strict), vec![2]);
        assert_eq!(char_breaks("きゃく", LineBreak::Normal), vec![2]);
        assert_eq!(char_breaks("きゃく", LineBreak::Auto), vec![2]);
        assert_eq!(char_breaks("きゃく", LineBreak::Loose), vec![1, 2]);
    }

    #[test]
    fn prolonged_sound_mark_follows_small_kana_rule() {
        assert_eq!(char_breaks("カード", LineBreak::Normal), vec![2]);
        assert_eq!(char_breaks("カード", LineBreak::Loose), vec![1, 2]);
    }

    #[test]
    fn iteration_mark_starts_a_line_unless_strict() {
        // 人々 — 々 repeats the previous ideograph.
        assert_eq!(char_breaks("人々人", LineBreak::Strict), vec![2]);
        assert_eq!(char_breaks("人々人", LineBreak::Normal), vec![1, 2]);
        assert_eq!(char_breaks("人々人", LineBreak::Loose), vec![1, 2]);
    }

    #[test]
    fn exclamation_never_starts_a_line() {
        for s in [LineBreak::Strict, LineBreak::Normal, LineBreak::Loose] {
            assert_eq!(char_breaks("本！語", s), vec![2], "strictness={s:?}");
        }
    }

    #[test]
    fn postfix_and_prefix_relax_in_loose() {
        // 「50％」 — % must not start a line unless `loose`.
        assert_eq!(char_breaks("五％五", LineBreak::Normal), vec![2]);
        assert_eq!(char_breaks("五％五", LineBreak::Loose), vec![1, 2]);
        // ￥ must not end a line unless `loose`.
        assert_eq!(char_breaks("五￥五", LineBreak::Normal), vec![1]);
        assert_eq!(char_breaks("五￥五", LineBreak::Loose), vec![1, 2]);
    }

    #[test]
    fn cjk_latin_boundary_breaks() {
        // A break between an ideograph and Latin letters is allowed (UAX #14).
        assert_eq!(char_breaks("日本ab", LineBreak::Auto), vec![1, 2]);
        assert_eq!(char_breaks("ab日本", LineBreak::Auto), vec![2, 3]);
    }

    #[test]
    fn anywhere_breaks_latin() {
        assert_eq!(char_breaks("abc", LineBreak::Anywhere), vec![1, 2]);
        // …and ignores the CJK non-starter rules.
        assert_eq!(char_breaks("きゃく", LineBreak::Anywhere), vec![1, 2]);
    }

    #[test]
    fn anywhere_keeps_combining_marks_attached() {
        // "e" + combining acute + "b": no break between 'e' and the accent.
        assert_eq!(char_breaks("e\u{0301}b", LineBreak::Anywhere), vec![2]);
    }

    #[test]
    fn anywhere_keeps_zwj_sequences_together() {
        // Two ideographs joined by ZWJ: no break on either side of the joiner.
        assert!(char_breaks("日\u{200D}本", LineBreak::Anywhere).is_empty());
    }

    #[test]
    fn hangul_and_kana_are_cjk() {
        assert_eq!(char_breaks("한국어", LineBreak::Auto), vec![1, 2]);
        assert_eq!(char_breaks("カタカナ", LineBreak::Auto), vec![1, 2, 3]);
    }
}
