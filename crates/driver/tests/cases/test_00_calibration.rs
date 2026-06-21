//! Test 00-calibration.html — magenta markers for crop offset detection

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_00_calibration() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/00-calibration.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Check layout
    let boxes = session.layout_snapshot().expect("Failed to get layout snapshot");

    // Expect body with magenta background at 1024×720
    let body = boxes
        .iter()
        .find(|b| b.tag_name == "body")
        .expect("Body not found");

    assert_eq!(body.border_box.width, 1024.0, "Calibration body width should be 1024px");
    assert_eq!(body.border_box.height, 720.0, "Calibration body height should be 720px");

    // Verify magenta color (#ff00ff)
    let body_style = session
        .computed_style_snapshot("body")
        .expect("Failed to get style")
        .expect("Body style not found");

    if let Some(color) = &body_style.background_color {
        use lumen_layout::style::CssColor;
        if let CssColor::Rgba(c) = color {
            assert_eq!(c.r, 255, "Magenta red channel should be 255");
            assert_eq!(c.g, 0, "Magenta green channel should be 0");
            assert_eq!(c.b, 255, "Magenta blue channel should be 255");
        }
    }
}
