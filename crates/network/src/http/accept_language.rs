//! `Accept-Language` shared by the H1, H2 and H3 header builders.
//!
//! The value used to be the constant [`DEFAULT_ACCEPT_LANGUAGE`] with no way to
//! replace it, while `navigator.languages` could already be overridden — so a
//! Russian-locale user sent `ru-RU` to scripts and `en-US` on the wire, a
//! combination no real browser produces (COMPAT-PRIVACY-UNBREAK-LOCALE).
//!
//! The browser shell pushes **one** ordered language list into both layers;
//! this module turns it into the header ([`accept_language_header`]) and keeps
//! the process-global value the builders read. The global is the same shape as
//! the `User-Agent` override: the fingerprint profile is process-wide and the
//! active browser profile can change at runtime, so every request — on any
//! thread, including workers — must see the switch without rebuilding clients.
//!
//! [`HttpProfile::TorBrowser`] never reads the override: Tor Browser pins one
//! locale for the whole population and a real locale there is a de-anonymiser.

use std::sync::{Arc, RwLock};

use super::headers::HttpProfile;

/// Process-global `Accept-Language` value; `None` — every profile keeps its
/// built-in default.
static GLOBAL_ACCEPT_LANGUAGE: RwLock<Option<Arc<str>>> = RwLock::new(None);

/// Longest language tag accepted (RFC 5646 permits 35 characters in practice).
const MAX_TAG_LEN: usize = 35;

/// Set (or clear with `None`) the process-global `Accept-Language` value.
///
/// A value containing control characters is ignored — it would split the
/// header block (CR/LF injection) — and leaves the previous value in place.
pub fn set_global_accept_language(value: Option<String>) {
    if value.as_deref().is_some_and(|v| v.chars().any(char::is_control)) {
        return;
    }
    if let Ok(mut guard) = GLOBAL_ACCEPT_LANGUAGE.write() {
        *guard = value.filter(|v| !v.trim().is_empty()).map(|v| Arc::from(v.trim()));
    }
}

/// The active process-global `Accept-Language` value, if any.
#[must_use]
pub fn global_accept_language() -> Option<String> {
    GLOBAL_ACCEPT_LANGUAGE.read().ok().and_then(|g| g.as_deref().map(str::to_owned))
}

/// `Accept-Language` for a request built under `profile`.
pub(super) fn accept_language_for(profile: HttpProfile) -> String {
    if profile == HttpProfile::TorBrowser {
        return super::TOR_BROWSER_ACCEPT_LANGUAGE.to_owned();
    }
    global_accept_language().unwrap_or_else(|| super::DEFAULT_ACCEPT_LANGUAGE.to_owned())
}

/// Is `tag` shaped like a BCP 47 language tag (`ru`, `ru-RU`, `zh-Hant-TW`)?
fn is_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= MAX_TAG_LEN
        && tag.split('-').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// Browser-style expansion of a preference list: a regional tag is followed by
/// its bare language unless the list already has it (`[ru-RU, en-US]` →
/// `[ru-RU, ru, en-US, en]`, as Chrome and Edge present it). Tags that are not
/// language tags are dropped, duplicates are kept once.
#[must_use]
pub fn expand_language_list(tags: &[String]) -> Vec<String> {
    fn push(out: &mut Vec<String>, tag: &str) {
        if !out.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            out.push(tag.to_owned());
        }
    }
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim().replace('_', "-");
        if !is_language_tag(&tag) {
            continue;
        }
        push(&mut out, &tag);
        if let Some((base, _)) = tag.split_once('-') {
            push(&mut out, base);
        }
    }
    out
}

/// Build the `Accept-Language` value from an ordered language list: the first
/// tag plain, the rest weighted `q=0.9, 0.8, …` down to a floor of `0.1`
/// (`["ru-RU", "ru", "en-US", "en"]` → `ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7`).
/// An empty or all-invalid list yields the built-in default.
#[must_use]
pub fn accept_language_header(languages: &[String]) -> String {
    let tags: Vec<&str> = languages
        .iter()
        .map(|t| t.trim())
        .filter(|t| is_language_tag(t))
        .collect();
    if tags.is_empty() {
        return super::DEFAULT_ACCEPT_LANGUAGE.to_owned();
    }
    let mut out = String::new();
    for (i, tag) in tags.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(tag);
        if i > 0 {
            let tenths = 10usize.saturating_sub(i).max(1);
            out.push_str(&format!(";q=0.{tenths}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| (*t).to_owned()).collect()
    }

    #[test]
    fn header_matches_chrome_weighting() {
        assert_eq!(accept_language_header(&v(&["en-US", "en"])), "en-US,en;q=0.9");
        assert_eq!(
            accept_language_header(&v(&["ru-RU", "ru", "en-US", "en"])),
            "ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7"
        );
        assert_eq!(accept_language_header(&v(&["de"])), "de");
    }

    #[test]
    fn weights_floor_at_one_tenth() {
        let tags: Vec<String> = (0..14).map(|i| format!("x{i}")).collect();
        let h = accept_language_header(&tags);
        assert!(h.ends_with("x13;q=0.1"), "{h}");
        assert!(h.contains("x9;q=0.1,x10;q=0.1"), "{h}");
    }

    #[test]
    fn empty_or_invalid_list_gives_default() {
        assert_eq!(accept_language_header(&[]), super::super::DEFAULT_ACCEPT_LANGUAGE);
        assert_eq!(
            accept_language_header(&v(&["", "bad\r\nX: 1", "a b"])),
            super::super::DEFAULT_ACCEPT_LANGUAGE
        );
    }

    #[test]
    fn expansion_inserts_base_once_after_first_variant() {
        assert_eq!(expand_language_list(&v(&["ru-RU", "en-US"])), v(&["ru-RU", "ru", "en-US", "en"]));
        assert_eq!(expand_language_list(&v(&["en-GB", "en-US"])), v(&["en-GB", "en", "en-US"]));
        assert_eq!(expand_language_list(&v(&["ru-RU", "ru", "en"])), v(&["ru-RU", "ru", "en"]));
        assert_eq!(expand_language_list(&v(&["ru_RU", "x y", "RU-ru"])), v(&["ru-RU", "ru"]));
    }
}
