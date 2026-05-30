//! Test 41-table.html — display:table layout + global column sizing.
//!
//! Table 1 (`.tbl`) is a thead/tbody/tfoot row-group table: 4 columns × 5 rows of
//! `.td` cells (header/footer 40px tall, body 60px). Table 2 (`.tbl2`) is the
//! load-bearing case for *global column width*: each column takes the MAX declared
//! width across rows, so both rows resolve to columns [120, 200, 200] even though
//! the per-cell widths differ. The native `<table>` provides 9 `td`-tag cells.

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
fn test_41_table() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/41-table.html");

    // `.td` class cells: table1 (4×5 = 20) + table2 (3×2 = 6) = 26.
    let cells = session.all_layout_boxes_by_selector(".td").expect("query .td");
    assert_eq!(cells.len(), 26, "expected 26 .td cells across tables 1 and 2");

    // Table 1 row heights by section: header[0..4] = 40, body[4..16] = 60, footer[16..20] = 40.
    for (i, c) in cells[0..4].iter().enumerate() {
        assert!(
            (c.border_box.width - 120.0).abs() < 1.0 && (c.border_box.height - 40.0).abs() < 1.0,
            "header cell[{i}] should be 120x40, got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }
    for (i, c) in cells[4..16].iter().enumerate() {
        assert!(
            (c.border_box.width - 120.0).abs() < 1.0 && (c.border_box.height - 60.0).abs() < 1.0,
            "body cell[{i}] should be 120x60, got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }
    for (i, c) in cells[16..20].iter().enumerate() {
        assert!(
            (c.border_box.width - 120.0).abs() < 1.0 && (c.border_box.height - 40.0).abs() < 1.0,
            "footer cell[{i}] should be 120x40, got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }

    // Table 2: both rows resolve to the SAME column widths [120, 200, 200] — the max
    // of each column's declared widths — proving global column alignment.
    let row1 = &cells[20..23];
    let row2 = &cells[23..26];
    let expected_cols = [120.0, 200.0, 200.0];
    for (col, &w) in expected_cols.iter().enumerate() {
        assert!(
            (row1[col].border_box.width - w).abs() < 1.0,
            "table2 row1 col{col} should be {w}px, got {}",
            row1[col].border_box.width
        );
        assert!(
            (row2[col].border_box.width - w).abs() < 1.0,
            "table2 row2 col{col} should be {w}px (global column width), got {}",
            row2[col].border_box.width
        );
    }

    // Native <table>: 3 cols × 3 rows = 9 td-tag cells, each 120px wide.
    let native = session.all_layout_boxes_by_selector("td").expect("query td");
    assert_eq!(native.len(), 9, "expected 9 native td cells");
    for (i, c) in native.iter().enumerate() {
        assert!(
            (c.border_box.width - 120.0).abs() < 1.0,
            "native td[{i}] should be 120px wide, got {}",
            c.border_box.width
        );
    }
}
