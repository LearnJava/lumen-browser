//! CSS Text L3 §5.1/§5.5 — soft wrap opportunities *inside* a run of text that
//! carries no collapsible whitespace.
//!
//! The inline line-breaker splits text on document white space. What is left
//! is a "word" in the CSS sense: it may still hold break opportunities — after
//! a hyphen or dash, around a zero-width space, between two ideographs, inside
//! a Thai phrase — and must *not* break where UAX #14 forbids it (NBSP, WORD
//! JOINER, a non-breaking hyphen, a line starting with a closing bracket).
//!
//! The classes and pair rules of UAX #14 come from `icu_segmenter` (Unicode
//! `LineBreak` data for every code point, the dictionary/LSTM models for
//! the SA scripts), tailored by `line-break` and `word-break`:
//!
//! | `line-break` | ICU strictness | line may start with                    |
//! |--------------|----------------|----------------------------------------|
//! | `strict`     | `Strict`       | neither iteration marks nor small kana |
//! | `normal`     | `Normal`       | iteration marks, `〜`, `゠`             |
//! | `loose`      | `Loose`        | the above + small kana, `ー`, `・` …    |
//! | `anywhere`   | —              | any typographic character unit (L4 §5.3)|
//!
//! `auto` is UA-defined; Lumen resolves it to `normal`, matching the other
//! engines. `word-break: break-all` / `keep-all` select ICU's word option.
//!
//! This module is a table lookup only — it never measures text. The greedy
//! "how much fits on this line" decision lives in `box_tree::wrap_inline_run`.

use crate::style::{LineBreak, WordBreak};
use icu_segmenter::options::{LineBreakOptions, LineBreakStrictness, LineBreakWordOption};

/// Byte offsets inside `text` at which a soft wrap is allowed.
///
/// `text` is one white-space-delimited word; offsets are strictly between `0`
/// and `text.len()` and always fall on a `char` boundary. `line-break:
/// anywhere` breaks any script around every grapheme cluster and ignores
/// `word-break`.
pub fn break_opportunities(text: &str, strictness: LineBreak, word_break: WordBreak) -> Vec<usize> {
    if strictness == LineBreak::Anywhere {
        return anywhere_opportunities(text);
    }
    provider().opportunities(text, strictness, word_break)
}

/// Dictionary and rule based UAX #14 segmentation. Trait-anchor (ADR-027): the
/// provisional implementation is `icu_segmenter`; layout only sees this trait.
pub trait LineBreakProvider: Send + Sync {
    /// Byte offsets strictly inside `text` where a line may break, under the
    /// given `line-break` strictness (never `Anywhere`) and `word-break`.
    fn opportunities(&self, text: &str, strictness: LineBreak, word_break: WordBreak) -> Vec<usize>;
}

/// `icu_segmenter` with compiled rule and LSTM/dictionary data: one segmenter
/// per (strictness, word option) pair, built on first use.
struct IcuLineBreaker {
    segmenters: [std::sync::OnceLock<icu_segmenter::LineSegmenterBorrowed<'static>>; 9],
}

impl IcuLineBreaker {
    fn slot(strictness: LineBreakStrictness, word: LineBreakWordOption) -> usize {
        let s = match strictness {
            LineBreakStrictness::Loose => 0,
            LineBreakStrictness::Normal => 1,
            _ => 2,
        };
        let w = match word {
            LineBreakWordOption::BreakAll => 1,
            LineBreakWordOption::KeepAll => 2,
            _ => 0,
        };
        s * 3 + w
    }
}

impl LineBreakProvider for IcuLineBreaker {
    fn opportunities(&self, text: &str, strictness: LineBreak, word_break: WordBreak) -> Vec<usize> {
        let strictness = match strictness {
            LineBreak::Strict => LineBreakStrictness::Strict,
            LineBreak::Loose => LineBreakStrictness::Loose,
            _ => LineBreakStrictness::Normal,
        };
        let word = match word_break {
            WordBreak::BreakAll => LineBreakWordOption::BreakAll,
            WordBreak::KeepAll => LineBreakWordOption::KeepAll,
            _ => LineBreakWordOption::Normal,
        };
        let seg = self.segmenters[Self::slot(strictness, word)].get_or_init(|| {
            let mut options = LineBreakOptions::default();
            options.strictness = Some(strictness);
            options.word_option = Some(word);
            icu_segmenter::LineSegmenter::new_auto(options)
        });
        seg.segment_str(text).filter(|&p| p > 0 && p < text.len()).collect()
    }
}

fn provider() -> &'static dyn LineBreakProvider {
    static P: std::sync::OnceLock<IcuLineBreaker> = std::sync::OnceLock::new();
    P.get_or_init(|| IcuLineBreaker { segmenters: Default::default() })
}

/// `line-break: anywhere` — a soft wrap opportunity around every typographic
/// character unit (CSS Text L4 §5.3).
///
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

