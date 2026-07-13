//! V8-based JS runtime (slice S1 — runtime skeleton).
//!
//! Mirrors the `QuickJsRuntime` thread-dispatch pattern: a dedicated OS thread
//! owns the `v8::OwnedIsolate` (which is `!Send`); the handle exposes
//! `JsRuntime` methods that dispatch jobs to that thread via a bounded
//! `SyncSender`. Each job runs to completion before the caller unblocks
//! (blocking `recv`), so borrows of the caller's stack are sound via the
//! same `transmute`-lifetime trick used by `QuickJsRuntime::run`.
//!
//! Feature-gated: compiled only when `v8-backend` is enabled.

use lumen_core::{JsError, JsResult, JsRuntime, JsValue, SuspendedHeap};
use std::sync::{
    Once,
    mpsc::{SyncSender, Sender, sync_channel},
};
use std::thread::JoinHandle;

// ── Platform initialization ───────────────────────────────────────────────────

/// Process-global V8 platform, initialized exactly once.
static V8_INIT: Once = Once::new();

/// Initialize the V8 platform for this process.
///
/// Safe to call multiple times — subsequent calls are no-ops. All code that
/// creates a `v8::Isolate` (including the smoke test in `v8_smoke.rs`) must
/// call this first so there is exactly one `initialize_platform` call.
pub fn ensure_v8_platform() {
    V8_INIT.call_once(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

// ── Thread-local state ────────────────────────────────────────────────────────

/// V8 isolate + global context, owned exclusively by the JS thread.
///
/// Both `OwnedIsolate` and the `Global<Context>` are `!Send`; they are
/// created in [`v8_thread_main`] and never leave it.
struct V8Inner {
    isolate: v8::OwnedIsolate,
    /// Persistent handle to the main JS context.
    context: v8::Global<v8::Context>,
}

// ── Command channel ───────────────────────────────────────────────────────────

/// A unit of work executed on the JS thread against the live [`V8Inner`].
///
/// The caller blocks until the job completes (`rx.recv()`), so even though
/// the box is `'static` (required by `SyncSender`), it may safely capture
/// borrows from the caller's stack for the duration of the call.
type V8Job = Box<dyn FnOnce(&mut V8Inner) + Send + 'static>;

/// Messages the shell sends to the dedicated V8 JS thread.
enum V8Command {
    /// Run a job against the runtime.
    Run(V8Job),
    /// Shut down the thread and drop the isolate.
    Shutdown,
}

/// Bound for the V8 command queue (same value as `QuickJsRuntime`).
const V8_CMD_QUEUE_BOUND: usize = 64;

// ── Thread entry point ────────────────────────────────────────────────────────

/// Entry point of the dedicated V8 thread.
///
/// Initialises the V8 platform (idempotent), creates the isolate and context,
/// signals the caller via `init_tx`, then services [`V8Command`]s until the
/// channel closes or [`V8Command::Shutdown`] arrives.
fn v8_thread_main(
    cmd_rx: std::sync::mpsc::Receiver<V8Command>,
    init_tx: Sender<Result<(), JsError>>,
) {
    ensure_v8_platform();

    let mut isolate = v8::Isolate::new(Default::default());
    // Create the context inside a short-lived HandleScope so the scope's borrow
    // of `isolate` ends before we move `isolate` into `V8Inner`.
    let context = {
        // scope! pins the HandleScope and gives scope: &mut PinnedRef<HandleScope<'_, ()>>
        v8::scope!(let scope, &mut isolate);
        let ctx = v8::Context::new(scope, Default::default());
        // scope deref-coerces to &Isolate via PinnedRef<HandleScope<'_,()>> → Isolate
        v8::Global::new(scope, ctx)
    };

    let mut inner = V8Inner { isolate, context };
    let _ = init_tx.send(Ok(()));

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            V8Command::Run(job) => job(&mut inner),
            V8Command::Shutdown => break,
        }
    }
    // `inner` (OwnedIsolate + Global<Context>) drops here, on its owning thread.
}

