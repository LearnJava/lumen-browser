//! Compositor scaffolding (P2 1B, interface-first).
//!
//! Compositor — отдельный слой между layout-вычислением и pixel-paint-ом,
//! который владеет иерархией Layer-ов (`LayerTree`) и принимает на каждом
//! кадре «изменения сцены» от main thread-а (`commit`). Главная польза —
//! отдельная фаза, в которую можно вынести скролл / transform / opacity без
//! relayout-а (off-main-thread scroll, GPU-accelerated transform).
//!
//! Phase 0 — два concrete impl-а одного trait-а:
//! - `InProcessCompositor` — single-thread, синхронный, без Mutex.
//! - `ThreadedCompositor` + `ThreadedCompositorHandle` — Mutex-обёрнутая
//!   версия. Main thread держит owner-а и шлёт `commit`, render/compositor
//!   thread держит cloned `ThreadedCompositorHandle` и читает active.
//!
//! Реальный compositor thread (отдельный поток с tick-loop-ом), blend-pipeline,
//! GPU-layer pipeline — следующие задачи (P2 4, P2 1B шаг (c)). Сейчас оба
//! impl-а используют одну и ту же two-buffer-модель и API — drop-in переход
//! между ними не меняет потребителя.
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
//!   two-buffer (pending / active) и отдаёт активный snapshot через `Arc`.
//!
//! **Почему `Arc<dyn LayerTree>` вместо `&dyn LayerTree` / `Box<dyn LayerTree>`:**
//! main thread пишет в pending, compositor/render thread читает active. Если
//! бы `active_tree()` возвращал `&dyn LayerTree` на поле внутри `Mutex`, нам
//! пришлось бы держать `MutexGuard` живым на время рендера кадра — это
//! блокировало бы main thread от следующего `commit`. Возврат cloned `Arc` —
//! O(1) atomic refcount bump, lock сразу освобождается, main thread свободен.
//!
//! Phase 0 ограничения:
//! - `BasicLayerTree::single_layer(commands)` — один layer на всю страницу
//!   (root stacking context). Реальное разбиение по stacking contexts —
//!   задача P1 п.2A (наполнение `StackingContextId`).
//! - `Compositor::commit` не использует `PropertyTrees` в рендере — Phase 0
//!   рендер плоский (без transform / opacity / scroll). API уже принимает
//!   trees, чтобы драматически не менять сигнатуру при подключении реального
//!   compositor pipeline.

use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
/// - `active_tree()` / `active_trees()` — Arc-snapshot активной версии,
///   рендерится в текущем кадре. `Arc::clone` на возврате; не держит lock.
///
/// `commit` принимает `Arc<dyn LayerTree + Send + Sync>` — caller владеет
/// своим Arc, compositor клонирует refcount себе. Это позволяет одному
/// snapshot-у одновременно жить в pending одного compositor-а и в active
/// другого (или в active + render-loop в одной сессии).
pub trait Compositor {
    /// Кладёт новое состояние в pending-буфер. Active не меняется — старая
    /// сцена продолжает рендериться до следующего `flush_pending`. Повторный
    /// `commit` до flush-а перезаписывает pending (последний коммит выигрывает —
    /// каждые 16 мс рендерить промежуточный layout не нужно).
    fn commit(
        &mut self,
        trees: Arc<PropertyTrees>,
        layer_tree: Arc<dyn LayerTree + Send + Sync>,
    );

    /// Атомарно промотирует pending → active. Возвращает `true`, если был
    /// pending для промоушна; `false`, если новых обновлений не было (active
    /// остаётся прежним, ре-рендерить не нужно).
    fn flush_pending(&mut self) -> bool;

    /// Есть ли pending-обновление, ожидающее flush-а. Используется
    /// рендер-loop-ом, чтобы решить, нужен ли invalidate / repaint.
    fn has_pending(&self) -> bool;

    /// Snapshot активного layer tree — то, что рендерится в текущем кадре.
    /// `None` пока не было ни одного `flush_pending`-а.
    fn active_tree(&self) -> Option<Arc<dyn LayerTree + Send + Sync>>;

