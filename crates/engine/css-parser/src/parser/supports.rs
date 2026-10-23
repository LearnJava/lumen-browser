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
    /// Место блока среди обычных правил листа — см. [`BlockPos`].
    pub pos: BlockPos,
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
    /// `at-rule(@<name>)` — CSS Conditional L4 §3.3. Хранит lowercase-имя
    /// at-правила без `@`; evaluator сверяет его со списком
    /// [`SUPPORTED_AT_RULES`].
    AtRule(String),
    /// Невалидный или нераспознанный тест — evaluator возвращает false.
    Unknown,
}

/// Технологии шрифтов (`<font-tech>`, CSS Fonts L4 §4.3), которые
/// `lumen-font` реально реализует: OpenType-фичи (GSUB/GPOS) и вариативные
/// шрифты (fvar/gvar/avar/HVAR/MVAR). Цветные глифы (COLR/CPAL, sbix, CBDT,
/// SVG-in-OpenType), палитры, AAT/Graphite-фичи и инкрементальная загрузка
/// пока не поддержаны — см. `crates/engine/font/src/lib.rs` (заголовок).
pub(crate) const SUPPORTED_FONT_TECH: &[&str] =
    &["features-opentype", "variations", "color-colrv0", "palettes"];

/// At-правила, которые парсер/CSSOM Lumen распознают (`at-rule(@<name>)`,
/// CSS Conditional L4 §3.3). `@charset` не at-правило в этом смысле, а
/// `@position-try` и подобные пока не разбираются.
pub(crate) const SUPPORTED_AT_RULES: &[&str] = &[
    "property", "media", "import", "font-face", "font-palette-values", "font-feature-values",
    "layer", "supports", "keyframes", "counter-style", "page", "scope", "starting-style",
    "container", "color-profile", "function", "mixin", "view-transition", "namespace",
    "swash", "stylistic", "ornaments", "annotation", "styleset", "character-variant",
];

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
            // `selector(<complex-selector>)` — ровно один complex-селектор,
            // разобранный целиком (список `a, b` недопустим).
            Self::Selector(sel) => {
                let mut parser = Parser::new(sel);
                let one = parser.parse_complex_selector();
                parser.skip_ws_and_comments();
                parser.peek().is_none()
                    && one.is_some_and(|c| {
                        c.is_supported() && complex_selector_is_valid(&c, true)
                    })
            }
            Self::AtRule(name) => SUPPORTED_AT_RULES.contains(&name.as_str()),
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
/// Грамматика CSS Conditional L3 §2 / L4 §2:
/// `<supports-condition> = not <in-parens> | <in-parens> [and <in-parens>]* |
/// <in-parens> [or <in-parens>]*`,
/// `<in-parens> = ( <supports-condition> ) | <supports-feature> | <general-enclosed>`,
/// `<supports-feature> = ( <property>: <value> [!important]? ) | selector(…) |
/// font-tech(…) | font-format(…)`.
///
/// - `not`/`and`/`or` — идентификаторы: за ключевым словом обязателен пробел
///   (`not(`/`and(`/`or(` — функциональные токены, а не операторы), `and` и
///   `or` на одном уровне не смешиваются, `not` не сочетается с `and`/`or`.
/// - Скобки, чьё содержимое не разобралось ни как условие, ни как объявление
///   (`<general-enclosed>`), дают [`SupportsCondition::Unknown`] — ложь, но
///   условие вокруг остаётся допустимым, поэтому `not (…)` над ними истинно.
/// - Любой другой синтаксический сбой (хвостовые токены, смесь связок, голый
///   ident) делает недопустимым всё условие → `Unknown`.
/// - Нерекурсивный `selector(...)` хранит сырой селектор; реальный
///   match — отложенная задача.
pub fn parse_supports_condition(s: &str) -> SupportsCondition {
    let b = s.trim().as_bytes();
    let mut pos = 0usize;
    let result = parse_condition(b, &mut pos);
    skip_ws(b, &mut pos);
    match result {
        Some(c) if pos >= b.len() => c,
        _ => SupportsCondition::Unknown,
    }
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

/// Пробелы и комментарии `/* … */` между токенами условия.
fn skip_ws(b: &[u8], p: &mut usize) {
    loop {
        while *p < b.len() && b[*p].is_ascii_whitespace() {
            *p += 1;
        }
        if b[*p..].starts_with(b"/*") {
            *p = comment_end(b, *p);
        } else {
            break;
        }
    }
}

/// Индекс сразу за комментарием, начинающимся в `start` (EOF закрывает его).
fn comment_end(b: &[u8], start: usize) -> usize {
    b[start + 2..]
        .windows(2)
        .position(|w| w == b"*/")
        .map_or(b.len(), |i| start + 2 + i + 2)
}

/// Индекс сразу за строкой, открытой кавычкой в `start` (EOF закрывает её).
fn string_end(b: &[u8], start: usize) -> usize {
    let quote = b[start];
    let mut q = start + 1;
    while q < b.len() {
        match b[q] {
            b'\\' => q += 1,
            c if c == quote => return q + 1,
            _ => {}
        }
        q += 1;
    }
    b.len()
}

/// Индекс `)`, закрывающей `(` в `open`; строки и комментарии скобок не
/// считают. EOF закрывает блок (CSS Syntax L3 §5.4.7) → `b.len()`.
fn block_end(b: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    let mut q = open;
    while q < b.len() {
        match b[q] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return q;
                }
            }
            b'"' | b'\'' => {
                q = string_end(b, q);
                continue;
            }
            b'/' if b.get(q + 1) == Some(&b'*') => {
                q = comment_end(b, q);
                continue;
            }
            b'\\' => q += 1,
            _ => {}
        }
        q += 1;
    }
    b.len()
}

