//! Entropy-coded scan decoder (ISO/IEC 10918-1 §F.2).
//!
//! После SOS читаем MCU за MCU:
//! 1. Для каждого компонента scan-а — `Hi × Vi` блоков 8×8.
//! 2. На блок:
//!    - DC: Huffman-decode size S, читаем S битов, восстанавливаем signed
//!      delta, прибавляем к DC predictor этого компонента.
//!    - AC: Huffman-decode `RS = (run<<4)|size`; `00` = EOB (конец блока),
//!      `0xF0` = ZRL (16 нулей подряд), иначе пропускаем `run` коэффициентов
//!      и записываем signed value длины `size` в позицию `1+run`.
//! 3. После всех коэффициентов: de-zigzag, dequantize, IDCT, level shift +128.
//! 4. Записать blockes в соответствующее место component-grid.
//! 5. После всех MCU: при наличии 3 компонент — YCbCr→RGB с chroma upsampling
//!    (nearest-neighbour replication).
//! 6. Restart marker (RSTm) каждые `restart_interval` MCU: align reader,
//!    сбросить DC predictors, обновить modulo-8 счётчик.

use super::bit_reader::JpegBitReader;
use super::color::ycbcr_to_rgb;
use super::idct::idct_8x8;
use super::marker::{JpegContext, JpegError, SegmentReader, ZIGZAG};

/// Декодирует scan и возвращает финальный pixel buffer:
/// `Gray8` (1 компонент) — `data.len() = w * h`,
/// `Rgb8` (3 компонента) — `data.len() = w * h * 3`.
pub fn decode_scan(reader: &mut SegmentReader<'_>, ctx: &JpegContext) -> Result<Vec<u8>, JpegError> {
    let frame = &ctx.frame;
    let h_max = u32::from(frame.h_max);
    let v_max = u32::from(frame.v_max);
    let width = u32::from(frame.width);
    let height = u32::from(frame.height);

    // Размер MCU в пикселях.
    let mcu_w = h_max * 8;
    let mcu_h = v_max * 8;
    let mcus_per_row = width.div_ceil(mcu_w);
    let mcus_per_col = height.div_ceil(mcu_h);

    // Component grids: каждый компонент имеет свой непрерывный буфер
    // ширины `mcus_per_row × Hi × 8` пикселей.
    let mut grids: Vec<Vec<u8>> = Vec::with_capacity(frame.components.len());
    let mut grid_widths = Vec::with_capacity(frame.components.len());
    let mut grid_heights = Vec::with_capacity(frame.components.len());
    for c in &frame.components {
        let gw = mcus_per_row * u32::from(c.h_sampling) * 8;
        let gh = mcus_per_col * u32::from(c.v_sampling) * 8;
        grids.push(vec![0u8; (gw * gh) as usize]);
        grid_widths.push(gw);
        grid_heights.push(gh);
    }

    // DC predictors per-component (по позиционному индексу в frame).
    let mut dc_pred = vec![0i32; frame.components.len()];

    let mut bit_reader = JpegBitReader::new(reader.bytes(), reader.pos());
    let mut mcu_since_restart: u32 = 0;
    let mut next_rst: u8 = 0; // RST0..RST7 циклически.

    for mcu_y in 0..mcus_per_col {
        for mcu_x in 0..mcus_per_row {
            // Restart: проверяем перед чтением MCU, чтобы RSTm в начале не сломался.
            if ctx.restart_interval > 0 && mcu_since_restart == u32::from(ctx.restart_interval) {
                let marker = bit_reader.read_restart_marker()?;
                let expected_rst = 0xD0 | next_rst;
                if marker != expected_rst {
                    return Err(JpegError::UnexpectedMarker(marker));
                }
                next_rst = (next_rst + 1) & 0x07;
                for p in dc_pred.iter_mut() {
                    *p = 0;
                }
                mcu_since_restart = 0;
            }

            // Декодируем все блоки всех компонент в этом MCU.
            for (ci, comp) in frame.components.iter().enumerate() {
                let sc = ctx
                    .scan
                    .iter()
                    .find(|s| s.frame_index == ci)
                    .ok_or(JpegError::BadScanComponent(comp.id))?;
                let dc_table = ctx.dc_tables[sc.dc_table as usize]
                    .as_ref()
                    .ok_or(JpegError::MissingHuffmanTable {
                        class: 0,
                        id: sc.dc_table,
                    })?;
                let ac_table = ctx.ac_tables[sc.ac_table as usize]
                    .as_ref()
                    .ok_or(JpegError::MissingHuffmanTable {
                        class: 1,
                        id: sc.ac_table,
                    })?;
                let qt = ctx.quant_tables[comp.qt_id as usize]
                    .as_ref()
                    .ok_or(JpegError::MissingQuantTable(comp.qt_id))?;

                for by in 0..u32::from(comp.v_sampling) {
                    for bx in 0..u32::from(comp.h_sampling) {
                        let mut block = [0i32; 64];
                        decode_block(&mut bit_reader, dc_table, ac_table, &mut dc_pred[ci], &mut block)?;
                        // Dequantize в natural order (qt уже de-zigzagged в DQT parser-е).
                        for k in 0..64 {
                            block[k] *= i32::from(qt[k]);
                        }
                        // IDCT + level shift + clamp (in-place).
                        idct_8x8(&mut block);

                        // Запись в component grid.
                        let gx = mcu_x * u32::from(comp.h_sampling) * 8 + bx * 8;
                        let gy = mcu_y * u32::from(comp.v_sampling) * 8 + by * 8;
                        let gw = grid_widths[ci];
                        let grid = &mut grids[ci];
                        for y in 0..8 {
                            for x in 0..8 {
                                let pos = ((gy + y) * gw + gx + x) as usize;
                                grid[pos] = block[(y * 8 + x) as usize] as u8;
                            }
                        }
                    }
                }
            }

            mcu_since_restart += 1;
        }
    }

    // Update reader position to where bit_reader stopped (для возможной
    // диагностики; сейчас не используется).
    let _ = reader;

    // Финальная сборка output buffer-а.
    if frame.components.len() == 1 {
        // Grayscale: вырезаем width×height из grids[0].
        let gw = grid_widths[0];
        let mut out = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                out.push(grids[0][(y * gw + x) as usize]);
            }
        }
        Ok(out)
    } else {
        // YCbCr: Y в grids[0], Cb в [1], Cr в [2]. Cb/Cr могут быть subsampled.
        let mut out = Vec::with_capacity((width * height * 3) as usize);
        let y_gw = grid_widths[0];
        let cb_gw = grid_widths[1];
        let cr_gw = grid_widths[2];
        let h_y = u32::from(frame.components[0].h_sampling);
        let v_y = u32::from(frame.components[0].v_sampling);
        let h_cb = u32::from(frame.components[1].h_sampling);
        let v_cb = u32::from(frame.components[1].v_sampling);
        let h_cr = u32::from(frame.components[2].h_sampling);
        let v_cr = u32::from(frame.components[2].v_sampling);

        for y in 0..height {
            for x in 0..width {
                // Nearest-neighbour upsampling: ищем chroma sample, который
                // соответствует данному пикселю Y. Для 4:2:0 (h_y=v_y=2, h_c=v_c=1)
                // это (x/2, y/2). Общая формула: (x * h_c / h_max, y * v_c / v_max).
                let yv = grids[0][((y * v_y / v_max) * y_gw + (x * h_y / h_max)) as usize];
                let cb_v = grids[1][((y * v_cb / v_max) * cb_gw + (x * h_cb / h_max)) as usize];
                let cr_v = grids[2][((y * v_cr / v_max) * cr_gw + (x * h_cr / h_max)) as usize];
                let (r, g, b) = ycbcr_to_rgb(yv, cb_v, cr_v);
                out.push(r);
                out.push(g);
                out.push(b);
            }
        }
        Ok(out)
    }
}