// ── Public handle ─────────────────────────────────────────────────────────────

/// V8-backed JS runtime implementing [`JsRuntime`].
///
/// The isolate lives on a dedicated thread; methods block until the dispatched
/// job completes. Cheap to clone via `Arc` if shared access is needed (but
/// callers typically hold one runtime per tab).
pub struct V8JsRuntime {
    /// Channel to the JS thread.
    cmd_tx: SyncSender<V8Command>,
    /// Join handle taken in `Drop` after sending `Shutdown`.
    js_thread: Option<JoinHandle<()>>,
}

impl V8JsRuntime {
    /// Create a new V8 runtime on a dedicated thread.
    pub fn new() -> Result<Self, JsError> {
        let (cmd_tx, cmd_rx) = sync_channel::<V8Command>(V8_CMD_QUEUE_BOUND);
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), JsError>>();
        let js_thread = std::thread::Builder::new()
            .name("lumen-v8".to_string())
            .spawn(move || v8_thread_main(cmd_rx, init_tx))
            .map_err(|e| JsError::Runtime(format!("spawn V8 thread: {e}")))?;
        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(JsError::Runtime("V8 thread died during init".into())),
        }
        Ok(Self { cmd_tx, js_thread: Some(js_thread) })
    }

    /// Dispatch `f` to the JS thread, blocking until it completes.
    ///
    /// # Safety
    /// `f` may borrow from the caller's stack; we block on `rx.recv()` until the
    /// JS thread executes the job, so every borrow stays live. Erasing `'_` to
    /// `'static` is sound for the same reason as in `QuickJsRuntime::run`.
    fn run<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut V8Inner) -> R + Send,
        R: Send,
    {
        let (tx, rx) = std::sync::mpsc::channel::<R>();
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + '_> = Box::new(move |inner| {
            let _ = tx.send(f(inner));
        });
        // SAFETY: we block on rx.recv() below until the JS thread has completed
        // the job. Any borrows captured by `f` (e.g. `&str` args) outlive the
        // execution. The two Box types have identical fat-pointer layout; the
        // transmute only adjusts the lifetime annotation.
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + 'static> = unsafe {
            std::mem::transmute::<
                Box<dyn FnOnce(&mut V8Inner) + Send + '_>,
                Box<dyn FnOnce(&mut V8Inner) + Send + 'static>,
            >(job)
        };
        if self.cmd_tx.send(V8Command::Run(job)).is_err() {
            panic!("lumen-v8 thread terminated unexpectedly");
        }
        rx.recv().expect("lumen-v8 thread dropped without replying")
    }
}

impl Drop for V8JsRuntime {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(V8Command::Shutdown);
        if let Some(handle) = self.js_thread.take() {
            let _ = handle.join();
        }
    }
}

