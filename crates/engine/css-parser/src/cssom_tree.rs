//! Дерево CSSOM листа стилей (CSSOM §6.4, CSS Nesting 1, CSS Cascade L5/L6).
//!
//! Типизированные коллекции [`crate::Stylesheet`] (`rules`, `media_rules`, …)
//! хранят правила «плоско»: вложенные правила на разборе разворачиваются в
//! соседние `Rule` (`nesting_expand.rs`), at-правила лежат в собственных
//! коллекциях без общего порядка. Для `CSSStyleSheet.cssRules` этого мало —
//! нужны иерархия (`cssRules[0].cssRules[1].parentRule`), все виды правил
//! (`@layer`, `@scope`, `@import`, …) и запись внутрь групп. Поэтому CSSOM
//! работает с отдельным деревом [`CssomTree`], которое строится из исходного
//! текста листа и является **единственным авторитетом** для всех CSSOM-правок.
//!
//! * [`parse`] — универсальный разборщик уровня правил;
//! * [`serialize`] — `cssText` в формате Chrome (подтверждён WPT);
//! * [`mutate`] — `insertRule`/`deleteRule`/сеттеры с ошибками CSSOM.
//!
//! Нетронутый узел помнит точный исходный срез (`raw`) — [`CssomTree::to_source`]
//! отдаёт его как есть; изменённый узел и его предки регенерируются.

mod mutate;
mod parse;
mod serialize;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicU32, Ordering};

use crate::Declaration;

pub use mutate::CssomTreeError;

/// Процесс-уникальный id узла: устаревшая JS-обёртка никогда не совпадёт с
/// узлом, созданным после неё (даже в другом листе).
static NEXT_NODE_ID: AtomicU32 = AtomicU32::new(1);

fn fresh_id() -> u32 {
    NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Вид CSSOM-правила.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// Обычное стилевое правило `sel { … }` (в том числе вложенное).
    Style,
    /// `@media`.
    Media,
    /// `@supports`.
    Supports,
    /// `@container`.
    Container,
    /// `@layer name { … }` — блочная форма.
    Layer,
    /// `@layer a, b;` — оператор.
    LayerStatement,
    /// `@scope`.
    Scope,
    /// `@starting-style`.
    StartingStyle,
    /// `@import`.
    Import,
    /// `@namespace`.
    Namespace,
    /// `@font-face`.
    FontFace,
    /// `@keyframes`.
    Keyframes,
    /// Кадр внутри `@keyframes`.
    Keyframe,
    /// `@page`.
    Page,
    /// `@property`.
    Property,
    /// `@counter-style`.
    CounterStyle,
    /// `@font-feature-values`.
    FontFeatureValues,
    /// Декларации после вложенного правила (CSS Nesting 1 §6.1).
    NestedDeclarations,
    /// `@mixin` (CSS Mixins) — работает через собственные нативы по `raw`.
    Mixin,
    /// Правило, которое каскад знает, а CSSOM не показывает (`@function`,
    /// `@custom-media`, `@charset`, неизвестные at-правила): живёт в дереве
    /// ради `to_source`, но не входит в `cssRules`.
    Opaque,
}

/// Что лежит в теле правила.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleBody {
    /// Только вложенные правила (и, внутри стилевых правил, декларации).
    Rules,
    /// Только декларации.
    Decls,
    /// Кадры `@keyframes`.
    Keyframes,
    /// Тела нет (оператор).
    None,
}

impl RuleKind {
    /// Имя класса JS-обёртки (`CSSStyleRule`, …).
    pub fn js_class(self) -> &'static str {
        match self {
            Self::Style => "CSSStyleRule",
            Self::Media => "CSSMediaRule",
            Self::Supports => "CSSSupportsRule",
            Self::Container => "CSSContainerRule",
            Self::Layer => "CSSLayerBlockRule",
            Self::LayerStatement => "CSSLayerStatementRule",
            Self::Scope => "CSSScopeRule",
            Self::StartingStyle => "CSSStartingStyleRule",
            Self::Import => "CSSImportRule",
            Self::Namespace => "CSSNamespaceRule",
            Self::FontFace => "CSSFontFaceRule",
            Self::Keyframes => "CSSKeyframesRule",
            Self::Keyframe => "CSSKeyframeRule",
            Self::Page => "CSSPageRule",
            Self::Property => "CSSPropertyRule",
            Self::CounterStyle => "CSSCounterStyleRule",
            Self::FontFeatureValues => "CSSFontFeatureValuesRule",
            Self::NestedDeclarations => "CSSNestedDeclarations",
            Self::Mixin => "CSSMixinRule",
            Self::Opaque => "CSSRule",
        }
    }

    /// Показывается ли правило в `cssRules`.
    pub fn visible(self) -> bool {
        self != Self::Opaque
    }

    /// Группирующее ли правило (`CSSGroupingRule`): у него есть `cssRules`.
    pub fn is_grouping(self) -> bool {
        matches!(
            self,
            Self::Style
                | Self::Media
                | Self::Supports
                | Self::Container
                | Self::Layer
                | Self::Scope
                | Self::StartingStyle
        )
    }

    /// Тело правила.
    pub fn body(self) -> RuleBody {
        match self {
            Self::Style
            | Self::Media
            | Self::Supports
            | Self::Container
            | Self::Layer
            | Self::Scope
            | Self::StartingStyle => RuleBody::Rules,
            Self::FontFace
            | Self::Keyframe
            | Self::Page
            | Self::Property
            | Self::CounterStyle
            | Self::NestedDeclarations => RuleBody::Decls,
            Self::Keyframes => RuleBody::Keyframes,
            Self::FontFeatureValues => RuleBody::Decls,
            Self::LayerStatement
            | Self::Import
            | Self::Namespace
            | Self::Mixin
            | Self::Opaque => RuleBody::None,
        }
    }
}

