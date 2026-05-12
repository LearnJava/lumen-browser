//! HTML-токенизатор (Phase 0 — минимальный набор состояний).
//!
//! Реализованы: Data, TagOpen, TagName, EndTag, Attribute (name/value
//! quoted/unquoted), SelfClosing, Comment, базовые character references
//! (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, `&#NNN;`, `&#xHH;`).
//!
//! Отложено: DOCTYPE (пропускаем), CDATA, raw-text script/style, полный
//! набор named entities (есть ~2000+ в HTML5 spec; реализуем при первой
//! реальной странице, где это потребуется).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    StartTag {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Text(String),
    Comment(String),
}

pub struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn consume(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.consume();
            } else {
                break;
            }
        }
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        if self.pos >= self.input.len() {
            return None;
        }

        // Data state: текст до следующего '<' или '&'.
        let mut text = String::new();
        while let Some(c) = self.peek() {
            match c {
                '<' => break,
                '&' => {
                    self.consume();
                    if let Some(decoded) = self.try_consume_entity() {
                        text.push_str(&decoded);
                    } else {
                        text.push('&');
                    }
                }
                _ => {
                    self.consume();
                    text.push(c);
                }
            }
        }

        if !text.is_empty() {
            return Some(Token::Text(text));
        }

        // Стоим на '<' или EOF.
        if self.peek() != Some('<') {
            return None;
        }
        self.consume(); // '<'

        match self.peek() {
            Some('/') => {
                self.consume();
                self.consume_end_tag()
            }
            Some('!') => {
                // Comment <!-- ... --> или DOCTYPE (пропускаем).
                self.consume();
                if self.rest().starts_with("--") {
                    self.pos += 2;
                    self.consume_comment()
                } else {
                    // DOCTYPE / прочее объявление — съесть до '>'.
                    while let Some(c) = self.consume() {
                        if c == '>' {
                            break;
                        }
                    }
                    self.next()
                }
            }
            Some(c) if c.is_ascii_alphabetic() => self.consume_start_tag(),
            _ => {
                // Битый '<' — отдаём как текст.
                Some(Token::Text("<".to_string()))
            }
        }
    }
}

