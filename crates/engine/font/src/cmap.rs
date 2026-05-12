//! `cmap` table — Unicode codepoint → glyph index.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/cmap>.
//!
//! Поддерживаются два формата subtable:
//!
//! - **Format 4** — сегментированный маппинг для BMP (U+0000..U+FFFF).
//!   Latin, Cyrillic, Greek и большинство современных скриптов.
//!
//! - **Format 12** — sequential groups для полного Unicode диапазона
//!   (U+0000..U+10FFFF). Эмодзи (SMP U+1F000+), математические символы,
//!   исторические письменности, CJK Extension-B и далее.
//!
//! При выборе subtable предпочитаем format 12 за полноту покрытия.

use crate::binary::BinaryReader;
use crate::face::FontError;

const CMAP: [u8; 4] = *b"cmap";

pub struct Cmap<'a> {
    subtable: CmapSubtable<'a>,
}

enum CmapSubtable<'a> {
    Format4(Format4<'a>),
    Format12(Format12<'a>),
}

impl<'a> Cmap<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let _version = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        let num_tables = r.read_u16().ok_or(FontError::UnexpectedEof)?;

        // Собираем кандидатов: (rank, offset). Меньший rank = лучше.
        // Предпочитаем записи с full-Unicode coverage (они ведут к format 12).
        let mut candidates: Vec<(u8, u32)> = Vec::new();
        for _ in 0..num_tables {
            let platform_id = r.read_u16().ok_or(FontError::UnexpectedEof)?;
            let encoding_id = r.read_u16().ok_or(FontError::UnexpectedEof)?;
            let offset = r.read_u32().ok_or(FontError::UnexpectedEof)?;
            let rank: u8 = match (platform_id, encoding_id) {
                (3, 10) => 0,         // Windows Unicode full → format 12
                (0, 6) => 0,          // Unicode full → format 12
                (0, 4) => 1,          // Unicode 2.0+ (BMP или full)
                (3, 1) => 2,          // Windows Unicode BMP → format 4
                (0, 3) => 3,          // Unicode 2.0 BMP
                (0, 0..=2) => 4,      // Unicode 1.0 / variation sequences
                _ => continue,        // Mac Roman, Symbol и др. — пропускаем
            };
            candidates.push((rank, offset));
        }

        if candidates.is_empty() {
            return Err(FontError::InvalidTable(CMAP));
        }

        // Сортируем по rank, пробуем каждого по убыванию предпочтения.
        candidates.sort_unstable_by_key(|&(rank, _)| rank);

        for (_, offset) in candidates {
            let Some(subtable_data) = data.get(offset as usize..) else {
                continue;
            };
            if subtable_data.len() < 2 {
                continue;
            }
            let format = u16::from_be_bytes([subtable_data[0], subtable_data[1]]);
            match format {
                12 => {
                    if let Ok(f12) = Format12::parse(subtable_data) {
                        return Ok(Self {
                            subtable: CmapSubtable::Format12(f12),
                        });
                    }
                }
                4 => {
                    if let Ok(f4) = Format4::parse(subtable_data) {
                        return Ok(Self {
                            subtable: CmapSubtable::Format4(f4),
                        });
                    }
                }
                _ => continue,
            }
        }

        Err(FontError::InvalidTable(CMAP))
    }

    /// Возвращает glyph index для codepoint, либо `None` если не отображён.
    /// `0` — это `.notdef` («тофу»), возвращаем как `Some(0)`.
    pub fn glyph_index(&self, codepoint: u32) -> Option<u16> {
        match &self.subtable {
            CmapSubtable::Format4(f4) => f4.glyph_index(codepoint),
            CmapSubtable::Format12(f12) => f12.glyph_index(codepoint),
        }
    }
}

// ── Format 12 ────────────────────────────────────────────────────────────────

/// Format 12: Segmented coverage (полный Unicode диапазон).
///
/// Subtable layout (после u16 format):
/// - reserved: u16
/// - length: u32
/// - language: u32
/// - numGroups: u32
/// - groups[numGroups]: SequentialMapGroup { startCharCode: u32, endCharCode: u32, startGlyphID: u32 }
struct Format12<'a> {
    num_groups: u32,
    /// Сырые байты массива SequentialMapGroup (12 байт на группу).
    groups_data: &'a [u8],
}

