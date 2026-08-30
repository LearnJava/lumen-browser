//! Containment (`contain`/`content-visibility`/`interpolate-size`) и
//! CSS Container Queries: `container-type`, вычисление окружения запроса
//! (`ContainerContext`) и оценка условий (`evaluate_container_condition`,
//! `apply_container_rules`). Это не тип значения, а вычисление —
//! отдельный файл вне `style::values`.
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `struct ContainFlags` до конца `apply_container_rules`) без правок тел.

use std::collections::HashMap;

use lumen_core::geom::Size;
use lumen_css_parser::{Declaration, Specificity, Stylesheet};
use lumen_dom::{Document, DocumentMode, NodeId};

use crate::style::apply::apply_declaration;
use crate::style::computed::ComputedStyle;
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
    let s = condition.trim();
    // Handle `not (...)` and `not style(...)`.
    if let Some(rest) = s.strip_prefix("not") {
        let rest = rest.trim();
        if rest.starts_with('(') || rest.to_ascii_lowercase().starts_with("style(") {
            return !evaluate_container_condition(rest, ctx);
        }
    }
    // Split on top-level `and` / `or`.
    if let Some((lhs, rhs)) = split_top_level_logical(s, " and ") {
        return evaluate_container_condition(lhs, ctx) && evaluate_container_condition(rhs, ctx);
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " or ") {
        return evaluate_container_condition(lhs, ctx) || evaluate_container_condition(rhs, ctx);
    }
    // Handle `style(...)` queries.
    let s_lower = s.to_ascii_lowercase();
    if s_lower.starts_with("style(") && s.ends_with(')') {
        // Extract content between `style(` and the final `)`.
        let inner = s[6..s.len() - 1].trim();
        return evaluate_style_query(inner, ctx);
    }
    // Feature: `(feature: value)`.
    let inner = s.strip_prefix('(').and_then(|x| x.strip_suffix(')'));
    let inner = match inner {
        Some(i) => i.trim(),
        None => return false,
    };
    // Parse `feature: value`.
    let colon = inner.find(':');
    let (feature, value) = if let Some(pos) = colon {
        (inner[..pos].trim(), inner[pos + 1..].trim())
    } else {
        // Boolean feature (e.g. `(color)`) — unsupported in Phase 0.
        return false;
    };
    let px = parse_css_length_to_px(value);
    match (feature, px) {
        ("min-width", Some(v))  => ctx.width >= v,
        ("max-width", Some(v))  => ctx.width <= v,
        ("width", Some(v))      => (ctx.width - v).abs() < 0.5,
        ("min-height", Some(v)) => ctx.height.is_some_and(|h| h >= v),
        ("max-height", Some(v)) => ctx.height.is_none_or(|h| h <= v),
        ("height", Some(v))     => ctx.height.is_some_and(|h| (h - v).abs() < 0.5),
        _ => false,
    }
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
fn evaluate_style_query(s: &str, ctx: &ContainerContext) -> bool {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("not") {
        let rest = rest.trim();
        if rest.starts_with('(') {
            return !evaluate_style_query(rest, ctx);
        }
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " and ") {
        return evaluate_style_query(lhs, ctx) && evaluate_style_query(rhs, ctx);
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " or ") {
        return evaluate_style_query(lhs, ctx) || evaluate_style_query(rhs, ctx);
    }
    // `<style-in-parens>` grouping: strip one layer and recurse.
    if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return evaluate_style_query(inner, ctx);
    }
    // Leaf: a bare `<style-feature>` (boolean `prop` or declaration `prop: value`).
    evaluate_style_feature(s, ctx)
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
        let want = normalize_style_value(value);
        if name.starts_with("--") {
            return resolve_container_custom_prop(ctx, name).map(|v| normalize_style_value(&v))
                == Some(want);
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
    expand_vars(raw, &ctx.custom_props, 0)
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

/// Applies matching `@container` rules from `sheet` to `style`.
/// Called during the second layout pass for descendants of container elements.
/// `ctx` — resolved size of the nearest container ancestor.
pub fn apply_container_rules(
    style: &mut ComputedStyle,
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
    dark_mode: bool,
) {
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for container_rule in &sheet.container_rules {
        // Name filter: if the rule has a name, the context must include that name.
        if container_rule.name.as_ref().is_some_and(|rule_name| {
            !ctx.names.iter().any(|n| n == rule_name)
        }) {
            continue;
        }
        if !evaluate_container_condition(&container_rule.condition, ctx) {
            continue;
        }
        // Apply declarations from matching rules.
        for rule in &container_rule.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if best.is_some() {
                let em = style.font_size;
                let pw = style.font_weight;
                let inherited = style.clone();
                for decl in &rule.declarations {
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
    }
}

