//! V8 port of the "Core DOM basics" test row (S12b-24-core), the first slice of
//! the `dom.rs` test-monolith migration described in
//! `docs/tasks/ph3-v8-migration.md`: console/SVG/node identity/`self`&`window`
//! aliasing, Canvas 2D, `getElementById`/`querySelector`/attributes/text content/
//! the `Image` constructor, `alert`/`print`, timers + `scheduler.postTask`, and
//! the History API.
//!
//! The 99 tests moved here verbatim from the QuickJS monolith above — their bodies
//! are `rt.eval(...)` plus a handful of `update_layout_rects`/`flush_canvas_updates`/
//! `register_img_bitmaps`/`take_*` calls that `V8JsRuntime` mirrors one-for-one — so
//! the only edit was which runtime the fixture builds. One assertion did change:
//! see `canvas_get_context_webgl_returns_functional_context`.
//!
//! Gated on `v8-backend` like every other ported module (see `csp.rs`,
//! `pointer_capture.rs`): the QuickJS copies are gone, V8 is the default engine
//! (ADR-018) and carries the coverage from here on.

use super::*;
use crate::v8_runtime::V8JsRuntime;

mod canvas_interface_membership;
mod canvas_size_attributes;
mod canvas_object_model;
mod selectors_canvas_window;

/// V8 twin of [`super::runtime_with_dom`]: same fixture document, same
/// `install_dom` argument list (the two signatures are identical), same
/// `_LUMEN_EXTENSION_ACTIVE` pre-eval so `chrome.runtime` is present.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// Wrap raw RGBA8 test pixels in a shared `Arc<Image>` for `register_img_bitmaps`
/// (BUG-272 срез 20: the store shares the decoded `Arc<Image>` rather than an
/// eager RGBA8 copy). `data` is a `width × height` RGBA8 buffer. Moved down here
/// with the `drawImage(<img>)` tests — its only callers.
fn test_img_bitmap(width: u32, height: u32, data: Vec<u8>) -> Arc<lumen_image::Image> {
    Arc::new(lumen_image::Image {
        width,
        height,
        format: lumen_image::PixelFormat::Rgba8,
        data,
        icc_profile: None,
    })
}

/// V8 twin of [`super::runtime_with_url`]: the fixture document installed
/// against a concrete page URL, which is what the History API tests need
/// (`pushState` resolves relative URLs against it).
fn v8_runtime_with_url(url: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), url, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn console_log_does_not_crash() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("console.log('hello from test')").unwrap();
}

// BUG-243: dynamic SVG built via document.createElementNS must produce NATIVE
// arena nodes (carrying __nid__) so that appendChild attaches them to the Rust
// document tree and layout/paint can see them. The previous svg.rs override
// returned detached `new Ctor()` objects without __nid__, which native
// appendChild silently dropped — leaving script-built SVG invisible.
#[test]
fn create_element_ns_builds_native_svg_tree() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var NS = 'http://www.w3.org/2000/svg';\
                     var svg = document.createElementNS(NS, 'svg');\
                     var rect = document.createElementNS(NS, 'rect');\
                     svg.appendChild(rect);\
                     document.getElementById('main').appendChild(svg);\
                     typeof svg.__nid__ === 'number' && typeof rect.__nid__ === 'number' \
                       && document.querySelectorAll('svg').length === 1 \
                       && document.querySelectorAll('rect').length === 1",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// BUG-243: the repro page (docs/roadmap-svg-cleaves.html) builds its UI with
// ParentNode.append() (variadic, accepts strings) and clears the SVG via
// `while (svg.firstChild) svg.removeChild(svg.firstChild)`. Both were missing on
// native elements, so the page threw "not a function" before rendering. Verify
// append() attaches node+string children and firstChild/removeChild can clear them.
#[test]
fn element_append_and_first_child_round_trip() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var box = document.createElement('div');\
                     var a = document.createElement('span');\
                     var b = document.createElement('b');\
                     box.append(a, b);\
                     var built = box.firstChild.__nid__ === a.__nid__ && box.lastChild.__nid__ === b.__nid__;\
                     box.append('trailing text');\
                     var n = 0; while (box.firstChild) { box.removeChild(box.firstChild); if (++n > 20) break; }\
                     built && box.firstChild === null",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// BUG-291: repeated wraps of the same underlying node (via .lastChild,
// .parentElement, .children, etc.) must return the SAME JS object, not a
// fresh wrapper each time. `testharness.js`'s `Output.show_results` relies
// on `tbody.lastChild.lastChild.appendChild(...)` reading back the very
// node it just appended two statements earlier — with fresh wrappers each
// access, `tbody.lastChild` after appending a child-of-a-child came back
// stale/inconsistent and the nested `.lastChild` was `null`, throwing
// `TypeError: Cannot read properties of null (reading 'appendChild')` and
// aborting `notify_complete()` before the WPT result callback ran.
#[test]
fn repeated_node_access_returns_identical_wrapper() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var tbody = document.createElement('tbody');\
                     var tr = document.createElement('tr');\
                     var td = document.createElement('td');\
                     tr.appendChild(td);\
                     tbody.appendChild(tr);\
                     var identityHolds = tbody.lastChild === tr && tr.lastChild === td;\
                     var expando = tbody.lastChild;\
                     expando._probe = 'kept';\
                     var expandoSurvives = tbody.lastChild._probe === 'kept';\
                     var nested = tbody.lastChild.lastChild;\
                     var assertionsNode = document.createElement('div');\
                     var appended = nested !== null && (nested.appendChild(assertionsNode), true);\
                     identityHolds && expandoSurvives && appended",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// BUG-243: installing the SVG shim must not abort. It previously threw at
// `class SVGElement extends Element` because no global `Element` class exists,
// which killed the whole shim (and silently disabled SVG typed interfaces).
#[test]
fn svg_shim_installs_and_exposes_svg_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval("typeof window.SVGElement === 'function' && typeof window.SVGSVGElement === 'function'")
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// BUG-233: `self` must be defined as a global aliasing `window`
// (WindowOrWorkerGlobalScope). Webpack runtimes reference bare `self`;
// without this they throw `ReferenceError: self is not defined`.
#[test]
fn self_window_globalthis_are_the_same_object() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "typeof self !== 'undefined' \
                     && self === window \
                     && window.self === window \
                     && window.window === window \
                     && globalThis.self === window \
                     && window.top === window \
                     && window.parent === window \
                     && window.frames === window",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// BUG-233: a property stored on `self` must be visible through `window`
