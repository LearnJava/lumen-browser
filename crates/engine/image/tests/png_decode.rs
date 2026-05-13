//! Интеграционные тесты PNG-декодера на реальных файлах.
//!
//! Фикстуры в `tests/fixtures/` генерируются Python-скриптом
//! (см. README шаги для воспроизведения), сжимаются стандартным zlib,
//! без сторонних кодеров. Проверяем: размеры, формат пикселя, точное
//! содержимое пикселей. Это «синтетика не заменяет реальность» из
//! CLAUDE.md (§Tests-first) применённое к декодеру изображений.

use lumen_image::{PixelFormat, decode_png};

const RGB_3X2: &[u8] = include_bytes!("fixtures/rgb8_3x2.png");
const RGBA_4X2: &[u8] = include_bytes!("fixtures/rgba8_4x2.png");
const GRAY_4X4: &[u8] = include_bytes!("fixtures/gray8_4x4.png");
const GRAYA_2X2: &[u8] = include_bytes!("fixtures/graya8_2x2.png");
const RGB_FILTERS_2X4: &[u8] = include_bytes!("fixtures/rgb8_filters_2x4.png");
const RGB_PAETH_2X2: &[u8] = include_bytes!("fixtures/rgb8_paeth_2x2.png");

#[test]
fn decode_rgb8_3x2() {
    let img = decode_png(RGB_3X2).unwrap();
    assert_eq!(img.width, 3);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 0, 0, 255, 0, 0, 0, 255, // row 0: red, green, blue
            0, 0, 0, 255, 255, 255, 255, 255, 0, // row 1: black, white, yellow
        ]
    );
}

#[test]
fn decode_rgba8_4x2() {
    let img = decode_png(RGBA_4X2).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255,
            0, 255, 255, 255, 255, 0, 255, 255, 255, 255, 0, 255, 128, 128, 128, 128,
        ]
    );
}

#[test]
fn decode_gray8_4x4() {
    let img = decode_png(GRAY_4X4).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 4);
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(
        img.data,
        vec![
            0, 32, 64, 96, 128, 160, 192, 224, 255, 224, 192, 160, 128, 96, 64, 32,
        ]
    );
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
    // Файл сжат вручную с фильтрами 0, 1, 2, 3 по строкам — проверяет
    // развёртку каждого фильтра конкретно (Python zlib сам фильтры
    // не выбирает, поэтому строки префильтрованы скриптом).
    let img = decode_png(RGB_FILTERS_2X4).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 4);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(
        img.data,
        vec![
            100, 110, 120, 130, 140, 150, // row 0 (filter None)
            50, 60, 70, 80, 90, 100, // row 1 (filter Sub)
            200, 210, 220, 230, 240, 250, // row 2 (filter Up)
            10, 20, 30, 40, 50, 60, // row 3 (filter Avg)
        ]
    );
}

#[test]
fn decode_paeth_filter() {
    let img = decode_png(RGB_PAETH_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(
        img.data,
        vec![
            100, 110, 120, 130, 140, 150, // row 0 (filter None)
            50, 60, 70, 80, 90, 100, // row 1 (filter Paeth)
        ]
    );
}

#[test]
fn decode_rejects_non_png() {
    let bad = b"not a png at all";
    let err = decode_png(bad).unwrap_err();
    assert!(matches!(err, lumen_image::DecodeError::InvalidSignature));
}

#[test]
fn decode_rejects_truncated_file() {
    // Берём валидный заголовок, но обрезаем перед IEND.
    let truncated = &RGB_3X2[..RGB_3X2.len() - 16];
    let err = decode_png(truncated).unwrap_err();
    // В зависимости от того, на каком месте обрезание попало, это либо
    // UnexpectedEof, либо NoEndChunk.
    assert!(matches!(
        err,
        lumen_image::DecodeError::UnexpectedEof
            | lumen_image::DecodeError::NoEndChunk
            | lumen_image::DecodeError::BadCrc { .. }
    ));
}

#[test]
fn decode_rejects_corrupted_crc() {
    let mut corrupted = RGB_3X2.to_vec();
    // Портим один байт в данных IHDR (после 8-байтовой сигнатуры
    // и 4-байтовой длины + 4-байтового типа).
    corrupted[16] ^= 0xFF;
    let err = decode_png(&corrupted).unwrap_err();
    assert!(matches!(
        err,
        lumen_image::DecodeError::BadCrc { .. }
            | lumen_image::DecodeError::BadIhdr(_)
    ));
}
