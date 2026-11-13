//! Детектор и декодеры кодировок.
//!
//! Набор: Unicode-семейство (UTF-8 с BOM и без, UTF-16 LE/BE по BOM, UTF-32
//! LE/BE) и все однобайтовые кодировки WHATWG Encoding Standard с полной
//! таблицей меток §4.2 (ENC-SINGLEBYTE; раньше — Phase 0: UTF-8/16/32,
//! windows-1251, windows-1252, KOI8-R, IBM866).
//!
//! Алгоритм определения кодировки:
//! 1. BOM (UTF-8 / UTF-16) — самый надёжный сигнал, проверяем первым.
//! 2. HTML meta-sniff в первых ~1024 байтах: `<meta charset=...>` или
//!    `<meta http-equiv="Content-Type" content="...; charset=...">`.
//! 3. content_type_hint от транспорта (HTTP-заголовок, в Phase 0 — None).
//! 4. Если UTF-8 валиден целиком — UTF-8.
//! 5. Иначе частотная эвристика: декодируем «как бы» во все три
//!    однобайтовых варианта и выбираем тот, где доля кириллицы выше.
//!
//! Декодеры не падают на нелегальных байтах: для однобайтовых кодировок
//! «нелегальных» байтов нет (каждый кодпойнт определён, в крайнем случае
//! отображается на U+FFFD), для UTF-8 битый sequence заменяется на U+FFFD.

mod decoder;
mod detect;
mod ext_impl;
pub mod hyphenation_impl;
mod tables;
pub mod unicode_provider;

pub use decoder::{decode, decode_to_string, decode_to_string_opts};
pub use detect::{detect, detect_stylesheet_encoding, sniff_meta_charset};
pub use ext_impl::HeuristicDetector;
pub use hyphenation_impl::KnuthLiangHyphenation;
pub use unicode_provider::Icu4xUnicodeProvider;

/// Поддерживаемые кодировки: Unicode-семейство (UTF-8/16/32) и все
/// однобайтовые кодировки WHATWG Encoding Standard (ENC-SINGLEBYTE).
///
/// Для всех — стабильное имя в lowercase (то, что вернёт `name()`)
/// совпадает с WHATWG Encoding Standard label-ами.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// UTF-8 — кодировка по умолчанию и единственная, в которую мы кодируем.
    Utf8,
    /// UTF-16 little-endian; определяется по BOM `FF FE`.
    Utf16Le,
    /// UTF-16 big-endian; определяется по BOM `FE FF`.
    Utf16Be,
    /// UTF-32 LE — четырёхбайтная кодировка, помеченная WHATWG как
    /// obsolete. Декодируем для backward compat (никогда не encode-им).
    Utf32Le,
    /// UTF-32 BE — то же что Utf32Le, но big-endian. Detect по BOM
    /// `00 00 FE FF`.
    Utf32Be,
    /// IBM866 (CP866) — однобайтная кириллица DOS.
    Ibm866,
    /// ISO-8859-2 (Central European).
    Iso8859_2,
    /// ISO-8859-3 (South European).
    Iso8859_3,
    /// ISO-8859-4 (Baltic).
    Iso8859_4,
    /// ISO-8859-5 (Cyrillic).
    Iso8859_5,
    /// ISO-8859-6 (Arabic; большинство позиций не определены).
    Iso8859_6,
    /// ISO-8859-7 (Greek).
    Iso8859_7,
    /// ISO-8859-8 (Hebrew, визуальный вариант).
    Iso8859_8,
    /// ISO-8859-8-I (Hebrew, логический вариант; таблица общая с ISO-8859-8).
    Iso8859_8I,
    /// ISO-8859-10 (Nordic).
    Iso8859_10,
    /// ISO-8859-13 (Baltic).
    Iso8859_13,
    /// ISO-8859-14 (Celtic).
    Iso8859_14,
    /// ISO-8859-15 (Latin-9).
    Iso8859_15,
    /// ISO-8859-16 (South-Eastern European).
    Iso8859_16,
    /// KOI8-R — однобайтная кириллица (RFC 1489), встречается в старых архивах.
    Koi8R,
    /// KOI8-U — украинская вариация KOI8-R.
    Koi8U,
    /// macintosh (Mac Roman).
    Macintosh,
    /// windows-874 (Thai / TIS-620).
    Windows874,
    /// windows-1250 (Central European).
    Windows1250,
    /// windows-1251 — однобайтная кириллица, дефолт русскоязычного веба до UTF-8.
    Windows1251,
    /// windows-1252 — западноевропейская однобайтная; под неё WHATWG сводит метки
    /// `ascii`/`latin1`/`iso-8859-1` (BUG-1713).
    Windows1252,
    /// windows-1253 (Greek).
    Windows1253,
    /// windows-1254 (Turkish; таблица = ISO-8859-9).
    Windows1254,
    /// windows-1255 (Hebrew).
    Windows1255,
    /// windows-1256 (Arabic).
    Windows1256,
    /// windows-1257 (Baltic).
    Windows1257,
    /// windows-1258 (Vietnamese).
    Windows1258,
    /// x-mac-cyrillic (Mac Cyrillic).
    MacCyrillic,
}

