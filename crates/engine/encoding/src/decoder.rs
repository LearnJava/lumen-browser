//! Декодеры: байты → `String` для выбранной кодировки.
//!
//! Для всех вариантов гарантия: вызов не паникует и всегда возвращает валидный
//! `String`. Битые UTF-8 sequence заменяются на U+FFFD (как `from_utf8_lossy`),
//! у однобайтовых нелегальных байтов нет — каждое значение определено в
//! таблице (U+FFFD только на 0x98 в cp1251).

use crate::Encoding;
use crate::tables::{CP866, KOI8_R, WIN1251};

/// Декодирует байты в строку. Алиас для [`decode_to_string`], короткий и
/// привычный по аналогии с `encoding_rs::Encoding::decode`.
#[must_use]
pub fn decode(encoding: Encoding, bytes: &[u8]) -> String {
    decode_to_string(encoding, bytes)
}

/// То же, что [`decode`], но с явным именем — для случаев, когда из
/// контекста не очевидно, что возвращается `String`.
#[must_use]
pub fn decode_to_string(encoding: Encoding, bytes: &[u8]) -> String {
    match encoding {
        Encoding::Utf8 => decode_utf8(bytes),
        Encoding::Windows1251 => decode_single_byte(bytes, &WIN1251),
        Encoding::Koi8R => decode_single_byte(bytes, &KOI8_R),
        Encoding::Cp866 => decode_single_byte(bytes, &CP866),
    }
}

fn decode_utf8(bytes: &[u8]) -> String {
    // BOM EF BB BF: если есть в начале — режем, остальное декодируем lossy.
    let trimmed = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(trimmed).into_owned()
}

fn decode_single_byte(bytes: &[u8], table: &[char; 128]) -> String {
    // Каждый байт превращается ровно в один char; в худшем случае char
    // занимает 3 байта в UTF-8 (вся кириллица помещается в 2 байта,
    // box-drawing и большинство пунктуации — в 3). Резервируем под 3×len.
    let mut out = String::with_capacity(bytes.len() * 3);
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else {
            out.push(table[(b - 0x80) as usize]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_identical_across_encodings() {
        let bytes = b"<html><body>hello</body></html>";
        for enc in [
            Encoding::Utf8,
            Encoding::Windows1251,
            Encoding::Koi8R,
            Encoding::Cp866,
        ] {
            assert_eq!(decode(enc, bytes), "<html><body>hello</body></html>");
        }
    }

    #[test]
    fn utf8_bom_is_stripped() {
        let bytes = b"\xEF\xBB\xBF\xD0\x9F\xD1\x80\xD0\xB8\xD0\xB2\xD0\xB5\xD1\x82";
        assert_eq!(decode(Encoding::Utf8, bytes), "Привет");
    }

    #[test]
    fn utf8_invalid_becomes_replacement() {
        // 0xFF — невалидный начальный байт UTF-8.
        let bytes = b"ab\xFFcd";
        let out = decode(Encoding::Utf8, bytes);
        assert!(out.contains('\u{FFFD}'));
        assert!(out.contains("ab"));
        assert!(out.contains("cd"));
    }

    #[test]
    fn win1251_decodes_privet() {
        // «Привет» в Windows-1251.
        let bytes = &[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
        assert_eq!(decode(Encoding::Windows1251, bytes), "Привет");
    }

    #[test]
    fn koi8r_decodes_privet() {
        // «Привет» в KOI8-R. Те же символы, другие байты.
        let bytes = &[0xF0, 0xD2, 0xC9, 0xD7, 0xC5, 0xD4];
        assert_eq!(decode(Encoding::Koi8R, bytes), "Привет");
    }

    #[test]
    fn cp866_decodes_privet() {
        let bytes = &[0x8F, 0xE0, 0xA8, 0xA2, 0xA5, 0xE2];
        assert_eq!(decode(Encoding::Cp866, bytes), "Привет");
    }

    #[test]
    fn win1251_decodes_mixed_ascii_and_cyrillic() {
        // "<p>привет</p>" в cp1251.
        let bytes = &[
            b'<', b'p', b'>', 0xEF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, b'<', b'/', b'p', b'>',
        ];
        assert_eq!(decode(Encoding::Windows1251, bytes), "<p>привет</p>");
    }

    #[test]
    fn win1251_yo() {
        // Ё и ё на «нестандартных» местах — отдельно проверяем.
        let bytes = &[0xA8, 0xB8];
        assert_eq!(decode(Encoding::Windows1251, bytes), "Ёё");
    }
}
