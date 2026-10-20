//! CSSOM-10: правки листа через дерево [`CssomTree`].
//!
//! Дерево — единственный авторитет для `insertRule`/`deleteRule`/сеттеров
//! правил. После успешной правки типизированные коллекции листа пересобираются
//! из `tree.to_source()`, а [`Stylesheet::source`] остаётся исходным текстом
//! DOM: по нему шелл и `SheetSync` узнают лист в каскаде.

use super::*;
use crate::cssom_tree::{CssomCell, CssomTree, CssomTreeError, RuleKind};

impl Stylesheet {
    /// Дерево CSSOM листа; строится из [`Self::source`] при первом обращении.
    pub fn cssom_tree(&self) -> &CssomTree {
        self.cssom.get_or_build(self.source.as_deref())
    }

    /// Пересобирает типизированные коллекции из дерева, сохраняя `source`.
    fn rebuild_from_tree(&mut self, tree: CssomTree) {
        let mut fresh = parse(&tree.to_source());
        fresh.source = self.source.take();
        fresh.cssom = CssomCell::filled(tree);
        *self = fresh;
    }

    /// `insertRule(text, index)` внутри `parent` (`None` — корень листа).
    /// Возвращает индекс вставки среди видимых детей.
    pub fn cssom_insert_rule(
        &mut self,
        parent: Option<u32>,
        text: &str,
        index: usize,
    ) -> Result<usize, CssomTreeError> {
        let mut tree = self.cssom.take().unwrap_or_else(|| CssomTree::parse(self.source.as_deref().unwrap_or("")));
        let at_root_end = parent.is_none() && index == tree.visible_len();
        let at = match tree.insert_rule(parent, text, index) {
            Ok(at) => at,
            Err(e) => {
                self.cssom = CssomCell::filled(tree);
                return Err(e);
            }
        };
        // Быстрый путь CSS-in-JS: правило в конец корня, `@mixin` в листе нет —
        // полная пересборка на каждую вставку была бы квадратичной.
        if at_root_end && self.mixin_rules.is_empty() {
            let inserted = tree.rules.iter().rev().find(|n| n.kind.visible()).and_then(|n| tree.node_source(n.id));
            if let Some(src) = inserted {
                let piece = parse(&src);
                if piece.mixin_rules.is_empty() {
                    self.merge_from(piece);
                    self.cssom = CssomCell::filled(tree);
                    return Ok(at);
                }
            }
        }
        self.rebuild_from_tree(tree);
        Ok(at)
    }

    /// `deleteRule(index)` внутри `parent`.
    pub fn cssom_delete_rule(&mut self, parent: Option<u32>, index: usize) -> Result<(), CssomTreeError> {
        let mut tree = self.cssom.take().unwrap_or_else(|| CssomTree::parse(self.source.as_deref().unwrap_or("")));
        let outcome = tree.delete_rule(parent, index);
        if outcome.is_ok() {
            self.rebuild_from_tree(tree);
        } else {
            self.cssom = CssomCell::filled(tree);
        }
        outcome
    }

    /// `CSSStyleSheet.replace()/replaceSync()` (CSSOM §4.6): `@import` rules of the new text
    /// are parsed but dropped — a constructed sheet never loads anything.
    pub fn cssom_drop_imports(&mut self) {
        let imports: Vec<usize> = self
            .cssom_tree()
            .visible_children_of(None)
            .map(|rules| {
                rules.iter().enumerate().filter(|(_, r)| r.kind == RuleKind::Import).map(|(i, _)| i).collect()
            })
            .unwrap_or_default();
        for index in imports.into_iter().rev() {
            let _ = self.cssom_delete_rule(None, index);
        }
    }

    /// Сеттер прелюдии (`selectorText`, `conditionText`, `name`, …).
    /// `Ok(false)` — текст невалиден и по спецификации игнорируется.
    pub fn cssom_set_prelude(&mut self, id: u32, text: &str) -> Result<bool, CssomTreeError> {
        let mut tree = self.cssom.take().unwrap_or_else(|| CssomTree::parse(self.source.as_deref().unwrap_or("")));
        let outcome = tree.set_prelude(id, text);
        if matches!(outcome, Ok(true)) {
            self.rebuild_from_tree(tree);
        } else {
            self.cssom = CssomCell::filled(tree);
        }
        outcome
    }

    /// Заменяет собственные декларации узла (`.style` правила).
    pub fn cssom_set_decls(&mut self, id: u32, decls: Vec<Declaration>) -> Result<(), CssomTreeError> {
        let mut tree = self.cssom.take().unwrap_or_else(|| CssomTree::parse(self.source.as_deref().unwrap_or("")));
        let outcome = tree.set_decls(id, decls);
        if outcome.is_ok() {
            self.rebuild_from_tree(tree);
        } else {
            self.cssom = CssomCell::filled(tree);
        }
        outcome
    }

