//! Декодер JPEG/JFIF baseline (SOF0) для Lumen.
//!
//! Свой код, без `jpeg-decoder` / `image` (см. §5 политики в `CLAUDE.md`).
//! Phase 0 покрывает то, что встречается на типовой веб-странице:
//! Baseline DCT (SOF0), 8-битная глубина, YCbCr 3-канальный или Y-only
//! grayscale, sampling factors 1×1/2×2/2×1/1×2, restart markers (DRI/RST).
//!
//! **Не поддерживается:** progressive (SOF2), arithmetic coding,
//! lossless (SOF3), hierarchical, 12-битная глубина, CMYK (4 канала),
//! ICC color profiles (APP2 пропускается, без интерпретации). Всё это
//! реальная веб-страница встречает редко, добавим отдельными задачами.
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
    let pixels = scan::decode_scan(&mut reader, &context)?;

    let format = match context.frame.components.len() {
        1 => PixelFormat::Gray8,
        3 => PixelFormat::Rgb8,
        n => return Err(JpegError::UnsupportedComponentCount(n)),
    };

    Ok(Image {
        width: u32::from(context.frame.width),
        height: u32::from(context.frame.height),
        format,
        data: pixels,
    })
}
