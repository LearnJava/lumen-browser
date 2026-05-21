//! Декодер растровых изображений для Lumen.
//!
//! Реализуется самостоятельно, без `image` / `png` / `jpeg-decoder` (см. §5
//! политики зависимостей в `CLAUDE.md`). Phase 0 покрывает PNG для случаев,
//! которые реально встречаются на современных веб-страницах:
//! grayscale / grayscale + alpha / RGB / RGBA при `bit_depth ∈ {8, 16}` +
//! palette (color_type 3) при `bit_depth ∈ {1, 2, 4, 8}` + опциональный
//! `tRNS` для прозрачности. 16-битные сэмплы downsample-ятся в 8-битные
//! отбрасыванием младшего байта (libpng `PNG_TRANSFORM_STRIP_16`). Фильтры
//! 0–4 по спецификации. **Adam7-interlacing поддерживается** для всех
//! поддерживаемых color types / bit-depths. JPEG добавляется отдельной задачей.
//!
//! Декодер не паникует на повреждённом входе — каждая ошибка возвращается
//! как `DecodeError` с конкретной причиной.

mod jpeg;
mod png;

pub use jpeg::{decode_jpeg, JpegError};
pub use png::decode_png;

/// PNG-сигнатура: `89 50 4E 47 0D 0A 1A 0A` (PNG §5.2).
pub const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// JPEG SOI + начало следующего маркера: `FF D8 FF` (ISO/IEC 10918-1 §B.1.1.3 +
/// B.2.4 — `FF D8` это SOI, далее обязан идти ещё один маркер, поэтому
/// третий байт всегда `FF`). Проверка трёх байт даёт надёжный sniff: одиночные
/// `FF D8` без продолжения встречаются в случайных бинарниках, а `FF D8 FF` —
/// уже специфично для JPEG.
pub const JPEG_SIGNATURE_PREFIX: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// Декодирует растровое изображение, определяя формат по сигнатуре первых
/// байтов: PNG (`89 50 4E 47 0D 0A 1A 0A`) либо JPEG (`FF D8 FF`).
///
/// Если сигнатура не совпала ни с одной из поддерживаемых, либо вход короче
/// нужного для распознавания, возвращается `ImageError::UnknownFormat`. Это
/// поведение отличается от `decode_png(bytes)`, который при «не PNG» отдаёт
/// `DecodeError::InvalidSignature`: общий dispatch более снисходителен и
/// перекладывает решение «как реагировать на чужой формат» на caller-а.
///
/// # Errors
/// - [`ImageError::UnknownFormat`] — сигнатура неизвестна (вкл. слишком короткий вход).
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

/// Ошибка `decode` — либо unknown format, либо проброшенная ошибка
/// конкретного декодера. Имеет `Display`, чтобы caller мог просто
/// `format!("{err}")` без match-а.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Первые байты не похожи ни на одну из известных сигнатур, либо вход
    /// слишком короток. Конкретного формата по этому ответу определить нельзя.
    UnknownFormat,
    /// PNG-сигнатура распознана, но декодер вернул ошибку.
    Png(DecodeError),
    /// JPEG-сигнатура распознана, но декодер вернул ошибку.
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
    fn from(e: DecodeError) -> Self {
        Self::Png(e)
    }
}

impl From<JpegError> for ImageError {
    fn from(e: JpegError) -> Self {
        Self::Jpeg(e)
    }
}

/// Декодированное растровое изображение в плотной row-major упаковке без
/// padding-а между строками. Длина `data` равна `width * height *
/// bytes_per_pixel(format)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl Image {
    /// Возвращает пиксели в формате RGBA8 (4 байта на пиксель).
    /// Если формат уже `Rgba8` — копирует буфер без преобразования.
    #[must_use]
    pub fn to_rgba8(&self) -> Vec<u8> {
        let pixel_count = self.width as usize * self.height as usize;
        let mut out = Vec::with_capacity(pixel_count * 4);
        match self.format {
            PixelFormat::Gray8 => {
                for &g in &self.data {
                    out.extend_from_slice(&[g, g, g, 255]);
                }
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
            PixelFormat::Rgba8 => {
                out.extend_from_slice(&self.data);
            }
        }
        out
    }
}

