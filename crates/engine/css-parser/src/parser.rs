//! CSS-парсер (Phase 0+).
//!
//! Поддерживается:
//!   - правила `selector_list { decl_list }`;
//!   - simple selectors: type / class / id / universal / attribute / pseudo-class;
//!   - compound selectors (`p.foo#bar:first-child`);
//!   - complex selectors с combinator-ами: descendant ` `, child `>`,
//!     next-sibling `+`, later-sibling `~`;
//!   - attribute selectors `[name]`, `[name=val]`, `[name~=val]`, `[name|=val]`,
//!     `[name^=val]`, `[name$=val]`, `[name*=val]`;
//!   - базовые pseudo-classes (`:first-child`, `:last-child`, `:only-child`,
//!     `:empty`, `:root`); неизвестные / interactive (`:hover`, `:focus`, …)
//!     сохраняются как `PseudoClass::Unsupported(name)` и при матчинге всегда
//!     возвращают `false`;
//!   - pseudo-elements `::name` парсятся отдельным узлом, никогда не матчат
//!     (т.к. в DOM им ничего не соответствует);
//!   - комментарии `/* */`, перечисление селекторов через `,`, опциональный
//!     trailing `;`. At-rules (`@media`, `@import`) пропускаются.
//!
//! Не поддерживается (отложено): функциональные pseudo (`:nth-child(2n+1)`,
//! `:not(...)`), `case-insensitive` модификатор `[attr=val i]`, namespace
//! prefix в селекторах, значения деклараций типизированно (length / color /
//! calc) — значения хранятся как сырые строки.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    Type(String),
    Class(String),
    Id(String),
    Universal,
    Attribute(AttrSelector),
    PseudoClass(PseudoClass),
    /// `::before`, `::after` и т.д. В Phase 0 никогда не матчит — DOM-узла нет.
    PseudoElement(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrSelector {
    pub name: String,
    pub op: Option<AttrOp>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `=` — точное совпадение.
    Equals,
    /// `~=` — значение содержит whitespace-разделённое слово.
    Includes,
    /// `|=` — точное совпадение или префикс с `-` (для `lang="ru-RU"`).
    DashMatch,
    /// `^=` — префикс.
    Prefix,
    /// `$=` — суффикс.
    Suffix,
    /// `*=` — подстрока.
    Substring,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoClass {
    FirstChild,
    LastChild,
    OnlyChild,
    Empty,
    Root,
    /// `:hover`, `:focus`, `:active`, и т.п. — парсятся, но в Phase 0 никогда
    /// не матчат (нет интерактивного состояния). Хранится имя для отладки.
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSelector {
    pub parts: Vec<SimpleSelector>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    /// Пробел между compound-ами: `a b` — `b` потомок `a`.
    Descendant,
    /// `>` — прямой ребёнок.
    Child,
    /// `+` — следующий sibling.
    NextSibling,
    /// `~` — любой последующий sibling.
    LaterSibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexSelector {
    /// Левый compound. Например, в `a b > c`: head = `a`,
    /// tail = `[(Descendant, b), (Child, c)]`.
    pub head: CompoundSelector,
    pub tail: Vec<(Combinator, CompoundSelector)>,
}

impl ComplexSelector {
    /// Specificity по CSS Selectors Level 3 §16:
    /// - `a` — число `#id`-частей;
    /// - `b` — число классов, attribute-селекторов и pseudo-classes;
    /// - `c` — число type-селекторов и pseudo-elements.
    ///
    /// Universal `*` и combinator-ы не считаются.
    pub fn specificity(&self) -> Specificity {
        let mut spec = Specificity::default();
        accumulate_specificity(&self.head, &mut spec);
        for (_, comp) in &self.tail {
            accumulate_specificity(comp, &mut spec);
        }
        spec
    }
}

fn accumulate_specificity(comp: &CompoundSelector, spec: &mut Specificity) {
    for part in &comp.parts {
        match part {
            SimpleSelector::Id(_) => spec.a = spec.a.saturating_add(1),
            SimpleSelector::Class(_)
            | SimpleSelector::Attribute(_)
            | SimpleSelector::PseudoClass(_) => spec.b = spec.b.saturating_add(1),
            SimpleSelector::Type(_) | SimpleSelector::PseudoElement(_) => {
                spec.c = spec.c.saturating_add(1);
            }
            SimpleSelector::Universal => {}
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Specificity {
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

impl Ord for Specificity {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.a, self.b, self.c).cmp(&(other.a, other.b, other.c))
    }
}

impl PartialOrd for Specificity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<ComplexSelector>,
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

    /// Возвращает true, если был whitespace или comment, и продвигает позицию.
    fn skip_ws_and_comments_track(&mut self) -> bool {
        let start = self.pos;
        self.skip_ws_and_comments();
        self.pos != start
    }

    fn parse_stylesheet(&mut self) -> Stylesheet {
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('@') => self.skip_at_rule(),
                Some(_) => {
                    let before = self.pos;
                    if let Some(rule) = self.parse_rule() {
                        rules.push(rule);
                    } else if self.pos == before {
                        // Защита от бесконечного цикла: parse_rule не сдвинул
                        // позицию — принудительно проглатываем один символ.
                        self.consume();
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
            // Не удалось разобрать селекторы — пропустим до конца блока,
            // чтобы не зациклиться.
            if self.pos == start {
                self.consume();
            }
            self.recover_to_block_end();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();
        Some(Rule {
            selectors,
            declarations,
        })
    }

    fn recover_to_block_end(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                '{' => {
                    self.consume();
                    self.skip_block();
                    return;
                }
                ';' => {
                    self.consume();
                    return;
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_selector_list(&mut self) -> Vec<ComplexSelector> {
        let mut sels = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.parse_complex_selector() {
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

    fn parse_complex_selector(&mut self) -> Option<ComplexSelector> {
        let head = self.parse_compound_selector()?;
        let mut tail = Vec::new();
        loop {
            // Между compound-ами может быть whitespace + явный combinator,
            // либо просто whitespace (descendant), либо ничего (значит конец).
            let had_ws = self.skip_ws_and_comments_track();
            match self.peek() {
                None | Some(',') | Some('{') | Some('}') => break,
                Some('>') => {
                    self.consume();
                    self.skip_ws_and_comments();
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::Child, comp));
                }
                Some('+') => {
                    self.consume();
                    self.skip_ws_and_comments();
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::NextSibling, comp));
                }
                Some('~') => {
                    self.consume();
                    self.skip_ws_and_comments();
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::LaterSibling, comp));
                }
                Some(_) if had_ws => {
                    let comp = self.parse_compound_selector()?;
                    tail.push((Combinator::Descendant, comp));
                }
                Some(_) => break,
            }
        }
        Some(ComplexSelector { head, tail })
    }

    fn parse_compound_selector(&mut self) -> Option<CompoundSelector> {
        let mut parts = Vec::new();
        while let Some(part) = self.parse_simple_selector() {
            parts.push(part);
        }
        if parts.is_empty() {
            None
        } else {
            Some(CompoundSelector { parts })
        }
    }

    fn parse_simple_selector(&mut self) -> Option<SimpleSelector> {
        match self.peek()? {
            '*' => {
                self.consume();
                Some(SimpleSelector::Universal)
            }
            '.' => {
                self.consume();
                Some(SimpleSelector::Class(self.parse_ident()?))
            }
            '#' => {
                self.consume();
                Some(SimpleSelector::Id(self.parse_ident()?))
            }
            '[' => self.parse_attr_selector(),
            ':' => self.parse_pseudo(),
            c if is_ident_start(c) => Some(SimpleSelector::Type(self.parse_ident()?)),
            _ => None,
        }
    }

    fn parse_attr_selector(&mut self) -> Option<SimpleSelector> {
        self.consume(); // '['
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        self.skip_ws_and_comments();
        let op = match self.peek()? {
            ']' => {
                self.consume();
                return Some(SimpleSelector::Attribute(AttrSelector {
                    name,
                    op: None,
                    value: None,
                }));
            }
            '=' => {
                self.consume();
                AttrOp::Equals
            }
            '~' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Includes
            }
            '|' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::DashMatch
            }
            '^' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Prefix
            }
            '$' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Suffix
            }
            '*' => {
                self.consume();
                if self.peek() != Some('=') {
                    self.recover_to_attr_end();
                    return None;
                }
                self.consume();
                AttrOp::Substring
            }
            _ => {
                self.recover_to_attr_end();
                return None;
            }
        };
        self.skip_ws_and_comments();
        let value = self.parse_attr_value()?;
        self.skip_ws_and_comments();
        if self.peek() != Some(']') {
            self.recover_to_attr_end();
            return None;
        }
        self.consume(); // ']'
        Some(SimpleSelector::Attribute(AttrSelector {
            name,
            op: Some(op),
            value: Some(value),
        }))
    }

    fn parse_attr_value(&mut self) -> Option<String> {
        match self.peek()? {
            q @ ('"' | '\'') => {
                self.consume();
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c == q {
                        self.consume();
                        return Some(s);
                    }
                    self.consume();
                    s.push(c);
                }
                None
            }
            _ => self.parse_ident(),
        }
    }

    fn recover_to_attr_end(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                ']' => {
                    self.consume();
                    return;
                }
                '{' | '}' | ';' => return,
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_pseudo(&mut self) -> Option<SimpleSelector> {
        self.consume(); // ':'
        let is_element = if self.peek() == Some(':') {
            self.consume();
            true
        } else {
            false
        };
        let name = self.parse_ident()?;
        // Функциональные pseudo (например `:nth-child(2n+1)`) — не поддерживаем,
        // но грамматику не ломаем: проглатываем скобки.
        if self.peek() == Some('(') {
            self.consume();
            let mut depth = 1;
            while let Some(c) = self.peek() {
                self.consume();
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            // Функциональные pseudo всегда не матчат в Phase 0.
            return Some(SimpleSelector::PseudoClass(PseudoClass::Unsupported(name)));
        }
        if is_element {
            return Some(SimpleSelector::PseudoElement(name));
        }
        let pc = match name.as_str() {
            "first-child" => PseudoClass::FirstChild,
            "last-child" => PseudoClass::LastChild,
            "only-child" => PseudoClass::OnlyChild,
            "empty" => PseudoClass::Empty,
            "root" => PseudoClass::Root,
            _ => PseudoClass::Unsupported(name),
        };
        Some(SimpleSelector::PseudoClass(pc))
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

    /// Удобный конструктор для тестов: ComplexSelector из одной compound с
    /// единственным simple-селектором.
    fn one(part: SimpleSelector) -> ComplexSelector {
        ComplexSelector {
            head: CompoundSelector { parts: vec![part] },
            tail: Vec::new(),
        }
    }

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
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Type("p".into()))]);
        assert_eq!(s.rules[0].declarations.len(), 1);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn class_selector() {
        let s = parse(".foo { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Class("foo".into()))]);
    }

    #[test]
    fn id_selector() {
        let s = parse("#bar { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Id("bar".into()))]);
    }

    #[test]
    fn universal_selector() {
        let s = parse("* { box-sizing: border-box; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Universal)]);
    }

    #[test]
    fn multiple_selectors() {
        let s = parse("p, h1, h2 { color: red; }");
        assert_eq!(
            s.rules[0].selectors,
            vec![
                one(SimpleSelector::Type("p".into())),
                one(SimpleSelector::Type("h1".into())),
                one(SimpleSelector::Type("h2".into())),
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
        assert_eq!(s.rules[0].selectors[0], one(SimpleSelector::Type("p".into())));
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
            vec![one(SimpleSelector::Class("привет".into()))]
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

    // ──────────────── compound selectors ────────────────

    #[test]
    fn compound_type_and_class() {
        let s = parse("p.foo { color: red; }");
        assert_eq!(s.rules[0].selectors.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Type("p".into()),
                SimpleSelector::Class("foo".into()),
            ]
        );
    }

    #[test]
    fn compound_type_class_id() {
        let s = parse("p.foo#bar { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Type("p".into()),
                SimpleSelector::Class("foo".into()),
                SimpleSelector::Id("bar".into()),
            ]
        );
    }

    #[test]
    fn compound_two_classes() {
        let s = parse(".a.b { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Class("a".into()),
                SimpleSelector::Class("b".into()),
            ]
        );
    }

    // ──────────────── combinators ────────────────

    #[test]
    fn descendant_combinator() {
        let s = parse("div p { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("p".into())]);
    }

    #[test]
    fn child_combinator() {
        let s = parse("ul > li { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::Child);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("li".into())]);
    }

    #[test]
    fn next_sibling_combinator() {
        let s = parse("h1 + p { margin-top: 0; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::NextSibling);
    }

    #[test]
    fn later_sibling_combinator() {
        let s = parse("h1 ~ p { color: gray; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::LaterSibling);
    }

    #[test]
    fn chained_combinators() {
        let s = parse("body main > article p { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("body".into())]);
        assert_eq!(sel.tail.len(), 3);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[1].0, Combinator::Child);
        assert_eq!(sel.tail[2].0, Combinator::Descendant);
    }

    #[test]
    fn combinator_around_compound() {
        let s = parse("nav.main > a.link { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 2);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].1.parts.len(), 2);
    }

    // ──────────────── attribute selectors ────────────────

    #[test]
    fn attribute_presence() {
        let s = parse("[disabled] { opacity: 0.5; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.name, "disabled");
                assert_eq!(a.op, None);
                assert_eq!(a.value, None);
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_equals_unquoted() {
        let s = parse("[type=submit] { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.name, "type");
                assert_eq!(a.op, Some(AttrOp::Equals));
                assert_eq!(a.value.as_deref(), Some("submit"));
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_equals_quoted() {
        let s = parse(r#"[lang="ru-RU"] { font-family: serif; }"#);
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.value.as_deref(), Some("ru-RU"));
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_all_operators() {
        let ops = [
            ("[a~=v]", AttrOp::Includes),
            ("[a|=v]", AttrOp::DashMatch),
            ("[a^=v]", AttrOp::Prefix),
            ("[a$=v]", AttrOp::Suffix),
            ("[a*=v]", AttrOp::Substring),
        ];
        for (src, expected) in ops {
            let s = parse(&format!("{src} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            match p {
                SimpleSelector::Attribute(a) => assert_eq!(a.op, Some(expected), "src={src}"),
                _ => panic!("expected attribute selector for {src}"),
            }
        }
    }

    #[test]
    fn attribute_combined_with_type() {
        let s = parse("a[href] { color: blue; }");
        let head = &s.rules[0].selectors[0].head;
        assert_eq!(head.parts.len(), 2);
        assert!(matches!(head.parts[0], SimpleSelector::Type(ref t) if t == "a"));
        assert!(matches!(&head.parts[1], SimpleSelector::Attribute(a) if a.name == "href"));
    }

    // ──────────────── pseudo-classes / pseudo-elements ────────────────

    #[test]
    fn pseudo_first_child() {
        let s = parse("p:first-child { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        assert!(matches!(
            &head.parts[1],
            SimpleSelector::PseudoClass(PseudoClass::FirstChild)
        ));
    }

    #[test]
    fn pseudo_known_names() {
        let cases = [
            ("first-child", PseudoClass::FirstChild),
            ("last-child", PseudoClass::LastChild),
            ("only-child", PseudoClass::OnlyChild),
            ("empty", PseudoClass::Empty),
            ("root", PseudoClass::Root),
        ];
        for (name, expected) in cases {
            let s = parse(&format!(":{name} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            match p {
                SimpleSelector::PseudoClass(pc) => assert_eq!(pc, &expected, "name={name}"),
                _ => panic!("expected pseudo-class for {name}"),
            }
        }
    }

    #[test]
    fn pseudo_unsupported_kept_as_name() {
        let s = parse(":hover { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) => assert_eq!(n, "hover"),
            _ => panic!("expected unsupported pseudo-class"),
        }
    }

    #[test]
    fn pseudo_functional_swallowed() {
        let s = parse(":nth-child(2n+1) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        // Функциональные pseudo всегда Unsupported.
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) => {
                assert_eq!(n, "nth-child");
            }
            _ => panic!("expected unsupported functional pseudo"),
        }
    }

    #[test]
    fn pseudo_element_double_colon() {
        let s = parse("p::before { content: \"\"; }");
        let head = &s.rules[0].selectors[0].head;
        assert!(matches!(&head.parts[1], SimpleSelector::PseudoElement(n) if n == "before"));
    }

    // ──────────────── specificity ────────────────

    #[test]
    fn specificity_universal_is_zero() {
        let s = parse("* { color: red; }");
        let spec = s.rules[0].selectors[0].specificity();
        assert_eq!(spec, Specificity { a: 0, b: 0, c: 0 });
    }

    #[test]
    fn specificity_type_is_001() {
        let s = parse("p { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 1 }
        );
    }

    #[test]
    fn specificity_class_is_010() {
        let s = parse(".foo { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_id_is_100() {
        let s = parse("#bar { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_complex() {
        // a#b.c[d] p:hover — id=1, class+attr+pseudo=3, type=2 → (1,3,2)
        let s = parse("a#b.c[d] p:hover { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 3, c: 2 }
        );
    }

    #[test]
    fn specificity_ordering() {
        let high = Specificity { a: 0, b: 1, c: 0 }; // .foo
        let low = Specificity { a: 0, b: 0, c: 5 }; // div div div div div
        assert!(high > low);
    }

    // ──────────────── edge cases для recovery ────────────────

    #[test]
    fn unknown_combinator_breaks_rule() {
        // `% p` — `%` не start_ident и не combinator, должен быть recovery.
        // Дальше нормальное правило парсится.
        let s = parse("% p { color: red; } a { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![SimpleSelector::Type("a".into())]
        );
    }

    #[test]
    fn malformed_attribute_recovers() {
        let s = parse("[a$$=foo] { color: red; } p { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![SimpleSelector::Type("p".into())]
        );
    }
}
