//! Test 32-list-markers.html — list-style-type / list-style-position markers.
//!
//! Eight lists exercise disc/circle/square/decimal/lower-alpha/lower-roman markers
//! plus inside-position and `none`. The load-bearing checks: every `<li>` lays out
//! as a 26.4px-tall list-item block, consecutive items stack with a 28.4px y-step
//! (26.4 box + 2px margin), and a marker glyph box (24x22.4) is generated for each
//! item EXCEPT the two in the `list-style-type:none` list → 20 markers for 22 items.

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
fn test_32_list_markers() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/32-list-markers.html");

    // 8 lists × items: 6 lists of 3 + 2 lists of 2 = 22 <li> total.
    let lis = session.all_layout_boxes_by_selector("li").expect("query li");
    assert_eq!(lis.len(), 22, "expected 22 li boxes");
    for (i, li) in lis.iter().enumerate() {
        assert!(
            (li.border_box.height - 26.4).abs() < 1.0,
            "li[{i}] should be 26.4px tall (16px text × 1.4 lh + 2px×2 padding), got {}",
            li.border_box.height
        );
    }

    // First list: three items stack with a 28.4px step (26.4 box + 2px bottom margin).
    let step = lis[1].border_box.y - lis[0].border_box.y;
    assert!(
        (step - 28.4).abs() < 1.0,
        "consecutive li y-step should be 28.4 (box + margin), got {step}"
    );

    // Marker glyph boxes (24x22.4) are anonymous (empty tag_name); count them in the
    // flat snapshot. 6 marker-lists × 3 + 2 inside-list markers = 20 (none-list emits 0).
    let snap = session.layout_snapshot().expect("snapshot");
    let markers = snap
        .iter()
        .filter(|b| {
            (b.border_box.width - 24.0).abs() < 0.5 && (b.border_box.height - 22.4).abs() < 0.5
        })
        .count();
    assert_eq!(markers, 20, "expected 20 marker boxes (22 items − 2 in the none list)");
}
