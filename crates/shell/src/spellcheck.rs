//! Spell-check integration (P3-spell slice 2): dictionary loading from the
//! portable `data/spell/` folder, word extraction, misspelled-range detection
//! and the red squiggly-underline overlay for form controls.
//!
//! Pure logic layer: text measurement is abstracted behind a closure so the
//! module is unit-testable without a font backend; painting produces plain
//! `DisplayCommand`s that the shell appends to the overlay display list.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use lumen_core::ext::SpellChecker;
use lumen_core::geom::Rect;
use lumen_core::spell::HunspellDictionary;
use lumen_layout::Color;
use lumen_paint::DisplayCommand;

use crate::adblock::browser_data_dir;

/// Папка с пользовательскими словарями: `<exe_dir>/data/spell`.
pub fn spell_data_dir() -> PathBuf {
    browser_data_dir().join("spell")
}

/// Комбинированный словарь нескольких локалей. Слово считается верным,
/// если оно верно хотя бы в одном из подключённых словарей.
#[derive(Debug, Default)]
pub struct MultiDictionary {
    dicts: Vec<HunspellDictionary>,
    locale: String,
}

impl MultiDictionary {
    /// Создаёт пустой набор словарей (спелл-чек отключён).
    pub fn empty() -> Self {
        Self {
            dicts: Vec::new(),
            locale: "null".to_string(),
        }
    }

    /// Проверяет, загружен ли хотя бы один словарь.
    pub fn is_empty(&self) -> bool {
        self.dicts.is_empty()
    }
}

impl SpellChecker for MultiDictionary {
    fn check(&self, word: &str) -> bool {
        if self.dicts.is_empty() {
            return true;
        }
        self.dicts.iter().any(|d| d.check(word))
    }

    fn suggest(&self, word: &str) -> Vec<String> {
        if self.check(word) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for d in &self.dicts {
            for s in d.suggest(word) {
                if seen.insert(s.clone()) {
                    out.push(s);
                    if out.len() >= 8 {
                        return out;
                    }
                }
            }
        }
        out
    }

    fn locale(&self) -> &str {
        &self.locale
    }
}

/// Константы для волнистой линии (перенесены из lumen-paint).
const WAVY_AMPLITUDE_FACTOR: f32 = 1.5;
const WAVY_WAVELENGTH_FACTOR: f32 = 4.0;

/// Рисует волнистое подчёркивание в `out`.
fn emit_wavy_line(out: &mut Vec<DisplayCommand>, x: f32, y: f32, width: f32, thickness: f32, color: Color) {
    let amplitude = thickness * WAVY_AMPLITUDE_FACTOR;
    let wavelength = thickness * WAVY_WAVELENGTH_FACTOR;
    let step = (thickness * 0.5).max(1.0);
    let cy = y + thickness * 0.5;
    let end = x + width;
    let mut cx = x;
    while cx < end {
        let w = step.min(end - cx);
        if w <= 0.0 {
            break;
        }
        let sample_x = cx + w * 0.5;
        let phase = (sample_x - x) / wavelength * std::f32::consts::TAU;
        let dy = phase.sin() * amplitude;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(cx, cy + dy - thickness * 0.5, w, thickness),
            color,
        });
        cx += step;
    }
}

/// Загружает все пары `<stem>.aff` + `<stem>.dic` из `dir`.
/// Пары сортируются по имени для детерминизма. Нечитаемые файлы или ошибки
/// парсинга пропускаются молча. Локаль результата — стемы через "+", при пустом
/// наборе — "null". Несуществующая директория даёт empty().
pub fn load_dictionaries(dir: &Path) -> MultiDictionary {
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(it) => it.filter_map(|e| e.ok()).collect(),
        Err(_) => return MultiDictionary::empty(),
    };
    entries.sort_by_key(|e| e.file_name());

    let mut dicts = Vec::new();
    let mut stems = Vec::new();

    for entry in entries {
        let path = entry.path();
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if path.extension().and_then(|e| e.to_str()) != Some("aff") {
            continue;
        }
        let dic_path = path.with_extension("dic");
        if !dic_path.exists() {
            continue;
        }
        let aff_bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let dic_bytes = match std::fs::read(&dic_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let aff = String::from_utf8_lossy(&aff_bytes);
        let dic = String::from_utf8_lossy(&dic_bytes);
        match HunspellDictionary::from_aff_dic(&aff, &dic, stem) {
            Ok(d) => {
                dicts.push(d);
                stems.push(stem.to_string());
            }
            Err(_) => continue,
        }
    }

    let locale = if stems.is_empty() {
        "null".to_string()
    } else {
        stems.join("+")
    };
    MultiDictionary { dicts, locale }
}

