//! Test 39-gradients.html — linear / radial / repeating gradients.
//!
//! 12 gradient swatches across 3 flex rows. A gradient is a paint-time
//! background-image and never affects layout, so each swatch keeps its declared
//! width/height. We assert the geometry of every gradient class.

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

fn assert_one(session: &InProcessSession, selector: &str, w: f32, h: f32) {
    let boxes = session
        .all_layout_boxes_by_selector(selector)
        .unwrap_or_else(|_| panic!("query {selector}"));
    assert_eq!(boxes.len(), 1, "expected exactly one {selector}");
    let b = &boxes[0];
    assert!(
        (b.border_box.width - w).abs() < 1.0 && (b.border_box.height - h).abs() < 1.0,
        "{selector} should be {w}x{h} (gradient is paint-time), got {}x{}",
        b.border_box.width,
        b.border_box.height
    );
}

#[test]
fn test_39_gradients() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/39-gradients.html");

    // Row 1: linear gradients.
    assert_one(&session, ".lg-to-right", 180.0, 80.0);
    assert_one(&session, ".lg-to-bottom", 180.0, 80.0);
    assert_one(&session, ".lg-45deg", 180.0, 80.0);
    assert_one(&session, ".lg-3stops", 180.0, 80.0);
    assert_one(&session, ".lg-transparent", 180.0, 80.0);

    // Row 2: radial gradients.
    assert_one(&session, ".rg-center", 180.0, 120.0);
    assert_one(&session, ".rg-offset", 180.0, 120.0);
    assert_one(&session, ".rg-3stops", 180.0, 120.0);
    assert_one(&session, ".rg-ellipse", 240.0, 120.0);

    // Row 3: repeating + stacked.
    assert_one(&session, ".rep-linear", 180.0, 80.0);
    assert_one(&session, ".rep-radial", 180.0, 80.0);
    assert_one(&session, ".stacked", 180.0, 80.0);
}
