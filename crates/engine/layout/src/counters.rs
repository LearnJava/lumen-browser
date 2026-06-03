//! CSS Counters resolution — CSS Lists L3 §6.4 + CSS Content L3 §4 +
//! CSS Counter Styles L3 (custom `@counter-style` rules).
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
//!
//! # Custom counter styles
//! `CounterStyleRegistry` maps counter style names to `CounterStyleDef` — parsed
//! descriptors from `@counter-style` at-rules. Use `build_counter_style_registry`
//! to build from a `Stylesheet`, then pass to `format_counter_with_registry`.

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
/// Precomputes CSS counter values for the entire document tree.
/// `dark_mode` is forwarded to `@media (prefers-color-scheme: dark)` matching
/// during style computation so counter-related styles resolve correctly.
pub fn precompute_counters(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    flat: &FlatTree,
    dark_mode: bool,
) -> CounterMap {
    let root_style = ComputedStyle::root();
    let mut ctx = CounterCtx::default();
    let mut map = CounterMap::new();
    walk(doc, sheet, doc.root(), &root_style, viewport, flat, &mut ctx, &mut map, dark_mode);
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
    dark_mode: bool,
) {
    match &doc.get(id).data {
        // Text / comment / doctype / fragment — no counter properties, no children.
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. }
        | NodeData::ShadowRoot { .. } | NodeData::DocumentFragment => return,

        // Document node: has no style of its own; just recurse into children.
        NodeData::Document => {
            for &child_id in flat.children_of(doc, id) {
                walk(doc, sheet, child_id, inherited, viewport, flat, ctx, map, dark_mode);
            }
            return;
        }

        NodeData::Element { .. } => {} // handled below
    }

    let style = compute_style(doc, id, sheet, inherited, viewport, dark_mode);

    // CSS Lists L3 §6.4: counter-reset first, then counter-increment.
    ctx.apply_reset(&style.counter_reset);
    ctx.apply_increment(&style.counter_increment);

    map.insert(id, ctx.snapshot());

    for &child_id in flat.children_of(doc, id) {
        walk(doc, sheet, child_id, &style, viewport, flat, ctx, map, dark_mode);
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

// ─── Custom counter styles (CSS Counter Styles L3) ───────────────────────────

/// Numbering algorithm for a `@counter-style` rule — CSS Counter Styles L3 §4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterSystem {
    /// Cycles through symbols repeatedly (§4.2).
    Cyclic,
    /// Positional numeral system using symbols as digits (§4.3).
    Numeric,
    /// Bijective base-N (like spreadsheet columns: A…Z, AA…AZ, …) (§4.4).
    Alphabetic,
    /// Like cyclic but each symbol repeats more times per pass (§4.5).
    Symbolic,
    /// Weighted sum, like roman numerals (§4.6).
    Additive,
    /// Finite range starting at `first` value (§4.1).
    Fixed(i32),
    /// Inherits algorithm + symbols from another counter style (§4.7).
    Extends(String),
}

/// Counter range bound: `None` means ±infinite (CSS Counter Styles L3 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeBound {
    /// Inclusive lower bound, or `None` for −∞.
    pub min: Option<i32>,
    /// Inclusive upper bound, or `None` for +∞.
    pub max: Option<i32>,
}

/// Range descriptor value (CSS Counter Styles L3 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterRange {
    /// System-dependent default range.
    Auto,
    /// Explicit list of inclusive ranges.
    Explicit(Vec<RangeBound>),
}

