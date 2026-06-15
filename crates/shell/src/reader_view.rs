//! Reader View (§D-3) — strips clutter from an HTML page and renders a clean
//! article layout optimised for reading (max-width 680 px, 1.1 rem body font,
//! line-height 1.6).
//!
//! Entry points:
//! - [`extract_article`] — parse raw HTML and return an [`ArticleContent`] if
//!   an article region is found.
//! - [`build_reader_html`] — wrap extracted content in the reader template.
//!
//! The extraction is intentionally lightweight (no full DOM parse).  The
//! algorithm scans byte-level tag boundaries to locate `<article>`, `<main>`,
//! or `role="main"` regions, then strips noise tags from the inner HTML.

// ── Public types ─────────────────────────────────────────────────────────────

/// Article content extracted from a raw HTML page.
#[derive(Debug, Clone)]
pub struct ArticleContent {
    /// Page or article title (from `<title>`, or first `<h1>`/`<h2>` found).
    pub title: String,
    /// Inner HTML of the detected article region, stripped of noise tags.
    pub body_html: String,
    /// Optional author name from `<meta name="author">`.
    pub author: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse `html` and extract the main article content.
///
/// Looks for (in priority order):
/// 1. `<article>…</article>`
/// 2. `<main>…</main>`
/// 3. An element with `role="main"` (any tag, balanced end-tag extraction)
///
/// Returns `None` when none of the above regions can be found.
pub fn extract_article(html: &str) -> Option<ArticleContent> {
    let lower = html.to_ascii_lowercase();

    let body_html = find_article_region(&lower, html)
        .map(strip_noise_tags)
        .filter(|s| !s.trim().is_empty())?;

    let title = extract_title(html, &body_html);
    let author = extract_author_meta(html);

    Some(ArticleContent { title, body_html, author })
}

/// Wrap an [`ArticleContent`] in the reader template and return a
/// self-contained HTML string.
pub fn build_reader_html(article: &ArticleContent) -> String {
    let title_escaped = html_escape(&article.title);
    let author_line = article.author.as_deref().map(|a| {
        format!("<p class=\"reader-meta\">By {}</p>", html_escape(a))
    }).unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  body {{
    background: #f8f7f4;
    color: #2c2c2c;
    font-family: Georgia, 'Times New Roman', serif;
    margin: 0;
    padding: 0;
  }}
  .reader-container {{
    max-width: 680px;
    margin: 40px auto;
    padding: 20px 32px;
  }}
  .reader-title {{
    font-size: 1.8rem;
    line-height: 1.3;
    margin: 0 0 8px 0;
    color: #1a1a1a;
  }}
  .reader-meta {{
    color: #888;
    font-size: 0.9rem;
    margin: 0 0 32px 0;
  }}
  .reader-body {{
    font-size: 1.1rem;
    line-height: 1.6;
  }}
  .reader-body p {{ margin: 0 0 1em 0; }}
  .reader-body h1, .reader-body h2, .reader-body h3 {{
    line-height: 1.3;
    margin: 1.5em 0 0.5em 0;
  }}
  .reader-body img {{ max-width: 100%; height: auto; display: block; margin: 1em 0; }}
  .reader-body a {{ color: #4a90d9; }}
  .reader-body pre, .reader-body code {{
    font-family: 'Courier New', monospace;
    font-size: 0.95rem;
    background: #f0efec;
    border-radius: 3px;
  }}
  .reader-body pre {{ padding: 12px; overflow-x: auto; }}
  .reader-body code {{ padding: 1px 4px; }}
  .reader-body blockquote {{
    border-left: 3px solid #ccc;
    margin: 1em 0;
    padding-left: 16px;
    color: #555;
  }}
</style>
</head>
<body>
<div class="reader-container">
  <h1 class="reader-title">{title}</h1>
  {author_line}<div class="reader-body">{body}</div>
</div>
</body>
</html>"#,
        title = title_escaped,
        author_line = author_line,
        body = article.body_html,
    )
}

// ── Extraction helpers ────────────────────────────────────────────────────────

/// Find the inner HTML of the article region. `lower` is `html.to_ascii_lowercase()`.
fn find_article_region<'a>(lower: &str, html: &'a str) -> Option<&'a str> {
    // Priority 1: <article>
    if let Some(inner) = extract_tag_inner(lower, html, "article") {
        return Some(inner);
    }
    // Priority 2: <main>
    if let Some(inner) = extract_tag_inner(lower, html, "main") {
        return Some(inner);
    }
    // Priority 3: role="main"
    extract_role_main_inner(lower, html)
}

