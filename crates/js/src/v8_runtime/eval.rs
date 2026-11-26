//! Реализация трейта [`lumen_core::JsRuntime`] для [`V8JsRuntime`] и
//! исполнение верхнеуровневых скриптов с отчётом о непойманном исключении
//! (BUG-591/BUG-813): `eval_and_report*`, `eval_module*_and_report*`.
//!
//! Вынесено из `v8_runtime.rs` батчем SPLIT-JS5. Вместе с impl'ами сюда уехали
//! два локальных макроса, `with_tc!` и `report_exception_via!`: у обоих нет
//! вызывателей за пределами этого файла, а `macro_rules!` виден только ниже
//! своего объявления по тексту — поэтому порядок «макрос, затем его площадки»
//! сохранён ровно исходный. Третий, `compile_cached!`, остался при своём кэше
//! в [`super::code_cache`] и берётся отсюда по пути.

use super::*;
// Кэш байт-кода живёт в соседнем модуле; макрос и имена, которые он
// подставляет в точке раскрытия, берутся по пути.
use super::code_cache::{
    CODE_CACHE, CODE_CACHE_MAX_ENTRIES, CODE_CACHE_MIN_LEN, code_cache_hash, compile_cached,
};

/// Shared scope-setup boilerplate: create pinned HandleScope + ContextScope +
/// pinned TryCatch, then call the provided closure with the TryCatch ref.
///
/// The macro-heavy setup hides the three-step scope dance required by rusty_v8
/// v150 (scope! → ContextScope → tc_scope!) and avoids duplicating it across
/// eval/set_global/get_global/call_function.
macro_rules! with_tc {
    ($inner:expr, |$tc:ident, $ctx:ident| $body:expr) => {{
        // Disjoint field borrows: scope borrows isolate mutably, context immutably.
        let isolate = &mut $inner.isolate;
        let context_global = &$inner.context;
        // scope! pins the HandleScope; scope: &mut PinnedRef<HandleScope<'_, ()>>
        v8::scope!(let scope, isolate);
        // Local<'_, Context> — Copy, usable after ContextScope is created
        let $ctx = v8::Local::new(scope, context_global);
        // ContextScope enters the context; scope: &mut ContextScope<…, HandleScope<…>>
        let scope = &mut v8::ContextScope::new(scope, $ctx);
        // tc_scope! pins TryCatch; $tc: &mut PinnedRef<TryCatch<…, HandleScope<…>>>
        v8::tc_scope!($tc, scope);
        $body
    }};
}

/// Тело `eval` без диспетчеризации: компиляция и исполнение скрипта в текущем
/// контексте. Общая для блокирующего [`JsRuntime::eval`] и
/// [`V8JsRuntime::eval_detached`].
#[allow(clippy::unwrap_used)] // tc.exception().unwrap() — унаследовано, docs/lint-policy.md §10
pub(super) fn eval_script(inner: &mut V8Inner, script: &str) -> JsResult<JsValue> {
    // `tc.exception().unwrap()` — унаследовано от прежнего тела `eval`
    // (docs/lint-policy.md §10); атрибут на функции: внутри макроса allow не применить.
    with_tc!(inner, |tc, ctx| {
        // BUG-753 срез 2: internal evals see the internal container.
        let wrapped = crate::internal_globals::wrap_for_container(tc, ctx, script);
        let script = wrapped.as_deref().unwrap_or(script);
        let src = v8::String::new(tc, script)
            .ok_or_else(|| JsError::Runtime("OOM: script string".into()))?;

        let compiled = compile_cached!(tc, script, src);
        if tc.has_caught() {
            let exc = tc.exception().unwrap();
            return Err(v8_err(tc, exc));
        }
        let compiled = compiled
            .ok_or_else(|| JsError::Runtime("script compile returned None".into()))?;

        // PERF-JS-PROF: тот же скоуп, что у eval_and_report_via — eval() —
        // основной путь диспетчера колбэков (тысячи вызовов на загрузку),
        // без него время исполнения скрипта было невидимо.
        let _run_prof = lumen_core::profile::scope("js.run");
        let result = compiled.run(tc);
        drop(_run_prof);
        if tc.has_caught() {
            let exc = tc.exception().unwrap();
            return Err(v8_err(tc, exc));
        }
        match result {
            Some(val) => from_v8(tc, val),
            None => Err(JsError::Runtime("script returned no value".into())),
        }
    })
}

impl JsRuntime for V8JsRuntime {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn eval(&self, script: &str) -> JsResult<JsValue> {
        self.run(|inner| eval_script(inner, script))
    }

    /// Evaluate `source` as an ES module (HTML LS §8.1.3 `<script type=module>`).
    ///
    /// S12b-23: replaces the trait default (which ran module source through
    /// classic `eval` and choked on `export`/`import` — BUG-350). Machinery
    /// lives in [`crate::v8_esm`]; this method only bridges the `TryCatch`.
    fn eval_module(&self, source: &str) -> JsResult<()> {
        // Phase 0 decorator transform, as on the QuickJS path: the transformer
        // is a plain JS global, so it needs no raw scope access.
        let source = crate::decorators::maybe_transform_decorators_v8(self, source)
            .unwrap_or_else(|| source.to_owned());
        self.run(|inner| {
            with_tc!(inner, |tc, _ctx| {
                match crate::v8_esm::evaluate_entry_module(tc, &source) {
                    Ok(()) => Ok(()),
                    Err(_failure) => match tc.exception() {
                        Some(exc) => Err(v8_err(tc, exc)),
                        // V8 returned an empty handle without throwing (OOM on a
                        // string allocation, termination): still an error, but
                        // there is no exception to describe it.
                        None => Err(JsError::Runtime("module eval failed".into())),
                    },
                }
            })
        })
    }