impl Encoding {
    /// Стабильное имя кодировки. Используется в API детектора.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16Le => "utf-16le",
            Self::Utf16Be => "utf-16be",
            Self::Utf32Le => "utf-32le",
            Self::Utf32Be => "utf-32be",
            Self::Ibm866 => "ibm866",
            Self::Iso8859_2 => "iso-8859-2",
            Self::Iso8859_3 => "iso-8859-3",
            Self::Iso8859_4 => "iso-8859-4",
            Self::Iso8859_5 => "iso-8859-5",
            Self::Iso8859_6 => "iso-8859-6",
            Self::Iso8859_7 => "iso-8859-7",
            Self::Iso8859_8 => "iso-8859-8",
            Self::Iso8859_8I => "iso-8859-8-i",
            Self::Iso8859_10 => "iso-8859-10",
            Self::Iso8859_13 => "iso-8859-13",
            Self::Iso8859_14 => "iso-8859-14",
            Self::Iso8859_15 => "iso-8859-15",
            Self::Iso8859_16 => "iso-8859-16",
            Self::Koi8R => "koi8-r",
            Self::Koi8U => "koi8-u",
            Self::Macintosh => "macintosh",
            Self::Windows874 => "windows-874",
            Self::Windows1250 => "windows-1250",
            Self::Windows1251 => "windows-1251",
            Self::Windows1252 => "windows-1252",
            Self::Windows1253 => "windows-1253",
            Self::Windows1254 => "windows-1254",
            Self::Windows1255 => "windows-1255",
            Self::Windows1256 => "windows-1256",
            Self::Windows1257 => "windows-1257",
            Self::Windows1258 => "windows-1258",
            Self::MacCyrillic => "x-mac-cyrillic",
        }
    }

    /// Каноничное имя кодировки по таблице «Names and labels» WHATWG Encoding
    /// Standard — именно эта капитализация (`"UTF-8"`, `"IBM866"`, но
    /// `"windows-1251"` с маленькой w) ожидается в `document.characterSet`
    /// (DOM §7.3, BUG-358). Отличается от [`Encoding::name`], который всегда
    /// в lowercase — то, что хочет `TextDecoder.prototype.encoding`
    /// (Encoding Standard §4.1 get-encoding-name лоуеркейсит имя всегда).
    #[must_use]
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16LE",
            Self::Utf16Be => "UTF-16BE",
            Self::Utf32Le => "UTF-32LE",
            Self::Utf32Be => "UTF-32BE",
            Self::Ibm866 => "IBM866",
            Self::Iso8859_2 => "ISO-8859-2",
            Self::Iso8859_3 => "ISO-8859-3",
            Self::Iso8859_4 => "ISO-8859-4",
            Self::Iso8859_5 => "ISO-8859-5",
            Self::Iso8859_6 => "ISO-8859-6",
            Self::Iso8859_7 => "ISO-8859-7",
            Self::Iso8859_8 => "ISO-8859-8",
            Self::Iso8859_8I => "ISO-8859-8-I",
            Self::Iso8859_10 => "ISO-8859-10",
            Self::Iso8859_13 => "ISO-8859-13",
            Self::Iso8859_14 => "ISO-8859-14",
            Self::Iso8859_15 => "ISO-8859-15",
            Self::Iso8859_16 => "ISO-8859-16",
            Self::Koi8R => "KOI8-R",
            Self::Koi8U => "KOI8-U",
            Self::Macintosh => "macintosh",
            Self::Windows874 => "windows-874",
            Self::Windows1250 => "windows-1250",
            Self::Windows1251 => "windows-1251",
            Self::Windows1252 => "windows-1252",
            Self::Windows1253 => "windows-1253",
            Self::Windows1254 => "windows-1254",
            Self::Windows1255 => "windows-1255",
            Self::Windows1256 => "windows-1256",
            Self::Windows1257 => "windows-1257",
            Self::Windows1258 => "windows-1258",
            Self::MacCyrillic => "x-mac-cyrillic",
        }
    }

    /// Парсит label кодировки (case-insensitive, по полной таблице меток
    /// WHATWG Encoding Standard §4.2 — генерируется в `tables/labels.rs`).
    /// Нормализация: trim ASCII-пробелов по краям (BUG-1695 п. 1), удаление
    /// ASCII-пробелов внутри, lowercase. WHATWG-совместимо `utf-16` без
    /// суффикса мапится на **LE** (Microsoft-историческое значение), а не на
    /// BE — это решение пользователей в реальном вебе, не наша инициатива.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        let normalized: String = label
            .trim()
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        tables::label_to_encoding(&normalized)
    }

    /// Таблица старшей половины (0x80–0xFF) однобайтной кодировки —
    /// инверсия декодера для [`encode_byte`].
    fn single_byte_table(self) -> Option<&'static [char; 128]> {
        match self {
            Self::Ibm866 => Some(&tables::IBM866),
            Self::Iso8859_2 => Some(&tables::ISO_8859_2),
            Self::Iso8859_3 => Some(&tables::ISO_8859_3),
            Self::Iso8859_4 => Some(&tables::ISO_8859_4),
            Self::Iso8859_5 => Some(&tables::ISO_8859_5),
            Self::Iso8859_6 => Some(&tables::ISO_8859_6),
            Self::Iso8859_7 => Some(&tables::ISO_8859_7),
            Self::Iso8859_8 | Self::Iso8859_8I => Some(&tables::ISO_8859_8),
            Self::Iso8859_10 => Some(&tables::ISO_8859_10),
            Self::Iso8859_13 => Some(&tables::ISO_8859_13),
            Self::Iso8859_14 => Some(&tables::ISO_8859_14),
            Self::Iso8859_15 => Some(&tables::ISO_8859_15),
            Self::Iso8859_16 => Some(&tables::ISO_8859_16),
            Self::Koi8R => Some(&tables::KOI8_R),
            Self::Koi8U => Some(&tables::KOI8_U),
            Self::Macintosh => Some(&tables::MACINTOSH),
            Self::Windows874 => Some(&tables::WINDOWS_874),
            Self::Windows1250 => Some(&tables::WINDOWS_1250),
            Self::Windows1251 => Some(&tables::WINDOWS_1251),
            Self::Windows1252 => Some(&tables::WINDOWS_1252),
            Self::Windows1253 => Some(&tables::WINDOWS_1253),
            Self::Windows1254 => Some(&tables::WINDOWS_1254),
            Self::Windows1255 => Some(&tables::WINDOWS_1255),
            Self::Windows1256 => Some(&tables::WINDOWS_1256),
            Self::Windows1257 => Some(&tables::WINDOWS_1257),
            Self::Windows1258 => Some(&tables::WINDOWS_1258),
            Self::MacCyrillic => Some(&tables::X_MAC_CYRILLIC),
            _ => None,
        }
    }
}