/// Extract the content between the first matching open/close tag pair.
///
/// Handles nesting: `<article>` inside `<article>` increments a depth counter.
fn extract_tag_inner<'a>(lower: &str, html: &'a str, tag: &str) -> Option<&'a str> {
    let open_pat = format!("<{}", tag);
    let close_pat = format!("</{}>", tag);

    let start_lower = lower.find(open_pat.as_str())?;
    // Find the end of the opening tag (skip to `>`).
    let gt = lower[start_lower..].find('>')?;
    let content_start = start_lower + gt + 1;

    // Find balanced closing tag.
    let mut depth = 1usize;
    let mut search_from = content_start;
    loop {
        let next_open = lower[search_from..].find(open_pat.as_str());
        let next_close = lower[search_from..].find(close_pat.as_str());

        match (next_open, next_close) {
            (_, None) => return None, // no closing tag found
            (Some(no), Some(nc)) if no < nc => {
                // A nested opening tag comes first; check it's a full tag (followed by space or >).
                let abs_no = search_from + no;
                let after_open = &lower[abs_no + open_pat.len()..];
                let is_tag = after_open.starts_with('>') || after_open.starts_with(' ')
                    || after_open.starts_with('\t') || after_open.starts_with('\n')
                    || after_open.starts_with('/');
                if is_tag {
                    depth += 1;
                }
                search_from = abs_no + 1;
            }
            (_, Some(nc)) => {
                let abs_nc = search_from + nc;
                depth -= 1;
                if depth == 0 {
                    return Some(&html[content_start..abs_nc]);
                }
                search_from = abs_nc + close_pat.len();
            }
        }
    }
}

/// Find an element with `role="main"` and return its inner HTML.
///
/// Only supports double-quoted `role="main"`.  Finds the tag name, then
/// extracts the balanced inner content using `extract_tag_inner`.
fn extract_role_main_inner<'a>(lower: &str, html: &'a str) -> Option<&'a str> {
    let role_attr = "role=\"main\"";
    let pos = lower.find(role_attr)?;

    // Scan backwards to find the opening `<` of this tag.
    let tag_start = lower[..pos].rfind('<')?;
    let after_lt = &lower[tag_start + 1..];

    // Extract tag name: sequence of ascii letters/digits after `<`.
    let tag_name: String = after_lt.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    if tag_name.is_empty() {
        return None;
    }

    // Re-anchor lower/html at tag_start so extract_tag_inner finds the right one.
    extract_tag_inner(&lower[tag_start..], &html[tag_start..], &tag_name)
}

/// Strip noise tags (`<script>`, `<style>`, `<nav>`, `<aside>`, `<header>`,
/// `<footer>`) and their contents from `html`.
fn strip_noise_tags(html: &str) -> String {
    let noise = &["script", "style", "nav", "aside", "header", "footer"];
    let mut result = html.to_owned();
    for tag in noise {
        result = strip_tag_and_contents(&result, tag);
    }
    result
}

