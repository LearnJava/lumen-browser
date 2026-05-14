//! Интеграционные тесты для `lumen_image::decode` — диспатч по сигнатуре
//! между PNG и JPEG декодерами на реальных фикстурах.

use lumen_image::{decode, ImageError, PixelFormat};

const RGB_PNG_3X2: &[u8] = include_bytes!("fixtures/rgb8_3x2.png");
const GRAY_PNG_4X4: &[u8] = include_bytes!("fixtures/gray8_4x4.png");
const JPEG_RGB_GRADIENT: &[u8] = include_bytes!("fixtures/gradient_red_blue_32x16.jpg");
const JPEG_GRAY_BLACK: &[u8] = include_bytes!("fixtures/gray_black_16x16.jpg");

#[test]
fn decode_real_png_rgb() {
    let img = decode(RGB_PNG_3X2).expect("PNG fixture должен декодироваться");
    assert_eq!(img.width, 3);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
}

#[test]
fn decode_real_png_grayscale() {
    let img = decode(GRAY_PNG_4X4).expect("PNG fixture должен декодироваться");
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 4);
    assert_eq!(img.format, PixelFormat::Gray8);
}

#[test]
fn decode_real_jpeg_rgb() {
    let img = decode(JPEG_RGB_GRADIENT).expect("JPEG fixture должен декодироваться");
    assert_eq!(img.width, 32);
    assert_eq!(img.height, 16);
    assert_eq!(img.format, PixelFormat::Rgb8);
}

#[test]
fn decode_real_jpeg_grayscale() {
    let img = decode(JPEG_GRAY_BLACK).expect("JPEG fixture должен декодироваться");
    assert_eq!(img.width, 16);
    assert_eq!(img.height, 16);
    assert_eq!(img.format, PixelFormat::Gray8);
}

#[test]
fn decode_garbage_returns_unknown_format() {
    let garbage = b"this is not an image, just some text bytes for testing";
    assert_eq!(decode(garbage), Err(ImageError::UnknownFormat));
}

#[test]
fn decode_png_signature_with_truncated_body_returns_png_error() {
    // PNG-сигнатура есть, но дальше пусто — диспатч должен уйти в PNG-декодер.
    let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.push(0); // длина чанка обрывается
    let err = decode(&bytes).unwrap_err();
    assert!(matches!(err, ImageError::Png(_)));
}
