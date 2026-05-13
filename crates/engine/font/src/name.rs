//! `name` table — человекочитаемые имена шрифта (family, subfamily, copyright …).
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/name>.
//!
//! Нам нужно одно: family name для индексации системного font matcher-а.
//! Поэтому парсер минимальный: читаем все NameRecord, выбираем лучший по
//! приоритету (Typographic Family > Family; Windows Unicode > MacRoman),
//! декодируем строку.
//!
//! Не поддерживаются: language tag records (version 1), platforms кроме
//! Windows / Mac / Unicode — реальные TTF/OTF из дикой природы используют
//! почти исключительно Windows Unicode для имён.

use crate::binary::BinaryReader;
use crate::face::FontError;

const NAME: [u8; 4] = *b"name";

/// Стандартные `nameID`-ы из spec §6.
mod name_id {
    pub const FAMILY: u16 = 1;
    pub const SUBFAMILY: u16 = 2;
    pub const FULL: u16 = 4;
    pub const TYPOGRAPHIC_FAMILY: u16 = 16;
    pub const TYPOGRAPHIC_SUBFAMILY: u16 = 17;
}

/// Platform IDs из spec §3.
mod platform {
    pub const UNICODE: u16 = 0;
    pub const MACINTOSH: u16 = 1;
    pub const WINDOWS: u16 = 3;
}

/// Минимальный набор строк, нужных font matcher-у.
///
/// Поля `Option<String>`, потому что не все шрифты содержат все NameID-ы
/// (особенно `typographic_family` — он опциональный, появляется только
/// у шрифтов с >4 face-ами в семействе).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Name {
    /// `nameID = 1`. Базовый family name (например, "Inter").
    pub family: Option<String>,
    /// `nameID = 2`. "Regular", "Bold", "Italic", "Bold Italic".
    pub subfamily: Option<String>,
    /// `nameID = 4`. Полное имя ("Inter Regular").
    pub full: Option<String>,
    /// `nameID = 16`. Семейство, объединяющее >4 face-а (preferred над `family`).
    pub typographic_family: Option<String>,
    /// `nameID = 17`. Subfamily для typographic group ("ExtraBold", "Thin Italic").
    pub typographic_subfamily: Option<String>,
}

impl Name {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        // version: 0 или 1. Нам этого различия не нужно (мы игнорируем
        // langTagRecord-ы версии 1).
        let _version = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        let count = r.read_u16().ok_or(FontError::UnexpectedEof)? as usize;
        let storage_offset = r.read_u16().ok_or(FontError::UnexpectedEof)? as usize;

        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(NameRecord::read(&mut r)?);
        }

        let storage = data
            .get(storage_offset..)
            .ok_or(FontError::InvalidTable(NAME))?;

        Ok(Self {
            family: select(&records, storage, name_id::FAMILY),
            subfamily: select(&records, storage, name_id::SUBFAMILY),
            full: select(&records, storage, name_id::FULL),
            typographic_family: select(&records, storage, name_id::TYPOGRAPHIC_FAMILY),
            typographic_subfamily: select(&records, storage, name_id::TYPOGRAPHIC_SUBFAMILY),
        })
    }

    /// «Лучшее» family name: typographic, если есть, иначе обычный family.
    /// Соответствует тому, что показал бы пользователю системный font
    /// menu (CSS Fonts L4 §4.3 говорит matching по family name; typographic
    /// — preferred-форма).
    pub fn best_family(&self) -> Option<&str> {
        self.typographic_family
            .as_deref()
            .or(self.family.as_deref())
    }
}

#[derive(Debug, Clone, Copy)]
struct NameRecord {
    platform_id: u16,
    encoding_id: u16,
    language_id: u16,
    name_id: u16,
    length: u16,
    string_offset: u16,
}

impl NameRecord {
    fn read(r: &mut BinaryReader) -> Result<Self, FontError> {
        Ok(Self {
            platform_id: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            encoding_id: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            language_id: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            name_id: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            length: r.read_u16().ok_or(FontError::UnexpectedEof)?,
            string_offset: r.read_u16().ok_or(FontError::UnexpectedEof)?,
        })
    }

