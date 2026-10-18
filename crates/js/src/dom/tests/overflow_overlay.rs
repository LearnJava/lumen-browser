//! CSS Overflow L3 §3.1: legacy value alias `overflow: overlay` ≡ `auto` в `element.style` и `CSS.supports`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn overlay_is_accepted_and_reads_back_as_auto() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.overflow = 'overlay'; \
             var a = d.style.overflow + '/' + d.style.overflowX + '/' + d.style.overflowY; \
             var e = document.createElement('div'); e.style.overflowY = 'OVERLAY'; \
             var f = document.createElement('div'); f.style.overflow = 'hidden overlay'; \
             a + '|' + e.style.overflowY + '|' + f.style.overflow + '|' + CSS.supports('overflow', 'overlay') \
             + '|' + CSS.supports('overflow-y: overlay')"
        ),
        "auto/auto/auto|auto|hidden auto|true|true"
    );
}
