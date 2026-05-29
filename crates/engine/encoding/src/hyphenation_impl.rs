//! Knuth–Liang automatic hyphenation via the `hyphenation` crate.
//!
//! Implements `HyphenationProvider` using pre-built TeX dictionaries for all
//! languages bundled via `embed_all`. Dictionaries are deserialized on first
//! use per locale and then cached in a `Mutex<HashMap>`.

use std::collections::HashMap;
use std::sync::Mutex;

use hyphenation::{Hyphenator, Language, Load, Standard};
use lumen_core::ext::HyphenationProvider;

/// Knuth–Liang hyphenation with per-locale lazy-loaded embedded dictionaries.
///
/// Thread-safe: the inner `Mutex` is locked only during hyphenation.
/// Each locale's dictionary is loaded at most once (on the first call
/// for that locale) and then reused for all subsequent calls.
pub struct KnuthLiangHyphenation {
    cache: Mutex<HashMap<String, Standard>>,
}

impl KnuthLiangHyphenation {
    /// Create a new provider with an empty cache.
    pub fn new() -> Self {
        Self { cache: Mutex::new(HashMap::new()) }
    }
}

impl Default for KnuthLiangHyphenation {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a BCP 47 locale tag to a `hyphenation::Language` variant.
///
/// Only the primary language subtag (before `-`) is used.
/// Returns `None` for unsupported languages (hyphenation is skipped).
fn locale_to_language(locale: &str) -> Option<Language> {
    let primary = locale.split('-').next().unwrap_or(locale).to_lowercase();
    match primary.as_str() {
        "en" | "" => Some(Language::EnglishUS),
        "ru"      => Some(Language::Russian),
        "de"      => Some(Language::German1996),
        "fr"      => Some(Language::French),
        "uk"      => Some(Language::Ukrainian),
        "nl"      => Some(Language::Dutch),
        "es"      => Some(Language::Spanish),
        "pt"      => Some(Language::Portuguese),
        "it"      => Some(Language::Italian),
        "pl"      => Some(Language::Polish),
        "cs"      => Some(Language::Czech),
        _         => None,
    }
}

impl HyphenationProvider for KnuthLiangHyphenation {
    fn locales(&self) -> Vec<String> {
        vec![
            "en".into(), "ru".into(), "de".into(), "fr".into(),
            "uk".into(), "nl".into(), "es".into(), "pt".into(),
            "it".into(), "pl".into(), "cs".into(),
        ]
    }

    fn hyphenate(&self, word: &str, locale: &str) -> Vec<usize> {
        // Normalize to primary language subtag for cache keying.
        let effective = if locale.is_empty() { "en" } else { locale };
        let lang_key: String = effective
            .split('-')
            .next()
            .unwrap_or(effective)
            .to_lowercase();

        let mut cache = self.cache.lock().unwrap();

        // Lazy-load the dictionary on first use for this locale.
        if !cache.contains_key(&lang_key)
            && let Some(lang) = locale_to_language(&lang_key)
            && let Ok(dict) = Standard::from_embedded(lang)
        {
            cache.insert(lang_key.clone(), dict);
        }

        cache
            .get(&lang_key)
            .map(|dict| dict.hyphenate(word).breaks)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_basic_hyphenation() {
        let hp = KnuthLiangHyphenation::new();
        // "hyphenation" should have at least one break point
        let breaks = hp.hyphenate("hyphenation", "en");
        assert!(!breaks.is_empty(), "expected break points for 'hyphenation'");
    }

    #[test]
    fn english_short_word_no_breaks() {
        let hp = KnuthLiangHyphenation::new();
        // "the" is too short to hyphenate
        let breaks = hp.hyphenate("the", "en");
        assert!(breaks.is_empty(), "short word 'the' should have no breaks");
    }

    #[test]
    fn empty_locale_defaults_to_english() {
        let hp = KnuthLiangHyphenation::new();
        let breaks_en = hp.hyphenate("hyphenation", "en");
        let breaks_empty = hp.hyphenate("hyphenation", "");
        assert_eq!(breaks_en, breaks_empty, "empty locale should default to English");
    }

    #[test]
    fn russian_hyphenation() {
        let hp = KnuthLiangHyphenation::new();
        // "переносы" (transfers) should have break points
        let breaks = hp.hyphenate("переносы", "ru");
        assert!(!breaks.is_empty(), "expected break points for 'переносы'");
    }

    #[test]
    fn unsupported_locale_returns_empty() {
        let hp = KnuthLiangHyphenation::new();
        // "zh" (Chinese) is not supported — no break points returned
        let breaks = hp.hyphenate("hyphenation", "zh");
        assert!(breaks.is_empty(), "unsupported locale should return no breaks");
    }

    #[test]
    fn cached_dict_used_on_second_call() {
        let hp = KnuthLiangHyphenation::new();
        // Two calls with same locale should produce identical results.
        let first = hp.hyphenate("anfractuous", "en");
        let second = hp.hyphenate("anfractuous", "en");
        assert_eq!(first, second);
    }

    #[test]
    fn break_positions_are_valid_char_boundaries() {
        let hp = KnuthLiangHyphenation::new();
        let word = "hyphenation";
        let breaks = hp.hyphenate(word, "en");
        for &pos in &breaks {
            assert!(word.is_char_boundary(pos),
                    "break position {pos} is not a char boundary in '{word}'");
        }
    }

    #[test]
    fn provider_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<KnuthLiangHyphenation>();
    }
}
