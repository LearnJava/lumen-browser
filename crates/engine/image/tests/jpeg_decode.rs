//! Интеграционные тесты для собственного JPEG-декодера.
//!
//! Фикстуры сгенерированы ImageMagick (8-bit JPEG, baseline DCT):
//! - `gray_*_16x16.jpg` — grayscale (1 component), для проверки Y-only path-а.
//! - `rgb_444_*.jpg` — 4:4:4 chroma subsampling (без subsampling-а).
//! - `rgb_420_*.jpg` — 4:2:0 (стандартный для web JPEG).
//!
//! JPEG — lossy, поэтому проверки на значения пикселей идут с допусками
//! ±5..10 от ожидаемого.

use lumen_image::{decode_jpeg, JpegError, PixelFormat};

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
        // R ≈ 255, G/B ≈ 0; JPEG lossy + YCbCr round-trip ⇒ ±15.
        assert!(px[0] > 230, "R={} ожидался ≈255", px[0]);
        assert!(px[1] < 30, "G={} ожидался ≈0", px[1]);
        assert!(px[2] < 30, "B={} ожидался ≈0", px[2]);
    }
}

#[test]
fn decode_solid_blue_4_2_0_with_chroma_subsampling() {
    let image = decode_jpeg(BLUE_420).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        // (0, 128, 255) → ±15 после round-trip.
        assert!(px[0] < 30, "R={} ожидался ≈0", px[0]);
        assert!(
            (110..=150).contains(&px[1]),
            "G={} ожидался ≈128",
            px[1]
        );
        assert!(px[2] > 230, "B={} ожидался ≈255", px[2]);
    }
}

#[test]
fn decode_grayscale_single_component_yields_gray8() {
    let image = decode_jpeg(GRAY_DARK).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Gray8);
    assert_eq!(image.data.len(), 16 * 16);
    // Чёрный → значения ≈ 0.
    for &v in &image.data {
        assert!(v < 20, "ожидалось ~0, получено {v}");
    }
}

#[test]
fn decode_grayscale_light_value() {
    let image = decode_jpeg(GRAY_LIGHT).unwrap();
    assert_eq!(image.format, PixelFormat::Gray8);
    // Светло-серый rgb(200,200,200) → значение Y ≈ 200.
    for &v in &image.data {
        assert!((185..=215).contains(&v), "ожидалось ~200, получено {v}");
    }
}

#[test]
fn empty_input_fails_cleanly() {
    let err = decode_jpeg(&[]).unwrap_err();
    assert_eq!(err, JpegError::NoSoi);
}

#[test]
fn random_garbage_fails_cleanly() {
    let bytes = [0xDE, 0xAD, 0xBE, 0xEF];
    let err = decode_jpeg(&bytes).unwrap_err();
    assert_eq!(err, JpegError::NoSoi);
}

#[test]
fn truncated_after_soi_fails_cleanly() {
    let bytes = [0xFF, 0xD8];
    let err = decode_jpeg(&bytes).unwrap_err();
    assert_eq!(err, JpegError::UnexpectedEof);
}

#[test]
fn decode_solid_green_4_2_2_horizontal_subsampling() {
    let image = decode_jpeg(GREEN_422).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        // (0, 200, 0).
        assert!(px[0] < 30);
        assert!((180..=220).contains(&px[1]), "G={} ожидался ≈200", px[1]);
        assert!(px[2] < 30);
    }
}

#[test]
fn decode_non_mcu_aligned_dimensions_17x9() {
    // 17×9 не кратно 16×16 MCU (4:2:0): декодер читает 32×16 пикселей,
    // обрезает до 17×9 при сборке.
    let image = decode_jpeg(PURPLE_NONALIGNED).unwrap();
    assert_eq!(image.width, 17);
    assert_eq!(image.height, 9);
    assert_eq!(image.format, PixelFormat::Rgb8);
    assert_eq!(image.data.len(), 17 * 9 * 3);
    // rgb(150, 50, 200) — ±20 после YCbCr lossy round-trip.
    for px in image.data.chunks_exact(3) {
        assert!((130..=170).contains(&px[0]), "R={} ожидался ≈150", px[0]);
        assert!((30..=70).contains(&px[1]), "G={} ожидался ≈50", px[1]);
        assert!((180..=220).contains(&px[2]), "B={} ожидался ≈200", px[2]);
    }
}

