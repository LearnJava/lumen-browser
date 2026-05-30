//! Test 13-visibility-opacity.html — visibility and opacity.
//!
//! 5 `.box` (120x80) alternating visibility visible/hidden, then 6 `.opbox`
//! (140x90) at opacity 0.1→1.0. `visibility: hidden` still occupies space, so
//! all five boxes are laid out and spaced one (width + margin) apart. opacity
//! never affects layout. We assert geometry, that hidden boxes still take their
//! slot, the first box's visibility, and the first opbox's opacity (0.1).

use lumen_driver::{BrowserSession, InProcessSession};
use lumen_layout::style::Visibility;

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
fn test_13_visibility_opacity() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/13-visibility-opacity.html");

    // visibility row: 5 boxes, all 120x80; hidden boxes still occupy space.
    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 5, "expected 5 visibility boxes");
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 120.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "box[{i}] should be 120x80, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
    // hidden boxes still take their slot: each sits at least one (width + margin)
    // = 136px to the right of the previous on the same row. The exact advance
    // also includes inter-inline-block whitespace, so we assert a tolerant range.
    for i in 1..boxes.len() {
        assert!(
            (boxes[i].border_box.y - boxes[0].border_box.y).abs() < 1.0,
            "box[{i}] should share row with box[0]"
        );
        let dx = boxes[i].border_box.x - boxes[i - 1].border_box.x;
        assert!(
            (136.0..=145.0).contains(&dx),
            "box[{i}] should advance ~136-145px past box[{}] (hidden still occupies space), got dx={dx}",
            i - 1
        );
    }

    // First box is `visibility: visible`.
    let box_style = session
        .computed_style_snapshot(".box")
        .expect("style")
        .expect("first box style");
    assert_eq!(
        box_style.visibility,
        Visibility::Visible,
        "first box should be visible"
    );

    // opacity row: 6 boxes, all 140x90 regardless of opacity.
    let opbox = session
        .all_layout_boxes_by_selector(".opbox")
        .expect("query .opbox");
    assert_eq!(opbox.len(), 6, "expected 6 opacity boxes");
    for (i, b) in opbox.iter().enumerate() {
        assert!(
            (b.border_box.width - 140.0).abs() < 1.0 && (b.border_box.height - 90.0).abs() < 1.0,
            "opbox[{i}] should be 140x90, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // First opbox is `opacity: 0.10`.
    let op_style = session
        .computed_style_snapshot(".opbox")
        .expect("style")
        .expect("first opbox style");
    assert!(
        (op_style.opacity - 0.10).abs() < 0.01,
        "first opbox opacity should be 0.10, got {}",
        op_style.opacity
    );
}
