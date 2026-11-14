//! BUG-1792 — `HTMLCollection` had no `Symbol.iterator`: every real engine
//! makes the interface iterable (Blink/WebKit declare `iterable<Element>`,
//! Gecko since FF 51), but `[...el.children]`/`for..of` here threw
//! `TypeError: e.children is not iterable` (lenta.ru player code). Covers the
//! spread/for-of/Array.from/forEach paths over `children` on the interfaces
//! where ParentNode children come back as a collection: element (HTML, SVG,
//! shadow-host), `ShadowRoot`, `DocumentFragment` (`template.content`),
//! `Document`, plus a static `getElementsByTagName` collection.

#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false, None)
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
fn children_spread_and_for_of_on_html_svg_shadow_elements() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var div = document.createElement('div');
var a = div.appendChild(document.createElement('a'));
var b = div.appendChild(document.createElement('b'));
var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
var rect = svg.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'rect'));
var host = document.createElement('section');
var child = host.appendChild(document.createElement('p'));
var ok = true;
[div, svg, host].forEach(function(el) {
    // spread + for..of are the two shapes lenta's minimised code uses
    ok = ok && Array.isArray([...el.children]) && el.children.length >= 1;
    for (var x of el.children) ok = ok && x instanceof Element;
    ok = ok && Array.from(el.children)[0] === el.children[0];
});
[...div.children][0] === a && [...div.children][1] === b
    && [...svg.children][0] === rect && [...host.children][0] === child && ok
"#
    ));
}

#[test]
fn children_iterable_on_shadowroot_template_content_document() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var host = document.createElement('div');
var root = host.attachShadow({ mode: 'open' });
var shadowChild = root.appendChild(document.createElement('span'));
var tmpl = document.createElement('template');
var fragChild = tmpl.content.appendChild(document.createElement('i'));
var ok = [...root.children][0] === shadowChild
    && [...tmpl.content.children][0] === fragChild
    && Array.isArray([...document.children])
    && [...document.children].length === document.children.length;
var light = host.appendChild(document.createElement('span'));
var col = host.getElementsByTagName('span');
var counted = 0;
for (var c of col) counted++;
[...col][0] === light && counted === 1 && ok
"#
    ));
}

#[test]
fn collection_iterator_protocol_matches_nodelist() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var div = document.createElement('div');
div.appendChild(document.createElement('a'));
div.appendChild(document.createElement('b'));
var col = div.children;
col instanceof HTMLCollection
    && typeof col.values === 'function' && typeof col.keys === 'function'
    && typeof col.entries === 'function' && typeof col.forEach === 'function'
    && [...col.entries()][0][0] === 0 && [...col.entries()][0][1] === col[0]
    && [...col.entries()].length === 2
    && col.keys().next().value === 0
    && [...col.values()].length === 2
    && col[Symbol.iterator]().next().value === col[0]
    // interface members are non-enumerable: for-in sees indices only (BUG-323)
    && (function() { var s = []; for (var p in col) s.push(p); return s.join(',') === '0,1'; })()
"#
    ));
}
