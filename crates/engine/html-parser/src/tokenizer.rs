//! HTML-токенизатор (Phase 0 — минимальный набор состояний).
//!
//! Реализованы: Data, TagOpen, TagName, EndTag, Attribute (name/value
//! quoted/unquoted), SelfClosing, Comment, базовые character references
//! (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, `&#NNN;`, `&#xHH;`),
//! RAWTEXT для `<script>` и `<style>` (содержимое — литеральный текст,
//! завершается только `</tag` + терминатор; character references не
//! декодируются, угловые скобки трактуются как текст).
//!
//! Отложено: DOCTYPE (пропускаем), CDATA, RCDATA для `<title>`/`<textarea>`
//! (как RAWTEXT, но с декодированием entities), полный набор named entities
//! (есть ~2000+ в HTML5 spec; реализуем при первой реальной странице, где
//! это потребуется).

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
    /// Если `Some(tag)` — следующий вызов `next()` парсит содержимое
    /// как RAWTEXT до `</tag` (case-insensitive) + терминатор.
    raw_text: Option<String>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            raw_text: None,
        }
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
        // RAWTEXT state: внутри <script>/<style>. Читаем литеральный текст,
        // ни '<', ни '&' не имеют специального значения — кроме `</tag` с
        // терминатором, который выводит нас из режима. Сам `</tag>` потом
        // токенизируется обычным путём как EndTag.
        if let Some(tag) = self.raw_text.take() {
            let mut text = String::new();
            while let Some(c) = self.peek() {
                if c == '<' && self.starts_with_end_tag(&tag) {
                    break;
                }
                self.consume();
                text.push(c);
            }
            if !text.is_empty() {
                return Some(Token::Text(text));
            }
            // Текста не было — провалимся в обычный data state, где
            // следующий же символ '<' разберётся как закрывающий тег.
        }

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

        if !self_closing && is_raw_text_element(&name) {
            self.raw_text = Some(name.clone());
        }

        Some(Token::StartTag {
            name,
            attrs,
            self_closing,
        })
    }

    /// `rest` начинается с `</TAG` (case-insensitive), за которым идёт
    /// один из терминаторов: пробел/таб/перевод строки/`/`/`>` или EOF?
    fn starts_with_end_tag(&self, tag: &str) -> bool {
        let rest = self.rest().as_bytes();
        // `</`
        if rest.len() < 2 || rest[0] != b'<' || rest[1] != b'/' {
            return false;
        }
        let tag_bytes = tag.as_bytes();
        let after_slash = &rest[2..];
        if after_slash.len() < tag_bytes.len() {
            return false;
        }
        for (i, &t) in tag_bytes.iter().enumerate() {
            if after_slash[i].to_ascii_lowercase() != t {
                return false;
            }
        }
        match after_slash.get(tag_bytes.len()) {
            None => true,
            Some(&b) => matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | b'/' | b'>'),
        }
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

