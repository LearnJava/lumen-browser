//! Разбор псевдоклассов и псевдоэлементов (включая функциональные), `:lang()`,
//! относительных селекторов `:has()` и `An+B [of S]` — вынесено из
//! `selectors.rs` без изменения поведения.

use super::*;

impl Parser<'_> {
    pub(crate) fn parse_pseudo(&mut self) -> Option<SimpleSelector> {
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
            if is_element {
                // Функциональный pseudo-element (например ::slotted(...))
                let pe = self.parse_functional_pseudo_element(&lower);
                self.skip_to_paren_close();
                return Some(SimpleSelector::PseudoElement(pe.unwrap_or(
                    PseudoElementKind::Unknown(name)
                )));
            } else {
                // Функциональный pseudo-class (например :is(...))
                let pc = self.parse_functional_pseudo_body(&lower);
                self.skip_to_paren_close();
                return Some(SimpleSelector::PseudoClass(pc.unwrap_or_else(|| {
                    PseudoClass::Unsupported(name.clone())
                })));
            }
        }
        // CSS 2.1 §5.12.3: четыре унаследованных псевдоэлемента допускают и
        // однодвоеточную запись (`a:before`).
        let legacy_single_colon =
            matches!(lower.as_str(), "before" | "after" | "first-line" | "first-letter");
        if is_element || legacy_single_colon {
            let pe = match lower.as_str() {
                "before" => PseudoElementKind::Before,
                "after" => PseudoElementKind::After,
                "first-line" => PseudoElementKind::FirstLine,
                "first-letter" => PseudoElementKind::FirstLetter,
                "marker" => PseudoElementKind::Marker,
                "selection" => PseudoElementKind::Selection,
                "target-text" => PseudoElementKind::TargetText,
                "spelling-error" => PseudoElementKind::SpellingError,
                "grammar-error" => PseudoElementKind::GrammarError,
                "placeholder" => PseudoElementKind::Placeholder,
                "checkmark" => PseudoElementKind::Checkmark,
                "picker-icon" => PseudoElementKind::PickerIcon,
                "scroll-marker" => PseudoElementKind::ScrollMarker,
                "scroll-marker-group" => PseudoElementKind::ScrollMarkerGroup,
                "column" => PseudoElementKind::Column,
                "view-transition" => PseudoElementKind::ViewTransition,
                _ => PseudoElementKind::Unknown(name),
            };
            return Some(SimpleSelector::PseudoElement(pe));
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
            "placeholder-shown" => PseudoClass::PlaceholderShown,
            "required" => PseudoClass::Required,
            "optional" => PseudoClass::Optional,
            "read-only" => PseudoClass::ReadOnly,
            "read-write" => PseudoClass::ReadWrite,
            "disabled" => PseudoClass::Disabled,
            "enabled" => PseudoClass::Enabled,
            "checked" => PseudoClass::Checked,
            "indeterminate" => PseudoClass::Indeterminate,
            "default" => PseudoClass::Default,
            "hover" => PseudoClass::Hover,
            "focus" => PseudoClass::Focus,
            "active" => PseudoClass::Active,
            "focus-within" => PseudoClass::FocusWithin,
            "focus-visible" => PseudoClass::FocusVisible,
            "link" => PseudoClass::Link,
            "visited" => PseudoClass::Visited,
            "any-link" => PseudoClass::AnyLink,
            "valid" => PseudoClass::Valid,
            "invalid" => PseudoClass::Invalid,
            "user-valid" => PseudoClass::UserValid,
            "user-invalid" => PseudoClass::UserInvalid,
            "in-range" => PseudoClass::InRange,
            "out-of-range" => PseudoClass::OutOfRange,
            "scope" => PseudoClass::Scope,
            "target" => PseudoClass::Target,
            "target-within" => PseudoClass::TargetWithin,
            "defined" => PseudoClass::Defined,
            "fullscreen" => PseudoClass::Fullscreen,
            "modal" => PseudoClass::Modal,
            "popover-open" => PseudoClass::PopoverOpen,
            "interest-source" => PseudoClass::InterestSource,
            "interest-target" => PseudoClass::InterestTarget,
            "current" => PseudoClass::Current,
            "past" => PseudoClass::Past,
            "future" => PseudoClass::Future,
            "host" => PseudoClass::Host(None),
            "heading" => PseudoClass::Heading(None),
            _ => PseudoClass::Unsupported(name),
        };
        Some(SimpleSelector::PseudoClass(pc))
    }

    /// Парсит тело `:foo(...)` для известных функциональных pseudo. Возвращает
    /// `None` для неизвестных или невалидных тел — caller обернёт в Unsupported
    /// и проглотит остаток до `)`.
    pub(crate) fn parse_functional_pseudo_body(&mut self, name_lower: &str) -> Option<PseudoClass> {
        match name_lower {
            "nth-child" => {
                let (spec, of) = self.parse_nth_spec_with_of()?;
                Some(PseudoClass::NthChild(spec, of))
            }
            "nth-last-child" => {
                let (spec, of) = self.parse_nth_spec_with_of()?;
                Some(PseudoClass::NthLastChild(spec, of))
            }
            "nth-of-type" => Some(PseudoClass::NthOfType(self.parse_nth_spec()?)),
            "nth-last-of-type" => Some(PseudoClass::NthLastOfType(self.parse_nth_spec()?)),
            "not" => {
                // CSS Selectors L4 §5.4: внутри `:not(...)` допустим полный
                // selector-list (complex-селекторы с combinator-ами), nested
                // `:not(:not(...))` тоже разрешён.
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Not(list))
            }
            "is" => {
                // Selectors L4 §3.4: forgiving-список — пустой или с
                // невалидными элементами аргумент валиден (ничего не выбирает).
                let list = self.parse_forgiving_selector_list();
                if self.peek() != Some(')') {
                    return None;
                }
                Some(PseudoClass::Is(list))
            }
            "where" => {
                let list = self.parse_forgiving_selector_list();
                if self.peek() != Some(')') {
                    return None;
                }
                Some(PseudoClass::Where(list))
            }
            "has" => {
                // CSS Selectors L4 §17.2: relative-selector-list. Каждый
                // элемент — combinator + selector, или просто selector
                // (implicit descendant).
                let list = self.parse_relative_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Has(list))
            }
            "host" => {
                // CSS Scoping L1 §6.1: `:host` и `:host(selector-list)`.
                // При парсинге `:host(...)` парсим selector-list внутри.
                // Если список пустой — невалидно.
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoClass::Host(Some(list)))
            }
            "dir" => {
                // CSS Selectors L4 §13.2: `:dir(<ident>)`; `ltr`/`rtl`
                // (ASCII case-insensitive) матчат по directionality, любой
                // другой идентификатор валиден, но ничего не выбирает.
                self.skip_ws_and_comments();
                let kw = self.parse_ident()?;
                self.skip_ws_and_comments();
                if self.peek() != Some(')') {
                    return None;
                }
                match kw.to_ascii_lowercase().as_str() {
                    "ltr" => Some(PseudoClass::Dir(DirArg::Ltr)),
                    "rtl" => Some(PseudoClass::Dir(DirArg::Rtl)),
                    _ => Some(PseudoClass::DirOther(kw)),
                }
            }
            "lang" => {
                // CSS Selectors L4 §8.2: список `<ident> | <string>` —
                // language range-ы (RFC 4647 §3.3.2). Нормализуем к lowercase
                // для case-insensitive сопоставления; в ident допустим `*`
                // (в том числе экранированный `\*`). Невалидный элемент
                // (в т.ч. ident с цифры) делает невалидным весь псевдокласс.
                let mut tags: Vec<String> = Vec::new();
                loop {
                    self.skip_ws_and_comments();
                    let tag = match self.peek() {
                        Some(q @ ('"' | '\'')) => self.parse_lang_string(q)?,
                        _ => self.parse_lang_ident()?,
                    };
                    tags.push(tag.to_ascii_lowercase());
                    self.skip_ws_and_comments();
                    if self.peek() == Some(',') {
                        self.consume();
                    } else {
                        break;
                    }
                }
                Some(PseudoClass::Lang(tags))
            }
            "state" => {
                // CSS Selectors L4 §17.4: single custom-ident argument,
                // case-sensitive (custom-ident, not a BCP-47 tag — no
                // lowercasing, unlike `:lang()`).
                self.skip_ws_and_comments();
                let ident = self.parse_ident()?;
                self.skip_ws_and_comments();
                if self.peek() != Some(')') {
                    return None;
                }
                Some(PseudoClass::State(ident))
            }
            "heading" => {
                // `:heading(<integer>)` — single plain <integer>, no an+b
                // formula and no `of <selector-list>` clause (unlike
                // `:nth-child`), so a raw-token collect + `str::parse` is
                // enough; caller falls back to `Unsupported` on a bad token.
                self.skip_ws_and_comments();
                let mut raw = String::new();
                while let Some(c) = self.peek() {
                    if c == ')' {
                        break;
                    }
                    raw.push(c);
                    self.consume();
                }
                let n: i32 = raw.trim().parse().ok()?;
                Some(PseudoClass::Heading(Some(n)))
            }
            _ => None,
        }
    }

    /// Строка `:lang("…")`: позиция на открывающей кавычке. `None`, если
    /// строка не закрыта до конца ввода.
    fn parse_lang_string(&mut self, quote: char) -> Option<String> {
        self.consume();
        let mut out = String::new();
        loop {
            match self.consume()? {
                c if c == quote => return Some(out),
                '\\' => out.push(self.consume()?),
                c => out.push(c),
            }
        }
    }

    /// Идентификатор `:lang(…)`: буквы, цифры, `-`, `_`, не-ASCII, `*` и
    /// экранированные символы; первый неэкранированный символ не цифра.
    fn parse_lang_ident(&mut self) -> Option<String> {
        let mut out = String::new();
        let mut first = true;
        while let Some(c) = self.peek() {
            if c == '\\' {
                self.consume();
                out.push(self.consume()?);
            } else if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '*') || !c.is_ascii() {
                if first && c.is_ascii_digit() {
                    return None;
                }
                out.push(c);
                self.consume();
            } else {
                break;
            }
            first = false;
        }
        if out.is_empty() { None } else { Some(out) }
    }

    /// Парсит relative-selector-list для `:has()`. Каждый элемент — опциональный
    /// ведущий combinator (`>`, `+`, `~`) + сам complex selector.
    pub(crate) fn parse_relative_selector_list(&mut self) -> Vec<RelativeSelector> {
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None | Some(')') => break,
                _ => {}
            }
            let combinator = match self.peek() {
                Some('>') => { self.consume(); Some(Combinator::Child) }
                Some('+') => { self.consume(); Some(Combinator::NextSibling) }
                Some('~') => { self.consume(); Some(Combinator::LaterSibling) }
                _ => None,
            };
            self.skip_ws_and_comments();
            let Some(selector) = self.parse_complex_selector() else {
                // Невалидный selector — пропускаем до запятой/конца.
                while let Some(c) = self.peek() {
                    if c == ',' || c == ')' { break; }
                    self.consume();
                }
                if self.peek() == Some(',') { self.consume(); }
                continue;
            };
            out.push(RelativeSelector { combinator, selector });
            self.skip_ws_and_comments();
            if self.peek() == Some(',') {
                self.consume();
            } else {
                break;
            }
        }
        out
    }

    /// Парсит тело функционального pseudo-element (например `::slotted(...)` или `::highlight(...)`).
    /// Возвращает `None` для неизвестных или невалидных тел — caller обернёт
    /// в `Unknown(name)` и проглотит остаток до `)`.
    pub(crate) fn parse_functional_pseudo_element(&mut self, name_lower: &str) -> Option<PseudoElementKind> {
        match name_lower {
            "slotted" => {
                // CSS Scoping L1 §6.2: `::slotted(selector-list)` матчит element,
                // который слотирован через этот `<slot>` и матчит хотя бы один
                // из селекторов списка.
                let list = self.parse_selector_list();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || list.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::Slotted(Some(list)))
            }
            "part" => {
                // CSS Shadow Parts L1 §2: `::part(<ident>+)` — непустой список
                // имён, разделённых пробелами. Не идент (`1a`, строка) — весь
                // селектор невалиден (вызывающий оборачивает `None` в `Unknown`).
                let mut names = Vec::new();
                loop {
                    self.skip_ws_and_comments();
                    if self.peek() == Some(')') {
                        break;
                    }
                    names.push(self.parse_ident()?);
                }
                if names.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::Part(names))
            }
            "highlight" => {
                // CSS Highlight API L1 §3: `::highlight(name)` матчит элемент,
                // который стилизуется через highlight с заданным именем.
                self.skip_ws_and_comments();
                let name = self.parse_ident().unwrap_or_default();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || name.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::Highlight(name))
            }
            "picker" => {
                // HTML/CSS «Customizable Select»: `::picker(select)` targets the
                // pop-up picker part of a `base-select` `<select>`. Only the
                // `select` ident is defined today; store it for forward-compat.
                self.skip_ws_and_comments();
                let arg = self.parse_ident().unwrap_or_default();
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || arg.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::Picker(arg.to_ascii_lowercase()))
            }
            "scroll-button" => {
                // CSS Overflow L5 §scroll-buttons: `::scroll-button(<direction>
                // | *)` — the argument is either a direction ident or the
                // literal `*` (not an ident, needs its own branch). A quoted
                // string (`'up'`), a bare number, or a comma-separated list
                // all fail to produce a single trailing `)` and are rejected
                // below, same as `arg.is_empty()` for `::scroll-button()`.
                self.skip_ws_and_comments();
                let arg = if self.peek() == Some('*') {
                    self.consume();
                    "*".to_string()
                } else {
                    self.parse_ident().unwrap_or_default().to_ascii_lowercase()
                };
                self.skip_ws_and_comments();
                if self.peek() != Some(')') || arg.is_empty() {
                    return None;
                }
                Some(PseudoElementKind::ScrollButton(arg))
            }
            "view-transition-group"
            | "view-transition-image-pair"
            | "view-transition-old"
            | "view-transition-new" => {
                // CSS View Transitions L1 §6.2–6.5: the argument is a
                // `<custom-ident>` (the `view-transition-name` to target) or
                // `*`. The CSS-wide keywords and `none` are not valid
                // `<custom-ident>`s here (`none` is excluded from
                // `view-transition-name`'s ident too). The L2 `name.class`
                // form needs `view-transition-class`, which is not modeled —
                // it is rejected like any other trailing junk.
                self.skip_ws_and_comments();
                let arg = if self.peek() == Some('*') {
                    self.consume();
                    "*".to_string()
                } else {
                    self.parse_ident().unwrap_or_default()
                };
                self.skip_ws_and_comments();
                if self.peek() != Some(')')
                    || arg.is_empty()
                    || matches!(
                        arg.to_ascii_lowercase().as_str(),
                        "none" | "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default"
                    )
                {
                    return None;
                }
                Some(match name_lower {
                    "view-transition-group" => PseudoElementKind::ViewTransitionGroup(arg),
                    "view-transition-image-pair" => PseudoElementKind::ViewTransitionImagePair(arg),
                    "view-transition-old" => PseudoElementKind::ViewTransitionOld(arg),
                    _ => PseudoElementKind::ViewTransitionNew(arg),
                })
            }
            _ => None,
        }
    }

    /// Парсит `an+b`, число или ключевые слова `odd`/`even`. Останавливается на
    /// `)` или конце ввода — caller съест `)` через `skip_to_paren_close`.
    /// **Не** парсит `of <selector-list>` — для этого `parse_nth_spec_with_of`;
    /// этот метод оставлен для `:nth-of-type` / `:nth-last-of-type` (per spec
    /// они не поддерживают `of` clause).
    pub(crate) fn parse_nth_spec(&mut self) -> Option<NthSpec> {
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

    /// Парсит `an+b [of <selector-list>]` для `:nth-child` / `:nth-last-child`
    /// (CSS Selectors L4 §6.6.5.1). Возвращает `(NthSpec, Option<list>)`:
    /// `None` для list означает отсутствие `of`-clause; `Some(non-empty list)`
    /// — фильтр siblings. Пустой `of` clause (`of` без следующего selector-а)
    /// → возврат `None` из всего метода — caller fallback-ит на `Unsupported`.
    ///
    /// Алгоритм: собираем raw-tokens до встречи `of` (ASCII case-insensitive,
    /// окружённого whitespace или скобками — чтобы `2nof.x` не схлопывалось)
    /// либо `)`. Затем nth-spec парсится из собранного prefix; если за ним
    /// есть `of` — парсим selector-list до `)`.
    pub(crate) fn parse_nth_spec_with_of(&mut self) -> Option<(NthSpec, Option<Vec<ComplexSelector>>)> {
        self.skip_ws_and_comments();
        let mut raw = String::new();
        // Собираем nth-spec токены до встречи `of`-keyword (отделённого
        // whitespace по обе стороны: spec требует whitespace вокруг `of`,
        // чтобы `2nof.x` не схлопнулось как nth-spec `2nof` + `.x`).
        // Без `of` — собираем всё до `)`, как старый `parse_nth_spec`.
        loop {
            let saved = self.pos;
            self.skip_ws_and_comments();
            let after_ws = self.pos;
            let Some(c) = self.peek() else { break };
            if c == ')' {
                self.pos = saved;
                break;
            }
            if after_ws > saved && self.peek_ident_matches_of() {
                // Откатываемся к началу whitespace, чтобы of-clause увидел
                // boundary сам.
                self.pos = saved;
                break;
            }
            if after_ws > saved {
                raw.push(' ');
            }
            raw.push(c);
            self.consume();
        }
        let spec = parse_nth_spec_str(raw.trim())?;
        self.skip_ws_and_comments();
        if !self.peek_ident_matches_of() {
            return Some((spec, None));
        }
        self.consume(); // 'o'
        self.consume(); // 'f'
        self.skip_ws_and_comments();
        let list = self.parse_selector_list();
        self.skip_ws_and_comments();
        if list.is_empty() || self.peek() != Some(')') {
            return None;
        }
        Some((spec, Some(list)))
    }

    /// Возвращает true, если следующие 2 байта — `of` (ASCII case-insensitive)
    /// И за ними следует НЕ-ident-continuation байт (whitespace, `)`, EOF,
    /// и т.д.). Без consume.
    pub(crate) fn peek_ident_matches_of(&self) -> bool {
        let b = self.input.as_bytes();
        let p = self.pos;
        if p + 1 >= b.len() {
            return false;
        }
        if !b[p].eq_ignore_ascii_case(&b'o') || !b[p + 1].eq_ignore_ascii_case(&b'f') {
            return false;
        }
        match b.get(p + 2) {
            None => true,
            Some(&c) => !c.is_ascii_alphanumeric() && c != b'-' && c != b'_',
        }
    }

    pub(crate) fn skip_to_paren_close(&mut self) {
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
}
