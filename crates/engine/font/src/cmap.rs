//! `cmap` table — Unicode codepoint → glyph index.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/cmap>.
//!
//! Phase 0 — только subtable формата 4 (сегментированный маппинг для BMP,
//! U+0000..U+FFFF). Это покрывает Latin, Cyrillic, Greek, и большую часть
//! современных скриптов кроме эмодзи и редких символов вне BMP. Format 12
//! (для full Unicode SMP/SIP) подключим позже, когда понадобится emoji.

use crate::binary::BinaryReader;
use crate::face::FontError;

const CMAP: [u8; 4] = *b"cmap";

pub struct Cmap<'a> {
    subtable: Format4<'a>,
}

impl<'a> Cmap<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let _version = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        let num_tables = r.read_u16().ok_or(FontError::UnexpectedEof)?;

        // Выбираем «лучший» encoding record. Чем меньше rank, тем выше приоритет.
        let mut best_offset: Option<u32> = None;
        let mut best_rank = u8::MAX;
        for _ in 0..num_tables {
            let platform_id = r.read_u16().ok_or(FontError::UnexpectedEof)?;
            let encoding_id = r.read_u16().ok_or(FontError::UnexpectedEof)?;
            let offset = r.read_u32().ok_or(FontError::UnexpectedEof)?;
            // Только subtable-ы, которые имеют шанс быть форматом 4 (BMP Unicode).
            let rank = match (platform_id, encoding_id) {
                (3, 1) => 0,           // Windows Unicode BMP
                (0, 3) | (0, 4) => 1,  // Unicode 2.0+ BMP / full
                (0, 0..=2) => 2,       // Unicode 1.0 / variation
                _ => continue,         // Mac Roman, Symbol и др. — пропускаем
            };
            if rank < best_rank {
                best_rank = rank;
                best_offset = Some(offset);
            }
        }

        let offset = best_offset.ok_or(FontError::InvalidTable(CMAP))? as usize;
        let subtable_data = data.get(offset..).ok_or(FontError::InvalidTable(CMAP))?;
        let subtable = Format4::parse(subtable_data)?;
        Ok(Self { subtable })
    }

    /// Возвращает glyph index для codepoint, либо `None` если не отображён.
    /// `0` — это специальный glyph `.notdef` («тофу»), его возвращаем как `Some(0)`.
    pub fn glyph_index(&self, codepoint: u32) -> Option<u16> {
        self.subtable.glyph_index(codepoint)
    }
}

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

        // Линейный поиск сегмента с endCode >= cp. Сегменты гарантированно
        // отсортированы по endCode по спеке, так что binary search корректен,
        // но реальных сегментов в TTF мало (десятки), линейный достаточен.
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
            return None; // codepoint попал в gap между сегментами
        }
        let delta = read_i16_at(self.id_delta, i)?;
        let range_offset = read_u16_at(self.id_range_offset, i)?;

        if range_offset == 0 {
            // Прямой mapping: glyph = (cp + delta) mod 65536.
            // delta as u16 сохраняет битовый паттерн, wrapping_add даёт modulo.
            return Some(cp.wrapping_add(delta as u16));
        }

        // Косвенный mapping через glyphIdArray. Формула из спеки:
        //   glyphAddr = &idRangeOffset[i] + idRangeOffset[i] + 2 * (cp - startCode[i])
        let addr = self
            .id_range_offset_pos
            .checked_add(i * 2)?
            .checked_add(range_offset as usize)?
            .checked_add(2 * (cp - start) as usize)?;
        let bytes: [u8; 2] = self.subtable_data.get(addr..addr + 2)?.try_into().ok()?;
        let glyph = u16::from_be_bytes(bytes);
        if glyph == 0 {
            Some(0) // не отображён, но возвращаем 0 (notdef) согласно спеке
        } else {
            Some(glyph.wrapping_add(delta as u16))
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает синтетический cmap с одной subtable формата 4.
    /// segments: [(start, end, idDelta, idRangeOffset)], последний должен быть sentinel (0xFFFF).
    fn build_cmap_format4(segments: &[(u16, u16, i16, u16)], glyph_id_array: &[u16]) -> Vec<u8> {
        assert!(!segments.is_empty());
        let seg_count = segments.len() as u16;
        let seg_count_x2 = seg_count * 2;

        let mut subtable = Vec::new();
        subtable.extend_from_slice(&4u16.to_be_bytes()); // format
        subtable.extend_from_slice(&0u16.to_be_bytes()); // length (fix later)
        subtable.extend_from_slice(&0u16.to_be_bytes()); // language
        subtable.extend_from_slice(&seg_count_x2.to_be_bytes());
        subtable.extend_from_slice(&seg_count_x2.to_be_bytes()); // searchRange (приближение)
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

        let mut full = Vec::new();
        full.extend_from_slice(&0u16.to_be_bytes()); // cmap version
        full.extend_from_slice(&1u16.to_be_bytes()); // numTables
        full.extend_from_slice(&3u16.to_be_bytes()); // platformID (Windows)
        full.extend_from_slice(&1u16.to_be_bytes()); // encodingID (BMP)
        full.extend_from_slice(&12u32.to_be_bytes()); // offset (4 + 8 = 12)
        full.extend_from_slice(&subtable);
        full
    }

    #[test]
    fn latin_uppercase_via_delta() {
        // A..Z (0x41..0x5A) → glyphs 1..26 через idDelta = -0x40.
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
        // Cyrillic block U+0410..U+044F → glyphs 100..163 через idDelta.
        // 'А' = 0x0410, glyph 100 → delta = 100 - 0x0410 = 100 - 1040 = -940.
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
    fn beyond_bmp_returns_none() {
        let data = build_cmap_format4(
            &[(0x0041, 0x005A, -0x40, 0), (0xFFFF, 0xFFFF, 1, 0)],
            &[],
        );
        let cmap = Cmap::parse(&data).unwrap();
        assert_eq!(cmap.glyph_index(0x1F600), None); // 😀
    }

    #[test]
    fn id_range_offset_indirect_lookup() {
        // Один реальный сегмент A..C (0x41..0x43) + sentinel.
        // Через glyphIdArray, а не через delta.
        //
        // Layout idRangeOffset[2] (= 4 байта), затем glyphIdArray.
        // idRangeOffset[0] = 4 байта от начала idRangeOffset[0] →
        //   попадаем сразу за idRangeOffset[1] = начало glyphIdArray.
        // idRangeOffset[1] = 0 (sentinel: не используется).
        // glyphIdArray = [200, 201, 202] (A→200, B→201, C→202).
        //
        // delta = 0 для нашего сегмента → glyph = glyphIdArray[index].
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
        // Конструируем cmap с двумя записями: (1,0)=Mac Roman (skip)
        // и (3,1)=Windows BMP. Парсер должен выбрать (3,1).
        let inner = build_cmap_format4(
            &[(0x0041, 0x005A, -0x40, 0), (0xFFFF, 0xFFFF, 1, 0)],
            &[],
        );
        // inner начинается с 4 байт cmap header + 8 байт record — отбросим их.
        let subtable_bytes = &inner[12..];

        let mut full = Vec::new();
        full.extend_from_slice(&0u16.to_be_bytes()); // version
        full.extend_from_slice(&2u16.to_be_bytes()); // numTables
        // Mac Roman record (offset 999 = далеко за пределами файла, не должно
        // быть прочитано — проверка, что мы выбрали Windows и игнорировали Mac)
        full.extend_from_slice(&1u16.to_be_bytes()); // platformID
        full.extend_from_slice(&0u16.to_be_bytes()); // encodingID
        full.extend_from_slice(&999u32.to_be_bytes());
        // Windows BMP record: offset = 4 + 8 + 8 = 20
        full.extend_from_slice(&3u16.to_be_bytes());
        full.extend_from_slice(&1u16.to_be_bytes());
        full.extend_from_slice(&20u32.to_be_bytes());
        full.extend_from_slice(subtable_bytes);

        let cmap = Cmap::parse(&full).unwrap();
        assert_eq!(cmap.glyph_index(b'A' as u32), Some(1));
    }
}
