//! `glyf` table — outline данные глифов.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/glyf>.
//!
//! Phase 0 — только simple glyphs (numberOfContours >= 0). Composite
//! glyphs (когда один глиф собран из других — `é` = `e` + acute и т.п.)
//! помечаем `Outline::Composite` и пропускаем; добавим, когда упрёмся
//! в конкретный шрифт, который их активно использует.
//!
//! Координаты глифа хранятся в font units (см. `head.units_per_em`),
//! дельта-кодированы, флаги поддерживают RLE через бит REPEAT.

use crate::binary::BinaryReader;
use crate::face::FontError;

// Биты в byte флага точки.
const FLAG_ON_CURVE: u8 = 0x01;
const FLAG_X_SHORT: u8 = 0x02;
const FLAG_Y_SHORT: u8 = 0x04;
const FLAG_REPEAT: u8 = 0x08;
const FLAG_X_SAME_OR_POSITIVE: u8 = 0x10;
const FLAG_Y_SAME_OR_POSITIVE: u8 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBox {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlinePoint {
    pub x: i16,
    pub y: i16,
    pub on_curve: bool,
}

#[derive(Debug, Clone)]
pub struct Contour {
    pub points: Vec<OutlinePoint>,
}

#[derive(Debug, Clone)]
pub enum Outline {
    Simple(Vec<Contour>),
    /// Composite glyph — собран из нескольких других глифов с трансформацией.
    /// Используется для оптимизации: например, кириллическая `А` ссылается на
    /// латинскую `A` без копирования контуров.
    Composite(Vec<CompositeComponent>),
}

/// Один компонент composite-глифа: ссылка на другой глиф + 2×2 матрица + offset.
///
/// Применение к точке: `(x', y') = (a·x + c·y + dx, b·x + d·y + dy)`,
/// где `transform = [a, b, c, d]`, `offset = (dx, dy)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeComponent {
    pub glyph_id: u16,
    pub transform: [f32; 4],
    pub offset: (f32, f32),
}

#[derive(Debug, Clone)]
pub struct Glyph {
    pub bbox: BoundingBox,
    pub outline: Outline,
}

impl Glyph {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let n_contours = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let bbox = BoundingBox {
            x_min: r.read_i16().ok_or(FontError::UnexpectedEof)?,
            y_min: r.read_i16().ok_or(FontError::UnexpectedEof)?,
            x_max: r.read_i16().ok_or(FontError::UnexpectedEof)?,
            y_max: r.read_i16().ok_or(FontError::UnexpectedEof)?,
        };

        let outline = if n_contours < 0 {
            Outline::Composite(parse_composite(&mut r)?)
        } else {
            Outline::Simple(parse_simple_outline(&mut r, n_contours as usize)?)
        };

        Ok(Self { bbox, outline })
    }
}

// Флаги composite-глифа.
const COMPOSITE_ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const COMPOSITE_ARGS_ARE_XY_VALUES: u16 = 0x0002;
const COMPOSITE_WE_HAVE_A_SCALE: u16 = 0x0008;
const COMPOSITE_MORE_COMPONENTS: u16 = 0x0020;
const COMPOSITE_WE_HAVE_X_AND_Y_SCALE: u16 = 0x0040;
const COMPOSITE_WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

