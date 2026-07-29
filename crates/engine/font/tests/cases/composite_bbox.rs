//! BUG-423 — composite-глиф с битым bbox в заголовке.
//!
//! Реальные шрифты пишут в заголовок composite-глифа не то, что требует
//! спека: «YS Text Home» (ya.ru) хранит у кириллических букв, собранных из
//! латинских (`а`←`a`, `с`←`c`, `о`←`o`, `е`←`e`, `р`←`p`), четвёрку вида
//! `(advance, xMin, yMin, xMax)` — то есть `x_max < x_min`. Растеризатор на
//! таком боксе получал отрицательную ширину и возвращал `None`: буква не
//! рисовалась вовсе, а advance (из `hmtx`) оставался верным, поэтому на
//! странице оставался пробел ровно её ширины.
//!
//! Здесь тот же шрифт собирается синтетически: glyph 0 — квадрат с честным
//! bbox, glyph 1 — composite-ссылка на него с вывернутым заголовком.

use lumen_font::{Font, Outline, Rasterizer};

const UNITS_PER_EM: u16 = 1000;
/// Квадрат base-глифа в font units.
const SQUARE: (i16, i16, i16, i16) = (0, 0, 100, 100);
/// Заведомо битый bbox composite-заголовка: `x_max < x_min` (как в YS Text).
const INVERTED: (i16, i16, i16, i16) = (717, 46, -10, 465);

fn write_offset_table(out: &mut Vec<u8>, num_tables: u16) {
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // sfnt version 1.0
    out.extend_from_slice(&num_tables.to_be_bytes());
    out.extend_from_slice(&[0u8; 6]); // search_range / entry_selector / range_shift
}

fn write_record(out: &mut Vec<u8>, tag: &[u8; 4], offset: u32, length: u32) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&0u32.to_be_bytes()); // checksum не валидируется
    out.extend_from_slice(&offset.to_be_bytes());
    out.extend_from_slice(&length.to_be_bytes());
}

fn build_head() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    out.extend_from_slice(&0u32.to_be_bytes()); // fontRevision
    out.extend_from_slice(&0u32.to_be_bytes()); // checkSumAdjustment
    out.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // MAGIC_NUMBER
    out.extend_from_slice(&0u16.to_be_bytes()); // flags
    out.extend_from_slice(&UNITS_PER_EM.to_be_bytes());
    out.extend_from_slice(&[0u8; 16]); // created + modified
    out.extend_from_slice(&0i16.to_be_bytes()); // xMin
    out.extend_from_slice(&0i16.to_be_bytes()); // yMin
    out.extend_from_slice(&1000i16.to_be_bytes()); // xMax
    out.extend_from_slice(&1000i16.to_be_bytes()); // yMax
    out.extend_from_slice(&[0u8; 6]); // macStyle + lowestRecPPEM + fontDirectionHint
    out.extend_from_slice(&0i16.to_be_bytes()); // indexToLocFormat = short
    out.extend_from_slice(&0i16.to_be_bytes()); // glyphDataFormat
    out
}

fn build_maxp(num_glyphs: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version 1.0
    out.extend_from_slice(&num_glyphs.to_be_bytes());
    out.extend_from_slice(&[0u8; 26]);
    out
}

/// Simple glyph: один контур-квадрат, 4 точки on-curve, дельты в i16.
fn build_square_glyph(bbox: (i16, i16, i16, i16)) -> Vec<u8> {
    let points = [(SQUARE.0, SQUARE.1), (SQUARE.2, SQUARE.1), (SQUARE.2, SQUARE.3), (SQUARE.0, SQUARE.3)];
    let mut out = Vec::new();
    out.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours
    out.extend_from_slice(&bbox.0.to_be_bytes());
    out.extend_from_slice(&bbox.1.to_be_bytes());
    out.extend_from_slice(&bbox.2.to_be_bytes());
    out.extend_from_slice(&bbox.3.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes()); // endPtsOfContours[0]
    out.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
    // Флаги: ON_CURVE (0x01) без short-форм — координаты идут как i16-дельты.
    out.extend(std::iter::repeat_n(0x01u8, points.len()));
    let mut cur = 0i32;
    for (x, _) in points {
        out.extend_from_slice(&((x as i32 - cur) as i16).to_be_bytes());
        cur = x as i32;
    }
    let mut cur = 0i32;
    for (_, y) in points {
        out.extend_from_slice(&((y as i32 - cur) as i16).to_be_bytes());
        cur = y as i32;
    }
    if out.len() % 2 != 0 {
        out.push(0);
    }
    out
}

