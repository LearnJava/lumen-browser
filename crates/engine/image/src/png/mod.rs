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
//! Phase 0 покрывает grayscale / grayscale+alpha / RGB / RGBA при
//! `bit_depth ∈ {8, 16}` (16-битные сэмплы downsample-ятся до 8 бит на
//! канал отбрасыванием младшего байта), palette (color_type 3) при
//! `bit_depth ∈ {1, 2, 4, 8}` с опциональным `tRNS`, sub-byte depth
//! (1/2/4) для grayscale; всё без interlacing. Adam7 — отдельной задачей.

pub(crate) mod chunk;
pub(crate) mod filter;
pub(crate) mod ihdr;
pub(crate) mod inflate;
pub(crate) mod palette;
pub(crate) mod sub_byte;
pub(crate) mod trns_nonpalette;

use crate::{DecodeError, Image, IhdrError, PaletteError, PixelFormat};

/// 8-байтовая PNG-сигнатура. Первый байт `0x89` имеет старший бит, чтобы
/// текстовые транспорты сразу определяли бинарный поток; далее ASCII `PNG`;
/// `\r\n` детектирует кривую конверсию переводов строк; `0x1a` (Ctrl-Z)
/// останавливает DOS `type`; `\n` детектирует «обратную» CR-вырезку.
pub(crate) const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Декодировать PNG-поток в `Image`. Поддержаны цветовые типы grayscale /
/// grayscale+alpha / RGB / RGBA / palette; для grayscale и palette — также
/// bit_depth 1/2/4 (sub-byte unpack + scaling); для всех не-палитровых
/// типов — bit_depth 16 (downsample до 8 бит); прочие комбинации
/// возвращаются как `Unsupported(...)` или `BadPalette(...)`.
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
/// 6. При sub-byte depth — распаковываем биты в один байт на сэмпл.
/// 7. Для grayscale при sub-byte — масштабируем сэмплы до полного 8-битного
///    диапазона (PNG §13.12).
/// 8. При 16-bit — downsample до 8 бит на канал (отбрасываем младший байт).
/// 9. Для color_type 3 — расширяем индексы в Rgb8 (без tRNS) / Rgba8 (с tRNS).
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
    let is_grayscale = matches!(header.color_type, ihdr::ColorType::Grayscale);

    let mut idat: Vec<u8> = Vec::new();
    let mut plte: Option<Vec<[u8; 3]>> = None;
    let mut trns_palette: Option<Vec<u8>> = None;
    let mut trns_grayscale: Option<u16> = None;
    let mut trns_rgb: Option<(u16, u16, u16)> = None;
    let mut seen_trns = false;
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
            b"tRNS" => {
                if seen_trns {
                    return Err(DecodeError::BadPalette(PaletteError::DuplicateChunk {
                        kind: *b"tRNS",
                    }));
                }
                seen_trns = true;
                match header.color_type {
                    ihdr::ColorType::Palette => {
                        // Палитровый tRNS: alpha-таблица. Должен идти после PLTE.
                        let plte_count = match &plte {
                            Some(p) => p.len(),
                            None => {
                                return Err(DecodeError::BadPalette(
                                    PaletteError::TrnsBeforePlte,
                                ));
                            }
                        };
                        trns_palette =
                            Some(palette::parse_trns_palette(c.data, plte_count)?);
                    }
                    ihdr::ColorType::Grayscale => {
                        trns_grayscale =
                            Some(trns_nonpalette::parse_trns_grayscale(c.data)?);
                    }
                    ihdr::ColorType::Rgb => {
                        trns_rgb = Some(trns_nonpalette::parse_trns_rgb(c.data)?);
                    }
                    ihdr::ColorType::GrayscaleAlpha | ihdr::ColorType::Rgba => {
                        return Err(DecodeError::BadPalette(
                            PaletteError::UnexpectedForAlphaType,
                        ));
                    }
                }
            }
            b"IEND" => {
                seen_iend = true;
                break;
            }
            _ => {
                // Прочие чанки безопасно игнорируем: ancillary-метаданные
                // (sRGB / gAMA / pHYs / tEXt / iCCP / cHRM),
                // suggested palette для RGB и пр.
            }
        }
    }
    if !seen_iend {
        return Err(DecodeError::NoEndChunk);
    }
    if idat.is_empty() {
        return Err(DecodeError::NoImageData);
    }

    // Раннее отбраковывание неподдерживаемых вариантов, общих для всех
    // color types. Interlaced ещё валиден по IHDR §11.2.2 table 11.1,
    // но не реализован в Phase 0.
    if header.interlaced {
        return Err(DecodeError::Unsupported(
            crate::UnsupportedReason::Interlaced,
        ));
    }
    let is_sub_byte = matches!(header.bit_depth, 1 | 2 | 4);
    let is_16bit = header.bit_depth == 16;
    // Sub-byte допустим только для grayscale (color_type 0) и palette (3),
    // 16-bit — только для не-палитровых (0/2/4/6) — по IHDR §11.2.2 table 11.1
    // (уже проверено в Ihdr::parse(); здесь невозможные комбинации не встречаются).

    // Определяем выходной формат и параметры фильтра.
    let format = if is_palette {
        if plte.is_none() {
            return Err(DecodeError::BadPalette(PaletteError::MissingForIndexed));
        }
        if trns_palette.is_some() {
            PixelFormat::Rgba8
        } else {
            PixelFormat::Rgb8
        }
    } else if is_grayscale {
        // grayscale при любом из 1/2/4/8/16 → Gray8 (или GrayAlpha8 если есть tRNS).
        if trns_grayscale.is_some() {
            PixelFormat::GrayAlpha8
        } else {
            PixelFormat::Gray8
        }
    } else if matches!(header.color_type, ihdr::ColorType::Rgb) {
        // RGB при bit_depth ∈ {8, 16}.
        if trns_rgb.is_some() {
            PixelFormat::Rgba8
        } else {
            PixelFormat::Rgb8
        }
    } else {
        // GrayA / RGBA при bit_depth ∈ {8, 16}.
        header.pixel_format()?
    };

    // PNG §9.2: filter operates на байтовом уровне, с offset = bpp байт
    // для Sub / Average / Paeth. Для sub-byte депт спецификация явно
    // задаёт bpp = 1 байт; для 8+ бит — channels × ceil(bit_depth/8).
    let channels = match header.color_type {
        ihdr::ColorType::Grayscale | ihdr::ColorType::Palette => 1u32,
        ihdr::ColorType::GrayscaleAlpha => 2,
        ihdr::ColorType::Rgb => 3,
        ihdr::ColorType::Rgba => 4,
    };
    let bits_per_scanline =
        u64::from(header.width) * u64::from(channels) * u64::from(header.bit_depth);
    let scanline_bytes = u32::try_from(bits_per_scanline.div_ceil(8))
        .map_err(|_| DecodeError::BadImageDataSize {
            expected: 0,
            actual: idat.len(),
        })?;
    let filter_bpp =
        (usize::from(header.bit_depth) * channels as usize).max(8) / 8;
    // Для filter::unfilter передаём «ширину байт-блока» и bpp так, чтобы
    // их произведение равнялось scanline_bytes. Для 8+ бит это совпадает
    // с пиксельной шириной; для sub-byte — bpp=1 и filter_width = scanline_bytes.
    let filter_width = scanline_bytes / filter_bpp as u32;

    let raw = inflate::inflate_zlib(&idat).map_err(DecodeError::BadDeflate)?;
    let unfiltered = filter::unfilter(&raw, filter_width, header.height, filter_bpp)?;

    // При sub-byte распаковываем биты в один байт на сэмпл; при 16-bit
    // downsample-им до 8 бит на канал отбрасыванием младшего байта (PNG
    // хранит сэмплы big-endian, поэтому high byte всегда первый из пары).
    // Это эквивалент `PNG_TRANSFORM_STRIP_16` в libpng — небольшая потеря
    // точности (0xFFFE → 0xFF, не 0xFE), но достаточно для веб-отображения.
    // Иначе — скан-линии уже плотные.
    let samples = if is_sub_byte {
        let mut s =
            sub_byte::unpack_bits(&unfiltered, header.width, header.height, header.bit_depth);
        if is_grayscale {
            sub_byte::scale_grayscale_to_8bit(&mut s, header.bit_depth);
        }
        s
    } else if is_16bit {
        downsample_16bit_to_8bit(&unfiltered)
    } else {
        unfiltered
    };

    let pixels = if is_palette {
        // unwrap-ы безопасны: plte проверен выше, format определён через tRNS.
        let plte_ref = plte.as_ref().unwrap();
        if let Some(alpha) = trns_palette.as_deref() {
            palette::expand_to_rgba(&samples, plte_ref, alpha, header.width)?
        } else {
            palette::expand_to_rgb(&samples, plte_ref, header.width)?
        }
    } else if let Some(raw) = trns_grayscale {
        // Non-palette grayscale + tRNS → GrayAlpha8.
        let transparent = trns_nonpalette::normalize_trns_value_to_8bit(raw, header.bit_depth);
        trns_nonpalette::expand_gray_with_trns(&samples, transparent)
    } else if let Some((r, g, b)) = trns_rgb {
        // Non-palette RGB + tRNS → Rgba8.
        let transparent = (
            trns_nonpalette::normalize_trns_value_to_8bit(r, header.bit_depth),
            trns_nonpalette::normalize_trns_value_to_8bit(g, header.bit_depth),
            trns_nonpalette::normalize_trns_value_to_8bit(b, header.bit_depth),
        );
        trns_nonpalette::expand_rgb_with_trns(&samples, transparent)
    } else {
        samples
    };

    Ok(Image {
        width: header.width,
        height: header.height,
        format,
        data: pixels,
    })
}