    /// Pre-register an ES module `source` under its resolved `specifier` so
    /// other modules can `import` it. Mirrors
    /// [`crate::QuickJsRuntime::register_module_source`].
    fn register_module_source(&self, specifier: &str, source: &str) {
        self.run(|_inner| crate::v8_esm::register_source(specifier, source));
    }

    fn eval_module_at(&self, url: &str, source: &str) -> JsResult<()> {
        let source = crate::decorators::maybe_transform_decorators_v8(self, source)
            .unwrap_or_else(|| source.to_owned());
        let url = url.to_owned();
        self.run(move |inner| {
            with_tc!(inner, |tc, _ctx| {
                match crate::v8_esm::evaluate_module_url(tc, &url, &source) {
                    Ok(()) => Ok(()),
                    Err(_failure) => match tc.exception() {
                        Some(exc) => Err(v8_err(tc, exc)),
                        None => Err(JsError::Runtime("module eval failed".into())),
                    },
                }
            })
        })
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn set_global(&self, name: &str, value: JsValue) -> JsResult<()> {
        self.run(|inner| {
            with_tc!(inner, |tc, ctx| {
                let key = v8::String::new(tc, name)
                    .ok_or_else(|| JsError::Runtime(format!("OOM: key '{name}'")))?;
                let val = to_v8(tc, value)?;
                // ctx is Local<Context> (Copy); use it to obtain the global object.
                let global = crate::internal_globals::holder_for(tc, ctx, name);
                global.set(tc, key.into(), val);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                Ok(())
            })
        })
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn get_global(&self, name: &str) -> JsResult<JsValue> {
        self.run(|inner| {
            with_tc!(inner, |tc, ctx| {
                let key = v8::String::new(tc, name)
                    .ok_or_else(|| JsError::Runtime(format!("OOM: key '{name}'")))?;
                let global = crate::internal_globals::holder_for(tc, ctx, name);
                let val = global
                    .get(tc, key.into())
                    .ok_or_else(|| JsError::Runtime(format!("global '{name}' not found")))?;
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                from_v8(tc, val)
            })
        })
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn call_function(&self, name: &str, args: &[JsValue]) -> JsResult<JsValue> {
        self.run(|inner| {
            with_tc!(inner, |tc, ctx| {
                let key = v8::String::new(tc, name)
                    .ok_or_else(|| JsError::Runtime(format!("OOM: function '{name}'")))?;
                let global = crate::internal_globals::holder_for(tc, ctx, name);
                let func_val = global
                    .get(tc, key.into())
                    .ok_or_else(|| JsError::Runtime(format!("'{name}' not found in globals")))?;
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let func: v8::Local<v8::Function> = func_val
                    .try_into()
                    .map_err(|_| JsError::Runtime(format!("'{name}' is not a function")))?;
                let mut v8_args: Vec<v8::Local<v8::Value>> = Vec::with_capacity(args.len());
                for a in args.iter().cloned() {
                    v8_args.push(to_v8(tc, a)?);
                }
                let recv = v8::undefined(tc).into();
                // PERF-JS-PROF: вход в JS-колбэк (rAF/таймер/событие через
                // `_lumen_*`-диспетчер) — внутри js.job, но вне js.run.
                let _call_prof = lumen_core::profile::scope("js.call");
                let result = func.call(tc, recv, &v8_args);
                drop(_call_prof);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                match result {
                    Some(val) => from_v8(tc, val),
                    None => Ok(JsValue::Null),
                }
            })
        })
    }

    fn engine_name(&self) -> &'static str {
        "v8"
    }

    fn suspend(&mut self) -> JsResult<SuspendedHeap> {
        let raw = self.run(|inner| -> Vec<u8> {
            let baseline = inner.baseline_globals.clone();
            with_tc!(inner, |tc, ctx| {
                let global = ctx.global(tc);
                let Some(own_props) = global.get_own_property_names(tc, Default::default())
                else {
                    return Vec::new();
                };
                // Structured-clone each candidate value in isolation first, so a
                // non-cloneable one (a `Function`, mainly — F1 in the migration
                // brief) is dropped instead of poisoning the whole capture.
                let wrapper = v8::Object::new(tc);
                let mut has_any = false;
                for i in 0..own_props.length() {
                    let Some(key) = own_props.get_index(tc, i) else {
                        continue;
                    };
                    let Some(key_str) = key.to_string(tc) else {
                        continue;
                    };
                    let key_str = key_str.to_rust_string_lossy(tc);
                    if baseline.contains(&key_str) {
                        continue;
                    }
                    let Some(val) = global.get(tc, key) else {
                        if tc.has_caught() {
                            tc.reset();
                        }
                        continue;
                    };
                    if tc.has_caught() {
                        // A getter on this global threw (e.g. a native binding
                        // with side effects) — skip it, don't poison later keys.
                        tc.reset();
                        continue;
                    }
                    let probe = v8::ValueSerializer::new(tc, Box::new(LumenValueSerializerImpl));
                    probe.write_header();
                    let wrote = probe.write_value(ctx, val);
                    if tc.has_caught() {
                        tc.reset();
                        continue;
                    }
                    if wrote == Some(true) {
                        wrapper.set(tc, key, val);
                        has_any = true;
                    }
                }
                if !has_any {
                    return Vec::new();
                }
                // Every value in `wrapper` already round-tripped individually
                // above, so this final pass is expected to succeed; handle
                // failure defensively rather than assume it.
                let serializer = v8::ValueSerializer::new(tc, Box::new(LumenValueSerializerImpl));
                serializer.write_header();
                let wrote = serializer.write_value(ctx, wrapper.into());
                if tc.has_caught() {
                    tc.reset();
                    return Vec::new();
                }
                match wrote {
                    Some(true) => serializer.release(),
                    _ => Vec::new(),
                }
            })
        });
        match heap_snapshot::compress_heap(&raw) {
            Ok(heap) => Ok(heap),
            // Over the per-tab cap: skip heap persistence, same policy as the
            // QuickJS backend (`QuickJsRuntime::suspend`) — never block
            // hibernation on a large heap.
            Err(heap_snapshot::HeapSnapshotError::TooLarge { .. }) => Ok(SuspendedHeap::default()),
            Err(e) => Err(JsError::Runtime(e.to_string())),
        }
    }

    fn resume(snapshot: SuspendedHeap) -> JsResult<Self> {
        let raw = heap_snapshot::decompress_heap(&snapshot)
            .map_err(|e| JsError::Runtime(e.to_string()))?;
        let rt = Self::new()?;
        if raw.is_empty() {
            return Ok(rt);
        }
        rt.run(|inner| -> JsResult<()> {
            with_tc!(inner, |tc, ctx| {
                let deserializer =
                    v8::ValueDeserializer::new(tc, Box::new(LumenValueDeserializerImpl), &raw);
                if deserializer.read_header(ctx) != Some(true) {
                    return Err(JsError::Runtime("suspended heap: corrupt header".into()));
                }
                let value = deserializer.read_value(ctx).ok_or_else(|| {
                    JsError::Runtime("suspended heap: failed to deserialize".into())
                })?;
                let Ok(obj) = v8::Local::<v8::Object>::try_from(value) else {
                    // Nothing (or a non-object root) was captured — nothing to restore.
                    return Ok(());
                };
                let own_props = obj.get_own_property_names(tc, Default::default()).ok_or_else(
                    || JsError::Runtime("suspended heap: get_own_property_names failed".into()),
                )?;
                let global = ctx.global(tc);
                for i in 0..own_props.length() {
                    let Some(key) = own_props.get_index(tc, i) else {
                        continue;
                    };
                    let Some(val) = obj.get(tc, key) else {
                        continue;
                    };
                    global.set(tc, key, val);
                }
                Ok(())
            })
        })?;
        Ok(rt)
    }
}

// ── Top-level script execution with uncaught-exception reporting (BUG-591) ──

/// Best-effort-extract a filename/line/column from `$tc`'s `v8::Message` and
/// call the global `$reporter(exc, filename, lineno, colno)` — the shim's
/// `_lumen_report_exception` for a page script, `_lumen_report_worker_exception`
/// for a worker's own top-level script (BUG-813). A free-standing macro rather
/// than a function because the `TryCatch`/`HandleScope` type in `$tc` is
/// lifetime-parameterized per call site (`with_tc!`'s expansion).
///
/// A missing global is not an error: a bare runtime (unit test, `--dump-*`)
/// carries no shim at all, and the exception still reaches the caller as the
/// returned `Err`.
macro_rules! report_exception_via {
    ($tc:expr, $exc:expr, $reporter:expr) => {{
        let message = $tc.message();
        let (filename, lineno, colno) = message
            .map(|m| {
                let filename = m
                    .get_script_resource_name($tc)
                    .and_then(|v| v.to_string($tc))
                    .map(|s| s.to_rust_string_lossy($tc))
                    .unwrap_or_default();
                let lineno = m.get_line_number($tc).unwrap_or(0) as i32;
                let colno = m.get_start_column() as i32;
                (filename, lineno, colno)
            })
            .unwrap_or_default();
        {
            v8::tc_scope!(rtc, $tc);
            let ctx = rtc.get_current_context();
            let global = ctx.global(rtc);
            let holder = crate::internal_globals::holder_for(rtc, ctx, $reporter);
            if let Some(key) = v8::String::new(rtc, $reporter)
                && let Some(report_fn) = holder
                    .get(rtc, key.into())
                    .and_then(|v| v8::Local::<v8::Function>::try_from(v).ok())
                && let Some(filename_v) = v8::String::new(rtc, &filename)
            {
                let lineno_v = v8::Integer::new(rtc, lineno);
                let colno_v = v8::Integer::new(rtc, colno);
                let _ = report_fn.call(
                    rtc,
                    global.into(),
                    &[$exc, filename_v.into(), lineno_v.into(), colno_v.into()],
                );
            }
        }
    }};
}

impl V8JsRuntime {
    /// COMPAT-HANG этап 2: fire-and-forget `eval` — задача уходит в очередь
    /// JS-потока, вызывающий НЕ ждёт её окончания. Для помп фоновых вкладок и
    /// фреймов: их таймерные колбэки исполняются в чужом изоляте, и
    /// блокирующий `eval` с зависшим скриптом навсегда замораживал бы
    /// UI-поток в `rx.recv()` (плашка показывать некому — цикл событий стоит).
    ///
    /// Ответ не посылается вовсе, поэтому `sync_channel`-очередь не блокирует
    /// отправителя (`try_send`); переполнение очереди за зависшим рантаймом
    /// отсечено гейтом `stall_handle().is_busy()` на вызывающей стороне.
    /// Окончание задачи будит цикл событий оболочки
    /// ([`lumen_core::script_stall::wake_shell`]), чтобы отложенные чтения
    /// (`take_timer_wakeup` и др.) увидели свежее состояние на следующем пассе.
    pub fn eval_detached(&self, script: String) {
        let job = move |inner: &mut V8Inner| {
            let _ = eval_script(inner, &script);
            lumen_core::script_stall::wake_shell();
        };
        let _ = self.cmd_tx.try_send(V8Command::Run(Box::new(job)));
    }

    /// Evaluate a classic top-level `<script>` body exactly like
    /// [`JsRuntime::eval`], except that an uncaught exception (compile *or*
    /// runtime) is additionally reported through the shim's window `error`
    /// pipeline (HTML LS §8.1.3.6 "report the exception") before the error is
    /// returned to the caller as before.
    ///
    /// Only the genuine top-level page-script boundary
    /// (`crates/shell/src/main.rs`'s classic-script loop) should call this —
    /// plain [`JsRuntime::eval`] stays the right choice for every internal
    /// helper eval (shim bootstrap, feature probes, deterministic-mode
    /// overrides, cookie-banner triggers, …), whose caught error is not a
    /// page-visible event and must not become one.
    ///
    /// `v8::Message` (populated by V8 for both compile and runtime errors)
    /// gives a structured script name/line/column here, which is why this
    /// goes through Rust rather than reusing the JS-side
    /// `_lumen_report_exception` fallback that timers/rAF/queueMicrotask use
    /// — those only ever see the exception value after V8 has already
    /// discarded the `Message`, so they best-effort-parse `Error.stack`
    /// instead (`crate::dom::WEB_API_SHIM`).
    pub fn eval_and_report(&self, script: &str) -> JsResult<JsValue> {
        self.eval_and_report_via(script, "_lumen_report_exception")
    }

    /// [`Self::eval_and_report`] with the reporting function named explicitly.
    ///
    /// A `WorkerGlobalScope` has no `window` and no `_lumen_report_exception`;
    /// its own "report the exception" entry point is
    /// `_lumen_report_worker_exception` (`crate::worker_shims::worker_global_shim`),
    /// which fires `error` at the worker scope first and only forwards to the
    /// owning `Worker` object on the page if nothing cancelled it (BUG-813).
    /// That is the only reason this name is a parameter — page code must keep
    /// going through [`Self::eval_and_report`].
    #[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
    pub fn eval_and_report_via(&self, script: &str, reporter: &str) -> JsResult<JsValue> {
        let reporter = reporter.to_owned();
        self.run(move |inner| {
            with_tc!(inner, |tc, _ctx| {
                let src = v8::String::new(tc, script)
                    .ok_or_else(|| JsError::Runtime("OOM: script string".into()))?;

                let compiled = compile_cached!(tc, script, src);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    report_exception_via!(tc, exc, reporter.as_str());
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("script compile returned None".into()))?;

                let _run_prof = lumen_core::profile::scope("js.run");
                let result = compiled.run(tc);
                drop(_run_prof);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    report_exception_via!(tc, exc, reporter.as_str());
                    return Err(v8_err(tc, exc));
                }
                match result {
                    // THREAD-3 slice 8 (BUG-1034): a top-level classic script's
                    // completion value is the value of its last expression
                    // statement, which for an idiom like
                    // `self.globalThis = self.globalThis || self` is the
                    // global object itself. Every real caller of this method
                    // discards the `Ok` payload (`scripts.rs`'s `Ok(_) => {}`,
                    // both worker call sites' `.map(|_| ())`) — the *only*
                    // reason it is not always `JsValue::Undefined` is that
                    // `eval_and_report_matches_eval_on_success` pins it to
                    // round-trip primitives identically to plain `eval()`.
                    // `from_v8` has no total-size bound, only a per-path depth
                    // cap (`FROM_V8_MAX_DEPTH`) — walking `window`/`document`
                    // this way revisits the same shared DOM subgraphs through
                    // every distinct path to them, and on a real page that is
                    // combinatorially large, which is what produced this
                    // hang's continuous GC/allocation storm (confirmed live
                    // on duckduckgo.com: `compiled.run` itself returns in
                    // microseconds, the hang is entirely in this conversion).
                    // An object/array completion is discarded by every
                    // consumer anyway, so it is not walked at all here.
                    Some(val) if val.is_object() || val.is_array() => Ok(JsValue::Undefined),
                    Some(val) => from_v8(tc, val),
                    None => Err(JsError::Runtime("script returned no value".into())),
                }
            })
        })
    }

