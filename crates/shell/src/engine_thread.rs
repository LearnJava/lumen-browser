//! Persistent engine thread — off-UI-thread layout executor (ADR-016 M2.2).
//!
//! # Что это
//!
//! Долгоживущий фоновый поток `lumen-engine`, которому UI-сторона отдаёт
//! **задание** (замыкание, считающее layout+display-list по immutable-снапшоту),
//! а он возвращает готовый **коммит** обратно через latest-wins слот. M2 переносит
//! тяжёлый конвейер (style → layout → сборка display-list) с UI-потока сюда.
//!
//! M2.1 поставил каркас (поток парковался и ничего не делал). M2.2 делает поток
//! живым: [`EngineThread::submit`] шлёт задание, поток исполняет **только
//! новейшее** валидное задание пачки (coalescing) и кладёт результат в слот;
//! [`EngineThread::take_committed`] забирает его на UI-стороне. Исполнитель
//! обобщён по типу коммита `C`, поэтому этот модуль не зависит от layout-типов —
//! конкретный `EngineCommit` (с `LayoutBox`) и само задание живут в `main.rs`.
//!
//! # Инварианты ADR-016
//!
//! - **Cross-thread data = immutable snapshots (инвариант 1).** Задание захватывает
//!   `Arc`-снимки (документ, стили, шрифты); коммит `C` — владеющий результат.
//! - **Latest-wins, queue depth 1, coalescing (инвариант 2).** Из дренированной
//!   пачки исполняется только задание с наибольшим `generation` (старее —
//!   отменённые), результат кладётся в слот на один элемент; непрочитанный
//!   перезаписывается новее пришедшим.
//! - **Generation-guard.** Задание с `generation` старше уже исполненного —
//!   устаревшая (отменённая) навигация/relayout; отбрасывается без исполнения
//!   (тот же принцип, что `RenderDone`-гвард `generation != load_generation`).
//! - **Idle = parked on condvar (инвариант 6).** Поток спит на блокирующем
//!   `recv()`; без заданий CPU не тратится (сохраняем ~0% idle из BUG-271).

use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Задание/сигнал движковому потоку. Задания `Run` коалесцируются (latest-wins с
/// generation-guard); `Readback` — request/reply, исполняется всегда и отвечает
/// напрямую вызывающему; `Shutdown` завершает поток (шлётся из [`EngineThread`]'s
/// `Drop`).
enum EngineMsg<C> {
    /// Считать коммит `C` (замыкание исполняется на движковом потоке).
    /// `generation` — монотонный номер relayout/навигации, под которым задание
    /// поставлено; используется для coalescing и generation-guard.
    Run {
        /// Монотонный номер relayout/навигации задания.
        generation: u64,
        /// Работа, считающая коммит (style + layout + display-list) off-thread.
        job: Box<dyn FnOnce() -> C + Send>,
    },
    /// Считать коммит `C` синхронно и вернуть его **напрямую** вызывающему через
    /// `reply` (request/reply, ADR-016 M2.2c-1). В отличие от [`EngineMsg::Run`]
    /// readback **не коалесцируется** и **не проходит generation-guard**:
    /// вызывающая сторона блокируется на `reply.recv()` и ждёт ровно этот
    /// результат (свежая геометрия сразу после relayout — hit-test, caret,
    /// scrollIntoView), поэтому задание обязано исполниться. Результат идёт в
    /// `reply`, а не в latest-wins слот, и `applied_generation` он не двигает.
    ///
    /// Пока не конструируется вне тестов: механизм приземлён в M2.2c-1, а живые
    /// вызывающие (hit-test/caret/scrollIntoView) подключаются в M2.2c-3 — после
    /// того как M2.2c-2 переносит `js_ctx` на движковый поток. Тесты `run_batch_*`
    /// и `readback_*` покрывают путь целиком.
    #[allow(dead_code, reason = "механизм M2.2c-1; живые вызывающие — M2.2c-3")]
    Readback {
        /// Работа, считающая коммит off-thread.
        job: Box<dyn FnOnce() -> C + Send>,
        /// Одноразовый канал ответа (queue depth 1). Дроп без отправки (например,
        /// в shutdown-пачке) разблокирует вызывающего с `Err` → откат на sync.
        reply: SyncSender<C>,
    },
    /// Завершение потока.
    Shutdown,
}

/// Выходной слот коммита: latest-wins, queue depth 1 (ADR-016 инвариант 2).
/// Поток кладёт новейший исполненный коммит, UI-сторона забирает его через
/// [`EngineThread::take_committed`]; непрочитанный перезаписывается новее
/// пришедшим (устаревший роняется, а не копится).
type CommitSlot<C> = Arc<Mutex<Option<C>>>;

/// Хэндл долгоживущего движкового потока (ADR-016 M2.2).
///
/// Обобщён по типу коммита `C` (в shell — `crate::EngineCommit`), поэтому модуль
/// не зависит от layout-типов. Владеет управляющим каналом и слотом коммита; при
/// `Drop` шлёт `Shutdown` и джойнит поток.
pub struct EngineThread<C: Send + 'static> {
    /// Упорядоченный канал заданий движковому потоку.
    tx: Sender<EngineMsg<C>>,
    /// Latest-wins слот, куда поток кладёт новейший исполненный коммит.
    latest: CommitSlot<C>,
    /// Handle потока для join при shutdown.
    join: Option<JoinHandle<()>>,
}

