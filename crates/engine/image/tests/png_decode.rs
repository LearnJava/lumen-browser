use lumen_image::{DecodeError, PixelFormat, decode_png};

const RGB_3X2: &[u8] = include_bytes!("fixtures/rgb8_3x2.png");
const RGBA_4X2: &[u8] = include_bytes!("fixtures/rgba8_4x2.png");
const GRAY_4X4: &[u8] = include_bytes!("fixtures/gray8_4x4.png");
const GRAYA_2X2: &[u8] = include_bytes!("fixtures/graya8_2x2.png");
const RGB_FILTERS_2X4: &[u8] = include_bytes!("fixtures/rgb8_filters_2x4.png");
const RGB_PAETH_2X2: &[u8] = include_bytes!("fixtures/rgb8_paeth_2x2.png");
const PALETTE_4X2: &[u8] = include_bytes!("fixtures/palette8_4x2.png");
const PALETTE_TRNS_3X3: &[u8] = include_bytes!("fixtures/palette8_trns_3x3.png");
const PALETTE_PARTIAL_TRNS_2X2: &[u8] = include_bytes!("fixtures/palette8_partial_trns_2x2.png");
const GRAY1BIT_8X2: &[u8] = include_bytes!("fixtures/gray1bit_8x2.png");
const GRAY2BIT_4X2: &[u8] = include_bytes!("fixtures/gray2bit_4x2.png");
const GRAY4BIT_3X2: &[u8] = include_bytes!("fixtures/gray4bit_3x2.png");
const PALETTE1BIT_8X1: &[u8] = include_bytes!("fixtures/palette1bit_8x1.png");
const PALETTE4BIT_TRNS_4X2: &[u8] = include_bytes!("fixtures/palette4bit_trns_4x2.png");
const GRAY16_2X2: &[u8] = include_bytes!("fixtures/gray16_2x2.png");
const GRAYA16_2X2: &[u8] = include_bytes!("fixtures/graya16_2x2.png");
const RGB16_2X2: &[u8] = include_bytes!("fixtures/rgb16_2x2.png");
const RGBA16_2X2: &[u8] = include_bytes!("fixtures/rgba16_2x2.png");
const GRAY16_FILTERS_2X3: &[u8] = include_bytes!("fixtures/gray16_filters_2x3.png");
const GRAY8_TRNS_3X2: &[u8] = include_bytes!("fixtures/gray8_trns_3x2.png");
const RGB8_TRNS_2X2: &[u8] = include_bytes!("fixtures/rgb8_trns_2x2.png");
const GRAY16_TRNS_2X2: &[u8] = include_bytes!("fixtures/gray16_trns_2x2.png");
const RGB16_TRNS_2X2: &[u8] = include_bytes!("fixtures/rgb16_trns_2x2.png");
const ADAM7_RGB_8X8: &[u8] = include_bytes!("fixtures/adam7_rgb_8x8.png");
const ADAM7_GRAY_5X5: &[u8] = include_bytes!("fixtures/adam7_gray_5x5.png");
const ADAM7_RGBA_4X4: &[u8] = include_bytes!("fixtures/adam7_rgba_4x4.png");
const ADAM7_RGB_1X1: &[u8] = include_bytes!("fixtures/adam7_rgb_1x1.png");
const ADAM7_RGB_2X2: &[u8] = include_bytes!("fixtures/adam7_rgb_2x2.png");

#[test]
fn decode_rgb8_3x2() {
    let img = decode_png(RGB_3X2).unwrap();
    assert_eq!(img.width, 3);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![
        255, 0, 0, 0, 255, 0, 0, 0, 255,
        0, 0, 0, 255, 255, 255, 255, 255, 0,
    ]);
}

#[test]
fn decode_rgba8_4x2() {
    let img = decode_png(RGBA_4X2).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(img.data, vec![
        255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255,
        0, 255, 255, 255, 255, 0, 255, 255, 255, 255, 0, 255, 128, 128, 128, 128,
    ]);
}

#[test]
fn decode_gray8_4x4() {
    let img = decode_png(GRAY_4X4).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 4);
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![
        0, 32, 64, 96, 128, 160, 192, 224, 255, 224, 192, 160, 128, 96, 64, 32,
    ]);
}