/// Извлекает байтовые диапазоны слов в `text`.
/// Токен — максимальная последовательность символов, где каждый символ
/// удовлетворяет `is_alphanumeric() || c == '\'' || c == '’' || c == '-'`.
/// Токены, содержащие цифры, пропускаются. Краевые `'`, `’`, `-` обрезаются.
/// Возвращает Vec<(start_byte, end_byte)>, валидные для `&text[s..e]`.
pub fn extract_words(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start: Option<usize> = None;

    for (idx, ch) in text.char_indices() {
        let is_word_char = ch.is_alphanumeric() || ch == '\'' || ch == '’' || ch == '-';
        if is_word_char {
            if start.is_none() {
                start = Some(idx);
            }
        } else if let Some(s) = start.take() {
            let token = &text[s..idx];
            if !token.chars().any(|c| c.is_ascii_digit()) {
                let trimmed = token.trim_matches(|c| c == '\'' || c == '’' || c == '-');
                if !trimmed.is_empty() {
                    let trimmed_start = s + (trimmed.as_ptr() as usize - token.as_ptr() as usize);
                    let trimmed_end = trimmed_start + trimmed.len();
                    ranges.push((trimmed_start, trimmed_end));
                }
            }
        }
    }

    if let Some(s) = start {
        let token = &text[s..];
        if !token.chars().any(|c| c.is_ascii_digit()) {
            let trimmed = token.trim_matches(|c| c == '\'' || c == '’' || c == '-');
            if !trimmed.is_empty() {
                let trimmed_start = s + (trimmed.as_ptr() as usize - token.as_ptr() as usize);
                let trimmed_end = trimmed_start + trimmed.len();
                ranges.push((trimmed_start, trimmed_end));
            }
        }
    }

    ranges
}

/// Возвращает диапазоны слов, для которых `checker.check` вернул `false`, при
/// этом слова, чей lowercase присутствует в `allow`, считаются верными
/// (пользовательский словарь + «Пропустить» на сессию). Ограничение: не более
/// 100 диапазонов.
pub fn misspelled_ranges_with(
    checker: &dyn SpellChecker,
    text: &str,
    allow: &HashSet<String>,
) -> Vec<(usize, usize)> {
    extract_words(text)
        .into_iter()
        .filter(|(s, e)| {
            let word = &text[*s..*e];
            !checker.check(word) && !allow.contains(&word.to_lowercase())
        })
        .take(100)
        .collect()
}

/// Находит байтовый диапазон слова в `text`, чья горизонтальная проекция
/// содержит `x` (пиксели от начала строки). `measure` — ширина подстроки в px.
/// Возвращает `None`, если под точкой нет слова.
pub fn word_at_x(text: &str, x: f32, measure: &dyn Fn(&str) -> f32) -> Option<(usize, usize)> {
    for (s, e) in extract_words(text) {
        let x0 = measure(&text[..s]);
        let x1 = measure(&text[..e]);
        if x >= x0 && x < x1 {
            return Some((s, e));
        }
    }
    None
}

