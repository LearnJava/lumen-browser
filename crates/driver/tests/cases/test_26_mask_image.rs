//! Test 26-mask-image.html — CSS mask-image / mask-mode.
//!
//! mask-image is a paint-time effect (Phase 0: rendered as no-op), so it must not
//! change layout. Six 200x200 boxes laid out in two flex rows: linear/radial mask,
//! a control, then alpha/luminance mask-mode and a second control.

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
fn test_26_mask_image() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/26-mask-image.html");

    // All six demo boxes keep their declared 200x200 regardless of mask settings.
    for sel in [
        ".mask-linear",
        ".mask-radial",
        ".no-mask",
        ".mask-mode-alpha",
        ".mask-mode-luma",
        ".no-mask-2",
    ] {
        let b = session
            .layout_box_by_selector(sel)
            .expect("query box")
            .unwrap_or_else(|| panic!("{sel} not found"));
        assert!(
            (b.border_box.width - 200.0).abs() < 1.0 && (b.border_box.height - 200.0).abs() < 1.0,
            "{sel} should be 200x200 (mask is paint-only), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Within a flex row, boxes advance by 230px (200 width + 30px gap).
    let linear = session.layout_box_by_selector(".mask-linear").unwrap().unwrap();
    let radial = session.layout_box_by_selector(".mask-radial").unwrap().unwrap();
    assert!(
        (radial.border_box.x - linear.border_box.x - 230.0).abs() < 1.0,
        "flex gap+width step should be 230px, got {}",
        radial.border_box.x - linear.border_box.x
    );
}
