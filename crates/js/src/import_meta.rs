//! `import.meta` source-level preprocessor for ES modules.
//!
//! QuickJS (rquickjs 0.11) exposes no public API to inject properties into a
//! module's `import.meta` object after compilation, and the [`Loader`] trait
//! receives no hook to do so before it either.  This module therefore
//! source-transforms module code before QuickJS sees it:
//!
//! * Every `import.meta` occurrence (outside strings / comments / template
//!   literals) is replaced by `__$lumen_meta__` — a plain `var` defined at the
//!   top of the module via a one-line preamble.
//! * The preamble sets `.url` (the resolved module specifier), `.resolve(s)`
//!   (Phase 0: simple base-dir join for relative paths), and `.env` (Vite-style
//!   compat stub: `{MODE:"production", DEV:false, PROD:true, BASE_URL:"/", SSR:false}`).
//!
//! The transformer is **fail-open**: when no `import.meta` appears in the
//! source, `transform_import_meta` returns `None` so callers can skip the
//! string reallocation.

/// Transform `import.meta` in `source`, binding `url` as `.url`.
///
/// Returns `Some(transformed)` if any `import.meta` occurrences were found
/// outside strings / comments, `None` if the source is unchanged.
pub fn transform_import_meta(source: &str, url: &str) -> Option<String> {
    // Fast pre-filter: if the literal bytes aren't present, skip tokenising.
    if !source.contains("import") {
        return None;
    }

    let ranges = find_import_meta_ranges(source);
    if ranges.is_empty() {
        return None;
    }

    let preamble = build_preamble(url);
    let mut out = String::with_capacity(preamble.len() + source.len() + 16);
    out.push_str(&preamble);

    let mut last = 0;
    for (start, end) in &ranges {
        out.push_str(&source[last..*start]);
        out.push_str("__$lumen_meta__");
        last = *end;
    }
    out.push_str(&source[last..]);
    Some(out)
}

// ── Preamble builder ──────────────────────────────────────────────────────────

fn build_preamble(url: &str) -> String {
    let esc = escape_js_string(url);
    let mut p = String::with_capacity(300);
    p.push_str("var __$lumen_meta__=Object.create(null);");
    p.push_str(&format!("__$lumen_meta__.url=\"{esc}\";"));
    // Phase 0 resolver: handles absolute URLs and relative path joins.
    p.push_str("__$lumen_meta__.resolve=function(s){");
    p.push_str("if(!s)return __$lumen_meta__.url;");
    p.push_str("if(/^[a-zA-Z][a-zA-Z0-9+-.]*:/.test(s))return s;");
    p.push_str("var b=__$lumen_meta__.url;");
    p.push_str("var d=b.slice(0,b.lastIndexOf('/')+1);");
    p.push_str("return d+s;");
    p.push_str("};");
    // Vite-style env stub so `import.meta.env.MODE` doesn't throw.
    p.push_str("__$lumen_meta__.env={MODE:\"production\",DEV:false,PROD:true,BASE_URL:\"/\",SSR:false};");
    p.push('\n');
    p
}

/// Escape `s` for embedding inside a JS double-quoted string literal.
fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

// ── Scanner ───────────────────────────────────────────────────────────────────

/// Locate byte ranges of `import.meta` token sequences in `src`, skipping
/// occurrences inside string literals, template literals, and comments.
///
/// Returns a list of `(start, end)` pairs where `src[start..end]` covers
/// the full `import.meta` sequence (including any whitespace between tokens).
fn find_import_meta_ranges(src: &str) -> Vec<(usize, usize)> {
    let b = src.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    while i < n {
        match b[i] {
            // Line comment — skip to end of line.
            b'/' if i + 1 < n && b[i + 1] == b'/' => {
                i += 2;
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
            }
            // Block comment — skip to `*/`.
            b'/' if i + 1 < n && b[i + 1] == b'*' => {
                i += 2;
                while i + 1 < n && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            // Double-quoted string.
            b'"' => {
                i += 1;
                while i < n && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < n {
                    i += 1;
                }
            }
            // Single-quoted string.
            b'\'' => {
                i += 1;
                while i < n && b[i] != b'\'' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < n {
                    i += 1;
                }
            }
            // Template literal (simplified: no nested `${...}` depth tracking).
            b'`' => {
                i += 1;
                while i < n && b[i] != b'`' {
                    if b[i] == b'\\' {
                        i += 1;
                    } else if b[i] == b'$' && i + 1 < n && b[i + 1] == b'{' {
                        i += 2;
                        let mut depth = 1u32;
                        while i < n && depth > 0 {
                            match b[i] {
                                b'{' => depth += 1,
                                b'}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                b'\\' => {
                                    i += 1;
                                }
                                _ => {}
                            }
                            i += 1;
                        }
                    }
                    i += 1;
                }
                if i < n {
                    i += 1;
                }
            }
            // Any identifier char: check for `import` keyword.
            c if is_id_start(c) => {
                let id_start = i;
                i += 1;
                while i < n && is_id_char(b[i]) {
                    i += 1;
                }
                let word = &src[id_start..i];
                if word != "import" {
                    continue;
                }
                // We have `import`.  Look ahead (skipping whitespace) for `.meta`.
                let after_import = i;
                let mut j = after_import;
                while j < n && matches!(b[j], b' ' | b'\t' | b'\r' | b'\n') {
                    j += 1;
                }
                if j >= n || b[j] != b'.' {
                    continue;
                }
                let dot_pos = j;
                j += 1;
                while j < n && matches!(b[j], b' ' | b'\t' | b'\r' | b'\n') {
                    j += 1;
                }
                // Must be exactly `meta` followed by a non-id char.
                if j + 4 <= n && &b[j..j + 4] == b"meta" {
                    let after_meta = j + 4;
                    if after_meta >= n || !is_id_char(b[after_meta]) {
                        // Confirm `.` is not preceded by whitespace in a way
                        // that would make it a member expression on a different
                        // value.  Since `import` is a keyword, `import.meta` is
                        // always the `import.meta` meta-property.
                        let _ = dot_pos; // used implicitly via j chain
                        ranges.push((id_start, after_meta));
                        i = after_meta;
                        continue;
                    }
                }
                // Not `import.meta` — leave i at end of `import` identifier.
            }
            _ => {
                i += 1;
            }
        }
    }

    ranges
}

