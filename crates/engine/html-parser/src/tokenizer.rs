//! HTML-токенизатор (Phase 0 — минимальный набор состояний).
//!
//! Реализованы: Data, TagOpen, TagName, EndTag, Attribute (name/value
//! quoted/unquoted), SelfClosing, Comment, базовые character references
//! (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, `&#NNN;`, `&#xHH;`),
//! **text-only режимы** (HTML5 §13.2.5.2):
//!
//! - RAWTEXT для `<script>` и `<style>` — `<` и `&` буквальны, entities
//!   не декодируются;
//! - RCDATA для `<title>` и `<textarea>` — `<` буквален, но `&entity;`
//!   декодируются (это нужно, чтобы `<title>Foo &amp; Bar</title>`
//!   дало текст `Foo & Bar`).
//!
//! Оба режима завершаются только `</tag` + терминатор (case-insensitive).
//!
//! Отложено: DOCTYPE (пропускаем), CDATA, полный набор named entities
//! (есть ~2000+ в HTML5 spec; реализуем при первой реальной странице,
//! где это потребуется).

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
    /// `<!DOCTYPE name PUBLIC "public_id" "system_id">` или
    /// `<!DOCTYPE name SYSTEM "system_id">`. `name` хранится в lower-case
    /// (HTML5 §13.2.5.53 нормализует). `public_id`/`system_id` —
    /// `None` если соответствующий keyword отсутствует, `Some("")` если
    /// keyword присутствует с пустой строкой. Различие важно для
    /// quirks-detection: limited-quirks правила для HTML 4.01
    /// Frameset/Transitional зависят от наличия system_id, не его
    /// содержимого (HTML5 §13.2.5.1).
    Doctype {
        name: String,
        public_id: Option<String>,
        system_id: Option<String>,
    },
}

