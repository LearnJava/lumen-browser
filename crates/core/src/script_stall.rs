//! Сторож зависших JS-задач (COMPAT-HANG).
//!
//! Скрипт страницы исполняется на собственном потоке V8 и может не вернуть
//! управление никогда (`while(1){}`, 19-секундные задачи BUG-935). Раньше
//! оболочка этого не замечала: ни плашки, ни способа остановить скрипт.
//!
//! Модуль не знает про V8. Рантайм (`lumen-js`) заводит [`StallHandle`], отмечает
//! начало и конец каждой задачи ([`StallHandle::job_started`] /
//! [`StallHandle::job_finished`]) и отдаёт замыкание, прерывающее исполнение
//! ([`StallHandle::set_terminator`]). Единственный фоновый поток раз в
//! [`POLL_MS`] смотрит на занятые рантаймы; задача, идущая дольше порога
//! ([`threshold_ms`]), помечается «о ней сообщено» и будит оболочку
//! ([`set_wake`]). Оболочка читает состояние сама ([`StallHandle::stalled_ms`])
//! и по кнопкам плашки зовёт [`StallHandle::wait_more`] или
//! [`StallHandle::terminate`] — напрямую с UI-потока, не через JS-поток.
//!
//! Простой не стоит ничего: пока ни одна задача не идёт, сторож припаркован
//! (`thread::park`), а не опрашивает.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// Через сколько мс непрерывной работы задачи показывается плашка.
pub const DEFAULT_THRESHOLD_MS: u64 = 5_000;
/// После «Подождать» плашка возвращается, если задача идёт ещё столько мс.
pub const REARM_MS: u64 = 15_000;
/// Период опроса сторожа, пока хоть одна задача занята.
const POLL_MS: u64 = 250;
/// Через сколько мс работы задача считается «зависшей» для [`any_hung`]: UI-поток
/// перестаёт ждать ответов движкового потока, который стоит за этой задачей.
/// Раньше порога плашки — плашка ждёт терпеливого пользователя, а UI ждать не должен.
const HUNG_GATE_MS: u64 = 1_500;

/// Порог показа плашки; `LUMEN_SCRIPT_STALL_MS=<N>` переопределяет (тесты, автоматизация).
#[must_use]
pub fn threshold_ms() -> u64 {
    static T: OnceLock<u64> = OnceLock::new();
    *T.get_or_init(|| {
        std::env::var("LUMEN_SCRIPT_STALL_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(DEFAULT_THRESHOLD_MS)
    })
}

/// Миллисекунды от начала процесса, всегда `>= 1`: ноль значит «не занят».
fn now_ms() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    u64::try_from(EPOCH.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX - 1) + 1
}

type Terminator = Box<dyn Fn() + Send + Sync>;

struct Shared {
    /// Когда началась текущая задача; `0` — рантайм свободен.
    busy_since: AtomicU64,
    /// Когда пользователь нажал «Подождать»; `0` — не нажимал на этой задаче.
    quiet_since: AtomicU64,
    /// Сторож уже сообщил об этой задаче (и разбудил оболочку).
    reported: AtomicBool,
    /// Задача идёт дольше [`HUNG_GATE_MS`] (учтена в [`HUNG`]); снимает конец
    /// задачи, «Подождать» не трогает.
    hung: AtomicBool,
    /// Рантаймом владеет вкладка оболочки (иначе это загрузка или фрейм).
    claimed: AtomicBool,
    terminator: Mutex<Option<Terminator>>,
}

impl Shared {
    /// Сколько мс задача зависла сверх порога: `None`, если рантайм свободен
    /// или порог ещё не вышел.
    fn stalled_ms(&self, now: u64) -> Option<u64> {
        let busy = self.busy_since.load(Ordering::Acquire);
        if busy == 0 {
            return None;
        }
        let quiet = self.quiet_since.load(Ordering::Acquire);
        let (from, limit) = if quiet > busy { (quiet, REARM_MS) } else { (busy, threshold_ms()) };
        let running = now.saturating_sub(from);
        (running >= limit).then_some(now.saturating_sub(busy))
    }

