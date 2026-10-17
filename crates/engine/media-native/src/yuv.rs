//! Полупланарные кадры YUV 4:2:0 (NV12, P010), которые отдают декодеры ОС → RGBA8.

#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

use crate::registry::DecodedFrame;

/// Матрица преобразования цвета (Rec. 601 или Rec. 709).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Matrix {
    Bt601,
    Bt709,
}

impl Matrix {
    /// Когда поток матрицу не назвал: HD — Rec. 709, SD — Rec. 601.
    pub(crate) fn guess(height: u32) -> Self {
        if height >= 720 { Self::Bt709 } else { Self::Bt601 }
    }
}

/// Раскладка кадра в памяти.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layout {
    /// Видимые размеры, пиксели.
    pub width: u32,
    pub height: u32,
    /// Байтов между строками (общий для Y и UV).
    pub stride: usize,
    /// Смещение плоскости UV от начала буфера.
    pub uv_offset: usize,
    /// 2 для P010 (16 бит на сэмпл, значение в старших битах), иначе 1.
    pub bytes_per_sample: usize,
    pub matrix: Matrix,
    /// Полный диапазон 0–255 (иначе «ТВ»: Y 16–235, C 16–240).
    pub full_range: bool,
}

/// Кадр NV12/P010, скопированный из памяти декодера ОС; в RGBA переводится по
/// требованию ([`DecodedFrame::to_rgba8`]).
pub(crate) struct SemiplanarFrame {
    pub pts_us: i64,
    pub layout: Layout,
    pub data: Vec<u8>,
}

impl DecodedFrame for SemiplanarFrame {
    fn pts_us(&self) -> i64 {
        self.pts_us
    }

    fn width(&self) -> u32 {
        self.layout.width
    }

    fn height(&self) -> u32 {
        self.layout.height
    }

    fn to_rgba8(&self) -> Result<Vec<u8>, String> {
        semiplanar_to_rgba(&self.data, &self.layout)
    }
}

/// Коэффициенты Q8: `(масштаб яркости, V→R, U→G, V→G, U→B)`.
fn coefficients(matrix: Matrix, full_range: bool) -> (i32, i32, i32, i32, i32) {
    match (matrix, full_range) {
        (Matrix::Bt601, false) => (298, 409, 100, 208, 516),
        (Matrix::Bt709, false) => (298, 459, 55, 136, 541),
        (Matrix::Bt601, true) => (256, 359, 88, 183, 454),
        (Matrix::Bt709, true) => (256, 403, 48, 120, 475),
    }
}

