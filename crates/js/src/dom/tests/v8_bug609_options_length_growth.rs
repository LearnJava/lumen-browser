//! BUG-609 — `HTMLOptionsCollection.prototype.length` setter (and its
//! `HTMLSelectElement.prototype.length` twin, spec-identical per HTML LS
//! §4.10.7) never implemented the growth path: an in-range `N` larger than
//! the current length left the collection unchanged instead of appending
//! bare `<option>` elements up to `N`. Out-of-range rejection (`N < 0` or
//! `N > 100000`) already worked and must keep working.
//!
//! The WPT source (`select/options-length-too-large.html`) grows to
//! `100000`/`100002` — well past this engine's pre-existing
//! `lumen_dom::MAX_DOM_NODES` arena cap (50 000, BUG-418), which is an
//! unrelated architectural limit, not part of this defect. These tests
//! exercise the same growth algorithm at a magnitude the arena can hold.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

const SETUP: &str = "var s = document.createElement('select'); \
                      s.appendChild(document.createElement('option')); \
                      s.appendChild(document.createElement('option')); \
                      s.appendChild(document.createElement('option'));";

#[test]
fn options_length_setter_ignores_out_of_range_values() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval("s.options.length = -1;").unwrap();
    assert!(is_true(&rt, "s.options.length === 3"));
    rt.eval("s.options.length = 100001;").unwrap();
    assert!(is_true(&rt, "s.options.length === 3"));
    rt.eval("s.options.length = Number.MAX_SAFE_INTEGER;").unwrap();
    assert!(is_true(&rt, "s.options.length === 3"));
}

#[test]
fn options_length_setter_grows_by_appending_bare_options() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval("s.options.length = 40;").unwrap();
    assert!(is_true(&rt, "s.options.length === 40"));
    rt.eval("s.appendChild(new Option()); s.appendChild(new Option());")
        .unwrap();
    assert!(is_true(&rt, "s.options.length === 42"));
    rt.eval("s.options.length = 41;").unwrap();
    assert!(is_true(&rt, "s.options.length === 41"));
}

/// `select.length = N` runs the same length-setting algorithm.
#[test]
fn select_length_setter_grows_too() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval("s.length = 5;").unwrap();
    assert!(is_true(&rt, "s.length === 5 && s.options.length === 5"));
    rt.eval("s.length = 2;").unwrap();
    assert!(is_true(&rt, "s.length === 2"));
}
