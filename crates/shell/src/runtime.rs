// Public API runtime-а пока используется только из тестов; реальная
// интеграция в Lumen-app (winit-loop) — следующая задача. До неё все
// handle-методы выглядят dead-code-ом для non-test сборки.
#![allow(dead_code)]

//! HTML event loop runtime: task queues, microtask checkpoint, requestAnimationFrame,
//! observer registries.
//!
//! Реализует контракт **shell-а** по HTML Living Standard §8.1.4 «Event loops»:
//! - выбор одной task за step, microtask checkpoint после неё;
//! - drain-all microtask семантика (вновь поставленные microtask-и того же
//!   checkpoint выполняются в нём же — FIFO);
//! - rendering opportunity stage, на котором запускаются rAF-callback-и и
//!   delivery-стадия наблюдателей (Resize/Intersection/Mutation).
//!
//! JS engine **не** требуется: callback — это `Box<dyn FnOnce>` (или `Rc<dyn Fn>`
//! для наблюдателей, потому что они могут срабатывать многократно). Когда подключим
//! QuickJS, JS-function будет оборачиваться в Rust-closure и кидаться в эту же
//! очередь. Так Web Animations / Service Worker / DOM mutation events найдут
//! готовую точку диспатча, не дожидаясь JS engine.
//!
//! Threading: runtime — single-threaded (`Rc<RefCell<…>>`), как и сам HTML event
//! loop. Cross-thread обмен — через channels на shell-уровне (winit user-events
//! и т.п.), а не внутри runtime.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

/// Источник task-а — HTML §8.1.4.3 «Task sources». Каждому источнику —
/// своя FIFO-очередь; `TaskQueue::pop` обходит очереди в порядке
/// `PRIORITY_ORDER`, выбирая первую непустую.
///
/// Варианты намеренно перечислены полностью — это поверхность для будущей
/// классификации task-ов (network → networking, setTimeout → timer, и т. д.),
/// поэтому `dead_code` пока разрешён.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskSource {
    DomManipulation,
    UserInteraction,
    Networking,
    HistoryTraversal,
    IdleTask,
    Rendering,
    Timer,
}

/// Сколько различных task source-ов в [`TaskSource`]. Используется как
/// размер массива per-source очередей в [`TaskQueue`].
const TASK_SOURCE_COUNT: usize = 7;

impl TaskSource {
    /// Стабильный индекс источника в массиве per-source очередей.
    /// Значение не пересекается с приоритетом — приоритет задан отдельно
    /// в `PRIORITY_ORDER`, чтобы можно было менять без правки storage layout.
    const fn as_index(self) -> usize {
        match self {
            TaskSource::DomManipulation => 0,
            TaskSource::UserInteraction => 1,
            TaskSource::Networking => 2,
            TaskSource::HistoryTraversal => 3,
            TaskSource::IdleTask => 4,
            TaskSource::Rendering => 5,
            TaskSource::Timer => 6,
        }
    }

    /// Порядок выбора task-а: первая запись — highest priority.
    ///
    /// HTML §8.1.4.2 оставляет точный порядок на усмотрение UA. Наш порядок:
    /// `UserInteraction` > `DomManipulation` > `HistoryTraversal` >
    /// `Networking` > `Timer` > `Rendering` > `IdleTask`. Соответствует
    /// подходу Chromium scheduler — input важнее всего, idle — в самом конце.
    /// `HistoryTraversal` выше `Networking` потому, что back/forward — это
    /// прямое действие пользователя.
    pub const PRIORITY_ORDER: [TaskSource; TASK_SOURCE_COUNT] = [
        TaskSource::UserInteraction,
        TaskSource::DomManipulation,
        TaskSource::HistoryTraversal,
        TaskSource::Networking,
        TaskSource::Timer,
        TaskSource::Rendering,
        TaskSource::IdleTask,
    ];
}

/// Task — отложенное действие, выполняемое за пределами текущего call-stack-а.
/// `FnOnce`, потому что task выполняется ровно один раз; для повторяющегося
/// поведения caller сам перепланирует следующую task из своего closure.
pub struct Task {
    source: TaskSource,
    closure: Box<dyn FnOnce()>,
}

impl Task {
    pub fn new<F: FnOnce() + 'static>(source: TaskSource, closure: F) -> Self {
        Self {
            source,
            closure: Box::new(closure),
        }
    }

    pub fn source(&self) -> TaskSource {
        self.source
    }

    pub fn run(self) {
        (self.closure)();
    }
}

