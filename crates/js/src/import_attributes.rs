//! TC39 Import Attributes (Stage 3) Phase 0 — `import ... with { type: 'json' }`.
//!
//! QuickJS (via rquickjs 0.11) cannot parse the `with { ... }` clause, and the
//! `rquickjs::loader::Loader` trait receives only the resolved specifier — no
//! attributes. Phase 0 therefore pre-processes module source before QuickJS
//! sees it:
//!
//! * static `import` / `export ... from` statements: the `with { ... }` clause
//!   (and the legacy `assert { ... }` spelling) is stripped from the source and
//!   the declared `type` is recorded in a [`ModuleTypeRegistry`] keyed by the
//!   resolved specifier;
//! * [`LumenLoader`](crate::esm::LumenLoader) consults the registry at load
//!   time: `type: 'json'` modules are validated as JSON (the "JSON-assert
//!   guard") and compiled as a synthetic `export default JSON.parse(...)`
//!   module; any other declared type fails the load.
//!
//! Not supported (Phase 0): dynamic `import(spec, { with: { ... } })` options
//! objects (left untouched — QuickJS ignores the extra argument) and attribute
//! keys other than `type` (the clause is stripped, extra keys are ignored).
//!
//! The transformer is fail-open: source without a recognised attribute clause
//! is returned unchanged so QuickJS's own diagnostics surface.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Module type declared by an import attribute (`with { type: '...' }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleType {
    /// `type: 'json'` — the module source is parsed as JSON and default-exported.
    Json,
    /// Any other declared type — loading the module fails (per spec an
    /// unsupported attribute type is an error at import time).
    Unsupported(String),
}

impl ModuleType {
    /// Map a raw attribute value (`"json"`, `"css"`, ...) to a `ModuleType`.
    pub fn from_attr(value: &str) -> Self {
        if value == "json" {
            Self::Json
        } else {
            Self::Unsupported(value.to_owned())
        }
    }
}

/// Shared registry: resolved module specifier → declared module type.
///
/// Written by [`strip_import_attributes`] callers (`QuickJsRuntime::eval_module`
/// and `register_module_source`); read by `LumenLoader::load` when the module
/// graph is instantiated.
pub type ModuleTypeRegistry = Arc<Mutex<HashMap<String, ModuleType>>>;

/// Creates an empty [`ModuleTypeRegistry`].
pub fn new_type_registry() -> ModuleTypeRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

// ── Tokenizer ─────────────────────────────────────────────────────────────────

/// Token class produced by the minimal lexer. Strings keep their byte range so
/// the specifier/attribute values can be extracted; comments are skipped;
/// template literals and regex literals are opaque so `with` inside them is
/// never mistaken for an attribute clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    /// Identifier or keyword (also `#private` names).
    Id,
    /// String literal, quotes included in the range.
    Str,
    /// Numeric literal.
    Num,
    /// Single punctuation character.
    Punct,
    /// Opaque chunk: template literal segment or regex literal.
    Opaque,
}

/// A lexed token: kind + byte range into the source.
#[derive(Debug, Clone, Copy)]
struct Tok {
    kind: TokKind,
    start: usize,
    end: usize,
}

fn is_id_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_id_char(c: u8) -> bool {
    is_id_start(c) || c.is_ascii_digit()
}

/// Keywords after which a `/` starts a regex literal (not division).
const REGEX_KEYWORDS: [&str; 14] = [
    "return", "typeof", "instanceof", "in", "of", "new", "delete", "void", "throw", "case", "do",
    "else", "yield", "await",
];

