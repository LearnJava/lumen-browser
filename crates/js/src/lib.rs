pub mod dom;

use lumen_core::{JsError, JsResult, JsRuntime, JsValue};
use lumen_dom::Document;
use rquickjs::{Array, Context, Ctx, FromJs, Function, IntoJs, Object, Runtime, Type, Value};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub use dom::NavigateRequest;
pub use lumen_core::WebStorage;

/// QuickJS-based JS runtime via `rquickjs`.
///
/// QuickJS is single-threaded; `Mutex` provides the exclusive access needed
/// to satisfy `JsRuntime: Send + Sync`.
pub struct QuickJsRuntime {
    inner: Mutex<Inner>,
    /// Navigation request written by JS via `location.href=`, `location.assign()` etc.
    /// Captured inside `install_dom_api`; read by `take_navigate_request`.
    nav_out: Arc<Mutex<Option<NavigateRequest>>>,
    /// Next timer wakeup deadline as Unix epoch ms (set by `_lumen_request_wakeup`).
    /// Read by the shell event loop to schedule `ControlFlow::WaitUntil`.
    /// `take_timer_wakeup` atomically clears after reading.
    timer_wakeup: Arc<Mutex<Option<f64>>>,
    /// Set to `true` by any DOM-mutating JS binding (setAttribute, textContent,
    /// appendChild, etc.). The shell reads and clears this after each rAF pass
    /// to decide whether a relayout is needed before the next paint.
    dom_dirty: Arc<AtomicBool>,
    /// Set to `true` when JS calls `requestAnimationFrame(fn)`.
    /// Cleared (and returned) by `take_raf_pending` after each rendering step.
    /// Shell uses this to decide whether to request the next redraw for animations.
    raf_pending: Arc<AtomicBool>,
    /// Layout bounding rects updated after each relayout by the shell.
    /// Maps NodeId index (u32) → [x, y, width, height] in viewport-relative CSS px.
    /// Read by `_lumen_get_bounding_rect(nid)` from JS (e.g., ResizeObserver/IntersectionObserver).
    layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Current viewport size [width, height] in CSS px.
    /// Updated by the shell on every resize; read by `_lumen_get_viewport_size()`.
    viewport_size: Arc<Mutex<[f32; 2]>>,
}

struct Inner {
    // Runtime must outlive its Context; keeping both under one lock avoids
    // any ordering issues on drop.
    _rt: Runtime,
    ctx: Context,
}

// SAFETY: QuickJS context is accessed only under the Mutex — never from
// multiple threads concurrently. Runtime wraps its own Mutex internally.
unsafe impl Send for QuickJsRuntime {}
unsafe impl Sync for QuickJsRuntime {}

impl QuickJsRuntime {
    pub fn new() -> Result<Self, JsError> {
        let rt = Runtime::new().map_err(|e| JsError::Runtime(e.to_string()))?;
        let ctx = Context::full(&rt).map_err(|e| JsError::Runtime(e.to_string()))?;
        Ok(Self {
            inner: Mutex::new(Inner { _rt: rt, ctx }),
            nav_out: Arc::new(Mutex::new(None)),
            timer_wakeup: Arc::new(Mutex::new(None)),
            dom_dirty: Arc::new(AtomicBool::new(false)),
            raf_pending: Arc::new(AtomicBool::new(false)),
            layout_rects: Arc::new(Mutex::new(HashMap::new())),
            viewport_size: Arc::new(Mutex::new([0.0, 0.0])),
        })
    }

