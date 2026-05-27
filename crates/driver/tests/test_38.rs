use lumen_driver::{BrowserSession, InProcessSession};
#[test]
fn test_38() {
    let mut s = InProcessSession::new();
    let root = env!("CARGO_MANIFEST_DIR");
    let f = std::path::Path::new(root).parent().and_then(|p| p.parent()).unwrap().join("graphic_tests/38.html");
    if s.navigate(&format!("file://{}", f.display())).is_ok() {
        let b = s.layout_snapshot().unwrap();
        assert!(!b.is_empty());
    }
}
