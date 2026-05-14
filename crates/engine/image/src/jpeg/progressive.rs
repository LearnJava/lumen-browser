//! Progressive JPEG (SOF2) decoder (ISO/IEC 10918-1 §G).
//!
//! В отличие от baseline (один scan, все 64 коэффициента на блок), progressive
//! разделяет коэффициенты на несколько scan-ов:
//!
//! - **Spectral selection** (`Ss..=Se`): scan покрывает только часть zigzag-диапазона.
//!   DC scan — `Ss=Se=0`. AC scans — `1≤Ss≤Se≤63`, всегда **non-interleaved** (Ns=1).
//! - **Successive approximation** (`Ah / Al`): scan несёт один бит value-precision.
//!   Initial scan: `Ah=0`, значения сохраняются как `extend(magnitude) << Al`.
//!   Refinement scan: `Ah = Al + 1`, добавляется ещё один младший бит к уже сохранённому.
//!
//! Поток:
//!
//! 1. Coefficient buffers per-component (i32, natural row-major, до dequantize)
//!    аллоцируются один раз — все scans пишут в них.
//! 2. После каждого scan-а bit-stream выравнивается; `SegmentReader` дочитывает
//!    промежуточные DHT/DQT/DRI и следующий SOS (или EOI).
//! 3. По EOI — dequantize + IDCT каждого блока, затем YCbCr→RGB upsample
//!    (как в baseline path-е).
//!
//! Алгоритм AC refinement (§G.1.2.3) — самая тонкая часть: при decode каждой
//! RS-пары биты refinement записываются в **уже-non-zero** коэффициенты, а
//! новые non-zero вставляются между ними на нужной zero-позиции (skip-counter
//! считает только zero-positions, не non-zero).

use super::bit_reader::JpegBitReader;
use super::color::ycbcr_to_rgb;
use super::huffman::HuffmanTable;
use super::idct::idct_8x8;
use super::marker::{
    Frame, JpegContext, JpegError, NextSegment, ScanComponent, ScanInfo, SegmentReader, ZIGZAG,
};

/// Coefficient buffer одного компонента: `blocks_h × blocks_v` блоков ×
/// 64 i32 (natural row-major внутри блока). До dequantize/IDCT.
struct ComponentCoefs {
    blocks_h: u32,
    blocks_v: u32,
    data: Vec<i32>,
}

impl ComponentCoefs {
    fn new(blocks_h: u32, blocks_v: u32) -> Self {
        Self {
            blocks_h,
            blocks_v,
            data: vec![0i32; (blocks_h * blocks_v * 64) as usize],
        }
    }

    fn block_mut(&mut self, bx: u32, by: u32) -> &mut [i32] {
        let start = ((by * self.blocks_h + bx) * 64) as usize;
        &mut self.data[start..start + 64]
    }
}

/// Главный entry: уже считанный `ctx` содержит frame + первый SOS-scan +
/// все таблицы. Цикл: decode scan → читать следующий segment → repeat → EOI →
/// финализация в pixel buffer.
pub fn decode_progressive(
    reader: &mut SegmentReader<'_>,
    mut ctx: JpegContext,
) -> Result<Vec<u8>, JpegError> {
    let frame = ctx.frame.clone();
    let h_max = u32::from(frame.h_max);
    let v_max = u32::from(frame.v_max);
    let width = u32::from(frame.width);
    let height = u32::from(frame.height);
    let mcu_w = h_max * 8;
    let mcu_h = v_max * 8;
    let mcus_per_row = width.div_ceil(mcu_w);
    let mcus_per_col = height.div_ceil(mcu_h);

    let mut coefs: Vec<ComponentCoefs> = frame
        .components
        .iter()
        .map(|c| {
            let bh = mcus_per_row * u32::from(c.h_sampling);
            let bv = mcus_per_col * u32::from(c.v_sampling);
            ComponentCoefs::new(bh, bv)
        })
        .collect();

    let mut current_scan = ctx.scan.clone();
    let mut bit_pos = reader.pos();

    loop {
        let next_pos = decode_one_scan(
            reader.bytes(),
            bit_pos,
            &frame,
            &current_scan,
            &ctx,
            &mut coefs,
            mcus_per_row,
            mcus_per_col,
        )?;
        reader.set_pos(next_pos);

        match reader.read_next_segment_after_scan(&mut ctx)? {
            NextSegment::Scan(s) => {
                current_scan = s;
                bit_pos = reader.pos();
            }
            NextSegment::Eoi => break,
        }
    }

    finalize_pixels(
        &frame,
        &ctx.quant_tables,
        &coefs,
        mcus_per_row,
        mcus_per_col,
    )
}