/// Per-source очереди task-ов. Каждый `TaskSource` — отдельная FIFO,
/// внутри источника порядок strict «кто раньше пришёл, тот раньше идёт».
/// `pop` обходит источники в `TaskSource::PRIORITY_ORDER` и возвращает
/// первую найденную task — этим достигается приоритезация
/// (UserInteraction опережает Networking, даже если был поставлен позже).
///
/// Хранение — массив фиксированной длины `TASK_SOURCE_COUNT`, индекс через
/// `TaskSource::as_index`. Это даёт O(1) на queue и O(K) на pop, где
/// K = число источников (7) — на практике не отличается от константы.
pub struct TaskQueue {
    queues: [VecDeque<Task>; TASK_SOURCE_COUNT],
    /// Суммарное число task-ов во всех очередях. Кэшируем, чтобы
    /// `len` / `is_empty` оставались O(1) без пересчёта через массив.
    total: usize,
}

impl Default for TaskQueue {
    fn default() -> Self {
        // `[VecDeque::new(); N]` нельзя — `VecDeque` не Copy. `from_fn`
        // вызывает конструктор для каждого слота независимо.
        Self {
            queues: std::array::from_fn(|_| VecDeque::new()),
            total: 0,
        }
    }
}

impl TaskQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue(&mut self, task: Task) {
        let idx = task.source.as_index();
        self.queues[idx].push_back(task);
        self.total += 1;
    }

    /// Достать task с highest-priority непустой очереди (по
    /// `TaskSource::PRIORITY_ORDER`). Внутри одного источника — FIFO.
    pub fn pop(&mut self) -> Option<Task> {
        for src in TaskSource::PRIORITY_ORDER {
            let q = &mut self.queues[src.as_index()];
            if let Some(t) = q.pop_front() {
                self.total -= 1;
                return Some(t);
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// Длина очереди конкретного источника — для тестов и метрик
    /// (например, размер «idle backlog» на vsync).
    pub fn len_of(&self, source: TaskSource) -> usize {
        self.queues[source.as_index()].len()
    }
}

/// Microtask — действие, выполняемое в microtask checkpoint после каждой
/// task / rendering step. Семантика drain-all: вновь поставленный microtask
/// внутри checkpoint выполняется в том же checkpoint (а не в следующем),
/// поэтому checkpoint завершается только когда очередь пуста.
pub struct Microtask {
    closure: Box<dyn FnOnce()>,
}

impl Microtask {
    pub fn new<F: FnOnce() + 'static>(closure: F) -> Self {
        Self {
            closure: Box::new(closure),
        }
    }

    pub fn run(self) {
        (self.closure)();
    }
}

#[derive(Default)]
pub struct MicrotaskQueue {
    queue: VecDeque<Microtask>,
}

impl MicrotaskQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue(&mut self, mt: Microtask) {
        self.queue.push_back(mt);
    }

    pub fn pop(&mut self) -> Option<Microtask> {
        self.queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

/// Уникальный идентификатор rAF-callback-а, возвращается `request_animation_frame`.
/// Передаётся в `cancel_animation_frame`, чтобы отменить вызов до того, как
/// rendering step его исполнит.
pub type AnimationFrameHandle = u32;

/// Тип наблюдателя — определяет, в какой стадии rendering steps его callback
/// будет вызван (HTML §8.1.5.1, шаги 13–17). Phase 0 не различает реальный
/// триггер «наблюдаемое изменилось»: `deliver_observer_records` дёргает все
/// активные callback-и указанного типа.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObserverKind {
    Resize,
    Intersection,
    Mutation,
}

/// Уникальный handle наблюдателя. `disconnect_observer` снимает регистрацию.
pub type ObserverHandle = u32;

/// Регистрация наблюдателя. `Rc<dyn Fn()>` (не `FnOnce`), потому что один и
/// тот же observer срабатывает многократно по мере изменений; `Rc` нужен,
/// чтобы можно было сделать дешёвый snapshot активного списка перед delivery
/// (callback внутри delivery может disconnect-ить себя или соседнего observer-а
/// — snapshot защищает текущую итерацию).
struct ObserverEntry {
    handle: ObserverHandle,
    kind: ObserverKind,
    callback: Rc<dyn Fn()>,
}

/// rAF-callback с handle-ом. `FnOnce`, потому что rAF по спецификации
/// одноразовый — для повторяющейся анимации callback сам перепланирует следующий.
struct AnimationFrameCallback {
    handle: AnimationFrameHandle,
    closure: Box<dyn FnOnce(f64)>,
}

/// Внутреннее состояние event-loop-а под `Rc<RefCell<…>>`. Доступ из
/// closure-ов task-ов / microtask-ов идёт через `EventLoopHandle`, что снимает
/// типовой конфликт «closure владеет EventLoop / EventLoop запускает closure».
#[derive(Default)]
struct State {
    tasks: TaskQueue,
    microtasks: MicrotaskQueue,
    raf: Vec<AnimationFrameCallback>,
    next_raf_handle: AnimationFrameHandle,
    /// Handle-ы rAF, отменённые до выполнения. Подчищаются после rendering step.
    cancelled_raf: HashSet<AnimationFrameHandle>,
    observers: Vec<ObserverEntry>,
    next_observer_handle: ObserverHandle,
}

/// Результат одной итерации `step()`: запустилась ли task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResult {
    /// Запустили одну task и выполнили microtask checkpoint.
    Ran,
    /// Очередь tasks пуста; microtask-чекпоинт всё равно прогнали
    /// (вдруг кто-то закинул microtask напрямую).
    Idle,
}

