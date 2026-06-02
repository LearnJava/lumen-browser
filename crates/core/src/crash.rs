//! Crash recorder: кольцевой буфер последних событий + дамп при панике.
//!
//! Реализует пункт §9.6 / lumen-plan.md «Crash hook на `EventSink`»: последние
//! N событий ([`Event`]) держатся в памяти, и при панике процесса их снимок
//! вместе с текстом паники сбрасывается в файл-дамп **до** завершения процесса.
//! Цель — посмертная диагностика: «что движок делал за мгновение до краха».
//!
//! [`CrashRecorder`] — декоратор над [`EventSink`]: он записывает каждое
//! событие в кольцевой буфер и (опционально) форвардит его дальше реальному
//! наблюдателю (network log UI и т.п.). Поэтому его можно вставить в цепочку
//! sink-ов без потери существующей доставки событий.
//!
//! Механизм разделён на чистые, юнит-тестируемые куски ([`format_crash_dump`],
//! [`write_crash_dump`]) и тонкую обвязку panic-hook ([`CrashRecorder::install_panic_hook`]),
//! которая лишь склеивает их с process-global `std::panic::set_hook`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::event::Event;
use crate::ext::EventSink;

/// Сколько последних событий держим по умолчанию. §9.6 называет 50 —
/// достаточно, чтобы увидеть навигацию + серию запросов перед крахом, и
/// дёшево по памяти (50 коротких строк).
pub const DEFAULT_CAPACITY: usize = 50;

/// Одна запись в кольцевом буфере: текстовое представление события плюс
/// контекст для упорядочивания в дампе.
#[derive(Debug, Clone)]
struct EventRecord {
    /// Порядковый номер с момента старта рекордера (1-based, монотонно
    /// растёт; не сбрасывается при вытеснении из буфера). Позволяет понять,
    /// сколько событий «утекло» до первой строки дампа.
    seq: u64,
    /// Монотонное время от создания рекордера до записи события, в мс.
    /// Используется `Instant`, а не wall-clock, чтобы не зависеть от
    /// перевода системных часов.
    at_ms: u128,
    /// `Debug`-представление события. Храним строку, а не `Event`, чтобы не
    /// тащить в буфер `Url`/`String`-аллокации сверх нужного и чтобы дамп
    /// формировался без обращения к чужим типам.
    text: String,
}

/// Внутреннее состояние под `Mutex`: кольцевой буфер + счётчик.
#[derive(Debug)]
struct Inner {
    /// Кольцевой буфер событий, oldest-first. Длина ≤ `capacity`.
    buffer: VecDeque<EventRecord>,
    /// Максимальная ёмкость буфера. При превышении вытесняется самый старый.
    capacity: usize,
    /// Сколько всего событий записано (включая вытесненные). Источник `seq`.
    total: u64,
}

/// Рекордер событий с кольцевым буфером и дампом при панике.
///
/// Реализует [`EventSink`]: каждый `emit` записывается в буфер (и форвардится
/// downstream-sink-у, если задан). `&self`-интерфейс trait-а сохранён за счёт
/// внутренней `Mutex`-синхронизации, так что рекордер можно делить между
/// потоками через `Arc`.
pub struct CrashRecorder {
    /// Разделяемое состояние. `Arc`, чтобы panic-hook мог держать снимок
    /// буфера даже после того, как остальные владельцы рекордера уже ушли.
    inner: Arc<Mutex<Inner>>,
    /// Момент создания рекордера — база для относительных таймстампов.
    start: Instant,
    /// Необязательный следующий sink в цепочке. `None` — рекордер только
    /// копит события, никуда их не форвардя (headless/тесты).
    downstream: Option<Arc<dyn EventSink>>,
}

