//! Custom XML general entity expansion for XML-flavoured documents
//! (GAP-XMLDOC срез 28, BUG-786).
//!
//! XML §4.2.2 lets a DOCTYPE internal subset declare a general entity
//! (`<!ENTITY name "value">`), and every later `&name;` reference in the
//! document is replaced by `value` *before* the document is parsed as
//! markup — so a replacement text containing tags (the corpus shape this
//! module targets) becomes real elements, not literal text. The tokenizer's
//! own DOCTYPE state (`Tokenizer::consume_doctype`, GAP-XMLDOC срез 21)
//! already walks the same bracketed internal subset to find its end, but
//! only to skip it — it never reads a declaration out of it. Reusing that
//! walk inside the tokenizer's per-character cursor would mean threading a
//! splice-capable input source through every position-based helper there
//! (`rest()`, `consume_tag_name`, attribute parsing, …); this module instead
//! resolves entities as a textual pre-pass over the whole document string,
//! which is what pull-mode ([`crate::tree_builder::parse_xml_flavoured`])
//! already has in hand as one contiguous `&str` before tokenization starts.

use std::borrow::Cow;
use std::collections::HashMap;

/// Case-insensitive substring search that returns a byte offset valid on
/// `haystack` itself — `str::to_ascii_lowercase` only rewrites ASCII bytes,
/// so it never changes any byte's position or width.
fn find_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    haystack.to_ascii_lowercase().find(needle_lower)
}

/// Reads a `"..."`/`'...'` quoted string starting at `s`'s first char.
/// Returns `(content, rest-after-closing-quote)`.
fn consume_quoted(s: &str) -> Option<(&str, &str)> {
    let mut chars = s.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let start = quote.len_utf8();
    let body = &s[start..];
    let end = body.find(quote)?;
    Some((&body[..end], &body[end + quote.len_utf8()..]))
}

/// Longest chain of `&a;` → `&b;` → … this module will follow before it
/// gives up and leaves the reference literal (GAP-XMLDOC срез 30). XML §4.1
/// makes a recursive entity a *fatal* error rather than capping the depth;
/// a cap is the lenient equivalent that cannot hang, and no real document
/// nests general entities anywhere near this deep.
const MAX_ENTITY_DEPTH: usize = 8;

/// Total number of references this module will expand across one document.
/// Together with [`MAX_ENTITY_DEPTH`] this bounds the classic "billion
/// laughs" shape (`<!ENTITY aN "&aN-1;&aN-1;">`), whose output is
/// exponential in the depth alone. Measured corpus documents expand
/// single-digit counts; 1x1-green.svg, the densest, expands 10.
const MAX_ENTITY_EXPANSIONS: usize = 100_000;

/// Longest declared entity name this module accepts, and therefore the
/// widest window in which a `&` can still be the start of a reference.
const MAX_ENTITY_NAME_LEN: usize = 64;

/// XML §2.3 `Name` is far broader than this, but every corpus instance
/// measured for this срез uses a plain ASCII identifier — same narrowing
/// principle as `foreign_content::strip_known_html_prefix` (hardcode the
/// measured shape, not a general resolver).
fn is_valid_entity_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ENTITY_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Finds the DOCTYPE's internal subset (`<!DOCTYPE … [ … ]>`) and extracts
/// every `<!ENTITY name "value">` declared in it. Empty if there is no
/// DOCTYPE, no internal subset, or no entity declarations — the overwhelmingly
/// common case, so callers should skip the (allocating) expansion pass
/// entirely when this comes back empty.
fn parse_declared_entities(input: &str) -> HashMap<String, String> {
    let mut entities = HashMap::new();
    let Some(doctype_pos) = find_ci(input, "<!doctype") else {
        return entities;
    };
    let after_doctype = &input[doctype_pos..];
    let Some(bracket_rel) = after_doctype.find('[') else {
        return entities;
    };
    let subset_start = doctype_pos + bracket_rel + '['.len_utf8();

    // Same balanced-`<...>` walk as `Tokenizer::consume_doctype` (GAP-XMLDOC
    // срез 21) to find the subset's closing `]` — a `<?x y?>`/`<!--x-->`
    // inside the subset may itself contain `>`, which must not be mistaken
    // for the subset's own end.
    let mut depth: i32 = 0;
    let mut subset_end = input.len();
    for (i, c) in input[subset_start..].char_indices() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            ']' if depth == 0 => {
                subset_end = subset_start + i;
                break;
            }
            _ => {}
        }
    }
    let subset = &input[subset_start..subset_end];

    let mut rest = subset;
    while let Some(pos) = find_ci(rest, "<!entity") {
        let after_kw = rest[pos + "<!entity".len()..].trim_start();
        let name_end = after_kw
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(after_kw.len());
        let name = &after_kw[..name_end];
        let after_name = after_kw[name_end..].trim_start();
        match consume_quoted(after_name) {
            Some((value, tail)) => {
                if is_valid_entity_name(name) {
                    entities.entry(name.to_string()).or_insert_with(|| value.to_string());
                }
                rest = tail;
            }
            // Malformed/parameter-entity/no-quoted-value declaration — skip
            // just the keyword and keep scanning, same lenient stance as the
            // rest of this crate's tokenizer.
            None => rest = &rest[pos + "<!entity".len()..],
        }
    }
    entities
}

