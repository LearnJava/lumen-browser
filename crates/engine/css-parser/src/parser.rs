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
//!   - structural pseudo-classes:
//!       - `:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`;
//!       - `:first-of-type`, `:last-of-type`, `:only-of-type`;
//!       - `:nth-child(an+b)`, `:nth-last-child(an+b)`,
//!         `:nth-of-type(an+b)`, `:nth-last-of-type(an+b)` — формулы
//!         `an+b`, целые числа, ключевые слова `odd` / `even`;
//!       - `:not(compound)` — отрицание; внутри — compound selector
//!         без combinator-ов;
//!       - `:is(selector-list)` / `:where(selector-list)` — CSS4; матчит,
//!         если матчит любой из селекторов списка. Внутри разрешены любые
//!         complex-селекторы. Specificity для `:is` = максимум по списку,
//!         для `:where` = 0.
//!   - interactive pseudo-classes (`:hover`, `:focus`, …) сохраняются как
//!     `PseudoClass::Unsupported(name)` и при матчинге всегда возвращают `false`;
//!   - pseudo-elements `::name` парсятся отдельным узлом, никогда не матчат
//!     (т.к. в DOM им ничего не соответствует);
//!   - комментарии `/* */`, перечисление селекторов через `,`, опциональный
//!     trailing `;`. At-rules (`@media`, `@import`) пропускаются.
//!
//! Не поддерживается (отложено): `:has(...)`, `:not(complex)` со списком
//! селекторов или combinator-ами, case-insensitive модификатор `[attr=val i]`,
//! namespace prefix в селекторах, типизированные значения деклараций
//! (length / color / calc).

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
    /// Модификатор `i` из CSS Selectors L4 §6.3.6 — ASCII case-insensitive
    /// сравнение значения. `s` явно ставит false (как default). Применим только
    /// при `op = Some(_)`; без оператора (`[attr]`) флаг игнорируется парсером.
    pub case_insensitive: bool,
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
    FirstOfType,
    LastOfType,
    OnlyOfType,
    /// `:nth-child(an+b)` — индекс среди всех element-sibling-ов (1-based).
    NthChild(NthSpec),
    /// `:nth-last-child(an+b)` — индекс с конца.
    NthLastChild(NthSpec),
    /// `:nth-of-type(an+b)` — индекс среди sibling-ов того же тега.
    NthOfType(NthSpec),
    /// `:nth-last-of-type(an+b)` — индекс с конца среди sibling-ов того же тега.
    NthLastOfType(NthSpec),
    /// `:not(compound)` — отрицание compound-селектора. Внутри запрещены
    /// combinator-ы (CSS3 §6.6.7); `:not(:not(...))` тоже нельзя — поэтому
    /// аргумент хранится как `CompoundSelector`, не как полный селектор.
    Not(Box<CompoundSelector>),
    /// `:is(s1, s2, …)` — матчит, если матчит хоть один из селекторов.
    /// CSS4 Selectors §17. Specificity вычисляется как максимум по списку
    /// (наследуется в родителя), независимо от того, какой именно матчит.
    Is(Vec<ComplexSelector>),
    /// `:where(s1, s2, …)` — то же, что `:is`, но specificity = 0 (всегда).
    /// Полезно для default-стилей, которые легко перебить любым правилом.
    Where(Vec<ComplexSelector>),
    /// `:hover`, `:focus`, `:active`, и т.п. — парсятся, но в Phase 0 никогда
    /// не матчат (нет интерактивного состояния). Хранится имя для отладки.
    Unsupported(String),
}

/// Формула `an+b` из CSS Selectors §6.6.5.1. Элемент с 1-based индексом `i`
/// матчит, если существует целое `n >= 0` такое, что `i = a*n + b`.
///
/// Преобразование ключевых слов:
///   - `odd` → `2n+1`;
///   - `even` → `2n+0`;
///   - просто число `5` → `0n+5` (точное совпадение).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NthSpec {
    pub a: i32,
    pub b: i32,
}

impl NthSpec {
    pub const ODD: Self = Self { a: 2, b: 1 };
    pub const EVEN: Self = Self { a: 2, b: 0 };