    fn clear_hung(&self) {
        if self.hung.swap(false, Ordering::AcqRel) {
            HUNG.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// Отмечает задачу зависшей, если она идёт дольше [`HUNG_GATE_MS`].
    fn mark_hung(&self, now: u64) {
        let busy = self.busy_since.load(Ordering::Acquire);
        if busy != 0
            && now.saturating_sub(busy) >= HUNG_GATE_MS.min(threshold_ms())
            && !self.hung.swap(true, Ordering::AcqRel)
        {
            HUNG.fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// Сколько рантаймов занято сейчас; ноль — сторож паркуется.
static BUSY: AtomicUsize = AtomicUsize::new(0);
/// Сколько рантаймов сейчас «зависли» (см. [`Shared::hung`]).
static HUNG: AtomicUsize = AtomicUsize::new(0);
static REGISTRY: Mutex<Vec<Weak<Shared>>> = Mutex::new(Vec::new());
static WATCHDOG: OnceLock<std::thread::Thread> = OnceLock::new();
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Задаёт, как разбудить оболочку (её цикл событий спит на `Wait`). Первый вызов
/// побеждает; повторные игнорируются.
pub fn set_wake(f: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(f));
}

fn wake() {
    if let Some(f) = WAKE.get() {
        f();
    }
}

fn registry() -> std::sync::MutexGuard<'static, Vec<Weak<Shared>>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ручка одного JS-рантайма: пишет рантайм, читает и управляет оболочка.
#[derive(Clone)]
pub struct StallHandle(Arc<Shared>);

impl Default for StallHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl StallHandle {
    /// Заводит ручку и ставит её на учёт у сторожа (поток стартует лениво).
    #[must_use]
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            busy_since: AtomicU64::new(0),
            quiet_since: AtomicU64::new(0),
            reported: AtomicBool::new(false),
            hung: AtomicBool::new(false),
            claimed: AtomicBool::new(false),
            terminator: Mutex::new(None),
        });
        {
            let mut reg = registry();
            reg.retain(|w| w.strong_count() > 0);
            reg.push(Arc::downgrade(&shared));
        }
        ensure_watchdog();
        Self(shared)
    }

    /// Замыкание, прерывающее исполняемый сейчас скрипт (`TerminateExecution`).
    /// Должно быть безопасно звать с любого потока.
    pub fn set_terminator(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.0.terminator.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
    }

    /// Рантайм начал задачу (на JS-потоке).
    pub fn job_started(&self) {
        self.0.quiet_since.store(0, Ordering::Release);
        self.0.reported.store(false, Ordering::Release);
        self.0.clear_hung();
        if self.0.busy_since.swap(now_ms(), Ordering::AcqRel) == 0
            && BUSY.fetch_add(1, Ordering::AcqRel) == 0
            && let Some(w) = WATCHDOG.get()
        {
            w.unpark();
        }
    }

    /// Рантайм закончил задачу (на JS-потоке). Будит оболочку, если о задаче
    /// уже сообщали: плашка должна уйти.
    pub fn job_finished(&self) {
        if self.0.busy_since.swap(0, Ordering::AcqRel) != 0 {
            BUSY.fetch_sub(1, Ordering::AcqRel);
        }
        self.0.clear_hung();
        if self.0.reported.swap(false, Ordering::AcqRel) {
            wake();
        }
    }

    /// Сколько мс идёт зависшая задача (от её начала); `None` — всё в порядке
    /// либо порог ещё не вышел.
    #[must_use]
    pub fn stalled_ms(&self) -> Option<u64> {
        self.0.stalled_ms(now_ms())
    }

    /// «Подождать»: плашка скрывается и вернётся, если задача проработает ещё
    /// [`REARM_MS`].
    pub fn wait_more(&self) {
        self.0.quiet_since.store(now_ms(), Ordering::Release);
        self.0.reported.store(false, Ordering::Release);
    }

    /// «Остановить скрипт»: прервать исполнение. `false`, если рантайм свободен
    /// (прерывать нечего) или замыкание не задано.
    pub fn terminate(&self) -> bool {
        if self.0.busy_since.load(Ordering::Acquire) == 0 {
            return false;
        }
        let guard = self.0.terminator.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|f| f()).is_some()
    }

