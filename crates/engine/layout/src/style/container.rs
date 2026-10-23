//! Containment (`contain`/`content-visibility`/`interpolate-size`) и
//! CSS Container Queries: `container-type`, вычисление окружения запроса
//! (`ContainerContext`) и оценка условий (`evaluate_container_condition`,
//! `apply_container_rules`). Это не тип значения, а вычисление —
//! отдельный файл вне `style::values`.
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `struct ContainFlags` до конца `apply_container_rules`) без правок тел.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::{Declaration, Rule, Stylesheet};
use lumen_dom::{Document, DocumentMode, NodeId};

use crate::style::apply::apply_declaration;
use crate::style::computed::ComputedStyle;
use crate::style::container_scroll::{evaluate_scroll_state_query, ScrollStateInfo};
use crate::style::matching::matches_complex;
use crate::style::parse::color::parse_color;
use crate::style::substitute::{expand_attr_val, expand_vars};
use crate::style::values::length::parse_length;
use crate::style::values::timing::CustomProps;

/// CSS Containment L3 §3 — `contain` property.
/// Bitflags: bit0=size, bit1=inline-size, bit2=layout, bit3=style, bit4=paint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContainFlags(pub u8);

impl ContainFlags {
    pub const NONE: Self = Self(0);
    pub const SIZE: Self = Self(1 << 0);
    pub const INLINE_SIZE: Self = Self(1 << 1);
    pub const LAYOUT: Self = Self(1 << 2);
    pub const STYLE: Self = Self(1 << 3);
    pub const PAINT: Self = Self(1 << 4);
    /// `strict` = size + layout + style + paint
    pub const STRICT: Self = Self(1 | (1 << 2) | (1 << 3) | (1 << 4));
    /// `content` = layout + style + paint
    pub const CONTENT: Self = Self((1 << 2) | (1 << 3) | (1 << 4));
}

/// CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentVisibility {
    #[default]
    Visible,
    Auto,
    Hidden,
}

/// CSS Sizing L4 §4.5 — `interpolate-size` property value.
///
/// Controls whether keyword sizes (`auto`, `min-content`, `max-content`,
/// `fit-content`) can participate in CSS transitions and animations.
/// When `AllowKeywords` is active, the layout engine resolves keyword sizes
/// to their px equivalent at transition start, enabling smooth
/// `height: 0 → height: auto` transitions.
///
/// # CSS: interpolate-size
/// P4 wires this enum via `apply_declaration("interpolate-size", ...)` and
/// stores the result in `ComputedStyle::interpolate_size`. The engine reads
/// it in `TransitionScheduler::sync()` to decide whether to allow keyword
/// size interpolation for a given element.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InterpolateSizeMode {
    /// CSS Sizing L4 §4.5.1 initial value — keyword sizes are discrete.
    /// Transitions that start or end at a keyword size snap at `t = 0.5`.
    #[default]
    NumericOnly,
    /// CSS Sizing L4 §4.5 `allow-keywords` value — keyword sizes resolve
    /// to their px value at transition start, enabling smooth animations.
    AllowKeywords,
}

/// CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContainerType {
    #[default]
    Normal,
    Size,
    InlineSize,
}