/// Parsed `@counter-style` rule — CSS Counter Styles L3 §2.
#[derive(Debug, Clone, PartialEq)]
pub struct CounterStyleDef {
    /// Numbering algorithm.
    pub system: CounterSystem,
    /// Symbol list for cyclic/alphabetic/symbolic/numeric/fixed systems.
    pub symbols: Vec<String>,
    /// `(weight, symbol)` pairs for additive system, sorted descending by weight.
    pub additive_symbols: Vec<(i32, String)>,
    /// Prepended to every counter representation.
    pub prefix: String,
    /// Appended to every counter representation (default `"."`).
    pub suffix: String,
    /// Range descriptor controlling when fallback is used.
    pub range: CounterRange,
    /// `(min_length, pad_symbol)` — pad representation to min length.
    pub pad: Option<(i32, String)>,
    /// `(negative_prefix, negative_suffix)` applied when value < 0.
    pub negative: (String, String),
    /// Fallback counter style name (default `"decimal"`).
    pub fallback: String,
}

impl Default for CounterStyleDef {
    fn default() -> Self {
        Self {
            system: CounterSystem::Symbolic,
            symbols: Vec::new(),
            additive_symbols: Vec::new(),
            prefix: String::new(),
            suffix: ".".to_string(),
            range: CounterRange::Auto,
            pad: None,
            negative: ("-".to_string(), String::new()),
            fallback: "decimal".to_string(),
        }
    }
}

/// Maps counter style names to their parsed `CounterStyleDef`.
pub type CounterStyleRegistry = HashMap<String, CounterStyleDef>;

/// Build a `CounterStyleRegistry` from all `@counter-style` rules in a stylesheet.
pub fn build_counter_style_registry(sheet: &Stylesheet) -> CounterStyleRegistry {
    sheet
        .counter_styles
        .iter()
        .map(|rule| (rule.name.clone(), parse_counter_style_descriptors(&rule.declarations)))
        .collect()
}

fn parse_counter_style_descriptors(
    declarations: &[lumen_css_parser::Declaration],
) -> CounterStyleDef {
    let mut def = CounterStyleDef::default();
    for decl in declarations {
        match decl.property.trim().to_ascii_lowercase().as_str() {
            "system" => def.system = parse_counter_system(&decl.value),
            "symbols" => def.symbols = parse_symbols_list(&decl.value),
            "additive-symbols" => def.additive_symbols = parse_additive_symbols(&decl.value),
            "prefix" => def.prefix = parse_single_symbol(&decl.value),
            "suffix" => def.suffix = parse_single_symbol(&decl.value),
            "range" => def.range = parse_counter_range(&decl.value),
            "pad" => def.pad = parse_pad_descriptor(&decl.value),
            "negative" => def.negative = parse_negative_descriptor(&decl.value),
            "fallback" => def.fallback = decl.value.trim().to_string(),
            _ => {}
        }
    }
    def
}

fn parse_counter_system(value: &str) -> CounterSystem {
    let s = value.trim().to_ascii_lowercase();
    if s == "cyclic" {
        CounterSystem::Cyclic
    } else if s == "numeric" {
        CounterSystem::Numeric
    } else if s == "alphabetic" {
        CounterSystem::Alphabetic
    } else if s == "symbolic" {
        CounterSystem::Symbolic
    } else if s == "additive" {
        CounterSystem::Additive
    } else if let Some(rest) = s.strip_prefix("fixed") {
        let start = rest.trim().parse::<i32>().unwrap_or(1);
        CounterSystem::Fixed(start)
    } else if let Some(rest) = s.strip_prefix("extends") {
        CounterSystem::Extends(rest.trim().to_string())
    } else {
        CounterSystem::Symbolic
    }
}

/// Parse a CSS symbol list (space-separated quoted strings or idents).
fn parse_symbols_list(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut symbols = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            i += 1;
            let (s, end) = parse_css_string_from(&chars, i, quote);
            i = end;
            symbols.push(s);
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            if !tok.is_empty() {
                symbols.push(tok);
            }
        }
    }
    symbols
}

/// Parse a single CSS symbol (quoted string or ident).
fn parse_single_symbol(value: &str) -> String {
    let s = value.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        let inner: Vec<char> = s[1..s.len() - 1].chars().collect();
        let (result, _) = parse_css_string_from(&inner, 0, s.chars().next().unwrap());
        result
    } else {
        s.to_string()
    }
}

