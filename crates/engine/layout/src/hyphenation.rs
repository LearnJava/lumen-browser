//! CSS Text L3 §6 — `hyphens: auto` algorithm stub.
//!
//! Provides the public [`SoftHyphenPoint`] type and [`collect_hyphen_points`]
//! function, which unify the two sources of hyphenation break points:
//!
//! - **U+00AD SOFT HYPHEN** characters embedded in the source text (`hyphens: manual`)
//! - **Algorithmic breaks** from a [`HyphenationProvider`] dictionary (`hyphens: auto`)
//!
//! The result is a sorted, deduplicated `Vec<SoftHyphenPoint>` that the inline
//! line-breaking algorithm can use to attempt hyphenation before forcing a wrap.
//!
//! P4 wiring point: when `hyphens: auto` is set in `ComputedStyle`, the layout
//! engine should call [`collect_hyphen_points`] with the word, locale from
//! `lang` attribute, and the configured `HyphenationProvider`.
//!
//! Phase 1 (dictionary hyphenation): install a real `HyphenationProvider` backed
//! by TeX `hyph-*.pat` pattern files.

use lumen_core::ext::HyphenationProvider;
use crate::style::Hyphens;

/// A potential soft-hyphen break position within a word's *display* string.
///
/// The `byte_offset` is relative to the **display string** — the version of
/// the word with all U+00AD SOFT HYPHEN characters removed.  The caller
/// should insert a visible `'-'` at this position when rendering the broken
/// prefix, and continue with the suffix on the next line.
///
/// Produced by [`collect_hyphen_points`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SoftHyphenPoint {
    /// Byte offset in the display string (U+00AD stripped) where a `-` can be appended.
    pub byte_offset: usize,
}

