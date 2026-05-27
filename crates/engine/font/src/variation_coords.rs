//! Normalized variation coordinates for a font instance.
//!
//! Stores per-axis normalized values in `[-1.0, 1.0]` range (or `[0.0, 1.0]` for
//! postScriptSlantAngle after avar mapping). Used by rasterizer to apply gvar
//! deltas when rendering variable-font glyphs.
//!
//! Phase 0 usage:
//! - `VariationCoords` is built from CSS `font-variation-settings` values.
//! - Each axis is normalized relative to `fvar.axis[i].default` via linear
//!   interpolation in `[min, default]` or `[default, max]` range.
//! - avar mapping is applied if present (post-normalization axis value mapping).
//! - Optical sizing (`font-optical-sizing` CSS property + `opsz` axis) handled
//!   by inserting a CSS-derived opsz value into the coordinate vector.

use crate::fvar::Fvar;
use crate::avar::Avar;

/// Normalized variation coordinates for a font instance. Stores one f32 per axis
/// in the font's `fvar` table, in normalized `[-1.0, 1.0]` range (or applied
/// through avar mapping). Index of each coord matches the index of the axis in
/// `Fvar::axes`.
///
/// # Invariants
/// - Length equals font's axis count (from `fvar.axes.len()`).
/// - Each value is in roughly `[-1.0, 1.0]` range (avar may expand slightly).
/// - Ownership model: caller's responsibility to ensure axis count matches gvar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VariationCoords(pub Vec<f32>);

impl VariationCoords {
    /// Creates an empty coordinate vector (no variations applied; uses default
    /// instance).
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Builds normalized coordinates from CSS `font-variation-settings` values.
    ///
    /// Each CSS value is matched to an axis in `fvar` by tag. Missing axes are
    /// filled with their default value. Values are clamped to `[axis.min, axis.max]`
    /// and then linearly normalized to `[-1.0, 1.0]` relative to the axis's default.
    /// avar mapping is applied if present.
    ///
    /// Returns empty if `fvar` is not present or not variable.
    pub fn from_css_settings(
        fvar: &Fvar,
        avar: &Avar,
        css_settings: &[([u8; 4], f32)],
    ) -> Self {
        if !fvar.is_variable() {
            return Self::empty();
        }

        let mut coords = Vec::with_capacity(fvar.axes.len());
        for (axis_idx, axis) in fvar.axes.iter().enumerate() {
            // Find user-supplied value for this axis tag, or use default.
            let user_val = css_settings
                .iter()
                .find(|(tag, _)| tag == &axis.tag)
                .map_or(axis.default, |(_, v)| *v);

            // Clamp to valid range.
            let clamped = axis.clamp(user_val);

            // Normalize to [-1.0, 1.0] relative to default.
            let linear = if (clamped - axis.default).abs() < f32::EPSILON {
                0.0
            } else if clamped < axis.default {
                let range = axis.default - axis.min;
                if range < f32::EPSILON {
                    0.0
                } else {
                    (clamped - axis.default) / range
                }
            } else {
                let range = axis.max - axis.default;
                if range < f32::EPSILON {
                    0.0
                } else {
                    (clamped - axis.default) / range
                }
            };

            // Apply avar mapping (post-normalization axis value mapping).
            coords.push(avar.normalize(axis_idx, linear));
        }

        Self(coords)
    }

    /// Returns the coordinate vector as a slice.
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    /// Returns the coordinate vector as a mutable slice (for P4 to update optical sizing).
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        &mut self.0
    }

    /// Returns true if no coordinates are set (default instance).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of axes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Gets coordinate for a specific axis by tag (for debugging / CSS property hookup).
    ///
    /// Returns None if axis not found or coords not built yet.
    pub fn get_axis_by_tag(&self, fvar: &Fvar, tag: [u8; 4]) -> Option<f32> {
        fvar.axes
            .iter()
            .position(|a| a.tag == tag)
            .and_then(|idx| self.0.get(idx).copied())
    }

    /// Sets a specific axis coordinate by tag.
    /// Returns true if successful, false if axis not found.
    ///
    /// Used by P4 to inject optical sizing (`opsz` axis) or other CSS-driven
    /// variations.
    pub fn set_axis_by_tag(&mut self, fvar: &Fvar, tag: [u8; 4], value: f32) -> bool {
        if let Some(idx) = fvar.axes.iter().position(|a| a.tag == tag) {
            if idx < self.0.len() {
                self.0[idx] = value;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_coords_are_empty() {
        let coords = VariationCoords::empty();
        assert!(coords.is_empty());
        assert_eq!(coords.len(), 0);
    }

    #[test]
    fn coords_from_empty_fvar_returns_empty() {
        let fvar = Fvar::default();
        let avar = Avar::default();
        let settings = &[];
        let coords = VariationCoords::from_css_settings(&fvar, &avar, settings);
        assert!(coords.is_empty());
    }
}