/// Элементы, чьё содержимое в HTML5 — RAWTEXT (литеральный текст до
/// `</tag` + терминатор; character references не декодируются).
/// `<title>` и `<textarea>` относятся к RCDATA — пока не реализовано.
fn is_raw_text_element(name: &str) -> bool {
    matches!(name, "script" | "style")
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

    // --- RAWTEXT mode для <script> и <style> ---

    #[test]
    fn script_with_html_content_is_text() {
        let t = tok("<script>var x = '<b>hi</b>';</script>");
        assert_eq!(t.len(), 3);
        assert!(matches!(t[0], Token::StartTag { ref name, .. } if name == "script"));
        assert_eq!(t[1], Token::Text("var x = '<b>hi</b>';".into()));
        assert!(matches!(t[2], Token::EndTag { ref name } if name == "script"));
    }

    #[test]
    fn script_with_less_than_operator() {
        let t = tok("<script>if (a < b) f();</script>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("if (a < b) f();".into()));
    }

    #[test]
    fn script_entity_kept_literal() {
        // RAWTEXT: character references НЕ декодируются.
        let t = tok("<script>x = '&amp;';</script>");
        assert_eq!(t[1], Token::Text("x = '&amp;';".into()));
    }

    #[test]
    fn script_end_tag_case_insensitive() {
        let t = tok("<script>x = 1;</SCRIPT>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("x = 1;".into()));
        assert!(matches!(t[2], Token::EndTag { ref name } if name == "script"));
    }

    #[test]
    fn script_end_tag_with_whitespace() {
        // </script  > — терминатор после имени допускает пробелы.
        let t = tok("<script>x = 1;</script  >");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("x = 1;".into()));
    }

    #[test]
    fn script_fake_end_tag_not_matched() {
        // </scripto> — 'o' после "script" не является терминатором (пробел/`/`/`>`).
        let t = tok("<script>foo </scripto> bar</script>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("foo </scripto> bar".into()));
    }

    #[test]
    fn script_lonely_open_angle_is_text() {
        // '<' без '/' внутри script — текст.
        let t = tok("<script>x = 5; y = '<';</script>");
        assert_eq!(t[1], Token::Text("x = 5; y = '<';".into()));
    }

    #[test]
    fn empty_script() {
        let t = tok("<script></script>");
        assert_eq!(t.len(), 2);
        assert!(matches!(t[0], Token::StartTag { ref name, .. } if name == "script"));
        assert!(matches!(t[1], Token::EndTag { ref name } if name == "script"));
    }

    #[test]
    fn unclosed_script_at_eof() {
        // </script отсутствует — текст до конца ввода.
        let t = tok("<script>x = 1");
        assert_eq!(t.len(), 2);
        assert_eq!(t[1], Token::Text("x = 1".into()));
    }

    #[test]
    fn style_with_braces_and_lt() {
        let t = tok("<style>p { color: red; } /* < */</style>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("p { color: red; } /* < */".into()));
        assert!(matches!(t[2], Token::EndTag { ref name } if name == "style"));
    }

    #[test]
    fn style_entity_kept_literal() {
        let t = tok("<style>p::before { content: '&amp;'; }</style>");
        assert_eq!(t[1], Token::Text("p::before { content: '&amp;'; }".into()));
    }

    #[test]
    fn script_with_nested_close_tag_in_string() {
        // Классическая ловушка: </script> внутри строки JS всё равно закрывает блок.
        // Это поведение HTML5 spec — пользователь должен писать <\/script>.
        let t = tok("<script>x = '</script>';</script>");
        assert_eq!(t.len(), 5);
        assert_eq!(t[1], Token::Text("x = '".into()));
        assert!(matches!(t[2], Token::EndTag { ref name } if name == "script"));
        assert_eq!(t[3], Token::Text("';".into()));
        assert!(matches!(t[4], Token::EndTag { ref name } if name == "script"));
    }

    #[test]
    fn script_with_attributes_still_enters_rawtext() {
        let t = tok(r#"<script type="text/javascript">if (1 < 2) {}</script>"#);
        match &t[0] {
            Token::StartTag { name, attrs, .. } => {
                assert_eq!(name, "script");
                assert_eq!(attrs.len(), 1);
            }
            _ => panic!(),
        }
        assert_eq!(t[1], Token::Text("if (1 < 2) {}".into()));
    }

    #[test]
    fn self_closing_script_does_not_enter_rawtext() {
        // <script/> с self-closing → следующий текст НЕ внутри script-блока.
        // Lenient: tree builder может всё равно открыть script, но токенизатор
        // не должен переключаться в RAWTEXT при явно self-closing.
        let t = tok("<script/><b>x</b>");
        assert!(matches!(t[0], Token::StartTag { ref name, self_closing: true, .. } if name == "script"));
        assert!(matches!(t[1], Token::StartTag { ref name, .. } if name == "b"));
    }
}