// ── JsRuntime impl ────────────────────────────────────────────────────────────

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
    fn eval(&self, script: &str) -> JsResult<JsValue> {
        self.run(|inner| {
            with_tc!(inner, |tc, _ctx| {
                let src = v8::String::new(tc, script)
                    .ok_or_else(|| JsError::Runtime("OOM: script string".into()))?;

                let compiled = v8::Script::compile(tc, src, None);
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
        // S11 will implement real ValueSerializer-based serialisation.
        Ok(SuspendedHeap::default())
    }

    fn resume(_snapshot: SuspendedHeap) -> JsResult<Self> {
        Self::new()
    }
}

// ── Value converters ──────────────────────────────────────────────────────────

/// Convert a V8 `Local<Value>` to a `JsValue`.
///
/// `scope` must be a `&PinScope<'s, '_>` (= `PinnedRef<HandleScope<'_, Context>>`).
/// Any scope that deref-coerces to one is accepted (e.g. `&mut PinnedRef<TryCatch<…>>`).
fn from_v8<'s>(
    scope: &v8::PinScope<'s, '_>,
    val: v8::Local<'s, v8::Value>,
) -> JsResult<JsValue> {
    if val.is_null() || val.is_undefined() {
        return Ok(JsValue::Null);
    }
    if val.is_boolean() {
        return Ok(JsValue::Bool(val.boolean_value(scope)));
    }
    if val.is_number() {
        return Ok(JsValue::Number(val.number_value(scope).unwrap_or(f64::NAN)));
    }
    if val.is_string() {
        let s = val
            .to_string(scope)
            .ok_or_else(|| JsError::Runtime("string conversion failed".into()))?;
        return Ok(JsValue::String(s.to_rust_string_lossy(scope)));
    }
    if val.is_array() {
        let arr: v8::Local<v8::Array> = val.try_into().unwrap();
        let len = arr.length();
        let mut items = Vec::with_capacity(len as usize);
        for i in 0..len {
            let elem = arr
                .get_index(scope, i)
                .ok_or_else(|| JsError::Runtime(format!("array[{i}] is missing")))?;
            items.push(from_v8(scope, elem)?);
        }
        return Ok(JsValue::Array(items));
    }
    if val.is_object() {
        let obj: v8::Local<v8::Object> = val.try_into().unwrap();
        let own_props = obj
            .get_own_property_names(scope, Default::default())
            .ok_or_else(|| JsError::Runtime("get_own_property_names failed".into()))?;
        let mut entries: Vec<(String, JsValue)> = Vec::new();
        for i in 0..own_props.length() {
            let key = own_props.get_index(scope, i).unwrap();
            let key_str = key
                .to_string(scope)
                .ok_or_else(|| JsError::Runtime("property key to_string failed".into()))?
                .to_rust_string_lossy(scope);
            let prop_val = obj
                .get(scope, key)
                .ok_or_else(|| JsError::Runtime(format!("get '{key_str}' failed")))?;
            entries.push((key_str, from_v8(scope, prop_val)?));
        }
        return Ok(JsValue::object(entries));
    }
    Ok(JsValue::Undefined)
}

/// Convert a `JsValue` to a V8 `Local<Value>`.
fn to_v8<'s>(
    scope: &v8::PinScope<'s, '_>,
    val: JsValue,
) -> JsResult<v8::Local<'s, v8::Value>> {
    Ok(match val {
        JsValue::Null | JsValue::Undefined => v8::null(scope).into(),
        JsValue::Bool(b) => v8::Boolean::new(scope, b).into(),
        JsValue::Number(n) => v8::Number::new(scope, n).into(),
        JsValue::String(s) => v8::String::new(scope, &s)
            .ok_or_else(|| JsError::Runtime("OOM: string allocation".into()))?
            .into(),
        JsValue::Array(items) => {
            let arr = v8::Array::new(scope, items.len() as i32);
            for (i, item) in items.into_iter().enumerate() {
                let v8_item = to_v8(scope, item)?;
                arr.set_index(scope, i as u32, v8_item);
            }
            arr.into()
        }
        JsValue::Object(entries) => {
            let obj = v8::Object::new(scope);
            for (k, v) in entries {
                let key = v8::String::new(scope, &k)
                    .ok_or_else(|| JsError::Runtime("OOM: key allocation".into()))?;
                let v8_val = to_v8(scope, v)?;
                obj.set(scope, key.into(), v8_val);
            }
            obj.into()
        }
    })
}

