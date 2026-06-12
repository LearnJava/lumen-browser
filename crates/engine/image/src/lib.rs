mod jpeg;
mod png;
pub mod webp;
mod gif;
pub mod avif;
pub mod jxl;
pub mod heic;
pub mod decode_cache;

pub use decode_cache::{ImageDecodeCache, ImageHandle, ImageKey};
pub use jpeg::{decode_jpeg, JpegError};
pub use png::{decode_png, encode_png_rgba8};
pub use webp::{WebpError, WebpImageDecoder, decode_webp, is_webp};
pub use gif::{decode_gif, decode_gif_animated, AnimatedFrame, AnimatedGif, GifError, GifLoopCount, is_gif};
pub use avif::{AvifError, AvifImageDecoder, decode_avif, is_avif};
pub use jxl::{JxlError, decode_jxl, is_jxl};
pub use heic::{HeicError, decode_heic, is_heic};

/// PNG-сигнатура: `89 50 4E 47 0D 0A 1A 0A` (PNG §5.2).
pub const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// JPEG SOI + начало следующего маркера: `FF D8 FF`.
pub const JPEG_SIGNATURE_PREFIX: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// MIME-типы изображений, которые `decode` умеет декодировать.
///
/// Передаётся в `PictureParams::supported_types` через `lumen-layout`, чтобы
/// неподдерживаемые `<source type="...">` пропускались picker-ом и браузер
/// выбирал подходящий fallback вместо пустой коробки.
///
/// Содержит только форматы, которые реально декодируются в готовые пиксели.
/// `image/jxl` / `image/heic` / `image/heif` НЕ входят: их декодеры — заглушки
/// (`decode_jxl` / `decode_heic` всегда возвращают `Err`), поэтому объявлять их
/// поддерживаемыми означало бы заставить picker выбрать такой `<source>` и
/// показать пустую коробку — ровно то, что эта функция призвана предотвратить.
/// `image/avif` остаётся: декодер настоящий, лишь за feature-флагом `avif`.
#[must_use]
pub fn supported_mime_types() -> &'static [&'static str] {
    &["image/png", "image/jpeg", "image/jpg", "image/gif", "image/webp", "image/avif"]
}

/// Декодирует растровое изображение по сигнатуре первых байтов.
///
/// # Errors
/// - [`ImageError::UnknownFormat`] — сигнатура не распознана.
/// - [`ImageError::Png`] — PNG-сигнатура совпала, но декодер выдал ошибку.
/// - [`ImageError::Jpeg`] — JPEG-сигнатура совпала, но декодер выдал ошибку.
/// - [`ImageError::Gif`] — GIF-сигнатура (GIF87a/GIF89a) совпала, но декодер выдал ошибку.
/// - [`ImageError::Webp`] — WebP-сигнатура (RIFF/WEBP) совпала, но декодер выдал ошибку.
/// - [`ImageError::Avif`] — AVIF ftyp-бокс обнаружен, но декодирование не удалось.
/// - [`ImageError::Jxl`] — JPEG XL сигнатура обнаружена, но декодирование не поддерживается.
pub fn decode(bytes: &[u8]) -> Result<Image, ImageError> {
    if bytes.len() >= PNG_SIGNATURE.len() && bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE {
        return decode_png(bytes).map_err(ImageError::Png);
    }
    if bytes.len() >= JPEG_SIGNATURE_PREFIX.len()
        && bytes[..JPEG_SIGNATURE_PREFIX.len()] == JPEG_SIGNATURE_PREFIX
    {
        return decode_jpeg(bytes).map_err(ImageError::Jpeg);
    }
    if is_gif(bytes) {
        return decode_gif(bytes).map_err(ImageError::Gif);
    }
    if is_webp(bytes) {
        let (width, height, data) = decode_webp(bytes).map_err(ImageError::Webp)?;
        return Ok(Image { width, height, format: PixelFormat::Rgba8, data, icc_profile: None });
    }
    if is_avif(bytes) {
        let (width, height, data) = decode_avif(bytes).map_err(ImageError::Avif)?;
        return Ok(Image { width, height, format: PixelFormat::Rgba8, data, icc_profile: None });
    }
    if is_jxl(bytes) {
        return Err(ImageError::Jxl(decode_jxl(bytes).unwrap_err()));
    }
    if is_heic(bytes) {
        return Err(ImageError::Heic(decode_heic(bytes).unwrap_err()));
    }
    Err(ImageError::UnknownFormat)
}

/// Ошибка `decode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Сигнатура не совпала ни с одним известным форматом.
    UnknownFormat,
    Png(DecodeError),
    Jpeg(JpegError),
    /// WebP-контейнер распознан (RIFF/WEBP), но декодирование не удалось.
    Webp(WebpError),
    /// GIF-сигнатура распознана (GIF87a/GIF89a), но декодирование не удалось.
    Gif(GifError),
    /// AVIF ftyp-бокс обнаружен (brand=avif/avis), но декодирование не удалось.
    Avif(AvifError),
    /// JPEG XL сигнатура распознана, но декодирование не поддерживается (Phase 0).
    Jxl(JxlError),
    /// HEIC/HEIF ftyp-бокс обнаружен, но декодирование не поддерживается (Phase 1).
    Heic(HeicError),
}

