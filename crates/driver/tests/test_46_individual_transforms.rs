//! Test 46-individual-transforms.html — CSS translate/rotate/scale individual properties

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_46_individual_transforms() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/46-individual-transforms.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");
    assert!(!boxes.is_empty(), "Layout should contain at least one box");
}
