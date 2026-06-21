//! Test 09-margin.html — margin-left staircase and margin-top gaps.

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
fn test_09_margin() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/09-margin.html");

    // margin-left staircase: 0/24/48/96/192px on 5 fixed-width (400px) blocks.
    let steps = session
        .all_layout_boxes_by_selector(".step")
        .expect("query .step");
    assert_eq!(steps.len(), 5, "expected 5 staircase blocks");
    for s in &steps {
        assert!(
            (s.border_box.width - 400.0).abs() < 1.0,
            "step width should be 400, got {}",
            s.border_box.width
        );
    }
    // The X delta between consecutive steps equals the margin-left increment.
    let dx: Vec<f32> = steps
        .windows(2)
        .map(|w| w[1].border_box.x - w[0].border_box.x)
        .collect();
    let expected_dx = [24.0_f32, 24.0, 48.0, 96.0];
    for (i, d) in dx.iter().enumerate() {
        assert!(
            (d - expected_dx[i]).abs() < 1.0,
            "margin-left delta[{i}] should be {}, got {}",
            expected_dx[i],
            d
        );
    }

    // margin-top gaps: 6 gap-blocks (320px wide) stacked with increasing top gaps.
    let blocks = session
        .all_layout_boxes_by_selector(".gap-block")
        .expect("query .gap-block");
    assert_eq!(blocks.len(), 6, "expected 6 gap blocks");
    for b in &blocks {
        assert!(
            (b.border_box.width - 320.0).abs() < 1.0,
            "gap-block width should be 320, got {}",
            b.border_box.width
        );
    }
    // Vertical gaps between consecutive blocks: 0/4/8/16/32/64 (top of next minus bottom of prev).
    let gaps: Vec<f32> = blocks
        .windows(2)
        .map(|w| w[1].border_box.y - (w[0].border_box.y + w[0].border_box.height))
        .collect();
    let expected_gaps = [4.0_f32, 8.0, 16.0, 32.0, 64.0];
    for (i, g) in gaps.iter().enumerate() {
        assert!(
            (g - expected_gaps[i]).abs() < 1.0,
            "margin-top gap[{i}] should be {}, got {}",
            expected_gaps[i],
            g
        );
    }
}
