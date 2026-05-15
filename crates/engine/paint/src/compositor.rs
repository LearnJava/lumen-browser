//! Compositor scaffolding (P2 1B, interface-first).
//!
//! Compositor — отдельный слой между layout-вычислением и pixel-paint-ом,
//! который владеет иерархией Layer-ов (`LayerTree`) и принимает на каждом
//! кадре «изменения сцены» от main thread-а (`commit`). Главная польза —
//! отдельная фаза, в которую можно вынести скролл / transform / opacity без
//! relayout-а (off-main-thread scroll, GPU-accelerated transform).
//!
//! Phase 0 — только контракты и in-process trivial-impl-ы. Реальный
//! compositor thread, blend-pipeline, hit testing — следующие задачи (P2
//! 2A/2B/3B/4). Подсистемы строятся **против trait-ов** ниже, не против
//! конкретных типов: drop-in переход на реальный impl без правки потребителей.
//!
//! Архитектура (как в Chromium):
//! - `LayerTree` — иерархия layer-ов (root + детей); каждый layer — bbox +
//!   ссылка на `StackingContextId` + локальный display-list. Не владеет
//!   property trees — те держатся compositor-ом отдельно и индексируются
//!   по `PropertyTreeNodeId`.
//! - `PropertyTrees` — четыре дерева (transform / scroll / effect / clip),
//!   реализованы в `lumen-layout::property_trees` (P1 Sprint 0). Mutations
//!   property-узлов compositor применяет без relayout-а.
//! - `Compositor` — принимает `commit(trees, layer_tree)`; внутри ведёт
//!   two-buffer (pending / active) и отдаёт активный tree на render.
//!
//! Phase 0 ограничения:
//! - `BasicLayerTree::single_layer(commands)` — один layer на всю страницу
//!   (root stacking context). Реальное разбиение по stacking contexts —
//!   задача P1 п.2A (наполнение `StackingContextId`).
//! - `InProcessCompositor` синхронный: commit копирует layer tree в active
//!   немедленно (без атомарного swap и буфера pending). Two-buffer-модель
//!   — задача compositor thread (P2, после Sprint 0).
//! - `Compositor::commit` не использует `PropertyTrees` — Phase 0 рендер
//!   плоский (без transform / opacity / scroll). API уже принимает trees,
//!   чтобы драматически не менять сигнатуру при подключении реального
//!   compositor pipeline.

use std::sync::Arc;

use lumen_core::geom::Rect;
use lumen_layout::{PropertyTrees, StackingContextId};

use crate::display_list::DisplayCommand;

/// Один layer: bbox + связь со stacking context-ом + локальный display list.
///
/// `bbox` — координаты в пространстве экрана (Phase 0; в фазе с transform-ами
/// тут будут координаты до transform-а).
pub trait Layer {
    fn bbox(&self) -> Rect;
    fn stacking_context(&self) -> StackingContextId;
    fn commands(&self) -> &[DisplayCommand];
}

/// Коллекция layer-ов. Trait-обстракция, чтобы compositor мог принимать
/// разные impl-ы (например, immutable snapshot vs. mutable builder).
pub trait LayerTree {
    fn layer_count(&self) -> usize;
    fn layer(&self, idx: usize) -> Option<&dyn Layer>;
}

/// Sprint 0 / Phase 0 concrete impl. Owned struct без интерлевания —
/// caller строит `Vec<BasicLayer>` и оборачивает.
#[derive(Debug, Clone)]
pub struct BasicLayer {
    pub bbox: Rect,
    pub stacking_context: StackingContextId,
    pub commands: Vec<DisplayCommand>,
}

impl Layer for BasicLayer {
    fn bbox(&self) -> Rect {
        self.bbox
    }
    fn stacking_context(&self) -> StackingContextId {
        self.stacking_context
    }
    fn commands(&self) -> &[DisplayCommand] {
        &self.commands
    }
}

/// Sprint 0 / Phase 0 concrete impl. Один display-list = один layer
/// (root stacking context). Phase 1+ — разбиение по stacking contexts.
#[derive(Debug, Clone, Default)]
pub struct BasicLayerTree {
    pub layers: Vec<BasicLayer>,
}

impl BasicLayerTree {
    /// Пустой tree (нет ни одного layer-а). Полезен как начальное состояние
    /// `InProcessCompositor` до первого commit-а.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Phase 0: оборачивает весь display-list в один layer на bbox-страницы
    /// с `StackingContextId::ROOT`. При появлении нескольких stacking
    /// contexts в P1 п.2A — этот helper заменяется на разбиение в
    /// `build_display_list` (или новый `build_layer_tree`).
    #[must_use]
    pub fn single_layer(bbox: Rect, commands: Vec<DisplayCommand>) -> Self {
        Self {
            layers: vec![BasicLayer {
                bbox,
                stacking_context: StackingContextId::ROOT,
                commands,
            }],
        }
    }
}

