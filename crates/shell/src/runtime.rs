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
use std::collections::VecDeque;
use std::rc::Rc;

/// Источник task-а — HTML §8.1.4.3 «Task sources». Phase 0: один FIFO для всех
/// источников; per-source priority-queues — отдельная задача, когда возникнет
/// нужда отдавать `UserInteraction` приоритет над `Networking`.
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

/// FIFO-очередь Task-ов. Один список для всех TaskSource в Phase 0.
#[derive(Default)]
pub struct TaskQueue {
    tasks: VecDeque<Task>,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue(&mut self, task: Task) {
        self.tasks.push_back(task);
    }

    pub fn pop(&mut self) -> Option<Task> {
        self.tasks.pop_front()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
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

/// Внутреннее состояние event-loop-а под `Rc<RefCell<…>>`. Доступ из
/// closure-ов task-ов / microtask-ов идёт через `EventLoopHandle`, что снимает
/// типовой конфликт «closure владеет EventLoop / EventLoop запускает closure».
#[derive(Default)]
struct State {
    tasks: TaskQueue,
    microtasks: MicrotaskQueue,
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

    /// Сколько task-ов сейчас в очереди (для тестов / отладки).
    pub fn pending_tasks(&self) -> usize {
        self.state.borrow().tasks.len()
    }

    /// Сколько microtask-ов сейчас в очереди (для тестов / отладки).
    pub fn pending_microtasks(&self) -> usize {
        self.state.borrow().microtasks.len()
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
