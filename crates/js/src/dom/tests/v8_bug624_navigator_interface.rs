//! BUG-624 — `navigator` is an instance of a real `Navigator` interface
//! (HTML LS §8.9.1): a global interface object that cannot be constructed,
//! members as brand-checked accessors/operations on `Navigator.prototype`,
//! and no own properties left on the singleton after `install_dom`.

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

#[test]
fn navigator_is_an_instance_of_the_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof Navigator === 'function'"));
    assert!(is_true(&rt, "navigator instanceof Navigator"));
    assert!(is_true(&rt, "Object.getPrototypeOf(navigator) === Navigator.prototype"));
    assert!(is_true(&rt, "Object.prototype.toString.call(navigator) === '[object Navigator]'"));
    assert!(is_true(&rt, "window.Navigator === Navigator"));
}

#[test]
fn interface_object_is_not_constructible_and_not_enumerable() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { try { new Navigator(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "(function() { try { Navigator(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(globalThis, 'Navigator').enumerable === false"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(Navigator, 'prototype').writable === false"));
}

/// The core of the bug: every member used to be a writable own data property.
#[test]
fn members_live_on_the_prototype_not_the_instance() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(
        is_true(&rt, "Object.getOwnPropertyNames(navigator).length === 0"),
        "own names left: {:?}",
        rt.eval("Object.getOwnPropertyNames(navigator).join(',')").unwrap()
    );
    for attr in ["userAgent", "language", "languages", "onLine", "platform", "hardwareConcurrency", "permissions", "userActivation", "clipboard"] {
        let code = format!(
            "(function() {{ var d = Object.getOwnPropertyDescriptor(Navigator.prototype, '{attr}'); \
               return !!d && typeof d.get === 'function' && d.set === undefined && d.enumerable; }})()"
        );
        assert!(is_true(&rt, &code), "{attr} must be a readonly accessor on Navigator.prototype");
    }
    for op in ["sendBeacon", "share", "canShare", "getGamepads"] {
        let code = format!(
            "(function() {{ var d = Object.getOwnPropertyDescriptor(Navigator.prototype, '{op}'); \
               return !!d && typeof d.value === 'function'; }})()"
        );
        assert!(is_true(&rt, &code), "{op} must be an operation on Navigator.prototype");
    }
}

#[test]
fn values_are_unchanged_and_attributes_are_readonly() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof navigator.userAgent === 'string' && navigator.userAgent.length > 0"));
    assert!(is_true(&rt, "navigator.language === navigator.languages[0]"));
    assert!(is_true(&rt, "navigator.permissions === navigator.permissions"));
    assert!(is_true(&rt, "typeof navigator.permissions.query === 'function'"));
    rt.eval("navigator.userAgent = 'forged';").unwrap();
    assert!(is_true(&rt, "navigator.userAgent !== 'forged'"));
    assert!(is_true(&rt, "(function() { 'use strict'; try { navigator.onLine = true; return false; } catch (e) { return e instanceof TypeError; } })()"));
}

/// WebIDL getters are brand-checked: calling one on anything but a
/// `Navigator` instance throws instead of returning the singleton's value.
#[test]
fn getters_are_brand_checked() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "(function() { try { Navigator.prototype.userAgent; return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "(function() { var g = Object.getOwnPropertyDescriptor(Navigator.prototype, 'language').get; \
                          try { g.call({}); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(Navigator.prototype, 'userAgent').get.name === 'get userAgent'"));
}

/// BUG-295's live path runs the override script on an already-loaded page,
/// where `userAgent` is now a setter-less prototype getter.
#[test]
fn user_agent_override_script_reaches_a_loaded_page() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&crate::v8_runtime::user_agent_override_script("LumenBug624UA/1.0")).unwrap();
    assert!(is_true(&rt, "navigator.userAgent === 'LumenBug624UA/1.0'"));
    assert!(is_true(&rt, "Object.getOwnPropertyNames(navigator).length === 0"));
}
