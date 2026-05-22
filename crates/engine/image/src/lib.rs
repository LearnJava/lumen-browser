mod jpeg;
mod png;

pub use jpeg::{decode_jpeg, JpegError};
pub use png::decode_png;

/// PNG-сигнатура: `89 50 4E 47 0D 0A 1A 0A` (PNG §5.2).
pub const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// JPEG SOI + начало следующего маркера: `FF D8 FF`.
pub const JPEG_SIGNATURE_PREFIX: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// Декодирует растровое изображение по сигнатуре первых байтов.
///
/// # Errors
/// - [`ImageError::UnknownFormat`] — сигнатура не распознана.
/// - [`ImageError::Png`] — PNG-сигнатура совпала, но декодер выдал ошибку.
/// - [`ImageError::Jpeg`] — JPEG-сигнатура совпала, но декодер выдал ошибку.
pub fn decode(bytes: &[u8]) -> Result<Image, ImageError> {
    if bytes.len() >= PNG_SIGNATURE.len() && bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE {
        return decode_png(bytes).map_err(ImageError::Png);
    }
    if bytes.len() >= JPEG_SIGNATURE_PREFIX.len()
        && bytes[..JPEG_SIGNATURE_PREFIX.len()] == JPEG_SIGNATURE_PREFIX
    {
        return decode_jpeg(bytes).map_err(ImageError::Jpeg);
    }
    Err(ImageError::UnknownFormat)
}

/// Ошибка `decode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    UnknownFormat,
    Png(DecodeError),
    Jpeg(JpegError),
}

impl core::fmt::Display for ImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownFormat => write!(f, "формат изображения не распознан по сигнатуре"),
            Self::Png(e) => write!(f, "PNG: {e}"),
            Self::Jpeg(e) => write!(f, "JPEG: {e}"),
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

/// Декодированное растровое изображение в плотной row-major упаковке.
/// Длина `data` равна `width * height * bytes_per_pixel(format)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl Image {
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

    Image { width: dst_w, height: dst_h, format: PixelFormat::Rgba8, data: out }
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

    Image { width: dst_w, height: dst_h, format: PixelFormat::Rgba8, data: out }
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

    fn solid_image(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> Image {
        let data = vec![r, g, b, a].into_iter().cycle().take((w * h * 4) as usize).collect();
        Image { width: w, height: h, format: PixelFormat::Rgba8, data }
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
        let src = Image { width: 2, height: 1, format: PixelFormat::Rgba8, data };
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
}
