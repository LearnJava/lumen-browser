//! CSS Counters resolution — CSS Lists L3 §6.4 + CSS Content L3 §4.
//!
//! Implements a pre-order DOM traversal that computes a per-element snapshot of
//! counter values (after the element's own counter-reset + counter-increment, before
//! children). These snapshots are used by `content_to_inline_segments` to resolve
//! `counter()` / `counters()` / `attr()` in `content:` for `::before` / `::after`.
//!
//! # Algorithm
//! For each element in pre-order:
//! 1. Apply `counter-reset` (push a new scope value onto the named stack).
//! 2. Apply `counter-increment` (add to the top of the named stack).
//! 3. Save a snapshot (`NodeId → stacks`) for `counter()` / `counters()` resolution.
//! 4. Recurse into children.
//! 5. Pop the scopes added in step 1.
//!
//! This gives correct resolution for `::before` pseudo-elements (which read the state
//! after the element's own increment but before children). `::after` reads post-children
//! state; for Phase 0 we use the same pre-children snapshot (sufficient for the common
//! `counter-increment` on `li` + `::before { content: counter(x) }` pattern).

use std::collections::HashMap;

use lumen_dom::{Document, FlatTree, NodeData, NodeId};

use crate::style::{compute_style, ComputedStyle};
use lumen_css_parser::Stylesheet;
use lumen_core::Size;

/// Per-element counter stacks snapshot.
///
/// Maps counter name → ordered stack of current values (outermost scope first,
/// innermost last). The stack holds all nested scopes so `counters()` can join them.
pub type CounterSnapshot = HashMap<String, Vec<i32>>;

/// Maps each element `NodeId` to its counter snapshot (after own reset/increment,
/// before children). Used during content resolution for `::before` / `::after`.
pub type CounterMap = HashMap<NodeId, CounterSnapshot>;

/// Mutable state threaded through the pre-order DOM traversal.
#[derive(Default)]
struct CounterCtx {
    /// name → stack of scope values (innermost = last).
    stacks: HashMap<String, Vec<i32>>,
}

impl CounterCtx {
    /// Push new scope(s) from `counter-reset`. Returns the list of names that were reset
    /// so the caller can pop them later.
    fn apply_reset(&mut self, resets: &[(String, i32)]) {
        for (name, val) in resets {
            self.stacks.entry(name.clone()).or_default().push(*val);
        }
    }

    /// Increment top-of-stack for each entry in `counter-increment`. Auto-creates a
    /// counter with value 0 if it has never been reset.
    fn apply_increment(&mut self, increments: &[(String, i32)]) {
        for (name, val) in increments {
            let stack = self.stacks.entry(name.clone()).or_default();
            if stack.is_empty() {
                stack.push(0);
            }
            *stack.last_mut().unwrap() += val;
        }
    }

    /// Snapshot the current stacks for this node.
    fn snapshot(&self) -> CounterSnapshot {
        self.stacks.clone()
    }

    /// Pop the scopes that were pushed by `apply_reset` for the given reset list.
    fn pop_reset(&mut self, resets: &[(String, i32)]) {
        for (name, _) in resets {
            if let Some(stack) = self.stacks.get_mut(name) {
                stack.pop();
                if stack.is_empty() {
                    self.stacks.remove(name);
                }
            }
        }
    }
}

/// Build a `CounterMap` by walking the DOM in pre-order.
///
/// Each element's snapshot captures counter state after its own `counter-reset`
/// and `counter-increment`, before any children are processed. This is the correct
/// state for resolving `counter()` in `::before` content.
pub fn precompute_counters(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    flat: &FlatTree,
) -> CounterMap {
    let root_style = ComputedStyle::root();
    let mut ctx = CounterCtx::default();
    let mut map = CounterMap::new();
    walk(doc, sheet, doc.root(), &root_style, viewport, flat, &mut ctx, &mut map);
    map
}

#[allow(clippy::too_many_arguments)]
fn walk(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    ctx: &mut CounterCtx,
    map: &mut CounterMap,
) {
    match &doc.get(id).data {
        // Text / comment / doctype — no counter properties, no children to visit.
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. }
        | NodeData::ShadowRoot { .. } => return,

        // Document node: has no style of its own; just recurse into children.
        NodeData::Document => {
            for &child_id in flat.children_of(doc, id) {
                walk(doc, sheet, child_id, inherited, viewport, flat, ctx, map);
            }
            return;
        }

        NodeData::Element { .. } => {} // handled below
    }

    let style = compute_style(doc, id, sheet, inherited, viewport);

    // CSS Lists L3 §6.4: counter-reset first, then counter-increment.
    ctx.apply_reset(&style.counter_reset);
    ctx.apply_increment(&style.counter_increment);

    map.insert(id, ctx.snapshot());

    for &child_id in flat.children_of(doc, id) {
        walk(doc, sheet, child_id, &style, viewport, flat, ctx, map);
    }

    ctx.pop_reset(&style.counter_reset);
}