/// Length of the `<!DOCTYPE … >` declaration starting at `s`'s first byte,
/// internal subset included. Mirrors `Tokenizer::consume_doctype`'s walk
/// (GAP-XMLDOC срез 21): a `<?x?>`/`<!--x-->`/`<!ENTITY …>` inside `[ … ]`
/// carries its own `>`, which is not the declaration's end. Unterminated
/// input yields the whole remaining string, the same lenient stance the
/// tokenizer takes at EOF.
fn doctype_declaration_len(s: &str) -> usize {
    const KEYWORD: &str = "<!doctype";
    let mut in_subset = false;
    let mut depth: i32 = 0;
    for (i, c) in s[KEYWORD.len()..].char_indices() {
        match c {
            '[' if !in_subset => in_subset = true,
            '<' if in_subset => depth += 1,
            '>' if in_subset && depth > 0 => depth -= 1,
            ']' if in_subset && depth == 0 => in_subset = false,
            '>' if !in_subset => return KEYWORD.len() + i + c.len_utf8(),
            _ => {}
        }
    }
    s.len()
}

/// Length of the construct at `s`'s first byte (always a `<`) **inside which
/// XML does not recognise an entity reference at all**, or `None` if this
/// `<` opens ordinary markup.
///
/// The four constructs (GAP-XMLDOC срез 30, BUG-786):
/// - a comment, XML §2.5 — its content is not parsed for references;
/// - a CDATA section, XML §2.7 — the whole point of it is that `&` and `<`
///   inside are literal characters;
/// - a processing instruction, XML §2.6 — likewise literal;
/// - the DOCTYPE declaration itself, XML §4.4.7 "Bypassed" — a reference
///   inside an `<!ENTITY …>` value literal is *not* expanded where it
///   stands, only when the entity it declares is used.
///
/// An unterminated construct swallows the rest of the input rather than
/// falling back to per-character scanning: re-entering expansion inside a
/// construct the tokenizer will itself consume to EOF would reintroduce
/// exactly the defect this guards against.
fn opaque_span_len(s: &str) -> Option<usize> {
    fn span_to(s: &str, open: &str, close: &str) -> usize {
        s[open.len()..].find(close).map_or(s.len(), |o| open.len() + o + close.len())
    }
    if s.starts_with("<!--") {
        return Some(span_to(s, "<!--", "-->"));
    }
    if s.starts_with("<![CDATA[") {
        return Some(span_to(s, "<![CDATA[", "]]>"));
    }
    if s.starts_with("<?") {
        return Some(span_to(s, "<?", "?>"));
    }
    // `<!DOCTYPE` is pure ASCII, so comparing raw bytes is both
    // case-insensitive-correct and char-boundary safe.
    let keyword = b"<!doctype";
    if s.len() >= keyword.len() && s.as_bytes()[..keyword.len()].eq_ignore_ascii_case(keyword) {
        return Some(doctype_declaration_len(s));
    }
    None
}

