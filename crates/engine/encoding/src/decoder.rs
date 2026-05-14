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
        Encoding::Utf16Le => decode_utf16(bytes, /*little_endian=*/ true),
        Encoding::Utf16Be => decode_utf16(bytes, /*little_endian=*/ false),
        Encoding::Utf32Le => decode_utf32(bytes, /*little_endian=*/ true),
        Encoding::Utf32Be => decode_utf32(bytes, /*little_endian=*/ false),
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

/// Декодирует UTF-16 в little-endian или big-endian порядке.
///
/// Гарантии: не паникует, никогда не возвращает invalid UTF-8.
/// На ошибки (lone surrogate, нечётное число байт) подставляет U+FFFD.
/// BOM в начале (FF FE для LE, FE FF для BE) — снимается; если BOM
/// совпадает с указанным порядком, всё в порядке; если нет — снимается
/// тоже (так удобнее для тестов), но в реальной жизни такой mismatch
/// невозможен, потому что detector прислал бы Encoding под BOM.
fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    // Снять BOM любого варианта (LE: FF FE, BE: FE FF) — детектор уже
    // выбрал правильный Encoding по BOM-у, наша задача — пропустить байты.
    let bytes = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        &bytes[2..]
    } else {
        bytes
    };

    let read_u16 = |hi_lo: [u8; 2]| -> u16 {
        if little_endian {
            u16::from_le_bytes(hi_lo)
        } else {
            u16::from_be_bytes(hi_lo)
        }
    };

    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i + 2 <= bytes.len() {
        let u = read_u16([bytes[i], bytes[i + 1]]);
        i += 2;

        // High surrogate — нужен второй u16 в диапазоне 0xDC00..=0xDFFF.
        if (0xD800..=0xDBFF).contains(&u) {
            if i + 2 <= bytes.len() {
                let low = read_u16([bytes[i], bytes[i + 1]]);
                if (0xDC00..=0xDFFF).contains(&low) {
                    i += 2;
                    let code = 0x10000
                        + ((u32::from(u) - 0xD800) << 10)
                        + (u32::from(low) - 0xDC00);
                    // Surrogate-пара всегда даёт валидный code point.
                    out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    continue;
                }
            }
            // Одиночный high surrogate (или EOF после него) — replacement.
            out.push('\u{FFFD}');
            continue;
        }

        // Low surrogate без предшествующего high — invalid.
        if (0xDC00..=0xDFFF).contains(&u) {
            out.push('\u{FFFD}');
            continue;
        }

        // Обычный BMP code point.
        out.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}'));
    }

    // Нечётное число байт — последний полубайт не декодируется.
    if i < bytes.len() {
        out.push('\u{FFFD}');
    }

    out
}