impl core::fmt::Display for ImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownFormat => write!(f, "формат изображения не распознан по сигнатуре"),
            Self::Png(e) => write!(f, "PNG: {e}"),
            Self::Jpeg(e) => write!(f, "JPEG: {e}"),
            Self::Webp(e) => write!(f, "WebP: {e}"),
            Self::Gif(e) => write!(f, "GIF: {e}"),
            Self::Avif(e) => write!(f, "AVIF: {e}"),
            Self::Jxl(e) => write!(f, "JPEG XL: {e}"),
            Self::Heic(e) => write!(f, "HEIC/HEIF: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl From<DecodeError> for ImageError {
    fn from(e: DecodeError) -> Self { Self::Png(e) }
}

impl From<JpegError> for ImageError {
    fn from(e: JpegError) -> Self { Self::Jpeg(e) }
}

impl From<WebpError> for ImageError {
    fn from(e: WebpError) -> Self { Self::Webp(e) }
}

impl From<GifError> for ImageError {
    fn from(e: GifError) -> Self { Self::Gif(e) }
}

impl From<AvifError> for ImageError {
    fn from(e: AvifError) -> Self { Self::Avif(e) }
}

impl From<JxlError> for ImageError {
    fn from(e: JxlError) -> Self { Self::Jxl(e) }
}

impl From<HeicError> for ImageError {
    fn from(e: HeicError) -> Self { Self::Heic(e) }
}

/// Идентифицированный цветовой охват ICC профиля.
///
/// Используется для выбора матрицы конвертации при загрузке изображения в GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IccGamut {
    /// Профиль опознан как sRGB — конвертация не нужна.
    Srgb,
    /// DCI-P3 / Display P3 (D65) — применяется P3→sRGB матрица.
    DisplayP3,
    /// ITU-R BT.2020 / Rec. 2020 — применяется Rec2020→sRGB матрица.
    Rec2020,
    /// Профиль не опознан — конвертация пропускается.
    Unknown,
}

/// ICC профиль изображения (опциональный).
///
/// Содержит сырые данные ICC профиля.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IccProfile {
    /// Сырые байты ICC профиля.
    pub data: Vec<u8>,
}

impl IccProfile {
    /// Проверяет минимальный размер ICC профиля (128 байт).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.data.len() >= 128
    }

    /// Определяет цветовой охват по сигнатуре пространства данных (bytes 16-19)
    /// и описанию профиля (сканирование ASCII-байт).
    ///
    /// Практичная эвристика — не требует полного ICC CMM. Работает для реальных
    /// Display P3 и Rec2020 изображений с Apple/Adobe профилями.
    #[must_use]
    pub fn detect_gamut(&self) -> IccGamut {
        if self.data.len() < 20 {
            return IccGamut::Unknown;
        }
        // Bytes 16-19: color space of data (e.g., b"RGB " / b"GRAY" / b"CMYK").
        // Only RGB profiles need gamut conversion.
        if &self.data[16..20] != b"RGB " {
            return IccGamut::Unknown;
        }

        // Fast path: check the profile description tag by scanning raw bytes
        // for known wide-gamut marker strings. ICC desc data follows the 128-byte
        // header + tag table, but scanning raw bytes is equally reliable for
        // well-formed standard profiles without parsing the full tag directory.
        let haystack = &self.data;

        if contains_ascii(haystack, b"Display P3")
            || contains_ascii(haystack, b"DCI-P3")
            || contains_ascii(haystack, b"DCI P3")
            || contains_utf16be(haystack, "Display P3")
            || contains_utf16be(haystack, "DCI-P3")
        {
            return IccGamut::DisplayP3;
        }

        if contains_ascii(haystack, b"Rec. 2020")
            || contains_ascii(haystack, b"Rec2020")
            || contains_ascii(haystack, b"BT.2020")
            || contains_ascii(haystack, b"BT2020")
            || contains_utf16be(haystack, "Rec. 2020")
            || contains_utf16be(haystack, "Rec2020")
            || contains_utf16be(haystack, "BT.2020")
        {
            return IccGamut::Rec2020;
        }

        // sRGB profiles explicitly labelled.
        if contains_ascii(haystack, b"sRGB") || contains_ascii(haystack, b"IEC 61966") {
            return IccGamut::Srgb;
        }

        IccGamut::Unknown
    }
}