// and vice-versa, because webpack stores its chunk registry on `self`
// and later reads it back. They are the same object reference.
#[test]
fn self_and_window_share_property_storage() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "(self.webpackChunk = self.webpackChunk || []).push([1]); \
                     Array.isArray(window.webpackChunk) && window.webpackChunk.length === 1",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// BUG-280: `window` must literally BE the real QuickJS global object, not
// a plain object cross-referenced with `globalThis` via a fixed alias
// list. Any property assigned via `window.foo = ...` — including names
// not known in advance, e.g. testharness.js's dynamic `expose(fn, name)`
// (`window[name] = fn`) — must resolve as a bare, unqualified identifier.
#[test]
fn dynamic_window_property_is_bare_reachable() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "window.__bug280_probe = function() { return 42; }; \
                     typeof __bug280_probe === 'function' && __bug280_probe() === 42",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

// BUG-280: same for a property assigned via bare `self`, matching the
// real-browser invariant `self === window === globalThis` (same object).
#[test]
fn dynamic_self_property_is_bare_reachable() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "self.__bug280_probe2 = 'hi'; \
                     typeof __bug280_probe2 !== 'undefined' && __bug280_probe2 === 'hi' \
                     && window === globalThis",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_element_by_id_found() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.getElementById('main') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_element_by_id_not_found() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.getElementById('nonexistent') === null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_element_by_id_tag_name() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.getElementById('main').tagName")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("DIV".into()));
}

#[test]
fn query_selector_by_id() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('#main') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_by_class() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('.highlight') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_by_tag() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("document.querySelector('span') !== null").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// BUG-291: Element.querySelector(All) must be scoped to the calling
// element's descendants, and must therefore also work on a subtree that
// is not (yet) attached to the document — `document.querySelector` has no
// path to reach such nodes at all. Before the fix, `_lumen_query_selector`
// ignored `this` and always searched from `document.root()`, so this
// returned `null` and crashed `testharness.js`'s results renderer
// (`Output.show_results`) with `Cannot read properties of null`.
#[test]
fn element_query_selector_finds_descendant_in_detached_subtree() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var table = document.createElement('table'); \
                     var tbody = document.createElement('tbody'); \
                     table.appendChild(tbody); \
                     table.querySelector('tbody') === tbody",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_query_selector_all_finds_matches_in_detached_subtree() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var ul = document.createElement('ul'); \
                     ul.appendChild(document.createElement('li')); \
                     ul.appendChild(document.createElement('li')); \
                     ul.querySelectorAll('li').length",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(2.0));
}

// BUG-291: Element.querySelector must be scoped to *descendants* — it
// must not match the calling element itself, nor elements outside its
// subtree (the pre-fix implementation searched the whole document).
#[test]
fn element_query_selector_excludes_self_and_siblings() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var a = document.createElement('div'); a.id = 'scope'; \
                     var b = document.createElement('div'); b.id = 'outside'; \
                     document.body.appendChild(a); document.body.appendChild(b); \
                     a.querySelector('#scope') === null && a.querySelector('#outside') === null",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// BUG-291: repeated access to the same DOM node must yield the same JS
// wrapper object (`===` identity), matching real engines and required by
// `testharness.js`'s results renderer (`tbody.lastChild === row`-style
// checks). Before the fix, `_lumen_make_element` minted a fresh object on
// every call.
#[test]
fn repeated_node_access_yields_stable_identity() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var parent = document.createElement('div'); \
                     var child = document.createElement('span'); \
                     parent.appendChild(child); \
                     parent.lastChild === child && \
                     parent.firstChild === parent.lastChild && \
                     parent.children[0] === child && \
                     document.getElementById('main') === document.getElementById('main')",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── BUG-849: wrapper members live on a shared prototype ───────────────
// The interface used to be rebuilt per node (~250 own closures), which
// cost ~142 us per `createElement` and killed the process at 40 000
// nodes. These pin the properties that move made observable.

// The whole interface must now be inherited, not owned: `Object.keys`
// on a node is empty (matching a real engine) while every member still
// resolves and `instanceof` is unchanged.
#[test]
fn wrapper_members_are_inherited_not_own() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var el = document.createElement('div'); \
                     Object.keys(el).length === 0 && \
                     !Object.prototype.hasOwnProperty.call(el, 'tagName') && \
                     !Object.prototype.hasOwnProperty.call(el, 'onclick') && \
                     Object.prototype.hasOwnProperty.call(el, '__nid__') && \
                     el.tagName === 'DIV' && el.getAttribute('x') === null && \
                     el instanceof HTMLDivElement && el instanceof Element",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// Sharing the members must not share per-node state: the lazily built
// `classList`/`style`/`dataset`/`attributes` slots are still one per
// node and still stable under `===` across reads.
#[test]
fn wrapper_lazy_slots_are_per_node_and_stable() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var a = document.createElement('div'); \
                     var b = document.createElement('div'); \
                     a.style === a.style && a.classList === a.classList && \
                     a.dataset === a.dataset && a.attributes === a.attributes && \
                     a.style !== b.style && a.classList !== b.classList && \
                     (a.style.color = 'red', b.style.color === '' && \
                      a.getAttribute('style').indexOf('red') >= 0)",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// `on<type>` is one accessor pair per NAME now; the value behind it must
// still be per node, and an expando must still land on the instance.
#[test]
fn wrapper_on_handlers_and_expandos_stay_per_node() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var a = document.createElement('div'); \
                     var b = document.createElement('div'); \
                     var f = function() {}; \
                     a.onclick = f; a.mine = 7; \
                     a.onclick === f && b.onclick === null && \
                     a.mine === 7 && b.mine === undefined && \
                     Object.prototype.hasOwnProperty.call(a, 'mine')",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// A dialog's `returnValue` used to be a closure variable of the builder.
#[test]
fn wrapper_dialog_return_value_is_per_node() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var a = document.createElement('dialog'); \
                     var b = document.createElement('dialog'); \
                     a.returnValue === '' && (a.returnValue = 'ok', \
                     a.returnValue === 'ok' && b.returnValue === '')",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// The shared prototype sits BELOW the interface prototype, so a member
// defined by both still resolves to the wrapper's own — BUG-383's
// `select.remove(index)` is the canonical case.
#[test]
fn wrapper_members_still_shadow_the_interface_prototype() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var s = document.createElement('select'); \
                     var o = document.createElement('option'); \
                     s.appendChild(o); s.remove(0); \
                     s.children.length === 0 && \
                     s.remove !== HTMLSelectElement.prototype.remove",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// Text/Comment wrappers get the CharacterData half of the bundle and
