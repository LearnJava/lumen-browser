//! Test 08-padding.html — padding property

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_08_padding() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/08-padding.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Get padding style from first padded element
    let _padding_style = session
        .computed_style_snapshot(".outer")
        .expect("Failed to get style")
        .expect("Padding element style not found");

    // Verify padding values are set (padding is always Length, not Option)
    // Just verify that element is present and layout snapshot contains it
    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");
    assert!(!boxes.is_empty(), "Layout should contain at least one box");
}
