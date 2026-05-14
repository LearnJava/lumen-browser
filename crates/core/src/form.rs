//! Form data encoding — `application/x-www-form-urlencoded` и
//! `multipart/form-data`.
//!
//! Spec:
//! - URL-encoded: <https://url.spec.whatwg.org/#urlencoded-serializing>
//! - Multipart: RFC 7578 + WHATWG HTML §form-data-set.
//!
//! Phase 0: serializers. Реальный fetch с этими encoding-ами (Content-Type
//! заголовок, body upload) — отдельная задача в HttpClient.

use std::fmt::Write as _;

/// Запись формы — пара (name, value) с опциональным filename (для multipart).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormEntry {
    pub name: String,
    pub value: FormValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormValue {
    /// Текстовое значение (UTF-8).
    Text(String),
    /// Бинарный файл с MIME-типом и опц. filename.
    File {
        filename: String,
        content_type: String,
        bytes: Vec<u8>,
    },
}

impl FormEntry {
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: FormValue::Text(value.into()),
        }
    }

    pub fn file(
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            name: name.into(),
            value: FormValue::File {
                filename: filename.into(),
                content_type: content_type.into(),
                bytes,
            },
        }
    }
}

/// Сериализует form-set как `application/x-www-form-urlencoded`.
/// File entries трактуются как Text(filename) per WHATWG форма submit.
///
/// Encoding: `name=value&name=value&...`; пробелы → `+`; не-ASCII и
/// зарезервированные — percent-encoded в UTF-8.
pub fn encode_form_urlencoded(entries: &[FormEntry]) -> String {
    let mut out = String::new();
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        push_form_encoded(&mut out, &e.name);
        out.push('=');
        match &e.value {
            FormValue::Text(s) => push_form_encoded(&mut out, s),
            FormValue::File { filename, .. } => push_form_encoded(&mut out, filename),
        }
    }
    out
}