    /// Snapshot активных property trees — то, что рендерится в текущем кадре.
    /// `None` пока не было ни одного `flush_pending`-а.
    fn active_trees(&self) -> Option<Arc<PropertyTrees>>;
}

/// Single-thread in-process compositor: синхронный swap, без Mutex.
/// Pending/active живут как `Arc`-snapshot-ы, чтобы потребитель мог
/// клонировать active и хранить его за пределами compositor-а (например,
/// renderer держит copy на время кадра).
pub struct InProcessCompositor {
    pending_layer_tree: Option<Arc<dyn LayerTree + Send + Sync>>,
    pending_trees: Option<Arc<PropertyTrees>>,
    active_layer_tree: Option<Arc<dyn LayerTree + Send + Sync>>,
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
    fn commit(
        &mut self,
        trees: Arc<PropertyTrees>,
        layer_tree: Arc<dyn LayerTree + Send + Sync>,
    ) {
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

    fn active_tree(&self) -> Option<Arc<dyn LayerTree + Send + Sync>> {
        self.active_layer_tree.clone()
    }

    fn active_trees(&self) -> Option<Arc<PropertyTrees>> {
        self.active_trees.clone()
    }
}

// ---------------------------------------------------------------------------
// VsyncNotifier — condvar-based wakeup для compositor thread (P2 1B.2)
// ---------------------------------------------------------------------------

/// Condvar-pair для передачи vsync/commit-нотификации compositor thread-у.
///
/// Dirty-флаг устраняет потерю нотификации: если `notify()` вызван в то
/// время, когда thread выполняет `flush_pending()` (не ждёт на condvar),
/// флаг остаётся `true` и thread увидит его при следующей проверке перед
/// `wait_timeout`.
pub(crate) struct VsyncNotifier {
    dirty: Mutex<bool>,
    cond: Condvar,
}

impl VsyncNotifier {
    pub(crate) fn new() -> Self {
        Self {
            dirty: Mutex::new(false),
            cond: Condvar::new(),
        }
    }

    /// Вызывается из `commit()` — будит compositor thread немедленно.
    pub(crate) fn notify(&self) {
        *self.dirty.lock().expect("VsyncNotifier dirty mutex poisoned") = true;
        self.cond.notify_one();
    }

    /// Блокируется до нотификации или timeout-а; сбрасывает dirty-флаг.
    /// Если `notify()` был вызван до входа в метод (dirty=true) — возвращает
    /// немедленно без ожидания.
    pub(crate) fn wait_for_next_tick(&self, timeout: Duration) {
        let mut dirty = self
            .dirty
            .lock()
            .expect("VsyncNotifier dirty mutex poisoned");
        if !*dirty {
            let (guard, _) = self
                .cond
                .wait_timeout(dirty, timeout)
                .expect("VsyncNotifier condvar poisoned");
            dirty = guard;
        }
        *dirty = false;
    }
}

// ---------------------------------------------------------------------------

/// Внутреннее shared state ThreadedCompositor-а. Один Mutex на все четыре
/// слота (pending+active × layer_tree+trees) — простая модель, гарантирует
/// что commit и flush видят consistent состояние. Lock contention пока не
/// проблема: Phase 0 рендер на main thread, commit-ы редкие (1 на кадр).
struct ThreadedState {
    pending_layer_tree: Option<Arc<dyn LayerTree + Send + Sync>>,
    pending_trees: Option<Arc<PropertyTrees>>,
    active_layer_tree: Option<Arc<dyn LayerTree + Send + Sync>>,
    active_trees: Option<Arc<PropertyTrees>>,
}

impl ThreadedState {
    fn new() -> Self {
        Self {
            pending_layer_tree: None,
            pending_trees: None,
            active_layer_tree: None,
            active_trees: None,
        }
    }
}

/// Thread-safe compositor: тот же API two-buffer-а, но `commit` и
/// `flush_pending` могут вызываться из разных threads. Используется когда
/// main thread шлёт commit-ы, а compositor/render thread читает active.
///
/// `ThreadedCompositor` — *owner*-структура (реализует [`Compositor`]
/// trait через `&mut self` для drop-in замены `InProcessCompositor`).
/// Для shared доступа из других threads — [`ThreadedCompositor::handle`].
///
/// **Vsync tick-loop (P2 1B.2):** каждый `commit()` вызывает
/// `notifier.notify()`, мгновенно будя compositor thread. Без commit-ов
/// thread спит ровно `TARGET_FRAME_DURATION` (≈16.67 мс = 60 fps).
pub struct ThreadedCompositor {
    state: Arc<Mutex<ThreadedState>>,
    notifier: Arc<VsyncNotifier>,
}

impl ThreadedCompositor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ThreadedState::new())),
            notifier: Arc::new(VsyncNotifier::new()),
        }
    }

    /// Cheap-clone handle для другого потока: shared доступ к тому же
    /// state-у и notifier-у. Используется когда render/compositor thread
    /// должен читать active, пока main thread держит owner-а и пишет pending.
    #[must_use]
    pub fn handle(&self) -> ThreadedCompositorHandle {
        ThreadedCompositorHandle {
            state: Arc::clone(&self.state),
            notifier: Arc::clone(&self.notifier),
        }
    }
}

