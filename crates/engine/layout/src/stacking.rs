//! Stacking context model (CSS 2.1 Appendix E + CSS Positioned Layout L3 §9.10).
//!
//! Stacking context — это элемент, формирующий собственный «слой» z-композиции.
//! Внутри контекста дети сортируются по z-index; снаружи контекста его дети
//! не могут оказаться между чужими z-уровнями.
//!
//! Контекст создают (CSS Positioned Layout L3 §9.10):
//! - корневой элемент,
//! - `position: absolute|relative` с z-index ≠ auto,
//! - `position: fixed|sticky` (всегда),
//! - `opacity < 1`,
//! - `transform` / `filter` / `clip-path` ≠ none,
//! - `mix-blend-mode` ≠ normal,
//! - `isolation: isolate`,
//! - `will-change: <stacking-property>` (e.g. `transform`, `opacity`,
//!   `filter`, `position`, `z-index`),
//! - flex/grid item с z-index ≠ auto.
//!
//! `StackingTree::build` строит плоское представление по `&LayoutBox`. Дочерние
//! stacking-контексты сортируются по z-index (`auto` трактуется как 0; stable
//! sort сохраняет древесный порядок для одинаковых z). Сам painting order
//! traversal (CSS 2.1 Appendix E, 7 фаз) — задача P2 п.2A.

use crate::box_tree::{BoxKind, LayoutBox};
use crate::style::{ComputedStyle, Isolation, MixBlendMode, Position};

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
#[derive(Debug, Clone, Default)]
pub struct StackingTree {
    pub contexts: Vec<StackingContext>,
}

impl StackingTree {
    /// Дерево с единственным root-контекстом без детей. Используется в
    /// тестах и как fallback для пустых layout-деревьев.
    pub fn empty_root() -> Self {
        Self {
            contexts: vec![StackingContext {
                id: StackingContextId::ROOT,
                z_index: None,
                children: Vec::new(),
            }],
        }
    }

    /// Построение stacking-дерева из layout-дерева.
    ///
    /// Корневой `LayoutBox` всегда создаёт root stacking context (CSS
    /// Positioned Layout L3 §9.10 п.1). Дальше дерево обходится pre-order:
    /// каждый box проверяется триггерами §9.10 — если создаёт собственный
    /// stacking context, в `contexts` добавляется новая запись, и она же
    /// становится текущим parent SC для своих потомков. Иначе потомки
    /// продолжают принадлежать тому же parent SC.
    ///
    /// После обхода children каждого SC сортируются stable по z-index
    /// (CSS Painting Order L3 §3): negative z, затем `auto` / 0, затем
    /// positive z. Ties — древесный порядок.
    pub fn build(root: &LayoutBox) -> Self {
        let mut tree = Self {
            contexts: vec![StackingContext {
                id: StackingContextId::ROOT,
                z_index: None,
                children: Vec::new(),
            }],
        };
        for child in &root.children {
            walk(child, StackingContextId::ROOT, &mut tree);
        }
        // Stable sort по z (CSS 2.1 Appendix E + Painting Order L3 §3).
        // Каждый SC хранит детей в древесном порядке; сортировка по z с
        // tie-breaking по дереву = stable sort с ключом z_or_zero.
        for ctx_idx in 0..tree.contexts.len() {
            let mut children = std::mem::take(&mut tree.contexts[ctx_idx].children);
            children.sort_by_key(|id| z_sort_key(&tree.contexts[id.0 as usize]));
            tree.contexts[ctx_idx].children = children;
        }
        tree
    }

    pub fn root(&self) -> &StackingContext {
        &self.contexts[0]
    }
}

/// Ключ сортировки SC по z-index. `auto` (None) → 0 (CSS Painting Order L3
/// §3 «auto value is treated as 0»). Возвращает `i32`, чтобы negative-z
/// корректно шли перед positive.
fn z_sort_key(ctx: &StackingContext) -> i32 {
    ctx.z_index.unwrap_or(0)
}