/// Байт, в который `enc` кодирует кодпойнт `c`, — прямой для ASCII и первый
/// обратный индекс таблицы для старшей половины (WHATWG Encoding §4 «index of
/// a code point» берёт именно первый). `None` — кодпойнт в `enc` не
/// представим: плейсхолдеры U+FFFD неопределённых позиций не считаются
/// представимыми, чтобы U+FFFD всегда шёл по пути numeric character reference
/// (URL Standard §1.5). Энкодер существует только ради URL-query (BUG-1775):
/// TextEncoder по спецификации всегда UTF-8 и этой функцией не пользуется.
#[must_use]
pub fn encode_byte(enc: Encoding, c: char) -> Option<u8> {
    if (c as u32) < 0x80 {
        return Some(c as u8);
    }
    let table = enc.single_byte_table()?;
    table
        .iter()
        .position(|&t| t == c && c != '\u{FFFD}')
        .map(|i| i as u8 + 0x80)
}

/// HTML LS §2.1 «encoding parse a URL» + URL Standard §4.2 (query state):
/// не-ASCII кодпойнты query резолвимого URL кодируются кодировкой ДОКУМЕНТА,
/// а не всегда UTF-8. Кодпойнт, который кодировка не может представить,
/// записывается своим numeric character reference (`&#N;`), сам
/// процент-кодированным (URL §1.5 «percent-encode after encoding», шаг
/// encoder error) — так `ÿ` в документе windows-1251 даёт `%26%23255%3B`,
/// а U+FFFD в windows-1252 — `%26%2365533%3B` (BUG-1775).
///
/// Это пред-кодирование входной строки ДО url-парсера: готовые `%XX`
/// последовательности — ASCII — не трогаются (парсер не раскрывает их),
/// не-ASCII вне query (хост, путь, фрагмент) тоже — их кодирует сам парсер
/// как UTF-8, как требует стандарт. UTF-8/16/32 и так дают UTF-8 query
/// (HTML подставляет UTF-8 вместо UTF-16 для URL), поэтому возвращается
/// вход без изменений.
#[must_use]
pub fn encode_url_query(href: &str, enc: Encoding) -> String {
    if enc.single_byte_table().is_none() {
        return href.to_string();
    }
    // Query открывает первый '?', не лежащий внутри фрагмента (`page#f?x` —
    // '?' принадлежит фрагменту, кодируется парсером как UTF-8).
    let Some(q) = href.find('?') else {
        return href.to_string();
    };
    if href.find('#').is_some_and(|h| h < q) {
        return href.to_string();
    }
    let (head, rest) = href.split_at(q + 1); // head включает '?'
    let end = rest.find('#').unwrap_or(rest.len());
    let (query, tail) = rest.split_at(end);
    let mut out = String::with_capacity(href.len());
    out.push_str(head);
    for c in query.chars() {
        if c.is_ascii() {
            out.push(c);
        } else if let Some(b) = encode_byte(enc, c) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push_str(&format!("%26%23{}%3B", c as u32));
        }
    }
    out.push_str(tail);
    out
}