impl Default for ThreadedCompositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compositor for ThreadedCompositor {
    fn commit(
        &mut self,
        trees: Arc<PropertyTrees>,
        layer_tree: Arc<dyn LayerTree + Send + Sync>,
    ) {
        {
            let mut guard = self
                .state
                .lock()
                .expect("ThreadedCompositor state mutex poisoned");
            guard.pending_trees = Some(trees);
            guard.pending_layer_tree = Some(layer_tree);
        }
        self.notifier.notify();
    }

    fn flush_pending(&mut self) -> bool {
        let mut guard = self
            .state
            .lock()
            .expect("ThreadedCompositor state mutex poisoned");
        let Some(trees) = guard.pending_trees.take() else {
            return false;
        };
        let layer_tree = guard
            .pending_layer_tree
            .take()
            .expect("pending_trees и pending_layer_tree всегда set/unset вместе");
        guard.active_trees = Some(trees);
        guard.active_layer_tree = Some(layer_tree);
        true
    }

    fn has_pending(&self) -> bool {
        self.state
            .lock()
            .expect("ThreadedCompositor state mutex poisoned")
            .pending_trees
            .is_some()
    }

    fn active_tree(&self) -> Option<Arc<dyn LayerTree + Send + Sync>> {
        self.state
            .lock()
            .expect("ThreadedCompositor state mutex poisoned")
            .active_layer_tree
            .clone()
    }

    fn active_trees(&self) -> Option<Arc<PropertyTrees>> {
        self.state
            .lock()
            .expect("ThreadedCompositor state mutex poisoned")
            .active_trees
            .clone()
    }
}

/// Cheap-clone handle на тот же state, что и parent [`ThreadedCompositor`].
/// `&self` методы — interior mutability через Mutex; несколько threads
/// могут держать свои clone-ы handle-а одновременно.
///
/// Семантика: handle и owner равноправны. owner-у не нужен «exclusive»-доступ —
/// он просто реализует [`Compositor`] trait (`&mut self`) для совместимости
/// с in-process call sites, а handle даёт `&self` API для shared-thread
/// потребителей. Внутри оба используют один и тот же `Arc<Mutex<...>>`.
///
/// `notifier` разделяется с owner-ом: `commit()` через handle тоже будит
/// compositor thread.
#[derive(Clone)]
pub struct ThreadedCompositorHandle {
    state: Arc<Mutex<ThreadedState>>,
    notifier: Arc<VsyncNotifier>,
}

impl ThreadedCompositorHandle {
    pub fn commit(
        &self,
        trees: Arc<PropertyTrees>,
        layer_tree: Arc<dyn LayerTree + Send + Sync>,
    ) {
        {
            let mut guard = self
                .state
                .lock()
                .expect("ThreadedCompositorHandle state mutex poisoned");
            guard.pending_trees = Some(trees);
            guard.pending_layer_tree = Some(layer_tree);
        }
        self.notifier.notify();
    }

