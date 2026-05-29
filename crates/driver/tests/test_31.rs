//! Test 31-clip-path.html — clip-path inset/circle/ellipse/polygon (paint-only).
//!
//! clip-path is a paint-time effect and must not change layout. Each row holds
//! several boxes that share a class; every box keeps the declared geometry
//! regardless of which clip shape is applied: 5 `.box` (140x100), 3 `.circle-box`
//! (120x120), 2 `.ellipse-box` (160x100), 2 `.poly-box` (140x140), 2 `.combo-box`
//! (140x100). All boxes use box-sizing:border-box with no border.

use lumen_driver::{BrowserSession, InProcessSession};

fn navigate(session: &mut InProcessSession, file: &str) {
    let root = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(root)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join(file);
    session
        .navigate(&format!("file://{}", path.display()))
        .expect("navigate");
}

#[test]
fn test_31_clip_path() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/31-clip-path.html");

    // Each (selector, count, width, height) group must keep its declared geometry —
    // clip-path never alters the layout box.
    let groups = [
        (".box", 5, 140.0, 100.0),
        (".circle-box", 3, 120.0, 120.0),
        (".ellipse-box", 2, 160.0, 100.0),
        (".poly-box", 2, 140.0, 140.0),
        (".combo-box", 2, 140.0, 100.0),
    ];
    for (sel, count, w, h) in groups {
        let boxes = session
            .all_layout_boxes_by_selector(sel)
            .unwrap_or_else(|_| panic!("query {sel}"));
        assert_eq!(boxes.len(), count, "expected {count} {sel} boxes");
        for (i, b) in boxes.iter().enumerate() {
            assert!(
                (b.border_box.width - w).abs() < 1.0 && (b.border_box.height - h).abs() < 1.0,
                "{sel}[{i}] should be {w}x{h} (clip-path is paint-only), got {}x{}",
                b.border_box.width,
                b.border_box.height
            );
        }
    }
}
