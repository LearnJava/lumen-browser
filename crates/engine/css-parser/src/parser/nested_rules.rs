//! Разбор блока деклараций с вложенными правилами и at-правилами
//! (CSS Nesting 1 §3–§5) — вынесено из `parser.rs` без изменения поведения.

use super::*;

impl Parser<'_> {
    /// CSS Nesting L1 §3–§5 — parse declaration block that may contain nested rules and at-rules.
    /// Returns (declarations, flattened nested rules, nested at-rules).
    ///
    /// Handles:
    /// - `& selector { }` — explicit nesting with `&`
    /// - `.child { }`, `#id { }`, `[attr] { }`, `:hover { }`, `* { }` — implicit descendant nesting
    /// - `> .child { }`, `+ .sib { }`, `~ .sib { }` — implicit relative-combinator nesting
    /// - `@media / @supports / @layer / @container { }` — nested at-rules
    pub(super) fn parse_declaration_block_with_nesting(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Declaration>, Vec<Rule>, Vec<AtRuleOutcome>) {
        let mut decls = Vec::new();
        let mut nested: Vec<Rule> = Vec::new();
        let mut at_rules: Vec<AtRuleOutcome> = Vec::new();
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
                Some('&') => {
                    // Explicit nesting with `&`; второй `&` в prelude (`& + &`,
                    // `&.a &`) — подстановка `:is(<родитель>)` по всей цепочке.
                    let (r, a) = match self.parse_nested_rule_with_extra_amp(parent_sels) {
                        Some(parsed) => parsed,
                        None => self.parse_nested_rule_amp(parent_sels),
                    };
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §4: implicit descendant — `.foo {}`, `#id {}`, `[attr] {}`,
                // `:pseudo {}`, `* {}` cannot start a property name, so treat as nested rule.
                Some('.') | Some('#') | Some('[') | Some(':') | Some('*') => {
                    let (r, a) = self.parse_implicit_nested_rule(parent_sels, None);
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §4: implicit relative — `> .foo {}`, `+ .sib {}`, `~ .sib {}`.
                Some('>') | Some('+') | Some('~') => {
                    // SAFETY: we just peeked this char, consume() cannot return None here.
                    let c = self.consume().unwrap_or('>');
                    let comb = match c {
                        '+' => Combinator::NextSibling,
                        '~' => Combinator::LaterSibling,
                        _ => Combinator::Child, // '>'
                    };
                    self.skip_ws_and_comments();
                    let (r, a) = self.parse_implicit_nested_rule(parent_sels, Some(comb));
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §5: nested at-rule — except `@apply`
                // (CSS Mixins L1), which is a declaration-position at-rule,
                // not a nested conditional-group rule: pushed as a marker
                // `Declaration` (see `MIXIN_APPLY_MARKER`'s doc comment) so
                // it keeps its exact source position relative to sibling
                // declarations for cascade ordering.
                Some('@') => {
                    let at_start = self.pos;
                    self.consume(); // '@'
                    let ident = self.parse_ident().unwrap_or_default();
                    if ident.eq_ignore_ascii_case("nest") {
                        let (r, a) = self.parse_legacy_nest_rule(parent_sels);
                        nested.extend(r);
                        at_rules.extend(a);
                    } else if ident.eq_ignore_ascii_case("apply") {
                        let raw_start = self.pos;
                        if self.parse_apply_rule().is_some() {
                            let raw = self.input[raw_start..self.pos].to_string();
                            decls.push(Declaration {
                                property: MIXIN_APPLY_MARKER.to_string(),
                                value: raw,
                                important: false,
                            });
                        }
                    } else {
                        self.pos = at_start;
                        let ats = self.parse_nested_at_rule(parent_sels);
                        at_rules.extend(ats);
                    }
                }
                // CSS Nesting L1 §4 (relaxed): `div { }` — an ident-led prelude that
                // reaches a `{` before any `;`/`}` is a nested rule, not a declaration
                // (custom properties keep `--x: { … }`).
                Some(c) if (c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()) && self.ident_led_block_ahead() => {
                    let (r, a) = self.parse_implicit_nested_rule(parent_sels, None);
                    nested.extend(r);
                    at_rules.extend(a);
                }
                _ => match self.parse_declaration() {
                    Some(d) => decls.push(d),
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        (decls, nested, at_rules)
    }

    /// Legacy `@nest <selector-list> { declarations }` (css-nesting-1 ED до
    /// 2023-02; в текущей спеке удалён, но старые таблицы стилей его содержат).
    /// Курсор — сразу после `@nest`. Каждый complex-селектор prelude обязан
    /// содержать хотя бы один `&` (иначе правило невалидно и блок пропускается
    /// целиком); `&` заменяется на родительский селектор в любой позиции
    /// (`@nest .dark & { }`, `@nest :not(&) { }`), а не только в начале, как в
    /// `& sel` — этим `@nest` и отличается от современной формы. Семантика
    /// подстановки — `:is(<родитель>)` (Nesting L1 §3.1); одиночный compound без
    /// хвоста подставляется текстом как есть, чтобы не раздувать специфичность
    /// и не терять pseudo-element-ы.
    fn parse_legacy_nest_rule(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        self.skip_ws_and_comments();
        let prelude_start = self.pos;
        // Prelude до `{` верхнего уровня; строки и скобки не завершают prelude.
        let mut depth: i32 = 0;
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek() {
            match quote {
                Some(q) => {
                    if c == '\\' {
                        self.consume();
                    } else if c == q {
                        quote = None;
                    }
                }
                None => match c {
                    '"' | '\'' => quote = Some(c),
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth -= 1,
                    '{' | ';' | '}' if depth <= 0 => break,
                    _ => {}
                },
            }
            self.consume();
        }
        if self.peek() != Some('{') || parent_sels.is_empty() {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        let prelude = self.input[prelude_start..self.pos].to_string();
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
        let Some(substituted) = substitute_nesting_selector(&prelude, &parent_css) else {
            self.recover_to_block_end();
            return (vec![], vec![]);
        };
        let mut sub = Parser::new(&substituted);
        let expanded_sels = sub.parse_selector_list();
        sub.skip_ws_and_comments();
        if expanded_sels.is_empty() || sub.peek().is_some() {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.consume(); // '{'
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut result = vec![Rule { selectors: expanded_sels, declarations }];
        result.extend(sub_nested);
        (result, sub_at)
    }

    /// Parse `& [combinator] selector-list { declarations }` and expand into flat rules.
    /// The `&` has already been peeked but not consumed.
    fn parse_nested_rule_amp(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        self.consume(); // consume '&'
        let had_ws = self.skip_ws_and_comments_track();
        // Determine if there's an explicit combinator after &.
        let combinator: Option<Combinator> = match self.peek() {
            Some('>') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::Child) }
            Some('+') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::NextSibling) }
            Some('~') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::LaterSibling) }
            Some('{') => None, // bare `& { }` — same element as parent
            _ if had_ws => Some(Combinator::Descendant),
            _ => None, // `&.class` / `&[attr]` / `&#id` — compound join
        };
        // Parse the selector list that follows (may be empty for bare `& { }`).
        let nested_sels: Vec<ComplexSelector> = if self.peek() == Some('{') {
            vec![] // bare `& { }` — same element
        } else {
            let s = self.parse_selector_list();
            if s.is_empty() {
                self.recover_to_block_end();
                return (vec![], vec![]);
            }
            s
        };
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.consume(); // '{'
        // Expand: combine each parent selector with each nested selector.
        let expanded_sels = if nested_sels.is_empty() {
            parent_sels.to_vec() // bare `& { }` = same as parent
        } else {
            expand_nesting(parent_sels, combinator, &nested_sels)
        };
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut result = vec![Rule { selectors: expanded_sels, declarations }];
        result.extend(sub_nested);
        (result, sub_at)
    }

    /// CSS Nesting L1 §4: implicit nesting — `.child { }` inside a rule block
    /// is treated as `& .child { }` (descendant). Called when we see a selector-
    /// start token (`.`, `#`, `[`, `:`, `*`) without an explicit `&`.
    /// `combinator` — pre-parsed explicit combinator (`>`, `+`, `~`), or `None`
    /// for implicit descendant.
    pub(super) fn parse_implicit_nested_rule(
        &mut self,
        parent_sels: &[ComplexSelector],
        combinator: Option<Combinator>,
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        if let Some(parsed) = self.parse_nested_rule_with_inner_amp(parent_sels, combinator) {
            return parsed;
        }
        let nested_sels = self.parse_selector_list();
        if nested_sels.is_empty() {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.consume(); // '{'
        // Implicit nesting without explicit combinator → descendant.
        let comb = combinator.unwrap_or(Combinator::Descendant);
        let expanded_sels = expand_nesting(parent_sels, Some(comb), &nested_sels);
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut rules = vec![Rule { selectors: expanded_sels, declarations }];
        rules.extend(sub_nested);
        (rules, sub_at)
    }

    /// Парсит тело group at-rule (`@container`/`@media`/…), которое не вложено
    /// ни в какое qualified-правило (`parent_sels` пуст) — то есть ведёт себя
    /// как обычный rule-list stylesheet-уровня: bare-объявления здесь
    /// невалидны (нет селектора, к которому их привязать), поэтому любой
    /// не-`@`-токен — это обычное qualified-правило с произвольным селектором
    /// (включая голый type-селектор вроде `p`, который CSS Nesting L1 §4
    /// запрещает как неоднозначный только внутри уже открытого style-правила).
    /// Вложенные at-rules всплывают отдельным `Vec` (плоская модель). Курсор
    /// должен стоять сразу после `{`, потребляет закрывающую `}`.
    pub(super) fn parse_bare_group_body(&mut self) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        let mut rules = Vec::new();
        let mut at_rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    at_rules.extend(self.parse_nested_at_rule(&[]));
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, nested_at)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                        at_rules.extend(nested_at);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        (rules, at_rules)
    }

    /// Парсит тело nested conditional-group at-rule (после уже consume-нутого
    /// `{`) с полной рекурсией CSS Nesting L1 §5: bare-декларации сворачиваются
    /// в синтетическое правило с селекторами `parent_sels`, вложенные правила
    /// добавляются следом, вложенные at-rules возвращаются отдельным `Vec`
    /// (всплывают на stylesheet-уровень). Общий код для веток `@media` /
    /// `@supports` / `@layer` / `@container` / `@scope` в
    /// [`Self::parse_nested_at_rule`]. Курсор должен стоять сразу после `{`.
    /// Пустой `parent_sels` означает, что мы на самом деле не вложены ни в
    /// какое qualified-правило (например, `@media` внутри `@container` на
    /// stylesheet-уровне) — тогда делегирует в [`Self::parse_bare_group_body`],
    /// у которой другая грамматика тела (rule-list, а не declarations+nesting).
    fn parse_nested_group_body(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        if parent_sels.is_empty() {
            return self.parse_bare_group_body();
        }
        let (decls, inner_rules, inner_at) =
            self.parse_declaration_block_with_nesting(parent_sels);
        let mut rules = Vec::new();
        if !decls.is_empty() {
            rules.push(Rule {
                selectors: parent_sels.to_vec(),
                declarations: decls,
            });
        }
        rules.extend(inner_rules);
        (rules, inner_at)
    }

    /// CSS Nesting L1 §5: nested at-rule inside a qualified rule.
    /// Example: `.parent { @media (min-width: 800px) { color: red; } }`
    /// expands to: `@media (min-width: 800px) { .parent { color: red; } }`.
    /// Supports `@media`, `@supports`, `@layer`, `@container`, `@scope`.
    pub(super) fn parse_nested_at_rule(&mut self, parent_sels: &[ComplexSelector]) -> Vec<AtRuleOutcome> {
        let start = self.pos;
        self.consume(); // '@'
        let name = self.parse_ident().unwrap_or_default();
        self.skip_ws_and_comments();

        if name.eq_ignore_ascii_case("media") {
            let query_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' || c == ';' {
                    break;
                }
                self.consume();
            }
            // BUG-793: nested `@media;` has no block — consume the `;`
            // instead of leaving the parser stranded right before it (see
            // the top-level `parse_media_rule` fix for the failure mode).
            if self.peek() == Some(';') {
                self.consume();
                return vec![];
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let query_str = self.input[query_start..self.pos].trim();
            let query = parse_media_query(query_str);
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes = vec![AtRuleOutcome::Media(MediaRule { query, rules, pos: BlockPos::default() })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("supports") {
            let cond_start = self.pos;
            let mut depth: i32 = 0;
            while let Some(c) = self.peek() {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    // Лишняя `)` — обычный токен прелюдии (BUG-1493), не минус-глубина.
                    depth = (depth - 1).max(0);
                } else if c == '{' && depth == 0 {
                    break;
                } else if c == ';' && depth == 0 {
                    // BUG-793: nested `@supports;` has no block.
                    break;
                }
                self.consume();
            }
            if self.peek() == Some(';') {
                self.consume();
                return vec![];
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let cond_str = self.input[cond_start..self.pos].trim();
            let condition = parse_supports_condition(cond_str);
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes =
                vec![AtRuleOutcome::Supports(SupportsRule { condition, rules, pos: BlockPos::default() })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("layer") {
            let names_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' || c == ';' {
                    break;
                }
                self.consume();
            }
            let prelude = self.input[names_start..self.pos].trim();
            if self.peek() == Some(';') {
                self.consume();
                return vec![];
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let layer_name = if prelude.is_empty() {
                None
            } else {
                Some(prelude.to_string())
            };
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes = vec![AtRuleOutcome::LayerBlock {
                name: layer_name,
                rules,
                // `@layer` nested inside an ordinary style rule's own body
                // (CSS Nesting) goes through `parse_nested_group_body`, not
                // `parse_layer_at_rule`'s block-form loop — this slice's
                // `@mixin`-inside-`@layer` special case (BUG-518 срез 3)
                // only covers the latter; a `@mixin` here remains
                // unsupported, same as before this change.
                mixin_rules: Vec::new(),
                nested: Vec::new(),
            }];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("container") {
            // CSS Containment L3 §3: опциональное имя перед condition — тот же
            // разбор прелюдии, что и для top-level `@container`.
            let Some((cont_name, condition)) = self.parse_container_prelude() else {
                return vec![];
            };
            self.consume(); // '{'
            let (rules, mut inner_at) = self.parse_nested_group_body(parent_sels);
            super::at_rules::enclose_container_rules(&mut inner_at, &cont_name, &condition);
            let mut outcomes = vec![AtRuleOutcome::Container(ContainerRule {
                name: cont_name,
                condition,
                enclosing: Vec::new(),
                rules,
            })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("scope") {
            // CSS Cascade L6 §3: `@scope (<root>)? [to (<limit>)]?` вложенный в
            // qualified-правило. Прелюдия — тот же разбор, что и для top-level
            // `@scope`; тело — рекурсивный declaration-block с `parent_sels`.
            let (root, limit) = self.parse_scope_prelude();
            self.skip_ws_and_comments();
            // BUG-793: nested `@scope;` has no block — consume the `;`.
            if self.peek() == Some(';') {
                self.consume();
                return vec![];
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes =
                vec![AtRuleOutcome::Scope(ScopeRule { root, limit, rules })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        // Unknown nested at-rule — skip the block.
        self.pos = start;
        self.skip_at_rule();
        vec![]
    }
}
