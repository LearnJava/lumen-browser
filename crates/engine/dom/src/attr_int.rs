//! Числовой разбор значений содержимых атрибутов по HTML LS §2.4.4 — «rules for
//! parsing integers» / «rules for parsing non-negative integers».
//!
//! Отдельный модуль, а не свободная функция в `lib.rs`: правило одно на весь
//! движок, а реализаций до BUG-452 было три, и все три расходились между собой.
//! JS-сторона зовёт своё зеркало (`_lumen_parse_integer`,
//! `crates/js/src/shim/web_api_shim_tail_b.js`) — эти два разбора обязаны
//! отвечать одинаково, иначе `canvas.width` из JS и ширина бокса в layout
//! разъезжаются на одном и том же атрибуте (ровно это и было: `width="100.999"`
//! рисовался как 300, а из скрипта читался как 100).
//!
//! Почему не `str::parse::<u32>()`, которым это было написано в layout: у него
//! свои правила, не совпадающие со спекой ни в одну сторону — он отвергает
//! `"100.999"`, `"100em"`, `"0x100"` (спека даёт 100/100/**0**) и принимает
//! ведущий `+` без всякого хвоста.

/// Верхняя граница отражаемого `unsigned long`/`long` (HTML LS §2.6.2): всё, что
/// выходит за неё, отражается значением по умолчанию, а не насыщается.
pub const REFLECT_LONG_MAX: i64 = 2_147_483_647;

/// ASCII whitespace по Infra §4.6 — TAB, LF, FF, CR, SPACE. Именно этот набор
/// пропускает §2.4.4.1; `str::trim()` шире (он ест ещё и U+00A0), поэтому здесь
/// не годится.
fn is_ascii_ws(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

/// HTML LS §2.4.4.1 «rules for parsing integers».
///
/// Пропускает ведущий ASCII whitespace, берёт необязательный знак, требует хотя
/// бы одну ASCII-цифру, собирает цифры и **останавливается** — хвост после них
/// игнорируется, а не отвергается (`"100em"` → `Some(100)`, `"0x100"` →
/// `Some(0)`). `None` — ошибка разбора: пусто, только пробелы, знак без цифры,
/// первый значащий символ не цифра.
///
/// Значение считается в `i64`, чтобы переполнение `u32`/`i32` осталось
/// наблюдаемым для вызывающего (§2.6.2 отвечает на него дефолтом, а не
/// обрезанием); разбор насыщается на `i64::MAX`, чтобы строка из тысячи цифр не
/// паниковала в отладочной сборке.
pub fn parse_integer(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && is_ascii_ws(b[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    if i >= b.len() || !b[i].is_ascii_digit() {
        return None;
    }
    let mut n: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n
            .saturating_mul(10)
            .saturating_add(i64::from(b[i] - b'0'));
        i += 1;
    }
    Some(if neg { -n } else { n })
}

/// HTML LS §2.4.4.2 «rules for parsing non-negative integers»: [`parse_integer`],
/// затем отказ на отрицательном результате.
pub fn parse_non_negative_integer(s: &str) -> Option<i64> {
    parse_integer(s).filter(|n| *n >= 0)
}

/// Геттер отражаемого `unsigned long` (HTML LS §2.6.2): разобрать значение как
/// неотрицательное целое и ответить `default`, если разбор не удался или
/// результат вне 0…2147483647.
///
/// `attr` — значение содержимого атрибута; `None` означает «атрибут
/// отсутствует», что тоже даёт `default`.
pub fn reflect_unsigned_long(attr: Option<&str>, default: u32) -> u32 {
    match attr.and_then(parse_non_negative_integer) {
        Some(n) if n <= REFLECT_LONG_MAX => n as u32,
        _ => default,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Таблица — дословно ожидания WPT `2d.canvas.host.size.attributes.parse.*`
    /// (вендорено в `tests/wpt/html/canvas/element/canvas-host/`), потому что
    /// именно они задают, чем «rules for parsing integers» отличаются от
    /// `parse::<u32>()` и от `parseInt`.
    #[test]
    fn wpt_canvas_size_attribute_table() {
        for (input, want) in [
            ("0", 0u32),
            ("100.999", 100),
            ("100em", 100),
            ("", 300),
            ("100e1", 100),
            ("0x100", 0),
            ("#!?", 300),
            ("-100", 300),
            ("0100", 100),
            ("  ", 300),
            ("100%", 100),
            ("+100", 100),
            ("  100", 100),
            ("100#!?", 100),
            ("\r\n\t\u{0c}100", 100),
        ] {
            assert_eq!(
                reflect_unsigned_long(Some(input), 300),
                want,
                "attr {input:?}"
            );
        }
    }

    /// Верхняя граница §2.6.2 — не насыщение: значение за ней отражается
    /// дефолтом. Без этого `<canvas width="4294967291">` читался бы дословно.
    #[test]
    fn out_of_range_reflects_the_default() {
        assert_eq!(reflect_unsigned_long(Some("2147483647"), 300), 2147483647);
        assert_eq!(reflect_unsigned_long(Some("2147483648"), 300), 300);
        assert_eq!(reflect_unsigned_long(Some("4294967291"), 300), 300);
        // Тысяча девяток не должна ни паниковать, ни завернуться в маленькое
        // число — только насытиться и уехать за границу.
        assert_eq!(reflect_unsigned_long(Some(&"9".repeat(1000)), 300), 300);
    }

    #[test]
    fn missing_attribute_is_the_default() {
        assert_eq!(reflect_unsigned_long(None, 150), 150);
    }

    /// Знак разбирается, но неотрицательный вариант его отвергает — эти два
    /// уровня спека держит раздельно, и `tabindex="-1"` живёт на нижнем.
    #[test]
    fn signed_and_unsigned_levels_differ() {
        assert_eq!(parse_integer("-1"), Some(-1));
        assert_eq!(parse_non_negative_integer("-1"), None);
        assert_eq!(parse_integer("+5zzz"), Some(5));
        assert_eq!(parse_integer("+"), None);
        assert_eq!(parse_integer("-"), None);
        assert_eq!(parse_integer("\u{a0}5"), None, "U+00A0 не ASCII whitespace");
    }
}
