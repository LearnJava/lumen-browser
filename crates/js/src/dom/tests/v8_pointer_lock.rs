//! V8 port of the final flat-tail slice of the `dom.rs` test-monolith migration
//! (S12b-24-pointer-lock, `docs/tasks/ph3-v8-migration.md`) — the last contiguous
//! region before the nested `mod v8_core` boundary. Only the first 6 tests are
//! actually Pointer Lock API (W3C Pointer Lock L2 §2-4 + Phase 1); the remaining
//! 23 are an un-headered grab-bag left over from earlier slices: `Comment`/`Text`
//! constructors + CharacterData prototype chain/methods (BUG-313/314/322/325),
//! `DocumentType`/`DOMImplementation` (BUG-321/324), and native element/text
//! wrapper `instanceof` resolution — the section comment above named only the
//! first cluster, same "don't trust the header" gotcha as S12b-24-css-storage-nav-misc.
//! All 29 bodies are synchronous `rt.eval(...)`, ported verbatim.

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

#[test]
fn pointer_lock_request_sets_lock_state() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 el.requestPointerLock(); \
                 document.pointerLockElement === el"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pointer_lock_request_dispatches_pointerlockchange() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var fired = false; \
                 document.addEventListener('pointerlockchange', function() { fired = true; }); \
                 el.requestPointerLock(); \
                 fired"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pointer_lock_exit_clears_lock_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 el.requestPointerLock(); \
                 document.exitPointerLock(); \
                 document.pointerLockElement === null"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_on_pointerlockchange_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "typeof document.onpointerlockchange === 'object' && \
                 typeof document.exitPointerLock === 'function'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn locked_mousemove_delivers_movement_deltas() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var mx = 0, my = 0; \
                 el.addEventListener('mousemove', function(e) { mx = e.movementX; my = e.movementY; }); \
                 _lumen_dispatch_locked_mousemove(el.__nid__, 100, 200, 15, -8, 0); \
                 mx === 15 && my === -8"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn locked_mousemove_delivers_pointermove() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var fired = false; \
                 el.addEventListener('pointermove', function(e) { fired = e.movementX === 7; }); \
                 _lumen_dispatch_locked_mousemove(el.__nid__, 0, 0, 7, 3, 0); \
                 fired"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn locked_mousemove_client_coords_preserved() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var cx = -1, cy = -1; \
                 el.addEventListener('mousemove', function(e) { cx = e.clientX; cy = e.clientY; }); \
                 _lumen_dispatch_locked_mousemove(el.__nid__, 42, 99, 5, 5, 0); \
                 cx === 42 && cy === 99"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-313: document.createProcessingInstruction returns a PI node with the
// given target/data, nodeType 7, and this document as ownerDocument.
#[test]
fn create_processing_instruction_returns_node() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var pi = document.createProcessingInstruction('xml-stylesheet', 'href=\"a.css\"'); \
                 pi.target === 'xml-stylesheet' && pi.data === 'href=\"a.css\"' && \
                 pi.nodeType === 7 && pi.nodeName === 'xml-stylesheet' && \
                 pi.ownerDocument === document"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-313: valid XML Name targets from the WPT corpus are accepted, including
// a colon (`xml:fail`) and the middle-dot NameChar (`A·A`).
#[test]
fn create_processing_instruction_accepts_valid_names() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "['xml:fail', 'A\\u00B7A', 'a0'].every(function(t) { \
                    try { return document.createProcessingInstruction(t, 'x').target === t; } \
                    catch (e) { return false; } \
                 })"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-313: invalid targets and `?>` in data throw InvalidCharacterError
// (DOMException with legacy code 5).
#[test]
fn create_processing_instruction_rejects_invalid() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var bad = [['A', '?>'], ['\\u00B7A', 'x'], ['\\u00D7A', 'x'], \
                            ['A\\u00D7', 'x'], ['\\\\A', 'x'], ['\\f', 'x'], ['0', 'x']]; \
                 bad.every(function(pair) { \
                    try { document.createProcessingInstruction(pair[0], pair[1]); return false; } \
                    catch (e) { return e.name === 'InvalidCharacterError' && e.code === 5; } \
                 })"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-314: `new Comment(data)` / `new Text(data)` build detached CharacterData
