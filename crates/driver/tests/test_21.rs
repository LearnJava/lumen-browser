//! Test 21-border-style.html — dashed / dotted / double border styles.
//!
//! border-style is a paint-time concern; with box-sizing:border-box the declared
//! 180x80 border-box stays constant for every variant (dashed/dotted/double at
//! widths 2..16px, a per-side mix, and a sub-3px double that falls back to solid).
//! 15 `.b` boxes total (dashed×4 + dotted×4 + double×5 + per-side mix + thin double).

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
fn test_21_border_style() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/21-border-style.html");

    let boxes = session.all_layout_boxes_by_selector(".b").expect("query .b");
    assert_eq!(boxes.len(), 15, "expected 15 border-style demo boxes");

    // box-sizing:border-box → declared 180x80 border-box is invariant under any
    // border width/style (the border is drawn inside the box).
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 180.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "box[{i}] should stay 180x80 (border-box), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Inline-block row wraps at 4 boxes; box[4] starts a new row directly below
    // box[0] (same x, one row-step lower).
    assert!(
        (boxes[4].border_box.x - boxes[0].border_box.x).abs() < 1.0,
        "box[4] should wrap to a new row aligned under box[0]"
    );
    assert!(
        boxes[4].border_box.y > boxes[0].border_box.y + 80.0,
        "box[4] should be on a lower row than box[0]"
    );
}
