//! BUG-1167 — a custom element *is* its wrapper object: the class prototype
//! and whatever its constructor stored on `this` live there, not in the DOM.
//! The node-wrapper cache holds `WeakRef`s (GAP-P3GCJSDOM), so once nothing
//! in script referenced an upgraded element, a GC let the cache rebuild a
//! plain `HTMLElement` for the same node — Polymer's
//! `attributeChangedCallback` then ran on an object without
//! `_attributeToProperty` (youtube). Also covers the upgrade reactions
//! HTML LS §4.13.5 "upgrade an element" steps 4-5 require.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
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

/// An element built by `createElement` for a defined tag, inserted, and
/// then left without a single script reference keeps its class and its
/// constructor state across a GC.
#[test]
fn constructed_element_keeps_class_and_state_after_gc() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "class XKeep extends HTMLElement { \
             constructor() { super(); this._state = 42; } \
             ping() { return 'pong'; } \
         } \
         customElements.define('x-keep', XKeep); \
         (function() { document.body.appendChild(document.createElement('x-keep')); })();",
    )
    .unwrap();
    rt.force_gc_for_testing();
    assert_eq!(
        s(&rt, "(function(e) { return [e instanceof XKeep, e._state, e.ping()].join(); })(document.querySelector('x-keep'))"),
        "true,42,pong"
    );
}

/// The youtube shape: markup parsed before `define`, upgraded by `define`,
/// then an observed attribute changes after a GC. The callback must run on
/// the constructed instance, and the upgrade itself must have queued
/// `attributeChangedCallback` for the attribute already present, then
/// `connectedCallback` — both after the constructor.
#[test]
fn upgraded_element_callbacks_run_on_the_instance_after_gc() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.getElementById('main').innerHTML = '<x-late foo=\"a\" bar=\"z\"></x-late>'; \
         var log = []; \
         class XLate extends HTMLElement { \
             static get observedAttributes() { return ['foo']; } \
             constructor() { super(); this._ready = true; log.push('ctor'); } \
             _mark() { return this instanceof XLate && this._ready === true; } \
             attributeChangedCallback(n, o, v) { log.push('attr:' + n + ':' + o + ':' + v + ':' + this._mark()); } \
             connectedCallback() { log.push('conn:' + this._mark()); } \
         } \
         customElements.define('x-late', XLate);",
    )
    .unwrap();
    assert_eq!(s(&rt, "log.join('|')"), "ctor|attr:foo:null:a:true|conn:true");
    rt.force_gc_for_testing();
    assert_eq!(
        s(&rt, "(document.querySelector('x-late').setAttribute('foo', 'b'), log.slice(3).join('|'))"),
        "attr:foo:a:b:true"
    );
    // Re-insertion after GC neither re-runs the constructor nor loses `this`.
    assert_eq!(
        s(&rt, "(function(e) { document.body.appendChild(e); return log.slice(4).join('|'); })(document.querySelector('x-late'))"),
        "conn:true"
    );
}

/// A constructor that throws leaves the element "failed" (§4.13.5 upgrade
/// step 8): no connected/attributeChanged reactions for it, ever, and no
/// second construction attempt on re-insertion.
#[test]
fn failed_upgrade_gets_no_reactions() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.getElementById('main').innerHTML = '<x-bad foo=\"a\"></x-bad>'; \
         var log = []; \
         class XBad extends HTMLElement { \
             static get observedAttributes() { return ['foo']; } \
             constructor() { super(); log.push('ctor'); throw new Error('boom'); } \
             attributeChangedCallback() { log.push('attr'); } \
             connectedCallback() { log.push('conn'); } \
         } \
         customElements.define('x-bad', XBad);",
    )
    .unwrap();
    assert_eq!(
        s(&rt, "(function(e) { e.setAttribute('foo', 'b'); document.body.appendChild(e); return log.join('|'); })(document.querySelector('x-bad'))"),
        "ctor"
    );
}

/// ShadyDOM's init (`webcomponents-sd.js`, youtube) calls the captured
/// `EventTarget.prototype.addEventListener` on `window`; it threw on the
/// missing `_listeners`, the init stopped before `window.ShadowRoot = …`, and
/// every later `attachShadow` died on `b.Aa is not a function`. The three
/// EventTarget methods must reach the target's own listener store, both ways
/// round, for the window, the document and a node.
#[test]
fn event_target_prototype_methods_work_on_window_document_and_nodes() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ET = EventTarget.prototype, hits = []; \
         function rec(tag) { return function(e) { hits.push(tag + ':' + e.type); }; } \
         ET.addEventListener.call(window, 'x-win', rec('win')); \
         ET.addEventListener.call(document, 'x-doc', rec('doc')); \
         var el = document.getElementById('main'); \
         ET.addEventListener.call(el, 'x-el', rec('el')); \
         window.dispatchEvent(new Event('x-win')); \
         document.dispatchEvent(new Event('x-doc')); \
         el.dispatchEvent(new Event('x-el')); \
         var own = rec('own'); \
         el.addEventListener('x-own', own); \
         ET.dispatchEvent.call(el, new Event('x-own')); \
         ET.removeEventListener.call(el, 'x-own', own); \
         el.dispatchEvent(new Event('x-own'));",
    )
    .unwrap();
    assert_eq!(s(&rt, "hits.join('|')"), "win:x-win|doc:x-doc|el:x-el|own:x-own");
    // A pure-JS target keeps its own store.
    assert_eq!(
        s(&rt, "(function() { var t = new EventTarget(), n = 0; \
                 t.addEventListener('p', function() { n++; }); \
                 ET.dispatchEvent.call(t, new Event('p')); return n; })()"),
        "1"
    );
}