// nodes with the correct nodeType/nodeName, stringified data, and the current
// document as ownerDocument.
#[test]
fn comment_text_constructors_build_nodes() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var c = new Comment('hi'), t = new Text('yo'); \
                 c.nodeType === 8 && c.nodeName === '#comment' && c.data === 'hi' && \
                 c.nodeValue === 'hi' && c.ownerDocument === document && \
                 t.nodeType === 3 && t.nodeName === '#text' && t.data === 'yo' && \
                 new Comment().data === '' && new Comment(null).data === 'null' && \
                 new Text(42).data === '42'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-314: the CharacterData prototype chain and `instanceof` resolve for
// `new Comment()`/`new Text()` and the detached ProcessingInstruction node.
#[test]
fn character_data_prototype_chain() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var c = new Comment(); \
                 Object.getPrototypeOf(c) === Comment.prototype && \
                 Object.getPrototypeOf(Comment.prototype) === CharacterData.prototype && \
                 Object.getPrototypeOf(CharacterData.prototype) === Node.prototype && \
                 c instanceof Comment && c instanceof CharacterData && c instanceof Node && \
                 (new Text()) instanceof Text && \
                 document.createProcessingInstruction('a', 'b') instanceof ProcessingInstruction"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-322: native-backed element/text wrappers get a real [[Prototype]]
// chain too — tag-appropriate HTML*Element for elements (falling back to
// plain HTMLElement for tags without a dedicated interface, e.g. a custom
// element name), Text for text nodes — so `instanceof` resolves the same
// way it does for the detached constructor forms covered by
// `character_data_prototype_chain` above.
#[test]
fn element_prototype_chain_instanceof() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var div = document.createElement('div'); \
                 var span = document.getElementsByClassName('highlight')[0]; \
                 var textNode = document.createTextNode('x'); \
                 var unknown = document.createElement('foo-bar'); \
                 div instanceof HTMLDivElement && div instanceof HTMLElement && \
                 div instanceof Element && div instanceof Node && \
                 Object.getPrototypeOf(HTMLDivElement.prototype) === HTMLElement.prototype && \
                 Object.getPrototypeOf(HTMLElement.prototype) === Element.prototype && \
                 Object.getPrototypeOf(Element.prototype) === Node.prototype && \
                 span instanceof HTMLSpanElement && \
                 document.body instanceof HTMLBodyElement && document.body instanceof HTMLElement && \
                 unknown instanceof HTMLElement && !(unknown instanceof HTMLDivElement) && \
                 textNode instanceof Text && textNode instanceof CharacterData && textNode instanceof Node && \
                 !(div instanceof Text)"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-367 (1): DOM LS §4.9 `localName`/`prefix` exist on every element
// wrapper — `localName` is the lower-case tag name for HTML elements and
// `prefix` is present-and-null (Lumen parses no prefixes), rather than
// both being absent as they were before.
#[test]
fn element_local_name_and_prefix() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var div = document.createElement('DIV'); \
                     var main = document.getElementById('main'); \
                     [div, main, document.body, document.documentElement].every(function(el) { \
                         return typeof el.localName === 'string' \
                             && el.localName === el.tagName.toLowerCase() \
                             && 'prefix' in el && el.prefix === null; \
                     }) && main.localName === 'div' && document.body.localName === 'body'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-367 (2): DOM LS §4.9 upper-cases `tagName` only for elements in the
// HTML namespace. Foreign content keeps the author's case verbatim, which
// matters for the case-sensitive SVG tag names (`linearGradient`), while
// HTML elements are unaffected.
#[test]
fn tag_name_upper_cased_only_in_html_namespace() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var SVG = 'http://www.w3.org/2000/svg'; \
                     var rect = document.createElementNS(SVG, 'rect'); \
                     var grad = document.createElementNS(SVG, 'linearGradient'); \
                     var div  = document.createElement('div'); \
                     rect.tagName === 'rect' && rect.nodeName === 'rect' && \
                     rect.localName === 'rect' && rect.namespaceURI === SVG && \
                     grad.tagName === 'linearGradient' && grad.localName === 'linearGradient' && \
                     div.tagName === 'DIV' && div.nodeName === 'DIV' && div.localName === 'div' && \
                     document.createTextNode('x').nodeName === '#text'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-367 (3): `__nid__` is the wrapper's internal arena handle. It must