#[cfg(test)]
mod tests {
    use super::Encoding;
    use crate::tables::*;

    #[test]
    fn latin1_family_labels_resolve_to_windows_1252() {
        for label in [
            "ascii", "us-ascii", "ansi_x3.4-1968", "latin1", "l1", "iso-8859-1", "ISO8859-1",
            "iso88591", "iso_8859-1", "iso_8859-1:1987", "iso-ir-100", "csisolatin1", "cp819",
            "ibm819", "cp1252", "x-cp1252", "windows-1252", " Latin1 ",
        ] {
            assert_eq!(Encoding::from_label(label), Some(Encoding::Windows1252), "{label}");
        }
        assert_eq!(Encoding::Windows1252.name(), "windows-1252");
        assert_eq!(Encoding::Windows1252.canonical_name(), "windows-1252");
    }

    #[test]
    fn every_single_byte_encoding_decodes_by_index() {
        // Байты 0x80–0xFF → ровно скаляры таблицы, по индексу байт − 0x80
        // (ENC-SINGLEBYTE: юнит-проверка на каждую кодировку).
        let cases: [(&str, Encoding, &[char; 128]); 27] = [
            ("ibm866", Encoding::Ibm866, &IBM866),
            ("iso-8859-2", Encoding::Iso8859_2, &ISO_8859_2),
            ("iso-8859-3", Encoding::Iso8859_3, &ISO_8859_3),
            ("iso-8859-4", Encoding::Iso8859_4, &ISO_8859_4),
            ("iso-8859-5", Encoding::Iso8859_5, &ISO_8859_5),
            ("iso-8859-6", Encoding::Iso8859_6, &ISO_8859_6),
            ("iso-8859-7", Encoding::Iso8859_7, &ISO_8859_7),
            ("iso-8859-8", Encoding::Iso8859_8, &ISO_8859_8),
            ("iso-8859-8-i", Encoding::Iso8859_8I, &ISO_8859_8),
            ("iso-8859-10", Encoding::Iso8859_10, &ISO_8859_10),
            ("iso-8859-13", Encoding::Iso8859_13, &ISO_8859_13),
            ("iso-8859-14", Encoding::Iso8859_14, &ISO_8859_14),
            ("iso-8859-15", Encoding::Iso8859_15, &ISO_8859_15),
            ("iso-8859-16", Encoding::Iso8859_16, &ISO_8859_16),
            ("koi8-r", Encoding::Koi8R, &KOI8_R),
            ("koi8-u", Encoding::Koi8U, &KOI8_U),
            ("macintosh", Encoding::Macintosh, &MACINTOSH),
            ("windows-874", Encoding::Windows874, &WINDOWS_874),
            ("windows-1250", Encoding::Windows1250, &WINDOWS_1250),
            ("windows-1251", Encoding::Windows1251, &WINDOWS_1251),
            ("windows-1252", Encoding::Windows1252, &WINDOWS_1252),
            ("windows-1253", Encoding::Windows1253, &WINDOWS_1253),
            ("windows-1254", Encoding::Windows1254, &WINDOWS_1254),
            ("windows-1255", Encoding::Windows1255, &WINDOWS_1255),
            ("windows-1256", Encoding::Windows1256, &WINDOWS_1256),
            ("windows-1257", Encoding::Windows1257, &WINDOWS_1257),
            ("windows-1258", Encoding::Windows1258, &WINDOWS_1258),
        ];
        for (name, enc, table) in cases {
            let bytes: Vec<u8> = (0x80u8..=0xFF).collect();
            let expected: String = table.iter().collect();
            assert_eq!(super::decode(enc, &bytes), expected, "{name}");
        }
    }