// elements do not, exactly as the old per-node branch decided.
#[test]
fn wrapper_character_data_members_are_text_only() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "var t = document.createTextNode('hi'); \
                     var c = document.createComment('x'); \
                     var e = document.createElement('div'); \
                     t.data === 'hi' && t.nodeValue === 'hi' && \
                     (t.data = 'ho', t.textContent === 'ho') && \
                     c.data === 'x' && e.nodeValue === undefined && \
                     t instanceof Text && c instanceof Comment && \
                     t.onclick === undefined",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_content_get() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.getElementById('main').textContent")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("Hello".into()));
}

#[test]
fn text_content_set_mutates_dom() {
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval("document.getElementById('main').textContent = 'World';")
        .unwrap();
    drop(rt);
    let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
    // The div#main should now have a single text child "World".
    let body_id = find_element_by_tag(&doc, "body").unwrap();
    let div_id = doc.get(body_id).children[0];
    let text = collect_text_content(&doc, div_id);
    assert_eq!(text, "World");
}

#[test]
fn set_attribute_mutates_dom() {
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval("document.getElementById('main').setAttribute('data-x', '42');")
        .unwrap();
    drop(rt);
    let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
    let body_id = find_element_by_tag(&doc, "body").unwrap();
    let div_id = doc.get(body_id).children[0];
    assert_eq!(doc.get(div_id).get_attr("data-x"), Some("42"));
}

#[test]
fn get_attribute_returns_value() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.getElementById('main').getAttribute('id')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("main".into()));
}

#[test]
fn get_attribute_returns_null_for_missing() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.getElementById('main').getAttribute('data-missing') === null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_title_get() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("document.title").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("Test Page".into()));
}

#[test]
fn document_title_set() {
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval("document.title = 'New Title';").unwrap();
    drop(rt);
    let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
    let title_text = find_element_by_tag(&doc, "title")
        .map(|nid| collect_text_content(&doc, nid))
        .unwrap_or_default();
    assert_eq!(title_text, "New Title");
}

#[test]
fn document_body_not_null() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("document.body !== null").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

/// BUG-703: `document.head` was absent from the live `document` (only
/// `body`/`documentElement` existed), so webpack's chunk loader —
/// `document.head.appendChild(script)` — threw on every bundled site.
#[test]
fn document_head_is_the_head_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "'head' in document && document.head !== null \
                     && document.head.tagName === 'HEAD' \
                     && document.head.__nid__ === document.querySelector('head').__nid__",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

/// BUG-703 regression, in the exact shape the webpack runtime uses it:
/// a freshly created `<script>` appended to `document.head` must land
/// in the real tree.
#[test]
fn document_head_accepts_appended_script() {
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    let ok = rt
        .eval(
            "var s = document.createElement('script'); \
                     s.src = 'https://example.com/chunk.js'; \
                     document.head.appendChild(s); \
                     document.head.lastChild.tagName === 'SCRIPT' \
                     && document.querySelectorAll('head script').length === 1",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

/// BUG-703: `element.dataset` did not exist at all (only an SVG stub
/// returning `{}`), so `script.dataset.mmid = ...` threw mid-bootstrap.
#[test]
fn dataset_maps_data_attributes_both_ways() {
    let rt = v8_runtime_with_dom(make_doc());
    // read: data-foo-bar → dataset.fooBar
    let read = rt
        .eval(
            "var d = document.getElementById('main'); \
                     d.setAttribute('data-foo-bar', 'v1'); \
                     d.dataset.fooBar === 'v1' && ('fooBar' in d.dataset) \
                     && d.dataset.missing === undefined",
        )
        .unwrap();
    assert_eq!(read, lumen_core::JsValue::Bool(true));
    // write: dataset.mmId → data-mm-id, and delete removes the attribute
    let write = rt
        .eval(
            "var d = document.getElementById('main'); \
                     d.dataset.mmId = 'x7'; \
                     var set = d.getAttribute('data-mm-id') === 'x7'; \
                     var keys = Object.keys(d.dataset).sort().join(','); \
                     delete d.dataset.fooBar; \
                     set && keys === 'fooBar,mmId' && !d.hasAttribute('data-foo-bar') \
                       && d.dataset.mmId === 'x7'",
        )
        .unwrap();
    assert_eq!(write, lumen_core::JsValue::Bool(true));
    // identity is stable, like a browser's DOMStringMap
    let same = rt
        .eval("document.getElementById('main').dataset === document.getElementById('main').dataset")
        .unwrap();
    assert_eq!(same, lumen_core::JsValue::Bool(true));
}

/// BUG-414: the WPT `dataset` tests assert `instanceof DOMStringMap`,
/// and one of them asserts it for an SVG element — which used to hit
/// `svg.rs`'s `get dataset() { return {}; }` stub instead.
#[test]
fn dataset_is_a_domstringmap_on_html_and_svg() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var svg = document.createElementNS('http://www.w3.org/2000/svg', 'circle'); \
                     svg.setAttribute('data-r', '5'); \
                     var html = document.getElementById('main'); \
                     typeof DOMStringMap === 'function' \
                     && html.dataset instanceof DOMStringMap \
                     && svg.dataset instanceof DOMStringMap \
                     && svg.dataset.r === '5' \
                     && (svg.dataset.r = '9', svg.getAttribute('data-r') === '9')",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
    let ctor = rt
        .eval("(function(){ try { new DOMStringMap(); return false; } catch (e) { return e instanceof TypeError; } })()")
        .unwrap();
    assert_eq!(ctor, lumen_core::JsValue::Bool(true));
}

/// BUG-703: the detached-document half of the shim's Document split had
/// neither `head` nor `body` (see the `_lumen_build_detached_document`
/// vs live-`document` note in that function's comment).
#[test]
fn detached_document_exposes_head_and_body() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.implementation.createHTMLDocument('t'); \
                     d.head !== null && d.head.tagName === 'HEAD' \
                     && d.body !== null && d.body.tagName === 'BODY' \
                     && document.implementation.createDocument(null, '', null).head === null",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

/// BUG-415: the detached document had no `Node` mutation members at all
/// — `doc.removeChild(doc.documentElement)`, the opening line of most
/// WPT document tests, threw `is not a function`.
#[test]
fn detached_document_has_node_mutation_members() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.implementation.createHTMLDocument('t'); \
                     var root = d.documentElement; \
                     d.removeChild(root) === root && d.documentElement === null \
                     && d.body === null && d.hasChildNodes() === true \
                     && d.appendChild(root) === root && d.documentElement === root \
                     && d.lastChild === root && d.childElementCount === 1 \
                     && d.contains(d.body) && d.contains(root) && !d.contains(null) \
                     && (function() { \
                            var x = d.createElement('x'); \
                            return d.replaceChild(x, root) === root \
                                && d.documentElement === x \
                                && d.insertBefore(root, x) === root \
                                && d.documentElement === root \
                                && d.firstElementChild === root; \
                        })()",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
    let throws = rt
        .eval(
            "(function() { \
                        var d = document.implementation.createHTMLDocument('t'); \
                        try { d.removeChild(d.createElement('p')); return false; } \
                        catch (e) { return e.name === 'NotFoundError'; } \
                     })()",
        )
        .unwrap();
    assert_eq!(throws, lumen_core::JsValue::Bool(true));
}