/// HTML event loop. Реализует §8.1.4.2 «Processing model» в минимально полезном
/// виде: одна task → microtask checkpoint.
pub struct EventLoop {
    state: Rc<RefCell<State>>,
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLoop {
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(State::default())),
        }
    }

    /// Дешёвая клон-копия handle-а для постановки task-ов извне и изнутри
    /// closure-ов. `Rc::clone` — один указатель.
    pub fn handle(&self) -> EventLoopHandle {
        EventLoopHandle {
            state: Rc::clone(&self.state),
        }
    }

    /// Один step event-loop-а:
    /// 1. Вытащить одну task из очереди (если есть) и выполнить.
    /// 2. Microtask checkpoint — drain-all.
    ///
    /// Возвращает `StepResult::Ran`, если task была; `Idle` иначе. В обоих
    /// случаях microtask checkpoint выполняется — это упрощает caller-у
    /// сценарий «закинул microtask и вызвал step, чтобы он отработал».
    pub fn step(&self) -> StepResult {
        let task = self.state.borrow_mut().tasks.pop();
        let ran = if let Some(t) = task {
            t.run();
            StepResult::Ran
        } else {
            StepResult::Idle
        };
        self.perform_microtask_checkpoint();
        ran
    }

    /// HTML §8.1.4.4 «Microtask checkpoint». Drain-all: вновь поставленный
    /// microtask внутри checkpoint выполняется здесь же, цикл идёт до пустой
    /// очереди.
    pub fn perform_microtask_checkpoint(&self) {
        loop {
            let mt = self.state.borrow_mut().microtasks.pop();
            let Some(mt) = mt else {
                break;
            };
            mt.run();
        }
    }

    /// Rendering opportunity stage — HTML §8.1.5.1 «Run the animation frame
    /// callbacks». Выполняет snapshot текущего списка rAF-callback-ов с
    /// `timestamp_ms`, после чего гонит microtask checkpoint. Новые rAF,
    /// зарегистрированные внутри callback-а, попадают в **следующий** frame —
    /// snapshot берётся через `mem::take`, а новые регистрации копятся в чистом
    /// `state.raf`.
    ///
    /// Cancelled handles (`cancel_animation_frame` до начала этого rendering
    /// step) пропускаются и удаляются из `cancelled_raf`. Cancel внутри текущего
    /// step-а (для callback-ов того же frame, ещё не выполненных) учитывается:
    /// проверка `cancelled_raf.contains` происходит перед каждым вызовом
    /// callback-а отдельно.
    pub fn run_rendering_step(&self, timestamp_ms: f64) {
        let frame: Vec<AnimationFrameCallback> = {
            let mut state = self.state.borrow_mut();
            std::mem::take(&mut state.raf)
        };
        for cb in frame {
            let cancelled = self.state.borrow_mut().cancelled_raf.remove(&cb.handle);
            if cancelled {
                continue;
            }
            (cb.closure)(timestamp_ms);
        }
        // По спеке §8.1.5.1 после frame-callback-ов — microtask checkpoint.
        self.perform_microtask_checkpoint();
    }

    /// Сколько task-ов сейчас в очереди (для тестов / отладки).
    pub fn pending_tasks(&self) -> usize {
        self.state.borrow().tasks.len()
    }

    /// Сколько microtask-ов сейчас в очереди (для тестов / отладки).
    pub fn pending_microtasks(&self) -> usize {
        self.state.borrow().microtasks.len()
    }

    /// Сколько rAF-callback-ов сейчас ждёт следующего rendering step
    /// (для тестов / отладки).
    pub fn pending_animation_frames(&self) -> usize {
        self.state.borrow().raf.len()
    }

    /// Сколько активных наблюдателей указанного типа (для тестов / отладки).
    pub fn active_observers(&self, kind: ObserverKind) -> usize {
        self.state
            .borrow()
            .observers
            .iter()
            .filter(|e| e.kind == kind)
            .count()
    }

    /// Доставить records всем активным наблюдателям указанного типа.
    /// Phase 0: callback вызывается без аргумента-records, реальная агрегация
    /// «что изменилось с прошлого delivery» подключится, когда DOM/layout
    /// начнут эмитить mutation / resize / intersection events.
    ///
    /// Snapshot-pattern: список callback-ов копируется (через `Rc::clone`)
    /// до начала итерации, чтобы callback, который disconnect-ит себя или
    /// соседнего observer-а, не сломал текущий delivery. Изменения видны на
    /// следующем delivery.
    pub fn deliver_observer_records(&self, kind: ObserverKind) {
        let callbacks: Vec<Rc<dyn Fn()>> = self
            .state
            .borrow()
            .observers
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| Rc::clone(&e.callback))
            .collect();
        for cb in callbacks {
            cb();
        }
    }
}