/// Collect soft-hyphen break points for `word` under the given `hyphens` policy.
///
/// # Arguments
/// - `word` — raw word as it appears in the text content (may contain U+00AD).
/// - `locale` — BCP 47 language tag (e.g. `"en-US"`, `"ru-RU"`). Passed to
///   [`HyphenationProvider::hyphenate`] when `hyphens == Auto`.  Empty string
///   asks the provider to use its default dictionary.
/// - `hyphens` — the computed value of the CSS `hyphens` property.
/// - `provider` — the active [`HyphenationProvider`].  For `hyphens: manual`
///   the provider is not called.  For `hyphens: auto` it is called with the
///   display string (U+00AD stripped) and `locale`.
///
/// # Returns
/// Sorted, deduplicated `Vec<SoftHyphenPoint>` in ascending `byte_offset` order.
/// Empty when `hyphens == None` or when no break points are found.
///
/// # Example
/// ```
/// # use lumen_layout::hyphenation::{collect_hyphen_points, SoftHyphenPoint};
/// # use lumen_layout::style::Hyphens;
/// # use lumen_core::ext::NullHyphenationProvider;
/// // "hyph\u{00AD}en" has a manual break between 'h' and 'e'.
/// let word = "hyph\u{00AD}en";
/// let pts = collect_hyphen_points(word, "en-US", Hyphens::Manual, &NullHyphenationProvider);
/// assert_eq!(pts.len(), 1);
/// assert_eq!(pts[0].byte_offset, 4); // "hyph" is 4 bytes
/// ```
pub fn collect_hyphen_points(
    word: &str,
    locale: &str,
    hyphens: Hyphens,
    provider: &dyn HyphenationProvider,
) -> Vec<SoftHyphenPoint> {
    if hyphens == Hyphens::None {
        return Vec::new();
    }

    // Strip U+00AD from the word to get the display string, recording where
    // each soft hyphen appeared in the display-byte coordinate space.
    let mut display = String::with_capacity(word.len());
    let mut points: Vec<SoftHyphenPoint> = Vec::new();

    for ch in word.chars() {
        if ch == '\u{00AD}' {
            points.push(SoftHyphenPoint { byte_offset: display.len() });
        } else {
            display.push(ch);
        }
    }

    // For `hyphens: auto`, add provider-supplied break points on the display string.
    if hyphens == Hyphens::Auto && !display.is_empty() {
        for off in provider.hyphenate(&display, locale) {
            // Provider returns byte offsets within `display`; skip out-of-bounds values.
            if off > 0 && off < display.len() {
                points.push(SoftHyphenPoint { byte_offset: off });
            }
        }
    }

    points.sort_unstable();
    points.dedup();
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::NullHyphenationProvider;
    use crate::style::Hyphens;

    // ── Hyphens::None ─────────────────────────────────────────────────────────

    #[test]
    fn none_returns_empty() {
        let word = "hyph\u{00AD}en";
        let pts = collect_hyphen_points(word, "en", Hyphens::None, &NullHyphenationProvider);
        assert!(pts.is_empty());
    }

    #[test]
    fn none_ignores_auto_provider() {
        // Even if a provider were non-null, Hyphens::None should short-circuit.
        let pts = collect_hyphen_points("typography", "en", Hyphens::None, &NullHyphenationProvider);
        assert!(pts.is_empty());
    }

    // ── Hyphens::Manual ───────────────────────────────────────────────────────

    #[test]
    fn manual_no_shy_returns_empty() {
        let pts = collect_hyphen_points("hello", "en", Hyphens::Manual, &NullHyphenationProvider);
        assert!(pts.is_empty());
    }

    #[test]
    fn manual_single_shy() {
        // "hyph\u{00AD}en" → display "hyphen", break at offset 4 ("hyph").
        let pts = collect_hyphen_points("hyph\u{00AD}en", "en", Hyphens::Manual, &NullHyphenationProvider);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].byte_offset, 4);
    }

    #[test]
    fn manual_multiple_shy() {
        // "ty\u{00AD}po\u{00AD}gra\u{00AD}phy"
        // display = "typography", breaks at byte offsets 2 ("ty"), 4 ("typo"), 7 ("typogra").
        let word = "ty\u{00AD}po\u{00AD}gra\u{00AD}phy";
        let pts = collect_hyphen_points(word, "en", Hyphens::Manual, &NullHyphenationProvider);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0].byte_offset, 2);
        assert_eq!(pts[1].byte_offset, 4);
        assert_eq!(pts[2].byte_offset, 7);
    }

    #[test]
    fn manual_adjacent_shy_deduplicated() {
        // Two adjacent U+00AD produce the same display offset → dedup to one point.
        let word = "ab\u{00AD}\u{00AD}cd";
        let pts = collect_hyphen_points(word, "en", Hyphens::Manual, &NullHyphenationProvider);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].byte_offset, 2);
    }

    #[test]
    fn manual_does_not_call_provider() {
        // NullHyphenationProvider returns no points; manual mode must not rely on it.
        let pts = collect_hyphen_points(
            "sup\u{00AD}er",
            "en",
            Hyphens::Manual,
            &NullHyphenationProvider,
        );
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].byte_offset, 3);
    }

    // ── Hyphens::Auto ─────────────────────────────────────────────────────────

    #[test]
    fn auto_null_provider_returns_only_shy() {
        // NullHyphenationProvider returns no auto points.
        // Only the embedded U+00AD contributes.
        let pts = collect_hyphen_points("hyph\u{00AD}en", "en", Hyphens::Auto, &NullHyphenationProvider);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].byte_offset, 4);
    }

    #[test]
    fn auto_no_shy_null_provider_empty() {
        let pts = collect_hyphen_points("hello", "en", Hyphens::Auto, &NullHyphenationProvider);
        assert!(pts.is_empty());
    }

    #[test]
    fn auto_custom_provider_merges_points() {
        struct MockProvider;
        impl HyphenationProvider for MockProvider {
            fn hyphenate(&self, word: &str, _locale: &str) -> Vec<usize> {
                // Provide break points at offsets 2 and 6 (within the display string).
                if word == "typography" { vec![2, 6] } else { vec![] }
            }
            fn locales(&self) -> Vec<String> { vec!["en-US".into()] }
        }
        // Word with U+00AD at display offset 4 ("typo") + provider adds 2 ("ty") and 6 ("typogr").
        let word = "typo\u{00AD}graphy";
        let pts = collect_hyphen_points(word, "en-US", Hyphens::Auto, &MockProvider);
        let offsets: Vec<usize> = pts.iter().map(|p| p.byte_offset).collect();
        // Expected: 2, 4, 6 — sorted, no duplicates.
        assert_eq!(offsets, vec![2, 4, 6]);
    }

    #[test]
    fn auto_provider_deduplicates_with_shy() {
        struct MockProvider;
        impl HyphenationProvider for MockProvider {
            fn hyphenate(&self, _word: &str, _locale: &str) -> Vec<usize> {
                vec![4] // same offset as the U+00AD below
            }
            fn locales(&self) -> Vec<String> { vec![] }
        }
        let word = "hyph\u{00AD}en"; // U+00AD at display offset 4
        let pts = collect_hyphen_points(word, "en", Hyphens::Auto, &MockProvider);
        // Dedup: 4 appears only once.
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].byte_offset, 4);
    }

    #[test]
    fn auto_provider_skips_zero_and_end_offsets() {
        struct MockProvider;
        impl HyphenationProvider for MockProvider {
            fn hyphenate(&self, word: &str, _locale: &str) -> Vec<usize> {
                vec![0, word.len(), 3] // 0 and word.len() are invalid; 3 is valid
            }
            fn locales(&self) -> Vec<String> { vec![] }
        }
        let pts = collect_hyphen_points("hyphen", "en", Hyphens::Auto, &MockProvider);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].byte_offset, 3);
    }

    // ── sorted output ─────────────────────────────────────────────────────────

    #[test]
    fn output_is_sorted() {
        // Multiple U+00AD in reverse logical order — output must still be sorted.
        let word = "a\u{00AD}b\u{00AD}c\u{00AD}d";
        let pts = collect_hyphen_points(word, "", Hyphens::Manual, &NullHyphenationProvider);
        let offsets: Vec<usize> = pts.iter().map(|p| p.byte_offset).collect();
        let mut sorted = offsets.clone();
        sorted.sort_unstable();
        assert_eq!(offsets, sorted);
    }

    // ── multibyte UTF-8 ───────────────────────────────────────────────────────

    #[test]
    fn multibyte_chars_correct_offsets() {
        // "café\u{00AD}shop" — 'é' is 2 bytes (0xC3 0xA9).
        // display = "caféshop", U+00AD was after 'é', which is byte offset 5.
        let word = "caf\u{00E9}\u{00AD}shop";
        let pts = collect_hyphen_points(word, "en", Hyphens::Manual, &NullHyphenationProvider);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].byte_offset, 5); // "café" = c(1)+a(1)+f(1)+é(2) = 5 bytes
    }
}
