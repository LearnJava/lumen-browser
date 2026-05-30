//! Test 02-color-named.html — CSS named colors.
//!
//! 18 `.sw` swatches (140x80 inline-block, box-sizing: border-box) each painted
//! with a named CSS color. Color resolution is paint-time; here we assert the
//! box geometry is unaffected and that the first swatch (`red`) resolves to the
//! concrete sRGB value (255, 0, 0).

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
fn test_02_color_named() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/02-color-named.html");

    let sw = session.all_layout_boxes_by_selector(".sw").expect("query .sw");
    assert_eq!(sw.len(), 18, "expected 18 named-color swatches");

    for (i, b) in sw.iter().enumerate() {
        assert!(
            (b.border_box.width - 140.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "swatch[{i}] should be 140x80, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // First swatch is `background: red` → resolves to sRGB (255, 0, 0).
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
        (255, 0, 0),
        "named color `red` should resolve to (255,0,0)"
    );
}
