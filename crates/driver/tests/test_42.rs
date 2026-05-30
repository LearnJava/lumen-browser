//! Test 42-position-sticky.html — position:sticky in the unscrolled state.
//!
//! With no scroll, sticky elements stay at their NATURAL flow position (not clamped
//! to their inset) and following in-flow blocks are not shifted. Load-bearing checks:
//! `.sticky-bar` sits at flow y=21 (not pinned to top:10 → 11), `.sticky-side` sits
//! at flow x=1 (not pinned to left:10 → 11), and `.block-a` directly follows the
//! sticky bar (bar.bottom + 20px margin), proving sticky does not remove from flow.

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
fn test_42_position_sticky() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/42-position-sticky.html");

    // (selector, x, y, w, h) — natural flow positions, sticky NOT pinned (unscrolled).
    let expected = [
        (".sticky-bar", 212.0, 21.0, 600.0, 60.0),
        (".block-a", 312.0, 101.0, 400.0, 80.0),
        (".sticky-side", 1.0, 201.0, 120.0, 100.0),
        (".block-b", 262.0, 321.0, 500.0, 60.0),
        (".sticky-bottom", 362.0, 401.0, 300.0, 50.0),
    ];
    for (sel, x, y, w, h) in expected {
        let b = box_of(&mut session, sel);
        assert!(
            (b.border_box.x - x).abs() < 1.0
                && (b.border_box.y - y).abs() < 1.0
                && (b.border_box.width - w).abs() < 1.0
                && (b.border_box.height - h).abs() < 1.0,
            "{sel} should be {w}x{h} at ({x},{y}), got {}x{} at ({},{})",
            b.border_box.width,
            b.border_box.height,
            b.border_box.x,
            b.border_box.y
        );
    }

    // block-a directly follows the sticky bar: bar.bottom (81) + 20px margin = 101.
    let bar = box_of(&mut session, ".sticky-bar");
    let block_a = box_of(&mut session, ".block-a");
    assert!(
        (block_a.border_box.y - (bar.border_box.y + bar.border_box.height + 20.0)).abs() < 1.0,
        "block-a should follow the sticky bar in normal flow (not shifted)"
    );
}
