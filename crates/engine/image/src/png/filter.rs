//! Развёртка PNG-фильтров скан-линий (PNG §9).
//!
//! Каждая скан-линия в IDAT начинается с байта типа фильтра (0..=4),
//! за которым идут `width * bytes_per_pixel` фильтрованных байтов.
//! Развёртка идёт сверху вниз; результат — плотный row-major массив
//! без filter-байтов.
//!
//! Фильтры арифметические по модулю 256 (wraparound u8), формулы:
//! - None  (0): `x = filt`
//! - Sub   (1): `x = filt + a`           (a — байт слева, bpp назад)
//! - Up    (2): `x = filt + b`           (b — байт сверху, прошлая строка)
//! - Avg   (3): `x = filt + ⌊(a+b)/2⌋`
//! - Paeth (4): `x = filt + Paeth(a,b,c)` (c — слева-сверху)
//!
//! Для первой строки `b = c = 0`; для первых `bpp` байтов любой строки
//! `a = c = 0`.

use crate::DecodeError;

pub(crate) fn unfilter(
    filtered: &[u8],
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
) -> Result<Vec<u8>, DecodeError> {
    let row_data_bytes = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or(DecodeError::BadImageDataSize {
            expected: 0,
            actual: filtered.len(),
        })?;
    let expected = (1 + row_data_bytes)
        .checked_mul(height as usize)
        .ok_or(DecodeError::BadImageDataSize {
            expected: 0,
            actual: filtered.len(),
        })?;
    if filtered.len() != expected {
        return Err(DecodeError::BadImageDataSize {
            expected,
            actual: filtered.len(),
        });
    }

    let mut out = vec![0u8; row_data_bytes * height as usize];
    let mut prev_row_start: Option<usize> = None;

    for row in 0..height {
        let in_row_start = row as usize * (1 + row_data_bytes);
        let filter_kind = filtered[in_row_start];
        let in_data = &filtered[in_row_start + 1..in_row_start + 1 + row_data_bytes];
        let out_row_start = row as usize * row_data_bytes;

        match filter_kind {
            0 => out[out_row_start..out_row_start + row_data_bytes].copy_from_slice(in_data),
            1 => unfilter_sub(in_data, &mut out[out_row_start..], bytes_per_pixel),
            2 => unfilter_up(in_data, &mut out, out_row_start, prev_row_start, row_data_bytes),
            3 => unfilter_avg(
                in_data,
                &mut out,
                out_row_start,
                prev_row_start,
                bytes_per_pixel,
                row_data_bytes,
            ),
            4 => unfilter_paeth(
                in_data,
                &mut out,
                out_row_start,
                prev_row_start,
                bytes_per_pixel,
                row_data_bytes,
            ),
            other => return Err(DecodeError::BadFilter { row, kind: other }),
        }

        prev_row_start = Some(out_row_start);
    }

    Ok(out)
}

fn unfilter_sub(in_data: &[u8], out_row: &mut [u8], bpp: usize) {
    for i in 0..in_data.len() {
        let a = if i >= bpp { out_row[i - bpp] } else { 0 };
        out_row[i] = in_data[i].wrapping_add(a);
    }
}

fn unfilter_up(
    in_data: &[u8],
    out: &mut [u8],
    out_row_start: usize,
    prev_row_start: Option<usize>,
    row_bytes: usize,
) {
    for i in 0..row_bytes {
        let b = match prev_row_start {
            Some(p) => out[p + i],
            None => 0,
        };
        out[out_row_start + i] = in_data[i].wrapping_add(b);
    }
}

fn unfilter_avg(
    in_data: &[u8],
    out: &mut [u8],
    out_row_start: usize,
    prev_row_start: Option<usize>,
    bpp: usize,
    row_bytes: usize,
) {
    for i in 0..row_bytes {
        let a = if i >= bpp { out[out_row_start + i - bpp] } else { 0 };
        let b = match prev_row_start {
            Some(p) => out[p + i],
            None => 0,
        };
        let avg = ((u16::from(a) + u16::from(b)) / 2) as u8;
        out[out_row_start + i] = in_data[i].wrapping_add(avg);
    }
}

fn unfilter_paeth(
    in_data: &[u8],
    out: &mut [u8],
    out_row_start: usize,
    prev_row_start: Option<usize>,
    bpp: usize,
    row_bytes: usize,
) {
    for i in 0..row_bytes {
        let a = if i >= bpp { out[out_row_start + i - bpp] } else { 0 };
        let b = match prev_row_start {
            Some(p) => out[p + i],
            None => 0,
        };
        let c = match (prev_row_start, i >= bpp) {
            (Some(p), true) => out[p + i - bpp],
            _ => 0,
        };
        out[out_row_start + i] = in_data[i].wrapping_add(paeth_predictor(a, b, c));
    }
}

