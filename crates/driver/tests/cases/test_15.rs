//! Test 15-box-shadow.html — box-shadow variants.
//!
//! box-shadow is painted outside the border-box and must NOT affect layout:
//! all 8 `.b` boxes stay 120x80 and flow on one inline-block row.

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
fn test_15_box_shadow() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/15-box-shadow.html");

    let boxes = session.all_layout_boxes_by_selector(".b").expect("query .b");
    assert_eq!(boxes.len(), 8, "expected 8 box-shadow demo boxes");

    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 120.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "box[{i}] should stay 120x80 (shadow must not affect layout), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
        // margin-box must not be inflated by the shadow: width = 120 + margin-right(48).
        assert!(
            (b.margin_box.width - 168.0).abs() < 1.0,
            "box[{i}] margin-box width should be 168 (120 + 48 margin), got {}",
            b.margin_box.width
        );
    }
}
