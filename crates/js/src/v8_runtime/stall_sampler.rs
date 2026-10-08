//! Сэмплер зависаний JS-потока (BUG-935 срез 53).
//!
//! Движковый поток под живой страницей иногда сутками «занят» одной JS-задачей
//! (`[engine] task 19218ms … about_to_wait.rs:280`), и по логу шелла видно лишь
//! точку входа — `tick_timers`/`pump_*`, — но не то, *какой скрипт* крутится.
//! Отладчика в среде нет (BUG-935 срез 3), поэтому стек снимает сам V8.
//!
//! Включается `LUMEN_JS_STALL_SAMPLE_MS=<N>`: сторожевой поток раз в 50 мс
//! смотрит, сколько уже идёт текущая `V8Command::Run`, и если дольше N мс —
//! просит изолят о прерывании (`request_interrupt`). Колбэк выполняется
//! **на JS-потоке посреди работающего скрипта**, снимает
//! `StackTrace::current_stack_trace` и печатает его в stderr. Повтор — раз в N мс
//! на ту же задачу, поэтому долгая задача даёт серию снимков.
//!
//! Без переменной не создаётся ни поток, ни счётчик — поведение не меняется.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Сколько кадров стека печатать на снимок.
const FRAME_LIMIT: usize = 12;

/// Порог зависания из `LUMEN_JS_STALL_SAMPLE_MS`; `None` — сэмплер выключен.
fn threshold_ms() -> Option<u64> {
    static T: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *T.get_or_init(|| {
        std::env::var("LUMEN_JS_STALL_SAMPLE_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&n| n > 0)
    })
}

thread_local! {
    /// Контекст страницы для колбэка прерывания (он идёт на этом же потоке).
    static SAMPLED_CONTEXT: std::cell::RefCell<Option<v8::Global<v8::Context>>> =
        const { std::cell::RefCell::new(None) };
}

/// Разделяемое состояние «идёт ли задача и с какого момента».
pub(super) struct StallSampler {
    /// Миллисекунды от `epoch` до начала текущей задачи, `0` — поток свободен.
    busy_since_ms: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    epoch: Instant,
}

impl StallSampler {
    /// `None`, если сэмплер выключен переменной окружения.
    pub(super) fn start(
        isolate: &mut v8::OwnedIsolate,
        context: &v8::Global<v8::Context>,
    ) -> Option<Self> {
        let threshold = threshold_ms()?;
        // Клон `Global` — до спавна: на этом потоке, пока изолят доступен.
        let own = context.clone();
        SAMPLED_CONTEXT.with(|c| *c.borrow_mut() = Some(own));
        let handle = isolate.thread_safe_handle();
        let busy_since_ms = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let epoch = Instant::now();
        let (busy, stop_flag) = (busy_since_ms.clone(), stop.clone());
        let spawned = std::thread::Builder::new()
            .name("js-stall-sampler".into())
            .spawn(move || {
                // Номер задачи, на которой уже стреляли, и когда — чтобы
                // повторять снимок раз в `threshold`, а не на каждый опрос.
                let mut last_fired: Option<(u64, u64)> = None;
                while !stop_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                    let since = busy.load(Ordering::Acquire);
                    if since == 0 {
                        last_fired = None;
                        continue;
                    }
                    let now = epoch.elapsed().as_millis() as u64;
                    let running = now.saturating_sub(since);
                    let due = match last_fired {
                        Some((task, at)) if task == since => now.saturating_sub(at) >= threshold,
                        _ => running >= threshold,
                    };
                    if !due {
                        continue;
                    }
                    last_fired = Some((since, now));
                    // Длительность — через `data` (по значению, без выделений).
                    let data = running as usize as *mut c_void;
                    if !handle.request_interrupt(interrupt_cb, data) {
                        break; // изолят уничтожен
                    }
                }
            });
        spawned.ok()?;
        Some(Self { busy_since_ms, stop, epoch })
    }

    /// Вызывается перед задачей на JS-потоке.
    pub(super) fn job_started(&self) {
        // `+ 1`: ноль зарезервирован под «свободен».
        let ms = self.epoch.elapsed().as_millis() as u64 + 1;
        self.busy_since_ms.store(ms, Ordering::Release);
    }

    /// Вызывается после задачи на JS-потоке.
    pub(super) fn job_finished(&self) {
        self.busy_since_ms.store(0, Ordering::Release);
    }
}

impl Drop for StallSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Колбэк прерывания: исполняется на JS-потоке внутри работающего скрипта.
unsafe extern "C" fn interrupt_cb(isolate: v8::UnsafeRawIsolatePtr, data: *mut c_void) {
    let running_ms = data as usize;
    let mut isolate = isolate;
    // SAFETY: V8 зовёт колбэк на потоке, владеющем изолятом, пока тот жив.
    let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut isolate) };
    v8::scope!(let scope, isolate);
    // Стек не зависит от контекста, но API кадров требует `HandleScope<Context>`.
    let Some(ctx) = SAMPLED_CONTEXT.with(|c| c.borrow().as_ref().map(|g| v8::Local::new(scope, g)))
    else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, ctx);
    let Some(trace) = v8::StackTrace::current_stack_trace(scope, FRAME_LIMIT) else {
        eprintln!("[js-stall] задача идёт {running_ms} мс, стек недоступен");
        return;
    };
    let mut out = format!("[js-stall] задача идёт {running_ms} мс, JS-стек:");
    for i in 0..trace.get_frame_count() {
        let Some(frame) = trace.get_frame(scope, i) else { continue };
        let func = frame
            .get_function_name(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "<anon>".to_string());
        let script = frame
            .get_script_name_or_source_url(scope)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "<no-script>".to_string());
        out.push_str(&format!(
            "\n[js-stall]   #{i} {func} ({script}:{}:{})",
            frame.get_line_number(),
            frame.get_column()
        ));
    }
    eprintln!("{out}");
}