/// Composite glyph: одна ссылка на `base_gid` со смещением (0, 0) и
/// заголовочным bbox `bbox` (в тесте — заведомо вывернутым).
fn build_composite_glyph(base_gid: u16, bbox: (i16, i16, i16, i16)) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(-1i16).to_be_bytes()); // numberOfContours = -1
    out.extend_from_slice(&bbox.0.to_be_bytes());
    out.extend_from_slice(&bbox.1.to_be_bytes());
    out.extend_from_slice(&bbox.2.to_be_bytes());
    out.extend_from_slice(&bbox.3.to_be_bytes());
    out.extend_from_slice(&0x0002u16.to_be_bytes()); // flags = ARGS_ARE_XY_VALUES
    out.extend_from_slice(&base_gid.to_be_bytes());
    out.push(0); // arg1 = dx = 0 (i8)
    out.push(0); // arg2 = dy = 0 (i8)
    out
}

/// TTF из двух глифов: 0 — квадрат, 1 — composite-ссылка на него.
fn build_font(composite_bbox: (i16, i16, i16, i16)) -> Vec<u8> {
    let simple = build_square_glyph(SQUARE);
    let composite = build_composite_glyph(0, composite_bbox);
    let mut glyf = simple.clone();
    glyf.extend_from_slice(&composite);
    assert_eq!(simple.len() % 2, 0, "short loca требует чётных смещений");
    assert_eq!(glyf.len() % 2, 0);
    let loca: Vec<u8> = [0u16, (simple.len() / 2) as u16, (glyf.len() / 2) as u16]
        .iter()
        .flat_map(|w| w.to_be_bytes())
        .collect();

    let head = build_head();
    let maxp = build_maxp(2);
    let entries: Vec<(&[u8; 4], &[u8])> = vec![
        (b"head", &head),
        (b"maxp", &maxp),
        (b"loca", &loca),
        (b"glyf", &glyf),
    ];

    let mut out = Vec::new();
    write_offset_table(&mut out, entries.len() as u16);
    let mut offset = 12 + 16 * entries.len() as u32;
    for (tag, data) in &entries {
        write_record(&mut out, tag, offset, data.len() as u32);
        offset += data.len().next_multiple_of(4) as u32;
    }
    for (_, data) in &entries {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

#[test]
fn composite_with_inverted_header_bbox_still_rasterizes() {
    let bytes = build_font(INVERTED);
    let font = Font::parse(&bytes).unwrap();

    // Сырой глиф — действительно composite с вывернутым заголовком: иначе
    // тест проверял бы не тот путь.
    let raw = font.glyph(1).unwrap().expect("composite glyph");
    assert!(matches!(raw.outline, Outline::Composite(_)));
    assert!(raw.bbox.x_max < raw.bbox.x_min, "заголовок должен быть битым");

    let resolved = font.glyph_resolved(1).unwrap().expect("resolved");
    let Outline::Simple(contours) = &resolved.outline else {
        panic!("composite обязан развернуться в Simple");
    };
    assert_eq!(contours.len(), 1);
    // bbox посчитан по точкам компонента, а не взят из битого заголовка.
    assert_eq!(
        (resolved.bbox.x_min, resolved.bbox.y_min, resolved.bbox.x_max, resolved.bbox.y_max),
        SQUARE,
    );

    // Растеризация даёт непустой битмап с чернилами — до BUG-423 здесь был None.
    let bitmap = Rasterizer::new(32.0, UNITS_PER_EM)
        .rasterize(&resolved)
        .expect("глиф с битым заголовочным bbox обязан растеризоваться");
    assert!(bitmap.width >= 3 && bitmap.height >= 3);
    assert!(
        bitmap.pixels.iter().any(|&p| p > 16),
        "битмап не должен быть пустым"
    );
}

#[test]
fn composite_with_valid_header_bbox_matches_component() {
    // Контроль: у честного заголовка результат тот же — правка не меняет
    // геометрию корректных шрифтов.
    let font_bytes = build_font(SQUARE);
    let font = Font::parse(&font_bytes).unwrap();
    let resolved = font.glyph_resolved(1).unwrap().expect("resolved");
    assert_eq!(
        (resolved.bbox.x_min, resolved.bbox.y_min, resolved.bbox.x_max, resolved.bbox.y_max),
        SQUARE,
    );
}