/// BUG-415: `body` must be rooted at an HTML-namespace `html` element,
/// and `readyState`/`title`/the tree accessors existed nowhere.
#[test]
fn detached_document_body_scope_and_accessors() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.implementation.createHTMLDocument('hello'); \
                     d.removeChild(d.documentElement); \
                     var b = d.appendChild(d.createElement('body')); \
                     b.appendChild(d.createElement('frameset')); \
                     d.body === null \
                     && document.implementation.createHTMLDocument('t').readyState === 'complete'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
    let acc = rt
        .eval(
            "var d2 = document.implementation.createHTMLDocument('  hello   world '); \
                     d2.body.innerHTML = '<p id=\"a\" class=\"c\">x</p><span>y</span>'; \
                     d2.title === 'hello world' \
                     && d2.getElementById('a') !== null && d2.getElementById('a').tagName === 'P' \
                     && d2.getElementById('nope') === null \
                     && d2.querySelector('#a').tagName === 'P' \
                     && d2.querySelectorAll('p, span').length === 2 \
                     && d2.getElementsByTagName('span').length === 1 \
                     && d2.getElementsByTagName('html').length === 1 \
                     && d2.getElementsByClassName('c').length === 1 \
                     && (d2.title = 'set', d2.title === 'set')",
        )
        .unwrap();
    assert_eq!(acc, lumen_core::JsValue::Bool(true));
}

/// BUG-854: `<frame>` was an `HTMLElement` with no IDL attributes at
/// all, so a page could neither recognize it nor read its `src` — while
/// `<frameset>` next door already had its own interface (BUG-415).
/// `contentDocument`/`contentWindow` answer `null` here on purpose: the
/// sub-document registry (`frame_bridge.rs`) is filled by the shell
/// after the child loads, and this runtime has no shell.
#[test]
fn frame_element_interface_and_reflection() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var f = document.createElement('frame'); \
                     f instanceof HTMLFrameElement && f instanceof HTMLElement \
                     && f.constructor.name === 'HTMLFrameElement' \
                     && f.src === '' && f.name === '' && f.noResize === false \
                     && (f.name = 'n1', f.getAttribute('name') === 'n1' && f.name === 'n1') \
                     && (f.noResize = true, f.getAttribute('noresize') === '' && f.noResize === true) \
                     && (f.frameBorder = '0', f.getAttribute('frameborder') === '0') \
                     && (f.marginWidth = '4', f.getAttribute('marginwidth') === '4') \
                     && (f.marginHeight = '5', f.getAttribute('marginheight') === '5') \
                     && (f.scrolling = 'no', f.getAttribute('scrolling') === 'no') \
                     && f.contentDocument === null && f.contentWindow === null",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

/// BUG-415: `document.body` had no setter on the detached document, and
/// `HTMLFrameSetElement` did not exist.
#[test]
fn detached_document_body_setter() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var d = document.implementation.createHTMLDocument('t'); \
                     var f = d.createElement('frameset'); \
                     f instanceof HTMLFrameSetElement \
                     && (d.body = f, d.body.tagName === 'FRAMESET') \
                     && (function() { \
                            var e = document.implementation.createHTMLDocument('t'); \
                            e.removeChild(e.documentElement); \
                            try { e.body = e.createElement('body'); return false; } \
                            catch (err) { return err.name === 'HierarchyRequestError'; } \
                        })() \
                     && (function() { \
                            try { d.body = 'text'; return false; } \
                            catch (err) { return err instanceof TypeError; } \
                        })()",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

/// BUG-415: a port of the `doc`-side subtests of WPT
/// `html/dom/documents/dom-tree-accessors/Document.body.html`, the test
/// whose first `doc.removeChild(...)` used to take 17 of its 24 failures
/// with it. Each `assert_equals` of the original becomes one clause.
///
/// The four subtests that put an element in a foreign namespace *and*
/// expect it to stay distinguishable from the HTML one are omitted: the
/// arena stores `Namespace` as a six-value enum, so
/// `createElementNS('http://example.org/test', 'body')` is indistinguishable
/// from `createElement('body')` — [BUG-830], a separate defect one layer down.
#[test]
fn detached_document_wpt_document_body_subtests() {
    let rt = v8_runtime_with_dom(make_doc());
    let script = "\
                function mk() { \
                    var d = document.implementation.createHTMLDocument(''); \
                    d.removeChild(d.documentElement); \
                    return d; \
                } \
                var r = []; var d, html, b, f, x; \
                r.push(mk().body === null); \
                d = mk(); d.appendChild(d.createElement('html')); r.push(d.body === null); \
                d = mk(); html = d.appendChild(d.createElement('html')); \
                b = html.appendChild(d.createElement('body')); \
                html.appendChild(d.createElement('frameset')); \
                r.push(d.body.isSameNode(b)); \
                d = mk(); html = d.appendChild(d.createElement('html')); \
                f = html.appendChild(d.createElement('frameset')); \
                html.appendChild(d.createElement('body')); \
                r.push(d.body.isSameNode(f)); \
                d = mk(); html = d.appendChild(d.createElement('html')); \
                x = html.appendChild(d.createElement('x')); \
                x.appendChild(d.createElement('body')); \
                b = html.appendChild(d.createElement('body')); \
                r.push(d.body.isSameNode(b)); \
                d = mk(); d.appendChild(d.createElement('body')); r.push(d.body === null); \
                d = mk(); d.appendChild(d.createElement('frameset')); r.push(d.body === null); \
                d = mk(); b = d.appendChild(d.createElement('body')); \
                b.appendChild(d.createElement('frameset')); r.push(d.body === null); \
                d = mk(); f = d.appendChild(d.createElement('frameset')); \
                f.appendChild(d.createElement('body')); r.push(d.body === null); \
                d = document.implementation.createHTMLDocument(); \
                b = d.createElement('body'); d.body = b; r.push(d.body.isSameNode(b)); \
                d = document.implementation.createHTMLDocument(); \
                f = d.createElement('frameset'); d.body = f; r.push(d.body.isSameNode(f)); \
                d = mk(); html = d.appendChild(d.createElement('html')); \
                f = html.appendChild(d.createElement('frameset')); \
                b = d.createElement('body'); d.body = b; \
                r.push(f.parentNode === null && d.body.isSameNode(b)); \
                d = mk(); html = d.appendChild(d.createElement('html')); \
                b = html.appendChild(d.createElement('body')); \
                var f1 = html.appendChild(d.createElement('frameset')); \
                var f2 = d.createElement('frameset'); d.body = f2; \
                r.push(b.parentNode === null && f1.parentNode.isSameNode(html) \
                       && d.body.isSameNode(f2) && f2.nextSibling.isSameNode(f1)); \
                d = mk(); d.appendChild(d.createElement('test')); \
                b = d.createElement('body'); d.body = b; \
                r.push(d.documentElement.firstChild.isSameNode(b) && d.body === null); \
                r.indexOf(false)";
    let first_failure = rt.eval(script).unwrap();
    assert_eq!(first_failure, lumen_core::JsValue::Number(-1.0));
}

