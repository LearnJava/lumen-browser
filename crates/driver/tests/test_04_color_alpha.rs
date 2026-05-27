use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_04_color_alpha() {
    let mut session = InProcessSession::new();
    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root).parent().and_then(|p| p.parent())
        .expect("workspace").join("graphic_tests/04-color-alpha.html");
    session.navigate(&format!("file://{}", test_file.display())).expect("navigate");
    let boxes = session.layout_snapshot().expect("layout");
    assert!(!boxes.is_empty());
}
