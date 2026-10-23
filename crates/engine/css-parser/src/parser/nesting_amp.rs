//! Вложенные правила с `&` не в голове prelude и с типовым селектором во главе
//! (CSS Nesting 1 §3.1, §4) — вынесено из `parser.rs`.

use super::*;

/// Конец prelude в `input`: байтовое смещение первого `{`/`;`/`}` верхнего уровня
/// (вне строк, комментариев, скобок) и сам символ; `None` — до конца текста.
pub(super) fn prelude_terminator(input: &str) -> (usize, Option<char>) {
    let b = input.as_bytes();
    let (mut i, mut depth) = (0, 0i32);
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            c @ (b'{' | b';' | b'}') if depth <= 0 => return (i, Some(c as char)),
            _ => {}
        }
        i += 1;
    }
    (input.len(), None)
}

/// `prelude` — это `ident :` (имя свойства и двоеточие, дальше сразу `{`).
fn is_property_colon_prelude(prelude: &str) -> bool {
    let Some(head) = prelude.trim_end().strip_suffix(':') else {
        return false;
    };
    let head = head.trim_end();
    !head.is_empty() && head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_') || !c.is_ascii())
}

impl Parser<'_> {
    /// Курсор на идентификаторе в теле стилевого правила: до ближайшего `;`/`}`
    /// верхнего уровня встречается `{` — это вложенное правило с типовым селектором.
    /// Пользовательское свойство (`--x: {…}`) остаётся объявлением. Курсор не сдвигается.
    ///
    /// Так же остаётся объявлением `ident : {…}`: `{}`-блок, которым значение
    /// начинается, — допустимое значение (CSS Syntax §5.4.4; `color: { [ var(--a) ] }`,
    /// BUG-1500), а `ident:` без продолжения не бывает селектором.
    pub(super) fn ident_led_block_ahead(&self) -> bool {
        let rest = &self.input[self.pos..];
        let (len, term) = prelude_terminator(rest);
        term == Some('{') && !rest.starts_with("--") && !is_property_colon_prelude(&rest[..len])
    }

    /// Prelude с ведущим `&` и ещё одним `&` дальше по цепочке (`& + &`, `&.a &`):
    /// общий путь [`Self::parse_nested_rule_with_inner_amp`], а не `& <комбинатор> <список>`,
    /// который второй `&` разобрать не может. `None` — второго `&` нет, курсор не сдвинут.
    pub(super) fn parse_nested_rule_with_extra_amp(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> Option<(Vec<Rule>, Vec<AtRuleOutcome>)> {
        let rest = &self.input[self.pos..];
        let (len, _) = prelude_terminator(rest);
        if rest[..len].matches('&').count() < 2 {
            return None;
        }
        self.parse_nested_rule_with_inner_amp(parent_sels, None)
    }

    /// CSS Nesting L1 §3.1: a nested prelude that mentions `&` somewhere other than
    /// its head (`.stuff, :is(&) #inner2 { }`) — `&` stands for `:is(<родитель>)`
    /// wherever it is, and a list entry without `&` is a descendant of the parent.
    /// `None` — в prelude до `{` нет `&` (обычный неявный путь); курсор не сдвинут.
    pub(super) fn parse_nested_rule_with_inner_amp(
        &mut self,
        parent_sels: &[ComplexSelector],
        combinator: Option<Combinator>,
    ) -> Option<(Vec<Rule>, Vec<AtRuleOutcome>)> {
        let start = self.pos;
        let (len, term) = prelude_terminator(&self.input[start..]);
        let end = start + len;
        let has_block = term == Some('{');
        let prelude = &self.input[start..end];
        if !has_block || parent_sels.is_empty() || !prelude.contains('&') {
            return None;
        }
        let parent_css = if let [only] = parent_sels {
            if only.tail.is_empty() {
                only.to_css_str()
            } else {
                format!(":is({})", only.to_css_str())
            }
        } else {
            let list: Vec<String> = parent_sels.iter().map(ComplexSelector::to_css_str).collect();
            format!(":is({})", list.join(", "))
        };
        let lead = match combinator {
            Some(Combinator::Child) => "> ",
            Some(Combinator::NextSibling) => "+ ",
            Some(Combinator::LaterSibling) => "~ ",
            _ => "",
        };
        let parts: Vec<String> = split_selector_commas(prelude)
            .into_iter()
            .map(|part| {
                let part = format!("{lead}{}", part.trim());
                substitute_nesting_selector(&part, &parent_css)
                    .unwrap_or_else(|| format!("{parent_css} {part}"))
            })
            .collect();
        let substituted = parts.join(", ");
        let mut sub = Parser::new(&substituted);
        let expanded_sels = sub.parse_selector_list();
        sub.skip_ws_and_comments();
        if expanded_sels.is_empty() || sub.peek().is_some() {
            self.pos = end;
            self.recover_to_block_end();
            return Some((vec![], vec![]));
        }
        self.pos = end;
        self.consume(); // '{'
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut rules = vec![Rule { selectors: expanded_sels, declarations }];
        rules.extend(sub_nested);
        Some((rules, sub_at))
    }
}