#[test]
fn create_element_and_append() {
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var p = document.createElement('p'); \
                 p.textContent = 'new paragraph'; \
                 document.body.appendChild(p);",
    )
    .unwrap();
    drop(rt);
    let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
    let body_id = find_element_by_tag(&doc, "body").unwrap();
    let body = doc.get(body_id);
    // body should now have 2 children: the original div + the new <p>
    assert_eq!(body.children.len(), 2);
    let p_id = body.children[1];
    assert_eq!(
        doc.get(p_id)
            .element_name()
            .map(|n| n.local.as_str()),
        Some("p")
    );
    assert_eq!(collect_text_content(&doc, p_id), "new paragraph");
}

#[test]
fn query_selector_all_returns_array() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelectorAll('span').length")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn get_elements_by_class_name_document() {
    // BUG-302: getElementsByClassName was missing from WEB_API_SHIM.
    let rt = v8_runtime_with_dom(make_doc());
    let hit = rt
        .eval("document.getElementsByClassName('highlight').length")
        .unwrap();
    assert_eq!(hit, lumen_core::JsValue::Number(1.0));
    let miss = rt
        .eval("document.getElementsByClassName('nope').length")
        .unwrap();
    assert_eq!(miss, lumen_core::JsValue::Number(0.0));
    // Empty / whitespace-only token list yields an empty collection.
    let empty = rt
        .eval("document.getElementsByClassName('   ').length")
        .unwrap();
    assert_eq!(empty, lumen_core::JsValue::Number(0.0));
}

#[test]
fn get_elements_by_class_name_scoped_element() {
    // BUG-302: the scoped variant lives on Element too, restricted to the
    // element's own descendants.
    let rt = v8_runtime_with_dom(make_doc());
    let inside = rt
        .eval("document.body.getElementsByClassName('highlight').length")
        .unwrap();
    assert_eq!(inside, lumen_core::JsValue::Number(1.0));
    // The <span.highlight> has no descendants, so scoping to it finds none.
    let none = rt
        .eval(
            "document.getElementsByClassName('highlight')[0]\
                     .getElementsByClassName('highlight').length",
        )
        .unwrap();
    assert_eq!(none, lumen_core::JsValue::Number(0.0));
}

