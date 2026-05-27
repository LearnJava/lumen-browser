//! Test 49-background-blend-mode.html — CSS background-blend-mode blending modes

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_49_background_blend_mode() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/49-background-blend-mode.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Verify layout snapshot
    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");
    assert!(!boxes.is_empty(), "Layout should contain at least one box");

    // Expect body element with magenta background
    let body = boxes
        .iter()
        .find(|b| b.tag_name == "body")
        .expect("Body not found");
    assert_eq!(body.border_box.width, 1024.0, "Body width should be 1024px");
    assert_eq!(body.border_box.height, 720.0, "Body height should be 720px");

    // Verify there are multiple div elements (blend mode boxes and rows)
    let divs: Vec<_> = boxes.iter().filter(|b| b.tag_name == "div").collect();
    assert!(divs.len() >= 10, "Should have at least 10 div elements for boxes and rows");
}