/// Remove all occurrences of `<tag …>…</tag>` (case-insensitive) from `html`.
fn strip_tag_and_contents(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open_pat = format!("<{}", tag);
    let close_pat = format!("</{}>", tag);

    let mut out = String::with_capacity(html.len());
    let mut pos = 0usize;

    while pos < html.len() {
        // Find next open tag.
        let Some(rel_open) = lower[pos..].find(open_pat.as_str()) else {
            out.push_str(&html[pos..]);
            break;
        };
        let abs_open = pos + rel_open;

        // Verify it is a real tag boundary.
        let after_name = &lower[abs_open + open_pat.len()..];
        let is_tag = after_name.starts_with('>') || after_name.starts_with(' ')
            || after_name.starts_with('\t') || after_name.starts_with('\n')
            || after_name.starts_with('/');
        if !is_tag {
            out.push_str(&html[pos..abs_open + 1]);
            pos = abs_open + 1;
            continue;
        }

        // Output everything before this tag.
        out.push_str(&html[pos..abs_open]);

        // Find matching close tag, handling nesting.
        let mut depth = 1usize;
        let mut search = abs_open + open_pat.len();
        let end_pos = loop {
            let next_open = lower[search..].find(open_pat.as_str());
            let next_close = lower[search..].find(close_pat.as_str());
            match (next_open, next_close) {
                (_, None) => break html.len(), // unclosed tag — skip rest
                (Some(no), Some(nc)) if no < nc => {
                    let abs = search + no;
                    let after = &lower[abs + open_pat.len()..];
                    let is_real = after.starts_with('>') || after.starts_with(' ')
                        || after.starts_with('\t') || after.starts_with('\n')
                        || after.starts_with('/');
                    if is_real {
                        depth += 1;
                    }
                    search = abs + 1;
                }
                (_, Some(nc)) => {
                    let abs = search + nc;
                    depth -= 1;
                    if depth == 0 {
                        break abs + close_pat.len();
                    }
                    search = abs + close_pat.len();
                }
            }
        };
        pos = end_pos;
    }
    out
}

/// Extract a title: first try `<title>` tag, then first `<h1>` or `<h2>` in
/// the article body.
fn extract_title(full_html: &str, body_html: &str) -> String {
    // Try <title> in full document.
    let lower_full = full_html.to_ascii_lowercase();
    if let Some(start) = lower_full.find("<title")
        && let Some(gt) = lower_full[start..].find('>')
    {
        let content_start = start + gt + 1;
        if let Some(end) = lower_full[content_start..].find("</title") {
            let raw = &full_html[content_start..content_start + end];
            let text = strip_all_tags(raw).trim().to_owned();
            if !text.is_empty() {
                return text;
            }
        }
    }

    // Fall back to first <h1> or <h2> in body.
    for heading in &["h1", "h2"] {
        let lower_body = body_html.to_ascii_lowercase();
        let open = format!("<{}", heading);
        if let Some(start) = lower_body.find(open.as_str())
            && let Some(gt) = lower_body[start..].find('>')
        {
            let content_start = start + gt + 1;
            let close = format!("</{}>", heading);
            if let Some(end) = lower_body[content_start..].find(close.as_str()) {
                let raw = &body_html[content_start..content_start + end];
                let text = strip_all_tags(raw).trim().to_owned();
                if !text.is_empty() {
                    return text;
                }
            }
        }
    }

    String::new()
}

/// Extract `content` attribute of `<meta name="author" content="…">`.
fn extract_author_meta(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    // Find <meta name="author"
    let pos = lower.find("name=\"author\"")?;
    // Scan within the surrounding <meta … > tag for content="…"
    let tag_start = lower[..pos].rfind('<')?;
    let tag_end = lower[pos..].find('>')?;
    let tag_slice = &html[tag_start..pos + tag_end + 1];
    let lower_slice = tag_slice.to_ascii_lowercase();

    let content_pos = lower_slice.find("content=\"")?;
    let after = &tag_slice[content_pos + 9..];
    let end = after.find('"')?;
    let author = after[..end].trim().to_owned();
    if author.is_empty() { None } else { Some(author) }
}