    #[test]
    fn every_label_of_every_supported_encoding_resolves() {
        // Метка по имени кодировки всегда резолвится в ту же кодировку,
        // и имя/canonical_name согласованы (критерий «label→encoding целиком»).
        let all = [
            Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Utf32Le,
            Encoding::Utf32Be, Encoding::Ibm866, Encoding::Iso8859_2, Encoding::Iso8859_3,
            Encoding::Iso8859_4, Encoding::Iso8859_5, Encoding::Iso8859_6, Encoding::Iso8859_7,
            Encoding::Iso8859_8, Encoding::Iso8859_8I, Encoding::Iso8859_10,
            Encoding::Iso8859_13, Encoding::Iso8859_14, Encoding::Iso8859_15,
            Encoding::Iso8859_16, Encoding::Koi8R, Encoding::Koi8U, Encoding::Macintosh,
            Encoding::Windows874, Encoding::Windows1250, Encoding::Windows1251,
            Encoding::Windows1252, Encoding::Windows1253, Encoding::Windows1254,
            Encoding::Windows1255, Encoding::Windows1256, Encoding::Windows1257,
            Encoding::Windows1258, Encoding::MacCyrillic,
        ];
        for enc in all {
            assert_eq!(Encoding::from_label(enc.name()), Some(enc), "{}", enc.name());
            assert_eq!(
                Encoding::from_label(enc.canonical_name()),
                Some(enc),
                "{}",
                enc.canonical_name()
            );
        }
        // Метки вне набора — None, в том числе многобайтовые (ENC-CJK).
        for label in ["shift_jis", "gbk", "big5", "euc-kr", "replacement", "x-user-defined"] {
            assert_eq!(Encoding::from_label(label), None, "{label}");
        }
    }

