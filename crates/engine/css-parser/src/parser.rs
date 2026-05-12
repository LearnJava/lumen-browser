//! Минимальный CSS-парсер (Phase 0).
//!
//! Поддерживается: правила вида `selector_list { decl_list }`, селекторы
//! type / class / id / universal, комментарии `/* */`, комбинированные
//! селекторы через `,`, опциональный trailing `;`. At-rules (`@media`,
//! `@import`, ...) пропускаются — это lenient режим до фазы со стилевым
//! каскадом.
//!
//! Значения деклараций хранятся как сырые строки. Типизация значений
//! (length, color, calc, …) появится вместе со style cascade в §6.4.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Type(String),
    Class(String),
    Id(String),
    Universal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

pub fn parse(input: &str) -> Stylesheet {
    Parser::new(input).parse_stylesheet()
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
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

    fn skip_ws_and_comments(&mut self) {
        loop {
            while let Some(c) = self.peek() {
                if c.is_whitespace() {
                    self.consume();
                } else {
                    break;
                }
            }
            if self.rest().starts_with("/*") {
                self.pos += 2;
                while !self.rest().starts_with("*/") && self.pos < self.input.len() {
                    self.consume();
                }
                if self.rest().starts_with("*/") {
                    self.pos += 2;
                }
            } else {
                break;
            }
        }
    }

    fn parse_stylesheet(&mut self) -> Stylesheet {
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('@') => self.skip_at_rule(),
                Some(_) => {
                    if let Some(rule) = self.parse_rule() {
                        rules.push(rule);
                    } else {
                        break;
                    }
                }
            }
        }
        Stylesheet { rules }
    }

    fn skip_at_rule(&mut self) {
        self.consume(); // '@'
        while let Some(c) = self.peek() {
            match c {
                ';' => {
                    self.consume();
                    return;
                }
                '{' => {
                    self.consume();
                    self.skip_block();
                    return;
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn skip_block(&mut self) {
        let mut depth = 1;
        while let Some(c) = self.peek() {
            match c {
                '{' => {
                    self.consume();
                    depth += 1;
                }
                '}' => {
                    self.consume();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_rule(&mut self) -> Option<Rule> {
        let start = self.pos;
        let selectors = self.parse_selector_list();
        self.skip_ws_and_comments();
        if selectors.is_empty() || self.peek() != Some('{') {
            // Bail из текущего «правила» — сдвинемся до следующего блока
            // или конца, чтобы не зациклиться на мусоре.
            if self.pos == start {
                self.consume();
            }
            self.skip_at_rule();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();
        Some(Rule {
            selectors,
            declarations,
        })
    }

    fn parse_selector_list(&mut self) -> Vec<Selector> {
        let mut sels = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.parse_simple_selector() {
                Some(s) => sels.push(s),
                None => break,
            }
            self.skip_ws_and_comments();
            if self.peek() == Some(',') {
                self.consume();
                continue;
            }
            break;
        }
        sels
    }

    fn parse_simple_selector(&mut self) -> Option<Selector> {
        match self.peek()? {
            '*' => {
                self.consume();
                Some(Selector::Universal)
            }
            '.' => {
                self.consume();
                Some(Selector::Class(self.parse_ident()?))
            }
            '#' => {
                self.consume();
                Some(Selector::Id(self.parse_ident()?))
            }
            c if is_ident_start(c) => Some(Selector::Type(self.parse_ident()?)),
            _ => None,
        }
    }

    fn parse_ident(&mut self) -> Option<String> {
        let first = self.peek()?;
        if !is_ident_start(first) {
            return None;
        }
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.consume();
                s.push(c);
            } else {
                break;
            }
        }
        Some(s)
    }

    fn parse_declaration_block(&mut self) -> Vec<Declaration> {
        let mut decls = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some(';') => {
                    self.consume();
                    continue;
                }
                _ => match self.parse_declaration() {
                    Some(d) => decls.push(d),
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        decls
    }

    fn recover_to_decl_boundary(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                ';' => {
                    self.consume();
                    return;
                }
                '}' => return,
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_declaration(&mut self) -> Option<Declaration> {
        self.skip_ws_and_comments();
        let property = self.parse_ident()?;
        self.skip_ws_and_comments();
        if self.peek() != Some(':') {
            return None;
        }
        self.consume();
        let value = self.parse_value_until_terminator();
        Some(Declaration {
            property,
            value: value.trim().to_string(),
        })
    }

    fn parse_value_until_terminator(&mut self) -> String {
        let mut s = String::new();
        let mut in_string: Option<char> = None;
        while let Some(c) = self.peek() {
            match (in_string, c) {
                (None, ';') | (None, '}') => break,
                (Some(q), c) if c == q => {
                    self.consume();
                    s.push(c);
                    in_string = None;
                }
                (None, '"') | (None, '\'') => {
                    self.consume();
                    s.push(c);
                    in_string = Some(c);
                }
                _ => {
                    self.consume();
                    s.push(c);
                }
            }
        }
        s
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '-' || c >= '\u{00A0}'
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        assert_eq!(parse(""), Stylesheet::default());
    }

    #[test]
    fn whitespace_and_comment_only() {
        assert_eq!(parse("  /* hi */  "), Stylesheet::default());
    }

    #[test]
    fn single_rule() {
        let s = parse("p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selectors, vec![Selector::Type("p".into())]);
        assert_eq!(s.rules[0].declarations.len(), 1);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn class_selector() {
        let s = parse(".foo { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![Selector::Class("foo".into())]);
    }

    #[test]
    fn id_selector() {
        let s = parse("#bar { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![Selector::Id("bar".into())]);
    }

    #[test]
    fn universal_selector() {
        let s = parse("* { box-sizing: border-box; }");
        assert_eq!(s.rules[0].selectors, vec![Selector::Universal]);
    }

    #[test]
    fn multiple_selectors() {
        let s = parse("p, h1, h2 { color: red; }");
        assert_eq!(
            s.rules[0].selectors,
            vec![
                Selector::Type("p".into()),
                Selector::Type("h1".into()),
                Selector::Type("h2".into()),
            ]
        );
    }

    #[test]
    fn multiple_declarations() {
        let s = parse("p { color: red; font-size: 14px; margin: 0; }");
        assert_eq!(s.rules[0].declarations.len(), 3);
        assert_eq!(s.rules[0].declarations[1].property, "font-size");
        assert_eq!(s.rules[0].declarations[1].value, "14px");
    }

    #[test]
    fn trailing_semicolon_optional() {
        let with = parse("p { color: red; }");
        let without = parse("p { color: red }");
        assert_eq!(with, without);
    }

    #[test]
    fn empty_rule() {
        let s = parse("p {}");
        assert_eq!(s.rules.len(), 1);
        assert!(s.rules[0].declarations.is_empty());
    }

    #[test]
    fn multiple_rules() {
        let s = parse("p { color: red; } h1 { font-size: 24px; }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].declarations[0].property, "font-size");
    }

    #[test]
    fn comments_between_and_within() {
        let s = parse("/* one */ p /* hmm */ { /* x */ color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn at_import_skipped() {
        let s = parse("@import \"foo.css\"; p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selectors[0], Selector::Type("p".into()));
    }

    #[test]
    fn at_media_block_skipped() {
        let s = parse("@media print { p { color: black; } } p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn cyrillic_class_selector() {
        let s = parse(".привет { color: red; }");
        assert_eq!(
            s.rules[0].selectors,
            vec![Selector::Class("привет".into())]
        );
    }

    #[test]
    fn cyrillic_value_with_quotes() {
        let s = parse(r#"p { font-family: "Иваново", sans-serif; }"#);
        assert_eq!(
            s.rules[0].declarations[0].value,
            r#""Иваново", sans-serif"#
        );
    }

    #[test]
    fn malformed_declaration_skipped() {
        // Битая декларация в середине — не должна валить остальные.
        let s = parse("p { color: red; broken; font-size: 14px; }");
        assert_eq!(s.rules[0].declarations.len(), 2);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[1].property, "font-size");
    }

    #[test]
    fn negative_and_complex_values() {
        let s = parse("p { margin: -10px; background: url(\"a.png\"); }");
        assert_eq!(s.rules[0].declarations[0].value, "-10px");
        assert_eq!(s.rules[0].declarations[1].value, "url(\"a.png\")");
    }

    #[test]
    fn important_kept_in_value() {
        let s = parse("p { color: red !important; }");
        assert_eq!(s.rules[0].declarations[0].value, "red !important");
    }

    #[test]
    fn vendor_prefix_property() {
        let s = parse("p { -webkit-user-select: none; }");
        assert_eq!(s.rules[0].declarations[0].property, "-webkit-user-select");
    }
}
