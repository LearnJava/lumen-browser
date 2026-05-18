//! Корневая структура шрифта: разбор offset table и таблиц-каталога.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff>.

use std::fmt;

use crate::binary::BinaryReader;

/// Заголовок TTF/OTF файла. Указывает, сколько таблиц в шрифте.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffsetTable {
    pub sfnt_version: u32,
    pub num_tables: u16,
    pub search_range: u16,
    pub entry_selector: u16,
    pub range_shift: u16,
}

impl OffsetTable {
    /// `0x00010000` — TrueType outlines.
    pub const SFNT_TRUETYPE: u32 = 0x00010000;
    /// `'OTTO'` — CFF/PostScript outlines в OpenType.
    pub const SFNT_OPENTYPE: u32 = 0x4F54544F;
    /// `'true'` — старый формат TrueType на macOS.
    pub const SFNT_TRUE: u32 = 0x74727565;

    pub fn read(r: &mut BinaryReader) -> Result<Self, FontError> {
        Ok(Self {
            sfnt_version: r.read_u32().ok_or(FontError::UnexpectedEof)?,
            num_tables: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            search_range: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            entry_selector: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            range_shift: r.read_u16().ok_or(FontError::UnexpectedEof)?,
        })
    }
}

/// Запись в каталоге таблиц: где в файле лежит конкретная таблица.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableRecord {
    pub tag: [u8; 4],
    pub checksum: u32,
    pub offset: u32,
    pub length: u32,
}

impl TableRecord {
    pub fn read(r: &mut BinaryReader) -> Result<Self, FontError> {
        Ok(Self {
            tag: r.read_tag().ok_or(FontError::UnexpectedEof)?,
            checksum: r.read_u32().ok_or(FontError::UnexpectedEof)?,
            offset: r.read_u32().ok_or(FontError::UnexpectedEof)?,
            length: r.read_u32().ok_or(FontError::UnexpectedEof)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    UnexpectedEof,
    InvalidSfntVersion(u32),
    TableOutOfBounds([u8; 4]),
    TableNotFound([u8; 4]),
    InvalidTable([u8; 4]),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => f.write_str("unexpected end of font data"),
            Self::InvalidSfntVersion(v) => write!(f, "invalid sfnt version: {v:#010x}"),
            Self::TableOutOfBounds(tag) => write!(f, "table {} out of bounds", tag_str(tag)),
            Self::TableNotFound(tag) => write!(f, "table {} not found", tag_str(tag)),
            Self::InvalidTable(tag) => write!(f, "table {} malformed", tag_str(tag)),
        }
    }
}

impl std::error::Error for FontError {}

fn tag_str(tag: &[u8; 4]) -> String {
    String::from_utf8_lossy(tag).into_owned()
}

/// Распарсенный шрифт: каталог таблиц + ссылка на оригинальные байты.
/// Сами таблицы (cmap, glyf, …) разбираются по запросу.
#[derive(Debug, Clone)]
pub struct Font<'a> {
    data: &'a [u8],
    offset_table: OffsetTable,
    tables: Vec<TableRecord>,
}

