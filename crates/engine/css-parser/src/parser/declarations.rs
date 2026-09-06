//! Декларации CSS: тип [`Declaration`] и разбор declaration-block.
//!
//! Вырезано из `parser.rs` (SPLIT-CP1 срез 2/2) без изменения поведения.

// Долг по документации: код перенесён из `parser.rs` как есть; файл
// написан до включения `missing_docs`. Счётчики — docs/lint-policy.md §10.
#![allow(missing_docs)]

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    /// `!important` флаг (CSS Cascade L4 §8.1). При равной specificity
    /// `important = true` побеждает `important = false`.
    pub important: bool,
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
    /// grammar for — skips that whole matched block via [`Self::skip_block`]
    /// and stops right there instead of continuing to scan for a trailing
    /// `;`: the declaration that legitimately follows the block must be left
    /// for the caller's own loop to parse, not swallowed into this recovery
    /// too (BUG-519 — a naive depth-0-`;`-or-`}` scan would eat exactly that
    /// following declaration, since a `;` inside the block no longer stops
    /// it once brace nesting is tracked, but the text after the block still
    /// ends in its own `;`).
    pub(crate) fn recover_to_decl_boundary(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                ';' => {
                    self.consume();
                    return;
                }
                '}' => return,
                '{' => {
                    self.consume();
                    self.skip_block();
                    return;
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    pub(crate) fn parse_declaration(&mut self) -> Option<Declaration> {
        self.skip_ws_and_comments();
        let property = self.parse_ident()?;
        self.skip_ws_and_comments();
        if self.peek() != Some(':') {
            return None;
        }
        self.consume();
        let value = self.parse_value_until_terminator();
        let (value, important) = extract_important(value.trim());
        Some(Declaration {
            property,
            value,
            important,
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
    pub(crate) fn parse_value_until_terminator(&mut self) -> String {
        let mut s = String::new();
        let mut in_string: Option<char> = None;
        let mut depth: u32 = 0;
        while let Some(c) = self.peek() {
            match (in_string, c) {
                (None, ';') | (None, '}') if depth == 0 => break,
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
                (None, '(') | (None, '[') => {
                    depth += 1;
                    self.consume();
                    s.push(c);
                }
                (None, ')') | (None, ']') => {
                    depth = depth.saturating_sub(1);
                    self.consume();
                    s.push(c);
                }
                _ => {
                    self.consume();
                    s.push(c);
                }
            }
        }
        s
    }
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