    /// Чем меньше rank — тем предпочтительнее запись. Приоритет:
    /// 1. Windows Unicode + English (US) — самый распространённый формат.
    /// 2. Windows Unicode + любой английский (en-*) или нейтральный.
    /// 3. Windows Unicode + любой другой язык.
    /// 4. Unicode platform (любая encoding) — встречается реже.
    /// 5. MacRoman English.
    /// 6. Всё остальное.
    fn rank(self) -> u32 {
        match (self.platform_id, self.encoding_id, self.language_id) {
            (platform::WINDOWS, 1, 0x0409) => 0,
            (platform::WINDOWS, 1, lang) if lang & 0xff == 0x09 => 1,
            (platform::WINDOWS, 1, _) => 2,
            (platform::WINDOWS, 10, 0x0409) => 3,
            (platform::WINDOWS, 10, _) => 4,
            (platform::UNICODE, _, _) => 5,
            (platform::MACINTOSH, 0, 0) => 6,
            _ => 100,
        }
    }

    /// Декодирует строку из storage по `(string_offset, length)`.
    fn decode(self, storage: &[u8]) -> Option<String> {
        let start = self.string_offset as usize;
        let end = start.checked_add(self.length as usize)?;
        let bytes = storage.get(start..end)?;
        match (self.platform_id, self.encoding_id) {
            // Windows Unicode BMP UCS-2 BE (encoding 1) или Unicode full UTF-16 BE (encoding 10).
            (platform::WINDOWS, 1) | (platform::WINDOWS, 10) => decode_utf16_be(bytes),
            // Generic Unicode platform — все encoding-и UTF-16 BE.
            (platform::UNICODE, _) => decode_utf16_be(bytes),
            // MacRoman — упрощённо как ASCII; не-ASCII становится '?'. В family
            // names это почти всегда чистый ASCII, и шрифт всё равно даёт
            // дубль через Windows Unicode записи.
            (platform::MACINTOSH, 0) => Some(decode_mac_roman_ascii(bytes)),
            _ => None,
        }
    }
}

/// Выбирает запись с минимальным rank среди тех, что имеют данный `name_id`,
/// и декодирует её. None — если такой записи нет ни в одной приемлемой кодировке.
fn select(records: &[NameRecord], storage: &[u8], name_id: u16) -> Option<String> {
    records
        .iter()
        .filter(|r| r.name_id == name_id)
        .min_by_key(|r| r.rank())
        .and_then(|r| r.decode(storage))
}

fn decode_utf16_be(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let code_units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    Some(String::from_utf16_lossy(&code_units))
}