/// Позиции байтов из `targets` на верхнем уровне `b` — вне `()`/`[]`/`{}`,
/// строк и комментариев.
fn top_level_positions(b: &[u8], targets: &[u8]) -> Vec<(usize, u8)> {
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut q = 0usize;
    while q < b.len() {
        let c = b[q];
        match c {
            b'"' | b'\'' => {
                q = string_end(b, q);
                continue;
            }
            b'/' if b.get(q + 1) == Some(&b'*') => {
                q = comment_end(b, q);
                continue;
            }
            b'\\' => q += 1,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            // `<!--` is one <CDO-token>, not a `!` delimiter.
            b'!' if q > 0 && b[q - 1] == b'<' && b[q + 1..].starts_with(b"--") => {}
            _ if depth == 0 && targets.contains(&c) => found.push((q, c)),
            _ => {}
        }
        q += 1;
    }
    found
}

/// Ident characters, where a backslash plus the next byte is an escape (`--\30`, `--\d800`).
fn is_ident_with_escapes(b: &[u8]) -> bool {
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 1;
        } else if !is_ident_byte(b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80
}

/// `not`/`and`/`or` в позиции `*p`: идентификатор, за которым обязателен
/// пробел или комментарий — `not(`/`and(`/`or(` это функциональные токены.
fn match_keyword(b: &[u8], p: &mut usize, kw: &[u8]) -> bool {
    let end = *p + kw.len();
    if end > b.len() || !b[*p..end].eq_ignore_ascii_case(kw) {
        return false;
    }
    let spaced = b.get(end).is_some_and(u8::is_ascii_whitespace) || b[end..].starts_with(b"/*");
    if spaced {
        *p = end;
    }
    spaced
}

/// `<supports-condition>`; `None` — синтаксическая ошибка.
fn parse_condition(b: &[u8], p: &mut usize) -> Option<SupportsCondition> {
    skip_ws(b, p);
    if match_keyword(b, p, b"not") {
        return Some(SupportsCondition::Not(Box::new(parse_in_parens(b, p)?)));
    }
    let mut terms = vec![parse_in_parens(b, p)?];
    let mut is_and = None;
    loop {
        skip_ws(b, p);
        let this_and = if match_keyword(b, p, b"and") {
            true
        } else if match_keyword(b, p, b"or") {
            false
        } else {
            break;
        };
        // `and` и `or` на одном уровне не смешиваются (Conditional L3 §2).
        if *is_and.get_or_insert(this_and) != this_and {
            return None;
        }
        terms.push(parse_in_parens(b, p)?);
    }
    Some(match is_and {
        None => terms.remove(0),
        Some(true) => SupportsCondition::And(terms),
        Some(false) => SupportsCondition::Or(terms),
    })
}

