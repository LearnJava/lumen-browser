//! Test 05-border-width.html — border-width variations.
//!
//! 10 `.b` boxes, content-box, content size 140x80, `border-style: solid`.
//! Because box-sizing is content-box, the border adds to the visual size, so
//! border_box = content + (left+right) / (top+bottom) border widths. We assert
//! the resulting geometry for the uniform and asymmetric border-width cases,
//! plus the first box's computed per-side border widths (all 1px).

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
fn test_05_border_width() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/05-border-width.html");

    let b = session.all_layout_boxes_by_selector(".b").expect("query .b");
    assert_eq!(b.len(), 10, "expected 10 bordered boxes");

    // border_box = 140 + (l+r) wide, 80 + (t+b) tall (content-box).
    let expected: [(f32, f32); 10] = [
        (142.0, 82.0),  // border 1px uniform
        (144.0, 84.0),  // 2px uniform
        (148.0, 88.0),  // 4px uniform
        (156.0, 96.0),  // 8px uniform
        (172.0, 112.0), // 16px uniform
        (144.0, 96.0),  // 8 2 8 2 (t r b l): l+r=4, t+b=16
        (172.0, 84.0),  // 2 16 2 16: l+r=32, t+b=4
        (160.0, 89.0),  // 1 4 8 16: l+r=20, t+b=9
        (142.0, 97.0),  // 16 1 1 1: l+r=2, t+b=17
        (157.0, 82.0),  // 1 1 1 16: l+r=17, t+b=2
    ];
    for (i, (w, h)) in expected.iter().enumerate() {
        assert!(
            (b[i].border_box.width - w).abs() < 1.0 && (b[i].border_box.height - h).abs() < 1.0,
            "box[{i}] should be {w}x{h}, got {}x{}",
            b[i].border_box.width,
            b[i].border_box.height
        );
    }

    // First box: border-width: 1px → all four sides resolve to 1px.
    let style = session
        .computed_style_snapshot(".b")
        .expect("style")
        .expect("first box style");
    for (name, w) in [
        ("top", style.border_top_width),
        ("right", style.border_right_width),
        ("bottom", style.border_bottom_width),
        ("left", style.border_left_width),
    ] {
        assert!(
            (w - 1.0).abs() < 0.5,
            "first box border-{name}-width should be 1px, got {w}"
        );
    }
}
