//! WICG Scroll To Text Fragment — URL fragment-directive parsing (`:~:text=...`).
//!
//! A URL like `#intro:~:text=hello,-world` carries two independent pieces
//! glued together with the `:~:` marker: an ordinary element-id fragment
//! (`intro`, may be empty) and one or more `text=` directives separated by
//! `&`. Everything past `:~:` must never reach `:target` matching or
//! [`crate::links::find_element_by_id`] — no real `id` attribute contains
//! `:~:`, but the literal directive text can coincidentally collide with an
//! `id` value, so the two must be split explicitly rather than left for a
//! failed lookup to shrug off.
//!
//! This module only parses; the text search and reveal algorithm (STTF-1
//! remaining scope, see `bugs/BUG-972-OPEN.md`) are separate slices.

/// One `text=` fragment directive (WICG Text Fragments §3.2 `TextDirective`).
///
/// `prefix`/`suffix` are context hints for disambiguating a match, not part
/// of the highlighted range itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDirective {
    /// Text expected to immediately precede the match (`prefix-,` form).
    pub prefix: Option<String>,
    /// Start of the text range to find. Never empty for a well-formed directive.
    pub text_start: String,
    /// End of the text range, when the match spans a phrase (`start,end` form).
    pub text_end: Option<String>,
    /// Text expected to immediately follow the match (`,-suffix` form).
    pub suffix: Option<String>,
}

/// Result of splitting a URL fragment into its id part and directive list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedFragment {
    /// The element-id portion before `:~:`, or the whole fragment when no
    /// directive marker is present. `None` when that portion is empty (a
    /// fragment that is only `:~:text=...`, or the empty string).
    pub element_id: Option<String>,
    /// Directives found after `:~:`, in order. Directives that don't parse
    /// (missing `text=` prefix, wrong shape) are silently dropped, per the
    /// spec's error-recovery stance — a malformed directive must not break
    /// navigation to the id part.
    pub directives: Vec<TextDirective>,
}

/// Split a URL fragment (without the leading `#`) into its element-id part
/// and `:~:text=` directives.
pub fn parse_fragment(fragment: &str) -> ParsedFragment {
    let (id_part, directive_part) = match fragment.split_once(":~:") {
        Some((id, rest)) => (id, Some(rest)),
        None => (fragment, None),
    };
    let element_id = if id_part.is_empty() { None } else { Some(id_part.to_owned()) };
    let directives = directive_part
        .map(|rest| rest.split('&').filter_map(parse_text_directive).collect())
        .unwrap_or_default();
    ParsedFragment { element_id, directives }
}

/// Parse a single `&`-separated component of the directive part. Only the
/// `text=` directive exists today (WICG defines no others yet); anything
/// else, or a `text=` value with the wrong number of comma-separated parts,
/// returns `None` rather than a best-effort guess.
fn parse_text_directive(component: &str) -> Option<TextDirective> {
    let value = component.strip_prefix("text=")?;
    let mut parts: Vec<&str> = value.split(',').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let suffix = if parts.len() > 1 && parts[parts.len() - 1].starts_with('-') {
        let raw = parts.pop()?;
        Some(percent_decode(&raw[1..]))
    } else {
        None
    };
    let prefix = if parts.len() > 1 && parts[0].ends_with('-') {
        let raw = parts.remove(0);
        Some(percent_decode(&raw[..raw.len() - 1]))
    } else {
        None
    };
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let text_start = percent_decode(parts[0]);
    if text_start.is_empty() {
        return None;
    }
    let text_end = parts.get(1).map(|s| percent_decode(s));
    Some(TextDirective { prefix, text_start, text_end, suffix })
}

/// Minimal percent-decoder for fragment-directive components: `%XX` → byte,
/// invalid/incomplete escapes pass through literally (same tolerant stance
/// as `lumen_network::percent_decode_bytes`, reimplemented here to avoid a
/// cross-crate dependency for ~10 lines). Unlike query-string decoding,
/// `+` is NOT treated as space — the fragment directive grammar has no such
/// rule.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_id_without_directive_is_unaffected() {
        let p = parse_fragment("intro");
        assert_eq!(p.element_id.as_deref(), Some("intro"));
        assert!(p.directives.is_empty());
    }

    #[test]
    fn empty_fragment_has_no_id_and_no_directives() {
        let p = parse_fragment("");
        assert_eq!(p.element_id, None);
        assert!(p.directives.is_empty());
    }

    #[test]
    fn id_with_single_text_directive() {
        let p = parse_fragment("intro:~:text=hello");
        assert_eq!(p.element_id.as_deref(), Some("intro"));
        assert_eq!(
            p.directives,
            vec![TextDirective {
                prefix: None,
                text_start: "hello".to_owned(),
                text_end: None,
                suffix: None,
            }]
        );
    }

    #[test]
    fn directive_only_fragment_has_no_element_id() {
        let p = parse_fragment(":~:text=hello");
        assert_eq!(p.element_id, None);
        assert_eq!(p.directives.len(), 1);
    }

    #[test]
    fn text_start_and_end_range() {
        let p = parse_fragment(":~:text=hello,world");
        assert_eq!(p.directives[0].text_start, "hello");
        assert_eq!(p.directives[0].text_end.as_deref(), Some("world"));
    }

    #[test]
    fn prefix_and_suffix_context_hints() {
        let p = parse_fragment(":~:text=before-,hello,-after");
        let d = &p.directives[0];
        assert_eq!(d.prefix.as_deref(), Some("before"));
        assert_eq!(d.text_start, "hello");
        assert_eq!(d.text_end, None);
        assert_eq!(d.suffix.as_deref(), Some("after"));
    }

    #[test]
    fn full_four_part_directive() {
        let p = parse_fragment(":~:text=pre-,hello,world,-post");
        let d = &p.directives[0];
        assert_eq!(d.prefix.as_deref(), Some("pre"));
        assert_eq!(d.text_start, "hello");
        assert_eq!(d.text_end.as_deref(), Some("world"));
        assert_eq!(d.suffix.as_deref(), Some("post"));
    }

    #[test]
    fn multiple_directives_joined_with_ampersand() {
        let p = parse_fragment(":~:text=foo&text=bar");
        assert_eq!(p.directives.len(), 2);
        assert_eq!(p.directives[0].text_start, "foo");
        assert_eq!(p.directives[1].text_start, "bar");
    }

    #[test]
    fn percent_encoded_comma_and_hyphen_survive_in_text() {
        // A literal comma/hyphen inside the matched text must be escaped by
        // the producer (`%2C`/`%2D`) so it isn't mistaken for a delimiter.
        let p = parse_fragment(":~:text=a%2Cb%2Dc");
        assert_eq!(p.directives[0].text_start, "a,b-c");
    }

    #[test]
    fn non_text_directive_is_dropped() {
        let p = parse_fragment("intro:~:selector=foo");
        assert_eq!(p.element_id.as_deref(), Some("intro"));
        assert!(p.directives.is_empty());
    }

    #[test]
    fn malformed_directive_with_too_many_parts_is_dropped() {
        let p = parse_fragment(":~:text=a,b,c,d,e");
        assert!(p.directives.is_empty());
    }

    #[test]
    fn empty_text_start_is_dropped() {
        let p = parse_fragment(":~:text=");
        assert!(p.directives.is_empty());
    }

    #[test]
    fn only_marker_yields_no_id_and_no_directives() {
        let p = parse_fragment(":~:");
        assert_eq!(p.element_id, None);
        assert!(p.directives.is_empty());
    }
}