/// Декодирует UTF-32 в LE или BE. По 4 байта на code point. BOM
/// (`FF FE 00 00` для LE, `00 00 FE FF` для BE) автоматически снимается
/// независимо от endian (defensive — detector обычно уже выбрал верный
/// вариант). Ошибки: code point вне Unicode (> U+10FFFF), surrogate
/// (U+D800..=U+DFFF), нецелое число байт — replacement U+FFFD.
fn decode_utf32(bytes: &[u8], little_endian: bool) -> String {
    let bytes = if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00])
        || bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
    {
        &bytes[4..]
    } else {
        bytes
    };

    let read_u32 = |b: [u8; 4]| -> u32 {
        if little_endian {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };

    // Капасити: каждый code point в UTF-8 — до 4 байт, в исходных
    // данных — 4 байта, так что строка не больше входа в UTF-8 байтах.
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let code = read_u32([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        i += 4;
        // Невалидные: > U+10FFFF или surrogates → U+FFFD.
        match char::from_u32(code) {
            Some(c) => out.push(c),
            None => out.push('\u{FFFD}'),
        }
    }
    // Остаток < 4 байт — невалидный хвост.
    if i < bytes.len() {
        out.push('\u{FFFD}');
    }
    out
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

    // --- UTF-16 ---

    #[test]
    fn utf16_le_ascii() {
        // "ABC" в UTF-16 LE без BOM: 41 00 42 00 43 00.
        let bytes = &[0x41, 0x00, 0x42, 0x00, 0x43, 0x00];
        assert_eq!(decode(Encoding::Utf16Le, bytes), "ABC");
    }

    #[test]
    fn utf16_be_ascii() {
        let bytes = &[0x00, 0x41, 0x00, 0x42, 0x00, 0x43];
        assert_eq!(decode(Encoding::Utf16Be, bytes), "ABC");
    }

    #[test]
    fn utf16_le_bom_stripped() {
        // FF FE = LE BOM. Затем "Hi".
        let bytes = &[0xFF, 0xFE, 0x48, 0x00, 0x69, 0x00];
        assert_eq!(decode(Encoding::Utf16Le, bytes), "Hi");
    }

    #[test]
    fn utf16_be_bom_stripped() {
        let bytes = &[0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69];
        assert_eq!(decode(Encoding::Utf16Be, bytes), "Hi");
    }

    #[test]
    fn utf16_le_cyrillic() {
        // «Привет»: П=041F р=0440 и=0438 в=0432 е=0435 т=0442.
        let bytes = &[
            0x1F, 0x04, 0x40, 0x04, 0x38, 0x04, 0x32, 0x04, 0x35, 0x04, 0x42, 0x04,
        ];
        assert_eq!(decode(Encoding::Utf16Le, bytes), "Привет");
    }

    #[test]
    fn utf16_be_cyrillic() {
        let bytes = &[
            0x04, 0x1F, 0x04, 0x40, 0x04, 0x38, 0x04, 0x32, 0x04, 0x35, 0x04, 0x42,
        ];
        assert_eq!(decode(Encoding::Utf16Be, bytes), "Привет");
    }

    #[test]
    fn utf16_le_supplementary_emoji() {
        // U+1F600 (😀) = surrogate pair: D83D DE00.
        // LE bytes: 3D D8 00 DE.
        let bytes = &[0x3D, 0xD8, 0x00, 0xDE];
        assert_eq!(decode(Encoding::Utf16Le, bytes), "\u{1F600}");
    }

    #[test]
    fn utf16_be_supplementary_emoji() {
        let bytes = &[0xD8, 0x3D, 0xDE, 0x00];
        assert_eq!(decode(Encoding::Utf16Be, bytes), "\u{1F600}");
    }

    #[test]
    fn utf16_lone_high_surrogate() {
        // High surrogate без low → U+FFFD.
        let bytes = &[0x3D, 0xD8, 0x41, 0x00]; // D83D, затем 0x0041 (A)
        let s = decode(Encoding::Utf16Le, bytes);
        assert!(s.contains('\u{FFFD}'));
        assert!(s.contains('A'));
    }

    #[test]
    fn utf16_lone_low_surrogate() {
        // Low surrogate без предшествующего high → U+FFFD.
        let bytes = &[0x00, 0xDE, 0x41, 0x00];
        let s = decode(Encoding::Utf16Le, bytes);
        assert!(s.contains('\u{FFFD}'));
        assert!(s.contains('A'));
    }

    #[test]
    fn utf16_odd_byte_count_emits_replacement() {
        // 3 байта — нечётно. Последний — лишний полубайт.
        let bytes = &[0x41, 0x00, 0x42];
        let s = decode(Encoding::Utf16Le, bytes);
        assert!(s.starts_with('A'));
        assert!(s.ends_with('\u{FFFD}'));
    }

    #[test]
    fn utf16_empty() {
        assert_eq!(decode(Encoding::Utf16Le, &[]), "");
        assert_eq!(decode(Encoding::Utf16Be, &[]), "");
    }

    #[test]
    fn utf16_le_bom_only() {
        // Только BOM, ничего после — пустая строка.
        assert_eq!(decode(Encoding::Utf16Le, &[0xFF, 0xFE]), "");
    }

    // ── UTF-32 ──

    #[test]
    fn utf32_le_ascii() {
        // 'A' = U+0041 → 41 00 00 00 в LE.
        let bytes = &[0x41, 0x00, 0x00, 0x00, 0x42, 0x00, 0x00, 0x00];
        assert_eq!(decode(Encoding::Utf32Le, bytes), "AB");
    }

    #[test]
    fn utf32_be_ascii() {
        // 'A' = U+0041 → 00 00 00 41 в BE.
        let bytes = &[0x00, 0x00, 0x00, 0x41, 0x00, 0x00, 0x00, 0x42];
        assert_eq!(decode(Encoding::Utf32Be, bytes), "AB");
    }

    #[test]
    fn utf32_le_bom_stripped() {
        // BOM `FF FE 00 00` + 'A' = U+0041.
        let bytes = &[0xFF, 0xFE, 0x00, 0x00, 0x41, 0x00, 0x00, 0x00];
        assert_eq!(decode(Encoding::Utf32Le, bytes), "A");
    }

    #[test]
    fn utf32_be_bom_stripped() {
        // BOM `00 00 FE FF` + 'A' = U+0041 в BE.
        let bytes = &[0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, 0x41];
        assert_eq!(decode(Encoding::Utf32Be, bytes), "A");
    }

    #[test]
    fn utf32_le_cyrillic() {
        // 'П' = U+041F.
        let bytes = &[0x1F, 0x04, 0x00, 0x00];
        assert_eq!(decode(Encoding::Utf32Le, bytes), "П");
    }

    #[test]
    fn utf32_supplementary_plane_emoji() {
        // 😀 = U+1F600. В LE — 00 F6 01 00.
        let bytes = &[0x00, 0xF6, 0x01, 0x00];
        assert_eq!(decode(Encoding::Utf32Le, bytes), "😀");
    }

    #[test]
    fn utf32_invalid_code_point() {
        // U+110000 — за пределами Unicode.
        let bytes = &[0x00, 0x00, 0x11, 0x00];
        let s = decode(Encoding::Utf32Le, bytes);
        assert_eq!(s, "\u{FFFD}");
    }

    #[test]
    fn utf32_surrogate_invalid() {
        // U+D800 — surrogate, не валидный code point в UTF-32.
        let bytes = &[0x00, 0xD8, 0x00, 0x00];
        let s = decode(Encoding::Utf32Le, bytes);
        assert_eq!(s, "\u{FFFD}");
    }

    #[test]
    fn utf32_trailing_partial_bytes() {
        // 'A' + 3 лишних байта.
        let bytes = &[0x41, 0x00, 0x00, 0x00, 0x42, 0x00, 0x00];
        let s = decode(Encoding::Utf32Le, bytes);
        assert_eq!(s, "A\u{FFFD}");
    }

    #[test]
    fn utf32_empty() {
        assert_eq!(decode(Encoding::Utf32Le, &[]), "");
        assert_eq!(decode(Encoding::Utf32Be, &[]), "");
    }
}