/// Масштабирует `src` до `(dst_w × dst_h)` билинейной интерполяцией.
///
/// Возвращает новый [`Image`] в формате [`PixelFormat::Rgba8`].
/// Если `dst_w` или `dst_h` равны нулю, подставляется 1 (deg fallback).
/// Маппинг центров пикселей (браузерная конвенция):
/// выходной пиксель `(dx, dy)` соответствует источнику
/// `(dx + 0.5) * src_w / dst_w - 0.5`.
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

/// Формат пикселя декодированного изображения. Все варианты — 8 бит на канал.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 1 канал: яркость.
    Gray8,
    /// 2 канала: яркость + alpha.
    GrayAlpha8,
    /// 3 канала: R, G, B.
    Rgb8,
    /// 4 канала: R, G, B, A.
    Rgba8,
}

impl PixelFormat {
    /// Количество байтов на пиксель.
    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::GrayAlpha8 => 2,
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }

    /// Количество каналов в пикселе.
    #[must_use]
    pub const fn channels(self) -> usize {
        self.bytes_per_pixel()
    }
}

/// Ошибки декодирования.
///
/// Каждый вариант указывает конкретное место поломки: парсер сигнатуры,
/// длина чанка, CRC, неподдерживаемая комбинация bit_depth + color_type, и т.д.
/// Это даёт пользователю крепкое сообщение для диагностики и не теряет
/// контекст при пропагации через `?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Первые 8 байтов не равны PNG-сигнатуре `89 50 4E 47 0D 0A 1A 0A`.
    InvalidSignature,
    /// Обрезанный поток: ожидалось больше байтов, чем дано.
    UnexpectedEof,
    /// CRC32 чанка не совпал с записанным значением.
    BadCrc {
        chunk_type: [u8; 4],
        expected: u32,
        actual: u32,
    },
    /// Длина чанка превышает разумные пределы (>= 2^31 запрещено
    /// спецификацией PNG §11.2.2).
    ChunkTooLong { len: u32 },
    /// IHDR должен быть первым чанком и иметь длину 13 байтов.
    BadIhdr(IhdrError),
    /// Файл не содержит ни одного `IDAT`-чанка.
    NoImageData,
    /// IEND-чанк отсутствует, поток оборвался раньше.
    NoEndChunk,
    /// В Phase 0 не поддерживается: interlacing, palette, 16-bit, и пр.
    Unsupported(UnsupportedReason),
    /// Поток DEFLATE/zlib повреждён.
    BadDeflate(InflateError),
    /// Неверный тип фильтра скан-линии (допустимы 0..=4).
    BadFilter { row: u32, kind: u8 },
    /// IDAT расшифровался в неожиданное количество байтов (нарушает
    /// width × height × bpp + height фильтрующих байтов).
    BadImageDataSize { expected: usize, actual: usize },
    /// Проблема с палитрой (PLTE / tRNS) — детали в `PaletteError`.
    BadPalette(PaletteError),
}

/// Детализированные причины ошибки IHDR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IhdrError {
    /// Длина чанка не равна 13.
    WrongLength(u32),
    /// Ширина или высота равна нулю (запрещено PNG §11.2.2).
    ZeroDimension,
    /// `compression_method` ≠ 0 (PNG предусматривает только метод 0 = deflate).
    BadCompression(u8),
    /// `filter_method` ≠ 0.
    BadFilter(u8),
    /// Сочетание `bit_depth` + `color_type` не соответствует таблице §11.2.2.
    BadBitDepthForColorType { bit_depth: u8, color_type: u8 },
    /// Неизвестный `color_type` (допустимы 0, 2, 3, 4, 6).
    UnknownColorType(u8),
    /// Неизвестный `interlace_method` (допустимы 0 и 1).
    UnknownInterlace(u8),
}

/// Что именно не поддерживается на текущем этапе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedReason {
    /// 1/2/4-битная глубина — реализуема, но Phase 0 ограничен 8 (касается
    /// и grayscale, и palette).
    SubByteDepth(u8),
}