impl CrashRecorder {
    /// Рекордер с ёмкостью буфера по умолчанию ([`DEFAULT_CAPACITY`]) и без
    /// downstream-sink-а.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Рекордер с заданной ёмкостью буфера и без downstream-sink-а.
    /// `capacity = 0` нормализуется в 1 (буфер всегда хранит хотя бы
    /// последнее событие).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buffer: VecDeque::new(),
                capacity: capacity.max(1),
                total: 0,
            })),
            start: Instant::now(),
            downstream: None,
        }
    }

    /// Рекордер, форвардящий каждое событие дальше указанному sink-у после
    /// записи в буфер. Позволяет вставить crash-рекордер в существующую
    /// цепочку доставки событий, ничего не ломая.
    pub fn with_downstream(downstream: Arc<dyn EventSink>) -> Self {
        Self {
            downstream: Some(downstream),
            ..Self::new()
        }
    }

    /// Снимок текущего содержимого буфера в виде готовых строк дампа
    /// (oldest-first). Каждая строка: `[+<ms> ms] #<seq> <event-debug>`.
    /// Не блокирует надолго — копирует под кратким захватом мьютекса.
    pub fn recent_events(&self) -> Vec<String> {
        let guard = match self.inner.lock() {
            Ok(g) => g,
            // Мьютекс отравлен предыдущей паникой — всё равно читаем данные,
            // дамп важнее чистоты блокировки.
            Err(poisoned) => poisoned.into_inner(),
        };
        guard
            .buffer
            .iter()
            .map(|r| format!("[+{} ms] #{} {}", r.at_ms, r.seq, r.text))
            .collect()
    }

    /// Сколько событий записано всего с момента старта (включая вытесненные
    /// из буфера). Полезно для тестов и заголовка дампа.
    pub fn total_recorded(&self) -> u64 {
        match self.inner.lock() {
            Ok(g) => g.total,
            Err(poisoned) => poisoned.into_inner().total,
        }
    }

    /// Установить process-global panic-hook, который при панике пишет дамп
    /// последних событий + текста паники в `dump_dir` и затем вызывает
    /// ранее установленный hook (печать в stderr / backtrace сохраняются).
    ///
    /// Идемпотентность не гарантируется: повторный вызов вложит ещё один
    /// слой поверх предыдущего hook-а. Вызывать один раз при старте процесса
    /// (обычно из shell `main`). Ошибка записи файла молча игнорируется —
    /// нельзя дать упасть самому обработчику паники.
    pub fn install_panic_hook(self: &Arc<Self>, dump_dir: impl Into<PathBuf>) {
        let recorder = Arc::clone(self);
        let dir = dump_dir.into();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let msg = panic_message(info);
            let dump = format_crash_dump(&recorder.recent_events(), &msg);
            let _ = write_crash_dump(&dir, &dump);
            prev(info);
        }));
    }
}

impl Default for CrashRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for CrashRecorder {
    fn emit(&self, event: &Event) {
        let at_ms = self.start.elapsed().as_millis();
        {
            let mut guard = match self.inner.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.total += 1;
            let seq = guard.total;
            let cap = guard.capacity;
            guard.buffer.push_back(EventRecord {
                seq,
                at_ms,
                text: format!("{event:?}"),
            });
            while guard.buffer.len() > cap {
                guard.buffer.pop_front();
            }
        }
        if let Some(downstream) = &self.downstream {
            downstream.emit(event);
        }
    }
}

