//! Test 29-container-queries.html — @container size/inline-size queries.
//!
//! Four containers with container-type set; each child's height is driven by a
//! @container query against the container's width. The load-bearing check is that
//! the SAME `.c-child` rule resolves to height 60 in the wide (300px) container
//! but stays at the default 20px in the narrow (150px) container, proving the
//! query is actually evaluated against each container's size.

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
fn test_29_container_queries() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/29-container-queries.html");

    // Containers: wide 300, narrow 150, named 200, max-width 120 (+2px border each).
    let expected_w = [(".c-wide", 304.0), (".c-narrow", 154.0), (".c-named", 204.0), (".c-maxw", 124.0)];
    for (sel, w) in expected_w {
        let b = session
            .layout_box_by_selector(sel)
            .expect("query")
            .unwrap_or_else(|| panic!("{sel} not found"));
        assert!(
            (b.border_box.width - w).abs() < 1.0 && (b.border_box.height - 124.0).abs() < 1.0,
            "{sel} should be {w}x124, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // `.c-child` appears in c-wide (query min-width:200 applies → 60px) and
    // c-narrow (query does NOT apply → default 20px).
    let children = session.all_layout_boxes_by_selector(".c-child").expect("query .c-child");
    assert_eq!(children.len(), 2, "expected 2 .c-child boxes");
    assert!(
        (children[0].border_box.height - 60.0).abs() < 1.0,
        "wide container child should be 60px (query applies), got {}",
        children[0].border_box.height
    );
    assert!(
        (children[1].border_box.height - 20.0).abs() < 1.0,
        "narrow container child should stay 20px (query does NOT apply), got {}",
        children[1].border_box.height
    );

    // Named (sidebar min-width:150) and max-width (max-width:200) queries both apply → 60px.
    let named = session.layout_box_by_selector(".c-named-child").unwrap().expect(".c-named-child");
    assert!(
        (named.border_box.height - 60.0).abs() < 1.0,
        ".c-named-child should be 60px (named query applies), got {}",
        named.border_box.height
    );
    let maxw = session.layout_box_by_selector(".c-maxw-child").unwrap().expect(".c-maxw-child");
    assert!(
        (maxw.border_box.height - 60.0).abs() < 1.0,
        ".c-maxw-child should be 60px (max-width query applies), got {}",
        maxw.border_box.height
    );
}