/// Parse `additive-symbols: <integer> <symbol> (, <integer> <symbol>)*`.
fn parse_additive_symbols(value: &str) -> Vec<(i32, String)> {
    let mut result = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let chars: Vec<char> = part.chars().collect();
        let mut i = 0;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        // Parse leading integer
        let neg = i < chars.len() && chars[i] == '-';
        if neg {
            i += 1;
        }
        let num_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == num_start {
            continue;
        }
        let num_str: String = chars[if neg { num_start - 1 } else { num_start }..i]
            .iter()
            .collect();
        let Ok(num) = num_str.parse::<i32>() else {
            continue;
        };
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let sym_str: String = chars[i..].iter().collect();
        let sym = parse_single_symbol(sym_str.trim());
        result.push((num, sym));
    }
    // Spec requires descending order for the additive algorithm.
    result.sort_unstable_by_key(|&(w, _): &(i32, _)| std::cmp::Reverse(w));
    result
}

/// Parse `range: auto | [ [ <integer> | infinite ]{2} ]#`.
fn parse_counter_range(value: &str) -> CounterRange {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" {
        return CounterRange::Auto;
    }
    let mut bounds = Vec::new();
    for pair in v.split(',') {
        let parts: Vec<&str> = pair.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let min = if parts[0] == "infinite" {
            None
        } else {
            parts[0].parse::<i32>().ok()
        };
        let max = if parts[1] == "infinite" {
            None
        } else {
            parts[1].parse::<i32>().ok()
        };
        bounds.push(RangeBound { min, max });
    }
    if bounds.is_empty() {
        CounterRange::Auto
    } else {
        CounterRange::Explicit(bounds)
    }
}

/// Parse `pad: <integer> <symbol>`.
fn parse_pad_descriptor(value: &str) -> Option<(i32, String)> {
    let chars: Vec<char> = value.trim().chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let num_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == num_start {
        return None;
    }
    let num_str: String = chars[num_start..i].iter().collect();
    let Ok(len) = num_str.parse::<i32>() else {
        return None;
    };
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let sym_str: String = chars[i..].iter().collect();
    let sym = parse_single_symbol(sym_str.trim());
    Some((len, sym))
}

/// Parse `negative: <symbol> <symbol>?` — prefix and optional suffix.
fn parse_negative_descriptor(value: &str) -> (String, String) {
    let v = value.trim();
    // May be one or two space-separated symbols (each can be a quoted string).
    let syms = parse_symbols_list(v);
    let prefix = syms.first().cloned().unwrap_or_else(|| "-".to_string());
    let suffix = syms.get(1).cloned().unwrap_or_default();
    (prefix, suffix)
}

/// Parse a quoted CSS string body (char array starting after the opening quote).
/// Returns (unescaped_string, index_after_closing_quote).
fn parse_css_string_from(chars: &[char], start: usize, quote: char) -> (String, usize) {
    let mut result = String::new();
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        if c == quote {
            i += 1; // skip closing quote
            break;
        }
        if c == '\\' {
            i += 1;
            // Collect up to 6 hex digits
            let mut hex = String::new();
            while i < chars.len() && chars[i].is_ascii_hexdigit() && hex.len() < 6 {
                hex.push(chars[i]);
                i += 1;
            }
            if !hex.is_empty() {
                // Skip optional single whitespace after hex escape (CSS §4.1.3)
                if i < chars.len()
                    && (chars[i] == ' ' || chars[i] == '\t' || chars[i] == '\n'
                        || chars[i] == '\r' || chars[i] == '\x0C')
                {
                    i += 1;
                }
                if let Ok(code) = u32::from_str_radix(&hex, 16)
                    && let Some(ch) = char::from_u32(code)
                {
                    result.push(ch);
                }
            } else if i < chars.len() {
                // Non-hex escape: include character literally
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(c);
            i += 1;
        }
    }
    (result, i)
}

// ─── Counter formatting with registry ────────────────────────────────────────

