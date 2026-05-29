//! Test 45-multiple-backgrounds.html — CSS Backgrounds L3 §3 multiple background layers.
//!
//! Nine 200x120 `.box` swatches each stack several background layers (gradients,
//! background-position/size/repeat/clip/origin). Multiple backgrounds are paint-only:
//! none of them affects layout, so every `.box` keeps its declared 200x120 border-box
//! (box-sizing:border-box). The load-bearing check is that the layer count/flavour
//! never perturbs geometry, and the flex row advances by 216px (200 + 16px gap).

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
fn test_45_multiple_backgrounds() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/45-multiple-backgrounds.html");

    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 9, "expected 9 multi-background swatches");

    // Every swatch keeps 200x120 regardless of how many background layers it stacks.
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 200.0).abs() < 1.0 && (b.border_box.height - 120.0).abs() < 1.0,
            "box[{i}] should be 200x120 (backgrounds are paint-only), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Row 1 has four swatches advancing by 216px (200 width + 16px gap).
    for i in 0..3 {
        assert!(
            (boxes[i + 1].border_box.x - boxes[i].border_box.x - 216.0).abs() < 1.0,
            "row1 flex step should be 216px (200 + 16 gap), got {}",
            boxes[i + 1].border_box.x - boxes[i].border_box.x
        );
    }

    // Row 2 wraps under row 1: the 5th swatch shares x with the 1st but sits lower.
    assert!(
        (boxes[4].border_box.x - boxes[0].border_box.x).abs() < 1.0
            && boxes[4].border_box.y > boxes[0].border_box.y + 100.0,
        "the 5th swatch should start a new row under the 1st"
    );
}
