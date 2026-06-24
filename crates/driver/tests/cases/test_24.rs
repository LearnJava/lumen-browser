//! Test 24-vertical-align.html — vertical-align top/middle/bottom on inline-block.
//!
//! Row 1 holds three 80px-wide inline-blocks of differing heights (100/60/40)
//! aligned top / middle / bottom within a 120px row (100px line area starting at
//! y=21). Assertions check the alignment geometry: top box sits at the line top,
//! bottom box's bottom edge equals the top box's bottom edge, and the middle box
//! aligns its centre to `baseline − x-height/2` (CSS 2.1 §10.8.1) — NOT the line
//! centre. The tall top-aligned box pulls the baseline up to ~34px from the line
//! top, so the 60px middle box's top sits at the line top (BUG-182).

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
fn test_24_vertical_align() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/24-vertical-align.html");

    let row1 = session
        .layout_box_by_selector(".row1")
        .expect("query .row1")
        .expect(".row1 not found");
    assert!(
        (row1.border_box.width - 400.0).abs() < 1.0 && (row1.border_box.height - 120.0).abs() < 1.0,
        "row1 should be 400x120, got {}x{}",
        row1.border_box.width,
        row1.border_box.height
    );

    let ibs = session.all_layout_boxes_by_selector(".ib").expect("query .ib");
    assert_eq!(ibs.len(), 3, "expected 3 inline-block items");

    // Document order: top (h=100), middle (h=60), bottom (h=40); all 80px wide.
    let expected_h = [100.0_f32, 60.0, 40.0];
    for (i, b) in ibs.iter().enumerate() {
        assert!(
            (b.border_box.width - 80.0).abs() < 1.0
                && (b.border_box.height - expected_h[i]).abs() < 1.0,
            "ib[{i}] should be 80x{}, got {}x{}",
            expected_h[i],
            b.border_box.width,
            b.border_box.height
        );
    }

    let (top, middle, bottom) = (&ibs[0], &ibs[1], &ibs[2]);

    // The tallest (top-aligned) box defines the line top; the line spans 100px.
    let line_top = top.border_box.y;
    let line_bottom = line_top + 100.0;

    // vertical-align:bottom → the bottom box's bottom edge meets the line bottom,
    // i.e. equals the top box's bottom edge.
    let top_bottom = top.border_box.y + top.border_box.height;
    let bottom_bottom = bottom.border_box.y + bottom.border_box.height;
    assert!(
        (top_bottom - bottom_bottom).abs() < 1.5,
        "vertical-align:bottom should align bottom edges: top={top_bottom}, bottom={bottom_bottom}"
    );

    // vertical-align:middle → box centre aligns to (baseline − x-height/2), NOT the
    // line centre (BUG-182). The 100px top-aligned box raises the baseline to ~34px
    // from the line top, so the 60px middle box (centre 30px below the baseline minus
    // x/2) lands with its top at the line top — matching Edge, which renders the teal
    // box flush with the red box top. This is NOT the line centre (line_top + 20).
    let _ = line_bottom;
    assert!(
        (middle.border_box.y - line_top).abs() < 2.0,
        "vertical-align:middle box top should sit at the line top (baseline-correct), \
         got middle.y={}, line_top={line_top}",
        middle.border_box.y
    );

    let row2 = session
        .layout_box_by_selector(".row2")
        .expect("query .row2")
        .expect(".row2 not found");
    assert!(
        (row2.border_box.width - 600.0).abs() < 1.0 && (row2.border_box.height - 80.0).abs() < 1.0,
        "row2 should be 600x80, got {}x{}",
        row2.border_box.width,
        row2.border_box.height
    );
}
