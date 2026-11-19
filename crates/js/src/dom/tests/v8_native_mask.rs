//! BUG-1826: Web API functions are JS shims, so `Function.prototype.toString`
//! must report `[native code]` for them — anti-bot fingerprinting (Cloudflare)
//! treats leaked shim source as a fake browser.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

#[test]
fn web_api_functions_report_native_code() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "[atob, btoa, fetch, TextEncoder.prototype.encode, \
             document.createElement, Math.random]\
             .every(function (f) { \
               return Function.prototype.toString.call(f).indexOf('[native code]') !== -1; \
             })",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn page_functions_keep_their_source() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var myPageFn = function() { return 1; }; \
             Function.prototype.toString.call(myPageFn).indexOf('[native code]') === -1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn native_functions_keep_working_after_masking() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("atob(btoa('mask')) === 'mask' && typeof fetch === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn masked_function_to_string_has_native_shape() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var s = Function.prototype.toString.call(atob); \
             /^function atob\\(\\) \\{ \\[native code\\] \\}$/.test(s) || \
             s.indexOf('[native code]') !== -1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
