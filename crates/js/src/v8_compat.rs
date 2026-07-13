//! Compat layer (S2) — ergonomic native-function registration for V8.
//!
//! Provides `IntoV8NativeFn<Tag>` + `register_v8_native` so downstream
//! `install_*` functions look identical to the rquickjs originals:
//!
//! ```rust,ignore
//! reg!("_lumen_console_log", move |msg: String| {
//!     eprintln!("[JS] {msg}");
//! });
//! reg!("_lumen_get_count", move || -> u32 { 42 });
//! ```
//!
//! After S2 lands, porting the 380 rquickjs bindings in `dom.rs` is
//! mechanical: replace `Function::new(ctx.clone(), …)` with `reg!(…)`.
//! Feature-gated: compiled only when `v8-backend` is enabled.

use lumen_core::{JsError, JsResult, JsValue};
use std::ffi::c_void;
use std::marker::PhantomData;

// ── JsValue ↔ Rust type bridges ────────────────────────────────────────────

/// Convert a typed Rust value from a `JsValue` argument.
///
/// Implementations provide JS-style coercion (e.g. `Number → String`).
pub(crate) trait FromJsValue: Sized {
    /// Extract `Self` from `val`.  `idx` is the 0-based argument position,
    /// used only for error messages.
    fn from_js_value(val: JsValue, idx: usize) -> JsResult<Self>;
}

/// Convert a typed Rust return value into a `JsValue`.
pub(crate) trait IntoJsReturn {
    fn into_js_return(self) -> JsValue;
}

// ── FromJsValue impls ───────────────────────────────────────────────────────

impl FromJsValue for JsValue {
    fn from_js_value(val: JsValue, _: usize) -> JsResult<Self> {
        Ok(val)
    }
}

impl FromJsValue for String {
    fn from_js_value(val: JsValue, idx: usize) -> JsResult<Self> {
        match val {
            JsValue::String(s) => Ok(s),
            JsValue::Number(n) => Ok(format_number(n)),
            JsValue::Bool(b) => Ok(if b { "true".into() } else { "false".into() }),
            JsValue::Null => Ok("null".into()),
            JsValue::Undefined => {
                Err(JsError::Runtime(format!("arg[{idx}]: expected string, got undefined")))
            }
            JsValue::Array(_) | JsValue::Object(_) => Ok("[object]".into()),
        }
    }
}

impl FromJsValue for f64 {
    fn from_js_value(val: JsValue, idx: usize) -> JsResult<Self> {
        match val {
            JsValue::Number(n) => Ok(n),
            JsValue::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
            JsValue::String(s) => s
                .parse::<f64>()
                .map_err(|_| JsError::Runtime(format!("arg[{idx}]: cannot parse '{s}' as f64"))),
            JsValue::Null | JsValue::Undefined => Ok(0.0),
            _ => Err(JsError::Runtime(format!("arg[{idx}]: expected number"))),
        }
    }
}

impl FromJsValue for u32 {
    fn from_js_value(val: JsValue, idx: usize) -> JsResult<Self> {
        Ok(f64::from_js_value(val, idx)? as u32)
    }
}

impl FromJsValue for i32 {
    fn from_js_value(val: JsValue, idx: usize) -> JsResult<Self> {
        Ok(f64::from_js_value(val, idx)? as i32)
    }
}

impl FromJsValue for bool {
    fn from_js_value(val: JsValue, _: usize) -> JsResult<Self> {
        Ok(match val {
            JsValue::Bool(b) => b,
            JsValue::Number(n) => n != 0.0,
            JsValue::String(s) => !s.is_empty(),
            JsValue::Null | JsValue::Undefined => false,
            JsValue::Array(_) | JsValue::Object(_) => true,
        })
    }
}

impl<T: FromJsValue> FromJsValue for Option<T> {
    fn from_js_value(val: JsValue, idx: usize) -> JsResult<Self> {
        match val {
            JsValue::Null | JsValue::Undefined => Ok(None),
            other => T::from_js_value(other, idx).map(Some),
        }
    }
}

// ── IntoJsReturn impls ──────────────────────────────────────────────────────

impl IntoJsReturn for () {
    fn into_js_return(self) -> JsValue {
        JsValue::Undefined
    }
}

impl IntoJsReturn for JsValue {
    fn into_js_return(self) -> JsValue {
        self
    }
}

impl IntoJsReturn for String {
    fn into_js_return(self) -> JsValue {
        JsValue::String(self)
    }
}

impl IntoJsReturn for f64 {
    fn into_js_return(self) -> JsValue {
        JsValue::Number(self)
    }
}

