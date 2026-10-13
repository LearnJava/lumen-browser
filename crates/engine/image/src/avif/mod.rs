//! AVIF декодер (AV1 Image File Format, ISO/IEC 23008-12).
//!
//! AVIF — ISOBMFF-контейнер с AV1-кодированным изображением.
//!
//! Phase 0 ограничения:
//! - Только первый (и единственный) кадр статичных AVIF.
//! - Анимированный AVIF (major_brand `avis`) распознаётся, но декодируется
//!   только первый кадр; полная анимация — Wave 3.
//! - ICC-профиль не извлекается (icc_profile поле → None).
//!
//! Декодирование всегда включено и не требует системных библиотек:
//! контейнер разбирает `avif-parse`, AV1 — `rav1d` (Rust-порт dav1d, без `asm`).
//! Feature `avif` в Cargo.toml осталась пустой для совместимости.

/// Ошибка декодирования AVIF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvifError {
    /// Байты не являются валидным ISOBMFF-файлом с ftyp=avif/avis.
    InvalidSignature,
    /// Контейнер распознан, но декодер вернул ошибку.
    Decode(String),
}

impl core::fmt::Display for AvifError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "не AVIF: ftyp-бокс не найден или бренд не avif/avis"),
            Self::Decode(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for AvifError {}

/// Проверяет AVIF-сигнатуру по ISOBMFF ftyp-боксу.
///
/// AVIF/AVIS — ISOBMFF-контейнер. Первый бокс в файле — `ftyp`:
/// - Байты 0–3: размер бокса (u32 big-endian).
/// - Байты 4–7: тип бокса (`ftyp`).
/// - Байты 8–11: major brand (`avif` или `avis`).
///
/// Метод проверяет только major brand; совместимые бренды не сканируются
/// (достаточно для 99 % реальных AVIF-файлов).
#[must_use]
pub fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    if &bytes[4..8] != b"ftyp" {
        return false;
    }
    let brand = &bytes[8..12];
    brand == b"avif" || brand == b"avis"
}

/// Декодирует AVIF-файл в RGBA8 (4 байта на пиксель, row-major).
///
/// Возвращает `(ширина, высота, rgba8_данные)`.
///
/// # Errors
/// - [`AvifError::InvalidSignature`] — сигнатура AVIF не найдена.
/// - [`AvifError::Decode`] — декодирование не удалось.
pub fn decode_avif(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), AvifError> {
    if !is_avif(bytes) {
        return Err(AvifError::InvalidSignature);
    }
    decode_avif_impl(bytes)
}

fn decode_avif_impl(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), AvifError> {
    let data = avif_parse::read_avif(&mut &bytes[..])
        .map_err(|e| AvifError::Decode(format!("avif-parse: {e:?}")))?;
    let color = decode_av1(&data.primary_item)?;
    let alpha = match data.alpha_item.as_deref() {
        Some(a) => Some(decode_av1(a)?),
        None => None,
    };
    to_rgba8(&color, alpha.as_ref(), data.premultiplied_alpha)
}

/// Распакованный AV1-кадр: плоскости приведены к `u16`.
struct Frame {
    w: usize,
    h: usize,
    layout: u32,
    bpc: u32,
    full_range: bool,
    mtrx: u32,
    planes: [Vec<u16>; 3],
    /// Ширина плоскостей (Y, U/V).
    stride: [usize; 2],
}

/// Предел площади кадра: защита от декомпрессионной бомбы (~256 МБ RGBA).
const MAX_PIXELS: u32 = 64 * 1024 * 1024;

/// Закрывает контекст rav1d на любом выходе из `decode_av1`.
struct CtxGuard(Option<rav1d::include::dav1d::dav1d::Dav1dContext>);

impl Drop for CtxGuard {
    fn drop(&mut self) {
        // SAFETY: контекст получен из `dav1d_open` и закрывается ровно один раз.
        unsafe { rav1d::src::lib::dav1d_close(Some(core::ptr::NonNull::from(&mut self.0))) };
    }
}

