//! Page Source Viewer (§D-2) — renders raw HTML with 4-colour syntax
//! highlighting in a dark-themed `<pre>` block.
//!
//! Colours follow the VS Code dark+ convention:
//! - **tag** names and angle-brackets → `#569cd6` (blue)
//! - **attribute** names → `#d7ba7d` (gold)
//! - **string** attribute values → `#ce9178` (salmon)
//! - **comments** `<!-- -->` → `#608b4e` (green)
//!
//! Entry point: [`build_view_source_html`].

/// Wrap `raw` HTML source in a syntax-highlighted page.
///
/// `url` is the original URL, shown in the `<title>`.
pub fn build_view_source_html(url: &str, raw: &str) -> String {
    let highlighted = highlight_html(raw);
    let url_escaped = html_escape(url);
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>view-source:{url}</title>
<style>
  body {{ background: #1e1e1e; margin: 0; padding: 0; color: #d4d4d4; }}
  pre {{
    font-family: 'Courier New', monospace;
    font-size: 0.875rem;
    line-height: 1.5;
    margin: 0;
    padding: 16px;
    white-space: pre-wrap;
    word-break: break-all;
  }}
  .vs-tag  {{ color: #569cd6; }}
  .vs-attr {{ color: #d7ba7d; }}
  .vs-str  {{ color: #ce9178; }}
  .vs-cmt  {{ color: #608b4e; }}
</style>
</head>
<body>
<pre>{highlighted}</pre>
</body>
</html>"#,
        url = url_escaped,
        highlighted = highlighted,
    )
}

// ── Tokeniser ─────────────────────────────────────────────────────────────────

/// Walk `src` char-by-char and emit HTML with `<span>` highlights.
fn highlight_html(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(src.len() * 2);
    let mut i = 0;

    while i < n {
        // ── HTML comment: <!-- ... --> ──────────────────────────────────────
        if i + 3 < n
            && chars[i] == '<'
            && chars[i + 1] == '!'
            && chars[i + 2] == '-'
            && chars[i + 3] == '-'
        {
            out.push_str("<span class=\"vs-cmt\">&lt;!--");
            i += 4;
            while i < n {
                if i + 2 < n && chars[i] == '-' && chars[i + 1] == '-' && chars[i + 2] == '>' {
                    out.push_str("--&gt;");
                    i += 3;
                    break;
                }
                push_char(&mut out, chars[i]);
                i += 1;
            }
            out.push_str("</span>");
            continue;
        }

        // ── Any other tag: <tagname …> or </tagname> or <!DOCTYPE …> ───────
        if chars[i] == '<' {
            i += 1; // skip '<'

            // Detect '/' for closing tags.
            let closing = i < n && chars[i] == '/';
            if closing {
                i += 1;
            }

            // Read tag name (may be empty for bare '<').
            let tag_start = i;
            while i < n
                && chars[i] != '>'
                && !chars[i].is_ascii_whitespace()
                && chars[i] != '/'
            {
                i += 1;
            }
            let tag_name: String = chars[tag_start..i].iter().collect();

            // Emit `<[/]tagname` in blue.
            out.push_str("<span class=\"vs-tag\">&lt;");
            if closing {
                out.push('/');
            }
            push_str(&mut out, &tag_name);
            out.push_str("</span>");

            // ── Attributes (only meaningful inside opening tags) ──────────
            loop {
                // Skip whitespace before next attribute or '>'.
                while i < n && chars[i].is_ascii_whitespace() {
                    push_char(&mut out, chars[i]);
                    i += 1;
                }

                if i >= n || chars[i] == '>' {
                    break;
                }

                // Self-closing '/>' — emit '/' in blue, then stop.
                if chars[i] == '/' && i + 1 < n && chars[i + 1] == '>' {
                    out.push_str("<span class=\"vs-tag\">/</span>");
                    i += 1; // '>' handled below
                    break;
                }

                // Read attribute name: everything up to '=', '>', '/' or whitespace.
                let attr_start = i;
                while i < n
                    && chars[i] != '='
                    && chars[i] != '>'
                    && chars[i] != '/'
                    && !chars[i].is_ascii_whitespace()
                {
                    i += 1;
                }

                if i > attr_start {
                    let name: String = chars[attr_start..i].iter().collect();
                    out.push_str("<span class=\"vs-attr\">");
                    push_str(&mut out, &name);
                    out.push_str("</span>");
                }

                // Attribute value after '='.
                if i < n && chars[i] == '=' {
                    out.push('=');
                    i += 1;
                    if i < n && (chars[i] == '"' || chars[i] == '\'') {
                        let q = chars[i];
                        out.push_str("<span class=\"vs-str\">");
                        push_char(&mut out, q);
                        i += 1;
                        while i < n && chars[i] != q {
                            push_char(&mut out, chars[i]);
                            i += 1;
                        }
                        if i < n {
                            push_char(&mut out, chars[i]); // closing quote
                            i += 1;
                        }
                        out.push_str("</span>");
                    } else {
                        // Unquoted value.
                        out.push_str("<span class=\"vs-str\">");
                        while i < n && chars[i] != '>' && !chars[i].is_ascii_whitespace() {
                            push_char(&mut out, chars[i]);
                            i += 1;
                        }
                        out.push_str("</span>");
                    }
                }
            }

            // Closing '>'.
            if i < n && chars[i] == '>' {
                out.push_str("<span class=\"vs-tag\">&gt;</span>");
                i += 1;
            }
            continue;
        }

        // ── Plain text ───────────────────────────────────────────────────────
        push_char(&mut out, chars[i]);
        i += 1;
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Append `ch` HTML-escaped into `out`.
fn push_char(out: &mut String, ch: char) {
    match ch {
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '&' => out.push_str("&amp;"),
        _ => out.push(ch),
    }
}

/// Append a string char-by-char through [`push_char`].
fn push_str(out: &mut String, s: &str) {
    for ch in s.chars() {
        push_char(out, ch);
    }
}

/// Escape `s` for use inside an HTML attribute or text node.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_view_source_html_contains_pre() {
        let out = build_view_source_html("https://example.com", "<p>Hi</p>");
        assert!(out.contains("<pre>"), "output must contain a <pre> element");
        assert!(out.contains("</pre>"), "output must close <pre>");
    }

    #[test]
    fn build_view_source_html_title_contains_url() {
        let out = build_view_source_html("https://example.com/page", "");
        assert!(
            out.contains("view-source:https://example.com/page"),
            "title should contain view-source: prefix"
        );
    }

    #[test]
    fn highlight_wraps_tag_name_in_vs_tag_span() {
        let out = highlight_html("<div>");
        assert!(out.contains("vs-tag"), "tag name should get vs-tag class");
        assert!(out.contains("div"), "tag name should be present");
    }

    #[test]
    fn highlight_wraps_attribute_name_in_vs_attr_span() {
        let out = highlight_html(r#"<div class="foo">"#);
        assert!(out.contains("vs-attr"), "attribute name should get vs-attr class");
        assert!(out.contains("vs-str"), "attribute value should get vs-str class");
    }

    #[test]
    fn highlight_wraps_comment_in_vs_cmt_span() {
        let out = highlight_html("<!-- a comment -->");
        assert!(out.contains("vs-cmt"), "comment should get vs-cmt class");
        assert!(out.contains("a comment"), "comment content should be preserved");
    }

    #[test]
    fn highlight_escapes_raw_angle_brackets_in_text() {
        let out = highlight_html("hello & world");
        assert!(out.contains("&amp;"), "& should be escaped");
    }

    #[test]
    fn highlight_closing_tag() {
        let out = highlight_html("</div>");
        assert!(out.contains('/'), "closing slash should appear");
        assert!(out.contains("vs-tag"), "closing tag should get vs-tag class");
    }

    #[test]
    fn highlight_comment_with_nested_dashes() {
        // Comments with content that looks like — but isn't — an end marker.
        let out = highlight_html("<!-- a - b -->");
        assert!(out.contains("vs-cmt"), "should still be a comment");
        assert!(out.contains("a - b"), "content preserved");
    }

    #[test]
    fn build_view_source_html_dark_background() {
        let out = build_view_source_html("x", "");
        assert!(out.contains("#1e1e1e"), "should use dark background");
    }

    #[test]
    fn highlight_self_closing_tag() {
        let out = highlight_html(r#"<br />"#);
        assert!(out.contains("vs-tag"), "self-closing tag should have vs-tag");
    }

    #[test]
    fn highlight_plain_text_passthrough() {
        let out = highlight_html("hello world");
        assert_eq!(out, "hello world");
    }

    #[test]
    fn highlight_single_quoted_attribute() {
        let out = highlight_html("<a href='foo'>");
        assert!(out.contains("vs-str"), "single-quoted value should get vs-str");
        assert!(out.contains("foo"), "value content preserved");
    }
}
