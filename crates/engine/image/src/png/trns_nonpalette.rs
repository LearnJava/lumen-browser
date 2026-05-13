//! `tRNS` для не-палитровых типов изображения — single-color
//! transparency (PNG §11.3.2.1).
//!
//! Для color_type 0 (Grayscale) и 2 (RGB) `tRNS` содержит конкретное
//! значение сэмпла (одно gray или один RGB-triple) в формате big-endian
//! u16-сэмплов. Любой пиксель, точно равный этому значению, считается
//! полностью прозрачным (alpha=0); остальные — полностью непрозрачными
//! (alpha=255). Без полутонов: либо «всё» либо «ничего».
//!
//! tRNS для color_type 4 (GrayA) и 6 (RGBA) — запрещён: в этих
//! пикселях уже есть alpha-канал.
//!
//! Spec хранит values как 16-битные независимо от `bit_depth`. Для
//! bit_depth 8 актуальные данные в low byte, для 16 — full u16, для
//! 1/2/4 — value в range `0..2^bitdepth-1`. Сравнение делается **после**
//! всех трансформаций (sub-byte unpack+scale, 16-bit downsample), для
//! чего raw tRNS-value пред-нормализуется к 8 битам тем же путём,
//! что и сэмплы.
//!
//! **Известное ограничение Phase 0:** 16-bit downsample отбрасывает
//! младший байт, поэтому два различных 16-битных значения с одним
//! и тем же high byte будут оба считаться match-ом против tRNS. Для
//! точного 16-битного match-а нужно сравнение до downsample-а —
//! рефактор отдельной задачей при первом реальном кейсе.

use crate::{DecodeError, PaletteError};

/// Распарсить `tRNS` для color_type 0 (Grayscale). Ровно 2 байта = u16 BE.
pub(crate) fn parse_trns_grayscale(data: &[u8]) -> Result<u16, DecodeError> {
    if data.len() != 2 {
        return Err(DecodeError::BadPalette(
            PaletteError::BadTrnsLengthForGrayscale(
                u32::try_from(data.len()).unwrap_or(u32::MAX),
            ),
        ));
    }
    Ok(u16::from_be_bytes([data[0], data[1]]))
}

/// Распарсить `tRNS` для color_type 2 (RGB). Ровно 6 байт = 3 u16 BE.
pub(crate) fn parse_trns_rgb(data: &[u8]) -> Result<(u16, u16, u16), DecodeError> {
    if data.len() != 6 {
        return Err(DecodeError::BadPalette(PaletteError::BadTrnsLengthForRgb(
            u32::try_from(data.len()).unwrap_or(u32::MAX),
        )));
    }
    let r = u16::from_be_bytes([data[0], data[1]]);
    let g = u16::from_be_bytes([data[2], data[3]]);
    let b = u16::from_be_bytes([data[4], data[5]]);
    Ok((r, g, b))
}

/// Перевести raw 16-битное tRNS-значение в 8-битный эквивалент по правилам
/// той же трансформации, что применяется к сэмплам:
/// - bit_depth = 16 → high byte (`value >> 8`), синхронно с downsample;
/// - bit_depth = 8 → low byte (`value as u8`), без потери;
/// - bit_depth = 1/2/4 → scale с множителем (255/85/17) — синхронно с
///   `sub_byte::scale_grayscale_to_8bit`.
pub(crate) fn normalize_trns_value_to_8bit(raw: u16, bit_depth: u8) -> u8 {
    match bit_depth {
        16 => (raw >> 8) as u8,
        8 => raw as u8,
        4 => (raw as u8 & 0x0F) * 17,
        2 => (raw as u8 & 0x03) * 85,
        1 => if raw & 1 != 0 { 255 } else { 0 },
        _ => raw as u8,
    }
}

/// Расширить grayscale-сэмплы (после всех трансформаций) в GrayAlpha8 на
/// основе сравнения с `transparent` (уже нормализованным к 8 битам).
pub(crate) fn expand_gray_with_trns(samples: &[u8], transparent: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &gray in samples {
        out.push(gray);
        out.push(if gray == transparent { 0 } else { 255 });
    }
    out
}

