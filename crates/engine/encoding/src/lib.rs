//! Детектор и декодеры кодировок.
//!
//! Phase 0 покрывает то, что реально встречается в русскоязычном вебе и
//! в локальных файлах: UTF-8 (с BOM и без), UTF-16 LE/BE (BOM-driven —
//! файлы из «Сохранить как → Unicode» в блокноте Windows), Windows-1251,
//! KOI8-R, CP866. Эти шесть закрывают подавляющее большинство кириллических
//! документов; ISO-8859-5 и MacCyrillic настолько редки, что их добавим
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

pub use decoder::{decode, decode_to_string};
pub use detect::{detect, sniff_meta_charset};
pub use ext_impl::HeuristicDetector;
pub use hyphenation_impl::KnuthLiangHyphenation;
pub use unicode_provider::Icu4xUnicodeProvider;

/// Поддерживаемые в Phase 0 кодировки.
///
/// Для всех — стабильное имя в lowercase (то, что вернёт `name()`)
/// совпадает с WHATWG Encoding Standard label-ами.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// UTF-32 LE — четырёхбайтная кодировка, помеченная WHATWG как
    /// obsolete. Декодируем для backward compat (никогда не encode-им).
    Utf32Le,
    /// UTF-32 BE — то же что Utf32Le, но big-endian. Detect по BOM
    /// `00 00 FE FF`.
    Utf32Be,
    Windows1251,
    Koi8R,
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
            Self::Koi8R => "koi8-r",
            Self::Cp866 => "ibm866",
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
            "koi8-r" | "koi8r" | "koi8_r" | "koi" | "cskoi8r" => Some(Self::Koi8R),
            "ibm866" | "cp866" | "866" | "csibm866" => Some(Self::Cp866),
            _ => None,
        }
    }
}
