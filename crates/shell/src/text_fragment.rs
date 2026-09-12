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
//! Срез 2 adds the text-search half: [`find_directive_match`] locates a
//! [`TextDirective`] inside the page's already-rendered visible text
//! ([`lumen_layout::collect_visible_text`]), the same substrate the
//! regex find-in-page mode (`crate::find::find_matches_regex`) searches.
//! The reveal algorithm — `hidden="until-found"`, `beforematch`, opening the
//! nearest `<details>`, `::target-text` highlighting — remains a later
//! slice (see `bugs/BUG-972-OPEN.md`).

use lumen_layout::TextFragment;

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

/// Where a [`TextDirective`] was found in the page's visible text: the
/// screen rectangle of every [`TextFragment`] the match touches (in document
/// order), for scrolling and — in a later slice — highlighting.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveMatch {
    /// One rect per distinct [`TextFragment`] the match touches, in document order.
    pub rects: Vec<lumen_core::geom::Rect>,
}

impl DirectiveMatch {
    /// Smallest rectangle covering every fragment rect in the match.
    /// Panics-free: `rects` is never empty for a value produced by
    /// [`find_directive_match`].
    pub fn bounding_rect(&self) -> lumen_core::geom::Rect {
        let mut it = self.rects.iter();
        let Some(first) = it.next() else {
            return lumen_core::geom::Rect::ZERO;
        };
        let mut min_x = first.x;
        let mut min_y = first.y;
        let mut max_x = first.x + first.width;
        let mut max_y = first.y + first.height;
        for r in it {
            min_x = min_x.min(r.x);
            min_y = min_y.min(r.y);
            max_x = max_x.max(r.x + r.width);
            max_y = max_y.max(r.y + r.height);
        }
        lumen_core::geom::Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }
}

/// One character of the concatenated visible-text buffer, tagged with the
/// index (into the `frags` slice passed to [`find_directive_match`]) of the
/// [`TextFragment`] it came from. Characters inserted between fragments as a
/// word-boundary approximation (see module doc) carry the index of the
/// fragment immediately before them.
struct BufChar {
    ch: char,
    frag_index: usize,
}

