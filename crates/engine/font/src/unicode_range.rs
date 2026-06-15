//! CSS `unicode-range` descriptor parser — CSS Fonts L4 §5.
//!
//! Парсит значения вида `U+0000-007F, U+0400-04FF, U+26??` в список
//! `UnicodeRange`. Используется `MultiFontMeasurer` для фильтрации
//! @font-face face-ов при выборе шрифта для конкретного символа.

/// Один диапазон кодепоинтов из `unicode-range:` дескриптора @font-face.
///
/// Конечные точки включительны: `start..=end`. Одиночный кодепоинт
/// имеет `start == end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnicodeRange {
    /// Начало диапазона (включительно).
    pub start: u32,
    /// Конец диапазона (включительно).
    pub end: u32,
}

impl UnicodeRange {
    /// Проверяет, входит ли кодепоинт `cp` в этот диапазон.
    pub fn contains(self, cp: u32) -> bool {
        cp >= self.start && cp <= self.end
    }
}

/// Парсит CSS `unicode-range` дескриптор в список `UnicodeRange`.
///
/// Формат (CSS Fonts L4 §5.2):
/// - одиночный кодепоинт: `U+0041`
/// - диапазон: `U+0041-005A`
/// - wildcard: `U+26??` (каждый `?` заменяет `0`-`F`)
///
/// Токены разделяются запятыми. Неизвестные токены тихо пропускаются.
/// Если строка пуста или все токены невалидны — возвращает пустой Vec.
pub fn parse_unicode_ranges(s: &str) -> Vec<UnicodeRange> {
    let mut out = Vec::new();
    for token in s.split(',') {
        let token = token.trim();
        let Some(hex_part) = token.strip_prefix("U+").or_else(|| token.strip_prefix("u+")) else {
            continue;
        };

        if hex_part.contains('?') {
            // Wildcard: заменяем `?` на `0` для start и `F` для end.
            let start_str: String = hex_part.chars().map(|c| if c == '?' { '0' } else { c }).collect();
            let end_str: String = hex_part.chars().map(|c| if c == '?' { 'F' } else { c }).collect();
            let Ok(start) = u32::from_str_radix(&start_str, 16) else { continue };
            let Ok(end) = u32::from_str_radix(&end_str, 16) else { continue };
            if start <= end && end <= 0x10_FFFF {
                out.push(UnicodeRange { start, end });
            }
        } else if let Some((lo, hi)) = hex_part.split_once('-') {
            // Диапазон: U+0041-005A
            let Ok(start) = u32::from_str_radix(lo.trim(), 16) else { continue };
            let Ok(end) = u32::from_str_radix(hi.trim(), 16) else { continue };
            if start <= end && end <= 0x10_FFFF {
                out.push(UnicodeRange { start, end });
            }
        } else {
            // Одиночный кодепоинт: U+0041
            let Ok(cp) = u32::from_str_radix(hex_part.trim(), 16) else { continue };
            if cp <= 0x10_FFFF {
                out.push(UnicodeRange { start: cp, end: cp });
            }
        }
    }
    out
}

/// Проверяет, покрывается ли кодепоинт хотя бы одним диапазоном из списка.
///
/// Если список пуст — считается, что ограничений нет (нет `unicode-range`
/// дескриптора), и функция возвращает `true`.
pub fn codepoint_in_ranges(cp: u32, ranges: &[UnicodeRange]) -> bool {
    if ranges.is_empty() {
        return true;
    }
    ranges.iter().any(|r| r.contains(cp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_codepoint() {
        let r = parse_unicode_ranges("U+0041");
        assert_eq!(r, vec![UnicodeRange { start: 0x41, end: 0x41 }]);
    }

    #[test]
    fn range() {
        let r = parse_unicode_ranges("U+0041-005A");
        assert_eq!(r, vec![UnicodeRange { start: 0x41, end: 0x5A }]);
    }

    #[test]
    fn wildcard() {
        let r = parse_unicode_ranges("U+26??");
        assert_eq!(r, vec![UnicodeRange { start: 0x2600, end: 0x26FF }]);
    }

    #[test]
    fn multiple_tokens() {
        let r = parse_unicode_ranges("U+0000-007F, U+0400-04FF");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], UnicodeRange { start: 0x0000, end: 0x007F });
        assert_eq!(r[1], UnicodeRange { start: 0x0400, end: 0x04FF });
    }

    #[test]
    fn lowercase_u_prefix() {
        let r = parse_unicode_ranges("u+0041");
        assert_eq!(r, vec![UnicodeRange { start: 0x41, end: 0x41 }]);
    }

    #[test]
    fn invalid_token_skipped() {
        let r = parse_unicode_ranges("notarange, U+0041");
        assert_eq!(r, vec![UnicodeRange { start: 0x41, end: 0x41 }]);
    }

    #[test]
    fn empty_string() {
        assert!(parse_unicode_ranges("").is_empty());
    }

    #[test]
    fn codepoint_in_ranges_empty_is_true() {
        assert!(codepoint_in_ranges(0x41, &[]));
    }

    #[test]
    fn codepoint_in_ranges_hit() {
        let ranges = parse_unicode_ranges("U+0020-007E");
        assert!(codepoint_in_ranges(b'A' as u32, &ranges));
    }

    #[test]
    fn codepoint_in_ranges_miss() {
        let ranges = parse_unicode_ranges("U+0020-007E");
        assert!(!codepoint_in_ranges(0x0400, &ranges)); // кириллица за пределами
    }

    #[test]
    fn wildcard_contains() {
        let r = parse_unicode_ranges("U+26??");
        assert!(codepoint_in_ranges(0x2602, &r)); // ☂
        assert!(!codepoint_in_ranges(0x2700, &r)); // вне диапазона
    }

    #[test]
    fn reversed_range_skipped() {
        // start > end → невалидно, должно игнорироваться
        let r = parse_unicode_ranges("U+00FF-0000");
        assert!(r.is_empty());
    }

    #[test]
    fn over_unicode_max_skipped() {
        let r = parse_unicode_ranges("U+200000");
        assert!(r.is_empty());
    }
}
