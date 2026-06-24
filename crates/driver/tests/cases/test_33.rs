//! Test 33-multi-column.html — column-count / column-width / column-gap / column-rule /
//! column-span:all / column-fill:auto.
//!
//! Seven `.mc` multi-column containers. After commit cefb8475 (C8/P4) the HTML was extended
//! with column-span:all (container 5) and column-fill:auto (container 6), which changed
//! widths and heights. Ground-truth sizes verified via --dump-layout:
//!
//!   mc[0]: 480×52   column-count:2, gap:20  — 2 .col children
//!   mc[1]: 690×52   column-count:3, gap:30  — 3 .col children
//!   mc[2]: 740×52   column-count:4, gap:20  — 4 .col children
//!   mc[3]: 480×52   column-count:2, gap:60  — 2 .col children
//!   mc[4]: 660×64   column-count:3, gap:12, column-span:all — auto height from content.
//!     Two 36px col-sm blocks balance-fragment across 3 columns to 24px each (72/3),
//!     then the 8px span (+4px margins = 16px), then two more col-sm fragmenting to
//!     24px: 24+16+24 = 64. Verified against Edge (getBoundingClientRect → 64) and
//!     TEST-33 pixel parity (≈0.1%). The earlier 88px figure predated BUG-186 column
//!     fragmentation (atomic one-box-per-column placement, which Edge does not do).
//!   mc[5]: 660×80   column-count:3, gap:12, column-fill:auto — explicit height
//!   mc[6]: 900×52   column-count:5, gap:16  — 5 .col children
//!
//! Load-bearing checks: 16 `.col` boxes total; first container columns are 230px wide
//! with 250px advance (230 width + 20 gap).

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

    let mc = session.all_layout_boxes_by_selector(".mc").expect("query .mc");
    assert_eq!(mc.len(), 7, "expected 7 multi-column containers");

    // Width and height per container (ground-truth from --dump-layout).
    let expected: [(f32, f32); 7] = [
        (480.0, 52.0),
        (690.0, 52.0),
        (740.0, 52.0),
        (480.0, 52.0),
        (660.0, 64.0), // column-span:all, auto height (Edge-verified; BUG-186 fragmentation)
        (660.0, 80.0), // column-fill:auto, explicit height
        (900.0, 52.0),
    ];
    for (i, (w, h)) in expected.iter().enumerate() {
        assert!(
            (mc[i].border_box.width - w).abs() < 1.0 && (mc[i].border_box.height - h).abs() < 1.0,
            ".mc[{i}] should be {w}x{h}, got {}x{}",
            mc[i].border_box.width,
            mc[i].border_box.height
        );
    }

    // Containers 5 and 6 use .col-sm; only containers 1-4 and 7 use .col.
    // 2+3+4+2+5 = 16 total.
    let cols = session.all_layout_boxes_by_selector(".col").expect("query .col");
    assert_eq!(cols.len(), 16, "expected 16 .col boxes");

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
