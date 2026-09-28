//! BUG-1123 — `Node : EventTarget` (DOM §4.4), `Window : EventTarget`
//! (HTML §7.2), `XMLHttpRequestEventTarget : EventTarget` (XHR §3.1).
//! `Node.prototype` inherited straight from `Object.prototype`, the window and
//! the document carried their own listener methods and XHR had a private base:
//! Meta's hyperion (whatsapp) reported "Invalid prototype chain", and ShadyDOM
//! (youtube) copied `EventTarget.prototype`'s descriptors into
//! `__shady_native_*` names that no node reached.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// Evaluates `expr`; an exception comes back as `THROW:<message>`.
fn s(rt: &V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.message; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

/// Names of the constructors along `obj`'s prototype chain.
const CHAIN: &str = "function chain(o) { var out = []; \
     for (var p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) \
         out.push(Object.prototype.hasOwnProperty.call(p, 'constructor') ? p.constructor.name : '?'); \
     return out.join(','); }";

#[test]
fn nodes_window_and_xhr_inherit_event_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(CHAIN).unwrap();
    assert_eq!(s(&rt, "chain(Node.prototype)"), "EventTarget,Object");
    assert_eq!(s(&rt, "Object.getPrototypeOf(Node) === EventTarget"), "true");
    assert_eq!(
        s(&rt, "[document.getElementById('main'), document, document.createTextNode('t'), window, \
                  new XMLHttpRequest(), new XMLHttpRequest().upload] \
                  .map(function(t) { return t instanceof EventTarget; }).join()"),
        "true,true,true,true,true,true"
    );
    assert_eq!(
        s(&rt, "chain(new XMLHttpRequest())"),
        "XMLHttpRequest,XMLHttpRequestEventTarget,EventTarget,Object"
    );
}

/// The three methods every target reaches are the `EventTarget.prototype`
/// ones — no own or intermediate copy anywhere on the way.
#[test]
fn event_target_methods_are_shared_by_identity() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function() { var ET = EventTarget.prototype, host = document.getElementById('main'); \
                 var sr = host.attachShadow({ mode: 'open' }); \
                 return [window, document, host, sr, document.createComment('c'), new XMLHttpRequest()] \
                     .map(function(t) { return ['addEventListener', 'removeEventListener', 'dispatchEvent'] \
                         .every(function(n) { return t[n] === ET[n]; }); }).join(); })()"),
        "true,true,true,true,true,true"
    );
}

/// The BUG-1123 repro: the captured prototype method on window, document and
/// an element, then a click that reaches the listener registered that way.
#[test]
fn captured_prototype_method_registers_and_delivers() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var r = {}, f = function() { r.fired = (r.fired || 0) + 1; }; \
         [['window', window], ['document', document], ['element', document.getElementById('main')]] \
         .forEach(function(p) { \
             try { EventTarget.prototype.addEventListener.call(p[1], 'focus', f, true); r['add_' + p[0]] = 'ok'; } \
             catch (e) { r['add_' + p[0]] = String(e); } }); \
         var d = document.getElementById('main'); \
         EventTarget.prototype.addEventListener.call(d, 'click', f); d.click();",
    )
    .unwrap();
    assert_eq!(
        s(&rt, "[r.add_window, r.add_document, r.add_element, r.fired].join()"),
        "ok,ok,ok,1"
    );
}

/// ShadyDOM's shape: copy `EventTarget.prototype`'s descriptors under new
/// names on that same prototype, then call them on a node and on the window.
#[test]
fn descriptors_copied_onto_event_target_prototype_reach_nodes() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "['addEventListener', 'removeEventListener', 'dispatchEvent'].forEach(function(n) { \
             Object.defineProperty(EventTarget.prototype, '__native_' + n, \
                 Object.getOwnPropertyDescriptor(EventTarget.prototype, n)); }); \
         var hits = [], el = document.getElementById('main'); \
         el.__native_addEventListener('x-a', function(e) { hits.push('el:' + e.type); }); \
         el.__native_dispatchEvent(new Event('x-a')); \
         window.__native_addEventListener('x-b', function(e) { hits.push('win:' + e.type); }); \
         window.__native_dispatchEvent(new Event('x-b'));",
    )
    .unwrap();
    assert_eq!(s(&rt, "hits.join('|')"), "el:x-a|win:x-b");
}

/// The window keeps its engine semantics behind the shared methods: the bare
/// global call, listeners on the document and the window reached by a
/// bubbling event, and removal.
#[test]
fn window_and_document_keep_their_dispatch() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = []; \
         addEventListener('x-bare', function(e) { log.push('bare:' + (this === window)); }); \
         dispatchEvent(new Event('x-bare')); \
         window.addEventListener('x-up', function() { log.push('win-up'); }); \
         document.addEventListener('x-up', function() { log.push('doc-up'); }); \
         document.getElementById('main').dispatchEvent(new Event('x-up', { bubbles: true })); \
         var gone = function() { log.push('removed'); }; \
         window.addEventListener('x-rm', gone); removeEventListener('x-rm', gone); \
         window.dispatchEvent(new Event('x-rm'));",
    )
    .unwrap();
    assert_eq!(s(&rt, "log.join('|')"), "bare:true|doc-up|win-up");
    assert_eq!(s(&rt, "Object.prototype.hasOwnProperty.call(window, 'addEventListener')"), "false");
}

/// A JS-only node never ran the `EventTarget` constructor; it still gets a
/// working listener store instead of throwing.
#[test]
fn detached_js_only_node_gets_a_listener_store() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function() { var t = new Text('x'), n = 0; \
                 t.addEventListener('p', function() { n++; }); \
                 t.dispatchEvent(new Event('p')); return n; })()"),
        "1"
    );
}

/// XHR listeners now come from `EventTarget.prototype`: `once`,
/// `handleEvent` objects and the prototype method called directly.
#[test]
fn xhr_listeners_use_event_target_semantics() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function() { var x = new XMLHttpRequest(), log = []; \
                 x.addEventListener('load', function() { log.push('once'); }, { once: true }); \
                 x.addEventListener('load', { handleEvent: function() { log.push('obj'); } }); \
                 EventTarget.prototype.addEventListener.call(x, 'load', function() { log.push('proto'); }); \
                 x.onload = function() { log.push('on'); }; \
                 x.dispatchEvent(new Event('load')); x.dispatchEvent(new Event('load')); \
                 return log.join(); })()"),
        "once,obj,proto,on,obj,proto,on"
    );
}
