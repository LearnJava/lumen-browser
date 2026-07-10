// ADR-016 M2.1 — каркас: коммит-путь (`EngineThread::commit` /
// `take_committed`), variant `Commit` и поля снапшота ещё не потребляются shell
// — их включает M2.2 (маршрутизация `relayout()` через движковый поток). До тех
// пор каркас помечен `allow(dead_code)`, чтобы `clippy -D warnings` был зелёным;
// снять этот атрибут при выполнении M2.2, когда путь станет живым.
#![allow(dead_code)]

//! Persistent engine thread — scaffold (ADR-016 M2).
//!
//! # Что это (M2.1)
//!
//! Каркас движкового потока, зеркальный [`crate::render_thread`]. M2 переносит
//! тяжёлый конвейер (style → layout → сборка display-list) с UI-потока на
//! долгоживущий фоновый поток, который **коммитит снапшоты** обратно. Этот срез
//! (M2.1) ставит только *каркас*: именованный поток `lumen-engine`, упорядоченный
//! управляющий канал, latest-wins слот коммита с generation-guard и тип-снапшот
//! [`EngineCommit`], над которым будут работать M2.2+. **Ни один relayout ещё не
//! перенесён** — при `LUMEN_ENGINE_THREAD` (по умолчанию выкл) поток просто
//! паркуется на `recv()` и ничего не делает; поведение shell не меняется.
//!
//! Сегодняшний конвейер использует одноразовый `std::thread::spawn(render_bytes)`
//! на каждую навигацию (`main.rs`, `LoadEvent::RawBytes`); M2.2 заменит его на
//! коммит через этот долгоживущий поток.
//!
//! # Инварианты ADR-016, которые закладывает каркас
//!
//! - **Cross-thread data = immutable snapshots (инвариант 1).** Движок коммитит
//!   [`EngineCommit`] с `Arc<DisplayList>`; UI-сторона забирает готовый снапшот,
//!   без разделяемого мутабельного состояния.
//! - **Latest-wins, queue depth 1, coalescing (инвариант 2).** Дренаж канала
//!   оставляет только новейший *валидный* коммит пачки; выходной слот держит
//!   ровно один коммит — медленный потребитель роняет устаревшие, а не копит их.
//! - **Generation-guard.** Коммит с `generation` старше уже применённого — это
//!   отменённая (устаревшая) навигация; он отбрасывается (тот же принцип, что
//!   `RenderDone`-гвард `generation != load_generation` в `main.rs`).
//! - **Idle = parked on condvar (инвариант 6).** Поток спит на блокирующем
//!   `recv()`; без коммитов CPU не тратится (сохраняем ~0% idle из BUG-271).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use lumen_core::geom::Size;
use lumen_paint::DisplayList;

/// Неизменяемый снапшот, который движковый поток коммитит UI-стороне
/// (ADR-016 инвариант 1). Всё содержимое — владеющая копия / `Arc`, поэтому
/// после коммита UI и движок не делят мутабельное состояние.
pub struct EngineCommit {
    /// Готовый display-list страницы (immutable, разделяемый через `Arc`).
    pub content: Arc<DisplayList>,
    /// Монотонный номер навигации, под которым построен коммит. Коммит с
    /// `generation` старше уже применённого отбрасывается как устаревший.
    pub generation: u64,
    /// Размеры layout-viewport, под которые построен `content` (CSS px).
    pub dims: Size,
}

/// Сообщение движковому потоку. Коммиты коалесцируются (latest-wins с
/// generation-guard); `Shutdown` завершает поток (шлётся из [`EngineThread`]'s
/// `Drop`).
enum EngineMsg {
    /// Новый снапшот страницы (latest-wins).
    Commit(EngineCommit),
    /// Завершение потока.
    Shutdown,
}

/// Выходной слот коммита: latest-wins, queue depth 1 (ADR-016 инвариант 2).
/// Движок кладёт новейший валидный коммит, UI-сторона забирает его через
/// [`EngineThread::take_committed`]; непрочитанный коммит перезаписывается
/// новее пришедшим (устаревший роняется, а не копится).
type CommitSlot = Arc<Mutex<Option<EngineCommit>>>;

/// Хэндл долгоживущего движкового потока (ADR-016 M2.1, каркас).
///
/// Владеет управляющим каналом и слотом коммита; при `Drop` шлёт `Shutdown` и
/// джойнит поток. В M2.1 через него ещё ничего не маршрутизируется — это
/// «место, куда будут коммитить» M2.2+.
pub struct EngineThread {
    /// Упорядоченный канал сообщений движковому потоку.
    tx: Sender<EngineMsg>,
    /// Latest-wins слот, куда поток кладёт новейший валидный коммит.
    latest: CommitSlot,
    /// Handle потока для join при shutdown.
    join: Option<JoinHandle<()>>,
}