/// Concatenates fragment text in document order, inserting a single space
/// between fragments so a directive spanning two runs (e.g. two adjacent
/// inline boxes with no literal whitespace between them, such as across a
/// block boundary) still matches — the same approximation the WICG algorithm
/// makes by treating block boundaries as word boundaries. A real inter-word
/// space already present in a fragment's own text is unaffected; this can
/// only ever produce an *extra* separator, never merge two words that should
/// stay apart, so it cannot manufacture a false match across a genuine word.
fn build_buffer(frags: &[TextFragment]) -> Vec<BufChar> {
    let mut out = Vec::new();
    for (idx, frag) in frags.iter().enumerate() {
        if frag.text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(BufChar { ch: ' ', frag_index: idx.saturating_sub(1) });
        }
        for ch in frag.text.chars() {
            out.push(BufChar { ch, frag_index: idx });
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `\b`-style word-boundary test at buffer position `pos` (a gap between
/// characters, `0..=buf.len()`).
fn is_boundary(buf: &[BufChar], pos: usize) -> bool {
    let before = pos.checked_sub(1).map(|i| is_word_char(buf[i].ch));
    let after = buf.get(pos).map(|b| is_word_char(b.ch));
    match (before, after) {
        (None, None) => true,
        (None, Some(a)) => a,
        (Some(b), None) => b,
        (Some(b), Some(a)) => b != a,
    }
}

fn chars_eq_ci(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_lowercase().eq(b.to_lowercase())
}

/// Finds the first occurrence of `needle` in `buf` at or after `from`, case
/// insensitive, whose start and end both land on a word boundary. Returns
/// the buffer index of the match's first character.
fn find_boundary_match(buf: &[BufChar], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > buf.len() {
        return None;
    }
    let last_start = buf.len() - needle.len();
    let mut i = from;
    while i <= last_start {
        if is_boundary(buf, i)
            && is_boundary(buf, i + needle.len())
            && (0..needle.len()).all(|k| chars_eq_ci(buf[i + k].ch, needle[k]))
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `true` when the text immediately before `pos` (after trimming trailing
/// whitespace) ends with `context`, case insensitive. `context: None` always
/// satisfies — an absent `prefix`/`suffix` imposes no constraint.
fn context_precedes(buf: &[BufChar], pos: usize, context: Option<&str>) -> bool {
    let Some(context) = context else { return true };
    let context_chars: Vec<char> = context.chars().collect();
    if context_chars.is_empty() {
        return true;
    }
    let mut end = pos;
    while end > 0 && buf[end - 1].ch.is_whitespace() {
        end -= 1;
    }
    if end < context_chars.len() {
        return false;
    }
    let start = end - context_chars.len();
    (0..context_chars.len()).all(|k| chars_eq_ci(buf[start + k].ch, context_chars[k]))
}

/// `true` when the text immediately after `pos` (after trimming leading
/// whitespace) starts with `context`, case insensitive.
fn context_follows(buf: &[BufChar], pos: usize, context: Option<&str>) -> bool {
    let Some(context) = context else { return true };
    let context_chars: Vec<char> = context.chars().collect();
    if context_chars.is_empty() {
        return true;
    }
    let mut start = pos;
    while start < buf.len() && buf[start].ch.is_whitespace() {
        start += 1;
    }
    if start + context_chars.len() > buf.len() {
        return false;
    }
    (0..context_chars.len()).all(|k| chars_eq_ci(buf[start + k].ch, context_chars[k]))
}

/// Locates a [`TextDirective`] inside the page's visible text.
///
/// `frags` is [`lumen_layout::collect_visible_text`]'s output for the
/// current layout tree, in document order. Implements the core of the WICG
/// Text Fragments matching algorithm: find the first word-boundary,
/// case-insensitive occurrence of `text_start` (constrained by `prefix`
/// immediately before it, modulo whitespace); if `text_end` is present,
/// extend the match to the first such occurrence of it at or after
/// `text_start`'s end (constrained by `suffix` immediately after). Returns
/// `None` when no candidate satisfies every constraint — including when
/// `frags` is empty or `text_start` doesn't occur at all.
///
/// Known gap vs. the full spec algorithm: no Unicode diacritic-insensitive
/// or full-width/compatibility normalization (same limitation as the
/// existing find-in-page regex mode, `crate::find`) — matching is
/// case-folding only.
pub fn find_directive_match(frags: &[TextFragment], directive: &TextDirective) -> Option<DirectiveMatch> {
    if frags.is_empty() {
        return None;
    }
    let buf = build_buffer(frags);
    let start_chars: Vec<char> = directive.text_start.chars().collect();
    if start_chars.is_empty() {
        return None;
    }
    let mut search_from = 0;
    while let Some(start_idx) = find_boundary_match(&buf, &start_chars, search_from) {
        let start_match_end = start_idx + start_chars.len();
        if context_precedes(&buf, start_idx, directive.prefix.as_deref()) {
            let match_end = match &directive.text_end {
                Some(text_end) => {
                    let end_chars: Vec<char> = text_end.chars().collect();
                    find_boundary_match(&buf, &end_chars, start_match_end)
                        .map(|e| e + end_chars.len())
                }
                None => Some(start_match_end),
            };
            if let Some(match_end) = match_end
                && context_follows(&buf, match_end, directive.suffix.as_deref())
            {
                let mut frag_indices: Vec<usize> = buf[start_idx..match_end]
                    .iter()
                    .map(|b| b.frag_index)
                    .collect();
                frag_indices.dedup();
                let rects = frag_indices.into_iter().map(|i| frags[i].rect).collect();
                return Some(DirectiveMatch { rects });
            }
        }
        search_from = start_idx + 1;
    }
    None
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

    // ── find_directive_match ────────────────────────────────────────────────

    fn frag(text: &str, x: f32, y: f32, w: f32, h: f32) -> TextFragment {
        use lumen_core::geom::Rect;
        use lumen_dom::NodeId;
        TextFragment {
            text: text.to_owned(),
            rect: Rect::new(x, y, w, h),
            node: NodeId::from_index(1),
            char_offset: 0,
        }
    }

    fn directive(text_start: &str) -> TextDirective {
        TextDirective {
            prefix: None,
            text_start: text_start.to_owned(),
            text_end: None,
            suffix: None,
        }
    }

    #[test]
    fn no_fragments_no_match() {
        let d = directive("hello");
        assert!(find_directive_match(&[], &d).is_none());
    }

    #[test]
    fn simple_word_match_within_one_fragment() {
        let frags = [frag("hello world", 0.0, 0.0, 100.0, 20.0)];
        let m = find_directive_match(&frags, &directive("world")).unwrap();
        assert_eq!(m.rects, vec![frags[0].rect]);
    }

    #[test]
    fn case_insensitive_match() {
        let frags = [frag("Hello World", 0.0, 0.0, 100.0, 20.0)];
        assert!(find_directive_match(&frags, &directive("WORLD")).is_some());
    }

    #[test]
    fn substring_without_word_boundary_does_not_match() {
        // "ell" is inside "hello" but doesn't start/end on a word boundary.
        let frags = [frag("hello world", 0.0, 0.0, 100.0, 20.0)];
        assert!(find_directive_match(&frags, &directive("ell")).is_none());
    }

    #[test]
    fn missing_text_is_no_match() {
        let frags = [frag("hello world", 0.0, 0.0, 100.0, 20.0)];
        assert!(find_directive_match(&frags, &directive("xyz")).is_none());
    }

    #[test]
    fn match_spans_two_fragments_with_inserted_boundary() {
        // Two adjacent runs with no literal space between them (e.g. across
        // an inline element boundary) — the search still finds a directive
        // whose words land one per fragment.
        let frags = [
            frag("hello", 0.0, 0.0, 50.0, 20.0),
            frag("world", 50.0, 0.0, 50.0, 20.0),
        ];
        let d = TextDirective {
            prefix: None,
            text_start: "hello".to_owned(),
            text_end: Some("world".to_owned()),
            suffix: None,
        };
        let m = find_directive_match(&frags, &d).unwrap();
        assert_eq!(m.rects, vec![frags[0].rect, frags[1].rect]);
    }

    #[test]
    fn range_match_covers_start_and_end_fragment() {
        let frags = [
            frag("one two three four", 0.0, 0.0, 200.0, 20.0),
        ];
        let d = TextDirective {
            prefix: None,
            text_start: "two".to_owned(),
            text_end: Some("three".to_owned()),
            suffix: None,
        };
        assert!(find_directive_match(&frags, &d).is_some());
    }

    #[test]
    fn prefix_constraint_rejects_wrong_context() {
        let frags = [frag("say hello world", 0.0, 0.0, 200.0, 20.0)];
        let matching = TextDirective {
            prefix: Some("say".to_owned()),
            text_start: "hello".to_owned(),
            text_end: None,
            suffix: None,
        };
        assert!(find_directive_match(&frags, &matching).is_some());

        let non_matching = TextDirective {
            prefix: Some("bye".to_owned()),
            text_start: "hello".to_owned(),
            text_end: None,
            suffix: None,
        };
        assert!(find_directive_match(&frags, &non_matching).is_none());
    }

    #[test]
    fn suffix_constraint_rejects_wrong_context() {
        let frags = [frag("hello world today", 0.0, 0.0, 200.0, 20.0)];
        let matching = TextDirective {
            prefix: None,
            text_start: "world".to_owned(),
            text_end: None,
            suffix: Some("today".to_owned()),
        };
        assert!(find_directive_match(&frags, &matching).is_some());

        let non_matching = TextDirective {
            prefix: None,
            text_start: "world".to_owned(),
            text_end: None,
            suffix: Some("yesterday".to_owned()),
        };
        assert!(find_directive_match(&frags, &non_matching).is_none());
    }

    #[test]
    fn first_of_multiple_occurrences_wins() {
        let frags = [frag("cat cat cat", 0.0, 0.0, 200.0, 20.0)];
        let m = find_directive_match(&frags, &directive("cat")).unwrap();
        // Any of the three would be a valid `rects` value (same fragment),
        // but the algorithm must not panic/loop and must return the fragment.
        assert_eq!(m.rects, vec![frags[0].rect]);
    }

    #[test]
    fn bounding_rect_unions_multiple_fragment_rects() {
        let m = DirectiveMatch {
            rects: vec![
                lumen_core::geom::Rect::new(0.0, 0.0, 50.0, 20.0),
                lumen_core::geom::Rect::new(60.0, 0.0, 40.0, 20.0),
            ],
        };
        let b = m.bounding_rect();
        assert!((b.x - 0.0).abs() < 0.01);
        assert!((b.width - 100.0).abs() < 0.01);
    }

    #[test]
    fn empty_fragment_text_is_skipped_in_buffer() {
        let frags = [
            frag("", 0.0, 0.0, 0.0, 0.0),
            frag("hello", 0.0, 0.0, 50.0, 20.0),
        ];
        let m = find_directive_match(&frags, &directive("hello")).unwrap();
        assert_eq!(m.rects, vec![frags[1].rect]);
    }
}