/// Понизить 16-битные сэмплы до 8-битных, оставив только high-byte каждой
/// пары. PNG хранит u16 в big-endian — high byte идёт первым; результат имеет
/// ровно половину исходной длины. Эквивалент `PNG_TRANSFORM_STRIP_16` в
/// libpng. Альтернатива (`((s + 128) / 257) as u8`, точное округление) даёт
/// чуть более правильное визуальное значение для интенсивностей вроде 0xFF80,
/// но усложняет код без выигрыша для типичных веб-сценариев.
fn downsample_16bit_to_8bit(samples_be16: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples_be16.len() / 2);
    for pair in samples_be16.chunks_exact(2) {
        out.push(pair[0]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::downsample_16bit_to_8bit;

    #[test]
    fn downsample_takes_high_byte_only() {
        let input = vec![0xFF, 0x80, 0x00, 0x00, 0xC0, 0x40, 0x40, 0xC0];
        assert_eq!(downsample_16bit_to_8bit(&input), vec![0xFF, 0x00, 0xC0, 0x40]);
    }

    #[test]
    fn downsample_empty() {
        assert_eq!(downsample_16bit_to_8bit(&[]), Vec::<u8>::new());
    }

    #[test]
    fn downsample_ignores_odd_tail() {
        // chunks_exact игнорирует неполный последний кусок — для корректного
        // потока PNG это никогда не происходит, но защита от panic полезна.
        let input = vec![0xAB, 0xCD, 0xEF];
        assert_eq!(downsample_16bit_to_8bit(&input), vec![0xAB]);
    }
}
