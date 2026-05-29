//! Test 33-multi-column.html — column-count / column-width / column-gap / column-rule.
//!
//! Seven `.mc` multi-column containers (60px tall, widths 480/690/740/480/680/320/900)
//! each hold N `.col` children that the engine distributes across columns. The
//! load-bearing checks: 22 `.col` boxes total, and in the first container
//! (column-count:2, gap:20, width:480) the two columns are 230px wide and advance
//! by 250px (column width + gap), proving the gap is honoured.

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
fn test_33_multi_column() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/33-multi-column.html");

    // Seven containers, declared widths in document order; all 60px tall.
    let mc = session.all_layout_boxes_by_selector(".mc").expect("query .mc");
    let widths = [480.0, 690.0, 740.0, 480.0, 680.0, 320.0, 900.0];
    assert_eq!(mc.len(), 7, "expected 7 multi-column containers");
    for (i, w) in widths.iter().enumerate() {
        assert!(
            (mc[i].border_box.width - w).abs() < 1.0 && (mc[i].border_box.height - 60.0).abs() < 1.0,
            ".mc[{i}] should be {w}x60, got {}x{}",
            mc[i].border_box.width,
            mc[i].border_box.height
        );
    }

    // Children: 2+3+4+2+4+2+5 = 22 columns total.
    let cols = session.all_layout_boxes_by_selector(".col").expect("query .col");
    assert_eq!(cols.len(), 22, "expected 22 column boxes");

    // First container (column-count:2, gap:20, width:480): each column is
    // (480 − 20) / 2 = 230px wide and the second starts 250px (230 + 20 gap) right.
    assert!(
        (cols[0].border_box.width - 230.0).abs() < 1.0,
        "first column should be 230px wide, got {}",
        cols[0].border_box.width
    );
    let advance = cols[1].border_box.x - cols[0].border_box.x;
    assert!(
        (advance - 250.0).abs() < 1.0,
        "column advance should be 250px (230 width + 20 gap), got {advance}"
    );
}