/// Декодирует один 8×8 блок DCT-коэффициентов в natural order (с de-zigzag).
fn decode_block(
    reader: &mut JpegBitReader<'_>,
    dc_table: &super::huffman::HuffmanTable,
    ac_table: &super::huffman::HuffmanTable,
    dc_pred: &mut i32,
    block: &mut [i32; 64],
) -> Result<(), JpegError> {
    // DC: Huffman-decode size, прочитать size битов, восстановить delta.
    let dc_size = dc_table.decode(reader)?;
    if dc_size > 15 {
        return Err(JpegError::BadCoefficientSize(dc_size));
    }
    let dc_value_bits = reader.read_bits(dc_size)?;
    let dc_delta = extend(dc_value_bits, dc_size);
    *dc_pred += dc_delta;
    block[0] = *dc_pred; // в natural order DC = (0,0) = индекс 0.

    // AC: декодируем коэффициенты в zigzag-позиции 1..=63.
    let mut k = 1usize;
    while k < 64 {
        let rs = ac_table.decode(reader)?;
        let run = (rs >> 4) as usize;
        let size = rs & 0x0F;
        if size == 0 {
            if run == 15 {
                // ZRL: 16 нулей подряд, без значения.
                k += 16;
                continue;
            }
            // EOB: остальное всё нули — уже инициализировано.
            break;
        }
        if size > 15 {
            return Err(JpegError::BadCoefficientSize(size));
        }
        k += run;
        if k >= 64 {
            return Err(JpegError::BadCoefficientPosition(k));
        }
        let raw = reader.read_bits(size)?;
        let value = extend(raw, size);
        // De-zigzag: ZIGZAG[k] — позиция в natural row-major.
        block[ZIGZAG[k]] = value;
        k += 1;
    }
    Ok(())
}

/// `EXTEND` процедура §F.2.1.1: восстанавливает signed integer из
/// magnitude `bits` (длины `size`).
fn extend(bits: u16, size: u8) -> i32 {
    if size == 0 {
        return 0;
    }
    let v = i32::from(bits);
    let threshold = 1i32 << (size - 1);
    if v < threshold {
        v + (-1i32 << size) + 1
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extend_basic_values() {
        // size=0 → 0.
        assert_eq!(extend(0, 0), 0);
        // size=1: 0 → -1, 1 → 1.
        assert_eq!(extend(0, 1), -1);
        assert_eq!(extend(1, 1), 1);
        // size=2: 00→-3, 01→-2, 10→2, 11→3.
        assert_eq!(extend(0b00, 2), -3);
        assert_eq!(extend(0b01, 2), -2);
        assert_eq!(extend(0b10, 2), 2);
        assert_eq!(extend(0b11, 2), 3);
        // size=3: 000→-7, …, 011→-4, 100→4, …, 111→7.
        assert_eq!(extend(0b000, 3), -7);
        assert_eq!(extend(0b011, 3), -4);
        assert_eq!(extend(0b100, 3), 4);
        assert_eq!(extend(0b111, 3), 7);
    }

    #[test]
    fn extend_size_8_full_range() {
        // size=8: 0 → -255, 127 → -128, 128 → 128, 255 → 255.
        assert_eq!(extend(0, 8), -255);
        assert_eq!(extend(127, 8), -128);
        assert_eq!(extend(128, 8), 128);
        assert_eq!(extend(255, 8), 255);
    }
}
