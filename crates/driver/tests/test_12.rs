//! Test 12-display.html — display:block / inline-block / none.

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
fn test_12_display() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/12-display.html");

    // display:block — 3 blue divs, each on its own line, declared widths kept.
    let blocks = session
        .all_layout_boxes_by_selector("div[style*='#3182ce']")
        .expect("query blue blocks");
    assert_eq!(blocks.len(), 3, "expected 3 block divs");
    let block_w = [480.0_f32, 320.0, 600.0];
    for (i, b) in blocks.iter().enumerate() {
        assert!(
            (b.border_box.width - block_w[i]).abs() < 1.0,
            "block[{i}] width should be {}, got {}",
            block_w[i],
            b.border_box.width
        );
        assert!(
            (b.border_box.height - 40.0).abs() < 1.0,
            "block[{i}] height should be 40, got {}",
            b.border_box.height
        );
    }
    // Blocks stack vertically (each below the previous).
    assert!(
        blocks[0].border_box.y < blocks[1].border_box.y
            && blocks[1].border_box.y < blocks[2].border_box.y,
        "block divs should stack vertically"
    );

    // display:inline-block — 5 red boxes sit side by side on one line.
    let reds = session
        .all_layout_boxes_by_selector("div[style*='#e53e3e']")
        .expect("query red boxes");
    assert_eq!(reds.len(), 5, "expected 5 inline-block boxes");
    let row_y = reds[0].border_box.y;
    for r in &reds {
        assert!(
            (r.border_box.width - 120.0).abs() < 1.0 && (r.border_box.height - 80.0).abs() < 1.0,
            "red inline-block should be 120x80, got {}x{}",
            r.border_box.width,
            r.border_box.height
        );
        assert!(
            (r.border_box.y - row_y).abs() < 1.0,
            "red inline-blocks should share one row"
        );
    }
    // Horizontally ordered left-to-right.
    assert!(
        reds.windows(2).all(|w| w[0].border_box.x < w[1].border_box.x),
        "red inline-blocks should flow left to right"
    );

    // display:none — 5 green divs declared, the 3rd is display:none, so only 4
    // produce a visible (non-zero) box.
    let greens = session
        .all_layout_boxes_by_selector("div[style*='#38a169']")
        .expect("query green boxes");
    let visible_greens = greens
        .iter()
        .filter(|b| b.border_box.width > 1.0 && b.border_box.height > 1.0)
        .count();
    assert_eq!(
        visible_greens, 4,
        "display:none should remove one of the 5 green boxes (got {visible_greens} visible)"
    );

    // vertical-align:top — 4 purple inline-blocks of varying heights, top-aligned.
    let purples = session
        .all_layout_boxes_by_selector("div[style*='#805ad5']")
        .expect("query purple boxes");
    assert_eq!(purples.len(), 4, "expected 4 purple boxes");
    let purple_h = [40.0_f32, 80.0, 120.0, 60.0];
    let top_y = purples[0].border_box.y;
    for (i, p) in purples.iter().enumerate() {
        assert!(
            (p.border_box.height - purple_h[i]).abs() < 1.0,
            "purple[{i}] height should be {}, got {}",
            purple_h[i],
            p.border_box.height
        );
        assert!(
            (p.border_box.y - top_y).abs() < 1.0,
            "purple[{i}] should be top-aligned (y={top_y}), got {}",
            p.border_box.y
        );
    }
}