impl<'a> Font<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let offset_table = OffsetTable::read(&mut r)?;
        match offset_table.sfnt_version {
            OffsetTable::SFNT_TRUETYPE
            | OffsetTable::SFNT_OPENTYPE
            | OffsetTable::SFNT_TRUE => {}
            other => return Err(FontError::InvalidSfntVersion(other)),
        }
        let mut tables = Vec::with_capacity(offset_table.num_tables as usize);
        for _ in 0..offset_table.num_tables {
            tables.push(TableRecord::read(&mut r)?);
        }
        Ok(Self {
            data,
            offset_table,
            tables,
        })
    }

    pub fn offset_table(&self) -> &OffsetTable {
        &self.offset_table
    }

    pub fn tables(&self) -> &[TableRecord] {
        &self.tables
    }

    /// Возвращает байты таблицы по 4-байтовому тегу, либо `None`,
    /// если таблицы нет / она выходит за границы файла.
    pub fn table(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        let rec = self.tables.iter().find(|t| &t.tag == tag)?;
        let start = rec.offset as usize;
        let end = start.checked_add(rec.length as usize)?;
        self.data.get(start..end)
    }

    pub fn head(&self) -> Result<crate::head::Head, FontError> {
        let data = self.table(b"head").ok_or(FontError::TableNotFound(*b"head"))?;
        crate::head::Head::parse(data)
    }

    pub fn maxp(&self) -> Result<crate::maxp::Maxp, FontError> {
        let data = self.table(b"maxp").ok_or(FontError::TableNotFound(*b"maxp"))?;
        crate::maxp::Maxp::parse(data)
    }

    pub fn cmap(&self) -> Result<crate::cmap::Cmap<'a>, FontError> {
        let data = self.table(b"cmap").ok_or(FontError::TableNotFound(*b"cmap"))?;
        crate::cmap::Cmap::parse(data)
    }

    pub fn hhea(&self) -> Result<crate::hhea::Hhea, FontError> {
        let data = self.table(b"hhea").ok_or(FontError::TableNotFound(*b"hhea"))?;
        crate::hhea::Hhea::parse(data)
    }

    pub fn hmtx(&self) -> Result<crate::hmtx::Hmtx<'a>, FontError> {
        let hhea = self.hhea()?;
        let maxp = self.maxp()?;
        let data = self.table(b"hmtx").ok_or(FontError::TableNotFound(*b"hmtx"))?;
        crate::hmtx::Hmtx::parse(data, hhea.number_of_h_metrics, maxp.num_glyphs)
    }

    pub fn loca(&self) -> Result<crate::loca::Loca<'a>, FontError> {
        let head = self.head()?;
        let maxp = self.maxp()?;
        let data = self.table(b"loca").ok_or(FontError::TableNotFound(*b"loca"))?;
        crate::loca::Loca::parse(data, head.index_to_loc_format, maxp.num_glyphs)
    }

    pub fn glyf(&self) -> Result<crate::glyf::Glyf<'a>, FontError> {
        let data = self.table(b"glyf").ok_or(FontError::TableNotFound(*b"glyf"))?;
        Ok(crate::glyf::Glyf::new(data))
    }

    pub fn name(&self) -> Result<crate::name::Name, FontError> {
        let data = self.table(b"name").ok_or(FontError::TableNotFound(*b"name"))?;
        crate::name::Name::parse(data)
    }

    pub fn os2(&self) -> Result<crate::os2::Os2, FontError> {
        let data = self.table(b"OS/2").ok_or(FontError::TableNotFound(*b"OS/2"))?;
        crate::os2::Os2::parse(data)
    }

    /// `fvar` (Font Variations) — описание variation axes (wght / wdth / slnt /
    /// opsz / ital / custom). Возвращает `Err(TableNotFound)` для non-variable
    /// fonts (обычные `.ttf` / `.otf` без вариаций — каков и bundled Inter
    /// Regular). Phase 0 — парсятся только axis records, без instances
    /// (Variable Fonts L1 enabler).
    pub fn fvar(&self) -> Result<crate::fvar::Fvar, FontError> {
        let data = self.table(b"fvar").ok_or(FontError::TableNotFound(*b"fvar"))?;
        crate::fvar::Fvar::parse(data)
    }

    /// `avar` (Axis Variations) — piecewise-linear перенормализация осей из
    /// linear-normalized `[-1, 0, 1]` в spec-correct normalized для lookup в
    /// `gvar`. Опционально: variable font может не иметь `avar`, и тогда
    /// все оси трактуются как identity (`Avar::default()` тоже identity).
    /// Возвращает `Err(TableNotFound)`, если таблицы нет — caller обычно
    /// fallback на identity.
    pub fn avar(&self) -> Result<crate::avar::Avar, FontError> {
        let data = self.table(b"avar").ok_or(FontError::TableNotFound(*b"avar"))?;
        crate::avar::Avar::parse(data)
    }

    /// `HVAR` (Horizontal Metrics Variations) — variation deltas для
    /// advance width / LSB / RSB per glyph. При активном variation-
    /// instance шрифта runtime берёт base-метрики из `hmtx`, ищет
    /// (outer, inner)-индекс через `Hvar::advance_width_index(glyph_id)`,
    /// вычисляет delta через `ItemVariationStore` (когда `evaluate`
    /// будет реализован) и прибавляет к base. Опционально — variable
    /// font может не иметь HVAR, и тогда rasterizer использует `gvar`
    /// (дороже: реинтерполировать outline и пересчитать метрики
    /// вручную). Возвращает `Err(TableNotFound)` для не-VF и для VF
    /// без HVAR.
    pub fn hvar(&self) -> Result<crate::hvar::Hvar, FontError> {
        let data = self.table(b"HVAR").ok_or(FontError::TableNotFound(*b"HVAR"))?;
        crate::hvar::Hvar::parse(data)
    }

    /// `VVAR` (Vertical Metrics Variations) — зеркало `HVAR` для
    /// вертикальных метрик: advance height / TSB / BSB / vertical
    /// origin Y. Используется в шрифтах с поддержкой вертикального
    /// текста (CJK vertical, Mongolian). Возвращает
    /// `Err(TableNotFound)` для шрифтов без VVAR (большинство
    /// западных VF) и для не-VF.
    pub fn vvar(&self) -> Result<crate::vvar::Vvar, FontError> {
        let data = self.table(b"VVAR").ok_or(FontError::TableNotFound(*b"VVAR"))?;
        crate::vvar::Vvar::parse(data)
    }

    /// Удобная обёртка: glyph_id → outline. `None`, если глиф пустой
    /// (например, space). Composite-глифы возвращаются с `Outline::Composite`
    /// (компонентами) — для разрешения в простые контуры используй
    /// [`Font::glyph_resolved`].
    pub fn glyph(&self, glyph_id: u16) -> Result<Option<crate::glyf::Glyph>, FontError> {
        let loca = self.loca()?;
        let glyf = self.glyf()?;
        match loca.glyph_range(glyph_id) {
            None => Ok(None),
            Some((offset, length)) => Ok(Some(glyf.glyph_at(offset, length)?)),
        }
    }

    /// Возвращает глиф с рекурсивно развёрнутыми composite-компонентами:
    /// все ссылки на другие глифы заменены их трансформированными контурами,
    /// результат всегда `Outline::Simple`.
    ///
    /// Ограничение глубины — 8 уровней (защита от циклических ссылок в битых
    /// шрифтах). При превышении и при ссылке на отсутствующий глиф компонент
    /// тихо пропускается.
    pub fn glyph_resolved(
        &self,
        glyph_id: u16,
    ) -> Result<Option<crate::glyf::Glyph>, FontError> {
        self.glyph_resolved_depth(glyph_id, 0)
    }

    fn glyph_resolved_depth(
        &self,
        glyph_id: u16,
        depth: u32,
    ) -> Result<Option<crate::glyf::Glyph>, FontError> {
        const MAX_DEPTH: u32 = 8;
        if depth > MAX_DEPTH {
            return Ok(None);
        }

        let Some(glyph) = self.glyph(glyph_id)? else {
            return Ok(None);
        };
        let components = match glyph.outline {
            crate::glyf::Outline::Simple(_) => return Ok(Some(glyph)),
            crate::glyf::Outline::Composite(c) => c,
        };

        let mut merged: Vec<crate::glyf::Contour> = Vec::new();
        for comp in components {
            let Some(sub) = self.glyph_resolved_depth(comp.glyph_id, depth + 1)? else {
                continue;
            };
            let crate::glyf::Outline::Simple(sub_contours) = sub.outline else {
                continue; // не должно случаться после рекурсии, но защитимся
            };

            // Считаем XY-смещение компонента в координатах parent-а.
            // Anchor::Offset — берём напрямую. Anchor::Points — ищем
            // parent.point[parent_idx] в `merged` (уже-собранные контуры
            // от предыдущих компонент) и child.point[child_idx] в
            // `sub_contours` после применения transform-а; смещение =
            // parent_xy − transformed_child_xy.
            let (dx, dy) = match comp.anchor {
                crate::glyf::Anchor::Offset(dx, dy) => (dx, dy),
                crate::glyf::Anchor::Points { parent: pi, child: ci } => {
                    let parent_xy = nth_point_xy(&merged, pi as usize);
                    let transformed_child = nth_point_xy(&sub_contours, ci as usize)
                        .map(|(cx, cy)| {
                            let tx = comp.transform[0] * cx + comp.transform[2] * cy;
                            let ty = comp.transform[1] * cx + comp.transform[3] * cy;
                            (tx, ty)
                        });
                    match (parent_xy, transformed_child) {
                        (Some((px, py)), Some((tx, ty))) => (px - tx, py - ty),
                        // Если хотя бы одна точка не найдена (битый шрифт
                        // или out-of-range index) — fallback на (0, 0);
                        // компонент окажется в parent-origin. Визуально
                        // приемлемо для legacy edge-case (раньше офсет
                        // всегда был (0, 0)).
                        _ => (0.0, 0.0),
                    }
                }
            };

            for contour in sub_contours {
                let transformed = contour
                    .points
                    .into_iter()
                    .map(|p| {
                        let x = p.x as f32;
                        let y = p.y as f32;
                        // (x', y') = (a·x + c·y + dx, b·x + d·y + dy)
                        let nx = comp.transform[0] * x + comp.transform[2] * y + dx;
                        let ny = comp.transform[1] * x + comp.transform[3] * y + dy;
                        crate::glyf::OutlinePoint {
                            x: nx.round() as i16,
                            y: ny.round() as i16,
                            on_curve: p.on_curve,
                        }
                    })
                    .collect();
                merged.push(crate::glyf::Contour { points: transformed });
            }
        }

        Ok(Some(crate::glyf::Glyph {
            bbox: glyph.bbox,
            outline: crate::glyf::Outline::Simple(merged),
        }))
    }
}

