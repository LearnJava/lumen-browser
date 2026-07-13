//! Integration tests for `V8JsRuntime` (slice S1).
//!
//! Mirrors the `QuickJsRuntime` unit-test suite so the two backends are held
//! to the same behavioural contract. All tests are feature-gated to keep the
//! default (QuickJS-only) build unaffected.
#![cfg(feature = "v8-backend")]

use lumen_core::{JsError, JsRuntime, JsValue, SuspendedHeap};
use lumen_js::v8_runtime::V8JsRuntime;

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
