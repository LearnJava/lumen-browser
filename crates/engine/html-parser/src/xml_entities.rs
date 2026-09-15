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

/// XML §2.3 `Name` is far broader than this, but every corpus instance
/// measured for this срез uses a plain ASCII identifier — same narrowing
/// principle as `foreign_content::strip_known_html_prefix` (hardcode the
/// measured shape, not a general resolver).
fn is_valid_entity_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
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
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let matched = after.find(';').and_then(|semi| {
            let name = &after[..semi];
            entities.get(name).map(|value| (value, &after[semi + 1..]))
        });
        match matched {
            Some((value, tail)) => {
                out.push_str(value);
                rest = tail;
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
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
}