fn parse_composite(r: &mut BinaryReader) -> Result<Vec<CompositeComponent>, FontError> {
    let mut components = Vec::new();
    loop {
        let flags = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        let glyph_id = r.read_u16().ok_or(FontError::UnexpectedEof)?;

        let (arg1, arg2) = if flags & COMPOSITE_ARG_1_AND_2_ARE_WORDS != 0 {
            let a = r.read_i16().ok_or(FontError::UnexpectedEof)? as f32;
            let b = r.read_i16().ok_or(FontError::UnexpectedEof)? as f32;
            (a, b)
        } else {
            // i8: один байт со знаком.
            let a = r.read_u8().ok_or(FontError::UnexpectedEof)? as i8 as f32;
            let b = r.read_u8().ok_or(FontError::UnexpectedEof)? as i8 as f32;
            (a, b)
        };

        // Если флаг ARGS_ARE_XY_VALUES — это (dx, dy) в font units.
        // Иначе — индексы точек для выравнивания (рудимент, в современных шрифтах
        // практически не встречается; считаем offset = (0, 0)).
        let offset = if flags & COMPOSITE_ARGS_ARE_XY_VALUES != 0 {
            (arg1, arg2)
        } else {
            (0.0, 0.0)
        };

        let transform = if flags & COMPOSITE_WE_HAVE_A_SCALE != 0 {
            let s = read_f2dot14(r)?;
            [s, 0.0, 0.0, s]
        } else if flags & COMPOSITE_WE_HAVE_X_AND_Y_SCALE != 0 {
            let xs = read_f2dot14(r)?;
            let ys = read_f2dot14(r)?;
            [xs, 0.0, 0.0, ys]
        } else if flags & COMPOSITE_WE_HAVE_A_TWO_BY_TWO != 0 {
            let a = read_f2dot14(r)?;
            let b = read_f2dot14(r)?;
            let c = read_f2dot14(r)?;
            let d = read_f2dot14(r)?;
            [a, b, c, d]
        } else {
            [1.0, 0.0, 0.0, 1.0]
        };

        components.push(CompositeComponent {
            glyph_id,
            transform,
            offset,
        });

        if flags & COMPOSITE_MORE_COMPONENTS == 0 {
            break;
        }
    }
    Ok(components)
}

/// F2DOT14: 16-битный fixed-point с 14 битами на дробную часть.
fn read_f2dot14(r: &mut BinaryReader) -> Result<f32, FontError> {
    let raw = r.read_i16().ok_or(FontError::UnexpectedEof)?;
    Ok(raw as f32 / 16384.0)
}

fn parse_simple_outline(
    r: &mut BinaryReader,
    n_contours: usize,
) -> Result<Vec<Contour>, FontError> {
    if n_contours == 0 {
        return Ok(Vec::new());
    }

    let mut end_pts = Vec::with_capacity(n_contours);
    for _ in 0..n_contours {
        end_pts.push(r.read_u16().ok_or(FontError::UnexpectedEof)?);
    }
    let total_points = *end_pts.last().unwrap() as usize + 1;

    // Пропускаем TrueType-инструкции (hinting) — Phase 0 без grid-fitting.
    let instr_len = r.read_u16().ok_or(FontError::UnexpectedEof)?;
    r.skip(instr_len as usize).ok_or(FontError::UnexpectedEof)?;

    let flags = read_flags(r, total_points)?;
    let x_coords = read_coords(r, &flags, FLAG_X_SHORT, FLAG_X_SAME_OR_POSITIVE)?;
    let y_coords = read_coords(r, &flags, FLAG_Y_SHORT, FLAG_Y_SAME_OR_POSITIVE)?;

    // Собираем контуры по end_pts.
    let mut contours = Vec::with_capacity(n_contours);
    let mut start = 0usize;
    for &end in &end_pts {
        let end_idx = end as usize;
        if end_idx >= total_points || end_idx < start {
            return Err(FontError::InvalidTable(*b"glyf"));
        }
        let mut points = Vec::with_capacity(end_idx - start + 1);
        for i in start..=end_idx {
            points.push(OutlinePoint {
                x: x_coords[i],
                y: y_coords[i],
                on_curve: flags[i] & FLAG_ON_CURVE != 0,
            });
        }
        contours.push(Contour { points });
        start = end_idx + 1;
    }
    Ok(contours)
}

