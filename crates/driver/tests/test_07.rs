//! Test 07-box-sizing.html — content-box vs border-box sizing.
//!
//! 4 pairs of `.cb` (content-box) / `.bb` (border-box) divs, all with
//! `border: 8px`. content-box adds padding+border to the declared width;
//! border-box keeps the declared width as the border-box width.

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
fn test_07_box_sizing() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/07-box-sizing.html");

    let cb = session
        .all_layout_boxes_by_selector(".cb")
        .expect("query .cb");
    let bb = session
        .all_layout_boxes_by_selector(".bb")
        .expect("query .bb");

    assert_eq!(cb.len(), 4, "expected 4 content-box divs");
    assert_eq!(bb.len(), 4, "expected 4 border-box divs");

    // content-box border-box width = declared width + 2*padding + 2*border(8).
    // pair1: 200+16+16=232; pair2: 200+40+16=256; pair3: 200+64+16=280; pair4: 320+40+16=376
    let cb_widths = [232.0_f32, 256.0, 280.0, 376.0];
    let cb_heights = [80.0_f32, 104.0, 136.0, 104.0];
    for (i, b) in cb.iter().enumerate() {
        assert!(
            (b.border_box.width - cb_widths[i]).abs() < 1.0,
            "content-box[{i}] width should be {}, got {}",
            cb_widths[i],
            b.border_box.width
        );
        assert!(
            (b.border_box.height - cb_heights[i]).abs() < 1.0,
            "content-box[{i}] height should be {}, got {}",
            cb_heights[i],
            b.border_box.height
        );
    }

    // border-box: declared width IS the border-box width (border lives inside).
    // Heights where declared height < padding+border collapse content to 0, so
    // the border-box height equals padding+border (CSS 2.1 §10.6.x):
    // pair2 48<56→56, pair3 56<80→80, pair4 48<56→56.
    let bb_widths = [200.0_f32, 200.0, 200.0, 320.0];
    let bb_heights = [48.0_f32, 56.0, 80.0, 56.0];
    for (i, b) in bb.iter().enumerate() {
        assert!(
            (b.border_box.width - bb_widths[i]).abs() < 1.0,
            "border-box[{i}] width should be {}, got {}",
            bb_widths[i],
            b.border_box.width
        );
        assert!(
            (b.border_box.height - bb_heights[i]).abs() < 1.0,
            "border-box[{i}] height should be {}, got {}",
            bb_heights[i],
            b.border_box.height
        );
    }
}