    /// [`Self::eval_and_report_via`], except the reporter is called **only**
    /// for a runtime failure (the script compiled, then threw while running),
    /// never for a compile/parse failure — the module counterpart
    /// ([`Self::eval_module_at_and_report_via`]) already has this split via
    /// `ModuleFailure::{Load,Runtime}`; a classic script has no such enum from
    /// V8, so the two `has_caught()` checks below are it.
    ///
    /// A shared worker's top-level script is the one caller that needs this
    /// (BUG-905): a parse failure never entered the worker's own global scope,
    /// so there is no "the scope's own `onerror` may cancel this" step to run
    /// — HTML LS routes a script-fetch/parse failure straight to a plain
    /// `error` `Event` on every owning `SharedWorker`, which the caller builds
    /// itself from the `Err` this returns (mirroring how it already handles a
    /// module *load* failure). A *runtime* failure, by contrast, does reach
    /// the worker's own scope, so the reporter still runs for it exactly as
    /// in [`Self::eval_and_report_via`].
    #[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
    pub fn eval_and_report_via_runtime_only(&self, script: &str, reporter: &str) -> JsResult<JsValue> {
        let reporter = reporter.to_owned();
        self.run(move |inner| {
            with_tc!(inner, |tc, _ctx| {
                let src = v8::String::new(tc, script)
                    .ok_or_else(|| JsError::Runtime("OOM: script string".into()))?;

                let compiled = compile_cached!(tc, script, src);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("script compile returned None".into()))?;

                let _run_prof = lumen_core::profile::scope("js.run");
                let result = compiled.run(tc);
                drop(_run_prof);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    report_exception_via!(tc, exc, reporter.as_str());
                    return Err(v8_err(tc, exc));
                }
                match result {
                    Some(val) if val.is_object() || val.is_array() => Ok(JsValue::Undefined),
                    Some(val) => from_v8(tc, val),
                    None => Err(JsError::Runtime("script returned no value".into())),
                }
            })
        })
    }

    /// Evaluate `source` as the entry ES module of a top-level page load
    /// ([`crate::v8_esm::evaluate_entry_module`]), additionally reporting a
    /// **runtime** failure through the shim's window `error` pipeline — the
    /// same "report the exception" step [`Self::eval_and_report`] performs for
    /// classic scripts (BUG-591). A **load** failure (parse/link/import-not-
    /// found — the module body never started evaluating) is deliberately NOT
    /// reported here: per HTML LS that case belongs to the script element's
    /// own `error` event, not `window.onerror`/`'error'`, and reporting it
    /// through this path would misfire `window.onerror` on an ordinary 404 or
    /// missing import.
    ///
    /// Only the genuine top-level page-script boundary
    /// (`crates/shell/src/main.rs`'s module-script loop) should call this —
    /// [`JsRuntime::eval_module`] stays the right choice everywhere else
    /// (tests, internal helpers), whose caught error must not become a
    /// page-visible event.
    pub fn eval_module_and_report(&self, source: &str) -> JsResult<()> {
        let source = crate::decorators::maybe_transform_decorators_v8(self, source)
            .unwrap_or_else(|| source.to_owned());
        self.run(|inner| {
            with_tc!(inner, |tc, _ctx| {
                match crate::v8_esm::evaluate_entry_module(tc, &source) {
                    Ok(()) => Ok(()),
                    Err(failure) => match tc.exception() {
                        Some(exc) => {
                            if failure == crate::v8_esm::ModuleFailure::Runtime {
                                report_exception_via!(tc, exc, "_lumen_report_exception");
                            }
                            Err(v8_err(tc, exc))
                        }
                        None => Err(JsError::Runtime("module eval failed".into())),
                    },
                }
            })
        })
    }

    /// External-module counterpart of [`Self::eval_module_and_report`], for
    /// `<script type=module src=URL>` ([`crate::v8_esm::evaluate_module_url`]).
    /// Same runtime-only reporting rule.
    pub fn eval_module_at_and_report(&self, url: &str, source: &str) -> JsResult<()> {
        self.eval_module_at_and_report_via(url, source, "_lumen_report_exception")
    }

    /// [`Self::eval_module_at_and_report`] with the reporting function named
    /// explicitly — see [`Self::eval_and_report_via`] for why a worker needs a
    /// different one (BUG-813). The runtime-only rule is unchanged: a module
    /// *load* failure is still not reported here, so a caller that has its own
    /// fallback path for that case (`crate::worker::run_worker_thread_v8`) has
    /// to ask the scope whether the reporter actually ran.
    pub fn eval_module_at_and_report_via(
        &self,
        url: &str,
        source: &str,
        reporter: &str,
    ) -> JsResult<()> {
        let source = crate::decorators::maybe_transform_decorators_v8(self, source)
            .unwrap_or_else(|| source.to_owned());
        let url = url.to_owned();
        let reporter = reporter.to_owned();
        self.run(move |inner| {
            with_tc!(inner, |tc, _ctx| {
                match crate::v8_esm::evaluate_module_url(tc, &url, &source) {
                    Ok(()) => Ok(()),
                    Err(failure) => match tc.exception() {
                        Some(exc) => {
                            if failure == crate::v8_esm::ModuleFailure::Runtime {
                                report_exception_via!(tc, exc, reporter.as_str());
                            }
                            Err(v8_err(tc, exc))
                        }
                        None => Err(JsError::Runtime("module eval failed".into())),
                    },
                }
            })
        })
    }
}