fn decode_av1(obu: &[u8]) -> Result<Frame, AvifError> {
    use core::ptr::NonNull;
    use rav1d::include::dav1d::data::Dav1dData;
    use rav1d::include::dav1d::dav1d::Dav1dSettings;
    use rav1d::include::dav1d::picture::Dav1dPicture;
    use std::mem::MaybeUninit;

    const EAGAIN: i32 = -11;
    let err = |what: &str, code: i32| AvifError::Decode(format!("rav1d: {what} ({code})"));

    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    // SAFETY: `dav1d_default_settings` полностью инициализирует переданную структуру.
    let mut settings = unsafe {
        rav1d::src::lib::dav1d_default_settings(NonNull::from(&mut settings).cast());
        settings.assume_init()
    };
    settings.n_threads = 1;
    settings.max_frame_delay = 1;
    settings.frame_size_limit = MAX_PIXELS;

    let mut guard = CtxGuard(None);
    // SAFETY: оба указателя валидны на запись/чтение на время вызова.
    let rc = unsafe {
        rav1d::src::lib::dav1d_open(Some(NonNull::from(&mut guard.0)), Some(NonNull::from(&mut settings)))
    };
    if rc.0 < 0 || guard.0.is_none() {
        return Err(err("open", rc.0));
    }

    let mut data = MaybeUninit::<Dav1dData>::uninit();
    // SAFETY: `dav1d_data_create` пишет в `data` и возвращает буфер длины `obu.len()`.
    let mut data = unsafe {
        let buf = rav1d::src::lib::dav1d_data_create(Some(NonNull::from(&mut data).cast()), obu.len());
        if buf.is_null() {
            return Err(AvifError::Decode("rav1d: data_create".into()));
        }
        core::ptr::copy_nonoverlapping(obu.as_ptr(), buf, obu.len());
        data.assume_init()
    };

    let mut pic: Option<Dav1dPicture> = None;
    let mut outcome = Err(AvifError::Decode("rav1d: кадр не получен".into()));
    loop {
        if data.sz > 0 {
            // SAFETY: контекст и данные валидны; `sz` уменьшается по мере потребления.
            let rc = unsafe { rav1d::src::lib::dav1d_send_data(guard.0, Some(NonNull::from(&mut data))) };
            if rc.0 < 0 && rc.0 != EAGAIN {
                outcome = Err(err("send_data", rc.0));
                break;
            }
        }
        let mut out = MaybeUninit::<Dav1dPicture>::uninit();
        // SAFETY: `dav1d_get_picture` всегда пишет в `out`.
        let rc = unsafe { rav1d::src::lib::dav1d_get_picture(guard.0, Some(NonNull::from(&mut out).cast())) };
        if rc.0 == 0 {
            // SAFETY: при коде 0 картинка инициализирована.
            pic = Some(unsafe { out.assume_init() });
            break;
        }
        if rc.0 != EAGAIN {
            outcome = Err(err("get_picture", rc.0));
            break;
        }
        if data.sz == 0 {
            break; // данные кончились, кадра нет
        }
    }
    // SAFETY: освобождает остаток данных (no-op, если всё потреблено).
    unsafe { rav1d::src::lib::dav1d_data_unref(Some(NonNull::from(&mut data))) };

    let Some(mut pic) = pic else { return outcome };
    let frame = copy_picture(&pic);
    // SAFETY: картинка получена из `dav1d_get_picture` и освобождается один раз.
    unsafe { rav1d::src::lib::dav1d_picture_unref(Some(NonNull::from(&mut pic))) };
    frame
}

