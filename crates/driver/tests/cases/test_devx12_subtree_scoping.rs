//! DEVX-12: `query`/`screenshot`/display-list dump/relayout scoped to one
//! element's subtree instead of the whole page.
//!
//! Covers the DoD from `docs/tasks/p1-introspection-track.md` §DEVX-12:
//! each scoped read returns only data from inside the selector's subtree,
//! `screenshot_scoped` crops to the element's border-box, and
//! `eval_scoped`/`relayout_scoped` recompute geometry for a mutated subtree
//! without a full-page relayout — visible the same way DEVX-9's full
//! relayout is (`layout_box_by_selector`, CPU screenshot).
#![cfg(all(feature = "v8", feature = "cpu-render"))]

use lumen_driver::{BrowserSession, InProcessSession};

/// RGBA8 pixel at `(x, y)` in a `screenshot_cpu_rgba()`/decoded PNG image.
fn pixel(image: &lumen_image::Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    [image.data[i], image.data[i + 1], image.data[i + 2], image.data[i + 3]]
}

fn page() -> InProcessSession {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0">
                 <div id="outside" style="width:10px;height:10px"></div>
                 <div id="scope">
                   <p id="inside" style="width:20px;height:5px">hi</p>
                 </div>
               </body></html>"#,
        )
        .expect("navigate_html failed");
    session
}

#[test]
fn layout_snapshot_scoped_excludes_boxes_outside_the_subtree() {
    let session = page();
    let boxes = session.layout_snapshot_scoped("#scope").expect("layout_snapshot_scoped failed");
    let ids: Vec<u32> = boxes.iter().map(|b| b.node_id).collect();
    assert!(
        boxes.iter().any(|b| b.tag_name == "p"),
        "expected #scope's descendant <p> in the scoped snapshot: {ids:?}"
    );
    let outside = session.layout_box_by_selector("#outside").unwrap().unwrap();
    assert!(
        !ids.contains(&outside.node_id),
        "#outside is a sibling of #scope, not a descendant — must not appear in the scoped snapshot"
    );
}

#[test]
fn layout_snapshot_scoped_empty_for_missing_selector() {
    let session = page();
    let boxes = session.layout_snapshot_scoped("#does-not-exist").expect("layout_snapshot_scoped failed");
    assert!(boxes.is_empty());
}

#[test]
fn query_scoped_restricts_to_subtree() {
    let session = page();
    let inside = session.query_scoped("#scope", "p").expect("query_scoped failed");
    assert_eq!(inside.len(), 1, "expected exactly one <p> inside #scope");

    let outside = session.query_scoped("#outside", "p").expect("query_scoped failed");
    assert!(outside.is_empty(), "#outside has no <p> descendant");
}

#[test]
fn screenshot_scoped_crops_to_element_border_box() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0;background:white">
                 <div id="box" style="position:absolute;left:5px;top:5px;width:10px;height:10px;background:red"></div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    let png = session.screenshot_scoped("#box").expect("screenshot_scoped failed");
    let image = lumen_image::decode(&png).expect("decode cropped png");
    assert_eq!(image.width, 10, "cropped width must match border-box width");
    assert_eq!(image.height, 10, "cropped height must match border-box height");
    assert_eq!(pixel(&image, 5, 5), [255, 0, 0, 255], "crop must contain the box's own red fill");
}

#[test]
fn screenshot_scoped_errors_for_missing_selector() {
    let session = page();
    assert!(session.screenshot_scoped("#does-not-exist").is_err());
}

#[test]
fn display_list_scoped_contains_only_subtree_commands() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0">
                 <div id="outside" style="width:10px;height:10px;background:blue"></div>
                 <div id="scope" style="width:10px;height:10px;background:red"></div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    let dump = session.display_list_scoped("#scope").expect("display_list_scoped failed");
    assert!(dump.contains("FillRect"), "expected #scope's own fill command in the dump:\n{dump}");
    // #ff0000 red (scope) must appear, #0000ff blue (outside sibling) must not.
    assert!(dump.contains("#ff0000ff"), "expected #scope's own color in the dump:\n{dump}");
    assert!(!dump.contains("#0000ffff"), "must not include #outside's commands:\n{dump}");
}

#[test]
fn display_list_scoped_errors_for_missing_selector() {
    let session = page();
    assert!(session.display_list_scoped("#does-not-exist").is_err());
}

#[test]
fn eval_scoped_geometry_mutation_visible_in_layout_box_by_selector() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body><div id="box" style="width:50px;height:20px"></div></body></html>"#,
        )
        .expect("navigate_html failed");

    let before = session.layout_box_by_selector("#box").unwrap().unwrap();
    assert_eq!(before.border_box.width, 50.0, "sanity: pre-mutation width");

    session
        .eval_scoped("#box", "document.getElementById('box').style.width = '200px'")
        .expect("eval_scoped failed");

    let after = session.layout_box_by_selector("#box").unwrap().unwrap();
    assert_eq!(
        after.border_box.width, 200.0,
        "DEVX-12: eval_scoped's subtree relayout must reflect the post-eval DOM geometry"
    );
}

#[test]
fn relayout_scoped_errors_for_missing_selector() {
    let mut session = page();
    assert!(session.relayout_scoped("#does-not-exist").is_err());
}