// ── BUG-979: FramePeerBridge — synchronous cross-isolate frame globals ─────

use crate::frame_peer_bridge::{FramePeerBridge, envelope_error, envelope_tag, envelope_value};

/// BUG-1815: `name` в изоляте пира всё ещё указывает на неизменённый встроенный
/// объект ECMAScript (идентичность со снимком, который шим бриджа снял при
/// установке — `_lumen_pristine_intrinsics` в контейнере). Такой объект не
/// пересекает границу изолятов с методами: читатель получает свой одноимённый
/// встроенный (`frame_bridge.rs` `wrapWinFacadeGlobals`, прецедент BUG-1813).
#[cfg(feature = "v8-backend")]
fn names_pristine_intrinsic<'s>(
    scope: &v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Context>,
    name: &str,
    val: v8::Local<'s, v8::Value>,
) -> bool {
    let Some(container) = crate::internal_globals::container(scope, ctx) else {
        return false;
    };
    let Some(key) = v8::String::new(scope, "_lumen_pristine_intrinsics") else {
        return false;
    };
    let Some(map) = container.get(scope, key.into()) else {
        return false;
    };
    let Ok(map) = v8::Local::<v8::Object>::try_from(map) else {
        return false;
    };
    let Some(key) = v8::String::new(scope, name) else {
        return false;
    };
    map.get(scope, key.into())
        .is_some_and(|snapshot| snapshot.strict_equals(val))
}