/// Percent-encoding для urlencoded form: пробел → `+`, остальное —
/// per WHATWG `application/x-www-form-urlencoded percent-encode set`:
/// всё кроме ALPHA/DIGIT/`*`/`-`/`.`/`_` percent-encoded.
fn push_form_encoded(out: &mut String, s: &str) {
    for b in s.bytes() {
        match b {
            b' ' => out.push('+'),
            b'*' | b'-' | b'.' | b'_' => out.push(b as char),
            b if b.is_ascii_alphanumeric() => out.push(b as char),
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
}

/// Decode urlencoded form value: `+` → пробел; `%HH` → байт. Не-валидные
/// последовательности (нечётный hex, неполный `%`) — возвращаются как есть.
/// Возвращает `String`; если bytes после decode не UTF-8 — lossy.
pub fn decode_form_value(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        if b == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Сериализует form-set как `multipart/form-data` (RFC 7578).
/// Возвращает (boundary, body bytes). Content-Type header должен быть
/// `multipart/form-data; boundary={boundary}`.
///
/// Boundary — фиксированный (для детерминизма). В production boundary
/// должен быть случайным; для Phase 0 текущая реализация достаточна.
pub fn encode_form_multipart(entries: &[FormEntry], boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for e in entries {
        out.extend_from_slice(b"--");
        out.extend_from_slice(boundary.as_bytes());
        out.extend_from_slice(b"\r\n");
        match &e.value {
            FormValue::Text(s) => {
                let header = format!(
                    "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                    escape_quoted_string(&e.name)
                );
                out.extend_from_slice(header.as_bytes());
                out.extend_from_slice(s.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
            FormValue::File {
                filename,
                content_type,
                bytes,
            } => {
                let header = format!(
                    "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                    escape_quoted_string(&e.name),
                    escape_quoted_string(filename),
                    content_type
                );
                out.extend_from_slice(header.as_bytes());
                out.extend_from_slice(bytes);
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"--\r\n");
    out
}

/// Escape для quoted-string в HTTP header (RFC 7578 §4.2 + RFC 2616).
/// `"` → `%22`, `\r`/`\n` → `%0D` / `%0A` per WHATWG HTML §form-data-set.
fn escape_quoted_string(s: &str) -> String {
    s.replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace('"', "%22")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── x-www-form-urlencoded ──

    #[test]
    fn urlencoded_simple_pair() {
        let s = encode_form_urlencoded(&[FormEntry::text("name", "value")]);
        assert_eq!(s, "name=value");
    }

    #[test]
    fn urlencoded_multiple_pairs() {
        let s = encode_form_urlencoded(&[
            FormEntry::text("a", "1"),
            FormEntry::text("b", "2"),
        ]);
        assert_eq!(s, "a=1&b=2");
    }

    #[test]
    fn urlencoded_space_to_plus() {
        let s = encode_form_urlencoded(&[FormEntry::text("k", "a b")]);
        assert_eq!(s, "k=a+b");
    }

    #[test]
    fn urlencoded_percent_for_reserved() {
        let s = encode_form_urlencoded(&[FormEntry::text("k", "a/b&c=d")]);
        assert_eq!(s, "k=a%2Fb%26c%3Dd");
    }

    #[test]
    fn urlencoded_cyrillic() {
        // "Привет" UTF-8: D0 9F D1 80 D0 B8 D0 B2 D0 B5 D1 82
        let s = encode_form_urlencoded(&[FormEntry::text("k", "Привет")]);
        assert_eq!(s, "k=%D0%9F%D1%80%D0%B8%D0%B2%D0%B5%D1%82");
    }

    #[test]
    fn urlencoded_safe_chars_unencoded() {
        // ALPHA, DIGIT, `*`, `-`, `.`, `_`.
        let s = encode_form_urlencoded(&[FormEntry::text("k", "abc-XYZ_123*.")]);
        assert_eq!(s, "k=abc-XYZ_123*.");
    }

    #[test]
    fn urlencoded_file_uses_filename() {
        let s = encode_form_urlencoded(&[FormEntry::file(
            "upload",
            "report.pdf",
            "application/pdf",
            vec![1, 2, 3],
        )]);
        assert_eq!(s, "upload=report.pdf");
    }

    // ── decode ──

    #[test]
    fn decode_basic() {
        assert_eq!(decode_form_value("a+b"), "a b");
        assert_eq!(decode_form_value("a%20b"), "a b");
        assert_eq!(decode_form_value("a%2Fb"), "a/b");
    }

    #[test]
    fn decode_cyrillic() {
        assert_eq!(decode_form_value("%D0%9F%D1%80%D0%B8%D0%B2%D0%B5%D1%82"), "Привет");
    }

    #[test]
    fn decode_invalid_percent_preserved() {
        // `%XY` — не hex, оставляется как есть.
        assert_eq!(decode_form_value("a%XYb"), "a%XYb");
    }

    #[test]
    fn decode_round_trip() {
        let original = "Привет, мир! a=b&c=d";
        let encoded = encode_form_urlencoded(&[FormEntry::text("k", original)]);
        let payload = encoded.split('=').nth(1).unwrap();
        assert_eq!(decode_form_value(payload), original);
    }

    // ── multipart/form-data ──

    #[test]
    fn multipart_text_field() {
        let body = encode_form_multipart(
            &[FormEntry::text("name", "value")],
            "----boundary123",
        );
        let s = String::from_utf8(body).unwrap();
        assert!(s.contains("------boundary123\r\n"));
        assert!(s.contains("Content-Disposition: form-data; name=\"name\""));
        assert!(s.contains("\r\n\r\nvalue\r\n"));
        assert!(s.ends_with("------boundary123--\r\n"));
    }

    #[test]
    fn multipart_file_field() {
        let body = encode_form_multipart(
            &[FormEntry::file(
                "upload",
                "test.txt",
                "text/plain",
                b"hello".to_vec(),
            )],
            "boundary",
        );
        let s = String::from_utf8(body).unwrap();
        assert!(s.contains("filename=\"test.txt\""));
        assert!(s.contains("Content-Type: text/plain"));
        assert!(s.contains("\r\n\r\nhello\r\n"));
    }

    #[test]
    fn multipart_mixed_text_and_file() {
        let body = encode_form_multipart(
            &[
                FormEntry::text("desc", "summary"),
                FormEntry::file("file", "x.bin", "application/octet-stream", vec![0, 1]),
            ],
            "b",
        );
        // Должно содержать оба, в порядке.
        let mut s = String::new();
        for b in &body {
            s.push(*b as char);
        }
        let desc_pos = s.find("desc").unwrap();
        let file_pos = s.find("filename").unwrap();
        assert!(desc_pos < file_pos);
    }

    #[test]
    fn multipart_escapes_quotes_in_filename() {
        let body = encode_form_multipart(
            &[FormEntry::file(
                "f",
                "ev\"il.txt",
                "text/plain",
                b"x".to_vec(),
            )],
            "b",
        );
        let s = String::from_utf8(body).unwrap();
        assert!(s.contains("filename=\"ev%22il.txt\""));
    }

    #[test]
    fn multipart_binary_content_preserved() {
        let body = encode_form_multipart(
            &[FormEntry::file(
                "f",
                "x.bin",
                "application/octet-stream",
                vec![0x00, 0xFF, 0xC0, 0xDE],
            )],
            "b",
        );
        // Проверяем что binary-байты сохранены.
        let needle = &[0x00u8, 0xFF, 0xC0, 0xDE];
        assert!(body.windows(4).any(|w| w == needle));
    }

    #[test]
    fn multipart_empty_entries_yields_closing_boundary_only() {
        let body = encode_form_multipart(&[], "b");
        assert_eq!(body, b"--b--\r\n");
    }
}