/// Resolved container dimensions, passed during style re-computation for container queries.
/// CSS Container Queries L1 §3: size features (width, height) evaluated against this context.
#[derive(Debug, Clone)]
pub struct ContainerContext {
    /// Content width of the container element in pixels.
    pub width: f32,
    /// Content height if definite (None when auto/unknown).
    pub height: Option<f32>,
    /// The container's `container-name` values (for named queries).
    pub names: Vec<String>,
    /// Custom properties (`--*`) контейнера — для style() queries (CSS Containment L3 §4).
    /// Shares the container's own [`CustomProps`] allocation, so building a
    /// `ContainerContext` costs no map copy.
    pub custom_props: CustomProps,
    /// Container's own computed style, serialized the same way as
    /// `window.getComputedStyle()` (`selector_query::computed_style_to_map`) —
    /// used to resolve `style()` queries against standard (non-custom) properties.
    pub style_props: HashMap<String, String>,
    /// Container's own font-size — the `em` basis when resolving relative
    /// units in a `style()` query's declared value.
    pub font_size: f32,
    /// Viewport size — the `vw`/`vh`/`vmin`/`vmax` basis when resolving
    /// relative units in a `style()` query's declared value.
    pub viewport: Size,
    /// Height of the *container's own* containing block (its immediate
    /// parent's content box) — the CSS2.1 §10.5 basis for resolving `%` in
    /// the container's own `height`/`top`/`bottom`/`min-height`/
    /// `max-height` during a `style()` query. Distinct from `height` above,
    /// which is the container's own (already-resolved) content height
    /// exposed to descendants for `(min-height: …)`-style size queries.
    /// Always a concrete pixel value: by the time `ContainerContext` is
    /// built, the whole tree has already been laid out to definite rects,
    /// so this doesn't distinguish an explicitly-sized parent from one whose
    /// own height was itself content-derived (CSS2.1 §10.5's "if the height
    /// of the containing block is not specified explicitly… the percentage
    /// value computes to auto" is not modeled here).
    pub own_containing_block_height: f32,
    /// Which query kinds this container answers (`container-type`).
    pub kinds: ContainerKinds,
    /// `scroll-state()` state of a `container-type: scroll-state` container.
    pub scroll_state: ScrollStateInfo,
    /// The next container up the ancestor chain. A query resolves its own
    /// container per feature kind (CSS Conditional 5 §«container selection»):
    /// the nearest ancestor with the right `container-type` and, for a named
    /// `@container`, the right `container-name`.
    pub parent: Option<Arc<ContainerContext>>,
    /// Whether each `sheet.container_rules[i]` query holds under this container, in rule
    /// order — the answer depends on the container, not on the element, so the second pass
    /// computes it once per container ([`ContainerContext::with_rule_hits`]) instead of once
    /// per box. Empty: not precomputed, [`matching_container_rules`] evaluates the query.
    pub rule_hits: Vec<bool>,
}

impl ContainerContext {
    /// `self` with [`Self::rule_hits`] computed against `sheet`.
    pub fn with_rule_hits(mut self, sheet: &Stylesheet) -> Self {
        self.rule_hits = sheet
            .container_rules
            .iter()
            .map(|r| evaluate_container_rule(r, &self))
            .collect();
        self
    }
}

/// Query kinds a container answers — the `container-type` axes of
/// CSS Conditional 5 §3.1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContainerKinds {
    /// `size` / `inline-size`: answers size features (`width`, `height`, …).
    pub size: bool,
    /// `scroll-state`: answers `scroll-state(…)`.
    pub scroll_state: bool,
    /// The box cannot take size containment (CSS Conditional 5 §size-container): a size
    /// query that picks this container is unknown, and does not fall through to an outer one.
    pub ineligible: bool,
}

/// `display` values whose box cannot be a size container: no principal box
/// (`contents`), non-atomic inline-level boxes, internal table boxes and tables.
pub fn size_containment_ineligible(display: crate::style::Display) -> bool {
    use crate::style::Display::*;
    matches!(
        display,
        Contents | Inline | Table | InlineTable | TableRowGroup | TableHeaderGroup | TableFooterGroup | TableRow
            | TableColumnGroup | TableColumn | TableCell
    )
}

impl ContainerKinds {
    /// A size container.
    pub const SIZE: Self = Self { size: true, scroll_state: false, ineligible: false };
}

