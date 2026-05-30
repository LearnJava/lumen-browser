//! Test 43-intrinsic-sizing.html — width: max-content / min-content / fit-content.
//!
//! Six pairs: a gray `.ref` bar with an explicit width and a green `.test` bar sized
//! by an intrinsic keyword wrapping a fixed-width child. The load-bearing check is
//! that each `.test` resolves to the SAME width as its `.ref` partner — i.e. the
//! intrinsic keyword computed the child's content width (300/250/400/180/600/500).

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
fn test_43_intrinsic_sizing() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/43-intrinsic-sizing.html");

    let refs = session.all_layout_boxes_by_selector(".ref").expect("query .ref");
    let tests = session.all_layout_boxes_by_selector(".test").expect("query .test");
    assert_eq!(refs.len(), 6, "expected 6 reference bars");
    assert_eq!(tests.len(), 6, "expected 6 intrinsic-sized bars");

    // Each intrinsic-sized .test must match its explicit-width .ref partner.
    let expected = [300.0, 250.0, 400.0, 180.0, 600.0, 500.0];
    for (i, &w) in expected.iter().enumerate() {
        assert!(
            (refs[i].border_box.width - w).abs() < 1.0,
            "ref[{i}] should be {w}px (sanity), got {}",
            refs[i].border_box.width
        );
        assert!(
            (tests[i].border_box.width - w).abs() < 1.0,
            "test[{i}] intrinsic width should resolve to {w}px (matching ref), got {}",
            tests[i].border_box.width
        );
    }
}