#[test]
fn get_elements_by_tag_name_scoped_element() {
    // BUG-416: `Element.prototype.getElementsByTagName` was missing
    // entirely (`el.getElementsByTagName is not a function`), even
    // though the element already carried `getElementsByClassName` and
    // `querySelectorAll`. DOM LS §4.5: scoped to the element's
    // descendants, the element itself excluded.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var wrap = document.getElementById('main'); \
                     typeof wrap.getElementsByTagName === 'function' && \
                     wrap.getElementsByTagName('span').length === 1 && \
                     wrap.getElementsByTagName('span')[0].className === 'highlight' && \
                     wrap.getElementsByTagName('div').length === 0 && \
                     document.body.getElementsByTagName('div').length === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_elements_by_tag_name_is_case_insensitive_for_html() {
    // BUG-416: the old document-side body handed the name to the
    // selector engine, which compares a type selector to the local name
    // by exact string equality — so an upper-cased ask found nothing at
    // all. DOM LS §4.5 folds the ask for HTML-namespace elements.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "document.getElementsByTagName('div').length === 1 && \
                     document.getElementsByTagName('DIV').length === 1 && \
                     document.getElementsByTagName('DiV').length === 1 && \
                     document.getElementById('main').parentNode\
                        .getElementsByTagName('SPAN').length === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_elements_by_tag_name_is_case_sensitive_for_foreign_content() {
    // BUG-416 / WPT `html/syntax/parsing/Element.getElementsByTagName-foreign-0*`:
    // an element outside the HTML namespace matches its qualified name
    // exactly — an SVG <linearGradient> answers only to the exact
    // spelling, never to the folded one.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var SVG = 'http://www.w3.org/2000/svg'; \
                     var svg = document.createElementNS(SVG, 'svg'); \
                     var grad = document.createElementNS(SVG, 'linearGradient'); \
                     svg.appendChild(grad); document.body.appendChild(svg); \
                     document.getElementsByTagName('linearGradient').length === 1 && \
                     document.getElementsByTagName('lineargradient').length === 0 && \
                     document.getElementsByTagName('LINEARGRADIENT').length === 0 && \
                     svg.getElementsByTagName('linearGradient').length === 1 && \
                     svg.getElementsByTagName('lineargradient').length === 0",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_elements_by_tag_name_star_matches_elements_only() {
    // BUG-416: '*' is «every descendant ELEMENT», in tree order — the
    // two text nodes of the fixture must not show up.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var all = document.getElementsByTagName('*'); \
                     all.length === 6 && \
                     all.map(function(e) { return e.tagName; }).join(',') === \
                        'HTML,HEAD,TITLE,BODY,DIV,SPAN' && \
                     document.getElementById('main').getElementsByTagName('*').length === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_elements_by_tag_name_ns_on_document_and_element() {
    // BUG-416: `getElementsByTagNameNS` was missing from BOTH the
    // document and the element. DOM LS §4.5: '*' is a wildcard in
    // either position, null and '' both mean «no namespace», and the
    // local name is compared case-sensitively whatever the namespace.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var SVG = 'http://www.w3.org/2000/svg'; \
                     var HTML = 'http://www.w3.org/1999/xhtml'; \
                     var svg = document.createElementNS(SVG, 'svg'); \
                     var grad = document.createElementNS(SVG, 'linearGradient'); \
                     svg.appendChild(grad); document.body.appendChild(svg); \
                     var bare = document.createElementNS(null, 'bare'); \
                     document.body.appendChild(bare); \
                     typeof document.getElementsByTagNameNS === 'function' && \
                     typeof document.body.getElementsByTagNameNS === 'function' && \
                     document.getElementsByTagNameNS(SVG, '*').length === 2 && \
                     document.getElementsByTagNameNS(SVG, 'linearGradient').length === 1 && \
                     document.getElementsByTagNameNS(SVG, 'lineargradient').length === 0 && \
                     document.getElementsByTagNameNS(SVG, 'div').length === 0 && \
                     document.getElementsByTagNameNS(HTML, 'div').length === 1 && \
                     document.getElementsByTagNameNS(HTML, 'DIV').length === 0 && \
                     document.getElementsByTagNameNS('*', 'linearGradient').length === 1 && \
                     document.getElementsByTagNameNS(null, 'bare').length === 1 && \
                     document.getElementsByTagNameNS('', 'bare').length === 1 && \
                     document.getElementsByTagNameNS(HTML, 'bare').length === 0 && \
                     svg.getElementsByTagNameNS(SVG, '*').length === 1 && \
                     svg.getElementsByTagNameNS(SVG, '*')[0] === grad",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_elements_by_name_document() {
    // BUG-412: getElementsByName was missing from WEB_API_SHIM entirely
    // (`'getElementsByName' in document` === false). HTML LS §3.1.5:
    // matches the `name` content attribute case-sensitively, ignores
    // `id`, ignores elements outside the HTML namespace, and runs the
    // argument through the DOMString conversion first.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var d = document.createElement('div'); \
                     d.setAttribute('name', 'abcd'); document.body.appendChild(d); \
                     var n = document.createElement('div'); \
                     n.setAttribute('name', 'null'); document.body.appendChild(n); \
                     var u = document.createElement('div'); \
                     u.setAttribute('name', 'undefined'); document.body.appendChild(u); \
                     var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); \
                     svg.setAttribute('name', 'abcd'); document.body.appendChild(svg); \
                     document.getElementsByName('abcd').length === 1 && \
                     document.getElementsByName('abcd')[0] === d && \
                     document.getElementsByName('ABCD').length === 0 && \
                     document.getElementsByName('main').length === 0 && \
                     document.getElementsByName('nope').length === 0 && \
                     document.getElementsByName(null)[0] === n && \
                     document.getElementsByName(undefined)[0] === u",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_elements_by_name_is_live_node_list() {
    // BUG-412: the result is a live NodeList (DOM §4.2.10.1), not the
    // static array `getElementsByTagName`/`getElementsByClassName`
    // settle for and not an HTMLCollection — so it tracks later
    // insertions/removals and carries no `namedItem`.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var input = document.createElement('input'); \
                     input.setAttribute('name', 'test'); document.body.appendChild(input); \
                     var list = document.getElementsByName('test'); \
                     var l1 = list.length; \
                     var embed = document.createElement('embed'); \
                     embed.setAttribute('name', 'test'); document.body.appendChild(embed); \
                     var l2 = list.length; \
                     document.body.removeChild(embed); \
                     var l3 = list.length; \
                     l1 === 1 && l2 === 2 && l3 === 1 && \
                     list instanceof NodeList && !(list instanceof HTMLCollection) && \
                     Object.prototype.toString.call(list) === '[object NodeList]' && \
                     !('namedItem' in list) && list.item(0) === input && \
                     list.item(9) === null && \
                     (function() { var seen = 0; \
                        list.forEach(function(el) { if (el === input) seen++; }); \
                        return seen === 1; })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn inner_text_setter_turns_line_breaks_into_br() {
    // BUG-413: HTML LS §3.2.7 — the `innerText` setter replaces all
    // children with the «rendered text fragment»: runs of text become
    // Text nodes, every line break becomes a `<br>`, and a CRLF pair
    // counts as one break while `\n\n` counts as two. `null` maps to the
    // empty string ([LegacyNullToEmptyString]), `undefined` does not.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var c = document.getElementById('main'); \
                     function set(v) { \
                       var d = document.createElement('div'); c.appendChild(d); \
                       d.innerText = v; return d.innerHTML; } \
                     [set('abc'), set('abc\\ndef'), set('abc\\rdef'), \
                      set('abc\\r\\ndef'), set('abc\\n\\ndef'), set('abc\\r\\rdef'), \
                      set('\\r\\nabc'), set('abc\\r\\n'), set(''), set(null), \
                      set(undefined), set('abc  def')].join('|')",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "abc|abc<br>def|abc<br>def|abc<br>def|abc<br><br>def|abc<br><br>def|\
                     <br>abc|abc<br>|||undefined|abc  def"
                .into()
        )
    );
}

#[test]
fn inner_text_setter_replaces_existing_children_with_one_text_node() {
    // BUG-413: WPT `innertext-setter.html` asserts the assignment leaves
    // exactly one — and a *new* — Text node behind, with no empty text
    // siblings, both for a rendered element and for a detached one.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var live = document.getElementById('main'); \
                     var oldChild = live.firstChild; \
                     live.innerText = 'abc'; \
                     var det = document.createElement('div'); \
                     det.innerHTML = '<b>x</b>y'; det.innerText = 'zzz'; \
                     live.firstChild.nodeType === 3 && live.firstChild.data === 'abc' && \
                     live.firstChild.nextSibling === null && live.firstChild !== oldChild && \
                     det.childNodes.length === 1 && det.firstChild.data === 'zzz'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn inner_text_and_outer_text_absent_outside_html_namespace() {
    // BUG-413: both are `HTMLElement` members, so on an SVG element the
    // assignment must behave like a write to a plain object — the
    // element's children stay untouched and the value reads back.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); \
                     document.getElementById('main').appendChild(svg); \
                     svg.innerText = 'abc'; svg.outerText = 'def'; \
                     svg.innerHTML === '' && svg.innerText === 'abc' && \
                     svg.outerText === 'def' && svg.parentNode !== null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn outer_text_setter_merges_only_the_touching_text_nodes() {
    // BUG-413: HTML LS §3.2.7 — the `outerText` setter replaces the
    // element itself, then folds the Text nodes that used to sit either
    // side of it into the inserted text. It is NOT a `normalize()`: the
    // Text nodes further out stay separate.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var c = document.getElementById('main'); c.innerHTML = ''; \
                     c.append('A', 'B', document.createElement('span'), 'D', 'E'); \
                     c.childNodes[2].outerText = 'Replaced'; \
                     c.innerHTML === 'ABReplacedDE' && c.childNodes.length === 3 && \
                     c.childNodes[0].data === 'A' && \
                     c.childNodes[1].data === 'BReplacedD' && \
                     c.childNodes[2].data === 'E'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn outer_text_setter_empty_string_removes_the_element() {
    // BUG-413: an empty assignment still inserts one empty Text node, so
    // the two neighbours end up merged into a single node and the element
    // itself is gone.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var c = document.getElementById('main'); c.innerHTML = ''; \
                     c.append('1', '2', document.createElement('span'), '3', '4'); \
                     c.childNodes[2].outerText = ''; \
                     var lonely = document.createElement('div'); \
                     c.appendChild(lonely); \
                     var only = document.createElement('p'); \
                     lonely.appendChild(only); only.outerText = ''; \
                     c.childNodes.length === 4 && c.childNodes[1].data === '23' && \
                     lonely.childNodes.length === 1 && lonely.childNodes[0].data === ''",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn outer_text_setter_line_breaks_and_detached_element() {
    // BUG-413: an all-newline assignment yields only `<br>`s with no Text
    // nodes between them, and an element with no parent must throw
    // NoModificationAllowedError instead of silently doing nothing.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var c = document.getElementById('main'); c.innerHTML = '<span>x</span>'; \
                     c.firstElementChild.outerText = '\\n\\r\\n\\r'; \
                     var threw = ''; \
                     try { document.createElement('span').outerText = ''; } \
                     catch (e) { threw = e.name; } \
                     c.innerHTML + '/' + c.childNodes.length + '/' + threw",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("<br><br><br>/3/NoModificationAllowedError".into())
    );
}

/// UA `display` for the handful of tags the `innerText` getter tests use.
/// Everything unlisted is `inline`, which is what an unstyled `<span>` /
/// `<b>` / `<br>` computes to.
fn ua_display(tag: &str) -> &'static str {
    match tag {
        "html" | "body" | "head" | "div" | "p" | "section" | "pre" | "h1" | "h2"
        | "ul" | "ol" => "block",
        "li" => "list-item",
        "table" => "table",
        "tbody" | "thead" | "tfoot" => "table-row-group",
        "tr" => "table-row",
        "td" | "th" => "table-cell",
        "caption" => "table-caption",
        _ => "inline",
    }
}

/// Publishes the computed-style snapshot the `innerText` getter reads
/// (BUG-413, slice 2), mimicking what `collect_computed_styles` produces.
///
/// Three properties of the real snapshot are what the getter is built on,
/// so the stand-in has to reproduce all three or it would test something
/// else: an element that owns a box gets an entry; an **inline** element
/// does not, because its content is flattened into the enclosing block's
/// inline run; and the style governing that content is published on the
/// **text node** instead (`lumen_layout::INLINE_SEGMENT_PROPERTIES`),
/// with the inherited values resolved.
///
/// `overrides` re-styles elements by `id`. A `display: none` override
/// drops the element *and its whole subtree*, text nodes included, rather
/// than recording `"none"` — that is what a real snapshot looks like: no
/// box, no segment, no entry.
fn publish_render_snapshot(
    rt: &V8JsRuntime,
    doc: &Arc<Mutex<Document>>,
    overrides: &[(&str, &[(&str, &str)])],
) {
    /// The three inherited values in flight down the walk, in the order
    /// of `INLINE_SEGMENT_PROPERTIES`.
    type Inherited = [String; 3];

    fn walk(
        doc: &Document,
        id: lumen_dom::NodeId,
        inherited: &Inherited,
        overrides: &[(&str, &[(&str, &str)])],
        rects: &mut std::collections::HashMap<u32, [f32; 4]>,
        styles: &mut std::collections::HashMap<
            u32,
            std::collections::HashMap<String, String>,
        >,
    ) {
        let node = doc.get(id);
        let mut inherited = inherited.clone();
        if let Some(name) = node.element_name() {
            let mut display = ua_display(&name.local).to_string();
            if let Some(elem_id) = node.get_attr("id") {
                for (want, props) in overrides {
                    if *want != elem_id {
                        continue;
                    }
                    for (k, v) in *props {
                        match *k {
                            "display" => display = (*v).to_string(),
                            "visibility" => inherited[0] = (*v).to_string(),
                            "white-space" => inherited[1] = (*v).to_string(),
                            "text-transform" => inherited[2] = (*v).to_string(),
                            other => panic!("unhandled override {other}"),
                        }
                    }
                }
            }
            if display == "none" {
                return;
            }
            if display != "inline" {
                let mut m = std::collections::HashMap::new();
                m.insert("display".to_string(), display);
                for (prop, value) in
                    lumen_layout::INLINE_SEGMENT_PROPERTIES.iter().zip(&inherited)
                {
                    m.insert((*prop).to_string(), value.clone());
                }
                let idx = id.index() as u32;
                rects.insert(idx, [0.0, 0.0, 10.0, 10.0]);
                styles.insert(idx, m);
            }
        } else if matches!(node.data, NodeData::Text(_)) {
            // Only a text node becomes an inline segment, and only a
            // segment carries the inherited three.
            let m = lumen_layout::INLINE_SEGMENT_PROPERTIES
                .iter()
                .zip(&inherited)
                .map(|(p, v)| ((*p).to_string(), v.clone()))
                .collect();
            styles.insert(id.index() as u32, m);
        }
        for &child in &node.children {
            walk(doc, child, &inherited, overrides, rects, styles);
        }
    }

    let mut rects = std::collections::HashMap::new();
    let mut styles = std::collections::HashMap::new();
    let initial: Inherited = [
        "visible".to_string(),
        "normal".to_string(),
        "none".to_string(),
    ];
    {
        let guard = doc.lock().unwrap();
        walk(&guard, guard.root(), &initial, overrides, &mut rects, &mut styles);
    }
    rt.update_layout_rects(rects);
    rt.update_computed_styles(styles);
}

#[test]
fn inner_text_getter_reports_rendered_text_with_block_breaks() {
    // BUG-413 slice 2: HTML LS §3.2.7 — the getter answers *rendered*
    // text, so a `<p>` contributes a required line break count of 2 at
    // both ends (steps 7 and 5-6 of the getter), the counts at the very
    // start and end are stripped, and the run between the two paragraphs
    // becomes exactly two line feeds. `textContent` would run it all
    // together instead.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var c = document.getElementById('main'); \
                 c.innerHTML = '<p>hello <b>world</b></p><p>second</p>';",
    )
    .unwrap();
    publish_render_snapshot(&rt, &doc, &[]);
    let r = rt.eval("document.getElementById('main').innerText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello world\n\nsecond".into()));
    // The spec gives `outerText` no getter of its own — same steps.
    let o = rt.eval("document.getElementById('main').outerText").unwrap();
    assert_eq!(o, lumen_core::JsValue::String("hello world\n\nsecond".into()));
}

