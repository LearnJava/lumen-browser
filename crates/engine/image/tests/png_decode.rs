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
fn decode_gray1bit_8x2() {
    // 1-bit grayscale: 0→0, 1→255. Чередующиеся пиксели по строкам.
    let img = decode_png(GRAY1BIT_8X2).unwrap();
    assert_eq!(img.width, 8);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 255, 0, 255, 0, 255, 0, // row 0
            0, 255, 0, 255, 0, 255, 0, 255, // row 1
        ]
    );
}

#[test]
fn decode_gray2bit_4x2() {
    // 2-bit grayscale: 0/1/2/3 → 0/85/170/255 (множитель 85).
    let img = decode_png(GRAY2BIT_4X2).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(
        img.data,
        vec![
            0, 85, 170, 255, // row 0: [0,1,2,3]
            255, 170, 85, 0, // row 1: [3,2,1,0]
        ]
    );
}

#[test]
fn decode_gray4bit_3x2_with_trailing_padding() {
    // 4-bit grayscale: 0/8/15 → 0/136/255 (множитель 17). width=3 → trailing
    // nibble в последнем байте каждой строки игнорируется.
    let img = decode_png(GRAY4BIT_3X2).unwrap();
    assert_eq!(img.width, 3);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(
        img.data,
        vec![
            0, 136, 255, // row 0: [0,8,15]
            255, 136, 0, // row 1: [15,8,0]
        ]
    );
}

#[test]
fn decode_palette1bit_8x1() {
    // 1-bit palette: 2-цветная (black/white). Индексы 1/0 чередуются.
    let img = decode_png(PALETTE1BIT_8X1).unwrap();
    assert_eq!(img.width, 8);
    assert_eq!(img.height, 1);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(
        img.data,
        vec![
            255, 255, 255, // index 1 = white
            0, 0, 0, // index 0 = black
            255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0,
        ]
    );
}

#[test]
fn decode_palette4bit_with_trns_4x2() {
    // 4-bit palette (4 entries из 16 возможных), tRNS = [255,255,255,0].
    // Index 3 → grey прозрачный (alpha=0).
    let img = decode_png(PALETTE4BIT_TRNS_4X2).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 0, // row 0
            128, 128, 128, 0, 0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, // row 1
        ]
    );
}

#[test]
fn decode_gray16_2x2_downsamples_to_gray8() {
    // 16-bit grayscale, big-endian u16. Сэмплы:
    // row0: 0x0000, 0x8080 → high byte 0, 0x80
    // row1: 0xFFFF, 0x4040 → high byte 0xFF, 0x40
    let img = decode_png(GRAY16_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![0, 0x80, 0xFF, 0x40]);
}

#[test]
fn decode_graya16_2x2_downsamples_to_graya8() {
    // 16-bit GrayAlpha. Пары (gray, alpha) big-endian.
    // row0: (0xFFFF,0xFFFF), (0x8080,0x4040) → 255,255, 128,64
    // row1: (0x0000,0x8080), (0xC0C0,0x0000) → 0,128, 192,0
    let img = decode_png(GRAYA16_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::GrayAlpha8);
    assert_eq!(img.data, vec![255, 255, 128, 64, 0, 128, 192, 0]);
}

#[test]
fn decode_rgb16_2x2_downsamples_to_rgb8() {
    // 16-bit RGB. row0: red, green; row1: mid grey, blue-purple
    let img = decode_png(RGB16_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 0, 0, 255, 0, // row 0: red, green
            128, 128, 128, 64, 192, 255, // row 1: grey, purple-blue
        ]
    );
}

#[test]
fn decode_rgba16_2x2_downsamples_to_rgba8() {
    // 16-bit RGBA. row0: red opaque, green half; row1: blue transparent, purple
    let img = decode_png(RGBA16_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 0, 255, 0, 255, 0, 128, // row 0
            0, 0, 255, 0, 192, 64, 128, 64, // row 1
        ]
    );
}

#[test]
fn decode_gray16_with_filters_2x3() {
    // 16-bit grayscale c фильтрами None / Sub / Up — проверяет, что
    // filter_bpp=2 (channels=1, bit_depth=16 → 16/8=2) корректно
    // обрабатывается развёрткой фильтров до downsample-а.
    // Expected (после high-byte): row0 [0,255], row1 [128,64], row2 [192,64]
    let img = decode_png(GRAY16_FILTERS_2X3).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 3);
    assert_eq!(img.format, PixelFormat::Gray8);
    assert_eq!(img.data, vec![0, 255, 128, 64, 192, 64]);
}

