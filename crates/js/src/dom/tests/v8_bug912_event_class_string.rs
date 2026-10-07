//! BUG-912 — every event interface prototype owns its `@@toStringTag`, so
//! `Object.prototype.toString.call(new Event('x'))` is `[object Event]` and a
//! subclass never answers with its base's name.

use super::*;
use crate::v8_runtime::V8JsRuntime;

#[test]
fn event_classes_name_themselves() {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    for n in [
        "Event", "CustomEvent", "UIEvent", "MouseEvent", "KeyboardEvent", "FocusEvent", "ErrorEvent",
        "MessageEvent", "CloseEvent",
    ] {
        let code = format!(
            "(function(){{ var C = globalThis.{n}; var o = new C('x'); \
             var d = Object.getOwnPropertyDescriptor(C.prototype, Symbol.toStringTag); \
             return Object.prototype.toString.call(o) === '[object {n}]' && !!d && !d.writable && !d.enumerable && d.configurable; }})()"
        );
        assert_eq!(rt.eval(&code).unwrap(), lumen_core::JsValue::Bool(true), "{n}");
    }
}