impl CounterStyleDef {
    /// Check whether `val` is within the effective range for this style.
    fn in_range(&self, val: i32) -> bool {
        match &self.range {
            CounterRange::Auto => self.auto_in_range(val),
            CounterRange::Explicit(bounds) => bounds.iter().any(|b| {
                b.min.is_none_or(|m| val >= m) && b.max.is_none_or(|m| val <= m)
            }),
        }
    }

    fn auto_in_range(&self, val: i32) -> bool {
        match &self.system {
            CounterSystem::Cyclic | CounterSystem::Numeric => true,
            CounterSystem::Alphabetic | CounterSystem::Symbolic => val >= 1,
            CounterSystem::Additive => val >= 0,
            CounterSystem::Fixed(start) => {
                val >= *start
                    && (val as i64 - *start as i64) < self.symbols.len() as i64
            }
            CounterSystem::Extends(_) => true,
        }
    }
}

/// Format a counter value using the registry (custom `@counter-style`) first,
/// then fall back to built-in `format_counter` for standard style names.
pub fn format_counter_with_registry(
    val: i32,
    style_name: &str,
    registry: &CounterStyleRegistry,
) -> String {
    if let Some(def) = registry.get(style_name) {
        format_with_def(val, def, registry, 8)
    } else {
        format_counter(val, style_name)
    }
}

/// Recursively format using a `CounterStyleDef` (depth-limited to avoid cycles).
fn format_with_def(
    val: i32,
    def: &CounterStyleDef,
    registry: &CounterStyleRegistry,
    depth: u32,
) -> String {
    if depth == 0 {
        return val.to_string();
    }

    // Check range; use fallback if out of range.
    if !def.in_range(val) {
        return format_fallback(val, &def.fallback, registry, depth - 1);
    }

    // Step 1: Generate initial token using system algorithm.
    let abs = val.unsigned_abs() as i32;
    let token_opt = match &def.system {
        CounterSystem::Cyclic => {
            format_cyclic(val, &def.symbols)
        }
        CounterSystem::Numeric => {
            format_numeric(abs, &def.symbols)
        }
        CounterSystem::Alphabetic => {
            if val < 1 {
                None
            } else {
                format_alphabetic(val, &def.symbols)
            }
        }
        CounterSystem::Symbolic => {
            if val < 1 {
                None
            } else {
                format_symbolic(val, &def.symbols)
            }
        }
        CounterSystem::Additive => {
            format_additive(val, &def.additive_symbols)
        }
        CounterSystem::Fixed(first) => {
            let idx = val - first;
            if idx < 0 || (idx as usize) >= def.symbols.len() {
                None
            } else {
                Some(def.symbols[idx as usize].clone())
            }
        }
        CounterSystem::Extends(name) => {
            // Delegate entirely to the named base style.
            if let Some(base) = registry.get(name) {
                let s = format_with_def(val, base, registry, depth - 1);
                return s;
            }
            return format_counter(val, name);
        }
    };

    let Some(mut token) = token_opt else {
        return format_fallback(val, &def.fallback, registry, depth - 1);
    };

    // Step 2: Apply negative descriptor for negative values.
    if val < 0 {
        token = format!("{}{}{}", def.negative.0, token, def.negative.1);
    }

    // Step 3: Apply pad descriptor.
    if let Some((min_len, pad_sym)) = &def.pad {
        let cur_len = token.chars().count() as i32;
        if cur_len < *min_len {
            let needed = (*min_len - cur_len) as usize;
            let padding = pad_sym.repeat(needed);
            token = format!("{padding}{token}");
        }
    }

    // Step 4: Prepend prefix + append suffix.
    format!("{}{}{}", def.prefix, token, def.suffix)
}

fn format_fallback(
    val: i32,
    fallback: &str,
    registry: &CounterStyleRegistry,
    depth: u32,
) -> String {
    if let Some(def) = registry.get(fallback) {
        format_with_def(val, def, registry, depth)
    } else {
        format_counter(val, fallback)
    }
}

