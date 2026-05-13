//! Punycode encoding (RFC 3492 §6.3).
//!
//! Используется для IDN: преобразование Unicode-меток в ASCII-форму
//! `xn--…`. На этапе Phase 0 реализован только encode — decode (для
//! отображения Unicode-формы пользователю) добавим, когда понадобится.
//!
//! Алгоритм — bootstring с параметрами Punycode (base=36, tmin=1,
//! tmax=26, skew=38, damp=700, initial_bias=72, initial_n=128). Это
//! variable-length base-36 кодирование Unicode-кодпоинтов в ASCII
//! (a-z = 0-25, 0-9 = 26-35).

use crate::error::{Error, Result};

const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;

fn adapt(mut delta: u32, numpoints: u32, firsttime: bool) -> u32 {
    delta /= if firsttime { DAMP } else { 2 };
    delta += delta / numpoints;
    let mut k = 0;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (((BASE - TMIN + 1) * delta) / (delta + SKEW))
}

fn digit_to_char(d: u32) -> char {
    debug_assert!(d < BASE);
    if d < 26 {
        (b'a' + d as u8) as char
    } else {
        (b'0' + (d - 26) as u8) as char
    }
}

/// Кодирует Unicode-строку в Punycode согласно RFC 3492.
///
/// Возвращает строку: сначала базовые ASCII-символы из входа (если
/// есть), потом `-` как разделитель, потом extended-часть. Если базовых
/// нет — extended часть идёт сразу без `-`.
///
/// Пустой вход возвращает пустую строку.
pub fn encode(input: &str) -> Result<String> {
    let codepoints: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let input_len = codepoints.len() as u32;

    let mut output = String::new();

    for &c in &codepoints {
        if c < 0x80 {
            output.push(c as u8 as char);
        }
    }
    let basic = output.len() as u32;
    let mut h = basic;
    if basic > 0 && basic < input_len {
        output.push('-');
    }

    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;

    while h < input_len {
        // Минимальный non-basic codepoint >= n среди input-а.
        let m = codepoints
            .iter()
            .copied()
            .filter(|&c| c >= n)
            .min()
            .expect("non-empty: h < input_len гарантирует existence");

        delta = delta
            .checked_add(
                (m - n)
                    .checked_mul(h + 1)
                    .ok_or_else(|| Error::Other("punycode: delta overflow".into()))?,
            )
            .ok_or_else(|| Error::Other("punycode: delta overflow".into()))?;
        n = m;

        for &c in &codepoints {
            if c < n {
                delta = delta
                    .checked_add(1)
                    .ok_or_else(|| Error::Other("punycode: delta overflow".into()))?;
            } else if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    let d = t + ((q - t) % (BASE - t));
                    output.push(digit_to_char(d));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                output.push(digit_to_char(q));
                bias = adapt(delta, h + 1, h == basic);
                delta = 0;
                h += 1;
            }
        }

        delta += 1;
        n += 1;
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert_eq!(encode("").unwrap(), "");
    }

    #[test]
    fn all_ascii_no_delimiter() {
        // Все символы basic, extended-части нет — `-` НЕ добавляется.
        assert_eq!(encode("hello").unwrap(), "hello");
    }

    #[test]
    fn cyrillic_primer() {
        // пример → e1afmkfd (verified против RFC 3492 алгоритма вручную)
        assert_eq!(encode("пример").unwrap(), "e1afmkfd");
    }

    #[test]
    fn cyrillic_rf() {
        // рф → p1ai (TLD .рф)
        assert_eq!(encode("рф").unwrap(), "p1ai");
    }

    #[test]
    fn cyrillic_prezident() {
        // президент → d1abbgf6aiiy (известный пример из IDN-доменов)
        assert_eq!(encode("президент").unwrap(), "d1abbgf6aiiy");
    }

    #[test]
    fn cyrillic_test() {
        // тест → e1aybc
        assert_eq!(encode("тест").unwrap(), "e1aybc");
    }

    #[test]
    fn cyrillic_rus() {
        // рус → p1acf
        assert_eq!(encode("рус").unwrap(), "p1acf");
    }

    #[test]
    fn mixed_ascii_cyrillic() {
        // "a-привет" : basic части 'a' и '-', потом delimiter '-', потом extended.
        // Basic = "a-", потом '-', потом encoded
        let result = encode("a-привет").unwrap();
        assert!(result.starts_with("a--"), "got: {result}");
    }

    #[test]
    fn cjk_chinese() {
        // 你好 → 6qq79v (популярный CJK тест-кейс)
        assert_eq!(encode("你好").unwrap(), "6qq79v");
    }
}
