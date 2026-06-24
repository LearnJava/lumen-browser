//! Test 38-z-index.html — absolutely-positioned stacking contexts.
//!
//! z-index controls paint order only and cannot be observed from layout, so this
//! test verifies the *positioning* that the stacking demo relies on: seven
//! absolutely-positioned boxes resolve their top/left against the relative `.__f`
//! padding box (origin at 1+40 = 41 from the viewport, but offsets are measured as
//! `top/left + 1` because there is no border). The three concentric squares are
//! each inset 50px, proving nested absolute boxes stay centred.

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
fn test_38_z_index() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/38-z-index.html");

    // (selector, x, y, w, h) — left/top offsets resolve against the padding box (+1 origin).
    let expected = [
        (".green", 81.0, 81.0, 300.0, 300.0),
        (".blue", 131.0, 131.0, 200.0, 200.0),
        (".red", 181.0, 181.0, 100.0, 100.0),
        (".neg", 81.0, 461.0, 120.0, 120.0),
        (".auto-z", 241.0, 461.0, 120.0, 120.0),
        (".zero-z", 401.0, 461.0, 120.0, 120.0),
        (".high-z", 441.0, 481.0, 120.0, 120.0),
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

    // The three concentric squares are each inset by exactly 50px on both axes.
    let green = box_of(&mut session, ".green");
    let blue = box_of(&mut session, ".blue");
    let red = box_of(&mut session, ".red");
    assert!(
        (blue.border_box.x - green.border_box.x - 50.0).abs() < 1.0
            && (red.border_box.x - blue.border_box.x - 50.0).abs() < 1.0,
        "concentric squares should each be inset 50px"
    );
}
