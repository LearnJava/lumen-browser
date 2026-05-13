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
//! - Для `color_type = 3` обязателен `PLTE` чанк до `IDAT` (PNG §11.2.3);
//!   опциональный `tRNS` (после `PLTE`, до `IDAT`) делает entries
//!   полу-/полностью прозрачными.
//! - `IDAT` (один или несколько последовательных) содержит zlib-сжатую
//!   последовательность фильтрованных скан-линий.
//! - `IEND` — пустой, маркирует конец.
//!
//! Phase 0 ограничивается реальными случаями современного веба: 8-битные
//! RGB / RGBA / grayscale / grayscale+alpha **и palette (color_type 3)
//! с bit_depth=8 + опц. tRNS**, фильтры 0–4, без interlacing. 16-битная
//! глубина, 1/2/4-битная palette и Adam7 — отдельными задачами.

pub(crate) mod chunk;
pub(crate) mod filter;
pub(crate) mod ihdr;
pub(crate) mod inflate;
pub(crate) mod palette;

use crate::{DecodeError, Image, IhdrError, PaletteError, PixelFormat};

/// 8-байтовая PNG-сигнатура. Первый байт `0x89` имеет старший бит, чтобы
/// текстовые транспорты сразу определяли бинарный поток; далее ASCII `PNG`;
/// `\r\n` детектирует кривую конверсию переводов строк; `0x1a` (Ctrl-Z)
/// останавливает DOS `type`; `\n` детектирует «обратную» CR-вырезку.
pub(crate) const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Декодировать PNG-поток в `Image`. Поддержаны цветовые типы
/// grayscale / grayscale+alpha / RGB / RGBA / palette при `bit_depth = 8`,
/// без interlacing. Прочие комбинации возвращаются как `Unsupported(...)`
/// или `BadPalette(...)`.
///
/// Алгоритм:
/// 1. Проверяем 8-байтовую сигнатуру.
/// 2. Первый чанк должен быть `IHDR` — парсим заголовок.
/// 3. Сканируем чанки: `PLTE` → палитра (только для color_type 3 семантически
///    значима; для grayscale 0/4 — ошибка; для RGB 2/6 — «suggested palette»,
///    игнорируется). `tRNS` после `PLTE` (в палитровом контексте — alpha-
///    таблица). `IDAT` собираются в один zlib-поток. Auxiliary-чанки
///    (sRGB / pHYs / tEXt / iCCP / cHRM / …) игнорируются (PNG §11.3 —
///    safe-to-ignore). `IEND` маркирует конец.
/// 4. Inflate-им конкатенированные IDAT через свой zlib decoder.
/// 5. Развёртываем фильтры скан-линий → плотный row-major массив байтов.
/// 6. Для color_type 3 — расширяем индексы в Rgb8 (без tRNS) / Rgba8 (с tRNS).
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
    let is_palette = matches!(header.color_type, ihdr::ColorType::Palette);

    let mut idat: Vec<u8> = Vec::new();
    let mut plte: Option<Vec<[u8; 3]>> = None;
    let mut trns_palette: Option<Vec<u8>> = None;
    let mut seen_iend = false;

    while let Some(chunk_result) = reader.next_chunk() {
        let c = chunk_result?;
        match &c.kind {
            b"IDAT" => idat.extend_from_slice(c.data),
            b"PLTE" => {
                if plte.is_some() {
                    return Err(DecodeError::BadPalette(PaletteError::DuplicateChunk {
                        kind: *b"PLTE",
                    }));
                }
                // PNG §11.3.2: PLTE для color_type 0 / 4 — ошибка.
                if matches!(
                    header.color_type,
                    ihdr::ColorType::Grayscale | ihdr::ColorType::GrayscaleAlpha
                ) {
                    return Err(DecodeError::BadPalette(
                        PaletteError::UnexpectedForGrayscale,
                    ));
                }
                plte = Some(palette::parse_plte(c.data)?);
            }
            b"tRNS" if is_palette => {
                // Палитровый tRNS: alpha-таблица. Должен идти после PLTE.
                let plte_count = match &plte {
                    Some(p) => p.len(),
                    None => {
                        return Err(DecodeError::BadPalette(PaletteError::TrnsBeforePlte));
                    }
                };
                if trns_palette.is_some() {
                    return Err(DecodeError::BadPalette(PaletteError::DuplicateChunk {
                        kind: *b"tRNS",
                    }));
                }
                trns_palette = Some(palette::parse_trns_palette(c.data, plte_count)?);
            }
            b"IEND" => {
                seen_iend = true;
                break;
            }
            _ => {
                // Прочие чанки безопасно игнорируем: ancillary-метаданные
                // (sRGB / gAMA / pHYs / tEXt / iCCP / cHRM), tRNS для
                // не-палитровых типов (single-color transparency для
                // RGB/Gray — Phase 0 не реализована, файл рендерится
                // как непрозрачный), suggested palette для RGB и пр.
            }
        }
    }
    if !seen_iend {
        return Err(DecodeError::NoEndChunk);
    }
    if idat.is_empty() {
        return Err(DecodeError::NoImageData);
    }

    // Для палитры формат пиксельного выхода зависит от наличия tRNS.
    // Прочие случаи делегируем существующему `Ihdr::pixel_format()`.
    let (format, bpp_for_unfilter) = if is_palette {
        // bit_depth = 8 проверяется здесь, потому что pixel_format() мы для
        // палитры не зовём (он по-прежнему отвергает Palette как контракт
        // «вызывать только для не-палитровых»).
        if header.bit_depth != 8 {
            return Err(DecodeError::Unsupported(
                crate::UnsupportedReason::SubByteDepth(header.bit_depth),
            ));
        }
        if header.interlaced {
            return Err(DecodeError::Unsupported(
                crate::UnsupportedReason::Interlaced,
            ));
        }
        if plte.is_none() {
            return Err(DecodeError::BadPalette(PaletteError::MissingForIndexed));
        }
        let fmt = if trns_palette.is_some() {
            PixelFormat::Rgba8
        } else {
            PixelFormat::Rgb8
        };
        // Палитровый поток — 1 байт = 1 пиксель (палитровый индекс).
        (fmt, 1usize)
    } else {
        let fmt = header.pixel_format()?;
        (fmt, fmt.bytes_per_pixel())
    };

    let raw = inflate::inflate_zlib(&idat).map_err(DecodeError::BadDeflate)?;
    let unfiltered =
        filter::unfilter(&raw, header.width, header.height, bpp_for_unfilter)?;

    let pixels = if is_palette {
        // unwrap-ы безопасны: plte проверен выше, format определён через tRNS.
        let plte_ref = plte.as_ref().unwrap();
        if let Some(alpha) = trns_palette.as_deref() {
            palette::expand_to_rgba(&unfiltered, plte_ref, alpha, header.width)?
        } else {
            palette::expand_to_rgb(&unfiltered, plte_ref, header.width)?
        }
    } else {
        unfiltered
    };

    Ok(Image {
        width: header.width,
        height: header.height,
        format,
        data: pixels,
    })
}