/// Evaluates a raw @container condition string against a `ContainerContext`.
///
/// Phase 0: handles `(min-width: Npx)`, `(max-width: Npx)`, `(min-height: Npx)`,
/// `(max-height: Npx)`, `(width: Npx)`, `(height: Npx)`, and `and`/`or`/`not` operators.
/// Also supports `style(--prop: value)` and boolean `style(--prop)` forms
/// (CSS Containment L3 §4). Custom-property style queries compare the container's
/// value against the query value as *normalized* token streams — internal runs of
/// whitespace collapse to a single space and whitespace around commas is removed,
/// so `style(--gap: 1px 2px)` matches a container declaring `--gap: 1px  2px` or
/// `--gap:1px 2px` (CSS Custom Properties L1 §2 «computed value is the specified
/// value with whitespace trimmed»). The container's declared value is `var()`-expanded
/// against its own `custom_props` map before comparison — e.g. a container with
/// `--base: 8px; --gap: var(--base);` matches `style(--gap: 8px)` — mirroring how
/// `var()` is substituted when a custom property is consumed elsewhere in the cascade.
/// Standard (non-custom) properties are compared against the container's own
/// computed style (`ctx.style_props`, same serialization as `getComputedStyle()`):
/// `style(display: flex)` matches a container computed to `display: flex`. The
/// comparison is case-insensitive after the same whitespace/comma normalization
/// used for custom properties, so it works for keyword and length values whose
/// author-written form matches the serialized form (`style(width: 100px)` against
/// a computed `100px`); if that normalized comparison fails, both sides are also
/// tried as CSS colors and as lengths (`style_query_value_matches`), so
/// `style(color: red)` matches a computed `rgb(255, 0, 0)`, `style(border-width:
/// 2pt)` matches a computed `2.6667px`, and relative lengths (`em`, `%`,
/// viewport units) resolve against the container's own `font_size`/`viewport`
/// (`style(width: 1em)` matches a computed `16px` on a container whose
/// font-size is `16px`) — the same `em`/viewport basis `cq*` units use,
/// since a `style()` query's declared value is evaluated as if specified on
/// the container element itself (CSS Containment L3 §4). The `%` basis is
/// picked per queried property by `style_query_percent_basis` — the
/// container's width by default, but its own font-size for `line-height` and
/// its own containing block's height for `height`/`top`/`bottom`/
/// `min-height`/`max-height`.
/// Boolean form (`style(--prop)` / `style(prop)` without a value) is true when the
/// container has any value for that property — for custom properties this checks
/// `custom_props`, for standard properties `style_props` (a standard property never
/// computes to the custom-property-only guaranteed-invalid value, so in practice
/// this is true whenever the container's computed style was resolved for it).
/// A single `style()` call may itself combine multiple property queries with
/// `and`/`or`/`not`, each wrapped in its own parentheses — e.g.
/// `style((--a: 1) and (--b: 2))` or `style(not (display: none))` — per the
/// formal grammar (`<style-query> = <style-condition> | <style-feature>`,
/// CSS Containment L3 §5.2); see `evaluate_style_query`.
/// Phase 0 limitations:
/// - `state()` container queries: not a Lumen gap — the CSS Containment L3
///   spec itself removed/deferred state query features, so there is nothing
///   to implement against.
/// - Vertical box-model properties (`margin-top`/`margin-bottom`/
///   `padding-top`/`padding-bottom`) resolve `%` against the container's
///   width per CSS2.1 §8.3/§10.3 (correct — the containing block width is
///   the basis for *all four* margin/padding sides).
/// - `height`/`top`/`bottom`/`min-height`/`max-height` resolve `%` against
///   `ContainerContext::own_containing_block_height` — the container's own
///   immediate parent's content height, correctly distinct from the
///   container's own size or width (see that field's doc). The one
///   remaining approximation: this value is always treated as definite,
///   since Lumen's post-layout box tree no longer distinguishes a parent
///   whose height was explicitly specified from one whose height was itself
///   content-derived (CSS2.1 §10.5 would compute the `%` as `auto` in the
///   latter case).
///
/// Unknown features → false (safe fallback).
pub fn evaluate_container_condition(condition: &str, ctx: &ContainerContext) -> bool {
    evaluate_container_query(condition, None, ctx)
}

/// The `@container` rule applies under `ctx`: its own query and the ones of the
/// `@container` rules it is nested in all hold, each against its own container.
pub fn evaluate_container_rule(rule: &lumen_css_parser::ContainerRule, ctx: &ContainerContext) -> bool {
    rule.preludes().all(|(name, condition)| evaluate_container_query(condition, name, ctx))
}

/// Three-valued truth of a container condition: `None` is «unknown»
/// (CSS Conditional 5 §«general-enclosed»; `unknown` and `not unknown` are both
/// unknown, so a rule with an unknown condition never applies).
pub(super) type Tri = Option<bool>;

fn tri_not(t: Tri) -> Tri {
    t.map(|b| !b)
}

fn tri_and(a: Tri, b: Tri) -> Tri {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn tri_or(a: Tri, b: Tri) -> Tri {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

/// `not`/`and`/`or`/parenthesised-group structure shared by container conditions,
/// `style()` and `scroll-state()`; `leaf` evaluates one bare feature (text without
/// its parentheses) or a whole `style(…)`/`scroll-state(…)` function.
pub(super) fn eval_tri(s: &str, leaf: &dyn Fn(&str) -> Tri) -> Tri {
    let s = s.trim();
    if s.len() > 3 && s.is_char_boundary(3) && s[..3].eq_ignore_ascii_case("not") {
        let rest = &s[3..];
        if rest.starts_with(char::is_whitespace) {
            return tri_not(eval_tri(rest, leaf));
        }
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " and ") {
        return tri_and(eval_tri(lhs, leaf), eval_tri(rhs, leaf));
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " or ") {
        return tri_or(eval_tri(lhs, leaf), eval_tri(rhs, leaf));
    }
    if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return eval_tri(inner, leaf);
    }
    leaf(s)
}

/// Nearest container on the chain starting at `ctx` that answers `want` and (for a
/// named `@container`) carries `name`.
fn resolve_container<'a>(
    ctx: &'a ContainerContext,
    name: Option<&str>,
    want: impl Fn(&ContainerContext) -> bool,
) -> Option<&'a ContainerContext> {
    let mut cur = Some(ctx);
    while let Some(c) = cur {
        if want(c) && name.is_none_or(|n| c.names.iter().any(|x| x == n)) {
            return Some(c);
        }
        cur = c.parent.as_deref();
    }
    None
}

