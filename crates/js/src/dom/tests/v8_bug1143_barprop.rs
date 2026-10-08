//! BUG-1143 — `window.locationbar`/`menubar`/`personalbar`/`scrollbars`/
//! `statusbar`/`toolbar` and the `BarProp` interface (HTML LS §7.2.4).
//! weibo's bundle reads the bare global `toolbar`; before the fix every one
//! of these names was a `ReferenceError`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        doc,
        "https://example.test/",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false, None,
    )
    .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

const BARS: &str =
    "['locationbar', 'menubar', 'personalbar', 'scrollbars', 'statusbar', 'toolbar']";

/// The bug report's repro: each name resolves as a bare identifier to an
/// object whose `visible` is `true` (Chrome's answer in a normal window).
#[test]
fn bare_bar_globals_are_visible_barprops() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        &format!(
            "{BARS}.every(function(n) {{ var v = eval(n); \
             return v instanceof BarProp && v.visible === true && window[n] === v; }})"
        )
    ));
    assert!(is_true(&rt, "typeof BarProp === 'function'"));
    assert!(is_true(
        &rt,
        "Object.prototype.toString.call(toolbar) === '[object BarProp]'"
    ));
}

/// One object per attribute, and `BarProp` is not constructible from script.
#[test]
fn bars_are_distinct_and_constructor_is_illegal() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "toolbar !== menubar && toolbar === window.toolbar"));
    assert!(is_true(
        &rt,
        "(function() { try { new BarProp(); return false; } \
         catch (e) { return e instanceof TypeError; } })()"
    ));
    assert!(is_true(
        &rt,
        "Object.getOwnPropertyDescriptor(BarProp.prototype, 'visible').set === undefined"
    ));
}

/// `[Replaceable]`: an own enumerable, configurable accessor with a setter
/// (`window-properties.https.html`), and assigning shadows it with the value.
#[test]
fn bars_are_replaceable_accessors() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        &format!(
            "{BARS}.every(function(n) {{ var d = Object.getOwnPropertyDescriptor(window, n); \
             return !!d && typeof d.get === 'function' && typeof d.set === 'function' \
             && d.enumerable && d.configurable && !(n in Window.prototype); }})"
        )
    ));
    rt.eval("window.toolbar = 42;").unwrap();
    assert!(is_true(
        &rt,
        "toolbar === 42 && Object.getOwnPropertyDescriptor(window, 'toolbar').writable === true"
    ));
}
