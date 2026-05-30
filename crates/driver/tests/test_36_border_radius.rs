//! Test 36-border-radius.html — border-radius variations.
//!
//! Cards exercising uniform/percent/elliptical/per-corner radii, with and
//! without borders. `border-radius` is a paint-time corner clip and never
//! changes the layout box; with box-sizing: border-box the border does not grow
//! the box either. We assert every card class keeps its declared geometry.

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

fn assert_all(session: &InProcessSession, selector: &str, count: usize, w: f32, h: f32) {
    let boxes = session
        .all_layout_boxes_by_selector(selector)
        .unwrap_or_else(|_| panic!("query {selector}"));
    assert_eq!(boxes.len(), count, "expected {count} of {selector}");
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - w).abs() < 1.0 && (b.border_box.height - h).abs() < 1.0,
            "{selector}[{i}] should be {w}x{h} (radius is paint-time), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
}

#[test]
fn test_36_border_radius() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/36-border-radius.html");

    assert_all(&session, ".r1", 6, 100.0, 70.0); // uniform radius, no border
    assert_all(&session, ".r2", 6, 100.0, 70.0); // radius + 3px border (border-box)
    assert_all(&session, ".pill", 2, 140.0, 44.0); // pill (999px / 50%)
    assert_all(&session, ".circle", 3, 70.0, 70.0); // 50% → circle
    assert_all(&session, ".asym", 6, 110.0, 70.0); // per-corner radii
    assert_all(&session, ".clamp", 2, 80.0, 80.0); // large radius clamped
}