/// Тело `peer_global_get` без диспетчеризации — исполняется на JS-потоке пира
/// внутри `with_tc!`. Общее для [`V8JsRuntime`] и лёгкого [`V8PeerHandle`]:
/// тело не зависит от того, чьим `cmd_tx` задача доехала.
///
/// `name` может быть dotted-путём (`testlib.addScript`): фасад объектного
/// глобала пира (`frame_bridge.rs` `peerObjectFacade`) читает методы
/// вложенных объектов именно так — сериализованная копия объекта методов не
/// несёт (функции через границу изолятов не ходят), а плоский `globalThis[name]`
/// до вложенного свойства не достаёт.
#[cfg(feature = "v8-backend")]
fn peer_global_get_body(inner: &mut V8Inner, name: &str) -> JsValue {
    with_tc!(inner, |tc, ctx| {
        let mut holder = ctx.global(tc);
        let mut parts = name.split('.').peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let Some(key) = v8::String::new(tc, part) else {
                return envelope_error("OOM: property name");
            };
            let val = holder.get(tc, key.into());
            if tc.has_caught() {
                tc.reset();
                return envelope_tag("absent");
            }
            let Some(val) = val else {
                return envelope_tag("absent");
            };
            if val.is_undefined() {
                return envelope_tag("absent");
            }
            if !last {
                let Ok(next) = v8::Local::<v8::Object>::try_from(val) else {
                    return envelope_tag("absent");
                };
                holder = next;
                continue;
            }
            // BUG-1815: встроенный объект realm'а пира отдаётся не копией,
            // а тегом — читатель подставит свой одноимённый встроенный.
            if val.is_object()
                && !val.is_function()
                && names_pristine_intrinsic(tc, ctx, part, val)
            {
                return envelope_tag("builtin");
            }
            if val.is_function() {
                return envelope_tag("function");
            }
            return match from_v8(tc, val) {
                Ok(v) => envelope_value(v),
                Err(e) => envelope_error(e.to_string()),
            };
        }
        envelope_tag("absent")
    })
}

