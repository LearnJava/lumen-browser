//! Test 14-overflow.html — overflow / overflow-x / overflow-y.
//!
//! overflow is a paint-time clip; it must NOT change the layout box sizes.
//! Each demo: a 160x100 `.ct` container holding a larger 220x140 `.ch` child.

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
fn test_14_overflow() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/14-overflow.html");

    let cols = session.all_layout_boxes_by_selector(".col").expect("query .col");
    let containers = session.all_layout_boxes_by_selector(".ct").expect("query .ct");
    let children = session.all_layout_boxes_by_selector(".ch").expect("query .ch");

    assert_eq!(cols.len(), 4, "expected 4 demo columns");
    assert_eq!(containers.len(), 4, "expected 4 overflow containers");
    assert_eq!(children.len(), 4, "expected 4 overflowing children");

    // Containers keep their declared 160x100 regardless of overflow value.
    for c in &containers {
        assert!(
            (c.border_box.width - 160.0).abs() < 1.0 && (c.border_box.height - 100.0).abs() < 1.0,
            "overflow container should stay 160x100, got {}x{}",
            c.border_box.width,
            c.border_box.height
        );
    }
    // The child keeps its intrinsic 220x140 (overflow clips painting, not layout).
    for ch in &children {
        assert!(
            (ch.border_box.width - 220.0).abs() < 1.0 && (ch.border_box.height - 140.0).abs() < 1.0,
            "overflowing child should stay 220x140, got {}x{}",
            ch.border_box.width,
            ch.border_box.height
        );
    }
}
