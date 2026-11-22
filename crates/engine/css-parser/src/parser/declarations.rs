//! Декларации CSS: тип [`Declaration`] и разбор declaration-block.
//!
//! Вырезано из `parser.rs` (SPLIT-CP1 срез 2/2) без изменения поведения.

// Долг по документации: код перенесён из `parser.rs` как есть; файл
// написан до включения `missing_docs`. Счётчики — docs/lint-policy.md §10.
#![allow(missing_docs)]

use super::*;

// ── PERF-DECL-FLAGS ────────────────────────────────────────────────────────
// Content flags precomputed once at parse time so the cascade's per-node
// per-declaration scans (`contains("var(")`, `contains_env_call`, trim +
// `eq_ignore_ascii_case` keyword probes — run for every matched declaration
// of every element of every pass) become bit tests. Flags describe ONLY the
// raw `value` text; `attr()`/`var()` expansion produces new declarations
// through the same constructors, so expanded values get their own flags.
//
// Every flag is conservative in the safe direction: a clear bit proves the
// substring/keyword is absent; a set bit may still be answered `false` by the
// precise predicate (callers keep the precise check behind the flag).

/// `value` contains the exact substring `var(` (case-sensitive — mirrors the
/// cascade's own `contains("var(")` predicate byte for byte).
pub const DECL_F_HAS_VAR: u32 = 1;
/// `value` may contain an `env(` call: case-insensitive substring probe
/// anywhere in the text (strings/comments included — over-approximation is
/// safe, the precise `contains_env_call` still decides behind the flag).
pub const DECL_F_HAS_ENV_MAYBE: u32 = 2;
/// `value` contains the exact substring `attr(`.
pub const DECL_F_HAS_ATTR: u32 = 4;
/// `value` contains `--` (the `--fn()` custom-function gate's probe).
pub const DECL_F_HAS_DASHDASH: u32 = 8;

/// Keyword id occupies bits 8..11: the trimmed value, compared
/// case-insensitively, is exactly one of the CSS-wide / revert keywords the
/// cascade probes for. `0` = none.
pub const DECL_KW_SHIFT: u32 = 8;
pub const DECL_KW_MASK: u32 = 0x7 << DECL_KW_SHIFT;

/// Keyword ids stored in [`DECL_KW_MASK`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeclKeyword {
    None = 0,
    Inherit = 1,
    Initial = 2,
    Unset = 3,
    Revert = 4,
    RevertLayer = 5,
    RevertRule = 6,
}

impl Declaration {
    /// CSS-wide / revert keyword of the trimmed value, if it is exactly one —
    /// the same answer the cascade's `trim().eq_ignore_ascii_case(...)` probes
    /// give, computed once at parse time.
    pub fn keyword(&self) -> DeclKeyword {
        match (self.flags & DECL_KW_MASK) >> DECL_KW_SHIFT {
            1 => DeclKeyword::Inherit,
            2 => DeclKeyword::Initial,
            3 => DeclKeyword::Unset,
            4 => DeclKeyword::Revert,
            5 => DeclKeyword::RevertLayer,
            6 => DeclKeyword::RevertRule,
            _ => DeclKeyword::None,
        }
    }

    pub fn has_var(&self) -> bool {
        self.flags & DECL_F_HAS_VAR != 0
    }
    /// Conservative: `true` means "might contain `env(`" — callers still run
    /// the precise `contains_env_call` behind it.
    pub fn has_env_maybe(&self) -> bool {
        self.flags & DECL_F_HAS_ENV_MAYBE != 0
    }
    pub fn has_attr(&self) -> bool {
        self.flags & DECL_F_HAS_ATTR != 0
    }
    pub fn has_dashdash(&self) -> bool {
        self.flags & DECL_F_HAS_DASHDASH != 0
    }
}

