//! BUG-954 — `appendChild`/`insertBefore`/`select.add` of an inclusive ancestor
//! of the parent must throw `HierarchyRequestError` (DOM §4.2.3) instead of
//! building a cycle that hangs the engine.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn bool_eval(script: &str) -> bool {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        other => panic!("expected bool, got {other:?}: {script}"),
    }
}

#[test]
fn append_child_of_ancestor_throws_and_leaves_tree_intact() {
    assert!(bool_eval(
        r#"
var a = document.createElement('div'), b = document.createElement('div');
a.appendChild(b);
var n1 = '', n2 = '', n3 = '';
try { b.appendChild(a); } catch (e) { n1 = e.name; }
try { a.appendChild(a); } catch (e) { n2 = e.name; }
try { b.insertBefore(a, null); } catch (e) { n3 = e.name; }
n1 === 'HierarchyRequestError' && n2 === 'HierarchyRequestError' && n3 === 'HierarchyRequestError'
  && b.parentNode === a && a.parentNode === null
"#
    ));
}

#[test]
fn insert_before_and_select_add_of_ancestor_throw() {
    assert!(bool_eval(
        r#"
var opt = document.createElement('option'), sel = document.createElement('select');
opt.appendChild(sel);
var ref = document.createElement('span');
sel.appendChild(ref);
var n1 = '', n2 = '';
try { sel.insertBefore(opt, ref); } catch (e) { n1 = e.name; }
try { sel.add(opt); } catch (e) { n2 = e.name; }
n1 === 'HierarchyRequestError' && n2 === 'HierarchyRequestError' && sel.parentNode === opt
"#
    ));
}
