//! Test 37-float-clear.html — float:left / float:right / clear:both flow.
//!
//! Five 940px rows. The floated boxes carry only inline styles (no selectors), so
//! they are located in the flat snapshot by geometry. Load-bearing behaviour:
//! a left float pushes following in-flow content to its right edge, a right float
//! pins to the row's right edge, and `clear:both` drops a bar below the tallest
//! float in the row.

use lumen_driver::{BoxModel, BrowserSession, InProcessSession};

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

/// Find the first snapshot box whose border-box matches (x, y, w, h) within 1.5px.
fn find_box(snap: &[BoxModel], x: f32, y: f32, w: f32, h: f32) -> BoxModel {
    snap.iter()
        .find(|b| {
            (b.border_box.x - x).abs() < 1.5
                && (b.border_box.y - y).abs() < 1.5
                && (b.border_box.width - w).abs() < 1.5
                && (b.border_box.height - h).abs() < 1.5
        })
        .unwrap_or_else(|| panic!("no box at ({x},{y}) {w}x{h}"))
        .clone()
}

#[test]
fn test_37_float_clear() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/37-float-clear.html");

    // Five rows, all 940px wide.
    let rows = session.all_layout_boxes_by_selector(".row").expect("query .row");
    assert_eq!(rows.len(), 5, "expected 5 rows");
    for (i, r) in rows.iter().enumerate() {
        assert!(
            (r.border_box.width - 940.0).abs() < 1.0,
            "row[{i}] should be 940px wide, got {}",
            r.border_box.width
        );
    }

    let snap = session.layout_snapshot().expect("snapshot");

    // Section 1: left float (160x80 at x=25). The in-flow fill starts at x=185 —
    // exactly the float's right edge → float reserved 160px on the left.
    let lfloat = find_box(&snap, 25.0, 25.0, 160.0, 80.0);
    let fill1 = find_box(&snap, 185.0, 25.0, 780.0, 80.0);
    assert!(
        (fill1.border_box.x - (lfloat.border_box.x + lfloat.border_box.width)).abs() < 1.5,
        "left-float fill should start at float's right edge"
    );

    // Section 2: right float pins to the row's right edge (x=805, right=965=25+940).
    let rfloat = find_box(&snap, 805.0, 121.0, 160.0, 80.0);
    assert!(
        (rfloat.border_box.x + rfloat.border_box.width - 965.0).abs() < 1.5,
        "right float should reach the row's right edge (965)"
    );

    // Section 5: clear:both bar (940x20) drops below the tallest float — the 100px
    // left float at y=389 → bottom 489, which is exactly the bar's top.
    let tall_float = find_box(&snap, 25.0, 389.0, 200.0, 100.0);
    let clear_bar = find_box(&snap, 25.0, 489.0, 940.0, 20.0);
    assert!(
        (clear_bar.border_box.y - (tall_float.border_box.y + tall_float.border_box.height)).abs()
            < 1.5,
        "clear:both bar should sit at the tallest float's bottom edge"
    );
}