/// Returns true if `haystack` contains `needle` as a contiguous byte slice.
fn contains_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Returns true if `haystack` contains `text` encoded as big-endian UTF-16.
fn contains_utf16be(haystack: &[u8], text: &str) -> bool {
    let needle: Vec<u8> = text.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
    if needle.is_empty() { return false; }
    haystack.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// Применяет ICC-коррекцию к RGBA8 пикселям in-place.
///
/// Конвертирует Display P3 или Rec2020 пиксели в sRGB для корректного отображения
/// на sRGB-мониторах. Для `IccGamut::Srgb` и `IccGamut::Unknown` — no-op.
pub fn correct_rgba_pixels(rgba: &mut [u8], profile: &IccProfile) {
    match profile.detect_gamut() {
        IccGamut::DisplayP3 => convert_pixels_to_srgb(rgba, p3_to_srgb_pixel),
        IccGamut::Rec2020 => convert_pixels_to_srgb(rgba, rec2020_to_srgb_pixel),
        IccGamut::Srgb | IccGamut::Unknown => {}
    }
}

fn convert_pixels_to_srgb(rgba: &mut [u8], converter: fn(f32, f32, f32) -> (f32, f32, f32)) {
    for pixel in rgba.chunks_exact_mut(4) {
        let r = pixel[0] as f32 / 255.0;
        let g = pixel[1] as f32 / 255.0;
        let b = pixel[2] as f32 / 255.0;
        let (sr, sg, sb) = converter(r, g, b);
        pixel[0] = (sr.clamp(0.0, 1.0) * 255.0).round() as u8;
        pixel[1] = (sg.clamp(0.0, 1.0) * 255.0).round() as u8;
        pixel[2] = (sb.clamp(0.0, 1.0) * 255.0).round() as u8;
        // alpha channel unchanged
    }
}

/// Display P3 gamma-encoded → sRGB gamma-encoded (per-pixel).
/// Decode P3 gamma → linear P3 → linear sRGB → encode sRGB gamma.
fn p3_to_srgb_pixel(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let lr = srgb_gamma_decode(r);
    let lg = srgb_gamma_decode(g);
    let lb = srgb_gamma_decode(b);
    let (sr, sg, sb) = p3_linear_to_srgb_linear(lr, lg, lb);
    (srgb_gamma_encode(sr), srgb_gamma_encode(sg), srgb_gamma_encode(sb))
}

/// Rec2020 gamma-encoded → sRGB gamma-encoded (per-pixel).
fn rec2020_to_srgb_pixel(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let lr = rec2020_gamma_decode(r);
    let lg = rec2020_gamma_decode(g);
    let lb = rec2020_gamma_decode(b);
    let (sr, sg, sb) = rec2020_linear_to_srgb_linear(lr, lg, lb);
    (srgb_gamma_encode(sr), srgb_gamma_encode(sg), srgb_gamma_encode(sb))
}

/// Display P3 linear → sRGB linear (CSS Color L4 §10.9 matrix).
fn p3_linear_to_srgb_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let sr =  1.224_94  * r - 0.224_94  * g;
    let sg = -0.042_076 * r + 1.042_076 * g;
    let sb = -0.019_692 * r - 0.078_654 * g + 1.098_346 * b;
    (sr, sg, sb)
}

/// Rec2020 linear → sRGB linear (CSS Color L4 §10.9 matrix).
fn rec2020_linear_to_srgb_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let sr =  1.660_491 * r - 0.587_641 * g - 0.072_85  * b;
    let sg = -0.124_551 * r + 1.132_9   * g - 0.008_35  * b;
    let sb = -0.018_151 * r - 0.100_578 * g + 1.118_73  * b;
    (sr, sg, sb)
}

/// sRGB / P3 gamma decode: encoded → linear (IEC 61966-2-1).
fn srgb_gamma_decode(c: f32) -> f32 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// sRGB gamma encode: linear → encoded.
fn srgb_gamma_encode(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Rec2020 (BT.2020 OETF) gamma decode: encoded → linear.
fn rec2020_gamma_decode(c: f32) -> f32 {
    const ALPHA: f32 = 1.099_296_8;
    const BETA: f32 = 0.018_053_97;
    if c < 4.5 * BETA { c / 4.5 } else { ((c + (ALPHA - 1.0)) / ALPHA).powf(1.0 / 0.45) }
}

/// Декодированное растровое изображение в плотной row-major упаковке.
/// Длина `data` равна `width * height * bytes_per_pixel(format)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
    /// Опциональный ICC профиль изображения.
    pub icc_profile: Option<IccProfile>,
}

impl Image {
    /// Детектирует цветовое пространство изображения из ICC профиля или сигнатуры изображения.
    ///
    /// Возвращает `lumen_layout::style::ColorSpace::Srgb` если профиль отсутствует или невалиден.
    /// Для AVIF: в Phase 1 будет использовать `libavif` binding для извлечения ICC профиля.
    pub fn detect_color_space(&self) -> lumen_layout::style::ColorSpace {
        if let Some(ref profile) = self.icc_profile {
            lumen_paint::detect_color_space_from_icc(&profile.data)
        } else {
            lumen_layout::style::ColorSpace::Srgb
        }
    }

    /// Возвращает пиксели в формате RGBA8 (4 байта на пиксель).
    #[must_use]
    pub fn to_rgba8(&self) -> Vec<u8> {
        let pixel_count = self.width as usize * self.height as usize;
        let mut out = Vec::with_capacity(pixel_count * 4);
        match self.format {
            PixelFormat::Gray8 => {
                for &g in &self.data { out.extend_from_slice(&[g, g, g, 255]); }
            }
            PixelFormat::GrayAlpha8 => {
                for pair in self.data.chunks_exact(2) {
                    out.extend_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
                }
            }
            PixelFormat::Rgb8 => {
                for t in self.data.chunks_exact(3) {
                    out.extend_from_slice(&[t[0], t[1], t[2], 255]);
                }
            }
            PixelFormat::Rgba8 => { out.extend_from_slice(&self.data); }
        }
        out
    }

    /// Возвращает пиксели в формате RGBA8 с применением tone-mapping.
    /// Конвертирует Display P3 и Rec2020 изображения в sRGB.
    #[must_use]
    pub fn to_rgba8_tone_mapped(&self) -> Vec<u8> {
        let mut out = self.to_rgba8();
        let color_space = self.detect_color_space();
        apply_tone_mapping(color_space, &mut out);
        out
    }
}

/// Apply tone mapping for a detected color space.
///
/// Converts RGBA8 pixels from Display P3 / Rec2020 to sRGB for standard display.
/// Pixels are expected in RGBA8 format (4 bytes per pixel).
/// Phase 2: Implements pixel-level conversion using color space transformation matrices.
pub fn apply_tone_mapping(color_space: lumen_layout::style::ColorSpace, pixel_data: &mut [u8]) {
    match color_space {
        lumen_layout::style::ColorSpace::Srgb => {
            // No conversion needed for sRGB
        }
        lumen_layout::style::ColorSpace::DisplayP3 => {
            apply_p3_to_srgb(pixel_data);
        }
        lumen_layout::style::ColorSpace::Rec2020 => {
            apply_rec2020_to_srgb(pixel_data);
        }
    }
}

