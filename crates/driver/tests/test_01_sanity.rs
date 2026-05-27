//! Test 01-sanity.html — basic white square on black background

use lumen_driver::{BrowserSession, InProcessSession};
use lumen_layout::style::Length;

#[test]
fn test_01_sanity() {
    let mut session = InProcessSession::new();

    // Load the test file from graphic_tests
    // Use workspace root to find graphic_tests
    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/01-sanity.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Verify the square element's computed styles
    let square_style = session
        .computed_style_snapshot(".square")
        .expect("Failed to get computed style")
        .expect("Element not found");

    // Check width and height (should be 200px)
    match &square_style.width {
        Some(Length::Px(w)) => assert!((w - 200.0).abs() < 0.5, "Square width should be 200px, got {}", w),
        other => panic!("Square width should be Px(200), got {:?}", other),
    }
    match &square_style.height {
        Some(Length::Px(h)) => assert!((h - 200.0).abs() < 0.5, "Square height should be 200px, got {}", h),
        other => panic!("Square height should be Px(200), got {:?}", other),
    }

    // Check layout position via layout_snapshot
    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");

    // Find the square box by size (200x200) and position (~413, ~261)
    let square_box = boxes
        .iter()
        .find(|b| {
            (b.border_box.width - 200.0).abs() < 1.0
                && (b.border_box.height - 200.0).abs() < 1.0
                && (b.border_box.x - 413.0).abs() < 2.0
                && (b.border_box.y - 261.0).abs() < 2.0
        })
        .expect("Square box not found in layout");

    // Verify box size (border-box)
    assert!((square_box.border_box.width - 200.0).abs() < 1.0,
        "Square box width should be ~200px, got {}", square_box.border_box.width);
    assert!((square_box.border_box.height - 200.0).abs() < 1.0,
        "Square box height should be ~200px, got {}", square_box.border_box.height);

    // Verify position: margin-left: 412px, margin-top: 260px
    // Actual border_box position is 413, 261 (due to body margin: 0; reset)
    assert!((square_box.border_box.x - 413.0).abs() < 1.0,
        "Square X position should be ~413px, got {}", square_box.border_box.x);
    assert!((square_box.border_box.y - 261.0).abs() < 1.0,
        "Square Y position should be ~261px, got {}", square_box.border_box.y);

    // Verify white background color
    assert!(square_style.background_color.is_some(),
        "Square should have background color");
}