/// Walks `input`, copying it into `out` while replacing every `&name;`
/// reference to a declared entity with that entity's replacement text —
/// which is itself walked the same way, so a chain `&a;` → `&b;` → `<b/>`
/// resolves to markup (XML §4.4.2 "Included"), bounded by
/// [`MAX_ENTITY_DEPTH`] and `budget`. Spans listed by [`opaque_span_len`]
/// are copied verbatim.
fn expand_into(
    out: &mut String,
    input: &str,
    entities: &HashMap<String, String>,
    depth: usize,
    budget: &mut usize,
) {
    let mut i = 0;
    while i < input.len() {
        let rest = &input[i..];
        match rest.as_bytes().first() {
            Some(b'<') => match opaque_span_len(rest) {
                Some(len) => {
                    out.push_str(&rest[..len]);
                    i += len;
                }
                None => {
                    out.push('<');
                    i += 1;
                }
            },
            Some(b'&') => {
                // A declared name is at most `MAX_ENTITY_NAME_LEN` bytes
                // (`is_valid_entity_name`), so the `;` can only be inside that
                // window — bounding the search keeps a document full of stray
                // `&` from costing a full-tail scan each time.
                let after = &rest[1..];
                let mut window_end = (MAX_ENTITY_NAME_LEN + 1).min(after.len());
                while !after.is_char_boundary(window_end) {
                    window_end -= 1;
                }
                let window = &after[..window_end];
                let resolved = window.find(';').and_then(|semi| {
                    entities.get(&window[..semi]).map(|value| (value, 1 + semi + 1))
                });
                match resolved {
                    Some((value, consumed)) if depth < MAX_ENTITY_DEPTH && *budget > 0 => {
                        *budget -= 1;
                        expand_into(out, value, entities, depth + 1, budget);
                        i += consumed;
                    }
                    // Undeclared name, or a chain too deep / too wide to be
                    // anything but an entity bomb: leave the reference
                    // literal, exactly as an unknown `&foo;` already was.
                    _ => {
                        out.push('&');
                        i += 1;
                    }
                }
            }
            _ => {
                let next = rest.find(['<', '&']).map_or(input.len(), |o| i + o);
                out.push_str(&input[i..next]);
                i = next;
            }
        }
    }
}