    #[test]
    fn label_trims_ascii_whitespace() {
        // BUG-1695 п. 1: ASCII-пробелы по краям (и внутри) метки не мешают.
        assert_eq!(Encoding::from_label("  windows-1251\t"), Some(Encoding::Windows1251));
        assert_eq!(Encoding::from_label("iso 8859 5"), Some(Encoding::Iso8859_5));
    }

    #[test]
    fn encode_byte_uses_first_index_and_skips_fffd() {
        // Прямое соответствие ASCII и высоких позиций обеих 125x.
        assert_eq!(super::encode_byte(Encoding::Windows1252, 'a'), Some(b'a'));
        assert_eq!(super::encode_byte(Encoding::Windows1252, 'ÿ'), Some(0xFF));
        assert_eq!(super::encode_byte(Encoding::Windows1251, 'ё'), Some(0xB8));
        // ÿ нет в windows-1251 → numeric character reference.
        assert_eq!(super::encode_byte(Encoding::Windows1251, 'ÿ'), None);
        // U+FFFD не представим ни в одной таблице (плейсхолдер не считается).
        assert_eq!(super::encode_byte(Encoding::Windows1252, '\u{FFFD}'), None);
        // Многобайтные семейства не имеют однобайтного энкодера.
        assert_eq!(super::encode_byte(Encoding::Utf8, 'ÿ'), None);
    }

    #[test]
    fn url_query_encoding_matches_wpt_expectations() {
        use super::encode_url_query;
        // attributes.sub.html?encoding=windows-1252: `?ÿ` → `?%FF`.
        assert_eq!(encode_url_query("?ÿ", Encoding::Windows1252), "?%FF");
        // windows-1251 (x-cp1251 вариант): `ÿ` → numeric character reference.
        assert_eq!(encode_url_query("?ÿ", Encoding::Windows1251), "?%26%23255%3B");
        // lone-surrogates.sub.html: U+FFFD в windows-1252 → `&#65533;` в %-виде.
        assert_eq!(encode_url_query("?\u{FFFD}", Encoding::Windows1252), "?%26%2365533%3B");
        // UTF-8 документ — пред-кодирование не нужно, строку не трогаем.
        assert_eq!(encode_url_query("?ÿ", Encoding::Utf8), "?ÿ");
        // Готовые %-последовательности (ASCII) не перекодируются; не-query
        // части (путь до '?', фрагмент после '#') остаются на url-парсере.
        assert_eq!(
            encode_url_query("/п?%41ÿ#ÿ", Encoding::Windows1252),
            "/п?%41%FF#ÿ",
        );
        // Без '?' запроса нет — строка не меняется.
        assert_eq!(encode_url_query("/ÿ", Encoding::Windows1252), "/ÿ");
        // '?' внутри фрагмента не открывает query — фрагмент кодирует парсер.
        assert_eq!( encode_url_query("p#f?ÿ", Encoding::Windows1252), "p#f?ÿ");
    }
}
