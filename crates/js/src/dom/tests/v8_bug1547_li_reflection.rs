//! BUG-1547 — `HTMLLIElement.value`/`.type` were absent (`li.value` read `undefined`).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn li_value_is_a_long_reflection_defaulting_to_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var li = document.createElement('li');").unwrap();
    assert!(is_true(&rt, "li.value === 0"));
    rt.eval("li.value = 7;").unwrap();
    assert!(is_true(&rt, "li.getAttribute('value') === '7' && li.value === 7"));
}

#[test]
fn li_type_reflects_the_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var li = document.createElement('li'); li.setAttribute('type', 'a');").unwrap();
    assert!(is_true(&rt, "li.type === 'a'"));
}
