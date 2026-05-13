//! Парсинг палитровых чанков `PLTE` / `tRNS` и развёртка
//! палитровых индексов в RGB / RGBA-пиксели.
//!
//! PNG-палитра — список из 1..=256 RGB-triples в чанке `PLTE`
//! (PNG §11.2.3). Опциональный `tRNS` для color_type 3 содержит до
//! `len(PLTE)` alpha-значений: tRNS[i] — alpha для индекса i.
//! Отсутствующие alpha-значения трактуются как 255 (opaque), что
//! позволяет файлам с одним прозрачным индексом не нести 256 байт alpha.
//!
//! После unfilter-а скан-линий каждый байт — это палитровый индекс
//! (для bit_depth=8). Расширяем: один входной байт → 3 байта Rgb8
//! (если tRNS нет) или 4 байта Rgba8.

use crate::{DecodeError, PaletteError};

/// Распарсить `PLTE`-чанк. Длина должна делиться на 3, итоговое число
/// entries — в диапазоне 1..=256 (PNG §11.2.3).
pub(crate) fn parse_plte(data: &[u8]) -> Result<Vec<[u8; 3]>, DecodeError> {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    if !data.len().is_multiple_of(3) {
        return Err(DecodeError::BadPalette(PaletteError::BadPlteLength(len)));
    }
    let count = data.len() / 3;
    if !(1..=256).contains(&count) {
        return Err(DecodeError::BadPalette(PaletteError::PlteOutOfRange(count)));
    }
    let mut palette = Vec::with_capacity(count);
    for chunk in data.chunks_exact(3) {
        palette.push([chunk[0], chunk[1], chunk[2]]);
    }
    Ok(palette)
}

/// Распарсить `tRNS`-чанк в палитровом контексте. Длина данных может быть
/// меньше или равна числу entries в `PLTE`. Отсутствующие entries —
/// alpha = 255. Длина больше `PLTE` — ошибка.
pub(crate) fn parse_trns_palette(
    data: &[u8],
    plte_count: usize,
) -> Result<Vec<u8>, DecodeError> {
    if data.len() > plte_count {
        return Err(DecodeError::BadPalette(PaletteError::TrnsTooLong {
            plte_count,
            trns_count: data.len(),
        }));
    }
    let mut alpha = vec![255u8; plte_count];
    alpha[..data.len()].copy_from_slice(data);
    Ok(alpha)
}

/// Развернуть палитровые индексы в `Rgb8` (без tRNS).
pub(crate) fn expand_to_rgb(
    indices: &[u8],
    palette: &[[u8; 3]],
    width: u32,
) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::with_capacity(indices.len() * 3);
    for (i, &idx) in indices.iter().enumerate() {
        let entry = palette.get(idx as usize).ok_or_else(|| {
            DecodeError::BadPalette(PaletteError::IndexOutOfRange {
                row: u32::try_from(i / width.max(1) as usize).unwrap_or(u32::MAX),
                col: u32::try_from(i % width.max(1) as usize).unwrap_or(u32::MAX),
                index: idx,
                plte_count: palette.len(),
            })
        })?;
        out.extend_from_slice(entry);
    }
    Ok(out)
}

