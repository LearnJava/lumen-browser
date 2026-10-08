//! `@supports` (CSS Conditional Rules L3 §2): [`SupportsRule`], [`SupportsCondition`],
//! разбор условия и тело правила.
//!
//! Вырезано из `parser/at_rules.rs` (SPLIT-CP2) без изменения поведения.

use super::*;

/// `@supports <condition> { rules }` блок — CSS Conditional Rules L3 §2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportsRule {
    pub condition: SupportsCondition,
    pub rules: Vec<Rule>,
}

/// Условие в `@supports (...)`. Грамматика:
/// `<condition> = <negation> | <conjunction> | <disjunction> | <test>`
/// `<negation>  = "not" <inside-parens>`
/// `<conjunction> = <test> ("and" <test>)+`
/// `<disjunction> = <test> ("or" <test>)+`
/// `<test>       = "(" <property>: <value> ")" | "(" <condition> ")"`.
///
/// Phase 0: парсер также распознаёт `selector(<simple>)` (CSS Conditional
/// L4) и сохраняет селектор как сырую строку.
/// Функциональные тесты `font-tech(<font-tech>)` и
/// `font-format(<font-format>)` (CSS Conditional L4 §4 / CSS Fonts L4 §4.3)
/// тоже типизированы — evaluator сверяет аргумент со списком технологий и
/// форматов шрифтов, поддержанных движком `lumen-font`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportsCondition {
    /// `(prop: value)` — declaration test. Текущий supports-evaluator
    /// проверяет, что `property` есть в списке known-property-имён,
    /// не валидируя value (для Phase 0 этого достаточно — мы поддерживаем
    /// конкретный набор properties, и tests типа `(display: grid)`
    /// возвращают true, потому что мы парсим `display`, даже если
    /// реального grid layout-а нет).
    Decl { property: String, value: String },
    Not(Box<SupportsCondition>),
    And(Vec<SupportsCondition>),
    Or(Vec<SupportsCondition>),
    /// `selector(<sel>)` — CSS Conditional L4. Phase 0 не оценивает.
    Selector(String),
    /// `font-tech(<font-tech>)` — CSS Conditional L4 §4 / CSS Fonts L4 §4.3.
    /// Хранит lowercase-ключевое слово технологии шрифта (например,
    /// `variations`, `color-colrv1`, `features-opentype`). Evaluator
    /// возвращает `true`, если технология реализована в `lumen-font`.
    FontTech(String),
    /// `font-format(<font-format>)` — CSS Conditional L4 §4 / CSS Fonts L4 §4.3.
    /// Хранит lowercase-ключевое слово формата шрифта (например, `woff2`,
    /// `opentype`, `truetype`). Кавычки legacy-строкового синтаксиса
    /// (`font-format("woff2")`) снимаются при разборе. Evaluator возвращает
    /// `true`, если формат декодируется движком `lumen-font`.
    FontFormat(String),
    /// Невалидный или нераспознанный тест — evaluator возвращает false.
    Unknown,
}

/// Технологии шрифтов (`<font-tech>`, CSS Fonts L4 §4.3), которые
/// `lumen-font` реально реализует: OpenType-фичи (GSUB/GPOS) и вариативные
/// шрифты (fvar/gvar/avar/HVAR/MVAR). Цветные глифы (COLR/CPAL, sbix, CBDT,
/// SVG-in-OpenType), палитры, AAT/Graphite-фичи и инкрементальная загрузка
/// пока не поддержаны — см. `crates/engine/font/src/lib.rs` (заголовок).
pub(crate) const SUPPORTED_FONT_TECH: &[&str] = &["features-opentype", "variations"];