/// Маркер фасада кадра (`__lumen_frame_facade` + `__nid__`, см.
/// `frame_facade_marker` в `value.rs`), приехавший аргументом
/// `peer_global_call`/`peer_global_set`. `__nid__` индексирует арену
/// ДОКУМЕНТА пира (фасад `top.document.body` нёс nid тела документа
/// адресата), поэтому честная регидратация — `_lumen_make_node` в этом
/// контексте: она оборачивает nid в узел собственного документа. Для
/// фасада чужого документа (не адресата вызова) это неточно — редкий
/// случай, принятый как есть.
#[cfg(feature = "v8-backend")]
fn rehydrate_frame_marker<'s>(
    tc: &v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Context>,
    val: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, ()> {
    fn marker_nid<'s>(
        tc: &v8::PinScope<'s, '_>,
        val: v8::Local<'s, v8::Value>,
    ) -> Option<f64> {
        let obj = v8::Local::<v8::Object>::try_from(val).ok()?;
        let flag = v8::String::new(tc, "__lumen_frame_facade")?;
        let fv = obj.get(tc, flag.into())?;
        if !fv.is_boolean() || !fv.boolean_value(tc) {
            return None;
        }
        let nk = v8::String::new(tc, "__nid__")?;
        let nv = obj.get(tc, nk.into())?;
        if nv.is_number() {
            nv.number_value(tc)
        } else {
            None
        }
    }
    if let Some(nid) = marker_nid(tc, val) {
        // `_lumen_make_node` — локальная функция IIFE-шима, на globalThis её
        // нет; внутренние имена живут акцессорами на контейнере `__lumen_C`
        // (internal_globals.rs, BUG-753). Fallback на global — для
        // минимальных контекстов без контейнера.
        let holder = crate::internal_globals::container(tc, ctx)
            .unwrap_or_else(|| ctx.global(tc));
        let Some(mk) = v8::String::new(tc, "_lumen_make_node") else {
            return Ok(val);
        };
        let Some(mv) = holder.get(tc, mk.into()) else {
            return Ok(val);
        };
        let Ok(f) = v8::Local::<v8::Function>::try_from(mv) else {
            return Ok(val);
        };
        let arg = v8::Number::new(tc, nid);
        // `Local` — Copy: при броске `_lumen_make_node` зовущий сбросит
        // TryCatch и подставит немаркированное `val`.
        return f
            .call(tc, v8::undefined(tc).into(), &[arg.into()])
            .ok_or(());
    }
    // Маркеры могут лежать и внутри массива-аргумента — свежие контейнеры
    // `to_v8` можно править на месте.
    if let Ok(arr) = v8::Local::<v8::Array>::try_from(val) {
        for i in 0..arr.length() {
            if let Some(item) = arr.get_index(tc, i)
                && item.is_object()
            {
                let r = rehydrate_frame_marker(tc, ctx, item)?;
                if !r.strict_equals(item) {
                    arr.set_index(tc, i, r);
                }
            }
        }
    }
    Ok(val)
}