#[test]
fn decode_grayscale_vertical_gradient_monotonic() {
    // 32×32 grayscale вертикальный градиент: верх — чёрный, низ — белый.
    let image = decode_jpeg(GRADIENT_V_32).unwrap();
    assert_eq!(image.format, PixelFormat::Gray8);
    assert_eq!(image.width, 32);
    assert_eq!(image.height, 32);
    // Верхняя строка ≈ 0..30, нижняя ≈ 220..255.
    let top: u32 = image.data[..32].iter().map(|&v| u32::from(v)).sum();
    let bottom: u32 = image.data[32 * 31..32 * 32].iter().map(|&v| u32::from(v)).sum();
    assert!(top / 32 < 30, "top mean = {}, ожидалось <30", top / 32);
    assert!(bottom / 32 > 220, "bottom mean = {}, ожидалось >220", bottom / 32);
    // Средняя строка ≈ 128 ± 30.
    let middle: u32 = image.data[32 * 16..32 * 17].iter().map(|&v| u32::from(v)).sum();
    assert!(
        (98..=158).contains(&(middle / 32)),
        "middle mean = {}, ожидалось ≈128",
        middle / 32
    );
}

#[test]
fn decode_color_gradient_red_to_blue() {
    // 32×16 RGB-градиент R→B. ImageMagick gradient: idёт сверху вниз,
    // поэтому проверяем top vs bottom row, не left vs right.
    let image = decode_jpeg(GRADIENT_RB).unwrap();
    assert_eq!(image.format, PixelFormat::Rgb8);
    assert_eq!(image.width, 32);
    assert_eq!(image.height, 16);
    let row_bytes = 32 * 3;
    let top_first = &image.data[..3];
    let bottom_last = &image.data[15 * row_bytes + 31 * 3..15 * row_bytes + 32 * 3];
    assert!(
        top_first[0] > 200 && top_first[2] < 50,
        "top = {top_first:?}, ожидался ≈красный"
    );
    assert!(
        bottom_last[2] > 200 && bottom_last[0] < 50,
        "bottom = {bottom_last:?}, ожидался ≈синий"
    );
}

#[test]
fn decode_jpeg_with_restart_interval_24x24() {
    // Файл создан с restart-interval = 2 MCU, поэтому RST0..RST7 встречаются
    // внутри scan-данных. Декодер должен правильно сбрасывать DC predictors
    // и пройти через все маркеры.
    let image = decode_jpeg(RESTART_24).unwrap();
    assert_eq!(image.width, 24);
    assert_eq!(image.height, 24);
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        // rgb(100,150,200) ± 25.
        assert!((75..=125).contains(&px[0]), "R={} ожидался ≈100", px[0]);
        assert!((125..=175).contains(&px[1]), "G={} ожидался ≈150", px[1]);
        assert!((175..=225).contains(&px[2]), "B={} ожидался ≈200", px[2]);
    }
}

#[test]
fn lossless_jpeg_marker_still_rejected() {
    // SOI + SOF3 (lossless) — не поддерживаем, в отличие от SOF2.
    let bytes = [
        0xFF, 0xD8, // SOI
        0xFF, 0xC3, // SOF3 — lossless
        0x00, 0x0B, // length 11
        0x08, 0x00, 0x08, 0x00, 0x08, // P=8, Y=8, X=8
        0x01, 0x01, 0x11, 0x00, // Nf=1, component
    ];
    let err = decode_jpeg(&bytes).unwrap_err();
    assert_eq!(err, JpegError::UnsupportedSof(0xC3));
}