impl<'a> Format12<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let format = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        if format != 12 {
            return Err(FontError::InvalidTable(CMAP));
        }
        r.skip(2).ok_or(FontError::UnexpectedEof)?; // reserved
        let _length = r.read_u32().ok_or(FontError::UnexpectedEof)?;
        let _language = r.read_u32().ok_or(FontError::UnexpectedEof)?;
        let num_groups = r.read_u32().ok_or(FontError::UnexpectedEof)?;
        let groups_bytes = (num_groups as usize)
            .checked_mul(12)
            .ok_or(FontError::InvalidTable(CMAP))?;
        let groups_data = r.read_bytes(groups_bytes).ok_or(FontError::UnexpectedEof)?;
        Ok(Self {
            num_groups,
            groups_data,
        })
    }

    fn glyph_index(&self, codepoint: u32) -> Option<u16> {
        let n = self.num_groups as usize;

        // Бинарный поиск: находим последнюю группу, где startCharCode ≤ codepoint.
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let start = read_u32_at(self.groups_data, mid * 12)?;
            if start <= codepoint {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let i = lo - 1;
        let start = read_u32_at(self.groups_data, i * 12)?;
        let end = read_u32_at(self.groups_data, i * 12 + 4)?;
        let start_glyph = read_u32_at(self.groups_data, i * 12 + 8)?;

        if codepoint > end {
            return None;
        }
        let offset = codepoint - start;
        // OpenType glyph ID всегда ≤ 65535 — усечение безопасно.
        let glyph_id = start_glyph.wrapping_add(offset) as u16;
        Some(glyph_id)
    }
}

// ── Format 4 ─────────────────────────────────────────────────────────────────

struct Format4<'a> {
    seg_count: usize,
    end_code: &'a [u8],
    start_code: &'a [u8],
    id_delta: &'a [u8],
    id_range_offset: &'a [u8],
    /// Позиция начала `idRangeOffset[0]` относительно начала subtable.
    id_range_offset_pos: usize,
    /// Полные байты subtable — нужны для адресной арифметики в idRangeOffset.
    subtable_data: &'a [u8],
}

