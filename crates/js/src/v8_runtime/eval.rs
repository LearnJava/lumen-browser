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

impl JsRuntime for V8JsRuntime {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn eval(&self, script: &str) -> JsResult<JsValue> {
        self.run(|inner| {
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

                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                match result {
                    Some(val) => from_v8(tc, val),
                    None => Err(JsError::Runtime("script returned no value".into())),
                }
            })
        })
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
                let global = ctx.global(tc);
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
                let global = ctx.global(tc);
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
                let global = ctx.global(tc);
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
                let result = func.call(tc, recv, &v8_args);
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
            if let Some(key) = v8::String::new(rtc, $reporter)
                && let Some(report_fn) = global
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
    /// `_lumen_report_worker_exception` (`crate::worker::worker_global_shim`),
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

                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    report_exception_via!(tc, exc, reporter.as_str());
                    return Err(v8_err(tc, exc));
                }
                match result {
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
