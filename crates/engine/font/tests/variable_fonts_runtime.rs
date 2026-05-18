//! Integration: `Font::glyph_resolved_with_coords` действительно применяет
//! gvar deltas в указанной точке пространства осей.
//!
//! Bundled Inter-Regular — статический шрифт без `gvar` (его покрывают
//! сценарии в `inter_real_font.rs`: coords игнорируется). Чтобы проверить
//! реальное применение deltas, собираем синтетический минимальный TTF:
//! head + maxp + loca + glyf (один simple glyph) + gvar (один TupleVariation
//! на одной оси). Этого достаточно для `Font::glyph_resolved_with_coords`
//! — он требует только loca/glyf для чтения base outline и gvar для deltas.

use lumen_font::{Font, OffsetTable, Outline};

// ───────────────── TTF builders ─────────────────

fn write_offset_table(out: &mut Vec<u8>, num_tables: u16) {
    out.extend_from_slice(&OffsetTable::SFNT_TRUETYPE.to_be_bytes());
    out.extend_from_slice(&num_tables.to_be_bytes());
    // search_range / entry_selector / range_shift — парсер их не использует,
    // оставляем нули. По spec они оптимизация для binary search в каталоге.
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
}

fn write_record(out: &mut Vec<u8>, tag: &[u8; 4], offset: u32, length: u32) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&0u32.to_be_bytes()); // checksum — Font::parse не валидирует
    out.extend_from_slice(&offset.to_be_bytes());
    out.extend_from_slice(&length.to_be_bytes());
}

fn build_head(units_per_em: u16, loc_format: i16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x00010000u32.to_be_bytes()); // version
    out.extend_from_slice(&0u32.to_be_bytes()); // fontRevision
    out.extend_from_slice(&0u32.to_be_bytes()); // checkSumAdjustment
    out.extend_from_slice(&0x5F0F3CF5u32.to_be_bytes()); // MAGIC_NUMBER
    out.extend_from_slice(&0u16.to_be_bytes()); // flags
    out.extend_from_slice(&units_per_em.to_be_bytes());
    out.extend_from_slice(&[0u8; 16]); // created + modified
    out.extend_from_slice(&(-100i16).to_be_bytes()); // xMin
    out.extend_from_slice(&(-200i16).to_be_bytes()); // yMin
    out.extend_from_slice(&1100i16.to_be_bytes()); // xMax
    out.extend_from_slice(&900i16.to_be_bytes()); // yMax
    out.extend_from_slice(&[0u8; 6]); // macStyle + lowestRecPPEM + fontDirectionHint
    out.extend_from_slice(&loc_format.to_be_bytes());
    out.extend_from_slice(&0i16.to_be_bytes()); // glyphDataFormat
    out
}

fn build_maxp(num_glyphs: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x00010000u32.to_be_bytes()); // version 1.0 (TrueType)
    out.extend_from_slice(&num_glyphs.to_be_bytes());
    // Остальные 26 байт v1.0 не читаются нашим парсером.
    out.extend_from_slice(&[0u8; 26]);
    out
}

/// loca short-format: u16 offsets, value × 2 = actual byte-offset в glyf.
fn build_loca_short(offsets_words: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    for &w in offsets_words {
        out.extend_from_slice(&w.to_be_bytes());
    }
    out
}

