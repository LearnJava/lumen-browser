//! Per-tab page zoom logic.
//!
//! Browser zoom works by shrinking or enlarging the *CSS layout viewport*:
//! `effective_viewport = physical_viewport / zoom_factor`.
//! A zoom_factor > 1.0 means the layout uses fewer CSS px (content appears larger);
//! < 1.0 means the layout is wider than the physical window (zoomed out / smaller text).
//!
//! The zoom factor is independent of `<meta name=viewport initial-scale>`.  Both
//! compose multiplicatively via [`effective_viewport`].

/// Default page zoom — 100%.
pub const ZOOM_DEFAULT: f32 = 1.0;
/// Minimum allowed zoom — 25%.
pub const ZOOM_MIN: f32 = 0.25;
/// Maximum allowed zoom — 400%.
pub const ZOOM_MAX: f32 = 4.0;
/// Zoom step per Ctrl+= or Ctrl+- key press.
pub const ZOOM_STEP: f32 = 0.1;

/// Increase zoom by one step, clamped to [`ZOOM_MAX`].
pub fn zoom_in(current: f32) -> f32 {
    (current + ZOOM_STEP).min(ZOOM_MAX)
}

/// Decrease zoom by one step, clamped to [`ZOOM_MIN`].
pub fn zoom_out(current: f32) -> f32 {
    (current - ZOOM_STEP).max(ZOOM_MIN)
}

/// Reset zoom to 100%.
pub fn zoom_reset() -> f32 {
    ZOOM_DEFAULT
}

/// Compute the CSS layout viewport size from the physical window size.
///
/// `meta_initial_scale` comes from `<meta name=viewport initial-scale=N>` (default 1.0).
/// `zoom_factor` is the user-controlled browser zoom.
/// Both factors compose multiplicatively: a larger combined scale → smaller layout viewport.
pub fn effective_viewport(
    physical_width: f32,
    physical_height: f32,
    meta_initial_scale: f32,
    zoom_factor: f32,
) -> (f32, f32) {
    let scale = (meta_initial_scale * zoom_factor).max(f32::EPSILON);
    (physical_width / scale, physical_height / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_in_clamps_at_max() {
        assert!((zoom_in(ZOOM_MAX)).abs() <= ZOOM_MAX + f32::EPSILON);
    }

    #[test]
    fn zoom_out_clamps_at_min() {
        assert!((zoom_out(ZOOM_MIN)) >= ZOOM_MIN - f32::EPSILON);
    }

    #[test]
    fn zoom_reset_returns_default() {
        assert_eq!(zoom_reset(), ZOOM_DEFAULT);
    }

    #[test]
    fn effective_viewport_no_scale() {
        let (w, h) = effective_viewport(1024.0, 768.0, 1.0, 1.0);
        assert!((w - 1024.0).abs() < 0.01);
        assert!((h - 768.0).abs() < 0.01);
    }

    #[test]
    fn effective_viewport_zoom_in() {
        // zoom=2.0 → layout sees half the pixels → 512×384 CSS px
        let (w, h) = effective_viewport(1024.0, 768.0, 1.0, 2.0);
        assert!((w - 512.0).abs() < 0.01);
        assert!((h - 384.0).abs() < 0.01);
    }

    #[test]
    fn effective_viewport_meta_scale() {
        // initial-scale=2.0, zoom=1.0 → same as zoom=2.0
        let (w, h) = effective_viewport(1024.0, 768.0, 2.0, 1.0);
        assert!((w - 512.0).abs() < 0.01);
        assert!((h - 384.0).abs() < 0.01);
    }

    #[test]
    fn effective_viewport_combined_scale() {
        // initial-scale=2.0, zoom=2.0 → 4× total scale → 256×192 CSS px
        let (w, h) = effective_viewport(1024.0, 768.0, 2.0, 2.0);
        assert!((w - 256.0).abs() < 0.01);
        assert!((h - 192.0).abs() < 0.01);
    }
}