/// Декодирует один scan; возвращает байтовую позицию, на которой `SegmentReader`
/// должен продолжить marker-loop (указывает на `FF NN` — следующий segment).
#[allow(clippy::too_many_arguments)]
fn decode_one_scan(
    bytes: &[u8],
    start_pos: usize,
    frame: &Frame,
    scan: &ScanInfo,
    ctx: &JpegContext,
    coefs: &mut [ComponentCoefs],
    mcus_per_row: u32,
    mcus_per_col: u32,
) -> Result<usize, JpegError> {
    let mut reader = JpegBitReader::new(bytes, start_pos);
    let mut dc_pred = vec![0i32; frame.components.len()];
    let mut eob_run: u32 = 0;
    let mut mcu_since_restart: u32 = 0;
    let mut next_rst: u8 = 0;

    let is_dc = scan.ss == 0;
    let interleaved = scan.components.len() > 1;

    // Размеры scan-loop-а.
    let (mcus_h, mcus_v, comp_block_factors): (u32, u32, Vec<(u32, u32)>) = if interleaved {
        // DC interleaved scan: проход по MCU как в baseline, внутри каждой
        // MCU — Hi×Vi блоков на компонент.
        let factors = scan
            .components
            .iter()
            .map(|sc| {
                let c = &frame.components[sc.frame_index];
                (u32::from(c.h_sampling), u32::from(c.v_sampling))
            })
            .collect();
        (mcus_per_row, mcus_per_col, factors)
    } else {
        // Non-interleaved scan: один компонент, MCU = 1 блок.
        let sc = &scan.components[0];
        let c = &frame.components[sc.frame_index];
        let bh = mcus_per_row * u32::from(c.h_sampling);
        let bv = mcus_per_col * u32::from(c.v_sampling);
        (bh, bv, vec![(1, 1)])
    };

    for mcu_y in 0..mcus_v {
        for mcu_x in 0..mcus_h {
            if ctx.restart_interval > 0 && mcu_since_restart == u32::from(ctx.restart_interval) {
                let marker = reader.read_restart_marker()?;
                let expected = 0xD0 | next_rst;
                if marker != expected {
                    return Err(JpegError::UnexpectedMarker(marker));
                }
                next_rst = (next_rst + 1) & 0x07;
                for p in dc_pred.iter_mut() {
                    *p = 0;
                }
                eob_run = 0;
                mcu_since_restart = 0;
            }

            for (sc_idx, sc) in scan.components.iter().enumerate() {
                let (block_w, block_h) = comp_block_factors[sc_idx];
                let comp = &frame.components[sc.frame_index];
                for by in 0..block_h {
                    for bx in 0..block_w {
                        let (block_x, block_y) = if interleaved {
                            (
                                mcu_x * u32::from(comp.h_sampling) + bx,
                                mcu_y * u32::from(comp.v_sampling) + by,
                            )
                        } else {
                            (mcu_x, mcu_y)
                        };

                        let block = coefs[sc.frame_index].block_mut(block_x, block_y);

                        if is_dc {
                            decode_dc_block(
                                &mut reader,
                                scan,
                                sc,
                                &ctx.dc_tables,
                                &mut dc_pred[sc.frame_index],
                                block,
                            )?;
                        } else {
                            decode_ac_block(
                                &mut reader,
                                scan,
                                sc,
                                &ctx.ac_tables,
                                block,
                                &mut eob_run,
                            )?;
                        }
                    }
                }
            }

            mcu_since_restart += 1;
        }
    }

    reader.byte_align();
    Ok(reader.resync_pos_for_segments())
}

/// DC scan (Ss = Se = 0): initial — обычный DC с `<< Al`,
/// refinement — 1 бит, добавляется в позицию `Al`.
fn decode_dc_block(
    reader: &mut JpegBitReader<'_>,
    scan: &ScanInfo,
    sc: &ScanComponent,
    dc_tables: &[Option<HuffmanTable>; 4],
    dc_pred: &mut i32,
    block: &mut [i32],
) -> Result<(), JpegError> {
    if scan.ah == 0 {
        let table = dc_tables[sc.dc_table as usize].as_ref().ok_or(
            JpegError::MissingHuffmanTable {
                class: 0,
                id: sc.dc_table,
            },
        )?;
        let s = table.decode(reader)?;
        if s > 15 {
            return Err(JpegError::BadCoefficientSize(s));
        }
        let bits = reader.read_bits(s)?;
        let delta = extend(bits, s);
        *dc_pred += delta;
        block[0] = *dc_pred << scan.al;
    } else {
        let bit = i32::from(reader.read_bit()?);
        block[0] |= bit << scan.al;
    }
    Ok(())
}

