//! Минимальный JSON parser per RFC 8259.
//!
//! Используется для: Web Manifest typed parsing, plugin capabilities,
//! настройки пользователя, snapshot форматы. Свой парсер (~200 LOC),
//! не serde — соответствует политике «default — своё» для парсеров.
//!
//! Поддерживает: null/true/false/Number/String/Array/Object. Number
//! хранится как f64 — для browser-сценариев этого достаточно. Дубликаты
//! ключей в Object — last-wins (per RFC §4 «JSON parsers MAY ignore
//! the order in which the members appear»).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    /// BTreeMap чтобы итерация шла в лексикографическом порядке ключей —
    /// детерминированно для тестов.
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        if let Self::Number(n) = self {
            Some(*n)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }

    pub fn as_array(&self) -> Option<&[JsonValue]> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        if let Self::Object(o) = self {
            Some(o)
        } else {
            None
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object()?.get(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonError {
    UnexpectedEof,
    UnexpectedChar { expected: &'static str, got: char, at: usize },
    InvalidEscape(char),
    InvalidNumber(String),
    InvalidLiteral(String),
    /// JSON-документ содержит trailing garbage после первого value.
    TrailingGarbage { at: usize },
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected EOF"),
            Self::UnexpectedChar { expected, got, at } => write!(
                f,
                "unexpected `{got}` at pos {at}, expected {expected}"
            ),
            Self::InvalidEscape(c) => write!(f, "invalid escape `\\{c}`"),
            Self::InvalidNumber(s) => write!(f, "invalid number `{s}`"),
            Self::InvalidLiteral(s) => write!(f, "invalid literal `{s}`"),
            Self::TrailingGarbage { at } => write!(f, "trailing garbage at pos {at}"),
        }
    }
}

impl std::error::Error for JsonError {}

pub type JsonResult<T> = std::result::Result<T, JsonError>;

pub fn parse(text: &str) -> JsonResult<JsonValue> {
    let mut p = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos < p.bytes.len() {
        return Err(JsonError::TrailingGarbage { at: p.pos });
    }
    Ok(v)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> JsonResult<u8> {
        self.bytes.get(self.pos).copied().ok_or(JsonError::UnexpectedEof)
    }

    fn parse_value(&mut self) -> JsonResult<JsonValue> {
        self.skip_ws();
        match self.peek()? {
            b'"' => Ok(JsonValue::String(self.parse_string()?)),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' | b'f' => self.parse_bool(),
            b'n' => self.parse_null(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            other => Err(JsonError::UnexpectedChar {
                expected: "value",
                got: other as char,
                at: self.pos,
            }),
        }
    }

    fn parse_null(&mut self) -> JsonResult<JsonValue> {
        if self.bytes.get(self.pos..self.pos + 4) == Some(b"null") {
            self.pos += 4;
            return Ok(JsonValue::Null);
        }
        Err(JsonError::InvalidLiteral(self.snippet(4)))
    }

    fn parse_bool(&mut self) -> JsonResult<JsonValue> {
        if self.bytes.get(self.pos..self.pos + 4) == Some(b"true") {
            self.pos += 4;
            return Ok(JsonValue::Bool(true));
        }
        if self.bytes.get(self.pos..self.pos + 5) == Some(b"false") {
            self.pos += 5;
            return Ok(JsonValue::Bool(false));
        }
        Err(JsonError::InvalidLiteral(self.snippet(5)))
    }

    fn snippet(&self, len: usize) -> String {
        let end = (self.pos + len).min(self.bytes.len());
        String::from_utf8_lossy(&self.bytes[self.pos..end]).into_owned()
    }

    fn parse_string(&mut self) -> JsonResult<String> {
        // Уже на `"`.
        if self.peek()? != b'"' {
            return Err(JsonError::UnexpectedChar {
                expected: "\"",
                got: self.peek()? as char,
                at: self.pos,
            });
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            let b = self.peek()?;
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.peek()?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let code = self.parse_hex4()?;
                            if (0xD800..=0xDBFF).contains(&code) {
                                // High surrogate — должно быть \u + low surrogate.
                                if self.peek()? == b'\\'
                                    && self.bytes.get(self.pos + 1).copied() == Some(b'u')
                                {
                                    self.pos += 2;
                                    let low = self.parse_hex4()?;
                                    if (0xDC00..=0xDFFF).contains(&low) {
                                        let cp = 0x10000
                                            + ((code - 0xD800) << 10)
                                            + (low - 0xDC00);
                                        out.push(
                                            char::from_u32(cp).unwrap_or('\u{FFFD}'),
                                        );
                                        continue;
                                    }
                                }
                                out.push('\u{FFFD}');
                            } else if (0xDC00..=0xDFFF).contains(&code) {
                                out.push('\u{FFFD}');
                            } else {
                                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                            }
                        }
                        c => return Err(JsonError::InvalidEscape(c as char)),
                    }
                }
                // Multi-byte UTF-8 — копируем raw байты, валидность гарантирована
                // тем что входная строка `&str` уже UTF-8.
                _ => {
                    // Возможна multi-byte последовательность; pos уже после b.
                    // Найдём конец UTF-8 char-а.
                    let char_len = utf8_char_len(b);
                    let start = self.pos - 1;
                    let end = start + char_len;
                    if end > self.bytes.len() {
                        return Err(JsonError::UnexpectedEof);
                    }
                    let s = std::str::from_utf8(&self.bytes[start..end])
                        .map_err(|_| JsonError::UnexpectedEof)?;
                    out.push_str(s);
                    self.pos = end;
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> JsonResult<u32> {
        if self.pos + 4 > self.bytes.len() {
            return Err(JsonError::UnexpectedEof);
        }
        let s = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| JsonError::UnexpectedEof)?;
        let n = u32::from_str_radix(s, 16)
            .map_err(|_| JsonError::InvalidNumber(s.to_string()))?;
        self.pos += 4;
        Ok(n)
    }

    fn parse_number(&mut self) -> JsonResult<JsonValue> {
        let start = self.pos;
        if self.peek()? == b'-' {
            self.pos += 1;
        }
        // integer part.
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => {
                while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            other => {
                return Err(JsonError::UnexpectedChar {
                    expected: "digit",
                    got: other as char,
                    at: self.pos,
                });
            }
        }
        // fraction.
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
            self.pos += 1;
            let frac_start = self.pos;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if self.pos == frac_start {
                return Err(JsonError::InvalidNumber(self.snippet_from(start)));
            }
        }
        // exponent.
        if self.pos < self.bytes.len() && (self.bytes[self.pos] == b'e' || self.bytes[self.pos] == b'E') {
            self.pos += 1;
            if self.pos < self.bytes.len() && (self.bytes[self.pos] == b'+' || self.bytes[self.pos] == b'-') {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(JsonError::InvalidNumber(self.snippet_from(start)));
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| JsonError::UnexpectedEof)?;
        let n = s.parse::<f64>().map_err(|_| JsonError::InvalidNumber(s.to_string()))?;
        Ok(JsonValue::Number(n))
    }

    fn snippet_from(&self, start: usize) -> String {
        let end = (start + 16).min(self.bytes.len());
        String::from_utf8_lossy(&self.bytes[start..end]).into_owned()
    }

    fn parse_array(&mut self) -> JsonResult<JsonValue> {
        self.pos += 1; // '['
        let mut out = Vec::new();
        self.skip_ws();
        if self.peek()? == b']' {
            self.pos += 1;
            return Ok(JsonValue::Array(out));
        }
        loop {
            self.skip_ws();
            out.push(self.parse_value()?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Ok(JsonValue::Array(out));
                }
                other => {
                    return Err(JsonError::UnexpectedChar {
                        expected: ", or ]",
                        got: other as char,
                        at: self.pos,
                    });
                }
            }
        }
    }

    fn parse_object(&mut self) -> JsonResult<JsonValue> {
        self.pos += 1; // '{'
        let mut out = BTreeMap::new();
        self.skip_ws();
        if self.peek()? == b'}' {
            self.pos += 1;
            return Ok(JsonValue::Object(out));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek()? != b':' {
                return Err(JsonError::UnexpectedChar {
                    expected: ":",
                    got: self.peek()? as char,
                    at: self.pos,
                });
            }
            self.pos += 1;
            self.skip_ws();
            let val = self.parse_value()?;
            out.insert(key, val);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Ok(JsonValue::Object(out));
                }
                other => {
                    return Err(JsonError::UnexpectedChar {
                        expected: ", or }",
                        got: other as char,
                        at: self.pos,
                    });
                }
            }
        }
    }
}

fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1, // invalid, treat as 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_null() {
        assert_eq!(parse("null"), Ok(JsonValue::Null));
    }

    #[test]
    fn parse_booleans() {
        assert_eq!(parse("true"), Ok(JsonValue::Bool(true)));
        assert_eq!(parse("false"), Ok(JsonValue::Bool(false)));
    }

    #[test]
    fn parse_integers() {
        assert_eq!(parse("0"), Ok(JsonValue::Number(0.0)));
        assert_eq!(parse("42"), Ok(JsonValue::Number(42.0)));
        assert_eq!(parse("-17"), Ok(JsonValue::Number(-17.0)));
    }

    #[test]
    fn parse_floats() {
        assert_eq!(parse("2.5"), Ok(JsonValue::Number(2.5)));
        assert_eq!(parse("-0.5"), Ok(JsonValue::Number(-0.5)));
    }

    #[test]
    fn parse_exponent() {
        assert_eq!(parse("1e3"), Ok(JsonValue::Number(1000.0)));
        assert_eq!(parse("1.5E-2"), Ok(JsonValue::Number(0.015)));
        assert_eq!(parse("1e+2"), Ok(JsonValue::Number(100.0)));
    }

    #[test]
    fn parse_string_basic() {
        assert_eq!(parse(r#""hello""#), Ok(JsonValue::String("hello".to_string())));
    }

    #[test]
    fn parse_string_escapes() {
        assert_eq!(
            parse(r#""a\nb\tc""#),
            Ok(JsonValue::String("a\nb\tc".to_string()))
        );
        assert_eq!(
            parse(r#""\"\\\/""#),
            Ok(JsonValue::String("\"\\/".to_string()))
        );
    }

    #[test]
    fn parse_string_unicode_escape() {
        assert_eq!(parse(r#""A""#), Ok(JsonValue::String("A".to_string())));
        assert_eq!(
            parse(r#""Привет""#),
            Ok(JsonValue::String("Привет".to_string()))
        );
    }

    #[test]
    fn parse_string_surrogate_pair() {
        // 😀 = U+1F600 = high \uD83D + low \uDE00.
        let r = parse(r#""😀""#).unwrap();
        assert_eq!(r.as_str().unwrap(), "😀");
    }

    #[test]
    fn parse_string_cyrillic_passthrough() {
        // Кириллица в UTF-8 — должна проходить через парсер без escape-ей.
        assert_eq!(
            parse(r#""Привет""#),
            Ok(JsonValue::String("Привет".to_string()))
        );
    }

    #[test]
    fn parse_array_empty() {
        assert_eq!(parse("[]"), Ok(JsonValue::Array(vec![])));
    }

    #[test]
    fn parse_array_basic() {
        let r = parse("[1, 2, 3]").unwrap();
        let arr = r.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_number(), Some(1.0));
    }

    #[test]
    fn parse_array_nested() {
        let r = parse("[[1, 2], [3, 4]]").unwrap();
        let arr = r.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_array().unwrap().len(), 2);
    }

    #[test]
    fn parse_object_empty() {
        assert_eq!(parse("{}"), Ok(JsonValue::Object(BTreeMap::new())));
    }

    #[test]
    fn parse_object_basic() {
        let r = parse(r#"{"a": 1, "b": "two"}"#).unwrap();
        assert_eq!(r.get("a").unwrap().as_number(), Some(1.0));
        assert_eq!(r.get("b").unwrap().as_str(), Some("two"));
    }

    #[test]
    fn parse_object_duplicate_keys_last_wins() {
        let r = parse(r#"{"a": 1, "a": 2}"#).unwrap();
        assert_eq!(r.get("a").unwrap().as_number(), Some(2.0));
    }

    #[test]
    fn parse_nested_object() {
        let r = parse(r#"{"outer": {"inner": [1, 2]}}"#).unwrap();
        let inner = r.get("outer").unwrap().get("inner").unwrap();
        assert_eq!(inner.as_array().unwrap().len(), 2);
    }

    #[test]
    fn parse_whitespace_around_tokens() {
        let r = parse("  {  \"a\"  :  1  }  ").unwrap();
        assert_eq!(r.get("a").unwrap().as_number(), Some(1.0));
    }

    #[test]
    fn parse_trailing_garbage_rejected() {
        assert!(matches!(parse("1 2"), Err(JsonError::TrailingGarbage { .. })));
    }

    #[test]
    fn parse_unclosed_string() {
        assert!(parse(r#""hello"#).is_err());
    }

    #[test]
    fn parse_unclosed_array() {
        assert!(parse("[1, 2").is_err());
    }

    #[test]
    fn parse_invalid_literal() {
        assert!(parse("nulx").is_err());
    }

    #[test]
    fn parse_invalid_number() {
        assert!(parse("1.").is_err());
        assert!(parse("1e").is_err());
        assert!(parse("--1").is_err());
    }

    #[test]
    fn web_manifest_realistic_example() {
        let text = r##"{
            "name": "Example App",
            "short_name": "ExApp",
            "start_url": "/",
            "display": "standalone",
            "background_color": "#ffffff",
            "icons": [
                {"src": "/icon-192.png", "sizes": "192x192", "type": "image/png"}
            ]
        }"##;
        let r = parse(text).unwrap();
        assert_eq!(r.get("name").unwrap().as_str(), Some("Example App"));
        let icons = r.get("icons").unwrap().as_array().unwrap();
        assert_eq!(icons.len(), 1);
        assert_eq!(icons[0].get("src").unwrap().as_str(), Some("/icon-192.png"));
    }
}
