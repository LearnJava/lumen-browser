//! Test 04-color-alpha.html — alpha across rgba()/hsla()/#RRGGBBAA notations.
//!
//! Eighteen 140x80 `.sw` swatches lay out as inline-blocks, six per row over three
//! rows (x-step 160, y-step 100). Alpha never affects layout, so geometry is uniform;
//! the load-bearing colour check uses computed background-color on the first swatch,
//! which carries red at 10% alpha → rgba(229,62,62,26) (alpha stored as 0–255).

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
fn test_04_color_alpha() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/04-color-alpha.html");

    let sw = session.all_layout_boxes_by_selector(".sw").expect("query .sw");
    assert_eq!(sw.len(), 18, "expected 18 alpha swatches");

    // Every swatch is 140x80 regardless of alpha (alpha is paint-only).
    for (i, b) in sw.iter().enumerate() {
        assert!(
            (b.border_box.width - 140.0).abs() < 1.0 && (b.border_box.height - 80.0).abs() < 1.0,
            "sw[{i}] should be 140x80, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Six swatches per row across three rows: x advances 160px, y advances 100px.
    for row in 0..3 {
        for col in 0..6 {
            let b = &sw[row * 6 + col];
            let expect_x = 25.0 + col as f32 * 160.0;
            let expect_y = 25.0 + row as f32 * 100.0;
            assert!(
                (b.border_box.x - expect_x).abs() < 1.0 && (b.border_box.y - expect_y).abs() < 1.0,
                "sw[{},{}] should be at ({expect_x},{expect_y}), got ({},{})",
                row,
                col,
                b.border_box.x,
                b.border_box.y
            );
        }
    }

    // First swatch: rgba(229, 62, 62, 0.10) → alpha 0.10 × 255 ≈ 26 (0–255 storage).
    let bg = session
        .computed_style(".sw")
        .expect("computed_style")
        .expect(".sw not found")
        .properties
        .get("background-color")
        .cloned()
        .unwrap_or_default();
    assert_eq!(bg, "rgba(229,62,62,26)", "first swatch is red at 10% alpha");
}
