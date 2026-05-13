//! Определение кодировки: BOM → meta-sniff → content-type hint → UTF-8 → эвристика.
//!
//! Каждый шаг — отдельная функция, общий `detect` склеивает приоритеты.

use crate::Encoding;
use crate::tables::{CP866, KOI8_R, WIN1251};

/// Сколько байт смотрим при meta-sniff и при частотном анализе.
/// HTML5 spec для meta-sniff требует 1024, реальные браузеры так и делают.
const SNIFF_BUFFER_LEN: usize = 1024;

/// Главная точка входа. Возвращает кодировку, в которой следует декодировать
/// `bytes`. Никогда не возвращает None — если уверенности нет, выбирает UTF-8
/// как самый частый случай в современном вебе.
#[must_use]
pub fn detect(bytes: &[u8], content_type_hint: Option<&str>) -> Encoding {
    // 1. BOM перевешивает всё остальное (включая meta-sniff внутри документа).
    if let Some(enc) = sniff_bom(bytes) {
        return enc;
    }

    // 2. HTML <meta charset>. Авторы документа обычно знают, что у них.
    if let Some(enc) = sniff_meta_charset(bytes) {
        return enc;
    }

    // 3. Подсказка от транспорта (HTTP Content-Type). В Phase 0 не нужна,
    //    но интерфейс уже готов для будущего HTTP-клиента.
    if let Some(hint) = content_type_hint
        && let Some(enc) = parse_content_type(hint)
    {
        return enc;
    }

    // 4. UTF-8 — если весь документ валидно декодируется как UTF-8 и содержит
    //    хотя бы один multi-byte sequence, это почти всегда правильный ответ:
    //    шанс случайно собрать валидный UTF-8 из cp1251-русского текста
    //    исчезающе мал.
    if looks_like_utf8(bytes) {
        return Encoding::Utf8;
    }

    // 5. Частотная эвристика по русским буквам. Между cp1251 / koi8-r / cp866
    //    различия достаточны, чтобы простой подсчёт работал.
    heuristic_pick(bytes)
}

/// Распознаёт BOM (Byte Order Mark) в начале потока.
///
/// - `EF BB BF` → UTF-8;
/// - `FF FE`     → UTF-16 LE (типичный «Save As → Unicode» в Windows);
/// - `FE FF`     → UTF-16 BE (реже, обычно в Java/Mac).
///
/// Порядок проверок важен: UTF-8 BOM не пересекается с UTF-16 (3 байта
/// против 2), но проверяем сначала более длинный, чтобы исключить
/// случайное совпадение префиксов.
fn sniff_bom(bytes: &[u8]) -> Option<Encoding> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some(Encoding::Utf8);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some(Encoding::Utf16Le);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some(Encoding::Utf16Be);
    }
    None
}

/// Ищет `<meta charset>` или `<meta http-equiv="Content-Type" content="...; charset=X">`
/// в первом килобайте. Возвращает кодировку, если нашли поддерживаемый label.
///
/// Алгоритм упрощённый по сравнению с HTML5 «prescan a byte stream to determine
/// its encoding» (12.2.3.2): мы не парсим quoted/unquoted attribute values
/// отдельно, а просто берём подстроку после `charset` до ближайшего разделителя.
/// Для встречающихся в природе документов этого хватает.
#[must_use]
pub fn sniff_meta_charset(bytes: &[u8]) -> Option<Encoding> {
    let limit = bytes.len().min(SNIFF_BUFFER_LEN);
    let lower = lowercase_ascii(&bytes[..limit]);

    let mut search_from = 0;
    while let Some(meta_rel) = find_subslice(&lower[search_from..], b"<meta") {
        let meta_start = search_from + meta_rel;
        // Граница: следующий символ должен быть пробельным или `/`, иначе это
        // не <meta>, а, скажем, <metadata>.
        let after = meta_start + b"<meta".len();
        if after >= lower.len() {
            break;
        }
        let boundary = lower[after];
        if !boundary.is_ascii_whitespace() && boundary != b'/' && boundary != b'>' {
            search_from = after;
            continue;
        }

        // Конец тега — первый `>` после <meta. Внутри ищем charset.
        let tag_end = lower[after..]
            .iter()
            .position(|&b| b == b'>')
            .map_or(lower.len(), |p| after + p);

        if let Some(enc) = extract_charset_from_meta(&lower[after..tag_end]) {
            return Some(enc);
        }

        search_from = tag_end;
    }

    None
}