/// Собирает простой glyph с 1 контуром квадрат (4 угла), все on-curve, все
/// флаги = SHORT_X | SHORT_Y | X_POSITIVE | Y_POSITIVE для компактности.
/// Точки задаются абсолютными координатами; запишутся как dx/dy от
/// предыдущей точки (current_x starts at 0).
fn build_simple_glyph_square(points: [(i16, i16); 4], bbox: (i16, i16, i16, i16)) -> Vec<u8> {
    let mut out = Vec::new();
    // GlyphHeader
    out.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours = 1
    out.extend_from_slice(&bbox.0.to_be_bytes()); // xMin
    out.extend_from_slice(&bbox.1.to_be_bytes()); // yMin
    out.extend_from_slice(&bbox.2.to_be_bytes()); // xMax
    out.extend_from_slice(&bbox.3.to_be_bytes()); // yMax
    // SimpleGlyph
    out.extend_from_slice(&3u16.to_be_bytes()); // endPtsOfContours[0] = 3 (4-я точка index 3)
    out.extend_from_slice(&0u16.to_be_bytes()); // instructionLength = 0
    // Flags: 4 × (ON_CURVE | SHORT_X | SHORT_Y), без REPEAT (для простоты).
    // X/Y_POSITIVE задаём индивидуально в зависимости от знака delta.
    // Для простоты соберём deltas вначале, потом флаги вместе с ними.
    let mut current_x: i32 = 0;
    let mut current_y: i32 = 0;
    let mut flags: Vec<u8> = Vec::with_capacity(4);
    let mut x_bytes: Vec<u8> = Vec::with_capacity(4);
    let mut y_bytes: Vec<u8> = Vec::with_capacity(4);
    for (px, py) in points.iter() {
        let dx = (*px as i32) - current_x;
        let dy = (*py as i32) - current_y;
        current_x = *px as i32;
        current_y = *py as i32;
        let mut f = 0x01u8; // ON_CURVE
        // X — short если |dx| <= 255; иначе long (2 байта signed).
        if (-255..=255).contains(&dx) {
            f |= 0x02; // SHORT_X
            if dx >= 0 {
                f |= 0x10; // X_POSITIVE
                x_bytes.push(dx as u8);
            } else {
                x_bytes.push((-dx) as u8);
            }
        } else {
            x_bytes.extend_from_slice(&(dx as i16).to_be_bytes());
        }
        if (-255..=255).contains(&dy) {
            f |= 0x04; // SHORT_Y
            if dy >= 0 {
                f |= 0x20; // Y_POSITIVE
                y_bytes.push(dy as u8);
            } else {
                y_bytes.push((-dy) as u8);
            }
        } else {
            y_bytes.extend_from_slice(&(dy as i16).to_be_bytes());
        }
        flags.push(f);
    }
    out.extend(flags);
    out.extend(x_bytes);
    out.extend(y_bytes);
    // Pad to even length — Font::parse не требует, но loca short требует /2.
    if out.len() % 2 != 0 {
        out.push(0);
    }
    out
}

// ───────────────── gvar builders ─────────────────

const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;

fn put_f2dot14(v: f32) -> [u8; 2] {
    let scaled = (v * 16384.0).round() as i16;
    scaled.to_be_bytes()
}