impl IntoJsReturn for u32 {
    fn into_js_return(self) -> JsValue {
        JsValue::Number(self as f64)
    }
}

impl IntoJsReturn for i32 {
    fn into_js_return(self) -> JsValue {
        JsValue::Number(self as f64)
    }
}

impl IntoJsReturn for bool {
    fn into_js_return(self) -> JsValue {
        JsValue::Bool(self)
    }
}

impl<T: IntoJsReturn> IntoJsReturn for Option<T> {
    fn into_js_return(self) -> JsValue {
        match self {
            None => JsValue::Null,
            Some(v) => v.into_js_return(),
        }
    }
}

// ── V8NativeFn trait (object-safe) ─────────────────────────────────────────

/// Object-safe trait for all native functions registered with the V8 compat
/// layer.  Works at the `JsValue` level so the impl can be `dyn`-dispatched
/// without lifetime-generic methods.
pub(crate) trait V8NativeFn: Send + 'static {
    /// Invoke the native function.  `args` are the JS call arguments,
    /// pre-converted to `JsValue`.  Return value will be pushed back to JS.
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue>;
}

// ── Per-arity wrapper types ─────────────────────────────────────────────────

/// Arity-0 native function wrapper.
#[allow(dead_code)]
pub(crate) struct NativeFn0<R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn() -> R>,
}
// SAFETY: F: Send + 'static
unsafe impl<R, F: Send + 'static> Send for NativeFn0<R, F> {}

impl<R: IntoJsReturn + 'static, F: Fn() -> R + Send + 'static> V8NativeFn for NativeFn0<R, F> {
    fn call_js(&self, _args: &[JsValue]) -> JsResult<JsValue> {
        Ok((self.f)().into_js_return())
    }
}

/// Arity-1 native function wrapper.
pub(crate) struct NativeFn1<A, R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn(A) -> R>,
}
unsafe impl<A, R, F: Send + 'static> Send for NativeFn1<A, R, F> {}

impl<A: FromJsValue + 'static, R: IntoJsReturn + 'static, F: Fn(A) -> R + Send + 'static>
    V8NativeFn for NativeFn1<A, R, F>
{
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue> {
        let a = A::from_js_value(args.first().cloned().unwrap_or(JsValue::Undefined), 0)?;
        Ok((self.f)(a).into_js_return())
    }
}

/// Arity-2 native function wrapper.
#[allow(dead_code)]
pub(crate) struct NativeFn2<A, B, R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn(A, B) -> R>,
}
unsafe impl<A, B, R, F: Send + 'static> Send for NativeFn2<A, B, R, F> {}

impl<
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B) -> R + Send + 'static,
> V8NativeFn for NativeFn2<A, B, R, F>
{
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue> {
        let a = A::from_js_value(args.first().cloned().unwrap_or(JsValue::Undefined), 0)?;
        let b = B::from_js_value(args.get(1).cloned().unwrap_or(JsValue::Undefined), 1)?;
        Ok((self.f)(a, b).into_js_return())
    }
}

/// Arity-3 native function wrapper.
#[allow(dead_code)]
pub(crate) struct NativeFn3<A, B, C, R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn(A, B, C) -> R>,
}
unsafe impl<A, B, C, R, F: Send + 'static> Send for NativeFn3<A, B, C, R, F> {}

impl<
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C) -> R + Send + 'static,
> V8NativeFn for NativeFn3<A, B, C, R, F>
{
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue> {
        let a = A::from_js_value(args.first().cloned().unwrap_or(JsValue::Undefined), 0)?;
        let b = B::from_js_value(args.get(1).cloned().unwrap_or(JsValue::Undefined), 1)?;
        let c = C::from_js_value(args.get(2).cloned().unwrap_or(JsValue::Undefined), 2)?;
        Ok((self.f)(a, b, c).into_js_return())
    }
}

/// Arity-4 native function wrapper.
#[allow(dead_code, clippy::type_complexity)]
pub(crate) struct NativeFn4<A, B, C, D, R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn(A, B, C, D) -> R>,
}
unsafe impl<A, B, C, D, R, F: Send + 'static> Send for NativeFn4<A, B, C, D, R, F> {}

impl<
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D) -> R + Send + 'static,
> V8NativeFn for NativeFn4<A, B, C, D, R, F>
{
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue> {
        let a = A::from_js_value(args.first().cloned().unwrap_or(JsValue::Undefined), 0)?;
        let b = B::from_js_value(args.get(1).cloned().unwrap_or(JsValue::Undefined), 1)?;
        let c = C::from_js_value(args.get(2).cloned().unwrap_or(JsValue::Undefined), 2)?;
        let d = D::from_js_value(args.get(3).cloned().unwrap_or(JsValue::Undefined), 3)?;
        Ok((self.f)(a, b, c, d).into_js_return())
    }
}

