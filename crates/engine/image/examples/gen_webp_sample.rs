/// Generates samples/images/webp_sample.webp — a 200×150 blue-teal rectangle
/// used in graphic_tests/18-images.html to verify WebP decoding.
///
/// Run once from the worktree root:
///   cargo run --example gen_webp_sample -p lumen-image
fn main() {
    use image_webp::{ColorType, WebPEncoder};
    let (w, h) = (200u32, 150u32);
    // Solid blue-teal (#1e78c8) — visually distinct from the PNG test images.
    let data: Vec<u8> = (0..w * h).flat_map(|_| [0x1eu8, 0x78u8, 0xc8u8]).collect();
    let mut out = Vec::new();
    WebPEncoder::new(&mut out)
        .encode(&data, w, h, ColorType::Rgb8)
        .expect("encode WebP sample");
    let path = "samples/images/webp_sample.webp";
    std::fs::write(path, &out).expect("write webp_sample.webp");
    println!("Generated {path}: {} bytes ({w}x{h} RGB WebP)", out.len());
}
