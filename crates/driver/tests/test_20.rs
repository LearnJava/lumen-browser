//! Test 20-quirks-bgcolor.html — legacy hashless hex + bgcolor attribute.
//!
//! Five `.sw` swatches (160x80 inline-block row) colored via hashless hex, plus
//! a 2-row × 5-cell table where each `td` (120x80) is colored via the legacy
//! `bgcolor` presentational attribute. Color parsing is paint-time; here we only
//! assert that the box geometry is unaffected.

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
fn test_20_quirks_bgcolor() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/20-quirks-bgcolor.html");

    let sw = session.all_layout_boxes_by_selector(".sw").expect("query .sw");
    let td = session.all_layout_boxes_by_selector("td").expect("query td");

    assert_eq!(sw.len(), 5, "expected 5 hashless-hex swatches");
    assert_eq!(td.len(), 10, "expected 10 table cells (2 rows × 5)");

    // Swatches keep their declared 160x80 regardless of color-parsing mode.
    for (i, b) in sw.iter().enumerate() {
        assert!(
            (b.border_box.width - 160.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "swatch[{i}] should be 160x80, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
    // Every table cell is 120x80; bgcolor attribute does not affect layout.
    for (i, c) in td.iter().enumerate() {
        assert!(
            (c.border_box.width - 120.0).abs() < 1.0 && (c.border_box.height - 80.0).abs() < 1.0,
            "td[{i}] should be 120x80, got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }

    // The two table rows stack vertically: the 6th cell (row 2, col 1) sits one
    // row-height (80px) below the 1st cell, at the same x.
    let row1_first = &td[0];
    let row2_first = &td[5];
    assert!(
        (row2_first.border_box.x - row1_first.border_box.x).abs() < 1.0,
        "row-2 first cell should share x with row-1 first cell"
    );
    assert!(
        (row2_first.border_box.y - row1_first.border_box.y - 80.0).abs() < 1.0,
        "row-2 should sit 80px below row-1, got dy={}",
        row2_first.border_box.y - row1_first.border_box.y
    );
}
