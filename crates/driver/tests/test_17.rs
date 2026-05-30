//! Test 17-calc.html — calc() and math functions (min/max/clamp/sqrt/cos/abs/hypot).
//!
//! 14 green `.test` bars, each width given by a different expression that must
//! resolve to a known px value.

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
fn test_17_calc() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/17-calc.html");

    let tests = session.all_layout_boxes_by_selector(".test").expect("query .test");
    assert_eq!(tests.len(), 14, "expected 14 calc test bars");

    // Document-order expected resolved widths:
    //  g1 (200): calc(100+100), calc(600/3), calc(50*4), calc(250-50)
    //  g2 (150): min(200,150), max(80,150), clamp(100,150,300), clamp(150,80,300)
    //  g3 (200): sqrt(40000)*1px, cos(0)*200px, abs(-200px), hypot(120,160)
    //  g4 (150): min(calc(50+100),calc(40*5)), clamp(100,calc(30*5),200)
    let expected = [
        200.0_f32, 200.0, 200.0, 200.0, // group 1
        150.0, 150.0, 150.0, 150.0, // group 2
        200.0, 200.0, 200.0, 200.0, // group 3 (math functions)
        150.0, 150.0, // group 4 (nested)
    ];
    for (i, w) in expected.iter().enumerate() {
        assert!(
            (tests[i].border_box.width - w).abs() < 1.0,
            "test[{i}] should resolve to {w}px, got {}",
            tests[i].border_box.width
        );
    }
}