impl EngineThread {
    /// Запускает именованный движковый поток и возвращает хэндл.
    ///
    /// Поток немедленно паркуется на блокирующем `recv()` (инвариант 6) и ждёт
    /// первый коммит — до появления консьюмеров (M2.2+) он не потребляет CPU.
    ///
    /// # Errors
    /// Возвращает [`std::io::Error`], если ОС не смогла создать поток.
    pub fn spawn() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<EngineMsg>();
        let latest: CommitSlot = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&latest);
        let join = thread::Builder::new()
            .name("lumen-engine".to_owned())
            .spawn(move || engine_thread_main(&rx, &slot))?;
        Ok(Self { tx, latest, join: Some(join) })
    }

    /// Коммитит снапшот движковому потоку (fire-and-forget). Молча игнорирует,
    /// если поток уже завершён (штатно при shutdown).
    pub fn commit(&self, commit: EngineCommit) {
        let _ = self.tx.send(EngineMsg::Commit(commit));
    }

    /// Забирает новейший применённый коммит из слота, если он есть (latest-wins:
    /// вернёт самый свежий, промежуточные уже перезаписаны). Оставляет слот
    /// пустым до следующего коммита.
    pub fn take_committed(&self) -> Option<EngineCommit> {
        self.latest.lock().ok().and_then(|mut slot| slot.take())
    }
}

