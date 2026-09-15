//! BUG-576 — `HTMLOptionsCollection.prototype.add(element, before)` (HTML LS
//! §4.10.7). Mirrors `HTMLSelectElement.prototype.add` on `select.options`
//! itself: appends when `before` is omitted/null, inserts before an index or
//! an `<option>` reference otherwise, and delegates to the correct `<select>`
//! even when the collection was read before any option existed.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

#[test]
fn add_is_a_function_on_the_collection() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var sel = document.createElement('select');
typeof sel.options.add === 'function'
"#
    ));
}

#[test]
fn add_appends_when_before_is_omitted() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var sel = document.createElement('select');
var a = document.createElement('option'); a.text = 'a';
var b = document.createElement('option'); b.text = 'b';
sel.options.add(a);
sel.options.add(b);
sel.options.length === 2 && sel.options[0] === a && sel.options[1] === b
"#
    ));
}

#[test]
fn add_inserts_before_an_index() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var sel = document.createElement('select');
var a = document.createElement('option'); a.text = 'a';
var b = document.createElement('option'); b.text = 'b';
sel.options.add(a);
sel.options.add(b, 0);
sel.options.length === 2 && sel.options[0] === b && sel.options[1] === a
"#
    ));
}

#[test]
fn add_inserts_before_an_option_reference() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var sel = document.createElement('select');
var a = document.createElement('option'); a.text = 'a';
var b = document.createElement('option'); b.text = 'b';
sel.appendChild(a);
sel.options.add(b, a);
sel.options.length === 2 && sel.options[0] === b && sel.options[1] === a
"#
    ));
}

#[test]
fn add_works_on_a_collection_read_before_any_option_existed() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var sel = document.createElement('select');
var options = sel.options;
var a = document.createElement('option'); a.text = 'a';
options.add(a);
sel.options.length === 1 && sel.options[0] === a
"#
    ));
}

#[test]
fn add_adds_option_groups_too() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var sel = document.createElement('select');
var g = document.createElement('optgroup');
var a = document.createElement('option'); a.text = 'a';
g.appendChild(a);
sel.options.add(g);
sel.options.length === 1 && sel.options[0] === a
"#
    ));
}