/// Дёшево клонируемая ссылка на event loop. Closure-ы task-ов / microtask-ов
/// клонируют `EventLoopHandle` к себе и через него планируют новые задания.
#[derive(Clone)]
pub struct EventLoopHandle {
    state: Rc<RefCell<State>>,
}

impl EventLoopHandle {
    pub fn queue_task<F: FnOnce() + 'static>(&self, source: TaskSource, closure: F) {
        self.state
            .borrow_mut()
            .tasks
            .queue(Task::new(source, closure));
    }

    pub fn queue_microtask<F: FnOnce() + 'static>(&self, closure: F) {
        self.state
            .borrow_mut()
            .microtasks
            .queue(Microtask::new(closure));
    }

    /// Зарегистрировать rAF-callback. Будет вызван на ближайшем
    /// `run_rendering_step` с `timestamp_ms` этого step-а.
    pub fn request_animation_frame<F: FnOnce(f64) + 'static>(
        &self,
        closure: F,
    ) -> AnimationFrameHandle {
        let mut state = self.state.borrow_mut();
        // Phase 0: u32 handle, монотонный счётчик. Wrap-around через >4B вызовов
        // не учитываем — Phase 1 при необходимости заменим на (slot, gen)
        // пару, как делают Chromium и Firefox для defense-in-depth от ABA.
        let handle = state.next_raf_handle.wrapping_add(1);
        state.next_raf_handle = handle;
        state.raf.push(AnimationFrameCallback {
            handle,
            closure: Box::new(closure),
        });
        handle
    }

    /// Отменить rAF до выполнения. Если handle уже выполнен или неизвестен —
    /// no-op (CSS OM View §6 `cancelAnimationFrame` всегда non-throwing).
    pub fn cancel_animation_frame(&self, handle: AnimationFrameHandle) {
        self.state.borrow_mut().cancelled_raf.insert(handle);
    }

    /// Зарегистрировать observer выбранного типа. Callback-ы вызываются при
    /// `deliver_observer_records(kind)`. Возвращает handle для disconnect.
    pub fn register_observer<F: Fn() + 'static>(
        &self,
        kind: ObserverKind,
        callback: F,
    ) -> ObserverHandle {
        let mut state = self.state.borrow_mut();
        let handle = state.next_observer_handle.wrapping_add(1);
        state.next_observer_handle = handle;
        state.observers.push(ObserverEntry {
            handle,
            kind,
            callback: Rc::new(callback),
        });
        handle
    }

    /// Снять регистрацию наблюдателя. Неизвестный handle — no-op.
    pub fn disconnect_observer(&self, handle: ObserverHandle) {
        self.state
            .borrow_mut()
            .observers
            .retain(|e| e.handle != handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_log() -> Rc<RefCell<Vec<&'static str>>> {
        Rc::new(RefCell::new(Vec::new()))
    }

    #[test]
    fn task_queue_is_fifo() {
        let mut q = TaskQueue::new();
        let log = shared_log();
        let l1 = Rc::clone(&log);
        let l2 = Rc::clone(&log);
        q.queue(Task::new(TaskSource::Timer, move || l1.borrow_mut().push("a")));
        q.queue(Task::new(TaskSource::Timer, move || l2.borrow_mut().push("b")));
        assert_eq!(q.len(), 2);
        q.pop().unwrap().run();
        q.pop().unwrap().run();
        assert_eq!(*log.borrow(), vec!["a", "b"]);
        assert!(q.is_empty());
    }

    #[test]
    fn task_remembers_source() {
        let t = Task::new(TaskSource::UserInteraction, || {});
        assert_eq!(t.source(), TaskSource::UserInteraction);
    }

    #[test]
    fn microtask_queue_is_fifo_drain() {
        let mut q = MicrotaskQueue::new();
        let log = shared_log();
        let l1 = Rc::clone(&log);
        let l2 = Rc::clone(&log);
        q.queue(Microtask::new(move || l1.borrow_mut().push("x")));
        q.queue(Microtask::new(move || l2.borrow_mut().push("y")));
        q.pop().unwrap().run();
        q.pop().unwrap().run();
        assert_eq!(*log.borrow(), vec!["x", "y"]);
    }

    #[test]
    fn step_runs_one_task_per_call() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();
        let l1 = Rc::clone(&log);
        let l2 = Rc::clone(&log);
        h.queue_task(TaskSource::Timer, move || l1.borrow_mut().push("first"));
        h.queue_task(TaskSource::Timer, move || l2.borrow_mut().push("second"));
        assert_eq!(el.step(), StepResult::Ran);
        assert_eq!(*log.borrow(), vec!["first"]);
        assert_eq!(el.step(), StepResult::Ran);
        assert_eq!(*log.borrow(), vec!["first", "second"]);
        assert_eq!(el.step(), StepResult::Idle);
    }

    #[test]
    fn microtask_checkpoint_runs_after_task() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();

        // task пишет "t", потом планирует microtask, который пишет "mt".
        // Контракт: "t" — раньше "mt" (microtask checkpoint строго после task).
        let log_t = Rc::clone(&log);
        let log_mt = Rc::clone(&log);
        let h_inner = h.clone();
        h.queue_task(TaskSource::Timer, move || {
            log_t.borrow_mut().push("t");
            h_inner.queue_microtask(move || log_mt.borrow_mut().push("mt"));
        });

        el.step();
        assert_eq!(*log.borrow(), vec!["t", "mt"]);
    }

    #[test]
    fn microtask_can_schedule_more_microtasks_in_same_checkpoint() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();

        let log_a = Rc::clone(&log);
        let log_b = Rc::clone(&log);
        let h_inner = h.clone();
        h.queue_microtask(move || {
            log_a.borrow_mut().push("a");
            h_inner.queue_microtask(move || log_b.borrow_mut().push("b"));
        });
        // step ничего не делает в tasks (Idle), но microtask checkpoint
        // всё равно дренит — а это и есть проверка drain-all.
        assert_eq!(el.step(), StepResult::Idle);
        assert_eq!(*log.borrow(), vec!["a", "b"]);
    }

    #[test]
    fn task_can_schedule_next_task() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();

        let log_first = Rc::clone(&log);
        let log_second = Rc::clone(&log);
        let h_inner = h.clone();
        h.queue_task(TaskSource::Timer, move || {
            log_first.borrow_mut().push("first");
            h_inner.queue_task(TaskSource::Timer, move || {
                log_second.borrow_mut().push("second");
            });
        });

        // Первый step: запускает "first" + планирует "second", microtask пуст.
        // "second" в очереди — но НЕ в этом же step (одна task за step).
        el.step();
        assert_eq!(*log.borrow(), vec!["first"]);
        assert_eq!(el.pending_tasks(), 1);

        // Второй step: "second".
        el.step();
        assert_eq!(*log.borrow(), vec!["first", "second"]);
    }

    #[test]
    fn idle_step_still_drains_microtasks() {
        // Это пограничный случай: tasks пуст, но microtask закинули напрямую.
        // Тогда step возвращает Idle, но checkpoint всё равно прошёл.
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();
        let log_mt = Rc::clone(&log);
        h.queue_microtask(move || log_mt.borrow_mut().push("mt"));
        assert_eq!(el.step(), StepResult::Idle);
        assert_eq!(*log.borrow(), vec!["mt"]);
    }

    #[test]
    fn microtask_queue_len_and_is_empty() {
        let mut q = MicrotaskQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        q.queue(Microtask::new(|| {}));
        q.queue(Microtask::new(|| {}));
        assert!(!q.is_empty());
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn raf_runs_in_registration_order_with_timestamp() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = Rc::new(RefCell::new(Vec::<(String, f64)>::new()));

        let l1 = Rc::clone(&log);
        let l2 = Rc::clone(&log);
        h.request_animation_frame(move |t| l1.borrow_mut().push(("first".into(), t)));
        h.request_animation_frame(move |t| l2.borrow_mut().push(("second".into(), t)));
        assert_eq!(el.pending_animation_frames(), 2);

        el.run_rendering_step(16.7);
        let log_ = log.borrow();
        assert_eq!(log_.len(), 2);
        assert_eq!(log_[0].0, "first");
        assert_eq!(log_[0].1, 16.7);
        assert_eq!(log_[1].0, "second");
        assert_eq!(log_[1].1, 16.7);
        assert_eq!(el.pending_animation_frames(), 0);
    }

    #[test]
    fn cancel_animation_frame_skips_callback() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();
        let l1 = Rc::clone(&log);
        let l2 = Rc::clone(&log);

        let h1 = h.request_animation_frame(move |_| l1.borrow_mut().push("kept"));
        let h2 = h.request_animation_frame(move |_| l2.borrow_mut().push("cancelled"));
        h.cancel_animation_frame(h2);

        el.run_rendering_step(0.0);
        assert_eq!(*log.borrow(), vec!["kept"]);

        // Сам по себе cancel неизвестного handle не паникует и не делает ничего.
        h.cancel_animation_frame(h1);
        h.cancel_animation_frame(9999);
    }

    #[test]
    fn raf_scheduled_inside_callback_goes_to_next_frame() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();

        let l_outer = Rc::clone(&log);
        let l_inner = Rc::clone(&log);
        let h_inner = h.clone();
        h.request_animation_frame(move |_| {
            l_outer.borrow_mut().push("outer");
            h_inner.request_animation_frame(move |_| l_inner.borrow_mut().push("inner"));
        });

        // Frame 1: outer выполнен, inner запланирован но в следующий frame.
        el.run_rendering_step(0.0);
        assert_eq!(*log.borrow(), vec!["outer"]);
        assert_eq!(el.pending_animation_frames(), 1);

        // Frame 2: inner.
        el.run_rendering_step(16.7);
        assert_eq!(*log.borrow(), vec!["outer", "inner"]);
        assert_eq!(el.pending_animation_frames(), 0);
    }

    #[test]
    fn rendering_step_runs_microtask_checkpoint() {
        // rAF callback планирует microtask — он должен выполниться
        // в этом же rendering step, по спеке §8.1.5.1.
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();

        let l_raf = Rc::clone(&log);
        let l_mt = Rc::clone(&log);
        let h_inner = h.clone();
        h.request_animation_frame(move |_| {
            l_raf.borrow_mut().push("raf");
            h_inner.queue_microtask(move || l_mt.borrow_mut().push("mt"));
        });

        el.run_rendering_step(0.0);
        assert_eq!(*log.borrow(), vec!["raf", "mt"]);
    }

    #[test]
    fn empty_rendering_step_is_noop() {
        let el = EventLoop::new();
        // run на пустом rAF-списке — должно отработать без паники.
        el.run_rendering_step(42.0);
        assert_eq!(el.pending_animation_frames(), 0);
    }

    #[test]
    fn observers_register_and_disconnect() {
        let el = EventLoop::new();
        let h = el.handle();
        let h1 = h.register_observer(ObserverKind::Resize, || {});
        let h2 = h.register_observer(ObserverKind::Resize, || {});
        let h3 = h.register_observer(ObserverKind::Mutation, || {});
        assert_eq!(el.active_observers(ObserverKind::Resize), 2);
        assert_eq!(el.active_observers(ObserverKind::Mutation), 1);
        assert_eq!(el.active_observers(ObserverKind::Intersection), 0);

        h.disconnect_observer(h1);
        assert_eq!(el.active_observers(ObserverKind::Resize), 1);
        h.disconnect_observer(h2);
        assert_eq!(el.active_observers(ObserverKind::Resize), 0);

        // Disconnect неизвестного handle — no-op.
        h.disconnect_observer(9999);
        h.disconnect_observer(h3);
        assert_eq!(el.active_observers(ObserverKind::Mutation), 0);
    }

    #[test]
    fn deliver_observer_records_calls_only_matching_kind() {
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();

        let l_resize = Rc::clone(&log);
        let l_intersect = Rc::clone(&log);
        let l_mutation = Rc::clone(&log);
        h.register_observer(ObserverKind::Resize, move || {
            l_resize.borrow_mut().push("resize");
        });
        h.register_observer(ObserverKind::Intersection, move || {
            l_intersect.borrow_mut().push("intersection");
        });
        h.register_observer(ObserverKind::Mutation, move || {
            l_mutation.borrow_mut().push("mutation");
        });

        el.deliver_observer_records(ObserverKind::Resize);
        assert_eq!(*log.borrow(), vec!["resize"]);

        el.deliver_observer_records(ObserverKind::Mutation);
        assert_eq!(*log.borrow(), vec!["resize", "mutation"]);
    }

    #[test]
    fn observer_callback_called_each_delivery() {
        // Observer ≠ rAF: один и тот же callback срабатывает многократно.
        let el = EventLoop::new();
        let h = el.handle();
        let counter = Rc::new(RefCell::new(0_usize));
        let c = Rc::clone(&counter);
        h.register_observer(ObserverKind::Resize, move || {
            *c.borrow_mut() += 1;
        });

        el.deliver_observer_records(ObserverKind::Resize);
        el.deliver_observer_records(ObserverKind::Resize);
        el.deliver_observer_records(ObserverKind::Resize);
        assert_eq!(*counter.borrow(), 3);
    }

    #[test]
    fn observer_can_disconnect_during_delivery() {
        // Snapshot-pattern: callback дёргает disconnect — текущая итерация
        // продолжается со старым списком, но следующий delivery видит
        // обновлённый.
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();

        let suicide_handle = Rc::new(RefCell::new(None::<ObserverHandle>));
        let sh_clone = Rc::clone(&suicide_handle);
        let h_inner = h.clone();
        let l1 = Rc::clone(&log);
        let id = h.register_observer(ObserverKind::Resize, move || {
            l1.borrow_mut().push("suicide");
            if let Some(handle) = *sh_clone.borrow() {
                h_inner.disconnect_observer(handle);
            }
        });
        *suicide_handle.borrow_mut() = Some(id);

        let l2 = Rc::clone(&log);
        h.register_observer(ObserverKind::Resize, move || {
            l2.borrow_mut().push("other");
        });

        // Первый delivery: оба сработали; "suicide" disconnect-ит сам себя.
        el.deliver_observer_records(ObserverKind::Resize);
        assert_eq!(*log.borrow(), vec!["suicide", "other"]);

        // Второй delivery: только "other".
        el.deliver_observer_records(ObserverKind::Resize);
        assert_eq!(*log.borrow(), vec!["suicide", "other", "other"]);
    }

    #[test]
    fn user_interaction_beats_networking_even_if_queued_later() {
        // Cross-source priority — главный контракт задачи.
        let mut q = TaskQueue::new();
        let log = shared_log();
        let l_net = Rc::clone(&log);
        let l_ui = Rc::clone(&log);
        q.queue(Task::new(TaskSource::Networking, move || {
            l_net.borrow_mut().push("net")
        }));
        q.queue(Task::new(TaskSource::UserInteraction, move || {
            l_ui.borrow_mut().push("ui")
        }));
        q.pop().unwrap().run();
        q.pop().unwrap().run();
        // UI первой, хотя пришла позже сетевой.
        assert_eq!(*log.borrow(), vec!["ui", "net"]);
    }

    #[test]
    fn priority_order_consumes_every_source_exactly_once() {
        // Закидываем по одной task в каждый source в обратном priority-порядке;
        // pop должен выдать ровно PRIORITY_ORDER.
        let mut q = TaskQueue::new();
        let log: Rc<RefCell<Vec<TaskSource>>> = Rc::new(RefCell::new(Vec::new()));
        let mut sources: Vec<TaskSource> = TaskSource::PRIORITY_ORDER.into();
        sources.reverse();
        for src in sources {
            let l = Rc::clone(&log);
            q.queue(Task::new(src, move || l.borrow_mut().push(src)));
        }
        while let Some(t) = q.pop() {
            t.run();
        }
        assert_eq!(*log.borrow(), TaskSource::PRIORITY_ORDER.to_vec());
        assert!(q.is_empty());
    }

    #[test]
    fn intra_source_remains_fifo_under_priority() {
        // Два task-а одного источника — порядок строго FIFO; третий из
        // более приоритетного источника лезет вперёд них.
        let mut q = TaskQueue::new();
        let log = shared_log();
        let l1 = Rc::clone(&log);
        let l2 = Rc::clone(&log);
        let l_ui = Rc::clone(&log);
        q.queue(Task::new(TaskSource::Timer, move || l1.borrow_mut().push("t1")));
        q.queue(Task::new(TaskSource::Timer, move || l2.borrow_mut().push("t2")));
        q.queue(Task::new(TaskSource::UserInteraction, move || {
            l_ui.borrow_mut().push("ui")
        }));
        q.pop().unwrap().run();
        q.pop().unwrap().run();
        q.pop().unwrap().run();
        assert_eq!(*log.borrow(), vec!["ui", "t1", "t2"]);
    }

    #[test]
    fn len_and_is_empty_track_total_across_sources() {
        let mut q = TaskQueue::new();
        assert!(q.is_empty());
        q.queue(Task::new(TaskSource::Networking, || {}));
        q.queue(Task::new(TaskSource::Timer, || {}));
        q.queue(Task::new(TaskSource::Networking, || {}));
        assert_eq!(q.len(), 3);
        assert_eq!(q.len_of(TaskSource::Networking), 2);
        assert_eq!(q.len_of(TaskSource::Timer), 1);
        assert_eq!(q.len_of(TaskSource::UserInteraction), 0);
        // PRIORITY_ORDER: Timer выше IdleTask — но не выше Networking.
        // Pop отдаст Networking (раньше Timer в priority chain? — нет,
        // Timer выше Networking? — нет, Networking приоритетнее Timer.
        // Проверим: первая попа выдаст Networking, потом Networking,
        // потом Timer.
        assert!(q.pop().is_some());
        assert_eq!(q.len(), 2);
        assert_eq!(q.len_of(TaskSource::Networking), 1);
        assert!(!q.is_empty());
    }

    #[test]
    fn event_loop_step_honours_priority_across_sources() {
        // Та же проверка, но через EventLoop::step — публичный API,
        // который реально использует winit-loop.
        let el = EventLoop::new();
        let h = el.handle();
        let log = shared_log();
        let l_idle = Rc::clone(&log);
        let l_net = Rc::clone(&log);
        let l_ui = Rc::clone(&log);
        h.queue_task(TaskSource::IdleTask, move || l_idle.borrow_mut().push("idle"));
        h.queue_task(TaskSource::Networking, move || l_net.borrow_mut().push("net"));
        h.queue_task(TaskSource::UserInteraction, move || {
            l_ui.borrow_mut().push("ui")
        });
        el.step();
        el.step();
        el.step();
        assert_eq!(*log.borrow(), vec!["ui", "net", "idle"]);
    }

    #[test]
    fn pending_counters_reflect_queue_state() {
        let el = EventLoop::new();
        let h = el.handle();
        assert_eq!(el.pending_tasks(), 0);
        assert_eq!(el.pending_microtasks(), 0);
        h.queue_task(TaskSource::Timer, || {});
        h.queue_microtask(|| {});
        h.queue_microtask(|| {});
        assert_eq!(el.pending_tasks(), 1);
        assert_eq!(el.pending_microtasks(), 2);
        // step запускает task + drain микротасков.
        el.step();
        assert_eq!(el.pending_tasks(), 0);
        assert_eq!(el.pending_microtasks(), 0);
    }
}
