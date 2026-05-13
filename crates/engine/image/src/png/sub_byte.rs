//! Распаковка PNG-скан-линий с глубиной 1, 2 или 4 бита на сэмпл +
//! масштабирование grayscale-сэмплов до полного 8-битного диапазона.
//!
//! PNG §7.2 / §13.12: при `bit_depth < 8` сэмплы упакованы в байты с
//! **MSB-first** ordering — старший бит первого сэмпла, потом следующий
//! и т.д. Число сэмплов в байте = 8/bit_depth (8 для 1-bit, 4 для 2-bit,
//! 2 для 4-bit). Длина упакованной скан-линии — `ceil(width × bpp / 8)`
//! байтов; trailing-биты в последнем байте просто игнорируются.
//!
//! После распаковки grayscale-сэмплы масштабируются на 8-битный диапазон
//! по PNG §13.12:
//! - 1-bit: 0 → 0, 1 → 255 (множитель 255);
//! - 2-bit: 0 → 0, 1 → 85, 2 → 170, 3 → 255 (множитель 85);
//! - 4-bit: 0..=15 → 0..=255 (множитель 17, t.е. `n * 0x11`).
//!
//! Эти множители выбираются так, чтобы граничные значения (0 и max)
//! отображались в 0 и 255, а равномерное распределение сохранялось.
//!
//! Для палитровых сэмплов масштабировать **нельзя** — это индексы в PLTE.
//! Распаковка к одному байту-индексу сохраняется как есть; диапазон
//! 0..=(2^bit_depth - 1).

/// Распаковать упакованные сэмплы в плотный массив «один байт на сэмпл».
/// `bit_depth ∈ {1, 2, 4}`. Возвращает `width * height` байт.
/// При `bit_depth = 8` функция не вызывается — orchestrator пропускает unpack.
pub(crate) fn unpack_bits(packed: &[u8], width: u32, height: u32, bit_depth: u8) -> Vec<u8> {
    debug_assert!(
        matches!(bit_depth, 1 | 2 | 4),
        "unpack_bits принимает только 1/2/4 битную глубину"
    );
    let w = width as usize;
    let h = height as usize;
    let bytes_per_row = w.div_ceil(8 / bit_depth as usize);
    debug_assert_eq!(packed.len(), bytes_per_row * h);
    let mask = (1u8 << bit_depth) - 1;
    let samples_per_byte = 8 / bit_depth;
    let mut out = Vec::with_capacity(w * h);
    for row in 0..h {
        let row_packed = &packed[row * bytes_per_row..(row + 1) * bytes_per_row];
        let mut produced = 0usize;
        for &byte in row_packed {
            for i in 0..samples_per_byte {
                if produced >= w {
                    break;
                }
                // MSB-first: первый сэмпл в старших битах байта.
                let shift = (samples_per_byte - 1 - i) * bit_depth;
                out.push((byte >> shift) & mask);
                produced += 1;
            }
            if produced >= w {
                break;
            }
        }
    }
    out
}

/// Масштабировать grayscale-сэмплы до полного 8-битного диапазона по PNG §13.12.
/// Работает in-place. `bit_depth ∈ {1, 2, 4}`.
pub(crate) fn scale_grayscale_to_8bit(samples: &mut [u8], bit_depth: u8) {
    let factor = match bit_depth {
        1 => 255u8,
        2 => 85u8,
        4 => 17u8,
        _ => unreachable!("scale_grayscale_to_8bit принимает только 1/2/4"),
    };
    for s in samples.iter_mut() {
        *s = s.wrapping_mul(factor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_1bit_single_row() {
        // 8 сэмплов в байте, MSB-first.
        // 0b10110010 → [1, 0, 1, 1, 0, 0, 1, 0]
        let out = unpack_bits(&[0b1011_0010], 8, 1, 1);
        assert_eq!(out, vec![1, 0, 1, 1, 0, 0, 1, 0]);
    }

    #[test]
    fn unpack_1bit_with_padding() {
        // width=5 → 5 первых бит из байта; trailing 3 бита игнорируются.
        // 0b10110000 → [1, 0, 1, 1, 0]
        let out = unpack_bits(&[0b1011_0000], 5, 1, 1);
        assert_eq!(out, vec![1, 0, 1, 1, 0]);
    }

    #[test]
    fn unpack_2bit_single_row() {
        // 4 сэмпла в байте: 0b11_10_01_00 → [3, 2, 1, 0]
        let out = unpack_bits(&[0b1110_0100], 4, 1, 2);
        assert_eq!(out, vec![3, 2, 1, 0]);
    }

    #[test]
    fn unpack_2bit_with_padding() {
        // width=3 → первые 3 сэмпла (6 бит); trailing 2 бита игнорируются.
        // 0b11_10_01_xx → [3, 2, 1]
        let out = unpack_bits(&[0b1110_0100], 3, 1, 2);
        assert_eq!(out, vec![3, 2, 1]);
    }

    #[test]
    fn unpack_4bit_single_row() {
        // 2 сэмпла в байте: 0xAB → [0xA, 0xB]
        let out = unpack_bits(&[0xAB, 0xCD], 4, 1, 4);
        assert_eq!(out, vec![0xA, 0xB, 0xC, 0xD]);
    }

    #[test]
    fn unpack_4bit_with_padding() {
        // width=3 → 3 сэмпла из 2 байт; trailing nibble игнорируется.
        // [0xAB, 0xCD] → [0xA, 0xB, 0xC]
        let out = unpack_bits(&[0xAB, 0xCD], 3, 1, 4);
        assert_eq!(out, vec![0xA, 0xB, 0xC]);
    }

    #[test]
    fn unpack_multiple_rows_distinct() {
        // 2 строки по 2 сэмпла 4-bit (1 байт на строку):
        // row0 = 0x12, row1 = 0x34 → [1,2,3,4]
        let out = unpack_bits(&[0x12, 0x34], 2, 2, 4);
        assert_eq!(out, vec![0x1, 0x2, 0x3, 0x4]);
    }

    #[test]
    fn unpack_multiple_rows_with_padding() {
        // 2 строки по 3 сэмпла 2-bit (1 байт на строку, trailing 2 бита pad):
        // row0 0b11_10_01_xx → [3,2,1]
        // row1 0b00_01_10_xx → [0,1,2]
        let out = unpack_bits(&[0b1110_0100, 0b0001_1000], 3, 2, 2);
        assert_eq!(out, vec![3, 2, 1, 0, 1, 2]);
    }

    #[test]
    fn scale_1bit() {
        let mut s = vec![0, 1, 1, 0, 1];
        scale_grayscale_to_8bit(&mut s, 1);
        assert_eq!(s, vec![0, 255, 255, 0, 255]);
    }

    #[test]
    fn scale_2bit() {
        let mut s = vec![0, 1, 2, 3];
        scale_grayscale_to_8bit(&mut s, 2);
        assert_eq!(s, vec![0, 85, 170, 255]);
    }

    #[test]
    fn scale_4bit() {
        let mut s = vec![0, 1, 5, 0xA, 0xF];
        scale_grayscale_to_8bit(&mut s, 4);
        assert_eq!(s, vec![0, 17, 85, 170, 255]);
    }
}
