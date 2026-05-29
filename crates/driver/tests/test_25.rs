//! Test 25-table-layout.html — auto/fixed column widths and row stacking.
//!
//! Three tables: t1 = 4 auto-width cells under a 440px table → 110px each;
//! t2 = 5 explicit-width cells (60/120/80/100/80); t3 = 3×4 grid of 100x60 cells
//! that stack into three rows. Asserts column widths and row offsets.

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
fn test_25_table_layout() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/25-table-layout.html");

    // t1: 4 auto-width cells split a 440px table into equal 110px columns.
    let t1 = session.all_layout_boxes_by_selector(".t1 td").expect("query .t1 td");
    assert_eq!(t1.len(), 4, "t1 expected 4 cells");
    for (i, c) in t1.iter().enumerate() {
        assert!(
            (c.border_box.width - 110.0).abs() < 1.0 && (c.border_box.height - 80.0).abs() < 1.0,
            "t1 cell[{i}] should be 110x80, got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }

    // t2: explicit column widths in document order.
    let t2 = session.all_layout_boxes_by_selector(".t2 td").expect("query .t2 td");
    assert_eq!(t2.len(), 5, "t2 expected 5 cells");
    let t2_w = [60.0_f32, 120.0, 80.0, 100.0, 80.0];
    for (i, c) in t2.iter().enumerate() {
        assert!(
            (c.border_box.width - t2_w[i]).abs() < 1.0 && (c.border_box.height - 80.0).abs() < 1.0,
            "t2 cell[{i}] should be {}x80, got {}x{}",
            t2_w[i],
            c.border_box.width,
            c.border_box.height
        );
    }

    // t3: 3 rows × 4 cells, each 100x60; rows stack 60px apart.
    let t3 = session.all_layout_boxes_by_selector(".t3 td").expect("query .t3 td");
    assert_eq!(t3.len(), 12, "t3 expected 12 cells (3×4)");
    for (i, c) in t3.iter().enumerate() {
        assert!(
            (c.border_box.width - 100.0).abs() < 1.0 && (c.border_box.height - 60.0).abs() < 1.0,
            "t3 cell[{i}] should be 100x60, got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }
    // Cell 0 (row1 col1), cell 4 (row2 col1), cell 8 (row3 col1) share x and step 60px in y.
    assert!(
        (t3[4].border_box.x - t3[0].border_box.x).abs() < 1.0
            && (t3[8].border_box.x - t3[0].border_box.x).abs() < 1.0,
        "t3 first column should share x across rows"
    );
    assert!(
        (t3[4].border_box.y - t3[0].border_box.y - 60.0).abs() < 1.0
            && (t3[8].border_box.y - t3[4].border_box.y - 60.0).abs() < 1.0,
        "t3 rows should stack 60px apart, got {} and {}",
        t3[4].border_box.y - t3[0].border_box.y,
        t3[8].border_box.y - t3[4].border_box.y
    );
}
