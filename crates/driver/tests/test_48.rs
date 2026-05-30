//! Test 48-line-clamp.html — CSS Overflow L4 §3.2 -webkit-line-clamp truncation.
//!
//! Four clamp columns (231.5px wide, step 247.5) inside a `display:flex` row. Each
//! column is a `display:-webkit-box` with `-webkit-line-clamp: N`, so its inner
//! anonymous inline box is truncated to N lines of 40px (explicit line-height) =
//! N × 40px. Because the row uses the default `align-items: stretch`, all four
//! columns stretch to the flex line's cross size — the tallest, `.b4` at 4 × 40 =
//! 160px — so every `.box` border-box is 160px tall, matching Edge
//! (graphic_tests/screenshots/48-edge.png), *not* a 40/80/120/160 staircase. Row 3
//! is an unclamped 200px reference; row 4 is an explicit staircase (40/80/120/160)
//! that visually encodes the per-column clamp line counts. `test_48_line_clamp`
//! locks the geometry the engine already gets right (labels, references, staircase,
//! flex columns); `test_48_line_clamp_flex_items_stretch_equal` locks the
//! Edge-verified fact that the clamp columns all stretch to 160px. The inner
//! per-column line truncation itself (the actual line-clamp effect) is
//! regression-tested directly in `lumen-layout` (`line_clamp_truncates_to_n_lines`
//! et al.). Former BUG-047 mis-read this stretch as "line-clamp does not truncate
//! height"; the truncation lives on the inline box, not on the stretched flex item.

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
fn test_48_line_clamp() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/48-line-clamp.html");

    // Four flex columns share width 231.5 and advance by 247.5px (231.5 + 16 gap).
    let labels = session.all_layout_boxes_by_selector(".lbl").expect("query .lbl");
    assert_eq!(labels.len(), 4, "expected 4 label placeholders");
    let xs = [25.0, 272.5, 520.0, 767.5];
    for (i, lb) in labels.iter().enumerate() {
        assert!(
            (lb.border_box.width - 231.5).abs() < 1.0 && (lb.border_box.height - 20.0).abs() < 1.0,
            "lbl[{i}] should be 231.5x20, got {}x{}",
            lb.border_box.width,
            lb.border_box.height
        );
        assert!(
            (lb.border_box.x - xs[i]).abs() < 1.0,
            "lbl[{i}] x should be {}, got {}",
            xs[i],
            lb.border_box.x
        );
    }

    // Unclamped reference boxes: explicit 200px height (= 5 × 40px line-box).
    let refs = session.all_layout_boxes_by_selector(".ref").expect("query .ref");
    assert_eq!(refs.len(), 4, "expected 4 unclamped reference boxes");
    for (i, r) in refs.iter().enumerate() {
        assert!(
            (r.border_box.width - 231.5).abs() < 1.0 && (r.border_box.height - 200.0).abs() < 1.0,
            "ref[{i}] should be 231.5x200, got {}x{}",
            r.border_box.width,
            r.border_box.height
        );
    }

    // Staircase reference (row 4): explicit heights encode the clamp targets 1/2/3/4 lines.
    let stairs = session.all_layout_boxes_by_selector(".stair").expect("query .stair");
    assert_eq!(stairs.len(), 4, "expected 4 staircase boxes");
    let stair_h = [40.0, 80.0, 120.0, 160.0];
    for (i, &h) in stair_h.iter().enumerate() {
        assert!(
            (stairs[i].border_box.height - h).abs() < 1.0,
            "stair[{i}] should be {h}px tall, got {}",
            stairs[i].border_box.height
        );
    }
}

/// Edge ground truth (graphic_tests/screenshots/48-edge.png): the four clamp
/// columns are flex items in a `display:flex` row with default
/// `align-items: stretch`, so each `.box` stretches to the flex line's cross size —
/// the tallest clamped column, `.b4` at 4 × 40 = 160px. All four `.box` border-boxes
/// are therefore 160px tall in Edge, *not* a 40/80/120/160 staircase. The visible
/// line-clamp truncation happens on the inner anonymous inline box (its height
/// resolves to 40/80/120/160px), which is regression-tested directly in
/// `lumen-layout`. This test locks the flex-stretch height that closed BUG-047.
#[test]
fn test_48_line_clamp_flex_items_stretch_equal() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/48-line-clamp.html");

    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 4, "expected 4 clamped boxes");
    for (i, b) in boxes.iter().enumerate() {
        assert!(
            (b.border_box.height - 160.0).abs() < 1.0,
            "box[{i}] should stretch to the flex line cross size (160px), got {}",
            b.border_box.height
        );
    }
}