fn read_flags(r: &mut BinaryReader, total_points: usize) -> Result<Vec<u8>, FontError> {
    let mut flags = Vec::with_capacity(total_points);
    while flags.len() < total_points {
        let f = r.read_u8().ok_or(FontError::UnexpectedEof)?;
        flags.push(f);
        if f & FLAG_REPEAT != 0 {
            let repeat = r.read_u8().ok_or(FontError::UnexpectedEof)? as usize;
            for _ in 0..repeat {
                if flags.len() >= total_points {
                    break;
                }
                flags.push(f);
            }
        }
    }
    Ok(flags)
}

fn read_coords(
    r: &mut BinaryReader,
    flags: &[u8],
    short_bit: u8,
    same_or_positive_bit: u8,
) -> Result<Vec<i16>, FontError> {
    let mut coords = Vec::with_capacity(flags.len());
    let mut current = 0i32;
    for &f in flags {
        let delta: i32 = if f & short_bit != 0 {
            // 1-байтная абсолютная величина; знак из same_or_positive_bit.
            let b = r.read_u8().ok_or(FontError::UnexpectedEof)? as i32;
            if f & same_or_positive_bit != 0 {
                b
            } else {
                -b
            }
        } else if f & same_or_positive_bit != 0 {
            // SAME: координата равна предыдущей, дельта = 0.
            0
        } else {
            // 2-байтная знаковая дельта.
            r.read_i16().ok_or(FontError::UnexpectedEof)? as i32
        };
        current = current.wrapping_add(delta);
        // Координата TTF — i16; для штатных шрифтов точно влезает.
        coords.push(current as i16);
    }
    Ok(coords)
}

/// Удобный view над байтами `glyf` для разбора глифа по offset/length из loca.
pub struct Glyf<'a> {
    data: &'a [u8],
}

