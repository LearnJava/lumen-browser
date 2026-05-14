//! Декодер JPEG/JFIF baseline (SOF0) и progressive (SOF2) для Lumen.
//!
//! Свой код, без `jpeg-decoder` / `image` (см. §5 политики в `CLAUDE.md`).
//! Покрывает то, что встречается на типовой веб-странице: Baseline / Progressive
//! Huffman DCT, 8-битная глубина, YCbCr 3-канальный или Y-only grayscale,
//! sampling factors 1×1/2×2/2×1/1×2, restart markers (DRI/RST), переопределение
//! Huffman-таблиц между progressive-scan-ами.
//!
//! **Не поддерживается:** arithmetic coding (SOF9-11), lossless (SOF3),
//! hierarchical (SOF5-7), 12-битная глубина, CMYK (4 канала), ICC color
//! profiles (APP2 пропускается, без интерпретации).
//!
//! Декодер не паникует на повреждённом входе — каждая ошибка возвращается
//! как `JpegError` с конкретной причиной.
//!
//! ## Поток исполнения
//!
//! 1. `read_segments` — последовательное чтение marker-segments до SOS.
//!    Накапливает `DQT` (quantization tables), `DHT` (Huffman tables),
//!    `SOF0` (параметры frame-а), `DRI` (restart interval).
//! 2. `decode_scan` после SOS — entropy-coded data: bit-by-bit Huffman
//!    decode → DC/AC коэффициенты → de-zigzag + dequantize → IDCT →
//!    YCbCr-блок → запись в output buffer (с chroma upsampling по
//!    sampling factors).
//! 3. EOI завершает поток.

mod bit_reader;
mod color;
mod huffman;
mod idct;
mod marker;
mod progressive;
mod scan;

use crate::{Image, PixelFormat};

pub use self::marker::JpegError;

/// Декодирует JPEG/JFIF baseline → `Image` (`Gray8` для 1-компонентных
/// файлов, `Rgb8` для 3-компонентных YCbCr).
///
/// # Errors
///
/// Возвращает `JpegError` при любом нарушении формата — обрезанный поток,
/// неизвестный marker, неподдерживаемый профиль (не SOF0), повреждённые
/// quantization / Huffman таблицы, выход за границы изображения и пр.
pub fn decode_jpeg(bytes: &[u8]) -> Result<Image, JpegError> {
    let mut reader = marker::SegmentReader::new(bytes);
    let context = reader.read_until_scan()?;

    let format = match context.frame.components.len() {
        1 => PixelFormat::Gray8,
        3 => PixelFormat::Rgb8,
        n => return Err(JpegError::UnsupportedComponentCount(n)),
    };
    let width = u32::from(context.frame.width);
    let height = u32::from(context.frame.height);

    let pixels = if context.frame.is_progressive {
        progressive::decode_progressive(&mut reader, context)?
    } else {
        scan::decode_scan(&mut reader, &context)?
    };

    Ok(Image {
        width,
        height,
        format,
        data: pixels,
    })
}
