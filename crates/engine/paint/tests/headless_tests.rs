/// GPU headless render tests — require a real GPU adapter.
///
/// Marked `#[ignore]` by default so they don't run in CPU-only CI.
/// Run explicitly with:
///   cargo test -p lumen-paint --test headless_tests -- --include-ignored
use lumen_core::geom::Rect;
use lumen_paint::{DisplayCommand, Renderer};

const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

fn red_rect_dl(w: f32, h: f32) -> Vec<DisplayCommand> {
    vec![DisplayCommand::FillRect {
        rect: Rect { x: 0.0, y: 0.0, width: w, height: h },
        color: lumen_core::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
    }]
}

#[test]
#[ignore = "requires GPU adapter"]
fn headless_render_dimensions() {
    let mut r = Renderer::new_headless(64, 48, INTER.to_vec())
        .expect("headless renderer");
    let img = r.render_to_image(&red_rect_dl(64.0, 48.0), &[], 0.0, 0.0)
        .expect("render_to_image");
    assert_eq!(img.width, 64);
    assert_eq!(img.height, 48);
    assert_eq!(img.data.len(), 64 * 48 * 4);
}

#[test]
#[ignore = "requires GPU adapter"]
fn headless_render_red_rect() {
    let mut r = Renderer::new_headless(64, 64, INTER.to_vec())
        .expect("headless renderer");
    r.set_font_provider(None);
    let img = r.render_to_image(&red_rect_dl(64.0, 64.0), &[], 0.0, 0.0)
        .expect("render_to_image");

    // Centre pixel should be red (R=255 G=0 B=0 A=255).
    let cx = 32usize;
    let cy = 32usize;
    let offset = (cy * 64 + cx) * 4;
    let pix = &img.data[offset..offset + 4];
    assert_eq!(pix[0], 255, "R должен быть 255");
    assert_eq!(pix[1], 0,   "G должен быть 0");
    assert_eq!(pix[2], 0,   "B должен быть 0");
}

#[test]
#[ignore = "requires GPU adapter"]
fn headless_resize_updates_dimensions() {
    let mut r = Renderer::new_headless(32, 32, INTER.to_vec())
        .expect("headless renderer");
    r.resize(128, 96);
    let img = r.render_to_image(&red_rect_dl(128.0, 96.0), &[], 0.0, 0.0)
        .expect("render_to_image after resize");
    assert_eq!(img.width, 128);
    assert_eq!(img.height, 96);
}