/// Форматы шрифтов (`<font-format>`, CSS Fonts L4 §4.3), которые
/// `lumen-font` умеет декодировать: TrueType (glyf), OpenType (CFF/glyf +
/// OT layout), WOFF1 (`decode_woff1`) и WOFF2 (`decode_woff2`). Контейнеры
/// `collection` (.ttc), `embedded-opentype` (EOT) и `svg`-шрифты не
/// поддержаны — см. `crates/engine/font/src/woff2.rs` и `lib.rs`.
pub(crate) const SUPPORTED_FONT_FORMAT: &[&str] = &["opentype", "truetype", "woff", "woff2"];

impl SupportsCondition {
    /// Вычислить условие: вернуть `true`, если потребитель поддерживает
    /// все объявления в условии. `known_properties` — список property-
    /// имён, которые css-parser/layout распознают (например, `display`,
    /// `color`, `grid-template-columns`).
    ///
    /// `Selector(<sel>)` (CSS Conditional L4 §4.2 `selector()`) парсится и
    /// признаётся поддержанным, если каждая его часть распознаётся движком —
    /// см. [`ComplexSelector::is_supported`]. Пустой/невалидный селектор → `false`.
    /// `FontTech`/`FontFormat` сверяются со списками технологий и форматов,
    /// которые реально реализует `lumen-font` ([`SUPPORTED_FONT_TECH`] /
    /// [`SUPPORTED_FONT_FORMAT`]). `Unknown` → `false`.
    pub fn evaluate(&self, known_properties: &[&str]) -> bool {
        match self {
            // CSS Variables L1 §2: a custom property accepts any token
            // sequence as its value, so once a UA implements custom
            // properties at all, `@supports (--x: <anything>)` must always
            // be considered supported — it is never present in
            // `known_properties` (a hand-written list of standard names).
            Self::Decl { property, .. } if property.starts_with("--") => true,
            Self::Decl { property, .. } => known_properties
                .iter()
                .any(|p| p.eq_ignore_ascii_case(property)),
            Self::Not(c) => !c.evaluate(known_properties),
            Self::And(cs) => cs.iter().all(|c| c.evaluate(known_properties)),
            Self::Or(cs) => cs.iter().any(|c| c.evaluate(known_properties)),
            Self::Selector(sel) => {
                let list = parse_selector_list(sel);
                !list.is_empty() && list.iter().all(ComplexSelector::is_supported)
            }
            Self::FontTech(tech) => SUPPORTED_FONT_TECH
                .iter()
                .any(|t| t.eq_ignore_ascii_case(tech)),
            Self::FontFormat(fmt) => SUPPORTED_FONT_FORMAT
                .iter()
                .any(|f| f.eq_ignore_ascii_case(fmt)),
            Self::Unknown => false,
        }
    }
}

/// Парсит `@supports`-условие из строки между `@supports` и `{`.
///
/// Грамматика (упрощённая): `<expr> = <term> (("and"|"or") <term>)*`,
/// `<term> = "not"? <atom>`, `<atom> = "(" <inner> ")" | "selector(" sel ")"`,
/// `<inner> = <expr> | <prop ":" value>`.
///
/// Phase 0 ограничения:
/// - Mixing `and` и `or` на одном уровне не разрешено (per spec), но
///   парсер lenient — берёт первый встретившийся combinator и применяет
///   его ко всем term-ам этого уровня. Реалистичные tests этого не
///   нарушают (`(a) and (b) and (c)` или `(a) or (b)`); смешанные — UB.
/// - Нерекурсивный `selector(...)` хранит сырой селектор; реальный
///   match — отложенная задача.
pub fn parse_supports_condition(s: &str) -> SupportsCondition {
    let s = s.trim();
    if s.is_empty() {
        return SupportsCondition::Unknown;
    }
    let bytes = s.as_bytes();
    let mut pos = 0usize;
    let result = parse_supports_expr(bytes, &mut pos);
    skip_ws(bytes, &mut pos);
    if pos < bytes.len() {
        // Если что-то осталось — это синтаксическая ошибка; возвращаем
        // частично разобранное (lenient).
    }
    result
}

