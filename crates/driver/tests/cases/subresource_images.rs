//! BUG-430 — the headless `InProcessSession` pipeline must fetch and decode the
//! page's *local* images, so that a page with pictures renders pictures.
//!
//! Before the fix `run_pipeline` loaded no subresources at all: every
//! `DrawImage` resolved to an unregistered key and painted the grey placeholder
//! quad, and every `DrawBackgroundImage` painted nothing. The gap was invisible
//! to `cases::snapshot_cpu` — its `18-images`/`19-object-fit` references were
//! frames full of grey rectangles, so a total breakage of decoding, source
//! selection or resampling would have stayed green.
//!
//! These tests assert the properties that were broken — decoded pixels reaching
//! the raster, intrinsic size reaching layout, a background image reaching the
//! raster, and the offline policy (nothing is fetched over the network) — not
//! snapshot equality.
#![cfg(feature = "cpu-render")]

use lumen_driver::{BrowserSession, InProcessSession};
use std::path::{Path, PathBuf};

/// Solid-colour RGBA8 PNG `w`×`h`, written next to the page under test.
fn write_png(path: &Path, w: u32, h: u32, rgba: [u8; 4]) {
    let image = lumen_image::Image {
        width: w,
        height: h,
        format: lumen_image::PixelFormat::Rgba8,
        data: rgba.iter().copied().cycle().take((w * h * 4) as usize).collect(),
        icc_profile: None,
    };
    let png = lumen_image::encode_png_rgba8(&image).expect("encode fixture png");
    std::fs::write(path, png).expect("write fixture png");
}

/// Create a fresh directory under the OS temp dir, unique per `name`.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lumen-bug430-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Write `html` into `dir/page.html` and navigate a fresh session to it.
///
/// The page has to come off the filesystem: a relative `src` resolves against
/// the page's own directory, and `navigate_html` gives the document no base at
/// all (that path is covered by `relative_src_without_local_base_is_skipped`).
fn session_on_page(dir: &Path, html: &str) -> InProcessSession {
    let page = dir.join("page.html");
    std::fs::write(&page, html).expect("write page");
    let mut session = InProcessSession::new();
    session
        .navigate(&format!("file://{}", page.display()))
        .expect("navigate");
    session
}

/// Count pixels whose RGB equals `(r, g, b)` (alpha ignored).
fn count_rgb(image: &lumen_image::Image, r: u8, g: u8, b: u8) -> usize {
    image
        .to_rgba8()
        .chunks_exact(4)
        .filter(|px| px[0] == r && px[1] == g && px[2] == b)
        .count()
}

/// The grey `<img>` placeholder quad (`cpu_raster::rasterize_image_placeholder`).
const PLACEHOLDER: (u8, u8, u8) = (217, 217, 217);

#[test]
fn local_img_pixels_reach_the_cpu_raster() {
    let dir = scratch_dir("img-pixels");
    write_png(&dir.join("red.png"), 4, 4, [0xd0, 0x21, 0x21, 0xff]);
    let session = session_on_page(
        &dir,
        r#"<html><body style="margin:0">
           <img src="red.png" width="200" height="100" alt="">
           </body></html>"#,
    );

    let image = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba");
    let red = count_rgb(&image, 0xd0, 0x21, 0x21);
    let grey = count_rgb(&image, PLACEHOLDER.0, PLACEHOLDER.1, PLACEHOLDER.2);
    assert!(
        red > 19_000,
        "decoded image is missing from the CPU render: {red} red pixels of an \
         expected ~20000, {grey} grey (BUG-430 — the session fetched no \
         subresources, so DrawImage fell back to the placeholder)"
    );
    assert_eq!(grey, 0, "placeholder still painted over the decoded image");
}

#[test]
fn intrinsic_size_of_a_local_image_reaches_layout() {
    let dir = scratch_dir("intrinsic");
    write_png(&dir.join("wide.png"), 120, 80, [0x11, 0x22, 0x33, 0xff]);
    let session = session_on_page(
        &dir,
        r#"<html><body style="margin:0"><img src="wide.png" alt=""></body></html>"#,
    );

    let boxes = session.layout_snapshot().expect("layout_snapshot");
    assert!(
        boxes
            .iter()
            .any(|b| b.border_box.width == 120.0 && b.border_box.height == 80.0),
        "the <img> box did not take the decoded image's intrinsic 120x80 — \
         apply_intrinsic_size never ran before layout (BUG-430). Boxes: {:?}",
        boxes
            .iter()
            .map(|b| (&b.tag_name, b.border_box.width, b.border_box.height))
            .collect::<Vec<_>>()
    );
}