fn copy_picture(pic: &rav1d::include::dav1d::picture::Dav1dPicture) -> Result<Frame, AvifError> {
    let w = pic.p.w as usize;
    let h = pic.p.h as usize;
    let layout = pic.p.layout;
    let bpc = pic.p.bpc as u32;
    if w == 0 || h == 0 || !matches!(bpc, 8 | 10 | 12) {
        return Err(AvifError::Decode(format!("rav1d: неподдерживаемый кадр {w}x{h} bpc={bpc}")));
    }
    let (cw, ch) = match layout {
        1 => (w.div_ceil(2), h.div_ceil(2)),
        2 => (w.div_ceil(2), h),
        _ => (w, h),
    };
    let (mtrx, full_range) = match pic.seq_hdr {
        // SAFETY: seq_hdr принадлежит картинке и жив до `picture_unref`.
        Some(p) => unsafe { (p.as_ref().mtrx, p.as_ref().color_range != 0) },
        None => (2, true),
    };
    let nplanes = if layout == 0 { 1 } else { 3 };
    let mut planes: [Vec<u16>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (i, plane) in planes.iter_mut().enumerate().take(nplanes) {
        let (pw, ph) = if i == 0 { (w, h) } else { (cw, ch) };
        let stride = pic.stride[usize::from(i != 0)];
        let base = pic.data[i].ok_or_else(|| AvifError::Decode("rav1d: нет плоскости".into()))?;
        let bytes_per = if bpc == 8 { 1 } else { 2 };
        plane.reserve_exact(pw * ph);
        for y in 0..ph {
            // SAFETY: rav1d гарантирует `ph` строк по `stride` байт, в каждой минимум `pw` сэмплов.
            let row = unsafe {
                core::slice::from_raw_parts(
                    base.as_ptr().cast::<u8>().offset(y as isize * stride),
                    pw * bytes_per,
                )
            };
            if bpc == 8 {
                plane.extend(row.iter().map(|&v| u16::from(v)));
            } else {
                plane.extend(row.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
            }
        }
    }
    Ok(Frame { w, h, layout, bpc, full_range, mtrx, planes, stride: [w, cw] })
}

/// Нормализованное значение (0..1) сэмпла яркости/альфы с учётом диапазона.
fn norm(v: u16, bpc: u32, full_range: bool) -> f32 {
    let v = f32::from(v);
    if full_range {
        v / f32::from((1u16 << bpc) - 1)
    } else {
        let s = f32::from(1u16 << (bpc - 8));
        ((v - 16.0 * s) / (219.0 * s)).clamp(0.0, 1.0)
    }
}

fn to_rgba8(
    c: &Frame,
    alpha: Option<&Frame>,
    premultiplied: bool,
) -> Result<(u32, u32, Vec<u8>), AvifError> {
    if let Some(a) = alpha
        && (a.w != c.w || a.h != c.h)
    {
        return Err(AvifError::Decode("AVIF: размер альфа-канала не совпадает с цветом".into()));
    }
    let maxv = f32::from((1u16 << c.bpc) - 1);
    let scale = f32::from(1u16 << (c.bpc - 8));
    // (Kr, Kb) по matrix_coefficients (ITU-T H.273).
    let (kr, kb) = match c.mtrx {
        4..=6 => (0.299_f32, 0.114_f32),
        7 => (0.212, 0.087),
        9 | 10 => (0.2627, 0.0593),
        _ => (0.2126, 0.0722),
    };
    let kg = 1.0 - kr - kb;
    let identity = c.mtrx == 0 && c.layout == 3;
    let (yoff, yscale, csc, half) = if c.full_range {
        (0.0, 1.0 / maxv, 1.0 / maxv, (maxv + 1.0) / 2.0)
    } else {
        (16.0 * scale, 1.0 / (219.0 * scale), 1.0 / (224.0 * scale), 128.0 * scale)
    };
    let mut out = vec![0u8; c.w * c.h * 4];
    for y in 0..c.h {
        for x in 0..c.w {
            let yv = f32::from(c.planes[0][y * c.stride[0] + x]);
            let (r, g, b) = if c.layout == 0 {
                let l = ((yv - yoff) * yscale).clamp(0.0, 1.0);
                (l, l, l)
            } else {
                let (cx, cy) = match c.layout {
                    1 => (x / 2, y / 2),
                    2 => (x / 2, y),
                    _ => (x, y),
                };
                let ci = cy * c.stride[1] + cx;
                let u = f32::from(c.planes[1][ci]);
                let v = f32::from(c.planes[2][ci]);
                if identity {
                    let n = |s: f32| ((s - yoff) * yscale).clamp(0.0, 1.0);
                    (n(v), n(yv), n(u))
                } else {
                    let yn = (yv - yoff) * yscale;
                    let cb = (u - half) * csc;
                    let cr = (v - half) * csc;
                    let r = yn + 2.0 * (1.0 - kr) * cr;
                    let b = yn + 2.0 * (1.0 - kb) * cb;
                    let g = (yn - kr * r - kb * b) / kg;
                    (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
                }
            };
            let a = alpha.map_or(1.0, |al| norm(al.planes[0][y * al.stride[0] + x], al.bpc, al.full_range));
            let un = if premultiplied && a > 0.0 { 1.0 / a } else { 1.0 };
            let o = (y * c.w + x) * 4;
            out[o] = ((r * un).min(1.0) * 255.0 + 0.5) as u8;
            out[o + 1] = ((g * un).min(1.0) * 255.0 + 0.5) as u8;
            out[o + 2] = ((b * un).min(1.0) * 255.0 + 0.5) as u8;
            out[o + 3] = (a * 255.0 + 0.5) as u8;
        }
    }
    Ok((c.w as u32, c.h as u32, out))
}

/// Реализация [`lumen_core::ext::ImageDecoder`] для AVIF.
///
/// Регистрируется в диспетчере `lumen_image::decode()` и в
/// `supported_mime_types()` для фильтрации `<source type="image/avif">`.
pub struct AvifImageDecoder;

impl lumen_core::ext::ImageDecoder for AvifImageDecoder {
    fn format_name(&self) -> &'static str {
        "avif"
    }

    fn sniff(&self, bytes: &[u8]) -> bool {
        is_avif(bytes)
    }

    fn mime_types(&self) -> &'static [&'static str] {
        &["image/avif"]
    }

    fn decode_rgba8(&self, bytes: &[u8]) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        decode_avif(bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::ImageDecoder as _;

    /// Минимальный ftyp-бокс с major brand `avif`.
    fn make_avif_ftyp_header(brand: &[u8; 4]) -> Vec<u8> {
        let mut v = vec![
            0x00, 0x00, 0x00, 0x18, // box size = 24
            b'f', b't', b'y', b'p', // box type = ftyp
        ];
        v.extend_from_slice(brand); // major brand
        v.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        v.extend_from_slice(b"mif1"); // compatible brand
        v.extend_from_slice(&[0u8; 64]); // payload (garbage — не декодируется)
        v
    }

    #[test]
    fn avif_major_brand_detected() {
        let bytes = make_avif_ftyp_header(b"avif");
        assert!(is_avif(&bytes));
    }

    #[test]
    fn avis_major_brand_detected() {
        let bytes = make_avif_ftyp_header(b"avis");
        assert!(is_avif(&bytes));
    }

    #[test]
    fn other_brand_not_detected() {
        let bytes = make_avif_ftyp_header(b"mp42");
        assert!(!is_avif(&bytes));
    }

    #[test]
    fn too_short_not_detected() {
        assert!(!is_avif(&[]));
        assert!(!is_avif(&[0x00; 11]));
    }

    #[test]
    fn png_not_detected() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert!(!is_avif(png));
    }

    #[test]
    fn webp_not_detected() {
        let webp = b"RIFF\x00\x00\x00\x00WEBP\x00\x00";
        assert!(!is_avif(webp));
    }

    #[test]
    fn jpeg_not_detected() {
        let jpg = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";
        assert!(!is_avif(jpg));
    }

    #[test]
    fn invalid_signature_error_on_non_avif() {
        let result = decode_avif(b"not an avif file at all");
        assert_eq!(result, Err(AvifError::InvalidSignature));
    }

    #[test]
    fn avif_header_but_bad_payload_returns_decode_error() {
        // Сигнатура валидная, но данные мусорные → AvifError::Decode
        let bytes = make_avif_ftyp_header(b"avif");
        let result = decode_avif(&bytes);
        assert!(
            matches!(result, Err(AvifError::Decode(_))),
            "ожидался AvifError::Decode, получено {result:?}"
        );
    }

    #[test]
    fn avif_error_display_invalid_signature() {
        let s = format!("{}", AvifError::InvalidSignature);
        assert!(!s.is_empty());
    }

    #[test]
    fn avif_error_display_decode() {
        let s = format!("{}", AvifError::Decode("test error".to_string()));
        assert!(s.contains("test error"));
    }

    #[test]
    fn image_decoder_trait_format_name() {
        assert_eq!(AvifImageDecoder.format_name(), "avif");
    }

    #[test]
    fn image_decoder_trait_mime_types() {
        assert!(AvifImageDecoder.mime_types().contains(&"image/avif"));
    }

    #[test]
    fn image_decoder_trait_sniff_positive() {
        let bytes = make_avif_ftyp_header(b"avif");
        assert!(AvifImageDecoder.sniff(&bytes));
    }

    #[test]
    fn image_decoder_trait_sniff_negative() {
        assert!(!AvifImageDecoder.sniff(b"not avif"));
    }

    #[test]
    fn image_decoder_trait_decode_error_on_bad_payload() {
        let bytes = make_avif_ftyp_header(b"avif");
        let result = AvifImageDecoder.decode_rgba8(&bytes);
        assert!(result.is_err(), "мусорные данные должны вернуть ошибку");
    }
}
