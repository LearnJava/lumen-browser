//! Таблицы однобайтовых кодировок и таблица меток WHATWG Encoding Standard.
//!
//! `single_byte.rs` — байт 0x80..=0xFF → Unicode codepoint для всех
//! однобайтовых кодировок Encoding Standard; `labels.rs` — label → encoding.
//! Оба сгенерированы скриптом `crates/engine/encoding/gen_tables.py` из
//! `index-*.txt` / `encodings.json` спецификации; правки руками вносить
//! нельзя, только перегенерация.
//!
//! U+FFFD на позиции таблицы — неопределённый байт: при обычном декодировании
//! он так и декодируется в U+FFFD, в fatal-режиме `TextDecoder` это сигнал
//! ошибки (ни одна однобайтовая кодировка WHATWG не отображает байт на
//! U+FFFD легитимно).

mod labels;
mod single_byte;

pub(crate) use labels::label_to_encoding;
pub(crate) use single_byte::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win1251_uppercase_range_is_linear() {
        // 0xC0 (index 64) → 'А' (U+0410), и далее по алфавиту до 0xDF → 'Я'.
        for i in 0..32u8 {
            assert_eq!(WINDOWS_1251[(64 + i) as usize] as u32, 0x0410 + u32::from(i));
        }
    }

    #[test]
    fn win1251_lowercase_range_is_linear() {
        // 0xE0 (index 96) → 'а' (U+0430), и далее по алфавиту до 0xFF → 'я'.
        for i in 0..32u8 {
            assert_eq!(WINDOWS_1251[(96 + i) as usize] as u32, 0x0430 + u32::from(i));
        }
    }

    #[test]
    fn win1251_yo_pair() {
        // Ё и ё на «нестандартных» местах — отдельно проверяем.
        assert_eq!(WINDOWS_1251[(0xA8 - 0x80) as usize], 'Ё');
        assert_eq!(WINDOWS_1251[(0xB8 - 0x80) as usize], 'ё');
    }

    #[test]
    fn win1252_c1_and_latin1_ranges() {
        assert_eq!(WINDOWS_1252[0], '\u{20AC}');
        assert_eq!(WINDOWS_1252[(0x81 - 0x80) as usize], '\u{0081}');
        assert_eq!(WINDOWS_1252[(0x9F - 0x80) as usize], '\u{0178}');
        for b in 0xA0..=0xFFu32 {
            assert_eq!(WINDOWS_1252[(b - 0x80) as usize] as u32, b);
        }
    }

    #[test]
    fn koi8r_phonetic_layout() {
        // KOI8-R: 'а' = 0xC1 (а не 0xE0 как в cp1251/cp866).
        assert_eq!(KOI8_R[(0xC1 - 0x80) as usize], 'а');
        assert_eq!(KOI8_R[(0xD7 - 0x80) as usize], 'в');
        assert_eq!(KOI8_R[(0xE1 - 0x80) as usize], 'А');
    }

    #[test]
    fn cp866_uppercase_block_one() {
        // 0x80..=0x8F → А..П.
        for i in 0..16u8 {
            assert_eq!(IBM866[i as usize] as u32, 0x0410 + u32::from(i));
        }
    }

    #[test]
    fn every_table_has_128_entries_without_surrogates() {
        // Генератор кладёт U+FFFD на неопределённые позиции; суррогатов быть
        // не должно нигде — иначе декодер выдаст невалидный char.
        let tables: [(&str, &[char; 128]); 27] = [
            ("ibm866", &IBM866), ("iso-8859-2", &ISO_8859_2), ("iso-8859-3", &ISO_8859_3),
            ("iso-8859-4", &ISO_8859_4), ("iso-8859-5", &ISO_8859_5), ("iso-8859-6", &ISO_8859_6),
            ("iso-8859-7", &ISO_8859_7), ("iso-8859-8", &ISO_8859_8), ("iso-8859-10", &ISO_8859_10),
            ("iso-8859-13", &ISO_8859_13), ("iso-8859-14", &ISO_8859_14),
            ("iso-8859-15", &ISO_8859_15), ("iso-8859-16", &ISO_8859_16),
            ("koi8-r", &KOI8_R), ("koi8-u", &KOI8_U), ("macintosh", &MACINTOSH),
            ("windows-874", &WINDOWS_874), ("windows-1250", &WINDOWS_1250),
            ("windows-1251", &WINDOWS_1251), ("windows-1252", &WINDOWS_1252),
            ("windows-1253", &WINDOWS_1253), ("windows-1254", &WINDOWS_1254),
            ("windows-1255", &WINDOWS_1255), ("windows-1256", &WINDOWS_1256),
            ("windows-1257", &WINDOWS_1257), ("windows-1258", &WINDOWS_1258),
            ("x-mac-cyrillic", &X_MAC_CYRILLIC),
        ];
        for (name, table) in tables {
            assert_eq!(table.len(), 128, "{name}");
            for (i, &c) in table.iter().enumerate() {
                assert!(!(0xD800..=0xDFFF).contains(&(c as u32)), "{name}[{i}] surrogate");
            }
        }
    }

    #[test]
    fn undefined_positions_match_spec() {
        // Неопределённые позиции по Encoding Standard — ровно U+FFFD в таблице.
        assert_eq!(ISO_8859_6[(0xA2 - 0x80) as usize], '\u{FFFD}');
        assert_eq!(ISO_8859_8[(0xA1 - 0x80) as usize], '\u{FFFD}');
        assert_eq!(WINDOWS_874[(0xDB - 0x80) as usize], '\u{FFFD}');
        assert_eq!(WINDOWS_1255[(0xD9 - 0x80) as usize], '\u{FFFD}');
        // А 0x98 в windows-1251 — определён (C1-контроль U+0098).
        assert_eq!(WINDOWS_1251[(0x98 - 0x80) as usize], '\u{0098}');
    }

    #[test]
    fn koi8u_ukrainian_letters() {
        // Отличия KOI8-U от KOI8-R: украинские буквы вместо псевдографики
        // (строчные в A-половине, заглавные в B-половине — как ё/Ё в KOI8-R).
        assert_eq!(KOI8_U[(0xA4 - 0x80) as usize], 'є');
        assert_eq!(KOI8_U[(0xB4 - 0x80) as usize], 'Є');
        assert_eq!(KOI8_U[(0xA6 - 0x80) as usize], 'і');
        assert_eq!(KOI8_U[(0xB6 - 0x80) as usize], 'І');
        assert_eq!(KOI8_U[(0xA7 - 0x80) as usize], 'ї');
        assert_eq!(KOI8_U[(0xB7 - 0x80) as usize], 'Ї');
        assert_eq!(KOI8_U[(0xAD - 0x80) as usize], 'ґ');
        assert_eq!(KOI8_U[(0xBD - 0x80) as usize], 'Ґ');
    }

    #[test]
    fn iso8859_8i_shares_table_with_iso8859_8() {
        // ISO-8859-8-I — тот же индекс, что ISO-8859-8, отличается только
        // направлением текста (логическое vs визуальное), которое здесь
        // не реализуется.
        assert_eq!(ISO_8859_8[(0xE0 - 0x80) as usize], 'א');
    }

    #[test]
    fn macintosh_anchors() {
        assert_eq!(MACINTOSH[0], 'Ä');
        assert_eq!(MACINTOSH[(0xF0 - 0x80) as usize], '\u{F8FF}'); // Apple logo
    }
}