impl<C: Send + 'static> EngineThread<C> {
    /// Запускает именованный движковый поток и возвращает хэндл.
    ///
    /// Поток немедленно паркуется на блокирующем `recv()` (инвариант 6) и ждёт
    /// первое задание — до появления консьюмеров он не потребляет CPU.
    ///
    /// # Errors
    /// Возвращает [`std::io::Error`], если ОС не смогла создать поток.
    pub fn spawn() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<EngineMsg<C>>();
        let latest: CommitSlot<C> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&latest);
        let join = thread::Builder::new()
            .name("lumen-engine".to_owned())
            .spawn(move || engine_thread_main(&rx, &slot))?;
        Ok(Self { tx, latest, join: Some(join) })
    }

    /// Ставит задание движковому потоку (fire-and-forget). `generation` —
    /// монотонный номер relayout/навигации; задание с меньшим `generation`, чем
    /// уже исполненное, отбрасывается. Молча игнорирует, если поток уже завершён
    /// (штатно при shutdown).
    pub fn submit(&self, generation: u64, job: impl FnOnce() -> C + Send + 'static) {
        let _ = self.tx.send(EngineMsg::Run { generation, job: Box::new(job) });
    }

    /// Забирает новейший исполненный коммит из слота, если он есть (latest-wins:
    /// вернёт самый свежий, промежуточные уже перезаписаны). Оставляет слот
    /// пустым до следующего коммита.
    pub fn take_committed(&self) -> Option<C> {
        self.latest.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Request/reply: ставит задание и **блокируется**, пока движковый поток не
    /// вернёт ровно его результат (ADR-016 M2.2c-1). В отличие от [`Self::submit`]
    /// (fire-and-forget, latest-wins, результат забирают позже через
    /// [`Self::take_committed`]), readback нужен вызывающему **прямо сейчас** —
    /// свежая геометрия сразу после relayout (hit-test, caret, scrollIntoView),
    /// поэтому задание не коалесцируется, минует generation-guard и latest-wins
    /// слот, а результат приходит по одноразовому каналу ответа.
    ///
    /// Задание всё равно исполняется **по порядку** в дренированной пачке: любой
    /// `submit`, поставленный раньше в той же пачке, применится до readback, так
    /// что тот видит согласованное состояние потока (хотя своё замыкание readback
    /// считает по собственному immutable-снимку — инвариант 1).
    ///
    /// Возвращает `None`, если поток уже завершён или получил `Shutdown` раньше,
    /// чем исполнил задание (канал ответа дропнут) — вызывающая сторона тогда
    /// откатывается на синхронный путь.
    #[allow(dead_code, reason = "механизм M2.2c-1; живые вызывающие — M2.2c-3")]
    pub fn readback(&self, job: impl FnOnce() -> C + Send + 'static) -> Option<C> {
        // Queue depth 1: ровно один ответ на одно задание.
        let (reply_tx, reply_rx) = mpsc::sync_channel::<C>(1);
        self.tx
            .send(EngineMsg::Readback { job: Box::new(job), reply: reply_tx })
            .ok()?;
        // Блокируемся до ответа; `Err` (sender дропнут при shutdown) → None.
        reply_rx.recv().ok()
    }
}

