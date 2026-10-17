//! Готовый RGBA-кадр от декодера, который сам переводит цвет (GStreamer,
//! COMPAT-MEDIA-OSDEC-LINUX): `videoconvert` берёт матрицу и диапазон из потока.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::registry::DecodedFrame;

/// Кадр RGBA8 без выравнивания строк.
pub(crate) struct RgbaFrame {
    pub pts_us: i64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl DecodedFrame for RgbaFrame {
    fn pts_us(&self) -> i64 {
        self.pts_us
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn to_rgba8(&self) -> Result<Vec<u8>, String> {
        Ok(self.rgba.clone())
    }
}

/// Байты буфера `width × height` RGBA → плотные строки. Буфер может быть шире
/// (выравнивание строк): шаг — длина, делённая на высоту.
///
/// # Errors
/// Размер буфера не согласован с кадром.
pub(crate) fn pack_rgba(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let (w, h) = (width as usize, height as usize);
    let row = w.checked_mul(4).ok_or("ширина кадра слишком велика")?;
    let total = row.checked_mul(h).ok_or("размер кадра слишком велик")?;
    if w == 0 || h == 0 {
        return Err(format!("пустой кадр {width}×{height}"));
    }
    if data.len() == total {
        return Ok(data.to_vec());
    }
    let stride = data.len() / h;
    if !data.len().is_multiple_of(h) || stride < row {
        return Err(format!("кадр {width}×{height} не помещается в буфер из {} байт", data.len()));
    }
    let mut out = Vec::with_capacity(total);
    for y in 0..h {
        out.extend_from_slice(&data[y * stride..y * stride + row]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tight_buffer_is_copied_as_is() {
        let data: Vec<u8> = (0..16).collect();
        assert_eq!(pack_rgba(&data, 2, 2).unwrap(), data);
    }

    #[test]
    fn padded_rows_are_trimmed() {
        // 1×2 пикселя при шаге 8 байт: по 4 байта полезных и 4 выравнивания.
        let data = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0];
        assert_eq!(pack_rgba(&data, 1, 2).unwrap(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn mismatched_buffers_are_errors() {
        assert!(pack_rgba(&[0; 15], 2, 2).is_err());
        assert!(pack_rgba(&[0; 8], 2, 2).is_err());
        assert!(pack_rgba(&[], 0, 0).is_err());
        assert!(pack_rgba(&[0; 4], 0, 1).is_err());
    }

    #[test]
    fn frame_reports_its_geometry() {
        let f = RgbaFrame { pts_us: 40_000, width: 1, height: 1, rgba: vec![9, 8, 7, 255] };
        assert_eq!((f.pts_us(), f.width(), f.height()), (40_000, 1, 1));
        assert_eq!(f.to_rgba8().unwrap(), vec![9, 8, 7, 255]);
    }
}