/// Size features the engine knows by name: evaluated for the `name: length` form,
/// `false` for the forms not modelled yet (range syntax, `aspect-ratio`, `orientation`).
fn is_known_size_feature(name: &str) -> bool {
    let base = name.strip_prefix("min-").or_else(|| name.strip_prefix("max-")).unwrap_or(name);
    matches!(base, "width" | "height" | "inline-size" | "block-size" | "aspect-ratio" | "orientation")
}

/// Evaluates a raw `@container` prelude against the container chain starting at
/// `ctx`; `name` is the `@container <name>` the parser split off the first entry.
/// A comma list is true when any entry is (`<container-condition>#`); every entry after
/// the first carries its own leading `<name>`. «Unknown» counts as false.
pub fn evaluate_container_query(condition: &str, name: Option<&str>, ctx: &ContainerContext) -> bool {
    split_top_level_commas(condition).into_iter().enumerate().any(|(i, entry)| {
        let (entry_name, body) = if i == 0 { (name, entry.trim()) } else { split_leading_name(entry) };
        evaluate_container_entry(body, entry_name, ctx)
    })
}

/// The prelude asks a `style()` question — every element is a style container, so the
/// second pass has to give each one a context (`box_tree::container_anchor`).
pub fn query_mentions_style(condition: &str) -> bool {
    condition.to_ascii_lowercase().contains("style(")
}

/// A `style()` in the prelude may read a standard property (`style(color: red)`,
/// `style(display)`), not only `--*`: the container's `style_props` map is needed.
/// Conservative: a custom-property-only prelude is `false`, anything else `true`.
pub fn query_reads_standard_property(condition: &str) -> bool {
    let lower = condition.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("style(") {
        rest = &rest[i + "style(".len()..];
        // The first feature, and every feature after a `(` of a condition form.
        let mut starts = vec![0usize];
        starts.extend(rest.match_indices('(').map(|(j, _)| j + 1));
        if starts.into_iter().any(|j| {
            rest[j..].trim_start().chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        }) {
            return true;
        }
    }
    false
}

/// Container features a query reads — they pick the one query container (CSS
/// Conditional 5 §«container selection»): the nearest ancestor that answers all of them.
/// A size feature is eligible on its own axis: `width` needs the inline axis, `height` the
/// block axis (only `container-type: size` has it).
#[derive(Default, Clone, Copy)]
struct Needs {
    inline: bool,
    block: bool,
    scroll_state: bool,
}

impl Needs {
    fn or(self, o: Needs) -> Needs {
        Needs { inline: self.inline | o.inline, block: self.block | o.block, scroll_state: self.scroll_state | o.scroll_state }
    }
}

/// Axes a size-feature name asks for; `aspect-ratio`/`orientation` read both.
fn feature_needs(name: &str) -> Needs {
    let base = name.strip_prefix("min-").or_else(|| name.strip_prefix("max-")).unwrap_or(name);
    match base {
        "width" | "inline-size" => Needs { inline: true, ..Needs::default() },
        "height" | "block-size" => Needs { block: true, ..Needs::default() },
        "aspect-ratio" | "orientation" => Needs { inline: true, block: true, ..Needs::default() },
        _ => Needs::default(),
    }
}