/// Paeth-предиктор (PNG §9.4). Все аргументы u8, арифметика в i16 ради
/// корректного сравнения abs-расстояний.
fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfilter_single_row_none() {
        // 1 строка, 3 пикселя RGB (bpp=3): фильтр 0 + 9 байт данных.
        let filt = vec![0u8, 10, 20, 30, 40, 50, 60, 70, 80, 90];
        let out = unfilter(&filt, 3, 1, 3).unwrap();
        assert_eq!(out, vec![10, 20, 30, 40, 50, 60, 70, 80, 90]);
    }

    #[test]
    fn unfilter_sub_grayscale() {
        // 1 строка, 5 пикселей Gray (bpp=1). Sub: x[i] = filt[i] + x[i-1].
        // filt = [1, 1, 2, 3, 5, 8] → output 1, 1+1=2, 2+2=4, 4+3=7, 7+5=12, 12+8=20.
        // Но bpp=1 значит «a = byte before»; row_data_bytes=5; expected len = 1+5 = 6.
        let filt = vec![1u8, 1, 2, 3, 5, 8]; // filter_kind=1, потом 5 байт
        let out = unfilter(&filt, 5, 1, 1).unwrap();
        assert_eq!(out, vec![1, 3, 6, 11, 19]);
    }

    #[test]
    fn unfilter_up_two_rows() {
        // 2 строки, 3 байта (Gray width=3, bpp=1). Первая None (b=0).
        // row0 filt = [0, 10, 20, 30] → out [10,20,30]
        // row1 filt = [2, 5, 5, 5]    → x = filt + above:
        //   out[3]=5+10=15, out[4]=5+20=25, out[5]=5+30=35
        let filt = vec![0, 10, 20, 30, 2, 5, 5, 5];
        let out = unfilter(&filt, 3, 2, 1).unwrap();
        assert_eq!(out, vec![10, 20, 30, 15, 25, 35]);
    }

    #[test]
    fn unfilter_paeth_first_row_first_pixel() {
        // На первой пикселе первой строки a=b=c=0, paeth=0, x = filt.
        let filt = vec![4, 42];
        let out = unfilter(&filt, 1, 1, 1).unwrap();
        assert_eq!(out, vec![42]);
    }

    #[test]
    fn paeth_predictor_examples() {
        // a=b=c=0 → 0.
        assert_eq!(paeth_predictor(0, 0, 0), 0);
        // a=10, b=20, c=15. p = 10+20-15=15. pa=|15-10|=5, pb=|15-20|=5, pc=|15-15|=0.
        // pa<=pb && pa<=pc? 5<=5 && 5<=0? нет. pb<=pc? 5<=0? нет. → c = 15.
        assert_eq!(paeth_predictor(10, 20, 15), 15);
        // a=10, b=10, c=5. p=15. pa=5, pb=5, pc=10. pa<=pb && pa<=pc → a=10.
        assert_eq!(paeth_predictor(10, 10, 5), 10);
    }

    #[test]
    fn unfilter_avg_two_rows() {
        // 1×2 RGBA (bpp=4, 1 строка 4 байт, 2 строки → 2×(1+4)=10 байт filt).
        // row0 None: [0, 10,20,30,40] → out[0..4]=10,20,30,40
        // row1 Avg: filter=3, для каждого байта x = filt + (a+b)/2
        //   i=0: a=0 (i<bpp), b=10 → avg=5. x = 1+5=6
        //   i=1: a=0, b=20 → avg=10. x = 2+10=12
        //   i=2: a=0, b=30 → avg=15. x = 3+15=18
        //   i=3: a=0, b=40 → avg=20. x = 4+20=24
        let filt = vec![0, 10, 20, 30, 40, 3, 1, 2, 3, 4];
        let out = unfilter(&filt, 1, 2, 4).unwrap();
        assert_eq!(out, vec![10, 20, 30, 40, 6, 12, 18, 24]);
    }

    #[test]
    fn unfilter_rejects_unknown_filter() {
        let filt = vec![5, 0, 0, 0];
        let err = unfilter(&filt, 3, 1, 1).unwrap_err();
        assert!(matches!(err, DecodeError::BadFilter { kind: 5, row: 0 }));
    }

    #[test]
    fn unfilter_rejects_wrong_length() {
        let filt = vec![0, 10, 20]; // ожидаем 4 байта (1 filter + 3 data) для 3×1×1
        let err = unfilter(&filt, 3, 1, 1).unwrap_err();
        assert!(matches!(err, DecodeError::BadImageDataSize { .. }));
    }
}