    /// Рантайм занят прямо сейчас (любой длительности).
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.0.busy_since.load(Ordering::Acquire) != 0
    }

    /// Вкладка оболочки взяла рантайм себе: он больше не «ничей».
    pub fn claim(&self) {
        self.0.claimed.store(true, Ordering::Release);
    }

    /// Та же ли это ручка (один рантайм).
    #[must_use]
    pub fn same_runtime(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Зависшая задача рантайма, которого ещё не взяла вкладка (идёт загрузка
/// страницы или работает фрейм). Самая долгая из таких.
#[must_use]
pub fn unclaimed_stalled() -> Option<StallHandle> {
    unclaimed_stalled_at(now_ms())
}

fn unclaimed_stalled_at(now: u64) -> Option<StallHandle> {
    let reg = registry();
    reg.iter()
        .filter_map(Weak::upgrade)
        .filter(|s| !s.claimed.load(Ordering::Acquire))
        .filter_map(|s| s.stalled_ms(now).map(|ms| (ms, s)))
        .max_by_key(|(ms, _)| *ms)
        .map(|(_, s)| StallHandle(s))
}

/// Есть ли JS-задача, идущая дольше [`HUNG_GATE_MS`]. UI-поток по этому признаку
/// не ждёт ответов движкового потока (он стоит за зависшим скриптом): блокирующее
/// чтение вернуло бы ответ только через таймаут, а в обход очереди — никогда.
#[must_use]
pub fn any_hung() -> bool {
    HUNG.load(Ordering::Acquire) > 0
}

fn ensure_watchdog() {
    WATCHDOG.get_or_init(|| {
        let spawned = std::thread::Builder::new().name("js-watchdog".into()).spawn(watchdog_loop);
        match spawned {
            Ok(h) => h.thread().clone(),
            // Без сторожа плашки не будет, остальное работает: подставляем
            // текущий поток — `unpark` на нём безвреден.
            Err(_) => std::thread::current(),
        }
    });
}

fn watchdog_loop() {
    loop {
        if BUSY.load(Ordering::Acquire) == 0 {
            std::thread::park();
            continue;
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
        let now = now_ms();
        let live: Vec<Arc<Shared>> = registry().iter().filter_map(Weak::upgrade).collect();
        for s in &live {
            s.mark_hung(now);
        }
        let newly: Vec<&Arc<Shared>> = live
            .iter()
            .filter(|s| s.stalled_ms(now).is_some() && !s.reported.swap(true, Ordering::AcqRel))
            .collect();
        if !newly.is_empty() {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_handle_never_stalls() {
        let h = StallHandle::new();
        assert_eq!(h.stalled_ms(), None);
        assert!(!h.terminate());
    }

    #[test]
    fn stall_needs_threshold_and_wait_rearms() {
        let s = Shared {
            busy_since: AtomicU64::new(1_000),
            quiet_since: AtomicU64::new(0),
            reported: AtomicBool::new(false),
            hung: AtomicBool::new(false),
            claimed: AtomicBool::new(false),
            terminator: Mutex::new(None),
        };
        let t = threshold_ms();
        assert_eq!(s.stalled_ms(1_000 + t - 1), None);
        assert_eq!(s.stalled_ms(1_000 + t), Some(t));
        // «Подождать» на отметке 1000+t: снова молчим REARM_MS.
        s.quiet_since.store(1_000 + t, Ordering::Release);
        assert_eq!(s.stalled_ms(1_000 + t + REARM_MS - 1), None);
        assert_eq!(s.stalled_ms(1_000 + t + REARM_MS), Some(t + REARM_MS));
    }

    #[test]
    fn terminate_runs_only_while_busy() {
        let h = StallHandle::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let h2 = Arc::clone(&hits);
        h.set_terminator(move || {
            h2.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!h.terminate());
        h.job_started();
        assert!(h.terminate());
        h.job_finished();
        assert!(!h.terminate());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unclaimed_listing_skips_claimed_handles() {
        let h = StallHandle::new();
        h.job_started();
        // Смотрим из будущего: порог уже вышел, ждать его в тесте незачем.
        let later = now_ms() + threshold_ms() + 1;
        assert!(unclaimed_stalled_at(later).is_some_and(|u| u.same_runtime(&h)));
        h.claim();
        assert!(unclaimed_stalled_at(later).is_none_or(|u| !u.same_runtime(&h)));
        h.job_finished();
    }
}