/// Парсит значение `override-colors` из `@font-palette-values`.
/// Формат: comma-separated `<u16-index> <color-string>` пары.
/// CSS Fonts L4 §13.3. Хранит color как raw string — resolve через
/// `parse_color` выполняется в layout при использовании palette.
pub(crate) fn parse_override_colors(s: &str) -> Vec<(u16, String)> {
    let mut result = Vec::new();
    for pair in s.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, char::is_whitespace);
        if let (Some(idx_str), Some(color_str)) = (parts.next(), parts.next())
            && let Ok(idx) = idx_str.trim().parse::<u16>()
        {
            let color = color_str.trim().to_string();
            if !color.is_empty() {
                result.push((idx, color));
            }
        }
    }
    result
}

pub(crate) fn skip_ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && b[*p].is_ascii_whitespace() {
        *p += 1;
    }
}

pub(crate) fn match_keyword_ci(b: &[u8], p: &mut usize, kw: &[u8]) -> bool {
    skip_ws(b, p);
    if *p + kw.len() > b.len() {
        return false;
    }
    if !b[*p..*p + kw.len()].eq_ignore_ascii_case(kw) {
        return false;
    }
    // Граница: следующий символ — не ident-char.
    let after = *p + kw.len();
    if after < b.len() {
        let c = b[after];
        if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' {
            return false;
        }
    }
    *p = after;
    true
}

pub(crate) fn parse_supports_expr(b: &[u8], p: &mut usize) -> SupportsCondition {
    let first = parse_supports_term(b, p);
    skip_ws(b, p);
    // Определяем combinator (если есть).
    let saved = *p;
    if match_keyword_ci(b, p, b"and") {
        let mut terms = vec![first];
        loop {
            terms.push(parse_supports_term(b, p));
            skip_ws(b, p);
            let save = *p;
            if !match_keyword_ci(b, p, b"and") {
                *p = save;
                break;
            }
        }
        return SupportsCondition::And(terms);
    }
    *p = saved;
    if match_keyword_ci(b, p, b"or") {
        let mut terms = vec![first];
        loop {
            terms.push(parse_supports_term(b, p));
            skip_ws(b, p);
            let save = *p;
            if !match_keyword_ci(b, p, b"or") {
                *p = save;
                break;
            }
        }
        return SupportsCondition::Or(terms);
    }
    first
}

pub(crate) fn parse_supports_term(b: &[u8], p: &mut usize) -> SupportsCondition {
    skip_ws(b, p);
    if match_keyword_ci(b, p, b"not") {
        let inner = parse_supports_atom(b, p);
        return SupportsCondition::Not(Box::new(inner));
    }
    parse_supports_atom(b, p)
}

/// Если ввод в позиции `*p` начинается с функции `name` (case-insensitive),
/// продвинуть `*p` за закрывающую `)` и вернуть содержимое скобок как строку.
/// Иначе оставить `*p` без изменений и вернуть `None`. Учитывает вложенные
/// скобки в аргументе (хотя для `font-tech`/`font-format` они не нужны).
pub(crate) fn match_func_arg(b: &[u8], p: &mut usize, name: &[u8]) -> Option<String> {
    let n = name.len();
    if *p + n > b.len() || !b[*p..*p + n].eq_ignore_ascii_case(name) {
        return None;
    }
    let start = *p + n;
    let mut q = start;
    let mut depth: i32 = 1;
    while q < b.len() && depth > 0 {
        match b[q] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            break;
        }
        q += 1;
    }
    let arg = std::str::from_utf8(&b[start..q]).unwrap_or("").to_string();
    if q < b.len() && b[q] == b')' {
        q += 1;
    }
    *p = q;
    Some(arg)
}