/// AC scan (1 ≤ Ss ≤ Se ≤ 63). Initial vs refinement по `scan.ah`.
fn decode_ac_block(
    reader: &mut JpegBitReader<'_>,
    scan: &ScanInfo,
    sc: &ScanComponent,
    ac_tables: &[Option<HuffmanTable>; 4],
    block: &mut [i32],
    eob_run: &mut u32,
) -> Result<(), JpegError> {
    let ac_table = ac_tables[sc.ac_table as usize].as_ref().ok_or(
        JpegError::MissingHuffmanTable {
            class: 1,
            id: sc.ac_table,
        },
    )?;

    if scan.ah == 0 {
        decode_ac_initial(reader, scan, ac_table, block, eob_run)
    } else {
        decode_ac_refinement(reader, scan, ac_table, block, eob_run)
    }
}

/// AC initial scan (§G.1.2.2): обычный RLE+EOBn по [Ss..=Se], значения `<< Al`.
fn decode_ac_initial(
    reader: &mut JpegBitReader<'_>,
    scan: &ScanInfo,
    ac_table: &HuffmanTable,
    block: &mut [i32],
    eob_run: &mut u32,
) -> Result<(), JpegError> {
    if *eob_run > 0 {
        *eob_run -= 1;
        return Ok(());
    }
    let se = scan.se as usize;
    let mut k = scan.ss as usize;
    while k <= se {
        let rs = ac_table.decode(reader)?;
        let run = (rs >> 4) as usize;
        let size = rs & 0x0F;
        if size == 0 {
            if run == 15 {
                // ZRL: 16 нулей подряд.
                k += 16;
                if k > se + 1 {
                    return Err(JpegError::BadCoefficientPosition(k));
                }
                continue;
            }
            // EOBn: следующие (1<<run)+extra-1 блоков тоже пустые (для этого spectral band-а).
            let r = run as u8;
            *eob_run = 1u32 << r;
            if r > 0 {
                *eob_run += u32::from(reader.read_bits(r)?);
            }
            *eob_run -= 1; // текущий блок учитывается
            break;
        }
        if size > 10 {
            // AC magnitude ≤ 10 бит по spec.
            return Err(JpegError::BadCoefficientSize(size));
        }
        k += run;
        if k > se {
            return Err(JpegError::BadCoefficientPosition(k));
        }
        let bits = reader.read_bits(size)?;
        let value = extend(bits, size);
        block[ZIGZAG[k]] = value << scan.al;
        k += 1;
    }
    Ok(())
}

/// AC refinement scan (§G.1.2.3): refine существующих non-zero коэффициентов
/// 1 битом каждый; новые non-zero (от RS с size=1) вставляются в zero-позицию
/// после пропуска `run` zero-positions. ZRL = пропустить 16 zero-positions.
/// EOBn = войти в EOB-mode для текущего блока и (eob_run-1) следующих.
fn decode_ac_refinement(
    reader: &mut JpegBitReader<'_>,
    scan: &ScanInfo,
    ac_table: &HuffmanTable,
    block: &mut [i32],
    eob_run: &mut u32,
) -> Result<(), JpegError> {
    let p1: i32 = 1 << scan.al;
    let m1: i32 = -p1;
    let se = scan.se as usize;
    let mut k = scan.ss as usize;

    if *eob_run == 0 {
        'outer: while k <= se {
            let rs = ac_table.decode(reader)?;
            let mut run = (rs >> 4) as usize;
            let size = rs & 0x0F;

            let new_value: i32 = if size == 0 {
                if run < 15 {
                    // EOBn: устанавливаем eob_run, выходим в EOB-режим.
                    let r = run as u8;
                    *eob_run = 1u32 << r;
                    if r > 0 {
                        *eob_run += u32::from(reader.read_bits(r)?);
                    }
                    break 'outer;
                }
                // ZRL: пропускаем 16 zero-positions, без нового non-zero.
                0
            } else if size == 1 {
                let bit = reader.read_bit()?;
                if bit == 1 {
                    p1
                } else {
                    m1
                }
            } else {
                return Err(JpegError::BadCoefficientSize(size));
            };

            // Refine existing non-zero и skip zero-positions Z раз (Z = run+1 для size=0/ZRL,
            // Z = run+1 для size=1 — мы выйдем когда run станет -1, т.е. после run+1
            // zero-positions). В коде мы выходим при run == 0 на следующей zero-position,
            // т.е. фактически пропускаем `run` zeros и оставляем k на (run+1)-й zero-position
            // (там размещаем new_value, если size=1).
            while k <= se {
                let pos = ZIGZAG[k];
                if block[pos] != 0 {
                    let bit = reader.read_bit()?;
                    if bit == 1 {
                        if block[pos] > 0 {
                            block[pos] += p1;
                        } else {
                            block[pos] -= p1;
                        }
                    }
                    k += 1;
                } else {
                    if run == 0 {
                        break;
                    }
                    run -= 1;
                    k += 1;
                }
            }

            if k > se {
                // Дошли до конца блока, не разместив new_value — это нормально только
                // если new_value == 0 (ZRL у самого конца). Иначе corrupted stream.
                if new_value != 0 {
                    return Err(JpegError::BadCoefficientPosition(k));
                }
                break;
            }

            if new_value != 0 {
                block[ZIGZAG[k]] = new_value;
            }
            k += 1;
        }
    }

    // EOB-run mode: refine оставшиеся non-zero в [k..=Se], decrement eob_run.
    if *eob_run > 0 {
        while k <= se {
            let pos = ZIGZAG[k];
            if block[pos] != 0 {
                let bit = reader.read_bit()?;
                if bit == 1 {
                    if block[pos] > 0 {
                        block[pos] += p1;
                    } else {
                        block[pos] -= p1;
                    }
                }
            }
            k += 1;
        }
        *eob_run -= 1;
    }

    Ok(())
}