fn leaf_needs(s: &str) -> Needs {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("scroll-state(") {
        return Needs { scroll_state: true, ..Needs::default() };
    }
    if lower.starts_with("style(") {
        return Needs::default();
    }
    if s.contains(['<', '>', '=']) && !s.contains(':') {
        // Range syntax: every feature name among the operands.
        return lower
            .split(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
            .filter(|w| !w.is_empty())
            .fold(Needs::default(), |n, w| n.or(feature_needs(w)));
    }
    let head: String = lower.chars().take_while(|c| c.is_ascii_alphabetic() || *c == '-').collect();
    feature_needs(&head)
}

fn evaluate_container_entry(condition: &str, name: Option<&str>, ctx: &ContainerContext) -> bool {
    // `@container <name> { … }`: no query, true when a container of that name exists.
    if condition.trim().is_empty() {
        return name.is_some() && resolve_container(ctx, name, |_| true).is_some();
    }
    let needs = std::cell::Cell::new(Needs::default());
    eval_tri(condition, &|leaf| {
        needs.set(needs.get().or(leaf_needs(leaf)));
        None
    });
    let needs = needs.get();
    let Some(c) = resolve_container(ctx, name, |c| {
        (!needs.inline || c.kinds.size)
            && (!needs.block || (c.kinds.size && c.height.is_some()))
            && (!needs.scroll_state || c.kinds.scroll_state)
    }) else {
        return false;
    };
    if c.kinds.ineligible && (needs.inline || needs.block) {
        return false;
    }
    eval_tri(condition, &|leaf| container_leaf(leaf, c)) == Some(true)
}

/// Splits `s` at the commas outside parentheses.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// `<name>? <query>` of a comma-list entry after the first: the leading identifier
/// (not `not`, not a function) is the container name.
fn split_leading_name(entry: &str) -> (Option<&str>, &str) {
    let entry = entry.trim();
    let end = entry.find(|c: char| c.is_whitespace() || c == '(').unwrap_or(entry.len());
    let (head, rest) = entry.split_at(end);
    if head.is_empty() || rest.starts_with('(') || head.eq_ignore_ascii_case("not") || rest.trim().is_empty() {
        return (None, entry);
    }
    (Some(head), rest.trim_start())
}

/// Axis a size feature measures: the inline axis (`width`/`inline-size`) is always
/// there on a size container, the block axis only on `container-type: size`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Inline,
    Block,
}

fn axis_of(feature: &str) -> Option<Axis> {
    match feature {
        "width" | "inline-size" => Some(Axis::Inline),
        "height" | "block-size" => Some(Axis::Block),
        _ => None,
    }
}

/// The container's size on `axis`; `None` when the container does not expose it.
fn axis_size(c: &ContainerContext, axis: Axis) -> Option<f32> {
    match axis {
        Axis::Inline => Some(c.width),
        Axis::Block => c.height,
    }
}

fn compare(l: f32, op: &str, r: f32) -> Option<bool> {
    Some(match op {
        "<" => l < r,
        "<=" => l <= r,
        ">" => l > r,
        ">=" => l >= r,
        "=" => (l - r).abs() < 0.5,
        _ => return None,
    })
}

/// Range syntax: `<feature> <op> <length>`, `<length> <op> <feature>`,
/// `<length> <op> <feature> <op> <length>` (CSS Conditional 5 §«range context»).
fn range_feature(s: &str, c: &ContainerContext) -> Tri {
    let (mut operands, mut ops): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    let b = s.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        if matches!(b[i], b'<' | b'>' | b'=') {
            let mut j = i + 1;
            if b[i] != b'=' && b.get(j) == Some(&b'=') {
                j += 1;
            }
            operands.push(s[start..i].trim());
            ops.push(&s[i..j]);
            (start, i) = (j, j);
        } else {
            i += 1;
        }
    }
    operands.push(s[start..].trim());
    if !(2..=3).contains(&operands.len()) || operands.iter().any(|o| o.is_empty()) {
        return None;
    }
    let mut features = 0;
    let mut values: Vec<Option<f32>> = Vec::new();
    for o in &operands {
        let lower = o.to_ascii_lowercase();
        if let Some(axis) = axis_of(&lower) {
            features += 1;
            values.push(axis_size(c, axis));
        } else {
            values.push(Some(parse_css_length_to_px(o)?));
        }
    }
    if features != 1 {
        return None;
    }
    let mut result = true;
    for (w, op) in values.windows(2).zip(&ops) {
        result &= compare(w[0]?, op, w[1]?)?;
    }
    Some(result)
}

fn container_leaf(s: &str, c: &ContainerContext) -> Tri {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("style(") && s.ends_with(')') {
        return evaluate_style_query(&s[6..s.len() - 1], c);
    }
    if lower.starts_with("scroll-state(") && s.ends_with(')') {
        return evaluate_scroll_state_query(&s["scroll-state(".len()..s.len() - 1], &c.scroll_state);
    }
    // Feature: `feature: value` (the parentheses were stripped by `eval_tri`).
    let Some(pos) = s.find(':') else {
        if s.contains(['<', '>', '=']) {
            return range_feature(s, c);
        }
        // Boolean context: a size feature is true when the container has a nonzero size on
        // that axis; `orientation`/`aspect-ratio` need both axes and are not modelled.
        let head = s.trim().to_ascii_lowercase();
        if let Some(axis) = axis_of(&head) {
            return axis_size(c, axis).map(|v| v > 0.0);
        }
        return is_known_size_feature(&head).then(|| c.height.is_some().then_some(false)).flatten();
    };
    let feature = s[..pos].trim().to_ascii_lowercase();
    let value = s[pos + 1..].trim();
    if !is_known_size_feature(&feature) {
        return None;
    }
    let base = feature.strip_prefix("min-").or_else(|| feature.strip_prefix("max-")).unwrap_or(&feature);
    let Some(axis) = axis_of(base) else {
        // `orientation`/`aspect-ratio`: both axes or nothing.
        return c.height.is_some().then_some(false);
    };
    let size = axis_size(c, axis)?;
    let Some(v) = parse_css_length_to_px(value) else { return Some(false) };
    Some(if feature.starts_with("min-") {
        size >= v
    } else if feature.starts_with("max-") {
        size <= v
    } else {
        (size - v).abs() < 0.5
    })
}