/// Узел дерева CSSOM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssomNode {
    /// Процесс-уникальный id (адрес узла для JS).
    pub id: u32,
    /// Вид правила.
    pub kind: RuleKind,
    /// Прелюдия в канонической записи: селекторы / условие / имя / url.
    pub prelude: String,
    /// Собственные декларации (стилевое правило, `@font-face`, кадр, …).
    pub decls: Vec<Declaration>,
    /// Вложенные правила (включая `NestedDeclarations` и скрытые `Opaque`).
    pub children: Vec<CssomNode>,
    /// Точный исходный срез нетронутого узла.
    pub raw: Option<String>,
}

impl CssomNode {
    pub(crate) fn new(kind: RuleKind, prelude: String) -> Self {
        Self { id: fresh_id(), kind, prelude, decls: Vec::new(), children: Vec::new(), raw: None }
    }

    /// Дети, видимые в `cssRules`.
    pub fn visible_children(&self) -> impl Iterator<Item = &CssomNode> {
        self.children.iter().filter(|c| c.kind.visible())
    }

    /// Число видимых детей (`cssRules.length`).
    pub fn visible_len(&self) -> usize {
        self.visible_children().count()
    }
}

/// Дерево правил листа.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CssomTree {
    /// Правила верхнего уровня (включая скрытые).
    pub rules: Vec<CssomNode>,
}

impl CssomTree {
    /// Строит дерево из исходного текста листа.
    pub fn parse(source: &str) -> Self {
        Self { rules: parse::parse_sheet(source) }
    }

    /// Число видимых правил верхнего уровня (`cssRules.length`).
    pub fn visible_len(&self) -> usize {
        self.rules.iter().filter(|r| r.kind.visible()).count()
    }

    /// Узел по id.
    pub fn get(&self, id: u32) -> Option<&CssomNode> {
        find(&self.rules, id)
    }

    /// Родитель узла: `None` — узла нет, `Some(None)` — узел верхнего уровня.
    pub fn parent_of(&self, id: u32) -> Option<Option<u32>> {
        if self.rules.iter().any(|r| r.id == id) {
            return Some(None);
        }
        parent_in(&self.rules, id)
    }

    /// Видимые дети узла (`None` — корень листа).
    pub fn visible_children_of(&self, parent: Option<u32>) -> Option<Vec<&CssomNode>> {
        match parent {
            None => Some(self.rules.iter().filter(|r| r.kind.visible()).collect()),
            Some(id) => self.get(id).map(|n| n.visible_children().collect()),
        }
    }

    /// Текст каскада: `raw` нетронутых узлов, регенерация изменённых.
    pub fn to_source(&self) -> String {
        serialize::tree_source(&self.rules)
    }

    /// `CSSRule.cssText` узла.
    pub fn css_text(&self, id: u32) -> Option<String> {
        self.get(id).map(serialize::css_text)
    }

    /// Вставляет правило из текста; `parent = None` — корень листа.
    /// Возвращает индекс вставки среди видимых детей.
    pub fn insert_rule(
        &mut self,
        parent: Option<u32>,
        text: &str,
        index: usize,
    ) -> Result<usize, CssomTreeError> {
        mutate::insert_rule(self, parent, text, index)
    }

    /// Удаляет видимое правило с индексом `index`.
    pub fn delete_rule(&mut self, parent: Option<u32>, index: usize) -> Result<(), CssomTreeError> {
        mutate::delete_rule(self, parent, index)
    }

    /// Меняет прелюдию (`selectorText`, `conditionText`, `keyText`, …).
    /// `Ok(false)` — текст невалиден и по спецификации игнорируется.
    pub fn set_prelude(&mut self, id: u32, text: &str) -> Result<bool, CssomTreeError> {
        mutate::set_prelude(self, id, text)
    }

    /// Заменяет собственные декларации узла.
    pub fn set_decls(&mut self, id: u32, decls: Vec<Declaration>) -> Result<(), CssomTreeError> {
        mutate::set_decls(self, id, decls)
    }

    /// Обновляет `raw` узла без изменения структуры (mixin-нативы).
    pub fn set_raw(&mut self, id: u32, raw: String) -> bool {
        match find_mut(&mut self.rules, id) {
            Some(n) => {
                n.raw = Some(raw);
                true
            }
            None => false,
        }
    }
}

fn find(nodes: &[CssomNode], id: u32) -> Option<&CssomNode> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let Some(f) = find(&n.children, id) {
            return Some(f);
        }
    }
    None
}

fn parent_in(nodes: &[CssomNode], id: u32) -> Option<Option<u32>> {
    for n in nodes {
        if n.children.iter().any(|c| c.id == id) {
            return Some(Some(n.id));
        }
        if let Some(p) = parent_in(&n.children, id) {
            return Some(p);
        }
    }
    None
}

pub(crate) fn find_mut(nodes: &mut [CssomNode], id: u32) -> Option<&mut CssomNode> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let Some(f) = find_mut(&mut n.children, id) {
            return Some(f);
        }
    }
    None
}
