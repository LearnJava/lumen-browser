//! Раскрытие CSS Nesting L1: `&`-подстановка и декартово раскрытие селекторов.
//!
//! Вырезано из `parser.rs` (SPLIT-CP2) без изменения поведения.

use super::*;

/// Заменяет каждый `&` prelude-а legacy `@nest` на `parent_css`. `None`, если
/// в каком-либо complex-селекторе (части списка по запятой верхнего уровня)
/// нет `&` — для `@nest` это делает правило невалидным. `&` внутри строк и
/// `[attr]`-скобок — не nesting-селектор и не трогается.
pub(super) fn substitute_nesting_selector(prelude: &str, parent_css: &str) -> Option<String> {
    let mut out = String::with_capacity(prelude.len() + parent_css.len());
    let mut has_amp = false;
    let mut depth_paren = 0i32;
    let mut in_attr = false;
    let mut quote: Option<char> = None;
    let mut chars = prelude.chars();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                out.push(c);
            }
            '\\' => {
                out.push(c);
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '[' => {
                in_attr = true;
                out.push(c);
            }
            ']' => {
                in_attr = false;
                out.push(c);
            }
            '(' => {
                depth_paren += 1;
                out.push(c);
            }
            ')' => {
                depth_paren -= 1;
                out.push(c);
            }
            '&' if !in_attr => {
                has_amp = true;
                out.push_str(parent_css);
            }
            ',' if !in_attr && depth_paren == 0 => {
                if !has_amp {
                    return None;
                }
                has_amp = false;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    has_amp.then_some(out)
}

/// Hard cap on selectors a single [`expand_nesting`] call can produce.
///
/// CSS Nesting L1 doesn't bound cartesian growth (`parents.len() *
/// nested.len()`), and the expanded list becomes the `parents` of the next
/// nesting level — so on malformed input where recovery keeps entering
/// [`Parser::parse_implicit_nested_rule`] instead of terminating, the
/// selector count compounds multiplicatively *per level of nesting depth*
/// instead of growing additively with input size. A 676-byte fuzzer
/// minimization reached 50 MiB / ×74 000 blowup this way (BUG-788). Real
/// stylesheets never come close to four figures of selectors from nesting
/// alone, so truncating here only ever discards pathological expansion, not
/// legitimate rules.
const MAX_EXPANDED_SELECTORS: usize = 1024;

/// CSS Nesting L1 §3 — expand `& (combinator) nested` into concrete selectors.
///
/// `combinator = None`  → compound join (e.g. `&.foo` → `parent.foo`)
/// `combinator = Some(c)` → `parent c nested` (e.g. `& span` → `parent descendant span`)
pub(super) fn expand_nesting(
    parents: &[ComplexSelector],
    combinator: Option<Combinator>,
    nested: &[ComplexSelector],
) -> Vec<ComplexSelector> {
    let mut result = Vec::new();
    'outer: for parent in parents {
        for n in nested {
            if result.len() >= MAX_EXPANDED_SELECTORS {
                break 'outer;
            }
            let expanded = match combinator {
                None => {
                    // `&.foo` → merge parent head with nested head, keep tails.
                    let mut head = parent.head.clone();
                    head.parts.extend_from_slice(&n.head.parts);
                    let mut tail = parent.tail.clone();
                    tail.extend_from_slice(&n.tail);
                    ComplexSelector { head, tail }
                }
                Some(comb) => {
                    // `& span` → parent + (comb, nested_head) + nested_tail
                    let mut tail = parent.tail.clone();
                    tail.push((comb, n.head.clone()));
                    tail.extend_from_slice(&n.tail);
                    ComplexSelector { head: parent.head.clone(), tail }
                }
            };
            result.push(expanded);
        }
    }
    result
}