    /// Install DOM Web API globals (`document`, `window`, `console`, etc.) into
    /// this runtime.  Must be called before running any user scripts that access
    /// the DOM.  The `doc` Arc is captured by the registered native functions;
    /// drop the runtime (via `drop(runtime)`) before calling
    /// `Arc::try_unwrap(doc)` to recover the document after script execution.
    ///
    /// `page_url` initialises `window.location` with the current page URL.
    /// `fetch_provider` is forwarded to `window.fetch()`.
    /// `ws_provider` is forwarded to `new WebSocket(url)`.
    /// `ls_store` — shared localStorage for this origin; persists across reloads.
    ///   Pass a fresh `Arc::new(Mutex::new(WebStorage::default()))` per origin.
    ///   A fresh `sessionStorage` is created automatically inside.
    /// Pass `None` for providers in sandboxed contexts or unit tests.
    pub fn install_dom(
        &self,
        doc: Arc<Mutex<Document>>,
        page_url: &str,
        fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
        ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
        ls_store: Option<Arc<Mutex<WebStorage>>>,
    ) -> JsResult<()> {
        let ls = ls_store.unwrap_or_else(|| Arc::new(Mutex::new(WebStorage::default())));
        let ss = Arc::new(Mutex::new(WebStorage::default()));
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            dom::install_dom_api(
                &ctx,
                doc,
                page_url,
                Arc::clone(&self.nav_out),
                fetch_provider,
                ws_provider,
                ls,
                ss,
                Arc::clone(&self.timer_wakeup),
                Arc::clone(&self.dom_dirty),
                Arc::clone(&self.raf_pending),
                Arc::clone(&self.layout_rects),
                Arc::clone(&self.viewport_size),
            )
            .map_err(|e| rq_err(&ctx, e))
        })
    }

    /// Consume any navigation request that JS placed via `location.href =` etc.
    /// Returns `None` if no navigation was requested during script execution.
    /// Must be called before `drop(runtime)` to avoid losing the request.
    pub fn take_navigate_request(&self) -> Option<NavigateRequest> {
        self.nav_out.lock().unwrap().take()
    }

    /// Returns `true` if JS mutated the DOM since the last call, clearing the flag.
    ///
    /// Called by the shell event loop after each rAF pass to decide whether a
    /// relayout is needed before the next paint.
    pub fn take_dom_dirty(&self) -> bool {
        self.dom_dirty.swap(false, Ordering::Relaxed)
    }

    /// Returns `true` if `requestAnimationFrame` was called since the last call,
    /// clearing the flag.
    ///
    /// Shell reads this after each rendering step: if `true`, another redraw must
    /// be requested so the animation loop gets its next frame.
    pub fn take_raf_pending(&self) -> bool {
        self.raf_pending.swap(false, Ordering::Relaxed)
    }

    /// Take the next timer wakeup as Unix epoch ms, clearing the stored value.
    ///
    /// Called by the shell event loop in `about_to_wait` to schedule
    /// `ControlFlow::WaitUntil` so the loop wakes up when the next JS timer fires.
    /// Returns `None` when no timers are pending.
    pub fn take_timer_wakeup(&self) -> Option<f64> {
        self.timer_wakeup.lock().unwrap().take()
    }

    /// Replace the layout bounding-rect table with a fresh snapshot.
    ///
    /// Called by the shell after every `relayout_page` call.  Maps `NodeId`
    /// index (u32) → `[x, y, width, height]` in viewport-relative CSS px
    /// (border-box coordinates, same as `getBoundingClientRect`).
    pub fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>) {
        *self.layout_rects.lock().unwrap() = rects;
    }

    /// Update the viewport dimensions.
    ///
    /// Called by the shell after every resize event and at initial load.
    /// `width` and `height` are in CSS px (physical size / device pixel ratio).
    pub fn update_viewport_size(&self, width: f32, height: f32) {
        *self.viewport_size.lock().unwrap() = [width, height];
    }
}

impl Default for QuickJsRuntime {
    fn default() -> Self {
        Self::new().expect("QuickJS runtime init failed")
    }
}

impl JsRuntime for QuickJsRuntime {
    fn eval(&self, script: &str) -> JsResult<JsValue> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            let val: Value = ctx.eval(script).map_err(|e| rq_err(&ctx, e))?;
            from_rq(&ctx, val)
        })
    }

    fn set_global(&self, name: &str, value: JsValue) -> JsResult<()> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            let rq = to_rq(&ctx, value)?;
            ctx.globals().set(name, rq).map_err(|e| rq_err(&ctx, e))
        })
    }

    fn get_global(&self, name: &str) -> JsResult<JsValue> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            let val: Value = ctx.globals().get(name).map_err(|e| rq_err(&ctx, e))?;
            from_rq(&ctx, val)
        })
    }

    fn call_function(&self, name: &str, args: &[JsValue]) -> JsResult<JsValue> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            // Verify the function exists before building the call.
            let _: Function = ctx.globals().get(name).map_err(|e| rq_err(&ctx, e))?;

            // rquickjs 0.11 Function::call requires IntoArgs (fixed-size tuples only);
            // Function::apply does not exist. Work around with a temporary global +
            // JS Function.prototype.apply via eval.
            let arr = Array::new(ctx.clone())
                .map_err(|e| JsError::Runtime(e.to_string()))?;
            for (i, v) in args.iter().enumerate() {
                let rq_val = to_rq(&ctx, v.clone())?;
                arr.set(i, rq_val).map_err(|e| JsError::Runtime(e.to_string()))?;
            }
            ctx.globals()
                .set("__lum_args__", arr)
                .map_err(|e| rq_err(&ctx, e))?;

            let script = format!("{name}.apply(null, __lum_args__)");
            let result: Value = ctx.eval(script.as_str()).map_err(|e| rq_err(&ctx, e))?;

            // Clean up the temporary global.
            ctx.eval::<Value, _>("delete __lum_args__").ok();

            from_rq(&ctx, result)
        })
    }

    fn engine_name(&self) -> &'static str {
        "quickjs"
    }
}

