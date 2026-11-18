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
    /// Уже сброшенные в дамп script_id — один файл на скрипт за жизнь изолята.
    static DUMPED_SCRIPTS: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Каталог для дампа исходников скриптов зависших задач (BUG-1826: без
/// исходника строка `foo (orchestrate/chl_page:1:234567)` обфусцированного
/// блоба нечитаема). `None` — дамп выключен, сэмплер печатает только стек.
fn dump_dir() -> Option<&'static str> {
    static D: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    D.get_or_init(|| std::env::var("LUMEN_STALL_DUMP_DIR").ok().filter(|s| !s.is_empty()))
        .as_deref()
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
    // BUG-1826: динамика кучи между пробами отличает «VM считает» от «VM давится
    // GC»: рост used между двумя пробами на 2 с = скорость аллокаций, а время в GC
    // V8 засчитывает тому JS-кадру, из которого аллоцировал. До scope!: иначе
    // второй мутабельный заём изолята.
    let heap = isolate.get_heap_statistics();
    v8::scope!(let scope, isolate);
    // Стек не зависит от контекста, но API кадров требует `HandleScope<Context>`.
    let Some(ctx) = SAMPLED_CONTEXT.with(|c| c.borrow().as_ref().map(|g| v8::Local::new(scope, g)))
    else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, ctx);
    let Some(trace) = v8::StackTrace::current_stack_trace(scope, FRAME_LIMIT) else {
        eprintln!(
            "[js-stall] задача идёт {running_ms} мс, стек недоступен (куча used={} total={} malloced={})",
            heap.used_heap_size(),
            heap.total_heap_size(),
            heap.malloced_memory(),
        );
        return;
    };
    let mut out = format!(
        "[js-stall] задача идёт {running_ms} мс, куча used={} total={} external={} malloced={}, JS-стек:",
        heap.used_heap_size(),
        heap.total_heap_size(),
        heap.external_memory(),
        heap.malloced_memory(),
    );
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
        // BUG-1826: исходник горячего (верхнего) кадра — один файл на скрипт.
        if i == 0
            && let Some(dir) = dump_dir()
            && let Some(src) = frame.get_script_source(scope)
        {
            let sid = frame.get_script_id();
            if DUMPED_SCRIPTS.with(|d| d.borrow_mut().insert(sid)) {
                let text = src.to_rust_string_lossy(scope);
                let path = std::path::Path::new(dir).join(format!("stall-script-{sid}.js"));
                match std::fs::write(&path, &text) {
                    Ok(()) => out.push_str(&format!(
                        "\n[js-stall]   исходник №{sid} ({} байт) → {}",
                        text.len(),
                        path.display()
                    )),
                    Err(e) => out.push_str(&format!("\n[js-stall]   дамп №{sid} не удался: {e}")),
                }
            }
        }
        // BUG-1826: счётчики preload-скрипта (BiDi `script.addPreloadScript`) —
        // какие API VM дёргает в реальном времени. Нет глобала — тихо.
        let probe_name = v8::String::new(scope, "__lumenStallProbe").unwrap();
        if let Some(val) = ctx.global(scope).get(scope, probe_name.into())
            && let Ok(obj) = val.try_cast::<v8::Object>()
        {
            if let Some(keys) = obj.get_own_property_names(scope, Default::default()) {
                for i in 0..keys.length() {
                    let Some(key) = keys.get_index(scope, i) else { continue };
                    let Some(key_str) = key.to_string(scope) else { continue };
                    if let Some(v) = obj.get(scope, key.into())
                        && let Ok(n) = v.try_cast::<v8::Number>()
                    {
                        out.push_str(&format!(
                            "\n[js-stall]   {}={}",
                            key_str.to_rust_string_lossy(scope),
                            n.value() as u64
                        ));
                    }
                }
            }
        }
    }
    // BUG-1826: снимок окружения — то, что читает фингерпринт challenge, снятый
    // изнутри зависшей задачи. Гейт `LUMEN_STALL_SNAPSHOT=1`; без него тишина.
    if snapshot_enabled() {
        let src = v8::String::new(scope, SNAPSHOT_JS).unwrap();
        if let Some(script) = v8::Script::compile(scope, src, None)
            && let Some(v) = script.run(scope)
            && let Some(s) = v.to_string(scope)
        {
            out.push_str(&format!("\n[js-stall]   env: {}", s.to_rust_string_lossy(scope)));
        } else {
            out.push_str("\n[js-stall]   env: <снимок не считался>");
        }
    }
    eprintln!("{out}");
}

/// Тело снимка: только чтения (новые canvas/объекты локальные), результаты в
/// JSON-строке. `pn` — длительность 100k `Math.sqrt` в мс performance.now,
/// тот же замер ложится на Chrome-плечо через CDP.
const SNAPSHOT_JS: &str = r#"(() => {
  const j = (x) => { try { return JSON.stringify(x); } catch (e) { return '"ERR"'; } };
  const r = {};
  r.ua = navigator.userAgent;
  r.plat = navigator.platform;
  r.cores = navigator.hardwareConcurrency;
  r.mem = navigator.deviceMemory;
  r.langs = j(navigator.languages);
  r.tz = Intl.DateTimeFormat().resolvedOptions().timeZone;
  r.tzo = new Date().getTimezoneOffset();
  r.sw = screen.width; r.sh = screen.height; r.cd = screen.colorDepth;
  r.dpr = devicePixelRatio; r.iw = innerWidth; r.ih = innerHeight;
  r.hidden = document.hidden; r.vis = document.visibilityState;
  let t0 = performance.now();
  let x = 0;
  for (let i = 0; i < 100000; i++) x += Math.sqrt(i);
  r.pn = Math.round((performance.now() - t0) * 100) / 100;
  r.prec = (() => {
    const d = new Set();
    let a = performance.now();
    for (let i = 0; i < 200; i++) { const b = performance.now(); d.add(Math.round((b - a) * 1e4)); a = b; }
    return Array.from(d).slice(0, 8).join(',');
  })();
  const u8 = new Uint8Array(8);
  crypto.getRandomValues(u8);
  r.rng = j(Array.from(u8));
  try {
    const c = document.createElement('canvas');
    c.width = 50; c.height = 20;
    const g = c.getContext('2d');
    g.fillText('x', 1, 10);
    r.cvs = c.toDataURL().length;
  } catch (e) { r.cvs = 'ERR:' + e.message; }
  try {
    const c2 = document.createElement('canvas');
    const g2 = c2.getContext('webgl');
    r.gl = g2 ? String(g2.getParameter(37446)) + '|' + String(g2.getParameter(7936)) : 'none';
  } catch (e) { r.gl = 'ERR:' + e.message; }
  // BUG-1826: challenge ждёт Resource-Timing записи о собственной загрузке
  // (getEntries/entryTypes/transferSize в его строках) — пустой буфер = вечное
  // ожидание.
  r.res = performance.getEntries
    ? performance.getEntries().map(e => (e.entryType || '?') + ':' + String(e.name || '').slice(-34)).slice(0, 12).join('|')
    : 'none';
  r.opt = window._cf_chl_opt ? Object.keys(window._cf_chl_opt).length : -1;
  return JSON.stringify(r);
})()"#;

/// `LUMEN_STALL_SNAPSHOT=1` включает печать [`SNAPSHOT_JS`] каждой пробой.
fn snapshot_enabled() -> bool {
    static S: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *S.get_or_init(|| std::env::var("LUMEN_STALL_SNAPSHOT").is_ok_and(|v| v == "1"))
}
