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
