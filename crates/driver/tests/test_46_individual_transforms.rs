//! Test 46-individual-transforms.html — CSS Transforms L2 individual props.
//!
//! 10 `.box` swatches (80x80) driven by the individual `translate` / `rotate` /
//! `scale` properties (and combinations with the `transform` shorthand). These
//! are paint-time transforms and must not alter the layout box, so every `.box`
//! keeps its declared 80x80 size.

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
fn test_46_individual_transforms() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/46-individual-transforms.html");

    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 10, "expected 10 transform boxes (5 rows × 2)");

    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 80.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "box[{i}] should stay 80x80 (individual transforms are paint-time), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
}
