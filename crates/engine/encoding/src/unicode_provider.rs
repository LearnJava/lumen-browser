//! `Icu4xUnicodeProvider` — реализация [`UnicodeProvider`] через ICU4x.
//!
//! Используются:
//! - `icu_segmenter` — UAX #14 line-break / UAX #29 grapheme + word.
//! - `unicode-bidi` — UAX #9 bidirectional algorithm.
//!
//! Segmenter-ы инициализируются один раз и хранятся как `*Borrowed<'static>` —
//! просто ссылки на compile-time Unicode-таблицы (embedded через `compiled_data`).
//! Эти типы — `Copy`, `Send + Sync`.

use icu_segmenter::{
    GraphemeClusterSegmenter, GraphemeClusterSegmenterBorrowed, LineSegmenter,
    LineSegmenterBorrowed, WordSegmenter, WordSegmenterBorrowed,
    options::{LineBreakOptions, WordBreakInvariantOptions},
};
use lumen_core::ext::UnicodeProvider;

/// ICU4x-провайдер Unicode-операций.
///
/// Stateless at runtime (содержит только `'static` ссылки на compile-time данные).
/// `Copy + Clone`, передаётся без `Arc`.
#[derive(Debug, Clone, Copy)]
pub struct Icu4xUnicodeProvider {
    line: LineSegmenterBorrowed<'static>,
    grapheme: GraphemeClusterSegmenterBorrowed<'static>,
    word: WordSegmenterBorrowed<'static>,
}

impl Icu4xUnicodeProvider {
    /// Создаёт провайдер с auto-режимом (LSTM/dictionary для CJK/Thai/etc).
    pub fn new() -> Self {
        Self {
            line: LineSegmenter::new_auto(LineBreakOptions::default()),
            grapheme: GraphemeClusterSegmenter::new(),
            word: WordSegmenter::new_auto(WordBreakInvariantOptions::default()),
        }
    }

    /// Облегчённая версия — только Latin + UAX #14 rules, без LSTM.
    pub fn new_latin() -> Self {
        Self {
            line: LineSegmenter::new_for_non_complex_scripts(LineBreakOptions::default()),
            grapheme: GraphemeClusterSegmenter::new(),
            word: WordSegmenter::new_auto(WordBreakInvariantOptions::default()),
        }
    }
}

impl Default for Icu4xUnicodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: LineSegmenterBorrowed<'static>, GraphemeClusterSegmenterBorrowed<'static>, and
// WordSegmenterBorrowed<'static> contain only &'static references to compile-time data,
// which makes them Send + Sync. ICU4x marks these Copy/Clone for exactly this reason.
unsafe impl Send for Icu4xUnicodeProvider {}
unsafe impl Sync for Icu4xUnicodeProvider {}

impl UnicodeProvider for Icu4xUnicodeProvider {
    /// UAX #14 line-break opportunities: байтовые позиции допустимых разрывов,
    /// не включая 0 и `text.len()`.
    fn line_break_opportunities(&self, text: &str) -> Vec<usize> {
        self.line
            .segment_str(text)
            .filter(|&pos| pos > 0 && pos < text.len())
            .collect()
    }

    /// Границы графемных кластеров (UAX #29): включает 0 и `text.len()`.
    /// Для пустой строки — `[0]`.
    fn grapheme_boundaries(&self, text: &str) -> Vec<usize> {
        if text.is_empty() {
            return vec![0];
        }
        self.grapheme.segment_str(text).collect()
    }

    /// Границы слов (UAX #29): включает 0 и `text.len()`.
    fn word_boundaries(&self, text: &str) -> Vec<usize> {
        if text.is_empty() {
            return vec![0];
        }
        self.word.segment_str(text).collect()
    }