#[test]
fn decode_gray8_with_trns_yields_grayalpha8() {
    // 8-bit grayscale + tRNS=0 (черный — прозрачный).
    // row0: [0, 128, 255], row1: [0, 0, 255]
    let img = decode_png(GRAY8_TRNS_3X2).unwrap();
    assert_eq!(img.width, 3);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::GrayAlpha8);
    assert_eq!(
        img.data,
        vec![
            0, 0, 128, 255, 255, 255, // row 0: black transparent, mid opaque, white opaque
            0, 0, 0, 0, 255, 255, // row 1: two black transparent, white opaque
        ]
    );
}

#[test]
fn decode_rgb8_with_trns_yields_rgba8() {
    // 8-bit RGB + tRNS=(255,0,255) — magenta прозрачный.
    let img = decode_png(RGB8_TRNS_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(
        img.data,
        vec![
            255, 0, 0, 255, // red opaque
            255, 0, 255, 0, // magenta transparent
            255, 0, 255, 0, // magenta transparent
            255, 255, 255, 255, // white opaque
        ]
    );
}

#[test]
fn decode_gray16_with_trns_yields_grayalpha8() {
    // 16-bit grayscale + tRNS=0xFFFF. После downsample: 0,255,128,255;
    // tRNS normalized: 0xFFFF → 0xFF = 255.
    let img = decode_png(GRAY16_TRNS_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::GrayAlpha8);
    assert_eq!(
        img.data,
        vec![
            0, 255, 255, 0, // black opaque, white transparent
            128, 255, 255, 0, // mid opaque, white transparent
        ]
    );
}

#[test]
fn decode_rgb16_with_trns_yields_rgba8() {
    // 16-bit RGB + tRNS=(0xFFFF,0xFFFF,0xFFFF) = white transparent.
    let img = decode_png(RGB16_TRNS_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgba8);
    assert_eq!(
        img.data,
        vec![
            255, 255, 255, 0, 255, 0, 0, 255, // row 0: white transparent, red opaque
            255, 255, 255, 0, 0, 255, 0, 255, // row 1: white transparent, green opaque
        ]
    );
}

#[test]
fn decode_adam7_rgb_8x8() {
    // 8x8 RGB, pixel(col,row) = (col*32, row*32, 128).
    // Все 7 passes должны корректно собраться в исходное изображение.
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
    // 5x5 grayscale, pixel(col,row) = col*40 + row*8.
    // Нечётный размер: некоторые passes имеют ph=0.
    let img = decode_png(ADAM7_GRAY_5X5).unwrap();
    assert_eq!(img.width, 5);
    assert_eq!(img.height, 5);
    assert_eq!(img.format, PixelFormat::Gray8);
    for row in 0..5u32 {
        for col in 0..5u32 {
            let expected = (col * 40 + row * 8) as u8;
            assert_eq!(
                img.data[(row * 5 + col) as usize],
                expected,
                "row {row} col {col}"
            );
        }
    }
}

#[test]
fn decode_adam7_rgba_4x4() {
    // 4x4 RGBA — проверяет 4-байтовый bpp в Adam7-сборке.
    let img = decode_png(ADAM7_RGBA_4X4).unwrap();
    assert_eq!(img.width, 4);
    assert_eq!(img.height, 4);
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
    // Минимальный кейс: 1x1 RGB. Только pass 1 имеет 1 пиксель;
    // остальные 6 passes пустые → skip-логика проверяется.
    let img = decode_png(ADAM7_RGB_1X1).unwrap();
    assert_eq!(img.width, 1);
    assert_eq!(img.height, 1);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(img.data, vec![255, 127, 0]);
}

#[test]
fn decode_adam7_rgb_2x2() {
    // 2x2 RGB. Passes:
    //   1 (0,0)=(10,20,30), 6 (1,0)=(40,50,60), 7 (0,1) (1,1)=(70,80,90)(100,110,120)
    let img = decode_png(ADAM7_RGB_2X2).unwrap();
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(img.format, PixelFormat::Rgb8);
    assert_eq!(
        img.data,
        vec![
            10, 20, 30, 40, 50, 60, // row 0
            70, 80, 90, 100, 110, 120, // row 1
        ]
    );
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