/// CSS Positioned Layout L3 §9.10 — создаёт ли элемент собственный
/// stacking context.
///
/// Триггеры (Phase 0 set):
/// - `position: fixed | sticky` — всегда;
/// - `position: relative | absolute` с явным `z-index` (≠ auto);
/// - `opacity < 1`;
/// - `transform != none` (непустой `Vec<TransformFn>`);
/// - `filter != none` (непустой `Vec<FilterFn>`);
/// - `clip-path != none`;
/// - `mix-blend-mode != normal`;
/// - `isolation: isolate`;
/// - `will-change` с указанием stacking-свойства (`transform`, `opacity`,
///   `filter`, `position`, `z-index`, `clip-path`, `mask`, `mix-blend-mode`,
///   `isolation`, `perspective`).
///
/// Отложено до flex/grid layout: flex/grid item с z-index ≠ auto. Сейчас
/// проверяется только для положенных боксов (если родитель использует
/// `display: flex|grid`, мы ещё не пересчитываем — флаг flex/grid item
/// добавим вместе с реальным flex/grid pass).
pub fn creates_stacking_context(style: &ComputedStyle) -> bool {
    match style.position {
        Position::Fixed | Position::Sticky => return true,
        Position::Relative | Position::Absolute => {
            if style.z_index.is_some() {
                return true;
            }
        }
        Position::Static => {}
    }
    if style.opacity < 1.0 {
        return true;
    }
    if !style.transform.is_empty()
        || style.translate.is_some()
        || style.rotate.is_some()
        || style.scale.is_some()
    {
        return true;
    }
    if !style.filter.is_empty() {
        return true;
    }
    // CSS Filter Effects L2 §2 / CSS Compositing L1: a `backdrop-filter` other
    // than `none` creates a stacking context (the element needs an isolated
    // layer to capture and filter the backdrop behind it).
    if !style.backdrop_filter.is_empty() {
        return true;
    }
    if style.clip_path.is_some() {
        return true;
    }
    if style.mix_blend_mode != MixBlendMode::Normal {
        return true;
    }
    if style.isolation == Isolation::Isolate {
        return true;
    }
    if style.will_change.iter().any(|s| is_stacking_will_change(s)) {
        return true;
    }
    false
}

/// Проверка ident-а из `will-change` — называет ли он property, которое
/// при non-initial значении создаёт stacking context. Per CSS Will Change L1
/// §3 «If any non-initial value of a property would cause the element to
/// generate a stacking context, [...] specifying that property in will-change
/// must create a stacking context on the element».
fn is_stacking_will_change(ident: &str) -> bool {
    matches!(
        ident.trim().to_ascii_lowercase().as_str(),
        "transform"
            | "opacity"
            | "filter"
            | "position"
            | "z-index"
            | "clip-path"
            | "mask"
            | "mix-blend-mode"
            | "isolation"
            | "perspective"
    )
}

/// Анонимные / неучаствующие в layout box-ы не имеют DOM-элемента, к
/// которому может быть привязан собственный stacking context — их стиль
/// клонируется от родителя (включая non-inherited вроде `opacity`), и
/// если бы мы их учитывали, каждый текстовый InlineRun под opacity:0.5
/// порождал бы фантомный SC. Замечание: реальный DOM-элемент над таким
/// InlineRun-ом уже создаёт SC.
pub fn box_can_own_stacking_context(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Image { .. } | BoxKind::FormControl { .. })
}

/// Pre-order обход layout-дерева с накоплением stacking-контекстов.
fn walk(b: &LayoutBox, parent_sc: StackingContextId, tree: &mut StackingTree) {
    let current_sc =
        if box_can_own_stacking_context(b) && creates_stacking_context(&b.style) {
            let new_id = StackingContextId(tree.contexts.len() as u32);
            tree.contexts.push(StackingContext {
                id: new_id,
                z_index: b.style.z_index,
                children: Vec::new(),
            });
            tree.contexts[parent_sc.0 as usize].children.push(new_id);
            new_id
        } else {
            parent_sc
        };
    for child in &b.children {
        walk(child, current_sc, tree);
    }
}