// not show up in `Object.keys`/`for…in`/spread (a Lumen fingerprint on
// every node), and it must not be writable — the shim resolves tree
// mutations through it, so a page-script assignment used to re-point
// `appendChild(a)` at whatever node `a.__nid__` was overwritten with.
#[test]
fn nid_handle_is_hidden_and_immutable() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var host = document.createElement('div'); \
                     document.body.appendChild(host); \
                     host.innerHTML = '<span id=\"a\">A</span><span id=\"b\">B</span>'; \
                     var a = document.getElementById('a'), b = document.getElementById('b'); \
                     var dest = document.createElement('div'); \
                     document.body.appendChild(dest); \
                     var d = Object.getOwnPropertyDescriptor(a, '__nid__'); \
                     var keysClean = Object.keys(a).indexOf('__nid__') < 0 \
                         && Object.keys(document.body).indexOf('__nid__') < 0; \
                     var forInClean = true; \
                     for (var k in a) { if (k === '__nid__') forInClean = false; } \
                     try { a.__nid__ = b.__nid__; } catch (e) {} \
                     dest.appendChild(a); \
                     d.enumerable === false && d.writable === false && \
                     keysClean && forInClean && a.__nid__ !== b.__nid__ && \
                     dest.children.length === 1 && dest.children[0].id === 'a' && \
                     host.children.length === 1 && host.children[0].id === 'b'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-367 (5): HTML LS §3.1.3 — a tag name the specification does not