/// MacRoman → String: ASCII (0..=0x7F) пропускаем как есть, остальное — '?'.
/// Полная MacRoman-таблица не нужна: дубликат всегда есть в Windows-записи.
fn decode_mac_roman_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { '?' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Конструктор синтетической `name` таблицы для тестов.
    struct NameBuilder {
        records: Vec<(u16, u16, u16, u16, Vec<u8>)>, // platform, encoding, language, nameID, raw bytes
    }

    impl NameBuilder {
        fn new() -> Self {
            Self {
                records: Vec::new(),
            }
        }

        fn windows_unicode(mut self, language: u16, name_id: u16, text: &str) -> Self {
            let mut bytes = Vec::new();
            for cu in text.encode_utf16() {
                bytes.extend_from_slice(&cu.to_be_bytes());
            }
            self.records.push((platform::WINDOWS, 1, language, name_id, bytes));
            self
        }

        fn mac_roman(mut self, name_id: u16, text: &str) -> Self {
            self.records
                .push((platform::MACINTOSH, 0, 0, name_id, text.as_bytes().to_vec()));
            self
        }

        fn build(self) -> Vec<u8> {
            let count = self.records.len();
            let header_len = 6;
            let record_len = 12;
            let storage_offset = header_len + record_len * count;

            let mut out = Vec::new();
            out.extend_from_slice(&0u16.to_be_bytes()); // version
            out.extend_from_slice(&(count as u16).to_be_bytes());
            out.extend_from_slice(&(storage_offset as u16).to_be_bytes());

            let mut string_offset: u16 = 0;
            let mut storage: Vec<u8> = Vec::new();
            for (platform, encoding, language, name_id, bytes) in &self.records {
                let length = bytes.len() as u16;
                out.extend_from_slice(&platform.to_be_bytes());
                out.extend_from_slice(&encoding.to_be_bytes());
                out.extend_from_slice(&language.to_be_bytes());
                out.extend_from_slice(&name_id.to_be_bytes());
                out.extend_from_slice(&length.to_be_bytes());
                out.extend_from_slice(&string_offset.to_be_bytes());
                storage.extend_from_slice(bytes);
                string_offset += length;
            }
            out.extend_from_slice(&storage);
            out
        }
    }

    #[test]
    fn parses_windows_unicode_family() {
        let data = NameBuilder::new()
            .windows_unicode(0x0409, name_id::FAMILY, "Inter")
            .windows_unicode(0x0409, name_id::SUBFAMILY, "Regular")
            .build();
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.family.as_deref(), Some("Inter"));
        assert_eq!(name.subfamily.as_deref(), Some("Regular"));
        assert_eq!(name.best_family(), Some("Inter"));
    }

    #[test]
    fn typographic_family_wins_over_family() {
        let data = NameBuilder::new()
            .windows_unicode(0x0409, name_id::FAMILY, "Inter Bold")
            .windows_unicode(0x0409, name_id::TYPOGRAPHIC_FAMILY, "Inter")
            .build();
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.best_family(), Some("Inter"));
    }

    #[test]
    fn prefers_english_windows_unicode_over_other_language() {
        let data = NameBuilder::new()
            .windows_unicode(0x040C, name_id::FAMILY, "Intèr") // French
            .windows_unicode(0x0409, name_id::FAMILY, "Inter") // US English — должен победить
            .windows_unicode(0x0407, name_id::FAMILY, "Anders") // German
            .build();
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.family.as_deref(), Some("Inter"));
    }

    #[test]
    fn windows_unicode_wins_over_mac_roman() {
        let data = NameBuilder::new()
            .mac_roman(name_id::FAMILY, "MacRomanName")
            .windows_unicode(0x0409, name_id::FAMILY, "WindowsName")
            .build();
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.family.as_deref(), Some("WindowsName"));
    }

    #[test]
    fn mac_roman_used_when_windows_missing() {
        let data = NameBuilder::new()
            .mac_roman(name_id::FAMILY, "MacOnly")
            .build();
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.family.as_deref(), Some("MacOnly"));
    }

    #[test]
    fn cyrillic_family_name() {
        let data = NameBuilder::new()
            .windows_unicode(0x0419, name_id::FAMILY, "ПТ Санс") // ru-RU
            .build();
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.family.as_deref(), Some("ПТ Санс"));
    }

    #[test]
    fn missing_name_table_fields_are_none() {
        let data = NameBuilder::new().build();
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.family, None);
        assert_eq!(name.best_family(), None);
    }

    #[test]
    fn truncated_table_rejected() {
        let data: Vec<u8> = vec![0u8; 4]; // меньше 6-байтового заголовка
        assert!(matches!(Name::parse(&data), Err(FontError::UnexpectedEof)));
    }

    #[test]
    fn truncated_record_rejected() {
        // version + count=1 + storageOffset, но record не дописан
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&18u16.to_be_bytes());
        data.extend_from_slice(&[0u8; 8]); // только половина record-а (нужно 12)
        assert!(matches!(Name::parse(&data), Err(FontError::UnexpectedEof)));
    }

    #[test]
    fn record_with_unsupported_platform_is_ignored() {
        // platform = 5 (нечто экзотическое) — должно быть проигнорировано
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_be_bytes()); // version
        data.extend_from_slice(&1u16.to_be_bytes()); // count
        data.extend_from_slice(&18u16.to_be_bytes()); // storageOffset
        // record
        data.extend_from_slice(&5u16.to_be_bytes()); // platform
        data.extend_from_slice(&0u16.to_be_bytes()); // encoding
        data.extend_from_slice(&0u16.to_be_bytes()); // language
        data.extend_from_slice(&name_id::FAMILY.to_be_bytes());
        data.extend_from_slice(&4u16.to_be_bytes()); // length
        data.extend_from_slice(&0u16.to_be_bytes()); // string_offset
        data.extend_from_slice(b"abcd");
        let name = Name::parse(&data).unwrap();
        assert_eq!(name.family, None);
    }
}