/// Arity-5 native function wrapper.
#[allow(dead_code, clippy::type_complexity)]
pub(crate) struct NativeFn5<A, B, C, D, E, R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn(A, B, C, D, E) -> R>,
}
unsafe impl<A, B, C, D, E, R, F: Send + 'static> Send for NativeFn5<A, B, C, D, E, R, F> {}

impl<
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    E: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D, E) -> R + Send + 'static,
> V8NativeFn for NativeFn5<A, B, C, D, E, R, F>
{
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue> {
        let a = A::from_js_value(args.first().cloned().unwrap_or(JsValue::Undefined), 0)?;
        let b = B::from_js_value(args.get(1).cloned().unwrap_or(JsValue::Undefined), 1)?;
        let c = C::from_js_value(args.get(2).cloned().unwrap_or(JsValue::Undefined), 2)?;
        let d = D::from_js_value(args.get(3).cloned().unwrap_or(JsValue::Undefined), 3)?;
        let e = E::from_js_value(args.get(4).cloned().unwrap_or(JsValue::Undefined), 4)?;
        Ok((self.f)(a, b, c, d, e).into_js_return())
    }
}

/// Arity-6 native function wrapper.
#[allow(dead_code, clippy::type_complexity)]
pub(crate) struct NativeFn6<A, B, C, D, E, G, R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn(A, B, C, D, E, G) -> R>,
}
unsafe impl<A, B, C, D, E, G, R, F: Send + 'static> Send for NativeFn6<A, B, C, D, E, G, R, F> {}

impl<
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    E: FromJsValue + 'static,
    G: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D, E, G) -> R + Send + 'static,
> V8NativeFn for NativeFn6<A, B, C, D, E, G, R, F>
{
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue> {
        let a = A::from_js_value(args.first().cloned().unwrap_or(JsValue::Undefined), 0)?;
        let b = B::from_js_value(args.get(1).cloned().unwrap_or(JsValue::Undefined), 1)?;
        let c = C::from_js_value(args.get(2).cloned().unwrap_or(JsValue::Undefined), 2)?;
        let d = D::from_js_value(args.get(3).cloned().unwrap_or(JsValue::Undefined), 3)?;
        let e = E::from_js_value(args.get(4).cloned().unwrap_or(JsValue::Undefined), 4)?;
        let g = G::from_js_value(args.get(5).cloned().unwrap_or(JsValue::Undefined), 5)?;
        Ok((self.f)(a, b, c, d, e, g).into_js_return())
    }
}

/// Arity-7 native function wrapper.
#[allow(dead_code, clippy::type_complexity)]
pub(crate) struct NativeFn7<A, B, C, D, E, G, H, R, F> {
    pub(crate) f: F,
    _ph: PhantomData<fn(A, B, C, D, E, G, H) -> R>,
}
unsafe impl<A, B, C, D, E, G, H, R, F: Send + 'static> Send
    for NativeFn7<A, B, C, D, E, G, H, R, F>
{
}

impl<
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    E: FromJsValue + 'static,
    G: FromJsValue + 'static,
    H: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D, E, G, H) -> R + Send + 'static,
> V8NativeFn for NativeFn7<A, B, C, D, E, G, H, R, F>
{
    fn call_js(&self, args: &[JsValue]) -> JsResult<JsValue> {
        let a = A::from_js_value(args.first().cloned().unwrap_or(JsValue::Undefined), 0)?;
        let b = B::from_js_value(args.get(1).cloned().unwrap_or(JsValue::Undefined), 1)?;
        let c = C::from_js_value(args.get(2).cloned().unwrap_or(JsValue::Undefined), 2)?;
        let d = D::from_js_value(args.get(3).cloned().unwrap_or(JsValue::Undefined), 3)?;
        let e = E::from_js_value(args.get(4).cloned().unwrap_or(JsValue::Undefined), 4)?;
        let g = G::from_js_value(args.get(5).cloned().unwrap_or(JsValue::Undefined), 5)?;
        let h = H::from_js_value(args.get(6).cloned().unwrap_or(JsValue::Undefined), 6)?;
        Ok((self.f)(a, b, c, d, e, g, h).into_js_return())
    }
}

// ── Constructor functions — wrap typed closures into boxed V8NativeFn ──────
//
// Free functions avoid the "unconstrained type parameter" coherence error that
// would arise from `impl<A,R,F> SomeTrait for F where F: Fn(A)->R`.  The `reg!`
// macro in `v8_runtime.rs` calls the function matching the closure's arity.