/// Replaces every `&name;` reference to a custom-declared general entity
/// with its literal replacement text, so the tokenizer sees that text as
/// ordinary input and parses it as markup (or plain text, if that's all the
/// entity contains) like any other part of the document. Built-in named
/// character references (`&amp;`, `&lt;`, …) are untouched — they are never
/// present in the `entities` map this builds, since it only reads `<!ENTITY
/// …>` declarations, never HTML5's predefined table.
///
/// Borrows (no allocation) when the document has no declared entities at
/// all — true for every non-XML document and the overwhelming majority of
/// XML-flavoured ones.
pub(crate) fn expand_custom_general_entities(input: &str) -> Cow<'_, str> {
    let entities = parse_declared_entities(input);
    if entities.is_empty() {
        return Cow::Borrowed(input);
    }

    let mut out = String::with_capacity(input.len());
    let mut budget = MAX_ENTITY_EXPANSIONS;
    expand_into(&mut out, input, &entities, 0, &mut budget);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_doctype_no_allocation() {
        assert!(matches!(expand_custom_general_entities("<p>hi</p>"), Cow::Borrowed(_)));
    }

    #[test]
    fn no_internal_subset_no_allocation() {
        let input = "<!DOCTYPE html><p>hi</p>";
        assert!(matches!(expand_custom_general_entities(input), Cow::Borrowed(_)));
    }

    #[test]
    fn expands_markup_entity_reference() {
        let input = "<!DOCTYPE html [\n<!ENTITY tree \"<span id='x'>unknown.</span>\">\n]>\n<p>result: &tree;</p>";
        let expanded = expand_custom_general_entities(input);
        assert_eq!(
            expanded,
            "<!DOCTYPE html [\n<!ENTITY tree \"<span id='x'>unknown.</span>\">\n]>\n<p>result: <span id='x'>unknown.</span></p>"
        );
    }

    #[test]
    fn leaves_predefined_named_references_untouched() {
        let input = "<!DOCTYPE svg [\n<!ENTITY tree \"x\">\n]>\n<p>a &amp; b</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.contains("a &amp; b"));
    }

    #[test]
    fn unknown_reference_left_literal() {
        let input = "<!DOCTYPE html [\n<!ENTITY tree \"x\">\n]>\n<p>&nope;</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.contains("&nope;"));
    }

    #[test]
    fn single_quoted_entity_value() {
        let input = "<!DOCTYPE html [\n<!ENTITY tree 'x'>\n]>\n<p>&tree;</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.ends_with("<p>x</p>"));
    }

    #[test]
    fn pi_inside_subset_does_not_confuse_bracket_depth() {
        let input = "<!DOCTYPE html [\n<?pi some > content?>\n<!ENTITY tree \"x\">\n]>\n<p>&tree;</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.ends_with("<p>x</p>"));
    }

    // ---- GAP-XMLDOC срез 30 ----

    #[test]
    fn reference_inside_cdata_section_is_left_literal() {
        let input = "<!DOCTYPE html [\n<!ENTITY t \"<i>M</i>\">\n]>\n<p><![CDATA[a&t;b]]></p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.ends_with("<p><![CDATA[a&t;b]]></p>"), "{expanded}");
    }

    #[test]
    fn reference_inside_comment_is_left_literal() {
        let input = "<!DOCTYPE html [\n<!ENTITY t \"--><script>x</script>\">\n]>\n<!-- &t; --><p>z</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.ends_with("<!-- &t; --><p>z</p>"), "{expanded}");
    }

    #[test]
    fn reference_inside_processing_instruction_is_left_literal() {
        let input = "<!DOCTYPE html [\n<!ENTITY h \"evil.css\">\n]>\n<?xml-stylesheet href=\"&h;\"?><p>z</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.contains("<?xml-stylesheet href=\"&h;\"?>"), "{expanded}");
    }

    #[test]
    fn declaration_value_is_bypassed_not_expanded_in_place() {
        // XML §4.4.7: the `&b;` written inside `a`'s value literal stays put;
        // it is resolved when `&a;` is *used*, not where it is declared.
        let input = "<!DOCTYPE html [\n<!ENTITY b \"X\">\n<!ENTITY a \"&b;\">\n]>\n<p>&a;</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.contains("<!ENTITY a \"&b;\">"), "{expanded}");
        assert!(expanded.ends_with("<p>X</p>"), "{expanded}");
    }

    #[test]
    fn nested_reference_resolves_through_the_chain() {
        let input = "<!DOCTYPE html [\n<!ENTITY c \"<b>deep</b>\">\n<!ENTITY b \"&c;\">\n<!ENTITY a \"&b;\">\n]>\n<p>[&a;]</p>";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.ends_with("<p>[<b>deep</b>]</p>"), "{expanded}");
    }

    #[test]
    fn self_referential_entity_terminates() {
        let input = "<!DOCTYPE html [\n<!ENTITY a \"x&a;\">\n]>\n<p>&a;</p>";
        let expanded = expand_custom_general_entities(input);
        // Depth-capped, so the innermost reference survives literally; the
        // only assertion that matters is that this returns at all.
        assert!(expanded.ends_with("<p>xxxxxxxx&a;</p>"), "{expanded}");
    }

    #[test]
    fn entity_bomb_is_bounded() {
        let mut input = String::from("<!DOCTYPE html [\n<!ENTITY a0 \"xxxxxxxxxx\">\n");
        for level in 1..MAX_ENTITY_DEPTH {
            input.push_str(&format!(
                "<!ENTITY a{level} \"&a{prev};&a{prev};&a{prev};&a{prev};\">\n",
                prev = level - 1
            ));
        }
        input.push_str(&format!("]>\n<p>&a{};</p>", MAX_ENTITY_DEPTH - 1));
        let expanded = expand_custom_general_entities(&input);
        assert!(expanded.len() < 4 * 1024 * 1024, "expanded to {} bytes", expanded.len());
    }

    #[test]
    fn unterminated_cdata_section_swallows_the_rest() {
        let input = "<!DOCTYPE html [\n<!ENTITY t \"M\">\n]>\n<p><![CDATA[&t;";
        let expanded = expand_custom_general_entities(input);
        assert!(expanded.ends_with("<p><![CDATA[&t;"), "{expanded}");
    }
}
