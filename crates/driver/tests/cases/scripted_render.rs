//! BUG-429 — the headless `InProcessSession` pipeline must run the page's own
//! scripts and hand the resulting Canvas 2D bitmaps to the CPU rasterizer.
//!
//! Before the fix, `run_pipeline` installed a V8 runtime but never executed a
//! single `<script>` of the page, and `screenshot_cpu_rgba` passed an
//! unconditionally empty image set to `render_to_image_cpu`. Both gaps were
//! invisible to `cases::snapshot_cpu`: its `57-canvas-2d` reference was a blank
//! frame, so a total breakage of Canvas 2D stayed green.
//!
//! These tests assert the two halves separately, by the property that was
//! broken — DOM built by a script reaching layout, and canvas pixels reaching
//! the raster — not by snapshot equality.
#![cfg(all(feature = "cpu-render", feature = "v8"))]

use lumen_driver::{BrowserSession, InProcessSession};

/// Count pixels whose RGB equals `(r, g, b)` (alpha ignored).
fn count_rgb(image: &lumen_image::Image, r: u8, g: u8, b: u8) -> usize {
    image
        .to_rgba8()
        .chunks_exact(4)
        .filter(|px| px[0] == r && px[1] == g && px[2] == b)
        .count()
}

#[test]
fn page_script_mutates_dom_before_layout() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body><div id="host"></div>
               <script>
                 var d = document.createElement('div');
                 d.setAttribute('style', 'width: 111px; height: 37px');
                 document.getElementById('host').appendChild(d);
               </script></body></html>"#,
        )
        .expect("navigate_html");

    let boxes = session.layout_snapshot().expect("layout_snapshot");
    assert!(
        boxes
            .iter()
            .any(|b| b.border_box.width == 111.0 && b.border_box.height == 37.0),
        "script-created div is missing from layout — page scripts did not run \
         before layout (BUG-429). Boxes: {:?}",
        boxes.iter().map(|b| &b.tag_name).collect::<Vec<_>>()
    );
}

#[test]
fn canvas_2d_pixels_reach_the_cpu_raster() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0">
               <canvas id="c" width="200" height="100"
                       style="width:200px;height:100px"></canvas>
               <script>
                 var ctx = document.getElementById('c').getContext('2d');
                 ctx.fillStyle = '#f97316';
                 ctx.fillRect(0, 0, 200, 100);
               </script></body></html>"#,
        )
        .expect("navigate_html");

    let image = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba");
    let orange = count_rgb(&image, 0xf9, 0x73, 0x16);
    assert!(
        orange > 10_000,
        "canvas fill is missing from the CPU render: {orange} orange pixels of an \
         expected ~20000 (BUG-429 — the image set handed to render_to_image_cpu \
         carried no canvas bitmaps)"
    );
}

#[test]
fn canvas_redrawn_after_navigation_is_picked_up() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0">
               <canvas id="c" width="200" height="100"
                       style="width:200px;height:100px"></canvas>
               <script>
                 var ctx = document.getElementById('c').getContext('2d');
                 ctx.fillStyle = '#f97316';
                 ctx.fillRect(0, 0, 200, 100);
               </script></body></html>"#,
        )
        .expect("navigate_html");

    // First screenshot drains the dirty buffer; the second must still see the
    // pixels (the drain is accumulated, not consumed per screenshot), and a
    // repaint through `eval` must replace them.
    let _ = session.screenshot_cpu_rgba().expect("first screenshot");
    let again = session.screenshot_cpu_rgba().expect("second screenshot");
    assert!(
        count_rgb(&again, 0xf9, 0x73, 0x16) > 10_000,
        "canvas pixels vanished on the second screenshot — the drain is consumed \
         instead of accumulated"
    );

    session
        .eval(
            "var ctx = document.getElementById('c').getContext('2d'); \
             ctx.fillStyle = '#10b981'; ctx.fillRect(0, 0, 200, 100); 1",
        )
        .expect("eval repaint");
    let repainted = session.screenshot_cpu_rgba().expect("third screenshot");
    assert!(
        count_rgb(&repainted, 0x10, 0xb9, 0x81) > 10_000,
        "canvas repainted through eval() is not visible in the next screenshot"
    );
}