    /// UAX #9 bidi runs: `(start_byte, end_byte, is_rtl)`.
    /// Покрывают весь текст без перекрытий в logical-порядке.
    fn bidi_runs(&self, text: &str, base_rtl: bool) -> Vec<(usize, usize, bool)> {
        if text.is_empty() {
            return Vec::new();
        }
        use unicode_bidi::{BidiInfo, Level};
        let para_level = Some(if base_rtl { Level::rtl() } else { Level::ltr() });
        let bidi = BidiInfo::new(text, para_level);
        let levels = &bidi.levels;

        let mut runs: Vec<(usize, usize, bool)> = Vec::new();
        let mut run_start = 0usize;
        let mut run_rtl = levels.first().map(Level::is_rtl).unwrap_or(base_rtl);

        for (char_idx, (byte_off, _ch)) in text.char_indices().enumerate() {
            let is_rtl = levels.get(char_idx).map(Level::is_rtl).unwrap_or(base_rtl);
            if is_rtl != run_rtl {
                runs.push((run_start, byte_off, run_rtl));
                run_start = byte_off;
                run_rtl = is_rtl;
            }
        }
        runs.push((run_start, text.len(), run_rtl));
        runs
    }

    fn provider_name(&self) -> &'static str {
        "icu4x"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Icu4xUnicodeProvider {
        Icu4xUnicodeProvider::new_latin()
    }

    // ── line_break_opportunities ─────────────────────────────────────────────

    #[test]
    fn line_break_simple_words() {
        let breaks = p().line_break_opportunities("Hello world");
        // UAX #14: break opportunity after "Hello " (before "world"), not at 0 or 11.
        assert!(breaks.contains(&6), "expected break after 'Hello ': {breaks:?}");
        assert!(!breaks.contains(&0), "must not include 0");
        assert!(!breaks.contains(&11), "must not include text.len()");
    }

    #[test]
    fn line_break_empty_string() {
        assert!(p().line_break_opportunities("").is_empty());
    }

    #[test]
    fn line_break_no_break_opportunity() {
        assert!(p().line_break_opportunities("HelloWorld").is_empty());
    }

    // ── grapheme_boundaries ──────────────────────────────────────────────────

    #[test]
    fn grapheme_empty() {
        assert_eq!(p().grapheme_boundaries(""), vec![0]);
    }

    #[test]
    fn grapheme_ascii() {
        let b = p().grapheme_boundaries("abc");
        assert!(b.contains(&0));
        assert!(b.contains(&3));
    }

    #[test]
    fn grapheme_combining_accent() {
        // "e\u{0301}" = 'e' + combining acute (é as two code points, 3 bytes)
        let text = "e\u{0301}b";
        let b = p().grapheme_boundaries(text);
        // grapheme clusters: [0, 3, 4]
        assert_eq!(b[0], 0);
        assert_eq!(*b.last().unwrap(), text.len());
        assert!(b.contains(&3), "boundary after combining cluster: {b:?}");
        assert!(!b.contains(&1), "must NOT split inside combining cluster");
    }

    // ── word_boundaries ──────────────────────────────────────────────────────

    #[test]
    fn word_empty() {
        assert_eq!(p().word_boundaries(""), vec![0]);
    }

    #[test]
    fn word_two_words() {
        let b = p().word_boundaries("hello world");
        assert!(b.contains(&0));
        assert!(b.contains(&11));
        assert!(b.len() > 2, "expected multiple word boundaries: {b:?}");
    }

    // ── bidi_runs ────────────────────────────────────────────────────────────

    #[test]
    fn bidi_pure_ltr() {
        let runs = p().bidi_runs("Hello", false);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], (0, 5, false));
    }

    #[test]
    fn bidi_empty() {
        assert!(p().bidi_runs("", false).is_empty());
    }

    #[test]
    fn bidi_covers_full_text() {
        let text = "Hello world";
        let runs = p().bidi_runs(text, false);
        assert_eq!(runs.first().unwrap().0, 0);
        assert_eq!(runs.last().unwrap().1, text.len());
        for w in runs.windows(2) {
            assert_eq!(w[0].1, w[1].0, "runs must be contiguous");
        }
    }

    #[test]
    fn provider_name() {
        assert_eq!(p().provider_name(), "icu4x");
    }
}