    pub fn flush_pending(&self) -> bool {
        let mut guard = self
            .state
            .lock()
            .expect("ThreadedCompositorHandle state mutex poisoned");
        let Some(trees) = guard.pending_trees.take() else {
            return false;
        };
        let layer_tree = guard
            .pending_layer_tree
            .take()
            .expect("pending_trees и pending_layer_tree всегда set/unset вместе");
        guard.active_trees = Some(trees);
        guard.active_layer_tree = Some(layer_tree);
        true
    }

    #[must_use]
    pub fn has_pending(&self) -> bool {
        self.state
            .lock()
            .expect("ThreadedCompositorHandle state mutex poisoned")
            .pending_trees
            .is_some()
    }

    #[must_use]
    pub fn active_tree(&self) -> Option<Arc<dyn LayerTree + Send + Sync>> {
        self.state
            .lock()
            .expect("ThreadedCompositorHandle state mutex poisoned")
            .active_layer_tree
            .clone()
    }

    #[must_use]
    pub fn active_trees(&self) -> Option<Arc<PropertyTrees>> {
        self.state
            .lock()
            .expect("ThreadedCompositorHandle state mutex poisoned")
            .active_trees
            .clone()
    }
}

// ---------------------------------------------------------------------------
// CompositorThread — реальный OS-поток с vsync tick-loop (P2 1B.1 + 1B.2)
// ---------------------------------------------------------------------------

/// Максимальная длина одного «vsync-тика» ≈ 16.67 мс = 60 fps.
/// Compositor thread спит не дольше этого значения; `commit()` будит его
/// раньше через `VsyncNotifier`. Значение 16_667 мкс выбрано точнее 16 мс —
/// избегает постепенного drift-а при непрерывном рендере без commit-ов.
const TARGET_FRAME_DURATION: Duration = Duration::from_micros(16_667);

/// Реальный compositor thread: отдельный OS-поток с vsync tick-loop.
///
/// Жизненный цикл:
/// - `CompositorThread::spawn(handle)` — запускает поток, возвращает owner.
/// - Пока owner живёт — поток работает.
/// - `CompositorThread::shutdown()` — выставляет shutdown-флаг, будит поток
///   через notifier и join-ит его. Поток выходит не дольше чем через один тик.
/// - Drop без явного `shutdown()` — выставляет флаг + notify, но НЕ join-ит.
///   Join в Drop — блокирующая операция, непредсказуема при раскрутке стека.
///
/// **Vsync tick-loop (P2 1B.2):**
/// thread спит на `notifier.wait_for_next_tick(TARGET_FRAME_DURATION)`:
/// - просыпается немедленно когда `commit()` вызывает `notifier.notify()`;
/// - или по таймауту `TARGET_FRAME_DURATION` (~16.67 мс = 60 fps) если
///   commit-ов не было — чтобы не пропустить idle-фреймы.
pub struct CompositorThread {
    shutdown: Arc<AtomicBool>,
    notifier: Arc<VsyncNotifier>,
    join_handle: Option<JoinHandle<()>>,
}

impl CompositorThread {
    /// Запускает compositor thread. `handle` — разделяемый доступ к state
    /// и notifier-у того же `ThreadedCompositor`, которым владеет main thread.
    pub fn spawn(handle: ThreadedCompositorHandle) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let notifier = Arc::clone(&handle.notifier);
        let join_handle = thread::spawn(move || {
            compositor_thread_main(handle, shutdown_flag);
        });
        Self {
            shutdown,
            notifier,
            join_handle: Some(join_handle),
        }
    }

    /// Запрашивает завершение потока и блокируется до его выхода.
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Будим поток — он может спать на condvar до TARGET_FRAME_DURATION.
        self.notifier.notify();
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for CompositorThread {
    fn drop(&mut self) {
        // Сигнализируем выход и будим поток — не join-им, Drop не место для блокировки.
        self.shutdown.store(true, Ordering::Relaxed);
        self.notifier.notify();
    }
}

