use lumen_image::{decode_jpeg, PixelFormat};

const RED_444: &[u8] = include_bytes!("fixtures/rgb_444_red_16x16.jpg");
const BLUE_420: &[u8] = include_bytes!("fixtures/rgb_420_blue_16x16.jpg");
const GRAY_DARK: &[u8] = include_bytes!("fixtures/gray_black_16x16.jpg");
const GRAY_LIGHT: &[u8] = include_bytes!("fixtures/gray_light_16x16.jpg");
const GREEN_422: &[u8] = include_bytes!("fixtures/rgb_422_green_16x16.jpg");
const PURPLE_NONALIGNED: &[u8] = include_bytes!("fixtures/rgb_420_purple_17x9.jpg");
const GRADIENT_V_32: &[u8] = include_bytes!("fixtures/gradient_v_32x32.jpg");
const GRADIENT_RB: &[u8] = include_bytes!("fixtures/gradient_red_blue_32x16.jpg");
const RESTART_24: &[u8] = include_bytes!("fixtures/restart_interval_24x24.jpg");
const PROG_RED_444: &[u8] = include_bytes!("fixtures/progressive_red_444_16x16.jpg");
const PROG_BLUE_420: &[u8] = include_bytes!("fixtures/progressive_blue_420_16x16.jpg");
const PROG_GRAY: &[u8] = include_bytes!("fixtures/progressive_gray_16x16.jpg");
const PROG_GRAD_V_32: &[u8] = include_bytes!("fixtures/progressive_gradient_v_32x32.jpg");
const PROG_GRAD_RB: &[u8] = include_bytes!("fixtures/progressive_gradient_rb_32x16.jpg");
const PROG_DC_REFINE: &[u8] = include_bytes!("fixtures/progressive_dc_refine_32x32.jpg");

#[test]
fn decode_solid_red_4_4_4_yields_red_pixels() {
    let image = decode_jpeg(RED_444).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Rgb8);
    assert_eq!(image.data.len(), 16 * 16 * 3);
    for px in image.data.chunks_exact(3) {
        assert!(px[0] > 230, "R={} expected ~255", px[0]);
        assert!(px[1] < 30, "G={} expected ~0", px[1]);
        assert!(px[2] < 30, "B={} expected ~0", px[2]);
    }
}

#[test]
fn decode_solid_blue_4_2_0_with_chroma_subsampling() {
    let image = decode_jpeg(BLUE_420).unwrap();
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        assert!(px[0] < 30, "R={} expected ~0", px[0]);
        assert!((100..=160).contains(&px[1]), "G={} expected ~128", px[1]);
        assert!(px[2] > 220, "B={} expected ~255", px[2]);
    }
}

#[test]
fn decode_grayscale_single_component_yields_gray8() {
    let image = decode_jpeg(GRAY_DARK).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Gray8);
    assert_eq!(image.data.len(), 16 * 16);
    for &v in &image.data {
        assert!(v < 20, "expected ~0, got {v}");
    }
}

#[test]
fn decode_grayscale_light_value() {
    let image = decode_jpeg(GRAY_LIGHT).unwrap();
    assert_eq!(image.format, PixelFormat::Gray8);
    for &v in &image.data {
        assert!((180..=220).contains(&v), "expected ~200, got {v}");
    }
}

#[test]
fn empty_input_fails_cleanly() {
    assert!(decode_jpeg(&[]).is_err());
}

#[test]
fn random_garbage_fails_cleanly() {
    assert!(decode_jpeg(&[0xDE, 0xAD, 0xBE, 0xEF]).is_err());
}

#[test]
fn truncated_after_soi_fails_cleanly() {
    assert!(decode_jpeg(&[0xFF, 0xD8]).is_err());
}

#[test]
fn decode_solid_green_4_2_2_horizontal_subsampling() {
    let image = decode_jpeg(GREEN_422).unwrap();
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        assert!(px[0] < 30);
        assert!((170..=230).contains(&px[1]), "G={} expected ~200", px[1]);
        assert!(px[2] < 30);
    }
}

#[test]
fn decode_non_mcu_aligned_dimensions_17x9() {
    let image = decode_jpeg(PURPLE_NONALIGNED).unwrap();
    assert_eq!(image.width, 17);
    assert_eq!(image.height, 9);
    assert_eq!(image.format, PixelFormat::Rgb8);
    assert_eq!(image.data.len(), 17 * 9 * 3);
    for px in image.data.chunks_exact(3) {
        assert!((120..=180).contains(&px[0]), "R={} expected ~150", px[0]);
        assert!((20..=80).contains(&px[1]), "G={} expected ~50", px[1]);
        assert!((170..=230).contains(&px[2]), "B={} expected ~200", px[2]);
    }
}

