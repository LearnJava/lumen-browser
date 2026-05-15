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
/// версию через `active_tree`/`active_trees`.
///
/// **Two-buffer commit-модель** (как в Chromium):
/// - `commit(trees, layer_tree)` — main thread кладёт новое состояние в
///   pending-буфер. Не делает active-промоушн сразу: рендер всё ещё видит
///   старую активную версию, никакого tearing-а кадра.
/// - `flush_pending() -> bool` — compositor (Phase 1+ — отдельный поток)
///   атомарно промотирует pending → active в начале своего «vsync-tick»-а.
///   `true` если промоушн был; `false` если pending пуст.
/// - `active_tree()` / `active_trees()` — то, что рендерится в текущем кадре.
///
/// Phase 0: один поток. Shell делает `commit(...); compositor.flush_pending();
/// renderer.render(...)` подряд. Promotion синхронный; атомарность важна
/// только когда commit и flush разъезжаются на потоки.
///
/// `commit` принимает `Arc<PropertyTrees>` — owned snapshot от main thread-а,
/// который compositor хранит без копирования. Layer tree —
/// `Box<dyn LayerTree + Send + Sync>` чтобы можно было передавать различные
/// impl-ы и (в будущем) перекладывать между потоками.
pub trait Compositor {
    /// Кладёт новое состояние в pending-буфер. Active не меняется — старая
    /// сцена продолжает рендериться до следующего `flush_pending`. Повторный
    /// `commit` до flush-а перезаписывает pending (последний коммит выигрывает —
    /// каждые 16 мс рендерить промежуточный layout не нужно).
    fn commit(&mut self, trees: Arc<PropertyTrees>, layer_tree: Box<dyn LayerTree + Send + Sync>);

    /// Атомарно промотирует pending → active. Возвращает `true`, если был
    /// pending для промоушна; `false`, если новых обновлений не было (active
    /// остаётся прежним, ре-рендерить не нужно).
    fn flush_pending(&mut self) -> bool;

    /// Есть ли pending-обновление, ожидающее flush-а. Используется
    /// рендер-loop-ом, чтобы решить, нужен ли invalidate / repaint.
    fn has_pending(&self) -> bool;

    /// Активный layer tree — то, что рендерится в текущем кадре.
    /// `None` пока не было ни одного `flush_pending`-а.
    fn active_tree(&self) -> Option<&dyn LayerTree>;

    /// Активные property trees — то, что рендерится в текущем кадре.
    /// `None` пока не было ни одного `flush_pending`-а.
    fn active_trees(&self) -> Option<&Arc<PropertyTrees>>;
}

/// Phase 0 in-process compositor: один поток, синхронный swap, без Mutex.
/// Будет заменён на отдельный thread в roadmap «compositor thread»; API
/// уже two-buffer-ный, чтобы переход был drop-in (поменять только Mutex
/// вокруг pending-слотов).
pub struct InProcessCompositor {
    pending_layer_tree: Option<Box<dyn LayerTree + Send + Sync>>,
    pending_trees: Option<Arc<PropertyTrees>>,
    active_layer_tree: Option<Box<dyn LayerTree + Send + Sync>>,
    active_trees: Option<Arc<PropertyTrees>>,
}

impl InProcessCompositor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending_layer_tree: None,
            pending_trees: None,
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
        self.pending_trees = Some(trees);
        self.pending_layer_tree = Some(layer_tree);
    }

    fn flush_pending(&mut self) -> bool {
        let Some(trees) = self.pending_trees.take() else {
            return false;
        };
        let layer_tree = self
            .pending_layer_tree
            .take()
            .expect("pending_trees и pending_layer_tree всегда set/unset вместе");
        self.active_trees = Some(trees);
        self.active_layer_tree = Some(layer_tree);
        true
    }

    fn has_pending(&self) -> bool {
        self.pending_trees.is_some()
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
        assert!(!comp.has_pending());
    }

    #[test]
    fn commit_does_not_promote_immediately() {
        let mut comp = InProcessCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            trees,
            Box::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );
        assert!(comp.has_pending());
        assert!(
            comp.active_tree().is_none(),
            "commit без flush_pending не должен менять active"
        );
        assert!(comp.active_trees().is_none());
    }

    #[test]
    fn flush_pending_promotes_pending_to_active() {
        let mut comp = InProcessCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            trees.clone(),
            Box::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );
        assert!(comp.flush_pending(), "был pending — flush возвращает true");
        let active = comp.active_tree().expect("после flush есть active");
        assert_eq!(active.layer_count(), 1);
        assert_eq!(active.layer(0).unwrap().bbox(), bbox);
        assert!(Arc::ptr_eq(
            comp.active_trees().expect("trees promoted"),
            &trees,
        ));
        assert!(!comp.has_pending(), "после flush pending пуст");
    }

    #[test]
    fn flush_pending_returns_false_when_empty() {
        let mut comp = InProcessCompositor::new();
        assert!(!comp.flush_pending());
    }

    #[test]
    fn commit_overwrites_pending() {
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
        assert!(comp.flush_pending());
        let active = comp.active_tree().unwrap();
        assert_eq!(
            active.layer(0).unwrap().bbox(),
            bbox_b,
            "последний commit до flush выигрывает"
        );
    }

    #[test]
    fn active_persists_across_flush_with_no_pending() {
        let mut comp = InProcessCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 100.0, 100.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            trees,
            Box::new(BasicLayerTree::single_layer(bbox, Vec::new())),
        );
        comp.flush_pending();
        // Без нового commit-а второй flush не меняет active.
        assert!(!comp.flush_pending());
        assert_eq!(comp.active_tree().unwrap().layer(0).unwrap().bbox(), bbox);
    }
}
