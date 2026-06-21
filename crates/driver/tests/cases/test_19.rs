//! Test 19-object-fit.html — object-fit / object-position.
//!
//! object-fit changes how image content is scaled WITHIN the replaced box; the
//! img layout box itself stays at its declared 180x120. 9 cells total.

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
fn test_19_object_fit() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/19-object-fit.html");

    let cells = session.all_layout_boxes_by_selector(".cell").expect("query .cell");
    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    let imgs = session.all_layout_boxes_by_selector("img").expect("query img");

    assert_eq!(cells.len(), 9, "expected 9 object-fit cells");
    assert_eq!(boxes.len(), 9, "expected 9 image boxes");
    assert_eq!(imgs.len(), 9, "expected 9 imgs");

    // Every img keeps its declared 180x120 regardless of object-fit mode.
    for (i, img) in imgs.iter().enumerate() {
        assert!(
            (img.border_box.width - 180.0).abs() < 1.0
                && (img.border_box.height - 120.0).abs() < 1.0,
            "img[{i}] layout box should be 180x120, got {}x{}",
            img.border_box.width,
            img.border_box.height
        );
    }
    // The .box container (with 1px border, box-sizing:border-box) is also 180x120.
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 180.0).abs() < 1.0 && (b.border_box.height - 120.0).abs() < 1.0,
            "box[{i}] should be 180x120, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
}
