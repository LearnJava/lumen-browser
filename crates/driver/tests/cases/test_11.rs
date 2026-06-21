//! Test 11-min-max-height.html — min-height/max-height clamping.
//!
//! Each pair places a gray `.ref` (expected resolved height) next to a green
//! `.test` whose height is constrained. The test must resolve to the ref height.

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
fn test_11_min_max_height() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/11-min-max-height.html");

    let refs = session.all_layout_boxes_by_selector(".ref").expect("query .ref");
    let tests = session.all_layout_boxes_by_selector(".test").expect("query .test");

    assert_eq!(refs.len(), 6, "expected 6 ref columns");
    assert_eq!(tests.len(), 6, "expected 6 test columns");

    // Expected resolved heights per pair:
    //  0: height:40 min-height:100        -> 100
    //  1: height:160 max-height:80        -> 80
    //  2: height:40 min:120 max:60        -> 120 (min wins)
    //  3: height:80 (baseline)            -> 80
    //  4: height:60 min-height:60         -> 60
    //  5: height:200 max-height:140       -> 140
    let expected = [100.0_f32, 80.0, 120.0, 80.0, 60.0, 140.0];
    for (i, h) in expected.iter().enumerate() {
        assert!(
            (refs[i].border_box.height - h).abs() < 1.0,
            "ref[{i}] height should be {h}, got {}",
            refs[i].border_box.height
        );
        assert!(
            (tests[i].border_box.height - h).abs() < 1.0,
            "test[{i}] resolved height should be {h}, got {}",
            tests[i].border_box.height
        );
        // Both columns are 60px wide inline-blocks.
        assert!(
            (tests[i].border_box.width - 60.0).abs() < 1.0,
            "test[{i}] width should be 60, got {}",
            tests[i].border_box.width
        );
    }
}
