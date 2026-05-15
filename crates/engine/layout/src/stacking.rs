//! Stacking context model (CSS 2.1 Appendix E + CSS Positioned Layout L3 §9.10).
//!
//! Sprint 0 — контракты: типы и API, реальное построение stacking-дерева
//! и painting order traversal — в треках P1 п.2A и P2 п.2A.
//!
//! Stacking context — это элемент, формирующий собственный "слой" z-композиции.
//! Внутри контекста дети сортируются по z-index; снаружи контекста его дети
//! не могут оказаться между чужими z-уровнями.
//!
//! Контекст создают (CSS Positioned Layout L3 §9.10):
//! - корневой элемент,
//! - `position: absolute|relative|fixed|sticky` с z-index ≠ auto,
//! - `opacity < 1`,
//! - `transform` / `filter` / `clip-path` / `mask` ≠ none,
//! - `mix-blend-mode` ≠ normal,
//! - `isolation: isolate`,
//! - `will-change: <stacking-property>`,
//! - flex/grid item с z-index ≠ auto.
//!
//! Phase 0: ничего из этого ещё не учитывает paint pipeline — он рисует в
//! порядке DOM. Заполнение происходит в P1 п.2A.

use crate::box_tree::LayoutBox;

/// Идентификатор stacking context-а. Монотонно растёт от 0; 0 = root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StackingContextId(pub u32);

impl StackingContextId {
    /// Корневой stacking context документа (`<html>`).
    pub const ROOT: Self = Self(0);

    pub fn raw(self) -> u32 {
        self.0
    }
}

/// CSS 2.1 Appendix E — 7-уровневый порядок отрисовки внутри stacking context.
///
/// Внутри одного stacking-контекста элементы рисуются строго в этом порядке,
/// независимо от DOM-порядка. Один и тот же box обычно занимает уровень 1
/// (фон/бордер) и один из уровней 3-6 (содержимое), плюс корневой stacking-
/// context отрисовывается полностью раньше своих descendant-stacking-ов с
/// z-index ≥ 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaintPhase {
    /// 1. Backgrounds + borders элемента, формирующего stacking context.
    RootBackground,
    /// 2. Дочерние stacking-контексты с отрицательным z-index (упорядочены
    ///    по возрастанию z, ties — по дереву).
    NegativeZ,
    /// 3. Backgrounds + borders блочных in-flow non-positioned потомков.
    BlockBackgrounds,
    /// 4. Floats и их содержимое.
    Floats,
    /// 5. In-flow inline-level non-positioned потомков (текст, inline-боксы).
    InlineContent,
    /// 6. Дочерние stacking-контексты с z-index `auto` (трактуется как 0)
    ///    и positioned-элементы с z-index `auto`.
    PositionedAndZAuto,
    /// 7. Дочерние stacking-контексты с положительным z-index.
    PositiveZ,
}

impl PaintPhase {
    /// Порядок обхода Painting Order traversal (CSS 2.1 Appendix E).
    pub const ORDER: [PaintPhase; 7] = [
        PaintPhase::RootBackground,
        PaintPhase::NegativeZ,
        PaintPhase::BlockBackgrounds,
        PaintPhase::Floats,
        PaintPhase::InlineContent,
        PaintPhase::PositionedAndZAuto,
        PaintPhase::PositiveZ,
    ];
}

/// Один stacking context: владелец-box + z-index + ссылки на дочерние
/// stacking-контексты, отсортированные по z-index (stable sort).
///
/// Phase 0 / Sprint 0: только структура. Заполнение / порядок traversal —
/// P1 п.2A / P2 п.2A.
#[derive(Debug, Clone, Default)]
pub struct StackingContext {
    pub id: StackingContextId,
    /// z-index собственного box-а. `None` = `auto` (для root всегда None).
    pub z_index: Option<i32>,
    /// Дочерние stacking-контексты. Порядок — по возрастанию z (stable),
    /// `auto` идёт между отрицательными и положительными.
    pub children: Vec<StackingContextId>,
}

/// Плоское представление stacking-дерева: вектор `StackingContext` + индексы
/// детей. `contexts[0]` всегда root.
///
/// Sprint 0 stub: пустой root без детей.
#[derive(Debug, Clone, Default)]
pub struct StackingTree {
    pub contexts: Vec<StackingContext>,
}

impl StackingTree {
    /// Stub: дерево с единственным root-контекстом, без layout traversal.
    /// Реальная фабрика из `&LayoutBox` появится в P1 п.2A.
    pub fn empty_root() -> Self {
        Self {
            contexts: vec![StackingContext {
                id: StackingContextId::ROOT,
                z_index: None,
                children: Vec::new(),
            }],
        }
    }

    /// Построение stacking-дерева из layout-дерева. Sprint 0 — stub:
    /// возвращает `empty_root()`. Реальная импл — P1 п.2A: должна
    /// определять создание контекстов по CSS Positioned Layout L3 §9.10
    /// (opacity<1, transform, filter, will-change, …) и собирать z-index
    /// порядок.
    pub fn build(_root: &LayoutBox) -> Self {
        Self::empty_root()
    }

    pub fn root(&self) -> &StackingContext {
        &self.contexts[0]
    }
}

/// Painting order — линейная последовательность пар `(StackingContextId, PaintPhase)`,
/// в которой compositor / rasterizer обходит дерево. Один сериализованный
/// порядок paint, готовый к feed-у в display list builder.
///
/// Sprint 0 stub: только root-контекст в порядке `PaintPhase::ORDER`.
/// Реальная импл — P2 п.2A.
#[derive(Debug, Clone, Default)]
pub struct PaintOrder {
    pub steps: Vec<(StackingContextId, PaintPhase)>,
}

impl PaintOrder {
    /// Sprint 0 stub: проход только по root, без потомков.
    pub fn from_tree(tree: &StackingTree) -> Self {
        let root = tree.root().id;
        let steps = PaintPhase::ORDER
            .iter()
            .map(|phase| (root, *phase))
            .collect();
        Self { steps }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_id_is_zero() {
        assert_eq!(StackingContextId::ROOT.raw(), 0);
    }

    #[test]
    fn empty_tree_has_only_root() {
        let tree = StackingTree::empty_root();
        assert_eq!(tree.contexts.len(), 1);
        assert_eq!(tree.root().id, StackingContextId::ROOT);
        assert!(tree.root().children.is_empty());
        assert_eq!(tree.root().z_index, None);
    }

    #[test]
    fn paint_order_has_seven_phases() {
        assert_eq!(PaintPhase::ORDER.len(), 7);
        // Phases отсортированы возрастающе (Ord-совместимость).
        let mut sorted = PaintPhase::ORDER;
        sorted.sort();
        assert_eq!(sorted, PaintPhase::ORDER);
    }

    #[test]
    fn paint_order_from_empty_tree_covers_root_phases() {
        let tree = StackingTree::empty_root();
        let order = PaintOrder::from_tree(&tree);
        assert_eq!(order.len(), 7);
        for (i, phase) in PaintPhase::ORDER.iter().enumerate() {
            assert_eq!(order.steps[i], (StackingContextId::ROOT, *phase));
        }
    }
}