/// Развернуть палитровые индексы в `Rgba8` через RGB-палитру + alpha-таблицу.
pub(crate) fn expand_to_rgba(
    indices: &[u8],
    palette: &[[u8; 3]],
    alpha: &[u8],
    width: u32,
) -> Result<Vec<u8>, DecodeError> {
    debug_assert_eq!(alpha.len(), palette.len());
    let mut out = Vec::with_capacity(indices.len() * 4);
    for (i, &idx) in indices.iter().enumerate() {
        let idx_u = idx as usize;
        let entry = palette.get(idx_u).ok_or_else(|| {
            DecodeError::BadPalette(PaletteError::IndexOutOfRange {
                row: u32::try_from(i / width.max(1) as usize).unwrap_or(u32::MAX),
                col: u32::try_from(i % width.max(1) as usize).unwrap_or(u32::MAX),
                index: idx,
                plte_count: palette.len(),
            })
        })?;
        out.extend_from_slice(entry);
        out.push(alpha[idx_u]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plte_basic() {
        let data = [
            0, 0, 0, // entry 0 = black
            255, 255, 255, // entry 1 = white
            255, 0, 0, // entry 2 = red
        ];
        let p = parse_plte(&data).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p[0], [0, 0, 0]);
        assert_eq!(p[1], [255, 255, 255]);
        assert_eq!(p[2], [255, 0, 0]);
    }

    #[test]
    fn parse_plte_max_256_entries() {
        let data: Vec<u8> = (0..256).flat_map(|i| [i as u8, i as u8, i as u8]).collect();
        let p = parse_plte(&data).unwrap();
        assert_eq!(p.len(), 256);
        assert_eq!(p[127], [127, 127, 127]);
    }

    #[test]
    fn parse_plte_rejects_zero_entries() {
        assert!(matches!(
            parse_plte(&[]),
            Err(DecodeError::BadPalette(PaletteError::PlteOutOfRange(0)))
        ));
    }

    #[test]
    fn parse_plte_rejects_more_than_256() {
        let data: Vec<u8> = vec![0; 3 * 257];
        assert!(matches!(
            parse_plte(&data),
            Err(DecodeError::BadPalette(PaletteError::PlteOutOfRange(257)))
        ));
    }

    #[test]
    fn parse_plte_rejects_non_multiple_of_3() {
        assert!(matches!(
            parse_plte(&[0, 1, 2, 3]),
            Err(DecodeError::BadPalette(PaletteError::BadPlteLength(4)))
        ));
    }

    #[test]
    fn parse_trns_palette_full() {
        let alpha = parse_trns_palette(&[0, 128, 255], 3).unwrap();
        assert_eq!(alpha, vec![0, 128, 255]);
    }

    #[test]
    fn parse_trns_palette_shorter_pads_with_255() {
        let alpha = parse_trns_palette(&[0, 128], 5).unwrap();
        assert_eq!(alpha, vec![0, 128, 255, 255, 255]);
    }

    #[test]
    fn parse_trns_palette_empty_all_opaque() {
        let alpha = parse_trns_palette(&[], 3).unwrap();
        assert_eq!(alpha, vec![255, 255, 255]);
    }

    #[test]
    fn parse_trns_palette_rejects_longer_than_plte() {
        assert!(matches!(
            parse_trns_palette(&[0, 1, 2, 3, 4], 3),
            Err(DecodeError::BadPalette(PaletteError::TrnsTooLong {
                plte_count: 3,
                trns_count: 5,
            }))
        ));
    }

    #[test]
    fn expand_to_rgb_basic() {
        let palette = [[10, 20, 30], [40, 50, 60], [70, 80, 90]];
        let indices = [0, 2, 1];
        let rgb = expand_to_rgb(&indices, &palette, 3).unwrap();
        assert_eq!(rgb, vec![10, 20, 30, 70, 80, 90, 40, 50, 60]);
    }

    #[test]
    fn expand_to_rgb_rejects_out_of_range_index() {
        let palette = [[0, 0, 0], [255, 255, 255]];
        let indices = [0, 1, 5]; // 5 outside palette of 2
        let err = expand_to_rgb(&indices, &palette, 3).unwrap_err();
        assert!(matches!(
            err,
            DecodeError::BadPalette(PaletteError::IndexOutOfRange {
                index: 5,
                plte_count: 2,
                ..
            })
        ));
    }

    #[test]
    fn expand_to_rgba_combines_palette_with_alpha() {
        let palette = [[10, 20, 30], [40, 50, 60]];
        let alpha = [0, 200];
        let indices = [0, 1, 0];
        let rgba = expand_to_rgba(&indices, &palette, &alpha, 3).unwrap();
        assert_eq!(
            rgba,
            vec![
                10, 20, 30, 0, //
                40, 50, 60, 200, //
                10, 20, 30, 0, //
            ]
        );
    }
}