/// Evaluates the content of a `style()` container query — CSS Containment L3
/// §5.2. Per the formal grammar (`<style-query> = <style-condition> |
/// <style-feature>`, `<style-condition> = not <style-in-parens> |
/// <style-in-parens> [and <style-in-parens>]* | [or <style-in-parens>]*`,
/// `<style-in-parens> = (<style-condition>) | (<style-feature>)`), a single
/// `style()` call may combine multiple property queries with `and`/`or`/`not`,
/// each wrapped in its own parentheses (e.g. `style((--a: 1) and (--b: 2))`,
/// `style(not (display: none))`). `<style-feature>` itself always queries
/// exactly one property — the grammar has no comma-separated multi-declaration
/// form — handled by `evaluate_style_feature`.
fn evaluate_style_query(s: &str, ctx: &ContainerContext) -> Tri {
    eval_tri(s, &|feature| {
        // A property name is an identifier: `scroll-state(…)` inside `style()` is
        // `<general-enclosed>` (unknown), not a property that is merely unset.
        let name = feature.split(':').next().unwrap_or("").trim();
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        valid.then(|| evaluate_style_feature(feature, ctx))
    })
}

/// Evaluates a single `<style-feature>` (CSS Containment L3 §5.2): the boolean
/// form `prop` (true iff the container has any value for it) or the
/// declaration form `prop: value` (true iff the container's own value matches,
/// with the same custom-property/var()-expansion and standard-property
/// canonicalization as `evaluate_container_condition`'s `style()` handling).
fn evaluate_style_feature(feature: &str, ctx: &ContainerContext) -> bool {
    let inner = feature.trim();
    // Boolean form: `--prop` or `prop`.
    if !inner.contains(':') {
        let name = inner.trim();
        if name.starts_with("--") {
            return resolve_container_custom_prop(ctx, name).is_some_and(|v| !v.trim().is_empty());
        }
        // Standard property: true if the container's computed style has
        // any value for it (a standard property never computes to the
        // custom-property-only guaranteed-invalid value).
        return ctx
            .style_props
            .get(&name.to_ascii_lowercase())
            .is_some_and(|v| !v.trim().is_empty());
    }
    // Declaration form: `--prop: value` or `prop: value`.
    if let Some((name, value)) = inner.split_once(':') {
        let name = name.trim();
        // `!important` in the query value is not part of the compared value.
        let value = value.trim_end();
        let value = value
            .len()
            .checked_sub("!important".len())
            .filter(|&i| value.is_char_boundary(i) && value[i..].eq_ignore_ascii_case("!important"))
            .map_or(value, |i| value[..i].trim_end());
        let want = normalize_style_value(value);
        if name.starts_with("--") {
            let have = resolve_container_custom_prop(ctx, name).map(|v| normalize_style_value(&v));
            if value.contains("var(") {
                // The query value substitutes `var()` against the container; a reference that
                // fails leaves it guaranteed-invalid, which only an unset property equals.
                return match expand_vars(value, &ctx.custom_props, 0, ctx.font_size, ctx.viewport) {
                    Some(v) => have == Some(normalize_style_value(&v)),
                    None => have.is_none(),
                };
            }
            return have == Some(want);
        }
        // Standard property: compare against the container's own computed
        // style (case-insensitive — CSS keywords are ASCII case-insensitive).
        let name_lower = name.to_ascii_lowercase();
        return ctx
            .style_props
            .get(&name_lower)
            .is_some_and(|v| style_query_value_matches(v, &want, &name_lower, ctx));
    }
    false
}