/// `<supports-in-parens>`: `( … )`, функциональный тест или `<general-enclosed>`.
fn parse_in_parens(b: &[u8], p: &mut usize) -> Option<SupportsCondition> {
    skip_ws(b, p);
    if b.get(*p) == Some(&b'(') {
        let end = block_end(b, *p);
        let inner = &b[*p + 1..end];
        *p = (end + 1).min(b.len());
        return Some(parse_paren_content(inner));
    }
    // Функция: `selector(…)`, `font-tech(…)`, `font-format(…)` или другая
    // (`<general-enclosed>` — ложь). Голый идентификатор — ошибка.
    let name_end = (*p..b.len()).find(|&i| !is_ident_byte(b[i])).unwrap_or(b.len());
    if name_end == *p || b.get(name_end) != Some(&b'(') {
        return None;
    }
    let name = b[*p..name_end].to_ascii_lowercase();
    let end = block_end(b, name_end);
    let arg = std::str::from_utf8(&b[name_end + 1..end]).ok()?.trim();
    *p = (end + 1).min(b.len());
    Some(match name.as_slice() {
        // `font-tech( <font-tech> )` / `font-format( <font-format> )`
        // (CSS Conditional L4 §4 / CSS Fonts L4 §4.3): один ident-аргумент;
        // у `font-format` допустим legacy-строковый синтаксис (кавычки снимаем).
        b"selector" => SupportsCondition::Selector(arg.to_string()),
        b"at-rule" => parse_at_rule_arg(arg),
        b"font-tech" => SupportsCondition::FontTech(arg.to_ascii_lowercase()),
        b"font-format" => {
            let unquoted = arg.trim_matches(['"', '\'']).trim();
            SupportsCondition::FontFormat(unquoted.to_ascii_lowercase())
        }
        _ => SupportsCondition::Unknown,
    })
}

/// Аргумент `at-rule(@<ident>)`: только имя, без дескрипторов и прелюдии.
fn parse_at_rule_arg(arg: &str) -> SupportsCondition {
    match arg.strip_prefix('@') {
        Some(name) if !name.is_empty() && name.bytes().all(is_ident_byte) => {
            SupportsCondition::AtRule(name.to_ascii_lowercase())
        }
        _ => SupportsCondition::Unknown,
    }
}

/// Содержимое `( … )`: объявление `prop: value`, если на верхнем уровне есть
/// `:`, иначе вложенное условие. Не разобралось — `<general-enclosed>` (ложь).
fn parse_paren_content(inner: &[u8]) -> SupportsCondition {
    if let Some(&(colon, _)) = top_level_positions(inner, b":").first() {
        let (Ok(prop), Ok(value)) =
            (std::str::from_utf8(&inner[..colon]), std::str::from_utf8(&inner[colon + 1..]))
        else {
            return SupportsCondition::Unknown;
        };
        return parse_declaration(prop.trim(), value);
    }
    let mut q = 0usize;
    let cond = parse_condition(inner, &mut q);
    skip_ws(inner, &mut q);
    match cond {
        Some(c) if q >= inner.len() => c,
        _ => SupportsCondition::Unknown,
    }
}

