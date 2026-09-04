//! CSS Color HDR L1 §2 — `dynamic-range-limit` / `dynamic-range-limit-mix()`.
//!
//! <https://drafts.csswg.org/css-color-hdr/#the-dynamic-range-limit-property>
//! BUG-508. Inherited; initial value `no-limit`. Phase 0: parse + store +
//! interpolate — no HDR display pipeline exists yet, so the value has no
//! rendering effect, only CSSOM observability. [`DynamicRangeLimit::interpolate`]
//! is a pure library function, mirrored in the Web Animations JS shim
//! (`_wa_lerp_dynamic_range_limit`, `web_api_shim_tail_b.js`) — but neither
//! that nor the native CSS Animations/Transitions engine (`crate::animation`,
//! a hardcoded 5-property table) actually calls it yet; this property does
//! not move computed style through any animation today (ДОРАБОТКА, see
//! `bugs/BUG-508-OPEN.md` — the live WPT `interpolation.html` "pass" is a
//! same-tick `getComputedStyle` vacuous-read artifact, not real coverage).
//!
//! Computed value collapses arbitrarily nested `dynamic-range-limit-mix()`
//! calls into a flat weighted mix of the three base keywords
//! (`standard`/`constrained`/`no-limit`), normalized so the listed
//! percentages sum to 100 and zero-weight components are dropped; if only
//! one component survives, the computed value is the bare keyword.

use lumen_core::geom::Size;

use crate::style::calc::calc_node_contains_percent;
use crate::style::values::length::parse_length_q;
use crate::style::{Length, split_top_level_commas};

/// The three base keywords `dynamic-range-limit-mix()` mixes between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicRangeLimitKeyword {
    Standard,
    Constrained,
    NoLimit,
}

impl DynamicRangeLimitKeyword {
    /// CSS text for this keyword, as used both in specified values and in
    /// `dynamic-range-limit-mix()` component serialization.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Constrained => "constrained",
            Self::NoLimit => "no-limit",
        }
    }
}

/// A flattened, normalized `dynamic-range-limit-mix()` computed value: three
/// percentages (0..=100) summing to 100, in canonical `standard,
/// constrained, no-limit` order. A component absent from serialization is
/// `0.0` here rather than omitted from the struct — [`DynamicRangeLimit`]
/// never constructs a `Mix` with fewer than two nonzero components (that
/// collapses to `Keyword` instead).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicRangeLimitMix {
    pub standard: f32,
    pub constrained: f32,
    pub no_limit: f32,
}

/// CSS Color HDR L1 §2 — `dynamic-range-limit` computed value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DynamicRangeLimit {
    Keyword(DynamicRangeLimitKeyword),
    Mix(DynamicRangeLimitMix),
}

impl Default for DynamicRangeLimit {
    /// Initial value per spec §2: `no-limit`.
    fn default() -> Self {
        Self::Keyword(DynamicRangeLimitKeyword::NoLimit)
    }
}

/// Rounds a `0.0..=1.0` fraction to a `0.0..=100.0` percentage, snapping out
/// float noise from repeated division (e.g. component weights like `60/200`)
/// to 4 decimal places — well below anything a test or author would specify.
fn fraction_to_rounded_percent(frac: f64) -> f32 {
    (((frac * 100.0) * 10_000.0).round() / 10_000.0) as f32
}

/// Splits `s` on top-level whitespace (not inside `(...)`) — the
/// space-separated-token analogue of [`split_top_level_commas`], needed to
/// pull apart `<keyword> <percentage>` (in either order) inside one
/// `dynamic-range-limit-mix()` component.
fn split_top_level_whitespace(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(i);
                }
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// Parses one mix-component's percentage token: `<percentage [0,100]>`.
/// A literal out of range is invalid (parse failure); a `calc()`-derived
/// percentage is clamped, per CSS Values L4 §10 range-clamping for computed
/// expressions.
fn parse_strict_percentage(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    let len = parse_length_q(s, is_quirks)?;
    match &len {
        Length::Percent(v) => {
            if *v < 0.0 || *v > 100.0 { None } else { Some(*v) }
        }
        Length::Calc(node) if calc_node_contains_percent(node) => {
            let v = node.resolve(em_basis, Some(100.0), viewport)?;
            Some(v.clamp(0.0, 100.0))
        }
        _ => None,
    }
}