/// Remove all HTML tags from `s`, leaving only text content.
fn strip_all_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Escape `<`, `>`, `&` for safe HTML attribute/text injection.
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
    fn extract_article_from_article_tag() {
        let html = "<html><body><article><p>Hello world</p></article></body></html>";
        let art = extract_article(html).expect("should find article");
        assert!(art.body_html.contains("Hello world"), "body should contain article text");
    }

    #[test]
    fn extract_article_from_main_tag() {
        let html = "<html><body><main><p>Main content</p></main></body></html>";
        let art = extract_article(html).expect("should find main");
        assert!(art.body_html.contains("Main content"));
    }

    #[test]
    fn extract_article_from_role_main() {
        let html = r#"<html><body><div role="main"><p>Role content</p></div></body></html>"#;
        let art = extract_article(html).expect("should find role=main");
        assert!(art.body_html.contains("Role content"));
    }

    #[test]
    fn extract_article_returns_none_when_no_content() {
        let html = "<html><body><nav>Menu</nav><footer>Footer</footer></body></html>";
        assert!(extract_article(html).is_none());
    }

    #[test]
    fn extract_article_strips_script_tags() {
        let html = "<html><body><article><script>alert(1)</script><p>Text</p></article></body></html>";
        let art = extract_article(html).expect("should find article");
        assert!(!art.body_html.contains("alert"), "script should be stripped");
        assert!(art.body_html.contains("Text"));
    }

    #[test]
    fn extract_article_strips_nav_aside_header_footer() {
        let html = r#"<html><body><article>
            <header>Site header</header>
            <nav>Nav links</nav>
            <p>Real article text.</p>
            <aside>Sidebar</aside>
            <footer>Footer</footer>
        </article></body></html>"#;
        let art = extract_article(html).expect("should find article");
        assert!(!art.body_html.contains("Site header"), "header should be stripped");
        assert!(!art.body_html.contains("Nav links"), "nav should be stripped");
        assert!(!art.body_html.contains("Sidebar"), "aside should be stripped");
        assert!(!art.body_html.contains("Footer"), "footer should be stripped");
        assert!(art.body_html.contains("Real article text."));
    }

    #[test]
    fn extract_title_from_title_tag() {
        let html = "<html><head><title>My Page</title></head><body><article><p>Body</p></article></body></html>";
        let art = extract_article(html).expect("should find article");
        assert_eq!(art.title, "My Page");
    }

    #[test]
    fn extract_title_from_h1_fallback() {
        let html = "<html><body><article><h1>Article Title</h1><p>Body</p></article></body></html>";
        let art = extract_article(html).expect("should find article");
        assert_eq!(art.title, "Article Title");
    }

    #[test]
    fn extract_author_from_meta() {
        let html = r#"<html><head><meta name="author" content="Jane Doe"></head><body><article><p>Text</p></article></body></html>"#;
        let art = extract_article(html).expect("should find article");
        assert_eq!(art.author.as_deref(), Some("Jane Doe"));
    }

    #[test]
    fn extract_author_none_when_missing() {
        let html = "<html><body><article><p>Text</p></article></body></html>";
        let art = extract_article(html).expect("should find article");
        assert!(art.author.is_none());
    }

    #[test]
    fn build_reader_html_contains_max_width() {
        let art = ArticleContent {
            title: "Test".to_owned(),
            body_html: "<p>Body</p>".to_owned(),
            author: None,
        };
        let out = build_reader_html(&art);
        assert!(out.contains("max-width: 680px"), "reader HTML should have max-width:680px");
    }

    #[test]
    fn build_reader_html_contains_font_size_and_line_height() {
        let art = ArticleContent {
            title: "Test".to_owned(),
            body_html: "<p>Body</p>".to_owned(),
            author: None,
        };
        let out = build_reader_html(&art);
        assert!(out.contains("font-size: 1.1rem"), "should have 1.1rem font-size");
        assert!(out.contains("line-height: 1.6"), "should have line-height:1.6");
    }

    #[test]
    fn build_reader_html_title_escaped() {
        let art = ArticleContent {
            title: "Foo <Bar> & Baz".to_owned(),
            body_html: "<p>text</p>".to_owned(),
            author: None,
        };
        let out = build_reader_html(&art);
        assert!(out.contains("Foo &lt;Bar&gt; &amp; Baz"));
    }

    #[test]
    fn html_escape_roundtrip() {
        assert_eq!(html_escape("<script>&\"</script>"), "&lt;script&gt;&amp;&quot;&lt;/script&gt;");
    }

    #[test]
    fn strip_all_tags_removes_markup() {
        assert_eq!(strip_all_tags("<b>hello</b> world"), "hello world");
    }

    #[test]
    fn nested_article_tag_balanced() {
        // extract_tag_inner should correctly balance nested tags
        let html = "<article><article><p>inner</p></article><p>outer</p></article>";
        let lower = html.to_ascii_lowercase();
        let inner = extract_tag_inner(&lower, html, "article").expect("balanced");
        assert!(inner.contains("outer"), "should return full outer content");
    }
}
