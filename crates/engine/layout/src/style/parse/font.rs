//! Разбор шрифтовых значений: `font-family` (CSS Fonts L3 §5.2.2),
//! `font-variation-settings`/`font-feature-settings` (CSS Fonts L4 §6.4/§7.4),
//! `font-palette` (CSS Fonts L4 §14.1), относительный `font-weight`
//! (`lighter`/`bolder`), UA `ROOT_FONT_SIZE`/`DEFAULT_FONT_FAMILY`.
//!
//! Перенесено батчем SPLIT-ST11 из `crates/engine/layout/src/style.rs`
//! (анкер `pub fn parse_font_family`) без правок тел: изменены только
//! видимость `parse_font_weight`/`relative_lighter`/`relative_bolder`
//! (`pub(in crate::style)`, были `fn`) и пути импортов; восемь `pub` item-ов
//! остались `pub` — публичная поверхность крейта (`pub mod style` в
//! `lib.rs`, обращения из шести крейтов), реэкспорт со старого пути
//! обязателен (правило §2.1 очереди SPLIT).

use crate::style::{FontFeatureSetting, FontVariationSetting, FontWeight};

/// Парсит `font-family: a, "b c", d` в Vec<String>. Запятые разделяют
/// семейства; кавычки (одинарные или двойные) обрамляют имя с пробелами.
/// Имена без кавычек: один или несколько whitespace-разделённых
/// идентификаторов сливаются в одну строку с одним пробелом
/// (`Times New Roman` → `"Times New Roman"`). Пустые имена пропускаются.
pub fn parse_font_family(val: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = val.chars().peekable();
    while chars.peek().is_some() {
        // Пропускаем ведущий whitespace и запятые.
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else {
                break;
            }
        }
        let Some(&first) = chars.peek() else { break };
        let name = if first == '"' || first == '\'' {
            chars.next();
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == first { break; }
                s.push(c);
            }
            // Пропускаем до следующей запятой / EOF.
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                chars.next();
            }
            s
        } else {
            // Unquoted: собираем до запятой, схлопывая whitespace в один пробел.
            let mut s = String::new();
            let mut prev_space = false;
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                chars.next();
                if c.is_whitespace() {
                    if !s.is_empty() && !prev_space {
                        s.push(' ');
                        prev_space = true;
                    }
                } else {
                    s.push(c);
                    prev_space = false;
                }
            }
            // Trim trailing space.
            while s.ends_with(' ') {
                s.pop();
            }
            s
        };
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
}
/// Парсит CSS `font-variation-settings` (CSS Fonts L4 §7).
///
/// Синтаксис: `normal | [<string> <number>]#`
/// Пример: `"wght" 600, "wdth" 80`
///
/// Возвращает `None` при синтаксической ошибке (CSS cascading игнорирует
/// невалидные объявления). `normal` → `Some(Vec::new())`.
pub fn parse_font_variation_settings(val: &str) -> Option<Vec<FontVariationSetting>> {
    let val = val.trim();
    if val.eq_ignore_ascii_case("normal") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for token_pair in val.split(',') {
        let pair = token_pair.trim();
        if pair.is_empty() {
            continue;
        }
        // Первый токен — quoted 4-char tag
        let (tag_str, rest) = if let Some(stripped) = pair.strip_prefix('"') {
            let end = stripped.find('"')?;
            (&stripped[..end], stripped[end + 1..].trim())
        } else {
            let stripped = pair.strip_prefix('\'')?;
            let end = stripped.find('\'')?;
            (&stripped[..end], stripped[end + 1..].trim())
        };
        // Tag должен быть ровно 4 ASCII символа
        if tag_str.len() != 4 || !tag_str.is_ascii() {
            return None;
        }
        let tag_bytes = tag_str.as_bytes();
        let tag: [u8; 4] = [tag_bytes[0], tag_bytes[1], tag_bytes[2], tag_bytes[3]];
        // Следующий токен — число
        let value: f32 = rest.parse().ok()?;
        out.push(FontVariationSetting { tag, value });
    }
    Some(out)
}
/// Парсит CSS `font-feature-settings` (CSS Fonts L3 §6).
///
/// Синтаксис: `normal | <feature-tag-value>#`, где
/// `<feature-tag-value> = <string> [ <integer> | on | off ]?`.
/// Пример: `"liga" 0, "smcp", "salt" 2, "kern" off`.
///
/// Тег — ровно 4 символа ASCII U+20–U+7E; значение опущено → 1,
/// `on` → 1, `off` → 0, целое должно быть ≥ 0. Возвращает `None` при
/// синтаксической ошибке (cascade игнорирует невалидные объявления).
/// `normal` → `Some(Vec::new())`.
pub fn parse_font_feature_settings(val: &str) -> Option<Vec<FontFeatureSetting>> {
    let val = val.trim();
    if val.eq_ignore_ascii_case("normal") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for token_pair in val.split(',') {
        let pair = token_pair.trim();
        if pair.is_empty() {
            continue;
        }
        // Первый токен — quoted 4-char tag.
        let (tag_str, rest) = if let Some(stripped) = pair.strip_prefix('"') {
            let end = stripped.find('"')?;
            (&stripped[..end], stripped[end + 1..].trim())
        } else {
            let stripped = pair.strip_prefix('\'')?;
            let end = stripped.find('\'')?;
            (&stripped[..end], stripped[end + 1..].trim())
        };
        // Тег — ровно 4 печатных ASCII-символа (U+20–U+7E).
        if tag_str.len() != 4 || !tag_str.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return None;
        }
        let tag_bytes = tag_str.as_bytes();
        let tag: [u8; 4] = [tag_bytes[0], tag_bytes[1], tag_bytes[2], tag_bytes[3]];
        // Второй токен опционален: <integer ≥ 0> | on | off; по умолчанию 1.
        let value: u32 = if rest.is_empty() || rest.eq_ignore_ascii_case("on") {
            1
        } else if rest.eq_ignore_ascii_case("off") {
            0
        } else {
            rest.parse().ok()?
        };
        out.push(FontFeatureSetting { tag, value });
    }
    Some(out)
}
/// CSS Fonts L4 §11.3 — computed value of `font-palette`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FontPalette {
    /// Default CPAL palette (index 0). Initial value.
    #[default]
    Normal,
    /// First CPAL palette flagged «usable with light background».
    Light,
    /// First CPAL palette flagged «usable with dark background».
    Dark,
    /// `<dashed-ident>` naming a `@font-palette-values` rule (case-sensitive,
    /// stored with the leading `--`).
    Custom(String),
}
/// Парсит CSS `font-palette`: `normal | light | dark | <dashed-ident>`
/// (CSS Fonts L4 §11.3). Ключевые слова case-insensitive, dashed-ident
/// case-sensitive. Возвращает `None` при невалидном значении (cascade
/// игнорирует объявление).
pub fn parse_font_palette(val: &str) -> Option<FontPalette> {
    let v = val.trim();
    if v.eq_ignore_ascii_case("normal") {
        return Some(FontPalette::Normal);
    }
    if v.eq_ignore_ascii_case("light") {
        return Some(FontPalette::Light);
    }
    if v.eq_ignore_ascii_case("dark") {
        return Some(FontPalette::Dark);
    }
    if v.len() > 2 && v.starts_with("--") && !v.contains(char::is_whitespace) {
        return Some(FontPalette::Custom(v.to_string()));
    }
    None
}
/// Парсит CSS `font-weight`. Поддерживает:
///   - `normal` → 400, `bold` → 700;
///   - численные `100`..`900` (или любое число 1..1000 — Variable Fonts);
///   - относительные `lighter` / `bolder` — резолвятся относительно `parent`
///     по таблице из CSS Fonts L4 §2.4.3.
pub(in crate::style) fn parse_font_weight(val: &str, parent: FontWeight) -> Option<FontWeight> {
    match val.trim() {
        "normal" => Some(FontWeight::NORMAL),
        "bold" => Some(FontWeight::BOLD),
        "lighter" => Some(relative_lighter(parent)),
        "bolder" => Some(relative_bolder(parent)),
        s => s.parse::<u16>().ok().filter(|&n| (1..=1000).contains(&n)).map(FontWeight),
    }
}
/// CSS Fonts L4 §2.4.3 таблица для `lighter`. Сужаем weight в сторону normal.
pub(in crate::style) fn relative_lighter(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        100..=349 => 100,
        350..=549 => 100,
        550..=749 => 400,
        _ => 700, // 750..=1000
    })
}
/// CSS Fonts L4 §2.4.3 таблица для `bolder`.
pub(in crate::style) fn relative_bolder(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        0..=349 => 400,
        350..=549 => 700,
        550..=749 => 900,
        _ => 900,
    })
}
/// Корневой font-size в CSS — 16px на момент Phase 0 (без `<html>`-стилей и
/// настроек пользователя). Используется как базис для `rem`.
pub const ROOT_FONT_SIZE: f32 = 16.0;
/// Дефолтное `font-family` документа (UA stylesheet, BUG-128).
///
/// HTML не задаёт конкретного значения — это «default font» настройки
/// браузера, и у Edge / Chrome / Firefox она равна `serif` (на Windows —
/// Times New Roman). Раньше корневой стиль нёс ПУСТОЙ список, а пустой
/// список в рендере (`Renderer::resolve_face_id`) зарезервирован за chrome
/// UI (bundled Golos Text, DS-4) — то есть страница без объявленного
/// `font-family` рисовалась шрифтом браузерного интерфейса.
///
/// Generic-имя резолвится в системный face на этапе рендера и измерения
/// (`FontProvider::pick_generic_face`, `GenericFaceSet`), поэтому здесь
/// хранится именно CSS-generic, а не конкретное имя семейства: платформенная
/// таблица кандидатов живёт в `lumen_core::ext::generic_family_candidates`.
///
/// Инвариант, на который опирается рендер: у контента `font_family` НИКОГДА
/// не пуст, пустой список бывает только у chrome-овых `DrawText`.
pub const DEFAULT_FONT_FAMILY: &str = "serif";
/// Дефолтный список `font-family` документа — см. [`DEFAULT_FONT_FAMILY`].
#[must_use]
pub fn default_font_family() -> Vec<String> {
    vec![DEFAULT_FONT_FAMILY.to_string()]
}