#[test]
fn inner_text_getter_skips_display_none_and_hidden_subtrees() {
    // BUG-413 slice 2: step 2 drops a `visibility: hidden` box and step 3
    // drops a box-less (`display: none`) one. This is the whole reason
    // the property cannot be served from the DOM alone — `textContent`
    // here would be "AXYB".
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var c = document.getElementById('main'); \
                 c.innerHTML = 'A<span id=\"gone\">X</span><span id=\"ghost\">Y</span>B';",
    )
    .unwrap();
    publish_render_snapshot(
        &rt,
        &doc,
        &[
            ("gone", &[("display", "none")]),
            ("ghost", &[("visibility", "hidden")]),
        ],
    );
    let r = rt.eval("document.getElementById('main').innerText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("AB".into()));
    let tc = rt.eval("document.getElementById('main').textContent").unwrap();
    assert_eq!(tc, lumen_core::JsValue::String("AXYB".into()));
}

#[test]
fn inner_text_getter_collapses_whitespace_across_inline_boundaries() {
    // BUG-413 slice 2: collapsing is not per-text-node — two inline
    // siblings that both touch a space contribute one, and a space at
    // either end of the result or next to a line break renders as
    // nothing at all.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var c = document.getElementById('main'); \
                 c.innerHTML = '  a  <span>  b  </span>\\n  c  ';",
    )
    .unwrap();
    publish_render_snapshot(&rt, &doc, &[]);
    let r = rt.eval("document.getElementById('main').innerText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a b c".into()));
}

