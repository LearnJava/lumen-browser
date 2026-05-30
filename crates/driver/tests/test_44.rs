//! Test 44-media-queries.html — @media screen/print/min-width/max-width/orientation/aspect-ratio.
//!
//! Six 316x224 boxes flex-wrap into two rows of three. Each box's background is set
//! by a different media query; at the 1024x720 viewport the load-bearing result is:
//! screen ✓ (b1 green), print ✗ (b2 stays dark), min-width:48em ✓ (b3 blue),
//! max-width:50rem ✗ (b4 stays dark), orientation:landscape ✓ (b5 purple),
//! min-aspect-ratio:1/1 ✓ (b6 cyan). Verified via computed background-color.

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

fn bg_rgb(session: &InProcessSession, sel: &str) -> String {
    session
        .computed_style(sel)
        .expect("computed_style")
        .unwrap_or_else(|| panic!("{sel} not found"))
        .properties
        .get("background-color")
        .cloned()
        .unwrap_or_default()
}

#[test]
fn test_44_media_queries() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/44-media-queries.html");

    // Six boxes, all 316x224, flex-wrap into two rows of three.
    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 6, "expected 6 media-query boxes");
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.width - 316.0).abs() < 1.0 && (b.border_box.height - 224.0).abs() < 1.0,
            "box[{i}] should be 316x224, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
    // Three per row: b3 ends row 1, b4 wraps to row 2 (same x as b1, lower y).
    assert!(
        (boxes[3].border_box.x - boxes[0].border_box.x).abs() < 1.0
            && boxes[3].border_box.y > boxes[0].border_box.y + 100.0,
        "the 4th box should wrap to a new row under the 1st"
    );

    // Media-query results via computed background-color (rgb triplet is alpha-agnostic).
    // Applied queries paint a colour; non-applied leave the #333333 default.
    assert!(bg_rgb(&session, "#b1").contains("34,197,94"), "@media screen → b1 green");
    assert!(bg_rgb(&session, "#b2").contains("51,51,51"), "@media print should NOT apply → b2 dark");
    assert!(bg_rgb(&session, "#b3").contains("59,130,246"), "min-width:48em → b3 blue");
    assert!(bg_rgb(&session, "#b4").contains("51,51,51"), "max-width:50rem should NOT apply → b4 dark");
    assert!(bg_rgb(&session, "#b5").contains("168,85,247"), "orientation:landscape → b5 purple");
    assert!(bg_rgb(&session, "#b6").contains("6,182,212"), "min-aspect-ratio:1/1 → b6 cyan");
}