#[test]
fn decode_grayscale_vertical_gradient_monotonic() {
    let image = decode_jpeg(GRADIENT_V_32).unwrap();
    assert_eq!(image.format, PixelFormat::Gray8);
    let top: u32 = image.data[..32].iter().map(|&v| u32::from(v)).sum();
    let bottom: u32 = image.data[32 * 31..32 * 32].iter().map(|&v| u32::from(v)).sum();
    assert!(top / 32 < 30, "top mean={}", top / 32);
    assert!(bottom / 32 > 220, "bottom mean={}", bottom / 32);
    let middle: u32 = image.data[32 * 16..32 * 17].iter().map(|&v| u32::from(v)).sum();
    assert!((88..=168).contains(&(middle / 32)), "middle mean={}", middle / 32);
}

#[test]
fn decode_color_gradient_red_to_blue() {
    let image = decode_jpeg(GRADIENT_RB).unwrap();
    assert_eq!(image.format, PixelFormat::Rgb8);
    let row_bytes = 32 * 3;
    let top_first = &image.data[..3];
    let bottom_last = &image.data[15 * row_bytes + 31 * 3..15 * row_bytes + 32 * 3];
    assert!(top_first[0] > 200 && top_first[2] < 50);
    assert!(bottom_last[2] > 200 && bottom_last[0] < 50);
}

#[test]
fn decode_jpeg_with_restart_interval_24x24() {
    let image = decode_jpeg(RESTART_24).unwrap();
    assert_eq!(image.width, 24);
    assert_eq!(image.height, 24);
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        assert!((60..=140).contains(&px[0]), "R={} expected ~100", px[0]);
        assert!((110..=190).contains(&px[1]), "G={} expected ~150", px[1]);
        assert!((160..=240).contains(&px[2]), "B={} expected ~200", px[2]);
    }
}

#[test]
fn lossless_jpeg_marker_still_rejected() {
    let bytes = [
        0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x0B,
        0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00,
    ];
    assert!(decode_jpeg(&bytes).is_err(), "SOF3 (lossless) must be rejected");
}

#[test]
fn decode_progressive_solid_red_444() {
    let image = decode_jpeg(PROG_RED_444).unwrap();
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        assert!(px[0] > 180);
        assert!(px[1] < 70);
        assert!(px[2] < 70);
    }
}

#[test]
fn decode_progressive_solid_blue_420_with_subsampling() {
    let image = decode_jpeg(PROG_BLUE_420).unwrap();
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        assert!(px[0] < 80);
        assert!(px[1] < 80);
        assert!(px[2] > 180);
    }
}

#[test]
fn decode_progressive_grayscale() {
    let image = decode_jpeg(PROG_GRAY).unwrap();
    assert_eq!(image.format, PixelFormat::Gray8);
    let mean: u32 = image.data.iter().map(|&v| u32::from(v)).sum::<u32>() / (16 * 16);
    assert!((60..=190).contains(&mean), "mean={mean}");
}

#[test]
fn decode_progressive_gradient_grayscale_is_monotonic() {
    let image = decode_jpeg(PROG_GRAD_V_32).unwrap();
    assert_eq!(image.format, PixelFormat::Gray8);
    let top: u32 = image.data[..32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    let bottom: u32 = image.data[32 * 31..32 * 32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    assert!(top < 30);
    assert!(bottom > 220);
}

#[test]
fn decode_progressive_with_dc_refinement_via_jpegtran() {
    let image = decode_jpeg(PROG_DC_REFINE).unwrap();
    assert_eq!(image.format, PixelFormat::Gray8);
    let top: u32 = image.data[..32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    let bottom: u32 = image.data[32 * 31..32 * 32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    assert!(top < 35);
    assert!(bottom > 220);
}

#[test]
fn decode_progressive_gradient_red_blue() {
    let image = decode_jpeg(PROG_GRAD_RB).unwrap();
    assert_eq!(image.format, PixelFormat::Rgb8);
    let row_bytes = 32 * 3;
    let top_first = &image.data[..3];
    let bottom_last = &image.data[15 * row_bytes + 31 * 3..15 * row_bytes + 32 * 3];
    assert!(top_first[0] > 200 && top_first[2] < 50);
    assert!(bottom_last[2] > 200 && bottom_last[0] < 50);
}