impl<C: Send + 'static> Drop for EngineThread<C> {
    fn drop(&mut self) {
        let _ = self.tx.send(EngineMsg::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Тело движкового потока: паркуется на блокирующем `recv()` (инвариант 6),
/// дренирует пришедшую пачку и исполняет её ([`run_batch`]) — latest-wins с
/// generation-guard. Выходит при `Shutdown` или закрытии канала (хэндл дропнут).
fn engine_thread_main<C: Send + 'static>(rx: &Receiver<EngineMsg<C>>, latest: &CommitSlot<C>) {
    let mut applied_generation: u64 = 0;
    loop {
        // Idle-park до первого сообщения (без polling — инвариант 6).
        let first = match rx.recv() {
            Ok(m) => m,
            Err(_) => return, // канал закрыт — хэндл дропнут
        };
        let mut batch = vec![first];
        // Дренируем всё, что уже в очереди, одним махом (coalescing). `try_recv`
        // возвращает `Err` и на пустоте, и на закрытии — в обоих случаях пачка
        // собрана, выходим из дренажа.
        while let Ok(m) = rx.try_recv() {
            batch.push(m);
        }
        if run_batch(batch, &mut applied_generation, latest) {
            return; // получен Shutdown
        }
    }
}

/// Исполняет одну дренированную пачку. При наличии `Shutdown` — сигналит выход
/// **не исполняя ничего** (в т.ч. readback: их каналы ответа дропаются → блокирующие
/// вызывающие получают `Err` и откатываются на sync). Иначе:
/// - для заданий `Run` работает latest-wins + generation-guard — исполняется
///   **только новейшее** валидное замыкание (остальные роняются — экономия
///   layout-работы), результат идёт в `latest`, `applied_generation` продвигается;
/// - каждый `Readback` исполняется **всегда** и по своей позиции в пачке (не
///   коалесцируется, `applied_generation`/`latest` не трогает), результат уходит
///   напрямую в его `reply` — вызывающий разблокируется ровно этим коммитом.
///
/// Возвращает `true`, если в пачке был `Shutdown`.
///
/// Вынесено из цикла ради модульного теста логики coalescing/gen-guard/readback
/// без поднятия потока.
fn run_batch<C: Send + 'static>(
    batch: Vec<EngineMsg<C>>,
    applied_generation: &mut u64,
    latest: &CommitSlot<C>,
) -> bool {
    if batch.iter().any(|m| matches!(m, EngineMsg::Shutdown)) {
        return true;
    }
    // Индекс новейшего валидного `Run` (latest-wins + gen-guard). `Readback` и
    // прочие сообщения не участвуют в выборе — они не `Run`.
    let gens: Vec<Option<u64>> = batch
        .iter()
        .map(|m| match m {
            EngineMsg::Run { generation, .. } => Some(*generation),
            _ => None,
        })
        .collect();
    let newest_run = newest_job_index(&gens, *applied_generation);
    for (i, msg) in batch.into_iter().enumerate() {
        match msg {
            EngineMsg::Run { generation, job } => {
                // Исполняем только новейший `Run`; ранние/устаревшие роняем.
                if Some(i) == newest_run {
                    *applied_generation = generation;
                    let commit = job();
                    if let Ok(mut slot) = latest.lock() {
                        *slot = Some(commit);
                    }
                }
            }
            EngineMsg::Readback { job, reply } => {
                // Readback исполняется всегда; результат — напрямую вызывающему.
                // `send` может вернуть `Err`, если тот отказался ждать — тогда
                // молча роняем (queue depth 1, никогда не блокирует поток).
                let commit = job();
                let _ = reply.send(commit);
            }
            // `Shutdown` уже отсеян ранним `return true` выше.
            EngineMsg::Shutdown => {}
        }
    }
    false
}

