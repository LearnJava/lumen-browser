//! CSS Fonts L4 §14 font-metric-override descriptor parser —
//! `ascent-override`/`descent-override`/`line-gap-override`/`size-adjust`.
//!
//! Only the `<percentage>` form is meaningful for metric substitution;
//! `normal` (the initial value) means "use the face's own metric", i.e. no
//! override — represented as `None`. A malformed value degrades to `None`
//! too: declarative `@font-face` has no exception mechanism (FONTLOAD-8), so
//! an invalid descriptor is treated as absent rather than rejecting the rule.
//! Mirrors the script-side contract in
//! `_lumen_font_face_parse_percent_descriptor` (`web_api_shim_mid.js`),
//! except that contract also preserves an invalid value for `.load()` to
//! reject later — there is no such deferred step on the CSS-connected path.
//!
//! `size-adjust`'s grammar has no `normal` keyword (default is `100%`), but
//! that is a non-issue for this parser: an out-of-grammar keyword is just
//! another malformed value, and malformed already degrades to `None` — which
//! the `size-adjust` call site reads as "no adjustment", i.e. the same 100%
//! default. No separate parser needed.

/// Parses an `ascent-override`/`descent-override`/`line-gap-override`/
/// `size-adjust` value.
///
/// Returns `Some(fraction)` for a valid non-negative `<percentage>` (`"90%"`
/// → `Some(0.9)`), `None` for `"normal"` or any unparseable value. For
/// `ascent-override`/`descent-override`/`line-gap-override`, `None` means "no
/// override, use the face's real metric"; for `size-adjust`, it means "no
/// adjustment, 100%" — both are the caller's default behavior.
pub fn parse_metric_override_percent(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("normal") {
        return None;
    }
    let digits = s.strip_suffix('%')?;
    let value: f32 = digits.parse().ok()?;
    if value < 0.0 {
        return None;
    }
    Some(value / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_value() {
        assert_eq!(parse_metric_override_percent("90%"), Some(0.9));
    }

    #[test]
    fn normal_keyword_is_no_override() {
        assert_eq!(parse_metric_override_percent("normal"), None);
    }

    #[test]
    fn normal_keyword_case_insensitive() {
        assert_eq!(parse_metric_override_percent("NORMAL"), None);
    }

    #[test]
    fn negative_percent_rejected() {
        assert_eq!(parse_metric_override_percent("-10%"), None);
    }

    #[test]
    fn missing_percent_sign_rejected() {
        assert_eq!(parse_metric_override_percent("90"), None);
    }

    #[test]
    fn empty_string_rejected() {
        assert_eq!(parse_metric_override_percent(""), None);
    }

    #[test]
    fn zero_percent_is_valid_override() {
        assert_eq!(parse_metric_override_percent("0%"), Some(0.0));
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(parse_metric_override_percent("  100%  "), Some(1.0));
    }

    #[test]
    fn size_adjust_150_percent() {
        assert_eq!(parse_metric_override_percent("150%"), Some(1.5));
    }

    #[test]
    fn size_adjust_out_of_grammar_keyword_degrades_to_none() {
        // `size-adjust` has no `normal` keyword — this is just another
        // malformed value, and malformed already means "no adjustment" (100%)
        // at the call site, same outcome as the literal default.
        assert_eq!(parse_metric_override_percent("normal"), None);
    }
}