/// Case-insensitive substring probe without allocation (over strings,
/// comments and escapes alike — deliberately crude, flags are allowed to
/// over-approximate).
fn contains_ci(haystack: &str, needle: &[u8]) -> bool {
    let h = haystack.as_bytes();
    h.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// Computes [`DECL_F_*`] flags for a declaration `value`. Called by the
/// parser and by every site that builds a `Declaration` with a fresh value
/// (attr()/var()/mixin expansion buffers).
pub fn decl_content_flags(value: &str) -> u32 {
    let mut flags = 0u32;
    if value.contains("var(") {
        flags |= DECL_F_HAS_VAR;
    }
    if contains_ci(value, b"env(") {
        flags |= DECL_F_HAS_ENV_MAYBE;
    }
    if value.contains("attr(") {
        flags |= DECL_F_HAS_ATTR;
    }
    if value.contains("--") {
        flags |= DECL_F_HAS_DASHDASH;
    }
    let kw = {
        let t = value.trim();
        if t.eq_ignore_ascii_case("inherit") {
            DeclKeyword::Inherit
        } else if t.eq_ignore_ascii_case("initial") {
            DeclKeyword::Initial
        } else if t.eq_ignore_ascii_case("unset") {
            DeclKeyword::Unset
        } else if t.eq_ignore_ascii_case("revert") {
            DeclKeyword::Revert
        } else if t.eq_ignore_ascii_case("revert-layer") {
            DeclKeyword::RevertLayer
        } else if t.eq_ignore_ascii_case("revert-rule") {
            DeclKeyword::RevertRule
        } else {
            DeclKeyword::None
        }
    };
    flags | ((kw as u32) << DECL_KW_SHIFT)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    /// `!important` флаг (CSS Cascade L4 §8.1). При равной specificity
    /// `important = true` побеждает `important = false`.
    pub important: bool,
    /// [`decl_content_flags`] of `value`, computed once at construction.
    pub flags: u32,
}

impl Declaration {
    /// One `"prop: value;"` / `"prop: value !important;"` line, the shape
    /// `CSSStyleDeclaration.cssText` (CSSOM §7.3) joins with a space between
    /// declarations.
    pub fn to_css_text(&self) -> String {
        if self.important {
            format!("{}: {} !important;", self.property, self.value)
        } else {
            format!("{}: {};", self.property, self.value)
        }
    }
}

impl<'a> Parser<'a> {
    pub(crate) fn parse_declaration_block(&mut self) -> Vec<Declaration> {
        let mut decls = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some(';') => {
                    self.consume();
                    continue;
                }
                _ => match self.parse_declaration() {
                    Some(d) => decls.push(d),
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        decls
    }

    /// Skips a malformed/unrecognized declaration up to the next `;` or `}`.
    /// If the malformed text turns out to be a brace-delimited block instead
    /// — e.g. `@supports (...) { result: PASS; }` inside a `@function`/
    /// `@mixin` body, which `parse_declaration_block` has no dedicated
    /// grammar for — skips that whole matched block via [`Self::scan_component_values`]
    /// and stops right there instead of continuing to scan for a trailing
    /// `;`: the declaration that legitimately follows the block must be left
    /// for the caller's own loop to parse, not swallowed into this recovery
    /// too (BUG-519 — a naive depth-0-`;`-or-`}` scan would eat exactly that
    /// following declaration, since a `;` inside the block no longer stops
    /// it once brace nesting is tracked, but the text after the block still
    /// ends in its own `;`).
    pub(crate) fn recover_to_decl_boundary(&mut self) {
        self.scan_component_values(Vec::new(), ScanEnd::SEMI | ScanEnd::CLOSE_BRACE | ScanEnd::OPEN_BRACE);
    }

    pub(crate) fn parse_declaration(&mut self) -> Option<Declaration> {
        self.skip_ws_and_comments();
        let mut property = self.parse_ident()?;
        // CSS 2.1 §4.1.3: property names are ASCII case-insensitive; custom
        // properties (`--*`) are the exception and stay case-sensitive.
        if !property.starts_with("--") {
            property.make_ascii_lowercase();
        }
        self.skip_ws_and_comments();
        if self.peek() != Some(':') {
            return None;
        }
        self.consume();
        // Custom properties keep their source text (CSS Variables L1 §2).
        let (value, valid) = self.parse_value_checked(!property.starts_with("--"));
        if !valid {
            return None;
        }
        let (value, important) = extract_important(value.trim());
        // WHATWG Compat §3: `-webkit-*` simple aliases resolve to the
        // unprefixed property here, once, for the whole cascade.
        let property = crate::compat_alias::canonical_property_name(&property)
            .map_or(property, str::to_string);
        let flags = decl_content_flags(&value);
        Some(Declaration {
            property,
            value,
            important,
            flags,
        })
    }

    /// CSS Syntax L3 §5.4.4 "consume a declaration's value" treats `;`/`}`
    /// as a terminator only at nesting depth 0 — a value is a sequence of
    /// component values, and a matched `(...)`/`[...]` is itself one
    /// component value regardless of what it contains. Without this, CSS
    /// Values L5's `if(<condition>: <value>; else: <value>;)` (whose own
    /// grammar uses `;` *inside* its parens to separate branches) gets
    /// truncated at the first branch's `;`, leaving the rest of the `if()`
    /// call to be misparsed as a bogus second declaration (BUG-519).
    ///
    /// Returns the value text and a validity flag: `false` when it holds a
    /// `<bad-string-token>` (a string cut by an unescaped newline, §4.3.5),
    /// which makes the whole declaration invalid. Blocks left open at EOF are
    /// closed in the returned text (§5.3 — EOF closes every open construct, so
    /// `rgb(0, 128, 0` is `rgb(0, 128, 0)`).
    pub(crate) fn parse_value_checked(&mut self, decode_escapes: bool) -> (String, bool) {
        let mut s = String::new();
        let mut in_string: Option<char> = None;
        // Closers of the open `(`/`[`/`{` blocks, innermost last; a closer
        // that does not match the innermost one is an ordinary token.
        let mut stack: Vec<char> = Vec::new();
        let mut valid = true;
        // Inside an unquoted `url(` the body is one url-token (CSS Syntax
        // §4.3.6): `/*` there is data, not a comment.
        let mut in_url = false;
        while let Some(c) = self.peek() {
            match (in_string, c) {
                (None, ';') | (None, '}') if stack.is_empty() => break,
                // CSS Syntax §4.3.2: `\` escapes the next code point, so
                // `\"` does not close a string and `\/*` is not a comment.
                // BUG-1380: outside strings a valid escape is expanded so the
                // later keyword/function matching sees `green` in `\67 reen`
                // and `url(` in `\75rl(`.
                (None, '\\') if decode_escapes && self.at_escape_start() => {
                    let start = self.pos;
                    self.consume();
                    let Some(d) = self.consume_escaped_code_point() else {
                        continue;
                    };
                    if in_url || escape_decodes_to_plain_ident_char(d, s.chars().next_back()) {
                        s.push(d);
                    } else {
                        // Structural or token-initial: keep the source form
                        // (`\3b `, `\31 0px`) so it cannot change tokenisation.
                        s.push_str(&self.input[start..self.pos]);
                    }
                }
                (_, '\\') => {
                    self.consume();
                    s.push(c);
                    if let Some(n) = self.consume() {
                        s.push(n);
                    }
                }
                // An unescaped newline ends a string token as a bad-string.
                (Some(_), '\n') => {
                    valid = false;
                    in_string = None;
                    self.consume();
                    s.push(c);
                }
                // CSS Syntax §4.3.2: a comment is dropped from the token
                // stream but still separates tokens, so it becomes one space
                // (`gr/*c*/een` stays two tokens, BUG-1379).
                (None, '/') if !in_url && self.peek_at(1) == Some('*') => {
                    self.consume();
                    self.consume();
                    match self.rest().find("*/") {
                        Some(end) => self.pos += end + 2,
                        None => self.pos = self.input.len(),
                    }
                    s.push(' ');
                }
                (None, ')') if in_url => {
                    in_url = false;
                    stack.pop();
                    self.consume();
                    s.push(c);
                }
                (Some(q), c) if c == q => {
                    self.consume();
                    s.push(c);
                    in_string = None;
                }
                (None, '"') | (None, '\'') => {
                    self.consume();
                    s.push(c);
                    in_string = Some(c);
                }
                (None, '(') | (None, '[') | (None, '{') => {
                    stack.push(match c {
                        '(' => ')',
                        '[' => ']',
                        _ => '}',
                    });
                    self.consume();
                    s.push(c);
                    if c == '(' && !in_url && s.len() >= 4 && s.as_bytes()[s.len() - 4..].eq_ignore_ascii_case(b"url(") {
                        let quoted = self.rest().trim_start().starts_with(['"', '\'']);
                        in_url = !quoted;
                    }
                    // CSS Variables L1 §3: the function name is ASCII case-insensitive
                    // (`VAR(--a)`); downstream code looks for the lowercase `var(`.
                    let n = s.len();
                    if c == '('
                        && n >= 4
                        && s.as_bytes()[n - 4..].eq_ignore_ascii_case(b"var(")
                        && (n == 4 || !is_ident_byte(s.as_bytes()[n - 5]))
                    {
                        s.replace_range(n - 4.., "var(");
                    }
                }
                (None, ')') | (None, ']') | (None, '}') => {
                    if stack.last() == Some(&c) {
                        stack.pop();
                    } else if c != '}' && !decode_escapes {
                        // A custom property is a `<declaration-value>`: no unmatched `)`/`]` (CSS Variables L1
                        // §2.1): `--a: red)` is dropped, the earlier `--a` stays (BUG-1500).
                        valid = false;
                    }
                    self.consume();
                    s.push(c);
                }
                _ => {
                    self.consume();
                    s.push(c);
                }
            }
        }
        if let Some(q) = in_string {
            s.push(q);
        }
        while let Some(closer) = stack.pop() {
            s.push(closer);
        }
        (s, valid)
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

/// Whether an expanded escape `d` can be spliced into the value text as
/// itself: a letter, `_` or non-ASCII code point always continues/starts an
/// ident; a digit or `-` only when it follows an ident character (at a token
/// start it would turn the ident into a number or a dashed ident).
fn escape_decodes_to_plain_ident_char(d: char, prev: Option<char>) -> bool {
    if d.is_ascii_alphabetic() || d == '_' || !d.is_ascii() {
        return true;
    }
    (d.is_ascii_digit() || d == '-')
        && prev.is_some_and(|p| p.is_ascii_alphanumeric() || p == '-' || p == '_' || !p.is_ascii())
}

/// CSS Cascade L4 §8.1: если значение оканчивается на `!important` (с
/// опциональным whitespace между `!` и словом, ASCII case-insensitive),
/// отделяет его и возвращает `(clean_value, true)`. Иначе — `(value, false)`.
///
/// Безопасно для строковых литералов: `content: "!important"` даёт
/// (value=`"!important"`, false), потому что после строки идёт `"`, а не
/// `important`. Не пытается обрабатывать комментарии внутри `!important`
/// (`!/* x */important`) и multiple `!important` — оба слишком экзотичны.
pub(crate) fn extract_important(value: &str) -> (String, bool) {
    let v = value.trim_end();
    let imp = b"important";
    if v.len() < imp.len() {
        return (value.to_string(), false);
    }
    if !v.as_bytes()[v.len() - imp.len()..].eq_ignore_ascii_case(imp) {
        return (value.to_string(), false);
    }
    let before_imp = v[..v.len() - imp.len()].trim_end();
    let Some(before_bang) = before_imp.strip_suffix('!') else {
        return (value.to_string(), false);
    };
    (before_bang.trim_end().to_string(), true)
}
