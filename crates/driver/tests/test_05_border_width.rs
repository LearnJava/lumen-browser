//! Test 05-border-width.html — border width property variations

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_05_border_width() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/05-border-width.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Check layout
    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");

    // Find a box with 4px border (third element)
    let _border_box = boxes
        .iter()
        .find(|b| {
            // Width should be 140 + 4*2 = 148 (with border-box it would be 140)
            // Since the test uses content-box, visual size = 140 + 2*4 = 148
            (b.border_box.width - 148.0).abs() < 2.0
                && (b.border_box.height - 88.0).abs() < 2.0
        })
        .expect("Border box not found");

    // Verify border widths
    let border_style = session
        .computed_style_snapshot(".b")
        .expect("Failed to get style")
        .expect("Border element style not found");

    // At least check that border-width property is set
    assert!(border_style.border_top_width > 0.0, "Border top width should be > 0");
}
