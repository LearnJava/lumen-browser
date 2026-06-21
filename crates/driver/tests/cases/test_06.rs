//! Test 06-border-sides.html — per-side borders and colors, currentColor

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_06_border_sides() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/06-border-sides.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Verify layout snapshot is not empty
    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");
    assert!(!boxes.is_empty(), "Layout should contain boxes");

    // Test 1: border-top only (should have top border)
    let boxes1 = session
        .all_layout_boxes_by_selector(".b")
        .expect("Failed to get boxes");
    assert!(!boxes1.is_empty(), "Should find border boxes");

    // Verify first box has reasonable dimensions (160x80 from CSS)
    let first_box = &boxes1[0];
    assert!(
        (first_box.border_box.width - 160.0).abs() < 2.0,
        "First box width should be ~160px, got {}",
        first_box.border_box.width
    );
    assert!(
        (first_box.border_box.height - 80.0).abs() < 2.0,
        "First box height should be ~80px, got {}",
        first_box.border_box.height
    );

    // Test currentColor borders (boxes with color: #ed8936 and #b794f4)
    let style6 = session
        .computed_style_snapshot("div[style*='color: #ed8936']")
        .expect("Failed to get computed style")
        .expect("Element not found");

    // currentColor border should exist (check one of the per-side widths)
    assert!(
        style6.border_top_width > 0.0,
        "currentColor border element should have border-top-width > 0"
    );
}
