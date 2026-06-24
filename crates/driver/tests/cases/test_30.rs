//! Test 30-css-filter.html — filter / backdrop-filter (paint-only).
//!
//! filter and backdrop-filter are paint-time effects and must not change layout.
//! Verified by checking that all boxes sharing a class keep identical geometry
//! regardless of which filter is applied: 8 `.box` (flex-shrunk but equal), 2
//! `.blur-base` (180x100), 3 `.hue-box` (100x100), 6 `.bd-card` (90x100+1px=92x102).

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
fn test_30_css_filter() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/30-css-filter.html");

    // Row 1: 8 `.box` items flex-shrink to an equal width; each filter variant must
    // produce the SAME geometry (filter is paint-only). Height is 120 + 2×3px = 126.
    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 8, "expected 8 filter boxes");
    let w0 = boxes[0].border_box.width;
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - w0).abs() < 1.0 && (b.border_box.height - 126.0).abs() < 1.0,
            "box[{i}] should match box[0] geometry ({w0}x126), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Row 2: blur filter does not change the 180x100 layout box.
    let blur = session.all_layout_boxes_by_selector(".blur-base").expect("query .blur-base");
    assert_eq!(blur.len(), 2, "expected 2 blur-base boxes");
    for (i, b) in blur.iter().enumerate() {
        assert!(
            (b.border_box.width - 180.0).abs() < 1.0 && (b.border_box.height - 100.0).abs() < 1.0,
            "blur-base[{i}] should be 180x100 (blur is paint-only), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Row 3: hue-rotate does not change the 100x100 layout box.
    let hue = session.all_layout_boxes_by_selector(".hue-box").expect("query .hue-box");
    assert_eq!(hue.len(), 3, "expected 3 hue boxes");
    for (i, b) in hue.iter().enumerate() {
        assert!(
            (b.border_box.width - 100.0).abs() < 1.0 && (b.border_box.height - 100.0).abs() < 1.0,
            "hue-box[{i}] should be 100x100, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Row 4: backdrop-filter cards keep their 90x100 + 1px border → 92x102 box.
    let cards = session.all_layout_boxes_by_selector(".bd-card").expect("query .bd-card");
    assert_eq!(cards.len(), 6, "expected 6 backdrop-filter cards");
    for (i, c) in cards.iter().enumerate() {
        assert!(
            (c.border_box.width - 92.0).abs() < 1.0 && (c.border_box.height - 102.0).abs() < 1.0,
            "bd-card[{i}] should be 92x102 (backdrop-filter is paint-only), got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }
}