/// Тело `peer_global_call` — см. [`peer_global_get_body`]. Dotted-путь
/// (`testlib.addScript`) резолвится до функции, а `this` вызова — объект-
/// держатель последнего сегмента (как у обычного `obj.method()`); у
/// плоского имени receiver остаётся `undefined`, как раньше.
#[cfg(feature = "v8-backend")]
fn peer_global_call_body(inner: &mut V8Inner, name: &str, args: &[JsValue]) -> JsValue {
    with_tc!(inner, |tc, ctx| {
        let dotted = name.contains('.');
        let mut holder = ctx.global(tc);
        let mut parts = name.split('.').peekable();
        let mut func_val = None;
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let Some(key) = v8::String::new(tc, part) else {
                return envelope_error("OOM: property name");
            };
            let val = holder.get(tc, key.into());
            if tc.has_caught() {
                tc.reset();
                return envelope_error(format!("reading '{name}' threw"));
            }
            let Some(val) = val else {
                return envelope_error(format!("'{name}' not found in globals"));
            };
            if last {
                func_val = Some(val);
                break;
            }
            let Ok(next) = v8::Local::<v8::Object>::try_from(val) else {
                return envelope_error(format!("'{name}' path is not an object"));
            };
            holder = next;
        }
        let Some(func_val) = func_val else {
            return envelope_error(format!("'{name}' not found in globals"));
        };
        let Ok(func) = v8::Local::<v8::Function>::try_from(func_val) else {
            return envelope_error(format!("'{name}' is not a function"));
        };
        let recv: v8::Local<v8::Value> = if dotted {
            holder.into()
        } else {
            v8::undefined(tc).into()
        };
        let mut v8_args: Vec<v8::Local<v8::Value>> = Vec::with_capacity(args.len());
        for a in args.iter().cloned() {
            match to_v8(tc, a) {
                Ok(v) => match rehydrate_frame_marker(tc, ctx, v) {
                    Ok(v) => v8_args.push(v),
                    Err(()) => {
                        tc.reset();
                        v8_args.push(v);
                    }
                },
                Err(e) => return envelope_error(e.to_string()),
            }
        }
        let _call_prof = lumen_core::profile::scope("js.call");
        let result = func.call(tc, recv, &v8_args);
        drop(_call_prof);
        if tc.has_caught() {
            let msg = tc
                .exception()
                .and_then(|exc| exc.to_string(tc))
                .map(|s| s.to_rust_string_lossy(tc))
                .unwrap_or_else(|| "cross-frame call threw".to_owned());
            return envelope_error(msg);
        }
        match result {
            Some(val) => match from_v8(tc, val) {
                Ok(v) => envelope_value(v),
                Err(e) => envelope_error(e.to_string()),
            },
            None => envelope_value(JsValue::Null),
        }
    })
}

/// Тело `peer_global_set` — та же dotted-развёртка, что у get/call: путь
/// ведёт к объекту-держателю, значение пишется в его последнее свойство
/// (`top.include6Loaded = true` из скрипта document.write-кадра,
/// WPT execution-timing 070/071).
#[cfg(feature = "v8-backend")]
fn peer_global_set_body(inner: &mut V8Inner, name: &str, value: JsValue) -> JsValue {
    with_tc!(inner, |tc, ctx| {
        let mut holder = ctx.global(tc);
        let mut parts = name.split('.').peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let Some(key) = v8::String::new(tc, part) else {
                return envelope_error("OOM: property name");
            };
            if last {
                let v = match to_v8(tc, value) {
                    Ok(v) => match rehydrate_frame_marker(tc, ctx, v) {
                        Ok(v) => v,
                        Err(()) => {
                            tc.reset();
                            v
                        }
                    },
                    Err(e) => return envelope_error(e.to_string()),
                };
                if holder.set(tc, key.into(), v).is_none() {
                    return envelope_error(format!("setting '{name}' failed"));
                }
                if tc.has_caught() {
                    let msg = tc
                        .exception()
                        .and_then(|exc| exc.to_string(tc))
                        .map(|s| s.to_rust_string_lossy(tc))
                        .unwrap_or_else(|| "cross-frame set threw".to_owned());
                    tc.reset();
                    return envelope_error(msg);
                }
                return envelope_value(JsValue::Bool(true));
            }
            let val = holder.get(tc, key.into());
            if tc.has_caught() {
                tc.reset();
                return envelope_error(format!("reading '{name}' threw"));
            }
            let val = match val {
                Some(v) => v,
                None => return envelope_error(format!("'{name}' not found in globals")),
            };
            let Ok(next) = v8::Local::<v8::Object>::try_from(val) else {
                return envelope_error(format!("'{name}' path is not an object"));
            };
            holder = next;
        }
        envelope_error(format!("'{name}' not found in globals"))
    })
}

impl FramePeerBridge for V8JsRuntime {
    fn peer_busy(&self) -> bool {
        // Занят И не в peer-ожидании: поток, заблокированный в `run_peer`,
        // pump'ит свои `RunPeer`-команды реентерабельно (`thread::recv_peer`),
        // поэтому «занят» для него — это «исполни и ответь сейчас», а не
        // «подожди неведомо сколько».
        self.stall.is_busy() && self.serving.load(Ordering::Relaxed) == 0
    }