pub(crate) fn parse_supports_atom(b: &[u8], p: &mut usize) -> SupportsCondition {
    skip_ws(b, p);
    // `font-tech( <font-tech> )` / `font-format( <font-format> )`
    // (CSS Conditional L4 §4 / CSS Fonts L4 §4.3). Один ident-аргумент;
    // у `font-format` допустим legacy-строковый синтаксис (кавычки снимаем).
    if let Some(arg) = match_func_arg(b, p, b"font-tech(") {
        return SupportsCondition::FontTech(arg.trim().to_ascii_lowercase());
    }
    if let Some(arg) = match_func_arg(b, p, b"font-format(") {
        let unquoted = arg.trim().trim_matches(['"', '\'']).trim();
        return SupportsCondition::FontFormat(unquoted.to_ascii_lowercase());
    }
    // `selector( ... )`
    let saved = *p;
    if *p + 9 <= b.len() && b[*p..*p + 9].eq_ignore_ascii_case(b"selector(") {
        *p += 9;
        let start = *p;
        let mut depth: i32 = 1;
        while *p < b.len() && depth > 0 {
            match b[*p] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            *p += 1;
        }
        let sel_str = std::str::from_utf8(&b[start..*p]).unwrap_or("").trim().to_string();
        if *p < b.len() && b[*p] == b')' {
            *p += 1;
        }
        return SupportsCondition::Selector(sel_str);
    }
    *p = saved;
    if *p < b.len() && b[*p] == b'(' {
        *p += 1;
        // Содержимое: может быть `<expr>` (nested condition) или
        // `<prop>: <value>`. Различаем по наличию `:` на верхнем уровне.
        let inner_start = *p;
        let mut depth: i32 = 1;
        while *p < b.len() && depth > 0 {
            match b[*p] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            *p += 1;
        }
        let inner = std::str::from_utf8(&b[inner_start..*p]).unwrap_or("");
        if *p < b.len() && b[*p] == b')' {
            *p += 1;
        }
        // Determine: declaration or nested condition. Top-level `:`?
        let inner_t = inner.trim();
        let mut colon_pos: Option<usize> = None;
        let inner_bytes = inner_t.as_bytes();
        let mut d: i32 = 0;
        for (i, &c) in inner_bytes.iter().enumerate() {
            match c {
                b'(' => d += 1,
                b')' => d -= 1,
                b':' if d == 0 => {
                    colon_pos = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(idx) = colon_pos {
            let property = inner_t[..idx].trim().to_string();
            let value = inner_t[idx + 1..].trim().to_string();
            if property.is_empty() {
                return SupportsCondition::Unknown;
            }
            return SupportsCondition::Decl { property, value };
        }
        return parse_supports_condition(inner_t);
    }
    SupportsCondition::Unknown
}

impl<'a> Parser<'a> {
    /// Парсит тело `@supports <condition> { rules }` — CSS Conditional Rules L3 §2.
    /// Берёт сырую condition-строку до `{` (с балансировкой `(`/`)`),
    /// затем парсит её через [`parse_supports_condition`]. Тело — обычные
    /// rules до `}`. Возвращает `None` если структура нарушена.
    pub(crate) fn parse_supports_rule(&mut self) -> Option<SupportsRule> {
        self.skip_ws_and_comments();
        let cond_start = self.pos;
        let mut depth: i32 = 0;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
            } else if c == '{' && depth == 0 {
                break;
            } else if c == ';' && depth == 0 {
                // BUG-793: top-level `;` (outside parens) closes the at-rule
                // without a block per CSS Syntax L3 §5.4.2 — stop here
                // instead of scanning past it into the next rule's block.
                break;
            }
            self.consume();
        }
        if self.peek() == Some(';') {
            self.consume();
            return None;
        }
        if self.peek() != Some('{') {
            return None;
        }
        let cond_str = self.input[cond_start..self.pos].trim();
        let condition = parse_supports_condition(cond_str);
        self.consume(); // '{'
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    // Nested @-правила внутри @supports пока skip.
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(SupportsRule { condition, rules })
    }
}