/// Minimal JS lexer: strings / comments / template literals / regex literals
/// are handled so that `with` inside them is never treated as a clause keyword.
fn tokenize(src: &str) -> Vec<Tok> {
    let b = src.as_bytes();
    let n = b.len();
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0usize;
    // Per-open-`${` brace counters for nested template interpolations.
    let mut tmpl_stack: Vec<u32> = Vec::new();

    fn regex_allowed(src: &str, toks: &[Tok]) -> bool {
        match toks.last() {
            None => true,
            Some(t) => match t.kind {
                TokKind::Id => REGEX_KEYWORDS.contains(&&src[t.start..t.end]),
                TokKind::Punct => !matches!(&src[t.start..t.end], ")" | "]" | "}"),
                _ => false,
            },
        }
    }

    // Scan a template chunk starting at `i` (src[i] is '`' or the '}' resuming
    // an interpolation). Returns the new position; pushes one opaque token.
    fn scan_template_chunk(b: &[u8], start: usize, tmpl_stack: &mut Vec<u32>) -> usize {
        let n = b.len();
        let mut i = start + 1;
        while i < n {
            match b[i] {
                b'\\' => i += 2,
                b'`' => {
                    i += 1;
                    break;
                }
                b'$' if i + 1 < n && b[i + 1] == b'{' => {
                    i += 2;
                    tmpl_stack.push(0);
                    break;
                }
                _ => i += 1,
            }
        }
        i.min(n)
    }

    while i < n {
        let c = b[i];
        // Whitespace
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b) {
            i += 1;
            continue;
        }
        // Line comment
        if c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            i = src[i..].find('\n').map_or(n, |j| i + j);
            continue;
        }
        // Block comment
        if c == b'/' && i + 1 < n && b[i + 1] == b'*' {
            i = src[i + 2..].find("*/").map_or(n, |j| i + 2 + j + 2);
            continue;
        }
        // String literal
        if c == b'"' || c == b'\'' {
            let s = i;
            i += 1;
            while i < n && b[i] != c {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            toks.push(Tok { kind: TokKind::Str, start: s, end: i });
            continue;
        }
        // Template literal start
        if c == b'`' {
            let s = i;
            i = scan_template_chunk(b, i, &mut tmpl_stack);
            toks.push(Tok { kind: TokKind::Opaque, start: s, end: i });
            continue;
        }
        // `}` resuming a template interpolation
        if c == b'}' && tmpl_stack.last() == Some(&0) {
            tmpl_stack.pop();
            let s = i;
            i = scan_template_chunk(b, i, &mut tmpl_stack);
            toks.push(Tok { kind: TokKind::Opaque, start: s, end: i });
            continue;
        }
        // Regex literal
        if c == b'/' && regex_allowed(src, &toks) {
            let s = i;
            i += 1;
            let mut in_class = false;
            let mut ok = false;
            while i < n {
                match b[i] {
                    b'\\' => {
                        i += 2;
                        continue;
                    }
                    b'\n' => break,
                    b'[' => in_class = true,
                    b']' => in_class = false,
                    b'/' if !in_class => {
                        i += 1;
                        ok = true;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            if ok {
                while i < n && is_id_char(b[i]) {
                    i += 1;
                }
                toks.push(Tok { kind: TokKind::Opaque, start: s, end: i });
            } else {
                i = s + 1;
                toks.push(Tok { kind: TokKind::Punct, start: s, end: s + 1 });
            }
            continue;
        }
        // Identifier / keyword / #private
        if is_id_start(c) || c == b'#' {
            let s = i;
            i += 1;
            while i < n && is_id_char(b[i]) {
                i += 1;
            }
            toks.push(Tok { kind: TokKind::Id, start: s, end: i });
            continue;
        }
        // Number
        if c.is_ascii_digit() {
            let s = i;
            i += 1;
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            toks.push(Tok { kind: TokKind::Num, start: s, end: i });
            continue;
        }
        // Punctuation (track braces inside template interpolations)
        if c == b'{'
            && let Some(top) = tmpl_stack.last_mut()
        {
            *top += 1;
        }
        if c == b'}'
            && let Some(top) = tmpl_stack.last_mut()
        {
            *top -= 1;
        }
        toks.push(Tok { kind: TokKind::Punct, start: i, end: i + 1 });
        i += 1;
    }
    toks
}

/// Extract the decoded value of a string-literal token (quotes stripped,
/// simple `\\`-escapes resolved). Returns `None` for unterminated strings.
fn str_value(src: &str, t: &Tok) -> Option<String> {
    if t.kind != TokKind::Str || t.end < t.start + 2 {
        return None;
    }
    let bytes = src.as_bytes();
    if bytes[t.end - 1] != bytes[t.start] {
        return None; // unterminated at EOF
    }
    let inner = src.get(t.start + 1..t.end - 1)?;
    if !inner.contains('\\') {
        return Some(inner.to_owned());
    }
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(c2) = it.next() {
                out.push(match c2 {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

// ── Transformer ───────────────────────────────────────────────────────────────

/// Strip `with { ... }` / `assert { ... }` import-attribute clauses from
/// `source`.
///
/// Returns `None` when the source contains no attribute clause (fast path) or
/// nothing was rewritten — the caller then uses the original source as-is.
/// Otherwise returns `Some((rewritten, attrs))` where `attrs` is a list of
/// `(raw_specifier, type_value)` pairs in source order. Imports whose clause
/// has no `type` key are stripped but contribute no pair.
pub fn strip_import_attributes(source: &str) -> Option<(String, Vec<(String, String)>)> {
    // Fast path: no clause keyword anywhere.
    if !source.contains("with") && !source.contains("assert") {
        return None;
    }
    let toks = tokenize(source);
    let mut edits: Vec<(usize, usize)> = Vec::new();
    let mut attrs_out: Vec<(String, String)> = Vec::new();

    let word = |t: &Tok| &source[t.start..t.end];

    let mut k = 0usize;
    while k < toks.len() {
        let t = toks[k];
        if t.kind != TokKind::Id || (word(&t) != "import" && word(&t) != "export") {
            k += 1;
            continue;
        }
        let kw = word(&t);
        // Dynamic `import(...)` and `import.meta` — not a static statement.
        if kw == "import"
            && let Some(nx) = toks.get(k + 1)
            && nx.kind == TokKind::Punct
            && matches!(word(nx), "(" | ".")
        {
            k += 2;
            continue;
        }
        // Locate the specifier string of this statement:
        // `import "spec"` | `import ... from "spec"` | `export ... from "spec"`.
        let mut spec_idx: Option<usize> = None;
        if kw == "import"
            && let Some(nx) = toks.get(k + 1)
            && nx.kind == TokKind::Str
        {
            spec_idx = Some(k + 1);
        } else {
            let mut j = k + 1;
            while let Some(tj) = toks.get(j) {
                match tj.kind {
                    TokKind::Punct if word(tj) == ";" => break,
                    TokKind::Id => {
                        let w = word(tj);
                        if w == "from" {
                            if let Some(s) = toks.get(j + 1)
                                && s.kind == TokKind::Str
                            {
                                spec_idx = Some(j + 1);
                            }
                            break;
                        }
                        // Statement clearly isn't a from-import; stop scanning.
                        if matches!(w, "import" | "export" | "class" | "function" | "const" | "let" | "var") && j > k + 1 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        let Some(si) = spec_idx else {
            k += 1;
            continue;
        };
        // Attribute clause directly after the specifier: `with { ... }`.
        let Some(clause_kw) = toks.get(si + 1) else {
            k = si + 1;
            continue;
        };
        if clause_kw.kind != TokKind::Id || !matches!(word(clause_kw), "with" | "assert") {
            k = si + 1;
            continue;
        }
        let Some(open) = toks.get(si + 2) else {
            k = si + 2;
            continue;
        };
        if open.kind != TokKind::Punct || word(open) != "{" {
            k = si + 2;
            continue;
        }
        // Find the balanced closing `}`.
        let mut depth = 0i32;
        let mut close_idx: Option<usize> = None;
        for (idx, tj) in toks.iter().enumerate().skip(si + 2) {
            if tj.kind == TokKind::Punct {
                match word(tj) {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            close_idx = Some(idx);
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        let Some(ci) = close_idx else {
            k = si + 2;
            continue;
        };
        // Parse `key: "value"` pairs inside the braces; pick out `type`.
        let mut type_value: Option<String> = None;
        let mut m = si + 3;
        while m + 2 < ci {
            let (key, colon, val) = (&toks[m], &toks[m + 1], &toks[m + 2]);
            if colon.kind != TokKind::Punct || word(colon) != ":" || val.kind != TokKind::Str {
                break;
            }
            let key_name = match key.kind {
                TokKind::Id => Some(word(key).to_owned()),
                TokKind::Str => str_value(source, key),
                _ => None,
            };
            if key_name.as_deref() == Some("type") {
                type_value = str_value(source, val);
            }
            m += 3;
            if let Some(comma) = toks.get(m)
                && comma.kind == TokKind::Punct
                && word(comma) == ","
            {
                m += 1;
            }
        }
        edits.push((clause_kw.start, toks[ci].end));
        if let Some(ty) = type_value
            && let Some(spec) = str_value(source, &toks[si])
        {
            attrs_out.push((spec, ty));
        }
        k = ci + 1;
    }

    if edits.is_empty() {
        return None;
    }
    let mut out = source.to_owned();
    for (s, e) in edits.iter().rev() {
        out.replace_range(*s..*e, "");
    }
    Some((out, attrs_out))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuickJsRuntime;
    use lumen_core::ext::JsRuntime;

    #[test]
    fn strips_with_clause_from_default_import() {
        let src = "import cfg from \"./config.json\" with { type: \"json\" };\nuse_(cfg);";
        let (out, attrs) = strip_import_attributes(src).unwrap();
        assert!(!out.contains("with"), "clause must be stripped: {out}");
        assert!(out.contains("import cfg from \"./config.json\""));
        assert!(out.contains("use_(cfg);"));
        assert_eq!(attrs, vec![("./config.json".to_owned(), "json".to_owned())]);
    }

    #[test]
    fn strips_legacy_assert_and_single_quotes() {
        let src = "import a from 'a.json' assert { type: 'json' };";
        let (out, attrs) = strip_import_attributes(src).unwrap();
        assert!(!out.contains("assert"));
        assert_eq!(attrs, vec![("a.json".to_owned(), "json".to_owned())]);
    }

    #[test]
    fn side_effect_import_and_namespace_import() {
        let src = "import \"styles.css\" with { type: \"css\" };\n\
                   import * as ns from \"data.json\" with { type: \"json\" };";
        let (out, attrs) = strip_import_attributes(src).unwrap();
        assert!(!out.contains("with"));
        assert_eq!(
            attrs,
            vec![
                ("styles.css".to_owned(), "css".to_owned()),
                ("data.json".to_owned(), "json".to_owned()),
            ]
        );
    }

    #[test]
    fn export_from_with_attributes() {
        let src = "export { a } from \"mod.json\" with { type: \"json\" };";
        let (out, attrs) = strip_import_attributes(src).unwrap();
        assert_eq!(out, "export { a } from \"mod.json\" ;");
        assert_eq!(attrs, vec![("mod.json".to_owned(), "json".to_owned())]);
    }

    #[test]
    fn untouched_sources_return_none() {
        // No attribute clause at all.
        assert!(strip_import_attributes("import x from 'm';").is_none());
        // `with` statement is not an attribute clause.
        assert!(strip_import_attributes("with (obj) { x = 1; }").is_none());
        // `with` inside strings / comments / templates is opaque.
        assert!(strip_import_attributes(
            "var s = \"import a from 'b' with { type: 'json' }\";\n\
             // import a from 'b' with { type: 'json' }\n\
             var t = `import a from 'b' with { type: 'json' }`;"
        )
        .is_none());
        // Dynamic import options object is left for QuickJS (Phase 0).
        assert!(strip_import_attributes(
            "import(\"x.json\", { with: { type: \"json\" } }).then(m => m);"
        )
        .is_none());
    }

    #[test]
    fn mixed_imports_only_clause_removed() {
        let src = "import a from 'plain.js';\nimport b from 'cfg.json' with { type: 'json' };";
        let (out, attrs) = strip_import_attributes(src).unwrap();
        assert!(out.contains("import a from 'plain.js';"));
        assert!(out.contains("import b from 'cfg.json' ;"));
        assert_eq!(attrs, vec![("cfg.json".to_owned(), "json".to_owned())]);
    }

    #[test]
    fn clause_without_type_key_is_stripped_silently() {
        let src = "import a from 'm.bin' with { lazy: 'true' };";
        let (out, attrs) = strip_import_attributes(src).unwrap();
        assert!(!out.contains("with"));
        assert!(attrs.is_empty());
    }

    // ── End-to-end through QuickJsRuntime ────────────────────────────────────

    #[test]
    fn e2e_json_module_import() {
        let rt = QuickJsRuntime::new().unwrap();
        rt.register_module_source("cfgmod", r#"{ "answer": 42, "name": "lumen" }"#);
        rt.eval_module(
            "import cfg from 'cfgmod' with { type: 'json' };\n\
             globalThis.__aa3_answer = cfg.answer;\n\
             globalThis.__aa3_name = cfg.name;",
        )
        .unwrap();
        let answer = rt.eval("globalThis.__aa3_answer").unwrap();
        assert_eq!(answer, crate::JsValue::Number(42.0));
        let name = rt.eval("globalThis.__aa3_name").unwrap();
        assert_eq!(name, crate::JsValue::String("lumen".into()));
    }

    #[test]
    fn e2e_invalid_json_fails_to_load() {
        // JSON-assert guard: declared `type: 'json'` but the body is not JSON.
        let rt = QuickJsRuntime::new().unwrap();
        rt.register_module_source("badjson", "export const not_json = 1;");
        let result =
            rt.eval_module("import x from 'badjson' with { type: 'json' }; globalThis.__x = x;");
        assert!(result.is_err(), "invalid JSON must fail the import");
    }

    #[test]
    fn e2e_unsupported_type_fails_to_load() {
        let rt = QuickJsRuntime::new().unwrap();
        rt.register_module_source("styles", "body { color: red; }");
        let result = rt.eval_module("import s from 'styles' with { type: 'css' };");
        assert!(result.is_err(), "unsupported attribute type must fail the import");
    }

    #[test]
    fn e2e_registered_module_with_attributes_is_preprocessed() {
        // A registered module whose own source uses import attributes:
        // the clause is stripped at registration time and the type recorded.
        let rt = QuickJsRuntime::new().unwrap();
        rt.register_module_source("data", r#"{ "v": 7 }"#);
        rt.register_module_source(
            "wrapper",
            "import d from 'data' with { type: 'json' };\nexport const v = d.v;",
        );
        rt.eval_module("import { v } from 'wrapper'; globalThis.__aa3_v = v;").unwrap();
        let v = rt.eval("globalThis.__aa3_v").unwrap();
        assert_eq!(v, crate::JsValue::Number(7.0));
    }
}
