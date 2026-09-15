//! BUG-589 — `window` was not a proper WebIDL exotic object: it lacked
//! `Symbol.toStringTag` (both on itself and on the "global scope polluter"
//! object in its prototype chain), and indexed `[[DefineOwnProperty]]`/
//! `[[Set]]` silently accepted any numeric key instead of rejecting an
//! unsupported index — `html/browsers/the-window-object/
//! window-prototype-chain.html` and `window-indexed-properties-strict.html`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn runtime_with(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.com/page.html", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_of(rt: &V8JsRuntime, code: &str) -> bool {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Bool(b) => b,
        other => panic!("{code}: expected a bool, got {other:?}"),
    }
}

fn string_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

/// `window-prototype-chain.html`: `Object.prototype.toString.call(window)`
/// must name the interface, and `window`'s own prototype must literally be
/// `Window.prototype`.
#[test]
fn window_reports_as_window() {
    let rt = runtime_with(make_doc());
    assert_eq!(
        string_of(&rt, "Object.prototype.toString.call(window)"),
        "[object Window]"
    );
    assert!(bool_of(&rt, "Object.getPrototypeOf(window) === Window.prototype"));
}

/// The "global scope polluter" object — one level above `Window.prototype`
/// in the chain — must be tagged `WindowProperties`, and sit directly on top
/// of `EventTarget.prototype`, which sits directly on top of
/// `Object.prototype` (the full chain HTML LS §7.3.3/WebIDL §3.9 describe).
#[test]
fn global_scope_polluter_is_tagged_and_chained_to_event_target() {
    let rt = runtime_with(make_doc());
    assert_eq!(
        string_of(
            &rt,
            "Object.prototype.toString.call(Object.getPrototypeOf(Object.getPrototypeOf(window)))"
        ),
        "[object WindowProperties]"
    );
    assert!(bool_of(
        &rt,
        "Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(window))) === EventTarget.prototype"
    ));
    assert!(bool_of(
        &rt,
        "Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(window)))) === Object.prototype"
    ));
}

/// Being reachable via `EventTarget.prototype` in the chain must not disturb
/// `window`'s own `addEventListener`/`dispatchEvent` (own properties shadow
/// the inherited ones) — and `window` must now correctly report as an
/// `EventTarget` instance, which it did not before this fix.
#[test]
fn window_is_still_a_working_event_target() {
    let rt = runtime_with(make_doc());
    assert!(bool_of(&rt, "window instanceof EventTarget"));
    assert!(bool_of(&rt, "typeof window.addEventListener === 'function'"));
    assert_eq!(
        string_of(
            &rt,
            r#"(function() {
                var seen = 'no';
                window.addEventListener('probe589', function() { seen = 'yes'; });
                window.dispatchEvent(new Event('probe589'));
                return seen;
            })()"#
        ),
        "yes"
    );
}

/// `window-indexed-properties-strict.html`'s self-contained assertion
/// (independent of iframe support): `2**32-2` is a valid array index with no
/// supported browsing context behind it, so `[[DefineOwnProperty]]` must
/// fail — surfacing as a strict-mode `TypeError` for a bare assignment, and
/// as `false` for the non-throwing `Reflect` forms.
#[test]
fn unsupported_indexed_property_rejects_in_strict_mode() {
    let rt = runtime_with(make_doc());
    assert_eq!(
        string_of(
            &rt,
            r#"(function() {
                'use strict';
                try { window[4294967294] = 1; return 'no throw'; }
                catch (e) { return e instanceof TypeError ? 'TypeError' : String(e); }
            })()"#
        ),
        "TypeError"
    );
    assert!(!bool_of(&rt, "Reflect.set(window, 4294967294, 2)"));
    assert!(!bool_of(&rt, "Reflect.defineProperty(window, 4294967294, { value: 3 })"));
    assert!(bool_of(&rt, "window[4294967294] === undefined"));
    assert!(!bool_of(&rt, "4294967294 in window"));
    // Deleting an unsupported (and therefore absent) index still succeeds —
    // no indexed deleter needed, matches plain-object semantics.
    assert!(bool_of(&rt, "delete window[4294967294]"));
}

/// The indexed handler must only intercept real array indices (`[0,
/// 2**32-2]`) — `2**32-1` is not one (falls back to a plain string-keyed
/// property) and negative numbers never reach the indexed path at all, so
/// both must keep behaving like ordinary properties, unaffected by the
/// strict-mode rejection above.
#[test]
fn borderline_non_index_keys_are_unaffected() {
    let rt = runtime_with(make_doc());
    rt.eval("window[4294967295] = 1;").unwrap();
    assert!(bool_of(&rt, "window[4294967295] === 1"));
    rt.eval("window[-1] = 'foo';").unwrap();
    assert_eq!(string_of(&rt, "window[-1]"), "foo");
}
