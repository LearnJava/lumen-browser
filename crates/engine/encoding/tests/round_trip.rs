//! Round-trip тесты: задаём ожидаемый русский текст, генерируем байты в каждой
//! из трёх однобайтовых кодировок, проверяем что детектор узнаёт кодировку и
//! декодер восстанавливает исходный текст.
//!
//! Обратная таблица «char → byte» строится через публичный `decode` —
//! декодируем единичный байт и запоминаем соответствие. Это сохраняет
//! приватность внутренних таблиц.

use std::collections::HashMap;

use lumen_encoding::{Encoding, decode, detect};

/// Строит обратную таблицу «Unicode-символ → байт» для однобайтовой кодировки.
/// Берёт каждый байт 0..=255, декодирует как единичный поток, запоминает
/// результат. Битые «не определено» значения (U+FFFD) исключаются — иначе
/// несколько байт сошлось бы на один символ.
fn reverse_table(target: Encoding) -> HashMap<char, u8> {
    assert!(
        !matches!(target, Encoding::Utf8),
        "UTF-8 не однобайтовая кодировка"
    );
    let mut map = HashMap::with_capacity(256);
    for b in 0u8..=255 {
        let decoded = decode(target, &[b]);
        let mut chars = decoded.chars();
        if let (Some(ch), None) = (chars.next(), chars.next())
            && ch != '\u{FFFD}'
        {
            map.entry(ch).or_insert(b);
        }
    }
    map
}

fn encode_single_byte(text: &str, target: Encoding) -> Option<Vec<u8>> {
    let table = reverse_table(target);
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        out.push(*table.get(&ch)?);
    }
    Some(out)
}

const SAMPLE: &str = "Скажи-ка, дядя, ведь не даром Москва, спалённая пожаром, \
                     французу отдана? Ведь были ж схватки боевые, да, говорят, \
                     ещё какие! Недаром помнит вся Россия про день Бородина!";

#[test]
fn cp1251_round_trip() {
    let bytes = encode_single_byte(SAMPLE, Encoding::Windows1251).expect("encode cp1251");
    assert_eq!(detect(&bytes, None), Encoding::Windows1251);
    assert_eq!(decode(Encoding::Windows1251, &bytes), SAMPLE);
}

#[test]
fn koi8r_round_trip() {
    let bytes = encode_single_byte(SAMPLE, Encoding::Koi8R).expect("encode koi8");
    assert_eq!(detect(&bytes, None), Encoding::Koi8R);
    assert_eq!(decode(Encoding::Koi8R, &bytes), SAMPLE);
}

#[test]
fn cp866_round_trip() {
    let bytes = encode_single_byte(SAMPLE, Encoding::Cp866).expect("encode cp866");
    assert_eq!(detect(&bytes, None), Encoding::Cp866);
    assert_eq!(decode(Encoding::Cp866, &bytes), SAMPLE);
}

#[test]
fn utf8_cyrillic_round_trip() {
    let bytes = SAMPLE.as_bytes();
    assert_eq!(detect(bytes, None), Encoding::Utf8);
    assert_eq!(decode(Encoding::Utf8, bytes), SAMPLE);
}

#[test]
fn html_document_cp1251_full_pipeline() {
    let html_template = "<!DOCTYPE html><html><head><meta charset=\"windows-1251\">\
        <title>Тест</title></head><body><h1>Заголовок</h1><p>Текст параграфа \
        на русском языке для проверки кодировки.</p></body></html>";
    let bytes = encode_single_byte(html_template, Encoding::Windows1251).expect("encode");

    assert_eq!(detect(&bytes, None), Encoding::Windows1251);
    let decoded = decode(Encoding::Windows1251, &bytes);
    assert!(decoded.contains("Заголовок"));
    assert!(decoded.contains("параграфа"));
}

#[test]
fn meta_overrides_heuristic_when_lying() {
    // Документ объявил cp1251, но реальные байты — UTF-8 cyrillic.
    // По HTML5 spec meta-sniff приоритетнее эвристики.
    let mut bytes = b"<head><meta charset=\"windows-1251\"></head><body>".to_vec();
    bytes.extend_from_slice("Привет".as_bytes());
    bytes.extend_from_slice(b"</body>");
    assert_eq!(detect(&bytes, None), Encoding::Windows1251);
}
