//! DEVX-9: `InProcessSession` re-layouts after a DOM mutation.
//!
//! Before this task, `click`/`type_text`/`eval` mutated the DOM through the
//! persistent V8 runtime, but `layout_snapshot`/`layout_box_by_selector`/
//! `screenshot` kept reflecting the page's geometry as it was at `navigate()`
//! — there was no relayout path on the headless pipeline to piggyback on
//! (mirrors the `getComputedStyle`/`getBoundingClientRect` snapshot-push gap
//! fixed by BUG-382, one layer up the same pipeline). These tests cover the
//! DoD from `docs/tasks/p1-introspection-track.md` §DEVX-9: an `eval` that
//! changes an element's geometry must be visible both in
//! `layout_box_by_selector` and in the deterministic CPU screenshot.
#![cfg(all(feature = "v8", feature = "cpu-render"))]

use lumen_driver::{BrowserSession, InProcessSession};

/// RGBA8 pixel at `(x, y)` in a `screenshot_cpu_rgba()` image.
fn pixel(image: &lumen_image::Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    [image.data[i], image.data[i + 1], image.data[i + 2], image.data[i + 3]]
}

#[test]
fn eval_geometry_mutation_visible_in_layout_box_by_selector() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body><div id="box" style="width:50px;height:20px"></div></body></html>"#,
        )
        .expect("navigate_html failed");

    let before = session
        .layout_box_by_selector("#box")
        .expect("layout_box_by_selector failed")
        .expect("#box not found");
    assert_eq!(before.border_box.width, 50.0, "sanity: pre-mutation width");

    session
        .eval("document.getElementById('box').style.width = '200px'")
        .expect("eval failed");

    let after = session
        .layout_box_by_selector("#box")
        .expect("layout_box_by_selector failed")
        .expect("#box not found after mutation");
    assert_eq!(
        after.border_box.width, 200.0,
        "DEVX-9: layout_box_by_selector must reflect the post-eval DOM geometry, \
         not the state at navigate()"
    );
}

#[test]
fn eval_geometry_mutation_visible_in_cpu_screenshot() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0;background:white">
                 <div id="box" style="width:10px;height:10px;background:red"></div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    // A probe pixel outside the initial 10x10 box, well clear of any edge
    // anti-aliasing once the box grows to cover it.
    let (probe_x, probe_y) = (50, 5);

    let before = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba failed");
    assert_eq!(
        pixel(&before, probe_x, probe_y),
        [255, 255, 255, 255],
        "sanity: probe pixel starts as page background"
    );

    session
        .eval("document.getElementById('box').style.width = '100px'")
        .expect("eval failed");

    let after = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba failed");
    assert_eq!(
        pixel(&after, probe_x, probe_y),
        [255, 0, 0, 255],
        "DEVX-9: CPU screenshot must reflect the widened box after eval, not the \
         pre-mutation geometry"
    );
}