/// Возвращает XY n-ой точки в линейном обходе всех контуров. Per
/// OpenType spec точки composite-глифа индексируются глобально по
/// всем контурам подряд. Используется для point-based выравнивания
/// компонент в `glyph_resolved_depth`.
fn nth_point_xy(contours: &[crate::glyf::Contour], idx: usize) -> Option<(f32, f32)> {
    let mut counter = 0usize;
    for contour in contours {
        if idx < counter + contour.points.len() {
            let p = contour.points[idx - counter];
            return Some((p.x as f32, p.y as f32));
        }
        counter += contour.points.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_offset_table(out: &mut Vec<u8>, num_tables: u16) {
        out.extend_from_slice(&OffsetTable::SFNT_TRUETYPE.to_be_bytes());
        out.extend_from_slice(&num_tables.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // search_range
        out.extend_from_slice(&0u16.to_be_bytes()); // entry_selector
        out.extend_from_slice(&0u16.to_be_bytes()); // range_shift
    }

    fn write_record(out: &mut Vec<u8>, tag: &[u8; 4], offset: u32, length: u32) {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&length.to_be_bytes());
    }

    #[test]
    fn parse_empty_font() {
        let mut bytes = Vec::new();
        write_offset_table(&mut bytes, 0);
        let font = Font::parse(&bytes).unwrap();
        assert_eq!(font.offset_table.num_tables, 0);
        assert!(font.tables.is_empty());
    }

    #[test]
    fn parse_with_two_tables() {
        let mut bytes = Vec::new();
        write_offset_table(&mut bytes, 2);
        write_record(&mut bytes, b"head", 100, 54);
        write_record(&mut bytes, b"glyf", 200, 50);

        let font = Font::parse(&bytes).unwrap();
        assert_eq!(font.tables.len(), 2);
        assert_eq!(&font.tables[0].tag, b"head");
        assert_eq!(font.tables[0].offset, 100);
        assert_eq!(&font.tables[1].tag, b"glyf");
    }

    #[test]
    fn table_lookup_returns_correct_slice() {
        let mut bytes = Vec::new();
        write_offset_table(&mut bytes, 1);
        // длина offset table = 12, длина одной записи = 16 → таблица начинается с 28.
        write_record(&mut bytes, b"data", 28, 4);
        bytes.extend_from_slice(b"hi!!");

        let font = Font::parse(&bytes).unwrap();
        assert_eq!(font.table(b"data"), Some(&b"hi!!"[..]));
        assert_eq!(font.table(b"nope"), None);
    }

    #[test]
    fn table_out_of_bounds_returns_none() {
        let mut bytes = Vec::new();
        write_offset_table(&mut bytes, 1);
        // offset за пределами файла
        write_record(&mut bytes, b"bad!", 9999, 4);
        let font = Font::parse(&bytes).unwrap();
        assert_eq!(font.table(b"bad!"), None);
    }

    #[test]
    fn invalid_sfnt_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xdeadbeefu32.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        match Font::parse(&bytes) {
            Err(FontError::InvalidSfntVersion(v)) => assert_eq!(v, 0xdeadbeef),
            other => panic!("expected InvalidSfntVersion, got {other:?}"),
        }
    }

    #[test]
    fn truncated_record_rejected() {
        let mut bytes = Vec::new();
        write_offset_table(&mut bytes, 1);
        // только половина записи (8 байт вместо 16)
        bytes.extend_from_slice(b"head");
        bytes.extend_from_slice(&0u32.to_be_bytes());
        assert!(matches!(Font::parse(&bytes), Err(FontError::UnexpectedEof)));
    }

    #[test]
    fn opentype_sfnt_accepted() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&OffsetTable::SFNT_OPENTYPE.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        let font = Font::parse(&bytes).unwrap();
        assert_eq!(font.offset_table.sfnt_version, OffsetTable::SFNT_OPENTYPE);
    }

    fn make_contour(points: &[(i16, i16)]) -> crate::glyf::Contour {
        crate::glyf::Contour {
            points: points
                .iter()
                .map(|&(x, y)| crate::glyf::OutlinePoint {
                    x,
                    y,
                    on_curve: true,
                })
                .collect(),
        }
    }

    #[test]
    fn nth_point_xy_first_contour() {
        // Один контур из 3 точек: индексы 0..2 возвращают эти точки.
        let contours = vec![make_contour(&[(10, 20), (30, 40), (50, 60)])];
        assert_eq!(super::nth_point_xy(&contours, 0), Some((10.0, 20.0)));
        assert_eq!(super::nth_point_xy(&contours, 1), Some((30.0, 40.0)));
        assert_eq!(super::nth_point_xy(&contours, 2), Some((50.0, 60.0)));
    }

    #[test]
    fn nth_point_xy_crosses_contour_boundary() {
        // Два контура — глобальный index продолжается во второй после
        // окончания первого. Per OpenType spec точки composite-глифа
        // нумеруются последовательно по всем контурам.
        let contours = vec![
            make_contour(&[(0, 0), (1, 1)]),
            make_contour(&[(2, 2), (3, 3)]),
        ];
        assert_eq!(super::nth_point_xy(&contours, 0), Some((0.0, 0.0)));
        assert_eq!(super::nth_point_xy(&contours, 1), Some((1.0, 1.0)));
        assert_eq!(super::nth_point_xy(&contours, 2), Some((2.0, 2.0)));
        assert_eq!(super::nth_point_xy(&contours, 3), Some((3.0, 3.0)));
    }

    #[test]
    fn nth_point_xy_out_of_range_returns_none() {
        let contours = vec![make_contour(&[(0, 0), (1, 1)])];
        assert_eq!(super::nth_point_xy(&contours, 5), None);
        assert_eq!(super::nth_point_xy(&[], 0), None);
    }
}
