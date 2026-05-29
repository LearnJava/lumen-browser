//! Test 35-grid-named-areas.html — grid-template-areas + spanning + dense auto-flow.
//!
//! A 2-column page layout places header/footer across both columns (964px) and
//! sidebar/main into the 200px/760px tracks (500px tall). A nested mini-grid proves
//! row-spanning: `.mini-a` occupies both 40px rows (40+2gap+40 = 82px) while its
//! siblings stay 40px. The dense grid proves column spans: `.dg-span2` = 2×90+3gap
//! = 183px, `.dg-span3` = 3×90+2×3gap = 276px.

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
fn test_35_grid_named_areas() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/35-grid-named-areas.html");

    // Page layout: header/footer span both columns (200+760+4gap = 964), sidebar/main
    // sit in their tracks at 500px tall.
    let areas = [
        (".header", 964.0, 60.0),
        (".footer", 964.0, 60.0),
        (".sidebar", 200.0, 500.0),
        (".main", 760.0, 500.0),
    ];
    for (sel, w, h) in areas {
        let b = box_of(&mut session, sel);
        assert!(
            (b.border_box.width - w).abs() < 1.0 && (b.border_box.height - h).abs() < 1.0,
            "{sel} should be {w}x{h}, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Nested mini-grid: `.mini-a` spans both rows (40 + 2gap + 40 = 82); the others
    // are single-row 40px. All columns are (300 − 2×2gap) / 3 = 98.67px wide.
    let mini_a = box_of(&mut session, ".mini-a");
    assert!(
        (mini_a.border_box.width - 98.67).abs() < 1.0 && (mini_a.border_box.height - 82.0).abs() < 1.0,
        ".mini-a should span 2 rows → 98.67x82, got {}x{}",
        mini_a.border_box.width,
        mini_a.border_box.height
    );
    for sel in [".mini-b", ".mini-c", ".mini-d", ".mini-e"] {
        let b = box_of(&mut session, sel);
        assert!(
            (b.border_box.width - 98.67).abs() < 1.0 && (b.border_box.height - 40.0).abs() < 1.0,
            "{sel} should be 98.67x40 (single row), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Dense grid spans: span-2 = 2×90 + 3gap = 183 (two such items), span-3 = 3×90 + 2×3gap = 276.
    let span2 = session.all_layout_boxes_by_selector(".dg-span2").expect("query .dg-span2");
    assert_eq!(span2.len(), 2, "expected 2 span-2 items");
    for (i, b) in span2.iter().enumerate() {
        assert!(
            (b.border_box.width - 183.0).abs() < 1.0 && (b.border_box.height - 50.0).abs() < 1.0,
            ".dg-span2[{i}] should be 183x50, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }
    let span3 = box_of(&mut session, ".dg-span3");
    assert!(
        (span3.border_box.width - 276.0).abs() < 1.0 && (span3.border_box.height - 50.0).abs() < 1.0,
        ".dg-span3 should be 276x50, got {}x{}",
        span3.border_box.width,
        span3.border_box.height
    );
}