/// Wrap a 0-argument closure as a boxed [`V8NativeFn`].
#[allow(dead_code)]
pub(crate) fn into_v8_fn0<R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    R: IntoJsReturn + 'static,
    F: Fn() -> R + Send + 'static,
{
    Box::new(NativeFn0 { f, _ph: PhantomData })
}

/// Wrap a 1-argument closure as a boxed [`V8NativeFn`].
pub(crate) fn into_v8_fn1<A, R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    A: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A) -> R + Send + 'static,
{
    Box::new(NativeFn1 { f, _ph: PhantomData })
}

/// Wrap a 2-argument closure as a boxed [`V8NativeFn`].
#[allow(dead_code)]
pub(crate) fn into_v8_fn2<A, B, R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B) -> R + Send + 'static,
{
    Box::new(NativeFn2 { f, _ph: PhantomData })
}

/// Wrap a 3-argument closure as a boxed [`V8NativeFn`].
#[allow(dead_code)]
pub(crate) fn into_v8_fn3<A, B, C, R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C) -> R + Send + 'static,
{
    Box::new(NativeFn3 { f, _ph: PhantomData })
}

/// Wrap a 4-argument closure as a boxed [`V8NativeFn`].
#[allow(dead_code)]
pub(crate) fn into_v8_fn4<A, B, C, D, R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D) -> R + Send + 'static,
{
    Box::new(NativeFn4 { f, _ph: PhantomData })
}

/// Wrap a 5-argument closure as a boxed [`V8NativeFn`].
#[allow(dead_code)]
pub(crate) fn into_v8_fn5<A, B, C, D, E, R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    E: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D, E) -> R + Send + 'static,
{
    Box::new(NativeFn5 { f, _ph: PhantomData })
}

/// Wrap a 6-argument closure as a boxed [`V8NativeFn`].
#[allow(dead_code)]
pub(crate) fn into_v8_fn6<A, B, C, D, E, G, R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    E: FromJsValue + 'static,
    G: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D, E, G) -> R + Send + 'static,
{
    Box::new(NativeFn6 { f, _ph: PhantomData })
}

/// Wrap a 7-argument closure as a boxed [`V8NativeFn`].
#[allow(dead_code)]
pub(crate) fn into_v8_fn7<A, B, C, D, E, G, H, R, F>(f: F) -> Box<dyn V8NativeFn + Send>
where
    A: FromJsValue + 'static,
    B: FromJsValue + 'static,
    C: FromJsValue + 'static,
    D: FromJsValue + 'static,
    E: FromJsValue + 'static,
    G: FromJsValue + 'static,
    H: FromJsValue + 'static,
    R: IntoJsReturn + 'static,
    F: Fn(A, B, C, D, E, G, H) -> R + Send + 'static,
{
    Box::new(NativeFn7 { f, _ph: PhantomData })
}

// ── Ownership handle for registered native closures ─────────────────────────

/// Owns a double-boxed `V8NativeFn + Send` allocated for `v8::External`.
///
/// `Box::into_raw(Box::new(f as Box<dyn V8NativeFn + Send>))` yields a thin
/// pointer to a fat pointer on the heap.  `OwnedNativeFn` stores that thin
/// pointer and frees it on drop.
pub(crate) struct OwnedNativeFn(pub *mut Box<dyn V8NativeFn + Send>);

// SAFETY: the pointer was created from `Box<Box<dyn V8NativeFn + Send>>`, and
// `dyn V8NativeFn + Send` is `Send`.
unsafe impl Send for OwnedNativeFn {}

impl Drop for OwnedNativeFn {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: created by Box::into_raw(Box::new(f))
            unsafe { drop(Box::from_raw(self.0)); }
        }
    }
}

// ── V8 value conversion helpers ─────────────────────────────────────────────

/// Convert a `v8::Local<Value>` to `JsValue` (best-effort; complex types → Null).
pub(crate) fn v8_to_jsvalue(
    scope: &v8::PinScope<'_, '_>,
    val: v8::Local<'_, v8::Value>,
) -> JsValue {
    if val.is_null() || val.is_undefined() {
        return JsValue::Null;
    }
    if val.is_boolean() {
        return JsValue::Bool(val.boolean_value(scope));
    }
    if val.is_number() {
        return JsValue::Number(val.number_value(scope).unwrap_or(f64::NAN));
    }
    if val.is_string() {
        return val
            .to_string(scope)
            .map(|s| JsValue::String(s.to_rust_string_lossy(scope)))
            .unwrap_or(JsValue::Null);
    }
    // Arrays, objects, functions → Null (sufficient for compat-layer natives)
    JsValue::Null
}

