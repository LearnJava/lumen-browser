//! Test 10-min-max-width.html — min-width/max-width clamping plus calc/min/clamp.
//!
//! Each row pairs a gray `.ref` bar (the expected resolved width) with a green
//! `.test` bar whose width is constrained. The test bar must resolve to the
//! same width as its ref.

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
fn test_10_min_max_width() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/10-min-max-width.html");

    let refs = session.all_layout_boxes_by_selector(".ref").expect("query .ref");
    let tests = session.all_layout_boxes_by_selector(".test").expect("query .test");

    assert_eq!(refs.len(), 6, "expected 6 ref bars");
    assert_eq!(tests.len(), 6, "expected 6 test bars");

    // Expected resolved widths per row:
    //  0: width:80 min-width:200      -> 200
    //  1: width:500 max-width:300     -> 300
    //  2: width:50 min:150 max:100    -> 150 (min wins over max)
    //  3: width:calc(100px + 100px)   -> 200
    //  4: width:min(300px, 200px)     -> 200
    //  5: width:clamp(100px,250px,200)-> 200
    let expected = [200.0_f32, 300.0, 150.0, 200.0, 200.0, 200.0];
    for (i, w) in expected.iter().enumerate() {
        assert!(
            (refs[i].border_box.width - w).abs() < 1.0,
            "ref[{i}] width should be {w}, got {}",
            refs[i].border_box.width
        );
        assert!(
            (tests[i].border_box.width - w).abs() < 1.0,
            "test[{i}] resolved width should be {w}, got {}",
            tests[i].border_box.width
        );
    }
}