#[test]
fn decode_progressive_solid_red_444() {
    // SOF2 + 4:4:4 sampling. Multi-scan: DC initial → AC initial spectral splits → AC refinements.
    let image = decode_jpeg(PROG_RED_444).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        assert!(px[0] > 180, "R={} ожидался ≈200", px[0]);
        assert!(px[1] < 70, "G={} ожидался ≈50", px[1]);
        assert!(px[2] < 70, "B={} ожидался ≈50", px[2]);
    }
}

#[test]
fn decode_progressive_solid_blue_420_with_subsampling() {
    // SOF2 + 4:2:0: интерливед DC scan покрывает все 3 компонента, далее non-interleaved AC.
    let image = decode_jpeg(PROG_BLUE_420).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Rgb8);
    for px in image.data.chunks_exact(3) {
        assert!(px[0] < 80, "R={} ожидался ≈50", px[0]);
        assert!(px[1] < 80, "G={} ожидался ≈50", px[1]);
        assert!(px[2] > 180, "B={} ожидался ≈200", px[2]);
    }
}

#[test]
fn decode_progressive_grayscale() {
    let image = decode_jpeg(PROG_GRAY).unwrap();
    assert_eq!(image.width, 16);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Gray8);
    // gray60 у ImageMagick — sRGB-нелинейное значение, ImageMagick конвертирует
    // в linear после команды. Допускаем широкий диапазон.
    let mean: u32 = image.data.iter().map(|&v| u32::from(v)).sum::<u32>() / (16 * 16);
    assert!(
        (60..=190).contains(&mean),
        "среднее = {mean}, ожидался mid-range grayscale"
    );
}

#[test]
fn decode_progressive_gradient_grayscale_is_monotonic() {
    // Чёрно-белый вертикальный градиент — проверяет, что AC scans (включая
    // refinement) корректно вносят high-frequency информацию.
    let image = decode_jpeg(PROG_GRAD_V_32).unwrap();
    assert_eq!(image.width, 32);
    assert_eq!(image.height, 32);
    assert_eq!(image.format, PixelFormat::Gray8);
    let top: u32 = image.data[..32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    let bottom: u32 =
        image.data[32 * 31..32 * 32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    assert!(top < 30, "top mean = {top}, ожидалось <30 (чёрный)");
    assert!(bottom > 220, "bottom mean = {bottom}, ожидалось >220 (белый)");
}

#[test]
fn decode_progressive_with_dc_refinement_via_jpegtran() {
    // Файл собран `jpegtran -progressive -scans` со всеми четырьмя типами scan:
    // DC initial → DC refine ×2 → AC initial → AC refine ×2. ImageMagick
    // default-progressive выпускает только DC initial и AC initial+refine, но не
    // DC refinement. Этот тест прокручивает оба недостающих path-а.
    let image = decode_jpeg(PROG_DC_REFINE).unwrap();
    assert_eq!(image.width, 32);
    assert_eq!(image.height, 32);
    assert_eq!(image.format, PixelFormat::Gray8);
    // Вертикальный градиент black-white: верх ~0, низ ~255.
    let top: u32 = image.data[..32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    let bottom: u32 =
        image.data[32 * 31..32 * 32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    assert!(top < 35, "top mean = {top}");
    assert!(bottom > 220, "bottom mean = {bottom}");
}

#[test]
fn decode_progressive_gradient_red_blue() {
    // 32×16 RGB gradient + 4:2:0 + multi-scan progressive.
    let image = decode_jpeg(PROG_GRAD_RB).unwrap();
    assert_eq!(image.width, 32);
    assert_eq!(image.height, 16);
    assert_eq!(image.format, PixelFormat::Rgb8);
    let row_bytes = 32 * 3;
    let top_first = &image.data[..3];
    let bottom_last = &image.data[15 * row_bytes + 31 * 3..15 * row_bytes + 32 * 3];
    assert!(
        top_first[0] > 200 && top_first[2] < 50,
        "top = {top_first:?}, ожидался ≈красный"
    );
    assert!(
        bottom_last[2] > 200 && bottom_last[0] < 50,
        "bottom = {bottom_last:?}, ожидался ≈синий"
    );
}