/// Parses the non-percentage half of a mix component: either a base keyword
/// (contributing 100% weight to itself) or a nested `dynamic-range-limit-mix(
/// ...)` (contributing its own recursively-flattened fractions). Returns
/// `[standard, constrained, no_limit]` fractions summing to `1.0`.
fn parse_value_token(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<[f64; 3]> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "standard" => Some([1.0, 0.0, 0.0]),
        "constrained" => Some([0.0, 1.0, 0.0]),
        "no-limit" => Some([0.0, 0.0, 1.0]),
        _ if lower.starts_with("dynamic-range-limit-mix(") && t.ends_with(')') => {
            let body = &t["dynamic-range-limit-mix(".len()..t.len() - 1];
            parse_mix_body(body, em_basis, viewport, is_quirks)
        }
        _ => None,
    }
}

/// Parses and flattens the comma-separated body of one `dynamic-range-limit-
/// mix(...)` call (at any nesting depth) into `[standard, constrained,
/// no_limit]` fractions summing to `1.0`. `None` on any grammar violation or
/// when the listed percentages sum to zero (nothing to normalize against).
fn parse_mix_body(body: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<[f64; 3]> {
    let parts = split_top_level_commas(body);
    if parts.len() < 2 {
        return None;
    }
    let mut components: Vec<([f64; 3], f64)> = Vec::with_capacity(parts.len());
    let mut total = 0.0f64;
    for part in &parts {
        let tokens = split_top_level_whitespace(part.trim());
        let [tok_a, tok_b] = tokens.as_slice() else { return None };
        // `<value> && <percentage>` — either order; exactly one token must
        // parse as the percentage half.
        let pct_a = parse_strict_percentage(tok_a, em_basis, viewport, is_quirks);
        let pct_b = parse_strict_percentage(tok_b, em_basis, viewport, is_quirks);
        let (pct, value_tok) = match (pct_a, pct_b) {
            (Some(p), None) => (p, *tok_b),
            (None, Some(p)) => (p, *tok_a),
            _ => return None,
        };
        let vec = parse_value_token(value_tok, em_basis, viewport, is_quirks)?;
        total += f64::from(pct);
        components.push((vec, f64::from(pct)));
    }
    if total <= 0.0 {
        return None;
    }
    let mut out = [0.0f64; 3];
    for (vec, pct) in &components {
        let w = pct / total;
        out[0] += w * vec[0];
        out[1] += w * vec[1];
        out[2] += w * vec[2];
    }
    Some(out)
}

impl DynamicRangeLimit {
    /// Parses a specified `dynamic-range-limit` value: a bare keyword or a
    /// (possibly nested) `dynamic-range-limit-mix(...)`. `em_basis`/
    /// `viewport` back `calc()` percentage arguments (CSS Values L4 §10,
    /// e.g. `calc(50% * sign(10em - 1px))`).
    pub fn parse(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<Self> {
        let t = s.trim();
        let lower = t.to_ascii_lowercase();
        match lower.as_str() {
            "standard" => return Some(Self::Keyword(DynamicRangeLimitKeyword::Standard)),
            "constrained" => return Some(Self::Keyword(DynamicRangeLimitKeyword::Constrained)),
            "no-limit" => return Some(Self::Keyword(DynamicRangeLimitKeyword::NoLimit)),
            _ => {}
        }
        if lower.starts_with("dynamic-range-limit-mix(") && t.ends_with(')') {
            let body = &t["dynamic-range-limit-mix(".len()..t.len() - 1];
            let frac = parse_mix_body(body, em_basis, viewport, is_quirks)?;
            return Some(Self::from_fractions(frac[0], frac[1], frac[2]));
        }
        None
    }

    /// Builds the computed value from `standard`/`constrained`/`no_limit`
    /// fractions of `1.0` (i.e. already-normalized mix weights), rounding
    /// away float noise and collapsing to a bare [`Keyword`](Self::Keyword)
    /// when at most one component survives rounding.
    fn from_fractions(standard: f64, constrained: f64, no_limit: f64) -> Self {
        Self::from_components(
            fraction_to_rounded_percent(standard),
            fraction_to_rounded_percent(constrained),
            fraction_to_rounded_percent(no_limit),
        )
    }

    /// Builds the computed value from `standard`/`constrained`/`no_limit`
    /// percentages (`0..=100`, expected to already sum to `100`) — the
    /// entry point animation interpolation uses, since lerping two
    /// already-normalized `(std, con, nl)` triples keeps the sum at 100
    /// without renormalizing.
    pub fn from_components(standard: f32, constrained: f32, no_limit: f32) -> Self {
        const EPS: f32 = 1e-6;
        let nonzero = [standard, constrained, no_limit].iter().filter(|v| v.abs() > EPS).count();
        if nonzero <= 1 {
            if standard.abs() > EPS {
                return Self::Keyword(DynamicRangeLimitKeyword::Standard);
            }
            if constrained.abs() > EPS {
                return Self::Keyword(DynamicRangeLimitKeyword::Constrained);
            }
            return Self::Keyword(DynamicRangeLimitKeyword::NoLimit);
        }
        Self::Mix(DynamicRangeLimitMix {
            standard: standard.max(0.0),
            constrained: constrained.max(0.0),
            no_limit: no_limit.max(0.0),
        })
    }

    /// `(standard, constrained, no_limit)` percentages summing to 100 — a
    /// bare keyword reads as 100% on itself.
    pub fn components(&self) -> (f32, f32, f32) {
        match self {
            Self::Keyword(DynamicRangeLimitKeyword::Standard) => (100.0, 0.0, 0.0),
            Self::Keyword(DynamicRangeLimitKeyword::Constrained) => (0.0, 100.0, 0.0),
            Self::Keyword(DynamicRangeLimitKeyword::NoLimit) => (0.0, 0.0, 100.0),
            Self::Mix(m) => (m.standard, m.constrained, m.no_limit),
        }
    }

    /// Componentwise linear interpolation between two computed values (CSS
    /// Color HDR L1 §2.1 interpolation: treat as a weighted mix vector, lerp
    /// each of the three weights, no renormalization needed since both
    /// inputs already sum to 100).
    pub fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        let (sa, ca, na) = a.components();
        let (sb, cb, nb) = b.components();
        Self::from_components(
            sa + (sb - sa) * t,
            ca + (cb - ca) * t,
            na + (nb - na) * t,
        )
    }

    /// Serializes back to canonical CSS text — `standard`/`constrained`/
    /// `no-limit` for a bare keyword, `dynamic-range-limit-mix(standard N%,
    /// constrained N%, no-limit N%)` (only nonzero components, canonical
    /// `standard, constrained, no-limit` order) for a mix.
    pub fn to_css(&self) -> String {
        match self {
            Self::Keyword(k) => k.as_str().to_string(),
            Self::Mix(m) => {
                let mut parts = Vec::with_capacity(3);
                if m.standard > 0.0 {
                    parts.push(format!("standard {}", fmt_percent(m.standard)));
                }
                if m.constrained > 0.0 {
                    parts.push(format!("constrained {}", fmt_percent(m.constrained)));
                }
                if m.no_limit > 0.0 {
                    parts.push(format!("no-limit {}", fmt_percent(m.no_limit)));
                }
                format!("dynamic-range-limit-mix({})", parts.join(", "))
            }
        }
    }
}

/// Compact percentage formatting (`50` not `50.0`, `87.5` not `87.500000`) —
/// same convention as `position_component_to_css`/`bg_size_axis_to_css` in
/// `selector_query.rs`.
fn fmt_percent(v: f32) -> String {
    if v.fract() == 0.0 { format!("{}%", v as i64) } else { format!("{v}%") }
}