pub struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
    /// Если `Some((tag, decode_entities))` — токенизатор в text-only
    /// режиме до `</tag` (case-insensitive) + терминатор. Угловые скобки
    /// внутри всегда литеральны. `decode_entities = true` для RCDATA
    /// (`<title>`, `<textarea>`), `false` для RAWTEXT (`<script>`, `<style>`).
    text_only: Option<(String, bool)>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            text_only: None,
        }
    }

    /// Создаёт tokenizer с заранее заданным `text_only`-состоянием.
    /// Используется push-tokenizer-ом для возобновления токенизации
    /// между chunk-ами: если предыдущий chunk закончился внутри
    /// `<script>`/`<style>` (RAWTEXT) или `<title>`/`<textarea>` (RCDATA),
    /// следующий chunk начинается в том же режиме.
    pub fn with_state(input: &'a str, text_only: Option<(String, bool)>) -> Self {
        Self {
            input,
            pos: 0,
            text_only,
        }
    }

    /// Текущая позиция курсора (в байтах от начала `input`). Используется
    /// push-tokenizer-ом, чтобы понять, сколько байт уже потреблено.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Текущее `text_only`-состояние. После исчерпания iterator-а это
    /// финальное состояние — push-tokenizer переносит его в следующий chunk.
    pub fn text_only_state(&self) -> Option<&(String, bool)> {
        self.text_only.as_ref()
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
        // Text-only state: внутри <script>/<style> (RAWTEXT) или
        // <title>/<textarea> (RCDATA). `<` без `/tag` — текст; `&` —
        // декодируется только в RCDATA. Завершает режим `</tag` +
        // терминатор; сам `</tag>` потом токенизируется как обычный EndTag.
        //
        // ВАЖНО для push-режима: если в text_only loop мы упёрлись
        // в EOF (не найдя `</tag`), state нужно восстановить — следующий
        // вызов `next()` на дополненном вводе должен продолжить text_only,
        // а не переключиться в data state. Если же выход через break
        // на `</tag`, state остаётся очищенным (так и было), и data
        // state корректно разберёт `</tag>` как EndTag.
        if let Some((tag, decode_entities)) = self.text_only.take() {
            let mut text = String::new();
            let mut hit_terminator = false;
            while let Some(c) = self.peek() {
                if c == '<' && self.starts_with_end_tag(&tag) {
                    hit_terminator = true;
                    break;
                }
                if decode_entities && c == '&' {
                    self.consume();
                    if let Some(decoded) = self.try_consume_entity() {
                        text.push_str(&decoded);
                    } else {
                        text.push('&');
                    }
                } else {
                    self.consume();
                    text.push(c);
                }
            }
            if !hit_terminator {
                self.text_only = Some((tag, decode_entities));
            }
            if !text.is_empty() {
                return Some(Token::Text(text));
            }
            if !hit_terminator {
                // EOF в пустом text_only — больше ничего не вернём.
                return None;
            }
            // hit_terminator + text пустой → fall through в data state.
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
                // Comment <!-- ... -->, DOCTYPE, или прочее markup declaration.
                self.consume();
                if self.rest().starts_with("--") {
                    self.pos += 2;
                    self.consume_comment()
                } else if self.rest_starts_with_ascii_ci("doctype") {
                    self.pos += "doctype".len();
                    self.consume_doctype()
                } else {
                    // Неизвестное `<!...` — съесть до '>' и продолжить.
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

        if !self_closing {
            if is_raw_text_element(&name) {
                self.text_only = Some((name.clone(), false));
            } else if is_rcdata_element(&name) {
                self.text_only = Some((name.clone(), true));
            }
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

    /// HTML5 §13.2.5.53–72: после `<!DOCTYPE` парсим имя, опционально
    /// PUBLIC / SYSTEM identifiers (cases-insensitive), завершаем на '>' или
    /// EOF. Имя нормализуется в lower-case.
    ///
    /// Поддерживаются формы:
    ///   - `<!DOCTYPE html>` — самый частый, всё что нужно для HTML5;
    ///   - `<!DOCTYPE html PUBLIC "id" "url">` — XHTML-like;
    ///   - `<!DOCTYPE html SYSTEM "url">`.
    ///
    /// Невалидные / неполные DOCTYPE-ы парсятся «как есть» и не дают ошибки —
    /// tokenizer lenient.
    fn consume_doctype(&mut self) -> Option<Token> {
        // Согласно spec, после `DOCTYPE` ожидается whitespace; lenient режим
        // тоже принимает '>' сразу (даст пустой name).
        self.skip_whitespace();
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c == '>' || c.is_whitespace() {
                break;
            }
            self.consume();
            name.push(c.to_ascii_lowercase());
        }
        self.skip_whitespace();

        let mut public_id: Option<String> = None;
        let mut system_id: Option<String> = None;
        if self.rest_starts_with_ascii_ci("public") {
            self.pos += "public".len();
            self.skip_whitespace();
            public_id = Some(self.consume_quoted_string().unwrap_or_default());
            self.skip_whitespace();
            // После public_id может идти system_id (тоже quoted), а может и
            // ничего (lenient).
            if matches!(self.peek(), Some('"' | '\'')) {
                system_id = Some(self.consume_quoted_string().unwrap_or_default());
            }
        } else if self.rest_starts_with_ascii_ci("system") {
            self.pos += "system".len();
            self.skip_whitespace();
            system_id = Some(self.consume_quoted_string().unwrap_or_default());
        }

        // Съесть всё до '>' (на случай мусора / несколько идентификаторов).
        while let Some(c) = self.consume() {
            if c == '>' {
                break;
            }
        }

        Some(Token::Doctype {
            name,
            public_id,
            system_id,
        })
    }

    /// Проверяет, начинается ли `rest()` с указанной строки в ASCII
    /// case-insensitive манере. Используется для keyword-ов в DOCTYPE.
    fn rest_starts_with_ascii_ci(&self, needle: &str) -> bool {
        let r = self.rest().as_bytes();
        let n = needle.as_bytes();
        r.len() >= n.len() && r[..n.len()].eq_ignore_ascii_case(n)
    }

    /// Читает строку в кавычках (`"..."` или `'...'`). Возвращает содержимое
    /// без кавычек. Если стартовая кавычка не найдена — None.
    fn consume_quoted_string(&mut self) -> Option<String> {
        let q = match self.peek() {
            Some('"') => '"',
            Some('\'') => '\'',
            _ => return None,
        };
        self.consume();
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == q {
                self.consume();
                return Some(s);
            }
            // EOF внутри строки — отдаём что собрали (lenient).
            if c == '>' {
                return Some(s);
            }
            self.consume();
            s.push(c);
        }
        Some(s)
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

        // Named character reference — берём имя до `;` (включительно),
        // максимум 32 байта чтобы не зацикливаться на повреждённом входе.
        // HTML5 spec требует, чтобы имя кончалось `;`; legacy form без
        // `;` (~100 ссылок типа `&amp` для HTML 4 compat) пока не
        // поддерживается.
        let end = rest
            .bytes()
            .take(32)
            .position(|b| b == b';')
            .map(|p| p + 1);
        if let Some(name_len) = end {
            let name = &rest[..name_len];
            if let Some(decoded) = crate::entities::lookup_named_entity(name) {
                self.pos += name_len;
                return Some(decoded.to_string());
            }
        }
        None
    }
}

/// Элементы, чьё содержимое в HTML5 — RAWTEXT (литеральный текст до
/// `</tag` + терминатор; character references **не** декодируются).
fn is_raw_text_element(name: &str) -> bool {
    matches!(name, "script" | "style")
}

/// Элементы, чьё содержимое — RCDATA (литеральный текст до `</tag` +
/// терминатор; character references декодируются). Это нужно, чтобы
/// `<title>Foo &amp; Bar</title>` стало текстом `Foo & Bar`, и чтобы
/// внутри `<textarea>` HTML-like содержимое (например `<world>`)
/// не превращалось в реальные теги.
fn is_rcdata_element(name: &str) -> bool {
    matches!(name, "title" | "textarea")
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
    fn doctype_html5_basic() {
        let t = tok("<!DOCTYPE html>");
        assert_eq!(
            t[0],
            Token::Doctype {
                name: "html".into(),
                public_id: None,
                system_id: None,
            }
        );
    }

    #[test]
    fn doctype_followed_by_content() {
        let t = tok("<!DOCTYPE html><p>x</p>");
        assert!(matches!(&t[0], Token::Doctype { name, .. } if name == "html"));
        assert!(matches!(&t[1], Token::StartTag { name, .. } if name == "p"));
    }

    #[test]
    fn doctype_case_insensitive_keyword() {
        // `<!doctype html>` (lowercase keyword) тоже валиден.
        let t = tok("<!doctype HTML>");
        assert!(matches!(&t[0], Token::Doctype { name, .. } if name == "html"));
    }

    #[test]
    fn doctype_html4_strict_with_public() {
        let t = tok(
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd">"#,
        );
        assert_eq!(
            t[0],
            Token::Doctype {
                name: "html".into(),
                public_id: Some("-//W3C//DTD HTML 4.01//EN".into()),
                system_id: Some("http://www.w3.org/TR/html4/strict.dtd".into()),
            }
        );
    }

    #[test]
    fn doctype_with_system_only() {
        let t = tok(r#"<!DOCTYPE html SYSTEM "about:legacy-compat">"#);
        assert_eq!(
            t[0],
            Token::Doctype {
                name: "html".into(),
                public_id: None,
                system_id: Some("about:legacy-compat".into()),
            }
        );
    }

    #[test]
    fn doctype_single_quoted_strings() {
        let t = tok("<!DOCTYPE html PUBLIC 'pid' 'sid'>");
        assert_eq!(
            t[0],
            Token::Doctype {
                name: "html".into(),
                public_id: Some("pid".into()),
                system_id: Some("sid".into()),
            }
        );
    }

    #[test]
    fn doctype_extra_whitespace_tolerated() {
        let t = tok("<!DOCTYPE   html   >");
        assert!(matches!(&t[0], Token::Doctype { name, .. } if name == "html"));
    }

    #[test]
    fn doctype_empty_name_lenient() {
        // `<!DOCTYPE>` без имени — не валиден по spec, но lenient: пустой name.
        let t = tok("<!DOCTYPE>");
        assert_eq!(
            t[0],
            Token::Doctype {
                name: String::new(),
                public_id: None,
                system_id: None,
            }
        );
    }

    #[test]
    fn doctype_public_missing_vs_empty() {
        // `PUBLIC "" ""` — public/system_id присутствуют, но пустые.
        // Различимо от полного отсутствия (`<!DOCTYPE html>` → None).
        let t = tok(r#"<!DOCTYPE html PUBLIC "" "">"#);
        assert_eq!(
            t[0],
            Token::Doctype {
                name: "html".into(),
                public_id: Some(String::new()),
                system_id: Some(String::new()),
            }
        );
    }

    #[test]
    fn doctype_public_without_system() {
        // PUBLIC с одним id — system_id отсутствует (None, не Some("")).
        let t = tok(r#"<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01 Frameset//EN">"#);
        assert_eq!(
            t[0],
            Token::Doctype {
                name: "html".into(),
                public_id: Some("-//W3C//DTD HTML 4.01 Frameset//EN".into()),
                system_id: None,
            }
        );
    }

    #[test]
    fn doctype_unknown_markup_declaration_still_skipped() {
        // `<![CDATA[...]]>` или `<!ENTITY ...>` — не наш случай, должно
        // молча скушать до '>' и не дать DOCTYPE-токен.
        let t = tok("<![CDATA[ignore this]]><p>x</p>");
        // Первый токен должен быть от `<p>`, CDATA пропущена.
        assert!(matches!(&t[0], Token::StartTag { name, .. } if name == "p"));
    }

    #[test]
    fn entity_named() {
        assert_eq!(tok("&amp;"), vec![Token::Text("&".into())]);
        assert_eq!(tok("&lt;&gt;"), vec![Token::Text("<>".into())]);
        assert_eq!(tok("&quot;"), vec![Token::Text("\"".into())]);
        assert_eq!(tok("&nbsp;"), vec![Token::Text("\u{00A0}".into())]);
    }

    #[test]
    fn entity_extended_named() {
        // Расширенный набор из 250+ имён.
        assert_eq!(tok("&copy;"), vec![Token::Text("\u{00A9}".into())]);
        assert_eq!(tok("&mdash;"), vec![Token::Text("\u{2014}".into())]);
        assert_eq!(tok("&hellip;"), vec![Token::Text("\u{2026}".into())]);
        assert_eq!(tok("&trade;"), vec![Token::Text("\u{2122}".into())]);
        assert_eq!(tok("&euro;"), vec![Token::Text("\u{20AC}".into())]);
        // Greek
        assert_eq!(tok("&alpha;"), vec![Token::Text("\u{03B1}".into())]);
        assert_eq!(tok("&Omega;"), vec![Token::Text("\u{03A9}".into())]);
        // Arrows
        assert_eq!(tok("&rarr;"), vec![Token::Text("\u{2192}".into())]);
        // Quotes
        assert_eq!(
            tok("&ldquo;hello&rdquo;"),
            vec![Token::Text("\u{201C}hello\u{201D}".into())]
        );
    }

    #[test]
    fn entity_case_sensitive_per_html5() {
        // HTML5 имена case-sensitive: Beta != beta.
        assert_eq!(tok("&Beta;"), vec![Token::Text("\u{0392}".into())]);
        assert_eq!(tok("&beta;"), vec![Token::Text("\u{03B2}".into())]);
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

    // --- RCDATA mode для <title> и <textarea> ---

    #[test]
    fn title_entity_decoded() {
        // RCDATA отличается от RAWTEXT тем, что character references
        // декодируются. &amp; → &.
        let t = tok("<title>Foo &amp; Bar</title>");
        assert_eq!(t.len(), 3);
        assert!(matches!(t[0], Token::StartTag { ref name, .. } if name == "title"));
        assert_eq!(t[1], Token::Text("Foo & Bar".into()));
        assert!(matches!(t[2], Token::EndTag { ref name } if name == "title"));
    }

    #[test]
    fn title_less_than_is_text() {
        // `<` без `</title` — это литеральный текст в RCDATA.
        let t = tok("<title>x < y</title>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("x < y".into()));
    }

    #[test]
    fn title_numeric_entity_decoded() {
        // &#x41; → 'A'.
        let t = tok("<title>&#x41;&#1055;</title>");
        assert_eq!(t[1], Token::Text("AП".into()));
    }

    #[test]
    fn title_unknown_entity_left_literal() {
        // Неизвестный entity сохраняется как '&foo;' буквально.
        let t = tok("<title>&unknown;</title>");
        assert_eq!(t[1], Token::Text("&unknown;".into()));
    }

    #[test]
    fn title_inner_tag_is_text() {
        // <b> внутри <title> — литеральный текст, не StartTag.
        let t = tok("<title>Hello <b>world</b></title>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("Hello <b>world</b>".into()));
        assert!(matches!(t[2], Token::EndTag { ref name } if name == "title"));
    }

    #[test]
    fn title_case_insensitive_end_tag() {
        let t = tok("<title>x</TITLE>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("x".into()));
        assert!(matches!(t[2], Token::EndTag { ref name } if name == "title"));
    }

    #[test]
    fn title_fake_end_tag_not_matched() {
        // </titles> — `s` после `title` не является терминатором.
        let t = tok("<title>foo</titles>bar</title>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("foo</titles>bar".into()));
    }

    #[test]
    fn empty_title() {
        let t = tok("<title></title>");
        assert_eq!(t.len(), 2);
        assert!(matches!(t[0], Token::StartTag { ref name, .. } if name == "title"));
        assert!(matches!(t[1], Token::EndTag { ref name } if name == "title"));
    }

    #[test]
    fn title_then_normal_content() {
        // После </title> токенизатор возвращается в обычный режим.
        let t = tok("<title>x &amp; y</title><p>z</p>");
        assert_eq!(t[1], Token::Text("x & y".into()));
        assert!(matches!(t[3], Token::StartTag { ref name, .. } if name == "p"));
        assert_eq!(t[4], Token::Text("z".into()));
    }

    #[test]
    fn textarea_entity_decoded() {
        // <textarea> тоже RCDATA.
        let t = tok("<textarea>Hello &amp; goodbye</textarea>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("Hello & goodbye".into()));
    }

    #[test]
    fn textarea_inner_tag_is_text() {
        // Классический случай: <textarea> с HTML-like содержимым.
        let t = tok("<textarea>&lt;script&gt;alert(1)&lt;/script&gt;</textarea>");
        // Entities декодируются → текст = "<script>alert(1)</script>", но
        // это литеральный текст, не парсится как теги.
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("<script>alert(1)</script>".into()));
    }

    #[test]
    fn textarea_raw_open_angle_is_text() {
        // `<world>` внутри textarea — литерал, не тег.
        let t = tok("<textarea>Hello <world></textarea>");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], Token::Text("Hello <world>".into()));
    }

    #[test]
    fn textarea_cyrillic_entity() {
        // &#1055; = 'П'. RCDATA декодирует numeric entities в кириллицу.
        let t = tok("<textarea>Привет &#8212; &amp; мир</textarea>");
        assert_eq!(t[1], Token::Text("Привет — & мир".into()));
    }

    #[test]
    fn self_closing_title_does_not_enter_rcdata() {
        // <title/> self-closing — RCDATA НЕ включается, как и у RAWTEXT.
        let t = tok("<title/><b>x</b>");
        assert!(matches!(t[0], Token::StartTag { ref name, self_closing: true, .. } if name == "title"));
        assert!(matches!(t[1], Token::StartTag { ref name, .. } if name == "b"));
    }

    #[test]
    fn title_unclosed_at_eof() {
        // </title> отсутствует — текст до конца ввода с декодированием.
        let t = tok("<title>x &amp; y");
        assert_eq!(t.len(), 2);
        assert_eq!(t[1], Token::Text("x & y".into()));
    }
}