/// `<property>: <value> [!important]?` внутри скобок. Значение по существу не
/// проверяется (BUG-1475), но структура декларации — да: у обычного свойства
/// значение не пусто и без `:`, `;` на верхнем уровне; `!important` — не более
/// одного раза, в конце и ровно этим словом.
fn parse_declaration(property: &str, value: &str) -> SupportsCondition {
    let pb = property.as_bytes();
    // `--` alone is reserved by CSS Variables L1 §2, not a custom property name.
    let valid_name = !pb.is_empty()
        && property != "--"
        && is_ident_with_escapes(pb)
        && !pb[0].is_ascii_digit()
        && !(pb[0] == b'-' && pb.get(1).is_some_and(u8::is_ascii_digit));
    if !valid_name {
        return SupportsCondition::Unknown;
    }
    let custom = property.starts_with("--");
    let vb = value.as_bytes();
    let marks = top_level_positions(vb, b":;!");
    if marks.iter().any(|&(_, c)| c == b';' || (c == b':' && !custom)) {
        return SupportsCondition::Unknown;
    }
    let mut bangs = marks.iter().filter(|&&(_, c)| c == b'!').map(|&(i, _)| i);
    let mut body = value;
    if let Some(bang) = bangs.next() {
        if bangs.next().is_some() || !is_important_tail(&vb[bang + 1..]) {
            return SupportsCondition::Unknown;
        }
        body = &value[..bang];
    }
    let body = body.trim();
    if body.is_empty() && !custom {
        return SupportsCondition::Unknown;
    }
    if !var_calls_valid(body) {
        return SupportsCondition::Unknown;
    }
    SupportsCondition::Decl { property: property.to_string(), value: body.to_string() }
}

/// Каждый `var()` в `value` разбирается по CSS Variables L1 §3: голова — ровно
/// один `--ident`, а fallback (`<declaration-value>`) не содержит `!` и `;` на
/// своём верхнем уровне (`var(--a,!)`, `var(--a,;)`, `var(1px)` — ошибка
/// разбора, и условие `@supports` над такой декларацией ложно, BUG-1500).
/// Fallback проверяется рекурсивно. Значение без `var(` — всегда `true`.
fn var_calls_valid(value: &str) -> bool {
    let b = value.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => i = string_end(b, i),
            b'/' if b.get(i + 1) == Some(&b'*') => i = comment_end(b, i),
            b'v' | b'V'
                if b.len() >= i + 4
                    && b[i..i + 4].eq_ignore_ascii_case(b"var(")
                    && (i == 0 || !is_ident_byte(b[i - 1])) =>
            {
                let open = i + 3;
                let close = block_end(b, open);
                let args = &b[open + 1..close];
                let comma = top_level_positions(args, b",").first().map(|&(p, _)| p);
                let head = &args[..comma.unwrap_or(args.len())];
                let head = String::from_utf8_lossy(head);
                let name = head.trim();
                if !(name.len() > 2 && name.starts_with("--") && is_ident_with_escapes(name.as_bytes())) {
                    return false;
                }
                if let Some(c) = comma {
                    let fallback = &args[c + 1..];
                    if !top_level_positions(fallback, b"!;").is_empty()
                        || !var_calls_valid(&String::from_utf8_lossy(fallback))
                    {
                        return false;
                    }
                }
                i = close + 1;
            }
            _ => i += 1,
        }
    }
    true
}

/// Хвост после `!`: ровно слово `important` (регистр не важен), пробелы и
/// комментарии вокруг допустимы.
fn is_important_tail(tail: &[u8]) -> bool {
    let mut q = 0usize;
    skip_ws(tail, &mut q);
    let start = q;
    while q < tail.len() && is_ident_byte(tail[q]) {
        q += 1;
    }
    let word = &tail[start..q];
    skip_ws(tail, &mut q);
    q >= tail.len() && word.eq_ignore_ascii_case(b"important")
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
                // Лишняя `)` — обычный токен прелюдии (CSS Syntax L3 §5.4.2), а не
                // отрицательная глубина: иначе `{` и `;` за ней уже не завершают
                // прелюдию и правило съедает следующее.
                depth = (depth - 1).max(0);
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
                    // Из nested @-правил в @supports поддержан только `@font-face`.
                    self.parse_conditional_body_at_rule(&LayerCondition::Supports(condition.clone()));
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
        Some(SupportsRule { condition, rules, pos: BlockPos::default() })
    }
}