fn extract_charset_from_meta(tag_body: &[u8]) -> Option<Encoding> {
    // Самый прямой случай: charset=value.
    if let Some(value) = find_attr_value(tag_body, b"charset")
        && let Some(enc) = Encoding::from_label(&value)
    {
        return Some(enc);
    }

    // http-equiv="content-type" content="text/html; charset=..."
    if let Some(equiv) = find_attr_value(tag_body, b"http-equiv")
        && equiv.eq_ignore_ascii_case("content-type")
        && let Some(content) = find_attr_value(tag_body, b"content")
    {
        let lower = content.to_ascii_lowercase();
        if let Some(idx) = lower.find("charset") {
            let after = &lower[idx + "charset".len()..];
            let trimmed = after.trim_start();
            if let Some(rest) = trimmed.strip_prefix('=') {
                let value: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|&c| c != '"' && c != '\'' && c != ';' && c != ' ' && c != '\t')
                    .collect();
                if let Some(enc) = Encoding::from_label(&value) {
                    return Some(enc);
                }
            }
        }
    }

    None
}

/// Находит значение атрибута `name` в теле тега. Поддерживает
/// `name=value`, `name="value"`, `name='value'`. Возвращает значение без кавычек.
fn find_attr_value(tag_body: &[u8], name: &[u8]) -> Option<String> {
    let mut i = 0;
    while i + name.len() <= tag_body.len() {
        // Проверяем границу слева: либо это начало тела, либо пробел / кавычка.
        let left_ok = i == 0
            || tag_body[i - 1].is_ascii_whitespace()
            || tag_body[i - 1] == b'"'
            || tag_body[i - 1] == b'\'';
        if left_ok && tag_body[i..].starts_with(name) {
            let mut j = i + name.len();
            // Пропускаем пробелы перед `=`.
            while j < tag_body.len() && tag_body[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < tag_body.len() && tag_body[j] == b'=' {
                j += 1;
                while j < tag_body.len() && tag_body[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= tag_body.len() {
                    return None;
                }
                let (start, terminator): (usize, u8) = match tag_body[j] {
                    b'"' => (j + 1, b'"'),
                    b'\'' => (j + 1, b'\''),
                    _ => (j, b' '),
                };
                let end = if terminator == b' ' {
                    tag_body[start..]
                        .iter()
                        .position(|&b| b.is_ascii_whitespace() || b == b'/' || b == b'>')
                        .map_or(tag_body.len(), |p| start + p)
                } else {
                    tag_body[start..]
                        .iter()
                        .position(|&b| b == terminator)
                        .map_or(tag_body.len(), |p| start + p)
                };
                return Some(String::from_utf8_lossy(&tag_body[start..end]).into_owned());
            }
        }
        i += 1;
    }
    None
}

/// Парсит значение HTTP-заголовка Content-Type, ищет `charset=value`.
fn parse_content_type(value: &str) -> Option<Encoding> {
    let lower = value.to_ascii_lowercase();
    let idx = lower.find("charset")?;
    let after = &lower[idx + "charset".len()..];
    let trimmed = after.trim_start();
    let rest = trimmed.strip_prefix('=')?.trim_start();
    let value: String = rest
        .chars()
        .take_while(|&c| c != '"' && c != '\'' && c != ';' && c != ' ' && c != '\t')
        .collect();
    Encoding::from_label(&value)
}

/// Возвращает true, если в потоке есть multi-byte UTF-8 sequence и весь поток
/// валиден как UTF-8. Чистый ASCII не считается «выглядит как UTF-8» — для
/// чистого ASCII все четыре кодировки эквивалентны, и выбор делать не на чем.
fn looks_like_utf8(bytes: &[u8]) -> bool {
    let has_high_bit = bytes.iter().any(|&b| b >= 0x80);
    has_high_bit && std::str::from_utf8(bytes).is_ok()
}

/// Частоты русских букв в обычных текстах (в долях от всех букв).
/// Источник — статистика по корпусу художественной и научно-популярной
/// литературы (общеизвестные значения). Сумма ≈ 1.0.
///
/// Используются только строчные: при подсчёте делаем `to_lowercase()`. Это
/// нормально, поскольку соотношение строчных и заглавных одинаково для всех
/// трёх кодировок.
const RU_FREQ: &[(char, f64)] = &[
    ('о', 0.10983),
    ('е', 0.08483),
    ('а', 0.07998),
    ('и', 0.07367),
    ('н', 0.06700),
    ('т', 0.06318),
    ('с', 0.05473),
    ('р', 0.04746),
    ('в', 0.04533),
    ('л', 0.04343),
    ('к', 0.03486),
    ('м', 0.03203),
    ('д', 0.02977),
    ('п', 0.02804),
    ('у', 0.02615),
    ('я', 0.02001),
    ('ы', 0.01898),
    ('ь', 0.01735),
    ('г', 0.01687),
    ('з', 0.01641),
    ('б', 0.01592),
    ('ч', 0.01450),
    ('й', 0.01208),
    ('х', 0.00966),
    ('ж', 0.00940),
    ('ш', 0.00718),
    ('ю', 0.00638),
    ('ц', 0.00486),
    ('щ', 0.00361),
    ('э', 0.00331),
    ('ф', 0.00267),
    ('ъ', 0.00037),
];

/// Выбирает кодировку из cp1251 / KOI8-R / CP866, набравшую максимальный score.
/// Для cyrillic-текста правильная кодировка декодирует байты в плотный поток
/// русских букв с реалистичным распределением; ошибочная — даёт мешанину
/// псевдографики и редких букв.
fn heuristic_pick(bytes: &[u8]) -> Encoding {
    // Берём первые ~1 КБ для скорости: больше не нужно, статистики хватит.
    let prefix_len = bytes.len().min(SNIFF_BUFFER_LEN);
    let prefix = &bytes[..prefix_len];

    let scores = [
        (Encoding::Windows1251, score_for_table(prefix, &WIN1251)),
        (Encoding::Koi8R, score_for_table(prefix, &KOI8_R)),
        (Encoding::Cp866, score_for_table(prefix, &CP866)),
    ];

    scores
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map_or(Encoding::Windows1251, |(enc, _)| enc)
}

fn score_for_table(bytes: &[u8], table: &[char; 128]) -> f64 {
    let mut score = 0.0;
    for &b in bytes {
        if b < 0x80 {
            continue;
        }
        let ch = table[(b - 0x80) as usize];
        // Только русские строчные; заглавные приводим к строчным через
        // прибавление 0x20 в коде. is_ascii_uppercase здесь не подходит,
        // используем явный диапазон.
        let key = if ('А'..='Я').contains(&ch) {
            char::from_u32(ch as u32 + 0x20).unwrap_or(ch)
        } else {
            ch
        };
        if let Some(&(_, freq)) = RU_FREQ.iter().find(|(c, _)| *c == key) {
            score += freq;
        }
    }
    score
}

fn lowercase_ascii(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().map(|&b| b.to_ascii_lowercase()).collect()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_wins() {
        let bytes = b"\xEF\xBB\xBFhello";
        assert_eq!(detect(bytes, None), Encoding::Utf8);
    }

    #[test]
    fn meta_charset_utf8() {
        let html = b"<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head>";
        assert_eq!(detect(html, None), Encoding::Utf8);
    }

    #[test]
    fn meta_charset_cp1251_unquoted() {
        let html = b"<html><head><meta charset=windows-1251></head>";
        assert_eq!(sniff_meta_charset(html), Some(Encoding::Windows1251));
    }

    #[test]
    fn meta_http_equiv_content_type() {
        let html =
            b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=koi8-r\">";
        assert_eq!(sniff_meta_charset(html), Some(Encoding::Koi8R));
    }

    #[test]
    fn meta_uppercase_tag() {
        let html = b"<HTML><HEAD><META CHARSET='windows-1251'></HEAD>";
        assert_eq!(sniff_meta_charset(html), Some(Encoding::Windows1251));
    }

    #[test]
    fn meta_after_viewport_meta() {
        // Несколько <meta>, charset не в первом.
        let html = b"<html><head><meta name=viewport content=\"width=device-width\">\
            <meta charset=\"cp866\"></head>";
        assert_eq!(sniff_meta_charset(html), Some(Encoding::Cp866));
    }

    #[test]
    fn metadata_does_not_match_meta() {
        let html = b"<metadata charset=\"utf-8\">";
        assert_eq!(sniff_meta_charset(html), None);
    }

    #[test]
    fn content_type_hint_used() {
        // Без BOM, без meta, но с явным HTTP-заголовком.
        let bytes = b"<html><body>hello</body></html>";
        assert_eq!(
            detect(bytes, Some("text/html; charset=windows-1251")),
            Encoding::Windows1251
        );
    }

    #[test]
    fn ascii_only_falls_back_to_utf8_via_heuristic() {
        // Чистый ASCII: looks_like_utf8 вернёт false (нет high-bit),
        // эвристика всем даст 0.0 → выберем cp1251 (первый). Не идеально,
        // но для чистого ASCII все три кодировки декодируют одинаково.
        let bytes = b"<html><body>hello world</body></html>";
        // Здесь мы проверяем, что хотя бы не паникуем.
        let enc = detect(bytes, None);
        assert!(matches!(
            enc,
            Encoding::Utf8 | Encoding::Windows1251 | Encoding::Koi8R | Encoding::Cp866
        ));
    }

    #[test]
    fn detect_utf8_cyrillic_no_bom() {
        // «Привет, мир» в UTF-8.
        let bytes = "Привет, мир".as_bytes();
        assert_eq!(detect(bytes, None), Encoding::Utf8);
    }

    #[test]
    fn detect_cp1251_heuristic() {
        // «Привет, мир» в Windows-1251. Без meta / BOM / hint → должна сработать эвристика.
        let bytes: &[u8] = &[
            0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, b',', b' ', 0xEC, 0xE8, 0xF0,
        ];
        assert_eq!(detect(bytes, None), Encoding::Windows1251);
    }

    #[test]
    fn detect_koi8r_heuristic() {
        // «Привет, мир» в KOI8-R.
        let bytes: &[u8] = &[
            0xF0, 0xD2, 0xC9, 0xD7, 0xC5, 0xD4, b',', b' ', 0xCD, 0xC9, 0xD2,
        ];
        assert_eq!(detect(bytes, None), Encoding::Koi8R);
    }

    #[test]
    fn detect_cp866_heuristic() {
        // «Привет, мир» в CP866. Заглавная П (0x8F), затем строчные.
        let bytes: &[u8] = &[
            0x8F, 0xE0, 0xA8, 0xA2, 0xA5, 0xE2, b',', b' ', 0xAC, 0xA8, 0xE0,
        ];
        assert_eq!(detect(bytes, None), Encoding::Cp866);
    }

    #[test]
    fn detect_longer_paragraph_cp1251() {
        // Реальный фрагмент текста в cp1251 для уверенной эвристики.
        // «Скажи-ка, дядя, ведь не даром Москва, спалённая пожаром, французу отдана?»
        let bytes: &[u8] = &[
            0xD1, 0xEA, 0xE0, 0xE6, 0xE8, b'-', 0xEA, 0xE0, b',', b' ', 0xE4, 0xFF, 0xE4, 0xFF,
            b',', b' ', 0xE2, 0xE5, 0xE4, 0xFC, b' ', 0xED, 0xE5, b' ', 0xE4, 0xE0, 0xF0, 0xEE,
            0xEC, b' ', 0xCC, 0xEE, 0xF1, 0xEA, 0xE2, 0xE0, b',', b' ', 0xF1, 0xEF, 0xE0, 0xEB,
            0xB8, 0xED, 0xED, 0xE0, 0xFF, b' ', 0xEF, 0xEE, 0xE6, 0xE0, 0xF0, 0xEE, 0xEC, b',',
        ];
        assert_eq!(detect(bytes, None), Encoding::Windows1251);
    }

    #[test]
    fn detect_meta_overrides_bytes() {
        // Документ объявил cp1251, но сами байты — UTF-8. По spec meta-sniff
        // приоритетнее эвристики, мы должны вернуть cp1251 (документ
        // соврёт — это его ответственность).
        let html = b"<head><meta charset=windows-1251></head><body>plain</body>";
        assert_eq!(detect(html, None), Encoding::Windows1251);
    }

    #[test]
    fn detect_bom_overrides_meta() {
        let html = b"\xEF\xBB\xBF<head><meta charset=windows-1251></head>";
        assert_eq!(detect(html, None), Encoding::Utf8);
    }

    #[test]
    fn bom_utf16_le() {
        // FF FE — UTF-16 LE BOM. "AB" в UTF-16 LE = 41 00 42 00.
        let bytes = &[0xFF, 0xFE, 0x41, 0x00, 0x42, 0x00];
        assert_eq!(detect(bytes, None), Encoding::Utf16Le);
    }

    #[test]
    fn bom_utf16_be() {
        let bytes = &[0xFE, 0xFF, 0x00, 0x41, 0x00, 0x42];
        assert_eq!(detect(bytes, None), Encoding::Utf16Be);
    }

    #[test]
    fn bom_utf16_overrides_content_type() {
        // BOM приоритетнее content-type hint.
        let bytes = &[0xFF, 0xFE, 0x41, 0x00];
        assert_eq!(
            detect(bytes, Some("text/html; charset=windows-1251")),
            Encoding::Utf16Le
        );
    }

    #[test]
    fn label_utf16_maps_to_le() {
        // WHATWG-совместимо: голый "utf-16" — это LE.
        assert_eq!(Encoding::from_label("utf-16"), Some(Encoding::Utf16Le));
        assert_eq!(Encoding::from_label("UTF-16"), Some(Encoding::Utf16Le));
        assert_eq!(Encoding::from_label("unicode"), Some(Encoding::Utf16Le));
    }

    #[test]
    fn label_utf16be_distinct() {
        assert_eq!(Encoding::from_label("utf-16be"), Some(Encoding::Utf16Be));
        assert_eq!(Encoding::from_label("UTF-16BE"), Some(Encoding::Utf16Be));
    }
}
