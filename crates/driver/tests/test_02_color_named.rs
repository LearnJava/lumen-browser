//! Test 02-color-named.html — CSS named colors

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_02_color_named() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/02-color-named.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");
    assert!(!boxes.is_empty(), "Layout should contain at least one box");
}
