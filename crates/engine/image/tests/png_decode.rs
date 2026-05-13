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
const PALETTE_4X2: &[u8] = include_bytes!("fixtures/palette8_4x2.png");
const PALETTE_TRNS_3X3: &[u8] = include_bytes!("fixtures/palette8_trns_3x3.png");
const PALETTE_PARTIAL_TRNS_2X2: &[u8] = include_bytes!("fixtures/palette8_partial_trns_2x2.png");
const GRAYSCALE_WITH_PLTE_2X1: &[u8] = include_bytes!("fixtures/grayscale_with_plte_2x1.png");

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

#[test]
fn decode_palette_without_trns_yields_rgb8() {
    // 4×2 палитра из [red, green, blue, white], индексы [0,1,2,3, 3,2,1,0].
    let img = decode_png(PALETTE_4X2).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, // row 0: r,g,b,w
            255, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, // row 1: w,b,g,r
        ]
    );
}

#[test]
fn decode_palette_with_full_trns_yields_rgba8() {
    // 3×3 палитра, tRNS = [0, 128, 255] — entry 0 прозрачен, 2 непрозрачен.
    let img = decode_png(PALETTE_TRNS_3X3).unwrap();
    assert_eq!(img.width, 3);
    assert_eq!(img.height, 3);
    assert_eq!(img.format, PixelFormat::Rgba8);
    // Палитра: 0=(200,100,50), 1=(50,100,200), 2=(100,200,50)
    // Alpha:    0=0,           1=128,           2=255
    // Индексы:  [0,1,2, 1,2,0, 2,0,1]
    assert_eq!(
        img.data,
        vec![
            200, 100, 50, 0, 50, 100, 200, 128, 100, 200, 50, 255, // row 0
            50, 100, 200, 128, 100, 200, 50, 255, 200, 100, 50, 0, // row 1
            100, 200, 50, 255, 200, 100, 50, 0, 50, 100, 200, 128, // row 2
        ]
    );
}

#[test]
fn decode_palette_with_partial_trns_pads_remaining_entries_with_255() {
    // tRNS длиной 1: только для первого entry. Остальные должны получить
    // alpha=255 (opaque) — это семантика PNG §11.3.2 для коротких tRNS.
    let img = decode_png(PALETTE_PARTIAL_TRNS_2X2).unwrap();
    assert_eq!(img.format, PixelFormat::Rgba8);
    // Палитра: 0=(255,255,255), 1=(128,128,128), 2=(0,0,0)
    // Alpha:    0=42,            1=255,           2=255
    // Индексы:  [0,1, 1,2]
    assert_eq!(
        img.data,
        vec![
            255, 255, 255, 42, 128, 128, 128, 255, // row 0
            128, 128, 128, 255, 0, 0, 0, 255, // row 1
        ]
    );
}

#[test]
fn decode_rejects_plte_on_grayscale() {
    // PNG §11.3.2: PLTE не должен присутствовать при color_type 0 / 4.
    let err = decode_png(GRAYSCALE_WITH_PLTE_2X1).unwrap_err();
    assert!(matches!(
        err,
        lumen_image::DecodeError::BadPalette(
            lumen_image::PaletteError::UnexpectedForGrayscale
        )
    ));
}

#[test]
fn decode_palette_rejects_missing_plte() {
    // Берём настоящий палитровый PNG и вырезаем PLTE-чанк целиком.
    // PLTE идёт сразу после IHDR: 8 байт сигнатуры + 25 байт IHDR-чанка
    // (4 length + 4 type + 13 data + 4 crc) = 33. PLTE-чанк: 4 length +
    // 4 type + 12 data (4 entries × RGB) + 4 crc = 24 байта.
    let mut stream = PALETTE_4X2.to_vec();
    let plte_start = 33;
    let plte_chunk_len = 4 + 4 + 12 + 4;
    stream.drain(plte_start..plte_start + plte_chunk_len);
    let err = decode_png(&stream).unwrap_err();
    assert!(matches!(
        err,
        lumen_image::DecodeError::BadPalette(
            lumen_image::PaletteError::MissingForIndexed
        )
    ));
}
