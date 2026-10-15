//! The user's language list as the operating system reports it.
//!
//! Feeds `Accept-Language` and `navigator.languages` in the Personal and Work
//! profiles (COMPAT-PRIVACY-UNBREAK-LOCALE): a browser that reports `en-US` to
//! every site on a Russian Windows is the rarest signature of all, and sites
//! answer it with the wrong language. The list is read once per process.

use std::sync::OnceLock;

/// The OS preference list, most preferred first, as BCP 47 tags (`ru-RU`).
/// Empty when the system reports nothing usable — the caller keeps its default.
#[must_use]
pub fn preferred_languages() -> &'static [String] {
    static LANGUAGES: OnceLock<Vec<String>> = OnceLock::new();
    LANGUAGES.get_or_init(read_os_languages)
}

#[cfg(target_os = "windows")]
fn read_os_languages() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

    let mut count: u32 = 0;
    let mut len: u32 = 0;
    // SAFETY: the first call only asks for the buffer length (null buffer).
    let ok = unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, std::ptr::null_mut(), &mut len) };
    if ok == 0 || len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u16; len as usize];
    // SAFETY: `buf` holds exactly `len` UTF-16 units, as the first call required.
    let ok = unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buf.as_mut_ptr(), &mut len) };
    if ok == 0 {
        return Vec::new();
    }
    split_multi_sz(&buf)
}

#[cfg(not(target_os = "windows"))]
fn read_os_languages() -> Vec<String> {
    // POSIX precedence: LANGUAGE (colon list) > LC_ALL > LC_MESSAGES > LANG.
    if let Ok(list) = std::env::var("LANGUAGE") {
        let tags: Vec<String> = list.split(':').filter_map(parse_posix_locale).collect();
        if !tags.is_empty() {
            return tags;
        }
    }
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .find_map(|v| parse_posix_locale(&v))
        .into_iter()
        .collect()
}

/// Split a Windows `MULTI_SZ` buffer (`ru-RU\0en-US\0\0`) into its strings.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn split_multi_sz(buf: &[u16]) -> Vec<String> {
    buf.split(|&u| u == 0)
        .take_while(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// `ru_RU.UTF-8@euro` → `ru-RU`; `C`, `POSIX` and empty values → `None`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn parse_posix_locale(value: &str) -> Option<String> {
    let base = value.split(['.', '@']).next()?.trim();
    if base.is_empty() || base == "C" || base == "POSIX" {
        return None;
    }
    let tag = base.replace('_', "-");
    tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-').then_some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_sz_stops_at_the_double_nul() {
        let raw: Vec<u16> = "ru-RU\0en-US\0\0junk".encode_utf16().collect();
        assert_eq!(split_multi_sz(&raw), vec!["ru-RU".to_owned(), "en-US".to_owned()]);
        assert!(split_multi_sz(&[0, 0]).is_empty());
    }

    #[test]
    fn posix_locale_names_become_bcp47_tags() {
        assert_eq!(parse_posix_locale("ru_RU.UTF-8").as_deref(), Some("ru-RU"));
        assert_eq!(parse_posix_locale("de_DE@euro").as_deref(), Some("de-DE"));
        assert_eq!(parse_posix_locale("fr").as_deref(), Some("fr"));
        assert_eq!(parse_posix_locale("C"), None);
        assert_eq!(parse_posix_locale("POSIX"), None);
        assert_eq!(parse_posix_locale(""), None);
    }

    #[test]
    fn the_reader_never_panics_and_returns_clean_tags() {
        for tag in preferred_languages() {
            assert!(!tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{tag}");
        }
    }
}