/// Resolves a container's custom property for a `style()` query: looks up `name`
/// in `ctx.custom_props` and expands any `var()` references against that same map
/// (CSS Variables L1 §3), so a chain like `--base: 8px; --gap: var(--base);`
/// resolves `--gap` to `8px` before comparison. Returns `None` if the property is
/// absent or its `var()` chain fails to resolve (unknown reference, no fallback,
/// or recursion past `VAR_EXPAND_MAX_DEPTH`).
fn resolve_container_custom_prop(ctx: &ContainerContext, name: &str) -> Option<String> {
    let raw = ctx.custom_props.get(name)?;
    expand_vars(raw, &ctx.custom_props, 0, ctx.font_size, ctx.viewport)
}

/// Normalizes a custom-property value for `style()` query comparison.
///
/// Collapses each run of ASCII whitespace to a single space, trims the ends, and
/// removes whitespace immediately around commas. This mirrors how a custom
/// property's computed value drops insignificant whitespace between tokens
/// (CSS Custom Properties L1 §2), so equivalent declarations compare equal
/// regardless of the author's spacing (`1px 2px` == `1px  2px`, `a,b` == `a, b`).
fn normalize_style_value(s: &str) -> String {
    // First collapse internal whitespace runs to single spaces.
    let collapsed: String = {
        let mut out = String::with_capacity(s.len());
        let mut prev_ws = false;
        for ch in s.trim().chars() {
            if ch.is_ascii_whitespace() {
                if !prev_ws {
                    out.push(' ');
                }
                prev_ws = true;
            } else {
                out.push(ch);
                prev_ws = false;
            }
        }
        out
    };
    // Then strip the spaces that sit directly around commas.
    let mut out = String::with_capacity(collapsed.len());
    let bytes = collapsed.as_bytes();
    for (i, ch) in collapsed.char_indices() {
        if ch == ' ' {
            let next_is_comma = bytes.get(i + 1) == Some(&b',');
            let prev_is_comma = i > 0 && bytes[i - 1] == b',';
            if next_is_comma || prev_is_comma {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Compares a container's serialized computed-style value against a `style()`
/// query's declared value for a standard (non-custom) property.
///
/// First tries the normalized token comparison (`normalize_style_value`,
/// case-insensitive). If that fails, falls back to two context-free
/// canonicalizations, tried in order:
/// 1. CSS colors: both sides parsed and compared by resolved RGBA channels —
///    so `style(color: red)` matches a container computed to
///    `color: rgb(255, 0, 0)` (CSS Color L4 §4, equivalent notations denote
///    the same color).
/// 2. CSS lengths: both sides parsed via `parse_length`, then resolved to px
///    using `ctx` as the basis — `ctx.font_size` for `em`, `ctx.viewport` for
///    `vw`/`vh`/`vmin`/`vmax` (CSS Values L3 §5.2/§6.1; absolute units like
///    `pt` resolve independent of any basis) — so `style(border-width: 2pt)`
///    matches a computed `2.6667px`, and `style(width: 1em)` matches a
///    computed `16px` on a container whose font-size is `16px`. The `%`
///    basis is picked per `prop_name` by `style_query_percent_basis` — e.g.
///    `line-height`'s is the container's own font-size, not its width.
///    Values that need layout context beyond `ctx` (`min-content`, unresolved
///    `cq*` outside a re-layout pass) don't resolve and fall through to the
///    textual comparison's `false`.
///
/// `want` must already be normalized by the caller. `prop_name` must already
/// be lowercased by the caller.
fn style_query_value_matches(computed: &str, want: &str, prop_name: &str, ctx: &ContainerContext) -> bool {
    if normalize_style_value(computed).eq_ignore_ascii_case(want) {
        return true;
    }
    if let (Some(a), Some(b)) = (parse_color(computed), parse_color(want)) {
        return a == b;
    }
    if let (Some(a), Some(b)) = (parse_length(computed), parse_length(want)) {
        let basis = Some(style_query_percent_basis(prop_name, ctx));
        if let (Some(pa), Some(pb)) = (
            a.resolve(ctx.font_size, basis, ctx.viewport),
            b.resolve(ctx.font_size, basis, ctx.viewport),
        ) {
            return (pa - pb).abs() < 0.01;
        }
    }
    false
}

/// Picks the `%` reference basis (in px) for a `style()` query's declared
/// value, based on which standard property is being queried — CSS Values L3
/// §5.2's «the percentage is calculated with respect to X» is per-property,
/// not a single container size. Mirrors the handful of properties whose
/// basis differs from the common "containing block width" default:
/// - `line-height`: the element's own font-size (CSS Inline L3 §4.6.2),
///   which for a `style()` query is the container's own `font_size`.
/// - Vertical box-model properties (`height`, `top`/`bottom`, vertical
///   `min-`/`max-height`): the *container's own* containing block's height
///   (CSS2.1 §10.5) — `ctx.own_containing_block_height`, i.e. the height of
///   the container's parent content box, not the container's own height
///   (`ctx.height` is a different quantity: the container's own resolved
///   size, exposed to descendants for `(min-height: …)`-style size queries).
///
/// Every other property (including `margin-top`/`margin-bottom`/
/// `padding-top`/`padding-bottom`, which CSS2.1 §8.3/§10.3 defines against
/// the containing block *width* despite being vertical) falls back to the
/// container's width, unchanged from before this function existed.
fn style_query_percent_basis(prop_name: &str, ctx: &ContainerContext) -> f32 {
    match prop_name {
        "line-height" => ctx.font_size,
        "height" | "min-height" | "max-height" | "top" | "bottom" => {
            ctx.own_containing_block_height
        }
        _ => ctx.width,
    }
}

/// Parses a CSS length value to pixels (px / em not supported — just px for Phase 0).
fn parse_css_length_to_px(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix("px") {
        n.trim().parse::<f32>().ok()
    } else if let Some(n) = s.strip_suffix("em") {
        // Phase 0: treat em as px (approximate).
        n.trim().parse::<f32>().ok()
    } else {
        s.parse::<f32>().ok()
    }
}

/// Splits `s` on the first occurrence of `sep` that is not inside parentheses.
fn split_top_level_logical<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let sep_bytes = sep.as_bytes();
    let s_bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i + sep.len() <= s.len() {
        match s_bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && s_bytes[i..].starts_with(sep_bytes) {
            return Some((&s[..i], &s[i + sep.len()..]));
        }
        i += 1;
    }
    None
}

/// The `@container` rules of `sheet` whose query holds under `ctx` and whose selector
/// matches `node`, in sheet order.
pub fn matching_container_rules<'a>(
    doc: &Document,
    node: NodeId,
    sheet: &'a Stylesheet,
    ctx: &ContainerContext,
) -> Vec<&'a Rule> {
    let mut out = Vec::new();
    let precomputed = ctx.rule_hits.len() == sheet.container_rules.len();
    for (i, container_rule) in sheet.container_rules.iter().enumerate() {
        if precomputed && !ctx.rule_hits[i] {
            continue;
        }
        let matched = container_rule.rules.iter().filter(|rule| {
            rule.selectors.iter().any(|complex| matches_complex(complex, doc, node))
        });
        if precomputed {
            out.extend(matched);
            continue;
        }
        // Not precomputed: a query is evaluated only for a rule that selects the node.
        let matched: Vec<&Rule> = matched.collect();
        if !matched.is_empty()
            && evaluate_container_rule(container_rule, ctx)
        {
            out.extend(matched);
        }
    }
    out
}

/// Applies matching `@container` rules from `sheet` to `style`.
/// Called during the second layout pass for descendants of container elements.
/// `ctx` — the nearest container ancestor; a query picks the container it needs
/// (type, name) from the chain behind it.
pub fn apply_container_rules(
    style: &mut ComputedStyle,
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
    dark_mode: bool,
) {
    let rules = matching_container_rules(doc, node, sheet, ctx);
    apply_matched_container_rules(style, &rules, doc, node, viewport, dark_mode);
}

/// Applies the declarations of `rules` ([`matching_container_rules`]) to `style`.
pub fn apply_matched_container_rules(
    style: &mut ComputedStyle,
    rules: &[&Rule],
    doc: &Document,
    node: NodeId,
    viewport: Size,
    dark_mode: bool,
) {
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for rule in rules {
        let em = style.font_size;
        let pw = style.font_weight;
        let inherited = style.clone();
        for decl in &rule.declarations {
            // Custom properties are not a cascade of `apply_declaration` (compute_style
            // collects them in its own pass): a `@container` rule sets them here.
            if decl.property.starts_with("--") {
                let value = if decl.value.contains("var(") {
                    expand_vars(&decl.value, &style.custom_props, 0, em, viewport)
                } else {
                    Some(decl.value.clone())
                };
                if let Some(value) = value {
                    style.custom_props.make_mut().insert(decl.property.clone(), value);
                }
                continue;
            }
            let attr_buf;
            let effective_decl: &Declaration = if decl.value.contains("attr(") {
                let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
                attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
                &attr_buf
            } else {
                decl
            };
            apply_declaration(style, effective_decl, em, viewport, pw, &inherited, &inherited, is_quirks, dark_mode);
        }
    }
}