/// Собрать текст crash-дампа из снимка событий и сообщения паники.
///
/// Чистая функция — без I/O и без обращения к часам сверх единственного
/// `SystemTime::now()` для заголовка. Формат человекочитаемый и стабильный
/// (юнит-тесты опираются на маркеры-строки).
pub fn format_crash_dump(events: &[String], panic_message: &str) -> String {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut out = String::with_capacity(256 + events.iter().map(|e| e.len() + 1).sum::<usize>());
    out.push_str("=== Lumen crash dump ===\n");
    out.push_str(&format!("time_unix_ms: {unix_ms}\n"));
    out.push_str(&format!("panic: {panic_message}\n"));
    out.push_str(&format!(
        "--- last {} event(s), oldest first ---\n",
        events.len()
    ));
    if events.is_empty() {
        out.push_str("(no events recorded)\n");
    } else {
        for line in events {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str("=== end of crash dump ===\n");
    out
}

/// Записать готовый текст дампа в новый файл `lumen-crash-<unix_ms>.log`
/// внутри `dir`. Каталог создаётся при необходимости. Возвращает путь
/// записанного файла.
///
/// Имя включает unix-миллисекунды, чтобы повторные краши не перетирали друг
/// друга. Отдельная функция (а не инлайн в hook) — чтобы покрыть запись
/// юнит-тестом, не вызывая настоящую панику.
pub fn write_crash_dump(dir: &Path, contents: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("lumen-crash-{unix_ms}.log"));
    std::fs::write(&path, contents)?;
    Ok(path)
}

/// Извлечь из `PanicHookInfo` человекочитаемое сообщение: payload (если это
/// `&str` / `String`) и место паники (`file:line:col`).
fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_owned());
    match info.location() {
        Some(loc) => format!("{payload} (at {}:{}:{})", loc.file(), loc.line(), loc.column()),
        None => payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::TabId;
    use crate::url::Url;

    fn nav(n: u32) -> Event {
        Event::Navigation {
            tab_id: TabId(n),
            url: Url::parse(&format!("https://example.com/{n}")).unwrap(),
        }
    }

    #[test]
    fn records_events_in_order() {
        let rec = CrashRecorder::new();
        rec.emit(&Event::TabCreated { tab_id: TabId(1) });
        rec.emit(&nav(1));
        let lines = rec.recent_events();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("TabCreated"));
        assert!(lines[1].contains("Navigation"));
        assert!(lines[0].contains("#1"));
        assert!(lines[1].contains("#2"));
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let rec = CrashRecorder::with_capacity(3);
        for i in 0..5 {
            rec.emit(&Event::TabCreated { tab_id: TabId(i) });
        }
        let lines = rec.recent_events();
        // Только последние 3 события остаются.
        assert_eq!(lines.len(), 3);
        // seq продолжает расти — самый старый в буфере это #3.
        assert!(lines[0].contains("#3"));
        assert!(lines[2].contains("#5"));
        // total учитывает вытесненные.
        assert_eq!(rec.total_recorded(), 5);
    }

    #[test]
    fn capacity_zero_normalizes_to_one() {
        let rec = CrashRecorder::with_capacity(0);
        rec.emit(&Event::TabCreated { tab_id: TabId(1) });
        rec.emit(&Event::TabCreated { tab_id: TabId(2) });
        let lines = rec.recent_events();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("TabId(2)"));
    }

    #[test]
    fn default_capacity_is_50() {
        let rec = CrashRecorder::new();
        for i in 0..60 {
            rec.emit(&Event::TabCreated { tab_id: TabId(i) });
        }
        assert_eq!(rec.recent_events().len(), DEFAULT_CAPACITY);
        assert_eq!(rec.total_recorded(), 60);
    }

    struct CountingSink {
        count: Arc<Mutex<usize>>,
    }
    impl EventSink for CountingSink {
        fn emit(&self, _event: &Event) {
            *self.count.lock().unwrap() += 1;
        }
    }

    #[test]
    fn forwards_to_downstream() {
        let count = Arc::new(Mutex::new(0usize));
        let rec = CrashRecorder::with_downstream(Arc::new(CountingSink {
            count: Arc::clone(&count),
        }));
        rec.emit(&Event::TabCreated { tab_id: TabId(1) });
        rec.emit(&Event::TabCreated { tab_id: TabId(2) });
        assert_eq!(*count.lock().unwrap(), 2);
        // И при этом сам тоже записал.
        assert_eq!(rec.recent_events().len(), 2);
    }

    #[test]
    fn format_dump_contains_markers_and_events() {
        let events = vec![
            "[+0 ms] #1 TabCreated { tab_id: TabId(1) }".to_owned(),
            "[+5 ms] #2 Navigation { .. }".to_owned(),
        ];
        let dump = format_crash_dump(&events, "boom (at src/x.rs:10:5)");
        assert!(dump.contains("=== Lumen crash dump ==="));
        assert!(dump.contains("panic: boom (at src/x.rs:10:5)"));
        assert!(dump.contains("last 2 event(s)"));
        assert!(dump.contains("#1 TabCreated"));
        assert!(dump.contains("#2 Navigation"));
        assert!(dump.contains("=== end of crash dump ==="));
    }

    #[test]
    fn format_dump_handles_no_events() {
        let dump = format_crash_dump(&[], "kaboom");
        assert!(dump.contains("(no events recorded)"));
        assert!(dump.contains("last 0 event(s)"));
    }

    #[test]
    fn write_dump_creates_file_with_contents() {
        let dir = std::env::temp_dir().join(format!(
            "lumen-crash-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let contents = "=== Lumen crash dump ===\npanic: test\n=== end of crash dump ===\n";
        let path = write_crash_dump(&dir, contents).unwrap();
        assert!(path.exists());
        let read = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read, contents);
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("lumen-crash-"));
        assert!(name.ends_with(".log"));
        // Уборка.
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn end_to_end_recorder_to_dump() {
        let rec = CrashRecorder::new();
        rec.emit(&Event::TabCreated { tab_id: TabId(7) });
        rec.emit(&nav(7));
        let dump = format_crash_dump(&rec.recent_events(), "simulated panic");
        assert!(dump.contains("panic: simulated panic"));
        assert!(dump.contains("TabCreated { tab_id: TabId(7) }"));
        assert!(dump.contains("Navigation"));
    }
}