#[test]
fn local_background_image_reaches_the_cpu_raster() {
    let dir = scratch_dir("background");
    write_png(&dir.join("green.png"), 4, 4, [0x1f, 0xa8, 0x4a, 0xff]);
    let session = session_on_page(
        &dir,
        r#"<html><body style="margin:0">
           <div style="width:200px;height:100px;
                       background-image:url(green.png);
                       background-size:200px 100px"></div>
           </body></html>"#,
    );

    let image = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba");
    let green = count_rgb(&image, 0x1f, 0xa8, 0x4a);
    assert!(
        green > 19_000,
        "background image is missing from the CPU render: {green} green pixels \
         of an expected ~20000 (BUG-430 — an unregistered DrawBackgroundImage \
         key paints nothing at all, so the frame stayed blank)"
    );
}

#[test]
fn network_src_is_not_fetched_and_keeps_the_placeholder() {
    let dir = scratch_dir("network");
    let session = session_on_page(
        &dir,
        r#"<html><body style="margin:0">
           <img src="http://127.0.0.1:9/never.png" width="200" height="100" alt="">
           </body></html>"#,
    );

    let image = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba");
    let grey = count_rgb(&image, PLACEHOLDER.0, PLACEHOLDER.1, PLACEHOLDER.2);
    assert!(
        grey > 19_000,
        "a network <img> must stay the grey placeholder — the offline pipeline \
         does not go to the network (BUG-430 policy): {grey} grey pixels of an \
         expected ~20000"
    );
}

#[test]
fn relative_src_without_local_base_is_skipped() {
    // `navigate_html` has no base URL, so a relative source cannot be resolved;
    // the box must degrade to the placeholder rather than reading a path
    // relative to the process's current directory.
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0">
               <img src="red.png" width="200" height="100" alt="">
               </body></html>"#,
        )
        .expect("navigate_html");

    let image = session.screenshot_cpu_rgba().expect("screenshot_cpu_rgba");
    let grey = count_rgb(&image, PLACEHOLDER.0, PLACEHOLDER.1, PLACEHOLDER.2);
    assert!(
        grey > 19_000,
        "a baseless relative <img> must stay the grey placeholder: {grey} grey \
         pixels of an expected ~20000"
    );
}

#[test]
fn navigation_drops_the_previous_page_images() {
    let dir = scratch_dir("navigate-away");
    write_png(&dir.join("blue.png"), 4, 4, [0x1d, 0x4e, 0xd8, 0xff]);
    let page_a = dir.join("a.html");
    std::fs::write(
        &page_a,
        r#"<html><body style="margin:0">
           <img src="blue.png" width="200" height="100" alt=""></body></html>"#,
    )
    .expect("write a.html");

    let mut session = InProcessSession::new();
    session
        .navigate(&format!("file://{}", page_a.display()))
        .expect("navigate a");
    assert!(
        count_rgb(
            &session.screenshot_cpu_rgba().expect("screenshot a"),
            0x1d,
            0x4e,
            0xd8
        ) > 19_000,
        "first page did not render its image"
    );

    // Second page references the same file name, which no longer exists next to
    // it: a stale registration from page A would paint blue pixels here.
    let dir_b = scratch_dir("navigate-away-b");
    let page_b = dir_b.join("b.html");
    std::fs::write(
        &page_b,
        r#"<html><body style="margin:0">
           <img src="blue.png" width="200" height="100" alt=""></body></html>"#,
    )
    .expect("write b.html");
    session
        .navigate(&format!("file://{}", page_b.display()))
        .expect("navigate b");

    let image = session.screenshot_cpu_rgba().expect("screenshot b");
    assert_eq!(
        count_rgb(&image, 0x1d, 0x4e, 0xd8),
        0,
        "images of the previous page survived the navigation"
    );
}