#[test]
fn inner_text_getter_line_feeds_from_br_and_block_children() {
    // BUG-413 slice 2: step 5 appends the `<br>` line feed as a plain
    // string, so it survives the stripping the break counts undergo,
    // while the `<div>` around "c" contributes a count of 1.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var c = document.getElementById('main'); \
                 c.innerHTML = 'a<br>b <div>c</div>';",
    )
    .unwrap();
    publish_render_snapshot(&rt, &doc, &[]);
    let r = rt.eval("document.getElementById('main').innerText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a\nb\nc".into()));
}

#[test]
fn inner_text_getter_honours_white_space_and_text_transform() {
    // BUG-413 slice 2: step 4 is «the text of the boxes», i.e. after the
    // `white-space` and `text-transform` the element was laid out with —
    // `pre` keeps every space, and `uppercase` reaches text the element
    // only inherits down to.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var c = document.getElementById('main'); \
                 c.innerHTML = '<pre id=\"raw\">  x  y  </pre><div id=\"shout\">go</div>';",
    )
    .unwrap();
    publish_render_snapshot(
        &rt,
        &doc,
        &[
            ("raw", &[("white-space", "pre")]),
            ("shout", &[("text-transform", "uppercase")]),
        ],
    );
    let r = rt.eval("document.getElementById('main').innerText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("  x  y  \nGO".into()));
}

#[test]
fn inner_text_getter_tabs_between_table_cells() {
    // BUG-413 slice 2: step 6 puts a tab after every table cell but the
    // last of its row, and the rows themselves break the line.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var c = document.getElementById('main'); \
                 c.innerHTML = '<table><tbody><tr><td>a</td><td>b</td></tr>' + \
                               '<tr><td>c</td></tr></tbody></table>';",
    )
    .unwrap();
    publish_render_snapshot(&rt, &doc, &[]);
    let r = rt.eval("document.getElementById('main').innerText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a\tb\nc".into()));
}

#[test]
fn inner_text_getter_falls_back_to_text_content_when_not_rendered() {
    // BUG-413 slice 2: step 1 of the getter — a node with no box answers
    // `textContent`, whitespace and hidden subtrees included. That covers
    // a detached element and, deliberately, a document the engine has not
    // laid out yet.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var d = document.createElement('div'); \
                 d.innerHTML = '  a  <span>b</span>';",
    )
    .unwrap();
    publish_render_snapshot(&rt, &doc, &[]);
    // Asserted against `textContent` rather than a literal: what the
    // fragment parser keeps of the leading whitespace is beside the
    // point, and the claim is «these two are the same string», which the
    // rendered path would break by collapsing the double space.
    let r = rt
        .eval("d.innerText === d.textContent && d.innerText.indexOf('a  b') >= 0")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn inner_text_and_outer_text_getters_absent_outside_html_namespace() {
    // BUG-413 slice 2: both are `HTMLElement` members, so an SVG element
    // — which shares this wrapper factory — must read as `undefined`
    // rather than as its rendered text.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval(
        "var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); \
                 svg.textContent = 'abc'; \
                 document.getElementById('main').appendChild(svg);",
    )
    .unwrap();
    publish_render_snapshot(&rt, &doc, &[]);
    let r = rt
        .eval("svg.innerText === undefined && svg.outerText === undefined")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn image_constructor_creates_img_element() {
    // BUG-305: `new Image()` must produce a native <img> wrapper.
    let rt = v8_runtime_with_dom(make_doc());
    let tag = rt.eval("new Image().tagName").unwrap();
    assert_eq!(tag, lumen_core::JsValue::String("IMG".into()));
}

#[test]
fn image_constructor_applies_width_height_args() {
    // BUG-305: Image(width, height) sets the width/height content attributes.
    let rt = v8_runtime_with_dom(make_doc());
    let dims = rt
        .eval(
            "var i = new Image(4, 6);\
                     i.getAttribute('width') + 'x' + i.getAttribute('height')",
        )
        .unwrap();
    assert_eq!(dims, lumen_core::JsValue::String("4x6".into()));
}