/// Combining marks and variation selectors that must not be split from the
/// base character. Covers the common ranges, not the full Unicode property.
pub(crate) fn is_combining(ch: char) -> bool {
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
        let opps = break_opportunities(text, strictness, WordBreak::Normal);
        text.char_indices()
            .enumerate()
            .filter(|(_, (off, _))| opps.contains(off))
            .map(|(idx, _)| idx)
            .collect()
    }

    #[test]
    fn latin_text_has_no_opportunities() {
        assert!(break_opportunities("Hello", LineBreak::Auto, WordBreak::Normal).is_empty());
        assert!(break_opportunities("longwordwithout", LineBreak::Strict, WordBreak::Normal).is_empty());
        // Latin punctuation must not open a break either.
        assert!(break_opportunities("f(x)=y!", LineBreak::Normal, WordBreak::Normal).is_empty());
    }

    #[test]
    fn thai_run_breaks_at_dictionary_words() {
        let text = "สวัสดีครับผมชื่อสมชายยินดีที่ได้รู้จัก";
        let opps = break_opportunities(text, LineBreak::Auto, WordBreak::Normal);
        assert!(opps.len() >= 3, "{opps:?}");
        assert!(opps.iter().all(|&o| text.is_char_boundary(o)));
    }

    #[test]
    fn khmer_run_breaks() {
        let text = "ខ្ញុំចូលចិត្តរៀនភាសាខ្មែរណាស់";
        assert!(!break_opportunities(text, LineBreak::Auto, WordBreak::Normal).is_empty());
    }

    #[test]
    fn empty_and_single_char() {
        assert!(break_opportunities("", LineBreak::Auto, WordBreak::Normal).is_empty());
        assert!(break_opportunities("漢", LineBreak::Auto, WordBreak::Normal).is_empty());
    }

    #[test]
    fn breaks_between_ideographs() {
        // 4 ideographs → breaks before chars 1, 2, 3.
        assert_eq!(char_breaks("日本語版", LineBreak::Auto), vec![1, 2, 3]);
    }

    #[test]
    fn opportunities_are_char_boundaries() {
        let text = "日本語版";
        for off in break_opportunities(text, LineBreak::Auto, WordBreak::Normal) {
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
    fn small_kana_starts_a_line_unless_strict() {
        // きゃく — ゃ is a small kana: `strict` keeps it glued to the previous
        // syllable, `normal` (hence `auto`) and `loose` break before it
        // (css/css-text/line-break/line-break-normal-011).
        assert_eq!(char_breaks("きゃく", LineBreak::Strict), vec![2]);
        assert_eq!(char_breaks("きゃく", LineBreak::Normal), vec![1, 2]);
        assert_eq!(char_breaks("きゃく", LineBreak::Auto), vec![1, 2]);
        assert_eq!(char_breaks("きゃく", LineBreak::Loose), vec![1, 2]);
    }

    #[test]
    fn prolonged_sound_mark_follows_small_kana_rule() {
        assert_eq!(char_breaks("カード", LineBreak::Strict), vec![2]);
        assert_eq!(char_breaks("カード", LineBreak::Normal), vec![1, 2]);
    }

    #[test]
    fn iteration_mark_starts_a_line_only_in_loose() {
        // 人々 — 々 repeats the previous ideograph.
        assert_eq!(char_breaks("人々人", LineBreak::Strict), vec![2]);
        assert_eq!(char_breaks("人々人", LineBreak::Normal), vec![2]);
        assert_eq!(char_breaks("人々人", LineBreak::Loose), vec![1, 2]);
    }

    #[test]
    fn exclamation_never_starts_a_line() {
        for s in [LineBreak::Strict, LineBreak::Normal, LineBreak::Loose] {
            assert_eq!(char_breaks("本！語", s), vec![2], "strictness={s:?}");
        }
    }

    #[test]
    fn postfix_and_prefix_stay_attached() {
        // 「50％」 — % must not start a line, ￥ must not end one.
        assert_eq!(char_breaks("五％五", LineBreak::Normal), vec![2]);
        assert_eq!(char_breaks("五￥五", LineBreak::Normal), vec![1]);
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

    fn chars_at(text: &str, word_break: WordBreak) -> Vec<usize> {
        let opps = break_opportunities(text, LineBreak::Auto, word_break);
        text.char_indices()
            .enumerate()
            .filter(|(_, (off, _))| opps.contains(off))
            .map(|(idx, _)| idx)
            .collect()
    }

    #[test]
    fn breaks_after_hyphens_and_dashes_in_latin_words() {
        // Hyphen-minus, HYPHEN (U+2010), EN DASH, VERTICAL LINE and ZWSP end a line.
        assert_eq!(char_breaks("ab-cd", LineBreak::Auto), vec![3]);
        assert_eq!(char_breaks("ab\u{2010}cd", LineBreak::Auto), vec![3]);
        assert_eq!(char_breaks("ab\u{2013}cd", LineBreak::Auto), vec![3]);
        assert_eq!(char_breaks("ab|cd", LineBreak::Auto), vec![3]);
        assert_eq!(char_breaks("ab\u{200B}cd", LineBreak::Auto), vec![3]);
    }

    #[test]
    fn glue_and_joiners_forbid_a_break() {
        for glue in [
            '\u{00A0}', '\u{202F}', '\u{2007}', '\u{2011}', '\u{2060}', '\u{FEFF}', '\u{180E}',
            '\u{034F}', '\u{0F08}',
        ] {
            let text = format!("ab{glue}cd");
            assert!(
                break_opportunities(&text, LineBreak::Auto, WordBreak::Normal).is_empty(),
                "U+{:04X}",
                glue as u32
            );
        }
    }

    #[test]
    fn keep_all_removes_ideograph_breaks() {
        assert_eq!(chars_at("日本語版", WordBreak::Normal), vec![1, 2, 3]);
        assert!(chars_at("日本語版", WordBreak::KeepAll).is_empty());
    }

    #[test]
    fn break_all_breaks_between_letters() {
        assert_eq!(chars_at("abc", WordBreak::BreakAll), vec![1, 2]);
        assert!(chars_at("abc", WordBreak::Normal).is_empty());
    }
}