/// Расширить RGB-сэмплы (после всех трансформаций) в Rgba8 на основе
/// сравнения с (`r_t`, `g_t`, `b_t`) — уже нормализованным RGB-triple.
pub(crate) fn expand_rgb_with_trns(
    samples: &[u8],
    transparent: (u8, u8, u8),
) -> Vec<u8> {
    let mut out = Vec::with_capacity((samples.len() / 3) * 4);
    for triple in samples.chunks_exact(3) {
        out.extend_from_slice(triple);
        let alpha = if triple[0] == transparent.0
            && triple[1] == transparent.1
            && triple[2] == transparent.2
        {
            0
        } else {
            255
        };
        out.push(alpha);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_grayscale_basic() {
        assert_eq!(parse_trns_grayscale(&[0x00, 0x80]).unwrap(), 0x0080);
        assert_eq!(parse_trns_grayscale(&[0xFF, 0xFF]).unwrap(), 0xFFFF);
    }

    #[test]
    fn parse_grayscale_rejects_wrong_length() {
        assert!(matches!(
            parse_trns_grayscale(&[0x00]),
            Err(DecodeError::BadPalette(
                PaletteError::BadTrnsLengthForGrayscale(1)
            ))
        ));
        assert!(matches!(
            parse_trns_grayscale(&[0x00, 0x01, 0x02]),
            Err(DecodeError::BadPalette(
                PaletteError::BadTrnsLengthForGrayscale(3)
            ))
        ));
    }

    #[test]
    fn parse_rgb_basic() {
        let (r, g, b) = parse_trns_rgb(&[0x00, 0xFF, 0x12, 0x34, 0xAB, 0xCD]).unwrap();
        assert_eq!(r, 0x00FF);
        assert_eq!(g, 0x1234);
        assert_eq!(b, 0xABCD);
    }

    #[test]
    fn parse_rgb_rejects_wrong_length() {
        assert!(matches!(
            parse_trns_rgb(&[0; 5]),
            Err(DecodeError::BadPalette(PaletteError::BadTrnsLengthForRgb(5)))
        ));
    }

    #[test]
    fn normalize_trns_8bit() {
        assert_eq!(normalize_trns_value_to_8bit(0x0080, 8), 0x80);
        assert_eq!(normalize_trns_value_to_8bit(0x00FF, 8), 0xFF);
    }

    #[test]
    fn normalize_trns_16bit_takes_high_byte() {
        assert_eq!(normalize_trns_value_to_8bit(0xFFFF, 16), 0xFF);
        assert_eq!(normalize_trns_value_to_8bit(0x8080, 16), 0x80);
        assert_eq!(normalize_trns_value_to_8bit(0x00FF, 16), 0x00);
    }

    #[test]
    fn normalize_trns_subbyte_grayscale_matches_scale() {
        // 1-bit: 0 → 0, 1 → 255
        assert_eq!(normalize_trns_value_to_8bit(0, 1), 0);
        assert_eq!(normalize_trns_value_to_8bit(1, 1), 255);
        // 2-bit: 0/1/2/3 → 0/85/170/255
        assert_eq!(normalize_trns_value_to_8bit(0, 2), 0);
        assert_eq!(normalize_trns_value_to_8bit(1, 2), 85);
        assert_eq!(normalize_trns_value_to_8bit(2, 2), 170);
        assert_eq!(normalize_trns_value_to_8bit(3, 2), 255);
        // 4-bit: 0/8/15 → 0/136/255
        assert_eq!(normalize_trns_value_to_8bit(0, 4), 0);
        assert_eq!(normalize_trns_value_to_8bit(8, 4), 136);
        assert_eq!(normalize_trns_value_to_8bit(15, 4), 255);
    }

    #[test]
    fn expand_gray_marks_matching_transparent() {
        let samples = [100, 50, 100, 200];
        let out = expand_gray_with_trns(&samples, 100);
        // 100 → alpha=0, 50 → alpha=255
        assert_eq!(out, vec![100, 0, 50, 255, 100, 0, 200, 255]);
    }

    #[test]
    fn expand_rgb_marks_matching_transparent() {
        let samples = [
            255, 0, 0, // red
            0, 255, 0, // green
            255, 0, 0, // red (same as transparent)
        ];
        let out = expand_rgb_with_trns(&samples, (255, 0, 0));
        assert_eq!(
            out,
            vec![
                255, 0, 0, 0, //
                0, 255, 0, 255, //
                255, 0, 0, 0, //
            ]
        );
    }

    #[test]
    fn expand_rgb_full_match_only() {
        // Только точный triple-match даёт transparent; частичный совпад — opaque.
        let samples = [255, 0, 0, 255, 1, 0];
        let out = expand_rgb_with_trns(&samples, (255, 0, 0));
        assert_eq!(out, vec![255, 0, 0, 0, 255, 1, 0, 255]);
    }
}