    /// Индекс верхнеуровневого `@mixin` с id `tree_id` в [`Self::cssom_rules`]
    /// — адрес для устаревших mixin-нативов.
    pub fn mixin_cssom_index(&self, tree_id: u32) -> Option<usize> {
        let ordinal = self
            .cssom_tree()
            .rules
            .iter()
            .filter(|n| n.kind == RuleKind::Mixin)
            .position(|n| n.id == tree_id)?;
        self.top_level_order
            .iter()
            .enumerate()
            .filter(|(_, k)| matches!(k, TopLevelRuleKind::Mixin(_)))
            .nth(ordinal)
            .map(|(i, _)| i)
    }

    /// Обратное к [`Self::mixin_cssom_index`]: id узла дерева для `@mixin` с
    /// индексом `cssom_idx` в [`Self::cssom_rules`].
    pub fn mixin_tree_id(&self, cssom_idx: usize) -> Option<u32> {
        let ordinal = self.top_level_order[..cssom_idx.min(self.top_level_order.len())]
            .iter()
            .filter(|k| matches!(k, TopLevelRuleKind::Mixin(_)))
            .count();
        if !matches!(self.top_level_order.get(cssom_idx), Some(TopLevelRuleKind::Mixin(_))) {
            return None;
        }
        self.cssom_tree().rules.iter().filter(|n| n.kind == RuleKind::Mixin).nth(ordinal).map(|n| n.id)
    }

    /// После правки результата `@mixin` старыми нативами обновляет исходный
    /// текст его узла в дереве, чтобы каскад получил актуальный `@mixin`.
    pub fn sync_mixin_node(&mut self, tree_id: u32) -> bool {
        let Some(idx) = self.mixin_cssom_index(tree_id) else { return false };
        let Some(CssomRuleRef::Mixin(m)) = self.cssom_rules().get(idx).copied() else { return false };
        let text = m.css_text();
        let mut tree = self.cssom.take().unwrap_or_else(|| CssomTree::parse(self.source.as_deref().unwrap_or("")));
        let ok = tree.set_raw(tree_id, text);
        self.cssom = CssomCell::filled(tree);
        ok
    }

    /// Текст листа для каскада — `tree.to_source()` (нетронутые узлы дословно).
    pub fn cssom_source_text(&self) -> String {
        self.cssom_tree().to_source()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_rule_into_group_rebuilds_collections_and_keeps_source() {
        let src = "@media screen { a { color: red } } b { color: blue }";
        let mut sheet = parse(src);
        let media = sheet.cssom_tree().rules[0].id;
        sheet.cssom_insert_rule(Some(media), "c { color: green }", 1).unwrap();
        assert_eq!(sheet.source(), Some(src));
        assert_eq!(sheet.media_rules[0].rules.len(), 2);
        assert_eq!(sheet.rules.len(), 1);
    }

    #[test]
    fn layer_statement_insert_reaches_layer_order() {
        let mut sheet = parse("a { color: red }");
        sheet.cssom_insert_rule(None, "@layer base, theme;", 0).unwrap();
        assert_eq!(sheet.layer_order, vec!["base".to_string(), "theme".to_string()]);
        assert_eq!(sheet.cssom_tree().visible_len(), 2);
    }

    #[test]
    fn append_fast_path_keeps_tree_and_rules() {
        let mut sheet = parse("");
        for i in 0..5 {
            let n = sheet.cssom_tree().visible_len();
            sheet.cssom_insert_rule(None, &format!(".c{i} {{ color: red }}"), n).unwrap();
        }
        assert_eq!(sheet.rules.len(), 5);
        assert_eq!(sheet.cssom_tree().visible_len(), 5);
        sheet.cssom_delete_rule(None, 0).unwrap();
        assert_eq!(sheet.rules.len(), 4);
    }

    #[test]
    fn failed_insert_keeps_the_tree() {
        let mut sheet = parse("a { color: red }");
        assert!(sheet.cssom_insert_rule(None, "a { color: red }", 9).is_err());
        assert_eq!(sheet.cssom_tree().visible_len(), 1);
        assert_eq!(sheet.rules.len(), 1);
    }

    #[test]
    fn clone_keeps_the_tree_ids() {
        let sheet = parse("a { color: red }");
        let id = sheet.cssom_tree().rules[0].id;
        assert_eq!(sheet.clone().cssom_tree().rules[0].id, id);
    }

    #[test]
    fn mixin_index_maps_tree_node_to_top_level_position() {
        let sheet = parse("a { color: red } @mixin --m() { color: blue } b { color: red }");
        let id = sheet.cssom_tree().rules.iter().find(|n| n.kind == RuleKind::Mixin).unwrap().id;
        let idx = sheet.mixin_cssom_index(id).unwrap();
        assert!(matches!(sheet.cssom_rules()[idx], CssomRuleRef::Mixin(_)));
    }
}
