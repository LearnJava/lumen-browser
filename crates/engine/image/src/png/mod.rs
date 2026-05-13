//! PNG-декодер по спецификации
//! <https://www.w3.org/TR/png-3/> (формальный текст ISO/IEC 15948).
//!
//! Структура файла PNG:
//! - 8 байтов сигнатуры `89 50 4E 47 0D 0A 1A 0A`,
//! - последовательность чанков: 4 байта длины (BE, без типа и CRC),
//!   4 байта типа (ASCII), `length` байтов данных, 4 байта CRC32 на
//!   `type || data`. Длина < 2^31.
//! - Первый чанк — `IHDR` (13 байтов): размеры, глубина, color type,
//!   compression/filter/interlace методы.
//! - `IDAT` (один или несколько последовательных) содержит zlib-сжатую
//!   последовательность фильтрованных скан-линий.
//! - `IEND` — пустой, маркирует конец.
//!
//! Phase 0 ограничивается реальными случаями современного веба: 8-битные
//! RGB / RGBA / grayscale / grayscale+alpha, фильтры 0–4, без interlacing,
//! без палитры. Этого хватает для CSS-ассетов и фотографий, опубликованных
//! современными CMS. 16-битная глубина и Adam7 поддерживаются по
//! необходимости отдельными PR.

pub(crate) mod chunk;
pub(crate) mod filter;
pub(crate) mod ihdr;
pub(crate) mod inflate;

use crate::{DecodeError, Image, IhdrError};

/// 8-байтовая PNG-сигнатура. Первый байт `0x89` имеет старший бит, чтобы
/// текстовые транспорты сразу определяли бинарный поток; далее ASCII `PNG`;
/// `\r\n` детектирует кривую конверсию переводов строк; `0x1a` (Ctrl-Z)
/// останавливает DOS `type`; `\n` детектирует «обратную» CR-вырезку.
pub(crate) const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Декодировать PNG-поток в `Image`. Поддержаны цветовые типы
/// grayscale / grayscale+alpha / RGB / RGBA при `bit_depth = 8`, без
/// interlacing и без палитры. Прочие комбинации возвращаются как
/// `Unsupported(...)`.
///
/// Алгоритм:
/// 1. Проверяем 8-байтовую сигнатуру.
/// 2. Первый чанк должен быть `IHDR` — парсим заголовок и определяем
///    `PixelFormat`. Если он `Unsupported`, выходим сразу.
/// 3. Сканируем чанки: `IDAT` собираются (PNG разрешает любое число
///    подряд идущих IDAT-чанков, склеиваемых в один zlib-поток);
///    auxiliary-чанки (sRGB, pHYs, tEXt, и т.п.) игнорируются —
///    PNG-spec явно разрешает; `IEND` маркирует конец.
/// 4. Inflate-им конкатенированные IDAT через свой zlib decoder.
/// 5. Развёртываем фильтры скан-линий → плотный row-major массив.
pub fn decode_png(bytes: &[u8]) -> Result<Image, DecodeError> {
    let after_sig = chunk::read_signature(bytes)?;
    let mut reader = chunk::ChunkReader::new(after_sig);

    // Первый чанк обязательно IHDR.
    let first = reader
        .next_chunk()
        .ok_or(DecodeError::UnexpectedEof)??;
    if &first.kind != b"IHDR" {
        return Err(DecodeError::BadIhdr(IhdrError::WrongLength(
            u32::try_from(first.data.len()).unwrap_or(u32::MAX),
        )));
    }
    let header = ihdr::Ihdr::parse(first.data)?;
    let format = header.pixel_format()?;

    let mut idat: Vec<u8> = Vec::new();
    let mut seen_iend = false;
    while let Some(chunk_result) = reader.next_chunk() {
        let c = chunk_result?;
        match &c.kind {
            b"IDAT" => idat.extend_from_slice(c.data),
            b"IEND" => {
                seen_iend = true;
                break;
            }
            _ => {
                // PNG §11.3: чанки, чьё имя начинается со строчной первой
                // буквы (ancillary), безопасно игнорировать. Критические
                // (PLTE, и т.д.) нам пока не нужны: палитра уже отвергнута
                // в Ihdr::pixel_format(), а tRNS / cHRM / gAMA / pHYs /
                // sRGB / iCCP / tEXt — стилевые/метаданные, не влияющие
                // на декодирование RGB(A) / grayscale на 8 бит.
            }
        }
    }
    if !seen_iend {
        return Err(DecodeError::NoEndChunk);
    }
    if idat.is_empty() {
        return Err(DecodeError::NoImageData);
    }

    let raw = inflate::inflate_zlib(&idat).map_err(DecodeError::BadDeflate)?;
    let pixels = filter::unfilter(
        &raw,
        header.width,
        header.height,
        format.bytes_per_pixel(),
    )?;

    Ok(Image {
        width: header.width,
        height: header.height,
        format,
        data: pixels,
    })
}
