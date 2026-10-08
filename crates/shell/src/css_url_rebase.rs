//! BUG-1127: rebase relative `url()` references of an external stylesheet
//! onto the stylesheet's own URL.
//!
//! CSS Values §4.3: a relative URL in a stylesheet resolves against the
//! stylesheet's URL, not the document's. The shell concatenates every sheet
//! into one text before the cascade parses it, and every consumer
//! (`@font-face` loading, background images, …) then resolves against the
//! document base — so `css/s.css` with `url("fonts/f.ttf")` fetched
//! `/fonts/f.ttf` instead of `/css/fonts/f.ttf`. Rewriting the references to
//! absolute ones right after the sheet is fetched, before concatenation,
//! keeps the base information that the merged text cannot carry.

use crate::resource_base::{path_to_file_url, ResolvedResource, ResourceBase};

/// Returns `css` with every relative `url(...)` token resolved against `base`
/// (the sheet's own location). Comments and strings are skipped, so text that
/// merely looks like `url(` inside them is left alone. Untouched: absolute
/// URLs (any scheme), fragment-only references (`url(#clip)` addresses the
/// document per CSS Values §4.3), empty URLs, and arguments with CSS escapes
/// (rare; rewriting them would need a full unescape/re-escape round trip).
pub(crate) fn rebase_css_urls(css: &str, base: &ResourceBase) -> String {
    let bytes = css.as_bytes();
    if !crate::stylesheets::contains_ignore_ascii_case(bytes, b"url(") {
        return css.to_owned();
    }
    let mut out = String::with_capacity(css.len() + 64);
    // Start of the not-yet-copied tail of `css`.
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = css[i + 2..].find("*/").map_or(bytes.len(), |p| i + 2 + p + 2);
            }
            q @ (b'"' | b'\'') => i = skip_string(bytes, i + 1, q),
            b'u' | b'U'
                if bytes.len() >= i + 4
                    && bytes[i..i + 4].eq_ignore_ascii_case(b"url(")
                    && (i == 0 || !is_ident_byte(bytes[i - 1])) =>
            {
                let open = i + 4;
                let Some(arg) = url_argument(bytes, open) else {
                    i = open;
                    continue;
                };
                if let Some(abs) = rebased(&css[arg.value.clone()], base) {
                    out.push_str(&css[copied..i]);
                    out.push_str("url(\"");
                    out.push_str(&abs);
                    out.push_str("\")");
                    copied = arg.end;
                }
                i = arg.end;
            }
            _ => i += 1,
        }
    }
    if copied == 0 {
        return css.to_owned();
    }
    out.push_str(&css[copied..]);
    out
}

/// The value range of a `url(` argument and the index just past its `)`.
struct UrlArgument {
    value: std::ops::Range<usize>,
    end: usize,
}

/// Parses the argument of a `url(` whose `(` ends right before `start`:
/// either one quoted string or an unquoted run, each followed by optional
/// whitespace and `)`. `None` when the token is malformed (unterminated,
/// or something other than whitespace before `)`) — left as is.
fn url_argument(bytes: &[u8], start: usize) -> Option<UrlArgument> {
    let mut i = skip_ws(bytes, start);
    let value = match bytes.get(i)? {
        &q @ (b'"' | b'\'') => {
            let body = i + 1;
            let close = skip_string(bytes, body, q);
            if bytes.get(close - 1) != Some(&q) || close - 1 < body {
                return None;
            }
            i = close;
            body..close - 1
        }
        _ => {
            let body = i;
            while i < bytes.len() && bytes[i] != b')' && !bytes[i].is_ascii_whitespace() {
                if matches!(bytes[i], b'"' | b'\'' | b'(') {
                    return None;
                }
                i += 1;
            }
            body..i
        }
    };
    i = skip_ws(bytes, i);
    (bytes.get(i) == Some(&b')')).then_some(UrlArgument { value, end: i + 1 })
}

/// The absolute form of `raw` against `base`, or `None` when `raw` must stay
/// verbatim (see [`rebase_css_urls`]).
fn rebased(raw: &str, base: &ResourceBase) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('#') || raw.contains('\\') || has_scheme(raw) {
        return None;
    }
    let abs = match base.resolve(raw) {
        ResolvedResource::Url(u) => u,
        ResolvedResource::File(p) => path_to_file_url(&p),
    };
    // A quoted CSS string cannot hold a raw `"` or newline; a resolved URL
    // never does, but a failed resolve falls back to the raw href.
    (!abs.contains(['"', '\n', '\r'])).then_some(abs)
}

/// RFC 3986 `scheme ":"` prefix — `data:`, `https:`, `blob:`, …
fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else { return false };
    let scheme = &s.as_bytes()[..colon];
    scheme.first().is_some_and(u8::is_ascii_alphabetic)
        && scheme.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// Index just past the closing `quote` of a string whose body starts at
/// `i`, honouring backslash escapes; an unterminated string ends at a
/// newline or the end of input (CSS Syntax §4.3.5 bad-string).
fn skip_string(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return i,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Bytes that continue a CSS identifier — `my-url(` is not a `url(` token.
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'\\') || b >= 0x80
}
