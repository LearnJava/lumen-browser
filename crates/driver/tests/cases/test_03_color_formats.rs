//! Test 03-color-formats.html — CSS color format variations.
//!
//! 9 `.sw` swatches (200x100 inline-block, box-sizing: border-box) exercising
//! every CSS color notation: named, #RGB, #RRGGBB, #RGBA, #RRGGBBAA, rgb(),
//! rgba(), hsl(), hsla(). Parsing is paint-time; we assert geometry is intact
//! and that the first swatch (`tomato`) resolves to its sRGB value (255,99,71).

use lumen_driver::{BrowserSession, InProcessSession};

fn navigate(session: &mut InProcessSession, file: &str) {
    let root = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(root)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join(file);
    session
        .navigate(&format!("file://{}", path.display()))
        .expect("navigate");
}

#[test]
fn test_03_color_formats() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/03-color-formats.html");

    let sw = session.all_layout_boxes_by_selector(".sw").expect("query .sw");
    assert_eq!(sw.len(), 9, "expected 9 color-format swatches");

    for (i, b) in sw.iter().enumerate() {
        assert!(
            (b.border_box.width - 200.0).abs() < 1.0 && (b.border_box.height - 100.0).abs() < 1.0,
            "swatch[{i}] should be 200x100, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // First swatch is `background: tomato` → resolves to sRGB (255, 99, 71).
    let style = session
        .computed_style_snapshot(".sw")
        .expect("style")
        .expect("first swatch style");
    let bg = style
        .background_color
        .expect("swatch has background-color")
        .to_color_opt()
        .expect("not currentcolor");
    assert_eq!(
        (bg.r, bg.g, bg.b),
        (255, 99, 71),
        "named color `tomato` should resolve to (255,99,71)"
    );
}
