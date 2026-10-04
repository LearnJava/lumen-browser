//! Накопитель `@layer`-данных одного прохода разбора листа.
//!
//! Вырезано из `parser.rs` (SPLIT-CP2) без изменения поведения.

use super::*;

/// Накопитель `@layer`-данных одного прохода [`Parser::parse_stylesheet`]:
/// порядок объявления layer-ов, их блоки и счётчик анонимных имён.
#[derive(Default)]
pub(super) struct LayerState {
    pub(super) order: Vec<String>,
    pub(super) blocks: Vec<LayerRule>,
    pub(super) anon_counter: usize,
    /// `@mixin`-ы из `@layer`-блоков, уже с проставленным `layer`; вызывающая
    /// сторона забирает их через [`Self::take_outputs`] после `register`.
    pub(super) mixins: Vec<MixinRule>,
    /// Layer-независимые at-rules из `@layer`-блоков (`@font-face` и т.п.) —
    /// вызывающая сторона обрабатывает их как верхнеуровневые.
    pub(super) hoisted: Vec<AtRuleOutcome>,
}

impl LayerState {
    /// Забирает накопленные `register`-ом `@mixin`-ы и поднятые at-rules.
    pub(super) fn take_outputs(&mut self) -> (Vec<MixinRule>, Vec<AtRuleOutcome>) {
        (std::mem::take(&mut self.mixins), std::mem::take(&mut self.hoisted))
    }

    /// Регистрирует имя в порядке объявления (первое упоминание побеждает).
    pub(super) fn declare(&mut self, name: String) {
        if !self.order.iter().any(|e| e == &name) {
            self.order.push(name);
        }
    }

    /// Блок `@layer [name] { … }`. `prefix` — полное имя внешнего layer-а
    /// (`None` на верхнем уровне): вложенный layer получает dotted-имя
    /// `outer.inner` (Cascade L5 §6.4.2). Содержимое `nested`:
    /// `@media`/`@supports` превращаются в [`LayerRule`] с условием, вложенные
    /// `@layer` регистрируются рекурсивно, всё прочее уходит в `hoisted` —
    /// вызывающая сторона обрабатывает это как обычные верхнеуровневые
    /// at-rules.
    pub(super) fn register(
        &mut self,
        prefix: Option<&str>,
        name: Option<String>,
        rules: Vec<Rule>,
        mixins: Vec<MixinRule>,
        nested: Vec<AtRuleOutcome>,
    ) {
        let local = name.unwrap_or_else(|| {
            self.anon_counter += 1;
            format!("__anon_{}__", self.anon_counter)
        });
        let full = match prefix {
            Some(p) => format!("{p}.{local}"),
            None => local,
        };
        self.declare(full.clone());
        for mut m in mixins {
            m.layer = Some(full.clone());
            self.mixins.push(m);
        }
        self.blocks.push(LayerRule { name: full.clone(), rules, condition: None });
        for o in nested {
            match o {
                AtRuleOutcome::Media(m) => self.blocks.push(LayerRule {
                    name: full.clone(),
                    rules: m.rules,
                    condition: Some(LayerCondition::Media(m.query)),
                }),
                AtRuleOutcome::Supports(sr) => self.blocks.push(LayerRule {
                    name: full.clone(),
                    rules: sr.rules,
                    condition: Some(LayerCondition::Supports(sr.condition)),
                }),
                AtRuleOutcome::LayerNames(names) => {
                    for n in names {
                        self.declare(format!("{full}.{n}"));
                    }
                }
                AtRuleOutcome::LayerBlock { name, rules, mixin_rules: lmr, nested } => {
                    self.register(Some(&full), name, rules, lmr, nested);
                }
                other => self.hoisted.push(other),
            }
        }
    }
}
