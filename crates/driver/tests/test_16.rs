//! Test 16-outline.html — outline width/color/offset.
//!
//! outline is painted outside the border-box and must NOT affect layout:
//! all 11 `.b` boxes stay 120x80, and a thick outline on a middle box does not
//! shift its neighbours.

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
fn test_16_outline() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/16-outline.html");

    let boxes = session.all_layout_boxes_by_selector(".b").expect("query .b");
    assert_eq!(boxes.len(), 11, "expected 11 outline demo boxes");

    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 120.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "box[{i}] should stay 120x80 (outline must not affect layout), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Last row (3 boxes, 16px margin-right + inline whitespace between them):
    // the middle box has a 12px outline but the third box is NOT pushed right —
    // outline takes no layout space. The two X steps must be equal.
    let last3 = &boxes[8..11];
    let step1 = last3[1].border_box.x - last3[0].border_box.x;
    let step2 = last3[2].border_box.x - last3[1].border_box.x;
    assert!(
        (step1 - step2).abs() < 1.0,
        "12px outline on middle box must not shift the third box; \
         steps should be equal, got {step1} vs {step2}"
    );
    // Step = width(120) + margin-right(16) + inline whitespace(~4px).
    assert!(
        (step2 - 140.0).abs() < 2.0,
        "last-row X step should be ~140, got {step2}"
    );
}
