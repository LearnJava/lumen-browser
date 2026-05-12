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
            for contour in sub_contours {
                let transformed = contour
                    .points
                    .into_iter()
                    .map(|p| {
                        let x = p.x as f32;
                        let y = p.y as f32;
                        // (x', y') = (a·x + c·y + dx, b·x + d·y + dy)
                        let nx = comp.transform[0] * x + comp.transform[2] * y + comp.offset.0;
                        let ny = comp.transform[1] * x + comp.transform[3] * y + comp.offset.1;
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
}