// ─── Counter value formatting ────────────────────────────────────────────────

/// Format a counter integer value according to the given `list-style-type` keyword.
///
/// Supported styles: `decimal` (default), `lower-alpha` / `lower-latin`,
/// `upper-alpha` / `upper-latin`, `lower-roman`, `upper-roman`, `disc`,
/// `circle`, `square`, `none`. Unrecognised styles fall back to `decimal`.
pub fn format_counter(val: i32, style: &str) -> String {
    match style.trim() {
        "none" => String::new(),
        "lower-alpha" | "lower-latin" => alpha_counter(val, false),
        "upper-alpha" | "upper-latin" => alpha_counter(val, true),
        "lower-roman" => roman_counter(val, false),
        "upper-roman" => roman_counter(val, true),
        "disc" => "\u{2022}".to_string(),
        "circle" => "\u{25E6}".to_string(),
        "square" => "\u{25AA}".to_string(),
        // "decimal" and everything else:
        _ => val.to_string(),
    }
}

fn alpha_counter(n: i32, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let mut n = n as u32;
    let mut result = Vec::new();
    loop {
        n -= 1;
        let ch = (b'a' + (n % 26) as u8) as char;
        result.push(if upper { ch.to_ascii_uppercase() } else { ch });
        n /= 26;
        if n == 0 {
            break;
        }
    }
    result.iter().rev().collect()
}

fn roman_counter(n: i32, upper: bool) -> String {
    if n <= 0 || n > 3999 {
        return n.to_string();
    }
    const VALS: &[(u32, &str)] = &[
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"),
        (100,  "c"), (90,  "xc"), (50,  "l"), (40,  "xl"),
        (10,   "x"), (9,   "ix"), (5,   "v"), (4,   "iv"),
        (1,    "i"),
    ];
    let mut n = n as u32;
    let mut out = String::new();
    for &(v, s) in VALS {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    if upper { out.to_uppercase() } else { out }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_decimal() {
        assert_eq!(format_counter(1, "decimal"), "1");
        assert_eq!(format_counter(42, "decimal"), "42");
        assert_eq!(format_counter(0, "decimal"), "0");
        assert_eq!(format_counter(-1, "decimal"), "-1");
    }

    #[test]
    fn format_lower_alpha() {
        assert_eq!(format_counter(1, "lower-alpha"), "a");
        assert_eq!(format_counter(26, "lower-alpha"), "z");
        assert_eq!(format_counter(27, "lower-alpha"), "aa");
        assert_eq!(format_counter(52, "lower-alpha"), "az");
        assert_eq!(format_counter(53, "lower-alpha"), "ba");
    }

    #[test]
    fn format_upper_alpha() {
        assert_eq!(format_counter(1, "upper-alpha"), "A");
        assert_eq!(format_counter(26, "upper-alpha"), "Z");
        assert_eq!(format_counter(27, "upper-alpha"), "AA");
    }

    #[test]
    fn format_lower_roman() {
        assert_eq!(format_counter(1, "lower-roman"), "i");
        assert_eq!(format_counter(4, "lower-roman"), "iv");
        assert_eq!(format_counter(9, "lower-roman"), "ix");
        assert_eq!(format_counter(14, "lower-roman"), "xiv");
        assert_eq!(format_counter(40, "lower-roman"), "xl");
        assert_eq!(format_counter(90, "lower-roman"), "xc");
        assert_eq!(format_counter(400, "lower-roman"), "cd");
        assert_eq!(format_counter(900, "lower-roman"), "cm");
        assert_eq!(format_counter(1994, "lower-roman"), "mcmxciv");
    }

    #[test]
    fn format_upper_roman() {
        assert_eq!(format_counter(1994, "upper-roman"), "MCMXCIV");
    }

    #[test]
    fn format_none() {
        assert_eq!(format_counter(5, "none"), "");
    }

    #[test]
    fn counter_ctx_reset_increment_pop() {
        let mut ctx = CounterCtx::default();
        ctx.apply_reset(&[("x".into(), 0)]);
        ctx.apply_increment(&[("x".into(), 1)]);
        assert_eq!(ctx.stacks["x"], vec![1]);

        // Nested scope
        ctx.apply_reset(&[("x".into(), 10)]);
        ctx.apply_increment(&[("x".into(), 1)]);
        assert_eq!(ctx.stacks["x"], vec![1, 11]);

        ctx.pop_reset(&[("x".into(), 10)]);
        assert_eq!(ctx.stacks["x"], vec![1]);

        ctx.pop_reset(&[("x".into(), 0)]);
        assert!(!ctx.stacks.contains_key("x"));
    }

    #[test]
    fn counter_ctx_auto_create_on_increment() {
        let mut ctx = CounterCtx::default();
        ctx.apply_increment(&[("y".into(), 1)]);
        assert_eq!(ctx.stacks["y"], vec![1]);
    }
}