/// Painting order — линейная последовательность пар `(StackingContextId,
/// PaintPhase)`, в которой compositor / rasterizer обходит дерево. Один
/// сериализованный порядок paint, готовый к feed-у в display list builder.
///
/// Каждая запись интерпретируется как «нарисовать SC при этой фазе»:
/// - `RootBackground` (1) — собственный фон/бордеры SC-owner-а.
/// - `BlockBackgrounds` (3) — фоны/бордеры блочных in-flow non-positioned
///   потомков внутри этого SC.
/// - `Floats` (4) — float-потомки.
/// - `InlineContent` (5) — inline-level non-positioned потомки.
///
/// Фазы `NegativeZ` / `PositionedAndZAuto` / `PositiveZ` (2 / 6 / 7) — это
/// «места рекурсии в дочерние SC»; в выводе `from_tree` они **не появляются**
/// сами по себе, но соответствующие child-SC steps вставлены между
/// «paint-meaningful» фазами родителя в правильном порядке. То есть:
/// `[parent.RootBackground, ...neg children fully painted..., parent.BlockBackgrounds,
///  parent.Floats, parent.InlineContent, ...auto/0 children..., ...positive children...]`.
#[derive(Debug, Clone, Default)]
pub struct PaintOrder {
    pub steps: Vec<(StackingContextId, PaintPhase)>,
}