/// Один GlyphVariationData блок: 1 tuple variation, embedded peak,
/// points=All (no PRIVATE_POINT_NUMBERS), word-packed x/y deltas.
fn build_glyph_blob_all_points(peak: &[f32], x_deltas: &[i16], y_deltas: &[i16]) -> Vec<u8> {
    assert_eq!(x_deltas.len(), y_deltas.len());
    let pack_word_run = |vals: &[i16]| -> Vec<u8> {
        let mut out = Vec::new();
        if !vals.is_empty() {
            assert!(vals.len() <= 64, "test helper supports run ≤64");
            out.push(0x40 | ((vals.len() as u8) - 1)); // word-run control
            for &v in vals {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        out
    };
    let x_packed = pack_word_run(x_deltas);
    let y_packed = pack_word_run(y_deltas);
    let tuple_data_size = (x_packed.len() + y_packed.len()) as u16;

    let mut header = Vec::new();
    header.extend_from_slice(&tuple_data_size.to_be_bytes());
    header.extend_from_slice(&EMBEDDED_PEAK_TUPLE.to_be_bytes());
    for &p in peak {
        header.extend_from_slice(&put_f2dot14(p));
    }

    let mut blob = Vec::new();
    blob.extend_from_slice(&1u16.to_be_bytes()); // tupleVariationCount = 1 (нет shared points)
    let data_offset = (4 + header.len()) as u16;
    blob.extend_from_slice(&data_offset.to_be_bytes());
    blob.extend_from_slice(&header);
    blob.extend_from_slice(&x_packed);
    blob.extend_from_slice(&y_packed);
    if blob.len() % 2 != 0 {
        blob.push(0);
    }
    blob
}

fn build_gvar(axis_count: u16, glyph_blobs: &[Vec<u8>]) -> Vec<u8> {
    let glyph_count = glyph_blobs.len() as u16;
    let shared_tuple_count = 0u16;
    let header_size = 20u32;
    let offsets_size = (glyph_count as u32 + 1) * 2; // short offsets
    let shared_tuples_offset = header_size + offsets_size;
    let shared_tuples_bytes_len = shared_tuple_count as u32 * axis_count as u32 * 2;
    let glyph_data_array_offset = shared_tuples_offset + shared_tuples_bytes_len;

    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes()); // major
    out.extend_from_slice(&0u16.to_be_bytes()); // minor
    out.extend_from_slice(&axis_count.to_be_bytes());
    out.extend_from_slice(&shared_tuple_count.to_be_bytes());
    out.extend_from_slice(&shared_tuples_offset.to_be_bytes());
    out.extend_from_slice(&glyph_count.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // flags = 0 (short offsets)
    out.extend_from_slice(&glyph_data_array_offset.to_be_bytes());

    let mut acc: u32 = 0;
    let push_offset = |out: &mut Vec<u8>, v: u32| {
        assert_eq!(v % 2, 0, "short offsets must be 2-byte aligned");
        out.extend_from_slice(&((v / 2) as u16).to_be_bytes());
    };
    push_offset(&mut out, acc);
    for blob in glyph_blobs {
        acc += blob.len() as u32;
        push_offset(&mut out, acc);
    }
    // Shared tuples (none).
    for blob in glyph_blobs {
        out.extend_from_slice(blob);
    }
    out
}

// ───────────────── Font assembly ─────────────────

/// Собирает полный TTF с одним simple glyph + gvar variation.
/// Возвращает байты + ожидаемые base-точки glyph 0.
fn make_font_with_one_variable_glyph() -> Vec<u8> {
    // Base outline: square (0,0)..(100,100), 4 точки on-curve.
    let base_points = [(0, 0), (100, 0), (100, 100), (0, 100)];
    let glyf_bytes = build_simple_glyph_square(base_points, (0, 0, 100, 100));

    // loca: glyph 0 at offset 0, end at offset glyf_bytes.len(); +1 sentinel.
    // short format хранит offset / 2 — glyf длина обязана быть кратна 2.
    assert_eq!(glyf_bytes.len() % 2, 0);
    let loca_bytes = build_loca_short(&[
        0,
        (glyf_bytes.len() / 2) as u16,
    ]);

    let head_bytes = build_head(1000, 0); // 1000 upm, short loca
    let maxp_bytes = build_maxp(1);

    // gvar: 1 axis. Glyph 0 has 1 tuple variation at peak=1.0 что сдвигает
    // все 4 outline-точки на (+10, +0). Phantom-points (4 штуки) тоже
    // указаны в deltas, но apply_variations_to_simple_outline их игнорирует.
    let glyph0_blob = build_glyph_blob_all_points(
        &[1.0],
        &[10, 10, 10, 10, 0, 0, 0, 0], // 4 outline x-deltas + 4 phantom
        &[0, 0, 0, 0, 0, 0, 0, 0],     // 4 outline y-deltas + 4 phantom
    );
    let gvar_bytes = build_gvar(1, std::slice::from_ref(&glyph0_blob));

    // Сборка: offset table + 5 records + tables.
    let num_tables = 5u16;
    let header_size = 12u32; // offset table
    let records_size = 16u32 * num_tables as u32;
    let mut tables_offset = header_size + records_size;
    let mut layout: Vec<(&[u8; 4], &[u8])> = Vec::new();
    // glyf и loca должны идти в алфавитном порядке tag-ов в records или
    // парсер ищет по тегу — порядок records не важен. Соблюдаем порядок
    // tables_offset, считаем offsets и lengths.
    let entries: Vec<(&[u8; 4], &[u8])> = vec![
        (b"head", &head_bytes),
        (b"maxp", &maxp_bytes),
        (b"loca", &loca_bytes),
        (b"glyf", &glyf_bytes),
        (b"gvar", &gvar_bytes),
    ];
    for e in &entries {
        layout.push(*e);
    }
    let mut out = Vec::new();
    write_offset_table(&mut out, num_tables);
    // Записываем records — все pad-аем до 4 байт.
    let mut current_offset = tables_offset;
    for (tag, data) in &layout {
        write_record(&mut out, tag, current_offset, data.len() as u32);
        let mut size = data.len() as u32;
        // 4-byte alignment между таблицами (Font::parse не требует, но это
        // safer для соответствия spec).
        if !size.is_multiple_of(4) {
            size += 4 - (size % 4);
        }
        current_offset += size;
    }
    tables_offset = header_size + records_size;
    // Записываем данные в том же порядке.
    let mut writer_pos = tables_offset;
    for (_tag, data) in &layout {
        out.extend_from_slice(data);
        writer_pos += data.len() as u32;
        // Pad to 4 bytes.
        while !writer_pos.is_multiple_of(4) {
            out.push(0);
            writer_pos += 1;
        }
    }
    out
}

// ───────────────── Tests ─────────────────

#[test]
fn synthetic_font_parses() {
    let data = make_font_with_one_variable_glyph();
    let font = Font::parse(&data).expect("synthetic font parses");
    assert_eq!(font.maxp().unwrap().num_glyphs, 1);
    // gvar присутствует — синтетический шрифт собран с ним.
    let gvar = font.gvar().expect("synthetic font has gvar");
    assert_eq!(gvar.axis_count, 1);
    assert_eq!(gvar.glyph_count, 1);
}

#[test]
fn glyph_resolved_at_axis_zero_matches_base_outline() {
    let data = make_font_with_one_variable_glyph();
    let font = Font::parse(&data).unwrap();
    // coords=[0.0] → scalar=0 для peak=1.0 variation → deltas не применяются.
    let g = font.glyph_resolved_with_coords(0, &[0.0]).unwrap().unwrap();
    let Outline::Simple(contours) = g.outline else {
        panic!("expected Simple");
    };
    assert_eq!(contours.len(), 1);
    let pts = &contours[0].points;
    assert_eq!(pts.len(), 4);
    assert_eq!((pts[0].x, pts[0].y), (0, 0));
    assert_eq!((pts[1].x, pts[1].y), (100, 0));
    assert_eq!((pts[2].x, pts[2].y), (100, 100));
    assert_eq!((pts[3].x, pts[3].y), (0, 100));
}

#[test]
fn glyph_resolved_at_axis_one_shifts_all_points_by_full_delta() {
    let data = make_font_with_one_variable_glyph();
    let font = Font::parse(&data).unwrap();
    // coords=[1.0] → scalar=1.0 → каждая outline-точка получает (+10, 0).
    let g = font.glyph_resolved_with_coords(0, &[1.0]).unwrap().unwrap();
    let Outline::Simple(contours) = g.outline else {
        panic!("expected Simple");
    };
    let pts = &contours[0].points;
    assert_eq!((pts[0].x, pts[0].y), (10, 0));
    assert_eq!((pts[1].x, pts[1].y), (110, 0));
    assert_eq!((pts[2].x, pts[2].y), (110, 100));
    assert_eq!((pts[3].x, pts[3].y), (10, 100));
}

#[test]
fn glyph_resolved_at_axis_half_shifts_by_half_delta() {
    let data = make_font_with_one_variable_glyph();
    let font = Font::parse(&data).unwrap();
    // coords=[0.5] → scalar=0.5 (linear от 0 до peak=1.0) → delta = +5 по x.
    let g = font.glyph_resolved_with_coords(0, &[0.5]).unwrap().unwrap();
    let Outline::Simple(contours) = g.outline else {
        panic!("expected Simple");
    };
    let pts = &contours[0].points;
    assert_eq!((pts[0].x, pts[0].y), (5, 0));
    assert_eq!((pts[1].x, pts[1].y), (105, 0));
}

#[test]
fn glyph_resolved_with_empty_coords_skips_gvar() {
    let data = make_font_with_one_variable_glyph();
    let font = Font::parse(&data).unwrap();
    // Empty coords — fast path: gvar даже не открывается.
    let g = font.glyph_resolved_with_coords(0, &[]).unwrap().unwrap();
    let base = font.glyph_resolved(0).unwrap().unwrap();
    let Outline::Simple(c1) = g.outline else { panic!() };
    let Outline::Simple(c2) = base.outline else { panic!() };
    assert_eq!(c1.len(), c2.len());
    for (a, b) in c1.iter().zip(c2.iter()) {
        assert_eq!(a.points.len(), b.points.len());
        for (p, q) in a.points.iter().zip(b.points.iter()) {
            assert_eq!(p, q);
        }
    }
}

#[test]
fn coords_length_mismatch_silently_skips_variation() {
    let data = make_font_with_one_variable_glyph();
    let font = Font::parse(&data).unwrap();
    // gvar.axis_count = 1, передаём coords длиной 2 → defensive: variation
    // пропускается в apply_variations_to_simple_outline. Outline = base.
    let g = font.glyph_resolved_with_coords(0, &[1.0, 1.0]).unwrap().unwrap();
    let Outline::Simple(contours) = g.outline else {
        panic!("expected Simple");
    };
    let pts = &contours[0].points;
    assert_eq!((pts[0].x, pts[0].y), (0, 0));
    assert_eq!((pts[1].x, pts[1].y), (100, 0));
}
