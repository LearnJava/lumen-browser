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
fn creates_stacking_context(style: &ComputedStyle) -> bool {
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
    if !style.transform.is_empty() {
        return true;
    }
    if !style.filter.is_empty() {
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
fn box_can_own_stacking_context(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block | BoxKind::Image { .. })
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
    fn paint_order_from_empty_tree_covers_root_phases() {
        let tree = StackingTree::empty_root();
        let order = PaintOrder::from_tree(&tree);
        assert_eq!(order.len(), 7);
        for (i, phase) in PaintPhase::ORDER.iter().enumerate() {
            assert_eq!(order.steps[i], (StackingContextId::ROOT, *phase));
        }
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
