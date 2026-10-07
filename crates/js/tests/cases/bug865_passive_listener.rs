//! BUG-865 — `addEventListener` option `passive` (DOM §2.7/§2.8): inside a
//! passive listener `preventDefault()` is a no-op, and `touchstart`/
//! `touchmove`/`wheel`/`mousewheel` on window/document/body default to passive.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_js::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse("<html><body><div id=a></div></body></html>")));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn flag(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        other => panic!("expected bool, got {other:?}: {script}"),
    }
}

/// `defaultPrevented` after dispatching a cancelable `type` at `target`
/// with one listener registered through `opts`.
fn prevented(target: &str, type_: &str, opts: &str) -> bool {
    let rt = rt();
    flag(
        &rt,
        &format!(
            "(function(){{ var t = {target}; var e = new Event('{type_}', {{cancelable:true, bubbles:true}});
               t.addEventListener('{type_}', function(ev){{ ev.preventDefault(); }}, {opts});
               t.dispatchEvent(e); return e.defaultPrevented; }})()"
        ),
    )
}

#[test]
fn explicit_passive_ignores_prevent_default() {
    assert!(!prevented("document.body", "click", "{passive:true}"));
    assert!(prevented("document.body", "click", "{passive:false}"));
    assert!(prevented("document.body", "click", "{}"));
}

#[test]
fn scroll_blocking_types_default_passive_on_window_document_body() {
    for t in ["window", "document", "document.body"] {
        for ty in ["touchstart", "touchmove", "wheel", "mousewheel"] {
            assert!(!prevented(t, ty, "{}"), "{t} {ty} must default to passive");
            assert!(prevented(t, ty, "{passive:false}"), "{t} {ty} passive:false");
        }
    }
}

#[test]
fn other_targets_and_types_are_not_passive_by_default() {
    assert!(prevented("document.createElement('div')", "wheel", "{}"));
    assert!(prevented("document.body", "keydown", "{}"));
}

#[test]
fn passive_listener_is_removable_by_original_function() {
    let rt = rt();
    assert!(flag(
        &rt,
        "(function(){ var n = 0; function f(){ n++; }
           document.body.addEventListener('wheel', f, {passive:true});
           window.addEventListener('wheel', f, {passive:true});
           document.body.removeEventListener('wheel', f);
           window.removeEventListener('wheel', f);
           document.body.dispatchEvent(new Event('wheel', {bubbles:true}));
           return n === 0; })()"
    ));
}

#[test]
fn passive_flag_is_scoped_to_the_listener() {
    let rt = rt();
    assert!(flag(
        &rt,
        "(function(){ var e = new Event('click', {cancelable:true, bubbles:true});
           document.body.addEventListener('click', function(ev){ ev.preventDefault(); }, {passive:true});
           document.body.addEventListener('click', function(ev){ ev.preventDefault(); });
           document.body.dispatchEvent(e); return e.defaultPrevented; })()"
    ));
}

#[test]
fn passive_option_getter_is_read() {
    let rt = rt();
    assert!(flag(
        &rt,
        "(function(){ var s = false;
           document.body.addEventListener('x', null, {get passive(){ s = true; return false; }});
           document.body.addEventListener('x', function(){}, {get passive(){ s = true; return false; }});
           return s; })()"
    ));
}
