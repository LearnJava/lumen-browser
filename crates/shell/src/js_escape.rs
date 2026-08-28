//! Escaping a Rust string for embedding in a JS string literal.
//!
//! Used by the shell's `eval_js` call sites that interpolate a value into a
//! single-quoted literal (BUG-436); [`js_string_literal`] is the same job for a
//! call site that wants the quotes emitted too (double-quoted, `v8` only).
//!
//! Moved out of `main.rs` by the SPLIT track (batches SH-5, SH-3d); behaviour
//! and signatures are unchanged.

/// Escape a single character for safe embedding in a JS string literal.
///
/// Converts `ch` to an ASCII or `\uXXXX` escape so the character can be
/// used in `"..."` or `'...'` JS string arguments passed via `eval_js`.
/// Both quote flavours are escaped because every call site in this crate
/// interpolates the result into a **single**-quoted literal вЂ” an unescaped
/// apostrophe there produced a syntax error and the whole dispatch script
/// was silently dropped (found while fixing BUG-436).
pub(crate) fn escape_js_string_char(ch: char) -> String {
    match ch {
        '"' => r#"\""#.to_owned(),
        '\'' => r"\'".to_owned(),
        '\\' => r"\\".to_owned(),
        '\n' => r"\n".to_owned(),
        '\r' => r"\r".to_owned(),
        '\t' => r"\t".to_owned(),
        c if (c as u32) < 0x20 || (c as u32) > 0x7E => {
            format!("\\u{:04X}", c as u32)
        }
        c => c.to_string(),
    }
}

/// Escape a whole string for safe embedding in a single-quoted JS literal.
///
/// Character-by-character application of [`escape_js_string_char`] вЂ” used to
/// hand a form control's new value to `_lumen_set_field_value` (BUG-436).
pub(crate) fn escape_js_string(s: &str) -> String {
    s.chars().map(escape_js_string_char).collect()
}

/// Encode `s` as a JS string literal (double-quoted, with escaping).
/// Used when building JS snippets from Rust strings (e.g., `_lumen_init_lazy_images`).
#[cfg(feature = "v8")]
pub(crate) fn js_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