/// Locates the byte range of a word from one rendered (wrapped) visual line
/// inside the field's full logical text (P3-spell slice 4).
///
/// A multi-line field (`<textarea>`, contenteditable) paints one `DrawText`
/// per wrapped visual line, so a word's byte offset inside that single line
/// is not its offset inside the field's full value — applying a correction
/// with the line-local offset would corrupt everything after the first line.
/// `prior_lines` are this field's rendered lines, in document order, that
/// precede the line containing the word; `line_text` is that line itself.
/// Each line is located inside `full_text` via a forward substring search
/// starting right after the previous line's match, so wrap-induced whitespace
/// differences don't shift the result as long as no line's text recurs
/// verbatim before its real position.
///
/// Returns `None` if a line can't be found (e.g. `full_text` is stale).
pub fn locate_line_word_in_full_text(
    full_text: &str,
    prior_lines: &[String],
    line_text: &str,
    word_start_in_line: usize,
    word_end_in_line: usize,
) -> Option<(usize, usize)> {
    let mut cursor = 0usize;
    for line in prior_lines {
        if line.is_empty() {
            continue;
        }
        let pos = full_text.get(cursor..)?.find(line.as_str())?;
        cursor += pos + line.len();
    }
    let line_start = if line_text.is_empty() {
        cursor
    } else {
        cursor + full_text.get(cursor..)?.find(line_text)?
    };
    Some((line_start + word_start_in_line, line_start + word_end_in_line))
}

/// Путь к пользовательскому словарю: `<exe_dir>/data/spell/user_words.txt`.
pub fn user_words_path() -> PathBuf {
    spell_data_dir().join("user_words.txt")
}

/// Загружает пользовательский словарь: по одному слову в строке, lowercase.
/// Пустые строки и ошибки чтения игнорируются (возвращается пустой набор).
pub fn load_user_words(path: &Path) -> HashSet<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => s
            .lines()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => HashSet::new(),
    }
}

/// Добавляет слово (lowercase) в файл пользовательского словаря, дописывая
/// строку в конец. Создаёт родительские папки и файл при необходимости.
pub fn add_user_word(path: &Path, word: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", word.to_lowercase())
}