/// `EXTEND` процедура §F.2.1.1 (та же, что в baseline scan).
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

/// После всех scans: dequantize + IDCT + YCbCr→RGB (если 3 компонента) или
/// просто Gray8 (1 компонент).
fn finalize_pixels(
    frame: &Frame,
    quant_tables: &[Option<[u16; 64]>; 4],
    coefs: &[ComponentCoefs],
    mcus_per_row: u32,
    mcus_per_col: u32,
) -> Result<Vec<u8>, JpegError> {
    let h_max = u32::from(frame.h_max);
    let v_max = u32::from(frame.v_max);
    let width = u32::from(frame.width);
    let height = u32::from(frame.height);

    // Component pixel grids (Vec<u8>) — same layout, что в baseline scan.
    let mut grids: Vec<Vec<u8>> = Vec::with_capacity(frame.components.len());
    let mut grid_widths = Vec::with_capacity(frame.components.len());

    for (ci, c) in frame.components.iter().enumerate() {
        let gw = mcus_per_row * u32::from(c.h_sampling) * 8;
        let gh = mcus_per_col * u32::from(c.v_sampling) * 8;
        let mut grid = vec![0u8; (gw * gh) as usize];
        let qt = quant_tables[c.qt_id as usize]
            .as_ref()
            .ok_or(JpegError::MissingQuantTable(c.qt_id))?;

        let bh = coefs[ci].blocks_h;
        let bv = coefs[ci].blocks_v;
        for by in 0..bv {
            for bx in 0..bh {
                let mut block = [0i32; 64];
                let src = &coefs[ci].data[((by * bh + bx) * 64) as usize..][..64];
                for k in 0..64 {
                    block[k] = src[k] * i32::from(qt[k]);
                }
                idct_8x8(&mut block);

                let gx = bx * 8;
                let gy = by * 8;
                for y in 0..8 {
                    for x in 0..8 {
                        let pos = ((gy + y) * gw + gx + x) as usize;
                        grid[pos] = block[(y * 8 + x) as usize] as u8;
                    }
                }
            }
        }
        grids.push(grid);
        grid_widths.push(gw);
    }

    if frame.components.len() == 1 {
        let gw = grid_widths[0];
        let mut out = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                out.push(grids[0][(y * gw + x) as usize]);
            }
        }
        Ok(out)
    } else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extend_matches_baseline_semantics() {
        // size=0 → 0; size>0 → signed magnitude.
        assert_eq!(extend(0, 0), 0);
        assert_eq!(extend(0, 1), -1);
        assert_eq!(extend(1, 1), 1);
        assert_eq!(extend(0b00, 2), -3);
        assert_eq!(extend(0b11, 2), 3);
    }

    #[test]
    fn coef_buffer_block_indexing_is_row_major() {
        let mut c = ComponentCoefs::new(2, 2);
        c.block_mut(0, 0)[0] = 1;
        c.block_mut(1, 0)[0] = 2;
        c.block_mut(0, 1)[0] = 3;
        c.block_mut(1, 1)[63] = 4;
        // Layout: блоки идут row-major (block_y × blocks_h + block_x), внутри
        // блока — 64 i32. Сверка через прямой индекс.
        assert_eq!(c.data[0], 1);
        assert_eq!(c.data[64], 2);
        assert_eq!(c.data[128], 3);
        assert_eq!(c.data[192 + 63], 4);
    }
}