fn rq_err(ctx: &Ctx<'_>, err: rquickjs::Error) -> JsError {
    match err {
        rquickjs::Error::Exception => {
            let exc = ctx.catch();
            JsError::Runtime(exc_message(ctx, exc))
        }
        _ => JsError::Runtime(err.to_string()),
    }
}

fn exc_message<'js>(ctx: &Ctx<'js>, exc: Value<'js>) -> String {
    // JS `Error` objects expose `.message`; fall back to string coercion.
    if let Ok(obj) = Object::from_js(ctx, exc.clone())
        && let Ok(msg) = obj.get::<_, String>("message")
    {
        return msg;
    }
    String::from_js(ctx, exc).unwrap_or_else(|_| "JS exception".into())
}

fn from_rq<'js>(ctx: &Ctx<'js>, val: Value<'js>) -> JsResult<JsValue> {
    match val.type_of() {
        Type::Undefined | Type::Null => Ok(JsValue::Null),
        Type::Bool => bool::from_js(ctx, val)
            .map(JsValue::Bool)
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::Int => i32::from_js(ctx, val)
            .map(|n| JsValue::Number(f64::from(n)))
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::Float => f64::from_js(ctx, val)
            .map(JsValue::Number)
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::String => String::from_js(ctx, val)
            .map(JsValue::String)
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::Array => {
            let arr = Array::from_js(ctx, val).map_err(|e| JsError::Runtime(e.to_string()))?;
            let mut items = Vec::with_capacity(arr.len());
            for i in 0..arr.len() {
                let elem: Value = arr.get(i).map_err(|e| JsError::Runtime(e.to_string()))?;
                items.push(from_rq(ctx, elem)?);
            }
            Ok(JsValue::Array(items))
        }
        Type::Object | Type::Function => {
            let obj = Object::from_js(ctx, val).map_err(|e| JsError::Runtime(e.to_string()))?;
            let mut entries: Vec<(String, JsValue)> = Vec::new();
            for prop in obj.props::<String, Value>() {
                let (k, v) = prop.map_err(|e| JsError::Runtime(e.to_string()))?;
                entries.push((k, from_rq(ctx, v)?));
            }
            Ok(JsValue::object(entries))
        }
        _ => Ok(JsValue::Undefined),
    }
}

fn to_rq<'js>(ctx: &Ctx<'js>, val: JsValue) -> JsResult<Value<'js>> {
    Ok(match val {
        JsValue::Null | JsValue::Undefined => Value::new_null(ctx.clone()),
        JsValue::Bool(b) => Value::new_bool(ctx.clone(), b),
        JsValue::Number(n) => Value::new_float(ctx.clone(), n),
        JsValue::String(s) => s
            .into_js(ctx)
            .map_err(|e| JsError::Runtime(e.to_string()))?,
        JsValue::Array(items) => {
            let rq_items = items
                .into_iter()
                .map(|v| to_rq(ctx, v))
                .collect::<JsResult<Vec<_>>>()?;
            rq_items
                .into_js(ctx)
                .map_err(|e| JsError::Runtime(e.to_string()))?
        }
        JsValue::Object(entries) => {
            let obj = Object::new(ctx.clone()).map_err(|e| JsError::Runtime(e.to_string()))?;
            for (k, v) in entries {
                let rq_val = to_rq(ctx, v)?;
                obj.set(k, rq_val)
                    .map_err(|e| JsError::Runtime(e.to_string()))?;
            }
            obj.into_js(ctx)
                .map_err(|e| JsError::Runtime(e.to_string()))?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::JsRuntime;

    fn rt() -> QuickJsRuntime {
        QuickJsRuntime::new().unwrap()
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
        rt.set_global("greeting", JsValue::String("hi".into()))
            .unwrap();
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
        // QuickJS wraps parse errors as runtime exceptions.
        assert!(matches!(
            rt().eval("function ("),
            Err(JsError::Runtime(_))
        ));
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
        rt.set_global("arr", JsValue::Array(vec![JsValue::Number(1.0), JsValue::Number(2.0)]))
            .unwrap();
        assert_eq!(
            rt.eval("arr[0] + arr[1]").unwrap(),
            JsValue::Number(3.0)
        );
    }

    #[test]
    fn engine_name() {
        assert_eq!(rt().engine_name(), "quickjs");
    }

    #[test]
    fn is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<QuickJsRuntime>();
    }
}
