//! Test 28-css-containment.html — contain: size / paint / layout / strict.
//!
//! The load-bearing behavior is `contain: size`: a sized-contained box ignores
//! its children's contribution to height, so with no explicit height it collapses
//! to just its borders (4px). `contain: strict` includes size and does the same.
//! paint/layout containment do not change box dimensions. All boxes are 200px wide
//! content + 2px border → 204px border-box.

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

fn box_of<'a>(session: &mut InProcessSession, sel: &str) -> lumen_driver::BoxModel {
    session
        .layout_box_by_selector(sel)
        .expect("query")
        .unwrap_or_else(|| panic!("{sel} not found"))
}

#[test]
fn test_28_css_containment() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/28-css-containment.html");

    // Baseline (no containment): auto height wraps the 80px child + 4px border = 84px.
    let none = box_of(&mut session, ".c-none");
    assert!(
        (none.border_box.width - 204.0).abs() < 1.0 && (none.border_box.height - 84.0).abs() < 1.0,
        ".c-none should be 204x84 (auto height wraps child), got {}x{}",
        none.border_box.width,
        none.border_box.height
    );

    // contain:size → child does NOT contribute to height → only the 4px border remains.
    let size = box_of(&mut session, ".c-size");
    assert!(
        (size.border_box.width - 204.0).abs() < 1.0 && (size.border_box.height - 4.0).abs() < 1.0,
        ".c-size should collapse to 204x4 (size containment), got {}x{}",
        size.border_box.width,
        size.border_box.height
    );

    // contain:strict includes size → same collapse to border-only height.
    let strict = box_of(&mut session, ".c-strict");
    assert!(
        (strict.border_box.width - 204.0).abs() < 1.0
            && (strict.border_box.height - 4.0).abs() < 1.0,
        ".c-strict should collapse to 204x4 (strict ⊇ size), got {}x{}",
        strict.border_box.width,
        strict.border_box.height
    );

    // paint/layout containment keep the explicit 200x100 → 204x104 border-box.
    for sel in [".c-paint", ".c-layout"] {
        let b = box_of(&mut session, sel);
        assert!(
            (b.border_box.width - 204.0).abs() < 1.0 && (b.border_box.height - 104.0).abs() < 1.0,
            "{sel} should stay 204x104, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
}