/// Tone-mapping matrix: Display P3 → sRGB (DCI-P3 to ITU-R BT.709).
const MATRIX_P3_TO_SRGB: [[f32; 3]; 3] = [
    [2.493496911, -0.829488387, -0.663963154],
    [-0.829488387, 1.762664402, 0.023807284],
    [-0.663963154, 0.023807284, 0.940628674],
];

/// Tone-mapping matrix: Rec. 2020 → sRGB (ITU-R BT.2020 to ITU-R BT.709).
const MATRIX_REC2020_TO_SRGB: [[f32; 3]; 3] = [
    [1.716651294, -0.355670783, -0.253365395],
    [-0.666684351, 1.616481667, 0.015768773],
    [-0.253365395, 0.015768773, 1.193313670],
];

fn apply_p3_to_srgb(pixel_data: &mut [u8]) {
    apply_matrix_transform(pixel_data, &MATRIX_P3_TO_SRGB);
}

fn apply_rec2020_to_srgb(pixel_data: &mut [u8]) {
    apply_matrix_transform(pixel_data, &MATRIX_REC2020_TO_SRGB);
}

fn apply_matrix_transform(pixel_data: &mut [u8], matrix: &[[f32; 3]; 3]) {
    for chunk in pixel_data.chunks_exact_mut(4) {
        let r = f32::from(chunk[0]) / 255.0;
        let g = f32::from(chunk[1]) / 255.0;
        let b = f32::from(chunk[2]) / 255.0;

        let new_r = (matrix[0][0] * r + matrix[0][1] * g + matrix[0][2] * b).clamp(0.0, 1.0);
        let new_g = (matrix[1][0] * r + matrix[1][1] * g + matrix[1][2] * b).clamp(0.0, 1.0);
        let new_b = (matrix[2][0] * r + matrix[2][1] * g + matrix[2][2] * b).clamp(0.0, 1.0);

        chunk[0] = (new_r * 255.0).round() as u8;
        chunk[1] = (new_g * 255.0).round() as u8;
        chunk[2] = (new_b * 255.0).round() as u8;
    }
}

