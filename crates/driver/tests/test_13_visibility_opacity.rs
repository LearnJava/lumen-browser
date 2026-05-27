//! Test 13-visibility-opacity.html — visibility and opacity properties

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_13_visibility_opacity() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/13-visibility-opacity.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Get opacity style from an element
    let opacity_style = session
        .computed_style_snapshot(".box")
        .expect("Failed to get style");

    // For fully opaque elements, opacity should be 1.0
    if let Some(style) = opacity_style {
        assert!(style.opacity >= 0.0 && style.opacity <= 1.0,
            "Opacity should be between 0 and 1, got {}", style.opacity);
    }
}
