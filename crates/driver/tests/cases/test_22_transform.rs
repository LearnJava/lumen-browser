//! Test 22-transform.html — CSS `transform` function variations.
//!
//! 5 rows × 6 `.cell` containers, each holding one 60x60 `.box` positioned
//! absolutely at top:20 left:30. `transform` (translate/rotate/scale/skew/
//! matrix/combinations) is applied at paint time and must not alter the layout
//! box, so every `.box` keeps its 60x60 size and its pre-transform offset
//! inside the cell.

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
fn test_22_transform() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/22-transform.html");

    let cells = session.all_layout_boxes_by_selector(".cell").expect("query .cell");
    assert_eq!(cells.len(), 30, "expected 30 transform cells (5 rows × 6)");

    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 30, "expected 30 transformed boxes");

    // transform is paint-time: every box keeps its declared 60x60 layout size.
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 60.0).abs() < 1.0 && (b.border_box.height - 60.0).abs() < 1.0,
            "box[{i}] should stay 60x60 (transform is paint-time), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // The first box sits at top:20 left:30 relative to its (position: relative)
    // cell's padding-box, which is inset by the cell's 1px border → offset 31/21.
    // transform must not move the layout box.
    assert!(
        (boxes[0].border_box.x - cells[0].border_box.x - 31.0).abs() < 1.0,
        "box[0] should be 31px right of cell[0] (30 left + 1px border), got dx={}",
        boxes[0].border_box.x - cells[0].border_box.x
    );
    assert!(
        (boxes[0].border_box.y - cells[0].border_box.y - 21.0).abs() < 1.0,
        "box[0] should be 21px below cell[0] (20 top + 1px border), got dy={}",
        boxes[0].border_box.y - cells[0].border_box.y
    );
}
