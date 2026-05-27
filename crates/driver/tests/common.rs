//! Common helpers for graphic tests.

use lumen_driver::{BrowserSession, BoxModel};
use lumen_layout::style::Length;

/// Find a box by CSS selector. Uses the selector query to locate the element
/// and its position in layout.
pub fn find_box_by_selector(
    session: &impl BrowserSession,
    selector: &str,
) -> Option<BoxModel> {
    let boxes = session.layout_snapshot().ok()?;
    let style = session.computed_style_snapshot(selector).ok()??;

    // Find the box that matches the expected size and position from computed style
    let expected_width = style.width.and_then(|w| match w {
        Length::Px(v) => Some(v),
        _ => None,
    });
    let expected_height = style.height.and_then(|h| match h {
        Length::Px(v) => Some(v),
        _ => None,
    });

    boxes.iter().find(|b| {
        // Match by size if specified
        let width_matches = expected_width.map_or(true, |w| (b.border_box.width - w).abs() < 1.0);
        let height_matches = expected_height.map_or(true, |h| (b.border_box.height - h).abs() < 1.0);
        width_matches && height_matches
    }).cloned()
}

/// Assert that a computed style color matches the expected RGBA.
pub fn assert_color_matches(
    actual: Option<&lumen_layout::style::CssColor>,
    expected_rgb: (u8, u8, u8),
    alpha: f32,
    msg: &str,
) {
    match actual {
        Some(lumen_layout::style::CssColor::Rgba(color)) => {
            assert_eq!(color.r, expected_rgb.0, "{}: red channel mismatch", msg);
            assert_eq!(color.g, expected_rgb.1, "{}: green channel mismatch", msg);
            assert_eq!(color.b, expected_rgb.2, "{}: blue channel mismatch", msg);
            let actual_alpha = color.a as f32 / 255.0;
            assert!((actual_alpha - alpha).abs() < 0.01, "{}: alpha mismatch", msg);
        }
        other => panic!("{}: expected Rgba color, got {:?}", msg, other),
    }
}