impl<'a> Format4<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let format = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        if format != 4 {
            return Err(FontError::InvalidTable(CMAP));
        }
        let _length = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        let _language = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        let seg_count_x2 = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        if seg_count_x2 == 0 || seg_count_x2 % 2 != 0 {
            return Err(FontError::InvalidTable(CMAP));
        }
        let seg_count = (seg_count_x2 / 2) as usize;
        r.skip(6).ok_or(FontError::UnexpectedEof)?; // searchRange, entrySelector, rangeShift

        let bpa = seg_count * 2; // bytes per array
        let end_code = r.read_bytes(bpa).ok_or(FontError::UnexpectedEof)?;
        r.skip(2).ok_or(FontError::UnexpectedEof)?; // reservedPad
        let start_code = r.read_bytes(bpa).ok_or(FontError::UnexpectedEof)?;
        let id_delta = r.read_bytes(bpa).ok_or(FontError::UnexpectedEof)?;
        let id_range_offset_pos = r.position();
        let id_range_offset = r.read_bytes(bpa).ok_or(FontError::UnexpectedEof)?;

        Ok(Self {
            seg_count,
            end_code,
            start_code,
            id_delta,
            id_range_offset,
            id_range_offset_pos,
            subtable_data: data,
        })
    }

    fn glyph_index(&self, codepoint: u32) -> Option<u16> {
        if codepoint > 0xFFFF {
            return None;
        }
        let cp = codepoint as u16;

        let mut found = None;
        for i in 0..self.seg_count {
            let end = read_u16_at(self.end_code, i)?;
            if end >= cp {
                found = Some(i);
                break;
            }
        }
        let i = found?;
        let start = read_u16_at(self.start_code, i)?;
        if start > cp {
            return None;
        }
        let delta = read_i16_at(self.id_delta, i)?;
        let range_offset = read_u16_at(self.id_range_offset, i)?;

        if range_offset == 0 {
            return Some(cp.wrapping_add(delta as u16));
        }

        let addr = self
            .id_range_offset_pos
            .checked_add(i * 2)?
            .checked_add(range_offset as usize)?
            .checked_add(2 * (cp - start) as usize)?;
        let bytes: [u8; 2] = self.subtable_data.get(addr..addr + 2)?.try_into().ok()?;
        let glyph = u16::from_be_bytes(bytes);
        if glyph == 0 {
            Some(0)
        } else {
            Some(glyph.wrapping_add(delta as u16))
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_u16_at(slice: &[u8], idx: usize) -> Option<u16> {
    let off = idx * 2;
    let bytes: [u8; 2] = slice.get(off..off + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_i16_at(slice: &[u8], idx: usize) -> Option<i16> {
    let off = idx * 2;
    let bytes: [u8; 2] = slice.get(off..off + 2)?.try_into().ok()?;
    Some(i16::from_be_bytes(bytes))
}

fn read_u32_at(slice: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = slice.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Builders ─────────────────────────────────────────────────────────────

    /// Синтетический cmap с одной subtable format 4 (Windows BMP).
    fn build_cmap_format4(segments: &[(u16, u16, i16, u16)], glyph_id_array: &[u16]) -> Vec<u8> {
        assert!(!segments.is_empty());
        let seg_count = segments.len() as u16;
        let seg_count_x2 = seg_count * 2;

        let mut subtable = Vec::new();
        subtable.extend_from_slice(&4u16.to_be_bytes()); // format
        subtable.extend_from_slice(&0u16.to_be_bytes()); // length (fix later)
        subtable.extend_from_slice(&0u16.to_be_bytes()); // language
        subtable.extend_from_slice(&seg_count_x2.to_be_bytes());
        subtable.extend_from_slice(&seg_count_x2.to_be_bytes()); // searchRange
        subtable.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
        subtable.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        for (_, end, _, _) in segments {
            subtable.extend_from_slice(&end.to_be_bytes());
        }
        subtable.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        for (start, _, _, _) in segments {
            subtable.extend_from_slice(&start.to_be_bytes());
        }
        for (_, _, delta, _) in segments {
            subtable.extend_from_slice(&delta.to_be_bytes());
        }
        for (_, _, _, range_off) in segments {
            subtable.extend_from_slice(&range_off.to_be_bytes());
        }
        for g in glyph_id_array {
            subtable.extend_from_slice(&g.to_be_bytes());
        }
        let length = subtable.len() as u16;
        subtable[2..4].copy_from_slice(&length.to_be_bytes());

        wrap_cmap(3, 1, &subtable) // platformID=3 (Windows), encodingID=1 (BMP)
    }

    /// Синтетический cmap с одной subtable format 12 (Windows Unicode full).
    fn build_cmap_format12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
        let num_groups = groups.len() as u32;
        let length = 16u32 + num_groups * 12;

        let mut subtable = Vec::new();
        subtable.extend_from_slice(&12u16.to_be_bytes()); // format
        subtable.extend_from_slice(&0u16.to_be_bytes());  // reserved
        subtable.extend_from_slice(&length.to_be_bytes());
        subtable.extend_from_slice(&0u32.to_be_bytes());  // language
        subtable.extend_from_slice(&num_groups.to_be_bytes());
        for &(start, end, glyph) in groups {
            subtable.extend_from_slice(&start.to_be_bytes());
            subtable.extend_from_slice(&end.to_be_bytes());
            subtable.extend_from_slice(&glyph.to_be_bytes());
        }

        wrap_cmap(3, 10, &subtable) // platformID=3 (Windows), encodingID=10 (full)
    }

    /// Оборачивает subtable в минимальный cmap-заголовок.
    fn wrap_cmap(platform_id: u16, encoding_id: u16, subtable: &[u8]) -> Vec<u8> {
        let offset = 4u32 + 8; // header (4 б) + один encoding record (8 б)
        let mut full = Vec::new();
        full.extend_from_slice(&0u16.to_be_bytes()); // cmap version
        full.extend_from_slice(&1u16.to_be_bytes()); // numTables
        full.extend_from_slice(&platform_id.to_be_bytes());
        full.extend_from_slice(&encoding_id.to_be_bytes());
        full.extend_from_slice(&offset.to_be_bytes());
        full.extend_from_slice(subtable);
        full
    }

    // ── Format 4 (существующие тесты) ────────────────────────────────────────

    #[test]
    fn latin_uppercase_via_delta() {
        let data = build_cmap_format4(
            &[
                (0x0041, 0x005A, -0x40, 0),
                (0xFFFF, 0xFFFF, 1, 0),
            ],
            &[],
        );
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(b'A' as u32), Some(1));
        assert_eq!(cmap.glyph_index(b'M' as u32), Some(13));
        assert_eq!(cmap.glyph_index(b'Z' as u32), Some(26));
    }

    #[test]
    fn cyrillic_block_via_delta() {
        let delta = 100i16 - 0x0410i16;
        let data = build_cmap_format4(
            &[
                (0x0410, 0x044F, delta, 0),
                (0xFFFF, 0xFFFF, 1, 0),
            ],
            &[],
        );
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index('А' as u32), Some(100));
        assert_eq!(cmap.glyph_index('я' as u32), Some(163));
    }

    #[test]
    fn unmapped_codepoint_returns_none() {
        let data = build_cmap_format4(
            &[
                (0x0041, 0x005A, -0x40, 0),
                (0xFFFF, 0xFFFF, 1, 0),
            ],
            &[],
        );
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index('0' as u32), None);
        assert_eq!(cmap.glyph_index('а' as u32), None);
    }

    #[test]
    fn format4_beyond_bmp_returns_none() {
        let data = build_cmap_format4(
            &[(0x0041, 0x005A, -0x40, 0), (0xFFFF, 0xFFFF, 1, 0)],
            &[],
        );
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(0x1F600), None); // 😀 — нет в BMP
    }

    #[test]
    fn id_range_offset_indirect_lookup() {
        let data = build_cmap_format4(
            &[
                (0x0041, 0x0043, 0, 4),
                (0xFFFF, 0xFFFF, 1, 0),
            ],
            &[200, 201, 202],
        );
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(b'A' as u32), Some(200));
        assert_eq!(cmap.glyph_index(b'B' as u32), Some(201));
        assert_eq!(cmap.glyph_index(b'C' as u32), Some(202));
    }

    #[test]
    fn prefers_windows_bmp_when_multiple_subtables() {
        let inner = build_cmap_format4(
            &[(0x0041, 0x005A, -0x40, 0), (0xFFFF, 0xFFFF, 1, 0)],
            &[],
        );
        let subtable_bytes = &inner[12..];

        let mut full = Vec::new();
        full.extend_from_slice(&0u16.to_be_bytes()); // version
        full.extend_from_slice(&2u16.to_be_bytes()); // numTables
        // Mac Roman — должен быть проигнорирован
        full.extend_from_slice(&1u16.to_be_bytes());
        full.extend_from_slice(&0u16.to_be_bytes());
        full.extend_from_slice(&999u32.to_be_bytes());
        // Windows BMP
        full.extend_from_slice(&3u16.to_be_bytes());
        full.extend_from_slice(&1u16.to_be_bytes());
        full.extend_from_slice(&20u32.to_be_bytes());
        full.extend_from_slice(subtable_bytes);

        let cmap = Cmap::parse(&full).unwrap();
        assert_eq!(cmap.glyph_index(b'A' as u32), Some(1));
    }

    // ── Format 12 (новые тесты) ───────────────────────────────────────────────

    #[test]
    fn format12_basic_mapping() {
        // Одна группа: U+0041..U+005A → glyphs 1..26
        let data = build_cmap_format12(&[(0x0041, 0x005A, 1)]);
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(b'A' as u32), Some(1));
        assert_eq!(cmap.glyph_index(b'Z' as u32), Some(26));
    }

    #[test]
    fn format12_emoji_glyph() {
        // 😀 U+1F600 → glyph 500
        let data = build_cmap_format12(&[(0x1F600, 0x1F64F, 500)]);
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(0x1F600), Some(500)); // 😀
        assert_eq!(cmap.glyph_index(0x1F601), Some(501)); // 😁
        assert_eq!(cmap.glyph_index(0x1F64F), Some(579)); // 🙏 (500 + 0x4F)
    }

    #[test]
    fn format12_group_boundary_values() {
        let data = build_cmap_format12(&[(0x1F300, 0x1F5FF, 100)]);
        let cmap = Cmap::parse(&data).unwrap();
        // Первый и последний в диапазоне
        assert_eq!(cmap.glyph_index(0x1F300), Some(100));
        assert_eq!(cmap.glyph_index(0x1F5FF), Some(100u16 + (0x1F5FF_u32 - 0x1F300_u32) as u16));
        // За пределами диапазона
        assert_eq!(cmap.glyph_index(0x1F2FF), None);
        assert_eq!(cmap.glyph_index(0x1F600), None);
    }

    #[test]
    fn format12_multiple_groups_binary_search() {
        // Три несмежных группы (по возрастанию startCharCode).
        let data = build_cmap_format12(&[
            (0x0041, 0x005A, 1),   // A..Z
            (0x0410, 0x044F, 100), // А..я (Cyrillic)
            (0x1F600, 0x1F64F, 500), // Emoticons
        ]);
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(b'A' as u32), Some(1));
        assert_eq!(cmap.glyph_index('А' as u32), Some(100));
        assert_eq!(cmap.glyph_index(0x1F600), Some(500));
        // Пробелы между группами
        assert_eq!(cmap.glyph_index(0x0060), None);
        assert_eq!(cmap.glyph_index(0x0400), None);
        assert_eq!(cmap.glyph_index(0x1F5FF), None);
    }

    #[test]
    fn format12_gap_between_groups_returns_none() {
        let data = build_cmap_format12(&[
            (0x0041, 0x0045, 1), // A..E
            (0x0050, 0x0055, 10), // P..U
        ]);
        let cmap = Cmap::parse(&data).unwrap();
        // В пробеле 0x0046..0x004F
        assert_eq!(cmap.glyph_index(b'F' as u32), None);
        assert_eq!(cmap.glyph_index(b'O' as u32), None);
        // До первой группы
        assert_eq!(cmap.glyph_index(0x0040), None);
    }

    #[test]
    fn format12_preferred_over_format4_when_both_present() {
        // Строим cmap с двумя encoding records:
        // (3,1) → format 4 c A..Z → glyphs 1..26
        // (3,10) → format 12 с emoji → glyph 500 для U+1F600
        // Парсер должен предпочесть (3,10) format 12, у которого rank=0.
        // Проверяем: emoji доступно, значит выбран format 12.

        let mut f4_sub = Vec::new();
        {
            let segs: &[(u16, u16, i16, u16)] = &[
                (0x0041, 0x005A, -0x40, 0),
                (0xFFFF, 0xFFFF, 1, 0),
            ];
            let sc = segs.len() as u16;
            f4_sub.extend_from_slice(&4u16.to_be_bytes());
            f4_sub.extend_from_slice(&0u16.to_be_bytes());
            f4_sub.extend_from_slice(&0u16.to_be_bytes());
            f4_sub.extend_from_slice(&(sc * 2).to_be_bytes());
            f4_sub.extend_from_slice(&(sc * 2).to_be_bytes());
            f4_sub.extend_from_slice(&0u16.to_be_bytes());
            f4_sub.extend_from_slice(&0u16.to_be_bytes());
            for (_, end, _, _) in segs { f4_sub.extend_from_slice(&end.to_be_bytes()); }
            f4_sub.extend_from_slice(&0u16.to_be_bytes());
            for (start, _, _, _) in segs { f4_sub.extend_from_slice(&start.to_be_bytes()); }
            for (_, _, delta, _) in segs { f4_sub.extend_from_slice(&delta.to_be_bytes()); }
            for (_, _, _, ro) in segs { f4_sub.extend_from_slice(&ro.to_be_bytes()); }
            let len = f4_sub.len() as u16;
            f4_sub[2..4].copy_from_slice(&len.to_be_bytes());
        }

        let mut f12_sub = Vec::new();
        {
            let groups: &[(u32, u32, u32)] = &[(0x1F600, 0x1F64F, 500)];
            let ng = groups.len() as u32;
            let len = 16u32 + ng * 12;
            f12_sub.extend_from_slice(&12u16.to_be_bytes());
            f12_sub.extend_from_slice(&0u16.to_be_bytes());
            f12_sub.extend_from_slice(&len.to_be_bytes());
            f12_sub.extend_from_slice(&0u32.to_be_bytes());
            f12_sub.extend_from_slice(&ng.to_be_bytes());
            for &(s, e, g) in groups {
                f12_sub.extend_from_slice(&s.to_be_bytes());
                f12_sub.extend_from_slice(&e.to_be_bytes());
                f12_sub.extend_from_slice(&g.to_be_bytes());
            }
        }

        // cmap header: version(2) + numTables(2) + 2 records × 8 б = 20 б
        let offset_f12 = 20u32;
        let offset_f4 = offset_f12 + f12_sub.len() as u32;

        let mut full = Vec::new();
        full.extend_from_slice(&0u16.to_be_bytes()); // version
        full.extend_from_slice(&2u16.to_be_bytes()); // numTables
        // (3,1) → format 4
        full.extend_from_slice(&3u16.to_be_bytes());
        full.extend_from_slice(&1u16.to_be_bytes());
        full.extend_from_slice(&offset_f4.to_be_bytes());
        // (3,10) → format 12
        full.extend_from_slice(&3u16.to_be_bytes());
        full.extend_from_slice(&10u16.to_be_bytes());
        full.extend_from_slice(&offset_f12.to_be_bytes());
        full.extend_from_slice(&f12_sub);
        full.extend_from_slice(&f4_sub);

        let cmap = Cmap::parse(&full).unwrap();
        // Format 12 выбран — emoji доступно
        assert_eq!(cmap.glyph_index(0x1F600), Some(500));
    }

    #[test]
    fn format12_bmp_codepoint_works_too() {
        // Format 12 умеет маппить и обычные BMP codepoints
        let data = build_cmap_format12(&[(0x0041, 0x005A, 10)]);
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(b'A' as u32), Some(10));
        assert_eq!(cmap.glyph_index(b'Z' as u32), Some(35));
    }
}