impl<'a> Tokenizer<'a> {
    fn consume_tag_name(&mut self) -> String {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' {
                self.consume();
                name.push(c.to_ascii_lowercase());
            } else {
                break;
            }
        }
        name
    }

    fn consume_start_tag(&mut self) -> Option<Token> {
        let name = self.consume_tag_name();
        let mut attrs = Vec::new();
        let mut self_closing = false;

        loop {
            self.skip_whitespace();
            match self.peek() {
                None => break,
                Some('>') => {
                    self.consume();
                    break;
                }
                Some('/') => {
                    self.consume();
                    if self.peek() == Some('>') {
                        self.consume();
                        self_closing = true;
                        break;
                    }
                    // одиночный '/' — игнорируем, идём дальше
                }
                Some(_) => {
                    if let Some((k, v)) = self.consume_attribute() {
                        attrs.push((k, v));
                    }
                }
            }
        }

        Some(Token::StartTag {
            name,
            attrs,
            self_closing,
        })
    }

    fn consume_attribute(&mut self) -> Option<(String, String)> {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() || c == '=' || c == '>' || c == '/' {
                break;
            }
            self.consume();
            name.push(c.to_ascii_lowercase());
        }
        if name.is_empty() {
            // Не получилось разобрать — сдвинемся на 1 символ, чтобы не зациклиться.
            self.consume();
            return None;
        }

        self.skip_whitespace();

        let value = if self.peek() == Some('=') {
            self.consume();
            self.skip_whitespace();
            self.consume_attribute_value()
        } else {
            String::new()
        };

        Some((name, value))
    }

    fn consume_attribute_value(&mut self) -> String {
        let mut value = String::new();
        let quote = match self.peek() {
            Some('"') => {
                self.consume();
                Some('"')
            }
            Some('\'') => {
                self.consume();
                Some('\'')
            }
            _ => None,
        };

        while let Some(c) = self.peek() {
            match (quote, c) {
                (Some(q), c) if c == q => {
                    self.consume();
                    break;
                }
                (None, c) if c.is_ascii_whitespace() || c == '>' => break,
                (_, '&') => {
                    self.consume();
                    if let Some(decoded) = self.try_consume_entity() {
                        value.push_str(&decoded);
                    } else {
                        value.push('&');
                    }
                }
                _ => {
                    self.consume();
                    value.push(c);
                }
            }
        }
        value
    }

    fn consume_end_tag(&mut self) -> Option<Token> {
        let name = self.consume_tag_name();
        while let Some(c) = self.consume() {
            if c == '>' {
                break;
            }
        }
        Some(Token::EndTag { name })
    }

    fn consume_comment(&mut self) -> Option<Token> {
        let mut content = String::new();
        loop {
            if self.rest().starts_with("-->") {
                self.pos += 3;
                return Some(Token::Comment(content));
            }
            match self.consume() {
                Some(c) => content.push(c),
                None => return Some(Token::Comment(content)),
            }
        }
    }

    /// Пробует распарсить character reference после уже потреблённого `&`.
    /// При успехе возвращает декодированную строку, иначе None (caller
    /// эмитит `&` буквально).
    fn try_consume_entity(&mut self) -> Option<String> {
        let rest = self.rest();

        if let Some(after_hash) = rest.strip_prefix('#') {
            let (digits, base, prefix_len) = if let Some(after_x) =
                after_hash.strip_prefix(|c: char| c == 'x' || c == 'X')
            {
                (after_x, 16u32, 2)
            } else {
                (after_hash, 10u32, 1)
            };

            let end = digits
                .bytes()
                .take_while(|b| {
                    if base == 16 {
                        b.is_ascii_hexdigit()
                    } else {
                        b.is_ascii_digit()
                    }
                })
                .count();
            if end == 0 {
                return None;
            }
            let code = u32::from_str_radix(&digits[..end], base).ok()?;
            let ch = char::from_u32(code)?;

            self.pos += prefix_len + end;
            if self.peek() == Some(';') {
                self.consume();
            }
            return Some(ch.to_string());
        }

        const ENTITIES: &[(&str, &str)] = &[
            ("amp;", "&"),
            ("lt;", "<"),
            ("gt;", ">"),
            ("quot;", "\""),
            ("apos;", "'"),
            ("nbsp;", "\u{00A0}"),
        ];
        for (name, val) in ENTITIES {
            if rest.starts_with(name) {
                self.pos += name.len();
                return Some((*val).to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(input: &str) -> Vec<Token> {
        Tokenizer::new(input).collect()
    }

    #[test]
    fn empty_input() {
        assert!(tok("").is_empty());
    }

    #[test]
    fn plain_text() {
        assert_eq!(tok("hello"), vec![Token::Text("hello".into())]);
    }

    #[test]
    fn simple_tag() {
        assert_eq!(
            tok("<p>hello</p>"),
            vec![
                Token::StartTag {
                    name: "p".into(),
                    attrs: vec![],
                    self_closing: false
                },
                Token::Text("hello".into()),
                Token::EndTag { name: "p".into() },
            ]
        );
    }

    #[test]
    fn tag_name_lowercased() {
        let t = tok("<DIV></DIV>");
        match &t[0] {
            Token::StartTag { name, .. } => assert_eq!(name, "div"),
            _ => panic!(),
        }
    }

    #[test]
    fn attribute_double_quoted() {
        let t = tok(r#"<a href="https://example.com">x</a>"#);
        match &t[0] {
            Token::StartTag { name, attrs, .. } => {
                assert_eq!(name, "a");
                assert_eq!(
                    attrs,
                    &vec![("href".into(), "https://example.com".into())]
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn attribute_single_quoted() {
        let t = tok("<a href='x'>y</a>");
        match &t[0] {
            Token::StartTag { attrs, .. } => {
                assert_eq!(attrs, &vec![("href".into(), "x".into())]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn attribute_unquoted() {
        let t = tok("<a href=foo>y</a>");
        match &t[0] {
            Token::StartTag { attrs, .. } => {
                assert_eq!(attrs, &vec![("href".into(), "foo".into())]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn attribute_no_value() {
        let t = tok("<input disabled>");
        match &t[0] {
            Token::StartTag { attrs, .. } => {
                assert_eq!(attrs, &vec![("disabled".into(), "".into())]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn multiple_attributes() {
        let t = tok(r#"<a href="x" class="y" id='z'>w</a>"#);
        match &t[0] {
            Token::StartTag { attrs, .. } => {
                assert_eq!(attrs.len(), 3);
                assert_eq!(attrs[0], ("href".into(), "x".into()));
                assert_eq!(attrs[1], ("class".into(), "y".into()));
                assert_eq!(attrs[2], ("id".into(), "z".into()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn self_closing() {
        let t = tok("<br/>");
        assert_eq!(
            t[0],
            Token::StartTag {
                name: "br".into(),
                attrs: vec![],
                self_closing: true
            }
        );
    }

    #[test]
    fn comment() {
        let t = tok("<!-- skip me -->");
        assert_eq!(t[0], Token::Comment(" skip me ".into()));
    }

    #[test]
    fn doctype_skipped() {
        let t = tok("<!DOCTYPE html><p>x</p>");
        assert!(matches!(t[0], Token::StartTag { .. }));
    }

    #[test]
    fn entity_named() {
        assert_eq!(tok("&amp;"), vec![Token::Text("&".into())]);
        assert_eq!(tok("&lt;&gt;"), vec![Token::Text("<>".into())]);
        assert_eq!(tok("&quot;"), vec![Token::Text("\"".into())]);
        assert_eq!(tok("&nbsp;"), vec![Token::Text("\u{00A0}".into())]);
    }

    #[test]
    fn entity_decimal() {
        assert_eq!(tok("&#1055;&#1088;&#1080;"), vec![Token::Text("При".into())]);
    }

    #[test]
    fn entity_hex() {
        assert_eq!(tok("&#x41;"), vec![Token::Text("A".into())]);
        assert_eq!(tok("&#x42F;"), vec![Token::Text("Я".into())]);
    }

    #[test]
    fn entity_unknown_kept_literal() {
        assert_eq!(tok("&foo;"), vec![Token::Text("&foo;".into())]);
    }

    #[test]
    fn cyrillic_text() {
        let t = tok("<p>Привет, мир</p>");
        assert_eq!(t[1], Token::Text("Привет, мир".into()));
    }

    #[test]
    fn cyrillic_attribute_value() {
        let t = tok(r#"<a title="Привет">x</a>"#);
        match &t[0] {
            Token::StartTag { attrs, .. } => {
                assert_eq!(attrs[0].1, "Привет");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn nested_structure() {
        let t = tok("<html><body><h1>Hello</h1></body></html>");
        assert_eq!(t.len(), 7); // 3 start, 1 text, 3 end
        assert!(matches!(t[0], Token::StartTag { ref name, .. } if name == "html"));
        assert_eq!(t[3], Token::Text("Hello".into()));
        assert!(matches!(t[6], Token::EndTag { ref name } if name == "html"));
    }

    #[test]
    fn entity_in_attribute_value() {
        let t = tok(r#"<a title="&lt;ok&gt;">x</a>"#);
        match &t[0] {
            Token::StartTag { attrs, .. } => {
                assert_eq!(attrs[0].1, "<ok>");
            }
            _ => panic!(),
        }
    }
}