impl PaintOrder {
    /// Строит painting order по CSS 2.1 Appendix E + CSS Painting Order L3 §3.
    ///
    /// Рекурсивный обход stacking-дерева:
    /// - Для каждого SC сначала эмитим `RootBackground`;
    /// - затем разворачиваем все child-SC с отрицательным z-index (`children`
    ///   уже отсортированы stable по z в `StackingTree::build`, так что
    ///   достаточно фильтра по знаку);
    /// - эмитим `BlockBackgrounds` + `Floats` + `InlineContent` собственного SC;
    /// - разворачиваем child-SC с z-index `auto` / 0;
    /// - разворачиваем child-SC с положительным z-index.
    ///
    /// Layout-уровень разбиения boxes по фазам 3/4/5 (block vs float vs inline)
    /// — отдельная забота renderer-а: ему нужно walk-нуть `LayoutBox`-tree,
    /// отфильтровать по SC-принадлежности и `display`/`float` свойствам.
    /// Здесь даём только SC-уровневую последовательность.
    pub fn from_tree(tree: &StackingTree) -> Self {
        let mut steps = Vec::new();
        if !tree.contexts.is_empty() {
            paint_sc(tree, StackingContextId::ROOT, &mut steps);
        }
        Self { steps }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Эмитит paint-фазы одного SC, рекурсивно разворачивая child-SC.
/// Children уже отсортированы stable по z в `StackingTree::build` —
/// мы только фильтруем по знаку z, не пере-сортируем.
fn paint_sc(
    tree: &StackingTree,
    sc_id: StackingContextId,
    out: &mut Vec<(StackingContextId, PaintPhase)>,
) {
    let sc = &tree.contexts[sc_id.0 as usize];

    out.push((sc_id, PaintPhase::RootBackground));

    // Phase 2: child-SC с z < 0 (в z-order, ties — древесный порядок).
    for &child_id in &sc.children {
        if tree.contexts[child_id.0 as usize]
            .z_index
            .is_some_and(|z| z < 0)
        {
            paint_sc(tree, child_id, out);
        }
    }

    // Phases 3, 4, 5: собственные блочные / float / inline-потомки.
    out.push((sc_id, PaintPhase::BlockBackgrounds));
    out.push((sc_id, PaintPhase::Floats));
    out.push((sc_id, PaintPhase::InlineContent));

    // Phase 6: child-SC с z `auto` (None) или 0.
    for &child_id in &sc.children {
        let z = tree.contexts[child_id.0 as usize].z_index;
        if z.is_none() || z == Some(0) {
            paint_sc(tree, child_id, out);
        }
    }

    // Phase 7: child-SC с z > 0.
    for &child_id in &sc.children {
        if tree.contexts[child_id.0 as usize]
            .z_index
            .is_some_and(|z| z > 0)
        {
            paint_sc(tree, child_id, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::box_tree::layout;
    use lumen_core::geom::Size;

    fn build_tree(html: &str, css: &str) -> StackingTree {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        StackingTree::build(&root)
    }

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
    fn paint_order_from_root_only_emits_four_phases() {
        // Без child-SC: только paint-meaningful фазы root-а
        // (RootBackground / BlockBackgrounds / Floats / InlineContent).
        // Фазы NegativeZ / PositionedAndZAuto / PositiveZ — маркеры рекурсии
        // в child-SC; в выводе не появляются, если детей нет.
        let tree = StackingTree::empty_root();
        let order = PaintOrder::from_tree(&tree);
        assert_eq!(
            order.steps,
            vec![
                (StackingContextId::ROOT, PaintPhase::RootBackground),
                (StackingContextId::ROOT, PaintPhase::BlockBackgrounds),
                (StackingContextId::ROOT, PaintPhase::Floats),
                (StackingContextId::ROOT, PaintPhase::InlineContent),
            ]
        );
    }

    /// Хелпер для построения тестовых деревьев без layout: вручную набираем
    /// `StackingContext`-ы с заданными z и parent-ссылками.
    fn build_synthetic_tree(z_per_sc: &[Option<i32>], children_per_sc: &[Vec<u32>]) -> StackingTree {
        assert_eq!(z_per_sc.len(), children_per_sc.len());
        let contexts = z_per_sc
            .iter()
            .zip(children_per_sc.iter())
            .enumerate()
            .map(|(i, (z, children))| StackingContext {
                id: StackingContextId(i as u32),
                z_index: *z,
                children: children.iter().map(|&c| StackingContextId(c)).collect(),
            })
            .collect();
        StackingTree { contexts }
    }

    #[test]
    fn negative_z_child_painted_after_root_background() {
        // root (z=None) → один child с z=-1
        let tree = build_synthetic_tree(&[None, Some(-1)], &[vec![1], vec![]]);
        let order = PaintOrder::from_tree(&tree);
        let root = StackingContextId::ROOT;
        let neg = StackingContextId(1);
        // root.RB → neg.RB / neg.BB / neg.F / neg.IC → root.BB / root.F / root.IC
        assert_eq!(
            order.steps,
            vec![
                (root, PaintPhase::RootBackground),
                (neg, PaintPhase::RootBackground),
                (neg, PaintPhase::BlockBackgrounds),
                (neg, PaintPhase::Floats),
                (neg, PaintPhase::InlineContent),
                (root, PaintPhase::BlockBackgrounds),
                (root, PaintPhase::Floats),
                (root, PaintPhase::InlineContent),
            ]
        );
    }

    #[test]
    fn positive_z_child_painted_after_root_inline_content() {
        // root → один child с z=1
        let tree = build_synthetic_tree(&[None, Some(1)], &[vec![1], vec![]]);
        let order = PaintOrder::from_tree(&tree);
        let root = StackingContextId::ROOT;
        let pos = StackingContextId(1);
        assert_eq!(
            order.steps,
            vec![
                (root, PaintPhase::RootBackground),
                (root, PaintPhase::BlockBackgrounds),
                (root, PaintPhase::Floats),
                (root, PaintPhase::InlineContent),
                (pos, PaintPhase::RootBackground),
                (pos, PaintPhase::BlockBackgrounds),
                (pos, PaintPhase::Floats),
                (pos, PaintPhase::InlineContent),
            ]
        );
    }

    #[test]
    fn auto_z_child_painted_in_phase_six() {
        // root → один child с z=None (auto). Phase 6: auto и 0 идут вместе
        // ПОСЛЕ inline content родителя, но ДО positive-z.
        let tree = build_synthetic_tree(&[None, None], &[vec![1], vec![]]);
        let order = PaintOrder::from_tree(&tree);
        // root.RB, root.BB, root.F, root.IC, then child fully
        assert_eq!(order.len(), 8);
        assert_eq!(order.steps[3].1, PaintPhase::InlineContent);
        assert_eq!(order.steps[4].0, StackingContextId(1));
        assert_eq!(order.steps[4].1, PaintPhase::RootBackground);
    }

    #[test]
    fn zero_z_child_painted_same_phase_as_auto() {
        // CSS Painting Order L3 §3: `auto value is treated as 0`. Дети с
        // z=0 и z=None оба идут в фазе 6 родителя.
        let tree = build_synthetic_tree(&[None, Some(0)], &[vec![1], vec![]]);
        let order = PaintOrder::from_tree(&tree);
        assert_eq!(order.steps[4].0, StackingContextId(1));
        assert_eq!(order.steps[4].1, PaintPhase::RootBackground);
    }

    #[test]
    fn mixed_z_children_ordered_negative_auto_positive() {
        // root c тремя детьми: neg(z=-1), auto(z=None), pos(z=2).
        // children_per_sc сохраняет порядок, в котором мы их добавляем —
        // именно так StackingTree::build делает stable sort.
        let tree = build_synthetic_tree(
            &[None, Some(-1), None, Some(2)],
            &[vec![1, 2, 3], vec![], vec![], vec![]],
        );
        let order = PaintOrder::from_tree(&tree);
        // root.RB → neg full → root.BB/F/IC → auto full → pos full
        let root = StackingContextId::ROOT;
        let neg = StackingContextId(1);
        let auto = StackingContextId(2);
        let pos = StackingContextId(3);
        assert_eq!(order.steps[0], (root, PaintPhase::RootBackground));
        // negative-z child block
        assert_eq!(order.steps[1], (neg, PaintPhase::RootBackground));
        assert_eq!(order.steps[2], (neg, PaintPhase::BlockBackgrounds));
        assert_eq!(order.steps[3], (neg, PaintPhase::Floats));
        assert_eq!(order.steps[4], (neg, PaintPhase::InlineContent));
        // own block/float/inline
        assert_eq!(order.steps[5], (root, PaintPhase::BlockBackgrounds));
        assert_eq!(order.steps[6], (root, PaintPhase::Floats));
        assert_eq!(order.steps[7], (root, PaintPhase::InlineContent));
        // auto child block
        assert_eq!(order.steps[8], (auto, PaintPhase::RootBackground));
        // ...auto.BB/F/IC then pos full
        assert_eq!(order.steps[12], (pos, PaintPhase::RootBackground));
        assert_eq!(order.len(), 16);
    }

    #[test]
    fn nested_child_sc_recurses_correctly() {
        // root → child(z=1) → grandchild(z=-1).
        // Между child.RB и child.BB должны вклиниться все 4 фазы grandchild-а.
        let tree = build_synthetic_tree(
            &[None, Some(1), Some(-1)],
            &[vec![1], vec![2], vec![]],
        );
        let order = PaintOrder::from_tree(&tree);
        let child = StackingContextId(1);
        let grand = StackingContextId(2);
        // root.RB, root.BB, root.F, root.IC, then child full (which includes grand inside)
        assert_eq!(order.steps[4], (child, PaintPhase::RootBackground));
        // grandchild (z=-1) приходит сразу после child.RB
        assert_eq!(order.steps[5], (grand, PaintPhase::RootBackground));
        assert_eq!(order.steps[8], (grand, PaintPhase::InlineContent));
        // child's own block/float/inline после grandchild
        assert_eq!(order.steps[9], (child, PaintPhase::BlockBackgrounds));
        assert_eq!(order.steps[10], (child, PaintPhase::Floats));
        assert_eq!(order.steps[11], (child, PaintPhase::InlineContent));
    }

    #[test]
    fn empty_tree_emits_no_steps() {
        // Деградирующий случай: contexts вообще пустой (например, layout
        // не отработал). Ничего эмитить нельзя — нет даже root-а.
        let tree = StackingTree { contexts: vec![] };
        let order = PaintOrder::from_tree(&tree);
        assert!(order.is_empty());
    }

    #[test]
    fn real_document_with_opacity_paints_child_after_root_inline() {
        // E2E через layout: `<div style="opacity:0.5">x</div>` — div создаёт
        // SC (z=None → phase 6). Дочерний SC рисуется после root.IC.
        let tree = build_tree(
            "<div>x</div>",
            "div { opacity: 0.5; }",
        );
        let order = PaintOrder::from_tree(&tree);
        // root + 1 child SC, всего 8 steps (4 + 4).
        assert_eq!(tree.contexts.len(), 2);
        assert_eq!(order.len(), 8);
        // root inline content на 4-й позиции, дочерний SC начинается с 5-й.
        assert_eq!(
            order.steps[3],
            (StackingContextId::ROOT, PaintPhase::InlineContent)
        );
        assert_eq!(
            order.steps[4].1,
            PaintPhase::RootBackground,
            "child SC начинается с RootBackground"
        );
        assert_eq!(order.steps[4].0, StackingContextId(1));
    }

    #[test]
    fn document_without_triggers_has_only_root() {
        // Простая страница без триггеров — все боксы остаются в root SC.
        let tree = build_tree("<p>a</p><p>b</p>", "");
        assert_eq!(tree.contexts.len(), 1);
        assert!(tree.root().children.is_empty());
    }

    #[test]
    fn opacity_lt_one_creates_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { opacity: 0.5; }");
        assert_eq!(tree.contexts.len(), 2);
        assert_eq!(tree.root().children.len(), 1);
        let child = &tree.contexts[1];
        assert_eq!(child.z_index, None);
    }

    #[test]
    fn opacity_one_does_not_create_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { opacity: 1; }");
        assert_eq!(tree.contexts.len(), 1);
    }

    #[test]
    fn transform_creates_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { transform: rotate(45deg); }");
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn filter_creates_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { filter: blur(2px); }");
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn clip_path_creates_stacking_context() {
        let tree = build_tree(
            "<div>x</div>",
            "div { clip-path: circle(50px at 50px 50px); }",
        );
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn backdrop_filter_creates_stacking_context() {
        // CSS Filter Effects L2 §2: backdrop-filter != none creates a stacking
        // context (regression: the trigger was missing, so the paint layer-ops
        // dropped PushBackdropFilter — empty display list for backdrop-only divs).
        let tree = build_tree("<div>x</div>", "div { backdrop-filter: blur(8px); }");
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn isolation_isolate_creates_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { isolation: isolate; }");
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn isolation_auto_does_not_create_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { isolation: auto; }");
        assert_eq!(tree.contexts.len(), 1);
    }

    #[test]
    fn mix_blend_mode_non_normal_creates_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { mix-blend-mode: multiply; }");
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn position_static_with_z_index_ignored() {
        // z-index применяется только к positioned (или flex/grid-item) — на
        // static-боксах он по spec игнорируется.
        let tree = build_tree("<div>x</div>", "div { z-index: 5; }");
        assert_eq!(tree.contexts.len(), 1);
    }

    #[test]
    fn position_relative_with_z_index_creates_stacking_context() {
        let tree = build_tree(
            "<div>x</div>",
            "div { position: relative; z-index: 5; }",
        );
        assert_eq!(tree.contexts.len(), 2);
        assert_eq!(tree.contexts[1].z_index, Some(5));
    }

    #[test]
    fn position_relative_without_z_index_does_not_create_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { position: relative; }");
        assert_eq!(tree.contexts.len(), 1);
    }

    #[test]
    fn position_fixed_creates_stacking_context_even_without_z_index() {
        let tree = build_tree("<div>x</div>", "div { position: fixed; }");
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn will_change_transform_creates_stacking_context() {
        let tree = build_tree("<div>x</div>", "div { will-change: transform; }");
        assert_eq!(tree.contexts.len(), 2);
    }

    #[test]
    fn will_change_color_does_not_create_stacking_context() {
        // color — не stacking-property, will-change на нём не создаёт SC.
        let tree = build_tree("<div>x</div>", "div { will-change: color; }");
        assert_eq!(tree.contexts.len(), 1);
    }

    #[test]
    fn nested_stacking_contexts_form_hierarchy() {
        // .outer создаёт SC (opacity); .inner — тоже (transform).
        // Дерево: root → outer → inner.
        let html = r#"<div class="outer"><div class="inner">x</div></div>"#;
        let css = ".outer { opacity: 0.5; } .inner { transform: scale(2); }";
        let tree = build_tree(html, css);
        assert_eq!(tree.contexts.len(), 3);
        // root → [outer]
        assert_eq!(tree.root().children.len(), 1);
        let outer_id = tree.root().children[0];
        // outer → [inner]
        assert_eq!(tree.contexts[outer_id.0 as usize].children.len(), 1);
    }

    #[test]
    fn descendant_without_trigger_attaches_to_nearest_ancestor_sc() {
        // .outer создаёт SC; .inner — нет. .inner всё равно «прилипает» к
        // outer-SC, но как обычный box, не как child SC.
        let html = r#"<div class="outer"><div class="inner">x</div></div>"#;
        let css = ".outer { opacity: 0.5; }";
        let tree = build_tree(html, css);
        // SC: root + outer.
        assert_eq!(tree.contexts.len(), 2);
        let outer = &tree.contexts[1];
        // outer не имеет child SC — inner унаследовал outer-SC, но без
        // создания собственного.
        assert!(outer.children.is_empty());
    }

    #[test]
    fn children_sorted_by_z_index_with_stable_order() {
        // Три SC с z 10, -5, 3 → ожидаем порядок: -5, 3, 10.
        let html = r#"<div class="a"></div><div class="b"></div><div class="c"></div>"#;
        let css = "
            div { position: relative; }
            .a { z-index: 10; }
            .b { z-index: -5; }
            .c { z-index: 3; }
        ";
        let tree = build_tree(html, css);
        assert_eq!(tree.root().children.len(), 3);
        let z: Vec<i32> = tree
            .root()
            .children
            .iter()
            .map(|id| tree.contexts[id.0 as usize].z_index.unwrap_or(0))
            .collect();
        assert_eq!(z, vec![-5, 3, 10]);
    }

    #[test]
    fn auto_z_treated_as_zero_in_sort() {
        // Три SC: opacity-trigger (z=auto → 0), positioned z=-1, positioned z=+1.
        // Порядок должен быть: -1, auto/0, +1.
        let html = r#"<div class="neg"></div><div class="auto"></div><div class="pos"></div>"#;
        let css = "
            .neg { position: relative; z-index: -1; }
            .auto { opacity: 0.5; }
            .pos { position: relative; z-index: 1; }
        ";
        let tree = build_tree(html, css);
        assert_eq!(tree.root().children.len(), 3);
        let z: Vec<Option<i32>> = tree
            .root()
            .children
            .iter()
            .map(|id| tree.contexts[id.0 as usize].z_index)
            .collect();
        assert_eq!(z, vec![Some(-1), None, Some(1)]);
    }

    #[test]
    fn stable_sort_preserves_tree_order_for_equal_z() {
        // Два SC с одинаковым z=5 — порядок в дереве должен сохраниться
        // (a перед b), так как stable sort.
        let html = r#"<div class="a"></div><div class="b"></div>"#;
        let css = "
            div { position: relative; z-index: 5; }
        ";
        let tree = build_tree(html, css);
        assert_eq!(tree.root().children.len(), 2);
        // Сравниваем по тому, какой LayoutBox-узел они представляют — но
        // у нас в SC нет ссылки на узел; полагаемся на порядок создания:
        // первый child вставлен раньше → имеет меньший id.
        let ids: Vec<u32> = tree.root().children.iter().map(|id| id.0).collect();
        assert!(ids[0] < ids[1], "expected stable order, got {ids:?}");
    }
}
