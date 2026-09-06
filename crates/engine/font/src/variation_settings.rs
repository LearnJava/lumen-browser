//! `@font-face` `font-variation-settings` descriptor parser — CSS Fonts L4
//! §6.2 / §14 (variable-font axis defaults for a face).
//!
//! Grammar mirrors the CSS property's (`lumen_layout::parse_font_variation_settings`),
//! but lives here rather than in `lumen-layout` because the consumer —
//! `lumen_core::FaceRecord`, reached from `lumen-paint`'s glyph rasterization —
//! sits below `lumen-layout` in the dependency graph (`core → font → … →
//! layout → paint`). Same per-token leniency as [`crate::unicode_range::
//! parse_unicode_ranges`]: an unparseable token is skipped, not fatal to the
//! whole descriptor — declarative `@font-face` has no exception mechanism
//! (FONTLOAD-8), so "skip and keep going" is the only sensible degradation
//! for a comma list.

/// Parses a `font-variation-settings` descriptor value into `(tag, value)`
/// pairs.
///
/// Format (CSS Fonts L4 §14): `normal | [ <string> <number> ]#`, e.g.
/// `"wght" 375, "slnt" -8`. `normal` and an empty string both mean "no
/// descriptor defaults" — `Vec::new()`. Each entry needs a quoted 4-byte
/// ASCII tag followed by a number; a malformed entry is skipped, the rest of
/// the list is still parsed.
pub fn parse_variation_settings(s: &str) -> Vec<([u8; 4], f32)> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("normal") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for entry in s.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some(parsed) = parse_entry(entry) else { continue };
        out.push(parsed);
    }
    out
}

fn parse_entry(entry: &str) -> Option<([u8; 4], f32)> {
    let rest = entry
        .strip_prefix('"')
        .or_else(|| entry.strip_prefix('\''))?;
    let quote = entry.as_bytes()[0] as char;
    let end = rest.find(quote)?;
    let tag_str = &rest[..end];
    if tag_str.len() != 4 || !tag_str.is_ascii() {
        return None;
    }
    let tag_bytes = tag_str.as_bytes();
    let tag: [u8; 4] = [tag_bytes[0], tag_bytes[1], tag_bytes[2], tag_bytes[3]];
    let value: f32 = rest[end + 1..].trim().parse().ok()?;
    Some((tag, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_axis() {
        assert_eq!(parse_variation_settings(r#""wght" 375"#), vec![(*b"wght", 375.0)]);
    }

    #[test]
    fn multiple_axes() {
        let r = parse_variation_settings(r#""wght" 375, "slnt" -8"#);
        assert_eq!(r, vec![(*b"wght", 375.0), (*b"slnt", -8.0)]);
    }

    #[test]
    fn normal_keyword_is_empty() {
        assert!(parse_variation_settings("normal").is_empty());
    }

    #[test]
    fn normal_keyword_case_insensitive() {
        assert!(parse_variation_settings("NORMAL").is_empty());
    }

    #[test]
    fn empty_string_is_empty() {
        assert!(parse_variation_settings("").is_empty());
    }

    #[test]
    fn single_quoted_tag() {
        assert_eq!(parse_variation_settings("'wght' 700"), vec![(*b"wght", 700.0)]);
    }

    #[test]
    fn negative_value() {
        assert_eq!(parse_variation_settings(r#""slnt" -10"#), vec![(*b"slnt", -10.0)]);
    }

    #[test]
    fn malformed_entry_skipped_rest_kept() {
        let r = parse_variation_settings(r#"not-a-tag, "wght" 400"#);
        assert_eq!(r, vec![(*b"wght", 400.0)]);
    }

    #[test]
    fn tag_wrong_length_skipped() {
        assert!(parse_variation_settings(r#""ab" 1"#).is_empty());
    }

    #[test]
    fn missing_value_skipped() {
        assert!(parse_variation_settings(r#""wght""#).is_empty());
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(parse_variation_settings(r#"  "wght"   375  "#), vec![(*b"wght", 375.0)]);
    }
}