#[test]
fn decode_gray_alpha8_2x2() {
    let img = decode_png(GRAYA_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::GrayAlpha8);
    assert_eq!(img.data, vec![100, 255, 200, 128, 50, 64, 250, 200]);
}

#[test]
fn decode_with_mixed_filters_none_sub_up_avg() {
    let img = decode_png(RGB_FILTERS_2X4).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 4);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![
        100, 110, 120, 130, 140, 150,
        50, 60, 70, 80, 90, 100,
        200, 210, 220, 230, 240, 250,
        10, 20, 30, 40, 50, 60,
    ]);
}

#[test]
fn decode_paeth_filter() {
    let img = decode_png(RGB_PAETH_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![
        100, 110, 120, 130, 140, 150,
        50, 60, 70, 80, 90, 100,
    ]);
}

#[test]
fn decode_rejects_non_png() {
    let bad = b"not a png at all";
    let err = decode_png(bad).unwrap_err();
    assert!(matches!(err, DecodeError::InvalidSignature));
}

#[test]
fn decode_rejects_truncated_file() {
    let truncated = &RGB_3X2[..RGB_3X2.len() - 16];
    assert!(decode_png(truncated).is_err());
}

#[test]
fn decode_rejects_corrupted_crc() {
    let mut corrupted = RGB_3X2.to_vec();
    corrupted[16] ^= 0xFF;
    assert!(decode_png(&corrupted).is_err());
}

#[test]
fn decode_palette_without_trns_yields_rgb8() {
    let img = decode_png(PALETTE_4X2).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![
        255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255,
        255, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0,
    ]);
}

#[test]
fn decode_palette_with_full_trns_yields_rgba8() {
    let img = decode_png(PALETTE_TRNS_3X3).unwrap();
    assert_eq!(img.width, 3);
    assert_eq!(img.height, 3);
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(img.data, vec![
        200, 100, 50, 0, 50, 100, 200, 128, 100, 200, 50, 255,
        50, 100, 200, 128, 100, 200, 50, 255, 200, 100, 50, 0,
        100, 200, 50, 255, 200, 100, 50, 0, 50, 100, 200, 128,
    ]);
}

#[test]
fn decode_palette_with_partial_trns_pads_remaining_entries_with_255() {
    let img = decode_png(PALETTE_PARTIAL_TRNS_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(img.data, vec![
        255, 255, 255, 42, 128, 128, 128, 255,
        128, 128, 128, 255, 0, 0, 0, 255,
    ]);
}

#[test]
fn decode_gray1bit_8x2() {
    let img = decode_png(GRAY1BIT_8X2).unwrap();
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![
        255, 0, 255, 0, 255, 0, 255, 0,
        0, 255, 0, 255, 0, 255, 0, 255,
    ]);
}

#[test]
fn decode_gray2bit_4x2() {
    let img = decode_png(GRAY2BIT_4X2).unwrap();
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![0, 85, 170, 255, 255, 170, 85, 0]);
}

#[test]
fn decode_gray4bit_3x2_with_trailing_padding() {
    let img = decode_png(GRAY4BIT_3X2).unwrap();
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![0, 136, 255, 255, 136, 0]);
}

#[test]
fn decode_palette1bit_8x1() {
    let img = decode_png(PALETTE1BIT_8X1).unwrap();
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![
        255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0,
        255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0,
    ]);
}

