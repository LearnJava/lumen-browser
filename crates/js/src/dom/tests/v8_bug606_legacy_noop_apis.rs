//! BUG-606 — obsolete-but-conforming legacy compat APIs missing entirely:
//! `document.clear()`/`captureEvents()`/`releaseEvents()`,
//! `window.captureEvents()`/`releaseEvents()`, `document.applets`,
//! `HTMLScriptElement.event`/`.htmlFor`. (`document.all`'s `[[IsHTMLDDA]]`
//! semantics are out of scope — split off as BUG-1057/GAP-DOCALLDDA, they
//! need a `rusty_v8` binding extension no JS-only fix can provide.)

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn document_and_window_legacy_methods_are_callable_noops() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof document.clear === 'function'"));
    assert!(is_true(&rt, "typeof document.captureEvents === 'function'"));
    assert!(is_true(&rt, "typeof document.releaseEvents === 'function'"));
    assert!(is_true(&rt, "typeof window.captureEvents === 'function'"));
    assert!(is_true(&rt, "typeof window.releaseEvents === 'function'"));
    assert!(is_true(&rt, "document.clear() === undefined"));
    assert!(is_true(&rt, "document.captureEvents() === undefined"));
    assert!(is_true(&rt, "document.releaseEvents() === undefined"));
    assert!(is_true(&rt, "window.captureEvents() === undefined"));
    assert!(is_true(&rt, "window.releaseEvents() === undefined"));
}

#[test]
fn document_applets_is_an_always_empty_live_html_collection() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        "document.applets instanceof HTMLCollection"
    ));
    assert!(is_true(&rt, "document.applets.length === 0"));
    rt.eval("document.body.appendChild(document.createElement('div'));")
        .unwrap();
    assert!(is_true(
        &rt,
        "document.applets.length === 0",
    ));
}

#[test]
fn html_script_element_event_and_htmlfor_reflect_verbatim() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var s = document.createElement('script');").unwrap();
    assert!(is_true(&rt, "s.event === ''"));
    assert!(is_true(&rt, "s.htmlFor === ''"));
    rt.eval("s.setAttribute('event', 'onclick'); s.setAttribute('for', 'window');")
        .unwrap();
    assert!(is_true(&rt, "s.event === 'onclick'"));
    assert!(is_true(&rt, "s.htmlFor === 'window'"));
    rt.eval("s.event = 'oncustom'; s.htmlFor = 'document';").unwrap();
    assert!(is_true(&rt, "s.getAttribute('event') === 'oncustom'"));
    assert!(is_true(&rt, "s.getAttribute('for') === 'document'"));
}