fn is_id_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_id_char(c: u8) -> bool {
    is_id_start(c) || c.is_ascii_digit()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuickJsRuntime;
    use lumen_core::ext::JsRuntime;

    // ── Unit: transformer ───────────────────────────────────────────────────

    #[test]
    fn no_import_meta_returns_none() {
        let src = "export const x = 1;";
        assert!(transform_import_meta(src, "https://example.com/app.js").is_none());
    }

    #[test]
    fn import_meta_in_string_not_transformed() {
        let src = r#"const s = "import.meta.url"; export default s;"#;
        assert!(transform_import_meta(src, "https://example.com/app.js").is_none());
    }

    #[test]
    fn import_meta_in_line_comment_not_transformed() {
        let src = "// import.meta.url is cool\nexport const x = 1;";
        assert!(transform_import_meta(src, "https://example.com/a.js").is_none());
    }

    #[test]
    fn import_meta_url_is_replaced() {
        let src = "export const u = import.meta.url;";
        let out = transform_import_meta(src, "https://example.com/app.js").unwrap();
        assert!(!out.contains("import.meta"), "import.meta must be replaced: {out}");
        assert!(out.contains("__$lumen_meta__.url"), "must reference meta var: {out}");
        assert!(out.contains("https://example.com/app.js"), "url must be in preamble: {out}");
    }

    #[test]
    fn import_meta_in_block_comment_not_transformed() {
        let src = "/* import.meta.url */\nexport const x = 1;";
        assert!(transform_import_meta(src, "https://example.com/a.js").is_none());
    }

    // ── Integration: end-to-end via QuickJS ─────────────────────────────────

    #[test]
    fn import_meta_url_returns_specifier() {
        let rt = QuickJsRuntime::new().unwrap();
        let specifier = "https://example.com/mymod.js";
        rt.register_module_source(specifier, "export const u = import.meta.url;");
        rt.eval_module(&format!("import {{ u }} from '{specifier}'; globalThis.__test_url = u;"))
            .unwrap();
        let got = rt.eval("globalThis.__test_url").unwrap();
        assert_eq!(got, crate::JsValue::String(specifier.into()));
    }

    #[test]
    fn import_meta_resolve_relative() {
        let rt = QuickJsRuntime::new().unwrap();
        let specifier = "https://example.com/app/main.js";
        rt.register_module_source(
            specifier,
            "export const r = import.meta.resolve('./utils.js');",
        );
        rt.eval_module(&format!(
            "import {{ r }} from '{specifier}'; globalThis.__res = r;"
        ))
        .unwrap();
        let got = rt.eval("globalThis.__res").unwrap();
        assert_eq!(got, crate::JsValue::String("https://example.com/app/./utils.js".into()));
    }

    #[test]
    fn import_meta_env_mode_is_production() {
        let rt = QuickJsRuntime::new().unwrap();
        let specifier = "https://example.com/env.js";
        rt.register_module_source(specifier, "export const mode = import.meta.env.MODE;");
        rt.eval_module(&format!(
            "import {{ mode }} from '{specifier}'; globalThis.__mode = mode;"
        ))
        .unwrap();
        let got = rt.eval("globalThis.__mode").unwrap();
        assert_eq!(got, crate::JsValue::String("production".into()));
    }
}
