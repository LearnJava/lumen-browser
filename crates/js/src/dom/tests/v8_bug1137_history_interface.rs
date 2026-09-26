//! BUG-1137 — `history` is an instance of a real `History` interface
//! (HTML LS §7.4.2): a non-constructible, non-enumerable global interface
//! object, members as brand-checked accessors/operations on
//! `History.prototype`, no own properties on the singleton.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// The bug report's repro: Chrome gives `function`, `true`, `true`,
/// `[object History]`, `History`.
#[test]
fn history_is_an_instance_of_the_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof History === 'function'"));
    assert!(is_true(&rt, "history instanceof History"));
    assert!(is_true(&rt, "Object.getPrototypeOf(history) === History.prototype"));
    assert!(is_true(&rt, "Object.prototype.toString.call(history) === '[object History]'"));
    assert!(is_true(&rt, "history.constructor.name === 'History'"));
    assert!(is_true(&rt, "window.History === History && window.history === history"));
}

#[test]
fn interface_object_is_not_constructible_and_not_enumerable() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { try { new History(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(globalThis, 'History').enumerable === false"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(History, 'prototype').writable === false"));
}

#[test]
fn members_live_on_the_prototype_and_are_brand_checked() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "Object.getOwnPropertyNames(history).length === 0"));
    for attr in ["length", "state"] {
        let code = format!(
            "(function() {{ var d = Object.getOwnPropertyDescriptor(History.prototype, '{attr}'); \
               return !!d && typeof d.get === 'function' && d.set === undefined && d.enumerable \
                 && d.get.name === 'get {attr}'; }})()"
        );
        assert!(is_true(&rt, &code), "{attr} must be a readonly accessor on History.prototype");
    }
    for op in ["go", "back", "forward", "pushState", "replaceState"] {
        let code = format!(
            "(function() {{ var d = Object.getOwnPropertyDescriptor(History.prototype, '{op}'); \
               return !!d && typeof d.value === 'function' \
                 && (function() {{ try {{ d.value.call({{}}); return false; }} \
                                   catch (e) {{ return e instanceof TypeError; }} }})(); }})()"
        );
        assert!(is_true(&rt, &code), "{op} must be a brand-checked operation on History.prototype");
    }
    assert!(is_true(&rt, "(function() { try { History.prototype.length; return false; } catch (e) { return e instanceof TypeError; } })()"));
}

/// The move onto the prototype must not break the members themselves.
#[test]
fn members_still_work_through_the_instance() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "history.state === null"));
    rt.eval("history.pushState({a: 1}, '', '#x')").unwrap();
    assert!(is_true(&rt, "history.state.a === 1 && location.hash === '#x'"));
    rt.eval("history.replaceState({a: 2}, '')").unwrap();
    assert!(is_true(&rt, "history.state.a === 2"));
}