// define gets `HTMLUnknownElement`; defined ones (including those without
// a dedicated interface) and valid custom element names get `HTMLElement`.
#[test]
fn unrecognized_tag_gets_html_unknown_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "function ctorOf(t) { return document.createElement(t); } \
                     var unknown = ['fencedframe', 'foo', 'abcd'].every(function(t) { \
                         var el = ctorOf(t); \
                         return el instanceof HTMLUnknownElement && el instanceof HTMLElement \
                             && el instanceof Element; \
                     }); \
                     var known = ['section', 'article', 'figure', 'center'].every(function(t) { \
                         var el = ctorOf(t); \
                         return el instanceof HTMLElement && !(el instanceof HTMLUnknownElement); \
                     }); \
                     var custom = ctorOf('my-widget'); \
                     unknown && known && \
                     custom instanceof HTMLElement && !(custom instanceof HTMLUnknownElement) && \
                     !(ctorOf('div') instanceof HTMLUnknownElement)",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-325: `Node.appendChild()` on any CharacterData receiver (Text/
// Comment/ProcessingInstruction) throws HierarchyRequestError — DOM
// §4.2.3 pre-insert validity forbids CharacterData from having children.
// Mirrors WPT `dom/nodes/CharacterData-appendChild.html`.
#[test]
fn character_data_append_child_throws_hierarchy_request_error() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "function create(type) { \
                     if (type === 'Text') return document.createTextNode('test'); \
                     if (type === 'Comment') return document.createComment('test'); \
                     return document.createProcessingInstruction('target', 'test'); \
                 } \
                 var types = ['Text', 'Comment', 'ProcessingInstruction']; \
                 types.every(function(t1) { \
                     return types.every(function(t2) { \
                         var n1 = create(t1), n2 = create(t2); \
                         try { n1.appendChild(n2); return false; } \
                         catch (e) { return e.name === 'HierarchyRequestError' && e.code === 3; } \
                     }); \
                 })"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// `document.createComment(data)` previously ignored `data` and built an
// empty *Text* node (not Comment) — nodeType 3, nodeName '#text', wrong
// [[Prototype]]. A live/arena-backed Comment must now report its real
// identity, matching WPT `dom/nodes/CharacterData-data.html` /
// `-surrogates.html`, which both operate on `document.createComment(...)`.
#[test]
fn create_comment_is_a_real_comment_node() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var c = document.createComment('hello'); \
                 c.nodeType === 8 && c.nodeName === '#comment' && c.data === 'hello' && \
                 c instanceof Comment && c instanceof CharacterData && c instanceof Node && \
                 !(c instanceof Text)"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// Regression test: `set_text_content` used to apply Element/Document
// "replace all children" semantics even to a leaf Text/Comment receiver —
// detaching its (empty) children and appending a brand-new CHILD text
// node under it, never touching the node's own string. A second write
// then read back a stale/concatenated value instead of the last-written
// one. `CharacterData.prototype.appendData`/`insertData`/`deleteData`/
// `replaceData` all route through the same `.data` setter, so this also
// covers WPT `dom/nodes/CharacterData-{data,appendData,insertData,
// deleteData,replaceData,substringData}.html`.
#[test]
fn live_text_and_comment_data_mutates_in_place() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var t = document.createTextNode('abc'); \
                 t.data = 'x'; t.data = 'y'; \
                 var c = document.createComment('abc'); \
                 c.data = 'x'; c.data = 'y'; \
                 t.data === 'y' && t.length === 1 && c.data === 'y' && c.length === 1"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// DOM §4.10 CharacterData interface methods, exercised on a live text
// node (arena-backed, not the detached `new Text()` form) — mirrors the
// worked examples in WPT `CharacterData-substringData.html` /
// `-insertData.html` / `-deleteData.html` / `-replaceData.html` /
// `-appendData.html`.
#[test]
fn character_data_methods_spec_examples() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var t = document.createTextNode('abcdef'); \
                 var results = []; \
                 results.push(t.substringData(2, 3) === 'cde'); \
                 results.push(t.substringData(2, 100) === 'cdef'); \
                 t.appendData('gh'); results.push(t.data === 'abcdefgh'); \
                 t.data = 'abcdef'; \
                 t.insertData(3, 'XYZ'); results.push(t.data === 'abcXYZdef'); \
                 t.data = 'abcdef'; \
                 t.deleteData(1, 2); results.push(t.data === 'adef'); \
                 t.data = 'abcdef'; \
                 t.replaceData(1, 2, 'XYZ'); results.push(t.data === 'aXYZdef'); \
                 try { t.substringData(1000, 1); results.push(false); } \
                 catch (e) { results.push(e.name === 'IndexSizeError'); } \
                 results.every(function(v) { return v === true; })"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-314: `new DocumentFragment()` is owned by the document and holds
// inserted children (`firstChild` compares === with the appended node).
#[test]
fn document_fragment_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var f = new DocumentFragment(); \
                 var t = document.createTextNode(''); \
                 f.appendChild(t); \
                 f.ownerDocument === document && f.firstChild === t"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-314: node/element interface globals resolve so `instanceof` no longer
// throws `X is not defined`.
#[test]
fn dom_interface_globals_defined() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "['Node','Element','CharacterData','Attr','Document','DocumentType', \
                  'ProcessingInstruction','HTMLElement','HTMLDivElement','HTMLInputElement'] \
                 .every(function(n) { return typeof globalThis[n] === 'function'; }) && \
                 (HTMLDivElement.prototype instanceof HTMLElement) && \
                 (HTMLElement.prototype instanceof Element) && \
                 (Element.prototype instanceof Node)"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-321: `document.doctype` is a DocumentType node reflecting `<!doctype …>`,
// referentially identical to the doctype child in `document.childNodes`.
#[test]
fn document_doctype_is_document_type() {
    let mut doc = Document::new();
    let dt = doc.create_doctype("html", "", "");
    let html = doc.create_element(QualName::html("html"));
    doc.append_child(doc.root(), dt);
    doc.append_child(doc.root(), html);
    let rt = v8_runtime_with_dom(Arc::new(Mutex::new(doc)));
    let r = rt
        .eval(
            "document.doctype instanceof DocumentType && \
                     document.doctype.name === 'html' && \
                     document.doctype.nodeType === 10 && \
                     document.doctype === document.childNodes[0]",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-321: a page without a doctype reports `document.doctype === null`.
#[test]
fn document_doctype_null_when_absent() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.doctype === null").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-321: `new Document()` is constructible and detached — createElement /
// appendChild work and a fresh document has a null doctype.
#[test]
fn new_document_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var nd = new Document(); \
                     nd.appendChild(nd.createElement('html')); \
                     (nd instanceof Document) && nd.nodeType === 9 && \
                     nd.doctype === null && nd.documentElement !== null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-324: `document.implementation` is a cached DOMImplementation — same
// object on repeated access, distinct per document (WPT
// `Document-implementation.html`).
#[test]
fn document_implementation_is_cached_dom_implementation() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var impl = document.implementation; \
                     (impl instanceof DOMImplementation) && \
                     (document.implementation === impl) && \
                     (document.implementation.createHTMLDocument().implementation !== impl)",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-324: `createDocumentType` builds a detached DocumentType whose
// ownerDocument is the document owning the implementation, immediately
// (not just after insertion) — WPT `DOMImplementation-createDocumentType.html`.
#[test]
fn create_document_type_reflects_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var dt = document.implementation.createDocumentType('test:root', '1234', 'sys'); \
                     dt.name === 'test:root' && dt.nodeName === 'test:root' && \
                     dt.publicId === '1234' && dt.systemId === 'sys' && \
                     dt.nodeValue === null && dt.ownerDocument === document && \
                     dt instanceof DocumentType && dt.nodeType === 10",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-324: an invalid XML Name (e.g. containing a space) throws
// InvalidCharacterError, matching `document.createProcessingInstruction`'s
// validation (same `_lumen_is_xml_name` helper).
#[test]
fn create_document_type_rejects_invalid_name() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "try { document.implementation.createDocumentType('a b', '', ''); false; } \
                     catch (e) { e.name === 'InvalidCharacterError'; }",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-324: `createHTMLDocument(title)` builds the standard html>head,body
// skeleton with a `<!doctype html>` and, when a title is given, a <title>
// text child — WPT `DOMImplementation-createHTMLDocument.html`. An explicit
// `undefined` title argument is treated as omitted (no <title> element).
#[test]
fn create_html_document_builds_skeleton() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var doc = document.implementation.createHTMLDocument('hi'); \
                     var noTitle = document.implementation.createHTMLDocument(undefined); \
                     (doc instanceof Document) && doc.childNodes.length === 2 && \
                     doc.doctype.name === 'html' && doc.doctype.publicId === '' && \
                     doc.documentElement.tagName === 'HTML' && \
                     doc.documentElement.firstChild.tagName === 'HEAD' && \
                     doc.documentElement.firstChild.firstChild.tagName === 'TITLE' && \
                     doc.documentElement.firstChild.firstChild.firstChild.data === 'hi' && \
                     doc.documentElement.lastChild.tagName === 'BODY' && \
                     noTitle.documentElement.firstChild.firstChild === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-324: `createDocument(namespace, qualifiedName, doctype)` returns an
// XMLDocument with the given doctype and a namespaced document element
// (or no document element when qualifiedName is empty) — WPT
// `DOMImplementation-createDocument.html`.
// BUG-367: the document element is in the SVG namespace, so its `tagName`
// is the un-folded qualified name `svg` — the previous `SVG` expectation
// encoded the unconditional upper-casing this bug removed.
#[test]
fn create_document_builds_xml_document() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var dt = document.implementation.createDocumentType('svg', '', ''); \
                     var doc = document.implementation.createDocument('http://www.w3.org/2000/svg', 'svg', dt); \
                     var empty = document.implementation.createDocument(null, '', null); \
                     (Object.getPrototypeOf(doc) === XMLDocument.prototype) && \
                     doc.nodeType === 9 && doc.contentType === 'image/svg+xml' && \
                     doc.doctype === dt && doc.documentElement.tagName === 'svg' && \
                     doc.documentElement.localName === 'svg' && \
                     doc.childNodes.length === 2 && \
                     empty.documentElement === null && empty.contentType === 'application/xml'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-324: `createDocument` requires at least 2 arguments (namespace,
// qualifiedName) — a WebIDL required-argument TypeError, not a
// DOMException.
#[test]
fn create_document_requires_two_arguments() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "['', undefined].every(function(v) { \
                        try { \
                            if (v === undefined) { document.implementation.createDocument(); } \
                            else { document.implementation.createDocument(v); } \
                            return false; \
                        } catch (e) { return e instanceof TypeError; } \
                     })",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// BUG-324: `hasFeature` is a legacy no-op — always true regardless of
// arguments (WPT `DOMImplementation-hasFeature.html`).
#[test]
fn has_feature_always_true() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "document.implementation.hasFeature() === true && \
                     document.implementation.hasFeature('bogus', '99.0') === true",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
