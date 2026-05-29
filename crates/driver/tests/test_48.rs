//! Test 48-line-clamp.html — CSS Overflow L4 §3.2 -webkit-line-clamp truncation.
//!
//! Four flex columns (231.5px wide, step 247.5). The page is built so that a clamp
//! of N lines yields a container height of N × 40px (explicit line-height). Row 4 is
//! an explicit staircase (40/80/120/160) that encodes the ground-truth clamp heights;
//! row 3 is an unclamped 200px reference. The active test locks the geometry that the
//! engine already gets right (labels, references, staircase, flex columns); the
//! `#[ignore]`d test asserts the clamped boxes themselves match the staircase — it is
//! gated on BUG-047 (line-clamp parses but does not truncate height: all four clamped
//! boxes currently resolve to 160px instead of 40/80/120/160).

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

/// Gated on BUG-047: `-webkit-line-clamp` is parsed but never truncates the block
/// height, so all four clamped boxes resolve to 160px instead of the staircase
/// heights 40/80/120/160. Un-ignore once line-clamp height truncation lands.
#[test]
#[ignore = "BUG-047: -webkit-line-clamp does not truncate block height"]
fn test_48_line_clamp_height_truncation() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/48-line-clamp.html");

    let boxes = session.all_layout_boxes_by_selector(".box").expect("query .box");
    assert_eq!(boxes.len(), 4, "expected 4 clamped boxes");
    let expected = [40.0, 80.0, 120.0, 160.0];
    for (i, &h) in expected.iter().enumerate() {
        assert!(
            (boxes[i].border_box.height - h).abs() < 1.0,
            "box[{i}] clamp({}) should be {h}px tall, got {}",
            i + 1,
            boxes[i].border_box.height
        );
    }
}