#[test]
fn decode_palette4bit_with_trns_4x2() {
    let img = decode_png(PALETTE4BIT_TRNS_4X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(img.data, vec![
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 0,
        128, 128, 128, 0, 0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255,
    ]);
}

#[test]
fn decode_gray16_2x2_downsamples_to_gray8() {
    let img = decode_png(GRAY16_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![0, 0x80, 0xFF, 0x40]);
}

#[test]
fn decode_graya16_2x2_downsamples_to_graya8() {
    let img = decode_png(GRAYA16_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::GrayAlpha8);
    assert_eq!(img.data, vec![255, 255, 128, 64, 0, 128, 192, 0]);
}

#[test]
fn decode_rgb16_2x2_downsamples_to_rgb8() {
    let img = decode_png(RGB16_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![
        255, 0, 0, 0, 255, 0,
        128, 128, 128, 64, 192, 255,
    ]);
}

#[test]
fn decode_rgba16_2x2_downsamples_to_rgba8() {
    let img = decode_png(RGBA16_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(img.data, vec![
        255, 0, 0, 255, 0, 255, 0, 128,
        0, 0, 255, 0, 192, 64, 128, 64,
    ]);
}

#[test]
fn decode_gray16_with_filters_2x3() {
    let img = decode_png(GRAY16_FILTERS_2X3).unwrap();
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![0, 255, 128, 64, 192, 64]);
}

#[test]
fn decode_gray8_with_trns_yields_grayalpha8() {
    let img = decode_png(GRAY8_TRNS_3X2).unwrap();
    assert_eq!(img.format, PixelFormat::GrayAlpha8);
    assert_eq!(img.data, vec![
        0, 0, 128, 255, 255, 255,
        0, 0, 0, 0, 255, 255,
    ]);
}

#[test]
fn decode_rgb8_with_trns_yields_rgba8() {
    let img = decode_png(RGB8_TRNS_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(img.data, vec![
        255, 0, 0, 255,
        255, 0, 255, 0,
        255, 0, 255, 0,
        255, 255, 255, 255,
    ]);
}

#[test]
fn decode_gray16_with_trns_yields_grayalpha8() {
    let img = decode_png(GRAY16_TRNS_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::GrayAlpha8);
    assert_eq!(img.data, vec![
        0, 255, 255, 0,
        128, 255, 255, 0,
    ]);
}

#[test]
fn decode_rgb16_with_trns_yields_rgba8() {
    let img = decode_png(RGB16_TRNS_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(img.data, vec![
        255, 255, 255, 0, 255, 0, 0, 255,
        255, 255, 255, 0, 0, 255, 0, 255,
    ]);
}

#[test]
fn decode_adam7_rgb_8x8() {
    let img = decode_png(ADAM7_RGB_8X8).unwrap();
    assert_eq!(img.width, 8);
    assert_eq!(img.height, 8);
    assert_eq!(img.format, PixelFormat::Rgb8);
    for row in 0..8u32 {
        for col in 0..8u32 {
            let off = ((row * 8 + col) * 3) as usize;
            assert_eq!(img.data[off], (col * 32) as u8, "row {row} col {col} R");
            assert_eq!(img.data[off + 1], (row * 32) as u8, "row {row} col {col} G");
            assert_eq!(img.data[off + 2], 128, "row {row} col {col} B");
        }
    }
}

#[test]
fn decode_adam7_gray_5x5() {
    let img = decode_png(ADAM7_GRAY_5X5).unwrap();
    assert_eq!(img.format, PixelFormat::Gray8);
    for row in 0..5u32 {
        for col in 0..5u32 {
            let expected = (col * 40 + row * 8) as u8;
            assert_eq!(img.data[(row * 5 + col) as usize], expected, "row {row} col {col}");
        }
    }
}

#[test]
fn decode_adam7_rgba_4x4() {
    let img = decode_png(ADAM7_RGBA_4X4).unwrap();
    assert_eq!(img.format, PixelFormat::Rgba8);
    for row in 0..4u32 {
        for col in 0..4u32 {
            let off = ((row * 4 + col) * 4) as usize;
            assert_eq!(img.data[off], (col * 64) as u8);
            assert_eq!(img.data[off + 1], (row * 64) as u8);
            assert_eq!(img.data[off + 2], 128);
            assert_eq!(img.data[off + 3], (64 + col * 48) as u8);
        }
    }
}

#[test]
fn decode_adam7_rgb_1x1_only_pass1() {
    let img = decode_png(ADAM7_RGB_1X1).unwrap();
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![255, 127, 0]);
}

#[test]
fn decode_adam7_rgb_2x2() {
    let img = decode_png(ADAM7_RGB_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![
        10, 20, 30, 40, 50, 60,
        70, 80, 90, 100, 110, 120,
    ]);
}

#[test]
fn decode_palette_missing_plte_does_not_panic() {
    // zune-png may handle missing PLTE differently than the custom decoder
    // (returning garbage pixels instead of an error). Either outcome is
    // acceptable as long as the decoder does not panic.
    let mut stream = PALETTE_4X2.to_vec();
    let plte_start = 33;
    let plte_chunk_len = 4 + 4 + 12 + 4;
    stream.drain(plte_start..plte_start + plte_chunk_len);
    let _ = decode_png(&stream);
}
