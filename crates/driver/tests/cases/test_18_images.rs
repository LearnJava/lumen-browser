//! Test 18-images.html — replaced-element sizing for `<img>` / `<picture>`.
//!
//! 16 `<img>` boxes across 4 rows: explicit width/height attributes, CSS
//! width/height, `<picture>` source selection, and `srcset` candidate picking.
//! Each `<img>` is a replaced element sized by its attributes/CSS regardless of
//! the decoded bitmap, so the layout geometry is fully deterministic.

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
fn test_18_images() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/18-images.html");

    let imgs = session.all_layout_boxes_by_selector("img").expect("query img");
    assert_eq!(imgs.len(), 16, "expected 16 <img> boxes (4 rows)");

    // Document-order expected sizes (attribute or CSS width/height).
    let expected: [(f32, f32); 16] = [
        (200.0, 150.0), // row1: agi
        (200.0, 150.0), // row1: perceptron
        (200.0, 150.0), // row1: sad_brain
        (200.0, 150.0), // row1: jpeg
        (80.0, 60.0),   // row2: CSS 80x60
        (160.0, 120.0), // row2: CSS 160x120
        (300.0, 225.0), // row2: CSS 300x225
        (160.0, 120.0), // row3: white bg
        (160.0, 120.0), // row3: red bg
        (160.0, 120.0), // row3: green bg
        (160.0, 120.0), // row3: blue bg
        (200.0, 150.0), // row4 A: picture media → img 200x150
        (200.0, 150.0), // row4 B: picture type fallback → img 200x150
        (200.0, 150.0), // row4 C: srcset width descriptor
        (160.0, 120.0), // row4 D: srcset density descriptor
        (200.0, 150.0), // row4 E: webp
    ];
    for (i, (w, h)) in expected.iter().enumerate() {
        assert!(
            (imgs[i].border_box.width - w).abs() < 1.0
                && (imgs[i].border_box.height - h).abs() < 1.0,
            "img[{i}] should be {w}x{h}, got {}x{}",
            imgs[i].border_box.width,
            imgs[i].border_box.height
        );
    }
}
