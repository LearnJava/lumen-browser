//! Test 47-svg-basic.html — inline SVG basic shapes + viewBox mapping.
//!
//! Seven inline-block `<svg>` roots lay out by their CSS width/height (the viewBox
//! only rescales the internal coordinate system, never the outer box). The shapes
//! (rect/circle/ellipse/line) become layout boxes whose geometry is the shape's
//! bounding box mapped through the viewBox transform. Load-bearing checks:
//! the 7 roots keep their declared sizes, and two viewBox cases resolve correctly —
//! svg #3 scales 2× (viewBox 0 0 100 100 in a 200x200 root) and svg #5 applies a
//! 50,50 origin offset (viewBox 50 50 200 150 at 1× scale).

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
fn test_47_svg_basic() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/47-svg-basic.html");

    // Seven inline-block <svg> roots — outer geometry from CSS, not viewBox.
    let roots = session.all_layout_boxes_by_selector("svg").expect("query svg");
    assert_eq!(roots.len(), 7, "expected 7 inline svg roots");
    let expected_roots = [
        (21.0, 21.0, 320.0, 120.0),
        (365.0, 21.0, 320.0, 120.0),
        (21.0, 154.86, 200.0, 200.0),
        (245.0, 154.86, 300.0, 200.0),
        (21.0, 368.72, 200.0, 150.0),
        (245.0, 368.72, 400.0, 150.0),
        (21.0, 532.58, 980.0, 140.0),
    ];
    for (i, (x, y, w, h)) in expected_roots.iter().enumerate() {
        let b = &roots[i];
        assert!(
            (b.border_box.x - x).abs() < 1.5
                && (b.border_box.y - y).abs() < 1.5
                && (b.border_box.width - w).abs() < 1.0
                && (b.border_box.height - h).abs() < 1.0,
            "svg[{i}] should be {w}x{h} at ({x},{y}), got {}x{} at ({},{})",
            b.border_box.width,
            b.border_box.height,
            b.border_box.x,
            b.border_box.y
        );
    }

    // Shapes become layout boxes; counts match the markup (15 rect, 8 circle, 5 ellipse, 1 line).
    let rects = session.all_layout_boxes_by_selector("rect").expect("query rect");
    let circles = session.all_layout_boxes_by_selector("circle").expect("query circle");
    let ellipses = session.all_layout_boxes_by_selector("ellipse").expect("query ellipse");
    let lines = session.all_layout_boxes_by_selector("line").expect("query line");
    assert_eq!(rects.len(), 15, "expected 15 <rect> shapes");
    assert_eq!(circles.len(), 8, "expected 8 <circle> shapes");
    assert_eq!(ellipses.len(), 5, "expected 5 <ellipse> shapes");
    assert_eq!(lines.len(), 1, "expected 1 <line> shape");

    // viewBox 2× scale (svg #3, root at 21,154.86): rect(10,10,30,30) → 60x60 at (41,174.86).
    find_box(&rects, 41.0, 174.86, 60.0, 60.0);

    // viewBox 50,50 origin offset (svg #5, root at 21,368.72, 1× scale):
    // rect(60,60,80,60) maps to (31,378.72) 80x60 — the 50px origin shifts it by 10px.
    find_box(&rects, 31.0, 378.72, 80.0, 60.0);
}