/// Строит команды отрисовки волнистого подчёркивания для ошибочных диапазонов.
/// `measure` — замыкание, возвращающее ширину строки в пикселях.
/// Цвет волны: красный (221, 30, 30, 255). Толщина 1.0, y = text_y + font_size * 0.95.
pub fn build_spell_overlay(
    text: &str,
    text_x: f32,
    text_y: f32,
    font_size: f32,
    ranges: &[(usize, usize)],
    measure: &dyn Fn(&str) -> f32,
) -> Vec<DisplayCommand> {
    let mut out = Vec::new();
    if ranges.is_empty() {
        return out;
    }
    let wave_y = text_y + font_size * 0.95;
    let color = Color { r: 221, g: 30, b: 30, a: 255 };
    let thickness = 1.0;

    for &(s, e) in ranges {
        let prefix = &text[..s];
        let word = &text[s..e];
        let x0 = text_x + measure(prefix);
        let w = measure(word);
        if w > 0.0 {
            emit_wavy_line(&mut out, x0, wave_y, w, thickness, color);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = r#"
SET UTF-8
TRY esianrtolcdugmphbyfvkwz
SFX D Y 2
SFX D   0   ed   [^e]
SFX D   e   ed   e
PFX U Y 1
PFX U   0   un   .
"#;

    const DIC: &str = r#"
4
walk/D
smile/D
lock/DU
привет
"#;

    #[test]
    fn extract_words_hello_world() {
        let text = "hello world";
        let ranges = extract_words(text);
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].0..ranges[0].1], "hello");
        assert_eq!(&text[ranges[1].0..ranges[1].1], "world");
    }

    #[test]
    fn extract_words_dont_stop() {
        let text = "don't stop";
        let ranges = extract_words(text);
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].0..ranges[0].1], "don't");
        assert_eq!(&text[ranges[1].0..ranges[1].1], "stop");
    }

    #[test]
    fn extract_words_cyrillic_hyphen() {
        let text = "по-русски и ещё";
        let ranges = extract_words(text);
        assert_eq!(ranges.len(), 3);
        assert_eq!(&text[ranges[0].0..ranges[0].1], "по-русски");
        assert_eq!(&text[ranges[1].0..ranges[1].1], "и");
        assert_eq!(&text[ranges[2].0..ranges[2].1], "ещё");
    }

    #[test]
    fn extract_words_digits_and_trim() {
        let text = "abc123 x2y";
        let ranges = extract_words(text);
        assert!(ranges.is_empty(), "tokens with digits should be skipped");

        let text2 = "'quoted'";
        let ranges2 = extract_words(text2);
        assert_eq!(ranges2.len(), 1);
        assert_eq!(&text2[ranges2[0].0..ranges2[0].1], "quoted");
    }

    #[test]
    fn multi_dictionary_empty() {
        let md = MultiDictionary::empty();
        assert!(md.is_empty());
        assert_eq!(md.locale(), "null");
        assert!(md.check("anything"));
        assert!(md.suggest("anything").is_empty());
    }

    #[test]
    fn multi_dictionary_two_dicts() {
        let dict1 = HunspellDictionary::from_aff_dic(AFF, DIC, "en_US").unwrap();
        let aff2 = "TRY ab\n";
        let dic2 = "1\nпока";
        let dict2 = HunspellDictionary::from_aff_dic(aff2, dic2, "ru_RU").unwrap();

        let mut md = MultiDictionary::empty();
        md.dicts = vec![dict1, dict2];
        md.locale = "en_US+ru_RU".to_string();

        assert!(md.check("walked"));
        assert!(md.check("пока"));
        assert!(!md.check("qqqq"));
    }

    #[test]
    fn multi_dictionary_suggest() {
        let dict1 = HunspellDictionary::from_aff_dic(AFF, DIC, "en_US").unwrap();
        let mut md = MultiDictionary::empty();
        md.dicts = vec![dict1];
        md.locale = "en_US".to_string();

        let sugg = md.suggest("walkk");
        assert!(!sugg.is_empty());
        assert!(sugg.contains(&"walk".to_string()));
        assert_eq!(sugg.len(), sugg.iter().collect::<std::collections::HashSet<_>>().len());
        assert!(sugg.len() <= 8);
    }

    #[test]
    fn misspelled_ranges_basic() {
        let dict = HunspellDictionary::from_aff_dic(AFF, DIC, "en_US").unwrap();
        let text = "walk walkz привет превет";
        let ranges = misspelled_ranges_with(&dict, text, &HashSet::new());
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].0..ranges[0].1], "walkz");
        assert_eq!(&text[ranges[1].0..ranges[1].1], "превет");
    }

    #[test]
    fn build_spell_overlay_non_empty() {
        let dict = HunspellDictionary::from_aff_dic(AFF, DIC, "en_US").unwrap();
        let text = "walk walkz";
        let ranges = misspelled_ranges_with(&dict, text, &HashSet::new());
        let measure = |s: &str| s.chars().count() as f32 * 10.0;
        let cmds = build_spell_overlay(text, 10.0, 20.0, 16.0, &ranges, &measure);

        assert!(!cmds.is_empty());
        for cmd in &cmds {
            let DisplayCommand::FillRect { rect, color } = cmd else {
                panic!("ожидались только FillRect");
            };
            assert_eq!(color.r, 221);
            assert_eq!(color.g, 30);
            assert_eq!(color.b, 30);
            assert_eq!(color.a, 255);
            let expected_y = 20.0 + 16.0 * 0.95;
            assert!((rect.y - expected_y).abs() < 2.0);
        }
        let DisplayCommand::FillRect { rect: first_rect, .. } = &cmds[0] else {
            panic!("ожидался FillRect");
        };
        let first_x = first_rect.x;
        let expected_first_x = 10.0 + measure("walk ");
        assert!((first_x - expected_first_x).abs() < 0.01);
    }

    #[test]
    fn build_spell_overlay_empty_ranges() {
        let cmds = build_spell_overlay("hello", 0.0, 0.0, 16.0, &[], &|s| s.len() as f32 * 10.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn load_dictionaries_temp_dir() {
        let dir = std::env::temp_dir().join("lumen_spell_test_load_dicts");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("en_US.aff"), AFF).unwrap();
        std::fs::write(dir.join("en_US.dic"), DIC).unwrap();

        let md = load_dictionaries(&dir);
        assert!(!md.is_empty());
        assert_eq!(md.locale(), "en_US");
        assert!(md.check("walked"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dictionaries_nonexistent() {
        let dir = std::env::temp_dir().join("lumen_spell_test_nonexistent_12345");
        let md = load_dictionaries(&dir);
        assert!(md.is_empty());
        assert_eq!(md.locale(), "null");
    }

    #[test]
    fn misspelled_ranges_with_allow_set() {
        let dict = HunspellDictionary::from_aff_dic(AFF, DIC, "en_US").unwrap();
        let text = "walk walkz привет превет";
        // Without allow-set: walkz + превет are misspelled.
        assert_eq!(misspelled_ranges_with(&dict, text, &HashSet::new()).len(), 2);
        // Allow "превет" (case-insensitive) → only walkz remains.
        let mut allow = HashSet::new();
        allow.insert("превет".to_string());
        let ranges = misspelled_ranges_with(&dict, text, &allow);
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].0..ranges[0].1], "walkz");
    }

    #[test]
    fn word_at_x_finds_word() {
        let text = "walk walkz";
        let measure = |s: &str| s.chars().count() as f32 * 10.0;
        // "walk" spans x in [0, 40); "walkz" spans [50, 100).
        assert_eq!(word_at_x(text, 5.0, &measure), Some((0, 4)));
        assert_eq!(word_at_x(text, 60.0, &measure), Some((5, 10)));
        // Space between words → no word.
        assert_eq!(word_at_x(text, 45.0, &measure), None);
        // Past the end → no word.
        assert_eq!(word_at_x(text, 200.0, &measure), None);
    }

    #[test]
    fn locate_line_word_single_line() {
        // Single-line field: prior_lines is empty, line_text == full_text.
        let full = "hello wrold";
        let found = locate_line_word_in_full_text(full, &[], "hello wrold", 6, 11);
        assert_eq!(found, Some((6, 11)));
    }

    #[test]
    fn locate_line_word_multi_line() {
        // Wrapped textarea value: "one two\nthree wrold\nfour" rendered as
        // three lines (wrap drops the newlines from each visual line's text).
        // The word is on the second line; "one two" is the only prior line.
        let full = "one two\nthree wrold\nfour";
        let found = locate_line_word_in_full_text(full, &["one two".to_owned()], "three wrold", 6, 11);
        let (s, e) = found.unwrap();
        assert_eq!(&full[s..e], "wrold");
    }

    #[test]
    fn locate_line_word_missing_line_returns_none() {
        let full = "hello world";
        let found = locate_line_word_in_full_text(full, &[], "not present", 0, 3);
        assert!(found.is_none());
    }

    #[test]
    fn locate_line_word_repeated_line_text_uses_forward_cursor() {
        // "wrold" appears in two identical lines — the second line's word
        // must resolve to the *second* occurrence, not the first.
        let full = "say wrold\nsay wrold";
        let prior = vec!["say wrold".to_owned()];
        let found = locate_line_word_in_full_text(full, &prior, "say wrold", 4, 9);
        let (s, e) = found.unwrap();
        assert_eq!(&full[s..e], "wrold");
        assert!(s > 9, "must resolve to the second line's occurrence, not the first");
    }

    #[test]
    fn user_words_load_and_add() {
        let dir = std::env::temp_dir().join("lumen_spell_test_user_words");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("user_words.txt");

        assert!(load_user_words(&path).is_empty());

        add_user_word(&path, "Превед").unwrap();
        add_user_word(&path, "walkz").unwrap();
        let words = load_user_words(&path);
        assert!(words.contains("превед"), "stored lowercase");
        assert!(words.contains("walkz"));
        assert_eq!(words.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