/// Индекс задания, которое должно победить в дренированной пачке (latest-wins +
/// generation-guard). `gens[i]` — `Some(generation)` для задания `Run`, `None`
/// для управляющих сообщений (игнорируются). Среди заданий с
/// `generation >= min_generation` (более старые — отменённый relayout,
/// отбрасываются) возвращает индекс с наибольшим `generation`; при равенстве
/// побеждает более поздний индекс (latest-wins). `None`, если валидного задания
/// в пачке нет.
fn newest_job_index(gens: &[Option<u64>], min_generation: u64) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (i, g) in gens.iter().enumerate() {
        let Some(g) = *g else { continue };
        if g < min_generation {
            continue; // устаревший relayout — отбрасываем
        }
        match best {
            // Строго меньший generation проигрывает — оставляем текущий best.
            Some((_, bg)) if g < bg => {}
            // Больший или равный побеждает (равный → поздний индекс).
            _ => best = Some((i, g)),
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Задание с указанным `generation`, возвращающее это же число коммитом.
    /// Тестовый тип коммита — `u64`, поэтому layout-типы здесь не нужны.
    fn run(generation: u64) -> EngineMsg<u64> {
        EngineMsg::Run { generation, job: Box::new(move || generation) }
    }

    #[test]
    fn newest_job_index_picks_latest_of_equal_generation() {
        // Три задания одного relayout подряд — побеждает последнее (latest-wins).
        let gens = [Some(1), Some(1), Some(1)];
        assert_eq!(newest_job_index(&gens, 0), Some(2));
    }

    #[test]
    fn newest_job_index_ignores_control_messages() {
        // `None` (Shutdown-подобные) не влияют на выбор.
        let gens = [Some(2), None, Some(5)];
        assert_eq!(newest_job_index(&gens, 0), Some(2));
    }

    #[test]
    fn newest_job_index_prefers_highest_generation_over_position() {
        // Более высокий generation побеждает, даже если он раньше по позиции.
        let gens = [Some(7), Some(5)];
        assert_eq!(newest_job_index(&gens, 0), Some(0));
    }

    #[test]
    fn newest_job_index_drops_stale_generations() {
        // min_generation=5: задание gen 3 устарело; gen 6 — валидно.
        let gens = [Some(3), Some(6)];
        assert_eq!(newest_job_index(&gens, 5), Some(1));
    }

    #[test]
    fn newest_job_index_none_when_all_stale() {
        // Все задания старше уже исполненного поколения → нечего исполнять.
        let gens = [Some(2), Some(4)];
        assert_eq!(newest_job_index(&gens, 5), None);
    }

    #[test]
    fn newest_job_index_none_without_jobs() {
        let gens = [None];
        assert_eq!(newest_job_index(&gens, 0), None);
    }

    #[test]
    fn run_batch_executes_newest_and_advances_generation() {
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        // Пачка relayout 1 и 2 — исполняется gen 2, поколение продвигается.
        let shutdown = run_batch(vec![run(1), run(2)], &mut applied, &latest);
        assert!(!shutdown);
        assert_eq!(applied, 2);
        assert_eq!(latest.lock().unwrap().take(), Some(2));
    }

    #[test]
    fn run_batch_drops_stale_job_and_keeps_generation() {
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 5;
        // Единственное задание устарело (gen 3 < 5) — слот пуст, поколение не падает.
        let shutdown = run_batch(vec![run(3)], &mut applied, &latest);
        assert!(!shutdown);
        assert_eq!(applied, 5);
        assert!(latest.lock().unwrap().is_none());
    }

    #[test]
    fn run_batch_coalesces_to_single_newest_job() {
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        // Десять заданий подряд — исполняется ровно одно, новейшее (queue depth 1).
        let batch: Vec<EngineMsg<u64>> = (1..=10).map(run).collect();
        run_batch(batch, &mut applied, &latest);
        assert_eq!(applied, 10);
        assert_eq!(latest.lock().unwrap().take(), Some(10));
    }

    #[test]
    fn run_batch_only_newest_closure_runs() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        // Счётчик исполнений: из пачки должно исполниться ровно одно замыкание,
        // остальные — дропнуться без вызова (экономия layout-работы).
        let calls = Arc::new(AtomicUsize::new(0));
        let mk = |generation: u64| {
            let c = Arc::clone(&calls);
            EngineMsg::Run {
                generation,
                job: Box::new(move || {
                    c.fetch_add(1, Ordering::SeqCst);
                    generation
                }),
            }
        };
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        run_batch(vec![mk(1), mk(2), mk(3)], &mut applied, &latest);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(latest.lock().unwrap().take(), Some(3));
    }

    #[test]
    fn run_batch_reports_shutdown() {
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        // Shutdown в пачке — сигнал выхода; задание при этом не исполняется.
        let shutdown = run_batch(vec![run(1), EngineMsg::Shutdown], &mut applied, &latest);
        assert!(shutdown);
        assert!(latest.lock().unwrap().is_none());
    }

    /// Readback-сообщение с известным каналом ответа. Тип коммита — `u64`.
    fn readback_msg(value: u64) -> (EngineMsg<u64>, mpsc::Receiver<u64>) {
        let (tx, rx) = mpsc::sync_channel::<u64>(1);
        (EngineMsg::Readback { job: Box::new(move || value), reply: tx }, rx)
    }

    #[test]
    fn run_batch_executes_readback_and_replies() {
        // Readback исполняется и отвечает напрямую; latest-wins слот не трогает.
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        let (msg, rx) = readback_msg(77);
        let shutdown = run_batch(vec![msg], &mut applied, &latest);
        assert!(!shutdown);
        assert_eq!(rx.recv().ok(), Some(77));
        assert_eq!(applied, 0, "readback не двигает applied_generation");
        assert!(latest.lock().unwrap().is_none(), "readback не пишет в слот");
    }

    #[test]
    fn run_batch_runs_readback_and_newest_run_together() {
        // Пачка [Run(1), Readback, Run(2)]: слот получает новейший Run (2),
        // readback независимо отвечает своим коммитом. Оба исполняются.
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        let (msg, rx) = readback_msg(500);
        let batch = vec![run(1), msg, run(2)];
        run_batch(batch, &mut applied, &latest);
        assert_eq!(applied, 2);
        assert_eq!(latest.lock().unwrap().take(), Some(2));
        assert_eq!(rx.recv().ok(), Some(500));
    }

    #[test]
    fn run_batch_never_coalesces_readbacks() {
        // Несколько readback в пачке — каждый исполняется и отвечает своему
        // вызывающему (в отличие от Run, которые коалесцируются до одного).
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        let (m1, rx1) = readback_msg(10);
        let (m2, rx2) = readback_msg(20);
        let (m3, rx3) = readback_msg(30);
        run_batch(vec![m1, m2, m3], &mut applied, &latest);
        assert_eq!(rx1.recv().ok(), Some(10));
        assert_eq!(rx2.recv().ok(), Some(20));
        assert_eq!(rx3.recv().ok(), Some(30));
    }

    #[test]
    fn run_batch_shutdown_drops_readback_reply() {
        // Shutdown в пачке роняет всё, включая readback: его reply дропается,
        // вызывающий получает Err (→ откат на sync).
        let latest: CommitSlot<u64> = Arc::new(Mutex::new(None));
        let mut applied = 0;
        let (msg, rx) = readback_msg(9);
        let shutdown = run_batch(vec![msg, EngineMsg::Shutdown], &mut applied, &latest);
        assert!(shutdown);
        assert!(rx.recv().is_err(), "reply-канал должен быть дропнут");
    }

    #[test]
    fn spawn_submit_and_shutdown_lifecycle() {
        // Полный жизненный цикл: поток стартует, исполняет задание, чисто
        // завершается на Drop (Shutdown + join). Коммит забираем спином с
        // yield — детерминированно без завязки на часы.
        let engine = EngineThread::<u64>::spawn().expect("spawn engine thread");
        engine.submit(1, || 42);
        let mut got = None;
        for _ in 0..100_000 {
            if let Some(c) = engine.take_committed() {
                got = Some(c);
                break;
            }
            thread::yield_now();
        }
        assert_eq!(got, Some(42));
        // Drop здесь: шлёт Shutdown и джойнит поток без паники.
    }

    #[test]
    fn readback_blocks_and_returns_result() {
        // End-to-end request/reply: `readback` блокируется до ответа потока и
        // возвращает ровно результат задания. Никакого спина — метод сам ждёт.
        let engine = EngineThread::<u64>::spawn().expect("spawn engine thread");
        let got = engine.readback(|| 1234);
        assert_eq!(got, Some(1234));
    }

    #[test]
    fn readback_after_submit_sees_thread_alive() {
        // Смешанная нагрузка: submit (fire-and-forget) + readback подряд.
        // readback обязан вернуться (поток жив, задание исполнено по порядку).
        let engine = EngineThread::<u64>::spawn().expect("spawn engine thread");
        engine.submit(1, || 1);
        engine.submit(2, || 2);
        assert_eq!(engine.readback(|| 99), Some(99));
    }
}
