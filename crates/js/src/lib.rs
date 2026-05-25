pub mod dom;

use lumen_core::{JsError, JsResult, JsRuntime, JsValue};
use lumen_dom::Document;
use rquickjs::{Array, Context, Ctx, FromJs, Function, IntoJs, Object, Runtime, Type, Value};
use std::sync::{Arc, Mutex};

/// QuickJS-based JS runtime via `rquickjs`.
///
/// QuickJS is single-threaded; `Mutex` provides the exclusive access needed
/// to satisfy `JsRuntime: Send + Sync`.
pub struct QuickJsRuntime {
    inner: Mutex<Inner>,
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
        })
    }

    /// Install DOM Web API globals (`document`, `window`, `console`, etc.) into
    /// this runtime.  Must be called before running any user scripts that access
    /// the DOM.  The `doc` Arc is captured by the registered native functions;
    /// drop the runtime (via `drop(runtime)`) before calling
    /// `Arc::try_unwrap(doc)` to recover the document after script execution.
    ///
    /// `fetch_provider` is forwarded to `window.fetch()`.
    /// `ws_provider` is forwarded to `new WebSocket(url)`.
    /// Pass `None` for either in sandboxed contexts or unit tests.
    pub fn install_dom(
        &self,
        doc: Arc<Mutex<Document>>,
        fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
        ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
    ) -> JsResult<()> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            dom::install_dom_api(&ctx, doc, fetch_provider, ws_provider)
                .map_err(|e| rq_err(&ctx, e))
        })
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
