//! BUG-1440: `ParentNode.append/prepend/replaceChildren` и
//! `ChildNode.before/after/replaceWith` раскрывают `DocumentFragment` в его детей
//! (DOM LS §4.2.6 «convert nodes into a node»), а не вставляют сам фрагмент.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

/// `main` = `<div><b id=b></b></div>` в body, `frag()` — свежий фрагмент `<i></i><u></u>`.
const SETUP: &str = "\
    var main = document.createElement('div'); document.body.appendChild(main);\
    var b = document.createElement('b'); b.id = 'b'; main.appendChild(b);\
    function frag() { var f = document.createDocumentFragment();\
        f.appendChild(document.createElement('i')); f.appendChild(document.createElement('u')); return f; }\
    function tags(n) { return Array.prototype.map.call(n.childNodes, function(c) { return c.nodeName; }).join(); }";

fn run(expr: &str) -> lumen_core::JsValue {
    let rt = rt();
    rt.eval(SETUP).unwrap();
    rt.eval(expr).unwrap()
}

fn s(v: &str) -> lumen_core::JsValue {
    lumen_core::JsValue::String(v.into())
}

#[test]
fn append_expands_fragment() {
    assert_eq!(run("var f = frag(); main.append(f); tags(main) + '|' + f.childNodes.length"), s("B,I,U|0"));
}

#[test]
fn prepend_expands_fragment() {
    assert_eq!(run("main.prepend(frag(), 'x'); tags(main)"), s("I,U,#text,B"));
}

#[test]
fn before_and_after_expand_fragment() {
    assert_eq!(run("b.before(frag()); b.after(frag()); tags(main)"), s("I,U,B,I,U"));
}

#[test]
fn replace_with_expands_fragment() {
    assert_eq!(run("b.replaceWith(frag()); tags(main)"), s("I,U"));
}

#[test]
fn replace_children_expands_fragment() {
    assert_eq!(run("main.replaceChildren(frag()); tags(main)"), s("I,U"));
}

#[test]
fn fragment_into_fragment() {
    assert_eq!(
        run("var t = document.createDocumentFragment(); t.append(frag(), frag()); \
             var c = t.childElementCount; t.prepend(frag()); c + '|' + tags(t)"),
        s("4|I,U,I,U,I,U")
    );
}

#[test]
fn shadow_root_expands_fragment() {
    assert_eq!(
        run("var sr = main.attachShadow({mode: 'open'}); sr.append(frag()); sr.prepend(frag()); tags(sr)"),
        s("I,U,I,U")
    );
}

#[test]
fn strings_and_nodes_still_work() {
    assert_eq!(run("main.append('t', document.createElement('p')); tags(main)"), s("B,#text,P"));
}