    /// Возвращает true, если элемент с 1-based индексом `index` матчит формулу.
    pub fn matches(&self, index: i32) -> bool {
        if self.a == 0 {
            return index == self.b;
        }
        // Нужно: index = a*n + b, n >= 0 (целое).
        // Значит (index - b) делится на a, и (index - b) / a >= 0.
        let diff = index - self.b;
        if diff == 0 {
            return true; // n = 0
        }
        if diff % self.a != 0 {
            return false;
        }
        let n = diff / self.a;
        n >= 0
    }
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

/// Максимум specificity среди списка ComplexSelector-ов. Используется для
/// `:is(...)` (CSS4 §17): pseudo-class contributes specificity of the most
/// specific item in its argument list.
fn max_list_specificity(list: &[ComplexSelector]) -> Option<Specificity> {
    list.iter().map(ComplexSelector::specificity).max()
}

fn accumulate_specificity(comp: &CompoundSelector, spec: &mut Specificity) {
    for part in &comp.parts {
        match part {
            SimpleSelector::Id(_) => spec.a = spec.a.saturating_add(1),
            SimpleSelector::Class(_) | SimpleSelector::Attribute(_) => {
                spec.b = spec.b.saturating_add(1);
            }
            SimpleSelector::PseudoClass(pc) => {
                // `:not(inner)` сам не считается, но содержимое — да (CSS3 §16).
                // `:is(...)` сам не считается, contributes max specificity по
                // списку (CSS4 §17). `:where(...)` — всегда 0.
                match pc {
                    PseudoClass::Not(inner) => accumulate_specificity(inner, spec),
                    PseudoClass::Is(list) => {
                        if let Some(max) = max_list_specificity(list) {
                            spec.a = spec.a.saturating_add(max.a);
                            spec.b = spec.b.saturating_add(max.b);
                            spec.c = spec.c.saturating_add(max.c);
                        }
                    }
                    PseudoClass::Where(_) => {} // contributes 0
                    _ => spec.b = spec.b.saturating_add(1),
                }
            }
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
    /// `!important` флаг (CSS Cascade L4 §8.1). При равной specificity
    /// `important = true` побеждает `important = false`.
    pub important: bool,
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
                // `)` — конец списка внутри функционального pseudo (`:is(...)` /
                // `:where(...)`); вне его `)` не появляется в правильном CSS.
                None | Some(',') | Some('{') | Some('}') | Some(')') => break,
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
                    case_insensitive: false,
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
        // CSS Selectors L4 §6.3.6: `i` или `s` после value — модификатор
        // сравнения. `i` — ASCII case-insensitive, `s` — explicit case-sensitive
        // (default). Парсятся case-insensitively сами по себе (`I` / `S` тоже
        // валидны).
        let case_insensitive = match self.peek() {
            Some('i' | 'I') => {
                self.consume();
                self.skip_ws_and_comments();
                true
            }
            Some('s' | 'S') => {
                self.consume();
                self.skip_ws_and_comments();
                false
            }
            _ => false,
        };
        if self.peek() != Some(']') {
            self.recover_to_attr_end();
            return None;
        }
        self.consume(); // ']'
        Some(SimpleSelector::Attribute(AttrSelector {
            name,
            op: Some(op),
            value: Some(value),
            case_insensitive,
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
        let lower = name.to_ascii_lowercase();
        if self.peek() == Some('(') {
            self.consume();
            let pc = self.parse_functional_pseudo_body(&lower);
            // Сожрать остаток до ')' если парсер вернул раньше времени или None.
            self.skip_to_paren_close();
            return Some(SimpleSelector::PseudoClass(pc.unwrap_or_else(|| {
                PseudoClass::Unsupported(name.clone())
            })));
        }
        if is_element {
            return Some(SimpleSelector::PseudoElement(name));
        }
        let pc = match lower.as_str() {
            "first-child" => PseudoClass::FirstChild,
            "last-child" => PseudoClass::LastChild,
            "only-child" => PseudoClass::OnlyChild,
            "empty" => PseudoClass::Empty,
            "root" => PseudoClass::Root,
            "first-of-type" => PseudoClass::FirstOfType,
            "last-of-type" => PseudoClass::LastOfType,
            "only-of-type" => PseudoClass::OnlyOfType,
            _ => PseudoClass::Unsupported(name),
        };
        Some(SimpleSelector::PseudoClass(pc))
    }

    /// Парсит тело `:foo(...)` для известных функциональных pseudo. Возвращает
    /// `None` для неизвестных или невалидных тел — caller обернёт в Unsupported
    /// и проглотит остаток до `)`.
    fn parse_functional_pseudo_body(&mut self, name_lower: &str) -> Option<PseudoClass> {
        match name_lower {
            "nth-child" => Some(PseudoClass::NthChild(self.parse_nth_spec()?)),
            "nth-last-child" => Some(PseudoClass::NthLastChild(self.parse_nth_spec()?)),
            "nth-of-type" => Some(PseudoClass::NthOfType(self.parse_nth_spec()?)),
            "nth-last-of-type" => Some(PseudoClass::NthLastOfType(self.parse_nth_spec()?)),
            "not" => {
                self.skip_ws_and_comments();
                let inner = self.parse_compound_selector()?;
                self.skip_ws_and_comments();
                // `:not(a b)` (с combinator-ом) в CSS3 запрещено — если после
                // compound есть что-то кроме `)`, считаем форму не поддерживаемой.
                if self.peek() != Some(')') {
                    return None;
                }
                // `:not(:not(...))` тоже запрещено в CSS3.
                if inner
                    .parts
                    .iter()
                    .any(|p| matches!(p, SimpleSelector::PseudoClass(PseudoClass::Not(_))))
                {
                    return None;
                }
                Some(PseudoClass::Not(Box::new(inner)))
            }
            "is" => {
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                // Должны быть на `)`; иначе argument невалиден.
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Is(list))
            }
            "where" => {
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Where(list))
            }
            _ => None,
        }
    }

    /// Парсит `an+b`, число или ключевые слова `odd`/`even`. Останавливается на
    /// `)` или конце ввода — caller съест `)` через `skip_to_paren_close`.
    fn parse_nth_spec(&mut self) -> Option<NthSpec> {
        self.skip_ws_and_comments();
        // Соберём «токен» формулы — всё до `)` или конца.
        let mut raw = String::new();
        while let Some(c) = self.peek() {
            if c == ')' {
                break;
            }
            raw.push(c);
            self.consume();
        }
        parse_nth_spec_str(raw.trim())
    }

    fn skip_to_paren_close(&mut self) {
        let mut depth = 1;
        while let Some(c) = self.peek() {
            self.consume();
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
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
        let (value, important) = extract_important(value.trim());
        Some(Declaration {
            property,
            value,
            important,
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

/// CSS Cascade L4 §8.1: если значение оканчивается на `!important` (с
/// опциональным whitespace между `!` и словом, ASCII case-insensitive),
/// отделяет его и возвращает `(clean_value, true)`. Иначе — `(value, false)`.
///
/// Безопасно для строковых литералов: `content: "!important"` даёт
/// (value=`"!important"`, false), потому что после строки идёт `"`, а не
/// `important`. Не пытается обрабатывать комментарии внутри `!important`
/// (`!/* x */important`) и multiple `!important` — оба слишком экзотичны.
fn extract_important(value: &str) -> (String, bool) {
    let v = value.trim_end();
    let imp = b"important";
    if v.len() < imp.len() {
        return (value.to_string(), false);
    }
    if !v.as_bytes()[v.len() - imp.len()..].eq_ignore_ascii_case(imp) {
        return (value.to_string(), false);
    }
    let before_imp = v[..v.len() - imp.len()].trim_end();
    let Some(before_bang) = before_imp.strip_suffix('!') else {
        return (value.to_string(), false);
    };
    (before_bang.trim_end().to_string(), true)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '-' || c >= '\u{00A0}'
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// Парсит формулу `an+b` из строки. Поддерживает `odd`, `even`, целые числа,
/// и любые комбинации `<int>?n<sign><int>?`. Пробелы внутри допустимы и
/// игнорируются (CSS spec).
fn parse_nth_spec_str(s: &str) -> Option<NthSpec> {
    let s: String = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if s == "odd" {
        return Some(NthSpec::ODD);
    }
    if s == "even" {
        return Some(NthSpec::EVEN);
    }
    if let Some(n_pos) = s.find('n') {
        let a_part = &s[..n_pos];
        let b_part = &s[n_pos + 1..];
        let a: i32 = match a_part {
            "" | "+" => 1,
            "-" => -1,
            _ => a_part.parse().ok()?,
        };
        let b: i32 = if b_part.is_empty() {
            0
        } else {
            if !b_part.starts_with('+') && !b_part.starts_with('-') {
                return None;
            }
            b_part.parse().ok()?
        };
        Some(NthSpec { a, b })
    } else {
        Some(NthSpec { a: 0, b: s.parse().ok()? })
    }
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

    // ──────────────── !important (CSS Cascade L4 §8.1) ────────────────

    #[test]
    fn declaration_default_not_important() {
        let s = parse("p { color: red; }");
        assert!(!s.rules[0].declarations[0].important);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn declaration_important_basic() {
        let s = parse("p { color: red !important; }");
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, "red");
    }

    #[test]
    fn declaration_important_no_space_before_bang() {
        let s = parse("p { color: red!important; }");
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, "red");
    }

    #[test]
    fn declaration_important_case_insensitive() {
        let s = parse("p { color: red !IMPORTANT; }");
        assert!(s.rules[0].declarations[0].important);
    }

    #[test]
    fn declaration_important_with_whitespace_between_bang_and_word() {
        // CSS Syntax §5.5.4 разрешает whitespace внутри `!important`.
        let s = parse("p { color: red !  important; }");
        assert!(s.rules[0].declarations[0].important);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn declaration_important_inside_quotes_not_stripped() {
        // `content: "!important"` — литерал, не модификатор.
        let s = parse(r#"p { content: "!important"; }"#);
        let d = &s.rules[0].declarations[0];
        assert!(!d.important);
        assert_eq!(d.value, r#""!important""#);
    }

    #[test]
    fn declaration_important_after_quoted_value() {
        // `font-family: "Arial" !important;` — флаг есть, value сохраняется.
        let s = parse(r#"p { font-family: "Arial" !important; }"#);
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, r#""Arial""#);
    }

    #[test]
    fn declaration_important_works_for_multiple() {
        let s = parse("p { color: red !important; font-size: 14px; }");
        assert!(s.rules[0].declarations[0].important);
        assert!(!s.rules[0].declarations[1].important);
    }

    #[test]
    fn declaration_value_ending_with_important_word_alone_not_flag() {
        // `value: important;` — без `!`, не флаг.
        let s = parse("p { font-weight: important; }");
        let d = &s.rules[0].declarations[0];
        assert!(!d.important);
        assert_eq!(d.value, "important");
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

    // ──────────────── case-insensitive attribute (CSS L4 §6.3.6) ────────────

    fn attr_at(s: &Stylesheet, rule: usize) -> &AttrSelector {
        match &s.rules[rule].selectors[0].head.parts[0] {
            SimpleSelector::Attribute(a) => a,
            other => panic!("expected attribute selector, got {other:?}"),
        }
    }

    #[test]
    fn attribute_case_insensitive_flag_lowercase() {
        let s = parse("[type=submit i] { color: red; }");
        let a = attr_at(&s, 0);
        assert!(a.case_insensitive);
        assert_eq!(a.value.as_deref(), Some("submit"));
    }

    #[test]
    fn attribute_case_insensitive_flag_uppercase() {
        // `I` тоже должен работать (флаги ASCII case-insensitive).
        let s = parse("[type=submit I] { color: red; }");
        assert!(attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_sensitive_explicit() {
        // `s` явно ставит case-sensitive (default).
        let s = parse("[type=submit s] { color: red; }");
        assert!(!attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_insensitive_with_quoted_value() {
        let s = parse(r#"[lang="EN-us" i] { color: red; }"#);
        let a = attr_at(&s, 0);
        assert!(a.case_insensitive);
        assert_eq!(a.value.as_deref(), Some("EN-us"));
    }

    #[test]
    fn attribute_case_insensitive_works_for_all_ops() {
        // Флаг `i` совместим со всеми операторами.
        for src in [
            "[a~=v i]",
            "[a|=v i]",
            "[a^=v i]",
            "[a$=v i]",
            "[a*=v i]",
        ] {
            let s = parse(&format!("{src} {{}}"));
            assert!(attr_at(&s, 0).case_insensitive, "ci flag lost in {src}");
        }
    }

    #[test]
    fn attribute_no_flag_default_case_sensitive() {
        let s = parse("[type=submit] { color: red; }");
        assert!(!attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_insensitive_with_extra_whitespace() {
        // Между value и `i` — любое количество пробелов.
        let s = parse("[type=submit   i ] { color: red; }");
        assert!(attr_at(&s, 0).case_insensitive);
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
            ("first-of-type", PseudoClass::FirstOfType),
            ("last-of-type", PseudoClass::LastOfType),
            ("only-of-type", PseudoClass::OnlyOfType),
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
    fn pseudo_nth_child_parsed() {
        let s = parse(":nth-child(2n+1) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthChild(spec)) => {
                assert_eq!(*spec, NthSpec { a: 2, b: 1 });
            }
            _ => panic!("expected NthChild(2n+1), got {p:?}"),
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

    // ──────────────── functional pseudo: :nth-* ────────────────

    #[test]
    fn nth_spec_str_keywords() {
        assert_eq!(parse_nth_spec_str("odd"), Some(NthSpec { a: 2, b: 1 }));
        assert_eq!(parse_nth_spec_str("even"), Some(NthSpec { a: 2, b: 0 }));
        assert_eq!(parse_nth_spec_str("ODD"), Some(NthSpec { a: 2, b: 1 }));
    }

    #[test]
    fn nth_spec_str_formulas() {
        let cases = [
            ("n", (1, 0)),
            ("+n", (1, 0)),
            ("-n", (-1, 0)),
            ("2n", (2, 0)),
            ("2n+1", (2, 1)),
            ("2n-1", (2, -1)),
            ("-2n+3", (-2, 3)),
            ("3n+0", (3, 0)),
            ("5", (0, 5)),
            ("-5", (0, -5)),
            ("2n + 1", (2, 1)), // пробелы допустимы
            ("  2n  ", (2, 0)),
        ];
        for (s, (a, b)) in cases {
            assert_eq!(
                parse_nth_spec_str(s),
                Some(NthSpec { a, b }),
                "input={s}"
            );
        }
    }

    #[test]
    fn nth_spec_str_invalid() {
        assert_eq!(parse_nth_spec_str(""), None);
        assert_eq!(parse_nth_spec_str("abc"), None);
        assert_eq!(parse_nth_spec_str("2x+1"), None);
        assert_eq!(parse_nth_spec_str("n+"), None); // нет числа после знака
    }

    #[test]
    fn nth_spec_matches_arithmetic() {
        let odd = NthSpec::ODD; // 2n+1: 1, 3, 5, ...
        for i in [1, 3, 5, 7, 999] {
            assert!(odd.matches(i), "i={i}");
        }
        for i in [0, 2, 4, -1] {
            assert!(!odd.matches(i), "i={i}");
        }
    }

    #[test]
    fn nth_spec_matches_first_three() {
        // -n+3 → элементы 1, 2, 3 (n=2, 1, 0). Индексы в CSS — 1-based,
        // нулевой случай в реальном matching-е не возникает.
        let spec = NthSpec { a: -1, b: 3 };
        assert!(spec.matches(1));
        assert!(spec.matches(2));
        assert!(spec.matches(3));
        assert!(!spec.matches(4));
        assert!(!spec.matches(5));
    }

    #[test]
    fn nth_spec_matches_constant() {
        // 5 → ровно пятый.
        let spec = NthSpec { a: 0, b: 5 };
        assert!(spec.matches(5));
        assert!(!spec.matches(4));
        assert!(!spec.matches(10));
    }

    #[test]
    fn pseudo_nth_variants_parsed() {
        let cases = [
            ("nth-child", "(2n+1)"),
            ("nth-last-child", "(odd)"),
            ("nth-of-type", "(3)"),
            ("nth-last-of-type", "(-n+2)"),
        ];
        for (name, arg) in cases {
            let s = parse(&format!(":{name}{arg} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            let pc = match p {
                SimpleSelector::PseudoClass(pc) => pc,
                _ => panic!("expected pseudo-class for :{name}{arg}"),
            };
            let is_nth = matches!(
                pc,
                PseudoClass::NthChild(_)
                    | PseudoClass::NthLastChild(_)
                    | PseudoClass::NthOfType(_)
                    | PseudoClass::NthLastOfType(_)
            );
            assert!(is_nth, "name={name} got {pc:?}");
        }
    }

    #[test]
    fn pseudo_nth_invalid_arg_falls_back_to_unsupported() {
        let s = parse(":nth-child(abc) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) => {
                assert_eq!(n, "nth-child");
            }
            _ => panic!("expected Unsupported(nth-child), got {p:?}"),
        }
        // Парсер должен дойти до конца правила и не оставить мусора.
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    // ──────────────── functional pseudo: :not ────────────────

    #[test]
    fn pseudo_not_simple() {
        let s = parse(":not(.foo) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(inner)) => {
                assert_eq!(inner.parts, vec![SimpleSelector::Class("foo".into())]);
            }
            _ => panic!("expected :not(.foo), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_compound() {
        let s = parse(":not(p.hl) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(inner)) => {
                assert_eq!(inner.parts.len(), 2);
                assert!(matches!(&inner.parts[0], SimpleSelector::Type(t) if t == "p"));
                assert!(matches!(&inner.parts[1], SimpleSelector::Class(c) if c == "hl"));
            }
            _ => panic!("expected :not(p.hl)"),
        }
    }

    #[test]
    fn pseudo_not_with_combinator_falls_back() {
        // `:not(a b)` запрещено в CSS3 (combinator внутри) → Unsupported.
        let s = parse(":not(a b) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(
            matches!(p, SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "not"),
            "got {p:?}"
        );
    }

    #[test]
    fn pseudo_not_nested_forbidden() {
        // `:not(:not(...))` запрещено в CSS3.
        let s = parse(":not(:not(.x)) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(
            matches!(p, SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "not"),
            "got {p:?}"
        );
    }

    #[test]
    fn specificity_not_uses_inner() {
        // :not(.foo) → внутренний .foo даёт b=1; сам :not — ноль.
        let s = parse(":not(.foo) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_not_with_id() {
        // :not(#x) → a=1, b=0, c=0.
        let s = parse(":not(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    // ──────────────── functional pseudo: :is, :where ────────────────

    fn pseudo_at(s: &Stylesheet, rule: usize, sel: usize, part: usize) -> &PseudoClass {
        match &s.rules[rule].selectors[sel].head.parts[part] {
            SimpleSelector::PseudoClass(pc) => pc,
            other => panic!("expected pseudo-class, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_is_class_list() {
        let s = parse(":is(.foo, .bar) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Is(list) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts, vec![SimpleSelector::Class("foo".into())]);
                assert_eq!(list[1].head.parts, vec![SimpleSelector::Class("bar".into())]);
            }
            _ => panic!("expected :is(...), got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_where_class_list() {
        let s = parse(":where(.foo, #bar) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Where(list) if list.len() == 2), "got {pc:?}");
    }

    #[test]
    fn pseudo_is_with_combinator_inside() {
        // CSS4 разрешает combinator-ы внутри :is — в отличие от :not.
        let s = parse(":is(a > b, c d) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Is(list) => {
                assert_eq!(list.len(), 2);
                // a > b: head 'a', tail [(Child, 'b')]
                assert_eq!(list[0].tail.len(), 1);
                assert_eq!(list[0].tail[0].0, Combinator::Child);
                // c d: head 'c', tail [(Descendant, 'd')]
                assert_eq!(list[1].tail.len(), 1);
                assert_eq!(list[1].tail[0].0, Combinator::Descendant);
            }
            _ => panic!("expected :is, got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_is_with_type_selector() {
        let s = parse("article :is(h1, h2) { color: red; }");
        let sel = &s.rules[0].selectors[0];
        // head = 'article', tail = [(Descendant, compound{:is(h1, h2)})]
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("article".into())]);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert!(matches!(
            &sel.tail[0].1.parts[0],
            SimpleSelector::PseudoClass(PseudoClass::Is(list)) if list.len() == 2
        ));
    }

    #[test]
    fn pseudo_is_empty_falls_back() {
        // `:is()` без аргументов — невалидно, должен дать Unsupported.
        let s = parse(":is() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "is"), "got {pc:?}");
    }

    #[test]
    fn pseudo_where_empty_falls_back() {
        let s = parse(":where() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "where"), "got {pc:?}");
    }

    #[test]
    fn specificity_is_takes_max_of_list() {
        // :is(.foo, #bar) → max = (#bar) = (1,0,0).
        let s = parse(":is(.foo, #bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_is_only_classes() {
        // :is(.foo, .bar) → max = (0,1,0).
        let s = parse(":is(.foo, .bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_where_always_zero() {
        // :where(#x) → 0,0,0 даже при id внутри.
        let s = parse(":where(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_where_combined_with_outer() {
        // `p:where(#x)` → p (c=1), :where contributes 0 → (0,0,1).
        let s = parse("p:where(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 1 }
        );
    }

    #[test]
    fn pseudo_is_with_whitespace_around_list() {
        // Внутри `:is( .foo , .bar )` бывают пробелы — парсер не должен терять
        // последний селектор из-за trailing whitespace перед `)`.
        let s = parse(":is( .foo , .bar ) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Is(list) if list.len() == 2), "got {pc:?}");
    }
}