/// Масштабирует `src` до `(dst_w × dst_h)` билинейной интерполяцией.
/// Возвращает новый [`Image`] в формате [`PixelFormat::Rgba8`].
#[must_use]
pub fn resize_bilinear(src: &Image, dst_w: u32, dst_h: u32) -> Image {
    let dst_w = dst_w.max(1);
    let dst_h = dst_h.max(1);

    let src_rgba = src.to_rgba8();
    let sw = src.width as usize;
    let sh = src.height as usize;
    let dw = dst_w as usize;
    let dh = dst_h as usize;

    let mut out = vec![0u8; dw * dh * 4];

    for dy in 0..dh {
        for dx in 0..dw {
            let sx = (dx as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            let sy = (dy as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;

            let x0 = (sx.floor() as i32).clamp(0, sw as i32 - 1) as usize;
            let y0 = (sy.floor() as i32).clamp(0, sh as i32 - 1) as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let y1 = (y0 + 1).min(sh - 1);

            let fx = sx - sx.floor();
            let fy = sy - sy.floor();

            let p00 = (y0 * sw + x0) * 4;
            let p10 = (y0 * sw + x1) * 4;
            let p01 = (y1 * sw + x0) * 4;
            let p11 = (y1 * sw + x1) * 4;

            let o = (dy * dw + dx) * 4;
            for c in 0..4usize {
                let v = src_rgba[p00 + c] as f32 * (1.0 - fx) * (1.0 - fy)
                    + src_rgba[p10 + c] as f32 * fx * (1.0 - fy)
                    + src_rgba[p01 + c] as f32 * (1.0 - fx) * fy
                    + src_rgba[p11 + c] as f32 * fx * fy;
                out[o + c] = v.round() as u8;
            }
        }
    }

    Image { width: dst_w, height: dst_h, format: PixelFormat::Rgba8, data: out, icc_profile: None }
}

/// Масштабирует `src` до `(dst_w × dst_h)` усреднением по площади (box filter).
/// Возвращает новый [`Image`] в формате [`PixelFormat::Rgba8`].
///
/// При downscale с коэффициентом k×k каждый выходной пиксель получает вес от
/// k² источников, что устраняет алиасинг. При upscale поведение идентично
/// bilinear (area < 1 pixel → единственная точка выборки). Для смешанных случаев
/// (down по X, up по Y) каждая ось работает независимо.
#[must_use]
pub fn resize_area_avg(src: &Image, dst_w: u32, dst_h: u32) -> Image {
    let dst_w = dst_w.max(1);
    let dst_h = dst_h.max(1);

    let src_rgba = src.to_rgba8();
    let sw = src.width as usize;
    let sh = src.height as usize;
    let dw = dst_w as usize;
    let dh = dst_h as usize;

    let scale_x = sw as f64 / dw as f64;
    let scale_y = sh as f64 / dh as f64;

    let mut out = vec![0u8; dw * dh * 4];

    for dy in 0..dh {
        let sy0 = dy as f64 * scale_y;
        let sy1 = sy0 + scale_y;
        let iy0 = sy0 as usize;
        let iy1 = (sy1.ceil() as usize).min(sh);

        for dx in 0..dw {
            let sx0 = dx as f64 * scale_x;
            let sx1 = sx0 + scale_x;
            let ix0 = sx0 as usize;
            let ix1 = (sx1.ceil() as usize).min(sw);

            let mut acc = [0.0f64; 4];
            let mut total_w = 0.0f64;

            for py in iy0..iy1 {
                let wy = (py as f64 + 1.0).min(sy1) - (py as f64).max(sy0);
                if wy <= 0.0 { continue; }
                for px in ix0..ix1 {
                    let wx = (px as f64 + 1.0).min(sx1) - (px as f64).max(sx0);
                    if wx <= 0.0 { continue; }
                    let w = wx * wy;
                    let base = (py * sw + px) * 4;
                    for c in 0..4 {
                        acc[c] += src_rgba[base + c] as f64 * w;
                    }
                    total_w += w;
                }
            }

            let o = (dy * dw + dx) * 4;
            if total_w > 0.0 {
                for c in 0..4 {
                    out[o + c] = (acc[c] / total_w).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    Image { width: dst_w, height: dst_h, format: PixelFormat::Rgba8, data: out, icc_profile: None }
}

/// Формат пикселя декодированного изображения. Все варианты — 8 бит на канал.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Gray8,
    GrayAlpha8,
    Rgb8,
    Rgba8,
}

impl PixelFormat {
    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::GrayAlpha8 => 2,
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }

    #[must_use]
    pub const fn channels(self) -> usize { self.bytes_per_pixel() }
}

/// Ошибки декодирования PNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Первые 8 байтов не равны PNG-сигнатуре.
    InvalidSignature,
    /// Все прочие ошибки (zune-png).
    Decode(String),
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "не PNG: сигнатура не совпала"),
            Self::Decode(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_unknown_format() {
        assert_eq!(decode(&[]), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn input_shorter_than_png_signature_unknown() {
        let bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A];
        assert_eq!(decode(&bytes), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn jpeg_soi_without_third_byte_unknown() {
        let bytes = [0xFF, 0xD8];
        assert_eq!(decode(&bytes), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn jpeg_soi_with_wrong_third_byte_unknown() {
        let bytes = [0xFF, 0xD8, 0xFE, 0x00, 0x00];
        assert_eq!(decode(&bytes), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn random_bytes_unknown_format() {
        let bytes = [0u8; 16];
        assert_eq!(decode(&bytes), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn png_signature_dispatches_to_png_decoder() {
        let mut bytes = Vec::from(PNG_SIGNATURE);
        bytes.extend_from_slice(&[0x00; 4]);
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, ImageError::Png(_)), "ожидался Png(_), получено {err:?}");
    }

    #[test]
    fn jpeg_signature_dispatches_to_jpeg_decoder() {
        let bytes = [0xFF, 0xD8, 0xFF, 0x00, 0x00];
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, ImageError::Jpeg(_)), "ожидался Jpeg(_), получено {err:?}");
    }

    #[test]
    fn image_error_from_decode_error() {
        let err: ImageError = DecodeError::InvalidSignature.into();
        assert!(matches!(err, ImageError::Png(DecodeError::InvalidSignature)));
    }

    #[test]
    fn image_error_display_includes_inner() {
        let err = ImageError::Png(DecodeError::InvalidSignature);
        let s = format!("{err}");
        assert!(s.starts_with("PNG:"), "Display должен начинаться с PNG: — получено {s:?}");
    }

    #[test]
    fn image_error_display_unknown_format() {
        let s = format!("{}", ImageError::UnknownFormat);
        assert!(!s.is_empty());
    }

    #[test]
    fn gif_signature_dispatches_to_gif_decoder() {
        let bytes = b"GIF89a\x00\x00\x00\x00\x00\x00";
        let err = decode(bytes).unwrap_err();
        assert!(matches!(err, ImageError::Gif(_)), "ожидалась Gif(_), получено {err:?}");
    }

    #[test]
    fn gif87a_signature_dispatches_to_gif_decoder() {
        let bytes = b"GIF87a\x00\x00\x00\x00\x00\x00";
        let err = decode(bytes).unwrap_err();
        assert!(matches!(err, ImageError::Gif(_)), "ожидалась Gif(_), получено {err:?}");
    }

    #[test]
    fn image_error_from_gif_error() {
        let err: ImageError = GifError::InvalidSignature.into();
        assert!(matches!(err, ImageError::Gif(GifError::InvalidSignature)));
    }

    #[test]
    fn image_error_display_gif() {
        let err = ImageError::Gif(GifError::InvalidSignature);
        let s = format!("{err}");
        assert!(s.starts_with("GIF:"), "Display должен начинаться с GIF: — получено {s:?}");
    }

    #[test]
    fn supported_mime_types_includes_gif() {
        let types = supported_mime_types();
        assert!(types.contains(&"image/gif"), "image/gif должен быть в поддерживаемых типах");
    }

    #[test]
    fn supported_mime_types_includes_avif() {
        let types = supported_mime_types();
        assert!(types.contains(&"image/avif"), "image/avif должен быть в поддерживаемых типах");
    }

    #[test]
    fn avif_signature_dispatches_to_avif_decoder() {
        // Минимальный ftyp-бокс с major brand avif
        let bytes: Vec<u8> = [
            0x00, 0x00, 0x00, 0x18_u8,   // box size = 24
            b'f', b't', b'y', b'p',       // box type = ftyp
            b'a', b'v', b'i', b'f',       // major brand = avif
            0x00, 0x00, 0x00, 0x00,       // minor version
            b'm', b'i', b'f', b'1',       // compatible brand
            0x00, 0x00, 0x00, 0x00,       // padding
        ]
        .into_iter()
        .chain([0u8; 32])
        .collect();
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, ImageError::Avif(_)), "ожидался Avif(_), получено {err:?}");
    }

    #[test]
    fn image_error_from_avif_error() {
        let err: ImageError = AvifError::InvalidSignature.into();
        assert!(matches!(err, ImageError::Avif(AvifError::InvalidSignature)));
    }

    #[test]
    fn image_error_display_avif() {
        let err = ImageError::Avif(AvifError::InvalidSignature);
        let s = format!("{err}");
        assert!(s.starts_with("AVIF:"), "Display должен начинаться с AVIF: — получено {s:?}");
    }

    fn solid_image(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> Image {
        let data = vec![r, g, b, a].into_iter().cycle().take((w * h * 4) as usize).collect();
        Image { width: w, height: h, format: PixelFormat::Rgba8, data, icc_profile: None }
    }

    #[test]
    fn area_avg_1x1_solid_preserves_color() {
        let src = solid_image(4, 4, 200, 100, 50, 255);
        let dst = resize_area_avg(&src, 1, 1);
        assert_eq!(dst.width, 1);
        assert_eq!(dst.height, 1);
        assert_eq!(&dst.data[..4], &[200, 100, 50, 255]);
    }

    #[test]
    fn area_avg_same_size_preserves_pixels() {
        let src = solid_image(3, 3, 128, 64, 32, 255);
        let dst = resize_area_avg(&src, 3, 3);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn area_avg_2x1_downscale_averages_correctly() {
        // 2×1 → 1×1: два горизонтальных пикселя, разный цвет.
        let data = vec![100, 0, 0, 255, 200, 0, 0, 255];
        let src = Image { width: 2, height: 1, format: PixelFormat::Rgba8, data, icc_profile: None };
        let dst = resize_area_avg(&src, 1, 1);
        assert_eq!(dst.data[0], 150); // (100+200)/2
    }

    #[test]
    fn area_avg_output_size_correct() {
        let src = solid_image(100, 80, 255, 0, 0, 255);
        let dst = resize_area_avg(&src, 20, 16);
        assert_eq!(dst.width, 20);
        assert_eq!(dst.height, 16);
        assert_eq!(dst.data.len(), 20 * 16 * 4);
        assert_eq!(dst.format, PixelFormat::Rgba8);
    }

    #[test]
    fn area_avg_upscale_works() {
        let src = solid_image(2, 2, 10, 20, 30, 255);
        let dst = resize_area_avg(&src, 4, 4);
        assert_eq!(dst.width, 4);
        assert_eq!(dst.height, 4);
        assert_eq!(&dst.data[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn area_avg_zero_size_clamped_to_1() {
        let src = solid_image(4, 4, 0, 0, 0, 255);
        let dst = resize_area_avg(&src, 0, 0);
        assert_eq!(dst.width, 1);
        assert_eq!(dst.height, 1);
    }

    // ── ICC gamut detection ───────────────────────────────────────────────

    fn icc_with_rgb_space_and_desc(desc: &[u8]) -> IccProfile {
        let mut data = vec![0u8; 256];
        // bytes 16-19: data color space = "RGB "
        data[16..20].copy_from_slice(b"RGB ");
        // bytes 36-39: file signature "acsp" (not required by detect_gamut, but realistic)
        data[36..40].copy_from_slice(b"acsp");
        // Embed description string at offset 130 (after 128-byte header + tag count).
        if 130 + desc.len() <= data.len() {
            data[130..130 + desc.len()].copy_from_slice(desc);
        }
        IccProfile { data }
    }

    #[test]
    fn detect_gamut_display_p3() {
        let p = icc_with_rgb_space_and_desc(b"Display P3");
        assert_eq!(p.detect_gamut(), IccGamut::DisplayP3);
    }

    #[test]
    fn detect_gamut_dci_p3_hyphen() {
        let p = icc_with_rgb_space_and_desc(b"DCI-P3");
        assert_eq!(p.detect_gamut(), IccGamut::DisplayP3);
    }

    #[test]
    fn detect_gamut_rec2020_dot() {
        let p = icc_with_rgb_space_and_desc(b"Rec. 2020");
        assert_eq!(p.detect_gamut(), IccGamut::Rec2020);
    }

    #[test]
    fn detect_gamut_bt2020() {
        let p = icc_with_rgb_space_and_desc(b"BT.2020");
        assert_eq!(p.detect_gamut(), IccGamut::Rec2020);
    }

    #[test]
    fn detect_gamut_srgb_label() {
        let p = icc_with_rgb_space_and_desc(b"sRGB");
        assert_eq!(p.detect_gamut(), IccGamut::Srgb);
    }

    #[test]
    fn detect_gamut_unknown_label() {
        let p = icc_with_rgb_space_and_desc(b"ProPhoto RGB");
        assert_eq!(p.detect_gamut(), IccGamut::Unknown);
    }

    #[test]
    fn detect_gamut_non_rgb_colorspace() {
        let mut data = vec![0u8; 256];
        data[16..20].copy_from_slice(b"GRAY");
        data[130..140].copy_from_slice(b"Display P3");
        let p = IccProfile { data };
        // Non-RGB space: even if description says P3, we can't convert.
        assert_eq!(p.detect_gamut(), IccGamut::Unknown);
    }

    #[test]
    fn detect_gamut_too_short() {
        let p = IccProfile { data: vec![0u8; 10] };
        assert_eq!(p.detect_gamut(), IccGamut::Unknown);
    }

    // ── correct_rgba_pixels ───────────────────────────────────────────────

    #[test]
    fn correct_rgba_srgb_profile_noop() {
        let profile = icc_with_rgb_space_and_desc(b"sRGB");
        let original = vec![200u8, 100, 50, 255];
        let mut rgba = original.clone();
        correct_rgba_pixels(&mut rgba, &profile);
        assert_eq!(rgba, original, "sRGB profile should not change pixels");
    }

    #[test]
    fn correct_rgba_unknown_profile_noop() {
        let profile = icc_with_rgb_space_and_desc(b"ProPhoto RGB");
        let original = vec![200u8, 100, 50, 128];
        let mut rgba = original.clone();
        correct_rgba_pixels(&mut rgba, &profile);
        assert_eq!(rgba, original, "unknown profile should not change pixels");
    }

    #[test]
    fn correct_rgba_p3_changes_pixels() {
        let profile = icc_with_rgb_space_and_desc(b"Display P3");
        // (200, 100, 50) in P3 gamma encoding — not a sRGB-identical point.
        let original = vec![200u8, 100, 50, 255];
        let mut rgba = original.clone();
        correct_rgba_pixels(&mut rgba, &profile);
        // After P3→sRGB conversion the RGB values change (different primaries).
        assert_ne!(rgba[..3], original[..3], "P3 correction should change RGB");
        // Alpha must be preserved.
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn correct_rgba_rec2020_changes_pixels() {
        let profile = icc_with_rgb_space_and_desc(b"Rec. 2020");
        let original = vec![180u8, 80, 40, 200];
        let mut rgba = original.clone();
        correct_rgba_pixels(&mut rgba, &profile);
        assert_ne!(rgba[..3], original[..3], "Rec2020 correction should change RGB");
        assert_eq!(rgba[3], 200, "alpha must not change");
    }

    #[test]
    fn correct_rgba_alpha_preserved_always() {
        let profile = icc_with_rgb_space_and_desc(b"Display P3");
        let mut rgba = vec![100u8, 150, 200, 77, 50, 60, 70, 128];
        correct_rgba_pixels(&mut rgba, &profile);
        assert_eq!(rgba[3], 77);
        assert_eq!(rgba[7], 128);
    }

    #[test]
    fn correct_rgba_white_pixel_roundtrip() {
        // White (255,255,255) in P3 = white in sRGB (identical by definition).
        let profile = icc_with_rgb_space_and_desc(b"Display P3");
        let mut rgba = vec![255u8, 255, 255, 255];
        correct_rgba_pixels(&mut rgba, &profile);
        assert_eq!(rgba, [255, 255, 255, 255]);
    }

    #[test]
    fn correct_rgba_black_pixel_roundtrip() {
        // Black (0,0,0) maps to black in any gamut.
        let profile = icc_with_rgb_space_and_desc(b"Display P3");
        let mut rgba = vec![0u8, 0, 0, 255];
        correct_rgba_pixels(&mut rgba, &profile);
        assert_eq!(rgba, [0, 0, 0, 255]);
    }

    #[test]
    fn jxl_signature_naked_format_detected() {
        let bytes = vec![0xFF, 0x0A, 0x00, 0x00];
        assert!(is_jxl(&bytes));
    }

    #[test]
    fn jxl_signature_isobmff_major_brand_detected() {
        let mut bytes = vec![0x00, 0x00, 0x00, 0x14]; // box size = 20
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"jxl "); // major brand
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        assert!(is_jxl(&bytes));
    }

    #[test]
    fn jxl_signature_isobmff_compatible_brand_detected() {
        let mut bytes = vec![0x00, 0x00, 0x00, 0x18]; // box size = 24
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"mj2 "); // different major brand
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        bytes.extend_from_slice(b"jxl "); // compatible brand
        assert!(is_jxl(&bytes));
    }

    #[test]
    fn jxl_not_detected_in_non_jxl_data() {
        let png_sig = vec![0x89, 0x50, 0x4E, 0x47]; // PNG
        assert!(!is_jxl(&png_sig));
    }

    #[test]
    fn jxl_decode_always_fails_phase0() {
        let bytes = vec![0xFF, 0x0A];
        let result = decode(&bytes);
        assert!(matches!(result, Err(ImageError::Jxl(_))));
    }

    #[test]
    fn supported_mime_types_excludes_jxl_stub() {
        // `decode_jxl` — заглушка (всегда Err), поэтому image/jxl НЕ должен
        // числиться поддерживаемым: иначе picture-picker выберет
        // `<source type="image/jxl">` и покажет пустую коробку вместо fallback.
        let types = supported_mime_types();
        assert!(!types.contains(&"image/jxl"), "image/jxl (декодер-заглушка) не должен числиться поддерживаемым");
    }

    fn make_ftyp_bytes(major: &[u8; 4]) -> Vec<u8> {
        let mut v = vec![0x00, 0x00, 0x00, 0x10]; // box size = 16
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(major);
        v.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        // Pad to 32 bytes so other checks don't fail due to short input.
        v.extend_from_slice(&[0x00; 16]);
        v
    }

    #[test]
    fn heic_signature_detected() {
        let bytes = make_ftyp_bytes(b"heic");
        assert!(is_heic(&bytes));
    }

    #[test]
    fn heic_decode_always_fails_phase1() {
        let bytes = make_ftyp_bytes(b"heic");
        let result = decode(&bytes);
        assert!(matches!(result, Err(ImageError::Heic(_))));
    }

    #[test]
    fn supported_mime_types_excludes_heic_stub() {
        // `decode_heic` — заглушка (всегда Err): image/heic не должен числиться
        // поддерживаемым, иначе picker выберет heic-source и покажет пустую коробку
        // вместо fallback на `<img src>` (BUG-069).
        let types = supported_mime_types();
        assert!(!types.contains(&"image/heic"), "image/heic (декодер-заглушка) не должен числиться поддерживаемым");
    }

    #[test]
    fn supported_mime_types_excludes_heif_stub() {
        // Та же причина, что и heic: `decode_heic` обслуживает heif-ветку заглушкой.
        let types = supported_mime_types();
        assert!(!types.contains(&"image/heif"), "image/heif (декодер-заглушка) не должен числиться поддерживаемым");
    }

    #[test]
    fn heic_error_from_conversion() {
        let err: ImageError = HeicError.into();
        assert!(matches!(err, ImageError::Heic(HeicError)));
    }

    #[test]
    fn heic_error_display() {
        let err = ImageError::Heic(HeicError);
        let s = format!("{err}");
        assert!(s.starts_with("HEIC/HEIF:"), "Display должен начинаться с HEIC/HEIF: — получено {s:?}");
    }

    #[test]
    fn detect_color_space_without_icc_returns_srgb() {
        let img = Image {
            width: 100,
            height: 100,
            format: PixelFormat::Rgba8,
            data: vec![255; 100 * 100 * 4],
            icc_profile: None,
        };
        assert_eq!(
            img.detect_color_space(),
            lumen_layout::style::ColorSpace::Srgb,
            "Image without ICC profile should default to sRGB"
        );
    }

    #[test]
    fn detect_color_space_with_srgb_icc() {
        // Create a minimal valid sRGB ICC profile (128 bytes with RGB signature)
        let mut profile_data = vec![0u8; 128];
        profile_data[16] = 0x52; // 'R'
        profile_data[17] = 0x47; // 'G'
        profile_data[18] = 0x42; // 'B'
        profile_data[19] = 0x20; // ' '

        let img = Image {
            width: 10,
            height: 10,
            format: PixelFormat::Rgba8,
            data: vec![128; 10 * 10 * 4],
            icc_profile: Some(IccProfile { data: profile_data }),
        };
        assert_eq!(
            img.detect_color_space(),
            lumen_layout::style::ColorSpace::Srgb,
            "sRGB profile should be detected correctly"
        );
    }

    #[test]
    fn detect_color_space_with_display_p3_icc() {
        // Create a Display P3 ICC profile (with "Display P3" text marker)
        let mut profile_data = vec![0u8; 200];
        profile_data[16] = 0x52;
        profile_data[17] = 0x47;
        profile_data[18] = 0x42;
        profile_data[19] = 0x20;

        let p3_text = b"Display P3";
        for (i, &b) in p3_text.iter().enumerate() {
            if 150 + i < profile_data.len() {
                profile_data[150 + i] = b;
            }
        }

        let img = Image {
            width: 10,
            height: 10,
            format: PixelFormat::Rgba8,
            data: vec![200; 10 * 10 * 4],
            icc_profile: Some(IccProfile { data: profile_data }),
        };
        assert_eq!(
            img.detect_color_space(),
            lumen_layout::style::ColorSpace::DisplayP3,
            "Display P3 profile should be detected correctly"
        );
    }

    #[test]
    fn tone_mapping_srgb_no_change() {
        let mut pixels = vec![255, 128, 64, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        let original = pixels.clone();
        apply_tone_mapping(lumen_layout::style::ColorSpace::Srgb, &mut pixels);
        assert_eq!(pixels, original);
    }

    #[test]
    fn tone_mapping_p3_converts_pixels() {
        let mut pixels = vec![255, 0, 0, 255];
        apply_tone_mapping(lumen_layout::style::ColorSpace::DisplayP3, &mut pixels);
        assert!(pixels[0] > 0, "P3 red should map to non-zero sRGB red");
        assert_eq!(pixels[3], 255, "Alpha unchanged");
    }

    #[test]
    fn tone_mapping_rec2020_converts_pixels() {
        let mut pixels = vec![255, 0, 0, 255];
        apply_tone_mapping(lumen_layout::style::ColorSpace::Rec2020, &mut pixels);
        assert!(pixels[0] > 0, "Rec2020 red should map to non-zero sRGB red");
        assert_eq!(pixels[3], 255, "Alpha unchanged");
    }

    #[test]
    fn tone_mapping_preserves_alpha() {
        let mut pixels = vec![255, 128, 64, 100, 200, 150, 50, 0];
        apply_tone_mapping(lumen_layout::style::ColorSpace::DisplayP3, &mut pixels);
        assert_eq!(pixels[3], 100, "First alpha preserved");
        assert_eq!(pixels[7], 0, "Second alpha preserved");
    }

    #[test]
    fn tone_mapping_black_stays_black() {
        let mut pixels = vec![0, 0, 0, 255];
        apply_tone_mapping(lumen_layout::style::ColorSpace::DisplayP3, &mut pixels);
        assert_eq!(pixels[0], 0, "Black R");
        assert_eq!(pixels[1], 0, "Black G");
        assert_eq!(pixels[2], 0, "Black B");
    }

    #[test]
    fn tone_mapping_white_stays_white() {
        let mut pixels = vec![255, 255, 255, 255];
        apply_tone_mapping(lumen_layout::style::ColorSpace::DisplayP3, &mut pixels);
        assert_eq!(pixels[0], 255, "White R");
        assert_eq!(pixels[1], 255, "White G");
        assert_eq!(pixels[2], 255, "White B");
    }

    #[test]
    fn tone_mapping_multiple_pixels() {
        let mut pixels = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        apply_tone_mapping(lumen_layout::style::ColorSpace::Rec2020, &mut pixels);
        assert_eq!(pixels[3], 255);
        assert_eq!(pixels[7], 255);
        assert_eq!(pixels[11], 255);
    }
}