fn compositor_thread_main(handle: ThreadedCompositorHandle, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        handle.notifier.wait_for_next_tick(TARGET_FRAME_DURATION);
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        handle.flush_pending();
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

    // --- InProcessCompositor ---

    #[test]
    fn in_process_compositor_starts_empty() {
        let comp = InProcessCompositor::new();
        assert!(comp.active_tree().is_none());
        assert!(comp.active_trees().is_none());
        assert!(!comp.has_pending());
    }

    #[test]
    fn in_process_commit_does_not_promote_immediately() {
        let mut comp = InProcessCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            trees,
            Arc::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );
        assert!(comp.has_pending());
        assert!(
            comp.active_tree().is_none(),
            "commit без flush_pending не должен менять active"
        );
        assert!(comp.active_trees().is_none());
    }

    #[test]
    fn in_process_flush_pending_promotes_pending_to_active() {
        let mut comp = InProcessCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            Arc::clone(&trees),
            Arc::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );
        assert!(comp.flush_pending(), "был pending — flush возвращает true");
        let active = comp.active_tree().expect("после flush есть active");
        assert_eq!(active.layer_count(), 1);
        assert_eq!(active.layer(0).unwrap().bbox(), bbox);
        assert!(Arc::ptr_eq(
            &comp.active_trees().expect("trees promoted"),
            &trees,
        ));
        assert!(!comp.has_pending(), "после flush pending пуст");
    }

    #[test]
    fn in_process_flush_pending_returns_false_when_empty() {
        let mut comp = InProcessCompositor::new();
        assert!(!comp.flush_pending());
    }

    #[test]
    fn in_process_commit_overwrites_pending() {
        let mut comp = InProcessCompositor::new();
        let bbox_a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let bbox_b = Rect::new(50.0, 50.0, 200.0, 200.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            Arc::clone(&trees),
            Arc::new(BasicLayerTree::single_layer(bbox_a, Vec::new())),
        );
        comp.commit(
            trees,
            Arc::new(BasicLayerTree::single_layer(bbox_b, Vec::new())),
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
    fn in_process_active_persists_across_flush_with_no_pending() {
        let mut comp = InProcessCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 100.0, 100.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            trees,
            Arc::new(BasicLayerTree::single_layer(bbox, Vec::new())),
        );
        comp.flush_pending();
        // Без нового commit-а второй flush не меняет active.
        assert!(!comp.flush_pending());
        assert_eq!(comp.active_tree().unwrap().layer(0).unwrap().bbox(), bbox);
    }

    // --- ThreadedCompositor (single-thread API parity с InProcessCompositor) ---

    #[test]
    fn threaded_compositor_starts_empty() {
        let comp = ThreadedCompositor::new();
        assert!(comp.active_tree().is_none());
        assert!(comp.active_trees().is_none());
        assert!(!comp.has_pending());
    }

    #[test]
    fn threaded_commit_does_not_promote_immediately() {
        let mut comp = ThreadedCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            trees,
            Arc::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );
        assert!(comp.has_pending());
        assert!(comp.active_tree().is_none());
        assert!(comp.active_trees().is_none());
    }

    #[test]
    fn threaded_flush_pending_promotes() {
        let mut comp = ThreadedCompositor::new();
        let bbox = Rect::new(0.0, 0.0, 800.0, 600.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            Arc::clone(&trees),
            Arc::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );
        assert!(comp.flush_pending());
        let active = comp.active_tree().expect("после flush есть active");
        assert_eq!(active.layer_count(), 1);
        assert!(Arc::ptr_eq(
            &comp.active_trees().expect("trees promoted"),
            &trees,
        ));
        assert!(!comp.has_pending());
    }

    #[test]
    fn threaded_flush_returns_false_when_empty() {
        let mut comp = ThreadedCompositor::new();
        assert!(!comp.flush_pending());
    }

    #[test]
    fn threaded_commit_overwrites_pending() {
        let mut comp = ThreadedCompositor::new();
        let bbox_a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let bbox_b = Rect::new(50.0, 50.0, 200.0, 200.0);
        let trees = Arc::new(PropertyTrees::empty());
        comp.commit(
            Arc::clone(&trees),
            Arc::new(BasicLayerTree::single_layer(bbox_a, Vec::new())),
        );
        comp.commit(
            trees,
            Arc::new(BasicLayerTree::single_layer(bbox_b, Vec::new())),
        );
        assert!(comp.flush_pending());
        let active = comp.active_tree().unwrap();
        assert_eq!(active.layer(0).unwrap().bbox(), bbox_b);
    }

    // --- ThreadedCompositorHandle: shared state с owner-ом ---

    #[test]
    fn handle_shares_state_with_owner() {
        let owner = ThreadedCompositor::new();
        let handle = owner.handle();
        let bbox = Rect::new(0.0, 0.0, 100.0, 100.0);
        let trees = Arc::new(PropertyTrees::empty());
        // commit через handle — owner видит pending.
        handle.commit(
            Arc::clone(&trees),
            Arc::new(BasicLayerTree::single_layer(bbox, Vec::new())),
        );
        assert!(owner.has_pending(), "handle.commit виден owner-у");
        // flush через handle — active появляется у обоих.
        assert!(handle.flush_pending());
        let active_owner = owner.active_tree().expect("owner видит active");
        let active_handle = handle.active_tree().expect("handle видит active");
        assert!(
            Arc::ptr_eq(&active_owner, &active_handle),
            "owner и handle отдают тот же Arc-snapshot"
        );
    }

    #[test]
    fn handle_clone_shares_state() {
        let owner = ThreadedCompositor::new();
        let handle_a = owner.handle();
        let handle_b = handle_a.clone();
        let bbox = Rect::new(0.0, 0.0, 50.0, 50.0);
        handle_a.commit(
            Arc::new(PropertyTrees::empty()),
            Arc::new(BasicLayerTree::single_layer(bbox, Vec::new())),
        );
        assert!(handle_b.has_pending(), "cloned handle видит pending");
        assert!(handle_b.flush_pending());
        assert_eq!(
            handle_a.active_tree().unwrap().layer(0).unwrap().bbox(),
            bbox
        );
    }

    // --- Multi-thread сценарий: main thread commit, другой thread flush+read ---

    #[test]
    fn cross_thread_commit_and_flush() {
        use std::thread;

        let mut owner = ThreadedCompositor::new();
        let handle = owner.handle();

        let bbox = Rect::new(0.0, 0.0, 400.0, 300.0);
        let trees = Arc::new(PropertyTrees::empty());
        owner.commit(
            trees,
            Arc::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );

        // Reader thread: видит pending, делает flush, читает active.
        let reader = thread::spawn(move || {
            assert!(handle.has_pending());
            assert!(handle.flush_pending());
            let active = handle.active_tree().expect("active после flush");
            active.layer(0).unwrap().bbox()
        });

        let observed_bbox = reader.join().expect("reader thread не паникнул");
        assert_eq!(observed_bbox, bbox);
        // owner после flush на reader-thread тоже видит active.
        assert_eq!(owner.active_tree().unwrap().layer(0).unwrap().bbox(), bbox);
    }

    #[test]
    fn cross_thread_concurrent_commits_last_wins() {
        use std::sync::{
            Barrier,
            atomic::{AtomicUsize, Ordering},
        };
        use std::thread;

        let owner = ThreadedCompositor::new();
        let barrier = Arc::new(Barrier::new(4));
        let counter = Arc::new(AtomicUsize::new(0));

        // 4 commit-thread-а одновременно — ровно один pending должен выжить.
        let mut writers = Vec::new();
        for i in 0..4 {
            let handle = owner.handle();
            let barrier = Arc::clone(&barrier);
            let counter = Arc::clone(&counter);
            writers.push(thread::spawn(move || {
                let side = 10.0 + i as f32;
                let bbox = Rect::new(0.0, 0.0, side, side);
                barrier.wait();
                handle.commit(
                    Arc::new(PropertyTrees::empty()),
                    Arc::new(BasicLayerTree::single_layer(bbox, Vec::new())),
                );
                counter.fetch_add(1, Ordering::SeqCst);
            }));
        }
        for w in writers {
            w.join().unwrap();
        }
        assert_eq!(counter.load(Ordering::SeqCst), 4);
        // После всех 4 commit-ов pending присутствует (последний выжил).
        assert!(owner.has_pending());
        let handle = owner.handle();
        assert!(handle.flush_pending());
        let active_bbox = handle.active_tree().unwrap().layer(0).unwrap().bbox();
        // bbox принадлежит одному из commit-ов (валидные значения 10..14).
        assert!(active_bbox.width >= 10.0 && active_bbox.width <= 13.0);
    }

    // --- CompositorThread: реальный OS-поток ---

    #[test]
    fn compositor_thread_flushes_pending_asynchronously() {
        use std::time::{Duration, Instant};

        let mut owner = ThreadedCompositor::new();
        let handle = owner.handle();
        let ct = CompositorThread::spawn(handle);

        let bbox = Rect::new(0.0, 0.0, 256.0, 128.0);
        owner.commit(
            Arc::new(PropertyTrees::empty()),
            Arc::new(BasicLayerTree::single_layer(bbox, sample_commands())),
        );

        // Vsync wakeup: поток должен flush-нуть значительно быстрее 200 мс.
        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            if owner.active_tree().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "compositor thread не flush-нул за 200 мс");
            thread::sleep(Duration::from_millis(1));
        }

        let active = owner.active_tree().unwrap();
        assert_eq!(active.layer(0).unwrap().bbox(), bbox);
        ct.shutdown();
    }

    #[test]
    fn compositor_thread_wakes_on_commit_faster_than_full_frame() {
        // commit() вызывает notify() — поток должен проснуться << TARGET_FRAME_DURATION
        use std::time::{Duration, Instant};

        let mut owner = ThreadedCompositor::new();
        let handle = owner.handle();
        let ct = CompositorThread::spawn(handle);

        // Даём потоку уйти в ожидание на condvar.
        thread::sleep(Duration::from_millis(5));

        let bbox = Rect::new(0.0, 0.0, 64.0, 64.0);
        let t0 = Instant::now();
        owner.commit(
            Arc::new(PropertyTrees::empty()),
            Arc::new(BasicLayerTree::single_layer(bbox, Vec::new())),
        );

        // Ждём flush; при condvar-wakeup должно уложиться в 50 мс
        // (TARGET_FRAME_DURATION ~16.67 мс + планировщик ОС).
        let deadline = t0 + Duration::from_millis(50);
        loop {
            if owner.active_tree().is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "vsync wakeup не сработал за 50 мс"
            );
            thread::sleep(Duration::from_millis(1));
        }

        ct.shutdown();
    }

    #[test]
    fn compositor_thread_shutdown_is_clean() {
        let owner = ThreadedCompositor::new();
        let handle = owner.handle();
        let ct = CompositorThread::spawn(handle);
        // shutdown() должен вернуться без паники или дедлока.
        ct.shutdown();
    }

    // --- VsyncNotifier ---

    #[test]
    fn vsync_notifier_wait_returns_after_notify() {
        use std::time::{Duration, Instant};

        let notifier = Arc::new(VsyncNotifier::new());
        let n2 = Arc::clone(&notifier);

        // Поток ждёт на notifier с большим timeout-ом.
        let t = thread::spawn(move || {
            let t0 = Instant::now();
            n2.wait_for_next_tick(Duration::from_secs(10));
            t0.elapsed()
        });

        // Даём потоку уйти в ожидание.
        thread::sleep(Duration::from_millis(5));
        notifier.notify();

        let elapsed = t.join().unwrap();
        // Поток должен был проснуться << 1 с (не весь timeout).
        assert!(
            elapsed < Duration::from_millis(200),
            "notify не разбудил поток вовремя: {elapsed:?}"
        );
    }

    #[test]
    fn vsync_notifier_dirty_flag_prevents_lost_wakeup() {
        // notify() до wait_for_next_tick() — dirty=true, wait возвращает немедленно.
        use std::time::{Duration, Instant};

        let notifier = VsyncNotifier::new();
        notifier.notify();

        let t0 = Instant::now();
        notifier.wait_for_next_tick(Duration::from_secs(10));
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "pre-notify должен давать немедленный возврат, но прошло {elapsed:?}"
        );
    }
}
