//! Детектор и декодеры кодировок.
//!
//! Phase 0 покрывает то, что реально встречается в русскоязычном вебе и
//! в локальных файлах: UTF-8 (с BOM и без), UTF-16 LE/BE (BOM-driven —
//! файлы из «Сохранить как → Unicode» в блокноте Windows), Windows-1251,
//! KOI8-R, CP866 — эти шесть закрывают подавляющее большинство кириллических
//! документов, — плюс windows-1252 (метки `ascii`/`latin1`/`iso-8859-1`, BUG-1713).
//! ISO-8859-5 и MacCyrillic настолько редки, что их добавим
//! по факту встречи.
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

/// Поддерживаемые в Phase 0 кодировки.
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
    /// windows-1251 — однобайтная кириллица, дефолт русскоязычного веба до UTF-8.
    Windows1251,
    /// windows-1252 — западноевропейская однобайтная; под неё WHATWG сводит метки
    /// `ascii`/`latin1`/`iso-8859-1` (BUG-1713).
    Windows1252,
    /// KOI8-R — однобайтная кириллица (RFC 1489), встречается в старых архивах.
    Koi8R,
    /// CP866 — однобайтная кириллица DOS; встречается в текстовых вложениях.
    Cp866,
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
            Self::Windows1251 => "windows-1251",
            Self::Windows1252 => "windows-1252",
            Self::Koi8R => "koi8-r",
            Self::Cp866 => "ibm866",
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
            Self::Windows1251 => "windows-1251",
            Self::Windows1252 => "windows-1252",
            Self::Koi8R => "KOI8-R",
            Self::Cp866 => "IBM866",
        }
    }

    /// Парсит label кодировки (case-insensitive, с алиасами).
    ///
    /// Алиасы взяты из WHATWG Encoding Standard, оставлены только нужные
    /// для cyrillic-set + UTF-16. Заметим: WHATWG-совместимо `utf-16` без
    /// суффикса мапится на **LE** (Microsoft-историческое значение), а не на
    /// BE — это решение пользователей в реальном вебе, не наша инициатива.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        let normalized: String = label
            .trim()
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match normalized.as_str() {
            "utf-8" | "utf8" | "unicode-1-1-utf-8" => Some(Self::Utf8),
            "utf-16" | "utf-16le" | "utf16" | "utf16le" | "unicode" | "csunicode"
            | "iso-10646-ucs-2" | "ucs-2" => Some(Self::Utf16Le),
            "utf-16be" | "utf16be" => Some(Self::Utf16Be),
            "utf-32" | "utf32" | "utf-32le" | "utf32le" => Some(Self::Utf32Le),
            "utf-32be" | "utf32be" => Some(Self::Utf32Be),
            "windows-1251" | "cp1251" | "x-cp1251" | "windows1251" => Some(Self::Windows1251),
            "windows-1252" | "cp1252" | "x-cp1252" | "ascii" | "us-ascii" | "ansi_x3.4-1968"
            | "iso-8859-1" | "iso8859-1" | "iso88591" | "iso_8859-1" | "iso_8859-1:1987"
            | "iso-ir-100" | "latin1" | "l1" | "csisolatin1" | "cp819" | "ibm819" => Some(Self::Windows1252),
            "koi8-r" | "koi8r" | "koi8_r" | "koi" | "cskoi8r" => Some(Self::Koi8R),
            "ibm866" | "cp866" | "866" | "csibm866" => Some(Self::Cp866),
            _ => None,
        }
    }

    /// Таблица старшей половины (0x80–0xFF) однобайтной кодировки —
    /// инверсия декодера для [`encode_byte`].
    fn single_byte_table(self) -> Option<&'static [char; 128]> {
        match self {
            Self::Windows1251 => Some(&crate::tables::WIN1251),
            Self::Windows1252 => Some(&crate::tables::WIN1252),
            Self::Koi8R => Some(&crate::tables::KOI8_R),
            Self::Cp866 => Some(&crate::tables::CP866),
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
