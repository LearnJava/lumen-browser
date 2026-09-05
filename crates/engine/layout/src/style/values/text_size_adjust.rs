//! CSS Text Size Adjustment Module L1 — `text-size-adjust` / legacy alias
//! `-webkit-text-size-adjust`.
//!
//! <https://drafts.csswg.org/css-size-adjust-1/#adjustment-control>
//! BUG-513. Inherited; initial value `auto`. Phase 0: parse + store +
//! interpolate — no mobile auto-inflation rendering pipeline exists, so the
//! value has CSSOM/animation observability only, no rendering effect (same
//! shape as `dynamic_range_limit`/BUG-508).
//!
//! `none` is a legacy synonym whose *computed* value is `100%` (spec
//! §propdef) — [`TextSizeAdjust::parse`] folds it into `Percentage(100.0)`
//! immediately, so there is no separate `None` variant to keep in sync with
//! `Percentage` elsewhere (interpolation, serialization).

use lumen_core::geom::Size;

use crate::style::Length;
use crate::style::calc::calc_node_contains_percent;
use crate::style::values::length::parse_length_q;

/// CSS Text Size Adjustment Module L1 §2 — `text-size-adjust` computed value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextSizeAdjust {
    Auto,
    /// `<percentage [0,∞]>` — also the computed value of the legacy `none`
    /// keyword (`Percentage(100.0)`).
    Percentage(f32),
}

impl Default for TextSizeAdjust {
    /// Initial value per spec §2: `auto`.
    fn default() -> Self {
        Self::Auto
    }
}

impl TextSizeAdjust {
    /// Parses a specified value: `auto | none | <percentage [0,∞]>`.
    /// `em_basis`/`viewport` back `calc()` percentage arguments (CSS Values
    /// L4 §10, e.g. `calc(10% * sign(1em))`).
    pub fn parse(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<Self> {
        let t = s.trim();
        let lower = t.to_ascii_lowercase();
        match lower.as_str() {
            "auto" => return Some(Self::Auto),
            "none" => return Some(Self::Percentage(100.0)),
            _ => {}
        }
        let len = parse_length_q(t, is_quirks)?;
        match len {
            Length::Percent(v) if v >= 0.0 => Some(Self::Percentage(v)),
            Length::Calc(node) if calc_node_contains_percent(&node) => {
                let v = node.resolve(em_basis, Some(100.0), viewport)?;
                if v >= 0.0 { Some(Self::Percentage(v)) } else { None }
            }
            _ => None,
        }
    }

    /// Linear interpolation between two computed values (regular
    /// interpolation of `<percentage>`, CSS Values and Units L4 §17.2.2) —
    /// discrete when either side is `auto` (nothing numeric to lerp
    /// against), clamped to `[0,∞)` per the property's own grammar.
    pub fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        match (a, b) {
            (Self::Percentage(pa), Self::Percentage(pb)) => Self::Percentage((pa + (pb - pa) * t).max(0.0)),
            _ => {
                if t < 0.5 { *a } else { *b }
            }
        }
    }

    /// Serializes back to canonical CSS text.
    pub fn to_css(&self) -> String {
        match self {
            Self::Auto => "auto".to_string(),
            Self::Percentage(v) => fmt_percent(*v),
        }
    }
}

/// Compact percentage formatting (`50%` not `50.000%`), rounded to 4 decimal
/// places to snap out float noise from `calc()` arithmetic (e.g.
/// `calc(10% + 5%)` landing on `15.000001` instead of `15.0`) — same
/// convention as `dynamic_range_limit::fraction_to_rounded_percent`.
fn fmt_percent(v: f32) -> String {
    let rounded: f32 = (v * 10_000.0).round() / 10_000.0;
    if rounded.fract() == 0.0 { format!("{}%", rounded as i64) } else { format!("{rounded}%") }
}