impl<'a> Glyf<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn glyph_at(&self, offset: u32, length: u32) -> Result<Glyph, FontError> {
        let start = offset as usize;
        let end = start
            .checked_add(length as usize)
            .ok_or(FontError::UnexpectedEof)?;
        let bytes = self
            .data
            .get(start..end)
            .ok_or(FontError::InvalidTable(*b"glyf"))?;
        Glyph::parse(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Строит байты простого треугольного глифа (3 точки, все on-curve, 2-байтовые координаты).
    /// Точки: (0, 0), (100, 0), (50, 100).
    fn triangle_glyph_bytes() -> Vec<u8> {
        let mut out = Vec::new();
        // numberOfContours = 1
        out.extend_from_slice(&1i16.to_be_bytes());
        // bbox
        out.extend_from_slice(&0i16.to_be_bytes()); // x_min
        out.extend_from_slice(&0i16.to_be_bytes()); // y_min
        out.extend_from_slice(&100i16.to_be_bytes()); // x_max
        out.extend_from_slice(&100i16.to_be_bytes()); // y_max
        // endPtsOfContours[1] = [2]  (3 точки, индексы 0..2)
        out.extend_from_slice(&2u16.to_be_bytes());
        // instructionLength = 0
        out.extend_from_slice(&0u16.to_be_bytes());
        // flags: все on-curve, без short / same → 3 байта по 0x01
        out.push(0x01);
        out.push(0x01);
        out.push(0x01);
        // x deltas (2 байта каждая): +0, +100, -50
        out.extend_from_slice(&0i16.to_be_bytes());
        out.extend_from_slice(&100i16.to_be_bytes());
        out.extend_from_slice(&(-50i16).to_be_bytes());
        // y deltas: +0, +0, +100
        out.extend_from_slice(&0i16.to_be_bytes());
        out.extend_from_slice(&0i16.to_be_bytes());
        out.extend_from_slice(&100i16.to_be_bytes());
        out
    }

    #[test]
    fn parse_simple_triangle() {
        let bytes = triangle_glyph_bytes();
        let glyph = Glyph::parse(&bytes).unwrap();
        assert_eq!(
            glyph.bbox,
            BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 100,
                y_max: 100
            }
        );
        let Outline::Simple(contours) = &glyph.outline else {
            panic!("expected simple outline");
        };
        assert_eq!(contours.len(), 1);
        assert_eq!(contours[0].points.len(), 3);
        assert_eq!(
            contours[0].points[0],
            OutlinePoint {
                x: 0,
                y: 0,
                on_curve: true
            }
        );
        assert_eq!(
            contours[0].points[1],
            OutlinePoint {
                x: 100,
                y: 0,
                on_curve: true
            }
        );
        assert_eq!(
            contours[0].points[2],
            OutlinePoint {
                x: 50,
                y: 100,
                on_curve: true
            }
        );
    }

    #[test]
    fn composite_glyph_single_component_with_offset() {
        // Composite-глиф с одной ссылкой на другой glyph_id=5, offset (10, 20),
        // identity transform, без MORE_COMPONENTS.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-1i16).to_be_bytes()); // numberOfContours = -1
        bytes.extend_from_slice(&[0u8; 8]); // bbox
        // flags = ARGS_ARE_XY_VALUES (без words, без scale, без more)
        bytes.extend_from_slice(&0x0002u16.to_be_bytes());
        bytes.extend_from_slice(&5u16.to_be_bytes()); // glyph_id
        bytes.push(10u8); // arg1 (i8)
        bytes.push(20u8); // arg2 (i8)

        let glyph = Glyph::parse(&bytes).unwrap();
        let Outline::Composite(components) = &glyph.outline else {
            panic!("expected Composite, got {:?}", glyph.outline);
        };
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].glyph_id, 5);
        assert_eq!(components[0].offset, (10.0, 20.0));
        assert_eq!(components[0].transform, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn composite_glyph_with_words_and_scale() {
        // Composite: glyph_id=42, offset = i16 значения (300, -150),
        // uniform scale 0.5, нет MORE_COMPONENTS.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-1i16).to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        // flags = ARG_1_AND_2_ARE_WORDS | ARGS_ARE_XY_VALUES | WE_HAVE_A_SCALE
        bytes.extend_from_slice(&0x000Bu16.to_be_bytes());
        bytes.extend_from_slice(&42u16.to_be_bytes());
        bytes.extend_from_slice(&300i16.to_be_bytes());
        bytes.extend_from_slice(&(-150i16).to_be_bytes());
        // scale = 0.5 → F2DOT14: 0.5 × 16384 = 8192
        bytes.extend_from_slice(&8192i16.to_be_bytes());

        let glyph = Glyph::parse(&bytes).unwrap();
        let Outline::Composite(components) = &glyph.outline else {
            panic!();
        };
        assert_eq!(components[0].glyph_id, 42);
        assert_eq!(components[0].offset, (300.0, -150.0));
        assert_eq!(components[0].transform, [0.5, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn composite_glyph_two_components() {
        // Два компонента: glyph_id=1 со scale 1.0, glyph_id=2 без scale.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-1i16).to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);

        // Component 1: ARGS_XY | MORE_COMPONENTS
        bytes.extend_from_slice(&0x0022u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.push(0u8);
        bytes.push(0u8);

        // Component 2: ARGS_XY (без MORE_COMPONENTS — последний)
        bytes.extend_from_slice(&0x0002u16.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.push(5u8);
        bytes.push(10u8);

        let glyph = Glyph::parse(&bytes).unwrap();
        let Outline::Composite(components) = &glyph.outline else {
            panic!();
        };
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].glyph_id, 1);
        assert_eq!(components[1].glyph_id, 2);
        assert_eq!(components[1].offset, (5.0, 10.0));
    }

    #[test]
    fn composite_glyph_with_2x2_matrix() {
        // Полная 2×2 матрица: масштаб + поворот.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-1i16).to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        // flags = ARGS_XY | WE_HAVE_A_TWO_BY_TWO
        bytes.extend_from_slice(&0x0082u16.to_be_bytes());
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.push(0u8);
        bytes.push(0u8);
        // Matrix: 0.707, 0.707, -0.707, 0.707 (45° rotation, ish).
        // 0.707 × 16384 ≈ 11585
        bytes.extend_from_slice(&11585i16.to_be_bytes());
        bytes.extend_from_slice(&11585i16.to_be_bytes());
        bytes.extend_from_slice(&(-11585i16).to_be_bytes());
        bytes.extend_from_slice(&11585i16.to_be_bytes());

        let glyph = Glyph::parse(&bytes).unwrap();
        let Outline::Composite(components) = &glyph.outline else {
            panic!();
        };
        let t = components[0].transform;
        assert!((t[0] - 0.707).abs() < 0.001);
        assert!((t[1] - 0.707).abs() < 0.001);
        assert!((t[2] + 0.707).abs() < 0.001);
        assert!((t[3] - 0.707).abs() < 0.001);
    }

    #[test]
    fn zero_contours_yields_empty_outline() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0i16.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]); // bbox
        // 0 contours: дальше идут только instructionLength=0 + flags=[]
        bytes.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
        let glyph = Glyph::parse(&bytes).unwrap();
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        assert!(contours.is_empty());
    }

    /// Глиф с двумя короткими (1-байтными) координатами и repeat-флагом.
    /// Точки: (5, 0), (10, 0), (10, 5). Все on-curve.
    #[test]
    fn short_coords_and_repeat_flag() {
        let mut out = Vec::new();
        out.extend_from_slice(&1i16.to_be_bytes()); // 1 contour
        out.extend_from_slice(&[0u8; 8]); // bbox (упрощённо)
        out.extend_from_slice(&2u16.to_be_bytes()); // endPts = [2]
        out.extend_from_slice(&0u16.to_be_bytes()); // instructionLength

        // Каждая точка: on-curve + x_short + x_same_or_positive (это значит «положительное короткое x»)
        //   = 0x01 | 0x02 | 0x10 = 0x13
        // и y_short + y_same_or_positive = 0x04 | 0x20 = 0x24
        // итого 0x37 для всех трёх — используем REPEAT (0x08).
        let f = 0x01 | 0x02 | 0x10 | 0x04 | 0x20 | 0x08; // 0x3F
        out.push(f);
        out.push(2); // repeat=2 → ещё 2 раза тот же флаг

        // x: 5, 5 (накопится 10), 0 (на месте)
        out.push(5);
        out.push(5);
        out.push(0);
        // y: 0, 0, 5
        out.push(0);
        out.push(0);
        out.push(5);

        let glyph = Glyph::parse(&out).unwrap();
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        assert_eq!(contours[0].points[0].x, 5);
        assert_eq!(contours[0].points[1].x, 10);
        assert_eq!(contours[0].points[2].x, 10);
        assert_eq!(contours[0].points[2].y, 5);
    }

    #[test]
    fn off_curve_points_preserved() {
        // Один контур, 2 точки: on-curve (0,0), off-curve (50, 50).
        let mut out = Vec::new();
        out.extend_from_slice(&1i16.to_be_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&1u16.to_be_bytes()); // endPts = [1]
        out.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
        out.push(0x01); // on-curve
        out.push(0x00); // off-curve
        out.extend_from_slice(&0i16.to_be_bytes()); // x delta
        out.extend_from_slice(&50i16.to_be_bytes());
        out.extend_from_slice(&0i16.to_be_bytes()); // y delta
        out.extend_from_slice(&50i16.to_be_bytes());

        let glyph = Glyph::parse(&out).unwrap();
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        assert!(contours[0].points[0].on_curve);
        assert!(!contours[0].points[1].on_curve);
    }

    #[test]
    fn glyf_view_returns_correct_slice() {
        // Положим в "glyf" один триангл с offset=4 и длиной = его размер.
        let g = triangle_glyph_bytes();
        let mut data = vec![0xAA, 0xBB, 0xCC, 0xDD]; // 4 байта мусора в начале
        data.extend_from_slice(&g);
        let glyf = Glyf::new(&data);
        let glyph = glyf.glyph_at(4, g.len() as u32).unwrap();
        assert_eq!(glyph.bbox.x_max, 100);
    }
}