/// Кадр NV12/P010 → RGBA8 (`width * height * 4` байт).
///
/// # Errors
/// Буфер короче, чем требует раскладка.
pub(crate) fn semiplanar_to_rgba(data: &[u8], l: &Layout) -> Result<Vec<u8>, String> {
    let (w, h) = (l.width as usize, l.height as usize);
    let bps = l.bytes_per_sample;
    let fits = (|| {
        let y_end = l.stride.checked_mul(h)?;
        let uv_end = l.uv_offset.checked_add(l.stride.checked_mul(h.div_ceil(2))?)?;
        (w > 0 && h > 0 && l.stride >= w.div_ceil(2) * 2 * bps && y_end <= l.uv_offset && uv_end <= data.len())
            .then_some(())
    })();
    if fits.is_none() {
        return Err(format!(
            "кадр {w}×{h} не помещается в буфер ({} байт, stride {}, смещение UV {})",
            data.len(),
            l.stride,
            l.uv_offset
        ));
    }
    let (ky, kvr, kug, kvg, kub) = coefficients(l.matrix, l.full_range);
    let luma_black = if l.full_range { 0 } else { 16 };
    // Сэмпл `i` строки: старший байт у 16-битного формата, сам байт у 8-битного.
    let sample = |row: &[u8], i: usize| -> i32 {
        if bps == 2 { i32::from(row[i * 2 + 1]) } else { i32::from(row[i]) }
    };
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let yrow = &data[y * l.stride..];
        let uvrow = &data[l.uv_offset + (y / 2) * l.stride..];
        for x in 0..w {
            let luma = (sample(yrow, x) - luma_black) * ky;
            let u = sample(uvrow, (x / 2) * 2) - 128;
            let v = sample(uvrow, (x / 2) * 2 + 1) - 128;
            let r = (luma + kvr * v + 128) >> 8;
            let g = (luma - kug * u - kvg * v + 128) >> 8;
            let b = (luma + kub * u + 128) >> 8;
            let o = (y * w + x) * 4;
            out[o] = r.clamp(0, 255) as u8;
            out[o + 1] = g.clamp(0, 255) as u8;
            out[o + 2] = b.clamp(0, 255) as u8;
            out[o + 3] = 255;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(w: u32, h: u32, full: bool) -> Layout {
        Layout {
            width: w,
            height: h,
            stride: w as usize,
            uv_offset: (w * h) as usize,
            bytes_per_sample: 1,
            matrix: Matrix::Bt601,
            full_range: full,
        }
    }

    #[test]
    fn limited_range_black_white_and_gray() {
        // 2×2: Y = 16 (чёрный), 235 (белый), 126 (серый), 16; нейтральный цвет 128.
        let data = [16, 235, 126, 16, 128, 128];
        let rgba = semiplanar_to_rgba(&data, &layout(2, 2, false)).unwrap();
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[255, 255, 255, 255]);
        let g = rgba[8];
        assert!((125..=131).contains(&g) && rgba[9] == g && rgba[10] == g, "{:?}", &rgba[8..12]);
    }

    #[test]
    fn full_range_passes_luma_through() {
        let data = [0, 255, 64, 200, 128, 128];
        let rgba = semiplanar_to_rgba(&data, &layout(2, 2, true)).unwrap();
        assert_eq!(&rgba[0..3], &[0, 0, 0]);
        assert_eq!(&rgba[4..7], &[255, 255, 255]);
        assert_eq!(&rgba[8..11], &[64, 64, 64]);
    }

    #[test]
    fn red_chroma_dominates_red_channel() {
        // Y = 81, U = 90, V = 240 — «красный» в Rec. 601.
        let data = [81, 81, 81, 81, 90, 240];
        let rgba = semiplanar_to_rgba(&data, &layout(2, 2, false)).unwrap();
        assert!(rgba[0] > 240 && rgba[1] < 20 && rgba[2] < 20, "{:?}", &rgba[0..4]);
    }

    #[test]
    fn p010_uses_high_bytes() {
        let mut data = Vec::new();
        for y in [235u16, 16, 16, 235] {
            data.extend_from_slice(&(y << 8).to_le_bytes());
        }
        for c in [128u16, 128] {
            data.extend_from_slice(&(c << 8).to_le_bytes());
        }
        let l = Layout { stride: 4, uv_offset: 8, bytes_per_sample: 2, ..layout(2, 2, false) };
        let rgba = semiplanar_to_rgba(&data, &l).unwrap();
        assert_eq!(&rgba[0..3], &[255, 255, 255]);
        assert_eq!(&rgba[4..7], &[0, 0, 0]);
    }

    #[test]
    fn short_buffer_is_error() {
        assert!(semiplanar_to_rgba(&[0; 5], &layout(2, 2, false)).is_err());
        assert!(semiplanar_to_rgba(&[], &layout(0, 0, false)).is_err());
    }

    #[test]
    fn padded_stride_and_odd_width() {
        // 3×2 при stride 4: нечётная ширина → общая UV-пара на последний столбец.
        let data = [16, 16, 16, 9, 16, 16, 16, 9, 128, 128, 128, 128];
        let l = Layout { stride: 4, uv_offset: 8, ..layout(3, 2, false) };
        let rgba = semiplanar_to_rgba(&data, &l).unwrap();
        assert_eq!(rgba.len(), 3 * 2 * 4);
        assert!(rgba.chunks(4).all(|p| p[0] == 0 && p[3] == 255));
    }
}
