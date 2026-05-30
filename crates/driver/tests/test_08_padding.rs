//! Test 08-padding.html — padding shorthand variations.
//!
//! 9 `.outer` boxes (auto width, box-sizing: border-box) each wrapping a 100x60
//! `.inner`. With auto width the outer border_box = inner content + padding on
//! each axis. We assert every outer's resulting geometry across 1/2/4-value
//! padding shorthands, the inner size, and the first outer's computed padding.

use lumen_driver::{BrowserSession, InProcessSession};
use lumen_layout::style::Length;

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
fn test_08_padding() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/08-padding.html");

    let inner = session
        .all_layout_boxes_by_selector(".inner")
        .expect("query .inner");
    assert_eq!(inner.len(), 9, "expected 9 inner content boxes");
    for (i, b) in inner.iter().enumerate() {
        assert!(
            (b.border_box.width - 100.0).abs() < 1.0 && (b.border_box.height - 60.0).abs() < 1.0,
            "inner[{i}] should stay 100x60, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    let outer = session
        .all_layout_boxes_by_selector(".outer")
        .expect("query .outer");
    assert_eq!(outer.len(), 9, "expected 9 padded outer boxes");

    // outer border_box = 100 + (pl+pr) wide, 60 + (pt+pb) tall.
    let expected: [(f32, f32); 9] = [
        (108.0, 68.0),  // padding: 4
        (124.0, 84.0),  // 12
        (148.0, 108.0), // 24
        (180.0, 140.0), // 40
        (148.0, 68.0),  // 4 24 (v h): l+r=48, t+b=8
        (108.0, 108.0), // 24 4: l+r=8, t+b=48
        (164.0, 100.0), // 8 16 32 48 (t r b l): l+r=64, t+b=40
        (140.0, 100.0), // 32 8 8 32: l+r=40, t+b=40
        (100.0, 60.0),  // 0
    ];
    for (i, (w, h)) in expected.iter().enumerate() {
        assert!(
            (outer[i].border_box.width - w).abs() < 1.0
                && (outer[i].border_box.height - h).abs() < 1.0,
            "outer[{i}] should be {w}x{h}, got {}x{}",
            outer[i].border_box.width,
            outer[i].border_box.height
        );
    }

    // First outer is `padding: 4px` → all four sides resolve to Px(4).
    let style = session
        .computed_style_snapshot(".outer")
        .expect("style")
        .expect("first outer style");
    for (name, len) in [
        ("top", &style.padding_top),
        ("right", &style.padding_right),
        ("bottom", &style.padding_bottom),
        ("left", &style.padding_left),
    ] {
        match len {
            Length::Px(px) => assert!(
                (px - 4.0).abs() < 0.5,
                "first outer padding-{name} should be 4px, got {px}"
            ),
            other => panic!("first outer padding-{name} should be Px(4), got {other:?}"),
        }
    }
}