/// Детализированные причины ошибки палитры (`PLTE` / `tRNS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteError {
    /// `PLTE` отсутствует, а `color_type = 3` требует палитры (PNG §11.2.3).
    MissingForIndexed,
    /// `PLTE` присутствует у grayscale `color_type = 0 / 4`,
    /// что запрещено (PNG §11.3.2).
    UnexpectedForGrayscale,
    /// Длина `PLTE` не делится на 3 (palette хранит triples R/G/B).
    BadPlteLength(u32),
    /// `PLTE` содержит более 256 entries или 0 entries
    /// (PNG ограничивает 1..=256).
    PlteOutOfRange(usize),
    /// `tRNS` встретился раньше `PLTE` (нарушение ordering PNG §11.3.2).
    TrnsBeforePlte,
    /// `tRNS` содержит больше alpha-значений, чем entries в `PLTE`.
    TrnsTooLong { plte_count: usize, trns_count: usize },
    /// Дублирующийся `PLTE` или `tRNS` чанк.
    DuplicateChunk { kind: [u8; 4] },
    /// Палитровый индекс за пределами `PLTE` — повреждённый PNG-файл.
    IndexOutOfRange { row: u32, col: u32, index: u8, plte_count: usize },
    /// `tRNS` для color_type 0 (grayscale) должен содержать ровно 2 байта
    /// (один u16 big-endian — gray sample считающийся прозрачным).
    BadTrnsLengthForGrayscale(u32),
    /// `tRNS` для color_type 2 (RGB) должен содержать ровно 6 байт
    /// (три u16 big-endian — RGB-color считающийся прозрачным).
    BadTrnsLengthForRgb(u32),
    /// `tRNS` для color_type 4 / 6 запрещён PNG §11.3.2.1 — alpha уже есть в пикселе.
    UnexpectedForAlphaType,
}

/// Ошибки парсера DEFLATE/zlib (RFC 1950, 1951).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InflateError {
    /// zlib-заголовок (2 байта CMF + FLG) не валиден.
    BadZlibHeader,
    /// Несоответствие adler-32 в конце zlib-потока.
    BadAdler32 { expected: u32, actual: u32 },
    /// Запрещённый BTYPE = 11 (зарезервирован спецификацией).
    ReservedBlockType,
    /// LEN ≠ ~NLEN в stored-блоке.
    BadStoredLength,
    /// Bitstream закончился раньше, чем декодер успел дочитать символ.
    UnexpectedEndOfBitstream,
    /// Канонические коды Huffman не валидны (превышение бюджета,
    /// слишком длинные коды и т.п.).
    BadHuffmanCodes,
    /// distance указывает за начало уже декодированных данных.
    DistanceTooFar,
    /// Неверный код длины / дистанции (>= 30 для distance, > 285 для length).
    BadLengthOrDistanceCode,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "не PNG: сигнатура не совпала"),
            Self::UnexpectedEof => write!(f, "обрезанный поток"),
            Self::BadCrc {
                chunk_type,
                expected,
                actual,
            } => write!(
                f,
                "CRC32 не совпал в чанке {:?}: ожидалось {:#x}, получено {:#x}",
                core::str::from_utf8(chunk_type).unwrap_or("<?>"),
                expected,
                actual
            ),
            Self::ChunkTooLong { len } => write!(f, "длина чанка {len} превышает 2^31"),
            Self::BadIhdr(e) => write!(f, "IHDR: {e:?}"),
            Self::NoImageData => write!(f, "файл не содержит IDAT"),
            Self::NoEndChunk => write!(f, "не найден IEND"),
            Self::Unsupported(r) => write!(f, "не поддерживается в Phase 0: {r:?}"),
            Self::BadDeflate(e) => write!(f, "DEFLATE: {e:?}"),
            Self::BadFilter { row, kind } => {
                write!(f, "неизвестный фильтр {kind} в строке {row}")
            }
            Self::BadImageDataSize { expected, actual } => {
                write!(f, "ожидалось {expected} байтов IDAT, получено {actual}")
            }
            Self::BadPalette(e) => write!(f, "PLTE/tRNS: {e:?}"),
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
        // 7 байт — короче PNG-сигнатуры (8) и не подходит под JPEG (нужно FF D8 FF).
        let bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A];
        assert_eq!(decode(&bytes), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn jpeg_soi_without_third_byte_unknown() {
        // FF D8 без FF — недостаточно для JPEG dispatch.
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
        // PNG-сигнатура совпадает, дальше декодер падает на IHDR — это
        // нормальный path, важно что dispatch ушёл в PNG, а не вернул UnknownFormat.
        let mut bytes = Vec::from(PNG_SIGNATURE);
        bytes.extend_from_slice(&[0x00; 4]); // обрывающаяся длина чанка
        let err = decode(&bytes).unwrap_err();
        assert!(matches!(err, ImageError::Png(_)), "ожидался Png(_), получено {err:?}");
    }

    #[test]
    fn jpeg_signature_dispatches_to_jpeg_decoder() {
        // SOI + FF — dispatch уйдёт в JPEG, который упрётся в обрезанный поток.
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
}