fn format_cyclic(val: i32, symbols: &[String]) -> Option<String> {
    if symbols.is_empty() {
        return None;
    }
    let len = symbols.len() as i32;
    // CSS spec: index = (val - 1) mod S, using mathematical (Euclidean) modulo.
    let idx = (val - 1).rem_euclid(len) as usize;
    Some(symbols[idx].clone())
}

fn format_numeric(abs: i32, symbols: &[String]) -> Option<String> {
    let len = symbols.len();
    if len < 2 {
        return None;
    }
    if abs == 0 {
        return Some(symbols[0].clone());
    }
    let mut digits: Vec<&str> = Vec::new();
    let mut n = abs as usize;
    while n > 0 {
        digits.push(&symbols[n % len]);
        n /= len;
    }
    digits.reverse();
    Some(digits.join(""))
}

fn format_alphabetic(val: i32, symbols: &[String]) -> Option<String> {
    if symbols.is_empty() || val < 1 {
        return None;
    }
    let len = symbols.len();
    let mut n = val as usize;
    let mut chars: Vec<&str> = Vec::new();
    loop {
        n -= 1;
        chars.push(&symbols[n % len]);
        n /= len;
        if n == 0 {
            break;
        }
    }
    chars.reverse();
    Some(chars.join(""))
}

fn format_symbolic(val: i32, symbols: &[String]) -> Option<String> {
    if symbols.is_empty() || val < 1 {
        return None;
    }
    let len = symbols.len();
    let idx = (val as usize - 1) % len;
    let repeat = (val as usize - 1) / len + 1;
    Some(symbols[idx].repeat(repeat))
}