/// Convert a `JsValue` to a `v8::Local<Value>`.
pub(crate) fn jsvalue_to_v8<'s>(
    scope: &v8::PinScope<'s, '_>,
    val: JsValue,
) -> v8::Local<'s, v8::Value> {
    match val {
        JsValue::Null | JsValue::Undefined => v8::null(scope).into(),
        JsValue::Bool(b) => v8::Boolean::new(scope, b).into(),
        JsValue::Number(n) => v8::Number::new(scope, n).into(),
        JsValue::String(s) => v8::String::new(scope, &s)
            .map(Into::into)
            .unwrap_or_else(|| v8::null(scope).into()),
        JsValue::Array(items) => {
            let arr = v8::Array::new(scope, items.len() as i32);
            for (i, item) in items.into_iter().enumerate() {
                let v = jsvalue_to_v8(scope, item);
                arr.set_index(scope, i as u32, v);
            }
            arr.into()
        }
        JsValue::Object(entries) => {
            let obj = v8::Object::new(scope);
            for (k, v) in entries {
                if let Some(key) = v8::String::new(scope, &k) {
                    let val = jsvalue_to_v8(scope, v);
                    obj.set(scope, key.into(), val);
                }
            }
            obj.into()
        }
    }
}

// ── The single V8 callback for all compat-layer natives ────────────────────

/// Universal V8 trampoline for compat-layer native functions.
///
/// Retrieves the `Box<dyn V8NativeFn + Send>` stored in the External passed as
/// `data`, converts the JS args to `JsValue`, dispatches to `call_js`, and sets
/// the return value.  If `call_js` returns an error a `TypeError` is scheduled.
pub(crate) fn native_fn_trampoline(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    // Retrieve closure from External data.
    let data: v8::Local<v8::Value> = args.data();
    let ext: v8::Local<v8::External> = match data.try_into() {
        Ok(e) => e,
        Err(_) => return, // should never happen
    };
    // SAFETY: pointer was stored by `register_v8_native` from a `Box::into_raw`
    // call.  It is kept alive in `V8Inner::native_fn_store` for the lifetime of
    // the isolate.
    let fn_ref: &dyn V8NativeFn =
        unsafe { &**(ext.value() as *const Box<dyn V8NativeFn + Send>) };

    // Convert V8 arguments to JsValue.
    let n = args.length() as usize;
    let mut js_args: Vec<JsValue> = Vec::with_capacity(n);
    for i in 0..n {
        js_args.push(v8_to_jsvalue(scope, args.get(i as i32)));
    }

    // Dispatch.
    match fn_ref.call_js(&js_args) {
        Ok(result) => {
            let v8_val = jsvalue_to_v8(scope, result);
            rv.set(v8_val);
        }
        Err(e) => {
            // Schedule a JS TypeError so the error propagates to JS callers.
            let msg_str = match e {
                JsError::Runtime(s) | JsError::Parse(s) => s,
                JsError::NotImplemented => "not implemented".into(),
            };
            if let Some(msg) = v8::String::new(scope, &msg_str) {
                let exc = v8::Exception::type_error(scope, msg);
                scope.throw_exception(exc);
            }
        }
    }
}

// ── Public registration helper ──────────────────────────────────────────────

/// Register a native function in the V8 global object.
///
/// `store` must be `V8Inner::native_fn_store`; it keeps the closure alive for
/// the isolate's lifetime.  `scope` must be a `ContextScope` so `ctx.global`
/// is accessible.
///
/// # Safety
///
/// `scope` must have a current context (i.e. be inside a `ContextScope`).
pub(crate) fn register_v8_native(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    name: &str,
    f: Box<dyn V8NativeFn + Send>,
) -> JsResult<()> {
    // Double-box to get a stable thin pointer for the External.
    let outer: Box<Box<dyn V8NativeFn + Send>> = Box::new(f);
    let thin_ptr: *mut Box<dyn V8NativeFn + Send> = Box::into_raw(outer);
    store.push(OwnedNativeFn(thin_ptr));

    let ext = v8::External::new(scope, thin_ptr as *mut c_void);
    let func = v8::Function::builder(native_fn_trampoline)
        .data(ext.into())
        .build(scope)
        .ok_or_else(|| JsError::Runtime(format!("V8: failed to create function '{name}'")))?;

    let key = v8::String::new(scope, name)
        .ok_or_else(|| JsError::Runtime(format!("V8: OOM creating key '{name}'")))?;
    ctx.global(scope).set(scope, key.into(), func.into());
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
