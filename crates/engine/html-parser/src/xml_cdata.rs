//! CDATA-wrapper stripping for XML-flavoured documents (GAP-XMLDOC, BUG-786).
//!
//! HTML5 §13.2.5 treats `<style>`/`<script>` as RAWTEXT: `<![CDATA[`/`]]>`
//! carry no special meaning there and stay literal text, so a CSS or JS
//! parser sees an invalid token at the very start of the block. Real XML
//! parsing strips the CDATA marker before the content reaches the embedded
//! language — this module reproduces just that one XML rule for documents
//! Lumen already identifies as XML-flavoured (`.xhtml`/`.xht`/`.svg`,
//! `application/xhtml+xml`, …), without introducing a separate XML tree
//! builder ([`crate::tree_builder`] still drives the parse).

/// Strips a single leading `<![CDATA[` / trailing `]]>` wrapper from `s`, if
/// present. Only whitespace is tolerated between the wrapper markers and the
/// content — the shape every corpus instance measured for BUG-786 uses
/// (`<style><![CDATA[ ... ]]></style>`). Text without a CDATA wrapper is
/// returned unchanged (borrowed, so the common — non-XML — case allocates
/// nothing extra beyond what the caller already holds).
pub(crate) fn strip_cdata_wrapper(s: &str) -> &str {
    let after_open = match s.trim_start().strip_prefix("<![CDATA[") {
        Some(rest) => rest,
        None => return s,
    };
    match after_open.trim_end().strip_suffix("]]>") {
        Some(inner) => inner,
        None => after_open,
    }
}

#[cfg(test)]
mod tests {
    use super::strip_cdata_wrapper;

    #[test]
    fn strips_wrapper() {
        assert_eq!(
            strip_cdata_wrapper("<![CDATA[\ndiv { color: red; }\n]]>"),
            "\ndiv { color: red; }\n"
        );
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        assert_eq!(
            strip_cdata_wrapper("  <![CDATA[x]]>  "),
            "x"
        );
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(strip_cdata_wrapper("div { color: red; }"), "div { color: red; }");
    }

    #[test]
    fn requires_closing_marker_to_strip_it() {
        // Malformed / truncated input — strip the opener only, keep the rest
        // verbatim rather than guessing where content ends.
        assert_eq!(strip_cdata_wrapper("<![CDATA[div{}"), "div{}");
    }

    #[test]
    fn does_not_strip_cdata_appearing_mid_content() {
        assert_eq!(
            strip_cdata_wrapper("a { content: \"<![CDATA[\"; }"),
            "a { content: \"<![CDATA[\"; }"
        );
    }
}