    /// Пир в данный момент ждёт чужой вызов и потому способен обслужить
    /// `RunPeer`-команду реентерабельно — стражу ревхода можно пустить
    /// обратное ребро (см. `frame_peer_bridge::enter_call`).
    fn peer_reentrant_ready(&self) -> bool {
        self.serving.load(Ordering::Relaxed) > 0
    }

    fn peer_global_get(&self, name: &str) -> JsValue {
        let name = name.to_owned();
        self.run_peer(move |inner| peer_global_get_body(inner, &name))
    }

    fn peer_global_call(&self, name: &str, args: &[JsValue]) -> JsValue {
        let name = name.to_owned();
        let args = args.to_vec();
        self.run_peer(move |inner| peer_global_call_body(inner, &name, &args))
    }

    fn peer_global_set(&self, name: &str, value: JsValue) -> JsValue {
        let name = name.to_owned();
        self.run_peer(move |inner| peer_global_set_body(inner, &name, value))
    }
}

/// Лёгкий [`FramePeerBridge`] без `Arc<V8JsRuntime>`: канал команд клонируется
/// из рантайма при создании. Нужен начальному about:blank-кадру
/// (`frame_bridge::ensure_initial_frame_runtime`): его `register_parent_document`
/// получает peer ЧИТАТЕЛЯ, а `Arc` читателя в js-крейте недоступен — handle
/// собирается из его же `cmd_tx`.
///
/// Два отличия от [`V8JsRuntime`]-пира:
/// - `peer_busy` всегда `false`: вызов НЕ отказывается на занятом пире, а
///   встаёт в его очередь команд за текущей задачей. Страж ревхода
///   (`frame_peer_bridge::enter_call`) держит ребро читатель→пир всё время
///   блокировки, поэтому задача пира не может ответить вызовом в читателя —
///   взаимного ожидания нет, а «пир занят» здесь означает лишь «подожди».
///   Для начального кадра это семантика спекового same-origin вызова
///   (`top.log` из `document.write`-скрипта, WPT execution-timing 070-075).
/// - отправка не паникует при смерти потока пира: `try_run` возвращает
///   `None`, и методы отдают конверт `error`.
#[cfg(feature = "v8-backend")]
pub struct V8PeerHandle {
    pub(crate) cmd_tx: SyncSender<V8Command>,
    /// Счётчик peer-ожиданий рантайма-владельца канала — для
    /// `peer_reentrant_ready` (обратное ребро стража ревхода).
    pub(crate) serving: Arc<AtomicUsize>,
}

#[cfg(feature = "v8-backend")]
impl V8PeerHandle {
    /// `Option`-вариант диспетчера `V8JsRuntime::run`: `None` — поток пира
    /// мёртв или задача не доехала.
    fn try_run<R, F>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut V8Inner) -> R + Send,
        R: Send,
    {
        let (tx, rx) = std::sync::mpsc::channel::<R>();
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + '_> = Box::new(move |inner| {
            let _ = tx.send(f(inner));
        });
        // Тот же приём, что в `V8JsRuntime::run`: блокируемся на `rx.recv()`,
        // поэтому заимствования вызывающего живы до конца задачи.
        // SAFETY: `rx.recv()` ниже держит вызывающего до завершения job,
        // поэтому любые заимствования в `f` не переживают отправку.
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + 'static> = unsafe {
            std::mem::transmute::<
                Box<dyn FnOnce(&mut V8Inner) + Send + '_>,
                Box<dyn FnOnce(&mut V8Inner) + Send + 'static>,
            >(job)
        };
        // `RunPeer` + `recv_peer`: вызов peer'а — тот же реентерабельный
        // канал, что у `V8JsRuntime::run_peer`; на JS-потоке вызывающего
        // ожидание попутно обслуживает его собственные `RunPeer`-команды.
        self.cmd_tx.send(V8Command::RunPeer(job)).ok()?;
        super::thread::recv_peer(&rx)
    }
}

#[cfg(feature = "v8-backend")]
impl FramePeerBridge for V8PeerHandle {
    /// См. документацию типа: занятый пир — это отложенный ответ, а не отказ.
    fn peer_busy(&self) -> bool {
        false
    }

    /// Рантайм-владелец канала ждёт чужой вызов и pump'ит свои `RunPeer` —
    /// обратное ребро стража ревхода здесь так же безопасно, как у
    /// [`V8JsRuntime`]-пира.
    fn peer_reentrant_ready(&self) -> bool {
        self.serving.load(Ordering::Relaxed) > 0
    }

    fn peer_global_get(&self, name: &str) -> JsValue {
        let name = name.to_owned();
        self.try_run(move |inner| peer_global_get_body(inner, &name))
            .unwrap_or_else(|| envelope_error("peer runtime is gone"))
    }

    fn peer_global_call(&self, name: &str, args: &[JsValue]) -> JsValue {
        let name = name.to_owned();
        let args = args.to_vec();
        self.try_run(move |inner| peer_global_call_body(inner, &name, &args))
            .unwrap_or_else(|| envelope_error("peer runtime is gone"))
    }

    fn peer_global_set(&self, name: &str, value: JsValue) -> JsValue {
        let name = name.to_owned();
        self.try_run(move |inner| peer_global_set_body(inner, &name, value))
            .unwrap_or_else(|| envelope_error("peer runtime is gone"))
    }
}