impl Drop for EngineThread {
    fn drop(&mut self) {
        let _ = self.tx.send(EngineMsg::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Тело движкового потока: паркуется на блокирующем `recv()` (инвариант 6),
/// дренирует пришедшую пачку и применяет её ([`apply_batch`]) — latest-wins с
/// generation-guard. Выходит при `Shutdown` или закрытии канала (хэндл дропнут).
fn engine_thread_main(rx: &Receiver<EngineMsg>, latest: &CommitSlot) {
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
        if apply_batch(batch, &mut applied_generation, latest) {
            return; // получен Shutdown
        }
    }
}

/// Применяет одну дренированную пачку: при наличии `Shutdown` — сигналит выход;
/// иначе выбирает новейший валидный коммит (latest-wins + generation-guard) и
/// кладёт его в `latest`, продвигая `applied_generation`. Устаревшие коммиты
/// (generation старше применённого) и все, кроме победившего, отбрасываются.
/// Возвращает `true`, если в пачке был `Shutdown` (поток должен выйти).
///
/// Вынесено из цикла ради модульного теста логики коалесцинга/gen-guard без
/// поднятия потока.
fn apply_batch(batch: Vec<EngineMsg>, applied_generation: &mut u64, latest: &CommitSlot) -> bool {
    if batch.iter().any(|m| matches!(m, EngineMsg::Shutdown)) {
        return true;
    }
    if let Some(idx) = newest_commit_index(&batch, *applied_generation)
        && let Some(EngineMsg::Commit(commit)) = batch.into_iter().nth(idx)
    {
        *applied_generation = commit.generation;
        if let Ok(mut slot) = latest.lock() {
            *slot = Some(commit);
        }
    }
    false
}

/// Индекс коммита, который должен победить в дренированной пачке (latest-wins +
/// generation-guard). Среди всех `Commit` с `generation >= min_generation`
/// (более старые — отменённая навигация, отбрасываются) возвращает индекс
/// коммита с наибольшим `generation`; при равенстве побеждает более поздний
/// индекс (latest-wins). Не-коммиты и устаревшие коммиты игнорируются.
/// `None`, если валидного коммита в пачке нет.
fn newest_commit_index(batch: &[EngineMsg], min_generation: u64) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (i, msg) in batch.iter().enumerate() {
        if let EngineMsg::Commit(c) = msg {
            if c.generation < min_generation {
                continue; // устаревшая навигация — отбрасываем
            }
            match best {
                // Строго меньший generation проигрывает — оставляем текущий best.
                Some((_, g)) if c.generation < g => {}
                // Больший или равный generation побеждает (равный → поздний индекс).
                _ => best = Some((i, c.generation)),
            }
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Коммит-сообщение с указанным `generation` и пустым content.
    fn commit(generation: u64) -> EngineMsg {
        EngineMsg::Commit(EngineCommit {
            content: Arc::new(DisplayList::new()),
            generation,
            dims: Size::new(1024.0, 720.0),
        })
    }

    #[test]
    fn newest_commit_index_picks_latest_of_equal_generation() {
        // Три коммита одной навигации подряд — побеждает последний (latest-wins).
        let batch = vec![commit(1), commit(1), commit(1)];
        assert_eq!(newest_commit_index(&batch, 0), Some(2));
    }

    #[test]
    fn newest_commit_index_ignores_control_messages() {
        // Между коммитами есть Shutdown-подобные управляющие — они не влияют
        // на выбор (здесь эмулируем произвольный порядок коммитов).
        let batch = vec![commit(2), commit(5)];
        assert_eq!(newest_commit_index(&batch, 0), Some(1));
    }

    #[test]
    fn newest_commit_index_prefers_highest_generation_over_position() {
        // Более высокий generation побеждает, даже если он раньше по позиции.
        let batch = vec![commit(7), commit(5)];
        assert_eq!(newest_commit_index(&batch, 0), Some(0));
    }

    #[test]
    fn newest_commit_index_drops_stale_generations() {
        // min_generation=5: коммит gen 3 устарел; gen 6 — валиден.
        let batch = vec![commit(3), commit(6)];
        assert_eq!(newest_commit_index(&batch, 5), Some(1));
    }

    #[test]
    fn newest_commit_index_none_when_all_stale() {
        // Все коммиты старше уже применённого поколения → нечего применять.
        let batch = vec![commit(2), commit(4)];
        assert_eq!(newest_commit_index(&batch, 5), None);
    }

    #[test]
    fn newest_commit_index_none_without_commits() {
        let batch = vec![EngineMsg::Shutdown];
        assert_eq!(newest_commit_index(&batch, 0), None);
    }

    #[test]
    fn apply_batch_deposits_newest_and_advances_generation() {
        let latest: CommitSlot = Arc::new(Mutex::new(None));
        let mut applied = 0;
        // Пачка навигаций 1 и 2 — в слот ложится gen 2, поколение продвигается.
        let shutdown = apply_batch(vec![commit(1), commit(2)], &mut applied, &latest);
        assert!(!shutdown);
        assert_eq!(applied, 2);
        let deposited = latest.lock().unwrap().take();
        assert_eq!(deposited.map(|c| c.generation), Some(2));
    }

    #[test]
    fn apply_batch_drops_stale_commit_and_keeps_generation() {
        let latest: CommitSlot = Arc::new(Mutex::new(None));
        let mut applied = 5;
        // Единственный коммит устарел (gen 3 < 5) — слот пуст, поколение не падает.
        let shutdown = apply_batch(vec![commit(3)], &mut applied, &latest);
        assert!(!shutdown);
        assert_eq!(applied, 5);
        assert!(latest.lock().unwrap().is_none());
    }

    #[test]
    fn apply_batch_coalesces_to_single_latest_commit() {
        let latest: CommitSlot = Arc::new(Mutex::new(None));
        let mut applied = 0;
        // Десять коммитов подряд — в слот попадает ровно один, новейший (queue depth 1).
        let batch: Vec<EngineMsg> = (1..=10).map(commit).collect();
        apply_batch(batch, &mut applied, &latest);
        assert_eq!(applied, 10);
        assert_eq!(latest.lock().unwrap().as_ref().map(|c| c.generation), Some(10));
    }

    #[test]
    fn apply_batch_reports_shutdown() {
        let latest: CommitSlot = Arc::new(Mutex::new(None));
        let mut applied = 0;
        // Shutdown в пачке — сигнал выхода; коммит при этом не применяется.
        let shutdown = apply_batch(vec![commit(1), EngineMsg::Shutdown], &mut applied, &latest);
        assert!(shutdown);
        assert!(latest.lock().unwrap().is_none());
    }

    #[test]
    fn spawn_commit_and_shutdown_lifecycle() {
        // Полный жизненный цикл: поток стартует, принимает коммит, чисто
        // завершается на Drop (Shutdown + join). Коммит забираем спином с
        // yield — детерминированно без завязки на часы.
        let engine = EngineThread::spawn().expect("spawn engine thread");
        engine.commit(EngineCommit {
            content: Arc::new(DisplayList::new()),
            generation: 1,
            dims: Size::new(800.0, 600.0),
        });
        let mut got = None;
        for _ in 0..100_000 {
            if let Some(c) = engine.take_committed() {
                got = Some(c);
                break;
            }
            thread::yield_now();
        }
        assert_eq!(got.map(|c| c.generation), Some(1));
        // Drop здесь: шлёт Shutdown и джойнит поток без паники.
    }
}