fn format_additive(val: i32, tuples: &[(i32, String)]) -> Option<String> {
    if val < 0 {
        return None;
    }
    if val == 0 {
        return tuples
            .iter()
            .find(|(w, _)| *w == 0)
            .map(|(_, s)| s.clone());
    }
    let mut result = String::new();
    let mut remaining = val as u64;
    for (weight, symbol) in tuples {
        if *weight <= 0 {
            continue;
        }
        let w = *weight as u64;
        if remaining >= w {
            let count = (remaining / w) as usize;
            remaining -= w * count as u64;
            for _ in 0..count {
                result.push_str(symbol);
            }
        }
    }
    if remaining > 0 { None } else { Some(result) }
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

    // ── Custom counter style tests ────────────────────────────────────────────

    fn s(v: &str) -> String { v.to_string() }
    fn sym(v: &[&str]) -> Vec<String> { v.iter().map(|x| s(x)).collect() }

    #[test]
    fn cyclic_basic() {
        let syms = sym(&["a", "b", "c"]);
        assert_eq!(format_cyclic(1, &syms), Some(s("a")));
        assert_eq!(format_cyclic(2, &syms), Some(s("b")));
        assert_eq!(format_cyclic(3, &syms), Some(s("c")));
        assert_eq!(format_cyclic(4, &syms), Some(s("a")));
        assert_eq!(format_cyclic(0, &syms), Some(s("c")));
        assert_eq!(format_cyclic(-1, &syms), Some(s("b")));
    }

    #[test]
    fn cyclic_empty_symbols() {
        assert_eq!(format_cyclic(1, &[]), None);
    }

    #[test]
    fn numeric_decimal_like() {
        let syms = sym(&["0","1","2","3","4","5","6","7","8","9"]);
        assert_eq!(format_numeric(0, &syms), Some(s("0")));
        assert_eq!(format_numeric(5, &syms), Some(s("5")));
        assert_eq!(format_numeric(10, &syms), Some(s("10")));
        assert_eq!(format_numeric(42, &syms), Some(s("42")));
        assert_eq!(format_numeric(100, &syms), Some(s("100")));
    }

    #[test]
    fn numeric_binary() {
        let syms = sym(&["0", "1"]);
        assert_eq!(format_numeric(0, &syms), Some(s("0")));
        assert_eq!(format_numeric(1, &syms), Some(s("1")));
        assert_eq!(format_numeric(2, &syms), Some(s("10")));
        assert_eq!(format_numeric(5, &syms), Some(s("101")));
    }

    #[test]
    fn alphabetic_basic() {
        let syms = sym(&["a","b","c","d","e","f","g","h","i","j",
                         "k","l","m","n","o","p","q","r","s","t",
                         "u","v","w","x","y","z"]);
        assert_eq!(format_alphabetic(1, &syms), Some(s("a")));
        assert_eq!(format_alphabetic(26, &syms), Some(s("z")));
        assert_eq!(format_alphabetic(27, &syms), Some(s("aa")));
        assert_eq!(format_alphabetic(52, &syms), Some(s("az")));
        assert_eq!(format_alphabetic(53, &syms), Some(s("ba")));
    }

    #[test]
    fn symbolic_basic() {
        let syms = sym(&["*", "†", "‡"]);
        assert_eq!(format_symbolic(1, &syms), Some(s("*")));
        assert_eq!(format_symbolic(3, &syms), Some(s("‡")));
        assert_eq!(format_symbolic(4, &syms), Some(s("**")));
        // val=7: idx=(7-1)%3=0, repeat=(7-1)/3+1=3 → "***"
        assert_eq!(format_symbolic(7, &syms), Some(s("***")));
    }

    #[test]
    fn symbolic_repeat_count() {
        let syms = sym(&["*"]);
        assert_eq!(format_symbolic(1, &syms), Some(s("*")));
        assert_eq!(format_symbolic(2, &syms), Some(s("**")));
        assert_eq!(format_symbolic(3, &syms), Some(s("***")));
    }

    #[test]
    fn additive_roman_numerals() {
        let tuples: Vec<(i32, String)> = vec![
            (1000, s("M")), (900, s("CM")), (500, s("D")), (400, s("CD")),
            (100, s("C")),  (90, s("XC")),  (50, s("L")),  (40, s("XL")),
            (10, s("X")),   (9, s("IX")),   (5, s("V")),   (4, s("IV")),
            (1, s("I")),
        ];
        assert_eq!(format_additive(1, &tuples), Some(s("I")));
        assert_eq!(format_additive(4, &tuples), Some(s("IV")));
        assert_eq!(format_additive(9, &tuples), Some(s("IX")));
        assert_eq!(format_additive(14, &tuples), Some(s("XIV")));
        assert_eq!(format_additive(1994, &tuples), Some(s("MCMXCIV")));
    }

    #[test]
    fn additive_cannot_represent_returns_none() {
        // Only has weight 2 — can't represent odd numbers
        let tuples: Vec<(i32, String)> = vec![(2, s("□"))];
        assert_eq!(format_additive(1, &tuples), None);
        assert_eq!(format_additive(2, &tuples), Some(s("□")));
        assert_eq!(format_additive(4, &tuples), Some(s("□□")));
    }

    #[test]
    fn parse_symbols_list_quoted() {
        let syms = parse_symbols_list("\"A\" \"B\" \"C\"");
        assert_eq!(syms, vec!["A", "B", "C"]);
    }

    #[test]
    fn parse_symbols_list_unquoted() {
        let syms = parse_symbols_list("A B C D");
        assert_eq!(syms, vec!["A", "B", "C", "D"]);
    }

    #[test]
    fn parse_symbols_list_unicode_escape() {
        let syms = parse_symbols_list("\"\\1F44D\" \"\\1F44E\"");
        assert_eq!(syms, vec!["👍", "👎"]);
    }

    #[test]
    fn parse_additive_symbols_basic() {
        let tuples = parse_additive_symbols("1000 \"M\", 100 \"C\", 1 \"I\"");
        assert_eq!(tuples[0], (1000, s("M")));
        assert_eq!(tuples[1], (100, s("C")));
        assert_eq!(tuples[2], (1, s("I")));
    }

    #[test]
    fn parse_counter_system_variants() {
        assert_eq!(parse_counter_system("cyclic"), CounterSystem::Cyclic);
        assert_eq!(parse_counter_system("numeric"), CounterSystem::Numeric);
        assert_eq!(parse_counter_system("alphabetic"), CounterSystem::Alphabetic);
        assert_eq!(parse_counter_system("symbolic"), CounterSystem::Symbolic);
        assert_eq!(parse_counter_system("additive"), CounterSystem::Additive);
        assert_eq!(parse_counter_system("fixed"), CounterSystem::Fixed(1));
        assert_eq!(parse_counter_system("fixed 3"), CounterSystem::Fixed(3));
        assert_eq!(parse_counter_system("extends my-style"),
                   CounterSystem::Extends(s("my-style")));
    }

    #[test]
    fn format_with_registry_cyclic_thumbs() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Cyclic,
            symbols: sym(&["👍", "👎"]),
            suffix: " ".to_string(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("thumbs"), def);
        assert_eq!(format_counter_with_registry(1, "thumbs", &registry), "👍 ");
        assert_eq!(format_counter_with_registry(2, "thumbs", &registry), "👎 ");
        assert_eq!(format_counter_with_registry(3, "thumbs", &registry), "👍 ");
    }

    #[test]
    fn format_with_registry_fallback_to_decimal() {
        let registry = CounterStyleRegistry::new();
        // Unknown style name falls back to built-in
        assert_eq!(format_counter_with_registry(5, "decimal", &registry), "5");
        assert_eq!(format_counter_with_registry(3, "lower-roman", &registry), "iii");
    }

    #[test]
    fn format_with_registry_negative_value() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Numeric,
            symbols: sym(&["0","1","2","3","4","5","6","7","8","9"]),
            negative: (s("-"), s("")),
            suffix: String::new(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("my-num"), def);
        assert_eq!(format_counter_with_registry(-5, "my-num", &registry), "-5");
    }

    #[test]
    fn format_with_registry_pad() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Numeric,
            symbols: sym(&["0","1","2","3","4","5","6","7","8","9"]),
            pad: Some((3, s("0"))),
            suffix: String::new(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("zero-padded"), def);
        assert_eq!(format_counter_with_registry(1, "zero-padded", &registry), "001");
        assert_eq!(format_counter_with_registry(12, "zero-padded", &registry), "012");
        assert_eq!(format_counter_with_registry(123, "zero-padded", &registry), "123");
        assert_eq!(format_counter_with_registry(1234, "zero-padded", &registry), "1234");
    }

    #[test]
    fn format_with_registry_fixed_range() {
        let mut registry = CounterStyleRegistry::new();
        let def = CounterStyleDef {
            system: CounterSystem::Fixed(1),
            symbols: sym(&["①","②","③"]),
            suffix: String::new(),
            ..CounterStyleDef::default()
        };
        registry.insert(s("circled"), def);
        assert_eq!(format_counter_with_registry(1, "circled", &registry), "①");
        assert_eq!(format_counter_with_registry(3, "circled", &registry), "③");
        // Out of range → fallback to decimal
        assert_eq!(format_counter_with_registry(4, "circled", &registry), "4");
    }

    #[test]
    fn parse_counter_range_auto() {
        assert_eq!(parse_counter_range("auto"), CounterRange::Auto);
    }

    #[test]
    fn parse_counter_range_explicit() {
        let r = parse_counter_range("1 10, 20 infinite");
        match r {
            CounterRange::Explicit(bounds) => {
                assert_eq!(bounds.len(), 2);
                assert_eq!(bounds[0].min, Some(1));
                assert_eq!(bounds[0].max, Some(10));
                assert_eq!(bounds[1].min, Some(20));
                assert_eq!(bounds[1].max, None);
            }
            _ => panic!("expected Explicit"),
        }
    }
}