impl LayerTree for BasicLayerTree {
    fn layer_count(&self) -> usize {
        self.layers.len()
    }
    fn layer(&self, idx: usize) -> Option<&dyn Layer> {
        self.layers.get(idx).map(|l| l as &dyn Layer)
    }
}

/// Compositor: получает обновления сцены через `commit`, отдаёт активную
/// версию через `active_tree`. Реальный compositor работает в отдельном
/// потоке с two-buffer (pending → atomic swap → active); Phase 0 —
/// синхронный `InProcessCompositor`.
///
/// `commit` принимает `Arc<PropertyTrees>` — owned snapshot от main thread-а,
/// который compositor хранит без копирования. Layer tree — `Box<dyn LayerTree>`
/// чтобы можно было передавать различные impl-ы.
pub trait Compositor {
    /// Принимает новую сцену. Phase 0 impl-ы кладут её сразу в active.
    fn commit(&mut self, trees: Arc<PropertyTrees>, layer_tree: Box<dyn LayerTree + Send + Sync>);

    /// Активный layer tree — то, что рендерится в текущем кадре.
    /// `None` пока не было ни одного commit-а.
    fn active_tree(&self) -> Option<&dyn LayerTree>;

    /// Активные property trees — то, что рендерится в текущем кадре.
    /// `None` пока не было ни одного commit-а.
    fn active_trees(&self) -> Option<&Arc<PropertyTrees>>;
}

/// Phase 0 in-process compositor: один поток, синхронный swap. Будет заменён
/// на отдельный thread с two-buffer (pending / active + atomic swap) в P2
/// «compositor thread + property trees + layer tree».
pub struct InProcessCompositor {
    active_layer_tree: Option<Box<dyn LayerTree + Send + Sync>>,
    active_trees: Option<Arc<PropertyTrees>>,
}

impl InProcessCompositor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            active_layer_tree: None,
            active_trees: None,
        }
    }
}

impl Default for InProcessCompositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compositor for InProcessCompositor {
    fn commit(&mut self, trees: Arc<PropertyTrees>, layer_tree: Box<dyn LayerTree + Send + Sync>) {
        self.active_trees = Some(trees);
        self.active_layer_tree = Some(layer_tree);
    }

    fn active_tree(&self) -> Option<&dyn LayerTree> {
        // Сужаем `dyn LayerTree + Send + Sync` до `dyn LayerTree`: trait
        // object с auto-trait-ами — отдельный тип, нужен явный reborrow.
        self.active_layer_tree.as_ref().map(|b| &**b as &dyn LayerTree)
    }

    fn active_trees(&self) -> Option<&Arc<PropertyTrees>> {
        self.active_trees.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_layout::Color;

    fn sample_commands() -> Vec<DisplayCommand> {
        vec![DisplayCommand::FillRect {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            color: Color::BLACK,
        }]
    }

    #[test]
    fn empty_tree_has_no_layers() {
        let tree = BasicLayerTree::empty();
        assert_eq!(tree.layer_count(), 0);
        assert!(tree.layer(0).is_none());
    }

    #[test]
    fn single_layer_wraps_one_layer() {
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let tree = BasicLayerTree::single_layer(bbox, sample_commands());
        assert_eq!(tree.layer_count(), 1);
        let layer = tree.layer(0).unwrap();
        assert_eq!(layer.bbox(), bbox);
        assert_eq!(layer.stacking_context(), StackingContextId::ROOT);
        assert_eq!(layer.commands().len(), 1);
    }

    #[test]
    fn compositor_starts_empty() {
        let comp = InProcessCompositor::new();
        assert!(comp.active_tree().is_none());
        assert!(comp.active_trees().is_none());
    }

    #[test]
    fn compositor_commits_layer_tree() {
        let mut comp = InProcessCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let tree = BasicLayerTree::single_layer(bbox, sample_commands());
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(trees.clone(), Box::new(tree));
        let active = comp.active_tree().expect("commit makes tree active");
        assert_eq!(active.layer_count(), 1);
        assert_eq!(active.layer(0).unwrap().bbox(), bbox);
        assert!(Arc::ptr_eq(
            comp.active_trees().expect("trees committed"),
            &trees,
        ));
    }

    #[test]
    fn compositor_replaces_on_subsequent_commits() {
        let mut comp = InProcessCompositor::new();
        let bbox_a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let bbox_b = Rect::new(50.0, 50.0, 200.0, 200.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            trees.clone(),
            Box::new(BasicLayerTree::single_layer(bbox_a, Vec::new())),
        );
        comp.commit(
            trees,
            Box::new(BasicLayerTree::single_layer(bbox_b, Vec::new())),
        );
        let active = comp.active_tree().unwrap();
        assert_eq!(active.layer(0).unwrap().bbox(), bbox_b);
    }
}
