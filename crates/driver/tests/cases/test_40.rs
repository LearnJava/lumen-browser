//! Test 40-conic-gradients.html — conic-gradient / repeating-conic-gradient (paint-only).
//!
//! Conic gradients are background paint and must not affect layout. Eight 180x180
//! swatches (default, two-color, from-angle, at-position, deg-stops, pie, and two
//! repeating variants) plus one 360x180 wide box. Also checks the flex row advances
//! by 196px (180 width + 16px gap).

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

fn box_of(session: &mut InProcessSession, sel: &str) -> lumen_driver::BoxModel {
    session
        .layout_box_by_selector(sel)
        .expect("query")
        .unwrap_or_else(|| panic!("{sel} not found"))
}

#[test]
fn test_40_conic_gradients() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/40-conic-gradients.html");

    // Eight square swatches all keep 180x180 regardless of gradient flavour.
    for sel in [
        ".c-default",
        ".c-two-color",
        ".c-from-90",
        ".c-at-corner",
        ".c-deg-stops",
        ".c-pie",
        ".c-repeating-4",
        ".c-repeating-8",
    ] {
        let b = box_of(&mut session, sel);
        assert!(
            (b.border_box.width - 180.0).abs() < 1.0 && (b.border_box.height - 180.0).abs() < 1.0,
            "{sel} should be 180x180 (conic gradient is paint-only), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // The rectangular box is 360x180 — angles are box-relative, geometry unchanged.
    let wide = box_of(&mut session, ".c-wide");
    assert!(
        (wide.border_box.width - 360.0).abs() < 1.0 && (wide.border_box.height - 180.0).abs() < 1.0,
        ".c-wide should be 360x180, got {}x{}",
        wide.border_box.width,
        wide.border_box.height
    );

    // First flex row advances by 196px (180 + 16px gap).
    let first = box_of(&mut session, ".c-default");
    let second = box_of(&mut session, ".c-two-color");
    assert!(
        (second.border_box.x - first.border_box.x - 196.0).abs() < 1.0,
        "flex step should be 196px (180 + 16 gap), got {}",
        second.border_box.x - first.border_box.x
    );
}