/// Extract an error message from a V8 exception value.
fn v8_err<'s>(
    scope: &v8::PinScope<'s, '_>,
    exc: v8::Local<'s, v8::Value>,
) -> JsError {
    // Try obj.message first (covers Error instances), fall back to string coercion.
    if let Ok(obj) = v8::Local::<v8::Object>::try_from(exc)
        && let Some(msg_key) = v8::String::new(scope, "message")
        && let Some(msg_val) = obj.get(scope, msg_key.into())
        && msg_val.is_string()
        && let Some(s) = msg_val.to_string(scope)
    {
        return JsError::Runtime(s.to_rust_string_lossy(scope));
    }
    let msg = exc
        .to_string(scope)
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "JS exception".into());
    JsError::Runtime(msg)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::JsRuntime;

    fn rt() -> V8JsRuntime {
        V8JsRuntime::new().unwrap()
    }

    #[test]
    fn eval_number() {
        assert_eq!(rt().eval("1 + 2").unwrap(), JsValue::Number(3.0));
    }

    #[test]
    fn eval_string() {
        assert_eq!(
            rt().eval(r#""hello" + " world""#).unwrap(),
            JsValue::String("hello world".into())
        );
    }

    #[test]
    fn eval_bool() {
        assert_eq!(rt().eval("true").unwrap(), JsValue::Bool(true));
        assert_eq!(rt().eval("false").unwrap(), JsValue::Bool(false));
    }

    #[test]
    fn eval_null() {
        assert_eq!(rt().eval("null").unwrap(), JsValue::Null);
    }

    #[test]
    fn set_get_global() {
        let rt = rt();
        rt.set_global("x", JsValue::Number(42.0)).unwrap();
        assert_eq!(rt.get_global("x").unwrap(), JsValue::Number(42.0));
    }

    #[test]
    fn set_global_string() {
        let rt = rt();
        rt.set_global("greeting", JsValue::String("hi".into())).unwrap();
        assert_eq!(
            rt.get_global("greeting").unwrap(),
            JsValue::String("hi".into())
        );
    }

    #[test]
    fn call_function_add() {
        let rt = rt();
        rt.eval("function add(a, b) { return a + b; }").unwrap();
        assert_eq!(
            rt.call_function("add", &[JsValue::Number(3.0), JsValue::Number(4.0)])
                .unwrap(),
            JsValue::Number(7.0)
        );
    }

    #[test]
    fn call_function_no_args() {
        let rt = rt();
        rt.eval("function forty_two() { return 42; }").unwrap();
        assert_eq!(
            rt.call_function("forty_two", &[]).unwrap(),
            JsValue::Number(42.0)
        );
    }

    #[test]
    fn eval_array() {
        assert_eq!(
            rt().eval("[1, 2, 3]").unwrap(),
            JsValue::Array(vec![
                JsValue::Number(1.0),
                JsValue::Number(2.0),
                JsValue::Number(3.0),
            ])
        );
    }

    #[test]
    fn eval_object() {
        let val = rt().eval(r#"({ a: 1, b: "x" })"#).unwrap();
        assert_eq!(
            val,
            JsValue::object([
                ("a".to_string(), JsValue::Number(1.0)),
                ("b".to_string(), JsValue::String("x".into())),
            ])
        );
    }

    #[test]
    fn eval_runtime_error() {
        assert!(matches!(
            rt().eval("throw new Error('boom')"),
            Err(JsError::Runtime(_))
        ));
    }

    #[test]
    fn eval_syntax_error() {
        assert!(matches!(rt().eval("function ("), Err(JsError::Runtime(_))));
    }

    #[test]
    fn round_trip_bool() {
        let rt = rt();
        rt.set_global("flag", JsValue::Bool(true)).unwrap();
        assert_eq!(rt.eval("flag").unwrap(), JsValue::Bool(true));
    }

    #[test]
    fn round_trip_array() {
        let rt = rt();
        rt.set_global(
            "arr",
            JsValue::Array(vec![JsValue::Number(1.0), JsValue::Number(2.0)]),
        )
        .unwrap();
        assert_eq!(rt.eval("arr[0] + arr[1]").unwrap(), JsValue::Number(3.0));
    }

    #[test]
    fn engine_name() {
        assert_eq!(rt().engine_name(), "v8");
    }

    #[test]
    fn is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<V8JsRuntime>();
    }

    #[test]
    fn resume_produces_functional_runtime() {
        let fresh = V8JsRuntime::resume(SuspendedHeap::default()).unwrap();
        assert_eq!(fresh.eval("6 * 7").unwrap(), JsValue::Number(42.0));
    }
}
